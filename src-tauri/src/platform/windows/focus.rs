use std::ffi::c_void;
use std::mem::size_of;
use std::path::Path;
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{CloseHandle, ERROR_ACCESS_DENIED, HANDLE, HWND};
use windows::Win32::Security::{
    GetTokenInformation, TOKEN_ELEVATION, TOKEN_QUERY, TokenElevation, TokenUIAccess,
};
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

struct ThreadInputAttachment {
    current_tid: u32,
    other_tid: u32,
    attached: bool,
}

impl ThreadInputAttachment {
    unsafe fn attach(current_tid: u32, other_tid: u32) -> Self {
        let attached = other_tid != 0
            && other_tid != current_tid
            && unsafe { AttachThreadInput(current_tid, other_tid, true) }.as_bool();
        Self {
            current_tid,
            other_tid,
            attached,
        }
    }
}

impl Drop for ThreadInputAttachment {
    fn drop(&mut self) {
        if self.attached {
            // SAFETY: Paired with a successful `AttachThreadInput(..., true)` on the same thread.
            let _ = unsafe { AttachThreadInput(self.current_tid, self.other_tid, false) };
        }
    }
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
        // UIPI only blocks reading/sending input when the target is elevated/UIAccess and Redline is not.
        is_elevated: target_elevated && !self_elevated,
    })
}

/// Verifies that `expected_hwnd` is still a valid window, currently holds foreground focus,
/// and is still owned by `expected_pid` (protecting against HWND reuse and focus-shift TOCTOU).
pub fn is_foreground_target(expected_hwnd: isize, expected_pid: u32) -> bool {
    if expected_hwnd == 0 || expected_pid == 0 {
        return false;
    }
    let hwnd = HWND(expected_hwnd as *mut c_void);
    // SAFETY: Standard Win32 window and foreground queries.
    unsafe {
        if !IsWindow(Some(hwnd)).as_bool() || GetForegroundWindow() != hwnd {
            return false;
        }
        let mut actual_pid: u32 = 0;
        GetWindowThreadProcessId(hwnd, Some(&mut actual_pid));
        actual_pid == expected_pid
    }
}

/// Restores focus to `hwnd_raw` owned by `expected_pid` and verifies the target window actually
/// became foreground. Returns `false` if the window no longer exists, its PID changed, or focus
/// could not be restored.
pub fn restore_focus(hwnd_raw: isize, expected_pid: u32) -> bool {
    if hwnd_raw == 0 || expected_pid == 0 {
        return false;
    }
    let hwnd = HWND(hwnd_raw as *mut c_void);

    // SAFETY: Win32 window validity & foreground APIs on a recorded HWND.
    unsafe {
        if !IsWindow(Some(hwnd)).as_bool() {
            return false;
        }

        let mut target_pid: u32 = 0;
        let target_tid = GetWindowThreadProcessId(hwnd, Some(&mut target_pid));
        if target_pid != expected_pid {
            return false;
        }

        if GetForegroundWindow() == hwnd {
            return true;
        }

        let _ = AllowSetForegroundWindow(target_pid);
        let _ = SetForegroundWindow(hwnd);
        if is_foreground_target(hwnd_raw, expected_pid) {
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

        {
            let _attach_fg = ThreadInputAttachment::attach(current_tid, fg_tid);
            let _attach_target = if target_tid != fg_tid {
                Some(ThreadInputAttachment::attach(current_tid, target_tid))
            } else {
                None
            };

            let _ = BringWindowToTop(hwnd);
            let _ = SetForegroundWindow(hwnd);
        }

        let deadline = Instant::now() + Duration::from_millis(150);
        while Instant::now() < deadline {
            if is_foreground_target(hwnd_raw, expected_pid) {
                return true;
            }
            if !IsWindow(Some(hwnd)).as_bool() {
                return false;
            }
            std::thread::sleep(Duration::from_millis(10));
        }

        is_foreground_target(hwnd_raw, expected_pid)
    }
}

fn inspect_process(pid: u32) -> (String, bool) {
    if pid == 0 {
        // Fail closed when owning PID cannot be determined.
        return ("unknown".to_string(), true);
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
    // SAFETY: Queries elevation & UIAccess token flags; ERROR_ACCESS_DENIED from an unelevated
    // caller indicates a higher-integrity (elevated or protected) target process.
    let res = unsafe { OpenProcessToken(process, TOKEN_QUERY, &mut token) };
    if let Err(err) = res {
        return err.code() == ERROR_ACCESS_DENIED.to_hresult();
    }

    let mut elevation = TOKEN_ELEVATION::default();
    let mut return_len: u32 = 0;
    let elev_ok = unsafe {
        GetTokenInformation(
            token,
            TokenElevation,
            Some((&mut elevation as *mut TOKEN_ELEVATION).cast::<c_void>()),
            size_of::<TOKEN_ELEVATION>() as u32,
            &mut return_len,
        )
        .is_ok()
    };

    let mut ui_access: u32 = 0;
    let mut ui_len: u32 = 0;
    let ui_ok = unsafe {
        GetTokenInformation(
            token,
            TokenUIAccess,
            Some((&mut ui_access as *mut u32).cast::<c_void>()),
            size_of::<u32>() as u32,
            &mut ui_len,
        )
        .is_ok()
    };

    let _ = unsafe { CloseHandle(token) };
    (elev_ok && elevation.TokenIsElevated != 0) || (ui_ok && ui_access != 0)
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

/// Terminal emulators where copy is `Ctrl+Shift+C` rather than `Ctrl+C`, and where pasting
/// multi-line text directly can execute shell commands.
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

    #[test]
    fn zero_hwnd_or_pid_fails_closed() {
        assert!(!is_foreground_target(0, 1234));
        assert!(!is_foreground_target(1234, 0));
        assert!(!restore_focus(0, 1234));
        assert!(!restore_focus(1234, 0));
    }
}
