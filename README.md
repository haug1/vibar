# vibar

A minimal configurable Wayland taskbar using Rust + GTK4 + `gtk4-layer-shell`.

# Previews

## Example bar

<img width="2558" height="51" alt="image" src="https://github.com/user-attachments/assets/bac93f3a-917a-416c-a71b-d2a9f2d507ed" />

## playerctl integration

<img width="200" height="200" alt="image" src="https://github.com/user-attachments/assets/d9f36c1e-2834-4c7d-a2c7-31c924a3465f" />

## pulseaudio integration

<img width="300" height="200" alt="image" src="https://github.com/user-attachments/assets/25273b45-04c0-4cdf-8f1b-54275b65aed1" />

## Getting Started

Install OS dependencies and Rust toolchain:

```bash
make deps
```

Note: `make deps` currently supports Arch-based and Fedora/RHEL-based distros only.

Generate a lockfile (commit it):

```bash
make lock
```

## Build And Run

```bash
make build
make run
```

Install system-wide:

```bash
make build-release
make install
```

Notes:

- `make build-release` compiles as your current user (so cargo caches are reused).
- `make install` only copies `target/release/vibar` into install location.
- Installs binary to `/usr/local/bin/vibar` by default (`PREFIX`/`BINDIR` override supported).

## Verification

Run individual checks:

```bash
make check
make fmt
make lint
make test
```

Run CI-equivalent checks:

```bash
make ci
```

## Dependency Automation

- Dependabot checks Rust (`cargo`) and GitHub Actions dependencies weekly.
- Dependabot PRs are configured to auto-merge when repository checks pass.

## Documentation

- Module configuration and styling selectors: [`docs/modules.md`](./docs/modules.md)
- Developer architecture and extension notes: [`docs/developer.md`](./docs/developer.md)
- Default example config: [`config.jsonc`](./config.jsonc)
- Base stylesheet loaded by default: [`style.css`](./style.css)

## Features

- Bottom-anchored layer-shell bar
- One bar window per connected monitor at startup
- Configurable horizontal layout with `left`, `center`, `right` areas
- Configurable module system (`docs/modules.md`)
- Sway modules for workspaces, active mode, and active-window title (`sway/workspaces`, `sway/mode`, `sway/window`)
- Workspace module supports container and per-workspace-button CSS class overrides
- Playerctl supports `max-width` display mode for adaptive title width
- Playerctl shows a truncation cue when text is clipped
- Playerctl hover tooltip appears only when text is clipped
- Backlight module with Waybar-style `format-icons`, optional `device` selection, and event-driven `/sys/class/backlight` updates via udev
- Backlight module supports scroll brightness control (default logind DBus path or custom scroll commands)
- Battery module with `/sys/class/power_supply` auto-discovery, `format-icons`, and dynamic level/status CSS classes
- Temperature module with Linux sensor path/thermal-zone sources, threshold states, and Waybar-style placeholders
- PulseAudio module supports optional popup controls for default sink mute/volume, active app stream volume/mute, and output-port switching, plus separate left/right click commands
- Tray module uses event-driven StatusNotifier updates with debounced refreshes and widget reuse to reduce CPU/DBus overhead
- Config file search order:
  - `~/.config/vibar/config.jsonc`
  - embedded `config.jsonc` bundled in binary (final fallback)
- CSS layering support:
  - embedded default `style.css`
  - optional user CSS loaded on top
  - default CSS can be disabled via `style.load-default`

# Preview bar config

Here's the configuration used for the example preview bar above:

