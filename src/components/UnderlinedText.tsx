import { buildUnderlinedSegments, type LintCategory, type LintDto } from "../lib/grammarLogic";

interface UnderlinedTextProps {
  text: string;
  lints: LintDto[];
  ignoredIds: ReadonlySet<string>;
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

export function UnderlinedText({ text, lints, ignoredIds, onSelectLint }: UnderlinedTextProps) {
  const segments = buildUnderlinedSegments(text, lints, ignoredIds);

  return (
    <div className="max-h-24 overflow-y-auto rounded-md border border-line bg-elevated px-2.5 py-2 text-xs leading-relaxed whitespace-pre-wrap text-ink select-text">
      {segments.map((seg, idx) =>
        seg.lint ? (
          <span
            key={`${seg.lint.id}-${idx}`}
            title={seg.lint.message}
            onClick={() => onSelectLint?.(seg.lint!)}
            className={`${underlineClasses(seg.lint.category)} cursor-pointer`}
          >
            {seg.text}
          </span>
        ) : (
          <span key={idx}>{seg.text}</span>
        ),
      )}
    </div>
  );
}
