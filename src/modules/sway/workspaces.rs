use std::process::Command;
use std::sync::{Arc, OnceLock};
use std::{cell::RefCell, rc::Rc};

use gtk::gdk;
use gtk::prelude::*;
use gtk::{Box as GtkBox, Button, Label, Orientation, Widget};
use serde::Deserialize;
use serde_json::{Map, Value};
use swayipc::{Connection, EventType};

use crate::modules::broadcaster::{BackendRegistry, Broadcaster};
use crate::modules::{
    apply_css_classes, poll_receiver_widget, ModuleBuildContext, ModuleConfig, ModuleFactory,
};

#[derive(Debug, Deserialize, Clone, Default)]
pub(crate) struct WorkspaceConfig {
    #[serde(default)]
    pub(crate) class: Option<String>,
    #[serde(rename = "button-class", alias = "button_class", default)]
    pub(crate) button_class: Option<String>,
}

#[derive(Debug, Clone)]
struct WorkspacesUpdate {
    workspaces: Vec<WorkspaceInfo>,
    focused_workspace: Option<String>,
}

#[derive(Debug, Clone)]
struct WorkspaceInfo {
    name: String,
    num: i32,
    output: String,
    focused: bool,
    visible: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct WorkspacesSharedKey {}

pub(crate) struct SwayWorkspaceFactory;

pub(crate) const FACTORY: SwayWorkspaceFactory = SwayWorkspaceFactory;
pub(crate) const MODULE_TYPE: &str = "sway/workspaces";

impl ModuleFactory for SwayWorkspaceFactory {
    fn module_type(&self) -> &'static str {
        MODULE_TYPE
    }

    fn init(&self, config: &ModuleConfig, context: &ModuleBuildContext) -> Result<Widget, String> {
        let parsed = parse_config(config)?;
        Ok(build_workspaces_module(
            context.monitor_connector.clone(),
            context.monitor.clone(),
            parsed.class,
            parsed.button_class,
        )
        .upcast())
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

fn workspaces_registry(
) -> &'static BackendRegistry<WorkspacesSharedKey, Broadcaster<WorkspacesUpdate>> {
    static REGISTRY: OnceLock<BackendRegistry<WorkspacesSharedKey, Broadcaster<WorkspacesUpdate>>> =
        OnceLock::new();
    REGISTRY.get_or_init(BackendRegistry::new)
}

fn subscribe_shared_workspaces() -> std::sync::mpsc::Receiver<WorkspacesUpdate> {
    let key = WorkspacesSharedKey {};

    let (broadcaster, start_worker) =
        workspaces_registry().get_or_create(key.clone(), Broadcaster::new);
    let receiver = broadcaster.subscribe();

    if start_worker {
        start_workspaces_worker(key, broadcaster);
    }

    receiver
}

fn start_workspaces_worker(
    key: WorkspacesSharedKey,
    broadcaster: Arc<Broadcaster<WorkspacesUpdate>>,
) {
    std::thread::spawn(move || {
        broadcaster.broadcast(query_workspaces());

        loop {
            if broadcaster.subscriber_count() == 0 {
                workspaces_registry().remove(&key, &broadcaster);
                return;
            }

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
                if broadcaster.subscriber_count() == 0 {
                    workspaces_registry().remove(&key, &broadcaster);
                    return;
                }
                broadcaster.broadcast(query_workspaces());
            }

            if workspace_debug_enabled() {
                eprintln!("vibar/workspaces: event stream ended, reconnecting");
            }
            std::thread::sleep(std::time::Duration::from_millis(200));
        }
    });
}

fn query_workspaces() -> WorkspacesUpdate {
    let mut connection = match Connection::new() {
        Ok(conn) => conn,
        Err(_) => {
            return WorkspacesUpdate {
                workspaces: Vec::new(),
                focused_workspace: None,
            };
        }
    };

    let workspaces = match connection.get_workspaces() {
        Ok(items) => items,
        Err(_) => {
            return WorkspacesUpdate {
                workspaces: Vec::new(),
                focused_workspace: None,
            };
        }
    };

    let focused_from_list = workspaces
        .iter()
        .find(|ws| ws.focused)
        .map(|ws| ws.name.clone());

    let focused_from_tree = focused_workspace_name_from_tree(&mut connection);
    let focused_workspace = focused_from_tree.or(focused_from_list);

    let infos = workspaces
        .into_iter()
        .map(|ws| WorkspaceInfo {
            name: ws.name,
            num: ws.num,
            output: ws.output,
            focused: ws.focused,
            visible: ws.visible,
        })
        .collect();

    WorkspacesUpdate {
        workspaces: infos,
        focused_workspace,
    }
}

