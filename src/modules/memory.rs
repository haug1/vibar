use std::fs;
use std::time::Duration;

use glib::ControlFlow;
use gtk::prelude::*;
use gtk::{Label, Widget};
use serde::Deserialize;
use serde_json::Value;

use crate::modules::{
    apply_css_classes, attach_primary_click_command, ModuleBuildContext, ModuleConfig,
};

use super::ModuleFactory;

const MIN_MEMORY_INTERVAL_SECS: u32 = 1;
const DEFAULT_MEMORY_INTERVAL_SECS: u32 = 5;
const DEFAULT_MEMORY_FORMAT: &str = "{used_percentage}%";
pub(crate) const MODULE_TYPE: &str = "memory";

#[derive(Debug, Deserialize, Clone)]
pub(crate) struct MemoryConfig {
    #[serde(default)]
    pub(crate) format: Option<String>,
    #[serde(default)]
    pub(crate) click: Option<String>,
    #[serde(rename = "on-click", default)]
    pub(crate) on_click: Option<String>,
    #[serde(default = "default_memory_interval")]
    pub(crate) interval_secs: u32,
    #[serde(default)]
    pub(crate) class: Option<String>,
}

#[derive(Debug, Clone)]
struct MemoryStatus {
    total_bytes: u64,
    used_bytes: u64,
    free_bytes: u64,
    available_bytes: u64,
}

pub(crate) struct MemoryFactory;

pub(crate) const FACTORY: MemoryFactory = MemoryFactory;

impl ModuleFactory for MemoryFactory {
    fn module_type(&self) -> &'static str {
        MODULE_TYPE
    }

    fn init(&self, config: &ModuleConfig, _context: &ModuleBuildContext) -> Result<Widget, String> {
        let parsed = parse_config(config)?;
        let format = parsed
            .format
            .unwrap_or_else(|| DEFAULT_MEMORY_FORMAT.to_string());
        let click_command = parsed.click.or(parsed.on_click);

        Ok(build_memory_module(format, click_command, parsed.interval_secs, parsed.class).upcast())
    }
}

fn default_memory_interval() -> u32 {
    DEFAULT_MEMORY_INTERVAL_SECS
}

pub(crate) fn parse_config(module: &ModuleConfig) -> Result<MemoryConfig, String> {
    if module.module_type != MODULE_TYPE {
        return Err(format!(
            "expected module type '{}', got '{}'",
            MODULE_TYPE, module.module_type
        ));
    }

    serde_json::from_value(Value::Object(module.config.clone()))
        .map_err(|err| format!("invalid {} module config: {err}", MODULE_TYPE))
}

pub(crate) fn normalized_memory_interval(interval_secs: u32) -> u32 {
    interval_secs.max(MIN_MEMORY_INTERVAL_SECS)
}

pub(crate) fn build_memory_module(
    format: String,
    click_command: Option<String>,
    interval_secs: u32,
    class: Option<String>,
) -> Label {
    let label = Label::new(None);
    label.add_css_class("module");
    label.add_css_class("memory");

    apply_css_classes(&label, class.as_deref());
    attach_primary_click_command(&label, click_command);

    let effective_interval_secs = normalized_memory_interval(interval_secs);
    if effective_interval_secs != interval_secs {
        eprintln!(
            "memory interval_secs={} is too low; clamping to {} second",
            interval_secs, effective_interval_secs
        );
    }

    let (sender, receiver) = std::sync::mpsc::channel::<String>();
    let poll_format = format.clone();
    std::thread::spawn(move || loop {
        let text = match read_memory_status() {
            Ok(status) => render_format(&poll_format, &status),
            Err(err) => format!("memory error: {err}"),
        };
        let _ = sender.send(text);
        std::thread::sleep(Duration::from_secs(u64::from(effective_interval_secs)));
    });

    glib::timeout_add_local(Duration::from_millis(200), {
        let label = label.clone();
        move || {
            while let Ok(text) = receiver.try_recv() {
                label.set_text(&text);
            }
            ControlFlow::Continue
        }
    });

    label
}

