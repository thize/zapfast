//! GPUI chat spike: a WhatsApp-like client in ZapFast's look, built to answer
//! whether GPUI can carry the real port. See README.md.

mod app;
mod assets;
mod bench;
mod chat;
mod copy;
mod data;
mod markup;
mod palette;
mod scenes;
mod selection;
mod shell;
mod timefmt;
mod ui;

fn main() {
    let cli = match bench::Cli::parse(std::env::args().skip(1)) {
        Ok(cli) => cli,
        Err(message) => {
            eprintln!("{message}");
            eprintln!(
                "usage: gpui-chat-spike [--scene chat|media|platform] [--bench idle|scroll] [--seconds N] [--snapshot DIR] [--jump MESSAGE]"
            );
            std::process::exit(2);
        }
    };
    println!("PID {}", std::process::id());
    app::run(cli);
}
