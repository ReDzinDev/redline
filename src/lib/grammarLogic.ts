/**
 * Pure grammar-fix logic (Apply guard, Fix all reverse order, Undo stack,
 * stale response filtering, underline segmentation). Tested with Vitest.
 */
export type LintCategory = "spelling" | "grammar" | "style";

export interface SuggestionDto {
  label: string;
  replacement: string;
}

export interface LintDto {
  id: string;
  start: number;
  end: number;
  flagged: string;
  category: LintCategory;
  kind: string;
  message: string;
  suggestions: SuggestionDto[];
}

export interface LintResponseDto {
  requestId: number;
  english: boolean;
  lints: LintDto[];
}

export const MAX_VISIBLE_LINT_CARDS = 20;

export function isLatestLintResponse(responseRequestId: number, latestRequestId: number): boolean {
  return responseRequestId === latestRequestId;
}

/**
 * Applies a single suggestion ONLY if `text.slice(start, end) === lint.flagged`.
 * Prevents stale or double-clicked Apply actions from corrupting text.
 */
export function applySingleFix(
  text: string,
  lint: LintDto,
  suggestion: SuggestionDto,
): string | null {
  if (lint.start < 0 || lint.end < lint.start || lint.end > text.length) {
    return null;
  }
  if (text.slice(lint.start, lint.end) !== lint.flagged) {
    return null;
  }
  return text.slice(0, lint.start) + suggestion.replacement + text.slice(lint.end);
}

/**
 * Applies the first suggestion of every visible, non-ignored lint that has one,
 * ordered from the last span to the first so earlier UTF-16 offsets stay valid.
 */
export function applyAllFixes(
  text: string,
  lints: LintDto[],
  ignoredIds: ReadonlySet<string>,
): { text: string; appliedCount: number } {
  const candidates = lints
    .filter((lint) => !ignoredIds.has(lint.id) && lint.suggestions.length > 0)
    .sort((a, b) => b.start - a.start);

  let current = text;
  let appliedCount = 0;

  for (const lint of candidates) {
    const first = lint.suggestions[0];
    if (!first) continue;
    const next = applySingleFix(current, lint, first);
    if (next !== null) {
      current = next;
      appliedCount += 1;
    }
  }

  return { text: current, appliedCount };
}

export function pushUndo(stack: readonly string[], previousText: string): string[] {
  return [...stack, previousText];
}

export function popUndo(stack: readonly string[]): {
  previousText: string | null;
  nextStack: string[];
} {
  if (stack.length === 0) {
    return { previousText: null, nextStack: [] };
  }
  const previousText = stack[stack.length - 1] ?? null;
  return { previousText, nextStack: stack.slice(0, -1) };
}

export function sliceVisibleCards(
  lints: LintDto[],
  ignoredIds: ReadonlySet<string>,
  limit = MAX_VISIBLE_LINT_CARDS,
): { visible: LintDto[]; overflowCount: number; fixableCount: number } {
  const active = lints.filter((l) => !ignoredIds.has(l.id));
  const fixableCount = active.filter((l) => l.suggestions.length > 0).length;
  const visible = active.slice(0, limit);
  const overflowCount = Math.max(0, active.length - limit);
  return { visible, overflowCount, fixableCount };
}

export interface TextSegment {
  text: string;
  lint: LintDto | null;
}

/** Splits `text` into plain and underlined segments based on non-overlapping sorted lints. */
export function buildUnderlinedSegments(
  text: string,
  lints: LintDto[],
  ignoredIds: ReadonlySet<string>,
): TextSegment[] {
  const active = lints
    .filter(
      (l) =>
        !ignoredIds.has(l.id) &&
        l.start >= 0 &&
        l.end > l.start &&
        l.end <= text.length &&
        text.slice(l.start, l.end) === l.flagged,
    )
    .sort((a, b) => a.start - b.start);

  const segments: TextSegment[] = [];
  let cursor = 0;

  for (const lint of active) {
    if (lint.start < cursor) continue;
    if (lint.start > cursor) {
      segments.push({ text: text.slice(cursor, lint.start), lint: null });
    }
    segments.push({ text: text.slice(lint.start, lint.end), lint });
    cursor = lint.end;
  }

  if (cursor < text.length) {
    segments.push({ text: text.slice(cursor), lint: null });
  }

  return segments;
}
