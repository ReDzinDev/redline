import { useCallback, useEffect, useRef, useState } from "react";
import { ActionBar } from "../components/ActionBar";
import { ResultPanel, type ResultState } from "../components/ResultPanel";
import {
  acceptPopupResult,
  cancelRewrite,
  copyToClipboard,
  dismissPopup,
  isApiError,
  listenPopupSession,
  openInEditor,
  openSettings,
  resizePopup,
  rewriteText,
  type ActionKind,
  type ActionRequestDto,
  type ToneOption,
} from "../lib/ipc";
import { formatCaptureError, MESSAGES } from "../lib/messages";
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
  const [text, setText] = useState("");
  const [captureErrorCode, setCaptureErrorCode] = useState<string | null>(null);
  const [recentCustom, setRecentCustom] = useState<string[]>([]);
  const [subMode, setSubMode] = useState<"none" | "tone" | "custom">("none");
  const [customInput, setCustomInput] = useState("");
  const [activeAction, setActiveAction] = useState<ActionRequestDto | null>(null);
  const [result, setResult] = useState<ResultState>(INITIAL_RESULT);
  const [copied, setCopied] = useState(false);
  const [liveAnnouncement, setLiveAnnouncement] = useState("");
  const [fallbackNotice, setFallbackNotice] = useState<string | null>(null);

  const requestIdRef = useRef(0);
  const expandedRef = useRef(false);

  const runAction = useCallback((currentText: string, req: ActionRequestDto) => {
    const nextRequestId = ++requestIdRef.current;
    const actionLabel = labelForAction(req);
    setActiveAction(req);
    setSubMode("none");
    setCopied(false);
    setLiveAnnouncement("");
    setResult({
      ...INITIAL_RESULT,
      status: "streaming",
      actionLabel,
    });

    rewriteText(nextRequestId, currentText, req, (evt) => {
      if (evt.requestId !== requestIdRef.current) return;

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
      if (nextRequestId !== requestIdRef.current) return;
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
      requestIdRef.current += 1;
      setCopied(false);
      setFallbackNotice(null);
      setLiveAnnouncement("");
      setSubMode("none");
      setCustomInput("");
      setActiveAction(null);
      setResult(INITIAL_RESULT);

      if (evt.phase === "capturing") {
        setPhase("capturing");
        setSourceApp(evt.sourceApp);
        setText("");
        setCaptureErrorCode(null);
      } else if (evt.phase === "ready") {
        setPhase("ready");
        setSourceApp(evt.sourceApp);
        setText(evt.text);
        setCaptureErrorCode(null);
        setRecentCustom(evt.recentCustomInstructions);

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
  }, [runAction]);

  // Switch between compact (460x320) and expanded (460x520) only on discrete state transitions.
  const shouldExpand = result.status !== "idle" || subMode !== "none";
  useEffect(() => {
    if (expandedRef.current === shouldExpand) return;
    expandedRef.current = shouldExpand;
    resizePopup(shouldExpand).catch(() => {});
  }, [shouldExpand]);

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

  const handleAcceptText = useCallback(async (textToPaste: string) => {
    try {
      const res = await acceptPopupResult(textToPaste);
      if (res.outcome === "copied_fallback") {
        setFallbackNotice(MESSAGES.popup.copiedFallbackNotice);
      }
    } catch {
      // Errors logged in Rust; popup stays clean.
    }
  }, []);

  const handleCopy = useCallback(async (value: string) => {
    try {
      await copyToClipboard(value);
      setCopied(true);
    } catch {
      // Ignore copy failure
    }
  }, []);

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

      if (e.key === "Enter") {
        if (shouldIgnoreGlobalEnter(tagName, isContentEditable, e.isComposing)) {
          return;
        }
        const action = resolveEnterAction({
          phase,
          aiStatus: result.status,
          aiUnchanged: result.unchanged,
          harperFixesApplied: false,
        });
        if (action === "noop") return;
        e.preventDefault();
        if (action === "paste_ai") {
          void handleAcceptText(result.text);
        } else if (action === "paste_fixed") {
          void handleAcceptText(text);
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

      const matchedAction = MESSAGES.actions.find((a) => a.key === e.key);
      if (matchedAction) {
        e.preventDefault();
        handleSelectAction(matchedAction.kind);
      }
    }

    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [
    handleAcceptText,
    handleSelectAction,
    handleSelectTone,
    phase,
    result.status,
    result.text,
    result.unchanged,
    subMode,
    text,
  ]);

  const enterOutcome = resolveEnterAction({
    phase,
    aiStatus: result.status,
    aiUnchanged: result.unchanged,
    harperFixesApplied: false,
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
    <main className="flex h-full flex-col gap-3 border border-line bg-canvas p-3 text-ink select-none">
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
          <button
            type="button"
            onClick={() => openInEditor().catch(() => {})}
            className="rounded-md bg-accent px-3 py-1.5 text-xs font-medium text-accent-contrast transition hover:bg-accent-hover focus-visible:ring-2 focus-visible:ring-accent"
          >
            {MESSAGES.popup.openInEditor}
          </button>
        </div>
      )}

      {phase === "ready" && (
        <div className="flex min-h-0 flex-1 flex-col gap-2.5">
          <div className="max-h-24 overflow-y-auto rounded-md border border-line bg-elevated px-2.5 py-2 text-xs leading-relaxed whitespace-pre-wrap text-ink select-text">
            {text}
          </div>

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
            copied={copied}
            onRetry={() => {
              if (activeAction) runAction(text, activeAction);
            }}
            onCopy={(val) => {
              void handleCopy(val);
            }}
            onOpenSettings={() => {
              openSettings().catch(() => {});
            }}
          />

          {fallbackNotice && (
            <p role="status" className="text-xs font-medium text-status-info">
              {fallbackNotice}
            </p>
          )}
        </div>
      )}

      <footer className="flex items-center justify-between gap-2 border-t border-line pt-2 text-[11px] text-ink-muted">
        <span>{hintText}</span>
        <div className="flex items-center gap-2">
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
