use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::Duration;

use zbus::blocking::connection::Builder as ConnectionBuilder;
use zbus::blocking::fdo::DBusProxy;
use zbus::blocking::{Connection, MessageIterator, Proxy};
use zbus::message::Header;
use zbus::message::Type as MessageType;
use zbus::zvariant::OwnedValue;
use zbus::Error as ZbusError;
use zbus::MatchRule;
use zbus::Result as ZbusResult;

use super::types::{
    TrayIconPixmap, TrayItemSnapshot, ITEM_INTERFACE, WATCHER_DESTINATION, WATCHER_INTERFACE,
    WATCHER_PATH,
};

#[derive(Debug, Default)]
struct WatcherState {
    registered_items: Vec<String>,
    host_registered: bool,
}

#[derive(Clone)]
struct LocalStatusNotifierWatcher {
    state: Arc<Mutex<WatcherState>>,
}

#[zbus::interface(name = "org.kde.StatusNotifierWatcher")]
impl LocalStatusNotifierWatcher {
    fn register_status_notifier_item(&self, service: &str, #[zbus(header)] header: Header<'_>) {
        let sender = header.sender().map(|value| value.to_string());
        let Some(item_id) = normalize_registered_item_id(service, sender.as_deref()) else {
            if tray_debug_enabled() {
                eprintln!(
                    "vibar/tray: rejected RegisterStatusNotifierItem service={service:?} sender={sender:?}"
                );
            }
            return;
        };

        let Ok(mut guard) = self.state.lock() else {
            return;
        };
        if !guard.registered_items.iter().any(|item| item == &item_id) {
            if tray_debug_enabled() {
                eprintln!(
                    "vibar/tray: registered item via local watcher: {item_id} (service={service:?} sender={sender:?})"
                );
            }
            guard.registered_items.push(item_id);
            if tray_debug_enabled() {
                eprintln!(
                    "vibar/tray: local watcher item count={}",
                    guard.registered_items.len()
                );
            }
        }
    }

    fn register_status_notifier_host(&self, service: &str) {
        let Ok(mut guard) = self.state.lock() else {
            return;
        };
        if !guard.host_registered {
            guard.host_registered = true;
            if tray_debug_enabled() {
                eprintln!("vibar/tray: local watcher host registration: service={service:?}");
            }
        }
    }

    #[zbus(property)]
    fn registered_status_notifier_items(&self) -> Vec<String> {
        self.state
            .lock()
            .map(|guard| guard.registered_items.clone())
            .unwrap_or_default()
    }

    #[zbus(property)]
    fn is_status_notifier_host_registered(&self) -> bool {
        true
    }

    #[zbus(property)]
    fn protocol_version(&self) -> i32 {
        0
    }
}

static LOCAL_WATCHER_INIT: OnceLock<()> = OnceLock::new();
const DBUS_PROPERTIES_INTERFACE: &str = "org.freedesktop.DBus.Properties";
const WATCHER_ITEM_REGISTERED_SIGNAL: &str = "StatusNotifierItemRegistered";
const WATCHER_ITEM_UNREGISTERED_SIGNAL: &str = "StatusNotifierItemUnregistered";
const PROPERTIES_CHANGED_SIGNAL: &str = "PropertiesChanged";

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
                    "vibar/tray: no session bus for {destination}{path} methods={}",
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
                    "vibar/tray: failed proxy for {destination}{path} methods={}",
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
                        eprintln!("vibar/tray: method ok {destination}{path} {method}({x}, {y})");
                    }
                    return;
                }
                Err(err) => {
                    if tray_debug_enabled() {
                        eprintln!(
                            "vibar/tray: method error {destination}{path} {method}({x}, {y}): {err}"
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
                "vibar/tray: no supported click methods for {destination}{path} tried={}",
                methods.join(",")
            );
        }
    });
}

pub(super) fn start_refresh_listeners(trigger_tx: std::sync::mpsc::Sender<()>) {
    start_name_owner_listener(trigger_tx.clone());
    start_watcher_item_listener(trigger_tx.clone(), WATCHER_ITEM_REGISTERED_SIGNAL);
    start_watcher_item_listener(trigger_tx.clone(), WATCHER_ITEM_UNREGISTERED_SIGNAL);
    start_item_properties_listener(trigger_tx);
}

pub(super) fn open_session_connection() -> Option<Connection> {
    ensure_local_watcher_fallback();
    match Connection::session() {
        Ok(connection) => Some(connection),
        Err(err) => {
            if tray_debug_enabled() {
                eprintln!("vibar/tray: no session bus while initializing tray backend: {err}");
            }
            None
        }
    }
}

