//! The Media scene: pictures and animations in WhatsApp-like media bubbles.
//!
//! Static pictures (generated at runtime, plus real ones from ZapFast's demo
//! assets), a GIF and an animated WebP played by GPUI's own `img()`, a Lottie
//! sticker and a synthetic call video driven by timers that exist only while
//! the scene is visible.

use crate::bench::Stats;
use crate::chat::conversation::wallpaper;
use crate::media::RateMeter;
use crate::media::frames::{self, BUBBLE_WIDTH, bubble_size};
use crate::media::lottie;
use crate::media::synth::{self, Fixtures, Picture};
use crate::media::video::VideoStats;
use crate::palette::Palette;
use crate::ui::{name_color, palette};
use gpui::{
    AppContext as _, Context, Corners, Entity, InteractiveElement, IntoElement, ObjectFit,
    ParentElement, Render, RenderImage, Role, ScrollHandle, SharedString,
    StatefulInteractiveElement, StyleRefinement, Styled, StyledImage, Task, Window, canvas, div,
    img, prelude::FluentBuilder, px,
};
use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Size of a sticker, in logical pixels.
const STICKER: f32 = 180.0;
/// Size of the GIF tile.
const GIF_SIZE: f32 = 220.0;
/// Size of the call video on screen (the frames are 640x360).
const VIDEO_VIEW: (f32, f32) = (480.0, 270.0);
/// The rendering scale of the Lottie sticker (the canvas is 128 points).
const LOTTIE_SCALE: f32 = 2.0;

/// What a tile shows through GPUI's `img()`.
#[derive(Clone)]
pub enum Source {
    Path(PathBuf),
    /// A file of the embedded asset source (see `assets.rs`).
    Asset(&'static str),
}

// ----- tiles: one view each, so an animation re-renders only itself ----------

/// A picture `img()` plays by itself: GIF or animated WebP.
pub struct AnimTile {
    source: Option<Source>,
    size: (f32, f32),
    /// A sticker has no backing tile behind its transparent pixels.
    sticker: bool,
    renders: RateMeter,
    stats: Rc<Stats>,
}

impl AnimTile {
    fn new(size: (f32, f32), stats: Rc<Stats>) -> Self {
        Self {
            source: None,
            size,
            sticker: false,
            renders: RateMeter::default(),
            stats,
        }
    }
}

impl Render for AnimTile {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette(cx);
        let now = Instant::now();
        self.renders.tick(now);
        self.stats.render();
        let (w, h) = self.size;
        div()
            .relative()
            .size_full()
            .rounded(px(8.))
            .overflow_hidden()
            .when(!self.sticker, |tile| tile.bg(p.surface))
            .when_some(self.source.clone(), |tile, source| {
                let image = match source {
                    Source::Path(path) => img(path),
                    Source::Asset(path) => img(SharedString::from(path)),
                };
                tile.child(
                    image
                        .w(px(w))
                        .h(px(h))
                        .object_fit(ObjectFit::Cover)
                        .rounded(px(8.)),
                )
            })
            .child(rate_chip(self.renders.per_second(now), "renders/s"))
    }
}

/// A small chip over a tile's corner.
fn rate_chip(value: usize, unit: &str) -> impl IntoElement {
    div()
        .absolute()
        .left(px(6.))
        .bottom(px(6.))
        .px(px(6.))
        .py(px(1.))
        .rounded(px(6.))
        .bg(gpui::hsla(0., 0., 0., 0.55))
        .text_color(gpui::white())
        .text_size(px(11.))
        .child(SharedString::from(format!("{value} {unit}")))
}

/// The Lottie sticker: frames rendered once by `rasterlottie` on a background
/// thread, then one frame index advanced by a timer at the animation's own
/// rate. The timer exists only between `set_active(true)` and `set_active(false)`.
pub struct LottieTile {
    clip: Option<Arc<RenderImage>>,
    fps: f32,
    index: usize,
    loading: Option<Task<()>>,
    timer: Option<Task<()>>,
    active: bool,
    error: Option<String>,
    renders: RateMeter,
    stats: Rc<Stats>,
}

impl LottieTile {
    fn new(stats: Rc<Stats>) -> Self {
        Self {
            clip: None,
            fps: 30.0,
            index: 0,
            loading: None,
            timer: None,
            active: false,
            error: None,
            renders: RateMeter::default(),
            stats,
        }
    }

