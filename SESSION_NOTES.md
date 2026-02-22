# Session Notes

Purpose: fast orientation for future coding sessions. Keep this concise and current.

## Project Snapshot

- Name: `vibar`
- Goal: minimal Wayland taskbar app (Rust + GTK4 + `gtk4-layer-shell`)
- Current status: project passes CI (`make ci`)
- Primary runtime target: sway/Wayland
- CI build environment: GitHub Actions Ubuntu runner with Fedora 41 container for consistent GTK4 layer-shell dev packages

## Core Behavior

- Bottom-anchored layer-shell bar
- One bar window per connected monitor at startup
- Non-focusable bar windows (`KeyboardMode::None`, no focus-on-click)
- 3 layout areas: `left`, `center`, `right`
- Config-driven modules (current set documented in `docs/modules.md`)
- Group module supports nested modules for shared styling and optional drawer reveal behavior
- Config lookup order: `~/.config/vibar/config.jsonc` then `./config.jsonc`
- Workspace module is output-local per monitor and event-driven via sway IPC
- Clock module supports optional `click` / `on-click` shell actions
- PulseAudio module supports click actions, Waybar-style format keys, scroll volume, and event-driven native `libpulse` updates
- Exec module supports optional `click` / `on-click` shell actions
- Exec module parses Waybar-compatible output (`i3blocks` line format + JSON `text`/`class`) and applies dynamic CSS classes from output
- Embedded default CSS can be layered with optional user CSS (`style.path`)
- `style.load-default` can disable embedded default CSS
- Shared helper `modules::attach_primary_click_command(...)` centralizes click-command wiring and `.clickable` CSS class behavior across modules
- `VIBAR_DEBUG_DOM=1` prints widget tree + CSS classes at startup and periodically (default 10s); interval override via `VIBAR_DEBUG_DOM_INTERVAL_SECS`

## Standard Commands

- Install deps: `make deps`
- Generate lockfile: `make lock`
- Build: `make build`
- Run: `make run`
- CI-equivalent checks: `make ci`

## Docs And Entry Points

- User docs: `README.md`
- Module config and styling: `docs/modules.md`
- Developer architecture/extension workflow: `docs/developer.md`
- Example config: `config.jsonc`
- App entry: `src/main.rs`
- Module registry and dispatch: `src/modules/mod.rs`

## Conventions

- Use lockfile-based builds (`--locked`) for reproducibility
- Keep changes small and testable
- Prefer config/module iteration over large rewrites
- Preserve concise docs and predictable command flow

## Maintenance Policy

- This file must be updated by the coding agent whenever session-critical context changes.
- Keep only durable, high-signal information (no long logs, no transient chatter).
- If a section becomes stale, correct it in the same PR/commit as code changes.
