import { diffWordsWithSpace } from "diff";
import { useMemo } from "react";

interface DiffViewProps {
  original: string;
  modified: string;
}

export function DiffView({ original, modified }: DiffViewProps) {
  const parts = useMemo(
    () =>
      diffWordsWithSpace(original, modified, { timeout: 250, maxEditLength: 4000 }) ?? [
        { removed: true, added: false, value: original },
        { added: true, removed: false, value: modified },
      ],
    [original, modified],
  );

  return (
    <div className="text-sm leading-relaxed whitespace-pre-wrap text-ink">
      {parts.map((part, idx) => {
        if (part.added) {
          return (
            <ins
              key={idx}
              className="rounded-sm bg-diff-added-bg px-0.5 text-diff-added-ink no-underline"
            >
              {part.value}
            </ins>
          );
        }
        if (part.removed) {
          return (
            <del
              key={idx}
              className="rounded-sm bg-diff-removed-bg px-0.5 text-diff-removed-ink line-through"
            >
              {part.value}
            </del>
          );
        }
        return <span key={idx}>{part.value}</span>;
      })}
    </div>
  );
}
