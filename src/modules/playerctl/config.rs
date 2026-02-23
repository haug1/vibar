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
    #[serde(default)]
    pub(super) marquee: PlayerctlMarqueeMode,
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
    pub(super) fixed_width: Option<u32>,
    pub(super) marquee: PlayerctlMarqueeMode,
}

impl PlayerctlConfig {
    pub(super) fn into_view(self) -> PlayerctlViewConfig {
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
            fixed_width: self.fixed_width.and_then(normalize_fixed_width),
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

pub(super) fn normalize_fixed_width(value: u32) -> Option<u32> {
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
