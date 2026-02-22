# Module Configuration

This document is the canonical configuration reference for all currently supported module types.

## Config Shape

Top-level config uses three layout areas:

```jsonc
{
  "style": {
    "load-default": true,
    "path": "~/.config/vibar/style.css"
  },
  "areas": {
    "left": [{ "type": "sway/workspaces" }],
    "center": [{ "type": "playerctl", "format": "{status_icon} {artist} - {title}" }],
    "right": [
      {
        "type": "group",
        "class": "media-group",
        "drawer": true,
        "modules": [{ "type": "pulseaudio" }, { "type": "tray" }]
      },
      { "type": "disk", "format": "{free} \uf0a0 ", "click": "dolphin" },
      { "type": "cpu", "format": "{used_percentage}% ", "interval_secs": 1 },
      { "type": "clock" }
    ]
  }
}
```

Each entry in an area is a module object with a required `"type"` key.

## Styling Overview

CSS loading behavior:

- Embedded `style.css` is loaded by default.
- Optional user CSS file can be loaded via top-level `style.path`.
- User CSS is loaded after default CSS, so it can override default rules.
- Set top-level `style.load-default` to `false` to disable embedded default CSS.
- Relative `style.path` values resolve from the selected config file directory.

Common layout selectors:

- `.bar`
- `.left`
- `.center`
- `.right`
- `.module` (base module label styling and default opacity)
- `.module.clickable` (applied when a module has left-click actions; interaction state only)

Built-in utility classes (optional):

- `.v-pill`: applies pill-style module chrome (background, border, radius, padding).
- `.v-square`: same chrome style with square corners (`border-radius: 0`).

## `group`

Schema:

```json
{
  "type": "group",
  "class": "optional-css-classes",
  "spacing": 6,
  "modules": [{ "type": "pulseaudio" }, { "type": "tray" }],
  "drawer": {
    "label-closed": "",
    "label-open": "",
    "start-open": false
  }
}
```

Fields:

- `modules` (required): child modules rendered inside the group.
- `children` (optional alias): alias for `modules`.
- `class` (optional): extra CSS class(es) on the group container (whitespace-separated).
- `spacing` (optional): spacing in px between child modules.
  - Default: `6`
  - Minimum: `0` (values below are clamped)
- `drawer` (optional): if set, child modules render inside a revealable drawer.
  - `true`: enable drawer with defaults.
  - object form supports:
    - `label-closed` / `label_closed` (optional): toggle label when collapsed.
      - Default: ``
    - `label-open` / `label_open` (optional): toggle label when expanded.
      - Default: ``
    - `start-open` / `start_open` (optional): initial drawer state.
      - Default: `false`

Behavior:

- Logical grouping container for submodules.
- Group container can be styled as one unit while preserving child module behavior.
- With `drawer` enabled, child modules are shown in a popover positioned above the bar toggle (context-menu style).
- Drawer popover content is vertical.
- Child module initialization errors include the failing child index.
- Group modules can be nested.

Styling:

- Group container classes: `.module.group`
- Drawer-enabled group class: `.group-drawer`
- Drawer toggle button class: `.group-toggle`
- Drawer popover class: `.group-popover`
- Child row container class: `.group-content`
- Optional extra class via `class` field.

## `sway/workspaces`

Minimal schema:

```json
{ "type": "sway/workspaces" }
```

Behavior:

- Sway IPC workspace module.
- Updates on workspace/output events (event-driven refresh).
- On multi-monitor setups, each bar window shows only workspaces for its output.
- Clicking a workspace button focuses that workspace in sway.

Styling:

- Container classes: `.module.workspaces`
- Per-workspace button class: `.menu-button`
- Active state classes: `.menu-button.active`, `.menu-button.workspace-active`

## `clock`

Schema:

```json
{
  "type": "clock",
  "format": "%a %d. %b %H:%M:%S",
  "click": "optional shell command"
}
```

Fields:

- `format` (optional): `chrono` format string.
  - Default: `%a %d. %b %H:%M:%S`
