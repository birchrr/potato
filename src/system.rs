//! Windows calls: Roblox clients in every session, their memory, Windows users, admin rights.

use std::ffi::c_void;
use std::path::PathBuf;

use windows::core::{PCWSTR, PWSTR, w};
use windows::Win32::Foundation::{CloseHandle, FILETIME, HANDLE, HLOCAL, LocalFree, LUID};
use windows::Win32::Security::Authorization::ConvertSidToStringSidW;
use windows::Win32::Security::{
    AdjustTokenPrivileges, GetTokenInformation, LookupAccountSidW, LookupPrivilegeValueW, SE_PRIVILEGE_ENABLED,
    SID_NAME_USE, TOKEN_ADJUST_PRIVILEGES, TOKEN_PRIVILEGES, TOKEN_QUERY, TOKEN_USER, TokenUser,
};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::ProcessStatus::{K32EmptyWorkingSet, K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
use windows::Win32::System::Registry::{
    HKEY, HKEY_LOCAL_MACHINE, KEY_READ, RRF_RT_REG_EXPAND_SZ, RRF_RT_REG_SZ, RegCloseKey, RegEnumKeyExW, RegGetValueW,
    RegOpenKeyExW,
};
use windows::Win32::System::RemoteDesktop::{ProcessIdToSessionId, WTSGetActiveConsoleSessionId};
use windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
use windows::Win32::System::Threading::{
    BELOW_NORMAL_PRIORITY_CLASS, GetActiveProcessorCount, GetCurrentProcess, GetProcessTimes, IDLE_PRIORITY_CLASS,
    NORMAL_PRIORITY_CLASS, PROCESS_TERMINATE, TerminateProcess,
    OpenProcess, OpenProcessToken, PROCESS_POWER_THROTTLING_EXECUTION_SPEED, PROCESS_POWER_THROTTLING_STATE,
    PROCESS_QUERY_INFORMATION, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SET_INFORMATION, PROCESS_SET_QUOTA,
    ProcessPowerThrottling, SetPriorityClass, SetProcessAffinityMask, SetProcessInformation, ALL_PROCESSOR_GROUPS,
};

use crate::config::Priority;
use windows::Win32::UI::Shell::{IsUserAnAdmin, ShellExecuteW};
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

const ROBLOX_EXE: &str = "RobloxPlayerBeta.exe";

/// One running Roblox window, in any session (RDP users included).
#[derive(Clone, Debug)]
pub struct Client {
    pub pid: u32,
    pub session: u32,
    /// the Windows account it runs as, as a SID string; matches `Profile::sid`
    pub sid: String,
    pub user: String,
    pub ram: u64,
    /// when it started (Windows file time), to tell newer clients from older ones
    pub started: u64,
    /// CPU time used so far (100 ns units), for its CPU % between two looks
    pub cpu_time: u64,
}

/// A Windows account on this PC that has Roblox installed.
#[derive(Clone, Debug)]
pub struct Profile {
    pub sid: String,
    pub name: String,
    pub local: PathBuf,
}

pub fn is_admin() -> bool {
    unsafe { IsUserAnAdmin().as_bool() }
}

/// Start Potato again with admin rights (Windows asks first) and let this copy close.
pub fn relaunch_as_admin() -> bool {
    let Ok(exe) = std::env::current_exe() else { return false };
    let exe: Vec<u16> = exe.as_os_str().encode_wide_z();
    let r = unsafe { ShellExecuteW(None, w!("runas"), PCWSTR(exe.as_ptr()), None, None, SW_SHOWNORMAL) };
    r.0 as usize > 32
}

/// Rights an admin has but has to switch on: reaching other users' processes, and emptying the standby list.
pub fn enable_privileges() {
    unsafe {
        let mut token = HANDLE::default();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_ADJUST_PRIVILEGES | TOKEN_QUERY, &mut token).is_err() {
            return;
        }
        for name in [w!("SeDebugPrivilege"), w!("SeProfileSingleProcessPrivilege"), w!("SeIncreaseQuotaPrivilege")] {
            let mut luid = LUID::default();
            if LookupPrivilegeValueW(None, name, &mut luid).is_ok() {
                let mut tp = TOKEN_PRIVILEGES { PrivilegeCount: 1, ..Default::default() };
                tp.Privileges[0].Luid = luid;
                tp.Privileges[0].Attributes = SE_PRIVILEGE_ENABLED;
                let _ = AdjustTokenPrivileges(token, false, Some(&tp), 0, None, None);
            }
        }
        let _ = CloseHandle(token);
    }
}