    /// Whether a timer is running (the idle tests ask).
    #[cfg(test)]
    pub fn is_running(&self) -> bool {
        self.timer.is_some()
    }

    pub fn set_active(&mut self, active: bool, cx: &mut Context<Self>) {
        self.active = active;
        if !active {
            self.timer = None;
            self.loading = None;
            return;
        }
        if self.clip.is_some() {
            if self.timer.is_none() {
                self.start(cx);
            }
        } else if self.loading.is_none() {
            self.loading = Some(cx.spawn(async move |this, cx| {
                let rendered = cx
                    .background_executor()
                    .spawn(async { lottie::render(lottie::BOUNCE, LOTTIE_SCALE) })
                    .await;
                this.update(cx, |this, cx| {
                    this.loading = None;
                    match rendered {
                        Ok(clip) => {
                            let delay = Duration::from_secs_f32(1.0 / clip.fps);
                            this.fps = clip.fps;
                            this.clip = Some(frames::render_image(
                                clip.frames.into_iter().map(|f| (f, delay)).collect(),
                            ));
                            if this.active {
                                this.start(cx);
                            }
                        }
                        Err(error) => this.error = Some(error),
                    }
                    cx.notify();
                })
                .ok();
            }));
        }
    }

    /// Advances the frame at the animation's rate. The index follows the
    /// clock, so a late tick skips frames instead of falling behind.
    fn start(&mut self, cx: &mut Context<Self>) {
        let Some(clip) = self.clip.clone() else {
            return;
        };
        let count = clip.frame_count();
        let fps = f64::from(self.fps);
        self.timer = Some(cx.spawn(async move |this, cx| {
            let started = Instant::now();
            let mut last = usize::MAX;
            loop {
                let next = (started.elapsed().as_secs_f64() * fps).floor() as u64 + 1;
                let due = started + Duration::from_secs_f64(next as f64 / fps);
                cx.background_executor()
                    .timer(due.saturating_duration_since(Instant::now()))
                    .await;
                let index = (next as usize) % count;
                if index == last {
                    continue;
                }
                last = index;
                if this
                    .update(cx, |this, cx| {
                        this.index = index;
                        this.stats.render();
                        cx.notify();
                    })
                    .is_err()
                {
                    break;
                }
            }
        }));
    }
}

impl Render for LottieTile {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette(cx);
        let now = Instant::now();
        self.renders.tick(now);
        let frame = self.clip.clone().map(|clip| (clip, self.index));
        div()
            .relative()
            .size_full()
            .when_some(frame, |tile, (clip, index)| {
                tile.child(
                    canvas(
                        |_, _, _| (),
                        move |bounds, _, window, _| {
                            let _ = window.paint_image(
                                bounds,
                                bounds,
                                Corners::default(),
                                clip.clone(),
                                index,
                                false,
                            );
                        },
                    )
                    .size_full(),
                )
            })
            .when_some(self.error.clone(), |tile, error| {
                tile.child(
                    div()
                        .text_size(px(12.))
                        .text_color(p.secondary)
                        .child(SharedString::from(error)),
                )
            })
            .child(rate_chip(self.renders.per_second(now), "renders/s"))
    }
}

/// The synthetic call video: a new 640x360 image each 1/30 s, replacing the
/// previous one (and dropping it from the GPU atlas), as a decoded call frame
/// would be pushed.
pub struct VideoTile {
    image: Option<Arc<RenderImage>>,
    /// Counts frames pushed; paint records a frame only when this moved.
    sequence: u64,
    recorded: Rc<Cell<u64>>,
    timer: Option<Task<()>>,
    stats: Rc<VideoStats>,
    scene_stats: Rc<Stats>,
    /// Frames made since the tile was created.
    made: u64,
}

impl VideoTile {
    fn new(stats: Rc<VideoStats>, scene_stats: Rc<Stats>) -> Self {
        Self {
            image: None,
            sequence: 0,
            recorded: Rc::default(),
            timer: None,
            stats,
            scene_stats,
            made: 0,
        }
    }

    #[cfg(test)]
    pub fn is_running(&self) -> bool {
        self.timer.is_some()
    }

