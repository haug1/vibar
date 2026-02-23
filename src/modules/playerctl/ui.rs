use std::cell::RefCell;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use glib::ControlFlow;
use gtk::prelude::*;
use gtk::{
    Box as GtkBox, Button, DrawingArea, EventControllerMotion, GestureClick, Grid, Label,
    Orientation, Overlay, Popover, PositionType, Scale, Widget,
};

use crate::modules::apply_css_classes;

use super::backend::{call_player_method, call_set_position};
use super::config::{PlayerctlControlsOpenMode, PlayerctlMarqueeMode};
use super::model::{format_timestamp_micros, metadata_seek_ratio, PlayerctlMetadata};

#[derive(Clone)]
pub(super) struct PlayerctlControlsUi {
    pub(super) popover: Popover,
    metadata_status_value: Label,
    metadata_player_value: Label,
    metadata_artist_value: Label,
    metadata_album_value: Label,
    metadata_title_value: Label,
    previous_button: Button,
    play_pause_button: Button,
    next_button: Button,
    seek_scale: Scale,
    seek_widget: Widget,
    seek_time_widget: Widget,
    seek_position_label: Label,
    seek_length_label: Label,
    suppress_seek_callback: Arc<AtomicBool>,
    seek_update_hold_until: Arc<std::sync::Mutex<Option<Instant>>>,
    current_metadata: Arc<std::sync::Mutex<Option<PlayerctlMetadata>>>,
    show_seek: bool,
}

#[derive(Clone)]
pub(super) struct PlayerctlCarouselUi {
    root: Overlay,
    width_limit_px: i32,
    pub(super) area: DrawingArea,
    pub(super) marquee: PlayerctlMarqueeMode,
    state: Rc<RefCell<PlayerctlCarouselState>>,
}

#[derive(Clone)]
pub(super) struct PlayerctlTooltipUi {
    label: Label,
}

#[derive(Debug)]
struct PlayerctlCarouselState {
    full_text: String,
    layout: Option<gtk::pango::Layout>,
    content_width_px: f64,
    viewport_width_px: i32,
    text_height_px: i32,
    offset_px: f64,
    last_tick: Instant,
    hover_active: bool,
    open_active: bool,
    hold_until: Option<Instant>,
    waiting_restart: bool,
}

pub(super) fn build_carousel_ui(
    root: &Overlay,
    max_width_chars: u32,
    extra_classes: Option<&str>,
    marquee: PlayerctlMarqueeMode,
) -> PlayerctlCarouselUi {
    let area = DrawingArea::new();
    area.add_css_class("playerctl-carousel");
    area.set_overflow(gtk::Overflow::Hidden);
    area.set_focusable(false);
    area.set_can_target(false);
    area.set_hexpand(false);
    area.set_halign(gtk::Align::Start);
    area.set_vexpand(false);
    area.set_valign(gtk::Align::Center);

    let width_limit_px = width_px_for_widget(&area, max_width_chars);
    let viewport_width_px = 1;
    let viewport_height_px = fixed_height_px_from_label_probe(extra_classes);
    area.set_content_width(viewport_width_px);
    area.set_content_height(viewport_height_px);
    area.set_size_request(viewport_width_px, -1);

    root.set_overflow(gtk::Overflow::Hidden);
    root.set_size_request(viewport_width_px, -1);
    root.set_hexpand(false);
    root.set_halign(gtk::Align::Start);
    root.set_valign(gtk::Align::Center);

    let state = Rc::new(RefCell::new(PlayerctlCarouselState {
        full_text: String::new(),
        layout: None,
        content_width_px: 0.0,
        viewport_width_px,
        text_height_px: 0,
        offset_px: 0.0,
        last_tick: Instant::now(),
        hover_active: false,
        open_active: false,
        hold_until: None,
        waiting_restart: false,
    }));

    area.set_draw_func({
        let state = state.clone();
        move |area, context, width, height| {
            let state = state.borrow();
            let Some(layout) = state.layout.as_ref() else {
                return;
            };
            let y = ((height - state.text_height_px).max(0) as f64) / 2.0;
            let show_overflow_hint = should_show_overflow_hint(&state, marquee);
            let hint_width_px = if show_overflow_hint {
                overflow_hint_width_px(area)
            } else {
                0
            };
            let text_clip_width_px = (width - hint_width_px).max(1);

            context.save().ok();
            context.rectangle(0.0, 0.0, f64::from(text_clip_width_px), f64::from(height));
            context.clip();

            render_layout_at(area, context, -state.offset_px, y, layout);

            if state.content_width_px > area.allocated_width() as f64 {
                let next_x = -state.offset_px + state.content_width_px + carousel_gap_px();
                if next_x < area.allocated_width() as f64 {
                    render_layout_at(area, context, next_x, y, layout);
                }
            }
            context.restore().ok();

            if show_overflow_hint {
                render_overflow_hint(area, context, y);
            }
        }
    });

    PlayerctlCarouselUi {
        root: root.clone(),
        width_limit_px,
        area,
        marquee,
        state,
    }
}

