#![cfg_attr(windows, windows_subsystem = "windows")]

use potato::{app, helper, system};

fn main() -> eframe::Result {
    // left behind by the last update (app.rs install_update)
    if let Ok(me) = std::env::current_exe() {
        let _ = std::fs::remove_file(me.with_extension("old"));
    }
    if std::env::args().any(|a| a == "--helper") {
        helper::run();   // runs as each logged-on user, without admin
        return Ok(());
    }
    // the app needs admin to reach other accounts' Roblox; Windows asks, and saying no just closes it
    if !system::is_admin() {
        system::relaunch_as_admin();
        return Ok(());
    }
    app::run()
}
