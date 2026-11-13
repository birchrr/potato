//! Open Maximized. Windows only lets a program move windows in its own login session, so the main app does
//! its own session and a hidden copy of Potato (`Potato.exe --helper`) runs in every other one, RDP logins
//! included. A scheduled task starts that copy whenever any user logs on.

use std::collections::{HashMap, HashSet};
use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HWND, LPARAM};
use windows::Win32::System::Threading::{
    CreateMutexW, OpenMutexW, OpenProcess, PROCESS_NAME_WIN32, SYNCHRONIZATION_SYNCHRONIZE, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetClientRect, GetWindowThreadProcessId, IsHungAppWindow, IsIconic, IsWindowVisible, IsZoomed, SW_MAXIMIZE, ShowWindow,
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
    let mut jobs = SessionJobs::default();
    loop {
        let config = std::fs::read_to_string(crate::engine::config_path()).map(|t| crate::config::Config::from_json(&t)).unwrap_or_default();
        let a = config.account(&me);
        if a.on && a.plan.maximized {
            m.tick();
        }
        session_tick(&me, &mut jobs);
        std::thread::sleep(Duration::from_secs(1));
    }
}

/// What only code inside a login can do for that login's Roblox: rejoins, muting, spotting freezes and
/// taking screenshots for the Discord report. Run every second by the helper (or by the app for its own login).
#[derive(Default)]
pub struct SessionJobs {
    rejoined: u64,
    muted: HashSet<u32>,
    hung: HashSet<isize>,
    freezes: u32,
    shot_done: u64,
}

/// The folder where the helpers leave freeze counts and screenshots for the report (see engine.rs).
pub fn report_dir() -> PathBuf {
    crate::engine::config_path().with_file_name("report")
}

pub fn session_tick(me: &str, jobs: &mut SessionJobs) {
    serve_rejoins(me, &mut jobs.rejoined);
    let config = std::fs::read_to_string(crate::engine::config_path()).map(|t| crate::config::Config::from_json(&t)).unwrap_or_default();
    let a = config.account(me);
    mute(a.on && a.mute, &mut jobs.muted);

    let mut windows: Vec<isize> = Vec::new();
    unsafe {
        let _ = EnumWindows(Some(collect), LPARAM(&mut windows as *mut _ as isize));
    }
    let hung: HashSet<isize> = windows.iter().copied().filter(|h| unsafe { IsHungAppWindow(HWND(*h as *mut _)).as_bool() }).collect();
    let before = jobs.freezes;
    jobs.freezes += hung.difference(&jobs.hung).count() as u32;   // each window counts once per freeze
    jobs.hung = hung;

    let dir = report_dir();
    let asked = std::fs::read_to_string(dir.join("request.txt")).ok().and_then(|t| t.trim().parse::<u64>().ok()).unwrap_or(0);
    let mut shots = None;
    if asked > jobs.shot_done {
        jobs.shot_done = asked;
        let _ = std::fs::create_dir_all(&dir);
        shots = Some(windows.iter().enumerate().filter(|(i, h)| capture(**h, &dir.join(format!("{me}-{i}.png")))).count());
    }
    if shots.is_some() || jobs.freezes != before {
        let _ = std::fs::create_dir_all(&dir);
        let json = serde_json::json!({ "id": jobs.shot_done, "freezes": jobs.freezes, "frozenNow": jobs.hung.len(), "shots": shots });
        let _ = std::fs::write(dir.join(format!("{me}.json")), json.to_string());
    }
}

/// Mute (or unmute what Potato muted) every Roblox audio session in this login.
fn mute(on: bool, muted: &mut HashSet<u32>) {
    use windows::Win32::Media::Audio::{
        IAudioSessionControl2, IAudioSessionManager2, IMMDeviceEnumerator, ISimpleAudioVolume, MMDeviceEnumerator, eConsole, eRender,
    };
    use windows::Win32::System::Com::{CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx};
    use windows::core::Interface;
    if !on && muted.is_empty() {
        return;
    }
    let roblox: HashSet<u32> = crate::system::clients().iter().filter(|c| c.session == crate::system::current_session()).map(|c| c.pid).collect();
    let _ = (|| -> windows::core::Result<()> {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            let devices: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
            let manager: IAudioSessionManager2 = devices.GetDefaultAudioEndpoint(eRender, eConsole)?.Activate(CLSCTX_ALL, None)?;
            let sessions = manager.GetSessionEnumerator()?;
            for i in 0..sessions.GetCount()? {
                let session = sessions.GetSession(i)?;
                let pid = session.cast::<IAudioSessionControl2>()?.GetProcessId()?;
                let volume = session.cast::<ISimpleAudioVolume>()?;
                if on && roblox.contains(&pid) && !muted.contains(&pid) {
                    volume.SetMute(true, std::ptr::null())?;
                    muted.insert(pid);
                } else if !on && muted.remove(&pid) {
                    volume.SetMute(false, std::ptr::null())?;
                }
            }
        }
        Ok(())
    })();
    muted.retain(|pid| roblox.contains(pid));
}