pub(super) fn set_playerctl_text(
    label: &Label,
    tooltip_ui: &PlayerctlTooltipUi,
    carousel: Option<&PlayerctlCarouselUi>,
    text: &str,
) {
    tooltip_ui.label.set_text(text);

    if let Some(carousel) = carousel {
        let should_reset = {
            let state = carousel.state.borrow();
            state.full_text != text
        };

        if should_reset {
            reset_carousel_state(carousel, text);
            carousel.area.queue_draw();
        }
    } else {
        label.set_text(text);
    }
}

pub(super) fn build_playerctl_tooltip(
    root: &Overlay,
    controls_popover: Option<&Popover>,
) -> PlayerctlTooltipUi {
    let popover = Popover::new();
    popover.add_css_class("playerctl-tooltip-popover");
    popover.set_has_arrow(true);
    popover.set_position(PositionType::Top);
    popover.set_autohide(false);
    popover.set_parent(root);

    let label = Label::new(None);
    label.add_css_class("playerctl-tooltip-label");
    label.set_wrap(false);
    label.set_single_line_mode(true);
    label.set_xalign(0.0);
    popover.set_child(Some(&label));

    let tooltip_suppressed = Arc::new(AtomicBool::new(false));
    if let Some(controls_popover) = controls_popover {
        let tooltip_suppressed_on_show = tooltip_suppressed.clone();
        let tooltip_popover = popover.clone();
        controls_popover.connect_show(move |_| {
            tooltip_suppressed_on_show.store(true, Ordering::Relaxed);
            tooltip_popover.popdown();
        });
        let tooltip_suppressed_on_hide = tooltip_suppressed.clone();
        controls_popover.connect_hide(move |_| {
            tooltip_suppressed_on_hide.store(false, Ordering::Relaxed);
        });
    }

    let motion = EventControllerMotion::new();
    {
        let popover = popover.clone();
        let label = label.clone();
        let tooltip_suppressed = tooltip_suppressed.clone();
        motion.connect_enter(move |_, _, _| {
            if !tooltip_suppressed.load(Ordering::Relaxed) && !label.text().is_empty() {
                popover.popup();
            }
        });
    }
    {
        let popover = popover.clone();
        motion.connect_leave(move |_| {
            popover.popdown();
        });
    }
    root.add_controller(motion);

    PlayerctlTooltipUi { label }
}

pub(super) fn install_carousel_hover_tracking(root: &Overlay, carousel: &PlayerctlCarouselUi) {
    let motion = EventControllerMotion::new();
    {
        let state = carousel.state.clone();
        motion.connect_enter(move |_, _, _| {
            if let Ok(mut state) = state.try_borrow_mut() {
                state.hover_active = true;
                state.last_tick = Instant::now();
            }
        });
    }
    {
        let state = carousel.state.clone();
        let area = carousel.area.clone();
        motion.connect_leave(move |_| {
            if let Ok(mut state) = state.try_borrow_mut() {
                state.hover_active = false;
                state.offset_px = 0.0;
                state.hold_until = Some(Instant::now() + Duration::from_millis(350));
                state.waiting_restart = false;
            }
            area.queue_draw();
        });
    }
    root.add_controller(motion);
}

