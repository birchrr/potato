//! Potato's own settings, one JSON file shared by every Windows user on the PC: one set of settings per
//! Windows account (each RDP login is its own account), plus a few that are the same for the whole PC.

use std::collections::BTreeMap;

use serde_json::{json, Value};

use crate::tweaks::Plan;

/// CPU priority for Roblox clients. Normal leaves Windows' default.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Priority {
    Normal,
    BelowNormal,
    Idle,
}

impl Priority {
    pub const ALL: [Priority; 3] = [Priority::Normal, Priority::BelowNormal, Priority::Idle];

    pub fn label(self) -> &'static str {
        match self {
            Priority::Normal => "Normal",
            Priority::BelowNormal => "Lower",
            Priority::Idle => "Low",
        }
    }

    fn key(self) -> &'static str {
        match self {
            Priority::Normal => "normal",
            Priority::BelowNormal => "belowNormal",
            Priority::Idle => "idle",
        }
    }
}

/// Everything Potato does to one Windows account's Roblox.
#[derive(Clone, Debug, PartialEq)]
pub struct Account {
    /// off: Potato leaves this account alone
    pub on: bool,
    pub plan: Plan,
    pub priority: Priority,
    /// Windows 11 Efficiency Mode
    pub efficiency: bool,
    /// keep only this many Roblox clients, the newest; older ones are closed (0 = off)
    pub max_clients: u32,
    /// silence the clients (the account's helper does it, in its own login)
    pub mute: bool,
    /// hard CPU cap per client in percent of the whole PC (0 = off)
    pub cpu_limit: u32,
    /// restart a client into its server once it uses more than this many MB (0 = off)
    pub ram_limit_mb: u32,
    /// reopen a crashed client in the server it was in
    pub rejoin_crashed: bool,
}

impl Default for Account {
    /// what an account gets before anyone changes it: lowest graphics at 30 FPS, opened maximized
    fn default() -> Self {
        Account {
            on: true,
            plan: Plan { lowest_graphics: true, fps_cap: Some(30), ultra_low: false, maximized: true },
            priority: Priority::Normal,
            efficiency: false,
            max_clients: 0,
            mute: false,
            cpu_limit: 0,
            ram_limit_mb: 0,
            rejoin_crashed: false,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Config {
    /// by Windows account SID; accounts not listed get `Account::default()`
    pub accounts: BTreeMap<String, Account>,
    /// logical cores each client gets to itself (0 = off: Windows decides)
    pub cores_per_client: u32,
    /// trim every Roblox client this often (0 = off)
    pub trim_minutes: u32,
    /// empty the standby list when RAM in use goes over this percent (0 = off)
    pub clean_above: u32,
    /// restart every client into its server this often (0 = off)
    pub restart_hours: u32,
    /// Roblox's upload speed limit in kbit/s (0 = none)
    pub upload_kbps: u32,
    /// turn the PC off at this time each day, "HH:MM" (empty = never)
    pub shutdown_at: String,
    pub discord: Discord,
}

/// Status reports posted to a Discord channel through a webhook.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Discord {
    /// https://discord.com/api/webhooks/... (empty = off)
    pub webhook: String,
    pub every_minutes: u32,
    pub screenshots: bool,
}

impl Discord {
    /// Only Discord's own webhook addresses are used, so a typo can't send reports somewhere else.
    pub fn valid_webhook(url: &str) -> bool {
        ["https://discord.com/api/webhooks/", "https://discordapp.com/api/webhooks/", "https://ptb.discord.com/api/webhooks/", "https://canary.discord.com/api/webhooks/"]
            .iter().any(|p| url.strip_prefix(p).is_some_and(|rest| !rest.is_empty() && rest.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '-' | '_'))))
    }
}

impl Default for Config {
    fn default() -> Self {
        Config {
            accounts: BTreeMap::new(), cores_per_client: 0, trim_minutes: 5, clean_above: 0, restart_hours: 0, upload_kbps: 0,
            shutdown_at: String::new(), discord: Discord { webhook: String::new(), every_minutes: 30, screenshots: true },
        }
    }
}

impl Config {
    pub fn account(&self, sid: &str) -> Account {
        self.accounts.get(sid).cloned().unwrap_or_default()
    }

    pub fn account_mut(&mut self, sid: &str) -> &mut Account {
        self.accounts.entry(sid.to_string()).or_default()
    }

