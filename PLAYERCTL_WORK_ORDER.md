# Playerctl Work Order

This file is the execution checklist for upgrading the `playerctl` module.
Update checkbox status as work progresses.

## Scope

Top priorities from current work order:

- [ ] Event-driven updates (no polling loop for metadata/state changes)
- [ ] Playback controls UI (play/pause, previous, next, seek slider)
- [ ] Dynamic visibility (show when active, hide when idle/no player)

Out of scope for first pass:

- [ ] Player volume adjustment from this module (track as follow-up)

## Delivery Strategy

Implement in small, reviewable slices. Do **not** one-shot all features.

- [ ] Slice 1: Event-driven backend/state model
- [ ] Slice 2: Dynamic visibility wiring
- [ ] Slice 3: Popover controls + seeking slider
- [ ] Slice 4: Hardening/tests/docs cleanup

## Slice 1: Event-Driven Backend

Goal: Replace interval polling with event-driven MPRIS updates.

Tasks:

- [ ] Introduce backend state model struct (status, player id, title/artist/album, position, duration, capability flags)
- [ ] Implement DBus watcher using existing `zbus` dependency (avoid shelling out to `playerctl --follow`)
- [ ] Listen for player lifecycle and metadata/status changes (`NameOwnerChanged`, `PropertiesChanged`)
- [ ] Select active player deterministically when multiple players exist (document strategy)
- [ ] Push state updates to GTK thread via channel
- [ ] Render label text from state placeholders
- [ ] Preserve existing click command support and CSS class behavior

Acceptance criteria:

- [ ] Metadata/status updates happen without periodic metadata polling
- [ ] No regressions in existing module config parsing
- [ ] `make ci` passes

## Slice 2: Dynamic Visibility

Goal: Module hides when inactive and reappears when active.

Config additions:

- [ ] `hide_when_idle` (bool, default `false`)
- [ ] Optional `show_when_paused` (bool, default `true`) for behavior tuning

Tasks:

- [ ] Apply visibility rules from live state
- [ ] Hide when no active player or stopped (per config)
- [ ] Ensure widget reappears immediately on activity
- [ ] Add CSS class toggles for state (`status-playing`, `status-paused`, `status-stopped`, `no-player`)

Acceptance criteria:

- [ ] Module visibility changes without restart
- [ ] Works across player start/stop transitions
- [ ] `make ci` passes

## Slice 3: Controls Popover + Seek Slider

Goal: Add optional on-click controls UI with transport controls and precise seeking.

Config additions:

- [ ] `controls.enabled` (bool, default `false`)
- [ ] `controls.open` trigger mode (initial: left-click only)
- [ ] `controls.show_seek` (bool, default `true`)

Tasks:

- [ ] Replace bare label root with container suitable for popover anchor
- [ ] Add popover with buttons: previous, play/pause, next
- [ ] Wire buttons to MPRIS methods (`Previous`, `PlayPause`, `Next`)
- [ ] Add seek slider bound to position/duration
- [ ] Implement precise seek via `SetPosition` and guard with `CanSeek`
- [ ] Prevent slider feedback loops while scrubbing
- [ ] Keep label mode functional when controls disabled

Acceptance criteria:

- [ ] Controls work on at least one common player (e.g. Spotify/mpv)
- [ ] Seek interactions are stable and precise
- [ ] Popover UX doesn’t break bar layout
- [ ] `make ci` passes

## Slice 4: Hardening and Documentation

Tasks:

- [ ] Add/expand unit tests for parsing/rendering/visibility logic
- [ ] Add focused integration-ish tests for state transition handling where feasible
- [ ] Update `docs/modules.md` with new config keys and behavior
- [ ] Update `README.md` feature summary
- [ ] Update `docs/developer.md` architecture notes
- [ ] Update `SESSION_NOTES.md` with final capabilities
- [ ] Refresh `config.jsonc` example if defaults/schema changed

Acceptance criteria:

- [ ] `make ci` passes
- [ ] Docs/config/examples remain in sync

## Implementation Notes (for agent)

- Keep changes incremental and commit per slice.
- Prefer existing module patterns (`parse_config`, `ModuleFactory`, GTK main-thread updates).
- Keep click command support (`click` / `on-click`) compatible unless explicitly replaced by controls mode.
- Avoid destructive git operations.
- If unexpected unrelated local changes appear, stop and ask user.

## Open Questions (resolve before Slice 3)

- [ ] Should controls popover replace `click` command when enabled, or can both coexist via separate triggers?
- [ ] Desired active-player policy when multiple players are running (most recent, playing-first, config-specified)
- [ ] Exact idle definition for visibility (no player vs paused vs stopped)
