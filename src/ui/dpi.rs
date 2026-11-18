//! Windows-specific Per-Monitor DPI scaling utilities and cross-monitor drag fix.
//!
//! ## DPI Detection
//! Provides monitor DPI detection for initial window sizing and placement
//! across multi-monitor setups (e.g. 4K 150% and 2K 100%).
//!
//! ## Cross-Monitor Drag Fix
//! Fixes cursor position jump when dragging the window between monitors with
//! different DPI scaling factors. This is a workaround for a bug in winit 0.30's
//! `WM_DPICHANGED` handler that overrides the OS-suggested window rect with its
//! own calculation, causing cursor-to-window misalignment during drag.

#[cfg(windows)]
pub mod windows {
    use windows_sys::Win32::Graphics::Gdi::{
        MonitorFromPoint, HMONITOR, MONITOR_DEFAULTTONEAREST, MONITOR_DEFAULTTOPRIMARY,
    };
    use windows_sys::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};

    /// Queries the DPI scale factor (e.g. 1.0, 1.5) for a given screen point (x, y).
    /// Used at startup to determine the DPI of the monitor where the window will appear.
    #[must_use]
    pub fn get_dpi_scale_at_point(x: i32, y: i32) -> f32 {
        unsafe {
            let pt = windows_sys::Win32::Foundation::POINT { x, y };
            let hmonitor: HMONITOR = MonitorFromPoint(pt, MONITOR_DEFAULTTONEAREST);
            if !hmonitor.is_null() {
                let mut dpi_x = 0;
                let mut dpi_y = 0;
                if GetDpiForMonitor(hmonitor, MDT_EFFECTIVE_DPI, &raw mut dpi_x, &raw mut dpi_y)
                    == 0
                    && dpi_x > 0
                {
                    return dpi_x as f32 / 96.0;
                }
            }
        }
        1.0
    }

    /// Queries the DPI scale factor of the primary monitor (where `winit` creates windows first).
    #[must_use]
    pub fn get_primary_monitor_dpi_scale() -> f32 {
        unsafe {
            let pt = windows_sys::Win32::Foundation::POINT { x: 0, y: 0 };
            let hmonitor: HMONITOR = MonitorFromPoint(pt, MONITOR_DEFAULTTOPRIMARY);
            if !hmonitor.is_null() {
                let mut dpi_x = 0;
                let mut dpi_y = 0;
                if GetDpiForMonitor(hmonitor, MDT_EFFECTIVE_DPI, &raw mut dpi_x, &raw mut dpi_y)
                    == 0
                    && dpi_x > 0
                {
                    return dpi_x as f32 / 96.0;
                }
            }
        }
        1.0
    }
}

// ---------------------------------------------------------------------------
// Cross-monitor drag fix  (Windows only)
// ---------------------------------------------------------------------------
//
// Root cause (winit 0.30.13, event_loop.rs WM_DPICHANGED handler):
//
// 1. winit ignores the OS-suggested window rect (`lparam`) and recalculates
//    the window size itself:
//      new_physical = old_physical.to_logical(old_dpi).to_physical(new_dpi)
//    This can differ by ±1–2 px from the OS suggestion due to integer rounding.
//
// 2. The cursor-position bias compensation only adjusts the *horizontal*
//    offset and mixes the `suggested_rect` width with the `conservative_rect`
//    width, producing a non-zero net error proportional to the size mismatch.
//
// 3. A pixel-by-pixel "nudge loop" may further displace the window when the
//    calculated rect straddles the monitor boundary.
//
// Fix:
//   Subclass the HWND.  During a size-move loop (drag / resize), intercept
//   WM_DPICHANGED:
//     a) Forward the message to winit so it fires ScaleFactorChanged and
//        updates its internal state.
//     b) Immediately call SetWindowPos with the OS-suggested rect to override
//        winit's broken positioning.
//
//   Outside of a size-move loop, winit's original handling is used unmodified.

#[cfg(windows)]
use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};

/// Whether the window is currently being moved/resized (inside `WM_ENTERSIZEMOVE..WM_EXITSIZEMOVE`).
#[cfg(windows)]
static IN_SIZE_MOVE: AtomicBool = AtomicBool::new(false);

/// Original window procedure pointer, saved before subclassing.
#[cfg(windows)]
static ORIGINAL_WNDPROC: AtomicIsize = AtomicIsize::new(0);

