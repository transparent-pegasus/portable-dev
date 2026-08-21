# Portable Dev

Lists the dev servers `process-compose` supervises on one host, and lets you open any of
them from a phone. List view and tile view, one switch between them. Installable as a PWA.

## What this is

You have a machine running several repos' dev servers and a tunnel or VPN that already
reaches it. Portable Dev is the index page: one tap per server, with a live up/down dot.

```
phone ──> tunnel / VPN ──> 10.99.99.1:8788 ──> Portable Dev
                           10.99.99.1:8789 ──> your dev server
                           10.99.99.1:8790 ──> another one
```

It does not proxy anything, and it manages no tunnel, firewall, or DNS. Servers are
addressed by port; the gateway is yours to run. The whole coupling is two values.

## Quick start

```sh
cp .env.example .env                                # BIND_ADDR and PORT_RANGE
cp process-compose.yaml.example process-compose.yaml
$EDITOR process-compose.yaml                        # register your repos
make build && make run                              # landing page on $BIND_ADDR:$PORT
make up                                             # the dev servers themselves
```

`BIND_ADDR` defaults to `10.99.99.1`, a private address held on `lo`. If nothing owns it
yet, `make run` fails with `EADDRNOTAVAIL`; `ip addr add 10.99.99.1/32 dev lo` creates it
for the current boot. Binding a real interface address works too — read the next section
first if you do.

## The registry

`process-compose.yaml` is the only source of truth. It is git-ignored — the repos on your
machine are yours, not the project's — so start from `process-compose.yaml.example`. A
process appears in the UI when its `environment` declares a `PORT`:

```yaml
processes:
  my-api:
    description: My API           # subtitle in the UI; optional
    working_dir: ../my-api
    command: npm run dev -- --host $BIND_ADDR --port $PORT
    environment:
      - PORT=8789                 # this is what gets listed
      - ICON=public/logo.svg      # optional, relative to working_dir
```

Each entry shows its repo's own favicon. With no `ICON=`, the conventional spots under
`working_dir` are tried — `public/favicon.{svg,png,ico}`, `web/public/favicon.{svg,png}`,
`static/favicon.ico`, then the same names at the root — and the status dot stands in when
nothing is found or the file later disappears. Icons are resolved once at startup and
served from `/api/icon/<name>`, so a request names a server, never a path.

Two rules, one enforced and one you have to keep:

- Every port must be inside `PORT_RANGE`. Portable Dev refuses to start otherwise. The
  range is whatever your gateway's firewall rule and access policy actually cover — a port
  outside it is reachable by anyone who can route to the address and is authorized by
  nothing. That is a security bug, not a broken link. Widen the range on the gateway
  first, here second. Leave `PORT_RANGE` blank to switch the check off.
- Bind `$BIND_ADDR`. `127.0.0.1` is unreachable through a tunnel; `0.0.0.0` publishes the
  server to every network the host is on. Most dev servers need an explicit `--host`.

Set each dev server's own host-check too (Vite's `server.allowedHosts`, Next's
`allowedDevOrigins`, and so on). Portable Dev's Host/Origin guard protects its own socket
and nothing else.

## What you are exposing

A dev server is not hardened software: arbitrary file reads and code execution are ordinary
features of the tooling. Everything inside `PORT_RANGE` is as reachable as Portable Dev
itself, so the range is one shared authorization boundary — put the identity check at the
gateway and keep the boundary small.

## Endpoints

| Path | Response |
|---|---|
| `/` | the UI |
| `/api/servers` | `[{ name, description, port, url, icon, up }]` — `up` is a 400 ms TCP connect |
| `/api/icon/<name>` | that repo's favicon, or 404 |

Requests whose `Host` is not `BIND_ADDR:PORT`, `localhost:PORT`, or `127.0.0.1:PORT` get
403, and so do requests carrying a foreign `Origin`. That is what closes DNS rebinding.

## PWA install

The manifest and service worker are in place, but `http://10.99.99.1:<port>` is not a
browser secure context — only `localhost`, `127.0.0.0/8`, and `::1` are. Service workers do
not register there, so over a plain-HTTP tunnel the page works as an ordinary web page and
iOS "Add to Home Screen" gives a bookmark rather than an installed PWA.

It installs properly wherever the page is a secure context: `localhost` during development,
or behind your own TLS terminator. Nothing else in the app depends on it.

## Configuration

| Variable | Default | Meaning |
|---|---|---|
| `BIND_ADDR` | `10.99.99.1` | address every server in this arrangement binds |
| `PORT` | `8788` | Portable Dev's own port |
| `PORT_RANGE` | *(blank)* | ports the gateway covers; blank disables the check |
| `COMPOSE_FILE` | `process-compose.yaml` | the registry |
| `WEB_DIR` | `web/dist` | built UI |

`make` reads these from `.env`; the binary itself reads only the process environment. Run
it through `make run`, a systemd unit, or process-compose — a bare `./portable-dev` sees no
`.env`, so `PORT_RANGE` is unset and the range check silently does nothing.

## Development

```sh
make check    # test, then lint, then format — the whole gate
make test     # cargo test + vitest
make lint     # clippy -D warnings + biome lint
make format   # cargo fmt --check + biome format
make fix      # the write half: cargo fmt + biome check --write
cd web && npm run dev
```

Each of `test`, `lint`, and `format` covers both sides; `-src` and `-web` suffixes run one
alone (`make test-web`). Every run tees its full output to `.tmp/<target>.log`, so a failure
can be read back without re-running it. `check` never edits the tree — `fix` is the only
target that writes.

The UI fetches `/api/servers`, so `astro dev` alone shows the error state — run `make run`
alongside it, or point Astro's dev server at it.

## License

MIT
