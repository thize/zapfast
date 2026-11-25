//! Native notifications, two ways, so the spike can say which one works.
//!
//! * GPUI's own API (`App::show_system_notification`, gpui-pre `app.rs:2079`;
//!   macOS: `gpui-pre-macos/src/system_notifications.rs`, UserNotifications).
//!   It returns nothing: delivery, permission and failure are only logged, so
//!   the one thing the app can ask beforehand is whether it runs from a bundle
//!   (`App::app_path`).
//! * `fastframe-notifications`, ZapFast's crate: a worker thread, a permission
//!   request, and an `Event` stream (`Shown`, `Failed(error)`, `Activated`).

use super::{APP_NAME, BUNDLE_ID};
use fastframe_notifications::{
    App as Identity, Client, Error, Event, Handle, Notification, Permission,
};
use futures::channel::mpsc;
use gpui::{App, SystemNotification, SystemNotificationAction};
use std::sync::{Arc, Mutex};

pub const TAG: &str = "gpui-chat-spike-test";

/// Explains an error in the words the Platform scene shows.
pub fn describe_error(error: &Error) -> String {
    match error {
        Error::BundleRequired => {
            "macOS só entrega notificações a um .app assinado: rode scripts/bundle-macos.sh".into()
        }
        Error::PermissionDenied => "permissão negada nos Ajustes do Sistema".into(),
        other => other.to_string(),
    }
}

/// What the scene shows for a permission.
pub fn describe_permission(permission: Permission) -> &'static str {
    match permission {
        Permission::Granted => "permitida",
        Permission::NotDetermined => "ainda não respondida",
        Permission::Denied => "negada",
        Permission::Unknown => "desconhecida",
    }
}

/// Posts through GPUI. Returns the line for the status area.
pub fn post_gpui(cx: &mut App, serial: u32) -> String {
    let bundled = super::is_bundled();
    cx.show_system_notification(SystemNotification {
        tag: TAG.into(),
        title: format!("{APP_NAME}: teste {serial}").into(),
        body: "Notificação enviada pela API do GPUI.".into(),
        actions: vec![SystemNotificationAction {
            id: "open".into(),
            label: "Abrir".into(),
        }],
    });
    if bundled {
        format!("GPUI: enviada (teste {serial}); o GPUI não informa se foi entregue")
    } else {
        format!(
            "GPUI: chamada feita (teste {serial}), mas o app não roda de um .app: o GPUI ignora e só registra no log"
        )
    }
}

/// The fastframe client, made on first use.
pub struct Fastframe {
    client: Option<Result<Client, Error>>,
    events: mpsc::UnboundedSender<String>,
    serial: u32,
    /// Kept while clicks matter (dropping the last handle releases the
    /// listener); the oldest are released after 32, as ZapFast does.
    handles: Arc<Mutex<Vec<Handle>>>,
}

const MAX_HANDLES: usize = 32;

impl Fastframe {
    pub fn new(events: mpsc::UnboundedSender<String>) -> Self {
        Self {
            client: None,
            events,
            serial: 0,
            handles: Arc::default(),
        }
    }

    fn client(&mut self) -> &Result<Client, Error> {
        self.client.get_or_insert_with(|| {
            Client::new(
                Identity::new(BUNDLE_ID, APP_NAME)
                    .desktop_entry("zapfast")
                    .icon("zapfast")
                    // ZapFast's own logo and brand green, as src/notify.rs does.
                    .logo(
                        include_bytes!("../../../../packaging/macos/icon-1024.png"),
                        [0x00, 0xa8, 0x84],
                    ),
            )
        })
    }

    /// Requests permission (which may prompt, once), then shows a
    /// notification. Everything that happens afterwards arrives as lines on
    /// the channel, from the notification worker's thread.
    pub fn post(&mut self) {
        self.serial += 1;
        let serial = self.serial;
        let events = self.events.clone();
        let say = move |line: String| {
            let _ = events.unbounded_send(line);
        };
        let client = match self.client() {
            Ok(client) => client.clone(),
            Err(error) => {
                say(format!("fastframe: sem cliente: {}", describe_error(error)));
                return;
            }
        };
        // The permission request waits for the user's answer, so it runs off
        // the UI thread; its future is runtime-agnostic (tokio's oneshot).
        let events = self.events.clone();
        let handles = self.handles.clone();
        std::thread::spawn(move || {
            let say = |line: String| {
                let _ = events.unbounded_send(line);
            };
            match client.request_permission_blocking() {
                Ok(permission) => say(format!(
                    "fastframe: permissão {}",
                    describe_permission(permission)
                )),
                Err(error) => {
                    say(format!("fastframe: permissão: {}", describe_error(&error)));
                    return;
                }
            }
            let sender = events.clone();
            let shown = client.show_with(
                Notification::new(format!("{APP_NAME}: teste {serial}"))
                    .body("Notificação enviada pelo fastframe-notifications.")
                    .key(format!("{TAG}-{serial}"))
                    .category("im.received")
                    .action("open", "Abrir"),
                move |event| {
                    let _ = sender.unbounded_send(match event {
                        Event::Shown => "fastframe: o sistema aceitou (Shown)".to_owned(),
                        Event::Activated => "fastframe: clicada (Activated)".to_owned(),
                        Event::Action(id) => format!("fastframe: ação {id}"),
                        Event::Dismissed => "fastframe: dispensada".to_owned(),
                        Event::Closed => "fastframe: fechada".to_owned(),
                        Event::Failed(error) => {
                            format!("fastframe: falhou: {}", describe_error(&error))
                        }
                    });
                },
            );
            match shown {
                Ok(handle) => {
                    let mut kept = handles.lock().unwrap_or_else(|e| e.into_inner());
                    kept.push(handle);
                    if kept.len() > MAX_HANDLES {
                        kept.remove(0);
                    }
                    say(format!("fastframe: na fila (teste {serial})"));
                }
                Err(error) => say(format!("fastframe: não enviou: {}", describe_error(&error))),
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errors_and_permissions_are_explained_in_portuguese() {
        assert!(describe_error(&Error::BundleRequired).contains("bundle-macos.sh"));
        assert!(describe_error(&Error::PermissionDenied).contains("negada"));
        assert_eq!(describe_permission(Permission::Granted), "permitida");
        assert_eq!(describe_permission(Permission::Denied), "negada");
    }

    #[test]
    fn an_unbundled_run_reports_the_bundle_requirement() {
        // `cargo test` is not an app bundle, so macOS refuses before it asks
        // for anything (Client::new itself succeeds; delivery says why not).
        let (sender, mut receiver) = mpsc::unbounded();
        let mut fastframe = Fastframe::new(sender);
        fastframe.post();
        let mut lines = Vec::new();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while std::time::Instant::now() < deadline {
            while let Ok(line) = receiver.try_recv() {
                lines.push(line);
            }
            if lines.len() >= 2 {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        let all = lines.join("\n");
        if cfg!(target_os = "macos") {
            assert!(all.contains("bundle-macos.sh"), "{all}");
        } else {
            assert!(!all.is_empty(), "{all}");
        }
    }
}
