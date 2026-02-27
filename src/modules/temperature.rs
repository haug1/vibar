use std::fs;
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

const MIN_TEMPERATURE_INTERVAL_SECS: u32 = 1;
const DEFAULT_TEMPERATURE_INTERVAL_SECS: u32 = 10;
const DEFAULT_TEMPERATURE_FORMAT: &str = "{temperatureC}°C {icon}";
const TEMPERATURE_STATE_CLASSES: [&str; 4] = [
    "temperature-normal",
    "temperature-warning",
    "temperature-critical",
    "temperature-unknown",
];
pub(crate) const MODULE_TYPE: &str = "temperature";

#[derive(Debug, Deserialize, Clone)]
pub(crate) struct TemperatureConfig {
    #[serde(default)]
    pub(crate) format: Option<String>,
    #[serde(rename = "format-warning", default)]
    pub(crate) format_warning: Option<String>,
    #[serde(rename = "format-critical", default)]
    pub(crate) format_critical: Option<String>,
    #[serde(default = "default_temperature_interval")]
    pub(crate) interval_secs: u32,
    #[serde(rename = "path", alias = "hwmon-path", alias = "hwmon_path", default)]
    pub(crate) sensor_path: Option<String>,
    #[serde(rename = "thermal-zone", alias = "thermal_zone", default)]
    pub(crate) thermal_zone: Option<u32>,
    #[serde(rename = "warning-threshold", alias = "warning_threshold", default)]
    pub(crate) warning_threshold: Option<i32>,
    #[serde(rename = "critical-threshold", alias = "critical_threshold", default)]
    pub(crate) critical_threshold: Option<i32>,
    #[serde(rename = "format-icons", default = "default_temperature_icons")]
    pub(crate) format_icons: Vec<String>,
    #[serde(default)]
    pub(crate) click: Option<String>,
    #[serde(rename = "on-click", default)]
    pub(crate) on_click: Option<String>,
    #[serde(default)]
    pub(crate) class: Option<String>,
}

#[derive(Debug, Clone, Copy)]
struct TemperatureReading {
    celsius: f64,
}

#[derive(Debug, Clone)]
struct TemperatureUiUpdate {
    text: String,
    state_class: &'static str,
    visible: bool,
}

#[derive(Debug, Clone)]
struct TemperatureRuntimeConfig {
    sensor_path: String,
    base_format: String,
    warning_format: Option<String>,
    critical_format: Option<String>,
    warning_threshold: Option<i32>,
    critical_threshold: Option<i32>,
    format_icons: Vec<String>,
    interval_secs: u32,
    click_command: Option<String>,
    class: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct TemperatureSharedKey {
    sensor_path: String,
    base_format: String,
    warning_format: Option<String>,
    critical_format: Option<String>,
    warning_threshold: Option<i32>,
    critical_threshold: Option<i32>,
    format_icons: Vec<String>,
    interval_secs: u32,
}

pub(crate) struct TemperatureFactory;

pub(crate) const FACTORY: TemperatureFactory = TemperatureFactory;

impl ModuleFactory for TemperatureFactory {
    fn module_type(&self) -> &'static str {
        MODULE_TYPE
    }

    fn init(&self, config: &ModuleConfig, _context: &ModuleBuildContext) -> Result<Widget, String> {
        let parsed = parse_config(config)?;
        let click_command = parsed.click.or(parsed.on_click);
        let base_format = parsed
            .format
            .unwrap_or_else(|| DEFAULT_TEMPERATURE_FORMAT.to_string());

        Ok(build_temperature_module(TemperatureRuntimeConfig {
            sensor_path: resolve_temperature_sensor_path(
                parsed.sensor_path.clone(),
                parsed.thermal_zone,
            ),
            base_format,
            warning_format: parsed.format_warning,
            critical_format: parsed.format_critical,
            warning_threshold: parsed.warning_threshold,
            critical_threshold: parsed.critical_threshold,
            format_icons: parsed.format_icons,
            interval_secs: parsed.interval_secs,
            click_command,
            class: parsed.class,
        })
        .upcast())
    }
}

fn default_temperature_interval() -> u32 {
    DEFAULT_TEMPERATURE_INTERVAL_SECS
}

fn default_temperature_icons() -> Vec<String> {
    vec![
        "".to_string(),
        "".to_string(),
        "".to_string(),
        "".to_string(),
        "".to_string(),
    ]
}

pub(crate) fn parse_config(module: &ModuleConfig) -> Result<TemperatureConfig, String> {
    if module.module_type != MODULE_TYPE {
        return Err(format!(
            "expected module type '{}', got '{}'",
            MODULE_TYPE, module.module_type
        ));
    }

    serde_json::from_value(Value::Object(module.config.clone()))
        .map_err(|err| format!("invalid {} module config: {err}", MODULE_TYPE))
}

