use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};

use glib::ControlFlow;
use gtk::prelude::*;
use gtk::{
    Box as GtkBox, Button, GestureClick, Label, Orientation, Overlay, Popover, PositionType, Scale,
    Widget,
};
use serde::Deserialize;
use serde_json::Value;
use zbus::blocking::fdo::DBusProxy;
use zbus::blocking::{Connection, MessageIterator, Proxy};
use zbus::message::Type as MessageType;
use zbus::zvariant::{ObjectPath, OwnedValue};
use zbus::MatchRule;

use crate::modules::{
    apply_css_classes, attach_primary_click_command, ModuleBuildContext, ModuleConfig,
};

use super::ModuleFactory;

const DEFAULT_PLAYERCTL_INTERVAL_SECS: u32 = 1;
const DEFAULT_PLAYERCTL_FORMAT: &str = "{status_icon} {title}";
const DEFAULT_NO_PLAYER_TEXT: &str = "No media";
const MPRIS_PREFIX: &str = "org.mpris.MediaPlayer2.";
const MPRIS_PATH: &str = "/org/mpris/MediaPlayer2";
const MPRIS_PLAYER_INTERFACE: &str = "org.mpris.MediaPlayer2.Player";
const MPRIS_ROOT_INTERFACE: &str = "org.mpris.MediaPlayer2";
const DBUS_PROPERTIES_INTERFACE: &str = "org.freedesktop.DBus.Properties";
const PLAYERCTL_STATE_CLASSES: [&str; 4] = [
    "status-playing",
    "status-paused",
    "status-stopped",
    "no-player",
];
pub(crate) const MODULE_TYPE: &str = "playerctl";

#[derive(Debug, Deserialize, Clone)]
pub(crate) struct PlayerctlConfig {
    #[serde(default)]
    pub(crate) format: Option<String>,
    #[serde(default)]
    pub(crate) click: Option<String>,
    #[serde(rename = "on-click", default)]
    pub(crate) on_click: Option<String>,
    // Kept for backward-compatibility with existing configs.
    #[serde(default = "default_playerctl_interval")]
    pub(crate) interval_secs: u32,
    #[serde(default)]
    pub(crate) player: Option<String>,
    #[serde(default)]
    pub(crate) class: Option<String>,
    #[serde(default = "default_no_player_text")]
    pub(crate) no_player_text: String,
    #[serde(rename = "hide-when-idle", alias = "hide_when_idle", default)]
    pub(crate) hide_when_idle: bool,
    #[serde(
        rename = "show-when-paused",
        alias = "show_when_paused",
        default = "default_show_when_paused"
    )]
    pub(crate) show_when_paused: bool,
    #[serde(default)]
    pub(crate) controls: PlayerctlControlsConfig,
}

#[derive(Debug, Deserialize, Clone)]
pub(crate) struct PlayerctlControlsConfig {
    #[serde(default)]
    pub(crate) enabled: bool,
    #[serde(default)]
    pub(crate) open: PlayerctlControlsOpenMode,
    #[serde(
        rename = "show-seek",
        alias = "show_seek",
        default = "default_show_seek"
    )]
    pub(crate) show_seek: bool,
}

#[derive(Debug, Deserialize, Clone, Copy, Default)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum PlayerctlControlsOpenMode {
    #[serde(alias = "left_click", alias = "left")]
    #[default]
    LeftClick,
}