    pub fn to_json(&self) -> String {
        let accounts: serde_json::Map<String, Value> = self.accounts.iter().map(|(sid, a)| (sid.clone(), json!({
            "on": a.on, "lowestGraphics": a.plan.lowest_graphics, "fpsCap": a.plan.fps_cap, "ultraLow": a.plan.ultra_low,
            "maximized": a.plan.maximized, "priority": a.priority.key(), "efficiency": a.efficiency, "maxClients": a.max_clients,
            "mute": a.mute, "cpuLimit": a.cpu_limit, "ramLimitMb": a.ram_limit_mb, "rejoinCrashed": a.rejoin_crashed,
        }))).collect();
        serde_json::to_string_pretty(&json!({
            "accounts": accounts, "coresPerClient": self.cores_per_client, "trimMinutes": self.trim_minutes, "cleanAbove": self.clean_above,
            "restartHours": self.restart_hours, "uploadKbps": self.upload_kbps, "shutdownAt": self.shutdown_at,
            "discord": { "webhook": self.discord.webhook, "everyMinutes": self.discord.every_minutes, "screenshots": self.discord.screenshots },
        }))
        .unwrap()
    }

    /// Settings from the file; anything missing or broken falls back to the default.
    pub fn from_json(text: &str) -> Config {
        let d = Config::default();
        let Ok(v) = serde_json::from_str::<Value>(text) else { return d };
        let num = |v: &Value, or: u32| v.as_u64().map(|n| n as u32).unwrap_or(or);
        let accounts = v["accounts"].as_object().map(|m| m.iter().map(|(sid, a)| {
            let base = Account::default();
            let flag = |k: &str, or: bool| a[k].as_bool().unwrap_or(or);
            (sid.clone(), Account {
                on: flag("on", base.on),
                plan: Plan {
                    lowest_graphics: flag("lowestGraphics", base.plan.lowest_graphics),
                    fps_cap: if a.get("fpsCap").is_some_and(Value::is_null) { None } else { Some(num(&a["fpsCap"], 30)) },
                    ultra_low: flag("ultraLow", base.plan.ultra_low),
                    maximized: flag("maximized", base.plan.maximized),
                },
                priority: Priority::ALL.into_iter().find(|p| a["priority"].as_str() == Some(p.key())).unwrap_or(base.priority),
                efficiency: flag("efficiency", base.efficiency),
                max_clients: num(&a["maxClients"], 0),
                mute: flag("mute", base.mute),
                cpu_limit: num(&a["cpuLimit"], 0).min(100),
                ram_limit_mb: num(&a["ramLimitMb"], 0),
                rejoin_crashed: flag("rejoinCrashed", base.rejoin_crashed),
            })
        }).collect()).unwrap_or_default();
        Config {
            accounts,
            cores_per_client: num(&v["coresPerClient"], d.cores_per_client),
            trim_minutes: num(&v["trimMinutes"], d.trim_minutes),
            clean_above: num(&v["cleanAbove"], d.clean_above),
            restart_hours: num(&v["restartHours"], d.restart_hours),
            upload_kbps: num(&v["uploadKbps"], d.upload_kbps),
            shutdown_at: v["shutdownAt"].as_str().and_then(parse_time).map(format_time).unwrap_or_default(),
            discord: Discord {
                webhook: v["discord"]["webhook"].as_str().filter(|u| Discord::valid_webhook(u)).unwrap_or_default().to_string(),
                every_minutes: num(&v["discord"]["everyMinutes"], d.discord.every_minutes).max(5),
                screenshots: v["discord"]["screenshots"].as_bool().unwrap_or(d.discord.screenshots),
            },
        }
    }
}

/// A time of day as minutes after midnight. Forgiving about how it's typed: "06:30", "6:30", "6.30", "630",
/// "0630", "18", "6pm", "6:30 pm".
pub fn parse_time(t: &str) -> Option<u32> {
    let t = t.trim().to_ascii_lowercase();
    let (t, pm) = match (t.strip_suffix("pm"), t.strip_suffix("am")) {
        (Some(x), _) => (x.trim().to_string(), Some(true)),
        (_, Some(x)) => (x.trim().to_string(), Some(false)),
        _ => (t, None),
    };
    let (h, m) = match t.split_once([':', '.']) {
        Some((h, m)) => (h.parse::<u32>().ok()?, m.parse::<u32>().ok()?),
        None if t.len() >= 3 && t.len() <= 4 => (t[..t.len() - 2].parse().ok()?, t[t.len() - 2..].parse().ok()?),
        None => (t.parse().ok()?, 0),
    };
    let h = match pm {
        Some(true) if h < 12 => h + 12,
        Some(false) if h == 12 => 0,
        Some(_) if h > 12 => return None,
        _ => h,
    };
    (h < 24 && m < 60).then_some(h * 60 + m)
}