- `click` (optional): shell command run on left click.
- `on-click` (optional): alias for `click` (Waybar-style key).

Behavior:

- Updates every second on GTK main loop.

Styling:

- Label classes: `.module.clock`
- Click-enabled labels also include: `.clickable`

## `playerctl`

Schema:

```json
{
  "type": "playerctl",
  "format": "{status_icon} {title}",
  "player": "spotify",
  "no_player_text": "No media",
  "hide-when-idle": true,
  "show-when-paused": true,
  "controls": {
    "enabled": true,
    "open": "left-click",
    "show_seek": true
  },
  "class": "optional-css-classes"
}
```

Fields:

- `format` (optional): output format template.
  - Default: `{status_icon} {title}`
- `player` (optional): player selector passed to `playerctl --player <name>`.
- `interval_secs` (optional): polling interval in seconds.
  - Default: `1`
  - Note: kept for backward-compatibility; ignored by event-driven backend.
- `no_player_text` (optional): text shown when no matching player is available.
  - Default: `No media`
- `hide-when-idle` / `hide_when_idle` (optional): hide module when idle/no player.
  - Default: `false`
- `show-when-paused` / `show_when_paused` (optional): when `hide-when-idle=true`, keep module visible while paused.
  - Default: `true`
- `click` (optional): shell command run on left click.
- `on-click` (optional): alias for `click` (Waybar-style key).
- `controls` (optional): popover controls UI configuration.
  - `enabled` (optional): enable left-click popover controls (`Previous`, `PlayPause`, `Next`).
    - Default: `false`
  - `open` (optional): trigger mode for opening controls popover.
    - Supported values: `left-click`
    - Default: `left-click`
  - `show_seek` (optional): show/hide seek slider in the controls popover.
    - Default: `true`
- `class` (optional): extra CSS class(es) on the module widget (whitespace-separated).

Format placeholders:

- `{status}`
- `{status_icon}`
- `{player}`
- `{artist}`
- `{album}`
- `{title}`

Behavior:

- Event-driven updates from MPRIS over DBus (`NameOwnerChanged` + `PropertiesChanged`).
- Active player selection policy: `playing` > `paused` > `stopped`, then stable bus-name sort.
- If no matching player exists, module text falls back to `no_player_text`.
- When `controls.enabled=true`, left-click opens a popover with transport buttons and optional seek slider.
- Seek writes use MPRIS `SetPosition` (guarded by `CanSeek`, track id presence, and positive duration).
- Slider updates ignore backend refresh while scrubbing to avoid seek feedback loops.
- When `controls.enabled=false`, click behavior remains legacy (`click` / `on-click` command).
- Status icon defaults:
  - `playing` -> ``
  - `paused` -> ``
  - `stopped` -> ``
  - fallback -> ``

Styling:

- Label classes: `.module.playerctl`
- State classes: `.status-playing`, `.status-paused`, `.status-stopped`, `.no-player`
- Click-enabled modules include: `.clickable` (both shell-click and controls-enabled cases)
- Controls popover classes: `.playerctl-controls-popover`, `.playerctl-controls-content`, `.playerctl-controls-row`, `.playerctl-control-button`, `.playerctl-seek-scale`
- Optional extra class via `class` field.

## `exec`

Schema:

```json
{
  "type": "exec",
  "command": "your shell command",
  "click": "optional shell command",
  "interval_secs": 5,
  "class": "optional-css-classes"
}
```

Fields:

- `command` (required): shell command executed with `sh -c`.
- `click` (optional): shell command run on left click.
- `on-click` (optional): alias for `click` (Waybar-style key).
- `interval_secs` (optional): polling interval in seconds.
  - Default: `5`
  - Minimum: `1` (values below are clamped)
- `class` (optional): extra CSS class(es) on the module label (whitespace-separated).

Behavior:

