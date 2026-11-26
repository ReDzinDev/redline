import type { ActionKind, ApiErrorDto, KeyTestResultDto, ToneOption } from "./ipc";

/**
 * Centralized user-facing copy.
 * Rust returns structured error codes; all wording lives here so tone stays consistent.
 */
export const MESSAGES = {
  app: {
    name: "Redline",
    tagline: "Lightweight writing assistant running in your system tray.",
  },
  actions: [
    { key: "1", kind: "improve" as ActionKind, label: "Improve" },
    { key: "2", kind: "fix_grammar" as ActionKind, label: "Fix grammar" },
    { key: "3", kind: "shorten" as ActionKind, label: "Shorten" },
    { key: "4", kind: "make_clearer" as ActionKind, label: "Clearer" },
    { key: "5", kind: "change_tone" as ActionKind, label: "Tone" },
    { key: "6", kind: "custom" as ActionKind, label: "Custom" },
  ],
  tones: [
    { key: "1", tone: "professional" as ToneOption, label: "Professional" },
    { key: "2", tone: "friendly" as ToneOption, label: "Friendly" },
    { key: "3", tone: "casual" as ToneOption, label: "Casual" },
    { key: "4", tone: "confident" as ToneOption, label: "Confident" },
  ],
  popup: {
    capturing: "Capturing…",
    headerFrom: (appLabel: string) => (appLabel ? `Redline · from ${appLabel}` : "Redline"),
    closeLabel: "Close",
    openInEditor: "Open in editor",
    openSettings: "Open Settings",
    retry: "Retry",
    copy: "Copy",
    copied: "Copied",
    copiedFallbackNotice: "Copied: paste it with Ctrl+V.",
    noChanges: "No changes",
    resultReadyAnnouncement: "Result ready",
    streamingLabel: "Streaming…",
    cutOffHeadline: "The result was cut off.",
    customPlaceholder: "Describe how to edit this text…",
    customSubmit: "Run",
    recentCustomLabel: "Recent",
    hints: {
      paste: "↵ Paste",
      close: "↵ Close",
      streaming: "Wait for the result or press Esc",
      cutOffOrError: "Retry or Copy · Esc Close",
      editor: "Ctrl+E Editor",
      esc: "Esc Close",
    },
  },
  captureErrors: {
    nothing_selected: "Select some text first.",
    elevated_target:
      "Redline can't read from apps running as administrator. Use the editor instead.",
    selection_too_long: "Too long for the popup.",
    clipboard_busy: "The clipboard is busy. Try again.",
    default: "Couldn't capture the selected text.",
  },
  settings: {
    heading: "Settings",
    bareLoopHint: (hotkey: string) =>
      `Select text in any app and press ${hotkey} to open the inline popup. Close this window to return to the system tray.`,
    hotkeyConflict: (hotkey: string) =>
      `${hotkey} is used by another app. Pick another shortcut or close the conflicting app.`,
    settingsRecovered:
      "Your previous settings file couldn't be read, so defaults were restored (backup saved as settings.bad.json).",
    aiSectionTitle: "AI (OpenRouter)",
    apiKeyLabel: "OpenRouter API key",
    apiKeyPlaceholder: "sk-or-v1-…",
    apiKeySavedBadge: "Saved in Windows Credential Manager",
    apiKeyMissingBadge: "No key saved",
    saveKeyButton: "Save",
    removeKeyButton: "Remove",
    testKeyButton: "Test",
    testingKeyButton: "Testing…",
    keySavedNotice: "Key saved to Windows Credential Manager.",
    keyRemovedNotice: "Key removed from Windows Credential Manager.",
    modelLabel: "Model",
    privacyLabel: "Don't use providers that train on your text",
    privacyHelp: "Sends data_collection: deny to exclude providers that retain and train on text.",
    formatKeyTestSuccess: (res: KeyTestResultDto): string => {
      const name = res.label ? `${res.label} · ` : "";
      const limit =
        res.limitRemaining === null
          ? "no limit on this key"
          : `key limit remaining: $${res.limitRemaining.toFixed(4)}`;
      return `Key valid — ${name}${limit}`;
    },
  },
} as const;

export function formatCaptureError(code: string): string {
  switch (code) {
    case "nothing_selected":
      return MESSAGES.captureErrors.nothing_selected;
    case "elevated_target":
      return MESSAGES.captureErrors.elevated_target;
    case "selection_too_long":
      return MESSAGES.captureErrors.selection_too_long;
    case "clipboard_busy":
      return MESSAGES.captureErrors.clipboard_busy;
    default:
      return MESSAGES.captureErrors.default;
  }
}

export type ErrorNextStep = "settings" | "retry" | "credits" | "none";

export function formatApiError(err: ApiErrorDto): {
  headline: string;
  detail: string | null;
  nextStep: ErrorNextStep;
} {
  switch (err.code) {
    case "ai_no_key":
      return {
        headline: "Add an OpenRouter key to use AI actions. Grammar checks work without one.",
        detail: null,
        nextStep: "settings",
      };
    case "ai_bad_request":
      return {
        headline: "OpenRouter couldn't run this request.",
        detail: err.detail,
        nextStep: "settings",
      };
    case "ai_unauthorized":
      return {
        headline: "OpenRouter rejected the key.",
        detail: err.detail,
        nextStep: "settings",
      };
    case "ai_payment_required":
      return {
        headline: "Out of credits or over this key's limit.",
        detail: err.detail,
        nextStep: "credits",
      };
    case "ai_forbidden":
      return {
        headline: "OpenRouter blocked this request.",
        detail: err.detail,
        nextStep: "settings",
      };
    case "ai_no_provider":
      return {
        headline: "No provider for this model meets your privacy settings, or it's unavailable.",
        detail: err.detail,
        nextStep: "settings",
      };
    case "ai_rate_limited":
      return {
        headline:
          err.retryAfterSecs !== null
            ? `Rate limited. Try again in ${err.retryAfterSecs} s.`
            : "Rate limited. Try again in a moment.",
        detail: err.detail,
        nextStep: "retry",
      };
    case "ai_bad_gateway":
      return {
        headline: "This model isn't responding right now.",
        detail: err.detail,
        nextStep: "retry",
      };
    case "ai_network":
      return {
        headline: "Can't reach OpenRouter or it took too long.",
        detail: err.detail,
        nextStep: "retry",
      };
    case "ai_cut_off":
    case "ai_mid_stream":
      return {
        headline: "The result was cut off.",
        detail: err.detail,
        nextStep: "retry",
      };
    case "ai_empty":
      return {
        headline: "The model returned nothing.",
        detail: null,
        nextStep: "retry",
      };
    case "secret_store_error":
      return {
        headline: "Windows Credential Manager returned an error.",
        detail: err.detail,
        nextStep: "settings",
      };
    case "settings_save_failed":
      return {
        headline: "Couldn't save settings.",
        detail: err.detail,
        nextStep: "none",
      };
    default:
      return {
        headline: "Something went wrong.",
        detail: err.detail,
        nextStep: "retry",
      };
  }
}