impl Default for PlayerctlControlsConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            open: PlayerctlControlsOpenMode::LeftClick,
            show_seek: default_show_seek(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PlayerctlMetadata {
    status: String,
    status_icon: &'static str,
    player: String,
    artist: String,
    album: String,
    title: String,
    position_micros: Option<i64>,
    length_micros: Option<i64>,
    can_go_previous: bool,
    can_go_next: bool,
    can_play: bool,
    can_pause: bool,
    can_seek: bool,
    track_id: Option<String>,
    bus_name: String,
}

#[derive(Debug, Clone)]
struct PlayerctlViewConfig {
    format: String,
    click_command: Option<String>,
    interval_secs: u32,
    player: Option<String>,
    class: Option<String>,
    no_player_text: String,
    hide_when_idle: bool,
    show_when_paused: bool,
    controls_enabled: bool,
    controls_open: PlayerctlControlsOpenMode,
    controls_show_seek: bool,
}

#[derive(Debug, Clone)]
enum BackendUpdate {
    Snapshot(Option<PlayerctlMetadata>),
    Error(String),
}

pub(crate) struct PlayerctlFactory;

pub(crate) const FACTORY: PlayerctlFactory = PlayerctlFactory;

impl ModuleFactory for PlayerctlFactory {
    fn module_type(&self) -> &'static str {
        MODULE_TYPE
    }

    fn init(&self, config: &ModuleConfig, _context: &ModuleBuildContext) -> Result<Widget, String> {
        let parsed = parse_config(config)?;
        let view = PlayerctlViewConfig {
            format: parsed
                .format
                .unwrap_or_else(|| DEFAULT_PLAYERCTL_FORMAT.to_string()),
            click_command: parsed.click.or(parsed.on_click),
            interval_secs: parsed.interval_secs,
            player: parsed.player,
            class: parsed.class,
            no_player_text: parsed.no_player_text,
            hide_when_idle: parsed.hide_when_idle,
            show_when_paused: parsed.show_when_paused,
            controls_enabled: parsed.controls.enabled,
            controls_open: parsed.controls.open,
            controls_show_seek: parsed.controls.show_seek,
        };

        Ok(build_playerctl_module(view).upcast())
    }
}

fn default_playerctl_interval() -> u32 {
    DEFAULT_PLAYERCTL_INTERVAL_SECS
}

fn default_no_player_text() -> String {
    DEFAULT_NO_PLAYER_TEXT.to_string()
}

fn default_show_when_paused() -> bool {
    true
}

fn default_show_seek() -> bool {
    true
}

pub(crate) fn parse_config(module: &ModuleConfig) -> Result<PlayerctlConfig, String> {
    if module.module_type != MODULE_TYPE {
        return Err(format!(
            "expected module type '{}', got '{}'",
            MODULE_TYPE, module.module_type
        ));
    }

    serde_json::from_value(Value::Object(module.config.clone()))
        .map_err(|err| format!("invalid {} module config: {err}", MODULE_TYPE))
}

#[derive(Clone)]
struct PlayerctlControlsUi {
    popover: Popover,
    previous_button: Button,
    play_pause_button: Button,
    next_button: Button,
    seek_scale: Scale,
    seek_widget: Widget,
    seek_time_widget: Widget,
    seek_position_label: Label,
    seek_length_label: Label,
    suppress_seek_callback: Arc<AtomicBool>,
    seek_update_hold_until: Arc<std::sync::Mutex<Option<Instant>>>,
    current_metadata: Arc<std::sync::Mutex<Option<PlayerctlMetadata>>>,
    show_seek: bool,
}

fn build_playerctl_module(config: PlayerctlViewConfig) -> Overlay {
    let root = Overlay::new();
    root.add_css_class("module");
    root.add_css_class("playerctl");
    root.set_focusable(false);
    root.set_focus_on_click(false);

    apply_css_classes(&root, config.class.as_deref());

    let label = Label::new(None);
    label.set_xalign(0.0);
    label.set_focusable(false);
    root.set_child(Some(&label));

    if !config.controls_enabled {
        attach_primary_click_command(&root, config.click_command.clone());
    }

    let controls_ui = if config.controls_enabled {
        let controls_ui = build_controls_ui(&root, config.controls_show_seek);
        install_controls_open_gesture(&root, &controls_ui.popover, config.controls_open);
        Some(controls_ui)
    } else {
        None
    };

    if config.interval_secs != DEFAULT_PLAYERCTL_INTERVAL_SECS {
        eprintln!(
            "playerctl interval_secs={} is ignored in event-driven mode",
            config.interval_secs
        );
    }

    let (sender, receiver) = mpsc::channel::<BackendUpdate>();
    std::thread::spawn({
        let player = config.player.clone();
        move || run_event_backend(sender, player)
    });

    glib::timeout_add_local(Duration::from_millis(200), {
        let root = root.clone();
        let label = label.clone();
        let format = config.format.clone();
        let no_player_text = config.no_player_text.clone();
        let hide_when_idle = config.hide_when_idle;
        let show_when_paused = config.show_when_paused;
        let controls_ui = controls_ui.clone();
        move || {
            while let Ok(update) = receiver.try_recv() {
                let (text, visibility, state_class) = match update {
                    BackendUpdate::Snapshot(Some(metadata)) => {
                        if let Some(controls) = &controls_ui {
                            refresh_controls_ui(controls, Some(&metadata));
                        }
                        (
                            render_format(&format, &metadata),
                            should_show_metadata(Some(&metadata), hide_when_idle, show_when_paused),
                            status_css_class(&metadata.status),
                        )
                    }
                    BackendUpdate::Snapshot(None) => (
                        {
                            if let Some(controls) = &controls_ui {
                                refresh_controls_ui(controls, None);
                            }
                            no_player_text.clone()
                        },
                        should_show_metadata(None, hide_when_idle, show_when_paused),
                        "no-player",
                    ),
                    BackendUpdate::Error(err) => {
                        if let Some(controls) = &controls_ui {
                            refresh_controls_ui(controls, None);
                        }
                        (format!("playerctl error: {err}"), true, "no-player")
                    }
                };
                label.set_text(&text);
                root.set_visible(visibility);
                apply_state_class(&root, state_class);
            }
            ControlFlow::Continue
        }
    });

    if let Some(controls) = controls_ui {
        wire_controls_actions(controls);
    }

    root
}

