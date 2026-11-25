//! A still picture in a bubble or as a sticker.

use crate::frames::bubble_size;
use gpui::{
    App, ElementId, Hsla, ImageSource, InteractiveElement as _, IntoElement, ObjectFit,
    ParentElement as _, RenderImage, RenderOnce, Role, SharedString,
    StatefulInteractiveElement as _, Styled as _, StyledImage as _, Window, div, img,
    prelude::FluentBuilder as _, px,
};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Where a picture comes from.
#[derive(Clone)]
pub enum MediaSource {
    /// A file on disk (the downloads cache): GPUI decodes and caches it.
    Path(Arc<Path>),
    /// A file of the app's `AssetSource`.
    Asset(SharedString),
    /// Pixels already in GPUI's format (avatars, thumbnails, decoded frames).
    Image(Arc<RenderImage>),
}

impl MediaSource {
    pub fn path(path: impl Into<PathBuf>) -> Self {
        Self::Path(path.into().into())
    }

    pub fn asset(path: impl Into<SharedString>) -> Self {
        Self::Asset(path.into())
    }

    pub(crate) fn image_source(&self) -> ImageSource {
        match self {
            Self::Path(path) => ImageSource::from(path.clone()),
            Self::Asset(path) => ImageSource::from(path.clone()),
            Self::Image(image) => ImageSource::from(image.clone()),
        }
    }
}

impl From<PathBuf> for MediaSource {
    fn from(path: PathBuf) -> Self {
        Self::path(path)
    }
}

impl From<Arc<RenderImage>> for MediaSource {
    fn from(image: Arc<RenderImage>) -> Self {
        Self::Image(image)
    }
}

/// A picture of a fixed size: cover-cropped, rounded, with a backdrop behind
/// pixels that are not there yet, and an accessible label. GIFs and animated
/// WebPs play too, through GPUI's `img()`, but re-render their parent view
/// each frame: put those in an [`AnimatedPicture`](crate::AnimatedPicture).
#[derive(IntoElement)]
pub struct MediaPicture {
    id: ElementId,
    source: MediaSource,
    size: (f32, f32),
    radius: f32,
    fit: ObjectFit,
    backdrop: Option<Hsla>,
    label: Option<SharedString>,
}

impl MediaPicture {
    /// A picture in a box of `width` x `height` logical pixels.
    pub fn new(
        id: impl Into<ElementId>,
        source: impl Into<MediaSource>,
        width: f32,
        height: f32,
    ) -> Self {
        Self {
            id: id.into(),
            source: source.into(),
            size: (width, height),
            radius: 8.0,
            fit: ObjectFit::Cover,
            backdrop: None,
            label: None,
        }
    }

    /// A picture of `width` x `height` pixels in a message bubble: the box
    /// [`bubble_size`] gives it.
    pub fn bubble(
        id: impl Into<ElementId>,
        source: impl Into<MediaSource>,
        pixel_width: u32,
        pixel_height: u32,
    ) -> Self {
        let (w, h) = bubble_size(pixel_width, pixel_height);
        Self::new(id, source, w, h)
    }

    /// A sticker: square, no rounding, no backdrop, contained rather than cropped.
    pub fn sticker(id: impl Into<ElementId>, source: impl Into<MediaSource>, side: f32) -> Self {
        Self::new(id, source, side, side)
            .radius(0.0)
            .fit(ObjectFit::Contain)
    }

    pub fn radius(mut self, radius: f32) -> Self {
        self.radius = radius;
        self
    }

    pub fn fit(mut self, fit: ObjectFit) -> Self {
        self.fit = fit;
        self
    }

    /// The colour behind the picture while it loads, and behind transparency.
    pub fn backdrop(mut self, color: impl Into<Hsla>) -> Self {
        self.backdrop = Some(color.into());
        self
    }

    /// What a screen reader says (the message's caption or a description).
    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
        self
    }
}

impl RenderOnce for MediaPicture {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let (w, h) = self.size;
        div()
            .id(self.id)
            .role(Role::Image)
            .when_some(self.label, |element, label| element.aria_label(label))
            .w(px(w))
            .h(px(h))
            .flex_none()
            .overflow_hidden()
            .rounded(px(self.radius))
            .when_some(self.backdrop, |element, color| element.bg(color))
            .child(
                img(self.source.image_source())
                    .w(px(w))
                    .h(px(h))
                    .object_fit(self.fit)
                    .rounded(px(self.radius)),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frames::{BUBBLE_MAX_HEIGHT, BUBBLE_WIDTH, render_image_from_bgra};
    use gpui::{Context, Render, TestAppContext, point};

    struct Host {
        picture: Box<dyn Fn() -> MediaPicture>,
    }

    impl Render for Host {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().flex().items_start().child(
                div()
                    .debug_selector(|| "host".into())
                    .child((self.picture)()),
            )
        }
    }

    fn solid() -> MediaSource {
        MediaSource::Image(render_image_from_bgra(8, 8, vec![200; 8 * 8 * 4]).expect("8x8"))
    }

    fn laid_out(
        cx: &mut TestAppContext,
        picture: impl Fn() -> MediaPicture + 'static,
    ) -> gpui::Bounds<gpui::Pixels> {
        let (_, cx) = cx.add_window_view(|_, _| Host {
            picture: Box::new(picture),
        });
        cx.run_until_parked();
        cx.debug_bounds("host").expect("the host was laid out")
    }

    #[gpui::test]
    fn a_bubble_picture_takes_the_bubble_box(cx: &mut TestAppContext) {
        // A portrait picture: 300 wide, capped at 340, so it is cropped.
        let bounds = laid_out(cx, || MediaPicture::bubble("p", solid(), 540, 960));
        assert_eq!(f32::from(bounds.size.width), BUBBLE_WIDTH);
        assert_eq!(f32::from(bounds.size.height), BUBBLE_MAX_HEIGHT);
        assert_eq!(bounds.origin, point(px(0.), px(0.)));
    }

    #[gpui::test]
    fn a_sticker_is_a_square_of_its_own_size(cx: &mut TestAppContext) {
        let bounds = laid_out(cx, || MediaPicture::sticker("s", solid(), 180.0));
        assert_eq!(f32::from(bounds.size.width), 180.0);
        assert_eq!(f32::from(bounds.size.height), 180.0);
    }

    #[gpui::test]
    fn a_picture_from_a_missing_file_still_lays_out_its_box(cx: &mut TestAppContext) {
        let bounds = laid_out(cx, || {
            MediaPicture::new("m", MediaSource::path("/nonexistent/a.png"), 120.0, 80.0)
                .label("A photo")
                .backdrop(gpui::black())
        });
        assert_eq!(
            (f32::from(bounds.size.width), f32::from(bounds.size.height)),
            (120.0, 80.0)
        );
    }
}
