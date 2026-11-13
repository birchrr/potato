pub mod config;
pub mod files;
pub mod tweaks;
pub mod version;
pub mod app;
pub mod engine;
#[cfg(windows)]
pub mod tray;
#[cfg(windows)]
pub mod helper;
#[cfg(not(windows))]
#[path = "helper_demo.rs"]
pub mod helper;
#[cfg(windows)]
pub mod system;
// fake accounts and clients, so the window can be drawn and checked on any OS (tests/window.rs)
#[cfg(not(windows))]
#[path = "system_demo.rs"]
pub mod system;
pub mod rejoin;
pub mod report;
