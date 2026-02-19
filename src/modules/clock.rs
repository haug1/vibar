use chrono::Local;
use glib::ControlFlow;
use gtk::prelude::*;
use gtk::{Label, Widget};

use crate::config::ModuleConfig;

use super::ModuleFactory;

const DEFAULT_CLOCK_FMT: &str = "%a %d. %b %H:%M:%S";

pub(crate) struct ClockFactory;

pub(crate) const FACTORY: ClockFactory = ClockFactory;

impl ModuleFactory for ClockFactory {
    fn init(&self, config: &ModuleConfig) -> Option<Widget> {
        let ModuleConfig::Clock { format } = config else {
            return None;
        };

        Some(build_clock_module(format.clone()).upcast())
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
