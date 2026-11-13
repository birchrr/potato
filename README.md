<p align="center"><img src="assets/logo.svg" width="128" alt="Potato logo: a pixel potato"></p>

<h1 align="center">Potato</h1>

Run more Roblox on one PC. One small Windows app that puts **every Roblox client on your PC** on low:
lowest graphics, a frame rate cap and trimmed RAM. That includes clients running in other Windows users' **RDP
sessions**, so you can run more alts on the same PC.

It never asks for your Roblox login, never injects into Roblox and never unlocks anything. It only changes
Roblox's own settings files and does what Task Manager and Mem Reduct already do with memory.

## One Setup Per Account

Every Windows account on the PC (each RDP login is one) gets its own settings. Pick a starting point, then change anything:

| | Graphics | FPS | CPU |
| --- | --- | --- | --- |
| **Main** | Lowest | 30 | Normal |
| **Alt** | Lowest | 25 | Lower priority |
| **Ultra Low** | Lowest, plus lowest textures, no grass, no anti-aliasing | 15 | Low priority and Efficiency Mode |

## What It Does

| | |
| --- | --- |
| **Every Client, Every Session** | Lists each Roblox window on the PC: which Windows user runs it, its session and its RAM. |
| **Lowest Graphics And FPS Cap** | Uses Roblox's own Graphics Quality and Maximum Frame Rate settings, for every Windows user you tick. |
| **Keeps Them Applied** | Roblox rewrites its settings when it closes. Potato puts yours back the moment it does, before anything relaunches it. Roblox updates are covered too. |
| **Open Maximized** | Every Roblox window opens filling the screen, title bar kept, in every RDP login. You can still resize it after. |
| **Close Extra Clients** | Keeps only the newest Roblox window(s) per account and closes older stuck ones, like the pile left after an internet outage. |
| **Trim RAM** | Moves memory Roblox isn't using right now to the page file, every few minutes or on a click. |
| **CPU Like Process Lasso** | Lower the clients' priority, give each client its own CPU cores, and turn on Windows 11 Efficiency Mode. None of it changes how the game looks. |
| **Clean Memory** | Empties Windows' standby list when RAM gets full, like Mem Reduct. |
| **Update Banner** | Tells you when a new version is on GitHub. |
| **Restore Originals** | Every file Potato changes gets a backup first. One click puts them all back. |

## Install

1. Download `Potato.exe` from the [latest release](https://github.com/birchrr/potato/releases/latest).
2. Run it. Windows may warn because the app isn't signed: **More Info**, then **Run Anyway**.
3. Say yes when Windows asks for admin. Potato needs it to reach other accounts' Roblox (RDP logins) and clean memory.

Run it on the main account. Leave it open: it re-applies settings and trims memory while it runs.
Its settings live in `C:\ProgramData\Potato\settings.json`.

Needs Windows 10 or 11, x64.

## How It Works

- **Settings:** each user's `AppData\Local\Roblox\GlobalBasicSettings_*.xml` (`SavedQualityLevel`,
  `GraphicsQualityLevel`, `FramerateCap`). Potato only edits values already in the file, and only while that
  user's Roblox is closed, since Roblox writes the file when it exits.
- **Extra Cuts:** `ClientAppSettings.json` Fast Flags in each Roblox version folder, plus Bloxstrap's and Fishstrap's
  `Modifications` folder. Only flags on Roblox's
  [allowlist](https://devforum.roblox.com/t/allowlist-for-local-client-configuration-via-fast-flags/3966569),
  so Roblox actually reads them. Your own flags stay.
- **Open Maximized:** turns Roblox's own Fullscreen setting off (that one hides the title bar), then maximizes each new Roblox
  window. Windows only lets a program move windows in its own login, so Potato copies itself to `C:\ProgramData\Potato` and adds a
  hidden logon task that runs `Potato.exe --helper` in every login.
- **Users:** every Windows account with Roblox installed, from the registry's profile list. Clients are matched
  to their account by the user they run as.
- **CPU:** `SetPriorityClass`, `SetProcessAffinityMask` and power throttling (Efficiency Mode) on each client. Clients are
  numbered oldest first, so each keeps its cores while others open and close.
- **Memory:** `EmptyWorkingSet` on each Roblox client and `NtSetSystemInformation` to purge the standby list.

## Build

The logo lives in `assets/logo.txt` (a pixel map). After changing it, run `python scripts/icons.py` to redraw the icon files.


```powershell
cargo test --lib
cargo build --release
```

The settings logic is plain Rust and its tests run on any OS. Off Windows the app builds against fake accounts
(`src/system_demo.rs`), so `cargo test --test window -- --ignored` draws the real window to `target/window-*.png`
(needs a GPU or a software Vulkan driver like lavapipe).

## License

MIT. Not affiliated with Roblox Corporation. Geist font by Vercel, under the SIL Open Font License (`assets/fonts/OFL.txt`).
