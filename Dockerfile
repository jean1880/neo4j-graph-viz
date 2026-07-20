# Self-contained image for the Neo4j graph viewer.
# Runs fetch_graph.py on a loop (server-side, holds NEO4J_PASSWORD) and serves
# the static force-graph page. Only graph.json ever reaches the browser.
FROM python:3.12-slim

LABEL org.opencontainers.image.source="https://github.com/jean1880/neo4j-graph-viz"

WORKDIR /app

RUN pip install --no-cache-dir "neo4j>=5,<6"

COPY fetch_graph.py index.html force-graph.min.js docker-entrypoint.sh ./
RUN chmod +x docker-entrypoint.sh

ENV PORT=8080 \
    REFRESH_SECONDS=3600 \
    NEO4J_HOST=bolt://localhost:7687

EXPOSE 8080
ENTRYPOINT ["./docker-entrypoint.sh"]
