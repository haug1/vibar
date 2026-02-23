use serde::Deserialize;

const DEFAULT_PLAYERCTL_INTERVAL_SECS: u32 = 1;
const DEFAULT_PLAYERCTL_FORMAT: &str = "{status_icon} {title}";
const DEFAULT_NO_PLAYER_TEXT: &str = "No media";

#[derive(Debug, Deserialize, Clone)]
pub(super) struct PlayerctlConfig {
    #[serde(default)]
    pub(super) format: Option<String>,
    #[serde(default)]
    pub(super) click: Option<String>,
    #[serde(rename = "on-click", default)]
    pub(super) on_click: Option<String>,
    #[serde(default = "default_playerctl_interval")]
    pub(super) interval_secs: u32,
    #[serde(default)]
    pub(super) player: Option<String>,
    #[serde(default)]
    pub(super) class: Option<String>,
    #[serde(default = "default_no_player_text")]
    pub(super) no_player_text: String,
    #[serde(rename = "hide-when-idle", alias = "hide_when_idle", default)]
    pub(super) hide_when_idle: bool,
    #[serde(
        rename = "show-when-paused",
        alias = "show_when_paused",
        default = "default_show_when_paused"
    )]
    pub(super) show_when_paused: bool,
    #[serde(default)]
    pub(super) controls: PlayerctlControlsConfig,
    #[serde(rename = "fixed-width", alias = "fixed_width", default)]
    pub(super) fixed_width: Option<u32>,
    #[serde(rename = "max-width", alias = "max_width", default)]
    pub(super) max_width: Option<u32>,
    #[serde(default)]
    pub(super) marquee: PlayerctlMarqueeMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PlayerctlWidthMode {
    Fixed,
    Max,
}

#[derive(Debug, Deserialize, Clone)]
pub(super) struct PlayerctlControlsConfig {
    #[serde(default)]
    pub(super) enabled: bool,
    #[serde(default)]
    pub(super) open: PlayerctlControlsOpenMode,
    #[serde(
        rename = "show-seek",
        alias = "show_seek",
        default = "default_show_seek"
    )]
    pub(super) show_seek: bool,
}

#[derive(Debug, Deserialize, Clone, Copy, Default)]
#[serde(rename_all = "kebab-case")]
pub(super) enum PlayerctlControlsOpenMode {
    #[serde(alias = "left_click", alias = "left")]
    #[default]
    LeftClick,
}

#[derive(Debug, Deserialize, Clone, Copy, Default, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub(super) enum PlayerctlMarqueeMode {
    #[default]
    Off,
    #[serde(alias = "on-hover", alias = "on_hover", alias = "hover_only")]
    Hover,
    #[serde(alias = "while-open", alias = "while_open", alias = "on-open")]
    Open,
    Always,
}

#[derive(Debug, Clone)]
pub(super) struct PlayerctlViewConfig {
    pub(super) format: String,
    pub(super) click_command: Option<String>,
    pub(super) interval_secs: u32,
    pub(super) player: Option<String>,
    pub(super) class: Option<String>,
    pub(super) no_player_text: String,
    pub(super) hide_when_idle: bool,
    pub(super) show_when_paused: bool,
    pub(super) controls_enabled: bool,
    pub(super) controls_open: PlayerctlControlsOpenMode,
    pub(super) controls_show_seek: bool,
    pub(super) width_chars: Option<u32>,
    pub(super) width_mode: Option<PlayerctlWidthMode>,
    pub(super) marquee: PlayerctlMarqueeMode,
}

impl PlayerctlConfig {
    pub(super) fn into_view(self) -> PlayerctlViewConfig {
        let fixed_width = self.fixed_width.and_then(normalize_width_chars);
        let max_width = self.max_width.and_then(normalize_width_chars);
        let (width_chars, width_mode) = if let Some(width) = fixed_width {
            (Some(width), Some(PlayerctlWidthMode::Fixed))
        } else if let Some(width) = max_width {
            (Some(width), Some(PlayerctlWidthMode::Max))
        } else {
            (None, None)
        };

        PlayerctlViewConfig {
            format: self
                .format
                .unwrap_or_else(|| DEFAULT_PLAYERCTL_FORMAT.to_string()),
            click_command: self.click.or(self.on_click),
            interval_secs: self.interval_secs,
            player: self.player,
            class: self.class,
            no_player_text: self.no_player_text,
            hide_when_idle: self.hide_when_idle,
            show_when_paused: self.show_when_paused,
            controls_enabled: self.controls.enabled,
            controls_open: self.controls.open,
            controls_show_seek: self.controls.show_seek,
            width_chars,
            width_mode,
            marquee: self.marquee,
        }
    }
}

impl Default for PlayerctlControlsConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            open: PlayerctlControlsOpenMode::LeftClick,
            show_seek: default_show_seek(),
        }
    }
}

pub(super) fn default_playerctl_interval() -> u32 {
    DEFAULT_PLAYERCTL_INTERVAL_SECS
}

pub(super) fn normalize_width_chars(value: u32) -> Option<u32> {
    if value == 0 {
        return None;
    }

    Some(value)
}

fn default_no_player_text() -> String {
    DEFAULT_NO_PLAYER_TEXT.to_string()
}

fn default_show_when_paused() -> bool {
    true
}

