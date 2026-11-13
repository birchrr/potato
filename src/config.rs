//! Potato's own settings, one JSON file shared by every Windows user on the PC.

use serde_json::{json, Value};

use crate::tweaks::Plan;

#[derive(Clone, Debug, PartialEq)]
pub struct Config {
    pub plan: Plan,
    /// SIDs of the Windows users whose Roblox Potato keeps light
    pub users: Vec<String>,
    /// trim every Roblox client this often (0 = off)
    pub trim_minutes: u32,
    /// empty the standby list when RAM in use goes over this percent (0 = off)
    pub clean_above: u32,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            plan: Plan { lowest_graphics: true, fps_cap: Some(30), extras: false, macro_safe: true },
            users: Vec::new(),
            trim_minutes: 5,
            clean_above: 0,
        }
    }
}

impl Config {
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(&json!({
            "lowestGraphics": self.plan.lowest_graphics, "fpsCap": self.plan.fps_cap, "extras": self.plan.extras,
            "macroSafe": self.plan.macro_safe, "users": self.users, "trimMinutes": self.trim_minutes,
            "cleanAbove": self.clean_above,
        }))
        .unwrap()
    }

    /// Settings from the file; anything missing or broken falls back to the default.
    pub fn from_json(text: &str) -> Config {
        let d = Config::default();
        let Ok(v) = serde_json::from_str::<Value>(text) else { return d };
        let flag = |k: &str, or: bool| v[k].as_bool().unwrap_or(or);
        let num = |k: &str, or: u32| v[k].as_u64().map(|n| n as u32).unwrap_or(or);
        Config {
            plan: Plan {
                lowest_graphics: flag("lowestGraphics", d.plan.lowest_graphics),
                fps_cap: if v["fpsCap"].is_null() && v.get("fpsCap").is_some() { None } else { Some(num("fpsCap", 30)) },
                extras: flag("extras", d.plan.extras),
                macro_safe: flag("macroSafe", d.plan.macro_safe),
            },
            users: v["users"].as_array().map(|a| a.iter().filter_map(|s| s.as_str().map(String::from)).collect()).unwrap_or_default(),
            trim_minutes: num("trimMinutes", d.trim_minutes),
            clean_above: num("cleanAbove", d.clean_above),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips() {
        let c = Config { plan: Plan { fps_cap: None, ..Config::default().plan }, users: vec!["S-1-5-21-1".into()], trim_minutes: 2, clean_above: 85 };
        assert_eq!(Config::from_json(&c.to_json()), c);
        let c = Config { trim_minutes: 0, ..Config::default() };
        assert_eq!(Config::from_json(&c.to_json()), c);
    }

    #[test]
    fn broken_file_gives_defaults() {
        assert_eq!(Config::from_json("{"), Config::default());
        assert_eq!(Config::from_json("{}"), Config::default());
    }
}
