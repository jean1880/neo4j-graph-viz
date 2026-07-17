# Neo4j graph viewer — local dev + container build.
# Secrets (NEO4J_PASSWORD) always come from ~/.env, never this file.
IMAGE       ?= neo4j-graph-viz
DOCKER_PORT ?= 8902   # host port for the local container test (native view.sh uses 8901)
NAS_SSH     ?= nas   # ssh alias for the Unraid NAS (docker load target)

.DEFAULT_GOAL := help

## help: list targets
help:
	@grep -E '^## ' $(MAKEFILE_LIST) | sed 's/## /  /'

## local: fetch the graph and serve the viewer natively on 127.0.0.1:8901
local:
	./view.sh

## fetch: regenerate graph.json from Neo4j (sources ~/.env)
fetch:
	bash -c 'source $$HOME/.env && python3 fetch_graph.py'

## lint: shellcheck the shell scripts
lint:
	shellcheck view.sh docker-entrypoint.sh

## build: build the container image
build:
	docker build -t $(IMAGE) .

## run: build + run the container locally for a smoke test (http://127.0.0.1:$(DOCKER_PORT))
run: build
	bash -c 'source $$HOME/.env && docker run --rm \
	  -e NEO4J_PASSWORD -e NEO4J_HOST \
	  -p 127.0.0.1:$(DOCKER_PORT):8080 --name $(IMAGE) $(IMAGE)'

## stop: stop the local test container
stop:
	-docker stop $(IMAGE)

## deploy: build + ship the image to the NAS (docker save | ssh docker load)
##         the Dockerman container + nginx vhost are managed in infra-config
deploy: build
	docker save $(IMAGE) | ssh $(NAS_SSH) 'docker load'
	@echo "image on NAS. To pick up the new image, restart the container:"
	@echo "  ssh $(NAS_SSH) 'docker restart neo4j-graph-viz'"

## clean: stop the container and remove the image
clean: stop
	-docker rmi $(IMAGE)

.PHONY: help local fetch lint build run stop clean deploy
