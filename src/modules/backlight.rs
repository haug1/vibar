use std::fs;
use std::os::fd::AsRawFd;
use std::path::Path;
use std::process::Command;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use gtk::prelude::*;
use gtk::{EventControllerScroll, EventControllerScrollFlags, Label, Widget};
use serde::Deserialize;
use serde_json::Value;
use zbus::blocking::{Connection, Proxy};

use crate::modules::broadcaster::{attach_subscription, BackendRegistry, Subscription};
use crate::modules::{
    escape_markup_text, render_markup_template, ModuleBuildContext, ModuleConfig, ModuleLabel,
};

use super::ModuleFactory;

const MIN_BACKLIGHT_INTERVAL_SECS: u32 = 1;
const DEFAULT_BACKLIGHT_INTERVAL_SECS: u32 = 2;
const DEFAULT_SCROLL_STEP: f64 = 1.0;
const DEFAULT_MIN_BRIGHTNESS: f64 = 0.0;
const DEFAULT_BACKLIGHT_FORMAT: &str = "{percent}% {icon}";
const BACKEND_WAKE_POLL_MILLIS: u64 = 50;
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
    #[serde(rename = "on-scroll-up", default)]
    pub(crate) on_scroll_up: Option<String>,
    #[serde(rename = "on-scroll-down", default)]
    pub(crate) on_scroll_down: Option<String>,
    #[serde(rename = "scroll-step", default = "default_scroll_step")]
    pub(crate) scroll_step: f64,
    #[serde(rename = "min-brightness", default = "default_min_brightness")]
    pub(crate) min_brightness: f64,
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

#[derive(Debug, Clone)]
enum BacklightControlMessage {
    AdjustByPercent {
        increase: bool,
        step_percent: f64,
        min_percent: f64,
    },
}

struct BacklightBackend {
    preferred_device: Option<String>,
    devices: Vec<BacklightDevice>,
    selected: Option<BacklightSnapshot>,
    last_error: Option<String>,
}

struct UdevMonitor {
    monitor: udev::MonitorSocket,
}

/// Shared state for backlight: broadcast for UI updates + control channel for
/// brightness adjustments (scroll events).
struct SharedBacklightState {
    broadcaster: crate::modules::broadcaster::Broadcaster<BacklightUiUpdate>,
    control_tx: std::sync::Mutex<Sender<BacklightControlMessage>>,
    control_rx: std::sync::Mutex<Option<Receiver<BacklightControlMessage>>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct BacklightSharedKey {
    device: Option<String>,
    format: String,
    format_icons: Vec<String>,
    interval_secs: u32,
}

pub(crate) struct BacklightFactory;

pub(crate) const FACTORY: BacklightFactory = BacklightFactory;

impl ModuleFactory for BacklightFactory {
    fn module_type(&self) -> &'static str {
        MODULE_TYPE
    }

    fn init(&self, config: &ModuleConfig, _context: &ModuleBuildContext) -> Result<Widget, String> {
        let parsed = parse_config(config)?;
        Ok(build_backlight_module(parsed).upcast())
    }
}

fn default_backlight_interval() -> u32 {
    DEFAULT_BACKLIGHT_INTERVAL_SECS
}

fn default_scroll_step() -> f64 {
    DEFAULT_SCROLL_STEP
}

fn default_min_brightness() -> f64 {
    DEFAULT_MIN_BRIGHTNESS
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

fn backlight_registry() -> &'static BackendRegistry<BacklightSharedKey, SharedBacklightState> {
    static REGISTRY: OnceLock<BackendRegistry<BacklightSharedKey, SharedBacklightState>> =
        OnceLock::new();
    REGISTRY.get_or_init(BackendRegistry::new)
}

