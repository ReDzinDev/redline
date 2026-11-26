import { useCallback, useEffect, useRef, useState } from "react";
import { ActionBar } from "../components/ActionBar";
import { LintCardList } from "../components/LintCardList";
import { ResultPanel, type ResultState } from "../components/ResultPanel";
import { UnderlinedText } from "../components/UnderlinedText";
import {
  applyAllFixes,
  applySingleFix,
  isLatestLintResponse,
  popUndo,
  pushUndo,
  sliceVisibleCards,
  type LintDto,
  type SuggestionDto,
} from "../lib/grammarLogic";
import {
  acceptPopupResult,
  cancelRewrite,
  copyToClipboard,
  dismissPopup,
  isApiError,
  lintText,
  listenPopupSession,
  openInEditor,
  openSettings,
  resizePopup,
  rewriteText,
  type AcceptHistoryMetaDto,
  type ActionKind,
  type ActionRequestDto,
  type ToneOption,
} from "../lib/ipc";
import { formatApiError, formatCaptureError, MESSAGES } from "../lib/messages";
import {
  formatSourceAppLabel,
  isEditableTarget,
  resolveEnterAction,
  shouldIgnoreGlobalEnter,
} from "../lib/popupLogic";

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
  if (req.kind === "custom") {
    return "Custom";
  }
  return MESSAGES.actions.find((a) => a.kind === req.kind)?.label ?? req.kind;
}