fn build_controls_ui(root: &Overlay, show_seek: bool) -> PlayerctlControlsUi {
    root.add_css_class("clickable");
    root.add_css_class("playerctl-controls-enabled");

    let popover = Popover::new();
    popover.add_css_class("playerctl-controls-popover");
    popover.set_autohide(true);
    popover.set_has_arrow(true);
    popover.set_position(PositionType::Top);
    popover.set_parent(root);

    let content = GtkBox::new(Orientation::Vertical, 6);
    content.add_css_class("playerctl-controls-content");
    popover.set_child(Some(&content));

    let buttons_row = GtkBox::new(Orientation::Horizontal, 6);
    buttons_row.add_css_class("playerctl-controls-row");
    content.append(&buttons_row);

    let previous_button = Button::with_label("");
    previous_button.add_css_class("playerctl-control-button");
    buttons_row.append(&previous_button);

    let play_pause_button = Button::with_label("");
    play_pause_button.add_css_class("playerctl-control-button");
    buttons_row.append(&play_pause_button);

    let next_button = Button::with_label("");
    next_button.add_css_class("playerctl-control-button");
    buttons_row.append(&next_button);

    let seek_scale = Scale::with_range(Orientation::Horizontal, 0.0, 1.0, 0.001);
    seek_scale.add_css_class("playerctl-seek-scale");
    seek_scale.set_draw_value(false);
    seek_scale.set_hexpand(true);
    seek_scale.set_sensitive(false);

    let seek_widget: Widget = seek_scale.clone().upcast();
    seek_widget.set_visible(show_seek);
    content.append(&seek_widget);

    let seek_time_row = GtkBox::new(Orientation::Horizontal, 8);
    seek_time_row.add_css_class("playerctl-seek-time-row");
    let seek_position_label = Label::new(Some("00:00"));
    seek_position_label.add_css_class("playerctl-seek-time");
    seek_position_label.set_xalign(0.0);
    seek_position_label.set_hexpand(true);
    seek_time_row.append(&seek_position_label);

    let seek_length_label = Label::new(Some("00:00"));
    seek_length_label.add_css_class("playerctl-seek-time");
    seek_length_label.set_xalign(1.0);
    seek_length_label.set_hexpand(true);
    seek_time_row.append(&seek_length_label);

    let seek_time_widget: Widget = seek_time_row.clone().upcast();
    seek_time_widget.set_visible(show_seek);
    content.append(&seek_time_widget);

    let suppress_seek_callback = Arc::new(AtomicBool::new(false));
    let seek_update_hold_until = Arc::new(std::sync::Mutex::new(None));
    let current_metadata = Arc::new(std::sync::Mutex::new(None));

    let press_gesture = GestureClick::builder().button(1).build();
    {
        let seek_update_hold_until = seek_update_hold_until.clone();
        press_gesture.connect_pressed(move |_, _, _, _| {
            if let Ok(mut slot) = seek_update_hold_until.lock() {
                *slot = Some(Instant::now() + Duration::from_secs(2));
            }
        });
    }
    {
        let seek_update_hold_until = seek_update_hold_until.clone();
        press_gesture.connect_released(move |_, _, _, _| {
            if let Ok(mut slot) = seek_update_hold_until.lock() {
                *slot = Some(Instant::now() + Duration::from_millis(300));
            }
        });
    }
    seek_scale.add_controller(press_gesture);

    PlayerctlControlsUi {
        popover,
        previous_button,
        play_pause_button,
        next_button,
        seek_scale,
        seek_widget,
        seek_time_widget,
        seek_position_label,
        seek_length_label,
        suppress_seek_callback,
        seek_update_hold_until,
        current_metadata,
        show_seek,
    }
}