pub fn clients() -> Vec<Client> {
    let mut out = Vec::new();
    unsafe {
        let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else { return out };
        let mut e = PROCESSENTRY32W { dwSize: size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
        let mut ok = Process32FirstW(snap, &mut e).is_ok();
        while ok {
            let len = e.szExeFile.iter().position(|&c| c == 0).unwrap_or(e.szExeFile.len());
            if String::from_utf16_lossy(&e.szExeFile[..len]).eq_ignore_ascii_case(ROBLOX_EXE) {
                out.push(client(e.th32ProcessID));
            }
            ok = Process32NextW(snap, &mut e).is_ok();
        }
        let _ = CloseHandle(snap);
    }
    out
}

fn client(pid: u32) -> Client {
    let mut session = 0;
    let _ = unsafe { ProcessIdToSessionId(pid, &mut session) };
    let mut c = Client { pid, session, sid: String::new(), user: String::new(), ram: 0, started: 0, cpu_time: 0 };
    unsafe {
        let Ok(h) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_QUERY_INFORMATION, false, pid)
            .or_else(|_| OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid))
        else {
            return c;
        };
        let mut mem = PROCESS_MEMORY_COUNTERS::default();
        if K32GetProcessMemoryInfo(h, &mut mem, size_of::<PROCESS_MEMORY_COUNTERS>() as u32).as_bool() {
            c.ram = mem.WorkingSetSize as u64;
        }
        let (mut created, mut exited, mut kernel, mut user) = (FILETIME::default(), FILETIME::default(), FILETIME::default(), FILETIME::default());
        if GetProcessTimes(h, &mut created, &mut exited, &mut kernel, &mut user).is_ok() {
            let t = |f: FILETIME| (f.dwHighDateTime as u64) << 32 | f.dwLowDateTime as u64;
            c.started = t(created);
            c.cpu_time = t(kernel) + t(user);
        }
        if let Some((sid, user)) = owner(h) {
            c.sid = sid;
            c.user = user;
        }
        let _ = CloseHandle(h);
    }
    c
}

/// The account a process runs as: (SID string, user name).
unsafe fn owner(process: HANDLE) -> Option<(String, String)> {
    let mut token = HANDLE::default();
    unsafe { OpenProcessToken(process, TOKEN_QUERY, &mut token).ok()? };
    let mut buf = vec![0u8; 256];
    let mut len = 0;
    let got = unsafe { GetTokenInformation(token, TokenUser, Some(buf.as_mut_ptr() as *mut c_void), buf.len() as u32, &mut len) };
    let _ = unsafe { CloseHandle(token) };
    got.ok()?;
    let sid = unsafe { (*(buf.as_ptr() as *const TOKEN_USER)).User.Sid };
    let mut text = PWSTR::null();
    unsafe { ConvertSidToStringSidW(sid, &mut text).ok()? };
    let sid_text = unsafe { text.to_string().unwrap_or_default() };
    unsafe { LocalFree(Some(HLOCAL(text.0 as *mut c_void))) };
    let (mut name, mut domain) = ([0u16; 256], [0u16; 256]);
    let (mut nl, mut dl, mut use_) = (256u32, 256u32, SID_NAME_USE::default());
    let user = unsafe {
        LookupAccountSidW(None, sid, Some(PWSTR(name.as_mut_ptr())), &mut nl, Some(PWSTR(domain.as_mut_ptr())), &mut dl, &mut use_)
    }
    .map(|_| String::from_utf16_lossy(&name[..nl as usize]))
    .unwrap_or_default();
    Some((sid_text, user))
}

/// The Windows login session at the PC's own screen; every other session is someone connected over RDP.
pub fn console_session() -> u32 {
    unsafe { WTSGetActiveConsoleSessionId() }
}

