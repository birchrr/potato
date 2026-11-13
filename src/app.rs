//! The window, and the loop that keeps every account's Roblox light.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use eframe::egui::{self, Color32, CornerRadius, FontFamily, Frame, Margin, RichText, Sense, Stroke, Ui};
use crate::config::{self, Account, Config, Preset, Priority};
use crate::files;
use crate::tweaks::core_masks;
use crate::version::Release;
use crate::rejoin;

use crate::helper::{self, Maximizer};
use crate::system::{self, Client, Cpu, Profile};

const TICK: Duration = Duration::from_secs(2);
const UPDATE_EVERY: Duration = Duration::from_secs(10 * 60);   // GitHub allows 60 unsigned-in checks an hour per PC
#[cfg(windows)]
const RELEASES: &str = "https://api.github.com/repos/birchrr/potato/releases/latest";

const BG: Color32 = Color32::from_rgb(12, 12, 13);
const PANEL: Color32 = Color32::from_rgb(19, 19, 21);
const RAISE: Color32 = Color32::from_rgb(26, 26, 29);
const LINE: Color32 = Color32::from_rgb(38, 38, 42);
const TEXT: Color32 = Color32::from_rgb(237, 237, 239);
const DIM: Color32 = Color32::from_rgb(161, 161, 170);
const FAINT: Color32 = Color32::from_rgb(113, 113, 122);
const ACCENT: Color32 = Color32::from_rgb(217, 163, 91);   // the logo's potato skin
const ON_ACCENT: Color32 = Color32::from_rgb(27, 18, 6);
const GOOD: Color32 = Color32::from_rgb(74, 222, 128);
const WAIT: Color32 = Color32::from_rgb(245, 176, 65);
const BAD: Color32 = Color32::from_rgb(248, 113, 113);

pub fn run() -> eframe::Result {
    system::enable_privileges();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_title("Potato")
            .with_icon(eframe::icon_data::from_png_bytes(include_bytes!("../assets/icon.png")).unwrap_or_default())
            .with_inner_size([1000.0, 800.0]).with_min_inner_size([860.0, 560.0]),
        ..Default::default()
    };
    eframe::run_native("Potato", options, Box::new(|cc| Ok(Box::new(App::new(&cc.egui_ctx)))))
}

/// Potato's settings file, readable by every Windows user (the helpers read it from their own sessions).
pub fn config_path() -> PathBuf {
    let base = std::env::var_os("ProgramData").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    base.join("Potato").join("settings.json")
}

/// Where an account's settings stand.
#[derive(Clone, Copy, PartialEq)]
enum State {
    Applied,
    /// their Roblox is open; Potato writes the moment it closes
    Waiting,
    Failed,
}

/// Where a newer release stands. Shared with the threads that check for it and install it.
#[derive(Clone, Default)]
#[cfg_attr(not(windows), allow(dead_code))]   // only Windows checks for updates
enum UpdateState {
    #[default]
    None,
    Available(Release),
    Installing(String),
    Failed(String),
}

type Update = Arc<Mutex<UpdateState>>;

pub struct App {
    config: Config,
    me: String,
    clients: Vec<Client>,
    profiles: Vec<Profile>,
    states: HashMap<String, State>,
    selected: Option<String>,
    memory: (u64, u64),
    freed: u64,
    /// what each client's CPU settings were last set to, so they're only set again when something changes
    cpu_set: HashMap<u32, (Priority, Option<u64>, bool)>,
    cores: usize,
    maximizer: Maximizer,
    helper_installed: Option<bool>,
    update: Update,
    update_dismissed: bool,
    last_update_check: Option<Instant>,
    last_tick: Option<Instant>,
    last_trim: Instant,
    last_clean: Instant,
    last_restart: Instant,
    /// Restart All Clients was clicked once and asks for a second click until then
    confirm_restart: Option<Instant>,
    note: String,
}

impl App {
    pub fn new(ctx: &egui::Context) -> App {
        style(ctx);
        let config = std::fs::read_to_string(config_path()).map(|t| Config::from_json(&t)).unwrap_or_default();
        let profiles = system::profiles();
        let me = system::current_sid();
        let mine = me.clone();
        std::thread::spawn(move || {
            let mut done = 0;
            loop {
                if !helper::running_here() {
                    helper::serve_rejoins(&mine, &mut done);   // a helper in this login would do it instead
                }
                std::thread::sleep(Duration::from_secs(1));
            }
        });
        App {
            config,
            me,
            clients: Vec::new(),
            selected: profiles.first().map(|p| p.sid.clone()),
            profiles,
            states: HashMap::new(),
            memory: (0, 0),
            freed: 0,
            cpu_set: HashMap::new(),
            cores: system::logical_cores(),
            maximizer: Maximizer::default(),
            helper_installed: None,
            update: Update::default(),
            update_dismissed: false,
            last_update_check: None,
            last_tick: None,
            last_trim: Instant::now(),
            last_clean: Instant::now(),
            last_restart: Instant::now(),
            confirm_restart: None,
            note: String::new(),
        }
    }

