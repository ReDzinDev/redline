import { describe, expect, it } from "vitest";
import {
  applyAllFixes,
  applySingleFix,
  buildUnderlinedSegments,
  isLatestLintResponse,
  popUndo,
  pushUndo,
  sliceVisibleCards,
  type LintDto,
} from "./grammarLogic";

function makeLint(overrides: Partial<LintDto>): LintDto {
  return {
    id: "1",
    start: 0,
    end: 3,
    flagged: "Teh",
    category: "spelling",
    kind: "Spelling",
    message: "Did you mean The?",
    suggestions: [{ label: "The", replacement: "The" }],
    ...overrides,
  };
}

describe("applySingleFix (flagged guard)", () => {
  it("replaces span when text slice matches flagged", () => {
    const lint = makeLint({ start: 0, end: 3, flagged: "Teh" });
    expect(applySingleFix("Teh cat sat.", lint, lint.suggestions[0]!)).toBe("The cat sat.");
  });

  it("refuses to corrupt text if span no longer matches flagged (stale or double-click)", () => {
    const lint = makeLint({ start: 0, end: 3, flagged: "Teh" });
    expect(applySingleFix("The cat sat.", lint, lint.suggestions[0]!)).toBeNull();
  });

  it("handles UTF-16 offsets around emoji accurately", () => {
    const text = "🚀 Teh cat";
    // 🚀 is 2 UTF-16 code units, space is index 2, "Teh" is 3..6
    const lint = makeLint({ start: 3, end: 6, flagged: "Teh" });
    expect(applySingleFix(text, lint, lint.suggestions[0]!)).toBe("🚀 The cat");
  });
});

describe("applyAllFixes (reverse span ordering & ignore support)", () => {
  it("applies first suggestion from last span to first without offset drift", () => {
    const text = "Teh quik fox jumpd.";
    const lints: LintDto[] = [
      makeLint({
        id: "a",
        start: 0,
        end: 3,
        flagged: "Teh",
        suggestions: [{ label: "The", replacement: "The" }],
      }),
      makeLint({
        id: "b",
        start: 4,
        end: 8,
        flagged: "quik",
        suggestions: [{ label: "quick", replacement: "quick" }],
      }),
      makeLint({
        id: "c",
        start: 13,
        end: 18,
        flagged: "jumpd",
        suggestions: [{ label: "jumped", replacement: "jumped" }],
      }),
    ];

    const res = applyAllFixes(text, lints, new Set(["b"]));
    expect(res.appliedCount).toBe(2);
    expect(res.text).toBe("The quik fox jumped.");
  });
});

describe("undo stack, stale lint responses, card cap, and segmentation", () => {
  it("pushes and pops undo entries cleanly", () => {
    const s1 = pushUndo([], "original");
    const s2 = pushUndo(s1, "after apply 1");
    const firstPop = popUndo(s2);
    expect(firstPop.previousText).toBe("after apply 1");
    const secondPop = popUndo(firstPop.nextStack);
    expect(secondPop.previousText).toBe("original");
    expect(popUndo(secondPop.nextStack).previousText).toBeNull();
  });

  it("drops stale out-of-order lint responses", () => {
    expect(isLatestLintResponse(4, 5)).toBe(false);
    expect(isLatestLintResponse(5, 5)).toBe(true);
  });

  it("caps visible cards at 20 and reports overflow count", () => {
    const many = Array.from({ length: 25 }, (_, i) => makeLint({ id: String(i) }));
    const { visible, overflowCount, fixableCount } = sliceVisibleCards(many, new Set(["0"]));
    expect(visible).toHaveLength(20);
    expect(overflowCount).toBe(4);
    expect(fixableCount).toBe(24);
  });

  it("segments text with distinct underline lints", () => {
    const lint = makeLint({ start: 0, end: 3, flagged: "Teh" });
    const segments = buildUnderlinedSegments("Teh cat", [lint], new Set());
    expect(segments).toEqual([
      { text: "Teh", lint },
      { text: " cat", lint: null },
    ]);
  });
});
