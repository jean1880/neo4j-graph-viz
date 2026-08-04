# syntax=docker/dockerfile:1
# Multi-stage build: Vue SPA + Rust/axum backend. The Rust backend serves the built SPA and
# GET /api/graph; the endpoint and credentials are injected at runtime, never baked in. Only
# the /api/graph JSON reaches the browser — no Bolt endpoint or credentials.
#
# Base images are pinned to an explicit Debian release (trixie) rather than a floating `-slim`.
# The builder and the runtime must stay on the SAME release: a binary linked against a newer
# glibc fails at container START with a loader error, which no build-time check would catch.

# --- 1. Build the Vue SPA (pulls @nuvek/ui from its public git tag; its prepare builds dist) ---
FROM node:22-trixie-slim AS frontend
WORKDIR /app/frontend
# git is needed for the @nuvek/ui git dependency.
RUN apt-get update && apt-get install -y --no-install-recommends git \
  && rm -rf /var/lib/apt/lists/*
COPY frontend/package.json frontend/package-lock.json ./
RUN npm ci
COPY frontend/ ./
# Relative asset base by default, so one image works at any mount path (see vite.config.ts).
ARG VITE_BASE_PATH
ARG VITE_APP_TITLE
RUN npm run build

# --- 2. Build the Rust backend (pulls nuvek-web from its public git tag) ---
FROM rust:1-slim-trixie AS backend
WORKDIR /app
# git + ca-certificates for the https cargo git dependency.
RUN apt-get update && apt-get install -y --no-install-recommends git ca-certificates \
  && rm -rf /var/lib/apt/lists/*
COPY Cargo.toml Cargo.lock ./
COPY src/ ./src/
# --locked: build exactly what Cargo.lock pins, and fail rather than silently resolving anew.
RUN cargo build --release --locked

# --- 3. Runtime ---
FROM debian:trixie-slim
LABEL org.opencontainers.image.source="https://github.com/jean1880/neo4j-graph-viz"
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates \
  && rm -rf /var/lib/apt/lists/*
# Run unprivileged: many orchestrators enforce runAsNonRoot and will not schedule a root image.
# A fixed high uid keeps it valid even where the container is run with an arbitrary user.
RUN useradd --system --uid 10001 --user-group --no-create-home appuser
WORKDIR /app
COPY --from=backend /app/target/release/neo4j-graph-viz /usr/local/bin/neo4j-graph-viz
COPY --from=frontend /app/frontend/dist /app/dist
USER 10001:10001
# The backend refreshes the graph on an in-process TTL (GRAPH_CACHE_TTL_SECS); no cron loop.
# NEO4J_HOST + NEO4J_PASSWORD are injected at RUNTIME (orchestrator env / secret) — never baked
# here, so no endpoint or credential ships in the published image. See .env.example.
ENV BIND=0.0.0.0 \
    PORT=8080
EXPOSE 8080
# No HEALTHCHECK: this image ships no HTTP client to probe with, and adding curl to a slim
# runtime for that alone is not worth it. GET /healthz is there — point the orchestrator's own
# probe (Kubernetes httpGet, Docker Compose healthcheck with its own tooling) at it.
ENTRYPOINT ["neo4j-graph-viz"]
