use std::ffi::c_void;
use std::mem::size_of;
use std::path::Path;
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{CloseHandle, ERROR_ACCESS_DENIED, HANDLE, HWND};
use windows::Win32::Security::{GetTokenInformation, TOKEN_ELEVATION, TOKEN_QUERY, TokenElevation};
use windows::Win32::System::Threading::{
    AttachThreadInput, GetCurrentProcess, GetCurrentThreadId, OpenProcess, OpenProcessToken,
    PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AllowSetForegroundWindow, BringWindowToTop, GetForegroundWindow, GetWindowThreadProcessId,
    IsWindow, SetForegroundWindow,
};
use windows::core::PWSTR;

#[derive(Debug, Clone)]
pub struct TargetWindow {
    pub hwnd: isize,
    pub process_name: String,
    pub pid: u32,
    pub is_elevated: bool,
}

/// Captures metadata about the currently focused foreground window.
pub fn capture_foreground_window() -> Option<TargetWindow> {
    // SAFETY: Win32 foreground window query takes no pointers and returns null HWND if none.
    let hwnd = unsafe { GetForegroundWindow() };
    if hwnd.0.is_null() {
        return None;
    }

    let mut pid: u32 = 0;
    // SAFETY: `pid` is a valid mutable u32 pointer for GetWindowThreadProcessId.
    unsafe {
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
    }

    let (process_name, target_elevated) = inspect_process(pid);
    let self_elevated = is_current_process_elevated();

    Some(TargetWindow {
        hwnd: hwnd.0 as isize,
        process_name,
        pid,
        // UIPI only blocks reading/sending input when the target is elevated and Redline is not.
        is_elevated: target_elevated && !self_elevated,
    })
}

/// Restores focus to `hwnd` and verifies the target window actually became foreground.
/// Returns `false` if the window no longer exists or focus could not be restored.
pub fn restore_focus(hwnd_raw: isize) -> bool {
    if hwnd_raw == 0 {
        return false;
    }
    let hwnd = HWND(hwnd_raw as *mut c_void);

    // SAFETY: Win32 window validity & foreground APIs on a recorded HWND.
    unsafe {
        if !IsWindow(Some(hwnd)).as_bool() {
            return false;
        }
        if GetForegroundWindow() == hwnd {
            return true;
        }

        let mut target_pid: u32 = 0;
        let target_tid = GetWindowThreadProcessId(hwnd, Some(&mut target_pid));
        if target_pid != 0 {
            let _ = AllowSetForegroundWindow(target_pid);
        }

        let _ = SetForegroundWindow(hwnd);
        if GetForegroundWindow() == hwnd {
            return true;
        }

        // Briefly attach thread input if the first SetForegroundWindow was blocked by focus lock.
        let current_tid = GetCurrentThreadId();
        let fg_hwnd = GetForegroundWindow();
        let fg_tid = if !fg_hwnd.0.is_null() {
            GetWindowThreadProcessId(fg_hwnd, None)
        } else {
            0
        };

        let attached_fg = fg_tid != 0
            && fg_tid != current_tid
            && AttachThreadInput(current_tid, fg_tid, true).as_bool();
        let attached_target = target_tid != 0
            && target_tid != current_tid
            && target_tid != fg_tid
            && AttachThreadInput(current_tid, target_tid, true).as_bool();

        let _ = BringWindowToTop(hwnd);
        let _ = SetForegroundWindow(hwnd);

        if attached_target {
            let _ = AttachThreadInput(current_tid, target_tid, false);
        }
        if attached_fg {
            let _ = AttachThreadInput(current_tid, fg_tid, false);
        }

        let deadline = Instant::now() + Duration::from_millis(150);
        while Instant::now() < deadline {
            if GetForegroundWindow() == hwnd {
                return true;
            }
            if !IsWindow(Some(hwnd)).as_bool() {
                return false;
            }
            std::thread::sleep(Duration::from_millis(10));
        }

        GetForegroundWindow() == hwnd
    }
}

