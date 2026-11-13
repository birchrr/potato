//! Everything Potato does, on its own thread, so it keeps working while the window is in the tray. The window
//! (app.rs) only shows this state and changes the settings.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::config::{self, Config, Priority};
use crate::files;
use crate::helper::{self, Maximizer};
use crate::rejoin;
use crate::system::{self, Client, Cpu, Profile};
use crate::tweaks::core_masks;
use crate::version::Release;

const TICK: Duration = Duration::from_secs(2);
const UPDATE_EVERY: Duration = Duration::from_secs(10 * 60);   // GitHub allows 60 signed-out checks an hour per PC
#[cfg(windows)]
const RELEASES: &str = "https://api.github.com/repos/birchrr/potato/releases/latest";

/// Potato's settings file, readable by every Windows user (the helpers read it from their own sessions).
pub fn config_path() -> PathBuf {
    let base = std::env::var_os("ProgramData").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    base.join("Potato").join("settings.json")
}

/// Where an account's settings stand.
#[derive(Clone, Copy, PartialEq)]
pub enum State {
    Applied,
    /// their Roblox is open; Potato writes the moment it closes
    Waiting,
    Failed,
}

/// Where a newer release stands. Shared with the threads that check for it and install it.
#[derive(Clone, Default)]
#[cfg_attr(not(windows), allow(dead_code))]   // only Windows checks for updates
pub enum UpdateState {
    #[default]
    None,
    Available(Release),
    Installing(String),
    Failed(String),
}

pub type Update = Arc<Mutex<UpdateState>>;

pub struct Engine {
    pub config: Config,
    pub me: String,
    pub clients: Vec<Client>,
    pub profiles: Vec<Profile>,
    pub states: HashMap<String, State>,
    pub memory: (u64, u64),
    pub freed: u64,
    pub cores: usize,
    pub update: Update,
    pub note: String,
    /// each client's CPU use since the last look, in percent of the whole PC
    pub cpu_pct: HashMap<u32, f32>,
    /// restarts and crash rejoins per account, for the report
    pub restarts: HashMap<String, u32>,
    /// what each client's CPU settings were last set to, so they're only set again when something changes
    cpu_set: HashMap<u32, (Priority, Option<u64>, bool)>,
    cpu_seen: HashMap<u32, (u64, Instant)>,
    /// each capped client's job and the cap it has
    jobs: HashMap<u32, (u32, isize)>,
    /// clients Potato closed itself, so they don't count as crashes
    closed_by_us: HashSet<u32>,
    /// the last clients seen: PID -> account
    last_clients: HashMap<u32, String>,
    /// where each client is, for accounts that rejoin after a crash: PID -> (server, when it was read)
    whereabouts: HashMap<u32, (rejoin::Server, Instant)>,
    /// crashed clients waiting a moment in case something else reopens them: (account, server, when)
    crashed: Vec<(String, rejoin::Server, Instant)>,
    upload_applied: Option<u32>,
    shutdown_done: Option<u32>,
    last_report: Instant,
    /// a report waiting for the helpers' screenshots: (request id, when asked)
    report_pending: Option<(u64, Instant)>,
    reported_freezes: HashMap<String, u32>,
    maximizer: Maximizer,
    helper_installed: Option<bool>,
    last_update_check: Option<Instant>,
    last_tick: Option<Instant>,
    last_trim: Instant,
    last_clean: Instant,
    last_restart: Instant,
}

impl Default for Engine {
    fn default() -> Self {
        Self::new()
    }
}

