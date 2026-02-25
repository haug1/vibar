use gtk::prelude::*;
use gtk::{
    Align, ArrowType, Box as GtkBox, Label, MenuButton, Orientation, Popover, PositionType, Widget,
};
use serde::de::Deserializer;
use serde::Deserialize;
use serde_json::Value;

use crate::modules::{
    apply_css_classes, build_module, ModuleBuildContext, ModuleConfig, ModuleFactory,
};

#[derive(Debug, Deserialize, Clone)]
pub(crate) struct GroupConfig {
    #[serde(default, alias = "children")]
    pub(crate) modules: Vec<ModuleConfig>,
    #[serde(default)]
    pub(crate) class: Option<String>,
    #[serde(default = "default_spacing")]
    pub(crate) spacing: i32,
    #[serde(default, deserialize_with = "deserialize_drawer")]
    pub(crate) drawer: Option<GroupDrawerConfig>,
}

#[derive(Debug, Deserialize, Clone)]
pub(crate) struct GroupDrawerConfig {
    #[serde(
        rename = "label-closed",
        alias = "label_closed",
        default = "default_drawer_label_closed"
    )]
    pub(crate) label_closed: String,
    #[serde(
        rename = "label-open",
        alias = "label_open",
        default = "default_drawer_label_open"
    )]
    pub(crate) label_open: String,
    #[serde(rename = "start-open", alias = "start_open", default)]
    pub(crate) start_open: bool,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(untagged)]
pub(crate) enum GroupDrawerInput {
    Enabled(bool),
    Config(GroupDrawerConfig),
}

pub(crate) struct GroupFactory;

pub(crate) const FACTORY: GroupFactory = GroupFactory;
pub(crate) const MODULE_TYPE: &str = "group";

impl ModuleFactory for GroupFactory {
    fn module_type(&self) -> &'static str {
        MODULE_TYPE
    }

    fn init(&self, config: &ModuleConfig, context: &ModuleBuildContext) -> Result<Widget, String> {
        let parsed = parse_config(config)?;
        build_group_module(parsed, context).map(|widget| widget.upcast())
    }
}

pub(crate) fn parse_config(module: &ModuleConfig) -> Result<GroupConfig, String> {
    if module.module_type != MODULE_TYPE {
        return Err(format!(
            "expected module type '{}', got '{}'",
            MODULE_TYPE, module.module_type
        ));
    }

    let config: GroupConfig = serde_json::from_value(Value::Object(module.config.clone()))
        .map_err(|err| format!("invalid {} module config: {err}", MODULE_TYPE))?;
    if config.modules.is_empty() {
        return Err("invalid group module config: field `modules` must not be empty".to_string());
    }
    Ok(config)
}

fn build_group_module(config: GroupConfig, context: &ModuleBuildContext) -> Result<GtkBox, String> {
    let spacing = normalized_spacing(config.spacing);
    let container = GtkBox::new(Orientation::Horizontal, spacing);
    container.add_css_class("module");
    container.add_css_class("group");
    container.set_focusable(false);
    container.set_focus_on_click(false);

    apply_css_classes(&container, config.class.as_deref());

    let drawer_enabled = config.drawer.is_some();
    let child_orientation = if config.drawer.is_some() {
        Orientation::Vertical
    } else {
        Orientation::Horizontal
    };
    let child_container = GtkBox::new(child_orientation, spacing);
    child_container.add_css_class("group-content");
    child_container.set_focusable(false);
    child_container.set_focus_on_click(false);

    for (idx, child_config) in config.modules.iter().enumerate() {
        let widget = build_module(child_config, context)
            .map_err(|err| format!("invalid child module at index {idx}: {err}"))?;
        if drawer_enabled {
            widget.set_halign(Align::Fill);
            widget.set_hexpand(true);
        }
        child_container.append(&widget);
    }

    if let Some(drawer) = config.drawer {
        container.add_css_class("group-drawer");

        let toggle = MenuButton::new();
        toggle.add_css_class("group-toggle");
        toggle.set_focusable(false);
        toggle.set_direction(ArrowType::Up);
        let toggle_label = Label::new(Some(if drawer.start_open {
            &drawer.label_open
        } else {
            &drawer.label_closed
        }));
        toggle_label.set_focusable(false);
        toggle.set_property("child", &toggle_label);

        let popover = Popover::new();
        popover.set_autohide(true);
        popover.set_has_arrow(true);
        popover.set_position(PositionType::Top);
        popover.add_css_class("group-popover");
        popover.set_child(Some(&child_container));
        toggle.set_popover(Some(&popover));

        let open_label = drawer.label_open;
        let closed_label = drawer.label_closed;
        let label_for_show = toggle_label.clone();
        let open_label_for_show = open_label.clone();
        popover.connect_show(move |popover| {
            popover.set_position(PositionType::Top);
            label_for_show.set_text(open_label_for_show.as_str());
        });
        let label_for_hide = toggle_label.clone();
        let closed_label_for_hide = closed_label.clone();
        popover.connect_hide(move |_| {
            label_for_hide.set_text(closed_label_for_hide.as_str());
        });

        container.append(&toggle);
        if drawer.start_open {
            popover.popup();
            toggle_label.set_text(open_label.as_str());
        }
    } else {
        container.append(&child_container);
    }

    Ok(container)
}

