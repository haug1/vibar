use std::collections::HashMap;
use std::process::Command;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use glib::ControlFlow;
use gtk::prelude::*;
use gtk::{Label, Widget};
use serde::Deserialize;
use serde_json::Value;

use crate::modules::{ModuleBuildContext, ModuleConfig};

use super::ModuleFactory;

const MIN_EXEC_INTERVAL_SECS: u32 = 1;
pub(crate) const MODULE_TYPE: &str = "exec";

#[derive(Debug, Deserialize, Clone)]
pub(crate) struct ExecConfig {
    pub(crate) command: String,
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
        Ok(build_exec_module(parsed.command, parsed.interval_secs, parsed.class).upcast())
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

    if let Some(class_name) = class {
        label.add_css_class(&class_name);
    }

    let receiver = subscribe_shared_exec_output(command, effective_interval_secs);

    glib::timeout_add_local(std::time::Duration::from_millis(200), {
        let label = label.clone();
        move || {
            while let Ok(text) = receiver.try_recv() {
                label.set_text(&text);
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

#[derive(Default)]
struct SharedExecBackend {
    latest: Mutex<Option<String>>,
    subscribers: Mutex<Vec<std::sync::mpsc::Sender<String>>>,
}

impl SharedExecBackend {
    fn add_subscriber(&self, sender: std::sync::mpsc::Sender<String>) {
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

    fn broadcast(&self, text: String) {
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
) -> std::sync::mpsc::Receiver<String> {
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

    let (sender, receiver) = std::sync::mpsc::channel::<String>();
    backend.add_subscriber(sender);
    receiver
}

fn start_shared_exec_worker(key: ExecSharedKey, backend: Arc<SharedExecBackend>) {
    std::thread::spawn(move || loop {
        backend.broadcast(run_exec_command(&key.command));
        std::thread::sleep(Duration::from_secs(u64::from(key.interval_secs)));
    });
}

fn run_exec_command(command: &str) -> String {
    match Command::new("sh").arg("-c").arg(command).output() {
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
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;
    use std::time::Duration;

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
    fn run_exec_command_prefers_stdout() {
        let output = run_exec_command("printf 'out'; printf 'err' >&2");
        assert_eq!(output, "out");
    }

    #[test]
    fn run_exec_command_falls_back_to_stderr() {
        let output = run_exec_command("printf 'err-only' >&2");
        assert_eq!(output, "err-only");
    }

    #[test]
    fn shared_exec_backend_broadcasts_to_all_subscribers() {
        let backend = SharedExecBackend::default();
        let (sender_a, recv_a) = mpsc::channel();
        let (sender_b, recv_b) = mpsc::channel();

        backend.add_subscriber(sender_a);
        backend.add_subscriber(sender_b);
        backend.broadcast("42".to_string());

        assert_eq!(
            recv_a
                .recv_timeout(Duration::from_millis(100))
                .expect("subscriber A should receive update"),
            "42"
        );
        assert_eq!(
            recv_b
                .recv_timeout(Duration::from_millis(100))
                .expect("subscriber B should receive update"),
            "42"
        );
    }

    #[test]
    fn shared_exec_backend_replays_latest_to_new_subscriber() {
        let backend = SharedExecBackend::default();
        backend.broadcast("latest".to_string());

        let (sender, receiver) = mpsc::channel();
        backend.add_subscriber(sender);

        assert_eq!(
            receiver
                .recv_timeout(Duration::from_millis(100))
                .expect("subscriber should receive latest value immediately"),
            "latest"
        );
    }

    #[test]
    fn shared_exec_backend_drops_disconnected_subscribers() {
        let backend = SharedExecBackend::default();
        let (dead_sender, dead_receiver) = mpsc::channel::<String>();
        drop(dead_receiver);

        let (alive_sender, _alive_receiver) = mpsc::channel::<String>();
        backend.add_subscriber(dead_sender);
        backend.add_subscriber(alive_sender);
        backend.broadcast("x".to_string());

        let subscriber_count = backend
            .subscribers
            .lock()
            .expect("subscribers mutex should lock")
            .len();
        assert_eq!(subscriber_count, 1);
    }
}
