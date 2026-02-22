# Playerctl Work Order

Active execution checklist for the remaining `playerctl` upgrades.
Keep this file current as slices are completed.

## Status

- [x] Slice 1 complete: event-driven backend/state model
- [x] Slice 2 complete: dynamic visibility + state CSS classes
- [x] Slice 3 complete: controls popover + seek slider
- [ ] Slice 4 pending: hardening + docs/config sync

## Current Behavior (implemented)

- Event-driven updates via DBus (`NameOwnerChanged` + `PropertiesChanged`)
- Active player auto-selection policy: `playing` > `paused` > `stopped`, then stable name sort
- Dynamic visibility config:
  - `hide-when-idle` / `hide_when_idle` (default `false`)
  - `show-when-paused` / `show_when_paused` (default `true`)
- State CSS classes on module label:
  - `status-playing`
  - `status-paused`
  - `status-stopped`
  - `no-player`

## Slice 3: Controls Popover + Seek Slider

Goal: Optional on-click controls UI with transport controls and precise seeking.

Config additions:

- [x] `controls.enabled` (bool, default `false`)
- [x] `controls.open` trigger mode (initial scope: left-click)
- [x] `controls.show_seek` (bool, default `true`)

Tasks:

- [x] Replace bare label root with container suitable for popover anchor
- [x] Add popover with buttons: previous, play/pause, next
- [x] Wire buttons to MPRIS methods (`Previous`, `PlayPause`, `Next`)
- [x] Add seek slider bound to `position` / `duration`
- [x] Implement precise seek via `SetPosition` (guard with `CanSeek`)
- [x] Prevent slider feedback loops while scrubbing
- [x] Keep legacy label behavior when controls are disabled

Acceptance criteria:

- [ ] Controls work with at least one common player (Spotify/mpv) (manual runtime validation pending)
- [x] Seek interactions are stable and precise
- [ ] Popover behavior does not break bar layout (manual runtime validation pending)
- [x] `make ci` passes

## Slice 4: Hardening + Docs

Tasks:

- [ ] Add/expand tests for rendering, state transition, and seek behavior logic
- [ ] Update `docs/modules.md` for all new `playerctl` keys and classes
- [ ] Update `README.md` feature summary
- [ ] Update `docs/developer.md` architecture notes
- [ ] Update `SESSION_NOTES.md` final capability summary
- [ ] Update `config.jsonc` example if schema/defaults changed

Acceptance criteria:

- [ ] `make ci` passes
- [ ] Docs, examples, and implementation are in sync

## Open Decisions Before Slice 3

- [x] Trigger semantics when controls are enabled:
  - implemented as left-click popover trigger (`controls.open=left-click`) while preserving legacy `click`/`on-click` behavior when controls are disabled
- [ ] Multi-player control target policy when several players are active
- [ ] Whether paused state should keep seek slider interactive by default

## Notes for Next Session

- Keep changes incremental and commit per slice.
- Preserve backward compatibility for current module keys unless intentionally versioned.
- Avoid destructive git/file operations.
