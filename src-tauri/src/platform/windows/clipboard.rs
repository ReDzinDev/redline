use std::ffi::c_void;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use thiserror::Error;
use windows::Win32::Foundation::{
    ERROR_SUCCESS, GlobalFree, HANDLE, HGLOBAL, HINSTANCE, HWND, LPARAM, LRESULT, SetLastError,
    WPARAM,
};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, EnumClipboardFormats, GetClipboardData,
    GetClipboardFormatNameW, GetClipboardSequenceNumber, OpenClipboard, RegisterClipboardFormatW,
    SetClipboardData,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Memory::{
    GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GWLP_USERDATA,
    GetWindowLongPtrW, HWND_MESSAGE, MSG, MsgWaitForMultipleObjects, PM_REMOVE, PeekMessageW,
    QS_ALLINPUT, RegisterClassW, SetWindowLongPtrW, TranslateMessage, WINDOW_EX_STYLE,
    WINDOW_STYLE, WM_DESTROYCLIPBOARD, WM_RENDERFORMAT, WNDCLASSW,
};
use windows::core::{PCWSTR, w};

use crate::platform::windows::input;

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

const MAX_SNAPSHOT_TOTAL_BYTES: usize = 50 * 1024 * 1024;
const MAX_SINGLE_FORMAT_BYTES: usize = 25 * 1024 * 1024;
const OPEN_CLIPBOARD_ATTEMPTS: usize = 10;
const OPEN_CLIPBOARD_BACKOFF: Duration = Duration::from_millis(15);
const DELAYED_RENDER_TIMEOUT: Duration = Duration::from_millis(1000);
const POST_RENDER_GRACE: Duration = Duration::from_millis(45);

#[derive(Debug, Error)]
pub enum ClipboardError {
    #[error("clipboard is busy")]
    Busy,
    #[error("clipboard operation failed: {0}")]
    Os(String),
}

/// Pure decision function for which clipboard formats are safe and useful to snapshot/restore.
/// Keeps HGLOBAL data formats (text, DIB/DIBV5 images, HDROP files, PNG, HTML, RTF, etc.)
/// and skips GDI/COM handles and Windows clipboard-history exclusion markers.
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

    // Registered formats (>= 0xC000).
    if format_id >= REGISTERED_FORMAT_START {
        let Some(name) = format_name else {
            return false;
        };
        let lower = name.to_ascii_lowercase();
        let excluded = matches!(
            lower.as_str(),
            "excludeclipboardcontentfrommonitorprocessing"
                | "canincludeinclipboardhistory"
                | "canuploadtocloudclipboard"
                | "clipboard viewer ignore"
                | "dataobject"
                | "ole private data"
                | "object descriptor"
                | "link source"
                | "link source descriptor"
                | "embed source"
                | "embedded object"
                | "ownerlink"
                | "objectlink"
        );
        return !excluded;
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

            // SAFETY: Reading HGLOBAL bytes while holding OpenClipboard.
            let Ok(handle) = (unsafe { GetClipboardData(fmt) }) else {
                continue;
            };
            if handle.0.is_null() {
                continue;
            }
            let hglobal = HGLOBAL(handle.0);
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

/// Reads `CF_UNICODETEXT` from the clipboard with retry.
pub fn read_unicode_text() -> Result<Option<String>, ClipboardError> {
    let _guard = open_clipboard_with_retry(None)?;
    let Ok(handle) = (unsafe { GetClipboardData(CF_UNICODETEXT) }) else {
        return Ok(None);
    };
    if handle.0.is_null() {
        return Ok(None);
    }

    let hglobal = HGLOBAL(handle.0);
    let byte_len = unsafe { GlobalSize(hglobal) };
    if byte_len < 2 {
        return Ok(None);
    }

    let ptr = unsafe { GlobalLock(hglobal) };
    if ptr.is_null() {
        return Ok(None);
    }
    let u16_slice = unsafe { std::slice::from_raw_parts(ptr.cast::<u16>(), byte_len / 2) };
    let nul_pos = u16_slice
        .iter()
        .position(|&c| c == 0)
        .unwrap_or(u16_slice.len());
    let text = String::from_utf16_lossy(&u16_slice[..nul_pos]);
    let _ = unsafe { GlobalUnlock(hglobal) };
    Ok(Some(text))
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
        bytes.extend_from_slice(&unit.to_le_bytes());
    }
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes
}

