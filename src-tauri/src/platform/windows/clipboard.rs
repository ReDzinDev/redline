use std::cell::Cell;
use std::ffi::c_void;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use thiserror::Error;
use windows::Win32::Foundation::{
    ERROR_SUCCESS, GlobalFree, HANDLE, HGLOBAL, HINSTANCE, HWND, LPARAM, LRESULT, SetLastError,
    WPARAM,
};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, EnumClipboardFormats, GetClipboardData,
    GetClipboardFormatNameW, GetClipboardOwner, GetClipboardSequenceNumber, OpenClipboard,
    RegisterClipboardFormatW, SetClipboardData,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Memory::{
    GMEM_MOVEABLE, GlobalAlloc, GlobalFlags, GlobalLock, GlobalSize, GlobalUnlock,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GWLP_USERDATA,
    GetWindowLongPtrW, HWND_MESSAGE, InSendMessage, MSG, MsgWaitForMultipleObjects, PM_REMOVE,
    PeekMessageW, QS_ALLINPUT, RegisterClassW, SetWindowLongPtrW, TranslateMessage,
    WINDOW_EX_STYLE, WINDOW_STYLE, WM_CLOSE, WM_DESTROYCLIPBOARD, WM_RENDERALLFORMATS,
    WM_RENDERFORMAT, WNDCLASSW,
};
use windows::core::{PCWSTR, w};

use crate::platform::windows::{focus, input};

const CF_TEXT: u32 = 1;
const CF_BITMAP: u32 = 2;
const CF_METAFILEPICT: u32 = 3;
const CF_OEMTEXT: u32 = 7;
const CF_DIB: u32 = 8;
const CF_PALETTE: u32 = 9;
const CF_UNICODETEXT: u32 = 13;
const CF_ENHMETAFILE: u32 = 14;
const CF_HDROP: u32 = 15;
const CF_LOCALE: u32 = 16;
const CF_DIBV5: u32 = 17;
const CF_OWNERDISPLAY: u32 = 0x0080;
const CF_DSPTEXT: u32 = 0x0081;
const CF_DSPENHMETAFILE: u32 = 0x008E;
const CF_PRIVATEFIRST: u32 = 0x0200;
const CF_PRIVATELAST: u32 = 0x02FF;
const CF_GDIOBJFIRST: u32 = 0x0300;
const CF_GDIOBJLAST: u32 = 0x03FF;
const REGISTERED_FORMAT_START: u32 = 0xC000;

/// Win32 `GMEM_INVALID_HANDLE` returned by `GlobalFlags` when a handle is not a valid `HGLOBAL`.
const GMEM_INVALID_HANDLE: u32 = 0x8000;

const MAX_SNAPSHOT_TOTAL_BYTES: usize = 50 * 1024 * 1024;
const MAX_SINGLE_FORMAT_BYTES: usize = 25 * 1024 * 1024;
const OPEN_CLIPBOARD_ATTEMPTS: usize = 10;
const OPEN_CLIPBOARD_BACKOFF: Duration = Duration::from_millis(15);
const DELAYED_RENDER_TIMEOUT: Duration = Duration::from_millis(1000);
const POST_RENDER_GRACE: Duration = Duration::from_millis(45);
const PASTE_MODIFIER_WAIT_TIMEOUT: Duration = Duration::from_millis(150);

