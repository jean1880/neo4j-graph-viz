# CLAUDE.md

## What this is

A local, on-demand, Obsidian-style force-directed viewer for the homelab Neo4j
graph (`bolt://localhost:7687`, user `neo4j`). It exists because Neo4j Browser
only draws query-result subgraphs and Bloom is Enterprise-only — this renders a
persistent, searchable "map of everything."

Two ways to run it:
- **Native / on-demand (dev):** `view.sh` (or `make local`) fetches the graph and serves
  it on `127.0.0.1:8901`, opening the browser. Use this while iterating.
- **Deployed service:** a self-contained Docker container on the NAS (Dockerman-managed)
  that refreshes `graph.json` hourly and serves it, reverse-proxied by the OpenWrt Nginx
  at **`graph.example.com`**. Image, Dockerman template, and Nginx vhost are all
  GitOps-managed (see Deploy) — no hot `docker run`.

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
- `Dockerfile` / `docker-entrypoint.sh` — self-contained image: hourly `fetch_graph.py`
  refresh loop + static server on `:8080`. Bakes the app, never `graph.json` (`.dockerignore`).
- `Makefile` — `local`, `fetch`, `lint`, `build`, `run` (local container smoke test),
  `deploy` (build + `docker save | ssh docker load` to the NAS).

## Deploy (GitOps)

The service is promoted, not hot-run. The three managed pieces:
- **Image:** built by CI (`.github/workflows/build.yml`) on push to `master`, dual-pushed to
  **GHCR** (`ghcr.io/jean1880/neo4j-graph-viz:latest`) and **Docker Hub**, matching the
  nordlynx/plex-server house style (amd64 only — the NAS is x86). Unraid pulls the **public**
  GHCR package with no credentials. CI needs repo secrets `DOCKER_HUB_USERNAME` /
  `DOCKER_HUB_TOKEN`. NEVER `docker save | ssh docker load` — sideloading an untracked image
  breaks Dockerman's update-check/version tracking. Break-glass manual publish: `make push`
  (needs `docker login ghcr.io`). Image bakes only app code — no secrets.
- **Container:** `infra-config` → `roles/nas_docker_templates/templates/my-neo4j-graph-viz.xml.j2`
  (Dockerman template; `<Repository>` = the GHCR image, `<Registry>` = its GHCR page, so Unraid
  manages it as a normal registry image; `NEO4J_PASSWORD` from the `neo4j_password` vault var).
  To update the running container: bump the image via CI, then recreate it from the template in
  the Unraid Docker UI (or force-update) so Dockerman pulls the new tag.
- **Nginx route:** `infra-config` → `proxied_services` (`neo4j-viz`, port 8902) renders the
  OpenWrt vhost. Wildcard `*.example.com` DNS already resolves it; wildcard TLS.

## Rules

- **Secrets:** `NEO4J_PASSWORD` comes from the environment only. Never hardcode it,
  never emit it into `graph.json`, never expose a Neo4j endpoint to the browser —
  the browser only ever loads static `graph.json`.
- **Two surfaces, one secret rule:** `view.sh` stays bound to `127.0.0.1` (local dev). The
  deployed container binds `0.0.0.0:8080` *inside* the container and is only reached via the
  NAS host-port mapping and the OpenWrt Nginx vhost — never expose the Neo4j bolt endpoint or
  the password to the browser. Any change to the deployed surface goes through `infra-config`
  (Dockerman template + `proxied_services`), not a hot `docker run` on the NAS.
- **No framework creep:** it's a single static HTML page on purpose. Prefer adding to
  the existing vanilla JS over introducing a bundler/React.
- **Node-shape drift:** if the graph gains new labels, colours auto-assign (hashed
  hue) and the legend picks them up — no code change needed. Only touch `fetch_graph.py`
  if the id/name/degree contract changes.

Global mandates (verify, minimal-fix, secrets, masking) are in `~/.claude/CLAUDE.md`
— not restated here.
