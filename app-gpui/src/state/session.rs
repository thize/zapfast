//! Who is linked and how the link stands: the screen the app opens on.

use gpui::{Context, EventEmitter};
use zapfast::backend::LinkStatus;

use super::qr::QrModules;

/// The linked account's identity, as `Event::Me` reports it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Me {
    pub id: String,
    pub lid: Option<String>,
    pub name: Option<String>,
    pub about: Option<String>,
}

/// What a view should show when the link changes in a way worth saying.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SessionEvent {
    /// The phone linked this computer, or the link came back.
    Connected,
    /// The link dropped after having been up.
    ConnectionLost,
    /// The phone unlinked this device: the saved login is gone.
    LoggedOut,
}

/// The pairing screen's choice, kept so a new code does not flip it back.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PairingMethod {
    #[default]
    Qr,
    PhoneNumber,
}

#[derive(Debug)]
pub struct Session {
    pub link: LinkStatus,
    pub me: Option<Me>,
    /// Whether the first history sync is running, and how far it got.
    pub syncing: bool,
    pub sync_progress: Option<u32>,
    /// The login QR as modules, for as long as `link` waits for a scan.
    pub qr: Option<QrModules>,
    qr_text: Option<String>,
    /// The eight-character code to type on the phone, when one was asked for.
    pub pair_code: Option<String>,
    /// The phone number the pairing code was asked for.
    pub pairing_phone: Option<String>,
    pub method: PairingMethod,
    /// Whether the link was ever up in this run, so a drop is news but the
    /// startup sequence is not.
    was_connected: bool,
}

impl EventEmitter<SessionEvent> for Session {}

impl Default for Session {
    fn default() -> Self {
        Self {
            link: LinkStatus::Starting,
            me: None,
            syncing: false,
            sync_progress: None,
            qr: None,
            qr_text: None,
            pair_code: None,
            pairing_phone: None,
            method: PairingMethod::default(),
            was_connected: false,
        }
    }
}

impl Session {
    pub fn set_link(&mut self, status: LinkStatus, cx: &mut Context<Self>) {
        match &status {
            LinkStatus::Unlinked {
                qr,
                pair_code,
                pairing_phone,
            } => {
                // The code is redrawn only when its text changed: it comes
                // again with every refresh of the link status.
                if self.qr_text != *qr {
                    self.qr = qr.as_deref().and_then(QrModules::from_text);
                    self.qr_text = qr.clone();
                }
                self.pair_code = pair_code.clone();
                self.pairing_phone = pairing_phone.clone();
            }
            _ => {
                self.qr = None;
                self.qr_text = None;
                self.pair_code = None;
                self.pairing_phone = None;
            }
        }
        match &status {
            LinkStatus::Connected => {
                self.was_connected = true;
                cx.emit(SessionEvent::Connected);
            }
            LinkStatus::Disconnected { .. } if self.was_connected => {
                cx.emit(SessionEvent::ConnectionLost);
            }
            LinkStatus::LoggedOut => {
                self.was_connected = false;
                self.me = None;
                self.syncing = false;
                self.sync_progress = None;
                cx.emit(SessionEvent::LoggedOut);
            }
            _ => {}
        }
        if self.link != status {
            self.link = status;
        }
        cx.notify();
    }

    pub fn set_me(&mut self, me: Me, cx: &mut Context<Self>) {
        if self.me.as_ref() != Some(&me) {
            self.me = Some(me);
            cx.notify();
        }
    }

    pub fn set_syncing(&mut self, syncing: bool, cx: &mut Context<Self>) {
        if self.syncing != syncing {
            self.syncing = syncing;
            if !syncing {
                self.sync_progress = None;
            }
            cx.notify();
        }
    }

    pub fn set_sync_progress(&mut self, percent: u32, cx: &mut Context<Self>) {
        if self.sync_progress != Some(percent) {
            self.sync_progress = Some(percent);
            cx.notify();
        }
    }

    /// Whether the login screen, rather than the chats, is what to show.
    pub fn needs_login(&self) -> bool {
        matches!(
            self.link,
            LinkStatus::Unlinked { .. } | LinkStatus::LoggedOut
        )
    }

