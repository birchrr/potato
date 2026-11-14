#![cfg_attr(windows, windows_subsystem = "windows")]

use potato::{app, helper, system};

fn main() -> eframe::Result {
    // left behind by the last update (engine.rs install_update); the old copy may still be closing, and Windows
    // won't delete a running exe, so keep trying for half a minute
    if let Ok(me) = std::env::current_exe() {
        let old = me.with_extension("old");
        std::thread::spawn(move || {
            for _ in 0..30 {
                if std::fs::remove_file(&old).is_ok() || !old.exists() {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_secs(1));
            }
        });
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
