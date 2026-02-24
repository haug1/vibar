use std::fs;
use std::path::Path;
use std::sync::mpsc::Sender;
use std::time::Duration;

use glib::ControlFlow;
use gtk::prelude::*;
use gtk::{Label, Widget};
use serde::Deserialize;
use serde_json::Value;

use crate::modules::{
    apply_css_classes, attach_primary_click_command, escape_markup_text, render_markup_template,
    ModuleBuildContext, ModuleConfig,
};

use super::ModuleFactory;

const MIN_BACKLIGHT_INTERVAL_SECS: u32 = 1;
const DEFAULT_BACKLIGHT_INTERVAL_SECS: u32 = 2;
const DEFAULT_BACKLIGHT_FORMAT: &str = "{percent}% {icon}";
const UI_DRAIN_INTERVAL_MILLIS: u64 = 200;
const BACKLIGHT_LEVEL_CLASSES: [&str; 4] = [
    "brightness-low",
    "brightness-medium",
    "brightness-high",
    "brightness-unknown",
];
pub(crate) const MODULE_TYPE: &str = "backlight";

#[derive(Debug, Deserialize, Clone)]
pub(crate) struct BacklightConfig {
    #[serde(default)]
    pub(crate) format: Option<String>,
    #[serde(default = "default_backlight_interval", alias = "interval")]
    pub(crate) interval_secs: u32,
    #[serde(default)]
    pub(crate) device: Option<String>,
    #[serde(rename = "format-icons", default = "default_backlight_icons")]
    pub(crate) format_icons: Vec<String>,
    #[serde(default)]
    pub(crate) click: Option<String>,
    #[serde(rename = "on-click", default)]
    pub(crate) on_click: Option<String>,
    #[serde(default)]
    pub(crate) class: Option<String>,
}

#[derive(Debug, Clone)]
struct BacklightDevice {
    name: String,
    actual_brightness: u64,
    max_brightness: u64,
    powered: bool,
}

#[derive(Debug, Clone)]
struct BacklightSnapshot {
    device: BacklightDevice,
    percent: u16,
}

#[derive(Debug, Clone)]
struct BacklightUiUpdate {
    text: String,
    visible: bool,
    level_class: &'static str,
}

pub(crate) struct BacklightFactory;

pub(crate) const FACTORY: BacklightFactory = BacklightFactory;

impl ModuleFactory for BacklightFactory {
    fn module_type(&self) -> &'static str {
        MODULE_TYPE
    }

    fn init(&self, config: &ModuleConfig, _context: &ModuleBuildContext) -> Result<Widget, String> {
        let parsed = parse_config(config)?;
        let format = parsed
            .format
            .unwrap_or_else(|| DEFAULT_BACKLIGHT_FORMAT.to_string());
        let click_command = parsed.click.or(parsed.on_click);

        Ok(build_backlight_module(
            format,
            parsed.interval_secs,
            parsed.device,
            parsed.format_icons,
            click_command,
            parsed.class,
        )
        .upcast())
    }
}

fn default_backlight_interval() -> u32 {
    DEFAULT_BACKLIGHT_INTERVAL_SECS
}

fn default_backlight_icons() -> Vec<String> {
    vec![
        "".to_string(),
        "".to_string(),
        "".to_string(),
        "".to_string(),
        "".to_string(),
        "".to_string(),
        "".to_string(),
        "".to_string(),
        "".to_string(),
    ]
}

pub(crate) fn parse_config(module: &ModuleConfig) -> Result<BacklightConfig, String> {
    if module.module_type != MODULE_TYPE {
        return Err(format!(
            "expected module type '{}', got '{}'",
            MODULE_TYPE, module.module_type
        ));
    }

    serde_json::from_value(Value::Object(module.config.clone()))
        .map_err(|err| format!("invalid {} module config: {err}", MODULE_TYPE))
}

pub(crate) fn normalized_backlight_interval(interval_secs: u32) -> u32 {
    interval_secs.max(MIN_BACKLIGHT_INTERVAL_SECS)
}

