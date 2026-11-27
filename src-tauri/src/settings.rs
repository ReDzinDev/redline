use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::ai::provider::ReasoningMode;
use crate::app_info::{CUSTOM_INSTRUCTION_MAX_CHARS, DEFAULT_HOTKEY, DEFAULT_MODEL};

pub const SETTINGS_VERSION: u32 = 1;
pub const MAX_RECENT_CUSTOM_INSTRUCTIONS: usize = 5;
const MAX_SETTINGS_FILE_BYTES: u64 = 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub version: u32,
    pub hotkey: String,
    pub model: String,
    pub custom_model_id: Option<String>,
    pub custom_model_reasoning: Option<ReasoningMode>,
    pub deny_data_collection: bool,
    pub history_enabled: bool,
    pub auto_run_action: Option<String>,
    pub dialect: String,
    pub recent_custom_instructions: Vec<String>,
    pub welcome_completed: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            version: SETTINGS_VERSION,
            hotkey: DEFAULT_HOTKEY.to_string(),
            model: DEFAULT_MODEL.to_string(),
            custom_model_id: None,
            custom_model_reasoning: None,
            deny_data_collection: true,
            history_enabled: true,
            auto_run_action: None,
            dialect: "american".to_string(),
            recent_custom_instructions: Vec::new(),
            welcome_completed: false,
        }
    }
}

impl Settings {
    /// Returns the active model ID (custom ID if set and non-empty, otherwise the selected model).
    pub fn active_model_id(&self) -> &str {
        self.custom_model_id
            .as_deref()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or(&self.model)
    }

    /// Remembers the last 5 distinct custom instructions (newest first), capped in length.
    pub fn push_recent_custom_instruction(&mut self, instruction: &str) {
        let trimmed: String = instruction
            .trim()
            .chars()
            .take(CUSTOM_INSTRUCTION_MAX_CHARS)
            .collect();
        if trimmed.is_empty() {
            return;
        }
        self.recent_custom_instructions
            .retain(|existing| !existing.eq_ignore_ascii_case(&trimmed));
        self.recent_custom_instructions.insert(0, trimmed);
        self.recent_custom_instructions
            .truncate(MAX_RECENT_CUSTOM_INSTRUCTIONS);
    }
}

#[derive(Debug)]
pub struct LoadedSettings {
    pub settings: Settings,
    /// True if an unreadable or corrupt settings file was renamed to `settings.bad.json`.
    pub recovered_from_corrupt: bool,
}

pub fn settings_path(config_dir: &Path) -> PathBuf {
    config_dir.join("settings.json")
}

/// Loads settings from `path`. Never crashes on corrupt JSON: renames the broken file
/// to `settings.bad.json` and returns defaults with `recovered_from_corrupt = true`.
pub fn load_from_path(path: &Path) -> LoadedSettings {
    if !path.exists() {
        return LoadedSettings {
            settings: Settings::default(),
            recovered_from_corrupt: false,
        };
    }

    if let Ok(meta) = fs::metadata(path)
        && meta.len() > MAX_SETTINGS_FILE_BYTES
    {
        log::warn!(
            "settings file exceeds size cap ({} bytes); backing up and loading defaults",
            meta.len()
        );
        backup_corrupt_file(path);
        return LoadedSettings {
            settings: Settings::default(),
            recovered_from_corrupt: true,
        };
    }

    match fs::read_to_string(path) {
        Ok(raw) => match serde_json::from_str::<Settings>(&raw) {
            Ok(mut parsed) => {
                if parsed.version < SETTINGS_VERSION {
                    parsed.version = SETTINGS_VERSION;
                }
                if parsed.hotkey.trim().is_empty() {
                    parsed.hotkey = DEFAULT_HOTKEY.to_string();
                }
                if parsed.model.trim().is_empty() {
                    parsed.model = DEFAULT_MODEL.to_string();
                }
                parsed
                    .recent_custom_instructions
                    .truncate(MAX_RECENT_CUSTOM_INSTRUCTIONS);
                for item in &mut parsed.recent_custom_instructions {
                    *item = item.chars().take(CUSTOM_INSTRUCTION_MAX_CHARS).collect();
                }
                LoadedSettings {
                    settings: parsed,
                    recovered_from_corrupt: false,
                }
            }
            Err(err) => {
                log::warn!(
                    "settings JSON invalid (kind={:?} line={} col={}); backing up and loading defaults",
                    err.classify(),
                    err.line(),
                    err.column()
                );
                backup_corrupt_file(path);
                LoadedSettings {
                    settings: Settings::default(),
                    recovered_from_corrupt: true,
                }
            }
        },
        Err(err) => {
            log::warn!(
                "failed reading settings file ({}); loading defaults",
                err.kind()
            );
            backup_corrupt_file(path);
            LoadedSettings {
                settings: Settings::default(),
                recovered_from_corrupt: true,
            }
        }
    }
}

