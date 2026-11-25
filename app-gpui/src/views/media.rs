//! The pictures, stickers and clips inside message bubbles, drawn with the
//! `zapfast-gpui-media` components.
//!
//! A bubble shows what the message carries at hand: the downloaded file once
//! it is there, the small preview WhatsApp sent along until then, and a ring
//! or a "tap to retry" over it while the file is on its way. Everything that
//! moves (animated stickers, Lottie) is an entity of its own shown through
//! `cached`, and plays only while its row is on screen, so a list at rest
//! asks for no frame.

use super::conversation::ConversationView;
use super::msgview::MsgView;
use crate::palette::Palette;
use crate::ui::icon;
use gpui::{
    AnyElement, AppContext as _, Bounds, Context, CursorStyle, Entity, InteractiveElement as _,
    IntoElement, ParentElement as _, Pixels, RenderImage, Role, SharedString,
    StatefulInteractiveElement as _, Styled as _, canvas, div, hsla, prelude::FluentBuilder as _,
    px,
};
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;
use zapfast::model::{Content, Media, MediaState};
use zapfast_gpui_media::animated::decode_poster;
use zapfast_gpui_media::{
    AnimatedPicture, LottiePlayer, LottieSource, MediaPicture, MediaSource, bubble_size, cached,
};

/// Side of a sticker, in logical pixels.
pub const STICKER_SIDE: f32 = 160.0;
/// Side of a round video message.
pub const NOTE_SIDE: f32 = 200.0;

/// What a message's media is, for drawing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Photo,
    Video { gif: bool, note: bool },
    Sticker { animated: bool, lottie: bool },
}

impl Kind {
    pub fn of(content: &Content) -> Option<Self> {
        match content {
            Content::Image { .. } => Some(Self::Photo),
            Content::Video { gif, note, .. } => Some(Self::Video {
                gif: *gif,
                note: *note,
            }),
            Content::Sticker { media, animated } => Some(Self::Sticker {
                animated: *animated,
                lottie: media.mime == "application/was",
            }),
            _ => None,
        }
    }

    /// Whether the file downloads as soon as the row shows even with
    /// automatic downloads off (stickers always do).
    fn always_downloads(self) -> bool {
        matches!(self, Self::Sticker { .. })
    }
}

/// The box of a message's media, in logical pixels.
pub fn size_of(kind: Kind, media: &Media) -> (f32, f32) {
    match kind {
        Kind::Photo | Kind::Video { note: false, .. } => {
            bubble_size(media.width.unwrap_or(0), media.height.unwrap_or(0))
        }
        Kind::Video { note: true, .. } => (NOTE_SIDE, NOTE_SIDE),
        Kind::Sticker { .. } => (STICKER_SIDE, STICKER_SIDE),
    }
}

/// Whether a message's media is still to be downloaded, and may be now.
pub fn wants_download(kind: Kind, media: &Media, auto_download: bool) -> bool {
    media.path.is_none()
        && matches!(media.state, MediaState::Idle)
        && media.is_within_download_limit()
        && (auto_download || kind.always_downloads())
}

/// Where the media was painted in the frame just drawn: each bubble's picture
/// notes its box here, and the view reads it back once the list is done, to
/// know which rows are really on screen (the list also lays out and paints
/// rows in its overdraw, and says nothing about which are visible).
/// Where a message's picture was painted, and in which frame.
type Seen = (Bounds<Pixels>, u64);

#[derive(Clone, Default)]
pub struct Painted {
    frame: Rc<Cell<u64>>,
    seen: Rc<RefCell<HashMap<String, Seen>>>,
}

impl Painted {
    /// A new frame is being built: what was recorded before is out of date.
    pub fn next_frame(&self) {
        self.frame.set(self.frame.get() + 1);
    }

    fn record(&self, id: &str, bounds: Bounds<Pixels>) {
        self.seen
            .borrow_mut()
            .insert(id.to_owned(), (bounds, self.frame.get()));
    }

    /// The messages painted this frame whose picture touches `viewport`.
    pub fn visible(&self, viewport: Bounds<Pixels>) -> HashSet<String> {
        let frame = self.frame.get();
        let mut seen = self.seen.borrow_mut();
        seen.retain(|_, (_, at)| *at == frame);
        seen.iter()
            .filter(|(_, (bounds, _))| {
                bounds.bottom() > viewport.top() && bounds.top() < viewport.bottom()
            })
            .map(|(id, _)| id.clone())
            .collect()
    }
}

/// Per-chat state of the bubbles' media.
#[derive(Default)]
pub struct BubbleMedia {
    pub painted: Painted,
    /// The preview of each message, decoded once (`None`: undecodable).
    previews: HashMap<String, Option<Arc<RenderImage>>>,
    animated: HashMap<String, Entity<AnimatedPicture>>,
    lottie: HashMap<String, Entity<LottiePlayer>>,
    /// Messages whose row is on screen.
    shown: HashSet<String>,
}

