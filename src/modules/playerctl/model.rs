use std::collections::HashMap;

use zbus::zvariant::{ObjectPath, OwnedValue};

#[derive(Debug, Clone)]
pub(super) enum BackendUpdate {
    Snapshot(Option<PlayerctlMetadata>),
    Error(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct PlayerctlMetadata {
    pub(super) status: String,
    pub(super) status_icon: &'static str,
    pub(super) player: String,
    pub(super) artist: String,
    pub(super) album: String,
    pub(super) title: String,
    pub(super) position_micros: Option<i64>,
    pub(super) length_micros: Option<i64>,
    pub(super) can_go_previous: bool,
    pub(super) can_go_next: bool,
    pub(super) can_play: bool,
    pub(super) can_pause: bool,
    pub(super) can_seek: bool,
    pub(super) track_id: Option<String>,
    pub(super) bus_name: String,
}

pub(super) fn select_active_player(
    candidates: Vec<PlayerctlMetadata>,
) -> Option<PlayerctlMetadata> {
    candidates.into_iter().min_by(|a, b| {
        active_rank(&a.status)
            .cmp(&active_rank(&b.status))
            .then(a.bus_name.cmp(&b.bus_name))
    })
}

pub(super) fn matches_player_filter(bus_name: &str, filter: &str) -> bool {
    bus_name == filter
        || bus_name
            .strip_prefix(super::backend::MPRIS_PREFIX)
            .is_some_and(|short_name| short_name == filter)
}

pub(super) fn short_player_name(bus_name: &str) -> String {
    bus_name
        .strip_prefix(super::backend::MPRIS_PREFIX)
        .unwrap_or(bus_name)
        .to_string()
}

pub(super) fn normalize_status(status: &str) -> String {
    status.to_ascii_lowercase()
}

pub(super) fn status_icon_for(status: &str) -> &'static str {
    match status {
        "playing" => "",
        "paused" => "",
        "stopped" => "",
        _ => "",
    }
}

pub(super) fn metadata_string(metadata: &HashMap<String, OwnedValue>, key: &str) -> Option<String> {
    metadata
        .get(key)
        .and_then(|value| value.try_clone().ok())
        .and_then(|value| String::try_from(value).ok())
        .filter(|value| !value.is_empty())
}

pub(super) fn metadata_artist(metadata: &HashMap<String, OwnedValue>) -> Option<String> {
    let value = metadata.get("xesam:artist")?.try_clone().ok()?;

    if let Ok(artists) = Vec::<String>::try_from(value.try_clone().ok()?) {
        let joined = artists
            .into_iter()
            .filter(|artist| !artist.is_empty())
            .collect::<Vec<_>>()
            .join(", ");
        if !joined.is_empty() {
            return Some(joined);
        }
    }

    String::try_from(value)
        .ok()
        .filter(|value| !value.is_empty())
}

pub(super) fn metadata_i64(metadata: &HashMap<String, OwnedValue>, key: &str) -> Option<i64> {
    let value = metadata.get(key)?.try_clone().ok()?;

    i64::try_from(value.try_clone().ok()?)
        .ok()
        .or_else(|| i32::try_from(value.try_clone().ok()?).ok().map(i64::from))
        .or_else(|| {
            u64::try_from(value.try_clone().ok()?)
                .ok()
                .and_then(|v| i64::try_from(v).ok())
        })
        .or_else(|| u32::try_from(value).ok().map(i64::from))
}

pub(super) fn metadata_object_path_string(
    metadata: &HashMap<String, OwnedValue>,
    key: &str,
) -> Option<String> {
    let value = metadata.get(key)?.try_clone().ok()?;
    ObjectPath::try_from(value)
        .ok()
        .map(|path| path.to_string())
        .filter(|path| !path.is_empty())
}

pub(super) fn render_format(format: &str, metadata: &PlayerctlMetadata) -> String {
    format
        .replace("{status}", &metadata.status)
        .replace("{status_icon}", metadata.status_icon)
        .replace("{player}", &metadata.player)
        .replace("{artist}", &metadata.artist)
        .replace("{album}", &metadata.album)
        .replace("{title}", &metadata.title)
}

pub(super) fn should_show_metadata(
    metadata: Option<&PlayerctlMetadata>,
    hide_when_idle: bool,
    show_when_paused: bool,
) -> bool {
    if !hide_when_idle {
        return true;
    }

    let Some(metadata) = metadata else {
        return false;
    };

    match metadata.status.as_str() {
        "playing" => true,
        "paused" => show_when_paused,
        _ => false,
    }
}

pub(super) fn status_css_class(status: &str) -> &'static str {
    match status {
        "playing" => "status-playing",
        "paused" => "status-paused",
        "stopped" => "status-stopped",
        _ => "no-player",
    }
}

pub(super) fn metadata_seek_ratio(metadata: &PlayerctlMetadata) -> Option<f64> {
    let position = metadata.position_micros?;
    let length = metadata.length_micros?;
    if length <= 0 {
        return None;
    }

    Some(position as f64 / length as f64)
}

pub(super) fn format_timestamp_micros(value: Option<i64>) -> String {
    let Some(micros) = value else {
        return "00:00".to_string();
    };
    let total_seconds = (micros.max(0) / 1_000_000) as u64;
    let minutes = total_seconds / 60;
    let seconds = total_seconds % 60;
    format!("{minutes:02}:{seconds:02}")
}

