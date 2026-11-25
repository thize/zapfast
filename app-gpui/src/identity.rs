//! Everything that must differ from an installed ZapFast on the same machine,
//! derived from [`brand::APP_ID`] in one place.
//!
//! This app never reads or writes what ZapFast owns:
//!
//! | what | ZapFast | this app |
//! | --- | --- | --- |
//! | data folders | `AppDirs::of("zapfast")` | `AppDirs::of(APP_ID)` |
//! | keyring service | `rocks.zapfast.ZapFast` | `me.paolino.<APP_ID>` |
//! | instance lock and channel | `fastsapp` in ZapFast's runtime folder, and a fixed TCP port 47119 for copies from before the lock | `APP_ID` in this app's runtime folder, nothing on that port |
//! | login item | `zapfast` | `APP_ID` |
//! | updater | `zapfast::updates` | none: never started, its commands never sent |
//!
//! What is *not* done is as much a part of it: this app never calls
//! `AppDirs::adopt_previous_names` (it moves the folders of older ZapFast names
//! into place), `zapfast::single_instance` (it also talks to ZapFast's legacy
//! port), `zapfast::autostart` (it names ZapFast's login item and moves its old
//! LaunchAgent), or the updater. `tests/isolation.rs` checks the sources for
//! each of them.

use std::path::PathBuf;

use anyhow::{Context, Result};
use fastframe_autostart::Autostart;
use fastframe_instance::Slot;
use zapfast::model::AccountId;
use zapfast::paths::{AccountDirs, AppDirs};

use crate::brand::{APP_ID, APP_NAME};

/// The names this app runs under.
#[derive(Clone, Debug)]
pub struct Identity {
    /// Configuration, state, cache and runtime folders, none inside
    /// ZapFast's.
    pub dirs: AppDirs,
    /// The keyring service under which archive keys are filed.
    pub keyring_service: String,
}

impl Identity {
    /// The identity of a real run: the platform's standard folders, under this
    /// app's own name.
    pub fn discover() -> Self {
        let dirs = AppDirs::of(APP_ID).unwrap_or_else(|| {
            // No home directory. Relative folders of this app's name, never
            // ZapFast's.
            let fallback = std::env::current_dir().unwrap_or_default();
            AppDirs {
                config: fallback.join(format!("{APP_ID}-config")),
                state: fallback.join(format!("{APP_ID}-state")),
                cache: fallback.join(format!("{APP_ID}-cache")),
                runtime: fallback.join(format!("{APP_ID}-run")),
            }
        });
        Self::with_dirs(dirs)
    }

    /// An identity whose folders all sit under `root`, for tests and for runs
    /// that must not leave anything behind (`--demo`, `--snapshot`).
    pub fn under(root: &std::path::Path) -> Self {
        Self::with_dirs(AppDirs::under(root))
    }

    fn with_dirs(dirs: AppDirs) -> Self {
        Self {
            dirs,
            keyring_service: keyring_service_name(),
        }
    }

    /// The only account this app opens in phase 2. The folder is
    /// `<state>/accounts/<id>`, a layout ZapFast itself reads, which is why it
    /// must be under this app's own state folder and never ZapFast's.
    pub fn account(&self) -> AccountDirs {
        self.dirs.account(&AccountId::first())
    }

    /// Files this process's archive keys under the app's keyring service.
    /// Call it once, before the backend starts: a key read before it settles
    /// the service as ZapFast's, and this then fails.
    pub fn apply_keyring_service(&self) -> Result<()> {
        zapfast::archive::set_keyring_service(&self.keyring_service)
            .context("the keyring service could not be set")
    }

    /// The slot that keeps one copy of this app running per user. Its folder
    /// and its name are this app's own, so a running ZapFast neither blocks
    /// this app nor receives its requests.
    pub fn instance_slot(&self) -> Slot {
        Slot::at(&self.dirs.runtime, APP_ID)
    }

    /// The login item's registration. Nothing registers it in phase 2; it is
    /// here so that when something does, it cannot be ZapFast's.
    pub fn autostart(&self) -> Autostart {
        Autostart::new(APP_ID, APP_NAME)
    }

    /// Every folder the app may write to.
    pub fn folders(&self) -> [PathBuf; 4] {
        [
            self.dirs.config.clone(),
            self.dirs.state.clone(),
            self.dirs.cache.clone(),
            self.dirs.runtime.clone(),
        ]
    }
}

/// The bundle identifier the platform knows the app by (notifications, the
/// login item on macOS). Never ZapFast's `me.paolino.fastsapp`.
pub fn bundle_id() -> String {
    format!("me.paolino.{APP_ID}")
}

