use chrono::Local;
use glib::ControlFlow;
use gtk::prelude::*;
use gtk::{Label, Widget};
use serde::Deserialize;
use serde_json::{Map, Value};

use crate::modules::{
    apply_css_classes, attach_primary_click_command, render_markup_template, ModuleBuildContext,
    ModuleConfig,
};

use super::ModuleFactory;

const DEFAULT_CLOCK_FMT: &str = "%a %d. %b %H:%M:%S";
const DEFAULT_CLOCK_TEMPLATE: &str = "{}";
pub(crate) const MODULE_TYPE: &str = "clock";

#[derive(Debug, Deserialize, Clone)]
pub(crate) struct ClockConfig {
    #[serde(default)]
    pub(crate) format: Option<String>,
    #[serde(rename = "time-format", alias = "time_format", default)]
    pub(crate) time_format: Option<String>,
    #[serde(default)]
    pub(crate) click: Option<String>,
    #[serde(rename = "on-click", default)]
    pub(crate) on_click: Option<String>,
    #[serde(default)]
    pub(crate) class: Option<String>,
}

pub(crate) struct ClockFactory;

pub(crate) const FACTORY: ClockFactory = ClockFactory;

impl ModuleFactory for ClockFactory {
    fn module_type(&self) -> &'static str {
        MODULE_TYPE
    }

    fn init(&self, config: &ModuleConfig, _context: &ModuleBuildContext) -> Result<Widget, String> {
        let parsed = parse_config(config)?;
        let click_command = parsed.click.or(parsed.on_click);
        Ok(build_clock_module(
            parsed.format,
            parsed.time_format,
            click_command,
            parsed.class,
        )
        .upcast())
    }
}

pub(crate) fn default_module_config() -> ModuleConfig {
    let mut map = Map::new();
    map.insert("time-format".to_string(), Value::Null);
    map.insert("format".to_string(), Value::Null);
    ModuleConfig::new(MODULE_TYPE, map)
}

fn parse_config(module: &ModuleConfig) -> Result<ClockConfig, String> {
    if module.module_type != MODULE_TYPE {
        return Err(format!(
            "expected module type '{}', got '{}'",
            MODULE_TYPE, module.module_type
        ));
    }

    serde_json::from_value(Value::Object(module.config.clone()))
        .map_err(|err| format!("invalid {} module config: {err}", MODULE_TYPE))
}

pub(crate) fn build_clock_module(
    format: Option<String>,
    time_format: Option<String>,
    click_command: Option<String>,
    class: Option<String>,
) -> Label {
    let label = Label::new(None);
    label.add_css_class("module");
    label.add_css_class("clock");
    apply_css_classes(&label, class.as_deref());
    attach_primary_click_command(&label, click_command);

    let (template, time_fmt) = resolve_clock_formats(format, time_format);

    let update = {
        let label = label.clone();
        let template = template.clone();
        let time_fmt = time_fmt.clone();
        move || {
            let now = Local::now();
            let rendered_time = now.format(&time_fmt).to_string();
            let rendered = render_markup_template(&template, &[("{}", &rendered_time)]);
            let visible = !rendered.trim().is_empty();
            label.set_visible(visible);
            if visible {
                label.set_markup(&rendered);
            }
        }
    };

    update();

    let label_weak = label.downgrade();
    glib::timeout_add_seconds_local(1, move || {
        let Some(label) = label_weak.upgrade() else {
            return ControlFlow::Break;
        };

        let now = Local::now();
        let rendered_time = now.format(&time_fmt).to_string();
        let rendered = render_markup_template(&template, &[("{}", &rendered_time)]);
        let visible = !rendered.trim().is_empty();
        label.set_visible(visible);
        if visible {
            label.set_markup(&rendered);
        }

        ControlFlow::Continue
    });

    label
}

fn resolve_clock_formats(format: Option<String>, time_format: Option<String>) -> (String, String) {
    let template = format.unwrap_or_else(|| DEFAULT_CLOCK_TEMPLATE.to_string());
    let time_fmt = time_format.unwrap_or_else(|| DEFAULT_CLOCK_FMT.to_string());
    (template, time_fmt)
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use serde_json::Map;

    use super::*;

    #[test]
    fn parse_config_rejects_wrong_module_type() {
        let module = ModuleConfig::new("exec", Map::new());
        let err = parse_config(&module).expect_err("wrong type should fail");
        assert!(err.contains("expected module type 'clock'"));
    }

    #[test]
    fn parse_config_supports_click_aliases() {
        let click_module = ModuleConfig::new(
            MODULE_TYPE,
            serde_json::from_value(json!({
                "click": "foo"
            }))
            .expect("module config map should parse"),
        );
        let click_cfg = parse_config(&click_module).expect("click config should parse");
        assert_eq!(click_cfg.click.as_deref(), Some("foo"));

        let on_click_module = ModuleConfig::new(
            MODULE_TYPE,
            serde_json::from_value(json!({
                "on-click": "bar"
            }))
            .expect("module config map should parse"),
        );
        let on_click_cfg = parse_config(&on_click_module).expect("on-click config should parse");
        assert_eq!(on_click_cfg.on_click.as_deref(), Some("bar"));
    }

    #[test]
    fn resolve_clock_formats_uses_explicit_fields() {
        let (template, time_fmt) = resolve_clock_formats(
            Some("<span style=\"italic\">{}</span>".to_string()),
            Some("%H:%M".to_string()),
        );
        assert_eq!(template, "<span style=\"italic\">{}</span>");
        assert_eq!(time_fmt, "%H:%M");

        let (template, time_fmt) = resolve_clock_formats(Some("{}".to_string()), None);
        assert_eq!(template, "{}");
        assert_eq!(time_fmt, DEFAULT_CLOCK_FMT);
    }
}
