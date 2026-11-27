# AutumnDeck

<p align="center">
  <a href="https://apps.microsoft.com/detail/9P6NTH74WSMD"><img src="https://get.microsoft.com/images/en-us%20dark.svg" alt="Get it from Microsoft Store" height="52" /></a>
  &nbsp;
  <a href="https://alternativeto.net/software/autumndeck/"><img src="https://img.shields.io/badge/AlternativeTo-AutumnDeck-007ACC?style=for-the-badge" alt="AutumnDeck on AlternativeTo" height="36" /></a>
</p>

<p align="center">
  <strong>Native Windows 11 Taskbar Styler, Interactive PBR Docks, Modular Desktop Widgets &amp; Live Wallpaper Suite</strong><br/>
  Built from scratch in <strong>Rust + Win32 + Direct2D + DirectComposition</strong> for near-zero memory footprint (&lt;50 MB RAM) and zero Chromium/Electron bloat.
</p>

<p align="center">
  <img src="assets/screenshots/showcase-alpine-glass.jpg" alt="AutumnDeck Alpine Glass Suite on Windows 11" width="100%" />
</p>

---

## Showcase

<table>
  <tr>
    <td align="center" width="50%">
      <img src="assets/screenshots/showcase-marble-sanctuary.jpg" alt="Marble Sanctuary Suite" width="100%" /><br/>
      <sub><b>Marble Sanctuary</b> — Editorial clock, calendar, quote bar &amp; LP sleeve player</sub>
    </td>
    <td align="center" width="50%">
      <img src="assets/screenshots/showcase-aero-carbon.jpg" alt="Aero-Carbon PBR Suite" width="100%" /><br/>
      <sub><b>Aero-Carbon</b> — Carbon fiber split dock, vitals HUD &amp; Quick Settings</sub>
    </td>
  </tr>
  <tr>
    <td align="center" width="50%">
      <img src="assets/screenshots/studio-taskbar-themes.png" alt="AutumnDeck Studio Taskbar Folders" width="100%" /><br/>
      <sub><b>Taskbar Studio</b> — 90 curated taskbar &amp; dock themes across 8 styles</sub>
    </td>
    <td align="center" width="50%">
      <img src="assets/screenshots/studio-music-players.png" alt="AutumnDeck Studio Music Players" width="100%" /><br/>
      <sub><b>Music Players</b> — 29 reactive themes (12AX7 tubes, turntables, VU meters)</sub>
    </td>
  </tr>
</table>

---

## Features
- **Native Windows 11 Taskbar Styler:** Blur, Acrylic, Clear, Split Pill, and Katana Smoked Glass via lightweight XAML TAP composition.
- **Interactive & PBR Realistic Docks:** 90 curated themes including **20 Clean & Minimal** macOS Liquid Glass / Windhawk themes (Paired Top Menu Bar with Control Center & Calendar Popovers, easy **Top Menu Bar: ON/OFF** toggle in Themes & Dock context menus, Desktop-Only mode, Smart Titlebar Dodge, Pure Bottom Dock) plus **32–96 px Dock Icon Scaling**, **10–20 px Inline App Label Font Scaling**, **32 pinned apps** with drag-and-drop pinning & drag-to-reorder, locale-independent **Show Hidden Icons (`^`)**, and 12 baked PBR surface materials.
- **Modular Desktop Widgets + Pen & Canvas 2.0:**
  - Clock & Script Date
  - Dynamic Island Capsule Music Player & Turntable with Live Visualizer
  - Celestial Sun-Dial Weather Dashboard (7-day Open-Meteo + auto IP geolocation)
  - Telemetry Ring-Pill Vitals HUD
  - Control Center, Notes & To-Do, Pomodoro, Quick Apps & Extended Utility Suite (19 widgets total)
  - **10 Photorealistic Haute Horlogerie Themes** across all 19 widgets with distinct physical silhouettes, machined bezels, dial textures, and per-part material overrides
  - Full per-element Move, Scale, Rotate, Font Picker, **Reset Style vs Discard**, and Live Chameleon wallpaper color matching
- **Live Wallpaper Engine & Performance Mode:** Hardware-accelerated DXGI Flip-Model SwapChain (triple-buffered with non-low-latency B-frame lookahead, **Auto / 60 / 30 FPS** cap, and async Chameleon GPU readback) for `.mp4`/`.mkv`/`.webm` video loops and `.png`/`.jpg`/`.webp` static wallpapers, background non-blocking telemetry worker, offscreen widget caching, and **Performance Mode (`Smooth` / `Balanced Auto-Eco` / `Eco`)**.
- **72 Curated Suites & True Portable Mode:** 8 curated Suite Gallery folders (including **18 Photorealistic Horology** flagship setups) with 1-click switching, **Export / Import Setup (`.autumn`)**, and **True Portable Mode** (`portable.txt` / `--portable` storing everything inside `.\Data\`) under App Settings (`⚙`).

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

### Option 2: Standalone Releases (Portable ZIP & Setup Installer)
> [!NOTE]
> Free code signing provided by [SignPath.io](https://signpath.io), certificate by [SignPath Foundation](https://signpath.org).
> Official digitally signed store edition is also available via **[Option 1: Microsoft Store](https://apps.microsoft.com/detail/9P6NTH74WSMD)**.

1. Download from **[GitHub Releases](https://github.com/Shreyas123g/AutumnDeck/releases)**:
   - **Portable Edition:** `AutumnDeck-2.1.0-Portable.zip` (extract anywhere and run `AutumnDeck.exe` — includes `portable.txt` to store settings inside `.\Data\`).
   - **Installer:** `AutumnDeck-Setup-2.1.0.exe`
   - **MSIX Package:** `AutumnDeck_2.1.0.0_x64.msix`
2. Right-click the **AutumnDeck** tray icon (or press `Ctrl + Alt + T`) to open **AutumnDeck Studio**.
3. Open **`⚙` App Settings** in the top-right header to toggle **Launch at Windows Login** or **Export / Import Setup**.

#### Code Signing & Security
Free code signing provided by [SignPath.io](https://signpath.io), certificate by [SignPath Foundation](https://signpath.org). All binaries are compiled transparently from open-source Rust code with zero telemetry and zero third-party bundled software. Full privacy details: [PRIVACY.md](PRIVACY.md).

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

