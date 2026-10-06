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
    use windows_sys::Win32::Graphics::Gdi::{MonitorFromPoint, HMONITOR, MONITOR_DEFAULTTONEAREST};
    use windows_sys::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};

    /// Queries the DPI scale factor (e.g. 1.0, 1.5) for a given screen point (x, y).
    /// Used at startup to determine the DPI of the monitor where the window will appear.
    pub fn get_dpi_scale_at_point(x: i32, y: i32) -> f32 {
        unsafe {
            let pt = windows_sys::Win32::Foundation::POINT { x, y };
            let hmonitor: HMONITOR = MonitorFromPoint(pt, MONITOR_DEFAULTTONEAREST);
            if !hmonitor.is_null() {
                let mut dpi_x = 0;
                let mut dpi_y = 0;
                if GetDpiForMonitor(hmonitor, MDT_EFFECTIVE_DPI, &raw mut dpi_x, &raw mut dpi_y) == 0
                    && dpi_x > 0 {
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
    let old = unsafe { SetWindowLongPtrW(hwnd, GWL_WNDPROC, dpi_fix_wndproc as *const () as isize) };
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
                let proc: WNDPROC = Some(std::mem::transmute(original));
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
        let proc: WNDPROC = Some(std::mem::transmute(original));
        CallWindowProcW(proc, hwnd, msg, wparam, lparam)
    } else {
        DefWindowProcW(hwnd, msg, wparam, lparam)
    }
}
