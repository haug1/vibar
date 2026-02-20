# Session Notes

Purpose: fast orientation for future coding sessions. Keep this concise and current.

## Project Snapshot

- Name: `vibar`
- Goal: minimal Wayland taskbar app (Rust + GTK4 + `gtk4-layer-shell`)
- Current status: project passes CI (`make ci`)
- Primary runtime target: sway/Wayland

## Core Behavior

- Bottom-anchored layer-shell bar
- One bar window per connected monitor at startup
- Non-focusable bar windows (`KeyboardMode::None`, no focus-on-click)
- 3 layout areas: `left`, `center`, `right`
- Config-driven modules (current set documented in `docs/modules.md`)
- Workspace module is output-local per monitor and event-driven via sway IPC
- PulseAudio module supports click actions, Waybar-style format keys, scroll volume, and event-driven native `libpulse` updates
- Default CSS loaded from `style.css` with translucent module styling and hover states
- Click-enabled label modules (`disk`, `pulseaudio`) add `.clickable` CSS class when click commands are configured

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