fn subscribe_shared_backlight(
    config: &BacklightConfig,
    effective_interval_secs: u32,
) -> (
    Subscription<BacklightUiUpdate>,
    Sender<BacklightControlMessage>,
) {
    let format = config
        .format
        .clone()
        .unwrap_or_else(|| DEFAULT_BACKLIGHT_FORMAT.to_string());
    let key = BacklightSharedKey {
        device: config.device.clone(),
        format: format.clone(),
        format_icons: config.format_icons.clone(),
        interval_secs: effective_interval_secs,
    };

    let (shared, start_worker) = backlight_registry().get_or_create(key.clone(), || {
        let (control_tx, control_rx) = mpsc::channel();
        SharedBacklightState {
            broadcaster: crate::modules::broadcaster::Broadcaster::new(),
            control_tx: std::sync::Mutex::new(control_tx),
            control_rx: std::sync::Mutex::new(Some(control_rx)),
        }
    });

    let ui_rx = shared.broadcaster.subscribe();
    let control_tx = shared
        .control_tx
        .lock()
        .expect("backlight control_tx mutex poisoned")
        .clone();

    if start_worker {
        let control_rx = shared
            .control_rx
            .lock()
            .expect("backlight control_rx mutex poisoned")
            .take()
            .expect("control_rx should be present on first create");
        start_backlight_worker(key, shared, control_rx, format, config.clone());
    }

    (ui_rx, control_tx)
}

fn start_backlight_worker(
    key: BacklightSharedKey,
    shared: Arc<SharedBacklightState>,
    control_rx: Receiver<BacklightControlMessage>,
    format: String,
    config: BacklightConfig,
) {
    std::thread::spawn(move || {
        run_backlight_backend_loop(
            &key,
            &shared,
            control_rx,
            format,
            config.device,
            config.format_icons,
            key.interval_secs,
        );
    });
}

fn build_backlight_module(config: BacklightConfig) -> Label {
    let BacklightConfig {
        click,
        on_click,
        on_scroll_up,
        on_scroll_down,
        scroll_step,
        min_brightness,
        class,
        interval_secs,
        ..
    } = config.clone();
    let click_command = click.or(on_click);

    let label = ModuleLabel::new("backlight")
        .with_css_classes(class.as_deref())
        .with_click_command(click_command)
        .into_label();

    let effective_interval_secs = normalized_backlight_interval(interval_secs);
    if effective_interval_secs != interval_secs {
        eprintln!(
            "backlight interval_secs={} is too low; clamping to {} second",
            interval_secs, effective_interval_secs
        );
    }

    let (ui_subscription, control_tx) =
        subscribe_shared_backlight(&config, effective_interval_secs);

    attach_subscription(&label, ui_subscription, |label, update| {
        apply_backlight_ui_update(label, &update);
    });

    let scroll_step = normalized_scroll_step(scroll_step);
    if scroll_step > 0.0 || on_scroll_up.is_some() || on_scroll_down.is_some() {
        let scroll = EventControllerScroll::new(
            EventControllerScrollFlags::VERTICAL | EventControllerScrollFlags::DISCRETE,
        );

        if on_scroll_up.is_some() || on_scroll_down.is_some() {
            let up_command = on_scroll_up;
            let down_command = on_scroll_down;
            scroll.connect_scroll(move |_, _, dy| {
                if dy < 0.0 {
                    if let Some(command) = up_command.as_ref() {
                        spawn_shell_command(command);
                    }
                    return gtk::glib::Propagation::Stop;
                }
                if dy > 0.0 {
                    if let Some(command) = down_command.as_ref() {
                        spawn_shell_command(command);
                    }
                    return gtk::glib::Propagation::Stop;
                }
                gtk::glib::Propagation::Proceed
            });
        } else if scroll_step > 0.0 {
            let clamped_min_brightness = min_brightness.clamp(0.0, 100.0);
            scroll.connect_scroll(move |_, _, dy| {
                if dy < 0.0 {
                    let _ = control_tx.send(BacklightControlMessage::AdjustByPercent {
                        increase: true,
                        step_percent: scroll_step,
                        min_percent: clamped_min_brightness,
                    });
                    return gtk::glib::Propagation::Stop;
                }
                if dy > 0.0 {
                    let _ = control_tx.send(BacklightControlMessage::AdjustByPercent {
                        increase: false,
                        step_percent: scroll_step,
                        min_percent: clamped_min_brightness,
                    });
                    return gtk::glib::Propagation::Stop;
                }
                gtk::glib::Propagation::Proceed
            });
        }

        label.add_controller(scroll);
    }

    label
}

