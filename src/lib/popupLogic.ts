/**
 * Pure state-transition and formatting helpers for the inline popup.
 * Kept separate from React/Tauri so Vitest can test every rule directly.
 */
export type AiRunStatus = "idle" | "streaming" | "complete" | "cut_off" | "error";

export interface EnterContext {
  phase: "capturing" | "ready" | "error";
  aiStatus: AiRunStatus;
  aiUnchanged: boolean;
  harperFixesApplied: boolean;
}

export type EnterOutcome = "paste_ai" | "paste_fixed" | "close" | "noop";

/**
 * Implements the "What Enter does" table from section 4 of the Phase 1 spec:
 * - Capturing or capture error -> noop
 * - Streaming -> noop ("Wait for the result or press Esc")
 * - Error or cut-off AI result -> noop (never falls back to pasting Harper-fixed text)
 * - Complete AI result, different from input -> paste_ai
 * - Complete AI result, same as input -> close ("No changes")
 * - No AI result, Harper fixes applied -> paste_fixed
 * - Nothing changed -> close
 */
export function resolveEnterAction(ctx: EnterContext): EnterOutcome {
  if (ctx.phase !== "ready") {
    return "noop";
  }

  switch (ctx.aiStatus) {
    case "streaming":
    case "error":
    case "cut_off":
      return "noop";
    case "complete":
      return ctx.aiUnchanged ? "close" : "paste_ai";
    case "idle":
      return ctx.harperFixesApplied ? "paste_fixed" : "close";
  }
}

/** True when the event target is a text-editing control that owns printable keys. */
export function isEditableTarget(tagName: string | undefined, isContentEditable: boolean): boolean {
  if (isContentEditable) return true;
  if (!tagName) return false;
  const upper = tagName.toUpperCase();
  return upper === "INPUT" || upper === "TEXTAREA" || upper === "SELECT";
}

/** True when Enter should be left to the focused interactive element instead of global accept. */
export function shouldIgnoreGlobalEnter(
  tagName: string | undefined,
  isContentEditable: boolean,
  isComposing: boolean,
): boolean {
  if (isComposing) return true;
  if (isEditableTarget(tagName, isContentEditable)) return true;
  if (!tagName) return false;
  const upper = tagName.toUpperCase();
  return upper === "BUTTON" || upper === "A";
}

export function formatCostUsd(costUsd: number | null): string {
  if (costUsd === null || Number.isNaN(costUsd)) return "";
  if (costUsd === 0) return "$0";
  if (costUsd < 0.0001) return `$${costUsd.toFixed(6).replace(/0+$/, "")}`;
  if (costUsd < 0.01) return `$${costUsd.toFixed(5).replace(/0+$/, "")}`;
  return `$${costUsd.toFixed(4).replace(/0+$/, "")}`;
}

export function formatElapsedSeconds(elapsedMs: number | null): string {
  if (elapsedMs === null || elapsedMs < 0) return "";
  return `${(elapsedMs / 1000).toFixed(1)} s`;
}

export function formatSourceAppLabel(processName: string): string {
  const cleaned = processName.replace(/\.exe$/i, "").trim();
  if (!cleaned || cleaned.toLowerCase() === "unknown") return "";
  return cleaned.charAt(0).toUpperCase() + cleaned.slice(1);
}
