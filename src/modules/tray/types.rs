use std::collections::HashMap;

use serde::Deserialize;
use zbus::zvariant::OwnedValue;

pub(super) const WATCHER_DESTINATION: &str = "org.kde.StatusNotifierWatcher";
pub(super) const WATCHER_PATH: &str = "/StatusNotifierWatcher";
pub(super) const WATCHER_INTERFACE: &str = "org.kde.StatusNotifierWatcher";
pub(super) const ITEM_INTERFACE: &str = "org.kde.StatusNotifierItem";
pub(super) const DBUS_MENU_INTERFACE: &str = "com.canonical.dbusmenu";
pub(super) const MODULE_TYPE: &str = "tray";
pub(super) const DEFAULT_ICON_SIZE: i32 = 16;
pub(super) const MIN_ICON_SIZE: i32 = 8;
pub(super) const DEFAULT_POLL_INTERVAL_SECS: u32 = 2;
pub(super) const MIN_POLL_INTERVAL_SECS: u32 = 1;

#[derive(Debug, Deserialize, Clone)]
pub(super) struct TrayConfig {
    #[serde(default = "default_icon_size")]
    pub(super) icon_size: i32,
    #[serde(default = "default_poll_interval")]
    pub(super) poll_interval_secs: u32,
    #[serde(default)]
    pub(super) class: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct TrayItemSnapshot {
    pub(super) id: String,
    pub(super) destination: String,
    pub(super) path: String,
    pub(super) icon_name: String,
    pub(super) title: String,
}

#[derive(Debug, Clone)]
pub(super) struct TrayMenuEntry {
    pub(super) id: i32,
    pub(super) label: String,
    pub(super) icon_name: Option<String>,
    pub(super) icon_data: Option<Vec<u8>>,
    pub(super) enabled: bool,
    pub(super) visible: bool,
    pub(super) is_separator: bool,
    pub(super) submenu_hint: bool,
    pub(super) children: Vec<TrayMenuEntry>,
}

#[derive(Debug, Clone)]
pub(super) struct TrayMenuModel {
    pub(super) menu_path: String,
    pub(super) entries: Vec<TrayMenuEntry>,
}

pub(super) type TrayMenuLayout = (i32, HashMap<String, OwnedValue>, Vec<OwnedValue>);

fn default_icon_size() -> i32 {
    DEFAULT_ICON_SIZE
}

fn default_poll_interval() -> u32 {
    DEFAULT_POLL_INTERVAL_SECS
}