    fn save_config(&self) {
        let path = config_path();
        let _ = std::fs::create_dir_all(path.parent().unwrap());
        let _ = std::fs::write(path, self.config.to_json());
    }

    fn tick(&mut self) {
        if self.last_update_check.is_none_or(|t| t.elapsed() >= UPDATE_EVERY) {
            self.last_update_check = Some(Instant::now());
            check_for_update(self.update.clone());
        }
        if self.last_tick.is_some_and(|t| t.elapsed() < TICK) {
            return;
        }
        self.last_tick = Some(Instant::now());
        self.clients = system::clients();
        self.memory = system::memory();
        let running: HashSet<&str> = self.clients.iter().map(|c| c.sid.as_str()).collect();
        for p in &self.profiles {
            let a = self.config.account(&p.sid);
            if !a.on {
                self.states.remove(&p.sid);
                continue;
            }
            // Roblox rewrites its settings when it closes, so write while it's closed and only look while it runs
            let state = if running.contains(p.sid.as_str()) {
                match files::is_applied(&p.local, &a.plan) {
                    Ok(true) => State::Applied,
                    Ok(false) => State::Waiting,
                    Err(_) => State::Failed,
                }
            } else {
                match files::apply(&p.local, &a.plan) {
                    Ok(_) => State::Applied,
                    Err(_) => State::Failed,
                }
            };
            self.states.insert(p.sid.clone(), state);
        }
        self.close_extras();
        self.apply_cpu();
        self.keep_maximized();
        let hours = self.config.restart_hours;
        if hours > 0 && self.last_restart.elapsed() >= Duration::from_secs(hours as u64 * 3600) {
            self.restart_clients();
        }
        let minutes = self.config.trim_minutes;
        if minutes > 0 && self.last_trim.elapsed() >= Duration::from_secs(minutes as u64 * 60) {
            self.trim_all(false);
        }
        let (used, total) = self.memory;
        if self.config.clean_above > 0 && total > 0 && used * 100 / total >= self.config.clean_above as u64
            && self.last_clean.elapsed() >= Duration::from_secs(60) {
            self.clean(false);
        }
    }

    fn close_extras(&mut self) {
        let list: Vec<(&str, u32, u64)> = self.clients.iter().map(|c| (c.sid.as_str(), c.pid, c.started)).collect();
        let extra = config::extra_clients(&self.config, &list);
        if extra.is_empty() {
            return;
        }
        let n = extra.iter().filter(|pid| system::close(**pid)).count();
        self.note = format!("Closed {n} Extra {}", if n == 1 { "Client" } else { "Clients" });
        self.clients.retain(|c| !extra.contains(&c.pid));
    }

    /// Priority, cores and Efficiency Mode for every client. Clients are taken oldest first, so each keeps
    /// its cores while others open and close.
    fn apply_cpu(&mut self) {
        let mut list: Vec<&Client> = self.clients.iter().collect();
        list.sort_by_key(|c| (c.started, c.pid));
        let masks = (self.config.cores_per_client > 0).then(|| core_masks(list.len(), self.cores, self.config.cores_per_client as usize));
        let untouched = (Priority::Normal, None, false);
        for (i, c) in list.iter().enumerate() {
            let a = self.config.account(&c.sid);
            let want = if a.on { (a.priority, masks.as_ref().map(|m| m[i]), a.efficiency) } else { untouched };
            let had = self.cpu_set.get(&c.pid).copied().unwrap_or(untouched);
            if want != had && system::set_cpu(c.pid, &Cpu { priority: want.0, cores: want.1, efficiency: want.2 }) {
                self.cpu_set.insert(c.pid, want);
            }
        }
        let pids: HashSet<u32> = self.clients.iter().map(|c| c.pid).collect();
        self.cpu_set.retain(|pid, _| pids.contains(pid));
    }

    /// This session's windows here; every other session's through the helper, installed while any account
    /// wants it (accounts never changed use Main, which has it on).
    fn keep_maximized(&mut self) {
        let mine = self.config.account(&self.me);
        if mine.on && mine.plan.maximized {
            self.maximizer.tick();
        }
        let wanted = self.profiles.iter().map(|p| self.config.account(&p.sid)).any(|a| a.on && a.plan.maximized);
        if self.helper_installed != Some(wanted) {
            self.helper_installed = Some(wanted);
            if wanted {
                if !helper::install() {
                    self.note = "Couldn't Set Up Open Maximized For RDP Accounts".into();
                }
            } else {
                helper::uninstall();
            }
        }
    }

