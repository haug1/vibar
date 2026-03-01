# Developer Documentation

This document contains implementation-facing details that are intentionally kept out of `README.md`.

## Architecture

Module behavior, config fields, and styling selectors are documented in `docs/modules.md` (the canonical reference). This section covers code structure and implementation decisions only.

### Module System

- Runtime module dispatch is string-keyed by `type`.
- `src/modules/mod.rs` stores raw module config entries (`type: String` + dynamic `serde_json::Map`) and the `FACTORIES` registry.
- Each module file (or module directory) owns its `MODULE_TYPE` constant, typed config struct, config parsing, and widget initialization.
- `modules::build_module(...)` finds the registered factory by type and initializes it.
- `group` (`src/modules/group.rs`) is a composite module that recursively calls `build_module(...)` for child entries.

### Implementation Details

- `playerctl` layout: `src/modules/playerctl/mod.rs` (orchestration), `config.rs` (schema/defaults), `backend.rs` (MPRIS DBus via `zbus`), `model.rs` (pure metadata/format helpers), `ui.rs` (GTK tooltip/carousel/controls UI wiring).
- `pulseaudio` layout: `src/modules/pulseaudio/mod.rs` (factory/orchestration + render glue), `config.rs` (schema/defaults), `format.rs` (icon selection helpers), `backend.rs` (native `libpulse` session/query/mutator loop), `ui.rs` (GTK controls popover/widget refresh logic).
- `backlight` and `battery` use `udev` callbacks as primary update trigger with immediate GTK main-thread dispatch.

## Adding A Module

1. Create a module file under `src/modules/` (or subfolder like `src/modules/sway/`).
2. Add a `MODULE_TYPE` constant and typed config struct in that file.
3. Implement `ModuleFactory` for that module's factory.
4. Register the factory in `src/modules/mod.rs` `FACTORIES`.
5. For composite behavior, follow `src/modules/group.rs` (child module parsing + recursive build).
6. Add a `default_module_config()` helper if it should appear in built-in defaults.
7. Update docs/example config and run `make ci`.

## Troubleshooting

Debug environment variables (combine with `cargo run --locked`):

- `VIBAR_DEBUG_WORKSPACES=1` — log sway workspace state each refresh.
- `VIBAR_DEBUG_TRAY=1` — log tray DBus calls, discovery, and errors.
- `VIBAR_DEBUG_DOM=1` — dump GTK widget tree + CSS classes at startup and periodically. Override interval with `VIBAR_DEBUG_DOM_INTERVAL_SECS=<n>`.

## Notes

- For modules with shell click actions, use `modules::attach_primary_click_command(...)` for left click (`.clickable` class + gesture wiring), and `modules::attach_secondary_click_command(...)` for optional right-click command wiring.
- OS package requirements are centralized in `scripts/install-deps.sh`; CI installs system dependencies via that script to avoid drift.
- Dependabot config lives in `.github/dependabot.yml`; auto-merge workflow in `.github/workflows/dependabot-automerge.yml`. See `README.md` for the full dependency automation policy.
