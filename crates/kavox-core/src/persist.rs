//! Crash-safe JSON files: atomic writes, a `.bak` of the last good file, and recovery from corruption.

use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::error::Result;

pub struct Loaded<T> {
    pub value: T,
    /// Set when the file was newer than this build understands; saving would lose data.
    pub read_only: bool,
    pub notice: Option<String>,
}

/// Something stored with a `schema` version.
pub trait Versioned: DeserializeOwned + Default {
    const CURRENT: u32;
    fn schema(&self) -> u32;
    /// Upgrades older files in place; called only when `schema() < CURRENT`.
    fn migrate(&mut self) {}
}

pub fn load<T: Versioned>(path: &Path) -> Loaded<T> {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    match read::<T>(path) {
        Ok(Some(value)) => {
            let _ = fs::copy(path, sibling(path, "bak"));
            finish(value, None)
        }
        Ok(None) => Loaded {
            value: T::default(),
            read_only: false,
            notice: None,
        },
        Err(e) => {
            tracing::warn!("{name} is unreadable: {e}");
            let stamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            let _ = fs::rename(path, sibling(path, &format!("corrupt-{stamp}")));
            match read::<T>(&sibling(path, "bak")) {
                Ok(Some(value)) => finish(
                    value,
                    Some(format!(
                        "{name} was damaged, so Kavox restored the last backup."
                    )),
                ),
                _ => Loaded {
                    value: T::default(),
                    read_only: false,
                    notice: Some(format!(
                        "{name} was damaged and had no backup, so Kavox started fresh."
                    )),
                },
            }
        }
    }
}

fn finish<T: Versioned>(mut value: T, notice: Option<String>) -> Loaded<T> {
    if value.schema() > T::CURRENT {
        return Loaded {
            value,
            read_only: true,
            notice: Some(
                "Your data was saved by a newer Kavox. Changes won't be saved until you update."
                    .into(),
            ),
        };
    }
    if value.schema() < T::CURRENT {
        value.migrate();
    }
    Loaded {
        value,
        read_only: false,
        notice,
    }
}

fn read<T: DeserializeOwned>(path: &Path) -> Result<Option<T>> {
    match fs::read(path) {
        Ok(bytes) => Ok(Some(serde_json::from_slice(&bytes)?)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// Writes `value` to a temp file, fsyncs it, then renames it over `path`.
pub fn save<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let tmp = sibling(path, "tmp");
    let mut file = File::create(&tmp)?;
    file.write_all(&serde_json::to_vec_pretty(value)?)?;
    file.sync_all()?;
    fs::rename(&tmp, path)?;
    Ok(())
}

fn sibling(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".");
    name.push(suffix);
    path.with_file_name(name)
}

#[cfg(test)]
mod tests {
    use serde::Deserialize;

    use super::*;
    use crate::clip::tests::temp_dir;

    #[derive(Debug, Default, PartialEq, Serialize, Deserialize)]
    struct Doc {
        schema: u32,
        name: String,
    }

    impl Versioned for Doc {
        const CURRENT: u32 = 2;
        fn schema(&self) -> u32 {
            self.schema
        }
        fn migrate(&mut self) {
            self.name = format!("{} (migrated)", self.name);
            self.schema = 2;
        }
    }

    #[test]
    fn round_trips_and_keeps_backup() {
        let dir = temp_dir("persist");
        let path = dir.join("doc.json");
        assert_eq!(load::<Doc>(&path).value, Doc::default());
        save(
            &path,
            &Doc {
                schema: 2,
                name: "a".into(),
            },
        )
        .unwrap();
        let loaded = load::<Doc>(&path);
        assert_eq!(loaded.value.name, "a");
        assert!(dir.join("doc.json.bak").exists());
        assert!(!dir.join("doc.json.tmp").exists());
    }

    #[test]
    fn recovers_from_corruption_with_backup() {
        let dir = temp_dir("corrupt");
        let path = dir.join("doc.json");
        save(
            &path,
            &Doc {
                schema: 2,
                name: "good".into(),
            },
        )
        .unwrap();
        load::<Doc>(&path);
        fs::write(&path, b"{ broken").unwrap();
        let loaded = load::<Doc>(&path);
        assert_eq!(loaded.value.name, "good");
        assert!(loaded.notice.is_some());
        assert!(fs::read_dir(&dir).unwrap().any(|e| {
            e.unwrap()
                .file_name()
                .to_string_lossy()
                .contains("corrupt-")
        }));
    }

    #[test]
    fn migrates_old_and_protects_new_schemas() {
        let dir = temp_dir("schema");
        let path = dir.join("doc.json");
        save(
            &path,
            &Doc {
                schema: 1,
                name: "old".into(),
            },
        )
        .unwrap();
        assert_eq!(load::<Doc>(&path).value.name, "old (migrated)");
        save(
            &path,
            &Doc {
                schema: 9,
                name: "future".into(),
            },
        )
        .unwrap();
        assert!(load::<Doc>(&path).read_only);
    }
}
