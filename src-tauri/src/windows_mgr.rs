use std::sync::atomic::Ordering;
use std::time::Instant;

use tauri::webview::NewWindowResponse;
use tauri::{
    AppHandle, Manager, Url, WebviewUrl, WebviewWindow, WebviewWindowBuilder, WindowEvent,
};

use crate::AppState;
use crate::app_info::{
    APP_NAME, DEV_HOST, DEV_PORT, MAIN_ENTRY, POPUP_COMPACT_HEIGHT_LOGICAL, POPUP_ENTRY,
    POPUP_EXPANDED_HEIGHT_LOGICAL, POPUP_MIN_HEIGHT_LOGICAL, POPUP_MIN_WIDTH_LOGICAL,
    POPUP_WIDTH_LOGICAL, WINDOW_MAIN, WINDOW_POPUP,
};
use crate::platform::placement;

/// Navigation guard: rejects any URL that does not belong to Redline's exact bundled app origin.
/// CSP cannot prevent top-level navigation, so every webview attaches this check.
/// Rejects userinfo, non-default ports, and `https://tauri.localhost` (since `use_https_scheme`
/// is disabled and `wry` only intercepts `http://tauri.*` on Windows).
pub fn is_allowed_app_url(url: &Url) -> bool {
    if !url.username().is_empty() || url.password().is_some() {
        return false;
    }

    let scheme = url.scheme();
    let host = url.host_str();

    if scheme == "http" && host == Some("tauri.localhost") && url.port().is_none() {
        return true;
    }

    if scheme == "tauri" && host == Some("localhost") && url.port().is_none() {
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

    let window = WebviewWindowBuilder::new(app, WINDOW_MAIN, WebviewUrl::App(MAIN_ENTRY.into()))
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
        .on_new_window(|_, _| NewWindowResponse::Deny)
        .build()?;

    configure_hardened_webview(&window, false);

    let app_handle = app.clone();
    window.on_window_event(move |event| {
        if matches!(
            event,
            WindowEvent::CloseRequested { .. } | WindowEvent::Destroyed
        ) {
            crate::cancel_inflight(&app_handle.state::<AppState>(), WINDOW_MAIN);
        }
    });

    Ok(())
}

/// Pre-creates the frameless inline popup window hidden at startup so hotkey-to-popup is instant.
/// Hardens WebView2 settings and enables low-memory mode while hidden.
pub fn precreate_popup(app: &AppHandle) -> tauri::Result<()> {
    let window = WebviewWindowBuilder::new(app, WINDOW_POPUP, WebviewUrl::App(POPUP_ENTRY.into()))
        .title(APP_NAME)
        .visible(false)
        .decorations(false)
        .transparent(false)
        .shadow(true)
        .always_on_top(true)
        .skip_taskbar(true)
        .resizable(true)
        .inner_size(POPUP_WIDTH_LOGICAL, POPUP_COMPACT_HEIGHT_LOGICAL)
        .min_inner_size(POPUP_MIN_WIDTH_LOGICAL, POPUP_MIN_HEIGHT_LOGICAL)
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
        .on_new_window(|_, _| NewWindowResponse::Deny)
        .build()?;

    configure_hardened_webview(&window, true);

    let app_handle = app.clone();
    window.on_window_event(move |event| match event {
        WindowEvent::CloseRequested { api, .. } => {
            // Keep pre-created popup alive; hiding puts it back in low-memory mode and clears capture.
            api.prevent_close();
            let state = app_handle.state::<AppState>();
            crate::cancel_inflight(&state, WINDOW_POPUP);
            *state
                .active_capture
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
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
fn configure_hardened_webview(window: &WebviewWindow, low_memory: bool) {
    use webview2_com::Microsoft::Web::WebView2::Win32::{
        COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW, ICoreWebView2_19, ICoreWebView2Settings3,
        ICoreWebView2Settings4,
    };
    use windows::core::Interface;

    let _ = window.with_webview(move |wv| unsafe {
        let controller = wv.controller();
        if let Ok(core) = controller.CoreWebView2() {
            if let Ok(settings) = core.Settings() {
                if let Err(err) = settings.SetAreDefaultContextMenusEnabled(false) {
                    log::warn!("failed disabling WebView2 default context menus: {err}");
                }
                if let Ok(settings3) = settings.cast::<ICoreWebView2Settings3>()
                    && let Err(err) = settings3.SetAreBrowserAcceleratorKeysEnabled(false)
                {
                    log::warn!("failed disabling WebView2 accelerator keys: {err}");
                }
                if let Ok(settings4) = settings.cast::<ICoreWebView2Settings4>() {
                    if let Err(err) = settings4.SetIsGeneralAutofillEnabled(false) {
                        log::warn!("failed disabling WebView2 general autofill: {err}");
                    }
                    if let Err(err) = settings4.SetIsPasswordAutosaveEnabled(false) {
                        log::warn!("failed disabling WebView2 password autosave: {err}");
                    }
                }
            }
            if low_memory && let Ok(wv19) = core.cast::<ICoreWebView2_19>() {
                let _ = wv19.SetMemoryUsageTargetLevel(COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW);
            }
        }
    });
}

#[cfg(not(windows))]
fn configure_hardened_webview(_window: &WebviewWindow, _low_memory: bool) {}

#[cfg(windows)]
pub fn set_popup_webview_state(window: &WebviewWindow, visible: bool) {
    use webview2_com::Microsoft::Web::WebView2::Win32::{
        COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW, COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL,
        ICoreWebView2_19,
    };
    use windows::core::Interface;

    let level = if visible {
        COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL
    } else {
        COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW
    };
    let _ = window.with_webview(move |wv| unsafe {
        let controller = wv.controller();
        let _ = controller.SetIsVisible(visible);
        if let Ok(core) = controller.CoreWebView2()
            && let Ok(wv19) = core.cast::<ICoreWebView2_19>()
        {
            let _ = wv19.SetMemoryUsageTargetLevel(level);
        }
    });
}

#[cfg(not(windows))]
pub fn set_popup_webview_state(_window: &WebviewWindow, _visible: bool) {}

#[cfg(windows)]
pub fn focus_popup_webview(window: &WebviewWindow) {
    use webview2_com::Microsoft::Web::WebView2::Win32::COREWEBVIEW2_MOVE_FOCUS_REASON_PROGRAMMATIC;

    let _ = window.with_webview(|wv| unsafe {
        let _ = wv
            .controller()
            .MoveFocus(COREWEBVIEW2_MOVE_FOCUS_REASON_PROGRAMMATIC);
    });
}

#[cfg(not(windows))]
pub fn focus_popup_webview(_window: &WebviewWindow) {}

/// Returns true if the popup HWND is currently visible at the OS level.
pub fn is_popup_hwnd_visible(app: &AppHandle) -> bool {
    let Some(window) = app.get_webview_window(WINDOW_POPUP) else {
        return false;
    };
    window
        .hwnd()
        .map(|h| placement::is_window_visible(h.0 as isize))
        .unwrap_or(false)
}

/// Positions the popup at the cursor (DPI-aware, clamped to monitor work area) and shows it
/// WITHOUT activating it so the target app keeps focus during UIA/Ctrl+C capture.
pub fn show_popup_for_capture(app: &AppHandle) -> tauri::Result<()> {
    let Some(window) = app.get_webview_window(WINDOW_POPUP) else {
        return Ok(());
    };

    let state = app.state::<AppState>();
    state.popup_visible.store(true, Ordering::SeqCst);
    state.popup_capturing.store(true, Ordering::SeqCst);
    set_popup_webview_state(&window, true);

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

/// Activates the popup after capture completes on the UI thread, syncing `tao`'s visibility
/// flag (`window.show()`) so subsequent `window.set_focus()` and `window.hide()` calls work
/// reliably, and moves keyboard focus directly into WebView2 so Esc / 1-6 / Enter work right away.
pub fn activate_popup_after_capture(app: &AppHandle) {
    let state = app.state::<AppState>();
    *state
        .popup_activated_at
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(Instant::now());
    state.popup_capturing.store(false, Ordering::SeqCst);

    let app_handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        if let Some(window) = app_handle.get_webview_window(WINDOW_POPUP) {
            set_popup_webview_state(&window, true);
            let _ = window.show();
            let _ = window.set_focus();
            if let Ok(hwnd) = window.hwnd() {
                placement::force_activate_window(hwnd.0 as isize);
            }
            focus_popup_webview(&window);
        }
    });
}

/// Re-shows the popup on the UI thread with proper state tracking to display a fallback notice.
pub fn reshow_popup_for_notice(app: &AppHandle) {
    let state = app.state::<AppState>();
    state.popup_visible.store(true, Ordering::SeqCst);
    state.popup_capturing.store(false, Ordering::SeqCst);
    *state
        .popup_activated_at
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(Instant::now());

    let app_handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        if let Some(window) = app_handle.get_webview_window(WINDOW_POPUP) {
            set_popup_webview_state(&window, true);
            let _ = window.show();
            let _ = window.set_focus();
            if let Ok(hwnd) = window.hwnd() {
                placement::force_activate_window(hwnd.0 as isize);
            }
            focus_popup_webview(&window);
        }
    });
}

/// Switches popup between compact (500x320) and expanded (500x560) clamped to its current monitor,
/// respecting any larger manual size the user dragged to when expanding.
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
        let scale = work.scale_factor.max(0.5);
        let current_w_logical = f64::from(current_rect.right - current_rect.left) / scale;
        let current_h_logical = f64::from(current_rect.bottom - current_rect.top) / scale;
        let next_w = if expanded {
            POPUP_WIDTH_LOGICAL.max(current_w_logical)
        } else {
            POPUP_WIDTH_LOGICAL
        };
        let next_h = if expanded {
            target_h.max(current_h_logical)
        } else {
            target_h
        };
        let next = placement::compute_resized_rect(
            current_rect.left,
            current_rect.top,
            &work,
            next_w,
            next_h,
        );
        placement::move_resize_topmost(hwnd.0 as isize, next);
    }

    Ok(())
}

