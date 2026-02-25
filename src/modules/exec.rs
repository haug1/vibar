use std::collections::{HashMap, HashSet};
use std::process::Command;
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use glib::ControlFlow;
use gtk::prelude::*;
use gtk::{Align, Label, Widget};
use serde::Deserialize;
use serde_json::Value;

use crate::modules::{
    apply_css_classes, attach_primary_click_command, escape_markup_text, render_markup_template,
    ModuleBuildContext, ModuleConfig,
};

use super::ModuleFactory;

const MIN_EXEC_INTERVAL_SECS: u32 = 1;
pub(crate) const MODULE_TYPE: &str = "exec";

#[derive(Debug, Deserialize, Clone)]
pub(crate) struct ExecConfig {
    pub(crate) command: String,
    #[serde(default = "default_exec_format")]
    pub(crate) format: String,
    #[serde(default)]
    pub(crate) click: Option<String>,
    #[serde(rename = "on-click", default)]
    pub(crate) on_click: Option<String>,
    #[serde(default = "default_exec_interval")]
    pub(crate) interval_secs: u32,
    #[serde(default)]
    pub(crate) signal: Option<i32>,
    #[serde(default)]
    pub(crate) class: Option<String>,
}

fn default_exec_interval() -> u32 {
    5
}

fn default_exec_format() -> String {
    "{text}".to_string()
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
        let signal = normalize_exec_signal(parsed.signal)?;
        Ok(build_exec_module(
            parsed.command,
            parsed.format,
            click_command,
            parsed.interval_secs,
            signal,
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
    format: String,
    click_command: Option<String>,
    interval_secs: u32,
    signal: Option<i32>,
    class: Option<String>,
) -> Label {
    let label = Label::new(None);
    label.set_halign(Align::Start);
    label.set_xalign(0.0);
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

    let receiver = subscribe_shared_exec_output(command, format, effective_interval_secs, signal);

    glib::timeout_add_local(std::time::Duration::from_millis(200), {
        let label = label.clone();
        let mut active_dynamic_classes: Vec<String> = Vec::new();
        move || {
            while let Ok(rendered) = receiver.try_recv() {
                label.set_markup(&rendered.text);
                label.set_visible(rendered.visible);
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

pub(crate) fn normalize_exec_signal(signal: Option<i32>) -> Result<Option<i32>, String> {
    signal.map(exec_signal_to_signum).transpose()
}

fn exec_signal_to_signum(signal: i32) -> Result<i32, String> {
    if signal < 1 {
        return Err("invalid exec module config: `signal` must be >= 1".to_string());
    }

    let rt_min = libc::SIGRTMIN();
    let rt_max = libc::SIGRTMAX();
    let max_signal = rt_max - rt_min;

    if signal > max_signal {
        return Err(format!(
            "invalid exec module config: `signal` must be <= {max_signal}"
        ));
    }

    Ok(rt_min + signal)
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct ExecSharedKey {
    command: String,
    format: String,
    interval_secs: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct ExecRenderedOutput {
    text: String,
    classes: Vec<String>,
    visible: bool,
}

#[derive(Default)]
struct SharedExecBackend {
    latest: Mutex<Option<ExecRenderedOutput>>,
    subscribers: Mutex<Vec<std::sync::mpsc::Sender<ExecRenderedOutput>>>,
    refresh_sender: Mutex<Option<std::sync::mpsc::Sender<()>>>,
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

    fn set_refresh_sender(&self, sender: std::sync::mpsc::Sender<()>) {
        *self
            .refresh_sender
            .lock()
            .expect("exec backend refresh sender mutex poisoned") = Some(sender);
    }

    fn request_refresh(&self) {
        let sender = self
            .refresh_sender
            .lock()
            .expect("exec backend refresh sender mutex poisoned")
            .clone();

        if let Some(sender) = sender {
            let _ = sender.send(());
        }
    }
}

type SharedExecMap = HashMap<ExecSharedKey, Arc<SharedExecBackend>>;

fn shared_exec_backends() -> &'static Mutex<SharedExecMap> {
    static SHARED_EXEC_BACKENDS: OnceLock<Mutex<SharedExecMap>> = OnceLock::new();
    SHARED_EXEC_BACKENDS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn subscribe_shared_exec_output(
    command: String,
    format: String,
    interval_secs: u32,
    signal: Option<i32>,
) -> std::sync::mpsc::Receiver<ExecRenderedOutput> {
    let key = ExecSharedKey {
        command,
        format,
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

    if let Some(signum) = signal {
        register_exec_signal(signum, &backend);
    }

    let (sender, receiver) = std::sync::mpsc::channel::<ExecRenderedOutput>();
    backend.add_subscriber(sender);
    receiver
}

fn start_shared_exec_worker(key: ExecSharedKey, backend: Arc<SharedExecBackend>) {
    let (refresh_sender, refresh_receiver) = std::sync::mpsc::channel::<()>();
    backend.set_refresh_sender(refresh_sender);

    std::thread::spawn(move || loop {
        backend.broadcast(run_exec_command(&key.command, &key.format));
        match refresh_receiver.recv_timeout(Duration::from_secs(u64::from(key.interval_secs))) {
            Ok(()) | Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return,
        }
    });
}

#[derive(Default)]
struct ExecSignalRegistry {
    registered_signals: HashSet<i32>,
    signal_backends: HashMap<i32, Vec<Arc<SharedExecBackend>>>,
}

fn exec_signal_registry() -> &'static Mutex<ExecSignalRegistry> {
    static EXEC_SIGNAL_REGISTRY: OnceLock<Mutex<ExecSignalRegistry>> = OnceLock::new();
    EXEC_SIGNAL_REGISTRY.get_or_init(|| Mutex::new(ExecSignalRegistry::default()))
}

fn register_exec_signal(signum: i32, backend: &Arc<SharedExecBackend>) {
    ensure_exec_signal_dispatch_ready();

    let should_install = {
        let mut registry = exec_signal_registry()
            .lock()
            .expect("exec signal registry mutex poisoned");
        let listeners = registry.signal_backends.entry(signum).or_default();
        if !listeners
            .iter()
            .any(|existing| Arc::ptr_eq(existing, backend))
        {
            listeners.push(Arc::clone(backend));
        }
        registry.registered_signals.insert(signum)
    };

    if should_install {
        install_exec_signal_handler(signum);
    }
}

fn notify_exec_signal(signum: i32) {
    let backends = exec_signal_registry()
        .lock()
        .expect("exec signal registry mutex poisoned")
        .signal_backends
        .get(&signum)
        .cloned()
        .unwrap_or_default();

    for backend in backends {
        backend.request_refresh();
    }
}

static EXEC_SIGNAL_PIPE_WRITE_FD: AtomicI32 = AtomicI32::new(-1);

fn ensure_exec_signal_dispatch_ready() {
    static INIT: OnceLock<()> = OnceLock::new();
    INIT.get_or_init(|| {
        let mut fds = [0; 2];
        let pipe_result = unsafe { libc::pipe(fds.as_mut_ptr()) };
        if pipe_result != 0 {
            eprintln!("vibar/exec: failed to initialize signal pipe");
            return;
        }

        for &fd in &fds {
            let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
            if flags >= 0 {
                let _ = unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) };
            }

            let fd_flags = unsafe { libc::fcntl(fd, libc::F_GETFD) };
            if fd_flags >= 0 {
                let _ = unsafe { libc::fcntl(fd, libc::F_SETFD, fd_flags | libc::FD_CLOEXEC) };
            }
        }

        let read_fd = fds[0];
        let write_fd = fds[1];
        EXEC_SIGNAL_PIPE_WRITE_FD.store(write_fd, Ordering::Relaxed);

        glib::source::unix_fd_add_local(read_fd, glib::IOCondition::IN, move |_, _| {
            drain_exec_signal_pipe(read_fd);
            ControlFlow::Continue
        });
    });
}

fn install_exec_signal_handler(signum: i32) {
    let mut action: libc::sigaction = unsafe { std::mem::zeroed() };
    action.sa_flags = 0;
    action.sa_sigaction = exec_signal_handler as *const () as usize;
    unsafe {
        libc::sigemptyset(&mut action.sa_mask);
    }

    let rc = unsafe { libc::sigaction(signum, &action, std::ptr::null_mut()) };
    if rc != 0 {
        eprintln!("vibar/exec: failed to install signal handler for signal {signum}");
    }
}

extern "C" fn exec_signal_handler(signum: libc::c_int) {
    let write_fd = EXEC_SIGNAL_PIPE_WRITE_FD.load(Ordering::Relaxed);
    if write_fd < 0 {
        return;
    }

    let bytes = signum.to_ne_bytes();
    let _ = unsafe { libc::write(write_fd, bytes.as_ptr().cast(), bytes.len()) };
}

fn drain_exec_signal_pipe(read_fd: i32) {
    let mut bytes = [0_u8; std::mem::size_of::<libc::c_int>()];
    loop {
        let rc = unsafe { libc::read(read_fd, bytes.as_mut_ptr().cast(), bytes.len()) };
        if rc == bytes.len() as isize {
            let signum = i32::from_ne_bytes(bytes);
            notify_exec_signal(signum);
            continue;
        }

        if rc <= 0 {
            break;
        }
    }
}

fn run_exec_command(command: &str, format: &str) -> ExecRenderedOutput {
    match Command::new("sh").arg("-c").arg(command).output() {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout).to_string();
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();

            if !stdout.trim().is_empty() {
                parse_exec_output(&stdout, format)
            } else if !stderr.trim().is_empty() {
                apply_exec_format(
                    stderr.trim().to_string(),
                    Vec::new(),
                    HashMap::new(),
                    format,
                )
            } else {
                ExecRenderedOutput::default()
            }
        }
        Err(err) => ExecRenderedOutput {
            text: escape_markup_text(&format!("exec error: {err}")),
            classes: Vec::new(),
            visible: true,
        },
    }
}

fn parse_exec_output(raw: &str, format: &str) -> ExecRenderedOutput {
    let trimmed = raw.trim_end_matches(&['\r', '\n'][..]);
    if trimmed.is_empty() {
        return ExecRenderedOutput::default();
    }

    if let Ok(value) = serde_json::from_str::<Value>(trimmed) {
        return parse_json_exec_output(value, format);
    }

    parse_i3blocks_exec_output(trimmed, format)
}

fn parse_json_exec_output(value: Value, format: &str) -> ExecRenderedOutput {
    let text = value
        .get("text")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let classes = value
        .get("class")
        .map(parse_json_classes)
        .unwrap_or_default();
    let vars = parse_json_format_vars(&value);

    apply_exec_format(text, classes, vars, format)
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

fn parse_i3blocks_exec_output(raw: &str, format: &str) -> ExecRenderedOutput {
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

    apply_exec_format(text, classes, HashMap::new(), format)
}

fn split_classes(raw: &str) -> Vec<String> {
    raw.split_whitespace()
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>()
}

fn parse_json_format_vars(value: &Value) -> HashMap<String, String> {
    let Some(map) = value.as_object() else {
        return HashMap::new();
    };

    map.iter()
        .filter_map(|(key, value)| {
            value_to_placeholder_string(value).map(|value| (format!("{{{key}}}"), value))
        })
        .collect()
}

fn value_to_placeholder_string(value: &Value) -> Option<String> {
    match value {
        Value::String(v) => Some(v.clone()),
        Value::Number(v) => Some(v.to_string()),
        Value::Bool(v) => Some(v.to_string()),
        _ => None,
    }
}

fn apply_exec_format(
    text: String,
    classes: Vec<String>,
    json_vars: HashMap<String, String>,
    template: &str,
) -> ExecRenderedOutput {
    let visible = !text.trim().is_empty();
    let mut replacements: Vec<(String, String)> = vec![
        ("{}".to_string(), text.clone()),
        ("{text}".to_string(), text),
    ];
    replacements.extend(json_vars);

    let replacement_refs = replacements
        .iter()
        .map(|(placeholder, value)| (placeholder.as_str(), value.as_str()))
        .collect::<Vec<_>>();
    let rendered = render_markup_template(template, &replacement_refs);

    ExecRenderedOutput {
        text: rendered,
        classes,
        visible,
    }
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
    fn parse_config_supports_signal_field() {
        let module = ModuleConfig::new(
            MODULE_TYPE,
            serde_json::from_value(json!({
                "command": "echo ok",
                "signal": 8
            }))
            .expect("module config map should parse"),
        );
        let cfg = parse_config(&module).expect("signal config should parse");
        assert_eq!(cfg.signal, Some(8));
    }

    #[test]
    fn normalize_exec_signal_accepts_none() {
        assert_eq!(
            normalize_exec_signal(None).expect("none should be valid"),
            None
        );
    }

    #[test]
    fn normalize_exec_signal_rejects_zero() {
        let err = normalize_exec_signal(Some(0)).expect_err("signal=0 should be invalid");
        assert!(err.contains("`signal` must be >= 1"));
    }

    #[test]
    fn normalize_exec_signal_maps_to_realtime_signal_number() {
        let signum = normalize_exec_signal(Some(8))
            .expect("signal=8 should be valid")
            .expect("signal number should be present");
        assert_eq!(signum, libc::SIGRTMIN() + 8);
    }

    #[test]
    fn normalize_exec_signal_rejects_values_above_rtmax() {
        let max_signal = libc::SIGRTMAX() - libc::SIGRTMIN();
        let err = normalize_exec_signal(Some(max_signal + 1))
            .expect_err("signal above rtmax should be invalid");
        assert!(err.contains("`signal` must be <="));
    }

    #[test]
    fn run_exec_command_prefers_stdout() {
        let output = run_exec_command("printf 'out'; printf 'err' >&2", "{text}");
        assert_eq!(output.text, "out");
        assert!(output.classes.is_empty());
        assert!(output.visible);
    }

    #[test]
    fn run_exec_command_falls_back_to_stderr() {
        let output = run_exec_command("printf 'err-only' >&2", "{text}");
        assert_eq!(output.text, "err-only");
        assert!(output.classes.is_empty());
        assert!(output.visible);
    }

    #[test]
    fn run_exec_command_hides_when_output_is_empty() {
        let output = run_exec_command("printf ''", "{text}");
        assert_eq!(output.text, "");
        assert!(output.classes.is_empty());
        assert!(!output.visible);
    }

    #[test]
    fn parse_exec_output_supports_i3blocks_style_class_line() {
        let output = parse_exec_output("42%\n\nmedium", "{text}");
        assert_eq!(output.text, "42%");
        assert_eq!(output.classes, vec!["medium"]);
        assert!(output.visible);
    }

    #[test]
    fn parse_exec_output_supports_json_class_string() {
        let output = parse_exec_output(r#"{"text":"42%","class":"medium warning"}"#, "{text}");
        assert_eq!(output.text, "42%");
        assert_eq!(output.classes, vec!["medium", "warning"]);
        assert!(output.visible);
    }

    #[test]
    fn parse_exec_output_supports_json_class_array() {
        let output = parse_exec_output(r#"{"text":"42%","class":["medium","battery"]}"#, "{text}");
        assert_eq!(output.text, "42%");
        assert_eq!(output.classes, vec!["medium", "battery"]);
        assert!(output.visible);
    }

    #[test]
    fn parse_exec_output_applies_template_to_plain_text() {
        let output = parse_exec_output("42%", "<span style=\"italic\">{}</span>");
        assert_eq!(output.text, "<span style=\"italic\">42%</span>");
        assert!(output.visible);
    }

    #[test]
    fn parse_exec_output_maps_json_fields_into_template() {
        let output = parse_exec_output(
            r#"{"text":"42%","host":"n1","temp":66,"ok":true}"#,
            "{host} {text} {temp} {ok}",
        );
        assert_eq!(output.text, "n1 42% 66 true");
        assert!(output.visible);
    }

    #[test]
    fn parse_exec_output_escapes_template_values() {
        let output = parse_exec_output(
            r#"{"text":"<b>x</b>","name":"a&b"}"#,
            "<span>{name} {text}</span>",
        );
        assert_eq!(output.text, "<span>a&amp;b &lt;b&gt;x&lt;/b&gt;</span>");
        assert!(output.visible);
    }

    #[test]
    fn parse_exec_output_hides_when_text_is_empty() {
        let output = parse_exec_output(r#"{"text":"","class":"idle"}"#, "{text}");
        assert_eq!(output.text, "");
        assert_eq!(output.classes, vec!["idle"]);
        assert!(!output.visible);
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
            visible: true,
        });

        assert_eq!(
            recv_a
                .recv_timeout(Duration::from_millis(100))
                .expect("subscriber A should receive update"),
            ExecRenderedOutput {
                text: "42".to_string(),
                classes: vec!["ok".to_string()],
                visible: true,
            }
        );
        assert_eq!(
            recv_b
                .recv_timeout(Duration::from_millis(100))
                .expect("subscriber B should receive update"),
            ExecRenderedOutput {
                text: "42".to_string(),
                classes: vec!["ok".to_string()],
                visible: true,
            }
        );
    }

    #[test]
    fn shared_exec_backend_replays_latest_to_new_subscriber() {
        let backend = SharedExecBackend::default();
        backend.broadcast(ExecRenderedOutput {
            text: "latest".to_string(),
            classes: vec!["cached".to_string()],
            visible: true,
        });

        let (sender, receiver) = mpsc::channel();
        backend.add_subscriber(sender);

        assert_eq!(
            receiver
                .recv_timeout(Duration::from_millis(100))
                .expect("subscriber should receive latest value immediately"),
            ExecRenderedOutput {
                text: "latest".to_string(),
                classes: vec!["cached".to_string()],
                visible: true,
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
            visible: true,
        });

        let subscriber_count = backend
            .subscribers
            .lock()
            .expect("subscribers mutex should lock")
            .len();
        assert_eq!(subscriber_count, 1);
    }
}
