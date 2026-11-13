//! The window, and the loop that keeps every chosen user's Roblox light.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use eframe::egui::{self, Color32, CornerRadius, Frame, Margin, RichText, Stroke, Ui};
use potato::config::{Config, Priority};
use potato::files;
use potato::tweaks::{MACRO_SAFE_MIN_FPS, core_masks};

use crate::system::{self, Client, Cpu, Profile};

const TICK: Duration = Duration::from_secs(2);

const BG: Color32 = Color32::from_rgb(11, 11, 12);
const CARD: Color32 = Color32::from_rgb(20, 20, 22);
const LINE: Color32 = Color32::from_rgb(38, 38, 42);
const TEXT: Color32 = Color32::from_rgb(237, 237, 237);
const DIM: Color32 = Color32::from_rgb(140, 140, 148);
const ACCENT: Color32 = Color32::from_rgb(217, 163, 91);   // the logo's potato skin
const GOOD: Color32 = Color32::from_rgb(74, 222, 128);
const WARN: Color32 = Color32::from_rgb(248, 113, 113);

pub fn run() -> eframe::Result {
    system::enable_privileges();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_title("Potato")
            .with_icon(eframe::icon_data::from_png_bytes(include_bytes!("../assets/icon.png")).unwrap_or_default()).with_inner_size([540.0, 780.0]).with_min_inner_size([460.0, 520.0]),
        ..Default::default()
    };
    eframe::run_native("Potato", options, Box::new(|cc| Ok(Box::new(App::new(&cc.egui_ctx)))))
}

fn config_path() -> PathBuf {
    let base = std::env::var_os("ProgramData").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    base.join("Potato").join("settings.json")
}

/// Where each user's settings stand.
#[derive(Clone, Copy, PartialEq)]
enum State {
    Applied,
    /// their Roblox is open; Potato writes the moment it closes
    Waiting,
    Failed,
}

struct App {
    config: Config,
    admin: bool,
    clients: Vec<Client>,
    profiles: Vec<Profile>,
    states: HashMap<String, State>,
    /// the config each user's files were last written with
    applied: HashMap<String, Config>,
    memory: (u64, u64),
    /// what each client's CPU settings were last set to, so they're only set again when something changes
    cpu_set: HashMap<u32, (Priority, Option<u64>, bool)>,
    cores: usize,
    last_tick: Option<Instant>,
    last_trim: Instant,
    last_clean: Instant,
    note: String,
}

impl App {
    fn new(ctx: &egui::Context) -> App {
        style(ctx);
        let config = std::fs::read_to_string(config_path()).map(|t| Config::from_json(&t)).unwrap_or_default();
        let mut app = App {
            config,
            admin: system::is_admin(),
            clients: Vec::new(),
            profiles: system::profiles(),
            states: HashMap::new(),
            applied: HashMap::new(),
            memory: (0, 0),
            cpu_set: HashMap::new(),
            cores: system::logical_cores(),
            last_tick: None,
            last_trim: Instant::now(),
            last_clean: Instant::now(),
            note: String::new(),
        };
        if app.config.users.is_empty() {
            app.config.users = app.profiles.iter().map(|p| p.sid.clone()).collect();   // first run: everyone
        }
        app
    }

    fn save_config(&self) {
        let path = config_path();
        let _ = std::fs::create_dir_all(path.parent().unwrap());
        let _ = std::fs::write(path, self.config.to_json());
    }

    fn tick(&mut self) {
        if self.last_tick.is_some_and(|t| t.elapsed() < TICK) {
            return;
        }
        self.last_tick = Some(Instant::now());
        self.clients = system::clients();
        self.memory = system::memory();
        let running: HashSet<&str> = self.clients.iter().map(|c| c.sid.as_str()).collect();
        for p in &self.profiles {
            if !self.config.users.contains(&p.sid) {
                self.states.remove(&p.sid);
                continue;
            }
            let state = if running.contains(p.sid.as_str()) {
                if self.applied.get(&p.sid) == Some(&self.config) { State::Applied } else { State::Waiting }
            } else {
                // Roblox rewrites its settings when it closes, so check again every tick it's closed
                match files::apply(&p.local, &self.config.plan) {
                    Ok(_) => {
                        self.applied.insert(p.sid.clone(), self.config.clone());
                        State::Applied
                    }
                    Err(_) => State::Failed,
                }
            };
            self.states.insert(p.sid.clone(), state);
        }
        self.apply_cpu();
        let minutes = self.config.trim_minutes;
        if minutes > 0 && self.last_trim.elapsed() >= Duration::from_secs(minutes as u64 * 60) {
            self.trim_all(false);
        }
        let (used, total) = self.memory;
        if self.admin && self.config.clean_above > 0 && total > 0 && used * 100 / total >= self.config.clean_above as u64
            && self.last_clean.elapsed() >= Duration::from_secs(60) {
            self.clean(false);
        }
    }