// Win32 message constants — defined here to avoid depending on which
// windows-sys feature gate exposes each constant.
#[cfg(windows)]
const WM_DPICHANGED: u32 = 0x02E0;
#[cfg(windows)]
const WM_ENTERSIZEMOVE: u32 = 0x0231;
#[cfg(windows)]
const WM_EXITSIZEMOVE: u32 = 0x0232;
#[cfg(windows)]
const WM_NCDESTROY: u32 = 0x0082;

/// Installs a window subclass that fixes cursor position jumps when dragging
/// the window between monitors with different DPI scaling factors.
///
/// Call once after window creation. Subsequent calls are no-ops.
///
/// # Safety
///
/// `hwnd` must be a valid window handle. This function replaces the window
/// procedure via `SetWindowLongPtrW`; the original procedure is saved and
/// restored on `WM_NCDESTROY`.
#[cfg(windows)]
pub(crate) fn install_dpi_drag_fix(hwnd: isize) {
    use windows_sys::Win32::Foundation::HWND;
    use windows_sys::Win32::UI::WindowsAndMessaging::{SetWindowLongPtrW, GWL_WNDPROC};

    // Guard against double-installation.
    if ORIGINAL_WNDPROC.load(Ordering::Acquire) != 0 {
        return;
    }

    let hwnd = hwnd as HWND;
    let old =
        unsafe { SetWindowLongPtrW(hwnd, GWL_WNDPROC, dpi_fix_wndproc as *const () as isize) };
    if old != 0 {
        ORIGINAL_WNDPROC.store(old, Ordering::Release);
        log::info!("[DPI Fix] Installed cross-monitor drag fix (subclass wndproc)");
    } else {
        log::warn!("[DPI Fix] Failed to install window subclass for DPI drag fix");
    }
}

/// No-op on non-Windows platforms.
#[cfg(not(windows))]
pub(crate) fn install_dpi_drag_fix(_hwnd: isize) {}

// ---------------------------------------------------------------------------
// Subclass window procedure
// ---------------------------------------------------------------------------

#[cfg(windows)]
unsafe extern "system" fn dpi_fix_wndproc(
    hwnd: windows_sys::Win32::Foundation::HWND,
    msg: u32,
    wparam: usize,
    lparam: isize,
) -> isize {
    use windows_sys::Win32::Foundation::RECT;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CallWindowProcW, SetWindowLongPtrW, SetWindowPos, GWL_WNDPROC, SWP_NOACTIVATE,
        SWP_NOZORDER, WNDPROC,
    };

    match msg {
        WM_ENTERSIZEMOVE => {
            IN_SIZE_MOVE.store(true, Ordering::Relaxed);
        }

        WM_EXITSIZEMOVE => {
            IN_SIZE_MOVE.store(false, Ordering::Relaxed);
        }

        WM_DPICHANGED if IN_SIZE_MOVE.load(Ordering::Relaxed) => {
            // Save the OS-suggested window rect *before* winit processes the
            // message.  The OS calculates this rect specifically to keep the
            // cursor anchored at the correct relative position during drag.
            let suggested = *(lparam as *const RECT);

            // Forward to winit so it fires ScaleFactorChanged and updates its
            // internal scale_factor / pixels_per_point state.
            let result = call_original(hwnd, msg, wparam, lparam);

            // Override winit's broken positioning with the OS-suggested rect.
            // The OS rect preserves cursor-to-window anchoring across DPI
            // boundaries; winit's custom rect does not.
            SetWindowPos(
                hwnd,
                std::ptr::null_mut(),
                suggested.left,
                suggested.top,
                suggested.right - suggested.left,
                suggested.bottom - suggested.top,
                SWP_NOZORDER | SWP_NOACTIVATE,
            );

            return result;
        }

        WM_NCDESTROY => {
            // Restore original wndproc before window destruction.
            let original = ORIGINAL_WNDPROC.swap(0, Ordering::AcqRel);
            if original != 0 {
                SetWindowLongPtrW(hwnd, GWL_WNDPROC, original);
                let proc: WNDPROC = Some(std::mem::transmute::<
                    isize,
                    unsafe extern "system" fn(
                        windows_sys::Win32::Foundation::HWND,
                        u32,
                        usize,
                        isize,
                    ) -> isize,
                >(original));
                return CallWindowProcW(proc, hwnd, msg, wparam, lparam);
            }
        }

        _ => {}
    }

    // Forward all other messages to winit's original wndproc.
    call_original(hwnd, msg, wparam, lparam)
}

