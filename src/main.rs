use gtk::gdk;
use gtk::glib::ControlFlow;
use gtk::prelude::*;
use gtk::{Application, ApplicationWindow, Box as GtkBox, CenterBox, Orientation};
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

mod config;
mod modules;
mod style;

use config::{load_config, parse_config, Config, LoadedConfig};
use modules::{ModuleBuildContext, ModuleConfig};

const APP_ID: &str = "dev.haug1.vibar";
const CONFIG_RELOAD_DEBOUNCE_MILLIS: u64 = 200;

struct AppRuntime {
    app: Application,
    windows: Rc<RefCell<HashMap<String, ApplicationWindow>>>,
    config: Rc<RefCell<Config>>,
    config_source_path: RefCell<Option<PathBuf>>,
    style_runtime: RefCell<Option<Rc<style::StyleRuntime>>>,
    _monitor_model: gtk::gio::ListModel,
    _config_monitor: RefCell<Option<gtk::gio::FileMonitor>>,
    config_reload_source: RefCell<Option<gtk::glib::SourceId>>,
}

impl AppRuntime {
    fn sync_windows(&self) {
        sync_monitor_windows(&self.app, &self.config, &self.windows);
    }

    fn rebuild_windows(&self) {
        let removed_windows = {
            let mut tracked_windows = self.windows.borrow_mut();
            tracked_windows.drain().map(|(_, window)| window).collect()
        };
        close_windows_now(removed_windows);
        self.sync_windows();
    }

    fn apply_loaded_config(self: &Rc<Self>, loaded_config: LoadedConfig) {
        *self.config.borrow_mut() = loaded_config.config;
        *self.config_source_path.borrow_mut() = loaded_config.source_path;

        let style_runtime = {
            let config = self.config.borrow();
            style::StyleRuntime::install(&config.style, self.config_source_path.borrow().as_deref())
        };
        *self.style_runtime.borrow_mut() = style_runtime;

        self.install_config_watch();
        self.rebuild_windows();
    }

    fn reload_config_from_source(self: &Rc<Self>) {
        let Some(path) = self.config_source_path.borrow().clone() else {
            return;
        };
        let content = match fs::read_to_string(&path) {
            Ok(content) => content,
            Err(err) => {
                eprintln!("Failed to read config file {}: {err}", path.display());
                return;
            }
        };

        let parsed = match parse_config(&content) {
            Ok(config) => config,
            Err(err) => {
                eprintln!("Failed to parse {}: {err}", path.display());
                return;
            }
        };

        self.apply_loaded_config(LoadedConfig {
            config: parsed,
            source_path: Some(path),
        });
    }

    fn schedule_config_reload(self: &Rc<Self>) {
        if self.config_reload_source.borrow().is_some() {
            return;
        }

        let weak_runtime = Rc::downgrade(self);
        let source_id = gtk::glib::timeout_add_local_once(
            Duration::from_millis(CONFIG_RELOAD_DEBOUNCE_MILLIS),
            move || {
                let Some(runtime) = weak_runtime.upgrade() else {
                    return;
                };
                runtime.config_reload_source.borrow_mut().take();
                runtime.reload_config_from_source();
            },
        );
        *self.config_reload_source.borrow_mut() = Some(source_id);
    }

    fn install_config_watch(self: &Rc<Self>) {
        self._config_monitor.borrow_mut().take();

        let Some(path) = self.config_source_path.borrow().clone() else {
            return;
        };

        let file = gtk::gio::File::for_path(&path);
        let monitor = match file.monitor_file(
            gtk::gio::FileMonitorFlags::NONE,
            gtk::gio::Cancellable::NONE,
        ) {
            Ok(monitor) => monitor,
            Err(err) => {
                eprintln!("Failed to watch config file {}: {err}", path.display());
                return;
            }
        };

        let weak_runtime = Rc::downgrade(self);
        monitor.connect_changed(move |_, _, _, _| {
            if let Some(runtime) = weak_runtime.upgrade() {
                runtime.schedule_config_reload();
            }
        });
        *self._config_monitor.borrow_mut() = Some(monitor);
    }
}

fn main() {
    let app = Application::builder()
        .application_id(APP_ID)
        .flags(gtk::gio::ApplicationFlags::NON_UNIQUE)
        .build();

    app.connect_activate(|app| {
        let loaded_config = load_config();
        let initial_style_runtime = style::StyleRuntime::install(
            &loaded_config.config.style,
            loaded_config.source_path.as_deref(),
        );
        let current_config = Rc::new(RefCell::new(loaded_config.config.clone()));

        let windows = Rc::new(RefCell::new(HashMap::new()));
        sync_monitor_windows(app, &current_config, &windows);

        let Some(display) = gdk::Display::default() else {
            return;
        };
        let monitor_model = display.monitors();
        monitor_model.connect_items_changed({
            let app = app.clone();
            let config = Rc::clone(&current_config);
            let windows = Rc::clone(&windows);
            move |_, _, _, _| {
                sync_monitor_windows(&app, &config, &windows);
            }
        });

        let app_runtime = Rc::new(AppRuntime {
            app: app.clone(),
            windows,
            config: current_config,
            config_source_path: RefCell::new(loaded_config.source_path),
            style_runtime: RefCell::new(initial_style_runtime),
            _monitor_model: monitor_model,
            _config_monitor: RefCell::new(None),
            config_reload_source: RefCell::new(None),
        });
        app_runtime.install_config_watch();
        let app_runtime_for_shutdown = Rc::clone(&app_runtime);
        app.connect_shutdown(move |_| {
            let _ = &app_runtime_for_shutdown;
        });
    });

    app.run();
}

fn sync_monitor_windows(
    app: &Application,
    config: &Rc<RefCell<Config>>,
    windows: &Rc<RefCell<HashMap<String, ApplicationWindow>>>,
) {
    let config_snapshot = config.borrow().clone();
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
            let window = build_window(app, &config_snapshot, None);
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

        attach_monitor_connector_resolve_once(&monitor, app, config, windows);

        let window = build_window(app, &config_snapshot, Some(&monitor));
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

fn attach_monitor_connector_resolve_once(
    monitor: &gdk::Monitor,
    app: &Application,
    config: &Rc<RefCell<Config>>,
    windows: &Rc<RefCell<HashMap<String, ApplicationWindow>>>,
) {
    if monitor.connector().is_some() {
        return;
    }

    let monitor = monitor.clone();
    let handler_id = Rc::new(RefCell::new(None));
    let handler_id_for_cb = Rc::clone(&handler_id);
    let monitor_for_cb = monitor.clone();
    let id = monitor.connect_connector_notify({
        let app = app.clone();
        let config = Rc::clone(config);
        let windows = Rc::clone(windows);
        move |item| {
            if item.connector().is_none() {
                return;
            }

            sync_monitor_windows(&app, &config, &windows);

            if let Some(id) = handler_id_for_cb.borrow_mut().take() {
                monitor_for_cb.disconnect(id);
            }
        }
    });
    *handler_id.borrow_mut() = Some(id);
}

fn defer_close_windows(removed_windows: Vec<ApplicationWindow>) {
    if removed_windows.is_empty() {
        return;
    }

    gtk::glib::idle_add_local_once(move || {
        for window in removed_windows {
            window.close();
        }
    });
}

fn close_windows_now(removed_windows: Vec<ApplicationWindow>) {
    if removed_windows.is_empty() {
        return;
    }

    for window in removed_windows {
        window.close();
    }
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
        monitor: monitor.cloned(),
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
    gtk::glib::timeout_add_local(Duration::from_secs(interval_secs), {
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
