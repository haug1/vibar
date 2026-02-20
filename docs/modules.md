# Module Configuration

This document is the canonical configuration reference for all currently supported module types.

## Config Shape

Top-level config uses three layout areas:

```jsonc
{
  "areas": {
    "left": [{ "type": "workspaces" }],
    "center": [{ "type": "exec", "command": "echo center" }],
    "right": [
      { "type": "tray" },
      { "type": "disk", "format": "{free} \uf0a0 ", "click": "dolphin" },
      { "type": "clock" }
    ]
  }
}
```

Each entry in an area is a module object with a required `"type"` key.

## Styling Overview

`style.css` is loaded by default. Common layout selectors:

- `.bar`
- `.left`
- `.center`
- `.right`

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
{ "type": "clock", "format": "%a %d. %b %H:%M:%S" }
```

Fields:

- `format` (optional): `chrono` format string.
  - Default: `%a %d. %b %H:%M:%S`

Behavior:

- Updates every second on GTK main loop.

Styling:

- Label classes: `.module.clock`

## `exec`

Schema:

```json
{
  "type": "exec",
  "command": "your shell command",
  "interval_secs": 5,
  "class": "optional-css-class"
}
```

Fields:

- `command` (required): shell command executed with `sh -c`.
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
