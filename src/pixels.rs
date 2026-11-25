//! Pictures the backend produces, with no interface framework behind them.
//!
//! [`PixelImage`] keeps the memory layout and the colour meaning of egui's
//! `ColorImage`: rows from top to bottom of premultiplied sRGBA pixels. With
//! the `egui-ui` feature a picture crosses to egui and back without copying its
//! pixels, so the window uploads exactly what the worker made.

use std::fmt;

/// One premultiplied sRGBA pixel, laid out as egui's `Color32` is (four bytes,
/// aligned to four), so a vector of them can become a vector of colours.
#[repr(C)]
#[repr(align(4))]
#[derive(Clone, Copy, Default, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "egui-ui", derive(bytemuck::Pod, bytemuck::Zeroable))]
pub struct Pixel(pub [u8; 4]);

impl fmt::Debug for Pixel {
    /// Prints the contents with premultiplied alpha, as `Color32` does.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let [r, g, b, a] = self.0;
        write!(f, "#{r:02X}_{g:02X}_{b:02X}_{a:02X}")
    }
}

/// `val * (frac / 255)`, rounded, in integers. The same arithmetic as ecolor's,
/// so a picture premultiplies to the same bytes with or without egui.
const fn mul_frac_round(val: u8, frac: u8) -> u8 {
    let p = (val as u16) * (frac as u16) + 128;
    ((p + (p >> 8)) >> 8) as u8
}

impl Pixel {
    pub const TRANSPARENT: Self = Self([0; 4]);

    /// An opaque colour.
    pub const fn from_rgb(r: u8, g: u8, b: u8) -> Self {
        Self([r, g, b, 255])
    }

    /// From colours that alpha has already been multiplied into.
    pub const fn from_rgba_premultiplied(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self([r, g, b, a])
    }

    /// From a "normal" RGBA value with the alpha kept apart.
    pub const fn from_rgba_unmultiplied(r: u8, g: u8, b: u8, a: u8) -> Self {
        match a {
            0 => Self::TRANSPARENT,
            255 => Self::from_rgb(r, g, b),
            a => Self([
                mul_frac_round(r, a),
                mul_frac_round(g, a),
                mul_frac_round(b, a),
                a,
            ]),
        }
    }

    /// Red, multiplied by alpha.
    pub const fn r(self) -> u8 {
        self.0[0]
    }

    /// Green, multiplied by alpha.
    pub const fn g(self) -> u8 {
        self.0[1]
    }

    /// Blue, multiplied by alpha.
    pub const fn b(self) -> u8 {
        self.0[2]
    }

    /// Alpha: how opaque the pixel is.
    pub const fn a(self) -> u8 {
        self.0[3]
    }

    /// The premultiplied bytes.
    pub const fn to_array(self) -> [u8; 4] {
        self.0
    }

    /// The colour with its alpha divided out again: the inverse of
    /// [`Self::from_rgba_unmultiplied`], give or take rounding in what is
    /// transparent.
    pub fn to_srgba_unmultiplied(self) -> [u8; 4] {
        let [r, g, b, a] = self.0;
        match a {
            0 | 255 => self.0,
            a => {
                let factor = 255.0 / f32::from(a);
                let divide = |channel: u8| (factor * f32::from(channel) + 0.5) as u8;
                [divide(r), divide(g), divide(b), a]
            }
        }
    }
}

/// A picture in memory: `size` is width and height, and `pixels` run row by row
/// from the top.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct PixelImage {
    pub size: [usize; 2],
    pub pixels: Vec<Pixel>,
}

impl fmt::Debug for PixelImage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PixelImage")
            .field("size", &self.size)
            .field("pixel-count", &self.pixels.len())
            .finish_non_exhaustive()
    }
}

impl PixelImage {
    /// A picture of `size` made of `pixels`, which must fill it exactly.
    pub fn new(size: [usize; 2], pixels: Vec<Pixel>) -> Self {
        debug_assert!(
            size[0] * size[1] == pixels.len(),
            "size: {size:?}, pixels.len(): {}",
            pixels.len()
        );
        Self { size, pixels }
    }

    /// Width in pixels.
    pub fn width(&self) -> usize {
        self.size[0]
    }

    /// Height in pixels.
    pub fn height(&self) -> usize {
        self.size[1]
    }

