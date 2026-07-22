# syntax=docker/dockerfile:1
# Multi-stage build: Vue SPA + Rust/axum backend. The Rust backend serves the built SPA and
# GET /api/graph; NEO4J_PASSWORD is injected at runtime (Dockerman), never baked in. Only the
# /api/graph JSON reaches the browser — no Bolt endpoint or credentials.

# --- 1. Build the Vue SPA (pulls @nuvek/ui from its public git tag; its prepare builds dist) ---
FROM node:22-slim AS frontend
WORKDIR /app/frontend
# git is needed for the `github:jean1880/nuvek-ui` dependency.
RUN apt-get update && apt-get install -y --no-install-recommends git \
  && rm -rf /var/lib/apt/lists/*
COPY frontend/package.json frontend/package-lock.json ./
RUN npm ci
COPY frontend/ ./
RUN npm run build

# --- 2. Build the Rust backend (pulls nuvek-web from its public git tag) ---
FROM rust:1-slim AS backend
WORKDIR /app
# git + ca-certificates for the https cargo git dependency.
RUN apt-get update && apt-get install -y --no-install-recommends git ca-certificates \
  && rm -rf /var/lib/apt/lists/*
COPY Cargo.toml Cargo.lock ./
COPY src/ ./src/
RUN cargo build --release

# --- 3. Runtime ---
FROM debian:bookworm-slim
LABEL org.opencontainers.image.source="https://github.com/jean1880/neo4j-graph-viz"
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates \
  && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=backend /app/target/release/neo4j-graph-viz /usr/local/bin/neo4j-graph-viz
COPY --from=frontend /app/frontend/dist /app/dist
# The Rust backend refreshes the graph on an hourly in-process TTL (no cron loop needed).
# NEO4J_HOST + NEO4J_PASSWORD are injected at RUNTIME (Dockerman env / docker secret) — never
# baked here, so no private endpoint or credential ships in the published image.
ENV BIND=0.0.0.0 \
    PORT=8080
EXPOSE 8080
ENTRYPOINT ["neo4j-graph-viz"]
