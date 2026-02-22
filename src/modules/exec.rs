use std::collections::HashMap;
use std::process::Command;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use glib::ControlFlow;
use gtk::prelude::*;
use gtk::{Label, Widget};
use serde::Deserialize;
use serde_json::Value;

use crate::modules::{
    apply_css_classes, attach_primary_click_command, ModuleBuildContext, ModuleConfig,
};

use super::ModuleFactory;

const MIN_EXEC_INTERVAL_SECS: u32 = 1;
pub(crate) const MODULE_TYPE: &str = "exec";

#[derive(Debug, Deserialize, Clone)]
pub(crate) struct ExecConfig {
    pub(crate) command: String,
    #[serde(default)]
    pub(crate) click: Option<String>,
    #[serde(rename = "on-click", default)]
    pub(crate) on_click: Option<String>,
    #[serde(default = "default_exec_interval")]
    pub(crate) interval_secs: u32,
    #[serde(default)]
    pub(crate) class: Option<String>,
}

fn default_exec_interval() -> u32 {
    5
}

pub(crate) struct ExecFactory;

pub(crate) const FACTORY: ExecFactory = ExecFactory;

impl ModuleFactory for ExecFactory {
    fn module_type(&self) -> &'static str {
        MODULE_TYPE
    }

    fn init(&self, config: &ModuleConfig, _context: &ModuleBuildContext) -> Result<Widget, String> {
        let parsed = parse_config(config)?;
        let click_command = parsed.click.or(parsed.on_click);
        Ok(build_exec_module(
            parsed.command,
            click_command,
            parsed.interval_secs,
            parsed.class,
        )
        .upcast())
    }
}

pub(crate) fn parse_config(module: &ModuleConfig) -> Result<ExecConfig, String> {
    if module.module_type != MODULE_TYPE {
        return Err(format!(
            "expected module type '{}', got '{}'",
            MODULE_TYPE, module.module_type
        ));
    }

    serde_json::from_value(Value::Object(module.config.clone()))
        .map_err(|err| format!("invalid {} module config: {err}", MODULE_TYPE))
}

pub(crate) fn build_exec_module(
    command: String,
    click_command: Option<String>,
    interval_secs: u32,
    class: Option<String>,
) -> Label {
    let label = Label::new(None);
    label.add_css_class("module");
    label.add_css_class("exec");
    let effective_interval_secs = normalized_exec_interval(interval_secs);

    if effective_interval_secs != interval_secs {
        eprintln!(
            "exec interval_secs={} is too low; clamping to {} second",
            interval_secs, effective_interval_secs
        );
    }

    apply_css_classes(&label, class.as_deref());

    attach_primary_click_command(&label, click_command);

    let receiver = subscribe_shared_exec_output(command, effective_interval_secs);

    glib::timeout_add_local(std::time::Duration::from_millis(200), {
        let label = label.clone();
        let mut active_dynamic_classes: Vec<String> = Vec::new();
        move || {
            while let Ok(rendered) = receiver.try_recv() {
                label.set_text(&rendered.text);
                for class_name in &active_dynamic_classes {
                    label.remove_css_class(class_name);
                }
                for class_name in &rendered.classes {
                    label.add_css_class(class_name);
                }
                active_dynamic_classes = rendered.classes;
            }
            ControlFlow::Continue
        }
    });

    label
}

