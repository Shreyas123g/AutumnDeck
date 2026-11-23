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
   - **Installer:** `AutumnDeck-Setup-2.0.0.exe`
   - **Portable Edition:** `AutumnDeck-2.0.0-Portable.zip` (extract all files together and run `AutumnDeck.exe`)
2. Right-click the **AutumnDeck** tray icon (or press `Ctrl + Alt + T`) to open **AutumnDeck Studio**.
3. Open **`⚙` App Settings** in the top-right header to toggle **Launch at Windows Login** or **Export / Import Setup**.

---

## Code Signing Policy
Free code signing provided by [SignPath.io](https://signpath.io), certificate by [SignPath Foundation](https://signpath.org).

- **Authors & Committers:** [@Shreyas123g](https://github.com/Shreyas123g)
- **Reviewers & Approvers:** [@Shreyas123g](https://github.com/Shreyas123g)

---

## Privacy Policy
This program will not transfer any information to other networked systems unless specifically requested by the user or the person installing or operating it (e.g., retrieving weather data from Open-Meteo only when the Weather widget is enabled). AutumnDeck does not collect, track, or share any personal data, analytics, or telemetry. Full details: [PRIVACY.md](PRIVACY.md).

