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
    "left": [{ "type": "workspaces" }],
    "center": [{ "type": "exec", "command": "echo center" }],
    "right": [
      {
        "type": "group",
        "class": "media-group",
        "drawer": true,
        "modules": [{ "type": "pulseaudio" }, { "type": "tray" }]
      },
      { "type": "disk", "format": "{free} \uf0a0 ", "click": "dolphin" },
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
- `.module.clickable` (applied when a label-backed module has click actions)

## `group`

Schema:

```json
{
  "type": "group",
  "class": "optional-css-class",
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
- `class` (optional): extra CSS class on the group container.
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

## `workspaces`

Minimal schema:

```json
{ "type": "workspaces" }
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

## `exec`

Schema:

```json
{
  "type": "exec",
  "command": "your shell command",
  "click": "optional shell command",
  "interval_secs": 5,
  "class": "optional-css-class"
}
```

Fields:

- `command` (required): shell command executed with `sh -c`.
- `click` (optional): shell command run on left click.
- `on-click` (optional): alias for `click` (Waybar-style key).
- `interval_secs` (optional): polling interval in seconds.
  - Default: `5`
  - Minimum: `1` (values below are clamped)
- `class` (optional): extra CSS class on the module label.

Behavior:

- Shows command output in a label.
- If stdout is empty, stderr is used.
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
  "class": "optional-css-class"
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
- `class` (optional): extra CSS class on the module label.

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

## `tray`

Schema:

```json
{
  "type": "tray",
  "icon_size": 16,
  "poll_interval_secs": 2,
  "class": "optional-css-class"
}
```

Fields:

- `icon_size` (optional): tray icon size in px.
  - Default: `16`
  - Minimum: `8` (values below are clamped)
- `poll_interval_secs` (optional): tray item discovery/update poll interval.
  - Default: `2`
  - Minimum: `1` (values below are clamped)
- `class` (optional): extra CSS class on tray container.

Behavior:

- StatusNotifier-based tray.
- Left click triggers SNI `Activate`.
- Right click requests SNI menu and renders DBusMenu in GTK popover.
- Middle click triggers SNI `SecondaryActivate`.
- Icon lookup prefers theme icon names, then pixmap fallbacks.

Styling:

- Tray container classes: `.module.tray`
- Item class: `.tray-item`
- Menu classes: `.tray-menu-popover`, `.tray-menu-content`, `.tray-menu-item`
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
  "class": "optional-css-class"
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
- `class` (optional): extra CSS class on the module label.

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
