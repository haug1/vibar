use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::thread;
use std::time::Duration;

use glib::ControlFlow;
use gtk::prelude::*;
use gtk::{
    Box as GtkBox, Button, GestureClick, Image, Label, Orientation, Popover, PositionType,
    Separator, Widget,
};
use serde::Deserialize;
use serde_json::Value;
use zbus::blocking::{Connection, Proxy};
use zbus::zvariant::{OwnedObjectPath, OwnedValue};
use zbus::Error as ZbusError;
use zbus::Result as ZbusResult;

use crate::modules::{ModuleBuildContext, ModuleConfig};

use super::ModuleFactory;

const WATCHER_DESTINATION: &str = "org.kde.StatusNotifierWatcher";
const WATCHER_PATH: &str = "/StatusNotifierWatcher";
const WATCHER_INTERFACE: &str = "org.kde.StatusNotifierWatcher";
const ITEM_INTERFACE: &str = "org.kde.StatusNotifierItem";
const DBUS_MENU_INTERFACE: &str = "com.canonical.dbusmenu";
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

#[derive(Debug, Clone)]
struct TrayMenuEntry {
    id: i32,
    label: String,
    icon_name: Option<String>,
    icon_data: Option<Vec<u8>>,
    enabled: bool,
    visible: bool,
    is_separator: bool,
    submenu_hint: bool,
    children: Vec<TrayMenuEntry>,
}

#[derive(Debug, Clone)]
struct TrayMenuModel {
    menu_path: String,
    entries: Vec<TrayMenuEntry>,
}

type TrayMenuLayout = (i32, HashMap<String, OwnedValue>, Vec<OwnedValue>);

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
        let click_button = button.clone();
        let click = GestureClick::builder().button(0).build();
        click.connect_released(move |gesture, _, x, y| {
            let current_button = gesture.current_button();
            match current_button {
                1 => activate_item(destination.clone(), path.clone()),
                2 => secondary_activate_item(destination.clone(), path.clone(), x as i32, y as i32),
                3 => {
                    if !show_item_menu(&click_button, destination.clone(), path.clone()) {
                        context_menu_item(destination.clone(), path.clone(), x as i32, y as i32);
                    }
                }
                _ => {}
            }
        });
        button.add_controller(click);

        container.append(&button);
    }
}

fn activate_item(destination: String, path: String) {
    call_item_method(destination, path, "Activate", 0, 0);
}

fn context_menu_item(destination: String, path: String, x: i32, y: i32) {
    call_item_methods_with_fallback(
        destination,
        path,
        vec!["ContextMenu", "SecondaryActivate", "Activate"],
        x,
        y,
    );
}

fn show_item_menu(anchor: &Button, destination: String, path: String) -> bool {
    let Some(model) = fetch_dbus_menu_model(&destination, &path) else {
        return false;
    };

    if model.entries.is_empty() {
        return false;
    }

    if !has_visible_menu_entries(&model.entries) {
        return false;
    }

    let popover = Popover::new();
    popover.add_css_class("tray-menu-popover");
    popover.set_has_arrow(true);
    popover.set_autohide(true);
    popover.set_position(PositionType::Top);
    popover.set_parent(anchor);
    let content = GtkBox::new(Orientation::Vertical, 2);
    content.add_css_class("tray-menu-content");
    popover.set_child(Some(&content));

    let levels = Rc::new(RefCell::new(vec![model.entries]));
    render_menu_level(&content, &popover, &destination, &model.menu_path, &levels);
    popover.popup();

    true
}

fn has_visible_menu_entries(entries: &[TrayMenuEntry]) -> bool {
    entries
        .iter()
        .any(|entry| entry.visible && !entry.is_separator)
}

fn secondary_activate_item(destination: String, path: String, x: i32, y: i32) {
    call_item_method(destination, path, "SecondaryActivate", x, y);
}

fn call_item_method(destination: String, path: String, method: &'static str, x: i32, y: i32) {
    call_item_methods_with_fallback(destination, path, vec![method], x, y);
}

fn call_item_methods_with_fallback(
    destination: String,
    path: String,
    methods: Vec<&'static str>,
    x: i32,
    y: i32,
) {
    thread::spawn(move || {
        let Ok(connection) = Connection::session() else {
            if tray_debug_enabled() {
                eprintln!(
                    "mybar/tray: no session bus for {destination}{path} methods={}",
                    methods.join(",")
                );
            }
            return;
        };

        let Ok(proxy) = Proxy::new(
            &connection,
            destination.as_str(),
            path.as_str(),
            ITEM_INTERFACE,
        ) else {
            if tray_debug_enabled() {
                eprintln!(
                    "mybar/tray: failed proxy for {destination}{path} methods={}",
                    methods.join(",")
                );
            }
            return;
        };

        for method in &methods {
            let result: ZbusResult<()> = proxy.call(*method, &(x, y));
            match result {
                Ok(()) => {
                    if tray_debug_enabled() {
                        eprintln!("mybar/tray: method ok {destination}{path} {method}({x}, {y})");
                    }
                    return;
                }
                Err(err) => {
                    if tray_debug_enabled() {
                        eprintln!(
                            "mybar/tray: method error {destination}{path} {method}({x}, {y}): {err}"
                        );
                    }
                    if !is_method_missing_error(&err) {
                        return;
                    }
                }
            }
        }

        if tray_debug_enabled() {
            eprintln!(
                "mybar/tray: no supported click methods for {destination}{path} tried={}",
                methods.join(",")
            );
        }
    });
}

