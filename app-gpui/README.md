# ZapFast GPUI (phase 2: the app skeleton)

ZapFast's WhatsApp backend with a GPUI interface. This crate is a workspace of its own (own `Cargo.lock`), so the root `zapfast` package and its lock stay as upstream has them. It depends on the backend with `zapfast = { path = "..", default-features = false }`: no egui, no eframe.

The name is provisional and lives in one file, `src/brand.rs` (`APP_NAME`, `APP_ID`). Everything that must differ from an installed ZapFast is derived from `APP_ID`.

Phase 2 is the base: isolation, the bridge to the backend, the state as GPUI entities, a demo, and a skeleton window. The screens (login, chat list, conversation) are built on it; media is phase 3.

## Run

```
cd app-gpui
cargo run                      # a real account (see below)
cargo run -- --demo            # synthetic data, no account, nothing written to disk
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --check
```

Rust 1.98.0 (the root's pin). The dev profile is optimized (`opt-level` 1, dependencies 3; still no `--release`), because unoptimized GPUI text shaping makes everything look slow. The first build takes several minutes.

## Real account

1. `cargo run`. The app opens at the pairing screen state (`Unlinked` with a QR code, or a pairing code if asked for).
2. On the phone: WhatsApp, Linked devices, Link a device, scan the code. **This links a second, new device**: the phone then lists two computers (ZapFast and this app). The ZapFast you already use is not touched and keeps its own login.
3. The first sync downloads history into this app's own archive.

Where it lives (all under this app's name, never ZapFast's):

| What | Where |
| --- | --- |
| Data | `AppDirs::of("zapfast-gpui")`: on macOS `~/Library/Application Support/me.paolino.zapfast-gpui` (account in `accounts/1/`), caches under `~/Library/Caches/me.paolino.zapfast-gpui` |
| Keyring | service `me.paolino.zapfast-gpui` (ZapFast: `rocks.zapfast.ZapFast`) |
| Single-instance lock | `zapfast-gpui` in this app's runtime folder; ZapFast's is `fastsapp`, in its own |
| Login item | `zapfast-gpui` (nothing registers it yet) |
| Updater | none: never started, its commands are dropped |

To start over: delete that data folder and the keyring entries of service `me.paolino.zapfast-gpui`. ZapFast's files and entries are separate.

## Isolation (what the tests prove)

`src/identity.rs` derives the names; `tests/isolation.rs` reads this crate's own sources and fails if any of them reaches ZapFast's:

- `AppDirs::adopt_previous_names` (it moves the folders of older ZapFast names into place), `AppDirs::discover`, `adopt_single_account`;
- `zapfast::single_instance` (it also answers on ZapFast's fixed legacy TCP port 47119, so using it would surface a running ZapFast instead of starting), `fastsapp`;
- `zapfast::autostart` (it names ZapFast's login item and moves its old LaunchAgent);
- `zapfast::updates`, `fastframe_update`, and the four updater commands (`CheckForUpdates`, `InspectUpdate`, `DownloadUpdate`, `InstallUpdate`);
- the string `rocks.zapfast.ZapFast`.

Unit tests in `identity.rs` check that folders, keyring service, instance slot and login item are all different from ZapFast's, that the same archive folder gives two different keyring entries under the two services, and that a rooted identity writes only under its root.

Core changes, kept minimal and tested (`cargo test --lib --no-default-features` at the root): `archive::set_keyring_service` (the keyring service name was a fixed string in four places; the default is unchanged, so existing keys keep working) and `AppDirs::of` made public.

## Architecture

```
Backend (Tokio worker) --Event queue + Waker--> Bridge --futures channel--> GPUI task --> Store.apply_event
Views --intents--> Store --Command--> CommandSink (the Backend, or the demo's Responder)
```

- `bridge.rs`: the backend reports through an event queue and a `Waker`. The waker sends one message on a channel; a foreground task parked on that channel drains the queue and applies the events. No polling and no timers: idle stays at 0% (`--bench idle` prints `renders=0`). `wake_after` is a thread that sleeps once.
- `state/`: entities. `Store` is the root and the only writer: `apply_event` for events, intents (`open_chat`, `send_text`, `react`, `load_older`, `mark_read`, `pair_with_phone`, `request_avatar`) for commands. `Session` (link, QR as `QrModules`, pairing code, sync), `ChatList` (the list, search, pins first, typing, avatars) and one `Conversation` per chat (merge rules from the egui app, `ConversationEvent` so a list can splice itself). Types come from the core's `model.rs`; the egui `app.rs` is read, not changed.
- `demo.rs`: deterministic synthetic data and a `Responder` that answers the store's commands with the events the worker would send, through the same bridge. A `Bridge::detached_with` makes it; tests use the same.
- `instance.rs`: this app's own instance lock, through `fastframe-instance`; a second launch asks the first to show its window.

## Flags

| Flag | Effect |
| --- | --- |
| `--demo` | Synthetic data through the same event path; no account, no disk, no instance lock. |
| `--bench idle --seconds N` | Prints `BENCH idle renders=<renders of the root view after the first 2 s> seconds=N` and quits. Renders within 300 ms of a focus change are not counted. Runs on demo data. |
| `--snapshot DIR` | Saves `login.png` and `chats.png` (more as screens land) via `Window::render_to_image`, and quits. Demo data. |
| `--appearance light\|dark` | Forces a palette instead of following the OS. |

Automated runs (`--bench`, `--snapshot`) never take focus: macOS Accessory policy set before the run loop (`WindowingRequest::Headless`), unfocused window in the bottom-right corner. `ZAPFAST_GPUI_LOG=1` prints GPUI's `log` lines.

## Left for later

- The screens: login with QR (the model is ready: `Session::qr`), chat list, conversation, composer, quotes, reactions. They run on the demo.
- Phase 3: media (photos, GIF, WebP, Lottie, video, audio, stickers, documents). Non-text messages are a chip until then.
- Tray, Dock badge and notifications (the spike has them; `StoreEvent::Incoming` is where a notification hooks in), settings, multiple accounts, attachments, calls, search in messages, pinned messages.
- Reading the account's settings (read receipts are on until then), a toast layer for `StoreEvent::Error`/`Info`/`SendRefused`.
- Packaging: a signed `.app` with bundle id `me.paolino.zapfast-gpui` (`identity::bundle_id`) for notifications and the login item.
- Windows and Linux were not run.

## Manual checklist (macOS)

- [ ] Real login: `cargo run`, scan the QR as a new linked device; the phone lists a new computer and the installed ZapFast still works and is still linked.
- [ ] ZapFast open at the same time: both run, neither shows the other's window, neither asks for the other's keyring entries (Keychain Access: entries of service `me.paolino.zapfast-gpui` and `rocks.zapfast.ZapFast` are separate).
- [ ] Launch twice: the second launch exits and the first window comes forward.
- [ ] Send and receive: a text sent from this app arrives on the phone with ticks; one sent from the phone appears here, and an incoming message in the open chat is marked read.
- [ ] Accents and IME: ã, ç, é, dead keys, Option+e, and a Japanese IME in the composer (once the composer is in).
- [ ] Selection and copy across messages, `[HH:MM, D/M/YYYY] Name: text` (once the conversation is in).
- [ ] VoiceOver (Cmd+F5) reads the chat list, messages, composer.
- [ ] CPU at rest: with the window focused and unfocused, Activity Monitor stays near 0%; `cargo run -- --bench idle --seconds 8` prints `renders=0`.
- [ ] Light and dark follow the OS without restarting.
- [ ] No update check is made (Little Snitch or `nettop` shows no request to ZapFast's update host).
