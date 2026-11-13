//! Open Maximized. Windows only lets a program move windows in its own login session, so the main app does
//! its own session and a hidden copy of Potato (`Potato.exe --helper`) runs in every other one, RDP logins
//! included. A scheduled task starts that copy whenever any user logs on.

use std::collections::HashMap;
use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HWND, LPARAM};
use windows::Win32::System::Threading::{
    CreateMutexW, OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetWindowThreadProcessId, IsIconic, IsWindowVisible, IsZoomed, SW_MAXIMIZE, ShowWindow,
};
use windows::core::{BOOL, PWSTR, w};

const TASK: &str = "Potato Helper";
/// a new Roblox window is kept maximized this long (Roblox resizes itself while it loads), then left alone
const SETTLE: Duration = Duration::from_secs(30);
const NO_WINDOW: u32 = 0x0800_0000;

/// Remembers when each Roblox window was first seen.
#[derive(Default)]
pub struct Maximizer {
    seen: HashMap<isize, Instant>,
}

impl Maximizer {
    /// Maximize Roblox windows in this session that opened in the last few seconds and aren't maximized.
    /// Minimized windows are skipped, and older windows are never touched, so people can still resize them.
    pub fn tick(&mut self) {
        let mut windows: Vec<isize> = Vec::new();
        unsafe {
            let _ = EnumWindows(Some(collect), LPARAM(&mut windows as *mut _ as isize));
        }
        let now = Instant::now();
        self.seen.retain(|h, _| windows.contains(h));
        for h in windows {
            let first = *self.seen.entry(h).or_insert(now);
            let hwnd = HWND(h as *mut _);
            if now - first < SETTLE && unsafe { !IsZoomed(hwnd).as_bool() && !IsIconic(hwnd).as_bool() } {
                unsafe {
                    let _ = ShowWindow(hwnd, SW_MAXIMIZE);
                }
            }
        }
    }
}

unsafe extern "system" fn collect(hwnd: HWND, out: LPARAM) -> BOOL {
    let out = unsafe { &mut *(out.0 as *mut Vec<isize>) };
    if unsafe { IsWindowVisible(hwnd).as_bool() } && is_roblox(hwnd) {
        out.push(hwnd.0 as isize);
    }
    BOOL(1)
}

fn is_roblox(hwnd: HWND) -> bool {
    let mut pid = 0;
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    let Ok(h) = (unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }) else { return false };
    let mut buf = [0u16; 512];
    let mut len = buf.len() as u32;
    let ok = unsafe { QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len) }.is_ok();
    let _ = unsafe { CloseHandle(h) };
    ok && String::from_utf16_lossy(&buf[..len as usize]).to_ascii_lowercase().ends_with("\\robloxplayerbeta.exe")
}

/// `Potato.exe --helper`: no window; maximizes new Roblox windows in this session while this account has
/// Open Maximized on. Reads the settings every second, so changes in the app reach it without a restart.
pub fn run() {
    // one helper per session ("Local\\" names are per session)
    let mutex = unsafe { CreateMutexW(None, true, w!("Local\\PotatoHelper")) };
    if mutex.is_err() || unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
        return;
    }
    let me = crate::system::current_sid();
    let mut m = Maximizer::default();
    loop {
        let config = std::fs::read_to_string(crate::app::config_path()).map(|t| potato::config::Config::from_json(&t)).unwrap_or_default();
        let a = config.account(&me);
        if a.on && a.plan.maximized {
            m.tick();
        }
        std::thread::sleep(Duration::from_secs(1));
    }
}

/// Where the helper copy lives: a folder every Windows user can read.
fn helper_exe() -> PathBuf {
    crate::app::config_path().with_file_name("Potato.exe")
}

/// Copy Potato next to its settings and register the logon task, then start it in every session that's
/// already logged on. Returns false if Windows refused (needs admin).
pub fn install() -> bool {
    let Ok(me) = std::env::current_exe() else { return false };
    let exe = helper_exe();
    if std::fs::read(&me).ok() != std::fs::read(&exe).ok() {
        let _ = std::fs::create_dir_all(exe.parent().unwrap());
        let _ = std::fs::rename(&exe, exe.with_extension("old"));   // a running helper's exe can be renamed, not overwritten
        if std::fs::copy(&me, &exe).is_err() {
            return false;
        }
        let _ = std::fs::remove_file(exe.with_extension("old"));
    }
    // every member of Users, at their own logon, in their own session, without admin rights
    let xml = format!(r#"<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <Triggers><LogonTrigger><Enabled>true</Enabled></LogonTrigger></Triggers>
  <Principals><Principal id="Users"><GroupId>S-1-5-32-545</GroupId><RunLevel>LeastPrivilege</RunLevel></Principal></Principals>
  <Settings><MultipleInstancesPolicy>Parallel</MultipleInstancesPolicy><DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>
    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries><ExecutionTimeLimit>PT0S</ExecutionTimeLimit><Hidden>true</Hidden></Settings>
  <Actions Context="Users"><Exec><Command>{}</Command><Arguments>--helper</Arguments></Exec></Actions>
</Task>"#, exe.display());
    let file = exe.with_file_name("helper-task.xml");
    let utf16: Vec<u8> = [0xFF, 0xFE].into_iter().chain(xml.encode_utf16().flat_map(|u| u.to_le_bytes())).collect();
    if std::fs::write(&file, utf16).is_err() {
        return false;
    }
    let ok = schtasks(&["/Create", "/TN", TASK, "/XML", &file.to_string_lossy(), "/F"]);
    let _ = std::fs::remove_file(&file);
    ok && schtasks(&["/Run", "/TN", TASK])
}

/// Remove the logon task. Running helpers stay idle until their user logs off.
pub fn uninstall() {
    schtasks(&["/Delete", "/TN", TASK, "/F"]);
}

fn schtasks(args: &[&str]) -> bool {
    Command::new("schtasks").args(args).creation_flags(NO_WINDOW).status().is_ok_and(|s| s.success())
}
