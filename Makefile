SHELL := /usr/bin/env bash

.PHONY: deps lock build run check fmt lint test ci

deps:
	./scripts/install-deps.sh

lock:
	cargo generate-lockfile

build:
	./scripts/build.sh

run:
	cargo run --locked

check:
	cargo check --locked

fmt:
	cargo fmt --all

lint:
	cargo clippy --all-targets -- -D warnings

test:
	cargo test --locked

ci:
	cargo fmt --all -- --check
	cargo clippy --all-targets -- -D warnings
	cargo test --locked
