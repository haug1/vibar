use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, OnceLock};

use gtk::prelude::*;
use gtk::{EventControllerScroll, EventControllerScrollFlags, Label, Widget};
use libpulse_binding as pulse;
#[cfg(test)]
use pulse::context::subscribe::Facility;

use crate::modules::broadcaster::{
    attach_subscription, BackendRegistry, Broadcaster, Subscription,
};
use crate::modules::{
    apply_css_classes, attach_primary_click_command, attach_secondary_click_command,
    render_markup_template, ModuleBuildContext, ModuleConfig,
};

use super::ModuleFactory;

mod backend;
mod config;
mod format;
mod ui;

use self::backend::run_native_loop;
#[cfg(test)]
use self::backend::{is_relevant_pulse_event, percent_to_volume_delta};
use self::config::{
    parse_config, PulseAudioConfig, PulseAudioControlsOpenMode, PulseAudioFormatIcons,
    DEFAULT_FORMAT, DEFAULT_FORMAT_BLUETOOTH, DEFAULT_FORMAT_BLUETOOTH_MUTED, DEFAULT_FORMAT_MUTED,
    DEFAULT_FORMAT_SOURCE, DEFAULT_FORMAT_SOURCE_MUTED,
};
#[cfg(test)]
use self::format::classify_icon_kind_by_priority;
#[cfg(test)]
use self::format::volume_icon_from_list;
use self::format::IconKind;
use self::ui::{build_controls_ui, refresh_controls_ui};

const MAINLOOP_IDLE_SLEEP_MILLIS: u64 = 10;
const SESSION_RECONNECT_DELAY_SECS: u64 = 2;
const ICON_MUTED: &str = "";
const CONTROLS_UI_MAX_PERCENT: f64 = 150.0;
pub(crate) const MODULE_TYPE: &str = "pulseaudio";

#[derive(Debug, Clone)]
struct PulseState {
    volume: u32,
    muted: bool,
    source_muted: bool,
    bluetooth: bool,
    icon_kind: IconKind,
}

#[derive(Debug, Clone)]
struct AudioControlsState {
    sink_name: String,
    sinks: Vec<SinkDeviceEntry>,
    selected_sink_name: String,
    sink_volume: u32,
    sink_muted: bool,
    sink_ports: Vec<SinkPortEntry>,
    active_sink_port: Option<String>,
    sink_inputs: Vec<SinkInputEntry>,
}

#[derive(Debug, Clone)]
struct SinkPortEntry {
    name: String,
    description: String,
    available: pulse::def::PortAvailable,
}

#[derive(Debug, Clone)]
struct SinkDeviceEntry {
    name: String,
    description: String,
    available: bool,
    is_default: bool,
}

#[derive(Debug, Clone)]
struct SinkInputEntry {
    index: u32,
    name: String,
    volume: u32,
    muted: bool,
}

#[derive(Debug, Clone)]
enum WorkerCommand {
    VolumeStep {
        increase: bool,
        step: f64,
    },
    SetSinkMute {
        muted: bool,
    },
    SetSinkVolumePercent {
        percent: u32,
    },
    SetSinkInputMute {
        index: u32,
        muted: bool,
    },
    SetSinkInputVolumePercent {
        index: u32,
        percent: u32,
    },
    SetDefaultSink {
        sink_name: String,
    },
    SetSinkPort {
        sink_name: String,
        port_name: String,
    },
}

#[derive(Clone)]
struct UiUpdate {
    label_text: String,
    controls: Option<AudioControlsState>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct PulseSharedKey {
    format: Option<String>,
    format_bluetooth: Option<String>,
    format_bluetooth_muted: Option<String>,
    format_muted: Option<String>,
    format_source: Option<String>,
    format_source_muted: Option<String>,
    format_icons: PulseAudioFormatIcons,
}

struct SharedPulseState {
    broadcaster: Broadcaster<UiUpdate>,
    control_tx: Mutex<Sender<WorkerCommand>>,
    control_rx: Mutex<Option<Receiver<WorkerCommand>>>,
}

fn pulse_registry() -> &'static BackendRegistry<PulseSharedKey, SharedPulseState> {
    static REGISTRY: OnceLock<BackendRegistry<PulseSharedKey, SharedPulseState>> = OnceLock::new();
    REGISTRY.get_or_init(BackendRegistry::new)
}