fn active_rank(status: &str) -> u8 {
    match status {
        "playing" => 0,
        "paused" => 1,
        "stopped" => 2,
        _ => 3,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_player_filter_accepts_full_and_short_names() {
        assert!(matches_player_filter(
            "org.mpris.MediaPlayer2.spotify",
            "org.mpris.MediaPlayer2.spotify"
        ));
        assert!(matches_player_filter(
            "org.mpris.MediaPlayer2.spotify",
            "spotify"
        ));
        assert!(!matches_player_filter(
            "org.mpris.MediaPlayer2.spotify",
            "mpv"
        ));
    }

    #[test]
    fn select_active_player_prefers_playing_then_name() {
        let chosen = select_active_player(vec![
            PlayerctlMetadata {
                status: "paused".to_string(),
                status_icon: "",
                player: "vlc".to_string(),
                artist: String::new(),
                album: String::new(),
                title: String::new(),
                position_micros: None,
                length_micros: None,
                can_go_previous: false,
                can_go_next: false,
                can_play: false,
                can_pause: false,
                can_seek: false,
                track_id: None,
                bus_name: "org.mpris.MediaPlayer2.vlc".to_string(),
            },
            PlayerctlMetadata {
                status: "playing".to_string(),
                status_icon: "",
                player: "spotify".to_string(),
                artist: String::new(),
                album: String::new(),
                title: String::new(),
                position_micros: None,
                length_micros: None,
                can_go_previous: false,
                can_go_next: false,
                can_play: false,
                can_pause: false,
                can_seek: false,
                track_id: None,
                bus_name: "org.mpris.MediaPlayer2.spotify".to_string(),
            },
        ])
        .expect("one player should be selected");

        assert_eq!(chosen.status, "playing");
        assert_eq!(chosen.bus_name, "org.mpris.MediaPlayer2.spotify");
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
            position_micros: None,
            length_micros: None,
            can_go_previous: false,
            can_go_next: false,
            can_play: false,
            can_pause: false,
            can_seek: false,
            track_id: None,
            bus_name: "org.mpris.MediaPlayer2.spotify".to_string(),
        };

        let text = render_format(
            "{status_icon} {artist} - {title} ({player}) [{status}]",
            &metadata,
        );
        assert_eq!(text, " Boards of Canada - Roygbiv (spotify) [paused]");
    }

    #[test]
    fn should_show_metadata_respects_visibility_settings() {
        let playing = PlayerctlMetadata {
            status: "playing".to_string(),
            status_icon: "",
            player: String::new(),
            artist: String::new(),
            album: String::new(),
            title: String::new(),
            position_micros: None,
            length_micros: None,
            can_go_previous: false,
            can_go_next: false,
            can_play: false,
            can_pause: false,
            can_seek: false,
            track_id: None,
            bus_name: String::new(),
        };
        let paused = PlayerctlMetadata {
            status: "paused".to_string(),
            ..playing.clone()
        };
        let stopped = PlayerctlMetadata {
            status: "stopped".to_string(),
            ..playing.clone()
        };

        assert!(should_show_metadata(Some(&playing), true, true));
        assert!(should_show_metadata(Some(&paused), true, true));
        assert!(!should_show_metadata(Some(&paused), true, false));
        assert!(!should_show_metadata(Some(&stopped), true, true));
        assert!(!should_show_metadata(None, true, true));
        assert!(should_show_metadata(None, false, false));
    }

    #[test]
    fn status_css_class_maps_statuses() {
        assert_eq!(status_css_class("playing"), "status-playing");
        assert_eq!(status_css_class("paused"), "status-paused");
        assert_eq!(status_css_class("stopped"), "status-stopped");
        assert_eq!(status_css_class("unknown"), "no-player");
    }

    #[test]
    fn metadata_seek_ratio_handles_expected_cases() {
        let metadata = PlayerctlMetadata {
            status: "playing".to_string(),
            status_icon: "",
            player: String::new(),
            artist: String::new(),
            album: String::new(),
            title: String::new(),
            position_micros: Some(30_000_000),
            length_micros: Some(120_000_000),
            can_go_previous: false,
            can_go_next: false,
            can_play: false,
            can_pause: false,
            can_seek: true,
            track_id: Some("/org/mpris/MediaPlayer2/track/1".to_string()),
            bus_name: String::new(),
        };
        assert_eq!(metadata_seek_ratio(&metadata), Some(0.25));

        let zero_length = PlayerctlMetadata {
            length_micros: Some(0),
            ..metadata.clone()
        };
        assert_eq!(metadata_seek_ratio(&zero_length), None);

        let missing_position = PlayerctlMetadata {
            position_micros: None,
            ..metadata
        };
        assert_eq!(metadata_seek_ratio(&missing_position), None);
    }

    #[test]
    fn format_timestamp_micros_formats_mm_ss() {
        assert_eq!(format_timestamp_micros(None), "00:00");
        assert_eq!(format_timestamp_micros(Some(-5)), "00:00");
        assert_eq!(format_timestamp_micros(Some(5_000_000)), "00:05");
        assert_eq!(format_timestamp_micros(Some(65_000_000)), "01:05");
    }
}
