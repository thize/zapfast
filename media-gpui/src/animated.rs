//! A GIF or an animated WebP in its own view.
//!
//! GPUI's `img()` steps the frames itself and asks for a new animation frame
//! each time one is due, which re-renders the view it sits in. In a message
//! list that would be the whole list, so an animated picture is an entity shown
//! through [`cached`](crate::cached). While paused it shows its first frame,
//! decoded once off the main thread, and schedules nothing.

use crate::frames::{render_image, straight_rgba_to_bgra};
use crate::picture::{MediaPicture, MediaSource};
use gpui::{
    Context, ElementId, Hsla, IntoElement, ObjectFit, Render, RenderImage, SharedString,
    Styled as _, Task, Window, div,
};
use std::sync::Arc;
use std::time::Duration;

/// Decodes the first frame of a GIF or WebP (or any picture) as a GPUI image.
pub fn decode_poster(bytes: &[u8]) -> Option<Arc<RenderImage>> {
    let mut first = image::load_from_memory(bytes).ok()?.into_rgba8();
    straight_rgba_to_bgra(&mut first);
    Some(render_image(vec![(first, Duration::from_millis(100))]))
}

pub struct AnimatedPicture {
    id: ElementId,
    source: MediaSource,
    size: (f32, f32),
    sticker: bool,
    backdrop: Option<Hsla>,
    label: Option<SharedString>,
    playing: bool,
    poster: Option<Arc<RenderImage>>,
    loading_poster: Option<Task<()>>,
}

impl AnimatedPicture {
    /// A picture of `size` logical pixels, paused until [`set_playing`](Self::set_playing).
    pub fn new(id: impl Into<ElementId>, source: impl Into<MediaSource>, size: (f32, f32)) -> Self {
        Self {
            id: id.into(),
            source: source.into(),
            size,
            sticker: false,
            backdrop: None,
            label: None,
            playing: false,
            poster: None,
            loading_poster: None,
        }
    }

    /// No backdrop and contained, not cropped: transparent pixels show the chat.
    pub fn sticker(mut self) -> Self {
        self.sticker = true;
        self
    }

    pub fn backdrop(mut self, color: impl Into<Hsla>) -> Self {
        self.backdrop = Some(color.into());
        self
    }

    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
        self
    }

    pub fn is_playing(&self) -> bool {
        self.playing
    }

    /// Whether the first frame is ready to show while paused.
    pub fn has_poster(&self) -> bool {
        self.poster.is_some()
    }

    /// Plays or pauses. Pausing swaps in the first frame (decoded on first
    /// need), so nothing animates and nothing asks for a frame.
    pub fn set_playing(&mut self, playing: bool, cx: &mut Context<Self>) {
        if self.playing == playing {
            return;
        }
        self.playing = playing;
        if !playing {
            self.load_poster(cx);
        }
        cx.notify();
    }

    fn load_poster(&mut self, cx: &mut Context<Self>) {
        if self.poster.is_some() || self.loading_poster.is_some() {
            return;
        }
        let bytes = match &self.source {
            // Already one picture of pixels: it is its own poster.
            MediaSource::Image(image) => {
                self.poster = Some(image.clone());
                return;
            }
            MediaSource::Path(path) => PosterBytes::Path(path.clone()),
            MediaSource::Asset(name) => match cx.asset_source().load(name) {
                Ok(Some(data)) => PosterBytes::Data(data.into_owned()),
                _ => return,
            },
        };
        self.loading_poster = Some(cx.spawn(async move |this, cx| {
            let poster = cx
                .background_executor()
                .spawn(async move {
                    let data = match bytes {
                        PosterBytes::Path(path) => std::fs::read(path).ok()?,
                        PosterBytes::Data(data) => data,
                    };
                    decode_poster(&data)
                })
                .await;
            this.update(cx, |this, cx| {
                this.loading_poster = None;
                this.poster = poster;
                cx.notify();
            })
            .ok();
        }));
    }
}

enum PosterBytes {
    Path(Arc<std::path::Path>),
    Data(Vec<u8>),
}

impl Render for AnimatedPicture {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let (w, h) = self.size;
        let source = if self.playing {
            Some(self.source.clone())
        } else {
            self.poster.clone().map(MediaSource::Image)
        };
        let Some(source) = source else {
            // Paused and the first frame is still being decoded.
            return div().w(gpui::px(w)).h(gpui::px(h)).into_any_element();
        };
        let mut picture = MediaPicture::new(self.id.clone(), source, w, h);
        if self.sticker {
            picture = picture.radius(0.0).fit(ObjectFit::Contain);
        } else if let Some(color) = self.backdrop {
            picture = picture.backdrop(color);
        }
        if let Some(label) = self.label.clone() {
            picture = picture.label(label);
        }
        picture.into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frames::render_image_from_bgra;
    use gpui::{AppContext as _, TestAppContext};
    use image::codecs::gif::{GifEncoder, Repeat};
    use image::{Delay, Frame, Rgba, RgbaImage};

    /// A two-frame GIF: red, then blue.
    fn gif(path: &std::path::Path) {
        let mut encoder = GifEncoder::new(std::fs::File::create(path).expect("gif file"));
        encoder.set_repeat(Repeat::Infinite).expect("repeat");
        for colour in [[255, 0, 0, 255], [0, 0, 255, 255]] {
            let image = RgbaImage::from_pixel(8, 8, Rgba(colour));
            encoder
                .encode_frame(Frame::from_parts(
                    image,
                    0,
                    0,
                    Delay::from_numer_denom_ms(80, 1),
                ))
                .expect("frame");
        }
    }

    #[test]
    fn the_poster_is_the_first_frame_in_gpuis_pixel_order() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("a.gif");
        gif(&path);
        let poster = decode_poster(&std::fs::read(&path).expect("read")).expect("a poster");
        assert_eq!(poster.frame_count(), 1);
        // Red first; BGRA puts it last of the three colour bytes.
        assert_eq!(poster.as_bytes(0).expect("pixels")[..4], [0, 0, 255, 255]);
        assert!(decode_poster(b"not a picture").is_none());
    }

    #[gpui::test]
    fn pausing_shows_the_first_frame_and_playing_shows_the_source(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("a.gif");
        gif(&path);
        let picture = cx.new(|_| AnimatedPicture::new("gif", path, (64.0, 64.0)));
        assert!(!picture.read_with(cx, |p, _| p.is_playing()));
        assert!(!picture.read_with(cx, |p, _| p.has_poster()));
        picture.update(cx, |p, cx| p.set_playing(true, cx));
        assert!(picture.read_with(cx, |p, _| p.is_playing()));
        // Pausing decodes the poster off the main thread.
        picture.update(cx, |p, cx| p.set_playing(false, cx));
        cx.executor().allow_parking();
        for _ in 0..200 {
            cx.run_until_parked();
            if picture.read_with(cx, |p, _| p.has_poster()) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(picture.read_with(cx, |p, _| p.has_poster()));
        assert!(!picture.read_with(cx, |p, _| p.is_playing()));
    }

    #[gpui::test]
    fn a_picture_of_pixels_is_its_own_poster(cx: &mut TestAppContext) {
        let image = render_image_from_bgra(4, 4, vec![9; 64]).expect("4x4");
        let picture = cx.new(|_| AnimatedPicture::new("p", image, (32.0, 32.0)).sticker());
        picture.update(cx, |p, cx| p.set_playing(true, cx));
        picture.update(cx, |p, cx| p.set_playing(false, cx));
        assert!(picture.read_with(cx, |p, _| p.has_poster()));
    }
}