fn default_spacing() -> i32 {
    6
}

fn default_drawer_label_closed() -> String {
    "".to_string()
}

fn default_drawer_label_open() -> String {
    "".to_string()
}

impl Default for GroupDrawerConfig {
    fn default() -> Self {
        Self {
            label_closed: default_drawer_label_closed(),
            label_open: default_drawer_label_open(),
            start_open: false,
        }
    }
}

fn deserialize_drawer<'de, D>(deserializer: D) -> Result<Option<GroupDrawerConfig>, D::Error>
where
    D: Deserializer<'de>,
{
    let raw = Option::<GroupDrawerInput>::deserialize(deserializer)?;
    match raw {
        Some(GroupDrawerInput::Enabled(true)) => Ok(Some(GroupDrawerConfig::default())),
        Some(GroupDrawerInput::Enabled(false)) => Ok(None),
        Some(GroupDrawerInput::Config(drawer)) => Ok(Some(drawer)),
        None => Ok(None),
    }
}

fn normalized_spacing(spacing: i32) -> i32 {
    spacing.max(0)
}

#[cfg(test)]
mod tests {
    use serde_json::{json, Map};

    use super::*;

    #[test]
    fn parse_config_rejects_wrong_module_type() {
        let module = ModuleConfig::new("clock", Map::new());
        let err = parse_config(&module).expect_err("wrong type should fail");
        assert!(err.contains("expected module type 'group'"));
    }

    #[test]
    fn parse_config_requires_modules() {
        let module = ModuleConfig::new(MODULE_TYPE, Map::new());
        let err = parse_config(&module).expect_err("missing modules should fail");
        assert!(err.contains("field `modules` must not be empty"));
    }

    #[test]
    fn parse_config_supports_drawer_fields() {
        let module = ModuleConfig::new(
            MODULE_TYPE,
            serde_json::from_value(json!({
                "modules": [{ "type": "clock" }],
                "drawer": {
                    "label-closed": ">",
                    "label-open": "v",
                    "start-open": true
                }
            }))
            .expect("group config map should parse"),
        );
        let cfg = parse_config(&module).expect("group config should parse");
        let drawer = cfg.drawer.expect("drawer should parse");
        assert_eq!(drawer.label_closed, ">");
        assert_eq!(drawer.label_open, "v");
        assert!(drawer.start_open);
    }

    #[test]
    fn parse_config_supports_boolean_drawer() {
        let module = ModuleConfig::new(
            MODULE_TYPE,
            serde_json::from_value(json!({
                "modules": [{ "type": "clock" }],
                "drawer": true
            }))
            .expect("group config map should parse"),
        );
        let cfg = parse_config(&module).expect("group config should parse");
        assert!(cfg.drawer.is_some());
    }

    #[test]
    fn parse_config_supports_children_alias() {
        let module = ModuleConfig::new(
            MODULE_TYPE,
            serde_json::from_value(json!({
                "children": [{ "type": "clock" }]
            }))
            .expect("group config map should parse"),
        );
        let cfg = parse_config(&module).expect("group config should parse");
        assert_eq!(cfg.modules.len(), 1);
    }
}
