use std::io::{Read, Write};
use std::os::fd::AsRawFd;

use glib::ControlFlow;
use gtk::prelude::*;
use gtk::{Label, Widget};
use serde::Deserialize;
use serde_json::Value;
use swayipc::{Connection, EventType};

use crate::modules::{
    apply_css_classes, attach_primary_click_command, escape_markup_text, render_markup_template,
    ModuleBuildContext, ModuleConfig, ModuleFactory,
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

fn build_mode_module(
    format: String,
    click_command: Option<String>,
    class: Option<String>,
) -> Label {
    let label = Label::new(None);
    label.add_css_class("module");
    label.add_css_class("sway-mode");
    apply_css_classes(&label, class.as_deref());
    attach_primary_click_command(&label, click_command);

    let (mut signal_rx, signal_tx) = match std::os::unix::net::UnixStream::pair() {
        Ok(pair) => pair,
        Err(err) => {
            eprintln!("vibar/sway-mode: failed to create event signal pipe: {err}");
            refresh_mode(&label, &format);
            return label;
        }
    };
    if let Err(err) = signal_rx.set_nonblocking(true) {
        eprintln!("vibar/sway-mode: failed to set nonblocking event signal pipe: {err}");
        refresh_mode(&label, &format);
        return label;
    }

    start_mode_event_listener(signal_tx);
    refresh_mode(&label, &format);

    glib::source::unix_fd_add_local(
        signal_rx.as_raw_fd(),
        glib::IOCondition::IN | glib::IOCondition::HUP | glib::IOCondition::ERR,
        {
            let label = label.clone();
            let format = format.clone();
            move |_, condition| {
                if condition.intersects(glib::IOCondition::HUP | glib::IOCondition::ERR) {
                    return ControlFlow::Break;
                }

                let mut had_event = false;
                let mut buf = [0_u8; 64];
                loop {
                    match signal_rx.read(&mut buf) {
                        Ok(0) => return ControlFlow::Break,
                        Ok(_) => had_event = true,
                        Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => break,
                        Err(err) => {
                            eprintln!("vibar/sway-mode: failed to read event signal pipe: {err}");
                            return ControlFlow::Break;
                        }
                    }
                }

                if had_event {
                    refresh_mode(&label, &format);
                }
                ControlFlow::Continue
            }
        },
    );

    label
}

fn start_mode_event_listener(mut signal_tx: std::os::unix::net::UnixStream) {
    std::thread::spawn(move || loop {
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
            if signal_tx.write_all(&[1]).is_err() {
                return;
            }
        }

        std::thread::sleep(std::time::Duration::from_millis(200));
    });
}

fn refresh_mode(label: &Label, format: &str) {
    let mut connection = match Connection::new() {
        Ok(conn) => conn,
        Err(_) => {
            label.set_markup(&escape_markup_text("sway?"));
            label.set_visible(true);
            return;
        }
    };

    let mode = match connection.get_binding_state() {
        Ok(mode) => mode,
        Err(_) => {
            label.set_markup(&escape_markup_text("sway?"));
            label.set_visible(true);
            return;
        }
    };

    if mode == "default" || mode.is_empty() {
        label.set_visible(false);
        return;
    }

    label.set_visible(true);
    label.set_markup(&render_markup_template(format, &[("{}", &mode)]));
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
