import { useCallback, useEffect, useRef, useState } from "react";
import { ActionBar } from "../components/ActionBar";
import { LintCardList } from "../components/LintCardList";
import { ResultPanel, type ResultState } from "../components/ResultPanel";
import {
  applyAllFixes,
  applySingleFix,
  isLatestLintResponse,
  popUndo,
  pushUndo,
  type LintDto,
  type SuggestionDto,
} from "../lib/grammarLogic";
import {
  cancelRewrite,
  copyToClipboard,
  isApiError,
  lintText,
  openExternalUrl,
  recordHistoryEntry,
  rewriteText,
  type ActionKind,
  type ActionRequestDto,
  type ToneOption,
} from "../lib/ipc";
import { MESSAGES } from "../lib/messages";
import { formatSourceAppLabel } from "../lib/popupLogic";

const EDITOR_MAX_CHARS = 40_000;
const LINT_DEBOUNCE_MS = 300;

const INITIAL_RESULT: ResultState = {
  status: "idle",
  actionLabel: "",
  text: "",
  partialText: "",
  modelUsed: null,
  costUsd: null,
  elapsedMs: null,
  unchanged: false,
  error: null,
  cutOffDetail: null,
};

function labelForAction(req: ActionRequestDto): string {
  if (req.kind === "change_tone" && req.tone) {
    const toneObj = MESSAGES.tones.find((t) => t.tone === req.tone);
    return toneObj ? `Tone: ${toneObj.label}` : "Tone";
  }
  if (req.kind === "custom") return "Custom";
  return MESSAGES.actions.find((a) => a.kind === req.kind)?.label ?? req.kind;
}

interface WriteTabProps {
  text: string;
  sourceApp: string | null;
  recentCustom: string[];
  onTextChange: (next: string) => void;
  onRecentCustomChange: (next: string[]) => void;
  onOpenSettings: () => void;
}

