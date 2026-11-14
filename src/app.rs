//! The window: accounts down the left, the chosen account's settings on the right (like Windows Settings).
//! All the work happens in engine.rs; closing the window hides it to the tray.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use eframe::egui::{self, Color32, CornerRadius, FontFamily, Frame, Margin, RichText, Sense, Stroke, Ui};

use crate::config::Priority;
use crate::engine::{self, Engine, State, UpdateState, gb};

const BG: Color32 = Color32::from_rgb(10, 10, 10);
const NAV: Color32 = Color32::from_rgb(0, 0, 0);
const CARD: Color32 = Color32::from_rgb(21, 21, 21);
const CARD_HI: Color32 = Color32::from_rgb(30, 30, 30);
const LINE: Color32 = Color32::from_rgb(38, 38, 38);
const TEXT: Color32 = Color32::from_rgb(242, 242, 242);
const DIM: Color32 = Color32::from_rgb(180, 180, 180);
const FAINT: Color32 = Color32::from_rgb(142, 142, 142);   // 5.6:1 on CARD
const OFF: Color32 = Color32::from_rgb(64, 64, 64);
const GOOD: Color32 = Color32::from_rgb(74, 222, 128);
const BAD: Color32 = Color32::from_rgb(248, 113, 113);

pub fn run() -> eframe::Result {
    crate::system::enable_privileges();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_title("Potato")
            .with_icon(eframe::icon_data::from_png_bytes(include_bytes!("../assets/icon.png")).unwrap_or_default())
            .with_inner_size([1000.0, 760.0]).with_min_inner_size([820.0, 560.0]),
        ..Default::default()
    };
    eframe::run_native("Potato", options, Box::new(|cc| Ok(Box::new(App::new(cc)))))
}

/// Windows' "Animation effects" setting: off turns every animation here off (reduced motion).
static MOTION: AtomicBool = AtomicBool::new(true);

/// An animation's length, or none when Windows has animations off. Kept short: snappy, under the 400 ms
/// people notice as waiting.
fn dur(seconds: f32) -> f32 {
    if MOTION.load(Ordering::Relaxed) { seconds } else { 0.0 }
}

#[derive(Clone, PartialEq)]
enum Page {
    Account(String),
    WholePc,
}

pub struct App {
    engine: Arc<Mutex<Engine>>,
    page: Page,
    /// Restart All was clicked once and asks for a second click until then
    confirm_restart: Option<Instant>,
    update_dismissed: bool,
    /// what's typed in each account's FPS box
    fps_text: HashMap<String, String>,
    /// what's typed in the boxes whose value only counts once it's complete (a time, a link)
    shutdown_text: Option<String>,
    webhook_text: Option<String>,
    /// the header action running in the background, if any (so the window never waits on it)
    busy: Arc<Mutex<Option<&'static str>>>,
    /// the last result shown as a toast, and when it appeared (the window's own clock, so animation stays in step)
    toast: Option<(String, f64)>,
    /// the window, so the tray can show it again (None off Windows: closing then really closes)
    #[cfg_attr(not(windows), allow(dead_code))]
    hwnd: Option<isize>,
    #[cfg(windows)]
    _tray: Option<tray_icon::TrayIcon>,
}

impl App {
    pub fn new(cc: &eframe::CreationContext) -> App {
        MOTION.store(crate::system::animations_on(), Ordering::Relaxed);
        style(&cc.egui_ctx);
        let ctx = cc.egui_ctx.clone();
        let engine = Engine::start(move || ctx.request_repaint());
        let first = engine.lock().unwrap().profiles.first().map(|p| p.sid.clone());
        let hwnd = window_handle(cc);
        App {
            #[cfg(windows)]
            _tray: hwnd.and_then(|h| crate::tray::create(h, cc.egui_ctx.clone(), engine.clone())),
            engine,
            page: first.map_or(Page::WholePc, Page::Account),
            confirm_restart: None,
            update_dismissed: false,
            fps_text: HashMap::new(),
            shutdown_text: None,
            webhook_text: None,
            busy: Arc::default(),
            toast: None,
            hwnd,
        }
    }
}

#[cfg(windows)]
fn window_handle(cc: &eframe::CreationContext) -> Option<isize> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    match cc.window_handle().ok()?.as_raw() {
        RawWindowHandle::Win32(w) => Some(w.hwnd.get()),
        _ => None,
    }
}

