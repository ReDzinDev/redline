use tauri::{AppHandle, Manager, Url, WebviewUrl, WebviewWindowBuilder};

use crate::app_info::{APP_NAME, DEV_HOST, DEV_PORT, MAIN_ENTRY, WINDOW_MAIN};

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