pub(crate) fn normalized_exec_interval(interval_secs: u32) -> u32 {
    interval_secs.max(MIN_EXEC_INTERVAL_SECS)
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct ExecSharedKey {
    command: String,
    interval_secs: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct ExecRenderedOutput {
    text: String,
    classes: Vec<String>,
}

#[derive(Default)]
struct SharedExecBackend {
    latest: Mutex<Option<ExecRenderedOutput>>,
    subscribers: Mutex<Vec<std::sync::mpsc::Sender<ExecRenderedOutput>>>,
}

impl SharedExecBackend {
    fn add_subscriber(&self, sender: std::sync::mpsc::Sender<ExecRenderedOutput>) {
        if let Some(text) = self
            .latest
            .lock()
            .expect("exec backend latest mutex poisoned")
            .clone()
        {
            let _ = sender.send(text);
        }

        self.subscribers
            .lock()
            .expect("exec backend subscribers mutex poisoned")
            .push(sender);
    }

    fn broadcast(&self, text: ExecRenderedOutput) {
        *self
            .latest
            .lock()
            .expect("exec backend latest mutex poisoned") = Some(text.clone());

        self.subscribers
            .lock()
            .expect("exec backend subscribers mutex poisoned")
            .retain(|sender| sender.send(text.clone()).is_ok());
    }
}

type SharedExecMap = HashMap<ExecSharedKey, Arc<SharedExecBackend>>;

fn shared_exec_backends() -> &'static Mutex<SharedExecMap> {
    static SHARED_EXEC_BACKENDS: OnceLock<Mutex<SharedExecMap>> = OnceLock::new();
    SHARED_EXEC_BACKENDS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn subscribe_shared_exec_output(
    command: String,
    interval_secs: u32,
) -> std::sync::mpsc::Receiver<ExecRenderedOutput> {
    let key = ExecSharedKey {
        command,
        interval_secs,
    };
    let backend = {
        let mut backends = shared_exec_backends()
            .lock()
            .expect("exec backend map mutex poisoned");

        if let Some(existing) = backends.get(&key) {
            Arc::clone(existing)
        } else {
            let backend = Arc::new(SharedExecBackend::default());
            start_shared_exec_worker(key.clone(), Arc::clone(&backend));
            backends.insert(key, Arc::clone(&backend));
            backend
        }
    };

    let (sender, receiver) = std::sync::mpsc::channel::<ExecRenderedOutput>();
    backend.add_subscriber(sender);
    receiver
}

fn start_shared_exec_worker(key: ExecSharedKey, backend: Arc<SharedExecBackend>) {
    std::thread::spawn(move || loop {
        backend.broadcast(run_exec_command(&key.command));
        std::thread::sleep(Duration::from_secs(u64::from(key.interval_secs)));
    });
}

fn run_exec_command(command: &str) -> ExecRenderedOutput {
    match Command::new("sh").arg("-c").arg(command).output() {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout).to_string();
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();

            if !stdout.trim().is_empty() {
                parse_exec_output(&stdout)
            } else if !stderr.trim().is_empty() {
                ExecRenderedOutput {
                    text: stderr.trim().to_string(),
                    classes: Vec::new(),
                }
            } else {
                ExecRenderedOutput::default()
            }
        }
        Err(err) => ExecRenderedOutput {
            text: format!("exec error: {err}"),
            classes: Vec::new(),
        },
    }
}

fn parse_exec_output(raw: &str) -> ExecRenderedOutput {
    let trimmed = raw.trim_end_matches(&['\r', '\n'][..]);
    if trimmed.is_empty() {
        return ExecRenderedOutput::default();
    }

    if let Ok(value) = serde_json::from_str::<Value>(trimmed) {
        return parse_json_exec_output(value);
    }

    parse_i3blocks_exec_output(trimmed)
}

fn parse_json_exec_output(value: Value) -> ExecRenderedOutput {
    let text = value
        .get("text")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let classes = value
        .get("class")
        .map(parse_json_classes)
        .unwrap_or_default();

    ExecRenderedOutput { text, classes }
}

fn parse_json_classes(class_value: &Value) -> Vec<String> {
    match class_value {
        Value::String(class_name) => split_classes(class_name),
        Value::Array(items) => items
            .iter()
            .filter_map(Value::as_str)
            .flat_map(split_classes)
            .collect(),
        _ => Vec::new(),
    }
}

fn parse_i3blocks_exec_output(raw: &str) -> ExecRenderedOutput {
    let lines: Vec<&str> = raw
        .split('\n')
        .map(|line| line.trim_end_matches('\r'))
        .collect();
    let text = lines.first().copied().unwrap_or_default().to_string();
    let classes = if lines.len() >= 3 {
        split_classes(lines[2])
    } else {
        Vec::new()
    };

    ExecRenderedOutput { text, classes }
}

