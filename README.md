# mybar (MVP)

A minimal Wayland taskbar scaffold using Rust + GTK4 + `gtk4-layer-shell`.

Quick orientation for future sessions: see `SESSION_NOTES.md`.
Agent collaboration contract: see `AGENTS.md`.

## Standardized Project Workflow

This repository now follows a predictable Rust app workflow:

- Pinned Rust channel via `rust-toolchain.toml`
- Lockfile-based builds (`Cargo.lock` + `--locked`)
- Canonical task entrypoints via `Makefile`
- CI validation in `.github/workflows/ci.yml`

## One-Time Setup

Install OS dependencies and Rust toolchain:

```bash
make deps
```

Generate a lockfile (commit it):

```bash
make lock
```

## Day-to-Day Commands

Build:

```bash
make build
```

Run:

```bash
make run
```

Checks:

```bash
make check
make fmt
make lint
make test
```

Run the same verification used in CI:

```bash
make ci
```

## Scripts

- `scripts/install-deps.sh`
  - Installs system dependencies for Arch/CachyOS and Debian/Ubuntu families
  - Installs/configures `rustup` and sets stable toolchain
- `scripts/build.sh`
  - Verifies `cargo`, `pkg-config`, and GTK4 dev files
  - Requires `Cargo.lock`
  - Runs `cargo build --locked`

## Features

- Bottom-anchored layer-shell bar
- Configurable horizontal layout with `left`, `center`, `right` areas
- Module types:
  - `workspaces` (default in `left`, via sway IPC)
  - `clock` (default in `right`, updates every second on GTK main loop)
  - `exec` (runs shell command periodically and displays output)
- Optional CSS loading from `./style.css` (ignored if missing)

## Runtime Notes

Run under a Wayland compositor. The workspace module expects sway IPC.

## Configuration

By default the app reads `./config.jsonc` if it exists. If it is missing or invalid, built-in defaults are used.

Example:

```jsonc
{
  areas: {
    left: [
      { type: "workspaces" },
      { type: "exec", command: "echo left-module", interval_secs: 10 }
    ],
    center: [
      { type: "exec", command: "echo center", interval_secs: 2, class: "center-text" }
    ],
    right: [
      { type: "clock", format: "%a %d. %b %H:%M:%S" }
    ]
  }
}
```

Module schema:

- `workspaces`: `{ "type": "workspaces" }`
- `clock`: `{ "type": "clock", "format": "%a %d. %b %H:%M:%S" }`
- `exec`: `{ "type": "exec", "command": "your shell command", "interval_secs": 5, "class": "optional-css-class" }`

## Styling

If `./style.css` exists, it is loaded at startup.

Suggested selectors:

- `.bar`
- `.left`
- `.menu-button`
- `.clock`

## Next Steps

- Multi-monitor output handling
- More module types and robust module lifecycle
- Better config discovery/reload
