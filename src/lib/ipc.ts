import { invoke } from "@tauri-apps/api/core";

/**
 * Typed wrappers around Tauri commands and events (mirrored from src-tauri/src/dto.rs).
 */
export type ErrorCode =
  | "nothing_selected"
  | "elevated_target"
  | "selection_too_long"
  | "clipboard_busy"
  | "focus_restore_failed"
  | "hotkey_conflict"
  | "secret_store_error"
  | "settings_save_failed"
  | "invalid_model"
  | "ai_no_key"
  | "ai_bad_request"
  | "ai_unauthorized"
  | "ai_payment_required"
  | "ai_forbidden"
  | "ai_no_provider"
  | "ai_rate_limited"
  | "ai_bad_gateway"
  | "ai_network"
  | "ai_mid_stream"
  | "ai_cut_off"
  | "ai_empty"
  | "ai_unexpected";

export interface ApiErrorDto {
  code: ErrorCode | string;
  detail: string | null;
  retryAfterSecs: number | null;
}

export interface AppStatusDto {
  hasApiKey: boolean;
  hotkey: string;
  hotkeyConflict: string | null;
  settingsRecovered: boolean;
}

export interface SettingsDto {
  hotkey: string;
  model: string;
  customModelId: string | null;
  denyDataCollection: boolean;
  historyEnabled: boolean;
  autoRunAction: string | null;
  dialect: string;
}

export interface ModelOptionDto {
  id: string;
  label: string;
  inputCostPerMillion: number;
  outputCostPerMillion: number;
  note: string;
}

export interface KeyTestResultDto {
  label: string | null;
  limitRemaining: number | null;
}

export function isApiError(value: unknown): value is ApiErrorDto {
  return (
    typeof value === "object" &&
    value !== null &&
    "code" in value &&
    typeof (value as { code: unknown }).code === "string"
  );
}

export function getAppStatus(): Promise<AppStatusDto> {
  return invoke<AppStatusDto>("get_app_status");
}

export function getSettings(): Promise<SettingsDto> {
  return invoke<SettingsDto>("get_settings");
}

export function getCuratedModels(): Promise<ModelOptionDto[]> {
  return invoke<ModelOptionDto[]>("get_curated_models");
}

export function saveApiKey(key: string): Promise<boolean> {
  return invoke<boolean>("save_api_key", { key });
}

export function deleteApiKey(): Promise<boolean> {
  return invoke<boolean>("delete_api_key");
}

export function testApiKey(key?: string): Promise<KeyTestResultDto> {
  return invoke<KeyTestResultDto>("test_api_key", {
    key: key && key.trim().length > 0 ? key.trim() : null,
  });
}

export function saveModel(model: string): Promise<SettingsDto> {
  return invoke<SettingsDto>("save_model", { model });
}

export function savePrivacy(denyDataCollection: boolean): Promise<SettingsDto> {
  return invoke<SettingsDto>("save_privacy", { denyDataCollection });
}