/// Forwards a message to the saved original (winit) window procedure.
#[cfg(windows)]
#[inline]
unsafe fn call_original(
    hwnd: windows_sys::Win32::Foundation::HWND,
    msg: u32,
    wparam: usize,
    lparam: isize,
) -> isize {
    use windows_sys::Win32::UI::WindowsAndMessaging::{CallWindowProcW, DefWindowProcW, WNDPROC};

    let original = ORIGINAL_WNDPROC.load(Ordering::Acquire);
    if original != 0 {
        let proc: WNDPROC = Some(std::mem::transmute::<
            isize,
            unsafe extern "system" fn(
                windows_sys::Win32::Foundation::HWND,
                u32,
                usize,
                isize,
            ) -> isize,
        >(original));
        CallWindowProcW(proc, hwnd, msg, wparam, lparam)
    } else {
        DefWindowProcW(hwnd, msg, wparam, lparam)
    }
}

// ---------------------------------------------------------------------------
// Multi-monitor window position persistence & restoration
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct RectI32 {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
}

impl RectI32 {
    fn intersection_area(self, other: Self) -> i64 {
        let w = i64::from(self.right.min(other.right) - self.left.max(other.left));
        let h = i64::from(self.bottom.min(other.bottom) - self.top.max(other.top));
        if w > 0 && h > 0 {
            w * h
        } else {
            0
        }
    }

    const fn contains_with_margin(self, x: i32, y: i32, margin: i32) -> bool {
        x >= self.left - margin
            && x <= self.right + margin
            && y >= self.top - margin
            && y <= self.bottom + margin
    }
    fn clamp_to_work_area(x: i32, y: i32, work: RectI32) -> [i32; 2] {
        let min_x = work.left - 16;
        let max_x = (work.right - 120).max(min_x);
        let min_y = work.top - 8;
        let max_y = (work.bottom - 60).max(min_y);
        [x.clamp(min_x, max_x), y.clamp(min_y, max_y)]
    }
}

/// Validates and lightly clamps a saved physical outer window position against connected monitors.
/// Returns `None` if no connected monitor intersects the saved position (e.g. a monitor was unplugged).
fn validate_physical_position_against_monitors(
    monitors: &[(RectI32, RectI32)],
    pos: [i32; 2],
    size_px: [i32; 2],
) -> Option<[i32; 2]> {
    if monitors.is_empty() {
        return Some(pos);
    }

    let win_rect = RectI32 {
        left: pos[0],
        top: pos[1],
        right: pos[0].saturating_add(size_px[0].max(200)),
        bottom: pos[1].saturating_add(size_px[1].max(150)),
    };

    let mut best_idx: Option<usize> = None;
    let mut best_area: i64 = 0;
    for (idx, &(mon, _work)) in monitors.iter().enumerate() {
        let area = win_rect.intersection_area(mon);
        if area > best_area {
            best_area = area;
            best_idx = Some(idx);
        }
    }

    if best_idx.is_none() {
        best_idx = monitors
            .iter()
            .position(|&(mon, _work)| mon.contains_with_margin(pos[0], pos[1], 16));
    }

    let (_, work) = monitors[best_idx?];
    Some(RectI32::clamp_to_work_area(pos[0], pos[1], work))
}

#[cfg(windows)]
fn enumerate_monitors() -> Vec<(RectI32, RectI32)> {
    use windows_sys::core::BOOL;
    use windows_sys::Win32::Foundation::{LPARAM, RECT};
    use windows_sys::Win32::Graphics::Gdi::{
        EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFO,
    };

    unsafe extern "system" fn collect_monitor(
        hmonitor: HMONITOR,
        _hdc: HDC,
        _rect: *mut RECT,
        lparam: LPARAM,
    ) -> BOOL {
        let monitors = &mut *(lparam as *mut Vec<(RectI32, RectI32)>);
        let mut info: MONITORINFO = std::mem::zeroed();
        info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
        if GetMonitorInfoW(hmonitor, &raw mut info) != 0 {
            let mon = RectI32 {
                left: info.rcMonitor.left,
                top: info.rcMonitor.top,
                right: info.rcMonitor.right,
                bottom: info.rcMonitor.bottom,
            };
            let work = RectI32 {
                left: info.rcWork.left,
                top: info.rcWork.top,
                right: info.rcWork.right,
                bottom: info.rcWork.bottom,
            };
            monitors.push((mon, work));
        }
        1
    }

    let mut monitors: Vec<(RectI32, RectI32)> = Vec::new();
    unsafe {
        EnumDisplayMonitors(
            std::ptr::null_mut(),
            std::ptr::null(),
            Some(collect_monitor),
            std::ptr::from_mut(&mut monitors) as LPARAM,
        );
    }
    monitors
}