#[cfg(not(windows))]
fn window_handle(_: &eframe::CreationContext) -> Option<isize> {
    None
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        #[cfg(windows)]
        if let Some(h) = self.hwnd && ui.ctx().input(|i| i.viewport().close_requested()) {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::CancelClose);
            crate::tray::hide(h);   // keeps working in the tray; Quit Potato is in the tray menu
        }
        let engine = self.engine.clone();
        let mut e = engine.lock().unwrap();
        let before = e.config.clone();
        egui::Panel::left("nav").exact_size(250.0).resizable(false).frame(Frame::new().fill(NAV).inner_margin(Margin::same(14)))
            .show(ui, |ui| self.nav(ui, &e));
        egui::CentralPanel::default().frame(Frame::new().fill(BG).inner_margin(Margin::symmetric(32, 24))).show(ui, |ui| {
            egui::ScrollArea::vertical().id_salt("page").show(ui, |ui| {
                ui.set_width(ui.available_width().min(860.0));   // rows don't stretch across a wide screen
                self.banner(ui, &e);
                self.header(ui, &mut e);
                match self.page.clone() {
                    Page::Account(sid) => self.account_page(ui, &mut e, &sid),
                    Page::WholePc => self.pc_page(ui, &mut e),
                }
            });
        });
        if !e.note.is_empty() {
            self.toast = Some((std::mem::take(&mut e.note), ui.ctx().input(|i| i.time)));
        }
        self.draw_toast(ui.ctx());
        if e.config != before {
            e.changed();
        }
    }
}