impl Engine {
    pub fn new() -> Engine {
        Engine {
            config: std::fs::read_to_string(config_path()).map(|t| Config::from_json(&t)).unwrap_or_default(),
            me: system::current_sid(),
            clients: Vec::new(),
            profiles: system::profiles(),
            states: HashMap::new(),
            memory: (0, 0),
            freed: 0,
            cores: system::logical_cores(),
            update: Update::default(),
            note: String::new(),
            cpu_pct: HashMap::new(),
            restarts: HashMap::new(),
            cpu_set: HashMap::new(),
            cpu_seen: HashMap::new(),
            jobs: HashMap::new(),
            closed_by_us: HashSet::new(),
            last_clients: HashMap::new(),
            whereabouts: HashMap::new(),
            crashed: Vec::new(),
            upload_applied: None,
            shutdown_done: None,
            last_report: Instant::now(),
            report_pending: None,
            reported_freezes: HashMap::new(),
            maximizer: Maximizer::default(),
            helper_installed: None,
            last_update_check: None,
            last_tick: None,
            last_trim: Instant::now(),
            last_clean: Instant::now(),
            last_restart: Instant::now(),
        }
    }

    /// Start the engine on its own thread, plus the thread that reopens this login's clients after a restart
    /// (a helper running in this login would do that instead). Returns the shared engine for the window.
    pub fn start(on_change: impl Fn() + Send + 'static) -> Arc<Mutex<Engine>> {
        let engine = Arc::new(Mutex::new(Engine::new()));
        let e = engine.clone();
        std::thread::spawn(move || loop {
            e.lock().unwrap().tick();
            on_change();
            std::thread::sleep(Duration::from_millis(500));
        });
        let me = engine.lock().unwrap().me.clone();
        std::thread::spawn(move || {
            let mut jobs = helper::SessionJobs::default();
            loop {
                if !helper::running_here() {
                    helper::session_tick(&me, &mut jobs);   // a helper in this login would do it instead
                }
                std::thread::sleep(Duration::from_secs(1));
            }
        });
        engine
    }

    /// Save the settings and act on them right away.
    pub fn changed(&mut self) {
        let path = config_path();
        let _ = std::fs::create_dir_all(path.parent().unwrap());
        let _ = std::fs::write(path, self.config.to_json());
        self.last_tick = None;
    }

    pub fn tick(&mut self) {
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
        self.measure_cpu();
        self.watch_crashes();
        self.close_extras();
        self.restart_hogs();
        self.apply_cpu();
        self.keep_maximized();
        self.apply_upload_cap();
        self.shutdown_on_time();
        self.report();
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
        self.closed_by_us.extend(extra.iter().copied());
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
        // hard CPU caps: a job per capped client; a lifted cap goes back to 100%
        for c in &self.clients {
            let a = self.config.account(&c.sid);
            let want = if a.on { a.cpu_limit } else { 0 };
            let had = self.jobs.get(&c.pid).map_or(0, |j| j.0);
            if want == had || (want == 0 && had == 100) {
                continue;
            }
            let mut job = self.jobs.get(&c.pid).map(|j| j.1);
            if system::cpu_cap(c.pid, if want == 0 { 100 } else { want }, &mut job)
                && let Some(j) = job {
                self.jobs.insert(c.pid, (if want == 0 { 100 } else { want }, j));
            }
        }
        for (_, j) in self.jobs.extract_if(|pid, _| !pids.contains(pid)) {
            system::drop_job(j.1);
        }
    }

    fn measure_cpu(&mut self) {
        let now = Instant::now();
        let cores = self.cores.max(1) as f32;
        self.cpu_pct.clear();
        for c in &self.clients {
            if let Some((t, at)) = self.cpu_seen.get(&c.pid) {
                let wall = now.duration_since(*at).as_nanos() as f32 / 100.0;   // in 100 ns, like cpu_time
                if wall > 0.0 {
                    self.cpu_pct.insert(c.pid, (c.cpu_time.saturating_sub(*t) as f32 / wall / cores * 100.0).min(100.0));
                }
            }
        }
        self.cpu_seen = self.clients.iter().map(|c| (c.pid, (c.cpu_time, now))).collect();
    }