impl BubbleMedia {
    /// The messages whose row is on screen now, replaced as the list scrolls.
    /// What stopped being on screen pauses; what came into view plays.
    pub fn set_shown(&mut self, shown: HashSet<String>, cx: &mut gpui::App) {
        if shown == self.shown {
            return;
        }
        for id in self.shown.difference(&shown) {
            if let Some(picture) = self.animated.get(id) {
                picture.update(cx, |p, cx| p.set_playing(false, cx));
            }
            if let Some(player) = self.lottie.get(id) {
                player.update(cx, |p, cx| p.set_active(false, cx));
            }
        }
        for id in shown.difference(&self.shown) {
            if let Some(picture) = self.animated.get(id) {
                picture.update(cx, |p, cx| p.set_playing(true, cx));
            }
            if let Some(player) = self.lottie.get(id) {
                player.update(cx, |p, cx| p.set_active(true, cx));
            }
        }
        self.shown = shown;
    }

    /// Whether the row of message `id` is on screen.
    pub fn is_shown(&self, id: &str) -> bool {
        self.shown.contains(id)
    }

    fn preview(&mut self, view: &MsgView) -> Option<Arc<RenderImage>> {
        self.previews
            .entry(view.id().to_owned())
            .or_insert_with(|| view.message.thumbnail.as_deref().and_then(decode_poster))
            .clone()
    }

    /// Forgets the entities of messages that are gone.
    pub fn retain(&mut self, keep: impl Fn(&str) -> bool) {
        self.previews.retain(|id, _| keep(id));
        self.animated.retain(|id, _| keep(id));
        self.lottie.retain(|id, _| keep(id));
        self.shown.retain(|id| keep(id));
    }

    /// The element that stands for the media of `view`, `None` when the
    /// message has none this module draws (the bubble keeps its chip).
    pub fn element(
        &mut self,
        index: usize,
        view: &MsgView,
        progress: Option<u8>,
        palette: Palette,
        cx: &mut Context<ConversationView>,
    ) -> Option<AnyElement> {
        let kind = Kind::of(&view.message.content)?;
        let media = view.message.content.media()?;
        let (w, h) = size_of(kind, media);
        let id = view.id().to_owned();
        let label = SharedString::from(view.chip.clone().unwrap_or_default());
        let backdrop = if palette.dark {
            hsla(0., 0., 1., 0.08)
        } else {
            hsla(0., 0., 0., 0.08)
        };
        let radius = match kind {
            Kind::Video { note: true, .. } => NOTE_SIDE / 2.0,
            Kind::Sticker { .. } => 0.0,
            _ => 6.0,
        };
        let file = media.path.clone();
        let preview = self.preview(view);
        let sticker = matches!(kind, Kind::Sticker { .. });

        // What fills the box.
        let fill: AnyElement = match (kind, file.as_deref()) {
            (Kind::Sticker { lottie: true, .. }, Some(path)) => {
                let player = self.lottie.entry(id.clone()).or_insert_with(|| {
                    let source = LottieSource::Was(path.to_path_buf());
                    let shown = self.shown.contains(&id);
                    cx.new(|cx| {
                        let mut player = LottiePlayer::new();
                        player.load(source, cx);
                        player.set_active(shown, cx);
                        player
                    })
                });
                cached(player, (w, h)).into_any_element()
            }
            (Kind::Sticker { animated: true, .. }, Some(path)) => {
                let picture = self.animated.entry(id.clone()).or_insert_with(|| {
                    let source = MediaSource::path(path);
                    let shown = self.shown.contains(&id);
                    cx.new(|cx| {
                        let mut picture = AnimatedPicture::new(("sticker", index), source, (w, h))
                            .sticker()
                            .label(label.clone());
                        picture.set_playing(shown, cx);
                        picture
                    })
                });
                cached(picture, (w, h)).into_any_element()
            }
            // Video files are not pictures: the poster stands in for them.
            (Kind::Video { .. }, _) => still(index, preview, None, w, h, radius, backdrop, &label),
            (_, file) => still(index, preview, file, w, h, radius, backdrop, &label),
        };

        let badge = overlay(kind, media, progress);
        let clickable = !sticker || media.path.is_none();
        Some(
            div()
                .id(("media", index))
                .relative()
                .w(px(w))
                .h(px(h))
                .flex_none()
                .overflow_hidden()
                .rounded(px(radius))
                .when(clickable, |element| {
                    element.cursor(CursorStyle::PointingHand)
                })
                .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(cx.listener(move |this, _, _, cx| this.media_clicked(index, cx)))
                .child(fill)
                .child({
                    let painted = self.painted.clone();
                    let id = id.clone();
                    canvas(
                        |_, _, _| (),
                        move |bounds, _, _, _| painted.record(&id, bounds),
                    )
                    .absolute()
                    .inset_0()
                })
                .when_some(badge, |element, badge| element.child(badge))
                .into_any_element(),
        )
    }
}

