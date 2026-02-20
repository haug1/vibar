use std::io::{Read, Write};
use std::os::fd::AsRawFd;
use std::process::Command;

use glib::ControlFlow;
use gtk::prelude::*;
use gtk::{Box as GtkBox, Button, Label, Orientation, Widget};
use serde::Deserialize;
use serde_json::{Map, Value};
use swayipc::{Connection, EventType};

use crate::modules::{ModuleBuildContext, ModuleConfig, ModuleFactory};

#[derive(Debug, Deserialize, Clone, Default)]
pub(crate) struct WorkspaceConfig {}

pub(crate) struct SwayWorkspaceFactory;

pub(crate) const FACTORY: SwayWorkspaceFactory = SwayWorkspaceFactory;
pub(crate) const MODULE_TYPE: &str = "workspaces";

impl ModuleFactory for SwayWorkspaceFactory {
    fn module_type(&self) -> &'static str {
        MODULE_TYPE
    }

    fn init(&self, config: &ModuleConfig, context: &ModuleBuildContext) -> Result<Widget, String> {
        let _ = parse_config(config)?;
        Ok(build_workspaces_module(context.monitor_connector.clone()).upcast())
    }
}

pub(crate) fn default_module_config() -> ModuleConfig {
    ModuleConfig::new(MODULE_TYPE, Map::new())
}

fn parse_config(module: &ModuleConfig) -> Result<WorkspaceConfig, String> {
    if module.module_type != MODULE_TYPE {
        return Err(format!(
            "expected module type '{}', got '{}'",
            MODULE_TYPE, module.module_type
        ));
    }

    serde_json::from_value(Value::Object(module.config.clone()))
        .map_err(|err| format!("invalid {} module config: {err}", MODULE_TYPE))
}

pub(crate) fn build_workspaces_module(output_filter: Option<String>) -> GtkBox {
    let container = GtkBox::new(Orientation::Horizontal, 4);
    container.add_css_class("module");
    container.add_css_class("workspaces");

    let (mut signal_rx, signal_tx) = match std::os::unix::net::UnixStream::pair() {
        Ok(pair) => pair,
        Err(err) => {
            eprintln!("vibar/workspaces: failed to create event signal pipe: {err}");
            refresh_workspaces(&container, output_filter.as_deref());
            return container;
        }
    };
    if let Err(err) = signal_rx.set_nonblocking(true) {
        eprintln!("vibar/workspaces: failed to set nonblocking event signal pipe: {err}");
        refresh_workspaces(&container, output_filter.as_deref());
        return container;
    }

    start_workspace_event_listener(signal_tx);
    refresh_workspaces(&container, output_filter.as_deref());

    // Refresh only when the sway listener emits an event callback signal.
    glib::source::unix_fd_add_local(
        signal_rx.as_raw_fd(),
        glib::IOCondition::IN | glib::IOCondition::HUP | glib::IOCondition::ERR,
        {
            let container = container.clone();
            let output_filter = output_filter.clone();
            move |_, condition| {
                if condition.intersects(glib::IOCondition::HUP | glib::IOCondition::ERR) {
                    if workspace_debug_enabled() {
                        eprintln!("vibar/workspaces: event signal pipe closed");
                    }
                    return ControlFlow::Break;
                }

                let mut had_event = false;
                let mut buf = [0_u8; 64];
                loop {
                    match signal_rx.read(&mut buf) {
                        Ok(0) => {
                            if workspace_debug_enabled() {
                                eprintln!("vibar/workspaces: event signal pipe reached EOF");
                            }
                            return ControlFlow::Break;
                        }
                        Ok(_) => had_event = true,
                        Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => break,
                        Err(err) => {
                            eprintln!("vibar/workspaces: failed to read event signal pipe: {err}");
                            return ControlFlow::Break;
                        }
                    }
                }

                if had_event {
                    refresh_workspaces(&container, output_filter.as_deref());
                }
                ControlFlow::Continue
            }
        },
    );

    container
}

