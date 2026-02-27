use std::sync::{Arc, OnceLock};

use gtk::prelude::*;
use gtk::{Label, Widget};
use serde::Deserialize;
use serde_json::Value;
use swayipc::{Connection, EventType, Node, NodeType};

use crate::modules::broadcaster::{
    attach_subscription, BackendRegistry, Broadcaster, Subscription,
};
use crate::modules::{
    apply_css_classes, attach_primary_click_command, escape_markup_text, render_markup_template,
    ModuleBuildContext, ModuleConfig, ModuleFactory,
};

#[derive(Debug, Deserialize, Clone, Default)]
pub(crate) struct WindowConfig {
    #[serde(default = "default_format")]
    pub(crate) format: String,
    #[serde(default)]
    pub(crate) click: Option<String>,
    #[serde(rename = "on-click", default)]
    pub(crate) on_click: Option<String>,
    #[serde(default)]
    pub(crate) class: Option<String>,
}

#[derive(Debug, Clone)]
struct WindowUpdate {
    title: String,
    output: Option<String>,
    visible: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct WindowSharedKey {
    format: String,
}

pub(crate) struct SwayWindowFactory;

pub(crate) const FACTORY: SwayWindowFactory = SwayWindowFactory;
pub(crate) const MODULE_TYPE: &str = "sway/window";

fn default_format() -> String {
    "{}".to_string()
}

impl ModuleFactory for SwayWindowFactory {
    fn module_type(&self) -> &'static str {
        MODULE_TYPE
    }

    fn init(&self, config: &ModuleConfig, context: &ModuleBuildContext) -> Result<Widget, String> {
        let parsed = parse_config(config)?;
        let click_command = parsed.click.or(parsed.on_click);
        Ok(build_window_module(
            context.monitor_connector.clone(),
            parsed.format,
            click_command,
            parsed.class,
        )
        .upcast())
    }
}

fn parse_config(module: &ModuleConfig) -> Result<WindowConfig, String> {
    if module.module_type != MODULE_TYPE {
        return Err(format!(
            "expected module type '{}', got '{}'",
            MODULE_TYPE, module.module_type
        ));
    }

    serde_json::from_value(Value::Object(module.config.clone()))
        .map_err(|err| format!("invalid {} module config: {err}", MODULE_TYPE))
}

fn window_registry() -> &'static BackendRegistry<WindowSharedKey, Broadcaster<WindowUpdate>> {
    static REGISTRY: OnceLock<BackendRegistry<WindowSharedKey, Broadcaster<WindowUpdate>>> =
        OnceLock::new();
    REGISTRY.get_or_init(BackendRegistry::new)
}

fn subscribe_shared_window(format: String) -> Subscription<WindowUpdate> {
    let key = WindowSharedKey {
        format: format.clone(),
    };

    let (broadcaster, start_worker) =
        window_registry().get_or_create(key.clone(), Broadcaster::new);
    let receiver = broadcaster.subscribe();

    if start_worker {
        start_window_worker(key, broadcaster);
    }

    receiver
}

fn start_window_worker(key: WindowSharedKey, broadcaster: Arc<Broadcaster<WindowUpdate>>) {
    std::thread::spawn(move || {
        broadcaster.broadcast(query_focused_window(&key.format));

        loop {
            if broadcaster.subscriber_count() == 0 {
                window_registry().remove(&key, &broadcaster);
                return;
            }

            let connection = match Connection::new() {
                Ok(conn) => conn,
                Err(_) => {
                    std::thread::sleep(std::time::Duration::from_millis(500));
                    continue;
                }
            };

            let stream = match connection.subscribe([
                EventType::Window,
                EventType::Workspace,
                EventType::Output,
            ]) {
                Ok(stream) => stream,
                Err(_) => {
                    std::thread::sleep(std::time::Duration::from_millis(500));
                    continue;
                }
            };

            for _ in stream {
                if broadcaster.subscriber_count() == 0 {
                    window_registry().remove(&key, &broadcaster);
                    return;
                }
                broadcaster.broadcast(query_focused_window(&key.format));
            }

            std::thread::sleep(std::time::Duration::from_millis(200));
        }
    });
}