fn install_controls_open_gesture(
    root: &Overlay,
    popover: &Popover,
    open_mode: PlayerctlControlsOpenMode,
) {
    match open_mode {
        PlayerctlControlsOpenMode::LeftClick => {
            let click = GestureClick::builder().button(1).build();
            let popover = popover.clone();
            click.connect_pressed(move |_, _, _, _| {
                if popover.is_visible() {
                    popover.popdown();
                } else {
                    popover.popup();
                }
            });
            root.add_controller(click);
        }
    }
}

fn wire_controls_actions(controls_ui: PlayerctlControlsUi) {
    let current_metadata_for_previous = controls_ui.current_metadata.clone();
    controls_ui.previous_button.connect_clicked(move |_| {
        let bus_name = current_metadata_for_previous
            .lock()
            .ok()
            .and_then(|slot| slot.as_ref().map(|metadata| metadata.bus_name.clone()));
        if let Some(bus_name) = bus_name {
            std::thread::spawn(move || {
                let _ = call_player_method(&bus_name, "Previous");
            });
        }
    });

    let current_metadata_for_play_pause = controls_ui.current_metadata.clone();
    controls_ui.play_pause_button.connect_clicked(move |_| {
        let bus_name = current_metadata_for_play_pause
            .lock()
            .ok()
            .and_then(|slot| slot.as_ref().map(|metadata| metadata.bus_name.clone()));
        if let Some(bus_name) = bus_name {
            std::thread::spawn(move || {
                let _ = call_player_method(&bus_name, "PlayPause");
            });
        }
    });

    let current_metadata_for_next = controls_ui.current_metadata.clone();
    controls_ui.next_button.connect_clicked(move |_| {
        let bus_name = current_metadata_for_next
            .lock()
            .ok()
            .and_then(|slot| slot.as_ref().map(|metadata| metadata.bus_name.clone()));
        if let Some(bus_name) = bus_name {
            std::thread::spawn(move || {
                let _ = call_player_method(&bus_name, "Next");
            });
        }
    });

    let current_metadata_for_seek = controls_ui.current_metadata.clone();
    let suppress_seek_callback = controls_ui.suppress_seek_callback.clone();
    let seek_update_hold_until = controls_ui.seek_update_hold_until.clone();
    controls_ui.seek_scale.connect_value_changed(move |scale| {
        if suppress_seek_callback.load(Ordering::Relaxed) {
            return;
        }
        if let Ok(mut slot) = seek_update_hold_until.lock() {
            *slot = Some(Instant::now() + Duration::from_millis(700));
        }

        let Some(metadata) = current_metadata_for_seek
            .lock()
            .ok()
            .and_then(|slot| slot.clone())
        else {
            return;
        };

        let Some(duration_micros) = metadata.length_micros else {
            return;
        };
        if duration_micros <= 0 || !metadata.can_seek {
            return;
        }

        let Some(track_id) = metadata.track_id.clone() else {
            return;
        };

        let ratio = scale.value().clamp(0.0, 1.0);
        let target_position = ((duration_micros as f64) * ratio).round() as i64;
        let bus_name = metadata.bus_name;

        std::thread::spawn(move || {
            let _ = call_set_position(&bus_name, &track_id, target_position);
        });
    });
}

