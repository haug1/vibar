pub(crate) mod backlight;
pub(crate) mod battery;
pub(crate) mod clock;
pub(crate) mod cpu;
pub(crate) mod disk;
pub(crate) mod exec;
pub(crate) mod group;
pub(crate) mod memory;
pub(crate) mod playerctl;
pub(crate) mod pulseaudio;
pub(crate) mod sway;
pub(crate) mod temperature;
pub(crate) mod tray;

use gtk::gdk;
use gtk::prelude::*;
use gtk::{GestureClick, Widget};
use serde::Deserialize;
use serde_json::{Map, Value};

#[derive(Debug, Clone, Default)]
pub(crate) struct ModuleBuildContext {
    pub(crate) monitor_connector: Option<String>,
    pub(crate) monitor: Option<gdk::Monitor>,
}

#[derive(Debug, Deserialize, Clone)]
pub(crate) struct ModuleConfig {
    #[serde(rename = "type")]
    pub(crate) module_type: String,
    #[serde(flatten, default)]
    pub(crate) config: Map<String, Value>,
}

impl ModuleConfig {
    pub(crate) fn new(module_type: impl Into<String>, config: Map<String, Value>) -> Self {
        Self {
            module_type: module_type.into(),
            config,
        }
    }
}

pub(crate) trait ModuleFactory {
    fn module_type(&self) -> &'static str;
    fn init(&self, config: &ModuleConfig, context: &ModuleBuildContext) -> Result<Widget, String>;
}

const FACTORIES: &[&dyn ModuleFactory] = &[
    &backlight::FACTORY,
    &battery::FACTORY,
    &exec::FACTORY,
    &cpu::FACTORY,
    &disk::FACTORY,
    &memory::FACTORY,
    &playerctl::FACTORY,
    &group::FACTORY,
    &pulseaudio::FACTORY,
    &sway::mode::FACTORY,
    &sway::window::FACTORY,
    &sway::workspaces::FACTORY,
    &temperature::FACTORY,
    &clock::FACTORY,
    &tray::FACTORY,
];

pub(crate) fn build_module(
    config: &ModuleConfig,
    context: &ModuleBuildContext,
) -> Result<Widget, String> {
    let factory = FACTORIES
        .iter()
        .find(|factory| factory.module_type() == config.module_type)
        .ok_or_else(|| format!("unknown module type '{}'", config.module_type))?;

    factory.init(config, context)
}

pub(crate) fn attach_primary_click_command(widget: &impl IsA<Widget>, command: Option<String>) {
    if command.is_some() {
        widget.add_css_class("clickable");
    }
    attach_click_command(widget, 1, command);
}

pub(crate) fn attach_secondary_click_command(widget: &impl IsA<Widget>, command: Option<String>) {
    attach_click_command(widget, 3, command);
}

fn attach_click_command(widget: &impl IsA<Widget>, button: u32, command: Option<String>) {
    let Some(command) = command else {
        return;
    };

    let click = GestureClick::builder().button(button).build();
    click.connect_pressed(move |_, _, _, _| {
        let command = command.clone();
        std::thread::spawn(move || {
            let _ = std::process::Command::new("sh")
                .arg("-c")
                .arg(command)
                .spawn();
        });
    });
    widget.add_controller(click);
}

pub(crate) fn apply_css_classes(widget: &impl IsA<Widget>, classes: Option<&str>) {
    let Some(classes) = classes else {
        return;
    };

    for class_name in classes.split_whitespace() {
        widget.add_css_class(class_name);
    }
}

pub(crate) fn escape_markup_text(text: &str) -> String {
    gtk::glib::markup_escape_text(text).to_string()
}

pub(crate) fn render_markup_template(template: &str, replacements: &[(&str, &str)]) -> String {
    let mut rendered = template.to_string();
    for (placeholder, value) in replacements {
        rendered = rendered.replace(placeholder, &escape_markup_text(value));
    }
    rendered
}

#[cfg(test)]
mod tests {
    use serde_json::Map;

    use super::*;

    #[test]
    fn build_module_rejects_unknown_module_type() {
        let module = ModuleConfig::new("does-not-exist", Map::new());
        let err = build_module(&module, &ModuleBuildContext::default())
            .expect_err("unknown module should fail");
        assert!(err.contains("unknown module type 'does-not-exist'"));
    }
}