fn apply_backlight_ui_update(label: &Label, update: &BacklightUiUpdate) {
    let visible = update.visible && !update.text.trim().is_empty();
    label.set_visible(visible);
    if visible {
        label.set_markup(&update.text);
    }
    for class_name in BACKLIGHT_LEVEL_CLASSES {
        label.remove_css_class(class_name);
    }
    label.add_css_class(update.level_class);
}

fn run_backlight_backend_loop(
    key: &BacklightSharedKey,
    shared: &Arc<SharedBacklightState>,
    control_rx: Receiver<BacklightControlMessage>,
    format: String,
    preferred_device: Option<String>,
    format_icons: Vec<String>,
    interval_secs: u32,
) {
    let resync_interval = Duration::from_secs(u64::from(interval_secs));
    let mut last_resync = Instant::now();
    let mut backend = BacklightBackend::new(preferred_device);
    let mut udev_monitor = match UdevMonitor::new() {
        Ok(monitor) => Some(monitor),
        Err(err) => {
            eprintln!("backlight udev listener unavailable, using polling only: {err}");
            None
        }
    };

    backend.refresh_from_sysfs();
    shared
        .broadcaster
        .broadcast(backend.build_ui_update(&format, &format_icons));

    loop {
        if shared.broadcaster.subscriber_count() == 0 {
            backlight_registry().remove(key, shared);
            return;
        }

        while let Ok(message) = control_rx.try_recv() {
            if let Err(err) = backend.apply_control_message(message) {
                backend.last_error = Some(err);
            }
            backend.refresh_from_sysfs();
            shared
                .broadcaster
                .broadcast(backend.build_ui_update(&format, &format_icons));
        }

        let wake_timeout =
            millis_until_next_resync(last_resync, resync_interval).min(BACKEND_WAKE_POLL_MILLIS);

        if let Some(monitor) = udev_monitor.as_mut() {
            match wait_for_readable_fd(monitor.fd(), wake_timeout) {
                Ok(true) => {
                    if monitor.drain_events() {
                        backend.refresh_from_sysfs();
                        shared
                            .broadcaster
                            .broadcast(backend.build_ui_update(&format, &format_icons));
                    }
                }
                Ok(false) => {}
                Err(err) => {
                    eprintln!("backlight udev wait failed, listener stopped: {err}");
                    udev_monitor = None;
                }
            }
        } else {
            std::thread::sleep(Duration::from_millis(wake_timeout.max(1)));
        }

        if last_resync.elapsed() >= resync_interval {
            backend.refresh_from_sysfs();
            shared
                .broadcaster
                .broadcast(backend.build_ui_update(&format, &format_icons));
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

impl BacklightBackend {
    fn new(preferred_device: Option<String>) -> Self {
        Self {
            preferred_device,
            devices: Vec::new(),
            selected: None,
            last_error: None,
        }
    }

    fn refresh_from_sysfs(&mut self) {
        match read_backlight_devices() {
            Ok(devices) => {
                self.devices = devices;
                let selected = select_best_device(&self.devices, self.preferred_device.as_deref())
                    .cloned()
                    .map(snapshot_from_device);
                self.selected = selected;
                self.last_error = if self.selected.is_some() {
                    None
                } else {
                    Some("no backlight devices found".to_string())
                };
            }
            Err(err) => {
                self.last_error = Some(err);
                self.selected = None;
            }
        }
    }

    fn apply_control_message(&self, message: BacklightControlMessage) -> Result<(), String> {
        match message {
            BacklightControlMessage::AdjustByPercent {
                increase,
                step_percent,
                min_percent,
            } => {
                let device = self
                    .selected
                    .as_ref()
                    .map(|snapshot| snapshot.device.clone())
                    .ok_or_else(|| "no backlight devices found".to_string())?;
                set_backlight_by_percent_delta_for_device(
                    &device,
                    increase,
                    step_percent,
                    min_percent,
                )
            }
        }
    }

    fn build_ui_update(&self, format: &str, format_icons: &[String]) -> BacklightUiUpdate {
        if let Some(snapshot) = self.selected.as_ref() {
            return BacklightUiUpdate {
                text: render_format(format, snapshot, format_icons),
                visible: snapshot.device.powered,
                level_class: brightness_css_class(snapshot.percent),
            };
        }

        let error = self
            .last_error
            .as_deref()
            .unwrap_or("no backlight devices found");

        BacklightUiUpdate {
            text: escape_markup_text(&format!("backlight error: {error}")),
            visible: true,
            level_class: "brightness-unknown",
        }
    }
}

impl UdevMonitor {
    fn new() -> Result<Self, String> {
        let builder = udev::MonitorBuilder::new().map_err(|err| err.to_string())?;
        let builder = builder
            .match_subsystem("backlight")
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

fn spawn_shell_command(command: &str) {
    let command = command.to_string();
    std::thread::spawn(move || {
        let _ = Command::new("sh").arg("-c").arg(command).spawn();
    });
}

fn normalized_scroll_step(step: f64) -> f64 {
    if step <= 0.0 || !step.is_finite() {
        0.0
    } else {
        step
    }
}

fn set_backlight_by_percent_delta_for_device(
    device: &BacklightDevice,
    increase: bool,
    step_percent: f64,
    min_percent: f64,
) -> Result<(), String> {
    let max = device.max_brightness;
    if max == 0 {
        return Err("backlight max_brightness is 0".to_string());
    }

    let step_abs = ((step_percent.clamp(0.0, 100.0) / 100.0) * max as f64).round() as u64;
    if step_abs == 0 {
        return Ok(());
    }

    let min_abs = ((min_percent.clamp(0.0, 100.0) / 100.0) * max as f64).round() as u64;
    let current = device.actual_brightness;
    let mut target = if increase {
        current.saturating_add(step_abs).min(max)
    } else {
        current.saturating_sub(step_abs)
    };

    if !increase && current <= min_abs {
        return Ok(());
    }
    if !increase {
        target = target.max(min_abs);
    }

    if target == current {
        return Ok(());
    }

    set_brightness_via_logind(&device.name, target as u32)
}

fn set_brightness_via_logind(device_name: &str, brightness: u32) -> Result<(), String> {
    let connection =
        Connection::system().map_err(|err| format!("failed to connect to system dbus: {err}"))?;

    for session_path in [
        "/org/freedesktop/login1/session/auto",
        "/org/freedesktop/login1/session/self",
    ] {
        let proxy = Proxy::new(
            &connection,
            "org.freedesktop.login1",
            session_path,
            "org.freedesktop.login1.Session",
        )
        .map_err(|err| format!("failed to create login1 proxy: {err}"))?;

        if proxy
            .call_method("SetBrightness", &("backlight", device_name, brightness))
            .is_ok()
        {
            return Ok(());
        }
    }

    Err("failed to set brightness via login1 SetBrightness".to_string())
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

fn snapshot_from_device(device: BacklightDevice) -> BacklightSnapshot {
    let percent = if device.max_brightness == 0 {
        100
    } else {
        ((device.actual_brightness.saturating_mul(100)) / device.max_brightness).min(100) as u16
    };

    BacklightSnapshot { device, percent }
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
    fn normalized_scroll_step_disables_invalid_values() {
        assert_eq!(normalized_scroll_step(-1.0), 0.0);
        assert_eq!(normalized_scroll_step(0.0), 0.0);
        assert_eq!(normalized_scroll_step(f64::NAN), 0.0);
        assert_eq!(normalized_scroll_step(2.0), 2.0);
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
