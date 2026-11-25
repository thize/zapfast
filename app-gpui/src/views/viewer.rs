//! The picture viewer: a picture of the chat over the whole window, with zoom
//! (wheel, pinch, + and -), panning by dragging, and the chat's other pictures
//! one key away. Escape, the close button or a click beside the picture leave.
//!
//! Nothing here animates: the view draws again only when the reader does
//! something, so an open viewer costs nothing while it is looked at.

use super::timefmt;
use crate::ui::{icon, palette};
use gpui::{
    Bounds, Context, CursorStyle, EventEmitter, FocusHandle, Focusable, InteractiveElement as _,
    IntoElement, KeyBinding, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, ObjectFit,
    ParentElement as _, Pixels, Point, Render, Role, ScrollDelta, SharedString,
    StatefulInteractiveElement as _, Styled as _, StyledImage as _, Window, actions, div, hsla,
    img, point, px,
};
use std::path::PathBuf;
use std::sync::Arc;

actions!(
    viewer,
    [
        CloseViewer,
        PreviousPicture,
        NextPicture,
        ZoomIn,
        ZoomOut,
        ResetZoom
    ]
);

/// The viewer's keys.
pub fn bind_keys(cx: &mut gpui::App) {
    cx.bind_keys([
        KeyBinding::new("escape", CloseViewer, Some("Viewer")),
        KeyBinding::new("left", PreviousPicture, Some("Viewer")),
        KeyBinding::new("right", NextPicture, Some("Viewer")),
        KeyBinding::new("+", ZoomIn, Some("Viewer")),
        KeyBinding::new("=", ZoomIn, Some("Viewer")),
        KeyBinding::new("-", ZoomOut, Some("Viewer")),
        KeyBinding::new("0", ResetZoom, Some("Viewer")),
    ]);
}

/// Largest zoom, as a multiple of the picture fitted to the window.
pub const MAX_ZOOM: f32 = 8.0;
/// What one press of + or - multiplies the zoom by.
pub const ZOOM_STEP: f32 = 1.25;
/// A press that moves less than this is a click, not a drag.
const CLICK_SLOP: f32 = 4.0;

/// Zoom and pan of one picture, in logical pixels. The offset is the picture
/// centre's distance from the window centre.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Zoom {
    /// 1.0 is the picture fitted to the window.
    pub scale: f32,
    pub x: f32,
    pub y: f32,
}

impl Default for Zoom {
    fn default() -> Self {
        Self {
            scale: 1.0,
            x: 0.0,
            y: 0.0,
        }
    }
}

impl Zoom {
    /// The size of `image` fitted inside `view`, never enlarged past its own
    /// pixels. An unknown size (`0`) fills the view.
    pub fn fit(image: (f32, f32), view: (f32, f32)) -> (f32, f32) {
        if image.0 <= 0.0 || image.1 <= 0.0 {
            return view;
        }
        let k = (view.0 / image.0).min(view.1 / image.1).min(1.0);
        (image.0 * k, image.1 * k)
    }

    /// The size on screen of a picture whose fit is `fit`.
    pub fn shown(&self, fit: (f32, f32)) -> (f32, f32) {
        (fit.0 * self.scale, fit.1 * self.scale)
    }

    /// Multiplies the zoom by `factor`, keeping the point under `focus` (its
    /// distance from the window centre) where it is.
    pub fn zoom_at(&mut self, factor: f32, focus: (f32, f32), fit: (f32, f32), view: (f32, f32)) {
        let scale = (self.scale * factor).clamp(1.0, MAX_ZOOM);
        let ratio = scale / self.scale;
        self.x = focus.0 - (focus.0 - self.x) * ratio;
        self.y = focus.1 - (focus.1 - self.y) * ratio;
        self.scale = scale;
        self.clamp(fit, view);
    }

    pub fn pan(&mut self, dx: f32, dy: f32, fit: (f32, f32), view: (f32, f32)) {
        self.x += dx;
        self.y += dy;
        self.clamp(fit, view);
    }

    /// Keeps the picture covering the window along any side it overflows, and
    /// centred along the ones it does not.
    fn clamp(&mut self, fit: (f32, f32), view: (f32, f32)) {
        let (w, h) = self.shown(fit);
        let slack = |shown: f32, view: f32| ((shown - view) / 2.0).max(0.0);
        let (mx, my) = (slack(w, view.0), slack(h, view.1));
        self.x = self.x.clamp(-mx, mx);
        self.y = self.y.clamp(-my, my);
    }
}

/// One picture of the chat.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ViewerItem {
    pub message: String,
    pub path: PathBuf,
    pub caption: Option<String>,
    pub sender: String,
    pub timestamp: i64,
}

/// What the viewer tells the window around it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewerEvent {
    Close,
}