pub(super) fn install_carousel_open_tracking(popover: &Popover, carousel: &PlayerctlCarouselUi) {
    {
        let state = carousel.state.clone();
        popover.connect_show(move |_| {
            if let Ok(mut state) = state.try_borrow_mut() {
                state.open_active = true;
                state.last_tick = Instant::now();
            }
        });
    }
    {
        let state = carousel.state.clone();
        let area = carousel.area.clone();
        popover.connect_hide(move |_| {
            if let Ok(mut state) = state.try_borrow_mut() {
                state.open_active = false;
                state.offset_px = 0.0;
                state.hold_until = Some(Instant::now() + Duration::from_millis(350));
                state.waiting_restart = false;
            }
            area.queue_draw();
        });
    }
}

pub(super) fn install_carousel_animation(carousel: PlayerctlCarouselUi) {
    const SPEED_PX_PER_SEC: f64 = 48.0;
    const END_HOLD_MS: u64 = 700;
    const RESTART_HOLD_MS: u64 = 700;

    glib::timeout_add_local(Duration::from_millis(24), move || {
        if !carousel.area.is_mapped() {
            return ControlFlow::Continue;
        }
        if matches!(carousel.marquee, PlayerctlMarqueeMode::Off) {
            return ControlFlow::Continue;
        }
        let now = Instant::now();
        let mut should_redraw = false;
        let mut should_return_early = false;

        {
            let mut state = carousel.state.borrow_mut();
            let elapsed_secs = now.saturating_duration_since(state.last_tick).as_secs_f64();
            state.last_tick = now;

            if matches!(carousel.marquee, PlayerctlMarqueeMode::Hover) && !state.hover_active {
                should_return_early = true;
            }
            if matches!(carousel.marquee, PlayerctlMarqueeMode::Open) && !state.open_active {
                should_return_early = true;
            }

            if !should_return_early
                && (state.full_text.is_empty()
                    || state.content_width_px <= state.viewport_width_px as f64)
            {
                if state.offset_px != 0.0 {
                    state.offset_px = 0.0;
                    should_redraw = true;
                }
                state.hold_until = None;
                state.waiting_restart = false;
                should_return_early = true;
            }

            if !should_return_early {
                if let Some(hold_until) = state.hold_until {
                    if now < hold_until {
                        should_return_early = true;
                    } else {
                        state.hold_until = None;
                        if state.waiting_restart {
                            state.offset_px = 0.0;
                            state.waiting_restart = false;
                            state.hold_until = Some(now + Duration::from_millis(RESTART_HOLD_MS));
                            should_redraw = true;
                            should_return_early = true;
                        }
                    }
                }
            }

            if !should_return_early {
                state.offset_px += SPEED_PX_PER_SEC * elapsed_secs;
                let loop_distance = state.content_width_px + carousel_gap_px();
                if state.offset_px >= loop_distance {
                    state.offset_px = loop_distance;
                    state.waiting_restart = true;
                    state.hold_until = Some(now + Duration::from_millis(END_HOLD_MS));
                }
                should_redraw = true;
            }
        }

        if should_redraw {
            carousel.area.queue_draw();
        }

        if should_return_early {
            return ControlFlow::Continue;
        }

        ControlFlow::Continue
    });
}

