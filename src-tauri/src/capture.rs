use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use thiserror::Error;

use crate::app_info::POPUP_MAX_INPUT_CHARS;
use crate::platform::{clipboard, focus, input, uia};

const MODIFIER_WAIT_TIMEOUT: Duration = Duration::from_millis(300);
const CLIPBOARD_COPY_TIMEOUT: Duration = Duration::from_millis(500);
const WORKER_JOB_TIMEOUT: Duration = Duration::from_secs(3);
const WORKER_SHUTDOWN_TIMEOUT: Duration = Duration::from_millis(1500);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureMethod {
    Uia,
    Clipboard,
}

#[derive(Debug, Clone)]
pub struct CapturedSelection {
    pub target: focus::TargetWindow,
    pub text: String,
    pub method: CaptureMethod,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PasteOutcome {
    Pasted { rendered: bool },
    CopiedFallback,
}

#[derive(Debug, Error)]
pub enum CaptureError {
    #[error("platform worker is busy")]
    Busy,
    #[error("target app is running as administrator")]
    ElevatedTarget,
    #[error("no text selected")]
    NothingSelected,
    #[error("selection exceeds popup character limit ({char_count} chars)")]
    SelectionTooLong { char_count: usize },
    #[error("clipboard is busy")]
    ClipboardBusy,
    #[error("platform worker stopped")]
    WorkerUnavailable,
    #[error("capture failed: {0}")]
    Failed(String),
}

impl CaptureError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Busy => "worker_busy",
            Self::NothingSelected => "nothing_selected",
            Self::ElevatedTarget => "elevated_target",
            Self::SelectionTooLong { .. } => "selection_too_long",
            Self::ClipboardBusy => "clipboard_busy",
            Self::WorkerUnavailable | Self::Failed(_) => "capture_failed",
        }
    }
}

enum WorkerJob {
    Capture {
        target: focus::TargetWindow,
        cancel: Arc<AtomicBool>,
        reply: mpsc::Sender<Result<CapturedSelection, CaptureError>>,
    },
    Paste {
        target: focus::TargetWindow,
        text: String,
        cancel: Arc<AtomicBool>,
        reply: mpsc::Sender<Result<PasteOutcome, CaptureError>>,
    },
    RestoreFocus {
        target: focus::TargetWindow,
        cancel: Arc<AtomicBool>,
        reply: mpsc::Sender<bool>,
    },
    CopyPlain {
        text: String,
        reply: mpsc::Sender<Result<(), CaptureError>>,
    },
    Shutdown {
        reply: mpsc::Sender<()>,
    },
}

/// Ensures `busy` is always released by the worker thread when a job finishes (even on panic),
/// never prematurely by a timed-out caller while a Win32/clipboard operation is still executing.
struct BusyResetGuard(Arc<AtomicBool>);

impl Drop for BusyResetGuard {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

/// Owns the single dedicated MTA COM platform worker thread for UIA capture and clipboard/paste.
pub struct PlatformWorker {
    tx: mpsc::Sender<WorkerJob>,
    busy: Arc<AtomicBool>,
}

impl PlatformWorker {
    pub fn spawn() -> Self {
        let (tx, rx) = mpsc::channel::<WorkerJob>();
        let busy = Arc::new(AtomicBool::new(false));
        let busy_worker = Arc::clone(&busy);

        thread::Builder::new()
            .name("platform-worker".to_string())
            .spawn(move || {
                uia::init_mta_com();
                let uia_reader = uia::UiaReader::new();

                while let Ok(job) = rx.recv() {
                    match job {
                        WorkerJob::Capture {
                            target,
                            cancel,
                            reply,
                        } => {
                            let _guard = BusyResetGuard(Arc::clone(&busy_worker));
                            let res = run_capture(&uia_reader, target, &cancel);
                            let _ = reply.send(res);
                        }
                        WorkerJob::Paste {
                            target,
                            text,
                            cancel,
                            reply,
                        } => {
                            let _guard = BusyResetGuard(Arc::clone(&busy_worker));
                            let res = run_paste(&target, &text, &cancel);
                            let _ = reply.send(res);
                        }
                        WorkerJob::RestoreFocus {
                            target,
                            cancel,
                            reply,
                        } => {
                            let _guard = BusyResetGuard(Arc::clone(&busy_worker));
                            let ok = if cancel.load(Ordering::SeqCst) {
                                false
                            } else {
                                focus::restore_focus(target.hwnd, target.pid)
                            };
                            let _ = reply.send(ok);
                        }
                        WorkerJob::CopyPlain { text, reply } => {
                            let _guard = BusyResetGuard(Arc::clone(&busy_worker));
                            let res =
                                clipboard::write_plain_unicode_text(&text).map_err(map_clip_err);
                            let _ = reply.send(res);
                        }
                        WorkerJob::Shutdown { reply } => {
                            let _ = reply.send(());
                            break;
                        }
                    }
                }
            })
            .expect("failed to spawn platform-worker thread");

        Self { tx, busy }
    }