fn split_classes(raw: &str) -> Vec<String> {
    raw.split_whitespace()
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>()
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;
    use std::time::Duration;

    use serde_json::json;
    use serde_json::Map;

    use super::*;

    #[test]
    fn parse_config_rejects_wrong_module_type() {
        let module = ModuleConfig::new("clock", Map::new());
        let err = parse_config(&module).expect_err("wrong type should fail");
        assert!(err.contains("expected module type 'exec'"));
    }

    #[test]
    fn parse_config_requires_command() {
        let module = ModuleConfig::new(MODULE_TYPE, Map::new());
        let err = parse_config(&module).expect_err("missing command should fail");
        assert!(err.contains("invalid exec module config"));
    }

    #[test]
    fn parse_config_supports_click_aliases() {
        let click_module = ModuleConfig::new(
            MODULE_TYPE,
            serde_json::from_value(json!({
                "command": "echo ok",
                "click": "foo"
            }))
            .expect("module config map should parse"),
        );
        let click_cfg = parse_config(&click_module).expect("click config should parse");
        assert_eq!(click_cfg.click.as_deref(), Some("foo"));

        let on_click_module = ModuleConfig::new(
            MODULE_TYPE,
            serde_json::from_value(json!({
                "command": "echo ok",
                "on-click": "bar"
            }))
            .expect("module config map should parse"),
        );
        let on_click_cfg = parse_config(&on_click_module).expect("on-click config should parse");
        assert_eq!(on_click_cfg.on_click.as_deref(), Some("bar"));
    }

    #[test]
    fn run_exec_command_prefers_stdout() {
        let output = run_exec_command("printf 'out'; printf 'err' >&2");
        assert_eq!(output.text, "out");
        assert!(output.classes.is_empty());
    }

    #[test]
    fn run_exec_command_falls_back_to_stderr() {
        let output = run_exec_command("printf 'err-only' >&2");
        assert_eq!(output.text, "err-only");
        assert!(output.classes.is_empty());
    }

    #[test]
    fn parse_exec_output_supports_i3blocks_style_class_line() {
        let output = parse_exec_output("42%\n\nmedium");
        assert_eq!(output.text, "42%");
        assert_eq!(output.classes, vec!["medium"]);
    }

    #[test]
    fn parse_exec_output_supports_json_class_string() {
        let output = parse_exec_output(r#"{"text":"42%","class":"medium warning"}"#);
        assert_eq!(output.text, "42%");
        assert_eq!(output.classes, vec!["medium", "warning"]);
    }

    #[test]
    fn parse_exec_output_supports_json_class_array() {
        let output = parse_exec_output(r#"{"text":"42%","class":["medium","battery"]}"#);
        assert_eq!(output.text, "42%");
        assert_eq!(output.classes, vec!["medium", "battery"]);
    }

    #[test]
    fn shared_exec_backend_broadcasts_to_all_subscribers() {
        let backend = SharedExecBackend::default();
        let (sender_a, recv_a) = mpsc::channel();
        let (sender_b, recv_b) = mpsc::channel();

        backend.add_subscriber(sender_a);
        backend.add_subscriber(sender_b);
        backend.broadcast(ExecRenderedOutput {
            text: "42".to_string(),
            classes: vec!["ok".to_string()],
        });

        assert_eq!(
            recv_a
                .recv_timeout(Duration::from_millis(100))
                .expect("subscriber A should receive update"),
            ExecRenderedOutput {
                text: "42".to_string(),
                classes: vec!["ok".to_string()]
            }
        );
        assert_eq!(
            recv_b
                .recv_timeout(Duration::from_millis(100))
                .expect("subscriber B should receive update"),
            ExecRenderedOutput {
                text: "42".to_string(),
                classes: vec!["ok".to_string()]
            }
        );
    }

    #[test]
    fn shared_exec_backend_replays_latest_to_new_subscriber() {
        let backend = SharedExecBackend::default();
        backend.broadcast(ExecRenderedOutput {
            text: "latest".to_string(),
            classes: vec!["cached".to_string()],
        });

        let (sender, receiver) = mpsc::channel();
        backend.add_subscriber(sender);

        assert_eq!(
            receiver
                .recv_timeout(Duration::from_millis(100))
                .expect("subscriber should receive latest value immediately"),
            ExecRenderedOutput {
                text: "latest".to_string(),
                classes: vec!["cached".to_string()]
            }
        );
    }

    #[test]
    fn shared_exec_backend_drops_disconnected_subscribers() {
        let backend = SharedExecBackend::default();
        let (dead_sender, dead_receiver) = mpsc::channel::<ExecRenderedOutput>();
        drop(dead_receiver);

        let (alive_sender, _alive_receiver) = mpsc::channel::<ExecRenderedOutput>();
        backend.add_subscriber(dead_sender);
        backend.add_subscriber(alive_sender);
        backend.broadcast(ExecRenderedOutput {
            text: "x".to_string(),
            classes: Vec::new(),
        });

        let subscriber_count = backend
            .subscribers
            .lock()
            .expect("subscribers mutex should lock")
            .len();
        assert_eq!(subscriber_count, 1);
    }
}
