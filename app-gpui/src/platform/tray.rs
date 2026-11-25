//! The tray item and the Dock badge, through ZapFast's own `fastframe-tray`
//! and `fastframe-dock`, under this app's name.
//!
//! The tray hands its clicks over a `std::sync::mpsc` channel it exposes only
//! through `Tray::events()`, and calls a wake function from the thread the
//! click arrived on. Polling `events()` on a timer would wake the app forever;
//! instead the wake function pushes one `()` into a futures channel that a
//! GPUI task awaits, so the app sleeps until a click really happens.

use super::icon::{app_icon_rgba, tray_template_rgba};
use crate::brand::{APP_ID, APP_NAME};
use fastframe_dock::Badge;
use fastframe_tray::{Config, Event, MenuItem, Tray};
use futures::channel::mpsc;

pub const OPEN: &str = "open";
pub const QUIT: &str = "quit";

/// What a tray click asks the app to do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrayAction {
    Open,
    Quit,
}

/// A Dock click (`Show`), a left click on Linux (`Toggle`) and the first menu
/// entry all bring the window forward.
pub fn action_for(event: Event) -> Option<TrayAction> {
    match event {
        Event::Show | Event::Toggle | Event::Menu(OPEN) => Some(TrayAction::Open),
        Event::Menu(QUIT) => Some(TrayAction::Quit),
        Event::Menu(_) => None,
    }
}

pub fn config() -> Config {
    Config {
        id: APP_ID,
        title: APP_NAME.into(),
        icon: app_icon_rgba,
        template_icon: Some(tray_template_rgba),
        // The tooltip changes with the badge; a host that has the installed
        // icon's name would draw that instead of the pixels.
        themed_icon: false,
        menu: vec![
            MenuItem::action(OPEN, format!("Abrir {APP_NAME}")),
            MenuItem::Separator,
            MenuItem::action(QUIT, "Sair"),
        ],
    }
}

/// The tooltip for a badge count.
pub fn tooltip(count: u32) -> String {
    match count {
        0 => format!("{APP_NAME}\nNenhuma conversa não lida"),
        1 => format!("{APP_NAME}\n1 conversa não lida"),
        n => format!("{APP_NAME}\n{n} conversas não lidas"),
    }
}

/// The tray item, the Dock badge and the unread count they both show.
pub struct Hub {
    tray: Option<Tray>,
    badge: Badge,
    count: Option<u32>,
}

impl Hub {
    /// Makes the item (macOS: now, on the main thread, without activating the
    /// app). Returns the hub and the receiver that gets one `()` per batch of
    /// clicks.
    pub fn start() -> (Self, mpsc::UnboundedReceiver<()>) {
        let (sender, receiver) = mpsc::unbounded();
        let mut tray = Tray::spawn(config(), move || {
            // Called from the tray's thread; the receiver is awaited by a
            // foreground task, which this wakes.
            let _ = sender.unbounded_send(());
        });
        if let Some(tray) = &mut tray {
            tray.create_item();
            tray.set_tooltip(tooltip(0));
        }
        (
            Self {
                tray,
                badge: Badge::new(APP_ID),
                count: None,
            },
            receiver,
        )
    }

    /// Whether the OS gave us a tray item.
    pub fn has_tray(&self) -> bool {
        self.tray.is_some()
    }

    /// Clicks since the last call.
    pub fn events(&self) -> Vec<Event> {
        self.tray.as_ref().map(Tray::events).unwrap_or_default()
    }

    /// Shows `count` on the Dock tile (clears it at zero) and in the tooltip.
    /// Says whether it changed anything: the same count again is not work.
    pub fn set_count(&mut self, count: u32) -> bool {
        if self.count == Some(count) {
            return false;
        }
        self.count = Some(count);
        self.badge.set(count);
        if let Some(tray) = &mut self.tray {
            tray.set_tooltip(tooltip(count));
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clicks_map_to_open_and_quit() {
        assert_eq!(action_for(Event::Menu(OPEN)), Some(TrayAction::Open));
        assert_eq!(action_for(Event::Show), Some(TrayAction::Open));
        assert_eq!(action_for(Event::Toggle), Some(TrayAction::Open));
        assert_eq!(action_for(Event::Menu(QUIT)), Some(TrayAction::Quit));
        assert_eq!(action_for(Event::Menu("other")), None);
    }

    #[test]
    fn the_tray_is_this_apps_and_not_zapfasts() {
        let config = config();
        assert_eq!(config.id, APP_ID);
        assert_ne!(config.id, "zapfast");
        assert!(config.title.contains(APP_NAME));
        assert_eq!(config.menu.len(), 3);
        assert_eq!(config.menu[2], MenuItem::action(QUIT, "Sair"));
        assert_eq!((config.icon)(36).len(), 36 * 36 * 4);
    }

    #[test]
    fn the_tooltip_agrees_with_the_count() {
        assert!(tooltip(0).contains("Nenhuma"));
        assert!(tooltip(1).contains("1 conversa não lida"));
        assert!(tooltip(7).contains("7 conversas não lidas"));
    }
}
