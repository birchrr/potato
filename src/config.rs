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
}

/// One-click starting points for an account.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Preset {
    Main,
    Alt,
    UltraLow,
}

impl Preset {
    pub const ALL: [Preset; 3] = [Preset::Main, Preset::Alt, Preset::UltraLow];

    pub fn label(self) -> &'static str {
        match self {
            Preset::Main => "Main",
            Preset::Alt => "Alt",
            Preset::UltraLow => "Ultra Low",
        }
    }

    pub fn account(self) -> Account {
        let plan = |fps, ultra_low| Plan { lowest_graphics: true, fps_cap: Some(fps), ultra_low, maximized: true };
        match self {
            Preset::Main => Account { on: true, plan: plan(30, false), priority: Priority::Normal, efficiency: false, max_clients: 0 },
            Preset::Alt => Account { on: true, plan: plan(25, false), priority: Priority::BelowNormal, efficiency: false, max_clients: 0 },
            Preset::UltraLow => Account { on: true, plan: plan(15, true), priority: Priority::Idle, efficiency: true, max_clients: 0 },
        }
    }

    /// The preset an account's settings match, if any (Close Extra Clients doesn't count).
    pub fn of(a: &Account) -> Option<Preset> {
        Preset::ALL.into_iter().find(|p| Account { on: a.on, max_clients: a.max_clients, ..p.account() } == *a)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Config {
    /// by Windows account SID; accounts not listed get `Preset::Main`
    pub accounts: BTreeMap<String, Account>,
    /// logical cores each client gets to itself (0 = off: Windows decides)
    pub cores_per_client: u32,
    /// trim every Roblox client this often (0 = off)
    pub trim_minutes: u32,
    /// empty the standby list when RAM in use goes over this percent (0 = off)
    pub clean_above: u32,
    /// restart every client into its server this often (0 = off)
    pub restart_hours: u32,
}

impl Default for Config {
    fn default() -> Self {
        Config { accounts: BTreeMap::new(), cores_per_client: 0, trim_minutes: 5, clean_above: 0, restart_hours: 0 }
    }
}

impl Config {
    pub fn account(&self, sid: &str) -> Account {
        self.accounts.get(sid).cloned().unwrap_or_else(|| Preset::Main.account())
    }

    pub fn account_mut(&mut self, sid: &str) -> &mut Account {
        self.accounts.entry(sid.to_string()).or_insert_with(|| Preset::Main.account())
    }

    pub fn to_json(&self) -> String {
        let accounts: serde_json::Map<String, Value> = self.accounts.iter().map(|(sid, a)| (sid.clone(), json!({
            "on": a.on, "lowestGraphics": a.plan.lowest_graphics, "fpsCap": a.plan.fps_cap, "ultraLow": a.plan.ultra_low,
            "maximized": a.plan.maximized, "priority": a.priority.key(), "efficiency": a.efficiency, "maxClients": a.max_clients,
        }))).collect();
        serde_json::to_string_pretty(&json!({
            "accounts": accounts, "coresPerClient": self.cores_per_client, "trimMinutes": self.trim_minutes, "cleanAbove": self.clean_above,
            "restartHours": self.restart_hours,
        }))
        .unwrap()
    }

    /// Settings from the file; anything missing or broken falls back to the default.
    pub fn from_json(text: &str) -> Config {
        let d = Config::default();
        let Ok(v) = serde_json::from_str::<Value>(text) else { return d };
        let num = |v: &Value, or: u32| v.as_u64().map(|n| n as u32).unwrap_or(or);
        let accounts = v["accounts"].as_object().map(|m| m.iter().map(|(sid, a)| {
            let base = Preset::Main.account();
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
            })
        }).collect()).unwrap_or_default();
        Config {
            accounts,
            cores_per_client: num(&v["coresPerClient"], d.cores_per_client),
            trim_minutes: num(&v["trimMinutes"], d.trim_minutes),
            clean_above: num(&v["cleanAbove"], d.clean_above),
            restart_hours: num(&v["restartHours"], d.restart_hours),
        }
    }
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
        let mut c = Config { cores_per_client: 2, trim_minutes: 0, clean_above: 85, restart_hours: 6, ..Config::default() };
        c.accounts.insert("S-1-5-21-1".into(), Preset::UltraLow.account());
        c.account_mut("S-1-5-21-2").plan.fps_cap = None;
        c.account_mut("S-1-5-21-2").max_clients = 1;
        c.account_mut("S-1-5-21-3").on = false;
        assert_eq!(Config::from_json(&c.to_json()), c);
    }

    #[test]
    fn broken_file_gives_defaults() {
        assert_eq!(Config::from_json("{"), Config::default());
        assert_eq!(Config::from_json("{}"), Config::default());
    }

    #[test]
    fn presets_are_recognised() {
        let mut a = Preset::Alt.account();
        assert_eq!(Preset::of(&a), Some(Preset::Alt));
        a.max_clients = 1;
        assert_eq!(Preset::of(&a), Some(Preset::Alt));
        a.plan.fps_cap = Some(40);
        assert_eq!(Preset::of(&a), None);
        assert_eq!(Config::default().account("anyone"), Preset::Main.account());
    }
}