    fn trim_all(&mut self, say: bool) {
        self.last_trim = Instant::now();
        let before: u64 = self.clients.iter().map(|c| c.ram).sum();
        let n = self.clients.iter().filter(|c| system::trim(c.pid)).count();
        self.clients = system::clients();
        let freed = before.saturating_sub(self.clients.iter().map(|c| c.ram).sum());
        self.freed += freed;
        if say {
            self.note = format!("Trimmed {n} {}, Freed {}", if n == 1 { "Client" } else { "Clients" }, gb(freed));
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

    /// Close every client and reopen each in the server it was in. The reopening happens in each account's
    /// own login (helper.rs); clients whose server can't be read are left running.
    fn restart_clients(&mut self) {
        self.last_restart = Instant::now();
        let mut servers: HashMap<&str, Vec<rejoin::Server>> = HashMap::new();
        let (mut close, mut unknown) = (Vec::new(), 0);
        for c in &self.clients {
            let log = self.profiles.iter().find(|p| p.sid == c.sid).and_then(|p| files::client_log(&p.local, c.started));
            match rejoin::server(system::command_line(c.pid).as_deref(), log.as_deref()) {
                Some(s) if !c.sid.is_empty() => {
                    servers.entry(c.sid.as_str()).or_default().push(s);
                    close.push(c.pid);
                }
                _ => unknown += 1,
            }
        }
        let id = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64);
        let dir = helper::rejoin_dir();
        let _ = std::fs::create_dir_all(&dir);
        let mut written = Vec::new();
        for (sid, list) in &servers {
            if std::fs::write(dir.join(format!("{sid}.txt")), rejoin::request(id, list)).is_ok() {
                written.push(*sid);
            }
        }
        let closed = self.clients.iter().filter(|c| written.contains(&c.sid.as_str()) && close.contains(&c.pid))
            .filter(|c| system::close(c.pid)).count();
        self.note = format!("Restarting {closed} {}{}", if closed == 1 { "Client" } else { "Clients" },
            if unknown > 0 { format!(". Couldn't Find The Server For {unknown}, Left Those Open") } else { String::new() });
        self.last_tick = None;
    }

    fn restore(&mut self) {
        let mut n = 0;
        for p in &self.profiles {
            if self.config.account(&p.sid).on {
                n += files::restore(&files::all_files(&p.local)).unwrap_or(0);
                self.config.account_mut(&p.sid).on = false;   // otherwise the next tick would apply everything again
            }
        }
        self.note = format!("Restored {n} Files. Switch an account back on to use Potato again.");
    }
}

#[cfg(not(windows))]
fn check_for_update(_: Update) {}

#[cfg(not(windows))]
fn install_update(_: Release, _: Update) {}

/// GET a URL with curl (it ships with Windows 10 and 11), following GitHub's download redirects.
#[cfg(windows)]
fn fetch(url: &str, seconds: u32) -> Option<Vec<u8>> {
    use std::os::windows::process::CommandExt;
    let out = std::process::Command::new("curl").args(["-sfL", "-m", &seconds.to_string(), "-H", "User-Agent: Potato", url])
        .creation_flags(0x0800_0000).output().ok()?;
    out.status.success().then_some(out.stdout)
}

/// Ask GitHub for the latest release in the background.
#[cfg(windows)]
fn check_for_update(update: Update) {
    std::thread::spawn(move || {
        let Some(body) = fetch(RELEASES, 15) else { return };
        let Ok(v) = serde_json::from_slice::<serde_json::Value>(&body) else { return };
        let mut state = update.lock().unwrap();
        if let (UpdateState::None, Some(r)) = (&*state, crate::version::newer_release(&v, env!("CARGO_PKG_VERSION"))) {
            *state = UpdateState::Available(r);
        }
    });
}

/// Download the new Potato.exe, check it against the release's SHA-256, put it in place of this one and
/// restart into it. Windows lets a running exe be renamed but not overwritten, so this one becomes
/// Potato.old (deleted on the next start).
#[cfg(windows)]
fn install_update(r: Release, update: Update) {
    *update.lock().unwrap() = UpdateState::Installing(r.tag.clone());
    std::thread::spawn(move || {
        let fail = |msg: &str| *update.lock().unwrap() = UpdateState::Failed(msg.to_string());
        let (Some(exe), Some(sums)) = (fetch(&r.exe_url, 300), fetch(&r.sums_url, 30)) else { return fail("Couldn't download the update") };
        if !crate::version::matches_checksum(&exe, &String::from_utf8_lossy(&sums)) {
            return fail("The download didn't match its checksum, so it wasn't installed");
        }
        let Ok(me) = std::env::current_exe() else { return fail("Couldn't find Potato.exe") };
        let old = me.with_extension("old");
        let _ = std::fs::remove_file(&old);
        if std::fs::rename(&me, &old).is_err() {
            return fail("Couldn't replace Potato.exe");
        }
        if std::fs::write(&me, &exe).is_err() {
            let _ = std::fs::rename(&old, &me);   // put the working one back
            return fail("Couldn't replace Potato.exe");
        }
        if std::process::Command::new(&me).spawn().is_ok() {
            std::process::exit(0);
        }
        fail("Updated. Close Potato and open it again.");
    });
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        self.tick();
        ui.ctx().request_repaint_after(TICK);
        let before = self.config.clone();
        egui::CentralPanel::default().frame(Frame::new().fill(BG).inner_margin(Margin::same(20))).show(ui, |ui| {
            self.banner(ui);
            self.header(ui);
            ui.add_space(14.0);
            self.stats(ui);
            ui.add_space(14.0);
            let h = (ui.available_height() - 44.0).max(300.0);
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = 14.0;   // the gap between the columns; each column puts 8 back
                let right = 350.0;
                let left = ui.available_width() - right - 14.0;
                let down = egui::Layout::top_down(egui::Align::Min);
                let pc = 120.0;
                ui.allocate_ui_with_layout(egui::vec2(left, h), down, |ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    self.accounts(ui, h - pc - 14.0);
                    ui.add_space(14.0);
                    self.whole_pc(ui, pc);
                });
                ui.allocate_ui_with_layout(egui::vec2(right, h), down, |ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    self.settings(ui, h);
                });
            });
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new(&self.note).color(GOOD));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add(egui::Button::new(RichText::new("Restore Original Settings").color(DIM).underline()).frame(false)).clicked() {
                        self.restore();
                    }
                });
            });
        });
        if self.config != before {
            self.save_config();
            self.last_tick = None;   // apply right away
        }
    }
}