pub(crate) fn normalized_temperature_interval(interval_secs: u32) -> u32 {
    interval_secs.max(MIN_TEMPERATURE_INTERVAL_SECS)
}

fn resolve_temperature_sensor_path(
    explicit_path: Option<String>,
    thermal_zone: Option<u32>,
) -> String {
    if let Some(path) = explicit_path {
        return path;
    }

    let zone = thermal_zone.unwrap_or(0);
    format!("/sys/class/thermal/thermal_zone{zone}/temp")
}

fn temperature_registry(
) -> &'static BackendRegistry<TemperatureSharedKey, Broadcaster<TemperatureUiUpdate>> {
    static REGISTRY: OnceLock<
        BackendRegistry<TemperatureSharedKey, Broadcaster<TemperatureUiUpdate>>,
    > = OnceLock::new();
    REGISTRY.get_or_init(BackendRegistry::new)
}

fn subscribe_shared_temperature(
    config: &TemperatureRuntimeConfig,
) -> Subscription<TemperatureUiUpdate> {
    let key = TemperatureSharedKey {
        sensor_path: config.sensor_path.clone(),
        base_format: config.base_format.clone(),
        warning_format: config.warning_format.clone(),
        critical_format: config.critical_format.clone(),
        warning_threshold: config.warning_threshold,
        critical_threshold: config.critical_threshold,
        format_icons: config.format_icons.clone(),
        interval_secs: config.interval_secs,
    };

    let (broadcaster, start_worker) =
        temperature_registry().get_or_create(key.clone(), Broadcaster::new);
    let receiver = broadcaster.subscribe();

    if start_worker {
        start_temperature_worker(key, config.clone(), broadcaster);
    }

    receiver
}

fn start_temperature_worker(
    key: TemperatureSharedKey,
    config: TemperatureRuntimeConfig,
    broadcaster: Arc<Broadcaster<TemperatureUiUpdate>>,
) {
    let interval = Duration::from_secs(u64::from(config.interval_secs));
    std::thread::spawn(move || loop {
        let update = match read_temperature_reading(&config.sensor_path) {
            Ok(reading) => {
                let state_class = temperature_state_class(
                    reading,
                    config.warning_threshold,
                    config.critical_threshold,
                );
                let chosen_format = match state_class {
                    "temperature-critical" => config
                        .critical_format
                        .as_deref()
                        .unwrap_or(config.base_format.as_str()),
                    "temperature-warning" => config
                        .warning_format
                        .as_deref()
                        .unwrap_or(config.base_format.as_str()),
                    _ => config.base_format.as_str(),
                };
                let text = render_temperature_format(chosen_format, reading, &config.format_icons);

                TemperatureUiUpdate {
                    visible: !text.trim().is_empty(),
                    text,
                    state_class,
                }
            }
            Err(err) => TemperatureUiUpdate {
                text: escape_markup_text(&format!("temperature error: {err}")),
                state_class: "temperature-unknown",
                visible: true,
            },
        };

        broadcaster.broadcast(update);
        if broadcaster.subscriber_count() == 0 {
            temperature_registry().remove(&key, &broadcaster);
            return;
        }
        std::thread::sleep(interval);
    });
}

fn build_temperature_module(config: TemperatureRuntimeConfig) -> Label {
    let label = ModuleLabel::new("temperature")
        .with_css_classes(config.class.as_deref())
        .with_click_command(config.click_command.clone())
        .into_label();

    let effective_interval_secs = normalized_temperature_interval(config.interval_secs);
    if effective_interval_secs != config.interval_secs {
        eprintln!(
            "temperature interval_secs={} is too low; clamping to {} second",
            config.interval_secs, effective_interval_secs
        );
    }

    let config = TemperatureRuntimeConfig {
        interval_secs: effective_interval_secs,
        ..config
    };

    let subscription = subscribe_shared_temperature(&config);

    attach_subscription(&label, subscription, |label, update| {
        label.set_visible(update.visible);
        if update.visible {
            label.set_markup(&update.text);
        }
        for class_name in TEMPERATURE_STATE_CLASSES {
            label.remove_css_class(class_name);
        }
        label.add_css_class(update.state_class);
    });

    label
}