/// The keyring service of this app, never ZapFast's.
pub fn keyring_service_name() -> String {
    format!("me.paolino.{APP_ID}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use zapfast::archive::DEFAULT_KEYRING_SERVICE;

    /// ZapFast's names, as its own code spells them.
    const ZAPFAST_NAMES: [&str; 3] = ["zapfast", "fastsapp", "fastwhatsapp"];

    #[test]
    fn the_app_id_is_not_a_zapfast_name() {
        for name in ZAPFAST_NAMES {
            assert_ne!(APP_ID, name);
        }
        assert!(
            APP_ID
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'),
            "APP_ID is a filesystem and keyring name: {APP_ID:?}"
        );
    }

    #[test]
    fn data_folders_share_nothing_with_zapfasts() {
        let ours = Identity::discover();
        for name in ZAPFAST_NAMES {
            let Some(theirs) = AppDirs::of(name) else {
                continue; // No home directory here.
            };
            for ours in ours.folders() {
                for theirs in [
                    &theirs.config,
                    &theirs.state,
                    &theirs.cache,
                    &theirs.runtime,
                ] {
                    assert!(
                        !ours.starts_with(theirs) && !theirs.starts_with(&ours),
                        "{ours:?} overlaps {name}'s {theirs:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn the_account_folder_is_inside_the_apps_own_state() {
        let ours = Identity::discover();
        let account = ours.account();
        assert!(account.state.starts_with(&ours.dirs.state));
        assert!(account.cache.starts_with(&ours.dirs.cache));
        assert!(account.session_db().starts_with(&ours.dirs.state));
        assert!(account.archive_db().starts_with(&ours.dirs.state));
        if let Some(theirs) = AppDirs::of("zapfast") {
            let theirs = theirs.as_account();
            assert_ne!(account.state, theirs.state);
            assert_ne!(account.session_db(), theirs.session_db());
            assert_ne!(account.archive_db(), theirs.archive_db());
        }
    }

    #[test]
    fn the_settings_file_is_in_the_apps_own_config_folder() {
        let ours = Identity::discover();
        let file = ours.dirs.settings_file();
        assert!(file.starts_with(&ours.dirs.config), "{file:?}");
        for name in ZAPFAST_NAMES {
            if let Some(theirs) = AppDirs::of(name) {
                assert_ne!(file, theirs.settings_file(), "{name}'s settings file");
                assert!(!file.starts_with(&theirs.config), "inside {name}'s folder");
            }
        }
    }

    #[test]
    fn the_keyring_service_is_not_zapfasts() {
        assert_ne!(keyring_service_name(), DEFAULT_KEYRING_SERVICE);
        assert!(keyring_service_name().contains(APP_ID));
        assert_eq!(Identity::discover().keyring_service, keyring_service_name());
    }

    /// The same folder name under the two services is two keyring entries, so
    /// even an app pointed at ZapFast's folder by mistake could not read
    /// ZapFast's key.
    #[test]
    fn the_same_archive_folder_is_two_entries_under_the_two_services() {
        use keyring_core::api::CredentialStoreApi as _;
        let store = keyring_core::mock::Store::new().unwrap();
        let zapfast = store
            .build(DEFAULT_KEYRING_SERVICE, "archive-x", None)
            .unwrap();
        let ours = store
            .build(&keyring_service_name(), "archive-x", None)
            .unwrap();
        zapfast.set_secret(&[7; 32]).unwrap();
        assert!(matches!(
            ours.get_secret(),
            Err(keyring_core::Error::NoEntry)
        ));
    }

    #[test]
    fn the_instance_slot_is_not_zapfasts() {
        let ours = Identity::discover().instance_slot();
        for name in ZAPFAST_NAMES {
            let Some(theirs) = AppDirs::of(name) else {
                continue;
            };
            let theirs = Slot::at(&theirs.runtime, "fastsapp");
            assert_ne!(ours, theirs);
            assert!(!ours.dir().starts_with(theirs.dir()));
            assert!(!theirs.dir().starts_with(ours.dir()));
        }
        // The wire name differs too, so even a shared folder would not make
        // the two copies answer each other.
        assert!(format!("{ours:?}").contains(APP_ID));
        assert!(!format!("{ours:?}").contains("fastsapp"));
    }

    #[test]
    fn the_login_item_is_not_zapfasts() {
        assert_ne!(
            Identity::discover().autostart(),
            Autostart::new("zapfast", "ZapFast")
        );
        assert_ne!(
            Identity::discover().autostart(),
            Autostart::new("zapfast", APP_NAME)
        );
    }

    #[test]
    fn a_rooted_identity_stays_under_its_root() {
        let root = tempfile::tempdir().unwrap();
        let identity = Identity::under(root.path());
        for folder in identity.folders() {
            assert!(folder.starts_with(root.path()), "{folder:?}");
        }
        let account = identity.account();
        account.ensure().unwrap();
        assert!(account.state.starts_with(root.path()));
        assert!(account.cache.starts_with(root.path()));
        assert!(account.state.is_dir() && account.cache.is_dir());
    }
}