impl App {
    fn banner(&mut self, ui: &mut Ui) {
        let state = self.update.lock().unwrap().clone();
        let (title, note) = match &state {
            UpdateState::None => return,
            UpdateState::Available(_) if self.update_dismissed => return,
            UpdateState::Available(r) => (format!("Potato {} Is Out", r.tag), "Updates in a few seconds and restarts by itself.".to_string()),
            UpdateState::Installing(tag) => (format!("Installing Potato {tag}"), "Downloading and checking it. Potato restarts when it's done.".to_string()),
            UpdateState::Failed(msg) => ("Update Didn't Install".to_string(), msg.clone()),
        };
        Frame::new().fill(ACCENT.gamma_multiply(0.14)).stroke(Stroke::new(1.0, ACCENT.gamma_multiply(0.5)))
            .corner_radius(CornerRadius::same(10)).inner_margin(Margin::symmetric(14, 10)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(semi(&title, 13.5).color(TEXT));
                ui.label(RichText::new(note).color(DIM));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| match state {
                    UpdateState::Available(r) => {
                        if ui.add(egui::Button::new(RichText::new("Later").color(DIM)).frame(false)).clicked() {
                            self.update_dismissed = true;
                        }
                        if primary(ui, "Update Now").clicked() {
                            install_update(r, self.update.clone());
                        }
                    }
                    UpdateState::Installing(_) => { ui.spinner(); }
                    UpdateState::Failed(_) => {
                        if ui.add(egui::Button::new(RichText::new("Close").color(DIM)).frame(false)).clicked() {
                            *self.update.lock().unwrap() = UpdateState::None;
                        }
                    }
                    UpdateState::None => {}
                });
            });
        });
        ui.add_space(12.0);
    }

    fn header(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            logo(ui, 2.6);
            ui.add_space(4.0);
            ui.label(semi("Potato", 22.0).color(TEXT));
            ui.add_space(6.0);
            chip(ui, "Running", GOOD);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if primary(ui, "Trim All Roblox").clicked() {
                    self.trim_all(true);
                }
                let armed = self.confirm_restart.is_some_and(|t| t.elapsed() < Duration::from_secs(4));
                let label = if armed { "Click Again To Restart" } else { "Restart All Clients" };
                if ui.add(egui::Button::new(label).min_size(egui::vec2(0.0, 32.0)))
                    .on_hover_text("Closes every Roblox client and opens each one again in the same server").clicked() {
                    if armed {
                        self.confirm_restart = None;
                        self.restart_clients();
                    } else {
                        self.confirm_restart = Some(Instant::now());
                    }
                }
                if armed {
                    ui.ctx().request_repaint_after(Duration::from_millis(250));
                }
                if ui.add(egui::Button::new("Clean Memory").min_size(egui::vec2(0.0, 32.0))).clicked() {
                    self.clean(true);
                }
            });
        });
    }

    fn stats(&mut self, ui: &mut Ui) {
        let (used, total) = self.memory;
        let roblox: u64 = self.clients.iter().map(|c| c.ram).sum();
        panel(ui, |ui| {
            ui.columns(4, |cols| {
                stat(&mut cols[0], "Roblox Open", &self.clients.len().to_string(), if self.clients.len() == 1 { "client" } else { "clients" });
                stat(&mut cols[1], "Roblox RAM", &gb(roblox), "");
                stat(&mut cols[2], "PC RAM", &format!("{:.1}", used as f64 / GIB), &format!("/ {}", gb(total)));
                let share = if total > 0 { used as f32 / total as f32 } else { 0.0 };
                cols[2].add(egui::ProgressBar::new(share).desired_height(4.0).fill(if share > 0.85 { BAD } else { ACCENT }));
                stat(&mut cols[3], "Freed This Session", &gb(self.freed), "");
            });
        });
    }

    fn accounts(&mut self, ui: &mut Ui, h: f32) {
        let console = system::console_session();
        card(ui, h, "Accounts", "Click one to change its settings", |ui| {
            if self.profiles.is_empty() {
                ui.label(RichText::new("No Windows account with Roblox found").color(DIM));
                return;
            }
            let widths = [44.0, 0.0, 86.0, 80.0, 84.0, 110.0];
            row(ui, "head", &widths, false, |ui, col| {
                let t = ["", "Account", "Roblox", "RAM", "Settings", "Status"][col];
                ui.label(RichText::new(t).size(12.0).color(FAINT));
            });
            egui::ScrollArea::vertical().id_salt("accounts").show(ui, |ui| {
                for p in self.profiles.clone() {
                    let mine: Vec<&Client> = self.clients.iter().filter(|c| c.sid == p.sid).collect();
                    let ram: u64 = mine.iter().map(|c| c.ram).sum();
                    let a = self.config.account(&p.sid);
                    let rdp = mine.first().map(|c| c.session != console).unwrap_or(p.sid != self.me);
                    let picked = self.selected.as_deref() == Some(p.sid.as_str());
                    let state = self.states.get(&p.sid).copied();
                    let mut flip = None;
                    let resp = row(ui, &p.sid, &widths, picked, |ui, col| match col {
                        0 => {
                            let mut on = a.on;
                            if toggle(ui, &mut on).changed() {
                                flip = Some(on);
                            }
                        }
                        1 => {
                            ui.label(semi(&p.name, 13.5).color(TEXT));
                            tag(ui, if rdp { "RDP" } else { "This PC" });
                        }
                        2 => {
                            let t = match mine.len() { 0 => "Closed".to_string(), 1 => "Running".to_string(), n => format!("{n} Running") };
                            ui.label(RichText::new(t).color(if mine.is_empty() { FAINT } else { TEXT }));
                        }
                        3 => {
                            ui.label(RichText::new(if mine.is_empty() { "-".into() } else { gb(ram) }).color(if mine.is_empty() { FAINT } else { TEXT }));
                        }
                        4 => {
                            ui.label(RichText::new(if a.on { Preset::of(&a).map_or("Custom", |p| p.label()) } else { "Off" }).color(DIM));
                        }
                        _ => match state {
                            Some(State::Applied) => status(ui, GOOD, "Applied"),
                            Some(State::Waiting) => status(ui, WAIT, "Next Restart"),
                            Some(State::Failed) => status(ui, BAD, "Needs Admin"),
                            None => status(ui, FAINT, "Off"),
                        },
                    });
                    if let Some(on) = flip {
                        self.config.account_mut(&p.sid).on = on;
                    } else if resp.clicked() {
                        self.selected = Some(p.sid.clone());
                    }
                }
            });
        });
    }

    fn settings(&mut self, ui: &mut Ui, h: f32) {
        let Some(sid) = self.selected.clone() else {
            card(ui, h, "Settings", "", |ui| { ui.label(RichText::new("Pick an account").color(DIM)); });
            return;
        };
        let name = self.profiles.iter().find(|p| p.sid == sid).map_or("Account".to_string(), |p| p.name.clone());
        let sids: Vec<String> = self.profiles.iter().map(|p| p.sid.clone()).collect();
        let config = &mut self.config;
        card(ui, h, &format!("{name}'s Settings"), "", |ui| {
            egui::ScrollArea::vertical().id_salt("settings").show(ui, |ui| {
                let mut a = config.account(&sid);
                let mut preset = Preset::of(&a);
                if segmented(ui, &mut preset, &Preset::ALL.map(|p| (Some(p), p.label())))
                    && let Some(p) = preset {
                    a = Account { on: a.on, max_clients: a.max_clients, ..p.account() };
                }
                section(ui, "Game");
                setting(ui, "Lowest Graphics", "", |ui, id| { toggle(ui, &mut a.plan.lowest_graphics).labelled_by(id); });
                setting(ui, "Open Maximized", "Fills the screen, keeps the title bar", |ui, id| { toggle(ui, &mut a.plan.maximized).labelled_by(id); });
                const CUSTOM: Option<u32> = Some(0);
                let presets = [None, Some(15), Some(20), Some(25), Some(30), Some(40), Some(60), Some(120)];
                let custom = !presets.contains(&a.plan.fps_cap);
                setting(ui, "FPS Cap", "", |ui, id| {
                    let mut options: Vec<(Option<u32>, String)> = presets.iter().map(|f| (*f, f.map_or("Off".into(), |f| format!("{f} FPS")))).collect();
                    options.push((CUSTOM, "Custom".into()));
                    let mut pick = if custom { CUSTOM } else { a.plan.fps_cap };
                    if dropdown(ui, "fps", id, &mut pick, &options) {
                        a.plan.fps_cap = if pick == CUSTOM { Some(45) } else { pick };
                    }
                });
                if let Some(f) = a.plan.fps_cap.as_mut().filter(|_| custom) {
                    setting(ui, "Custom FPS", "", |ui, id| {
                        ui.add(egui::Slider::new(f, 5..=240).suffix(" FPS")).labelled_by(id);
                    });
                }
                setting(ui, "Ultra Low", "Lowest textures, no grass, no anti-aliasing", |ui, id| { toggle(ui, &mut a.plan.ultra_low).labelled_by(id); });
                section(ui, "CPU");
                setting(ui, "Priority", "", |ui, _| { segmented(ui, &mut a.priority, &Priority::ALL.map(|p| (p, p.label()))); });
                setting(ui, "Efficiency Mode", "Windows 11 runs it on less power", |ui, id| { toggle(ui, &mut a.efficiency).labelled_by(id); });
                section(ui, "Clients");
                setting(ui, "Close Extra Clients", "Closes older stuck ones", |ui, id| {
                    let options: Vec<(u32, String)> = [0, 1, 2, 3, 5].iter().map(|n| (*n, if *n == 0 { "Off".into() } else { format!("Keep {n}") })).collect();
                    dropdown(ui, "extras", id, &mut a.max_clients, &options);
                });
                ui.add_space(6.0);
                if sids.len() > 1 && ui.button("Use These For Every Account").clicked() {
                    for s in &sids {
                        *config.account_mut(s) = a.clone();
                    }
                }
                if a != config.account(&sid) {
                    *config.account_mut(&sid) = a;
                }
            });
        });
    }

    /// Settings that are the same for every account, side by side under the accounts table.
    fn whole_pc(&mut self, ui: &mut Ui, h: f32) {
        let cores = self.cores as u32;
        let c = &mut self.config;
        let off_or = |n: u32, f: &dyn Fn(u32) -> String| if n == 0 { "Off".to_string() } else { f(n) };
        card(ui, h, "Whole PC", "The same for every account", |ui| {
            ui.columns(4, |cols| {
                let list = [0, 1, 2, 3, 4, 6, 8].into_iter().filter(|n| *n <= cores).map(|n| (n, off_or(n, &|n| format!("{n} Each")))).collect::<Vec<_>>();
                stacked(&mut cols[0], "Own CPU Cores", &format!("{cores} on this PC"), |ui, id| { dropdown(ui, "cores", id, &mut c.cores_per_client, &list); });
                let list = [0, 1, 2, 5, 10, 15, 30].map(|n| (n, off_or(n, &|n| format!("Every {n} Min"))));
                stacked(&mut cols[1], "Trim RAM", "Frees unused memory", |ui, id| { dropdown(ui, "trim", id, &mut c.trim_minutes, &list); });
                let list = [0, 70, 75, 80, 85, 90].map(|n| (n, off_or(n, &|n| format!("Above {n}%"))));
                stacked(&mut cols[2], "Clean Memory", "When the PC is this full", |ui, id| { dropdown(ui, "clean", id, &mut c.clean_above, &list); });
                let list = [0, 1, 2, 4, 6, 12, 24].map(|n| (n, off_or(n, &|n| format!("Every {n} {}", if n == 1 { "Hour" } else { "Hours" }))));
                stacked(&mut cols[3], "Restart Clients", "Same server, fresh client", |ui, id| { dropdown(ui, "restart", id, &mut c.restart_hours, &list); });
            });
        });
    }
}

