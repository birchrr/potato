//! The Windows calls, faked with the accounts and clients of a typical RDP setup, so the window can be drawn
//! on any OS. Mirrors system.rs.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::config::Priority;

#[derive(Clone, Debug)]
pub struct Client {
    pub pid: u32,
    pub session: u32,
    pub sid: String,
    pub user: String,
    pub ram: u64,
    pub started: u64,
    pub cpu_time: u64,
}

#[derive(Clone, Debug)]
pub struct Profile {
    pub sid: String,
    pub name: String,
    pub local: PathBuf,
}

pub struct Cpu {
    pub priority: Priority,
    pub cores: Option<u64>,
    pub efficiency: bool,
}

const MB: u64 = 1 << 20;
/// After a trim the sample clients use 12% of their RAM: 3.4 GB freed, like a user's real 4 clients.
static TRIMMED: AtomicBool = AtomicBool::new(false);
fn ram(mb: u64) -> u64 {
    if TRIMMED.load(Ordering::Relaxed) { mb * MB * 12 / 100 } else { mb * MB }
}
const ACCOUNTS: [(&str, u32, u64); 5] = [("Jay", 1, 0), ("Roblox", 10, 1450), ("Main", 7, 1020), ("Alt1", 8, 797), ("Alt2", 9, 713)];

pub fn is_admin() -> bool {
    true
}

pub fn relaunch_as_admin() -> bool {
    false
}

pub fn enable_privileges() {}

pub fn clients() -> Vec<Client> {
    ACCOUNTS.iter().enumerate().filter(|(_, a)| a.2 > 0).map(|(i, (name, session, mb))| Client {
        pid: 1000 + i as u32, session: *session, sid: format!("S-1-5-21-{i}"), user: name.to_string(), ram: ram(*mb), started: i as u64,
        cpu_time: i as u64 * 1_000_000,
    }).collect()
}

pub fn profiles() -> Vec<Profile> {
    ACCOUNTS.iter().enumerate().map(|(i, (name, _, _))| Profile {
        sid: format!("S-1-5-21-{i}"), name: name.to_string(), local: std::env::temp_dir().join("potato-demo-missing"),
    }).collect()
}

pub fn command_line(_: u32) -> Option<String> {
    None
}

pub fn current_session() -> u32 {
    1
}

pub fn console_session() -> u32 {
    1
}

pub fn current_sid() -> String {
    "S-1-5-21-0".into()
}

pub fn close(_: u32) -> bool {
    true
}

pub fn trim(_: u32) -> bool {
    TRIMMED.store(true, Ordering::Relaxed);
    true
}

pub fn logical_cores() -> usize {
    16
}

pub fn set_cpu(_: u32, _: &Cpu) -> bool {
    true
}

pub fn cpu_cap(_: u32, _: u32, job: &mut Option<isize>) -> bool {
    *job = Some(1);
    true
}

pub fn drop_job(_: isize) {}

pub fn animations_on() -> bool {
    true
}

pub fn local_minutes() -> u32 {
    12 * 60
}

pub fn run(_: &str, _: &[&str]) -> bool {
    true
}

pub fn clean_standby() -> bool {
    true
}

pub fn notify(_: &str) {}

pub fn memory() -> (u64, u64) {
    let roblox: u64 = ACCOUNTS.iter().map(|a| ram(a.2)).sum();
    (19_560 * MB - ACCOUNTS.iter().map(|a| a.2 * MB).sum::<u64>() + roblox, 31_850 * MB)
}
