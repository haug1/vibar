use chrono::Local;
use glib::ControlFlow;
use gtk::prelude::*;
use gtk::{Label, Widget};
use serde::Deserialize;

use crate::modules::ModuleConfig;

use super::ModuleFactory;

const DEFAULT_CLOCK_FMT: &str = "%a %d. %b %H:%M:%S";

#[derive(Debug, Deserialize, Clone)]
pub(crate) struct ClockConfig {
    #[serde(default)]
    pub(crate) format: Option<String>,
}

pub(crate) struct ClockFactory;

pub(crate) const FACTORY: ClockFactory = ClockFactory;

impl ModuleFactory for ClockFactory {
    fn init(&self, config: &ModuleConfig) -> Option<Widget> {
        let ModuleConfig::Clock { config } = config else {
            return None;
        };

        Some(build_clock_module(config.format.clone()).upcast())
    }
}

pub(crate) fn build_clock_module(format: Option<String>) -> Label {
    let label = Label::new(None);
    label.add_css_class("module");
    label.add_css_class("clock");

    let fmt = format.unwrap_or_else(|| DEFAULT_CLOCK_FMT.to_string());

    let update = {
        let label = label.clone();
        let fmt = fmt.clone();
        move || {
            let now = Local::now();
            label.set_text(&now.format(&fmt).to_string());
        }
    };

    update();

    glib::timeout_add_seconds_local(1, move || {
        update();
        ControlFlow::Continue
    });

    label
}
