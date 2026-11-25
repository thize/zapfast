# GPUI chat spike

A WhatsApp-like chat client on GPUI in ZapFast's look, to test whether GPUI can carry the real port from egui. Standalone crate (empty `[workspace]`), not part of the root `zapfast` package.

## Run

```
cargo run                      # Chat scene, interactive
cargo test
cargo clippy --all-targets -- -D warnings
```

Scenes: Chat (complete), Media and Platform (placeholders for part 2, each in its own module under `src/scenes/`). Switch with Cmd+1..3 (Ctrl on other OSes) or the control in the sidebar header. Light and dark follow the OS.

Keys: Cmd+J cycles messages 2500, 5000, 7500 (placed at the top, brief highlight); Cmd+C copies the selection; Esc clears it; Enter sends, Shift+Enter inserts a newline.

## Flags

| Flag | Effect |
| --- | --- |
| `--scene chat\|media\|platform` | Scene shown at start. |
| `--bench idle --seconds N` | Prints `BENCH idle renders=<renders of the conversation view during the last N-2 s> seconds=N`, quits. Renders within 300 ms of the window gaining or losing focus are not counted (GPUI redraws everything then). |
| `--bench scroll --seconds N` | Scrolls bottom to top at constant speed; prints `BENCH scroll frames=.. p50_ms=.. p95_ms=.. max_ms=..` (and a `BENCH note=` line when the numbers look throttled), quits. |
| `--snapshot DIR` | Saves `chat.png`, `chat-selection.png`, `media.png`, `platform.png` (via `Window::render_to_image`, which needs GPUI's `test-support` feature, on by default here as `snapshot`). |
| `--jump N` | Opens with message N at the top (useful with `--snapshot`). |

`PID <pid>` is printed at startup. `ZAPFAST_SPIKE_TRACE=1` prints render timestamps.

### Background mode

Every automated run (`--bench`, `--snapshot`) is background mode and never takes focus: the activation policy is set to Accessory (no Dock icon, never activated), `cx.activate` is not called, the window opens with `focus: false` in the bottom-right corner of the screen. Interactive runs behave normally. GPUI exposes this itself (`App::set_activation_policy`), so no objc2 code was needed.

Observed effect: an unfocused window is paced at about 30 Hz (frame gaps near 33 ms) and idle redraws on focus changes are discounted. Focus is not traded for accurate numbers; the scroll bench prints `BENCH note=...` when p50 exceeds 24 ms. The build is a debug build, so absolute numbers are pessimistic.

## Results

Latest run on this Mac (debug build, background mode):

```
BENCH idle renders=0 seconds=8
BENCH scroll frames=241 p50_ms=33.38 p95_ms=38.89 max_ms=59.42
BENCH note=frame gaps cluster near 33 ms: ...
```

## Findings

- Dependencies: `gpui-pre` 0.3.14 (renamed `gpui`) and `gpui-pre-platform` 0.3.14 with `font-kit` and `runtime_shaders`, `gpui-component` 0.11.0. All exact-pinned. `runtime_shaders` is required: this Mac has no Metal toolchain and `gpui-pre-apple/build.rs` otherwise runs `xcrun metal`. Rust 1.98 builds it; no toolchain override.
- List: GPUI's `list` + `ListState` with `ListAlignment::Bottom` virtualizes variable-height rows; only visible rows plus overdraw are laid out. `scroll_to(ListOffset)` places a row exactly at the top. `bounds_for_item` returns `None` for every row above the end anchor of a bottom-aligned list, so tests count painted text layouts instead.
- Selection: GPUI has no selection for plain text. `SelectableText` wraps `StyledText`, paints the selection as a run background, and records its `TextLayout`; `TextLayout::index_for_position` maps the pointer to a byte offset. The model (`selection.rs`) is pure Rust with unicode-segmentation. Drag auto-scroll uses a timer that exists only while a drag is held near an edge. Copy format is in `copy.rs`.
- Composer: gpui-component `TextareaState` (`auto_grow(1, 6)`, `submit_on_enter`) provides IME and marked text.
- Idle: the composer caret blink is limited by `Theme.cursor_blink` with a 4 s idle timeout, and the conversation view is `cached`, so the caret never re-renders the list. GPUI redraws all views on window activation changes.
- Gotchas: `KeyBinding::new` rejects `secondary-`; use `cmd-`/`ctrl-` per OS. `Pixels` has no `total_cmp`. `Window::draw` must be called through `App::update_window`, not inside a `WindowHandle::update` on the root.
- Media and Platform snapshots are identical placeholders only if the scene is not switched before the draw; `--snapshot` draws explicitly after each switch.

## Manual checklist (macOS)

- [ ] Dead keys: type ã, ç, é in the composer.
- [ ] Option+e then a vowel (é, ú) builds the accent with marked text shown.
- [ ] Emoji picker (Ctrl+Cmd+Space) inserts into the composer.
- [ ] Japanese IME: compose, convert, commit.
- [ ] Arabic typing: cursor and text direction behave.
- [ ] Drag-select from inside one bubble across many, with auto-scroll at the edges; shift-click, double-click word, triple-click bubble, Esc, Cmd+C; pasted result is `[HH:MM, D/M/YYYY] Name: text` per message.
- [ ] VoiceOver reads the list (not verified; message text is exposed through `StyledText` only as far as GPUI's accessibility tree allows).
- [ ] Leave the window idle and unfocused: Activity Monitor CPU stays near 0.