pub struct Viewer {
    items: Arc<[ViewerItem]>,
    index: usize,
    /// The pixel size of the current picture, `(0, 0)` when unreadable.
    size: (f32, f32),
    zoom: Zoom,
    focus: FocusHandle,
    /// Where the pointer went down and where it was last, while dragging.
    drag: Option<(Point<Pixels>, Point<Pixels>)>,
}

impl EventEmitter<ViewerEvent> for Viewer {}

impl Focusable for Viewer {
    fn focus_handle(&self, _cx: &gpui::App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Viewer {
    /// A viewer of `items`, opened on `index`.
    pub fn new(items: Vec<ViewerItem>, index: usize, cx: &mut Context<Self>) -> Self {
        let index = index.min(items.len().saturating_sub(1));
        let mut viewer = Self {
            items: items.into(),
            index,
            size: (0.0, 0.0),
            zoom: Zoom::default(),
            focus: cx.focus_handle(),
            drag: None,
        };
        viewer.measure();
        viewer
    }

    pub fn index(&self) -> usize {
        self.index
    }

    pub fn count(&self) -> usize {
        self.items.len()
    }

    pub fn current(&self) -> Option<&ViewerItem> {
        self.items.get(self.index)
    }

    pub fn zoom(&self) -> Zoom {
        self.zoom
    }

    fn measure(&mut self) {
        self.size = self
            .current()
            .and_then(|item| image::image_dimensions(&item.path).ok())
            .map_or((0.0, 0.0), |(w, h)| (w as f32, h as f32));
    }

    fn view(window: &Window) -> (f32, f32) {
        let size = window.viewport_size();
        (size.width.as_f32(), size.height.as_f32())
    }

    fn fit(&self, view: (f32, f32)) -> (f32, f32) {
        Zoom::fit(self.size, view)
    }

    /// Shows picture `index` of the chat, unzoomed.
    pub fn show(&mut self, index: usize, cx: &mut Context<Self>) {
        if index >= self.items.len() || index == self.index {
            return;
        }
        self.index = index;
        self.zoom = Zoom::default();
        self.measure();
        cx.notify();
    }

    pub fn step(&mut self, by: isize, cx: &mut Context<Self>) {
        let target = self.index as isize + by;
        if target >= 0 {
            self.show(target as usize, cx);
        }
    }

    /// Zooms by `factor` around `focus` (a window position), or around the
    /// centre.
    pub fn zoom_by(
        &mut self,
        factor: f32,
        focus: Option<Point<Pixels>>,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        let view = Self::view(window);
        let centre = (view.0 / 2.0, view.1 / 2.0);
        let focus = focus.map_or((0.0, 0.0), |p| {
            (p.x.as_f32() - centre.0, p.y.as_f32() - centre.1)
        });
        let before = self.zoom;
        self.zoom.zoom_at(factor, focus, self.fit(view), view);
        if self.zoom != before {
            cx.notify();
        }
    }

    pub fn reset_zoom(&mut self, cx: &mut Context<Self>) {
        if self.zoom != Zoom::default() {
            self.zoom = Zoom::default();
            cx.notify();
        }
    }

    fn close(&mut self, cx: &mut Context<Self>) {
        cx.emit(ViewerEvent::Close);
    }

    fn mouse_down(&mut self, event: &MouseDownEvent) {
        self.drag = Some((event.position, event.position));
    }

    fn mouse_move(&mut self, event: &MouseMoveEvent, window: &Window, cx: &mut Context<Self>) {
        let Some((start, last)) = self.drag else {
            return;
        };
        let view = Self::view(window);
        let (dx, dy) = (
            (event.position.x - last.x).as_f32(),
            (event.position.y - last.y).as_f32(),
        );
        self.drag = Some((start, event.position));
        let before = self.zoom;
        self.zoom.pan(dx, dy, self.fit(view), view);
        if self.zoom != before {
            cx.notify();
        }
    }

    /// A press and release that barely moved, outside the picture, closes.
    fn mouse_up(&mut self, event: &MouseUpEvent, window: &Window, cx: &mut Context<Self>) {
        let Some((start, _)) = self.drag.take() else {
            return;
        };
        let moved = ((event.position.x - start.x).as_f32().powi(2)
            + (event.position.y - start.y).as_f32().powi(2))
        .sqrt();
        if moved > CLICK_SLOP {
            return;
        }
        let view = Self::view(window);
        let (w, h) = self.zoom.shown(self.fit(view));
        let picture = Bounds::centered_at(
            point(
                px(view.0 / 2.0 + self.zoom.x),
                px(view.1 / 2.0 + self.zoom.y),
            ),
            gpui::size(px(w), px(h)),
        );
        if !picture.contains(&event.position) {
            self.close(cx);
        }
    }
}

fn round_button(
    id: &'static str,
    label: &'static str,
    path: &'static str,
) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .role(Role::Button)
        .aria_label(label)
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .size(px(40.))
        .rounded_full()
        .cursor(CursorStyle::PointingHand)
        .hover(|s| s.bg(hsla(0., 0., 1., 0.14)))
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .child(icon(path, 22., hsla(0., 0., 1., 0.92)))
}

impl Render for Viewer {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette(cx);
        let view = Self::view(window);
        let fit = self.fit(view);
        let (w, h) = self.zoom.shown(fit);
        let item = self.current().cloned();
        let (count, index) = (self.count(), self.index);
        let white = hsla(0., 0., 1., 0.92);
        let title = item.as_ref().map(|item| {
            format!(
                "{} · {} {}",
                item.sender,
                timefmt::short_date(item.timestamp),
                timefmt::clock(item.timestamp)
            )
        });
        let percent = (self.zoom.scale * 100.0).round() as u32;