fn refresh_controls_ui(controls_ui: &PlayerctlControlsUi, metadata: Option<&PlayerctlMetadata>) {
    if let Ok(mut slot) = controls_ui.current_metadata.lock() {
        *slot = metadata.cloned();
    }

    let Some(metadata) = metadata else {
        controls_ui.previous_button.set_sensitive(false);
        controls_ui.play_pause_button.set_sensitive(false);
        controls_ui.play_pause_button.set_label("");
        controls_ui.next_button.set_sensitive(false);
        controls_ui.seek_scale.set_sensitive(false);
        controls_ui.seek_widget.set_visible(controls_ui.show_seek);
        controls_ui
            .seek_time_widget
            .set_visible(controls_ui.show_seek);
        controls_ui.seek_position_label.set_text("00:00");
        controls_ui.seek_length_label.set_text("00:00");
        return;
    };

    controls_ui
        .previous_button
        .set_sensitive(metadata.can_go_previous);
    controls_ui.next_button.set_sensitive(metadata.can_go_next);

    let can_toggle_playback = metadata.can_play || metadata.can_pause;
    controls_ui
        .play_pause_button
        .set_sensitive(can_toggle_playback);
    let toggle_icon = if metadata.status == "playing" {
        ""
    } else {
        ""
    };
    controls_ui.play_pause_button.set_label(toggle_icon);

    let can_seek = metadata.can_seek
        && metadata.length_micros.is_some_and(|length| length > 0)
        && metadata.track_id.is_some();
    controls_ui.seek_widget.set_visible(controls_ui.show_seek);
    controls_ui
        .seek_time_widget
        .set_visible(controls_ui.show_seek);
    controls_ui.seek_scale.set_sensitive(can_seek);

    if let Ok(mut slot) = controls_ui.seek_update_hold_until.lock() {
        if slot.is_some_and(|until| Instant::now() < until) {
            controls_ui
                .seek_position_label
                .set_text(&format_timestamp_micros(metadata.position_micros));
            controls_ui
                .seek_length_label
                .set_text(&format_timestamp_micros(metadata.length_micros));
            return;
        }
        *slot = None;
    }

    let ratio = metadata_seek_ratio(metadata).unwrap_or(0.0).clamp(0.0, 1.0);
    controls_ui
        .suppress_seek_callback
        .store(true, Ordering::Relaxed);
    controls_ui.seek_scale.set_value(ratio);
    controls_ui
        .suppress_seek_callback
        .store(false, Ordering::Relaxed);

    controls_ui
        .seek_position_label
        .set_text(&format_timestamp_micros(metadata.position_micros));
    controls_ui
        .seek_length_label
        .set_text(&format_timestamp_micros(metadata.length_micros));
}

fn metadata_seek_ratio(metadata: &PlayerctlMetadata) -> Option<f64> {
    let position = metadata.position_micros?;
    let length = metadata.length_micros?;
    if length <= 0 {
        return None;
    }

    Some(position as f64 / length as f64)
}

fn format_timestamp_micros(value: Option<i64>) -> String {
    let Some(micros) = value else {
        return "00:00".to_string();
    };
    let total_seconds = (micros.max(0) / 1_000_000) as u64;
    let minutes = total_seconds / 60;
    let seconds = total_seconds % 60;
    format!("{minutes:02}:{seconds:02}")
}

fn call_player_method(bus_name: &str, method: &str) -> Result<(), String> {
    let connection =
        Connection::session().map_err(|err| format!("failed to connect to D-Bus: {err}"))?;
    let proxy = Proxy::new(&connection, bus_name, MPRIS_PATH, MPRIS_PLAYER_INTERFACE)
        .map_err(|err| format!("failed to create player proxy for {bus_name}: {err}"))?;
    proxy
        .call_method(method, &())
        .map_err(|err| format!("failed to call {method} on {bus_name}: {err}"))?;
    Ok(())
}

