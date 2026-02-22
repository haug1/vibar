# Developer Documentation

This document contains implementation-facing details that are intentionally kept out of `README.md`.

## Architecture

- Runtime module dispatch is string-keyed by `type` (for example `exec`, `clock`, `workspaces`, `pulseaudio`).
- `src/modules/mod.rs` stores raw module config entries:
  - `type: String`
  - module-specific fields as a dynamic map (`serde_json::Map<String, Value>`)
- Each module file owns:
  - its `MODULE_TYPE` constant
  - typed config struct
  - config parsing from raw map
  - widget initialization
- `modules::build_module(...)` finds the registered factory by module type and initializes it.
- `group` is a composite module that recursively calls `build_module(...)` for child entries.
- `exec` supports Waybar-compatible output parsing (`i3blocks` line mode and JSON `text`/`class`) and applies dynamic output classes each update.
- PulseAudio module uses native `libpulse` subscriptions/introspection (`src/modules/pulseaudio.rs`) rather than shelling out to `pactl`.
- Config loading prefers `~/.config/vibar/config.jsonc`, then falls back to `./config.jsonc`.
- Top-level style config supports layered CSS (`style.load-default` + `style.path`).

## Adding A Module

1. Create a module file under `src/modules/` (or subfolder like `src/modules/sway/`).
2. Add a `MODULE_TYPE` constant and typed config struct in that file.
3. Implement `ModuleFactory` for that module's factory.
4. Register the factory in `src/modules/mod.rs` `FACTORIES`.
5. For composite behavior, follow `src/modules/group.rs` (child module parsing + recursive build).
6. Add a `default_module_config()` helper if it should appear in built-in defaults.
7. Update docs/example config and run `make ci`.

## Troubleshooting

- To log sway workspace state each refresh, run with `VIBAR_DEBUG_WORKSPACES=1`.
  - Example: `VIBAR_DEBUG_WORKSPACES=1 cargo run --locked`
- To log tray DBus click method calls/errors, run with `VIBAR_DEBUG_TRAY=1`.
  - Example: `VIBAR_DEBUG_TRAY=1 cargo run --locked`
- To print the GTK widget tree with CSS classes for selector discovery, run with `VIBAR_DEBUG_DOM=1`.
  - Example: `VIBAR_DEBUG_DOM=1 cargo run --locked`

## Notes

- Keep lockfile-based builds (`--locked`) for reproducibility.
- Keep `README.md` concise and point to expanded docs in `docs/`.
- For modules with shell click actions, use `modules::attach_primary_click_command(...)` so click handler wiring and `.clickable` class behavior stay consistent.
