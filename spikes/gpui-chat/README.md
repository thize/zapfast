# GPUI chat spike

A WhatsApp-like chat client on GPUI in ZapFast's look, to test whether GPUI can carry the real port from egui. Standalone crate (empty `[workspace]`), not part of the root `zapfast` package. Three scenes: Chat (part 1), Media and Platform (part 2), plus an accessibility probe.

Reused from ZapFast (never a WhatsApp asset): its Lucide icons (`assets/icons`, ISC), the wallpaper tile (`assets/wallpaper.svg`), the logo (`packaging/icons/zapfast.svg`), the demo pictures and the animated WebP sticker (`assets/demo`, CC0 or public domain, see its SOURCES.md), its tray icon rendering (`src/util.rs`, copied into `src/platform/icon.rs`), its color palette (`src/theme.rs`, light and dark, plus `wallpaper::doodle_ink`), and Inter. These are referenced by repository path (`include_bytes!("../../../assets/...")`), not copied. Only what ZapFast takes from its `fastframe-icons` and `fastframe-fonts` dependencies is copied here, with its licenses: seven Lucide icons (search, mic, ellipsis, plus, check, x, chevron-down) and `InterVariable.ttf`.

## Run

```
cargo run                      # Chat scene, interactive (opens 1200x800)
cargo test
cargo clippy --all-targets -- -D warnings
cargo build && scripts/bundle-macos.sh   # a signed .app, for notifications
```

Scenes: Chat, Media, Platform. Switch with Cmd+1..3 (Ctrl on other OSes) or the segmented control in the sidebar header. Light and dark follow the OS.

Keys: Cmd+J cycles messages 2500, 5000, 7500 (placed exactly at the top, brief highlight); Cmd+C copies the selection; Esc clears it; Enter sends, Shift+Enter inserts a newline. Mouse: drag selects across bubbles (auto-scroll at the edges), shift-click extends, double-click selects a word, triple-click a whole message. A button appears when the list is not at the bottom.

## Flags

| Flag | Effect |
| --- | --- |
| `--scene chat\|media\|platform` | Scene shown at start. |
| `--bench idle --seconds N` | Prints `BENCH idle renders=<renders of the conversation view during the last N-2 s> seconds=N`, quits. Renders within 300 ms of the window gaining or losing focus are not counted: GPUI redraws every view then, which is the user's doing, not idle repainting. |
| `--bench scroll --seconds N` | Scrolls from the last row to the first in `N * 60` frames (21 rows per frame for the 10,400-row conversation), after 1.5 s of warm-up, and prints `BENCH scroll frames=.. p50_ms=.. p95_ms=.. max_ms=..`: the gaps between the starts of consecutive frames (their renders). The frames are drawn by the benchmark itself, back to back (`Window::draw` in a loop), so a gap is what a frame costs, and the run does not depend on the OS delivering frames: not on focus, on Low Power Mode or on the window being covered. Stderr gets the CPU cost per frame (render to paint) and the gaps between paints. If nothing comes of it within `10 + 4N` s it prints `BENCH scroll error=...` and exits 1: it never hangs. |
| `--paced` | With `--bench scroll`: frames come from the display link, as in an interactive run (see Background mode). Gaps then sit at the display's 16.7 ms with 0.1 ms of scheduler noise, so `p95_ms` lands within 0.1 ms of 16.7 on either side, and a window the platform reports `Hidden` gets no frames: the run ends with the error line above. |
| `--minimize` | Minimizes the window right after it opens, so the platform reports it `Hidden` and delivers no frames (what a window covered by other windows does). To check that the benchmarks do not depend on visibility. |
| `--bench video --seconds N` | Runs the Media scene's synthetic call video alone (640x360, 30 fps, a new image per frame). Prints `BENCH video frames=<n> fps=<x> p95_ms=<y>`: frames painted after the first second, their rate over the time until the end, and the 95th percentile of each frame's cost from `render` to the end of its `paint` (the atlas upload happens inside it). While the window is `Hidden` the platform paints nothing, so the benchmark draws the frames itself every 4 ms (`draw_while_hidden`). Stderr has the average, the gaps between painted frames, the background generation time, the frames made but never painted, resident memory, the window's visibility at the end and how many frames the benchmark drew itself. |
| `--snapshot DIR` | Saves `chat.png`, `chat-selection.png` (a selection across five bubbles), `media.png`, `media-2.png`, `media-3.png` (the Media column scrolled by 0, 900 and 1800 px), `platform.png` via `Window::render_to_image`, which GPUI only compiles with its `test-support` feature (the default `snapshot` feature here). |
| `--jump N` | Opens with message N at the top (handy with `--snapshot`). |
| `--appearance light\|dark` | Forces a palette instead of following the OS. |
| `--probe notify-gpui\|notify-fastframe` | Posts a test notification through one API, waits 6 s and prints `PROBE notify ...` lines (what each one reported), then quits. |
| `--probe badge` | A regular (Dock-visible) but never activated app puts 3 on its Dock tile for 10 s, prints `PROBE badge count=3`, clears it, quits. |
| `--a11y-dump FILE` | While any accessibility client is attached, writes GPUI's accessibility tree (`Window::debug_a11y_tree_json`) to FILE after each render. |

