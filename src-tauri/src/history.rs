use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::app_info::{EDITOR_MAX_INPUT_CHARS, sanitize_log_value};

pub const HISTORY_VERSION: u32 = 1;
pub const MAX_HISTORY_ENTRIES: usize = 20;
const MAX_HISTORY_FILE_BYTES: u64 = 10 * 1024 * 1024;
const MAX_META_CHARS: usize = 128;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewHistoryEntry {
    pub action: String,
    pub source_app: Option<String>,
    pub model: String,
    pub cost_usd: Option<f64>,
    pub original: String,
    pub result: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryEntry {
    pub id: String,
    pub timestamp_ms: u64,
    pub action: String,
    pub source_app: Option<String>,
    pub model: String,
    pub cost_usd: Option<f64>,
    pub original: String,
    pub result: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct HistoryFile {
    #[serde(default = "default_version")]
    version: u32,
    #[serde(default)]
    entries: Vec<HistoryEntry>,
}

fn default_version() -> u32 {
    HISTORY_VERSION
}

impl Default for HistoryFile {
    fn default() -> Self {
        Self {
            version: HISTORY_VERSION,
            entries: Vec::new(),
        }
    }
}

pub struct HistoryStore {
    path: PathBuf,
    entries: Vec<HistoryEntry>,
    pub recovered_from_corrupt: bool,
}

pub fn history_path(data_dir: &Path) -> PathBuf {
    data_dir.join("history.json")
}

fn corrupt_backup_path(path: &Path) -> PathBuf {
    path.with_file_name("history.bad.json")
}

fn truncate_chars(value: &str, max_chars: usize) -> String {
    value.chars().take(max_chars).collect()
}

impl HistoryStore {
    pub fn load_from_path(path: PathBuf) -> Self {
        if !path.exists() {
            let has_backup = corrupt_backup_path(&path).exists();
            return Self {
                path,
                entries: Vec::new(),
                recovered_from_corrupt: has_backup,
            };
        }

        if let Ok(meta) = fs::metadata(&path)
            && meta.len() > MAX_HISTORY_FILE_BYTES
        {
            log::warn!(
                "history file exceeds size cap ({} bytes); backing up to history.bad.json",
                meta.len()
            );
            backup_corrupt_file(&path);
            return Self {
                path,
                entries: Vec::new(),
                recovered_from_corrupt: true,
            };
        }

        match fs::read_to_string(&path) {
            Ok(raw) => match serde_json::from_str::<HistoryFile>(&raw) {
                Ok(mut parsed) => {
                    parsed.entries.truncate(MAX_HISTORY_ENTRIES);
                    let has_backup = corrupt_backup_path(&path).exists();
                    Self {
                        path,
                        entries: parsed.entries,
                        recovered_from_corrupt: has_backup,
                    }
                }
                Err(err) => {
                    log::warn!(
                        "history JSON invalid (kind={:?} line={} col={}); backing up to history.bad.json",
                        err.classify(),
                        err.line(),
                        err.column()
                    );
                    backup_corrupt_file(&path);
                    Self {
                        path,
                        entries: Vec::new(),
                        recovered_from_corrupt: true,
                    }
                }
            },
            Err(err) => {
                log::warn!(
                    "failed reading history file ({}); starting empty",
                    err.kind()
                );
                backup_corrupt_file(&path);
                Self {
                    path,
                    entries: Vec::new(),
                    recovered_from_corrupt: true,
                }
            }
        }
    }

    pub fn entries(&self) -> Vec<HistoryEntry> {
        self.entries.clone()
    }

    /// Records a new history entry when `enabled` is true, capping at `MAX_HISTORY_ENTRIES` and
    /// bounding field lengths.
    pub fn record(&mut self, enabled: bool, new_entry: NewHistoryEntry) -> std::io::Result<()> {
        if !enabled {
            return Ok(());
        }

        let timestamp_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        let id = format!("{timestamp_ms}-{}", self.entries.len());

        let action = sanitize_log_value(&new_entry.action, MAX_META_CHARS);
        let source_app = new_entry
            .source_app
            .as_deref()
            .map(|s| sanitize_log_value(s, MAX_META_CHARS))
            .filter(|s| !s.is_empty());
        let model = sanitize_log_value(&new_entry.model, MAX_META_CHARS);
        let cost_usd = new_entry.cost_usd.filter(|c| c.is_finite() && *c >= 0.0);

        self.entries.insert(
            0,
            HistoryEntry {
                id,
                timestamp_ms,
                action,
                source_app,
                model,
                cost_usd,
                original: truncate_chars(&new_entry.original, EDITOR_MAX_INPUT_CHARS),
                result: truncate_chars(&new_entry.result, EDITOR_MAX_INPUT_CHARS),
            },
        );
        self.entries.truncate(MAX_HISTORY_ENTRIES);
        self.persist()
    }

    /// Clears all in-memory and on-disk history, including any corrupt-recovery `history.bad.json`.
    pub fn clear(&mut self) -> std::io::Result<()> {
        self.entries.clear();
        self.recovered_from_corrupt = false;
        let bad_path = corrupt_backup_path(&self.path);
        if bad_path.exists() {
            fs::remove_file(&bad_path)?;
        }
        self.persist()
    }

    fn persist(&self) -> std::io::Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        let payload = HistoryFile {
            version: HISTORY_VERSION,
            entries: self.entries.clone(),
        };
        let serialized = serde_json::to_string_pretty(&payload)
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidData, err))?;
        let tmp_path = self.path.with_extension("json.tmp");
        {
            let mut file = File::create(&tmp_path)?;
            file.write_all(serialized.as_bytes())?;
            file.sync_all()?;
        }
        fs::rename(&tmp_path, &self.path)?;
        Ok(())
    }
}