    pub fn is_busy(&self) -> bool {
        self.busy.load(Ordering::SeqCst)
    }

    /// Captures the current selection from `target`. Ignores overlapping invocations when busy.
    pub fn capture_selection(
        &self,
        target: focus::TargetWindow,
    ) -> Result<CapturedSelection, CaptureError> {
        if self
            .busy
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return Err(CaptureError::Busy);
        }

        let cancel = Arc::new(AtomicBool::new(false));
        let (reply_tx, reply_rx) = mpsc::channel();
        if self
            .tx
            .send(WorkerJob::Capture {
                target,
                cancel: Arc::clone(&cancel),
                reply: reply_tx,
            })
            .is_err()
        {
            self.busy.store(false, Ordering::SeqCst);
            return Err(CaptureError::WorkerUnavailable);
        }

        reply_rx.recv_timeout(WORKER_JOB_TIMEOUT).map_err(|_| {
            // Signal cancellation; do NOT clear `busy` until the worker finishes cleanup.
            cancel.store(true, Ordering::SeqCst);
            CaptureError::Failed("capture timed out".to_string())
        })?
    }

    /// Returns focus to `target` and pastes `text` via delayed rendering with clipboard restore.
    /// If focus cannot be restored or drifts, copies `text` normally so the user can Ctrl+V manually.
    pub fn paste_result(
        &self,
        target: focus::TargetWindow,
        text: String,
    ) -> Result<PasteOutcome, CaptureError> {
        if self
            .busy
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return Err(CaptureError::Busy);
        }

        let cancel = Arc::new(AtomicBool::new(false));
        let (reply_tx, reply_rx) = mpsc::channel();
        if self
            .tx
            .send(WorkerJob::Paste {
                target,
                text,
                cancel: Arc::clone(&cancel),
                reply: reply_tx,
            })
            .is_err()
        {
            self.busy.store(false, Ordering::SeqCst);
            return Err(CaptureError::WorkerUnavailable);
        }

        reply_rx.recv_timeout(WORKER_JOB_TIMEOUT).map_err(|_| {
            // Signal cancellation so a stalled worker aborts before firing late SendInput(Ctrl+V).
            cancel.store(true, Ordering::SeqCst);
            CaptureError::Failed("paste timed out".to_string())
        })?
    }

    /// Returns focus to `target` without touching the clipboard.
    pub fn restore_focus(&self, target: focus::TargetWindow) -> bool {
        if self
            .busy
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return false;
        }

        let cancel = Arc::new(AtomicBool::new(false));
        let (reply_tx, reply_rx) = mpsc::channel();
        if self
            .tx
            .send(WorkerJob::RestoreFocus {
                target,
                cancel: Arc::clone(&cancel),
                reply: reply_tx,
            })
            .is_err()
        {
            self.busy.store(false, Ordering::SeqCst);
            return false;
        }

        reply_rx
            .recv_timeout(Duration::from_secs(1))
            .unwrap_or_else(|_| {
                cancel.store(true, Ordering::SeqCst);
                false
            })
    }

    /// Serializes plain clipboard writes on the platform worker so they never race with an active
    /// `ClipboardSnapshot` capture or delayed-render paste.
    pub fn copy_plain_text(&self, text: String) -> Result<(), CaptureError> {
        if self
            .busy
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return Err(CaptureError::ClipboardBusy);
        }

        let (reply_tx, reply_rx) = mpsc::channel();
        if self
            .tx
            .send(WorkerJob::CopyPlain {
                text,
                reply: reply_tx,
            })
            .is_err()
        {
            self.busy.store(false, Ordering::SeqCst);
            return Err(CaptureError::WorkerUnavailable);
        }

        reply_rx
            .recv_timeout(Duration::from_secs(2))
            .map_err(|_| CaptureError::ClipboardBusy)?
    }

    /// Waits briefly for any active capture/paste job to finish and restore the clipboard before exit.
    pub fn shutdown(&self) {
        let (reply_tx, reply_rx) = mpsc::channel();
        if self
            .tx
            .send(WorkerJob::Shutdown { reply: reply_tx })
            .is_ok()
        {
            let _ = reply_rx.recv_timeout(WORKER_SHUTDOWN_TIMEOUT);
        }
    }
}