export function WriteTab({
  text,
  sourceApp,
  recentCustom,
  onTextChange,
  onRecentCustomChange,
  onOpenSettings,
}: WriteTabProps) {
  const [lintLoading, setLintLoading] = useState(false);
  const [lintEnglish, setLintEnglish] = useState(true);
  const [lints, setLints] = useState<LintDto[]>([]);
  const [ignoredIds, setIgnoredIds] = useState<Set<string>>(() => new Set());
  const [undoStack, setUndoStack] = useState<string[]>([]);
  const [subMode, setSubMode] = useState<"none" | "tone" | "custom">("none");
  const [customInput, setCustomInput] = useState("");
  const [activeAction, setActiveAction] = useState<ActionRequestDto | null>(null);
  const [result, setResult] = useState<ResultState>(INITIAL_RESULT);
  const [showDiff, setShowDiff] = useState(false);
  const [copied, setCopied] = useState(false);
  const [liveAnnouncement, setLiveAnnouncement] = useState("");

  const textareaRef = useRef<HTMLTextAreaElement | null>(null);
  const lintReqIdRef = useRef(0);
  const aiReqIdRef = useRef(0);

  // Debounced 300ms Harper linting with stale-response drop.
  useEffect(() => {
    const trimmed = text.trim();
    if (!trimmed) {
      lintReqIdRef.current += 1;
      return;
    }

    const timer = window.setTimeout(() => {
      const reqId = ++lintReqIdRef.current;
      setLintLoading(true);
      lintText(text, reqId)
        .then((res) => {
          if (!isLatestLintResponse(res.requestId, lintReqIdRef.current)) return;
          setLintEnglish(res.english);
          setLints(res.lints);
          setLintLoading(false);
        })
        .catch(() => {
          if (reqId !== lintReqIdRef.current) return;
          setLintLoading(false);
        });
    }, LINT_DEBOUNCE_MS);

    return () => window.clearTimeout(timer);
  }, [text]);

  const hasText = text.trim().length > 0;
  const effectiveLints = hasText ? lints : [];
  const effectiveEnglish = hasText ? lintEnglish : true;
  const effectiveLoading = hasText ? lintLoading : false;

  const clearAiResult = useCallback(() => {
    aiReqIdRef.current += 1;
    cancelRewrite().catch(() => {});
    setActiveAction(null);
    setResult(INITIAL_RESULT);
    setShowDiff(false);
    setCopied(false);
    setLiveAnnouncement("");
  }, []);

  const runAction = useCallback((currentText: string, req: ActionRequestDto) => {
    if (currentText.trim().length === 0 || currentText.length > EDITOR_MAX_CHARS) return;
    const nextRequestId = ++aiReqIdRef.current;
    const actionLabel = labelForAction(req);
    setActiveAction(req);
    setSubMode("none");
    setShowDiff(false);
    setCopied(false);
    setLiveAnnouncement("");
    setResult({
      ...INITIAL_RESULT,
      status: "streaming",
      actionLabel,
    });

    rewriteText(nextRequestId, currentText, req, (evt) => {
      if (evt.requestId !== aiReqIdRef.current) return;

      switch (evt.event) {
        case "delta":
          setResult((prev) => ({ ...prev, text: prev.text + evt.text }));
          break;
        case "complete":
          setResult((prev) => ({
            ...prev,
            status: "complete",
            text: evt.text,
            modelUsed: evt.modelUsed,
            costUsd: evt.costUsd,
            elapsedMs: evt.elapsedMs,
            unchanged: evt.unchanged,
          }));
          setLiveAnnouncement(MESSAGES.popup.resultReadyAnnouncement);
          break;
        case "cutOff":
          setResult((prev) => ({
            ...prev,
            status: "cut_off",
            partialText: evt.partialText || prev.text,
            cutOffDetail: evt.detail,
          }));
          break;
        case "error":
          setResult((prev) => ({ ...prev, status: "error", error: evt.error }));
          break;
      }
    }).catch((err: unknown) => {
      if (nextRequestId !== aiReqIdRef.current) return;
      const apiErr = isApiError(err)
        ? err
        : { code: "ai_unexpected", detail: String(err), retryAfterSecs: null };
      setResult((prev) => ({ ...prev, status: "error", error: apiErr }));
    });
  }, []);

  const handleSelectAction = useCallback(
    (kind: ActionKind) => {
      if (kind === "change_tone") {
        setSubMode((prev) => (prev === "tone" ? "none" : "tone"));
        return;
      }
      if (kind === "custom") {
        setSubMode((prev) => (prev === "custom" ? "none" : "custom"));
        return;
      }
      runAction(text, { kind });
    },
    [runAction, text],
  );

  const handleSelectTone = useCallback(
    (tone: ToneOption) => {
      runAction(text, { kind: "change_tone", tone });
    },
    [runAction, text],
  );

  const handleRunCustom = useCallback(
    (instruction: string) => {
      onRecentCustomChange(
        [
          instruction,
          ...recentCustom.filter((item) => item.toLowerCase() !== instruction.toLowerCase()),
        ].slice(0, 5),
      );
      runAction(text, { kind: "custom", customInstruction: instruction });
    },
    [onRecentCustomChange, recentCustom, runAction, text],
  );

  const handleApplyLint = useCallback(
    (lint: LintDto, suggestion: SuggestionDto) => {
      const nextText = applySingleFix(text, lint, suggestion);
      if (nextText === null || nextText === text) return;
      setUndoStack((prev) => pushUndo(prev, text));
      onTextChange(nextText);
      if (result.status !== "idle") clearAiResult();
    },
    [clearAiResult, onTextChange, result.status, text],
  );

  const handleFixAll = useCallback(() => {
    const { text: nextText, appliedCount } = applyAllFixes(text, lints, ignoredIds);
    if (appliedCount === 0 || nextText === text) return;
    setUndoStack((prev) => pushUndo(prev, text));
    onTextChange(nextText);
    if (result.status !== "idle") clearAiResult();
  }, [clearAiResult, ignoredIds, lints, onTextChange, result.status, text]);

  const handleUndo = useCallback(() => {
    const { previousText, nextStack } = popUndo(undoStack);
    if (previousText === null) return;
    setUndoStack(nextStack);
    onTextChange(previousText);
    if (result.status !== "idle") clearAiResult();
  }, [clearAiResult, onTextChange, result.status, undoStack]);

  const handleSelectCard = useCallback((lint: LintDto) => {
    const el = textareaRef.current;
    if (!el) return;
    el.focus();
    el.setSelectionRange(lint.start, lint.end);
  }, []);

  const handleUseResult = useCallback(
    (nextText: string) => {
      setUndoStack((prev) => pushUndo(prev, text));
      recordHistoryEntry({
        action: result.actionLabel || "AI rewrite",
        sourceApp,
        model: result.modelUsed ?? "OpenRouter",
        costUsd: result.costUsd,
        original: text,
        result: nextText,
      }).catch(() => {});
      onTextChange(nextText);
      clearAiResult();
    },
    [
      clearAiResult,
      onTextChange,
      result.actionLabel,
      result.costUsd,
      result.modelUsed,
      sourceApp,
      text,
    ],
  );

  // In Write tab, digits/letters type normally; actions trigger on Ctrl+1..Ctrl+6.
  useEffect(() => {
    function onKeyDown(e: KeyboardEvent) {
      if (!(e.ctrlKey || e.metaKey) || e.altKey) return;
      const matched = MESSAGES.actions.find((a) => a.key === e.key);
      if (matched) {
        e.preventDefault();
        handleSelectAction(matched.kind);
      }
    }
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [handleSelectAction]);

  const overLimit = text.length > EDITOR_MAX_CHARS;
  const appLabel = sourceApp ? formatSourceAppLabel(sourceApp) : "";

  return (
    <div className="flex min-h-0 flex-1 flex-col gap-3">
      <div className="grid min-h-0 flex-1 grid-cols-1 gap-3 md:grid-cols-[1fr_280px]">
        <div className="flex min-h-0 flex-col gap-2">
          <div className="flex items-center justify-between gap-2 text-xs text-ink-muted">
            <span>{appLabel ? MESSAGES.write.fromApp(appLabel) : ""}</span>
            <div className="flex items-center gap-2">
              <span className={overLimit ? "text-status-error" : ""}>
                {text.length.toLocaleString()} / {EDITOR_MAX_CHARS.toLocaleString()}
              </span>
              {text.length > 0 && (
                <button
                  type="button"
                  onClick={() => {
                    onTextChange("");
                    clearAiResult();
                    setIgnoredIds(new Set());
                    setUndoStack([]);
                  }}
                  className="rounded-sm px-1.5 py-0.5 text-ink-secondary hover:bg-hover focus-visible:ring-2 focus-visible:ring-accent"
                >
                  {MESSAGES.write.clear}
                </button>
              )}
            </div>
          </div>

          <textarea
            ref={textareaRef}
            value={text}
            onChange={(e) => onTextChange(e.target.value)}
            placeholder={MESSAGES.write.placeholder}
            className="min-h-40 flex-1 resize-none rounded-md border border-line-strong bg-canvas p-3 text-sm leading-relaxed text-ink outline-none focus-visible:ring-2 focus-visible:ring-accent"
          />

          {overLimit && (
            <p role="alert" className="text-xs text-status-error">
              {MESSAGES.write.charLimitExceeded}
            </p>
          )}
        </div>

        <aside className="min-h-0 overflow-y-auto rounded-md border border-line bg-elevated p-3">
          <LintCardList
            loading={effectiveLoading}
            english={effectiveEnglish}
            lints={effectiveLints}
            ignoredIds={ignoredIds}
            canUndo={undoStack.length > 0}
            onApply={handleApplyLint}
            onIgnore={(id) =>
              setIgnoredIds((prev) => {
                const next = new Set(prev);
                next.add(id);
                return next;
              })
            }
            onFixAll={handleFixAll}
            onUndo={handleUndo}
            onSelectCard={handleSelectCard}
          />
          {!effectiveLoading && effectiveEnglish && effectiveLints.length === 0 && (
            <p className="text-xs text-ink-muted">{MESSAGES.grammar.noIssues}</p>
          )}
        </aside>
      </div>

      <ActionBar
        activeKind={activeAction?.kind ?? null}
        subMode={subMode}
        customInput={customInput}
        recentCustom={recentCustom}
        disabled={text.trim().length === 0 || overLimit}
        keyPrefix="Ctrl+"
        onSelectAction={handleSelectAction}
        onSelectTone={handleSelectTone}
        onCustomInputChange={setCustomInput}
        onRunCustom={handleRunCustom}
      />

      <ResultPanel
        result={result}
        originalText={text}
        showDiff={showDiff}
        copied={copied}
        onToggleDiff={() => setShowDiff((prev) => !prev)}
        onRetry={() => {
          if (activeAction) runAction(text, activeAction);
        }}
        onCopy={(val) => {
          copyToClipboard(val)
            .then(() => setCopied(true))
            .catch(() => {});
        }}
        onOpenSettings={onOpenSettings}
        onOpenCredits={() => {
          openExternalUrl("openrouter_credits").catch(() => {});
        }}
        onUseResult={handleUseResult}
      />

      <span className="sr-only" aria-live="polite">
        {liveAnnouncement}
      </span>
    </div>
  );
}
