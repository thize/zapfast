# zapfast-gpui-media

Media components for ZapFast's GPUI interface, turned from what `spikes/gpui-chat` (Media scene) proved into pieces the app can use. Standalone crate (empty `[workspace]`, own `Cargo.lock`), so the root package is untouched. It depends on GPUI only (`gpui-pre =0.3.14`, the spike's pins) and knows nothing of ZapFast's backend.

| Component | What it is |
| --- | --- |
| `MediaPicture` | A still picture (file, asset or `Arc<RenderImage>`): `bubble` (300 px wide, 120 to 340 px tall, cover-cropped), `sticker` (square, contained), rounded corners, backdrop while loading, accessible label. |
| `AnimatedPicture` | GIF or animated WebP, played by GPUI's `img()` inside its own view. Paused, it shows its first frame (decoded once, off the main thread) and schedules nothing. |
| `LottiePlayer` | Lottie sticker from JSON or a WhatsApp `.was` archive, rendered by `rasterlottie` off the main thread under `LottieLimits` (the root package's caps). One timer at the animation's own rate, alive only while active. Inactive: first frame only. |
| `VideoSurface` | Shows decoded `VideoFrame`s. `attach` takes a `futures` channel receiver and awaits it on GPUI's executor (no polling, no timer); only the newest frame is uploaded, each as a new `RenderImage` with the previous one released from the atlas. Optional `VideoStats` for frame rate and render-to-paint cost. |
| `cached(&entity, size)` | Shows any of the above as a cached view of fixed size, so an animation re-renders only itself, never the message list around it. Put everything that moves behind it. |

`frames` holds the pixel helpers: `bubble_size`, `render_image_from_premultiplied_rgba` (the layout of ZapFast's `PixelImage`, for avatars and thumbnails) and the BGRA conversions (GPUI paints BGRA with straight alpha).

## Using it

```rust
let sticker = cx.new(|_| AnimatedPicture::new("s1", MediaSource::path(file), (180., 180.)).sticker());
sticker.update(cx, |s, cx| s.set_playing(visible, cx));   // from the list's visibility
// in render:
cached(&sticker, (180., 180.))

let lottie = cx.new(|cx| { let mut p = LottiePlayer::new(); p.load(LottieSource::Was(path), cx); p });
lottie.update(cx, |p, cx| p.set_active(visible, cx));

let (frames, rx) = futures::channel::mpsc::unbounded();   // the decoder thread sends VideoFrame
video.update(cx, |v, cx| v.attach(rx, cx));
```

The owner of the components decides when they are active (row scrolled into view, window visible, call on screen). An inactive component has no timer and asks for no animation frame.

## Run and check

```
cargo test
cargo clippy --all-targets -- -D warnings
cargo run --example gallery                                  # a window with one of each, all playing
cargo run --example gallery -- --bench idle --seconds 12     # BENCH idle cpu_percent=..
cargo run --example gallery -- --bench active --seconds 12   # BENCH active cpu_percent=.. and BENCH video frames=.. fps=.. p95_ms=..
```

Automated runs start as an Accessory app with an unfocused window in the screen corner, so they never take focus. The pictures come from ZapFast's `assets/demo` (CC0 or public domain); the Lottie in `testdata/bounce.json` is hand-written.

## Measured (Apple Silicon Mac, 12 s runs, first 2 s excluded)

| Run | Result |
| --- | --- |
| `--bench idle` (everything paused, no frames fed) | 0.02% CPU (2 ms in 10 s) |
| `--bench active` (animated WebP, Lottie at 30 fps, 640x360 video at 30 fps) | 9.6% CPU, video 30.1 fps painted, render-to-paint p95 0.16 ms |

## Not here yet

- Decoding real video: `src/video.rs` in the root package decodes MP4/H.264 into egui textures. A GPUI player would feed this `VideoSurface` from the same decoder thread (its `VideoFrame`s are what the call video needs too).
- Voice messages, documents and the picture viewer (zoom, pan): app-level.
- The spike still carries its own copies of this code; it can depend on this crate later.
- Needs a Mac to confirm: how it looks (colour, cropping, rounded corners), that an animated WebP stops while scrolled out of view in a real list, and VoiceOver reading `MediaPicture`'s label.