    pub fn set_active(&mut self, active: bool, cx: &mut Context<Self>) {
        if !active {
            self.timer = None;
            if let Some(image) = self.image.take() {
                cx.drop_image(image, None);
            }
            return;
        }
        if self.timer.is_some() {
            return;
        }
        self.timer = Some(cx.spawn(async move |this, cx| {
            let started = Instant::now();
            let period = Duration::from_secs_f64(1.0 / synth::VIDEO_FPS);
            let mut n: u64 = 0;
            loop {
                let due = started + period.mul_f64(n as f64);
                cx.background_executor()
                    .timer(due.saturating_duration_since(Instant::now()))
                    .await;
                // A frame that is already late is skipped, not queued.
                let behind = (started.elapsed().as_secs_f64() * synth::VIDEO_FPS) as u64;
                n = n.max(behind);
                // Made off the main thread, as a call's decoded frames would be.
                let begun = Instant::now();
                let frame = cx
                    .background_executor()
                    .spawn(async move { synth::video_frame(n) })
                    .await;
                let generate_ms = begun.elapsed().as_secs_f64() * 1000.0;
                let pushed = this.update(cx, |this, cx| this.push(frame, generate_ms, cx));
                if pushed.is_err() {
                    break;
                }
                n += 1;
            }
        }));
    }

    /// Wraps the pixels in a new image (a new id, so the GPU atlas takes it
    /// up on the next paint), releases the previous one and asks for a frame.
    fn push(&mut self, bgra: Vec<u8>, generate_ms: f64, cx: &mut Context<Self>) {
        let Some(image) =
            frames::render_image_from_bgra(synth::VIDEO_WIDTH, synth::VIDEO_HEIGHT, bgra)
        else {
            return;
        };
        self.stats.generated(generate_ms);
        self.scene_stats.render();
        self.sequence += 1;
        self.made += 1;
        if let Some(previous) = self.image.replace(image) {
            cx.drop_image(previous, None);
        }
        cx.notify();
    }
}

impl Render for VideoTile {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette(cx);
        let render_started = Instant::now();
        let (fps, mean_ms) = self.stats.live(render_started);
        let stats = self.stats.clone();
        let recorded = self.recorded.clone();
        let sequence = self.sequence;
        let (w, h) = VIDEO_VIEW;
        div()
            .relative()
            .size_full()
            .rounded(px(8.))
            .overflow_hidden()
            .bg(gpui::black())
            .when_some(self.image.clone(), |tile, image| {
                tile.child(img(image).w(px(w)).h(px(h)))
            })
            // Painted after the picture: the time from this render to the end
            // of its own paint is the frame's cost (render, layout, the atlas
            // upload inside `paint_image`, paint).
            .child(
                canvas(
                    |_, _, _| (),
                    move |_, _, _, _| {
                        if sequence != 0 && recorded.replace(sequence) != sequence {
                            let now = Instant::now();
                            stats.painted(now, (now - render_started).as_secs_f64() * 1000.0);
                        }
                    },
                )
                .absolute()
                .size(px(1.)),
            )
            .child(
                div()
                    .absolute()
                    .left(px(8.))
                    .bottom(px(8.))
                    .px(px(8.))
                    .py(px(2.))
                    .rounded(px(6.))
                    .bg(gpui::hsla(0., 0., 0., 0.6))
                    .text_color(gpui::white())
                    .text_size(px(12.))
                    .child(SharedString::from(format!(
                        "{fps} fps · upload+render {mean_ms:.2} ms"
                    ))),
            )
            .when(self.image.is_none(), |tile| {
                tile.child(
                    div()
                        .absolute()
                        .inset_0()
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_color(p.secondary)
                        .child(SharedString::from("vídeo pausado")),
                )
            })
    }
}

// ----- the scene --------------------------------------------------------------

pub struct MediaScene {
    video_only: bool,
    fixtures: Option<Fixtures>,
    error: Option<String>,
    loading: Option<Task<()>>,
    active: bool,
    gif: Entity<AnimTile>,
    webp: Entity<AnimTile>,
    lottie: Entity<LottieTile>,
    video: Entity<VideoTile>,
    video_stats: Rc<VideoStats>,
    scroll: ScrollHandle,
}

impl MediaScene {
    /// `video_only` is the benchmark's layout: just the call video.
    pub fn new(video_only: bool, stats: Rc<Stats>, cx: &mut Context<Self>) -> Self {
        let video_stats = VideoStats::new();
        let gif = cx.new(|_| AnimTile::new((GIF_SIZE, GIF_SIZE), stats.clone()));
        let webp = cx.new(|_| {
            let mut tile = AnimTile::new((STICKER, STICKER), stats.clone());
            tile.source = Some(Source::Asset("demo/stickers/astronaut-float.webp"));
            tile.sticker = true;
            tile
        });
        let lottie = cx.new(|_| LottieTile::new(stats.clone()));
        let video = cx.new(|_| VideoTile::new(video_stats.clone(), stats));
        Self {
            video_only,
            fixtures: None,
            error: None,
            loading: None,
            active: false,
            gif,
            webp,
            lottie,
            video,
            video_stats,
            scroll: ScrollHandle::new(),
        }
    }

