pub(crate) mod backlight;
pub(crate) mod battery;
pub(crate) mod broadcaster;
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

use std::time::Duration;

use gtk::gdk;
use gtk::glib::ControlFlow;
use gtk::prelude::*;
use gtk::{GestureClick, Label, Widget};
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

/// Builder that consolidates repeated label setup across modules.
pub(crate) struct ModuleLabel {
    module_class: &'static str,
    user_classes: Option<String>,
    click_command: Option<String>,
}

impl ModuleLabel {
    pub(crate) fn new(module_class: &'static str) -> Self {
        Self {
            module_class,
            user_classes: None,
            click_command: None,
        }
    }

    pub(crate) fn with_css_classes(mut self, classes: Option<&str>) -> Self {
        self.user_classes = classes.map(ToOwned::to_owned);
        self
    }

    pub(crate) fn with_click_command(mut self, command: Option<String>) -> Self {
        self.click_command = command;
        self
    }

    pub(crate) fn into_label(self) -> Label {
        let label = Label::new(None);
        label.add_css_class("module");
        label.add_css_class(self.module_class);
        apply_css_classes(&label, self.user_classes.as_deref());
        attach_primary_click_command(&label, self.click_command);
        label
    }
}

/// Consolidates the common `timeout_add_local(200ms)` + `label_weak.upgrade()`
/// + `try_recv` loop into a single call.
///
/// `apply_fn` receives the label and each update value, and is responsible for
/// setting markup, visibility, CSS classes, etc.
pub(crate) fn poll_receiver<U: 'static>(
    label: &Label,
    receiver: std::sync::mpsc::Receiver<U>,
    apply_fn: impl Fn(&Label, U) + 'static,
) {
    let label_weak = label.downgrade();
    gtk::glib::timeout_add_local(Duration::from_millis(200), move || {
        let Some(label) = label_weak.upgrade() else {
            return ControlFlow::Break;
        };
        while let Ok(update) = receiver.try_recv() {
            apply_fn(&label, update);
        }
        ControlFlow::Continue
    });
}

/// Like [`poll_receiver`] but works with any widget type (e.g. `GtkBox`,
/// `Overlay`) instead of only `Label`.
pub(crate) fn poll_receiver_widget<W, U>(
    widget: &W,
    receiver: std::sync::mpsc::Receiver<U>,
    mut apply_fn: impl FnMut(&W, U) + 'static,
) where
    W: IsA<Widget> + Clone + 'static,
    U: 'static,
{
    let widget_weak = widget.downgrade();
    gtk::glib::timeout_add_local(Duration::from_millis(200), move || {
        let Some(widget) = widget_weak.upgrade() else {
            return ControlFlow::Break;
        };
        while let Ok(update) = receiver.try_recv() {
            apply_fn(&widget, update);
        }
        ControlFlow::Continue
    });
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
