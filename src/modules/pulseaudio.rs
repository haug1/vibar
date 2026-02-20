use std::io::{BufRead, BufReader};
use std::process::Command;
use std::process::Stdio;
use std::time::Duration;

use glib::{ControlFlow, Propagation};
use gtk::prelude::*;
use gtk::{EventControllerScroll, EventControllerScrollFlags, GestureClick, Label, Widget};
use serde::Deserialize;
use serde_json::Value;

use crate::modules::{ModuleBuildContext, ModuleConfig};

use super::ModuleFactory;

const UI_DRAIN_INTERVAL_MILLIS: u64 = 200;
const SUBSCRIBE_RECONNECT_DELAY_SECS: u64 = 2;
const DEFAULT_SCROLL_STEP: f64 = 1.0;
const DEFAULT_FORMAT: &str = "{volume}% {icon}  {format_source}";
const DEFAULT_FORMAT_BLUETOOTH: &str = "{volume}% {icon} {format_source}";
const DEFAULT_FORMAT_BLUETOOTH_MUTED: &str = " {icon} {format_source}";
const DEFAULT_FORMAT_MUTED: &str = " {format_source}";
const DEFAULT_FORMAT_SOURCE: &str = "";
const DEFAULT_FORMAT_SOURCE_MUTED: &str = "";
const ICON_VOLUME_LOW: &str = "";
const ICON_VOLUME_MEDIUM: &str = "";
const ICON_VOLUME_HIGH: &str = "";
const ICON_HEADPHONE: &str = "";
const ICON_HANDS_FREE: &str = "";
const ICON_HEADSET: &str = "";
const ICON_PHONE: &str = "";
const ICON_PORTABLE: &str = "";
const ICON_CAR: &str = "";
pub(crate) const MODULE_TYPE: &str = "pulseaudio";

#[derive(Debug, Deserialize, Clone)]
pub(crate) struct PulseAudioConfig {
    #[serde(rename = "scroll-step", default = "default_scroll_step")]
    pub(crate) scroll_step: f64,
    #[serde(default)]
    pub(crate) format: Option<String>,
    #[serde(rename = "format-bluetooth", default)]
    pub(crate) format_bluetooth: Option<String>,
    #[serde(rename = "format-bluetooth-muted", default)]
    pub(crate) format_bluetooth_muted: Option<String>,
    #[serde(rename = "format-muted", default)]
    pub(crate) format_muted: Option<String>,
    #[serde(rename = "format-source", default)]
    pub(crate) format_source: Option<String>,
    #[serde(rename = "format-source-muted", default)]
    pub(crate) format_source_muted: Option<String>,
    #[serde(rename = "format-icons", default = "default_format_icons")]
    pub(crate) format_icons: PulseAudioFormatIcons,
    #[serde(default)]
    pub(crate) click: Option<String>,
    #[serde(rename = "on-click", default)]
    pub(crate) on_click: Option<String>,
    #[serde(default)]
    pub(crate) class: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
pub(crate) struct PulseAudioFormatIcons {
    #[serde(default)]
    pub(crate) headphone: Option<String>,
    #[serde(rename = "hands-free", default)]
    pub(crate) hands_free: Option<String>,
    #[serde(default)]
    pub(crate) headset: Option<String>,
    #[serde(default)]
    pub(crate) phone: Option<String>,
    #[serde(default)]
    pub(crate) portable: Option<String>,
    #[serde(default)]
    pub(crate) car: Option<String>,
    #[serde(default = "default_volume_icons")]
    pub(crate) default: Vec<String>,
}

#[derive(Debug, Clone)]
struct PulseState {
    volume: u32,
    muted: bool,
    source_muted: bool,
    bluetooth: bool,
    icon_kind: IconKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IconKind {
    Headphone,
    HandsFree,
    Headset,
    Phone,
    Portable,
    Car,
    Default,
}

pub(crate) struct PulseAudioFactory;

pub(crate) const FACTORY: PulseAudioFactory = PulseAudioFactory;

impl ModuleFactory for PulseAudioFactory {
    fn module_type(&self) -> &'static str {
        MODULE_TYPE
    }