/// Hides the popup on the UI thread via Win32 `SW_HIDE`, syncs `tao`'s visibility flag,
/// and switches WebView2 into invisible low-memory mode.
pub fn hide_popup(app: &AppHandle) {
    let state = app.state::<AppState>();
    state.popup_visible.store(false, Ordering::SeqCst);
    state.popup_capturing.store(false, Ordering::SeqCst);

    let app_handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        hide_popup_on_main_thread(&app_handle);
    });
}

/// Async helper for Tokio command handlers: waits for the UI thread to finish hiding the popup
/// before restoring focus to the target window and simulating Ctrl+V.
pub async fn hide_popup_and_wait(app: &AppHandle) {
    let state = app.state::<AppState>();
    state.popup_visible.store(false, Ordering::SeqCst);
    state.popup_capturing.store(false, Ordering::SeqCst);

    let (tx, rx) = std::sync::mpsc::channel();
    let app_handle = app.clone();
    if app
        .run_on_main_thread(move || {
            hide_popup_on_main_thread(&app_handle);
            let _ = tx.send(());
        })
        .is_ok()
    {
        let _ = tauri::async_runtime::spawn_blocking(move || {
            let _ = rx.recv_timeout(std::time::Duration::from_millis(250));
        })
        .await;
    }
}

