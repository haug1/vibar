use std::collections::HashMap;
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::time::Duration;

use zbus::blocking::fdo::DBusProxy;
use zbus::blocking::{Connection, MessageIterator, Proxy};
use zbus::message::Type as MessageType;
use zbus::zvariant::{ObjectPath, OwnedValue};
use zbus::MatchRule;

use super::model::{
    matches_player_filter, metadata_artist, metadata_i64, metadata_object_path_string,
    metadata_string, normalize_status, select_active_player, short_player_name, status_icon_for,
    BackendUpdate, PlayerctlMetadata,
};

pub(super) const MPRIS_PREFIX: &str = "org.mpris.MediaPlayer2.";
pub(super) const MPRIS_PATH: &str = "/org/mpris/MediaPlayer2";
const MPRIS_PLAYER_INTERFACE: &str = "org.mpris.MediaPlayer2.Player";
const MPRIS_ROOT_INTERFACE: &str = "org.mpris.MediaPlayer2";
const DBUS_PROPERTIES_INTERFACE: &str = "org.freedesktop.DBus.Properties";

pub(super) fn call_player_method(bus_name: &str, method: &str) -> Result<(), String> {
    let connection =
        Connection::session().map_err(|err| format!("failed to connect to D-Bus: {err}"))?;
    let proxy = Proxy::new(&connection, bus_name, MPRIS_PATH, MPRIS_PLAYER_INTERFACE)
        .map_err(|err| format!("failed to create player proxy for {bus_name}: {err}"))?;
    proxy
        .call_method(method, &())
        .map_err(|err| format!("failed to call {method} on {bus_name}: {err}"))?;
    Ok(())
}

pub(super) fn call_set_position(
    bus_name: &str,
    track_id: &str,
    position_micros: i64,
) -> Result<(), String> {
    let connection =
        Connection::session().map_err(|err| format!("failed to connect to D-Bus: {err}"))?;
    let proxy = Proxy::new(&connection, bus_name, MPRIS_PATH, MPRIS_PLAYER_INTERFACE)
        .map_err(|err| format!("failed to create player proxy for {bus_name}: {err}"))?;
    let track_path = ObjectPath::try_from(track_id)
        .map_err(|err| format!("failed to parse track id '{track_id}' as object path: {err}"))?;
    proxy
        .call_method("SetPosition", &(track_path, position_micros))
        .map_err(|err| format!("failed to call SetPosition on {bus_name}: {err}"))?;
    Ok(())
}

pub(super) fn run_event_backend(ui_sender: Sender<BackendUpdate>, player_filter: Option<String>) {
    let (trigger_tx, trigger_rx) = mpsc::channel::<()>();

    start_name_owner_listener(trigger_tx.clone());
    start_properties_listener(trigger_tx);

    if !publish_snapshot(&ui_sender, player_filter.as_deref()) {
        return;
    }

    while let Ok(_) | Err(RecvTimeoutError::Timeout) =
        trigger_rx.recv_timeout(Duration::from_millis(500))
    {
        if !publish_snapshot(&ui_sender, player_filter.as_deref()) {
            return;
        }
    }
}

fn publish_snapshot(ui_sender: &Sender<BackendUpdate>, player_filter: Option<&str>) -> bool {
    let update = match query_active_player_metadata(player_filter) {
        Ok(snapshot) => BackendUpdate::Snapshot(snapshot),
        Err(err) => BackendUpdate::Error(err),
    };

    ui_sender.send(update).is_ok()
}

fn start_name_owner_listener(trigger_tx: Sender<()>) {
    std::thread::spawn(move || {
        let Ok(connection) = Connection::session() else {
            eprintln!("playerctl: failed to open session bus for NameOwnerChanged listener");
            return;
        };
        let Ok(proxy) = DBusProxy::new(&connection) else {
            eprintln!("playerctl: failed to create DBus proxy for NameOwnerChanged listener");
            return;
        };
        let Ok(mut signals) = proxy.receive_name_owner_changed() else {
            eprintln!("playerctl: failed to subscribe to NameOwnerChanged");
            return;
        };

        for signal in &mut signals {
            if name_owner_changed_is_mpris(&signal) && trigger_tx.send(()).is_err() {
                return;
            }
        }
    });
}