`PID <pid>` is printed at startup; stdout carries nothing else besides the BENCH/SNAPSHOT/PROBE lines. `ZAPFAST_SPIKE_TRACE=1` prints render timestamps (idle), painted video frames (video) and every change of the window's visibility. `ZAPFAST_SPIKE_LOG=1` prints GPUI's `log` lines (notification and accessibility outcomes appear only there).

### Media scene

WhatsApp-style bubbles over the chat wallpaper, in a scrolling column. Everything that moves lives in its own cached view, so an animation re-renders only itself.

- **Static**: PNG (960x540), JPEG (540x960 portrait) and lossless WebP (600x600), all generated at runtime into a temp dir with the `image` crate on a background thread, plus two real pictures from `assets/demo/photos` (landscape and a 900x1200 portrait). Bubbles are 300 px wide, as tall as the aspect ratio says between 120 and 340 px, with `ObjectFit::Cover` and rounded corners (`bubble_size`).
- **GIF**: generated at runtime (200x200, 16 frames of 70 ms, `image`'s GIF encoder), written to the temp dir and shown with `img(path)`: GPUI decodes it and steps the frames itself.
- **Animated WebP**: `assets/demo/stickers/astronaut-float.webp` (ZapFast's fixture: 16 frames of 100 ms), shown with `img("demo/stickers/...")`. There is no animated-WebP encoder in `image` (its WebP encoder is lossless and still-only), so none is generated.
- **Lottie sticker**: ZapFast has no Lottie fixture in the repository (`src/lottie.rs`'s tests build `.was` archives around an empty animation), so `src/media/lottie.rs` holds a hand-written one (a bouncing, squashing ball and its shadow, 128x128, 60 frames at 30 fps). It is rendered with `rasterlottie` =0.2.4 with ZapFast's feature set on a background thread (60 frames at 2x), converted to straight-alpha BGRA, and played by a timer at the animation's own rate that advances a frame index only when it changes; `Window::paint_image` selects the frame.
- **Synthetic call video**: 640x360 BGRA frames made on a background thread (a scrolling hue gradient, a bouncing box, the frame number), pushed every 1/30 s as a new `RenderImage`; the previous one is released from the GPU atlas with `App::drop_image`. The tile shows its measured fps and the average render-to-paint time.

A scene that is not shown runs nothing: `MediaScene::set_active(false)` drops the timers and the video's image, and the shell also deactivates it while the window is covered or minimized (`Window::observe_window_visibility`). Tests: no timer survives `set_active(false)`; the video pushes new images and releases them.

### Platform scene

- **Tray and Dock badge** (`src/platform/tray.rs`): `fastframe-tray` and `fastframe-dock`, declared as the root `Cargo.toml` does, with ZapFast's logo as the icon and as the macOS template image. Menu: Abrir ZapFast, Sair. Buttons: +1 / zerar contador (Dock tile and tooltip). The tray's `wake` function pushes one `()` into a futures channel that a GPUI foreground task awaits: no timer, no polling (`fastframe_tray::idle` is for windowless apps and is not needed). The item is made with `create_item()`, not `attach()`, because `attach()` activates the app.
- **Notifications** (`src/platform/notify.rs`): the Notificar (GPUI) button uses `App::show_system_notification`; Notificar (fastframe) uses `fastframe-notifications` (permission request, then `show_with` and its events).
- **Menu bar** (`src/platform/menus.rs`): application menu (Services, Quit, Cmd+Q), Editar (Desfazer, Refazer, Recortar, Copiar, Colar, Selecionar tudo as `OsAction`s sending gpui-component's `input::*` actions, which the composer handles and the message list answers for Copy and Select All) and Visualizar (the three scenes).
- **Accessibility**: see the findings below; the scene has a button that copies the tree.

### Accessibility probe

gpui-pre 0.3.14 has a real accessibility layer on AccessKit (`src/_accessibility.rs`, `src/window/a11y.rs`): `div().id(..).role(Role::..)`, `aria_label`, `aria_description`, `aria_selected`, `aria_position_in_set`/`size_of_set`, `aria_value`, `on_a11y_action`, `a11y_synthetic_children`, and `Window::is_a11y_active` / `debug_a11y_tree_json`. The tree is built only while an assistive technology has asked for it. The spike gives roles and labels to: scene tabs (`TabList`, `Tab`, selected), chat list (`List` of `ListItem`s, `Ana, 08:41, 3 mensagens não lidas: ...`, position in set), the message list (`List`, each message a `ListItem` labelled `Ada, 10:41: text`, with reactions, quote and delivery state as the description, position `n of 10,000`), day separators (`Heading`), the buttons, and the Media/Platform scenes.

Checked against the real macOS accessibility API (what VoiceOver reads), with System Events while the app ran: the tab list is an `AXTabGroup "Cenas"` of `AXRadioButton`s with the selected state in `AXValue`; the chat list is an `AXList "Conversas"` of `AXGroup`s whose `AXTitle` is the label; the message list is an `AXList "Mensagens de Família Souza"` with one `AXGroup` per message, `AXTitle` `Você, 23:34: Alguém pode confirmar...` and `AXHelp` the description; the composer is an `AXTextArea` (title and placeholder `Digite uma mensagem`); buttons are `AXButton`s with their labels. This is not VoiceOver itself: nobody listened to it (see the checklist).

### Background mode

Every automated run (`--bench`, `--snapshot`) never takes focus: the activation policy is Accessory (no Dock icon, never activated), `cx.activate` is not called, and the window opens unfocused in the bottom-right corner. Interactive runs behave normally.

Three things about frames in such a run, each found by a failing benchmark:

- **GPUI throttles unfocused windows to 30 fps by itself.** `WindowOptions::inactive_frame_interval` defaults to 33.3 ms (and `low_power_frame_interval` too, in Low Power Mode; `gpui-pre window.rs:1989-2062`, `platform.rs:2727-2737`). The earlier note blamed "display pacing" for the 33 ms gaps; it was this cap, and it beats against the video's 30 Hz timer (33.33 ms between pushes against a 33.33 ms minimum between frames), so a push can wait a whole extra interval for its frame. Automated runs set both to `None`; interactive windows keep the defaults.
- **A window the platform calls `Hidden` gets no frames at all** (covered by other windows, minimized, another Space, display asleep: `window.visibility()`, `NSWindow.occlusionState`). The scroll bench used to ask for each next frame from inside the previous one, so one hidden window meant a frame that never came and a process idle at 0% CPU forever (found by the verifier; reproduced here with `--paced --minimize`, which now ends with `BENCH scroll error=no result after 34 s (window visibility Hidden)`). The scroll bench now draws its own frames, and the video bench draws while hidden.
- **Gaps measured between paints include the frames' own jitter.** On a 60 Hz display (this Mac's) a display-paced frame starts every 16.67 ms, but a frame that costs 7 ms followed by one that costs 10 ms paints 3 ms further apart than a vsync without dropping anything. Paced paint gaps came out at p50 16.7, p95 19.3 ms. The BENCH line therefore uses the gaps between frame starts (p95 16.74 to 16.78 paced).

A frame drawn by the benchmark is `Window::draw`: render, layout, prepaint, paint, image uploads to the atlas. It is not presented: while a display link is attached only its own tick can hand out a drawable (`metal_renderer.rs:528-535`), and the GPU command encoding for the scene is not in the number. The paced mode and the interactive app do include it.

The dev profile is optimized (`[profile.dev]` opt-level 1, dependencies 3; still no `--release`): unoptimized GPUI text shaping made the numbers meaningless.

## Results

Last run on this Mac (Apple Silicon, 60 Hz display, macOS, optimized dev profile, background mode, on battery):

```
BENCH idle renders=0 seconds=8
BENCH scroll frames=480 p50_ms=6.31 p95_ms=8.31 max_ms=10.96      # five runs: p95 8.3 to 8.6 (one run, 10.6, with another build running)
frame CPU cost (render to paint): p50=5.81 ms p95=7.80 ms max=10.48 ms   (stderr)
BENCH scroll frames=480 p50_ms=6.47 p95_ms=8.42 max_ms=11.13      # same with --minimize (window Hidden)
BENCH scroll frames=480 p50_ms=16.66 p95_ms=16.74 max_ms=32.57    # --paced: the display's cadence; paint gaps p95 19.3 (stderr)
BENCH scroll error=no result after 34 s (window visibility Hidden): the platform delivered no frames   # --paced --minimize
```

```
BENCH idle renders=0 seconds=8            # tray item and Dock badge code active; CPU time did not move over 8 s
BENCH video frames=210 fps=30.0 p95_ms=2.31
BENCH video frames=210 fps=30.1 p95_ms=2.53
BENCH video frames=870 fps=30.0 p95_ms=2.36   # 30 s; resident memory 132 MB at the end, 149 MB after 3 s: no growth
BENCH video frames=210 fps=30.1 p95_ms=2.16   # window Visible, five runs 30.0 to 30.1
BENCH video frames=210 fps=30.1 p95_ms=2.10   # --minimize: Hidden at the end, 970 to 1020 frames drawn by the bench itself
```

The video's stderr detail: render-to-paint average 1.5 to 1.8 ms, gaps between painted frames p50 33.3 ms / p95 35 ms (38 ms when the bench draws them itself), background generation of a frame about 1 to 2 ms, no frame made and not painted. `idle renders` counts renders of any view plus the tile timers' ticks, so a hidden scene that kept animating would show up. Earlier video runs gave 74 frames (10.6 fps, 120 frames never painted), 0 frames and 26.4 fps (that run ended `Hidden`): a window the platform reports `Hidden` is not painted, and the inactive-window cap can delay a frame. Both are handled (see Background mode); the bench still prints the window's visibility.

The scroll bench lays out 21 new rows per frame, so every frame lays out a whole new screen from cold; 6 ms p50 and 11 ms max for that is the worst case, not a typical wheel scroll. Per-frame cost p95 (8 ms) is half a 60 Hz frame, so there is no off-screen re-layout or text-layout caching to win here: the cost is shaping the rows that are new on screen, which no cache can precompute for rows never seen.

`cargo test`: 82 tests (part 1's 57, plus 25: media 9 (bubble sizes, BGRA conversion, generated formats read back, video frames, Lottie renders 60 distinct frames, video statistics, rate meter), platform 8 (tray actions and menu, tooltip, icon rasterization, menu structure, notification errors), accessibility labels 2, and 6 GPUI view tests: no timer survives hiding the Media scene, the video pushes and releases images, Edit menu actions in the message list and in the composer, the Platform scene counts the badge and renders). Part 1's: selection model 15, markup 6, data 6, copy format 5, bench plumbing 4 (flags, percentiles, line shapes, the frame-counted scroll run), selectable segments 4, time 2, ui 2, assets 1, palette 1, and 11 GPUI tests that build the real views headless: the scroll benchmark finishing with no platform frames, row virtualization, jumping, sending, selection by simulated mouse events including double/triple/shift click and edge auto-scroll, clipboard format through the real action, Enter vs Shift+Enter in the composer, and "idle never re-renders").

## Findings

What GPUI / gpui-component give, and what had to be built:

- **List**: GPUI's `list` + `ListState` with `ListAlignment::Bottom` virtualizes variable-height rows (only visible rows plus 600 px overdraw are laid out) and starts at the bottom. `scroll_to(ListOffset)` places a row exactly at the top (Cmd+J). Used instead of gpui-component's `MessageScroller`, which is a top-aligned tail-following list over the same `ListState`.
- **Selection**: GPUI has no text selection for plain elements. Built: `SelectableText` (wraps `StyledText`, paints the selection as a text-run background, so wrapping and bidi need no geometry of mine; records its `TextLayout` once painted) plus the pure model in `selection.rs` (unicode-segmentation; offsets are logical, always on grapheme boundaries). `TextLayout::index_for_position` / `position_for_index` (gpui-pre `elements/text.rs:834`, `:868`) did the hit-testing. Mouse handling is window-level (`window.on_mouse_event` from a `canvas`) so a drag survives leaving the list. The copy format is in `copy.rs`.
- **Composer**: gpui-component `TextareaState` + `Textarea` bring IME/marked text, undo, the OS emoji picker and context menu. `auto_grow(1, 6)` and `submit_on_enter(true)` are `#[doc(hidden)]` builders (gpui-base `input/base/state.rs:1529`, `:13877`) but work and are tested.
- **Assets**: `svg()` tints Lucide icons (and the wallpaper tile, as ZapFast's doodles) with the text color; `img("logo.svg")` renders the colored logo. Color emoji (ZWJ families, skin tones, flags) shape and render through CoreText. Inter Variable loads with `TextSystem::add_fonts`; bold and the semibold names render.
- **Idle**: the conversation view is `cached`, so the composer's caret blink never re-renders the list (tested). gpui-component starts the blink on activation and never stops it on deactivation (`gpui-base input/base/state.rs:972-985`); this spike sets `Theme.cursor_blink` to blink for 4 s after activity, then rest, and to `steady()` whenever the window is inactive. Net effect: no timers when idle.

Limitations and bugs, with evidence (registry paths under `gpui-pre-0.3.14/src`, `gpui-base-0.11.0/src`):

1. **`ListState::scroll_by` cannot scroll up from the bottom of a bottom-aligned list** (`elements/list.rs:554-583` vs `:1173-1185`): with no anchor it starts from the end of the content, not from the top of the viewport, so a small step up leaves less than a viewport under the new anchor and the next layout snaps back to the bottom. Found by the auto-scroll test. Workaround: `scroll_px_offset_for_scrollbar` + `set_offset_from_scrollbar` (`:768`, `:783`). The wheel path is unaffected.
2. **`ListState::is_scrolled_to_end` returns `None` whenever any row is unmeasured** (`:470-478`, `has_unknown_height`), i.e. always for a 10,000-row list; the spike uses `logical_scroll_top().item_ix >= item_count` for "at the bottom" instead. `examples/list_example.rs:81` reports a scrollbar fraction above 1.0 for bottom-aligned lists (a known bug it demonstrates), so no scrollbar is drawn.
3. **`bounds_for_item` depends on the logical scroll top** (`:712-722`) and returns `None` for rows above it, so hit-testing uses the painted text layouts instead of asking the list. `TextLayout::index_for_position` panics if the element was not painted (`elements/text.rs:834-841`), hence the registry.
4. **`Window::render_to_image` is gated on `test-support`** (`window.rs:3255-3257`); screenshots need that feature in the normal build, and the frame must be drawn explicitly (`window.draw(cx).clear(cx)` through `App::update_window`; inside a typed `WindowHandle::update` of the root it panics, `app/entity_map.rs:192`). The frame loop pauses for occluded windows, so a snapshot taken without the explicit draw can be a stale frame (the first attempt saved the Media scene twice).
5. **`KeyBinding::new` rejects `secondary-`** (`keymap/binding.rs:35-47`, `platform/keystroke.rs:157-166`); only `KeyBinding::load` resolves it, so the spike picks `cmd-` / `ctrl-` per OS.
6. **An absolutely positioned child needs `inset_0()`**, not `size_full()`: without offsets it is placed after its siblings (the pointer canvas sat below the list until a headless test caught it).
7. **Bidi**: right-to-left lines (Arabic, Hebrew) shape in the right order but stay left-aligned in the bubble; GPUI has no paragraph direction. Selection of RTL text is by logical offsets, so a drag over mixed text selects logical, not visual, ranges.
8. **Window activation redraws every view** (`window.refresh()` on focus changes), including cached ones, which is why the idle bench discounts renders near an activation event.
9. **Unfocused windows get 30 Hz frames, by default, from GPUI itself**: `WindowOptions::inactive_frame_interval` (33.3 ms) and `low_power_frame_interval`; set them to `None` where that matters (see Background mode). A window the platform reports `Hidden` gets none.
10. **Metal**: `gpui-pre-apple/build.rs:128-215` shells out to `xcrun metal`; without the toolchain only `runtime_shaders` builds (shaders compile at launch).
11. gpui-component's `Root` is required as the window root; its icons and assets are not used (the spike ships its own `AssetSource`).

Part 2 (Media, Platform, accessibility), with evidence:

12. **`img()` animates GIF and animated WebP, but only in the active window, and re-renders its view every frame.** `elements/img.rs:317-336` advances `frame_index` only `if window.is_window_active()` and calls `window.request_animation_frame()` (`:373-380`), which notifies the view being rendered (`window.rs:3096`): the animation is paced by the display, not by the frame delays (a 100 ms WebP still repaints its view at refresh rate), and an unfocused window shows frame 0. In the unfocused snapshot the GIF and WebP tiles read 2 renders/s and stay still. Not measured with a focused window (automated runs never take focus). Per-tile cached views (as here) keep that repainting off the rest of the scene. GPUI decodes animated WebP itself (`decode_webp_image`, `platform.rs:3515`).
13. **Pixel format**: `RenderImage` frames are BGRA with straight alpha (`platform.rs:3455`; Metal blends with `SourceAlpha`, `metal_renderer.rs:1439`), while `rasterlottie` returns premultiplied RGBA, so Lottie frames need a swap and a division by alpha (`media/frames.rs`). The frame count and delays of a multi-frame `RenderImage` are public; `Window::paint_image` takes a frame index, which is what a timer-driven player needs.
14. **A new `RenderImage` per video frame works, if the old one is released.** Every `RenderImage` gets a new id and its own atlas tile; without `App::drop_image` a 640x360 stream would add about 0.9 MB per frame. With it, resident memory stays flat (132 MB after 30 s). Measured on this Mac in an optimized dev build: 30.0 fps, p95 2.3 to 2.5 ms from render to the end of paint. GPUI also has `Window::paint_surface` / `surface()` (macOS `CVPixelBuffer`), the natural path for a real decoder; not benchmarked.
15. **Nothing is painted for a `Hidden` window.** The platform stops requesting frames for a window it reports covered, minimized, on another Space or on a sleeping display (`WindowVisibility`, `platform.rs:84-118`); timers must follow `observe_window_visibility` (the shell does) and a benchmark must report the visibility (the video bench does).
16. **Tray**: `fastframe-tray` works with GPUI's run loop with no polling: `Tray::spawn(config, wake)`, `create_item()` on the main thread after launch (the item is a real `NSStatusItem`: System Events lists one menu-bar extra, `status menu`), `events()` drained when `wake` fires. `Tray::attach()` activates the app, so it is not used. The tray crate's `Dock` reopen handler is added to GPUI's application delegate by `class_addMethod`; a Dock click is not tested. The tray's menu items could not be read back through the accessibility API (System Events found no menu under the item), and nobody clicked them: **Abrir** / **Sair** are covered only by the mapping test.
17. **Dock badge**: `fastframe_dock::Badge::set(3)` shows 3 on the Dock tile (read back from the Dock's accessibility tree: `AXStatusLabel` = 3) and clears at 0. It needs a regular (not Accessory) app; the other runs are Accessory and have no Dock tile.
18. **Native notifications need a signed `.app` on macOS, and the two APIs do not coexist.** Unbundled, GPUI logs `system notifications disabled: not running from an app bundle` and does nothing (`system_notifications.rs`), and fastframe returns `BundleRequired`. In an ad-hoc signed bundle (`scripts/bundle-macos.sh`, `dev.zapfast.gpui-spike`): GPUI's `show_system_notification` reaches `requestAuthorization`, which failed here with `Notifications are not allowed for this application` (logged only: the API returns nothing), so no banner was seen and delivery is **not demonstrated**; fastframe returns `Unsupported` because `gpui_component::init` has already registered GPUI's response handler (`gpui-component notification.rs:588`), which installs GPUI's `UNUserNotificationCenter` delegate, and fastframe refuses to share it (`fastframe-notifications macos.rs:310-324`). Either API alone is workable; with gpui-component, GPUI's is the one that is not locked out, but its response callback belongs to gpui-component (clicks on app-posted notifications are ignored there), so the app cannot learn that a notification was clicked without replacing gpui-component's handler. Whether a notification appears with a Developer ID signature is untested.
19. **The Edit menu works through GPUI's `OsAction`**: the menu bar is set with `cx.set_menus`; System Events lists Editar with all six items, enabled by `is_action_available` (the OS also adds Writing Tools, AutoFill, Dictation and Emoji & Symbols). The items send gpui-component's `input::Copy` / `Paste` / `SelectAll`, which the composer handles (tested through `dispatch_action`); the message list answers Copy and Select All. Shortcuts are not shown in the menu: GPUI looks bindings up without their key context and these are bound in `Input` / `Conversation`; the keys still work through the keymap. The real Cmd+V and Cmd+C key presses were not typed by anyone.
20. **Accessibility**: the GPUI layer is usable and the tree reaches macOS (see the probe above). Gaps found: (a) plain text children are not nodes (only `text!`-built `Text` elements and elements with an id and a role are), so every row has to carry its own label; (b) `ListItem` reaches macOS as an unspecific `AXGroup`; (c) `aria_position_in_set` / `size_of_set` do not show up through System Events (unknown whether VoiceOver uses them); (d) the list is virtualized, so only the rows `list()` lays out (3 messages plus their overdraw at the bottom, 36 nodes in all) exist in the tree: there is no accessible scroll action on the list and a screen-reader user cannot reach older messages without the scroll wheel or keyboard scrolling; (e) gpui-component's `Textarea` already publishes a `MultilineTextInput` with the placeholder as its label, a `TextRun` and `Focus` / `SetValue` actions (the label cannot be set from `Textarea`'s public API); my first wrapper node was removed as redundant; (f) `SelectableText` (a `StyledText`) exposes nothing, so the message text is read from the row's label, not from the text itself, and the selection is invisible to assistive technology; (g) the accessibility tree is built only when a client asks, and System Events asks, so `--a11y-dump` works without VoiceOver; VoiceOver itself was not run.
21. **GPUI's test platform panics on `App::app_path`** (`platform/test/platform.rs:772`, `unimplemented!()`), so a view that calls it cannot be tested; `platform::is_bundled` reads the executable path instead.

Not done: VoiceOver (see checklist), delivery of a native notification (finding 18), a click on the tray menu, the Dock-reopen click, a focused-window measurement of `img()`'s repaint rate, animated WebP generation (no encoder), Windows and Linux (the platform code is `cfg`-clean only for macOS here), , a scrollbar (limitation 2), message search, an emoji picker beyond the OS one, Windows/Linux builds (nothing platform-specific was written, but only macOS was run).

## Dependencies

`gpui-pre` 0.3.14 (renamed `gpui`) and `gpui-pre-platform` 0.3.14 with `font-kit` and `runtime_shaders`, `gpui-component` 0.11.0, `unicode-segmentation` 1; all GPUI crates exact-pinned with `=`. Part 2 adds `image` 0.25 (png, jpeg, webp, gif), `rasterlottie` =0.2.4 (ZapFast's declaration), `resvg` 0.45 (ZapFast's), `fastframe-tray`, `fastframe-dock` and `fastframe-notifications` (ZapFast's `tag = "v0.17.1"` declarations, unchanged), `futures`, `log` and `tempfile`. Rust 1.98.0 (the repository's pin) builds it; no toolchain override was needed.

## Manual checklist (macOS)

- [ ] Dead keys: type ã, ç, é in the composer.
- [ ] Option+e then a vowel (é, ú) builds the accent with marked text shown.
- [ ] Emoji picker (Ctrl+Cmd+Space) inserts into the composer.
- [ ] Japanese IME: compose, convert, commit; the candidate window sits at the caret.
- [ ] Arabic typing: caret and text direction behave.
- [ ] Drag-select from inside one bubble across many, with auto-scroll at the edges; shift-click, double-click word, triple-click bubble, Esc, Cmd+C; the pasted result is `[HH:MM, D/M/YYYY] Name: text` per message (plain text inside one message).
- [ ] The composer grows to six lines then scrolls; Enter sends and the list jumps to the bottom; the scroll-to-bottom button appears and works.
- [ ] Cmd+J cycles 2500, 5000, 7500 with each message exactly at the top.
- [ ] VoiceOver (Cmd+F5) reads the chat list, a message (`Ada, 10:41: text`), the composer and the scene tabs; a new message sent from the composer is announced; the list can be scrolled to older messages with the keyboard.
- [ ] Menu-bar icon (run `cargo run -- --scene platform`): it shows ZapFast's logo; the menu has Abrir ZapFast and Sair; Abrir brings the window forward from another app; Sair quits; closing and reopening the window from the Dock works.
- [ ] Dock badge: +1 shows 1, 2, 3 on the Dock icon; Zerar clears it; the tooltip of the menu-bar icon follows.
- [ ] Notification: run `scripts/bundle-macos.sh`, open the `.app`, press both buttons in the Platform scene, allow notifications when macOS asks; a banner appears; its Abrir button and a click on its body return to the window.
- [ ] Menu bar: Editar > Copiar, Colar, Selecionar tudo in the composer (type, select, copy, paste), and Copiar / Selecionar tudo with the message list focused (click a message, then Cmd+A, Cmd+C; paste in the composer); Cmd+Z undoes.
- [ ] Media scene: the GIF and the animated WebP move while the window is focused and stop in the background; the Lottie ball bounces smoothly at 30 fps; the video counter runs without hitches; switching to Chat stops all of them (Activity Monitor CPU back near 0).
- [ ] Leave the window idle, focused and unfocused: Activity Monitor CPU stays near 0 and the caret stops blinking after a few seconds.
- [ ] Switch macOS between light and dark: the app follows without restarting.
