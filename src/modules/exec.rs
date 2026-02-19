use std::process::Command;

use glib::ControlFlow;
use gtk::prelude::*;
use gtk::{Label, Widget};
use serde::Deserialize;
use serde_json::Value;

use crate::modules::ModuleConfig;

use super::ModuleFactory;

const MIN_EXEC_INTERVAL_SECS: u32 = 1;
pub(crate) const MODULE_TYPE: &str = "exec";

#[derive(Debug, Deserialize, Clone)]
pub(crate) struct ExecConfig {
    pub(crate) command: String,
    #[serde(default = "default_exec_interval")]
    pub(crate) interval_secs: u32,
    #[serde(default)]
    pub(crate) class: Option<String>,
}

fn default_exec_interval() -> u32 {
    5
}

pub(crate) struct ExecFactory;

pub(crate) const FACTORY: ExecFactory = ExecFactory;

impl ModuleFactory for ExecFactory {
    fn module_type(&self) -> &'static str {
        MODULE_TYPE
    }

    fn init(&self, config: &ModuleConfig) -> Result<Widget, String> {
        let parsed = parse_config(config)?;
        Ok(build_exec_module(parsed.command, parsed.interval_secs, parsed.class).upcast())
    }
}

pub(crate) fn parse_config(module: &ModuleConfig) -> Result<ExecConfig, String> {
    if module.module_type != MODULE_TYPE {
        return Err(format!(
            "expected module type '{}', got '{}'",
            MODULE_TYPE, module.module_type
        ));
    }

    serde_json::from_value(Value::Object(module.config.clone()))
        .map_err(|err| format!("invalid {} module config: {err}", MODULE_TYPE))
}

pub(crate) fn build_exec_module(
    command: String,
    interval_secs: u32,
    class: Option<String>,
) -> Label {
    let label = Label::new(None);
    label.add_css_class("module");
    label.add_css_class("exec");
    let effective_interval_secs = normalized_exec_interval(interval_secs);

    if effective_interval_secs != interval_secs {
        eprintln!(
            "exec interval_secs={} is too low; clamping to {} second",
            interval_secs, effective_interval_secs
        );
    }

    if let Some(class_name) = class {
        label.add_css_class(&class_name);
    }

    let (sender, receiver) = std::sync::mpsc::channel::<String>();

    glib::timeout_add_local(std::time::Duration::from_millis(200), {
        let label = label.clone();
        move || {
            while let Ok(text) = receiver.try_recv() {
                label.set_text(&text);
            }
            ControlFlow::Continue
        }
    });

    trigger_exec_command(command.clone(), sender.clone());

    glib::timeout_add_seconds_local(effective_interval_secs, move || {
        trigger_exec_command(command.clone(), sender.clone());
        ControlFlow::Continue
    });

    label
}

pub(crate) fn normalized_exec_interval(interval_secs: u32) -> u32 {
    interval_secs.max(MIN_EXEC_INTERVAL_SECS)
}

fn trigger_exec_command(command: String, sender: std::sync::mpsc::Sender<String>) {
    std::thread::spawn(move || {
        let text = match Command::new("sh").arg("-c").arg(&command).output() {
            Ok(output) => {
                let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
                let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();

                if !stdout.is_empty() {
                    stdout
                } else if !stderr.is_empty() {
                    stderr
                } else {
                    String::new()
                }
            }
            Err(err) => format!("exec error: {err}"),
        };

        let _ = sender.send(text);
    });
}
