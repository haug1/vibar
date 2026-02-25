use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use glib::ControlFlow;
use gtk::prelude::*;
use gtk::{EventControllerScroll, EventControllerScrollFlags, Label, Widget};
use libpulse_binding as pulse;
use pulse::callbacks::ListResult;
use pulse::context::introspect::{ServerInfo, SinkInfo};
use pulse::context::subscribe::{Facility, InterestMaskSet};
use pulse::context::{Context, FlagSet as ContextFlagSet, State as ContextState};
use pulse::mainloop::standard::{IterateResult, Mainloop};
use pulse::operation::State as OperationState;
use pulse::proplist::{properties, Proplist};
use pulse::volume::Volume;
use serde::Deserialize;
use serde_json::Value;

use crate::modules::{
    apply_css_classes, attach_primary_click_command, escape_markup_text, render_markup_template,
    ModuleBuildContext, ModuleConfig,
};

use super::ModuleFactory;

const MAINLOOP_IDLE_SLEEP_MILLIS: u64 = 10;
const UI_DRAIN_INTERVAL_MILLIS: u64 = 16;
const SESSION_RECONNECT_DELAY_SECS: u64 = 2;
const DEFAULT_SCROLL_STEP: f64 = 1.0;
const DEFAULT_FORMAT: &str = "{volume}% {icon}  {format_source}";
const DEFAULT_FORMAT_BLUETOOTH: &str = "{volume}% {icon} {format_source}";
const DEFAULT_FORMAT_BLUETOOTH_MUTED: &str = " {icon} {format_source}";
const DEFAULT_FORMAT_MUTED: &str = " {format_source}";
const DEFAULT_FORMAT_SOURCE: &str = "";
const DEFAULT_FORMAT_SOURCE_MUTED: &str = "";
const ICON_VOLUME_LOW: &str = "";
const ICON_VOLUME_MEDIUM: &str = "";
const ICON_VOLUME_HIGH: &str = "";
const ICON_HEADPHONE: &str = "";
const ICON_HANDS_FREE: &str = "";
const ICON_HEADSET: &str = "";
const ICON_PHONE: &str = "";
const ICON_PORTABLE: &str = "";
const ICON_CAR: &str = "";
pub(crate) const MODULE_TYPE: &str = "pulseaudio";

