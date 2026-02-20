SHELL := /bin/bash

# Load project .env for non-compose commands (for example local-verify).
# Docker Compose already reads .env by itself, but Make variables do not.
ifneq (,$(wildcard ./.env))
include .env
export
endif

COMPOSE ?= docker compose
CKB_NODE_MODE ?= $(if $(findstring internal,$(COMPOSE_PROFILES)),internal,external)
CKBADGER_DATA_PATH ?= ./data/ckbadger-store
COMPOSE_PROJECT ?= $(notdir $(CURDIR))
VERIFY_API_URL ?= $(or $(CKBADGER_API_URL),http://localhost:3001/api/v1)
VERIFY_DEPTH ?= fast
VERIFY_RPC_URL ?= $(VERIFY_CKB_RPC_URL)
CONFIRM ?= 0

.PHONY: help local-up local-down local-reset local-verify

help:
	@echo "Available targets:"
	@echo "  make local-up                          Start local dependencies"
	@echo "  make local-down                        Stop local dependencies"
	@echo "  make local-reset CONFIRM=1             Delete local RocksDB + redis volumes/data"
	@echo "  make local-verify                      Run verify --depth fast"
	@echo "  make local-verify VERIFY_DEPTH=sampling VERIFY_RPC_URL=http://localhost:8114"

local-up:
ifeq ($(CKB_NODE_MODE),internal)
	$(COMPOSE) --profile internal up -d redis ckb-node
else
	$(COMPOSE) up -d redis
endif
	@echo "Dependencies started."
	@echo "Node mode: $(CKB_NODE_MODE)"
	@echo "Start indexer: cargo run -p ckbadger-indexer"
	@echo "Start API: cargo run -p ckbadger-api"
	@echo "Start frontend: cd frontend && pnpm dev"

local-down:
	$(COMPOSE) down

local-reset:
	@if [ "$(CONFIRM)" != "1" ]; then \
		echo "Refusing to delete data without CONFIRM=1"; \
		echo "Example: make local-reset CONFIRM=1"; \
		exit 1; \
	fi
	-$(COMPOSE) stop redis api indexer frontend >/dev/null 2>&1 || true
	rm -rf "$(CKBADGER_DATA_PATH)" "$(CKBADGER_DATA_PATH)-api-secondary"
	@if command -v docker >/dev/null 2>&1; then \
		project="$${COMPOSE_PROJECT_NAME:-$(COMPOSE_PROJECT)}"; \
		rocksdb_vols=$$(docker volume ls -q --filter "label=com.docker.compose.project=$$project" --filter "label=com.docker.compose.volume=ckbadger-data"); \
		redis_vols=$$(docker volume ls -q --filter "label=com.docker.compose.project=$$project" --filter "label=com.docker.compose.volume=redis-data"); \
		if [ -n "$$rocksdb_vols$$redis_vols" ]; then \
			docker volume rm $$rocksdb_vols $$redis_vols >/dev/null || true; \
		fi; \
	fi
	@echo "Deleted local data path: $(CKBADGER_DATA_PATH)"
	@echo "Deleted Docker volumes (if present): ckbadger-data, redis-data"

local-verify:
	@RPC_ARG=""; \
	if [ -n "$(VERIFY_RPC_URL)" ]; then RPC_ARG="--rpc-url $(VERIFY_RPC_URL)"; fi; \
	cargo run -p ckbadger-indexer -- verify --depth "$(VERIFY_DEPTH)" --api-url "$(VERIFY_API_URL)" $$RPC_ARG