/// Hold a client to at most `percent` of the whole PC's CPU, through a Windows job object (100 lifts it).
/// `job` keeps the client's job between calls; the first call makes it and puts the client in it.
pub fn cpu_cap(pid: u32, percent: u32, job: &mut Option<isize>) -> bool {
    use windows::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_CPU_RATE_CONTROL_ENABLE, JOB_OBJECT_CPU_RATE_CONTROL_HARD_CAP,
        JOBOBJECT_CPU_RATE_CONTROL_INFORMATION, JobObjectCpuRateControlInformation, SetInformationJobObject,
    };
    unsafe {
        let handle = match *job {
            Some(h) => HANDLE(h as *mut c_void),
            None => {
                let Ok(j) = CreateJobObjectW(None, None) else { return false };
                let Ok(p) = OpenProcess(PROCESS_SET_QUOTA | PROCESS_TERMINATE, false, pid) else {
                    let _ = CloseHandle(j);
                    return false;
                };
                let ok = AssignProcessToJobObject(j, p).is_ok();
                let _ = CloseHandle(p);
                if !ok {
                    let _ = CloseHandle(j);
                    return false;
                }
                *job = Some(j.0 as isize);
                j
            }
        };
        let mut info = JOBOBJECT_CPU_RATE_CONTROL_INFORMATION {
            ControlFlags: JOB_OBJECT_CPU_RATE_CONTROL_ENABLE | JOB_OBJECT_CPU_RATE_CONTROL_HARD_CAP,
            ..Default::default()
        };
        info.Anonymous.CpuRate = percent.clamp(1, 100) * 100;   // in 1/100 of a percent
        SetInformationJobObject(handle, JobObjectCpuRateControlInformation, &info as *const _ as *const c_void,
                                size_of::<JOBOBJECT_CPU_RATE_CONTROL_INFORMATION>() as u32).is_ok()
    }
}

/// Let go of a client's job (when the client has closed).
pub fn drop_job(job: isize) {
    let _ = unsafe { CloseHandle(HANDLE(job as *mut c_void)) };
}

/// Windows' "Animation effects" setting (Settings, Accessibility, Visual effects).
pub fn animations_on() -> bool {
    use windows::Win32::UI::WindowsAndMessaging::{SPI_GETCLIENTAREAANIMATION, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, SystemParametersInfoW};
    let mut on = windows::core::BOOL(1);
    let _ = unsafe { SystemParametersInfoW(SPI_GETCLIENTAREAANIMATION, 0, Some(&mut on as *mut _ as *mut c_void), SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0)) };
    on.as_bool()
}

/// Minutes since midnight, PC time.
pub fn local_minutes() -> u32 {
    let t = unsafe { windows::Win32::System::SystemInformation::GetLocalTime() };
    t.wHour as u32 * 60 + t.wMinute as u32
}

/// Run a Windows command without a console window; true if it worked.
pub fn run(program: &str, args: &[&str]) -> bool {
    use std::os::windows::process::CommandExt;
    std::process::Command::new(program).args(args).creation_flags(0x0800_0000).status().is_ok_and(|s| s.success())
}

/// Close a Roblox client.
pub fn close(pid: u32) -> bool {
    unsafe {
        let Ok(h) = OpenProcess(PROCESS_TERMINATE, false, pid) else { return false };
        let ok = TerminateProcess(h, 0).is_ok();
        let _ = CloseHandle(h);
        ok
    }
}

/// The SID of the account this copy of Potato runs as.
pub fn current_sid() -> String {
    unsafe { owner(GetCurrentProcess()) }.map(|(sid, _)| sid).unwrap_or_default()
}

/// Move a client's memory out to the page file, like Mem Reduct does. Roblox pulls back what it uses.
pub fn trim(pid: u32) -> bool {
    unsafe {
        let Ok(h) = OpenProcess(PROCESS_SET_QUOTA | PROCESS_QUERY_INFORMATION, false, pid) else { return false };
        let ok = K32EmptyWorkingSet(h).as_bool();
        let _ = CloseHandle(h);
        ok
    }
}

/// Logical cores (threads) on the PC.
pub fn logical_cores() -> usize {
    unsafe { GetActiveProcessorCount(ALL_PROCESSOR_GROUPS) as usize }
}