fn start_name_owner_listener(trigger_tx: std::sync::mpsc::Sender<()>) {
    thread::spawn(move || {
        let Ok(connection) = Connection::session() else {
            if tray_debug_enabled() {
                eprintln!("vibar/tray: failed to open session bus for NameOwnerChanged listener");
            }
            return;
        };
        let Ok(proxy) = DBusProxy::new(&connection) else {
            if tray_debug_enabled() {
                eprintln!("vibar/tray: failed to create DBus proxy for NameOwnerChanged listener");
            }
            return;
        };
        let Ok(mut signals) = proxy.receive_name_owner_changed() else {
            if tray_debug_enabled() {
                eprintln!("vibar/tray: failed to subscribe to NameOwnerChanged");
            }
            return;
        };

        for signal in &mut signals {
            let Ok(args) = signal.args() else {
                continue;
            };
            let name = args.name().to_string();
            // Refresh only for tray-related names to avoid turning generic DBus churn
            // into continuous tray snapshot rebuilds.
            if is_tray_relevant_name(&name) && trigger_tx.send(()).is_err() {
                return;
            }
        }
    });
}

fn is_tray_relevant_name(name: &str) -> bool {
    name.contains("StatusNotifier") || name.contains("ayatana")
}

fn start_watcher_item_listener(trigger_tx: std::sync::mpsc::Sender<()>, member: &'static str) {
    thread::spawn(move || {
        let Ok(connection) = Connection::session() else {
            if tray_debug_enabled() {
                eprintln!("vibar/tray: failed to open session bus for watcher signal listener");
            }
            return;
        };

        let rule = match MatchRule::builder()
            .msg_type(MessageType::Signal)
            .interface(WATCHER_INTERFACE)
            .and_then(|builder| builder.member(member))
            .and_then(|builder| builder.path(WATCHER_PATH))
            .map(|builder| builder.build())
        {
            Ok(rule) => rule,
            Err(err) => {
                if tray_debug_enabled() {
                    eprintln!(
                        "vibar/tray: failed to build watcher signal match rule ({member}): {err}"
                    );
                }
                return;
            }
        };

        let Ok(iterator) = MessageIterator::for_match_rule(rule, &connection, Some(256)) else {
            if tray_debug_enabled() {
                eprintln!("vibar/tray: failed to subscribe to watcher signal ({member})");
            }
            return;
        };

        for message in iterator {
            if message.is_ok() && trigger_tx.send(()).is_err() {
                return;
            }
        }
    });
}

fn start_item_properties_listener(trigger_tx: std::sync::mpsc::Sender<()>) {
    thread::spawn(move || {
        let Ok(connection) = Connection::session() else {
            if tray_debug_enabled() {
                eprintln!("vibar/tray: failed to open session bus for item property listener");
            }
            return;
        };

        let rule = match MatchRule::builder()
            .msg_type(MessageType::Signal)
            .interface(DBUS_PROPERTIES_INTERFACE)
            .and_then(|builder| builder.member(PROPERTIES_CHANGED_SIGNAL))
            .map(|builder| builder.build())
        {
            Ok(rule) => rule,
            Err(err) => {
                if tray_debug_enabled() {
                    eprintln!("vibar/tray: failed to build properties signal match rule: {err}");
                }
                return;
            }
        };

        let Ok(iterator) = MessageIterator::for_match_rule(rule, &connection, Some(512)) else {
            if tray_debug_enabled() {
                eprintln!("vibar/tray: failed to subscribe to properties signal");
            }
            return;
        };

        for message in iterator {
            let Ok(message) = message else {
                continue;
            };
            if is_tray_item_properties_changed(&message) && trigger_tx.send(()).is_err() {
                return;
            }
        }
    });
}

fn is_tray_item_properties_changed(message: &zbus::Message) -> bool {
    let Ok((interface_name, changed, invalidated)) =
        message
            .body()
            .deserialize::<(String, HashMap<String, OwnedValue>, Vec<String>)>()
    else {
        return false;
    };

    interface_name == ITEM_INTERFACE && (!changed.is_empty() || !invalidated.is_empty())
}

