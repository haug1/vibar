# vibar

<img width="2560" height="45" alt="image" src="https://github.com/user-attachments/assets/1e79efb0-39db-4db8-9c77-c47ff62d789f" />

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

## Documentation

- Module configuration and styling selectors: [`docs/modules.md`](./docs/modules.md)
- Developer architecture and extension notes: [`docs/developer.md`](./docs/developer.md)
- Default example config: [`config.jsonc`](./config.jsonc)
- Base stylesheet loaded by default: [`style.css`](./style.css)

## Features

- Bottom-anchored layer-shell bar
- One bar window per connected monitor at startup
- Configurable horizontal layout with `left`, `center`, `right` areas
- Configurable module system (`docs/modules.md`)
- Sway modules for workspaces, active mode, and active-window title (`sway/workspaces`, `sway/mode`, `sway/window`)
- Workspace module supports container and per-workspace-button CSS class overrides
- Playerctl supports `max-width` display mode for adaptive title width
- Playerctl shows a truncation cue when text is clipped
- Playerctl hover tooltip appears only when text is clipped
- Backlight module with Waybar-style `format-icons`, optional `device` selection, and `/sys/class/backlight` polling
- Config file search order:
  - `~/.config/vibar/config.jsonc`
  - `./config.jsonc` (fallback)
- CSS layering support:
  - embedded default `style.css`
  - optional user CSS loaded on top
  - default CSS can be disabled via `style.load-default`

## Troubleshooting

- If text updates leave tiny font/glyph dots, it may help to set an explicit `line-height` on the affected module class (for example `line-height: 1.5;`).

## Acknowledgements

- [Waybar](https://github.com/Alexays/Waybar) for long-running status bar design ideas and overall behavior references that influenced this project.

## License

MIT. See [`LICENSE`](./LICENSE).
