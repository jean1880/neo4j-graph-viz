#!/usr/bin/env bash
# Build the Vue SPA + Rust backend and open the graph viewer locally.
#
# Local-only: binds 127.0.0.1. Connection settings come from the environment — either already
# exported, or from a git-ignored .env beside this script (copy .env.example). Nothing is
# hardcoded here, so this file is safe to publish.
set -euo pipefail
cd "$(dirname "$0")"

# Load .env if present, without clobbering anything already exported.
# ENV_FILE overrides the path (e.g. ENV_FILE=~/.config/graph-viz.env ./view.sh).
ENV_FILE="${ENV_FILE:-.env}"
if [[ -f "$ENV_FILE" ]]; then
  # shellcheck disable=SC1090  # runtime path
  set -a && source "$ENV_FILE" && set +a
fi

if [[ -z "${NEO4J_HOST:-}" || -z "${NEO4J_PASSWORD:-}" ]]; then
  echo "error: NEO4J_HOST and NEO4J_PASSWORD must be set." >&2
  echo "       cp .env.example .env and fill it in, or export them, or set ENV_FILE." >&2
  exit 1
fi

PORT="${PORT:-8901}"

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
# `open` is macOS, `xdg-open` is Linux — this script runs on both.
if command -v xdg-open >/dev/null 2>&1; then
  BROWSER_OPEN=xdg-open
elif command -v open >/dev/null 2>&1; then
  BROWSER_OPEN=open
fi
if [[ -n "${BROWSER_OPEN:-}" ]]; then
  (sleep 2 && "$BROWSER_OPEN" "http://localhost:${PORT}/" >/dev/null 2>&1) &
fi

# Local dev talks over loopback, where compressing the payload costs CPU on both ends to shorten
# a transfer that is already effectively free. The deployed container leaves it on (it sits
# behind the Nginx vhost, where it is a clear win). Override by exporting GRAPH_COMPRESSION=1.
exec env BIND=127.0.0.1 PORT="${PORT}" \
  GRAPH_COMPRESSION="${GRAPH_COMPRESSION:-0}" \
  ./target/release/neo4j-graph-viz
