/// Single source of truth for application names, identifiers, and window labels.
pub const APP_NAME: &str = "Redline";
pub const TRAY_ID: &str = "redline-tray";

pub const WINDOW_MAIN: &str = "main";
pub const WINDOW_POPUP: &str = "popup";
pub const MAIN_ENTRY: &str = "index.html";
pub const POPUP_ENTRY: &str = "popup.html";

pub const POPUP_WIDTH_LOGICAL: f64 = 460.0;
pub const POPUP_COMPACT_HEIGHT_LOGICAL: f64 = 320.0;
pub const POPUP_EXPANDED_HEIGHT_LOGICAL: f64 = 520.0;

pub const TRAY_OPEN_ID: &str = "tray-open";
pub const TRAY_SETTINGS_ID: &str = "tray-settings";
pub const TRAY_QUIT_ID: &str = "tray-quit";

pub const KEYRING_SERVICE: &str = "dev.redline.app";
pub const KEYRING_OPENROUTER_USER: &str = "openrouter_api_key";

pub const DEFAULT_HOTKEY: &str = "Ctrl+Alt+G";
pub const DEFAULT_MODEL: &str = "anthropic/claude-haiku-5.5";

pub const POPUP_MAX_INPUT_CHARS: usize = 12_000;
pub const EDITOR_MAX_INPUT_CHARS: usize = 40_000;

pub const DEV_HOST: &str = "localhost";
pub const DEV_PORT: u16 = 1420;