    /// Whether messages can be sent now.
    pub fn is_online(&self) -> bool {
        self.link.is_connected()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{AppContext as _, TestAppContext};
    use std::{cell::RefCell, rc::Rc};

    fn unlinked(qr: &str) -> LinkStatus {
        LinkStatus::Unlinked {
            qr: Some(qr.into()),
            pair_code: None,
            pairing_phone: None,
        }
    }

    fn watch(
        session: &gpui::Entity<Session>,
        cx: &mut TestAppContext,
    ) -> Rc<RefCell<Vec<SessionEvent>>> {
        let seen = Rc::new(RefCell::new(Vec::new()));
        let log = seen.clone();
        cx.update(|cx| {
            cx.subscribe(session, move |_, event: &SessionEvent, _| {
                log.borrow_mut().push(event.clone())
            })
            .detach()
        });
        seen
    }

    #[gpui::test]
    fn the_qr_follows_the_link_and_is_rebuilt_only_when_its_text_changes(cx: &mut TestAppContext) {
        let session = cx.new(|_| Session::default());
        session.update(cx, |s, cx| s.set_link(unlinked("2@first"), cx));
        let first = session.read_with(cx, |s, _| s.qr.clone());
        assert!(first.is_some());
        session.update(cx, |s, cx| s.set_link(unlinked("2@first"), cx));
        assert_eq!(session.read_with(cx, |s, _| s.qr.clone()), first);
        session.update(cx, |s, cx| s.set_link(unlinked("2@second"), cx));
        assert_ne!(session.read_with(cx, |s, _| s.qr.clone()), first);
        assert!(session.read_with(cx, |s, _| s.needs_login()));
        session.update(cx, |s, cx| s.set_link(LinkStatus::Connecting, cx));
        session.read_with(cx, |s, _| {
            assert!(s.qr.is_none() && !s.needs_login());
        });
    }

    #[gpui::test]
    fn pairing_code_and_phone_come_from_the_status(cx: &mut TestAppContext) {
        let session = cx.new(|_| Session::default());
        session.update(cx, |s, cx| {
            s.set_link(
                LinkStatus::Unlinked {
                    qr: None,
                    pair_code: Some("ABCD-1234".into()),
                    pairing_phone: Some("5511999990000".into()),
                },
                cx,
            )
        });
        session.read_with(cx, |s, _| {
            assert_eq!(s.pair_code.as_deref(), Some("ABCD-1234"));
            assert_eq!(s.pairing_phone.as_deref(), Some("5511999990000"));
            assert!(s.qr.is_none());
        });
    }

    #[gpui::test]
    fn only_a_drop_after_connecting_is_news(cx: &mut TestAppContext) {
        let session = cx.new(|_| Session::default());
        let seen = watch(&session, cx);
        for status in [
            LinkStatus::Starting,
            LinkStatus::Disconnected {
                reason: "startup".into(),
            },
            LinkStatus::Connecting,
        ] {
            session.update(cx, |s, cx| s.set_link(status, cx));
        }
        assert!(seen.borrow().is_empty());
        session.update(cx, |s, cx| s.set_link(LinkStatus::Connected, cx));
        session.update(cx, |s, cx| {
            s.set_link(
                LinkStatus::Disconnected {
                    reason: "network".into(),
                },
                cx,
            )
        });
        session.update(cx, |s, cx| s.set_link(LinkStatus::LoggedOut, cx));
        assert_eq!(
            *seen.borrow(),
            [
                SessionEvent::Connected,
                SessionEvent::ConnectionLost,
                SessionEvent::LoggedOut
            ]
        );
        session.read_with(cx, |s, _| assert!(s.needs_login() && s.me.is_none()));
    }

    #[gpui::test]
    fn sync_progress_resets_when_the_sync_ends(cx: &mut TestAppContext) {
        let session = cx.new(|_| Session::default());
        session.update(cx, |s, cx| {
            s.set_syncing(true, cx);
            s.set_sync_progress(40, cx);
        });
        session.read_with(cx, |s, _| {
            assert!(s.syncing);
            assert_eq!(s.sync_progress, Some(40));
        });
        session.update(cx, |s, cx| s.set_syncing(false, cx));
        session.read_with(cx, |s, _| assert_eq!(s.sync_progress, None));
    }
}
