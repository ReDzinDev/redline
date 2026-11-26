/**
 * Typed wrappers around Tauri commands and events (mirrored from src-tauri/src/dto.rs).
 * Populated as commands are added starting in Milestone 1.
 */
export type ErrorCode =
  | "nothing_selected"
  | "elevated_target"
  | "selection_too_long"
  | "clipboard_busy"
  | "focus_restore_failed"
  | "hotkey_conflict"
  | "secret_store_error";