fn read_memory_status() -> Result<MemoryStatus, String> {
    let meminfo = fs::read_to_string("/proc/meminfo")
        .map_err(|err| format!("failed to read /proc/meminfo: {err}"))?;
    parse_meminfo(&meminfo)
}

fn parse_meminfo(meminfo: &str) -> Result<MemoryStatus, String> {
    let mut total_kib: Option<u64> = None;
    let mut available_kib: Option<u64> = None;

    for line in meminfo.lines() {
        if total_kib.is_none() && line.starts_with("MemTotal:") {
            total_kib = parse_meminfo_line_value_kib(line);
        } else if available_kib.is_none() && line.starts_with("MemAvailable:") {
            available_kib = parse_meminfo_line_value_kib(line);
        }

        if total_kib.is_some() && available_kib.is_some() {
            break;
        }
    }

    let total_bytes = total_kib
        .ok_or_else(|| "missing MemTotal in /proc/meminfo".to_string())?
        .saturating_mul(1024);
    let available_bytes = available_kib
        .ok_or_else(|| "missing MemAvailable in /proc/meminfo".to_string())?
        .saturating_mul(1024);
    let available_bytes = available_bytes.min(total_bytes);
    let used_bytes = total_bytes.saturating_sub(available_bytes);

    Ok(MemoryStatus {
        total_bytes,
        used_bytes,
        free_bytes: total_bytes.saturating_sub(used_bytes),
        available_bytes,
    })
}

fn parse_meminfo_line_value_kib(line: &str) -> Option<u64> {
    line.split_whitespace().nth(1)?.parse::<u64>().ok()
}

fn render_format(format: &str, status: &MemoryStatus) -> String {
    let total = status.total_bytes as f64;
    let used_pct = if status.total_bytes == 0 {
        0.0
    } else {
        (status.used_bytes as f64 / total) * 100.0
    };
    let free_pct = if status.total_bytes == 0 {
        0.0
    } else {
        (status.free_bytes as f64 / total) * 100.0
    };
    let available_pct = if status.total_bytes == 0 {
        0.0
    } else {
        (status.available_bytes as f64 / total) * 100.0
    };

    format
        .replace("{used}", &format_bytes(status.used_bytes))
        .replace("{free}", &format_bytes(status.free_bytes))
        .replace("{available}", &format_bytes(status.available_bytes))
        .replace("{total}", &format_bytes(status.total_bytes))
        .replace("{used_percentage}", &format!("{used_pct:.0}"))
        .replace("{free_percentage}", &format!("{free_pct:.0}"))
        .replace("{available_percentage}", &format!("{available_pct:.0}"))
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
        assert!(err.contains("expected module type 'memory'"));
    }

    #[test]
    fn normalized_memory_interval_enforces_lower_bound() {
        assert_eq!(normalized_memory_interval(0), 1);
        assert_eq!(normalized_memory_interval(1), 1);
        assert_eq!(normalized_memory_interval(10), 10);
    }

    #[test]
    fn parse_meminfo_parses_bytes() {
        let meminfo = "MemTotal:       8000000 kB\nMemAvailable:   2000000 kB\n";
        let status = parse_meminfo(meminfo).expect("meminfo should parse");

        assert_eq!(status.total_bytes, 8_000_000 * 1024);
        assert_eq!(status.available_bytes, 2_000_000 * 1024);
        assert_eq!(status.used_bytes, 6_000_000 * 1024);
    }

    #[test]
    fn render_format_replaces_placeholders() {
        let status = MemoryStatus {
            total_bytes: 1000,
            used_bytes: 700,
            free_bytes: 300,
            available_bytes: 200,
        };
        let text = render_format("{used_percentage} {used} {available}", &status);
        assert_eq!(text, "70 700B 200B");
    }
}