fn start_properties_listener(trigger_tx: Sender<()>) {
    std::thread::spawn(move || {
        let Ok(connection) = Connection::session() else {
            eprintln!("playerctl: failed to open session bus for PropertiesChanged listener");
            return;
        };

        let rule = match MatchRule::builder()
            .msg_type(MessageType::Signal)
            .interface(DBUS_PROPERTIES_INTERFACE)
            .and_then(|builder| builder.member("PropertiesChanged"))
            .and_then(|builder| builder.path(MPRIS_PATH))
            .map(|builder| builder.build())
        {
            Ok(rule) => rule,
            Err(err) => {
                eprintln!("playerctl: failed to build PropertiesChanged match rule: {err}");
                return;
            }
        };

        let Ok(iterator) = MessageIterator::for_match_rule(rule, &connection, Some(256)) else {
            eprintln!("playerctl: failed to subscribe to PropertiesChanged");
            return;
        };

        for message in iterator {
            let Ok(message) = message else {
                continue;
            };

            if is_mpris_properties_changed(&message) && trigger_tx.send(()).is_err() {
                return;
            }
        }
    });
}

fn is_mpris_properties_changed(message: &zbus::Message) -> bool {
    let Ok((interface_name, _, _)) =
        message
            .body()
            .deserialize::<(String, HashMap<String, OwnedValue>, Vec<String>)>()
    else {
        return false;
    };

    interface_name == MPRIS_PLAYER_INTERFACE || interface_name == MPRIS_ROOT_INTERFACE
}

fn name_owner_changed_is_mpris(signal: &zbus::blocking::fdo::NameOwnerChanged) -> bool {
    signal
        .args()
        .ok()
        .map(|args| args.name().starts_with(MPRIS_PREFIX))
        .unwrap_or(false)
}

fn query_active_player_metadata(
    player_filter: Option<&str>,
) -> Result<Option<PlayerctlMetadata>, String> {
    let connection =
        Connection::session().map_err(|err| format!("failed to connect to D-Bus: {err}"))?;
    let proxy =
        DBusProxy::new(&connection).map_err(|err| format!("failed to create DBus proxy: {err}"))?;
    let names = proxy
        .list_names()
        .map_err(|err| format!("failed to list D-Bus names: {err}"))?;

    let mut players = names
        .into_iter()
        .map(|name| name.to_string())
        .filter(|name| name.starts_with(MPRIS_PREFIX))
        .collect::<Vec<_>>();
    players.sort();

    if let Some(filter) = player_filter {
        players.retain(|name| matches_player_filter(name, filter));
    }

    if players.is_empty() {
        return Ok(None);
    }

    let mut candidates = Vec::new();
    for bus_name in players {
        if let Ok(metadata) = read_player_metadata(&connection, &bus_name) {
            candidates.push(metadata);
        }
    }

    Ok(select_active_player(candidates))
}

fn read_player_metadata(
    connection: &Connection,
    bus_name: &str,
) -> Result<PlayerctlMetadata, String> {
    let player_proxy = Proxy::new(connection, bus_name, MPRIS_PATH, MPRIS_PLAYER_INTERFACE)
        .map_err(|err| format!("failed to create player proxy for {bus_name}: {err}"))?;
    let root_proxy = Proxy::new(connection, bus_name, MPRIS_PATH, MPRIS_ROOT_INTERFACE)
        .map_err(|err| format!("failed to create root proxy for {bus_name}: {err}"))?;

    let status = player_proxy
        .get_property::<String>("PlaybackStatus")
        .map(|raw| normalize_status(raw.as_str()))
        .map_err(|err| format!("failed to read PlaybackStatus for {bus_name}: {err}"))?;
    let metadata = player_proxy
        .get_property::<HashMap<String, OwnedValue>>("Metadata")
        .unwrap_or_default();

    let player = root_proxy
        .get_property::<String>("Identity")
        .ok()
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| short_player_name(bus_name));

    Ok(PlayerctlMetadata {
        status_icon: status_icon_for(&status),
        status,
        player,
        artist: metadata_artist(&metadata).unwrap_or_default(),
        album: metadata_string(&metadata, "xesam:album").unwrap_or_default(),
        title: metadata_string(&metadata, "xesam:title").unwrap_or_default(),
        position_micros: player_proxy.get_property::<i64>("Position").ok(),
        length_micros: metadata_i64(&metadata, "mpris:length"),
        can_go_previous: player_proxy
            .get_property::<bool>("CanGoPrevious")
            .unwrap_or(false),
        can_go_next: player_proxy
            .get_property::<bool>("CanGoNext")
            .unwrap_or(false),
        can_play: player_proxy
            .get_property::<bool>("CanPlay")
            .unwrap_or(false),
        can_pause: player_proxy
            .get_property::<bool>("CanPause")
            .unwrap_or(false),
        can_seek: player_proxy
            .get_property::<bool>("CanSeek")
            .unwrap_or(false),
        track_id: metadata_object_path_string(&metadata, "mpris:trackid"),
        bus_name: bus_name.to_string(),
    })
}