#[cfg(windows)]
fn validated_startup_physical_position(
    geom: &crate::ui::settings::WindowGeometryState,
) -> Option<[i32; 2]> {
    let phys_pos = geom.physical_position?;
    let monitors = enumerate_monitors();
    let size_px = geom.physical_inner_size.map_or_else(
        || [geom.inner_size[0] as i32, geom.inner_size[1] as i32],
        |s| [s[0] as i32, s[1] as i32],
    );
    validate_physical_position_against_monitors(&monitors, phys_pos, size_px)
}

/// Computes the logical startup position to pass to `ViewportBuilder::with_position`.
///
/// On Windows, `winit` creates the window on the primary monitor first and converts
/// `LogicalPosition` to physical pixels using the *primary monitor's* DPI scale factor.
/// Passing `physical_position / primary_dpi_scale` ensures `winit` moves the window
/// directly to the saved monitor (even when secondary and primary monitors have different DPIs).
#[must_use]
pub fn startup_logical_position(
    geom: &crate::ui::settings::WindowGeometryState,
) -> Option<[f32; 2]> {
    #[cfg(windows)]
    {
        if let Some(phys_pos) = validated_startup_physical_position(geom) {
            let primary_scale = windows::get_primary_monitor_dpi_scale();
            return Some([
                phys_pos[0] as f32 / primary_scale,
                phys_pos[1] as f32 / primary_scale,
            ]);
        }
        if geom.physical_position.is_some() {
            // Saved physical position was off-screen (e.g. secondary monitor disconnected);
            // let Windows place the window cleanly on an active monitor.
            return None;
        }
    }
    geom.position
}

/// Ensures on the first frame that the native Windows HWND is on the saved monitor
/// (and maximized if requested), overriding any `winit` cross-DPI placement nudge.
#[cfg(windows)]
pub(crate) fn apply_initial_window_placement(
    hwnd: isize,
    geom: &crate::ui::settings::WindowGeometryState,
) {
    use windows_sys::Win32::Foundation::{HWND, POINT, RECT};
    use windows_sys::Win32::Graphics::Gdi::{
        MonitorFromPoint, MonitorFromWindow, MONITOR_DEFAULTTONEAREST,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetWindowRect, IsZoomed, SetWindowPos, ShowWindow, SWP_NOACTIVATE, SWP_NOSIZE,
        SWP_NOZORDER, SW_MAXIMIZE, SW_RESTORE,
    };

    let Some([px, py]) = validated_startup_physical_position(geom) else {
        return;
    };

    let hwnd = hwnd as HWND;
    unsafe {
        let target_hmon = MonitorFromPoint(
            POINT {
                x: px + 60,
                y: py + 40,
            },
            MONITOR_DEFAULTTONEAREST,
        );
        let curr_hmon = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
        let is_zoomed = IsZoomed(hwnd) != 0;

        if geom.maximized {
            if curr_hmon != target_hmon || !is_zoomed {
                if is_zoomed {
                    ShowWindow(hwnd, SW_RESTORE);
                }
                SetWindowPos(
                    hwnd,
                    std::ptr::null_mut(),
                    px,
                    py,
                    0,
                    0,
                    SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
                );
                ShowWindow(hwnd, SW_MAXIMIZE);
            }
        } else {
            let mut rect: RECT = std::mem::zeroed();
            let pos_differs = if GetWindowRect(hwnd, &raw mut rect) != 0 {
                (rect.left - px).abs() > 2 || (rect.top - py).abs() > 2
            } else {
                true
            };
            if curr_hmon != target_hmon || pos_differs {
                SetWindowPos(
                    hwnd,
                    std::ptr::null_mut(),
                    px,
                    py,
                    0,
                    0,
                    SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
                );
            }
        }
    }
}

