#!/usr/bin/env bash
# Container entrypoint: keep graph.json fresh on a loop, serve the static viewer.
# NEO4J_PASSWORD is read from the environment (Dockerman), never baked into the
# image. The browser only ever loads the static graph.json — no bolt exposure.
# Deliberately NOT `set -e`: a failed fetch (Neo4j blip) must not kill serving.
set -uo pipefail
cd /app || exit 1

PORT="${PORT:-8080}"
REFRESH_SECONDS="${REFRESH_SECONDS:-3600}"

refresh() {
  if python3 fetch_graph.py; then
    echo "[graphviz] graph.json refreshed"
  else
    echo "[graphviz] WARN: fetch failed — keeping previous graph.json" >&2
  fi
}

# Populate once before serving; then refresh on the loop in the background.
refresh
( while true; do sleep "$REFRESH_SECONDS"; refresh; done ) &

echo "[graphviz] serving on 0.0.0.0:${PORT} (refresh every ${REFRESH_SECONDS}s)"
exec python3 -m http.server "$PORT" --bind 0.0.0.0
