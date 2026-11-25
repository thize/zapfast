//! The reader's settings, as a GPUI entity over the core's own
//! [`zapfast::settings::Settings`] (the same file format, in this app's own
//! config folder, never ZapFast's).
//!
//! A change is applied at once and written a moment later, off the interface
//! thread; a burst of changes (a toggle clicked twice) writes once. Nothing
//! runs while nothing changes, so an idle app stays idle.

use gpui::{Context, EventEmitter, Task};
use std::path::PathBuf;
use std::time::Duration;
use zapfast::settings::Settings;

/// How long after the last change the file is written.
pub const SAVE_DELAY: Duration = Duration::from_millis(400);

/// What the entity tells whoever shows or applies a setting.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettingsEvent {
    Changed,
}

pub struct AppSettings {
    settings: Settings,
    /// Where it is written; `None` keeps it in memory (demos, tests).
    path: Option<PathBuf>,
    dirty: bool,
    saving: Option<Task<()>>,
}

impl EventEmitter<SettingsEvent> for AppSettings {}

impl Default for AppSettings {
    fn default() -> Self {
        Self::in_memory()
    }
}

impl AppSettings {
    /// The defaults, never written anywhere.
    pub fn in_memory() -> Self {
        Self {
            settings: Settings::default(),
            path: None,
            dirty: false,
            saving: None,
        }
    }

    /// The settings of the file at `path` (defaults when it is missing or
    /// unreadable), written back there on every change.
    pub fn at(path: PathBuf) -> Self {
        Self {
            settings: Settings::load(&path),
            path: Some(path),
            dirty: false,
            saving: None,
        }
    }

    /// Switches to the file at `path`: its settings take over (announced as a
    /// change, so what follows them follows) and later changes are written
    /// there.
    pub fn attach(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        self.settings = Settings::load(&path);
        self.path = Some(path);
        self.dirty = false;
        self.saving = None;
        cx.emit(SettingsEvent::Changed);
        cx.notify();
    }

    pub fn get(&self) -> &Settings {
        &self.settings
    }

    /// Changes the settings through `change`; a change that alters nothing
    /// is not an event and is not written.
    pub fn update(&mut self, change: impl FnOnce(&mut Settings), cx: &mut Context<Self>) {
        let before = self.settings.clone();
        change(&mut self.settings);
        if self.settings == before {
            return;
        }
        cx.emit(SettingsEvent::Changed);
        cx.notify();
        if self.path.is_some() {
            self.dirty = true;
            self.saving = Some(cx.spawn(async move |this, cx| {
                cx.background_executor().timer(SAVE_DELAY).await;
                this.update(cx, |this, _| this.flush()).ok();
            }));
        }
    }

    /// Writes the file now if a change is waiting to be written (when the
    /// app quits). Failures are logged and never shown: the settings in use
    /// are right either way.
    pub fn flush(&mut self) {
        self.saving = None;
        let Some(path) = self.path.as_ref().filter(|_| self.dirty) else {
            return;
        };
        self.dirty = false;
        if let Some(folder) = path.parent()
            && let Err(error) = std::fs::create_dir_all(folder)
        {
            log::warn!("could not create the settings folder: {error}");
            return;
        }
        if let Err(error) = self.settings.save(path) {
            log::warn!("could not save the settings: {error}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{AppContext as _, TestAppContext};
    use zapfast::settings::ThemeChoice;

    #[gpui::test]
    fn a_change_is_applied_at_once_and_written_once_after_the_delay(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config").join("settings.json");
        let settings = cx.new(|_| AppSettings::at(path.clone()));
        let events = std::rc::Rc::new(std::cell::Cell::new(0));
        let seen = events.clone();
        let _subscription = cx.update(|cx| {
            cx.subscribe(&settings, move |_, _: &SettingsEvent, _| {
                seen.set(seen.get() + 1)
            })
        });
        settings.update(cx, |s, cx| s.update(|s| s.notifications = false, cx));
        settings.update(cx, |s, cx| s.update(|s| s.theme = ThemeChoice::Dark, cx));
        // Applied, announced twice, not on the disk yet.
        assert!(!settings.read_with(cx, |s, _| s.get().notifications));
        assert_eq!(events.get(), 2);
        assert!(!path.exists());
        cx.executor().advance_clock(SAVE_DELAY * 2);
        cx.run_until_parked();
        let reread = Settings::load(&path);
        assert!(!reread.notifications);
        assert_eq!(reread.theme, ThemeChoice::Dark);
    }

    #[gpui::test]
    fn a_change_that_alters_nothing_is_not_an_event_and_memory_never_writes(
        cx: &mut TestAppContext,
    ) {
        let settings = cx.new(|_| AppSettings::in_memory());
        let events = std::rc::Rc::new(std::cell::Cell::new(0));
        let seen = events.clone();
        let _subscription = cx.update(|cx| {
            cx.subscribe(&settings, move |_, _: &SettingsEvent, _| {
                seen.set(seen.get() + 1)
            })
        });
        settings.update(cx, |s, cx| s.update(|s| s.notifications = true, cx));
        assert_eq!(events.get(), 0, "notifications start on");
        settings.update(cx, |s, cx| s.update(|s| s.auto_download = false, cx));
        assert_eq!(events.get(), 1);
        settings.update(cx, |s, _| s.flush());
    }

    #[gpui::test]
    fn flushing_on_quit_writes_without_waiting(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let settings = cx.new(|_| AppSettings::at(path.clone()));
        settings.update(cx, |s, cx| s.update(|s| s.send_read_receipts = false, cx));
        settings.update(cx, |s, _| s.flush());
        assert!(!Settings::load(&path).send_read_receipts);
    }
}
