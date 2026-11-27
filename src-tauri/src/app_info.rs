/// Single source of truth for application names, identifiers, and window labels.
pub const APP_NAME: &str = "Redline";
pub const TRAY_ID: &str = "redline-tray";

pub const WINDOW_MAIN: &str = "main";
pub const WINDOW_POPUP: &str = "popup";
pub const MAIN_ENTRY: &str = "index.html";
pub const POPUP_ENTRY: &str = "popup.html";

pub const POPUP_WIDTH_LOGICAL: f64 = 500.0;
pub const POPUP_COMPACT_HEIGHT_LOGICAL: f64 = 320.0;
pub const POPUP_EXPANDED_HEIGHT_LOGICAL: f64 = 560.0;
pub const POPUP_MIN_WIDTH_LOGICAL: f64 = 420.0;
pub const POPUP_MIN_HEIGHT_LOGICAL: f64 = 280.0;

pub const TRAY_OPEN_ID: &str = "tray-open";
pub const TRAY_PAUSE_ID: &str = "tray-pause";
pub const TRAY_SETTINGS_ID: &str = "tray-settings";
pub const TRAY_QUIT_ID: &str = "tray-quit";

pub const REPO_URL: &str = "https://github.com/ReDzinDev/redline";
pub const OPENROUTER_KEYS_URL: &str = "https://openrouter.ai/keys";
pub const OPENROUTER_CREDITS_URL: &str = "https://openrouter.ai/settings/credits";

pub const KEYRING_SERVICE: &str = "dev.redline.app";
pub const KEYRING_OPENROUTER_USER: &str = "openrouter_api_key";

pub const DEFAULT_HOTKEY: &str = "Ctrl+Alt+G";
pub const DEFAULT_MODEL: &str = "anthropic/claude-haiku-5.5";

pub const POPUP_MAX_INPUT_CHARS: usize = 12_000;
pub const EDITOR_MAX_INPUT_CHARS: usize = 40_000;
pub const CUSTOM_INSTRUCTION_MAX_CHARS: usize = 1_000;

pub const DEV_HOST: &str = "localhost";
pub const DEV_PORT: u16 = 1420;

/// Sanitizes an external or user-configurable metadata string before logging or surfacing it:
/// removes C0/C1 control characters (including CR/LF/ESC), BiDi controls, and caps char length.
pub fn sanitize_log_value(raw: &str, max_chars: usize) -> String {
    raw.chars()
        .filter(|&ch| {
            !ch.is_control()
                && !matches!(
                    ch,
                    '\u{061C}' | '\u{200E}' | '\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}'
                )
        })
        .take(max_chars)
        .collect::<String>()
        .trim()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_log_value_strips_controls_bidi_and_truncates() {
        let dirty = "openai/gpt\r\nFAKE LOG\x1b[31m\u{202e}evil";
        assert_eq!(sanitize_log_value(dirty, 128), "openai/gptFAKE LOG[31mevil");
        assert_eq!(sanitize_log_value("abcdefghij", 4), "abcd");
    }
}
