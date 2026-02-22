use std::process::Command;
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

const MIN_PLAYERCTL_INTERVAL_SECS: u32 = 1;
const DEFAULT_PLAYERCTL_INTERVAL_SECS: u32 = 1;
const DEFAULT_PLAYERCTL_FORMAT: &str = "{status_icon} {title}";
const DEFAULT_NO_PLAYER_TEXT: &str = "No media";
const METADATA_FORMAT: &str = "{{status}}\t{{playerName}}\t{{artist}}\t{{album}}\t{{title}}";
pub(crate) const MODULE_TYPE: &str = "playerctl";

#[derive(Debug, Deserialize, Clone)]
pub(crate) struct PlayerctlConfig {
    #[serde(default)]
    pub(crate) format: Option<String>,
    #[serde(default)]
    pub(crate) click: Option<String>,
    #[serde(rename = "on-click", default)]
    pub(crate) on_click: Option<String>,
    #[serde(default = "default_playerctl_interval")]
    pub(crate) interval_secs: u32,
    #[serde(default)]
    pub(crate) player: Option<String>,
    #[serde(default)]
    pub(crate) class: Option<String>,
    #[serde(default = "default_no_player_text")]
    pub(crate) no_player_text: String,
}

#[derive(Debug, Clone)]
struct PlayerctlMetadata {
    status: String,
    status_icon: &'static str,
    player: String,
    artist: String,
    album: String,
    title: String,
}

pub(crate) struct PlayerctlFactory;

pub(crate) const FACTORY: PlayerctlFactory = PlayerctlFactory;

impl ModuleFactory for PlayerctlFactory {
    fn module_type(&self) -> &'static str {
        MODULE_TYPE
    }

    fn init(&self, config: &ModuleConfig, _context: &ModuleBuildContext) -> Result<Widget, String> {
        let parsed = parse_config(config)?;
        let format = parsed
            .format
            .unwrap_or_else(|| DEFAULT_PLAYERCTL_FORMAT.to_string());
        let click_command = parsed.click.or(parsed.on_click);

        Ok(build_playerctl_module(
            format,
            click_command,
            parsed.interval_secs,
            parsed.player,
            parsed.class,
            parsed.no_player_text,
        )
        .upcast())
    }
}

fn default_playerctl_interval() -> u32 {
    DEFAULT_PLAYERCTL_INTERVAL_SECS
}

fn default_no_player_text() -> String {
    DEFAULT_NO_PLAYER_TEXT.to_string()
}

pub(crate) fn parse_config(module: &ModuleConfig) -> Result<PlayerctlConfig, String> {
    if module.module_type != MODULE_TYPE {
        return Err(format!(
            "expected module type '{}', got '{}'",
            MODULE_TYPE, module.module_type
        ));
    }

    serde_json::from_value(Value::Object(module.config.clone()))
        .map_err(|err| format!("invalid {} module config: {err}", MODULE_TYPE))
}

pub(crate) fn normalized_playerctl_interval(interval_secs: u32) -> u32 {
    interval_secs.max(MIN_PLAYERCTL_INTERVAL_SECS)
}