pub(super) fn fetch_tray_snapshot_with_connection(
    connection: &Connection,
    host_registered: &mut bool,
) -> Vec<TrayItemSnapshot> {
    ensure_local_watcher_fallback();

    let Ok(watcher) = Proxy::new(
        connection,
        WATCHER_DESTINATION,
        WATCHER_PATH,
        WATCHER_INTERFACE,
    ) else {
        if tray_debug_enabled() {
            eprintln!(
                "vibar/tray: failed to create watcher proxy {WATCHER_DESTINATION}{WATCHER_PATH}"
            );
        }
        *host_registered = false;
        return Vec::new();
    };

    if !*host_registered {
        // Empty host name tells watcher to use sender bus name for registration.
        let host_name = "";
        let register_result: ZbusResult<()> =
            watcher.call("RegisterStatusNotifierHost", &(host_name,));
        match register_result {
            Ok(()) => {
                *host_registered = true;
            }
            Err(err) => {
                if tray_debug_enabled() {
                    eprintln!("vibar/tray: RegisterStatusNotifierHost failed: {err}");
                }
                return Vec::new();
            }
        }
    }

    let Ok(items) = watcher.get_property::<Vec<String>>("RegisteredStatusNotifierItems") else {
        if tray_debug_enabled() {
            eprintln!("vibar/tray: failed to read RegisteredStatusNotifierItems");
        }
        return Vec::new();
    };
    if tray_debug_enabled() {
        eprintln!(
            "vibar/tray: watcher returned {} registered item(s): {:?}",
            items.len(),
            items
        );
    }

    let mut snapshots = items
        .into_iter()
        .filter_map(|raw| {
            let parsed = parse_item_address(raw.clone());
            if parsed.is_none() && tray_debug_enabled() {
                eprintln!("vibar/tray: invalid tray item address from watcher: {raw:?}");
            }
            parsed
        })
        .filter_map(|(id, destination, path)| fetch_item(connection, id, destination, path))
        .collect::<Vec<_>>();

    snapshots.sort_by(|a, b| a.id.cmp(&b.id));
    if tray_debug_enabled() {
        eprintln!(
            "vibar/tray: resolved {} tray snapshot item(s)",
            snapshots.len()
        );
    }
    snapshots
}

