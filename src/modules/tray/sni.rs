use std::thread;

use zbus::blocking::{Connection, Proxy};
use zbus::Error as ZbusError;
use zbus::Result as ZbusResult;

use super::types::{
    TrayIconPixmap, TrayItemSnapshot, ITEM_INTERFACE, WATCHER_DESTINATION, WATCHER_INTERFACE,
    WATCHER_PATH,
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
    let (icon_name, icon_pixmap, icon_theme_path, title) = {
        let proxy = Proxy::new(
            connection,
            destination.as_str(),
            path.as_str(),
            ITEM_INTERFACE,
        )
        .ok()?;

        let icon_name_value = proxy
            .get_property::<String>("IconName")
            .ok()
            .unwrap_or_default();
        let attention_icon_name_value = proxy
            .get_property::<String>("AttentionIconName")
            .ok()
            .unwrap_or_default();
        let icon_name = if !icon_name_value.is_empty() {
            icon_name_value.clone()
        } else {
            attention_icon_name_value.clone()
        };

        let icon_pixmap = proxy
            .get_property::<Vec<(i32, i32, Vec<u8>)>>("IconPixmap")
            .ok()
            .and_then(select_icon_pixmap)
            .or_else(|| {
                proxy
                    .get_property::<Vec<(i32, i32, Vec<u8>)>>("AttentionIconPixmap")
                    .ok()
                    .and_then(select_icon_pixmap)
            });

        let title = proxy
            .get_property::<String>("Title")
            .ok()
            .filter(|value: &String| !value.is_empty())
            .unwrap_or_else(|| id.clone());

        let icon_theme_path = proxy
            .get_property::<String>("IconThemePath")
            .ok()
            .filter(|value: &String| !value.is_empty());

        (icon_name, icon_pixmap, icon_theme_path, title)
    };

    Some(TrayItemSnapshot {
        id,
        destination,
        path,
        icon_name,
        icon_pixmap,
        icon_theme_path,
        title,
    })
}

fn select_icon_pixmap(entries: Vec<(i32, i32, Vec<u8>)>) -> Option<TrayIconPixmap> {
    entries
        .into_iter()
        .filter_map(|(width, height, argb_data)| {
            if width <= 0 || height <= 0 {
                return None;
            }
            let expected_len = usize::try_from(width).ok()? * usize::try_from(height).ok()? * 4;
            if argb_data.len() < expected_len {
                return None;
            }

            Some(TrayIconPixmap {
                width,
                height,
                argb_data,
            })
        })
        .max_by_key(|pixmap| pixmap.width * pixmap.height)
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

#[cfg(test)]
mod tests {
    use super::select_icon_pixmap;

    #[test]
    fn select_icon_pixmap_picks_largest_valid_entry() {
        let picked = select_icon_pixmap(vec![
            (16, 16, vec![0; 16 * 16 * 4]),
            (24, 24, vec![0; 24 * 24 * 4]),
            (32, 32, vec![0; 16]),
        ])
        .expect("a valid pixmap should be selected");

        assert_eq!(picked.width, 24);
        assert_eq!(picked.height, 24);
    }
}