fn build_backlight_module(
    format: String,
    interval_secs: u32,
    preferred_device: Option<String>,
    format_icons: Vec<String>,
    click_command: Option<String>,
    class: Option<String>,
) -> Label {
    let label = Label::new(None);
    label.add_css_class("module");
    label.add_css_class("backlight");

    apply_css_classes(&label, class.as_deref());
    attach_primary_click_command(&label, click_command);

    let effective_interval_secs = normalized_backlight_interval(interval_secs);
    if effective_interval_secs != interval_secs {
        eprintln!(
            "backlight interval_secs={} is too low; clamping to {} second",
            interval_secs, effective_interval_secs
        );
    }

    let (sender, receiver) = std::sync::mpsc::channel::<BacklightUiUpdate>();
    std::thread::spawn(move || {
        let _ = sender.send(build_backlight_ui_update(
            &format,
            preferred_device.as_deref(),
            &format_icons,
        ));

        spawn_udev_listener(
            sender.clone(),
            format.clone(),
            preferred_device.clone(),
            format_icons.clone(),
        );

        loop {
            std::thread::sleep(Duration::from_secs(u64::from(effective_interval_secs)));
            let _ = sender.send(build_backlight_ui_update(
                &format,
                preferred_device.as_deref(),
                &format_icons,
            ));
        }
    });

    glib::timeout_add_local(Duration::from_millis(UI_DRAIN_INTERVAL_MILLIS), {
        let label = label.clone();
        move || {
            while let Ok(update) = receiver.try_recv() {
                label.set_markup(&update.text);
                label.set_visible(update.visible);
                for class_name in BACKLIGHT_LEVEL_CLASSES {
                    label.remove_css_class(class_name);
                }
                label.add_css_class(update.level_class);
            }
            ControlFlow::Continue
        }
    });

    label
}

fn spawn_udev_listener(
    sender: Sender<BacklightUiUpdate>,
    format: String,
    preferred_device: Option<String>,
    format_icons: Vec<String>,
) {
    std::thread::spawn(move || {
        let builder = match udev::MonitorBuilder::new() {
            Ok(builder) => builder,
            Err(err) => {
                eprintln!("backlight udev listener unavailable, using polling only: {err}");
                return;
            }
        };
        let builder = match builder.match_subsystem("backlight") {
            Ok(builder) => builder,
            Err(err) => {
                eprintln!("backlight udev subsystem filter failed, using polling only: {err}");
                return;
            }
        };
        let monitor = match builder.listen() {
            Ok(monitor) => monitor,
            Err(err) => {
                eprintln!("backlight udev listen failed, using polling only: {err}");
                return;
            }
        };

        for _event in monitor.iter() {
            let _ = sender.send(build_backlight_ui_update(
                &format,
                preferred_device.as_deref(),
                &format_icons,
            ));
        }
    });
}

fn build_backlight_ui_update(
    format: &str,
    preferred_device: Option<&str>,
    format_icons: &[String],
) -> BacklightUiUpdate {
    match read_backlight_snapshot(preferred_device) {
        Ok(snapshot) => BacklightUiUpdate {
            text: render_format(format, &snapshot, format_icons),
            visible: snapshot.device.powered,
            level_class: brightness_css_class(snapshot.percent),
        },
        Err(err) => BacklightUiUpdate {
            text: escape_markup_text(&format!("backlight error: {err}")),
            visible: true,
            level_class: "brightness-unknown",
        },
    }
}

fn read_backlight_snapshot(preferred_device: Option<&str>) -> Result<BacklightSnapshot, String> {
    let devices = read_backlight_devices()?;
    let best = select_best_device(&devices, preferred_device)
        .ok_or_else(|| "no backlight devices found".to_string())?;

    let percent = if best.max_brightness == 0 {
        100
    } else {
        ((best.actual_brightness.saturating_mul(100)) / best.max_brightness).min(100) as u16
    };

    Ok(BacklightSnapshot {
        device: best.clone(),
        percent,
    })
}

fn read_backlight_devices() -> Result<Vec<BacklightDevice>, String> {
    let mut devices = Vec::new();
    let entries = fs::read_dir("/sys/class/backlight")
        .map_err(|err| format!("failed to read /sys/class/backlight: {err}"))?;

    for entry in entries {
        let entry =
            entry.map_err(|err| format!("failed to read backlight directory entry: {err}"))?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }

        let name = entry.file_name().to_string_lossy().to_string();
        let actual_brightness = read_actual_brightness(&path)?;
        let max_brightness = read_u64_field(&path, "max_brightness")?;
        let powered = read_powered_flag(&path).unwrap_or(true);

        devices.push(BacklightDevice {
            name,
            actual_brightness,
            max_brightness,
            powered,
        });
    }

    Ok(devices)
}

fn read_actual_brightness(device_path: &Path) -> Result<u64, String> {
    let actual_path = device_path.join("actual_brightness");
    if actual_path.exists() {
        read_u64_file(&actual_path)
            .map_err(|err| format!("failed to read {}: {err}", actual_path.display()))
    } else {
        read_u64_field(device_path, "brightness")
    }
}

fn read_u64_field(device_path: &Path, field: &str) -> Result<u64, String> {
    let field_path = device_path.join(field);
    read_u64_file(&field_path)
        .map_err(|err| format!("failed to read {}: {err}", field_path.display()))
}

