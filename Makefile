SHELL := /usr/bin/env bash

PREFIX ?= /usr/local
BINDIR ?= $(PREFIX)/bin
APP_NAME := vibar

.PHONY: deps lock build build-release run check fmt lint test ci install uninstall

deps:
	./scripts/install-deps.sh

lock:
	cargo generate-lockfile

build:
	./scripts/build.sh

build-release:
	cargo build --release --locked

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

install:
	@test -x target/release/$(APP_NAME) || (echo "Missing target/release/$(APP_NAME). Run 'make build-release' first." >&2; exit 1)
	install -Dm755 target/release/$(APP_NAME) $(DESTDIR)$(BINDIR)/$(APP_NAME)

uninstall:
	rm -f $(DESTDIR)$(BINDIR)/$(APP_NAME)