/// A still picture: the preview underneath, the file over it once there.
#[allow(clippy::too_many_arguments)]
fn still(
    index: usize,
    preview: Option<Arc<RenderImage>>,
    file: Option<&Path>,
    w: f32,
    h: f32,
    radius: f32,
    backdrop: gpui::Hsla,
    label: &SharedString,
) -> AnyElement {
    let mut stack = div()
        .relative()
        .w(px(w))
        .h(px(h))
        .flex_none()
        .rounded(px(radius))
        .bg(backdrop);
    if let Some(preview) = preview {
        stack = stack.child(
            div().absolute().inset_0().child(
                MediaPicture::new(("preview", index), MediaSource::Image(preview), w, h)
                    .radius(radius)
                    .label(label.clone()),
            ),
        );
    }
    if let Some(file) = file {
        stack = stack.child(
            div().absolute().inset_0().child(
                MediaPicture::new(("file", index), MediaSource::path(file), w, h)
                    .radius(radius)
                    .label(label.clone()),
            ),
        );
    }
    stack.into_any_element()
}

/// What sits over the picture: a play button on a clip, a ring while it
/// downloads, a way to ask for it when it is not there.
fn overlay(kind: Kind, media: &Media, progress: Option<u8>) -> Option<AnyElement> {
    let dim = hsla(0., 0., 0., 0.45);
    let circle = |child: AnyElement| {
        div()
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .child(
                div()
                    .min_w(px(46.))
                    .h(px(46.))
                    .px(px(10.))
                    .rounded_full()
                    .bg(dim)
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(px(13.))
                    .text_color(hsla(0., 0., 1., 1.))
                    .child(child),
            )
            .into_any_element()
    };
    let white = hsla(0., 0., 1., 1.);
    match (&media.state, &media.path) {
        (MediaState::Downloading, _) => {
            let text = progress.map_or_else(|| "…".to_owned(), |p| format!("{p}%"));
            Some(circle(
                div()
                    .id(("progress", 0usize))
                    .role(Role::Status)
                    .aria_label(format!("Baixando {text}"))
                    .child(SharedString::from(text))
                    .into_any_element(),
            ))
        }
        (MediaState::Failed(_), _) => Some(circle(
            div()
                .flex()
                .items_center()
                .gap(px(6.))
                .child(icon("icons/download.svg", 18., white))
                .child("Tentar de novo")
                .into_any_element(),
        )),
        (MediaState::Idle, None) if !matches!(kind, Kind::Sticker { .. }) => Some(circle(
            icon("icons/download.svg", 22., white).into_any_element(),
        )),
        (MediaState::Idle, _) => match kind {
            Kind::Video { gif: false, .. } => Some(
                div()
                    .absolute()
                    .inset_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        div()
                            .size(px(46.))
                            .rounded_full()
                            .bg(dim)
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(icon("icons/play.svg", 22., white)),
                    )
                    .into_any_element(),
            ),
            Kind::Video { gif: true, .. } => Some(
                div()
                    .absolute()
                    .left(px(6.))
                    .top(px(6.))
                    .px(px(6.))
                    .py(px(1.))
                    .rounded(px(4.))
                    .bg(dim)
                    .text_size(px(11.))
                    .text_color(white)
                    .child("GIF")
                    .into_any_element(),
            ),
            _ => None,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn media(size: u64) -> Media {
        Media {
            mime: "image/jpeg".into(),
            size,
            width: Some(1000),
            height: Some(500),
            album: None,
            path: None,
            state: MediaState::Idle,
        }
    }

    #[test]
    fn photos_take_the_bubble_box_and_stickers_a_square() {
        let m = media(10);
        assert_eq!(size_of(Kind::Photo, &m), (300.0, 150.0));
        assert_eq!(
            size_of(
                Kind::Sticker {
                    animated: false,
                    lottie: false
                },
                &m
            ),
            (STICKER_SIDE, STICKER_SIDE)
        );
        assert_eq!(
            size_of(
                Kind::Video {
                    gif: false,
                    note: true
                },
                &m
            ),
            (NOTE_SIDE, NOTE_SIDE)
        );
    }

    #[test]
    fn a_download_is_wanted_once_and_only_within_the_limit() {
        let mut m = media(10);
        assert!(wants_download(Kind::Photo, &m, true));
        assert!(!wants_download(Kind::Photo, &m, false));
        let sticker = Kind::Sticker {
            animated: false,
            lottie: false,
        };
        assert!(wants_download(sticker, &m, false));
        m.state = MediaState::Downloading;
        assert!(!wants_download(sticker, &m, true));
        m.state = MediaState::Idle;
        m.path = Some("/tmp/a.jpg".into());
        assert!(!wants_download(Kind::Photo, &m, true));
        m.path = None;
        m.size = 65 * 1024 * 1024;
        assert!(!wants_download(sticker, &m, true));
    }
}
