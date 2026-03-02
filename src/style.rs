use std::cell::RefCell;
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::{Rc, Weak};
use std::time::Duration;

use gtk::gdk;
use gtk::gio;
use gtk::prelude::*;

use crate::config::{resolve_style_path, StyleConfig};

const USER_STYLE_RELOAD_DEBOUNCE_MILLIS: u64 = 150;

pub(crate) struct StyleRuntime {
    display: gdk::Display,
    default_provider: Option<gtk::CssProvider>,
    user_css_path: Option<PathBuf>,
    user_css_provider: RefCell<Option<gtk::CssProvider>>,
    user_css_monitor: RefCell<Option<gio::FileMonitor>>,
    reload_debounce_source: RefCell<Option<gtk::glib::SourceId>>,
}

impl StyleRuntime {
    pub(crate) fn install(style: &StyleConfig, config_source: Option<&Path>) -> Option<Rc<Self>> {
        let display = gdk::Display::default()?;

        let default_provider = if style.load_default {
            let default_provider = gtk::CssProvider::new();
            default_provider.load_from_data(include_str!("../style.css"));
            gtk::style_context_add_provider_for_display(
                &display,
                &default_provider,
                gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
            );
            Some(default_provider)
        } else {
            None
        };

        let user_css_path = style
            .path
            .as_deref()
            .map(|path| resolve_style_path(path, config_source));

        let runtime = Rc::new(Self {
            display,
            default_provider,
            user_css_path,
            user_css_provider: RefCell::new(None),
            user_css_monitor: RefCell::new(None),
            reload_debounce_source: RefCell::new(None),
        });

        runtime.load_user_css_once();
        runtime.install_user_css_watch();

        Some(runtime)
    }

    fn load_user_css_once(&self) {
        let Some(path) = self.user_css_path.as_ref() else {
            return;
        };

        let content = match fs::read_to_string(path) {
            Ok(content) => content,
            Err(err) => {
                eprintln!("Failed to read CSS file {}: {err}", path.display());
                return;
            }
        };

        let provider = gtk::CssProvider::new();
        provider.load_from_data(&content);

        if let Some(previous) = self.user_css_provider.borrow_mut().take() {
            gtk::style_context_remove_provider_for_display(&self.display, &previous);
        }

        gtk::style_context_add_provider_for_display(
            &self.display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION + 1,
        );
        *self.user_css_provider.borrow_mut() = Some(provider);
    }

    fn install_user_css_watch(self: &Rc<Self>) {
        let Some(path) = self.user_css_path.as_ref() else {
            return;
        };

        let file = gio::File::for_path(path);
        let monitor = match file.monitor_file(gio::FileMonitorFlags::NONE, gio::Cancellable::NONE) {
            Ok(monitor) => monitor,
            Err(err) => {
                eprintln!("Failed to watch CSS file {}: {err}", path.display());
                return;
            }
        };

        let weak_runtime = Rc::downgrade(self);
        monitor.connect_changed(move |_, _, _, _| {
            if let Some(runtime) = weak_runtime.upgrade() {
                runtime.schedule_user_css_reload();
            }
        });

        *self.user_css_monitor.borrow_mut() = Some(monitor);
    }

    fn schedule_user_css_reload(self: &Rc<Self>) {
        if self.reload_debounce_source.borrow().is_some() {
            return;
        }

        let weak_runtime: Weak<Self> = Rc::downgrade(self);
        let source_id = gtk::glib::timeout_add_local_once(
            Duration::from_millis(USER_STYLE_RELOAD_DEBOUNCE_MILLIS),
            move || {
                let Some(runtime) = weak_runtime.upgrade() else {
                    return;
                };
                runtime.reload_debounce_source.borrow_mut().take();
                runtime.load_user_css_once();
            },
        );
        *self.reload_debounce_source.borrow_mut() = Some(source_id);
    }
}

impl Drop for StyleRuntime {
    fn drop(&mut self) {
        if let Some(source_id) = self.reload_debounce_source.borrow_mut().take() {
            source_id.remove();
        }

        if let Some(provider) = self.user_css_provider.borrow_mut().take() {
            gtk::style_context_remove_provider_for_display(&self.display, &provider);
        }

        if let Some(provider) = self.default_provider.take() {
            gtk::style_context_remove_provider_for_display(&self.display, &provider);
        }
    }
}
