import { useEffect, useState } from "react";
import {
  deleteApiKey,
  isApiError,
  openExternalUrl,
  openLogFolder,
  saveApiKey,
  saveAutoRunAction,
  saveCustomModel,
  saveDialect,
  saveHistoryEnabled,
  saveHotkey,
  saveModel,
  savePrivacy,
  setAutostartEnabled,
  testApiKey,
  type AppStatusDto,
  type ModelOptionDto,
  type SettingsDto,
} from "../lib/ipc";
import { formatApiError, MESSAGES } from "../lib/messages";

type Feedback =
  | { kind: "info"; headline: string; detail?: string | null }
  | { kind: "error"; headline: string; detail?: string | null };

function errorToFeedback(err: unknown): Feedback {
  if (isApiError(err)) {
    const formatted = formatApiError(err);
    return { kind: "error", headline: formatted.headline, detail: formatted.detail };
  }
  return {
    kind: "error",
    headline: "Something went wrong.",
    detail: err instanceof Error ? err.message : String(err),
  };
}

function formatRecordedShortcut(e: KeyboardEvent): string | null {
  const modifiers: string[] = [];
  if (e.ctrlKey) modifiers.push("Ctrl");
  if (e.altKey) modifiers.push("Alt");
  if (e.shiftKey) modifiers.push("Shift");
  if (e.metaKey) modifiers.push("Super");

  // Ignore bare modifier presses.
  if (["Control", "Alt", "Shift", "Meta"].includes(e.key)) {
    return null;
  }
  if (modifiers.length === 0) {
    return null;
  }

  const key =
    e.key.length === 1
      ? e.key.toUpperCase()
      : e.key === " "
        ? "Space"
        : e.key.replace(/^Arrow/, "");
  return [...modifiers, key].join("+");
}

interface SettingsTabProps {
  status: AppStatusDto;
  settings: SettingsDto;
  models: ModelOptionDto[];
  onStatusChange: (next: AppStatusDto) => void;
  onSettingsChange: (next: SettingsDto) => void;
}

