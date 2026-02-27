use std::collections::HashMap;
use std::env;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use gtk::gdk::{MemoryFormat, MemoryTexture, Texture};
use gtk::prelude::*;
use gtk::{Box as GtkBox, Button, GestureClick, IconLookupFlags, Image, Orientation, Widget};
use serde_json::Value;

use crate::modules::broadcaster::{BackendRegistry, Broadcaster};
use crate::modules::{apply_css_classes, poll_receiver_widget, ModuleBuildContext, ModuleConfig};

use super::ModuleFactory;

mod menu_dbus;
mod menu_ui;
mod sni;
mod types;

use types::{
    TrayConfig, TrayIconPixmap, TrayItemSnapshot, MIN_ICON_SIZE, MIN_POLL_INTERVAL_SECS,
    MODULE_TYPE,
};

const REFRESH_DEBOUNCE_MILLIS: u64 = 120;

#[derive(Clone)]
struct RenderedTrayItem {
    snapshot: TrayItemSnapshot,
    button: Button,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct TraySharedKey {
    icon_size: i32,
    poll_interval_secs: u32,
}

pub(crate) struct TrayFactory;

pub(crate) const FACTORY: TrayFactory = TrayFactory;

impl ModuleFactory for TrayFactory {
    fn module_type(&self) -> &'static str {
        MODULE_TYPE
    }

    fn init(&self, config: &ModuleConfig, _context: &ModuleBuildContext) -> Result<Widget, String> {
        let parsed = parse_config(config)?;
        Ok(build_tray_module(parsed).upcast())
    }
}

fn parse_config(module: &ModuleConfig) -> Result<TrayConfig, String> {
    if module.module_type != MODULE_TYPE {
        return Err(format!(
            "expected module type '{}', got '{}'",
            MODULE_TYPE, module.module_type
        ));
    }

    serde_json::from_value(Value::Object(module.config.clone()))
        .map_err(|err| format!("invalid {} module config: {err}", MODULE_TYPE))
}

fn normalized_icon_size(icon_size: i32) -> i32 {
    icon_size.max(MIN_ICON_SIZE)
}

fn normalized_poll_interval_secs(interval: u32) -> u32 {
    interval.max(MIN_POLL_INTERVAL_SECS)
}

fn tray_registry() -> &'static BackendRegistry<TraySharedKey, Broadcaster<Vec<TrayItemSnapshot>>> {
    static REGISTRY: OnceLock<BackendRegistry<TraySharedKey, Broadcaster<Vec<TrayItemSnapshot>>>> =
        OnceLock::new();
    REGISTRY.get_or_init(BackendRegistry::new)
}

fn subscribe_shared_tray(
    icon_size: i32,
    poll_interval_secs: u32,
) -> std::sync::mpsc::Receiver<Vec<TrayItemSnapshot>> {
    let key = TraySharedKey {
        icon_size,
        poll_interval_secs,
    };

    let (broadcaster, start_worker) = tray_registry().get_or_create(key.clone(), Broadcaster::new);
    let receiver = broadcaster.subscribe();

    if start_worker {
        start_tray_worker(key, broadcaster);
    }

    receiver
}

fn start_tray_worker(key: TraySharedKey, broadcaster: Arc<Broadcaster<Vec<TrayItemSnapshot>>>) {
    std::thread::spawn(move || {
        let (refresh_tx, refresh_rx) = mpsc::channel::<()>();
        sni::start_refresh_listeners(refresh_tx);

        let mut last = Vec::<TrayItemSnapshot>::new();
        let mut host_registered = false;
        let mut connection = sni::open_session_connection();

        while let Ok(()) | Err(RecvTimeoutError::Timeout) =
            refresh_rx.recv_timeout(Duration::from_secs(u64::from(key.poll_interval_secs)))
        {
            if broadcaster.subscriber_count() == 0 {
                tray_registry().remove(&key, &broadcaster);
                return;
            }

            coalesce_refresh_events(&refresh_rx, Duration::from_millis(REFRESH_DEBOUNCE_MILLIS));

            if connection.is_none() {
                connection = sni::open_session_connection();
                host_registered = false;
            }

            let snapshot = connection
                .as_ref()
                .map(|conn| sni::fetch_tray_snapshot_with_connection(conn, &mut host_registered))
                .unwrap_or_default();

            if snapshot != last {
                broadcaster.broadcast(snapshot.clone());
                last = snapshot;
            }
        }

        // refresh_rx disconnected — all listener threads exited
        tray_registry().remove(&key, &broadcaster);
    });
}

