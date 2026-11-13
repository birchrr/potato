#![cfg_attr(windows, windows_subsystem = "windows")]

use potato::{app, helper, system};

fn main() -> eframe::Result {
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
