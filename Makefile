SHELL := /bin/bash

.PHONY: help build check test lint verify

help:
	@echo "Available targets:"
	@echo "  make build     Build the ckbadger CLI binary"
	@echo "  make check     Type-check and lint all Rust code"
	@echo "  make test      Run all tests (Rust + frontend)"
	@echo "  make lint      Run frontend lint and type-check"
	@echo "  make verify    Run data integrity verification (requires running API)"

build:
	cargo build -p ckbadger

check:
	cargo check && cargo clippy

test:
	cargo test && cd frontend && pnpm test

lint:
	cd frontend && pnpm lint && pnpm type-check

verify:
	cargo run -p ckbadger -- verify --depth fast
