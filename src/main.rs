mod args;
mod config;
mod controller;
mod crypto;
mod dates;
mod dayone;
mod editor;
mod entry;
mod export;
mod journal;
mod path;
mod stats;
mod ui;

fn main() {
    let raw: Vec<String> = std::env::args().skip(1).collect();
    let args = match args::parse(&raw) {
        Ok(a) => a,
        Err(e) => {
            ui::error(&e);
            eprintln!("Try 'jrnl --help' for usage.");
            std::process::exit(2);
        }
    };
    ui::init(args.debug);
    match controller::run(args) {
        Ok(code) => std::process::exit(code),
        Err(e) => {
            ui::error(&format!("{e:#}"));
            std::process::exit(1);
        }
    }
}
