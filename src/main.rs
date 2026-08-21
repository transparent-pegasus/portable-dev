//! Landing page + status API for the dev servers process-compose supervises.
//!
//! `process-compose.yaml` is the single registry: each process declares its own
//! `PORT` in `environment`, and that is what this server lists. See the README
//! for why every port must sit inside `PORT_RANGE`.

use std::{collections::BTreeMap, net::SocketAddr, sync::Arc, time::Duration};

use axum::{
    extract::{Request, State},
    http::{header::HOST, header::ORIGIN, HeaderValue, StatusCode},
    middleware::{from_fn_with_state, Next},
    response::Response,
    routing::get,
    Json, Router,
};
use serde::{Deserialize, Serialize};
use tokio::{net::TcpStream, task::JoinSet, time::timeout};
use tower_http::services::ServeDir;

const PROBE_TIMEOUT: Duration = Duration::from_millis(400);

#[derive(Deserialize)]
struct Compose {
    #[serde(default)]
    processes: BTreeMap<String, Process>,
}

#[derive(Deserialize)]
struct Process {
    description: Option<String>,
    #[serde(default)]
    environment: Vec<String>,
}

#[derive(Serialize, Clone)]
struct Server {
    name: String,
    description: String,
    port: u16,
    url: String,
}

#[derive(Serialize)]
struct Status {
    #[serde(flatten)]
    server: Server,
    up: bool,
}

#[derive(Clone)]
struct App {
    servers: Arc<Vec<Server>>,
    hosts: Arc<Vec<String>>,
    bind_addr: Arc<str>,
}

/// Ports come from each process's own `PORT=` entry; a process without one is
/// not a dev server we can link to, so it is left out of the list.
fn read_registry(path: &str, bind_addr: &str) -> Result<Vec<Server>, String> {
    let text = std::fs::read_to_string(path).map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => {
            format!("{path} not found. Copy the example: `cp {path}.example {path}`")
        }
        _ => format!("{path}: {e}"),
    })?;
    let compose: Compose = serde_yaml_ng::from_str(&text).map_err(|e| format!("{path}: {e}"))?;

    Ok(compose
        .processes
        .into_iter()
        .filter_map(|(name, process)| {
            let port: u16 = process
                .environment
                .iter()
                .find_map(|entry| entry.strip_prefix("PORT="))?
                .trim()
                .parse()
                .ok()?;
            Some(Server {
                url: format!("http://{bind_addr}:{port}/"),
                description: process.description.unwrap_or_default(),
                name,
                port,
            })
        })
        .collect())
}

/// `8787-8799`, or a bare `8787`. Blank means "no range declared".
fn parse_range(spec: &str) -> Result<(u16, u16), String> {
    let (low, high) = spec.split_once('-').unwrap_or((spec, spec));
    let bound = |text: &str| {
        text.trim()
            .parse::<u16>()
            .map_err(|e| format!("PORT_RANGE `{spec}`: {e}"))
    };
    let (low, high) = (bound(low)?, bound(high)?);
    if low > high {
        return Err(format!(
            "PORT_RANGE `{spec}`: low bound is above the high bound"
        ));
    }
    Ok((low, high))
}

/// `PORT_RANGE` is the set of ports the gateway's firewall rule and access policy
/// actually cover. A port outside it is reachable from the LAN and authorized by
/// nothing — a security failure, so refuse to start rather than serve a link to it.
fn check_ports(range: &str, ports: &[(String, u16)]) -> Result<(), String> {
    if range.trim().is_empty() {
        return Ok(());
    }
    let (low, high) = parse_range(range.trim())?;
    let stray: Vec<String> = ports
        .iter()
        .filter(|(_, port)| *port < low || *port > high)
        .map(|(name, port)| format!("{name} ({port})"))
        .collect();
    if stray.is_empty() {
        return Ok(());
    }
    Err(format!(
        "outside PORT_RANGE {low}-{high}: {}\nThose ports are exposed to the LAN and authorized by nothing. Move them into the range, or widen PORT_RANGE on the gateway first and here second.",
        stray.join(", ")
    ))
}

/// Closes DNS rebinding against this port. A gateway's own guards cover its socket
/// only — every server behind it answers for itself.
async fn guard_host(
    State(app): State<App>,
    request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let text = |value: Option<&HeaderValue>| value.and_then(|v| v.to_str().ok()).map(str::to_owned);
    // Scoped so no borrow of `request` is alive across the await below.
    let (host, origin) = {
        let headers = request.headers();
        (text(headers.get(HOST)), text(headers.get(ORIGIN)))
    };
    let allowed = |value: &str| {
        let value = value
            .trim_start_matches("http://")
            .trim_start_matches("https://");
        app.hosts.iter().any(|host| host == value)
    };

    if !host.as_deref().is_some_and(&allowed) {
        return Err(StatusCode::FORBIDDEN);
    }
    if origin.as_deref().is_some_and(|origin| !allowed(origin)) {
        return Err(StatusCode::FORBIDDEN);
    }
    Ok(next.run(request).await)
}