/// How one client should use the CPU. Off values put it back to Windows' normal.
pub struct Cpu {
    pub priority: Priority,
    /// None: every core
    pub cores: Option<u64>,
    pub efficiency: bool,
}

/// Set a client's priority, cores and Efficiency Mode, like Process Lasso. Returns false if Windows refused.
pub fn set_cpu(pid: u32, cpu: &Cpu) -> bool {
    unsafe {
        let Ok(h) = OpenProcess(PROCESS_SET_INFORMATION | PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else { return false };
        let class = match cpu.priority {
            Priority::Normal => NORMAL_PRIORITY_CLASS,
            Priority::BelowNormal => BELOW_NORMAL_PRIORITY_CLASS,
            Priority::Idle => IDLE_PRIORITY_CLASS,
        };
        let all = if logical_cores() >= 64 { u64::MAX } else { (1u64 << logical_cores()) - 1 };
        let mut ok = SetPriorityClass(h, class).is_ok();
        ok &= SetProcessAffinityMask(h, cpu.cores.unwrap_or(all) as usize).is_ok();
        let state = PROCESS_POWER_THROTTLING_STATE {
            Version: 1,
            ControlMask: PROCESS_POWER_THROTTLING_EXECUTION_SPEED,
            StateMask: if cpu.efficiency { PROCESS_POWER_THROTTLING_EXECUTION_SPEED } else { 0 },
        };
        ok &= SetProcessInformation(h, ProcessPowerThrottling, &state as *const _ as *const c_void,
                                    size_of::<PROCESS_POWER_THROTTLING_STATE>() as u32).is_ok();
        let _ = CloseHandle(h);
        ok
    }
}

#[link(name = "ntdll")]
unsafe extern "system" {
    fn NtSetSystemInformation(class: i32, info: *const c_void, len: u32) -> i32;
    fn NtQueryInformationProcess(process: HANDLE, class: i32, info: *mut c_void, len: u32, written: *mut u32) -> i32;
}

/// The command line a client was started with (the same text Task Manager shows), to find a private server.
pub fn command_line(pid: u32) -> Option<String> {
    const PROCESS_COMMAND_LINE_INFORMATION: i32 = 60;
    #[repr(C)]
    struct UnicodeString {
        length: u16,
        _max: u16,
        buffer: *const u16,
    }
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut len = 0u32;
        let _ = NtQueryInformationProcess(h, PROCESS_COMMAND_LINE_INFORMATION, std::ptr::null_mut(), 0, &mut len);
        let mut buf = vec![0u64; (len as usize).div_ceil(8).max(2)];   // u64s keep the pointer inside aligned
        let ok = len > 0 && NtQueryInformationProcess(h, PROCESS_COMMAND_LINE_INFORMATION, buf.as_mut_ptr() as *mut c_void, len, &mut len) >= 0;
        let _ = CloseHandle(h);
        if !ok {
            return None;
        }
        let s = &*(buf.as_ptr() as *const UnicodeString);
        Some(String::from_utf16_lossy(std::slice::from_raw_parts(s.buffer, s.length as usize / 2)))
    }
}

/// Open a Roblox join link in this session, as this user.
pub fn launch(link: &str) -> bool {
    let link: Vec<u16> = link.encode_utf16().chain([0]).collect();
    unsafe { ShellExecuteW(None, w!("open"), PCWSTR(link.as_ptr()), None, None, SW_SHOWNORMAL) }.0 as usize > 32
}

/// The login session this copy of Potato runs in.
pub fn current_session() -> u32 {
    let mut s = 0;
    let _ = unsafe { ProcessIdToSessionId(std::process::id(), &mut s) };
    s
}

/// Empty Windows' standby list (cached memory nothing uses right now). Needs admin.
pub fn clean_standby() -> bool {
    const SYSTEM_MEMORY_LIST_INFORMATION: i32 = 80;
    const MEMORY_PURGE_STANDBY_LIST: u32 = 4;
    let cmd = MEMORY_PURGE_STANDBY_LIST;
    unsafe { NtSetSystemInformation(SYSTEM_MEMORY_LIST_INFORMATION, &cmd as *const u32 as *const c_void, 4) >= 0 }
}

