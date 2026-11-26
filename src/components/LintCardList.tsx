import { useRef } from "react";
import { sliceVisibleCards, type LintDto, type SuggestionDto } from "../lib/grammarLogic";
import { MESSAGES } from "../lib/messages";

interface LintCardListProps {
  loading: boolean;
  english: boolean;
  lints: LintDto[];
  ignoredIds: ReadonlySet<string>;
  canUndo: boolean;
  highlightedLintId?: string | null;
  onApply: (lint: LintDto, suggestion: SuggestionDto) => void;
  onIgnore: (lintId: string) => void;
  onFixAll: () => void;
  onUndo: () => void;
  onSelectCard?: (lint: LintDto) => void;
}

export function LintCardList({
  loading,
  english,
  lints,
  ignoredIds,
  canUndo,
  highlightedLintId,
  onApply,
  onIgnore,
  onFixAll,
  onUndo,
  onSelectCard,
}: LintCardListProps) {
  const listRef = useRef<HTMLDivElement | null>(null);
  const { visible, overflowCount, fixableCount } = sliceVisibleCards(lints, ignoredIds);

  function focusNextCard(currentIdx: number) {
    requestAnimationFrame(() => {
      const container = listRef.current;
      if (!container) return;
      const cards = container.querySelectorAll<HTMLElement>("[data-lint-card]");
      if (cards.length === 0) return;
      const target = cards[Math.min(currentIdx, cards.length - 1)];
      const focusable = target?.querySelector<HTMLElement>("button") ?? target;
      focusable?.focus();
    });
  }

  if (loading && lints.length === 0) {
    return <p className="text-xs text-ink-muted">{MESSAGES.grammar.checking}</p>;
  }

  if (!english) {
    return <p className="text-xs text-ink-muted">{MESSAGES.grammar.englishOnly}</p>;
  }

  if (visible.length === 0 && !canUndo) {
    return null;
  }

  return (
    <section aria-label={MESSAGES.grammar.sectionLabel} className="space-y-1.5">
      <div className="flex items-center justify-between gap-2">
        <span className="text-[11px] font-medium text-ink-secondary">
          {visible.length > 0
            ? MESSAGES.grammar.issuesCount(visible.length + overflowCount)
            : MESSAGES.grammar.noIssues}
        </span>
        <div className="flex items-center gap-1.5">
          {canUndo && (
            <button
              type="button"
              onClick={onUndo}
              className="rounded-sm border border-line-strong bg-canvas px-2 py-0.5 text-[11px] font-medium text-ink transition hover:bg-hover focus-visible:ring-2 focus-visible:ring-accent"
            >
              {MESSAGES.grammar.undo}
            </button>
          )}
          {fixableCount >= 2 && (
            <button
              type="button"
              onClick={onFixAll}
              className="inline-flex items-center gap-1 rounded-sm bg-accent px-2 py-0.5 text-[11px] font-medium text-accent-contrast transition hover:bg-accent-hover focus-visible:ring-2 focus-visible:ring-accent"
            >
              <kbd className="font-mono text-[10px] opacity-80">F</kbd>
              <span>{MESSAGES.grammar.fixAll}</span>
            </button>
          )}
        </div>
      </div>

      {visible.length > 0 && (
        <div ref={listRef} className="max-h-36 space-y-1.5 overflow-y-auto pr-0.5">
          {visible.map((lint, idx) => {
            const isHighlighted = highlightedLintId === lint.id;
            return (
              <article
                key={lint.id}
                data-lint-card
                onClick={() => onSelectCard?.(lint)}
                className={`space-y-1 rounded-md border bg-elevated p-2 text-xs ${
                  isHighlighted ? "border-accent" : "border-line"
                }`}
              >
                <div className="flex items-start justify-between gap-2">
                  <div className="min-w-0 space-y-0.5">
                    <div className="flex items-center gap-1.5">
                      <span className="rounded-sm border border-line bg-canvas px-1 py-0.5 text-[10px] font-medium text-ink-secondary">
                        {lint.category}
                      </span>
                      <span className="truncate font-medium text-ink">{lint.flagged}</span>
                    </div>
                    <p className="text-[11px] text-ink-secondary">{lint.message}</p>
                  </div>
                  <button
                    type="button"
                    onClick={(e) => {
                      e.stopPropagation();
                      onIgnore(lint.id);
                      focusNextCard(idx);
                    }}
                    className="shrink-0 rounded-sm px-1.5 py-0.5 text-[11px] text-ink-muted transition hover:bg-hover hover:text-ink focus-visible:ring-2 focus-visible:ring-accent"
                  >
                    {MESSAGES.grammar.ignore}
                  </button>
                </div>

                {lint.suggestions.length > 0 && (
                  <div className="flex flex-wrap gap-1">
                    {lint.suggestions.slice(0, 3).map((sug, sIdx) => (
                      <button
                        key={`${lint.id}-${sIdx}`}
                        type="button"
                        onClick={(e) => {
                          e.stopPropagation();
                          onApply(lint, sug);
                          focusNextCard(idx);
                        }}
                        className="rounded-sm border border-line-strong bg-canvas px-2 py-0.5 text-[11px] font-medium text-ink transition hover:bg-hover focus-visible:ring-2 focus-visible:ring-accent"
                      >
                        {sug.label}
                      </button>
                    ))}
                  </div>
                )}
              </article>
            );
          })}

          {overflowCount > 0 && (
            <p className="px-1 text-[11px] text-ink-muted">
              {MESSAGES.grammar.moreCount(overflowCount)}
            </p>
          )}
        </div>
      )}
    </section>
  );
}
