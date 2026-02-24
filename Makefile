SHELL := /bin/bash

# Load project .env for non-compose commands (for example verify).
# Docker Compose already reads .env by itself, but Make variables do not.
ifneq (,$(wildcard ./.env))
include .env
export
endif

COMPOSE ?= docker compose
CKB_NODE_MODE ?= $(if $(findstring internal,$(COMPOSE_PROFILES)),internal,external)
CKBADGER_DATA_PATH ?= ./data/ckbadger-store
CKBADGER_HEAVY_DATA_PATH ?= ./data/ckbadger-heavy-store
COMPOSE_PROJECT ?= $(notdir $(CURDIR))
VERIFY_API_URL ?= $(or $(CKBADGER_API_URL),http://localhost:3001/api/v1)
VERIFY_DEPTH ?= fast
VERIFY_RPC_URL ?= $(VERIFY_CKB_RPC_URL)
TUI_ARGS ?=
CONFIRM ?= 0
SERVICE ?=
SERVICES ?= $(strip $(SERVICE))
REBUILD_SERVICES_ALLOWED := redis ckb-node indexer api frontend
REBUILD_ALL_SERVICES_EXTERNAL := redis indexer api frontend
REBUILD_ALL_SERVICES_INTERNAL := redis ckb-node indexer api frontend

.PHONY: help up down reset verify rebuild rebuild-all tui

help:
	@echo "Available targets:"
	@echo "  make up                                Start local stack"
	@echo "  make down                              Stop local stack"
	@echo "  make rebuild SERVICES=\"api frontend\"   Rebuild + restart one/more services"
	@echo "  make rebuild-all                       Rebuild all services for current node mode"
	@echo "  make tui                               Run monitoring TUI"
	@echo "  make reset CONFIRM=1                   Delete local RocksDB + redis volumes/data"
	@echo "  make verify                            Run verify --depth fast"
	@echo "  make verify VERIFY_DEPTH=sampling VERIFY_RPC_URL=http://localhost:8114"

up:
ifeq ($(CKB_NODE_MODE),internal)
	$(COMPOSE) --profile internal up -d redis ckb-node indexer api frontend
else
	$(COMPOSE) up -d redis indexer api frontend
endif
	@echo "Services started."
	@echo "Node mode: $(CKB_NODE_MODE)"
	@echo "Frontend: http://localhost:3000"
	@echo "API: http://localhost:3001/api/v1"

down:
	$(COMPOSE) down

rebuild:
	@if [ -z "$(SERVICES)" ]; then \
		echo "Usage: make rebuild SERVICES=\"<service> [service ...]\""; \
		echo "Allowed services: $(REBUILD_SERVICES_ALLOWED)"; \
		exit 1; \
	fi
	@normal_services=""; \
	needs_ckb_node=0; \
	for svc in $(SERVICES); do \
		case " $(REBUILD_SERVICES_ALLOWED) " in \
			*" $$svc "*) ;; \
			*) \
				echo "Unsupported service: $$svc"; \
				echo "Allowed services: $(REBUILD_SERVICES_ALLOWED)"; \
				exit 1; \
				;; \
		esac; \
		if [ "$$svc" = "ckb-node" ]; then \
			needs_ckb_node=1; \
		else \
			normal_services="$$normal_services $$svc"; \
		fi; \
	done; \
	if [ "$$needs_ckb_node" = "1" ] && [ "$(CKB_NODE_MODE)" != "internal" ]; then \
		echo "ckb-node rebuild requires internal mode."; \
		echo "Set COMPOSE_PROFILES=internal in .env, or run:"; \
		echo "  make rebuild SERVICES=ckb-node CKB_NODE_MODE=internal"; \
		exit 1; \
	fi; \
	if [ "$$needs_ckb_node" = "1" ]; then \
		$(COMPOSE) --profile internal up -d --build --force-recreate ckb-node; \
	fi; \
	if [ -n "$${normal_services## }" ]; then \
		$(COMPOSE) up -d --build --no-deps --force-recreate $$normal_services; \
	fi; \
	echo "Rebuilt and restarted services: $(SERVICES)"

rebuild-all:
ifeq ($(CKB_NODE_MODE),internal)
	$(MAKE) rebuild SERVICES="$(REBUILD_ALL_SERVICES_INTERNAL)"
else
	$(MAKE) rebuild SERVICES="$(REBUILD_ALL_SERVICES_EXTERNAL)"
endif

tui:
	cargo run -p ckbadger-tui $(if $(strip $(TUI_ARGS)),-- $(TUI_ARGS),)

reset:
	@if [ "$(CONFIRM)" != "1" ]; then \
		echo "Refusing to delete data without CONFIRM=1"; \
		echo "Example: make reset CONFIRM=1"; \
		exit 1; \
	fi
	-$(COMPOSE) stop redis api indexer frontend >/dev/null 2>&1 || true
	rm -rf "$(CKBADGER_DATA_PATH)" "$(CKBADGER_DATA_PATH)-api-secondary" \
		"$(CKBADGER_HEAVY_DATA_PATH)" "$(CKBADGER_HEAVY_DATA_PATH)-api-secondary"
	@if command -v docker >/dev/null 2>&1; then \
		project="$${COMPOSE_PROJECT_NAME:-$(COMPOSE_PROJECT)}"; \
		rocksdb_vols=$$(docker volume ls -q --filter "label=com.docker.compose.project=$$project" --filter "label=com.docker.compose.volume=ckbadger-data"); \
		heavy_vols=$$(docker volume ls -q --filter "label=com.docker.compose.project=$$project" --filter "label=com.docker.compose.volume=ckbadger-heavy-data"); \
		redis_vols=$$(docker volume ls -q --filter "label=com.docker.compose.project=$$project" --filter "label=com.docker.compose.volume=redis-data"); \
		if [ -n "$$rocksdb_vols$$heavy_vols$$redis_vols" ]; then \
			docker volume rm $$rocksdb_vols $$heavy_vols $$redis_vols >/dev/null || true; \
		fi; \
	fi
	@echo "Deleted local data path: $(CKBADGER_DATA_PATH)"
	@echo "Deleted local heavy data path: $(CKBADGER_HEAVY_DATA_PATH)"
	@echo "Deleted Docker volumes (if present): ckbadger-data, ckbadger-heavy-data, redis-data"

verify:
	@RPC_ARG=""; \
	if [ -n "$(VERIFY_RPC_URL)" ]; then RPC_ARG="--rpc-url $(VERIFY_RPC_URL)"; fi; \
	cargo run -p ckbadger-indexer -- verify --depth "$(VERIFY_DEPTH)" --api-url "$(VERIFY_API_URL)" $$RPC_ARG
