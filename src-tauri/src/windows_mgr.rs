use std::sync::atomic::Ordering;
use std::time::Instant;

use tauri::{
    AppHandle, Manager, Url, WebviewUrl, WebviewWindow, WebviewWindowBuilder, WindowEvent,
};

use crate::AppState;
use crate::app_info::{
    APP_NAME, DEV_HOST, DEV_PORT, MAIN_ENTRY, POPUP_COMPACT_HEIGHT_LOGICAL, POPUP_ENTRY,
    POPUP_EXPANDED_HEIGHT_LOGICAL, POPUP_WIDTH_LOGICAL, WINDOW_MAIN, WINDOW_POPUP,
};
use crate::platform::placement;

/// Navigation guard: rejects any URL that does not belong to Redline's own app origin.
/// CSP cannot prevent top-level navigation, so every webview attaches this check.
pub fn is_allowed_app_url(url: &Url) -> bool {
    let scheme = url.scheme();
    let host = url.host_str();

    if matches!(scheme, "tauri" | "http" | "https") && host == Some("tauri.localhost") {
        return true;
    }

    if scheme == "tauri" && host == Some("localhost") {
        return true;
    }

    if cfg!(debug_assertions)
        && scheme == "http"
        && matches!(host, Some(DEV_HOST) | Some("127.0.0.1"))
        && url.port() == Some(DEV_PORT)
    {
        return true;
    }

    false
}

/// Opens the main window on demand, or brings the existing one to the front.
/// Closing the main window destroys its webview so idle RAM stays low.
pub fn open_or_focus_main(app: &AppHandle) -> tauri::Result<()> {
    if let Some(window) = app.get_webview_window(WINDOW_MAIN) {
        let _ = window.unminimize();
        window.show()?;
        window.set_focus()?;
        return Ok(());
    }

    WebviewWindowBuilder::new(app, WINDOW_MAIN, WebviewUrl::App(MAIN_ENTRY.into()))
        .title(APP_NAME)
        .inner_size(860.0, 620.0)
        .min_inner_size(640.0, 480.0)
        .center()
        .on_navigation(|url| {
            let allowed = is_allowed_app_url(url);
            if !allowed {
                log::warn!("blocked navigation to disallowed scheme={}", url.scheme());
            }
            allowed
        })
        .build()?;

    Ok(())
}

/// Pre-creates the frameless inline popup window hidden at startup so hotkey-to-popup is instant.
/// Disables WebView2 browser accelerator keys and enables low-memory mode while hidden.
pub fn precreate_popup(app: &AppHandle) -> tauri::Result<()> {
    let window = WebviewWindowBuilder::new(app, WINDOW_POPUP, WebviewUrl::App(POPUP_ENTRY.into()))
        .title(APP_NAME)
        .visible(false)
        .decorations(false)
        .transparent(false)
        .shadow(true)
        .always_on_top(true)
        .skip_taskbar(true)
        .resizable(false)
        .inner_size(POPUP_WIDTH_LOGICAL, POPUP_COMPACT_HEIGHT_LOGICAL)
        .on_navigation(|url| {
            let allowed = is_allowed_app_url(url);
            if !allowed {
                log::warn!(
                    "blocked popup navigation to disallowed scheme={}",
                    url.scheme()
                );
            }
            allowed
        })
        .build()?;

    configure_popup_webview(&window);

    let app_handle = app.clone();
    window.on_window_event(move |event| match event {
        WindowEvent::CloseRequested { api, .. } => {
            // Keep pre-created popup alive; hiding puts it back in low-memory mode.
            api.prevent_close();
            crate::cancel_inflight(&app_handle.state::<AppState>(), WINDOW_POPUP);
            hide_popup(&app_handle);
        }
        WindowEvent::Focused(false) => {
            let state = app_handle.state::<AppState>();
            if state.should_dismiss_on_blur() {
                crate::cancel_inflight(&state, WINDOW_POPUP);
                hide_popup(&app_handle);
                *state
                    .active_capture
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
            }
        }
        _ => {}
    });

    Ok(())
}

#[cfg(windows)]
fn configure_popup_webview(window: &WebviewWindow) {
    use webview2_com::Microsoft::Web::WebView2::Win32::{
        COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW, ICoreWebView2_19, ICoreWebView2Settings3,
    };
    use windows::core::Interface;

    let _ = window.with_webview(|wv| unsafe {
        let controller = wv.controller();
        if let Ok(core) = controller.CoreWebView2() {
            if let Ok(settings) = core.Settings()
                && let Ok(settings3) = settings.cast::<ICoreWebView2Settings3>()
                && let Err(err) = settings3.SetAreBrowserAcceleratorKeysEnabled(false)
            {
                log::warn!("failed disabling WebView2 accelerator keys: {err}");
            }
            if let Ok(wv19) = core.cast::<ICoreWebView2_19>() {
                let _ = wv19.SetMemoryUsageTargetLevel(COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW);
            }
        }
    });
}