fn inspect_process(pid: u32) -> (String, bool) {
    if pid == 0 {
        return ("unknown".to_string(), false);
    }

    // SAFETY: OpenProcess / QueryFullProcessImageNameW / OpenProcessToken with owned handles.
    unsafe {
        let handle = match OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) {
            Ok(h) => h,
            Err(err) => {
                let elevated = err.code() == ERROR_ACCESS_DENIED.to_hresult();
                return ("unknown".to_string(), elevated);
            }
        };

        let name = query_process_basename(handle).unwrap_or_else(|| "unknown".to_string());
        let elevated = query_handle_elevated(handle);
        let _ = CloseHandle(handle);
        (name, elevated)
    }
}

unsafe fn query_process_basename(handle: HANDLE) -> Option<String> {
    let mut buf = vec![0u16; 1024];
    let mut len = u32::try_from(buf.len()).ok()?;
    // SAFETY: `buf` is valid writable UTF-16 buffer of length `len`.
    unsafe {
        QueryFullProcessImageNameW(
            handle,
            PROCESS_NAME_WIN32,
            PWSTR(buf.as_mut_ptr()),
            &mut len,
        )
        .ok()?;
    }
    let full = String::from_utf16_lossy(&buf[..len as usize]);
    let base = Path::new(&full)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(&full);
    Some(base.to_string())
}

unsafe fn query_handle_elevated(process: HANDLE) -> bool {
    let mut token = HANDLE::default();
    // SAFETY: Queries elevation token; ERROR_ACCESS_DENIED from an unelevated caller indicates
    // a higher-integrity (elevated or protected) target process.
    let res = unsafe { OpenProcessToken(process, TOKEN_QUERY, &mut token) };
    if let Err(err) = res {
        return err.code() == ERROR_ACCESS_DENIED.to_hresult();
    }

    let mut elevation = TOKEN_ELEVATION::default();
    let mut return_len: u32 = 0;
    let ok = unsafe {
        GetTokenInformation(
            token,
            TokenElevation,
            Some((&mut elevation as *mut TOKEN_ELEVATION).cast::<c_void>()),
            size_of::<TOKEN_ELEVATION>() as u32,
            &mut return_len,
        )
        .is_ok()
    };
    let _ = unsafe { CloseHandle(token) };
    ok && elevation.TokenIsElevated != 0
}

fn is_current_process_elevated() -> bool {
    // SAFETY: GetCurrentProcess returns a pseudo-handle that does not need to be closed.
    unsafe { query_handle_elevated(GetCurrentProcess()) }
}

/// Process names where UIA text selection is unreliable or triggers heavy accessibility modes.
pub fn should_skip_uia(process_name: &str) -> bool {
    let lower = process_name.to_ascii_lowercase();
    matches!(lower.as_str(), "code.exe" | "javaw.exe")
}

/// Terminal emulators where copy is `Ctrl+Shift+C` rather than `Ctrl+C`.
pub fn is_terminal_process(process_name: &str) -> bool {
    let lower = process_name.to_ascii_lowercase();
    matches!(
        lower.as_str(),
        "windowsterminal.exe"
            | "wt.exe"
            | "cmd.exe"
            | "powershell.exe"
            | "pwsh.exe"
            | "conhost.exe"
            | "alacritty.exe"
            | "wezterm-gui.exe"
            | "mintty.exe"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uia_skip_list_and_terminal_detection_are_case_insensitive() {
        assert!(should_skip_uia("Code.exe"));
        assert!(should_skip_uia("code.exe"));
        assert!(should_skip_uia("JAVAW.EXE"));
        assert!(!should_skip_uia("notepad.exe"));

        assert!(is_terminal_process("WindowsTerminal.exe"));
        assert!(is_terminal_process("pwsh.exe"));
        assert!(!is_terminal_process("chrome.exe"));
    }
}
