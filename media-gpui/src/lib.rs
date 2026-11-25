//! Media components for ZapFast's GPUI interface, built from what the
//! `spikes/gpui-chat` Media scene proved:
//!
//! - [`MediaPicture`]: a still picture (PNG, JPEG, WebP, or any `RenderImage`)
//!   in a message bubble or as a sticker, cover-cropped and rounded.
//! - [`AnimatedPicture`]: a GIF or animated WebP, played by GPUI's `img()`,
//!   with a pause that shows the first frame instead.
//! - [`LottiePlayer`]: a Lottie sticker (JSON, or WhatsApp's `.was` archive)
//!   rasterized on a background thread and played by a timer that exists only
//!   while the sticker is active.
//! - [`VideoSurface`]: a surface for decoded video frames pushed by another
//!   thread, latest frame wins.
//!
//! Everything that moves is its own view, shown through [`cached`], so an
//! animation re-renders only itself and never the message list around it. An
//! inactive component schedules nothing: no timer, no animation frame, so an
//! idle window stays at 0% CPU.
//!
//! The crate depends on GPUI only: it knows nothing of ZapFast's backend, so
//! the app converts `PixelImage`s with [`frames::render_image_from_premultiplied_rgba`].

pub mod animated;
pub mod frames;
pub mod lottie;
pub mod picture;
pub mod video;

pub use animated::AnimatedPicture;
pub use frames::{
    BUBBLE_MAX_HEIGHT, BUBBLE_MIN_HEIGHT, BUBBLE_WIDTH, bubble_size, render_image,
    render_image_from_bgra, render_image_from_premultiplied_rgba,
};
pub use lottie::{LottieLimits, LottiePlayer, LottieSource, LottieState};
pub use picture::{MediaPicture, MediaSource};
pub use video::{VideoFit, VideoFrame, VideoStats, VideoSurface};

use gpui::{Entity, IntoElement, Render, StyleRefinement, Styled as _, px};

/// A view of a fixed size that re-renders only when it is notified, so one
/// animation never re-renders the rest of its parent. Put every component that
/// moves behind this.
pub fn cached<V: Render>(view: &Entity<V>, size: (f32, f32)) -> impl IntoElement {
    view.clone()
        .cached(StyleRefinement::default().w(px(size.0)).h(px(size.1)))
}
