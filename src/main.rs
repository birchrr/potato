#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(windows)]
mod app;
#[cfg(windows)]
mod helper;
#[cfg(windows)]
mod system;

#[cfg(windows)]
fn main() -> eframe::Result {
    if std::env::args().any(|a| a == "--helper") {
        helper::run();
        return Ok(());
    }
    app::run()
}

#[cfg(not(windows))]
fn main() {
    eprintln!("Potato runs on Windows.");
}
