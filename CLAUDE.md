# Portable Dev

@CLAUDE.local.md

## Overview

A web application that lists the dev servers `process-compose` supervises on this host and
lets you open one — from a phone, over whatever tunnel or VPN already reaches the host.

The app is gateway-agnostic. It supervises dev servers, assigns their ports, and links to
them; it never proxies, and it manages no tunnel, firewall, or DNS of its own. The gateway
is somebody else's repository, and the coupling is two values: `BIND_ADDR` and `PORT_RANGE`.

## Tech Stack

Infra — designed to sit behind a Cloudflare Tunnel with Cloudflare Access in front,
`cloudflared` and its nftables rules laid down by Terraform and run under systemd. None of
that is managed in this repository. `process-compose` supervises what is.

Application — Rust (axum, tokio) serves `web/dist` and `/api/servers` on `BIND_ADDR:PORT`.
TypeScript on Astro builds the static UI, installable as a PWA.

Tools — Cargo, Vitest, Biome.

## Development Rules

1. Follow `.claude/skills/artful-simplicity/SKILL.md` to keep the implementation to the bare minimum and pursue art-level simplicity.
2. `process-compose.yaml` is the single registry.
3. Every port used here must fall inside `PORT_RANGE`.