impl App {
    fn nav(&mut self, ui: &mut Ui, e: &Engine) {
        ui.horizontal(|ui| {
            logo(ui, 1.6);
            ui.label(semi("Potato", 15.0).color(TEXT));
        });
        ui.add_space(14.0);
        let (used, total) = e.memory;
        Frame::new().fill(CARD).corner_radius(CornerRadius::same(8)).inner_margin(Margin::same(12)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            // counts to the new value instead of jumping (follow through)
            let shown = ui.ctx().animate_value_with_time(egui::Id::unique("ram-used"), used as f32, dur(0.6));
            ui.label(semi(&gb(shown as u64), 22.0).color(TEXT));
            ui.label(RichText::new(format!("of {} RAM in use", gb(total))).size(12.0).color(FAINT));
            let share = if total > 0 { used as f32 / total as f32 } else { 0.0 };
            ui.add(egui::ProgressBar::new(share).desired_height(4.0).fill(if share > 0.85 { BAD } else { TEXT }));
            let roblox: u64 = e.clients.iter().map(|c| c.ram).sum();
            ui.label(RichText::new(format!("Roblox {} · Saved {}", gb(roblox), gb(e.freed))).size(12.0).color(FAINT));
        });
        ui.add_space(14.0);
        ui.label(RichText::new("Accounts").size(12.0).color(FAINT));
        let mut marker = None;
        for p in &e.profiles {
            let ram: u64 = e.clients.iter().filter(|c| c.sid == p.sid).map(|c| c.ram).sum();
            let running = e.clients.iter().any(|c| c.sid == p.sid);
            let right = if !e.config.account(&p.sid).on { "Off".to_string() } else if running { gb(ram) } else { "Closed".into() };
            let picked = self.page == Page::Account(p.sid.clone());
            let r = nav_item(ui, &p.name, &right, Some(if running { GOOD } else { OFF }), picked);
            if picked {
                marker = Some(r.rect);
            }
            if r.clicked() {
                self.page = Page::Account(p.sid.clone());
            }
        }
        if e.profiles.is_empty() {
            ui.label(RichText::new("No account with Roblox yet").color(FAINT));
        }
        ui.add_space(14.0);
        ui.label(RichText::new("Potato").size(12.0).color(FAINT));
        let r = nav_item(ui, "Whole PC", "", None, self.page == Page::WholePc);
        if self.page == Page::WholePc {
            marker = Some(r.rect);
        }
        if r.clicked() {
            self.page = Page::WholePc;
        }
        // the marker slides from the old choice to the new one (post to post, slow in and out)
        if let Some(rect) = marker {
            let y = ui.ctx().animate_value_with_time(egui::Id::unique("nav-marker"), rect.top(), dur(0.2));
            ui.painter().rect_filled(egui::Rect::from_min_size(egui::pos2(rect.left(), y + 11.0), egui::vec2(3.0, 18.0)), 2.0, TEXT);
        }
    }

    /// Results float up at the bottom and fade away (staging, exaggerated feedback).
    fn draw_toast(&mut self, ctx: &egui::Context) {
        let Some((text, at)) = &self.toast else { return };
        let age = (ctx.input(|i| i.time) - at) as f32;
        let (show, life) = (4.0, dur(0.22).max(0.001));
        if age > show + life {
            self.toast = None;
            return;
        }
        let come = egui::emath::easing::back_out((age / life).min(1.0));
        let go = 1.0 - ((age - show) / life).clamp(0.0, 1.0);
        egui::Area::new(egui::Id::unique("toast")).anchor(egui::Align2::CENTER_BOTTOM, egui::vec2(125.0, -24.0 + (1.0 - come) * 18.0))
            .interactable(false).show(ctx, |ui| {
                ui.set_opacity(come.min(1.0) * go);
                Frame::new().fill(TEXT).corner_radius(CornerRadius::same(10)).inner_margin(Margin::symmetric(18, 11)).show(ui, |ui| {
                    ui.add(egui::Label::new(semi(text, 13.5).color(NAV)).extend());   // one line, never squeezed by the window edge
                });
            });
        ctx.request_repaint();
    }

    /// Run a header action on its own thread, so the window answers at once (Doherty threshold).
    fn run_action(&self, name: &'static str, ctx: &egui::Context, work: impl FnOnce(&mut Engine) + Send + 'static) {
        let mut busy = self.busy.lock().unwrap();
        if busy.is_some() {
            return;
        }
        *busy = Some(name);
        let (engine, flag, ctx) = (self.engine.clone(), self.busy.clone(), ctx.clone());
        std::thread::spawn(move || {
            work(&mut engine.lock().unwrap());
            *flag.lock().unwrap() = None;
            ctx.request_repaint();
        });
    }

    fn banner(&mut self, ui: &mut Ui, e: &Engine) {
        let state = e.update.lock().unwrap().clone();
        let (title, note) = match &state {
            UpdateState::None => return,
            UpdateState::Available(_) if self.update_dismissed => return,
            UpdateState::Available(r) => (format!("Potato {} Is Out", r.tag), "Installs in a few seconds and restarts by itself.".to_string()),
            UpdateState::Installing(tag) => (format!("Installing Potato {tag}"), "Downloading and checking it.".to_string()),
            UpdateState::Failed(m) => ("Update Didn't Install".to_string(), m.clone()),
        };
        Frame::new().fill(CARD_HI).stroke(Stroke::new(1.0, LINE)).corner_radius(CornerRadius::same(8)).inner_margin(Margin::symmetric(16, 12)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(semi(&title, 14.0).color(TEXT));
                    ui.label(RichText::new(note).size(12.5).color(FAINT));
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| match state {
                    UpdateState::Available(r) => {
                        if primary(ui, "Update Now").clicked() {
                            engine::install_update(r, e.update.clone());
                        }
                        if ui.add(egui::Button::new(RichText::new("Later").color(DIM)).frame(false)).clicked() {
                            self.update_dismissed = true;
                        }
                    }
                    UpdateState::Installing(_) => { ui.spinner(); }
                    UpdateState::Failed(_) => {
                        if ui.button("Close").clicked() {
                            *e.update.lock().unwrap() = UpdateState::None;
                        }
                    }
                    UpdateState::None => {}
                });
            });
        });
        ui.add_space(18.0);
    }

    fn header(&mut self, ui: &mut Ui, e: &mut Engine) {
        let (title, crumb) = match &self.page {
            Page::Account(sid) => {
                let name = e.profiles.iter().find(|p| &p.sid == sid).map_or("Account".to_string(), |p| p.name.clone());
                let running = match e.clients.iter().filter(|c| &c.sid == sid).count() {
                    0 => "Roblox Closed".to_string(),
                    1 => "Roblox Running".to_string(),
                    n => format!("{n} Clients Running"),
                };
                let state = match e.states.get(sid) {
                    Some(State::Applied) => "Settings Applied",
                    Some(State::Waiting) => "Applies When Roblox Restarts",
                    Some(State::Failed) => "Couldn't Write Settings",
                    None => "Potato Is Off",
                };
                (name, format!("{running} · {state}"))
            }
            Page::WholePc => ("Whole PC".to_string(), "Settings for every account".to_string()),
        };
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(semi(&title, 28.0).color(TEXT));
                ui.label(RichText::new(crumb).size(13.0).color(FAINT));
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let busy = *self.busy.lock().unwrap();
                if action(ui, "Trim All Roblox", busy == Some("trim")).on_hover_text("Frees memory every Roblox client isn't using right now").clicked() {
                    self.run_action("trim", ui.ctx(), |e| e.trim_all(true));
                }
                let armed = self.confirm_restart.is_some_and(|t| t.elapsed() < Duration::from_secs(4));
                if action(ui, if armed { "Click Again To Restart" } else { "Restart All" }, busy == Some("restart"))
                    .on_hover_text("Closes every Roblox client and opens each one again in the same server").clicked() {
                    if armed {
                        self.confirm_restart = None;
                        self.run_action("restart", ui.ctx(), |e| e.restart_clients());
                    } else {
                        self.confirm_restart = Some(Instant::now());
                    }
                }
                if armed {
                    ui.ctx().request_repaint_after(Duration::from_millis(250));
                }
                if action(ui, "Clean Memory", busy == Some("clean")).on_hover_text("Empties Windows' standby memory").clicked() {
                    self.run_action("clean", ui.ctx(), |e| e.clean(true));
                }
            });
        });
        ui.add_space(10.0);
    }

    fn account_page(&mut self, ui: &mut Ui, e: &mut Engine, sid: &str) {
        let mut a = e.config.account(sid);
        group(ui, "Potato", |ui| {
            switch_row(ui, "Use Potato On This Account", "Off leaves this account's Roblox alone", true, &mut a.on);
        });
        // most used first, and no group longer than four rows (Hick's and Miller's laws)
        ui.add_enabled_ui(a.on, |ui| {
            group(ui, "Game", |ui| {
                row(ui, "FPS Cap", "Type a number, or leave it empty for no cap", false, |ui, id| {
                    let text = self.fps_text.entry(sid.to_string()).or_insert_with(|| a.plan.fps_cap.map_or(String::new(), |f| f.to_string()));
                    ui.label(RichText::new("FPS").color(FAINT));
                    let r = ui.add(egui::TextEdit::singleline(text).desired_width(64.0).hint_text("No Cap").align(egui::Align2::CENTER_CENTER)).labelled_by(id);
                    if r.changed() {
                        text.retain(|c| c.is_ascii_digit());   // "60fps", "60 FPS" and "60" all work
                        text.truncate(3);
                        a.plan.fps_cap = text.parse::<u32>().ok().filter(|f| *f > 0);
                    }
                });
                switch_row(ui, "Lowest Graphics", "Roblox's own Graphics Quality, all the way down", false, &mut a.plan.lowest_graphics);
                switch_row(ui, "Ultra Low", "Lowest textures, no grass, blocky shapes, gray sky, frozen lighting", false, &mut a.plan.ultra_low);
                switch_row(ui, "Open Maximized", "Fills the screen and keeps the title bar", true, &mut a.plan.maximized);
            });
            group(ui, "CPU", |ui| {
                row(ui, "Priority", "Lower keeps your PC smooth while Roblox runs", false, |ui, id| {
                    dropdown(ui, "priority", id, &mut a.priority, &Priority::ALL.map(|p| (p, p.label().to_string())));
                });
                row(ui, "CPU Limit", "Most of the PC's CPU one client may use. Empty for no limit", false, |ui, id| {
                    number_box(ui, id, &mut a.cpu_limit, 100, "%", "None");
                });
                switch_row(ui, "Efficiency Mode", "Windows 11 runs it slower to save CPU and heat. Can lower FPS: best for alts", true, &mut a.efficiency);
            });
            group(ui, "Clients", |ui| {
                let mut one = a.max_clients > 0;
                if switch_row(ui, "One Client Only", "If a second Roblox opens on this account, the older one closes", false, &mut one) {
                    a.max_clients = one as u32;
                }
                row(ui, "Restart Memory Hogs", "A client using more than this restarts into the same server. Empty for never", false, |ui, id| {
                    number_box(ui, id, &mut a.ram_limit_mb, 99_999, "MB", "Never");
                });
                switch_row(ui, "Rejoin After A Crash", "Reopens a client that closed by itself in the same server. Leave off if a macro rejoins for you", false, &mut a.rejoin_crashed);
                switch_row(ui, "Mute", "Silences this account's Roblox", true, &mut a.mute);
            });
        });
        ui.add_space(16.0);
        if e.profiles.len() > 1 && ui.add(egui::Button::new("Use These Settings For Every Account").min_size(egui::vec2(0.0, 36.0))).clicked() {
            for p in e.profiles.clone() {
                *e.config.account_mut(&p.sid) = a.clone();
                self.fps_text.remove(&p.sid);
            }
            e.note = "Copied To Every Account".into();
        }
        if a != e.config.account(sid) {
            *e.config.account_mut(sid) = a;
        }
    }

    fn pc_page(&mut self, ui: &mut Ui, e: &mut Engine) {
        let cores = e.cores as u32;
        let off_or = |n: u32, f: &dyn Fn(u32) -> String| if n == 0 { "Off".to_string() } else { f(n) };
        let c = &mut e.config;
        group(ui, "Memory", |ui| {
            let list = [0, 1, 2, 5, 10, 15, 30].map(|n| (n, off_or(n, &|n| format!("Every {n} Min"))));
            row(ui, "Trim Roblox RAM", "Moves memory Roblox isn't using to the page file", false, |ui, id| { dropdown(ui, "trim", id, &mut c.trim_minutes, &list); });
            row(ui, "Clean Memory", "Empties Windows' standby memory when RAM use goes over this. Empty for never", false, |ui, id| {
                ui.label(RichText::new("%").color(FAINT));
                let mut text = if c.clean_above == 0 { String::new() } else { c.clean_above.to_string() };
                if ui.add(egui::TextEdit::singleline(&mut text).desired_width(56.0).hint_text("Off").align(egui::Align2::CENTER_CENTER)).labelled_by(id).changed() {
                    text.retain(|ch| ch.is_ascii_digit());
                    c.clean_above = text.parse::<u32>().unwrap_or(0).min(99);
                }
            });
            switch_row(ui, "Notifications", "A Windows notification each time a trim or clean frees RAM", true, &mut c.notify);
        });
        group(ui, "CPU", |ui| {
            let list: Vec<(u32, String)> = [0, 1, 2, 3, 4, 6, 8].into_iter().filter(|n| *n <= cores).map(|n| (n, off_or(n, &|n| format!("{n} Each")))).collect();
            row(ui, "Own CPU Cores", &format!("Each client gets its own cores ({cores} on this PC)"), true, |ui, id| { dropdown(ui, "cores", id, &mut c.cores_per_client, &list); });
        });
        group(ui, "Clients", |ui| {
            let list = [0, 1, 2, 4, 6, 12, 24].map(|n| (n, off_or(n, &|n| format!("Every {n} {}", if n == 1 { "Hour" } else { "Hours" }))));
            row(ui, "Restart Clients", "Closes them and opens each in the same server", true, |ui, id| { dropdown(ui, "restart", id, &mut c.restart_hours, &list); });
        });
        group(ui, "Network And Power", |ui| {
            row(ui, "Upload Cap", "Limits how fast all of Roblox can upload, for slow internet. Empty for no cap", false, |ui, id| {
                number_box(ui, id, &mut c.upload_kbps, 1_000_000, "kbit/s", "None");
            });
            row(ui, "Turn PC Off At", "Every day at this time, like 6:30, 0630 or 6pm. Empty for never", true, |ui, id| {
                let text = self.shutdown_text.get_or_insert_with(|| c.shutdown_at.clone());
                let r = ui.add(egui::TextEdit::singleline(text).desired_width(84.0).hint_text("Never").align(egui::Align2::CENTER_CENTER)).labelled_by(id);
                let parsed = crate::config::parse_time(text);
                if r.changed() {
                    text.truncate(8);
                    if text.trim().is_empty() {
                        c.shutdown_at.clear();
                    } else if let Some(t) = parsed {
                        c.shutdown_at = crate::config::format_time(t);
                    }
                }
                if r.lost_focus() && let Some(t) = parsed {
                    *text = crate::config::format_time(t);   // show how it was understood
                }
                if !r.has_focus() && !text.trim().is_empty() && parsed.is_none() {   // not while still typing
                    ui.label(RichText::new("Like 6:30 or 6pm").size(12.5).color(BAD));
                }
            });
        });
        let mut test = false;
        group(ui, "Discord Report", |ui| {
            row(ui, "Webhook", "Discord channel settings, Integrations, Webhooks, Copy Webhook URL", false, |ui, id| {
                let text = self.webhook_text.get_or_insert_with(|| c.discord.webhook.clone());
                let hide = !text.is_empty();   // the link is a password for the channel
                let r = ui.add(egui::TextEdit::singleline(text).desired_width(260.0).hint_text("https://discord.com/api/webhooks/...").password(hide)).labelled_by(id);
                if r.changed() {
                    *text = text.trim().to_string();
                    if text.is_empty() || crate::config::Discord::valid_webhook(text) {
                        c.discord.webhook = text.clone();
                    }
                }
                if !text.is_empty() && !crate::config::Discord::valid_webhook(text) {
                    ui.label(RichText::new("Not a Discord webhook").size(12.5).color(BAD));
                }
            });
            switch_row(ui, "Screenshots", "Adds a picture of each Remote Desktop's Roblox. A minimized or disconnected one can't be captured", false, &mut c.discord.screenshots);
            row(ui, "Test It", "Sends a report now. Potato sends one by itself on every hour", true, |ui, id| {
                test = ui.add_enabled(!c.discord.webhook.is_empty(), egui::Button::new("Send Report")).labelled_by(id).clicked();
            });
        });
        if test {
            e.send_report_now();
            e.note = "Sending A Report To Discord".into();
        }
        let mut restore = false;
        group(ui, "Potato", |ui| {
            row(ui, "Restore Original Settings", "Puts back every Roblox file Potato changed and turns it off", false, |ui, id| {
                restore = ui.button("Restore").labelled_by(id).clicked();
            });
            row(ui, "Quit Potato", "Closing the window keeps Potato running in the tray", true, |ui, id| {
                if ui.button("Quit").labelled_by(id).clicked() {
                    std::process::exit(0);
                }
            });
        });
        if restore {
            e.restore();
            self.fps_text.clear();
        }
    }
}

