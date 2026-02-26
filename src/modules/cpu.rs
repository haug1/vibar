use std::fs;
use std::time::Duration;

use gtk::glib::ControlFlow;
use gtk::prelude::*;
use gtk::{Label, Widget};
use serde::Deserialize;
use serde_json::Value;

use crate::modules::{
    apply_css_classes, attach_primary_click_command, escape_markup_text, render_markup_template,
    ModuleBuildContext, ModuleConfig,
};

use super::ModuleFactory;

const MIN_CPU_INTERVAL_SECS: u32 = 1;
const DEFAULT_CPU_INTERVAL_SECS: u32 = 5;
const DEFAULT_CPU_FORMAT: &str = "{used_percentage}%";
const CPU_USAGE_CLASSES: [&str; 5] = [
    "usage-low",
    "usage-medium",
    "usage-high",
    "usage-critical",
    "usage-unknown",
];
pub(crate) const MODULE_TYPE: &str = "cpu";

#[derive(Debug, Deserialize, Clone)]
pub(crate) struct CpuConfig {
    #[serde(default)]
    pub(crate) format: Option<String>,
    #[serde(default)]
    pub(crate) click: Option<String>,
    #[serde(rename = "on-click", default)]
    pub(crate) on_click: Option<String>,
    #[serde(default = "default_cpu_interval")]
    pub(crate) interval_secs: u32,
    #[serde(default)]
    pub(crate) class: Option<String>,
}

#[derive(Debug, Clone, Copy)]
struct CpuSnapshot {
    idle: u64,
    total: u64,
}

#[derive(Debug, Clone)]
struct CpuUpdate {
    text: String,
    usage_class: &'static str,
}

pub(crate) struct CpuFactory;

pub(crate) const FACTORY: CpuFactory = CpuFactory;

impl ModuleFactory for CpuFactory {
    fn module_type(&self) -> &'static str {
        MODULE_TYPE
    }

    fn init(&self, config: &ModuleConfig, _context: &ModuleBuildContext) -> Result<Widget, String> {
        let parsed = parse_config(config)?;
        let format = parsed
            .format
            .unwrap_or_else(|| DEFAULT_CPU_FORMAT.to_string());
        let click_command = parsed.click.or(parsed.on_click);

        Ok(build_cpu_module(format, click_command, parsed.interval_secs, parsed.class).upcast())
    }
}

fn default_cpu_interval() -> u32 {
    DEFAULT_CPU_INTERVAL_SECS
}

pub(crate) fn parse_config(module: &ModuleConfig) -> Result<CpuConfig, String> {
    if module.module_type != MODULE_TYPE {
        return Err(format!(
            "expected module type '{}', got '{}'",
            MODULE_TYPE, module.module_type
        ));
    }

    serde_json::from_value(Value::Object(module.config.clone()))
        .map_err(|err| format!("invalid {} module config: {err}", MODULE_TYPE))
}

pub(crate) fn normalized_cpu_interval(interval_secs: u32) -> u32 {
    interval_secs.max(MIN_CPU_INTERVAL_SECS)
}

pub(crate) fn build_cpu_module(
    format: String,
    click_command: Option<String>,
    interval_secs: u32,
    class: Option<String>,
) -> Label {
    let label = Label::new(None);
    label.add_css_class("module");
    label.add_css_class("cpu");

    apply_css_classes(&label, class.as_deref());
    attach_primary_click_command(&label, click_command);

    let effective_interval_secs = normalized_cpu_interval(interval_secs);
    if effective_interval_secs != interval_secs {
        eprintln!(
            "cpu interval_secs={} is too low; clamping to {} second",
            interval_secs, effective_interval_secs
        );
    }

    let (sender, receiver) = std::sync::mpsc::channel::<CpuUpdate>();
    let poll_format = format.clone();
    std::thread::spawn(move || {
        let mut previous: Option<CpuSnapshot> = None;

        loop {
            let update = match read_cpu_snapshot() {
                Ok(current) => {
                    let Some(prev) = previous else {
                        previous = Some(current);
                        std::thread::sleep(Duration::from_millis(100));
                        continue;
                    };
                    let usage = cpu_usage_between(prev, current);
                    previous = Some(current);
                    CpuUpdate {
                        text: render_format(&poll_format, usage),
                        usage_class: usage_css_class(usage),
                    }
                }
                Err(err) => CpuUpdate {
                    text: escape_markup_text(&format!("cpu error: {err}")),
                    usage_class: "usage-unknown",
                },
            };

            if sender.send(update).is_err() {
                return;
            }
            std::thread::sleep(Duration::from_secs(u64::from(effective_interval_secs)));
        }
    });

    let label_weak = label.downgrade();
    gtk::glib::timeout_add_local(Duration::from_millis(200), {
        move || {
            let Some(label) = label_weak.upgrade() else {
                return ControlFlow::Break;
            };
            while let Ok(update) = receiver.try_recv() {
                let visible = !update.text.trim().is_empty();
                label.set_visible(visible);
                if visible {
                    label.set_markup(&update.text);
                }
                for class_name in CPU_USAGE_CLASSES {
                    label.remove_css_class(class_name);
                }
                label.add_css_class(update.usage_class);
            }
            ControlFlow::Continue
        }
    });

    label
}

