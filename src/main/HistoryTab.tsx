import { useCallback, useEffect, useState } from "react";
import { DiffView } from "../components/DiffView";
import { clearHistory, copyToClipboard, getHistory, type HistoryEntryDto } from "../lib/ipc";
import { MESSAGES } from "../lib/messages";
import { formatCostUsd, formatSourceAppLabel } from "../lib/popupLogic";

interface HistoryTabProps {
  historyEnabled: boolean;
}

export function HistoryTab({ historyEnabled }: HistoryTabProps) {
  const [entries, setEntries] = useState<HistoryEntryDto[]>([]);
  const [expandedId, setExpandedId] = useState<string | null>(null);
  const [copiedKey, setCopiedKey] = useState<string | null>(null);

  const loadEntries = useCallback(() => {
    getHistory()
      .then(setEntries)
      .catch(() => {});
  }, []);

  useEffect(() => {
    loadEntries();
  }, [loadEntries]);

  async function handleClear() {
    try {
      await clearHistory();
      setEntries([]);
      setExpandedId(null);
    } catch {
      // Ignore clear error
    }
  }

  async function handleCopy(key: string, value: string) {
    try {
      await copyToClipboard(value);
      setCopiedKey(key);
    } catch {
      // Ignore
    }
  }

  return (
    <div className="flex min-h-0 flex-1 flex-col gap-3">
      {!historyEnabled && (
        <div
          role="status"
          className="rounded-md border border-line-strong bg-elevated px-3 py-2 text-xs text-ink-secondary"
        >
          {MESSAGES.history.disabledNotice}
        </div>
      )}

      <div className="flex items-center justify-between gap-2">
        <span className="text-xs text-ink-muted">
          {entries.length > 0 ? `${entries.length} / 20` : ""}
        </span>
        {entries.length > 0 && (
          <button
            type="button"
            onClick={handleClear}
            className="rounded-md border border-line-strong bg-canvas px-2.5 py-1 text-xs font-medium text-ink-secondary transition hover:bg-hover focus-visible:ring-2 focus-visible:ring-accent"
          >
            {MESSAGES.history.clearButton}
          </button>
        )}
      </div>

      {entries.length === 0 ? (
        <div className="flex flex-1 items-center justify-center text-sm text-ink-muted">
          {MESSAGES.history.empty}
        </div>
      ) : (
        <div className="min-h-0 flex-1 space-y-2 overflow-y-auto pr-0.5">
          {entries.map((entry) => {
            const isOpen = expandedId === entry.id;
            const appLabel = entry.sourceApp ? formatSourceAppLabel(entry.sourceApp) : null;
            const cost = formatCostUsd(entry.costUsd);
            const meta = [
              formatTimestamp(entry.timestampMs),
              entry.action,
              appLabel,
              shortModel(entry.model),
              cost,
            ]
              .filter(Boolean)
              .join(" · ");

            return (
              <article
                key={entry.id}
                className="rounded-md border border-line bg-elevated p-3 text-xs"
              >
                <button
                  type="button"
                  aria-expanded={isOpen}
                  aria-controls={`history-diff-${entry.id}`}
                  onClick={() => setExpandedId(isOpen ? null : entry.id)}
                  className="flex w-full items-center justify-between gap-2 rounded-sm text-left font-medium text-ink outline-none focus-visible:ring-2 focus-visible:ring-accent"
                >
                  <span className="min-w-0 flex-1">
                    <span className="block truncate">{meta}</span>
                    <span className="mt-0.5 block truncate font-normal text-ink-secondary">
                      {entry.result}
                    </span>
                  </span>
                  <span className="shrink-0 rounded-sm border border-line-strong bg-canvas px-2 py-0.5 text-2xs font-normal text-ink-secondary">
                    {isOpen ? MESSAGES.history.hideDiff : MESSAGES.history.showDiff}
                  </span>
                </button>

                {isOpen && (
                  <div
                    id={`history-diff-${entry.id}`}
                    className="mt-3 space-y-2 border-t border-line pt-2.5"
                  >
                    <div className="rounded-sm border border-line bg-canvas p-2.5">
                      <DiffView original={entry.original} modified={entry.result} />
                    </div>
                    <div className="flex flex-wrap gap-2">
                      <button
                        type="button"
                        onClick={() => void handleCopy(`${entry.id}-orig`, entry.original)}
                        className="rounded-sm border border-line-strong bg-canvas px-2.5 py-1 text-xs font-medium text-ink hover:bg-hover focus-visible:ring-2 focus-visible:ring-accent"
                      >
                        {copiedKey === `${entry.id}-orig`
                          ? MESSAGES.history.copied
                          : MESSAGES.history.copyOriginal}
                      </button>
                      <button
                        type="button"
                        onClick={() => void handleCopy(`${entry.id}-res`, entry.result)}
                        className="rounded-sm bg-accent px-2.5 py-1 text-xs font-medium text-accent-contrast hover:bg-accent-hover focus-visible:ring-2 focus-visible:ring-accent"
                      >
                        {copiedKey === `${entry.id}-res`
                          ? MESSAGES.history.copied
                          : MESSAGES.history.copyResult}
                      </button>
                    </div>
                  </div>
                )}
              </article>
            );
          })}
        </div>
      )}
    </div>
  );
}

function formatTimestamp(ms: number): string {
  try {
    return new Date(ms).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
  } catch {
    return "";
  }
}

function shortModel(modelId: string): string {
  const parts = modelId.split("/");
  return parts[parts.length - 1] ?? modelId;
}