fn subscribe_shared_pulse(
    config: &PulseAudioConfig,
) -> (Subscription<UiUpdate>, Sender<WorkerCommand>) {
    let key = PulseSharedKey {
        format: config.format.clone(),
        format_bluetooth: config.format_bluetooth.clone(),
        format_bluetooth_muted: config.format_bluetooth_muted.clone(),
        format_muted: config.format_muted.clone(),
        format_source: config.format_source.clone(),
        format_source_muted: config.format_source_muted.clone(),
        format_icons: config.format_icons.clone(),
    };

    let render_config = config.clone();
    let (shared, start_worker) = pulse_registry().get_or_create(key.clone(), || {
        let (control_tx, control_rx) = mpsc::channel();
        SharedPulseState {
            broadcaster: Broadcaster::new(),
            control_tx: Mutex::new(control_tx),
            control_rx: Mutex::new(Some(control_rx)),
        }
    });

    let ui_rx = shared.broadcaster.subscribe();
    let control_tx = shared
        .control_tx
        .lock()
        .expect("pulse control_tx mutex poisoned")
        .clone();

    if start_worker {
        let control_rx = shared
            .control_rx
            .lock()
            .expect("pulse control_rx mutex poisoned")
            .take()
            .expect("control_rx should be present on first create");
        start_pulse_worker(key, shared, control_rx, render_config);
    }

    (ui_rx, control_tx)
}

fn start_pulse_worker(
    key: PulseSharedKey,
    shared: Arc<SharedPulseState>,
    control_rx: Receiver<WorkerCommand>,
    config: PulseAudioConfig,
) {
    std::thread::spawn(move || {
        run_native_loop(&shared.broadcaster, control_rx, config);
        pulse_registry().remove(&key, &shared);
    });
}

pub(crate) struct PulseAudioFactory;

pub(crate) const FACTORY: PulseAudioFactory = PulseAudioFactory;

impl ModuleFactory for PulseAudioFactory {
    fn module_type(&self) -> &'static str {
        MODULE_TYPE
    }

    fn init(&self, config: &ModuleConfig, _context: &ModuleBuildContext) -> Result<Widget, String> {
        let parsed = parse_config(config)?;
        let click_command = parsed.click.clone().or(parsed.on_click.clone());
        let right_click_command = parsed.right_click.clone().or(parsed.on_right_click.clone());
        Ok(build_pulseaudio_module(parsed, click_command, right_click_command).upcast())
    }
}

