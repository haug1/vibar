use super::config::{
    PulseAudioFormatIcons, ICON_CAR, ICON_HANDS_FREE, ICON_HEADPHONE, ICON_HEADSET, ICON_PHONE,
    ICON_PORTABLE, ICON_VOLUME_HIGH, ICON_VOLUME_LOW, ICON_VOLUME_MEDIUM,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum IconKind {
    Headphone,
    Speaker,
    Hdmi,
    HandsFree,
    Headset,
    Phone,
    Portable,
    Car,
    Hifi,
    Default,
}

pub(super) fn classify_icon_kind_by_priority(content: &str) -> IconKind {
    if content.contains("headphone") {
        return IconKind::Headphone;
    }
    if content.contains("speaker") {
        return IconKind::Speaker;
    }
    if content.contains("hdmi") {
        return IconKind::Hdmi;
    }
    if content.contains("headset") {
        return IconKind::Headset;
    }
    if content.contains("hands-free") || content.contains("handsfree") {
        return IconKind::HandsFree;
    }
    if content.contains("portable") {
        return IconKind::Portable;
    }
    if contains_word(content, "car") {
        return IconKind::Car;
    }
    if content.contains("hifi") {
        return IconKind::Hifi;
    }
    if content.contains("phone") {
        return IconKind::Phone;
    }
    IconKind::Default
}

fn contains_word(content: &str, needle: &str) -> bool {
    content
        .split(|c: char| !c.is_ascii_alphanumeric() && c != '-')
        .any(|token| token == needle)
}

impl PulseAudioFormatIcons {
    pub(super) fn icon_for(&self, kind: IconKind, volume: u32) -> String {
        match kind {
            IconKind::Headphone => self
                .headphone
                .as_deref()
                .unwrap_or(ICON_HEADPHONE)
                .to_string(),
            IconKind::Speaker => self
                .speaker
                .as_deref()
                .unwrap_or(ICON_VOLUME_HIGH)
                .to_string(),
            IconKind::Hdmi => self.hdmi.as_deref().unwrap_or(ICON_VOLUME_HIGH).to_string(),
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
            IconKind::Hifi => self.hifi.as_deref().unwrap_or(ICON_VOLUME_HIGH).to_string(),
            IconKind::Default => volume_icon_from_list(&self.default, volume),
        }
    }
}

pub(super) fn volume_icon_from_list(icons: &[String], volume: u32) -> String {
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
