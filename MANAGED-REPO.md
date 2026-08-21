# Being managed by Portable Dev

Audience: a repository whose dev server should appear in Portable Dev's list and be
reachable from a phone. Written to be copied into that repository.

Portable Dev supervises dev servers with `process-compose` and publishes an index page.
It is not a proxy and not a framework. The whole coupling is two environment variables
and one entry in a YAML file.

## 0. The shape in one line

```
phone ──> tunnel / VPN ──> $BIND_ADDR:<your port> ──> your dev server
```

Portable Dev owns everything left of the last arrow. You own the process on the right.
The port number is the only thing that distinguishes it from every other repo's server.

## 1. What Portable Dev does

- Starts, stops, and restarts your dev server, from the `command` you supply.
- Passes it `BIND_ADDR` and `PORT` in the environment.
- Assigns your port, once, and keeps the record of which repo holds which.
- Lists your repo on its index page with your favicon, and links to it.
- Probes your port with a 400 ms TCP connect for the up/down dot.

## 2. What Portable Dev never does

- Proxy, rewrite, or terminate TLS for you. Your server answers on its own port, directly.
- Discover your port. It reads exactly what you declare.
- Health-check over HTTP, parse your logs, or restart you on a failing response.
- Modify anything in your repository.
- Manage a tunnel, firewall rule, or DNS record on your behalf.

## 3. What your repo must do

**Bind `$BIND_ADDR`, on `$PORT`, from the environment.** Not `127.0.0.1` — that is
unreachable through a tunnel. Not `0.0.0.0` — that publishes your dev server to every
network the host is attached to. Most dev servers need an explicit flag:

```sh
vite dev --host "$BIND_ADDR" --port "$PORT" --strictPort
astro dev --host "$BIND_ADDR" --port "$PORT"
next dev --hostname "$BIND_ADDR" --port "$PORT"
python -m http.server "$PORT" --bind "$BIND_ADDR"
```

Use the strict-port flag where one exists. Silently falling forward to the next free port
lands you outside the covered range — see §6, where that is a security failure and not an
inconvenience.

**Stay in the foreground.** `process-compose` supervises the process it starts. A command
that daemonizes and returns exits immediately as far as the supervisor is concerned, and
the real server is left unmanaged — it will not be stopped, restarted, or reported. Some
tools daemonize by default and offer a separate `stop` subcommand; if yours does, find its
foreground flag. Some only do it when stdout is not a terminal, which is exactly how a
supervisor runs you — so test the command the way it will actually be started, not from
your shell.

**Register the process that serves your whole app, not just its front end.** If a
backend serves both an API and the built UI, that backend is what belongs in the registry.
A bundler's dev server started on its own returns the page and 404s every request the page
makes — the UI renders, and nothing in it works. Symptoms point at the tunnel; the cause is
that half the app was never started.

**Log to stdout and stderr, and exit non-zero on failure.** That is the whole interface
for "did it work". Nothing reads your log files.

**Set your own host allowlist.** Portable Dev's `Host`/`Origin` guard protects its own
socket and nothing else. DNS rebinding against `$BIND_ADDR:<your port>` is yours to close,
in your framework's setting — Vite `server.allowedHosts`, Next `allowedDevOrigins`, Django
`ALLOWED_HOSTS`, Rails `config.hosts`. Check the default before assuming it is safe.

**Do not bind a port other than the one you were given**, and do not add rules to the
gateway's nftables table. §6 explains what breaks.

## 4. Registering

One entry in Portable Dev's `process-compose.yaml`. `PORT` in `environment` is what makes
the process appear in the UI; a process without one is supervised but not listed.

```yaml
processes:
  my-repo:
    description: What this serves      # subtitle in the UI; optional
    working_dir: ../my-repo
    command: aube run dev -- --host $BIND_ADDR --port $PORT --strictPort
    environment:
      - PORT=8789                      # assigned by Portable Dev, not by you
```

`working_dir` is relative to Portable Dev's checkout. `BIND_ADDR` comes from the file's
global `environment`; `PORT` is per-process.

The `command` runs in your repo, with your tooling — the examples here use `aube`, which is
what this host standardizes on. `aube run <script> -- <flags>` forwards flags to the script;
`aube exec <binary> -- <flags>` runs a binary from `node_modules/.bin` directly, which is the
form to reach for when a script does not take the flags you need.

## 5. Your icon

With no declaration, these are tried under `working_dir`, in order:

```
public/favicon.svg   public/favicon.png   public/favicon.ico
web/public/favicon.svg   web/public/favicon.png
static/favicon.ico
favicon.svg   favicon.png   favicon.ico
```

Ship one of those and you get an icon for free. To point somewhere else, add
`ICON=path/to/icon.svg` to `environment`, relative to `working_dir`. Files are resolved
once at startup and served from `/api/icon/<name>`; nothing else in your repo is readable
through Portable Dev. If the file is missing, the status dot stands in.

## 6. Security — the part not to skip

**The covered port range is one shared authorization boundary.** Every port in it sits
behind the same gateway rule and the same identity check. Whatever you start there is
exactly as exposed as everything else in the range — no more, and no less.

**Outside the range, on the same address, is open.** Two failures at once: the gateway's
drop rule does not cover the port, so Linux's weak host model lets a LAN peer with a static
route reach it; and no access policy claims the destination. This is why the strict-port
flag matters, and why Portable Dev refuses to start when a registered port falls outside
the range rather than listing a link to it.

**A dev server is not hardened software.** Arbitrary file reads and code execution are
ordinary features of dev tooling — module servers, REPL endpoints, source maps over the
whole working tree. Putting yours here is a deliberate decision. The boundary is one
identity on one device; it is not a sandbox, and nothing in this document narrows it.

**`http://<address>:<port>` is not a browser secure context.** Only `localhost`,
`127.0.0.0/8`, and `::1` qualify. Service workers, `crypto.subtle`, the clipboard API and
`getUserMedia` are unavailable to your pages. Plan around it, or terminate TLS yourself.

**HMR works.** The route is L4, so WebSocket upgrades pass through unchanged as long as the
client connects to the same `host:port` it loaded from. If your bundler hard-codes an HMR
host, point it at `$BIND_ADDR`.

## 7. Verifying

```sh
# your server is on the alias, not on loopback and not on the wildcard
ss -ltn "src $BIND_ADDR"

# from another machine on the LAN, every covered port must time out or refuse
nc -vz -w 3 "$BIND_ADDR" <your port>   # expected: no route / timeout, never a banner
```

From the enrolled device, your repo answers on its own port, and Portable Dev's index
shows it with a green dot.
