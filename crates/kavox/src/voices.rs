//! Voice changer UI metadata mirroring `kavox-core::voice`.

use crate::icons::IconName;

pub const NORMAL: &str = kavox_core::voice::NORMAL;
pub const CUSTOM: &str = kavox_core::voice::CUSTOM;

#[derive(Clone, Copy)]
pub struct VoiceInfo {
    pub id: &'static str,
    pub name: &'static str,
    pub icon: IconName,
    pub color: &'static str,
    pub semitones: f32,
    pub wobble: bool,
}

pub const VOICES: &[VoiceInfo] = &[
    VoiceInfo {
        id: "chipmunk",
        name: "Chipmunk",
        icon: IconName::Chipmunk,
        color: "#f0a020",
        semitones: 10.0,
        wobble: false,
    },
    VoiceInfo {
        id: "pixie",
        name: "Pixie",
        icon: IconName::Pixie,
        color: "#f0607e",
        semitones: 5.0,
        wobble: false,
    },
    VoiceInfo {
        id: "deep",
        name: "Deep",
        icon: IconName::Deep,
        color: "#3b9cff",
        semitones: -4.0,
        wobble: false,
    },
    VoiceInfo {
        id: "giant",
        name: "Giant",
        icon: IconName::Giant,
        color: "#7d8c5a",
        semitones: -8.0,
        wobble: false,
    },
    VoiceInfo {
        id: "monster",
        name: "Monster",
        icon: IconName::Monster,
        color: "#a3a29d",
        semitones: -12.0,
        wobble: false,
    },
    VoiceInfo {
        id: "wobbly",
        name: "Wobbly",
        icon: IconName::Wobbly,
        color: "#1fb5a0",
        semitones: 0.0,
        wobble: true,
    },
    VoiceInfo {
        id: "drunk",
        name: "Drunk",
        icon: IconName::Drunk,
        color: "#c9ad7a",
        semitones: -2.0,
        wobble: true,
    },
    VoiceInfo {
        id: "alien",
        name: "Alien",
        icon: IconName::Alien,
        color: "#1fb5a0",
        semitones: 3.0,
        wobble: true,
    },
];

pub fn voice_name(id: &str) -> &'static str {
    if id == CUSTOM {
        return "Custom";
    }
    VOICES
        .iter()
        .find(|v| v.id == id)
        .map(|v| v.name)
        .unwrap_or("Normal")
}

/// "+5 st", "−4 st" or "0 st", with a real minus sign.
pub fn format_semitones(st: f32) -> String {
    let rounded = (st * 10.0).round() / 10.0;
    if rounded == 0.0 {
        return "0 st".into();
    }
    let sign = if rounded > 0.0 { "+" } else { "−" };
    let abs = rounded.abs();
    if (abs - abs.round()).abs() < 0.01 {
        format!("{sign}{} st", abs.round() as i32)
    } else {
        format!("{sign}{abs:.1} st")
    }
}
