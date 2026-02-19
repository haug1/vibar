use std::process::Command;

use glib::ControlFlow;
use gtk::prelude::*;
use gtk::{Label, Widget};

use crate::modules::ModuleConfig;

use super::ModuleFactory;

const MIN_EXEC_INTERVAL_SECS: u32 = 1;

pub(crate) struct ExecFactory;

pub(crate) const FACTORY: ExecFactory = ExecFactory;

impl ModuleFactory for ExecFactory {
    fn init(&self, config: &ModuleConfig) -> Option<Widget> {
        let ModuleConfig::Exec {
            command,
            interval_secs,
            class,
        } = config
        else {
            return None;
        };

        Some(build_exec_module(command.clone(), *interval_secs, class.clone()).upcast())
    }
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
