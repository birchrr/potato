#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(windows)]
mod app;
#[cfg(windows)]
mod system;

#[cfg(windows)]
fn main() -> eframe::Result {
    app::run()
}

#[cfg(not(windows))]
fn main() {
    eprintln!("Potato runs on Windows.");
}
