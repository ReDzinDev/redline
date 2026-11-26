import { buildUnderlinedSegments, type LintCategory, type LintDto } from "../lib/grammarLogic";
import { MESSAGES } from "../lib/messages";

interface UnderlinedTextProps {
  text: string;
  lints: LintDto[];
  ignoredIds: ReadonlySet<string>;
  compact?: boolean;
  onSelectLint?: (lint: LintDto) => void;
}

function underlineClasses(category: LintCategory): string {
  switch (category) {
    case "spelling":
      return "underline decoration-wavy decoration-status-error underline-offset-2";
    case "grammar":
      return "underline decoration-solid decoration-status-info underline-offset-2";
    case "style":
      return "underline decoration-dotted decoration-status-warning underline-offset-2";
  }
}

export function UnderlinedText({
  text,
  lints,
  ignoredIds,
  compact = false,
  onSelectLint,
}: UnderlinedTextProps) {
  const segments = buildUnderlinedSegments(text, lints, ignoredIds);

  return (
    <div
      className={`${compact ? "max-h-16" : "max-h-24"} shrink-0 overflow-y-auto rounded-md border border-line bg-elevated px-2.5 py-2 text-xs leading-relaxed whitespace-pre-wrap text-ink select-text`}
    >
      {segments.map((seg, idx) =>
        seg.lint ? (
          <button
            key={`${seg.lint.id}-${idx}`}
            type="button"
            title={seg.lint.message}
            aria-label={MESSAGES.a11y.selectIssue(seg.lint.category, seg.text)}
            onClick={() => onSelectLint?.(seg.lint!)}
            className={`${underlineClasses(seg.lint.category)} inline cursor-pointer rounded-sm border-0 bg-transparent p-0 text-left font-sans text-xs text-ink outline-none focus-visible:ring-2 focus-visible:ring-accent`}
          >
            {seg.text}
          </button>
        ) : (
          <span key={idx}>{seg.text}</span>
        ),
      )}
    </div>
  );
}