fn build_pulseaudio_module(
    config: PulseAudioConfig,
    click_command: Option<String>,
    right_click_command: Option<String>,
) -> Label {
    let label = Label::new(None);
    label.add_css_class("module");
    label.add_css_class("pulseaudio");

    apply_css_classes(&label, config.class.as_deref());

    let (ui_subscription, worker_tx) = subscribe_shared_pulse(&config);

    let controls_ui = if config.controls.enabled {
        let controls_ui = build_controls_ui(&label, worker_tx.clone(), config.controls.open);
        if matches!(config.controls.open, PulseAudioControlsOpenMode::LeftClick)
            && click_command.is_some()
        {
            eprintln!("pulseaudio click command is ignored when controls.open=left-click");
        } else {
            attach_primary_click_command(&label, click_command);
        }
        Some(controls_ui)
    } else {
        attach_primary_click_command(&label, click_command);
        None
    };
    if config.controls.enabled
        && matches!(config.controls.open, PulseAudioControlsOpenMode::RightClick)
        && right_click_command.is_some()
    {
        eprintln!("pulseaudio right-click command is ignored when controls.open=right-click");
    } else {
        attach_secondary_click_command(&label, right_click_command);
    }

    let scroll_step = normalized_scroll_step(config.scroll_step);
    if (scroll_step - config.scroll_step).abs() > f64::EPSILON {
        eprintln!(
            "pulseaudio scroll-step={} is too low; clamping to {}",
            config.scroll_step, scroll_step
        );
    }
    if scroll_step > 0.0 {
        let scroll = EventControllerScroll::new(
            EventControllerScrollFlags::VERTICAL | EventControllerScrollFlags::DISCRETE,
        );
        let scroll_tx = worker_tx.clone();
        scroll.connect_scroll(move |_, _, dy| {
            if dy < 0.0 {
                let _ = scroll_tx.send(WorkerCommand::VolumeStep {
                    increase: true,
                    step: scroll_step,
                });
                return gtk::glib::Propagation::Stop;
            }
            if dy > 0.0 {
                let _ = scroll_tx.send(WorkerCommand::VolumeStep {
                    increase: false,
                    step: scroll_step,
                });
                return gtk::glib::Propagation::Stop;
            }
            gtk::glib::Propagation::Proceed
        });
        label.add_controller(scroll);
    }

    attach_subscription(&label, ui_subscription, {
        let controls_ui = controls_ui.clone();
        move |label, update| {
            let visible = !update.label_text.trim().is_empty();
            label.set_visible(visible);
            if visible {
                label.set_markup(&update.label_text);
            }
            if let Some(state) = update.controls.as_ref() {
                if let Some(controls_ui) = controls_ui.as_ref() {
                    refresh_controls_ui(controls_ui, state, worker_tx.clone());
                }
            }
        }
    });

    label
}

pub(crate) fn normalized_scroll_step(step: f64) -> f64 {
    if step <= 0.0 || !step.is_finite() {
        0.0
    } else {
        step
    }
}

fn render_format(config: &PulseAudioConfig, state: &PulseState) -> String {
    let format = if state.muted {
        if state.bluetooth {
            config
                .format_bluetooth_muted
                .as_deref()
                .unwrap_or(DEFAULT_FORMAT_BLUETOOTH_MUTED)
        } else {
            config
                .format_muted
                .as_deref()
                .unwrap_or(DEFAULT_FORMAT_MUTED)
        }
    } else if state.bluetooth {
        config
            .format_bluetooth
            .as_deref()
            .unwrap_or(DEFAULT_FORMAT_BLUETOOTH)
    } else {
        config.format.as_deref().unwrap_or(DEFAULT_FORMAT)
    };

    let source = if state.source_muted {
        config
            .format_source_muted
            .as_deref()
            .unwrap_or(DEFAULT_FORMAT_SOURCE_MUTED)
    } else {
        config
            .format_source
            .as_deref()
            .unwrap_or(DEFAULT_FORMAT_SOURCE)
    };

    let icon = config.format_icons.icon_for(state.icon_kind, state.volume);

    render_markup_template(
        format,
        &[
            ("{volume}", &state.volume.to_string()),
            ("{icon}", &icon),
            ("{format_source}", source),
        ],
    )
}

#[cfg(test)]
mod tests {
    use serde_json::{json, Map};

    use super::*;

    #[test]
    fn parse_config_rejects_wrong_module_type() {
        let module = ModuleConfig::new("clock", Map::new());
        let err = parse_config(&module).expect_err("wrong type should fail");
        assert!(err.contains("expected module type 'pulseaudio'"));
    }

    #[test]
    fn render_format_applies_muted_and_source_placeholders() {
        let module = ModuleConfig::new(
            MODULE_TYPE,
            Map::from_iter([(
                "format-icons".to_string(),
                json!({ "default": ["a", "b", "c"] }),
            )]),
        );
        let config = parse_config(&module).expect("config should parse");
        let text = render_format(
            &config,
            &PulseState {
                volume: 80,
                muted: true,
                source_muted: false,
                bluetooth: false,
                icon_kind: IconKind::Default,
            },
        );
        assert_eq!(text, " ");
    }

