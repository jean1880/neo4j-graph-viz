# neo4j-graph-viz

A local, Obsidian-style force-directed viewer for the homelab Neo4j graph
(`bolt://localhost:7687`). Fills the gap left by Neo4j Browser (query
subgraphs only) and Bloom (Enterprise-only): a persistent "map of everything"
you can explore, filter, and search.

## Usage

```bash
./view.sh          # pull live data, serve on http://localhost:8901, open browser
PORT=9000 ./view.sh
```

`view.sh` sources `~/.env` for `NEO4J_PASSWORD`, regenerates `graph.json`
from the live graph, then serves the static viewer bound to `127.0.0.1` only.

## Controls

- **scroll** zoom · **drag** pan · **click** focus a node · **right-click** fit to view
- **hover** a node to highlight it and its neighbours (dims the rest)
- **search box** finds and centres a node by name
- **legend** — click a type to show/hide all nodes of that type

## How it works

- `fetch_graph.py` — connects with the `neo4j` driver (password from the
  environment, never hardcoded, never sent to the browser), pulls every node +
  relationship, and writes `graph.json` (`{nodes, links}` shaped for force-graph).
  Node colour = type, size = degree; `MH:*` nodes fold into one D&D domain group.
- `index.html` — [vasturiano/force-graph](https://github.com/vasturiano/force-graph)
  (vendored as `force-graph.min.js`, no runtime CDN) rendering `graph.json`.
- `graph.json` is a generated snapshot and is git-ignored — rerun `view.sh` to refresh.

## Notes

- Data is a snapshot taken at launch, not live-streaming. Re-run `view.sh` to refresh.
- The graph holds two worlds — homelab infra (Containers, Volumes, Ports, Skills,
  Repos, AnsibleRoles…) and the Monster Hunters D&D canon (Characters, Locations,
  Factions, Deities…). Use the legend toggles to isolate either.