fn semi(text: &str, size: f32) -> RichText {
    RichText::new(text).size(size).family(FontFamily::Name("semi".into()))
}

/// The pixel potato from assets/logo.txt, `px` points per pixel.
fn logo(ui: &mut Ui, px: f32) {
    let lines: Vec<&str> = include_str!("../assets/logo.txt").lines().filter(|l| !l.is_empty() && !l.starts_with('#')).collect();
    let colors: HashMap<char, Color32> = lines.iter().filter(|l| l.get(1..2) == Some("="))
        .filter_map(|l| Some((l.chars().next()?, Color32::from_hex(l.get(2..9)?).ok()?))).collect();
    let rows: Vec<&str> = lines.into_iter().filter(|l| l.get(1..2) != Some("=")).collect();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(rows[0].len() as f32 * px, rows.len() as f32 * px), Sense::hover());
    for (y, row) in rows.iter().enumerate() {
        for (x, k) in row.chars().enumerate() {
            let Some(&color) = colors.get(&k) else { continue };
            let min = rect.min + egui::vec2(x as f32 * px, y as f32 * px);
            ui.painter().rect_filled(egui::Rect::from_min_size(min, egui::vec2(px, px)), 0.0, color);
        }
    }
}

/// One entry in the left column: an optional status dot, a name, and a note on the right. The chosen one
/// gets a lighter background (its marker is drawn by nav, so it can slide). Tall rows: easy to hit.
fn nav_item(ui: &mut Ui, name: &str, right: &str, dot: Option<Color32>, picked: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 40.0), Sense::click());
    let p = ui.painter();
    let hover = ui.ctx().animate_bool_with_time(resp.id.with("hover"), resp.hovered() || picked, dur(0.12));
    if hover > 0.0 {
        p.rect_filled(rect, 6.0, if picked { CARD_HI } else { CARD.gamma_multiply(hover) });
    }
    let mut x = rect.left() + 14.0;
    if let Some(c) = dot {
        p.circle_filled(egui::pos2(x + 3.0, rect.center().y), 3.5, c);
        x += 16.0;
    }
    p.text(egui::pos2(x, rect.center().y), egui::Align2::LEFT_CENTER, name, egui::FontId::proportional(14.0), TEXT);
    p.text(egui::pos2(rect.right() - 12.0, rect.center().y), egui::Align2::RIGHT_CENTER, right, egui::FontId::proportional(12.5), FAINT);
    resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::Tab, true, picked, name));
    resp
}

