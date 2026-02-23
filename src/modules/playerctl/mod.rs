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
    apply_css_classes, attach_primary_click_command, ModuleBuildContext, ModuleConfig,
};

use super::ModuleFactory;
use backend::{run_event_backend, BackendUpdate};
use config::{
    default_playerctl_interval, PlayerctlConfig, PlayerctlMarqueeMode, PlayerctlViewConfig,
};
use model::{render_format, should_show_metadata, status_css_class};
use ui::{
    build_carousel_ui, build_controls_ui, build_playerctl_tooltip, install_carousel_animation,
    install_carousel_hover_tracking, install_carousel_open_tracking, install_controls_open_gesture,
    refresh_controls_ui, set_playerctl_text, wire_controls_actions,
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

    let tooltip_ui = build_playerctl_tooltip(&root);

    let label = Label::new(None);
    label.set_xalign(0.0);
    label.set_focusable(false);
    label.set_wrap(false);
    label.set_single_line_mode(true);

    let carousel = config.fixed_width.map(|fixed_width| {
        root.add_css_class("playerctl-fixed-width");
        build_carousel_ui(&root, fixed_width, config.class.as_deref(), config.marquee)
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
                set_playerctl_text(&label, &tooltip_ui, carousel.as_ref(), &text);
                root.set_visible(visibility);
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
    use serde_json::{json, Map};

    use super::config::{normalize_fixed_width, PlayerctlControlsOpenMode};
    use super::model::{
        format_timestamp_micros, matches_player_filter, metadata_seek_ratio, select_active_player,
        PlayerctlMetadata,
    };
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
    fn parse_config_supports_fixed_width_keys() {
        let kebab = ModuleConfig::new(
            MODULE_TYPE,
            serde_json::from_value(json!({
                "fixed-width": 40
            }))
            .expect("playerctl config map should parse"),
        );
        let snake = ModuleConfig::new(
            MODULE_TYPE,
            serde_json::from_value(json!({
                "fixed_width": 32
            }))
            .expect("playerctl config map should parse"),
        );

        let kebab_cfg = parse_config(&kebab).expect("config should parse");
        let snake_cfg = parse_config(&snake).expect("config should parse");

        assert_eq!(kebab_cfg.fixed_width, Some(40));
        assert_eq!(snake_cfg.fixed_width, Some(32));
    }

    #[test]
    fn normalize_fixed_width_rejects_zero() {
        assert_eq!(normalize_fixed_width(0), None);
        assert_eq!(normalize_fixed_width(1), Some(1));
    }

    #[test]
    fn parse_config_defaults_marquee_to_off() {
        let module = ModuleConfig::new(MODULE_TYPE, Map::new());
        let cfg = parse_config(&module).expect("config should parse");
        assert!(matches!(cfg.marquee, PlayerctlMarqueeMode::Off));
    }

    #[test]
    fn parse_config_supports_marquee_modes() {
        let hover = ModuleConfig::new(
            MODULE_TYPE,
            serde_json::from_value(json!({
                "marquee": "hover"
            }))
            .expect("playerctl config map should parse"),
        );
        let open = ModuleConfig::new(
            MODULE_TYPE,
            serde_json::from_value(json!({
                "marquee": "open"
            }))
            .expect("playerctl config map should parse"),
        );
        let always = ModuleConfig::new(
            MODULE_TYPE,
            serde_json::from_value(json!({
                "marquee": "always"
            }))
            .expect("playerctl config map should parse"),
        );

        let hover_cfg = parse_config(&hover).expect("config should parse");
        let open_cfg = parse_config(&open).expect("config should parse");
        let always_cfg = parse_config(&always).expect("config should parse");

        assert!(matches!(hover_cfg.marquee, PlayerctlMarqueeMode::Hover));
        assert!(matches!(open_cfg.marquee, PlayerctlMarqueeMode::Open));
        assert!(matches!(always_cfg.marquee, PlayerctlMarqueeMode::Always));
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
