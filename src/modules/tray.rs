use std::thread;
use std::time::Duration;

use glib::ControlFlow;
use gtk::prelude::*;
use gtk::{Box as GtkBox, Button, GestureClick, Image, Orientation, Widget};
use serde::Deserialize;
use serde_json::Value;
use zbus::blocking::{Connection, Proxy};
use zbus::Result as ZbusResult;

use crate::modules::ModuleConfig;

use super::ModuleFactory;

const WATCHER_DESTINATION: &str = "org.kde.StatusNotifierWatcher";
const WATCHER_PATH: &str = "/StatusNotifierWatcher";
const WATCHER_INTERFACE: &str = "org.kde.StatusNotifierWatcher";
const ITEM_INTERFACE: &str = "org.kde.StatusNotifierItem";
const MODULE_TYPE: &str = "tray";
const DEFAULT_ICON_SIZE: i32 = 16;
const MIN_ICON_SIZE: i32 = 8;
const DEFAULT_POLL_INTERVAL_SECS: u32 = 2;
const MIN_POLL_INTERVAL_SECS: u32 = 1;

#[derive(Debug, Deserialize, Clone)]
pub(crate) struct TrayConfig {
    #[serde(default = "default_icon_size")]
    pub(crate) icon_size: i32,
    #[serde(default = "default_poll_interval")]
    pub(crate) poll_interval_secs: u32,
    #[serde(default)]
    pub(crate) class: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TrayItemSnapshot {
    id: String,
    destination: String,
    path: String,
    icon_name: String,
    title: String,
}

pub(crate) struct TrayFactory;

pub(crate) const FACTORY: TrayFactory = TrayFactory;

impl ModuleFactory for TrayFactory {
    fn module_type(&self) -> &'static str {
        MODULE_TYPE
    }

    fn init(&self, config: &ModuleConfig) -> Result<Widget, String> {
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

fn default_icon_size() -> i32 {
    DEFAULT_ICON_SIZE
}

fn default_poll_interval() -> u32 {
    DEFAULT_POLL_INTERVAL_SECS
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
            let snapshot = fetch_tray_snapshot();
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
        button.connect_clicked(move |_| {
            activate_item(destination.clone(), path.clone());
        });

        let destination = item.destination.clone();
        let path = item.path.clone();
        let right_click = GestureClick::builder().button(3).build();
        right_click.connect_pressed(move |_, _, x, y| {
            context_menu_item(destination.clone(), path.clone(), x as i32, y as i32);
        });
        button.add_controller(right_click);

        let destination = item.destination.clone();
        let path = item.path.clone();
        let middle_click = GestureClick::builder().button(2).build();
        middle_click.connect_pressed(move |_, _, x, y| {
            secondary_activate_item(destination.clone(), path.clone(), x as i32, y as i32);
        });
        button.add_controller(middle_click);

        container.append(&button);
    }
}

fn activate_item(destination: String, path: String) {
    call_item_method(destination, path, "Activate", 0, 0);
}

fn context_menu_item(destination: String, path: String, x: i32, y: i32) {
    call_item_method(destination, path, "ContextMenu", x, y);
}

fn secondary_activate_item(destination: String, path: String, x: i32, y: i32) {
    call_item_method(destination, path, "SecondaryActivate", x, y);
}

fn call_item_method(destination: String, path: String, method: &'static str, x: i32, y: i32) {
    thread::spawn(move || {
        let Ok(connection) = Connection::session() else {
            return;
        };

        let Ok(proxy) = Proxy::new(
            &connection,
            destination.as_str(),
            path.as_str(),
            ITEM_INTERFACE,
        ) else {
            return;
        };

        let _result: ZbusResult<()> = proxy.call(method, &(x, y));
    });
}

fn fetch_tray_snapshot() -> Vec<TrayItemSnapshot> {
    let Ok(connection) = Connection::session() else {
        return Vec::new();
    };

    let Ok(watcher) = Proxy::new(
        &connection,
        WATCHER_DESTINATION,
        WATCHER_PATH,
        WATCHER_INTERFACE,
    ) else {
        return Vec::new();
    };

    let host_name = format!("mybar-{}", std::process::id());
    let _register_result: ZbusResult<()> =
        watcher.call("RegisterStatusNotifierHost", &(host_name.as_str(),));

    let Ok(items) = watcher.get_property::<Vec<String>>("RegisteredStatusNotifierItems") else {
        return Vec::new();
    };

    let mut snapshots = items
        .into_iter()
        .filter_map(parse_item_address)
        .filter_map(|(id, destination, path)| fetch_item(&connection, id, destination, path))
        .collect::<Vec<_>>();

    snapshots.sort_by(|a, b| a.id.cmp(&b.id));
    snapshots
}

fn parse_item_address(raw: String) -> Option<(String, String, String)> {
    if raw.starts_with('/') {
        return None;
    }

    let (destination, path_tail) = raw.split_once('/')?;
    if destination.is_empty() || path_tail.is_empty() {
        return None;
    }

    let path = format!("/{path_tail}");
    Some((raw.clone(), destination.to_string(), path))
}

fn fetch_item(
    connection: &Connection,
    id: String,
    destination: String,
    path: String,
) -> Option<TrayItemSnapshot> {
    let (icon_name, title) = {
        let proxy = Proxy::new(
            connection,
            destination.as_str(),
            path.as_str(),
            ITEM_INTERFACE,
        )
        .ok()?;

        let icon_name = proxy
            .get_property::<String>("IconName")
            .ok()
            .filter(|value: &String| !value.is_empty())
            .or_else(|| {
                proxy
                    .get_property::<String>("AttentionIconName")
                    .ok()
                    .filter(|value: &String| !value.is_empty())
            })
            .unwrap_or_else(|| "image-missing".to_string());

        let title = proxy
            .get_property::<String>("Title")
            .ok()
            .filter(|value: &String| !value.is_empty())
            .unwrap_or_else(|| id.clone());

        (icon_name, title)
    };

    Some(TrayItemSnapshot {
        id,
        destination,
        path,
        icon_name,
        title,
    })
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
        let parsed = parse_item_address(":1.42/StatusNotifierItem".to_string())
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
