# vibar

A minimal Wayland taskbar using Rust + GTK4 + `gtk4-layer-shell`.

Quick orientation for future sessions: `SESSION_NOTES.md`.
Agent collaboration contract: `AGENTS.md`.

## Getting Started

Install OS dependencies and Rust toolchain:

```bash
make deps
```

Generate a lockfile (commit it):

```bash
make lock
```

## Build And Run

```bash
make build
make run
```

## Verification

Run individual checks:

```bash
make check
make fmt
make lint
make test
```

Run CI-equivalent checks:

```bash
make ci
```

## Features

- Bottom-anchored layer-shell bar
- One bar window per connected monitor at startup
- Configurable horizontal layout with `left`, `center`, `right` areas
- Configurable module system
- Playerctl module with event-driven MPRIS metadata/status, optional idle auto-hide, and optional left-click controls popover (transport + seek)
- Exec module supports Waybar-compatible output parsing (i3blocks lines + JSON `text`/`class`)
- CPU module with configurable polling interval, format placeholders, and default usage-level CSS classes
- Memory module with configurable polling interval and format placeholders (including `{used_percentage}`)
- Group module for logical submodule grouping and optional drawer-style expansion
- Native PulseAudio module with event-driven updates and scroll-step volume control
- Config file search order:
  - `~/.config/vibar/config.jsonc`
  - `./config.jsonc` (fallback)
- CSS layering support:
  - embedded default `style.css`
  - optional user CSS loaded on top
  - default CSS can be disabled via config flag
  - built-in utility classes for module chrome (`v-pill`, `v-square`)

## Configuration And Styling

Config is loaded in this order:

1. `~/.config/vibar/config.jsonc`
2. `./config.jsonc`

If no candidate exists (or both are invalid), built-in defaults are used.

Styling config supports:

- loading embedded default `style.css` (`style.load-default`, default `true`)
- loading a user CSS file (`style.path`) after default CSS for overrides
- disabling embedded defaults and using only user CSS

- Default example config: [`config.jsonc`](./config.jsonc)
- Full module configuration and module-specific styling selectors:
  - [`docs/modules.md`](./docs/modules.md)
- Base stylesheet loaded by default:
  - [`style.css`](./style.css)

## Expanded Docs

- Module configuration and styling selectors: [`docs/modules.md`](./docs/modules.md)
- Developer architecture and extension notes: [`docs/developer.md`](./docs/developer.md)
  - Includes debug env vars such as `VIBAR_DEBUG_DOM=1` for widget/CSS selector discovery.

## Acknowledgements

- [Waybar](https://github.com/Alexays/Waybar) for long-running status bar design ideas and overall behavior references that influenced this project.
