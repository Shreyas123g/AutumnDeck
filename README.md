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

### Windows 11 Smart App Control & Code Signing
AutumnDeck binaries are signed with an official project Authenticode certificate (`AutumnDeck.cer`).

On Windows 11 (22H2 / 24H2+) with **Smart App Control (SAC)** or strict security policies enabled:
1. **Portable Edition (Recommended):**
   - Download and extract `AutumnDeck-2.0.0-Portable.zip`.
   - Right-click `Install-Certificate.bat` and select **Run as administrator** once.
   - This registers the AutumnDeck certificate into your PC's **Trusted Publishers** store so Windows 11 natively trusts AutumnDeck and allows taskbar styling without warnings or requiring Advanced Boot.
2. Double-click `AutumnDeck.exe` and enjoy!

---

## Community & Sponsorship
If you love AutumnDeck and want to support ongoing development, new presets, and features:
- **Patreon:** [patreon.com/c/AutumnDeck](https://www.patreon.com/c/AutumnDeck)
- **GitHub Sponsors:** [github.com/sponsors/Shreyas123g](https://github.com/sponsors/Shreyas123g)

---

## Privacy Policy
This program will not transfer any information to other networked systems unless specifically requested by the user or the person installing or operating it (e.g., retrieving weather data from Open-Meteo only when the Weather widget is enabled). AutumnDeck does not collect, track, or share any personal data, analytics, or telemetry. Full details: [PRIVACY.md](PRIVACY.md).