pub(super) fn build_controls_ui(root: &Overlay, show_seek: bool) -> PlayerctlControlsUi {
    root.add_css_class("clickable");
    root.add_css_class("playerctl-controls-enabled");

    let popover = Popover::new();
    popover.add_css_class("playerctl-controls-popover");
    popover.set_autohide(true);
    popover.set_has_arrow(true);
    popover.set_position(PositionType::Top);
    popover.set_parent(root);
    {
        let root = root.clone();
        let popover_for_callback = popover.clone();
        popover.connect_show(move |_| {
            // Keep controls width in lockstep with the module width.
            popover_for_callback.set_size_request(root.allocated_width().max(1), -1);
        });
    }

    let content = GtkBox::new(Orientation::Vertical, 6);
    content.add_css_class("playerctl-controls-content");
    content.set_halign(gtk::Align::Fill);
    content.set_hexpand(true);
    popover.set_child(Some(&content));

    let buttons_row = GtkBox::new(Orientation::Horizontal, 6);
    buttons_row.add_css_class("playerctl-controls-row");
    buttons_row.set_halign(gtk::Align::Center);
    content.append(&buttons_row);

    let previous_button = Button::with_label("");
    previous_button.add_css_class("playerctl-control-button");
    buttons_row.append(&previous_button);

    let play_pause_button = Button::with_label("");
    play_pause_button.add_css_class("playerctl-control-button");
    buttons_row.append(&play_pause_button);

    let next_button = Button::with_label("");
    next_button.add_css_class("playerctl-control-button");
    buttons_row.append(&next_button);

    let metadata_grid = Grid::new();
    metadata_grid.add_css_class("playerctl-controls-metadata-grid");
    metadata_grid.set_row_spacing(4);
    metadata_grid.set_column_spacing(10);
    metadata_grid.set_halign(gtk::Align::Fill);
    metadata_grid.set_hexpand(true);
    content.append(&metadata_grid);

    let (status_key, metadata_status_value) = build_controls_metadata_labels("Status");
    metadata_grid.attach(&status_key, 0, 0, 1, 1);
    metadata_grid.attach(&metadata_status_value, 1, 0, 1, 1);
    let (player_key, metadata_player_value) = build_controls_metadata_labels("Player");
    metadata_grid.attach(&player_key, 0, 1, 1, 1);
    metadata_grid.attach(&metadata_player_value, 1, 1, 1, 1);
    let (artist_key, metadata_artist_value) = build_controls_metadata_labels("Artist");
    metadata_grid.attach(&artist_key, 0, 2, 1, 1);
    metadata_grid.attach(&metadata_artist_value, 1, 2, 1, 1);
    let (album_key, metadata_album_value) = build_controls_metadata_labels("Album");
    metadata_grid.attach(&album_key, 0, 3, 1, 1);
    metadata_grid.attach(&metadata_album_value, 1, 3, 1, 1);
    let (title_key, metadata_title_value) = build_controls_metadata_labels("Title");
    metadata_grid.attach(&title_key, 0, 4, 1, 1);
    metadata_grid.attach(&metadata_title_value, 1, 4, 1, 1);

    let seek_scale = Scale::with_range(Orientation::Horizontal, 0.0, 1.0, 0.001);
    seek_scale.add_css_class("playerctl-seek-scale");
    seek_scale.set_draw_value(false);
    seek_scale.set_hexpand(true);
    seek_scale.set_sensitive(false);

    let seek_widget: Widget = seek_scale.clone().upcast();
    seek_widget.set_visible(show_seek);
    content.append(&seek_widget);

    let seek_time_row = GtkBox::new(Orientation::Horizontal, 8);
    seek_time_row.add_css_class("playerctl-seek-time-row");
    let seek_position_label = Label::new(Some("00:00"));
    seek_position_label.add_css_class("playerctl-seek-time");
    seek_position_label.set_xalign(0.0);
    seek_position_label.set_hexpand(true);
    seek_time_row.append(&seek_position_label);

    let seek_length_label = Label::new(Some("00:00"));
    seek_length_label.add_css_class("playerctl-seek-time");
    seek_length_label.set_xalign(1.0);
    seek_length_label.set_hexpand(true);
    seek_time_row.append(&seek_length_label);

    let seek_time_widget: Widget = seek_time_row.clone().upcast();
    seek_time_widget.set_visible(show_seek);
    content.append(&seek_time_widget);

    let suppress_seek_callback = Arc::new(AtomicBool::new(false));
    let seek_update_hold_until = Arc::new(std::sync::Mutex::new(None));
    let current_metadata = Arc::new(std::sync::Mutex::new(None));

    let press_gesture = GestureClick::builder().button(1).build();
    {
        let seek_update_hold_until = seek_update_hold_until.clone();
        press_gesture.connect_pressed(move |_, _, _, _| {
            if let Ok(mut slot) = seek_update_hold_until.lock() {
                *slot = Some(Instant::now() + Duration::from_secs(2));
            }
        });
    }
    {
        let seek_update_hold_until = seek_update_hold_until.clone();
        press_gesture.connect_released(move |_, _, _, _| {
            if let Ok(mut slot) = seek_update_hold_until.lock() {
                *slot = Some(Instant::now() + Duration::from_millis(300));
            }
        });
    }
    seek_scale.add_controller(press_gesture);

    PlayerctlControlsUi {
        popover,
        metadata_status_value,
        metadata_player_value,
        metadata_artist_value,
        metadata_album_value,
        metadata_title_value,
        previous_button,
        play_pause_button,
        next_button,
        seek_scale,
        seek_widget,
        seek_time_widget,
        seek_position_label,
        seek_length_label,
        suppress_seek_callback,
        seek_update_hold_until,
        current_metadata,
        show_seek,
    }
}