    /// Priority, cores and Efficiency Mode for every client. Clients are taken oldest first (by PID), so each
    /// keeps its cores while others open and close.
    fn apply_cpu(&mut self) {
        let mut pids: Vec<u32> = self.clients.iter().map(|c| c.pid).collect();
        pids.sort();
        let c = &self.config;
        let masks = (c.cores_per_client > 0).then(|| core_masks(pids.len(), self.cores, c.cores_per_client as usize));
        let untouched = (Priority::Off, None, false);
        for (i, pid) in pids.iter().enumerate() {
            let want = (c.priority, masks.as_ref().map(|m| m[i]), c.efficiency);
            let had = self.cpu_set.get(pid).copied().unwrap_or(untouched);
            if want != had && system::set_cpu(*pid, &Cpu { priority: want.0, cores: want.1, efficiency: want.2 }) {
                self.cpu_set.insert(*pid, want);
            }
        }
        self.cpu_set.retain(|pid, _| pids.contains(pid));
    }

    fn trim_all(&mut self, say: bool) {
        self.last_trim = Instant::now();
        let before: u64 = self.clients.iter().map(|c| c.ram).sum();
        let n = self.clients.iter().filter(|c| system::trim(c.pid)).count();
        self.clients = system::clients();
        let after: u64 = self.clients.iter().map(|c| c.ram).sum();
        if say {
            self.note = format!("Trimmed {n} of {} Clients, Freed {}", self.clients.len(), gb(before.saturating_sub(after)));
        }
    }

    fn clean(&mut self, say: bool) {
        self.last_clean = Instant::now();
        let before = system::memory().0;
        let ok = system::clean_standby();
        self.trim_all(false);
        self.memory = system::memory();
        if say {
            self.note = if ok { format!("Cleaned Memory, Freed {}", gb(before.saturating_sub(self.memory.0))) } else { "Couldn't Clean Memory: Needs Admin".into() };
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        self.tick();
        ui.ctx().request_repaint_after(TICK);
        let before = self.config.clone();
        egui::CentralPanel::default().frame(Frame::new().fill(BG).inner_margin(Margin::same(18))).show(ui, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                self.header(ui);
                ui.add_space(14.0);
                self.clients_card(ui);
                ui.add_space(12.0);
                self.settings_card(ui);
                ui.add_space(12.0);
                self.cpu_card(ui);
                ui.add_space(12.0);
                self.memory_card(ui);
                if !self.note.is_empty() {
                    ui.add_space(10.0);
                    ui.label(RichText::new(&self.note).color(DIM));
                }
            });
        });
        if self.config != before {
            self.save_config();
            self.last_tick = None;   // apply right away
        }
    }
}