/// A Windows notification from Potato's tray icon, like Mem Reduct's (Windows shows tray balloons as toasts).
pub fn notify(text: &str) {
    use windows::Win32::UI::Shell::{NIF_INFO, NIIF_LARGE_ICON, NIIF_USER, NIM_MODIFY, NOTIFYICONDATAW, Shell_NotifyIconW};
    use windows::Win32::UI::WindowsAndMessaging::{FindWindowExW, GetWindowThreadProcessId};
    unsafe {
        // the tray-icon crate's hidden window for our icon; shortcut: assumes Potato's one tray icon has id 1
        let mut hwnd = FindWindowExW(None, None, w!("tray_icon_app"), PCWSTR::null());
        while let Ok(h) = hwnd {
            let mut pid = 0;
            GetWindowThreadProcessId(h, Some(&mut pid));
            if pid == std::process::id() {
                break;
            }
            hwnd = FindWindowExW(None, Some(h), w!("tray_icon_app"), PCWSTR::null());
        }
        let Ok(hwnd) = hwnd else { return };
        let mut nid: NOTIFYICONDATAW = std::mem::zeroed();
        nid.cbSize = size_of::<NOTIFYICONDATAW>() as u32;
        nid.hWnd = hwnd;
        nid.uID = 1;
        nid.uFlags = NIF_INFO;
        nid.dwInfoFlags = NIIF_USER | NIIF_LARGE_ICON;   // the potato icon, not a stock one
        for (dst, src) in [(&mut nid.szInfoTitle[..], "Potato"), (&mut nid.szInfo[..], text)] {
            let room = dst.len() - 1;   // the last one stays 0, ending the text
            for (d, c) in dst.iter_mut().take(room).zip(src.encode_utf16()) {
                *d = c;
            }
        }
        let _ = Shell_NotifyIconW(NIM_MODIFY, &nid);
    }
}

/// (in use, total) bytes of RAM.
pub fn memory() -> (u64, u64) {
    let mut m = MEMORYSTATUSEX { dwLength: size_of::<MEMORYSTATUSEX>() as u32, ..Default::default() };
    if unsafe { GlobalMemoryStatusEx(&mut m) }.is_err() {
        return (0, 0);
    }
    (m.ullTotalPhys - m.ullAvailPhys, m.ullTotalPhys)
}

/// Every Windows account on the PC with Roblox installed, from the registry's profile list.
pub fn profiles() -> Vec<Profile> {
    let mut out = Vec::new();
    unsafe {
        let mut key = HKEY::default();
        if RegOpenKeyExW(HKEY_LOCAL_MACHINE, w!("SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion\\ProfileList"), None, KEY_READ, &mut key).is_err() {
            return out;
        }
        for i in 0.. {
            let mut name = [0u16; 256];
            let mut len = 256u32;
            if RegEnumKeyExW(key, i, Some(PWSTR(name.as_mut_ptr())), &mut len, None, None, None, None).is_err() {
                break;
            }
            let sid = String::from_utf16_lossy(&name[..len as usize]);
            if !sid.starts_with("S-1-5-21-") {
                continue;   // system accounts
            }
            let mut buf = [0u16; 520];
            let mut size = (buf.len() * 2) as u32;
            let sub: Vec<u16> = sid.encode_utf16().chain([0]).collect();
            if RegGetValueW(key, PCWSTR(sub.as_ptr()), w!("ProfileImagePath"), RRF_RT_REG_SZ | RRF_RT_REG_EXPAND_SZ,
                            None, Some(buf.as_mut_ptr() as *mut c_void), Some(&mut size)).is_err() {
                continue;
            }
            let home = PathBuf::from(String::from_utf16_lossy(&buf[..(size as usize / 2).saturating_sub(1)]));
            let local = home.join("AppData").join("Local");
            if local.join("Roblox").is_dir() {
                let name = home.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| sid.clone());
                out.push(Profile { sid, name, local });
            }
        }
        let _ = RegCloseKey(key);
    }
    out
}

trait WideZ {
    fn encode_wide_z(&self) -> Vec<u16>;
}

impl WideZ for std::ffi::OsStr {
    fn encode_wide_z(&self) -> Vec<u16> {
        use std::os::windows::ffi::OsStrExt;
        self.encode_wide().chain([0]).collect()
    }
}
