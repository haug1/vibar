use std::fs;
use std::path::Path;

use gtk::gdk;

use crate::config::{resolve_style_path, StyleConfig};

pub(crate) fn load_styles(style: &StyleConfig, config_source: Option<&Path>) {
    let Some(display) = gdk::Display::default() else {
        return;
    };

    if style.load_default {
        load_css_from_data(
            &display,
            include_str!("../style.css"),
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }

    if let Some(path) = style.path.as_deref() {
        let resolved = resolve_style_path(path, config_source);
        match fs::read_to_string(&resolved) {
            Ok(content) => {
                // Apply user CSS after default CSS so user selectors can override defaults.
                load_css_from_data(
                    &display,
                    &content,
                    gtk::STYLE_PROVIDER_PRIORITY_APPLICATION + 1,
                );
            }
            Err(err) => {
                eprintln!("Failed to read CSS file {}: {err}", resolved.display());
            }
        }
    }
}

fn load_css_from_data(display: &gdk::Display, css: &str, priority: u32) {
    let provider = gtk::CssProvider::new();
    provider.load_from_data(css);

    gtk::style_context_add_provider_for_display(display, &provider, priority);
}