    /// Notice clients that closed without Potato closing them. For accounts with Rejoin After A Crash, wait
    /// 20 seconds in case something else (like a macro) reopens them, then send them back to their server.
    fn watch_crashes(&mut self) {
        let now: HashMap<u32, String> = self.clients.iter().map(|c| (c.pid, c.sid.clone())).collect();
        for (pid, sid) in &self.last_clients {
            if now.contains_key(pid) || self.closed_by_us.remove(pid) {
                continue;
            }
            if let Some((server, _)) = self.whereabouts.remove(pid)
                && self.config.account(sid).on && self.config.account(sid).rejoin_crashed {
                self.crashed.push((sid.clone(), server, Instant::now()));
            }
        }
        self.last_clients = now;
        // keep each watched client's server fresh (its log is re-read every 30 s, so teleports are followed)
        for c in &self.clients {
            let a = self.config.account(&c.sid);
            let stale = self.whereabouts.get(&c.pid).is_none_or(|(_, at)| at.elapsed() > Duration::from_secs(30));
            if a.on && a.rejoin_crashed && stale {
                let log = self.profiles.iter().find(|p| p.sid == c.sid).and_then(|p| files::client_log(&p.local, c.started));
                if let Some(s) = rejoin::server(system::command_line(c.pid).as_deref(), log.as_deref()) {
                    self.whereabouts.insert(c.pid, (s, Instant::now()));
                }
            }
        }
        let pids: HashSet<u32> = self.clients.iter().map(|c| c.pid).collect();
        self.whereabouts.retain(|pid, _| pids.contains(pid));
        let mut due = Vec::new();
        self.crashed.retain(|(sid, server, at)| {
            if self.clients.iter().any(|c| &c.sid == sid && c.started >= system_time_now().saturating_sub(at.elapsed().as_nanos() as u64 / 100)) {
                return false;   // a new client opened for the account: something else rejoined it
            }
            if at.elapsed() < Duration::from_secs(20) {
                return true;
            }
            due.push((sid.clone(), server.clone()));
            false
        });
        for (sid, server) in due {
            if self.send_rejoins(&[(sid.as_str(), vec![server])]) > 0 {
                *self.restarts.entry(sid).or_default() += 1;
                self.note = "Rejoined A Crashed Client".into();
            }
        }
    }

    /// Restart clients that grew past their account's memory limit.
    fn restart_hogs(&mut self) {
        let hogs: HashSet<u32> = self.clients.iter().filter(|c| {
            let a = self.config.account(&c.sid);
            a.on && a.ram_limit_mb > 0 && c.ram > a.ram_limit_mb as u64 * (1 << 20)
        }).map(|c| c.pid).collect();
        if !hogs.is_empty() {
            self.restart(|c| hogs.contains(&c.pid));
        }
    }

    /// Roblox's upload limit, through a Windows QoS policy (applied again whenever Potato starts).
    fn apply_upload_cap(&mut self) {
        let want = self.config.upload_kbps;
        if self.upload_applied == Some(want) {
            return;
        }
        self.upload_applied = Some(want);
        let mut script = "Remove-NetQosPolicy -Name Potato -PolicyStore ActiveStore -Confirm:$false -ErrorAction SilentlyContinue".to_string();
        if want > 0 {
            script += &format!("; New-NetQosPolicy -Name Potato -PolicyStore ActiveStore -AppPathNameMatchCondition RobloxPlayerBeta.exe -ThrottleRateActionBitsPerSecond {}", want as u64 * 1000);
        }
        if !system::run("powershell", &["-NoProfile", "-NonInteractive", "-Command", &script]) && want > 0 {
            self.note = "Couldn't Set The Upload Cap".into();
        }
    }

    fn shutdown_on_time(&mut self) {
        let Some(at) = config::parse_time(&self.config.shutdown_at) else { return };
        let now = system::local_minutes();
        if now != at {
            self.shutdown_done = None;
        } else if self.shutdown_done.is_none() {
            self.shutdown_done = Some(at);
            system::run("shutdown", &["/s", "/t", "60", "/c", "Potato is turning the PC off in a minute (shutdown /a cancels)"]);
            self.note = "Turning The PC Off In A Minute".into();
        }
    }

