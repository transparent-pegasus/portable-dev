# Portable Dev

@CLAUDE.local.md

## Overview

A web application that lists the dev servers `process-compose` supervises on this host and
lets you open one — from a phone, over whatever tunnel or VPN already reaches the host.

This repo owns the way in: the Cloudflare Tunnel, the `lo` alias, the nftables rule, and
`PORT_RANGE`. Exactly one repository may own those — a Cloudflare account has one device
profile, one Zero Trust organization and one WARP enrollment application, and two managers
flip-flop. A managed repo stands its own tunnel targets down; see `MANAGED-REPO.md`.

The app itself never proxies. It supervises dev servers, assigns their ports, and links to
them; servers answer for themselves on their own port.

## Tech Stack

- Infra: Cloudflare Access, Cloudflare Tunnel, Terraform, cloudflared, systemd, nftables
- Application: Rust, TypeScript, Astro
- Tools: Cargo, Aube, Vitest, Biome

## Development Rules

1. Follow `.claude/skills/artful-simplicity/SKILL.md` to keep the implementation to the bare minimum and pursue art-level simplicity.
2. `process-compose.yaml` is the single registry.
3. Every port used here must fall inside `PORT_RANGE`.