fn hide_popup_on_main_thread(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(WINDOW_POPUP) {
        if let Ok(hwnd) = window.hwnd() {
            placement::hide_window_immediately(hwnd.0 as isize);
        }
        let _ = window.hide();
        set_popup_webview_state(&window, false);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allows_exact_tauri_app_origins_only() {
        for raw in [
            "http://tauri.localhost/index.html",
            "http://tauri.localhost/popup.html",
            "tauri://localhost/index.html",
        ] {
            let url = Url::parse(raw).expect("valid test URL");
            assert!(is_allowed_app_url(&url), "expected allowed: {raw}");
        }
    }

    #[test]
    fn rejects_external_non_default_ports_https_and_userinfo_urls() {
        for raw in [
            "https://tauri.localhost/popup.html",
            "https://tauri.localhost:8443/index.html",
            "http://tauri.localhost:8080/index.html",
            "http://user@tauri.localhost/index.html",
            "http://user:pass@tauri.localhost/index.html",
            "tauri://localhost:1234/index.html",
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

        let with_userinfo =
            Url::parse("http://user@localhost:1420/index.html").expect("valid userinfo URL");
        assert!(!is_allowed_app_url(&with_userinfo));

        let wrong_port = Url::parse("http://localhost:3000/").expect("valid URL");
        assert!(!is_allowed_app_url(&wrong_port));
    }
}
