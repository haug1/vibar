use std::thread;

use zbus::blocking::{Connection, Proxy};
use zbus::Error as ZbusError;
use zbus::Result as ZbusResult;

use super::types::{
    TrayItemSnapshot, ITEM_INTERFACE, WATCHER_DESTINATION, WATCHER_INTERFACE, WATCHER_PATH,
};

pub(super) fn activate_item(destination: String, path: String, x: i32, y: i32) {
    call_item_method(destination, path, "Activate", x, y);
}

pub(super) fn secondary_activate_item(destination: String, path: String, x: i32, y: i32) {
    call_item_method(destination, path, "SecondaryActivate", x, y);
}

pub(super) fn context_menu_item(destination: String, path: String, x: i32, y: i32) {
    call_item_methods_with_fallback(
        destination,
        path,
        vec!["ContextMenu", "SecondaryActivate", "Activate"],
        x,
        y,
    );
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

pub(super) fn fetch_tray_snapshot() -> Vec<TrayItemSnapshot> {
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

pub(super) fn parse_item_address(raw: String) -> Option<(String, String, String)> {
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

fn tray_debug_enabled() -> bool {
    std::env::var("MYBAR_DEBUG_TRAY")
        .map(|v| matches!(v.to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on"))
        .unwrap_or(false)
}

fn is_method_missing_error(err: &ZbusError) -> bool {
    matches!(
        err,
        ZbusError::MethodError(name, _, _)
            if name.as_str() == "org.freedesktop.DBus.Error.UnknownMethod"
                || name.as_str() == "org.freedesktop.DBus.Error.UnknownInterface"
    )
}
