use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};

use gtk::prelude::*;
use gtk::{Box as GtkBox, Button, GestureClick, Label, Orientation, Popover, PositionType, Scale};
use libpulse_binding as pulse;

use super::config::{PulseAudioControlsOpenMode, ICON_VOLUME_HIGH};
use super::{AudioControlsState, WorkerCommand, CONTROLS_UI_MAX_PERCENT, ICON_MUTED};

#[derive(Clone)]
pub(super) struct PulseAudioControlsUi {
    sink_mute_button: Button,
    sink_volume_scale: Scale,
    sink_volume_percent_label: Label,
    sinks_box: GtkBox,
    sink_ports_box: GtkBox,
    sink_inputs_box: GtkBox,
    suppress_sink_scale_callback: Arc<AtomicBool>,
    sink_muted_state: Arc<AtomicBool>,
}

pub(super) fn build_controls_ui(
    label: &Label,
    worker_tx: mpsc::Sender<WorkerCommand>,
    open_mode: PulseAudioControlsOpenMode,
) -> PulseAudioControlsUi {
    label.add_css_class("clickable");
    label.add_css_class("pulseaudio-controls-enabled");

    let popover = Popover::new();
    popover.add_css_class("pulseaudio-controls-popover");
    popover.set_autohide(true);
    popover.set_has_arrow(true);
    popover.set_position(PositionType::Top);
    popover.set_parent(label);

    let content = GtkBox::new(Orientation::Vertical, 6);
    content.add_css_class("pulseaudio-controls-content");
    popover.set_child(Some(&content));

    content.append(&build_controls_section_label("Main volume"));
    let sink_row = GtkBox::new(Orientation::Horizontal, 6);
    sink_row.add_css_class("pulseaudio-controls-sink-row");
    content.append(&sink_row);

    let sink_mute_button = Button::with_label(ICON_VOLUME_HIGH);
    sink_mute_button.add_css_class("pulseaudio-control-button");
    sink_row.append(&sink_mute_button);

    let sink_volume_scale =
        Scale::with_range(Orientation::Horizontal, 0.0, CONTROLS_UI_MAX_PERCENT, 1.0);
    sink_volume_scale.add_css_class("pulseaudio-volume-scale");
    sink_volume_scale.set_hexpand(true);
    sink_volume_scale.set_draw_value(false);
    sink_row.append(&sink_volume_scale);
    let sink_volume_percent_label = Label::new(Some("0%"));
    sink_volume_percent_label.add_css_class("pulseaudio-volume-percent");
    sink_row.append(&sink_volume_percent_label);

    let ports_box = GtkBox::new(Orientation::Vertical, 4);
    ports_box.add_css_class("pulseaudio-controls-ports");
    let sinks_box = GtkBox::new(Orientation::Vertical, 4);
    sinks_box.add_css_class("pulseaudio-controls-sinks");
    content.append(&build_controls_section_label("Select device"));
    content.append(&sinks_box);
    content.append(&build_controls_section_label("Select output"));
    content.append(&ports_box);

    let inputs_box = GtkBox::new(Orientation::Vertical, 4);
    inputs_box.add_css_class("pulseaudio-controls-inputs");
    content.append(&build_controls_section_label("Programs volume"));
    content.append(&inputs_box);

    install_controls_open_gesture(label, &popover, open_mode);

    let suppress_sink_scale_callback = Arc::new(AtomicBool::new(false));
    let sink_muted_state = Arc::new(AtomicBool::new(false));
    {
        let worker_tx = worker_tx.clone();
        let suppress = suppress_sink_scale_callback.clone();
        let percent_label = sink_volume_percent_label.clone();
        sink_volume_scale.connect_value_changed(move |scale| {
            let percent = scale.value().round().clamp(0.0, CONTROLS_UI_MAX_PERCENT) as u32;
            percent_label.set_text(&format!("{percent}%"));
            if suppress.load(Ordering::Relaxed) {
                return;
            }
            let _ = worker_tx.send(WorkerCommand::SetSinkVolumePercent { percent });
        });
    }
    {
        let worker_tx = worker_tx.clone();
        let sink_muted_state = sink_muted_state.clone();
        sink_mute_button.connect_clicked(move |_| {
            let _ = worker_tx.send(WorkerCommand::SetSinkMute {
                muted: !sink_muted_state.load(Ordering::Relaxed),
            });
        });
    }

    PulseAudioControlsUi {
        sink_mute_button,
        sink_volume_scale,
        sink_volume_percent_label,
        sinks_box,
        sink_ports_box: ports_box,
        sink_inputs_box: inputs_box,
        suppress_sink_scale_callback,
        sink_muted_state,
    }
}

fn build_controls_section_label(text: &str) -> Label {
    let label = Label::new(Some(text));
    label.add_css_class("pulseaudio-controls-section-title");
    label.set_xalign(0.0);
    label
}

fn install_controls_open_gesture(
    label: &Label,
    popover: &Popover,
    open_mode: PulseAudioControlsOpenMode,
) {
    let button = match open_mode {
        PulseAudioControlsOpenMode::LeftClick => 1,
        PulseAudioControlsOpenMode::RightClick => 3,
    };
    let click = GestureClick::builder().button(button).build();
    let popover = popover.clone();
    click.connect_pressed(move |_, _, _, _| {
        if popover.is_visible() {
            popover.popdown();
        } else {
            popover.popup();
        }
    });
    label.add_controller(click);
}