pub(super) fn install_controls_open_gesture(
    root: &Overlay,
    popover: &Popover,
    open_mode: PlayerctlControlsOpenMode,
) {
    match open_mode {
        PlayerctlControlsOpenMode::LeftClick => {
            let click = GestureClick::builder().button(1).build();
            let popover = popover.clone();
            click.connect_pressed(move |_, _, _, _| {
                if popover.is_visible() {
                    popover.popdown();
                } else {
                    popover.popup();
                }
            });
            root.add_controller(click);
        }
    }
}

pub(super) fn wire_controls_actions(controls_ui: PlayerctlControlsUi) {
    let current_metadata_for_previous = controls_ui.current_metadata.clone();
    controls_ui.previous_button.connect_clicked(move |_| {
        let bus_name = current_metadata_for_previous
            .lock()
            .ok()
            .and_then(|slot| slot.as_ref().map(|metadata| metadata.bus_name.clone()));
        if let Some(bus_name) = bus_name {
            std::thread::spawn(move || {
                let _ = call_player_method(&bus_name, "Previous");
            });
        }
    });

    let current_metadata_for_play_pause = controls_ui.current_metadata.clone();
    controls_ui.play_pause_button.connect_clicked(move |_| {
        let bus_name = current_metadata_for_play_pause
            .lock()
            .ok()
            .and_then(|slot| slot.as_ref().map(|metadata| metadata.bus_name.clone()));
        if let Some(bus_name) = bus_name {
            std::thread::spawn(move || {
                let _ = call_player_method(&bus_name, "PlayPause");
            });
        }
    });

    let current_metadata_for_next = controls_ui.current_metadata.clone();
    controls_ui.next_button.connect_clicked(move |_| {
        let bus_name = current_metadata_for_next
            .lock()
            .ok()
            .and_then(|slot| slot.as_ref().map(|metadata| metadata.bus_name.clone()));
        if let Some(bus_name) = bus_name {
            std::thread::spawn(move || {
                let _ = call_player_method(&bus_name, "Next");
            });
        }
    });

    let current_metadata_for_seek = controls_ui.current_metadata.clone();
    let suppress_seek_callback = controls_ui.suppress_seek_callback.clone();
    let seek_update_hold_until = controls_ui.seek_update_hold_until.clone();
    controls_ui.seek_scale.connect_value_changed(move |scale| {
        if suppress_seek_callback.load(Ordering::Relaxed) {
            return;
        }
        if let Ok(mut slot) = seek_update_hold_until.lock() {
            *slot = Some(Instant::now() + Duration::from_millis(700));
        }

        let Some(metadata) = current_metadata_for_seek
            .lock()
            .ok()
            .and_then(|slot| slot.clone())
        else {
            return;
        };

        let Some(duration_micros) = metadata.length_micros else {
            return;
        };
        if duration_micros <= 0 || !metadata.can_seek {
            return;
        }

        let Some(track_id) = metadata.track_id.clone() else {
            return;
        };

        let ratio = scale.value().clamp(0.0, 1.0);
        let target_position = ((duration_micros as f64) * ratio).round() as i64;
        let bus_name = metadata.bus_name;

        std::thread::spawn(move || {
            let _ = call_set_position(&bus_name, &track_id, target_position);
        });
    });
}