/// A time back as "HH:MM".
pub fn format_time(minutes: u32) -> String {
    format!("{:02}:{:02}", minutes / 60, minutes % 60)
}

/// The clients to close when accounts have a client limit: per account, everything but its newest
/// `max_clients`. Each client is (account SID, PID, start time).
pub fn extra_clients(config: &Config, clients: &[(&str, u32, u64)]) -> Vec<u32> {
    let mut by_account: BTreeMap<&str, Vec<(u64, u32)>> = BTreeMap::new();
    for &(sid, pid, started) in clients {
        by_account.entry(sid).or_default().push((started, pid));
    }
    let mut out = Vec::new();
    for (sid, mut list) in by_account {
        let a = config.account(sid);
        if !a.on || a.max_clients == 0 || sid.is_empty() {
            continue;   // no limit, or a client whose account Potato couldn't read
        }
        list.sort_by(|x, y| y.cmp(x));   // newest first
        out.extend(list.into_iter().skip(a.max_clients as usize).map(|(_, pid)| pid));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extra_clients_closes_the_oldest_past_the_limit() {
        let mut c = Config::default();
        c.account_mut("A").max_clients = 1;
        c.account_mut("B").max_clients = 2;
        let clients = [("A", 1, 100), ("A", 2, 300), ("A", 3, 200), ("B", 4, 1), ("B", 5, 2), ("C", 6, 1), ("C", 7, 2), ("", 8, 1), ("", 9, 2)];
        let mut out = extra_clients(&c, &clients);
        out.sort();
        assert_eq!(out, vec![1, 3]);   // A keeps its newest (2); B is within its limit; C has none; unknown accounts are left alone
        c.account_mut("A").on = false;
        assert!(extra_clients(&c, &clients).is_empty());
    }

    #[test]
    fn round_trips() {
        let mut c = Config { cores_per_client: 2, trim_minutes: 0, clean_above: 85, restart_hours: 6, upload_kbps: 512,
                                shutdown_at: "06:30".into(), ..Config::default() };
        c.discord = Discord { webhook: "https://discord.com/api/webhooks/123/abc-DEF_9".into(), every_minutes: 15, screenshots: false };
        c.account_mut("S-1-5-21-4").mute = true;
        c.account_mut("S-1-5-21-4").cpu_limit = 25;
        c.account_mut("S-1-5-21-4").ram_limit_mb = 2500;
        c.account_mut("S-1-5-21-4").rejoin_crashed = true;
        c.account_mut("S-1-5-21-1").plan.ultra_low = true;
        c.account_mut("S-1-5-21-1").priority = Priority::Idle;
        c.account_mut("S-1-5-21-2").plan.fps_cap = None;
        c.account_mut("S-1-5-21-2").max_clients = 1;
        c.account_mut("S-1-5-21-3").on = false;
        assert_eq!(Config::from_json(&c.to_json()), c);
    }

    #[test]
    fn times_and_webhooks_are_checked() {
        assert_eq!(parse_time("06:30"), Some(390));
        assert_eq!(parse_time("23:59"), Some(1439));
        assert_eq!(parse_time("24:00"), None);
        for (typed, want) in [("6:30", 390), ("6.30", 390), ("630", 390), ("0630", 390), ("18", 1080), ("6pm", 1080),
                              ("6:30 PM", 1110), ("12am", 0), ("12pm", 720), (" 23:05 ", 1385)] {
            assert_eq!(parse_time(typed), Some(want), "{typed}");
        }
        for bad in ["13pm", "25", "6:75", "abc", ""] {
            assert_eq!(parse_time(bad), None, "{bad}");
        }
        assert_eq!(format_time(390), "06:30");
        assert!(Discord::valid_webhook("https://discord.com/api/webhooks/123/abc-DEF_9"));
        assert!(!Discord::valid_webhook("https://evil.example/api/webhooks/1/2"));
        assert!(!Discord::valid_webhook("https://discord.com/api/webhooks/1/2?x=<script>"));
        assert!(!Discord::valid_webhook("https://discord.com/api/webhooks/"));
    }

    #[test]
    fn broken_file_gives_defaults() {
        assert_eq!(Config::from_json("{"), Config::default());
        assert_eq!(Config::from_json("{}"), Config::default());
    }

}
