# AGENTS.md

Context for AI coding agents working in this repo. Human-readable too — if it disagrees with
the code, the code wins; fix this file.

## What this is

A force-directed viewer for a Neo4j graph. A **Rust/axum backend** reads the graph over Bolt
and serves it as `GET /api/graph`; a **Vue 3 SPA** renders it with
[vasturiano/force-graph](https://github.com/vasturiano/force-graph).

It exists because Neo4j Browser only draws query-result subgraphs and Bloom is Enterprise-only.
This renders a persistent, searchable "map of everything": pan/zoom, hover to highlight a
node's neighbourhood, click for a property panel, filter by label, search by name.

## Layout

| Path | What lives there |
| --- | --- |
| `src/main.rs` | Service wiring: config from env, Neo4j connect, router, in-memory cache, bind |
| `src/graph.rs` | Bolt fetch + the `{nodes, links}` transform, and `FetchOptions` (all shaping config) |
| `frontend/src/composables/useGraph.ts` | Singleton store: graph data, selection, hover, filters |
| `frontend/src/components/` | `GraphCanvas` (force-graph host), `HudPanel`, `TypeLegend`, `NodeDetail` |
| `Dockerfile` | 3-stage build: SPA (node) → binary (rust) → slim runtime serving both |
| `view.sh` | Local dev entry point (builds both, binds loopback, opens a browser) |

## Configuration — all runtime, none compiled in

**No endpoint, credential, or private identifier may ever be committed or baked into the
image.** `main.rs` uses `required_env` with no compiled default; if you find yourself adding a
fallback host, that's the bug. See `.env.example` for the full list.

Required: `NEO4J_HOST` (full URI — `bolt://`, `neo4j://`, or `neo4j+s://` for TLS/Aura),
`NEO4J_PASSWORD`. Optional: `NEO4J_USER`, `NEO4J_DATABASE`, `BIND`, `PORT`.

Everything the fetch does is `FetchOptions` (`src/graph.rs`), read from env once at startup.
Nothing about the target schema is compiled in:

- `NEO4J_DATABASE` — which database, for multi-database servers.
- `GRAPH_NODE_LABELS` / `GRAPH_REL_TYPES` — allow-lists narrowing what is fetched. Empty by
  default (the whole graph). Applied in Cypher, not after, so they cut query cost too. They are
  passed as query **parameters**, never string-spliced into the Cypher — keep it that way.
- `GRAPH_NAME_KEYS` — property keys tried in order for a node's display name.
- `GRAPH_WRAPPER_LABELS` — labels that namespace a node rather than describe it. Dropped when
  choosing the display label; the first match becomes the node's `group`. Empty by default,
  which is why the viewer works against an unknown schema with no configuration.
- `GRAPH_SKIP_PROPS` — property keys never sent to the browser (default `embedding,vector`).
- `GRAPH_MAX_NODES` / `GRAPH_MAX_RELS` / `GRAPH_MAX_PROP_CHARS` — fetch caps.
- `GRAPH_CACHE_TTL_SECS` — how long a fetched graph is reused (`0` disables the cache).

The queries use `elementId()`, so **Neo4j 5+** is required; on 4.x that function does not exist.

Frontend build-time: `VITE_APP_TITLE` (tab + HUD heading), `VITE_API_TARGET` (dev proxy).

## Build, run, test

```bash
cp .env.example .env         # then fill in NEO4J_HOST / NEO4J_PASSWORD
./view.sh                    # build both, serve on 127.0.0.1:8901, open a browser
make gate                    # fmt + clippy -D warnings + cargo test + SPA build + typecheck
make verify                  # build image, run it, smoke-test it, tear down
make verify-published        # same, against the image CI actually published
make smoke URL=https://…     # smoke-test a deployed instance
```

`make gate` is what CI runs. Run it before calling any change done — `clippy` is `-D warnings`,
so a warning is a build failure. `make verify` is the stronger check: it proves the *container*
works, which `gate` cannot.

Two Makefile conventions worth not breaking:

- **Comments go on their own line.** Make captures trailing whitespace before an inline `#`, so
  `PORT ?= 8902  # note` silently yields `"8902  "` and corrupts anything concatenating it.
- **Only `APP_ENV` keys are forwarded into the container**, never the whole env file — the file
  may hold unrelated secrets, and `--env-file` would inject every one of them.

## Deploy

CI (`.github/workflows/build.yml`) gates then builds on push to the default branch, publishing
to GHCR (and to Docker Hub only if a `DOCKER_HUB_USERNAME` secret exists, so forks build green).
`linux/amd64` only — add platforms to the `platforms:` list if you need them.

Run the image with the environment injected by your orchestrator. The container defaults to
`BIND=0.0.0.0 PORT=8080`; expose it through a reverse proxy rather than directly.

## Constraints an agent must not break

- **Secrets stay server-side.** The browser receives only `/api/graph` JSON — never a Bolt
  endpoint or a credential. Anything that would send connection details to the client is wrong,
  including "helpful" error messages: `main.rs` deliberately reports config failures to the log,
  not the response body.
- **`/api/graph` is unauthenticated and returns everything the fetch options admit.** There is
  no authorization layer here by design — put an authenticating proxy in front of it on any
  network where the graph contents are not public, and use `GRAPH_SKIP_PROPS` for sensitive
  properties. If you add auth, it belongs in front of the router in `main.rs`, and this line
  needs updating.
- **Queries stay `LIMIT`-bounded.** `fetch()` returns the whole graph; an unbounded `MATCH (n)`
  will exhaust server memory and hand the browser a payload no force simulation can lay out.
- **Nothing schema-specific gets hardcoded.** New labels auto-assign a hashed hue
  (`frontend/src/color.ts`) and the legend picks them up. Adding a label should never require a
  code change — only the id/name/degree contract in `graph.rs` justifies one.
- **Nothing deployment-specific gets hardcoded either.** Vite's `base` is `'./'` and the API is
  resolved against `document.baseURI` (`useGraph.ts`), so one image runs at the site root or
  under any proxy prefix. A root-absolute `/api/graph` or `/assets/…` breaks every path-prefixed
  deployment — don't reintroduce one.
- **Builder and runtime images stay on the same Debian release.** A binary linked against a
  newer glibc than the runtime fails when the *container starts*, not when the image builds, so
  CI goes green and the deploy dies.
- **Keep the SPA reactivity shape.** `useGraph`'s `data` is a `shallowRef` over a `markRaw`'d
  payload because d3-force mutates every node ~60×/s; making it deeply reactive is a large,
  silent performance regression. Same reason `linksByNode` is precomputed rather than scanned.
- **`CLAUDE.md`, `.claude/`, and `.env` are git-ignored** and may contain deployment-specific
  detail. Never move their content into a tracked file, and never commit them.

## Shared dependencies

Two libraries are consumed by git tag from public repos (no registry, no auth):

- **`nuvek-web`** (`Cargo.toml`) — axum SPA-serving, config, telemetry, and health scaffold.
  Provides `config::required_env` / `env_or`, `serve::attach_spa`, `health::routes` (`/healthz`).
- **`@nuvek/ui`** (`frontend/package.json`) — CSS design tokens + Vue components. All styling
  derives from its tokens; prefer a token over a hardcoded colour or spacing value.

Both need `git` present in the build image — that's why the Dockerfile installs it in the node
and rust stages. Changing either dependency's tag is a deliberate, reviewable one-line diff.
