use std::fs;
use std::path::{Path, PathBuf};
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

const POWER_SUPPLY_PATH: &str = "/sys/class/power_supply";
const MIN_BATTERY_INTERVAL_SECS: u32 = 1;
const DEFAULT_BATTERY_INTERVAL_SECS: u32 = 10;
const DEFAULT_BATTERY_FORMAT: &str = "{capacity}% {icon}";
const BATTERY_LEVEL_CLASSES: [&str; 5] = [
    "battery-critical",
    "battery-low",
    "battery-medium",
    "battery-high",
    "battery-unknown",
];
const BATTERY_STATUS_CLASSES: [&str; 5] = [
    "status-charging",
    "status-discharging",
    "status-full",
    "status-not-charging",
    "status-unknown",
];
pub(crate) const MODULE_TYPE: &str = "battery";

#[derive(Debug, Deserialize, Clone)]
pub(crate) struct BatteryConfig {
    #[serde(default)]
    pub(crate) format: Option<String>,
    #[serde(default)]
    pub(crate) click: Option<String>,
    #[serde(rename = "on-click", default)]
    pub(crate) on_click: Option<String>,
    #[serde(default = "default_battery_interval")]
    pub(crate) interval_secs: u32,
    #[serde(default)]
    pub(crate) device: Option<String>,
    #[serde(rename = "format-icons", default = "default_battery_icons")]
    pub(crate) format_icons: Vec<String>,
    #[serde(default)]
    pub(crate) class: Option<String>,
}

#[derive(Debug, Clone)]
struct BatterySnapshot {
    device_name: String,
    capacity: u8,
    status: String,
}

#[derive(Debug, Clone)]
struct BatteryUiUpdate {
    text: String,
    visible: bool,
    level_class: &'static str,
    status_class: &'static str,
}

pub(crate) struct BatteryFactory;

pub(crate) const FACTORY: BatteryFactory = BatteryFactory;

impl ModuleFactory for BatteryFactory {
    fn module_type(&self) -> &'static str {
        MODULE_TYPE
    }

    fn init(&self, config: &ModuleConfig, _context: &ModuleBuildContext) -> Result<Widget, String> {
        let parsed = parse_config(config)?;
        let format = parsed
            .format
            .unwrap_or_else(|| DEFAULT_BATTERY_FORMAT.to_string());
        let click_command = parsed.click.or(parsed.on_click);

        Ok(build_battery_module(
            format,
            click_command,
            parsed.interval_secs,
            parsed.device,
            parsed.format_icons,
            parsed.class,
        )
        .upcast())
    }
}

fn default_battery_interval() -> u32 {
    DEFAULT_BATTERY_INTERVAL_SECS
}

fn default_battery_icons() -> Vec<String> {
    vec![
        "".to_string(),
        "".to_string(),
        "".to_string(),
        "".to_string(),
        "".to_string(),
    ]
}

pub(crate) fn parse_config(module: &ModuleConfig) -> Result<BatteryConfig, String> {
    if module.module_type != MODULE_TYPE {
        return Err(format!(
            "expected module type '{}', got '{}'",
            MODULE_TYPE, module.module_type
        ));
    }

    serde_json::from_value(Value::Object(module.config.clone()))
        .map_err(|err| format!("invalid {} module config: {err}", MODULE_TYPE))
}

pub(crate) fn normalized_battery_interval(interval_secs: u32) -> u32 {
    interval_secs.max(MIN_BATTERY_INTERVAL_SECS)
}

