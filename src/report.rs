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

/// One look at the PC, taken every 30 seconds between reports for the graphs.
pub struct Sample {
    /// seconds since the first sample
    pub at: u64,
    pub pc_used: u64,
    pub roblox: u64,
    /// Roblox's CPU use, percent of the whole PC
    pub cpu: f32,
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

/// The webhook body (`payload_json`) and the files to attach, in order (`attachment://shotN.png`): the report
/// image (`card`, drawn by crate::card) first, then one Roblox screenshot per account that has one.
pub fn build(accounts: &[Account], card: PathBuf) -> (String, Vec<PathBuf>) {
    let mut embeds = vec![json!({ "color": POTATO, "image": { "url": "attachment://shot0.png" } })];
    let mut files = vec![card];
    for a in accounts.iter().filter(|a| !a.screenshots.is_empty()).take(9) {
        let status = if !a.on { "Potato Off" } else if a.frozen_now > 0 { "Not Responding" } else if a.clients.is_empty() { "Closed" } else { "Running" };
        let color = if a.frozen_now > 0 { WARN } else if a.clients.is_empty() || !a.on { GREY } else { GOOD };
        embeds.push(json!({
            "title": format!("{} · {status}", a.name),
            "color": color,
            "image": { "url": format!("attachment://shot{}.png", files.len()) },
        }));
        files.push(a.screenshots[0].clone());
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
    fn report_image_first_then_each_screenshot() {
        let (payload, files) = build(&[account("Main", 1, true), account("Jay", 0, false), account("Alt1", 1, true)], "/tmp/card.png".into());
        let v: Value = serde_json::from_str(&payload).unwrap();
        assert_eq!(files, vec![PathBuf::from("/tmp/card.png"), PathBuf::from("/tmp/Main.png"), PathBuf::from("/tmp/Alt1.png")]);
        assert_eq!(v["embeds"].as_array().unwrap().len(), 3);
        assert_eq!(v["embeds"][0]["image"]["url"], "attachment://shot0.png");
        assert_eq!(v["embeds"][1]["title"], "Main · Running");
        assert_eq!(v["embeds"][2]["image"]["url"], "attachment://shot2.png");
        assert_eq!(v["allowed_mentions"]["parse"], json!([]));   // account names can't ping anyone
    }

    #[test]
    fn stays_inside_discords_limits() {
        let many: Vec<Account> = (0..15).map(|i| account(&format!("A{i}"), 1, true)).collect();
        let (payload, files) = build(&many, "/tmp/card.png".into());
        let v: Value = serde_json::from_str(&payload).unwrap();
        assert_eq!(v["embeds"].as_array().unwrap().len(), 10);
        assert_eq!(files.len(), 10);
    }
}
