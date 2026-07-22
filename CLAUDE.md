# CLAUDE.md

## What this is

An Obsidian-style force-directed viewer for the homelab Neo4j graph. A **Rust/axum backend**
reads the graph over Bolt and serves it as `GET /api/graph`; a **Vue 3 SPA** renders it with
[vasturiano/force-graph](https://github.com/vasturiano/force-graph). It exists because Neo4j
Browser only draws query-result subgraphs and Bloom is Enterprise-only — this renders a
persistent, searchable "map of everything."

The Neo4j endpoint + credentials (`NEO4J_HOST`, `NEO4J_PASSWORD`) are **injected at runtime**
(Dockerman env / docker secret / `~/.env`) — **never committed or baked into the image
or binary**. The browser only ever receives the `/api/graph` JSON; it never sees a Bolt endpoint
or a password.

Two ways to run it:
- **Local dev:** `view.sh` builds the SPA + Rust backend and serves both on `127.0.0.1:8901`,
  opening the browser (sources `~/.env` for `NEO4J_HOST`/`NEO4J_PASSWORD`).
- **Deployed service:** a Docker container on the NAS (Dockerman-managed), reverse-proxied by
  the OpenWrt Nginx. Image, Dockerman template, and Nginx vhost are GitOps-managed — no hot
  `docker run`.

## Layout

- `src/` — the Rust/axum backend. `main.rs` (service: router, cache handler, config, bind),
  `graph.rs` (Bolt fetch + `{nodes, links}` transform: `pick_label`/`pick_name`/`clean_props`,
  degree; label drops the generic `MH`/`HL` wrappers; `group` = `MH` vs `infra`; `elementId()`
  ids). `GET /api/graph` is in-memory cached (hourly TTL + `?refresh=1`, stale-if-error).
- `frontend/` — Vite + Vue 3 + TS SPA. `GraphCanvas` (force-graph host), `HudPanel`, `TypeLegend`,
  `NodeDetail`; `useGraph` singleton composable fetches `/api/graph`. Styled entirely on
  `@nuvek/ui` design tokens.
- `Cargo.toml` / `Cargo.lock` — backend deps, incl. `neo4rs` (Bolt) and **`nuvek-web`** (shared
  axum scaffold — serve/config/telemetry/health — a public git-tag dep).
- `frontend/package.json` — **`@nuvek/ui`** (shared design system, public git-tag dep) + `force-graph`.
- `Dockerfile` — multi-stage: build the SPA (node) + the Rust binary (rust), then a slim runtime
  serving both. `view.sh` — local-dev entry point. `graph.json` — legacy generated snapshot, git-ignored.

## Shared libraries

Both are public git repos consumed by tag (no auth, no registry):
- **`nuvek-web`** (`github:jean1880/nuvek-web`, cargo git tag) — the axum SPA-serving + config +
  telemetry + health scaffold. Secret-free by construction.
- **`@nuvek/ui`** (`github:jean1880/nuvek-ui#<tag>`, npm git dep with a `prepare` build) — the
  CSS tokens + Vue component design system.

## Deploy (GitOps)

The service is promoted, not hot-run:
- **Image:** built by CI (`.github/workflows/build.yml`) on push to `master`, dual-pushed to
  **GHCR** and **public Docker Hub** (amd64 only — the NAS is x86). The image bakes only app code
  — **no endpoint, no credential, no private identifier**. Break-glass manual publish: `make push`.
- **Container:** `infra-config` → `roles/nas_docker_templates/templates/my-neo4j-graph-viz.xml.j2`
  (Dockerman template). It injects `NEO4J_HOST` (env) + `NEO4J_PASSWORD` (vault) at runtime — these
  live in `infra-config`/vault, never in this repo or the image. Bump the image via CI, then
  force-update the container so Dockerman pulls the new tag.
- **Nginx route:** `infra-config` → `proxied_services` renders the OpenWrt vhost on the internal
  wildcard domain (wildcard TLS).

## Rules

- **No identifying info in the image or committed code:** the Neo4j endpoint/credentials are
  runtime-injected. Never bake a private IP/host into `src/`, the `Dockerfile`, or `frontend/`;
  `main.rs` uses `required_env("NEO4J_HOST")` with no compiled default.
- **Secrets never reach the browser:** `NEO4J_PASSWORD` stays server-side; the browser only loads
  the `/api/graph` JSON (which carries no endpoint or credential). This is enforced by the API
  boundary — the SPA never talks Bolt.
- **Bind discipline:** `view.sh` binds `127.0.0.1` (local dev); the container binds `0.0.0.0:8080`
  internally, reached only via the NAS host-port mapping + Nginx vhost.
- **This is a full Rust/axum + Vue app on purpose** — it consumes the shared `nuvek-web` +
  `@nuvek/ui` stack. (Supersedes the earlier "single static HTML / no framework creep" identity.)
- **Node-shape drift:** new labels auto-assign a hashed-hue colour and the legend picks them up —
  no code change unless the id/name/degree contract in `graph.rs` changes.

Global mandates (verify, minimal-fix, secrets, masking) are in `~/.claude/CLAUDE.md` — not restated here.