fn call_set_position(bus_name: &str, track_id: &str, position_micros: i64) -> Result<(), String> {
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

fn run_event_backend(ui_sender: Sender<BackendUpdate>, player_filter: Option<String>) {
    let (trigger_tx, trigger_rx) = mpsc::channel::<()>();

    start_name_owner_listener(trigger_tx.clone());
    start_properties_listener(trigger_tx);

    // Prime with one initial snapshot.
    publish_snapshot(&ui_sender, player_filter.as_deref());

    while let Ok(_) | Err(RecvTimeoutError::Timeout) =
        trigger_rx.recv_timeout(Duration::from_millis(500))
    {
        publish_snapshot(&ui_sender, player_filter.as_deref());
    }
}

fn publish_snapshot(ui_sender: &Sender<BackendUpdate>, player_filter: Option<&str>) {
    let update = match query_active_player_metadata(player_filter) {
        Ok(snapshot) => BackendUpdate::Snapshot(snapshot),
        Err(err) => BackendUpdate::Error(err),
    };

    let _ = ui_sender.send(update);
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
            if name_owner_changed_is_mpris(&signal) {
                let _ = trigger_tx.send(());
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

            if is_mpris_properties_changed(&message) {
                let _ = trigger_tx.send(());
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

fn select_active_player(candidates: Vec<PlayerctlMetadata>) -> Option<PlayerctlMetadata> {
    candidates.into_iter().min_by(|a, b| {
        active_rank(&a.status)
            .cmp(&active_rank(&b.status))
            .then(a.bus_name.cmp(&b.bus_name))
    })
}

fn active_rank(status: &str) -> u8 {
    match status {
        "playing" => 0,
        "paused" => 1,
        "stopped" => 2,
        _ => 3,
    }
}

fn matches_player_filter(bus_name: &str, filter: &str) -> bool {
    bus_name == filter
        || bus_name
            .strip_prefix(MPRIS_PREFIX)
            .is_some_and(|short_name| short_name == filter)
}

fn short_player_name(bus_name: &str) -> String {
    bus_name
        .strip_prefix(MPRIS_PREFIX)
        .unwrap_or(bus_name)
        .to_string()
}

fn normalize_status(status: &str) -> String {
    status.to_ascii_lowercase()
}

fn status_icon_for(status: &str) -> &'static str {
    match status {
        "playing" => "",
        "paused" => "",
        "stopped" => "",
        _ => "",
    }
}

fn metadata_string(metadata: &HashMap<String, OwnedValue>, key: &str) -> Option<String> {
    metadata
        .get(key)
        .and_then(|value| value.try_clone().ok())
        .and_then(|value| String::try_from(value).ok())
        .filter(|value| !value.is_empty())
}

fn metadata_artist(metadata: &HashMap<String, OwnedValue>) -> Option<String> {
    let value = metadata.get("xesam:artist")?.try_clone().ok()?;

    if let Ok(artists) = Vec::<String>::try_from(value.try_clone().ok()?) {
        let joined = artists
            .into_iter()
            .filter(|artist| !artist.is_empty())
            .collect::<Vec<_>>()
            .join(", ");
        if !joined.is_empty() {
            return Some(joined);
        }
    }

    String::try_from(value)
        .ok()
        .filter(|value| !value.is_empty())
}

fn metadata_i64(metadata: &HashMap<String, OwnedValue>, key: &str) -> Option<i64> {
    let value = metadata.get(key)?.try_clone().ok()?;

    i64::try_from(value.try_clone().ok()?)
        .ok()
        .or_else(|| i32::try_from(value.try_clone().ok()?).ok().map(i64::from))
        .or_else(|| {
            u64::try_from(value.try_clone().ok()?)
                .ok()
                .and_then(|v| i64::try_from(v).ok())
        })
        .or_else(|| u32::try_from(value).ok().map(i64::from))
}

fn metadata_object_path_string(
    metadata: &HashMap<String, OwnedValue>,
    key: &str,
) -> Option<String> {
    let value = metadata.get(key)?.try_clone().ok()?;
    ObjectPath::try_from(value)
        .ok()
        .map(|path| path.to_string())
        .filter(|path| !path.is_empty())
}

fn render_format(format: &str, metadata: &PlayerctlMetadata) -> String {
    format
        .replace("{status}", &metadata.status)
        .replace("{status_icon}", metadata.status_icon)
        .replace("{player}", &metadata.player)
        .replace("{artist}", &metadata.artist)
        .replace("{album}", &metadata.album)
        .replace("{title}", &metadata.title)
}

fn should_show_metadata(
    metadata: Option<&PlayerctlMetadata>,
    hide_when_idle: bool,
    show_when_paused: bool,
) -> bool {
    if !hide_when_idle {
        return true;
    }

    let Some(metadata) = metadata else {
        return false;
    };

    match metadata.status.as_str() {
        "playing" => true,
        "paused" => show_when_paused,
        _ => false,
    }
}

fn status_css_class(status: &str) -> &'static str {
    match status {
        "playing" => "status-playing",
        "paused" => "status-paused",
        "stopped" => "status-stopped",
        _ => "no-player",
    }
}

fn apply_state_class(widget: &impl IsA<Widget>, active_class: &str) {
    for class_name in PLAYERCTL_STATE_CLASSES {
        widget.remove_css_class(class_name);
    }
    widget.add_css_class(active_class);
}

#[cfg(test)]
mod tests {
    use serde_json::{json, Map};

    use super::*;

    #[test]
    fn parse_config_rejects_wrong_module_type() {
        let module = ModuleConfig::new("clock", Map::new());
        let err = parse_config(&module).expect_err("wrong type should fail");
        assert!(err.contains("expected module type 'playerctl'"));
    }

    #[test]
    fn matches_player_filter_accepts_full_and_short_names() {
        assert!(matches_player_filter(
            "org.mpris.MediaPlayer2.spotify",
            "org.mpris.MediaPlayer2.spotify"
        ));
        assert!(matches_player_filter(
            "org.mpris.MediaPlayer2.spotify",
            "spotify"
        ));
        assert!(!matches_player_filter(
            "org.mpris.MediaPlayer2.spotify",
            "mpv"
        ));
    }

    #[test]
    fn select_active_player_prefers_playing_then_name() {
        let chosen = select_active_player(vec![
            PlayerctlMetadata {
                status: "paused".to_string(),
                status_icon: "",
                player: "vlc".to_string(),
                artist: String::new(),
                album: String::new(),
                title: String::new(),
                position_micros: None,
                length_micros: None,
                can_go_previous: false,
                can_go_next: false,
                can_play: false,
                can_pause: false,
                can_seek: false,
                track_id: None,
                bus_name: "org.mpris.MediaPlayer2.vlc".to_string(),
            },
            PlayerctlMetadata {
                status: "playing".to_string(),
                status_icon: "",
                player: "spotify".to_string(),
                artist: String::new(),
                album: String::new(),
                title: String::new(),
                position_micros: None,
                length_micros: None,
                can_go_previous: false,
                can_go_next: false,
                can_play: false,
                can_pause: false,
                can_seek: false,
                track_id: None,
                bus_name: "org.mpris.MediaPlayer2.spotify".to_string(),
            },
        ])
        .expect("one player should be selected");

        assert_eq!(chosen.status, "playing");
        assert_eq!(chosen.bus_name, "org.mpris.MediaPlayer2.spotify");
    }

    #[test]
    fn render_format_replaces_placeholders() {
        let metadata = PlayerctlMetadata {
            status: "paused".to_string(),
            status_icon: "",
            player: "spotify".to_string(),
            artist: "Boards of Canada".to_string(),
            album: "Music Has the Right to Children".to_string(),
            title: "Roygbiv".to_string(),
            position_micros: None,
            length_micros: None,
            can_go_previous: false,
            can_go_next: false,
            can_play: false,
            can_pause: false,
            can_seek: false,
            track_id: None,
            bus_name: "org.mpris.MediaPlayer2.spotify".to_string(),
        };

        let text = render_format(
            "{status_icon} {artist} - {title} ({player}) [{status}]",
            &metadata,
        );
        assert_eq!(text, " Boards of Canada - Roygbiv (spotify) [paused]");
    }

    #[test]
    fn parse_config_applies_visibility_defaults() {
        let module = ModuleConfig::new(MODULE_TYPE, Map::new());
        let cfg = parse_config(&module).expect("config should parse");

        assert!(!cfg.hide_when_idle);
        assert!(cfg.show_when_paused);
    }

    #[test]
    fn parse_config_applies_controls_defaults() {
        let module = ModuleConfig::new(MODULE_TYPE, Map::new());
        let cfg = parse_config(&module).expect("config should parse");

        assert!(!cfg.controls.enabled);
        assert!(cfg.controls.show_seek);
        assert!(matches!(
            cfg.controls.open,
            PlayerctlControlsOpenMode::LeftClick
        ));
    }

    #[test]
    fn parse_config_supports_controls_keys() {
        let module = ModuleConfig::new(
            MODULE_TYPE,
            serde_json::from_value(json!({
                "controls": {
                    "enabled": true,
                    "open": "left-click",
                    "show_seek": false
                }
            }))
            .expect("playerctl config map should parse"),
        );
        let cfg = parse_config(&module).expect("config should parse");

        assert!(cfg.controls.enabled);
        assert!(matches!(
            cfg.controls.open,
            PlayerctlControlsOpenMode::LeftClick
        ));
        assert!(!cfg.controls.show_seek);
    }

    #[test]
    fn parse_config_supports_controls_aliases() {
        let module = ModuleConfig::new(
            MODULE_TYPE,
            serde_json::from_value(json!({
                "controls": {
                    "enabled": true,
                    "open": "left_click",
                    "show-seek": false
                }
            }))
            .expect("playerctl config map should parse"),
        );
        let cfg = parse_config(&module).expect("config should parse");

        assert!(cfg.controls.enabled);
        assert!(matches!(
            cfg.controls.open,
            PlayerctlControlsOpenMode::LeftClick
        ));
        assert!(!cfg.controls.show_seek);
    }

    #[test]
    fn should_show_metadata_respects_visibility_settings() {
        let playing = PlayerctlMetadata {
            status: "playing".to_string(),
            status_icon: "",
            player: String::new(),
            artist: String::new(),
            album: String::new(),
            title: String::new(),
            position_micros: None,
            length_micros: None,
            can_go_previous: false,
            can_go_next: false,
            can_play: false,
            can_pause: false,
            can_seek: false,
            track_id: None,
            bus_name: String::new(),
        };
        let paused = PlayerctlMetadata {
            status: "paused".to_string(),
            ..playing.clone()
        };
        let stopped = PlayerctlMetadata {
            status: "stopped".to_string(),
            ..playing.clone()
        };

        assert!(should_show_metadata(Some(&playing), true, true));
        assert!(should_show_metadata(Some(&paused), true, true));
        assert!(!should_show_metadata(Some(&paused), true, false));
        assert!(!should_show_metadata(Some(&stopped), true, true));
        assert!(!should_show_metadata(None, true, true));
        assert!(should_show_metadata(None, false, false));
    }

    #[test]
    fn status_css_class_maps_statuses() {
        assert_eq!(status_css_class("playing"), "status-playing");
        assert_eq!(status_css_class("paused"), "status-paused");
        assert_eq!(status_css_class("stopped"), "status-stopped");
        assert_eq!(status_css_class("unknown"), "no-player");
    }

    #[test]
    fn metadata_seek_ratio_handles_expected_cases() {
        let metadata = PlayerctlMetadata {
            status: "playing".to_string(),
            status_icon: "",
            player: String::new(),
            artist: String::new(),
            album: String::new(),
            title: String::new(),
            position_micros: Some(30_000_000),
            length_micros: Some(120_000_000),
            can_go_previous: false,
            can_go_next: false,
            can_play: false,
            can_pause: false,
            can_seek: true,
            track_id: Some("/org/mpris/MediaPlayer2/track/1".to_string()),
            bus_name: String::new(),
        };
        assert_eq!(metadata_seek_ratio(&metadata), Some(0.25));

        let zero_length = PlayerctlMetadata {
            length_micros: Some(0),
            ..metadata.clone()
        };
        assert_eq!(metadata_seek_ratio(&zero_length), None);

        let missing_position = PlayerctlMetadata {
            position_micros: None,
            ..metadata
        };
        assert_eq!(metadata_seek_ratio(&missing_position), None);
    }

    #[test]
    fn format_timestamp_micros_formats_mm_ss() {
        assert_eq!(format_timestamp_micros(None), "00:00");
        assert_eq!(format_timestamp_micros(Some(-5)), "00:00");
        assert_eq!(format_timestamp_micros(Some(5_000_000)), "00:05");
        assert_eq!(format_timestamp_micros(Some(65_000_000)), "01:05");
    }
}
