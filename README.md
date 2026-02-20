# mybar

A minimal Wayland taskbar scaffold using Rust + GTK4 + `gtk4-layer-shell`.

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
- PulseAudio volume module with scroll-step volume control and Waybar-style formatting
- Default styling loaded from repo `style.css` (embedded at build time)

## Configuration And Styling

By default the app reads `./config.jsonc` if it exists. If it is missing or invalid, built-in defaults are used.

- Default example config: [`config.jsonc`](./config.jsonc)
- Full module configuration and module-specific styling selectors:
  - [`docs/modules.md`](./docs/modules.md)
- Base stylesheet loaded by default:
  - [`style.css`](./style.css)

## Expanded Docs

- Module configuration and styling selectors: [`docs/modules.md`](./docs/modules.md)
- Developer architecture and extension notes: [`docs/developer.md`](./docs/developer.md)

## Acknowledgements

- [Waybar](https://github.com/Alexays/Waybar) for long-running status bar design ideas and overall behavior references that influenced this project.