- Shows command output in a label.
- If stdout is empty, stderr is used as fallback text.
- Output parsing is Waybar-compatible:
  - i3blocks style (default): line 1 = text, line 2 = tooltip (ignored), line 3 = CSS class list.
  - JSON style: if output is valid JSON, `text` and `class` fields are used (`class` supports string or string array).
- Identical `command` + `interval_secs` instances share one backend poller across bar windows.

Styling:

- Label classes: `.module.exec`
- Click-enabled labels also include: `.clickable`
- Optional extra class via `class` field.

## `disk`

Schema:

```json
{
  "type": "disk",
  "format": "{free} \uf0a0 ",
  "click": "dolphin",
  "path": "/",
  "interval_secs": 30,
  "class": "optional-css-classes"
}
```

Fields:

- `format` (optional): output format template.
  - Default: `{free}`
- `click` (optional): shell command run on left click.
- `on-click` (optional): alias for `click` (Waybar-style key).
- `path` (optional): filesystem path passed to `df`.
  - Default: `/`
- `interval_secs` (optional): polling interval in seconds.
  - Default: `30`
  - Minimum: `1` (values below are clamped)
- `class` (optional): extra CSS class(es) on the module label (whitespace-separated).

Format placeholders:

- `{free}`
- `{used}`
- `{total}`
- `{path}`
- `{percentage_free}`
- `{percentage_used}`

Behavior:

- Polls disk stats with `df -B1 -P <path>`.
- Values are rendered in binary units (`B`, `K`, `M`, `G`, `T`, `P`).

Styling:

- Label classes: `.module.disk`
- Click-enabled labels also include: `.clickable`
- Optional extra class via `class` field.

## `memory`

Schema:

```json
{
  "type": "memory",
  "format": "{used_percentage} \uf2db ",
  "click": "optional shell command",
  "interval_secs": 5,
  "class": "optional-css-classes"
}
```

Fields:

- `format` (optional): output format template.
  - Default: `{used_percentage}%`
- `click` (optional): shell command run on left click.
- `on-click` (optional): alias for `click` (Waybar-style key).
- `interval_secs` (optional): polling interval in seconds.
  - Default: `5`
  - Minimum: `1` (values below are clamped)
- `class` (optional): extra CSS class(es) on the module label (whitespace-separated).

Format placeholders:

- `{used_percentage}`
- `{free_percentage}`
- `{available_percentage}`
- `{used}`
- `{free}`
- `{available}`
- `{total}`

Behavior:

- Polls `/proc/meminfo` and uses `MemTotal` / `MemAvailable`.
- Byte values are rendered in binary units (`B`, `K`, `M`, `G`, `T`, `P`).

Styling:

- Label classes: `.module.memory`
- Click-enabled labels also include: `.clickable`
- Optional extra class via `class` field.

## `cpu`

Schema:

```json
{
  "type": "cpu",
  "format": "{used_percentage}% ",
  "click": "optional shell command",
  "interval_secs": 5,
  "class": "optional-css-classes"
}
```

Fields:

- `format` (optional): output format template.
  - Default: `{used_percentage}%`
- `click` (optional): shell command run on left click.
- `on-click` (optional): alias for `click` (Waybar-style key).
- `interval_secs` (optional): polling interval in seconds.
  - Default: `5`
  - Minimum: `1` (values below are clamped)
- `class` (optional): extra CSS class(es) on the module label (whitespace-separated).

Format placeholders:

- `{used_percentage}`
- `{idle_percentage}`

Behavior:

- Polls `/proc/stat` and reads aggregate CPU counters from `cpu` line.
- Uses deltas between samples to compute usage percentage.
- Adds usage-state CSS class on each update:
  - `usage-low` for `< 30%`
  - `usage-medium` for `30-59%`
  - `usage-high` for `60-84%`
  - `usage-critical` for `>= 85%`
  - `usage-unknown` when sampling fails

Styling:

- Label classes: `.module.cpu`
- Dynamic usage classes: `.usage-low`, `.usage-medium`, `.usage-high`, `.usage-critical`, `.usage-unknown`
- Click-enabled labels also include: `.clickable`
- Optional extra class via `class` field.