#[derive(Debug, Error)]
pub enum ClipboardError {
    #[error("clipboard is busy")]
    Busy,
    #[error("clipboard text exceeds maximum length ({char_count} chars)")]
    TooLarge { char_count: usize },
    #[error("clipboard operation failed: {0}")]
    Os(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DelayedPasteOutcome {
    Pasted { rendered: bool },
    FocusLost,
}

/// Pure decision function for which clipboard formats are safe and useful to snapshot/restore.
/// Keeps standard HGLOBAL formats plus an explicit allowlist of well-known HGLOBAL-backed
/// registered formats (PNG, HTML, RTF, Explorer drop metadata, etc.) so opaque non-HGLOBAL
/// custom formats and Windows clipboard-history exclusion markers are never dereferenced.
pub fn should_snapshot_format(format_id: u32, format_name: Option<&str>) -> bool {
    // Standard HGLOBAL formats that we always preserve.
    if matches!(
        format_id,
        CF_TEXT | CF_OEMTEXT | CF_DIB | CF_UNICODETEXT | CF_HDROP | CF_LOCALE | CF_DIBV5
    ) {
        return true;
    }

    // Standard handle/GDI/private formats that cannot be copied via GlobalLock.
    if matches!(
        format_id,
        CF_BITMAP | CF_METAFILEPICT | CF_PALETTE | CF_ENHMETAFILE | CF_OWNERDISPLAY
    ) || (CF_DSPTEXT..=CF_DSPENHMETAFILE).contains(&format_id)
        || (CF_PRIVATEFIRST..=CF_PRIVATELAST).contains(&format_id)
        || (CF_GDIOBJFIRST..=CF_GDIOBJLAST).contains(&format_id)
    {
        return false;
    }

    // Registered formats (>= 0xC000): explicit allowlist of well-known HGLOBAL data formats.
    if format_id >= REGISTERED_FORMAT_START {
        let Some(name) = format_name else {
            return false;
        };
        let lower = name.to_ascii_lowercase();
        return matches!(
            lower.as_str(),
            "png"
                | "jfif"
                | "gif"
                | "image/png"
                | "image/jpeg"
                | "image/gif"
                | "image/webp"
                | "html format"
                | "rich text format"
                | "rich text format without objects"
                | "csv"
                | "text/html"
                | "text/rtf"
                | "preferred dropeffect"
                | "performed dropeffect"
                | "logical performed dropeffect"
                | "shell idlist array"
                | "filename"
                | "filenamew"
                | "uniformresourcelocator"
                | "uniformresourcelocatorw"
                | "chromium web custom mime data format"
                | "chromium internal source url"
                | "org.chromium.web-custom-data"
        );
    }

    false
}

struct ExclusionFormats {
    exclude_monitor: u32,
    can_history: u32,
    can_cloud: u32,
}

fn exclusion_formats() -> &'static ExclusionFormats {
    static FORMATS: OnceLock<ExclusionFormats> = OnceLock::new();
    FORMATS.get_or_init(|| unsafe {
        ExclusionFormats {
            exclude_monitor: RegisterClipboardFormatW(w!(
                "ExcludeClipboardContentFromMonitorProcessing"
            )),
            can_history: RegisterClipboardFormatW(w!("CanIncludeInClipboardHistory")),
            can_cloud: RegisterClipboardFormatW(w!("CanUploadToCloudClipboard")),
        }
    })
}

/// Tags the currently-open clipboard so Windows ignores it in Win+V history and cloud sync.
/// Caller MUST hold `OpenClipboard`.
unsafe fn write_exclusion_markers() {
    let fmts = exclusion_formats();
    if fmts.exclude_monitor != 0 {
        let _ = unsafe { set_hglobal_bytes(fmts.exclude_monitor, &1u32.to_le_bytes()) };
    }
    if fmts.can_history != 0 {
        let _ = unsafe { set_hglobal_bytes(fmts.can_history, &0u32.to_le_bytes()) };
    }
    if fmts.can_cloud != 0 {
        let _ = unsafe { set_hglobal_bytes(fmts.can_cloud, &0u32.to_le_bytes()) };
    }
}

unsafe fn set_hglobal_bytes(format: u32, bytes: &[u8]) -> Result<(), ClipboardError> {
    let alloc_len = bytes.len().max(1);
    let hglobal = unsafe { GlobalAlloc(GMEM_MOVEABLE, alloc_len) }
        .map_err(|e| ClipboardError::Os(e.to_string()))?;

    let ptr = unsafe { GlobalLock(hglobal) };
    if ptr.is_null() {
        let _ = unsafe { GlobalFree(Some(hglobal)) };
        return Err(ClipboardError::Os("GlobalLock failed".to_string()));
    }

    unsafe {
        if bytes.is_empty() {
            *(ptr.cast::<u8>()) = 0;
        } else {
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr.cast::<u8>(), bytes.len());
        }
        let _ = GlobalUnlock(hglobal);
    }

