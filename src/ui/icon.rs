//! Application icon utilities.

use eframe::egui;

/// Embedded icon bytes (256x256 optimized PNG for fast startup and low memory usage).
const ICON_BYTES: &[u8] = include_bytes!("../../logo/icon-256.png");

/// Loads the application window icon from embedded PNG data.
#[must_use]
pub fn load_app_icon() -> Option<egui::IconData> {
    match image::load_from_memory(ICON_BYTES) {
        Ok(image) => {
            let rgba = image.to_rgba8();
            let (width, height) = rgba.dimensions();
            Some(egui::IconData {
                rgba: rgba.into_raw(),
                width,
                height,
            })
        }
        Err(e) => {
            log::warn!("Failed to decode application icon: {e}");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_app_icon() {
        let icon = load_app_icon().expect("App icon should be loaded successfully");
        assert_eq!(icon.width, 256);
        assert_eq!(icon.height, 256);
        assert_eq!(icon.rgba.len(), 256 * 256 * 4);
    }
}

