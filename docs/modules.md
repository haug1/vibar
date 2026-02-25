# Module Configuration

This document is the canonical configuration reference for all currently supported module types.

## Config Shape

Top-level config uses three layout areas:

```jsonc
{
  "style": {
    "load-default": true,
    "path": "~/.config/vibar/style.css",
  },
  "areas": {
    "left": [{ "type": "sway/workspaces" }],
    "center": [
      { "type": "sway/mode", "class": "v-pill" },
      { "type": "sway/window" },
    ],
    "right": [
      {
        "type": "group",
        "class": "media-group",
        "drawer": true,
        "modules": [{ "type": "pulseaudio" }, { "type": "tray" }],
      },
      { "type": "disk", "format": "{free} \uf0a0 ", "click": "dolphin" },
      { "type": "cpu", "format": "{used_percentage}% ", "interval_secs": 1 },
      { "type": "temperature", "format": "{temperatureC}°C {icon}", "thermal-zone": 0 },
      { "type": "battery", "format": "{capacity}% {icon}" },
      { "type": "clock" },
    ],
  },
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

Schema:

```json
{
  "type": "sway/workspaces",
  "class": "optional-css-classes",
  "button-class": "optional-workspace-button-css-classes"
}
```

Fields:

- `class` (optional): extra CSS class(es) on the module container (whitespace-separated).
- `button-class` / `button_class` (optional): extra CSS class(es) on each workspace button (whitespace-separated).

Behavior:

- Sway IPC workspace module.
- Updates on workspace/output events (event-driven refresh).
- On multi-monitor setups, each bar window shows only workspaces for its output.
- Clicking a workspace button focuses that workspace in sway.

Styling:

- Container classes: `.module.workspaces`
- Per-workspace button class: `.menu-button`
- Active state classes: `.menu-button.active`, `.menu-button.workspace-active`
- Optional extra container class via `class` field.
- Optional extra per-button class via `button-class` field.

## `sway/window`

Minimal schema:

```json
{
  "type": "sway/window",
  "format": "{}",
  "click": "optional shell command",
  "class": "optional-css-classes"
}
```

Fields:

- `format` (optional): window-title display template.
  - Supports Pango markup.
  - Replaced title text is markup-escaped before insertion.
  - Supported placeholders: `{}` and `{title}`
  - Default: `{}`
- `click` (optional): shell command run on left click.
- `on-click` (optional): alias for `click`.
- `class` (optional): extra CSS class(es) on the module label (whitespace-separated).

Behavior:

- Sway IPC focused-window title module.
- Updates on window/workspace/output events (event-driven refresh).
- Shows the currently focused window title.
- On multi-monitor setups, module is only visible on the bar whose output owns the focused workspace.

Styling:

- Label classes: `.module.sway-window`

## `sway/mode`

Minimal schema:

```json
{
  "type": "sway/mode",
  "format": "{}",
  "click": "optional shell command",
  "class": "optional-css-classes"
}
```

Fields:

- `format` (optional): mode display format where `{}` is replaced with the active mode name.
  - Supports Pango markup (for example `<span style="italic">{}</span>`).
  - Replaced mode text is markup-escaped before insertion.
  - Default: `{}`
- `click` (optional): shell command run on left click.
- `on-click` (optional): alias for `click`.
- `class` (optional): extra CSS class(es) on the module label (whitespace-separated).

Behavior:

- Sway IPC binding mode module.
- Updates on sway `mode` events (event-driven refresh).
- Hidden when mode is `default`.
- Visible in non-default modes (for example `resize`).

Styling:

- Label classes: `.module.sway-mode`

## `clock`

Schema:

```json
{
  "type": "clock",
  "time-format": "%a %d. %b %H:%M:%S",
  "format": "<span style=\"italic\">{}</span>",
  "click": "optional shell command",
  "class": "optional-css-classes"
}
```

Fields:

- `time-format` / `time_format` (optional): `chrono` format string for the raw time value.
  - Default: `%a %d. %b %H:%M:%S`
- `format` (optional): display template where `{}` is replaced with the formatted time.
  - Supports Pango markup.
  - Replaced time text is markup-escaped before insertion.
  - Default: `{}`
- `click` (optional): shell command run on left click.
- `on-click` (optional): alias for `click`.
- `class` (optional): extra CSS class(es) on the module label (whitespace-separated).

Behavior:

- Updates every second on GTK main loop.

Styling:

- Label classes: `.module.clock`

## `playerctl`

Schema:

```json
{
  "type": "playerctl",
  "format": "{status_icon} {title}",
  "max-width": 40,
  "marquee": "hover",
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
  - Supports Pango markup.
  - Placeholder values are markup-escaped before insertion.
  - Default: `{status_icon} {title}`
- `max-width` / `max_width` (optional): maximum visible width in character cells.
  - If set, long text is clipped to this width, but short text keeps its natural width.
  - `0` disables max-width behavior.
- `marquee` (optional): carousel animation mode for overflow text when `max-width` is set.
  - Supported values: `off`, `hover`, `open`, `always`
  - Default: `off` (while animating, app will use a lot more resources, so it's disabled default)
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
- `on-click` (optional): alias for `click`.
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
- With `max-width` set, the module shrinks to content for short text and caps width for long text.
- With `max-width` set and text overflow, the module renders a visible `…` truncation cue.
- If `marquee=hover`, `marquee=open`, or `marquee=always`, long text scrolls smoothly by pixel offset (stable with proportional fonts).
- `marquee=off` keeps clipped static text and avoids continuous animation overhead.
- `marquee=open` animates only while the controls popover is open (`controls.enabled=true`).
- Playerctl text is exposed as a hover tooltip only when text is actually truncated (and controls are closed), so clipped text remains discoverable without extra noise.
- When `controls.enabled=true`, left-click opens a popover with centered transport buttons on top, a key/value metadata list (`Status`, `Player`, `Artist`, `Album`, `Title`), and optional seek slider.
- While the controls popover is open, hover tooltip display is temporarily suppressed to avoid UI overlap.
- Controls popover width follows the module width; long metadata values wrap within that width (`WordChar` wrapping).
- Seek writes use MPRIS `SetPosition` (guarded by `CanSeek`, track id presence, and positive duration).
- Slider updates ignore backend refresh while scrubbing to avoid seek feedback loops.
- Controls popover seek UI includes `MM:ss` progress labels (current position left, total length right).
- When `controls.enabled=false`, click behavior remains legacy (`click` / `on-click` command).
- Status icon defaults:
  - `playing` -> ``
  - `paused` -> ``
  - `stopped` -> ``
  - fallback -> ``

Styling:

- Label classes: `.module.playerctl`
- State classes: `.status-playing`, `.status-paused`, `.status-stopped`, `.no-player`
- Width-mode carousel classes: `.playerctl-max-width`, `.playerctl-carousel`
- Controls popover classes: `.playerctl-controls-popover`, `.playerctl-controls-content`, `.playerctl-controls-row`, `.playerctl-control-button`, `.playerctl-controls-metadata-grid`, `.playerctl-controls-metadata-key`, `.playerctl-controls-metadata-value`, `.playerctl-seek-scale`, `.playerctl-seek-time-row`, `.playerctl-seek-time`
- Optional extra class via `class` field.

## `exec`

Schema:

```json
{
  "type": "exec",
  "command": "your shell command",
  "format": "<span style=\"italic\">{}</span>",
  "click": "optional shell command",
  "interval_secs": 5,
  "signal": 8,
  "class": "optional-css-classes"
}
```

Fields:

- `command` (required): shell command executed with `sh -c`.
- `format` (optional): output display template.
  - Supports Pango markup.
  - Placeholder values are markup-escaped before insertion.
  - Default: `{text}`
- `click` (optional): shell command run on left click.
- `on-click` (optional): alias for `click`.
- `interval_secs` (optional): polling interval in seconds.
  - Default: `5`
  - Minimum: `1` (values below are clamped)
- `signal` (optional): realtime signal offset (`SIGRTMIN + signal`) that triggers an immediate refresh.
  - Valid range: `1..=(SIGRTMAX - SIGRTMIN)`.
  - Example trigger: `pkill -RTMIN+8 vibar` when `"signal": 8`.
- `class` (optional): extra CSS class(es) on the module label (whitespace-separated).

Behavior:

- Shows command output in a label.
- If stdout is empty, stderr is used as fallback text.
- Module auto-hides when parsed output text is empty.
- Output parsing is Waybar-compatible:
  - i3blocks style (default): line 1 = text, line 2 = tooltip (ignored), line 3 = CSS class list.
  - JSON style: if output is valid JSON, `text` and `class` fields are used (`class` supports string or string array).
- Formatting placeholders:
  - `{}` and `{text}` map to the parsed output text.
  - For JSON output, top-level string/number/bool properties can be referenced as `{property}`.
- Identical `command` + `format` + `interval_secs` instances share one backend poller across bar windows.
- Signal-triggered refreshes wake the shared backend immediately (without waiting for the next interval tick).

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
  "class": "optional-css-classes"
}
```

Fields:

- `format` (optional): output format template.
  - Supports Pango markup.
  - Placeholder values are markup-escaped before insertion.
  - Default: `{free}`
- `click` (optional): shell command run on left click.
- `on-click` (optional): alias for `click`.
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
  - Supports Pango markup.
  - Placeholder values are markup-escaped before insertion.
  - Default: `{used_percentage}%`
- `click` (optional): shell command run on left click.
- `on-click` (optional): alias for `click`.
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
  - Supports Pango markup.
  - Placeholder values are markup-escaped before insertion.
  - Default: `{used_percentage}%`
- `click` (optional): shell command run on left click.
- `on-click` (optional): alias for `click`.
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
- Optional extra class via `class` field.

## `temperature`

Schema:

```json
{
  "type": "temperature",
  "format": "{temperatureC}°C {icon}",
  "format-warning": "{temperatureC}°C {icon}",
  "format-critical": "{temperatureC}°C {icon}",
  "interval_secs": 10,
  "thermal-zone": 0,
  "path": "/sys/class/hwmon/hwmon0/temp1_input",
  "warning-threshold": 70,
  "critical-threshold": 85,
  "format-icons": ["", "", "", "", ""],
  "click": "optional shell command",
  "class": "optional-css-classes"
}
```

Fields:

- `format` (optional): output format template.
  - Supports Pango markup.
  - Placeholder values are markup-escaped before insertion.
  - Default: `{temperatureC}°C {icon}`
- `format-warning` / `format_warning` (optional): template override when warning threshold is reached.
- `format-critical` / `format_critical` (optional): template override when critical threshold is reached.
- `interval_secs` (optional): polling interval in seconds.
  - Default: `10`
  - Minimum: `1` (values below are clamped)
- `path` / `hwmon-path` / `hwmon_path` (optional): explicit sensor file to read.
  - When omitted, module uses `thermal-zone`.
- `thermal-zone` / `thermal_zone` (optional): thermal zone index used for default path.
  - Default: `0` (path `/sys/class/thermal/thermal_zone0/temp`)
- `warning-threshold` / `warning_threshold` (optional): warning temperature in Celsius.
- `critical-threshold` / `critical_threshold` (optional): critical temperature in Celsius.
- `format-icons` (optional): icon list mapped by Celsius value over `0..100`.
  - Empty list renders `{icon}` as empty text.
  - Default: `["", "", "", "", ""]`
- `click` (optional): shell command run on left click.
- `on-click` (optional): alias for `click`.
- `class` (optional): extra CSS class(es) on the module label (whitespace-separated).

Format placeholders:

- `{temperature_c}` / `{temperatureC}`
- `{temperature_f}` / `{temperatureF}`
- `{temperature_k}` / `{temperatureK}`
- `{icon}`

Behavior:

- Reads numeric temperature values from Linux sensor files.
- Supports both millidegree input (for example `42500`) and degree input (for example `42`).
- Hides the module when the selected format renders empty output text.
- Adds temperature-state CSS class on each update:
  - `temperature-normal` (default)
  - `temperature-warning` (at/above warning threshold)
  - `temperature-critical` (at/above critical threshold)
  - `temperature-unknown` when reading/parsing fails

Styling:

- Label classes: `.module.temperature`
- Dynamic temperature classes: `.temperature-normal`, `.temperature-warning`, `.temperature-critical`, `.temperature-unknown`
- Optional extra class via `class` field.

## `backlight`

Schema:

```json
{
  "type": "backlight",
  "format": "{percent}% {icon}",
  "interval_secs": 2,
  "device": "intel_backlight",
  "format-icons": ["", "", "", "", "", "", "", "", ""],
  "scroll-step": 1.0,
  "min-brightness": 0.0,
  "on-scroll-up": "optional shell command",
  "on-scroll-down": "optional shell command",
  "click": "optional shell command",
  "class": "optional-css-classes"
}
```

Fields:

- `format` (optional): output format template.
  - Supports Pango markup.
  - Placeholder values are markup-escaped before insertion.
  - Default: `{percent}% {icon}`
- `interval_secs` / `interval` (optional): safety resync interval in seconds.
  - Default: `2`
  - Minimum: `1` (values below are clamped)
- `device` (optional): preferred backlight device in `/sys/class/backlight` (for example `intel_backlight`).
  - If omitted (or not found), module falls back to the device with the largest `max_brightness`.
- `format-icons` (optional): icon list mapped by brightness percentage.
  - Empty list renders `{icon}` as empty text.
  - Default: `["", "", "", "", "", "", "", "", ""]`
- `scroll-step` (optional): amount in percent changed per scroll event when using default scroll behavior.
  - Default: `1.0`
  - Values `<= 0` disable default scroll brightness control.
- `min-brightness` (optional): lower clamp percentage for default scroll-down behavior.
  - Default: `0.0`
  - Range is clamped to `0..100`.
- `on-scroll-up` (optional): shell command for scroll-up.
- `on-scroll-down` (optional): shell command for scroll-down.
  - If either scroll command is set, custom commands are used for scrolling instead of default brightness control.
- `click` (optional): shell command run on left click.
- `on-click` (optional): alias for `click`.
- `class` (optional): extra CSS class(es) on the module label (whitespace-separated).

Format placeholders:

- `{percent}`
- `{icon}`
- `{brightness}`
- `{max}`
- `{device}`

Behavior:

- Reads Linux backlight data from `/sys/class/backlight/*`.
- Uses `udev` backlight events as primary update trigger with immediate GTK main-thread dispatch.
- Keeps `interval_secs` as a coarse periodic resync fallback/safety path (not the primary update cadence).
- Uses `actual_brightness` when present, otherwise `brightness`.
- By default, scroll up/down adjusts brightness via logind DBus `SetBrightness`.
- Maintains cached backlight device state and selected-device snapshot (`device` preference first, otherwise largest `max_brightness`).
- Hides the module when the chosen device reports `bl_power != 0`.
- Adds brightness-state CSS class on each update:
  - `brightness-low` for `< 34%`
  - `brightness-medium` for `34-66%`
  - `brightness-high` for `>= 67%`
  - `brightness-unknown` when polling fails

Styling:

- Label classes: `.module.backlight`
- Dynamic brightness classes: `.brightness-low`, `.brightness-medium`, `.brightness-high`, `.brightness-unknown`
- Optional extra class via `class` field.

## `battery`

Schema:

```json
{
  "type": "battery",
  "format": "{capacity}% {icon}",
  "interval_secs": 10,
  "device": "BAT0",
  "format-icons": ["", "", "", "", ""],
  "click": "optional shell command",
  "class": "optional-css-classes"
}
```

Fields:

- `format` (optional): output format template.
  - Supports Pango markup.
  - Placeholder values are markup-escaped before insertion.
  - Default: `{capacity}% {icon}`
- `interval_secs` (optional): safety resync interval in seconds.
  - Default: `10`
  - Minimum: `1` (values below are clamped)
- `device` (optional): preferred battery device in `/sys/class/power_supply` (for example `BAT0`).
  - If omitted, module auto-discovers battery devices and picks the first device name in sorted order.
- `format-icons` (optional): icon list mapped by battery percentage.
  - Empty list renders `{icon}` as empty text.
  - Default: `["", "", "", "", ""]`
- `click` (optional): shell command run on left click.
- `on-click` (optional): alias for `click`.
- `class` (optional): extra CSS class(es) on the module label (whitespace-separated).

Format placeholders:

- `{capacity}`
- `{percent}` (alias of `{capacity}`)
- `{status}`
- `{icon}`
- `{device}`

Behavior:

- Reads battery data from Linux `/sys/class/power_supply/*`.
- Auto-discovers battery devices by `capacity` file + `BAT*` name or `type=Battery`.
- Uses `udev` `power_supply` events as primary update trigger with immediate GTK main-thread dispatch.
- Keeps `interval_secs` as a coarse periodic resync fallback/safety path (not the primary update cadence).
- Hides the module when no battery device is available.
- Adds battery-level CSS class on each update:
  - `battery-critical` for `< 15%`
  - `battery-low` for `15-34%`
  - `battery-medium` for `35-69%`
  - `battery-high` for `>= 70%`
  - `battery-unknown` when polling fails
- Adds battery-status CSS class on each update:
  - `status-charging`
  - `status-discharging`
  - `status-full`
  - `status-not-charging`
  - `status-unknown`

Styling:

- Label classes: `.module.battery`
- Dynamic level classes: `.battery-critical`, `.battery-low`, `.battery-medium`, `.battery-high`, `.battery-unknown`
- Dynamic status classes: `.status-charging`, `.status-discharging`, `.status-full`, `.status-not-charging`, `.status-unknown`
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
  "controls": {
    "enabled": true,
    "open": "right-click"
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
  - Supports Pango markup.
  - Placeholder values are markup-escaped before insertion.
  - Default: `{volume}% {icon}  {format_source}`
- `format-bluetooth` (optional): format used for Bluetooth sinks.
  - Supports Pango markup.
  - Placeholder values are markup-escaped before insertion.
  - Default: `{volume}% {icon} {format_source}`
- `format-bluetooth-muted` (optional): format used for muted Bluetooth sinks.
  - Supports Pango markup.
  - Placeholder values are markup-escaped before insertion.
  - Default: ` {icon} {format_source}`
- `format-muted` (optional): format used for muted non-Bluetooth sinks.
  - Supports Pango markup.
  - Placeholder values are markup-escaped before insertion.
  - Default: ` {format_source}`
- `format-source` (optional): source indicator when source is unmuted.
  - Supports Pango markup.
  - Placeholder values are markup-escaped before insertion.
  - Default: ``
- `format-source-muted` (optional): source indicator when source is muted.
  - Supports Pango markup.
  - Placeholder values are markup-escaped before insertion.
  - Default: ``
- `format-icons` (optional): icon mapping object for sink types and volume.
  - Supported keys: `headphone`, `speaker`, `hdmi`, `headset`, `hands-free`, `portable`, `car`, `hifi`, `phone`, `default`
  - `default` is an array of volume-level icons.
  - Default: `["", "", ""]`
- `controls` (optional): popup audio controls attached to the module.
  - `enabled` (optional): enable the popup.
    - Default: `false`
  - `open` (optional): click gesture that toggles popup visibility.
    - Supported values: `left-click`, `right-click`
    - Default: `right-click`
- `click` (optional): shell command run on left click.
- `on-click` (optional): alias for `click`.
- `class` (optional): extra CSS class(es) on the module label (whitespace-separated).

Format placeholders:

- `{volume}`
- `{icon}`
- `{format_source}`

Behavior:

- Uses native `libpulse` subscription callbacks for near-immediate updates.
- On each relevant audio event, reads default sink volume/mute and default source mute state via PulseAudio introspection.
- Subscribes to sink-input events so active app stream controls stay in sync while streams start/stop.
- Detects device icon category from sink `active_port.name + device form factor` using Waybar-style priority matching.
  - Match order: `headphone`, `speaker`, `hdmi`, `headset`, `hands-free`, `portable`, `car`, `hifi`, `phone`
- Scroll up/down adjusts default sink volume by `scroll-step`.
- With `controls.enabled=true`, popup includes:
  - default sink mute toggle + volume slider
  - output device list with availability labels and default-device marker
  - output-port buttons for the selected output device
  - per-stream mute toggles + volume sliders for active playback streams
  - percentage labels next to main/per-stream sliders with immediate updates while dragging
- If `controls.open=left-click`, module `click` command is ignored.

Styling:

- Label classes: `.module.pulseaudio`
- Popup classes: `.pulseaudio-controls-popover`, `.pulseaudio-controls-content`, `.pulseaudio-controls-section-title`, `.pulseaudio-controls-sink-row`, `.pulseaudio-controls-sinks`, `.pulseaudio-controls-ports`, `.pulseaudio-controls-inputs`, `.pulseaudio-controls-input-row`, `.pulseaudio-controls-input-name`, `.pulseaudio-control-button`, `.pulseaudio-volume-scale`, `.pulseaudio-controls-empty`
- Optional extra class via `class` field.