    if let Err(err) = unsafe { SetClipboardData(format, Some(HANDLE(hglobal.0))) } {
        let _ = unsafe { GlobalFree(Some(hglobal)) };
        return Err(ClipboardError::Os(err.to_string()));
    }

    Ok(())
}

struct ClipboardGuard;

impl Drop for ClipboardGuard {
    fn drop(&mut self) {
        // SAFETY: Always paired with a successful `OpenClipboard`.
        let _ = unsafe { CloseClipboard() };
    }
}

fn open_clipboard_with_retry(owner: Option<HWND>) -> Result<ClipboardGuard, ClipboardError> {
    for attempt in 0..OPEN_CLIPBOARD_ATTEMPTS {
        // SAFETY: Standard Win32 OpenClipboard.
        if unsafe { OpenClipboard(owner) }.is_ok() {
            return Ok(ClipboardGuard);
        }
        if attempt + 1 < OPEN_CLIPBOARD_ATTEMPTS {
            std::thread::sleep(OPEN_CLIPBOARD_BACKOFF);
        }
    }
    Err(ClipboardError::Busy)
}

fn query_format_name(format_id: u32) -> Option<String> {
    if format_id < REGISTERED_FORMAT_START {
        return None;
    }
    let mut buf = [0u16; 256];
    // SAFETY: `buf` is valid writable UTF-16 slice.
    let len = unsafe { GetClipboardFormatNameW(format_id, &mut buf) };
    if len <= 0 {
        return None;
    }
    Some(String::from_utf16_lossy(&buf[..len as usize]))
}

#[derive(Debug)]
struct SavedFormat {
    format_id: u32,
    bytes: Vec<u8>,
}

/// RAII snapshot of all safe clipboard formats.
/// Automatically restores the clipboard on `Drop` unless explicitly restored or disarmed,
/// guaranteeing a panic or error mid-capture/mid-paste never loses the user's clipboard.
pub struct ClipboardSnapshot {
    formats: Vec<SavedFormat>,
    restored_or_disarmed: bool,
}

impl ClipboardSnapshot {
    pub fn capture() -> Result<Self, ClipboardError> {
        let _guard = open_clipboard_with_retry(None)?;
        let mut formats = Vec::new();
        let mut total_bytes: usize = 0;
        let mut fmt = 0u32;

        loop {
            // SAFETY: EnumClipboardFormats called while clipboard is open.
            fmt = unsafe { EnumClipboardFormats(fmt) };
            if fmt == 0 {
                break;
            }
            let name = query_format_name(fmt);
            if !should_snapshot_format(fmt, name.as_deref()) {
                continue;
            }

            // SAFETY: Reading allowlisted HGLOBAL bytes while holding OpenClipboard.
            let Ok(handle) = (unsafe { GetClipboardData(fmt) }) else {
                continue;
            };
            if handle.0.is_null() {
                continue;
            }
            let hglobal = HGLOBAL(handle.0);
            if unsafe { GlobalFlags(hglobal) } == GMEM_INVALID_HANDLE {
                continue;
            }
            let size = unsafe { GlobalSize(hglobal) };
            if size == 0
                || size > MAX_SINGLE_FORMAT_BYTES
                || total_bytes.saturating_add(size) > MAX_SNAPSHOT_TOTAL_BYTES
            {
                continue;
            }

            let ptr = unsafe { GlobalLock(hglobal) };
            if ptr.is_null() {
                continue;
            }
            let slice = unsafe { std::slice::from_raw_parts(ptr.cast::<u8>(), size) };
            let bytes = slice.to_vec();
            let _ = unsafe { GlobalUnlock(hglobal) };

            total_bytes += bytes.len();
            formats.push(SavedFormat {
                format_id: fmt,
                bytes,
            });
        }

        Ok(Self {
            formats,
            restored_or_disarmed: false,
        })
    }

    pub fn restore(mut self) -> Result<(), ClipboardError> {
        let res = self.restore_inner();
        self.restored_or_disarmed = true;
        res
    }

    /// Prevents `Drop` from overwriting the clipboard (used if another app took ownership).
    pub fn disarm(&mut self) {
        self.restored_or_disarmed = true;
    }