fn fetch_dbus_menu_model(destination: &str, item_path: &str) -> Option<TrayMenuModel> {
    let connection = Connection::session().ok()?;
    let item_proxy = Proxy::new(&connection, destination, item_path, ITEM_INTERFACE).ok()?;

    let menu_path = item_proxy
        .get_property::<OwnedObjectPath>("Menu")
        .ok()
        .map(|path| path.to_string())?;

    if menu_path == "/" {
        return None;
    }

    let entries = {
        let menu_proxy = Proxy::new(
            &connection,
            destination,
            menu_path.as_str(),
            DBUS_MENU_INTERFACE,
        )
        .ok()?;

        let _about_to_show: ZbusResult<bool> = menu_proxy.call("AboutToShow", &(0_i32,));

        let (_revision, root): (u32, TrayMenuLayout) = menu_proxy
            .call("GetLayout", &(0_i32, -1_i32, Vec::<String>::new()))
            .ok()?;

        let initial_entries = root
            .2
            .into_iter()
            .filter_map(parse_menu_entry_node)
            .collect::<Vec<_>>();

        let mut submenu_ids = Vec::new();
        collect_submenu_ids(&initial_entries, &mut submenu_ids);
        for id in submenu_ids {
            let _submenu_about_to_show: ZbusResult<bool> = menu_proxy.call("AboutToShow", &(id,));
        }

        let (_revision, refreshed_root): (u32, TrayMenuLayout) = menu_proxy
            .call("GetLayout", &(0_i32, -1_i32, Vec::<String>::new()))
            .ok()?;

        refreshed_root
            .2
            .into_iter()
            .filter_map(parse_menu_entry_node)
            .collect::<Vec<_>>()
    };

    Some(TrayMenuModel { menu_path, entries })
}

fn parse_menu_entry_node(value: OwnedValue) -> Option<TrayMenuEntry> {
    let (id, props, children): TrayMenuLayout = value.try_into().ok()?;
    let label = read_menu_label(&props);
    let icon_name = read_string_prop(&props, "icon-name").filter(|value| !value.is_empty());
    let icon_data = read_bytes_prop(&props, "icon-data").filter(|value| !value.is_empty());
    let enabled = read_bool_prop(&props, "enabled").unwrap_or(true);
    let visible = read_bool_prop(&props, "visible").unwrap_or(true);
    let is_separator = read_string_prop(&props, "type")
        .is_some_and(|item_type| item_type.eq_ignore_ascii_case("separator"));
    let submenu_hint = read_string_prop(&props, "children-display")
        .is_some_and(|display| display.eq_ignore_ascii_case("submenu"));
    let children = children
        .into_iter()
        .filter_map(parse_menu_entry_node)
        .collect::<Vec<_>>();

    Some(TrayMenuEntry {
        id,
        label,
        icon_name,
        icon_data,
        enabled,
        visible,
        is_separator,
        submenu_hint,
        children,
    })
}

fn collect_submenu_ids(entries: &[TrayMenuEntry], ids: &mut Vec<i32>) {
    for entry in entries {
        if entry.submenu_hint {
            ids.push(entry.id);
        }
        collect_submenu_ids(&entry.children, ids);
    }
}

fn read_menu_label(props: &HashMap<String, OwnedValue>) -> String {
    let raw = read_string_prop(props, "label").unwrap_or_default();
    let without_mnemonic = raw.replace('_', "");
    if without_mnemonic.trim().is_empty() {
        "Menu item".to_string()
    } else {
        without_mnemonic
    }
}

fn read_string_prop(props: &HashMap<String, OwnedValue>, key: &str) -> Option<String> {
    props
        .get(key)
        .and_then(|value| <&str>::try_from(value).ok().map(ToString::to_string))
}

fn read_bool_prop(props: &HashMap<String, OwnedValue>, key: &str) -> Option<bool> {
    props.get(key).and_then(|value| bool::try_from(value).ok())
}

fn read_bytes_prop(props: &HashMap<String, OwnedValue>, key: &str) -> Option<Vec<u8>> {
    props
        .get(key)
        .and_then(|value| value.try_clone().ok())
        .and_then(|value| Vec::<u8>::try_from(value).ok())
}

fn image_from_icon_data(data: &[u8]) -> Option<Image> {
    let loader = gtk::gdk_pixbuf::PixbufLoader::new();
    loader.write(data).ok()?;
    loader.close().ok()?;
    let pixbuf = loader.pixbuf()?;
    let texture = gtk::gdk::Texture::for_pixbuf(&pixbuf);
    let image = Image::from_paintable(Some(&texture));
    image.set_pixel_size(DEFAULT_ICON_SIZE);
    Some(image)
}