    fn init(&self, config: &ModuleConfig, _context: &ModuleBuildContext) -> Result<Widget, String> {
        let parsed = parse_config(config)?;
        let click_command = parsed.click.clone().or(parsed.on_click.clone());
        Ok(build_pulseaudio_module(parsed, click_command).upcast())
    }
}

fn default_scroll_step() -> f64 {
    DEFAULT_SCROLL_STEP
}

fn default_volume_icons() -> Vec<String> {
    vec![
        ICON_VOLUME_LOW.to_string(),
        ICON_VOLUME_MEDIUM.to_string(),
        ICON_VOLUME_HIGH.to_string(),
    ]
}

fn default_format_icons() -> PulseAudioFormatIcons {
    PulseAudioFormatIcons {
        headphone: None,
        hands_free: None,
        headset: None,
        phone: None,
        portable: None,
        car: None,
        default: default_volume_icons(),
    }
}

pub(crate) fn parse_config(module: &ModuleConfig) -> Result<PulseAudioConfig, String> {
    if module.module_type != MODULE_TYPE {
        return Err(format!(
            "expected module type '{}', got '{}'",
            MODULE_TYPE, module.module_type
        ));
    }

    serde_json::from_value(Value::Object(module.config.clone()))
        .map_err(|err| format!("invalid {} module config: {err}", MODULE_TYPE))
}

fn build_pulseaudio_module(config: PulseAudioConfig, click_command: Option<String>) -> Label {
    let label = Label::new(None);
    label.add_css_class("module");
    label.add_css_class("pulseaudio");

    if let Some(class_name) = config.class.clone() {
        label.add_css_class(&class_name);
    }

    if let Some(command) = click_command {
        let click = GestureClick::builder().button(1).build();
        click.connect_pressed(move |_, _, _, _| {
            run_click_command(&command);
        });
        label.add_controller(click);
    }

    let scroll_step = normalized_scroll_step(config.scroll_step);
    if (scroll_step - config.scroll_step).abs() > f64::EPSILON {
        eprintln!(
            "pulseaudio scroll-step={} is too low; clamping to {}",
            config.scroll_step, scroll_step
        );
    }
    if scroll_step > 0.0 {
        let scroll = EventControllerScroll::new(
            EventControllerScrollFlags::VERTICAL | EventControllerScrollFlags::DISCRETE,
        );
        scroll.connect_scroll(move |_, _, dy| {
            if dy < 0.0 {
                run_volume_step(scroll_step, true);
                return Propagation::Stop;
            }
            if dy > 0.0 {
                run_volume_step(scroll_step, false);
                return Propagation::Stop;
            }
            Propagation::Proceed
        });
        label.add_controller(scroll);
    }

    let (sender, receiver) = std::sync::mpsc::channel::<String>();
    let render_config = config.clone();
    std::thread::spawn(move || run_subscribe_loop(sender, render_config));

    glib::timeout_add_local(Duration::from_millis(UI_DRAIN_INTERVAL_MILLIS), {
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

pub(crate) fn normalized_scroll_step(step: f64) -> f64 {
    if step <= 0.0 || !step.is_finite() {
        0.0
    } else {
        step
    }
}

fn run_click_command(command: &str) {
    let command = command.to_string();
    std::thread::spawn(move || {
        let _ = Command::new("sh").arg("-c").arg(command).spawn();
    });
}

fn run_volume_step(step: f64, increase: bool) {
    let delta = format!(
        "{}{}%",
        if increase { "+" } else { "-" },
        format_step_value(step)
    );
    std::thread::spawn(move || {
        let _ = Command::new("pactl")
            .args(["set-sink-volume", "@DEFAULT_SINK@", &delta])
            .status();
    });
}

fn format_step_value(step: f64) -> String {
    let rendered = format!("{step:.3}");
    rendered
        .trim_end_matches('0')
        .trim_end_matches('.')
        .to_string()
}

fn run_subscribe_loop(sender: std::sync::mpsc::Sender<String>, config: PulseAudioConfig) {
    loop {
        send_rendered_state(&sender, &config);
        if let Err(err) = subscribe_for_updates(&sender, &config) {
            let _ = sender.send(format!("audio error: {err}"));
            std::thread::sleep(Duration::from_secs(SUBSCRIBE_RECONNECT_DELAY_SECS));
        }
    }
}

fn send_rendered_state(sender: &std::sync::mpsc::Sender<String>, config: &PulseAudioConfig) {
    let text = match read_pulseaudio_state() {
        Ok(state) => render_format(config, &state),
        Err(err) => format!("audio error: {err}"),
    };
    let _ = sender.send(text);
}

fn subscribe_for_updates(
    sender: &std::sync::mpsc::Sender<String>,
    config: &PulseAudioConfig,
) -> Result<(), String> {
    let mut child = Command::new("pactl")
        .arg("subscribe")
        .stdout(Stdio::piped())
        .spawn()
        .map_err(|err| format!("failed to run pactl subscribe: {err}"))?;

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "pactl subscribe did not provide stdout".to_string())?;
    let reader = BufReader::new(stdout);

    for line in reader.lines() {
        let line = line.map_err(|err| format!("failed reading pactl subscribe output: {err}"))?;
        if is_subscription_event_relevant(&line) {
            send_rendered_state(sender, config);
        }
    }

    let status = child
        .wait()
        .map_err(|err| format!("failed waiting on pactl subscribe: {err}"))?;
    if !status.success() {
        return Err(format!("pactl subscribe exited with status {status}"));
    }

    Err("pactl subscribe stream ended".to_string())
}

fn is_subscription_event_relevant(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    lower.contains(" on sink ")
        || lower.contains(" on source ")
        || lower.contains(" on server ")
        || lower.contains(" on card ")
}

fn read_pulseaudio_state() -> Result<PulseState, String> {
    let volume_output = run_pactl(&["get-sink-volume", "@DEFAULT_SINK@"])?;
    let mute_output = run_pactl(&["get-sink-mute", "@DEFAULT_SINK@"])?;
    let source_mute_output = run_pactl(&["get-source-mute", "@DEFAULT_SOURCE@"])?;
    let sink_name = read_default_sink_name()?;
    let sinks_output = run_pactl(&["list", "sinks"])?;
    let sink_meta = parse_sink_metadata(&sinks_output, &sink_name);

    Ok(PulseState {
        volume: parse_volume_percent(&volume_output)?,
        muted: parse_mute_state(&mute_output)?,
        source_muted: parse_mute_state(&source_mute_output)?,
        bluetooth: sink_meta.bluetooth,
        icon_kind: sink_meta.icon_kind,
    })
}

fn run_pactl(args: &[&str]) -> Result<String, String> {
    let output = Command::new("pactl")
        .args(args)
        .output()
        .map_err(|err| format!("failed to run pactl {}: {err}", args.join(" ")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(if stderr.is_empty() {
            format!(
                "pactl {} exited with status {}",
                args.join(" "),
                output.status
            )
        } else {
            format!("pactl {} failed: {stderr}", args.join(" "))
        });
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

fn read_default_sink_name() -> Result<String, String> {
    let info = run_pactl(&["info"])?;
    parse_default_sink_name(&info)
}

fn parse_default_sink_name(info_output: &str) -> Result<String, String> {
    let sink_name = info_output
        .lines()
        .find_map(|line| line.trim().strip_prefix("Default Sink:"))
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_string);
    sink_name.ok_or_else(|| "could not parse default sink name from pactl info".to_string())
}

fn parse_volume_percent(volume_output: &str) -> Result<u32, String> {
    for token in volume_output.split_whitespace() {
        if let Some(raw) = token.strip_suffix('%') {
            let cleaned = raw.trim_matches(|c: char| !c.is_ascii_digit());
            if cleaned.is_empty() {
                continue;
            }
            let value = cleaned
                .parse::<u32>()
                .map_err(|err| format!("failed parsing volume '{cleaned}': {err}"))?;
            return Ok(value);
        }
    }
    Err("could not parse sink volume percentage".to_string())
}

fn parse_mute_state(mute_output: &str) -> Result<bool, String> {
    let value = mute_output
        .lines()
        .find_map(|line| line.trim().strip_prefix("Mute:"))
        .map(str::trim)
        .ok_or_else(|| "could not parse mute state".to_string())?;

    match value {
        "yes" => Ok(true),
        "no" => Ok(false),
        other => Err(format!("unexpected mute state '{other}'")),
    }
}

#[derive(Debug, Clone, Copy)]
struct SinkMetadata {
    bluetooth: bool,
    icon_kind: IconKind,
}

fn parse_sink_metadata(sinks_output: &str, default_sink_name: &str) -> SinkMetadata {
    let mut current_name: Option<String> = None;
    let mut current_description: Option<String> = None;
    let mut current_active_port: Option<String> = None;
    let mut best: Option<SinkMetadata> = None;

    let mut finalize =
        |name: Option<String>, description: Option<String>, active_port: Option<String>| {
            let Some(name) = name else {
                return;
            };
            if name != default_sink_name {
                return;
            }
            let lower = format!(
                "{} {} {}",
                name.to_lowercase(),
                description.unwrap_or_default().to_lowercase(),
                active_port.unwrap_or_default().to_lowercase()
            );
            let bluetooth = lower.contains("bluez") || lower.contains("bluetooth");
            let icon_kind = classify_icon_kind(&lower);
            best = Some(SinkMetadata {
                bluetooth,
                icon_kind,
            });
        };

    for line in sinks_output.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("Sink #") {
            finalize(
                current_name.take(),
                current_description.take(),
                current_active_port.take(),
            );
            current_name = None;
            current_description = None;
            current_active_port = None;
            continue;
        }

        if let Some(name) = trimmed.strip_prefix("Name:") {
            current_name = Some(name.trim().to_string());
            continue;
        }
        if let Some(description) = trimmed.strip_prefix("Description:") {
            current_description = Some(description.trim().to_string());
            continue;
        }
        if let Some(active_port) = trimmed.strip_prefix("Active Port:") {
            current_active_port = Some(active_port.trim().to_string());
        }
    }

    finalize(current_name, current_description, current_active_port);

    best.unwrap_or_else(|| {
        let lower = default_sink_name.to_lowercase();
        SinkMetadata {
            bluetooth: lower.contains("bluez") || lower.contains("bluetooth"),
            icon_kind: classify_icon_kind(&lower),
        }
    })
}

fn classify_icon_kind(content: &str) -> IconKind {
    if content.contains("hands-free") || content.contains("handsfree") {
        return IconKind::HandsFree;
    }
    if content.contains("headset") {
        return IconKind::Headset;
    }
    if content.contains("headphone") {
        return IconKind::Headphone;
    }
    if content.contains("portable") {
        return IconKind::Portable;
    }
    if content.contains("phone") {
        return IconKind::Phone;
    }
    if content.contains("car") {
        return IconKind::Car;
    }
    IconKind::Default
}

fn render_format(config: &PulseAudioConfig, state: &PulseState) -> String {
    let format = if state.muted {
        if state.bluetooth {
            config
                .format_bluetooth_muted
                .as_deref()
                .unwrap_or(DEFAULT_FORMAT_BLUETOOTH_MUTED)
        } else {
            config
                .format_muted
                .as_deref()
                .unwrap_or(DEFAULT_FORMAT_MUTED)
        }
    } else if state.bluetooth {
        config
            .format_bluetooth
            .as_deref()
            .unwrap_or(DEFAULT_FORMAT_BLUETOOTH)
    } else {
        config.format.as_deref().unwrap_or(DEFAULT_FORMAT)
    };

    let source = if state.source_muted {
        config
            .format_source_muted
            .as_deref()
            .unwrap_or(DEFAULT_FORMAT_SOURCE_MUTED)
    } else {
        config
            .format_source
            .as_deref()
            .unwrap_or(DEFAULT_FORMAT_SOURCE)
    };

    let icon = config.format_icons.icon_for(state.icon_kind, state.volume);

    format
        .replace("{volume}", &state.volume.to_string())
        .replace("{icon}", &icon)
        .replace("{format_source}", source)
}

impl PulseAudioFormatIcons {
    fn icon_for(&self, kind: IconKind, volume: u32) -> String {
        match kind {
            IconKind::Headphone => self
                .headphone
                .as_deref()
                .unwrap_or(ICON_HEADPHONE)
                .to_string(),
            IconKind::HandsFree => self
                .hands_free
                .as_deref()
                .unwrap_or(ICON_HANDS_FREE)
                .to_string(),
            IconKind::Headset => self.headset.as_deref().unwrap_or(ICON_HEADSET).to_string(),
            IconKind::Phone => self.phone.as_deref().unwrap_or(ICON_PHONE).to_string(),
            IconKind::Portable => self
                .portable
                .as_deref()
                .unwrap_or(ICON_PORTABLE)
                .to_string(),
            IconKind::Car => self.car.as_deref().unwrap_or(ICON_CAR).to_string(),
            IconKind::Default => volume_icon_from_list(&self.default, volume),
        }
    }
}

fn volume_icon_from_list(icons: &[String], volume: u32) -> String {
    if icons.is_empty() {
        return if volume == 0 {
            ICON_VOLUME_LOW.to_string()
        } else if volume < 67 {
            ICON_VOLUME_MEDIUM.to_string()
        } else {
            ICON_VOLUME_HIGH.to_string()
        };
    }

    let clamped = volume.min(100) as usize;
    let len = icons.len();
    let idx = ((clamped * len) / 100).min(len - 1);
    icons[idx].clone()
}

#[cfg(test)]
mod tests {
    use serde_json::{json, Map};

    use super::*;

    #[test]
    fn parse_config_rejects_wrong_module_type() {
        let module = ModuleConfig::new("clock", Map::new());
        let err = parse_config(&module).expect_err("wrong type should fail");
        assert!(err.contains("expected module type 'pulseaudio'"));
    }

    #[test]
    fn parse_default_sink_name_extracts_name() {
        let info =
            "Server Name: PulseAudio\nDefault Sink: alsa_output.pci-0000_00_1f.3.analog-stereo\n";
        let name = parse_default_sink_name(info).expect("default sink should parse");
        assert_eq!(name, "alsa_output.pci-0000_00_1f.3.analog-stereo");
    }

    #[test]
    fn parse_volume_percent_extracts_first_percentage() {
        let output =
            "Volume: front-left: 49152 / 75% / -7.50 dB,   front-right: 49152 / 75% / -7.50 dB";
        assert_eq!(
            parse_volume_percent(output).expect("volume should parse"),
            75
        );
    }

    #[test]
    fn parse_mute_state_parses_yes_no() {
        assert!(parse_mute_state("Mute: yes").expect("yes should parse"));
        assert!(!parse_mute_state("Mute: no").expect("no should parse"));
    }

    #[test]
    fn parse_sink_metadata_matches_default_sink() {
        let sinks = r#"
Sink #1
    Name: alsa_output.pci-0000_00_1f.3.analog-stereo
    Description: Built-in Audio Analog Stereo
    Active Port: analog-output-speaker
Sink #2
    Name: bluez_output.00_11_22_33_44_55.1
    Description: Headset
    Active Port: headset-output
"#;
        let meta = parse_sink_metadata(sinks, "bluez_output.00_11_22_33_44_55.1");
        assert!(meta.bluetooth);
        assert_eq!(meta.icon_kind, IconKind::Headset);
    }

    #[test]
    fn render_format_applies_muted_and_source_placeholders() {
        let module = ModuleConfig::new(
            MODULE_TYPE,
            Map::from_iter([(
                "format-icons".to_string(),
                json!({ "default": ["a", "b", "c"] }),
            )]),
        );
        let config = parse_config(&module).expect("config should parse");
        let text = render_format(
            &config,
            &PulseState {
                volume: 80,
                muted: true,
                source_muted: false,
                bluetooth: false,
                icon_kind: IconKind::Default,
            },
        );
        assert_eq!(text, " ");
    }

    #[test]
    fn normalized_scroll_step_disables_zero_and_negative() {
        assert_eq!(normalized_scroll_step(0.0), 0.0);
        assert_eq!(normalized_scroll_step(-1.0), 0.0);
        assert_eq!(normalized_scroll_step(1.5), 1.5);
    }

    #[test]
    fn volume_icon_from_list_maps_range() {
        let icons = vec!["low".to_string(), "med".to_string(), "high".to_string()];
        assert_eq!(volume_icon_from_list(&icons, 0), "low");
        assert_eq!(volume_icon_from_list(&icons, 50), "med");
        assert_eq!(volume_icon_from_list(&icons, 100), "high");
    }

    #[test]
    fn subscription_event_filter_matches_audio_updates() {
        assert!(is_subscription_event_relevant("Event 'change' on sink #1"));
        assert!(is_subscription_event_relevant("Event 'new' on source #2"));
        assert!(is_subscription_event_relevant(
            "Event 'remove' on server #0"
        ));
        assert!(!is_subscription_event_relevant("Event 'new' on client #12"));
    }
}