    /// A picture of `size` in one colour.
    pub fn filled(size: [usize; 2], pixel: Pixel) -> Self {
        Self::new(size, vec![pixel; size[0] * size[1]])
    }

    /// From flat RGBA bytes with the alpha kept apart. Panics unless `rgba` is
    /// exactly four bytes for each pixel of `size`.
    pub fn from_rgba_unmultiplied(size: [usize; 2], rgba: &[u8]) -> Self {
        assert_eq!(
            size[0] * size[1] * 4,
            rgba.len(),
            "size: {size:?}, rgba.len(): {}",
            rgba.len()
        );
        let pixels = rgba
            .as_chunks::<4>()
            .0
            .iter()
            .map(|p| Pixel::from_rgba_unmultiplied(p[0], p[1], p[2], p[3]))
            .collect();
        Self::new(size, pixels)
    }

    /// From flat RGBA bytes that alpha has already been multiplied into. Panics
    /// unless `rgba` is exactly four bytes for each pixel of `size`.
    pub fn from_rgba_premultiplied(size: [usize; 2], rgba: &[u8]) -> Self {
        assert_eq!(
            size[0] * size[1] * 4,
            rgba.len(),
            "size: {size:?}, rgba.len(): {}",
            rgba.len()
        );
        let pixels = rgba
            .as_chunks::<4>()
            .0
            .iter()
            .map(|p| Pixel::from_rgba_premultiplied(p[0], p[1], p[2], p[3]))
            .collect();
        Self::new(size, pixels)
    }
}

/// Zero-copy moves between these types and egui's.
#[cfg(feature = "egui-ui")]
mod egui_images {
    use std::sync::Arc;

    use super::{Pixel, PixelImage};

    impl From<egui::Color32> for Pixel {
        fn from(color: egui::Color32) -> Self {
            Self(color.to_array())
        }
    }

    impl From<Pixel> for egui::Color32 {
        fn from(pixel: Pixel) -> Self {
            let [r, g, b, a] = pixel.0;
            Self::from_rgba_premultiplied(r, g, b, a)
        }
    }

    /// The pixel buffer is reused as it is: `Pixel` and `Color32` are both four
    /// bytes aligned to four, so the vector's allocation is valid for either.
    impl From<PixelImage> for egui::ColorImage {
        fn from(image: PixelImage) -> Self {
            Self::new(image.size, bytemuck::cast_vec(image.pixels))
        }
    }

    /// The pixel buffer is reused as it is. egui's `source_size`, which only an
    /// SVG raster sets apart from the pixels' own size, is not kept.
    impl From<egui::ColorImage> for PixelImage {
        fn from(image: egui::ColorImage) -> Self {
            Self::new(image.size, bytemuck::cast_vec(image.pixels))
        }
    }

    impl PixelImage {
        /// A shared picture as egui's: its pixels are reused when this is the
        /// only handle on them, and copied otherwise.
        pub fn into_shared_color_image(image: Arc<Self>) -> Arc<egui::ColorImage> {
            Arc::new(
                Arc::try_unwrap(image)
                    .unwrap_or_else(|shared| (*shared).clone())
                    .into(),
            )
        }

