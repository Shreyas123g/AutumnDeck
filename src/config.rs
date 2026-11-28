use std::ffi::c_void;
use std::fs;
use std::io::Write;
use std::mem::size_of;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use windows::core::{w, Interface, PCWSTR, PWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, PROPERTYKEY, RECT, WPARAM};
use windows::Win32::Storage::FileSystem::{
    MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
};
use windows::Win32::System::Com::CoTaskMemFree;
use windows::Win32::System::Com::StructuredStorage::{PropVariantClear, PropVariantToStringAlloc};
use windows::Win32::System::LibraryLoader::GetModuleFileNameW;
use windows::Win32::System::Registry::{
    RegCloseKey, RegCreateKeyExW, RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW,
    RegSetValueExW, HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ, KEY_WRITE, REG_BINARY,
    REG_DWORD, REG_OPTION_NON_VOLATILE, REG_SZ, REG_VALUE_TYPE,
};
use windows::Win32::UI::Controls::Dialogs::{
    GetOpenFileNameW, GetSaveFileNameW, OFN_FILEMUSTEXIST, OFN_NOCHANGEDIR, OFN_OVERWRITEPROMPT,
    OFN_PATHMUSTEXIST, OPENFILENAMEW,
};
use windows::Win32::UI::Shell::Common::ITEMIDLIST;
use windows::Win32::UI::Shell::PropertiesSystem::{
    IPropertyStore, SHGetPropertyStoreFromIDList, GPS_DEFAULT,
};
use windows::Win32::UI::Shell::{
    SHAppBarMessage, SHGetNameFromIDList, ShellExecuteW, APPBARDATA, SIGDN_DESKTOPABSOLUTEPARSING,
    SIGDN_NORMALDISPLAY, SIGDN_PARENTRELATIVEPARSING,
};
use windows::Win32::UI::WindowsAndMessaging::{
    FindWindowW, SendNotifyMessageW, HWND_BROADCAST, SMTO_ABORTIFHUNG, SW_SHOWNORMAL,
    WM_SETTINGCHANGE,
};

use crate::widgets_extra::{
    default_ext_widgets, default_notes_items, default_world_clock_cities, ExtWidgetConfig,
    ExtWidgetId, EXT_WIDGET_COUNT,
};

const ABM_GETSTATE_CMD: u32 = 4;
const ABS_AUTOHIDE_FLAG: isize = 1;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TaskbarMode {
    SmartMaximize,
    HoverReveal,
    StealthHide,
    AlwaysVisible,
}

impl TaskbarMode {
    pub fn as_str(self) -> &'static str {
        match self {
            TaskbarMode::SmartMaximize => "smart",
            TaskbarMode::HoverReveal => "hover",
            TaskbarMode::StealthHide => "stealth",
            TaskbarMode::AlwaysVisible => "always",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            TaskbarMode::SmartMaximize => "Smart Auto-Hide (Desktop Visible • Fullscreen on Maximize)",
            TaskbarMode::HoverReveal => "Edge Hover Peek (100% Screen • Left-Edge Reveal)",
            TaskbarMode::StealthHide => "Full Stealth Hide (Toggle via Ctrl+Alt+T)",
            TaskbarMode::AlwaysVisible => "Always Visible (Docked Crystal Taskbar)",
        }
    }

    pub fn short_label(self) -> &'static str {
        match self {
            TaskbarMode::SmartMaximize => "Smart Auto-Hide",
            TaskbarMode::HoverReveal => "Edge Hover Peek",
            TaskbarMode::StealthHide => "Full Stealth",
            TaskbarMode::AlwaysVisible => "Always Visible",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "hover" | "edge" => TaskbarMode::HoverReveal,
            "stealth" | "hidden" | "hide" => TaskbarMode::StealthHide,
            "always" | "dock" | "visible" => TaskbarMode::AlwaysVisible,
            _ => TaskbarMode::SmartMaximize,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MacDockEdge {
    Bottom,
    Top,
    Left,
    Right,
}

impl MacDockEdge {
    pub fn as_str(self) -> &'static str {
        match self {
            MacDockEdge::Bottom => "bottom",
            MacDockEdge::Top => "top",
            MacDockEdge::Left => "left",
            MacDockEdge::Right => "right",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            MacDockEdge::Bottom => "Bottom Edge",
            MacDockEdge::Top => "Top Edge",
            MacDockEdge::Left => "Left Edge",
            MacDockEdge::Right => "Right Edge",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "top" => MacDockEdge::Top,
            "left" => MacDockEdge::Left,
            "right" => MacDockEdge::Right,
            _ => MacDockEdge::Bottom,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MacDockHideMode {
    SmartAutoHide,
    HoverReveal,
    AlwaysVisible,
    StealthHide,
}

impl MacDockHideMode {
    pub fn as_str(self) -> &'static str {
        match self {
            MacDockHideMode::SmartAutoHide => "smart",
            MacDockHideMode::HoverReveal => "hover",
            MacDockHideMode::AlwaysVisible => "always",
            MacDockHideMode::StealthHide => "stealth",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "hover" | "edge" => MacDockHideMode::HoverReveal,
            "always" | "visible" => MacDockHideMode::AlwaysVisible,
            "stealth" | "hidden" | "hide" => MacDockHideMode::StealthHide,
            _ => MacDockHideMode::SmartAutoHide,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PillMode {
    SplitPills,
    UnifiedPill,
    FullBar,
    NativeDefault,
}

impl PillMode {
    pub fn as_str(self) -> &'static str {
        match self {
            PillMode::SplitPills => "split",
            PillMode::UnifiedPill => "unified",
            PillMode::FullBar => "full",
            PillMode::NativeDefault => "native",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            PillMode::SplitPills => "Split Pills",
            PillMode::UnifiedPill => "Unified Pill",
            PillMode::FullBar => "Full Bar",
            PillMode::NativeDefault => "Default",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "unified" => PillMode::UnifiedPill,
            "full" => PillMode::FullBar,
            "native" | "default" | "off" => PillMode::NativeDefault,
            _ => PillMode::SplitPills,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SurfaceMode {
    ClearTint,
    Acrylic,
    Blur,
    SystemDefault,
}

impl SurfaceMode {
    pub fn as_str(self) -> &'static str {
        match self {
            SurfaceMode::ClearTint => "clear",
            SurfaceMode::Acrylic => "acrylic",
            SurfaceMode::Blur => "blur",
            SurfaceMode::SystemDefault => "default",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            SurfaceMode::ClearTint => "Clear Glass",
            SurfaceMode::Acrylic => "Acrylic Blur",
            SurfaceMode::Blur => "Frost Blur",
            SurfaceMode::SystemDefault => "System",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "acrylic" => SurfaceMode::Acrylic,
            "blur" => SurfaceMode::Blur,
            "default" | "off" | "native" | "system" => SurfaceMode::SystemDefault,
            _ => SurfaceMode::ClearTint,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TaskbarVizStyle {
    Off,
    EqualizerBars,
    AuroraWave,
    EdgePulse,
    AmbientGlow,
    AuroraFlow,
    CyberScan,
    EmberDrift,
    ChromeSweep,
    StarlightPulse,
    HoverSnow,
    HoverFire,
    HoverWater,
    HoverSakura,
    HoverCosmic,
    HoverPlasma,
    HoverMacWave,
    HoverMercury,
    HoverHydroBubbles,
    HoverQuantum,
    HoverBiolum,
    HoverSynthGrid,
}

impl TaskbarVizStyle {
    pub const ALL: [TaskbarVizStyle; 12] = [
        TaskbarVizStyle::Off,
        TaskbarVizStyle::EqualizerBars,
        TaskbarVizStyle::AuroraWave,
        TaskbarVizStyle::EdgePulse,
        TaskbarVizStyle::AmbientGlow,
        TaskbarVizStyle::AuroraFlow,
        TaskbarVizStyle::HoverSnow,
        TaskbarVizStyle::HoverFire,
        TaskbarVizStyle::HoverWater,
        TaskbarVizStyle::HoverSakura,
        TaskbarVizStyle::HoverCosmic,
        TaskbarVizStyle::HoverPlasma,
    ];

    pub const KINETIC_PAGE: [TaskbarVizStyle; 12] = [
        TaskbarVizStyle::Off,
        TaskbarVizStyle::HoverMacWave,
        TaskbarVizStyle::HoverMercury,
        TaskbarVizStyle::HoverHydroBubbles,
        TaskbarVizStyle::HoverQuantum,
        TaskbarVizStyle::HoverBiolum,
        TaskbarVizStyle::HoverSynthGrid,
        TaskbarVizStyle::CyberScan,
        TaskbarVizStyle::EmberDrift,
        TaskbarVizStyle::ChromeSweep,
        TaskbarVizStyle::StarlightPulse,
        TaskbarVizStyle::AuroraFlow,
    ];

    pub fn fx_page(page: usize) -> &'static [TaskbarVizStyle; 12] {
        if page % 2 == 1 {
            &Self::KINETIC_PAGE
        } else {
            &Self::ALL
        }
    }

    pub const WIDGET_HOVER_FX: [TaskbarVizStyle; 7] = [
        TaskbarVizStyle::Off,
        TaskbarVizStyle::HoverSnow,
        TaskbarVizStyle::HoverFire,
        TaskbarVizStyle::HoverWater,
        TaskbarVizStyle::HoverSakura,
        TaskbarVizStyle::HoverCosmic,
        TaskbarVizStyle::HoverPlasma,
    ];

    pub fn fx_short_label(self) -> &'static str {
        match self {
            TaskbarVizStyle::Off => "Off",
            TaskbarVizStyle::HoverSnow => "Snow",
            TaskbarVizStyle::HoverFire => "Fire",
            TaskbarVizStyle::HoverWater => "Water",
            TaskbarVizStyle::HoverSakura => "Sakura",
            TaskbarVizStyle::HoverCosmic => "Cosmic",
            TaskbarVizStyle::HoverPlasma => "Plasma",
            _ => self.label(),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            TaskbarVizStyle::Off => "off",
            TaskbarVizStyle::EqualizerBars => "bars",
            TaskbarVizStyle::AuroraWave => "wave",
            TaskbarVizStyle::EdgePulse => "pulse",
            TaskbarVizStyle::AmbientGlow => "ambient",
            TaskbarVizStyle::AuroraFlow => "aurora_flow",
            TaskbarVizStyle::CyberScan => "cyber_scan",
            TaskbarVizStyle::EmberDrift => "ember_drift",
            TaskbarVizStyle::ChromeSweep => "chrome_sweep",
            TaskbarVizStyle::StarlightPulse => "starlight",
            TaskbarVizStyle::HoverSnow => "hover_snow",
            TaskbarVizStyle::HoverFire => "hover_fire",
            TaskbarVizStyle::HoverWater => "hover_water",
            TaskbarVizStyle::HoverSakura => "hover_sakura",
            TaskbarVizStyle::HoverCosmic => "hover_cosmic",
            TaskbarVizStyle::HoverPlasma => "hover_plasma",
            TaskbarVizStyle::HoverMacWave => "hover_mac_wave",
            TaskbarVizStyle::HoverMercury => "hover_mercury",
            TaskbarVizStyle::HoverHydroBubbles => "hover_bubbles",
            TaskbarVizStyle::HoverQuantum => "hover_quantum",
            TaskbarVizStyle::HoverBiolum => "hover_biolum",
            TaskbarVizStyle::HoverSynthGrid => "hover_synth",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            TaskbarVizStyle::Off => "Off",
            TaskbarVizStyle::EqualizerBars => "EQ Bars",
            TaskbarVizStyle::AuroraWave => "Fluid Wave",
            TaskbarVizStyle::EdgePulse => "Edge Glow",
            TaskbarVizStyle::AmbientGlow => "Ambient",
            TaskbarVizStyle::AuroraFlow => "Aurora",
            TaskbarVizStyle::CyberScan => "Cyber HUD",
            TaskbarVizStyle::EmberDrift => "Embers",
            TaskbarVizStyle::ChromeSweep => "Chrome",
            TaskbarVizStyle::StarlightPulse => "Starlight",
            TaskbarVizStyle::HoverSnow => "Snowfall",
            TaskbarVizStyle::HoverFire => "Inferno",
            TaskbarVizStyle::HoverWater => "Water",
            TaskbarVizStyle::HoverSakura => "Sakura",
            TaskbarVizStyle::HoverCosmic => "Cosmic",
            TaskbarVizStyle::HoverPlasma => "Plasma",
            TaskbarVizStyle::HoverMacWave => "Mac Wave",
            TaskbarVizStyle::HoverMercury => "Mercury",
            TaskbarVizStyle::HoverHydroBubbles => "Bubbles",
            TaskbarVizStyle::HoverQuantum => "Quantum",
            TaskbarVizStyle::HoverBiolum => "Biolum",
            TaskbarVizStyle::HoverSynthGrid => "Grid Warp",
        }
    }

    pub fn is_interactive(self) -> bool {
        matches!(
            self,
            TaskbarVizStyle::HoverSnow
                | TaskbarVizStyle::HoverFire
                | TaskbarVizStyle::HoverWater
                | TaskbarVizStyle::HoverSakura
                | TaskbarVizStyle::HoverCosmic
                | TaskbarVizStyle::HoverPlasma
                | TaskbarVizStyle::HoverMacWave
                | TaskbarVizStyle::HoverMercury
                | TaskbarVizStyle::HoverHydroBubbles
                | TaskbarVizStyle::HoverQuantum
                | TaskbarVizStyle::HoverBiolum
                | TaskbarVizStyle::HoverSynthGrid
        )
    }

    pub fn is_kinetic_page(self) -> bool {
        matches!(
            self,
            TaskbarVizStyle::HoverMacWave
                | TaskbarVizStyle::HoverMercury
                | TaskbarVizStyle::HoverHydroBubbles
                | TaskbarVizStyle::HoverQuantum
                | TaskbarVizStyle::HoverBiolum
                | TaskbarVizStyle::HoverSynthGrid
        )
    }

    pub fn is_ambient_live(self) -> bool {
        self.is_interactive()
            || matches!(
                self,
                TaskbarVizStyle::AuroraFlow
                    | TaskbarVizStyle::CyberScan
                    | TaskbarVizStyle::EmberDrift
                    | TaskbarVizStyle::ChromeSweep
                    | TaskbarVizStyle::StarlightPulse
                    | TaskbarVizStyle::AmbientGlow
            )
    }

    pub fn from_str(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "bars" | "eq" | "equalizer" => TaskbarVizStyle::EqualizerBars,
            "wave" | "aurora" => TaskbarVizStyle::AuroraWave,
            "pulse" | "edge" => TaskbarVizStyle::EdgePulse,
            "ambient" | "glow" => TaskbarVizStyle::AmbientGlow,
            "aurora_flow" | "silk" => TaskbarVizStyle::AuroraFlow,
            "cyber_scan" | "cyber" | "hud" => TaskbarVizStyle::CyberScan,
            "ember_drift" | "ember" | "embers" => TaskbarVizStyle::EmberDrift,
            "chrome_sweep" | "chrome" => TaskbarVizStyle::ChromeSweep,
            "starlight" | "stars" => TaskbarVizStyle::StarlightPulse,
            "hover_snow" | "snow" | "snowfall" => TaskbarVizStyle::HoverSnow,
            "hover_fire" | "fire" | "inferno" => TaskbarVizStyle::HoverFire,
            "hover_water" | "water" | "ripple" => TaskbarVizStyle::HoverWater,
            "hover_sakura" | "sakura" | "petals" => TaskbarVizStyle::HoverSakura,
            "hover_cosmic" | "cosmic" | "stardust" => TaskbarVizStyle::HoverCosmic,
            "hover_plasma" | "plasma" => TaskbarVizStyle::HoverPlasma,
            "hover_mac_wave" | "mac_wave" => TaskbarVizStyle::HoverMacWave,
            "hover_mercury" | "mercury" => TaskbarVizStyle::HoverMercury,
            "hover_bubbles" | "bubbles" => TaskbarVizStyle::HoverHydroBubbles,
            "hover_quantum" | "quantum" => TaskbarVizStyle::HoverQuantum,
            "hover_biolum" | "biolum" => TaskbarVizStyle::HoverBiolum,
            "hover_synth" | "synth" | "grid_warp" => TaskbarVizStyle::HoverSynthGrid,
            _ => TaskbarVizStyle::Off,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VizColorMode {
    ThemeNative,
    CustomAccent,
    MatchWallpaper,
}

impl VizColorMode {
    pub const SWATCHES: [(u32, &'static str); 8] = [
        (0x00E8FF, "Cyber Cyan"),
        (0xFF38A8, "Synth Magenta"),
        (0x8CFF3A, "Electric Lime"),
        (0x7A86FF, "Vivid Indigo"),
        (0xFF7A28, "Solar Orange"),
        (0x34E86C, "Studio Emerald"),
        (0xFF3B5C, "Crimson Laser"),
        (0xE8F6FF, "Frost Ice"),
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            VizColorMode::ThemeNative => "native",
            VizColorMode::CustomAccent => "custom",
            VizColorMode::MatchWallpaper => "wallpaper",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            VizColorMode::ThemeNative => "Native",
            VizColorMode::CustomAccent => "Custom",
            VizColorMode::MatchWallpaper => "Wallpaper",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "custom" | "accent" | "custom_accent" => VizColorMode::CustomAccent,
            "wallpaper" | "match_wallpaper" | "chameleon" => VizColorMode::MatchWallpaper,
            _ => VizColorMode::ThemeNative,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TaskbarThemeFolder {
    CleanMinimal,
    RealisticMaterials,
    InteractiveDocks,
    OsDocks,
    KineticThemes,
    InteractiveElements,
    LiveThemes,
    MusicThemes,
    All,
}

impl TaskbarThemeFolder {
    #[allow(dead_code)]
    pub const ALL: [TaskbarThemeFolder; 9] = [
        TaskbarThemeFolder::CleanMinimal,
        TaskbarThemeFolder::RealisticMaterials,
        TaskbarThemeFolder::InteractiveDocks,
        TaskbarThemeFolder::OsDocks,
        TaskbarThemeFolder::KineticThemes,
        TaskbarThemeFolder::InteractiveElements,
        TaskbarThemeFolder::LiveThemes,
        TaskbarThemeFolder::MusicThemes,
        TaskbarThemeFolder::All,
    ];

    pub const CURATED: [TaskbarThemeFolder; 8] = [
        TaskbarThemeFolder::CleanMinimal,
        TaskbarThemeFolder::RealisticMaterials,
        TaskbarThemeFolder::InteractiveDocks,
        TaskbarThemeFolder::OsDocks,
        TaskbarThemeFolder::KineticThemes,
        TaskbarThemeFolder::InteractiveElements,
        TaskbarThemeFolder::LiveThemes,
        TaskbarThemeFolder::MusicThemes,
    ];

    pub fn label(self) -> &'static str {
        match self {
            TaskbarThemeFolder::CleanMinimal => "Clean & Minimal (20)",
            TaskbarThemeFolder::RealisticMaterials => "Realistic Themes (18)",
            TaskbarThemeFolder::InteractiveDocks => "Interactive Docks (20)",
            TaskbarThemeFolder::OsDocks => "OS & Iconic Docks (6)",
            TaskbarThemeFolder::KineticThemes => "Kinetic Interactive (6)",
            TaskbarThemeFolder::InteractiveElements => "Interactive Elements (6)",
            TaskbarThemeFolder::LiveThemes => "Live & Crystal (8)",
            TaskbarThemeFolder::MusicThemes => "Music Themes (6)",
            TaskbarThemeFolder::All => "All Themes (90)",
        }
    }

    pub fn card_meta(self) -> (&'static str, &'static str, TaskbarTheme) {
        match self {
            TaskbarThemeFolder::CleanMinimal => (
                "Clean & Minimal",
                "Animated OS26 Liquid Glass, Matter Tiles, Bubbles, One UI & DockLike",
                TaskbarTheme::Os26LiquidGlassLight,
            ),
            TaskbarThemeFolder::RealisticMaterials => (
                "Realistic & Calm",
                "Live Water, Zen Mist, Brushed Metal, Hi-Fi, Marble & Wood",
                TaskbarTheme::RealisticWaterDock,
            ),
            TaskbarThemeFolder::InteractiveDocks => (
                "Interactive Docks",
                "Split Islands, Orbs & HUDs",
                TaskbarTheme::AuroraRibbonDock,
            ),
            TaskbarThemeFolder::OsDocks => (
                "OS & Iconic Docks",
                "Cupertino, Aero 7 & Retro 95",
                TaskbarTheme::CupertinoDock,
            ),
            TaskbarThemeFolder::KineticThemes => (
                "Kinetic Interactive",
                "Spring Wave, Mercury & 3D Grid",
                TaskbarTheme::KineticCupertinoWave,
            ),
            TaskbarThemeFolder::InteractiveElements => (
                "Interactive Physics",
                "Snow, Fire, Water, Sakura & Plasma",
                TaskbarTheme::HoverInferno,
            ),
            TaskbarThemeFolder::LiveThemes => (
                "Live & Crystal",
                "Aurora, Cyber HUD, Sunset & Glass",
                TaskbarTheme::CyberNeonDock,
            ),
            TaskbarThemeFolder::MusicThemes => (
                "Music Themes",
                "Live EQ & audio waves",
                TaskbarTheme::LiveMusicBars,
            ),
            TaskbarThemeFolder::All => (
                "All Taskbar Themes",
                "Browse all Clean, Realistic, Docks, Kinetic, Elemental & Music",
                TaskbarTheme::Os26LiquidGlassLight,
            ),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TaskbarTheme {
    // Clean & Minimal (20 authentic Windhawk-inspired native Windows 11 taskbar themes)
    Os26LiquidGlassLight,
    Os26LiquidGlassDark,
    MatterTilesLight,
    MatterTilesDark,
    OneUiLightChips,
    OneUiDarkChips,
    SquircleFrosted,
    BubblesDark,
    BubblesLight,
    PillsDetached,
    RosePineTiles,
    CatppuccinTiles,
    LiquidGlassClear,
    DockLikeLight,
    DockLikeDark,
    BlobLight,
    LuminosityFloat,
    BorderlessGhost,
    AerisGlassLight,
    CleanSlateCards,
    // Realistic Materials, Water & Calm Themes (18 photorealistic Direct2D PBR docks)
    BrushedTitaniumDock,
    VacuumTubeHiFiDock,
    SwissHorologyDock,
    SaddleLeatherDock,
    ObsidianCarbonDock,
    VintageCassetteDock,
    RealisticWaterDock,
    RainWindowDock,
    OceanTideDock,
    ZenCalmMistDock,
    CalmKoiPondDock,
    SunlitForestCanopyDock,
    CarraraMarbleDock,
    WarmWalnutDeskDock,
    FrostedGlacierIceDock,
    SilkLinenCalmDock,
    AbyssalTrenchDock,
    GoldenHourHorizonDock,
    // Folder 0: Interactive Virtual Shell Docks (19 artistic standalone docks)
    QuartzShellDock,
    CyberpunkHudDock,
    DynamicCapsuleDock,
    FrutigerAquaDock,
    InvisibleGhostDock,
    AuroraRibbonDock,
    NextSlatePlankDock,
    KumikoWoodDock,
    LiquidChromeDock,
    SakuraBreezeDock,
    SolsticeAstrolabeDock,
    VaporwaveHorizonDock,
    EditorialPaperDock,
    BiolumAbyssDock,
    TerminalPhosphorDock,
    PrismRefractionDock,
    ObsidianEmberDock,
    BauhausGeometricDock,
    StarfieldWarpDock,
    KatanaGlassDock,
    // Folder 1: OS & Iconic Docks (6 Windhawk-style OS transformations)
    CupertinoDock,
    DynamicIslandDock,
    FrutigerAero7,
    UnixPolybar,
    CyberArasakaDeck,
    Retro95Classic,
    // Folder 2: Kinetic Interactive (6 high-animation hover physics themes)
    KineticCupertinoWave,
    KineticLiquidMercury,
    KineticHydroChamber,
    KineticQuantumCollider,
    KineticBiolumAbyss,
    KineticSynthTrampoline,
    // Folder 3: Interactive Hover Physics Elements (6 elemental themes)
    HoverSnowfall,
    HoverInferno,
    HoverWaterRipples,
    HoverSakuraWind,
    HoverNebulaStardust,
    HoverNeonPlasma,
    // Folder 4: Live Taskbar & Crystal Themes (8)
    AutumnCrystal,
    WallpaperGlass,
    FrostedSplit,
    AuroraBorealis,
    CyberNeonDock,
    SunsetEmber,
    LiquidChrome,
    StarlightMinimal,
    // Folder 5: Music Visualizer Themes (6 music themes)
    LiveMusicBars,
    NeonWave,
    PureClearPulse,
    MirrorBeatPill,
    BassPulseSplit,
    VuPeakStudio,
}

impl TaskbarTheme {
    pub const ALL: [TaskbarTheme; 90] = [
        TaskbarTheme::Os26LiquidGlassLight,
        TaskbarTheme::Os26LiquidGlassDark,
        TaskbarTheme::MatterTilesLight,
        TaskbarTheme::MatterTilesDark,
        TaskbarTheme::OneUiLightChips,
        TaskbarTheme::OneUiDarkChips,
        TaskbarTheme::SquircleFrosted,
        TaskbarTheme::BubblesDark,
        TaskbarTheme::BubblesLight,
        TaskbarTheme::PillsDetached,
        TaskbarTheme::RosePineTiles,
        TaskbarTheme::CatppuccinTiles,
        TaskbarTheme::LiquidGlassClear,
        TaskbarTheme::DockLikeLight,
        TaskbarTheme::DockLikeDark,
        TaskbarTheme::BlobLight,
        TaskbarTheme::LuminosityFloat,
        TaskbarTheme::BorderlessGhost,
        TaskbarTheme::AerisGlassLight,
        TaskbarTheme::CleanSlateCards,
        TaskbarTheme::RealisticWaterDock,
        TaskbarTheme::RainWindowDock,
        TaskbarTheme::BrushedTitaniumDock,
        TaskbarTheme::WarmWalnutDeskDock,
        TaskbarTheme::AbyssalTrenchDock,
        TaskbarTheme::FrostedGlacierIceDock,
        TaskbarTheme::SaddleLeatherDock,
        TaskbarTheme::SunlitForestCanopyDock,
        TaskbarTheme::ObsidianCarbonDock,
        TaskbarTheme::GoldenHourHorizonDock,
        TaskbarTheme::ZenCalmMistDock,
        TaskbarTheme::VacuumTubeHiFiDock,
        TaskbarTheme::SwissHorologyDock,
        TaskbarTheme::VintageCassetteDock,
        TaskbarTheme::OceanTideDock,
        TaskbarTheme::CalmKoiPondDock,
        TaskbarTheme::CarraraMarbleDock,
        TaskbarTheme::SilkLinenCalmDock,
        TaskbarTheme::QuartzShellDock,
        TaskbarTheme::CyberpunkHudDock,
        TaskbarTheme::DynamicCapsuleDock,
        TaskbarTheme::FrutigerAquaDock,
        TaskbarTheme::InvisibleGhostDock,
        TaskbarTheme::AuroraRibbonDock,
        TaskbarTheme::NextSlatePlankDock,
        TaskbarTheme::KumikoWoodDock,
        TaskbarTheme::LiquidChromeDock,
        TaskbarTheme::SakuraBreezeDock,
        TaskbarTheme::SolsticeAstrolabeDock,
        TaskbarTheme::VaporwaveHorizonDock,
        TaskbarTheme::EditorialPaperDock,
        TaskbarTheme::BiolumAbyssDock,
        TaskbarTheme::TerminalPhosphorDock,
        TaskbarTheme::PrismRefractionDock,
        TaskbarTheme::ObsidianEmberDock,
        TaskbarTheme::BauhausGeometricDock,
        TaskbarTheme::StarfieldWarpDock,
        TaskbarTheme::KatanaGlassDock,
        TaskbarTheme::CupertinoDock,
        TaskbarTheme::DynamicIslandDock,
        TaskbarTheme::FrutigerAero7,
        TaskbarTheme::UnixPolybar,
        TaskbarTheme::CyberArasakaDeck,
        TaskbarTheme::Retro95Classic,
        TaskbarTheme::KineticCupertinoWave,
        TaskbarTheme::KineticLiquidMercury,
        TaskbarTheme::KineticHydroChamber,
        TaskbarTheme::KineticQuantumCollider,
        TaskbarTheme::KineticBiolumAbyss,
        TaskbarTheme::KineticSynthTrampoline,
        TaskbarTheme::HoverSnowfall,
        TaskbarTheme::HoverInferno,
        TaskbarTheme::HoverWaterRipples,
        TaskbarTheme::HoverSakuraWind,
        TaskbarTheme::HoverNebulaStardust,
        TaskbarTheme::HoverNeonPlasma,
        TaskbarTheme::AutumnCrystal,
        TaskbarTheme::WallpaperGlass,
        TaskbarTheme::FrostedSplit,
        TaskbarTheme::AuroraBorealis,
        TaskbarTheme::CyberNeonDock,
        TaskbarTheme::SunsetEmber,
        TaskbarTheme::LiquidChrome,
        TaskbarTheme::StarlightMinimal,
        TaskbarTheme::LiveMusicBars,
        TaskbarTheme::NeonWave,
        TaskbarTheme::PureClearPulse,
        TaskbarTheme::MirrorBeatPill,
        TaskbarTheme::BassPulseSplit,
        TaskbarTheme::VuPeakStudio,
    ];

    pub const CLEAN_MINIMAL_FOLDER: [TaskbarTheme; 20] = [
        TaskbarTheme::Os26LiquidGlassLight,
        TaskbarTheme::Os26LiquidGlassDark,
        TaskbarTheme::MatterTilesLight,
        TaskbarTheme::MatterTilesDark,
        TaskbarTheme::OneUiLightChips,
        TaskbarTheme::OneUiDarkChips,
        TaskbarTheme::SquircleFrosted,
        TaskbarTheme::BubblesDark,
        TaskbarTheme::BubblesLight,
        TaskbarTheme::PillsDetached,
        TaskbarTheme::RosePineTiles,
        TaskbarTheme::CatppuccinTiles,
        TaskbarTheme::LiquidGlassClear,
        TaskbarTheme::DockLikeLight,
        TaskbarTheme::DockLikeDark,
        TaskbarTheme::BlobLight,
        TaskbarTheme::LuminosityFloat,
        TaskbarTheme::BorderlessGhost,
        TaskbarTheme::AerisGlassLight,
        TaskbarTheme::CleanSlateCards,
    ];

    pub const REALISTIC_FOLDER: [TaskbarTheme; 18] = [
        TaskbarTheme::RealisticWaterDock,
        TaskbarTheme::RainWindowDock,
        TaskbarTheme::BrushedTitaniumDock,
        TaskbarTheme::WarmWalnutDeskDock,
        TaskbarTheme::AbyssalTrenchDock,
        TaskbarTheme::FrostedGlacierIceDock,
        TaskbarTheme::SaddleLeatherDock,
        TaskbarTheme::SunlitForestCanopyDock,
        TaskbarTheme::ObsidianCarbonDock,
        TaskbarTheme::GoldenHourHorizonDock,
        TaskbarTheme::ZenCalmMistDock,
        TaskbarTheme::VacuumTubeHiFiDock,
        TaskbarTheme::SwissHorologyDock,
        TaskbarTheme::VintageCassetteDock,
        TaskbarTheme::OceanTideDock,
        TaskbarTheme::CalmKoiPondDock,
        TaskbarTheme::CarraraMarbleDock,
        TaskbarTheme::SilkLinenCalmDock,
    ];

    pub const VIRTUAL_DOCK_THEMES: [TaskbarTheme; 58] = [
        TaskbarTheme::Os26LiquidGlassLight,
        TaskbarTheme::Os26LiquidGlassDark,
        TaskbarTheme::MatterTilesLight,
        TaskbarTheme::MatterTilesDark,
        TaskbarTheme::OneUiLightChips,
        TaskbarTheme::OneUiDarkChips,
        TaskbarTheme::SquircleFrosted,
        TaskbarTheme::BubblesDark,
        TaskbarTheme::BubblesLight,
        TaskbarTheme::PillsDetached,
        TaskbarTheme::RosePineTiles,
        TaskbarTheme::CatppuccinTiles,
        TaskbarTheme::LiquidGlassClear,
        TaskbarTheme::DockLikeLight,
        TaskbarTheme::DockLikeDark,
        TaskbarTheme::BlobLight,
        TaskbarTheme::LuminosityFloat,
        TaskbarTheme::BorderlessGhost,
        TaskbarTheme::AerisGlassLight,
        TaskbarTheme::CleanSlateCards,
        TaskbarTheme::RealisticWaterDock,
        TaskbarTheme::RainWindowDock,
        TaskbarTheme::BrushedTitaniumDock,
        TaskbarTheme::WarmWalnutDeskDock,
        TaskbarTheme::AbyssalTrenchDock,
        TaskbarTheme::FrostedGlacierIceDock,
        TaskbarTheme::SaddleLeatherDock,
        TaskbarTheme::SunlitForestCanopyDock,
        TaskbarTheme::ObsidianCarbonDock,
        TaskbarTheme::GoldenHourHorizonDock,
        TaskbarTheme::ZenCalmMistDock,
        TaskbarTheme::VacuumTubeHiFiDock,
        TaskbarTheme::SwissHorologyDock,
        TaskbarTheme::VintageCassetteDock,
        TaskbarTheme::OceanTideDock,
        TaskbarTheme::CalmKoiPondDock,
        TaskbarTheme::CarraraMarbleDock,
        TaskbarTheme::SilkLinenCalmDock,
        TaskbarTheme::QuartzShellDock,
        TaskbarTheme::CyberpunkHudDock,
        TaskbarTheme::DynamicCapsuleDock,
        TaskbarTheme::FrutigerAquaDock,
        TaskbarTheme::InvisibleGhostDock,
        TaskbarTheme::AuroraRibbonDock,
        TaskbarTheme::NextSlatePlankDock,
        TaskbarTheme::KumikoWoodDock,
        TaskbarTheme::LiquidChromeDock,
        TaskbarTheme::SakuraBreezeDock,
        TaskbarTheme::SolsticeAstrolabeDock,
        TaskbarTheme::VaporwaveHorizonDock,
        TaskbarTheme::EditorialPaperDock,
        TaskbarTheme::BiolumAbyssDock,
        TaskbarTheme::TerminalPhosphorDock,
        TaskbarTheme::PrismRefractionDock,
        TaskbarTheme::ObsidianEmberDock,
        TaskbarTheme::BauhausGeometricDock,
        TaskbarTheme::StarfieldWarpDock,
        TaskbarTheme::KatanaGlassDock,
    ];

    pub const INTERACTIVE_DOCKS_FOLDER: [TaskbarTheme; 20] = [
        TaskbarTheme::QuartzShellDock,
        TaskbarTheme::CyberpunkHudDock,
        TaskbarTheme::DynamicCapsuleDock,
        TaskbarTheme::FrutigerAquaDock,
        TaskbarTheme::InvisibleGhostDock,
        TaskbarTheme::AuroraRibbonDock,
        TaskbarTheme::NextSlatePlankDock,
        TaskbarTheme::KumikoWoodDock,
        TaskbarTheme::LiquidChromeDock,
        TaskbarTheme::SakuraBreezeDock,
        TaskbarTheme::SolsticeAstrolabeDock,
        TaskbarTheme::VaporwaveHorizonDock,
        TaskbarTheme::EditorialPaperDock,
        TaskbarTheme::BiolumAbyssDock,
        TaskbarTheme::TerminalPhosphorDock,
        TaskbarTheme::PrismRefractionDock,
        TaskbarTheme::ObsidianEmberDock,
        TaskbarTheme::BauhausGeometricDock,
        TaskbarTheme::StarfieldWarpDock,
        TaskbarTheme::KatanaGlassDock,
    ];

    pub const OS_DOCKS_FOLDER: [TaskbarTheme; 6] = [
        TaskbarTheme::CupertinoDock,
        TaskbarTheme::DynamicIslandDock,
        TaskbarTheme::FrutigerAero7,
        TaskbarTheme::UnixPolybar,
        TaskbarTheme::CyberArasakaDeck,
        TaskbarTheme::Retro95Classic,
    ];

    pub const KINETIC_FOLDER: [TaskbarTheme; 6] = [
        TaskbarTheme::KineticCupertinoWave,
        TaskbarTheme::KineticLiquidMercury,
        TaskbarTheme::KineticHydroChamber,
        TaskbarTheme::KineticQuantumCollider,
        TaskbarTheme::KineticBiolumAbyss,
        TaskbarTheme::KineticSynthTrampoline,
    ];

    pub const INTERACTIVE_FOLDER: [TaskbarTheme; 6] = [
        TaskbarTheme::HoverSnowfall,
        TaskbarTheme::HoverInferno,
        TaskbarTheme::HoverWaterRipples,
        TaskbarTheme::HoverSakuraWind,
        TaskbarTheme::HoverNebulaStardust,
        TaskbarTheme::HoverNeonPlasma,
    ];

    pub const LIVE_FOLDER: [TaskbarTheme; 8] = [
        TaskbarTheme::AutumnCrystal,
        TaskbarTheme::WallpaperGlass,
        TaskbarTheme::FrostedSplit,
        TaskbarTheme::AuroraBorealis,
        TaskbarTheme::CyberNeonDock,
        TaskbarTheme::SunsetEmber,
        TaskbarTheme::LiquidChrome,
        TaskbarTheme::StarlightMinimal,
    ];

    pub const MUSIC_FOLDER: [TaskbarTheme; 6] = [
        TaskbarTheme::LiveMusicBars,
        TaskbarTheme::NeonWave,
        TaskbarTheme::PureClearPulse,
        TaskbarTheme::MirrorBeatPill,
        TaskbarTheme::BassPulseSplit,
        TaskbarTheme::VuPeakStudio,
    ];

    pub fn in_folder(folder: TaskbarThemeFolder) -> &'static [TaskbarTheme] {
        match folder {
            TaskbarThemeFolder::CleanMinimal => &Self::CLEAN_MINIMAL_FOLDER,
            TaskbarThemeFolder::RealisticMaterials => &Self::REALISTIC_FOLDER,
            TaskbarThemeFolder::InteractiveDocks => &Self::INTERACTIVE_DOCKS_FOLDER,
            TaskbarThemeFolder::OsDocks => &Self::OS_DOCKS_FOLDER,
            TaskbarThemeFolder::KineticThemes => &Self::KINETIC_FOLDER,
            TaskbarThemeFolder::InteractiveElements => &Self::INTERACTIVE_FOLDER,
            TaskbarThemeFolder::LiveThemes => &Self::LIVE_FOLDER,
            TaskbarThemeFolder::MusicThemes => &Self::MUSIC_FOLDER,
            TaskbarThemeFolder::All => &Self::ALL,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            TaskbarTheme::Os26LiquidGlassLight => "wh_os26_liquid_glass_light",
            TaskbarTheme::Os26LiquidGlassDark => "wh_os26_liquid_glass_dark",
            TaskbarTheme::MatterTilesLight => "wh_matter_tiles_light",
            TaskbarTheme::MatterTilesDark => "wh_matter_tiles_dark",
            TaskbarTheme::OneUiLightChips => "wh_one_ui_light_chips",
            TaskbarTheme::OneUiDarkChips => "wh_one_ui_dark_chips",
            TaskbarTheme::SquircleFrosted => "wh_squircle_frosted",
            TaskbarTheme::BubblesDark => "wh_bubbles_dark",
            TaskbarTheme::BubblesLight => "wh_bubbles_light",
            TaskbarTheme::PillsDetached => "wh_pills_detached",
            TaskbarTheme::RosePineTiles => "wh_rose_pine_tiles",
            TaskbarTheme::CatppuccinTiles => "wh_catppuccin_tiles",
            TaskbarTheme::LiquidGlassClear => "wh_liquid_glass_clear",
            TaskbarTheme::DockLikeLight => "wh_docklike_light",
            TaskbarTheme::DockLikeDark => "wh_docklike_dark",
            TaskbarTheme::BlobLight => "wh_blob_light",
            TaskbarTheme::LuminosityFloat => "wh_luminosity_float",
            TaskbarTheme::BorderlessGhost => "wh_borderless_ghost",
            TaskbarTheme::AerisGlassLight => "wh_aeris_glass_light",
            TaskbarTheme::CleanSlateCards => "wh_clean_slate_cards",
            TaskbarTheme::BrushedTitaniumDock => "real_brushed_titanium",
            TaskbarTheme::VacuumTubeHiFiDock => "real_vacuum_tube_hifi",
            TaskbarTheme::SwissHorologyDock => "real_swiss_horology",
            TaskbarTheme::SaddleLeatherDock => "real_saddle_leather",
            TaskbarTheme::ObsidianCarbonDock => "real_obsidian_carbon",
            TaskbarTheme::VintageCassetteDock => "real_vintage_cassette",
            TaskbarTheme::RealisticWaterDock => "real_crystal_water",
            TaskbarTheme::RainWindowDock => "real_rain_window",
            TaskbarTheme::OceanTideDock => "real_ocean_tide",
            TaskbarTheme::ZenCalmMistDock => "real_zen_calm_mist",
            TaskbarTheme::CalmKoiPondDock => "real_calm_koi_pond",
            TaskbarTheme::SunlitForestCanopyDock => "real_forest_sunbeams",
            TaskbarTheme::CarraraMarbleDock => "real_carrara_marble",
            TaskbarTheme::WarmWalnutDeskDock => "real_warm_walnut",
            TaskbarTheme::FrostedGlacierIceDock => "real_glacier_ice",
            TaskbarTheme::SilkLinenCalmDock => "real_silk_linen_calm",
            TaskbarTheme::AbyssalTrenchDock => "real_abyssal_trench",
            TaskbarTheme::GoldenHourHorizonDock => "real_golden_hour_coast",
            TaskbarTheme::QuartzShellDock => "quartz_shell_dock",
            TaskbarTheme::CyberpunkHudDock => "dock_cyberpunk_hud",
            TaskbarTheme::DynamicCapsuleDock => "dock_dynamic_capsule",
            TaskbarTheme::FrutigerAquaDock => "dock_frutiger_aqua",
            TaskbarTheme::InvisibleGhostDock => "dock_invisible_ghost",
            TaskbarTheme::AuroraRibbonDock => "dock_aurora_ribbon",
            TaskbarTheme::NextSlatePlankDock => "dock_next_slate",
            TaskbarTheme::KumikoWoodDock => "dock_kumiko_wood",
            TaskbarTheme::LiquidChromeDock => "dock_liquid_chrome",
            TaskbarTheme::SakuraBreezeDock => "dock_sakura_breeze",
            TaskbarTheme::SolsticeAstrolabeDock => "dock_solstice_astrolabe",
            TaskbarTheme::VaporwaveHorizonDock => "dock_vaporwave_horizon",
            TaskbarTheme::EditorialPaperDock => "dock_editorial_paper",
            TaskbarTheme::BiolumAbyssDock => "dock_biolum_abyss",
            TaskbarTheme::TerminalPhosphorDock => "dock_terminal_phosphor",
            TaskbarTheme::PrismRefractionDock => "dock_prism_refraction",
            TaskbarTheme::ObsidianEmberDock => "dock_obsidian_ember",
            TaskbarTheme::BauhausGeometricDock => "dock_bauhaus_geometric",
            TaskbarTheme::StarfieldWarpDock => "dock_starfield_warp",
            TaskbarTheme::KatanaGlassDock => "dock_katana_glass",
            TaskbarTheme::CupertinoDock => "cupertino_dock",
            TaskbarTheme::DynamicIslandDock => "dynamic_island",
            TaskbarTheme::FrutigerAero7 => "frutiger_aero_7",
            TaskbarTheme::UnixPolybar => "unix_polybar",
            TaskbarTheme::CyberArasakaDeck => "cyber_arasaka_deck",
            TaskbarTheme::Retro95Classic => "retro_95_classic",
            TaskbarTheme::KineticCupertinoWave => "kinetic_cupertino_wave",
            TaskbarTheme::KineticLiquidMercury => "kinetic_liquid_mercury",
            TaskbarTheme::KineticHydroChamber => "kinetic_hydro_chamber",
            TaskbarTheme::KineticQuantumCollider => "kinetic_quantum_collider",
            TaskbarTheme::KineticBiolumAbyss => "kinetic_biolum_abyss",
            TaskbarTheme::KineticSynthTrampoline => "kinetic_synth_trampoline",
            TaskbarTheme::HoverSnowfall => "hover_snowfall",
            TaskbarTheme::HoverInferno => "hover_inferno",
            TaskbarTheme::HoverWaterRipples => "hover_water",
            TaskbarTheme::HoverSakuraWind => "hover_sakura",
            TaskbarTheme::HoverNebulaStardust => "hover_cosmic",
            TaskbarTheme::HoverNeonPlasma => "hover_plasma",
            TaskbarTheme::AutumnCrystal => "autumn_crystal",
            TaskbarTheme::WallpaperGlass => "wallpaper_glass",
            TaskbarTheme::FrostedSplit => "frosted_split",
            TaskbarTheme::AuroraBorealis => "aurora_borealis",
            TaskbarTheme::CyberNeonDock => "cyber_neon_dock",
            TaskbarTheme::SunsetEmber => "sunset_ember",
            TaskbarTheme::LiquidChrome => "liquid_chrome",
            TaskbarTheme::StarlightMinimal => "starlight_minimal",
            TaskbarTheme::LiveMusicBars => "live_music_bars",
            TaskbarTheme::NeonWave => "neon_wave",
            TaskbarTheme::PureClearPulse => "pure_clear_pulse",
            TaskbarTheme::MirrorBeatPill => "mirror_beat_pill",
            TaskbarTheme::BassPulseSplit => "bass_pulse_split",
            TaskbarTheme::VuPeakStudio => "vu_peak_studio",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "wh_os26_liquid_glass_light" | "os26_liquid_glass_light" | "liquid_glass_light" => {
                TaskbarTheme::Os26LiquidGlassLight
            }
            "wh_os26_liquid_glass_dark" | "os26_liquid_glass_dark" | "liquid_glass_dark" => {
                TaskbarTheme::Os26LiquidGlassDark
            }
            "wh_matter_tiles_light" | "matter_light" | "matter_tiles_light" => {
                TaskbarTheme::MatterTilesLight
            }
            "wh_matter_tiles_dark" | "matter_dark" | "matter" => TaskbarTheme::MatterTilesDark,
            "wh_one_ui_light_chips" | "one_ui_light_chips" | "one_ui" => {
                TaskbarTheme::OneUiLightChips
            }
            "wh_one_ui_dark_chips" | "one_ui_dark_chips" => TaskbarTheme::OneUiDarkChips,
            "wh_squircle_frosted" | "squircle_frosted" | "squircle" => {
                TaskbarTheme::SquircleFrosted
            }
            "wh_bubbles_dark" | "bubbles_dark" | "bubbles" => TaskbarTheme::BubblesDark,
            "wh_bubbles_light" | "bubbles_light" => TaskbarTheme::BubblesLight,
            "wh_pills_detached" | "pills_detached" | "pills" => TaskbarTheme::PillsDetached,
            "wh_rose_pine_tiles" | "rose_pine_tiles" | "rose_pine" => TaskbarTheme::RosePineTiles,
            "wh_catppuccin_tiles" | "catppuccin_tiles" | "catppuccin" => {
                TaskbarTheme::CatppuccinTiles
            }
            "wh_liquid_glass_clear" | "liquid_glass_clear" | "liquid_glass_2" => {
                TaskbarTheme::LiquidGlassClear
            }
            "wh_docklike_light" | "docklike_light" => TaskbarTheme::DockLikeLight,
            "wh_docklike_dark" | "docklike_dark" | "docklike" => TaskbarTheme::DockLikeDark,
            "wh_blob_light" | "blob_light" | "blob" => TaskbarTheme::BlobLight,
            "wh_luminosity_float" | "luminosity_float" | "luminosity" => {
                TaskbarTheme::LuminosityFloat
            }
            "wh_borderless_ghost" | "borderless_ghost" | "borderless" => {
                TaskbarTheme::BorderlessGhost
            }
            "wh_aeris_glass_light" | "aeris_glass_light" | "aeris" => TaskbarTheme::AerisGlassLight,
            "wh_clean_slate_cards" | "clean_slate_cards" | "clean_slate" => {
                TaskbarTheme::CleanSlateCards
            }
            "real_brushed_titanium" | "brushed_titanium" => TaskbarTheme::BrushedTitaniumDock,
            "real_vacuum_tube_hifi" | "vacuum_tube_hifi" => TaskbarTheme::VacuumTubeHiFiDock,
            "real_swiss_horology" | "swiss_horology" => TaskbarTheme::SwissHorologyDock,
            "real_saddle_leather" | "saddle_leather" => TaskbarTheme::SaddleLeatherDock,
            "real_obsidian_carbon" | "obsidian_carbon" => TaskbarTheme::ObsidianCarbonDock,
            "real_vintage_cassette" | "vintage_cassette" => TaskbarTheme::VintageCassetteDock,
            "real_crystal_water" | "realistic_water" | "crystal_water" => TaskbarTheme::RealisticWaterDock,
            "real_rain_window" | "rain_window" => TaskbarTheme::RainWindowDock,
            "real_ocean_tide" | "ocean_tide" => TaskbarTheme::OceanTideDock,
            "real_zen_calm_mist" | "zen_calm" | "realistic_calm" => TaskbarTheme::ZenCalmMistDock,
            "real_calm_koi_pond" | "koi_pond" => TaskbarTheme::CalmKoiPondDock,
            "real_forest_sunbeams" | "forest_sunbeams" => TaskbarTheme::SunlitForestCanopyDock,
            "real_carrara_marble" | "carrara_marble" => TaskbarTheme::CarraraMarbleDock,
            "real_warm_walnut" | "warm_walnut" => TaskbarTheme::WarmWalnutDeskDock,
            "real_glacier_ice" | "glacier_ice" => TaskbarTheme::FrostedGlacierIceDock,
            "real_silk_linen_calm" | "silk_linen" => TaskbarTheme::SilkLinenCalmDock,
            "real_abyssal_trench" | "abyssal_trench" | "deep_abyssal_trench" => {
                TaskbarTheme::AbyssalTrenchDock
            }
            "real_golden_hour_coast" | "golden_hour_horizon" | "golden_hour" => {
                TaskbarTheme::GoldenHourHorizonDock
            }
            "quartz_shell_dock" | "quartz_fisheye_dock" | "shell_dock" => {
                TaskbarTheme::QuartzShellDock
            }
            "dock_cyberpunk_hud" => TaskbarTheme::CyberpunkHudDock,
            "dock_dynamic_capsule" => TaskbarTheme::DynamicCapsuleDock,
            "dock_frutiger_aqua" => TaskbarTheme::FrutigerAquaDock,
            "dock_invisible_ghost" => TaskbarTheme::InvisibleGhostDock,
            "dock_aurora_ribbon" => TaskbarTheme::AuroraRibbonDock,
            "dock_next_slate" => TaskbarTheme::NextSlatePlankDock,
            "dock_kumiko_wood" => TaskbarTheme::KumikoWoodDock,
            "dock_liquid_chrome" => TaskbarTheme::LiquidChromeDock,
            "dock_sakura_breeze" => TaskbarTheme::SakuraBreezeDock,
            "dock_solstice_astrolabe" => TaskbarTheme::SolsticeAstrolabeDock,
            "dock_vaporwave_horizon" => TaskbarTheme::VaporwaveHorizonDock,
            "dock_editorial_paper" => TaskbarTheme::EditorialPaperDock,
            "dock_biolum_abyss" => TaskbarTheme::BiolumAbyssDock,
            "dock_terminal_phosphor" => TaskbarTheme::TerminalPhosphorDock,
            "dock_prism_refraction" => TaskbarTheme::PrismRefractionDock,
            "dock_obsidian_ember" => TaskbarTheme::ObsidianEmberDock,
            "dock_bauhaus_geometric" => TaskbarTheme::BauhausGeometricDock,
            "dock_starfield_warp" => TaskbarTheme::StarfieldWarpDock,
            "dock_katana_glass" | "katana_glass" | "katana_dock" => TaskbarTheme::KatanaGlassDock,
            "cupertino_dock" | "macos_dock" | "cupertino" => TaskbarTheme::CupertinoDock,
            "dynamic_island" | "chromeos" => TaskbarTheme::DynamicIslandDock,
            "frutiger_aero_7" | "aero7" | "win7" => TaskbarTheme::FrutigerAero7,
            "unix_polybar" | "polybar" | "hyprland" => TaskbarTheme::UnixPolybar,
            "cyber_arasaka_deck" | "arasaka" => TaskbarTheme::CyberArasakaDeck,
            "retro_95_classic" | "retro95" | "win95" => TaskbarTheme::Retro95Classic,
            "kinetic_cupertino_wave" | "mac_wave" => TaskbarTheme::KineticCupertinoWave,
            "kinetic_liquid_mercury" | "mercury" => TaskbarTheme::KineticLiquidMercury,
            "kinetic_hydro_chamber" | "hydro_bubbles" => TaskbarTheme::KineticHydroChamber,
            "kinetic_quantum_collider" | "quantum" => TaskbarTheme::KineticQuantumCollider,
            "kinetic_biolum_abyss" | "biolum" => TaskbarTheme::KineticBiolumAbyss,
            "kinetic_synth_trampoline" | "synth_grid" => TaskbarTheme::KineticSynthTrampoline,
            "hover_snowfall" | "snowfall" => TaskbarTheme::HoverSnowfall,
            "hover_inferno" | "inferno" => TaskbarTheme::HoverInferno,
            "hover_water" | "water" => TaskbarTheme::HoverWaterRipples,
            "hover_sakura" | "sakura" => TaskbarTheme::HoverSakuraWind,
            "hover_cosmic" | "cosmic" => TaskbarTheme::HoverNebulaStardust,
            "hover_plasma" | "plasma" => TaskbarTheme::HoverNeonPlasma,
            "wallpaper_glass" => TaskbarTheme::WallpaperGlass,
            "frosted_split" => TaskbarTheme::FrostedSplit,
            "aurora_borealis" => TaskbarTheme::AuroraBorealis,
            "cyber_neon_dock" => TaskbarTheme::CyberNeonDock,
            "sunset_ember" => TaskbarTheme::SunsetEmber,
            "liquid_chrome" => TaskbarTheme::LiquidChrome,
            "starlight_minimal" => TaskbarTheme::StarlightMinimal,
            "live_music_bars" => TaskbarTheme::LiveMusicBars,
            "neon_wave" => TaskbarTheme::NeonWave,
            "pure_clear_pulse" => TaskbarTheme::PureClearPulse,
            "mirror_beat_pill" => TaskbarTheme::MirrorBeatPill,
            "bass_pulse_split" => TaskbarTheme::BassPulseSplit,
            "vu_peak_studio" => TaskbarTheme::VuPeakStudio,
            _ => TaskbarTheme::AutumnCrystal,
        }
    }

    pub fn from_opt_str(s: &str) -> Option<Self> {
        let low = s.trim().to_ascii_lowercase();
        if low.is_empty() || low == "inherit" || low == "none" || low == "default" {
            None
        } else {
            Some(Self::from_str(&low))
        }
    }

    pub fn opt_to_str(opt: Option<Self>) -> &'static str {
        match opt {
            Some(t) => t.as_str(),
            None => "inherit",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            TaskbarTheme::Os26LiquidGlassLight => "OS26 Liquid Glass Light",
            TaskbarTheme::Os26LiquidGlassDark => "OS26 Liquid Glass Dark",
            TaskbarTheme::MatterTilesLight => "Matter Tiles Light",
            TaskbarTheme::MatterTilesDark => "Matter Tiles Dark",
            TaskbarTheme::OneUiLightChips => "One UI Light Chips",
            TaskbarTheme::OneUiDarkChips => "One UI Dark Chips",
            TaskbarTheme::SquircleFrosted => "Squircle Frosted",
            TaskbarTheme::BubblesDark => "Bubbles Dark",
            TaskbarTheme::BubblesLight => "Bubbles Light",
            TaskbarTheme::PillsDetached => "Pills Detached",
            TaskbarTheme::RosePineTiles => "Rose Pine Tiles",
            TaskbarTheme::CatppuccinTiles => "Catppuccin Tiles",
            TaskbarTheme::LiquidGlassClear => "Liquid Glass Clear",
            TaskbarTheme::DockLikeLight => "DockLike Light",
            TaskbarTheme::DockLikeDark => "DockLike Dark",
            TaskbarTheme::BlobLight => "Blob Light",
            TaskbarTheme::LuminosityFloat => "Luminosity Float",
            TaskbarTheme::BorderlessGhost => "Borderless Ghost",
            TaskbarTheme::AerisGlassLight => "Aeris Glass Light",
            TaskbarTheme::CleanSlateCards => "Clean Slate Cards",
            TaskbarTheme::BrushedTitaniumDock => "Titanium CNC Machined",
            TaskbarTheme::VacuumTubeHiFiDock => "Audiophile Vacuum Tube",
            TaskbarTheme::SwissHorologyDock => "Swiss Haute Horlogerie",
            TaskbarTheme::SaddleLeatherDock => "Saddle-Stitched Leather",
            TaskbarTheme::ObsidianCarbonDock => "Leica Matte Carbon",
            TaskbarTheme::VintageCassetteDock => "Cassette Futurism 1984",
            TaskbarTheme::RealisticWaterDock => "Crystal Lagoon Water",
            TaskbarTheme::RainWindowDock => "Rainy Glass Window",
            TaskbarTheme::OceanTideDock => "Deep Ocean Tide",
            TaskbarTheme::ZenCalmMistDock => "Zen Morning Mist",
            TaskbarTheme::CalmKoiPondDock => "Tranquil Koi Pond",
            TaskbarTheme::SunlitForestCanopyDock => "Forest Sunbeams",
            TaskbarTheme::CarraraMarbleDock => "Carrara Marble & Gold",
            TaskbarTheme::WarmWalnutDeskDock => "Oiled Walnut Wood",
            TaskbarTheme::FrostedGlacierIceDock => "Nordic Glacial Ice",
            TaskbarTheme::SilkLinenCalmDock => "Warm Linen & Dusk",
            TaskbarTheme::AbyssalTrenchDock => "Deep Abyssal Trench",
            TaskbarTheme::GoldenHourHorizonDock => "Golden Hour Horizon",
            TaskbarTheme::QuartzShellDock => "Quartz Fish-Eye Dock",
            TaskbarTheme::CyberpunkHudDock => "Cyberpunk HUD Deck",
            TaskbarTheme::DynamicCapsuleDock => "Dynamic Island Pill",
            TaskbarTheme::FrutigerAquaDock => "Frutiger Aqua Gloss",
            TaskbarTheme::InvisibleGhostDock => "Invisible Ghost Float",
            TaskbarTheme::AuroraRibbonDock => "Aurora Silk Ribbon",
            TaskbarTheme::NextSlatePlankDock => "NeXT Slate Plank",
            TaskbarTheme::KumikoWoodDock => "Kumiko Hinoki Craft",
            TaskbarTheme::LiquidChromeDock => "Liquid Quicksilver",
            TaskbarTheme::SakuraBreezeDock => "Sakura Twilight Breeze",
            TaskbarTheme::SolsticeAstrolabeDock => "Solstice Astrolabe",
            TaskbarTheme::VaporwaveHorizonDock => "Outrun Vapor Grid",
            TaskbarTheme::EditorialPaperDock => "Swiss Editorial E-Ink",
            TaskbarTheme::BiolumAbyssDock => "Abyssal Biolume",
            TaskbarTheme::TerminalPhosphorDock => "Phosphor CRT Scope",
            TaskbarTheme::PrismRefractionDock => "Crystal Prism Facet",
            TaskbarTheme::ObsidianEmberDock => "Volcanic Obsidian",
            TaskbarTheme::BauhausGeometricDock => "Bauhaus Construct",
            TaskbarTheme::StarfieldWarpDock => "Deep-Space Telemetry",
            TaskbarTheme::KatanaGlassDock => "Katana Smoked Glass",
            TaskbarTheme::CupertinoDock => "Cupertino Quartz Dock",
            TaskbarTheme::DynamicIslandDock => "Dynamic Island Shelf",
            TaskbarTheme::FrutigerAero7 => "Frutiger Aero 7 Glass",
            TaskbarTheme::UnixPolybar => "UnixPorn Polybar",
            TaskbarTheme::CyberArasakaDeck => "Cyberpunk HUD Deck",
            TaskbarTheme::Retro95Classic => "Retro 95 Classic",
            TaskbarTheme::KineticCupertinoWave => "Cupertino Kinetic Wave",
            TaskbarTheme::KineticLiquidMercury => "Liquid Quicksilver",
            TaskbarTheme::KineticHydroChamber => "Hydro Fluid Chamber",
            TaskbarTheme::KineticQuantumCollider => "Quantum Collider",
            TaskbarTheme::KineticBiolumAbyss => "Abyssal Bioluminescence",
            TaskbarTheme::KineticSynthTrampoline => "Synthwave Grid Warp",
            TaskbarTheme::HoverSnowfall => "Winter Snowfall",
            TaskbarTheme::HoverInferno => "Hearth Inferno",
            TaskbarTheme::HoverWaterRipples => "Liquid Caustics",
            TaskbarTheme::HoverSakuraWind => "Sakura Breeze",
            TaskbarTheme::HoverNebulaStardust => "Cosmic Gravity",
            TaskbarTheme::HoverNeonPlasma => "Tesla Plasma",
            TaskbarTheme::AutumnCrystal => "Autumn Crystal",
            TaskbarTheme::WallpaperGlass => "Wallpaper Live Glass",
            TaskbarTheme::FrostedSplit => "Frosted Split Pills",
            TaskbarTheme::AuroraBorealis => "Aurora Borealis",
            TaskbarTheme::CyberNeonDock => "Cyberpunk HUD Dock",
            TaskbarTheme::SunsetEmber => "Sunset Ember Drift",
            TaskbarTheme::LiquidChrome => "Liquid Chrome Island",
            TaskbarTheme::StarlightMinimal => "Starlight Minimal",
            TaskbarTheme::LiveMusicBars => "Live Music Bars",
            TaskbarTheme::NeonWave => "Neon Sound Wave",
            TaskbarTheme::PureClearPulse => "Invisible Floating",
            TaskbarTheme::MirrorBeatPill => "Mirror Beat Capsule",
            TaskbarTheme::BassPulseSplit => "Bass Pulse Split",
            TaskbarTheme::VuPeakStudio => "Studio VU Peaks",
        }
    }

    pub fn badge(self) -> &'static str {
        match self {
            TaskbarTheme::Os26LiquidGlassLight => "Light Glass",
            TaskbarTheme::Os26LiquidGlassDark => "Liquid Glass",
            TaskbarTheme::MatterTilesLight => "Light Tiles",
            TaskbarTheme::MatterTilesDark => "Float Tiles",
            TaskbarTheme::OneUiLightChips => "One UI 8.5",
            TaskbarTheme::OneUiDarkChips => "Light Rail",
            TaskbarTheme::SquircleFrosted => "Squircle",
            TaskbarTheme::BubblesDark => "Circles",
            TaskbarTheme::BubblesLight => "Light Circles",
            TaskbarTheme::PillsDetached => "Pills",
            TaskbarTheme::RosePineTiles => "Rose Pine",
            TaskbarTheme::CatppuccinTiles => "Mocha Tiles",
            TaskbarTheme::LiquidGlassClear => "Clear Glass",
            TaskbarTheme::DockLikeLight => "macOS Light",
            TaskbarTheme::DockLikeDark => "DockLike",
            TaskbarTheme::BlobLight => "Blob Light",
            TaskbarTheme::LuminosityFloat => "Luminosity",
            TaskbarTheme::BorderlessGhost => "Zero Bar",
            TaskbarTheme::AerisGlassLight => "Aeris Light",
            TaskbarTheme::CleanSlateCards => "CleanSlate",
            TaskbarTheme::BrushedTitaniumDock => "Brushed Ti",
            TaskbarTheme::VacuumTubeHiFiDock => "Hi-Fi Tube",
            TaskbarTheme::SwissHorologyDock => "Horology",
            TaskbarTheme::SaddleLeatherDock => "Leather",
            TaskbarTheme::ObsidianCarbonDock => "Carbon 2x2",
            TaskbarTheme::VintageCassetteDock => "Tape 1984",
            TaskbarTheme::RealisticWaterDock => "Live Water",
            TaskbarTheme::RainWindowDock => "Rain Glass",
            TaskbarTheme::OceanTideDock => "Tide Foam",
            TaskbarTheme::ZenCalmMistDock => "Calm Zen",
            TaskbarTheme::CalmKoiPondDock => "Koi Pond",
            TaskbarTheme::SunlitForestCanopyDock => "Calm Woods",
            TaskbarTheme::CarraraMarbleDock => "Marble",
            TaskbarTheme::WarmWalnutDeskDock => "Walnut",
            TaskbarTheme::FrostedGlacierIceDock => "Glacial Ice",
            TaskbarTheme::SilkLinenCalmDock => "Calm Linen",
            TaskbarTheme::AbyssalTrenchDock => "Abyssal",
            TaskbarTheme::GoldenHourHorizonDock => "Golden Hr",
            TaskbarTheme::QuartzShellDock => "Shell Dock",
            TaskbarTheme::CyberpunkHudDock => "Sci-Fi Deck",
            TaskbarTheme::DynamicCapsuleDock => "OLED Pill",
            TaskbarTheme::FrutigerAquaDock => "2000s Gloss",
            TaskbarTheme::InvisibleGhostDock => "Borderless",
            TaskbarTheme::AuroraRibbonDock => "Live Aurora",
            TaskbarTheme::NextSlatePlankDock => "Workstation",
            TaskbarTheme::KumikoWoodDock => "Woodcraft",
            TaskbarTheme::LiquidChromeDock => "Molten Metal",
            TaskbarTheme::SakuraBreezeDock => "Blossom",
            TaskbarTheme::SolsticeAstrolabeDock => "Brass Clock",
            TaskbarTheme::VaporwaveHorizonDock => "80s Horizon",
            TaskbarTheme::EditorialPaperDock => "Paper Ink",
            TaskbarTheme::BiolumAbyssDock => "Deep Sea",
            TaskbarTheme::TerminalPhosphorDock => "P31 CRT",
            TaskbarTheme::PrismRefractionDock => "Dispersion",
            TaskbarTheme::ObsidianEmberDock => "Magma Forge",
            TaskbarTheme::BauhausGeometricDock => "Modernist",
            TaskbarTheme::StarfieldWarpDock => "Warp Flight",
            TaskbarTheme::KatanaGlassDock => "Minimal Glass",
            TaskbarTheme::CupertinoDock => "macOS Dock",
            TaskbarTheme::DynamicIslandDock => "Island OS",
            TaskbarTheme::FrutigerAero7 => "Win7 Aero",
            TaskbarTheme::UnixPolybar => "Hyprland",
            TaskbarTheme::CyberArasakaDeck => "Chamfer HUD",
            TaskbarTheme::Retro95Classic => "90s Bevel",
            TaskbarTheme::KineticCupertinoWave => "Mac Spring",
            TaskbarTheme::KineticLiquidMercury => "Fluid Metal",
            TaskbarTheme::KineticHydroChamber => "Live Bubbles",
            TaskbarTheme::KineticQuantumCollider => "Beam Clash",
            TaskbarTheme::KineticBiolumAbyss => "Living Glow",
            TaskbarTheme::KineticSynthTrampoline => "3D Grid Warp",
            TaskbarTheme::HoverSnowfall => "Hover Snow",
            TaskbarTheme::HoverInferno => "Hover Fire",
            TaskbarTheme::HoverWaterRipples => "Hover Water",
            TaskbarTheme::HoverSakuraWind => "Hover Wind",
            TaskbarTheme::HoverNebulaStardust => "Hover Gravity",
            TaskbarTheme::HoverNeonPlasma => "Hover Plasma",
            TaskbarTheme::AutumnCrystal => "Classic #1",
            TaskbarTheme::WallpaperGlass => "Live BG",
            TaskbarTheme::FrostedSplit => "Frost Blur",
            TaskbarTheme::AuroraBorealis => "Live Silk",
            TaskbarTheme::CyberNeonDock => "Live HUD",
            TaskbarTheme::SunsetEmber => "Live Embers",
            TaskbarTheme::LiquidChrome => "Live Sheen",
            TaskbarTheme::StarlightMinimal => "Live Stars",
            TaskbarTheme::LiveMusicBars => "Music EQ",
            TaskbarTheme::NeonWave => "Music Wave",
            TaskbarTheme::PureClearPulse => "Music Float",
            TaskbarTheme::MirrorBeatPill => "Music Mirror",
            TaskbarTheme::BassPulseSplit => "Music Rim",
            TaskbarTheme::VuPeakStudio => "Music VU",
        }
    }

    pub fn subtitle(self) -> &'static str {
        match self {
            TaskbarTheme::Os26LiquidGlassLight => {
                "Clear frost outline rail with luminous white liquid-glass icon keycaps"
            }
            TaskbarTheme::Os26LiquidGlassDark => {
                "Razor-thin glass rail with frosted liquid-glass squircle keycaps & bevels"
            }
            TaskbarTheme::MatterTilesLight => {
                "Zero bar — snow-white floating squircle tiles over wallpaper"
            }
            TaskbarTheme::MatterTilesDark => {
                "Zero bar — dark matte squircle tiles floating over wallpaper"
            }
            TaskbarTheme::OneUiLightChips => {
                "Sleek dark glass rail cradling bright white rounded icon chips"
            }
            TaskbarTheme::OneUiDarkChips => {
                "Airy pearl-frost glass rail cradling crisp dark obsidian icon chips"
            }
            TaskbarTheme::SquircleFrosted => {
                "Zero bar — independent smoky frosted squircle islands per app icon"
            }
            TaskbarTheme::BubblesDark => {
                "Standalone round graphite bubbles with focus ring & tray pill"
            }
            TaskbarTheme::BubblesLight => {
                "Standalone milky-white circular glass bubbles over wallpaper"
            }
            TaskbarTheme::PillsDetached => {
                "Zero bar — detached dark-frosted bordered icon pills floating freely"
            }
            TaskbarTheme::RosePineTiles => {
                "Zero bar — floating Rosé Pine tiles with rose-gold focus border"
            }
            TaskbarTheme::CatppuccinTiles => {
                "Zero bar — Catppuccin Mocha tiles with pastel lavender border"
            }
            TaskbarTheme::LiquidGlassClear => {
                "Ultra-clear stadium glass pill with bright refractive white edge sheen"
            }
            TaskbarTheme::DockLikeLight => {
                "Clean alabaster-white macOS DockLike shelf with separate tray island"
            }
            TaskbarTheme::DockLikeDark => {
                "Classic dark navy-slate DockLike shelf with subtle tray separation"
            }
            TaskbarTheme::BlobLight => {
                "Soft ice-gray ribbon with pure-white elevated pill tabs for each app"
            }
            TaskbarTheme::LuminosityFloat => {
                "Wide centered floating frosted pill softly tinted by your wallpaper"
            }
            TaskbarTheme::BorderlessGhost => {
                "Clear floating icons over desktop with whisper-soft readability halo"
            }
            TaskbarTheme::AerisGlassLight => {
                "Full-width crisp icy-white glass bar with flat square translucent blocks"
            }
            TaskbarTheme::CleanSlateCards => {
                "Full-width deep glass bar with dark-frosted bordered app cards"
            }
            TaskbarTheme::BrushedTitaniumDock => {
                "Anisotropic CNC-milled titanium plank with Torx bolts & bevels"
            }
            TaskbarTheme::VacuumTubeHiFiDock => {
                "Champagne brass hi-fi deck with warm glowing 12AX7 glass tubes"
            }
            TaskbarTheme::SwissHorologyDock => {
                "Guilloche engine-turned midnight bezel with ruby jewel pivots"
            }
            TaskbarTheme::SaddleLeatherDock => {
                "Hand-stitched cognac calfskin strap with antique brass rivets"
            }
            TaskbarTheme::ObsidianCarbonDock => {
                "Stealth matte vulcanite islands with 2x2 twill carbon & ruby dot"
            }
            TaskbarTheme::VintageCassetteDock => {
                "1984 industrial ABS cyberdeck with spinning dual cassette reels"
            }
            TaskbarTheme::RealisticWaterDock => {
                "Sunlit crystal lagoon water with animated caustics & hover ripples"
            }
            TaskbarTheme::RainWindowDock => {
                "Twilight rain-frosted windowpane with refractive 3D droplets"
            }
            TaskbarTheme::OceanTideDock => {
                "Full-edge deep ocean tide swell with lacy translucent sea-foam"
            }
            TaskbarTheme::ZenCalmMistDock => {
                "Ultra-calm misty sage-stone islands with serene raked sand arcs"
            }
            TaskbarTheme::CalmKoiPondDock => {
                "Peaceful jade reflection pond with lily pads & gliding koi fish"
            }
            TaskbarTheme::SunlitForestCanopyDock => {
                "Calm mossy cedar bar with warm sunlit canopy god-rays & pollen"
            }
            TaskbarTheme::CarraraMarbleDock => {
                "Polished white Italian Carrara marble slab with kintsugi gold"
            }
            TaskbarTheme::WarmWalnutDeskDock => {
                "Full-edge hand-oiled American black walnut wood grain desk rail"
            }
            TaskbarTheme::FrostedGlacierIceDock => {
                "Faceted crystalline glacier ice with 45° chromatic prism sheen"
            }
            TaskbarTheme::SilkLinenCalmDock => {
                "Soft woven oatmeal linen islands with stitched dusk calm glow"
            }
            TaskbarTheme::AbyssalTrenchDock => {
                "Deep abyssal liquid density filter with interactive bioluminescent plankton"
            }
            TaskbarTheme::GoldenHourHorizonDock => {
                "Coastal dusk horizon with shimmering solar water parallax reflections"
            }
            TaskbarTheme::QuartzShellDock => {
                "Interactive floating quartz dock with fish-eye zoom & bounce"
            }
            TaskbarTheme::CyberpunkHudDock => {
                "Stepped tactical HUD cockpit with angled slots & clock telemetry"
            }
            TaskbarTheme::DynamicCapsuleDock => {
                "Split 3-island dynamic capsules with telemetry & live clock"
            }
            TaskbarTheme::FrutigerAquaDock => {
                "Full-edge 2009 aqua taskbar with window chips & live clock"
            }
            TaskbarTheme::InvisibleGhostDock => {
                "Zero-bar spotlight icons hovering freely with moonbeam aura"
            }
            TaskbarTheme::AuroraRibbonDock => {
                "Split 3-island aurora bars with silk ribbons & live clock"
            }
            TaskbarTheme::NextSlatePlankDock => {
                "Full-edge brushed workstation taskbar with beveled title tiles"
            }
            TaskbarTheme::KumikoWoodDock => {
                "Walnut joinery bar unfolding shoji lattice labels inline"
            }
            TaskbarTheme::LiquidChromeDock => {
                "Curved liquid quicksilver wave dock with metallic sheen"
            }
            TaskbarTheme::SakuraBreezeDock => {
                "Twilight cherry-blossom wave dock with fluttering petals"
            }
            TaskbarTheme::SolsticeAstrolabeDock => {
                "Brass clockwork console with circular portholes & sundial"
            }
            TaskbarTheme::VaporwaveHorizonDock => {
                "Stepped 80s Outrun synth deck with neon horizon & clock strip"
            }
            TaskbarTheme::EditorialPaperDock => {
                "Flat ivory gallery bar unfolding inline Swiss typography on hover"
            }
            TaskbarTheme::BiolumAbyssDock => {
                "Independent floating abyssal bubbles bobbing without a bar"
            }
            TaskbarTheme::TerminalPhosphorDock => {
                "CRT phosphor console with ASCII status header & [ ] keycaps"
            }
            TaskbarTheme::PrismRefractionDock => {
                "Standalone octagonal crystal prism jewels floating freely"
            }
            TaskbarTheme::ObsidianEmberDock => {
                "Volcanic basalt forge deck with hearth sockets & live sparks"
            }
            TaskbarTheme::BauhausGeometricDock => {
                "Modernist primary block tiles sliding open inline names on hover"
            }
            TaskbarTheme::StarfieldWarpDock => {
                "Split 3-island space cockpit bar with live clock & warp stars"
            }
            TaskbarTheme::KatanaGlassDock => {
                "Ultra-minimal unified smoked-silver floating glass dock (0% idle CPU)"
            }
            TaskbarTheme::CupertinoDock => "Floating Mac-style quartz glass shelf with dock divider",
            TaskbarTheme::DynamicIslandDock => "Compact matte obsidian capsule with LED indicator",
            TaskbarTheme::FrutigerAero7 => "2009 aqua gloss bar with specular horizon sheen",
            TaskbarTheme::UnixPolybar => "Minimal Linux chips with pastel accents & workspaces",
            TaskbarTheme::CyberArasakaDeck => "45-degree chamfered octagonal HUD with corner brackets",
            TaskbarTheme::Retro95Classic => "Classic 3D beveled slate chrome & sunken tray frame",
            TaskbarTheme::KineticCupertinoWave => "Mac dock lip swells & ripples toward cursor on spring",
            TaskbarTheme::KineticLiquidMercury => "Molten liquid chrome splits & flings mirror droplets",
            TaskbarTheme::KineticHydroChamber => "Buoyant 3D water bubbles swirl in cursor eddies & pop",
            TaskbarTheme::KineticQuantumCollider => "Opposing neon beams bend into cursor to spark clashes",
            TaskbarTheme::KineticBiolumAbyss => "Abyssal tendrils & plankton ignite in cursor wakes",
            TaskbarTheme::KineticSynthTrampoline => "80s neon wireframe grid bends & springs under hover",
            TaskbarTheme::HoverSnowfall => "Crystalline snowflakes swirl & gust off mouse wake",
            TaskbarTheme::HoverInferno => "Roaring flames bend & ignite bright sparks on hover",
            TaskbarTheme::HoverWaterRipples => "Caustic water pool with mouse-carved ring ripples",
            TaskbarTheme::HoverSakuraWind => "Tumbling cherry petals scatter & spin in cursor breeze",
            TaskbarTheme::HoverNebulaStardust => "Gravity lens orbits stars & comets toward pointer",
            TaskbarTheme::HoverNeonPlasma => "Ionized plasma filaments arc & snap to mouse touch",
            TaskbarTheme::AutumnCrystal => "Unified clear crystal capsule with amber glow rim",
            TaskbarTheme::WallpaperGlass => "Live video passthrough + wallpaper-matched accent",
            TaskbarTheme::FrostedSplit => "Separate dock & tray capsules with reactive lip",
            TaskbarTheme::AuroraBorealis => "Flowing northern-lights silk ribbons across capsule",
            TaskbarTheme::CyberNeonDock => "Tech HUD brackets + sweeping neon laser scanner",
            TaskbarTheme::SunsetEmber => "Warm golden firefly embers drifting behind icons",
            TaskbarTheme::LiquidChrome => "Metallic mirror capsule with sweeping light sheen",
            TaskbarTheme::StarlightMinimal => "Twinkling starlight constellation nodes & threads",
            TaskbarTheme::LiveMusicBars => "Real-time equalizer music bars dancing behind icons",
            TaskbarTheme::NeonWave => "Smooth dual-ribbon GPU audio waveform across bar",
            TaskbarTheme::PureClearPulse => "Zero card backdrop — icons & beat bars over video",
            TaskbarTheme::MirrorBeatPill => "Center-out symmetrical studio spectrum inside pill",
            TaskbarTheme::BassPulseSplit => "Split frosted pills with beat-reactive neon rim",
            TaskbarTheme::VuPeakStudio => "Full-bar studio LED equalizer with peak-hold caps",
        }
    }

    pub fn uses_virtual_dock(self) -> bool {
        Self::CLEAN_MINIMAL_FOLDER.contains(&self)
            || Self::INTERACTIVE_DOCKS_FOLDER.contains(&self)
            || Self::REALISTIC_FOLDER.contains(&self)
    }

    pub fn is_clean_minimal(self) -> bool {
        Self::CLEAN_MINIMAL_FOLDER.contains(&self)
    }

    #[allow(dead_code)]
    pub fn is_light_surface(self) -> bool {
        matches!(
            self,
            TaskbarTheme::Os26LiquidGlassLight
                | TaskbarTheme::MatterTilesLight
                | TaskbarTheme::OneUiDarkChips
                | TaskbarTheme::BubblesLight
                | TaskbarTheme::DockLikeLight
                | TaskbarTheme::BlobLight
                | TaskbarTheme::AerisGlassLight
        )
    }

    pub fn uses_icon_pads(self) -> bool {
        matches!(
            self,
            TaskbarTheme::Os26LiquidGlassLight
                | TaskbarTheme::Os26LiquidGlassDark
                | TaskbarTheme::MatterTilesLight
                | TaskbarTheme::MatterTilesDark
                | TaskbarTheme::OneUiLightChips
                | TaskbarTheme::OneUiDarkChips
                | TaskbarTheme::SquircleFrosted
                | TaskbarTheme::BubblesDark
                | TaskbarTheme::BubblesLight
                | TaskbarTheme::PillsDetached
                | TaskbarTheme::RosePineTiles
                | TaskbarTheme::CatppuccinTiles
                | TaskbarTheme::BlobLight
                | TaskbarTheme::AerisGlassLight
                | TaskbarTheme::CleanSlateCards
        )
    }

    /// True for zero-bar Windhawk tile/circle themes where app icons float as independent
    /// individual tiles directly over the desktop wallpaper (no enclosing bar behind `cap_idx == 0`).
    pub fn is_independent_tile_dock(self) -> bool {
        matches!(
            self,
            TaskbarTheme::MatterTilesLight
                | TaskbarTheme::MatterTilesDark
                | TaskbarTheme::SquircleFrosted
                | TaskbarTheme::BubblesDark
                | TaskbarTheme::BubblesLight
                | TaskbarTheme::PillsDetached
                | TaskbarTheme::RosePineTiles
                | TaskbarTheme::CatppuccinTiles
        )
    }

    pub fn preview_preset(self) -> (PillMode, SurfaceMode, u8, bool, TaskbarVizStyle, bool) {
        match self {
            TaskbarTheme::Os26LiquidGlassLight => (
                PillMode::SplitPills,
                SurfaceMode::Blur,
                148,
                true,
                TaskbarVizStyle::Off,
                false,
            ),
            TaskbarTheme::Os26LiquidGlassDark => (
                PillMode::SplitPills,
                SurfaceMode::Blur,
                104,
                true,
                TaskbarVizStyle::Off,
                false,
            ),
            TaskbarTheme::MatterTilesLight => (
                PillMode::SplitPills,
                SurfaceMode::ClearTint,
                226,
                false,
                TaskbarVizStyle::Off,
                false,
            ),
            TaskbarTheme::MatterTilesDark => (
                PillMode::SplitPills,
                SurfaceMode::ClearTint,
                228,
                false,
                TaskbarVizStyle::Off,
                false,
            ),
            TaskbarTheme::OneUiLightChips => (
                PillMode::SplitPills,
                SurfaceMode::Blur,
                118,
                true,
                TaskbarVizStyle::Off,
                false,
            ),
            TaskbarTheme::OneUiDarkChips => (
                PillMode::SplitPills,
                SurfaceMode::Blur,
                162,
                true,
                TaskbarVizStyle::Off,
                false,
            ),
            TaskbarTheme::SquircleFrosted => (
                PillMode::SplitPills,
                SurfaceMode::ClearTint,
                194,
                false,
                TaskbarVizStyle::Off,
                false,
            ),
            TaskbarTheme::BubblesDark => (
                PillMode::SplitPills,
                SurfaceMode::ClearTint,
                226,
                false,
                TaskbarVizStyle::Off,
                false,
            ),
            TaskbarTheme::BubblesLight => (
                PillMode::SplitPills,
                SurfaceMode::ClearTint,
                220,
                false,
                TaskbarVizStyle::Off,
                false,
            ),
            TaskbarTheme::PillsDetached => (
                PillMode::SplitPills,
                SurfaceMode::ClearTint,
                204,
                true,
                TaskbarVizStyle::Off,
                false,
            ),
            TaskbarTheme::RosePineTiles => (
                PillMode::SplitPills,
                SurfaceMode::ClearTint,
                232,
                true,
                TaskbarVizStyle::Off,
                false,
            ),
            TaskbarTheme::CatppuccinTiles => (
                PillMode::SplitPills,
                SurfaceMode::ClearTint,
                234,
                true,
                TaskbarVizStyle::Off,
                false,
            ),
            TaskbarTheme::LiquidGlassClear => (
                PillMode::SplitPills,
                SurfaceMode::Blur,
                62,
                true,
                TaskbarVizStyle::Off,
                false,
            ),
            TaskbarTheme::DockLikeLight => (
                PillMode::SplitPills,
                SurfaceMode::Blur,
                182,
                true,
                TaskbarVizStyle::Off,
                false,
            ),
            TaskbarTheme::DockLikeDark => (
                PillMode::SplitPills,
                SurfaceMode::ClearTint,
                224,
                true,
                TaskbarVizStyle::Off,
                false,
            ),
            TaskbarTheme::BlobLight => (
                PillMode::FullBar,
                SurfaceMode::Blur,
                188,
                false,
                TaskbarVizStyle::Off,
                false,
            ),
            TaskbarTheme::LuminosityFloat => (
                PillMode::UnifiedPill,
                SurfaceMode::Blur,
                124,
                true,
                TaskbarVizStyle::Off,
                false,
            ),
            TaskbarTheme::BorderlessGhost => (
                PillMode::SplitPills,
                SurfaceMode::ClearTint,
                0,
                false,
                TaskbarVizStyle::Off,
                false,
            ),
            TaskbarTheme::AerisGlassLight => (
                PillMode::FullBar,
                SurfaceMode::Blur,
                172,
                false,
                TaskbarVizStyle::Off,
                false,
            ),
            TaskbarTheme::CleanSlateCards => (
                PillMode::FullBar,
                SurfaceMode::Blur,
                138,
                true,
                TaskbarVizStyle::Off,
                false,
            ),
            TaskbarTheme::RealisticWaterDock
            | TaskbarTheme::RainWindowDock
            | TaskbarTheme::OceanTideDock
            | TaskbarTheme::CalmKoiPondDock
            | TaskbarTheme::FrostedGlacierIceDock
            | TaskbarTheme::AbyssalTrenchDock
            | TaskbarTheme::GoldenHourHorizonDock => (
                PillMode::UnifiedPill,
                SurfaceMode::Blur,
                112,
                true,
                TaskbarVizStyle::AmbientGlow,
                false,
            ),
            TaskbarTheme::ZenCalmMistDock
            | TaskbarTheme::SunlitForestCanopyDock
            | TaskbarTheme::SilkLinenCalmDock => (
                PillMode::SplitPills,
                SurfaceMode::Blur,
                156,
                true,
                TaskbarVizStyle::Off,
                false,
            ),
            TaskbarTheme::BrushedTitaniumDock
            | TaskbarTheme::VacuumTubeHiFiDock
            | TaskbarTheme::SwissHorologyDock
            | TaskbarTheme::SaddleLeatherDock
            | TaskbarTheme::ObsidianCarbonDock
            | TaskbarTheme::VintageCassetteDock
            | TaskbarTheme::CarraraMarbleDock
            | TaskbarTheme::WarmWalnutDeskDock => (
                PillMode::UnifiedPill,
                SurfaceMode::ClearTint,
                228,
                true,
                TaskbarVizStyle::Off,
                false,
            ),
            TaskbarTheme::QuartzShellDock
            | TaskbarTheme::KatanaGlassDock
            | TaskbarTheme::AuroraRibbonDock
            | TaskbarTheme::SakuraBreezeDock
            | TaskbarTheme::BiolumAbyssDock
            | TaskbarTheme::PrismRefractionDock
            | TaskbarTheme::StarfieldWarpDock
            | TaskbarTheme::CupertinoDock => (
                PillMode::SplitPills,
                SurfaceMode::Blur,
                66,
                true,
                TaskbarVizStyle::AmbientGlow,
                false,
            ),
            TaskbarTheme::DynamicCapsuleDock => (
                PillMode::UnifiedPill,
                SurfaceMode::ClearTint,
                228,
                true,
                TaskbarVizStyle::AmbientGlow,
                false,
            ),
            TaskbarTheme::CyberpunkHudDock
            | TaskbarTheme::VaporwaveHorizonDock
            | TaskbarTheme::TerminalPhosphorDock
            | TaskbarTheme::ObsidianEmberDock => (
                PillMode::UnifiedPill,
                SurfaceMode::ClearTint,
                205,
                true,
                TaskbarVizStyle::AmbientGlow,
                false,
            ),
            TaskbarTheme::FrutigerAquaDock | TaskbarTheme::LiquidChromeDock => (
                PillMode::UnifiedPill,
                SurfaceMode::Blur,
                118,
                true,
                TaskbarVizStyle::AmbientGlow,
                false,
            ),
            TaskbarTheme::InvisibleGhostDock => (
                PillMode::UnifiedPill,
                SurfaceMode::ClearTint,
                0,
                false,
                TaskbarVizStyle::AmbientGlow,
                false,
            ),
            TaskbarTheme::NextSlatePlankDock
            | TaskbarTheme::KumikoWoodDock
            | TaskbarTheme::SolsticeAstrolabeDock
            | TaskbarTheme::EditorialPaperDock
            | TaskbarTheme::BauhausGeometricDock => (
                PillMode::UnifiedPill,
                SurfaceMode::ClearTint,
                220,
                true,
                TaskbarVizStyle::Off,
                false,
            ),
            TaskbarTheme::DynamicIslandDock => (
                PillMode::SplitPills,
                SurfaceMode::ClearTint,
                222,
                true,
                TaskbarVizStyle::AmbientGlow,
                false,
            ),
            TaskbarTheme::FrutigerAero7 => (
                PillMode::FullBar,
                SurfaceMode::Blur,
                118,
                true,
                TaskbarVizStyle::Off,
                false,
            ),
            TaskbarTheme::UnixPolybar => (
                PillMode::SplitPills,
                SurfaceMode::ClearTint,
                212,
                true,
                TaskbarVizStyle::Off,
                false,
            ),
            TaskbarTheme::CyberArasakaDeck => (
                PillMode::SplitPills,
                SurfaceMode::ClearTint,
                196,
                true,
                TaskbarVizStyle::CyberScan,
                false,
            ),
            TaskbarTheme::Retro95Classic => (
                PillMode::FullBar,
                SurfaceMode::ClearTint,
                235,
                false,
                TaskbarVizStyle::Off,
                false,
            ),
            TaskbarTheme::KineticCupertinoWave => (
                PillMode::SplitPills,
                SurfaceMode::Blur,
                62,
                true,
                TaskbarVizStyle::HoverMacWave,
                false,
            ),
            TaskbarTheme::KineticLiquidMercury => (
                PillMode::UnifiedPill,
                SurfaceMode::Acrylic,
                112,
                true,
                TaskbarVizStyle::HoverMercury,
                false,
            ),
            TaskbarTheme::KineticHydroChamber => (
                PillMode::UnifiedPill,
                SurfaceMode::Blur,
                74,
                true,
                TaskbarVizStyle::HoverHydroBubbles,
                false,
            ),
            TaskbarTheme::KineticQuantumCollider => (
                PillMode::UnifiedPill,
                SurfaceMode::Acrylic,
                122,
                true,
                TaskbarVizStyle::HoverQuantum,
                false,
            ),
            TaskbarTheme::KineticBiolumAbyss => (
                PillMode::UnifiedPill,
                SurfaceMode::ClearTint,
                88,
                true,
                TaskbarVizStyle::HoverBiolum,
                false,
            ),
            TaskbarTheme::KineticSynthTrampoline => (
                PillMode::UnifiedPill,
                SurfaceMode::Acrylic,
                138,
                true,
                TaskbarVizStyle::HoverSynthGrid,
                false,
            ),
            TaskbarTheme::HoverSnowfall => (
                PillMode::UnifiedPill,
                SurfaceMode::ClearTint,
                38,
                true,
                TaskbarVizStyle::HoverSnow,
                false,
            ),
            TaskbarTheme::HoverInferno => (
                PillMode::UnifiedPill,
                SurfaceMode::ClearTint,
                48,
                true,
                TaskbarVizStyle::HoverFire,
                false,
            ),
            TaskbarTheme::HoverWaterRipples => (
                PillMode::UnifiedPill,
                SurfaceMode::Blur,
                42,
                true,
                TaskbarVizStyle::HoverWater,
                false,
            ),
            TaskbarTheme::HoverSakuraWind => (
                PillMode::UnifiedPill,
                SurfaceMode::ClearTint,
                34,
                true,
                TaskbarVizStyle::HoverSakura,
                false,
            ),
            TaskbarTheme::HoverNebulaStardust => (
                PillMode::UnifiedPill,
                SurfaceMode::Acrylic,
                56,
                true,
                TaskbarVizStyle::HoverCosmic,
                false,
            ),
            TaskbarTheme::HoverNeonPlasma => (
                PillMode::SplitPills,
                SurfaceMode::ClearTint,
                52,
                true,
                TaskbarVizStyle::HoverPlasma,
                false,
            ),
            TaskbarTheme::AutumnCrystal => (
                PillMode::UnifiedPill,
                SurfaceMode::ClearTint,
                0,
                true,
                TaskbarVizStyle::Off,
                false,
            ),
            TaskbarTheme::WallpaperGlass => (
                PillMode::UnifiedPill,
                SurfaceMode::ClearTint,
                22,
                true,
                TaskbarVizStyle::AmbientGlow,
                false,
            ),
            TaskbarTheme::FrostedSplit => (
                PillMode::SplitPills,
                SurfaceMode::Blur,
                96,
                false,
                TaskbarVizStyle::EdgePulse,
                false,
            ),
            TaskbarTheme::AuroraBorealis => (
                PillMode::UnifiedPill,
                SurfaceMode::ClearTint,
                36,
                true,
                TaskbarVizStyle::AuroraFlow,
                false,
            ),
            TaskbarTheme::CyberNeonDock => (
                PillMode::UnifiedPill,
                SurfaceMode::Acrylic,
                72,
                true,
                TaskbarVizStyle::CyberScan,
                false,
            ),
            TaskbarTheme::SunsetEmber => (
                PillMode::SplitPills,
                SurfaceMode::ClearTint,
                40,
                true,
                TaskbarVizStyle::EmberDrift,
                false,
            ),
            TaskbarTheme::LiquidChrome => (
                PillMode::UnifiedPill,
                SurfaceMode::Blur,
                64,
                true,
                TaskbarVizStyle::ChromeSweep,
                false,
            ),
            TaskbarTheme::StarlightMinimal => (
                PillMode::UnifiedPill,
                SurfaceMode::ClearTint,
                16,
                false,
                TaskbarVizStyle::StarlightPulse,
                false,
            ),
            TaskbarTheme::LiveMusicBars => (
                PillMode::UnifiedPill,
                SurfaceMode::ClearTint,
                48,
                true,
                TaskbarVizStyle::EqualizerBars,
                false,
            ),
            TaskbarTheme::NeonWave => (
                PillMode::UnifiedPill,
                SurfaceMode::ClearTint,
                52,
                true,
                TaskbarVizStyle::AuroraWave,
                true,
            ),
            TaskbarTheme::PureClearPulse => (
                PillMode::FullBar,
                SurfaceMode::ClearTint,
                0,
                false,
                TaskbarVizStyle::EqualizerBars,
                false,
            ),
            TaskbarTheme::MirrorBeatPill => (
                PillMode::UnifiedPill,
                SurfaceMode::Acrylic,
                58,
                true,
                TaskbarVizStyle::EqualizerBars,
                true,
            ),
            TaskbarTheme::BassPulseSplit => (
                PillMode::SplitPills,
                SurfaceMode::Blur,
                76,
                true,
                TaskbarVizStyle::EdgePulse,
                true,
            ),
            TaskbarTheme::VuPeakStudio => (
                PillMode::FullBar,
                SurfaceMode::ClearTint,
                68,
                true,
                TaskbarVizStyle::EqualizerBars,
                false,
            ),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ClockThemeFolder {
    All,
    Horology,
    Realistic,
    SignatureSerif,
    MinimalIsland,
    CyberPoster,
}

impl ClockThemeFolder {
    #[allow(dead_code)]
    pub const ALL: [ClockThemeFolder; 6] = [
        ClockThemeFolder::All,
        ClockThemeFolder::Horology,
        ClockThemeFolder::Realistic,
        ClockThemeFolder::SignatureSerif,
        ClockThemeFolder::MinimalIsland,
        ClockThemeFolder::CyberPoster,
    ];

    pub fn label(self) -> &'static str {
        match self {
            ClockThemeFolder::All => "All Clocks (40)",
            ClockThemeFolder::Horology => "Photorealistic Horology (10)",
            ClockThemeFolder::Realistic => "Realistic & Calm (12)",
            ClockThemeFolder::SignatureSerif => "Signature & Art (6)",
            ClockThemeFolder::MinimalIsland => "Analog & Orbital (6)",
            ClockThemeFolder::CyberPoster => "Flip, Nixie & Tech (6)",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ClockStyle {
    Signature,
    Minimal,
    Editorial,
    Cyber,
    SplitStack,
    Island,
    TypoWatermark,
    BauhausGeo,
    JapaneseZen,
    ArtDecoGilded,
    GlassRibbon,
    ChronoDial,
    OrbitalEclipse,
    SunDialHorizon,
    FlipMech,
    NixieTube,
    RetroCasio,
    MatrixBinary,
    // Realistic & Calm Materials (12 True Realism PBR clocks)
    SwissChronometer,
    BraunIndustrial,
    WaterCausticsClock,
    ZenMistClock,
    LeatherFieldClock,
    WalnutDeskClock,
    RainWindowClock,
    AbyssalTrenchClock,
    GlacierIceClock,
    CarbonSteelClock,
    GoldenHourClock,
    AlpineMistClock,
    // Photorealistic Haute Horlogerie Collection (10 bespoke mechanical timepieces)
    HoroRoyalOak,
    HoroPatekMoon,
    HoroTourbillonGold,
    HoroDaytonaCeramic,
    HoroSubmarinerEmerald,
    HoroIwcTitanium,
    HoroBreguetPorcelain,
    HoroForgedCarbon,
    HoroWalnutRegulator,
    HoroSapphireObsidian,
}

impl ClockStyle {
    pub const ALL: [ClockStyle; 40] = [
        ClockStyle::HoroRoyalOak,
        ClockStyle::HoroPatekMoon,
        ClockStyle::HoroTourbillonGold,
        ClockStyle::HoroDaytonaCeramic,
        ClockStyle::HoroSubmarinerEmerald,
        ClockStyle::HoroIwcTitanium,
        ClockStyle::HoroBreguetPorcelain,
        ClockStyle::HoroForgedCarbon,
        ClockStyle::HoroWalnutRegulator,
        ClockStyle::HoroSapphireObsidian,
        ClockStyle::WaterCausticsClock,
        ClockStyle::RainWindowClock,
        ClockStyle::BraunIndustrial,
        ClockStyle::WalnutDeskClock,
        ClockStyle::AbyssalTrenchClock,
        ClockStyle::GlacierIceClock,
        ClockStyle::LeatherFieldClock,
        ClockStyle::AlpineMistClock,
        ClockStyle::CarbonSteelClock,
        ClockStyle::GoldenHourClock,
        ClockStyle::SwissChronometer,
        ClockStyle::ZenMistClock,
        ClockStyle::Signature,
        ClockStyle::ChronoDial,
        ClockStyle::FlipMech,
        ClockStyle::GlassRibbon,
        ClockStyle::ArtDecoGilded,
        ClockStyle::NixieTube,
        ClockStyle::OrbitalEclipse,
        ClockStyle::JapaneseZen,
        ClockStyle::RetroCasio,
        ClockStyle::BauhausGeo,
        ClockStyle::SunDialHorizon,
        ClockStyle::MatrixBinary,
        ClockStyle::TypoWatermark,
        ClockStyle::Editorial,
        ClockStyle::Minimal,
        ClockStyle::Island,
        ClockStyle::SplitStack,
        ClockStyle::Cyber,
    ];

    pub const HOROLOGY_FOLDER: [ClockStyle; 10] = [
        ClockStyle::HoroRoyalOak,
        ClockStyle::HoroPatekMoon,
        ClockStyle::HoroTourbillonGold,
        ClockStyle::HoroDaytonaCeramic,
        ClockStyle::HoroSubmarinerEmerald,
        ClockStyle::HoroIwcTitanium,
        ClockStyle::HoroBreguetPorcelain,
        ClockStyle::HoroForgedCarbon,
        ClockStyle::HoroWalnutRegulator,
        ClockStyle::HoroSapphireObsidian,
    ];

    pub const REALISTIC_FOLDER: [ClockStyle; 12] = [
        ClockStyle::WaterCausticsClock,
        ClockStyle::RainWindowClock,
        ClockStyle::BraunIndustrial,
        ClockStyle::WalnutDeskClock,
        ClockStyle::AbyssalTrenchClock,
        ClockStyle::GlacierIceClock,
        ClockStyle::LeatherFieldClock,
        ClockStyle::AlpineMistClock,
        ClockStyle::CarbonSteelClock,
        ClockStyle::GoldenHourClock,
        ClockStyle::SwissChronometer,
        ClockStyle::ZenMistClock,
    ];

    pub fn horology_theme(self) -> Option<crate::horology_pbr::HorologyTheme> {
        use crate::horology_pbr::HorologyTheme;
        match self {
            ClockStyle::HoroRoyalOak => Some(HorologyTheme::RoyalOakSteel),
            ClockStyle::HoroPatekMoon => Some(HorologyTheme::PatekMoonphase),
            ClockStyle::HoroTourbillonGold => Some(HorologyTheme::TourbillonGold),
            ClockStyle::HoroDaytonaCeramic => Some(HorologyTheme::DaytonaCeramic),
            ClockStyle::HoroSubmarinerEmerald => Some(HorologyTheme::SubmarinerEmerald),
            ClockStyle::HoroIwcTitanium => Some(HorologyTheme::IwcPilotTitanium),
            ClockStyle::HoroBreguetPorcelain => Some(HorologyTheme::BreguetPorcelain),
            ClockStyle::HoroForgedCarbon => Some(HorologyTheme::ForgedCarbonMoto),
            ClockStyle::HoroWalnutRegulator => Some(HorologyTheme::WalnutRegulator),
            ClockStyle::HoroSapphireObsidian => Some(HorologyTheme::SapphireObsidian),
            _ => None,
        }
    }

    pub fn pbr_material(self) -> Option<crate::realistic_pbr::PbrMaterial> {
        use crate::realistic_pbr::PbrMaterial;
        match self {
            ClockStyle::WaterCausticsClock => Some(PbrMaterial::CalmOceanLagoon),
            ClockStyle::RainWindowClock => Some(PbrMaterial::RainyWindowpane),
            ClockStyle::BraunIndustrial => Some(PbrMaterial::BrushedTitanium),
            ClockStyle::WalnutDeskClock | ClockStyle::SwissChronometer => {
                Some(PbrMaterial::VintageWalnutBrass)
            }
            ClockStyle::AbyssalTrenchClock => Some(PbrMaterial::DeepAbyssalTrench),
            ClockStyle::GlacierIceClock => Some(PbrMaterial::NordicGlacialIce),
            ClockStyle::LeatherFieldClock => Some(PbrMaterial::ExecutiveLeatherLinen),
            ClockStyle::AlpineMistClock | ClockStyle::ZenMistClock => {
                Some(PbrMaterial::AlpineForestMist)
            }
            ClockStyle::CarbonSteelClock => Some(PbrMaterial::IndustrialCarbonSteel),
            ClockStyle::GoldenHourClock => Some(PbrMaterial::GoldenHourHorizon),
            _ => None,
        }
    }

    pub fn is_realistic_interactive(self) -> bool {
        self.horology_theme().is_some() || self.pbr_material().is_some()
    }

    pub fn has_idle_motion(self) -> bool {
        self.horology_theme()
            .map(|h| h.has_idle_motion())
            .unwrap_or(false)
            || self
                .pbr_material()
                .map(|m| m.has_ambient_idle_motion())
                .unwrap_or(false)
    }

    pub const SIG_SERIF_FOLDER: [ClockStyle; 6] = [
        ClockStyle::Signature,
        ClockStyle::Editorial,
        ClockStyle::TypoWatermark,
        ClockStyle::BauhausGeo,
        ClockStyle::JapaneseZen,
        ClockStyle::ArtDecoGilded,
    ];
    pub const MIN_ISLAND_FOLDER: [ClockStyle; 6] = [
        ClockStyle::Minimal,
        ClockStyle::Island,
        ClockStyle::GlassRibbon,
        ClockStyle::ChronoDial,
        ClockStyle::OrbitalEclipse,
        ClockStyle::SunDialHorizon,
    ];
    pub const CYBER_POSTER_FOLDER: [ClockStyle; 6] = [
        ClockStyle::FlipMech,
        ClockStyle::NixieTube,
        ClockStyle::RetroCasio,
        ClockStyle::MatrixBinary,
        ClockStyle::SplitStack,
        ClockStyle::Cyber,
    ];

    pub fn in_folder(folder: ClockThemeFolder) -> &'static [ClockStyle] {
        match folder {
            ClockThemeFolder::All => &Self::ALL,
            ClockThemeFolder::Horology => &Self::HOROLOGY_FOLDER,
            ClockThemeFolder::Realistic => &Self::REALISTIC_FOLDER,
            ClockThemeFolder::SignatureSerif => &Self::SIG_SERIF_FOLDER,
            ClockThemeFolder::MinimalIsland => &Self::MIN_ISLAND_FOLDER,
            ClockThemeFolder::CyberPoster => &Self::CYBER_POSTER_FOLDER,
        }
    }

    pub fn folder_badge(self) -> &'static str {
        if let Some(ht) = self.horology_theme() {
            return ht.badge();
        }
        match self {
            ClockStyle::SwissChronometer
            | ClockStyle::BraunIndustrial
            | ClockStyle::WaterCausticsClock
            | ClockStyle::ZenMistClock
            | ClockStyle::LeatherFieldClock
            | ClockStyle::WalnutDeskClock
            | ClockStyle::RainWindowClock
            | ClockStyle::AbyssalTrenchClock
            | ClockStyle::GlacierIceClock
            | ClockStyle::CarbonSteelClock
            | ClockStyle::GoldenHourClock
            | ClockStyle::AlpineMistClock => "Realistic",
            ClockStyle::Signature
            | ClockStyle::Editorial
            | ClockStyle::TypoWatermark
            | ClockStyle::BauhausGeo
            | ClockStyle::JapaneseZen
            | ClockStyle::ArtDecoGilded => "Fine Art",
            ClockStyle::Minimal
            | ClockStyle::Island
            | ClockStyle::GlassRibbon
            | ClockStyle::ChronoDial
            | ClockStyle::OrbitalEclipse
            | ClockStyle::SunDialHorizon => "Analog & Glass",
            ClockStyle::FlipMech
            | ClockStyle::NixieTube
            | ClockStyle::RetroCasio
            | ClockStyle::MatrixBinary
            | ClockStyle::Cyber
            | ClockStyle::SplitStack => "Mech & Tech",
            _ => "Horology",
        }
    }

    pub fn uses_standard_plate(self) -> bool {
        matches!(
            self,
            ClockStyle::Signature
                | ClockStyle::Minimal
                | ClockStyle::Editorial
                | ClockStyle::Cyber
                | ClockStyle::SplitStack
                | ClockStyle::Island
                | ClockStyle::TypoWatermark
                | ClockStyle::JapaneseZen
        )
    }

    pub fn as_str(self) -> &'static str {
        if let Some(ht) = self.horology_theme() {
            return ht.as_str();
        }
        match self {
            ClockStyle::Signature => "signature",
            ClockStyle::Minimal => "minimal",
            ClockStyle::Editorial => "editorial",
            ClockStyle::Cyber => "cyber",
            ClockStyle::SplitStack => "split_stack",
            ClockStyle::Island => "island",
            ClockStyle::TypoWatermark => "typo_watermark",
            ClockStyle::BauhausGeo => "bauhaus_geo",
            ClockStyle::JapaneseZen => "japanese_zen",
            ClockStyle::ArtDecoGilded => "art_deco_gilded",
            ClockStyle::GlassRibbon => "glass_ribbon",
            ClockStyle::ChronoDial => "chrono_dial",
            ClockStyle::OrbitalEclipse => "orbital_eclipse",
            ClockStyle::SunDialHorizon => "sundial_horizon",
            ClockStyle::FlipMech => "flip_mech",
            ClockStyle::NixieTube => "nixie_tube",
            ClockStyle::RetroCasio => "retro_casio",
            ClockStyle::MatrixBinary => "matrix_binary",
            ClockStyle::SwissChronometer => "swiss_chronometer",
            ClockStyle::BraunIndustrial => "braun_industrial",
            ClockStyle::WaterCausticsClock => "water_caustics",
            ClockStyle::ZenMistClock => "zen_mist_clock",
            ClockStyle::LeatherFieldClock => "leather_field",
            ClockStyle::WalnutDeskClock => "walnut_desk",
            ClockStyle::RainWindowClock => "rain_window_clock",
            ClockStyle::AbyssalTrenchClock => "abyssal_trench_clock",
            ClockStyle::GlacierIceClock => "glacier_ice_clock",
            ClockStyle::CarbonSteelClock => "carbon_steel_clock",
            ClockStyle::GoldenHourClock => "golden_hour_clock",
            ClockStyle::AlpineMistClock => "alpine_mist_clock",
            _ => "horo_royal_oak",
        }
    }

    pub fn label(self) -> &'static str {
        if let Some(ht) = self.horology_theme() {
            return ht.label();
        }
        match self {
            ClockStyle::Signature => "Autumn Signature",
            ClockStyle::Minimal => "Nordic Minimal",
            ClockStyle::Editorial => "Editorial Serif",
            ClockStyle::Cyber => "Cyber HUD Deck",
            ClockStyle::SplitStack => "Big Typo Stack",
            ClockStyle::Island => "Floating Island",
            ClockStyle::TypoWatermark => "Ghost Watermark",
            ClockStyle::BauhausGeo => "Bauhaus Geometry",
            ClockStyle::JapaneseZen => "Hanko Zen Seal",
            ClockStyle::ArtDecoGilded => "Gatsby Deco Arch",
            ClockStyle::GlassRibbon => "Dual Medallion",
            ClockStyle::ChronoDial => "Chronograph Watch",
            ClockStyle::OrbitalEclipse => "Eclipse Orrery",
            ClockStyle::SunDialHorizon => "Solar Meridian Arc",
            ClockStyle::FlipMech => "Split-Flap Board",
            ClockStyle::NixieTube => "Nixie Tube Bank",
            ClockStyle::RetroCasio => "1984 Quartz LCD",
            ClockStyle::MatrixBinary => "Binary BCD Matrix",
            ClockStyle::SwissChronometer => "Swiss Horology Dial",
            ClockStyle::BraunIndustrial => "Brushed Titanium Hi-Fi",
            ClockStyle::WaterCausticsClock => "Calm Ocean Lagoon",
            ClockStyle::ZenMistClock => "Zen Morning Mist",
            ClockStyle::LeatherFieldClock => "Executive Leather & Linen",
            ClockStyle::WalnutDeskClock => "Vintage Walnut & Brass",
            ClockStyle::RainWindowClock => "Rainy Windowpane",
            ClockStyle::AbyssalTrenchClock => "Deep Abyssal Trench",
            ClockStyle::GlacierIceClock => "Nordic Glacial Ice",
            ClockStyle::CarbonSteelClock => "Matte Carbon & Steel",
            ClockStyle::GoldenHourClock => "Golden Hour Horizon",
            ClockStyle::AlpineMistClock => "Alpine Forest Mist",
            _ => "Royal Oak Guilloché Steel",
        }
    }

    pub fn subtitle(self) -> &'static str {
        if let Some(ht) = self.horology_theme() {
            return ht.subtitle();
        }
        match self {
            ClockStyle::Signature => "Bold day • Script • Spine",
            ClockStyle::Minimal => "Swiss clean horizontal",
            ClockStyle::Editorial => "Classic serif & calligraphy",
            ClockStyle::Cyber => "Tech segmented • HUD ticks",
            ClockStyle::SplitStack => "Stacked poster numerals",
            ClockStyle::Island => "Ultra-slim glass capsule",
            ClockStyle::TypoWatermark => "Colossal ghost day • Sharp time",
            ClockStyle::BauhausGeo => "Sun orb • Crosshair studio grid",
            ClockStyle::JapaneseZen => "Vermilion stamp • Ink underline",
            ClockStyle::ArtDecoGilded => "Stepped brass arch • Sunburst",
            ClockStyle::GlassRibbon => "Detached 60s ring + pill deck",
            ClockStyle::ChronoDial => "Live swept dial + sapphire readout",
            ClockStyle::OrbitalEclipse => "Planetary rings • Live moon orbit",
            ClockStyle::SunDialHorizon => "180° golden solar horizon gauge",
            ClockStyle::FlipMech => "3D hinged split-flap tiles",
            ClockStyle::NixieTube => "Warm cathode tubes on mahogany",
            ClockStyle::RetroCasio => "Octagonal watch bezel • LCD ghost",
            ClockStyle::MatrixBinary => "Live 6x4 BCD binary LED deck",
            ClockStyle::SwissChronometer => "Sunburst guilloché • Sapphire AR & blued hands",
            ClockStyle::BraunIndustrial => "Anisotropic titanium • Torx bolts & cursor glare",
            ClockStyle::WaterCausticsClock => "Voronoi water caustics • Live click/hover ripples",
            ClockStyle::ZenMistClock => "Serene sage mist • Breathing calm horizon",
            ClockStyle::LeatherFieldClock => "Pebble leather • Stitched ivory linen & rivets",
            ClockStyle::WalnutDeskClock => "Cathedral walnut grain • Slotted brass screws",
            ClockStyle::RainWindowClock => "Wet twilight glass • Interactive wiper & droplets",
            ClockStyle::AbyssalTrenchClock => "Bioluminescent plankton • Sonar click ripples",
            ClockStyle::GlacierIceClock => "Faceted glacial crystal • 45° chromatic prism",
            ClockStyle::CarbonSteelClock => "2x2 twill carbon weave • Hex steel & orange HUD",
            ClockStyle::GoldenHourClock => "Coastal dusk glare • Solar water shimmer track",
            ClockStyle::AlpineMistClock => "Volumetric pine mist • Sunbeams & drifting pollen",
            _ => "Octagonal 316L steel • 8 hex screws & tapisserie waffle dial",
        }
    }

    pub fn from_str(s: &str) -> Self {
        if let Some(ht) = crate::horology_pbr::HorologyTheme::from_str(s) {
            return Self::HOROLOGY_FOLDER[ht.idx()];
        }
        match s.trim().to_ascii_lowercase().as_str() {
            "minimal" | "nordic" => ClockStyle::Minimal,
            "editorial" | "serif" => ClockStyle::Editorial,
            "cyber" | "hud" => ClockStyle::Cyber,
            "split_stack" | "stack" | "poster" => ClockStyle::SplitStack,
            "island" | "pill" => ClockStyle::Island,
            "typo_watermark" | "watermark" => ClockStyle::TypoWatermark,
            "bauhaus_geo" | "bauhaus" => ClockStyle::BauhausGeo,
            "japanese_zen" | "zen" | "hanko" => ClockStyle::JapaneseZen,
            "art_deco_gilded" | "deco" | "gatsby" => ClockStyle::ArtDecoGilded,
            "glass_ribbon" | "medallion" | "ribbon" => ClockStyle::GlassRibbon,
            "chrono_dial" | "chrono" | "watch" => ClockStyle::ChronoDial,
            "orbital_eclipse" | "orbital" | "eclipse" => ClockStyle::OrbitalEclipse,
            "sundial_horizon" | "sundial" | "horizon" => ClockStyle::SunDialHorizon,
            "flip_mech" | "flip" => ClockStyle::FlipMech,
            "nixie_tube" | "nixie" => ClockStyle::NixieTube,
            "retro_casio" | "casio" | "lcd" => ClockStyle::RetroCasio,
            "matrix_binary" | "binary" | "bcd" => ClockStyle::MatrixBinary,
            "swiss_chronometer" | "swiss_chrono" => ClockStyle::SwissChronometer,
            "braun_industrial" | "braun" | "titanium_clock" => ClockStyle::BraunIndustrial,
            "water_caustics" | "water_clock" => ClockStyle::WaterCausticsClock,
            "zen_mist_clock" | "zen_mist" | "calm_clock" => ClockStyle::ZenMistClock,
            "leather_field" | "leather_clock" => ClockStyle::LeatherFieldClock,
            "walnut_desk" | "walnut_clock" => ClockStyle::WalnutDeskClock,
            "rain_window_clock" | "rain_clock" => ClockStyle::RainWindowClock,
            "abyssal_trench_clock" | "abyssal_clock" => ClockStyle::AbyssalTrenchClock,
            "glacier_ice_clock" | "glacier_clock" => ClockStyle::GlacierIceClock,
            "carbon_steel_clock" | "carbon_clock" => ClockStyle::CarbonSteelClock,
            "golden_hour_clock" | "golden_clock" => ClockStyle::GoldenHourClock,
            "alpine_mist_clock" | "alpine_clock" => ClockStyle::AlpineMistClock,
            _ => ClockStyle::Signature,
        }
    }

    pub fn default_fonts(self) -> (&'static str, &'static str, &'static str, &'static str) {
        if let Some(ht) = self.horology_theme() {
            return ht.default_fonts();
        }
        match self {
            ClockStyle::Signature => ("Morganite Bold", "Tuesday Night", "SF Pro Display", "SF Pro Display"),
            ClockStyle::Minimal => ("SF Pro Display", "SF Pro Display", "Segoe UI Light", "SF Pro Display"),
            ClockStyle::Editorial => ("Bodoni MT", "Edwardian Script ITC", "Georgia", "Palatino Linotype"),
            ClockStyle::Cyber => ("Cascadia Mono", "Cascadia Code", "Digital-7 Mono", "Consolas"),
            ClockStyle::SplitStack => ("Morganite Bold", "Tuesday Night", "Metropolis Extra Bold", "SF Pro Display"),
            ClockStyle::Island => ("SF Pro Display", "Tuesday Night", "SF Pro Display", "SF Pro Display"),
            ClockStyle::TypoWatermark => ("Bahnschrift SemiBold", "Tuesday Night", "Segoe UI Light", "SF Pro Display"),
            ClockStyle::BauhausGeo => ("Bahnschrift Bold", "Bahnschrift SemiBold", "Bahnschrift Bold", "Cascadia Mono"),
            ClockStyle::JapaneseZen => ("Georgia", "Tuesday Night", "Segoe UI Light", "SF Pro Display"),
            ClockStyle::ArtDecoGilded => ("Bodoni MT", "Edwardian Script ITC", "Georgia", "Palatino Linotype"),
            ClockStyle::GlassRibbon => ("Bahnschrift Bold", "Tuesday Night", "SF Pro Display", "SF Pro Display"),
            ClockStyle::ChronoDial => ("Bahnschrift Bold", "SF Pro Display", "Bahnschrift SemiBold", "Cascadia Mono"),
            ClockStyle::OrbitalEclipse => ("Bahnschrift Light", "Tuesday Night", "Segoe UI Light", "SF Pro Display"),
            ClockStyle::SunDialHorizon => ("Georgia", "Tuesday Night", "Segoe UI Light", "SF Pro Display"),
            ClockStyle::FlipMech => ("Bahnschrift Bold", "SF Pro Display", "Bahnschrift Bold", "Cascadia Mono"),
            ClockStyle::NixieTube => ("Bahnschrift Light", "Tuesday Night", "Segoe UI Light", "Cascadia Mono"),
            ClockStyle::RetroCasio => ("Cascadia Mono", "Cascadia Mono", "Cascadia Mono", "Consolas"),
            ClockStyle::MatrixBinary => ("Cascadia Mono", "Cascadia Code", "Cascadia Mono", "Consolas"),
            ClockStyle::SwissChronometer => ("Bodoni MT", "Georgia", "Bahnschrift SemiBold", "Palatino Linotype"),
            ClockStyle::BraunIndustrial => ("Bahnschrift SemiBold", "Bahnschrift Light", "Bahnschrift Bold", "Cascadia Mono"),
            ClockStyle::WaterCausticsClock => ("Segoe UI Light", "Tuesday Night", "SF Pro Display", "Segoe UI"),
            ClockStyle::ZenMistClock => ("Georgia", "Tuesday Night", "Segoe UI Light", "Segoe UI"),
            ClockStyle::LeatherFieldClock => ("Georgia", "Edwardian Script ITC", "Palatino Linotype", "Bahnschrift SemiBold"),
            ClockStyle::WalnutDeskClock => ("Bodoni MT", "Georgia", "Segoe UI Semibold", "Palatino Linotype"),
            ClockStyle::RainWindowClock => ("Segoe UI Light", "Tuesday Night", "SF Pro Display", "Segoe UI"),
            ClockStyle::AbyssalTrenchClock => ("Bahnschrift Light", "Segoe UI Light", "SF Pro Display", "Cascadia Mono"),
            ClockStyle::GlacierIceClock => ("Segoe UI Light", "Georgia", "SF Pro Display", "Segoe UI"),
            ClockStyle::CarbonSteelClock => ("Bahnschrift SemiBold", "Cascadia Mono", "Bahnschrift Bold", "Cascadia Mono"),
            ClockStyle::GoldenHourClock => ("Georgia", "Tuesday Night", "SF Pro Display", "Palatino Linotype"),
            ClockStyle::AlpineMistClock => ("Georgia", "Tuesday Night", "Segoe UI Light", "Segoe UI"),
            _ => ("Bahnschrift SemiBold", "Georgia", "Bahnschrift Bold", "SF Pro Display"),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MusicThemeFolder {
    All,
    Horology,
    Realistic,
    GlassVinyl,
    SpectrumEq,
    AnalogHardware,
}

impl MusicThemeFolder {
    #[allow(dead_code)]
    pub const ALL: [MusicThemeFolder; 6] = [
        MusicThemeFolder::All,
        MusicThemeFolder::Horology,
        MusicThemeFolder::Realistic,
        MusicThemeFolder::GlassVinyl,
        MusicThemeFolder::SpectrumEq,
        MusicThemeFolder::AnalogHardware,
    ];

    pub fn label(self) -> &'static str {
        match self {
            MusicThemeFolder::All => "All Music (39)",
            MusicThemeFolder::Horology => "Photorealistic Horology (10)",
            MusicThemeFolder::Realistic => "Realistic & Calm (10)",
            MusicThemeFolder::GlassVinyl => "Vinyl & Art (7)",
            MusicThemeFolder::SpectrumEq => "Waves & Rings (6)",
            MusicThemeFolder::AnalogHardware => "Retro & Hi-Fi (6)",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MusicStyle {
    Sanctuary,
    VinylDeck,
    MinimalPill,
    SpectrumDeck,
    NeonWave,
    CyberHud,
    TurntablePro,
    DualFloatOrb,
    EditorialSleeve,
    RadialHalo,
    OscilloscopeCrt,
    RibbonFlow,
    EclipsePlayer,
    CassetteDeck,
    AnalogVuMeter,
    HifiAmplifier,
    WinampRack,
    FlipJukebox,
    // Realistic & Calm Materials (10 True Realism PBR audio decks)
    VacuumTubeAmp,
    BrushedTitaniumDeck,
    RainWindowPlayer,
    CalmWaterRipple,
    LeatherFieldAudio,
    WalnutHiFiConsole,
    AbyssalTrenchAudio,
    GlacierIceAudio,
    CarbonSteelDeck,
    GoldenHourAudio,
    DynamicIslandCapsule,
    // Photorealistic Haute Horlogerie Collection (10 bespoke horology turntable/VU decks)
    HoroRoyalOakDeck,
    HoroPatekMoonDeck,
    HoroTourbillonDeck,
    HoroDaytonaDeck,
    HoroSubmarinerDeck,
    HoroIwcTitaniumDeck,
    HoroBreguetDeck,
    HoroForgedCarbonDeck,
    HoroWalnutRegulatorDeck,
    HoroSapphireObsidianDeck,
}

impl MusicStyle {
    pub const ALL: [MusicStyle; 39] = [
        MusicStyle::HoroRoyalOakDeck,
        MusicStyle::HoroPatekMoonDeck,
        MusicStyle::HoroTourbillonDeck,
        MusicStyle::HoroDaytonaDeck,
        MusicStyle::HoroSubmarinerDeck,
        MusicStyle::HoroIwcTitaniumDeck,
        MusicStyle::HoroBreguetDeck,
        MusicStyle::HoroForgedCarbonDeck,
        MusicStyle::HoroWalnutRegulatorDeck,
        MusicStyle::HoroSapphireObsidianDeck,
        MusicStyle::CalmWaterRipple,
        MusicStyle::RainWindowPlayer,
        MusicStyle::BrushedTitaniumDeck,
        MusicStyle::WalnutHiFiConsole,
        MusicStyle::AbyssalTrenchAudio,
        MusicStyle::GlacierIceAudio,
        MusicStyle::LeatherFieldAudio,
        MusicStyle::CarbonSteelDeck,
        MusicStyle::GoldenHourAudio,
        MusicStyle::VacuumTubeAmp,
        MusicStyle::DynamicIslandCapsule,
        MusicStyle::Sanctuary,
        MusicStyle::TurntablePro,
        MusicStyle::CassetteDeck,
        MusicStyle::DualFloatOrb,
        MusicStyle::EclipsePlayer,
        MusicStyle::AnalogVuMeter,
        MusicStyle::EditorialSleeve,
        MusicStyle::RadialHalo,
        MusicStyle::FlipJukebox,
        MusicStyle::OscilloscopeCrt,
        MusicStyle::RibbonFlow,
        MusicStyle::HifiAmplifier,
        MusicStyle::WinampRack,
        MusicStyle::VinylDeck,
        MusicStyle::MinimalPill,
        MusicStyle::SpectrumDeck,
        MusicStyle::NeonWave,
        MusicStyle::CyberHud,
    ];

    pub const HOROLOGY_FOLDER: [MusicStyle; 10] = [
        MusicStyle::HoroRoyalOakDeck,
        MusicStyle::HoroPatekMoonDeck,
        MusicStyle::HoroTourbillonDeck,
        MusicStyle::HoroDaytonaDeck,
        MusicStyle::HoroSubmarinerDeck,
        MusicStyle::HoroIwcTitaniumDeck,
        MusicStyle::HoroBreguetDeck,
        MusicStyle::HoroForgedCarbonDeck,
        MusicStyle::HoroWalnutRegulatorDeck,
        MusicStyle::HoroSapphireObsidianDeck,
    ];

    pub const REALISTIC_FOLDER: [MusicStyle; 10] = [
        MusicStyle::CalmWaterRipple,
        MusicStyle::RainWindowPlayer,
        MusicStyle::BrushedTitaniumDeck,
        MusicStyle::WalnutHiFiConsole,
        MusicStyle::AbyssalTrenchAudio,
        MusicStyle::GlacierIceAudio,
        MusicStyle::LeatherFieldAudio,
        MusicStyle::CarbonSteelDeck,
        MusicStyle::GoldenHourAudio,
        MusicStyle::VacuumTubeAmp,
    ];

    pub fn horology_theme(self) -> Option<crate::horology_pbr::HorologyTheme> {
        use crate::horology_pbr::HorologyTheme;
        match self {
            MusicStyle::HoroRoyalOakDeck => Some(HorologyTheme::RoyalOakSteel),
            MusicStyle::HoroPatekMoonDeck => Some(HorologyTheme::PatekMoonphase),
            MusicStyle::HoroTourbillonDeck => Some(HorologyTheme::TourbillonGold),
            MusicStyle::HoroDaytonaDeck => Some(HorologyTheme::DaytonaCeramic),
            MusicStyle::HoroSubmarinerDeck => Some(HorologyTheme::SubmarinerEmerald),
            MusicStyle::HoroIwcTitaniumDeck => Some(HorologyTheme::IwcPilotTitanium),
            MusicStyle::HoroBreguetDeck => Some(HorologyTheme::BreguetPorcelain),
            MusicStyle::HoroForgedCarbonDeck => Some(HorologyTheme::ForgedCarbonMoto),
            MusicStyle::HoroWalnutRegulatorDeck => Some(HorologyTheme::WalnutRegulator),
            MusicStyle::HoroSapphireObsidianDeck => Some(HorologyTheme::SapphireObsidian),
            _ => None,
        }
    }

    pub fn pbr_material(self) -> Option<crate::realistic_pbr::PbrMaterial> {
        use crate::realistic_pbr::PbrMaterial;
        match self {
            MusicStyle::CalmWaterRipple => Some(PbrMaterial::CalmOceanLagoon),
            MusicStyle::RainWindowPlayer => Some(PbrMaterial::RainyWindowpane),
            MusicStyle::BrushedTitaniumDeck | MusicStyle::VacuumTubeAmp => {
                Some(PbrMaterial::BrushedTitanium)
            }
            MusicStyle::WalnutHiFiConsole => Some(PbrMaterial::VintageWalnutBrass),
            MusicStyle::AbyssalTrenchAudio => Some(PbrMaterial::DeepAbyssalTrench),
            MusicStyle::GlacierIceAudio => Some(PbrMaterial::NordicGlacialIce),
            MusicStyle::LeatherFieldAudio => Some(PbrMaterial::ExecutiveLeatherLinen),
            MusicStyle::CarbonSteelDeck => Some(PbrMaterial::IndustrialCarbonSteel),
            MusicStyle::GoldenHourAudio => Some(PbrMaterial::GoldenHourHorizon),
            _ => None,
        }
    }

    pub fn is_realistic_interactive(self) -> bool {
        self.horology_theme().is_some() || self.pbr_material().is_some()
    }

    pub fn has_idle_motion(self) -> bool {
        self.horology_theme()
            .map(|h| h.has_idle_motion())
            .unwrap_or(false)
            || self
                .pbr_material()
                .map(|m| m.has_ambient_idle_motion())
                .unwrap_or(false)
    }

    pub const GLASS_VINYL_FOLDER: [MusicStyle; 7] = [
        MusicStyle::DynamicIslandCapsule,
        MusicStyle::Sanctuary,
        MusicStyle::VinylDeck,
        MusicStyle::MinimalPill,
        MusicStyle::TurntablePro,
        MusicStyle::DualFloatOrb,
        MusicStyle::EditorialSleeve,
    ];

    pub const SPECTRUM_EQ_FOLDER: [MusicStyle; 6] = [
        MusicStyle::SpectrumDeck,
        MusicStyle::NeonWave,
        MusicStyle::RadialHalo,
        MusicStyle::OscilloscopeCrt,
        MusicStyle::RibbonFlow,
        MusicStyle::EclipsePlayer,
    ];

    pub const ANALOG_HARDWARE_FOLDER: [MusicStyle; 6] = [
        MusicStyle::CassetteDeck,
        MusicStyle::AnalogVuMeter,
        MusicStyle::HifiAmplifier,
        MusicStyle::WinampRack,
        MusicStyle::FlipJukebox,
        MusicStyle::CyberHud,
    ];

    pub fn in_folder(folder: MusicThemeFolder) -> &'static [MusicStyle] {
        match folder {
            MusicThemeFolder::All => &Self::ALL,
            MusicThemeFolder::Horology => &Self::HOROLOGY_FOLDER,
            MusicThemeFolder::Realistic => &Self::REALISTIC_FOLDER,
            MusicThemeFolder::GlassVinyl => &Self::GLASS_VINYL_FOLDER,
            MusicThemeFolder::SpectrumEq => &Self::SPECTRUM_EQ_FOLDER,
            MusicThemeFolder::AnalogHardware => &Self::ANALOG_HARDWARE_FOLDER,
        }
    }

    pub fn as_str(self) -> &'static str {
        if let Some(ht) = self.horology_theme() {
            return ht.as_str();
        }
        match self {
            MusicStyle::Sanctuary => "sanctuary",
            MusicStyle::VinylDeck => "vinyl_deck",
            MusicStyle::MinimalPill => "minimal_pill",
            MusicStyle::SpectrumDeck => "spectrum_deck",
            MusicStyle::NeonWave => "neon_wave",
            MusicStyle::CyberHud => "cyber_hud",
            MusicStyle::TurntablePro => "turntable_pro",
            MusicStyle::DualFloatOrb => "dual_float_orb",
            MusicStyle::EditorialSleeve => "editorial_sleeve",
            MusicStyle::RadialHalo => "radial_halo",
            MusicStyle::OscilloscopeCrt => "oscilloscope_crt",
            MusicStyle::RibbonFlow => "ribbon_flow",
            MusicStyle::EclipsePlayer => "eclipse_player",
            MusicStyle::CassetteDeck => "cassette_deck",
            MusicStyle::AnalogVuMeter => "analog_vu_meter",
            MusicStyle::HifiAmplifier => "hifi_amplifier",
            MusicStyle::WinampRack => "winamp_rack",
            MusicStyle::FlipJukebox => "flip_jukebox",
            MusicStyle::VacuumTubeAmp => "vacuum_tube_amp",
            MusicStyle::BrushedTitaniumDeck => "brushed_titanium_deck",
            MusicStyle::RainWindowPlayer => "rain_window_player",
            MusicStyle::CalmWaterRipple => "calm_water_ripple",
            MusicStyle::LeatherFieldAudio => "leather_field_audio",
            MusicStyle::WalnutHiFiConsole => "walnut_hifi_console",
            MusicStyle::AbyssalTrenchAudio => "abyssal_trench_audio",
            MusicStyle::GlacierIceAudio => "glacier_ice_audio",
            MusicStyle::CarbonSteelDeck => "carbon_steel_deck",
            MusicStyle::GoldenHourAudio => "golden_hour_audio",
            MusicStyle::DynamicIslandCapsule => "dynamic_island_capsule",
            _ => "horo_royal_oak",
        }
    }

    pub fn from_str(s: &str) -> Self {
        if let Some(ht) = crate::horology_pbr::HorologyTheme::from_str(s) {
            return Self::HOROLOGY_FOLDER[ht.idx()];
        }
        match s.trim().to_ascii_lowercase().as_str() {
            "vinyl_deck" | "vinyl" => MusicStyle::VinylDeck,
            "minimal_pill" | "pill" => MusicStyle::MinimalPill,
            "spectrum_deck" | "spectrum" => MusicStyle::SpectrumDeck,
            "neon_wave" | "wave" => MusicStyle::NeonWave,
            "cyber_hud" | "cyber" => MusicStyle::CyberHud,
            "turntable_pro" | "turntable" => MusicStyle::TurntablePro,
            "dual_float_orb" | "dual_orb" => MusicStyle::DualFloatOrb,
            "editorial_sleeve" | "sleeve" => MusicStyle::EditorialSleeve,
            "radial_halo" | "radial" => MusicStyle::RadialHalo,
            "oscilloscope_crt" | "oscilloscope" | "crt" => MusicStyle::OscilloscopeCrt,
            "ribbon_flow" | "ribbon" => MusicStyle::RibbonFlow,
            "eclipse_player" | "eclipse" => MusicStyle::EclipsePlayer,
            "cassette_deck" | "cassette" | "tape" => MusicStyle::CassetteDeck,
            "analog_vu_meter" | "vu" | "vu_meter" => MusicStyle::AnalogVuMeter,
            "hifi_amplifier" | "hifi" | "amp" => MusicStyle::HifiAmplifier,
            "winamp_rack" | "winamp" | "rack" => MusicStyle::WinampRack,
            "flip_jukebox" | "jukebox" | "flip" => MusicStyle::FlipJukebox,
            "vacuum_tube_amp" | "vacuum_tube" | "tube_amp" => MusicStyle::VacuumTubeAmp,
            "brushed_titanium_deck" | "titanium_deck" => MusicStyle::BrushedTitaniumDeck,
            "rain_window_player" | "rain_player" => MusicStyle::RainWindowPlayer,
            "calm_water_ripple" | "water_ripple" => MusicStyle::CalmWaterRipple,
            "leather_field_audio" | "leather_audio" => MusicStyle::LeatherFieldAudio,
            "walnut_hifi_console" | "walnut_hifi" => MusicStyle::WalnutHiFiConsole,
            "abyssal_trench_audio" | "abyssal_audio" => MusicStyle::AbyssalTrenchAudio,
            "glacier_ice_audio" | "glacier_audio" => MusicStyle::GlacierIceAudio,
            "carbon_steel_deck" | "carbon_deck" => MusicStyle::CarbonSteelDeck,
            "golden_hour_audio" | "golden_audio" => MusicStyle::GoldenHourAudio,
            "dynamic_island_capsule" | "island_capsule" | "dynamic_island" => MusicStyle::DynamicIslandCapsule,
            _ => MusicStyle::Sanctuary,
        }
    }

    pub fn label(self) -> &'static str {
        if let Some(ht) = self.horology_theme() {
            return ht.label();
        }
        match self {
            MusicStyle::Sanctuary => "Autumn Sanctuary",
            MusicStyle::VinylDeck => "Spinning Vinyl Deck",
            MusicStyle::MinimalPill => "Floating Glass Pill",
            MusicStyle::SpectrumDeck => "Live Spectrum Bar",
            MusicStyle::NeonWave => "Neon Wave Studio",
            MusicStyle::CyberHud => "Cyber HUD Player",
            MusicStyle::TurntablePro => "Pro Plinth Turntable",
            MusicStyle::DualFloatOrb => "Dual Orbit & Pill",
            MusicStyle::EditorialSleeve => "Vinyl Jacket Peek",
            MusicStyle::RadialHalo => "Radial Halo 360°",
            MusicStyle::OscilloscopeCrt => "Oscilloscope CRT",
            MusicStyle::RibbonFlow => "Symmetry Wave Flow",
            MusicStyle::EclipsePlayer => "Solar Eclipse Wing",
            MusicStyle::CassetteDeck => "1984 Chrome Cassette",
            MusicStyle::AnalogVuMeter => "Twin Amber VU Meter",
            MusicStyle::HifiAmplifier => "Anodized Hi-Fi Amp",
            MusicStyle::WinampRack => "Studio Micro Rack",
            MusicStyle::FlipJukebox => "Split-Flap Jukebox",
            MusicStyle::VacuumTubeAmp => "12AX7 Vacuum Tube Amp",
            MusicStyle::BrushedTitaniumDeck => "Brushed Titanium Hi-Fi",
            MusicStyle::RainWindowPlayer => "Rainy Windowpane",
            MusicStyle::CalmWaterRipple => "Calm Ocean Lagoon",
            MusicStyle::LeatherFieldAudio => "Executive Leather Deck",
            MusicStyle::WalnutHiFiConsole => "Vintage Walnut Hi-Fi",
            MusicStyle::AbyssalTrenchAudio => "Deep Abyssal Sonar",
            MusicStyle::GlacierIceAudio => "Nordic Glacial Prism",
            MusicStyle::CarbonSteelDeck => "Matte Carbon Audio",
            MusicStyle::GoldenHourAudio => "Golden Hour Horizon",
            MusicStyle::DynamicIslandCapsule => "Dynamic Island Pill",
            _ => "Royal Oak Guilloché Steel",
        }
    }

    pub fn subtitle(self) -> &'static str {
        if let Some(ht) = self.horology_theme() {
            return ht.subtitle();
        }
        match self {
            MusicStyle::Sanctuary => "Warm crystal card • Squircle art & 12-band EQ",
            MusicStyle::VinylDeck => "Turntable vinyl disc • Golden tonearm accent",
            MusicStyle::MinimalPill => "Ultra-clean capsule • glowing transport ring",
            MusicStyle::SpectrumDeck => "Full-card 24-band equalizer behind track info",
            MusicStyle::NeonWave => "Dual-ribbon audio waveform streaming behind text",
            MusicStyle::CyberHud => "Tech HUD brackets • LED peak meter & neon frame",
            MusicStyle::TurntablePro => "Protruding LP platter • S-tonearm & brushed plinth",
            MusicStyle::DualFloatOrb => "Detached radial EQ cover orb + capsule console",
            MusicStyle::EditorialSleeve => "Collector square sleeve + half-pulled vinyl disc",
            MusicStyle::RadialHalo => "Open 360° reactive sound rings + frosted console",
            MusicStyle::OscilloscopeCrt => "Circular Lissajous scope • Live harmonic loop",
            MusicStyle::RibbonFlow => "Mirrored upper/lower stereo audio horizon",
            MusicStyle::EclipsePlayer => "Twin glass wings bisected by central cover disc",
            MusicStyle::CassetteDeck => "Spinning spoked reels • Paper label & tape bridge",
            MusicStyle::AnalogVuMeter => "Twin backlit L/R amber needle VU meters",
            MusicStyle::HifiAmplifier => "Machined console • LED gain ladder & tactile keys",
            MusicStyle::WinampRack => "2-tier studio rack • Peak-hold analyzer & readout",
            MusicStyle::FlipJukebox => "Hinged split-flap song tiles & framed art card",
            MusicStyle::VacuumTubeAmp => "Glowing glass triodes • Champagne brass & VU",
            MusicStyle::BrushedTitaniumDeck => "Anisotropic CNC titanium • Ballistic VU & bolts",
            MusicStyle::RainWindowPlayer => "Optical raindrops • Interactive wiper & twilight bokeh",
            MusicStyle::CalmWaterRipple => "Voronoi water caustics • Audio & cursor ripples",
            MusicStyle::LeatherFieldAudio => "Pebble leather • Waxed stitches & brass meters",
            MusicStyle::WalnutHiFiConsole => "Cathedral black walnut • Slotted brass & analog VU",
            MusicStyle::AbyssalTrenchAudio => "Bioluminescent sonar • Deep trench audio wave",
            MusicStyle::GlacierIceAudio => "Crystalline ice • 45° chromatic prism spectrum",
            MusicStyle::CarbonSteelDeck => "2x2 twill carbon • Stainless hex bolts & orange EQ",
            MusicStyle::GoldenHourAudio => "Coastal dusk glare • Shimmering solar audio wave",
            MusicStyle::DynamicIslandCapsule => "Frosted smoked capsule • squircle art, controls & live EQ",
            _ => "Sapphire turntable bezel • Twin analog VU dials & crown keys",
        }
    }

    pub fn badge(self) -> &'static str {
        if let Some(ht) = self.horology_theme() {
            return ht.badge();
        }
        match self {
            MusicStyle::Sanctuary => "Classic",
            MusicStyle::VinylDeck => "Vinyl",
            MusicStyle::MinimalPill => "Capsule",
            MusicStyle::SpectrumDeck => "Full EQ",
            MusicStyle::NeonWave => "Wave",
            MusicStyle::CyberHud => "Cyber",
            MusicStyle::TurntablePro => "Plinth",
            MusicStyle::DualFloatOrb => "Dual",
            MusicStyle::EditorialSleeve => "Sleeve",
            MusicStyle::RadialHalo => "360°",
            MusicStyle::OscilloscopeCrt => "Scope",
            MusicStyle::RibbonFlow => "Mirror",
            MusicStyle::EclipsePlayer => "Eclipse",
            MusicStyle::CassetteDeck => "Retro",
            MusicStyle::AnalogVuMeter => "VU L/R",
            MusicStyle::HifiAmplifier => "Hi-Fi",
            MusicStyle::WinampRack => "Rack",
            MusicStyle::FlipJukebox => "Flip",
            MusicStyle::VacuumTubeAmp => "Tube",
            MusicStyle::BrushedTitaniumDeck => "CNC Ti",
            MusicStyle::RainWindowPlayer => "Rain",
            MusicStyle::CalmWaterRipple => "Lagoon",
            MusicStyle::LeatherFieldAudio => "Leather",
            MusicStyle::WalnutHiFiConsole => "Walnut",
            MusicStyle::AbyssalTrenchAudio => "Abyssal",
            MusicStyle::GlacierIceAudio => "Glacial",
            MusicStyle::CarbonSteelDeck => "Carbon",
            MusicStyle::GoldenHourAudio => "Sunset",
            MusicStyle::DynamicIslandCapsule => "Island",
            _ => "Horology",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VitalsThemeFolder {
    All,
    Horology,
    Classic,
}

impl VitalsThemeFolder {
    pub fn label(self) -> &'static str {
        match self {
            VitalsThemeFolder::All => "All Vitals (15)",
            VitalsThemeFolder::Horology => "Photorealistic Horology (10)",
            VitalsThemeFolder::Classic => "Classic & HUD (5)",
        }
    }
}

pub const VITALS_HOROLOGY_STYLES: [u8; 10] = [5, 6, 7, 8, 9, 10, 11, 12, 13, 14];
pub const VITALS_CLASSIC_STYLES: [u8; 5] = [4, 0, 1, 2, 3];
pub const VITALS_ALL_STYLES: [u8; 15] = [5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 4, 0, 1, 2, 3];

pub fn vitals_styles_in_folder(folder: VitalsThemeFolder) -> &'static [u8] {
    match folder {
        VitalsThemeFolder::All => &VITALS_ALL_STYLES,
        VitalsThemeFolder::Horology => &VITALS_HOROLOGY_STYLES,
        VitalsThemeFolder::Classic => &VITALS_CLASSIC_STYLES,
    }
}

#[inline]
pub fn vitals_style_horology(st: u8) -> Option<crate::horology_pbr::HorologyTheme> {
    if (5..15).contains(&st) {
        crate::horology_pbr::HorologyTheme::from_idx((st - 5) as usize)
    } else {
        None
    }
}

#[inline]
pub fn vitals_uses_quad_telemetry(st: u8) -> bool {
    st == 4 || vitals_style_horology(st).is_some()
}

pub fn vitals_style_label(st: u8) -> &'static str {
    if let Some(ht) = vitals_style_horology(st) {
        return ht.label();
    }
    match st {
        0 => "Capsule Bars",
        1 => "Radial Rings",
        2 => "Cyber HUD",
        3 => "Minimal Text",
        _ => "Ring-Pill HUD",
    }
}

pub fn vitals_style_subtitle(st: u8) -> &'static str {
    if let Some(ht) = vitals_style_horology(st) {
        return ht.subtitle();
    }
    match st {
        0 => "Classic capsule telemetry with horizontal load bars",
        1 => "Circular radial rings for CPU, RAM & Power",
        2 => "Bracketed monospace telemetry HUD",
        3 => "Ultra-minimal pure typographic metrics",
        _ => "Quad CPU / GPU / RAM / TEMP ring-pill cockpit",
    }
}

pub fn vitals_style_badge(st: u8) -> &'static str {
    if let Some(ht) = vitals_style_horology(st) {
        return ht.badge();
    }
    match st {
        0 => "Bars",
        1 => "Rings",
        2 => "Cyber",
        3 => "Minimal",
        _ => "Quad HUD",
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CcThemeFolder {
    All,
    Horology,
    Classic,
}

impl CcThemeFolder {
    pub fn label(self) -> &'static str {
        match self {
            CcThemeFolder::All => "All Control Centers (14)",
            CcThemeFolder::Horology => "Photorealistic Horology (10)",
            CcThemeFolder::Classic => "Classic Tiles (4)",
        }
    }
}

pub const CC_HOROLOGY_STYLES: [u8; 10] = [4, 5, 6, 7, 8, 9, 10, 11, 12, 13];
pub const CC_CLASSIC_STYLES: [u8; 4] = [0, 1, 2, 3];
pub const CC_ALL_STYLES: [u8; 14] = [4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 0, 1, 2, 3];

pub fn cc_styles_in_folder(folder: CcThemeFolder) -> &'static [u8] {
    match folder {
        CcThemeFolder::All => &CC_ALL_STYLES,
        CcThemeFolder::Horology => &CC_HOROLOGY_STYLES,
        CcThemeFolder::Classic => &CC_CLASSIC_STYLES,
    }
}

#[inline]
pub fn cc_style_horology(st: u8) -> Option<crate::horology_pbr::HorologyTheme> {
    if (4..14).contains(&st) {
        crate::horology_pbr::HorologyTheme::from_idx((st - 4) as usize)
    } else {
        None
    }
}

pub fn cc_style_label(st: u8) -> &'static str {
    if let Some(ht) = cc_style_horology(st) {
        return ht.label();
    }
    match st {
        0 => "Rounded Glass",
        1 => "Capsule Pill",
        2 => "Cyber Chamfer",
        _ => "Minimal Flat",
    }
}

pub fn cc_style_subtitle(st: u8) -> &'static str {
    if let Some(ht) = cc_style_horology(st) {
        return ht.subtitle();
    }
    match st {
        0 => "Soft rounded quick-action tiles & frosted sliders",
        1 => "Full-capsule tactile pill controls",
        2 => "Precision sharp-beveled chamfer tiles",
        _ => "Ultra-clean translucent minimal controls",
    }
}

pub fn cc_style_badge(st: u8) -> &'static str {
    if let Some(ht) = cc_style_horology(st) {
        return ht.badge();
    }
    match st {
        0 => "Glass",
        1 => "Pill",
        2 => "Chamfer",
        _ => "Flat",
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AppsThemeFolder {
    All,
    Horology,
    Classic,
}

impl AppsThemeFolder {
    pub fn label(self) -> &'static str {
        match self {
            AppsThemeFolder::All => "All Quick Apps (14)",
            AppsThemeFolder::Horology => "Photorealistic Horology (10)",
            AppsThemeFolder::Classic => "Classic Shapes (4)",
        }
    }
}

pub const APPS_HOROLOGY_STYLES: [u8; 10] = [4, 5, 6, 7, 8, 9, 10, 11, 12, 13];
pub const APPS_CLASSIC_STYLES: [u8; 4] = [0, 1, 2, 3];
pub const APPS_ALL_STYLES: [u8; 14] = [4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 0, 1, 2, 3];

pub fn apps_styles_in_folder(folder: AppsThemeFolder) -> &'static [u8] {
    match folder {
        AppsThemeFolder::All => &APPS_ALL_STYLES,
        AppsThemeFolder::Horology => &APPS_HOROLOGY_STYLES,
        AppsThemeFolder::Classic => &APPS_CLASSIC_STYLES,
    }
}

#[inline]
pub fn apps_style_horology(st: u8) -> Option<crate::horology_pbr::HorologyTheme> {
    if (4..14).contains(&st) {
        crate::horology_pbr::HorologyTheme::from_idx((st - 4) as usize)
    } else {
        None
    }
}

pub fn apps_style_label(st: u8) -> &'static str {
    if let Some(ht) = apps_style_horology(st) {
        return ht.label();
    }
    match st {
        0 => "Squircle Glass",
        1 => "Circle Orb",
        2 => "Hexagon Tile",
        _ => "Cyber Chamfer",
    }
}

pub fn apps_style_subtitle(st: u8) -> &'static str {
    if let Some(ht) = apps_style_horology(st) {
        return ht.subtitle();
    }
    match st {
        0 => "Classic rounded squircle app tiles",
        1 => "Circular floating glass icon medallions",
        2 => "Geometric hexagonal shortcut grid",
        _ => "Octagonal chamfered tech app wells",
    }
}

pub fn apps_style_badge(st: u8) -> &'static str {
    if let Some(ht) = apps_style_horology(st) {
        return ht.badge();
    }
    match st {
        0 => "Squircle",
        1 => "Orb",
        2 => "Hex",
        _ => "Chamfer",
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ClockAlign {
    Left,
    Center,
    Right,
}

impl ClockAlign {
    pub fn as_str(self) -> &'static str {
        match self {
            ClockAlign::Left => "left",
            ClockAlign::Center => "center",
            ClockAlign::Right => "right",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "center" | "mid" => ClockAlign::Center,
            "right" | "end" => ClockAlign::Right,
            _ => ClockAlign::Left,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ClockElemId {
    Day = 0,
    Weekday = 1,
    Clock = 2,
    AmPm = 3,
    Orb = 4,
    Date = 5,
    Spine = 6,
    Card = 7,
}

impl ClockElemId {
    pub const ALL: [ClockElemId; 8] = [
        ClockElemId::Day,
        ClockElemId::Weekday,
        ClockElemId::Clock,
        ClockElemId::AmPm,
        ClockElemId::Orb,
        ClockElemId::Date,
        ClockElemId::Spine,
        ClockElemId::Card,
    ];

    pub fn idx(self) -> usize {
        self as usize
    }

    pub fn short_label(self) -> &'static str {
        match self {
            ClockElemId::Day => "Day",
            ClockElemId::Weekday => "Script",
            ClockElemId::Clock => "Time",
            ClockElemId::AmPm => "AM/PM",
            ClockElemId::Orb => "Orb",
            ClockElemId::Date => "Date",
            ClockElemId::Spine => "Spine",
            ClockElemId::Card => "Card",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CanvasStylePreset {
    CyberNeon,
    FrostedLuxury,
    RetroPhosphor,
    MinimalSlate,
    GoldAccent,
}

impl CanvasStylePreset {
    pub const ALL: [CanvasStylePreset; 5] = [
        CanvasStylePreset::CyberNeon,
        CanvasStylePreset::FrostedLuxury,
        CanvasStylePreset::RetroPhosphor,
        CanvasStylePreset::MinimalSlate,
        CanvasStylePreset::GoldAccent,
    ];

    pub fn label(self) -> &'static str {
        match self {
            CanvasStylePreset::CyberNeon => "Cyber Neon",
            CanvasStylePreset::FrostedLuxury => "Frost Luxury",
            CanvasStylePreset::RetroPhosphor => "Retro CRT",
            CanvasStylePreset::MinimalSlate => "Min Slate",
            CanvasStylePreset::GoldAccent => "Gold Accent",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ClockElemTransform {
    pub dx: i16,
    pub dy: i16,
    pub scale_pct: u16,
    pub rot_deg: i16,
    pub color_rgb: Option<u32>,
    pub font_scale_pct: u16,
    pub visible: bool,
    pub color2_rgb: Option<u32>,
    pub gradient_angle: u16,
    pub shadow_blur: u8,
    pub glow_rgb: Option<u32>,
    pub border_width: u8,
    pub border_rgb: Option<u32>,
}

impl Default for ClockElemTransform {
    fn default() -> Self {
        Self {
            dx: 0,
            dy: 0,
            scale_pct: 100,
            rot_deg: 0,
            color_rgb: None,
            font_scale_pct: 100,
            visible: true,
            color2_rgb: None,
            gradient_angle: 135,
            shadow_blur: 0,
            glow_rgb: None,
            border_width: 0,
            border_rgb: None,
        }
    }
}

impl ClockElemTransform {
    pub fn is_default(self) -> bool {
        self.dx == 0
            && self.dy == 0
            && self.scale_pct == 100
            && self.rot_deg == 0
            && self.color_rgb.is_none()
            && self.font_scale_pct == 100
            && self.visible
            && self.color2_rgb.is_none()
            && self.shadow_blur == 0
            && self.glow_rgb.is_none()
            && self.border_width == 0
            && self.border_rgb.is_none()
    }

    #[inline]
    pub fn gradient_rgb(self) -> Option<(u32, u32)> {
        Some((self.color_rgb?, self.color2_rgb?))
    }

    pub fn to_csv(self) -> String {
        let fmt_opt_rgb = |opt: Option<u32>| -> String {
            match opt {
                Some(rgb) => format!("0x{:06X}", rgb & 0x00FF_FFFF),
                None => "-".to_string(),
            }
        };
        let col = fmt_opt_rgb(self.color_rgb);
        if self.color2_rgb.is_none()
            && self.gradient_angle == 135
            && self.shadow_blur == 0
            && self.glow_rgb.is_none()
            && self.border_width == 0
            && self.border_rgb.is_none()
        {
            format!(
                "{},{},{},{},{},{},{}",
                self.dx,
                self.dy,
                self.scale_pct,
                self.rot_deg,
                col,
                self.font_scale_pct,
                if self.visible { 1 } else { 0 }
            )
        } else {
            format!(
                "{},{},{},{},{},{},{},{},{},{},{},{},{}",
                self.dx,
                self.dy,
                self.scale_pct,
                self.rot_deg,
                col,
                self.font_scale_pct,
                if self.visible { 1 } else { 0 },
                fmt_opt_rgb(self.color2_rgb),
                self.gradient_angle.min(360),
                self.shadow_blur.min(24),
                fmt_opt_rgb(self.glow_rgb),
                self.border_width.min(6),
                fmt_opt_rgb(self.border_rgb),
            )
        }
    }

    pub fn from_csv(s: &str) -> Option<Self> {
        let parts: Vec<&str> = s.split(',').collect();
        if parts.len() != 4 && parts.len() < 7 {
            return None;
        }
        let parse_opt_rgb = |opt_s: Option<&&str>| -> Option<u32> {
            opt_s.and_then(|v| {
                let t = v.trim();
                if t == "-" || t.is_empty() || t.eq_ignore_ascii_case("none") {
                    None
                } else {
                    let hex = t.trim_start_matches("0x").trim_start_matches("0X");
                    u32::from_str_radix(hex, 16).ok().map(|c| c & 0x00FF_FFFF)
                }
            })
        };
        let dx = parts[0].trim().parse::<i16>().ok()?.clamp(-280, 280);
        let dy = parts[1].trim().parse::<i16>().ok()?.clamp(-180, 180);
        let sc = parts[2].trim().parse::<u16>().ok()?.clamp(35, 260);
        let rot = parts[3].trim().parse::<i16>().ok()?.clamp(-180, 180);
        let color_rgb = parse_opt_rgb(parts.get(4));
        let font_scale_pct = parts
            .get(5)
            .and_then(|v| v.trim().parse::<u16>().ok())
            .unwrap_or(100)
            .clamp(50, 220);
        let visible = parts
            .get(6)
            .map(|v| v.trim() != "0")
            .unwrap_or(true);
        let color2_rgb = parse_opt_rgb(parts.get(7));
        let gradient_angle = parts
            .get(8)
            .and_then(|v| v.trim().parse::<u16>().ok())
            .unwrap_or(135)
            .clamp(0, 360);
        let shadow_blur = parts
            .get(9)
            .and_then(|v| v.trim().parse::<u8>().ok())
            .unwrap_or(0)
            .clamp(0, 24);
        let glow_rgb = parse_opt_rgb(parts.get(10));
        let border_width = parts
            .get(11)
            .and_then(|v| v.trim().parse::<u8>().ok())
            .unwrap_or(0)
            .clamp(0, 6);
        let border_rgb = parse_opt_rgb(parts.get(12));
        Some(Self {
            dx,
            dy,
            scale_pct: sc,
            rot_deg: rot,
            color_rgb,
            font_scale_pct: font_scale_pct,
            visible,
            color2_rgb,
            gradient_angle,
            shadow_blur,
            glow_rgb,
            border_width,
            border_rgb,
        })
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WidgetCanvasTarget {
    DateTime,
    Vitals,
    Music,
    ControlCenter,
    Apps,
    Ext(ExtWidgetId),
}

impl WidgetCanvasTarget {
    pub fn elem_count(self) -> usize {
        match self {
            WidgetCanvasTarget::DateTime => 8,
            WidgetCanvasTarget::Vitals
            | WidgetCanvasTarget::Music
            | WidgetCanvasTarget::ControlCenter
            | WidgetCanvasTarget::Ext(_) => 6,
            WidgetCanvasTarget::Apps => 5,
        }
    }

    pub fn is_card_slot(self, idx: usize) -> bool {
        match self {
            WidgetCanvasTarget::DateTime => idx == ClockElemId::Card.idx(),
            WidgetCanvasTarget::Vitals
            | WidgetCanvasTarget::Music
            | WidgetCanvasTarget::ControlCenter
            | WidgetCanvasTarget::Ext(_) => idx == 5,
            WidgetCanvasTarget::Apps => idx == 4,
        }
    }

    pub fn is_box_elem(self, idx: usize) -> bool {
        if self.is_card_slot(idx) {
            return true;
        }
        match self {
            WidgetCanvasTarget::DateTime => matches!(idx, 4 | 6),
            WidgetCanvasTarget::Apps => matches!(idx, 0 | 2),
            WidgetCanvasTarget::Vitals => matches!(idx, 3 | 4),
            WidgetCanvasTarget::Music => matches!(idx, 0 | 3 | 4),
            WidgetCanvasTarget::ControlCenter => matches!(idx, 0 | 3 | 4),
            WidgetCanvasTarget::Ext(_) => matches!(idx, 3 | 4),
        }
    }

    pub fn elem_label(self, idx: usize) -> &'static str {
        match self {
            WidgetCanvasTarget::DateTime => ClockElemId::ALL
                .get(idx)
                .map(|c| c.short_label())
                .unwrap_or("Elem"),
            WidgetCanvasTarget::Vitals => ["CPU", "RAM", "Power", "Sleep", "Shutdown", "Card"]
                .get(idx)
                .copied()
                .unwrap_or("Elem"),
            WidgetCanvasTarget::Music => ["Cover", "Title", "Artist", "Controls", "Prog/EQ", "Card"]
                .get(idx)
                .copied()
                .unwrap_or("Elem"),
            WidgetCanvasTarget::ControlCenter => ["Tiles", "Bright", "Volume", "Scroll", "Divider", "Card"]
                .get(idx)
                .copied()
                .unwrap_or("Elem"),
            WidgetCanvasTarget::Apps => ["Tiles", "Labels", "Glow Dots", "Add '+'", "Card"]
                .get(idx)
                .copied()
                .unwrap_or("Elem"),
            WidgetCanvasTarget::Ext(id) => {
                let labels: [&str; 6] = match id {
                    ExtWidgetId::Weather => ["City/Hdr", "Temp", "Details", "Icon/Pills", "Divider", "Card"],
                    ExtWidgetId::SearchBar => ["Engine", "Query", "Hint", "Action", "Focus", "Card"],
                    ExtWidgetId::Greeting => ["Greeting", "Quote", "Author", "Icon/Btns", "Divider", "Card"],
                    ExtWidgetId::Network => ["Header", "Speed", "Stats", "Sparkline", "Divider", "Card"],
                    ExtWidgetId::Gpu => ["Header", "Load", "Stats", "Bars", "Divider", "Card"],
                    ExtWidgetId::AudioSwitcher => ["Header", "Volume", "Devices", "Controls", "Meter", "Card"],
                    ExtWidgetId::Notes => ["Header", "Tasks", "Checks", "Buttons", "Divider", "Card"],
                    ExtWidgetId::Calendar => ["Month", "Weekdays", "Days", "Today/Btn", "Agenda", "Card"],
                    ExtWidgetId::Pomodoro => ["Header", "Timer", "Streak", "Ring/Btns", "Badge", "Card"],
                    ExtWidgetId::WorldClock => ["Header", "Cities", "Times", "Buttons", "Dials", "Card"],
                    ExtWidgetId::Clipboard => ["Header", "Snippets", "Length", "Rows/Btn", "Divider", "Card"],
                    ExtWidgetId::Drives => ["Header", "Percent", "Space", "Rings/Bar", "Divider", "Card"],
                    ExtWidgetId::Processes => ["Header", "App Name", "Memory", "Kill/Bars", "Divider", "Card"],
                    ExtWidgetId::RecycleBin => ["Header", "Items", "Size", "Ring/Icon", "Buttons", "Card"],
                };
                labels.get(idx).copied().unwrap_or("Elem")
            }
        }
    }

    pub fn canvas_title(self) -> String {
        match self {
            WidgetCanvasTarget::DateTime => "Clock Canvas Editor".to_string(),
            WidgetCanvasTarget::Vitals => "Vitals Canvas Editor".to_string(),
            WidgetCanvasTarget::Music => "Music Canvas Editor".to_string(),
            WidgetCanvasTarget::ControlCenter => "Control Center Canvas Editor".to_string(),
            WidgetCanvasTarget::Apps => "Apps Dock Canvas Editor".to_string(),
            WidgetCanvasTarget::Ext(id) => format!("{} Canvas Editor", id.short_title()),
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct AppShortcut {
    pub name: String,
    pub path: String,
    pub offset_x: i32,
    pub offset_y: i32,
}

pub fn default_apps() -> Vec<AppShortcut> {
    Vec::new()
}

#[derive(Clone, PartialEq, Eq)]
pub struct VideoWallpaperPreset {
    pub title: String,
    pub path: String,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PerformanceMode {
    Smooth,
    Balanced,
    Eco,
}

impl Default for PerformanceMode {
    fn default() -> Self {
        Self::Balanced
    }
}

impl PerformanceMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Smooth => "smooth",
            Self::Balanced => "balanced",
            Self::Eco => "eco",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "smooth" | "high" | "performance" => Self::Smooth,
            "eco" | "low" | "battery" => Self::Eco,
            _ => Self::Balanced,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::Smooth => "Smooth",
            Self::Balanced => "Balanced (Auto-Eco)",
            Self::Eco => "Eco (Low CPU)",
        }
    }

    pub fn description(&self) -> &'static str {
        match self {
            Self::Smooth => "Always full FPS across widgets & docks",
            Self::Balanced => "Auto-Eco (≤20 FPS) on Battery or background apps",
            Self::Eco => "Low-CPU cap (≤18 FPS) for max battery life",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WallpaperFpsCap {
    Auto,
    Fps60,
    Fps30,
}

impl Default for WallpaperFpsCap {
    fn default() -> Self {
        Self::Auto
    }
}

impl WallpaperFpsCap {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Fps60 => "60",
            Self::Fps30 => "30",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "60" | "fps60" | "60fps" => Self::Fps60,
            "30" | "fps30" | "30fps" => Self::Fps30,
            _ => Self::Auto,
        }
    }

    pub fn short_label(&self) -> &'static str {
        match self {
            Self::Auto => "Auto",
            Self::Fps60 => "60",
            Self::Fps30 => "30",
        }
    }
}

pub fn is_video_wallpaper_path(path: &str) -> bool {
    Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .map(|ext| matches!(ext.to_ascii_lowercase().as_str(), "mp4" | "mkv" | "webm" | "mov" | "avi"))
        .unwrap_or(false)
}

pub fn is_static_image_wallpaper_path(path: &str) -> bool {
    Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .map(|ext| matches!(ext.to_ascii_lowercase().as_str(), "png" | "jpg" | "jpeg" | "webp" | "bmp"))
        .unwrap_or(false)
}

pub fn is_supported_wallpaper_path(path: &str) -> bool {
    is_video_wallpaper_path(path) || is_static_image_wallpaper_path(path)
}

fn scan_wallpapers_in_dir(
    dir: &std::path::Path,
    depth: u8,
    min_img_bytes: u64,
    out: &mut Vec<VideoWallpaperPreset>,
) {
    if depth == 0 || !dir.is_dir() {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let p = entry.path();
        if p.is_dir() {
            scan_wallpapers_in_dir(&p, depth - 1, min_img_bytes, out);
        } else {
            let s = p.to_string_lossy().to_string();
            let is_vid = is_video_wallpaper_path(&s);
            let is_img = is_static_image_wallpaper_path(&s);
            if !is_vid && !is_img {
                continue;
            }
            if is_img {
                // Skip Lively companion thumbnail images and tiny non-wallpaper icons
                if let Some(stem) = p.file_stem().and_then(|n| n.to_str()) {
                    let sl = stem.to_ascii_lowercase();
                    if sl == "thumbnail" || sl == "preview" || sl.starts_with("thumb_") {
                        continue;
                    }
                }
                if min_img_bytes > 0 {
                    let sz = entry.metadata().map(|m| m.len()).unwrap_or(0);
                    if sz < min_img_bytes {
                        continue;
                    }
                }
            }
            if !out.iter().any(|x| x.path.eq_ignore_ascii_case(&s)) {
                let raw_title = p
                    .file_stem()
                    .and_then(|n| n.to_str())
                    .unwrap_or(if is_img { "Image Wallpaper" } else { "Video Wallpaper" })
                    .replace(['-', '_'], " ");
                out.push(VideoWallpaperPreset { title: raw_title, path: s });
            }
        }
    }
}

/// Fast local-only wallpaper scan (`current_path`, `%APPDATA%\AutumnDeck\wallpapers`,
/// and executable sibling folders). Runs in <1ms so UI threads never block on OneDrive/Downloads.
pub fn discover_wallpapers_fast_local(current_path: &str) -> Vec<VideoWallpaperPreset> {
    let mut out = Vec::new();
    let mut seen_names = std::collections::HashSet::<String>::new();
    let push_unique = |p: VideoWallpaperPreset, out: &mut Vec<VideoWallpaperPreset>, seen: &mut std::collections::HashSet<String>| {
        if let Some(fname) = Path::new(&p.path).file_name().and_then(|s| s.to_str()) {
            let key = fname.to_ascii_lowercase();
            if seen.contains(&key) {
                return;
            }
            seen.insert(key);
        }
        if !out.iter().any(|x| x.path.eq_ignore_ascii_case(&p.path)) {
            out.push(p);
        }
    };

    if !current_path.is_empty() && is_supported_wallpaper_path(current_path) && Path::new(current_path).exists() {
        let stem = Path::new(current_path)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Active Wallpaper")
            .replace(['-', '_'], " ");
        push_unique(
            VideoWallpaperPreset {
                title: stem,
                path: current_path.to_string(),
            },
            &mut out,
            &mut seen_names,
        );
    }

    let mut local_raw = Vec::new();
    scan_wallpapers_in_dir(&wallpapers_cache_dir(), 2, 0, &mut local_raw);
    if let Some(exe_dir) = self_exe_path().parent() {
        scan_wallpapers_in_dir(&exe_dir.join("Wallpapers"), 2, 0, &mut local_raw);
        scan_wallpapers_in_dir(&exe_dir.join("assets"), 1, 0, &mut local_raw);
    }
    for item in local_raw {
        push_unique(item, &mut out, &mut seen_names);
    }
    out.truncate(24);
    out
}

pub fn discover_video_wallpapers(current_path: &str) -> Vec<VideoWallpaperPreset> {
    let mut out = Vec::new();
    let mut seen_names = std::collections::HashSet::<String>::new();
    let push_unique = |p: VideoWallpaperPreset, out: &mut Vec<VideoWallpaperPreset>, seen: &mut std::collections::HashSet<String>| {
        if let Some(fname) = Path::new(&p.path).file_name().and_then(|s| s.to_str()) {
            let key = fname.to_ascii_lowercase();
            if seen.contains(&key) {
                return;
            }
            seen.insert(key);
        }
        if !out.iter().any(|x| x.path.eq_ignore_ascii_case(&p.path)) {
            out.push(p);
        }
    };

    if !current_path.is_empty() && is_supported_wallpaper_path(current_path) && Path::new(current_path).exists() {
        let stem = Path::new(current_path)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Active Wallpaper")
            .replace(['-', '_'], " ");
        push_unique(
            VideoWallpaperPreset {
                title: stem,
                path: current_path.to_string(),
            },
            &mut out,
            &mut seen_names,
        );
    }

    let mut local_raw = Vec::new();
    scan_wallpapers_in_dir(&wallpapers_cache_dir(), 2, 0, &mut local_raw);
    if let Some(exe_dir) = self_exe_path().parent() {
        scan_wallpapers_in_dir(&exe_dir.join("Wallpapers"), 2, 0, &mut local_raw);
        scan_wallpapers_in_dir(&exe_dir.join("assets"), 1, 0, &mut local_raw);
    }
    if let Ok(local_app) = std::env::var("LOCALAPPDATA") {
        let pkgs = PathBuf::from(&local_app).join("Packages");
        if let Ok(dirs) = fs::read_dir(&pkgs) {
            for d in dirs.flatten() {
                if d.file_name().to_string_lossy().to_ascii_lowercase().contains("lively") {
                    let lib = d.path().join("LocalCache").join("Local").join("Lively Wallpaper").join("Library");
                    scan_wallpapers_in_dir(&lib, 4, 65_536, &mut local_raw);
                }
            }
        }
        scan_wallpapers_in_dir(
            &PathBuf::from(&local_app).join("Lively Wallpaper").join("Library"),
            4,
            65_536,
            &mut local_raw,
        );
    }
    if let Ok(profile) = std::env::var("USERPROFILE") {
        let prof = PathBuf::from(&profile);
        scan_wallpapers_in_dir(&prof.join("Downloads"), 3, 65_536, &mut local_raw);
        scan_wallpapers_in_dir(&prof.join("Videos"), 2, 65_536, &mut local_raw);
        scan_wallpapers_in_dir(&prof.join("Pictures"), 2, 131_072, &mut local_raw);
        scan_wallpapers_in_dir(&prof.join("OneDrive").join("Pictures"), 2, 131_072, &mut local_raw);
        scan_wallpapers_in_dir(&prof.join("OneDrive").join("Desktop"), 2, 131_072, &mut local_raw);
    }
    for item in local_raw {
        push_unique(item, &mut out, &mut seen_names);
    }
    out.truncate(24);
    out
}

pub fn detect_default_wallpaper() -> String {
    let found = discover_video_wallpapers("");
    if let Some(autumn) = found.iter().find(|p| p.path.to_ascii_lowercase().contains("autumn")) {
        return cache_wallpaper_locally(&autumn.path);
    }
    found
        .into_iter()
        .next()
        .map(|p| cache_wallpaper_locally(&p.path))
        .unwrap_or_default()
}

pub fn cache_wallpaper_locally(src_path: &str) -> String {
    let src = src_path.trim();
    if src.is_empty() {
        return String::new();
    }
    let src_p = Path::new(src);
    if !src_p.is_file() {
        return src.to_string();
    }
    let cache_dir = wallpapers_cache_dir();
    let Some(fname) = src_p.file_name() else {
        return src.to_string();
    };
    let dest = cache_dir.join(fname);
    if src_p.eq(&dest) {
        return dest.to_string_lossy().to_string();
    }
    let _ = fs::create_dir_all(&cache_dir);
    let src_len = fs::metadata(src_p).map(|m| m.len()).unwrap_or(0);
    let need_copy = match fs::metadata(&dest) {
        Ok(dm) => dm.len() != src_len || src_len == 0,
        Err(_) => true,
    };
    if !need_copy || fs::copy(src_p, &dest).is_ok() {
        return dest.to_string_lossy().to_string();
    }
    src.to_string()
}

pub fn resolve_or_repair_wallpaper_path(cfg: &mut AppConfig) -> bool {
    let cur = cfg.wallpaper_path.trim().to_string();
    if !cur.is_empty() && Path::new(&cur).is_file() {
        let low = cur.to_ascii_lowercase();
        let cache_dir = wallpapers_cache_dir().to_string_lossy().to_ascii_lowercase();
        if !low.starts_with(&cache_dir) && (low.contains("livelywallpaper") || low.contains("\\wptmp\\")) {
            let cached = cache_wallpaper_locally(&cur);
            if !cached.eq_ignore_ascii_case(&cur) && Path::new(&cached).is_file() {
                cfg.wallpaper_path = cached;
                return true;
            }
        }
        return false;
    }

    if !cfg.wallpaper_enabled && cur.is_empty() {
        return false;
    }

    // Missing or relocated source file (e.g. Lively uninstalled): recover matching filename or fallback
    let missing_name = Path::new(&cur)
        .file_name()
        .and_then(|s| s.to_str())
        .map(|s| s.to_ascii_lowercase())
        .unwrap_or_default();

    let candidates = discover_video_wallpapers("");
    let matched = if !missing_name.is_empty() {
        candidates
            .iter()
            .find(|p| {
                Path::new(&p.path)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .map(|n| n.to_ascii_lowercase() == missing_name)
                    .unwrap_or(false)
            })
            .map(|p| p.path.clone())
    } else {
        None
    };

    let recovered = matched
        .or_else(|| {
            candidates
                .iter()
                .find(|p| p.path.to_ascii_lowercase().contains("autumn"))
                .map(|p| p.path.clone())
        })
        .or_else(|| candidates.into_iter().next().map(|p| p.path));

    if let Some(found_path) = recovered {
        let local_copy = cache_wallpaper_locally(&found_path);
        if Path::new(&local_copy).is_file() {
            cfg.wallpaper_path = local_copy;
            return true;
        }
    }
    false
}

#[derive(Clone, PartialEq, Eq)]
pub struct AppConfig {
    pub active_suite: String,
    pub mode: TaskbarMode,
    pub tb_theme: TaskbarTheme,
    pub pill_mode: PillMode,
    pub surface_mode: SurfaceMode,
    pub tint_alpha: u8,
    pub amber_border: bool,
    pub corner_radius: i32,
    pub tb_viz_style: TaskbarVizStyle,
    pub tb_viz_opacity: u8,
    pub tb_viz_mirror: bool,
    pub tb_viz_color_mode: VizColorMode,
    pub tb_viz_accent_rgb: u32,
    pub tb_hover_react: bool,
    pub tb_ambient_anim: bool,
    pub tb_show_backdrop: bool,
    pub tb_show_rim: bool,
    pub tb_show_fx: bool,
    pub viz_eco_fps: bool,
    pub anim_fps: u32,
    pub performance_mode: PerformanceMode,
    pub dt_hover_fx: TaskbarVizStyle,
    pub apps_hover_fx: TaskbarVizStyle,
    pub vitals_hover_fx: TaskbarVizStyle,
    pub music_hover_fx: TaskbarVizStyle,
    pub cc_hover_fx: TaskbarVizStyle,
    pub widgets_enabled: bool,
    pub widgets_locked: bool,
    pub widgets_24h: bool,
    pub widgets_hover_react: bool,
    pub widgets_live_material: bool,
    pub show_datetime: bool,
    pub show_apps: bool,
    pub show_vitals: bool,
    pub show_music: bool,
    pub show_cc: bool,

    // Widget positions in 1280x800 design units
    pub dt_x: i32,
    pub dt_y: i32,
    pub apps_x: i32,
    pub apps_y: i32,
    pub vitals_x: i32,
    pub vitals_y: i32,
    pub music_x: i32,
    pub music_y: i32,
    pub cc_x: i32,
    pub cc_y: i32,

    // Per-widget customization fields
    pub dt_scale_pct: u8,
    pub dt_show_plate: bool,
    pub dt_show_clock: bool,
    pub dt_glass_alpha: u8,
    pub dt_rim_alpha: u8,
    pub dt_show_spine: bool,
    pub dt_show_orb: bool,
    pub dt_style: ClockStyle,
    pub dt_align: ClockAlign,
    pub dt_show_seconds: bool,
    pub dt_show_ampm: bool,
    pub dt_show_weekday: bool,
    pub dt_show_date: bool,
    pub dt_corner_radius: u8,
    pub dt_letter_spacing: i8,
    pub dt_font_day: String,
    pub dt_font_script: String,
    pub dt_font_clock: String,
    pub dt_font_sub: String,
    pub dt_use_custom_color: bool,
    pub dt_accent_rgb: u32,
    pub dt_digits_rgb: u32,
    pub dt_script_rgb: u32,
    pub dt_bg_rgb: u32,
    pub dt_elem_transforms: [ClockElemTransform; 8],

    // Per-component Match Wallpaper accent flags (independent per widget & taskbar)
    pub tb_match_wallpaper: bool,
    pub dt_match_wallpaper: bool,
    pub apps_match_wallpaper: bool,
    pub vitals_match_wallpaper: bool,
    pub music_match_wallpaper: bool,
    pub cc_match_wallpaper: bool,

    pub apps_scale_pct: u8,
    pub apps_glass_alpha: u8,
    pub apps_rim_alpha: u8,
    pub apps_show_tiles: bool,
    pub apps_show_add_tile: bool,
    pub apps_match_theme: bool,
    pub apps_individual_move: bool,
    pub apps_cols: u8,
    pub apps_tile_shape: u8,
    pub apps_corner_radius: u8,
    pub apps_show_plate: bool,
    pub apps_show_labels: bool,
    pub apps_show_dots: bool,
    pub apps_font_label: String,
    pub apps_font_scale_pct: u8,
    pub apps_use_custom_color: bool,
    pub apps_accent_rgb: u32,
    pub apps_text_rgb: u32,
    pub apps_bg_rgb: u32,
    pub apps: Vec<AppShortcut>,

    pub vitals_scale_pct: u8,
    pub vitals_glass_alpha: u8,
    pub vitals_rim_alpha: u8,
    pub vitals_show_cpu: bool,
    pub vitals_show_ram: bool,
    pub vitals_show_power: bool,
    pub vitals_show_buttons: bool,
    pub vitals_style: u8,
    pub vitals_corner_radius: u8,
    pub vitals_show_plate: bool,
    pub vitals_show_dividers: bool,
    pub vitals_show_bars: bool,
    pub vitals_font_val: String,
    pub vitals_font_lbl: String,
    pub vitals_font_scale_pct: u8,
    pub vitals_use_custom_color: bool,
    pub vitals_accent_rgb: u32,
    pub vitals_text_rgb: u32,
    pub vitals_bg_rgb: u32,

    // Phase 2: NowPlaying Music Bar
    pub music_style: MusicStyle,
    pub music_scale_pct: u8,
    pub music_glass_alpha: u8,
    pub music_rim_alpha: u8,
    pub music_show_viz: bool,
    pub music_corner_radius: u8,
    pub music_show_plate: bool,
    pub music_show_cover: bool,
    pub music_show_title: bool,
    pub music_show_artist: bool,
    pub music_show_controls: bool,
    pub music_show_badge: bool,
    pub music_font_title: String,
    pub music_font_artist: String,
    pub music_font_scale_pct: u8,
    pub music_use_custom_color: bool,
    pub music_accent_rgb: u32,
    pub music_text_rgb: u32,
    pub music_bg_rgb: u32,

    // Phase 2: Quick Control Center
    pub cc_scale_pct: u8,
    pub cc_glass_alpha: u8,
    pub cc_rim_alpha: u8,
    pub cc_tile_style: u8,
    pub cc_corner_radius: u8,
    pub cc_show_plate: bool,
    pub cc_show_tiles: bool,
    pub cc_show_bright: bool,
    pub cc_show_vol: bool,
    pub cc_show_labels: bool,
    pub cc_show_scrollbar: bool,
    pub cc_show_dividers: bool,
    pub cc_font_label: String,
    pub cc_font_val: String,
    pub cc_font_scale_pct: u8,
    pub cc_use_custom_color: bool,
    pub cc_accent_rgb: u32,
    pub cc_text_rgb: u32,
    pub cc_bg_rgb: u32,

    // Pen Canvas Per-Element Transforms, Colors & Typography across all Desktop Widgets
    pub dt_elem_fonts: [String; 8],
    pub apps_elem_transforms: [ClockElemTransform; 5],
    pub apps_elem_fonts: [String; 5],
    pub vitals_elem_transforms: [ClockElemTransform; 6],
    pub vitals_elem_fonts: [String; 6],
    pub music_elem_transforms: [ClockElemTransform; 6],
    pub music_elem_fonts: [String; 6],
    pub cc_elem_transforms: [ClockElemTransform; 6],
    pub cc_elem_fonts: [String; 6],

    // Per-Widget Horology Material & Hardware Part Customization
    pub dt_horology: crate::horology_pbr::HorologyPartConfig,
    pub apps_horology: crate::horology_pbr::HorologyPartConfig,
    pub vitals_horology: crate::horology_pbr::HorologyPartConfig,
    pub music_horology: crate::horology_pbr::HorologyPartConfig,
    pub cc_horology: crate::horology_pbr::HorologyPartConfig,

    // Phase 3: Hardware-Accelerated Video & Static Image Wallpaper Engine
    pub wallpaper_enabled: bool,
    pub wallpaper_path: String,
    pub wallpaper_pause_maximized: bool,
    pub wallpaper_trim_memory: bool,
    pub wallpaper_mute: bool,
    pub wallpaper_volume_pct: u8,
    pub wallpaper_fps_cap: WallpaperFpsCap,
    pub game_mode_pause: bool,
    pub onboarding_done: bool,

    // Phase 4: Extended Desktop Widgets Suite (14 widgets + Sticky Notes checklist + World Clock cities)
    pub ext_widgets: [ExtWidgetConfig; EXT_WIDGET_COUNT],
    pub notes_title: String,
    pub notes_items: Vec<(bool, String)>,
    pub world_clock_cities: [String; 4],

    // Phase 5: Virtual Mac Dock Engine (Pure Win32 Shell Hook + Direct2D)
    pub mac_dock_enabled: bool,
    pub mac_dock_edge: MacDockEdge,
    pub mac_dock_hide: MacDockHideMode,
    pub mac_dock_icon_size: u8,
    pub mac_dock_mag_pct: u8,
    pub mac_dock_bounce: bool,
    pub mac_dock_dots: bool,
    pub dock_sync_win_pins: bool,
    pub mac_dock_pinned: Vec<AppShortcut>,

    // Phase 5b: Clean & Minimal macOS / Windhawk Bar & Status Menu Bar
    /// When true (`Bar: ON`), bottom dock uses pure macOS-style layout (no clock/tray clutter)
    /// and unlocks the dedicated Top Menu Bar toggle (`clean_top_bar_enabled`).
    /// When false (`Bar: OFF`), bottom dock uses full all-in-one taskbar mode (clock, tray, `^` on dock)
    /// and locks the Top Menu Bar OFF.
    pub dock_bar_enabled: bool,
    pub clean_top_bar_enabled: bool,
    pub clean_top_bar_edge: CleanTopBarEdge,
    pub clean_top_bar_style: CleanTopBarStyle,
    pub clean_top_bar_desktop_only: bool,

    // Phase 6: Modular Per-Widget Hyper-Customization Engine
    pub w_shelf: DockShelfFrameConfig,
    pub w_apps: DockAppWidgetConfig,
    pub w_clock: DockClockWidgetConfig,
    pub w_tray: DockTrayWidgetConfig,
    pub w_hud: DockHudWidgetConfig,
    pub w_util: DockUtilityWidgetConfig,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CleanTopBarEdge {
    Top,
    Bottom,
}

impl CleanTopBarEdge {
    pub fn as_str(self) -> &'static str {
        match self {
            CleanTopBarEdge::Top => "top",
            CleanTopBarEdge::Bottom => "bottom",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "bottom" | "bot" | "down" => CleanTopBarEdge::Bottom,
            _ => CleanTopBarEdge::Top,
        }
    }

    #[allow(dead_code)]
    pub fn label(self) -> &'static str {
        match self {
            CleanTopBarEdge::Top => "Top",
            CleanTopBarEdge::Bottom => "Bottom",
        }
    }

    pub fn toggle(self) -> Self {
        match self {
            CleanTopBarEdge::Top => CleanTopBarEdge::Bottom,
            CleanTopBarEdge::Bottom => CleanTopBarEdge::Top,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CleanTopBarStyle {
    Auto,
    FullWidth,
    Floating,
}

impl CleanTopBarStyle {
    pub fn as_str(self) -> &'static str {
        match self {
            CleanTopBarStyle::Auto => "auto",
            CleanTopBarStyle::FullWidth => "full",
            CleanTopBarStyle::Floating => "floating",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "full" | "fullwidth" | "macos" => CleanTopBarStyle::FullWidth,
            "floating" | "pill" | "capsule" | "windhawk" => CleanTopBarStyle::Floating,
            _ => CleanTopBarStyle::Auto,
        }
    }

    #[allow(dead_code)]
    pub fn label(self) -> &'static str {
        match self {
            CleanTopBarStyle::Auto => "Auto",
            CleanTopBarStyle::FullWidth => "Full Bar",
            CleanTopBarStyle::Floating => "Glass Pill",
        }
    }

    #[allow(dead_code)]
    pub fn next(self) -> Self {
        match self {
            CleanTopBarStyle::Auto => CleanTopBarStyle::FullWidth,
            CleanTopBarStyle::FullWidth => CleanTopBarStyle::Floating,
            CleanTopBarStyle::Floating => CleanTopBarStyle::Auto,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DockLayoutKind {
    FishEyeShelf,
    MultiIslandBar,
    FullEdgeTaskbar,
    IndependentOrbs,
    InlineMorphRail,
    HudConsoleDeck,
}

impl DockLayoutKind {
    pub const ALL: [DockLayoutKind; 6] = [
        DockLayoutKind::FishEyeShelf,
        DockLayoutKind::MultiIslandBar,
        DockLayoutKind::FullEdgeTaskbar,
        DockLayoutKind::IndependentOrbs,
        DockLayoutKind::InlineMorphRail,
        DockLayoutKind::HudConsoleDeck,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            DockLayoutKind::FishEyeShelf => "fisheye",
            DockLayoutKind::MultiIslandBar => "islands",
            DockLayoutKind::FullEdgeTaskbar => "fulledge",
            DockLayoutKind::IndependentOrbs => "orbs",
            DockLayoutKind::InlineMorphRail => "morph",
            DockLayoutKind::HudConsoleDeck => "hud",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            DockLayoutKind::FishEyeShelf => "Fish-Eye Shelf",
            DockLayoutKind::MultiIslandBar => "Multi-Island",
            DockLayoutKind::FullEdgeTaskbar => "Full-Edge Bar",
            DockLayoutKind::IndependentOrbs => "Floating Orbs",
            DockLayoutKind::InlineMorphRail => "Inline Morph",
            DockLayoutKind::HudConsoleDeck => "HUD Console",
        }
    }

    pub fn from_opt_str(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "fisheye" | "shelf" => Some(DockLayoutKind::FishEyeShelf),
            "islands" | "multi_island" => Some(DockLayoutKind::MultiIslandBar),
            "fulledge" | "full_edge" => Some(DockLayoutKind::FullEdgeTaskbar),
            "orbs" | "independent_orbs" => Some(DockLayoutKind::IndependentOrbs),
            "morph" | "inline_morph" => Some(DockLayoutKind::InlineMorphRail),
            "hud" | "hud_deck" => Some(DockLayoutKind::HudConsoleDeck),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DockShelfShape {
    Pill,
    Chamfer,
    Tile,
    FloatingZero,
}

impl DockShelfShape {
    pub const ALL: [DockShelfShape; 4] = [
        DockShelfShape::Pill,
        DockShelfShape::Chamfer,
        DockShelfShape::Tile,
        DockShelfShape::FloatingZero,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            DockShelfShape::Pill => "pill",
            DockShelfShape::Chamfer => "chamfer",
            DockShelfShape::Tile => "tile",
            DockShelfShape::FloatingZero => "zero",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            DockShelfShape::Pill => "Rounded Pill",
            DockShelfShape::Chamfer => "Chamfer Tech",
            DockShelfShape::Tile => "Sharp Tile",
            DockShelfShape::FloatingZero => "Zero Shelf",
        }
    }

    pub fn from_opt_str(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "pill" => Some(DockShelfShape::Pill),
            "chamfer" => Some(DockShelfShape::Chamfer),
            "tile" => Some(DockShelfShape::Tile),
            "zero" | "floating_zero" => Some(DockShelfShape::FloatingZero),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DockIndicatorStyle {
    CircleDot,
    Diamond,
    PillBar,
    SquareStamp,
    PhosphorCaret,
    Star4,
    EmberSpark,
    GeometricMixed,
    AquaOrb,
}

impl DockIndicatorStyle {
    pub const ALL: [DockIndicatorStyle; 9] = [
        DockIndicatorStyle::CircleDot,
        DockIndicatorStyle::PillBar,
        DockIndicatorStyle::AquaOrb,
        DockIndicatorStyle::Diamond,
        DockIndicatorStyle::SquareStamp,
        DockIndicatorStyle::PhosphorCaret,
        DockIndicatorStyle::Star4,
        DockIndicatorStyle::EmberSpark,
        DockIndicatorStyle::GeometricMixed,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            DockIndicatorStyle::CircleDot => "circle",
            DockIndicatorStyle::Diamond => "diamond",
            DockIndicatorStyle::PillBar => "pill",
            DockIndicatorStyle::SquareStamp => "square",
            DockIndicatorStyle::PhosphorCaret => "caret",
            DockIndicatorStyle::Star4 => "star4",
            DockIndicatorStyle::EmberSpark => "ember",
            DockIndicatorStyle::GeometricMixed => "geo_mix",
            DockIndicatorStyle::AquaOrb => "aqua_orb",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            DockIndicatorStyle::CircleDot => "Circle Dot",
            DockIndicatorStyle::Diamond => "Diamond",
            DockIndicatorStyle::PillBar => "Pill Bar",
            DockIndicatorStyle::SquareStamp => "Square Stamp",
            DockIndicatorStyle::PhosphorCaret => "CRT Caret",
            DockIndicatorStyle::Star4 => "4-Pt Star",
            DockIndicatorStyle::EmberSpark => "Ember Spark",
            DockIndicatorStyle::GeometricMixed => "Bauhaus Mix",
            DockIndicatorStyle::AquaOrb => "Aqua Orb",
        }
    }

    pub fn from_opt_str(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "circle" | "dot" => Some(DockIndicatorStyle::CircleDot),
            "diamond" => Some(DockIndicatorStyle::Diamond),
            "pill" => Some(DockIndicatorStyle::PillBar),
            "square" => Some(DockIndicatorStyle::SquareStamp),
            "caret" => Some(DockIndicatorStyle::PhosphorCaret),
            "star4" | "star" => Some(DockIndicatorStyle::Star4),
            "ember" => Some(DockIndicatorStyle::EmberSpark),
            "geo_mix" | "mixed" => Some(DockIndicatorStyle::GeometricMixed),
            "aqua_orb" | "orb" => Some(DockIndicatorStyle::AquaOrb),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DockArtLayer {
    Quartz,
    CyberHud,
    IslandTelemetry,
    AquaReflect,
    GhostAura,
    AuroraSilk,
    SlateTiles,
    KumikoLattice,
    LiquidChrome,
    SakuraPetals,
    AstrolabeTicks,
    VaporGrid,
    PaperHalftone,
    BiolumTendrils,
    PhosphorCrt,
    PrismSpectral,
    ObsidianMagma,
    BauhausPrimary,
    StarfieldWarp,
    // 18 Realistic Materials, Water & Calm Art Layers
    BrushedTitanium,
    VacuumTubeHiFi,
    SwissGuilloche,
    SaddleLeather,
    CarbonWeave,
    CassetteDeck1984,
    CrystalWater,
    RainWindowGlass,
    OceanTideFoam,
    ZenCalmMist,
    TranquilKoiPond,
    ForestSunbeams,
    CarraraMarble,
    OiledWalnutWood,
    GlacierCrystalIce,
    WarmLinenDusk,
    AbyssalTrench,
    GoldenHourHorizon,
    // 3 Clean & Minimal Windhawk-inspired Glass & Frost Art Layers
    LiquidGlassRefract,
    CleanFrost,
    LuminosityBloom,
}

impl DockArtLayer {
    pub const ALL: [DockArtLayer; 40] = [
        DockArtLayer::LiquidGlassRefract,
        DockArtLayer::CleanFrost,
        DockArtLayer::LuminosityBloom,
        DockArtLayer::CrystalWater,
        DockArtLayer::RainWindowGlass,
        DockArtLayer::BrushedTitanium,
        DockArtLayer::OiledWalnutWood,
        DockArtLayer::AbyssalTrench,
        DockArtLayer::GlacierCrystalIce,
        DockArtLayer::SaddleLeather,
        DockArtLayer::ForestSunbeams,
        DockArtLayer::CarbonWeave,
        DockArtLayer::GoldenHourHorizon,
        DockArtLayer::ZenCalmMist,
        DockArtLayer::VacuumTubeHiFi,
        DockArtLayer::SwissGuilloche,
        DockArtLayer::CassetteDeck1984,
        DockArtLayer::OceanTideFoam,
        DockArtLayer::TranquilKoiPond,
        DockArtLayer::CarraraMarble,
        DockArtLayer::WarmLinenDusk,
        DockArtLayer::Quartz,
        DockArtLayer::CyberHud,
        DockArtLayer::IslandTelemetry,
        DockArtLayer::AquaReflect,
        DockArtLayer::GhostAura,
        DockArtLayer::AuroraSilk,
        DockArtLayer::SlateTiles,
        DockArtLayer::KumikoLattice,
        DockArtLayer::LiquidChrome,
        DockArtLayer::SakuraPetals,
        DockArtLayer::AstrolabeTicks,
        DockArtLayer::VaporGrid,
        DockArtLayer::PaperHalftone,
        DockArtLayer::BiolumTendrils,
        DockArtLayer::PhosphorCrt,
        DockArtLayer::PrismSpectral,
        DockArtLayer::ObsidianMagma,
        DockArtLayer::BauhausPrimary,
        DockArtLayer::StarfieldWarp,
    ];

    pub fn pbr_material(self) -> Option<crate::realistic_pbr::PbrMaterial> {
        use crate::realistic_pbr::PbrMaterial;
        match self {
            DockArtLayer::CrystalWater | DockArtLayer::OceanTideFoam | DockArtLayer::TranquilKoiPond => {
                Some(PbrMaterial::CalmOceanLagoon)
            }
            DockArtLayer::RainWindowGlass => Some(PbrMaterial::RainyWindowpane),
            DockArtLayer::BrushedTitanium | DockArtLayer::VacuumTubeHiFi | DockArtLayer::CassetteDeck1984 => {
                Some(PbrMaterial::BrushedTitanium)
            }
            DockArtLayer::OiledWalnutWood | DockArtLayer::SwissGuilloche => {
                Some(PbrMaterial::VintageWalnutBrass)
            }
            DockArtLayer::AbyssalTrench => Some(PbrMaterial::DeepAbyssalTrench),
            DockArtLayer::GlacierCrystalIce => Some(PbrMaterial::NordicGlacialIce),
            DockArtLayer::SaddleLeather => Some(PbrMaterial::ExecutiveLeatherLinen),
            DockArtLayer::ForestSunbeams | DockArtLayer::ZenCalmMist => {
                Some(PbrMaterial::AlpineForestMist)
            }
            DockArtLayer::CarbonWeave => Some(PbrMaterial::IndustrialCarbonSteel),
            DockArtLayer::GoldenHourHorizon => Some(PbrMaterial::GoldenHourHorizon),
            DockArtLayer::CarraraMarble => Some(PbrMaterial::CarraraMarbleGold),
            DockArtLayer::WarmLinenDusk => Some(PbrMaterial::WovenLinenCalm),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            DockArtLayer::Quartz => "quartz",
            DockArtLayer::CyberHud => "cyber_hud",
            DockArtLayer::IslandTelemetry => "island_tel",
            DockArtLayer::AquaReflect => "aqua",
            DockArtLayer::GhostAura => "ghost",
            DockArtLayer::AuroraSilk => "aurora",
            DockArtLayer::SlateTiles => "slate",
            DockArtLayer::KumikoLattice => "kumiko",
            DockArtLayer::LiquidChrome => "chrome",
            DockArtLayer::SakuraPetals => "sakura",
            DockArtLayer::AstrolabeTicks => "astrolabe",
            DockArtLayer::VaporGrid => "vapor",
            DockArtLayer::PaperHalftone => "paper",
            DockArtLayer::BiolumTendrils => "biolum",
            DockArtLayer::PhosphorCrt => "phosphor",
            DockArtLayer::PrismSpectral => "prism",
            DockArtLayer::ObsidianMagma => "magma",
            DockArtLayer::BauhausPrimary => "bauhaus",
            DockArtLayer::StarfieldWarp => "starfield",
            DockArtLayer::BrushedTitanium => "brushed_titanium",
            DockArtLayer::VacuumTubeHiFi => "vacuum_tube",
            DockArtLayer::SwissGuilloche => "swiss_guilloche",
            DockArtLayer::SaddleLeather => "saddle_leather",
            DockArtLayer::CarbonWeave => "carbon_weave",
            DockArtLayer::CassetteDeck1984 => "cassette_1984",
            DockArtLayer::CrystalWater => "crystal_water",
            DockArtLayer::RainWindowGlass => "rain_window",
            DockArtLayer::OceanTideFoam => "ocean_tide",
            DockArtLayer::ZenCalmMist => "zen_calm_mist",
            DockArtLayer::TranquilKoiPond => "koi_pond",
            DockArtLayer::ForestSunbeams => "forest_sunbeams",
            DockArtLayer::CarraraMarble => "carrara_marble",
            DockArtLayer::OiledWalnutWood => "oiled_walnut",
            DockArtLayer::GlacierCrystalIce => "glacier_ice",
            DockArtLayer::WarmLinenDusk => "warm_linen",
            DockArtLayer::AbyssalTrench => "abyssal_trench",
            DockArtLayer::GoldenHourHorizon => "golden_hour_horizon",
            DockArtLayer::LiquidGlassRefract => "liquid_glass_refract",
            DockArtLayer::CleanFrost => "clean_frost",
            DockArtLayer::LuminosityBloom => "luminosity_bloom",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            DockArtLayer::Quartz => "Quartz Sheen",
            DockArtLayer::CyberHud => "Cyber HUD Scan",
            DockArtLayer::IslandTelemetry => "Island Glint",
            DockArtLayer::AquaReflect => "Aqua Ripples",
            DockArtLayer::GhostAura => "Ghost Clear",
            DockArtLayer::AuroraSilk => "Aurora Silk",
            DockArtLayer::SlateTiles => "Brushed Slate",
            DockArtLayer::KumikoLattice => "Kumiko Wood",
            DockArtLayer::LiquidChrome => "Liquid Chrome",
            DockArtLayer::SakuraPetals => "Sakura Petals",
            DockArtLayer::AstrolabeTicks => "Brass Ticks",
            DockArtLayer::VaporGrid => "Outrun Grid",
            DockArtLayer::PaperHalftone => "Halftone Ink",
            DockArtLayer::BiolumTendrils => "Abyssal Glow",
            DockArtLayer::PhosphorCrt => "CRT Scanlines",
            DockArtLayer::PrismSpectral => "Prism Facet",
            DockArtLayer::ObsidianMagma => "Magma Embers",
            DockArtLayer::BauhausPrimary => "Bauhaus Blocks",
            DockArtLayer::StarfieldWarp => "Warp Stars",
            DockArtLayer::BrushedTitanium => "Brushed Titanium",
            DockArtLayer::VacuumTubeHiFi => "Vacuum Tube Glow",
            DockArtLayer::SwissGuilloche => "Swiss Guilloche",
            DockArtLayer::SaddleLeather => "Saddle Leather",
            DockArtLayer::CarbonWeave => "Carbon 2x2 Weave",
            DockArtLayer::CassetteDeck1984 => "1984 Tape Reels",
            DockArtLayer::CrystalWater => "Crystal Water",
            DockArtLayer::RainWindowGlass => "Rainy Glass",
            DockArtLayer::OceanTideFoam => "Ocean Tide Foam",
            DockArtLayer::ZenCalmMist => "Zen Calm Mist",
            DockArtLayer::TranquilKoiPond => "Tranquil Koi",
            DockArtLayer::ForestSunbeams => "Forest Sunbeams",
            DockArtLayer::CarraraMarble => "Carrara Marble",
            DockArtLayer::OiledWalnutWood => "Oiled Walnut",
            DockArtLayer::GlacierCrystalIce => "Glacier Ice",
            DockArtLayer::WarmLinenDusk => "Warm Linen",
            DockArtLayer::AbyssalTrench => "Abyssal Trench",
            DockArtLayer::GoldenHourHorizon => "Golden Horizon",
            DockArtLayer::LiquidGlassRefract => "Liquid Glass",
            DockArtLayer::CleanFrost => "Clean Frost",
            DockArtLayer::LuminosityBloom => "Luminosity Glow",
        }
    }

    pub fn from_opt_str(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "quartz" => Some(DockArtLayer::Quartz),
            "cyber_hud" | "cyber" => Some(DockArtLayer::CyberHud),
            "island_tel" | "island" => Some(DockArtLayer::IslandTelemetry),
            "aqua" => Some(DockArtLayer::AquaReflect),
            "ghost" => Some(DockArtLayer::GhostAura),
            "aurora" => Some(DockArtLayer::AuroraSilk),
            "slate" => Some(DockArtLayer::SlateTiles),
            "kumiko" => Some(DockArtLayer::KumikoLattice),
            "chrome" => Some(DockArtLayer::LiquidChrome),
            "sakura" => Some(DockArtLayer::SakuraPetals),
            "astrolabe" => Some(DockArtLayer::AstrolabeTicks),
            "vapor" => Some(DockArtLayer::VaporGrid),
            "paper" => Some(DockArtLayer::PaperHalftone),
            "biolum" => Some(DockArtLayer::BiolumTendrils),
            "phosphor" | "crt" => Some(DockArtLayer::PhosphorCrt),
            "prism" => Some(DockArtLayer::PrismSpectral),
            "magma" | "obsidian" => Some(DockArtLayer::ObsidianMagma),
            "bauhaus" => Some(DockArtLayer::BauhausPrimary),
            "starfield" | "stars" => Some(DockArtLayer::StarfieldWarp),
            "brushed_titanium" | "titanium" => Some(DockArtLayer::BrushedTitanium),
            "vacuum_tube" | "tube" => Some(DockArtLayer::VacuumTubeHiFi),
            "swiss_guilloche" | "guilloche" => Some(DockArtLayer::SwissGuilloche),
            "saddle_leather" | "leather" => Some(DockArtLayer::SaddleLeather),
            "carbon_weave" | "carbon" => Some(DockArtLayer::CarbonWeave),
            "cassette_1984" | "cassette" => Some(DockArtLayer::CassetteDeck1984),
            "crystal_water" | "water" => Some(DockArtLayer::CrystalWater),
            "rain_window" | "rain" => Some(DockArtLayer::RainWindowGlass),
            "ocean_tide" | "ocean" => Some(DockArtLayer::OceanTideFoam),
            "zen_calm_mist" | "zen_mist" => Some(DockArtLayer::ZenCalmMist),
            "koi_pond" | "koi" => Some(DockArtLayer::TranquilKoiPond),
            "forest_sunbeams" | "forest" => Some(DockArtLayer::ForestSunbeams),
            "carrara_marble" | "marble" => Some(DockArtLayer::CarraraMarble),
            "oiled_walnut" | "walnut" => Some(DockArtLayer::OiledWalnutWood),
            "glacier_ice" | "glacier" => Some(DockArtLayer::GlacierCrystalIce),
            "warm_linen" | "linen" => Some(DockArtLayer::WarmLinenDusk),
            "abyssal_trench" => Some(DockArtLayer::AbyssalTrench),
            "golden_hour_horizon" | "golden_hour" => Some(DockArtLayer::GoldenHourHorizon),
            "liquid_glass_refract" | "liquid_glass" => Some(DockArtLayer::LiquidGlassRefract),
            "clean_frost" => Some(DockArtLayer::CleanFrost),
            "luminosity_bloom" | "luminosity" => Some(DockArtLayer::LuminosityBloom),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DockSlotCardStyle {
    InheritTheme,
    GlassPill,
    ChamferOctagon,
    FloatingOrb,
    FlatTile,
    MinimalNone,
}

impl DockSlotCardStyle {
    pub const ALL: [DockSlotCardStyle; 6] = [
        DockSlotCardStyle::InheritTheme,
        DockSlotCardStyle::GlassPill,
        DockSlotCardStyle::ChamferOctagon,
        DockSlotCardStyle::FloatingOrb,
        DockSlotCardStyle::FlatTile,
        DockSlotCardStyle::MinimalNone,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            DockSlotCardStyle::InheritTheme => "inherit",
            DockSlotCardStyle::GlassPill => "glass_pill",
            DockSlotCardStyle::ChamferOctagon => "chamfer_oct",
            DockSlotCardStyle::FloatingOrb => "floating_orb",
            DockSlotCardStyle::FlatTile => "flat_tile",
            DockSlotCardStyle::MinimalNone => "minimal_none",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            DockSlotCardStyle::InheritTheme => "Theme Default",
            DockSlotCardStyle::GlassPill => "Glass Pill",
            DockSlotCardStyle::ChamferOctagon => "Chamfer Octagon",
            DockSlotCardStyle::FloatingOrb => "Floating Orb",
            DockSlotCardStyle::FlatTile => "Flat Tile",
            DockSlotCardStyle::MinimalNone => "Minimal (No Card)",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "glass_pill" | "pill" => DockSlotCardStyle::GlassPill,
            "chamfer_oct" | "chamfer" => DockSlotCardStyle::ChamferOctagon,
            "floating_orb" | "orb" => DockSlotCardStyle::FloatingOrb,
            "flat_tile" | "tile" => DockSlotCardStyle::FlatTile,
            "minimal_none" | "none" | "minimal" => DockSlotCardStyle::MinimalNone,
            _ => DockSlotCardStyle::InheritTheme,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DockRunningLabelMode {
    InheritTheme,
    IconOnly,
    ActiveAppOnly,
    AllRunningApps,
    HoverExpandMorph,
}

impl DockRunningLabelMode {
    pub const ALL: [DockRunningLabelMode; 5] = [
        DockRunningLabelMode::InheritTheme,
        DockRunningLabelMode::IconOnly,
        DockRunningLabelMode::ActiveAppOnly,
        DockRunningLabelMode::AllRunningApps,
        DockRunningLabelMode::HoverExpandMorph,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            DockRunningLabelMode::InheritTheme => "inherit",
            DockRunningLabelMode::IconOnly => "icon_only",
            DockRunningLabelMode::ActiveAppOnly => "active_only",
            DockRunningLabelMode::AllRunningApps => "all_running",
            DockRunningLabelMode::HoverExpandMorph => "hover_morph",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            DockRunningLabelMode::InheritTheme => "Theme Default",
            DockRunningLabelMode::IconOnly => "Icons Only",
            DockRunningLabelMode::ActiveAppOnly => "Active Window Title",
            DockRunningLabelMode::AllRunningApps => "All Window Titles",
            DockRunningLabelMode::HoverExpandMorph => "Expand Title on Hover",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "icon_only" | "icons" => DockRunningLabelMode::IconOnly,
            "active_only" | "active" => DockRunningLabelMode::ActiveAppOnly,
            "all_running" | "all" => DockRunningLabelMode::AllRunningApps,
            "hover_morph" | "morph" => DockRunningLabelMode::HoverExpandMorph,
            _ => DockRunningLabelMode::InheritTheme,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DockDividerStyle {
    InheritTheme,
    ThinLine,
    CyberNotch,
    GlowingDiamond,
    BlankGap,
}

impl DockDividerStyle {
    pub const ALL: [DockDividerStyle; 5] = [
        DockDividerStyle::InheritTheme,
        DockDividerStyle::ThinLine,
        DockDividerStyle::CyberNotch,
        DockDividerStyle::GlowingDiamond,
        DockDividerStyle::BlankGap,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            DockDividerStyle::InheritTheme => "inherit",
            DockDividerStyle::ThinLine => "thin_line",
            DockDividerStyle::CyberNotch => "cyber_notch",
            DockDividerStyle::GlowingDiamond => "glow_diamond",
            DockDividerStyle::BlankGap => "blank_gap",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            DockDividerStyle::InheritTheme => "Theme Default",
            DockDividerStyle::ThinLine => "Vertical Line",
            DockDividerStyle::CyberNotch => "Cyber Brackets",
            DockDividerStyle::GlowingDiamond => "Glowing Diamond",
            DockDividerStyle::BlankGap => "Clean Space Gap",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "thin_line" | "line" => DockDividerStyle::ThinLine,
            "cyber_notch" | "cyber" => DockDividerStyle::CyberNotch,
            "glow_diamond" | "diamond" => DockDividerStyle::GlowingDiamond,
            "blank_gap" | "gap" => DockDividerStyle::BlankGap,
            _ => DockDividerStyle::InheritTheme,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DockClockEngine {
    InheritTheme,
    StackedDigital,
    SingleLinePill,
    NixieTubeGlow,
    AnalogChronograph,
    TerminalMatrix,
    BinaryMatrix,
}

impl DockClockEngine {
    pub const ALL: [DockClockEngine; 7] = [
        DockClockEngine::InheritTheme,
        DockClockEngine::StackedDigital,
        DockClockEngine::SingleLinePill,
        DockClockEngine::NixieTubeGlow,
        DockClockEngine::AnalogChronograph,
        DockClockEngine::TerminalMatrix,
        DockClockEngine::BinaryMatrix,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            DockClockEngine::InheritTheme => "inherit",
            DockClockEngine::StackedDigital => "stacked",
            DockClockEngine::SingleLinePill => "single_line",
            DockClockEngine::NixieTubeGlow => "nixie",
            DockClockEngine::AnalogChronograph => "analog_chrono",
            DockClockEngine::TerminalMatrix => "terminal",
            DockClockEngine::BinaryMatrix => "binary",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            DockClockEngine::InheritTheme => "Theme Default",
            DockClockEngine::StackedDigital => "Stacked Digital",
            DockClockEngine::SingleLinePill => "Single-Line Pill",
            DockClockEngine::NixieTubeGlow => "Nixie Cathode Tube",
            DockClockEngine::AnalogChronograph => "Analog Chronograph",
            DockClockEngine::TerminalMatrix => "Terminal CRT",
            DockClockEngine::BinaryMatrix => "Binary BCD Matrix",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "stacked" | "digital" => DockClockEngine::StackedDigital,
            "single_line" | "inline" => DockClockEngine::SingleLinePill,
            "nixie" | "nixie_tube" => DockClockEngine::NixieTubeGlow,
            "analog_chrono" | "analog" | "chrono" => DockClockEngine::AnalogChronograph,
            "terminal" | "crt" => DockClockEngine::TerminalMatrix,
            "binary" | "bcd" => DockClockEngine::BinaryMatrix,
            _ => DockClockEngine::InheritTheme,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DockClockWheelAction {
    AdjustVolume,
    CycleTimezoneOffset,
    None,
}

impl DockClockWheelAction {
    pub const ALL: [DockClockWheelAction; 3] = [
        DockClockWheelAction::AdjustVolume,
        DockClockWheelAction::CycleTimezoneOffset,
        DockClockWheelAction::None,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            DockClockWheelAction::AdjustVolume => "volume",
            DockClockWheelAction::CycleTimezoneOffset => "timezone",
            DockClockWheelAction::None => "none",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            DockClockWheelAction::AdjustVolume => "Scroll: System Volume",
            DockClockWheelAction::CycleTimezoneOffset => "Scroll: World Timezone",
            DockClockWheelAction::None => "Scroll: Off",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "timezone" | "tz" => DockClockWheelAction::CycleTimezoneOffset,
            "none" | "off" => DockClockWheelAction::None,
            _ => DockClockWheelAction::AdjustVolume,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TrayAudioMeterStyle {
    SpeakerGlyph,
    LiveVuEqualizer,
    VolumePercentText,
    RadialArc,
}

impl TrayAudioMeterStyle {
    pub const ALL: [TrayAudioMeterStyle; 4] = [
        TrayAudioMeterStyle::SpeakerGlyph,
        TrayAudioMeterStyle::LiveVuEqualizer,
        TrayAudioMeterStyle::VolumePercentText,
        TrayAudioMeterStyle::RadialArc,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            TrayAudioMeterStyle::SpeakerGlyph => "speaker",
            TrayAudioMeterStyle::LiveVuEqualizer => "live_vu",
            TrayAudioMeterStyle::VolumePercentText => "pct_text",
            TrayAudioMeterStyle::RadialArc => "radial_arc",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            TrayAudioMeterStyle::SpeakerGlyph => "Speaker Wave",
            TrayAudioMeterStyle::LiveVuEqualizer => "Live 4-Bar VU EQ",
            TrayAudioMeterStyle::VolumePercentText => "Volume % Readout",
            TrayAudioMeterStyle::RadialArc => "Radial Dial Arc",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "live_vu" | "vu" | "eq" => TrayAudioMeterStyle::LiveVuEqualizer,
            "pct_text" | "pct" => TrayAudioMeterStyle::VolumePercentText,
            "radial_arc" | "arc" => TrayAudioMeterStyle::RadialArc,
            _ => TrayAudioMeterStyle::SpeakerGlyph,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TrayBatteryMeterStyle {
    HorizontalCell,
    RadialRingGauge,
    PercentOnly,
    Hidden,
}

impl TrayBatteryMeterStyle {
    pub const ALL: [TrayBatteryMeterStyle; 4] = [
        TrayBatteryMeterStyle::HorizontalCell,
        TrayBatteryMeterStyle::RadialRingGauge,
        TrayBatteryMeterStyle::PercentOnly,
        TrayBatteryMeterStyle::Hidden,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            TrayBatteryMeterStyle::HorizontalCell => "cell",
            TrayBatteryMeterStyle::RadialRingGauge => "ring",
            TrayBatteryMeterStyle::PercentOnly => "pct",
            TrayBatteryMeterStyle::Hidden => "hidden",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            TrayBatteryMeterStyle::HorizontalCell => "Battery Cell",
            TrayBatteryMeterStyle::RadialRingGauge => "Radial Ring Gauge",
            TrayBatteryMeterStyle::PercentOnly => "Percentage Text",
            TrayBatteryMeterStyle::Hidden => "Hidden",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "ring" | "radial" => TrayBatteryMeterStyle::RadialRingGauge,
            "pct" | "percent" => TrayBatteryMeterStyle::PercentOnly,
            "hidden" | "off" => TrayBatteryMeterStyle::Hidden,
            _ => TrayBatteryMeterStyle::HorizontalCell,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TrayNetMeterStyle {
    SignalBars,
    LiveThroughputText,
    MinimalDot,
    Hidden,
}

impl TrayNetMeterStyle {
    pub const ALL: [TrayNetMeterStyle; 4] = [
        TrayNetMeterStyle::SignalBars,
        TrayNetMeterStyle::LiveThroughputText,
        TrayNetMeterStyle::MinimalDot,
        TrayNetMeterStyle::Hidden,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            TrayNetMeterStyle::SignalBars => "bars",
            TrayNetMeterStyle::LiveThroughputText => "speed",
            TrayNetMeterStyle::MinimalDot => "dot",
            TrayNetMeterStyle::Hidden => "hidden",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            TrayNetMeterStyle::SignalBars => "Signal Bars",
            TrayNetMeterStyle::LiveThroughputText => "Live Telemetry Tag",
            TrayNetMeterStyle::MinimalDot => "Status Beacon",
            TrayNetMeterStyle::Hidden => "Hidden",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "speed" | "throughput" => TrayNetMeterStyle::LiveThroughputText,
            "dot" | "beacon" => TrayNetMeterStyle::MinimalDot,
            "hidden" | "off" => TrayNetMeterStyle::Hidden,
            _ => TrayNetMeterStyle::SignalBars,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HudWingMode {
    InheritTheme,
    AlwaysEnabled,
    Hidden,
}

impl HudWingMode {
    pub const ALL: [HudWingMode; 3] = [
        HudWingMode::InheritTheme,
        HudWingMode::AlwaysEnabled,
        HudWingMode::Hidden,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            HudWingMode::InheritTheme => "inherit",
            HudWingMode::AlwaysEnabled => "always",
            HudWingMode::Hidden => "hidden",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            HudWingMode::InheritTheme => "Theme Default",
            HudWingMode::AlwaysEnabled => "Always Show Wing",
            HudWingMode::Hidden => "Hide HUD Wing",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "always" | "enabled" | "show" => HudWingMode::AlwaysEnabled,
            "hidden" | "hide" | "off" => HudWingMode::Hidden,
            _ => HudWingMode::InheritTheme,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HudWingShape {
    InheritTheme,
    ChamferTech,
    RoundedCapsule,
    MinimalUnderline,
}

impl HudWingShape {
    pub const ALL: [HudWingShape; 4] = [
        HudWingShape::InheritTheme,
        HudWingShape::ChamferTech,
        HudWingShape::RoundedCapsule,
        HudWingShape::MinimalUnderline,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            HudWingShape::InheritTheme => "inherit",
            HudWingShape::ChamferTech => "chamfer",
            HudWingShape::RoundedCapsule => "capsule",
            HudWingShape::MinimalUnderline => "underline",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            HudWingShape::InheritTheme => "Theme Default",
            HudWingShape::ChamferTech => "Chamfered Cockpit",
            HudWingShape::RoundedCapsule => "Rounded Glass Pill",
            HudWingShape::MinimalUnderline => "Minimal Neon Bar",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "chamfer" | "tech" => HudWingShape::ChamferTech,
            "capsule" | "pill" => HudWingShape::RoundedCapsule,
            "underline" | "minimal" => HudWingShape::MinimalUnderline,
            _ => HudWingShape::InheritTheme,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HudTelemetryContent {
    ActiveWindowTitle,
    TitleAndCpuRam,
    TitleAndMediaTrack,
}

impl HudTelemetryContent {
    pub const ALL: [HudTelemetryContent; 3] = [
        HudTelemetryContent::ActiveWindowTitle,
        HudTelemetryContent::TitleAndCpuRam,
        HudTelemetryContent::TitleAndMediaTrack,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            HudTelemetryContent::ActiveWindowTitle => "title",
            HudTelemetryContent::TitleAndCpuRam => "cpu_ram",
            HudTelemetryContent::TitleAndMediaTrack => "media",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            HudTelemetryContent::ActiveWindowTitle => "Active Window Title",
            HudTelemetryContent::TitleAndCpuRam => "Title + Live CPU & RAM",
            HudTelemetryContent::TitleAndMediaTrack => "Title + Now Playing Track",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "cpu_ram" | "vitals" => HudTelemetryContent::TitleAndCpuRam,
            "media" | "music" => HudTelemetryContent::TitleAndMediaTrack,
            _ => HudTelemetryContent::ActiveWindowTitle,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StartBadgeStyle {
    InheritTheme,
    Windows11Grid,
    CyberHexCore,
    TerminalPrompt,
    AquaOrb,
}

impl StartBadgeStyle {
    pub const ALL: [StartBadgeStyle; 5] = [
        StartBadgeStyle::InheritTheme,
        StartBadgeStyle::Windows11Grid,
        StartBadgeStyle::CyberHexCore,
        StartBadgeStyle::TerminalPrompt,
        StartBadgeStyle::AquaOrb,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            StartBadgeStyle::InheritTheme => "inherit",
            StartBadgeStyle::Windows11Grid => "win11",
            StartBadgeStyle::CyberHexCore => "cyber_hex",
            StartBadgeStyle::TerminalPrompt => "terminal",
            StartBadgeStyle::AquaOrb => "aqua_orb",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            StartBadgeStyle::InheritTheme => "Theme Default",
            StartBadgeStyle::Windows11Grid => "Windows 11 Grid",
            StartBadgeStyle::CyberHexCore => "Cyber Hex Core",
            StartBadgeStyle::TerminalPrompt => "Terminal [>_]",
            StartBadgeStyle::AquaOrb => "Crystal Orb",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "win11" | "grid" => StartBadgeStyle::Windows11Grid,
            "cyber_hex" | "hex" => StartBadgeStyle::CyberHexCore,
            "terminal" | "prompt" => StartBadgeStyle::TerminalPrompt,
            "aqua_orb" | "orb" => StartBadgeStyle::AquaOrb,
            _ => StartBadgeStyle::InheritTheme,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TaskbarWidgetTab {
    ShelfFrame,
    AppDock,
    Clock,
    SystemTray,
    HudWing,
    Utilities,
}

impl TaskbarWidgetTab {
    pub fn label(self) -> &'static str {
        match self {
            TaskbarWidgetTab::ShelfFrame => "Shelf & Frame",
            TaskbarWidgetTab::AppDock => "App Dock",
            TaskbarWidgetTab::Clock => "Clock",
            TaskbarWidgetTab::SystemTray => "System Tray",
            TaskbarWidgetTab::HudWing => "HUD Wing",
            TaskbarWidgetTab::Utilities => "Utilities",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DockShelfFrameConfig {
    pub theme_override: Option<TaskbarTheme>,
    pub layout_override: Option<DockLayoutKind>,
    pub shape_override: Option<DockShelfShape>,
    pub art_override: Option<DockArtLayer>,
    pub glass_alpha_pct: u8,
    pub rim_alpha_pct: u8,
    pub show_plate: bool,
    pub show_rim: bool,
    pub show_art: bool,
}

impl Default for DockShelfFrameConfig {
    fn default() -> Self {
        Self {
            theme_override: None,
            layout_override: None,
            shape_override: None,
            art_override: None,
            glass_alpha_pct: 100,
            rim_alpha_pct: 100,
            show_plate: true,
            show_rim: true,
            show_art: true,
        }
    }
}

impl DockShelfFrameConfig {
    #[inline]
    pub fn is_all_elements_off(&self) -> bool {
        !self.show_plate && !self.show_rim && !self.show_art
    }
}

pub const MAX_PINNED_APPS: usize = 32;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DockAppWidgetConfig {
    pub theme_override: Option<TaskbarTheme>,
    pub card_style: DockSlotCardStyle,
    pub indicator_override: Option<DockIndicatorStyle>,
    pub divider_style: DockDividerStyle,
    pub label_mode: DockRunningLabelMode,
    /// Inline app name font size on themed taskbars/docks (`10..=20 px`, default `12`).
    pub label_font_size_px: u8,
    pub per_app_brand_glow: bool,
    pub custom_glow_rgb: Option<u32>,
    pub show_window_count_badge: bool,
    pub middle_click_action: u8,
    pub wheel_action: u8,
    /// Opacity of the per-icon box / keycap / tile card (`0..=100`, `0` = completely transparent, default `100`).
    pub card_opacity_pct: u8,
    pub show_icons: bool,
    pub show_cards: bool,
    pub show_indicators: bool,
    pub show_separators: bool,
}

impl Default for DockAppWidgetConfig {
    fn default() -> Self {
        Self {
            theme_override: None,
            card_style: DockSlotCardStyle::InheritTheme,
            indicator_override: None,
            divider_style: DockDividerStyle::InheritTheme,
            label_mode: DockRunningLabelMode::InheritTheme,
            label_font_size_px: 12,
            per_app_brand_glow: false,
            custom_glow_rgb: None,
            show_window_count_badge: false,
            middle_click_action: 0,
            wheel_action: 0,
            card_opacity_pct: 100,
            show_icons: true,
            show_cards: true,
            show_indicators: true,
            show_separators: true,
        }
    }
}

impl DockAppWidgetConfig {
    #[inline]
    pub fn is_all_elements_off(&self) -> bool {
        !self.show_icons && !self.show_cards && !self.show_indicators && !self.show_separators
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DockClockWidgetConfig {
    pub theme_override: Option<TaskbarTheme>,
    pub engine: DockClockEngine,
    pub use_24h: Option<bool>,
    pub show_time: bool,
    pub show_seconds: bool,
    pub show_date: bool,
    pub show_weekday: bool,
    pub custom_accent_rgb: Option<u32>,
    pub custom_text_rgb: Option<u32>,
    pub font_scale_pct: u8,
    pub tz_offset_half_hours: i8,
    pub wheel_action: DockClockWheelAction,
}

impl Default for DockClockWidgetConfig {
    fn default() -> Self {
        Self {
            theme_override: None,
            engine: DockClockEngine::InheritTheme,
            use_24h: None,
            show_time: true,
            show_seconds: false,
            show_date: true,
            show_weekday: true,
            custom_accent_rgb: None,
            custom_text_rgb: None,
            font_scale_pct: 100,
            tz_offset_half_hours: 0,
            wheel_action: DockClockWheelAction::AdjustVolume,
        }
    }
}

impl DockClockWidgetConfig {
    #[inline]
    pub fn is_all_elements_off(&self) -> bool {
        !self.show_time && !self.show_date
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DockTrayWidgetConfig {
    pub theme_override: Option<TaskbarTheme>,
    pub audio_style: TrayAudioMeterStyle,
    pub battery_style: TrayBatteryMeterStyle,
    pub net_style: TrayNetMeterStyle,
    pub show_osd_on_scroll: bool,
    pub wheel_step_pct: u8,
    pub custom_accent_rgb: Option<u32>,
    pub show_net: bool,
    pub show_audio: bool,
    pub show_battery: bool,
}

impl Default for DockTrayWidgetConfig {
    fn default() -> Self {
        Self {
            theme_override: None,
            audio_style: TrayAudioMeterStyle::SpeakerGlyph,
            battery_style: TrayBatteryMeterStyle::HorizontalCell,
            net_style: TrayNetMeterStyle::SignalBars,
            show_osd_on_scroll: true,
            wheel_step_pct: 2,
            custom_accent_rgb: None,
            show_net: true,
            show_audio: true,
            show_battery: true,
        }
    }
}

impl DockTrayWidgetConfig {
    #[inline]
    pub fn eff_show_net(&self) -> bool {
        self.show_net && self.net_style != TrayNetMeterStyle::Hidden
    }

    #[inline]
    pub fn eff_show_audio(&self) -> bool {
        self.show_audio
    }

    #[inline]
    pub fn eff_show_battery(&self) -> bool {
        self.show_battery && self.battery_style != TrayBatteryMeterStyle::Hidden
    }

    #[inline]
    pub fn is_all_elements_off(&self) -> bool {
        !self.eff_show_net() && !self.eff_show_audio() && !self.eff_show_battery()
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct DockHudWidgetConfig {
    pub mode: HudWingMode,
    pub theme_override: Option<TaskbarTheme>,
    pub shape: HudWingShape,
    pub content: HudTelemetryContent,
    pub custom_prefix: String,
    pub width_pct: u8,
    pub plate_alpha_pct: u8,
    pub marquee_ticker: bool,
    pub show_telemetry: bool,
    pub show_active_app: bool,
}

impl Default for DockHudWidgetConfig {
    fn default() -> Self {
        Self {
            mode: HudWingMode::InheritTheme,
            theme_override: None,
            shape: HudWingShape::InheritTheme,
            content: HudTelemetryContent::ActiveWindowTitle,
            custom_prefix: String::new(),
            width_pct: 66,
            plate_alpha_pct: 97,
            marquee_ticker: false,
            show_telemetry: true,
            show_active_app: true,
        }
    }
}

impl DockHudWidgetConfig {
    #[inline]
    pub fn is_all_elements_off(&self) -> bool {
        !self.show_telemetry && !self.show_active_app
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DockUtilityWidgetConfig {
    pub theme_override: Option<TaskbarTheme>,
    pub start_style: StartBadgeStyle,
    pub show_start: bool,
    pub show_search: bool,
    pub show_taskview: bool,
    pub show_trash: bool,
    pub show_studio: bool,
    pub show_desktop_sliver: bool,
}

impl Default for DockUtilityWidgetConfig {
    fn default() -> Self {
        Self {
            theme_override: None,
            start_style: StartBadgeStyle::InheritTheme,
            show_start: true,
            show_search: true,
            show_taskview: true,
            show_trash: true,
            show_studio: true,
            show_desktop_sliver: true,
        }
    }
}

impl DockUtilityWidgetConfig {
    #[inline]
    pub fn is_all_elements_off(&self) -> bool {
        !self.show_start
            && !self.show_search
            && !self.show_taskview
            && !self.show_trash
            && !self.show_studio
    }
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            active_suite: String::new(),
            mode: TaskbarMode::AlwaysVisible,
            tb_theme: TaskbarTheme::AutumnCrystal,
            pill_mode: PillMode::SplitPills,
            surface_mode: SurfaceMode::ClearTint,
            tint_alpha: 105,
            amber_border: true,
            corner_radius: 20,
            tb_viz_style: TaskbarVizStyle::Off,
            tb_viz_opacity: 80,
            tb_viz_mirror: false,
            tb_viz_color_mode: VizColorMode::ThemeNative,
            tb_viz_accent_rgb: 0x00E8FF,
            tb_hover_react: true,
            tb_ambient_anim: true,
            tb_show_backdrop: true,
            tb_show_rim: true,
            tb_show_fx: true,
            viz_eco_fps: false,
            anim_fps: AppConfig::DEFAULT_ANIM_FPS,
            performance_mode: PerformanceMode::Balanced,
            dt_hover_fx: TaskbarVizStyle::Off,
            apps_hover_fx: TaskbarVizStyle::Off,
            vitals_hover_fx: TaskbarVizStyle::Off,
            music_hover_fx: TaskbarVizStyle::Off,
            cc_hover_fx: TaskbarVizStyle::Off,
            widgets_enabled: true,
            widgets_locked: false,
            widgets_24h: false,
            widgets_hover_react: true,
            widgets_live_material: true,
            show_datetime: true,
            show_apps: false,
            show_vitals: false,
            show_music: false,
            show_cc: false,

            dt_x: 68,
            dt_y: 42,
            apps_x: 68,
            apps_y: 198,
            vitals_x: 810,
            vitals_y: 502,
            music_x: 804,
            music_y: 42,
            cc_x: 828,
            cc_y: 178,

            dt_scale_pct: 100,
            dt_show_plate: true,
            dt_show_clock: true,
            dt_glass_alpha: 52,
            dt_rim_alpha: 115,
            dt_show_spine: true,
            dt_show_orb: true,
            dt_style: ClockStyle::Signature,
            dt_align: ClockAlign::Left,
            dt_show_seconds: true,
            dt_show_ampm: true,
            dt_show_weekday: true,
            dt_show_date: true,
            dt_corner_radius: 20,
            dt_letter_spacing: 0,
            dt_font_day: String::new(),
            dt_font_script: String::new(),
            dt_font_clock: String::new(),
            dt_font_sub: String::new(),
            dt_use_custom_color: false,
            dt_accent_rgb: 0xF5A642,
            dt_digits_rgb: 0xFFF3DC,
            dt_script_rgb: 0xF5A642,
            dt_bg_rgb: 0x16100C,
            dt_elem_transforms: [ClockElemTransform::default(); 8],
            tb_match_wallpaper: false,
            dt_match_wallpaper: false,
            apps_match_wallpaper: false,
            vitals_match_wallpaper: false,
            music_match_wallpaper: false,
            cc_match_wallpaper: false,

            apps_scale_pct: 100,
            apps_glass_alpha: 52,
            apps_rim_alpha: 130,
            apps_show_tiles: true,
            apps_show_add_tile: false,
            apps_match_theme: true,
            apps_individual_move: false,
            apps_cols: 6,
            apps_tile_shape: 0,
            apps_corner_radius: 18,
            apps_show_plate: true,
            apps_show_labels: true,
            apps_show_dots: true,
            apps_font_label: String::new(),
            apps_font_scale_pct: 100,
            apps_use_custom_color: false,
            apps_accent_rgb: 0xF5A642,
            apps_text_rgb: 0xFFF3DC,
            apps_bg_rgb: 0x16100C,
            apps: default_apps(),

            vitals_scale_pct: 100,
            vitals_glass_alpha: 55,
            vitals_rim_alpha: 120,
            vitals_show_cpu: true,
            vitals_show_ram: true,
            vitals_show_power: true,
            vitals_show_buttons: true,
            vitals_style: 0,
            vitals_corner_radius: 18,
            vitals_show_plate: true,
            vitals_show_dividers: true,
            vitals_show_bars: true,
            vitals_font_val: String::new(),
            vitals_font_lbl: String::new(),
            vitals_font_scale_pct: 100,
            vitals_use_custom_color: false,
            vitals_accent_rgb: 0xF5A642,
            vitals_text_rgb: 0xFFF3DC,
            vitals_bg_rgb: 0x16100C,

            music_style: MusicStyle::Sanctuary,
            music_scale_pct: 100,
            music_glass_alpha: 55,
            music_rim_alpha: 125,
            music_show_viz: true,
            music_corner_radius: 22,
            music_show_plate: true,
            music_show_cover: true,
            music_show_title: true,
            music_show_artist: true,
            music_show_controls: true,
            music_show_badge: true,
            music_font_title: String::new(),
            music_font_artist: String::new(),
            music_font_scale_pct: 100,
            music_use_custom_color: false,
            music_accent_rgb: 0xF5A642,
            music_text_rgb: 0xFFF3DC,
            music_bg_rgb: 0x16100C,

            cc_scale_pct: 100,
            cc_glass_alpha: 78,
            cc_rim_alpha: 135,
            cc_tile_style: 0,
            cc_corner_radius: 22,
            cc_show_plate: true,
            cc_show_tiles: true,
            cc_show_bright: true,
            cc_show_vol: true,
            cc_show_labels: true,
            cc_show_scrollbar: true,
            cc_show_dividers: true,
            cc_font_label: String::new(),
            cc_font_val: String::new(),
            cc_font_scale_pct: 100,
            cc_use_custom_color: false,
            cc_accent_rgb: 0xF5A642,
            cc_text_rgb: 0xFFF3DC,
            cc_bg_rgb: 0x16100C,

            dt_elem_fonts: std::array::from_fn(|_| String::new()),
            apps_elem_transforms: [ClockElemTransform::default(); 5],
            apps_elem_fonts: std::array::from_fn(|_| String::new()),
            vitals_elem_transforms: [ClockElemTransform::default(); 6],
            vitals_elem_fonts: std::array::from_fn(|_| String::new()),
            music_elem_transforms: [ClockElemTransform::default(); 6],
            music_elem_fonts: std::array::from_fn(|_| String::new()),
            cc_elem_transforms: [ClockElemTransform::default(); 6],
            cc_elem_fonts: std::array::from_fn(|_| String::new()),

            dt_horology: crate::horology_pbr::HorologyPartConfig::default(),
            apps_horology: crate::horology_pbr::HorologyPartConfig::default(),
            vitals_horology: crate::horology_pbr::HorologyPartConfig::default(),
            music_horology: crate::horology_pbr::HorologyPartConfig::default(),
            cc_horology: crate::horology_pbr::HorologyPartConfig::default(),

            wallpaper_enabled: false,
            wallpaper_path: String::new(),
            wallpaper_pause_maximized: true,
            wallpaper_trim_memory: false,
            wallpaper_mute: true,
            wallpaper_volume_pct: 0,
            wallpaper_fps_cap: WallpaperFpsCap::Auto,
            game_mode_pause: true,
            onboarding_done: false,

            ext_widgets: default_ext_widgets(),
            notes_title: "NOTES & TO-DO".to_string(),
            notes_items: default_notes_items(),
            world_clock_cities: default_world_clock_cities(),

            mac_dock_enabled: false,
            mac_dock_edge: MacDockEdge::Bottom,
            mac_dock_hide: MacDockHideMode::AlwaysVisible,
            mac_dock_icon_size: 36,
            mac_dock_mag_pct: 155,
            mac_dock_bounce: true,
            mac_dock_dots: true,
            dock_sync_win_pins: true,
            mac_dock_pinned: default_dock_pinned(),

            dock_bar_enabled: true,
            clean_top_bar_enabled: true,
            clean_top_bar_edge: CleanTopBarEdge::Top,
            clean_top_bar_style: CleanTopBarStyle::Auto,
            clean_top_bar_desktop_only: false,

            w_shelf: DockShelfFrameConfig::default(),
            w_apps: DockAppWidgetConfig::default(),
            w_clock: DockClockWidgetConfig::default(),
            w_tray: DockTrayWidgetConfig::default(),
            w_hud: DockHudWidgetConfig::default(),
            w_util: DockUtilityWidgetConfig::default(),
        }
    }
}

impl AppConfig {
    pub const DEFAULT_ANIM_FPS: u32 = 45;

    pub fn clamp_anim_fps(fps: u32) -> u32 {
        fps.clamp(15, 120)
    }

    /// Dynamically calculates the render timer interval in milliseconds: `max(8, 1000 / anim_fps)`.
    pub fn anim_frame_ms_for(fps: u32) -> u32 {
        let clamped = Self::clamp_anim_fps(fps);
        (1000 / clamped).max(8)
    }

    /// Active animation frame interval (ms) derived from `self.anim_fps`.
    pub fn anim_frame_ms(&self) -> u32 {
        Self::anim_frame_ms_for(self.anim_fps)
    }

    /// True when the Virtual Dock Bar mode is ON (pure macOS-style bottom dock + Top Bar option unlocked).
    /// When OFF, the bottom dock renders the full all-in-one taskbar (tray + clock + hidden icons) with no Top Bar.
    pub fn is_dock_bar_on(&self) -> bool {
        self.mac_dock_enabled && self.dock_bar_enabled
    }

    /// True when the macOS / Windhawk Status Menu Bar should render for the active Virtual Dock.
    /// Locked OFF whenever Bar mode is OFF or the dock is in Full Stealth Hide.
    pub fn is_clean_top_bar_active(&self) -> bool {
        self.is_dock_bar_on()
            && self.clean_top_bar_enabled
            && self.mac_dock_hide != MacDockHideMode::StealthHide
    }

    /// Resolves whether the Clean & Minimal Top Bar uses Floating Glass Pill silhouette vs Full-Width macOS band.
    pub fn resolved_clean_top_bar_floating(&self) -> bool {
        match self.clean_top_bar_style {
            CleanTopBarStyle::FullWidth => false,
            CleanTopBarStyle::Floating => true,
            CleanTopBarStyle::Auto => matches!(
                self.tb_theme,
                TaskbarTheme::LiquidGlassClear
                    | TaskbarTheme::MatterTilesLight
                    | TaskbarTheme::MatterTilesDark
                    | TaskbarTheme::SquircleFrosted
                    | TaskbarTheme::BubblesDark
                    | TaskbarTheme::BubblesLight
                    | TaskbarTheme::PillsDetached
                    | TaskbarTheme::RosePineTiles
                    | TaskbarTheme::CatppuccinTiles
                    | TaskbarTheme::OneUiLightChips
                    | TaskbarTheme::OneUiDarkChips
            ),
        }
    }

    /// Sets the Status Menu Bar edge (`Top` or `Bottom`) and auto-swaps the app dock
    /// to the opposite horizontal edge if they would otherwise collide on the same edge.
    pub fn set_clean_top_bar_edge(&mut self, edge: CleanTopBarEdge) {
        self.clean_top_bar_edge = edge;
        if self.is_dock_bar_on() && self.clean_top_bar_enabled {
            match (edge, self.mac_dock_edge) {
                (CleanTopBarEdge::Top, MacDockEdge::Top) => {
                    self.mac_dock_edge = MacDockEdge::Bottom;
                }
                (CleanTopBarEdge::Bottom, MacDockEdge::Bottom) => {
                    self.mac_dock_edge = MacDockEdge::Top;
                }
                _ => {}
            }
        }
    }

    /// Resets dock size, magnification, label font, glass opacity, Top Bar defaults, and module
    /// overrides back to the active theme's curated defaults while keeping `self.tb_theme` & pins!
    pub fn reset_dock_style_keep_theme(&mut self) {
        let cur_theme = self.tb_theme;
        let cur_pins = self.mac_dock_pinned.clone();
        let cur_edge = self.mac_dock_edge;
        let cur_hide = self.mac_dock_hide;
        self.reset_dock_widget_overrides();
        self.mac_dock_icon_size = 36;
        self.mac_dock_mag_pct = 155;
        self.mac_dock_bounce = true;
        self.mac_dock_dots = true;
        self.tb_hover_react = true;
        self.tb_ambient_anim = true;
        self.clean_top_bar_edge = CleanTopBarEdge::Top;
        self.clean_top_bar_style = CleanTopBarStyle::Auto;
        self.clean_top_bar_desktop_only = false;
        self.apply_taskbar_theme_preset(cur_theme);
        self.mac_dock_pinned = cur_pins;
        self.mac_dock_hide = cur_hide;
        self.sync_hide_mode_from_dock();
        self.set_mac_dock_edge_smart(cur_edge);
    }

    /// Sets `mac_dock_edge` while keeping the Clean & Minimal Top Bar on the opposite horizontal
    /// edge whenever both would otherwise share `Top` or `Bottom`.
    pub fn set_mac_dock_edge_smart(&mut self, edge: MacDockEdge) {
        self.mac_dock_edge = edge;
        if self.is_clean_top_bar_active() {
            match (edge, self.clean_top_bar_edge) {
                (MacDockEdge::Top, CleanTopBarEdge::Top) => {
                    self.clean_top_bar_edge = CleanTopBarEdge::Bottom;
                }
                (MacDockEdge::Bottom, CleanTopBarEdge::Bottom) => {
                    self.clean_top_bar_edge = CleanTopBarEdge::Top;
                }
                _ => {}
            }
        }
    }

    /// Updates `anim_fps` (clamped 15..=120) and keeps legacy `viz_eco_fps` synced (`<= 24`).
    pub fn set_anim_fps(&mut self, fps: u32) {
        self.anim_fps = Self::clamp_anim_fps(fps);
        self.viz_eco_fps = self.anim_fps <= 24;
    }

    /// Computes effective animation FPS after applying `PerformanceMode` auto-throttling:
    /// - `Smooth`: always runs at `anim_fps`.
    /// - `Balanced`: drops to `<= 20 FPS` automatically on Battery/Power Saver or while focused
    ///   in another application.
    /// - `Eco`: always caps animations to `<= 18 FPS`.
    pub fn effective_anim_fps(&self, on_battery_or_saver: bool, external_app_focused: bool) -> u32 {
        let base = Self::clamp_anim_fps(self.anim_fps);
        match self.performance_mode {
            PerformanceMode::Smooth => base,
            PerformanceMode::Balanced => {
                if on_battery_or_saver || external_app_focused {
                    base.min(20)
                } else {
                    base
                }
            }
            PerformanceMode::Eco => base.min(18),
        }
    }

    /// Unifies 12h/24h clock format across desktop widgets and the taskbar/dock clock.
    pub fn set_clock_24h(&mut self, use_24h: bool) {
        self.widgets_24h = use_24h;
        self.w_clock.use_24h = Some(use_24h);
    }

    /// Target frame interval (ms) for active audio spectrum visualizers (taskbar & music widget).
    pub fn viz_audio_frame_ms(&self) -> u32 {
        self.anim_frame_ms()
    }

    /// Target frame interval (ms) for the Dynamic Island music visualizer.
    pub fn viz_island_frame_ms(&self) -> u32 {
        self.anim_frame_ms()
    }
    pub fn reset_dock_widget_overrides(&mut self) {
        self.w_shelf = DockShelfFrameConfig::default();
        self.w_apps = DockAppWidgetConfig::default();
        self.w_clock = DockClockWidgetConfig::default();
        self.w_tray = DockTrayWidgetConfig::default();
        self.w_hud = DockHudWidgetConfig::default();
        self.w_util = DockUtilityWidgetConfig::default();
    }

    #[inline]
    pub fn is_native_tb_all_elements_off(&self) -> bool {
        !self.tb_show_backdrop && !self.tb_show_rim && !self.tb_show_fx
    }

    #[inline]
    pub fn is_dock_all_elements_off(&self) -> bool {
        self.w_shelf.is_all_elements_off()
            && self.w_apps.is_all_elements_off()
            && self.w_clock.is_all_elements_off()
            && self.w_tray.is_all_elements_off()
            && self.w_hud.is_all_elements_off()
            && self.w_util.is_all_elements_off()
    }

    pub fn canvas_elem(&self, target: WidgetCanvasTarget, idx: usize) -> ClockElemTransform {
        match target {
            WidgetCanvasTarget::DateTime => *self.dt_elem_transforms.get(idx).unwrap_or(&ClockElemTransform::default()),
            WidgetCanvasTarget::Vitals => *self.vitals_elem_transforms.get(idx).unwrap_or(&ClockElemTransform::default()),
            WidgetCanvasTarget::Music => *self.music_elem_transforms.get(idx).unwrap_or(&ClockElemTransform::default()),
            WidgetCanvasTarget::ControlCenter => *self.cc_elem_transforms.get(idx).unwrap_or(&ClockElemTransform::default()),
            WidgetCanvasTarget::Apps => *self.apps_elem_transforms.get(idx).unwrap_or(&ClockElemTransform::default()),
            WidgetCanvasTarget::Ext(id) => *self.ext_widgets[id.idx()]
                .elem_transforms
                .get(idx)
                .unwrap_or(&ClockElemTransform::default()),
        }
    }

    pub fn canvas_elem_mut(&mut self, target: WidgetCanvasTarget, idx: usize) -> &mut ClockElemTransform {
        let cap = target.elem_count().saturating_sub(1);
        let i = idx.min(cap);
        match target {
            WidgetCanvasTarget::DateTime => &mut self.dt_elem_transforms[i],
            WidgetCanvasTarget::Vitals => &mut self.vitals_elem_transforms[i],
            WidgetCanvasTarget::Music => &mut self.music_elem_transforms[i],
            WidgetCanvasTarget::ControlCenter => &mut self.cc_elem_transforms[i],
            WidgetCanvasTarget::Apps => &mut self.apps_elem_transforms[i],
            WidgetCanvasTarget::Ext(id) => &mut self.ext_widgets[id.idx()].elem_transforms[i],
        }
    }

    pub fn canvas_elem_font(&self, target: WidgetCanvasTarget, idx: usize) -> &str {
        let cap = target.elem_count().saturating_sub(1);
        let i = idx.min(cap);
        match target {
            WidgetCanvasTarget::DateTime => &self.dt_elem_fonts[i],
            WidgetCanvasTarget::Vitals => &self.vitals_elem_fonts[i],
            WidgetCanvasTarget::Music => &self.music_elem_fonts[i],
            WidgetCanvasTarget::ControlCenter => &self.cc_elem_fonts[i],
            WidgetCanvasTarget::Apps => &self.apps_elem_fonts[i],
            WidgetCanvasTarget::Ext(id) => &self.ext_widgets[id.idx()].elem_fonts[i],
        }
    }

    pub fn canvas_elem_font_mut(&mut self, target: WidgetCanvasTarget, idx: usize) -> &mut String {
        let cap = target.elem_count().saturating_sub(1);
        let i = idx.min(cap);
        match target {
            WidgetCanvasTarget::DateTime => &mut self.dt_elem_fonts[i],
            WidgetCanvasTarget::Vitals => &mut self.vitals_elem_fonts[i],
            WidgetCanvasTarget::Music => &mut self.music_elem_fonts[i],
            WidgetCanvasTarget::ControlCenter => &mut self.cc_elem_fonts[i],
            WidgetCanvasTarget::Apps => &mut self.apps_elem_fonts[i],
            WidgetCanvasTarget::Ext(id) => &mut self.ext_widgets[id.idx()].elem_fonts[i],
        }
    }

    pub fn reset_canvas_elem(&mut self, target: WidgetCanvasTarget, idx: usize) {
        *self.canvas_elem_mut(target, idx) = ClockElemTransform::default();
        self.canvas_elem_font_mut(target, idx).clear();
    }

    pub fn reset_canvas_all(&mut self, target: WidgetCanvasTarget) {
        for i in 0..target.elem_count() {
            self.reset_canvas_elem(target, i);
        }
    }

    pub fn horology_parts(&self, target: WidgetCanvasTarget) -> crate::horology_pbr::HorologyPartConfig {
        match target {
            WidgetCanvasTarget::DateTime => self.dt_horology,
            WidgetCanvasTarget::Apps => self.apps_horology,
            WidgetCanvasTarget::Vitals => self.vitals_horology,
            WidgetCanvasTarget::Music => self.music_horology,
            WidgetCanvasTarget::ControlCenter => self.cc_horology,
            WidgetCanvasTarget::Ext(id) => self.ext_widgets[id.idx()].horology,
        }
    }

    pub fn horology_parts_mut(
        &mut self,
        target: WidgetCanvasTarget,
    ) -> &mut crate::horology_pbr::HorologyPartConfig {
        match target {
            WidgetCanvasTarget::DateTime => &mut self.dt_horology,
            WidgetCanvasTarget::Apps => &mut self.apps_horology,
            WidgetCanvasTarget::Vitals => &mut self.vitals_horology,
            WidgetCanvasTarget::Music => &mut self.music_horology,
            WidgetCanvasTarget::ControlCenter => &mut self.cc_horology,
            WidgetCanvasTarget::Ext(id) => &mut self.ext_widgets[id.idx()].horology,
        }
    }

    pub fn active_horology_theme(
        &self,
        target: WidgetCanvasTarget,
    ) -> Option<crate::horology_pbr::HorologyTheme> {
        match target {
            WidgetCanvasTarget::DateTime => self.dt_style.horology_theme(),
            WidgetCanvasTarget::Music => self.music_style.horology_theme(),
            WidgetCanvasTarget::Vitals => vitals_style_horology(self.vitals_style),
            WidgetCanvasTarget::ControlCenter => cc_style_horology(self.cc_tile_style),
            WidgetCanvasTarget::Apps => apps_style_horology(self.apps_tile_shape),
            WidgetCanvasTarget::Ext(id) => self.ext_widgets[id.idx()].theme.horology_theme(),
        }
    }

    pub fn apply_canvas_style_preset(
        &mut self,
        target: WidgetCanvasTarget,
        idx: usize,
        preset: CanvasStylePreset,
    ) {
        let is_card = target.is_card_slot(idx);
        let is_box = target.is_box_elem(idx);
        let (col1, col2, angle, blur, glow, bw, bcol, font_name, card_glass, card_rad) = match preset {
            CanvasStylePreset::CyberNeon => (
                if is_card { 0x0A1224 } else { 0x00F0FF },
                Some(if is_card { 0x1A0933 } else { 0xFF007F }),
                135u16,
                14u8,
                Some(0x00F0FF),
                if is_box { 2u8 } else { 0u8 },
                Some(0x00F0FF),
                "Consolas",
                145u8,
                12u8,
            ),
            CanvasStylePreset::FrostedLuxury => (
                if is_card { 0x222938 } else { 0xFFFDF9 },
                Some(if is_card { 0x121620 } else { 0xD8E6F5 }),
                160u16,
                8u8,
                Some(0xFFFFFF),
                if is_box { 1u8 } else { 0u8 },
                Some(0xE2E8F0),
                "Georgia",
                115u8,
                22u8,
            ),
            CanvasStylePreset::RetroPhosphor => (
                if is_card { 0x140E04 } else { 0xFFB000 },
                Some(if is_card { 0x0A0702 } else { 0xFF6A00 }),
                90u16,
                12u8,
                Some(0xFF8C00),
                if is_box { 2u8 } else { 0u8 },
                Some(0xFFB000),
                "Courier New",
                168u8,
                6u8,
            ),
            CanvasStylePreset::MinimalSlate => (
                if is_card { 0x0F172A } else { 0xF1F5F9 },
                None,
                135u16,
                4u8,
                None,
                if is_box { 1u8 } else { 0u8 },
                Some(0x64748B),
                "Segoe UI Semibold",
                150u8,
                16u8,
            ),
            CanvasStylePreset::GoldAccent => (
                if is_card { 0x1C140A } else { 0xFFD700 },
                Some(if is_card { 0x0E0904 } else { 0xCC8400 }),
                135u16,
                10u8,
                Some(0xFFB800),
                if is_box { 2u8 } else { 0u8 },
                Some(0xFFD700),
                "Palatino Linotype",
                155u8,
                18u8,
            ),
        };
        {
            let slot = self.canvas_elem_mut(target, idx);
            slot.visible = true;
            slot.color_rgb = Some(col1);
            slot.color2_rgb = col2;
            slot.gradient_angle = angle;
            slot.shadow_blur = blur;
            slot.glow_rgb = glow;
            slot.border_width = bw;
            slot.border_rgb = bcol;
        }
        if is_card {
            match target {
                WidgetCanvasTarget::DateTime => {
                    self.dt_glass_alpha = card_glass;
                    self.dt_corner_radius = card_rad;
                }
                WidgetCanvasTarget::Apps => {
                    self.apps_glass_alpha = card_glass;
                    self.apps_corner_radius = card_rad;
                }
                WidgetCanvasTarget::Vitals => {
                    self.vitals_glass_alpha = card_glass;
                    self.vitals_corner_radius = card_rad;
                }
                WidgetCanvasTarget::Music => {
                    self.music_glass_alpha = card_glass;
                    self.music_corner_radius = card_rad;
                }
                WidgetCanvasTarget::ControlCenter => {
                    self.cc_glass_alpha = card_glass;
                    self.cc_corner_radius = card_rad;
                }
                WidgetCanvasTarget::Ext(id) => {
                    self.ext_widgets[id.idx()].glass_alpha = card_glass;
                    self.ext_widgets[id.idx()].corner_radius = card_rad;
                }
            }
        } else if !is_box {
            *self.canvas_elem_font_mut(target, idx) = font_name.to_string();
        }
    }

    pub fn set_all_widgets_hover_fx(&mut self, fx: TaskbarVizStyle) {
        self.dt_hover_fx = fx;
        self.apps_hover_fx = fx;
        self.vitals_hover_fx = fx;
        self.music_hover_fx = fx;
        self.cc_hover_fx = fx;
        for w in &mut self.ext_widgets {
            w.hover_fx = fx;
        }
    }

    #[inline]
    pub fn is_datetime_all_elements_off(&self) -> bool {
        let tr = &self.dt_elem_transforms;
        let plate_on = self.dt_show_plate && tr[ClockElemId::Card.idx()].visible;
        let clock_on = self.dt_show_clock && tr[ClockElemId::Clock.idx()].visible;
        let date_on = self.dt_show_date
            && (tr[ClockElemId::Day.idx()].visible || tr[ClockElemId::Date.idx()].visible);
        let wk_on = self.dt_show_weekday && tr[ClockElemId::Weekday.idx()].visible;
        let ampm_on = !self.widgets_24h && self.dt_show_ampm && tr[ClockElemId::AmPm.idx()].visible;
        let orb_on = self.dt_show_orb && tr[ClockElemId::Orb.idx()].visible;
        let spine_on = self.dt_show_spine && tr[ClockElemId::Spine.idx()].visible;
        let sec_on = self.dt_show_seconds
            && (tr[ClockElemId::Clock.idx()].visible || tr[ClockElemId::Orb.idx()].visible);
        !(plate_on || clock_on || date_on || wk_on || ampm_on || orb_on || spine_on || sec_on)
    }

    #[inline]
    pub fn is_apps_all_elements_off(&self) -> bool {
        let tr = &self.apps_elem_transforms;
        let plate_on = self.apps_show_plate && tr[4].visible;
        let tiles_on = self.apps_show_tiles && tr[0].visible && !self.apps.is_empty();
        let labels_on = self.apps_show_labels && tr[1].visible;
        let dots_on = self.apps_show_dots && tr[2].visible;
        let add_on = self.apps_show_add_tile && tr[3].visible && self.apps.len() < 12;
        !(plate_on || tiles_on || labels_on || dots_on || add_on)
    }

    #[inline]
    pub fn is_vitals_all_elements_off(&self) -> bool {
        let tr = &self.vitals_elem_transforms;
        let plate_on = self.vitals_show_plate && tr[5].visible;
        let cpu_on = self.vitals_show_cpu && tr[0].visible;
        let ram_on = self.vitals_show_ram && tr[1].visible;
        let pwr_on = self.vitals_show_power && tr[2].visible;
        let btns_on = self.vitals_show_buttons && (tr[3].visible || tr[4].visible);
        let bars_on = self.vitals_show_bars && (cpu_on || ram_on || pwr_on);
        let div_on = self.vitals_show_dividers && (cpu_on || ram_on || pwr_on || btns_on);
        !(plate_on || cpu_on || ram_on || pwr_on || btns_on || bars_on || div_on)
    }

    #[inline]
    pub fn is_music_all_elements_off(&self) -> bool {
        let tr = &self.music_elem_transforms;
        let plate_on = self.music_show_plate && tr[5].visible;
        let cov_on = self.music_show_cover && tr[0].visible;
        let tit_on = self.music_show_title && tr[1].visible;
        let art_on = self.music_show_artist && tr[2].visible;
        let ctl_on = self.music_show_controls && tr[3].visible;
        let viz_on = self.music_show_viz && tr[4].visible;
        let bdg_on = self.music_show_badge;
        !(plate_on || cov_on || tit_on || art_on || ctl_on || viz_on || bdg_on)
    }

    #[inline]
    pub fn is_cc_all_elements_off(&self) -> bool {
        let tr = &self.cc_elem_transforms;
        let plate_on = self.cc_show_plate && tr[5].visible;
        let tiles_on = self.cc_show_tiles && tr[0].visible;
        let br_on = self.cc_show_bright && tr[1].visible;
        let vol_on = self.cc_show_vol && tr[2].visible;
        let scr_on = self.cc_show_scrollbar && tr[3].visible;
        let div_on = self.cc_show_dividers && tr[4].visible;
        let lbl_on = self.cc_show_labels && (tiles_on || br_on || vol_on || scr_on);
        !(plate_on || tiles_on || br_on || vol_on || scr_on || div_on || lbl_on)
    }

    pub fn any_match_wallpaper(&self) -> bool {
        self.tb_match_wallpaper
            || self.dt_match_wallpaper
            || self.apps_match_wallpaper
            || self.vitals_match_wallpaper
            || self.music_match_wallpaper
            || self.cc_match_wallpaper
            || self.ext_widgets.iter().any(|w| w.match_wallpaper)
    }

    pub fn apply_taskbar_theme_preset(&mut self, theme: TaskbarTheme) {
        self.tb_theme = theme;
        let (pill, surf, tint, amber, viz, mirror) = theme.preview_preset();
        self.pill_mode = pill;
        self.surface_mode = surf;
        self.tint_alpha = tint;
        self.amber_border = amber;
        self.tb_viz_style = viz;
        self.tb_viz_mirror = mirror;
        self.tb_viz_opacity = 85;
        self.tb_show_backdrop = true;
        self.tb_show_rim = true;
        self.tb_show_fx = true;
        self.corner_radius = match theme {
            TaskbarTheme::Retro95Classic
            | TaskbarTheme::FrutigerAero7
            | TaskbarTheme::AerisGlassLight
            | TaskbarTheme::BlobLight
            | TaskbarTheme::CleanSlateCards => 0,
            TaskbarTheme::EditorialPaperDock | TaskbarTheme::TerminalPhosphorDock => 4,
            TaskbarTheme::UnixPolybar
            | TaskbarTheme::CyberpunkHudDock
            | TaskbarTheme::NextSlatePlankDock
            | TaskbarTheme::KumikoWoodDock
            | TaskbarTheme::BauhausGeometricDock => 8,
            TaskbarTheme::CyberArasakaDeck
            | TaskbarTheme::SolsticeAstrolabeDock
            | TaskbarTheme::DockLikeLight
            | TaskbarTheme::DockLikeDark => 10,
            TaskbarTheme::CyberNeonDock
            | TaskbarTheme::HoverNeonPlasma
            | TaskbarTheme::VaporwaveHorizonDock
            | TaskbarTheme::ObsidianEmberDock
            | TaskbarTheme::MatterTilesLight
            | TaskbarTheme::MatterTilesDark
            | TaskbarTheme::SquircleFrosted
            | TaskbarTheme::RosePineTiles
            | TaskbarTheme::CatppuccinTiles => 12,
            TaskbarTheme::KineticSynthTrampoline | TaskbarTheme::PillsDetached => 14,
            TaskbarTheme::PureClearPulse => 16,
            TaskbarTheme::FrostedSplit
            | TaskbarTheme::BassPulseSplit
            | TaskbarTheme::FrutigerAquaDock
            | TaskbarTheme::PrismRefractionDock
            | TaskbarTheme::StarfieldWarpDock
            | TaskbarTheme::LuminosityFloat => 18,
            TaskbarTheme::BorderlessGhost => 20,
            TaskbarTheme::QuartzShellDock
            | TaskbarTheme::KatanaGlassDock
            | TaskbarTheme::AuroraRibbonDock
            | TaskbarTheme::LiquidChromeDock
            | TaskbarTheme::SakuraBreezeDock
            | TaskbarTheme::BiolumAbyssDock
            | TaskbarTheme::CupertinoDock
            | TaskbarTheme::KineticCupertinoWave
            | TaskbarTheme::KineticHydroChamber
            | TaskbarTheme::LiquidChrome
            | TaskbarTheme::HoverWaterRipples
            | TaskbarTheme::Os26LiquidGlassLight
            | TaskbarTheme::Os26LiquidGlassDark
            | TaskbarTheme::OneUiLightChips
            | TaskbarTheme::OneUiDarkChips
            | TaskbarTheme::BubblesDark
            | TaskbarTheme::BubblesLight
            | TaskbarTheme::LiquidGlassClear => 24,
            TaskbarTheme::BrushedTitaniumDock
            | TaskbarTheme::VacuumTubeHiFiDock
            | TaskbarTheme::VintageCassetteDock
            | TaskbarTheme::WarmWalnutDeskDock
            | TaskbarTheme::OceanTideDock => 8,
            TaskbarTheme::SwissHorologyDock
            | TaskbarTheme::SaddleLeatherDock
            | TaskbarTheme::ObsidianCarbonDock
            | TaskbarTheme::CarraraMarbleDock
            | TaskbarTheme::FrostedGlacierIceDock => 14,
            TaskbarTheme::RealisticWaterDock
            | TaskbarTheme::RainWindowDock
            | TaskbarTheme::ZenCalmMistDock
            | TaskbarTheme::CalmKoiPondDock
            | TaskbarTheme::SunlitForestCanopyDock
            | TaskbarTheme::SilkLinenCalmDock
            | TaskbarTheme::AbyssalTrenchDock
            | TaskbarTheme::GoldenHourHorizonDock => 22,
            TaskbarTheme::DynamicIslandDock
            | TaskbarTheme::DynamicCapsuleDock
            | TaskbarTheme::InvisibleGhostDock => 26,
            _ => 20,
        };
        self.tb_match_wallpaper = matches!(
            theme,
            TaskbarTheme::WallpaperGlass
                | TaskbarTheme::AuroraBorealis
                | TaskbarTheme::LuminosityFloat
        );
        self.tb_viz_color_mode = if self.tb_match_wallpaper {
            VizColorMode::MatchWallpaper
        } else {
            VizColorMode::ThemeNative
        };
        let was_dock = self.mac_dock_enabled;
        let now_dock = theme.uses_virtual_dock();
        self.mac_dock_enabled = now_dock;
        self.dock_bar_enabled = theme.is_clean_minimal();
        self.clean_top_bar_enabled = theme.is_clean_minimal();
        if self.is_clean_top_bar_active() {
            self.set_clean_top_bar_edge(self.clean_top_bar_edge);
        }
        if was_dock && !now_dock {
            self.sync_hide_mode_from_dock();
        } else if !was_dock && now_dock {
            self.sync_dock_hide_from_mode();
        } else if now_dock && self.mac_dock_hide == MacDockHideMode::SmartAutoHide {
            self.mac_dock_hide = MacDockHideMode::AlwaysVisible;
        }
    }

    pub fn set_tb_viz_color_mode(&mut self, mode: VizColorMode) {
        self.tb_viz_color_mode = mode;
        self.tb_match_wallpaper = mode == VizColorMode::MatchWallpaper;
    }

    pub fn set_tb_match_wallpaper(&mut self, on: bool) {
        self.tb_match_wallpaper = on;
        if on {
            self.tb_viz_color_mode = VizColorMode::MatchWallpaper;
        } else if self.tb_viz_color_mode == VizColorMode::MatchWallpaper {
            self.tb_viz_color_mode = VizColorMode::ThemeNative;
        }
    }

    pub fn sync_hide_mode_from_dock(&mut self) {
        self.mode = match self.mac_dock_hide {
            MacDockHideMode::AlwaysVisible => TaskbarMode::AlwaysVisible,
            MacDockHideMode::HoverReveal => TaskbarMode::HoverReveal,
            MacDockHideMode::SmartAutoHide => TaskbarMode::SmartMaximize,
            MacDockHideMode::StealthHide => TaskbarMode::StealthHide,
        };
    }

    pub fn sync_dock_hide_from_mode(&mut self) {
        self.mac_dock_hide = match self.mode {
            TaskbarMode::AlwaysVisible | TaskbarMode::SmartMaximize => {
                MacDockHideMode::AlwaysVisible
            }
            TaskbarMode::HoverReveal => MacDockHideMode::HoverReveal,
            TaskbarMode::StealthHide => MacDockHideMode::StealthHide,
        };
    }

    pub fn apply_music_style_preset(&mut self, style: MusicStyle) {
        self.music_style = style;
        self.music_show_title = true;
        self.music_show_viz = true;
        if style.pbr_material().is_some() {
            self.music_use_custom_color = false;
        }
        self.music_elem_transforms = [ClockElemTransform::default(); 6];
        self.music_elem_fonts = std::array::from_fn(|_| String::new());
        match style {
            MusicStyle::Sanctuary => {
                self.music_glass_alpha = 55;
                self.music_rim_alpha = 125;
            }
            MusicStyle::VinylDeck => {
                self.music_glass_alpha = 72;
                self.music_rim_alpha = 165;
            }
            MusicStyle::MinimalPill => {
                self.music_glass_alpha = 36;
                self.music_rim_alpha = 110;
            }
            MusicStyle::SpectrumDeck => {
                self.music_glass_alpha = 48;
                self.music_rim_alpha = 150;
            }
            MusicStyle::NeonWave => {
                self.music_glass_alpha = 42;
                self.music_rim_alpha = 175;
            }
            MusicStyle::CyberHud => {
                self.music_glass_alpha = 88;
                self.music_rim_alpha = 210;
            }
            MusicStyle::TurntablePro => {
                self.music_glass_alpha = 82;
                self.music_rim_alpha = 185;
            }
            MusicStyle::DualFloatOrb => {
                self.music_glass_alpha = 52;
                self.music_rim_alpha = 155;
            }
            MusicStyle::EditorialSleeve => {
                self.music_glass_alpha = 74;
                self.music_rim_alpha = 165;
            }
            MusicStyle::RadialHalo => {
                self.music_glass_alpha = 58;
                self.music_rim_alpha = 160;
            }
            MusicStyle::OscilloscopeCrt => {
                self.music_glass_alpha = 92;
                self.music_rim_alpha = 190;
            }
            MusicStyle::RibbonFlow => {
                self.music_glass_alpha = 46;
                self.music_rim_alpha = 145;
            }
            MusicStyle::EclipsePlayer => {
                self.music_glass_alpha = 68;
                self.music_rim_alpha = 175;
            }
            MusicStyle::CassetteDeck => {
                self.music_glass_alpha = 96;
                self.music_rim_alpha = 195;
            }
            MusicStyle::AnalogVuMeter => {
                self.music_glass_alpha = 92;
                self.music_rim_alpha = 190;
            }
            MusicStyle::HifiAmplifier => {
                self.music_glass_alpha = 90;
                self.music_rim_alpha = 180;
            }
            MusicStyle::WinampRack => {
                self.music_glass_alpha = 94;
                self.music_rim_alpha = 185;
            }
            MusicStyle::FlipJukebox => {
                self.music_glass_alpha = 86;
                self.music_rim_alpha = 175;
            }
            MusicStyle::VacuumTubeAmp => {
                self.music_glass_alpha = 94;
                self.music_rim_alpha = 195;
            }
            MusicStyle::BrushedTitaniumDeck => {
                self.music_glass_alpha = 92;
                self.music_rim_alpha = 188;
            }
            MusicStyle::RainWindowPlayer => {
                self.music_glass_alpha = 76;
                self.music_rim_alpha = 165;
            }
            MusicStyle::CalmWaterRipple => {
                self.music_glass_alpha = 68;
                self.music_rim_alpha = 170;
            }
            MusicStyle::LeatherFieldAudio => {
                self.music_glass_alpha = 95;
                self.music_rim_alpha = 185;
            }
            MusicStyle::WalnutHiFiConsole => {
                self.music_glass_alpha = 94;
                self.music_rim_alpha = 180;
            }
            MusicStyle::AbyssalTrenchAudio => {
                self.music_glass_alpha = 88;
                self.music_rim_alpha = 195;
            }
            MusicStyle::GlacierIceAudio => {
                self.music_glass_alpha = 78;
                self.music_rim_alpha = 205;
            }
            MusicStyle::CarbonSteelDeck => {
                self.music_glass_alpha = 96;
                self.music_rim_alpha = 185;
            }
            MusicStyle::GoldenHourAudio => {
                self.music_glass_alpha = 84;
                self.music_rim_alpha = 192;
            }
            MusicStyle::DynamicIslandCapsule => {
                self.music_glass_alpha = 64;
                self.music_rim_alpha = 140;
                self.music_corner_radius = 37;
            }
            s if s.horology_theme().is_some() => {
                self.music_glass_alpha = 104;
                self.music_rim_alpha = 215;
                self.music_corner_radius = 18;
                self.music_use_custom_color = false;
                self.music_horology = crate::horology_pbr::HorologyPartConfig::default();
            }
            _ => {}
        }
    }

    pub fn apply_clock_style_preset(&mut self, style: ClockStyle) {
        self.dt_style = style;
        self.dt_show_clock = true;
        if style.pbr_material().is_some() || style.horology_theme().is_some() {
            self.dt_use_custom_color = false;
        }
        if style.horology_theme().is_some() {
            self.dt_horology = crate::horology_pbr::HorologyPartConfig::default();
        }
        self.dt_font_day.clear();
        self.dt_font_script.clear();
        self.dt_font_clock.clear();
        self.dt_font_sub.clear();
        self.dt_letter_spacing = 0;
        self.dt_elem_transforms = [ClockElemTransform::default(); 8];
        self.dt_elem_fonts = std::array::from_fn(|_| String::new());
        match style {
            ClockStyle::Signature => {
                self.dt_show_spine = true;
                self.dt_show_orb = true;
                self.dt_corner_radius = 20;
                self.dt_align = ClockAlign::Left;
            }
            ClockStyle::Minimal => {
                self.dt_show_spine = false;
                self.dt_show_orb = true;
                self.dt_corner_radius = 18;
                self.dt_align = ClockAlign::Center;
            }
            ClockStyle::Editorial => {
                self.dt_show_spine = true;
                self.dt_show_orb = false;
                self.dt_corner_radius = 14;
                self.dt_align = ClockAlign::Left;
            }
            ClockStyle::Cyber => {
                self.dt_show_spine = true;
                self.dt_show_orb = true;
                self.dt_corner_radius = 8;
                self.dt_align = ClockAlign::Left;
            }
            ClockStyle::SplitStack => {
                self.dt_show_spine = true;
                self.dt_show_orb = true;
                self.dt_corner_radius = 22;
                self.dt_align = ClockAlign::Center;
            }
            ClockStyle::Island => {
                self.dt_show_spine = false;
                self.dt_show_orb = true;
                self.dt_corner_radius = 36;
                self.dt_align = ClockAlign::Center;
            }
            ClockStyle::TypoWatermark => {
                self.dt_show_spine = true;
                self.dt_show_orb = false;
                self.dt_corner_radius = 22;
                self.dt_align = ClockAlign::Right;
            }
            ClockStyle::BauhausGeo => {
                self.dt_show_spine = true;
                self.dt_show_orb = true;
                self.dt_corner_radius = 6;
                self.dt_align = ClockAlign::Left;
            }
            ClockStyle::JapaneseZen => {
                self.dt_show_spine = true;
                self.dt_show_orb = false;
                self.dt_corner_radius = 12;
                self.dt_align = ClockAlign::Left;
            }
            ClockStyle::ArtDecoGilded => {
                self.dt_show_spine = true;
                self.dt_show_orb = true;
                self.dt_corner_radius = 10;
                self.dt_align = ClockAlign::Center;
            }
            ClockStyle::GlassRibbon => {
                self.dt_show_spine = false;
                self.dt_show_orb = true;
                self.dt_corner_radius = 44;
                self.dt_align = ClockAlign::Left;
            }
            ClockStyle::ChronoDial => {
                self.dt_show_spine = true;
                self.dt_show_orb = true;
                self.dt_corner_radius = 24;
                self.dt_align = ClockAlign::Left;
            }
            ClockStyle::OrbitalEclipse => {
                self.dt_show_spine = false;
                self.dt_show_orb = true;
                self.dt_corner_radius = 24;
                self.dt_align = ClockAlign::Center;
            }
            ClockStyle::SunDialHorizon => {
                self.dt_show_spine = true;
                self.dt_show_orb = true;
                self.dt_corner_radius = 24;
                self.dt_align = ClockAlign::Center;
            }
            ClockStyle::FlipMech => {
                self.dt_show_spine = true;
                self.dt_show_orb = true;
                self.dt_corner_radius = 14;
                self.dt_align = ClockAlign::Center;
            }
            ClockStyle::NixieTube => {
                self.dt_show_spine = true;
                self.dt_show_orb = true;
                self.dt_corner_radius = 14;
                self.dt_align = ClockAlign::Center;
            }
            ClockStyle::RetroCasio => {
                self.dt_show_spine = true;
                self.dt_show_orb = true;
                self.dt_corner_radius = 12;
                self.dt_align = ClockAlign::Left;
            }
            ClockStyle::MatrixBinary => {
                self.dt_show_spine = true;
                self.dt_show_orb = true;
                self.dt_corner_radius = 10;
                self.dt_align = ClockAlign::Left;
            }
            ClockStyle::SwissChronometer => {
                self.dt_show_spine = true;
                self.dt_show_orb = true;
                self.dt_corner_radius = 22;
                self.dt_align = ClockAlign::Left;
            }
            ClockStyle::BraunIndustrial => {
                self.dt_show_spine = true;
                self.dt_show_orb = true;
                self.dt_corner_radius = 12;
                self.dt_align = ClockAlign::Left;
            }
            ClockStyle::WaterCausticsClock => {
                self.dt_show_spine = false;
                self.dt_show_orb = true;
                self.dt_corner_radius = 24;
                self.dt_align = ClockAlign::Center;
            }
            ClockStyle::ZenMistClock => {
                self.dt_show_spine = true;
                self.dt_show_orb = true;
                self.dt_corner_radius = 22;
                self.dt_align = ClockAlign::Center;
            }
            ClockStyle::LeatherFieldClock => {
                self.dt_show_spine = true;
                self.dt_show_orb = true;
                self.dt_corner_radius = 16;
                self.dt_align = ClockAlign::Left;
            }
            ClockStyle::WalnutDeskClock => {
                self.dt_show_spine = true;
                self.dt_show_orb = true;
                self.dt_corner_radius = 14;
                self.dt_align = ClockAlign::Left;
            }
            ClockStyle::RainWindowClock => {
                self.dt_show_spine = false;
                self.dt_show_orb = true;
                self.dt_corner_radius = 22;
                self.dt_align = ClockAlign::Center;
            }
            ClockStyle::AbyssalTrenchClock => {
                self.dt_show_spine = true;
                self.dt_show_orb = true;
                self.dt_corner_radius = 20;
                self.dt_align = ClockAlign::Center;
            }
            ClockStyle::GlacierIceClock => {
                self.dt_show_spine = false;
                self.dt_show_orb = true;
                self.dt_corner_radius = 18;
                self.dt_align = ClockAlign::Center;
            }
            ClockStyle::CarbonSteelClock => {
                self.dt_show_spine = true;
                self.dt_show_orb = true;
                self.dt_corner_radius = 10;
                self.dt_align = ClockAlign::Left;
            }
            ClockStyle::GoldenHourClock => {
                self.dt_show_spine = true;
                self.dt_show_orb = true;
                self.dt_corner_radius = 22;
                self.dt_align = ClockAlign::Center;
            }
            ClockStyle::AlpineMistClock => {
                self.dt_show_spine = true;
                self.dt_show_orb = true;
                self.dt_corner_radius = 22;
                self.dt_align = ClockAlign::Center;
            }
            s if s.horology_theme().is_some() => {
                self.dt_show_spine = true;
                self.dt_show_orb = true;
                self.dt_glass_alpha = 110;
                self.dt_rim_alpha = 215;
                self.dt_corner_radius = 20;
                self.dt_align = ClockAlign::Left;
            }
            _ => {}
        }
    }

    pub fn resolved_dt_fonts(&self) -> (String, String, String, String) {
        let (d0, s0, c0, b0) = self.dt_style.default_fonts();
        let pick = |ov: &str, def: &str| {
            if ov.trim().is_empty() { def.to_string() } else { ov.trim().to_string() }
        };
        (
            pick(&self.dt_font_day, d0),
            pick(&self.dt_font_script, s0),
            pick(&self.dt_font_clock, c0),
            pick(&self.dt_font_sub, b0),
        )
    }
}

fn parse_bool(val: &str) -> bool {
    val != "0" && !val.eq_ignore_ascii_case("false")
}

fn parse_hex_rgb(val: &str) -> Option<u32> {
    let s = val.trim().trim_start_matches("0x").trim_start_matches("0X").trim_start_matches('#');
    u32::from_str_radix(s, 16).ok().map(|v| v & 0x00FF_FFFF)
}

#[derive(Clone, Copy, Default)]
pub struct WinTaskbarPrefs {
    pub align_center: bool,
    pub show_task_view: bool,
    pub show_widgets: bool,
}

static PORTABLE_CLI_FLAG: AtomicBool = AtomicBool::new(false);

pub fn set_portable_cli_flag(enabled: bool) {
    PORTABLE_CLI_FLAG.store(enabled, Ordering::Release);
}

pub fn portable_cli_flag() -> bool {
    PORTABLE_CLI_FLAG.load(Ordering::Acquire)
}

/// Pure testable resolver for AutumnDeck's active data root.
/// Returns `(data_dir, is_portable)`. Portable mode activates automatically when
/// `--portable` is passed OR `<exe_dir>\portable.txt` exists beside the executable.
pub fn resolve_app_data_dir(
    exe_dir: Option<&Path>,
    cli_portable: bool,
    appdata_env: Option<&Path>,
) -> (PathBuf, bool) {
    if let Some(dir) = exe_dir {
        if cli_portable || dir.join("portable.txt").is_file() {
            return (dir.join("Data"), true);
        }
    }
    let base = appdata_env
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    (base.join("AutumnDeck"), false)
}

pub fn is_portable_mode() -> bool {
    let exe = self_exe_path();
    let appdata = std::env::var("APPDATA").ok().map(PathBuf::from);
    resolve_app_data_dir(exe.parent(), portable_cli_flag(), appdata.as_deref()).1
}

pub fn app_data_dir() -> PathBuf {
    let exe = self_exe_path();
    let appdata = std::env::var("APPDATA").ok().map(PathBuf::from);
    resolve_app_data_dir(exe.parent(), portable_cli_flag(), appdata.as_deref()).0
}

pub fn status_snapshot_path() -> PathBuf {
    if is_portable_mode() {
        let dir = app_data_dir();
        let _ = fs::create_dir_all(&dir);
        dir.join("autumn_deck_status.txt")
    } else {
        std::env::temp_dir().join("autumn_deck_status.txt")
    }
}

pub fn portable_data_dir_display() -> String {
    if is_portable_mode() {
        let p = app_data_dir();
        let s = p.to_string_lossy().into_owned();
        if s.len() > 42 {
            "<exe_dir>\\Data".to_string()
        } else {
            s
        }
    } else {
        "%APPDATA%\\AutumnDeck".to_string()
    }
}

/// Converts a standard installation into True Portable Mode inside `exe_dir`:
/// creates `portable.txt`, migrates `config.ini`, backups, `pill_cache.ini`, `Themes/`,
/// `wallpapers/`, and `Diagnostics/` into `<exe_dir>\Data\`, and rewrites embedded
/// `src_data_dir` references in `config.ini` to the portable `Data` directory.
pub fn convert_to_portable_in(
    exe_dir: &Path,
    src_data_dir: &Path,
    cfg: &AppConfig,
) -> Result<PathBuf, String> {
    let dst_data_dir = exe_dir.join("Data");
    fs::create_dir_all(&dst_data_dir)
        .map_err(|e| format!("Cannot create portable Data folder: {}", e))?;

    let marker = exe_dir.join("portable.txt");
    let marker_body = "AutumnDeck Portable Mode\r\nKeep this file beside AutumnDeck.exe to store all settings, themes, wallpapers, and logs inside .\\Data\\\r\n";
    fs::write(&marker, marker_body)
        .map_err(|e| format!("Cannot write portable.txt (folder may be read-only): {}", e))?;

    if !src_data_dir.eq_ignore_ascii_case_path(&dst_data_dir) && src_data_dir.exists() {
        for sub in ["Themes", "wallpapers", "Diagnostics"] {
            let s = src_data_dir.join(sub);
            if s.is_dir() {
                copy_dir_recursive_if_missing(&s, &dst_data_dir.join(sub));
            }
        }
        for fname in ["pill_cache.ini", "config.ini.bak"] {
            let s = src_data_dir.join(fname);
            let d = dst_data_dir.join(fname);
            if s.is_file() && !d.exists() {
                let _ = fs::copy(&s, &d);
            }
        }
    }

    let src_str = src_data_dir.to_string_lossy().to_string();
    let dst_str = dst_data_dir.to_string_lossy().to_string();
    let mut portable_cfg = cfg.clone();
    if !src_str.is_empty() && !src_str.eq_ignore_ascii_case(&dst_str) {
        if portable_cfg
            .wallpaper_path
            .to_ascii_lowercase()
            .starts_with(&src_str.to_ascii_lowercase())
        {
            let tail = &portable_cfg.wallpaper_path[src_str.len()..];
            portable_cfg.wallpaper_path = format!("{}{}", dst_str, tail);
        }
    }
    let ini = serialize_config_ini(&portable_cfg);
    let dst_ini = dst_data_dir.join("config.ini");
    write_atomic_synced(&dst_ini, &ini)
        .map_err(|e| format!("Failed writing portable config.ini: {}", e))?;
    let _ = write_atomic_synced(&dst_data_dir.join("config.ini.bak"), &ini);
    Ok(dst_data_dir)
}

trait PathEqIgnoreCase {
    fn eq_ignore_ascii_case_path(&self, other: &Path) -> bool;
}

impl PathEqIgnoreCase for Path {
    fn eq_ignore_ascii_case_path(&self, other: &Path) -> bool {
        self.to_string_lossy()
            .eq_ignore_ascii_case(&other.to_string_lossy())
    }
}

pub fn convert_current_setup_to_portable(cfg: &AppConfig) -> Result<PathBuf, String> {
    let exe = self_exe_path();
    let Some(exe_dir) = exe.parent() else {
        return Err("Unable to determine AutumnDeck executable directory.".to_string());
    };
    let appdata = std::env::var("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."));
    let src_dir = if is_portable_mode() {
        app_data_dir()
    } else {
        appdata.join("AutumnDeck")
    };
    convert_to_portable_in(exe_dir, &src_dir, cfg)
}

fn copy_dir_recursive_if_missing(src: &Path, dst: &Path) {
    if fs::create_dir_all(dst).is_err() {
        return;
    }
    let Ok(entries) = fs::read_dir(src) else {
        return;
    };
    for entry in entries.flatten() {
        let sp = entry.path();
        let dp = dst.join(entry.file_name());
        if sp.is_dir() {
            copy_dir_recursive_if_missing(&sp, &dp);
        } else if !dp.exists() {
            let _ = fs::copy(&sp, &dp);
        }
    }
}

/// One-time seamless migration from legacy `%APPDATA%\AutumnTaskbar` to `%APPDATA%\AutumnDeck`.
/// Preserves the legacy directory untouched as a backup while updating copied cached paths.
pub fn migrate_legacy_appdata_if_needed() {
    if is_portable_mode() {
        return;
    }
    let new_dir = app_data_dir();
    if new_dir.join("config.ini").exists() {
        return;
    }
    let legacy_dir = new_dir.with_file_name("AutumnTaskbar");
    if !legacy_dir.join("config.ini").exists() {
        return;
    }
    copy_dir_recursive_if_missing(&legacy_dir, &new_dir);
    let legacy_str = legacy_dir.to_string_lossy().to_string();
    let new_str = new_dir.to_string_lossy().to_string();
    for fname in ["config.ini", "config.ini.bak"] {
        let p = new_dir.join(fname);
        if let Ok(raw) = fs::read_to_string(&p) {
            let updated = raw.replace(&legacy_str, &new_str);
            if updated != raw {
                let _ = write_atomic_synced(&p, &updated);
            }
        }
    }
}

pub fn wallpapers_cache_dir() -> PathBuf {
    app_data_dir().join("wallpapers")
}

pub fn pill_cache_path() -> PathBuf {
    app_data_dir().join("pill_cache.ini")
}

pub fn load_pill_cache(bar_rect: RECT, pill_mode: PillMode, u: f32) -> Option<Vec<RECT>> {
    let text = fs::read_to_string(pill_cache_path()).ok()?;
    let dpi_key = (u * 100.0).round() as i32;
    let expected = format!(
        "{},{},{},{}|{}|{}",
        bar_rect.left,
        bar_rect.top,
        bar_rect.right,
        bar_rect.bottom,
        pill_mode.as_str(),
        dpi_key
    );
    let mut lines = text.lines();
    if lines.next()?.trim() != expected {
        return None;
    }
    let mut out = Vec::new();
    for part in lines.next()?.split(';') {
        let nums: Vec<i32> = part
            .split(',')
            .filter_map(|n| n.trim().parse::<i32>().ok())
            .collect();
        if nums.len() == 4 && nums[2] > nums[0] && nums[3] > nums[1] {
            out.push(RECT {
                left: nums[0],
                top: nums[1],
                right: nums[2],
                bottom: nums[3],
            });
        }
    }
    if out.is_empty() { None } else { Some(out) }
}

pub fn save_pill_cache(bar_rect: RECT, pill_mode: PillMode, u: f32, pills: &[RECT]) {
    if pills.is_empty() {
        return;
    }
    let dir = app_data_dir();
    let _ = fs::create_dir_all(&dir);
    let dpi_key = (u * 100.0).round() as i32;
    let header = format!(
        "{},{},{},{}|{}|{}",
        bar_rect.left,
        bar_rect.top,
        bar_rect.right,
        bar_rect.bottom,
        pill_mode.as_str(),
        dpi_key
    );
    let rects: Vec<String> = pills
        .iter()
        .map(|r| format!("{},{},{},{}", r.left, r.top, r.right, r.bottom))
        .collect();
    let _ = fs::write(pill_cache_path(), format!("{}\n{}\n", header, rects.join(";")));
}

fn read_taskband_favorites_pins() -> Vec<AppShortcut> {
    const PKEY_AUMID: PROPERTYKEY = PROPERTYKEY {
        fmtid: windows::core::GUID::from_u128(0x9F4C2855_9F79_4B39_A8D0_E1D42DE1D5F3),
        pid: 5,
    };
    let mut out = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    unsafe {
        let mut hk = HKEY::default();
        if RegOpenKeyExW(
            HKEY_CURRENT_USER,
            w!(r"Software\Microsoft\Windows\CurrentVersion\Explorer\Taskband"),
            None,
            KEY_READ,
            &mut hk,
        )
        .is_err()
        {
            return out;
        }
        let mut val_type = REG_VALUE_TYPE::default();
        let mut cb = 0u32;
        if RegQueryValueExW(hk, w!("Favorites"), None, Some(&mut val_type), None, Some(&mut cb))
            .is_err()
            || val_type != REG_BINARY
            || cb < 8
            || cb > 512 * 1024
        {
            let _ = RegCloseKey(hk);
            return out;
        }
        let mut raw = vec![0u8; cb as usize];
        let ok = RegQueryValueExW(
            hk,
            w!("Favorites"),
            None,
            Some(&mut val_type),
            Some(raw.as_mut_ptr()),
            Some(&mut cb),
        )
        .is_ok();
        let _ = RegCloseKey(hk);
        if !ok || cb < 8 {
            return out;
        }
        raw.truncate(cb as usize);

        let mut off = 0usize;
        while off + 5 <= raw.len() && out.len() < 16 {
            let tag = raw[off];
            let len = i32::from_le_bytes([raw[off + 1], raw[off + 2], raw[off + 3], raw[off + 4]]);
            if tag == 0xFF || len <= 4 || off + 5 + (len as usize) > raw.len() {
                break;
            }
            let len_u = len as usize;
            let mut aligned = vec![0u32; (len_u + 3) / 4];
            std::ptr::copy_nonoverlapping(
                raw[off + 5..off + 5 + len_u].as_ptr(),
                aligned.as_mut_ptr() as *mut u8,
                len_u,
            );
            let pidl = aligned.as_ptr() as *const ITEMIDLIST;

            let disp = SHGetNameFromIDList(pidl, SIGDN_NORMALDISPLAY)
                .ok()
                .map(|p| {
                    let s = p.to_string().unwrap_or_default();
                    CoTaskMemFree(Some(p.0 as *const c_void));
                    s
                })
                .unwrap_or_default();
            let pars = SHGetNameFromIDList(pidl, SIGDN_DESKTOPABSOLUTEPARSING)
                .ok()
                .map(|p| {
                    let s = p.to_string().unwrap_or_default();
                    CoTaskMemFree(Some(p.0 as *const c_void));
                    s
                })
                .unwrap_or_default();
            let rel = SHGetNameFromIDList(pidl, SIGDN_PARENTRELATIVEPARSING)
                .ok()
                .map(|p| {
                    let s = p.to_string().unwrap_or_default();
                    CoTaskMemFree(Some(p.0 as *const c_void));
                    s
                })
                .unwrap_or_default();

            let mut aumid = String::new();
            let mut ps_raw: *mut c_void = std::ptr::null_mut();
            if SHGetPropertyStoreFromIDList(pidl, GPS_DEFAULT, &IPropertyStore::IID, &mut ps_raw)
                .is_ok()
                && !ps_raw.is_null()
            {
                let ps = IPropertyStore::from_raw(ps_raw);
                if let Ok(mut pv) = ps.GetValue(&PKEY_AUMID) {
                    if let Ok(pw) = PropVariantToStringAlloc(&pv) {
                        aumid = pw.to_string().unwrap_or_default();
                        CoTaskMemFree(Some(pw.0 as *const c_void));
                    }
                    let _ = PropVariantClear(&mut pv);
                }
            }

            let target_path = if pars.to_ascii_lowercase().ends_with(".lnk") && Path::new(&pars).exists()
            {
                pars
            } else if !aumid.trim().is_empty() {
                format!("shell:AppsFolder\\{}", aumid.trim())
            } else if rel.contains('!') {
                format!("shell:AppsFolder\\{}", rel.trim())
            } else if !pars.trim().is_empty() {
                pars
            } else {
                String::new()
            };

            let clean_name = disp.trim().to_string();
            let key = target_path.to_ascii_lowercase();
            if !target_path.is_empty() && !clean_name.is_empty() && !seen.contains(&key) {
                seen.push(key);
                out.push(AppShortcut {
                    name: clean_name,
                    path: target_path,
                    offset_x: 0,
                    offset_y: 0,
                });
            }
            off += 5 + len_u;
        }
    }
    out
}

pub fn read_windows_taskbar_pins() -> Vec<AppShortcut> {
    let fav_pins = read_taskband_favorites_pins();
    if !fav_pins.is_empty() {
        return fav_pins;
    }
    let mut out: Vec<AppShortcut> = Vec::new();
    let appdata = std::env::var("APPDATA").unwrap_or_default();
    if !appdata.is_empty() {
        let pinned_dir = PathBuf::from(&appdata)
            .join("Microsoft")
            .join("Internet Explorer")
            .join("Quick Launch")
            .join("User Pinned")
            .join("TaskBar");
        if let Ok(entries) = fs::read_dir(&pinned_dir) {
            let mut lnks: Vec<(String, String)> = entries
                .filter_map(|e| e.ok())
                .map(|e| e.path())
                .filter(|p| {
                    p.extension()
                        .and_then(|s| s.to_str())
                        .map(|s| s.eq_ignore_ascii_case("lnk"))
                        .unwrap_or(false)
                })
                .filter_map(|p| {
                    let name = p.file_stem()?.to_string_lossy().trim().to_string();
                    let full = p.to_string_lossy().to_string();
                    if name.is_empty() { None } else { Some((name, full)) }
                })
                .collect();
            lnks.sort_by(|a, b| a.0.to_ascii_lowercase().cmp(&b.0.to_ascii_lowercase()));
            for (name, path) in lnks.into_iter().take(MAX_PINNED_APPS) {
                out.push(AppShortcut {
                    name,
                    path,
                    offset_x: 0,
                    offset_y: 0,
                });
            }
        }
    }
    out
}

pub fn default_dock_pinned() -> Vec<AppShortcut> {
    let mut out = read_windows_taskbar_pins();
    if out.is_empty() {
        out.push(AppShortcut {
            name: "File Explorer".to_string(),
            path: "explorer.exe".to_string(),
            offset_x: 0,
            offset_y: 0,
        });
        out.push(AppShortcut {
            name: "Microsoft Edge".to_string(),
            path: "msedge.exe".to_string(),
            offset_x: 0,
            offset_y: 0,
        });
    }
    out
}

pub fn config_path() -> PathBuf {
    app_data_dir().join("config.ini")
}

#[cfg(test)]
pub fn parse_config_ini(text: &str) -> Option<AppConfig> {
    let is_v5 = text.contains("[AutumnStudioV5]");
    let is_v4 = text.contains("[AutumnStudioV4]");
    if !is_v5 && !is_v4 {
        return None;
    }
    let (cfg, _, _, _) = parse_config_body(text, is_v5);
    Some(cfg)
}

fn path_to_wide(p: &Path) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    p.as_os_str().encode_wide().chain(std::iter::once(0)).collect()
}

fn config_sibling_path(path: &Path, suffix: &str) -> PathBuf {
    let mut s = path.as_os_str().to_os_string();
    s.push(suffix);
    PathBuf::from(s)
}

fn has_valid_config_header(text: &str) -> bool {
    text.contains("[AutumnStudioV5]") || text.contains("[AutumnStudioV4]")
}

fn write_atomic_synced(path: &Path, contents: &str) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = config_sibling_path(path, ".tmp");
    {
        let mut f = fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&tmp)?;
        f.write_all(contents.as_bytes())?;
        f.sync_all()?;
    }
    let src_w = path_to_wide(&tmp);
    let dst_w = path_to_wide(path);
    let mut last_err = None;
    for delay_ms in [0u64, 5, 15] {
        if delay_ms > 0 {
            std::thread::sleep(std::time::Duration::from_millis(delay_ms));
        }
        let moved = unsafe {
            MoveFileExW(
                PCWSTR(src_w.as_ptr()),
                PCWSTR(dst_w.as_ptr()),
                MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
            )
        };
        if moved.is_ok() {
            return Ok(());
        }
        last_err = moved.err();
    }
    let res = fs::rename(&tmp, path);
    if res.is_err() {
        let _ = fs::remove_file(&tmp);
        if let Some(e) = last_err {
            return Err(std::io::Error::new(std::io::ErrorKind::Other, format!("{e:?}")));
        }
    }
    res
}

fn read_config_text_with_retry(path: &Path) -> (Option<String>, bool) {
    let existed = path.exists();
    for delay_ms in [0u64, 20, 50, 100] {
        if delay_ms > 0 {
            std::thread::sleep(std::time::Duration::from_millis(delay_ms));
        }
        match fs::read_to_string(path) {
            Ok(text) => {
                if has_valid_config_header(&text) {
                    return (Some(text), false);
                }
                return (None, false);
            }
            Err(_) if existed => continue,
            Err(_) => return (None, false),
        }
    }
    (None, existed)
}

pub fn load_config_from_path(path: &Path) -> AppConfig {
    let bak = config_sibling_path(path, ".bak");
    let bak_exists = bak.exists();
    let (primary, primary_locked) = read_config_text_with_retry(path);
    let (text, recovered_from_bak) = if let Some(t) = primary {
        (t, false)
    } else if let (Some(b), _) = read_config_text_with_retry(&bak) {
        (b, true)
    } else {
        let mut cfg = AppConfig::default();
        let _ = resolve_or_repair_wallpaper_path(&mut cfg);
        // Never overwrite an existing user config.ini if Windows logon AV/indexer merely held a lock.
        if !primary_locked {
            save_config_to_path(&cfg, path);
        }
        return cfg;
    };

    let is_v5 = text.contains("[AutumnStudioV5]");
    let (mut cfg, app_count, saw_per_widget_match, saw_dock_sync_flag) =
        parse_config_body(&text, is_v5);
    let mut pins_changed = false;
    if !saw_dock_sync_flag || cfg.dock_sync_win_pins {
        let win_pins = read_windows_taskbar_pins();
        if !win_pins.is_empty() {
            pins_changed = win_pins.len() != cfg.mac_dock_pinned.len()
                || win_pins.iter().zip(&cfg.mac_dock_pinned).any(|(a, b)| {
                    a.name != b.name || !a.path.eq_ignore_ascii_case(&b.path)
                });
            cfg.mac_dock_pinned = win_pins;
            cfg.dock_sync_win_pins = true;
        }
    }

    cfg.mac_dock_enabled = cfg.tb_theme.uses_virtual_dock();
    let repaired_wp = resolve_or_repair_wallpaper_path(&mut cfg);
    if recovered_from_bak
        || !bak_exists
        || !is_v5
        || app_count.is_none()
        || repaired_wp
        || !saw_per_widget_match
        || !saw_dock_sync_flag
        || pins_changed
    {
        save_config_to_path(&cfg, path);
    }
    cfg
}

pub fn load_config() -> AppConfig {
    load_config_from_path(&config_path())
}

/// Exports the active `AppConfig` to a portable `.autumn` / `.ini` file atomically.
#[allow(dead_code)]
pub fn export_setup_to_path(cfg: &AppConfig, dest: &Path) -> Result<(), String> {
    let mut portable = cfg.clone();
    portable.onboarding_done = true;
    let body = serialize_config_ini(&portable);
    write_atomic_synced(dest, &body).map_err(|e| format!("Failed to write setup: {e}"))
}

/// Saves a safety backup (`config.pre_import.bak`) of the live config before importing a setup.
#[allow(dead_code)]
pub fn backup_config_before_import(current: &AppConfig) {
    let bak = config_sibling_path(&config_path(), ".pre_import.bak");
    let _ = write_atomic_synced(&bak, &serialize_config_ini(current));
}

/// Imports and validates a `.autumn` / `.ini` setup file. Gracefully preserves the local
/// machine's `wallpaper_path` if the imported setup references a file not present on this PC.
#[allow(dead_code)]
pub fn import_setup_from_path(src: &Path, current: &AppConfig) -> Result<AppConfig, String> {
    let text = fs::read_to_string(src).map_err(|e| format!("Could not read setup file: {e}"))?;
    if !has_valid_config_header(&text) {
        return Err("Not a valid AutumnDeck setup file (missing [AutumnStudioV5] header).".to_string());
    }
    let is_v5 = text.contains("[AutumnStudioV5]");
    let (mut imported, _, _, saw_dock_sync_flag) = parse_config_body(&text, is_v5);
    imported.onboarding_done = true;
    imported.mac_dock_enabled = imported.tb_theme.uses_virtual_dock();

    if !saw_dock_sync_flag || imported.dock_sync_win_pins {
        let win_pins = read_windows_taskbar_pins();
        if !win_pins.is_empty() {
            imported.mac_dock_pinned = win_pins;
            imported.dock_sync_win_pins = true;
        }
    }

    // Preserve local wallpaper file if imported file path is empty or missing on this PC
    let imp_wp = imported.wallpaper_path.trim();
    if imp_wp.is_empty() || !Path::new(imp_wp).is_file() {
        if !current.wallpaper_path.is_empty() && Path::new(&current.wallpaper_path).is_file() {
            imported.wallpaper_path = current.wallpaper_path.clone();
        } else {
            let _ = resolve_or_repair_wallpaper_path(&mut imported);
        }
    }
    Ok(imported)
}

/// Opens a native Windows Save dialog for exporting a `.autumn` setup file.
#[allow(dead_code)]
pub fn pick_export_setup_file(owner: HWND) -> Option<PathBuf> {
    let mut buf = vec![0u16; 520];
    let def_name = to_wide("AutumnDeck-Setup.autumn");
    let copy_len = def_name.len().min(buf.len());
    buf[..copy_len].copy_from_slice(&def_name[..copy_len]);
    let filter = to_wide(
        "AutumnDeck Setup (*.autumn;*.ini)\0*.autumn;*.ini\0All Files (*.*)\0*.*\0",
    );
    let title = to_wide("Export AutumnDeck Setup");
    let def_ext = to_wide("autumn");
    let mut ofn = OPENFILENAMEW {
        lStructSize: size_of::<OPENFILENAMEW>() as u32,
        hwndOwner: owner,
        lpstrFilter: PCWSTR(filter.as_ptr()),
        lpstrFile: PWSTR(buf.as_mut_ptr()),
        nMaxFile: buf.len() as u32,
        lpstrTitle: PCWSTR(title.as_ptr()),
        lpstrDefExt: PCWSTR(def_ext.as_ptr()),
        Flags: OFN_OVERWRITEPROMPT | OFN_PATHMUSTEXIST | OFN_NOCHANGEDIR,
        ..Default::default()
    };
    let ok = unsafe { GetSaveFileNameW(&mut ofn).as_bool() };
    if ok {
        let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        let s = String::from_utf16_lossy(&buf[..end]).trim().to_string();
        if !s.is_empty() {
            return Some(PathBuf::from(s));
        }
    }
    None
}

/// Opens a native Windows Open dialog for importing a `.autumn` / `.ini` setup file.
#[allow(dead_code)]
pub fn pick_import_setup_file(owner: HWND) -> Option<PathBuf> {
    let mut buf = vec![0u16; 520];
    let filter = to_wide(
        "AutumnDeck Setup (*.autumn;*.ini)\0*.autumn;*.ini\0All Files (*.*)\0*.*\0",
    );
    let title = to_wide("Import AutumnDeck Setup");
    let mut ofn = OPENFILENAMEW {
        lStructSize: size_of::<OPENFILENAMEW>() as u32,
        hwndOwner: owner,
        lpstrFilter: PCWSTR(filter.as_ptr()),
        lpstrFile: PWSTR(buf.as_mut_ptr()),
        nMaxFile: buf.len() as u32,
        lpstrTitle: PCWSTR(title.as_ptr()),
        Flags: OFN_FILEMUSTEXIST | OFN_NOCHANGEDIR,
        ..Default::default()
    };
    let ok = unsafe { GetOpenFileNameW(&mut ofn).as_bool() };
    if ok {
        let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        let s = String::from_utf16_lossy(&buf[..end]).trim().to_string();
        if !s.is_empty() {
            return Some(PathBuf::from(s));
        }
    }
    None
}

/// Canonical URL for optional one-time developer tips on Ko-fi.
pub const KOFI_SUPPORT_URL: &str = "https://ko-fi.com/autumndeck";

fn open_shell_target(target_wide: &[u16]) {
    unsafe {
        let _ = ShellExecuteW(
            None,
            w!("open"),
            PCWSTR(target_wide.as_ptr()),
            PCWSTR::null(),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        );
    }
}

/// Opens `%APPDATA%\AutumnDeck` in Windows File Explorer.
pub fn open_app_data_folder() {
    let dir = app_data_dir();
    let _ = fs::create_dir_all(&dir);
    let wdir = path_to_wide(&dir);
    open_shell_target(&wdir);
}

/// Opens the developer's one-time Ko-fi tip page in the default browser.
pub fn open_kofi_support_page() {
    #[cfg(not(test))]
    {
        let wurl = to_wide(KOFI_SUPPORT_URL);
        open_shell_target(&wurl);
    }
}

fn parse_config_body(text: &str, is_v5: bool) -> (AppConfig, Option<usize>, bool, bool) {
    let mut cfg = AppConfig::default();
    let mut app_count: Option<usize> = None;
    let mut dynamic_slots: Vec<(String, String, i32, i32)> =
        vec![(String::new(), String::new(), 0, 0); 12];
    let mut dock_pin_count: Option<usize> = None;
    let mut dock_pin_slots: Vec<(String, String)> =
        vec![(String::new(), String::new()); MAX_PINNED_APPS];
    let mut legacy_slots: [(String, String); 3] = Default::default();
    let mut saw_legacy = false;
    let mut saw_per_widget_match = false;
    let mut saw_dock_sync_flag = false;
    let mut saw_onboarding = false;
    let mut saw_anim_fps = false;
    let mut saw_viz_color_mode = false;
    let mut saw_dock_bar_enabled = false;
    let mut canvas_schema: u8 = 5;
    let mut legacy_dynamic_accent: Option<bool> = None;
    let mut pending_ext_elems: Vec<(usize, usize, ClockElemTransform)> = Vec::new();
    let mut pending_ext_fonts: Vec<(usize, usize, String)> = Vec::new();
    let mut pending_ext_horology: Vec<(usize, crate::horology_pbr::HorologyPartConfig)> = Vec::new();

    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') || line.starts_with('[') {
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            let key = k.trim();
            let val = v.trim();
            match key {
                "active_suite" => cfg.active_suite = val.to_string(),
                "mode" => cfg.mode = TaskbarMode::from_str(val),
                "pill_mode" => cfg.pill_mode = PillMode::from_str(val),
                "surface_mode" => cfg.surface_mode = SurfaceMode::from_str(val),
                "tint_alpha" => {
                    if let Ok(a) = val.parse::<u8>() {
                        // Migrate legacy v4 near-opaque default down; allow full 0..=255 range in v5
                        cfg.tint_alpha = if !is_v5 && a > 235 { 105 } else { a };
                    }
                }
                "amber_border" => cfg.amber_border = parse_bool(val),
                "corner_radius" => {
                    if let Ok(r) = val.parse::<i32>() {
                        cfg.corner_radius = r.clamp(0, 32);
                    }
                }
                "tb_theme" => cfg.tb_theme = TaskbarTheme::from_str(val),
                "tb_viz_style" => cfg.tb_viz_style = TaskbarVizStyle::from_str(val),
                "tb_viz_opacity" => {
                    if let Ok(v) = val.parse::<u8>() {
                        cfg.tb_viz_opacity = v.clamp(20, 100);
                    }
                }
                "tb_viz_mirror" => cfg.tb_viz_mirror = parse_bool(val),
                "tb_viz_color_mode" => {
                    cfg.tb_viz_color_mode = VizColorMode::from_str(val);
                    saw_viz_color_mode = true;
                }
                "tb_viz_accent_rgb" => {
                    if let Some(c) = parse_hex_rgb(val) {
                        cfg.tb_viz_accent_rgb = c;
                    }
                }
                "tb_hover_react" => cfg.tb_hover_react = parse_bool(val),
                "tb_ambient_anim" => cfg.tb_ambient_anim = parse_bool(val),
                "tb_show_backdrop" => cfg.tb_show_backdrop = parse_bool(val),
                "tb_show_rim" => cfg.tb_show_rim = parse_bool(val),
                "tb_show_fx" => cfg.tb_show_fx = parse_bool(val),
                "anim_fps" => {
                    if let Ok(v) = val.parse::<u32>() {
                        cfg.set_anim_fps(v);
                        saw_anim_fps = true;
                    }
                }
                "performance_mode" => cfg.performance_mode = PerformanceMode::from_str(val),
                "viz_eco_fps" => {
                    if !saw_anim_fps {
                        cfg.viz_eco_fps = parse_bool(val);
                    }
                }
                "dt_hover_fx" => cfg.dt_hover_fx = TaskbarVizStyle::from_str(val),
                "apps_hover_fx" => cfg.apps_hover_fx = TaskbarVizStyle::from_str(val),
                "vitals_hover_fx" => cfg.vitals_hover_fx = TaskbarVizStyle::from_str(val),
                "music_hover_fx" => cfg.music_hover_fx = TaskbarVizStyle::from_str(val),
                "cc_hover_fx" => cfg.cc_hover_fx = TaskbarVizStyle::from_str(val),
                "widgets_enabled" => cfg.widgets_enabled = parse_bool(val),
                "widgets_locked" => cfg.widgets_locked = parse_bool(val),
                "widgets_24h" => cfg.widgets_24h = parse_bool(val),
                "widgets_hover_react" => cfg.widgets_hover_react = parse_bool(val),
                "widgets_live_material" => cfg.widgets_live_material = parse_bool(val),
                "show_datetime" => cfg.show_datetime = parse_bool(val),
                "show_apps" => cfg.show_apps = parse_bool(val),
                "show_vitals" => cfg.show_vitals = parse_bool(val),
                "show_music" => cfg.show_music = parse_bool(val),
                "show_cc" => cfg.show_cc = parse_bool(val),

                "dt_x" => if let Ok(v) = val.parse::<i32>() { cfg.dt_x = v.clamp(-200, 1220); },
                "dt_y" => if let Ok(v) = val.parse::<i32>() { cfg.dt_y = v.clamp(-60, 740); },
                "apps_x" => if let Ok(v) = val.parse::<i32>() { cfg.apps_x = v.clamp(-200, 1220); },
                "apps_y" => if let Ok(v) = val.parse::<i32>() { cfg.apps_y = v.clamp(-60, 740); },
                "vitals_x" => if let Ok(v) = val.parse::<i32>() { cfg.vitals_x = v.clamp(-200, 1220); },
                "vitals_y" => if let Ok(v) = val.parse::<i32>() { cfg.vitals_y = v.clamp(-60, 760); },
                "music_x" => if let Ok(v) = val.parse::<i32>() { cfg.music_x = v.clamp(-200, 1220); },
                "music_y" => if let Ok(v) = val.parse::<i32>() { cfg.music_y = v.clamp(-60, 760); },
                "cc_x" => if let Ok(v) = val.parse::<i32>() { cfg.cc_x = v.clamp(-200, 1220); },
                "cc_y" => if let Ok(v) = val.parse::<i32>() { cfg.cc_y = v.clamp(-60, 740); },

                "dt_scale_pct" => {
                    if let Ok(v) = val.parse::<u8>() {
                        cfg.dt_scale_pct = v.clamp(60, 160);
                    }
                }
                "dt_show_plate" => cfg.dt_show_plate = parse_bool(val),
                "dt_show_clock" => cfg.dt_show_clock = parse_bool(val),
                "dt_glass_alpha" => {
                    if let Ok(v) = val.parse::<u8>() {
                        cfg.dt_glass_alpha = v;
                    }
                }
                "dt_rim_alpha" => {
                    if let Ok(v) = val.parse::<u8>() {
                        cfg.dt_rim_alpha = v;
                    }
                }
                "dt_show_spine" => cfg.dt_show_spine = parse_bool(val),
                "dt_show_orb" => cfg.dt_show_orb = parse_bool(val),
                "dt_style" => cfg.dt_style = ClockStyle::from_str(val),
                "dt_align" => cfg.dt_align = ClockAlign::from_str(val),
                "dt_show_seconds" => cfg.dt_show_seconds = parse_bool(val),
                "dt_show_ampm" => cfg.dt_show_ampm = parse_bool(val),
                "dt_show_weekday" => cfg.dt_show_weekday = parse_bool(val),
                "dt_show_date" => cfg.dt_show_date = parse_bool(val),
                "dt_corner_radius" => if let Ok(v) = val.parse::<u8>() { cfg.dt_corner_radius = v.min(36); },
                "dt_letter_spacing" => if let Ok(v) = val.parse::<i8>() { cfg.dt_letter_spacing = v.clamp(-2, 8); },
                "dt_font_day" => cfg.dt_font_day = val.to_string(),
                "dt_font_script" => cfg.dt_font_script = val.to_string(),
                "dt_font_clock" => cfg.dt_font_clock = val.to_string(),
                "dt_font_sub" => cfg.dt_font_sub = val.to_string(),
                "dt_use_custom_color" => cfg.dt_use_custom_color = parse_bool(val),
                "dt_accent_rgb" => if let Some(c) = parse_hex_rgb(val) { cfg.dt_accent_rgb = c; },
                "dt_digits_rgb" => if let Some(c) = parse_hex_rgb(val) { cfg.dt_digits_rgb = c; },
                "dt_script_rgb" => if let Some(c) = parse_hex_rgb(val) { cfg.dt_script_rgb = c; },
                "dt_bg_rgb" => if let Some(c) = parse_hex_rgb(val) { cfg.dt_bg_rgb = c; },
                "dynamic_accent_enabled" => legacy_dynamic_accent = Some(parse_bool(val)),
                "tb_match_wallpaper" => {
                    cfg.tb_match_wallpaper = parse_bool(val);
                    saw_per_widget_match = true;
                }
                "dt_match_wallpaper" => {
                    cfg.dt_match_wallpaper = parse_bool(val);
                    saw_per_widget_match = true;
                }
                "apps_match_wallpaper" => {
                    cfg.apps_match_wallpaper = parse_bool(val);
                    saw_per_widget_match = true;
                }
                "vitals_match_wallpaper" => {
                    cfg.vitals_match_wallpaper = parse_bool(val);
                    saw_per_widget_match = true;
                }
                "music_match_wallpaper" => {
                    cfg.music_match_wallpaper = parse_bool(val);
                    saw_per_widget_match = true;
                }
                "cc_match_wallpaper" => {
                    cfg.cc_match_wallpaper = parse_bool(val);
                    saw_per_widget_match = true;
                }
                "apps_scale_pct" => {
                    if let Ok(v) = val.parse::<u8>() {
                        cfg.apps_scale_pct = v.clamp(70, 150);
                    }
                }
                "apps_glass_alpha" => {
                    if let Ok(v) = val.parse::<u8>() {
                        cfg.apps_glass_alpha = v;
                    }
                }
                "apps_rim_alpha" => {
                    if let Ok(v) = val.parse::<u8>() {
                        cfg.apps_rim_alpha = v;
                    }
                }
                "apps_show_add_tile" => cfg.apps_show_add_tile = parse_bool(val),
                "apps_match_theme" => cfg.apps_match_theme = parse_bool(val),
                "apps_individual_move" => cfg.apps_individual_move = parse_bool(val),
                "apps_cols" => if let Ok(v) = val.parse::<u8>() { cfg.apps_cols = v.clamp(1, 12); },
                "apps_tile_shape" => {
                    if let Ok(v) = val.parse::<u8>() {
                        cfg.apps_tile_shape = v.min(13);
                    } else if let Some(ht) = crate::horology_pbr::HorologyTheme::from_str(val) {
                        cfg.apps_tile_shape = 4 + ht.idx() as u8;
                    }
                }
                "apps_corner_radius" => if let Ok(v) = val.parse::<u8>() { cfg.apps_corner_radius = v.min(32); },
                "apps_show_plate" => cfg.apps_show_plate = parse_bool(val),
                "apps_show_tiles" => cfg.apps_show_tiles = parse_bool(val),
                "apps_show_labels" => cfg.apps_show_labels = parse_bool(val),
                "apps_show_dots" => cfg.apps_show_dots = parse_bool(val),
                "apps_font_label" => cfg.apps_font_label = val.to_string(),
                "apps_font_scale_pct" => if let Ok(v) = val.parse::<u8>() { cfg.apps_font_scale_pct = v.clamp(75, 135); },
                "apps_use_custom_color" => cfg.apps_use_custom_color = parse_bool(val),
                "apps_accent_rgb" => if let Some(c) = parse_hex_rgb(val) { cfg.apps_accent_rgb = c; },
                "apps_text_rgb" => if let Some(c) = parse_hex_rgb(val) { cfg.apps_text_rgb = c; },
                "apps_bg_rgb" => if let Some(c) = parse_hex_rgb(val) { cfg.apps_bg_rgb = c; },
                "app_count" => {
                    if let Ok(n) = val.parse::<usize>() {
                        app_count = Some(n.min(12));
                    }
                }
                "app1_name" if !val.is_empty() => { legacy_slots[0].0 = val.to_string(); saw_legacy = true; }
                "app1_path" if !val.is_empty() => { legacy_slots[0].1 = val.to_string(); saw_legacy = true; }
                "app2_name" if !val.is_empty() => { legacy_slots[1].0 = val.to_string(); saw_legacy = true; }
                "app2_path" if !val.is_empty() => { legacy_slots[1].1 = val.to_string(); saw_legacy = true; }
                "app3_name" if !val.is_empty() => { legacy_slots[2].0 = val.to_string(); saw_legacy = true; }
                "app3_path" if !val.is_empty() => { legacy_slots[2].1 = val.to_string(); saw_legacy = true; }
                "vitals_scale_pct" => {
                    if let Ok(v) = val.parse::<u8>() {
                        cfg.vitals_scale_pct = v.clamp(70, 150);
                    }
                }
                "vitals_glass_alpha" => {
                    if let Ok(v) = val.parse::<u8>() {
                        cfg.vitals_glass_alpha = v;
                    }
                }
                "vitals_rim_alpha" => {
                    if let Ok(v) = val.parse::<u8>() {
                        cfg.vitals_rim_alpha = v;
                    }
                }
                "vitals_show_cpu" => cfg.vitals_show_cpu = parse_bool(val),
                "vitals_show_ram" => cfg.vitals_show_ram = parse_bool(val),
                "vitals_show_power" => cfg.vitals_show_power = parse_bool(val),
                "vitals_show_buttons" => cfg.vitals_show_buttons = parse_bool(val),
                "vitals_style" => {
                    if let Ok(v) = val.parse::<u8>() {
                        cfg.vitals_style = v.min(14);
                    } else if let Some(ht) = crate::horology_pbr::HorologyTheme::from_str(val) {
                        cfg.vitals_style = 5 + ht.idx() as u8;
                    }
                }
                "vitals_corner_radius" => if let Ok(v) = val.parse::<u8>() { cfg.vitals_corner_radius = v.min(36); },
                "vitals_show_plate" => cfg.vitals_show_plate = parse_bool(val),
                "vitals_show_dividers" => cfg.vitals_show_dividers = parse_bool(val),
                "vitals_show_bars" => cfg.vitals_show_bars = parse_bool(val),
                "vitals_font_val" => cfg.vitals_font_val = val.to_string(),
                "vitals_font_lbl" => cfg.vitals_font_lbl = val.to_string(),
                "vitals_font_scale_pct" => if let Ok(v) = val.parse::<u8>() { cfg.vitals_font_scale_pct = v.clamp(75, 135); },
                "vitals_use_custom_color" => cfg.vitals_use_custom_color = parse_bool(val),
                "vitals_accent_rgb" => if let Some(c) = parse_hex_rgb(val) { cfg.vitals_accent_rgb = c; },
                "vitals_text_rgb" => if let Some(c) = parse_hex_rgb(val) { cfg.vitals_text_rgb = c; },
                "vitals_bg_rgb" => if let Some(c) = parse_hex_rgb(val) { cfg.vitals_bg_rgb = c; },
                "music_style" => cfg.music_style = MusicStyle::from_str(val),
                "music_scale_pct" => if let Ok(v) = val.parse::<u8>() { cfg.music_scale_pct = v.clamp(70, 150); },
                "music_glass_alpha" => if let Ok(v) = val.parse::<u8>() { cfg.music_glass_alpha = v; },
                "music_rim_alpha" => if let Ok(v) = val.parse::<u8>() { cfg.music_rim_alpha = v; },
                "music_show_viz" => cfg.music_show_viz = parse_bool(val),
                "music_corner_radius" => if let Ok(v) = val.parse::<u8>() { cfg.music_corner_radius = v.min(48); },
                "music_show_plate" => cfg.music_show_plate = parse_bool(val),
                "music_show_cover" => cfg.music_show_cover = parse_bool(val),
                "music_show_title" => cfg.music_show_title = parse_bool(val),
                "music_show_artist" => cfg.music_show_artist = parse_bool(val),
                "music_show_controls" => cfg.music_show_controls = parse_bool(val),
                "music_show_badge" => cfg.music_show_badge = parse_bool(val),
                "music_font_title" => cfg.music_font_title = val.to_string(),
                "music_font_artist" => cfg.music_font_artist = val.to_string(),
                "music_font_scale_pct" => if let Ok(v) = val.parse::<u8>() { cfg.music_font_scale_pct = v.clamp(75, 135); },
                "music_use_custom_color" => cfg.music_use_custom_color = parse_bool(val),
                "music_accent_rgb" => if let Some(c) = parse_hex_rgb(val) { cfg.music_accent_rgb = c; },
                "music_text_rgb" => if let Some(c) = parse_hex_rgb(val) { cfg.music_text_rgb = c; },
                "music_bg_rgb" => if let Some(c) = parse_hex_rgb(val) { cfg.music_bg_rgb = c; },
                "cc_scale_pct" => if let Ok(v) = val.parse::<u8>() { cfg.cc_scale_pct = v.clamp(70, 150); },
                "cc_glass_alpha" => if let Ok(v) = val.parse::<u8>() { cfg.cc_glass_alpha = v; },
                "cc_rim_alpha" => if let Ok(v) = val.parse::<u8>() { cfg.cc_rim_alpha = v; },
                "cc_tile_style" => {
                    if let Ok(v) = val.parse::<u8>() {
                        cfg.cc_tile_style = v.min(13);
                    } else if let Some(ht) = crate::horology_pbr::HorologyTheme::from_str(val) {
                        cfg.cc_tile_style = 4 + ht.idx() as u8;
                    }
                }
                "cc_corner_radius" => if let Ok(v) = val.parse::<u8>() { cfg.cc_corner_radius = v.min(36); },
                "cc_show_plate" => cfg.cc_show_plate = parse_bool(val),
                "cc_show_tiles" => cfg.cc_show_tiles = parse_bool(val),
                "cc_show_bright" => cfg.cc_show_bright = parse_bool(val),
                "cc_show_vol" => cfg.cc_show_vol = parse_bool(val),
                "cc_show_labels" => cfg.cc_show_labels = parse_bool(val),
                "cc_show_scrollbar" => cfg.cc_show_scrollbar = parse_bool(val),
                "cc_show_dividers" => cfg.cc_show_dividers = parse_bool(val),
                "cc_font_label" => cfg.cc_font_label = val.to_string(),
                "cc_font_val" => cfg.cc_font_val = val.to_string(),
                "cc_font_scale_pct" => if let Ok(v) = val.parse::<u8>() { cfg.cc_font_scale_pct = v.clamp(75, 135); },
                "cc_use_custom_color" => cfg.cc_use_custom_color = parse_bool(val),
                "cc_accent_rgb" => if let Some(c) = parse_hex_rgb(val) { cfg.cc_accent_rgb = c; },
                "cc_text_rgb" => if let Some(c) = parse_hex_rgb(val) { cfg.cc_text_rgb = c; },
                "cc_bg_rgb" => if let Some(c) = parse_hex_rgb(val) { cfg.cc_bg_rgb = c; },
                "dt_horology" => cfg.dt_horology = crate::horology_pbr::HorologyPartConfig::from_csv(val),
                "apps_horology" => cfg.apps_horology = crate::horology_pbr::HorologyPartConfig::from_csv(val),
                "vitals_horology" => cfg.vitals_horology = crate::horology_pbr::HorologyPartConfig::from_csv(val),
                "music_horology" => cfg.music_horology = crate::horology_pbr::HorologyPartConfig::from_csv(val),
                "cc_horology" => cfg.cc_horology = crate::horology_pbr::HorologyPartConfig::from_csv(val),
                "wallpaper_enabled" => cfg.wallpaper_enabled = parse_bool(val),
                "wallpaper_path" if !val.is_empty() => cfg.wallpaper_path = val.to_string(),
                "wallpaper_pause_maximized" => cfg.wallpaper_pause_maximized = parse_bool(val),
                "wallpaper_trim_memory" => cfg.wallpaper_trim_memory = parse_bool(val),
                "wallpaper_mute" => cfg.wallpaper_mute = parse_bool(val),
                "wallpaper_volume_pct" => if let Ok(v) = val.parse::<u8>() { cfg.wallpaper_volume_pct = v.min(100); },
                "wallpaper_fps_cap" => cfg.wallpaper_fps_cap = WallpaperFpsCap::from_str(val),
                "game_mode_pause" => cfg.game_mode_pause = parse_bool(val),
                "onboarding_done" => {
                    cfg.onboarding_done = parse_bool(val);
                    saw_onboarding = true;
                }
                "mac_dock_enabled" => cfg.mac_dock_enabled = parse_bool(val),
                "mac_dock_edge" => cfg.mac_dock_edge = MacDockEdge::from_str(val),
                "mac_dock_hide" => cfg.mac_dock_hide = MacDockHideMode::from_str(val),
                "mac_dock_icon_size" => if let Ok(v) = val.parse::<u8>() { cfg.mac_dock_icon_size = v.clamp(24, 48); },
                "mac_dock_mag_pct" => if let Ok(v) = val.parse::<u8>() { cfg.mac_dock_mag_pct = v.clamp(100, 185); },
                "mac_dock_bounce" => cfg.mac_dock_bounce = parse_bool(val),
                "mac_dock_dots" => cfg.mac_dock_dots = parse_bool(val),
                "dock_bar_enabled" => {
                    cfg.dock_bar_enabled = parse_bool(val);
                    saw_dock_bar_enabled = true;
                }
                "clean_top_bar_enabled" => cfg.clean_top_bar_enabled = parse_bool(val),
                "clean_top_bar_edge" => cfg.clean_top_bar_edge = CleanTopBarEdge::from_str(val),
                "clean_top_bar_style" => cfg.clean_top_bar_style = CleanTopBarStyle::from_str(val),
                "clean_top_bar_desktop_only" => cfg.clean_top_bar_desktop_only = parse_bool(val),
                "dock_sync_win_pins" => {
                    cfg.dock_sync_win_pins = parse_bool(val);
                    saw_dock_sync_flag = true;
                }
                "dock_pin_count" => {
                    if let Ok(n) = val.parse::<usize>() {
                        dock_pin_count = Some(n.min(MAX_PINNED_APPS));
                    }
                }
                // Phase 6: Modular Per-Widget Hyper-Customization keys
                "w_shelf_theme" => cfg.w_shelf.theme_override = TaskbarTheme::from_opt_str(val),
                "w_shelf_layout" => cfg.w_shelf.layout_override = DockLayoutKind::from_opt_str(val),
                "w_shelf_shape" => cfg.w_shelf.shape_override = DockShelfShape::from_opt_str(val),
                "w_shelf_art" => cfg.w_shelf.art_override = DockArtLayer::from_opt_str(val),
                "w_shelf_glass_pct" => if let Ok(v) = val.parse::<u8>() { cfg.w_shelf.glass_alpha_pct = v.clamp(0, 100); },
                "w_shelf_rim_pct" => if let Ok(v) = val.parse::<u8>() { cfg.w_shelf.rim_alpha_pct = v.clamp(0, 100); },
                "w_shelf_show_plate" => cfg.w_shelf.show_plate = parse_bool(val),
                "w_shelf_show_rim" => cfg.w_shelf.show_rim = parse_bool(val),
                "w_shelf_show_art" => cfg.w_shelf.show_art = parse_bool(val),

                "w_apps_theme" => cfg.w_apps.theme_override = TaskbarTheme::from_opt_str(val),
                "w_apps_card" => cfg.w_apps.card_style = DockSlotCardStyle::from_str(val),
                "w_apps_indicator" => cfg.w_apps.indicator_override = DockIndicatorStyle::from_opt_str(val),
                "w_apps_divider" => cfg.w_apps.divider_style = DockDividerStyle::from_str(val),
                "w_apps_labels" => cfg.w_apps.label_mode = DockRunningLabelMode::from_str(val),
                "w_apps_label_font_size" => if let Ok(v) = val.parse::<u8>() { cfg.w_apps.label_font_size_px = v.clamp(10, 20); },
                "w_apps_brand_glow" => cfg.w_apps.per_app_brand_glow = parse_bool(val),
                "w_apps_custom_glow" => cfg.w_apps.custom_glow_rgb = parse_hex_rgb(val),
                "w_apps_count_badge" => cfg.w_apps.show_window_count_badge = parse_bool(val),
                "w_apps_mclick" => if let Ok(v) = val.parse::<u8>() { cfg.w_apps.middle_click_action = v.min(2); },
                "w_apps_wheel" => if let Ok(v) = val.parse::<u8>() { cfg.w_apps.wheel_action = v.min(1); },
                "w_apps_card_opacity" => if let Ok(v) = val.parse::<u8>() { cfg.w_apps.card_opacity_pct = v.min(100); },
                "w_apps_show_icons" => cfg.w_apps.show_icons = parse_bool(val),
                "w_apps_show_cards" => cfg.w_apps.show_cards = parse_bool(val),
                "w_apps_show_indicators" => cfg.w_apps.show_indicators = parse_bool(val),
                "w_apps_show_separators" => cfg.w_apps.show_separators = parse_bool(val),

                "w_clock_theme" => cfg.w_clock.theme_override = TaskbarTheme::from_opt_str(val),
                "w_clock_engine" => cfg.w_clock.engine = DockClockEngine::from_str(val),
                "w_clock_24h" => {
                    let low = val.trim().to_ascii_lowercase();
                    cfg.w_clock.use_24h = if low == "inherit" || low == "none" || low.is_empty() {
                        None
                    } else {
                        Some(parse_bool(val))
                    };
                }
                "w_clock_show_time" => cfg.w_clock.show_time = parse_bool(val),
                "w_clock_seconds" => cfg.w_clock.show_seconds = parse_bool(val),
                "w_clock_date" => cfg.w_clock.show_date = parse_bool(val),
                "w_clock_weekday" => cfg.w_clock.show_weekday = parse_bool(val),
                "w_clock_accent" => cfg.w_clock.custom_accent_rgb = parse_hex_rgb(val),
                "w_clock_text" => cfg.w_clock.custom_text_rgb = parse_hex_rgb(val),
                "w_clock_scale_pct" => if let Ok(v) = val.parse::<u8>() { cfg.w_clock.font_scale_pct = v.clamp(75, 135); },
                "w_clock_tz_half_hr" => if let Ok(v) = val.parse::<i8>() { cfg.w_clock.tz_offset_half_hours = v.clamp(-24, 28); },
                "w_clock_wheel" => cfg.w_clock.wheel_action = DockClockWheelAction::from_str(val),

                "w_tray_theme" => cfg.w_tray.theme_override = TaskbarTheme::from_opt_str(val),
                "w_tray_audio" => cfg.w_tray.audio_style = TrayAudioMeterStyle::from_str(val),
                "w_tray_battery" => cfg.w_tray.battery_style = TrayBatteryMeterStyle::from_str(val),
                "w_tray_net" => cfg.w_tray.net_style = TrayNetMeterStyle::from_str(val),
                "w_tray_osd" => cfg.w_tray.show_osd_on_scroll = parse_bool(val),
                "w_tray_wheel_step" => if let Ok(v) = val.parse::<u8>() { cfg.w_tray.wheel_step_pct = v.clamp(1, 10); },
                "w_tray_accent" => cfg.w_tray.custom_accent_rgb = parse_hex_rgb(val),
                "w_tray_show_net" => cfg.w_tray.show_net = parse_bool(val),
                "w_tray_show_audio" => cfg.w_tray.show_audio = parse_bool(val),
                "w_tray_show_battery" => cfg.w_tray.show_battery = parse_bool(val),

                "w_hud_mode" => cfg.w_hud.mode = HudWingMode::from_str(val),
                "w_hud_theme" => cfg.w_hud.theme_override = TaskbarTheme::from_opt_str(val),
                "w_hud_shape" => cfg.w_hud.shape = HudWingShape::from_str(val),
                "w_hud_content" => cfg.w_hud.content = HudTelemetryContent::from_str(val),
                "w_hud_prefix" => cfg.w_hud.custom_prefix = val.to_string(),
                "w_hud_width_pct" => if let Ok(v) = val.parse::<u8>() { cfg.w_hud.width_pct = v.clamp(40, 95); },
                "w_hud_alpha_pct" => if let Ok(v) = val.parse::<u8>() { cfg.w_hud.plate_alpha_pct = v.clamp(50, 100); },
                "w_hud_marquee" => cfg.w_hud.marquee_ticker = parse_bool(val),
                "w_hud_show_telemetry" => cfg.w_hud.show_telemetry = parse_bool(val),
                "w_hud_show_active_app" => cfg.w_hud.show_active_app = parse_bool(val),

                "w_util_theme" => cfg.w_util.theme_override = TaskbarTheme::from_opt_str(val),
                "w_util_start_style" => cfg.w_util.start_style = StartBadgeStyle::from_str(val),
                "w_util_show_start" => cfg.w_util.show_start = parse_bool(val),
                "w_util_show_search" => cfg.w_util.show_search = parse_bool(val),
                "w_util_show_taskview" => cfg.w_util.show_taskview = parse_bool(val),
                "w_util_show_trash" => cfg.w_util.show_trash = parse_bool(val),
                "w_util_show_studio" => cfg.w_util.show_studio = parse_bool(val),
                "w_util_show_sliver" => cfg.w_util.show_desktop_sliver = parse_bool(val),

                "canvas_schema" => {
                    if let Ok(v) = val.parse::<u8>() {
                        canvas_schema = v;
                    }
                }
                _ => {
                    if let Some(rest) = key.strip_prefix("dock_pin_") {
                        if let Some((idx_str, field)) = rest.split_once('_') {
                            if let Ok(idx) = idx_str.parse::<usize>() {
                                if idx < dock_pin_slots.len() {
                                    match field {
                                        "name" => dock_pin_slots[idx].0 = val.to_string(),
                                        "path" => dock_pin_slots[idx].1 = val.to_string(),
                                        _ => {}
                                    }
                                }
                            }
                        }
                    } else if let Some(idx_s) = key.strip_prefix("dt_tr_") {
                        if let (Ok(idx), Some(tr)) = (idx_s.parse::<usize>(), ClockElemTransform::from_csv(val)) {
                            if idx < cfg.dt_elem_transforms.len() {
                                cfg.dt_elem_transforms[idx] = tr;
                            }
                        }
                    } else if let Some(idx_s) = key.strip_prefix("dt_elem_font_") {
                        if let Ok(idx) = idx_s.parse::<usize>() {
                            if idx < cfg.dt_elem_fonts.len() {
                                cfg.dt_elem_fonts[idx] = val.to_string();
                            }
                        }
                    } else if let Some(idx_s) = key.strip_prefix("apps_elem_font_") {
                        if let Ok(idx) = idx_s.parse::<usize>() {
                            if idx < cfg.apps_elem_fonts.len() {
                                cfg.apps_elem_fonts[idx] = val.to_string();
                            }
                        }
                    } else if let Some(idx_s) = key.strip_prefix("apps_elem_") {
                        if let (Ok(idx), Some(tr)) = (idx_s.parse::<usize>(), ClockElemTransform::from_csv(val)) {
                            if idx < cfg.apps_elem_transforms.len() {
                                cfg.apps_elem_transforms[idx] = tr;
                            }
                        }
                    } else if let Some(idx_s) = key.strip_prefix("vitals_elem_font_") {
                        if let Ok(idx) = idx_s.parse::<usize>() {
                            if idx < cfg.vitals_elem_fonts.len() {
                                cfg.vitals_elem_fonts[idx] = val.to_string();
                            }
                        }
                    } else if let Some(idx_s) = key.strip_prefix("vitals_elem_") {
                        if let (Ok(idx), Some(tr)) = (idx_s.parse::<usize>(), ClockElemTransform::from_csv(val)) {
                            if idx < cfg.vitals_elem_transforms.len() {
                                cfg.vitals_elem_transforms[idx] = tr;
                            }
                        }
                    } else if let Some(idx_s) = key.strip_prefix("music_elem_font_") {
                        if let Ok(idx) = idx_s.parse::<usize>() {
                            if idx < cfg.music_elem_fonts.len() {
                                cfg.music_elem_fonts[idx] = val.to_string();
                            }
                        }
                    } else if let Some(idx_s) = key.strip_prefix("music_elem_") {
                        if let (Ok(idx), Some(tr)) = (idx_s.parse::<usize>(), ClockElemTransform::from_csv(val)) {
                            if idx < cfg.music_elem_transforms.len() {
                                cfg.music_elem_transforms[idx] = tr;
                            }
                        }
                    } else if let Some(idx_s) = key.strip_prefix("cc_elem_font_") {
                        if let Ok(idx) = idx_s.parse::<usize>() {
                            if idx < cfg.cc_elem_fonts.len() {
                                cfg.cc_elem_fonts[idx] = val.to_string();
                            }
                        }
                    } else if let Some(idx_s) = key.strip_prefix("cc_elem_") {
                        if let (Ok(idx), Some(tr)) = (idx_s.parse::<usize>(), ClockElemTransform::from_csv(val)) {
                            if idx < cfg.cc_elem_transforms.len() {
                                cfg.cc_elem_transforms[idx] = tr;
                            }
                        }
                    } else if let Some(rest) = key.strip_prefix("ext_elem_font_") {
                        if let Some((w_s, i_s)) = rest.split_once('_') {
                            if let (Ok(wi), Ok(ei)) = (w_s.parse::<usize>(), i_s.parse::<usize>()) {
                                pending_ext_fonts.push((wi, ei, val.to_string()));
                            }
                        }
                    } else if let Some(rest) = key.strip_prefix("ext_elem_") {
                        if let Some((w_s, i_s)) = rest.split_once('_') {
                            if let (Ok(wi), Ok(ei), Some(tr)) = (
                                w_s.parse::<usize>(),
                                i_s.parse::<usize>(),
                                ClockElemTransform::from_csv(val),
                            ) {
                                pending_ext_elems.push((wi, ei, tr));
                            }
                        }
                    } else if let Some(rest) = key.strip_prefix("app_") {
                        if let Some((idx_str, field)) = rest.split_once('_') {
                            if let Ok(idx) = idx_str.parse::<usize>() {
                                if idx < dynamic_slots.len() {
                                    match field {
                                        "name" => dynamic_slots[idx].0 = val.to_string(),
                                        "path" => dynamic_slots[idx].1 = val.to_string(),
                                        "dx" => {
                                            if let Ok(d) = val.parse::<i32>() {
                                                dynamic_slots[idx].2 = d.clamp(-1280, 1280);
                                            }
                                        }
                                        "dy" => {
                                            if let Ok(d) = val.parse::<i32>() {
                                                dynamic_slots[idx].3 = d.clamp(-800, 800);
                                            }
                                        }
                                        _ => {}
                                    }
                                }
                            }
                        }
                    } else if let Some(idx_s) = key.strip_prefix("ext_horology_") {
                        if let Ok(idx) = idx_s.parse::<usize>() {
                            pending_ext_horology.push((
                                idx,
                                crate::horology_pbr::HorologyPartConfig::from_csv(val),
                            ));
                        }
                    } else if let Some(idx_s) = key.strip_prefix("ext_widget_") {
                        if let Ok(idx) = idx_s.parse::<usize>() {
                            if let Some(id) = ExtWidgetId::from_idx(idx) {
                                cfg.ext_widgets[idx] = ExtWidgetConfig::from_csv(id, val);
                            }
                        }
                    } else if key == "notes_title" {
                        let t = val.replace("â€”", "-").replace("â€“", "-");
                        let t = t.trim();
                        if !t.is_empty() {
                            cfg.notes_title = t.to_string();
                        }
                    } else if key == "world_clock_cities" {
                        for (i, part) in val.split('|').take(4).enumerate() {
                            let c = part.trim();
                            if !c.is_empty() {
                                cfg.world_clock_cities[i] = c.to_string();
                            }
                        }
                    } else if key == "note_item_count" {
                        if let Ok(cnt) = val.parse::<usize>() {
                            cfg.notes_items.truncate(cnt.min(12));
                        }
                    } else if let Some(idx_s) = key.strip_prefix("note_item_") {
                        if let Ok(idx) = idx_s.parse::<usize>() {
                            if idx < 12 {
                                if let Some((d_s, txt)) = val.split_once('|') {
                                    let done = d_s.trim() == "1";
                                    let fixed = txt.replace("â€”", "-").replace("â€“", "-");
                                    let clean_t = fixed.trim().to_string();
                                    if !clean_t.is_empty() {
                                        if idx < cfg.notes_items.len() {
                                            cfg.notes_items[idx] = (done, clean_t);
                                        } else {
                                            cfg.notes_items.push((done, clean_t));
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    if !saw_anim_fps {
        cfg.set_anim_fps(AppConfig::DEFAULT_ANIM_FPS);
    }
    if !saw_dock_bar_enabled {
        cfg.dock_bar_enabled = cfg.clean_top_bar_enabled;
    }
    if cfg.mac_dock_enabled && cfg.mac_dock_hide == MacDockHideMode::SmartAutoHide {
        cfg.mac_dock_hide = MacDockHideMode::AlwaysVisible;
    }

    if canvas_schema < 6 {
        if !cfg.music_elem_transforms[4].is_default() && cfg.music_elem_transforms[5].is_default() {
            cfg.music_elem_transforms[5] = cfg.music_elem_transforms[4];
            cfg.music_elem_transforms[4] = ClockElemTransform::default();
        }
        if !cfg.music_elem_fonts[4].is_empty() && cfg.music_elem_fonts[5].is_empty() {
            cfg.music_elem_fonts[5] = std::mem::take(&mut cfg.music_elem_fonts[4]);
        }
        if !cfg.cc_elem_transforms[4].is_default() && cfg.cc_elem_transforms[5].is_default() {
            cfg.cc_elem_transforms[5] = cfg.cc_elem_transforms[4];
            cfg.cc_elem_transforms[4] = ClockElemTransform::default();
        }
        if !cfg.cc_elem_fonts[4].is_empty() && cfg.cc_elem_fonts[5].is_empty() {
            cfg.cc_elem_fonts[5] = std::mem::take(&mut cfg.cc_elem_fonts[4]);
        }
    }

    for (wi, ei, tr) in pending_ext_elems {
        let target_ei = if canvas_schema < 6 && ei == 4 { 5 } else { ei };
        if wi < cfg.ext_widgets.len() && target_ei < cfg.ext_widgets[wi].elem_transforms.len() {
            cfg.ext_widgets[wi].elem_transforms[target_ei] = tr;
        }
    }
    for (wi, ei, f) in pending_ext_fonts {
        let target_ei = if canvas_schema < 6 && ei == 4 { 5 } else { ei };
        if wi < cfg.ext_widgets.len() && target_ei < cfg.ext_widgets[wi].elem_fonts.len() {
            cfg.ext_widgets[wi].elem_fonts[target_ei] = f;
        }
    }
    for (wi, hp) in pending_ext_horology {
        if wi < cfg.ext_widgets.len() {
            cfg.ext_widgets[wi].horology = hp;
        }
    }

    if let Some(cnt) = app_count {
        cfg.apps.clear();
        for (name, p, dx, dy) in dynamic_slots.into_iter().take(cnt) {
            if !name.is_empty() || !p.is_empty() {
                cfg.apps.push(AppShortcut {
                    name,
                    path: p,
                    offset_x: dx,
                    offset_y: dy,
                });
            }
        }
    } else if saw_legacy {
        cfg.apps.clear();
        for (name, p) in legacy_slots {
            if !name.is_empty() || !p.is_empty() {
                cfg.apps.push(AppShortcut {
                    name,
                    path: p,
                    offset_x: 0,
                    offset_y: 0,
                });
            }
        }
    }

    if !saw_per_widget_match {
        if let Some(true) = legacy_dynamic_accent {
            cfg.tb_match_wallpaper = true;
            cfg.dt_match_wallpaper = true;
            cfg.apps_match_wallpaper = true;
            cfg.vitals_match_wallpaper = true;
            cfg.music_match_wallpaper = true;
            cfg.cc_match_wallpaper = true;
        }
    }
    if saw_viz_color_mode {
        cfg.tb_match_wallpaper = cfg.tb_viz_color_mode == VizColorMode::MatchWallpaper;
    } else if cfg.tb_match_wallpaper {
        cfg.tb_viz_color_mode = VizColorMode::MatchWallpaper;
    }
    if !saw_onboarding {
        cfg.onboarding_done = true;
    }

    if let Some(dcnt) = dock_pin_count {
        cfg.mac_dock_pinned.clear();
        for (name, p) in dock_pin_slots.into_iter().take(dcnt) {
            if !name.is_empty() || !p.is_empty() {
                cfg.mac_dock_pinned.push(AppShortcut {
                    name,
                    path: p,
                    offset_x: 0,
                    offset_y: 0,
                });
            }
        }
    }
    (cfg, app_count, saw_per_widget_match, saw_dock_sync_flag)
}

static SAVE_IO_LOCK: Mutex<()> = Mutex::new(());
static ASYNC_SAVE_QUEUE: OnceLock<Arc<(Mutex<Option<(PathBuf, String)>>, Condvar)>> =
    OnceLock::new();

fn write_config_data_atomic(path: &Path, data: &str) {
    let _guard = SAVE_IO_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    if write_atomic_synced(path, data).is_ok() {
        let bak = config_sibling_path(path, ".bak");
        let _ = write_atomic_synced(&bak, data);
    }
}

fn async_save_queue() -> &'static Arc<(Mutex<Option<(PathBuf, String)>>, Condvar)> {
    ASYNC_SAVE_QUEUE.get_or_init(|| {
        let q = Arc::new((Mutex::new(None::<(PathBuf, String)>), Condvar::new()));
        let q_worker = Arc::clone(&q);
        let _ = std::thread::Builder::new()
            .name("autumn-cfg-save".into())
            .spawn(move || loop {
                let pending = {
                    let (lock, cv) = &*q_worker;
                    let mut g = lock.lock().unwrap_or_else(|e| e.into_inner());
                    while g.is_none() {
                        g = cv.wait(g).unwrap_or_else(|e| e.into_inner());
                    }
                    g.take()
                };
                if let Some((path, data)) = pending {
                    write_config_data_atomic(&path, &data);
                }
            });
        q
    })
}

pub fn save_config_to_path(cfg: &AppConfig, path: &Path) {
    let data = serialize_config_ini(cfg);
    write_config_data_atomic(path, &data);
}

pub fn save_config(cfg: &AppConfig) {
    save_config_to_path(cfg, &config_path());
}

/// Non-blocking single-slot coalescing save for interactive Studio/widget editing.
/// Serializes on the caller thread (~0.05 ms) and offloads NTFS `sync_all` + atomic
/// `.tmp`/`.bak` renames to a single persistent background worker so the UI thread never stalls.
pub fn save_config_async(cfg: &AppConfig) {
    save_config_to_path_async(cfg, &config_path());
}

pub fn save_config_to_path_async(cfg: &AppConfig, path: &Path) {
    let data = serialize_config_ini(cfg);
    let (lock, cv) = &**async_save_queue();
    {
        let mut g = lock.lock().unwrap_or_else(|e| e.into_inner());
        *g = Some((path.to_path_buf(), data));
    }
    cv.notify_one();
}

/// Cancels any stale queued async snapshot and flushes `cfg` synchronously (used on exit/shutdown).
pub fn flush_pending_config_save(cfg: &AppConfig) {
    if let Some(q) = ASYNC_SAVE_QUEUE.get() {
        let mut g = q.0.lock().unwrap_or_else(|e| e.into_inner());
        *g = None;
    }
    save_config(cfg);
}

pub fn serialize_config_ini(cfg: &AppConfig) -> String {
    let b = |x: bool| if x { 1 } else { 0 };
    let clean = |s: &str| s.replace(['\r', '\n'], " ");
    let mut out = String::new();
    out.push_str("[AutumnStudioV5]\n");
    out.push_str(&format!("active_suite={}\n", clean(&cfg.active_suite)));
    out.push_str(&format!("mode={}\n", cfg.mode.as_str()));
    out.push_str(&format!("tb_theme={}\n", cfg.tb_theme.as_str()));
    out.push_str(&format!("pill_mode={}\n", cfg.pill_mode.as_str()));
    out.push_str(&format!("surface_mode={}\n", cfg.surface_mode.as_str()));
    out.push_str(&format!("tint_alpha={}\n", cfg.tint_alpha));
    out.push_str(&format!("amber_border={}\n", b(cfg.amber_border)));
    out.push_str(&format!("corner_radius={}\n", cfg.corner_radius));
    out.push_str(&format!("tb_viz_style={}\n", cfg.tb_viz_style.as_str()));
    out.push_str(&format!("tb_viz_opacity={}\n", cfg.tb_viz_opacity));
    out.push_str(&format!("tb_viz_mirror={}\n", b(cfg.tb_viz_mirror)));
    out.push_str(&format!("tb_viz_color_mode={}\n", cfg.tb_viz_color_mode.as_str()));
    out.push_str(&format!(
        "tb_viz_accent_rgb=0x{:06X}\n",
        cfg.tb_viz_accent_rgb & 0x00FF_FFFF
    ));
    out.push_str(&format!("tb_hover_react={}\n", b(cfg.tb_hover_react)));
    out.push_str(&format!("tb_ambient_anim={}\n", b(cfg.tb_ambient_anim)));
    out.push_str(&format!("tb_show_backdrop={}\n", b(cfg.tb_show_backdrop)));
    out.push_str(&format!("tb_show_rim={}\n", b(cfg.tb_show_rim)));
    out.push_str(&format!("tb_show_fx={}\n", b(cfg.tb_show_fx)));
    out.push_str(&format!("viz_eco_fps={}\n", b(cfg.viz_eco_fps)));
    out.push_str(&format!("performance_mode={}\n", cfg.performance_mode.as_str()));
    out.push_str(&format!("dt_hover_fx={}\n", cfg.dt_hover_fx.as_str()));
    out.push_str(&format!("apps_hover_fx={}\n", cfg.apps_hover_fx.as_str()));
    out.push_str(&format!("vitals_hover_fx={}\n", cfg.vitals_hover_fx.as_str()));
    out.push_str(&format!("music_hover_fx={}\n", cfg.music_hover_fx.as_str()));
    out.push_str(&format!("cc_hover_fx={}\n", cfg.cc_hover_fx.as_str()));
    out.push_str(&format!("widgets_enabled={}\n", b(cfg.widgets_enabled)));
    out.push_str(&format!("widgets_locked={}\n", b(cfg.widgets_locked)));
    out.push_str(&format!("widgets_24h={}\n", b(cfg.widgets_24h)));
    out.push_str(&format!("widgets_hover_react={}\n", b(cfg.widgets_hover_react)));
    out.push_str(&format!("widgets_live_material={}\n", b(cfg.widgets_live_material)));
    out.push_str(&format!("show_datetime={}\n", b(cfg.show_datetime)));
    out.push_str(&format!("show_apps={}\n", b(cfg.show_apps)));
    out.push_str(&format!("show_vitals={}\n", b(cfg.show_vitals)));
    out.push_str(&format!("show_music={}\n", b(cfg.show_music)));
    out.push_str(&format!("show_cc={}\n", b(cfg.show_cc)));

    out.push_str(&format!("dt_x={}\n", cfg.dt_x));
    out.push_str(&format!("dt_y={}\n", cfg.dt_y));
    out.push_str(&format!("apps_x={}\n", cfg.apps_x));
    out.push_str(&format!("apps_y={}\n", cfg.apps_y));
    out.push_str(&format!("vitals_x={}\n", cfg.vitals_x));
    out.push_str(&format!("vitals_y={}\n", cfg.vitals_y));
    out.push_str(&format!("music_x={}\n", cfg.music_x));
    out.push_str(&format!("music_y={}\n", cfg.music_y));
    out.push_str(&format!("cc_x={}\n", cfg.cc_x));
    out.push_str(&format!("cc_y={}\n", cfg.cc_y));

    out.push_str(&format!("dt_scale_pct={}\n", cfg.dt_scale_pct));
    out.push_str(&format!("dt_show_plate={}\n", b(cfg.dt_show_plate)));
    out.push_str(&format!("dt_show_clock={}\n", b(cfg.dt_show_clock)));
    out.push_str(&format!("dt_glass_alpha={}\n", cfg.dt_glass_alpha));
    out.push_str(&format!("dt_rim_alpha={}\n", cfg.dt_rim_alpha));
    out.push_str(&format!("dt_show_spine={}\n", b(cfg.dt_show_spine)));
    out.push_str(&format!("dt_show_orb={}\n", b(cfg.dt_show_orb)));
    out.push_str(&format!("dt_style={}\n", cfg.dt_style.as_str()));
    out.push_str(&format!("dt_align={}\n", cfg.dt_align.as_str()));
    out.push_str(&format!("dt_show_seconds={}\n", b(cfg.dt_show_seconds)));
    out.push_str(&format!("dt_show_ampm={}\n", b(cfg.dt_show_ampm)));
    out.push_str(&format!("dt_show_weekday={}\n", b(cfg.dt_show_weekday)));
    out.push_str(&format!("dt_show_date={}\n", b(cfg.dt_show_date)));
    out.push_str(&format!("dt_corner_radius={}\n", cfg.dt_corner_radius));
    out.push_str(&format!("dt_letter_spacing={}\n", cfg.dt_letter_spacing));
    out.push_str(&format!("dt_font_day={}\n", clean(&cfg.dt_font_day)));
    out.push_str(&format!("dt_font_script={}\n", clean(&cfg.dt_font_script)));
    out.push_str(&format!("dt_font_clock={}\n", clean(&cfg.dt_font_clock)));
    out.push_str(&format!("dt_font_sub={}\n", clean(&cfg.dt_font_sub)));
    out.push_str(&format!("dt_use_custom_color={}\n", b(cfg.dt_use_custom_color)));
    out.push_str(&format!("dt_accent_rgb=0x{:06X}\n", cfg.dt_accent_rgb & 0x00FF_FFFF));
    out.push_str(&format!("dt_digits_rgb=0x{:06X}\n", cfg.dt_digits_rgb & 0x00FF_FFFF));
    out.push_str(&format!("dt_script_rgb=0x{:06X}\n", cfg.dt_script_rgb & 0x00FF_FFFF));
    out.push_str(&format!("dt_bg_rgb=0x{:06X}\n", cfg.dt_bg_rgb & 0x00FF_FFFF));
    let write_elems = |out: &mut String, prefix: &str, f_prefix: &str, trs: &[ClockElemTransform], fonts: &[String]| {
        for (i, tr) in trs.iter().enumerate() {
            if !tr.is_default() {
                out.push_str(&format!("{}{}={}\n", prefix, i, tr.to_csv()));
            }
        }
        for (i, f) in fonts.iter().enumerate() {
            if !f.trim().is_empty() {
                out.push_str(&format!("{}{}={}\n", f_prefix, i, clean(f)));
            }
        }
    };
    write_elems(&mut out, "dt_tr_", "dt_elem_font_", &cfg.dt_elem_transforms, &cfg.dt_elem_fonts);
    write_elems(&mut out, "apps_elem_", "apps_elem_font_", &cfg.apps_elem_transforms, &cfg.apps_elem_fonts);
    write_elems(&mut out, "vitals_elem_", "vitals_elem_font_", &cfg.vitals_elem_transforms, &cfg.vitals_elem_fonts);
    write_elems(&mut out, "music_elem_", "music_elem_font_", &cfg.music_elem_transforms, &cfg.music_elem_fonts);
    write_elems(&mut out, "cc_elem_", "cc_elem_font_", &cfg.cc_elem_transforms, &cfg.cc_elem_fonts);
    out.push_str(&format!("tb_match_wallpaper={}\n", b(cfg.tb_match_wallpaper)));
    out.push_str(&format!("dt_match_wallpaper={}\n", b(cfg.dt_match_wallpaper)));
    out.push_str(&format!("apps_match_wallpaper={}\n", b(cfg.apps_match_wallpaper)));
    out.push_str(&format!("vitals_match_wallpaper={}\n", b(cfg.vitals_match_wallpaper)));
    out.push_str(&format!("music_match_wallpaper={}\n", b(cfg.music_match_wallpaper)));
    out.push_str(&format!("cc_match_wallpaper={}\n", b(cfg.cc_match_wallpaper)));

    out.push_str(&format!("apps_scale_pct={}\n", cfg.apps_scale_pct));
    out.push_str(&format!("apps_glass_alpha={}\n", cfg.apps_glass_alpha));
    out.push_str(&format!("apps_rim_alpha={}\n", cfg.apps_rim_alpha));
    out.push_str(&format!("apps_show_add_tile={}\n", b(cfg.apps_show_add_tile)));
    out.push_str(&format!("apps_match_theme={}\n", b(cfg.apps_match_theme)));
    out.push_str(&format!("apps_individual_move={}\n", b(cfg.apps_individual_move)));
    let cnt = cfg.apps.len().min(12);
    out.push_str(&format!("app_count={}\n", cnt));
    for (i, app) in cfg.apps.iter().take(cnt).enumerate() {
        out.push_str(&format!("app_{}_name={}\n", i, clean(&app.name)));
        out.push_str(&format!("app_{}_path={}\n", i, clean(&app.path)));
        out.push_str(&format!("app_{}_dx={}\n", i, app.offset_x));
        out.push_str(&format!("app_{}_dy={}\n", i, app.offset_y));
    }

    out.push_str(&format!("vitals_scale_pct={}\n", cfg.vitals_scale_pct));
    out.push_str(&format!("vitals_glass_alpha={}\n", cfg.vitals_glass_alpha));
    out.push_str(&format!("vitals_rim_alpha={}\n", cfg.vitals_rim_alpha));
    out.push_str(&format!("vitals_show_cpu={}\n", b(cfg.vitals_show_cpu)));
    out.push_str(&format!("vitals_show_ram={}\n", b(cfg.vitals_show_ram)));
    out.push_str(&format!("vitals_show_power={}\n", b(cfg.vitals_show_power)));
    out.push_str(&format!("vitals_show_buttons={}\n", b(cfg.vitals_show_buttons)));
    out.push_str(&format!("vitals_style={}\n", cfg.vitals_style));
    out.push_str(&format!("vitals_corner_radius={}\n", cfg.vitals_corner_radius));
    out.push_str(&format!("vitals_show_plate={}\n", b(cfg.vitals_show_plate)));
    out.push_str(&format!("vitals_show_dividers={}\n", b(cfg.vitals_show_dividers)));
    out.push_str(&format!("vitals_show_bars={}\n", b(cfg.vitals_show_bars)));
    out.push_str(&format!("vitals_font_val={}\n", clean(&cfg.vitals_font_val)));
    out.push_str(&format!("vitals_font_lbl={}\n", clean(&cfg.vitals_font_lbl)));
    out.push_str(&format!("vitals_font_scale_pct={}\n", cfg.vitals_font_scale_pct));
    out.push_str(&format!("vitals_use_custom_color={}\n", b(cfg.vitals_use_custom_color)));
    out.push_str(&format!("vitals_accent_rgb=0x{:06X}\n", cfg.vitals_accent_rgb & 0x00FF_FFFF));
    out.push_str(&format!("vitals_text_rgb=0x{:06X}\n", cfg.vitals_text_rgb & 0x00FF_FFFF));
    out.push_str(&format!("vitals_bg_rgb=0x{:06X}\n", cfg.vitals_bg_rgb & 0x00FF_FFFF));

    out.push_str(&format!("apps_cols={}\n", cfg.apps_cols));
    out.push_str(&format!("apps_tile_shape={}\n", cfg.apps_tile_shape));
    out.push_str(&format!("apps_corner_radius={}\n", cfg.apps_corner_radius));
    out.push_str(&format!("apps_show_plate={}\n", b(cfg.apps_show_plate)));
    out.push_str(&format!("apps_show_tiles={}\n", b(cfg.apps_show_tiles)));
    out.push_str(&format!("apps_show_labels={}\n", b(cfg.apps_show_labels)));
    out.push_str(&format!("apps_show_dots={}\n", b(cfg.apps_show_dots)));
    out.push_str(&format!("apps_font_label={}\n", clean(&cfg.apps_font_label)));
    out.push_str(&format!("apps_font_scale_pct={}\n", cfg.apps_font_scale_pct));
    out.push_str(&format!("apps_use_custom_color={}\n", b(cfg.apps_use_custom_color)));
    out.push_str(&format!("apps_accent_rgb=0x{:06X}\n", cfg.apps_accent_rgb & 0x00FF_FFFF));
    out.push_str(&format!("apps_text_rgb=0x{:06X}\n", cfg.apps_text_rgb & 0x00FF_FFFF));
    out.push_str(&format!("apps_bg_rgb=0x{:06X}\n", cfg.apps_bg_rgb & 0x00FF_FFFF));

    out.push_str(&format!("music_style={}\n", cfg.music_style.as_str()));
    out.push_str(&format!("music_scale_pct={}\n", cfg.music_scale_pct));
    out.push_str(&format!("music_glass_alpha={}\n", cfg.music_glass_alpha));
    out.push_str(&format!("music_rim_alpha={}\n", cfg.music_rim_alpha));
    out.push_str(&format!("music_show_viz={}\n", b(cfg.music_show_viz)));
    out.push_str(&format!("music_corner_radius={}\n", cfg.music_corner_radius));
    out.push_str(&format!("music_show_plate={}\n", b(cfg.music_show_plate)));
    out.push_str(&format!("music_show_cover={}\n", b(cfg.music_show_cover)));
    out.push_str(&format!("music_show_title={}\n", b(cfg.music_show_title)));
    out.push_str(&format!("music_show_artist={}\n", b(cfg.music_show_artist)));
    out.push_str(&format!("music_show_controls={}\n", b(cfg.music_show_controls)));
    out.push_str(&format!("music_show_badge={}\n", b(cfg.music_show_badge)));
    out.push_str(&format!("music_font_title={}\n", clean(&cfg.music_font_title)));
    out.push_str(&format!("music_font_artist={}\n", clean(&cfg.music_font_artist)));
    out.push_str(&format!("music_font_scale_pct={}\n", cfg.music_font_scale_pct));
    out.push_str(&format!("music_use_custom_color={}\n", b(cfg.music_use_custom_color)));
    out.push_str(&format!("music_accent_rgb=0x{:06X}\n", cfg.music_accent_rgb & 0x00FF_FFFF));
    out.push_str(&format!("music_text_rgb=0x{:06X}\n", cfg.music_text_rgb & 0x00FF_FFFF));
    out.push_str(&format!("music_bg_rgb=0x{:06X}\n", cfg.music_bg_rgb & 0x00FF_FFFF));

    out.push_str(&format!("cc_scale_pct={}\n", cfg.cc_scale_pct));
    out.push_str(&format!("cc_glass_alpha={}\n", cfg.cc_glass_alpha));
    out.push_str(&format!("cc_rim_alpha={}\n", cfg.cc_rim_alpha));
    out.push_str(&format!("cc_tile_style={}\n", cfg.cc_tile_style));
    out.push_str(&format!("cc_corner_radius={}\n", cfg.cc_corner_radius));
    out.push_str(&format!("cc_show_plate={}\n", b(cfg.cc_show_plate)));
    out.push_str(&format!("cc_show_tiles={}\n", b(cfg.cc_show_tiles)));
    out.push_str(&format!("cc_show_bright={}\n", b(cfg.cc_show_bright)));
    out.push_str(&format!("cc_show_vol={}\n", b(cfg.cc_show_vol)));
    out.push_str(&format!("cc_show_labels={}\n", b(cfg.cc_show_labels)));
    out.push_str(&format!("cc_show_scrollbar={}\n", b(cfg.cc_show_scrollbar)));
    out.push_str(&format!("cc_show_dividers={}\n", b(cfg.cc_show_dividers)));
    out.push_str(&format!("cc_font_label={}\n", clean(&cfg.cc_font_label)));
    out.push_str(&format!("cc_font_val={}\n", clean(&cfg.cc_font_val)));
    out.push_str(&format!("cc_font_scale_pct={}\n", cfg.cc_font_scale_pct));
    out.push_str(&format!("cc_use_custom_color={}\n", b(cfg.cc_use_custom_color)));
    out.push_str(&format!("cc_accent_rgb=0x{:06X}\n", cfg.cc_accent_rgb & 0x00FF_FFFF));
    out.push_str(&format!("cc_text_rgb=0x{:06X}\n", cfg.cc_text_rgb & 0x00FF_FFFF));
    out.push_str(&format!("cc_bg_rgb=0x{:06X}\n", cfg.cc_bg_rgb & 0x00FF_FFFF));
    if !cfg.dt_horology.is_default() {
        out.push_str(&format!("dt_horology={}\n", cfg.dt_horology.to_csv()));
    }
    if !cfg.apps_horology.is_default() {
        out.push_str(&format!("apps_horology={}\n", cfg.apps_horology.to_csv()));
    }
    if !cfg.vitals_horology.is_default() {
        out.push_str(&format!("vitals_horology={}\n", cfg.vitals_horology.to_csv()));
    }
    if !cfg.music_horology.is_default() {
        out.push_str(&format!("music_horology={}\n", cfg.music_horology.to_csv()));
    }
    if !cfg.cc_horology.is_default() {
        out.push_str(&format!("cc_horology={}\n", cfg.cc_horology.to_csv()));
    }

    out.push_str(&format!("wallpaper_enabled={}\n", b(cfg.wallpaper_enabled)));
    out.push_str(&format!("wallpaper_path={}\n", clean(&cfg.wallpaper_path)));
    out.push_str(&format!("wallpaper_pause_maximized={}\n", b(cfg.wallpaper_pause_maximized)));
    out.push_str(&format!("wallpaper_trim_memory={}\n", b(cfg.wallpaper_trim_memory)));
    out.push_str(&format!("wallpaper_mute={}\n", b(cfg.wallpaper_mute)));
    out.push_str(&format!("wallpaper_volume_pct={}\n", cfg.wallpaper_volume_pct));
    out.push_str(&format!("wallpaper_fps_cap={}\n", cfg.wallpaper_fps_cap.as_str()));
    out.push_str(&format!("game_mode_pause={}\n", b(cfg.game_mode_pause)));
    out.push_str(&format!("onboarding_done={}\n", b(cfg.onboarding_done)));
    for (i, ew) in cfg.ext_widgets.iter().enumerate() {
        out.push_str(&format!("ext_widget_{}={}\n", i, ew.to_csv()));
        if !ew.horology.is_default() {
            out.push_str(&format!("ext_horology_{}={}\n", i, ew.horology.to_csv()));
        }
        write_elems(
            &mut out,
            &format!("ext_elem_{}_", i),
            &format!("ext_elem_font_{}_", i),
            &ew.elem_transforms,
            &ew.elem_fonts,
        );
    }
    out.push_str(&format!(
        "world_clock_cities={}|{}|{}|{}\n",
        clean(&cfg.world_clock_cities[0]),
        clean(&cfg.world_clock_cities[1]),
        clean(&cfg.world_clock_cities[2]),
        clean(&cfg.world_clock_cities[3]),
    ));
    out.push_str(&format!("notes_title={}\n", clean(&cfg.notes_title)));
    let ncnt = cfg.notes_items.len().min(8);
    out.push_str(&format!("note_item_count={}\n", ncnt));
    for (i, (done, txt)) in cfg.notes_items.iter().take(8).enumerate() {
        out.push_str(&format!("note_item_{}={}|{}\n", i, b(*done), clean(txt)));
    }
    out.push_str(&format!("mac_dock_enabled={}\n", b(cfg.mac_dock_enabled)));
    out.push_str(&format!("mac_dock_edge={}\n", cfg.mac_dock_edge.as_str()));
    out.push_str(&format!("mac_dock_hide={}\n", cfg.mac_dock_hide.as_str()));
    out.push_str(&format!("mac_dock_icon_size={}\n", cfg.mac_dock_icon_size));
    out.push_str(&format!("mac_dock_mag_pct={}\n", cfg.mac_dock_mag_pct));
    out.push_str(&format!("mac_dock_bounce={}\n", b(cfg.mac_dock_bounce)));
    out.push_str(&format!("mac_dock_dots={}\n", b(cfg.mac_dock_dots)));
    out.push_str(&format!("dock_bar_enabled={}\n", b(cfg.dock_bar_enabled)));
    out.push_str(&format!("clean_top_bar_enabled={}\n", b(cfg.clean_top_bar_enabled)));
    out.push_str(&format!("clean_top_bar_edge={}\n", cfg.clean_top_bar_edge.as_str()));
    out.push_str(&format!("clean_top_bar_style={}\n", cfg.clean_top_bar_style.as_str()));
    out.push_str(&format!("clean_top_bar_desktop_only={}\n", b(cfg.clean_top_bar_desktop_only)));
    out.push_str(&format!("dock_sync_win_pins={}\n", b(cfg.dock_sync_win_pins)));
    let dcnt = cfg.mac_dock_pinned.len().min(MAX_PINNED_APPS);
    out.push_str(&format!("dock_pin_count={}\n", dcnt));
    for (i, app) in cfg.mac_dock_pinned.iter().take(dcnt).enumerate() {
        out.push_str(&format!("dock_pin_{}_name={}\n", i, clean(&app.name)));
        out.push_str(&format!("dock_pin_{}_path={}\n", i, clean(&app.path)));
    }

    let opt_hex = |opt: Option<u32>| match opt {
        Some(rgb) => format!("0x{:06X}", rgb & 0x00FF_FFFF),
        None => "none".to_string(),
    };
    out.push_str("\n[widget.shelf]\n");
    out.push_str(&format!("w_shelf_theme={}\n", TaskbarTheme::opt_to_str(cfg.w_shelf.theme_override)));
    out.push_str(&format!("w_shelf_layout={}\n", cfg.w_shelf.layout_override.map(|x| x.as_str()).unwrap_or("inherit")));
    out.push_str(&format!("w_shelf_shape={}\n", cfg.w_shelf.shape_override.map(|x| x.as_str()).unwrap_or("inherit")));
    out.push_str(&format!("w_shelf_art={}\n", cfg.w_shelf.art_override.map(|x| x.as_str()).unwrap_or("inherit")));
    out.push_str(&format!("w_shelf_glass_pct={}\n", cfg.w_shelf.glass_alpha_pct));
    out.push_str(&format!("w_shelf_rim_pct={}\n", cfg.w_shelf.rim_alpha_pct));
    out.push_str(&format!("w_shelf_show_plate={}\n", b(cfg.w_shelf.show_plate)));
    out.push_str(&format!("w_shelf_show_rim={}\n", b(cfg.w_shelf.show_rim)));
    out.push_str(&format!("w_shelf_show_art={}\n", b(cfg.w_shelf.show_art)));

    out.push_str("\n[widget.dock_apps]\n");
    out.push_str(&format!("w_apps_theme={}\n", TaskbarTheme::opt_to_str(cfg.w_apps.theme_override)));
    out.push_str(&format!("w_apps_card={}\n", cfg.w_apps.card_style.as_str()));
    out.push_str(&format!("w_apps_indicator={}\n", cfg.w_apps.indicator_override.map(|x| x.as_str()).unwrap_or("inherit")));
    out.push_str(&format!("w_apps_divider={}\n", cfg.w_apps.divider_style.as_str()));
    out.push_str(&format!("w_apps_labels={}\n", cfg.w_apps.label_mode.as_str()));
    out.push_str(&format!("w_apps_label_font_size={}\n", cfg.w_apps.label_font_size_px.clamp(10, 20)));
    out.push_str(&format!("w_apps_brand_glow={}\n", b(cfg.w_apps.per_app_brand_glow)));
    out.push_str(&format!("w_apps_custom_glow={}\n", opt_hex(cfg.w_apps.custom_glow_rgb)));
    out.push_str(&format!("w_apps_count_badge={}\n", b(cfg.w_apps.show_window_count_badge)));
    out.push_str(&format!("w_apps_mclick={}\n", cfg.w_apps.middle_click_action));
    out.push_str(&format!("w_apps_wheel={}\n", cfg.w_apps.wheel_action));
    out.push_str(&format!("w_apps_card_opacity={}\n", cfg.w_apps.card_opacity_pct.min(100)));
    out.push_str(&format!("w_apps_show_icons={}\n", b(cfg.w_apps.show_icons)));
    out.push_str(&format!("w_apps_show_cards={}\n", b(cfg.w_apps.show_cards)));
    out.push_str(&format!("w_apps_show_indicators={}\n", b(cfg.w_apps.show_indicators)));
    out.push_str(&format!("w_apps_show_separators={}\n", b(cfg.w_apps.show_separators)));

    out.push_str("\n[widget.clock]\n");
    out.push_str(&format!("w_clock_theme={}\n", TaskbarTheme::opt_to_str(cfg.w_clock.theme_override)));
    out.push_str(&format!("w_clock_engine={}\n", cfg.w_clock.engine.as_str()));
    out.push_str(&format!(
        "w_clock_24h={}\n",
        match cfg.w_clock.use_24h {
            Some(true) => "1",
            Some(false) => "0",
            None => "inherit",
        }
    ));
    out.push_str(&format!("w_clock_show_time={}\n", b(cfg.w_clock.show_time)));
    out.push_str(&format!("w_clock_seconds={}\n", b(cfg.w_clock.show_seconds)));
    out.push_str(&format!("w_clock_date={}\n", b(cfg.w_clock.show_date)));
    out.push_str(&format!("w_clock_weekday={}\n", b(cfg.w_clock.show_weekday)));
    out.push_str(&format!("w_clock_accent={}\n", opt_hex(cfg.w_clock.custom_accent_rgb)));
    out.push_str(&format!("w_clock_text={}\n", opt_hex(cfg.w_clock.custom_text_rgb)));
    out.push_str(&format!("w_clock_scale_pct={}\n", cfg.w_clock.font_scale_pct));
    out.push_str(&format!("w_clock_tz_half_hr={}\n", cfg.w_clock.tz_offset_half_hours));
    out.push_str(&format!("w_clock_wheel={}\n", cfg.w_clock.wheel_action.as_str()));

    out.push_str("\n[widget.tray]\n");
    out.push_str(&format!("w_tray_theme={}\n", TaskbarTheme::opt_to_str(cfg.w_tray.theme_override)));
    out.push_str(&format!("w_tray_audio={}\n", cfg.w_tray.audio_style.as_str()));
    out.push_str(&format!("w_tray_battery={}\n", cfg.w_tray.battery_style.as_str()));
    out.push_str(&format!("w_tray_net={}\n", cfg.w_tray.net_style.as_str()));
    out.push_str(&format!("w_tray_osd={}\n", b(cfg.w_tray.show_osd_on_scroll)));
    out.push_str(&format!("w_tray_wheel_step={}\n", cfg.w_tray.wheel_step_pct));
    out.push_str(&format!("w_tray_accent={}\n", opt_hex(cfg.w_tray.custom_accent_rgb)));
    out.push_str(&format!("w_tray_show_net={}\n", b(cfg.w_tray.show_net)));
    out.push_str(&format!("w_tray_show_audio={}\n", b(cfg.w_tray.show_audio)));
    out.push_str(&format!("w_tray_show_battery={}\n", b(cfg.w_tray.show_battery)));

    out.push_str("\n[widget.hud]\n");
    out.push_str(&format!("w_hud_mode={}\n", cfg.w_hud.mode.as_str()));
    out.push_str(&format!("w_hud_theme={}\n", TaskbarTheme::opt_to_str(cfg.w_hud.theme_override)));
    out.push_str(&format!("w_hud_shape={}\n", cfg.w_hud.shape.as_str()));
    out.push_str(&format!("w_hud_content={}\n", cfg.w_hud.content.as_str()));
    out.push_str(&format!("w_hud_prefix={}\n", clean(&cfg.w_hud.custom_prefix)));
    out.push_str(&format!("w_hud_width_pct={}\n", cfg.w_hud.width_pct));
    out.push_str(&format!("w_hud_alpha_pct={}\n", cfg.w_hud.plate_alpha_pct));
    out.push_str(&format!("w_hud_marquee={}\n", b(cfg.w_hud.marquee_ticker)));
    out.push_str(&format!("w_hud_show_telemetry={}\n", b(cfg.w_hud.show_telemetry)));
    out.push_str(&format!("w_hud_show_active_app={}\n", b(cfg.w_hud.show_active_app)));

    out.push_str("\n[widget.utilities]\n");
    out.push_str(&format!("w_util_theme={}\n", TaskbarTheme::opt_to_str(cfg.w_util.theme_override)));
    out.push_str(&format!("w_util_start_style={}\n", cfg.w_util.start_style.as_str()));
    out.push_str(&format!("w_util_show_start={}\n", b(cfg.w_util.show_start)));
    out.push_str(&format!("w_util_show_search={}\n", b(cfg.w_util.show_search)));
    out.push_str(&format!("w_util_show_taskview={}\n", b(cfg.w_util.show_taskview)));
    out.push_str(&format!("w_util_show_trash={}\n", b(cfg.w_util.show_trash)));
    out.push_str(&format!("w_util_show_studio={}\n", b(cfg.w_util.show_studio)));
    out.push_str(&format!("w_util_show_sliver={}\n", b(cfg.w_util.show_desktop_sliver)));
    out.push_str("canvas_schema=6\n");

    out.push_str("\n[General]\n");
    out.push_str(&format!("anim_fps={}\n", AppConfig::clamp_anim_fps(cfg.anim_fps)));
    out
}

pub fn ensure_theme_folders_on_disk() {
    let mut roots = vec![app_data_dir().join("Themes")];
    if !is_portable_mode() {
        if let Some(exe_dir) = self_exe_path().parent() {
            roots.push(exe_dir.join("Themes"));
        }
    }
    for root in roots {
        let tb_live_dir = root.join("Taskbar_Live_Themes");
        let tb_music_dir = root.join("Taskbar_Music_Themes");
        let music_wid_dir = root.join("Music_Themes");
        let clock_dir = root.join("Clock_Themes");
        let _ = fs::create_dir_all(&tb_live_dir);
        let _ = fs::create_dir_all(&tb_music_dir);
        let _ = fs::create_dir_all(&music_wid_dir);
        let _ = fs::create_dir_all(&clock_dir);

        for &t in &TaskbarTheme::LIVE_FOLDER {
            let (pill, surf, tint, amber, viz, mirror) = t.preview_preset();
            let content = format!(
                "[TaskbarLiveTheme]\nid={}\nname={}\nfolder=Live_Taskbar\npill_mode={}\nsurface_mode={}\ntint_alpha={}\namber_border={}\nviz_style={}\nviz_mirror={}\ndescription={}\n",
                t.as_str(),
                t.label(),
                pill.as_str(),
                surf.as_str(),
                tint,
                usize::from(amber),
                viz.as_str(),
                usize::from(mirror),
                t.subtitle()
            );
            let _ = fs::write(tb_live_dir.join(format!("{}.ini", t.as_str())), content);
        }

        for &t in &TaskbarTheme::MUSIC_FOLDER {
            let (pill, surf, tint, amber, viz, mirror) = t.preview_preset();
            let content = format!(
                "[TaskbarMusicTheme]\nid={}\nname={}\nfolder=Music_Taskbar\npill_mode={}\nsurface_mode={}\ntint_alpha={}\namber_border={}\nviz_style={}\nviz_mirror={}\ndescription={}\n",
                t.as_str(),
                t.label(),
                pill.as_str(),
                surf.as_str(),
                tint,
                usize::from(amber),
                viz.as_str(),
                usize::from(mirror),
                t.subtitle()
            );
            let _ = fs::write(tb_music_dir.join(format!("{}.ini", t.as_str())), content);
        }

        for &m in &MusicStyle::ALL {
            let content = format!(
                "[MusicWidgetTheme]\nid={}\nname={}\nbadge={}\ndescription={}\n",
                m.as_str(),
                m.label(),
                m.badge(),
                m.subtitle()
            );
            let _ = fs::write(music_wid_dir.join(format!("{}.ini", m.as_str())), content);
        }

        for &c in &ClockStyle::ALL {
            let (f_day, f_scr, f_clk, f_sub) = c.default_fonts();
            let content = format!(
                "[ClockTheme]\nid={}\nname={}\nfolder={}\nfont_day={}\nfont_script={}\nfont_clock={}\nfont_sub={}\ndescription={}\n",
                c.as_str(),
                c.label(),
                c.folder_badge(),
                f_day,
                f_scr,
                f_clk,
                f_sub,
                c.subtitle()
            );
            let _ = fs::write(clock_dir.join(format!("{}.ini", c.as_str())), content);
        }
    }
    crate::suites::ensure_suite_themes_on_disk();
}

pub fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

pub fn self_exe_path() -> PathBuf {
    unsafe {
        let mut buf = [0u16; 1024];
        let n = GetModuleFileNameW(None, &mut buf) as usize;
        PathBuf::from(String::from_utf16_lossy(&buf[..n]))
    }
}

pub fn resolve_resources_dir() -> Option<PathBuf> {
    let exe = self_exe_path();
    let dir = exe.parent()?;
    for cand in [
        dir.join("@Resources"),
        dir.parent().map(|p| p.join("@Resources")).unwrap_or_default(),
    ] {
        if !cand.as_os_str().is_empty() && cand.is_dir() {
            return Some(cand);
        }
    }
    None
}

const REG_RUN_KEY: PCWSTR = w!(r"Software\Microsoft\Windows\CurrentVersion\Run");
const REG_TASK_KEY: PCWSTR =
    w!(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion\Schedule\TaskCache\Tree\AutumnDeck");
const CREATE_NO_WINDOW_FLAG: u32 = 0x0800_0000;

fn xml_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(ch),
        }
    }
    out
}

fn has_logon_task() -> bool {
    unsafe {
        let mut hk = HKEY::default();
        if RegOpenKeyExW(HKEY_LOCAL_MACHINE, REG_TASK_KEY, None, KEY_READ, &mut hk).is_ok() {
            let _ = RegCloseKey(hk);
            return true;
        }
        false
    }
}

fn has_run_key() -> bool {
    unsafe {
        let mut hk = HKEY::default();
        if RegOpenKeyExW(HKEY_CURRENT_USER, REG_RUN_KEY, None, KEY_READ, &mut hk).is_err() {
            return false;
        }
        let ok = RegQueryValueExW(hk, w!("AutumnDeck"), None, None, None, None).is_ok();
        let _ = RegCloseKey(hk);
        ok
    }
}

fn delete_run_key() {
    unsafe {
        let mut hk = HKEY::default();
        if RegOpenKeyExW(HKEY_CURRENT_USER, REG_RUN_KEY, None, KEY_WRITE, &mut hk).is_ok() {
            let _ = RegDeleteValueW(hk, w!("AutumnDeck"));
            let _ = RegDeleteValueW(hk, w!("AutumnTaskbar"));
            let _ = RegCloseKey(hk);
        }
    }
}

fn write_run_key(exe_str: &str) {
    unsafe {
        let mut hk = HKEY::default();
        if RegCreateKeyExW(
            HKEY_CURRENT_USER,
            REG_RUN_KEY,
            None,
            PCWSTR::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_WRITE,
            None,
            &mut hk,
            None,
        )
        .is_ok()
        {
            let _ = RegDeleteValueW(hk, w!("AutumnTaskbar"));
            let val_w = to_wide(&format!("\"{}\"", exe_str));
            let bytes = std::slice::from_raw_parts(val_w.as_ptr() as *const u8, val_w.len() * 2);
            let _ = RegSetValueExW(hk, w!("AutumnDeck"), None, REG_SZ, Some(bytes));
            let _ = RegCloseKey(hk);
        }
    }
}

fn register_logon_task(exe_path: &Path) -> bool {
    let exe_str = exe_path.to_string_lossy();
    let work_dir = exe_path
        .parent()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();
    let user = std::env::var("USERNAME").unwrap_or_default();
    let domain = std::env::var("USERDOMAIN").unwrap_or_default();
    let user_id = if !domain.is_empty() && !user.is_empty() {
        format!("{}\\{}", domain, user)
    } else {
        user
    };
    let user_tag = if user_id.is_empty() {
        String::new()
    } else {
        format!("<UserId>{}</UserId>", xml_escape(&user_id))
    };
    let work_tag = if work_dir.is_empty() {
        String::new()
    } else {
        format!("<WorkingDirectory>{}</WorkingDirectory>", xml_escape(&work_dir))
    };
    let xml = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-16\"?>\r\n\
<Task version=\"1.2\" xmlns=\"http://schemas.microsoft.com/windows/2004/02/mit/task\">\r\n\
  <Triggers>\r\n\
    <LogonTrigger>\r\n\
      <Enabled>true</Enabled>\r\n\
      {user_tag}\r\n\
    </LogonTrigger>\r\n\
  </Triggers>\r\n\
  <Principals>\r\n\
    <Principal id=\"Author\">\r\n\
      {user_tag}\r\n\
      <LogonType>InteractiveToken</LogonType>\r\n\
      <RunLevel>LeastPrivilege</RunLevel>\r\n\
    </Principal>\r\n\
  </Principals>\r\n\
  <Settings>\r\n\
    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>\r\n\
    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>\r\n\
    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>\r\n\
    <AllowHardTerminate>false</AllowHardTerminate>\r\n\
    <StartWhenAvailable>true</StartWhenAvailable>\r\n\
    <RunOnlyIfNetworkAvailable>false</RunOnlyIfNetworkAvailable>\r\n\
    <AllowStartOnDemand>true</AllowStartOnDemand>\r\n\
    <Enabled>true</Enabled>\r\n\
    <Hidden>false</Hidden>\r\n\
    <RunOnlyIfIdle>false</RunOnlyIfIdle>\r\n\
    <WakeToRun>false</WakeToRun>\r\n\
    <ExecutionTimeLimit>PT0S</ExecutionTimeLimit>\r\n\
    <Priority>6</Priority>\r\n\
  </Settings>\r\n\
  <Actions Context=\"Author\">\r\n\
    <Exec>\r\n\
      <Command>\"{}\"</Command>\r\n\
      {work_tag}\r\n\
    </Exec>\r\n\
  </Actions>\r\n\
</Task>\r\n",
        xml_escape(&exe_str)
    );

    let tmp_xml = std::env::temp_dir().join("autumn_deck_logon.xml");
    let mut bytes = Vec::with_capacity(2 + xml.len() * 2);
    bytes.push(0xFF);
    bytes.push(0xFE);
    for u in xml.encode_utf16() {
        bytes.extend_from_slice(&u.to_le_bytes());
    }
    if fs::write(&tmp_xml, &bytes).is_err() {
        return false;
    }
    let _ = std::process::Command::new("schtasks.exe")
        .args(["/Delete", "/TN", "AutumnTaskbar", "/F"])
        .creation_flags(CREATE_NO_WINDOW_FLAG)
        .status();
    let ok = std::process::Command::new("schtasks.exe")
        .args(["/Create", "/TN", "AutumnDeck", "/XML"])
        .arg(&tmp_xml)
        .arg("/F")
        .creation_flags(CREATE_NO_WINDOW_FLAG)
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    let _ = fs::remove_file(&tmp_xml);
    ok
}

fn delete_logon_task() {
    let _ = std::process::Command::new("schtasks.exe")
        .args(["/Delete", "/TN", "AutumnDeck", "/F"])
        .creation_flags(CREATE_NO_WINDOW_FLAG)
        .status();
    let _ = std::process::Command::new("schtasks.exe")
        .args(["/Delete", "/TN", "AutumnTaskbar", "/F"])
        .creation_flags(CREATE_NO_WINDOW_FLAG)
        .status();
}

pub fn is_startup_enabled() -> bool {
    has_run_key()
}

pub fn is_logon_task_enabled() -> bool {
    has_logon_task()
}

pub fn set_startup_enabled(enable: bool) {
    if enable {
        let exe = self_exe_path();
        write_run_key(&exe.to_string_lossy());
        std::thread::spawn(move || {
            let _ = register_logon_task(&exe);
        });
    } else {
        delete_run_key();
        std::thread::spawn(|| {
            delete_logon_task();
        });
    }
}

/// Synchronously removes both HKCU Run key and the Windows Logon Scheduled Task.
/// Used by `--uninstall-cleanup` so Inno Setup never leaves orphan startup entries.
pub fn disable_startup_sync() {
    delete_run_key();
    delete_logon_task();
}

/// Automatically upgrades legacy HKCU\...\Run autostart to a zero-delay Windows Logon Scheduled Task.
pub fn upgrade_startup_to_fast_logon() {
    if has_run_key() && !has_logon_task() {
        let exe = self_exe_path();
        let _ = register_logon_task(&exe);
    }
}

const REG_EXPLORER_ADV: PCWSTR =
    w!(r"Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced");

fn read_hkcu_dword(subkey: PCWSTR, value_name: &str, default_val: u32) -> u32 {
    unsafe {
        let mut hk = HKEY::default();
        if RegOpenKeyExW(HKEY_CURRENT_USER, subkey, None, KEY_READ, &mut hk).is_err() {
            return default_val;
        }
        let wname = to_wide(value_name);
        let mut val_type = REG_VALUE_TYPE::default();
        let mut buf = [0u8; 4];
        let mut size = 4u32;
        let ok = RegQueryValueExW(
            hk,
            PCWSTR(wname.as_ptr()),
            None,
            Some(&mut val_type),
            Some(buf.as_mut_ptr()),
            Some(&mut size),
        )
        .is_ok();
        let _ = RegCloseKey(hk);
        if ok && val_type == REG_DWORD && size == 4 {
            u32::from_le_bytes(buf)
        } else {
            default_val
        }
    }
}

fn write_hkcu_dword(subkey: PCWSTR, value_name: &str, value: u32) -> bool {
    unsafe {
        let mut hk = HKEY::default();
        let res = RegCreateKeyExW(
            HKEY_CURRENT_USER,
            subkey,
            None,
            PCWSTR::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_WRITE,
            None,
            &mut hk,
            None,
        );
        if res.is_err() {
            return false;
        }
        let wname = to_wide(value_name);
        let bytes = value.to_le_bytes();
        let ok = RegSetValueExW(hk, PCWSTR(wname.as_ptr()), None, REG_DWORD, Some(&bytes)).is_ok();
        let _ = RegCloseKey(hk);
        ok
    }
}

pub fn read_win_taskbar_prefs() -> WinTaskbarPrefs {
    let align_center = read_hkcu_dword(REG_EXPLORER_ADV, "TaskbarAl", 1) != 0;
    let show_task_view = read_hkcu_dword(REG_EXPLORER_ADV, "ShowTaskViewButton", 1) != 0;
    let show_widgets = read_hkcu_dword(REG_EXPLORER_ADV, "TaskbarDa", 1) != 0;
    WinTaskbarPrefs {
        align_center,
        show_task_view,
        show_widgets,
    }
}

pub fn get_native_autohide() -> bool {
    unsafe {
        let main = FindWindowW(w!("Shell_TrayWnd"), PCWSTR::null()).unwrap_or_default();
        let mut abd = APPBARDATA {
            cbSize: std::mem::size_of::<APPBARDATA>() as u32,
            hWnd: main,
            ..Default::default()
        };
        let state = SHAppBarMessage(ABM_GETSTATE_CMD, &mut abd);
        (state & (ABS_AUTOHIDE_FLAG as usize)) != 0
    }
}

pub fn apply_win_explorer_pref_checked(value_name: &str, value: u32) -> bool {
    if write_hkcu_dword(REG_EXPLORER_ADV, value_name, value) {
        notify_tray_settings_changed();
        true
    } else {
        false
    }
}

pub fn apply_win_explorer_pref(value_name: &str, value: u32) {
    let _ = apply_win_explorer_pref_checked(value_name, value);
}

pub fn notify_tray_settings_changed() {
    unsafe {
        let tray_param = w!("TraySettings");
        let _ = SendNotifyMessageW(
            HWND_BROADCAST,
            WM_SETTINGCHANGE,
            WPARAM(0),
            LPARAM(tray_param.as_ptr() as isize),
        );
        let main = FindWindowW(w!("Shell_TrayWnd"), PCWSTR::null()).unwrap_or_default();
        if !main.0.is_null() {
            let mut res = 0usize;
            let _ = windows::Win32::UI::WindowsAndMessaging::SendMessageTimeoutW(
                main,
                WM_SETTINGCHANGE,
                WPARAM(0),
                LPARAM(tray_param.as_ptr() as isize),
                SMTO_ABORTIFHUNG,
                150,
                Some(&mut res),
            );
        }
    }
}

fn write_hkcu_sz(subkey: PCWSTR, value_name: &str, value: &str) -> bool {
    unsafe {
        let mut hk = HKEY::default();
        let res = RegCreateKeyExW(
            HKEY_CURRENT_USER,
            subkey,
            None,
            PCWSTR::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_WRITE,
            None,
            &mut hk,
            None,
        );
        if res.is_err() {
            return false;
        }
        let wname = to_wide(value_name);
        let wval = to_wide(value);
        let bytes = std::slice::from_raw_parts(wval.as_ptr() as *const u8, wval.len() * 2);
        let ok = RegSetValueExW(hk, PCWSTR(wname.as_ptr()), None, REG_SZ, Some(bytes)).is_ok();
        let _ = RegCloseKey(hk);
        ok
    }
}

/// Syncs Windows' native taskbar system tray clock (`Control Panel\International`)
/// between 24-hour (`HH:mm` / `HH:mm:ss`) and 12-hour (`h:mm tt` / `h:mm:ss tt`)
/// and notifies Explorer via `WM_SETTINGCHANGE("intl")` so the live taskbar clock updates immediately.
pub fn apply_win_clock_24h(use_24h: bool) {
    let intl_key = w!(r"Control Panel\International");
    let (short_t, long_t, itime) = if use_24h {
        ("HH:mm", "HH:mm:ss", "1")
    } else {
        ("h:mm tt", "h:mm:ss tt", "0")
    };
    let ok1 = write_hkcu_sz(intl_key, "sShortTime", short_t);
    let ok2 = write_hkcu_sz(intl_key, "sTimeFormat", long_t);
    let _ = write_hkcu_sz(intl_key, "iTime", itime);
    if ok1 || ok2 {
        unsafe {
            let intl_param = w!("intl");
            let _ = SendNotifyMessageW(
                HWND_BROADCAST,
                WM_SETTINGCHANGE,
                WPARAM(0),
                LPARAM(intl_param.as_ptr() as isize),
            );
            let main = FindWindowW(w!("Shell_TrayWnd"), PCWSTR::null()).unwrap_or_default();
            if !main.0.is_null() {
                let mut res = 0usize;
                let _ = windows::Win32::UI::WindowsAndMessaging::SendMessageTimeoutW(
                    main,
                    WM_SETTINGCHANGE,
                    WPARAM(0),
                    LPARAM(intl_param.as_ptr() as isize),
                    SMTO_ABORTIFHUNG,
                    150,
                    Some(&mut res),
                );
            }
        }
    }
}

pub fn ensure_windows_transparency_enabled() {
    let sub = w!(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize");
    if read_hkcu_dword(sub, "EnableTransparency", 1) == 0 {
        let _ = write_hkcu_dword(sub, "EnableTransparency", 1);
    }
}

/// Returns true when Windows 11 system UI (taskbar / system tray) is set to Light Mode
/// (`SystemUsesLightTheme != 0`), where native tray text & glyphs are dark instead of white.
pub fn windows_system_uses_light_theme() -> bool {
    let sub = w!(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize");
    read_hkcu_dword(sub, "SystemUsesLightTheme", 0) != 0
}

/// Watches Explorer's taskbar registry keys at 0% CPU and notifies `notify_hwnd` whenever
/// taskbar size, screen position, icon alignment, or auto-hide preferences change.
pub fn start_explorer_pref_watcher(notify_hwnd: windows::Win32::Foundation::HWND, notify_msg: u32) {
    use windows::Win32::Foundation::{CloseHandle, HANDLE};
    use windows::Win32::System::Registry::{RegNotifyChangeKeyValue, REG_NOTIFY_CHANGE_LAST_SET};
    use windows::Win32::System::Threading::{CreateEventW, WaitForMultipleObjects, INFINITE};
    use windows::Win32::UI::WindowsAndMessaging::PostMessageW;

    let hw_raw = notify_hwnd.0 as isize;
    std::thread::spawn(move || unsafe {
        let mut hk_adv = HKEY::default();
        let ok_adv = RegOpenKeyExW(HKEY_CURRENT_USER, REG_EXPLORER_ADV, None, KEY_READ, &mut hk_adv).is_ok();
        let mut hk_stuck = HKEY::default();
        let sub_stuck = w!(r"Software\Microsoft\Windows\CurrentVersion\Explorer\StuckRects3");
        let ok_stuck = RegOpenKeyExW(HKEY_CURRENT_USER, sub_stuck, None, KEY_READ, &mut hk_stuck).is_ok();
        let mut hk_taskband = HKEY::default();
        let sub_taskband = w!(r"Software\Microsoft\Windows\CurrentVersion\Explorer\Taskband");
        let ok_taskband = RegOpenKeyExW(HKEY_CURRENT_USER, sub_taskband, None, KEY_READ, &mut hk_taskband).is_ok();

        let ev_adv = if ok_adv { CreateEventW(None, false, false, PCWSTR::null()).ok() } else { None };
        let ev_stuck = if ok_stuck { CreateEventW(None, false, false, PCWSTR::null()).ok() } else { None };
        let ev_taskband = if ok_taskband { CreateEventW(None, false, false, PCWSTR::null()).ok() } else { None };

        let mut pairs: Vec<(HKEY, HANDLE)> = Vec::new();
        if let Some(e) = ev_adv {
            if RegNotifyChangeKeyValue(hk_adv, false, REG_NOTIFY_CHANGE_LAST_SET, Some(e), true).is_ok() {
                pairs.push((hk_adv, e));
            } else {
                let _ = CloseHandle(e);
            }
        }
        if let Some(e) = ev_stuck {
            if RegNotifyChangeKeyValue(hk_stuck, false, REG_NOTIFY_CHANGE_LAST_SET, Some(e), true).is_ok() {
                pairs.push((hk_stuck, e));
            } else {
                let _ = CloseHandle(e);
            }
        }
        if let Some(e) = ev_taskband {
            if RegNotifyChangeKeyValue(hk_taskband, false, REG_NOTIFY_CHANGE_LAST_SET, Some(e), true).is_ok() {
                pairs.push((hk_taskband, e));
            } else {
                let _ = CloseHandle(e);
            }
        }
        if pairs.is_empty() {
            if ok_adv { let _ = RegCloseKey(hk_adv); }
            if ok_stuck { let _ = RegCloseKey(hk_stuck); }
            if ok_taskband { let _ = RegCloseKey(hk_taskband); }
            return;
        }
        let handles: Vec<HANDLE> = pairs.iter().map(|p| p.1).collect();

        loop {
            let wait = WaitForMultipleObjects(&handles, false, INFINITE);
            let idx = wait.0 as usize;
            if idx >= pairs.len() {
                break;
            }
            // Coalesce burst registry writes (e.g. TaskbarSize + SmallIcons together)
            std::thread::sleep(std::time::Duration::from_millis(35));
            let _ = PostMessageW(
                Some(windows::Win32::Foundation::HWND(hw_raw as *mut std::ffi::c_void)),
                notify_msg,
                WPARAM(0),
                LPARAM(0),
            );
            for (i, &(hk, ev)) in pairs.iter().enumerate() {
                if i == idx
                    || windows::Win32::System::Threading::WaitForSingleObject(ev, 0)
                        == windows::Win32::Foundation::WAIT_OBJECT_0
                {
                    let _ = RegNotifyChangeKeyValue(hk, false, REG_NOTIFY_CHANGE_LAST_SET, Some(ev), true);
                }
            }
        }

        for &(_, ev) in &pairs {
            let _ = CloseHandle(ev);
        }
        if ok_adv { let _ = RegCloseKey(hk_adv); }
        if ok_stuck { let _ = RegCloseKey(hk_stuck); }
        if ok_taskband { let _ = RegCloseKey(hk_taskband); }
    });
}

/// Hints Windows to release cold one-time setup pages (shader JIT, shell COM, Studio UI)
/// back to the OS standby list when heavy UI or video pipelines idle or close.
pub fn trim_process_working_set() {
    unsafe {
        let proc_h = windows::Win32::System::Threading::GetCurrentProcess();
        let _ = windows::Win32::System::Memory::SetProcessWorkingSetSizeEx(
            proc_h,
            usize::MAX,
            usize::MAX,
            windows::Win32::System::Memory::SETPROCESSWORKINGSETSIZEEX_FLAGS(0),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_five_slot_card_migrates_to_six_slot_schema() {
        // Legacy pre-schema-6 INI (Card was stored at slot 4) auto-migrates Card to slot 5
        let legacy_ini = "\
[AutumnStudioV5]
music_elem_4=0,0,100,0,0x112233,100,1
cc_elem_4=0,0,100,0,0x445566,100,1
ext_elem_0_4=0,0,100,0,0x778899,100,1
";
        let migrated = parse_config_ini(legacy_ini).expect("legacy 5-slot migration");
        assert_eq!(migrated.music_elem_transforms[5].color_rgb, Some(0x112233));
        assert_eq!(migrated.music_elem_transforms[4].color_rgb, None);
        assert_eq!(migrated.cc_elem_transforms[5].color_rgb, Some(0x445566));
        assert_eq!(migrated.cc_elem_transforms[4].color_rgb, None);
        assert_eq!(migrated.ext_widgets[0].elem_transforms[5].color_rgb, Some(0x778899));
        assert_eq!(migrated.ext_widgets[0].elem_transforms[4].color_rgb, None);
    }

    #[test]
    fn realistic_suites_survive_atomic_save_and_reload() {
        let dir = std::env::temp_dir().join(format!(
            "autumn_cfg_roundtrip_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let ini_path = dir.join("config.ini");
        let bak_path = config_sibling_path(&ini_path, ".bak");

        let real_suites: Vec<_> = crate::suites::SUITES
            .iter()
            .filter(|s| s.category == crate::suites::SuiteCategory::Realistic)
            .collect();
        assert_eq!(real_suites.len(), 18, "expected all 18 Realistic suites");

        for suite in real_suites {
            let mut cfg = AppConfig::default();
            crate::suites::apply_suite_to_config(suite, &mut cfg);
            save_config_to_path(&cfg, &ini_path);
            assert!(ini_path.exists(), "primary config missing for {}", suite.id);
            assert!(bak_path.exists(), "backup config missing for {}", suite.id);

            let loaded = load_config_from_path(&ini_path);
            assert_eq!(loaded.active_suite, suite.id, "active_suite lost on {}", suite.id);
            assert_eq!(loaded.tb_theme, suite.tb_theme, "tb_theme lost on {}", suite.id);
            assert!(loaded.mac_dock_enabled, "mac_dock_enabled lost on {}", suite.id);
            assert_eq!(loaded.mode, cfg.mode, "mode lost on {}", suite.id);
            assert_eq!(loaded.mac_dock_edge, cfg.mac_dock_edge, "dock_edge lost on {}", suite.id);
            assert_eq!(loaded.dt_style, cfg.dt_style, "dt_style lost on {}", suite.id);
            assert_eq!(loaded.music_style, cfg.music_style, "music_style lost on {}", suite.id);
            assert_eq!(loaded.ext_widgets[0].enabled, cfg.ext_widgets[0].enabled);
        }

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn corrupt_or_truncated_config_recovers_realistic_suite_from_backup() {
        let dir = std::env::temp_dir().join(format!(
            "autumn_cfg_bak_recover_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let ini_path = dir.join("config.ini");

        let suite = crate::suites::find_suite_by_id("real_crystal_water")
            .expect("real_crystal_water suite");
        let mut cfg = AppConfig::default();
        crate::suites::apply_suite_to_config(suite, &mut cfg);
        save_config_to_path(&cfg, &ini_path);

        // Simulate 0-byte truncation during mid-shutdown crash
        fs::write(&ini_path, "").unwrap();
        let recovered_empty = load_config_from_path(&ini_path);
        assert_eq!(recovered_empty.active_suite, "real_crystal_water");
        assert_eq!(recovered_empty.tb_theme, suite.tb_theme);
        assert!(recovered_empty.mac_dock_enabled);
        assert!(has_valid_config_header(&fs::read_to_string(&ini_path).unwrap()));

        // Simulate partial garbage truncation without [AutumnStudioV5] header
        fs::write(&ini_path, "mode=always\ntb_theme=broken_partial\n").unwrap();
        let recovered_corrupt = load_config_from_path(&ini_path);
        assert_eq!(recovered_corrupt.active_suite, "real_crystal_water");
        assert_eq!(recovered_corrupt.tb_theme, suite.tb_theme);
        assert!(recovered_corrupt.mac_dock_enabled);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn always_visible_defaults_and_cross_folder_edge_hover_sync() {
        // 1. Fresh default config uses AlwaysVisible for both native taskbar and virtual docks,
        // and keeps Reclaim RAM When Covered OFF by default for instant resume & Win11 26H2 continuity
        let def = AppConfig::default();
        assert_eq!(def.mode, TaskbarMode::AlwaysVisible);
        assert_eq!(def.mac_dock_hide, MacDockHideMode::AlwaysVisible);
        assert!(!def.wallpaper_trim_memory, "Reclaim RAM When Covered must default OFF");

        // 2. All 54 Suite Gallery presets default to AlwaysVisible
        for s in &crate::suites::SUITES {
            assert_eq!(s.tb_mode, TaskbarMode::AlwaysVisible, "Suite '{}' tb_mode must be AlwaysVisible", s.id);
            assert_eq!(s.dock_hide, MacDockHideMode::AlwaysVisible, "Suite '{}' dock_hide must be AlwaysVisible", s.id);
        }

        // 3. Edge Hover Reveal carries seamlessly from Realistic/Interactive dock to ALL 5 native folders and back
        let mut cfg = AppConfig::default();
        cfg.apply_taskbar_theme_preset(TaskbarTheme::RealisticWaterDock);
        assert!(cfg.mac_dock_enabled);
        cfg.mac_dock_hide = MacDockHideMode::HoverReveal;
        cfg.mode = TaskbarMode::StealthHide; // simulate prior desynced config

        for native_theme in [
            TaskbarTheme::CupertinoDock,
            TaskbarTheme::KineticCupertinoWave,
            TaskbarTheme::HoverInferno,
            TaskbarTheme::AutumnCrystal,
            TaskbarTheme::BassPulseSplit,
        ] {
            let mut c = cfg.clone();
            c.apply_taskbar_theme_preset(native_theme);
            assert!(!c.mac_dock_enabled);
            assert_eq!(
                c.mode,
                TaskbarMode::HoverReveal,
                "Switching from virtual dock HoverReveal to {:?} must enable native Edge Hover Reveal",
                native_theme
            );
            // Switching back to an Interactive Dock preserves HoverReveal too
            c.apply_taskbar_theme_preset(TaskbarTheme::AuroraRibbonDock);
            assert!(c.mac_dock_enabled);
            assert_eq!(c.mac_dock_hide, MacDockHideMode::HoverReveal);
        }

        // 4. Full Stealth Hide syncs cleanly between virtual docks and native taskbar + INI round-trip
        let mut stealth_cfg = AppConfig::default();
        stealth_cfg.apply_taskbar_theme_preset(TaskbarTheme::RealisticWaterDock);
        stealth_cfg.mac_dock_hide = MacDockHideMode::StealthHide;
        stealth_cfg.sync_hide_mode_from_dock();
        assert_eq!(stealth_cfg.mode, TaskbarMode::StealthHide);
        let ini = serialize_config_ini(&stealth_cfg);
        assert!(ini.contains("mac_dock_hide=stealth\n"));
        let parsed = parse_config_body(&ini, true).0;
        assert_eq!(parsed.mac_dock_hide, MacDockHideMode::StealthHide);
        assert_eq!(parsed.mode, TaskbarMode::StealthHide);

        // 5. Unified 24-hour clock format helper syncs widgets_24h and w_clock.use_24h
        stealth_cfg.set_clock_24h(true);
        assert!(stealth_cfg.widgets_24h);
        assert_eq!(stealth_cfg.w_clock.use_24h, Some(true));
        stealth_cfg.set_clock_24h(false);
        assert!(!stealth_cfg.widgets_24h);
        assert_eq!(stealth_cfg.w_clock.use_24h, Some(false));
    }

    #[test]
    fn anim_fps_defaults_45_cadence_and_ini_roundtrip() {
        let mut cfg = AppConfig::default();
        assert_eq!(cfg.anim_fps, 45, "Animation FPS must default to 45 FPS");
        assert!(!cfg.viz_eco_fps, "45 FPS default is above Eco 24 FPS threshold");
        assert_eq!(cfg.anim_frame_ms(), 22);
        assert_eq!(cfg.viz_audio_frame_ms(), 22);
        assert_eq!(cfg.viz_island_frame_ms(), 22);

        // Verify max(8, 1000 / anim_fps) formula across all presets
        assert_eq!(AppConfig::anim_frame_ms_for(24), 41);
        assert_eq!(AppConfig::anim_frame_ms_for(30), 33);
        assert_eq!(AppConfig::anim_frame_ms_for(45), 22);
        assert_eq!(AppConfig::anim_frame_ms_for(60), 16);
        assert_eq!(AppConfig::anim_frame_ms_for(90), 11);
        assert_eq!(AppConfig::anim_frame_ms_for(120), 8);

        cfg.set_anim_fps(24);
        assert!(cfg.viz_eco_fps);
        assert_eq!(cfg.anim_frame_ms(), 41);
        let ini_24 = serialize_config_ini(&cfg);
        assert!(ini_24.contains("[General]\nanim_fps=24\n"));
        let parsed_24 = parse_config_body(&ini_24, true).0;
        assert_eq!(parsed_24.anim_fps, 24);
        assert!(parsed_24.viz_eco_fps);

        cfg.set_anim_fps(120);
        assert!(!cfg.viz_eco_fps);
        assert_eq!(cfg.anim_frame_ms(), 8);
        let ini_120 = serialize_config_ini(&cfg);
        assert!(ini_120.contains("[General]\nanim_fps=120\n"));
        let parsed_120 = parse_config_body(&ini_120, true).0;
        assert_eq!(parsed_120.anim_fps, 120);
        assert!(!parsed_120.viz_eco_fps);

        // Legacy config without anim_fps migrates cleanly to 45 FPS default
        let legacy = "[AutumnStudioV5]\nviz_eco_fps=1\n";
        let parsed_legacy = parse_config_body(legacy, true).0;
        assert_eq!(parsed_legacy.anim_fps, 45);
        assert!(!parsed_legacy.viz_eco_fps);
    }

    #[test]
    fn export_and_import_setup_roundtrip_validation_and_wallpaper_fallback() {
        let dir = std::env::temp_dir().join(format!(
            "autumn_setup_export_import_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let setup_file = dir.join("my_setup.autumn");
        let fake_local_wp = dir.join("local_wp.png");
        fs::write(&fake_local_wp, b"PNG").unwrap();

        let mut exported = AppConfig::default();
        exported.active_suite = "slate_monochrome".to_string();
        exported.tb_theme = TaskbarTheme::KatanaGlassDock;
        exported.mac_dock_enabled = true;
        exported.widgets_24h = true;
        exported.set_anim_fps(60);
        exported.game_mode_pause = false;
        exported.wallpaper_enabled = true;
        exported.wallpaper_path = r"Z:\nonexistent_other_pc\missing.mp4".to_string();
        exported.wallpaper_pause_maximized = false;
        exported.wallpaper_trim_memory = true;

        export_setup_to_path(&exported, &setup_file).expect("export_setup_to_path must succeed");
        assert!(setup_file.is_file());

        let mut current = AppConfig::default();
        current.wallpaper_path = fake_local_wp.to_string_lossy().to_string();

        let imported = import_setup_from_path(&setup_file, &current)
            .expect("import_setup_from_path must succeed");
        assert_eq!(imported.active_suite, "slate_monochrome");
        assert_eq!(imported.tb_theme, TaskbarTheme::KatanaGlassDock);
        assert!(imported.mac_dock_enabled);
        assert!(imported.widgets_24h);
        assert_eq!(imported.anim_fps, 60);
        assert!(!imported.viz_eco_fps);
        assert!(!imported.game_mode_pause);
        assert!(!imported.wallpaper_pause_maximized);
        assert!(imported.wallpaper_trim_memory);
        assert!(imported.onboarding_done);
        assert_eq!(
            imported.wallpaper_path, current.wallpaper_path,
            "Missing cross-PC wallpaper path must preserve local machine wallpaper"
        );

        // Invalid setup file must be rejected cleanly
        let bad_file = dir.join("not_a_setup.txt");
        fs::write(&bad_file, "hello world\nnot an autumn setup\n").unwrap();
        assert!(import_setup_from_path(&bad_file, &current).is_err());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn viz_color_mode_and_accent_ini_roundtrip() {
        let mut cfg = AppConfig::default();
        cfg.apply_taskbar_theme_preset(TaskbarTheme::LiveMusicBars);
        assert_eq!(cfg.tb_viz_color_mode, VizColorMode::ThemeNative);
        assert!(!cfg.tb_match_wallpaper);

        cfg.set_tb_viz_color_mode(VizColorMode::CustomAccent);
        cfg.tb_viz_accent_rgb = 0xFF38A8;
        let ini = serialize_config_ini(&cfg);
        let (parsed, _, _, _) = parse_config_body(&ini, true);
        assert_eq!(parsed.tb_viz_color_mode, VizColorMode::CustomAccent);
        assert_eq!(parsed.tb_viz_accent_rgb, 0xFF38A8);
        assert!(!parsed.tb_match_wallpaper);

        cfg.set_tb_viz_color_mode(VizColorMode::MatchWallpaper);
        assert!(cfg.tb_match_wallpaper);
        let ini_wp = serialize_config_ini(&cfg);
        let (parsed_wp, _, _, _) = parse_config_body(&ini_wp, true);
        assert_eq!(parsed_wp.tb_viz_color_mode, VizColorMode::MatchWallpaper);
        assert!(parsed_wp.tb_match_wallpaper);
    }

    #[test]
    fn fresh_install_starts_clean_without_crowded_widgets() {
        let def = AppConfig::default();
        assert!(def.show_datetime, "Fresh install should show the signature clock");
        assert!(!def.show_apps, "Fresh install must not crowd desktop with Quick Apps");
        assert!(!def.show_vitals, "Fresh install must not crowd desktop with Vitals");
        assert!(!def.show_music, "Fresh install must not crowd desktop with Now Playing");
        assert!(!def.show_cc, "Fresh install must not crowd desktop with Control Center");
        assert!(
            def.ext_widgets.iter().all(|w| !w.enabled),
            "Fresh install must not enable extended widgets by default"
        );
    }

    #[test]
    fn async_config_save_coalesces_bursts_and_writes_latest_valid_ini() {
        let mut dir = std::env::temp_dir();
        dir.push(format!(
            "autumndeck_async_save_test_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let _ = fs::create_dir_all(&dir);
        let path = dir.join("config.ini");

        let mut cfg = AppConfig::default();
        for i in 0..20u8 {
            cfg.tint_alpha = 50 + i * 5;
            cfg.mac_dock_icon_size = 28 + (i % 16);
            save_config_to_path_async(&cfg, &path);
        }

        let bak_path = config_sibling_path(&path, ".bak");
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let mut last_parsed = None;
        while std::time::Instant::now() < deadline {
            if let Ok(body) = fs::read_to_string(&path) {
                let (parsed, _, _, _) = parse_config_body(&body, true);
                if parsed.tint_alpha == cfg.tint_alpha
                    && parsed.mac_dock_icon_size == cfg.mac_dock_icon_size
                    && bak_path.is_file()
                {
                    last_parsed = Some(parsed);
                    break;
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }

        // Wait for any in-flight worker write holding SAVE_IO_LOCK before deleting temp dir.
        let _io_guard = SAVE_IO_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let parsed = last_parsed.expect("async background worker must flush latest coalesced config");
        assert_eq!(parsed.tint_alpha, cfg.tint_alpha);
        assert_eq!(parsed.mac_dock_icon_size, cfg.mac_dock_icon_size);
        assert!(bak_path.is_file(), "backup config.ini.bak must also be written");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn clean_minimal_20_themes_roundtrip_virtual_dock_and_presets() {
        assert_eq!(TaskbarTheme::CLEAN_MINIMAL_FOLDER.len(), 20);
        assert_eq!(TaskbarTheme::VIRTUAL_DOCK_THEMES.len(), 58);
        assert_eq!(TaskbarTheme::ALL.len(), 90);
        assert_eq!(TaskbarThemeFolder::CURATED.len(), 8);
        assert_eq!(TaskbarThemeFolder::CleanMinimal.label(), "Clean & Minimal (20)");
        assert_eq!(TaskbarThemeFolder::All.label(), "All Themes (90)");

        let mut seen_keys = std::collections::HashSet::new();
        for &th in &TaskbarTheme::CLEAN_MINIMAL_FOLDER {
            assert!(th.is_clean_minimal());
            assert!(
                th.uses_virtual_dock(),
                "Clean & Minimal theme {:?} must use the animated Virtual Dock engine",
                th
            );
            assert!(
                TaskbarTheme::VIRTUAL_DOCK_THEMES.contains(&th),
                "Clean & Minimal theme {:?} must be registered in VIRTUAL_DOCK_THEMES",
                th
            );
            assert!(
                crate::dock_themes::dock_palette(th).is_some(),
                "Clean & Minimal theme {:?} must have a curated dock_palette",
                th
            );
            let spec = crate::dock_themes::dock_spec(th);
            assert_eq!(
                spec.light_mode_paper,
                th.is_light_surface(),
                "Clean & Minimal theme {:?} light_mode_paper must match is_light_surface",
                th
            );
            if th.is_independent_tile_dock() || th == TaskbarTheme::BorderlessGhost {
                assert_eq!(
                    spec.layout,
                    DockLayoutKind::IndependentOrbs,
                    "Zero-bar Clean & Minimal theme {:?} must use IndependentOrbs layout",
                    th
                );
                assert_eq!(spec.shape, DockShelfShape::FloatingZero);
            }
            let key = th.as_str();
            assert!(seen_keys.insert(key), "Duplicate theme key {}", key);
            assert_eq!(TaskbarTheme::from_str(key), th);
            assert!(!th.label().is_empty());
            assert!(!th.badge().is_empty());
            assert!(!th.subtitle().is_empty());

            let mut cfg = AppConfig::default();
            cfg.apply_taskbar_theme_preset(th);
            assert_eq!(cfg.tb_theme, th);
            assert!(cfg.mac_dock_enabled, "Clean & Minimal theme {:?} must activate mac_dock_enabled", th);
            assert_eq!(
                cfg.tb_viz_style,
                TaskbarVizStyle::Off,
                "Clean & Minimal theme {:?} must keep viz Off for 0% idle CPU",
                th
            );
            if th == TaskbarTheme::LuminosityFloat {
                assert!(cfg.tb_match_wallpaper);
            }
            let ini = serialize_config_ini(&cfg);
            let (parsed, _, _, _) = parse_config_body(&ini, true);
            assert_eq!(parsed.tb_theme, th);
        }

        // Verify Light themes, Icon-Pad themes, and Zero-Bar Independent Tile themes
        assert!(TaskbarTheme::Os26LiquidGlassLight.is_light_surface());
        assert!(TaskbarTheme::MatterTilesLight.is_light_surface());
        assert!(TaskbarTheme::DockLikeLight.is_light_surface());
        assert!(TaskbarTheme::BlobLight.is_light_surface());
        assert!(TaskbarTheme::AerisGlassLight.is_light_surface());
        assert!(!TaskbarTheme::Os26LiquidGlassDark.is_light_surface());
        assert!(!TaskbarTheme::MatterTilesDark.is_light_surface());

        assert!(TaskbarTheme::Os26LiquidGlassDark.uses_icon_pads());
        assert!(TaskbarTheme::MatterTilesDark.uses_icon_pads());
        assert!(TaskbarTheme::MatterTilesLight.uses_icon_pads());
        assert!(TaskbarTheme::BubblesDark.uses_icon_pads());
        assert!(TaskbarTheme::OneUiLightChips.uses_icon_pads());
        assert!(TaskbarTheme::RosePineTiles.uses_icon_pads());
        assert!(!TaskbarTheme::BorderlessGhost.uses_icon_pads());

        assert!(TaskbarTheme::MatterTilesDark.is_independent_tile_dock());
        assert!(TaskbarTheme::MatterTilesLight.is_independent_tile_dock());
        assert!(TaskbarTheme::SquircleFrosted.is_independent_tile_dock());
        assert!(TaskbarTheme::BubblesDark.is_independent_tile_dock());
        assert!(TaskbarTheme::PillsDetached.is_independent_tile_dock());
        assert!(TaskbarTheme::RosePineTiles.is_independent_tile_dock());
        assert!(!TaskbarTheme::Os26LiquidGlassDark.is_independent_tile_dock());
    }

    #[test]
    fn portable_mode_paths_conversion_and_32_pin_roundtrip() {
        let base = std::env::temp_dir().join(format!(
            "autumn_portable_test_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&base);
        let exe_dir = base.join("AppBin");
        let fake_appdata = base.join("FakeAppData");
        let src_data = fake_appdata.join("AutumnDeck");
        fs::create_dir_all(&exe_dir).unwrap();
        fs::create_dir_all(src_data.join("Themes")).unwrap();
        fs::create_dir_all(src_data.join("wallpapers")).unwrap();
        fs::create_dir_all(src_data.join("Diagnostics")).unwrap();
        fs::write(src_data.join("Themes").join("custom.ini"), "ok=1\n").unwrap();
        fs::write(src_data.join("wallpapers").join("sample.mp4"), b"vid").unwrap();
        fs::write(src_data.join("pill_cache.ini"), "cache\n").unwrap();

        // Standard mode (no portable.txt, no --portable) uses APPDATA\AutumnDeck
        let (std_dir, std_port) =
            resolve_app_data_dir(Some(&exe_dir), false, Some(&fake_appdata));
        assert!(!std_port);
        assert_eq!(std_dir, src_data);

        // --portable CLI flag activates <exe_dir>\Data without touching APPDATA
        let (cli_dir, cli_port) =
            resolve_app_data_dir(Some(&exe_dir), true, Some(&fake_appdata));
        assert!(cli_port);
        assert_eq!(cli_dir, exe_dir.join("Data"));

        // Convert to Portable creates portable.txt, copies subdirs, rewrites wallpaper path, and persists 32 pins + 48px macOS max icons + 18px label font + card opacity
        let mut cfg = AppConfig::default();
        assert_eq!(cfg.mac_dock_icon_size, 36);
        assert_eq!(cfg.w_apps.card_opacity_pct, 100);
        cfg.mac_dock_icon_size = 48;
        cfg.w_apps.label_font_size_px = 18;
        cfg.w_apps.card_opacity_pct = 0;
        cfg.wallpaper_path = src_data
            .join("wallpapers")
            .join("sample.mp4")
            .to_string_lossy()
            .into_owned();
        cfg.mac_dock_pinned = (0..MAX_PINNED_APPS)
            .map(|i| AppShortcut {
                name: format!("App {:02}", i),
                path: format!(r"C:\Tools\app_{:02}.exe", i),
                offset_x: 0,
                offset_y: 0,
            })
            .collect();
        assert_eq!(MAX_PINNED_APPS, 32);

        let dst_dir = convert_to_portable_in(&exe_dir, &src_data, &cfg)
            .expect("convert_to_portable_in must succeed");
        assert_eq!(dst_dir, exe_dir.join("Data"));
        assert!(exe_dir.join("portable.txt").is_file());
        assert!(dst_dir.join("config.ini").is_file());
        assert!(dst_dir.join("config.ini.bak").is_file());
        assert!(dst_dir.join("Themes").join("custom.ini").is_file());
        assert!(dst_dir.join("wallpapers").join("sample.mp4").is_file());
        assert!(dst_dir.join("pill_cache.ini").is_file());

        // Now resolve_app_data_dir automatically detects portable.txt without CLI flag
        let (auto_dir, auto_port) =
            resolve_app_data_dir(Some(&exe_dir), false, Some(&fake_appdata));
        assert!(auto_port);
        assert_eq!(auto_dir, dst_dir);

        // Verify saved portable config.ini roundtrips all 32 pins, 48px icon size, 18px label font, 0% card opacity, and rewritten wallpaper path
        let raw_ini = fs::read_to_string(dst_dir.join("config.ini")).unwrap();
        let parsed = parse_config_ini(&raw_ini).expect("portable config.ini must parse");
        assert_eq!(parsed.mac_dock_icon_size, 48);
        assert_eq!(parsed.w_apps.label_font_size_px, 18);
        assert_eq!(parsed.w_apps.card_opacity_pct, 0);
        assert_eq!(parsed.mac_dock_pinned.len(), 32);
        assert_eq!(parsed.mac_dock_pinned[31].name, "App 31");
        assert!(
            parsed
                .wallpaper_path
                .starts_with(&dst_dir.to_string_lossy().to_string()),
            "Portable conversion must rewrite cached wallpaper path into portable Data folder: {}",
            parsed.wallpaper_path
        );

        // Clamp guard verification for macOS icon size (24..=48), shelf/card opacity (0..=100), and label font size (10..=20)
        let clamp_ini = "[Taskbar]\nmac_dock_icon_size=140\nw_apps_label_font_size=28\nw_apps_card_opacity=220\nw_shelf_glass_pct=150\n";
        let (clamped_hi, _, _, _) = parse_config_body(clamp_ini, true);
        assert_eq!(clamped_hi.mac_dock_icon_size, 48);
        assert_eq!(clamped_hi.w_apps.label_font_size_px, 20);
        assert_eq!(clamped_hi.w_apps.card_opacity_pct, 100);
        assert_eq!(clamped_hi.w_shelf.glass_alpha_pct, 100);
        let clamp_lo_ini = "[Taskbar]\nmac_dock_icon_size=12\nw_apps_label_font_size=4\nw_apps_card_opacity=45\nw_shelf_glass_pct=0\nmac_dock_enabled=1\nmac_dock_hide=smart\n";
        let (clamped_lo, _, _, _) = parse_config_body(clamp_lo_ini, true);
        assert_eq!(clamped_lo.mac_dock_icon_size, 24);
        assert_eq!(clamped_lo.w_apps.label_font_size_px, 10);
        assert_eq!(clamped_lo.w_apps.card_opacity_pct, 45);
        assert_eq!(clamped_lo.w_shelf.glass_alpha_pct, 0);
        assert_eq!(
            clamped_lo.mac_dock_hide,
            MacDockHideMode::AlwaysVisible,
            "Legacy SmartAutoHide must coerce cleanly to AlwaysVisible on Dock themes"
        );

        // 3-State Bar / Top Bar gating: Bar OFF locks Top Bar OFF; Bar ON unlocks Top Bar ON/OFF independently
        let mut u_cfg = AppConfig::default();
        u_cfg.apply_taskbar_theme_preset(TaskbarTheme::QuartzShellDock);
        assert!(!u_cfg.is_dock_bar_on(), "Non-Clean docks default to Bar OFF (all-in-one dock)");
        assert!(!u_cfg.is_clean_top_bar_active());
        u_cfg.dock_bar_enabled = true;
        assert!(u_cfg.is_dock_bar_on());
        assert!(!u_cfg.is_clean_top_bar_active());
        u_cfg.clean_top_bar_enabled = true;
        assert!(u_cfg.is_clean_top_bar_active());
        u_cfg.dock_bar_enabled = false;
        assert!(!u_cfg.is_dock_bar_on());
        assert!(!u_cfg.is_clean_top_bar_active());
        u_cfg.dock_bar_enabled = true;
        assert!(u_cfg.is_dock_bar_on());
        assert!(u_cfg.is_clean_top_bar_active());
        u_cfg.mac_dock_hide = MacDockHideMode::StealthHide;
        assert!(!u_cfg.is_clean_top_bar_active(), "StealthHide hides Top Bar too");
        u_cfg.mac_dock_hide = MacDockHideMode::AlwaysVisible;
        u_cfg.mac_dock_edge = MacDockEdge::Bottom;
        u_cfg.set_clean_top_bar_edge(CleanTopBarEdge::Bottom);
        assert_eq!(u_cfg.mac_dock_edge, MacDockEdge::Top);

        let _ = fs::remove_dir_all(&base);
    }
}

