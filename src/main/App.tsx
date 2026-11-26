import { useEffect, useState } from "react";
import {
  deleteApiKey,
  getAppStatus,
  getCuratedModels,
  getSettings,
  isApiError,
  saveApiKey,
  saveModel,
  savePrivacy,
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

export function MainApp() {
  const [status, setStatus] = useState<AppStatusDto | null>(null);
  const [settings, setSettings] = useState<SettingsDto | null>(null);
  const [models, setModels] = useState<ModelOptionDto[]>([]);
  const [keyInput, setKeyInput] = useState("");
  const [busy, setBusy] = useState(false);
  const [testing, setTesting] = useState(false);
  const [feedback, setFeedback] = useState<Feedback | null>(null);

  useEffect(() => {
    let active = true;
    Promise.all([getAppStatus(), getSettings(), getCuratedModels()])
      .then(([loadedStatus, loadedSettings, loadedModels]) => {
        if (!active) return;
        setStatus(loadedStatus);
        setSettings(loadedSettings);
        setModels(loadedModels);
      })
      .catch((err: unknown) => {
        if (!active) return;
        setFeedback(errorToFeedback(err));
      });
    return () => {
      active = false;
    };
  }, []);

  async function handleSaveKey() {
    setBusy(true);
    setFeedback(null);
    try {
      const saved = await saveApiKey(keyInput);
      setKeyInput("");
      setStatus((prev) => (prev ? { ...prev, hasApiKey: saved } : prev));
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
      setStatus((prev) => (prev ? { ...prev, hasApiKey: saved } : prev));
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
      const result = await testApiKey(keyInput);
      setFeedback({
        kind: "info",
        headline: MESSAGES.settings.formatKeyTestSuccess(result),
      });
    } catch (err: unknown) {
      setFeedback(errorToFeedback(err));
    } finally {
      setTesting(false);
    }
  }

  async function handleModelChange(nextModel: string) {
    setFeedback(null);
    try {
      const updated = await saveModel(nextModel);
      setSettings(updated);
    } catch (err: unknown) {
      setFeedback(errorToFeedback(err));
    }
  }

  async function handlePrivacyChange(denyDataCollection: boolean) {
    setFeedback(null);
    try {
      const updated = await savePrivacy(denyDataCollection);
      setSettings(updated);
    } catch (err: unknown) {
      setFeedback(errorToFeedback(err));
    }
  }

  const hotkey = status?.hotkey ?? settings?.hotkey ?? "Ctrl+Alt+G";

  return (
    <main className="flex h-full flex-col justify-between gap-6 bg-canvas p-6 text-ink">
      <div className="space-y-6">
        <header className="space-y-1">
          <div className="flex items-center justify-between gap-4">
            <h1 className="text-xl font-semibold tracking-tight text-ink">
              {MESSAGES.app.name} · {MESSAGES.settings.heading}
            </h1>
            <kbd className="rounded-sm border border-line-strong bg-elevated px-2 py-0.5 font-mono text-xs text-ink-secondary">
              {hotkey}
            </kbd>
          </div>
          <p className="text-sm text-ink-secondary">{MESSAGES.settings.bareLoopHint(hotkey)}</p>
        </header>

        {status?.hotkeyConflict && (
          <div
            role="alert"
            className="rounded-md border border-status-warning bg-elevated px-4 py-3 text-sm text-ink"
          >
            {MESSAGES.settings.hotkeyConflict(status.hotkeyConflict)}
          </div>
        )}

        {status?.settingsRecovered && (
          <div
            role="status"
            className="rounded-md border border-line-strong bg-elevated px-4 py-3 text-sm text-ink-secondary"
          >
            {MESSAGES.settings.settingsRecovered}
          </div>
        )}

        <section className="space-y-4 rounded-lg border border-line bg-elevated p-4">
          <div className="flex items-center justify-between gap-2">
            <h2 className="text-sm font-semibold text-ink">{MESSAGES.settings.aiSectionTitle}</h2>
            <span className="text-xs text-ink-secondary">
              {status?.hasApiKey
                ? MESSAGES.settings.apiKeySavedBadge
                : MESSAGES.settings.apiKeyMissingBadge}
            </span>
          </div>

          <div className="space-y-2">
            <label
              htmlFor="openrouter-key"
              className="block text-xs font-medium text-ink-secondary"
            >
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
                className="min-w-0 flex-1 rounded-md border border-line-strong bg-canvas px-3 py-1.5 text-sm text-ink outline-none focus-visible:ring-2 focus-visible:ring-accent"
              />
              <button
                type="button"
                disabled={busy || testing || keyInput.trim().length === 0}
                onClick={handleSaveKey}
                className="rounded-md bg-accent px-3 py-1.5 text-sm font-medium text-accent-contrast transition hover:bg-accent-hover disabled:opacity-50"
              >
                {MESSAGES.settings.saveKeyButton}
              </button>
              <button
                type="button"
                disabled={busy || testing || (!status?.hasApiKey && keyInput.trim().length === 0)}
                onClick={handleTestKey}
                className="rounded-md border border-line-strong bg-canvas px-3 py-1.5 text-sm font-medium text-ink transition hover:bg-hover disabled:opacity-50"
              >
                {testing ? MESSAGES.settings.testingKeyButton : MESSAGES.settings.testKeyButton}
              </button>
              {status?.hasApiKey && (
                <button
                  type="button"
                  disabled={busy || testing}
                  onClick={handleRemoveKey}
                  className="rounded-md border border-line-strong bg-canvas px-3 py-1.5 text-sm font-medium text-ink-secondary transition hover:bg-hover disabled:opacity-50"
                >
                  {MESSAGES.settings.removeKeyButton}
                </button>
              )}
            </div>

            {feedback && (
              <div
                role={feedback.kind === "error" ? "alert" : "status"}
                className={`rounded-md border px-3 py-2 text-xs ${
                  feedback.kind === "error"
                    ? "border-status-error text-status-error"
                    : "border-line-strong text-ink"
                } bg-canvas`}
              >
                <p className="font-medium">{feedback.headline}</p>
                {feedback.detail && <p className="mt-1 text-ink-secondary">{feedback.detail}</p>}
              </div>
            )}
          </div>

          <div className="space-y-1.5">
            <label htmlFor="model-select" className="block text-xs font-medium text-ink-secondary">
              {MESSAGES.settings.modelLabel}
            </label>
            <select
              id="model-select"
              value={settings?.model ?? ""}
              onChange={(e) => handleModelChange(e.target.value)}
              className="w-full rounded-md border border-line-strong bg-canvas px-3 py-1.5 text-sm text-ink outline-none focus-visible:ring-2 focus-visible:ring-accent"
            >
              {models.map((m) => (
                <option key={m.id} value={m.id}>
                  {m.label} (${m.inputCostPerMillion.toFixed(2)} / $
                  {m.outputCostPerMillion.toFixed(2)} per 1M) — {m.note}
                </option>
              ))}
            </select>
          </div>

          <label className="flex items-start gap-2.5 text-sm text-ink">
            <input
              type="checkbox"
              checked={settings?.denyDataCollection ?? true}
              onChange={(e) => handlePrivacyChange(e.target.checked)}
              className="mt-0.5 accent-accent"
            />
            <span className="space-y-0.5">
              <span className="block text-sm font-medium text-ink">
                {MESSAGES.settings.privacyLabel}
              </span>
              <span className="block text-xs text-ink-muted">{MESSAGES.settings.privacyHelp}</span>
            </span>
          </label>
        </section>
      </div>
    </main>
  );
}
