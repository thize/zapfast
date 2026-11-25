# GPUI chat spike

A WhatsApp-like chat client on GPUI in ZapFast's look, to test whether GPUI can carry the real port from egui. Standalone crate (empty `[workspace]`), not part of the root `zapfast` package. This is part 1 of 2: the Chat scene is complete; Media and Platform are placeholders (one module each under `src/scenes/`) for part 2.

Reused from ZapFast (never a WhatsApp asset): its Lucide icons (`assets/icons`, ISC), the wallpaper tile (`assets/wallpaper.svg`), the logo (`packaging/icons/zapfast.svg`), its color palette (`src/theme.rs`, light and dark, plus `wallpaper::doodle_ink`), and Inter. These are referenced by repository path (`include_bytes!("../../../assets/...")`), not copied. Only what ZapFast takes from its `fastframe-icons` and `fastframe-fonts` dependencies is copied here, with its licenses: seven Lucide icons (search, mic, ellipsis, plus, check, x, chevron-down) and `InterVariable.ttf`.

## Run

```
cargo run                      # Chat scene, interactive (opens 1200x800)
cargo test
cargo clippy --all-targets -- -D warnings
```

Scenes: Chat, Media, Platform. Switch with Cmd+1..3 (Ctrl on other OSes) or the segmented control in the sidebar header. Light and dark follow the OS.

Keys: Cmd+J cycles messages 2500, 5000, 7500 (placed exactly at the top, brief highlight); Cmd+C copies the selection; Esc clears it; Enter sends, Shift+Enter inserts a newline. Mouse: drag selects across bubbles (auto-scroll at the edges), shift-click extends, double-click selects a word, triple-click a whole message. A button appears when the list is not at the bottom.

## Flags

| Flag | Effect |
| --- | --- |
| `--scene chat\|media\|platform` | Scene shown at start. |
| `--bench idle --seconds N` | Prints `BENCH idle renders=<renders of the conversation view during the last N-2 s> seconds=N`, quits. Renders within 300 ms of the window gaining or losing focus are not counted: GPUI redraws every view then, which is the user's doing, not idle repainting. |
| `--bench scroll --seconds N` | Scrolls from the last row to the first at a constant rate in rows per second (after 1.5 s of warm-up); prints `BENCH scroll frames=.. p50_ms=.. p95_ms=.. max_ms=..` and quits. Stderr gets the CPU cost per frame (render to paint) and, when the gaps look throttled, a `BENCH note=` line. |
| `--snapshot DIR` | Saves `chat.png`, `chat-selection.png` (a selection across five bubbles), `media.png`, `platform.png` via `Window::render_to_image`, which GPUI only compiles with its `test-support` feature (the default `snapshot` feature here). |
| `--jump N` | Opens with message N at the top (handy with `--snapshot`). |
| `--appearance light\|dark` | Forces a palette instead of following the OS. |

`PID <pid>` is printed at startup; stdout carries nothing else besides the BENCH/SNAPSHOT lines. `ZAPFAST_SPIKE_TRACE=1` prints render timestamps for the idle bench.

### Background mode

Every automated run (`--bench`, `--snapshot`) never takes focus: the activation policy is Accessory (no Dock icon, never activated), `cx.activate` is not called, and the window opens unfocused in the bottom-right corner. An unfocused window is paced at about 30 Hz (frame gaps near 33 ms), so the scroll bench's gaps measure the display pacing; the CPU cost line is what the frames themselves take. Interactive runs behave normally.

The dev profile is optimized (`[profile.dev]` opt-level 1, dependencies 3; still no `--release`): unoptimized GPUI text shaping made the numbers meaningless.

## Results

Last run on this Mac (Apple Silicon, macOS, optimized dev profile, background mode):

```
BENCH idle renders=0 seconds=8
BENCH scroll frames=241 p50_ms=33.37 p95_ms=36.41 max_ms=43.82
frame CPU cost (render to paint): p50=7.85 ms p95=11.67 ms max=14.95 ms   (stderr)
```

The scroll bench jumps about 20 rows per frame (10,400 rows in 8 s), so nearly every frame lays out a whole new screen from cold; 8 ms p50 and 15 ms max for that is the worst case, not a typical wheel scroll.

`cargo test`: 56 tests (selection model 15, markup 6, data 6, copy format 5, bench plumbing 4, selectable segments 4, time 2, ui 2, assets 1, palette 1, and 10 GPUI tests that build the real views headless: row virtualization, jumping, sending, selection by simulated mouse events including double/triple/shift click and edge auto-scroll, clipboard format through the real action, Enter vs Shift+Enter in the composer, and "idle never re-renders").

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
9. **Unfocused windows get 30 Hz frames** (observed, not from the source): benchmark numbers from background mode are paced by the display, not by GPUI.
10. **Metal**: `gpui-pre-apple/build.rs:128-215` shells out to `xcrun metal`; without the toolchain only `runtime_shaders` builds (shaders compile at launch).
11. gpui-component's `Root` is required as the window root; its icons and assets are not used (the spike ships its own `AssetSource`).

Not done: VoiceOver (see checklist), a scrollbar (limitation 2), message search, real media, an emoji picker beyond the OS one, Windows/Linux builds (nothing platform-specific was written, but only macOS was run).

## Dependencies

`gpui-pre` 0.3.14 (renamed `gpui`) and `gpui-pre-platform` 0.3.14 with `font-kit` and `runtime_shaders`, `gpui-component` 0.11.0, `unicode-segmentation` 1; all GPUI crates exact-pinned with `=`. Rust 1.98.0 (the repository's pin) builds it; no toolchain override was needed.

## Manual checklist (macOS)

- [ ] Dead keys: type ã, ç, é in the composer.
- [ ] Option+e then a vowel (é, ú) builds the accent with marked text shown.
- [ ] Emoji picker (Ctrl+Cmd+Space) inserts into the composer.
- [ ] Japanese IME: compose, convert, commit; the candidate window sits at the caret.
- [ ] Arabic typing: caret and text direction behave.
- [ ] Drag-select from inside one bubble across many, with auto-scroll at the edges; shift-click, double-click word, triple-click bubble, Esc, Cmd+C; the pasted result is `[HH:MM, D/M/YYYY] Name: text` per message (plain text inside one message).
- [ ] The composer grows to six lines then scrolls; Enter sends and the list jumps to the bottom; the scroll-to-bottom button appears and works.
- [ ] Cmd+J cycles 2500, 5000, 7500 with each message exactly at the top.
- [ ] VoiceOver reads the list and the composer (not verified; message text is exposed to the accessibility tree only as far as GPUI's `StyledText` allows).
- [ ] Leave the window idle, focused and unfocused: Activity Monitor CPU stays near 0 and the caret stops blinking after a few seconds.
- [ ] Switch macOS between light and dark: the app follows without restarting.
