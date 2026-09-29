//! Windows-specific Per-Monitor DPI scaling utilities.
//!
//! Provides monitor DPI detection for initial window sizing and placement
//! across multi-monitor setups (e.g. 4K 150% and 2K 100%).

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
                if GetDpiForMonitor(hmonitor, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y) == 0 {
                    if dpi_x > 0 {
                        return dpi_x as f32 / 96.0;
                    }
                }
            }
        }
        1.0
    }
}
