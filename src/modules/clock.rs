use chrono::Local;
use glib::ControlFlow;
use gtk::prelude::*;
use gtk::Label;

const DEFAULT_CLOCK_FMT: &str = "%a %d. %b %H:%M:%S";

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
