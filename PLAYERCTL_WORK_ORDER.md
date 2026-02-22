# Playerctl Work Order

Active execution checklist for the remaining `playerctl` upgrades.
Keep this file current as slices are completed.

## Status

- [x] Slice 1 complete: event-driven backend/state model
- [x] Slice 2 complete: dynamic visibility + state CSS classes
- [ ] Slice 3 pending: controls popover + seek slider
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

- [ ] `controls.enabled` (bool, default `false`)
- [ ] `controls.open` trigger mode (initial scope: left-click)
- [ ] `controls.show_seek` (bool, default `true`)

Tasks:

- [ ] Replace bare label root with container suitable for popover anchor
- [ ] Add popover with buttons: previous, play/pause, next
- [ ] Wire buttons to MPRIS methods (`Previous`, `PlayPause`, `Next`)
- [ ] Add seek slider bound to `position` / `duration`
- [ ] Implement precise seek via `SetPosition` (guard with `CanSeek`)
- [ ] Prevent slider feedback loops while scrubbing
- [ ] Keep legacy label behavior when controls are disabled

Acceptance criteria:

- [ ] Controls work with at least one common player (Spotify/mpv)
- [ ] Seek interactions are stable and precise
- [ ] Popover behavior does not break bar layout
- [ ] `make ci` passes

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

- [ ] Trigger semantics when controls are enabled:
  - replace existing `click` command behavior, or
  - coexist via separate trigger mapping
- [ ] Multi-player control target policy when several players are active
- [ ] Whether paused state should keep seek slider interactive by default

## Notes for Next Session

- Keep changes incremental and commit per slice.
- Preserve backward compatibility for current module keys unless intentionally versioned.
- Avoid destructive git/file operations.
