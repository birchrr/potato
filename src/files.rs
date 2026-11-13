//! Where a Windows user's Roblox files are, and writing them with a backup Potato can restore.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::tweaks::{self, Plan};

const BACKUP: &str = "potato-backup";

/// The in-game settings file under a user's `AppData\Local` (Studio has its own; skip it).
pub fn basic_settings(local: &Path) -> Option<PathBuf> {
    fs::read_dir(local.join("Roblox")).ok()?.flatten().map(|e| e.path()).find(|p| {
        let n = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
        n.starts_with("GlobalBasicSettings_") && n.ends_with(".xml") && !n.contains("Studio")
    })
}

/// Every Fast Flag file Roblox could read for this user: one per installed Roblox version, plus the ones
/// Bloxstrap and Fishstrap copy into each launch.
pub fn flag_files(local: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(versions) = fs::read_dir(local.join("Roblox").join("Versions")) {
        for v in versions.flatten().map(|e| e.path()) {
            if v.join("RobloxPlayerBeta.exe").is_file() {
                out.push(v.join("ClientSettings").join("ClientAppSettings.json"));
            }
        }
    }
    for launcher in ["Bloxstrap", "Fishstrap"] {
        let dir = local.join(launcher);
        if dir.is_dir() {
            out.push(dir.join("Modifications").join("ClientSettings").join("ClientAppSettings.json"));
        }
    }
    out
}

fn backup_of(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(format!(".{BACKUP}"));
    path.with_file_name(name)
}

/// Write `text` if the file says something else. The first time Potato touches a file it keeps the original
/// next to it (a file that didn't exist gets an empty backup, so Restore deletes it). Returns whether it wrote.
pub fn write(path: &Path, text: &str) -> io::Result<bool> {
    let old = fs::read_to_string(path).ok();
    if old.as_deref() == Some(text) {
        return Ok(false);
    }
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let backup = backup_of(path);
    if !backup.exists() {
        fs::write(&backup, old.as_deref().unwrap_or(""))?;
    }
    fs::write(path, text)?;
    Ok(true)
}

/// Put back what was there before Potato. Returns how many files it restored.
pub fn restore(paths: &[PathBuf]) -> io::Result<usize> {
    let mut n = 0;
    for path in paths {
        let backup = backup_of(path);
        let Ok(old) = fs::read_to_string(&backup) else { continue };
        if old.is_empty() {
            let _ = fs::remove_file(path);
        } else {
            fs::write(path, old)?;
        }
        fs::remove_file(&backup)?;
        n += 1;
    }
    Ok(n)
}

/// Every file Potato may have changed for this user.
pub fn all_files(local: &Path) -> Vec<PathBuf> {
    basic_settings(local).into_iter().chain(flag_files(local)).collect()
}

/// Each of this user's files that the plan would change, with its new text.
fn changes(local: &Path, plan: &Plan) -> io::Result<Vec<(PathBuf, String)>> {
    let mut out = Vec::new();
    if let Some(xml_path) = basic_settings(local) {
        let xml = fs::read_to_string(&xml_path)?;
        let next = tweaks::apply_basic_settings(&xml, plan);
        if next != xml {
            out.push((xml_path, next));
        }
    }
    for path in flag_files(local) {
        let json = fs::read_to_string(&path).unwrap_or_default();
        if json.is_empty() && !plan.ultra_low {
            continue;   // nothing to add and nothing to take out
        }
        if let Some(next) = tweaks::apply_flags(&json, plan).filter(|n| *n != json) {
            out.push((path, next));
        }
    }
    Ok(out)
}

/// Whether this user's files already match the plan (nothing left to write).
pub fn is_applied(local: &Path, plan: &Plan) -> io::Result<bool> {
    Ok(changes(local, plan)?.is_empty())
}

/// Apply the plan to one user's files. Call it only while none of that user's Roblox clients run: Roblox
/// writes its settings file when it closes. Returns how many files changed.
pub fn apply(local: &Path, plan: &Plan) -> io::Result<usize> {
    let todo = changes(local, plan)?;
    for (path, text) in &todo {
        write(path, text)?;
    }
    Ok(todo.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("potato-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn user(name: &str) -> PathBuf {
        let local = temp(name);
        let r = local.join("Roblox");
        fs::create_dir_all(r.join("Versions/version-abc")).unwrap();
        fs::write(r.join("Versions/version-abc/RobloxPlayerBeta.exe"), "").unwrap();
        fs::create_dir_all(r.join("Versions/version-studio")).unwrap();   // no player exe: not a player version
        fs::write(r.join("GlobalBasicSettings_13_Studio.xml"), "<int name=\"FramerateCap\">-1</int>").unwrap();
        fs::write(r.join("GlobalBasicSettings_13.xml"), "<int name=\"FramerateCap\">-1</int>").unwrap();
        fs::create_dir_all(local.join("Bloxstrap")).unwrap();
        local
    }

    fn plan(ultra_low: bool) -> Plan {
        Plan { lowest_graphics: true, fps_cap: Some(40), ultra_low, maximized: false }
    }

    #[test]
    fn finds_the_player_files_not_studio() {
        let local = user("find");
        assert!(basic_settings(&local).unwrap().ends_with("GlobalBasicSettings_13.xml"));
        let flags = flag_files(&local);
        assert_eq!(flags.len(), 2);
        assert!(flags.iter().any(|p| p.to_string_lossy().contains("version-abc")));
        assert!(flags.iter().any(|p| p.to_string_lossy().contains("Bloxstrap")));
    }

    #[test]
    fn apply_then_restore_gets_the_originals_back() {
        let local = user("restore");
        let xml = basic_settings(&local).unwrap();
        assert!(!is_applied(&local, &plan(true)).unwrap());
        assert_eq!(apply(&local, &plan(true)).unwrap(), 3);
        assert!(is_applied(&local, &plan(true)).unwrap());
        assert!(fs::read_to_string(&xml).unwrap().contains(">40<"));
        assert_eq!(apply(&local, &plan(true)).unwrap(), 0);   // already applied: nothing to write
        assert_eq!(restore(&all_files(&local)).unwrap(), 3);
        assert_eq!(fs::read_to_string(&xml).unwrap(), "<int name=\"FramerateCap\">-1</int>");
        assert!(flag_files(&local).iter().all(|p| !p.exists()));   // flag files Potato created are gone again
    }

    #[test]
    fn without_ultra_low_missing_flag_files_stay_missing() {
        let local = user("safe");
        assert_eq!(apply(&local, &plan(false)).unwrap(), 1);
        assert!(flag_files(&local).iter().all(|p| !p.exists()));
    }
}