    #[test]
    fn normalized_scroll_step_disables_zero_and_negative() {
        assert_eq!(normalized_scroll_step(0.0), 0.0);
        assert_eq!(normalized_scroll_step(-1.0), 0.0);
        assert_eq!(normalized_scroll_step(1.5), 1.5);
    }

    #[test]
    fn parse_config_controls_defaults_to_disabled_right_click() {
        let module = ModuleConfig::new(MODULE_TYPE, Map::new());
        let config = parse_config(&module).expect("config should parse");
        assert!(!config.controls.enabled);
        assert!(matches!(
            config.controls.open,
            PulseAudioControlsOpenMode::RightClick
        ));
    }

    #[test]
    fn parse_config_supports_right_click_aliases() {
        let right_click_module = ModuleConfig::new(
            MODULE_TYPE,
            Map::from_iter([("right-click".to_string(), json!("foo"))]),
        );
        let right_click_cfg =
            parse_config(&right_click_module).expect("right-click config should parse");
        assert_eq!(right_click_cfg.right_click.as_deref(), Some("foo"));
        assert!(right_click_cfg.on_right_click.is_none());

        let on_right_click_module = ModuleConfig::new(
            MODULE_TYPE,
            Map::from_iter([("on-right-click".to_string(), json!("bar"))]),
        );
        let on_right_click_cfg =
            parse_config(&on_right_click_module).expect("on-right-click config should parse");
        assert!(on_right_click_cfg.right_click.is_none());
        assert_eq!(on_right_click_cfg.on_right_click.as_deref(), Some("bar"));
    }

    #[test]
    fn volume_icon_from_list_maps_range() {
        let icons = vec!["low".to_string(), "med".to_string(), "high".to_string()];
        assert_eq!(volume_icon_from_list(&icons, 0), "low");
        assert_eq!(volume_icon_from_list(&icons, 50), "med");
        assert_eq!(volume_icon_from_list(&icons, 100), "high");
    }

    #[test]
    fn subscription_event_filter_matches_audio_updates() {
        assert!(is_relevant_pulse_event(
            Some(Facility::Sink),
            Some(pulse::context::subscribe::Operation::Changed)
        ));
        assert!(is_relevant_pulse_event(
            Some(Facility::Source),
            Some(pulse::context::subscribe::Operation::New)
        ));
        assert!(is_relevant_pulse_event(
            Some(Facility::Server),
            Some(pulse::context::subscribe::Operation::Removed)
        ));
        assert!(is_relevant_pulse_event(
            Some(Facility::SinkInput),
            Some(pulse::context::subscribe::Operation::Changed)
        ));
        assert!(!is_relevant_pulse_event(
            Some(Facility::Client),
            Some(pulse::context::subscribe::Operation::New)
        ));
        assert!(!is_relevant_pulse_event(Some(Facility::Sink), None));
    }

    #[test]
    fn percent_to_volume_delta_converts_step() {
        let delta = percent_to_volume_delta(1.0);
        assert!(delta.0 > 0);
    }

    #[test]
    fn classify_icon_kind_matches_priority_order() {
        assert_eq!(
            classify_icon_kind_by_priority("headphone speaker"),
            IconKind::Headphone
        );
        assert_eq!(
            classify_icon_kind_by_priority("my speaker"),
            IconKind::Speaker
        );
        assert_eq!(classify_icon_kind_by_priority("dock hdmi"), IconKind::Hdmi);
        assert_eq!(classify_icon_kind_by_priority("usb hifi"), IconKind::Hifi);
        assert_eq!(
            classify_icon_kind_by_priority("built in sound card"),
            IconKind::Default
        );
        assert_eq!(classify_icon_kind_by_priority("usb car kit"), IconKind::Car);
    }
}