    /// Scrolls the column to `y` logical pixels from the top (snapshots).
    pub fn scroll_to(&mut self, y: f32, cx: &mut Context<Self>) {
        self.scroll.set_offset(gpui::point(px(0.), px(-y)));
        cx.notify();
    }

    pub fn video_stats(&self) -> Rc<VideoStats> {
        self.video_stats.clone()
    }

    #[cfg(test)]
    pub fn timers_running(&self, cx: &gpui::App) -> bool {
        self.lottie.read(cx).is_running() || self.video.read(cx).is_running()
    }

    /// Starts or stops everything that moves. Called when the scene is shown or
    /// hidden and when the window is covered or minimized.
    pub fn set_active(&mut self, active: bool, cx: &mut Context<Self>) {
        if self.active == active {
            return;
        }
        self.active = active;
        self.video
            .update(cx, |tile, cx| tile.set_active(active, cx));
        if self.video_only {
            return;
        }
        self.lottie
            .update(cx, |tile, cx| tile.set_active(active, cx));
        if active && self.fixtures.is_none() && self.loading.is_none() {
            // The pictures are generated once, off the main thread.
            self.loading = Some(cx.spawn(async move |this, cx| {
                let made = cx
                    .background_executor()
                    .spawn(async { Fixtures::generate() })
                    .await;
                this.update(cx, |this, cx| {
                    this.loading = None;
                    match made {
                        Ok(fixtures) => {
                            let gif = Source::Path(fixtures.gif.path.clone());
                            this.gif.update(cx, |tile, _| tile.source = Some(gif));
                            this.fixtures = Some(fixtures);
                        }
                        Err(error) => this.error = Some(error.to_string()),
                    }
                    cx.notify();
                })
                .ok();
            }));
        }
    }

    fn picture(
        &self,
        id: &'static str,
        picture: &Picture,
        caption: String,
        outgoing: bool,
        p: Palette,
    ) -> impl IntoElement {
        let (w, h) = bubble_size(picture.width, picture.height);
        media_bubble(
            id,
            outgoing,
            "Ana",
            caption.clone(),
            div().w(px(w)).h(px(h)).bg(p.surface).child(
                img(picture.path.clone())
                    .w(px(w))
                    .h(px(h))
                    .object_fit(ObjectFit::Cover)
                    .rounded(px(8.)),
            ),
            caption,
            p,
        )
    }

    fn asset_picture(
        &self,
        id: &'static str,
        asset: &'static str,
        size: (u32, u32),
        caption: String,
        outgoing: bool,
        p: Palette,
    ) -> impl IntoElement {
        let (w, h) = bubble_size(size.0, size.1);
        media_bubble(
            id,
            outgoing,
            "Bruno Lima",
            caption.clone(),
            div().w(px(w)).h(px(h)).bg(p.surface).child(
                img(SharedString::from(asset))
                    .w(px(w))
                    .h(px(h))
                    .object_fit(ObjectFit::Cover)
                    .rounded(px(8.)),
            ),
            caption,
            p,
        )
    }
}

/// A media bubble: the picture with its rounded corners, a caption, the time.
fn media_bubble(
    id: &'static str,
    outgoing: bool,
    sender: &'static str,
    alt: String,
    content: impl IntoElement,
    caption: String,
    p: Palette,
) -> impl IntoElement {
    div()
        .w_full()
        .flex()
        .px(px(20.))
        .py(px(3.))
        .when(outgoing, |row| row.justify_end())
        .child(
            div()
                .id(id)
                .role(Role::Image)
                .aria_label(alt)
                .flex()
                .flex_col()
                .gap(px(2.))
                .p(px(3.))
                .rounded(px(10.))
                .bg(if outgoing { p.bubble_out } else { p.bubble_in })
                .text_color(p.text)
                .when(!outgoing, |bubble| {
                    bubble.child(
                        div()
                            .px(px(6.))
                            .pt(px(2.))
                            .text_size(px(12.5))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(name_color(sender, p.dark))
                            .child(SharedString::from(sender)),
                    )
                })
                .child(div().rounded(px(8.)).overflow_hidden().child(content))
                .child(
                    div()
                        .flex()
                        .items_end()
                        .gap(px(8.))
                        .px(px(6.))
                        .pb(px(2.))
                        .max_w(px(BUBBLE_WIDTH.max(GIF_SIZE)))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .text_size(px(13.5))
                                .line_height(px(18.))
                                .child(SharedString::from(caption)),
                        )
                        .child(
                            div()
                                .flex_none()
                                .text_size(px(11.))
                                .text_color(p.secondary)
                                .child(SharedString::from("10:41")),
                        ),
                ),
        )
}

