# Session Notes

Purpose: fast orientation for future coding sessions. Keep this concise and current.

## Project Snapshot

- Name: `mybar`
- Goal: minimal Wayland taskbar app (Rust + GTK4 + `gtk4-layer-shell`)
- Current status: MVP scaffold passes CI (`make ci`)
- Primary runtime target: sway/Wayland

## Core Behavior (MVP)

- Bottom-anchored layer-shell bar
- One bar window per connected monitor at startup
- 3 layout areas: `left`, `center`, `right`
- Default modules:
  - `left`: sway workspaces
  - `right`: clock (updates every 1s)
- Config-driven `exec` modules via `config.jsonc`
  - `exec.interval_secs` defaults to `5` and is clamped to a minimum of `1`
  - Identical `exec` modules (`command` + `interval_secs`) share one background poller across all monitor windows
- Config-driven `tray` module via `config.jsonc`
  - polls `org.kde.StatusNotifierWatcher` for tray items
  - renders icon-name based tray buttons
  - left click calls item `Activate`
  - right click prefers host-rendered DBusMenu (`Menu` + `com.canonical.dbusmenu`, including nested submenus) with fallback to item click methods
  - middle click calls item `SecondaryActivate`
  - `icon_size` defaults to `16` (min `8`), `poll_interval_secs` defaults to `2` (min `1`)
- Default CSS comes from repo `style.css` (embedded at build time)
- Workspaces active detection prefers sway `get_tree()` focus with fallback to `get_workspaces().focused`
- Workspace module subscribes to sway workspace/output events for event-driven refresh
- Workspace debug logging can be enabled with `MYBAR_DEBUG_WORKSPACES=1`
- Tray DBus click/method debug logging can be enabled with `MYBAR_DEBUG_TRAY=1`

## Standard Commands

- Install deps: `make deps`
- Generate lockfile: `make lock`
- Build: `make build`
- Run: `make run`
- CI-equivalent checks: `make ci`

## Key Files

- App entry: `src/main.rs`
- Config parsing/models: `src/config.rs`
- Style loading: `src/style.rs`
- Module builders: `src/modules/`
  - `ModuleConfig` is string-keyed (`type` + raw config map) in `src/modules/mod.rs`.
  - Each module file owns typed config parsing + widget init behind `ModuleFactory`.
  - `modules::build_module` resolves factory by module type and initializes dynamically.
- Tray module: `src/modules/tray.rs`
- Workspace module: `src/modules/sway/workspace.rs`
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
