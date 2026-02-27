use std::sync::{Arc, OnceLock};

use gtk::prelude::*;
use gtk::{Label, Widget};
use serde::Deserialize;
use serde_json::Value;
use swayipc::{Connection, EventType};

use crate::modules::broadcaster::{BackendRegistry, Broadcaster};
use crate::modules::{
    escape_markup_text, poll_receiver, render_markup_template, ModuleBuildContext, ModuleConfig,
    ModuleFactory, ModuleLabel,
};

#[derive(Debug, Deserialize, Clone, Default)]
pub(crate) struct ModeConfig {
    #[serde(default = "default_format")]
    pub(crate) format: String,
    #[serde(default)]
    pub(crate) click: Option<String>,
    #[serde(rename = "on-click", default)]
    pub(crate) on_click: Option<String>,
    #[serde(default)]
    pub(crate) class: Option<String>,
}

#[derive(Debug, Clone)]
struct ModeUpdate {
    text: String,
    visible: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct ModeSharedKey {
    format: String,
}

pub(crate) struct SwayModeFactory;

pub(crate) const FACTORY: SwayModeFactory = SwayModeFactory;
pub(crate) const MODULE_TYPE: &str = "sway/mode";

impl ModuleFactory for SwayModeFactory {
    fn module_type(&self) -> &'static str {
        MODULE_TYPE
    }

    fn init(&self, config: &ModuleConfig, _context: &ModuleBuildContext) -> Result<Widget, String> {
        let parsed = parse_config(config)?;
        let click_command = parsed.click.or(parsed.on_click);
        Ok(build_mode_module(parsed.format, click_command, parsed.class).upcast())
    }
}

fn default_format() -> String {
    "{}".to_string()
}

fn parse_config(module: &ModuleConfig) -> Result<ModeConfig, String> {
    if module.module_type != MODULE_TYPE {
        return Err(format!(
            "expected module type '{}', got '{}'",
            MODULE_TYPE, module.module_type
        ));
    }

    serde_json::from_value(Value::Object(module.config.clone()))
        .map_err(|err| format!("invalid {} module config: {err}", MODULE_TYPE))
}

fn mode_registry() -> &'static BackendRegistry<ModeSharedKey, Broadcaster<ModeUpdate>> {
    static REGISTRY: OnceLock<BackendRegistry<ModeSharedKey, Broadcaster<ModeUpdate>>> =
        OnceLock::new();
    REGISTRY.get_or_init(BackendRegistry::new)
}

fn subscribe_shared_mode(format: String) -> std::sync::mpsc::Receiver<ModeUpdate> {
    let key = ModeSharedKey {
        format: format.clone(),
    };

    let (broadcaster, start_worker) = mode_registry().get_or_create(key.clone(), Broadcaster::new);
    let receiver = broadcaster.subscribe();

    if start_worker {
        start_mode_worker(key, broadcaster);
    }

    receiver
}

fn start_mode_worker(key: ModeSharedKey, broadcaster: Arc<Broadcaster<ModeUpdate>>) {
    std::thread::spawn(move || {
        // Send initial mode state
        broadcaster.broadcast(query_current_mode(&key.format));

        loop {
            if broadcaster.subscriber_count() == 0 {
                mode_registry().remove(&key, &broadcaster);
                return;
            }

            let connection = match Connection::new() {
                Ok(conn) => conn,
                Err(_) => {
                    std::thread::sleep(std::time::Duration::from_millis(500));
                    continue;
                }
            };

            let stream = match connection.subscribe([EventType::Mode]) {
                Ok(stream) => stream,
                Err(_) => {
                    std::thread::sleep(std::time::Duration::from_millis(500));
                    continue;
                }
            };

            for _ in stream {
                if broadcaster.subscriber_count() == 0 {
                    mode_registry().remove(&key, &broadcaster);
                    return;
                }
                broadcaster.broadcast(query_current_mode(&key.format));
            }

            std::thread::sleep(std::time::Duration::from_millis(200));
        }
    });
}

fn query_current_mode(format: &str) -> ModeUpdate {
    let mut connection = match Connection::new() {
        Ok(conn) => conn,
        Err(_) => {
            return ModeUpdate {
                text: escape_markup_text("sway?"),
                visible: true,
            };
        }
    };

    let mode = match connection.get_binding_state() {
        Ok(mode) => mode,
        Err(_) => {
            return ModeUpdate {
                text: escape_markup_text("sway?"),
                visible: true,
            };
        }
    };

    if mode == "default" || mode.is_empty() {
        return ModeUpdate {
            text: String::new(),
            visible: false,
        };
    }

    let rendered = render_markup_template(format, &[("{}", &mode)]);
    ModeUpdate {
        visible: !rendered.trim().is_empty(),
        text: rendered,
    }
}

fn build_mode_module(
    format: String,
    click_command: Option<String>,
    class: Option<String>,
) -> Label {
    let label = ModuleLabel::new("sway-mode")
        .with_css_classes(class.as_deref())
        .with_click_command(click_command)
        .into_label();

    let receiver = subscribe_shared_mode(format);

    poll_receiver(&label, receiver, |label, update| {
        label.set_visible(update.visible);
        if update.visible {
            label.set_markup(&update.text);
        }
    });

    label
}

#[cfg(test)]
mod tests {
    use serde_json::Map;

    use super::*;

    #[test]
    fn parse_config_rejects_wrong_module_type() {
        let module = ModuleConfig::new("clock", Map::new());
        let err = parse_config(&module).expect_err("wrong type should fail");
        assert!(err.contains("expected module type 'sway/mode'"));
    }

    #[test]
    fn parse_config_supports_click_aliases() {
        let click_module = ModuleConfig::new(
            MODULE_TYPE,
            serde_json::from_str("{\"click\":\"echo click\"}")
                .expect("module config map should parse"),
        );
        let click_cfg = parse_config(&click_module).expect("click config should parse");
        assert_eq!(click_cfg.click.as_deref(), Some("echo click"));
        assert!(click_cfg.on_click.is_none());

        let on_click_module = ModuleConfig::new(
            MODULE_TYPE,
            serde_json::from_str("{\"on-click\":\"echo alias\"}")
                .expect("module config map should parse"),
        );
        let on_click_cfg = parse_config(&on_click_module).expect("on-click config should parse");
        assert!(on_click_cfg.click.is_none());
        assert_eq!(on_click_cfg.on_click.as_deref(), Some("echo alias"));
    }
}