pub(crate) fn build_battery_module(
    format: String,
    click_command: Option<String>,
    interval_secs: u32,
    preferred_device: Option<String>,
    format_icons: Vec<String>,
    class: Option<String>,
) -> Label {
    let label = Label::new(None);
    label.add_css_class("module");
    label.add_css_class("battery");

    apply_css_classes(&label, class.as_deref());
    attach_primary_click_command(&label, click_command);

    let effective_interval_secs = normalized_battery_interval(interval_secs);
    if effective_interval_secs != interval_secs {
        eprintln!(
            "battery interval_secs={} is too low; clamping to {} second",
            interval_secs, effective_interval_secs
        );
    }

    let (sender, receiver) = std::sync::mpsc::channel::<BatteryUiUpdate>();
    let poll_format = format.clone();
    std::thread::spawn(move || loop {
        let update = match read_battery_snapshot(
            Path::new(POWER_SUPPLY_PATH),
            preferred_device.as_deref(),
        ) {
            Ok(Some(snapshot)) => BatteryUiUpdate {
                text: render_format(&poll_format, &snapshot, &format_icons),
                visible: true,
                level_class: battery_level_css_class(snapshot.capacity),
                status_class: battery_status_css_class(&snapshot.status),
            },
            Ok(None) => BatteryUiUpdate {
                text: String::new(),
                visible: false,
                level_class: "battery-unknown",
                status_class: "status-unknown",
            },
            Err(err) => BatteryUiUpdate {
                text: escape_markup_text(&format!("battery error: {err}")),
                visible: true,
                level_class: "battery-unknown",
                status_class: "status-unknown",
            },
        };

        let _ = sender.send(update);
        std::thread::sleep(Duration::from_secs(u64::from(effective_interval_secs)));
    });

    glib::timeout_add_local(Duration::from_millis(200), {
        let label = label.clone();
        move || {
            while let Ok(update) = receiver.try_recv() {
                label.set_visible(update.visible);
                if update.visible {
                    label.set_markup(&update.text);
                }

                for class_name in BATTERY_LEVEL_CLASSES {
                    label.remove_css_class(class_name);
                }
                for class_name in BATTERY_STATUS_CLASSES {
                    label.remove_css_class(class_name);
                }
                label.add_css_class(update.level_class);
                label.add_css_class(update.status_class);
            }
            ControlFlow::Continue
        }
    });

    label
}

fn read_battery_snapshot(
    power_supply_root: &Path,
    preferred_device: Option<&str>,
) -> Result<Option<BatterySnapshot>, String> {
    let Some(device_path) = select_battery_device(power_supply_root, preferred_device)? else {
        return Ok(None);
    };
    let device_name = device_path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| format!("invalid battery device name: {}", device_path.display()))?
        .to_string();
    let capacity = read_percentage_file(&device_path.join("capacity"))?;
    let status = read_trimmed_or_default(&device_path.join("status"), "Unknown");

    Ok(Some(BatterySnapshot {
        device_name,
        capacity,
        status,
    }))
}

fn select_battery_device(
    power_supply_root: &Path,
    preferred_device: Option<&str>,
) -> Result<Option<PathBuf>, String> {
    if let Some(device) = preferred_device {
        let preferred_path = power_supply_root.join(device);
        if !preferred_path.exists() {
            return Err(format!(
                "preferred battery device '{}' not found in {}",
                device,
                power_supply_root.display()
            ));
        }
        if !is_battery_device(&preferred_path) {
            return Err(format!(
                "preferred device '{}' is not a battery device",
                preferred_path.display()
            ));
        }
        return Ok(Some(preferred_path));
    }

    let entries = fs::read_dir(power_supply_root)
        .map_err(|err| format!("failed to read {}: {err}", power_supply_root.display()))?;
    let mut candidates = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|err| format!("failed to read power-supply entry: {err}"))?;
        let path = entry.path();
        if is_battery_device(&path) {
            candidates.push(path);
        }
    }

    candidates.sort_by(|a, b| a.file_name().cmp(&b.file_name()));
    Ok(candidates.into_iter().next())
}

fn is_battery_device(path: &Path) -> bool {
    if !path.is_dir() || !path.join("capacity").is_file() {
        return false;
    }

    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    if name.starts_with("BAT") {
        return true;
    }

    let type_path = path.join("type");
    if let Ok(device_type) = fs::read_to_string(type_path) {
        return device_type.trim().eq_ignore_ascii_case("battery");
    }

    false
}

fn read_percentage_file(path: &Path) -> Result<u8, String> {
    let raw = fs::read_to_string(path)
        .map_err(|err| format!("failed to read {}: {err}", path.display()))?;
    let parsed = raw
        .trim()
        .parse::<u16>()
        .map_err(|err| format!("failed to parse '{}' as percentage: {err}", raw.trim()))?;
    Ok(parsed.min(100) as u8)
}

fn read_trimmed_or_default(path: &Path, default: &str) -> String {
    fs::read_to_string(path)
        .map(|value| {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                default.to_string()
            } else {
                trimmed.to_string()
            }
        })
        .unwrap_or_else(|_| default.to_string())
}

fn render_format(format: &str, snapshot: &BatterySnapshot, format_icons: &[String]) -> String {
    let icon = icon_for_capacity(format_icons, snapshot.capacity);
    render_markup_template(
        format,
        &[
            ("{capacity}", &snapshot.capacity.to_string()),
            ("{percent}", &snapshot.capacity.to_string()),
            ("{status}", &snapshot.status),
            ("{icon}", &icon),
            ("{device}", &snapshot.device_name),
        ],
    )
}

fn icon_for_capacity(format_icons: &[String], capacity: u8) -> String {
    if format_icons.is_empty() {
        return String::new();
    }
    if format_icons.len() == 1 {
        return format_icons[0].clone();
    }

    let clamped = capacity.min(100) as usize;
    let index = (clamped * (format_icons.len() - 1)) / 100;
    format_icons[index].clone()
}

