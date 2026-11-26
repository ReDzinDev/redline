import type { ApiErrorDto } from "../lib/ipc";
import { formatApiError, MESSAGES } from "../lib/messages";
import { formatCostUsd, formatElapsedSeconds, type AiRunStatus } from "../lib/popupLogic";

export interface ResultState {
  status: AiRunStatus;
  actionLabel: string;
  text: string;
  partialText: string;
  modelUsed: string | null;
  costUsd: number | null;
  elapsedMs: number | null;
  unchanged: boolean;
  error: ApiErrorDto | null;
  cutOffDetail: string | null;
}

interface ResultPanelProps {
  result: ResultState;
  copied: boolean;
  onRetry: () => void;
  onCopy: (text: string) => void;
  onOpenSettings: () => void;
}

export function ResultPanel({ result, copied, onRetry, onCopy, onOpenSettings }: ResultPanelProps) {
  if (result.status === "idle") {
    return null;
  }

  if (result.status === "error" && result.error) {
    const formatted = formatApiError(result.error);
    return (
      <section
        role="alert"
        className="space-y-2 rounded-md border border-status-error bg-elevated p-3 text-xs"
      >
        <p className="font-medium text-status-error">{formatted.headline}</p>
        {formatted.detail && <p className="text-ink-secondary">{formatted.detail}</p>}
        <div className="flex flex-wrap gap-2">
          {formatted.nextStep === "retry" && (
            <button
              type="button"
              onClick={onRetry}
              className="rounded-md bg-accent px-2.5 py-1 text-xs font-medium text-accent-contrast hover:bg-accent-hover focus-visible:ring-2 focus-visible:ring-accent"
            >
              {MESSAGES.popup.retry}
            </button>
          )}
          {formatted.nextStep === "settings" && (
            <button
              type="button"
              onClick={onOpenSettings}
              className="rounded-md bg-accent px-2.5 py-1 text-xs font-medium text-accent-contrast hover:bg-accent-hover focus-visible:ring-2 focus-visible:ring-accent"
            >
              {MESSAGES.popup.openSettings}
            </button>
          )}
        </div>
      </section>
    );
  }

  if (result.status === "cut_off") {
    return (
      <section
        role="alert"
        className="flex min-h-0 flex-1 flex-col gap-2 rounded-md border border-status-warning bg-elevated p-3 text-xs"
      >
        <div className="flex items-center justify-between gap-2">
          <p className="font-medium text-ink">{MESSAGES.popup.cutOffHeadline}</p>
          <div className="flex items-center gap-1.5">
            <button
              type="button"
              onClick={onRetry}
              className="rounded-md bg-accent px-2 py-0.5 text-xs font-medium text-accent-contrast hover:bg-accent-hover focus-visible:ring-2 focus-visible:ring-accent"
            >
              {MESSAGES.popup.retry}
            </button>
            {result.partialText.length > 0 && (
              <button
                type="button"
                onClick={() => onCopy(result.partialText)}
                className="rounded-md border border-line-strong bg-canvas px-2 py-0.5 text-xs font-medium text-ink hover:bg-hover focus-visible:ring-2 focus-visible:ring-accent"
              >
                {copied ? MESSAGES.popup.copied : MESSAGES.popup.copy}
              </button>
            )}
          </div>
        </div>
        {result.cutOffDetail && <p className="text-ink-secondary">{result.cutOffDetail}</p>}
        {result.partialText.length > 0 && (
          <div className="min-h-0 flex-1 overflow-y-auto rounded-sm border border-line bg-canvas p-2 text-sm whitespace-pre-wrap text-ink-secondary">
            {result.partialText}
          </div>
        )}
      </section>
    );
  }

  const footerParts = [
    result.actionLabel,
    result.modelUsed ? shortModelName(result.modelUsed) : null,
    formatCostUsd(result.costUsd),
    formatElapsedSeconds(result.elapsedMs),
  ].filter(Boolean);

  return (
    <section
      aria-busy={result.status === "streaming"}
      className="flex min-h-0 flex-1 flex-col gap-2 rounded-md border border-line bg-elevated p-3"
    >
      <div className="flex items-center justify-between gap-2 text-xs text-ink-secondary">
        <span className="font-medium text-ink">
          {result.status === "streaming"
            ? `${result.actionLabel} · ${MESSAGES.popup.streamingLabel}`
            : result.unchanged
              ? MESSAGES.popup.noChanges
              : result.actionLabel}
        </span>
        {result.status === "complete" && result.text.length > 0 && (
          <button
            type="button"
            onClick={() => onCopy(result.text)}
            className="rounded-sm border border-line-strong bg-canvas px-2 py-0.5 text-[11px] font-medium text-ink hover:bg-hover focus-visible:ring-2 focus-visible:ring-accent"
          >
            {copied ? MESSAGES.popup.copied : MESSAGES.popup.copy}
          </button>
        )}
      </div>

      <div className="min-h-0 flex-1 overflow-y-auto rounded-sm border border-line bg-canvas p-2.5 text-sm leading-relaxed whitespace-pre-wrap text-ink">
        {result.text}
      </div>

      {result.status === "complete" && footerParts.length > 0 && (
        <p className="truncate text-[11px] text-ink-muted">{footerParts.join(" · ")}</p>
      )}
    </section>
  );
}

function shortModelName(modelId: string): string {
  const parts = modelId.split("/");
  return parts[parts.length - 1] ?? modelId;
}
