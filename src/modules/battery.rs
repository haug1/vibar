use std::fs;
use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use gtk::prelude::*;
use gtk::{Label, Widget};
use serde::Deserialize;
use serde_json::Value;

use crate::modules::broadcaster::{BackendRegistry, Broadcaster};
use crate::modules::{
    escape_markup_text, poll_receiver, render_markup_template, ModuleBuildContext, ModuleConfig,
    ModuleLabel,
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

struct BatteryBackend {
    preferred_device: Option<String>,
    snapshot: Option<BatterySnapshot>,
    last_error: Option<String>,
}

struct UdevMonitor {
    monitor: udev::MonitorSocket,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct BatterySharedKey {
    device: Option<String>,
    format: String,
    format_icons: Vec<String>,
    interval_secs: u32,
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
        "".to_string(),
        "".to_string(),
        "".to_string(),
        "".to_string(),
        "".to_string(),
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

fn battery_registry() -> &'static BackendRegistry<BatterySharedKey, Broadcaster<BatteryUiUpdate>> {
    static REGISTRY: OnceLock<BackendRegistry<BatterySharedKey, Broadcaster<BatteryUiUpdate>>> =
        OnceLock::new();
    REGISTRY.get_or_init(BackendRegistry::new)
}

fn subscribe_shared_battery(
    format: String,
    preferred_device: Option<String>,
    format_icons: Vec<String>,
    interval_secs: u32,
) -> std::sync::mpsc::Receiver<BatteryUiUpdate> {
    let key = BatterySharedKey {
        device: preferred_device.clone(),
        format: format.clone(),
        format_icons: format_icons.clone(),
        interval_secs,
    };

    let (broadcaster, start_worker) =
        battery_registry().get_or_create(key.clone(), Broadcaster::new);
    let receiver = broadcaster.subscribe();

    if start_worker {
        start_battery_worker(key, format, preferred_device, format_icons, broadcaster);
    }

    receiver
}

fn start_battery_worker(
    key: BatterySharedKey,
    format: String,
    preferred_device: Option<String>,
    format_icons: Vec<String>,
    broadcaster: Arc<Broadcaster<BatteryUiUpdate>>,
) {
    std::thread::spawn(move || {
        run_battery_backend_loop(
            &key,
            &broadcaster,
            &format,
            preferred_device,
            &format_icons,
            key.interval_secs,
        );
    });
}

pub(crate) fn build_battery_module(
    format: String,
    click_command: Option<String>,
    interval_secs: u32,
    preferred_device: Option<String>,
    format_icons: Vec<String>,
    class: Option<String>,
) -> Label {
    let label = ModuleLabel::new("battery")
        .with_css_classes(class.as_deref())
        .with_click_command(click_command)
        .into_label();

    let effective_interval_secs = normalized_battery_interval(interval_secs);
    if effective_interval_secs != interval_secs {
        eprintln!(
            "battery interval_secs={} is too low; clamping to {} second",
            interval_secs, effective_interval_secs
        );
    }

    let receiver = subscribe_shared_battery(
        format,
        preferred_device,
        format_icons,
        effective_interval_secs,
    );

    poll_receiver(&label, receiver, |label, update| {
        apply_battery_ui_update(label, &update);
    });

    label
}

fn apply_battery_ui_update(label: &Label, update: &BatteryUiUpdate) {
    let visible = update.visible && !update.text.trim().is_empty();
    label.set_visible(visible);
    if visible {
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

fn run_battery_backend_loop(
    key: &BatterySharedKey,
    broadcaster: &Arc<Broadcaster<BatteryUiUpdate>>,
    format: &str,
    preferred_device: Option<String>,
    format_icons: &[String],
    interval_secs: u32,
) {
    let resync_interval = Duration::from_secs(u64::from(interval_secs));
    let mut last_resync = Instant::now();
    let mut backend = BatteryBackend::new(preferred_device);
    let mut udev_monitor = match UdevMonitor::new() {
        Ok(monitor) => Some(monitor),
        Err(err) => {
            eprintln!("battery udev listener unavailable, using polling only: {err}");
            None
        }
    };

    backend.refresh_from_sysfs();
    broadcaster.broadcast(backend.build_ui_update(format, format_icons));

    loop {
        if broadcaster.subscriber_count() == 0 {
            battery_registry().remove(key, broadcaster);
            return;
        }

        let wake_timeout = millis_until_next_resync(last_resync, resync_interval).min(50);

        if let Some(monitor) = udev_monitor.as_mut() {
            match wait_for_readable_fd(monitor.fd(), wake_timeout) {
                Ok(true) => {
                    if monitor.drain_events() {
                        backend.refresh_from_sysfs();
                        broadcaster.broadcast(backend.build_ui_update(format, format_icons));
                    }
                }
                Ok(false) => {}
                Err(err) => {
                    eprintln!("battery udev wait failed, listener stopped: {err}");
                    udev_monitor = None;
                }
            }
        } else {
            std::thread::sleep(Duration::from_millis(wake_timeout.max(1)));
        }

        if last_resync.elapsed() >= resync_interval {
            backend.refresh_from_sysfs();
            broadcaster.broadcast(backend.build_ui_update(format, format_icons));
            last_resync = Instant::now();
        }
    }
}

fn millis_until_next_resync(last_resync: Instant, interval: Duration) -> u64 {
    let elapsed = last_resync.elapsed();
    if elapsed >= interval {
        return 0;
    }

    interval
        .saturating_sub(elapsed)
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

impl BatteryBackend {
    fn new(preferred_device: Option<String>) -> Self {
        Self {
            preferred_device,
            snapshot: None,
            last_error: None,
        }
    }

    fn refresh_from_sysfs(&mut self) {
        match read_battery_snapshot(
            Path::new(POWER_SUPPLY_PATH),
            self.preferred_device.as_deref(),
        ) {
            Ok(snapshot) => {
                self.snapshot = snapshot;
                self.last_error = None;
            }
            Err(err) => {
                self.snapshot = None;
                self.last_error = Some(err);
            }
        }
    }

    fn build_ui_update(&self, format: &str, format_icons: &[String]) -> BatteryUiUpdate {
        if let Some(snapshot) = self.snapshot.as_ref() {
            let text = render_format(format, snapshot, format_icons);
            return BatteryUiUpdate {
                visible: !text.trim().is_empty(),
                text,
                level_class: battery_level_css_class(snapshot.capacity),
                status_class: battery_status_css_class(&snapshot.status),
            };
        }

        if let Some(err) = self.last_error.as_deref() {
            return BatteryUiUpdate {
                text: escape_markup_text(&format!("battery error: {err}")),
                visible: true,
                level_class: "battery-unknown",
                status_class: "status-unknown",
            };
        }

        BatteryUiUpdate {
            text: String::new(),
            visible: false,
            level_class: "battery-unknown",
            status_class: "status-unknown",
        }
    }
}

impl UdevMonitor {
    fn new() -> Result<Self, String> {
        let builder = udev::MonitorBuilder::new().map_err(|err| err.to_string())?;
        let builder = builder
            .match_subsystem("power_supply")
            .map_err(|err| err.to_string())?;
        let monitor = builder.listen().map_err(|err| err.to_string())?;

        Ok(Self { monitor })
    }

    fn fd(&self) -> i32 {
        self.monitor.as_raw_fd()
    }

    fn drain_events(&mut self) -> bool {
        let mut had_event = false;
        for _ in self.monitor.iter() {
            had_event = true;
        }
        had_event
    }
}

fn wait_for_readable_fd(fd: i32, timeout_millis: u64) -> Result<bool, String> {
    let mut pollfd = libc::pollfd {
        fd,
        events: libc::POLLIN,
        revents: 0,
    };

    let timeout_millis = timeout_millis.min(i32::MAX as u64) as i32;

    loop {
        // SAFETY: we pass a valid pointer to one pollfd entry and a correct count.
        let rc = unsafe { libc::poll(&mut pollfd, 1, timeout_millis) };
        if rc > 0 {
            if (pollfd.revents & libc::POLLIN) != 0 {
                return Ok(true);
            }
            return Err(format!("unexpected poll events: {}", pollfd.revents));
        }

        if rc == 0 {
            return Ok(false);
        }

        let err = std::io::Error::last_os_error();
        if err.kind() == std::io::ErrorKind::Interrupted {
            continue;
        }
        return Err(format!("poll failed: {err}"));
    }
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