fn run_capture(
    uia_reader: &uia::UiaReader,
    target: focus::TargetWindow,
    cancel: &AtomicBool,
) -> Result<CapturedSelection, CaptureError> {
    if target.is_elevated {
        return Err(CaptureError::ElevatedTarget);
    }
    if cancel.load(Ordering::SeqCst) || !focus::is_foreground_target(target.hwnd, target.pid) {
        return Err(CaptureError::NothingSelected);
    }

    // 1. Try UI Automation on the focused element (never touching clipboard) unless skipped.
    if !focus::should_skip_uia(&target.process_name)
        && let Some(text) = uia_reader.read_focused_selection(POPUP_MAX_INPUT_CHARS)
        && !text.is_empty()
    {
        return validate_captured(target, text, CaptureMethod::Uia);
    }

    if cancel.load(Ordering::SeqCst) {
        return Err(CaptureError::Failed("capture cancelled".to_string()));
    }

    // 2. Clipboard fallback with full RAII snapshot and automatic restore.
    input::wait_for_modifiers_released(MODIFIER_WAIT_TIMEOUT);

    // Re-verify foreground target HWND + PID immediately before injecting synthetic Ctrl+C.
    if cancel.load(Ordering::SeqCst) || !focus::is_foreground_target(target.hwnd, target.pid) {
        return Err(CaptureError::NothingSelected);
    }

    let snapshot = clipboard::ClipboardSnapshot::capture().map_err(map_clip_err)?;
    let seq_before = clipboard::sequence_number();

    let use_ctrl_shift_c = focus::is_terminal_process(&target.process_name);
    input::send_copy(use_ctrl_shift_c);

    let deadline = Instant::now() + CLIPBOARD_COPY_TIMEOUT;
    let mut seq_changed = false;
    while Instant::now() < deadline {
        if cancel.load(Ordering::SeqCst) {
            break;
        }
        if clipboard::sequence_number() != seq_before {
            seq_changed = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }

    let read_res = if seq_changed && !cancel.load(Ordering::SeqCst) {
        clipboard::read_unicode_text(POPUP_MAX_INPUT_CHARS).map_err(map_clip_err)
    } else {
        Ok(None)
    };

    // Always restore original clipboard immediately after reading, even on TooLarge error.
    if let Err(err) = snapshot.restore() {
        log::warn!("failed restoring clipboard after capture: {err}");
    }

    let captured_text = read_res?;
    match captured_text {
        Some(text) if !text.is_empty() => validate_captured(target, text, CaptureMethod::Clipboard),
        _ => Err(CaptureError::NothingSelected),
    }
}

fn validate_captured(
    target: focus::TargetWindow,
    text: String,
    method: CaptureMethod,
) -> Result<CapturedSelection, CaptureError> {
    let char_count = text.chars().count();
    if char_count > POPUP_MAX_INPUT_CHARS {
        return Err(CaptureError::SelectionTooLong { char_count });
    }
    Ok(CapturedSelection {
        target,
        text,
        method,
    })
}

fn run_paste(
    target: &focus::TargetWindow,
    text: &str,
    cancel: &AtomicBool,
) -> Result<PasteOutcome, CaptureError> {
    if cancel.load(Ordering::SeqCst) {
        return Err(CaptureError::Failed("paste cancelled".to_string()));
    }

    // Never auto-paste multi-line text directly into a terminal emulator (newlines execute commands).
    if focus::is_terminal_process(&target.process_name)
        && (text.contains('\n') || text.contains('\r'))
    {
        log::info!(
            "refusing auto-paste of multi-line text into terminal pid={}; using clipboard fallback",
            target.pid
        );
        clipboard::write_plain_unicode_text(text).map_err(map_clip_err)?;
        return Ok(PasteOutcome::CopiedFallback);
    }

    if !focus::restore_focus(target.hwnd, target.pid) {
        log::warn!(
            "could not restore focus to pid={}; copying result as fallback",
            target.pid
        );
        clipboard::write_plain_unicode_text(text).map_err(map_clip_err)?;
        return Ok(PasteOutcome::CopiedFallback);
    }

    // Brief settle so WinUI/Electron/browser edit controls finish processing WM_ACTIVATE
    // before receiving Ctrl+V.
    std::thread::sleep(Duration::from_millis(30));

    if cancel.load(Ordering::SeqCst) || !focus::is_foreground_target(target.hwnd, target.pid) {
        log::warn!(
            "focus shifted before delayed paste for pid={}; copying result as fallback",
            target.pid
        );
        clipboard::write_plain_unicode_text(text).map_err(map_clip_err)?;
        return Ok(PasteOutcome::CopiedFallback);
    }

    match clipboard::paste_with_delayed_rendering(text, (target.hwnd, target.pid), cancel)
        .map_err(map_clip_err)?
    {
        clipboard::DelayedPasteOutcome::Pasted { rendered } => {
            Ok(PasteOutcome::Pasted { rendered })
        }
        clipboard::DelayedPasteOutcome::FocusLost => {
            log::warn!(
                "focus drifted during delayed paste setup for pid={}; copying result as fallback",
                target.pid
            );
            clipboard::write_plain_unicode_text(text).map_err(map_clip_err)?;
            Ok(PasteOutcome::CopiedFallback)
        }
    }
}

fn map_clip_err(err: clipboard::ClipboardError) -> CaptureError {
    match err {
        clipboard::ClipboardError::Busy => CaptureError::ClipboardBusy,
        clipboard::ClipboardError::TooLarge { char_count } => {
            CaptureError::SelectionTooLong { char_count }
        }
        clipboard::ClipboardError::Os(msg) => CaptureError::Failed(msg),
    }
}