    /// Every few minutes, post a report to the Discord webhook. With screenshots on, the helpers are asked for
    /// them first and the report goes out a few seconds later.
    fn report(&mut self) {
        let d = self.config.discord.clone();
        if !config::Discord::valid_webhook(&d.webhook) {
            return;
        }
        if let Some((id, asked)) = self.report_pending {
            if asked.elapsed() >= Duration::from_secs(6) {
                self.report_pending = None;
                self.post_report(Some(id));
            }
            return;
        }
        if self.last_report.elapsed() >= Duration::from_secs(d.every_minutes.max(5) as u64 * 60) {
            self.send_report_now();
        }
    }

    /// Start a report right away (also the Send Test Report button).
    pub fn send_report_now(&mut self) {
        self.last_report = Instant::now();
        if self.config.discord.screenshots {
            let id = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64);
            let dir = helper::report_dir();
            let _ = std::fs::create_dir_all(&dir);
            if std::fs::write(dir.join("request.txt"), id.to_string()).is_ok() {
                self.report_pending = Some((id, Instant::now()));
                return;
            }
        }
        self.post_report(None);
    }

    fn post_report(&mut self, shots: Option<u64>) {
        let dir = helper::report_dir();
        let mut accounts = Vec::new();
        for p in &self.profiles {
            let a = self.config.account(&p.sid);
            let mine: Vec<&Client> = self.clients.iter().filter(|c| c.sid == p.sid).collect();
            let stats: serde_json::Value = std::fs::read_to_string(dir.join(format!("{}.json", p.sid))).ok()
                .and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
            let total = stats["freezes"].as_u64().unwrap_or(0) as u32;
            let last = self.reported_freezes.insert(p.sid.clone(), total).unwrap_or(total);
            let images: Vec<PathBuf> = match (shots, stats["id"].as_u64()) {
                (Some(id), Some(got)) if got == id => (0..stats["shots"].as_u64().unwrap_or(0).min(4))
                    .map(|i| dir.join(format!("{}-{i}.png", p.sid))).filter(|f| f.exists()).collect(),
                _ => Vec::new(),
            };
            accounts.push(crate::report::Account {
                name: p.name.clone(),
                on: a.on,
                clients: mine.iter().map(|c| crate::report::Client {
                    ram: c.ram, cpu: self.cpu_pct.get(&c.pid).copied().unwrap_or(0.0),
                    up_minutes: system_time_now().saturating_sub(c.started) / 600_000_000,
                }).collect(),
                freezes: total.saturating_sub(last),
                frozen_now: stats["frozenNow"].as_u64().unwrap_or(0) as u32,
                restarts: self.restarts.remove(&p.sid).unwrap_or(0),
                screenshots: images,
            });
        }
        let pc = crate::report::Pc { used: self.memory.0, total: self.memory.1, freed: self.freed, version: env!("CARGO_PKG_VERSION") };
        let (payload, files) = crate::report::build(&pc, &accounts);
        let webhook = self.config.discord.webhook.clone();
        std::thread::spawn(move || {
            let mut args: Vec<String> = vec!["-s".into(), "-m".into(), "60".into(), "--form-string".into(), format!("payload_json={payload}")];
            for (i, f) in files.iter().enumerate() {
                args.push("-F".into());
                args.push(format!("files[{i}]=@{};type=image/png;filename=shot{i}.png", f.display()));
            }
            args.push(webhook);
            let args: Vec<&str> = args.iter().map(String::as_str).collect();
            system::run("curl", &args);
        });
    }

    /// This login's windows here; every other login's through the helper, installed while any account wants it.
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

    pub fn trim_all(&mut self, say: bool) {
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

    pub fn clean(&mut self, say: bool) {
        self.last_clean = Instant::now();
        let before = system::memory().0;
        let ok = system::clean_standby();
        self.trim_all(false);
        self.memory = system::memory();
        if say {
            self.note = if ok { format!("Cleaned Memory, Freed {}", gb(before.saturating_sub(self.memory.0))) } else { "Couldn't Clean Memory".into() };
        }
    }

    /// Close every client and reopen each in the server it was in.
    pub fn restart_clients(&mut self) {
        self.last_restart = Instant::now();
        self.restart(|_| true);
    }

    /// Hand each account's join links to its login (helper.rs), one request file per account. Returns how
    /// many accounts got one.
    fn send_rejoins(&self, servers: &[(&str, Vec<rejoin::Server>)]) -> usize {
        let id = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64);
        let dir = helper::rejoin_dir();
        let _ = std::fs::create_dir_all(&dir);
        servers.iter().filter(|(sid, list)| std::fs::write(dir.join(format!("{sid}.txt")), rejoin::request(id, list)).is_ok()).count()
    }

    /// Close the picked clients and reopen each in the server it was in. The reopening happens in each
    /// account's own login (helper.rs); clients whose server can't be read are left running.
    fn restart(&mut self, pick: impl Fn(&Client) -> bool) {
        let mut servers: HashMap<&str, Vec<rejoin::Server>> = HashMap::new();
        let (mut close, mut unknown) = (Vec::new(), 0);
        for c in self.clients.iter().filter(|c| pick(c)) {
            let log = self.profiles.iter().find(|p| p.sid == c.sid).and_then(|p| files::client_log(&p.local, c.started));
            match rejoin::server(system::command_line(c.pid).as_deref(), log.as_deref()) {
                Some(s) if !c.sid.is_empty() => {
                    servers.entry(c.sid.as_str()).or_default().push(s);
                    close.push(c.pid);
                }
                _ => unknown += 1,
            }
        }
        let list: Vec<(&str, Vec<rejoin::Server>)> = servers.into_iter().collect();
        if self.send_rejoins(&list) < list.len() {
            self.note = "Couldn't Hand Every Restart To Its Account".into();
            return;
        }
        for (sid, l) in &list {
            *self.restarts.entry(sid.to_string()).or_default() += l.len() as u32;
        }
        self.closed_by_us.extend(close.iter().copied());
        let closed = close.iter().filter(|pid| system::close(**pid)).count();
        self.note = format!("Restarting {closed} {}{}", if closed == 1 { "Client" } else { "Clients" },
            if unknown > 0 { format!(". Couldn't Find The Server For {unknown}, Left Those Open") } else { String::new() });
        self.last_tick = None;
    }

    pub fn restore(&mut self) {
        let mut n = 0;
        for p in &self.profiles {
            if self.config.account(&p.sid).on {
                n += files::restore(&files::all_files(&p.local)).unwrap_or(0);
                self.config.account_mut(&p.sid).on = false;   // otherwise the next tick would apply everything again
            }
        }
        self.changed();
        self.note = format!("Restored {n} Files. Switch an account back on to use Potato again.");
    }
}

/// Now, as a Windows file time (100 ns since 1601), to compare with client start times.
fn system_time_now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos() as u64 / 100 + 116_444_736_000_000_000)
}

const GIB: f64 = (1u64 << 30) as f64;

pub fn gb(bytes: u64) -> String {
    let g = bytes as f64 / GIB;
    if bytes >= 1000 << 20 { format!("{g:.1} GB") } else { format!("{:.0} MB", bytes as f64 / (1u64 << 20) as f64) }
}

/// The demo build shows the update banner when POTATO_DEMO_UPDATE is set, so a test render can check it.
#[cfg(not(windows))]
fn check_for_update(update: Update) {
    if std::env::var_os("POTATO_DEMO_UPDATE").is_some() {
        *update.lock().unwrap() = UpdateState::Available(Release { tag: "v9.9.9".into(), exe_url: String::new(), sums_url: String::new() });
    }
}

#[cfg(not(windows))]
pub fn install_update(_: Release, _: Update) {}

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
pub fn install_update(r: Release, update: Update) {
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