    fn restore_inner(&self) -> Result<(), ClipboardError> {
        let _guard = open_clipboard_with_retry(None)?;
        // SAFETY: Clipboard is open; EmptyClipboard clears before restoring saved formats.
        unsafe { EmptyClipboard() }.map_err(|e| ClipboardError::Os(e.to_string()))?;

        for saved in &self.formats {
            if let Err(err) = unsafe { set_hglobal_bytes(saved.format_id, &saved.bytes) } {
                log::warn!(
                    "failed restoring clipboard format {}: {err}",
                    saved.format_id
                );
            }
        }

        // Mark restore invisible to Win+V so restoring the original clipboard doesn't duplicate it.
        unsafe { write_exclusion_markers() };
        Ok(())
    }
}

impl Drop for ClipboardSnapshot {
    fn drop(&mut self) {
        if !self.restored_or_disarmed
            && let Err(err) = self.restore_inner()
        {
            log::warn!("failed to restore clipboard on drop: {err}");
        }
    }
}

pub fn sequence_number() -> u32 {
    // SAFETY: GetClipboardSequenceNumber has no preconditions.
    unsafe { GetClipboardSequenceNumber() }
}

/// Reads `CF_UNICODETEXT` from the clipboard with retry, bounding decoded length to `max_chars`
/// so an oversized clipboard payload cannot trigger an OOM abort while `ClipboardSnapshot` is live.
pub fn read_unicode_text(max_chars: usize) -> Result<Option<String>, ClipboardError> {
    let _guard = open_clipboard_with_retry(None)?;
    let Ok(handle) = (unsafe { GetClipboardData(CF_UNICODETEXT) }) else {
        return Ok(None);
    };
    if handle.0.is_null() {
        return Ok(None);
    }

    let hglobal = HGLOBAL(handle.0);
    if unsafe { GlobalFlags(hglobal) } == GMEM_INVALID_HANDLE {
        return Ok(None);
    }
    let byte_len = unsafe { GlobalSize(hglobal) };
    if byte_len < 2 {
        return Ok(None);
    }

    // Worst-case UTF-16 length for `max_chars + 1` Unicode scalar values (all surrogate pairs).
    let max_u16_units = max_chars.saturating_add(1).saturating_mul(2);
    let total_units = byte_len / 2;
    let inspect_units = total_units.min(max_u16_units.saturating_add(1));

    let ptr = unsafe { GlobalLock(hglobal) };
    if ptr.is_null() {
        return Ok(None);
    }
    let u16_slice = unsafe { std::slice::from_raw_parts(ptr.cast::<u16>(), inspect_units) };
    let nul_pos = u16_slice.iter().position(|&c| c == 0);

    let result = match nul_pos {
        Some(pos) => Ok(Some(String::from_utf16_lossy(&u16_slice[..pos]))),
        None if total_units > max_u16_units => Err(ClipboardError::TooLarge {
            char_count: max_chars.saturating_add(1),
        }),
        None => Ok(Some(String::from_utf16_lossy(u16_slice))),
    };

    let _ = unsafe { GlobalUnlock(hglobal) };
    result
}

/// Writes plain Unicode text to the clipboard as a normal user copy (visible in Win+V history).
/// Used when focus cannot be restored so the user can manually press Ctrl+V.
pub fn write_plain_unicode_text(text: &str) -> Result<(), ClipboardError> {
    let utf16_bytes = encode_utf16_nul(text);
    let _guard = open_clipboard_with_retry(None)?;
    unsafe { EmptyClipboard() }.map_err(|e| ClipboardError::Os(e.to_string()))?;
    unsafe { set_hglobal_bytes(CF_UNICODETEXT, &utf16_bytes) }
}

fn encode_utf16_nul(text: &str) -> Vec<u8> {
    let mut bytes = Vec::with_capacity((text.len() + 1) * 2);
    for unit in text.encode_utf16() {
        // Never emit an interior NUL (0x0000) that would silently truncate CF_UNICODETEXT.
        if unit != 0 {
            bytes.extend_from_slice(&unit.to_le_bytes());
        }
    }
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes
}

/// Shared interior-mutable state read/written via `&DelayedPasteState` ONLY from both
/// `paste_with_delayed_rendering` and `paste_wndproc` (eliminating `&mut` aliasing UB).
struct DelayedPasteState {
    utf16_bytes: Vec<u8>,
    rendered: Cell<bool>,
    ownership_lost: Cell<bool>,
}

