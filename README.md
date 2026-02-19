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
- One bar window per connected monitor at startup
- Configurable horizontal layout with `left`, `center`, `right` areas
- Module types:
  - `workspaces` (default in `left`, via sway IPC; updates immediately on workspace/output events)
  - `clock` (default in `right`, updates every second on GTK main loop)
  - `exec` (runs shell command periodically and displays output, minimum interval is 1 second)
- Default styling loaded from repo `style.css` (embedded at build time)

## Runtime Notes

Run under a Wayland compositor. The workspace module expects sway IPC and subscribes to sway events for immediate updates.

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
  - `interval_secs` defaults to `5` and values below `1` are clamped to `1`

## Module Architecture

- Runtime module dispatch is string-keyed by `type` (e.g. `exec`, `clock`, `workspaces`).
- `src/modules/mod.rs` stores raw module config entries:
  - `type: String`
  - module-specific fields as a dynamic map (`serde_json::Map<String, Value>`)
- Each module file owns:
  - its type constant (e.g. `MODULE_TYPE`)
  - typed config struct
  - config parsing from raw map
  - widget initialization
- `modules::build_module(...)` finds the registered factory by module type and initializes it.

## Adding A Module

1. Create a module file under `src/modules/` (or subfolder like `src/modules/sway/`).
2. Add a `MODULE_TYPE` constant and typed config struct in that file.
3. Implement `ModuleFactory` for that module's factory:
   - `module_type()` returns `MODULE_TYPE`
   - `init()` parses the raw config map and builds the widget
4. Register the factory in `src/modules/mod.rs` `FACTORIES`.
5. Add a `default_module_config()` helper if it should appear in built-in defaults.
6. Update docs/example config and run `make ci`.

## Styling

`style.css` in the repository is the default maintained theme and is always loaded.
Keep this file up to date when module classes or interaction states change.

Suggested selectors:

- `.bar`
- `.left`
- `.menu-button`
- `.menu-button.active`
- `.menu-button.workspace-active`
- `.clock`

## Troubleshooting

- To log sway workspace state each refresh, run with `MYBAR_DEBUG_WORKSPACES=1`.
  - Example: `MYBAR_DEBUG_WORKSPACES=1 cargo run --locked`

## Next Steps

- Dynamic monitor hotplug handling (add/remove bars at runtime)
- More module types and robust module lifecycle
- Better config discovery/reload
