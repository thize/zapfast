//! What the views' headless tests share: a store fed by the demo's responder
//! through a detached bridge, as `--demo` wires it.

use std::rc::Rc;

use gpui::{AppContext as _, Entity, KeyBinding, TestAppContext};
use zapfast::i18n::Locale;

use crate::bridge::{Bridge, Injector};
use crate::demo;
use crate::palette::Palette;
use crate::shell::apply_theme;
use crate::state::Store;
use crate::ui::{ActiveLocale, ActivePalette};
use crate::views::conversation::{ClearSelection, CopySelection};

pub struct Demo {
    pub store: Entity<Store>,
    pub injector: Injector,
}

/// A store whose backend is the demo; `connect` says whether the account is
/// linked (the chat list arrives) or waiting for a QR scan.
pub fn launch(cx: &mut TestAppContext, connect: bool) -> Demo {
    cx.update(|cx| {
        gpui_component::init(cx);
        cx.set_global(ActivePalette(Palette::dark()));
        apply_theme(Palette::dark(), cx);
        cx.set_global(ActiveLocale(Locale::PortugueseBrazil));
        cx.bind_keys([
            KeyBinding::new("cmd-c", CopySelection, Some("Conversation")),
            KeyBinding::new("escape", ClearSelection, Some("Conversation")),
        ]);
    });
    let (mut bridge, injector) =
        Bridge::detached_with(|injector| Rc::new(demo::Responder::new(injector)));
    let store = cx.new(|cx| Store::new(bridge.sink(), cx));
    store.update(cx, |_, cx| bridge.pump(cx)).detach();
    if connect {
        demo::start(&injector);
    } else {
        demo::unlinked(&injector);
    }
    cx.run_until_parked();
    Demo { store, injector }
}