impl App {
    fn header(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            logo(ui, 3.0);
            ui.add_space(6.0);
            ui.vertical(|ui| {
                ui.label(RichText::new("Potato").size(26.0).strong().color(TEXT));
                ui.label(RichText::new("Roblox On Low: Every Client On This PC, RDP Included").color(DIM));
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if self.admin {
                    pill(ui, "Admin", GOOD);
                } else if ui.button("Restart As Admin").on_hover_text("Needed for other users' Roblox and for Clean Memory").clicked()
                    && system::relaunch_as_admin() {
                    std::process::exit(0);
                }
            });
        });
    }

    fn clients_card(&mut self, ui: &mut Ui) {
        card(ui, "Roblox Running", |ui| {
            let total: u64 = self.clients.iter().map(|c| c.ram).sum();
            ui.horizontal(|ui| {
                stat(ui, &self.clients.len().to_string(), "Clients");
                ui.add_space(24.0);
                stat(ui, &gb(total), "RAM Used");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add_enabled(!self.clients.is_empty(), egui::Button::new("Trim All")).clicked() {
                        self.trim_all(true);
                    }
                });
            });
            if self.clients.is_empty() {
                ui.label(RichText::new("No Roblox Open").color(DIM));
                return;
            }
            ui.add_space(6.0);
            let mut trim = None;
            egui::Grid::new("clients").num_columns(4).spacing([18.0, 6.0]).striped(false).show(ui, |ui| {
                for h in ["User", "Session", "RAM", ""] {
                    ui.label(RichText::new(h).small().color(DIM));
                }
                ui.end_row();
                for c in &self.clients {
                    ui.label(if c.user.is_empty() { "Unknown (Needs Admin)" } else { &c.user });
                    ui.label(c.session.to_string());
                    ui.label(gb(c.ram));
                    if ui.small_button("Trim").clicked() {
                        trim = Some(c.pid);
                    }
                    ui.end_row();
                }
            });
            if let Some(pid) = trim {
                self.note = if system::trim(pid) { "Trimmed".into() } else { "Couldn't Trim: Needs Admin".into() };
                self.last_tick = None;
            }
        });
    }

    fn settings_card(&mut self, ui: &mut Ui) {
        let c = &mut self.config;
        card(ui, "Game Settings", |ui| {
            ui.checkbox(&mut c.plan.macro_safe, "Macro Safe")
                .on_hover_text("Keeps the game looking how screen-reading macros like Natro Macro expect: no extra cuts, FPS 25 or more");
            hint(ui, "Only changes what Natro Macro asks for itself. Keep it on if you run a macro.");
            ui.add_space(6.0);
            ui.checkbox(&mut c.plan.lowest_graphics, "Lowest Graphics");
            ui.horizontal(|ui| {
                let mut capped = c.plan.fps_cap.is_some();
                if ui.checkbox(&mut capped, "Cap FPS").changed() {
                    c.plan.fps_cap = capped.then_some(30);
                }
                if let Some(fps) = c.plan.fps_cap.as_mut() {
                    let min = if c.plan.macro_safe { MACRO_SAFE_MIN_FPS } else { 5 };
                    *fps = (*fps).max(min);
                    ui.add(egui::Slider::new(fps, min..=240).suffix(" FPS"));
                }
            });
            ui.add_enabled_ui(!c.plan.macro_safe, |ui| {
                let mut extras = c.plan.extras && !c.plan.macro_safe;
                if ui.checkbox(&mut extras, "Extra Cuts").changed() {
                    c.plan.extras = extras;
                }
            });
            hint(ui, "Lowest textures, no grass, no anti-aliasing. Changes how the game looks, so macros can break.");
            ui.add_space(10.0);
            ui.label(RichText::new("Windows Users").small().color(DIM));
            if self.profiles.is_empty() {
                ui.label(RichText::new("No Windows user with Roblox found").color(DIM));
            }
            for p in &self.profiles {
                ui.horizontal(|ui| {
                    let mut on = c.users.contains(&p.sid);
                    if ui.checkbox(&mut on, &p.name).changed() {
                        if on { c.users.push(p.sid.clone()) } else { c.users.retain(|s| s != &p.sid) }
                    }
                    match self.states.get(&p.sid) {
                        Some(State::Applied) => pill(ui, "Applied", GOOD),
                        Some(State::Waiting) => pill(ui, "Applies When Roblox Closes", ACCENT),
                        Some(State::Failed) => pill(ui, "Couldn't Write: Needs Admin", WARN),
                        None => {}
                    }
                });
            }
            ui.add_space(8.0);
            if ui.button("Restore Originals").on_hover_text("Put back every file Potato changed, for the users ticked above").clicked() {
                let mut n = 0;
                for p in self.profiles.iter().filter(|p| c.users.contains(&p.sid)) {
                    n += files::restore(&files::all_files(&p.local)).unwrap_or(0);
                }
                c.users.clear();   // otherwise the next tick would apply everything again
                self.applied.clear();
                self.note = format!("Restored {n} Files. Tick a user to turn Potato back on.");
            }
        });
    }

    fn cpu_card(&mut self, ui: &mut Ui) {
        let cores = self.cores;
        let c = &mut self.config;
        card(ui, "CPU", |ui| {
            ui.horizontal(|ui| {
                ui.label("Priority");
                egui::ComboBox::from_id_salt("priority").selected_text(c.priority.label()).show_ui(ui, |ui| {
                    for p in Priority::ALL {
                        ui.selectable_value(&mut c.priority, p, p.label());
                    }
                });
            });
            ui.horizontal(|ui| {
                let mut on = c.cores_per_client > 0;
                if ui.checkbox(&mut on, "Give Each Client Its Own Cores").changed() {
                    c.cores_per_client = if on { 2.min(cores as u32) } else { 0 };
                }
                if c.cores_per_client > 0 {
                    ui.add(egui::DragValue::new(&mut c.cores_per_client).range(1..=cores as u32).suffix(" Cores"));
                }
            });
            ui.checkbox(&mut c.efficiency, "Efficiency Mode").on_hover_text("Windows 11 runs the clients on less power");
            hint(ui, &format!("{cores} cores on this PC. None of this changes how the game looks; Low priority and Efficiency Mode can lower FPS when the PC is busy."));
        });
    }

    fn memory_card(&mut self, ui: &mut Ui) {
        card(ui, "Memory", |ui| {
            let (used, total) = self.memory;
            let share = if total > 0 { used as f32 / total as f32 } else { 0.0 };
            ui.horizontal(|ui| {
                stat(ui, &format!("{} / {}", gb(used), gb(total)), "RAM In Use");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add_enabled(self.admin, egui::Button::new("Clean Memory")).on_disabled_hover_text("Needs admin").clicked() {
                        self.clean(true);
                    }
                });
            });
            ui.add(egui::ProgressBar::new(share).desired_height(6.0).fill(if share > 0.85 { WARN } else { ACCENT }));
            ui.add_space(8.0);
            let c = &mut self.config;
            ui.horizontal(|ui| {
                let mut on = c.trim_minutes > 0;
                if ui.checkbox(&mut on, "Trim Roblox Every").changed() {
                    c.trim_minutes = if on { 5 } else { 0 };
                }
                if c.trim_minutes > 0 {
                    ui.add(egui::DragValue::new(&mut c.trim_minutes).range(1..=120).suffix(" min"));
                }
            });
            ui.add_enabled_ui(self.admin, |ui| {
                ui.horizontal(|ui| {
                    let mut on = c.clean_above > 0;
                    if ui.checkbox(&mut on, "Clean Memory Above").changed() {
                        c.clean_above = if on { 85 } else { 0 };
                    }
                    if c.clean_above > 0 {
                        ui.add(egui::DragValue::new(&mut c.clean_above).range(50..=98).suffix("% Used"));
                    }
                });
            });
            hint(ui, "Trimming moves memory Roblox isn't using right now to the page file. It doesn't change the game.");
        });
    }
}

