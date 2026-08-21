//! Landing page + status API for the dev servers process-compose supervises.
//!
//! `process-compose.yaml` is the single registry: each process declares its own
//! `PORT` in `environment`, and that is what this server lists. See the README
//! for why every port must sit inside `PORT_RANGE`.

use std::{
    collections::{BTreeMap, HashMap},
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use axum::{
    extract::{Path as UrlPath, Request, State},
    http::{header, header::HOST, header::ORIGIN, HeaderValue, StatusCode},
    middleware::{from_fn_with_state, Next},
    response::IntoResponse,
    response::Response,
    routing::get,
    Json, Router,
};
use serde::{Deserialize, Serialize};
use tokio::{net::TcpStream, task::JoinSet, time::timeout};
use tower_http::services::ServeDir;

const PROBE_TIMEOUT: Duration = Duration::from_millis(400);

/// Looked up under a process's `working_dir` when it declares no `ICON=`.
const ICON_CANDIDATES: &[&str] = &[
    "public/favicon.svg",
    "public/favicon.png",
    "public/favicon.ico",
    "web/public/favicon.svg",
    "web/public/favicon.png",
    "static/favicon.ico",
    "favicon.svg",
    "favicon.png",
    "favicon.ico",
];

#[derive(Deserialize)]
struct Compose {
    #[serde(default)]
    processes: BTreeMap<String, Process>,
}

#[derive(Deserialize)]
struct Process {
    description: Option<String>,
    working_dir: Option<String>,
    #[serde(default)]
    environment: Vec<String>,
}

#[derive(Serialize, Clone)]
struct Server {
    name: String,
    description: String,
    port: u16,
    url: String,
    /// `/api/icon/<name>` when the repo has a favicon, otherwise absent.
    icon: Option<String>,
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
    /// Resolved once at startup. A request names a server, never a path, so
    /// nothing a client sends can reach a file this map does not already hold.
    icons: Arc<HashMap<String, PathBuf>>,
    hosts: Arc<Vec<String>>,
    bind_addr: Arc<str>,
}

/// `ICON=` in a process's environment wins — relative to `working_dir`, or
/// absolute. Failing that, the conventional spots in the repo are tried.
fn find_icon(working_dir: Option<&str>, declared: Option<&str>) -> Option<PathBuf> {
    let root = Path::new(working_dir?);
    if let Some(declared) = declared {
        let path = root.join(declared);
        return path.is_file().then_some(path);
    }
    ICON_CANDIDATES
        .iter()
        .map(|candidate| root.join(candidate))
        .find(|path| path.is_file())
}

fn icon_mime(path: &Path) -> &'static str {
    match path.extension().and_then(|ext| ext.to_str()) {
        Some("svg") => "image/svg+xml",
        Some("ico") => "image/vnd.microsoft.icon",
        Some("jpg" | "jpeg") => "image/jpeg",
        _ => "image/png",
    }
}

/// Ports come from each process's own `PORT=` entry; a process without one is
/// not a dev server we can link to, so it is left out of the list.
type Registry = (Vec<Server>, HashMap<String, PathBuf>);

fn read_registry(path: &str, bind_addr: &str) -> Result<Registry, String> {
    let text = std::fs::read_to_string(path).map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => {
            format!("{path} not found. Copy the example: `cp {path}.example {path}`")
        }
        _ => format!("{path}: {e}"),
    })?;
    let compose: Compose = serde_yaml_ng::from_str(&text).map_err(|e| format!("{path}: {e}"))?;

    let mut servers = Vec::new();
    let mut icons = HashMap::new();
    for (name, process) in compose.processes {
        let entry = |key: &str| {
            process
                .environment
                .iter()
                .find_map(|entry| entry.strip_prefix(key))
                .map(str::trim)
        };
        let Some(Ok(port)) = entry("PORT=").map(str::parse::<u16>) else {
            continue;
        };
        let icon = find_icon(process.working_dir.as_deref(), entry("ICON="));
        if let Some(icon) = icon {
            icons.insert(name.clone(), icon);
        }
        servers.push(Server {
            url: format!("http://{bind_addr}:{port}/"),
            description: process.description.unwrap_or_default(),
            icon: icons
                .contains_key(&name)
                .then(|| format!("/api/icon/{name}")),
            name,
            port,
        });
    }
    Ok((servers, icons))
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

async fn icon(State(app): State<App>, UrlPath(name): UrlPath<String>) -> impl IntoResponse {
    let Some(path) = app.icons.get(&name) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    match tokio::fs::read(path).await {
        Ok(bytes) => ([(header::CONTENT_TYPE, icon_mime(path))], bytes).into_response(),
        Err(_) => StatusCode::NOT_FOUND.into_response(),
    }
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

    let (servers, icons) = read_registry(&compose_file, &bind_addr)?;

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
        icons: Arc::new(icons),
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
        .route("/api/icon/{name}", get(icon))
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
        let (servers, icons) = read_registry(path.to_str().unwrap(), "10.99.99.1").unwrap();

        // `worker` declares no PORT, so it is not a linkable dev server.
        assert_eq!(servers.len(), 2);
        assert_eq!(servers[0].name, "api");
        assert_eq!(servers[0].port, 8790);
        assert_eq!(servers[0].description, "API dev server");
        assert_eq!(servers[0].url, "http://10.99.99.1:8790/");
        assert_eq!(servers[1].description, "");
        // No working_dir, so nothing to find an icon in.
        assert!(icons.is_empty());
        assert!(servers.iter().all(|server| server.icon.is_none()));
    }

    #[test]
    fn icons_come_from_icon_or_a_conventional_path_under_working_dir() {
        let root = std::env::temp_dir().join("portable-dev-icon-test");
        let public = root.join("public");
        std::fs::create_dir_all(&public).unwrap();
        std::fs::write(public.join("favicon.png"), b"png").unwrap();
        std::fs::write(root.join("brand.svg"), b"<svg/>").unwrap();
        let root = root.to_str().unwrap();

        // Conventional path, found without the process declaring anything.
        assert_eq!(
            find_icon(Some(root), None),
            Some(public.join("favicon.png"))
        );
        // An explicit ICON= wins over the conventional path.
        assert_eq!(
            find_icon(Some(root), Some("brand.svg"))
                .unwrap()
                .file_name(),
            Some("brand.svg".as_ref())
        );
        // A declared file that is not there is not silently replaced by the fallback.
        assert_eq!(find_icon(Some(root), Some("missing.png")), None);
        assert_eq!(find_icon(None, Some("brand.svg")), None);

        assert_eq!(icon_mime(Path::new("a/favicon.svg")), "image/svg+xml");
        assert_eq!(icon_mime(Path::new("a/favicon.png")), "image/png");
        assert_eq!(
            icon_mime(Path::new("a/favicon.ico")),
            "image/vnd.microsoft.icon"
        );

        std::fs::remove_dir_all(std::env::temp_dir().join("portable-dev-icon-test")).unwrap();
    }
}
