<p align="center"><img src="assets/logo.svg" width="128" alt="Potato logo: a pixel potato"></p>

<h1 align="center">Potato</h1>

Run more Roblox on one PC. One small Windows app that puts **every Roblox client on your PC** on low:
lowest graphics, a frame rate cap and trimmed RAM. That includes clients running in other Windows users' **RDP
sessions**, so you can run more alts on the same PC.

It never asks for your Roblox login, never injects into Roblox and never unlocks anything. It only changes
Roblox's own settings files and does what Task Manager and Mem Reduct already do with memory.

## Safe With Natro Macro

Macros like [Natro Macro](https://github.com/NatroTeam/NatroMacro) read the screen, so a tool that changes how the
game looks can break them. **Macro Safe** mode (on by default) only changes what Natro asks for itself:

- Graphics set to the lowest manual level, the exact value Natro checks for
- A frame rate cap of 25 or more (Natro warns below 25)
- Camera, shift lock, text size and everything else left alone

Turn Macro Safe off to also get **Extra Cuts**: lowest textures, no grass, no anti-aliasing.

## What It Does

| | |
| --- | --- |
| **Every Client, Every Session** | Lists each Roblox window on the PC: which Windows user runs it, its session and its RAM. |
| **Lowest Graphics And FPS Cap** | Uses Roblox's own Graphics Quality and Maximum Frame Rate settings, for every Windows user you tick. |
| **Keeps Them Applied** | Roblox rewrites its settings when it closes. Potato puts yours back the moment it does, before a macro relaunches it. Roblox updates are covered too. |
| **Trim RAM** | Moves memory Roblox isn't using right now to the page file, every few minutes or on a click. |
| **CPU Like Process Lasso** | Lower the clients' priority, give each client its own CPU cores, and turn on Windows 11 Efficiency Mode. None of it changes how the game looks. |
| **Clean Memory** | Empties Windows' standby list when RAM gets full, like Mem Reduct. |
| **Restore Originals** | Every file Potato changes gets a backup first. One click puts them all back. |

## Install

1. Download `Potato.exe` from the [latest release](https://github.com/birchrr/potato/releases/latest).
2. Run it. Windows may warn because the app isn't signed: **More Info**, then **Run Anyway**.
3. Click **Restart As Admin** to reach other users' Roblox (RDP sessions) and to use Clean Memory.

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

The settings logic is plain Rust and its tests run on any OS. The window and the Windows calls only build on Windows.

## License

MIT. Not affiliated with Roblox Corporation or Natro Macro.