const GIB: f64 = (1u64 << 30) as f64;

fn gb(bytes: u64) -> String {
    let g = bytes as f64 / GIB;
    if bytes >= 1000 << 20 { format!("{g:.1} GB") } else { format!("{:.0} MB", bytes as f64 / (1u64 << 20) as f64) }
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

fn panel(ui: &mut Ui, body: impl FnOnce(&mut Ui)) {
    Frame::new().fill(PANEL).stroke(Stroke::new(1.0, LINE)).corner_radius(CornerRadius::same(12)).inner_margin(Margin::symmetric(16, 12))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            body(ui);
        });
}

fn card(ui: &mut Ui, h: f32, title: &str, note: &str, body: impl FnOnce(&mut Ui)) {
    Frame::new().fill(PANEL).stroke(Stroke::new(1.0, LINE)).corner_radius(CornerRadius::same(12)).inner_margin(Margin::same(16))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.set_height(h - 34.0);
            ui.horizontal(|ui| {
                ui.label(semi(title, 14.0).color(TEXT));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| ui.label(RichText::new(note).size(12.0).color(FAINT)));
            });
            ui.add_space(8.0);
            body(ui);
        });
}

/// One table row with fixed column widths (0 = take what's left). The whole row is clickable; its click area
/// is registered before the cells (from last frame's size), so switches inside the row still get their clicks.
fn row(ui: &mut Ui, key: &str, widths: &[f32], picked: bool, mut cell: impl FnMut(&mut Ui, usize)) -> egui::Response {
    let id = ui.scope_id().with(("row", key));
    let last = ui.ctx().data(|d| d.get_temp::<egui::Rect>(id));
    let resp = last.map(|r| ui.interact(r, id, Sense::click()));
    let fixed: f32 = widths.iter().sum();
    // the name column takes the spare width, up to a point: on a wide window the columns stay together
    let flex = (ui.available_width() - fixed - 16.0 - 8.0 * widths.len() as f32).clamp(80.0, 280.0);
    let out = Frame::new().fill(if picked { RAISE } else { Color32::TRANSPARENT }).corner_radius(CornerRadius::same(8))
        .inner_margin(Margin::symmetric(8, 8)).show(ui, |ui| {
            ui.horizontal(|ui| {
                for (i, w) in widths.iter().enumerate() {
                    let w = if *w == 0.0 { flex } else { *w };
                    ui.allocate_ui_with_layout(egui::vec2(w, 22.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                        ui.set_min_width(w);
                        cell(ui, i);
                    });
                }
            });
        });
    let rect = out.response.rect;
    ui.ctx().data_mut(|d| d.insert_temp(id, rect));
    let resp = resp.unwrap_or(out.response);
    if resp.hovered() && !picked {
        ui.painter().rect_filled(rect, 8.0, Color32::from_white_alpha(4));
    }
    resp
}

