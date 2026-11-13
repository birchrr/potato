//! The tray icon next to the clock. Closing the window only hides it; Potato keeps working (engine.rs) until
//! Quit Potato in the tray menu.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use eframe::egui;
use tray_icon::menu::{Menu, MenuEvent, MenuItem};
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::{SW_HIDE, SW_SHOW, SetForegroundWindow, ShowWindow};

use crate::engine::Engine;

/// Keep the returned icon alive for as long as Potato runs; dropping it removes it from the tray.
pub fn create(hwnd: isize, ctx: egui::Context, engine: Arc<Mutex<Engine>>) -> Option<TrayIcon> {
    let icon = eframe::icon_data::from_png_bytes(include_bytes!("../assets/icon.png")).ok()?;
    let (open, trim, quit) = (MenuItem::new("Open Potato", true, None), MenuItem::new("Trim All Roblox", true, None), MenuItem::new("Quit Potato", true, None));
    let menu = Menu::new();
    menu.append_items(&[&open, &trim, &quit]).ok()?;
    let tray = TrayIconBuilder::new().with_tooltip("Potato").with_menu(Box::new(menu)).with_menu_on_left_click(false)
        .with_icon(Icon::from_rgba(icon.rgba, icon.width, icon.height).ok()?).build().ok()?;
    let (open, trim, quit) = (open.id().clone(), trim.id().clone(), quit.id().clone());
    std::thread::spawn(move || loop {
        while let Ok(e) = MenuEvent::receiver().try_recv() {
            if e.id == open {
                show(hwnd, &ctx);
            } else if e.id == trim {
                engine.lock().unwrap().trim_all(true);
            } else if e.id == quit {
                std::process::exit(0);   // settings are saved the moment they change
            }
        }
        while let Ok(e) = TrayIconEvent::receiver().try_recv() {
            if matches!(e, TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. }) {
                show(hwnd, &ctx);
            }
        }
        std::thread::sleep(Duration::from_millis(100));
    });
    Some(tray)
}

pub fn show(hwnd: isize, ctx: &egui::Context) {
    unsafe {
        let h = HWND(hwnd as *mut _);
        let _ = ShowWindow(h, SW_SHOW);
        let _ = SetForegroundWindow(h);
    }
    ctx.request_repaint();
}

pub fn hide(hwnd: isize) {
    unsafe {
        let _ = ShowWindow(HWND(hwnd as *mut _), SW_HIDE);
    }
}
