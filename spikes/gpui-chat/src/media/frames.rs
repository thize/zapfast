//! From decoded pixels to what GPUI paints, and the size of a media bubble.

use gpui::RenderImage;
use image::{Delay, Frame, RgbaImage};
use std::sync::Arc;
use std::time::Duration;

/// Widest a media bubble gets, in logical pixels (WhatsApp's is about 330).
pub const BUBBLE_WIDTH: f32 = 300.0;
/// Tallest: a portrait picture is cropped to this (object-fit: cover).
pub const BUBBLE_MAX_HEIGHT: f32 = 340.0;
/// Shortest: a very wide panorama is not squashed below this.
pub const BUBBLE_MIN_HEIGHT: f32 = 120.0;

/// The box a picture of `width` x `height` pixels gets in a bubble: always
/// [`BUBBLE_WIDTH`] wide, as tall as its aspect ratio says, clamped between
/// [`BUBBLE_MIN_HEIGHT`] and [`BUBBLE_MAX_HEIGHT`]. When the clamp bites, the
/// picture is cover-cropped to the box.
pub fn bubble_size(width: u32, height: u32) -> (f32, f32) {
    if width == 0 || height == 0 {
        return (BUBBLE_WIDTH, BUBBLE_WIDTH);
    }
    let natural = BUBBLE_WIDTH * height as f32 / width as f32;
    (
        BUBBLE_WIDTH,
        natural.clamp(BUBBLE_MIN_HEIGHT, BUBBLE_MAX_HEIGHT),
    )
}

/// GPUI's pixel order is BGRA with straight alpha (`decode_static_image`,
/// gpui-pre `platform.rs:3455`; the Metal blend is SourceAlpha,
/// `metal_renderer.rs:1439`). `rasterlottie` returns premultiplied RGBA, so
/// this swaps red and blue and divides the alpha out.
pub fn premultiplied_rgba_to_bgra(pixels: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(pixels.len());
    for pixel in pixels.as_chunks::<4>().0 {
        let alpha = u32::from(pixel[3]);
        let straight = |channel: u8| -> u8 {
            (u32::from(channel) * 255 + alpha / 2)
                .checked_div(alpha)
                .map_or(0, |v| v.min(255) as u8)
        };
        out.extend_from_slice(&[
            straight(pixel[2]),
            straight(pixel[1]),
            straight(pixel[0]),
            pixel[3],
        ]);
    }
    out
}

/// A GPUI image of one or more BGRA frames, each shown for its delay (GPUI's
/// `img()` plays a multi-frame image by itself).
pub fn render_image(frames: Vec<(RgbaImage, Duration)>) -> Arc<RenderImage> {
    let frames: Vec<Frame> = frames
        .into_iter()
        .map(|(bgra, delay)| Frame::from_parts(bgra, 0, 0, Delay::from_saturating_duration(delay)))
        .collect();
    Arc::new(RenderImage::new(frames))
}

/// One BGRA frame from raw bytes (`width * height * 4` of them).
pub fn render_image_from_bgra(width: u32, height: u32, bgra: Vec<u8>) -> Option<Arc<RenderImage>> {
    let image = RgbaImage::from_raw(width, height, bgra)?;
    Some(render_image(vec![(image, Duration::from_millis(33))]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bubbles_follow_the_aspect_ratio_within_limits() {
        assert_eq!(bubble_size(960, 540), (300.0, 168.75));
        assert_eq!(bubble_size(600, 600), (300.0, 300.0));
        // Portrait: capped, so the picture is cropped.
        assert_eq!(bubble_size(540, 960), (300.0, BUBBLE_MAX_HEIGHT));
        // Panorama: floored.
        assert_eq!(bubble_size(3000, 400), (300.0, BUBBLE_MIN_HEIGHT));
        assert_eq!(bubble_size(0, 0), (300.0, 300.0));
    }

    #[test]
    fn colour_conversions_use_gpuis_pixel_order() {
        assert_eq!(
            premultiplied_rgba_to_bgra(&[10, 20, 30, 255]),
            vec![30, 20, 10, 255]
        );
        // Half-covered pure red, premultiplied: straight alpha restores it.
        let out = premultiplied_rgba_to_bgra(&[128, 0, 0, 128, 9, 9, 9, 0]);
        assert_eq!(out, vec![0, 0, 255, 128, 0, 0, 0, 0]);
    }

    #[test]
    fn a_render_image_keeps_frames_and_delays() {
        let a = RgbaImage::new(4, 2);
        let image = render_image(vec![
            (a.clone(), Duration::from_millis(100)),
            (a, Duration::from_millis(40)),
        ]);
        assert_eq!(image.frame_count(), 2);
        assert_eq!(Duration::from(image.delay(0)), Duration::from_millis(100));
        assert_eq!(Duration::from(image.delay(1)), Duration::from_millis(40));
        assert_eq!(image.size(0).width.0, 4);
        assert!(render_image_from_bgra(2, 2, vec![0; 3]).is_none());
        assert!(render_image_from_bgra(2, 2, vec![0; 16]).is_some());
    }
}