/// A titled block of setting rows, like a group in Windows Settings.
fn group(ui: &mut Ui, title: &str, body: impl FnOnce(&mut Ui)) {
    ui.add_space(16.0);
    ui.label(semi(title, 14.0).color(TEXT));
    ui.add_space(6.0);
    Frame::new().fill(CARD).corner_radius(CornerRadius::same(8)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.spacing_mut().item_spacing.y = 0.0;
        body(ui);
    });
}

/// One setting: label and note on the left, its control on the right, a line under it unless it's `last`.
/// The control gets the label's id for `labelled_by`, so screen readers name it.
fn row(ui: &mut Ui, label: &str, note: &str, last: bool, control: impl FnOnce(&mut Ui, egui::Id)) {
    let r = Frame::new().inner_margin(Margin::symmetric(18, 12)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.spacing_mut().item_spacing.y = 3.0;
        ui.horizontal(|ui| {
            let id = ui.vertical(|ui| {
                let id = ui.label(RichText::new(label).size(14.0).color(TEXT)).id;
                if !note.is_empty() {
                    ui.label(RichText::new(note).size(12.5).color(FAINT));
                }
                id
            }).inner;
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| control(ui, id));
        });
    });
    if !last {
        let rect = r.response.rect;
        ui.painter().hline(rect.x_range().shrink(18.0), rect.bottom(), Stroke::new(1.0, LINE));
    }
}

