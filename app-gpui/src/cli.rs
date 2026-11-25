//! Command line flags.

use std::path::PathBuf;

/// Which automated run was asked for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BenchKind {
    /// Counts the renders of an app left alone: the CPU-at-rest check.
    Idle,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Cli {
    /// Runs on synthetic, deterministic data through the same event path as
    /// the real backend: no WhatsApp account, nothing written to disk.
    pub demo: bool,
    /// `--bench idle --seconds N`.
    pub bench: Option<(BenchKind, u64)>,
    /// Saves PNGs of the screens into this folder and quits.
    pub snapshot: Option<PathBuf>,
    /// Forces light or dark instead of following the OS (`Some(true)` is dark).
    pub dark: Option<bool>,
}

pub const USAGE: &str = "usage: zapfast-gpui [--demo] [--bench idle [--seconds N]] [--snapshot DIR] [--appearance light|dark]";

impl Cli {
    pub fn parse(args: impl IntoIterator<Item = String>) -> Result<Self, String> {
        let mut cli = Cli::default();
        let mut kind = None;
        let mut seconds = 8;
        let mut args = args.into_iter();
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--demo" => cli.demo = true,
                "--bench" => {
                    kind = Some(match args.next().as_deref() {
                        Some("idle") => BenchKind::Idle,
                        other => return Err(format!("--bench takes idle, not {other:?}")),
                    });
                }
                "--seconds" => {
                    seconds = args
                        .next()
                        .and_then(|n| n.parse().ok())
                        .filter(|n| *n >= 3)
                        .ok_or("--seconds takes a whole number of seconds, at least 3")?;
                }
                "--snapshot" => {
                    cli.snapshot = Some(args.next().ok_or("--snapshot takes a directory")?.into());
                }
                "--appearance" => {
                    cli.dark = Some(match args.next().as_deref() {
                        Some("dark") => true,
                        Some("light") => false,
                        _ => return Err("--appearance takes light or dark".into()),
                    });
                }
                other => return Err(format!("unknown argument {other}")),
            }
        }
        cli.bench = kind.map(|kind| (kind, seconds));
        Ok(cli)
    }

    /// Whether the run is automated: it must never take focus from the user,
    /// and it never touches the real account or the instance lock.
    pub fn automated(&self) -> bool {
        self.bench.is_some() || self.snapshot.is_some()
    }

    /// Whether the run uses synthetic data: `--demo`, and every automated run
    /// (a benchmark or a snapshot must not depend on, or touch, an account).
    pub fn synthetic(&self) -> bool {
        self.demo || self.automated()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<Cli, String> {
        Cli::parse(args.iter().map(|a| (*a).to_owned()))
    }

    #[test]
    fn flags() {
        assert_eq!(parse(&[]).unwrap(), Cli::default());
        let cli = parse(&["--demo", "--appearance", "dark"]).unwrap();
        assert!(cli.demo && cli.dark == Some(true) && !cli.automated());
        let cli = parse(&["--bench", "idle", "--seconds", "5"]).unwrap();
        assert_eq!(cli.bench, Some((BenchKind::Idle, 5)));
        assert!(cli.automated() && cli.synthetic());
        assert_eq!(parse(&["--bench", "idle"]).unwrap().bench.unwrap().1, 8);
        let cli = parse(&["--snapshot", "/tmp/out"]).unwrap();
        assert_eq!(cli.snapshot, Some("/tmp/out".into()));
        assert!(cli.synthetic());
    }

    #[test]
    fn a_real_run_is_neither_automated_nor_synthetic() {
        let cli = parse(&[]).unwrap();
        assert!(!cli.automated() && !cli.synthetic());
    }

    #[test]
    fn bad_flags_are_refused() {
        for args in [
            &["--bench"][..],
            &["--bench", "scroll"],
            &["--seconds", "x", "--bench", "idle"],
            &["--seconds", "1"],
            &["--snapshot"],
            &["--appearance", "sepia"],
            &["--nope"],
        ] {
            assert!(parse(args).is_err(), "{args:?}");
        }
    }
}