/// The pixel potato from assets/logo.txt, `px` points per pixel.
fn logo(ui: &mut Ui, px: f32) {
    let lines: Vec<&str> = include_str!("../assets/logo.txt").lines().filter(|l| !l.is_empty() && !l.starts_with('#')).collect();
    let colors: HashMap<char, Color32> = lines.iter().filter(|l| l.get(1..2) == Some("="))
        .filter_map(|l| Some((l.chars().next()?, Color32::from_hex(l.get(2..9)?).ok()?))).collect();
    let rows: Vec<&str> = lines.into_iter().filter(|l| l.get(1..2) != Some("=")).collect();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(rows[0].len() as f32 * px, rows.len() as f32 * px), egui::Sense::hover());
    for (y, row) in rows.iter().enumerate() {
        for (x, k) in row.chars().enumerate() {
            let Some(&color) = colors.get(&k) else { continue };
            let min = rect.min + egui::vec2(x as f32 * px, y as f32 * px);
            ui.painter().rect_filled(egui::Rect::from_min_size(min, egui::vec2(px, px)), 0.0, color);
        }
    }
}

fn gb(bytes: u64) -> String {
    let g = bytes as f64 / (1u64 << 30) as f64;
    if g >= 1.0 { format!("{g:.1} GB") } else { format!("{:.0} MB", bytes as f64 / (1u64 << 20) as f64) }
}

fn card(ui: &mut Ui, title: &str, body: impl FnOnce(&mut Ui)) {
    Frame::new().fill(CARD).stroke(Stroke::new(1.0, LINE)).corner_radius(CornerRadius::same(14)).inner_margin(Margin::same(16)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(RichText::new(title).size(15.0).strong().color(TEXT));
        ui.add_space(8.0);
        body(ui);
    });
}

fn stat(ui: &mut Ui, value: &str, label: &str) {
    ui.vertical(|ui| {
        ui.label(RichText::new(value).size(22.0).strong().color(TEXT));
        ui.label(RichText::new(label).small().color(DIM));
    });
}

fn pill(ui: &mut Ui, text: &str, color: Color32) {
    Frame::new().fill(color.gamma_multiply(0.15)).corner_radius(CornerRadius::same(99)).inner_margin(Margin::symmetric(9, 3)).show(ui, |ui| {
        ui.label(RichText::new(text).small().color(color));
    });
}

fn hint(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(text).small().color(DIM));
}

fn style(ctx: &egui::Context) {
    let mut v = egui::Visuals::dark();
    v.panel_fill = BG;
    v.window_fill = CARD;
    v.override_text_color = Some(TEXT);
    v.selection.bg_fill = ACCENT.gamma_multiply(0.8);
    v.selection.stroke = Stroke::new(1.0, BG);
    v.widgets.inactive.corner_radius = CornerRadius::same(8);
    v.widgets.hovered.corner_radius = CornerRadius::same(8);
    v.widgets.active.corner_radius = CornerRadius::same(8);
    v.widgets.inactive.weak_bg_fill = Color32::from_rgb(30, 30, 34);
    v.widgets.hovered.weak_bg_fill = Color32::from_rgb(42, 42, 48);
    v.widgets.hovered.bg_stroke = Stroke::new(1.0, ACCENT.gamma_multiply(0.6));
    ctx.set_visuals(v);
    ctx.all_styles_mut(|s| {
        s.spacing.item_spacing = egui::vec2(8.0, 6.0);
        s.spacing.button_padding = egui::vec2(12.0, 6.0);
    });
}
