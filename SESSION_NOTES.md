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
- Config-driven module system (canonical reference: `docs/modules.md`)
- Sway integration includes `sway/workspaces`, `sway/mode`, and `sway/window` (focused window title gated per output)
- `sway/window` supports Pango-markup `format` templates via `{}` / `{title}` placeholders
- `sway/workspaces` supports container `class` plus per-button `button-class` style overrides
- `sway/mode`, `clock`, `playerctl`, `cpu`, `memory`, `disk`, and `pulseaudio` support Pango markup in format fields (with escaped placeholder values)
- `backlight` module reads `/sys/class/backlight`, supports `format-icons` + optional explicit `device`, hides when panel power is reported off, and updates via udev callbacks with interval-based fallback/resync
- `backlight` also supports Pango-markup `format` templates with `{percent}`, `{icon}`, `{brightness}`, `{max}`, and `{device}` placeholders
- `backlight` supports scroll brightness control (`scroll-step`, `min-brightness`) via logind DBus by default, with optional `on-scroll-up` / `on-scroll-down` command overrides
- `exec` supports Pango-markup `format` templates with `{}` / `{text}` placeholders and top-level JSON property placeholders
- `playerctl` supports `max-width` (caps width while shrinking to content when short)
- `playerctl` shows a visible `…` cue when text is truncated
- `playerctl` hover tooltip appears only when text is truncated
- Config lookup order: `~/.config/vibar/config.jsonc` then `./config.jsonc`
- Embedded default CSS can be layered with optional user CSS (`style.path`)
- `style.load-default` can disable embedded default CSS

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

## Troubleshooting Flags

- `VIBAR_DEBUG_WORKSPACES=1`: log sway workspace refresh state
- `VIBAR_DEBUG_TRAY=1`: log tray DBus click method calls/errors
- `VIBAR_DEBUG_DOM=1`: print GTK widget tree + CSS classes at startup and periodically
- `VIBAR_DEBUG_DOM_INTERVAL_SECS=<n>`: override DOM dump interval (minimum `1`, default `10`)

## Conventions

- Use lockfile-based builds (`--locked`) for reproducibility
- Keep changes small and testable
- Prefer config/module iteration over large rewrites
- Preserve concise docs and predictable command flow

## Maintenance Policy

- This file must be updated by the coding agent whenever session-critical context changes.
- Keep only durable, high-signal information (no long logs, no transient chatter).
- If a section becomes stale, correct it in the same PR/commit as code changes.
