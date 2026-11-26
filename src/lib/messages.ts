import type { ApiErrorDto, KeyTestResultDto } from "./ipc";

/**
 * Centralized user-facing copy.
 * Rust returns structured error codes; all wording lives here so tone stays consistent.
 */
export const MESSAGES = {
  app: {
    name: "Redline",
    tagline: "Lightweight writing assistant running in your system tray.",
  },
  settings: {
    heading: "Settings",
    bareLoopHint: (hotkey: string) =>
      `Select text in any app and press ${hotkey} to fix grammar in place. Close this window to return to the system tray.`,
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
  popup: {
    capturing: "Capturing…",
  },
} as const;

export function formatApiError(err: ApiErrorDto): { headline: string; detail: string | null } {
  switch (err.code) {
    case "ai_no_key":
      return {
        headline: "Add an OpenRouter key to use AI actions. Grammar checks work without one.",
        detail: null,
      };
    case "ai_bad_request":
      return {
        headline: "OpenRouter couldn't run this request.",
        detail: err.detail,
      };
    case "ai_unauthorized":
      return {
        headline: "OpenRouter rejected the key.",
        detail: err.detail,
      };
    case "ai_payment_required":
      return {
        headline: "Out of credits or over this key's limit.",
        detail: err.detail,
      };
    case "ai_forbidden":
      return {
        headline: "OpenRouter blocked this request.",
        detail: err.detail,
      };
    case "ai_no_provider":
      return {
        headline: "No provider for this model meets your privacy settings, or it's unavailable.",
        detail: err.detail,
      };
    case "ai_rate_limited":
      return {
        headline:
          err.retryAfterSecs !== null
            ? `Rate limited. Try again in ${err.retryAfterSecs} s.`
            : "Rate limited. Try again in a moment.",
        detail: err.detail,
      };
    case "ai_bad_gateway":
      return {
        headline: "This model isn't responding right now.",
        detail: err.detail,
      };
    case "ai_network":
      return {
        headline: "Can't reach OpenRouter or it took too long.",
        detail: err.detail,
      };
    case "ai_cut_off":
    case "ai_mid_stream":
      return {
        headline: "The result was cut off.",
        detail: err.detail,
      };
    case "ai_empty":
      return {
        headline: "The model returned nothing.",
        detail: null,
      };
    case "secret_store_error":
      return {
        headline: "Windows Credential Manager returned an error.",
        detail: err.detail,
      };
    case "settings_save_failed":
      return {
        headline: "Couldn't save settings.",
        detail: err.detail,
      };
    default:
      return {
        headline: "Something went wrong.",
        detail: err.detail,
      };
  }
}
