//! ZapFast on GPUI. See README.md.

use zapfast_gpui::app::{self, Exit};
use zapfast_gpui::cli::{Cli, USAGE};

/// GPUI reports some outcomes only through the `log` crate (a notification
/// that could not be delivered, accessibility being switched on). With
/// `ZAPFAST_GPUI_LOG=1` they go to stderr.
struct StderrLog;

impl log::Log for StderrLog {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.level() <= log::Level::Info
    }

    fn log(&self, record: &log::Record<'_>) {
        if self.enabled(record.metadata()) {
            eprintln!(
                "[{}] {}: {}",
                record.level(),
                record.target(),
                record.args()
            );
        }
    }

    fn flush(&self) {}
}

fn main() {
    if std::env::var_os("ZAPFAST_GPUI_LOG").is_some() {
        let _ = log::set_logger(&StderrLog).map(|()| log::set_max_level(log::LevelFilter::Info));
    }
    let cli = match Cli::parse(std::env::args().skip(1)) {
        Ok(cli) => cli,
        Err(message) => {
            eprintln!("{message}");
            eprintln!("{USAGE}");
            std::process::exit(2);
        }
    };
    println!("PID {}", std::process::id());
    match app::run(cli) {
        Ok(()) => {}
        Err(Exit::AlreadyRunning) => {
            eprintln!("Already running: its window was asked to come forward.");
        }
        Err(Exit::Failed(message)) => {
            eprintln!("{message}");
            std::process::exit(1);
        }
    }
}