pub(super) fn refresh_controls_ui(
    controls_ui: &PlayerctlControlsUi,
    metadata: Option<&PlayerctlMetadata>,
    fallback_status_text: &str,
) {
    if let Ok(mut slot) = controls_ui.current_metadata.lock() {
        *slot = metadata.cloned();
    }

    let Some(metadata) = metadata else {
        controls_ui
            .metadata_status_value
            .set_text(fallback_status_text);
        controls_ui.metadata_player_value.set_text("—");
        controls_ui.metadata_artist_value.set_text("—");
        controls_ui.metadata_album_value.set_text("—");
        controls_ui.metadata_title_value.set_text("—");
        controls_ui.previous_button.set_sensitive(false);
        controls_ui.play_pause_button.set_sensitive(false);
        controls_ui.play_pause_button.set_label("");
        controls_ui.next_button.set_sensitive(false);
        controls_ui.seek_scale.set_sensitive(false);
        controls_ui.seek_widget.set_visible(controls_ui.show_seek);
        controls_ui
            .seek_time_widget
            .set_visible(controls_ui.show_seek);
        controls_ui.seek_position_label.set_text("00:00");
        controls_ui.seek_length_label.set_text("00:00");
        return;
    };

    controls_ui
        .previous_button
        .set_sensitive(metadata.can_go_previous);
    controls_ui.next_button.set_sensitive(metadata.can_go_next);

    let can_toggle_playback = metadata.can_play || metadata.can_pause;
    controls_ui
        .play_pause_button
        .set_sensitive(can_toggle_playback);
    let toggle_icon = if metadata.status == "playing" {
        ""
    } else {
        ""
    };
    controls_ui.play_pause_button.set_label(toggle_icon);
    controls_ui
        .metadata_status_value
        .set_text(metadata.status.as_str());
    controls_ui
        .metadata_player_value
        .set_text(metadata.player.as_str());
    controls_ui
        .metadata_artist_value
        .set_text(non_empty_or_dash(&metadata.artist));
    controls_ui
        .metadata_album_value
        .set_text(non_empty_or_dash(&metadata.album));
    controls_ui
        .metadata_title_value
        .set_text(non_empty_or_dash(&metadata.title));

    let can_seek = metadata.can_seek
        && metadata.length_micros.is_some_and(|length| length > 0)
        && metadata.track_id.is_some();
    controls_ui.seek_widget.set_visible(controls_ui.show_seek);
    controls_ui
        .seek_time_widget
        .set_visible(controls_ui.show_seek);
    controls_ui.seek_scale.set_sensitive(can_seek);

    if let Ok(mut slot) = controls_ui.seek_update_hold_until.lock() {
        if slot.is_some_and(|until| Instant::now() < until) {
            controls_ui
                .seek_position_label
                .set_text(&format_timestamp_micros(metadata.position_micros));
            controls_ui
                .seek_length_label
                .set_text(&format_timestamp_micros(metadata.length_micros));
            return;
        }
        *slot = None;
    }

    let ratio = metadata_seek_ratio(metadata).unwrap_or(0.0).clamp(0.0, 1.0);
    controls_ui
        .suppress_seek_callback
        .store(true, Ordering::Relaxed);
    controls_ui.seek_scale.set_value(ratio);
    controls_ui
        .suppress_seek_callback
        .store(false, Ordering::Relaxed);

    controls_ui
        .seek_position_label
        .set_text(&format_timestamp_micros(metadata.position_micros));
    controls_ui
        .seek_length_label
        .set_text(&format_timestamp_micros(metadata.length_micros));
}

pub(super) fn sync_controls_width(controls_ui: &PlayerctlControlsUi, module_width_px: i32) {
    let width = module_width_px.max(1);
    controls_ui.popover.set_size_request(width, -1);
    if let Some(child) = controls_ui.popover.child() {
        child.set_size_request(width, -1);
    }
}