fn backup_corrupt_file(path: &Path) {
    let bad_path = corrupt_backup_path(path);
    if let Err(err) = fs::rename(path, &bad_path) {
        log::warn!("could not rename corrupt history file: {}", err.kind());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(label: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("redline_history_{label}_{nanos}"));
        fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    #[test]
    fn enforces_twenty_entry_cap_and_persists_roundtrip() {
        let dir = temp_dir("cap");
        let path = history_path(&dir);
        let mut store = HistoryStore::load_from_path(path.clone());

        for i in 1..=25 {
            store
                .record(
                    true,
                    NewHistoryEntry {
                        action: format!("Action {i}"),
                        source_app: Some("notepad.exe".to_string()),
                        model: "Harper".to_string(),
                        cost_usd: Some(0.0),
                        original: format!("orig {i}"),
                        result: format!("res {i}"),
                    },
                )
                .expect("record");
        }

        assert_eq!(store.entries().len(), MAX_HISTORY_ENTRIES);
        assert_eq!(store.entries()[0].action, "Action 25");
        assert_eq!(store.entries()[19].action, "Action 6");

        let reloaded = HistoryStore::load_from_path(path);
        assert!(!reloaded.recovered_from_corrupt);
        assert_eq!(reloaded.entries().len(), MAX_HISTORY_ENTRIES);
        assert_eq!(reloaded.entries()[0].action, "Action 25");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn respects_off_switch_and_clear() {
        let dir = temp_dir("off");
        let path = history_path(&dir);
        let mut store = HistoryStore::load_from_path(path);

        store
            .record(
                false,
                NewHistoryEntry {
                    action: "Improve".to_string(),
                    source_app: None,
                    model: "anthropic/claude-haiku-5.5".to_string(),
                    cost_usd: Some(0.00002),
                    original: "a".to_string(),
                    result: "b".to_string(),
                },
            )
            .expect("record off");
        assert!(store.entries().is_empty());

        store
            .record(
                true,
                NewHistoryEntry {
                    action: "Grammar fixes".to_string(),
                    source_app: Some("Slack.exe".to_string()),
                    model: "Harper".to_string(),
                    cost_usd: Some(0.0),
                    original: "Teh cat".to_string(),
                    result: "The cat".to_string(),
                },
            )
            .expect("record on");
        assert_eq!(store.entries().len(), 1);

        store.clear().expect("clear");
        assert!(store.entries().is_empty());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn corrupt_history_file_backs_up_and_clear_removes_backup() {
        let dir = temp_dir("corrupt");
        let path = history_path(&dir);
        fs::write(&path, "[[not valid json").expect("write");
        let mut store = HistoryStore::load_from_path(path.clone());
        assert!(store.recovered_from_corrupt);
        assert!(store.entries().is_empty());
        assert!(!path.exists());
        assert!(dir.join("history.bad.json").exists());

        store.clear().expect("clear should purge history.bad.json");
        assert!(!store.recovered_from_corrupt);
        assert!(!dir.join("history.bad.json").exists());

        let _ = fs::remove_dir_all(&dir);
    }
}