fn read_cpu_snapshot() -> Result<CpuSnapshot, String> {
    let stat = fs::read_to_string("/proc/stat")
        .map_err(|err| format!("failed to read /proc/stat: {err}"))?;
    parse_proc_stat_cpu_line(&stat)
}

fn parse_proc_stat_cpu_line(stat: &str) -> Result<CpuSnapshot, String> {
    let line = stat
        .lines()
        .find(|line| line.starts_with("cpu "))
        .ok_or_else(|| "missing aggregate cpu line in /proc/stat".to_string())?;

    let values: Vec<u64> = line
        .split_whitespace()
        .skip(1)
        .map(|field| {
            field
                .parse::<u64>()
                .map_err(|err| format!("failed to parse cpu stat value '{field}': {err}"))
        })
        .collect::<Result<Vec<_>, _>>()?;

    if values.len() < 4 {
        return Err("aggregate cpu line in /proc/stat has fewer than 4 columns".to_string());
    }

    let idle = values[3] + values.get(4).copied().unwrap_or(0);
    let total: u64 = values.iter().sum();

    Ok(CpuSnapshot { idle, total })
}

fn cpu_usage_between(previous: CpuSnapshot, current: CpuSnapshot) -> f64 {
    let delta_total = current.total.saturating_sub(previous.total);
    if delta_total == 0 {
        return 0.0;
    }

    let delta_idle = current.idle.saturating_sub(previous.idle);
    ((delta_total.saturating_sub(delta_idle)) as f64 / delta_total as f64) * 100.0
}

fn render_format(format: &str, used_percentage: f64) -> String {
    let used_percentage = used_percentage.clamp(0.0, 100.0) as u16;
    let idle_percentage = 100u16.saturating_sub(used_percentage);

    render_markup_template(
        format,
        &[
            ("{used_percentage}", &used_percentage.to_string()),
            ("{idle_percentage}", &idle_percentage.to_string()),
        ],
    )
}

fn usage_css_class(used_percentage: f64) -> &'static str {
    if used_percentage < 30.0 {
        "usage-low"
    } else if used_percentage < 60.0 {
        "usage-medium"
    } else if used_percentage < 85.0 {
        "usage-high"
    } else {
        "usage-critical"
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
        assert!(err.contains("expected module type 'cpu'"));
    }

    #[test]
    fn normalized_cpu_interval_enforces_lower_bound() {
        assert_eq!(normalized_cpu_interval(0), 1);
        assert_eq!(normalized_cpu_interval(1), 1);
        assert_eq!(normalized_cpu_interval(10), 10);
    }

    #[test]
    fn parse_proc_stat_cpu_line_parses_totals() {
        let stat = "cpu  100 20 30 400 50 0 0 0 0 0\ncpu0 1 2 3 4 5 6 7 8 9 10\n";
        let snapshot = parse_proc_stat_cpu_line(stat).expect("cpu line should parse");
        assert_eq!(snapshot.idle, 450);
        assert_eq!(snapshot.total, 600);
    }

    #[test]
    fn cpu_usage_between_uses_deltas() {
        let previous = CpuSnapshot {
            idle: 1000,
            total: 2000,
        };
        let current = CpuSnapshot {
            idle: 1040,
            total: 2100,
        };
        let usage = cpu_usage_between(previous, current);
        assert_eq!(format!("{usage:.0}"), "60");
    }

    #[test]
    fn render_format_replaces_placeholders() {
        let text = render_format("{used_percentage}% {idle_percentage}%", 62.4);
        assert_eq!(text, "62% 38%");
    }

    #[test]
    fn render_format_truncates_percentage() {
        let text = render_format("{used_percentage}% {idle_percentage}%", 62.9);
        assert_eq!(text, "62% 38%");
    }

    #[test]
    fn usage_css_class_matches_thresholds() {
        assert_eq!(usage_css_class(0.0), "usage-low");
        assert_eq!(usage_css_class(29.9), "usage-low");
        assert_eq!(usage_css_class(30.0), "usage-medium");
        assert_eq!(usage_css_class(59.9), "usage-medium");
        assert_eq!(usage_css_class(60.0), "usage-high");
        assert_eq!(usage_css_class(84.9), "usage-high");
        assert_eq!(usage_css_class(85.0), "usage-critical");
        assert_eq!(usage_css_class(100.0), "usage-critical");
    }
}