/// Captures the current physical window position and maximized state directly from Win32.
///
/// Returns `None` when the window is minimized (so saved coordinates are never overwritten by `-32000`),
/// or `Some(changed)` indicating whether `geom` was modified.
#[cfg(windows)]
pub(crate) fn capture_window_geometry_windows(
    hwnd: isize,
    geom: &mut crate::ui::settings::WindowGeometryState,
) -> Option<bool> {
    use windows_sys::Win32::Foundation::{HWND, RECT};
    use windows_sys::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MonitorFromRect, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetWindowPlacement, GetWindowRect, IsIconic, IsZoomed, WINDOWPLACEMENT,
    };

    let hwnd = hwnd as HWND;
    unsafe {
        if IsIconic(hwnd) != 0 {
            return None;
        }

        let mut changed = false;
        let is_zoomed = IsZoomed(hwnd) != 0;
        if geom.maximized != is_zoomed {
            geom.maximized = is_zoomed;
            changed = true;
        }

        if is_zoomed {
            // Even while maximized, track which monitor the window occupies so moving a
            // maximized window to another monitor (e.g. Win+Shift+Left or snap) persists.
            let curr_hmon = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
            let mut mi: MONITORINFO = std::mem::zeroed();
            mi.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
            if !curr_hmon.is_null() && GetMonitorInfoW(curr_hmon, &raw mut mi) != 0 {
                let mut wp: WINDOWPLACEMENT = std::mem::zeroed();
                wp.length = std::mem::size_of::<WINDOWPLACEMENT>() as u32;
                let mut target_pos = [mi.rcWork.left + 40, mi.rcWork.top + 40];
                if GetWindowPlacement(hwnd, &raw mut wp) != 0 {
                    let dx = mi.rcWork.left - mi.rcMonitor.left;
                    let dy = mi.rcWork.top - mi.rcMonitor.top;
                    let norm_screen = RECT {
                        left: wp.rcNormalPosition.left + dx,
                        top: wp.rcNormalPosition.top + dy,
                        right: wp.rcNormalPosition.right + dx,
                        bottom: wp.rcNormalPosition.bottom + dy,
                    };
                    if MonitorFromRect(&raw const norm_screen, MONITOR_DEFAULTTONEAREST)
                        == curr_hmon
                        && norm_screen.left > -30_000
                        && norm_screen.top > -30_000
                    {
                        let work = RectI32 {
                            left: mi.rcWork.left,
                            top: mi.rcWork.top,
                            right: mi.rcWork.right,
                            bottom: mi.rcWork.bottom,
                        };
                        target_pos =
                            RectI32::clamp_to_work_area(norm_screen.left, norm_screen.top, work);
                    }
                }
                if geom.physical_position != Some(target_pos) {
                    geom.physical_position = Some(target_pos);
                    let dpi_scale =
                        windows::get_dpi_scale_at_point(target_pos[0] + 60, target_pos[1] + 40);
                    geom.position = Some([
                        target_pos[0] as f32 / dpi_scale,
                        target_pos[1] as f32 / dpi_scale,
                    ]);
                    changed = true;
                }
            }
        } else {
            let mut rect: RECT = std::mem::zeroed();
            if GetWindowRect(hwnd, &raw mut rect) != 0 && rect.left > -30_000 && rect.top > -30_000
            {
                let phys_pos = [rect.left, rect.top];
                if geom.physical_position != Some(phys_pos) {
                    geom.physical_position = Some(phys_pos);
                    changed = true;
                }
            }
        }

        Some(changed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_physical_position_dual_monitor_left_secondary_right_primary() {
        // Left monitor: 2K 100% (-2560..0, 0..1440)
        // Right monitor (main): 4K 150% (0..3840, 0..2160)
        let monitors = vec![
            (
                RectI32 {
                    left: -2560,
                    top: 0,
                    right: 0,
                    bottom: 1440,
                },
                RectI32 {
                    left: -2560,
                    top: 0,
                    right: 0,
                    bottom: 1400,
                },
            ),
            (
                RectI32 {
                    left: 0,
                    top: 0,
                    right: 3840,
                    bottom: 2160,
                },
                RectI32 {
                    left: 0,
                    top: 0,
                    right: 3840,
                    bottom: 2100,
                },
            ),
        ];

        // Normal position on left monitor
        assert_eq!(
            validate_physical_position_against_monitors(&monitors, [-1800, 150], [1100, 750]),
            Some([-1800, 150])
        );

        // Snapped to left edge of left monitor (invisible 8px border extends to -2568)
        assert_eq!(
            validate_physical_position_against_monitors(&monitors, [-2568, 0], [1280, 1400]),
            Some([-2568, 0])
        );

        // Disconnected left monitor: only right monitor remains -> returns None
        let single_monitor = vec![monitors[1]];
        assert_eq!(
            validate_physical_position_against_monitors(&single_monitor, [-1800, 150], [1100, 750]),
            None
        );
    }
}