fn battery_level_css_class(capacity: u8) -> &'static str {
    if capacity < 15 {
        "battery-critical"
    } else if capacity < 35 {
        "battery-low"
    } else if capacity < 70 {
        "battery-medium"
    } else {
        "battery-high"
    }
}

fn battery_status_css_class(status: &str) -> &'static str {
    if status.eq_ignore_ascii_case("charging") {
        "status-charging"
    } else if status.eq_ignore_ascii_case("discharging") {
        "status-discharging"
    } else if status.eq_ignore_ascii_case("full") {
        "status-full"
    } else if status.eq_ignore_ascii_case("not charging") {
        "status-not-charging"
    } else {
        "status-unknown"
    }
}

#[cfg(test)]
mod tests {
    use std::env;
    use std::time::{SystemTime, UNIX_EPOCH};

    use serde_json::Map;

    use super::*;

    fn test_dir(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time should move forward")
            .as_nanos();
        env::temp_dir().join(format!("vibar-battery-test-{name}-{nanos}"))
    }

    fn write(path: &Path, value: &str) {
        fs::write(path, value).expect("test file should write");
    }

    #[test]
    fn parse_config_rejects_wrong_module_type() {
        let module = ModuleConfig::new("clock", Map::new());
        let err = parse_config(&module).expect_err("wrong type should fail");
        assert!(err.contains("expected module type 'battery'"));
    }

    #[test]
    fn normalized_battery_interval_enforces_lower_bound() {
        assert_eq!(normalized_battery_interval(0), 1);
        assert_eq!(normalized_battery_interval(1), 1);
        assert_eq!(normalized_battery_interval(15), 15);
    }

    #[test]
    fn select_battery_device_prefers_explicit_device() {
        let root = test_dir("preferred");
        let bat0 = root.join("BAT0");
        fs::create_dir_all(&bat0).expect("battery dir should create");
        write(&bat0.join("capacity"), "55");
        write(&bat0.join("type"), "Battery");

        let selected =
            select_battery_device(&root, Some("BAT0")).expect("device selection should succeed");
        assert_eq!(selected, Some(bat0));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn select_battery_device_auto_picks_sorted_candidate() {
        let root = test_dir("auto");
        let ac = root.join("AC");
        let bat1 = root.join("BAT1");
        let bat0 = root.join("BAT0");
        fs::create_dir_all(&ac).expect("ac dir should create");
        fs::create_dir_all(&bat1).expect("bat1 dir should create");
        fs::create_dir_all(&bat0).expect("bat0 dir should create");
        write(&ac.join("type"), "Mains");
        write(&bat1.join("capacity"), "44");
        write(&bat1.join("type"), "Battery");
        write(&bat0.join("capacity"), "70");
        write(&bat0.join("type"), "Battery");

        let selected = select_battery_device(&root, None).expect("device selection should succeed");
        assert_eq!(selected, Some(bat0));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn read_battery_snapshot_returns_none_when_not_found() {
        let root = test_dir("none");
        fs::create_dir_all(&root).expect("root dir should create");

        let snapshot = read_battery_snapshot(&root, None).expect("read should succeed");
        assert!(snapshot.is_none());

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn render_format_replaces_placeholders() {
        let snapshot = BatterySnapshot {
            device_name: "BAT0".to_string(),
            capacity: 42,
            status: "Discharging".to_string(),
        };
        let icons = vec!["low".to_string(), "high".to_string()];
        let rendered = render_format(
            "{capacity} {percent} {status} {icon} {device}",
            &snapshot,
            &icons,
        );
        assert_eq!(rendered, "42 42 Discharging low BAT0");
    }

    #[test]
    fn icon_for_capacity_maps_full_range() {
        let icons = vec!["low".to_string(), "mid".to_string(), "high".to_string()];
        assert_eq!(icon_for_capacity(&icons, 0), "low");
        assert_eq!(icon_for_capacity(&icons, 50), "mid");
        assert_eq!(icon_for_capacity(&icons, 100), "high");
    }

    #[test]
    fn battery_status_css_class_maps_known_statuses() {
        assert_eq!(battery_status_css_class("Charging"), "status-charging");
        assert_eq!(
            battery_status_css_class("Discharging"),
            "status-discharging"
        );
        assert_eq!(battery_status_css_class("Full"), "status-full");
        assert_eq!(
            battery_status_css_class("Not charging"),
            "status-not-charging"
        );
        assert_eq!(battery_status_css_class("Unknown"), "status-unknown");
    }
}