pub fn save_to_path(path: &Path, settings: &Settings) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let serialized = serde_json::to_string_pretty(settings)
        .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidData, err))?;
    let tmp_path = path.with_extension("json.tmp");
    {
        let mut file = File::create(&tmp_path)?;
        file.write_all(serialized.as_bytes())?;
        file.sync_all()?;
    }
    fs::rename(&tmp_path, path)?;
    Ok(())
}

fn backup_corrupt_file(path: &Path) {
    let bad_path = path.with_file_name("settings.bad.json");
    if let Err(err) = fs::rename(path, &bad_path) {
        log::warn!("could not rename corrupt settings file: {}", err.kind());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir(label: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("redline_settings_{label}_{nanos}"));
        fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    #[test]
    fn missing_file_returns_defaults() {
        let dir = temp_dir("missing");
        let loaded = load_from_path(&settings_path(&dir));
        assert!(!loaded.recovered_from_corrupt);
        assert_eq!(loaded.settings, Settings::default());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn save_and_load_roundtrip() {
        let dir = temp_dir("roundtrip");
        let path = settings_path(&dir);
        let original = Settings {
            model: "openai/gpt-6-luna".to_string(),
            deny_data_collection: false,
            dialect: "british".to_string(),
            ..Settings::default()
        };
        save_to_path(&path, &original).expect("save settings");
        let loaded = load_from_path(&path);
        assert!(!loaded.recovered_from_corrupt);
        assert_eq!(loaded.settings, original);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn migrates_older_version_and_fills_missing_fields() {
        let dir = temp_dir("migrate");
        let path = settings_path(&dir);
        fs::write(&path, r#"{"version": 0, "model": "qwen/qwen3.7-flash"}"#).expect("write");
        let loaded = load_from_path(&path);
        assert!(!loaded.recovered_from_corrupt);
        assert_eq!(loaded.settings.version, SETTINGS_VERSION);
        assert_eq!(loaded.settings.model, "qwen/qwen3.7-flash");
        assert_eq!(loaded.settings.hotkey, DEFAULT_HOTKEY);
        assert!(loaded.settings.deny_data_collection);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn corrupt_json_renames_to_bad_and_falls_back() {
        let dir = temp_dir("corrupt");
        let path = settings_path(&dir);
        fs::write(&path, "{not valid json!!!").expect("write");
        let loaded = load_from_path(&path);
        assert!(loaded.recovered_from_corrupt);
        assert_eq!(loaded.settings, Settings::default());
        assert!(!path.exists());
        assert!(dir.join("settings.bad.json").exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn remembers_last_five_custom_instructions_deduplicated() {
        let mut s = Settings::default();
        for i in 1..=7 {
            s.push_recent_custom_instruction(&format!("Prompt {i}"));
        }
        assert_eq!(
            s.recent_custom_instructions,
            vec!["Prompt 7", "Prompt 6", "Prompt 5", "Prompt 4", "Prompt 3"]
        );
        s.push_recent_custom_instruction("prompt 4");
        assert_eq!(
            s.recent_custom_instructions,
            vec!["prompt 4", "Prompt 7", "Prompt 6", "Prompt 5", "Prompt 3"]
        );
    }
}