fn start_workspace_event_listener(mut signal_tx: std::os::unix::net::UnixStream) {
    std::thread::spawn(move || loop {
        let connection = match Connection::new() {
            Ok(conn) => conn,
            Err(err) => {
                if workspace_debug_enabled() {
                    eprintln!("vibar/workspaces: failed to connect for events: {err}");
                }
                std::thread::sleep(std::time::Duration::from_millis(500));
                continue;
            }
        };

        let stream = match connection.subscribe([EventType::Workspace, EventType::Output]) {
            Ok(stream) => stream,
            Err(err) => {
                if workspace_debug_enabled() {
                    eprintln!("vibar/workspaces: failed to subscribe to events: {err}");
                }
                std::thread::sleep(std::time::Duration::from_millis(500));
                continue;
            }
        };

        for event in stream {
            if workspace_debug_enabled() {
                eprintln!("vibar/workspaces: event={event:?}");
            }
            if signal_tx.write_all(&[1]).is_err() {
                return;
            }
        }

        if workspace_debug_enabled() {
            eprintln!("vibar/workspaces: event stream ended, reconnecting");
        }
        std::thread::sleep(std::time::Duration::from_millis(200));
    });
}

fn refresh_workspaces(container: &GtkBox, output_filter: Option<&str>) {
    while let Some(child) = container.first_child() {
        container.remove(&child);
    }

    let mut connection = match Connection::new() {
        Ok(conn) => conn,
        Err(_) => {
            let fallback = Label::new(Some("sway?"));
            fallback.add_css_class("workspace-status");
            container.append(&fallback);
            return;
        }
    };

    let mut workspaces = match connection.get_workspaces() {
        Ok(items) => items,
        Err(_) => {
            let fallback = Label::new(Some("sway?"));
            fallback.add_css_class("workspace-status");
            container.append(&fallback);
            return;
        }
    };

    if let Some(output) = output_filter {
        workspaces.retain(|ws| ws.output == output);
    }
    workspaces.sort_by_key(|w| w.num);

    let focused_workspace_from_list = workspaces
        .iter()
        .find(|ws| ws.focused)
        .map(|ws| ws.name.clone());

    let focused_workspace_from_tree = focused_workspace_name_from_tree(&mut connection);
    let focused_workspace = focused_workspace_from_tree
        .clone()
        .or_else(|| focused_workspace_from_list.clone());

    if workspace_debug_enabled() {
        eprintln!(
            "vibar/workspaces: output_filter={:?} focused(tree)={:?} focused(list)={:?} all=[{}]",
            output_filter,
            focused_workspace_from_tree,
            focused_workspace_from_list,
            workspaces
                .iter()
                .map(|ws| format!(
                    "{{name={},num={},focused={},visible={},output={}}}",
                    ws.name, ws.num, ws.focused, ws.visible, ws.output
                ))
                .collect::<Vec<_>>()
                .join(", ")
        );
    }

    for ws in workspaces {
        let button = Button::with_label(&ws.name);
        button.add_css_class("menu-button");
        button.set_focusable(false);

        if focused_workspace
            .as_ref()
            .is_some_and(|active_name| active_name == &ws.name)
        {
            button.add_css_class("active");
            button.add_css_class("workspace-active");
        }

        let ws_name = ws.name.clone();
        button.connect_clicked(move |_| {
            let _ = Command::new("swaymsg")
                .arg("workspace")
                .arg(ws_name.clone())
                .output();
        });

        container.append(&button);
    }
}

fn focused_workspace_name_from_tree(connection: &mut Connection) -> Option<String> {
    let tree = connection.get_tree().ok()?;
    focused_workspace_name_in_node(&tree)
}

fn focused_workspace_name_in_node(node: &swayipc::Node) -> Option<String> {
    focused_workspace_name_in_node_with_context(node, None)
}

fn focused_workspace_name_in_node_with_context(
    node: &swayipc::Node,
    current_workspace: Option<&str>,
) -> Option<String> {
    let workspace_ctx = if node.node_type == swayipc::NodeType::Workspace {
        node.name.as_deref().or(current_workspace)
    } else {
        current_workspace
    };

    if node.focused {
        return workspace_ctx.map(ToOwned::to_owned);
    }

    for child in &node.nodes {
        if let Some(name) = focused_workspace_name_in_node_with_context(child, workspace_ctx) {
            return Some(name);
        }
    }

    for child in &node.floating_nodes {
        if let Some(name) = focused_workspace_name_in_node_with_context(child, workspace_ctx) {
            return Some(name);
        }
    }

    None
}

fn workspace_debug_enabled() -> bool {
    std::env::var("VIBAR_DEBUG_WORKSPACES")
        .map(|v| matches!(v.to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on"))
        .unwrap_or(false)
}
