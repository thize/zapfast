//! GPUI chat spike: a WhatsApp-like client in ZapFast's look, built to answer
//! whether GPUI can carry the real port. See README.md.

mod app;
mod assets;
mod bench;
mod chat;
mod copy;
mod data;
mod markup;
mod media;
mod palette;
mod platform;
mod scenes;
mod selection;
mod shell;
mod timefmt;
mod ui;

/// GPUI reports some outcomes only through the `log` crate (a notification
/// that could not be delivered, accessibility being switched on). With
/// `ZAPFAST_SPIKE_LOG=1` they go to stderr.
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
    if std::env::var_os("ZAPFAST_SPIKE_LOG").is_some() {
        let _ = log::set_logger(&StderrLog).map(|()| log::set_max_level(log::LevelFilter::Info));
    }
    let cli = match bench::Cli::parse(std::env::args().skip(1)) {
        Ok(cli) => cli,
        Err(message) => {
            eprintln!("{message}");
            eprintln!(
                "usage: gpui-chat-spike [--scene chat|media|platform] [--bench idle|scroll|video] [--seconds N] [--snapshot DIR] [--paced] [--minimize] [--jump MESSAGE] [--appearance light|dark] [--probe notify-gpui|notify-fastframe|badge] [--a11y-dump FILE]"
            );
            std::process::exit(2);
        }
    };
    println!("PID {}", std::process::id());
    app::run(cli);
}