fn read_temperature_reading(sensor_path: &str) -> Result<TemperatureReading, String> {
    let raw = fs::read_to_string(sensor_path)
        .map_err(|err| format!("failed to read {sensor_path}: {err}"))?;
    let parsed = raw
        .trim()
        .parse::<i64>()
        .map_err(|err| format!("failed to parse '{}' as integer: {err}", raw.trim()))?;
    let celsius = if parsed.abs() >= 1_000 {
        parsed as f64 / 1000.0
    } else {
        parsed as f64
    };
    Ok(TemperatureReading { celsius })
}

fn temperature_state_class(
    reading: TemperatureReading,
    warning_threshold: Option<i32>,
    critical_threshold: Option<i32>,
) -> &'static str {
    let celsius = reading.celsius.round() as i32;

    if let Some(critical) = critical_threshold {
        if celsius >= critical {
            return "temperature-critical";
        }
    }
    if let Some(warning) = warning_threshold {
        if celsius >= warning {
            return "temperature-warning";
        }
    }
    "temperature-normal"
}

fn render_temperature_format(
    format: &str,
    reading: TemperatureReading,
    format_icons: &[String],
) -> String {
    let celsius = reading.celsius.round() as i32;
    let fahrenheit = (reading.celsius * 1.8 + 32.0).round() as i32;
    let kelvin = (reading.celsius + 273.15).round() as i32;
    let icon = super::icon_for_percentage(format_icons, celsius.clamp(0, 100) as u8);

    render_markup_template(
        format,
        &[
            ("{temperature_c}", &celsius.to_string()),
            ("{temperature_f}", &fahrenheit.to_string()),
            ("{temperature_k}", &kelvin.to_string()),
            ("{temperatureC}", &celsius.to_string()),
            ("{temperatureF}", &fahrenheit.to_string()),
            ("{temperatureK}", &kelvin.to_string()),
            ("{icon}", icon),
        ],
    )
}

#[cfg(test)]
mod tests {
    use std::env;
    use std::path::Path;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use serde_json::Map;

    use super::*;

    fn test_path(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time should move forward")
            .as_nanos();
        env::temp_dir().join(format!("vibar-temp-test-{name}-{nanos}"))
    }

    fn write(path: &Path, value: &str) {
        fs::write(path, value).expect("test file should write");
    }

    #[test]
    fn parse_config_rejects_wrong_module_type() {
        let module = ModuleConfig::new("clock", Map::new());
        let err = parse_config(&module).expect_err("wrong type should fail");
        assert!(err.contains("expected module type 'temperature'"));
    }

    #[test]
    fn normalized_temperature_interval_enforces_lower_bound() {
        assert_eq!(normalized_temperature_interval(0), 1);
        assert_eq!(normalized_temperature_interval(1), 1);
        assert_eq!(normalized_temperature_interval(10), 10);
    }

    #[test]
    fn resolve_temperature_sensor_path_prefers_explicit_path() {
        let path = resolve_temperature_sensor_path(Some("/tmp/sensor".to_string()), Some(4));
        assert_eq!(path, "/tmp/sensor");
    }

    #[test]
    fn resolve_temperature_sensor_path_uses_thermal_zone() {
        let path = resolve_temperature_sensor_path(None, Some(2));
        assert_eq!(path, "/sys/class/thermal/thermal_zone2/temp");
    }

    #[test]
    fn read_temperature_reading_parses_millidegree_values() {
        let path = test_path("millidegree");
        write(&path, "42500\n");

        let reading = read_temperature_reading(path.to_str().expect("utf8 path"))
            .expect("temperature should parse");
        assert_eq!(reading.celsius, 42.5);

        let _ = fs::remove_file(path);
    }

    #[test]
    fn render_temperature_format_replaces_placeholders() {
        let text = render_temperature_format(
            "{temperatureC} {temperatureF} {temperatureK} {icon}",
            TemperatureReading { celsius: 42.5 },
            &["cold".to_string(), "hot".to_string()],
        );

        assert_eq!(text, "43 109 316 cold");
    }

    #[test]
    fn temperature_state_class_applies_thresholds() {
        assert_eq!(
            temperature_state_class(TemperatureReading { celsius: 44.0 }, Some(45), Some(80)),
            "temperature-normal"
        );
        assert_eq!(
            temperature_state_class(TemperatureReading { celsius: 45.0 }, Some(45), Some(80)),
            "temperature-warning"
        );
        assert_eq!(
            temperature_state_class(TemperatureReading { celsius: 80.0 }, Some(45), Some(80)),
            "temperature-critical"
        );
    }

    #[test]
    fn temperature_visibility_hides_when_selected_format_is_empty() {
        let empty = "";
        let text = render_temperature_format(empty, TemperatureReading { celsius: 42.0 }, &[]);
        assert!(text.trim().is_empty());
    }
}