fn stat(ui: &mut Ui, label: &str, value: &str, unit: &str) {
    ui.label(RichText::new(label).size(12.0).color(FAINT));
    ui.horizontal(|ui| {
        ui.label(semi(value, 20.0).color(TEXT));
        if !unit.is_empty() {
            ui.label(RichText::new(unit).size(13.5).color(FAINT));
        }
    });
}

fn section(ui: &mut Ui, title: &str) {
    ui.add_space(10.0);
    ui.label(RichText::new(title.to_uppercase()).size(11.0).color(FAINT).extra_letter_spacing(1.0));
    ui.add_space(2.0);
}

/// A label on the left (with an optional note under it) and its control on the right. The control gets the
/// label's id for `labelled_by`, so screen readers name it.
fn setting(ui: &mut Ui, label: &str, note: &str, control: impl FnOnce(&mut Ui, egui::Id)) {
    ui.horizontal(|ui| {
        let id = ui.vertical(|ui| {
            let id = ui.label(RichText::new(label).size(13.5).color(TEXT)).id;
            if !note.is_empty() {
                ui.label(RichText::new(note).size(11.5).color(FAINT));
            }
            id
        }).inner;
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| control(ui, id));
    });
    ui.add_space(4.0);
}

fn toggle(ui: &mut Ui, on: &mut bool) -> egui::Response {
    let (rect, mut resp) = ui.allocate_exact_size(egui::vec2(32.0, 18.0), Sense::click());
    if resp.clicked() {
        *on = !*on;
        resp.mark_changed();
    }
    let t = ui.ctx().animate_bool_responsive(resp.id, *on);
    let r = rect.height() / 2.0;
    ui.painter().rect_filled(rect, r, if *on { ACCENT } else { Color32::from_rgb(58, 58, 64) });
    let x = egui::lerp((rect.left() + r)..=(rect.right() - r), t);
    ui.painter().circle_filled(egui::pos2(x, rect.center().y), r - 2.5, if *on { ON_ACCENT } else { DIM });
    resp
}

