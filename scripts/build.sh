#!/usr/bin/env bash
set -euo pipefail

if ! command -v cargo >/dev/null 2>&1; then
  echo "cargo not found. Install Rust toolchain first (see scripts/install-deps.sh)." >&2
  exit 1
fi

if ! command -v pkg-config >/dev/null 2>&1; then
  echo "pkg-config not found. Install system build dependencies first." >&2
  exit 1
fi

if ! pkg-config --exists gtk4; then
  echo "GTK4 development files not found (pkg-config gtk4 failed)." >&2
  echo "Run ./scripts/install-deps.sh" >&2
  exit 1
fi

if [[ ! -f Cargo.lock ]]; then
  echo "Cargo.lock is missing." >&2
  echo "Run: cargo generate-lockfile" >&2
  exit 1
fi

cargo build --locked
