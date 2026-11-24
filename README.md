# AutumnDeck

<p align="center">
  <a href="https://apps.microsoft.com/detail/9P6NTH74WSMD"><img src="https://get.microsoft.com/images/en-us%20dark.svg" alt="Get it from Microsoft Store" height="56" /></a>
</p>

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

### Option 1: Microsoft Store (Recommended — Digitally Signed & Auto-Updating)
[![Download from Microsoft Store](https://get.microsoft.com/images/en-us%20dark.svg)](https://apps.microsoft.com/detail/9P6NTH74WSMD)

- **Official Microsoft Signed Package:** 100% clean install with zero Windows SmartScreen or Smart App Control warnings.
- **Instant 1-Click Install:** Or open `ms-windows-store://pdp/?productid=9P6NTH74WSMD` directly.

---

### Option 2: Standalone Releases (Portable ZIP & Setup Installer — Unsigned)
> [!NOTE]
> Standalone binaries distributed on GitHub are compiled directly from open-source code and are **not digitally signed** (commercial certificates cost \$300+/year). For the **official digitally signed edition with zero security warnings**, please use **[Option 1: Microsoft Store](https://apps.microsoft.com/detail/9P6NTH74WSMD)**.

1. Download from **Releases**:
   - **Portable Edition:** `AutumnDeck-2.0.0-Portable.zip` (extract anywhere and run `AutumnDeck.exe`).
   - **Installer:** `AutumnDeck-Setup-2.0.0.exe`
2. Right-click the **AutumnDeck** tray icon (or press `Ctrl + Alt + T`) to open **AutumnDeck Studio**.
3. Open **`⚙` App Settings** in the top-right header to toggle **Launch at Windows Login** or **Export / Import Setup**.

#### ⚠️ Windows Security & Advanced Boot Troubleshooting
Normally, clicking **"More info" ➔ "Run anyway"** (or unblocking the `.zip`) is all that's required. If your Windows 11 PC has strict security blocking standalone unsigned apps:

1. **Unblock the Download:** Right-click `.zip` or `.exe` ➔ **Properties** ➔ check **Unblock** at the bottom ➔ click **Apply**.
2. **Advanced Boot (Disable Signature Enforcement):**
   - Hold the `Shift` key on your keyboard and click **Start ➔ Power ➔ Restart**.
   - Your PC will reboot to the blue recovery screen. Select:
     **Troubleshoot** ➔ **Advanced options** ➔ **Startup Settings** ➔ click **Restart**.
   - When the list of options appears, press **`7`** (or **`F7`**) on your keyboard for **"Disable driver signature enforcement"**.
   - Windows will boot up allowing all custom desktop mods, hooks, and standalone tools to execute freely.

---

## Community & Sponsorship
If you love AutumnDeck and want to support ongoing development, new presets, and features:

<p align="center">
  <a href="https://ko-fi.com/autumndeck"><img src="https://img.shields.io/badge/Ko--fi-Tip%20a%20Coffee-F16061?style=for-the-badge&logo=ko-fi&logoColor=white" alt="Tip on Ko-fi" /></a>
  <a href="https://www.patreon.com/c/AutumnDeck"><img src="https://img.shields.io/badge/Patreon-Support-FF424D?style=for-the-badge&logo=patreon&logoColor=white" alt="Support on Patreon" /></a>
  <a href="https://github.com/sponsors/Shreyas123g"><img src="https://img.shields.io/badge/GitHub%20Sponsors-Sponsor-EA4AAA?style=for-the-badge&logo=githubsponsors&logoColor=white" alt="GitHub Sponsors" /></a>
</p>

- **Ko-fi (One-time tips):** [ko-fi.com/autumndeck](https://ko-fi.com/autumndeck) — Tip any custom amount once via PayPal or Card.
- **Patreon:** [patreon.com/c/AutumnDeck](https://www.patreon.com/c/AutumnDeck)
- **GitHub Sponsors:** [github.com/sponsors/Shreyas123g](https://github.com/sponsors/Shreyas123g)

---

## Privacy Policy
This program will not transfer any information to other networked systems unless specifically requested by the user or the person installing or operating it (e.g., retrieving weather data from Open-Meteo only when the Weather widget is enabled). AutumnDeck does not collect, track, or share any personal data, analytics, or telemetry. Full details: [PRIVACY.md](PRIVACY.md).

