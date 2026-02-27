use std::ffi::CString;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use gtk::prelude::*;
use gtk::{Label, Widget};
use serde::Deserialize;
use serde_json::Value;

use crate::modules::broadcaster::{
    attach_subscription, BackendRegistry, Broadcaster, Subscription,
};
use crate::modules::{
    escape_markup_text, render_markup_template, ModuleBuildContext, ModuleConfig, ModuleLabel,
};

use super::ModuleFactory;

const MIN_DISK_INTERVAL_SECS: u32 = 1;
const DEFAULT_DISK_INTERVAL_SECS: u32 = 30;
const DEFAULT_DISK_PATH: &str = "/";
const DEFAULT_DISK_FORMAT: &str = "{free}";
pub(crate) const MODULE_TYPE: &str = "disk";

#[derive(Debug, Deserialize, Clone)]
pub(crate) struct DiskConfig {
    #[serde(default)]
    pub(crate) format: Option<String>,
    #[serde(default)]
    pub(crate) click: Option<String>,
    #[serde(rename = "on-click", default)]
    pub(crate) on_click: Option<String>,
    #[serde(default = "default_disk_interval")]
    pub(crate) interval_secs: u32,
    #[serde(default)]
    pub(crate) path: Option<String>,
    #[serde(default)]
    pub(crate) class: Option<String>,
}

#[derive(Debug, Clone)]
struct DiskStatus {
    path: String,
    free_bytes: u64,
    used_bytes: u64,
    total_bytes: u64,
}

#[derive(Debug, Clone)]
struct DiskUpdate {
    text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct DiskSharedKey {
    path: String,
    format: String,
    interval_secs: u32,
}

pub(crate) struct DiskFactory;

pub(crate) const FACTORY: DiskFactory = DiskFactory;

impl ModuleFactory for DiskFactory {
    fn module_type(&self) -> &'static str {
        MODULE_TYPE
    }

    fn init(&self, config: &ModuleConfig, _context: &ModuleBuildContext) -> Result<Widget, String> {
        let parsed = parse_config(config)?;
        let path = parsed.path.unwrap_or_else(|| DEFAULT_DISK_PATH.to_string());
        let format = parsed
            .format
            .unwrap_or_else(|| DEFAULT_DISK_FORMAT.to_string());
        let click_command = parsed.click.or(parsed.on_click);

        Ok(build_disk_module(
            path,
            format,
            click_command,
            parsed.interval_secs,
            parsed.class,
        )
        .upcast())
    }
}

fn default_disk_interval() -> u32 {
    DEFAULT_DISK_INTERVAL_SECS
}

pub(crate) fn parse_config(module: &ModuleConfig) -> Result<DiskConfig, String> {
    if module.module_type != MODULE_TYPE {
        return Err(format!(
            "expected module type '{}', got '{}'",
            MODULE_TYPE, module.module_type
        ));
    }

    serde_json::from_value(Value::Object(module.config.clone()))
        .map_err(|err| format!("invalid {} module config: {err}", MODULE_TYPE))
}

pub(crate) fn normalized_disk_interval(interval_secs: u32) -> u32 {
    interval_secs.max(MIN_DISK_INTERVAL_SECS)
}

fn disk_registry() -> &'static BackendRegistry<DiskSharedKey, Broadcaster<DiskUpdate>> {
    static REGISTRY: OnceLock<BackendRegistry<DiskSharedKey, Broadcaster<DiskUpdate>>> =
        OnceLock::new();
    REGISTRY.get_or_init(BackendRegistry::new)
}

fn subscribe_shared_disk(
    path: String,
    format: String,
    interval_secs: u32,
) -> Subscription<DiskUpdate> {
    let key = DiskSharedKey {
        path,
        format,
        interval_secs,
    };

    let (broadcaster, start_worker) = disk_registry().get_or_create(key.clone(), Broadcaster::new);
    let receiver = broadcaster.subscribe();

    if start_worker {
        start_disk_worker(key, broadcaster);
    }

    receiver
}

fn start_disk_worker(key: DiskSharedKey, broadcaster: Arc<Broadcaster<DiskUpdate>>) {
    let interval = Duration::from_secs(u64::from(key.interval_secs));
    std::thread::spawn(move || loop {
        let text = match read_disk_status(&key.path) {
            Ok(status) => render_format(&key.format, &status),
            Err(err) => escape_markup_text(&format!("disk error: {err}")),
        };
        broadcaster.broadcast(DiskUpdate { text });
        if broadcaster.subscriber_count() == 0 {
            disk_registry().remove(&key, &broadcaster);
            return;
        }
        std::thread::sleep(interval);
    });
}

