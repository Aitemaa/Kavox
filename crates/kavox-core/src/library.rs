//! Boards and sounds (`boards.json`), plus importing files into the library folder.

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use ulid::Ulid;

use crate::clip;
use crate::error::{Error, Result};
use crate::persist::Versioned;

pub const LIBRARY_SCHEMA: u32 = 1;
/// Pad dot colours, readable on dark and light pads. Keep in sync with PALETTE in src/api.ts.
pub const PALETTE: &[&str] = &[
    "#e8262d", "#f0a020", "#c9ad7a", "#1fb5a0", "#3b9cff", "#f0607e", "#a3a29d", "#7d8c5a",
];
const MAX_NAME_CHARS: usize = 60;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Retrigger {
    #[default]
    Restart,
    Overlap,
    Toggle,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Sound {
    pub id: String,
    pub name: String,
    /// File name inside the library's sounds folder.
    pub file: String,
    pub sha256: String,
    pub duration_ms: u64,
    pub volume: f32,
    pub norm_gain_db: f32,
    pub color: String,
    #[serde(default)]
    pub hotkey: Option<String>,
    #[serde(default)]
    pub retrigger: Retrigger,
    #[serde(default, rename = "loop")]
    pub looping: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Board {
    pub id: String,
    pub name: String,
    pub sounds: Vec<String>,
}

/// Fields the UI may change on a sound (hotkeys go through their own path).
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SoundPatch {
    pub name: Option<String>,
    pub volume: Option<f32>,
    pub color: Option<String>,
    pub retrigger: Option<Retrigger>,
    #[serde(rename = "loop")]
    pub looping: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Library {
    pub schema: u32,
    pub active_board: String,
    pub boards: Vec<Board>,
    pub sounds: BTreeMap<String, Sound>,
}

impl Default for Library {
    fn default() -> Self {
        let board = Board {
            id: Ulid::generate().to_string(),
            name: "Main".into(),
            sounds: Vec::new(),
        };
        Self {
            schema: LIBRARY_SCHEMA,
            active_board: board.id.clone(),
            boards: vec![board],
            sounds: BTreeMap::new(),
        }
    }
}

impl Versioned for Library {
    const CURRENT: u32 = LIBRARY_SCHEMA;
    fn schema(&self) -> u32 {
        self.schema
    }
}

impl Library {
    /// Fixes references after loading a hand-edited or partially written file.
    pub fn repair(&mut self) {
        if self.boards.is_empty() {
            *self = Self {
                sounds: std::mem::take(&mut self.sounds),
                ..Self::default()
            };
        }
        let mut seen = std::collections::HashSet::new();
        for board in &mut self.boards {
            board
                .sounds
                .retain(|id| self.sounds.contains_key(id) && seen.insert(id.clone()));
        }
        let orphans: Vec<String> = self
            .sounds
            .keys()
            .filter(|id| !seen.contains(*id))
            .cloned()
            .collect();
        self.boards[0].sounds.extend(orphans);
        if !self.boards.iter().any(|b| b.id == self.active_board) {
            self.active_board = self.boards[0].id.clone();
        }
    }

    pub fn sound(&self, id: &str) -> Result<&Sound> {
        self.sounds
            .get(id)
            .ok_or_else(|| Error::SoundNotFound(id.to_owned()))
    }

    pub fn board(&self, id: &str) -> Result<&Board> {
        self.boards
            .iter()
            .find(|b| b.id == id)
            .ok_or(Error::BoardNotFound)
    }

    fn board_mut(&mut self, id: &str) -> Result<&mut Board> {
        self.boards
            .iter_mut()
            .find(|b| b.id == id)
            .ok_or(Error::BoardNotFound)
    }

    pub fn board_of(&self, sound: &str) -> Option<&Board> {
        self.boards
            .iter()
            .find(|b| b.sounds.iter().any(|s| s == sound))
    }

    pub fn add_sound(&mut self, board: &str, sound: Sound) -> Result<()> {
        let id = sound.id.clone();
        self.board_mut(board)?.sounds.push(id.clone());
        self.sounds.insert(id, sound);
        Ok(())
    }

    pub fn update_sound(&mut self, id: &str, patch: SoundPatch) -> Result<&Sound> {
        let sound = self
            .sounds
            .get_mut(id)
            .ok_or_else(|| Error::SoundNotFound(id.to_owned()))?;
        if let Some(name) = patch.name {
            let name = clean_name(&name);
            if name.is_empty() {
                return Err(Error::Invalid("Name can't be empty".into()));
            }
            sound.name = name;
        }
        if let Some(volume) = patch.volume {
            sound.volume = if volume.is_finite() {
                volume.clamp(0.0, 2.0)
            } else {
                1.0
            };
        }
        if let Some(color) = patch.color {
            sound.color = color;
        }
        if let Some(retrigger) = patch.retrigger {
            sound.retrigger = retrigger;
        }
        if let Some(looping) = patch.looping {
            sound.looping = looping;
        }
        Ok(sound)
    }

    pub fn set_hotkey(&mut self, id: &str, hotkey: Option<String>) -> Result<()> {
        self.sounds
            .get_mut(id)
            .ok_or_else(|| Error::SoundNotFound(id.to_owned()))?
            .hotkey = hotkey;
        Ok(())
    }

    pub fn remove_sound(&mut self, id: &str) -> Result<Sound> {
        let sound = self
            .sounds
            .remove(id)
            .ok_or_else(|| Error::SoundNotFound(id.to_owned()))?;
        for board in &mut self.boards {
            board.sounds.retain(|s| s != id);
        }
        Ok(sound)
    }

    pub fn reorder(&mut self, board: &str, ids: Vec<String>) -> Result<()> {
        let board = self.board_mut(board)?;
        let mut current = board.sounds.clone();
        let mut proposed = ids.clone();
        current.sort();
        proposed.sort();
        if current != proposed {
            return Err(Error::Invalid(
                "Reorder must list the board's sounds exactly once".into(),
            ));
        }
        board.sounds = ids;
        Ok(())
    }

    pub fn create_board(&mut self, name: &str) -> Result<&Board> {
        let name = clean_name(name);
        if name.is_empty() {
            return Err(Error::Invalid("Board name can't be empty".into()));
        }
        self.boards.push(Board {
            id: Ulid::generate().to_string(),
            name,
            sounds: Vec::new(),
        });
        Ok(self.boards.last().expect("just pushed"))
    }

    pub fn rename_board(&mut self, id: &str, name: &str) -> Result<()> {
        let name = clean_name(name);
        if name.is_empty() {
            return Err(Error::Invalid("Board name can't be empty".into()));
        }
        self.board_mut(id)?.name = name;
        Ok(())
    }

    /// Removes the board and its sounds; the last board can't be deleted.
    pub fn delete_board(&mut self, id: &str) -> Result<Vec<Sound>> {
        if self.boards.len() <= 1 {
            return Err(Error::Invalid("Kavox needs at least one board".into()));
        }
        let index = self
            .boards
            .iter()
            .position(|b| b.id == id)
            .ok_or(Error::BoardNotFound)?;
        let board = self.boards.remove(index);
        if self.active_board == id {
            self.active_board = self.boards[index.saturating_sub(1).min(self.boards.len() - 1)]
                .id
                .clone();
        }
        Ok(board
            .sounds
            .iter()
            .filter_map(|s| self.sounds.remove(s))
            .collect())
    }

    pub fn set_active_board(&mut self, id: &str) -> Result<()> {
        self.board(id)?;
        self.active_board = id.to_owned();
        Ok(())
    }

    pub fn file_in_use(&self, file: &str) -> bool {
        self.sounds.values().any(|s| s.file == file)
    }

    /// sha256 → file name, so re-importing the same audio reuses the stored file.
    pub fn hashes(&self) -> HashMap<String, String> {
        self.sounds
            .values()
            .map(|s| (s.sha256.clone(), s.file.clone()))
            .collect()
    }

    /// Finds a sound by id, then exact name, then unique name prefix (case-insensitive).
    /// Among several matches, a single one on the active board wins.
    pub fn resolve(&self, target: &str) -> Result<&Sound> {
        let target = target.trim();
        if let Some(sound) = self.sounds.get(target) {
            return Ok(sound);
        }
        let lower = target.to_lowercase();
        let rules: [&dyn Fn(&Sound) -> bool; 2] = [&|s| s.name.to_lowercase() == lower, &|s| {
            s.name.to_lowercase().starts_with(&lower)
        }];
        for rule in rules {
            let matches: Vec<&Sound> = self.sounds.values().filter(|s| rule(s)).collect();
            match matches.len() {
                0 => continue,
                1 => return Ok(matches[0]),
                _ => {
                    let active = self
                        .board(&self.active_board)
                        .map(|b| b.sounds.as_slice())
                        .unwrap_or_default();
                    let on_active: Vec<&&Sound> =
                        matches.iter().filter(|s| active.contains(&s.id)).collect();
                    if on_active.len() == 1 {
                        return Ok(on_active[0]);
                    }
                    let names: Vec<&str> = matches.iter().map(|s| s.name.as_str()).collect();
                    return Err(Error::AmbiguousSound(target.to_owned(), names.join(", ")));
                }
            }
        }
        Err(Error::SoundNotFound(target.to_owned()))
    }
}

/// Copies an audio file into the library folder and analyzes it. Runs without the core lock.
pub fn prepare_import(
    src: &Path,
    sounds_dir: &Path,
    hashes: &HashMap<String, String>,
) -> Result<Sound> {
    let file_name = src
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    if !clip::is_supported(src) {
        return Err(Error::UnsupportedFormat(file_name));
    }
    let bytes = fs::read(src)?;
    let sha256: String = Sha256::digest(&bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let decoded = clip::decode(src)?;
    let analysis = clip::analyze(&decoded);

    let id = Ulid::generate().to_string();
    let file = match hashes.get(&sha256) {
        Some(existing) if sounds_dir.join(existing).exists() => existing.clone(),
        _ => {
            let ext = src
                .extension()
                .map(|e| e.to_string_lossy().to_ascii_lowercase())
                .unwrap_or_default();
            let file = format!("{id}.{ext}");
            fs::create_dir_all(sounds_dir)?;
            fs::write(sounds_dir.join(&file), &bytes)?;
            file
        }
    };
    let stem = src
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let name = match clean_name(&stem.replace(['_', '-'], " ")) {
        n if n.is_empty() => "Sound".to_owned(),
        n => n,
    };
    let color = PALETTE[sha256.bytes().next().unwrap_or(0) as usize % PALETTE.len()].to_owned();
    Ok(Sound {
        id,
        name,
        file,
        sha256,
        duration_ms: analysis.duration_ms,
        volume: 1.0,
        norm_gain_db: analysis.norm_gain_db,
        color,
        hotkey: None,
        retrigger: Retrigger::default(),
        looping: false,
    })
}

fn clean_name(name: &str) -> String {
    name.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(MAX_NAME_CHARS)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clip::tests::{temp_dir, write_wav};

    fn sound(id: &str, name: &str) -> Sound {
        Sound {
            id: id.into(),
            name: name.into(),
            file: format!("{id}.wav"),
            sha256: id.into(),
            duration_ms: 1000,
            volume: 1.0,
            norm_gain_db: 0.0,
            color: PALETTE[0].into(),
            hotkey: None,
            retrigger: Retrigger::Restart,
            looping: false,
        }
    }

    fn library() -> Library {
        let mut lib = Library::default();
        let main = lib.active_board.clone();
        lib.add_sound(&main, sound("a", "Airhorn")).unwrap();
        lib.add_sound(&main, sound("b", "Air raid")).unwrap();
        let other = lib.create_board("Memes").unwrap().id.clone();
        lib.add_sound(&other, sound("c", "Bruh")).unwrap();
        lib.add_sound(&other, sound("d", "airhorn")).unwrap();
        lib
    }

    #[test]
    fn resolves_by_id_name_and_prefix() {
        let lib = library();
        assert_eq!(lib.resolve("c").unwrap().name, "Bruh");
        assert_eq!(lib.resolve("bru").unwrap().id, "c");
        // "airhorn" exists on two boards; the active board's wins.
        assert_eq!(lib.resolve("AIRHORN").unwrap().id, "a");
        assert!(matches!(lib.resolve("air"), Err(Error::AmbiguousSound(..))));
        assert!(matches!(lib.resolve("nope"), Err(Error::SoundNotFound(_))));
    }

    #[test]
    fn reorder_requires_same_sounds() {
        let mut lib = library();
        let main = lib.active_board.clone();
        lib.reorder(&main, vec!["b".into(), "a".into()]).unwrap();
        assert_eq!(lib.board(&main).unwrap().sounds, ["b", "a"]);
        assert!(lib.reorder(&main, vec!["a".into()]).is_err());
        assert!(lib.reorder(&main, vec!["a".into(), "c".into()]).is_err());
    }

    #[test]
    fn deleting_board_removes_its_sounds_but_not_the_last_board() {
        let mut lib = library();
        let memes = lib.boards[1].id.clone();
        lib.set_active_board(&memes).unwrap();
        let removed = lib.delete_board(&memes).unwrap();
        assert_eq!(removed.len(), 2);
        assert_eq!(lib.active_board, lib.boards[0].id);
        assert!(!lib.sounds.contains_key("c"));
        let last = lib.boards[0].id.clone();
        assert!(lib.delete_board(&last).is_err());
    }

    #[test]
    fn repair_fixes_dangling_references() {
        let mut lib = library();
        lib.boards[0].sounds.push("ghost".into());
        lib.boards[1].sounds.push("a".into());
        lib.boards[1].sounds.retain(|s| s != "c");
        lib.active_board = "missing".into();
        lib.repair();
        assert!(!lib.boards[0].sounds.contains(&"ghost".to_string()));
        assert_eq!(
            lib.boards
                .iter()
                .filter(|b| b.sounds.contains(&"a".to_string()))
                .count(),
            1
        );
        assert!(lib.boards[0].sounds.contains(&"c".to_string()));
        assert_eq!(lib.active_board, lib.boards[0].id);
    }

    #[test]
    fn patch_validates_fields() {
        let mut lib = library();
        let s = lib
            .update_sound(
                "a",
                SoundPatch {
                    name: Some("  Big   horn ".into()),
                    volume: Some(5.0),
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(s.name, "Big horn");
        assert_eq!(s.volume, 2.0);
        assert!(
            lib.update_sound(
                "a",
                SoundPatch {
                    name: Some("   ".into()),
                    ..Default::default()
                }
            )
            .is_err()
        );
    }

    #[test]
    fn import_copies_once_per_content() {
        let src_dir = temp_dir("import-src");
        let sounds_dir = temp_dir("import-dst");
        let src = src_dir.join("vine_boom.wav");
        write_wav(&src, 48_000, 2, &vec![0.2; 48_000]);
        let first = prepare_import(&src, &sounds_dir, &HashMap::new()).unwrap();
        assert_eq!(first.name, "vine boom");
        assert_eq!(first.duration_ms, 500);
        assert!(sounds_dir.join(&first.file).exists());

        let hashes = HashMap::from([(first.sha256.clone(), first.file.clone())]);
        let second = prepare_import(&src, &sounds_dir, &hashes).unwrap();
        assert_ne!(first.id, second.id);
        assert_eq!(first.file, second.file);
        assert_eq!(fs::read_dir(&sounds_dir).unwrap().count(), 1);

        let txt = src_dir.join("notes.txt");
        fs::write(&txt, "hi").unwrap();
        assert!(matches!(
            prepare_import(&txt, &sounds_dir, &hashes),
            Err(Error::UnsupportedFormat(_))
        ));
    }
}