        /// The other way: a shared egui picture as ours.
        pub fn from_shared_color_image(image: Arc<egui::ColorImage>) -> Arc<Self> {
            Arc::new(
                Arc::try_unwrap(image)
                    .unwrap_or_else(|shared| (*shared).clone())
                    .into(),
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pixel_is_four_bytes_aligned_to_four() {
        assert_eq!(std::mem::size_of::<Pixel>(), 4);
        assert_eq!(std::mem::align_of::<Pixel>(), 4);
    }

    #[test]
    fn unmultiplied_colours_premultiply_and_come_back() {
        assert_eq!(
            Pixel::from_rgba_unmultiplied(9, 8, 7, 0),
            Pixel::TRANSPARENT
        );
        assert_eq!(
            Pixel::from_rgba_unmultiplied(9, 8, 7, 255),
            Pixel::from_rgb(9, 8, 7)
        );
        let half = Pixel::from_rgba_unmultiplied(200, 100, 50, 128);
        assert_eq!(half.to_array(), [100, 50, 25, 128]);
        assert_eq!(half.to_srgba_unmultiplied(), [199, 100, 50, 128]);
    }

    #[test]
    fn images_are_built_from_flat_bytes() {
        let flat = [10, 20, 30, 255, 40, 50, 60, 0];
        let image = PixelImage::from_rgba_unmultiplied([2, 1], &flat);
        assert_eq!(image.size, [2, 1]);
        assert_eq!(
            image.pixels,
            [Pixel::from_rgb(10, 20, 30), Pixel::TRANSPARENT]
        );
        let image = PixelImage::from_rgba_premultiplied([2, 1], &flat);
        assert_eq!(image.pixels[1].to_array(), [40, 50, 60, 0]);
        assert_eq!(
            PixelImage::filled([2, 3], Pixel::TRANSPARENT).pixels.len(),
            6
        );
    }

    #[test]
    #[should_panic(expected = "rgba.len()")]
    fn flat_bytes_must_fill_the_picture() {
        PixelImage::from_rgba_unmultiplied([2, 2], &[0; 4]);
    }

    /// Colours made apart from egui and by it premultiply to the same bytes, so
    /// a picture does not change when the worker stops depending on egui.
    #[cfg(feature = "egui-ui")]
    #[test]
    fn premultiplying_matches_egui_for_every_alpha_and_a_spread_of_colours() {
        for a in 0..=255u8 {
            for r in (0..=255u8).step_by(3) {
                for (g, b) in [(0, 255), (17, 130), (255, 1), (128, 128)] {
                    assert_eq!(
                        Pixel::from_rgba_unmultiplied(r, g, b, a).to_array(),
                        egui::Color32::from_rgba_unmultiplied(r, g, b, a).to_array()
                    );
                }
            }
        }
        for a in 0..=255u8 {
            for c in (0..=a).step_by(5) {
                assert_eq!(
                    Pixel::from_rgba_premultiplied(c, a, c, a).to_srgba_unmultiplied(),
                    egui::Color32::from_rgba_premultiplied(c, a, c, a).to_srgba_unmultiplied()
                );
            }
        }
    }

    #[cfg(feature = "egui-ui")]
    #[test]
    fn a_picture_crosses_to_egui_and_back_in_the_same_buffer() {
        let image = PixelImage::from_rgba_unmultiplied(
            [3, 2],
            &[
                1, 2, 3, 255, 4, 5, 6, 128, 7, 8, 9, 0, //
                10, 11, 12, 77, 13, 14, 15, 255, 16, 17, 18, 1,
            ],
        );
        let expected: Vec<[u8; 4]> = image.pixels.iter().map(|pixel| pixel.to_array()).collect();
        let moved = image.clone();
        let (pointer, capacity) = (moved.pixels.as_ptr().cast::<u8>(), moved.pixels.capacity());

        let color = egui::ColorImage::from(moved);
        assert_eq!(color.pixels.as_ptr().cast::<u8>(), pointer);
        assert_eq!(color.pixels.capacity(), capacity);
        assert_eq!(color.size, [3, 2]);
        assert_eq!(color.source_size, egui::vec2(3.0, 2.0));
        assert_eq!(
            color
                .pixels
                .iter()
                .map(|color| color.to_array())
                .collect::<Vec<_>>(),
            expected
        );

        let back = PixelImage::from(color);
        assert_eq!(back.pixels.as_ptr().cast::<u8>(), pointer);
        assert_eq!(back, image);
    }

    #[cfg(feature = "egui-ui")]
    #[test]
    fn a_shared_picture_is_reused_when_it_is_alone_and_copied_when_it_is_not() {
        use std::sync::Arc;

        let image = Arc::new(PixelImage::filled([4, 4], Pixel::from_rgb(1, 2, 3)));
        let pointer = image.pixels.as_ptr().cast::<u8>();
        let color = PixelImage::into_shared_color_image(image);
        assert_eq!(color.pixels.as_ptr().cast::<u8>(), pointer);

        let other = Arc::clone(&color);
        let copied = PixelImage::from_shared_color_image(color);
        assert_ne!(copied.pixels.as_ptr().cast::<u8>(), pointer);
        assert_eq!(copied.pixels.len(), 16);
        drop(other);
    }

    #[cfg(feature = "egui-ui")]
    #[test]
    fn single_pixels_convert_both_ways() {
        let pixel = Pixel::from_rgba_premultiplied(1, 2, 3, 4);
        let color = egui::Color32::from(pixel);
        assert_eq!(color.to_array(), [1, 2, 3, 4]);
        assert_eq!(Pixel::from(color), pixel);
    }
}