async fn list_servers(State(app): State<App>) -> Json<Vec<Status>> {
    let mut probes = JoinSet::new();
    for (index, server) in app.servers.iter().cloned().enumerate() {
        let target = format!("{}:{}", app.bind_addr, server.port);
        probes.spawn(async move {
            let up = matches!(
                timeout(PROBE_TIMEOUT, TcpStream::connect(&target)).await,
                Ok(Ok(_))
            );
            (index, Status { server, up })
        });
    }

    let mut statuses: Vec<(usize, Status)> = Vec::with_capacity(app.servers.len());
    while let Some(Ok(result)) = probes.join_next().await {
        statuses.push(result);
    }
    statuses.sort_by_key(|(index, _)| *index);
    Json(statuses.into_iter().map(|(_, status)| status).collect())
}

#[tokio::main]
async fn main() {
    if let Err(message) = run().await {
        eprintln!("portable-dev: {message}");
        std::process::exit(1);
    }
}

async fn run() -> Result<(), String> {
    let env = |key: &str, fallback: &str| std::env::var(key).unwrap_or_else(|_| fallback.into());
    let bind_addr = env("BIND_ADDR", "10.99.99.1");
    let port: u16 = env("PORT", "8788")
        .parse()
        .map_err(|e| format!("PORT: {e}"))?;
    let compose_file = env("COMPOSE_FILE", "process-compose.yaml");
    let web_dir = env("WEB_DIR", "web/dist");

    let servers = read_registry(&compose_file, &bind_addr)?;

    let mut ports = vec![("portable-dev".to_owned(), port)];
    ports.extend(
        servers
            .iter()
            .map(|server| (server.name.clone(), server.port)),
    );
    check_ports(&env("PORT_RANGE", ""), &ports)?;

    println!(
        "portable-dev: {} server(s) from {compose_file}",
        servers.len()
    );

    let app = App {
        servers: Arc::new(servers),
        hosts: Arc::new(
            [bind_addr.as_str(), "localhost", "127.0.0.1"]
                .iter()
                .map(|host| format!("{host}:{port}"))
                .collect(),
        ),
        bind_addr: bind_addr.as_str().into(),
    };

    let router = Router::new()
        .route("/api/servers", get(list_servers))
        .fallback_service(ServeDir::new(&web_dir).append_index_html_on_directories(true))
        .layer(from_fn_with_state(app.clone(), guard_host))
        .with_state(app);

    let socket: SocketAddr = format!("{bind_addr}:{port}")
        .parse()
        .map_err(|e| format!("BIND_ADDR `{bind_addr}`: {e}"))?;
    let listener = tokio::net::TcpListener::bind(socket).await.map_err(|e| {
        let hint = match e.kind() {
            std::io::ErrorKind::AddrNotAvailable => format!(
                "\nNothing owns {bind_addr} yet: add it with `ip addr add {bind_addr}/32 dev lo`, or start whatever service creates it."
            ),
            _ => String::new(),
        };
        format!("bind {socket}: {e}{hint}")
    })?;
    println!("portable-dev: listening on http://{socket}/");
    axum::serve(listener, router)
        .await
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ports_outside_the_declared_range_are_refused() {
        let inside = [("api".to_owned(), 8790), ("docs".to_owned(), 8799)];
        let straddling = [("api".to_owned(), 8790), ("stray".to_owned(), 9000)];

        assert!(check_ports("8787-8799", &inside).is_ok());
        assert!(
            check_ports("", &straddling).is_ok(),
            "no range declared means no check"
        );
        assert!(
            check_ports("8788", &[("self".to_owned(), 8788)]).is_ok(),
            "a bare port is a range of one"
        );

        let error = check_ports("8787-8799", &straddling).unwrap_err();
        assert!(error.contains("stray (9000)"), "{error}");
        assert!(!error.contains("api"), "{error}");
        assert!(
            check_ports("8799-8787", &inside).is_err(),
            "reversed bounds are a typo, not a range"
        );
    }

    #[test]
    fn registry_takes_ports_from_process_environment() {
        let yaml = r#"
version: "0.5"
processes:
  api:
    description: API dev server
    command: npm run dev
    environment: ["FOO=bar", "PORT=8790"]
  docs:
    command: mkdocs serve
    environment: ["PORT=8791"]
  worker:
    command: cargo run
    environment: ["RUST_LOG=debug"]
"#;
        let path = std::env::temp_dir().join("portable-dev-registry-test.yaml");
        std::fs::write(&path, yaml).unwrap();
        let servers = read_registry(path.to_str().unwrap(), "10.99.99.1").unwrap();

        // `worker` declares no PORT, so it is not a linkable dev server.
        assert_eq!(servers.len(), 2);
        assert_eq!(servers[0].name, "api");
        assert_eq!(servers[0].port, 8790);
        assert_eq!(servers[0].description, "API dev server");
        assert_eq!(servers[0].url, "http://10.99.99.1:8790/");
        assert_eq!(servers[1].description, "");
    }
}