/// A setting with an on/off switch where the whole row is the button (Fitts's law: a big target). Returns
/// true when it flipped.
fn switch_row(ui: &mut Ui, label: &str, note: &str, last: bool, on: &mut bool) -> bool {
    let bg = ui.painter().add(egui::Shape::Noop);   // hover tint, drawn under the row once its size is known
    let mut knob = egui::Rect::NOTHING;
    let r = Frame::new().inner_margin(Margin::symmetric(18, 12)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.spacing_mut().item_spacing.y = 3.0;
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(RichText::new(label).size(14.0).color(TEXT));
                if !note.is_empty() {
                    ui.label(RichText::new(note).size(12.5).color(FAINT));
                }
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                knob = ui.allocate_exact_size(egui::vec2(42.0, 22.0), Sense::hover()).0;
            });
        });
    });
    let rect = r.response.rect;
    let resp = ui.interact(rect, ui.scope_id().with(("switch", label)), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::Switch, ui.is_enabled(), *on, label));
    let flipped = resp.clicked();
    if flipped {
        *on = !*on;
    }
    let hover = ui.ctx().animate_bool_with_time(resp.id.with("hover"), resp.hovered(), dur(0.12));
    ui.painter().set(bg, egui::Shape::rect_filled(rect, 8.0, Color32::from_white_alpha((hover * 6.0) as u8)));
    paint_switch(ui, knob, *on, resp.is_pointer_button_down_on(), resp.id);
    if !last {
        ui.painter().hline(rect.x_range().shrink(18.0), rect.bottom(), Stroke::new(1.0, LINE));
    }
    flipped
}