fn render_menu_level(
    container: &GtkBox,
    popover: &Popover,
    destination: &str,
    menu_path: &str,
    levels: &Rc<RefCell<Vec<Vec<TrayMenuEntry>>>>,
) {
    while let Some(child) = container.first_child() {
        container.remove(&child);
    }

    let current_level = {
        let borrowed = levels.borrow();
        borrowed.last().cloned().unwrap_or_default()
    };

    if levels.borrow().len() > 1 {
        let back = Button::new();
        back.add_css_class("tray-menu-item");
        let row = GtkBox::new(Orientation::Horizontal, 8);
        let icon = Image::from_icon_name("go-previous-symbolic");
        icon.set_pixel_size(DEFAULT_ICON_SIZE);
        row.append(&icon);
        let label = Label::new(Some("Back"));
        label.set_xalign(0.0);
        label.set_hexpand(true);
        row.append(&label);
        back.set_child(Some(&row));

        let container_clone = container.clone();
        let popover_clone = popover.clone();
        let destination_clone = destination.to_string();
        let menu_path_clone = menu_path.to_string();
        let levels_clone = levels.clone();
        back.connect_clicked(move |_| {
            {
                let mut borrowed = levels_clone.borrow_mut();
                if borrowed.len() > 1 {
                    borrowed.pop();
                }
            }
            render_menu_level(
                &container_clone,
                &popover_clone,
                &destination_clone,
                &menu_path_clone,
                &levels_clone,
            );
        });
        container.append(&back);

        let separator = Separator::new(Orientation::Horizontal);
        container.append(&separator);
    }

    let mut previous_was_separator = true;
    for entry in current_level {
        if !entry.visible {
            continue;
        }

        if entry.is_separator {
            if previous_was_separator {
                continue;
            }
            let separator = Separator::new(Orientation::Horizontal);
            container.append(&separator);
            previous_was_separator = true;
            continue;
        }

        let button = Button::new();
        button.add_css_class("tray-menu-item");
        button.set_sensitive(entry.enabled);

        let row = GtkBox::new(Orientation::Horizontal, 8);
        if let Some(icon_name) = &entry.icon_name {
            let icon = Image::from_icon_name(icon_name);
            icon.set_pixel_size(DEFAULT_ICON_SIZE);
            row.append(&icon);
        } else if let Some(icon_data) = &entry.icon_data {
            if let Some(icon) = image_from_icon_data(icon_data) {
                row.append(&icon);
            }
        }
        let label = Label::new(Some(&entry.label));
        label.set_xalign(0.0);
        label.set_hexpand(true);
        row.append(&label);
        if !entry.children.is_empty() {
            let chevron = Label::new(Some("›"));
            row.append(&chevron);
        }
        button.set_child(Some(&row));

        if !entry.children.is_empty() {
            let children = entry.children.clone();
            let container_clone = container.clone();
            let popover_clone = popover.clone();
            let destination_clone = destination.to_string();
            let menu_path_clone = menu_path.to_string();
            let levels_clone = levels.clone();
            button.connect_clicked(move |_| {
                levels_clone.borrow_mut().push(children.clone());
                render_menu_level(
                    &container_clone,
                    &popover_clone,
                    &destination_clone,
                    &menu_path_clone,
                    &levels_clone,
                );
            });
        } else {
            let destination_clone = destination.to_string();
            let menu_path_clone = menu_path.to_string();
            let popover_clone = popover.clone();
            let id = entry.id;
            button.connect_clicked(move |_| {
                send_menu_event(destination_clone.clone(), menu_path_clone.clone(), id);
                popover_clone.popdown();
            });
        }

        container.append(&button);
        previous_was_separator = false;
    }
}

fn send_menu_event(destination: String, menu_path: String, item_id: i32) {
    thread::spawn(move || {
        let Ok(connection) = Connection::session() else {
            return;
        };
        let Ok(menu_proxy) = Proxy::new(
            &connection,
            destination.as_str(),
            menu_path.as_str(),
            DBUS_MENU_INTERFACE,
        ) else {
            return;
        };

        let _event_result: ZbusResult<()> = menu_proxy.call(
            "Event",
            &(
                item_id,
                "clicked",
                OwnedValue::from(0_i32),
                0_u32, // event timestamp is optional for many providers
            ),
        );
    });
}

fn is_method_missing_error(err: &ZbusError) -> bool {
    matches!(
        err,
        ZbusError::MethodError(name, _, _)
            if name.as_str() == "org.freedesktop.DBus.Error.UnknownMethod"
                || name.as_str() == "org.freedesktop.DBus.Error.UnknownInterface"
    )
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

fn tray_debug_enabled() -> bool {
    std::env::var("MYBAR_DEBUG_TRAY")
        .map(|v| matches!(v.to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on"))
        .unwrap_or(false)
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