/// A label with a note under it, then its control: for narrow columns.
fn stacked(ui: &mut Ui, label: &str, note: &str, control: impl FnOnce(&mut Ui, egui::Id)) {
    let id = ui.label(RichText::new(label).size(13.5).color(TEXT)).id;
    ui.label(RichText::new(note).size(11.5).color(FAINT));
    ui.add_space(2.0);
    control(ui, id);
}

/// A dropdown of fixed choices. Returns true when the choice changed.
fn dropdown<T: PartialEq + Copy>(ui: &mut Ui, salt: &str, label: egui::Id, value: &mut T, options: &[(T, String)]) -> bool {
    let shown = options.iter().find(|(v, _)| v == value).map_or("Custom", |(_, l)| l.as_str()).to_string();
    let mut changed = false;
    let r = egui::ComboBox::from_id_salt(salt).selected_text(shown).width(124.0).show_ui(ui, |ui| {
        for (v, l) in options {
            changed |= ui.selectable_value(value, *v, l).changed();
        }
    });
    r.response.labelled_by(label);
    changed
}

/// Buttons side by side, the chosen one filled. Returns true when the choice changed.
fn segmented<T: PartialEq + Copy>(ui: &mut Ui, value: &mut T, options: &[(T, &str)]) -> bool {
    let mut changed = false;
    Frame::new().fill(RAISE).stroke(Stroke::new(1.0, LINE)).corner_radius(CornerRadius::same(8)).inner_margin(Margin::same(2)).show(ui, |ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        ui.horizontal(|ui| {
            let rtl = ui.layout().main_dir() == egui::Direction::RightToLeft;   // inside a right-aligned row
            let order: Vec<&(T, &str)> = if rtl { options.iter().rev().collect() } else { options.iter().collect() };
            for (v, label) in order {
                let on = *value == *v;
                let b = egui::Button::new(RichText::new(*label).size(12.5).color(if on { ON_ACCENT } else { DIM }))
                    .fill(if on { ACCENT } else { Color32::TRANSPARENT }).stroke(Stroke::NONE).corner_radius(CornerRadius::same(6))
                    .min_size(egui::vec2(0.0, 24.0));
                if ui.add(b).clicked() && !on {
                    *value = *v;
                    changed = true;
                }
            }
        });
    });
    changed
}