## `tray`

Schema:

```json
{
  "type": "tray",
  "icon_size": 16,
  "poll_interval_secs": 2,
  "class": "optional-css-classes"
}
```

Fields:

- `icon_size` (optional): tray icon size in px.
  - Default: `16`
  - Minimum: `8` (values below are clamped)
- `poll_interval_secs` (optional): tray item discovery/update poll interval.
  - Default: `2`
  - Minimum: `1` (values below are clamped)
- `class` (optional): extra CSS class(es) on tray container (whitespace-separated).

Behavior:

- StatusNotifier-based tray.
- Left click triggers SNI `Activate`.
- Right click requests SNI menu and renders DBusMenu in GTK popover.
- Middle click triggers SNI `SecondaryActivate`.
- Toggleable DBusMenu entries (`toggle-type`/`toggle-state`) render with check/radio indicators.
- Icon lookup prefers theme icon names, then pixmap fallbacks.

Styling:

- Tray container classes: `.module.tray`
- Item class: `.tray-item`
- Menu classes: `.tray-menu-popover`, `.tray-menu-content`, `.tray-menu-item`, `.tray-menu-toggle`
- Optional extra class via `class` field.

## `pulseaudio`

Schema:

```json
{
  "type": "pulseaudio",
  "scroll-step": 1,
  "format": "{volume}% {icon}  {format_source}",
  "format-bluetooth": "{volume}% {icon} {format_source}",
  "format-bluetooth-muted": " {icon} {format_source}",
  "format-muted": " {format_source}",
  "format-source": "",
  "format-source-muted": "",
  "format-icons": {
    "headphone": "",
    "speaker": "",
    "hdmi": "",
    "hands-free": "",
    "headset": "",
    "phone": "",
    "portable": "",
    "car": "",
    "hifi": "",
    "default": ["", "", ""]
  },
  "click": "pavucontrol",
  "class": "optional-css-classes"
}
```

Fields:

- `scroll-step` (optional): amount in percent changed per scroll event.
  - Default: `1`
  - Values `<= 0` disable scroll volume changes.
- `format` (optional): default output format.
  - Default: `{volume}% {icon}  {format_source}`
- `format-bluetooth` (optional): format used for Bluetooth sinks.
  - Default: `{volume}% {icon} {format_source}`
- `format-bluetooth-muted` (optional): format used for muted Bluetooth sinks.
  - Default: ` {icon} {format_source}`
- `format-muted` (optional): format used for muted non-Bluetooth sinks.
  - Default: ` {format_source}`
- `format-source` (optional): source indicator when source is unmuted.
  - Default: ``
- `format-source-muted` (optional): source indicator when source is muted.
  - Default: ``
- `format-icons` (optional): icon mapping object for sink types and volume.
  - Supported keys: `headphone`, `speaker`, `hdmi`, `headset`, `hands-free`, `portable`, `car`, `hifi`, `phone`, `default`
  - `default` is an array of volume-level icons.
  - Default: `["", "", ""]`
- `click` (optional): shell command run on left click.
- `on-click` (optional): alias for `click` (Waybar-style key).
- `class` (optional): extra CSS class(es) on the module label (whitespace-separated).

Format placeholders:

- `{volume}`
- `{icon}`
- `{format_source}`

Behavior:

- Uses native `libpulse` subscription callbacks for near-immediate updates.
- On each relevant audio event, reads default sink volume/mute and default source mute state via PulseAudio introspection.
- Detects device icon category from sink `active_port.name + device form factor` using Waybar-style priority matching.
  - Match order: `headphone`, `speaker`, `hdmi`, `headset`, `hands-free`, `portable`, `car`, `hifi`, `phone`
- Scroll up/down adjusts default sink volume by `scroll-step`.

Styling:

- Label classes: `.module.pulseaudio`
- Click-enabled labels also include: `.clickable`
- Optional extra class via `class` field.