/// Save a Roblox window as a PNG, at most 960 px wide. Works while it's covered; a minimized window, or one in
/// a disconnected RDP login, has nothing drawn and is skipped.
fn capture(hwnd: isize, path: &std::path::Path) -> bool {
    use windows::Win32::Foundation::RECT;
    use windows::Win32::Graphics::Gdi::{
        BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CreateCompatibleBitmap, CreateCompatibleDC, DIB_RGB_COLORS, DeleteDC, DeleteObject,
        GetDC, GetDIBits, ReleaseDC, SelectObject,
    };
    use windows::Win32::Storage::Xps::{PRINT_WINDOW_FLAGS, PrintWindow};
    let h = HWND(hwnd as *mut _);
    let mut r = RECT::default();
    if unsafe { IsIconic(h).as_bool() || GetClientRect(h, &mut r).is_err() } {
        return false;
    }
    let (w, ht) = (r.right - r.left, r.bottom - r.top);
    if w < 50 || ht < 50 {
        return false;
    }
    let mut pixels = vec![0u8; (w * ht * 4) as usize];
    let ok = unsafe {
        let screen = GetDC(None);
        let dc = CreateCompatibleDC(Some(screen));
        let bmp = CreateCompatibleBitmap(screen, w, ht);
        let old = SelectObject(dc, bmp.into());
        let drawn = PrintWindow(h, dc, PRINT_WINDOW_FLAGS(3)).as_bool();   // client area, full content (works for DirectX)
        let mut info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER { biSize: size_of::<BITMAPINFOHEADER>() as u32, biWidth: w, biHeight: -ht, biPlanes: 1, biBitCount: 32, biCompression: BI_RGB.0, ..Default::default() },
            ..Default::default()
        };
        let got = GetDIBits(dc, bmp, 0, ht as u32, Some(pixels.as_mut_ptr() as *mut _), &mut info, DIB_RGB_COLORS);
        SelectObject(dc, old);
        let _ = DeleteObject(bmp.into());
        let _ = DeleteDC(dc);
        ReleaseDC(None, screen);
        drawn && got == ht
    };
    if !ok || pixels.chunks(4).all(|p| p[..3] == [0, 0, 0]) {
        return false;   // nothing drawn
    }
    let step = (w as usize).div_ceil(960).max(1);
    let (ow, oh) = ((w as usize).div_ceil(step), (ht as usize).div_ceil(step));
    let mut out = Vec::with_capacity(ow * oh * 3);
    for y in (0..ht as usize).step_by(step) {
        for x in (0..w as usize).step_by(step) {
            let p = (y * w as usize + x) * 4;
            out.extend_from_slice(&[pixels[p + 2], pixels[p + 1], pixels[p]]);   // BGRA -> RGB
        }
    }
    let Ok(file) = std::fs::File::create(path) else { return false };
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), ow as u32, oh as u32);
    enc.set_color(png::ColorType::Rgb);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header().and_then(|mut w| w.write_image_data(&out)).is_ok()
}

/// The folder where the app leaves rejoin requests, one file per account (see rejoin.rs).
pub fn rejoin_dir() -> PathBuf {
    crate::engine::config_path().with_file_name("rejoin")
}

/// If the app asked this account's clients to rejoin: wait for them to close, then open each join link, a few
/// seconds apart so Roblox isn't starting two at once. `done` is the last request handled, so each runs once.
pub fn serve_rejoins(me: &str, done: &mut u64) {
    let Ok(text) = std::fs::read_to_string(rejoin_dir().join(format!("{me}.txt"))) else { return };
    let Some((id, links)) = crate::rejoin::read_request(&text) else { return };
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64);
    if id == *done || now.saturating_sub(id) > 10 * 60 * 1000 {
        return;   // already handled, or left over from long ago
    }
    *done = id;
    let session = crate::system::current_session();
    let start = Instant::now();
    while crate::system::clients().iter().any(|c| c.session == session) && start.elapsed() < Duration::from_secs(30) {
        std::thread::sleep(Duration::from_millis(500));
    }
    for (i, link) in links.iter().enumerate() {
        if i > 0 {
            std::thread::sleep(Duration::from_secs(10));
        }
        crate::system::launch(link);
    }
}

/// Whether a helper is already running in this session (it then handles this account's rejoins).
pub fn running_here() -> bool {
    unsafe { OpenMutexW(SYNCHRONIZATION_SYNCHRONIZE, false, w!("Local\\PotatoHelper")) }.is_ok_and(|h| {
        let _ = unsafe { CloseHandle(h) };
        true
    })
}

/// Where the helper copy lives: a folder every Windows user can read.
fn helper_exe() -> PathBuf {
    crate::engine::config_path().with_file_name("Potato.exe")
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