/// The switch: the knob shrinks a touch when pressed (anticipation), stretches as it travels (squash and
/// stretch), overshoots and settles (follow through), all eased in and out.
fn paint_switch(ui: &Ui, rect: egui::Rect, on: bool, pressed: bool, id: egui::Id) {
    let ctx = ui.ctx();
    let t = ctx.animate_bool_with_time_and_easing(id.with("knob"), on, dur(0.24), egui::emath::easing::back_out);
    let press = ctx.animate_bool_with_time(id.with("press"), pressed, dur(0.08));
    let fill = ctx.animate_bool_with_time(id.with("fill"), on, dur(0.16));
    let r = rect.height() / 2.0;
    let p = ui.painter();
    p.rect_filled(rect, r, Color32::from_gray(0).lerp_to_gamma(TEXT, fill));
    p.rect_stroke(rect, r, Stroke::new(1.0, FAINT.lerp_to_gamma(TEXT, fill)), egui::StrokeKind::Inside);
    let travel = (t.clamp(0.0, 1.0) * std::f32::consts::PI).sin();   // biggest mid-way
    let kr = (r - 5.0) * (1.0 - 0.18 * press);
    let (w, h) = (kr * (1.0 + 0.55 * travel), kr * (1.0 - 0.12 * travel));
    let x = egui::lerp((rect.left() + r)..=(rect.right() - r), t);
    let knob = egui::Rect::from_center_size(egui::pos2(x, rect.center().y), egui::vec2(w * 2.0, h * 2.0));
    p.rect_filled(knob, h, DIM.lerp_to_gamma(NAV, fill));
}

/// A box for a whole number up to `max`, with its unit after it; empty means 0 (off).
/// A number that counts once typing is done (Enter or clicking away): while typing 2500 MB the limit would
/// otherwise be 2, 25 and 250 MB for a moment, and clients over those would restart.
fn number_box(ui: &mut Ui, label: egui::Id, value: &mut u32, max: u32, unit: &str, empty: &str) {
    ui.label(RichText::new(unit).color(FAINT));
    let draft = label.with("draft");
    let mut text = ui.data(|d| d.get_temp::<String>(draft)).unwrap_or_else(|| if *value == 0 { String::new() } else { value.to_string() });
    let r = ui.add(egui::TextEdit::singleline(&mut text).desired_width(72.0).hint_text(empty).align(egui::Align2::CENTER_CENTER)).labelled_by(label);
    text.retain(|c| c.is_ascii_digit());
    if r.has_focus() {
        ui.data_mut(|d| d.insert_temp(draft, text));
    } else {
        if r.lost_focus() {
            *value = text.parse::<u32>().unwrap_or(0).min(max);
        }
        ui.data_mut(|d| d.remove::<String>(draft));
    }
}

/// A dropdown of fixed choices. Returns true when the choice changed.
fn dropdown<T: PartialEq + Copy>(ui: &mut Ui, salt: &str, label: egui::Id, value: &mut T, options: &[(T, String)]) -> bool {
    let shown = options.iter().find(|(v, _)| v == value).map_or(String::new(), |(_, l)| l.clone());
    let mut changed = false;
    let r = egui::ComboBox::from_id_salt(salt).selected_text(shown).width(150.0).show_ui(ui, |ui| {
        for (v, l) in options {
            changed |= ui.selectable_value(value, *v, l).changed();
        }
    });
    r.response.labelled_by(label);
    changed
}

/// The one white button on a screen, for the thing to do next: the header buttons' size and press, inverted.
fn primary(ui: &mut Ui, text: &str) -> egui::Response {
    button(ui, text, false, true)
}

