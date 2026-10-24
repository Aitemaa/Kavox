//! Voice changer presets. The UI shows names and icons by `id`; the numbers live here so hotkeys and the tray agree.

pub struct VoicePreset {
    pub id: &'static str,
    pub semitones: f32,
    /// Peak swing of the pitch wobble in semitones; zero for a steady voice.
    pub wobble_depth: f32,
    pub wobble_hz: f32,
}

/// Selected when the voice changer has no preset; passes the mic through unchanged.
pub const NORMAL: &str = "normal";
/// Selected when the pitch dial drives the shift.
pub const CUSTOM: &str = "custom";

pub const PRESETS: &[VoicePreset] = &[
    VoicePreset {
        id: "chipmunk",
        semitones: 10.0,
        wobble_depth: 0.0,
        wobble_hz: 0.0,
    },
    VoicePreset {
        id: "pixie",
        semitones: 5.0,
        wobble_depth: 0.0,
        wobble_hz: 0.0,
    },
    VoicePreset {
        id: "deep",
        semitones: -4.0,
        wobble_depth: 0.0,
        wobble_hz: 0.0,
    },
    VoicePreset {
        id: "giant",
        semitones: -8.0,
        wobble_depth: 0.0,
        wobble_hz: 0.0,
    },
    VoicePreset {
        id: "monster",
        semitones: -12.0,
        wobble_depth: 0.0,
        wobble_hz: 0.0,
    },
    VoicePreset {
        id: "wobbly",
        semitones: 0.0,
        wobble_depth: 1.5,
        wobble_hz: 5.0,
    },
    VoicePreset {
        id: "drunk",
        semitones: -2.0,
        wobble_depth: 1.5,
        wobble_hz: 0.7,
    },
    VoicePreset {
        id: "alien",
        semitones: 3.0,
        wobble_depth: 0.7,
        wobble_hz: 12.0,
    },
];

pub fn preset(id: &str) -> Option<&'static VoicePreset> {
    PRESETS.iter().find(|p| p.id == id)
}

/// Name for hotkey conflict messages and the tray.
pub fn label(id: &str) -> String {
    let mut chars = id.chars();
    chars
        .next()
        .map(|c| c.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preset_ids_are_unique_and_in_range() {
        for (i, p) in PRESETS.iter().enumerate() {
            assert!(
                PRESETS[..i].iter().all(|q| q.id != p.id),
                "{} repeats",
                p.id
            );
            assert!((-12.0..=12.0).contains(&p.semitones));
            assert!(p.id != NORMAL && p.id != CUSTOM);
        }
    }

    #[test]
    fn labels_are_capitalised() {
        assert_eq!(label("chipmunk"), "Chipmunk");
        assert_eq!(label(""), "");
    }
}
