//! The login code as a grid of modules, ready for a view to paint as squares.
//!
//! The backend sends the text WhatsApp's phone app scans (`Unlinked { qr }`),
//! which changes every few seconds while it waits. Turning it into modules is
//! the only work here; the egui app does the same with the `qrcode` crate and
//! paints textures, this one leaves the painting to GPUI.

use qrcode::{Color, EcLevel, QrCode};

/// Dark and light modules of a QR code, row by row, without the quiet zone.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QrModules {
    /// Modules on a side.
    pub width: usize,
    dark: Vec<bool>,
}

/// The modules of white around a code that a scanner needs to find it.
pub const QUIET_ZONE: usize = 4;

impl QrModules {
    /// The code for `text`, or `None` when it does not fit in a QR code.
    pub fn from_text(text: &str) -> Option<Self> {
        // Low correction keeps the code small: the long text WhatsApp sends
        // scans better with larger modules than with redundancy.
        let code = QrCode::with_error_correction_level(text.as_bytes(), EcLevel::L).ok()?;
        let width = code.width();
        let dark = code
            .into_colors()
            .into_iter()
            .map(|color| color == Color::Dark)
            .collect();
        Some(Self { width, dark })
    }

    /// Whether the module at (`x`, `y`) is dark. Outside the code is light.
    pub fn is_dark(&self, x: usize, y: usize) -> bool {
        x < self.width && y < self.width && self.dark[y * self.width + x]
    }

    /// The dark modules as `(x, y)`, for a view that paints one square each.
    pub fn dark_modules(&self) -> impl Iterator<Item = (usize, usize)> + '_ {
        self.dark
            .iter()
            .enumerate()
            .filter(|(_, dark)| **dark)
            .map(|(index, _)| (index % self.width, index / self.width))
    }

    /// Side of the code with its quiet zone, in modules.
    pub fn width_with_quiet_zone(&self) -> usize {
        self.width + 2 * QUIET_ZONE
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_short_text_is_a_version_one_code_with_the_finder_patterns() {
        let code = QrModules::from_text("hello").unwrap();
        assert_eq!(code.width, 21);
        // The three 7x7 finder squares are dark on their outline and center.
        for (x0, y0) in [(0, 0), (14, 0), (0, 14)] {
            for i in 0..7 {
                assert!(code.is_dark(x0 + i, y0), "top edge {x0},{y0},{i}");
                assert!(code.is_dark(x0, y0 + i), "left edge");
                assert!(code.is_dark(x0 + i, y0 + 6), "bottom edge");
                assert!(code.is_dark(x0 + 6, y0 + i), "right edge");
            }
            assert!(code.is_dark(x0 + 3, y0 + 3), "center");
            assert!(!code.is_dark(x0 + 1, y0 + 1), "gap");
        }
        assert_eq!(code.width_with_quiet_zone(), 29);
    }

    #[test]
    fn modules_outside_are_light_and_the_iterator_agrees() {
        let code = QrModules::from_text("2@abc,def,ghi==,jkl==").unwrap();
        assert!(!code.is_dark(code.width, 0));
        assert!(!code.is_dark(0, code.width));
        let listed = code.dark_modules().count();
        let counted = (0..code.width)
            .flat_map(|y| (0..code.width).map(move |x| (x, y)))
            .filter(|&(x, y)| code.is_dark(x, y))
            .count();
        assert_eq!(listed, counted);
        assert!(listed > 0);
    }

    #[test]
    fn a_text_too_long_for_a_code_gives_none() {
        assert!(QrModules::from_text(&"x".repeat(8000)).is_none());
    }

    #[test]
    fn a_new_text_is_a_different_code() {
        assert_ne!(
            QrModules::from_text("2@one").unwrap(),
            QrModules::from_text("2@two").unwrap()
        );
    }
}
