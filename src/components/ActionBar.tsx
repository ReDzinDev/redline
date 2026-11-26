import type { ActionKind, ToneOption } from "../lib/ipc";
import { MESSAGES } from "../lib/messages";

interface ActionBarProps {
  activeKind: ActionKind | null;
  subMode: "none" | "tone" | "custom";
  customInput: string;
  recentCustom: string[];
  disabled?: boolean;
  keyPrefix?: string;
  onSelectAction: (kind: ActionKind) => void;
  onSelectTone: (tone: ToneOption) => void;
  onCustomInputChange: (value: string) => void;
  onRunCustom: (instruction: string) => void;
}

export function ActionBar({
  activeKind,
  subMode,
  customInput,
  recentCustom,
  disabled = false,
  keyPrefix = "",
  onSelectAction,
  onSelectTone,
  onCustomInputChange,
  onRunCustom,
}: ActionBarProps) {
  return (
    <div className="shrink-0 space-y-2">
      <div className="flex flex-wrap gap-1.5" role="toolbar" aria-label={MESSAGES.a11y.aiActions}>
        {MESSAGES.actions.map((item) => {
          const isSelected =
            activeKind === item.kind ||
            (subMode === "tone" && item.kind === "change_tone") ||
            (subMode === "custom" && item.kind === "custom");
          return (
            <button
              key={item.kind}
              type="button"
              disabled={disabled}
              onClick={() => onSelectAction(item.kind)}
              className={`inline-flex items-center gap-1 rounded-md border px-2 py-1 text-xs font-medium transition outline-none focus-visible:ring-2 focus-visible:ring-accent disabled:opacity-50 ${
                isSelected
                  ? "border-accent bg-accent text-accent-contrast"
                  : "border-line-strong bg-elevated text-ink hover:bg-hover"
              }`}
            >
              <kbd className="font-mono text-3xs opacity-80">
                {keyPrefix}
                {item.key}
              </kbd>
              <span>{item.label}</span>
            </button>
          );
        })}
      </div>

      {subMode === "tone" && (
        <div className="flex flex-wrap gap-1.5" role="group" aria-label={MESSAGES.a11y.chooseTone}>
          {MESSAGES.tones.map((t) => (
            <button
              key={t.tone}
              type="button"
              onClick={() => onSelectTone(t.tone)}
              className="inline-flex items-center gap-1 rounded-md border border-line-strong bg-canvas px-2 py-1 text-xs text-ink transition outline-none hover:bg-hover focus-visible:ring-2 focus-visible:ring-accent"
            >
              <kbd className="font-mono text-3xs text-ink-muted">{t.key}</kbd>
              <span>{t.label}</span>
            </button>
          ))}
        </div>
      )}

      {subMode === "custom" && (
        <div className="space-y-1.5">
          <form
            className="flex items-center gap-1.5"
            onSubmit={(e) => {
              e.preventDefault();
              if (customInput.trim().length > 0) {
                onRunCustom(customInput.trim());
              }
            }}
          >
            <input
              type="text"
              autoFocus
              value={customInput}
              onChange={(e) => onCustomInputChange(e.target.value)}
              placeholder={MESSAGES.popup.customPlaceholder}
              className="min-w-0 flex-1 rounded-md border border-line-strong bg-canvas px-2.5 py-1 text-xs text-ink outline-none focus-visible:ring-2 focus-visible:ring-accent"
            />
            <button
              type="submit"
              disabled={customInput.trim().length === 0}
              className="rounded-md bg-accent px-2.5 py-1 text-xs font-medium text-accent-contrast transition hover:bg-accent-hover focus-visible:ring-2 focus-visible:ring-accent disabled:opacity-50"
            >
              {MESSAGES.popup.customSubmit}
            </button>
          </form>
          {recentCustom.length > 0 && (
            <div className="flex flex-wrap items-center gap-1">
              <span className="text-2xs text-ink-muted">{MESSAGES.popup.recentCustomLabel}:</span>
              {recentCustom.map((item) => (
                <button
                  key={item}
                  type="button"
                  onClick={() => {
                    onCustomInputChange(item);
                    onRunCustom(item);
                  }}
                  className="max-w-full truncate rounded-sm border border-line bg-elevated px-1.5 py-0.5 text-2xs text-ink-secondary transition hover:bg-hover focus-visible:ring-2 focus-visible:ring-accent"
                >
                  {item}
                </button>
              ))}
            </div>
          )}
        </div>
      )}
    </div>
  );
}
