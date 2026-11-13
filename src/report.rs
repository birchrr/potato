//! The Discord report: one summary embed for the PC, then one embed per account with its numbers and a
//! screenshot of its Roblox. Discord takes at most 10 embeds and 10 files per message.

use std::path::PathBuf;

use serde_json::{Value, json};

pub struct Pc {
    pub used: u64,
    pub total: u64,
    pub freed: u64,
    pub version: &'static str,
}

pub struct Client {
    pub ram: u64,
    pub cpu: f32,
    pub up_minutes: u64,
}

pub struct Account {
    pub name: String,
    pub on: bool,
    pub clients: Vec<Client>,
    /// times a Roblox window stopped responding since the last report
    pub freezes: u32,
    pub frozen_now: u32,
    /// restarts and crash rejoins since the last report
    pub restarts: u32,
    pub screenshots: Vec<PathBuf>,
}

const POTATO: u32 = 0xD9A35B;
const GOOD: u32 = 0x4ADE80;
const WARN: u32 = 0xF5B041;
const GREY: u32 = 0x5A5A5A;

fn gb(bytes: u64) -> String {
    let g = bytes as f64 / (1u64 << 30) as f64;
    if bytes >= 1000 << 20 { format!("{g:.1} GB") } else { format!("{:.0} MB", bytes as f64 / (1u64 << 20) as f64) }
}

fn uptime(minutes: u64) -> String {
    if minutes >= 60 { format!("{}h {}m", minutes / 60, minutes % 60) } else { format!("{minutes}m") }
}

/// The webhook body (`payload_json`) and the screenshot files to attach, in order (`attachment://shotN.png`).
pub fn build(pc: &Pc, accounts: &[Account]) -> (String, Vec<PathBuf>) {
    let running: usize = accounts.iter().map(|a| a.clients.len()).sum();
    let roblox: u64 = accounts.iter().flat_map(|a| &a.clients).map(|c| c.ram).sum();
    let mut embeds = vec![json!({
        "title": "Potato Report",
        "color": POTATO,
        "fields": [
            {"name": "Roblox Open", "value": format!("{running} {}", if running == 1 { "client" } else { "clients" }), "inline": true},
            {"name": "Roblox RAM", "value": gb(roblox), "inline": true},
            {"name": "PC RAM", "value": format!("{} / {}", gb(pc.used), gb(pc.total)), "inline": true},
            {"name": "Freed By Potato", "value": gb(pc.freed), "inline": true},
        ],
        "footer": {"text": format!("Potato {}", pc.version)},
    })];
    let mut files = Vec::new();
    for a in accounts.iter().take(9) {
        let status = if !a.on { "Potato Off" } else if a.frozen_now > 0 { "Not Responding" } else if a.clients.is_empty() { "Closed" } else { "Running" };
        let color = if a.frozen_now > 0 { WARN } else if a.clients.is_empty() || !a.on { GREY } else { GOOD };
        let ram: u64 = a.clients.iter().map(|c| c.ram).sum();
        let cpu: f32 = a.clients.iter().map(|c| c.cpu).sum();
        let up = a.clients.iter().map(|c| c.up_minutes).max().unwrap_or(0);
        let mut e = json!({
            "title": a.name,
            "color": color,
            "fields": [
                {"name": "Status", "value": status, "inline": true},
                {"name": "RAM", "value": if a.clients.is_empty() { "-".to_string() } else { gb(ram) }, "inline": true},
                {"name": "CPU", "value": if a.clients.is_empty() { "-".to_string() } else { format!("{cpu:.0}%") }, "inline": true},
                {"name": "Up For", "value": if a.clients.is_empty() { "-".to_string() } else { uptime(up) }, "inline": true},
                {"name": "Freezes", "value": a.freezes.to_string(), "inline": true},
                {"name": "Restarts", "value": a.restarts.to_string(), "inline": true},
            ],
        });
        if let Some(shot) = a.screenshots.first() {
            e["image"] = json!({ "url": format!("attachment://shot{}.png", files.len()) });
            files.push(shot.clone());
        }
        embeds.push(e);
    }
    let attachments: Vec<Value> = (0..files.len()).map(|i| json!({"id": i, "filename": format!("shot{i}.png")})).collect();
    let payload = json!({ "username": "Potato", "embeds": embeds, "attachments": attachments, "allowed_mentions": {"parse": []} });
    (payload.to_string(), files)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn account(name: &str, clients: usize, shot: bool) -> Account {
        Account {
            name: name.into(), on: true,
            clients: (0..clients).map(|_| Client { ram: 1 << 30, cpu: 12.4, up_minutes: 135 }).collect(),
            freezes: 1, frozen_now: 0, restarts: 2,
            screenshots: if shot { vec![PathBuf::from(format!("/tmp/{name}.png"))] } else { vec![] },
        }
    }

    #[test]
    fn one_embed_per_account_with_its_screenshot() {
        let pc = Pc { used: 19 << 30, total: 31 << 30, freed: 3 << 30, version: "0.5.0" };
        let (payload, files) = build(&pc, &[account("Main", 1, true), account("Jay", 0, false), account("Alt1", 1, true)]);
        let v: Value = serde_json::from_str(&payload).unwrap();
        assert_eq!(v["embeds"].as_array().unwrap().len(), 4);
        assert_eq!(files, vec![PathBuf::from("/tmp/Main.png"), PathBuf::from("/tmp/Alt1.png")]);
        assert_eq!(v["embeds"][1]["image"]["url"], "attachment://shot0.png");
        assert_eq!(v["embeds"][3]["image"]["url"], "attachment://shot1.png");
        assert!(v["embeds"][2].get("image").is_none());
        assert_eq!(v["embeds"][1]["fields"][3]["value"], "2h 15m");
        assert_eq!(v["embeds"][2]["fields"][0]["value"], "Closed");
        assert_eq!(v["allowed_mentions"]["parse"], json!([]));   // account names can't ping anyone
    }

    #[test]
    fn stays_inside_discords_limits() {
        let many: Vec<Account> = (0..15).map(|i| account(&format!("A{i}"), 1, true)).collect();
        let (payload, files) = build(&Pc { used: 0, total: 0, freed: 0, version: "x" }, &many);
        let v: Value = serde_json::from_str(&payload).unwrap();
        assert_eq!(v["embeds"].as_array().unwrap().len(), 10);
        assert!(files.len() <= 10);
    }
}
