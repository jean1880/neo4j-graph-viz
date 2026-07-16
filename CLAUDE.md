# CLAUDE.md

## What this is

A local, on-demand, Obsidian-style force-directed viewer for the homelab Neo4j
graph (`bolt://localhost:7687`, user `neo4j`). It exists because Neo4j Browser
only draws query-result subgraphs and Bloom is Enterprise-only — this renders a
persistent, searchable "map of everything." Local tool by design: no container, no
Nginx vhost, no GitOps footprint. Run it when you want it.

## Layout

- `view.sh` — the only entry point. Sources `~/.env`, runs `fetch_graph.py`,
  serves the static viewer on `127.0.0.1:${PORT:-8901}`, opens the browser.
- `fetch_graph.py` — `neo4j` driver → `graph.json` (`{nodes, links}` for force-graph).
  Node `label` = most specific Neo4j label (drops the generic `MH` wrapper), `group`
  = `MH` vs `infra`, `deg` = degree. Uses `elementId()` for stable ids.
- `index.html` — vanilla JS + vendored `force-graph.min.js`. No build step, no framework.
- `force-graph.min.js` — vendored [vasturiano/force-graph](https://github.com/vasturiano/force-graph)
  UMD bundle. No runtime CDN so the viewer works offline. Bump by re-downloading.
- `graph.json` — generated snapshot, git-ignored. Never commit it.

## Rules

- **Secrets:** `NEO4J_PASSWORD` comes from the environment only. Never hardcode it,
  never emit it into `graph.json`, never expose a Neo4j endpoint to the browser —
  the browser only ever loads static `graph.json`.
- **Local-only:** keep the server bound to `127.0.0.1`. This is not a homelab service;
  if that ever changes, it needs a backend (creds can't live in the browser) and a
  proper Nginx/TLS vhost committed to `infra-config` — not a hot `docker run`.
- **No framework creep:** it's a single static HTML page on purpose. Prefer adding to
  the existing vanilla JS over introducing a bundler/React.
- **Node-shape drift:** if the graph gains new labels, colours auto-assign (hashed
  hue) and the legend picks them up — no code change needed. Only touch `fetch_graph.py`
  if the id/name/degree contract changes.

Global mandates (verify, minimal-fix, secrets, masking) are in `~/.claude/CLAUDE.md`
— not restated here.