export function SettingsTab({
  status,
  settings,
  models,
  onStatusChange,
  onSettingsChange,
}: SettingsTabProps) {
  const [keyInput, setKeyInput] = useState("");
  const [customModelInput, setCustomModelInput] = useState(settings.customModelId ?? "");
  const [recordingHotkey, setRecordingHotkey] = useState(false);
  const [busy, setBusy] = useState(false);
  const [testing, setTesting] = useState(false);
  const [checkingModel, setCheckingModel] = useState(false);
  const [feedback, setFeedback] = useState<Feedback | null>(null);
  const [generalFeedback, setGeneralFeedback] = useState<Feedback | null>(null);

  useEffect(() => {
    if (!recordingHotkey) return;
    function onKeyDown(e: KeyboardEvent) {
      e.preventDefault();
      e.stopPropagation();
      if (e.key === "Escape") {
        setRecordingHotkey(false);
        return;
      }
      const combo = formatRecordedShortcut(e);
      if (!combo) return;
      setRecordingHotkey(false);
      setGeneralFeedback(null);
      saveHotkey(combo)
        .then((updated) => {
          onSettingsChange(updated);
          onStatusChange({ ...status, hotkey: updated.hotkey, hotkeyConflict: null });
        })
        .catch((err: unknown) => {
          setGeneralFeedback(errorToFeedback(err));
        });
    }
    window.addEventListener("keydown", onKeyDown, true);
    return () => window.removeEventListener("keydown", onKeyDown, true);
  }, [onSettingsChange, onStatusChange, recordingHotkey, status]);

  async function handleSaveKey() {
    setBusy(true);
    setFeedback(null);
    try {
      const saved = await saveApiKey(keyInput);
      setKeyInput("");
      onStatusChange({ ...status, hasApiKey: saved });
      setFeedback({ kind: "info", headline: MESSAGES.settings.keySavedNotice });
    } catch (err: unknown) {
      setFeedback(errorToFeedback(err));
    } finally {
      setBusy(false);
    }
  }

  async function handleRemoveKey() {
    setBusy(true);
    setFeedback(null);
    try {
      const saved = await deleteApiKey();
      setKeyInput("");
      onStatusChange({ ...status, hasApiKey: saved });
      setFeedback({ kind: "info", headline: MESSAGES.settings.keyRemovedNotice });
    } catch (err: unknown) {
      setFeedback(errorToFeedback(err));
    } finally {
      setBusy(false);
    }
  }

  async function handleTestKey() {
    setTesting(true);
    setFeedback(null);
    try {
      const res = await testApiKey(keyInput);
      setFeedback({ kind: "info", headline: MESSAGES.settings.formatKeyTestSuccess(res) });
    } catch (err: unknown) {
      setFeedback(errorToFeedback(err));
    } finally {
      setTesting(false);
    }
  }

  async function handleCheckCustomModel() {
    const trimmed = customModelInput.trim();
    setCheckingModel(true);
    setFeedback(null);
    try {
      const updated = await saveCustomModel(trimmed.length > 0 ? trimmed : null);
      onSettingsChange(updated);
      if (updated.customModelId) {
        setFeedback({
          kind: "info",
          headline: MESSAGES.settings.customModelSavedNotice(updated.customModelId),
        });
      }
    } catch (err: unknown) {
      setFeedback(errorToFeedback(err));
    } finally {
      setCheckingModel(false);
    }
  }

  return (
    <div className="min-h-0 flex-1 space-y-4 overflow-y-auto pr-1">
      {status.hotkeyConflict && (
        <div
          role="alert"
          className="rounded-md border border-status-warning bg-elevated px-4 py-3 text-xs text-ink"
        >
          {MESSAGES.settings.hotkeyConflict(status.hotkeyConflict)}
        </div>
      )}

      {status.settingsRecovered && (
        <div
          role="status"
          className="rounded-md border border-line-strong bg-elevated px-4 py-3 text-xs text-ink-secondary"
        >
          {MESSAGES.settings.settingsRecovered}
        </div>
      )}

      {status.historyRecovered && (
        <div
          role="status"
          className="rounded-md border border-line-strong bg-elevated px-4 py-3 text-xs text-ink-secondary"
        >
          {MESSAGES.settings.historyRecovered}
        </div>
      )}

      {/* 1. General */}
      <section className="space-y-3 rounded-lg border border-line bg-elevated p-4">
        <h2 className="text-sm font-semibold text-ink">{MESSAGES.settings.generalSectionTitle}</h2>

        <div className="flex flex-wrap items-center justify-between gap-2">
          <div>
            <p className="text-xs font-medium text-ink">{MESSAGES.settings.hotkeyLabel}</p>
            {status.hotkeyPaused && (
              <p className="text-[11px] text-status-warning">
                {MESSAGES.settings.hotkeyPausedNote}
              </p>
            )}
          </div>
          <div className="flex items-center gap-2">
            <kbd className="rounded-sm border border-line-strong bg-canvas px-2 py-1 font-mono text-xs text-ink">
              {recordingHotkey ? MESSAGES.settings.hotkeyRecordingPrompt : settings.hotkey}
            </kbd>
            <button
              type="button"
              onClick={() => setRecordingHotkey((prev) => !prev)}
              className="rounded-md border border-line-strong bg-canvas px-2.5 py-1 text-xs font-medium text-ink hover:bg-hover focus-visible:ring-2 focus-visible:ring-accent"
            >
              {MESSAGES.settings.hotkeyRecordButton}
            </button>
          </div>
        </div>

        {generalFeedback && (
          <p
            role={generalFeedback.kind === "error" ? "alert" : "status"}
            className={`text-xs ${
              generalFeedback.kind === "error" ? "text-status-error" : "text-ink-secondary"
            }`}
          >
            {generalFeedback.headline}
          </p>
        )}

        <label className="flex items-center gap-2.5 text-xs text-ink">
          <input
            type="checkbox"
            checked={status.autostartEnabled}
            onChange={(e) => {
              const next = e.target.checked;
              setAutostartEnabled(next)
                .then((actual) => onStatusChange({ ...status, autostartEnabled: actual }))
                .catch((err: unknown) => setGeneralFeedback(errorToFeedback(err)));
            }}
            className="accent-accent"
          />
          <span>{MESSAGES.settings.autostartLabel}</span>
        </label>

        <label className="flex items-center gap-2.5 text-xs text-ink">
          <input
            type="checkbox"
            checked={settings.historyEnabled}
            onChange={(e) => {
              saveHistoryEnabled(e.target.checked)
                .then(onSettingsChange)
                .catch((err: unknown) => setGeneralFeedback(errorToFeedback(err)));
            }}
            className="accent-accent"
          />
          <span>{MESSAGES.settings.historyEnabledLabel}</span>
        </label>

        <div className="space-y-1">
          <label htmlFor="auto-run-select" className="block text-xs font-medium text-ink-secondary">
            {MESSAGES.settings.autoRunLabel}
          </label>
          <select
            id="auto-run-select"
            value={settings.autoRunAction ?? ""}
            onChange={(e) => {
              const val = e.target.value;
              saveAutoRunAction(val.length > 0 ? val : null)
                .then(onSettingsChange)
                .catch((err: unknown) => setGeneralFeedback(errorToFeedback(err)));
            }}
            className="w-full rounded-md border border-line-strong bg-canvas px-3 py-1.5 text-xs text-ink outline-none focus-visible:ring-2 focus-visible:ring-accent"
          >
            {MESSAGES.autoRunOptions.map((opt) => (
              <option key={opt.value} value={opt.value}>
                {opt.label}
              </option>
            ))}
          </select>
        </div>
      </section>

      {/* 2. AI */}
      <section className="space-y-3 rounded-lg border border-line bg-elevated p-4">
        <div className="flex items-center justify-between gap-2">
          <h2 className="text-sm font-semibold text-ink">{MESSAGES.settings.aiSectionTitle}</h2>
          <div className="flex items-center gap-3">
            <span className="text-xs text-ink-secondary">
              {status.hasApiKey
                ? MESSAGES.settings.apiKeySavedBadge
                : MESSAGES.settings.apiKeyMissingBadge}
            </span>
            <button
              type="button"
              onClick={() => openExternalUrl("openrouter_keys").catch(() => {})}
              className="text-xs font-medium text-accent underline underline-offset-2 focus-visible:ring-2 focus-visible:ring-accent"
            >
              {MESSAGES.settings.getKeyButton}
            </button>
          </div>
        </div>

        <div className="space-y-2">
          <label htmlFor="openrouter-key" className="block text-xs font-medium text-ink-secondary">
            {MESSAGES.settings.apiKeyLabel}
          </label>
          <div className="flex flex-wrap items-center gap-2">
            <input
              id="openrouter-key"
              type="password"
              autoComplete="off"
              spellCheck={false}
              value={keyInput}
              onChange={(e) => setKeyInput(e.target.value)}
              placeholder={MESSAGES.settings.apiKeyPlaceholder}
              className="min-w-0 flex-1 rounded-md border border-line-strong bg-canvas px-3 py-1.5 text-xs text-ink outline-none focus-visible:ring-2 focus-visible:ring-accent"
            />
            <button
              type="button"
              disabled={busy || testing || keyInput.trim().length === 0}
              onClick={handleSaveKey}
              className="rounded-md bg-accent px-3 py-1.5 text-xs font-medium text-accent-contrast transition hover:bg-accent-hover focus-visible:ring-2 focus-visible:ring-accent disabled:opacity-50"
            >
              {MESSAGES.settings.saveKeyButton}
            </button>
            <button
              type="button"
              disabled={busy || testing || (!status.hasApiKey && keyInput.trim().length === 0)}
              onClick={handleTestKey}
              className="rounded-md border border-line-strong bg-canvas px-3 py-1.5 text-xs font-medium text-ink transition hover:bg-hover focus-visible:ring-2 focus-visible:ring-accent disabled:opacity-50"
            >
              {testing ? MESSAGES.settings.testingKeyButton : MESSAGES.settings.testKeyButton}
            </button>
            {status.hasApiKey && (
              <button
                type="button"
                disabled={busy || testing}
                onClick={handleRemoveKey}
                className="rounded-md border border-line-strong bg-canvas px-3 py-1.5 text-xs font-medium text-ink-secondary transition hover:bg-hover focus-visible:ring-2 focus-visible:ring-accent disabled:opacity-50"
              >
                {MESSAGES.settings.removeKeyButton}
              </button>
            )}
          </div>
        </div>

        <div className="space-y-1">
          <label htmlFor="model-select" className="block text-xs font-medium text-ink-secondary">
            {MESSAGES.settings.modelLabel}
          </label>
          <select
            id="model-select"
            value={settings.model}
            onChange={(e) => {
              setFeedback(null);
              saveModel(e.target.value)
                .then((updated) => {
                  onSettingsChange(updated);
                  setCustomModelInput("");
                })
                .catch((err: unknown) => setFeedback(errorToFeedback(err)));
            }}
            className="w-full rounded-md border border-line-strong bg-canvas px-3 py-1.5 text-xs text-ink outline-none focus-visible:ring-2 focus-visible:ring-accent"
          >
            {models.map((m) => (
              <option key={m.id} value={m.id}>
                {m.label} (${m.inputCostPerMillion.toFixed(2)} / $
                {m.outputCostPerMillion.toFixed(2)} per 1M) — {m.note}
              </option>
            ))}
          </select>
        </div>

        <div className="space-y-1">
          <label
            htmlFor="custom-model-input"
            className="block text-xs font-medium text-ink-secondary"
          >
            {MESSAGES.settings.customModelLabel}
          </label>
          <div className="flex flex-wrap items-center gap-2">
            <input
              id="custom-model-input"
              type="text"
              value={customModelInput}
              onChange={(e) => setCustomModelInput(e.target.value)}
              placeholder={MESSAGES.settings.customModelPlaceholder}
              className="min-w-0 flex-1 rounded-md border border-line-strong bg-canvas px-3 py-1.5 font-mono text-xs text-ink outline-none focus-visible:ring-2 focus-visible:ring-accent"
            />
            <button
              type="button"
              disabled={checkingModel || customModelInput.trim().length === 0}
              onClick={handleCheckCustomModel}
              className="rounded-md border border-line-strong bg-canvas px-3 py-1.5 text-xs font-medium text-ink hover:bg-hover focus-visible:ring-2 focus-visible:ring-accent disabled:opacity-50"
            >
              {checkingModel
                ? MESSAGES.settings.customModelCheckingButton
                : MESSAGES.settings.customModelCheckButton}
            </button>
            {settings.customModelId && (
              <button
                type="button"
                disabled={checkingModel}
                onClick={() => {
                  setCustomModelInput("");
                  saveCustomModel(null)
                    .then(onSettingsChange)
                    .catch((err: unknown) => setFeedback(errorToFeedback(err)));
                }}
                className="rounded-md border border-line-strong bg-canvas px-2.5 py-1.5 text-xs text-ink-secondary hover:bg-hover focus-visible:ring-2 focus-visible:ring-accent"
              >
                {MESSAGES.settings.customModelClearButton}
              </button>
            )}
          </div>
        </div>

        <label className="flex items-start gap-2.5 text-xs text-ink">
          <input
            type="checkbox"
            checked={settings.denyDataCollection}
            onChange={(e) => {
              savePrivacy(e.target.checked)
                .then(onSettingsChange)
                .catch((err: unknown) => setFeedback(errorToFeedback(err)));
            }}
            className="mt-0.5 accent-accent"
          />
          <span className="space-y-0.5">
            <span className="block font-medium text-ink">{MESSAGES.settings.privacyLabel}</span>
            <span className="block text-[11px] text-ink-muted">
              {MESSAGES.settings.privacyHelp}
            </span>
          </span>
        </label>

        {feedback && (
          <div
            role={feedback.kind === "error" ? "alert" : "status"}
            className={`rounded-md border bg-canvas px-3 py-2 text-xs ${
              feedback.kind === "error"
                ? "border-status-error text-status-error"
                : "border-line-strong text-ink"
            }`}
          >
            <p className="font-medium">{feedback.headline}</p>
            {feedback.detail && <p className="mt-1 text-ink-secondary">{feedback.detail}</p>}
          </div>
        )}
      </section>

      {/* 3. Grammar */}
      <section className="space-y-2 rounded-lg border border-line bg-elevated p-4">
        <h2 className="text-sm font-semibold text-ink">{MESSAGES.grammar.sectionTitle}</h2>
        <div className="space-y-1">
          <label htmlFor="dialect-select" className="block text-xs font-medium text-ink-secondary">
            {MESSAGES.grammar.dialectLabel}
          </label>
          <select
            id="dialect-select"
            value={settings.dialect}
            onChange={(e) => {
              saveDialect(e.target.value)
                .then(onSettingsChange)
                .catch((err: unknown) => setFeedback(errorToFeedback(err)));
            }}
            className="w-full rounded-md border border-line-strong bg-canvas px-3 py-1.5 text-xs text-ink outline-none focus-visible:ring-2 focus-visible:ring-accent"
          >
            {MESSAGES.dialects.map((d) => (
              <option key={d.value} value={d.value}>
                {d.label}
              </option>
            ))}
          </select>
        </div>
      </section>

      {/* 4. About */}
      <section className="space-y-2 rounded-lg border border-line bg-elevated p-4">
        <h2 className="text-sm font-semibold text-ink">{MESSAGES.settings.aboutSectionTitle}</h2>
        <p className="text-xs font-medium text-ink">
          {MESSAGES.settings.versionLabel(status.appVersion)}
        </p>
        <p className="text-xs text-ink-secondary">{MESSAGES.settings.licensesText}</p>
        <div className="flex flex-wrap gap-2 pt-1">
          <button
            type="button"
            onClick={() => openLogFolder().catch(() => {})}
            className="rounded-md border border-line-strong bg-canvas px-3 py-1.5 text-xs font-medium text-ink hover:bg-hover focus-visible:ring-2 focus-visible:ring-accent"
          >
            {MESSAGES.settings.openLogFolderButton}
          </button>
          <button
            type="button"
            onClick={() => openExternalUrl("repo").catch(() => {})}
            className="rounded-md border border-line-strong bg-canvas px-3 py-1.5 text-xs font-medium text-ink hover:bg-hover focus-visible:ring-2 focus-visible:ring-accent"
          >
            {MESSAGES.settings.repoLinkButton}
          </button>
        </div>
      </section>
    </div>
  );
}