/// A sticker: no bubble, a caption in a small pill.
fn sticker(
    id: &'static str,
    outgoing: bool,
    alt: &'static str,
    size: (f32, f32),
    content: impl IntoElement,
    caption: String,
    p: Palette,
) -> impl IntoElement {
    div()
        .w_full()
        .flex()
        .px(px(20.))
        .py(px(6.))
        .when(outgoing, |row| row.justify_end())
        .child(
            div()
                .id(id)
                .role(Role::Image)
                .aria_label(alt)
                .flex()
                .flex_col()
                .items_center()
                .gap(px(4.))
                .child(div().w(px(size.0)).h(px(size.1)).child(content))
                .child(
                    div()
                        .px(px(8.))
                        .py(px(2.))
                        .rounded(px(8.))
                        .bg(p.surface)
                        .text_size(px(12.))
                        .text_color(p.secondary)
                        .child(SharedString::from(caption)),
                ),
        )
}

fn heading(text: &'static str, p: Palette) -> impl IntoElement {
    div().w_full().flex().justify_center().py(px(10.)).child(
        div()
            .px(px(12.))
            .py(px(4.))
            .rounded(px(8.))
            .bg(p.surface)
            .text_color(p.secondary)
            .text_size(px(12.5))
            .child(SharedString::from(text)),
    )
}

fn kib(bytes: u64) -> String {
    format!("{:.0} KB", bytes as f64 / 1024.0)
}

/// A tile as a cached view of a fixed size: it re-renders only when it is
/// notified, so one animation never re-renders the rest of the scene.
fn cached<V: Render>(tile: &Entity<V>, size: (f32, f32)) -> impl IntoElement {
    tile.clone()
        .cached(StyleRefinement::default().w(px(size.0)).h(px(size.1)))
}

impl Render for MediaScene {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette(cx);
        let root = div()
            .id("media-scene")
            .relative()
            .size_full()
            .overflow_hidden()
            .bg(p.chat)
            .child(wallpaper(p));

        if self.video_only {
            return root
                .child(
                    div()
                        .absolute()
                        .inset_0()
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(cached(&self.video, VIDEO_VIEW)),
                )
                .into_any_element();
        }

        let mut column = div()
            .id("media-column")
            .absolute()
            .inset_0()
            .overflow_y_scroll()
            .track_scroll(&self.scroll)
            .flex()
            .flex_col()
            .py(px(12.))
            .child(heading("Imagens estáticas", p));

        if let Some(error) = &self.error {
            column = column.child(heading("Não foi possível gerar as imagens", p));
            column = column.child(div().px(px(20.)).child(SharedString::from(error.clone())));
        } else if let Some(fixtures) = &self.fixtures {
            column = column
                .child(self.picture(
                    "png",
                    &fixtures.png,
                    format!(
                        "PNG gerado em tempo de execução, {}×{}, {}",
                        fixtures.png.width,
                        fixtures.png.height,
                        kib(fixtures.png.bytes)
                    ),
                    false,
                    p,
                ))
                .child(self.picture(
                    "jpeg",
                    &fixtures.jpeg,
                    format!(
                        "JPEG retrato, {}×{}, {}: cortado para caber (cover)",
                        fixtures.jpeg.width,
                        fixtures.jpeg.height,
                        kib(fixtures.jpeg.bytes)
                    ),
                    true,
                    p,
                ))
                .child(self.picture(
                    "webp",
                    &fixtures.webp,
                    format!(
                        "WebP sem perdas, {}×{}, {}",
                        fixtures.webp.width,
                        fixtures.webp.height,
                        kib(fixtures.webp.bytes)
                    ),
                    false,
                    p,
                ));
        } else {
            column = column.child(heading("Gerando as imagens…", p));
        }
        column = column
            .child(self.asset_picture(
                "demo-photo",
                "demo/photos/launch.jpg",
                (1024, 683),
                "Foto real do demo do ZapFast (assets/demo/photos/launch.jpg)".into(),
                true,
                p,
            ))
            .child(self.asset_picture(
                "demo-portrait",
                "demo/photos/spacewalk.jpg",
                (900, 1200),
                "Retrato real (900×1200), cortado em 300×340".into(),
                false,
                p,
            ))
            .child(heading("Animações", p));