        div()
            .id("viewer")
            .role(Role::Dialog)
            .aria_label("Visualizador de imagens")
            .key_context("Viewer")
            .track_focus(&self.focus)
            .absolute()
            .inset_0()
            .size_full()
            .bg(hsla(0., 0., 0.02, 1.))
            .cursor(if self.drag.is_some() {
                CursorStyle::ClosedHand
            } else {
                CursorStyle::OpenHand
            })
            .on_action(cx.listener(|this, _: &CloseViewer, _, cx| this.close(cx)))
            .on_action(cx.listener(|this, _: &PreviousPicture, _, cx| this.step(-1, cx)))
            .on_action(cx.listener(|this, _: &NextPicture, _, cx| this.step(1, cx)))
            .on_action(
                cx.listener(|this, _: &ZoomIn, window, cx| {
                    this.zoom_by(ZOOM_STEP, None, window, cx)
                }),
            )
            .on_action(cx.listener(|this, _: &ZoomOut, window, cx| {
                this.zoom_by(1.0 / ZOOM_STEP, None, window, cx)
            }))
            .on_action(cx.listener(|this, _: &ResetZoom, _, cx| this.reset_zoom(cx)))
            .on_scroll(cx.listener(|this, event: &gpui::ScrollEvent, window, cx| {
                let dy = match event.delta {
                    ScrollDelta::Pixels(delta) => delta.y.as_f32() * 0.004,
                    ScrollDelta::Lines(lines) => lines.y * 0.12,
                };
                this.zoom_by(dy.exp(), Some(event.position), window, cx);
                cx.stop_propagation();
            }))
            .on_pinch(cx.listener(|this, event: &gpui::PinchEvent, window, cx| {
                this.zoom_by(1.0 + event.delta, Some(event.position), window, cx);
            }))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &MouseDownEvent, _, _| this.mouse_down(event)),
            )
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, window, cx| {
                this.mouse_move(event, window, cx)
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, event: &MouseUpEvent, window, cx| {
                    this.mouse_up(event, window, cx)
                }),
            )
            .when_some(item.clone(), |root, item| {
                root.child(
                    div()
                        .absolute()
                        .left(px(view.0 / 2.0 + self.zoom.x - w / 2.0))
                        .top(px(view.1 / 2.0 + self.zoom.y - h / 2.0))
                        .w(px(w))
                        .h(px(h))
                        .child(
                            img(item.path.clone())
                                .w(px(w))
                                .h(px(h))
                                .object_fit(ObjectFit::Contain),
                        ),
                )
            })
            .child(
                div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .right_0()
                    .h(px(60.))
                    .px(px(16.))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .bg(hsla(0., 0., 0., 0.45))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(px(15.))
                            .text_color(white)
                            .child(SharedString::from(title.unwrap_or_default())),
                    )
                    .child(
                        div()
                            .id("zoom-level")
                            .role(Role::Status)
                            .aria_label(format!("Zoom {percent}%"))
                            .w(px(52.))
                            .text_size(px(13.))
                            .text_color(white)
                            .child(SharedString::from(format!("{percent}%"))),
                    )
                    .child(
                        round_button("zoom-out", "Diminuir zoom", "icons/zoom-out.svg").on_click(
                            cx.listener(|this, _, window, cx| {
                                this.zoom_by(1.0 / ZOOM_STEP, None, window, cx)
                            }),
                        ),
                    )
                    .child(
                        round_button("zoom-in", "Aumentar zoom", "icons/zoom-in.svg").on_click(
                            cx.listener(|this, _, window, cx| {
                                this.zoom_by(ZOOM_STEP, None, window, cx)
                            }),
                        ),
                    )
                    .child(
                        round_button("close-viewer", "Fechar", "icons/x.svg")
                            .on_click(cx.listener(|this, _, _, cx| this.close(cx))),
                    ),
            )
            .when(count > 1 && index > 0, |root| {
                root.child(
                    round_button(
                        "previous-picture",
                        "Imagem anterior",
                        "icons/chevron-left.svg",
                    )
                    .absolute()
                    .left(px(16.))
                    .top(px(view.1 / 2.0 - 20.))
                    .on_click(cx.listener(|this, _, _, cx| this.step(-1, cx))),
                )
            })
            .when(count > 1 && index + 1 < count, |root| {
                root.child(
                    round_button("next-picture", "Próxima imagem", "icons/chevron-right.svg")
                        .absolute()
                        .right(px(16.))
                        .top(px(view.1 / 2.0 - 20.))
                        .on_click(cx.listener(|this, _, _, cx| this.step(1, cx))),
                )
            })
            .when_some(
                item.and_then(|item| item.caption).filter(|c| !c.is_empty()),
                |root, caption| {
                    root.child(
                        div()
                            .absolute()
                            .bottom_0()
                            .left_0()
                            .right_0()
                            .px(px(24.))
                            .py(px(14.))
                            .flex()
                            .justify_center()
                            .bg(hsla(0., 0., 0., 0.45))
                            .text_size(px(15.))
                            .text_color(white)
                            .child(SharedString::from(caption)),
                    )
                },
            )
            .text_color(p.text)
    }
}

