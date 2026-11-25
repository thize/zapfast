//! Chat pictures: the file the backend saved, decoded off the UI thread and
//! shrunk to what an avatar needs, as the BGRA frame GPUI paints.

use gpui::{Context, RenderImage};
use image::{Delay, Frame, RgbaImage};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use zapfast::pixels::PixelImage;

/// The largest side kept, in pixels: twice the largest avatar drawn (40 points)
/// for a 2x display.
pub const MAX_SIDE: u32 = 96;

/// The pictures decoded so far, by file. A picture is decoded once, on a
/// background thread, and the views that observe the cache draw again when it
/// arrives; a file that cannot be read is not tried again.
#[derive(Default)]
pub struct Avatars {
    pictures: HashMap<PathBuf, Option<Arc<RenderImage>>>,
    loading: HashSet<PathBuf>,
}

impl Avatars {
    /// The picture of `path` if it is ready; otherwise starts decoding it.
    pub fn get(&mut self, path: &Path, cx: &mut Context<Self>) -> Option<Arc<RenderImage>> {
        if let Some(ready) = self.pictures.get(path) {
            return ready.clone();
        }
        if self.loading.insert(path.to_owned()) {
            let path = path.to_owned();
            cx.spawn(async move |this, cx| {
                let file = path.clone();
                let image = cx
                    .background_executor()
                    .spawn(async move { load(&file) })
                    .await;
                this.update(cx, |this, cx| {
                    this.loading.remove(&path);
                    this.pictures.insert(path, image);
                    cx.notify();
                })
                .ok();
            })
            .detach();
        }
        None
    }
}

/// Decodes the picture at `path`.
pub fn load(path: &Path) -> Option<Arc<RenderImage>> {
    let decoded = image::open(path).ok()?;
    Some(from_rgba(decoded.thumbnail(MAX_SIDE, MAX_SIDE).to_rgba8()))
}

/// A picture the core produced (`pixels::PixelImage`: premultiplied RGBA).
pub fn from_pixels(image: &PixelImage) -> Option<Arc<RenderImage>> {
    let mut bgra = Vec::with_capacity(image.pixels.len() * 4);
    for pixel in &image.pixels {
        let [r, g, b, a] = pixel.to_srgba_unmultiplied();
        bgra.extend_from_slice(&[b, g, r, a]);
    }
    let raw = RgbaImage::from_raw(image.width() as u32, image.height() as u32, bgra)?;
    Some(frame(raw))
}

/// GPUI's pixel order is BGRA with straight alpha, so red and blue swap.
fn from_rgba(mut rgba: RgbaImage) -> Arc<RenderImage> {
    for pixel in rgba.pixels_mut() {
        pixel.0.swap(0, 2);
    }
    frame(rgba)
}

fn frame(bgra: RgbaImage) -> Arc<RenderImage> {
    Arc::new(RenderImage::new(vec![Frame::from_parts(
        bgra,
        0,
        0,
        Delay::from_numer_denom_ms(100, 1),
    )]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use zapfast::pixels::Pixel;

    #[test]
    fn a_picture_file_is_shrunk_and_swapped_to_bgra() {
        let dir = tempfile_dir();
        let path = dir.join("a.png");
        let mut source = RgbaImage::new(400, 200);
        for pixel in source.pixels_mut() {
            *pixel = image::Rgba([255, 0, 0, 255]);
        }
        source.save(&path).unwrap();
        let image = load(&path).expect("decodes");
        let size = image.size(0);
        assert_eq!(size.width.0, 96);
        assert_eq!(size.height.0, 48);
        // Red in RGBA is blue-first in BGRA.
        assert_eq!(&image.as_bytes(0).unwrap()[..4], &[0, 0, 255, 255]);
        assert!(load(&dir.join("missing.png")).is_none());
    }

    #[test]
    fn core_pixels_become_straight_bgra() {
        let image = PixelImage::filled([2, 1], Pixel::from_rgb(10, 20, 30));
        let render = from_pixels(&image).expect("converts");
        assert_eq!(&render.as_bytes(0).unwrap()[..4], &[30, 20, 10, 255]);
    }

    fn tempfile_dir() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("zapfast-avatar-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }
}
