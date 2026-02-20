use std::process::Command;
use std::time::Duration;

use glib::ControlFlow;
use gtk::prelude::*;
use gtk::{GestureClick, Label, Widget};
use serde::Deserialize;
use serde_json::Value;

use crate::modules::{ModuleBuildContext, ModuleConfig};

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

pub(crate) fn build_disk_module(
    path: String,
    format: String,
    click_command: Option<String>,
    interval_secs: u32,
    class: Option<String>,
) -> Label {
    let label = Label::new(None);
    label.add_css_class("module");
    label.add_css_class("disk");

    if let Some(class_name) = class {
        label.add_css_class(&class_name);
    }

    if let Some(command) = click_command {
        let click = GestureClick::builder().button(1).build();
        click.connect_pressed(move |_, _, _, _| {
            run_click_command(&command);
        });
        label.add_controller(click);
    }

    let effective_interval_secs = normalized_disk_interval(interval_secs);
    if effective_interval_secs != interval_secs {
        eprintln!(
            "disk interval_secs={} is too low; clamping to {} second",
            interval_secs, effective_interval_secs
        );
    }

    let (sender, receiver) = std::sync::mpsc::channel::<String>();
    let poll_path = path.clone();
    let poll_format = format.clone();
    std::thread::spawn(move || loop {
        let text = match read_disk_status(&poll_path) {
            Ok(status) => render_format(&poll_format, &status),
            Err(err) => format!("disk error: {err}"),
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

fn run_click_command(command: &str) {
    let command = command.to_string();
    std::thread::spawn(move || {
        let _ = Command::new("sh").arg("-c").arg(command).spawn();
    });
}

fn read_disk_status(path: &str) -> Result<DiskStatus, String> {
    let output = Command::new("df")
        .arg("-B1")
        .arg("-P")
        .arg(path)
        .output()
        .map_err(|err| format!("failed to run df: {err}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(if stderr.is_empty() {
            format!("df exited with status {}", output.status)
        } else {
            format!("df failed: {stderr}")
        });
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    parse_df_output(path, &stdout)
}

fn parse_df_output(requested_path: &str, stdout: &str) -> Result<DiskStatus, String> {
    let line = stdout
        .lines()
        .nth(1)
        .ok_or_else(|| "missing df output row".to_string())?;
    let columns: Vec<&str> = line.split_whitespace().collect();
    if columns.len() < 6 {
        return Err(format!(
            "unexpected df output row (expected >=6 columns): '{line}'"
        ));
    }

    let total_bytes = columns[1]
        .parse::<u64>()
        .map_err(|err| format!("failed to parse total bytes '{}': {err}", columns[1]))?;
    let used_bytes = columns[2]
        .parse::<u64>()
        .map_err(|err| format!("failed to parse used bytes '{}': {err}", columns[2]))?;
    let free_bytes = columns[3]
        .parse::<u64>()
        .map_err(|err| format!("failed to parse free bytes '{}': {err}", columns[3]))?;
    let path = columns[5].to_string();

    Ok(DiskStatus {
        path: if path.is_empty() {
            requested_path.to_string()
        } else {
            path
        },
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

    format
        .replace("{path}", &status.path)
        .replace("{free}", &format_bytes(status.free_bytes))
        .replace("{used}", &format_bytes(status.used_bytes))
        .replace("{total}", &format_bytes(status.total_bytes))
        .replace("{percentage_free}", &format!("{free_pct:.0}"))
        .replace("{percentage_used}", &format!("{used_pct:.0}"))
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
    fn parse_df_output_parses_bytes() {
        let output =
            "Filesystem 1B-blocks Used Available Use% Mounted on\n/dev/sda1 1000 400 600 40% /\n";
        let status = parse_df_output("/", output).expect("df output should parse");

        assert_eq!(status.total_bytes, 1000);
        assert_eq!(status.used_bytes, 400);
        assert_eq!(status.free_bytes, 600);
        assert_eq!(status.path, "/");
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
