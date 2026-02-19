# Session Notes

Purpose: fast orientation for future coding sessions. Keep this concise and current.

## Project Snapshot

- Name: `mybar`
- Goal: minimal Wayland taskbar app (Rust + GTK4 + `gtk4-layer-shell`)
- Current status: MVP scaffold passes CI (`make ci`)
- Primary runtime target: sway/Wayland

## Core Behavior (MVP)

- Bottom-anchored layer-shell bar
- 3 layout areas: `left`, `center`, `right`
- Default modules:
  - `left`: sway workspaces
  - `right`: clock (updates every 1s)
- Config-driven `exec` modules via `config.jsonc`
  - `exec.interval_secs` defaults to `5` and is clamped to a minimum of `1`
- Default CSS comes from repo `style.css` (embedded at build time)
- Workspaces active detection prefers sway `get_tree()` focus with fallback to `get_workspaces().focused`
- Workspace debug logging can be enabled with `MYBAR_DEBUG_WORKSPACES=1`

## Standard Commands

- Install deps: `make deps`
- Generate lockfile: `make lock`
- Build: `make build`
- Run: `make run`
- CI-equivalent checks: `make ci`

## Key Files

- App entry: `src/main.rs`
- Rust deps: `Cargo.toml`
- Lockfile: `Cargo.lock`
- Toolchain pin: `rust-toolchain.toml`
- Task runner: `Makefile`
- OS/bootstrap scripts: `scripts/install-deps.sh`, `scripts/build.sh`
- CI: `.github/workflows/ci.yml`
- User config example: `config.jsonc`

## Conventions

- Use lockfile-based builds (`--locked`) for reproducibility
- Keep MVP changes small and testable
- Prefer config/module iteration over large rewrites
- Preserve concise docs and predictable command flow

## Maintenance Policy

- This file must be updated by the coding agent whenever session-critical context changes.
- Keep only durable, high-signal information (no long logs, no transient chatter).
- If a section becomes stale, correct it in the same PR/commit as code changes.
