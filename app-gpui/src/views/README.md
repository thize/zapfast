# The screens

Login with QR, the chat list and the conversation, in `src/views/`, drawn by `shell::Shell` from the entities in `src/state/` (`Store`, `Session`, `ChatList`, `Conversation`). Views read those entities and call the store's intents; they never see a backend event. `--demo` (the base's `demo::Responder`) answers the commands, so the screens run on synthetic data with no account. Run `cargo run -- --demo`, `--demo --snapshot DIR` (login, login-phone, chats, chat, chat-selection, chat-reply, chat-reactions) and `--demo --bench idle --seconds 8` (0 renders measured).

- **Login** (`login.rs`): the QR from `Session::qr` painted as quads (black on white in both themes, whole pixels per module), the steps to scan it, and "Conectar com número de telefone" (`Session::method`): the number becomes `Store::pair_with_phone`, and `Session::pair_code` is shown as `ABCD-EFGH`.
- **Chat list** (`chat_list.rs`): `ChatList::visible()` (pinned first, local search) in a `uniform_list`; picture (the file `Event::Avatar` names, decoded once off the UI thread by `avatar::Avatars`) or initials, name, preview with a chip for non-text content, time as WhatsApp writes it (`timefmt::list_label`), delivery ticks, unread badge or dot, pin and mute marks. A row asks for its picture the first time it is drawn (`Store::request_avatar`).
- **Conversation** (`conversation.rs`, `panel.rs`, `composer.rs`): a virtualized list that follows `ConversationEvent` (appended, prepended keeping the view in place, updated, reset); day separators, the unread divider, bubbles with sender in groups, quote (a click jumps to the quoted message), reactions, time and ticks; non-text messages as chips (`msgview.rs`); older messages when the top comes on screen (`Store::load_older`); reply from the hover button or the right-click menu (the draft lives in `ChatPanel`); react with six emoji (the same one takes it back); the spike's selection across messages (`selection.rs`, `selectable.rs`), copied as `[HH:MM, D/M/YYYY] Name: text` (`copy.rs`); the spike's composer with IME.

## Not done

Media (Phase 3: only chips), chat filters, archived, new chat, the `+` and `...` menus, call buttons, in-chat search, message info, forwarding, editing, deleting, starring, emoji picker, attachments, typing and presence, drafts, link previews, mentions, keyboard navigation of the list. A quote of one's own message shows the number instead of "Você" (the quote carries only the sender's id). Times use the system zone after `timefmt::use_system_zone()`, which the app must call (the demo pins UTC and a fixed "now").

## Manual checklist (macOS)

- [ ] `cargo run -- --demo`: open each chat; scroll up to load older messages (no jump); the scroll-to-bottom button.
- [ ] Hover a message: react and reply buttons; right-click menu; six emoji react, the same one takes it back; Esc or a click elsewhere closes the menu.
- [ ] Reply: the bar above the composer, the caret goes to the box, sending quotes the message, the X cancels.
- [ ] Composer: `ã ç é`, dead keys, Option+e then a vowel, emoji picker (Ctrl+Cmd+Space), Japanese IME; Enter sends, Shift+Enter breaks the line.
- [ ] Select across bubbles (edge auto-scroll), Cmd+C, paste in the composer; Cmd+A; Esc.
- [ ] Search box: `familia` finds `Família Souza`; the empty result.
- [ ] Real account: pair as a new device (QR, and by number), the chats load, send and receive, mark read.
- [ ] Light and dark; VoiceOver (Cmd+F5): list rows, `Name, HH:MM: text` with reply, reactions and delivery, the QR label; Activity Monitor near 0% idle.
