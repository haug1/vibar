use gtk::gdk;
use gtk::prelude::*;
use gtk::{Application, ApplicationWindow, Box as GtkBox, CenterBox, Orientation};
use gtk4_layer_shell::{Edge, Layer, LayerShell};

mod config;
mod modules;
mod style;

use config::{load_config, Config};
use modules::ModuleConfig;

const APP_ID: &str = "com.example.mybar";
const CONFIG_PATH: &str = "./config.jsonc";

fn main() {
    let app = Application::builder().application_id(APP_ID).build();

    app.connect_activate(|app| {
        let config = load_config(CONFIG_PATH);
        style::load_default_css();

        let monitors = connected_monitors();
        if monitors.is_empty() {
            let window = build_window(app, &config, None);
            window.present();
            return;
        }

        for monitor in monitors {
            let window = build_window(app, &config, Some(&monitor));
            window.present();
        }
    });

    app.run();
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
        .title("mybar")
        .decorated(false)
        .build();

    window.init_layer_shell();
    window.set_layer(Layer::Top);
    window.set_anchor(Edge::Left, true);
    window.set_anchor(Edge::Right, true);
    window.set_anchor(Edge::Bottom, true);
    window.auto_exclusive_zone_enable();
    if let Some(monitor) = monitor {
        window.set_monitor(Some(monitor));
    }

    let root = CenterBox::builder()
        .orientation(Orientation::Horizontal)
        .build();
    root.add_css_class("bar");

    let left = GtkBox::new(Orientation::Horizontal, 6);
    left.add_css_class("left");

    let center = GtkBox::new(Orientation::Horizontal, 6);
    center.add_css_class("center");

    let right = GtkBox::new(Orientation::Horizontal, 6);
    right.add_css_class("right");

    build_area(&left, &config.areas.left);
    build_area(&center, &config.areas.center);
    build_area(&right, &config.areas.right);

    root.set_start_widget(Some(&left));
    root.set_center_widget(Some(&center));
    root.set_end_widget(Some(&right));

    window.set_child(Some(&root));
    window
}

fn build_area(container: &GtkBox, modules: &[ModuleConfig]) {
    for module in modules {
        match modules::build_module(module) {
            Ok(widget) => container.append(&widget),
            Err(err) => {
                eprintln!("Failed to initialize module {module:?}: {err}");
            }
        }
    }
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
