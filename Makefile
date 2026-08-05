# Neo4j graph viewer — the whole flow: develop, gate, build, run, verify, publish.
#
# Endpoint + credentials (NEO4J_HOST / NEO4J_PASSWORD) come from $(ENV_FILE) at runtime, never
# from this file and never from the image. Point ENV_FILE at whatever holds them:
#
#   make run                              # uses ./.env
#   ENV_FILE=~/.my-env make run-published # uses another file
#
# NOTE: keep comments on their OWN lines. Make captures everything up to a trailing `#`,
# whitespace included, so `PORT ?= 8902   # note` yields the value "8902   " and silently
# corrupts anything it is concatenated into (e.g. a -p host:port mapping).
IMAGE       ?= neo4j-graph-viz
# Override for your own org: make push REGISTRY=ghcr.io/your-org
REGISTRY    ?= ghcr.io/OWNER
PUSH_IMAGE  ?= $(REGISTRY)/$(IMAGE)
# The CI-built image, used by pull / run-published.
PUBLISHED   ?= $(PUSH_IMAGE):latest
ENV_FILE    ?= .env
# Host port for container runs (native view.sh uses 8901).
DOCKER_PORT ?= 8902
URL         ?= http://127.0.0.1:$(DOCKER_PORT)

# The app's own configuration keys. Only these are forwarded into the container — never the
# whole env file, which may hold unrelated secrets that have no business in this container.
APP_ENV = NEO4J_HOST NEO4J_PASSWORD NEO4J_USER NEO4J_DATABASE \
          GRAPH_WRAPPER_LABELS GRAPH_SKIP_PROPS GRAPH_NODE_LABELS GRAPH_REL_TYPES \
          GRAPH_NAME_KEYS GRAPH_MAX_NODES GRAPH_MAX_RELS GRAPH_MAX_PROP_CHARS \
          GRAPH_CACHE_TTL_SECS
ENV_FLAGS = $(foreach v,$(APP_ENV),-e $(v))

# Source the env file, then hand docker only the named keys. `set -a` exports whatever the file
# defines, so both `FOO=bar` and `export FOO=bar` work; values never touch a log or a temp file.
define docker_run
	@test -f $(ENV_FILE) || { echo "missing $(ENV_FILE) — cp .env.example .env"; exit 1; }
	@bash -c 'set -a; . "$(ENV_FILE)"; set +a; \
	  docker run $(1) --name $(IMAGE) \
	    $(ENV_FLAGS) -e BIND=0.0.0.0 -e PORT=8080 \
	    -p 127.0.0.1:$(DOCKER_PORT):8080 $(2)'
endef

.DEFAULT_GOAL := help

## help: list targets
help:
	@grep -E '^## ' $(MAKEFILE_LIST) | sed 's/## /  /'

# --- develop --------------------------------------------------------------------------------

## local: build the SPA + Rust backend and serve on 127.0.0.1:8901 (opens a browser)
local:
	./view.sh

## gate: cargo fmt/clippy/test + frontend build/typecheck — what CI runs
gate:
	cargo fmt --check
	cargo clippy --all-targets -- -D warnings
	cargo test
	cd frontend && npm ci && npm run build && npm run typecheck

## lint: shellcheck the shell entry points
lint:
	shellcheck view.sh smoke.sh

# --- container ------------------------------------------------------------------------------

## build: build the container image locally
build:
	docker build -t $(IMAGE) .

## run: build + run locally in the foreground (loopback only, port DOCKER_PORT)
run: build
	$(call docker_run,--rm,$(IMAGE))

## up: build + run detached, then wait until it answers
up: build
	-@docker rm -f $(IMAGE) >/dev/null 2>&1
	$(call docker_run,-d,$(IMAGE))
	@$(MAKE) --no-print-directory wait

## login: authenticate to $(REGISTRY) using the gh CLI token (never printed)
login:
	@gh auth token | docker login $(firstword $(subst /, ,$(REGISTRY))) \
	  -u $$(gh api user --jq .login) --password-stdin

## pull: pull the published CI-built image
pull:
	docker pull $(PUBLISHED)

## run-published: run the CI-built image detached — verifies what CI actually shipped,
##                not a local rebuild. Pair with `make smoke`.
run-published: pull
	-@docker rm -f $(IMAGE) >/dev/null 2>&1
	$(call docker_run,-d,$(PUBLISHED))
	@$(MAKE) --no-print-directory wait

## wait: block until the running container answers /healthz
wait:
	@for i in $$(seq 1 30); do \
	  if curl -fsS -o /dev/null $(URL)/healthz 2>/dev/null; then \
	    echo "up at $(URL)"; exit 0; fi; sleep 1; done; \
	echo "did not come up — see: make logs"; exit 1

## smoke: run the read-only smoke test (override URL= to target a deployed instance)
smoke:
	@./smoke.sh $(URL)

## verify: full local proof — build, run detached, smoke-test, tear down
verify: up
	@./smoke.sh $(URL); status=$$?; $(MAKE) --no-print-directory stop; exit $$status

## verify-published: same, against the CI-built image rather than a local build
verify-published: run-published
	@./smoke.sh $(URL); status=$$?; $(MAKE) --no-print-directory stop; exit $$status

## logs: tail the running container's logs
logs:
	docker logs -f $(IMAGE)

## stop: stop/remove the container
stop:
	-@docker rm -f $(IMAGE) >/dev/null 2>&1 || true

# --- publish --------------------------------------------------------------------------------

## push: manual publish (CI on push to the default branch is the normal path).
##       Requires `make login` first. Set REGISTRY to your own org.
push:
	@case "$(PUSH_IMAGE)" in */OWNER/*) \
	  echo "set REGISTRY, e.g. make push REGISTRY=ghcr.io/your-org"; exit 1 ;; esac
	docker build -t $(PUSH_IMAGE):latest .
	docker push $(PUSH_IMAGE):latest
	@echo "pushed $(PUSH_IMAGE):latest"

## clean: stop the container and remove the local image
clean: stop
	-docker rmi $(IMAGE)

.PHONY: help local gate lint build run up login pull run-published wait smoke verify \
        verify-published logs stop push clean
