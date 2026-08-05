# neo4j-graph-viz

[![build](https://github.com/jean1880/neo4j-graph-viz/actions/workflows/build.yml/badge.svg)](https://github.com/jean1880/neo4j-graph-viz/actions/workflows/build.yml)
[![license: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-stable-orange.svg?logo=rust&logoColor=white)](Cargo.toml)
[![Vue 3](https://img.shields.io/badge/vue-3-42b883.svg?logo=vue.js&logoColor=white)](frontend/package.json)
[![Neo4j 5+](https://img.shields.io/badge/neo4j-5%2B-008cc1.svg?logo=neo4j&logoColor=white)](https://neo4j.com/)

An Obsidian-style force-directed viewer for a Neo4j graph — a persistent, searchable "map of
everything" you can explore, filter, and search.

It fills the gap left by Neo4j Browser (draws query results only, so you never see the whole
shape) and Bloom (Enterprise-only). Point it at a database and it renders every node and
relationship at once.

A **Rust/axum backend** reads the graph over Bolt and serves it as `GET /api/graph`; a
**Vue 3 SPA** renders it with [vasturiano/force-graph](https://github.com/vasturiano/force-graph).
The browser only ever receives that JSON — never a Bolt endpoint, never a credential.

## Quick start

```bash
cp .env.example .env    # fill in NEO4J_HOST and NEO4J_PASSWORD
./view.sh               # builds both halves, serves http://localhost:8901, opens a browser
```

Requires Rust (stable), Node 22+, and `git` on `PATH`.

## Controls

- **scroll** zoom · **drag** pan · **click** focus a node · **right-click** fit to view
- **hover** a node to highlight it and its neighbours (dims the rest)
- **search box** finds and centres a node by name
- **legend** — click a type to show/hide it; the graph re-fits to what's left

## Configuration

Everything is runtime environment config; nothing is compiled in. `.env.example` documents the
full set — the essentials:

| Variable | Default | Purpose |
| --- | --- | --- |
| `NEO4J_HOST` | *(required)* | Full URI — `bolt://`, `neo4j://`, or `neo4j+s://` for TLS/Aura |
| `NEO4J_PASSWORD` | *(required)* | Never logged, never sent to the browser |
| `NEO4J_USER` / `NEO4J_DATABASE` | `neo4j` / `neo4j` | User, and which database on a multi-database server |
| `GRAPH_NODE_LABELS` / `GRAPH_REL_TYPES` | *(empty — everything)* | Allow-lists narrowing what gets fetched |
| `GRAPH_NAME_KEYS` | `name,title,displayName,id,…` | Property keys tried in order for a node's display name |
| `GRAPH_SKIP_PROPS` | `embedding,vector` | Property keys withheld from the browser |
| `GRAPH_MAX_NODES` / `GRAPH_MAX_RELS` | `10000` / `20000` | Fetch caps; a truncated fetch logs a warning |
| `GRAPH_WRAPPER_LABELS` | *(empty)* | Labels that namespace rather than describe a node |
| `GRAPH_CACHE_TTL_SECS` | `3600` | How long a fetched graph is reused; `0` disables caching |
| `VITE_APP_TITLE` | `Graph Viewer` | Build-time title |

Out of the box it fetches **every node and relationship** and infers the rest: no label list, no
relationship types, no schema file. Point it at a database and it renders. The variables above
exist for when you want to narrow that — `GRAPH_NODE_LABELS` and `GRAPH_REL_TYPES` filter in
Cypher, so on a large graph they cut both the payload and the query cost.

Requires **Neo4j 5+** (the queries use `elementId()`).

Node colour is a hash of the label, so a schema the viewer has never seen still renders with
stable, distinct colours and a complete legend — no configuration required. Node size is degree.

`GRAPH_WRAPPER_LABELS` covers the case where every node in a subgraph carries a shared base
label (say `Base`) alongside its real type. Listing it here keeps the display label meaningful
and folds those nodes into one `group`.

## Security

**`/api/graph` is unauthenticated and returns every node, relationship, and property that the
fetch options admit.** That is the right default for a graph you'd publish and the wrong one for
anything else. Before exposing this beyond localhost:

- put an authenticating reverse proxy in front of it;
- add any sensitive property key to `GRAPH_SKIP_PROPS`;
- use a read-only Neo4j user.

Credentials stay server-side by construction — the SPA never talks Bolt, and the API response
carries no endpoint or credential.

## Running as a container

```bash
docker build -t neo4j-graph-viz .
docker run --rm --env-file .env -e BIND=0.0.0.0 -e PORT=8080 \
  -p 127.0.0.1:8902:8080 neo4j-graph-viz
```

The image bakes only application code, runs as an unprivileged user (`uid 10001`), and serves
both halves compressed. CI publishes to GHCR on push to the default branch.

### Behind a reverse proxy

Asset paths are relative and the API is resolved against the document, so the **same image**
works at the site root or under any prefix — no rebuild, no base-path env var:

```nginx
location /tools/graph/ {
    proxy_pass http://graph-viz:8080/;   # trailing slash strips the prefix
}
```

## Development

`make help` lists everything. The common flow:

```bash
make gate               # fmt + clippy -D warnings + test + SPA build/typecheck (what CI runs)
make verify             # build the image, run it, smoke-test it, tear down
make verify-published   # same, but against the image CI actually published
make smoke URL=https://graph.example.com   # smoke-test any deployed instance
```

`smoke.sh` is read-only and standalone — it checks health, that the SPA and its bundle resolve
(the first thing a path-prefixed deployment breaks), that responses are compressed, and that
the payload describes a usable graph rather than an empty or mislabelled one. Use it as a
post-deploy gate.

Config comes from `ENV_FILE` (default `.env`); only the app's own keys are forwarded into the
container, never the whole file:

```bash
ENV_FILE=~/some-other.env make verify-published
```

`AGENTS.md` documents the architecture, invariants, and constraints in more depth — worth
reading before a substantial change.

## Licence

MIT — see [LICENSE](LICENSE).