<details>
  <summary>my personal `config.jsonc`</summary>
  
  ```jsonc
  {
  // App searches config in this order:
  // 1) ~/.config/vibar/config.jsonc
  // 2) ./config.jsonc
  // If neither exists (or both are invalid), built-in defaults are used.
  "style": {
    // Optional. Defaults to true.
    // Set false to disable embedded default style.css.
    "load-default": true,
    // Optional. User CSS loaded after default CSS (if enabled), so it can override.
    // Relative paths are resolved from this config file's directory.
    "path": "~/.config/vibar/style.css",
  },
  "areas": {
    "left": [
      { "type": "sway/workspaces" },
      { "type": "sway/mode", "format": "<span style=\"italic\">{}</span>" },
      { "type": "sway/window" },
    ],
    "center": [],
    "right": [
      {
        "type": "temperature",
        "class": "v-square",
        "warning-threshold": 65,
        "format": "",
        "format-warning": "{temperatureC}°C {icon}",
        "format-critical": "{temperatureC}°C {icon}",
        "format-icons": [""],
      },
      {
        "type": "cpu",
        "class": "v-square",
        "format": "{used_percentage}% ",
        "interval_secs": 1,
      },
      {
        "type": "memory",
        "class": "v-square",
        "format": "{used_percentage}% \uf2db",
      },
      {
        "type": "disk",
        "class": "v-square",
        "format": "{free} \uf0a0 ",
        "click": "xdg-open $HOME",
      },
      {
        "type": "exec",
        "class": "finalmouse v-square",
        "command": "cat ~/.cache/finalmouse/battery",
        "interval_secs": 10,
      },
      {
        "type": "playerctl",
        "format": "{status_icon}  {artist} - {title}",
        "max-width": 30,
        "controls": {
          "enabled": true,
          "open": "left-click",
          "show_seek": true,
        },
        "marquee": "open",
        "class": "v-square",
        "hide-when-idle": true, // false is default
        // "show-when-paused": false, // true is default
      },
      {
        "type": "pulseaudio",
        "right-click": "pavucontrol",
        "controls": {
          // Optional inline popup controls (default disabled):
          "enabled": true,
          // "left-click" or "right-click" (default).
          // If set to "left-click", the top-level `click` command is ignored.
          "open": "left-click",
        },
        "class": "v-square",
      },
      {
        "type": "group",
        "drawer": true,
        "modules": [
          { "type": "tray", "icon_size": 16, "poll_interval_secs": 2 },
        ],
      },
      { "type": "clock", "time-format": "%a %d. %b %H:%M:%S" },
      {
        "type": "exec",
        "command": "~/.config/waybar/updates.py",
        "on-click": "alacritty -e yay --noconfirm && ~/.config/waybar/updates.py --force && pkill -RTMIN+8 vibar",
        "class": "updates v-square",
        "signal": 8,
        "interval_secs": 30,
      },
      {
        "type": "group",
        "drawer": {
          // Waybar transition settings are not supported in vibar.
          "label-closed": "",
          "label-open": "",
          "start-open": false,
        },
        "modules": [
          {
            "type": "exec",
            "command": "printf '⏻ Poweroff'",
            "on-click": "systemctl poweroff",
          },
          {
            "type": "exec",
            "command": "printf ' Restart'",
            "on-click": "systemctl reboot",
          },
          {
            "type": "exec",
            "command": "printf ' Sleep'",
            "on-click": "systemctl suspend",
          },
          {
            "type": "exec",
            "command": "printf ' Lock'",
            "on-click": "loginctl lock-session",
          },
        ],
      },
    ],
  },
}
  ```
</details>

<details>
  <summary>my personal `style.css`</summary>

```css
* {
  /* `otf-font-awesome` is required to be installed for icons */
  font-family:
    FontAwesome,
    JetBrainsMono Nerd Font,
    Arial,
    sans-serif;
  font-size: 1rem;
}

.finalmouse.critical {
  background-color: rgba(255, 0, 0, 0.5); /* Red - Critical */
}
.finalmouse.low {
  background-color: rgba(255, 165, 0, 0.5); /* Orange - Low */
}
.finalmouse.medium {
  background-color: rgba(255, 215, 0, 0.5); /* Gold/Yellow - Medium */
}
.finalmouse.high {
  background-color: rgba(144, 238, 144, 0.5); /* Light Green - High */
}
.finalmouse.full {
  background-color: rgba(0, 128, 0, 0.5); /* Green - Full */
}

.updates {
  background-color: rgba(176, 176, 0, 0.2);
  box-shadow: inset 0 -3px yellow;
}

.updates:hover {
  background-color: rgba(176, 176, 0, 0.5);
}

.updates.critical {
  background-color: rgba(176, 0, 0, 0.2);
  box-shadow: inset 0 -3px red;
}

.updates.critical:hover {
  background-color: rgba(176, 0, 0, 0.5);
}
```

</details>

## Troubleshooting

- If text updates leave tiny font/glyph dots, it may help to set an explicit `line-height` on the affected module class (for example `line-height: 1.5;`).

## Acknowledgements

- [Waybar](https://github.com/Alexays/Waybar) for long-running status bar design ideas and overall behavior references that influenced this project.

## License

MIT. See [`LICENSE`](./LICENSE).