/// RAII guard for the hidden `HWND_MESSAGE` delayed-render window.
/// Declared AFTER `ClipboardSnapshot` and `DelayedPasteState` so even during unwinding
/// `GWLP_USERDATA` is cleared and `DestroyWindow` runs BEFORE `state` or `snapshot` drop.
struct PasteWindowGuard {
    hwnd: HWND,
}

impl PasteWindowGuard {
    fn create(hinstance: HINSTANCE) -> Result<Self, ClipboardError> {
        // SAFETY: Creates a hidden message-only window owned by the platform worker thread.
        let hwnd = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE(0),
                w!("RedlineDelayedPasteWindow"),
                PCWSTR::null(),
                WINDOW_STYLE(0),
                0,
                0,
                0,
                0,
                Some(HWND_MESSAGE),
                None,
                Some(hinstance),
                None,
            )
        }
        .map_err(|e| ClipboardError::Os(e.to_string()))?;
        Ok(Self { hwnd })
    }

    fn hwnd(&self) -> HWND {
        self.hwnd
    }

    fn attach_state(&self, state: &DelayedPasteState) {
        // SAFETY: `state` outlives `self` (enforced by reverse local declaration drop order).
        unsafe {
            SetWindowLongPtrW(
                self.hwnd,
                GWLP_USERDATA,
                (state as *const DelayedPasteState) as isize,
            );
        }
    }
}

impl Drop for PasteWindowGuard {
    fn drop(&mut self) {
        // SAFETY: Detaches `GWLP_USERDATA` before destroying the window so no late callback can
        // observe a dangling pointer, and always runs before `DelayedPasteState` / `ClipboardSnapshot` drop.
        unsafe {
            SetWindowLongPtrW(self.hwnd, GWLP_USERDATA, 0);
            let _ = DestroyWindow(self.hwnd);
        }
    }
}

unsafe extern "system" fn paste_wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if msg == WM_CLOSE {
        // Ignore external WM_CLOSE so another process cannot prematurely destroy our paste window.
        return LRESULT(0);
    }

    let state_ptr = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *const DelayedPasteState;
    if !state_ptr.is_null() {
        // SAFETY: `state_ptr` points to a heap-pinned `DelayedPasteState` that outlives `hwnd`
        // (guaranteed by `PasteWindowGuard`'s drop order) and uses only shared `&DelayedPasteState`
        // + `Cell` interior mutability, avoiding any `&mut` aliasing violation.
        let state = unsafe { &*state_ptr };
        match msg {
            WM_RENDERFORMAT => {
                // Genuine Win32 delayed-render requests are sent synchronously by the OS.
                if wparam.0 as u32 == CF_UNICODETEXT && unsafe { InSendMessage() }.as_bool() {
                    if let Err(err) =
                        unsafe { set_hglobal_bytes(CF_UNICODETEXT, &state.utf16_bytes) }
                    {
                        log::warn!("WM_RENDERFORMAT failed to provide text: {err}");
                    }
                    state.rendered.set(true);
                }
                return LRESULT(0);
            }
            WM_RENDERALLFORMATS => {
                if let Ok(_guard) = open_clipboard_with_retry(Some(hwnd)) {
                    let _ = unsafe { set_hglobal_bytes(CF_UNICODETEXT, &state.utf16_bytes) };
                    state.rendered.set(true);
                }
                return LRESULT(0);
            }
            WM_DESTROYCLIPBOARD => {
                // Verify we genuinely lost clipboard ownership (guards against spoofed PostMessage).
                if unsafe { GetClipboardOwner() }.ok() != Some(hwnd) {
                    state.ownership_lost.set(true);
                }
                return LRESULT(0);
            }
            _ => {}
        }
    }
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

