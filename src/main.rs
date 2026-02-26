use glib::ControlFlow;
use gtk::gdk;
use gtk::prelude::*;
use gtk::{Application, ApplicationWindow, Box as GtkBox, CenterBox, Orientation};
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::time::Duration;

mod config;
mod modules;
mod style;

use config::{load_config, Config};
use modules::{ModuleBuildContext, ModuleConfig};

const APP_ID: &str = "dev.haug1.vibar";

fn main() {
    let app = Application::builder().application_id(APP_ID).build();

    app.connect_activate(|app| {
        let loaded_config = load_config();
        style::load_styles(
            &loaded_config.config.style,
            loaded_config.source_path.as_deref(),
        );

        let windows = Rc::new(RefCell::new(HashMap::new()));
        sync_monitor_windows(app, &loaded_config.config, &windows);

        let Some(display) = gdk::Display::default() else {
            return;
        };
        let monitors = display.monitors();
        monitors.connect_items_changed({
            let app = app.clone();
            let config = loaded_config.config.clone();
            let windows = Rc::clone(&windows);
            move |_, _, _, _| {
                sync_monitor_windows(&app, &config, &windows);
            }
        });
    });

    app.run();
}

fn sync_monitor_windows(
    app: &Application,
    config: &Config,
    windows: &Rc<RefCell<HashMap<String, ApplicationWindow>>>,
) {
    let monitors = connected_monitors();
    let monitor_keys = monitors
        .iter()
        .map(|monitor| (monitor_key(monitor), monitor.clone()))
        .collect::<Vec<_>>();
    let active_keys = monitor_keys
        .iter()
        .map(|(key, _)| key.clone())
        .collect::<HashSet<_>>();

    let mut tracked_windows = windows.borrow_mut();
    let mut removed_keys = tracked_windows
        .keys()
        .filter(|key| *key != FALLBACK_WINDOW_KEY && !active_keys.contains(*key))
        .cloned()
        .collect::<Vec<_>>();
    if !monitor_keys.is_empty() && tracked_windows.contains_key(FALLBACK_WINDOW_KEY) {
        removed_keys.push(FALLBACK_WINDOW_KEY.to_string());
    }

    let mut removed_windows = Vec::new();
    for key in removed_keys {
        if let Some(window) = tracked_windows.remove(&key) {
            removed_windows.push(window);
        }
    }

    if monitor_keys.is_empty() {
        if !tracked_windows.contains_key(FALLBACK_WINDOW_KEY) {
            let window = build_window(app, config, None);
            debug_dump_dom_if_enabled(&window, None);
            window.present();
            tracked_windows.insert(FALLBACK_WINDOW_KEY.to_string(), window);
        }
        drop(tracked_windows);
        defer_close_windows(removed_windows);
        return;
    }

    for (key, monitor) in monitor_keys {
        if tracked_windows.contains_key(&key) {
            continue;
        }
        let window = build_window(app, config, Some(&monitor));
        let connector = monitor.connector().map(|value| value.to_string());
        debug_dump_dom_if_enabled(&window, connector.as_deref());
        window.present();
        tracked_windows.insert(key, window);
    }

    drop(tracked_windows);
    defer_close_windows(removed_windows);
}

const FALLBACK_WINDOW_KEY: &str = "__fallback__";

fn monitor_key(monitor: &gdk::Monitor) -> String {
    let pointer = monitor.as_ptr();
    if let Some(connector) = monitor.connector() {
        return format!("connector:{connector}|ptr:{pointer:p}");
    }
    format!("ptr:{pointer:p}")
}

fn defer_close_windows(removed_windows: Vec<ApplicationWindow>) {
    if removed_windows.is_empty() {
        return;
    }

    glib::idle_add_local_once(move || {
        for window in removed_windows {
            window.close();
        }
    });
}

fn connected_monitors() -> Vec<gdk::Monitor> {
    let Some(display) = gdk::Display::default() else {
        return Vec::new();
    };
    let monitors = display.monitors();

    (0..monitors.n_items())
        .filter_map(|idx| monitors.item(idx))
        .filter_map(|obj| obj.downcast::<gdk::Monitor>().ok())
        .collect()
}

