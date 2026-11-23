# AutumnDeck

**Native Windows 11 Taskbar Styler, Interactive PBR Docks, Modular Desktop Widgets & Live Wallpaper Suite**

Built from scratch in **Rust + Win32 + Direct2D + DirectComposition** for ultra-low memory footprint and zero Chromium/Electron bloat.

---

## Features
- **Native Windows 11 Taskbar Styler:** Blur, Acrylic, Clear, Split Pill, and Katana Smoked Glass via lightweight XAML TAP composition.
- **Interactive & PBR Realistic Docks:** Smooth macOS-style magnification, live app previews, tray overflow, and 12 baked PBR surface materials (Carbon, Titanium, Walnut, Silk, Marble, and more).
- **Modular Desktop Widgets + Pen & Canvas 2.0:**
  - Clock & Script Date
  - Dynamic Island Capsule Music Player & Turntable with Live Visualizer
  - Celestial Sun-Dial Weather Dashboard (7-day Open-Meteo + auto IP geolocation)
  - Telemetry Ring-Pill Vitals HUD
  - Control Center, Notes & To-Do, Pomodoro, Quick Apps
  - Full per-element Move, Scale, Rotate, Font Picker, and Live Chameleon wallpaper color matching
- **Live Wallpaper Engine:** Hardware-accelerated DXGI Flip-Model SwapChain for `.mp4`/`.mkv`/`.webm` video loops and high-res `.png`/`.jpg`/`.webp` static wallpapers with instant fullscreen/game auto-pause.
- **30 Curated Suites & Portable Backup:** Instant Suite Gallery switching plus 1-click **Export / Import Setup (`.autumn`)** under App Settings (`⚙`).

---

## Requirements
- **Windows 11 64-bit** (22H2 / 23H2 / 24H2 / 26H2) recommended for native XAML taskbar styling
- **Windows 10 64-bit** (2004+) supported for Virtual Docks, Desktop Widgets, and Live Wallpapers

---

## Download & Installation
1. Download the latest release from **Releases**:
   - **Portable Edition (Recommended for Pre-releases):** `AutumnDeck-2.0.0-Portable.zip` (extract all files together, right-click `AutumnDeck.exe` -> Properties -> Unblock if present, and run).
   - **Installer:** `AutumnDeck-Setup-2.0.0.exe`
2. Right-click the **AutumnDeck** tray icon (or press `Ctrl + Alt + T`) to open **AutumnDeck Studio**.
3. Open **`⚙` App Settings** in the top-right header to toggle **Launch at Windows Login** or **Export / Import Setup**.

### Windows 11 Smart App Control & Code Signing Notice
> [!NOTE]
> **Code Signing In-Progress:** AutumnDeck has officially applied for free open-source code signing provided by [SignPath.io](https://signpath.io) (certificate by [SignPath Foundation](https://signpath.org)).
>
> On Windows 11 22H2 / 24H2+ machines with **Smart App Control (SAC)** or **Hypervisor Code Integrity (HVCI)** active, Windows blocks unsigned installers running from `%TEMP%` (`Error 4551: Application Control policy`) or unsigned DLL injection into `explorer.exe`.
>
> Once SignPath Foundation issues our official certificate, all releases will be signed automatically via GitHub Actions, and AutumnDeck will install and run natively on standard boot with zero warnings.

#### How to Test Pre-Release Builds (Prior to Official Signing)
If you are testing current unsigned pre-release builds on a Windows 11 PC with strict Smart App Control or HVCI enabled:

1. **Use Portable Edition (`.zip`):**
   - Download `AutumnDeck-2.0.0-Portable.zip`.
   - Right-click the `.zip` ➔ **Properties** ➔ check **Unblock** (if present) ➔ click **Apply**.
   - Extract the folder to your preferred location (e.g., `Downloads` or `Desktop`).

2. **If Windows 11 Blocks Unsigned DLL Injection:**
   - Hold down the **Shift** key while clicking **Restart** in the Windows Start menu (or run `shutdown /r /o /t 0` in PowerShell).
   - Navigate to: **Troubleshoot** ➔ **Advanced options** ➔ **Startup Settings** ➔ click **Restart**.
   - On the Startup Settings screen, press **`7`** or **`F7`** to select **Disable driver signature enforcement** (this temporarily suspends HVCI / unsigned injection restrictions for that test session).

3. **Verify Taskbar Styling:**
   - Double-click `AutumnDeck.exe`.
   - Choose any preset in **Taskbar Studio** (e.g., *Cosmic Gravity*, *Split Pill*, or *Katana Glass*).
   - Restart `explorer.exe` from Task Manager — styling will remain locked and persistent without reverting.

---

## Code Signing Policy
Free code signing provided by [SignPath.io](https://signpath.io), certificate by [SignPath Foundation](https://signpath.org).

- **Authors & Committers:** [@Shreyas123g](https://github.com/Shreyas123g)
- **Reviewers & Approvers:** [@Shreyas123g](https://github.com/Shreyas123g)

---

## Privacy Policy
This program will not transfer any information to other networked systems unless specifically requested by the user or the person installing or operating it (e.g., retrieving weather data from Open-Meteo only when the Weather widget is enabled). AutumnDeck does not collect, track, or share any personal data, analytics, or telemetry. Full details: [PRIVACY.md](PRIVACY.md).

