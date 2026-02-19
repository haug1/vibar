use std::collections::HashMap;
use std::thread;

use zbus::blocking::{Connection, Proxy};
use zbus::zvariant::{OwnedObjectPath, OwnedValue};
use zbus::Result as ZbusResult;

use super::types::{
    TrayMenuEntry, TrayMenuLayout, TrayMenuModel, DBUS_MENU_INTERFACE, ITEM_INTERFACE,
};

pub(super) fn fetch_dbus_menu_model(destination: &str, item_path: &str) -> Option<TrayMenuModel> {
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

pub(super) fn send_menu_event(destination: String, menu_path: String, item_id: i32) {
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