struct DelayedPasteState {
    utf16_bytes: Vec<u8>,
    rendered: bool,
    ownership_lost: bool,
}

unsafe extern "system" fn paste_wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let state_ptr = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *mut DelayedPasteState;
    if !state_ptr.is_null() {
        // SAFETY: `state_ptr` lives on the stack of `paste_with_delayed_rendering` for the
        // entire lifetime of `hwnd`.
        let state = unsafe { &mut *state_ptr };
        match msg {
            WM_RENDERFORMAT => {
                if wparam.0 as u32 == CF_UNICODETEXT {
                    if let Err(err) =
                        unsafe { set_hglobal_bytes(CF_UNICODETEXT, &state.utf16_bytes) }
                    {
                        log::warn!("WM_RENDERFORMAT failed to provide text: {err}");
                    }
                    state.rendered = true;
                }
                return LRESULT(0);
            }
            WM_DESTROYCLIPBOARD => {
                state.ownership_lost = true;
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

/// Pastes `text` into the currently focused target app using Windows delayed rendering:
/// 1. Snapshots all safe clipboard formats (with automatic Drop restore).
/// 2. Registers `CF_UNICODETEXT` as delayed-rendered (`NULL` handle) + Win+V exclusion markers.
/// 3. Sends `Ctrl+V`.
/// 4. Waits for the target app to actually pull `WM_RENDERFORMAT` (up to 1 s cap).
/// 5. Restores the original clipboard snapshot immediately afterward (unless another app took
///    clipboard ownership in the meantime).
pub fn paste_with_delayed_rendering(text: &str) -> Result<bool, ClipboardError> {
    let mut snapshot = ClipboardSnapshot::capture()?;
    let hinstance = ensure_paste_window_class()?;

    // SAFETY: Creates a hidden message-only window owned by the platform worker thread.
    let msg_hwnd = unsafe {
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

    let mut state = DelayedPasteState {
        utf16_bytes: encode_utf16_nul(text),
        rendered: false,
        ownership_lost: false,
    };

    unsafe {
        SetWindowLongPtrW(
            msg_hwnd,
            GWLP_USERDATA,
            (&mut state as *mut DelayedPasteState) as isize,
        );
    }

    // Set delayed-rendered CF_UNICODETEXT + Win+V exclusion markers while owning the clipboard.
    let setup_result: Result<(), ClipboardError> = (|| {
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
        Ok(())
    })();

    if let Err(err) = setup_result {
        unsafe {
            SetWindowLongPtrW(msg_hwnd, GWLP_USERDATA, 0);
            let _ = DestroyWindow(msg_hwnd);
        }
        return Err(err);
    }

    input::send_paste();

    let start = Instant::now();
    let hard_deadline = start + DELAYED_RENDER_TIMEOUT;
    let mut grace_deadline: Option<Instant> = None;

    while Instant::now() < hard_deadline {
        if state.ownership_lost {
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

        if state.ownership_lost {
            break;
        }

        if state.rendered {
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

    let rendered = state.rendered;
    let ownership_lost = state.ownership_lost;

    // Clear user pointer before restoring (restore triggers WM_DESTROYCLIPBOARD on msg_hwnd).
    unsafe {
        SetWindowLongPtrW(msg_hwnd, GWLP_USERDATA, 0);
    }

    if ownership_lost {
        // Another application wrote to the clipboard while we were waiting — do not clobber it.
        snapshot.disarm();
    } else if let Err(err) = snapshot.restore() {
        log::warn!("failed to restore clipboard after paste: {err}");
    }

    unsafe {
        let _ = DestroyWindow(msg_hwnd);
    }

    Ok(rendered)
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
    fn filters_registered_formats_correctly() {
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
    }
}
