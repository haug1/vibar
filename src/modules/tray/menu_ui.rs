use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use gtk::prelude::*;
use gtk::{
    Box as GtkBox, Button, IconLookupFlags, Image, Label, Orientation, Popover, PositionType,
    Separator,
};

use super::menu_dbus::{fetch_dbus_menu_model, send_menu_event};
use super::types::{TrayMenuEntry, DEFAULT_ICON_SIZE};

pub(super) fn show_item_menu(anchor: &Button, destination: String, path: String) -> bool {
    let Some(model) = fetch_dbus_menu_model(&destination, &path) else {
        return false;
    };

    if model.entries.is_empty() {
        return false;
    }

    if !has_visible_menu_entries(&model.entries) {
        return false;
    }

    let popover = Popover::new();
    popover.add_css_class("tray-menu-popover");
    popover.set_has_arrow(true);
    popover.set_autohide(true);
    popover.set_position(PositionType::Top);
    popover.set_parent(anchor);
    let content = GtkBox::new(Orientation::Vertical, 2);
    content.add_css_class("tray-menu-content");
    popover.set_child(Some(&content));

    let levels = Rc::new(RefCell::new(vec![model.entries]));
    render_menu_level(&content, &popover, &destination, &model.menu_path, &levels);
    popover.popup();

    true
}

fn has_visible_menu_entries(entries: &[TrayMenuEntry]) -> bool {
    entries
        .iter()
        .any(|entry| entry.visible && !entry.is_separator)
}

fn image_from_icon_data(data: &[u8]) -> Option<Image> {
    let loader = gtk::gdk_pixbuf::PixbufLoader::new();
    loader.write(data).ok()?;
    loader.close().ok()?;
    let pixbuf = loader.pixbuf()?;
    let texture = gtk::gdk::Texture::for_pixbuf(&pixbuf);
    let image = Image::from_paintable(Some(&texture));
    image.set_pixel_size(DEFAULT_ICON_SIZE);
    Some(image)
}

fn image_from_icon_name(icon_name: &str) -> Option<Image> {
    let display = gtk::gdk::Display::default()?;

    let mut themes = vec![gtk::IconTheme::for_display(&display)];
    for theme_name in ["Adwaita", "hicolor"] {
        let theme = gtk::IconTheme::new();
        theme.set_display(Some(&display));
        theme.set_theme_name(Some(theme_name));
        themes.push(theme);
    }

    let mut candidates = vec![icon_name.to_string()];
    if let Some(base) = icon_name.strip_suffix("-symbolic") {
        candidates.push(base.to_string());
    }

    for theme in themes {
        for candidate in &candidates {
            let flags = if candidate.ends_with("-symbolic") {
                IconLookupFlags::FORCE_SYMBOLIC
            } else {
                IconLookupFlags::empty()
            };
            let paintable = theme.lookup_icon(
                candidate.as_str(),
                &[],
                DEFAULT_ICON_SIZE,
                1,
                gtk::TextDirection::None,
                flags,
            );
            if is_missing_icon_name(paintable.icon_name().as_ref()) {
                continue;
            }

            let image = Image::from_paintable(Some(&paintable));
            image.set_pixel_size(DEFAULT_ICON_SIZE);
            return Some(image);
        }
    }

    None
}

fn is_missing_icon_name(icon_name: Option<&PathBuf>) -> bool {
    icon_name
        .and_then(|path| {
            path.file_name()
                .map(|name| name.to_string_lossy().to_string())
        })
        .is_some_and(|name| name == "image-missing")
}

fn render_menu_level(
    container: &GtkBox,
    popover: &Popover,
    destination: &str,
    menu_path: &str,
    levels: &Rc<RefCell<Vec<Vec<TrayMenuEntry>>>>,
) {
    while let Some(child) = container.first_child() {
        container.remove(&child);
    }

    let current_level = {
        let borrowed = levels.borrow();
        borrowed.last().cloned().unwrap_or_default()
    };

    if levels.borrow().len() > 1 {
        let back = Button::new();
        back.add_css_class("tray-menu-item");
        let row = GtkBox::new(Orientation::Horizontal, 8);
        let icon = Image::from_icon_name("go-previous-symbolic");
        icon.set_pixel_size(DEFAULT_ICON_SIZE);
        row.append(&icon);
        let label = Label::new(Some("Back"));
        label.set_xalign(0.0);
        label.set_hexpand(true);
        row.append(&label);
        back.set_child(Some(&row));

        let container_clone = container.clone();
        let popover_clone = popover.clone();
        let destination_clone = destination.to_string();
        let menu_path_clone = menu_path.to_string();
        let levels_clone = levels.clone();
        back.connect_clicked(move |_| {
            {
                let mut borrowed = levels_clone.borrow_mut();
                if borrowed.len() > 1 {
                    borrowed.pop();
                }
            }
            render_menu_level(
                &container_clone,
                &popover_clone,
                &destination_clone,
                &menu_path_clone,
                &levels_clone,
            );
        });
        container.append(&back);

        let separator = Separator::new(Orientation::Horizontal);
        container.append(&separator);
    }

    let mut previous_was_separator = true;
    for entry in current_level {
        if !entry.visible {
            continue;
        }

        if entry.is_separator {
            if previous_was_separator {
                continue;
            }
            let separator = Separator::new(Orientation::Horizontal);
            container.append(&separator);
            previous_was_separator = true;
            continue;
        }

        let button = Button::new();
        button.add_css_class("tray-menu-item");
        button.set_sensitive(entry.enabled);

        let row = GtkBox::new(Orientation::Horizontal, 8);
        if let Some(icon) = entry
            .icon_name
            .as_deref()
            .and_then(image_from_icon_name)
            .or_else(|| entry.icon_data.as_deref().and_then(image_from_icon_data))
        {
            row.append(&icon);
        }
        let label = Label::new(Some(&entry.label));
        label.set_xalign(0.0);
        label.set_hexpand(true);
        row.append(&label);
        if !entry.children.is_empty() {
            let chevron = Label::new(Some("›"));
            row.append(&chevron);
        }
        button.set_child(Some(&row));

        if !entry.children.is_empty() {
            let children = entry.children.clone();
            let container_clone = container.clone();
            let popover_clone = popover.clone();
            let destination_clone = destination.to_string();
            let menu_path_clone = menu_path.to_string();
            let levels_clone = levels.clone();
            button.connect_clicked(move |_| {
                levels_clone.borrow_mut().push(children.clone());
                render_menu_level(
                    &container_clone,
                    &popover_clone,
                    &destination_clone,
                    &menu_path_clone,
                    &levels_clone,
                );
            });
        } else {
            let destination_clone = destination.to_string();
            let menu_path_clone = menu_path.to_string();
            let popover_clone = popover.clone();
            let id = entry.id;
            button.connect_clicked(move |_| {
                send_menu_event(destination_clone.clone(), menu_path_clone.clone(), id);
                popover_clone.popdown();
            });
        }

        container.append(&button);
        previous_was_separator = false;
    }
}