/// The page header's buttons: one size and style, so they line up. They squash a little when pressed and
/// show a spinner while their work runs.
fn action(ui: &mut Ui, text: &str, busy: bool) -> egui::Response {
    button(ui, text, busy, false)
}

fn button(ui: &mut Ui, text: &str, busy: bool, primary: bool) -> egui::Response {
    let font = if primary { egui::FontId::new(13.5, FontFamily::Name("semi".into())) } else { egui::FontId::proportional(13.5) };
    let ink = if primary { NAV } else { TEXT };
    let galley = ui.painter().layout_no_wrap(text.to_string(), font, ink);
    let size = egui::vec2((galley.size().x + 32.0 + if busy { 20.0 } else { 0.0 }).max(120.0), 36.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, ui.is_enabled(), text));
    let ctx = ui.ctx();
    let press = ctx.animate_bool_with_time(resp.id.with("press"), resp.is_pointer_button_down_on(), dur(0.07));
    let hover = ctx.animate_bool_with_time(resp.id.with("hover"), resp.hovered(), dur(0.12));
    let rect = egui::Rect::from_center_size(rect.center(), rect.size() * (1.0 - 0.04 * press));
    let p = ui.painter();
    if primary {
        p.rect_filled(rect, 7.0, TEXT.lerp_to_gamma(Color32::from_gray(205), hover));
    } else {
        p.rect_filled(rect, 7.0, CARD_HI.lerp_to_gamma(Color32::from_gray(44), hover));
        p.rect_stroke(rect, 7.0, Stroke::new(1.0, LINE.lerp_to_gamma(Color32::from_gray(90), hover)), egui::StrokeKind::Inside);
    }
    let mut x = rect.center().x - galley.size().x / 2.0;
    if busy {
        x += 10.0;
        egui::Spinner::new().size(14.0).color(DIM).paint_at(ui, egui::Rect::from_center_size(egui::pos2(x - 14.0, rect.center().y), egui::vec2(14.0, 14.0)));
    }
    p.galley(egui::pos2(x, rect.center().y - galley.size().y / 2.0), galley, ink);
    resp
}

fn style(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert("geist".into(), Arc::new(egui::FontData::from_static(include_bytes!("../assets/fonts/Geist-Regular.ttf"))));
    fonts.font_data.insert("geist-semi".into(), Arc::new(egui::FontData::from_static(include_bytes!("../assets/fonts/Geist-SemiBold.ttf"))));
    fonts.families.entry(FontFamily::Proportional).or_default().insert(0, "geist".into());
    fonts.families.insert(FontFamily::Name("semi".into()), vec!["geist-semi".into(), "geist".into()]);
    ctx.set_missing_glyph_policy(egui::epaint::text::MissingGlyphPolicy::Tofu);
    ctx.set_fonts(fonts);

    let mut v = egui::Visuals::dark();
    // egui's dark-theme edge shading fattens dark text on the white buttons and toasts, and hinting snaps letters
    // to whole pixels, which looks jagged at 100% scaling: a middle gamma and no hinting keep every text smooth
    v.text_options.color_transfer_function = egui::epaint::FontColorTransferFunction::Gamma(0.8);
    v.text_options.font_hinting = false;
    v.panel_fill = BG;
    v.window_fill = CARD_HI;
    v.override_text_color = Some(TEXT);
    v.extreme_bg_color = Color32::from_rgb(14, 14, 14);   // text boxes
    v.selection.bg_fill = Color32::from_rgb(70, 70, 70);
    v.selection.stroke = Stroke::new(1.0, TEXT);
    for w in [&mut v.widgets.inactive, &mut v.widgets.hovered, &mut v.widgets.active, &mut v.widgets.open] {
        w.corner_radius = CornerRadius::same(6);
    }
    v.widgets.inactive.weak_bg_fill = CARD_HI;
    v.widgets.inactive.bg_fill = CARD_HI;
    v.widgets.inactive.bg_stroke = Stroke::new(1.0, LINE);
    v.widgets.hovered.weak_bg_fill = Color32::from_rgb(40, 40, 40);
    v.widgets.hovered.bg_stroke = Stroke::new(1.0, Color32::from_rgb(90, 90, 90));
    v.widgets.open.weak_bg_fill = Color32::from_rgb(40, 40, 40);
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, LINE);
    ctx.set_visuals(v);
    ctx.all_styles_mut(|s| {
        s.spacing.item_spacing = egui::vec2(8.0, 6.0);
        s.spacing.button_padding = egui::vec2(14.0, 7.0);
        s.text_styles.insert(egui::TextStyle::Body, egui::FontId::proportional(14.0));
        s.text_styles.insert(egui::TextStyle::Button, egui::FontId::proportional(13.5));
    });
}
