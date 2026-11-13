//! The window: accounts down the left, the chosen account's settings on the right (like Windows Settings).
//! All the work happens in engine.rs; closing the window hides it to the tray.

use std::collections::HashMap;
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
    /// the window, so the tray can show it again (None off Windows: closing then really closes)
    #[cfg_attr(not(windows), allow(dead_code))]
    hwnd: Option<isize>,
    #[cfg(windows)]
    _tray: Option<tray_icon::TrayIcon>,
}

impl App {
    pub fn new(cc: &eframe::CreationContext) -> App {
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
                if !e.note.is_empty() {
                    ui.add_space(14.0);
                    ui.label(RichText::new(&e.note).color(DIM));
                }
            });
        });
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
            ui.label(semi(&gb(used), 22.0).color(TEXT));
            ui.label(RichText::new(format!("of {} RAM in use", gb(total))).size(12.0).color(FAINT));
            let share = if total > 0 { used as f32 / total as f32 } else { 0.0 };
            ui.add(egui::ProgressBar::new(share).desired_height(4.0).fill(if share > 0.85 { BAD } else { TEXT }));
            let roblox: u64 = e.clients.iter().map(|c| c.ram).sum();
            ui.label(RichText::new(format!("Roblox {} · Saved {}", gb(roblox), gb(e.freed))).size(12.0).color(FAINT));
        });
        ui.add_space(14.0);
        ui.label(RichText::new("Accounts").size(12.0).color(FAINT));
        for p in &e.profiles {
            let ram: u64 = e.clients.iter().filter(|c| c.sid == p.sid).map(|c| c.ram).sum();
            let running = e.clients.iter().any(|c| c.sid == p.sid);
            let right = if !e.config.account(&p.sid).on { "Off".to_string() } else if running { gb(ram) } else { "Closed".into() };
            if nav_item(ui, &p.name, &right, Some(if running { GOOD } else { OFF }), self.page == Page::Account(p.sid.clone())).clicked() {
                self.page = Page::Account(p.sid.clone());
            }
        }
        if e.profiles.is_empty() {
            ui.label(RichText::new("No account with Roblox yet").color(FAINT));
        }
        ui.add_space(14.0);
        ui.label(RichText::new("Potato").size(12.0).color(FAINT));
        if nav_item(ui, "Whole PC", "", None, self.page == Page::WholePc).clicked() {
            self.page = Page::WholePc;
        }
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
                if action(ui, "Trim All Roblox").on_hover_text("Frees memory every Roblox client isn't using right now").clicked() {
                    e.trim_all(true);
                }
                let armed = self.confirm_restart.is_some_and(|t| t.elapsed() < Duration::from_secs(4));
                if action(ui, if armed { "Click Again To Restart" } else { "Restart All" })
                    .on_hover_text("Closes every Roblox client and opens each one again in the same server").clicked() {
                    if armed {
                        self.confirm_restart = None;
                        e.restart_clients();
                    } else {
                        self.confirm_restart = Some(Instant::now());
                    }
                }
                if armed {
                    ui.ctx().request_repaint_after(Duration::from_millis(250));
                }
                if action(ui, "Clean Memory").on_hover_text("Empties Windows' standby memory").clicked() {
                    e.clean(true);
                }
            });
        });
        ui.add_space(10.0);
    }

    fn account_page(&mut self, ui: &mut Ui, e: &mut Engine, sid: &str) {
        let mut a = e.config.account(sid);
        group(ui, "Potato", |ui| {
            row(ui, "Use Potato On This Account", "Off leaves this account's Roblox alone", true, |ui, id| { toggle(ui, &mut a.on).labelled_by(id); });
        });
        ui.add_enabled_ui(a.on, |ui| {
            group(ui, "Game", |ui| {
                row(ui, "Lowest Graphics", "Roblox's own Graphics Quality, all the way down", false, |ui, id| { toggle(ui, &mut a.plan.lowest_graphics).labelled_by(id); });
                row(ui, "Open Maximized", "Fills the screen and keeps the title bar", false, |ui, id| { toggle(ui, &mut a.plan.maximized).labelled_by(id); });
                row(ui, "FPS Cap", "Type a number, or leave it empty for no cap", false, |ui, id| {
                    let text = self.fps_text.entry(sid.to_string()).or_insert_with(|| a.plan.fps_cap.map_or(String::new(), |f| f.to_string()));
                    ui.label(RichText::new("FPS").color(FAINT));
                    let r = ui.add(egui::TextEdit::singleline(text).desired_width(64.0).hint_text("No Cap").align(egui::Align2::CENTER_CENTER)).labelled_by(id);
                    if r.changed() {
                        text.retain(|c| c.is_ascii_digit());
                        text.truncate(3);
                        a.plan.fps_cap = text.parse::<u32>().ok().filter(|f| *f > 0);
                    }
                });
                row(ui, "Ultra Low", "Lowest textures, lower resolution, no grass, blocky shapes, gray sky", true, |ui, id| { toggle(ui, &mut a.plan.ultra_low).labelled_by(id); });
            });
            group(ui, "CPU And Clients", |ui| {
                row(ui, "Priority", "Lower keeps your PC smooth while Roblox runs", false, |ui, id| {
                    dropdown(ui, "priority", id, &mut a.priority, &Priority::ALL.map(|p| (p, p.label().to_string())));
                });
                row(ui, "Efficiency Mode", "Windows 11 runs it slower to save CPU and heat. Can lower FPS: best for alts", false, |ui, id| { toggle(ui, &mut a.efficiency).labelled_by(id); });
                row(ui, "One Client Only", "If a second Roblox opens on this account, the older one closes", true, |ui, id| {
                    let mut one = a.max_clients > 0;
                    if toggle(ui, &mut one).labelled_by(id).changed() {
                        a.max_clients = one as u32;
                    }
                });
            });
        });
        ui.add_space(16.0);
        if e.profiles.len() > 1 && ui.button("Use These Settings For Every Account").clicked() {
            for p in e.profiles.clone() {
                *e.config.account_mut(&p.sid) = a.clone();
                self.fps_text.remove(&p.sid);
            }
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
            let list = [0, 70, 75, 80, 85, 90].map(|n| (n, off_or(n, &|n| format!("Above {n}%"))));
            row(ui, "Clean Memory", "Empties Windows' standby memory when the PC gets this full", true, |ui, id| { dropdown(ui, "clean", id, &mut c.clean_above, &list); });
        });
        group(ui, "CPU", |ui| {
            let list: Vec<(u32, String)> = [0, 1, 2, 3, 4, 6, 8].into_iter().filter(|n| *n <= cores).map(|n| (n, off_or(n, &|n| format!("{n} Each")))).collect();
            row(ui, "Own CPU Cores", &format!("Each client gets its own cores ({cores} on this PC)"), true, |ui, id| { dropdown(ui, "cores", id, &mut c.cores_per_client, &list); });
        });
        group(ui, "Clients", |ui| {
            let list = [0, 1, 2, 4, 6, 12, 24].map(|n| (n, off_or(n, &|n| format!("Every {n} {}", if n == 1 { "Hour" } else { "Hours" }))));
            row(ui, "Restart Clients", "Closes them and opens each in the same server", true, |ui, id| { dropdown(ui, "restart", id, &mut c.restart_hours, &list); });
        });
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
/// gets a lighter background and a bar on its left edge.
fn nav_item(ui: &mut Ui, name: &str, right: &str, dot: Option<Color32>, picked: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 36.0), Sense::click());
    let p = ui.painter();
    if picked || resp.hovered() {
        p.rect_filled(rect, 6.0, if picked { CARD_HI } else { CARD });
    }
    if picked {
        p.rect_filled(egui::Rect::from_min_size(rect.min + egui::vec2(0.0, 10.0), egui::vec2(3.0, 16.0)), 2.0, TEXT);
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

fn toggle(ui: &mut Ui, on: &mut bool) -> egui::Response {
    let (rect, mut resp) = ui.allocate_exact_size(egui::vec2(40.0, 20.0), Sense::click());
    if resp.clicked() {
        *on = !*on;
        resp.mark_changed();
    }
    resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::Switch, true, *on, ""));
    let t = ui.ctx().animate_bool_responsive(resp.id, *on);
    let r = rect.height() / 2.0;
    let p = ui.painter();
    if *on {
        p.rect_filled(rect, r, TEXT);
    } else {
        p.rect_stroke(rect, r, Stroke::new(1.0, FAINT), egui::StrokeKind::Inside);
    }
    let x = egui::lerp((rect.left() + r)..=(rect.right() - r), t);
    p.circle_filled(egui::pos2(x, rect.center().y), r - 5.0, if *on { NAV } else { DIM });
    resp
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

fn primary(ui: &mut Ui, text: &str) -> egui::Response {
    ui.add(egui::Button::new(semi(text, 13.5).color(NAV)).fill(TEXT).stroke(Stroke::NONE).min_size(egui::vec2(0.0, 34.0)))
}

/// The page header's buttons: one size and style, so they line up.
fn action(ui: &mut Ui, text: &str) -> egui::Response {
    ui.add(egui::Button::new(RichText::new(text).size(13.5)).min_size(egui::vec2(120.0, 36.0)))
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
