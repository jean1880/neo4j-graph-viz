# Neo4j graph viewer — local dev + container build.
# Endpoint + credentials (NEO4J_HOST / NEO4J_PASSWORD) come from the environment or a
# git-ignored .env at runtime — never this file, and never the image.
IMAGE       ?= neo4j-graph-viz
REGISTRY    ?= ghcr.io/OWNER            # override: make push REGISTRY=ghcr.io/your-org
PUSH_IMAGE  ?= $(REGISTRY)/$(IMAGE)
ENV_FILE    ?= .env
DOCKER_PORT ?= 8902   # host port for the local container test (native view.sh uses 8901)

.DEFAULT_GOAL := help

## help: list targets
help:
	@grep -E '^## ' $(MAKEFILE_LIST) | sed 's/## /  /'

## local: build the SPA + Rust backend and serve on 127.0.0.1:8901 (opens a browser)
local:
	./view.sh

## gate: cargo fmt/clippy/test + frontend build/typecheck
gate:
	cargo fmt --check
	cargo clippy --all-targets -- -D warnings
	cargo test
	cd frontend && npm ci && npm run build && npm run typecheck

## lint: shellcheck the shell entry point
lint:
	shellcheck view.sh

## build: build the container image
build:
	docker build -t $(IMAGE) .

## run: build + run the container locally for a smoke test (http://127.0.0.1:$(DOCKER_PORT))
##      Reads config from $(ENV_FILE) — copy .env.example first. Binds loopback only.
run: build
	@test -f $(ENV_FILE) || { echo "missing $(ENV_FILE) — cp .env.example .env"; exit 1; }
	docker run --rm --env-file $(ENV_FILE) \
	  -e BIND=0.0.0.0 -e PORT=8080 \
	  -p 127.0.0.1:$(DOCKER_PORT):8080 --name $(IMAGE) $(IMAGE)

## stop: stop the local test container
stop:
	-docker stop $(IMAGE)

## push: manual publish (CI on push to the default branch is the normal path).
##       Requires `docker login` against $(REGISTRY) first. Set REGISTRY to your own org.
push:
	@case "$(PUSH_IMAGE)" in */OWNER/*) \
	  echo "set REGISTRY, e.g. make push REGISTRY=ghcr.io/your-org"; exit 1 ;; esac
	docker build -t $(PUSH_IMAGE):latest .
	docker push $(PUSH_IMAGE):latest
	@echo "pushed $(PUSH_IMAGE):latest"

## clean: stop the container and remove the image
clean: stop
	-docker rmi $(IMAGE)

.PHONY: help local gate lint build run stop clean push