pub(super) fn parse_item_address(raw: String) -> Option<(String, String, String)> {
    if raw.is_empty() {
        return None;
    }

    // Some watchers publish just the bus name; default object path is /StatusNotifierItem.
    if !raw.contains('/') {
        return Some((raw.clone(), raw, "/StatusNotifierItem".to_string()));
    }

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
        let proxy = match Proxy::new(
            connection,
            destination.as_str(),
            path.as_str(),
            ITEM_INTERFACE,
        ) {
            Ok(proxy) => proxy,
            Err(err) => {
                if tray_debug_enabled() {
                    eprintln!(
                        "vibar/tray: failed item proxy for {destination}{path} ({id}): {err}"
                    );
                }
                return None;
            }
        };

        let status = proxy
            .get_property::<String>("Status")
            .ok()
            .unwrap_or_else(|| "Active".to_string());
        if status.eq_ignore_ascii_case("passive") {
            if tray_debug_enabled() {
                eprintln!("vibar/tray: skipping passive tray item {destination}{path} ({id})");
            }
            return None;
        }

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
    std::env::var("VIBAR_DEBUG_TRAY")
        .map(|v| matches!(v.to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on"))
        .unwrap_or(false)
}

fn ensure_local_watcher_fallback() {
    let _ = LOCAL_WATCHER_INIT.get_or_init(|| {
        let state = Arc::new(Mutex::new(WatcherState::default()));
        spawn_owner_cleanup_listener(state.clone());

        thread::spawn(move || {
            let watcher = LocalStatusNotifierWatcher { state };

            let connection = match ConnectionBuilder::session()
                .and_then(|builder| builder.name(WATCHER_DESTINATION))
                .and_then(|builder| builder.serve_at(WATCHER_PATH, watcher))
                .and_then(|builder| builder.build())
            {
                Ok(connection) => {
                    if tray_debug_enabled() {
                        eprintln!("vibar/tray: started local StatusNotifierWatcher fallback");
                    }
                    connection
                }
                Err(err) => {
                    if tray_debug_enabled() {
                        eprintln!("vibar/tray: local watcher fallback unavailable: {err}");
                    }
                    return;
                }
            };

            let _keep_connection_alive = connection;
            loop {
                thread::sleep(Duration::from_secs(3600));
            }
        });
    });
}

fn spawn_owner_cleanup_listener(state: Arc<Mutex<WatcherState>>) {
    thread::spawn(move || {
        let Ok(connection) = Connection::session() else {
            if tray_debug_enabled() {
                eprintln!("vibar/tray: local watcher cleanup listener failed to open session bus");
            }
            return;
        };
        let Ok(proxy) = DBusProxy::new(&connection) else {
            if tray_debug_enabled() {
                eprintln!("vibar/tray: local watcher cleanup listener failed to create DBusProxy");
            }
            return;
        };
        let Ok(mut signals) = proxy.receive_name_owner_changed() else {
            if tray_debug_enabled() {
                eprintln!("vibar/tray: local watcher cleanup listener failed to subscribe NameOwnerChanged");
            }
            return;
        };

        for signal in &mut signals {
            let Ok(args) = signal.args() else {
                continue;
            };
            let name = args.name().to_string();
            if args.new_owner().is_some() {
                continue;
            }

            let Ok(mut guard) = state.lock() else {
                continue;
            };
            let removed = remove_registered_items_for_name(&mut guard.registered_items, &name);
            if removed > 0 && tray_debug_enabled() {
                eprintln!(
                    "vibar/tray: local watcher pruned {removed} item(s) after owner vanished: {name}"
                );
            }
        }
    });
}

fn remove_registered_items_for_name(items: &mut Vec<String>, name: &str) -> usize {
    let before = items.len();
    items.retain(|item| !is_item_owned_by_name(item, name));
    before.saturating_sub(items.len())
}

fn is_item_owned_by_name(item: &str, name: &str) -> bool {
    item == name
        || item
            .strip_prefix(name)
            .is_some_and(|suffix| suffix.starts_with('/'))
}

fn normalize_registered_item_id(service: &str, sender: Option<&str>) -> Option<String> {
    if service.is_empty() {
        return None;
    }

    if service.starts_with('/') {
        let destination = sender?;
        return Some(format!("{destination}{service}"));
    }

    if service.contains('/') {
        return Some(service.to_string());
    }

    Some(format!("{service}/StatusNotifierItem"))
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
    use super::{
        is_item_owned_by_name, is_tray_relevant_name, normalize_registered_item_id,
        parse_item_address, remove_registered_items_for_name, select_icon_pixmap,
    };

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

    #[test]
    fn parse_item_address_accepts_service_name_without_path() {
        let parsed = parse_item_address("org.kde.StatusNotifierItem-123-1".to_string())
            .expect("service-only address should parse");
        assert_eq!(parsed.0, "org.kde.StatusNotifierItem-123-1");
        assert_eq!(parsed.1, "org.kde.StatusNotifierItem-123-1");
        assert_eq!(parsed.2, "/StatusNotifierItem");
    }

    #[test]
    fn parse_item_address_rejects_empty_value() {
        assert!(parse_item_address(String::new()).is_none());
    }

    #[test]
    fn normalize_registered_item_id_uses_sender_for_path_only() {
        let id = normalize_registered_item_id("/StatusNotifierItem", Some(":1.42"))
            .expect("path-only service should normalize");
        assert_eq!(id, ":1.42/StatusNotifierItem");
    }

    #[test]
    fn normalize_registered_item_id_defaults_service_only_path() {
        let id = normalize_registered_item_id("org.example.Tray", None)
            .expect("service-only entry should normalize");
        assert_eq!(id, "org.example.Tray/StatusNotifierItem");
    }

    #[test]
    fn remove_registered_items_for_name_drops_matching_owner_entries() {
        let mut items = vec![
            ":1.42/StatusNotifierItem".to_string(),
            ":1.43/org/example/Item".to_string(),
            "org.example.Service/StatusNotifierItem".to_string(),
        ];
        let removed = remove_registered_items_for_name(&mut items, ":1.42");
        assert_eq!(removed, 1);
        assert_eq!(
            items,
            vec![
                ":1.43/org/example/Item".to_string(),
                "org.example.Service/StatusNotifierItem".to_string()
            ]
        );
    }

    #[test]
    fn is_item_owned_by_name_checks_boundary() {
        assert!(is_item_owned_by_name(":1.5/StatusNotifierItem", ":1.5"));
        assert!(!is_item_owned_by_name(":1.50/StatusNotifierItem", ":1.5"));
    }

    #[test]
    fn is_tray_relevant_name_ignores_unique_bus_names() {
        assert!(!is_tray_relevant_name(":1.2048"));
        assert!(is_tray_relevant_name("org.kde.StatusNotifierWatcher"));
        assert!(is_tray_relevant_name("org.ayatana.indicator.application"));
    }
}