fn read_u64_file(path: &Path) -> Result<u64, String> {
    let raw = fs::read_to_string(path).map_err(|err| err.to_string())?;
    raw.trim()
        .parse::<u64>()
        .map_err(|err| format!("failed to parse '{}' as integer: {err}", raw.trim()))
}

fn read_powered_flag(device_path: &Path) -> Option<bool> {
    let power_path = device_path.join("bl_power");
    let raw = fs::read_to_string(power_path).ok()?;
    raw.trim().parse::<u8>().ok().map(|value| value == 0)
}

fn select_best_device<'a>(
    devices: &'a [BacklightDevice],
    preferred_device: Option<&str>,
) -> Option<&'a BacklightDevice> {
    if let Some(preferred) = preferred_device {
        if let Some(device) = devices.iter().find(|device| device.name == preferred) {
            return Some(device);
        }
    }

    devices.iter().max_by_key(|device| device.max_brightness)
}

fn render_format(format: &str, snapshot: &BacklightSnapshot, format_icons: &[String]) -> String {
    let icon = icon_for_percent(format_icons, snapshot.percent);
    render_markup_template(
        format,
        &[
            ("{percent}", &snapshot.percent.to_string()),
            ("{icon}", &icon),
            (
                "{brightness}",
                &snapshot.device.actual_brightness.to_string(),
            ),
            ("{max}", &snapshot.device.max_brightness.to_string()),
            ("{device}", &snapshot.device.name),
        ],
    )
}

fn icon_for_percent(format_icons: &[String], percent: u16) -> String {
    if format_icons.is_empty() {
        return String::new();
    }
    if format_icons.len() == 1 {
        return format_icons[0].clone();
    }

    let clamped = percent.min(100) as usize;
    let index = (clamped * (format_icons.len() - 1)) / 100;
    format_icons[index].clone()
}

fn brightness_css_class(percent: u16) -> &'static str {
    if percent < 34 {
        "brightness-low"
    } else if percent < 67 {
        "brightness-medium"
    } else {
        "brightness-high"
    }
}

#[cfg(test)]
mod tests {
    use serde_json::Map;

    use super::*;

    #[test]
    fn parse_config_rejects_wrong_module_type() {
        let module = ModuleConfig::new("clock", Map::new());
        let err = parse_config(&module).expect_err("wrong type should fail");
        assert!(err.contains("expected module type 'backlight'"));
    }

    #[test]
    fn normalized_backlight_interval_enforces_lower_bound() {
        assert_eq!(normalized_backlight_interval(0), 1);
        assert_eq!(normalized_backlight_interval(1), 1);
        assert_eq!(normalized_backlight_interval(10), 10);
    }

    #[test]
    fn select_best_device_prefers_explicit_match() {
        let devices = vec![
            BacklightDevice {
                name: "intel_backlight".to_string(),
                actual_brightness: 100,
                max_brightness: 1200,
                powered: true,
            },
            BacklightDevice {
                name: "amdgpu_bl0".to_string(),
                actual_brightness: 80,
                max_brightness: 255,
                powered: true,
            },
        ];

        let selected = select_best_device(&devices, Some("amdgpu_bl0")).expect("device expected");
        assert_eq!(selected.name, "amdgpu_bl0");
    }

    #[test]
    fn select_best_device_uses_largest_max_brightness_without_preference() {
        let devices = vec![
            BacklightDevice {
                name: "intel_backlight".to_string(),
                actual_brightness: 100,
                max_brightness: 1200,
                powered: true,
            },
            BacklightDevice {
                name: "amdgpu_bl0".to_string(),
                actual_brightness: 80,
                max_brightness: 255,
                powered: true,
            },
        ];

        let selected = select_best_device(&devices, None).expect("device expected");
        assert_eq!(selected.name, "intel_backlight");
    }

    #[test]
    fn icon_for_percent_maps_full_range() {
        let icons = vec!["low".to_string(), "mid".to_string(), "high".to_string()];
        assert_eq!(icon_for_percent(&icons, 0), "low");
        assert_eq!(icon_for_percent(&icons, 50), "mid");
        assert_eq!(icon_for_percent(&icons, 100), "high");
    }

    #[test]
    fn render_format_replaces_placeholders() {
        let snapshot = BacklightSnapshot {
            device: BacklightDevice {
                name: "intel_backlight".to_string(),
                actual_brightness: 480,
                max_brightness: 960,
                powered: true,
            },
            percent: 50,
        };
        let icons = vec!["icon".to_string()];

        let rendered = render_format(
            "{percent} {icon} {brightness}/{max} {device}",
            &snapshot,
            &icons,
        );
        assert_eq!(rendered, "50 icon 480/960 intel_backlight");
    }
}
