import { Channel, invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { LintResponseDto } from "./grammarLogic";

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
  | "invalid_action"
  | "autostart_error"
  | "opener_error"
  | "history_error"
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
  hotkeyPaused: boolean;
  settingsRecovered: boolean;
  historyRecovered: boolean;
  welcomeCompleted: boolean;
  autostartEnabled: boolean;
  appVersion: string;
}

export interface SettingsDto {
  hotkey: string;
  model: string;
  customModelId: string | null;
  denyDataCollection: boolean;
  historyEnabled: boolean;
  autoRunAction: string | null;
  dialect: string;
  recentCustomInstructions: string[];
  welcomeCompleted: boolean;
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

export interface HistoryEntryDto {
  id: string;
  timestampMs: number;
  action: string;
  sourceApp: string | null;
  model: string;
  costUsd: number | null;
  original: string;
  result: string;
}

export interface AcceptHistoryMetaDto {
  action: string;
  model: string;
  costUsd: number | null;
  original: string;
}

export type PopupSessionEvent =
  | {
      phase: "capturing";
      sessionId: number;
      sourceApp: string;
    }
  | {
      phase: "ready";
      sessionId: number;
      sourceApp: string;
      text: string;
      recentCustomInstructions: string[];
      autoRunAction: string | null;
    }
  | {
      phase: "error";
      sessionId: number;
      sourceApp: string;
      errorCode: string;
    };

export type ActionKind =
  | "improve"
  | "fix_grammar"
  | "shorten"
  | "make_clearer"
  | "change_tone"
  | "custom";

export type ToneOption = "professional" | "friendly" | "casual" | "confident";

export interface ActionRequestDto {
  kind: ActionKind;
  tone?: ToneOption;
  customInstruction?: string;
}

export type RewriteStreamEvent =
  | { event: "delta"; requestId: number; text: string }
  | {
      event: "complete";
      requestId: number;
      text: string;
      modelUsed: string;
      costUsd: number | null;
      elapsedMs: number;
      unchanged: boolean;
    }
  | {
      event: "cutOff";
      requestId: number;
      partialText: string;
      detail: string | null;
    }
  | {
      event: "error";
      requestId: number;
      error: ApiErrorDto;
    };

export interface AcceptResultDto {
  outcome: "pasted" | "copied_fallback";
}

export interface PendingEditorDraft {
  text: string;
  sourceApp: string | null;
}

export type ExternalUrlTarget = "repo" | "openrouter_keys" | "openrouter_credits";
export type MainTab = "write" | "history" | "settings";

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

export function saveCustomModel(customModelId: string | null): Promise<SettingsDto> {
  return invoke<SettingsDto>("save_custom_model", { customModelId });
}

export function savePrivacy(denyDataCollection: boolean): Promise<SettingsDto> {
  return invoke<SettingsDto>("save_privacy", { denyDataCollection });
}

export function saveDialect(dialect: string): Promise<SettingsDto> {
  return invoke<SettingsDto>("save_dialect", { dialect });
}

export function saveHotkey(hotkey: string): Promise<SettingsDto> {
  return invoke<SettingsDto>("save_hotkey", { hotkey });
}

export function saveHistoryEnabled(enabled: boolean): Promise<SettingsDto> {
  return invoke<SettingsDto>("save_history_enabled", { enabled });
}

export function saveAutoRunAction(action: string | null): Promise<SettingsDto> {
  return invoke<SettingsDto>("save_auto_run_action", { action });
}

export function setAutostartEnabled(enabled: boolean): Promise<boolean> {
  return invoke<boolean>("set_autostart_enabled", { enabled });
}

export function completeWelcome(): Promise<SettingsDto> {
  return invoke<SettingsDto>("complete_welcome");
}

export function getHistory(): Promise<HistoryEntryDto[]> {
  return invoke<HistoryEntryDto[]>("get_history");
}

export function clearHistory(): Promise<void> {
  return invoke<void>("clear_history");
}

export function recordHistoryEntry(entry: {
  action: string;
  sourceApp: string | null;
  model: string;
  costUsd: number | null;
  original: string;
  result: string;
}): Promise<void> {
  return invoke<void>("record_history_entry", { entry });
}

export function openExternalUrl(target: ExternalUrlTarget): Promise<void> {
  return invoke<void>("open_external_url", { target });
}

export function openLogFolder(): Promise<void> {
  return invoke<void>("open_log_folder");
}

export function lintText(text: string, requestId: number): Promise<LintResponseDto> {
  return invoke<LintResponseDto>("lint_text", { text, requestId });
}

export function rewriteText(
  requestId: number,
  text: string,
  action: ActionRequestDto,
  onEvent: (event: RewriteStreamEvent) => void,
): Promise<void> {
  const channel = new Channel<RewriteStreamEvent>();
  channel.onmessage = onEvent;
  return invoke<void>("rewrite_text", {
    requestId,
    text,
    action,
    onEvent: channel,
  });
}

export function cancelRewrite(): Promise<void> {
  return invoke<void>("cancel_rewrite");
}

export function resizePopup(expanded: boolean): Promise<void> {
  return invoke<void>("resize_popup", { expanded });
}

export function acceptPopupResult(
  sessionId: number,
  text: string,
  historyMeta?: AcceptHistoryMetaDto,
): Promise<AcceptResultDto> {
  return invoke<AcceptResultDto>("accept_popup_result", {
    sessionId,
    text,
    historyMeta: historyMeta ?? null,
  });
}

export function dismissPopup(restoreFocus: boolean): Promise<void> {
  return invoke<void>("dismiss_popup", { restoreFocus });
}

export function copyToClipboard(text: string): Promise<void> {
  return invoke<void>("copy_to_clipboard", { text });
}

export function openInEditor(text?: string): Promise<void> {
  return invoke<void>("open_in_editor", { text: text ?? null });
}

export function openSettings(): Promise<void> {
  return invoke<void>("open_settings");
}

export function takeEditorDraft(): Promise<PendingEditorDraft | null> {
  return invoke<PendingEditorDraft | null>("take_editor_draft");
}

export function listenPopupSession(
  handler: (event: PopupSessionEvent) => void,
): Promise<UnlistenFn> {
  return listen<PopupSessionEvent>("popup:session", (evt) => handler(evt.payload));
}

export function listenMainNavigate(handler: (tab: MainTab) => void): Promise<UnlistenFn> {
  return listen<string>("main:navigate", (evt) => {
    if (evt.payload === "write" || evt.payload === "history" || evt.payload === "settings") {
      handler(evt.payload);
    }
  });
}

export function listenEditorLoadDraft(
  handler: (draft: PendingEditorDraft) => void,
): Promise<UnlistenFn> {
  return listen<PendingEditorDraft>("editor:load-draft", (evt) => handler(evt.payload));
}
