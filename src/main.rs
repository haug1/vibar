use std::fs;
use std::path::Path;
use std::process::Command;

use chrono::Local;
use glib::ControlFlow;
use gtk::gdk;
use gtk::prelude::*;
use gtk::{Application, ApplicationWindow, Box as GtkBox, Button, CenterBox, Label, Orientation};
use gtk4_layer_shell::{Edge, Layer, LayerShell};
use serde::Deserialize;
use swayipc::Connection;

const APP_ID: &str = "com.example.mybar";
const CONFIG_PATH: &str = "./config.jsonc";
const DEFAULT_CLOCK_FMT: &str = "%a %d. %b %H:%M:%S";

#[derive(Debug, Deserialize, Clone)]
struct Config {
    #[serde(default)]
    areas: Areas,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            areas: Areas::default(),
        }
    }
}

#[derive(Debug, Deserialize, Clone)]
struct Areas {
    #[serde(default = "default_left")]
    left: Vec<ModuleConfig>,
    #[serde(default)]
    center: Vec<ModuleConfig>,
    #[serde(default = "default_right")]
    right: Vec<ModuleConfig>,
}

impl Default for Areas {
    fn default() -> Self {
        Self {
            left: default_left(),
            center: Vec::new(),
            right: default_right(),
        }
    }
}

fn default_left() -> Vec<ModuleConfig> {
    vec![ModuleConfig::Workspaces]
}

fn default_right() -> Vec<ModuleConfig> {
    vec![ModuleConfig::Clock { format: None }]
}

#[derive(Debug, Deserialize, Clone)]
#[serde(tag = "type", rename_all = "kebab-case")]
enum ModuleConfig {
    Exec {
        command: String,
        #[serde(default = "default_exec_interval")]
        interval_secs: u32,
        #[serde(default)]
        class: Option<String>,
    },
    Workspaces,
    Clock {
        #[serde(default)]
        format: Option<String>,
    },
}

fn default_exec_interval() -> u32 {
    5
}

fn main() {
    let app = Application::builder().application_id(APP_ID).build();

    app.connect_activate(|app| {
        let config = load_config(CONFIG_PATH);
        let window = build_window(app, &config);
        load_optional_css();
        window.present();
    });

    app.run();
}

fn load_config(path: &str) -> Config {
    match fs::read_to_string(path) {
        Ok(content) => match json5::from_str::<Config>(&content) {
            Ok(cfg) => cfg,
            Err(err) => {
                eprintln!("Failed to parse {path}: {err}");
                Config::default()
            }
        },
        Err(_) => Config::default(),
    }
}

fn load_optional_css() {
    let css_path = Path::new("./style.css");
    if !css_path.exists() {
        return;
    }

    let provider = gtk::CssProvider::new();
    provider.load_from_path(css_path);

    if let Some(display) = gdk::Display::default() {
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
}

fn build_window(app: &Application, config: &Config) -> ApplicationWindow {
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

    let root = CenterBox::builder().orientation(Orientation::Horizontal).build();
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
        match module {
            ModuleConfig::Exec {
                command,
                interval_secs,
                class,
            } => {
                let widget = build_exec_module(command.clone(), *interval_secs, class.clone());
                container.append(&widget);
            }
            ModuleConfig::Workspaces => {
                let widget = build_workspaces_module();
                container.append(&widget);
            }
            ModuleConfig::Clock { format } => {
                let widget = build_clock_module(format.clone());
                container.append(&widget);
            }
        }
    }
}

fn build_exec_module(command: String, interval_secs: u32, class: Option<String>) -> Label {
    let label = Label::new(None);
    label.add_css_class("module");
    label.add_css_class("exec");

    if let Some(class_name) = class {
        label.add_css_class(&class_name);
    }

    let (sender, receiver) = std::sync::mpsc::channel::<String>();

    glib::timeout_add_local(std::time::Duration::from_millis(200), {
        let label = label.clone();
        move || {
            while let Ok(text) = receiver.try_recv() {
                label.set_text(&text);
            }
            ControlFlow::Continue
        }
    });

    trigger_exec_command(command.clone(), sender.clone());

    glib::timeout_add_seconds_local(interval_secs, move || {
        trigger_exec_command(command.clone(), sender.clone());
        ControlFlow::Continue
    });

    label
}

fn trigger_exec_command(command: String, sender: std::sync::mpsc::Sender<String>) {
    std::thread::spawn(move || {
        let text = match Command::new("sh").arg("-c").arg(&command).output() {
            Ok(output) => {
                let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
                let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();

                if !stdout.is_empty() {
                    stdout
                } else if !stderr.is_empty() {
                    stderr
                } else {
                    String::new()
                }
            }
            Err(err) => format!("exec error: {err}"),
        };

        let _ = sender.send(text);
    });
}

fn build_workspaces_module() -> GtkBox {
    let container = GtkBox::new(Orientation::Horizontal, 4);
    container.add_css_class("module");
    container.add_css_class("workspaces");

    refresh_workspaces(&container);

    glib::timeout_add_seconds_local(1, {
        let container = container.clone();
        move || {
            refresh_workspaces(&container);
            ControlFlow::Continue
        }
    });

    container
}

fn refresh_workspaces(container: &GtkBox) {
    while let Some(child) = container.first_child() {
        container.remove(&child);
    }

    let mut connection = match Connection::new() {
        Ok(conn) => conn,
        Err(_) => {
            let fallback = Label::new(Some("sway?"));
            fallback.add_css_class("workspace-status");
            container.append(&fallback);
            return;
        }
    };

    let mut workspaces = match connection.get_workspaces() {
        Ok(items) => items,
        Err(_) => {
            let fallback = Label::new(Some("sway?"));
            fallback.add_css_class("workspace-status");
            container.append(&fallback);
            return;
        }
    };

    workspaces.sort_by_key(|w| w.num);

    for ws in workspaces {
        let button = Button::with_label(&ws.name);
        button.add_css_class("menu-button");

        if ws.focused {
            button.add_css_class("active");
        }

        let ws_name = ws.name.clone();
        button.connect_clicked(move |_| {
            let _ = Command::new("swaymsg")
                .arg("workspace")
                .arg(ws_name.clone())
                .output();
        });

        container.append(&button);
    }
}

fn build_clock_module(format: Option<String>) -> Label {
    let label = Label::new(None);
    label.add_css_class("module");
    label.add_css_class("clock");

    let fmt = format.unwrap_or_else(|| DEFAULT_CLOCK_FMT.to_string());

    let update = {
        let label = label.clone();
        let fmt = fmt.clone();
        move || {
            let now = Local::now();
            label.set_text(&now.format(&fmt).to_string());
        }
    };

    update();

    glib::timeout_add_seconds_local(1, move || {
        update();
        ControlFlow::Continue
    });

    label
}