fn reset_carousel_state(carousel: &PlayerctlCarouselUi, text: &str) {
    let layout = carousel.area.create_pango_layout(Some(text));
    let (text_width_px, text_height_px) = layout.pixel_size();
    let content_width_px = text_width_px.max(1);
    let viewport_width_px = content_width_px.min(carousel.width_limit_px);

    let mut state = carousel.state.borrow_mut();
    state.full_text = text.to_string();
    state.layout = Some(layout);
    state.content_width_px = content_width_px as f64;
    state.viewport_width_px = viewport_width_px;
    state.text_height_px = text_height_px.max(1);
    state.offset_px = 0.0;
    state.last_tick = Instant::now();
    state.hold_until = Some(Instant::now() + Duration::from_millis(900));
    state.waiting_restart = false;

    carousel.area.set_content_width(viewport_width_px);
    carousel.area.set_size_request(viewport_width_px, -1);
    carousel.root.set_size_request(viewport_width_px, -1);
}

fn width_px_for_widget(widget: &impl IsA<Widget>, width_chars: u32) -> i32 {
    let sample = "M".repeat(width_chars as usize);
    let layout = widget.create_pango_layout(Some(sample.as_str()));
    let (pixel_width, _) = layout.pixel_size();
    pixel_width.max(1)
}

fn fixed_height_px_from_label_probe(extra_classes: Option<&str>) -> i32 {
    let probe = Label::new(Some("Mg"));
    probe.add_css_class("module");
    probe.add_css_class("playerctl");
    apply_css_classes(&probe, extra_classes);
    probe.set_wrap(false);
    probe.set_single_line_mode(true);

    let (_, natural, _, _) = probe.measure(Orientation::Vertical, -1);
    natural.max(1)
}

fn carousel_gap_px() -> f64 {
    42.0
}

fn should_show_overflow_hint(
    state: &PlayerctlCarouselState,
    marquee: PlayerctlMarqueeMode,
) -> bool {
    let is_overflowing = state.content_width_px > state.viewport_width_px as f64;
    if !is_overflowing {
        return false;
    }

    match marquee {
        PlayerctlMarqueeMode::Off => true,
        PlayerctlMarqueeMode::Hover => !state.hover_active,
        PlayerctlMarqueeMode::Open => !state.open_active,
        PlayerctlMarqueeMode::Always => false,
    }
}

fn overflow_hint_width_px(area: &DrawingArea) -> i32 {
    let layout = area.create_pango_layout(Some("…"));
    let (width, _) = layout.pixel_size();
    width.max(1) + 4
}

fn render_overflow_hint(area: &DrawingArea, context: &gtk::cairo::Context, text_y: f64) {
    let hint = "…";
    let layout = area.create_pango_layout(Some(hint));
    let (hint_width, _) = layout.pixel_size();
    let x = f64::from((area.allocated_width() - hint_width - 1).max(0));
    render_layout_at(area, context, x, text_y, &layout);
}

fn build_controls_metadata_labels(key: &str) -> (Label, Label) {
    let key_label = Label::new(Some(key));
    key_label.add_css_class("playerctl-controls-metadata-key");
    key_label.set_xalign(0.0);
    key_label.set_halign(gtk::Align::Start);
    key_label.set_hexpand(false);

    let value_label = Label::new(Some("—"));
    value_label.add_css_class("playerctl-controls-metadata-value");
    value_label.set_xalign(0.0);
    value_label.set_justify(gtk::Justification::Left);
    value_label.set_halign(gtk::Align::Fill);
    value_label.set_hexpand(true);
    value_label.set_max_width_chars(1);
    value_label.set_wrap(true);
    value_label.set_wrap_mode(gtk::pango::WrapMode::WordChar);

    (key_label, value_label)
}

fn non_empty_or_dash(text: &str) -> &str {
    if text.is_empty() {
        "—"
    } else {
        text
    }
}

#[allow(deprecated)]
fn render_layout_at(
    area: &DrawingArea,
    context: &gtk::cairo::Context,
    x: f64,
    y: f64,
    layout: &gtk::pango::Layout,
) {
    gtk::render_layout(&area.style_context(), context, x, y, layout);
}
