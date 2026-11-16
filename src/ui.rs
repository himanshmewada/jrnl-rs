use colored::Colorize;
use std::io::{IsTerminal, Write};
use std::sync::atomic::{AtomicBool, Ordering};

pub static DEBUG: AtomicBool = AtomicBool::new(false);

pub fn init(debug: bool) {
    DEBUG.store(debug, Ordering::Relaxed);
    if !std::io::stdout().is_terminal() {
        colored::control::set_override(false);
    }
}

#[macro_export]
macro_rules! debug {
    ($($t:tt)*) => {
        if $crate::ui::DEBUG.load(std::sync::atomic::Ordering::Relaxed) {
            eprintln!("{}", format!("[debug] {}", format!($($t)*)));
        }
    };
}

pub fn error(msg: &str) {
    eprintln!("{} {}", "Error:".red().bold(), msg);
}
pub fn warning(msg: &str) {
    eprintln!("{} {}", "Warning:".yellow().bold(), msg);
}
pub fn normal(msg: &str) {
    eprintln!("{msg}");
}

pub fn prompt(question: &str) -> String {
    eprint!("{question}");
    std::io::stderr().flush().ok();
    let mut s = String::new();
    std::io::stdin().read_line(&mut s).unwrap_or(0);
    s.trim_end_matches(['\n', '\r']).to_string()
}

pub fn prompt_password(question: &str) -> String {
    rpassword::prompt_password(question).unwrap_or_default()
}

pub fn yes_no(question: &str, default: bool) -> bool {
    let suffix = if default { "[Y/n]" } else { "[y/N]" };
    match prompt(&format!("{question} {suffix} "))
        .trim()
        .to_lowercase()
        .as_str()
    {
        "y" | "yes" => true,
        "n" | "no" => false,
        _ => default,
    }
}