#[cfg(not(windows))]
fn configure_popup_webview(_window: &WebviewWindow) {}

#[cfg(windows)]
pub fn set_popup_low_memory(window: &WebviewWindow, low: bool) {
    use webview2_com::Microsoft::Web::WebView2::Win32::{
        COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW, COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL,
        ICoreWebView2_19,
    };
    use windows::core::Interface;

    let level = if low {
        COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW
    } else {
        COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL
    };
    let _ = window.with_webview(move |wv| unsafe {
        if let Ok(core) = wv.controller().CoreWebView2()
            && let Ok(wv19) = core.cast::<ICoreWebView2_19>()
        {
            let _ = wv19.SetMemoryUsageTargetLevel(level);
        }
    });
}

#[cfg(not(windows))]
pub fn set_popup_low_memory(_window: &WebviewWindow, _low: bool) {}

/// Positions the popup at the cursor (DPI-aware, clamped to monitor work area) and shows it
/// WITHOUT activating it so the target app keeps focus during UIA/Ctrl+C capture.
pub fn show_popup_for_capture(app: &AppHandle) -> tauri::Result<()> {
    let Some(window) = app.get_webview_window(WINDOW_POPUP) else {
        return Ok(());
    };

    let state = app.state::<AppState>();
    state.popup_visible.store(true, Ordering::SeqCst);
    state.popup_capturing.store(true, Ordering::SeqCst);
    set_popup_low_memory(&window, false);

    if let (Ok(hwnd), Some((cx, cy, work))) = (window.hwnd(), placement::cursor_monitor_work_area())
    {
        let rect = placement::compute_popup_rect(
            cx,
            cy,
            &work,
            POPUP_WIDTH_LOGICAL,
            POPUP_COMPACT_HEIGHT_LOGICAL,
            POPUP_EXPANDED_HEIGHT_LOGICAL,
        );
        placement::show_window_no_activate(hwnd.0 as isize, rect);
    } else {
        window.show()?;
    }

    Ok(())
}

/// Activates the popup after capture completes and records the timestamp for blur-grace handling.
pub fn activate_popup_after_capture(app: &AppHandle) {
    let state = app.state::<AppState>();
    *state
        .popup_activated_at
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(Instant::now());
    state.popup_capturing.store(false, Ordering::SeqCst);

    if let Some(window) = app.get_webview_window(WINDOW_POPUP) {
        let _ = window.set_focus();
    }
}

/// Switches popup between compact (460x320) and expanded (460x520) clamped to its current monitor.
pub fn resize_popup(app: &AppHandle, expanded: bool) -> tauri::Result<()> {
    let Some(window) = app.get_webview_window(WINDOW_POPUP) else {
        return Ok(());
    };

    let target_h = if expanded {
        POPUP_EXPANDED_HEIGHT_LOGICAL
    } else {
        POPUP_COMPACT_HEIGHT_LOGICAL
    };

    if let Ok(hwnd) = window.hwnd()
        && let Some((current_rect, work)) = placement::window_rect_and_work_area(hwnd.0 as isize)
    {
        let next = placement::compute_resized_rect(
            current_rect.left,
            current_rect.top,
            &work,
            POPUP_WIDTH_LOGICAL,
            target_h,
        );
        placement::move_resize_topmost(hwnd.0 as isize, next);
    }

    Ok(())
}

/// Hides the popup and switches its WebView2 back into low-memory mode.
pub fn hide_popup(app: &AppHandle) {
    let state = app.state::<AppState>();
    state.popup_visible.store(false, Ordering::SeqCst);
    state.popup_capturing.store(false, Ordering::SeqCst);

    if let Some(window) = app.get_webview_window(WINDOW_POPUP) {
        let _ = window.hide();
        set_popup_low_memory(&window, true);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allows_tauri_app_origins() {
        for raw in [
            "http://tauri.localhost/index.html",
            "https://tauri.localhost/popup.html",
            "tauri://localhost/index.html",
        ] {
            let url = Url::parse(raw).expect("valid test URL");
            assert!(is_allowed_app_url(&url), "expected allowed: {raw}");
        }
    }

    #[test]
    fn rejects_external_and_dangerous_urls() {
        for raw in [
            "https://example.com/",
            "http://example.com/",
            "https://openrouter.ai/keys",
            "file:///C:/Windows/System32/cmd.exe",
            "javascript:alert(1)",
            "data:text/html,<h1>hi</h1>",
            "http://tauri.localhost.evil.com/",
        ] {
            let url = Url::parse(raw).expect("valid test URL");
            assert!(!is_allowed_app_url(&url), "expected rejected: {raw}");
        }
    }

    #[test]
    fn dev_origin_allowed_only_in_debug_builds() {
        let url = Url::parse("http://localhost:1420/index.html").expect("valid dev URL");
        assert_eq!(is_allowed_app_url(&url), cfg!(debug_assertions));

        let wrong_port = Url::parse("http://localhost:3000/").expect("valid URL");
        assert!(!is_allowed_app_url(&wrong_port));
    }
}
