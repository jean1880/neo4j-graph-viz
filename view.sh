#!/usr/bin/env bash
# Build the Vue SPA + Rust backend and open the Obsidian-style Neo4j graph viewer.
# Local-only: binds 127.0.0.1 and reads NEO4J_PASSWORD from ~/.env. The Rust backend
# serves both the built SPA (frontend/dist) and GET /api/graph — no Python, no static snapshot.
set -euo pipefail
cd "$(dirname "$0")"

PORT="${PORT:-8901}"

# shellcheck source=/dev/null
source "$HOME/.env"

# 1. Build the frontend SPA — the Rust `serve` layer serves frontend/dist.
(
  cd frontend
  [[ -d node_modules ]] || npm install
  npm run build
)

# 2. Build the Rust backend (release).
cargo build --release

echo "Serving http://localhost:${PORT}/  (Ctrl-C to stop)"
# Open the browser once the server has had a moment to bind (exec replaces this shell).
(sleep 2 && xdg-open "http://localhost:${PORT}/" >/dev/null 2>&1) &
exec env BIND=127.0.0.1 PORT="${PORT}" ./target/release/neo4j-graph-viz
