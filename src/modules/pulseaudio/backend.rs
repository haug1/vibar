use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use libpulse_binding as pulse;
use pulse::callbacks::ListResult;
use pulse::context::introspect::{ServerInfo, SinkInfo, SinkInputInfo};
use pulse::context::subscribe::{Facility, InterestMaskSet};
use pulse::context::{Context, FlagSet as ContextFlagSet, State as ContextState};
use pulse::mainloop::standard::{IterateResult, Mainloop};
use pulse::operation::State as OperationState;
use pulse::proplist::{properties, Proplist};
use pulse::volume::Volume;

use crate::modules::broadcaster::Broadcaster;
use crate::modules::escape_markup_text;

use super::config::PulseAudioConfig;
use super::format::{classify_icon_kind_by_priority, IconKind};
use super::{
    normalized_scroll_step, render_format, AudioControlsState, PulseState, SinkDeviceEntry,
    SinkInputEntry, SinkPortEntry, UiUpdate, WorkerCommand, MAINLOOP_IDLE_SLEEP_MILLIS,
    SESSION_RECONNECT_DELAY_SECS,
};

#[derive(Debug, Clone)]
struct ServerDefaults {
    sink_name: String,
    source_name: Option<String>,
}

#[derive(Debug, Clone)]
struct SinkSnapshot {
    volume: u32,
    muted: bool,
    bluetooth: bool,
    icon_kind: IconKind,
    channels: pulse::volume::ChannelVolumes,
    ports: Vec<SinkPortEntry>,
    active_port_name: Option<String>,
}

pub(super) fn run_native_loop(
    broadcaster: &Broadcaster<UiUpdate>,
    worker_rx: Receiver<WorkerCommand>,
    config: PulseAudioConfig,
) {
    loop {
        if broadcaster.subscriber_count() == 0 {
            return;
        }
        match run_native_session(broadcaster, &worker_rx, &config) {
            Ok(()) => return,
            Err(err) => {
                broadcaster.broadcast(UiUpdate {
                    label_text: escape_markup_text(&format!("audio error: {err}")),
                    controls: None,
                });
                std::thread::sleep(Duration::from_secs(SESSION_RECONNECT_DELAY_SECS));
            }
        }
    }
}