fn build_window(
    app: &Application,
    config: &Config,
    monitor: Option<&gdk::Monitor>,
) -> ApplicationWindow {
    let window = ApplicationWindow::builder()
        .application(app)
        .title("vibar")
        .decorated(false)
        .build();
    window.add_css_class("vibar-window");

    window.init_layer_shell();
    window.set_layer(Layer::Top);
    window.set_keyboard_mode(KeyboardMode::None);
    window.set_anchor(Edge::Left, true);
    window.set_anchor(Edge::Right, true);
    window.set_anchor(Edge::Bottom, true);
    window.auto_exclusive_zone_enable();
    window.set_focusable(false);
    window.set_focus_on_click(false);
    if let Some(monitor) = monitor {
        window.set_monitor(Some(monitor));
    }

    let root = CenterBox::builder()
        .orientation(Orientation::Horizontal)
        .build();
    root.add_css_class("bar");
    root.set_focusable(false);
    root.set_focus_on_click(false);

    let left = GtkBox::new(Orientation::Horizontal, 6);
    left.add_css_class("left");
    left.set_focusable(false);
    left.set_focus_on_click(false);

    let center = GtkBox::new(Orientation::Horizontal, 6);
    center.add_css_class("center");
    center.set_focusable(false);
    center.set_focus_on_click(false);

    let right = GtkBox::new(Orientation::Horizontal, 6);
    right.add_css_class("right");
    right.set_focusable(false);
    right.set_focus_on_click(false);

    let context = ModuleBuildContext {
        monitor_connector: monitor
            .and_then(|item| item.connector())
            .map(|connector| connector.to_string()),
    };

    build_area(&left, &config.areas.left, &context);
    build_area(&center, &config.areas.center, &context);
    build_area(&right, &config.areas.right, &context);

    root.set_start_widget(Some(&left));
    root.set_center_widget(Some(&center));
    root.set_end_widget(Some(&right));

    window.set_child(Some(&root));
    window
}

fn build_area(container: &GtkBox, modules: &[ModuleConfig], context: &ModuleBuildContext) {
    for module in modules {
        match modules::build_module(module, context) {
            Ok(widget) => container.append(&widget),
            Err(err) => {
                eprintln!("Failed to initialize module {module:?}: {err}");
            }
        }
    }
}

fn debug_dump_dom_if_enabled(window: &ApplicationWindow, connector: Option<&str>) {
    if !dom_debug_enabled() {
        return;
    }

    let monitor_name = connector.unwrap_or("unknown").to_string();
    dump_dom_snapshot(window, &monitor_name);

    let interval_secs = dom_debug_interval_secs();
    let window_weak = window.downgrade();
    glib::timeout_add_local(Duration::from_secs(interval_secs), {
        let monitor_name = monitor_name.clone();
        move || {
            let Some(window) = window_weak.upgrade() else {
                return ControlFlow::Break;
            };
            dump_dom_snapshot(&window, &monitor_name);
            ControlFlow::Continue
        }
    });
}

fn debug_dump_widget_tree(widget: &gtk::Widget, depth: usize) {
    let indent = "  ".repeat(depth);
    let classes = widget
        .css_classes()
        .into_iter()
        .map(|s| s.to_string())
        .collect::<Vec<_>>()
        .join(" ");
    let type_name = widget.type_().name();

    if classes.is_empty() {
        eprintln!("vibar/dom: {indent}{type_name}");
    } else {
        eprintln!("vibar/dom: {indent}{type_name} .{classes}");
    }

    let mut child = widget.first_child();
    while let Some(current) = child {
        debug_dump_widget_tree(&current, depth + 1);
        child = current.next_sibling();
    }
}

fn dom_debug_enabled() -> bool {
    std::env::var("VIBAR_DEBUG_DOM")
        .map(|v| matches!(v.to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on"))
        .unwrap_or(false)
}

fn dom_debug_interval_secs() -> u64 {
    std::env::var("VIBAR_DEBUG_DOM_INTERVAL_SECS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|value| *value >= 1)
        .unwrap_or(10)
}

fn dump_dom_snapshot(window: &ApplicationWindow, monitor_name: &str) {
    eprintln!("vibar/dom: monitor={monitor_name}");
    let root: gtk::Widget = window.clone().upcast();
    debug_dump_widget_tree(&root, 0);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_config_defaults_to_builtin_areas() {
        let cfg = config::parse_config("{}").expect("config should parse");
        assert_eq!(cfg.areas.left.len(), 1);
        assert_eq!(cfg.areas.center.len(), 0);
        assert_eq!(cfg.areas.right.len(), 1);
    }

    #[test]
    fn parse_exec_module_uses_default_interval() {
        let cfg =
            config::parse_config(r#"{ areas: { left: [{ type: "exec", command: "echo ok" }] } }"#)
                .expect("config should parse");

        let exec_cfg =
            modules::exec::parse_config(&cfg.areas.left[0]).expect("exec config expected");
        assert_eq!(exec_cfg.interval_secs, 5);
    }

    #[test]
    fn normalized_exec_interval_enforces_lower_bound() {
        assert_eq!(modules::exec::normalized_exec_interval(0), 1);
        assert_eq!(modules::exec::normalized_exec_interval(1), 1);
        assert_eq!(modules::exec::normalized_exec_interval(10), 10);
    }
}
