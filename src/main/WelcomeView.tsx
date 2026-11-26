import { useState } from "react";
import {
  completeWelcome,
  isApiError,
  openExternalUrl,
  saveApiKey,
  setAutostartEnabled,
  testApiKey,
  type AppStatusDto,
} from "../lib/ipc";
import { formatApiError, MESSAGES } from "../lib/messages";

interface WelcomeViewProps {
  status: AppStatusDto;
  onComplete: (updates: { hasApiKey: boolean; autostartEnabled: boolean }) => void;
}

export function WelcomeView({ status, onComplete }: WelcomeViewProps) {
  const [keyInput, setKeyInput] = useState("");
  const [hasKey, setHasKey] = useState(status.hasApiKey);
  const [autostart, setAutostart] = useState(status.autostartEnabled);
  const [busy, setBusy] = useState(false);
  const [testing, setTesting] = useState(false);
  const [feedback, setFeedback] = useState<{ kind: "info" | "error"; text: string } | null>(null);

  async function handleSaveKey() {
    if (keyInput.trim().length === 0) return;
    setBusy(true);
    setFeedback(null);
    try {
      await saveApiKey(keyInput);
      setHasKey(true);
      setKeyInput("");
      setFeedback({ kind: "info", text: MESSAGES.settings.keySavedNotice });
    } catch (err: unknown) {
      const msg = isApiError(err) ? formatApiError(err).headline : String(err);
      setFeedback({ kind: "error", text: msg });
    } finally {
      setBusy(false);
    }
  }

  async function handleTestKey() {
    setTesting(true);
    setFeedback(null);
    try {
      const res = await testApiKey(keyInput);
      setFeedback({ kind: "info", text: MESSAGES.settings.formatKeyTestSuccess(res) });
    } catch (err: unknown) {
      const msg = isApiError(err) ? formatApiError(err).headline : String(err);
      setFeedback({ kind: "error", text: msg });
    } finally {
      setTesting(false);
    }
  }

  async function handleAutostartToggle(checked: boolean) {
    setAutostart(checked);
    try {
      const actual = await setAutostartEnabled(checked);
      setAutostart(actual);
    } catch (err: unknown) {
      setAutostart(!checked);
      const msg = isApiError(err) ? formatApiError(err).headline : String(err);
      setFeedback({ kind: "error", text: msg });
    }
  }

  async function handleDone() {
    setBusy(true);
    try {
      if (keyInput.trim().length > 0 && !hasKey) {
        await saveApiKey(keyInput);
        setHasKey(true);
      }
      await completeWelcome();
      onComplete({ hasApiKey: hasKey || keyInput.trim().length > 0, autostartEnabled: autostart });
    } finally {
      setBusy(false);
    }
  }

  return (
    <main className="mx-auto flex h-full max-w-lg flex-col justify-center gap-6 bg-canvas p-8 text-ink">
      <header className="space-y-2">
        <h1 className="text-2xl font-semibold tracking-tight text-ink">
          {MESSAGES.welcome.heading}
        </h1>
        <p className="text-sm text-ink-secondary">
          {MESSAGES.welcome.hotkeySummary(status.hotkey)}
        </p>
        <p className="rounded-md border border-line bg-elevated px-3 py-2 text-xs text-ink">
          {MESSAGES.welcome.offlineReassurance}
        </p>
      </header>

      <section className="space-y-3 rounded-lg border border-line bg-elevated p-4">
        <div className="flex items-center justify-between gap-2">
          <h2 className="text-sm font-medium text-ink">{MESSAGES.welcome.optionalKeyHeading}</h2>
          <button
            type="button"
            onClick={() => openExternalUrl("openrouter_keys").catch(() => {})}
            className="text-xs font-medium text-accent underline underline-offset-2 focus-visible:ring-2 focus-visible:ring-accent"
          >
            {MESSAGES.settings.getKeyButton}
          </button>
        </div>

        <div className="flex flex-wrap items-center gap-2">
          <input
            type="password"
            autoComplete="off"
            spellCheck={false}
            value={keyInput}
            onChange={(e) => setKeyInput(e.target.value)}
            placeholder={MESSAGES.settings.apiKeyPlaceholder}
            aria-label={MESSAGES.settings.apiKeyLabel}
            className="min-w-0 flex-1 rounded-md border border-line-strong bg-canvas px-3 py-1.5 text-sm text-ink outline-none focus-visible:ring-2 focus-visible:ring-accent"
          />
          <button
            type="button"
            disabled={busy || testing || keyInput.trim().length === 0}
            onClick={handleSaveKey}
            className="rounded-md border border-line-strong bg-canvas px-3 py-1.5 text-sm font-medium text-ink transition hover:bg-hover focus-visible:ring-2 focus-visible:ring-accent disabled:opacity-50"
          >
            {MESSAGES.settings.saveKeyButton}
          </button>
          <button
            type="button"
            disabled={busy || testing || (!hasKey && keyInput.trim().length === 0)}
            onClick={handleTestKey}
            className="rounded-md border border-line-strong bg-canvas px-3 py-1.5 text-sm font-medium text-ink transition hover:bg-hover focus-visible:ring-2 focus-visible:ring-accent disabled:opacity-50"
          >
            {testing ? MESSAGES.settings.testingKeyButton : MESSAGES.settings.testKeyButton}
          </button>
        </div>

        {feedback && (
          <p
            role={feedback.kind === "error" ? "alert" : "status"}
            className={`text-xs ${
              feedback.kind === "error" ? "text-status-error" : "text-ink-secondary"
            }`}
          >
            {feedback.text}
          </p>
        )}
      </section>

      <label className="flex items-center gap-2.5 text-sm text-ink">
        <input
          type="checkbox"
          checked={autostart}
          onChange={(e) => void handleAutostartToggle(e.target.checked)}
          className="accent-accent"
        />
        <span>{MESSAGES.welcome.startAtLogin}</span>
      </label>

      <div className="flex justify-end">
        <button
          type="button"
          disabled={busy}
          onClick={handleDone}
          className="rounded-md bg-accent px-5 py-2 text-sm font-medium text-accent-contrast transition hover:bg-accent-hover focus-visible:ring-2 focus-visible:ring-accent disabled:opacity-50"
        >
          {MESSAGES.welcome.doneButton}
        </button>
      </div>
    </main>
  );
}