fn default_show_seek() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use serde_json::{json, Map};

    use crate::modules::ModuleConfig;

    use super::*;

    #[test]
    fn parse_config_applies_visibility_defaults() {
        let module = ModuleConfig::new(super::super::MODULE_TYPE, Map::new());
        let cfg = super::super::parse_config(&module).expect("config should parse");

        assert!(!cfg.hide_when_idle);
        assert!(cfg.show_when_paused);
    }

    #[test]
    fn parse_config_applies_controls_defaults() {
        let module = ModuleConfig::new(super::super::MODULE_TYPE, Map::new());
        let cfg = super::super::parse_config(&module).expect("config should parse");

        assert!(!cfg.controls.enabled);
        assert!(cfg.controls.show_seek);
        assert!(matches!(
            cfg.controls.open,
            PlayerctlControlsOpenMode::LeftClick
        ));
    }

    #[test]
    fn parse_config_supports_controls_keys() {
        let module = ModuleConfig::new(
            super::super::MODULE_TYPE,
            serde_json::from_value(json!({
                "controls": {
                    "enabled": true,
                    "open": "left-click",
                    "show_seek": false
                }
            }))
            .expect("playerctl config map should parse"),
        );
        let cfg = super::super::parse_config(&module).expect("config should parse");

        assert!(cfg.controls.enabled);
        assert!(matches!(
            cfg.controls.open,
            PlayerctlControlsOpenMode::LeftClick
        ));
        assert!(!cfg.controls.show_seek);
    }

    #[test]
    fn parse_config_supports_controls_aliases() {
        let module = ModuleConfig::new(
            super::super::MODULE_TYPE,
            serde_json::from_value(json!({
                "controls": {
                    "enabled": true,
                    "open": "left_click",
                    "show-seek": false
                }
            }))
            .expect("playerctl config map should parse"),
        );
        let cfg = super::super::parse_config(&module).expect("config should parse");

        assert!(cfg.controls.enabled);
        assert!(matches!(
            cfg.controls.open,
            PlayerctlControlsOpenMode::LeftClick
        ));
        assert!(!cfg.controls.show_seek);
    }

    #[test]
    fn parse_config_supports_fixed_width_keys() {
        let kebab = ModuleConfig::new(
            super::super::MODULE_TYPE,
            serde_json::from_value(json!({
                "fixed-width": 40
            }))
            .expect("playerctl config map should parse"),
        );
        let snake = ModuleConfig::new(
            super::super::MODULE_TYPE,
            serde_json::from_value(json!({
                "fixed_width": 32
            }))
            .expect("playerctl config map should parse"),
        );

        let kebab_cfg = super::super::parse_config(&kebab).expect("config should parse");
        let snake_cfg = super::super::parse_config(&snake).expect("config should parse");

        assert_eq!(kebab_cfg.fixed_width, Some(40));
        assert_eq!(snake_cfg.fixed_width, Some(32));
    }

    #[test]
    fn parse_config_supports_max_width_keys() {
        let kebab = ModuleConfig::new(
            super::super::MODULE_TYPE,
            serde_json::from_value(json!({
                "max-width": 26
            }))
            .expect("playerctl config map should parse"),
        );
        let snake = ModuleConfig::new(
            super::super::MODULE_TYPE,
            serde_json::from_value(json!({
                "max_width": 24
            }))
            .expect("playerctl config map should parse"),
        );

        let kebab_cfg = super::super::parse_config(&kebab).expect("config should parse");
        let snake_cfg = super::super::parse_config(&snake).expect("config should parse");

        assert_eq!(kebab_cfg.max_width, Some(26));
        assert_eq!(snake_cfg.max_width, Some(24));
    }

    #[test]
    fn normalize_width_chars_rejects_zero() {
        assert_eq!(normalize_width_chars(0), None);
        assert_eq!(normalize_width_chars(1), Some(1));
    }

    #[test]
    fn into_view_prefers_fixed_width_over_max_width() {
        let module = ModuleConfig::new(
            super::super::MODULE_TYPE,
            serde_json::from_value(json!({
                "fixed-width": 40,
                "max-width": 24
            }))
            .expect("playerctl config map should parse"),
        );
        let cfg = super::super::parse_config(&module).expect("config should parse");
        let view = cfg.into_view();

        assert_eq!(view.width_chars, Some(40));
        assert_eq!(view.width_mode, Some(PlayerctlWidthMode::Fixed));
    }

    #[test]
    fn parse_config_defaults_marquee_to_off() {
        let module = ModuleConfig::new(super::super::MODULE_TYPE, Map::new());
        let cfg = super::super::parse_config(&module).expect("config should parse");
        assert!(matches!(cfg.marquee, PlayerctlMarqueeMode::Off));
    }

    #[test]
    fn parse_config_supports_marquee_modes() {
        let hover = ModuleConfig::new(
            super::super::MODULE_TYPE,
            serde_json::from_value(json!({
                "marquee": "hover"
            }))
            .expect("playerctl config map should parse"),
        );
        let open = ModuleConfig::new(
            super::super::MODULE_TYPE,
            serde_json::from_value(json!({
                "marquee": "open"
            }))
            .expect("playerctl config map should parse"),
        );
        let always = ModuleConfig::new(
            super::super::MODULE_TYPE,
            serde_json::from_value(json!({
                "marquee": "always"
            }))
            .expect("playerctl config map should parse"),
        );

        let hover_cfg = super::super::parse_config(&hover).expect("config should parse");
        let open_cfg = super::super::parse_config(&open).expect("config should parse");
        let always_cfg = super::super::parse_config(&always).expect("config should parse");

        assert!(matches!(hover_cfg.marquee, PlayerctlMarqueeMode::Hover));
        assert!(matches!(open_cfg.marquee, PlayerctlMarqueeMode::Open));
        assert!(matches!(always_cfg.marquee, PlayerctlMarqueeMode::Always));
    }
}