fn ensure_paste_window_class() -> Result<HINSTANCE, ClipboardError> {
    static CLASS_REGISTERED: OnceLock<Result<isize, String>> = OnceLock::new();
    let res = CLASS_REGISTERED.get_or_init(|| unsafe {
        let hmodule = GetModuleHandleW(PCWSTR::null()).map_err(|e| e.to_string())?;
        let hinstance = HINSTANCE(hmodule.0);
        let wc = WNDCLASSW {
            lpfnWndProc: Some(paste_wndproc),
            hInstance: hinstance,
            lpszClassName: w!("RedlineDelayedPasteWindow"),
            ..Default::default()
        };
        if RegisterClassW(&wc) == 0 {
            return Err("RegisterClassW failed".to_string());
        }
        Ok(hinstance.0 as isize)
    });

    match res {
        Ok(raw) => Ok(HINSTANCE(*raw as *mut c_void)),
        Err(msg) => Err(ClipboardError::Os(msg.clone())),
    }
}

/// Pastes `text` into `expected_target` `(hwnd, pid)` using Windows delayed rendering:
/// 1. Snapshots all allowlisted clipboard formats (with automatic Drop restore).
/// 2. Registers `CF_UNICODETEXT` as delayed-rendered (`NULL` handle) + Win+V exclusion markers.
/// 3. Waits for modifiers to be released, then re-verifies `expected_target` is still foreground
///    and `cancelled` is false immediately before sending `Ctrl+V`.
/// 4. Waits for the target app to pull `WM_RENDERFORMAT` (up to 1 s cap).
/// 5. Detaches/destroys the paste window and restores the original clipboard snapshot immediately
///    afterward (unless another app took clipboard ownership in the meantime).
pub fn paste_with_delayed_rendering(
    text: &str,
    expected_target: (isize, u32),
    cancelled: &AtomicBool,
) -> Result<DelayedPasteOutcome, ClipboardError> {
    // Drop order is LIFO: `msg_window` drops first (detaching GWLP_USERDATA & destroying HWND),
    // then `state`, then `snapshot` (calling EmptyClipboard safely with no live paste_wndproc).
    let mut snapshot = ClipboardSnapshot::capture()?;
    let hinstance = ensure_paste_window_class()?;

    let state = Box::new(DelayedPasteState {
        utf16_bytes: encode_utf16_nul(text),
        rendered: Cell::new(false),
        ownership_lost: Cell::new(false),
    });

    let msg_window = PasteWindowGuard::create(hinstance)?;
    msg_window.attach_state(&state);
    let msg_hwnd = msg_window.hwnd();

    // Set delayed-rendered CF_UNICODETEXT + Win+V exclusion markers while owning the clipboard.
    {
        let _guard = open_clipboard_with_retry(Some(msg_hwnd))?;
        unsafe { EmptyClipboard() }.map_err(|e| ClipboardError::Os(e.to_string()))?;
        // NULL handle registers delayed rendering; Windows returns NULL even on success,
        // so clear last error first and treat ERROR_SUCCESS (S_OK) as success.
        unsafe { SetLastError(ERROR_SUCCESS) };
        if let Err(err) = unsafe { SetClipboardData(CF_UNICODETEXT, None) }
            && err.code().is_err()
        {
            return Err(ClipboardError::Os(err.to_string()));
        }
        unsafe { write_exclusion_markers() };
    }

    input::wait_for_modifiers_released(PASTE_MODIFIER_WAIT_TIMEOUT);

    // Re-verify foreground HWND + PID and job cancellation right before injecting synthetic Ctrl+V.
    if cancelled.load(Ordering::SeqCst)
        || !focus::is_foreground_target(expected_target.0, expected_target.1)
    {
        drop(msg_window);
        if let Err(err) = snapshot.restore() {
            log::warn!("failed to restore clipboard after focus drift: {err}");
        }
        return Ok(DelayedPasteOutcome::FocusLost);
    }

    input::send_paste_keys();

    let start = Instant::now();
    let hard_deadline = start + DELAYED_RENDER_TIMEOUT;
    let mut grace_deadline: Option<Instant> = None;

    while Instant::now() < hard_deadline {
        if state.ownership_lost.get() || cancelled.load(Ordering::SeqCst) {
            break;
        }

        // Drain all queued messages for our message-only window.
        let mut msg = MSG::default();
        while unsafe { PeekMessageW(&mut msg, Some(msg_hwnd), 0, 0, PM_REMOVE) }.as_bool() {
            unsafe {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }

        if state.ownership_lost.get() || cancelled.load(Ordering::SeqCst) {
            break;
        }

        if state.rendered.get() {
            let gd = *grace_deadline.get_or_insert_with(|| Instant::now() + POST_RENDER_GRACE);
            if Instant::now() >= gd {
                break;
            }
        }

        let target_deadline = grace_deadline.map_or(hard_deadline, |g| g.min(hard_deadline));
        let remaining_ms = target_deadline
            .saturating_duration_since(Instant::now())
            .as_millis()
            .min(50) as u32;
        if remaining_ms == 0 {
            break;
        }

        // SAFETY: MsgWaitForMultipleObjects with empty handle slice sleeps until a window message
        // arrives or `remaining_ms` elapses.
        unsafe {
            MsgWaitForMultipleObjects(None, false, remaining_ms, QS_ALLINPUT);
        }
    }

    let rendered = state.rendered.get();
    let ownership_lost = state.ownership_lost.get();
    log::debug!(
        "paste_with_delayed_rendering finished: rendered={rendered} ownership_lost={ownership_lost} elapsed_ms={}",
        start.elapsed().as_millis()
    );

    // Explicitly detach and destroy the paste window BEFORE restoring the snapshot.
    drop(msg_window);

    if ownership_lost {
        // Another application wrote to the clipboard while we were waiting — do not clobber it.
        snapshot.disarm();
    } else if let Err(err) = snapshot.restore() {
        log::warn!("failed to restore clipboard after paste: {err}");
    }

    Ok(DelayedPasteOutcome::Pasted { rendered })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_standard_text_image_and_file_formats() {
        for fmt in [
            CF_TEXT,
            CF_OEMTEXT,
            CF_UNICODETEXT,
            CF_DIB,
            CF_DIBV5,
            CF_HDROP,
            CF_LOCALE,
        ] {
            assert!(should_snapshot_format(fmt, None), "expected keep for {fmt}");
        }
    }

    #[test]
    fn skips_gdi_handles_and_private_standard_formats() {
        for fmt in [
            CF_BITMAP,
            CF_METAFILEPICT,
            CF_PALETTE,
            CF_ENHMETAFILE,
            CF_OWNERDISPLAY,
            CF_DSPTEXT,
            CF_PRIVATEFIRST,
            CF_GDIOBJFIRST,
        ] {
            assert!(
                !should_snapshot_format(fmt, None),
                "expected skip for {fmt}"
            );
        }
    }

    #[test]
    fn filters_registered_formats_via_explicit_allowlist() {
        assert!(should_snapshot_format(0xC010, Some("PNG")));
        assert!(should_snapshot_format(0xC011, Some("HTML Format")));
        assert!(should_snapshot_format(0xC012, Some("Rich Text Format")));
        assert!(should_snapshot_format(0xC013, Some("Preferred DropEffect")));

        assert!(!should_snapshot_format(0xC020, None));
        assert!(!should_snapshot_format(
            0xC021,
            Some("ExcludeClipboardContentFromMonitorProcessing")
        ));
        assert!(!should_snapshot_format(
            0xC022,
            Some("CanIncludeInClipboardHistory")
        ));
        assert!(!should_snapshot_format(
            0xC023,
            Some("CanUploadToCloudClipboard")
        ));
        assert!(!should_snapshot_format(0xC024, Some("DataObject")));
        assert!(!should_snapshot_format(0xC025, Some("Ole Private Data")));
        // Arbitrary unknown registered formats must be excluded to prevent HGLOBAL type confusion.
        assert!(!should_snapshot_format(
            0xC099,
            Some("CustomOpaqueNonHglobalFormat")
        ));
    }

    #[test]
    fn encode_utf16_nul_strips_interior_nul_code_units() {
        let encoded = encode_utf16_nul("abc\0def");
        let units: Vec<u16> = encoded
            .as_chunks::<2>()
            .0
            .iter()
            .map(|&c| u16::from_le_bytes(c))
            .collect();
        assert_eq!(units.last(), Some(&0u16));
        assert!(!units[..units.len() - 1].contains(&0u16));
        assert_eq!(
            String::from_utf16(&units[..units.len() - 1]).unwrap(),
            "abcdef"
        );
    }
}
