mod backend;
mod config;
mod model;
mod ui;

use std::sync::mpsc;
use std::time::Duration;

use glib::ControlFlow;
use gtk::prelude::*;
use gtk::{Label, Overlay, Widget};
use serde_json::Value;

use crate::modules::{
    apply_css_classes, attach_primary_click_command, escape_markup_text, ModuleBuildContext,
    ModuleConfig,
};

use super::ModuleFactory;
use backend::run_event_backend;
use config::{
    default_playerctl_interval, PlayerctlConfig, PlayerctlMarqueeMode, PlayerctlViewConfig,
};
use model::{
    render_format, render_markup_format, should_show_metadata, status_css_class, BackendUpdate,
};
use ui::{
    build_carousel_ui, build_controls_ui, build_playerctl_tooltip, install_carousel_animation,
    install_carousel_hover_tracking, install_carousel_open_tracking, install_controls_open_gesture,
    refresh_controls_ui, set_playerctl_text, sync_controls_width, wire_controls_actions,
};

const PLAYERCTL_STATE_CLASSES: [&str; 4] = [
    "status-playing",
    "status-paused",
    "status-stopped",
    "no-player",
];
pub(crate) const MODULE_TYPE: &str = "playerctl";

pub(crate) struct PlayerctlFactory;

pub(crate) const FACTORY: PlayerctlFactory = PlayerctlFactory;

impl ModuleFactory for PlayerctlFactory {
    fn module_type(&self) -> &'static str {
        MODULE_TYPE
    }

    fn init(&self, config: &ModuleConfig, _context: &ModuleBuildContext) -> Result<Widget, String> {
        let parsed = parse_config(config)?;
        Ok(build_playerctl_module(parsed.into_view()).upcast())
    }
}

fn parse_config(module: &ModuleConfig) -> Result<PlayerctlConfig, String> {
    if module.module_type != MODULE_TYPE {
        return Err(format!(
            "expected module type '{}', got '{}'",
            MODULE_TYPE, module.module_type
        ));
    }

    serde_json::from_value(Value::Object(module.config.clone()))
        .map_err(|err| format!("invalid {} module config: {err}", MODULE_TYPE))
}

fn build_playerctl_module(config: PlayerctlViewConfig) -> Overlay {
    let root = Overlay::new();
    root.add_css_class("module");
    root.add_css_class("playerctl");
    root.set_focusable(false);
    root.set_focus_on_click(false);

    apply_css_classes(&root, config.class.as_deref());
    root.set_tooltip_text(None);

    let label = Label::new(None);
    label.set_xalign(0.0);
    label.set_focusable(false);
    label.set_wrap(false);
    label.set_single_line_mode(true);

    let carousel = config.max_width.map(|max_width| {
        root.add_css_class("playerctl-max-width");
        build_carousel_ui(&root, max_width, config.class.as_deref(), config.marquee)
    });
    if let Some(carousel) = &carousel {
        root.set_child(Some(&carousel.area));
    } else {
        root.set_child(Some(&label));
    }

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
    let tooltip_ui = build_playerctl_tooltip(&root, controls_ui.as_ref().map(|ui| &ui.popover));

    if config.interval_secs != default_playerctl_interval() {
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
        let carousel = carousel.clone();
        let tooltip_ui = tooltip_ui.clone();
        move || {
            while let Ok(update) = receiver.try_recv() {
                let (plain_text, markup_text, visibility, state_class) = match update {
                    BackendUpdate::Snapshot(Some(metadata)) => {
                        let plain_text = render_format(&format, &metadata);
                        let markup_text = render_markup_format(&format, &metadata);
                        if let Some(controls) = &controls_ui {
                            refresh_controls_ui(controls, Some(&metadata), "");
                        }
                        (
                            plain_text,
                            markup_text,
                            should_show_metadata(Some(&metadata), hide_when_idle, show_when_paused),
                            status_css_class(&metadata.status),
                        )
                    }
                    BackendUpdate::Snapshot(None) => {
                        let plain_text = no_player_text.clone();
                        let markup_text = escape_markup_text(&plain_text);
                        if let Some(controls) = &controls_ui {
                            refresh_controls_ui(controls, None, &plain_text);
                        }
                        (
                            plain_text,
                            markup_text,
                            should_show_metadata(None, hide_when_idle, show_when_paused),
                            "no-player",
                        )
                    }
                    BackendUpdate::Error(err) => {
                        let plain_text = format!("playerctl error: {err}");
                        let markup_text = escape_markup_text(&plain_text);
                        if let Some(controls) = &controls_ui {
                            refresh_controls_ui(controls, None, &plain_text);
                        }
                        (plain_text, markup_text, true, "no-player")
                    }
                };
                set_playerctl_text(
                    &label,
                    &tooltip_ui,
                    carousel.as_ref(),
                    &plain_text,
                    &markup_text,
                );
                if let Some(controls) = &controls_ui {
                    let width = root
                        .width_request()
                        .max(root.allocated_width())
                        .max(label.allocated_width())
                        .max(1);
                    sync_controls_width(controls, width);
                }
                root.set_visible(visibility && !plain_text.trim().is_empty());
                apply_state_class(&root, state_class);
            }
            ControlFlow::Continue
        }
    });

    if let Some(carousel) = &carousel {
        if matches!(carousel.marquee, PlayerctlMarqueeMode::Hover) {
            install_carousel_hover_tracking(&root, carousel);
        }
        if matches!(carousel.marquee, PlayerctlMarqueeMode::Open) {
            if let Some(controls) = &controls_ui {
                install_carousel_open_tracking(&controls.popover, carousel);
            }
        }
        install_carousel_animation(carousel.clone());
    }

    if let Some(controls) = controls_ui {
        wire_controls_actions(controls);
    }

    root
}

fn apply_state_class(widget: &impl IsA<Widget>, active_class: &str) {
    for class_name in PLAYERCTL_STATE_CLASSES {
        widget.remove_css_class(class_name);
    }
    widget.add_css_class(active_class);
}

#[cfg(test)]
mod tests {
    use serde_json::Map;

    use super::*;

    #[test]
    fn parse_config_rejects_wrong_module_type() {
        let module = ModuleConfig::new("clock", Map::new());
        let err = parse_config(&module).expect_err("wrong type should fail");
        assert!(err.contains("expected module type 'playerctl'"));
    }
}