fn coalesce_refresh_events(refresh_rx: &mpsc::Receiver<()>, debounce: Duration) {
    let deadline = Instant::now() + debounce;
    loop {
        let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
            break;
        };
        match refresh_rx.recv_timeout(remaining) {
            Ok(()) => {}
            Err(RecvTimeoutError::Timeout) | Err(RecvTimeoutError::Disconnected) => break,
        }
    }
}

fn build_tray_module(config: TrayConfig) -> GtkBox {
    let container = GtkBox::new(Orientation::Horizontal, 4);
    container.add_css_class("module");
    container.add_css_class("tray");

    apply_css_classes(&container, config.class.as_deref());

    let icon_size = normalized_icon_size(config.icon_size);
    let poll_interval_secs = normalized_poll_interval_secs(config.poll_interval_secs);

    let receiver = subscribe_shared_tray(icon_size, poll_interval_secs);

    poll_receiver_widget(&container, receiver, {
        let mut current = Vec::<TrayItemSnapshot>::new();
        let mut rendered = HashMap::<String, RenderedTrayItem>::new();
        move |container, snapshot| {
            if snapshot != current {
                render_tray_items(container, &snapshot, icon_size, &mut rendered);
                current = snapshot;
            }
        }
    });

    container
}

fn render_tray_items(
    container: &GtkBox,
    items: &[TrayItemSnapshot],
    icon_size: i32,
    rendered: &mut HashMap<String, RenderedTrayItem>,
) {
    let desired_ids = items
        .iter()
        .map(|item| item.id.clone())
        .collect::<std::collections::HashSet<_>>();
    rendered.retain(|id, existing| {
        if desired_ids.contains(id) {
            true
        } else {
            container.remove(&existing.button);
            false
        }
    });

    for item in items {
        let should_replace = rendered
            .get(&item.id)
            .map(|existing| existing.snapshot != *item)
            .unwrap_or(true);

        if should_replace {
            if let Some(existing) = rendered.remove(&item.id) {
                container.remove(&existing.button);
            }

            let button = build_item_button(item, icon_size);
            rendered.insert(
                item.id.clone(),
                RenderedTrayItem {
                    snapshot: item.clone(),
                    button,
                },
            );
        }
    }

    while let Some(child) = container.first_child() {
        container.remove(&child);
    }
    for item in items {
        if let Some(existing) = rendered.get(&item.id) {
            container.append(&existing.button);
        }
    }
}

fn build_item_button(item: &TrayItemSnapshot, icon_size: i32) -> Button {
    let button = Button::new();
    button.add_css_class("tray-item");
    button.set_focusable(false);
    button.set_tooltip_text(Some(&item.title));

    let image = image_for_item(item, icon_size);
    image.set_pixel_size(icon_size);
    button.set_child(Some(&image));

    let destination = item.destination.clone();
    let path = item.path.clone();
    let click_button = button.clone();
    let click = GestureClick::builder().button(0).build();
    click.connect_pressed(move |gesture, _, x, y| {
        let current_button = gesture.current_button();
        match current_button {
            1 => sni::activate_item(destination.clone(), path.clone(), x as i32, y as i32),
            2 => {
                sni::secondary_activate_item(destination.clone(), path.clone(), x as i32, y as i32)
            }
            3 => {
                if !menu_ui::show_item_menu(&click_button, destination.clone(), path.clone()) {
                    sni::context_menu_item(destination.clone(), path.clone(), x as i32, y as i32);
                }
            }
            _ => {}
        }
    });
    button.add_controller(click);
    button
}

fn image_for_item(item: &TrayItemSnapshot, icon_size: i32) -> Image {
    if !item.icon_name.is_empty() {
        let icon_path = Path::new(&item.icon_name);
        if icon_path.is_absolute() {
            let icon_file = gtk::gio::File::for_path(icon_path);
            if let Ok(texture) = Texture::from_file(&icon_file) {
                return Image::from_paintable(Some(&texture));
            }
        }

        if icon_path.components().count() > 1 {
            for base in icon_theme_paths(item.icon_theme_path.as_deref()) {
                let candidate = base.join(icon_path);
                let icon_file = gtk::gio::File::for_path(candidate);
                if let Ok(texture) = Texture::from_file(&icon_file) {
                    return Image::from_paintable(Some(&texture));
                }
            }
        }
    }

    if let Some(display) = gtk::gdk::Display::default() {
        let icon_theme = gtk::IconTheme::for_display(&display);
        for theme_path in icon_theme_paths(item.icon_theme_path.as_deref()) {
            if !icon_theme
                .search_path()
                .iter()
                .any(|path| path == &theme_path)
            {
                icon_theme.add_search_path(theme_path);
            }
        }

        if !item.icon_name.is_empty() {
            if let Some(image) = image_from_icon_theme(&icon_theme, &item.icon_name, icon_size) {
                return image;
            }
        }
    }

    if let Some(pixmap) = item.icon_pixmap.as_ref() {
        if let Some(image) = image_from_icon_pixmap(pixmap) {
            return image;
        }
    }

    let fallback_name = if item.icon_name.is_empty() {
        "image-missing"
    } else {
        &item.icon_name
    };
    let image = Image::from_icon_name(fallback_name);
    image.set_pixel_size(icon_size);
    image
}

