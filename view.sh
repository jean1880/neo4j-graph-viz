#!/usr/bin/env bash
# Pull the live Neo4j graph and open the Obsidian-style viewer in a browser.
# Local-only: binds to 127.0.0.1 and reads NEO4J_PASSWORD from ~/.env.
set -euo pipefail
cd "$(dirname "$0")"

PORT="${PORT:-8901}"

# shellcheck source=/dev/null
source "$HOME/.env"

python3 fetch_graph.py

echo "Serving http://localhost:${PORT}/  (Ctrl-C to stop)"
xdg-open "http://localhost:${PORT}/" >/dev/null 2>&1 || true
exec python3 -m http.server "${PORT}" --bind 127.0.0.1