pub(crate) fn build_workspaces_module(
    output_filter: Option<String>,
    monitor: Option<gdk::Monitor>,
    class: Option<String>,
    button_class: Option<String>,
) -> GtkBox {
    let resolved_output = Rc::new(RefCell::new(output_filter));
    try_resolve_output_filter(&resolved_output, monitor.as_ref());

    let container = GtkBox::new(Orientation::Horizontal, 4);
    container.add_css_class("module");
    container.add_css_class("workspaces");
    apply_css_classes(&container, class.as_deref());

    let receiver = subscribe_shared_workspaces();

    // Initial render
    {
        let output = resolved_output.borrow().clone();
        if output.is_some() {
            container.set_visible(true);
        } else {
            container.set_visible(false);
        }
    }

    poll_receiver_widget(&container, receiver, {
        let resolved_output = Rc::clone(&resolved_output);
        let monitor = monitor.clone();
        move |container, update| {
            if resolved_output.borrow().is_none() {
                try_resolve_output_filter(&resolved_output, monitor.as_ref());
            }
            let output = resolved_output.borrow().clone();
            if output.is_none() {
                container.set_visible(false);
                return;
            }
            render_workspaces(
                container,
                &update,
                output.as_deref(),
                button_class.as_deref(),
            );
            container.set_visible(true);
        }
    });

    // Deferred output resolution for monitors that aren't ready yet
    gtk::glib::timeout_add_local(std::time::Duration::from_millis(200), {
        let container_weak = container.downgrade();
        let resolved_output = Rc::clone(&resolved_output);
        move || {
            let Some(container) = container_weak.upgrade() else {
                return gtk::glib::ControlFlow::Break;
            };

            if resolved_output.borrow().is_none() {
                try_resolve_output_filter(&resolved_output, monitor.as_ref());
                if resolved_output.borrow().is_some() {
                    container.set_visible(true);
                }
            }

            gtk::glib::ControlFlow::Break
        }
    });

    container
}

fn try_resolve_output_filter(
    resolved_output: &Rc<RefCell<Option<String>>>,
    monitor: Option<&gdk::Monitor>,
) {
    if resolved_output.borrow().is_some() {
        return;
    }
    if let Some(connector) = monitor.and_then(|item| item.connector()) {
        *resolved_output.borrow_mut() = Some(connector.to_string());
    }
}

fn render_workspaces(
    container: &GtkBox,
    update: &WorkspacesUpdate,
    output_filter: Option<&str>,
    button_class: Option<&str>,
) {
    while let Some(child) = container.first_child() {
        container.remove(&child);
    }

    if update.workspaces.is_empty() {
        let fallback = Label::new(Some("sway?"));
        fallback.add_css_class("workspace-status");
        container.append(&fallback);
        return;
    }

    let mut workspaces: Vec<_> = update.workspaces.clone();
    if let Some(output) = output_filter {
        workspaces.retain(|ws| ws.output == output);
    }
    workspaces.sort_by_key(|w| w.num);

    if workspace_debug_enabled() {
        eprintln!(
            "vibar/workspaces: output_filter={:?} focused={:?} all=[{}]",
            output_filter,
            update.focused_workspace,
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
        apply_css_classes(&button, button_class);
        button.set_focusable(false);

        if update
            .focused_workspace
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

#[cfg(test)]
mod tests {
    use serde_json::{json, Map};

    use super::*;

    #[test]
    fn parse_config_rejects_wrong_module_type() {
        let module = ModuleConfig::new("clock", Map::new());
        let err = parse_config(&module).expect_err("wrong type should fail");
        assert!(err.contains("expected module type 'sway/workspaces'"));
    }

    #[test]
    fn parse_config_supports_button_class_aliases() {
        let kebab = ModuleConfig::new(
            MODULE_TYPE,
            serde_json::from_value(json!({
                "button-class": "foo bar"
            }))
            .expect("module config map should parse"),
        );
        let kebab_cfg = parse_config(&kebab).expect("kebab config should parse");
        assert_eq!(kebab_cfg.button_class.as_deref(), Some("foo bar"));

        let snake = ModuleConfig::new(
            MODULE_TYPE,
            serde_json::from_value(json!({
                "button_class": "baz"
            }))
            .expect("module config map should parse"),
        );
        let snake_cfg = parse_config(&snake).expect("snake config should parse");
        assert_eq!(snake_cfg.button_class.as_deref(), Some("baz"));
    }
}