export function PopupApp() {
  const [phase, setPhase] = useState<"capturing" | "ready" | "error">("capturing");
  const [sourceApp, setSourceApp] = useState("");
  const [initialText, setInitialText] = useState("");
  const [text, setText] = useState("");
  const [captureErrorCode, setCaptureErrorCode] = useState<string | null>(null);
  const [recentCustom, setRecentCustom] = useState<string[]>([]);
  const [subMode, setSubMode] = useState<"none" | "tone" | "custom">("none");
  const [customInput, setCustomInput] = useState("");
  const [activeAction, setActiveAction] = useState<ActionRequestDto | null>(null);
  const [result, setResult] = useState<ResultState>(INITIAL_RESULT);
  const [showDiff, setShowDiff] = useState(false);
  const [copied, setCopied] = useState(false);
  const [liveAnnouncement, setLiveAnnouncement] = useState("");
  const [fallbackNotice, setFallbackNotice] = useState<{
    kind: "info" | "error";
    text: string;
  } | null>(null);

  // Harper state
  const [lintLoading, setLintLoading] = useState(false);
  const [lintEnglish, setLintEnglish] = useState(true);
  const [lints, setLints] = useState<LintDto[]>([]);
  const [ignoredIds, setIgnoredIds] = useState<Set<string>>(() => new Set());
  const [undoStack, setUndoStack] = useState<string[]>([]);
  const [highlightedLintId, setHighlightedLintId] = useState<string | null>(null);
  const [lintsExpandedDuringResult, setLintsExpandedDuringResult] = useState(false);

  const aiRequestIdRef = useRef(0);
  const lintRequestIdRef = useRef(0);
  const expandedRef = useRef(false);

  const runLint = useCallback((targetText: string) => {
    const reqId = ++lintRequestIdRef.current;
    setLintLoading(true);
    lintText(targetText, reqId)
      .then((res) => {
        if (!isLatestLintResponse(res.requestId, lintRequestIdRef.current)) return;
        setLintEnglish(res.english);
        setLints(res.lints);
        setLintLoading(false);
      })
      .catch(() => {
        if (reqId !== lintRequestIdRef.current) return;
        setLintLoading(false);
      });
  }, []);

  const clearAiResult = useCallback(() => {
    aiRequestIdRef.current += 1;
    cancelRewrite().catch(() => {});
    setActiveAction(null);
    setResult(INITIAL_RESULT);
    setShowDiff(false);
    setCopied(false);
    setLiveAnnouncement("");
  }, []);

  const runAction = useCallback((currentText: string, req: ActionRequestDto) => {
    const nextRequestId = ++aiRequestIdRef.current;
    const actionLabel = labelForAction(req);
    setActiveAction(req);
    setSubMode("none");
    setShowDiff(false);
    setCopied(false);
    setLiveAnnouncement("");
    setLintsExpandedDuringResult(false);
    setResult({
      ...INITIAL_RESULT,
      status: "streaming",
      actionLabel,
    });

    rewriteText(nextRequestId, currentText, req, (evt) => {
      if (evt.requestId !== aiRequestIdRef.current) return;

      switch (evt.event) {
        case "delta":
          setResult((prev) => ({
            ...prev,
            text: prev.text + evt.text,
          }));
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
          setResult((prev) => ({
            ...prev,
            status: "error",
            error: evt.error,
          }));
          break;
      }
    }).catch((err: unknown) => {
      if (nextRequestId !== aiRequestIdRef.current) return;
      const apiErr = isApiError(err)
        ? err
        : { code: "ai_unexpected", detail: String(err), retryAfterSecs: null };
      setResult((prev) => ({
        ...prev,
        status: "error",
        error: apiErr,
      }));
    });
  }, []);

  useEffect(() => {
    let unlisten: (() => void) | null = null;
    listenPopupSession((evt) => {
      aiRequestIdRef.current += 1;
      lintRequestIdRef.current += 1;
      setCopied(false);
      setShowDiff(false);
      setFallbackNotice(null);
      setLiveAnnouncement("");
      setSubMode("none");
      setCustomInput("");
      setActiveAction(null);
      setResult(INITIAL_RESULT);
      setLints([]);
      setLintEnglish(true);
      setLintLoading(false);
      setIgnoredIds(new Set());
      setUndoStack([]);
      setHighlightedLintId(null);
      setLintsExpandedDuringResult(false);

      if (evt.phase === "capturing") {
        expandedRef.current = false;
        setPhase("capturing");
        setSourceApp(evt.sourceApp);
        setInitialText("");
        setText("");
        setCaptureErrorCode(null);
      } else if (evt.phase === "ready") {
        setPhase("ready");
        setSourceApp(evt.sourceApp);
        setInitialText(evt.text);
        setText(evt.text);
        setCaptureErrorCode(null);
        setRecentCustom(evt.recentCustomInstructions);

        runLint(evt.text);

        if (
          evt.autoRunAction &&
          (evt.autoRunAction === "improve" ||
            evt.autoRunAction === "fix_grammar" ||
            evt.autoRunAction === "shorten" ||
            evt.autoRunAction === "make_clearer")
        ) {
          runAction(evt.text, { kind: evt.autoRunAction });
        }
      } else {
        setPhase("error");
        setSourceApp(evt.sourceApp);
        setInitialText("");
        setText("");
        setCaptureErrorCode(evt.errorCode);
      }
    })
      .then((fn) => {
        unlisten = fn;
      })
      .catch(() => {});

    return () => {
      if (unlisten) unlisten();
    };
  }, [runAction, runLint]);

  const { visible: visibleLints, fixableCount } = sliceVisibleCards(lints, ignoredIds);
  const shouldExpand = result.status !== "idle" || subMode !== "none" || visibleLints.length >= 2;

  useEffect(() => {
    if (expandedRef.current === shouldExpand) return;
    expandedRef.current = shouldExpand;
    resizePopup(shouldExpand).catch(() => {});
  }, [shouldExpand]);

  const handleApplyLint = useCallback(
    (lint: LintDto, suggestion: SuggestionDto) => {
      const nextText = applySingleFix(text, lint, suggestion);
      if (nextText === null || nextText === text) return;
      setUndoStack((prev) => pushUndo(prev, text));
      setText(nextText);
      if (result.status !== "idle") {
        clearAiResult();
      }
      runLint(nextText);
    },
    [clearAiResult, result.status, runLint, text],
  );

  const handleFixAll = useCallback(() => {
    const { text: nextText, appliedCount } = applyAllFixes(text, lints, ignoredIds);
    if (appliedCount === 0 || nextText === text) return;
    setUndoStack((prev) => pushUndo(prev, text));
    setText(nextText);
    if (result.status !== "idle") {
      clearAiResult();
    }
    runLint(nextText);
  }, [clearAiResult, ignoredIds, lints, result.status, runLint, text]);

  const handleUndo = useCallback(() => {
    const { previousText, nextStack } = popUndo(undoStack);
    if (previousText === null) return;
    setUndoStack(nextStack);
    setText(previousText);
    if (result.status !== "idle") {
      clearAiResult();
    }
    runLint(previousText);
  }, [clearAiResult, result.status, runLint, undoStack]);

  const handleIgnoreLint = useCallback((lintId: string) => {
    setIgnoredIds((prev) => {
      const next = new Set(prev);
      next.add(lintId);
      return next;
    });
  }, []);

  const handleSelectAction = useCallback(
    (kind: ActionKind) => {
      if (phase !== "ready") return;
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
    [phase, runAction, text],
  );

  const handleSelectTone = useCallback(
    (tone: ToneOption) => {
      if (phase !== "ready") return;
      runAction(text, { kind: "change_tone", tone });
    },
    [phase, runAction, text],
  );

  const handleRunCustom = useCallback(
    (instruction: string) => {
      if (phase !== "ready") return;
      setRecentCustom((prev) =>
        [
          instruction,
          ...prev.filter((item) => item.toLowerCase() !== instruction.toLowerCase()),
        ].slice(0, 5),
      );
      runAction(text, { kind: "custom", customInstruction: instruction });
    },
    [phase, runAction, text],
  );

  const handleAcceptText = useCallback(
    async (textToPaste: string, historyMeta: AcceptHistoryMetaDto) => {
      try {
        const res = await acceptPopupResult(textToPaste, historyMeta);
        if (res.outcome === "copied_fallback") {
          setFallbackNotice({ kind: "info", text: MESSAGES.popup.copiedFallbackNotice });
        }
      } catch (err: unknown) {
        const headline = isApiError(err) ? formatApiError(err).headline : "Couldn't paste text.";
        setFallbackNotice({ kind: "error", text: headline });
      }
    },
    [],
  );

  const handleCopy = useCallback(async (value: string) => {
    try {
      await copyToClipboard(value);
      setCopied(true);
    } catch {
      // Ignore copy failure.
    }
  }, []);

  const harperFixesApplied = text !== initialText;

  useEffect(() => {
    function onKeyDown(e: KeyboardEvent) {
      const target = e.target as HTMLElement | null;
      const tagName = target?.tagName;
      const isContentEditable = target?.isContentEditable ?? false;

      if (e.key === "Escape") {
        e.preventDefault();
        if (subMode !== "none") {
          setSubMode("none");
          return;
        }
        cancelRewrite().catch(() => {});
        dismissPopup(true).catch(() => {});
        return;
      }

      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "e") {
        e.preventDefault();
        openInEditor(phase === "ready" ? text : undefined).catch(() => {});
        return;
      }

      if (
        (e.ctrlKey || e.metaKey) &&
        e.key.toLowerCase() === "z" &&
        !isEditableTarget(tagName, isContentEditable)
      ) {
        if (undoStack.length > 0) {
          e.preventDefault();
          handleUndo();
        }
        return;
      }

      if (e.key === "Enter") {
        if (shouldIgnoreGlobalEnter(tagName, isContentEditable, e.isComposing)) {
          return;
        }
        const action = resolveEnterAction({
          phase,
          aiStatus: result.status,
          aiUnchanged: result.unchanged,
          harperFixesApplied,
        });
        if (action === "noop") return;
        e.preventDefault();
        if (action === "paste_ai") {
          void handleAcceptText(result.text, {
            action: result.actionLabel || "AI rewrite",
            model: result.modelUsed ?? "OpenRouter",
            costUsd: result.costUsd,
            original: initialText,
          });
        } else if (action === "paste_fixed") {
          void handleAcceptText(text, {
            action: MESSAGES.grammar.harperHistoryAction,
            model: MESSAGES.grammar.harperHistoryModel,
            costUsd: 0,
            original: initialText,
          });
        } else if (action === "close") {
          dismissPopup(true).catch(() => {});
        }
        return;
      }

      if (e.ctrlKey || e.altKey || e.metaKey) return;
      if (isEditableTarget(tagName, isContentEditable)) return;
      if (phase !== "ready") return;

      if (subMode === "tone") {
        const matchedTone = MESSAGES.tones.find((t) => t.key === e.key);
        if (matchedTone) {
          e.preventDefault();
          handleSelectTone(matchedTone.tone);
        }
        return;
      }

      if (e.key.toLowerCase() === "d" && result.status === "complete" && !result.unchanged) {
        e.preventDefault();
        setShowDiff((prev) => !prev);
        return;
      }

      if (e.key.toLowerCase() === "f" && fixableCount >= 2) {
        e.preventDefault();
        handleFixAll();
        return;
      }

      const matchedAction = MESSAGES.actions.find((a) => a.key === e.key);
      if (matchedAction) {
        e.preventDefault();
        handleSelectAction(matchedAction.kind);
      }
    }

    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [
    fixableCount,
    handleAcceptText,
    handleFixAll,
    handleSelectAction,
    handleSelectTone,
    handleUndo,
    harperFixesApplied,
    initialText,
    phase,
    result.actionLabel,
    result.costUsd,
    result.modelUsed,
    result.status,
    result.text,
    result.unchanged,
    subMode,
    text,
    undoStack.length,
  ]);

  const enterOutcome = resolveEnterAction({
    phase,
    aiStatus: result.status,
    aiUnchanged: result.unchanged,
    harperFixesApplied,
  });

  const hintText =
    result.status === "streaming"
      ? MESSAGES.popup.hints.streaming
      : result.status === "cut_off" || result.status === "error"
        ? MESSAGES.popup.hints.cutOffOrError
        : enterOutcome === "paste_ai" || enterOutcome === "paste_fixed"
          ? MESSAGES.popup.hints.paste
          : MESSAGES.popup.hints.close;

  const appLabel = formatSourceAppLabel(sourceApp);

  return (
    <main className="flex h-full flex-col gap-2.5 border border-line bg-canvas p-3 text-ink select-none">
      <header className="flex items-center justify-between gap-2 border-b border-line pb-2">
        <span className="truncate text-xs font-semibold text-ink">
          {MESSAGES.popup.headerFrom(appLabel)}
        </span>
        <button
          type="button"
          aria-label={MESSAGES.popup.closeLabel}
          onClick={() => {
            cancelRewrite().catch(() => {});
            dismissPopup(true).catch(() => {});
          }}
          className="rounded-sm px-1.5 py-0.5 text-xs text-ink-muted transition hover:bg-hover hover:text-ink focus-visible:ring-2 focus-visible:ring-accent"
        >
          ×
        </button>
      </header>

      {phase === "capturing" && (
        <div className="flex flex-1 items-center justify-center text-sm text-ink-secondary">
          {MESSAGES.popup.capturing}
        </div>
      )}

      {phase === "error" && (
        <div className="flex flex-1 flex-col items-start justify-center gap-3 p-2">
          <p className="text-sm text-ink">{formatCaptureError(captureErrorCode ?? "")}</p>
          <div className="flex items-center gap-2">
            <button
              type="button"
              onClick={() => openInEditor().catch(() => {})}
              className="rounded-md bg-accent px-3 py-1.5 text-xs font-medium text-accent-contrast transition hover:bg-accent-hover focus-visible:ring-2 focus-visible:ring-accent"
            >
              {MESSAGES.popup.openInEditor}
            </button>
            <button
              type="button"
              onClick={() => dismissPopup(true).catch(() => {})}
              className="rounded-md border border-line-strong bg-canvas px-3 py-1.5 text-xs font-medium text-ink transition hover:bg-hover focus-visible:ring-2 focus-visible:ring-accent"
            >
              {MESSAGES.popup.closeLabel}
            </button>
          </div>
        </div>
      )}

      {phase === "ready" && (
        <div className="flex min-h-0 flex-1 flex-col gap-2 overflow-y-auto pr-0.5">
          <UnderlinedText
            text={text}
            lints={lints}
            ignoredIds={ignoredIds}
            compact={result.status !== "idle"}
            onSelectLint={(lint) => {
              setHighlightedLintId(lint.id);
              if (result.status !== "idle") {
                setLintsExpandedDuringResult(true);
              }
            }}
          />

          <LintCardList
            loading={lintLoading}
            english={lintEnglish}
            lints={lints}
            ignoredIds={ignoredIds}
            canUndo={undoStack.length > 0}
            highlightedLintId={highlightedLintId}
            collapsed={result.status !== "idle" && !lintsExpandedDuringResult}
            onToggleCollapse={
              result.status !== "idle"
                ? () => setLintsExpandedDuringResult((prev) => !prev)
                : undefined
            }
            onApply={handleApplyLint}
            onIgnore={handleIgnoreLint}
            onFixAll={handleFixAll}
            onUndo={handleUndo}
          />

          <ActionBar
            activeKind={activeAction?.kind ?? null}
            subMode={subMode}
            customInput={customInput}
            recentCustom={recentCustom}
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
              void handleCopy(val);
            }}
            onOpenSettings={() => {
              openSettings().catch(() => {});
            }}
            onPasteResult={() => {
              void handleAcceptText(result.text, {
                action: result.actionLabel || "AI rewrite",
                model: result.modelUsed ?? "OpenRouter",
                costUsd: result.costUsd,
                original: initialText,
              });
            }}
          />

          {fallbackNotice && (
            <div
              role={fallbackNotice.kind === "error" ? "alert" : "status"}
              className="flex shrink-0 items-center justify-between gap-2 rounded-md border border-line bg-elevated px-2.5 py-1.5 text-xs"
            >
              <p
                className={`font-medium ${
                  fallbackNotice.kind === "error" ? "text-status-error" : "text-status-info"
                }`}
              >
                {fallbackNotice.text}
              </p>
              <button
                type="button"
                onClick={() => dismissPopup(false).catch(() => {})}
                className="shrink-0 rounded-sm border border-line-strong bg-canvas px-2 py-0.5 text-2xs font-medium text-ink hover:bg-hover focus-visible:ring-2 focus-visible:ring-accent"
              >
                {MESSAGES.popup.closeLabel}
              </button>
            </div>
          )}
        </div>
      )}

      <footer className="flex items-center justify-between gap-2 border-t border-line pt-2 text-2xs text-ink-muted">
        <div className="flex items-center gap-2">
          <span>{hintText}</span>
          {enterOutcome === "paste_fixed" && (
            <button
              type="button"
              onClick={() => {
                void handleAcceptText(text, {
                  action: MESSAGES.grammar.harperHistoryAction,
                  model: MESSAGES.grammar.harperHistoryModel,
                  costUsd: 0,
                  original: initialText,
                });
              }}
              className="rounded-sm bg-accent px-2 py-0.5 text-2xs font-medium text-accent-contrast hover:bg-accent-hover focus-visible:ring-2 focus-visible:ring-accent"
            >
              {MESSAGES.popup.paste}
            </button>
          )}
        </div>
        <div className="flex items-center gap-2">
          {result.status === "complete" && !result.unchanged && (
            <span>{MESSAGES.popup.hints.diff}</span>
          )}
          <span>{MESSAGES.popup.hints.editor}</span>
          <span>{MESSAGES.popup.hints.esc}</span>
        </div>
      </footer>

      <span className="sr-only" aria-live="polite">
        {liveAnnouncement}
      </span>
    </main>
  );
}
