SHELL := /bin/bash

# Load project .env for non-compose commands (for example verify).
# Docker Compose already reads .env by itself, but Make variables do not.
ifneq (,$(wildcard ./.env))
include .env
export
endif

COMPOSE ?= docker compose
CKB_NODE_MODE ?= $(if $(findstring internal,$(COMPOSE_PROFILES)),internal,external)
ifndef CKBADGER_DOMAIN_DATA_PATH
CKBADGER_DOMAIN_DATA_PATH := ./data/ckbadger-store
endif
ifndef CKBADGER_APPEND_ONLY_DATA_PATH
CKBADGER_APPEND_ONLY_DATA_PATH := $(CKBADGER_DOMAIN_DATA_PATH)-append-only
endif
COMPOSE_PROJECT ?= $(notdir $(CURDIR))
VERIFY_API_URL ?= $(or $(CKBADGER_API_URL),http://localhost:3001/api/v1)
VERIFY_DEPTH ?= fast
VERIFY_RPC_URL ?= $(VERIFY_CKB_RPC_URL)
TUI_ARGS ?=
CONFIRM ?= 0
SERVICE ?=
SERVICES ?= $(strip $(SERVICE))
PERF_AUTO_BASELINE ?= 1
PERF_OUTPUT_DIR ?= artifacts/perf/bulk-sync
PERF_MONITOR_CONTAINER ?= ckbadger-indexer
PERF_MONITOR_MAX_SECONDS ?= 172800
PERF_MONITOR_POLL_SECONDS ?= 20
REBUILD_SERVICES_ALLOWED := redis ckb-node indexer api frontend
REBUILD_ALL_SERVICES_EXTERNAL := redis indexer api frontend
REBUILD_ALL_SERVICES_INTERNAL := redis ckb-node indexer api frontend

.PHONY: help up down reset verify rebuild rebuild-all tui perf-latest test-perf-scripts

help:
	@echo "Available targets:"
	@echo "  make up                                Start local stack"
	@echo "  make down                              Stop local stack"
	@echo "  make rebuild SERVICES=\"api frontend\"   Rebuild + restart one/more services"
	@echo "  make rebuild-all                       Rebuild all services for current node mode"
	@echo "  make perf-latest                       Show latest bulk-sync perf summary and deltas"
	@echo "  make tui                               Run monitoring TUI"
	@echo "  make test-perf-scripts                 Run perf script regression checks"
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
	@set -euo pipefail; \
	services="$(REBUILD_ALL_SERVICES_EXTERNAL)"; \
	if [ "$(CKB_NODE_MODE)" = "internal" ]; then \
		services="$(REBUILD_ALL_SERVICES_INTERNAL)"; \
	fi; \
	fresh_db=0; \
	if [ "$(PERF_AUTO_BASELINE)" = "1" ]; then \
		if scripts/perf/detect_fresh_db_rebuild.sh --compose-project "$(COMPOSE_PROJECT)" >/dev/null 2>&1; then \
			fresh_db=1; \
			echo "Perf auto baseline: fresh DB detected, will start monitor after rebuild-all."; \
		else \
			detect_rc=$$?; \
			if [ "$$detect_rc" -eq 1 ]; then \
				echo "Perf auto baseline: existing DB detected, skip auto baseline monitor."; \
			else \
				echo "Perf auto baseline: fresh DB detection unavailable (rc=$$detect_rc), skip auto baseline monitor."; \
			fi; \
		fi; \
	fi; \
	$(MAKE) rebuild SERVICES="$$services"; \
	if [ "$(PERF_AUTO_BASELINE)" = "1" ] && [ "$$fresh_db" = "1" ]; then \
		run_id=$$(date -u +%Y%m%dT%H%M%SZ); \
		run_dir="$(PERF_OUTPUT_DIR)/$$run_id"; \
		mkdir -p "$$run_dir"; \
		nohup scripts/perf/bulk_sync_monitor.sh \
			--output-root "$(PERF_OUTPUT_DIR)" \
			--run-id "$$run_id" \
			--container "$(PERF_MONITOR_CONTAINER)" \
			--max-seconds "$(PERF_MONITOR_MAX_SECONDS)" \
			--poll-seconds "$(PERF_MONITOR_POLL_SECONDS)" \
			--compose-project "$(COMPOSE_PROJECT)" \
			> "$$run_dir/monitor.nohup.log" 2>&1 < /dev/null & \
		monitor_pid=$$!; \
		echo "$$monitor_pid" > "$$run_dir/monitor.pid"; \
		echo "Perf auto baseline monitor started: pid=$$monitor_pid run_id=$$run_id"; \
		echo "Perf artifacts directory: $$run_dir"; \
		echo "Follow logs: tail -f $$run_dir/monitor.nohup.log"; \
	fi

tui:
	cargo run -p ckbadger-tui -- $(TUI_ARGS)

perf-latest:
	bash scripts/perf/perf_latest.sh --output-root "$(PERF_OUTPUT_DIR)"

test-perf-scripts:
	bash scripts/perf/tests/test_bulk_sync_report.sh
	bash scripts/perf/tests/test_perf_latest.sh

reset:
	@if [ "$(CONFIRM)" != "1" ]; then \
		echo "Refusing to delete data without CONFIRM=1"; \
		echo "Example: make reset CONFIRM=1"; \
		exit 1; \
	fi
	-$(COMPOSE) stop redis api indexer frontend >/dev/null 2>&1 || true
	rm -rf "$(CKBADGER_DOMAIN_DATA_PATH)" "$(CKBADGER_DOMAIN_DATA_PATH)-api-secondary"
	rm -rf "$(CKBADGER_APPEND_ONLY_DATA_PATH)" "$(CKBADGER_APPEND_ONLY_DATA_PATH)-api-secondary"
	@if command -v docker >/dev/null 2>&1; then \
		project="$${COMPOSE_PROJECT_NAME:-$(COMPOSE_PROJECT)}"; \
		rocksdb_vols=$$(docker volume ls -q --filter "label=com.docker.compose.project=$$project" --filter "label=com.docker.compose.volume=ckbadger-data"); \
		redis_vols=$$(docker volume ls -q --filter "label=com.docker.compose.project=$$project" --filter "label=com.docker.compose.volume=redis-data"); \
		if [ -n "$$rocksdb_vols$$redis_vols" ]; then \
			docker volume rm $$rocksdb_vols $$redis_vols >/dev/null || true; \
		fi; \
	fi
	@echo "Deleted local domain path: $(CKBADGER_DOMAIN_DATA_PATH)"
	@echo "Deleted local append-only path: $(CKBADGER_APPEND_ONLY_DATA_PATH)"
	@echo "Deleted Docker volumes (if present): ckbadger-data, redis-data"

verify:
	@RPC_ARG=""; \
	if [ -n "$(VERIFY_RPC_URL)" ]; then RPC_ARG="--rpc-url $(VERIFY_RPC_URL)"; fi; \
	cargo run -p ckbadger-indexer -- verify --depth "$(VERIFY_DEPTH)" --api-url "$(VERIFY_API_URL)" $$RPC_ARG