use gpui::prelude::FluentBuilder as _;

#[cfg(test)]
mod tests {
    use super::*;

    const VIEW: (f32, f32) = (1000.0, 600.0);

    #[test]
    fn a_picture_is_fitted_and_never_enlarged() {
        assert_eq!(Zoom::fit((2000.0, 1000.0), VIEW), (1000.0, 500.0));
        assert_eq!(Zoom::fit((500.0, 1200.0), VIEW), (250.0, 600.0));
        assert_eq!(Zoom::fit((200.0, 100.0), VIEW), (200.0, 100.0));
        assert_eq!(Zoom::fit((0.0, 0.0), VIEW), VIEW);
    }

    #[test]
    fn zooming_keeps_the_point_under_the_pointer_still() {
        let fit = Zoom::fit((2000.0, 1000.0), VIEW);
        let mut zoom = Zoom::default();
        // A point 100 px right of and 50 px above the window centre.
        let focus = (100.0, -50.0);
        zoom.zoom_at(2.0, focus, fit, VIEW);
        assert_eq!(zoom.scale, 2.0);
        // Before: the picture's point under `focus` was (100, -50) from its
        // centre; after, it is at 2x that from the new centre, which is
        // `offset` away, so it still lands on `focus`.
        assert!((zoom.x + 200.0 - 100.0).abs() < 0.01, "{zoom:?}");
        assert!((zoom.y - 100.0 + 50.0).abs() < 0.01, "{zoom:?}");
    }

    #[test]
    fn zoom_stays_between_the_fit_and_the_maximum() {
        let fit = Zoom::fit((4000.0, 3000.0), VIEW);
        let mut zoom = Zoom::default();
        for _ in 0..40 {
            zoom.zoom_at(ZOOM_STEP, (0.0, 0.0), fit, VIEW);
        }
        assert_eq!(zoom.scale, MAX_ZOOM);
        for _ in 0..80 {
            zoom.zoom_at(1.0 / ZOOM_STEP, (0.0, 0.0), fit, VIEW);
        }
        assert_eq!(zoom, Zoom::default(), "back to the fit, centred");
    }

    #[test]
    fn panning_never_pulls_the_picture_off_its_edges() {
        let fit = Zoom::fit((2000.0, 1000.0), VIEW);
        let mut zoom = Zoom::default();
        zoom.pan(300.0, 300.0, fit, VIEW);
        assert_eq!(zoom, Zoom::default(), "a fitted picture does not pan");
        zoom.zoom_at(2.0, (0.0, 0.0), fit, VIEW);
        zoom.pan(10_000.0, 10_000.0, fit, VIEW);
        // 2000 x 1000 shown in 1000 x 600: 500 and 200 of slack on each side.
        assert_eq!((zoom.x, zoom.y), (500.0, 200.0));
        zoom.pan(-10_000.0, -10_000.0, fit, VIEW);
        assert_eq!((zoom.x, zoom.y), (-500.0, -200.0));
    }

    #[test]
    fn zooming_out_recentres_what_no_longer_overflows() {
        let fit = Zoom::fit((2000.0, 1000.0), VIEW);
        let mut zoom = Zoom::default();
        zoom.zoom_at(4.0, (0.0, 0.0), fit, VIEW);
        zoom.pan(-1500.0, 0.0, fit, VIEW);
        assert!(zoom.x < 0.0);
        zoom.zoom_at(0.25, (0.0, 0.0), fit, VIEW);
        assert_eq!(zoom.x, 0.0);
    }
}
