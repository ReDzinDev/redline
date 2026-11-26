use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use thiserror::Error;

use crate::app_info::POPUP_MAX_INPUT_CHARS;
use crate::platform::{clipboard, focus, input, uia};

const MODIFIER_WAIT_TIMEOUT: Duration = Duration::from_millis(300);
const CLIPBOARD_COPY_TIMEOUT: Duration = Duration::from_millis(500);

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
        reply: mpsc::Sender<Result<CapturedSelection, CaptureError>>,
    },
    Paste {
        target: focus::TargetWindow,
        text: String,
        reply: mpsc::Sender<Result<PasteOutcome, CaptureError>>,
    },
    RestoreFocus {
        target: focus::TargetWindow,
        reply: mpsc::Sender<bool>,
    },
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
                        WorkerJob::Capture { target, reply } => {
                            let res = run_capture(&uia_reader, target);
                            busy_worker.store(false, Ordering::SeqCst);
                            let _ = reply.send(res);
                        }
                        WorkerJob::Paste {
                            target,
                            text,
                            reply,
                        } => {
                            let res = run_paste(&target, &text);
                            busy_worker.store(false, Ordering::SeqCst);
                            let _ = reply.send(res);
                        }
                        WorkerJob::RestoreFocus { target, reply } => {
                            let ok = focus::restore_focus(target.hwnd);
                            busy_worker.store(false, Ordering::SeqCst);
                            let _ = reply.send(ok);
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

        let (reply_tx, reply_rx) = mpsc::channel();
        if self
            .tx
            .send(WorkerJob::Capture {
                target,
                reply: reply_tx,
            })
            .is_err()
        {
            self.busy.store(false, Ordering::SeqCst);
            return Err(CaptureError::WorkerUnavailable);
        }

        reply_rx.recv_timeout(Duration::from_secs(3)).map_err(|_| {
            self.busy.store(false, Ordering::SeqCst);
            CaptureError::Failed("capture timed out".to_string())
        })?
    }

    /// Returns focus to `target` and pastes `text` via delayed rendering with clipboard restore.
    /// If focus cannot be restored, copies `text` normally so the user can Ctrl+V manually.
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

        let (reply_tx, reply_rx) = mpsc::channel();
        if self
            .tx
            .send(WorkerJob::Paste {
                target,
                text,
                reply: reply_tx,
            })
            .is_err()
        {
            self.busy.store(false, Ordering::SeqCst);
            return Err(CaptureError::WorkerUnavailable);
        }

        reply_rx.recv_timeout(Duration::from_secs(3)).map_err(|_| {
            self.busy.store(false, Ordering::SeqCst);
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

        let (reply_tx, reply_rx) = mpsc::channel();
        if self
            .tx
            .send(WorkerJob::RestoreFocus {
                target,
                reply: reply_tx,
            })
            .is_err()
        {
            self.busy.store(false, Ordering::SeqCst);
            return false;
        }

        reply_rx
            .recv_timeout(Duration::from_secs(1))
            .unwrap_or(false)
    }
}

fn run_capture(
    uia_reader: &uia::UiaReader,
    target: focus::TargetWindow,
) -> Result<CapturedSelection, CaptureError> {
    if target.is_elevated {
        return Err(CaptureError::ElevatedTarget);
    }

    // 1. Try UI Automation on the focused element (never touching clipboard) unless skipped.
    if !focus::should_skip_uia(&target.process_name)
        && let Some(text) = uia_reader.read_focused_selection()
        && !text.is_empty()
    {
        return validate_captured(target, text, CaptureMethod::Uia);
    }

    // 2. Clipboard fallback with full RAII snapshot and automatic restore.
    input::wait_for_modifiers_released(MODIFIER_WAIT_TIMEOUT);

    let snapshot = clipboard::ClipboardSnapshot::capture().map_err(map_clip_err)?;
    let seq_before = clipboard::sequence_number();

    let use_ctrl_shift_c = focus::is_terminal_process(&target.process_name);
    input::send_copy(use_ctrl_shift_c);

    let deadline = Instant::now() + CLIPBOARD_COPY_TIMEOUT;
    let mut seq_changed = false;
    while Instant::now() < deadline {
        if clipboard::sequence_number() != seq_before {
            seq_changed = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }

    let captured_text = if seq_changed {
        clipboard::read_unicode_text().map_err(map_clip_err)?
    } else {
        None
    };

    // Always restore original clipboard immediately after reading.
    if let Err(err) = snapshot.restore() {
        log::warn!("failed restoring clipboard after capture: {err}");
    }

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

fn run_paste(target: &focus::TargetWindow, text: &str) -> Result<PasteOutcome, CaptureError> {
    if !focus::restore_focus(target.hwnd) {
        log::warn!(
            "could not restore focus to {} (pid {}); copying result as fallback",
            target.process_name,
            target.pid
        );
        clipboard::write_plain_unicode_text(text).map_err(map_clip_err)?;
        return Ok(PasteOutcome::CopiedFallback);
    }

    // Brief settle so WinUI/Electron/browser edit controls finish processing WM_ACTIVATE
    // before receiving Ctrl+V.
    std::thread::sleep(Duration::from_millis(30));

    let rendered = clipboard::paste_with_delayed_rendering(text).map_err(map_clip_err)?;
    Ok(PasteOutcome::Pasted { rendered })
}

fn map_clip_err(err: clipboard::ClipboardError) -> CaptureError {
    match err {
        clipboard::ClipboardError::Busy => CaptureError::ClipboardBusy,
        clipboard::ClipboardError::Os(msg) => CaptureError::Failed(msg),
    }
}