fn query_focused_window(format: &str) -> WindowUpdate {
    let mut connection = match Connection::new() {
        Ok(conn) => conn,
        Err(_) => {
            return WindowUpdate {
                title: escape_markup_text("sway?"),
                output: None,
                visible: true,
            };
        }
    };

    let tree = match connection.get_tree() {
        Ok(tree) => tree,
        Err(_) => {
            return WindowUpdate {
                title: escape_markup_text("sway?"),
                output: None,
                visible: true,
            };
        }
    };

    let focused = focused_window_info(&tree);
    let output = focused.as_ref().and_then(|info| info.output.clone());
    let title = focused.and_then(|info| info.title).unwrap_or_default();

    if title.is_empty() {
        return WindowUpdate {
            title: String::new(),
            output,
            visible: false,
        };
    }

    let rendered = render_markup_template(format, &[("{}", &title), ("{title}", &title)]);
    let visible = !rendered.trim().is_empty();
    WindowUpdate {
        title: rendered,
        output,
        visible,
    }
}

fn build_window_module(
    output_filter: Option<String>,
    format: String,
    click_command: Option<String>,
    class: Option<String>,
) -> Label {
    let label = Label::new(None);
    label.add_css_class("module");
    label.add_css_class("sway-window");
    label.set_ellipsize(gtk::pango::EllipsizeMode::End);
    label.set_max_width_chars(80);
    apply_css_classes(&label, class.as_deref());
    attach_primary_click_command(&label, click_command);

    let subscription = subscribe_shared_window(format);

    attach_subscription(&label, subscription, move |label, update| {
        let belongs_to_output = match (output_filter.as_deref(), update.output.as_deref()) {
            (Some(expected), Some(current)) => expected == current,
            (Some(_), None) => false,
            (None, _) => true,
        };

        if !belongs_to_output || !update.visible {
            label.set_visible(false);
            return;
        }

        label.set_visible(true);
        label.set_markup(&update.title);
    });

    label
}

#[derive(Debug, Clone)]
struct FocusedWindowInfo {
    title: Option<String>,
    output: Option<String>,
}

fn focused_window_info(root: &Node) -> Option<FocusedWindowInfo> {
    focused_window_info_in_node(root, None)
}

fn focused_window_info_in_node(
    node: &Node,
    current_output: Option<&str>,
) -> Option<FocusedWindowInfo> {
    let output_ctx = if node.node_type == NodeType::Output {
        node.name.as_deref().or(current_output)
    } else {
        current_output
    };

    for child in &node.nodes {
        if let Some(info) = focused_window_info_in_node(child, output_ctx) {
            return Some(info);
        }
    }

    for child in &node.floating_nodes {
        if let Some(info) = focused_window_info_in_node(child, output_ctx) {
            return Some(info);
        }
    }

    if !node.focused {
        return None;
    }

    let title = match node.node_type {
        NodeType::Workspace | NodeType::Output | NodeType::Root => None,
        _ => node.name.clone(),
    };

    Some(FocusedWindowInfo {
        title,
        output: output_ctx.map(ToOwned::to_owned),
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
        assert!(err.contains("expected module type 'sway/window'"));
    }

    #[test]
    fn parse_config_supports_click_aliases() {
        let click_module = ModuleConfig::new(
            MODULE_TYPE,
            serde_json::from_str("{\"click\":\"echo click\"}")
                .expect("module config map should parse"),
        );
        let click_cfg = parse_config(&click_module).expect("click config should parse");
        assert_eq!(click_cfg.click.as_deref(), Some("echo click"));
        assert!(click_cfg.on_click.is_none());

        let on_click_module = ModuleConfig::new(
            MODULE_TYPE,
            serde_json::from_str("{\"on-click\":\"echo alias\"}")
                .expect("module config map should parse"),
        );
        let on_click_cfg = parse_config(&on_click_module).expect("on-click config should parse");
        assert!(on_click_cfg.click.is_none());
        assert_eq!(on_click_cfg.on_click.as_deref(), Some("echo alias"));
    }

    #[test]
    fn parse_config_has_default_format() {
        let module = ModuleConfig::new(MODULE_TYPE, Map::new());
        let cfg = parse_config(&module).expect("config should parse");
        assert_eq!(cfg.format, "{}");
    }
}