fn primary(ui: &mut Ui, text: &str) -> egui::Response {
    ui.add(egui::Button::new(semi(text, 13.5).color(ON_ACCENT)).fill(ACCENT).stroke(Stroke::NONE).min_size(egui::vec2(0.0, 32.0)))
}

fn chip(ui: &mut Ui, text: &str, dot: Color32) {
    Frame::new().fill(RAISE).corner_radius(CornerRadius::same(99)).inner_margin(Margin::symmetric(9, 3)).show(ui, |ui| {
        status(ui, dot, text);
    });
}

fn tag(ui: &mut Ui, text: &str) {
    Frame::new().fill(RAISE).corner_radius(CornerRadius::same(5)).inner_margin(Margin::symmetric(6, 1)).show(ui, |ui| {
        ui.label(RichText::new(text).size(11.5).color(DIM));
    });
}

fn status(ui: &mut Ui, color: Color32, text: &str) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(8.0, 8.0), Sense::hover());
    ui.painter().circle_filled(rect.center(), 3.5, color);
    ui.label(RichText::new(text).color(DIM));
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
    v.window_fill = PANEL;
    v.override_text_color = Some(TEXT);
    v.selection.bg_fill = ACCENT.gamma_multiply(0.8);
    v.selection.stroke = Stroke::new(1.0, BG);
    for w in [&mut v.widgets.inactive, &mut v.widgets.hovered, &mut v.widgets.active] {
        w.corner_radius = CornerRadius::same(8);
    }
    v.widgets.inactive.weak_bg_fill = RAISE;
    v.widgets.inactive.bg_stroke = Stroke::new(1.0, LINE);
    v.widgets.hovered.weak_bg_fill = Color32::from_rgb(34, 34, 38);
    v.widgets.hovered.bg_stroke = Stroke::new(1.0, ACCENT.gamma_multiply(0.6));
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, LINE);
    ctx.set_visuals(v);
    ctx.all_styles_mut(|s| {
        s.spacing.item_spacing = egui::vec2(8.0, 6.0);
        s.spacing.button_padding = egui::vec2(12.0, 6.0);
        s.text_styles.insert(egui::TextStyle::Body, egui::FontId::proportional(13.5));
        s.text_styles.insert(egui::TextStyle::Button, egui::FontId::proportional(13.5));
    });
}