pub(crate) fn build_playerctl_module(
    format: String,
    click_command: Option<String>,
    interval_secs: u32,
    player: Option<String>,
    class: Option<String>,
    no_player_text: String,
) -> Label {
    let label = Label::new(None);
    label.add_css_class("module");
    label.add_css_class("playerctl");

    apply_css_classes(&label, class.as_deref());
    attach_primary_click_command(&label, click_command);

    let effective_interval_secs = normalized_playerctl_interval(interval_secs);
    if effective_interval_secs != interval_secs {
        eprintln!(
            "playerctl interval_secs={} is too low; clamping to {} second",
            interval_secs, effective_interval_secs
        );
    }

    let (sender, receiver) = std::sync::mpsc::channel::<String>();
    std::thread::spawn(move || loop {
        let text = match read_playerctl_metadata(player.as_deref()) {
            Ok(Some(metadata)) => render_format(&format, &metadata),
            Ok(None) => no_player_text.clone(),
            Err(err) => format!("playerctl error: {err}"),
        };
        let _ = sender.send(text);
        std::thread::sleep(Duration::from_secs(u64::from(effective_interval_secs)));
    });

    glib::timeout_add_local(Duration::from_millis(200), {
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

fn read_playerctl_metadata(player: Option<&str>) -> Result<Option<PlayerctlMetadata>, String> {
    let mut command = Command::new("playerctl");
    if let Some(player_name) = player {
        command.arg("--player").arg(player_name);
    }
    let output = command
        .arg("metadata")
        .arg("--format")
        .arg(METADATA_FORMAT)
        .output()
        .map_err(|err| format!("failed to run playerctl: {err}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if is_no_players_error(&stderr) {
            return Ok(None);
        }

        let stderr = stderr.trim();
        return Err(if stderr.is_empty() {
            format!("playerctl exited with status {}", output.status)
        } else {
            format!("playerctl failed: {stderr}")
        });
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    parse_playerctl_output(&stdout).map(Some)
}

fn parse_playerctl_output(output: &str) -> Result<PlayerctlMetadata, String> {
    let line = output
        .lines()
        .next()
        .ok_or_else(|| "missing playerctl metadata output".to_string())?;
    let mut fields = line.splitn(5, '\t');

    let status_raw = fields.next().unwrap_or_default();
    let player = fields.next().unwrap_or_default().to_string();
    let artist = fields.next().unwrap_or_default().to_string();
    let album = fields.next().unwrap_or_default().to_string();
    let title = fields.next().unwrap_or_default().to_string();

    if status_raw.is_empty() {
        return Err("missing player status from playerctl output".to_string());
    }

    let status = status_raw.to_ascii_lowercase();
    let status_icon = status_icon_for(&status);

    Ok(PlayerctlMetadata {
        status,
        status_icon,
        player,
        artist,
        album,
        title,
    })
}

fn status_icon_for(status: &str) -> &'static str {
    match status {
        "playing" => "",
        "paused" => "",
        "stopped" => "",
        _ => "",
    }
}

fn is_no_players_error(stderr: &str) -> bool {
    let normalized = stderr.to_ascii_lowercase();
    normalized.contains("no players found")
        || normalized.contains("no player could handle this command")
}

fn render_format(format: &str, metadata: &PlayerctlMetadata) -> String {
    format
        .replace("{status}", &metadata.status)
        .replace("{status_icon}", metadata.status_icon)
        .replace("{player}", &metadata.player)
        .replace("{artist}", &metadata.artist)
        .replace("{album}", &metadata.album)
        .replace("{title}", &metadata.title)
}

#[cfg(test)]
mod tests {
    use serde_json::Map;

    use super::*;

    #[test]
    fn parse_config_rejects_wrong_module_type() {
        let module = ModuleConfig::new("clock", Map::new());
        let err = parse_config(&module).expect_err("wrong type should fail");
        assert!(err.contains("expected module type 'playerctl'"));
    }

    #[test]
    fn normalized_playerctl_interval_enforces_lower_bound() {
        assert_eq!(normalized_playerctl_interval(0), 1);
        assert_eq!(normalized_playerctl_interval(1), 1);
        assert_eq!(normalized_playerctl_interval(10), 10);
    }

    #[test]
    fn parse_playerctl_output_parses_metadata() {
        let metadata = parse_playerctl_output("Playing\tspotify\tArtist\tAlbum\tSong")
            .expect("playerctl output should parse");

        assert_eq!(metadata.status, "playing");
        assert_eq!(metadata.status_icon, "");
        assert_eq!(metadata.player, "spotify");
        assert_eq!(metadata.artist, "Artist");
        assert_eq!(metadata.album, "Album");
        assert_eq!(metadata.title, "Song");
    }

    #[test]
    fn is_no_players_error_matches_known_messages() {
        assert!(is_no_players_error("No players found"));
        assert!(is_no_players_error("No player could handle this command"));
        assert!(!is_no_players_error("some other failure"));
    }

    #[test]
    fn render_format_replaces_placeholders() {
        let metadata = PlayerctlMetadata {
            status: "paused".to_string(),
            status_icon: "",
            player: "spotify".to_string(),
            artist: "Boards of Canada".to_string(),
            album: "Music Has the Right to Children".to_string(),
            title: "Roygbiv".to_string(),
        };

        let text = render_format(
            "{status_icon} {artist} - {title} ({player}) [{status}]",
            &metadata,
        );
        assert_eq!(text, " Boards of Canada - Roygbiv (spotify) [paused]");
    }
}
