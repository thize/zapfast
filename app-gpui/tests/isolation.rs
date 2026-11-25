//! The app must never reach what an installed ZapFast owns. These tests read
//! this crate's own sources for the calls that would, so a later change that
//! brings one in fails here rather than in somebody's keyring.

use std::path::{Path, PathBuf};

/// What this app must not call or name, and why.
const FORBIDDEN: &[(&str, &str)] = &[
    (
        "adopt_previous_names",
        "moves the folders of older ZapFast names into the app's own",
    ),
    (
        "adopt_single_account",
        "belongs to ZapFast's single-account migration",
    ),
    (
        "single_instance",
        "ZapFast's lock also talks to its legacy TCP port",
    ),
    ("47_119", "ZapFast's legacy instance port"),
    ("47119", "ZapFast's legacy instance port"),
    ("fastsapp", "ZapFast's instance wire name"),
    ("zapfast::autostart", "names ZapFast's login item"),
    ("zapfast::updates", "the updater is never run"),
    ("fastframe_update", "the updater is never run"),
    ("CheckForUpdates", "the updater is never asked"),
    ("InspectUpdate", "the updater is never asked"),
    ("DownloadUpdate", "the updater is never asked"),
    ("InstallUpdate", "the updater is never asked"),
    ("AppDirs::discover", "ZapFast's own folders"),
    ("rocks.zapfast.ZapFast", "ZapFast's keyring service"),
    ("\"zapfast\"", "ZapFast's name"),
];

/// Files that name those things on purpose, to forbid or compare them.
const ALLOWED_IN: &[&str] = &["identity.rs", "bridge.rs"];

fn sources(dir: &Path, found: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            sources(&path, found);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            found.push(path);
        }
    }
}

/// The part of a source file before its `#[cfg(test)]` module.
fn production_code(text: &str) -> &str {
    text.split("#[cfg(test)]").next().unwrap_or(text)
}

#[test]
fn no_source_reaches_zapfasts_names_or_updater() {
    let mut files = Vec::new();
    sources(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
        &mut files,
    );
    assert!(!files.is_empty());
    let mut problems = Vec::new();
    for file in files {
        let name = file.file_name().unwrap().to_string_lossy().into_owned();
        let text = std::fs::read_to_string(&file).unwrap();
        for (line_number, line) in production_code(&text).lines().enumerate() {
            let code = line.trim_start();
            // Comments may say what is not done.
            if code.starts_with("//") {
                continue;
            }
            for (needle, why) in FORBIDDEN {
                if line.contains(needle) && !ALLOWED_IN.contains(&name.as_str()) {
                    problems.push(format!("{name}:{}: `{needle}` ({why})", line_number + 1));
                }
            }
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

/// `identity.rs` and `bridge.rs` may name them only to refuse or compare.
#[test]
fn the_files_allowed_to_name_them_never_call_them() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    for name in ALLOWED_IN {
        let Ok(text) = std::fs::read_to_string(src.join(name)) else {
            continue; // Not written yet.
        };
        for line in production_code(&text).lines() {
            let code = line.trim_start();
            if code.starts_with("//") {
                continue;
            }
            for call in [
                "adopt_previous_names(",
                "adopt_single_account(",
                "single_instance::",
                "AppDirs::discover(",
                "updates::updater",
            ] {
                assert!(!code.contains(call), "{name}: {line}");
            }
        }
    }
}