        if self.fixtures.is_some() {
            column = column.child(media_bubble(
                "gif",
                true,
                "Você",
                "GIF animado gerado em tempo de execução".into(),
                cached(&self.gif, (GIF_SIZE, GIF_SIZE)),
                "GIF gerado (200×200, 16 quadros), tocado pelo img() do GPUI".into(),
                p,
            ));
        }
        column = column
            .child(sticker(
                "webp-animated",
                false,
                "WebP animado: astronauta flutuando",
                (STICKER, STICKER),
                cached(&self.webp, (STICKER, STICKER)),
                "WebP animado do demo (16 quadros de 100 ms), img() do GPUI".into(),
                p,
            ))
            .child(sticker(
                "lottie",
                true,
                "Figurinha Lottie: bola quicando",
                (STICKER, STICKER),
                cached(&self.lottie, (STICKER, STICKER)),
                "Lottie (rasterlottie 0.2.4), 60 quadros a 30 fps, um temporizador".into(),
                p,
            ))
            .child(heading("Vídeo de chamada sintético", p))
            .child(media_bubble(
                "video",
                false,
                "Chamada de vídeo",
                "Vídeo sintético de 640×360 a 30 quadros por segundo".into(),
                cached(&self.video, VIDEO_VIEW),
                "640×360 a 30 fps, uma imagem nova a cada quadro".into(),
                p,
            ));
        root.child(column).into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::palette::Palette;
    use crate::ui::ActivePalette;
    use gpui::TestAppContext;

    fn open(cx: &mut TestAppContext, video_only: bool) -> Entity<MediaScene> {
        cx.update(|cx| {
            cx.set_global(ActivePalette(Palette::dark()));
            gpui_component::init(cx);
        });
        let (scene, _) = cx.add_window_view(|_, cx| MediaScene::new(video_only, Stats::new(), cx));
        scene
    }

    /// The idle rule: a scene that is not visible runs no timer.
    #[gpui::test]
    fn timers_run_only_while_the_scene_is_active(cx: &mut TestAppContext) {
        let scene = open(cx, false);
        assert!(!scene.read_with(cx, |s, cx| s.timers_running(cx)));
        scene.update(cx, |s, cx| s.set_active(true, cx));
        // The video starts at once; the Lottie timer starts once its frames
        // are rendered in the background.
        cx.executor().allow_parking();
        for _ in 0..400 {
            cx.executor().advance_clock(Duration::from_millis(50));
            cx.run_until_parked();
            std::thread::sleep(Duration::from_millis(5));
            if scene.read_with(cx, |s, cx| s.lottie.read(cx).is_running()) {
                break;
            }
        }
        assert!(scene.read_with(cx, |s, cx| s.video.read(cx).is_running()));
        assert!(scene.read_with(cx, |s, cx| s.lottie.read(cx).is_running()));
        scene.update(cx, |s, cx| s.set_active(false, cx));
        cx.run_until_parked();
        assert!(!scene.read_with(cx, |s, cx| s.timers_running(cx)));
    }

    #[gpui::test]
    fn the_video_pushes_new_images_and_releases_them_when_hidden(cx: &mut TestAppContext) {
        let scene = open(cx, true);
        scene.update(cx, |s, cx| s.set_active(true, cx));
        cx.executor().allow_parking();
        let video = scene.read_with(cx, |s, _| s.video.clone());
        for _ in 0..60 {
            cx.executor().advance_clock(Duration::from_millis(34));
            cx.run_until_parked();
            std::thread::sleep(Duration::from_millis(3));
        }
        let made = video.read_with(cx, |v, _| v.made);
        assert!(made >= 5, "{made} frames were made");
        assert!(video.read_with(cx, |v, _| v.image.is_some()));
        scene.update(cx, |s, cx| s.set_active(false, cx));
        assert!(
            video.read_with(cx, |v, _| v.image.is_none()),
            "released when hidden"
        );
        assert!(!video.read_with(cx, |v, _| v.is_running()));
    }
}
