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
  tabs: {
    write: "Write",
    history: "History",
    settings: "Settings",
  },
  a11y: {
    mainSections: "Main sections",
    aiActions: "AI actions",
    chooseTone: "Choose tone",
    selectIssue: (category: string, flagged: string) =>
      `Highlight ${category} suggestion for "${flagged}"`,
  },
  welcome: {
    heading: "Welcome to Redline",
    hotkeySummary: (hotkey: string) =>
      `Select text in any app and press ${hotkey} to check grammar or rewrite in place.`,
    offlineReassurance: "Offline English grammar checking works right now — no API key needed.",
    optionalKeyHeading: "Optional: Connect OpenRouter for AI rewrites",
    startAtLogin: "Start Redline when I sign in to Windows",
    doneButton: "Done",
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
  dialects: [
    { value: "american", label: "American English" },
    { value: "british", label: "British English" },
    { value: "canadian", label: "Canadian English" },
    { value: "australian", label: "Australian English" },
    { value: "indian", label: "Indian English" },
  ],
  autoRunOptions: [
    { value: "", label: "Off (wait for key press)" },
    { value: "improve", label: "Improve" },
    { value: "fix_grammar", label: "Fix grammar" },
    { value: "shorten", label: "Shorten" },
    { value: "make_clearer", label: "Make clearer" },
  ],
  grammar: {
    sectionLabel: "Grammar suggestions",
    checking: "Checking…",
    englishOnly: "Grammar check: English only",
    fixAll: "Fix all",
    undo: "Undo",
    ignore: "Ignore",
    noIssues: "No grammar issues",
    harperHistoryAction: "Grammar fixes",
    harperHistoryModel: "Harper",
    issuesCount: (n: number) => (n === 1 ? "1 suggestion" : `${n} suggestions`),
    moreCount: (n: number) => `+${n} more`,
    dialectLabel: "English dialect",
    sectionTitle: "Grammar",
  },
  write: {
    placeholder: "Type or paste text to check grammar or run AI actions (Ctrl+1–Ctrl+6)…",
    charLimitExceeded: "Over the 40,000-character limit for AI actions.",
    useThis: "Use this",
    clear: "Clear",
    fromApp: (app: string) => `From ${app}`,
  },
  history: {
    empty: "No rewrites or grammar fixes recorded yet.",
    disabledNotice: "History is turned off in Settings. Turn it on to keep the last 20 entries.",
    clearButton: "Clear history",
    copyOriginal: "Copy original",
    copyResult: "Copy result",
    copied: "Copied",
    showDiff: "Show diff",
    hideDiff: "Hide diff",
  },
  popup: {
    capturing: "Capturing…",
    headerFrom: (appLabel: string) => (appLabel ? `Redline · from ${appLabel}` : "Redline"),
    closeLabel: "Close",
    openInEditor: "Open in editor",
    openSettings: "Open Settings",
    tryAnotherModel: "Try another model",
    openCredits: "Open OpenRouter credits",
    paste: "Paste",
    retry: "Retry",
    copy: "Copy",
    copied: "Copied",
    resultTab: "Result",
    diffTab: "Diff",
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
      diff: "D Diff",
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
    generalSectionTitle: "General",
    hotkeyLabel: "Global hotkey",
    hotkeyRecordButton: "Record shortcut",
    hotkeyRecordingPrompt: "Press a shortcut (Esc to cancel)…",
    hotkeyPausedNote: "Hotkey is currently paused from the tray menu.",
    hotkeyConflict: (hotkey: string) => `${hotkey} is used by another app. Pick another shortcut.`,
    autostartLabel: "Start at login",
    historyEnabledLabel: "Save last 20 entries in History",
    autoRunLabel: "Run this action automatically when the popup opens",
    settingsRecovered:
      "Your previous settings file couldn't be read, so defaults were restored (backup saved as settings.bad.json).",
    historyRecovered:
      "Your previous history file couldn't be read, so a fresh list was started (backup saved as history.bad.json).",
    aiSectionTitle: "AI",
    apiKeyLabel: "OpenRouter API key",
    apiKeyPlaceholder: "sk-or-v1-…",
    apiKeySavedBadge: "Saved in Windows Credential Manager",
    apiKeyMissingBadge: "No key saved",
    getKeyButton: "Get a key",
    saveKeyButton: "Save",
    removeKeyButton: "Remove",
    testKeyButton: "Test",
    testingKeyButton: "Testing…",
    keySavedNotice: "Key saved to Windows Credential Manager.",
    keyRemovedNotice: "Key removed from Windows Credential Manager.",
    modelLabel: "Model",
    customModelLabel: "Custom model ID (optional)",
    customModelPlaceholder: "e.g. openai/gpt-6-luna",
    customModelCheckButton: "Verify & use",
    customModelCheckingButton: "Checking…",
    customModelClearButton: "Use curated",
    customModelSavedNotice: (id: string) => `Using custom model ${id}.`,
    privacyLabel: "Don't use providers that train on your text",
    privacyHelp: "Sends data_collection: deny to exclude providers that retain and train on text.",
    aboutSectionTitle: "About",
    versionLabel: (v: string) => `Redline v${v}`,
    licensesText: "MIT License · Offline grammar powered by Harper (Apache-2.0).",
    openLogFolderButton: "Open log folder",
    repoLinkButton: "GitHub repository",
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

export type ErrorNextStep =
  | "settings"
  | "try_another_model"
  | "retry"
  | "retry_or_model"
  | "credits"
  | "none";

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
        nextStep: "try_another_model",
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
        nextStep: "retry_or_model",
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
    case "selection_too_long":
      return {
        headline: MESSAGES.captureErrors.selection_too_long,
        detail: err.detail,
        nextStep: "none",
      };
    case "clipboard_busy":
      return {
        headline: MESSAGES.captureErrors.clipboard_busy,
        detail: err.detail,
        nextStep: "retry",
      };
    case "paste_failed":
      return {
        headline: "Couldn't paste into the target app.",
        detail: err.detail,
        nextStep: "none",
      };
    case "invalid_model":
      return {
        headline: "Choose a valid model ID.",
        detail: err.detail,
        nextStep: "settings",
      };
    case "invalid_action":
      return {
        headline: "Couldn't run that action.",
        detail: err.detail,
        nextStep: "none",
      };
    case "hotkey_conflict":
      return {
        headline: MESSAGES.settings.hotkeyConflict(err.detail ?? "That shortcut"),
        detail: null,
        nextStep: "none",
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
    case "autostart_error":
      return {
        headline: "Couldn't update the Windows startup setting.",
        detail: err.detail,
        nextStep: "none",
      };
    case "history_error":
      return {
        headline: "Couldn't update local history.",
        detail: err.detail,
        nextStep: "none",
      };
    case "opener_error":
      return {
        headline: "Couldn't open that link or folder.",
        detail: err.detail,
        nextStep: "none",
      };
    case "lint_failed":
      return {
        headline: "Grammar check couldn't complete.",
        detail: err.detail,
        nextStep: "retry",
      };
    default:
      return {
        headline: "Something went wrong.",
        detail: err.detail,
        nextStep: "retry",
      };
  }
}
