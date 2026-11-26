use std::ffi::c_void;
use std::mem::size_of;

use windows::Win32::Foundation::{HWND, POINT, RECT};
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, HMONITOR, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromPoint,
    MonitorFromWindow,
};
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::WindowsAndMessaging::{
    GetCursorPos, GetWindowRect, HWND_TOPMOST, SWP_NOACTIVATE, SWP_SHOWWINDOW, SetWindowPos,
};

const CURSOR_OFFSET_LOGICAL: f64 = 12.0;
const EDGE_MARGIN_LOGICAL: f64 = 8.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MonitorWorkArea {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
    pub scale_factor: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PopupRect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

/// Computes the initial popup rectangle in physical pixels:
/// - Default: below-right of the cursor.
/// - Flip direction is chosen using `expanded_height_logical` so expanding never jumps off-screen.
/// - Clamped inside the monitor's work area with DPI awareness.
pub fn compute_popup_rect(
    cursor_x: i32,
    cursor_y: i32,
    work: &MonitorWorkArea,
    width_logical: f64,
    compact_height_logical: f64,
    expanded_height_logical: f64,
) -> PopupRect {
    let scale = work.scale_factor.max(0.5);
    let width = (width_logical * scale).round() as i32;
    let compact_h = (compact_height_logical * scale).round() as i32;
    let expanded_h = (expanded_height_logical * scale).round() as i32;
    let offset = (CURSOR_OFFSET_LOGICAL * scale).round() as i32;
    let margin = (EDGE_MARGIN_LOGICAL * scale).round() as i32;

    let work_left = work.left + margin;
    let work_top = work.top + margin;
    let work_right = (work.right - margin).max(work_left + 1);
    let work_bottom = (work.bottom - margin).max(work_top + 1);

    let x = if cursor_x + offset + width <= work_right {
        cursor_x + offset
    } else {
        cursor_x - offset - width
    };

    // Flip vertically based on the expanded height so expanding stays anchored cleanly.
    let y = if cursor_y + offset + expanded_h <= work_bottom {
        cursor_y + offset
    } else {
        cursor_y - offset - compact_h
    };

    clamp_rect(
        x,
        y,
        width,
        compact_h,
        work_left,
        work_top,
        work_right,
        work_bottom,
    )
}

/// Computes a clamped physical rectangle when resizing an already-positioned popup.
pub fn compute_resized_rect(
    current_x: i32,
    current_y: i32,
    work: &MonitorWorkArea,
    width_logical: f64,
    target_height_logical: f64,
) -> PopupRect {
    let scale = work.scale_factor.max(0.5);
    let width = (width_logical * scale).round() as i32;
    let height = (target_height_logical * scale).round() as i32;
    let margin = (EDGE_MARGIN_LOGICAL * scale).round() as i32;

    let work_left = work.left + margin;
    let work_top = work.top + margin;
    let work_right = (work.right - margin).max(work_left + 1);
    let work_bottom = (work.bottom - margin).max(work_top + 1);

    clamp_rect(
        current_x,
        current_y,
        width,
        height,
        work_left,
        work_top,
        work_right,
        work_bottom,
    )
}

#[allow(clippy::too_many_arguments)]
fn clamp_rect(
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    work_left: i32,
    work_top: i32,
    work_right: i32,
    work_bottom: i32,
) -> PopupRect {
    let max_x = (work_right - width).max(work_left);
    let max_y = (work_bottom - height).max(work_top);
    PopupRect {
        x: x.clamp(work_left, max_x),
        y: y.clamp(work_top, max_y),
        width,
        height,
    }
}

fn monitor_work_area(hmonitor: HMONITOR) -> Option<MonitorWorkArea> {
    if hmonitor.0.is_null() {
        return None;
    }
    let mut info = MONITORINFO {
        cbSize: size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    // SAFETY: `info.cbSize` is initialized before calling GetMonitorInfoW.
    if !unsafe { GetMonitorInfoW(hmonitor, &mut info) }.as_bool() {
        return None;
    }

    let mut dpi_x: u32 = 96;
    let mut dpi_y: u32 = 96;
    let scale_factor =
        if unsafe { GetDpiForMonitor(hmonitor, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y) }.is_ok()
            && dpi_x > 0
        {
            f64::from(dpi_x) / 96.0
        } else {
            1.0
        };

    Some(MonitorWorkArea {
        left: info.rcWork.left,
        top: info.rcWork.top,
        right: info.rcWork.right,
        bottom: info.rcWork.bottom,
        scale_factor,
    })
}

pub fn cursor_monitor_work_area() -> Option<(i32, i32, MonitorWorkArea)> {
    let mut pt = POINT::default();
    unsafe { GetCursorPos(&mut pt) }.ok()?;
    let hmonitor = unsafe { MonitorFromPoint(pt, MONITOR_DEFAULTTONEAREST) };
    let work = monitor_work_area(hmonitor)?;
    Some((pt.x, pt.y, work))
}

pub fn window_rect_and_work_area(hwnd_raw: isize) -> Option<(RECT, MonitorWorkArea)> {
    if hwnd_raw == 0 {
        return None;
    }
    let hwnd = HWND(hwnd_raw as *mut c_void);
    let mut rect = RECT::default();
    unsafe { GetWindowRect(hwnd, &mut rect) }.ok()?;
    let hmonitor = unsafe { MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST) };
    let work = monitor_work_area(hmonitor)?;
    Some((rect, work))
}

/// Positions and shows the popup topmost WITHOUT activating it (keeps focus on target app
/// during UIA/Ctrl+C capture). Moves first, then sets the monitor-scaled size so cross-monitor
/// DPI switches stay sharp and clamped.
pub fn show_window_no_activate(hwnd_raw: isize, rect: PopupRect) {
    if hwnd_raw == 0 {
        return;
    }
    let hwnd = HWND(hwnd_raw as *mut c_void);
    // SAFETY: Moves/sizes/shows our own popup HWND without stealing foreground focus.
    unsafe {
        let _ = SetWindowPos(
            hwnd,
            Some(HWND_TOPMOST),
            rect.x,
            rect.y,
            rect.width,
            rect.height,
            SWP_NOACTIVATE | SWP_SHOWWINDOW,
        );
    }
}

/// Resizes and clamps an already-visible popup without disturbing focus.
pub fn move_resize_topmost(hwnd_raw: isize, rect: PopupRect) {
    if hwnd_raw == 0 {
        return;
    }
    let hwnd = HWND(hwnd_raw as *mut c_void);
    unsafe {
        let _ = SetWindowPos(
            hwnd,
            Some(HWND_TOPMOST),
            rect.x,
            rect.y,
            rect.width,
            rect.height,
            SWP_NOACTIVATE,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hd_monitor(scale: f64) -> MonitorWorkArea {
        MonitorWorkArea {
            left: 0,
            top: 0,
            right: 1920,
            bottom: 1040,
            scale_factor: scale,
        }
    }

    #[test]
    fn places_below_right_when_space_allows() {
        let work = hd_monitor(1.0);
        let rect = compute_popup_rect(200, 200, &work, 460.0, 320.0, 520.0);
        assert_eq!(rect.x, 212);
        assert_eq!(rect.y, 212);
        assert_eq!(rect.width, 460);
        assert_eq!(rect.height, 320);
    }

    #[test]
    fn flips_left_and_above_using_expanded_height() {
        let work = hd_monitor(1.0);
        // Cursor near bottom-right: compact (320) would fit below y=650, but expanded (520) does not!
        let rect = compute_popup_rect(1700, 650, &work, 460.0, 320.0, 520.0);
        assert!(rect.x < 1700, "should flip left of cursor: {rect:?}");
        assert!(
            rect.y < 650,
            "should flip above cursor using expanded height: {rect:?}"
        );
        assert!(rect.x + rect.width <= work.right);
        assert!(rect.y + rect.height <= work.bottom);
    }

    #[test]
    fn respects_high_dpi_and_secondary_monitor_offset() {
        let secondary = MonitorWorkArea {
            left: 1920,
            top: 0,
            right: 3840,
            bottom: 1080,
            scale_factor: 1.5,
        };
        let rect = compute_popup_rect(3800, 1050, &secondary, 460.0, 320.0, 520.0);
        assert_eq!(rect.width, 690);
        assert_eq!(rect.height, 480);
        assert!(rect.x >= secondary.left);
        assert!(rect.x + rect.width <= secondary.right);
        assert!(rect.y >= secondary.top);
        assert!(rect.y + rect.height <= secondary.bottom);
    }
}