pub(crate) fn build_disk_module(
    path: String,
    format: String,
    click_command: Option<String>,
    interval_secs: u32,
    class: Option<String>,
) -> Label {
    let label = ModuleLabel::new("disk")
        .with_css_classes(class.as_deref())
        .with_click_command(click_command)
        .into_label();

    let effective_interval_secs = normalized_disk_interval(interval_secs);
    if effective_interval_secs != interval_secs {
        eprintln!(
            "disk interval_secs={} is too low; clamping to {} second",
            interval_secs, effective_interval_secs
        );
    }

    let subscription = subscribe_shared_disk(path, format, effective_interval_secs);

    attach_subscription(&label, subscription, |label, update| {
        let visible = !update.text.trim().is_empty();
        label.set_visible(visible);
        if visible {
            label.set_markup(&update.text);
        }
    });

    label
}

fn read_disk_status(path: &str) -> Result<DiskStatus, String> {
    let c_path =
        CString::new(path).map_err(|_| format!("invalid path (contains null byte): {path}"))?;
    let mut stat: libc::statvfs = unsafe { std::mem::zeroed() };
    let ret = unsafe { libc::statvfs(c_path.as_ptr(), &mut stat) };
    if ret != 0 {
        return Err(format!(
            "statvfs failed for '{path}': {}",
            std::io::Error::last_os_error()
        ));
    }
    let block = stat.f_frsize;
    let total_bytes = stat.f_blocks * block;
    let free_bytes = stat.f_bavail * block;
    let used_bytes = total_bytes.saturating_sub(stat.f_bfree * block);
    Ok(DiskStatus {
        path: path.to_string(),
        free_bytes,
        used_bytes,
        total_bytes,
    })
}

fn render_format(format: &str, status: &DiskStatus) -> String {
    let free_pct = if status.total_bytes == 0 {
        0.0
    } else {
        (status.free_bytes as f64 / status.total_bytes as f64) * 100.0
    };
    let used_pct = if status.total_bytes == 0 {
        0.0
    } else {
        (status.used_bytes as f64 / status.total_bytes as f64) * 100.0
    };

    render_markup_template(
        format,
        &[
            ("{path}", &status.path),
            ("{free}", &format_bytes(status.free_bytes)),
            ("{used}", &format_bytes(status.used_bytes)),
            ("{total}", &format_bytes(status.total_bytes)),
            ("{percentage_free}", &format!("{free_pct:.0}")),
            ("{percentage_used}", &format!("{used_pct:.0}")),
        ],
    )
}

fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 6] = ["B", "K", "M", "G", "T", "P"];

    let mut value = bytes as f64;
    let mut unit_index = 0usize;
    while value >= 1024.0 && unit_index < UNITS.len() - 1 {
        value /= 1024.0;
        unit_index += 1;
    }

    if unit_index == 0 {
        format!("{bytes}{}", UNITS[unit_index])
    } else {
        let rounded = format!("{value:.1}");
        let compact = rounded.trim_end_matches('0').trim_end_matches('.');
        format!("{compact}{}", UNITS[unit_index])
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
        assert!(err.contains("expected module type 'disk'"));
    }

    #[test]
    fn normalized_disk_interval_enforces_lower_bound() {
        assert_eq!(normalized_disk_interval(0), 1);
        assert_eq!(normalized_disk_interval(1), 1);
        assert_eq!(normalized_disk_interval(10), 10);
    }

    #[test]
    fn read_disk_status_returns_nonzero_for_root() {
        let status = read_disk_status("/").expect("statvfs on / should succeed");
        assert!(status.total_bytes > 0);
        assert!(status.free_bytes > 0);
        assert_eq!(status.path, "/");
    }

    #[test]
    fn read_disk_status_rejects_nonexistent_path() {
        let result = read_disk_status("/nonexistent/path/that/does/not/exist");
        assert!(result.is_err());
    }

    #[test]
    fn render_format_replaces_placeholders() {
        let status = DiskStatus {
            path: "/".to_string(),
            free_bytes: 600,
            used_bytes: 400,
            total_bytes: 1000,
        };
        let text = render_format("{free} {path} {percentage_used}", &status);
        assert_eq!(text, "600B / 40");
    }
}