fn run_native_session(
    broadcaster: &Broadcaster<UiUpdate>,
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
            | InterestMaskSet::CARD
            | InterestMaskSet::SINK_INPUT,
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
                Ok(WorkerCommand::SetSinkMute { muted }) => {
                    if let Some(defaults) = last_defaults.as_ref() {
                        let _ = set_sink_mute(&context, &mut mainloop, &defaults.sink_name, muted);
                    }
                    dirty.store(true, Ordering::SeqCst);
                }
                Ok(WorkerCommand::SetSinkVolumePercent { percent }) => {
                    if let Some(defaults) = last_defaults.as_ref() {
                        let _ = set_sink_volume_percent(
                            &context,
                            &mut mainloop,
                            &defaults.sink_name,
                            percent,
                        );
                    }
                    dirty.store(true, Ordering::SeqCst);
                }
                Ok(WorkerCommand::SetSinkInputMute { index, muted }) => {
                    let _ = set_sink_input_mute(&context, &mut mainloop, index, muted);
                    dirty.store(true, Ordering::SeqCst);
                }
                Ok(WorkerCommand::SetSinkInputVolumePercent { index, percent }) => {
                    let _ = set_sink_input_volume_percent(&context, &mut mainloop, index, percent);
                    dirty.store(true, Ordering::SeqCst);
                }
                Ok(WorkerCommand::SetDefaultSink { sink_name }) => {
                    let _ = set_default_sink(&mut context, &mut mainloop, &sink_name);
                    dirty.store(true, Ordering::SeqCst);
                }
                Ok(WorkerCommand::SetSinkPort {
                    sink_name,
                    port_name,
                }) => {
                    let _ = set_sink_port(&context, &mut mainloop, &sink_name, &port_name);
                    dirty.store(true, Ordering::SeqCst);
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    // Control channel disconnected; all UI senders gone
                    return Ok(());
                }
            }
        }

        if dirty.swap(false, Ordering::SeqCst) {
            match query_current_state(&context, &mut mainloop) {
                Ok((state, defaults, controls_state)) => {
                    last_defaults = Some(defaults);
                    broadcaster.broadcast(UiUpdate {
                        label_text: render_format(config, &state),
                        controls: Some(controls_state),
                    });
                }
                Err(err) => {
                    broadcaster.broadcast(UiUpdate {
                        label_text: escape_markup_text(&format!("audio error: {err}")),
                        controls: None,
                    });
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

        if broadcaster.subscriber_count() == 0 {
            return Ok(());
        }
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

pub(super) fn is_relevant_pulse_event(
    facility: Option<Facility>,
    operation: Option<pulse::context::subscribe::Operation>,
) -> bool {
    let Some(facility) = facility else {
        return false;
    };

    let relevant_facility = matches!(
        facility,
        Facility::Sink | Facility::Source | Facility::Server | Facility::Card | Facility::SinkInput
    );
    let relevant_operation = operation.is_some();
    relevant_facility && relevant_operation
}

fn query_current_state(
    context: &Context,
    mainloop: &mut Mainloop,
) -> Result<(PulseState, ServerDefaults, AudioControlsState), String> {
    let defaults = query_server_defaults(context, mainloop)?;
    let sinks = query_sinks(context, mainloop, &defaults.sink_name)?;
    let sink_info = query_sink_info(context, mainloop, &defaults.sink_name)?;
    let sink_inputs = query_sink_inputs(context, mainloop)?;

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
        defaults.clone(),
        AudioControlsState {
            sink_name: defaults.sink_name.clone(),
            sinks,
            selected_sink_name: defaults.sink_name.clone(),
            sink_volume: sink_info.volume,
            sink_muted: sink_info.muted,
            sink_ports: sink_info.ports,
            active_sink_port: sink_info.active_port_name,
            sink_inputs,
        },
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
        channels: info.volume,
        ports: sink_ports_from_info(info),
        active_port_name: info
            .active_port
            .as_ref()
            .and_then(|port| port.name.as_ref())
            .map(|name| name.to_string()),
    }
}

fn sink_ports_from_info(info: &SinkInfo) -> Vec<SinkPortEntry> {
    let mut ports = Vec::new();
    for port in &info.ports {
        let Some(name) = port.name.as_ref() else {
            continue;
        };
        let description = port
            .description
            .as_ref()
            .map(|desc| desc.to_string())
            .unwrap_or_else(|| name.to_string());
        ports.push(SinkPortEntry {
            name: name.to_string(),
            description,
            available: port.available,
        });
    }
    ports
}

fn query_sinks(
    context: &Context,
    mainloop: &mut Mainloop,
    default_sink_name: &str,
) -> Result<Vec<SinkDeviceEntry>, String> {
    let slot = Arc::new(Mutex::new(None::<Result<Vec<SinkDeviceEntry>, String>>));
    let items = Arc::new(Mutex::new(Vec::<SinkDeviceEntry>::new()));
    let mut op = context.introspect().get_sink_info_list({
        let slot = Arc::clone(&slot);
        let items = Arc::clone(&items);
        let default_sink_name = default_sink_name.to_string();
        move |result| match result {
            ListResult::Item(info) => {
                if let Some(snapshot) = sink_device_from_info(info, &default_sink_name) {
                    items
                        .lock()
                        .expect("sink list mutex poisoned")
                        .push(snapshot);
                }
            }
            ListResult::End => {
                let mut guard = slot.lock().expect("sink list result mutex poisoned");
                if guard.is_none() {
                    let mut values = items.lock().expect("sink list mutex poisoned").clone();
                    values.sort_by(|a, b| a.description.cmp(&b.description));
                    *guard = Some(Ok(values));
                }
            }
            ListResult::Error => {
                *slot.lock().expect("sink list result mutex poisoned") =
                    Some(Err("pulseaudio sink list query failed".to_string()));
            }
        }
    });
    wait_for_operation(mainloop, &mut op)?;
    let result = slot
        .lock()
        .expect("sink list result mutex poisoned")
        .clone()
        .unwrap_or_else(|| Err("pulseaudio sink list query returned no data".to_string()));
    result
}

fn sink_device_from_info(info: &SinkInfo, default_sink_name: &str) -> Option<SinkDeviceEntry> {
    let name = info.name.as_ref().map(|value| value.to_string())?;
    let description = info
        .description
        .as_ref()
        .map(|value| value.to_string())
        .unwrap_or_else(|| name.clone());
    let available = sink_is_available(info);
    Some(SinkDeviceEntry {
        is_default: name == default_sink_name,
        name,
        description,
        available,
    })
}

fn sink_is_available(info: &SinkInfo) -> bool {
    if info.ports.is_empty() {
        return true;
    }
    info.ports
        .iter()
        .any(|port| port.available != pulse::def::PortAvailable::No)
}

fn query_sink_inputs(
    context: &Context,
    mainloop: &mut Mainloop,
) -> Result<Vec<SinkInputEntry>, String> {
    let slot = Arc::new(Mutex::new(None::<Result<Vec<SinkInputEntry>, String>>));
    let items = Arc::new(Mutex::new(Vec::<SinkInputEntry>::new()));
    let mut op = context.introspect().get_sink_input_info_list({
        let slot = Arc::clone(&slot);
        let items = Arc::clone(&items);
        move |result| match result {
            ListResult::Item(info) => {
                if let Some(snapshot) = sink_input_from_info(info) {
                    items
                        .lock()
                        .expect("sink input list mutex poisoned")
                        .push(snapshot);
                }
            }
            ListResult::End => {
                let mut guard = slot.lock().expect("sink input result mutex poisoned");
                if guard.is_none() {
                    let mut values = items
                        .lock()
                        .expect("sink input list mutex poisoned")
                        .clone();
                    values.sort_by(|a, b| a.name.cmp(&b.name));
                    *guard = Some(Ok(values));
                }
            }
            ListResult::Error => {
                *slot.lock().expect("sink input result mutex poisoned") =
                    Some(Err("pulseaudio sink input list query failed".to_string()));
            }
        }
    });
    wait_for_operation(mainloop, &mut op)?;
    let result = slot
        .lock()
        .expect("sink input result mutex poisoned")
        .clone()
        .unwrap_or_else(|| Err("pulseaudio sink input list query returned no data".to_string()));
    result
}

fn sink_input_from_info(info: &SinkInputInfo) -> Option<SinkInputEntry> {
    if !info.has_volume {
        return None;
    }
    let name = sink_input_display_name(info);
    Some(SinkInputEntry {
        index: info.index,
        name,
        volume: volume_to_percent(info.volume.avg()),
        muted: info.mute,
    })
}

fn sink_input_display_name(info: &SinkInputInfo) -> String {
    info.proplist
        .get_str(properties::APPLICATION_NAME)
        .or_else(|| info.proplist.get_str("media.name"))
        .or_else(|| info.proplist.get_str("application.process.binary"))
        .or_else(|| info.name.as_ref().map(|name| name.to_string()))
        .unwrap_or_else(|| format!("Stream {}", info.index))
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
    let mut current = sink_info.channels;

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

fn query_sink_input_channel_volumes(
    context: &Context,
    mainloop: &mut Mainloop,
    index: u32,
) -> Result<pulse::volume::ChannelVolumes, String> {
    let slot = Arc::new(Mutex::new(
        None::<Result<pulse::volume::ChannelVolumes, String>>,
    ));
    let mut op = context.introspect().get_sink_input_info(index, {
        let slot = Arc::clone(&slot);
        move |result| {
            let mut guard = slot.lock().expect("sink input volume mutex poisoned");
            match result {
                ListResult::Item(info) => {
                    *guard = Some(Ok(info.volume));
                }
                ListResult::End => {
                    if guard.is_none() {
                        *guard = Some(Err("pulseaudio sink input volume not found".to_string()));
                    }
                }
                ListResult::Error => {
                    *guard = Some(Err("pulseaudio sink input volume query failed".to_string()));
                }
            }
        }
    });
    wait_for_operation(mainloop, &mut op)?;

    let result = slot
        .lock()
        .expect("sink input volume mutex poisoned")
        .clone()
        .unwrap_or_else(|| Err("pulseaudio sink input volume query returned no data".to_string()));
    result
}

fn set_sink_mute(
    context: &Context,
    mainloop: &mut Mainloop,
    sink_name: &str,
    muted: bool,
) -> Result<(), String> {
    let mut introspector = context.introspect();
    let mut op = introspector.set_sink_mute_by_name(sink_name, muted, None);
    wait_for_operation(mainloop, &mut op)
}

fn set_sink_volume_percent(
    context: &Context,
    mainloop: &mut Mainloop,
    sink_name: &str,
    percent: u32,
) -> Result<(), String> {
    let mut channels = query_sink_channel_volumes(context, mainloop, sink_name)?;
    let channel_count = channels.len();
    let target = percent_to_volume_absolute(percent);
    channels.set(channel_count, target);
    let mut introspector = context.introspect();
    let mut op = introspector.set_sink_volume_by_name(sink_name, &channels, None);
    wait_for_operation(mainloop, &mut op)
}

fn set_sink_input_mute(
    context: &Context,
    mainloop: &mut Mainloop,
    index: u32,
    muted: bool,
) -> Result<(), String> {
    let mut introspector = context.introspect();
    let mut op = introspector.set_sink_input_mute(index, muted, None);
    wait_for_operation(mainloop, &mut op)
}

fn set_sink_input_volume_percent(
    context: &Context,
    mainloop: &mut Mainloop,
    index: u32,
    percent: u32,
) -> Result<(), String> {
    let mut channels = query_sink_input_channel_volumes(context, mainloop, index)?;
    let channel_count = channels.len();
    let target = percent_to_volume_absolute(percent);
    channels.set(channel_count, target);
    let mut introspector = context.introspect();
    let mut op = introspector.set_sink_input_volume(index, &channels, None);
    wait_for_operation(mainloop, &mut op)
}

fn set_sink_port(
    context: &Context,
    mainloop: &mut Mainloop,
    sink_name: &str,
    port_name: &str,
) -> Result<(), String> {
    let mut introspector = context.introspect();
    let mut op = introspector.set_sink_port_by_name(sink_name, port_name, None);
    wait_for_operation(mainloop, &mut op)
}

fn set_default_sink(
    context: &mut Context,
    mainloop: &mut Mainloop,
    sink_name: &str,
) -> Result<(), String> {
    let mut op = context.set_default_sink(sink_name, |_| {});
    wait_for_operation(mainloop, &mut op)
}

pub(super) fn percent_to_volume_delta(step: f64) -> Volume {
    let step = normalized_scroll_step(step).clamp(0.1, 100.0);
    let value = ((step / 100.0) * f64::from(Volume::NORMAL.0)).round() as u32;
    Volume(value.max(1))
}

fn percent_to_volume_absolute(percent: u32) -> Volume {
    let bounded = percent.min(150);
    let raw = ((bounded as f64 / 100.0) * f64::from(Volume::NORMAL.0)).round() as u32;
    Volume(raw)
}
