use serde::Deserialize;
use serde_json::Value;

use crate::modules::ModuleConfig;

use super::MODULE_TYPE;

pub(super) const DEFAULT_SCROLL_STEP: f64 = 1.0;
pub(super) const DEFAULT_FORMAT: &str = "{volume}% {icon}  {format_source}";
pub(super) const DEFAULT_FORMAT_BLUETOOTH: &str = "{volume}% {icon} {format_source}";
pub(super) const DEFAULT_FORMAT_BLUETOOTH_MUTED: &str = " {icon} {format_source}";
pub(super) const DEFAULT_FORMAT_MUTED: &str = " {format_source}";
pub(super) const DEFAULT_FORMAT_SOURCE: &str = "";
pub(super) const DEFAULT_FORMAT_SOURCE_MUTED: &str = "";
pub(super) const DEFAULT_CONTROLS_ENABLED: bool = false;
pub(super) const ICON_VOLUME_LOW: &str = "";
pub(super) const ICON_VOLUME_MEDIUM: &str = "";
pub(super) const ICON_VOLUME_HIGH: &str = "";
pub(super) const ICON_HEADPHONE: &str = "";
pub(super) const ICON_HANDS_FREE: &str = "";
pub(super) const ICON_HEADSET: &str = "";
pub(super) const ICON_PHONE: &str = "";
pub(super) const ICON_PORTABLE: &str = "";
pub(super) const ICON_CAR: &str = "";

#[derive(Debug, Deserialize, Clone)]
pub(crate) struct PulseAudioConfig {
    #[serde(rename = "scroll-step", default = "default_scroll_step")]
    pub(crate) scroll_step: f64,
    #[serde(default)]
    pub(crate) format: Option<String>,
    #[serde(rename = "format-bluetooth", default)]
    pub(crate) format_bluetooth: Option<String>,
    #[serde(rename = "format-bluetooth-muted", default)]
    pub(crate) format_bluetooth_muted: Option<String>,
    #[serde(rename = "format-muted", default)]
    pub(crate) format_muted: Option<String>,
    #[serde(rename = "format-source", default)]
    pub(crate) format_source: Option<String>,
    #[serde(rename = "format-source-muted", default)]
    pub(crate) format_source_muted: Option<String>,
    #[serde(rename = "format-icons", default = "default_format_icons")]
    pub(crate) format_icons: PulseAudioFormatIcons,
    #[serde(default)]
    pub(crate) click: Option<String>,
    #[serde(rename = "on-click", default)]
    pub(crate) on_click: Option<String>,
    #[serde(rename = "right-click", default)]
    pub(crate) right_click: Option<String>,
    #[serde(rename = "on-right-click", default)]
    pub(crate) on_right_click: Option<String>,
    #[serde(default)]
    pub(crate) controls: PulseAudioControlsConfig,
    #[serde(default)]
    pub(crate) class: Option<String>,
}

#[derive(Debug, Deserialize, Clone, Default)]
pub(crate) struct PulseAudioControlsConfig {
    #[serde(default = "default_controls_enabled")]
    pub(crate) enabled: bool,
    #[serde(default)]
    pub(crate) open: PulseAudioControlsOpenMode,
}

#[derive(Debug, Deserialize, Clone, Copy, Default, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum PulseAudioControlsOpenMode {
    LeftClick,
    #[default]
    RightClick,
}

#[derive(Debug, Deserialize, Clone, PartialEq, Eq, Hash)]
pub(crate) struct PulseAudioFormatIcons {
    #[serde(default)]
    pub(crate) headphone: Option<String>,
    #[serde(default)]
    pub(crate) speaker: Option<String>,
    #[serde(default)]
    pub(crate) hdmi: Option<String>,
    #[serde(rename = "hands-free", default)]
    pub(crate) hands_free: Option<String>,
    #[serde(default)]
    pub(crate) headset: Option<String>,
    #[serde(default)]
    pub(crate) phone: Option<String>,
    #[serde(default)]
    pub(crate) portable: Option<String>,
    #[serde(default)]
    pub(crate) car: Option<String>,
    #[serde(default)]
    pub(crate) hifi: Option<String>,
    #[serde(default = "default_volume_icons")]
    pub(crate) default: Vec<String>,
}

pub(crate) fn parse_config(module: &ModuleConfig) -> Result<PulseAudioConfig, String> {
    if module.module_type != MODULE_TYPE {
        return Err(format!(
            "expected module type '{}', got '{}'",
            MODULE_TYPE, module.module_type
        ));
    }

    serde_json::from_value(Value::Object(module.config.clone()))
        .map_err(|err| format!("invalid {} module config: {err}", MODULE_TYPE))
}

fn default_scroll_step() -> f64 {
    DEFAULT_SCROLL_STEP
}

fn default_controls_enabled() -> bool {
    DEFAULT_CONTROLS_ENABLED
}

fn default_volume_icons() -> Vec<String> {
    vec![
        ICON_VOLUME_LOW.to_string(),
        ICON_VOLUME_MEDIUM.to_string(),
        ICON_VOLUME_HIGH.to_string(),
    ]
}

fn default_format_icons() -> PulseAudioFormatIcons {
    PulseAudioFormatIcons {
        headphone: None,
        speaker: None,
        hdmi: None,
        hands_free: None,
        headset: None,
        phone: None,
        portable: None,
        car: None,
        hifi: None,
        default: default_volume_icons(),
    }
}
