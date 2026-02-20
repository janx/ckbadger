SHELL := /bin/bash

COMPOSE ?= docker compose
CKB_NODE_MODE ?= internal
CKBADGER_DATA_PATH ?= ./data/ckbadger-store
VERIFY_API_URL ?= http://localhost:3001/api/v1
VERIFY_DEPTH ?= fast
VERIFY_RPC_URL ?=
CONFIRM ?= 0

.PHONY: help local-up local-down local-reset local-verify

help:
	@echo "Available targets:"
	@echo "  make local-up                          Start local dependencies"
	@echo "  make local-down                        Stop local dependencies"
	@echo "  make local-reset CONFIRM=1             Delete local RocksDB data"
	@echo "  make local-verify                      Run verify --depth fast"
	@echo "  make local-verify VERIFY_DEPTH=sampling VERIFY_RPC_URL=http://localhost:8114"

local-up:
ifeq ($(CKB_NODE_MODE),internal)
	$(COMPOSE) --profile internal up -d redis ckb-node
else
	$(COMPOSE) up -d redis
endif
	@echo "Dependencies started."
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
	rm -rf "$(CKBADGER_DATA_PATH)" "$(CKBADGER_DATA_PATH)-api-secondary"
	@echo "Deleted local data path: $(CKBADGER_DATA_PATH)"

local-verify:
	@RPC_ARG=""; \
	if [ -n "$(VERIFY_RPC_URL)" ]; then RPC_ARG="--rpc-url $(VERIFY_RPC_URL)"; fi; \
	cargo run -p ckbadger-indexer -- verify --depth "$(VERIFY_DEPTH)" --api-url "$(VERIFY_API_URL)" $$RPC_ARG
