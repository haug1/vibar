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
- Configurable module system (`docs/modules.md`)
- Config file search order:
  - `~/.config/vibar/config.jsonc`
  - `./config.jsonc` (fallback)
- CSS layering support:
  - embedded default `style.css`
  - optional user CSS loaded on top
  - default CSS can be disabled via `style.load-default`

## Documentation

- User-facing setup and commands: `README.md`
- Session continuity notes: `SESSION_NOTES.md`
- Module configuration and styling selectors: [`docs/modules.md`](./docs/modules.md)
- Developer architecture and extension notes: [`docs/developer.md`](./docs/developer.md)
- Default example config: [`config.jsonc`](./config.jsonc)
- Base stylesheet loaded by default: [`style.css`](./style.css)

## Acknowledgements

- [Waybar](https://github.com/Alexays/Waybar) for long-running status bar design ideas and overall behavior references that influenced this project.
