use std::thread;
use std::time::Duration;

use glib::ControlFlow;
use gtk::prelude::*;
use gtk::{Box as GtkBox, Button, GestureClick, Image, Orientation, Widget};
use serde_json::Value;

use crate::modules::{ModuleBuildContext, ModuleConfig};

use super::ModuleFactory;

mod menu_dbus;
mod menu_ui;
mod sni;
mod types;

use types::{TrayConfig, TrayItemSnapshot, MIN_ICON_SIZE, MIN_POLL_INTERVAL_SECS, MODULE_TYPE};

pub(crate) struct TrayFactory;

pub(crate) const FACTORY: TrayFactory = TrayFactory;

impl ModuleFactory for TrayFactory {
    fn module_type(&self) -> &'static str {
        MODULE_TYPE
    }

    fn init(&self, config: &ModuleConfig, _context: &ModuleBuildContext) -> Result<Widget, String> {
        let parsed = parse_config(config)?;
        Ok(build_tray_module(parsed).upcast())
    }
}

fn parse_config(module: &ModuleConfig) -> Result<TrayConfig, String> {
    if module.module_type != MODULE_TYPE {
        return Err(format!(
            "expected module type '{}', got '{}'",
            MODULE_TYPE, module.module_type
        ));
    }

    serde_json::from_value(Value::Object(module.config.clone()))
        .map_err(|err| format!("invalid {} module config: {err}", MODULE_TYPE))
}

fn normalized_icon_size(icon_size: i32) -> i32 {
    icon_size.max(MIN_ICON_SIZE)
}

fn normalized_poll_interval_secs(interval: u32) -> u32 {
    interval.max(MIN_POLL_INTERVAL_SECS)
}

fn build_tray_module(config: TrayConfig) -> GtkBox {
    let container = GtkBox::new(Orientation::Horizontal, 4);
    container.add_css_class("module");
    container.add_css_class("tray");

    if let Some(class_name) = config.class {
        container.add_css_class(&class_name);
    }

    let icon_size = normalized_icon_size(config.icon_size);
    let poll_interval_secs = normalized_poll_interval_secs(config.poll_interval_secs);

    let (sender, receiver) = std::sync::mpsc::channel::<Vec<TrayItemSnapshot>>();

    thread::spawn(move || {
        let mut last = Vec::<TrayItemSnapshot>::new();
        loop {
            let snapshot = sni::fetch_tray_snapshot();
            if snapshot != last {
                if sender.send(snapshot.clone()).is_err() {
                    return;
                }
                last = snapshot;
            }
            thread::sleep(Duration::from_secs(u64::from(poll_interval_secs)));
        }
    });

    glib::timeout_add_local(Duration::from_millis(250), {
        let container = container.clone();
        let mut current = Vec::<TrayItemSnapshot>::new();

        move || {
            let mut next = None;
            while let Ok(snapshot) = receiver.try_recv() {
                next = Some(snapshot);
            }

            if let Some(snapshot) = next {
                if snapshot != current {
                    render_tray_items(&container, &snapshot, icon_size);
                    current = snapshot;
                }
            }

            ControlFlow::Continue
        }
    });

    container
}

fn render_tray_items(container: &GtkBox, items: &[TrayItemSnapshot], icon_size: i32) {
    while let Some(child) = container.first_child() {
        container.remove(&child);
    }

    for item in items {
        let button = Button::new();
        button.add_css_class("tray-item");
        button.set_focusable(false);
        button.set_tooltip_text(Some(&item.title));

        let image = Image::from_icon_name(&item.icon_name);
        image.set_pixel_size(icon_size);
        button.set_child(Some(&image));

        let destination = item.destination.clone();
        let path = item.path.clone();
        let click_button = button.clone();
        let click = GestureClick::builder().button(0).build();
        click.connect_pressed(move |gesture, _, x, y| {
            let current_button = gesture.current_button();
            match current_button {
                1 => sni::activate_item(destination.clone(), path.clone(), x as i32, y as i32),
                2 => sni::secondary_activate_item(
                    destination.clone(),
                    path.clone(),
                    x as i32,
                    y as i32,
                ),
                3 => {
                    if !menu_ui::show_item_menu(&click_button, destination.clone(), path.clone()) {
                        sni::context_menu_item(
                            destination.clone(),
                            path.clone(),
                            x as i32,
                            y as i32,
                        );
                    }
                }
                _ => {}
            }
        });
        button.add_controller(click);

        container.append(&button);
    }
}

#[cfg(test)]
mod tests {
    use serde_json::Map;

    use super::*;

    #[test]
    fn parse_config_rejects_wrong_module_type() {
        let module = ModuleConfig::new("clock", Map::new());
        let err = parse_config(&module).expect_err("wrong type should fail");
        assert!(err.contains("expected module type 'tray'"));
    }

    #[test]
    fn parse_item_address_parses_destination_and_path() {
        let parsed = sni::parse_item_address(":1.42/StatusNotifierItem".to_string())
            .expect("valid path should parse");
        assert_eq!(parsed.1, ":1.42");
        assert_eq!(parsed.2, "/StatusNotifierItem");
    }

    #[test]
    fn normalized_values_enforce_minimums() {
        assert_eq!(normalized_icon_size(2), MIN_ICON_SIZE);
        assert_eq!(normalized_poll_interval_secs(0), MIN_POLL_INTERVAL_SECS);
    }
}