fn image_from_icon_pixmap(pixmap: &TrayIconPixmap) -> Option<Image> {
    let width = usize::try_from(pixmap.width).ok()?;
    let height = usize::try_from(pixmap.height).ok()?;
    let pixel_count = width.checked_mul(height)?;
    let expected_len = pixel_count.checked_mul(4)?;
    if pixmap.argb_data.len() < expected_len {
        return None;
    }

    let mut rgba = vec![0u8; expected_len];
    for (src, dst) in pixmap
        .argb_data
        .chunks_exact(4)
        .take(pixel_count)
        .zip(rgba.chunks_exact_mut(4))
    {
        dst[0] = src[1];
        dst[1] = src[2];
        dst[2] = src[3];
        dst[3] = src[0];
    }

    let rowstride = i32::try_from(width.checked_mul(4)?).ok()?;
    let bytes = gtk::glib::Bytes::from_owned(rgba);
    let texture = MemoryTexture::new(
        pixmap.width,
        pixmap.height,
        MemoryFormat::R8g8b8a8,
        &bytes,
        usize::try_from(rowstride).ok()?,
    );
    Some(Image::from_paintable(Some(&texture)))
}

fn icon_theme_paths(raw: Option<&str>) -> Vec<PathBuf> {
    raw.map(|value| env::split_paths(value).collect())
        .unwrap_or_default()
}

fn image_from_icon_theme(
    icon_theme: &gtk::IconTheme,
    icon_name: &str,
    icon_size: i32,
) -> Option<Image> {
    let base_name = icon_name.strip_suffix("-symbolic");
    let mut candidates = vec![icon_name];
    if let Some(base_name) = base_name {
        candidates.push(base_name);
    }

    for candidate in candidates {
        let flags = if candidate.ends_with("-symbolic") {
            IconLookupFlags::FORCE_SYMBOLIC
        } else {
            IconLookupFlags::empty()
        };

        let paintable = icon_theme.lookup_icon(
            candidate,
            &[],
            icon_size,
            1,
            gtk::TextDirection::None,
            flags,
        );
        let file = paintable.file().and_then(|file| file.path());
        let looked_up_name = paintable.icon_name();
        if is_missing_icon_name(looked_up_name.as_ref()) {
            continue;
        }

        if candidate.ends_with("-symbolic") && looked_up_name.is_some() {
            return Some(Image::from_paintable(Some(&paintable)));
        }

        if let Some(path) = file.as_ref() {
            let icon_file = gtk::gio::File::for_path(path);
            match Texture::from_file(&icon_file) {
                Ok(texture) => return Some(Image::from_paintable(Some(&texture))),
                Err(_) => continue,
            }
        }

        if looked_up_name.is_some() {
            return Some(Image::from_paintable(Some(&paintable)));
        }
    }

    None
}

fn is_missing_icon_name(icon_name: Option<&PathBuf>) -> bool {
    icon_name
        .and_then(|path| {
            path.file_name()
                .map(|value| value.to_string_lossy().to_string())
        })
        .is_some_and(|name| name == "image-missing")
}

#[cfg(test)]
mod tests {
    use serde_json::Map;

    use super::*;

    #[test]
    fn parse_config_rejects_wrong_module_type() {
        let module = ModuleConfig::new("clock", Map::new());
        let err = parse_config(&module).expect_err("wrong type should fail");
        assert!(err.contains("expected module type 'tray'"));
    }

    #[test]
    fn parse_item_address_parses_destination_and_path() {
        let parsed = sni::parse_item_address(":1.42/StatusNotifierItem".to_string())
            .expect("valid path should parse");
        assert_eq!(parsed.1, ":1.42");
        assert_eq!(parsed.2, "/StatusNotifierItem");
    }

    #[test]
    fn normalized_values_enforce_minimums() {
        assert_eq!(normalized_icon_size(2), MIN_ICON_SIZE);
        assert_eq!(normalized_poll_interval_secs(0), MIN_POLL_INTERVAL_SECS);
    }
}