pub(super) fn refresh_controls_ui(
    controls_ui: &PulseAudioControlsUi,
    state: &AudioControlsState,
    worker_tx: mpsc::Sender<WorkerCommand>,
) {
    controls_ui.sink_mute_button.set_label(if state.sink_muted {
        ICON_MUTED
    } else {
        ICON_VOLUME_HIGH
    });
    controls_ui
        .sink_mute_button
        .set_tooltip_text(Some(&state.sink_name));
    controls_ui
        .sink_muted_state
        .store(state.sink_muted, Ordering::Relaxed);
    controls_ui
        .suppress_sink_scale_callback
        .store(true, Ordering::Relaxed);
    controls_ui
        .sink_volume_scale
        .set_value((state.sink_volume as f64).min(CONTROLS_UI_MAX_PERCENT));
    controls_ui
        .suppress_sink_scale_callback
        .store(false, Ordering::Relaxed);
    controls_ui
        .sink_volume_scale
        .set_tooltip_text(Some(&format!("Selected sink: {}%", state.sink_volume)));
    controls_ui
        .sink_volume_percent_label
        .set_text(&format!("{}%", state.sink_volume));

    clear_box_children(&controls_ui.sinks_box);
    if state.sinks.is_empty() {
        let no_sinks_label = Label::new(Some("No output devices"));
        no_sinks_label.add_css_class("pulseaudio-controls-empty");
        no_sinks_label.set_xalign(0.0);
        controls_ui.sinks_box.append(&no_sinks_label);
    } else {
        for sink in &state.sinks {
            let status = if sink.available {
                "available"
            } else {
                "unavailable"
            };
            let text = if sink.is_default {
                format!("{} (default, {status})", sink.description)
            } else {
                format!("{} ({status})", sink.description)
            };
            let button = Button::with_label(&text);
            button.add_css_class("pulseaudio-control-button");
            if sink.name == state.selected_sink_name {
                button.add_css_class("active");
            }
            if !sink.available {
                button.set_sensitive(false);
            }
            let worker_tx_for_sink = worker_tx.clone();
            let sink_name = sink.name.clone();
            button.connect_clicked(move |_| {
                let _ = worker_tx_for_sink.send(WorkerCommand::SetDefaultSink {
                    sink_name: sink_name.clone(),
                });
            });
            controls_ui.sinks_box.append(&button);
        }
    }

    clear_box_children(&controls_ui.sink_ports_box);
    if state.sink_ports.is_empty() {
        let no_ports_label = Label::new(Some("No output ports"));
        no_ports_label.add_css_class("pulseaudio-controls-empty");
        no_ports_label.set_xalign(0.0);
        controls_ui.sink_ports_box.append(&no_ports_label);
    } else {
        for port in &state.sink_ports {
            let button = Button::with_label(&port.description);
            button.add_css_class("pulseaudio-control-button");
            if state.active_sink_port.as_deref() == Some(port.name.as_str()) {
                button.add_css_class("active");
            }
            if port.available == pulse::def::PortAvailable::No {
                button.set_sensitive(false);
            }
            let port_name = port.name.clone();
            let sink_name = state.selected_sink_name.clone();
            let worker_tx = worker_tx.clone();
            button.connect_clicked(move |_| {
                let _ = worker_tx.send(WorkerCommand::SetSinkPort {
                    sink_name: sink_name.clone(),
                    port_name: port_name.clone(),
                });
            });
            controls_ui.sink_ports_box.append(&button);
        }
    }

    clear_box_children(&controls_ui.sink_inputs_box);
    if state.sink_inputs.is_empty() {
        let no_streams_label = Label::new(Some("No active playback streams"));
        no_streams_label.add_css_class("pulseaudio-controls-empty");
        no_streams_label.set_xalign(0.0);
        controls_ui.sink_inputs_box.append(&no_streams_label);
    } else {
        for input in &state.sink_inputs {
            let row = GtkBox::new(Orientation::Horizontal, 6);
            row.add_css_class("pulseaudio-controls-input-row");

            let mute_button = Button::with_label(if input.muted {
                ICON_MUTED
            } else {
                ICON_VOLUME_HIGH
            });
            mute_button.add_css_class("pulseaudio-control-button");
            let worker_tx_for_mute = worker_tx.clone();
            let index = input.index;
            let next_mute = !input.muted;
            mute_button.connect_clicked(move |_| {
                let _ = worker_tx_for_mute.send(WorkerCommand::SetSinkInputMute {
                    index,
                    muted: next_mute,
                });
            });
            row.append(&mute_button);

            let name_label = Label::new(Some(&input.name));
            name_label.add_css_class("pulseaudio-controls-input-name");
            name_label.set_hexpand(true);
            name_label.set_xalign(0.0);
            row.append(&name_label);

            let scale =
                Scale::with_range(Orientation::Horizontal, 0.0, CONTROLS_UI_MAX_PERCENT, 1.0);
            scale.add_css_class("pulseaudio-volume-scale");
            scale.set_draw_value(false);
            scale.set_width_request(120);
            scale.set_value((input.volume as f64).min(CONTROLS_UI_MAX_PERCENT));
            let percent_label = Label::new(Some(&format!("{}%", input.volume)));
            percent_label.add_css_class("pulseaudio-volume-percent");
            let worker_tx_for_volume = worker_tx.clone();
            let index = input.index;
            let percent_label_for_change = percent_label.clone();
            scale.connect_value_changed(move |scale| {
                let percent = scale.value().round().clamp(0.0, CONTROLS_UI_MAX_PERCENT) as u32;
                percent_label_for_change.set_text(&format!("{percent}%"));
                let _ = worker_tx_for_volume
                    .send(WorkerCommand::SetSinkInputVolumePercent { index, percent });
            });
            row.append(&scale);
            row.append(&percent_label);

            controls_ui.sink_inputs_box.append(&row);
        }
    }
}

fn clear_box_children(container: &GtkBox) {
    while let Some(child) = container.first_child() {
        container.remove(&child);
    }
}
