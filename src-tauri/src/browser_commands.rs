//! Embedded browser pane: a child webview (`tauri::Window::add_child`)
//! positioned over a placeholder `<div>` the frontend owns and keeps in
//! sync via `ResizeObserver`, plus native WebView2 DevTools for it
//! (`Webview::open_devtools`) — real console, network, and element
//! inspection rather than a hand-rolled console-capture panel.
//!
//! The webview gets its own label and deliberately has no entry in
//! `capabilities/*.json`. Tauri's permission model is default-deny per
//! window/webview label, so a page loaded here — which may be an arbitrary
//! external site the user typed in, not something this app controls — has
//! no path to invoke any `#[tauri::command]` at all. It is exactly as
//! sandboxed as a tab in a real browser, not a second window onto this
//! app's own privileges.
//!
//! One webview, reused across visits to the Browser page via `hide`/`show`
//! rather than destroyed and recreated — recreating on every navigation to
//! and from the page would mean losing scroll position and re-fetching
//! everything for no reason.

use tauri::webview::WebviewBuilder;
use tauri::{Emitter, LogicalPosition, LogicalSize, Manager, Position, Rect, Size, Window};

use crate::state::AppState;

const BROWSER_LABEL: &str = "browser-pane";
/// Emitted with the page's URL every time the embedded webview navigates —
/// including navigation the user triggers *inside* the page (clicking a
/// link), not just calls this module makes — so the address bar reflects
/// where the user actually is.
const NAV_EVENT: &str = "devos://browser-nav";

fn parse_url(url: &str) -> Result<url::Url, String> {
    url::Url::parse(url).map_err(|e| format!("invalid URL: {e}"))
}

fn bounds(x: f64, y: f64, width: f64, height: f64) -> Rect {
    Rect {
        position: Position::Logical(LogicalPosition { x, y }),
        size: Size::Logical(LogicalSize { width, height }),
    }
}

/// Open (or reuse, navigating and re-showing) the browser pane at the given
/// placeholder bounds, in logical pixels — the same unit
/// `getBoundingClientRect()` reports, so the frontend passes its measurement
/// straight through with no DPI math.
#[tauri::command]
pub async fn browser_open(
    window: Window,
    state: tauri::State<'_, AppState>,
    url: String,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
) -> Result<(), String> {
    let target = parse_url(&url)?;
    let mut guard = state.browser.lock().unwrap();

    if let Some(webview) = guard.as_ref() {
        webview.show().map_err(|e| e.to_string())?;
        webview
            .set_bounds(bounds(x, y, width, height))
            .map_err(|e| e.to_string())?;
        webview.navigate(target).map_err(|e| e.to_string())?;
        return Ok(());
    }

    let app_handle = window.app_handle().clone();
    let builder = WebviewBuilder::new(BROWSER_LABEL, tauri::WebviewUrl::External(target))
        .on_navigation(move |url| {
            let _ = app_handle.emit(NAV_EVENT, url.to_string());
            true
        });
    let webview = window
        .add_child(
            builder,
            Position::Logical(LogicalPosition { x, y }),
            Size::Logical(LogicalSize { width, height }),
        )
        .map_err(|e| e.to_string())?;
    *guard = Some(webview);
    Ok(())
}

/// Runs `f` against the open browser webview, or a uniform "not open" error
/// if the pane was never created or has since been closed — every command
/// below but `browser_open` goes through this rather than repeating the
/// `lock()` + `ok_or(...)` pair.
fn with_webview<T>(
    state: &tauri::State<'_, AppState>,
    f: impl FnOnce(&tauri::Webview) -> Result<T, String>,
) -> Result<T, String> {
    let guard = state.browser.lock().unwrap();
    let webview = guard.as_ref().ok_or("browser pane is not open")?;
    f(webview)
}

#[tauri::command]
pub async fn browser_navigate(
    state: tauri::State<'_, AppState>,
    url: String,
) -> Result<(), String> {
    let target = parse_url(&url)?;
    with_webview(&state, |w| w.navigate(target).map_err(|e| e.to_string()))
}

#[tauri::command]
pub async fn browser_reload(state: tauri::State<'_, AppState>) -> Result<(), String> {
    with_webview(&state, |w| w.reload().map_err(|e| e.to_string()))
}

/// Uses the page's own history stack via `eval` rather than this module
/// tracking one — Tauri's `Webview` exposes no `go_back`/`go_forward`, and
/// the browser's own history is the correct one to defer to anyway: it
/// already accounts for in-page navigation this module never sees.
#[tauri::command]
pub async fn browser_back(state: tauri::State<'_, AppState>) -> Result<(), String> {
    with_webview(&state, |w| {
        w.eval("history.back()").map_err(|e| e.to_string())
    })
}

#[tauri::command]
pub async fn browser_forward(state: tauri::State<'_, AppState>) -> Result<(), String> {
    with_webview(&state, |w| {
        w.eval("history.forward()").map_err(|e| e.to_string())
    })
}

#[tauri::command]
pub async fn browser_set_bounds(
    state: tauri::State<'_, AppState>,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
) -> Result<(), String> {
    with_webview(&state, |w| {
        w.set_bounds(bounds(x, y, width, height))
            .map_err(|e| e.to_string())
    })
}

/// Called when the Browser page unmounts (navigating elsewhere in DevOS) —
/// hidden, not closed, so returning to the page shows the same page at the
/// same scroll position rather than a fresh load.
#[tauri::command]
pub async fn browser_hide(state: tauri::State<'_, AppState>) -> Result<(), String> {
    with_webview(&state, |w| w.hide().map_err(|e| e.to_string()))
}

#[tauri::command]
pub async fn browser_open_devtools(state: tauri::State<'_, AppState>) -> Result<(), String> {
    with_webview(&state, |w| {
        w.open_devtools();
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_url_accepts_a_normal_http_url() {
        assert!(parse_url("http://localhost:3000").is_ok());
    }

    #[test]
    fn parse_url_rejects_a_string_with_no_colon_at_all() {
        assert!(parse_url("not a url").is_err());
    }

    /// The gotcha this module's frontend caller has to handle, not this
    /// function: "host:port" with no `http://` is valid per the URL spec —
    /// `Url::parse` reads "localhost" as the *scheme* and "3000" as an
    /// opaque path, not as a host to connect to. A webview asked to load it
    /// would show a blank page with no error, since nothing recognizes a
    /// `localhost:` scheme. Pinned here so nobody "fixes" `parse_url` on the
    /// assumption that a bare `host:port` already fails to parse.
    #[test]
    fn a_bare_host_port_string_parses_but_not_as_a_reachable_url() {
        let parsed = parse_url("localhost:3000").expect("this string does parse");
        assert_eq!(parsed.scheme(), "localhost");
        assert_ne!(
            parsed.scheme(),
            "http",
            "the frontend must prepend a real scheme before this is ever reachable"
        );
    }
}