#[derive(Debug, Deserialize, Clone)]
pub(crate) struct PulseAudioConfig {
    #[serde(rename = "scroll-step", default = "default_scroll_step")]
    pub(crate) scroll_step: f64,
    #[serde(default)]
    pub(crate) format: Option<String>,
    #[serde(rename = "format-bluetooth", default)]
    pub(crate) format_bluetooth: Option<String>,
    #[serde(rename = "format-bluetooth-muted", default)]
    pub(crate) format_bluetooth_muted: Option<String>,
    #[serde(rename = "format-muted", default)]
    pub(crate) format_muted: Option<String>,
    #[serde(rename = "format-source", default)]
    pub(crate) format_source: Option<String>,
    #[serde(rename = "format-source-muted", default)]
    pub(crate) format_source_muted: Option<String>,
    #[serde(rename = "format-icons", default = "default_format_icons")]
    pub(crate) format_icons: PulseAudioFormatIcons,
    #[serde(default)]
    pub(crate) click: Option<String>,
    #[serde(rename = "on-click", default)]
    pub(crate) on_click: Option<String>,
    #[serde(default)]
    pub(crate) class: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
pub(crate) struct PulseAudioFormatIcons {
    #[serde(default)]
    pub(crate) headphone: Option<String>,
    #[serde(default)]
    pub(crate) speaker: Option<String>,
    #[serde(default)]
    pub(crate) hdmi: Option<String>,
    #[serde(rename = "hands-free", default)]
    pub(crate) hands_free: Option<String>,
    #[serde(default)]
    pub(crate) headset: Option<String>,
    #[serde(default)]
    pub(crate) phone: Option<String>,
    #[serde(default)]
    pub(crate) portable: Option<String>,
    #[serde(default)]
    pub(crate) car: Option<String>,
    #[serde(default)]
    pub(crate) hifi: Option<String>,
    #[serde(default = "default_volume_icons")]
    pub(crate) default: Vec<String>,
}

#[derive(Debug, Clone)]
struct PulseState {
    volume: u32,
    muted: bool,
    source_muted: bool,
    bluetooth: bool,
    icon_kind: IconKind,
}

#[derive(Debug, Clone)]
struct ServerDefaults {
    sink_name: String,
    source_name: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IconKind {
    Headphone,
    Speaker,
    Hdmi,
    HandsFree,
    Headset,
    Phone,
    Portable,
    Car,
    Hifi,
    Default,
}

#[derive(Debug, Clone, Copy)]
enum WorkerCommand {
    VolumeStep { increase: bool, step: f64 },
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
        Ok(build_pulseaudio_module(parsed, click_command).upcast())
    }
}

fn default_scroll_step() -> f64 {
    DEFAULT_SCROLL_STEP
}

fn default_volume_icons() -> Vec<String> {
    vec![
        ICON_VOLUME_LOW.to_string(),
        ICON_VOLUME_MEDIUM.to_string(),
        ICON_VOLUME_HIGH.to_string(),
    ]
}

fn default_format_icons() -> PulseAudioFormatIcons {
    PulseAudioFormatIcons {
        headphone: None,
        speaker: None,
        hdmi: None,
        hands_free: None,
        headset: None,
        phone: None,
        portable: None,
        car: None,
        hifi: None,
        default: default_volume_icons(),
    }
}

pub(crate) fn parse_config(module: &ModuleConfig) -> Result<PulseAudioConfig, String> {
    if module.module_type != MODULE_TYPE {
        return Err(format!(
            "expected module type '{}', got '{}'",
            MODULE_TYPE, module.module_type
        ));
    }

    serde_json::from_value(Value::Object(module.config.clone()))
        .map_err(|err| format!("invalid {} module config: {err}", MODULE_TYPE))
}

fn build_pulseaudio_module(config: PulseAudioConfig, click_command: Option<String>) -> Label {
    let label = Label::new(None);
    label.add_css_class("module");
    label.add_css_class("pulseaudio");

    apply_css_classes(&label, config.class.as_deref());

    attach_primary_click_command(&label, click_command);

    let (worker_tx, worker_rx) = mpsc::channel::<WorkerCommand>();

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
        let worker_tx = worker_tx.clone();
        scroll.connect_scroll(move |_, _, dy| {
            if dy < 0.0 {
                let _ = worker_tx.send(WorkerCommand::VolumeStep {
                    increase: true,
                    step: scroll_step,
                });
                return glib::Propagation::Stop;
            }
            if dy > 0.0 {
                let _ = worker_tx.send(WorkerCommand::VolumeStep {
                    increase: false,
                    step: scroll_step,
                });
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        });
        label.add_controller(scroll);
    }

    let (ui_sender, ui_receiver) = mpsc::channel::<String>();
    let render_config = config.clone();
    std::thread::spawn(move || run_native_loop(ui_sender, worker_rx, render_config));

    glib::timeout_add_local(Duration::from_millis(UI_DRAIN_INTERVAL_MILLIS), {
        let label = label.clone();
        move || {
            while let Ok(text) = ui_receiver.try_recv() {
                label.set_markup(&text);
            }
            ControlFlow::Continue
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

fn run_native_loop(
    ui_sender: mpsc::Sender<String>,
    worker_rx: Receiver<WorkerCommand>,
    config: PulseAudioConfig,
) {
    loop {
        match run_native_session(&ui_sender, &worker_rx, &config) {
            Ok(()) => return,
            Err(err) => {
                let _ = ui_sender.send(escape_markup_text(&format!("audio error: {err}")));
                std::thread::sleep(Duration::from_secs(SESSION_RECONNECT_DELAY_SECS));
            }
        }
    }
}

fn run_native_session(
    ui_sender: &mpsc::Sender<String>,
    worker_rx: &Receiver<WorkerCommand>,
    config: &PulseAudioConfig,
) -> Result<(), String> {
    let mut proplist =
        Proplist::new().ok_or_else(|| "failed to create pulseaudio proplist".to_string())?;
    proplist
        .set_str(properties::APPLICATION_NAME, "vibar")
        .map_err(|err| format!("failed to set pulseaudio app name: {err:?}"))?;

    let mut mainloop =
        Mainloop::new().ok_or_else(|| "failed to create pulseaudio mainloop".to_string())?;
    let mut context = Context::new_with_proplist(&mainloop, "vibar-pulseaudio", &proplist)
        .ok_or_else(|| "failed to create pulseaudio context".to_string())?;

    context
        .connect(None, ContextFlagSet::NOFLAGS, None)
        .map_err(|err| format!("failed to connect pulseaudio context: {err:?}"))?;

    wait_for_context_ready(&mut mainloop, &context)?;

    let dirty = Arc::new(AtomicBool::new(true));
    context.set_subscribe_callback(Some(Box::new({
        let dirty = Arc::clone(&dirty);
        move |facility, operation, _| {
            if is_relevant_pulse_event(facility, operation) {
                dirty.store(true, Ordering::SeqCst);
            }
        }
    })));

    let mut subscribe_op = context.subscribe(
        InterestMaskSet::SINK
            | InterestMaskSet::SOURCE
            | InterestMaskSet::SERVER
            | InterestMaskSet::CARD,
        |_| {},
    );
    wait_for_operation(&mut mainloop, &mut subscribe_op)?;

    let mut last_defaults: Option<ServerDefaults> = None;

    loop {
        loop {
            match worker_rx.try_recv() {
                Ok(WorkerCommand::VolumeStep { increase, step }) => {
                    if let Some(defaults) = last_defaults.as_ref() {
                        let _ = apply_volume_step(
                            &context,
                            &mut mainloop,
                            &defaults.sink_name,
                            step,
                            increase,
                        );
                    }
                    dirty.store(true, Ordering::SeqCst);
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => return Ok(()),
            }
        }

        if dirty.swap(false, Ordering::SeqCst) {
            match query_current_state(&context, &mut mainloop) {
                Ok((state, defaults)) => {
                    last_defaults = Some(defaults);
                    let _ = ui_sender.send(render_format(config, &state));
                }
                Err(err) => {
                    let _ = ui_sender.send(escape_markup_text(&format!("audio error: {err}")));
                }
            }
        }

        match mainloop.iterate(false) {
            IterateResult::Success(_) => {}
            IterateResult::Quit(_) => return Err("pulseaudio mainloop quit".to_string()),
            IterateResult::Err(err) => {
                return Err(format!("pulseaudio mainloop iteration failed: {err:?}"));
            }
        }

        match context.get_state() {
            ContextState::Ready => {}
            ContextState::Failed => {
                return Err(format!("pulseaudio context failed: {:?}", context.errno()));
            }
            ContextState::Terminated => {
                return Err("pulseaudio context terminated".to_string());
            }
            _ => {}
        }

        std::thread::sleep(Duration::from_millis(MAINLOOP_IDLE_SLEEP_MILLIS));
    }
}

fn wait_for_context_ready(mainloop: &mut Mainloop, context: &Context) -> Result<(), String> {
    loop {
        match context.get_state() {
            ContextState::Ready => return Ok(()),
            ContextState::Failed => {
                return Err(format!(
                    "pulseaudio context failed while connecting: {:?}",
                    context.errno()
                ));
            }
            ContextState::Terminated => {
                return Err("pulseaudio context terminated while connecting".to_string());
            }
            _ => iterate_mainloop_blocking(mainloop)?,
        }
    }
}

fn wait_for_operation<ClosureProto: ?Sized>(
    mainloop: &mut Mainloop,
    operation: &mut pulse::operation::Operation<ClosureProto>,
) -> Result<(), String> {
    loop {
        match operation.get_state() {
            OperationState::Running => iterate_mainloop_blocking(mainloop)?,
            OperationState::Done => return Ok(()),
            OperationState::Cancelled => {
                return Err("pulseaudio operation was cancelled".to_string());
            }
        }
    }
}

fn iterate_mainloop_blocking(mainloop: &mut Mainloop) -> Result<(), String> {
    match mainloop.iterate(true) {
        IterateResult::Success(_) => Ok(()),
        IterateResult::Quit(_) => Err("pulseaudio mainloop quit".to_string()),
        IterateResult::Err(err) => Err(format!("pulseaudio mainloop iteration failed: {err:?}")),
    }
}

fn is_relevant_pulse_event(
    facility: Option<Facility>,
    operation: Option<pulse::context::subscribe::Operation>,
) -> bool {
    let Some(facility) = facility else {
        return false;
    };

    let relevant_facility = matches!(
        facility,
        Facility::Sink | Facility::Source | Facility::Server | Facility::Card
    );
    let relevant_operation = operation.is_some();
    relevant_facility && relevant_operation
}

fn query_current_state(
    context: &Context,
    mainloop: &mut Mainloop,
) -> Result<(PulseState, ServerDefaults), String> {
    let defaults = query_server_defaults(context, mainloop)?;
    let sink_info = query_sink_info(context, mainloop, &defaults.sink_name)?;

    let source_muted = match defaults.source_name.as_ref() {
        Some(source_name) => query_source_muted(context, mainloop, source_name)?,
        None => false,
    };

    Ok((
        PulseState {
            volume: sink_info.volume,
            muted: sink_info.muted,
            source_muted,
            bluetooth: sink_info.bluetooth,
            icon_kind: sink_info.icon_kind,
        },
        defaults,
    ))
}

fn query_server_defaults(
    context: &Context,
    mainloop: &mut Mainloop,
) -> Result<ServerDefaults, String> {
    let slot = Arc::new(Mutex::new(None::<ServerDefaults>));
    let mut op = context.introspect().get_server_info({
        let slot = Arc::clone(&slot);
        move |info: &ServerInfo| {
            let sink_name = info.default_sink_name.as_ref().map(|name| name.to_string());
            let source_name = info
                .default_source_name
                .as_ref()
                .map(|name| name.to_string());

            if let Some(sink_name) = sink_name {
                *slot.lock().expect("server defaults mutex poisoned") = Some(ServerDefaults {
                    sink_name,
                    source_name,
                });
            }
        }
    });
    wait_for_operation(mainloop, &mut op)?;

    let result = slot
        .lock()
        .expect("server defaults mutex poisoned")
        .clone()
        .ok_or_else(|| "pulseaudio default sink is unavailable".to_string());
    result
}

#[derive(Debug, Clone)]
struct SinkSnapshot {
    volume: u32,
    muted: bool,
    bluetooth: bool,
    icon_kind: IconKind,
}

fn query_sink_info(
    context: &Context,
    mainloop: &mut Mainloop,
    sink_name: &str,
) -> Result<SinkSnapshot, String> {
    let slot = Arc::new(Mutex::new(None::<Result<SinkSnapshot, String>>));
    let mut op = context.introspect().get_sink_info_by_name(sink_name, {
        let slot = Arc::clone(&slot);
        move |result| {
            let mut guard = slot.lock().expect("sink info mutex poisoned");
            match result {
                ListResult::Item(info) => {
                    *guard = Some(Ok(snapshot_from_sink_info(info)));
                }
                ListResult::End => {
                    if guard.is_none() {
                        *guard = Some(Err("pulseaudio sink info not found".to_string()));
                    }
                }
                ListResult::Error => {
                    *guard = Some(Err("pulseaudio sink info query failed".to_string()));
                }
            }
        }
    });
    wait_for_operation(mainloop, &mut op)?;

    let result = slot
        .lock()
        .expect("sink info mutex poisoned")
        .clone()
        .unwrap_or_else(|| Err("pulseaudio sink info query returned no data".to_string()));
    result
}

fn query_source_muted(
    context: &Context,
    mainloop: &mut Mainloop,
    source_name: &str,
) -> Result<bool, String> {
    let slot = Arc::new(Mutex::new(None::<Result<bool, String>>));
    let mut op = context.introspect().get_source_info_by_name(source_name, {
        let slot = Arc::clone(&slot);
        move |result| {
            let mut guard = slot.lock().expect("source info mutex poisoned");
            match result {
                ListResult::Item(info) => {
                    *guard = Some(Ok(info.mute));
                }
                ListResult::End => {
                    if guard.is_none() {
                        *guard = Some(Err("pulseaudio source info not found".to_string()));
                    }
                }
                ListResult::Error => {
                    *guard = Some(Err("pulseaudio source info query failed".to_string()));
                }
            }
        }
    });
    wait_for_operation(mainloop, &mut op)?;

    let result = slot
        .lock()
        .expect("source info mutex poisoned")
        .clone()
        .unwrap_or_else(|| Err("pulseaudio source info query returned no data".to_string()));
    result
}

fn snapshot_from_sink_info(info: &SinkInfo) -> SinkSnapshot {
    let volume = volume_to_percent(info.volume.avg());
    let port_name = info
        .active_port
        .as_ref()
        .and_then(|port| port.name.as_ref())
        .map(|name| name.to_string())
        .unwrap_or_default();
    let form_factor = info
        .proplist
        .get_str(properties::DEVICE_FORM_FACTOR)
        .unwrap_or_default();
    let lower = format!("{port_name}{form_factor}").to_ascii_lowercase();

    SinkSnapshot {
        volume,
        muted: info.mute,
        bluetooth: lower.contains("bluez") || lower.contains("bluetooth"),
        icon_kind: classify_icon_kind_by_priority(&lower),
    }
}

fn volume_to_percent(volume: Volume) -> u32 {
    ((volume.0 as f64 / Volume::NORMAL.0 as f64) * 100.0).round() as u32
}

fn apply_volume_step(
    context: &Context,
    mainloop: &mut Mainloop,
    sink_name: &str,
    step: f64,
    increase: bool,
) -> Result<(), String> {
    let sink_info = query_sink_info(context, mainloop, sink_name)?;
    let mut current = query_sink_channel_volumes(context, mainloop, sink_name)?;

    let delta = percent_to_volume_delta(step);
    if increase {
        let _ = current.increase(delta);
    } else {
        let _ = current.decrease(delta);
    }

    let mut introspector = context.introspect();
    let mut op = introspector.set_sink_volume_by_name(sink_name, &current, None);
    wait_for_operation(mainloop, &mut op)?;

    if sink_info.muted {
        let mut mute_op = introspector.set_sink_mute_by_name(sink_name, false, None);
        wait_for_operation(mainloop, &mut mute_op)?;
    }

    Ok(())
}

fn query_sink_channel_volumes(
    context: &Context,
    mainloop: &mut Mainloop,
    sink_name: &str,
) -> Result<pulse::volume::ChannelVolumes, String> {
    let slot = Arc::new(Mutex::new(
        None::<Result<pulse::volume::ChannelVolumes, String>>,
    ));
    let mut op = context.introspect().get_sink_info_by_name(sink_name, {
        let slot = Arc::clone(&slot);
        move |result| {
            let mut guard = slot.lock().expect("sink volume mutex poisoned");
            match result {
                ListResult::Item(info) => {
                    *guard = Some(Ok(info.volume));
                }
                ListResult::End => {
                    if guard.is_none() {
                        *guard = Some(Err("pulseaudio sink volume not found".to_string()));
                    }
                }
                ListResult::Error => {
                    *guard = Some(Err("pulseaudio sink volume query failed".to_string()));
                }
            }
        }
    });
    wait_for_operation(mainloop, &mut op)?;

    let result = slot
        .lock()
        .expect("sink volume mutex poisoned")
        .clone()
        .unwrap_or_else(|| Err("pulseaudio sink volume query returned no data".to_string()));
    result
}

fn percent_to_volume_delta(step: f64) -> Volume {
    let step = normalized_scroll_step(step).clamp(0.1, 100.0);
    let value = ((step / 100.0) * f64::from(Volume::NORMAL.0)).round() as u32;
    Volume(value.max(1))
}

fn classify_icon_kind_by_priority(content: &str) -> IconKind {
    if content.contains("headphone") {
        return IconKind::Headphone;
    }
    if content.contains("speaker") {
        return IconKind::Speaker;
    }
    if content.contains("hdmi") {
        return IconKind::Hdmi;
    }
    if content.contains("headset") {
        return IconKind::Headset;
    }
    if content.contains("hands-free") || content.contains("handsfree") {
        return IconKind::HandsFree;
    }
    if content.contains("portable") {
        return IconKind::Portable;
    }
    if contains_word(content, "car") {
        return IconKind::Car;
    }
    if content.contains("hifi") {
        return IconKind::Hifi;
    }
    if content.contains("phone") {
        return IconKind::Phone;
    }
    IconKind::Default
}

fn contains_word(content: &str, needle: &str) -> bool {
    content
        .split(|c: char| !c.is_ascii_alphanumeric() && c != '-')
        .any(|token| token == needle)
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

impl PulseAudioFormatIcons {
    fn icon_for(&self, kind: IconKind, volume: u32) -> String {
        match kind {
            IconKind::Headphone => self
                .headphone
                .as_deref()
                .unwrap_or(ICON_HEADPHONE)
                .to_string(),
            IconKind::Speaker => self
                .speaker
                .as_deref()
                .unwrap_or(ICON_VOLUME_HIGH)
                .to_string(),
            IconKind::Hdmi => self.hdmi.as_deref().unwrap_or(ICON_VOLUME_HIGH).to_string(),
            IconKind::HandsFree => self
                .hands_free
                .as_deref()
                .unwrap_or(ICON_HANDS_FREE)
                .to_string(),
            IconKind::Headset => self.headset.as_deref().unwrap_or(ICON_HEADSET).to_string(),
            IconKind::Phone => self.phone.as_deref().unwrap_or(ICON_PHONE).to_string(),
            IconKind::Portable => self
                .portable
                .as_deref()
                .unwrap_or(ICON_PORTABLE)
                .to_string(),
            IconKind::Car => self.car.as_deref().unwrap_or(ICON_CAR).to_string(),
            IconKind::Hifi => self.hifi.as_deref().unwrap_or(ICON_VOLUME_HIGH).to_string(),
            IconKind::Default => volume_icon_from_list(&self.default, volume),
        }
    }
}

fn volume_icon_from_list(icons: &[String], volume: u32) -> String {
    if icons.is_empty() {
        return if volume == 0 {
            ICON_VOLUME_LOW.to_string()
        } else if volume < 67 {
            ICON_VOLUME_MEDIUM.to_string()
        } else {
            ICON_VOLUME_HIGH.to_string()
        };
    }

    let clamped = volume.min(100) as usize;
    let len = icons.len();
    let idx = ((clamped * len) / 100).min(len - 1);
    icons[idx].clone()
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
