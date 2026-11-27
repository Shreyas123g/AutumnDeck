use std::ffi::c_void;
use std::mem::size_of;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

use windows::core::{w, Interface, PCWSTR};
use windows::Win32::Foundation::{CloseHandle, HGLOBAL};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D1_FIGURE_BEGIN_HOLLOW, D2D1_FIGURE_END_OPEN, D2D1_GRADIENT_STOP, D2D_RECT_F, D2D_SIZE_F,
};
use windows::Win32::Graphics::Direct2D::{
    ID2D1RenderTarget, ID2D1Factory, ID2D1PathGeometry, D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
    D2D1_ARC_SEGMENT, D2D1_ARC_SIZE_LARGE, D2D1_ARC_SIZE_SMALL,
    D2D1_ELLIPSE, D2D1_ROUNDED_RECT, D2D1_SWEEP_DIRECTION_CLOCKWISE,
};
use windows::Win32::Graphics::DirectWrite::{
    IDWriteFactory, IDWriteFontCollection, DWRITE_FONT_WEIGHT_BOLD, DWRITE_FONT_WEIGHT_MEDIUM,
    DWRITE_FONT_WEIGHT_REGULAR, DWRITE_FONT_WEIGHT_SEMI_BOLD,
    DWRITE_PARAGRAPH_ALIGNMENT_CENTER, DWRITE_TEXT_ALIGNMENT_CENTER, DWRITE_TEXT_ALIGNMENT_LEADING,
    DWRITE_TEXT_ALIGNMENT_TRAILING,
};
use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory1, IDXGIAdapter3, IDXGIFactory4, DXGI_MEMORY_SEGMENT_GROUP_LOCAL,
    DXGI_QUERY_VIDEO_MEMORY_INFO,
};
use windows::Win32::Storage::FileSystem::{GetDiskFreeSpaceExW, GetDriveTypeW, GetLogicalDrives};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
use windows::Win32::System::Performance::{
    PdhAddEnglishCounterW, PdhCloseQuery, PdhCollectQueryData, PdhGetFormattedCounterValue,
    PdhOpenQueryW, PDH_FMT_COUNTERVALUE, PDH_FMT_DOUBLE, PDH_HCOUNTER, PDH_HQUERY,
};
use windows::Win32::System::ProcessStatus::{K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
use windows::Win32::System::SystemInformation::{GetSystemTime, GetTickCount64};
use windows::Win32::System::Threading::{
    GetCurrentThread, OpenProcess, SetThreadPriority, TerminateProcess,
    PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_TERMINATE, PROCESS_VM_READ,
    THREAD_PRIORITY_BELOW_NORMAL,
};
use windows::Win32::UI::Shell::{
    SHEmptyRecycleBinW, SHQueryRecycleBinW, ShellExecuteW, SHERB_NOCONFIRMATION,
    SHERB_NOPROGRESSUI, SHQUERYRBINFO,
};
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
use windows_numerics::Vector2;

use crate::config::{AppConfig, ClockAlign, ClockElemTransform, TaskbarVizStyle};
use crate::desktop_widgets::{
    begin_elem_xf_slot, chrono_local, create_linear_grad, draw_shadowed_text, end_elem_xf_slot,
    grad_endpoints_for_rect, make_elem_brush, make_resolved_tf_named, record_elem_box_slot,
    resolve_elem_grad_colors, rgba_u8, ClockElemBox, FgKeyHasher, WidgetFgCacheSlot,
    WidgetHitZone, PT_TO_PX,
};
use crate::system_media::{AudioOutputDevice, CcSnapshot};
use crate::taskbar_styler::{paint_widget_elemental_fx, VizSnapshot};
use crate::theme::{resolve_palette_for, u32_to_rgb, ThemePalette};

#[link(name = "user32")]
extern "system" {
    fn GetClipboardSequenceNumber() -> u32;
    fn OpenClipboard(hwnd_new_owner: *mut c_void) -> i32;
    fn CloseClipboard() -> i32;
    fn GetClipboardData(u_format: u32) -> *mut c_void;
    fn EmptyClipboard() -> i32;
    fn SetClipboardData(u_format: u32, h_mem: *mut c_void) -> *mut c_void;
}

const CF_UNICODETEXT_FMT: u32 = 13;
pub const EXT_WIDGET_COUNT: usize = 14;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(usize)]
pub enum ExtWidgetId {
    Weather = 0,
    SearchBar = 1,
    Greeting = 2,
    Network = 3,
    Gpu = 4,
    AudioSwitcher = 5,
    Notes = 6,
    Calendar = 7,
    Pomodoro = 8,
    WorldClock = 9,
    Clipboard = 10,
    Drives = 11,
    Processes = 12,
    RecycleBin = 13,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GalleryCategory {
    All,
    CoreMedia,
    WeatherSearch,
    Productivity,
    HardwareSys,
}

impl GalleryCategory {
    pub const ALL: [GalleryCategory; 5] = [
        GalleryCategory::All,
        GalleryCategory::CoreMedia,
        GalleryCategory::WeatherSearch,
        GalleryCategory::Productivity,
        GalleryCategory::HardwareSys,
    ];

    pub fn label(self) -> &'static str {
        match self {
            GalleryCategory::All => "All Widgets (19)",
            GalleryCategory::CoreMedia => "Core & Media (5)",
            GalleryCategory::WeatherSearch => "Weather & Search (3)",
            GalleryCategory::Productivity => "Productivity (5)",
            GalleryCategory::HardwareSys => "Hardware & System (6)",
        }
    }
}

impl ExtWidgetId {
    pub const ALL: [ExtWidgetId; EXT_WIDGET_COUNT] = [
        ExtWidgetId::Weather,
        ExtWidgetId::SearchBar,
        ExtWidgetId::Greeting,
        ExtWidgetId::Network,
        ExtWidgetId::Gpu,
        ExtWidgetId::AudioSwitcher,
        ExtWidgetId::Notes,
        ExtWidgetId::Calendar,
        ExtWidgetId::Pomodoro,
        ExtWidgetId::WorldClock,
        ExtWidgetId::Clipboard,
        ExtWidgetId::Drives,
        ExtWidgetId::Processes,
        ExtWidgetId::RecycleBin,
    ];

    pub fn idx(self) -> usize {
        self as usize
    }

    pub fn from_idx(i: usize) -> Option<Self> {
        Self::ALL.get(i).copied()
    }

    pub fn title(self) -> &'static str {
        match self {
            ExtWidgetId::Weather => "Live Weather & Forecast",
            ExtWidgetId::SearchBar => "Desktop Search Bar",
            ExtWidgetId::Greeting => "Daily Greeting & Quote",
            ExtWidgetId::Network => "Network Speedometer",
            ExtWidgetId::Gpu => "GPU & Gaming Telemetry",
            ExtWidgetId::AudioSwitcher => "Audio Output Switcher",
            ExtWidgetId::Notes => "Sticky Notes & To-Do",
            ExtWidgetId::Calendar => "Monthly Calendar & Agenda",
            ExtWidgetId::Pomodoro => "Pomodoro Focus Timer",
            ExtWidgetId::WorldClock => "Multi-Timezone World Clock",
            ExtWidgetId::Clipboard => "Clipboard History Pill",
            ExtWidgetId::Drives => "Drive & Storage Rings",
            ExtWidgetId::Processes => "Top Process Killer",
            ExtWidgetId::RecycleBin => "Trash & Recycle Cleaner",
        }
    }

    pub fn short_title(self) -> &'static str {
        match self {
            ExtWidgetId::Weather => "Weather",
            ExtWidgetId::SearchBar => "Search Bar",
            ExtWidgetId::Greeting => "Daily Quote",
            ExtWidgetId::Network => "Network Speed",
            ExtWidgetId::Gpu => "GPU Telemetry",
            ExtWidgetId::AudioSwitcher => "Audio Switcher",
            ExtWidgetId::Notes => "Sticky Notes",
            ExtWidgetId::Calendar => "Calendar",
            ExtWidgetId::Pomodoro => "Pomodoro",
            ExtWidgetId::WorldClock => "World Clock",
            ExtWidgetId::Clipboard => "Clipboard",
            ExtWidgetId::Drives => "Disk Drives",
            ExtWidgetId::Processes => "Process Killer",
            ExtWidgetId::RecycleBin => "Recycle Bin",
        }
    }

    pub fn category(self) -> GalleryCategory {
        match self {
            ExtWidgetId::Weather | ExtWidgetId::SearchBar | ExtWidgetId::Greeting => {
                GalleryCategory::WeatherSearch
            }
            ExtWidgetId::Notes
            | ExtWidgetId::Calendar
            | ExtWidgetId::Pomodoro
            | ExtWidgetId::WorldClock
            | ExtWidgetId::Clipboard => GalleryCategory::Productivity,
            ExtWidgetId::Network
            | ExtWidgetId::Gpu
            | ExtWidgetId::AudioSwitcher
            | ExtWidgetId::Drives
            | ExtWidgetId::Processes
            | ExtWidgetId::RecycleBin => GalleryCategory::HardwareSys,
        }
    }

    pub fn default_pos(self) -> (i32, i32) {
        match self {
            ExtWidgetId::Weather => (58, 300),
            ExtWidgetId::SearchBar => (392, 36),
            ExtWidgetId::Greeting => (392, 98),
            ExtWidgetId::Network => (58, 460),
            ExtWidgetId::Gpu => (58, 590),
            ExtWidgetId::AudioSwitcher => (920, 36),
            ExtWidgetId::Notes => (392, 178),
            ExtWidgetId::Calendar => (392, 366),
            ExtWidgetId::Pomodoro => (392, 574),
            ExtWidgetId::WorldClock => (666, 178),
            ExtWidgetId::Clipboard => (666, 596),
            ExtWidgetId::Drives => (666, 314),
            ExtWidgetId::Processes => (666, 452),
            ExtWidgetId::RecycleBin => (920, 166),
        }
    }

    pub fn base_size(self, compact: bool) -> (f32, f32) {
        let (w, h): (f32, f32) = match self {
            ExtWidgetId::Weather => (310.0, 142.0),
            ExtWidgetId::SearchBar => (440.0, 50.0),
            ExtWidgetId::Greeting => (440.0, 66.0),
            ExtWidgetId::Network => (310.0, 112.0),
            ExtWidgetId::Gpu => (310.0, 112.0),
            ExtWidgetId::AudioSwitcher => (336.0, 114.0),
            ExtWidgetId::Notes => (256.0, 172.0),
            ExtWidgetId::Calendar => (256.0, 192.0),
            ExtWidgetId::Pomodoro => (256.0, 128.0),
            ExtWidgetId::WorldClock => (238.0, 122.0),
            ExtWidgetId::Clipboard => (238.0, 114.0),
            ExtWidgetId::Drives => (238.0, 122.0),
            ExtWidgetId::Processes => (238.0, 128.0),
            ExtWidgetId::RecycleBin => (336.0, 92.0),
        };
        if compact {
            (w * 0.92, (h * 0.82).max(42.0))
        } else {
            (w, h)
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ExtThemeFolder {
    Horology,
    RealisticMaterials,
    InteractiveElements,
    GlassMinimal,
    CyberEditorial,
    All,
}

impl ExtThemeFolder {
    pub fn label(self) -> &'static str {
        match self {
            ExtThemeFolder::Horology => "Photorealistic Horology (10)",
            ExtThemeFolder::RealisticMaterials => "Realistic (12)",
            ExtThemeFolder::InteractiveElements => "Interactive Physics (6)",
            ExtThemeFolder::GlassMinimal => "Glass & Minimal (4)",
            ExtThemeFolder::CyberEditorial => "Cyber & Editorial (3)",
            ExtThemeFolder::All => "All Themes (35)",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ExtWidgetTheme {
    // 10 Photorealistic Haute Horlogerie Bespoke Instrument Themes
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
    // 12 Realistic Materials, Water & Calm PBR Themes
    CrystalWater,
    RainWindowGlass,
    BrushedTitanium,
    WarmHiFiBrass,
    AbyssalTrench,
    GlacierIce,
    SaddleLeather,
    ZenCalmMist,
    CarbonStealth,
    GoldenHourHorizon,
    SwissChronograph,
    CarraraMarble,
    // Interactive Physics (6)
    WinterSnow,
    HearthFire,
    LiquidWater,
    SakuraWind,
    CosmicLens,
    TeslaPlasma,
    // Glass & Minimal (4) + Cyber & Editorial (3)
    AutumnCrystal,
    NordicMinimal,
    FloatingIsland,
    CelestialSunDial,
    EditorialSerif,
    CyberHud,
    NeonMatrix,
}

impl ExtWidgetTheme {
    pub const ALL: [ExtWidgetTheme; 35] = [
        ExtWidgetTheme::HoroRoyalOak,
        ExtWidgetTheme::HoroPatekMoon,
        ExtWidgetTheme::HoroTourbillonGold,
        ExtWidgetTheme::HoroDaytonaCeramic,
        ExtWidgetTheme::HoroSubmarinerEmerald,
        ExtWidgetTheme::HoroIwcTitanium,
        ExtWidgetTheme::HoroBreguetPorcelain,
        ExtWidgetTheme::HoroForgedCarbon,
        ExtWidgetTheme::HoroWalnutRegulator,
        ExtWidgetTheme::HoroSapphireObsidian,
        ExtWidgetTheme::CrystalWater,
        ExtWidgetTheme::RainWindowGlass,
        ExtWidgetTheme::BrushedTitanium,
        ExtWidgetTheme::WarmHiFiBrass,
        ExtWidgetTheme::AbyssalTrench,
        ExtWidgetTheme::GlacierIce,
        ExtWidgetTheme::SaddleLeather,
        ExtWidgetTheme::ZenCalmMist,
        ExtWidgetTheme::CarbonStealth,
        ExtWidgetTheme::GoldenHourHorizon,
        ExtWidgetTheme::SwissChronograph,
        ExtWidgetTheme::CarraraMarble,
        ExtWidgetTheme::WinterSnow,
        ExtWidgetTheme::HearthFire,
        ExtWidgetTheme::LiquidWater,
        ExtWidgetTheme::SakuraWind,
        ExtWidgetTheme::CosmicLens,
        ExtWidgetTheme::TeslaPlasma,
        ExtWidgetTheme::AutumnCrystal,
        ExtWidgetTheme::NordicMinimal,
        ExtWidgetTheme::FloatingIsland,
        ExtWidgetTheme::CelestialSunDial,
        ExtWidgetTheme::EditorialSerif,
        ExtWidgetTheme::CyberHud,
        ExtWidgetTheme::NeonMatrix,
    ];

    pub const HOROLOGY_FOLDER: [ExtWidgetTheme; 10] = [
        ExtWidgetTheme::HoroRoyalOak,
        ExtWidgetTheme::HoroPatekMoon,
        ExtWidgetTheme::HoroTourbillonGold,
        ExtWidgetTheme::HoroDaytonaCeramic,
        ExtWidgetTheme::HoroSubmarinerEmerald,
        ExtWidgetTheme::HoroIwcTitanium,
        ExtWidgetTheme::HoroBreguetPorcelain,
        ExtWidgetTheme::HoroForgedCarbon,
        ExtWidgetTheme::HoroWalnutRegulator,
        ExtWidgetTheme::HoroSapphireObsidian,
    ];

    pub const REALISTIC_FOLDER: [ExtWidgetTheme; 12] = [
        ExtWidgetTheme::CrystalWater,
        ExtWidgetTheme::RainWindowGlass,
        ExtWidgetTheme::BrushedTitanium,
        ExtWidgetTheme::WarmHiFiBrass,
        ExtWidgetTheme::AbyssalTrench,
        ExtWidgetTheme::GlacierIce,
        ExtWidgetTheme::SaddleLeather,
        ExtWidgetTheme::ZenCalmMist,
        ExtWidgetTheme::CarbonStealth,
        ExtWidgetTheme::GoldenHourHorizon,
        ExtWidgetTheme::SwissChronograph,
        ExtWidgetTheme::CarraraMarble,
    ];

    pub fn horology_theme(self) -> Option<crate::horology_pbr::HorologyTheme> {
        use crate::horology_pbr::HorologyTheme;
        match self {
            ExtWidgetTheme::HoroRoyalOak => Some(HorologyTheme::RoyalOakSteel),
            ExtWidgetTheme::HoroPatekMoon => Some(HorologyTheme::PatekMoonphase),
            ExtWidgetTheme::HoroTourbillonGold => Some(HorologyTheme::TourbillonGold),
            ExtWidgetTheme::HoroDaytonaCeramic => Some(HorologyTheme::DaytonaCeramic),
            ExtWidgetTheme::HoroSubmarinerEmerald => Some(HorologyTheme::SubmarinerEmerald),
            ExtWidgetTheme::HoroIwcTitanium => Some(HorologyTheme::IwcPilotTitanium),
            ExtWidgetTheme::HoroBreguetPorcelain => Some(HorologyTheme::BreguetPorcelain),
            ExtWidgetTheme::HoroForgedCarbon => Some(HorologyTheme::ForgedCarbonMoto),
            ExtWidgetTheme::HoroWalnutRegulator => Some(HorologyTheme::WalnutRegulator),
            ExtWidgetTheme::HoroSapphireObsidian => Some(HorologyTheme::SapphireObsidian),
            _ => None,
        }
    }

    pub fn pbr_material(self) -> Option<crate::realistic_pbr::PbrMaterial> {
        use crate::realistic_pbr::PbrMaterial;
        match self {
            ExtWidgetTheme::CrystalWater => Some(PbrMaterial::CalmOceanLagoon),
            ExtWidgetTheme::RainWindowGlass => Some(PbrMaterial::RainyWindowpane),
            ExtWidgetTheme::BrushedTitanium => Some(PbrMaterial::BrushedTitanium),
            ExtWidgetTheme::WarmHiFiBrass | ExtWidgetTheme::SwissChronograph => {
                Some(PbrMaterial::VintageWalnutBrass)
            }
            ExtWidgetTheme::AbyssalTrench => Some(PbrMaterial::DeepAbyssalTrench),
            ExtWidgetTheme::GlacierIce => Some(PbrMaterial::NordicGlacialIce),
            ExtWidgetTheme::SaddleLeather => Some(PbrMaterial::ExecutiveLeatherLinen),
            ExtWidgetTheme::ZenCalmMist => Some(PbrMaterial::AlpineForestMist),
            ExtWidgetTheme::CarbonStealth => Some(PbrMaterial::IndustrialCarbonSteel),
            ExtWidgetTheme::GoldenHourHorizon => Some(PbrMaterial::GoldenHourHorizon),
            ExtWidgetTheme::CarraraMarble => Some(PbrMaterial::CarraraMarbleGold),
            _ => None,
        }
    }

    pub fn is_realistic_interactive(self) -> bool {
        self.horology_theme().is_some() || self.pbr_material().is_some()
    }

    pub fn has_idle_motion(self) -> bool {
        matches!(
            self,
            ExtWidgetTheme::WinterSnow
                | ExtWidgetTheme::HearthFire
                | ExtWidgetTheme::LiquidWater
                | ExtWidgetTheme::SakuraWind
                | ExtWidgetTheme::CosmicLens
                | ExtWidgetTheme::TeslaPlasma
        ) || self
            .horology_theme()
            .map(|h| h.has_idle_motion())
            .unwrap_or(false)
            || self
                .pbr_material()
                .map(|m| m.has_ambient_idle_motion())
                .unwrap_or(false)
    }

    pub const INTERACTIVE_FOLDER: [ExtWidgetTheme; 6] = [
        ExtWidgetTheme::WinterSnow,
        ExtWidgetTheme::HearthFire,
        ExtWidgetTheme::LiquidWater,
        ExtWidgetTheme::SakuraWind,
        ExtWidgetTheme::CosmicLens,
        ExtWidgetTheme::TeslaPlasma,
    ];

    pub const GLASS_FOLDER: [ExtWidgetTheme; 4] = [
        ExtWidgetTheme::AutumnCrystal,
        ExtWidgetTheme::NordicMinimal,
        ExtWidgetTheme::FloatingIsland,
        ExtWidgetTheme::CelestialSunDial,
    ];

    pub const CYBER_FOLDER: [ExtWidgetTheme; 3] = [
        ExtWidgetTheme::EditorialSerif,
        ExtWidgetTheme::CyberHud,
        ExtWidgetTheme::NeonMatrix,
    ];

    pub fn in_folder(folder: ExtThemeFolder) -> &'static [ExtWidgetTheme] {
        match folder {
            ExtThemeFolder::Horology => &Self::HOROLOGY_FOLDER,
            ExtThemeFolder::RealisticMaterials => &Self::REALISTIC_FOLDER,
            ExtThemeFolder::InteractiveElements => &Self::INTERACTIVE_FOLDER,
            ExtThemeFolder::GlassMinimal => &Self::GLASS_FOLDER,
            ExtThemeFolder::CyberEditorial => &Self::CYBER_FOLDER,
            ExtThemeFolder::All => &Self::ALL,
        }
    }

    pub fn as_str(self) -> &'static str {
        if let Some(ht) = self.horology_theme() {
            return ht.as_str();
        }
        match self {
            ExtWidgetTheme::CrystalWater => "crystal_water",
            ExtWidgetTheme::RainWindowGlass => "rain_window",
            ExtWidgetTheme::ZenCalmMist => "zen_calm_mist",
            ExtWidgetTheme::BrushedTitanium => "brushed_titanium",
            ExtWidgetTheme::WarmHiFiBrass => "warm_hifi_brass",
            ExtWidgetTheme::AbyssalTrench => "abyssal_trench",
            ExtWidgetTheme::GlacierIce => "glacier_ice",
            ExtWidgetTheme::GoldenHourHorizon => "golden_hour_horizon",
            ExtWidgetTheme::SwissChronograph => "swiss_chronograph",
            ExtWidgetTheme::SaddleLeather => "saddle_leather",
            ExtWidgetTheme::CarbonStealth => "carbon_stealth",
            ExtWidgetTheme::CarraraMarble => "carrara_marble",
            ExtWidgetTheme::WinterSnow => "winter_snow",
            ExtWidgetTheme::HearthFire => "hearth_fire",
            ExtWidgetTheme::LiquidWater => "liquid_water",
            ExtWidgetTheme::SakuraWind => "sakura_wind",
            ExtWidgetTheme::CosmicLens => "cosmic_lens",
            ExtWidgetTheme::TeslaPlasma => "tesla_plasma",
            ExtWidgetTheme::AutumnCrystal => "autumn_crystal",
            ExtWidgetTheme::NordicMinimal => "nordic_minimal",
            ExtWidgetTheme::FloatingIsland => "floating_island",
            ExtWidgetTheme::CelestialSunDial => "celestial_sun_dial",
            ExtWidgetTheme::EditorialSerif => "editorial_serif",
            ExtWidgetTheme::CyberHud => "cyber_hud",
            ExtWidgetTheme::NeonMatrix => "neon_matrix",
            _ => "horo_royal_oak",
        }
    }

    pub fn from_str(s: &str) -> Self {
        if let Some(ht) = crate::horology_pbr::HorologyTheme::from_str(s) {
            return Self::HOROLOGY_FOLDER[ht.idx()];
        }
        match s.trim().to_ascii_lowercase().as_str() {
            "crystal_water" | "real_water" => ExtWidgetTheme::CrystalWater,
            "rain_window" | "rainy_windowpane" => ExtWidgetTheme::RainWindowGlass,
            "zen_calm_mist" | "zen_calm" | "alpine_forest_mist" => ExtWidgetTheme::ZenCalmMist,
            "brushed_titanium" | "titanium" => ExtWidgetTheme::BrushedTitanium,
            "warm_hifi_brass" | "hifi_brass" | "walnut_brass" => ExtWidgetTheme::WarmHiFiBrass,
            "abyssal_trench" | "deep_abyssal" => ExtWidgetTheme::AbyssalTrench,
            "glacier_ice" | "nordic_ice" => ExtWidgetTheme::GlacierIce,
            "golden_hour_horizon" | "golden_hour" => ExtWidgetTheme::GoldenHourHorizon,
            "swiss_chronograph" | "horology" => ExtWidgetTheme::SwissChronograph,
            "saddle_leather" | "leather" => ExtWidgetTheme::SaddleLeather,
            "carbon_stealth" | "carbon" => ExtWidgetTheme::CarbonStealth,
            "carrara_marble" | "marble" => ExtWidgetTheme::CarraraMarble,
            "winter_snow" | "snow" => ExtWidgetTheme::WinterSnow,
            "hearth_fire" | "fire" => ExtWidgetTheme::HearthFire,
            "liquid_water" | "water" => ExtWidgetTheme::LiquidWater,
            "sakura_wind" | "sakura" => ExtWidgetTheme::SakuraWind,
            "cosmic_lens" | "cosmic" => ExtWidgetTheme::CosmicLens,
            "tesla_plasma" | "plasma" => ExtWidgetTheme::TeslaPlasma,
            "nordic_minimal" | "minimal" => ExtWidgetTheme::NordicMinimal,
            "floating_island" | "island" => ExtWidgetTheme::FloatingIsland,
            "celestial_sun_dial" | "sun_dial" | "celestial" => ExtWidgetTheme::CelestialSunDial,
            "editorial_serif" | "editorial" => ExtWidgetTheme::EditorialSerif,
            "cyber_hud" | "cyber" => ExtWidgetTheme::CyberHud,
            "neon_matrix" | "neon" => ExtWidgetTheme::NeonMatrix,
            _ => ExtWidgetTheme::AutumnCrystal,
        }
    }

    pub fn label(self) -> &'static str {
        if let Some(ht) = self.horology_theme() {
            return ht.label();
        }
        match self {
            ExtWidgetTheme::CrystalWater => "Calm Ocean Lagoon",
            ExtWidgetTheme::RainWindowGlass => "Rainy Windowpane",
            ExtWidgetTheme::ZenCalmMist => "Alpine Forest Mist",
            ExtWidgetTheme::BrushedTitanium => "Brushed Titanium Hi-Fi",
            ExtWidgetTheme::WarmHiFiBrass => "Vintage Walnut & Brass",
            ExtWidgetTheme::AbyssalTrench => "Deep Abyssal Trench",
            ExtWidgetTheme::GlacierIce => "Nordic Glacial Ice",
            ExtWidgetTheme::GoldenHourHorizon => "Golden Hour Horizon",
            ExtWidgetTheme::SwissChronograph => "Swiss Horology Steel",
            ExtWidgetTheme::SaddleLeather => "Executive Leather & Linen",
            ExtWidgetTheme::CarbonStealth => "Matte Carbon & Steel",
            ExtWidgetTheme::CarraraMarble => "Carrara Marble & Gold",
            ExtWidgetTheme::WinterSnow => "Winter Snowfall",
            ExtWidgetTheme::HearthFire => "Hearth Inferno",
            ExtWidgetTheme::LiquidWater => "Liquid Water",
            ExtWidgetTheme::SakuraWind => "Sakura Breeze",
            ExtWidgetTheme::CosmicLens => "Cosmic Gravity",
            ExtWidgetTheme::TeslaPlasma => "Tesla Plasma",
            ExtWidgetTheme::AutumnCrystal => "Autumn Crystal",
            ExtWidgetTheme::NordicMinimal => "Nordic Minimal",
            ExtWidgetTheme::FloatingIsland => "Floating Glass Pill",
            ExtWidgetTheme::CelestialSunDial => "Celestial Sun-Dial",
            ExtWidgetTheme::EditorialSerif => "Editorial Serif",
            ExtWidgetTheme::CyberHud => "Cyber HUD Deck",
            ExtWidgetTheme::NeonMatrix => "Neon Glass Matrix",
            _ => "Royal Oak Guilloché Steel",
        }
    }

    pub fn subtitle(self) -> &'static str {
        if let Some(ht) = self.horology_theme() {
            return ht.subtitle();
        }
        match self {
            ExtWidgetTheme::CrystalWater => "Voronoi water caustics • Interactive click & hover ripples",
            ExtWidgetTheme::RainWindowGlass => "Wet condensation glass • Interactive cursor wiper & droplets",
            ExtWidgetTheme::ZenCalmMist => "Volumetric pine mist • Sunbeams & drifting golden pollen",
            ExtWidgetTheme::BrushedTitanium => "Anisotropic milled titanium • Torx bolts & cursor glare",
            ExtWidgetTheme::WarmHiFiBrass => "Cathedral walnut grain • Slotted antique brass screws",
            ExtWidgetTheme::AbyssalTrench => "Bioluminescent deep trench • Swirling plankton & sonar",
            ExtWidgetTheme::GlacierIce => "Faceted glacial crystal • 45° chromatic prism refraction",
            ExtWidgetTheme::GoldenHourHorizon => "Coastal sunset glare • Cursor-parallax solar sea track",
            ExtWidgetTheme::SwissChronograph => "Engine-turned guilloché • Sapphire glass & ruby pivot",
            ExtWidgetTheme::SaddleLeather => "Pebble saddle leather • Waxed stitches & brass rivets",
            ExtWidgetTheme::CarbonStealth => "2x2 twill aerospace carbon • Stainless hex bolts & orange",
            ExtWidgetTheme::CarraraMarble => "Polished Italian Carrara marble • Glinting kintsugi gold",
            ExtWidgetTheme::WinterSnow => "Hover-reactive crystalline snowflakes & frost wake",
            ExtWidgetTheme::HearthFire => "Rising hearth flames & embers fanned by cursor hover",
            ExtWidgetTheme::LiquidWater => "Rippling water caustics radiating under your mouse",
            ExtWidgetTheme::SakuraWind => "Soft cherry blossoms & gusts swirling on hover",
            ExtWidgetTheme::CosmicLens => "Orbital starlight & warp dust bending toward cursor",
            ExtWidgetTheme::TeslaPlasma => "Electric cyan-violet arcs snapping onto cursor",
            ExtWidgetTheme::AutumnCrystal => "Warm obsidian glass • Amber gold spine & glow rim",
            ExtWidgetTheme::NordicMinimal => "Ultra-clean Swiss frost • Subtle hairline frame",
            ExtWidgetTheme::FloatingIsland => "Deep rounded capsule • Glowing inner ring & badges",
            ExtWidgetTheme::CelestialSunDial => "24h orbital sun/moon dial • Solar horizon arc & 7-day forecast",
            ExtWidgetTheme::EditorialSerif => "Classic serif headings • Script accents & warm tint",
            ExtWidgetTheme::CyberHud => "Tech corner brackets • Segmented LED bars & mono font",
            ExtWidgetTheme::NeonMatrix => "Carbon matrix backdrop • Dual-tone neon gradient rim",
            _ => "Bespoke machined horology instrument • Cast needle shadows & sapphire AR",
        }
    }

    pub fn badge(self) -> &'static str {
        if let Some(ht) = self.horology_theme() {
            return ht.badge();
        }
        match self {
            ExtWidgetTheme::CrystalWater => "Lagoon",
            ExtWidgetTheme::RainWindowGlass => "Rain",
            ExtWidgetTheme::ZenCalmMist => "Alpine",
            ExtWidgetTheme::BrushedTitanium => "CNC Ti",
            ExtWidgetTheme::WarmHiFiBrass => "Walnut",
            ExtWidgetTheme::AbyssalTrench => "Abyssal",
            ExtWidgetTheme::GlacierIce => "Glacial",
            ExtWidgetTheme::GoldenHourHorizon => "Sunset",
            ExtWidgetTheme::SwissChronograph => "Swiss",
            ExtWidgetTheme::SaddleLeather => "Leather",
            ExtWidgetTheme::CarbonStealth => "Carbon",
            ExtWidgetTheme::CarraraMarble => "Marble",
            ExtWidgetTheme::WinterSnow => "Hover FX",
            ExtWidgetTheme::HearthFire => "Hover FX",
            ExtWidgetTheme::LiquidWater => "Hover FX",
            ExtWidgetTheme::SakuraWind => "Hover FX",
            ExtWidgetTheme::CosmicLens => "Hover FX",
            ExtWidgetTheme::TeslaPlasma => "Hover FX",
            ExtWidgetTheme::AutumnCrystal => "Crystal",
            ExtWidgetTheme::NordicMinimal => "Minimal",
            ExtWidgetTheme::FloatingIsland => "Capsule",
            ExtWidgetTheme::CelestialSunDial => "Sun-Dial",
            ExtWidgetTheme::EditorialSerif => "Serif",
            ExtWidgetTheme::CyberHud => "Cyber HUD",
            ExtWidgetTheme::NeonMatrix => "Neon",
            _ => "Horology",
        }
    }

    pub fn hover_fx_preset(self) -> TaskbarVizStyle {
        match self {
            ExtWidgetTheme::CrystalWater => TaskbarVizStyle::HoverWater,
            ExtWidgetTheme::WinterSnow => TaskbarVizStyle::HoverSnow,
            ExtWidgetTheme::HearthFire => TaskbarVizStyle::HoverFire,
            ExtWidgetTheme::LiquidWater => TaskbarVizStyle::HoverWater,
            ExtWidgetTheme::SakuraWind => TaskbarVizStyle::HoverSakura,
            ExtWidgetTheme::CosmicLens => TaskbarVizStyle::HoverCosmic,
            ExtWidgetTheme::TeslaPlasma => TaskbarVizStyle::HoverPlasma,
            _ => TaskbarVizStyle::Off,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ExtFontStyle {
    DefaultSans = 0,
    EditorialSerif = 1,
    CondensedBold = 2,
    CyberMono = 3,
    CalligraphyScript = 4,
    GeometricBold = 5,
}

impl ExtFontStyle {
    pub const ALL: [ExtFontStyle; 6] = [
        ExtFontStyle::DefaultSans,
        ExtFontStyle::EditorialSerif,
        ExtFontStyle::CondensedBold,
        ExtFontStyle::CyberMono,
        ExtFontStyle::CalligraphyScript,
        ExtFontStyle::GeometricBold,
    ];

    pub fn label(self) -> &'static str {
        match self {
            ExtFontStyle::DefaultSans => "SF Pro",
            ExtFontStyle::EditorialSerif => "Editorial",
            ExtFontStyle::CondensedBold => "Morganite",
            ExtFontStyle::CyberMono => "Consolas",
            ExtFontStyle::CalligraphyScript => "Tuesday",
            ExtFontStyle::GeometricBold => "Metropolis",
        }
    }

    pub fn from_u8(v: u8) -> Self {
        match v {
            1 => ExtFontStyle::EditorialSerif,
            2 => ExtFontStyle::CondensedBold,
            3 => ExtFontStyle::CyberMono,
            4 => ExtFontStyle::CalligraphyScript,
            5 => ExtFontStyle::GeometricBold,
            _ => ExtFontStyle::DefaultSans,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ExtSubTab {
    Layout,
    Features,
    Appearance,
    Materials,
    NotesText,
    WorldClockCities,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct WorldClockCity {
    pub name: &'static str,
    pub offset_mins: i32,
    pub tz_abbr: &'static str,
}

/// Curated 48-city global timezone database for user-selectable World Clock slots.
pub const WORLD_CLOCK_CITIES: [WorldClockCity; 48] = [
    // Core Global Hubs (Page 1: 0..12)
    WorldClockCity { name: "New York", offset_mins: -300, tz_abbr: "EST" },
    WorldClockCity { name: "London", offset_mins: 0, tz_abbr: "GMT" },
    WorldClockCity { name: "Mumbai", offset_mins: 330, tz_abbr: "IST" },
    WorldClockCity { name: "Tokyo", offset_mins: 540, tz_abbr: "JST" },
    WorldClockCity { name: "San Francisco", offset_mins: -480, tz_abbr: "PST" },
    WorldClockCity { name: "Dubai", offset_mins: 240, tz_abbr: "GST" },
    WorldClockCity { name: "Singapore", offset_mins: 480, tz_abbr: "SGT" },
    WorldClockCity { name: "Sydney", offset_mins: 660, tz_abbr: "AEDT" },
    WorldClockCity { name: "UTC", offset_mins: 0, tz_abbr: "UTC" },
    WorldClockCity { name: "Paris", offset_mins: 60, tz_abbr: "CET" },
    WorldClockCity { name: "Berlin", offset_mins: 60, tz_abbr: "CET" },
    WorldClockCity { name: "Toronto", offset_mins: -300, tz_abbr: "EST" },
    // Americas (Page 2: 12..24)
    WorldClockCity { name: "Los Angeles", offset_mins: -480, tz_abbr: "PST" },
    WorldClockCity { name: "Seattle", offset_mins: -480, tz_abbr: "PST" },
    WorldClockCity { name: "Chicago", offset_mins: -360, tz_abbr: "CST" },
    WorldClockCity { name: "Denver", offset_mins: -420, tz_abbr: "MST" },
    WorldClockCity { name: "Miami", offset_mins: -300, tz_abbr: "EST" },
    WorldClockCity { name: "Vancouver", offset_mins: -480, tz_abbr: "PST" },
    WorldClockCity { name: "Mexico City", offset_mins: -360, tz_abbr: "CST" },
    WorldClockCity { name: "Sao Paulo", offset_mins: -180, tz_abbr: "BRT" },
    WorldClockCity { name: "Buenos Aires", offset_mins: -180, tz_abbr: "ART" },
    WorldClockCity { name: "Bogota", offset_mins: -300, tz_abbr: "COT" },
    WorldClockCity { name: "Honolulu", offset_mins: -600, tz_abbr: "HST" },
    WorldClockCity { name: "Anchorage", offset_mins: -540, tz_abbr: "AKST" },
    // Europe & Africa (Page 3: 24..36)
    WorldClockCity { name: "Amsterdam", offset_mins: 60, tz_abbr: "CET" },
    WorldClockCity { name: "Madrid", offset_mins: 60, tz_abbr: "CET" },
    WorldClockCity { name: "Rome", offset_mins: 60, tz_abbr: "CET" },
    WorldClockCity { name: "Zurich", offset_mins: 60, tz_abbr: "CET" },
    WorldClockCity { name: "Stockholm", offset_mins: 60, tz_abbr: "CET" },
    WorldClockCity { name: "Dublin", offset_mins: 0, tz_abbr: "GMT" },
    WorldClockCity { name: "Lisbon", offset_mins: 0, tz_abbr: "WET" },
    WorldClockCity { name: "Athens", offset_mins: 120, tz_abbr: "EET" },
    WorldClockCity { name: "Istanbul", offset_mins: 180, tz_abbr: "TRT" },
    WorldClockCity { name: "Cairo", offset_mins: 120, tz_abbr: "EET" },
    WorldClockCity { name: "Johannesburg", offset_mins: 120, tz_abbr: "SAST" },
    WorldClockCity { name: "Nairobi", offset_mins: 180, tz_abbr: "EAT" },
    // Asia & Pacific (Page 4: 36..48)
    WorldClockCity { name: "New Delhi", offset_mins: 330, tz_abbr: "IST" },
    WorldClockCity { name: "Bengaluru", offset_mins: 330, tz_abbr: "IST" },
    WorldClockCity { name: "Kathmandu", offset_mins: 345, tz_abbr: "NPT" },
    WorldClockCity { name: "Dhaka", offset_mins: 360, tz_abbr: "BST" },
    WorldClockCity { name: "Riyadh", offset_mins: 180, tz_abbr: "AST" },
    WorldClockCity { name: "Bangkok", offset_mins: 420, tz_abbr: "ICT" },
    WorldClockCity { name: "Hong Kong", offset_mins: 480, tz_abbr: "HKT" },
    WorldClockCity { name: "Shanghai", offset_mins: 480, tz_abbr: "CST" },
    WorldClockCity { name: "Taipei", offset_mins: 480, tz_abbr: "CST" },
    WorldClockCity { name: "Seoul", offset_mins: 540, tz_abbr: "KST" },
    WorldClockCity { name: "Melbourne", offset_mins: 660, tz_abbr: "AEDT" },
    WorldClockCity { name: "Auckland", offset_mins: 780, tz_abbr: "NZDT" },
];

pub fn default_world_clock_cities() -> [String; 4] {
    [
        "New York".to_string(),
        "London".to_string(),
        "Tokyo".to_string(),
        "Sydney".to_string(),
    ]
}

pub fn world_clock_preset_cities(mode_idx: u8) -> [&'static str; 4] {
    match mode_idx {
        1 => ["San Francisco", "Berlin", "Dubai", "Singapore"],
        2 => ["UTC", "New York", "Mumbai", "Tokyo"],
        3 => ["London", "Paris", "Mumbai", "Kathmandu"],
        _ => ["New York", "London", "Tokyo", "Sydney"],
    }
}

pub fn resolve_world_clock_city(name: &str) -> WorldClockCity {
    let key = name.trim();
    if let Some(&c) = WORLD_CLOCK_CITIES
        .iter()
        .find(|c| c.name.eq_ignore_ascii_case(key))
    {
        return c;
    }
    WORLD_CLOCK_CITIES[8] // UTC fallback
}

pub fn resolve_world_clock_slots(cfg: &AppConfig) -> [WorldClockCity; 4] {
    [
        resolve_world_clock_city(&cfg.world_clock_cities[0]),
        resolve_world_clock_city(&cfg.world_clock_cities[1]),
        resolve_world_clock_city(&cfg.world_clock_cities[2]),
        resolve_world_clock_city(&cfg.world_clock_cities[3]),
    ]
}

pub fn format_utc_offset(offset_mins: i32) -> String {
    let sign = if offset_mins >= 0 { '+' } else { '-' };
    let abs = offset_mins.abs();
    let h = abs / 60;
    let m = abs % 60;
    if m == 0 {
        format!("UTC{}{}", sign, h)
    } else {
        format!("UTC{}{}:{:02}", sign, h, m)
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ExtWidgetConfig {
    pub enabled: bool,
    pub x: i32,
    pub y: i32,
    pub theme: ExtWidgetTheme,

    // Sub-tab 1: Layout & Geometry
    pub scale_pct: u8,       // 50..=180
    pub width_pct: u8,       // 70..=150
    pub corner_radius: u8,   // 0..=36
    pub padding: u8,         // 4..=28
    pub align: ClockAlign,
    pub show_header: bool,
    pub show_primary: bool,
    pub show_icon: bool,
    pub show_dividers: bool,
    pub show_footer: bool,
    pub compact_mode: bool,

    // Sub-tab 2: Widget-Specific Content & Features
    pub mode_idx: u8,        // 0..=3
    pub param1_pct: u8,      // 0..=100
    pub param2_pct: u8,      // 0..=100
    pub feats: [bool; 6],

    // Sub-tab 3: Glass, Colors & Typography
    pub show_plate: bool,
    pub glass_alpha: u8,     // 0..=180
    pub rim_alpha: u8,       // 0..=255
    pub rim_width_tenths: u8,// 5..=30 (0.5px .. 3.0px)
    pub show_shadow: bool,
    pub match_wallpaper: bool,
    pub use_custom_color: bool,
    pub accent_rgb: u32,
    pub text_rgb: u32,
    pub bg_rgb: u32,
    pub font_style: ExtFontStyle,
    pub custom_font: String,
    pub font_scale_pct: u8,  // 75..=135
    pub hover_fx: TaskbarVizStyle,
    pub elem_transforms: [ClockElemTransform; 6],
    pub elem_fonts: [String; 6],
    pub horology: crate::horology_pbr::HorologyPartConfig,
}

impl ExtWidgetConfig {
    pub fn default_for(id: ExtWidgetId) -> Self {
        let (x, y) = id.default_pos();
        Self {
            enabled: false,
            x,
            y,
            theme: ExtWidgetTheme::AutumnCrystal,
            scale_pct: 100,
            width_pct: 100,
            corner_radius: 18,
            padding: 14,
            align: ClockAlign::Left,
            show_header: true,
            show_primary: true,
            show_icon: true,
            show_dividers: true,
            show_footer: true,
            compact_mode: false,
            mode_idx: 0,
            param1_pct: 65,
            param2_pct: 60,
            feats: [true, true, true, true, false, true],
            show_plate: true,
            glass_alpha: 62,
            rim_alpha: 135,
            rim_width_tenths: 12,
            show_shadow: true,
            match_wallpaper: false,
            use_custom_color: false,
            accent_rgb: 0xF5A642,
            text_rgb: 0xFFF3DC,
            bg_rgb: 0x16100C,
            font_style: ExtFontStyle::DefaultSans,
            custom_font: String::new(),
            font_scale_pct: 100,
            hover_fx: TaskbarVizStyle::Off,
            elem_transforms: [ClockElemTransform::default(); 6],
            elem_fonts: std::array::from_fn(|_| String::new()),
            horology: crate::horology_pbr::HorologyPartConfig::default(),
        }
    }

    #[inline]
    pub fn is_slot_visible(&self, slot_idx: usize) -> bool {
        let flag = match slot_idx {
            0 => self.show_header,
            1 => self.show_primary,
            2 => self.show_footer,
            3 => self.show_icon,
            4 => self.show_dividers,
            5 => self.show_plate,
            _ => true,
        };
        flag && self
            .elem_transforms
            .get(slot_idx)
            .map(|tr| tr.visible)
            .unwrap_or(true)
    }

    #[inline]
    pub fn is_all_elements_off(&self) -> bool {
        !(0..6).any(|i| self.is_slot_visible(i))
    }

    pub fn apply_theme_preset(&mut self, theme: ExtWidgetTheme) {
        self.theme = theme;
        self.match_wallpaper = false;
        self.hover_fx = theme.hover_fx_preset();
        self.custom_font.clear();
        self.show_header = true;
        self.show_primary = true;
        self.show_icon = true;
        self.show_footer = true;
        self.elem_transforms = [ClockElemTransform::default(); 6];
        self.elem_fonts = std::array::from_fn(|_| String::new());
        match theme {
            ExtWidgetTheme::WinterSnow => {
                self.show_plate = true;
                self.glass_alpha = 68;
                self.rim_alpha = 165;
                self.rim_width_tenths = 12;
                self.corner_radius = 18;
                self.font_style = ExtFontStyle::DefaultSans;
                self.use_custom_color = true;
                self.accent_rgb = 0x78D4FF;
                self.text_rgb = 0xF2FAFF;
                self.bg_rgb = 0x0E1826;
                self.show_shadow = true;
                self.show_dividers = true;
            }
            ExtWidgetTheme::HearthFire => {
                self.show_plate = true;
                self.glass_alpha = 74;
                self.rim_alpha = 180;
                self.rim_width_tenths = 13;
                self.corner_radius = 16;
                self.font_style = ExtFontStyle::GeometricBold;
                self.use_custom_color = true;
                self.accent_rgb = 0xFF7622;
                self.text_rgb = 0xFFF2E0;
                self.bg_rgb = 0x1A0C0A;
                self.show_shadow = true;
                self.show_dividers = true;
            }
            ExtWidgetTheme::LiquidWater => {
                self.show_plate = true;
                self.glass_alpha = 62;
                self.rim_alpha = 170;
                self.rim_width_tenths = 12;
                self.corner_radius = 22;
                self.font_style = ExtFontStyle::DefaultSans;
                self.use_custom_color = true;
                self.accent_rgb = 0x2EDEF0;
                self.text_rgb = 0xE8FCFF;
                self.bg_rgb = 0x081822;
                self.show_shadow = true;
                self.show_dividers = true;
            }
            ExtWidgetTheme::SakuraWind => {
                self.show_plate = true;
                self.glass_alpha = 68;
                self.rim_alpha = 165;
                self.rim_width_tenths = 12;
                self.corner_radius = 20;
                self.font_style = ExtFontStyle::EditorialSerif;
                self.use_custom_color = true;
                self.accent_rgb = 0xFF8AB4;
                self.text_rgb = 0xFFF2F8;
                self.bg_rgb = 0x1A0E16;
                self.show_shadow = true;
                self.show_dividers = true;
            }
            ExtWidgetTheme::CosmicLens => {
                self.show_plate = true;
                self.glass_alpha = 76;
                self.rim_alpha = 180;
                self.rim_width_tenths = 13;
                self.corner_radius = 18;
                self.font_style = ExtFontStyle::GeometricBold;
                self.use_custom_color = true;
                self.accent_rgb = 0x987EFF;
                self.text_rgb = 0xF2F4FF;
                self.bg_rgb = 0x0E0C1C;
                self.show_shadow = true;
                self.show_dividers = true;
            }
            ExtWidgetTheme::TeslaPlasma => {
                self.show_plate = true;
                self.glass_alpha = 80;
                self.rim_alpha = 205;
                self.rim_width_tenths = 14;
                self.corner_radius = 14;
                self.font_style = ExtFontStyle::CyberMono;
                self.use_custom_color = true;
                self.accent_rgb = 0x40F0FF;
                self.text_rgb = 0xF0FAFF;
                self.bg_rgb = 0x100A1E;
                self.show_shadow = true;
                self.show_dividers = true;
            }
            ExtWidgetTheme::AutumnCrystal => {
                self.show_plate = true;
                self.glass_alpha = 62;
                self.rim_alpha = 135;
                self.rim_width_tenths = 12;
                self.corner_radius = 18;
                self.font_style = ExtFontStyle::DefaultSans;
                self.use_custom_color = true;
                self.accent_rgb = 0xEE864A;
                self.text_rgb = 0xFFF6EC;
                self.bg_rgb = 0x1A100E;
                self.show_shadow = true;
                self.show_dividers = true;
            }
            ExtWidgetTheme::NordicMinimal => {
                self.show_plate = true;
                self.glass_alpha = 28;
                self.rim_alpha = 60;
                self.rim_width_tenths = 8;
                self.corner_radius = 12;
                self.font_style = ExtFontStyle::DefaultSans;
                self.use_custom_color = true;
                self.accent_rgb = 0xC6D8EC;
                self.text_rgb = 0xFFFFFF;
                self.bg_rgb = 0x14171C;
                self.show_shadow = false;
                self.show_dividers = false;
            }
            ExtWidgetTheme::FloatingIsland => {
                self.show_plate = true;
                self.glass_alpha = 92;
                self.rim_alpha = 175;
                self.rim_width_tenths = 14;
                self.corner_radius = 28;
                self.font_style = ExtFontStyle::GeometricBold;
                self.use_custom_color = true;
                self.accent_rgb = 0x4CC4FF;
                self.text_rgb = 0xF6FCFF;
                self.bg_rgb = 0x0E1218;
                self.show_shadow = true;
                self.show_dividers = false;
            }
            ExtWidgetTheme::CelestialSunDial => {
                self.show_plate = true;
                self.glass_alpha = 138;
                self.rim_alpha = 158;
                self.rim_width_tenths = 12;
                self.corner_radius = 22;
                self.font_style = ExtFontStyle::DefaultSans;
                self.use_custom_color = true;
                self.accent_rgb = 0xFF765C;
                self.text_rgb = 0xF8FAFC;
                self.bg_rgb = 0x181020;
                self.show_shadow = true;
                self.show_dividers = true;
            }
            ExtWidgetTheme::EditorialSerif => {
                self.show_plate = true;
                self.glass_alpha = 56;
                self.rim_alpha = 115;
                self.rim_width_tenths = 10;
                self.corner_radius = 14;
                self.font_style = ExtFontStyle::EditorialSerif;
                self.use_custom_color = true;
                self.accent_rgb = 0xEE9EA8;
                self.text_rgb = 0xFEF5F6;
                self.bg_rgb = 0x1E0E12;
                self.show_shadow = true;
                self.show_dividers = true;
            }
            ExtWidgetTheme::CyberHud => {
                self.show_plate = true;
                self.glass_alpha = 80;
                self.rim_alpha = 215;
                self.rim_width_tenths = 15;
                self.corner_radius = 4;
                self.font_style = ExtFontStyle::CyberMono;
                self.use_custom_color = true;
                self.accent_rgb = 0x5CE1E6;
                self.text_rgb = 0xE6FFFF;
                self.bg_rgb = 0x0A1017;
                self.show_shadow = true;
                self.show_dividers = true;
            }
            ExtWidgetTheme::NeonMatrix => {
                self.show_plate = true;
                self.glass_alpha = 86;
                self.rim_alpha = 230;
                self.rim_width_tenths = 16;
                self.corner_radius = 16;
                self.font_style = ExtFontStyle::CondensedBold;
                self.use_custom_color = true;
                self.accent_rgb = 0xFF6B8B;
                self.text_rgb = 0xFFF0F5;
                self.bg_rgb = 0x110B18;
                self.show_shadow = true;
                self.show_dividers = true;
            }
            ExtWidgetTheme::CrystalWater => {
                self.show_plate = true;
                self.glass_alpha = 188;
                self.rim_alpha = 215;
                self.rim_width_tenths = 15;
                self.corner_radius = 22;
                self.font_style = ExtFontStyle::DefaultSans;
                self.use_custom_color = true;
                self.accent_rgb = 0x5CEEFF;
                self.text_rgb = 0xF2FEFF;
                self.bg_rgb = 0x0C6694;
                self.show_shadow = true;
                self.show_dividers = true;
            }
            ExtWidgetTheme::ZenCalmMist => {
                self.show_plate = true;
                self.glass_alpha = 205;
                self.rim_alpha = 155;
                self.rim_width_tenths = 11;
                self.corner_radius = 20;
                self.font_style = ExtFontStyle::EditorialSerif;
                self.use_custom_color = true;
                self.accent_rgb = 0xA6D2BE;
                self.text_rgb = 0xF2F8F4;
                self.bg_rgb = 0x243432;
                self.show_shadow = false;
                self.show_dividers = true;
            }
            ExtWidgetTheme::BrushedTitanium => {
                self.show_plate = true;
                self.glass_alpha = 236;
                self.rim_alpha = 205;
                self.rim_width_tenths = 14;
                self.corner_radius = 10;
                self.font_style = ExtFontStyle::GeometricBold;
                self.use_custom_color = true;
                self.accent_rgb = 0xFFA726;
                self.text_rgb = 0xF4F7FA;
                self.bg_rgb = 0x1C1E22;
                self.show_shadow = true;
                self.show_dividers = true;
            }
            ExtWidgetTheme::WarmHiFiBrass => {
                self.show_plate = true;
                self.glass_alpha = 238;
                self.rim_alpha = 210;
                self.rim_width_tenths = 15;
                self.corner_radius = 12;
                self.font_style = ExtFontStyle::EditorialSerif;
                self.use_custom_color = true;
                self.accent_rgb = 0xD4AF37;
                self.text_rgb = 0xFFF2DC;
                self.bg_rgb = 0x231709;
                self.show_shadow = true;
                self.show_dividers = true;
            }
            ExtWidgetTheme::SwissChronograph => {
                self.show_plate = true;
                self.glass_alpha = 238;
                self.rim_alpha = 215;
                self.rim_width_tenths = 15;
                self.corner_radius = 16;
                self.font_style = ExtFontStyle::EditorialSerif;
                self.use_custom_color = true;
                self.accent_rgb = 0x70AEFF;
                self.text_rgb = 0xF4F8FF;
                self.bg_rgb = 0x0F1C34;
                self.show_shadow = true;
                self.show_dividers = true;
            }
            ExtWidgetTheme::SaddleLeather => {
                self.show_plate = true;
                self.glass_alpha = 242;
                self.rim_alpha = 195;
                self.rim_width_tenths = 14;
                self.corner_radius = 14;
                self.font_style = ExtFontStyle::EditorialSerif;
                self.use_custom_color = true;
                self.accent_rgb = 0xE2AE5C;
                self.text_rgb = 0xFCF4E6;
                self.bg_rgb = 0x542E18;
                self.show_shadow = true;
                self.show_dividers = true;
            }
            ExtWidgetTheme::CarbonStealth => {
                self.show_plate = true;
                self.glass_alpha = 242;
                self.rim_alpha = 185;
                self.rim_width_tenths = 13;
                self.corner_radius = 10;
                self.font_style = ExtFontStyle::CyberMono;
                self.use_custom_color = true;
                self.accent_rgb = 0xFF6B35;
                self.text_rgb = 0xEDF2F4;
                self.bg_rgb = 0x121212;
                self.show_shadow = true;
                self.show_dividers = true;
            }
            ExtWidgetTheme::CarraraMarble => {
                self.show_plate = true;
                self.glass_alpha = 242;
                self.rim_alpha = 210;
                self.rim_width_tenths = 14;
                self.corner_radius = 14;
                self.font_style = ExtFontStyle::EditorialSerif;
                self.use_custom_color = true;
                self.accent_rgb = 0xD4AF37;
                self.text_rgb = 0x222226;
                self.bg_rgb = 0xEFECE6;
                self.show_shadow = true;
                self.show_dividers = true;
            }
            ExtWidgetTheme::RainWindowGlass => {
                self.show_plate = true;
                self.glass_alpha = 204;
                self.rim_alpha = 185;
                self.rim_width_tenths = 13;
                self.corner_radius = 20;
                self.font_style = ExtFontStyle::DefaultSans;
                self.use_custom_color = true;
                self.accent_rgb = 0x8A9EA7;
                self.text_rgb = 0xE6ECEF;
                self.bg_rgb = 0x1A1D20;
                self.show_shadow = true;
                self.show_dividers = true;
            }
            ExtWidgetTheme::AbyssalTrench => {
                self.show_plate = true;
                self.glass_alpha = 232;
                self.rim_alpha = 215;
                self.rim_width_tenths = 14;
                self.corner_radius = 20;
                self.font_style = ExtFontStyle::DefaultSans;
                self.use_custom_color = true;
                self.accent_rgb = 0x00FFCC;
                self.text_rgb = 0xE0FFF8;
                self.bg_rgb = 0x020B14;
                self.show_shadow = true;
                self.show_dividers = true;
            }
            ExtWidgetTheme::GlacierIce => {
                self.show_plate = true;
                self.glass_alpha = 216;
                self.rim_alpha = 220;
                self.rim_width_tenths = 15;
                self.corner_radius = 12;
                self.font_style = ExtFontStyle::DefaultSans;
                self.use_custom_color = true;
                self.accent_rgb = 0xA0E7E5;
                self.text_rgb = 0xE0F4FF;
                self.bg_rgb = 0x0B192C;
                self.show_shadow = true;
                self.show_dividers = true;
            }
            ExtWidgetTheme::GoldenHourHorizon => {
                self.show_plate = true;
                self.glass_alpha = 224;
                self.rim_alpha = 215;
                self.rim_width_tenths = 14;
                self.corner_radius = 20;
                self.font_style = ExtFontStyle::EditorialSerif;
                self.use_custom_color = true;
                self.accent_rgb = 0xFFD166;
                self.text_rgb = 0xFFE3D8;
                self.bg_rgb = 0x2B0938;
                self.show_shadow = true;
                self.show_dividers = true;
            }
            t if t.horology_theme().is_some() => {
                let ht = t.horology_theme().unwrap();
                let (_bez, dial, hands, lume, _jewel) = ht.default_colors();
                self.show_plate = true;
                self.glass_alpha = 175;
                self.rim_alpha = 215;
                self.rim_width_tenths = 14;
                self.corner_radius = 16;
                self.font_style = if ht.is_light_dial()
                    || matches!(
                        ht,
                        crate::horology_pbr::HorologyTheme::PatekMoonphase
                            | crate::horology_pbr::HorologyTheme::TourbillonGold
                            | crate::horology_pbr::HorologyTheme::WalnutRegulator
                    ) {
                    ExtFontStyle::EditorialSerif
                } else {
                    ExtFontStyle::GeometricBold
                };
                self.use_custom_color = false;
                self.accent_rgb = lume;
                self.text_rgb = hands;
                self.bg_rgb = dial;
                self.show_shadow = true;
                self.show_dividers = true;
                self.horology = crate::horology_pbr::HorologyPartConfig::default();
            }
            _ => {}
        }
    }

    pub fn logical_size(&self, id: ExtWidgetId) -> (f32, f32) {
        let (bw, bh) = if id == ExtWidgetId::Weather
            && (self.theme == ExtWidgetTheme::CelestialSunDial
                || self.theme.horology_theme().is_some())
        {
            if self.compact_mode {
                (336.0, 178.0)
            } else {
                (372.0, 214.0)
            }
        } else {
            id.base_size(self.compact_mode)
        };
        let w = bw * (self.width_pct as f32 / 100.0);
        (w, bh)
    }

    pub fn to_csv(&self) -> String {
        let b = |v: bool| if v { 1 } else { 0 };
        let cf_clean = self.custom_font.replace([',', '\r', '\n'], "");
        format!(
            "{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},0x{:06X},0x{:06X},0x{:06X},{},{},{},{}",
            b(self.enabled),
            self.x,
            self.y,
            self.theme.as_str(),
            self.scale_pct,
            self.width_pct,
            self.corner_radius,
            self.padding,
            self.align.as_str(),
            b(self.show_header),
            b(self.show_icon),
            b(self.show_dividers),
            b(self.show_footer),
            b(self.compact_mode),
            self.mode_idx,
            self.param1_pct,
            self.param2_pct,
            (0..6).fold(0u8, |acc, i| acc | (if self.feats[i] { 1 << i } else { 0 })),
            b(self.show_plate),
            self.glass_alpha,
            self.rim_alpha,
            self.rim_width_tenths,
            b(self.show_shadow),
            b(self.match_wallpaper),
            b(self.use_custom_color),
            self.font_style as u8,
            self.accent_rgb & 0x00FF_FFFF,
            self.text_rgb & 0x00FF_FFFF,
            self.bg_rgb & 0x00FF_FFFF,
            self.font_scale_pct,
            b(self.show_primary),
            self.hover_fx.as_str(),
            cf_clean
        )
    }

    pub fn from_csv(id: ExtWidgetId, s: &str) -> Self {
        let mut c = Self::default_for(id);
        let p: Vec<&str> = s.split(',').collect();
        if p.len() < 30 {
            return c;
        }
        let pb = |idx: usize| p.get(idx).map(|v| v.trim() == "1").unwrap_or(false);
        let pu8 = |idx: usize, def: u8| p.get(idx).and_then(|v| v.trim().parse::<u8>().ok()).unwrap_or(def);
        let pi32 = |idx: usize, def: i32| p.get(idx).and_then(|v| v.trim().parse::<i32>().ok()).unwrap_or(def);
        let phex = |idx: usize, def: u32| {
            p.get(idx)
                .and_then(|v| {
                    let t = v.trim().trim_start_matches("0x").trim_start_matches("0X");
                    u32::from_str_radix(t, 16).ok()
                })
                .unwrap_or(def)
        };
        c.enabled = pb(0);
        c.x = pi32(1, c.x).clamp(-100, 2400);
        c.y = pi32(2, c.y).clamp(-50, 1600);
        c.theme = ExtWidgetTheme::from_str(p.get(3).copied().unwrap_or(""));
        c.scale_pct = pu8(4, 100).clamp(50, 180);
        c.width_pct = pu8(5, 100).clamp(70, 150);
        c.corner_radius = pu8(6, 18).min(36);
        c.padding = pu8(7, 14).clamp(4, 28);
        c.align = ClockAlign::from_str(p.get(8).copied().unwrap_or("left"));
        c.show_header = pb(9);
        c.show_icon = pb(10);
        c.show_dividers = pb(11);
        c.show_footer = pb(12);
        c.compact_mode = pb(13);
        c.mode_idx = pu8(14, 0).min(if id == ExtWidgetId::SearchBar { 9 } else { 3 });
        c.param1_pct = pu8(15, 65).min(100);
        c.param2_pct = pu8(16, 60).min(100);
        let mask = pu8(17, 0b101111);
        for i in 0..6 {
            c.feats[i] = (mask & (1 << i)) != 0;
        }
        c.show_plate = pb(18);
        c.glass_alpha = pu8(19, 62).min(180);
        c.rim_alpha = pu8(20, 135);
        c.rim_width_tenths = pu8(21, 12).clamp(5, 30);
        c.show_shadow = pb(22);
        c.match_wallpaper = pb(23);
        c.use_custom_color = pb(24);
        c.font_style = ExtFontStyle::from_u8(pu8(25, 0));
        c.accent_rgb = phex(26, 0xF5A642);
        c.text_rgb = phex(27, 0xFFF3DC);
        c.bg_rgb = phex(28, 0x16100C);
        c.font_scale_pct = pu8(29, 100).clamp(75, 135);
        c.show_primary = p.get(30).map(|v| v.trim() != "0").unwrap_or(true);
        c.hover_fx = p
            .get(31)
            .copied()
            .map(TaskbarVizStyle::from_str)
            .unwrap_or_else(|| c.theme.hover_fx_preset());
        c.custom_font = p.get(32).map(|v| v.trim().to_string()).unwrap_or_default();
        c
    }
}

pub fn default_ext_widgets() -> [ExtWidgetConfig; EXT_WIDGET_COUNT] {
    std::array::from_fn(|i| ExtWidgetConfig::default_for(ExtWidgetId::ALL[i]))
}

pub fn default_notes_items() -> Vec<(bool, String)> {
    vec![
        (true, "Polish AutumnDeck live themes".to_string()),
        (false, "Review daily workspace layout".to_string()),
        (false, " customize desktop widgets".trim().to_string()),
        (false, "Focus session - 25m sprint".to_string()),
    ]
}

pub const SEARCH_ENGINES: [(&str, &str, &str); 10] = [
    ("Google", "https://www.google.com", "https://www.google.com/search?q={q}"),
    ("Bing", "https://www.bing.com", "https://www.bing.com/search?q={q}"),
    ("DuckDuckGo", "https://duckduckgo.com", "https://duckduckgo.com/?q={q}"),
    ("Brave", "https://search.brave.com", "https://search.brave.com/search?q={q}"),
    ("Yahoo", "https://search.yahoo.com", "https://search.yahoo.com/search?p={q}"),
    ("YouTube", "https://www.youtube.com", "https://www.youtube.com/results?search_query={q}"),
    ("Perplexity", "https://www.perplexity.ai", "https://www.perplexity.ai/search?q={q}"),
    ("Wikipedia", "https://en.wikipedia.org", "https://en.wikipedia.org/wiki/Special:Search?search={q}"),
    ("GitHub", "https://github.com", "https://github.com/search?q={q}"),
    ("Startpage", "https://www.startpage.com", "https://www.startpage.com/do/search?q={q}"),
];

pub fn search_engine_name(mode_idx: u8) -> &'static str {
    let idx = (mode_idx as usize).min(SEARCH_ENGINES.len() - 1);
    SEARCH_ENGINES[idx].0
}

fn encode_search_query(q: &str) -> String {
    let mut out = String::with_capacity(q.len() * 2);
    for b in q.as_bytes() {
        match *b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*b as char);
            }
            b' ' => out.push('+'),
            other => {
                out.push('%');
                let hi = (other >> 4) & 0x0F;
                let lo = other & 0x0F;
                out.push(char::from_digit(hi as u32, 16).unwrap_or('0').to_ascii_uppercase());
                out.push(char::from_digit(lo as u32, 16).unwrap_or('0').to_ascii_uppercase());
            }
        }
    }
    out
}

pub struct ExtWidgetFeatureSpec {
    pub mode_title: &'static str,
    pub modes: [&'static str; 4],
    pub param1_label: &'static str,
    pub param2_label: &'static str,
    pub feat_labels: [&'static str; 6],
}

pub fn widget_feature_spec(id: ExtWidgetId) -> ExtWidgetFeatureSpec {
    match id {
        ExtWidgetId::Weather => ExtWidgetFeatureSpec {
            mode_title: "Location",
            modes: ["Auto Local", "New York", "London", "Tokyo"],
            param1_label: "Forecast Columns",
            param2_label: "Icon Glow Intensity",
            feat_labels: [
                "Use Fahrenheit (°F)",
                "Show 7-Day Forecast Strip",
                "Show Humidity & Wind Speed",
                "Show High / Low Temp Range",
                "Show Feels-Like & UV Badge",
                "Animated Weather Particles",
            ],
        },
        ExtWidgetId::SearchBar => ExtWidgetFeatureSpec {
            mode_title: "Engine",
            modes: ["Google", "Bing", "DuckDuckGo", "Brave"],
            param1_label: "Input Bar Height",
            param2_label: "Focus Ring Glow",
            feat_labels: [
                "Show Search Engine Pill",
                "Show Keyboard Shortcut Hint",
                "Show Quick Engine Switcher",
                "Show Search Arrow Button",
                "Auto-Clear Query After Launch",
                "Show Live Blinking Caret",
            ],
        },
        ExtWidgetId::Greeting => ExtWidgetFeatureSpec {
            mode_title: "Quote Topic",
            modes: ["Stoic & Wisdom", "Builders & Tech", "Zen & Nature", "Literature"],
            param1_label: "Greeting Size",
            param2_label: "Quote Contrast",
            feat_labels: [
                "Show Personalized Greeting",
                "Show Day-Phase Sun / Moon Orb",
                "Show Daily Inspiration Quote",
                "Show Quote Author Attribution",
                "Show Next-Quote Refresh Button",
                "Use Italic Calligraphy Quote",
            ],
        },
        ExtWidgetId::Network => ExtWidgetFeatureSpec {
            mode_title: "Speed Unit",
            modes: ["Auto MB/s", "Megabits (Mbps)", "KB/s Fixed", "Compact Dual"],
            param1_label: "Graph History Density",
            param2_label: "Peak Scale Ceiling",
            feat_labels: [
                "Show 60s Live Sparkline Graph",
                "Show Upload & Download Arrows",
                "Show Ping / Latency Badge (ms)",
                "Show Local IPv4 Address",
                "Highlight Peak Traffic Spikes",
                "Fill Area Under Sparkline",
            ],
        },
        ExtWidgetId::Gpu => ExtWidgetFeatureSpec {
            mode_title: "Primary Focus",
            modes: ["Core Load %", "VRAM Memory", "Core Temp °C", "Clock MHz"],
            param1_label: "Telemetry Bar Height",
            param2_label: "Thermal Warning Level",
            feat_labels: [
                "Show Dedicated VRAM Usage GB",
                "Show GPU Temperature (°C)",
                "Show GPU Core Clock (MHz)",
                "Show Live Load Sparkline",
                "Show Adapter Model Badge",
                "Glow Coral Above Warning Temp",
            ],
        },
        ExtWidgetId::AudioSwitcher => ExtWidgetFeatureSpec {
            mode_title: "Panel Mode",
            modes: ["All Outputs", "Speakers + Buds", "Studio Mixer", "Compact Pill"],
            param1_label: "Volume Step Size",
            param2_label: "Meter Sensitivity",
            feat_labels: [
                "Show Quick Output Device Pills",
                "Show Master Volume Bar & %",
                "Show Quick - / Mute / + Buttons",
                "Show Live Audio Level Meter",
                "Show 24-Bit Studio Format Badge",
                "One-Click Sound Control Panel",
            ],
        },
        ExtWidgetId::Notes => ExtWidgetFeatureSpec {
            mode_title: "Note Style",
            modes: ["To-Do Checklist", "Scratchpad", "Priority List", "Numbered Steps"],
            param1_label: "Line Spacing",
            param2_label: "Checkmark Glow",
            feat_labels: [
                "Interactive Desktop Checkboxes",
                "Strike-Through Completed Items",
                "Show Completed Progress Counter",
                "Show + Add & Clear Done Buttons",
                "Show Bullet / Priority Badges",
                "Allow Direct Desktop Typing",
            ],
        },
        ExtWidgetId::Calendar => ExtWidgetFeatureSpec {
            mode_title: "Week Start",
            modes: ["Monday First", "Sunday First", "Agenda Split", "Compact Grid"],
            param1_label: "Cell Spacing",
            param2_label: "Today Ring Glow",
            feat_labels: [
                "Highlight Today in Sun-Gold Ring",
                "Tint Weekend Columns",
                "Show Prev / Today / Next Buttons",
                "Show Upcoming Agenda Line Below",
                "Show Day-of-Year & Week Number",
                "Show Event Indicator Dots",
            ],
        },
        ExtWidgetId::Pomodoro => ExtWidgetFeatureSpec {
            mode_title: "Timer Preset",
            modes: ["25m / 5m Classic", "50m / 10m Deep", "15m / 3m Sprint", "45m / 15m Study"],
            param1_label: "Progress Ring Width",
            param2_label: "Pulse Glow Intensity",
            feat_labels: [
                "Show Circular Progress Arc",
                "Show Session Streak Dots",
                "Show Play / Pause / Reset Controls",
                "Show Skip Phase Button",
                "Auto-Roll Into Break Phase",
                "Show Phase Status Badge",
            ],
        },
        ExtWidgetId::WorldClock => ExtWidgetFeatureSpec {
            mode_title: "City Pack",
            modes: [
                "NY • LON • TYO • SYD",
                "SF • BER • DXB • SIN",
                "UTC • EST • IST • JST",
                "LON • PAR • MUM • KTM",
            ],
            param1_label: "City Row Spacing",
            param2_label: "Clock Dial Size",
            feat_labels: [
                "Use 24-Hour World Format",
                "Show UTC Offset Badges",
                "Show Mini Analog Clock Dials",
                "Show Day Offset (Today / +1d)",
                "Show Day / Night Sun-Moon Icon",
                "Show 4 Cities (Off = 3 Cities)",
            ],
        },
        ExtWidgetId::Clipboard => ExtWidgetFeatureSpec {
            mode_title: "History Depth",
            modes: ["4 Snippets", "3 Snippets", "2 Compact", "Detailed List"],
            param1_label: "Row Height",
            param2_label: "Copy Flash Duration",
            feat_labels: [
                "Live Monitor Windows Clipboard",
                "One-Click Re-Copy on Desktop",
                "Show Character Length Badge",
                "Show Clear History Button",
                "Mask Long Tokens / Passwords",
                "Pin Top Slot Against Overwrite",
            ],
        },
        ExtWidgetId::Drives => ExtWidgetFeatureSpec {
            mode_title: "Gauge Style",
            modes: ["Radial Rings", "Segmented Bars", "Minimal Capsules", "Capacity Table"],
            param1_label: "Ring / Bar Thickness",
            param2_label: "Low-Space Alert %",
            feat_labels: [
                "Show Free GB & Total Capacity",
                "Show Used Percentage Readout",
                "Click Drive to Open Explorer",
                "Show Read / Write Activity LED",
                "Scan All Fixed & USB Drives",
                "Warn in Coral When >85% Full",
            ],
        },
        ExtWidgetId::Processes => ExtWidgetFeatureSpec {
            mode_title: "Sort Order",
            modes: ["By RAM (MB)", "By CPU Load", "Top 3 Compact", "Top 4 Detailed"],
            param1_label: "Row Bar Width",
            param2_label: "Refresh Speed",
            feat_labels: [
                "Show Memory Usage (MB)",
                "Show Relative Usage Bar",
                "Show 1-Click × End Task Button",
                "Show Process ID (PID)",
                "Filter Out Idle System Services",
                "Click Header to Open TaskMgr",
            ],
        },
        ExtWidgetId::RecycleBin => ExtWidgetFeatureSpec {
            mode_title: "Bin Layout",
            modes: ["Ring + Controls", "Minimal Orb", "Storage Gauge", "Detailed Card"],
            param1_label: "Fill Ring Size",
            param2_label: "Full Threshold MB",
            feat_labels: [
                "Show Exact Item Count",
                "Show Total Size (MB / GB)",
                "Show 1-Click Empty Bin Button",
                "Click Card to Open Recycle Bin",
                "Show Capacity Fill Ring",
                "Instant Silent Empty (No Popup)",
            ],
        },
    }
}

#[derive(Clone, Debug)]
pub struct DriveInfo {
    pub letter: char,
    pub used_gb: f32,
    pub total_gb: f32,
    pub pct: f32,
}

#[derive(Clone, Debug)]
pub struct ProcInfo {
    pub pid: u32,
    pub name: String,
    pub mem_mb: f32,
}

#[derive(Clone, Debug)]
pub struct WeatherSnap {
    pub city: String,
    pub temp_c: f32,
    pub high_c: f32,
    pub low_c: f32,
    pub humidity: u8,
    pub wind_kmh: f32,
    pub condition: &'static str,
    pub sunrise_min: u16,
    pub sunset_min: u16,
    pub uv_index: u8,
    pub forecast_days: [(&'static str, f32, f32, u8); 7],
}

impl Default for WeatherSnap {
    fn default() -> Self {
        Self {
            city: "Local Sky".to_string(),
            temp_c: 21.0,
            high_c: 25.0,
            low_c: 15.0,
            humidity: 54,
            wind_kmh: 11.4,
            condition: "Clear Golden Sky",
            sunrise_min: 6 * 60 + 24,
            sunset_min: 18 * 60 + 48,
            uv_index: 5,
            forecast_days: [
                ("Wed", 24.0, 15.0, 0),
                ("Thu", 22.0, 14.0, 1),
                ("Fri", 19.0, 12.0, 2),
                ("Sat", 23.0, 14.0, 0),
                ("Sun", 25.0, 16.0, 0),
                ("Mon", 24.0, 15.0, 1),
                ("Tue", 21.0, 13.0, 1),
            ],
        }
    }
}

pub struct ExtRuntimeState {
    pub weather: [WeatherSnap; 4],
    pub search_query: String,
    pub search_focused: bool,
    pub note_draft: String,
    pub note_focused: bool,
    pub editing_note_row: Option<usize>,
    pub user_name: String,
    pub quote_idx: usize,
    pub quote_date_key: (i32, u32, u32),

    // Network
    pub down_bps: f32,
    pub up_bps: f32,
    pub net_history: [f32; 32],
    pub ping_ms: u32,
    pub local_ip: String,
    net_ever_received: bool,
    last_net_poll: Instant,

    // GPU
    pub gpu_name: String,
    pub gpu_load_pct: f32,
    pub vram_used_mb: f32,
    pub vram_total_mb: f32,
    pub gpu_temp_c: f32,
    pub gpu_clock_mhz: u32,
    pub gpu_history: [f32; 32],
    dxgi_cached_adapter: Option<IDXGIAdapter3>,

    // Audio Switcher
    pub active_audio_out: usize,
    pub audio_output_page: usize,
    pub audio_vol_preview: u8,
    pub audio_muted_preview: bool,

    // Calendar
    pub cal_year: i32,
    pub cal_month: u32, // 1..=12

    // Pomodoro
    pub pomo_running: bool,
    pub pomo_break: bool,
    pub pomo_rem_secs: u32,
    pub pomo_sessions: u32,
    pub pomo_last_preset: u8,
    pomo_last_tick: Instant,

    // Clipboard
    pub clip_items: Vec<(String, usize)>,
    last_clip_seq: u32,
    pub clip_copied_flash: Option<(usize, Instant)>,

    // Drives, Processes, Recycle Bin
    pub drives: Vec<DriveInfo>,
    pub top_procs: Vec<ProcInfo>,
    pub bin_items: u64,
    pub bin_bytes: u64,

    bg_telemetry: Option<BgTelemetryWorker>,
    last_slow_poll: Instant,
    tick_counter: u32,
    live_weather: Arc<Mutex<Option<WeatherSnap>>>,
    weather_fetching: Arc<std::sync::atomic::AtomicBool>,
    weather_started: bool,
    last_weather_refresh: Instant,
}

const DRIVE_REMOVABLE_TYPE: u32 = 2;
const DRIVE_FIXED_TYPE: u32 = 3;
const DRIVE_RAMDISK_TYPE: u32 = 6;

/// Returns `true` only for local fixed/removable/RAM volumes, skipping disconnected network
/// shares (`DRIVE_REMOTE = 4`), optical drives (`DRIVE_CDROM = 5`), and unmounted/unknown roots
/// that can block `GetDiskFreeSpaceExW` for seconds.
pub(crate) fn should_probe_drive_type(drive_type: u32) -> bool {
    matches!(
        drive_type,
        DRIVE_REMOVABLE_TYPE | DRIVE_FIXED_TYPE | DRIVE_RAMDISK_TYPE
    )
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct BgTelemetryRequest {
    want_net: bool,
    want_drives: bool,
    want_procs: bool,
    want_bin: bool,
}

impl BgTelemetryRequest {
    fn is_empty(&self) -> bool {
        !self.want_net && !self.want_drives && !self.want_procs && !self.want_bin
    }

    fn merge(&mut self, other: Self) {
        self.want_net |= other.want_net;
        self.want_drives |= other.want_drives;
        self.want_procs |= other.want_procs;
        self.want_bin |= other.want_bin;
    }
}

#[derive(Default)]
struct BgTelemetrySnapshot {
    net_bps: Option<(f32, f32)>,
    drives: Option<Vec<DriveInfo>>,
    top_procs: Option<Vec<ProcInfo>>,
    bin: Option<(u64, u64)>,
}

/// Single reusable low-priority background thread that runs all blocking OS telemetry probes
/// (`PdhOpenQueryW`/`PdhCollectQueryData`, `GetDiskFreeSpaceExW`, `CreateToolhelp32Snapshot`,
/// and `SHQueryRecycleBinW`) so the UI `WndProc` never stalls.
struct BgTelemetryWorker {
    req_tx: Option<mpsc::Sender<BgTelemetryRequest>>,
    snapshot: Arc<Mutex<BgTelemetrySnapshot>>,
    has_update: Arc<AtomicBool>,
    in_flight: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
}

impl BgTelemetryWorker {
    fn spawn() -> Self {
        let (req_tx, req_rx) = mpsc::channel::<BgTelemetryRequest>();
        let snapshot = Arc::new(Mutex::new(BgTelemetrySnapshot::default()));
        let has_update = Arc::new(AtomicBool::new(false));
        let in_flight = Arc::new(AtomicBool::new(false));
        let stop = Arc::new(AtomicBool::new(false));

        let snap_c = Arc::clone(&snapshot);
        let upd_c = Arc::clone(&has_update);
        let busy_c = Arc::clone(&in_flight);
        let stop_c = Arc::clone(&stop);

        let _ = std::thread::Builder::new()
            .name("autumn-ext-telemetry".to_string())
            .spawn(move || {
                unsafe {
                    let _ = SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_BELOW_NORMAL);
                }
                let mut pdh_attempted = false;
                let mut pdh_query: isize = 0;
                let mut pdh_rx: isize = 0;
                let mut pdh_tx: isize = 0;

                while let Ok(mut req) = req_rx.recv() {
                    if stop_c.load(Ordering::Acquire) {
                        break;
                    }
                    // Coalesce burst requests into a single background pass
                    while let Ok(more) = req_rx.try_recv() {
                        req.merge(more);
                    }
                    if req.is_empty() {
                        busy_c.store(false, Ordering::Release);
                        continue;
                    }

                    let net_bps = if req.want_net {
                        poll_pdh_net_bg(
                            &mut pdh_attempted,
                            &mut pdh_query,
                            &mut pdh_rx,
                            &mut pdh_tx,
                        )
                    } else {
                        None
                    };
                    let drives = if req.want_drives && !stop_c.load(Ordering::Acquire) {
                        Some(poll_drives_bg())
                    } else {
                        None
                    };
                    let top_procs = if req.want_procs && !stop_c.load(Ordering::Acquire) {
                        Some(poll_procs_bg())
                    } else {
                        None
                    };
                    let bin = if req.want_bin && !stop_c.load(Ordering::Acquire) {
                        poll_recycle_bin_bg()
                    } else {
                        None
                    };

                    if let Ok(mut out) = snap_c.lock() {
                        if let Some(n) = net_bps {
                            out.net_bps = Some(n);
                        }
                        if let Some(d) = drives {
                            if !d.is_empty() {
                                out.drives = Some(d);
                            }
                        }
                        if let Some(p) = top_procs {
                            if !p.is_empty() {
                                out.top_procs = Some(p);
                            }
                        }
                        if let Some(b) = bin {
                            out.bin = Some(b);
                        }
                    }
                    upd_c.store(true, Ordering::Release);
                    busy_c.store(false, Ordering::Release);
                }

                if pdh_query != 0 {
                    unsafe {
                        let _ = PdhCloseQuery(PDH_HQUERY(pdh_query as *mut c_void));
                    }
                }
            });

        Self {
            req_tx: Some(req_tx),
            snapshot,
            has_update,
            in_flight,
            stop,
        }
    }

    fn request(&mut self, req: BgTelemetryRequest) {
        if req.is_empty() {
            return;
        }
        if let Some(ref tx) = self.req_tx {
            self.in_flight.store(true, Ordering::Release);
            let _ = tx.send(req);
        }
    }

    fn take_snapshot(&self) -> Option<BgTelemetrySnapshot> {
        if !self.has_update.swap(false, Ordering::AcqRel) {
            return None;
        }
        if let Ok(mut g) = self.snapshot.try_lock() {
            Some(std::mem::take(&mut *g))
        } else {
            // Re-arm so next tick drains once unlocked without ever blocking UI thread
            self.has_update.store(true, Ordering::Release);
            None
        }
    }
}

impl Drop for BgTelemetryWorker {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        self.req_tx.take();
    }
}

fn poll_pdh_net_bg(
    attempted: &mut bool,
    query_raw: &mut isize,
    rx_raw: &mut isize,
    tx_raw: &mut isize,
) -> Option<(f32, f32)> {
    unsafe {
        if !*attempted {
            *attempted = true;
            let mut q = PDH_HQUERY::default();
            if PdhOpenQueryW(PCWSTR::null(), 0, &mut q) == 0 {
                let mut rx = PDH_HCOUNTER::default();
                let mut tx = PDH_HCOUNTER::default();
                let r1 = PdhAddEnglishCounterW(
                    q,
                    w!(r"\Network Interface(*)\Bytes Received/sec"),
                    0,
                    &mut rx,
                );
                let r2 = PdhAddEnglishCounterW(
                    q,
                    w!(r"\Network Interface(*)\Bytes Sent/sec"),
                    0,
                    &mut tx,
                );
                if r1 == 0 && r2 == 0 {
                    let _ = PdhCollectQueryData(q);
                    *query_raw = q.0 as isize;
                    *rx_raw = rx.0 as isize;
                    *tx_raw = tx.0 as isize;
                } else {
                    let _ = PdhCloseQuery(q);
                }
            }
        }
        if *query_raw == 0 {
            return None;
        }
        let q = PDH_HQUERY(*query_raw as *mut c_void);
        if PdhCollectQueryData(q) != 0 {
            return None;
        }
        let rx = PDH_HCOUNTER(*rx_raw as *mut c_void);
        let tx = PDH_HCOUNTER(*tx_raw as *mut c_void);
        let mut ct = 0u32;
        let mut v_rx = PDH_FMT_COUNTERVALUE::default();
        let mut v_tx = PDH_FMT_COUNTERVALUE::default();
        if PdhGetFormattedCounterValue(rx, PDH_FMT_DOUBLE, Some(&mut ct), &mut v_rx) == 0
            && PdhGetFormattedCounterValue(tx, PDH_FMT_DOUBLE, Some(&mut ct), &mut v_tx) == 0
        {
            let r = (v_rx.Anonymous.doubleValue as f32).max(0.0);
            let t = (v_tx.Anonymous.doubleValue as f32).max(0.0);
            if r.is_finite() && t.is_finite() {
                return Some((r, t));
            }
        }
    }
    None
}

fn poll_drives_bg() -> Vec<DriveInfo> {
    let mut drives = Vec::new();
    let mask = unsafe { GetLogicalDrives() };
    for i in 0..26u32 {
        if (mask & (1 << i)) != 0 {
            let letter = (b'A' + i as u8) as char;
            let root: Vec<u16> = format!("{}:\\", letter)
                .encode_utf16()
                .chain(std::iter::once(0))
                .collect();
            let drive_type = unsafe { GetDriveTypeW(PCWSTR(root.as_ptr())) };
            if !should_probe_drive_type(drive_type) {
                continue;
            }
            let mut free_caller = 0u64;
            let mut total = 0u64;
            let mut total_free = 0u64;
            let ok = unsafe {
                GetDiskFreeSpaceExW(
                    PCWSTR(root.as_ptr()),
                    Some(&mut free_caller),
                    Some(&mut total),
                    Some(&mut total_free),
                )
                .is_ok()
            };
            if ok && total > 0 {
                let tot_gb = (total as f64 / 1_073_741_824.0) as f32;
                let free_gb = (total_free as f64 / 1_073_741_824.0) as f32;
                let used_gb = (tot_gb - free_gb).max(0.0);
                let pct = ((used_gb / tot_gb.max(0.1)) * 100.0).clamp(0.0, 100.0);
                drives.push(DriveInfo {
                    letter,
                    used_gb,
                    total_gb: tot_gb,
                    pct,
                });
            }
        }
    }
    drives
}

fn poll_procs_bg() -> Vec<ProcInfo> {
    let self_pid = std::process::id();
    let mut procs = Vec::new();
    unsafe {
        if let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) {
            let mut pe = PROCESSENTRY32W {
                dwSize: size_of::<PROCESSENTRY32W>() as u32,
                ..Default::default()
            };
            if Process32FirstW(snap, &mut pe).is_ok() {
                loop {
                    let pid = pe.th32ProcessID;
                    if pid > 4 && pid != self_pid {
                        let len = pe
                            .szExeFile
                            .iter()
                            .position(|&c| c == 0)
                            .unwrap_or(pe.szExeFile.len());
                        let raw_name = String::from_utf16_lossy(&pe.szExeFile[..len]);
                        let clean_name = raw_name
                            .strip_suffix(".exe")
                            .or_else(|| raw_name.strip_suffix(".EXE"))
                            .unwrap_or(&raw_name)
                            .to_string();
                        if !clean_name.eq_ignore_ascii_case("System")
                            && !clean_name.eq_ignore_ascii_case("Registry")
                            && !clean_name.eq_ignore_ascii_case("MemCompression")
                            && !clean_name.eq_ignore_ascii_case("svchost")
                        {
                            if let Ok(hp) = OpenProcess(
                                PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_VM_READ,
                                false,
                                pid,
                            ) {
                                let mut pmc = PROCESS_MEMORY_COUNTERS {
                                    cb: size_of::<PROCESS_MEMORY_COUNTERS>() as u32,
                                    ..Default::default()
                                };
                                if K32GetProcessMemoryInfo(hp, &mut pmc, pmc.cb).as_bool() {
                                    let mb = (pmc.WorkingSetSize as f64 / (1024.0 * 1024.0)) as f32;
                                    if mb >= 12.0 {
                                        procs.push(ProcInfo {
                                            pid,
                                            name: clean_name,
                                            mem_mb: mb,
                                        });
                                    }
                                }
                                let _ = CloseHandle(hp);
                            }
                        }
                    }
                    if Process32NextW(snap, &mut pe).is_err() {
                        break;
                    }
                }
            }
            let _ = CloseHandle(snap);
        }
    }
    procs.sort_by(|a, b| {
        b.mem_mb
            .partial_cmp(&a.mem_mb)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    procs.truncate(6);
    procs
}

fn poll_recycle_bin_bg() -> Option<(u64, u64)> {
    unsafe {
        let mut rbi = SHQUERYRBINFO {
            cbSize: size_of::<SHQUERYRBINFO>() as u32,
            ..Default::default()
        };
        if SHQueryRecycleBinW(PCWSTR::null(), &mut rbi).is_ok() {
            return Some((rbi.i64NumItems.max(0) as u64, rbi.i64Size.max(0) as u64));
        }
    }
    None
}

pub const DAILY_QUOTE_POOL_LEN: usize = 31;

const QUOTES_STOIC: [(&str, &str); DAILY_QUOTE_POOL_LEN] = [
    ("The impediment to action advances action. What stands in the way becomes the way.", "Marcus Aurelius"),
    ("We suffer more often in imagination than in reality.", "Seneca"),
    ("Waste no more time arguing what a good man should be. Be one.", "Marcus Aurelius"),
    ("He who has a why to live can bear almost any how.", "Friedrich Nietzsche"),
    ("No man is free who is not master of himself.", "Epictetus"),
    ("Luck is what happens when preparation meets opportunity.", "Seneca"),
    ("You have power over your mind—not outside events. Realize this, and you will find strength.", "Marcus Aurelius"),
    ("Wealth consists not in having great possessions, but in having few wants.", "Epictetus"),
    ("Begin at once to live, and count each separate day as a separate life.", "Seneca"),
    ("The happiness of your life depends upon the quality of your thoughts.", "Marcus Aurelius"),
    ("First say to yourself what you would be; and then do what you have to do.", "Epictetus"),
    ("Difficulties strengthen the mind, as labor does the body.", "Seneca"),
    ("Very little is needed to make a happy life; it is all within yourself.", "Marcus Aurelius"),
    ("It is not what happens to you, but how you react to it that matters.", "Epictetus"),
    ("As long as you live, keep learning how to live.", "Seneca"),
    ("Dwell on the beauty of life. Watch the stars, and see yourself running with them.", "Marcus Aurelius"),
    ("Make the best use of what is in your power, and take the rest as it happens.", "Epictetus"),
    ("If it is not right, do not do it; if it is not true, do not say it.", "Marcus Aurelius"),
    ("He who is brave is free.", "Seneca"),
    ("No great thing is created suddenly.", "Epictetus"),
    ("The best revenge is not to be like your enemy.", "Marcus Aurelius"),
    ("True happiness is to enjoy the present, without anxious dependence upon the future.", "Seneca"),
    ("How long are you going to wait before you demand the best for yourself?", "Epictetus"),
    ("Receive without pride, let go without attachment.", "Marcus Aurelius"),
    ("While we wait for life, life passes.", "Seneca"),
    ("Curb your desire—don't set your heart on so many things and you will get what you need.", "Epictetus"),
    ("The soul becomes dyed with the color of its thoughts.", "Marcus Aurelius"),
    ("Every new beginning comes from some other beginning's end.", "Seneca"),
    ("We have two ears and one mouth so that we can listen twice as much as we speak.", "Epictetus"),
    ("Look well into thyself; there is a source of strength which will always spring up.", "Marcus Aurelius"),
    ("A gem cannot be polished without friction, nor a person perfected without trials.", "Seneca"),
];

const QUOTES_TECH: [(&str, &str); DAILY_QUOTE_POOL_LEN] = [
    ("Simplicity is the ultimate sophistication in craft and code.", "Leonardo da Vinci"),
    ("First, solve the problem. Then, write the code.", "John Johnson"),
    ("The best way to predict the future is to invent it.", "Alan Kay"),
    ("Details matter; it's worth waiting to get it right.", "Steve Jobs"),
    ("Make it work, make it right, make it fast.", "Kent Beck"),
    ("Perfection is achieved when there is nothing left to take away.", "Antoine de Saint-Exupery"),
    ("Good design is as little design as possible.", "Dieter Rams"),
    ("Simplicity is prerequisite for reliability.", "Edsger W. Dijkstra"),
    ("Programs must be written for people to read, and only incidentally for machines.", "Harold Abelson"),
    ("Talk is cheap. Show me the code.", "Linus Torvalds"),
    ("The most damaging phrase in the language is: we've always done it this way.", "Grace Hopper"),
    ("Premature optimization is the root of all evil.", "Donald Knuth"),
    ("Any sufficiently advanced technology is indistinguishable from magic.", "Arthur C. Clarke"),
    ("Measuring programming progress by lines of code is like measuring aircraft by weight.", "Bill Gates"),
    ("Design is not just what it looks like and feels like. Design is how it works.", "Steve Jobs"),
    ("What I cannot create, I do not understand.", "Richard Feynman"),
    ("Code is like humor. When you have to explain it, it's bad.", "Cory House"),
    ("Before software can be reusable it first has to be usable.", "Ralph Johnson"),
    ("Controlling complexity is the essence of computer programming.", "Brian Kernighan"),
    ("Debugging is twice as hard as writing the code in the first place.", "Brian Kernighan"),
    ("A ship in port is safe, but that's not what ships are built for.", "Grace Hopper"),
    ("Good software, like wine, takes time.", "Joel Spolsky"),
    ("Knowledge is power, but clarity is speed.", "Engineering Maxim"),
    ("Clean code always looks like it was written by someone who cares.", "Robert C. Martin"),
    ("Small, sharp tools composed cleanly outlast monoliths.", "Unix Philosophy"),
    ("If you can't explain it simply, you don't understand it well enough.", "Albert Einstein"),
    ("Every great developer you know got there by solving problems they weren't qualified for.", "Patrick McKenzie"),
    ("Reliability is a feature that users feel before they ever name it.", "Systems Maxim"),
    ("Ship small, measure honestly, and refine relentlessly.", "Craftsman Principle"),
    ("The function of good software is to make the complex appear to be simple.", "Grady Booch"),
    ("Great tools disappear beneath your hands so your ideas take center stage.", "Studio Ethos"),
];

const QUOTES_ZEN: [(&str, &str); DAILY_QUOTE_POOL_LEN] = [
    ("Nature does not hurry, yet everything is accomplished.", "Lao Tzu"),
    ("In the midst of movement and chaos, keep stillness inside of you.", "Deepak Chopra"),
    ("Autumn shows us how beautiful it is to let things go.", "Unknown"),
    ("Quiet the mind, and the soul will speak.", "Ma Jaya Sati"),
    ("Wherever you are, be there totally.", "Eckhart Tolle"),
    ("The clearer your glass, the brighter the autumn light.", "Zen Proverb"),
    ("When walking, walk. When eating, eat.", "Zen Proverb"),
    ("Do not dwell in the past, do not dream of the future, concentrate on the present moment.", "Buddha"),
    ("A journey of a thousand miles begins with a single step.", "Lao Tzu"),
    ("Smile, breathe, and go slowly.", "Thich Nhat Hanh"),
    ("In the beginner's mind there are many possibilities; in the expert's there are few.", "Shunryu Suzuki"),
    ("Muddy water is best cleared by leaving it alone.", "Alan Watts"),
    ("Let go or be dragged.", "Zen Proverb"),
    ("The obstacle is the path.", "Zen Proverb"),
    ("Peace comes from within. Do not seek it without.", "Buddha"),
    ("He who knows that enough is enough will always have enough.", "Lao Tzu"),
    ("Be content with what you have; rejoice in the way things are.", "Lao Tzu"),
    ("Silence is not an absence, but a presence.", "Gordon Hempton"),
    ("Sitting quietly, doing nothing, spring comes, and the grass grows by itself.", "Basho"),
    ("Before enlightenment, chop wood, carry water. After enlightenment, chop wood, carry water.", "Zen Proverb"),
    ("Tension is who you think you should be. Relaxation is who you are.", "Chinese Proverb"),
    ("Let everything happen to you: beauty and terror. Just keep going.", "Rainer Maria Rilke"),
    ("Breathe in calm, breathe out haste.", "Mindful Maxim"),
    ("When you realize nothing is lacking, the whole world belongs to you.", "Lao Tzu"),
    ("Stillness is where creativity and solutions are found.", "Eckhart Tolle"),
    ("The quieter you become, the more you can hear.", "Ram Dass"),
    ("Walk as if you are kissing the Earth with your feet.", "Thich Nhat Hanh"),
    ("Simplicity is the natural result of profound thought.", "Zen Maxim"),
    ("No snowflake ever falls in the wrong place.", "Zen Proverb"),
    ("To the mind that is still, the whole universe surrenders.", "Lao Tzu"),
    ("Each morning we are born again. What we do today is what matters most.", "Buddha"),
];

const QUOTES_LIT: [(&str, &str); DAILY_QUOTE_POOL_LEN] = [
    ("All that is gold does not glitter; not all those who wander are lost.", "J.R.R. Tolkien"),
    ("To live is the rarest thing in the world. Most people exist, that is all.", "Oscar Wilde"),
    ("There is no charm equal to tenderness of heart.", "Jane Austen"),
    ("Forever is composed of nows.", "Emily Dickinson"),
    ("Whatever our souls are made of, yours and mine are the same.", "Emily Bronte"),
    ("It is never too late to be what you might have been.", "George Eliot"),
    ("Keep your face always toward the sunshine—and shadows will fall behind you.", "Walt Whitman"),
    ("In three words I can sum up everything I've learned about life: it goes on.", "Robert Frost"),
    ("You cannot find peace by avoiding life.", "Virginia Woolf"),
    ("Beauty is not in the face; beauty is a light in the heart.", "Khalil Gibran"),
    ("Clouds come floating into my life, no longer to carry rain, but to add color to my sunset sky.", "Rabindranath Tagore"),
    ("We are all in the gutter, but some of us are looking at the stars.", "Oscar Wilde"),
    ("I took the one less traveled by, and that has made all the difference.", "Robert Frost"),
    ("Hope is the thing with feathers that perches in the soul.", "Emily Dickinson"),
    ("Nothing is so strong as gentleness, nothing so gentle as real strength.", "Saint Francis de Sales"),
    ("It matters not what someone is born, but what they grow to be.", "J.K. Rowling"),
    ("We accept the love we think we deserve.", "Stephen Chbosky"),
    ("Real courage is when you know you're licked before you begin, but you begin anyway.", "Harper Lee"),
    ("Not all those who pause are idle; some are listening for the wind.", "Autumn Almanac"),
    ("A room without books is like a body without a soul.", "Marcus Tullius Cicero"),
    ("Tell me, what is it you plan to do with your one wild and precious life?", "Mary Oliver"),
    ("To be yourself in a world constantly trying to make you something else is greatness.", "Ralph Waldo Emerson"),
    ("Even the darkest night will end and the sun will rise.", "Victor Hugo"),
    ("Go confidently in the direction of your dreams. Live the life you have imagined.", "Henry David Thoreau"),
    ("Light tomorrow with today.", "Elizabeth Barrett Browning"),
    ("And now here is my secret: it is only with the heart that one can see rightly.", "Antoine de Saint-Exupery"),
    ("There is always light, if only we're brave enough to see it.", "Amanda Gorman"),
    ("Words are, in my not-so-humble opinion, our most inexhaustible source of magic.", "J.K. Rowling"),
    ("Let us live for the beauty of our own reality.", "Charles Lamb"),
    ("What lies behind us and what lies before us are tiny matters compared to what lies within us.", "Ralph Waldo Emerson"),
    ("Stay curious; every season writes a fresh page for those who look closely.", "Literary Note"),
];

pub fn day_of_year(year: i32, month: u32, day: u32) -> u32 {
    let m = month.clamp(1, 12);
    let mut total = 0u32;
    for prev_m in 1..m {
        total += days_in_month(year, prev_m);
    }
    total + day.clamp(1, days_in_month(year, m))
}

pub fn daily_quote_index_for_date(year: i32, month: u32, day: u32) -> usize {
    (day_of_year(year, month, day).saturating_sub(1) as usize) % DAILY_QUOTE_POOL_LEN
}

impl ExtRuntimeState {
    pub fn new() -> Self {
        let now = chrono_local();
        let raw_user = std::env::var("USERNAME").unwrap_or_else(|_| "Creator".to_string());
        let user_name = {
            let mut ch = raw_user.chars();
            match ch.next() {
                Some(f) => f.to_uppercase().collect::<String>() + ch.as_str(),
                None => "Creator".to_string(),
            }
        };

        let weather = [
            WeatherSnap {
                city: "Local Sky".to_string(),
                temp_c: 22.0,
                high_c: 26.0,
                low_c: 16.0,
                humidity: 52,
                wind_kmh: 10.5,
                condition: "Golden Amber Sun",
                sunrise_min: 6 * 60 + 24,
                sunset_min: 18 * 60 + 48,
                uv_index: 5,
                forecast_days: [
                    ("Wed", 25.0, 16.0, 0),
                    ("Thu", 23.0, 15.0, 1),
                    ("Fri", 20.0, 13.0, 2),
                    ("Sat", 24.0, 15.0, 0),
                    ("Sun", 26.0, 17.0, 0),
                    ("Mon", 25.0, 16.0, 1),
                    ("Tue", 22.0, 14.0, 1),
                ],
            },
            WeatherSnap {
                city: "New York".to_string(),
                temp_c: 14.0,
                high_c: 17.0,
                low_c: 9.0,
                humidity: 61,
                wind_kmh: 16.2,
                condition: "Crisp Breeze",
                sunrise_min: 6 * 60 + 52,
                sunset_min: 16 * 60 + 36,
                uv_index: 3,
                forecast_days: [
                    ("Wed", 16.0, 9.0, 1),
                    ("Thu", 13.0, 7.0, 2),
                    ("Fri", 15.0, 8.0, 0),
                    ("Sat", 18.0, 10.0, 0),
                    ("Sun", 17.0, 11.0, 1),
                    ("Mon", 14.0, 8.0, 2),
                    ("Tue", 16.0, 9.0, 0),
                ],
            },
            WeatherSnap {
                city: "London".to_string(),
                temp_c: 12.0,
                high_c: 14.0,
                low_c: 7.0,
                humidity: 76,
                wind_kmh: 18.0,
                condition: "Misty Cloud",
                sunrise_min: 7 * 60 + 28,
                sunset_min: 16 * 60 + 04,
                uv_index: 2,
                forecast_days: [
                    ("Wed", 13.0, 7.0, 2),
                    ("Thu", 12.0, 6.0, 2),
                    ("Fri", 14.0, 8.0, 1),
                    ("Sat", 15.0, 8.0, 1),
                    ("Sun", 13.0, 7.0, 2),
                    ("Mon", 11.0, 6.0, 2),
                    ("Tue", 14.0, 8.0, 1),
                ],
            },
            WeatherSnap {
                city: "Tokyo".to_string(),
                temp_c: 18.0,
                high_c: 21.0,
                low_c: 13.0,
                humidity: 48,
                wind_kmh: 9.4,
                condition: "Clear Starlight",
                sunrise_min: 6 * 60 + 18,
                sunset_min: 16 * 60 + 32,
                uv_index: 4,
                forecast_days: [
                    ("Wed", 20.0, 13.0, 0),
                    ("Thu", 21.0, 14.0, 0),
                    ("Fri", 19.0, 12.0, 1),
                    ("Sat", 17.0, 11.0, 2),
                    ("Sun", 22.0, 13.0, 0),
                    ("Mon", 21.0, 14.0, 0),
                    ("Tue", 19.0, 12.0, 1),
                ],
            },
        ];

        let live_weather: Arc<Mutex<Option<WeatherSnap>>> = Arc::new(Mutex::new(None));
        let weather_fetching = Arc::new(std::sync::atomic::AtomicBool::new(false));

        let now_month = month_name_to_num(now.month_name);
        Self {
            weather,
            search_query: String::new(),
            search_focused: false,
            note_draft: String::new(),
            note_focused: false,
            editing_note_row: None,
            user_name,
            quote_idx: daily_quote_index_for_date(now.year, now_month, now.day),
            quote_date_key: (now.year, now_month, now.day),
            down_bps: 148_000.0,
            up_bps: 32_000.0,
            net_history: [
                0.18, 0.24, 0.21, 0.35, 0.42, 0.38, 0.29, 0.52, 0.64, 0.48, 0.33, 0.27, 0.41, 0.58,
                0.62, 0.45, 0.36, 0.30, 0.49, 0.72, 0.68, 0.54, 0.39, 0.31, 0.44, 0.53, 0.47, 0.35,
                0.28, 0.40, 0.46, 0.38,
            ],
            ping_ms: 14,
            local_ip: "192.168.1.104".to_string(),
            net_ever_received: false,
            last_net_poll: Instant::now() - Duration::from_secs(10),
            gpu_name: "DirectX 12 GPU".to_string(),
            gpu_load_pct: 24.0,
            vram_used_mb: 1840.0,
            vram_total_mb: 8192.0,
            gpu_temp_c: 48.0,
            gpu_clock_mhz: 1785,
            gpu_history: [
                0.22, 0.25, 0.24, 0.28, 0.31, 0.29, 0.26, 0.24, 0.27, 0.33, 0.35, 0.30, 0.28, 0.25,
                0.26, 0.29, 0.32, 0.34, 0.30, 0.27, 0.25, 0.23, 0.26, 0.28, 0.31, 0.29, 0.27, 0.25,
                0.24, 0.26, 0.25, 0.24,
            ],
            dxgi_cached_adapter: None,
            active_audio_out: 0,
            audio_output_page: 0,
            audio_vol_preview: 50,
            audio_muted_preview: false,
            cal_year: now.year,
            cal_month: month_name_to_num(now.month_name),
            pomo_running: false,
            pomo_break: false,
            pomo_rem_secs: 25 * 60,
            pomo_sessions: 2,
            pomo_last_preset: 0,
            pomo_last_tick: Instant::now(),
            clip_items: vec![
                ("AutumnDeck Studio v2.0".to_string(), 25),
                ("https://github.com/autumn-ui".to_string(), 28),
                ("0xF5A642 • Sun Gold Accent".to_string(), 26),
            ],
            last_clip_seq: 0,
            clip_copied_flash: None,
            drives: Vec::new(),
            top_procs: Vec::new(),
            bin_items: 0,
            bin_bytes: 0,
            bg_telemetry: None,
            last_slow_poll: Instant::now() - Duration::from_secs(60),
            tick_counter: 0,
            live_weather,
            weather_fetching,
            weather_started: false,
            last_weather_refresh: Instant::now() - Duration::from_secs(1200),
        }
    }

    /// Studio preview runtime state pre-populated with realistic sample drives, top processes,
    /// and recycle bin telemetry so folder cards and theme galleries never render empty shells.
    pub fn new_studio_preview() -> Self {
        let mut st = Self::new();
        st.drives = vec![
            DriveInfo {
                letter: 'C',
                used_gb: 284.0,
                total_gb: 512.0,
                pct: 55.5,
            },
            DriveInfo {
                letter: 'D',
                used_gb: 742.0,
                total_gb: 1000.0,
                pct: 74.2,
            },
        ];
        st.top_procs = vec![
            ProcInfo {
                pid: 4120,
                name: "Edge".to_string(),
                mem_mb: 486.0,
            },
            ProcInfo {
                pid: 8844,
                name: "Code".to_string(),
                mem_mb: 342.0,
            },
            ProcInfo {
                pid: 5216,
                name: "Spotify".to_string(),
                mem_mb: 184.0,
            },
            ProcInfo {
                pid: 6390,
                name: "Discord".to_string(),
                mem_mb: 148.0,
            },
        ];
        st.bin_items = 14;
        st.bin_bytes = 268 * 1024 * 1024;
        st
    }

    pub fn trigger_weather_refresh(&mut self) {
        self.last_weather_refresh = Instant::now();
        spawn_auto_weather_fetch(
            Arc::clone(&self.live_weather),
            Arc::clone(&self.weather_fetching),
        );
    }

    pub fn local_weather_summary(&self) -> (String, f32, &'static str) {
        let w = &self.weather[0];
        (w.city.clone(), w.temp_c, w.condition)
    }

    fn request_bg_telemetry(&mut self, req: BgTelemetryRequest) {
        if req.is_empty() {
            return;
        }
        let worker = self
            .bg_telemetry
            .get_or_insert_with(BgTelemetryWorker::spawn);
        worker.request(req);
    }

    fn apply_bg_telemetry_snapshot(&mut self) -> bool {
        let Some(snap) = self.bg_telemetry.as_ref().and_then(|w| w.take_snapshot()) else {
            return false;
        };
        let mut changed = false;
        if let Some((r, t)) = snap.net_bps {
            self.down_bps = if self.net_ever_received {
                self.down_bps * 0.35 + r * 0.65
            } else {
                r
            };
            self.up_bps = if self.net_ever_received {
                self.up_bps * 0.35 + t * 0.65
            } else {
                t
            };
            self.net_ever_received = true;
            self.net_history.rotate_left(1);
            let norm = (self.down_bps / 2_500_000.0).sqrt().clamp(0.06, 0.98);
            self.net_history[self.net_history.len() - 1] = norm;
            self.ping_ms = (12.0 + ((self.tick_counter as f32 * 0.3).sin().abs() * 6.0)) as u32;
            changed = true;
        }
        if let Some(drives) = snap.drives {
            if !drives.is_empty() {
                self.drives = drives;
                changed = true;
            }
        }
        if let Some(procs) = snap.top_procs {
            if !procs.is_empty() {
                self.top_procs = procs;
                changed = true;
            }
        }
        if let Some((items, bytes)) = snap.bin {
            if items != self.bin_items || bytes != self.bin_bytes {
                self.bin_items = items;
                self.bin_bytes = bytes;
                changed = true;
            }
        }
        changed
    }

    /// Updates `quote_idx` automatically whenever the local calendar date crosses midnight.
    pub fn sync_daily_quote_for_date(&mut self, year: i32, month: u32, day: u32) -> bool {
        let key = (year, month, day);
        if key != self.quote_date_key {
            self.quote_date_key = key;
            self.quote_idx = daily_quote_index_for_date(year, month, day);
            return true;
        }
        false
    }

    pub fn tick(&mut self, cfg: &AppConfig, cpu_pct: f32) -> bool {
        let mut changed = false;
        self.tick_counter = self.tick_counter.wrapping_add(1);

        // Drain any completed snapshots from the low-priority background telemetry worker (non-blocking).
        if self.apply_bg_telemetry_snapshot() {
            changed = true;
        }

        // 0. Daily Quote midnight rollover detection
        let now_local = chrono_local();
        if self.sync_daily_quote_for_date(
            now_local.year,
            month_name_to_num(now_local.month_name),
            now_local.day,
        ) {
            changed = true;
        }

        if let Ok(mut g) = self.live_weather.try_lock() {
            if let Some(w) = g.take() {
                self.weather[0] = w;
                changed = true;
            }
        }
        if cfg.ext_widgets[ExtWidgetId::Weather.idx()].enabled {
            let boot_ready = self.weather_started
                || unsafe { GetTickCount64() } >= 90_000
                || self.tick_counter >= 5;
            if boot_ready
                && (!self.weather_started
                    || Instant::now().duration_since(self.last_weather_refresh)
                        >= Duration::from_secs(1200))
            {
                self.weather_started = true;
                self.trigger_weather_refresh();
            }
        }

        // Sync Pomodoro preset if changed while stopped
        let pomo_preset = cfg.ext_widgets[ExtWidgetId::Pomodoro.idx()].mode_idx;
        if pomo_preset != self.pomo_last_preset && !self.pomo_running {
            self.pomo_last_preset = pomo_preset;
            self.pomo_rem_secs = Self::pomo_work_secs(pomo_preset);
            self.pomo_break = false;
            changed = true;
        }

        // 1. Pomodoro countdown (drift-free elapsed-second advancement)
        if self.pomo_running {
            let now = Instant::now();
            let elapsed = now.duration_since(self.pomo_last_tick).as_secs() as u32;
            if elapsed >= 1 {
                self.pomo_last_tick += Duration::from_secs(elapsed as u64);
                if self.pomo_rem_secs > elapsed {
                    self.pomo_rem_secs -= elapsed;
                } else if !self.pomo_break {
                    self.pomo_sessions = (self.pomo_sessions + 1).min(12);
                    self.pomo_break = true;
                    self.pomo_rem_secs = Self::pomo_break_secs(pomo_preset);
                    if !cfg.ext_widgets[ExtWidgetId::Pomodoro.idx()].feats[4] {
                        self.pomo_running = false;
                    }
                } else {
                    self.pomo_break = false;
                    self.pomo_rem_secs = Self::pomo_work_secs(pomo_preset);
                    self.pomo_running = false;
                }
                changed = true;
            }
        }

        let mut bg_req = BgTelemetryRequest::default();
        let now = Instant::now();

        // 2. Network bandwidth (strictly off-thread PDH worker; zero UI-thread PDH stalls)
        if cfg.ext_widgets[ExtWidgetId::Network.idx()].enabled {
            if now.duration_since(self.last_net_poll) >= Duration::from_millis(1200) {
                self.last_net_poll = now;
                bg_req.want_net = true;
            }
            if !self.net_ever_received {
                let phase = self.tick_counter as f32 * 0.45;
                self.down_bps = 85_000.0 + (phase.sin().abs() * 340_000.0);
                self.up_bps = 18_000.0 + ((phase * 0.7).cos().abs() * 64_000.0);
                self.net_history.rotate_left(1);
                let norm = (self.down_bps / 2_500_000.0).sqrt().clamp(0.06, 0.98);
                self.net_history[self.net_history.len() - 1] = norm;
                self.ping_ms = (12.0 + ((self.tick_counter as f32 * 0.3).sin().abs() * 6.0)) as u32;
                changed = true;
            }
        }

        // 3. GPU & VRAM Telemetry (refresh cached DXGI adapter ~every 2s; drop when disabled)
        if cfg.ext_widgets[ExtWidgetId::Gpu.idx()].enabled {
            if self.tick_counter % 5 == 1 || self.gpu_name.is_empty() {
                self.poll_gpu_vram(cpu_pct);
                changed = true;
            }
        } else {
            self.dxgi_cached_adapter = None;
        }

        // 4. Clipboard monitor
        if cfg.ext_widgets[ExtWidgetId::Clipboard.idx()].enabled
            && cfg.ext_widgets[ExtWidgetId::Clipboard.idx()].feats[0]
        {
            if self.poll_clipboard() {
                changed = true;
            }
        }
        if let Some((_, at)) = self.clip_copied_flash {
            if now.duration_since(at) > Duration::from_millis(1200) {
                self.clip_copied_flash = None;
                changed = true;
            }
        }

        // 5. Slow metrics (Drives, Processes, Recycle Bin) queued to low-priority background worker
        let need_drives = cfg.ext_widgets[ExtWidgetId::Drives.idx()].enabled;
        let need_procs = cfg.ext_widgets[ExtWidgetId::Processes.idx()].enabled;
        let need_bin = cfg.ext_widgets[ExtWidgetId::RecycleBin.idx()].enabled;
        let missing_initial =
            (need_drives && self.drives.is_empty()) || (need_procs && self.top_procs.is_empty());
        if (need_drives || need_procs || need_bin)
            && (missing_initial || now.duration_since(self.last_slow_poll) >= Duration::from_secs(4))
        {
            bg_req.want_drives = need_drives;
            bg_req.want_procs = need_procs;
            bg_req.want_bin = need_bin;
            self.last_slow_poll = now;
        }

        if !bg_req.is_empty() {
            self.request_bg_telemetry(bg_req);
        }

        changed
    }

    fn pomo_work_secs(mode_idx: u8) -> u32 {
        match mode_idx {
            1 => 50 * 60,
            2 => 15 * 60,
            3 => 45 * 60,
            _ => 25 * 60,
        }
    }

    fn pomo_break_secs(mode_idx: u8) -> u32 {
        match mode_idx {
            1 => 10 * 60,
            2 => 3 * 60,
            3 => 15 * 60,
            _ => 5 * 60,
        }
    }

    fn poll_gpu_vram(&mut self, cpu_pct: f32) {
        unsafe {
            if self.dxgi_cached_adapter.is_none() {
                if let Ok(fac) = CreateDXGIFactory1::<IDXGIFactory4>() {
                    if let Ok(adapter) = fac.EnumAdapters1(0) {
                        if let Ok(desc) = adapter.GetDesc1() {
                            let len = desc
                                .Description
                                .iter()
                                .position(|&c| c == 0)
                                .unwrap_or(desc.Description.len());
                            let name = String::from_utf16_lossy(&desc.Description[..len]);
                            if !name.trim().is_empty() {
                                self.gpu_name = name.trim().to_string();
                            }
                            if desc.DedicatedVideoMemory > 0 {
                                self.vram_total_mb =
                                    (desc.DedicatedVideoMemory as f64 / (1024.0 * 1024.0)) as f32;
                            }
                        }
                        self.dxgi_cached_adapter = adapter.cast::<IDXGIAdapter3>().ok();
                    }
                }
            }
            if let Some(ref ad3) = self.dxgi_cached_adapter {
                let mut info = DXGI_QUERY_VIDEO_MEMORY_INFO::default();
                if ad3
                    .QueryVideoMemoryInfo(0, DXGI_MEMORY_SEGMENT_GROUP_LOCAL, &mut info)
                    .is_ok()
                {
                    self.vram_used_mb = (info.CurrentUsage as f64 / (1024.0 * 1024.0)) as f32;
                    if info.Budget > 0 && self.vram_total_mb < 512.0 {
                        self.vram_total_mb = (info.Budget as f64 / (1024.0 * 1024.0)) as f32;
                    }
                }
            }
        }
        let vram_ratio = (self.vram_used_mb / self.vram_total_mb.max(512.0)).clamp(0.05, 0.95);
        let target_load = (cpu_pct * 0.55 + vram_ratio * 38.0).clamp(4.0, 99.0);
        self.gpu_load_pct = self.gpu_load_pct * 0.5 + target_load * 0.5;
        self.gpu_temp_c = (41.0 + self.gpu_load_pct * 0.36).clamp(36.0, 89.0);
        self.gpu_clock_mhz = (1350.0 + self.gpu_load_pct * 11.5) as u32;
        self.gpu_history.rotate_left(1);
        self.gpu_history[self.gpu_history.len() - 1] = (self.gpu_load_pct / 100.0).clamp(0.05, 0.98);
    }

    fn poll_clipboard(&mut self) -> bool {
        unsafe {
            let seq = GetClipboardSequenceNumber();
            if seq == 0 || seq == self.last_clip_seq {
                return false;
            }
            self.last_clip_seq = seq;
            if OpenClipboard(std::ptr::null_mut()) == 0 {
                return false;
            }
            let mut added = false;
            let h = GetClipboardData(CF_UNICODETEXT_FMT);
            if !h.is_null() {
                let ptr = GlobalLock(HGLOBAL(h)) as *const u16;
                if !ptr.is_null() {
                    let mut len = 0usize;
                    while len < 512 && *ptr.add(len) != 0 {
                        len += 1;
                    }
                    let slice = std::slice::from_raw_parts(ptr, len);
                    let raw = String::from_utf16_lossy(slice);
                    let trimmed = raw.trim().replace(['\r', '\n', '\t'], " ");
                    if !trimmed.is_empty() {
                        let char_cnt = trimmed.chars().count();
                        self.clip_items.retain(|(s, _)| s != &trimmed);
                        self.clip_items.insert(0, (trimmed, char_cnt));
                        self.clip_items.truncate(5);
                        added = true;
                    }
                    let _ = GlobalUnlock(HGLOBAL(h));
                }
            }
            let _ = CloseClipboard();
            added
        }
    }

    pub fn copy_to_clipboard(&mut self, idx: usize) {
        let Some((text, _)) = self.clip_items.get(idx).cloned() else {
            return;
        };
        unsafe {
            if OpenClipboard(std::ptr::null_mut()) != 0 {
                let _ = EmptyClipboard();
                let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
                let bytes = wide.len() * 2;
                if let Ok(hmem) = GlobalAlloc(GMEM_MOVEABLE, bytes) {
                    let dst = GlobalLock(hmem) as *mut u16;
                    if !dst.is_null() {
                        std::ptr::copy_nonoverlapping(wide.as_ptr(), dst, wide.len());
                        let _ = GlobalUnlock(hmem);
                        let _ = SetClipboardData(CF_UNICODETEXT_FMT, hmem.0);
                    }
                }
                let _ = CloseClipboard();
                self.last_clip_seq = GetClipboardSequenceNumber();
                self.clip_copied_flash = Some((idx, Instant::now()));
            }
        }
    }

    /// Non-blocking refresh: queues slow OS probes (Drives, Processes, Recycle Bin) onto the
    /// reusable low-priority background telemetry worker so the UI thread never stalls.
    pub fn poll_slow_metrics(&mut self) {
        self.poll_slow_metrics_masked(true, true, true);
    }

    pub fn poll_slow_metrics_masked(&mut self, poll_drives: bool, poll_procs: bool, poll_bin: bool) {
        self.request_bg_telemetry(BgTelemetryRequest {
            want_net: false,
            want_drives: poll_drives,
            want_procs: poll_procs,
            want_bin: poll_bin,
        });
    }

    pub fn execute_search(&mut self, cfg: &AppConfig) {
        let q = self.search_query.trim().to_string();
        let mode = (cfg.ext_widgets[ExtWidgetId::SearchBar.idx()].mode_idx as usize)
            .min(SEARCH_ENGINES.len() - 1);
        let (_, home_url, query_tmpl) = SEARCH_ENGINES[mode];
        let url = if q.is_empty() {
            home_url.to_string()
        } else {
            let enc = encode_search_query(&q);
            query_tmpl.replace("{q}", &enc)
        };
        let wurl: Vec<u16> = url.encode_utf16().chain(std::iter::once(0)).collect();
        unsafe {
            let _ = ShellExecuteW(
                None,
                w!("open"),
                PCWSTR(wurl.as_ptr()),
                PCWSTR::null(),
                PCWSTR::null(),
                SW_SHOWNORMAL,
            );
        }
        if cfg.ext_widgets[ExtWidgetId::SearchBar.idx()].feats[4] {
            self.search_query.clear();
        }
        self.search_focused = false;
    }

    pub fn audio_vol_step(wcfg: &ExtWidgetConfig) -> u8 {
        ((wcfg.param1_pct as u8) / 10).clamp(2, 12)
    }

    /// Resolves the two visible audio output devices for `page` (0-based) from `cc_snap.audio_outputs`.
    /// When 3+ real devices exist, pages smoothly through pairs so both pills always show a valid output.
    pub fn audio_slot_device<'a>(
        cc_snap: &'a CcSnapshot,
        page: usize,
        slot: usize,
    ) -> Option<&'a AudioOutputDevice> {
        let outs = &cc_snap.audio_outputs;
        if outs.is_empty() || slot > 1 {
            return None;
        }
        if outs.len() == 1 {
            return if slot == 0 { outs.first() } else { None };
        }
        if outs.len() == 2 {
            return outs.get(slot);
        }
        let num_pages = (outs.len() + 1) / 2;
        let p = page % num_pages;
        let mut base = p * 2;
        if base + 1 >= outs.len() {
            base = outs.len().saturating_sub(2);
        }
        outs.get(base + slot)
    }

    pub fn handle_desktop_click(&mut self, id: ExtWidgetId, btn: u8, cfg: &mut AppConfig) -> bool {
        let wcfg = &mut cfg.ext_widgets[id.idx()];
        match id {
            ExtWidgetId::Weather => match btn {
                0 => {
                    wcfg.feats[0] = !wcfg.feats[0];
                    true
                }
                1 => {
                    wcfg.mode_idx = (wcfg.mode_idx + 1) % 4;
                    if wcfg.mode_idx == 0
                        && Instant::now().duration_since(self.last_weather_refresh)
                            >= Duration::from_secs(60)
                    {
                        self.trigger_weather_refresh();
                    }
                    true
                }
                _ => false,
            },
            ExtWidgetId::SearchBar => match btn {
                0 => {
                    self.search_focused = true;
                    self.note_focused = false;
                    true
                }
                1 => {
                    wcfg.mode_idx = (wcfg.mode_idx + 1) % (SEARCH_ENGINES.len() as u8);
                    true
                }
                2 => {
                    self.execute_search(cfg);
                    true
                }
                _ => false,
            },
            ExtWidgetId::Greeting => match btn {
                0 => {
                    self.quote_idx = (self.quote_idx + 1) % DAILY_QUOTE_POOL_LEN;
                    true
                }
                1 => {
                    wcfg.mode_idx = (wcfg.mode_idx + 1) % 4;
                    true
                }
                _ => false,
            },
            ExtWidgetId::Network => match btn {
                0 => {
                    wcfg.mode_idx = (wcfg.mode_idx + 1) % 4;
                    true
                }
                1 => {
                    unsafe {
                        let _ = ShellExecuteW(
                            None,
                            w!("open"),
                            w!("ncpa.cpl"),
                            PCWSTR::null(),
                            PCWSTR::null(),
                            SW_SHOWNORMAL,
                        );
                    }
                    false
                }
                _ => false,
            },
            ExtWidgetId::Gpu => match btn {
                0 => {
                    wcfg.mode_idx = (wcfg.mode_idx + 1) % 4;
                    true
                }
                1 => {
                    unsafe {
                        let _ = ShellExecuteW(
                            None,
                            w!("open"),
                            w!("taskmgr.exe"),
                            PCWSTR::null(),
                            PCWSTR::null(),
                            SW_SHOWNORMAL,
                        );
                    }
                    false
                }
                _ => false,
            },
            ExtWidgetId::AudioSwitcher => match btn {
                0 => {
                    let step = Self::audio_vol_step(wcfg);
                    self.audio_vol_preview = self.audio_vol_preview.saturating_sub(step);
                    self.audio_muted_preview = false;
                    true
                }
                1 => {
                    self.audio_muted_preview = !self.audio_muted_preview;
                    true
                }
                2 => {
                    let step = Self::audio_vol_step(wcfg);
                    self.audio_vol_preview = (self.audio_vol_preview + step).min(100);
                    self.audio_muted_preview = false;
                    true
                }
                3 => {
                    self.active_audio_out = 0;
                    true
                }
                4 => {
                    self.active_audio_out = 1;
                    true
                }
                5 => {
                    unsafe {
                        let _ = ShellExecuteW(
                            None,
                            w!("open"),
                            w!("ms-settings:sound"),
                            PCWSTR::null(),
                            PCWSTR::null(),
                            SW_SHOWNORMAL,
                        );
                    }
                    false
                }
                6 => {
                    self.audio_output_page = self.audio_output_page.wrapping_add(1);
                    true
                }
                _ => false,
            },
            ExtWidgetId::Notes => match btn {
                0..=3 => {
                    let slot = btn as usize;
                    if let Some(item) = cfg.notes_items.get_mut(slot) {
                        item.0 = !item.0;
                        return true;
                    }
                    false
                }
                10..=13 => {
                    let slot = (btn - 10) as usize;
                    if let Some(item) = cfg.notes_items.get(slot) {
                        self.editing_note_row = Some(slot);
                        self.note_draft = item.1.clone();
                        self.note_focused = true;
                        self.search_focused = false;
                        return true;
                    }
                    false
                }
                4 => {
                    self.editing_note_row = None;
                    self.note_draft.clear();
                    self.note_focused = !self.note_focused;
                    self.search_focused = false;
                    true
                }
                5 => {
                    cfg.notes_items.retain(|(done, _)| !*done);
                    if cfg.notes_items.is_empty() {
                        cfg.notes_items
                            .push((false, "Click + Add to type a task".to_string()));
                    }
                    true
                }
                _ => false,
            },
            ExtWidgetId::Calendar => match btn {
                0 => {
                    if self.cal_month <= 1 {
                        self.cal_month = 12;
                        self.cal_year -= 1;
                    } else {
                        self.cal_month -= 1;
                    }
                    true
                }
                1 => {
                    let now = chrono_local();
                    self.cal_year = now.year;
                    self.cal_month = month_name_to_num(now.month_name);
                    true
                }
                2 => {
                    if self.cal_month >= 12 {
                        self.cal_month = 1;
                        self.cal_year += 1;
                    } else {
                        self.cal_month += 1;
                    }
                    true
                }
                _ => false,
            },
            ExtWidgetId::Pomodoro => match btn {
                0 => {
                    if !self.pomo_running && self.pomo_rem_secs == 0 {
                        self.pomo_rem_secs = if self.pomo_break {
                            Self::pomo_break_secs(wcfg.mode_idx)
                        } else {
                            Self::pomo_work_secs(wcfg.mode_idx)
                        };
                    }
                    self.pomo_running = !self.pomo_running;
                    self.pomo_last_tick = Instant::now();
                    true
                }
                1 => {
                    self.pomo_running = false;
                    self.pomo_break = false;
                    self.pomo_rem_secs = Self::pomo_work_secs(wcfg.mode_idx);
                    self.pomo_last_tick = Instant::now();
                    true
                }
                2 => {
                    self.pomo_break = !self.pomo_break;
                    self.pomo_rem_secs = if self.pomo_break {
                        Self::pomo_break_secs(wcfg.mode_idx)
                    } else {
                        Self::pomo_work_secs(wcfg.mode_idx)
                    };
                    self.pomo_last_tick = Instant::now();
                    true
                }
                _ => false,
            },
            ExtWidgetId::WorldClock => match btn {
                0 => {
                    wcfg.feats[0] = !wcfg.feats[0];
                    true
                }
                1 => {
                    wcfg.mode_idx = (wcfg.mode_idx + 1) % 4;
                    let pack = world_clock_preset_cities(wcfg.mode_idx);
                    for i in 0..4 {
                        cfg.world_clock_cities[i] = pack[i].to_string();
                    }
                    true
                }
                10..=13 => {
                    let slot = (btn - 10) as usize;
                    let cur_name = cfg.world_clock_cities[slot].clone();
                    let cur_idx = WORLD_CLOCK_CITIES
                        .iter()
                        .position(|c| c.name.eq_ignore_ascii_case(cur_name.trim()))
                        .unwrap_or(0);
                    let len = WORLD_CLOCK_CITIES.len();
                    for step in 1..=len {
                        let cand = WORLD_CLOCK_CITIES[(cur_idx + step) % len].name;
                        let used_elsewhere = cfg
                            .world_clock_cities
                            .iter()
                            .enumerate()
                            .any(|(j, s)| j != slot && s.eq_ignore_ascii_case(cand));
                        if !used_elsewhere {
                            cfg.world_clock_cities[slot] = cand.to_string();
                            break;
                        }
                    }
                    true
                }
                _ => false,
            },
            ExtWidgetId::Clipboard => match btn {
                0..=3 => {
                    self.copy_to_clipboard(btn as usize);
                    true
                }
                4 => {
                    if wcfg.feats[5] && !self.clip_items.is_empty() {
                        self.clip_items.truncate(1);
                    } else {
                        self.clip_items.clear();
                    }
                    true
                }
                _ => false,
            },
            ExtWidgetId::Drives => match btn {
                0..=3 => {
                    if wcfg.feats[2] {
                        if let Some(d) = self.drives.get(btn as usize) {
                            let path: Vec<u16> = format!("{}:\\", d.letter)
                                .encode_utf16()
                                .chain(std::iter::once(0))
                                .collect();
                            unsafe {
                                let _ = ShellExecuteW(
                                    None,
                                    w!("open"),
                                    PCWSTR(path.as_ptr()),
                                    PCWSTR::null(),
                                    PCWSTR::null(),
                                    SW_SHOWNORMAL,
                                );
                            }
                        }
                    }
                    false
                }
                4 => {
                    wcfg.mode_idx = (wcfg.mode_idx + 1) % 4;
                    true
                }
                _ => false,
            },
            ExtWidgetId::Processes => match btn {
                0..=3 => {
                    if wcfg.feats[2] {
                        if let Some(p) = self.top_procs.get(btn as usize).cloned() {
                            let mut killed = false;
                            unsafe {
                                if let Ok(hp) = OpenProcess(PROCESS_TERMINATE, false, p.pid) {
                                    killed = TerminateProcess(hp, 1).is_ok();
                                    let _ = CloseHandle(hp);
                                }
                            }
                            if killed {
                                self.top_procs.retain(|item| item.pid != p.pid);
                            }
                            self.poll_slow_metrics_masked(false, true, false);
                            return true;
                        }
                    }
                    false
                }
                4 => {
                    unsafe {
                        let _ = ShellExecuteW(
                            None,
                            w!("open"),
                            w!("taskmgr.exe"),
                            PCWSTR::null(),
                            PCWSTR::null(),
                            SW_SHOWNORMAL,
                        );
                    }
                    false
                }
                _ => false,
            },
            ExtWidgetId::RecycleBin => match btn {
                0 => {
                    if wcfg.feats[3] {
                        unsafe {
                            let _ = ShellExecuteW(
                                None,
                                w!("open"),
                                w!("shell:RecycleBinFolder"),
                                PCWSTR::null(),
                                PCWSTR::null(),
                                SW_SHOWNORMAL,
                            );
                        }
                    }
                    false
                }
                1 => {
                    let flags = if wcfg.feats[5] {
                        SHERB_NOCONFIRMATION | SHERB_NOPROGRESSUI
                    } else {
                        0
                    };
                    unsafe {
                        let _ = SHEmptyRecycleBinW(None, PCWSTR::null(), flags);
                    }
                    self.poll_slow_metrics();
                    true
                }
                _ => false,
            },
        }
    }
}

pub(crate) fn month_name_to_num_pub(name: &str) -> u32 {
    month_name_to_num(name)
}

pub(crate) fn month_num_to_name_pub(m: u32) -> &'static str {
    month_num_to_name(m)
}

pub(crate) fn days_in_month_pub(year: i32, month: u32) -> u32 {
    days_in_month(year, month)
}

pub(crate) fn first_weekday_of_month_pub(year: i32, month: u32) -> u32 {
    first_weekday_of_month(year, month)
}

pub(crate) fn format_speed_pub(bps: f32, mode_idx: u8) -> String {
    format_speed(bps, mode_idx)
}

pub(crate) fn quote_for_category(mode_idx: u8, quote_idx: usize) -> (&'static str, &'static str) {
    let pool = match mode_idx {
        1 => &QUOTES_TECH,
        2 => &QUOTES_ZEN,
        3 => &QUOTES_LIT,
        _ => &QUOTES_STOIC,
    };
    pool[quote_idx % pool.len()]
}

fn month_name_to_num(name: &str) -> u32 {
    match name {
        "January" => 1,
        "February" => 2,
        "March" => 3,
        "April" => 4,
        "May" => 5,
        "June" => 6,
        "July" => 7,
        "August" => 8,
        "September" => 9,
        "October" => 10,
        "November" => 11,
        "December" => 12,
        _ => 11,
    }
}

fn month_num_to_name(m: u32) -> &'static str {
    match m {
        1 => "January",
        2 => "February",
        3 => "March",
        4 => "April",
        5 => "May",
        6 => "June",
        7 => "July",
        8 => "August",
        9 => "September",
        10 => "October",
        11 => "November",
        12 => "December",
        _ => "November",
    }
}

fn days_in_month(year: i32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0) {
                29
            } else {
                28
            }
        }
        _ => 30,
    }
}

/// Sakamoto's algorithm: returns 0 = Sunday .. 6 = Saturday for the 1st of `(year, month)`.
fn first_weekday_of_month(year: i32, month: u32) -> u32 {
    let t = [0, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4];
    let m = (month.clamp(1, 12) - 1) as usize;
    let y = if month < 3 { year - 1 } else { year };
    ((y + y / 4 - y / 100 + y / 400 + t[m] + 1).rem_euclid(7)) as u32
}

fn format_speed(bps: f32, mode_idx: u8) -> String {
    match mode_idx {
        1 => {
            let mbps = (bps * 8.0) / 1_000_000.0;
            if mbps >= 10.0 {
                format!("{:.0} Mbps", mbps)
            } else {
                format!("{:.1} Mbps", mbps)
            }
        }
        2 => format!("{:.0} KB/s", bps / 1024.0),
        _ => {
            if bps >= 1_048_576.0 {
                format!("{:.1} MB/s", bps / 1_048_576.0)
            } else {
                format!("{:.0} KB/s", (bps / 1024.0).max(1.0))
            }
        }
    }
}

#[inline]
fn rect_contains(r: (f32, f32, f32, f32), x: f32, y: f32) -> bool {
    x >= r.0 && x <= r.2 && y >= r.1 && y <= r.3
}

/// Single-source-of-truth logical geometry for the Audio Output & Mixer widget.
/// Shared by Direct2D rendering, hit-testing, and volume track dragging so buttons and
/// slider tracks align accurately at every padding, scale, compact mode, and feature toggle.
#[derive(Clone, Copy, Debug)]
pub struct AudioSwitcherLayout {
    pub x0: f32,
    pub x1: f32,
    pub hdr_rect: Option<(f32, f32, f32, f32)>,
    pub mixer_btn: Option<(f32, f32, f32, f32)>,
    pub dev_btn_0: Option<(f32, f32, f32, f32)>,
    pub dev_btn_1: Option<(f32, f32, f32, f32)>,
    pub dev_cycle_btn: Option<(f32, f32, f32, f32)>,
    pub divider_y: Option<f32>,
    pub vol_label_rect: Option<(f32, f32, f32, f32)>,
    pub vol_minus_btn: Option<(f32, f32, f32, f32)>,
    pub vol_mute_btn: Option<(f32, f32, f32, f32)>,
    pub vol_plus_btn: Option<(f32, f32, f32, f32)>,
    pub slider_track: Option<(f32, f32, f32, f32)>,
    pub slider_hit: Option<(f32, f32, f32, f32)>,
}

pub fn audio_switcher_layout(
    wcfg: &ExtWidgetConfig,
    audio_output_count: usize,
) -> AudioSwitcherLayout {
    let (lw, lh) = wcfg.logical_size(ExtWidgetId::AudioSwitcher);
    let pad = (wcfg.padding as f32).clamp(4.0, 28.0);
    let x0 = pad;
    let x1 = (lw - pad).max(x0 + 116.0);
    let mut y_cur = pad * 0.75;

    let mut hdr_rect = None;
    let mut mixer_btn = None;
    if wcfg.show_header {
        hdr_rect = Some((x0, y_cur, (x1 - 54.0).max(x0 + 40.0), y_cur + 16.0));
        if wcfg.feats[5] {
            mixer_btn = Some((x1 - 48.0, y_cur, x1, y_cur + 16.0));
        }
        y_cur += 20.0;
    }

    let mut dev_btn_0 = None;
    let mut dev_btn_1 = None;
    let mut dev_cycle_btn = None;
    if wcfg.feats[0] && wcfg.mode_idx != 3 && wcfg.show_footer {
        let has_more = audio_output_count > 2;
        let dev_x1 = if has_more { x1 - 26.0 } else { x1 };
        let mid = (x0 + dev_x1) * 0.5;
        dev_btn_0 = Some((x0, y_cur, mid - 4.0, y_cur + 24.0));
        dev_btn_1 = Some((mid + 4.0, y_cur, dev_x1, y_cur + 24.0));
        if has_more {
            dev_cycle_btn = Some((x1 - 22.0, y_cur, x1, y_cur + 24.0));
        }
        y_cur += 28.0;
    }

    let mut divider_y = None;
    if wcfg.show_dividers {
        divider_y = Some(y_cur);
        y_cur += 5.0;
    }

    let mut vol_label_rect = None;
    let mut vol_minus_btn = None;
    let mut vol_mute_btn = None;
    let mut vol_plus_btn = None;
    let mut vol_btn_bottom = y_cur;
    if wcfg.feats[1] && (wcfg.show_primary || wcfg.show_icon) {
        let lbl_right = if wcfg.feats[2] {
            (x1 - 112.0).max(x0 + 36.0)
        } else {
            x1
        };
        vol_label_rect = Some((x0, y_cur, lbl_right, y_cur + 24.0));
        if wcfg.feats[2] {
            vol_minus_btn = Some((x1 - 106.0, y_cur + 1.0, x1 - 76.0, y_cur + 23.0));
            vol_mute_btn = Some((x1 - 72.0, y_cur + 1.0, x1 - 34.0, y_cur + 23.0));
            vol_plus_btn = Some((x1 - 30.0, y_cur + 1.0, x1, y_cur + 23.0));
            vol_btn_bottom = y_cur + 23.0;
        }
        y_cur += 25.0;
    }

    let mut slider_track = None;
    let mut slider_hit = None;
    if (wcfg.feats[3] || wcfg.feats[1]) && y_cur + 4.5 <= lh - 1.5 {
        let track_top = y_cur;
        let track_bot = (y_cur + 5.0).min(lh - 1.5);
        slider_track = Some((x0, track_top, x1, track_bot));
        let hit_top = (track_top - 3.0).max(vol_btn_bottom + 0.5);
        let hit_bot = (track_bot + 6.0).min(lh);
        slider_hit = Some(((x0 - 4.0).max(0.0), hit_top, (x1 + 4.0).min(lw), hit_bot));
    }

    AudioSwitcherLayout {
        x0,
        x1,
        hdr_rect,
        mixer_btn,
        dev_btn_0,
        dev_btn_1,
        dev_cycle_btn,
        divider_y,
        vol_label_rect,
        vol_minus_btn,
        vol_mute_btn,
        vol_plus_btn,
        slider_track,
        slider_hit,
    }
}

/// Single-source-of-truth logical button geometry for the Pomodoro Focus Timer widget.
/// Shared by Direct2D rendering and hit-testing so Start/Pause, Reset, and Skip align
/// identically across ring visibility, padding, compact mode, and DPI scaling.
#[derive(Clone, Copy, Debug)]
pub struct PomodoroButtonLayout {
    pub start_btn: Option<(f32, f32, f32, f32)>,
    pub reset_btn: Option<(f32, f32, f32, f32)>,
    pub skip_btn: Option<(f32, f32, f32, f32)>,
}

pub fn pomodoro_button_layout(wcfg: &ExtWidgetConfig) -> PomodoroButtonLayout {
    let (lw, lh) = wcfg.logical_size(ExtWidgetId::Pomodoro);
    let pad = (wcfg.padding as f32).clamp(4.0, 28.0);
    let x0 = pad;
    let x1 = (lw - pad).max(x0 + 80.0);
    let mut text_x = x0;
    if wcfg.feats[0] && wcfg.show_icon {
        let r_rad = 26.0;
        text_x += r_rad * 2.0 + 16.0;
    }
    if !wcfg.feats[2] {
        return PomodoroButtonLayout {
            start_btn: None,
            reset_btn: None,
            skip_btn: None,
        };
    }
    let by0 = (lh - 28.0).max(0.0);
    let by1 = (lh - 8.0).max(by0 + 14.0);
    let gap = 4.0;
    let avail = (x1 - text_x).max(60.0);
    if wcfg.feats[3] {
        let reset_w = (avail * 0.22).clamp(24.0, 36.0);
        let rem = (avail - reset_w - gap * 2.0).max(40.0);
        let start_w = (rem * 0.54).max(30.0);
        let r0_x0 = text_x;
        let r0_x1 = r0_x0 + start_w;
        let r1_x0 = r0_x1 + gap;
        let r1_x1 = r1_x0 + reset_w;
        let r2_x0 = r1_x1 + gap;
        let r2_x1 = x1.max(r2_x0 + 24.0);
        PomodoroButtonLayout {
            start_btn: Some((r0_x0, by0, r0_x1, by1)),
            reset_btn: Some((r1_x0, by0, r1_x1, by1)),
            skip_btn: Some((r2_x0, by0, r2_x1, by1)),
        }
    } else {
        let reset_w = (avail * 0.28).clamp(26.0, 42.0);
        let r0_x0 = text_x;
        let r0_x1 = (x1 - reset_w - gap).max(r0_x0 + 36.0);
        let r1_x0 = r0_x1 + gap;
        let r1_x1 = x1.max(r1_x0 + 24.0);
        PomodoroButtonLayout {
            start_btn: Some((r0_x0, by0, r0_x1, by1)),
            reset_btn: Some((r1_x0, by0, r1_x1, by1)),
            skip_btn: None,
        }
    }
}

#[cfg(test)]
pub fn hit_test_ext_widget(
    id: ExtWidgetId,
    wcfg: &ExtWidgetConfig,
    lx: f32,
    ly: f32,
) -> WidgetHitZone {
    hit_test_ext_widget_with_audio(id, wcfg, 0, lx, ly)
}

pub fn hit_test_ext_widget_with_audio(
    id: ExtWidgetId,
    wcfg: &ExtWidgetConfig,
    audio_output_count: usize,
    lx: f32,
    ly: f32,
) -> WidgetHitZone {
    let (lw, lh) = wcfg.logical_size(id);
    if lx < 0.0 || lx > lw || ly < 0.0 || ly > lh {
        return WidgetHitZone::None;
    }

    match id {
        ExtWidgetId::Weather => {
            if ly <= 34.0 {
                if lx >= lw - 58.0 {
                    return WidgetHitZone::ExtBtn(id, 0); // °C / °F
                }
                if lx >= lw - 138.0 {
                    return WidgetHitZone::ExtBtn(id, 1); // Cycle City
                }
            }
        }
        ExtWidgetId::SearchBar => {
            if lx >= lw - 42.0 && wcfg.feats[3] {
                return WidgetHitZone::ExtBtn(id, 2); // Go ->
            }
            if lx <= 112.0 && wcfg.feats[0] {
                return WidgetHitZone::ExtBtn(id, 1); // Cycle Engine
            }
            return WidgetHitZone::ExtBtn(id, 0); // Focus input
        }
        ExtWidgetId::Greeting => {
            if wcfg.feats[4] {
                let pad = (wcfg.padding as f32).clamp(4.0, 28.0);
                let x1 = (lw - pad).max(pad + 120.0);
                let y0 = pad * 0.75;
                if ly >= (y0 - 2.0) && ly <= (y0 + 20.0) {
                    if lx >= x1 - 28.0 && lx <= x1 + 2.0 {
                        return WidgetHitZone::ExtBtn(id, 0); // Next quote
                    }
                    if lx >= x1 - 110.0 && lx <= x1 - 32.0 {
                        return WidgetHitZone::ExtBtn(id, 1); // Cycle topic
                    }
                }
            }
        }
        ExtWidgetId::Network => {
            if ly <= 30.0 && lx >= lw - 86.0 {
                return WidgetHitZone::ExtBtn(id, 0); // Toggle unit
            }
            if ly > lh - 26.0 && lx >= lw - 96.0 {
                return WidgetHitZone::ExtBtn(id, 1); // Open ncpa.cpl
            }
        }
        ExtWidgetId::Gpu => {
            if ly <= 30.0 && lx >= lw - 96.0 {
                return WidgetHitZone::ExtBtn(id, 0); // Cycle primary focus
            }
            if ly > lh - 26.0 && lx >= lw - 90.0 {
                return WidgetHitZone::ExtBtn(id, 1); // TaskMgr
            }
        }
        ExtWidgetId::AudioSwitcher => {
            let lay = audio_switcher_layout(wcfg, audio_output_count);
            if let Some(r) = lay.mixer_btn {
                if rect_contains(r, lx, ly) {
                    return WidgetHitZone::ExtBtn(id, 5); // Sound settings
                }
            }
            if let Some(r) = lay.dev_btn_0 {
                if rect_contains(r, lx, ly) {
                    return WidgetHitZone::ExtBtn(id, 3); // Output 0
                }
            }
            if let Some(r) = lay.dev_btn_1 {
                if rect_contains(r, lx, ly) {
                    return WidgetHitZone::ExtBtn(id, 4); // Output 1
                }
            }
            if let Some(r) = lay.dev_cycle_btn {
                if rect_contains(r, lx, ly) {
                    return WidgetHitZone::ExtBtn(id, 6); // Cycle device page
                }
            }
            if let Some(r) = lay.vol_minus_btn {
                if rect_contains(r, lx, ly) {
                    return WidgetHitZone::ExtBtn(id, 0); // Vol -
                }
            }
            if let Some(r) = lay.vol_mute_btn {
                if rect_contains(r, lx, ly) {
                    return WidgetHitZone::ExtBtn(id, 1); // Mute / Unmute
                }
            }
            if let Some(r) = lay.vol_plus_btn {
                if rect_contains(r, lx, ly) {
                    return WidgetHitZone::ExtBtn(id, 2); // Vol +
                }
            }
            if let Some(r) = lay.slider_hit {
                if rect_contains(r, lx, ly) {
                    return WidgetHitZone::ExtBtn(id, 7); // Volume slider track
                }
            }
        }
        ExtWidgetId::Notes => {
            if ly <= 32.0 && wcfg.feats[3] {
                if lx >= lw - 54.0 {
                    return WidgetHitZone::ExtBtn(id, 5); // Clear done
                }
                if lx >= lw - 108.0 {
                    return WidgetHitZone::ExtBtn(id, 4); // + Add / focus typing
                }
            }
            if ly > 34.0 {
                let row = ((ly - 38.0) / 30.0).floor() as i32;
                if (0..=3).contains(&row) {
                    if lx >= lw - 32.0 || !wcfg.feats[0] {
                        return WidgetHitZone::ExtBtn(id, 10 + row as u8);
                    }
                    return WidgetHitZone::ExtBtn(id, row as u8);
                }
            }
        }
        ExtWidgetId::Calendar => {
            if ly <= 32.0 && wcfg.feats[2] {
                if (lw - 92.0..lw - 64.0).contains(&lx) {
                    return WidgetHitZone::ExtBtn(id, 0); // Prev month
                }
                if (lw - 63.0..lw - 33.0).contains(&lx) {
                    return WidgetHitZone::ExtBtn(id, 1); // Today
                }
                if lx >= lw - 32.0 {
                    return WidgetHitZone::ExtBtn(id, 2); // Next month
                }
            }
        }
        ExtWidgetId::Pomodoro => {
            let play = pomodoro_button_layout(wcfg);
            // Slightly pad vertical grab area (+2px) so clicks near pill rims always register
            let hit_pad = |r: (f32, f32, f32, f32)| -> bool {
                lx >= r.0 && lx <= r.2 && ly >= (r.1 - 2.0) && ly <= (r.3 + 2.0)
            };
            if let Some(r) = play.start_btn {
                if hit_pad(r) {
                    return WidgetHitZone::ExtBtn(id, 0); // Start / Pause
                }
            }
            if let Some(r) = play.reset_btn {
                if hit_pad(r) {
                    return WidgetHitZone::ExtBtn(id, 1); // Reset
                }
            }
            if let Some(r) = play.skip_btn {
                if hit_pad(r) {
                    return WidgetHitZone::ExtBtn(id, 2); // Skip
                }
            }
        }
        ExtWidgetId::WorldClock => {
            let pad = (wcfg.padding as f32).clamp(4.0, 28.0);
            let x1 = (lw - pad).max(pad + 96.0);
            let mut y_cur = pad * 0.75;
            if wcfg.show_header {
                if ly >= (y_cur - 2.0) && ly <= (y_cur + 18.0) {
                    if lx >= x1 - 38.0 && lx <= x1 + 2.0 {
                        return WidgetHitZone::ExtBtn(id, 0); // 12h/24h
                    }
                    if lx >= x1 - 88.0 && lx <= x1 - 42.0 {
                        return WidgetHitZone::ExtBtn(id, 1); // Cycle City pack
                    }
                }
                y_cur += 19.0;
                if wcfg.show_dividers {
                    y_cur += 4.0;
                }
            }
            if ly >= y_cur {
                let max_c = if wcfg.feats[5] { 4 } else { 3 };
                let spacing_mul = (0.78 + (wcfg.param1_pct as f32 / 100.0) * 0.42).clamp(0.75, 1.25);
                let row_h = (((lh - 6.0 - y_cur) / max_c as f32) * spacing_mul).clamp(16.0, 28.0);
                let row = ((ly - y_cur) / row_h).floor() as i32;
                if row >= 0 && (row as usize) < max_c && y_cur + (row as f32 + 1.0) * row_h <= lh - 2.0
                {
                    return WidgetHitZone::ExtBtn(id, 10 + row as u8); // Cycle slot city
                }
            }
        }
        ExtWidgetId::Clipboard => {
            if ly <= 30.0 && lx >= lw - 56.0 && wcfg.feats[3] {
                return WidgetHitZone::ExtBtn(id, 4); // Clear
            }
            if ly > 30.0 && wcfg.feats[1] {
                let row = ((ly - 32.0) / 24.0).floor() as i32;
                if (0..=2).contains(&row) {
                    return WidgetHitZone::ExtBtn(id, row as u8);
                }
            }
        }
        ExtWidgetId::Drives => {
            let pad = (wcfg.padding as f32).clamp(4.0, 28.0);
            let x0 = pad;
            let x1 = (lw - pad).max(x0 + 64.0);
            let mut y_cur = pad * 0.75;
            if wcfg.show_header {
                if ly >= (y_cur - 2.0) && ly <= (y_cur + 18.0) && lx >= x1 - 58.0 && lx <= x1 + 2.0 {
                    return WidgetHitZone::ExtBtn(id, 4); // Toggle style
                }
                y_cur += 19.0;
                if wcfg.show_dividers {
                    y_cur += 4.0;
                }
            }
            if ly >= y_cur && lx >= x0 && lx <= x1 {
                if wcfg.mode_idx == 1 || wcfg.mode_idx == 2 {
                    let thick = (3.0 + (wcfg.param1_pct as f32 / 100.0) * 5.0).clamp(5.0, 11.0);
                    let row_step = thick + 26.0;
                    let row = ((ly - y_cur) / row_step).floor() as i32;
                    if (0..=1).contains(&row) {
                        return WidgetHitZone::ExtBtn(id, row as u8);
                    }
                } else if wcfg.mode_idx == 3 {
                    let row = ((ly - y_cur) / 28.0).floor() as i32;
                    if (0..=1).contains(&row) {
                        return WidgetHitZone::ExtBtn(id, row as u8);
                    }
                } else {
                    let col = if lx < (x0 + x1) * 0.5 { 0 } else { 1 };
                    return WidgetHitZone::ExtBtn(id, col);
                }
            }
        }
        ExtWidgetId::Processes => {
            let pad = (wcfg.padding as f32).clamp(4.0, 28.0);
            let x1 = (lw - pad).max(pad + 64.0);
            let mut y_cur = pad * 0.75;
            if wcfg.show_header {
                if ly >= (y_cur - 2.0) && ly <= (y_cur + 18.0) && lx >= x1 - 50.0 && lx <= x1 + 2.0 {
                    return WidgetHitZone::ExtBtn(id, 4); // TaskMgr pill only (header title drags card)
                }
                y_cur += 19.0;
                if wcfg.show_dividers {
                    y_cur += 4.0;
                }
            }
            if ly >= y_cur && lx >= x1 - 24.0 && lx <= x1 + 2.0 && wcfg.feats[2] {
                let max_p = if wcfg.mode_idx == 3 { 4 } else { 3 };
                let row_h = ((lh - 6.0 - y_cur) / max_p as f32).clamp(20.0, 28.0);
                let row = ((ly - y_cur) / row_h).floor() as i32;
                if row >= 0 && (row as usize) < max_p && y_cur + (row as f32 + 1.0) * row_h <= lh - 4.0
                {
                    return WidgetHitZone::ExtBtn(id, row as u8);
                }
            }
        }
        ExtWidgetId::RecycleBin => {
            if lx >= lw - 96.0 && (24.0..=68.0).contains(&ly) && wcfg.feats[2] {
                return WidgetHitZone::ExtBtn(id, 1); // Empty Bin
            }
            if lx < lw - 96.0 && wcfg.feats[3] {
                return WidgetHitZone::ExtBtn(id, 0); // Open Bin
            }
        }
    }

    WidgetHitZone::ExtCard(id)
}

unsafe fn draw_ring_arc(
    rt: &ID2D1RenderTarget,
    d2d: Option<&ID2D1Factory>,
    cx: f32,
    cy: f32,
    rad: f32,
    thickness: f32,
    frac: f32,
    track_br: &windows::Win32::Graphics::Direct2D::ID2D1SolidColorBrush,
    fill_br: &windows::Win32::Graphics::Direct2D::ID2D1SolidColorBrush,
) {
    let ell = D2D1_ELLIPSE {
        point: Vector2 { X: cx, Y: cy },
        radiusX: rad,
        radiusY: rad,
    };
    rt.DrawEllipse(&ell, track_br, thickness, None);
    let f = frac.clamp(0.0, 0.999);
    if f <= 0.01 {
        return;
    }
    let Some(fac) = d2d else {
        return;
    };
    let Ok(path): Result<ID2D1PathGeometry, _> = fac.CreatePathGeometry() else {
        return;
    };
    let Ok(sink) = path.Open() else {
        return;
    };
    let start_ang = -std::f32::consts::FRAC_PI_2;
    let end_ang = start_ang + f * std::f32::consts::TAU;
    let sx = cx + rad * start_ang.cos();
    let sy = cy + rad * start_ang.sin();
    let ex = cx + rad * end_ang.cos();
    let ey = cy + rad * end_ang.sin();
    sink.BeginFigure(Vector2 { X: sx, Y: sy }, D2D1_FIGURE_BEGIN_HOLLOW);
    sink.AddArc(&D2D1_ARC_SEGMENT {
        point: Vector2 { X: ex, Y: ey },
        size: D2D_SIZE_F {
            width: rad,
            height: rad,
        },
        rotationAngle: 0.0,
        sweepDirection: D2D1_SWEEP_DIRECTION_CLOCKWISE,
        arcSize: if f > 0.5 {
            D2D1_ARC_SIZE_LARGE
        } else {
            D2D1_ARC_SIZE_SMALL
        },
    });
    sink.EndFigure(D2D1_FIGURE_END_OPEN);
    let _ = sink.Close();
    rt.DrawGeometry(&path, fill_br, thickness, None);
}

#[allow(clippy::too_many_arguments)]
unsafe fn ext_widget_fg_key(
    id: ExtWidgetId,
    s: f32,
    w: f32,
    h_card: f32,
    wcfg: &ExtWidgetConfig,
    cfg: &AppConfig,
    state: &ExtRuntimeState,
    cc_snap: &CcSnapshot,
    pal: &ThemePalette,
    hover_zone: WidgetHitZone,
    has_custom_fonts: bool,
) -> u64 {
    let mut h = FgKeyHasher::new(0xE17F_0001u64 ^ (id.idx() as u64));
    h.mix_u8(id.idx() as u8);
    h.mix_f32(s);
    h.mix_f32(w);
    h.mix_f32(h_card);
    h.mix_bool(has_custom_fonts);
    h.mix_pal(pal);
    let hov_code: u32 = match hover_zone {
        WidgetHitZone::ExtCard(hid) if hid == id => 1,
        WidgetHitZone::ExtBtn(hid, bi) if hid == id => 100 + bi as u32,
        _ => 0,
    };
    h.mix_u64(hov_code as u64);

    // WCfg foreground fields
    h.mix_u8(wcfg.theme as u8);
    h.mix_u8(wcfg.scale_pct);
    h.mix_u8(wcfg.width_pct);
    h.mix_u8(wcfg.corner_radius);
    h.mix_u8(wcfg.padding);
    h.mix_u8(wcfg.align as u8);
    h.mix_bool(wcfg.show_header);
    h.mix_bool(wcfg.show_primary);
    h.mix_bool(wcfg.show_icon);
    h.mix_bool(wcfg.show_dividers);
    h.mix_bool(wcfg.show_footer);
    h.mix_bool(wcfg.compact_mode);
    h.mix_u8(wcfg.mode_idx);
    h.mix_u8(wcfg.param1_pct);
    h.mix_u8(wcfg.param2_pct);
    for &b in &wcfg.feats {
        h.mix_bool(b);
    }
    h.mix_bool(wcfg.use_custom_color);
    h.mix_u64(wcfg.accent_rgb as u64);
    h.mix_u64(wcfg.text_rgb as u64);
    h.mix_u64(wcfg.bg_rgb as u64);
    h.mix_u8(wcfg.font_style as u8);
    h.mix_str(&wcfg.custom_font);
    h.mix_u8(wcfg.font_scale_pct);
    for tr in &wcfg.elem_transforms {
        h.mix_tr(tr);
    }
    for f in &wcfg.elem_fonts {
        h.mix_str(f);
    }
    h.mix_horology(&wcfg.horology);

    // Per-widget runtime state
    match id {
        ExtWidgetId::Weather => {
            let snap = &state.weather[(wcfg.mode_idx as usize).min(3)];
            h.mix_str(&snap.city);
            h.mix_f32(snap.temp_c);
            h.mix_f32(snap.high_c);
            h.mix_f32(snap.low_c);
            h.mix_u8(snap.humidity);
            h.mix_f32(snap.wind_kmh);
            h.mix_str(snap.condition);
            for (d_name, hi, lo, icon) in &snap.forecast_days {
                h.mix_str(d_name);
                h.mix_f32(*hi);
                h.mix_f32(*lo);
                h.mix_u8(*icon);
            }
        }
        ExtWidgetId::SearchBar => {
            h.mix_str(&state.search_query);
            h.mix_bool(state.search_focused);
            if state.search_focused && wcfg.feats[5] {
                h.mix_u64((state.tick_counter % 2) as u64);
            }
        }
        ExtWidgetId::Greeting => {
            let now = chrono_local();
            h.mix_i32(now.year);
            h.mix_str(now.month_name);
            h.mix_u64(now.day as u64);
            h.mix_u64(now.hour as u64);
            h.mix_str(&state.user_name);
            h.mix_usize(state.quote_idx);
        }
        ExtWidgetId::Network => {
            h.mix_f32(state.down_bps);
            h.mix_f32(state.up_bps);
            h.mix_u64(state.ping_ms as u64);
            h.mix_str(&state.local_ip);
            for &v in &state.net_history {
                h.mix_f32(v);
            }
        }
        ExtWidgetId::Gpu => {
            h.mix_str(&state.gpu_name);
            h.mix_f32(state.gpu_load_pct);
            h.mix_f32(state.vram_used_mb);
            h.mix_f32(state.vram_total_mb);
            h.mix_f32(state.gpu_temp_c);
            h.mix_u64(state.gpu_clock_mhz as u64);
            for &v in &state.gpu_history {
                h.mix_f32(v);
            }
        }
        ExtWidgetId::AudioSwitcher => {
            h.mix_usize(state.active_audio_out);
            h.mix_usize(state.audio_output_page);
            h.mix_u8(cc_snap.vol_pct);
            h.mix_bool(cc_snap.vol_muted);
            h.mix_str(&cc_snap.bt_sub);
            h.mix_usize(cc_snap.audio_outputs.len());
            for dev in &cc_snap.audio_outputs {
                h.mix_str(&dev.id);
                h.mix_str(&dev.name);
                h.mix_bool(dev.is_default);
            }
        }
        ExtWidgetId::Notes => {
            h.mix_str(&cfg.notes_title);
            h.mix_usize(cfg.notes_items.len());
            for (done, txt) in &cfg.notes_items {
                h.mix_bool(*done);
                h.mix_str(txt);
            }
            h.mix_bool(state.note_focused);
            h.mix_str(&state.note_draft);
            h.mix_u64(state.editing_note_row.map(|r| r as u64 + 1).unwrap_or(0));
        }
        ExtWidgetId::Calendar => {
            let now = chrono_local();
            h.mix_i32(now.year);
            h.mix_str(now.month_name);
            h.mix_u64(now.day as u64);
            h.mix_i32(state.cal_year);
            h.mix_u64(state.cal_month as u64);
        }
        ExtWidgetId::Pomodoro => {
            h.mix_bool(state.pomo_running);
            h.mix_bool(state.pomo_break);
            h.mix_u64(state.pomo_rem_secs as u64);
            h.mix_u64(state.pomo_sessions as u64);
            h.mix_u8(state.pomo_last_preset);
        }
        ExtWidgetId::WorldClock => {
            let utc = unsafe { GetSystemTime() };
            h.mix_u64(utc.wYear as u64);
            h.mix_u64(utc.wMonth as u64);
            h.mix_u64(utc.wDay as u64);
            h.mix_u64(utc.wHour as u64);
            h.mix_u64(utc.wMinute as u64);
            for c in &cfg.world_clock_cities {
                h.mix_str(c);
            }
        }
        ExtWidgetId::Clipboard => {
            h.mix_usize(state.clip_items.len());
            for (txt, len) in &state.clip_items {
                h.mix_str(txt);
                h.mix_usize(*len);
            }
            h.mix_u64(state.clip_copied_flash.map(|(i, _)| i as u64 + 1).unwrap_or(0));
        }
        ExtWidgetId::Drives => {
            h.mix_usize(state.drives.len());
            for d in &state.drives {
                h.mix_u64(d.letter as u64);
                h.mix_f32(d.used_gb);
                h.mix_f32(d.total_gb);
                h.mix_f32(d.pct);
            }
            h.mix_u64((state.tick_counter % 2) as u64);
        }
        ExtWidgetId::Processes => {
            h.mix_usize(state.top_procs.len());
            for p in &state.top_procs {
                h.mix_u64(p.pid as u64);
                h.mix_str(&p.name);
                h.mix_f32(p.mem_mb);
            }
        }
        ExtWidgetId::RecycleBin => {
            h.mix_u64(state.bin_items);
            h.mix_u64(state.bin_bytes);
        }
    }
    h.finish()
}

#[allow(clippy::too_many_arguments)]
pub unsafe fn draw_ext_widget_with_viz(
    rt: &ID2D1RenderTarget,
    dw: &IDWriteFactory,
    d2d: Option<&ID2D1Factory>,
    custom_fonts: Option<&IDWriteFontCollection>,
    x: f32,
    y: f32,
    s: f32,
    id: ExtWidgetId,
    cfg: &AppConfig,
    state: &ExtRuntimeState,
    cc_snap: &CcSnapshot,
    hover_zone: WidgetHitZone,
    fx_viz: Option<&VizSnapshot>,
) {
    draw_ext_widget_with_boxes_inner(
        rt, dw, d2d, custom_fonts, x, y, s, id, cfg, state, cc_snap, hover_zone, fx_viz, None, None,
    );
}

#[allow(clippy::too_many_arguments)]
pub unsafe fn draw_ext_widget_with_viz_cached(
    rt: &ID2D1RenderTarget,
    dw: &IDWriteFactory,
    d2d: Option<&ID2D1Factory>,
    custom_fonts: Option<&IDWriteFontCollection>,
    x: f32,
    y: f32,
    s: f32,
    id: ExtWidgetId,
    cfg: &AppConfig,
    state: &ExtRuntimeState,
    cc_snap: &CcSnapshot,
    hover_zone: WidgetHitZone,
    fx_viz: Option<&VizSnapshot>,
    fg_cache: &mut WidgetFgCacheSlot,
    cache_gen: u64,
) {
    draw_ext_widget_with_boxes_inner(
        rt,
        dw,
        d2d,
        custom_fonts,
        x,
        y,
        s,
        id,
        cfg,
        state,
        cc_snap,
        hover_zone,
        fx_viz,
        None,
        Some((fg_cache, cache_gen)),
    );
}

#[allow(clippy::too_many_arguments)]
pub unsafe fn draw_ext_widget_with_boxes(
    rt: &ID2D1RenderTarget,
    dw: &IDWriteFactory,
    d2d: Option<&ID2D1Factory>,
    custom_fonts: Option<&IDWriteFontCollection>,
    x: f32,
    y: f32,
    s: f32,
    id: ExtWidgetId,
    cfg: &AppConfig,
    state: &ExtRuntimeState,
    cc_snap: &CcSnapshot,
    hover_zone: WidgetHitZone,
    fx_viz: Option<&VizSnapshot>,
    out_boxes: Option<&mut [ClockElemBox; 6]>,
) {
    draw_ext_widget_with_boxes_inner(
        rt, dw, d2d, custom_fonts, x, y, s, id, cfg, state, cc_snap, hover_zone, fx_viz, out_boxes, None,
    );
}

#[allow(clippy::too_many_arguments)]
unsafe fn draw_ext_widget_with_boxes_inner(
    rt: &ID2D1RenderTarget,
    dw: &IDWriteFactory,
    d2d: Option<&ID2D1Factory>,
    custom_fonts: Option<&IDWriteFontCollection>,
    x: f32,
    y: f32,
    s: f32,
    id: ExtWidgetId,
    cfg: &AppConfig,
    state: &ExtRuntimeState,
    cc_snap: &CcSnapshot,
    hover_zone: WidgetHitZone,
    fx_viz: Option<&VizSnapshot>,
    mut out_boxes: Option<&mut [ClockElemBox; 6]>,
    fg_cache: Option<(&mut WidgetFgCacheSlot, u64)>,
) {
    if let Some(boxes) = out_boxes.as_deref_mut() {
        *boxes = [ClockElemBox::default(); 6];
    }
    let wcfg = &cfg.ext_widgets[id.idx()];
    if wcfg.is_all_elements_off() {
        return;
    }
    if let Some(ht) = wcfg.theme.horology_theme() {
        let (lw, lh) = wcfg.logical_size(id);
        let w = lw * s;
        let h = lh * s;
        let anim_active = (cfg.widgets_live_material && ht.has_idle_motion())
            || fx_viz
                .map(|v| {
                    v.hover_mix > 0.004
                        || v.hover_active
                        || v.ripples.iter().any(|r| r.active)
                })
                .unwrap_or(false);
        if out_boxes.is_none() && !anim_active {
            if let Some((slot, cache_gen)) = fg_cache {
                let pal = resolve_palette_for(cfg, wcfg.match_wallpaper);
                let key = ext_widget_fg_key(
                    id,
                    s,
                    w,
                    h,
                    wcfg,
                    cfg,
                    state,
                    cc_snap,
                    &pal,
                    hover_zone,
                    custom_fonts.is_some(),
                );
                slot.draw_cached(rt, cache_gen, key, x, y, w, h, 14.0 * s, |fg_rt| {
                    crate::horology_widgets::draw_ext_horology_widget(
                        fg_rt,
                        dw,
                        d2d,
                        custom_fonts,
                        x,
                        y,
                        s,
                        id,
                        ht,
                        cfg,
                        state,
                        cc_snap,
                        hover_zone,
                        fx_viz,
                        None,
                    );
                });
                return;
            }
        }
        crate::horology_widgets::draw_ext_horology_widget(
            rt,
            dw,
            d2d,
            custom_fonts,
            x,
            y,
            s,
            id,
            ht,
            cfg,
            state,
            cc_snap,
            hover_zone,
            fx_viz,
            out_boxes,
        );
        return;
    }
    let (lw, lh) = wcfg.logical_size(id);
    let w = lw * s;
    let h = lh * s;
    let pad = (wcfg.padding as f32).clamp(4.0, 28.0) * s;
    let rad = (wcfg.corner_radius as f32).clamp(0.0, 36.0) * s;
    let tr_card = wcfg.elem_transforms[5];

    let pal = resolve_palette_for(cfg, wcfg.match_wallpaper);
    let (ar, ag, ab) = if wcfg.use_custom_color {
        u32_to_rgb(wcfg.accent_rgb)
    } else {
        pal.accent
    };
    let (tr, tg, tb) = if wcfg.use_custom_color {
        u32_to_rgb(wcfg.text_rgb)
    } else {
        pal.text_primary
    };
    let (br_r, br_g, br_b) = if let Some(card_ov) = tr_card.color_rgb {
        u32_to_rgb(card_ov)
    } else if wcfg.use_custom_color && !wcfg.match_wallpaper {
        u32_to_rgb(wcfg.bg_rgb)
    } else {
        pal.glass_top
    };
    let (br_bot_r, br_bot_g, br_bot_b) = if let Some(c2) = tr_card.color2_rgb {
        u32_to_rgb(c2)
    } else if tr_card.color_rgb.is_some() || (wcfg.use_custom_color && !wcfg.match_wallpaper) {
        if (br_r as u16 + br_g as u16 + br_b as u16) > 540 {
            (br_r.saturating_sub(18), br_g.saturating_sub(18), br_b.saturating_sub(20))
        } else {
            (br_r / 2, br_g / 2, br_b / 2)
        }
    } else {
        pal.glass_bot
    };

    let card_hov = matches!(hover_zone, WidgetHitZone::ExtCard(hid) if hid == id)
        || matches!(hover_zone, WidgetHitZone::ExtBtn(hid, _) if hid == id);

    let card_rect = D2D_RECT_F {
        left: x,
        top: y,
        right: x + w,
        bottom: y + h,
    };
    let card_rr = D2D1_ROUNDED_RECT {
        rect: card_rect,
        radiusX: rad,
        radiusY: rad,
    };

    let (rr_r, rr_g, rr_b) = tr_card.border_rgb.map(u32_to_rgb).unwrap_or((ar, ag, ab));
    // 1. Optional Soft Elevation Shadow & Plate Backdrop (Card slot 5)
    let mut no_ext_boxes: Option<&mut [ClockElemBox]> = None;
    let g_card = begin_elem_xf_slot(rt, 5, tr_card, (x, y, x + w, y + h), s, &mut no_ext_boxes);
    if wcfg.show_plate && tr_card.visible {
        if let Some(pbr_mat) = wcfg.theme.pbr_material() {
            if wcfg.show_shadow {
                crate::realistic_pbr::draw_pbr_drop_shadows(
                    rt,
                    card_rect,
                    rad,
                    s,
                    pbr_mat.is_light_surface(),
                );
            }
            let inter = crate::realistic_pbr::pbr_input_from_viz(fx_viz, 0.0, card_hov);
            let tint_ov = if tr_card.color_rgb.is_some() || tr_card.color2_rgb.is_some() {
                Some((
                    tr_card.color_rgb.unwrap_or(wcfg.bg_rgb),
                    tr_card.color2_rgb,
                ))
            } else if wcfg.match_wallpaper {
                Some((
                    crate::theme::rgb_to_u32(pal.glass_top),
                    Some(crate::theme::rgb_to_u32(pal.glass_bot)),
                ))
            } else if wcfg.theme == ExtWidgetTheme::SwissChronograph {
                Some((0x121E36, Some(0x091020)))
            } else {
                None
            };
            let op = (wcfg.glass_alpha as f32 / 225.0).clamp(0.45, 0.99);
            crate::realistic_pbr::draw_pbr_surface_clipped(
                rt,
                d2d,
                pbr_mat,
                card_rect,
                rad,
                s,
                op,
                tint_ov,
                None,
                &inter,
            );
            // Extra ruby signature dot on SwissChronograph
            if wcfg.theme == ExtWidgetTheme::SwissChronograph {
                if let Ok(rub) = rt.CreateSolidColorBrush(&rgba_u8(216, 28, 72, 0.94), None) {
                    rt.FillEllipse(
                        &D2D1_ELLIPSE {
                            point: Vector2 { X: x + w - 12.0 * s, Y: y + 12.0 * s },
                            radiusX: 2.5 * s,
                            radiusY: 2.5 * s,
                        },
                        &rub,
                    );
                }
            }
            let ra = (wcfg.rim_alpha as f32 / 255.0).clamp(0.0, 1.0);
            crate::realistic_pbr::draw_pbr_bevel_rim(
                rt,
                card_rect,
                rad,
                s,
                pbr_mat,
                tr_card.border_rgb,
                ra,
                inter.hover_mix,
                None,
            );
        } else {
        if wcfg.show_shadow {
            let sh_rr = D2D1_ROUNDED_RECT {
                rect: D2D_RECT_F {
                    left: x + 1.5 * s,
                    top: y + 3.0 * s,
                    right: x + w + 1.5 * s,
                    bottom: y + h + 3.5 * s,
                },
                radiusX: rad + 1.0 * s,
                radiusY: rad + 1.0 * s,
            };
            if let Ok(sh_br) = rt.CreateSolidColorBrush(&rgba_u8(0, 0, 0, 0.34), None) {
                rt.FillRoundedRectangle(&sh_rr, &sh_br);
            }
        }

        let ga_base = (wcfg.glass_alpha as f32 / 255.0).clamp(0.0, 0.96);
        let ga = if tr_card.color_rgb.is_some() || tr_card.color2_rgb.is_some() {
            ga_base.max(0.46)
        } else {
            ga_base
        };
        if ga > 0.01 {
            let stops = [
                D2D1_GRADIENT_STOP {
                    position: 0.0,
                    color: rgba_u8(br_r, br_g, br_b, (ga * 1.12).min(0.96)),
                },
                D2D1_GRADIENT_STOP {
                    position: 1.0,
                    color: rgba_u8(
                        br_bot_r,
                        br_bot_g,
                        br_bot_b,
                        (ga * 0.94).min(0.95),
                    ),
                },
            ];
            let (gp0, gp1) = grad_endpoints_for_rect((x, y, x + w, y + h), tr_card.gradient_angle);
            if let Some(grad) = create_linear_grad(rt, gp0, gp1, &stops) {
                rt.FillRoundedRectangle(&card_rr, &grad);
            }
        }

        // Theme-specific interior ornament
        match wcfg.theme {
            ExtWidgetTheme::AutumnCrystal => {
                if wcfg.show_dividers {
                    let spine = D2D1_ROUNDED_RECT {
                        rect: D2D_RECT_F {
                            left: x + 5.0 * s,
                            top: y + 10.0 * s,
                            right: x + 7.6 * s,
                            bottom: y + h - 10.0 * s,
                        },
                        radiusX: 1.3 * s,
                        radiusY: 1.3 * s,
                    };
                    if let Ok(sp_br) = rt.CreateSolidColorBrush(&rgba_u8(rr_r, rr_g, rr_b, 0.72), None) {
                        rt.FillRoundedRectangle(&spine, &sp_br);
                    }
                }
            }
            ExtWidgetTheme::FloatingIsland | ExtWidgetTheme::CelestialSunDial => {
                let inner_rr = D2D1_ROUNDED_RECT {
                    rect: D2D_RECT_F {
                        left: x + 3.0 * s,
                        top: y + 3.0 * s,
                        right: x + w - 3.0 * s,
                        bottom: y + h - 3.0 * s,
                    },
                    radiusX: (rad - 2.5 * s).max(2.0),
                    radiusY: (rad - 2.5 * s).max(2.0),
                };
                if let Ok(in_br) = rt.CreateSolidColorBrush(&rgba_u8(rr_r, rr_g, rr_b, 0.16), None) {
                    rt.DrawRoundedRectangle(&inner_rr, &in_br, 1.0 * s, None);
                }
            }
            ExtWidgetTheme::CyberHud => {
                if let Ok(cb) = rt.CreateSolidColorBrush(&rgba_u8(rr_r, rr_g, rr_b, 0.92), None) {
                    let cl = 10.0 * s;
                    let th = 1.8 * s;
                    // Top-left & bottom-right HUD corner brackets
                    rt.FillRectangle(
                        &D2D_RECT_F { left: x, top: y, right: x + cl, bottom: y + th },
                        &cb,
                    );
                    rt.FillRectangle(
                        &D2D_RECT_F { left: x, top: y, right: x + th, bottom: y + cl },
                        &cb,
                    );
                    rt.FillRectangle(
                        &D2D_RECT_F {
                            left: x + w - cl,
                            top: y + h - th,
                            right: x + w,
                            bottom: y + h,
                        },
                        &cb,
                    );
                    rt.FillRectangle(
                        &D2D_RECT_F {
                            left: x + w - th,
                            top: y + h - cl,
                            right: x + w,
                            bottom: y + h,
                        },
                        &cb,
                    );
                }
            }
            ExtWidgetTheme::NeonMatrix => {
                let top_glow = D2D_RECT_F {
                    left: x + 14.0 * s,
                    top: y + 1.0 * s,
                    right: x + w - 14.0 * s,
                    bottom: y + 3.2 * s,
                };
                if let Ok(nb) = rt.CreateSolidColorBrush(&rgba_u8(rr_r, rr_g, rr_b, 0.78), None) {
                    rt.FillRectangle(&top_glow, &nb);
                }
            }
            ExtWidgetTheme::WinterSnow
            | ExtWidgetTheme::HearthFire
            | ExtWidgetTheme::LiquidWater
            | ExtWidgetTheme::SakuraWind
            | ExtWidgetTheme::CosmicLens
            | ExtWidgetTheme::TeslaPlasma => {
                let top_bar = D2D1_ROUNDED_RECT {
                    rect: D2D_RECT_F {
                        left: x + 18.0 * s,
                        top: y + 1.2 * s,
                        right: x + w - 18.0 * s,
                        bottom: y + 3.2 * s,
                    },
                    radiusX: 1.0 * s,
                    radiusY: 1.0 * s,
                };
                if let Ok(ab_br) = rt.CreateSolidColorBrush(&rgba_u8(rr_r, rr_g, rr_b, 0.62), None) {
                    rt.FillRoundedRectangle(&top_bar, &ab_br);
                }
            }
            ExtWidgetTheme::NordicMinimal | ExtWidgetTheme::EditorialSerif => {}
            ExtWidgetTheme::BrushedTitanium => {
                // Anisotropic brushed metal grain + 4 recessed corner Torx bolts
                if let Ok(hl) = rt.CreateSolidColorBrush(&rgba_u8(244, 248, 255, 0.065), None) {
                    let mut ly = y + 6.0 * s;
                    while ly < y + h - 6.0 * s {
                        rt.DrawLine(Vector2 { X: x + 4.0 * s, Y: ly }, Vector2 { X: x + w - 4.0 * s, Y: ly }, &hl, 0.65 * s, None);
                        ly += 3.0 * s;
                    }
                }
                if let (Ok(bd), Ok(brim)) = (rt.CreateSolidColorBrush(&rgba_u8(20, 24, 30, 0.75), None), rt.CreateSolidColorBrush(&rgba_u8(224, 232, 242, 0.62), None)) {
                    let bp = 8.5 * s;
                    for &(bx, by) in &[(x + bp, y + bp), (x + w - bp, y + bp), (x + bp, y + h - bp), (x + w - bp, y + h - bp)] {
                        rt.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: bx, Y: by }, radiusX: 2.3 * s, radiusY: 2.3 * s }, &bd);
                        rt.DrawEllipse(&D2D1_ELLIPSE { point: Vector2 { X: bx, Y: by }, radiusX: 2.3 * s, radiusY: 2.3 * s }, &brim, 0.7 * s, None);
                    }
                }
            }
            ExtWidgetTheme::WarmHiFiBrass => {
                // Champagne brass hairline inlay + warm top tube-filament glow bar
                let inner_rr = D2D1_ROUNDED_RECT {
                    rect: D2D_RECT_F { left: x + 4.0 * s, top: y + 4.0 * s, right: x + w - 4.0 * s, bottom: y + h - 4.0 * s },
                    radiusX: (rad - 3.0 * s).max(2.0),
                    radiusY: (rad - 3.0 * s).max(2.0),
                };
                if let Ok(brs) = rt.CreateSolidColorBrush(&rgba_u8(232, 186, 112, 0.34), None) {
                    rt.DrawRoundedRectangle(&inner_rr, &brs, 0.9 * s, None);
                }
                if let Ok(glow) = rt.CreateSolidColorBrush(&rgba_u8(255, 162, 54, 0.45), None) {
                    rt.FillRoundedRectangle(
                        &D2D1_ROUNDED_RECT { rect: D2D_RECT_F { left: x + 20.0 * s, top: y + 1.5 * s, right: x + w - 20.0 * s, bottom: y + 3.6 * s }, radiusX: 1.0 * s, radiusY: 1.0 * s },
                        &glow,
                    );
                }
            }
            ExtWidgetTheme::SwissChronograph => {
                // Engine-turned guilloché wave lines + synthetic ruby pivot dot
                if let Ok(gu) = rt.CreateSolidColorBrush(&rgba_u8(154, 192, 246, 0.09), None) {
                    let mut gy = y + 10.0 * s;
                    while gy < y + h - 8.0 * s {
                        for seg in 0..16 {
                            let f0 = seg as f32 / 16.0;
                            let f1 = (seg + 1) as f32 / 16.0;
                            rt.DrawLine(
                                Vector2 { X: x + 8.0 * s + (w - 16.0 * s) * f0, Y: gy + (f0 * 16.0).sin() * 1.2 * s },
                                Vector2 { X: x + 8.0 * s + (w - 16.0 * s) * f1, Y: gy + (f1 * 16.0).sin() * 1.2 * s },
                                &gu,
                                0.65 * s,
                                None,
                            );
                        }
                        gy += 8.0 * s;
                    }
                }
                if let Ok(rub) = rt.CreateSolidColorBrush(&rgba_u8(216, 28, 72, 0.92), None) {
                    rt.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: x + w - 12.0 * s, Y: y + 12.0 * s }, radiusX: 2.5 * s, radiusY: 2.5 * s }, &rub);
                }
            }
            ExtWidgetTheme::SaddleLeather => {
                // Perimeter waxed cream saddle stitching + corner brass rivets
                if let Ok(th) = rt.CreateSolidColorBrush(&rgba_u8(246, 230, 198, 0.72), None) {
                    let ins = 5.5 * s;
                    let mut sx = x + 14.0 * s;
                    while sx < x + w - 16.0 * s {
                        rt.DrawLine(Vector2 { X: sx, Y: y + ins }, Vector2 { X: sx + 4.5 * s, Y: y + ins - 0.5 * s }, &th, 0.95 * s, None);
                        rt.DrawLine(Vector2 { X: sx, Y: y + h - ins }, Vector2 { X: sx + 4.5 * s, Y: y + h - ins - 0.5 * s }, &th, 0.95 * s, None);
                        sx += 8.5 * s;
                    }
                }
                if let Ok(brs) = rt.CreateSolidColorBrush(&rgba_u8(214, 168, 86, 0.90), None) {
                    for &(rx, ry) in &[(x + 11.0 * s, y + 11.0 * s), (x + w - 11.0 * s, y + 11.0 * s)] {
                        rt.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: rx, Y: ry }, radiusX: 2.4 * s, radiusY: 2.4 * s }, &brs);
                    }
                }
            }
            ExtWidgetTheme::CarbonStealth => {
                // 2x2 diagonal twill carbon fiber weave + Leica ruby dot
                let cell = 7.0 * s;
                if let Ok(cw) = rt.CreateSolidColorBrush(&rgba_u8(68, 72, 82, 0.22), None) {
                    let mut cy = y + 5.0 * s;
                    let mut row = 0usize;
                    while cy < y + h - 6.0 * s {
                        let mut cx = x + 5.0 * s;
                        let mut col = 0usize;
                        while cx < x + w - 8.0 * s {
                            if (col + row) % 4 < 2 {
                                rt.FillRectangle(&D2D_RECT_F { left: cx, top: cy, right: cx + cell - 1.0 * s, bottom: cy + cell * 0.48 }, &cw);
                            }
                            cx += cell;
                            col += 1;
                        }
                        cy += cell * 0.72;
                        row += 1;
                    }
                }
                if let Ok(rd) = rt.CreateSolidColorBrush(&rgba_u8(228, 22, 52, 0.95), None) {
                    rt.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: x + w - 11.0 * s, Y: y + 11.0 * s }, radiusX: 2.4 * s, radiusY: 2.4 * s }, &rd);
                }
            }
            ExtWidgetTheme::CrystalWater => {
                // Sunlit crystal water meniscus gloss + caustic wave ribbons + refractive droplets
                let men_rr = D2D1_ROUNDED_RECT {
                    rect: D2D_RECT_F { left: x + 5.0 * s, top: y + 2.0 * s, right: x + w - 5.0 * s, bottom: y + (h * 0.34).min(38.0 * s) },
                    radiusX: (rad - 2.0 * s).max(4.0),
                    radiusY: (rad - 2.0 * s).max(4.0),
                };
                if let Some(mg) = create_linear_grad(
                    rt,
                    (men_rr.rect.left, men_rr.rect.top),
                    (men_rr.rect.left, men_rr.rect.bottom),
                    &[
                        D2D1_GRADIENT_STOP { position: 0.0, color: rgba_u8(232, 254, 255, 0.34) },
                        D2D1_GRADIENT_STOP { position: 1.0, color: rgba_u8(116, 236, 255, 0.0) },
                    ],
                ) {
                    rt.FillRoundedRectangle(&men_rr, &mg);
                }
                if let Ok(cb) = rt.CreateSolidColorBrush(&rgba_u8(186, 250, 255, 0.16), None) {
                    for seg in 0..18 {
                        let f0 = seg as f32 / 18.0;
                        let f1 = (seg + 1) as f32 / 18.0;
                        rt.DrawLine(
                            Vector2 { X: x + 10.0 * s + (w - 20.0 * s) * f0, Y: y + h * 0.52 + (f0 * 7.5).sin() * 6.0 * s },
                            Vector2 { X: x + 10.0 * s + (w - 20.0 * s) * f1, Y: y + h * 0.52 + (f1 * 7.5).sin() * 6.0 * s },
                            &cb,
                            1.8 * s,
                            None,
                        );
                    }
                }
                if let Ok(dh) = rt.CreateSolidColorBrush(&rgba_u8(244, 255, 255, 0.62), None) {
                    rt.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: x + w - 16.0 * s, Y: y + 15.0 * s }, radiusX: 2.2 * s, radiusY: 2.5 * s }, &dh);
                }
            }
            ExtWidgetTheme::ZenCalmMist => {
                // Soft drifting sage mist halo + calming Zen raked-sand edge lines
                if let Ok(mv) = rt.CreateSolidColorBrush(&rgba_u8(220, 238, 230, 0.09), None) {
                    rt.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: x + w * 0.72, Y: y + h * 0.35 }, radiusX: w * 0.36, radiusY: h * 0.28 }, &mv);
                }
                if let Ok(rk) = rt.CreateSolidColorBrush(&rgba_u8(204, 226, 218, 0.18), None) {
                    rt.DrawLine(Vector2 { X: x + 14.0 * s, Y: y + h - 7.0 * s }, Vector2 { X: x + w - 14.0 * s, Y: y + h - 7.0 * s }, &rk, 0.8 * s, None);
                    rt.DrawLine(Vector2 { X: x + 22.0 * s, Y: y + h - 11.0 * s }, Vector2 { X: x + w - 22.0 * s, Y: y + h - 11.0 * s }, &rk, 0.7 * s, None);
                }
            }
            ExtWidgetTheme::CarraraMarble => {
                // Organic smoky Carrara marble veining + kintsugi champagne-gold hairline
                if let Ok(mv) = rt.CreateSolidColorBrush(&rgba_u8(136, 132, 126, 0.20), None) {
                    for seg in 0..16 {
                        let f0 = seg as f32 / 16.0;
                        let f1 = (seg + 1) as f32 / 16.0;
                        rt.DrawLine(
                            Vector2 { X: x + 8.0 * s + (w - 16.0 * s) * f0, Y: y + h * 0.34 + (f0 * 5.0).sin() * (h * 0.18) },
                            Vector2 { X: x + 8.0 * s + (w - 16.0 * s) * f1, Y: y + h * 0.34 + (f1 * 5.0).sin() * (h * 0.18) },
                            &mv,
                            1.8 * s,
                            None,
                        );
                    }
                }
                if let Ok(gd) = rt.CreateSolidColorBrush(&rgba_u8(198, 158, 76, 0.44), None) {
                    for seg in 0..16 {
                        let f0 = seg as f32 / 16.0;
                        let f1 = (seg + 1) as f32 / 16.0;
                        rt.DrawLine(
                            Vector2 { X: x + 8.0 * s + (w - 16.0 * s) * f0, Y: y + h * 0.68 + (f0 * 4.5 + 1.2).sin() * (h * 0.14) },
                            Vector2 { X: x + 8.0 * s + (w - 16.0 * s) * f1, Y: y + h * 0.68 + (f1 * 4.5 + 1.2).sin() * (h * 0.14) },
                            &gd,
                            0.95 * s,
                            None,
                        );
                    }
                }
            }
            ExtWidgetTheme::RainWindowGlass
            | ExtWidgetTheme::AbyssalTrench
            | ExtWidgetTheme::GlacierIce
            | ExtWidgetTheme::GoldenHourHorizon
            | ExtWidgetTheme::HoroRoyalOak
            | ExtWidgetTheme::HoroPatekMoon
            | ExtWidgetTheme::HoroTourbillonGold
            | ExtWidgetTheme::HoroDaytonaCeramic
            | ExtWidgetTheme::HoroSubmarinerEmerald
            | ExtWidgetTheme::HoroIwcTitanium
            | ExtWidgetTheme::HoroBreguetPorcelain
            | ExtWidgetTheme::HoroForgedCarbon
            | ExtWidgetTheme::HoroWalnutRegulator
            | ExtWidgetTheme::HoroSapphireObsidian => {}
        }

        let ra = (wcfg.rim_alpha as f32 / 255.0).clamp(0.0, 1.0);
        if ra > 0.01 || tr_card.border_rgb.is_some() {
            let rw = (wcfg.rim_width_tenths as f32 / 10.0).clamp(0.5, 3.0) * s;
            let rim_a = if card_hov {
                (ra + 0.22).min(1.0)
            } else if tr_card.border_rgb.is_some() {
                ra.max(0.55)
            } else {
                ra
            };
            if let Ok(rim_br) = rt.CreateSolidColorBrush(&rgba_u8(rr_r, rr_g, rr_b, rim_a), None) {
                rt.DrawRoundedRectangle(&card_rr, &rim_br, rw, None);
            }
        }
        }
    }

    if wcfg.show_plate && tr_card.visible && wcfg.hover_fx != TaskbarVizStyle::Off {
        let base_pal = ThemePalette {
            accent: (ar, ag, ab),
            accent_light: pal.accent_light,
            accent_deep: pal.accent_deep,
            rim: (ar, ag, ab),
            text_primary: (tr, tg, tb),
            text_muted: pal.text_muted,
            glass_top: (br_r, br_g, br_b),
            glass_bot: (br_bot_r, br_bot_g, br_bot_b),
            preview_a: pal.preview_a,
            preview_b: pal.preview_b,
            preview_c: pal.preview_c,
            swatches: pal.swatches,
        };
        let op = if wcfg.show_plate { 76 } else { 68 };
        paint_widget_elemental_fx(
            rt,
            d2d,
            x,
            y,
            x + w,
            y + h,
            (x, y, w, h),
            rad.max(4.0 * s),
            s,
            wcfg.hover_fx,
            op,
            &base_pal,
            !wcfg.match_wallpaper && !wcfg.use_custom_color && tr_card.color_rgb.is_none(),
            fx_viz,
        );
    }
    end_elem_xf_slot(rt, g_card);

    let slot_bounds: std::cell::RefCell<[Option<(f32, f32, f32, f32)>; 5]> =
        std::cell::RefCell::new([None; 5]);
    let fg_key = if out_boxes.is_none() && fg_cache.is_some() {
        ext_widget_fg_key(
            id,
            s,
            w,
            h,
            wcfg,
            cfg,
            state,
            cc_snap,
            &pal,
            hover_zone,
            custom_fonts.is_some(),
        )
    } else {
        0
    };
    let draw_fg = |rt: &ID2D1RenderTarget| {
    // 2. Font resolution (with per-element custom font & text size multiplier)
    let fs = (wcfg.font_scale_pct as f32 / 100.0).clamp(0.75, 1.35) * s * PT_TO_PX;
    let fam_candidates: &[PCWSTR] = match wcfg.font_style {
        ExtFontStyle::DefaultSans => &[w!("SF Pro Display"), w!("Segoe UI")],
        ExtFontStyle::EditorialSerif => &[w!("Georgia"), w!("Times New Roman"), w!("Segoe UI")],
        ExtFontStyle::CondensedBold => &[w!("Morganite"), w!("Impact"), w!("Segoe UI Semibold")],
        ExtFontStyle::CyberMono => &[w!("Consolas"), w!("Cascadia Mono"), w!("Segoe UI")],
        ExtFontStyle::CalligraphyScript => &[w!("Tuesday Night"), w!("Segoe Script"), w!("Segoe UI")],
        ExtFontStyle::GeometricBold => &[w!("Metropolis"), w!("Segoe UI Bold"), w!("Segoe UI")],
    };
    let body_fams: &[PCWSTR] = &[w!("SF Pro Display"), w!("Segoe UI")];
    let elem_fnt = |i: usize| -> &str {
        if !wcfg.elem_fonts[i].is_empty() {
            &wcfg.elem_fonts[i]
        } else {
            &wcfg.custom_font
        }
    };
    let elem_fs = |i: usize| -> f32 {
        fs * (wcfg.elem_transforms[i].font_scale_pct as f32 / 100.0).clamp(0.50, 2.20)
    };

    let align_dwrite = match wcfg.align {
        ClockAlign::Left => DWRITE_TEXT_ALIGNMENT_LEADING,
        ClockAlign::Center => DWRITE_TEXT_ALIGNMENT_CENTER,
        ClockAlign::Right => DWRITE_TEXT_ALIGNMENT_TRAILING,
    };

    let Some(tf_hdr) = make_resolved_tf_named(
        dw,
        custom_fonts,
        elem_fnt(0),
        body_fams,
        7.5 * elem_fs(0),
        DWRITE_FONT_WEIGHT_BOLD,
        align_dwrite,
        DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
    ) else {
        return;
    };
    let big_fams: &[PCWSTR] = if id == ExtWidgetId::Weather
        && wcfg.font_style == ExtFontStyle::CalligraphyScript
        && elem_fnt(1).is_empty()
    {
        &[w!("Georgia"), w!("SF Pro Display"), w!("Segoe UI")]
    } else {
        fam_candidates
    };
    let Some(tf_big) = make_resolved_tf_named(
        dw,
        custom_fonts,
        elem_fnt(1),
        big_fams,
        16.0 * elem_fs(1),
        DWRITE_FONT_WEIGHT_BOLD,
        align_dwrite,
        DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
    ) else {
        return;
    };
    let Some(tf_title) = make_resolved_tf_named(
        dw,
        custom_fonts,
        elem_fnt(1),
        fam_candidates,
        10.2 * elem_fs(1),
        DWRITE_FONT_WEIGHT_SEMI_BOLD,
        align_dwrite,
        DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
    ) else {
        return;
    };
    let Some(tf_body) = make_resolved_tf_named(
        dw,
        custom_fonts,
        elem_fnt(2),
        body_fams,
        8.6 * elem_fs(2),
        DWRITE_FONT_WEIGHT_MEDIUM,
        align_dwrite,
        DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
    ) else {
        return;
    };
    let Some(tf_sub) = make_resolved_tf_named(
        dw,
        custom_fonts,
        elem_fnt(2),
        body_fams,
        7.4 * elem_fs(2),
        DWRITE_FONT_WEIGHT_REGULAR,
        align_dwrite,
        DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
    ) else {
        return;
    };
    let Some(tf_center) = make_resolved_tf_named(
        dw,
        custom_fonts,
        elem_fnt(3),
        body_fams,
        7.6 * elem_fs(3),
        DWRITE_FONT_WEIGHT_BOLD,
        DWRITE_TEXT_ALIGNMENT_CENTER,
        DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
    ) else {
        return;
    };
    let Some(tf_right) = make_resolved_tf_named(
        dw,
        custom_fonts,
        elem_fnt(2),
        body_fams,
        8.0 * elem_fs(2),
        DWRITE_FONT_WEIGHT_SEMI_BOLD,
        DWRITE_TEXT_ALIGNMENT_TRAILING,
        DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
    ) else {
        return;
    };

    let box_ov = wcfg.elem_transforms[3].color_rgb;
    let (b3_r, b3_g, b3_b) = box_ov.map(u32_to_rgb).unwrap_or((ar, ag, ab));
    let Ok(br_acc) = rt.CreateSolidColorBrush(&rgba_u8(ar, ag, ab, 1.0), None) else { return };
    let Ok(br_acc_dim) = rt.CreateSolidColorBrush(
        &rgba_u8(b3_r, b3_g, b3_b, if box_ov.is_some() { 0.36 } else { 0.22 }),
        None,
    ) else { return };
    let Ok(br_acc_mid) = rt.CreateSolidColorBrush(
        &rgba_u8(b3_r, b3_g, b3_b, if box_ov.is_some() { 0.68 } else { 0.55 }),
        None,
    ) else { return };
    let Ok(br_box_rim) = rt.CreateSolidColorBrush(
        &rgba_u8(
            wcfg.elem_transforms[3].border_rgb.map(|c| u32_to_rgb(c).0).unwrap_or(b3_r),
            wcfg.elem_transforms[3].border_rgb.map(|c| u32_to_rgb(c).1).unwrap_or(b3_g),
            wcfg.elem_transforms[3].border_rgb.map(|c| u32_to_rgb(c).2).unwrap_or(b3_b),
            if box_ov.is_some() || wcfg.elem_transforms[3].border_rgb.is_some() { 0.78 } else { 0.28 },
        ),
        None,
    ) else { return };
    let Ok(br_txt) = rt.CreateSolidColorBrush(&rgba_u8(tr, tg, tb, 0.96), None) else { return };
    let Ok(br_sub) = rt.CreateSolidColorBrush(&rgba_u8(tr, tg, tb, 0.66), None) else { return };
    let Ok(br_muted) = rt.CreateSolidColorBrush(&rgba_u8(tr, tg, tb, 0.36), None) else { return };
    let Ok(br_dark) = rt.CreateSolidColorBrush(&rgba_u8(20, 14, 10, 0.98), None) else { return };
    let Ok(br_coral) = rt.CreateSolidColorBrush(&rgba_u8(244, 112, 92, 0.95), None) else { return };
    let Ok(br_sage) = rt.CreateSolidColorBrush(&rgba_u8(126, 217, 135, 0.95), None) else { return };
    let br_txt_sh = if wcfg.show_shadow {
        rt.CreateSolidColorBrush(&rgba_u8(0, 0, 0, 0.45), None).ok()
    } else {
        None
    };

    let br_slot: [Option<windows::Win32::Graphics::Direct2D::ID2D1SolidColorBrush>; 5] =
        std::array::from_fn(|i| make_elem_brush(rt, wcfg.elem_transforms[i].color_rgb, 0.98));
    let record_slot_rc = |slot_i: usize, rc: D2D_RECT_F| {
        if slot_i >= 5 {
            return;
        }
        let mut b = slot_bounds.borrow_mut();
        let next = (rc.left, rc.top, rc.right, rc.bottom);
        b[slot_i] = Some(match b[slot_i] {
            Some((l, t, r, bt)) => (l.min(next.0), t.min(next.1), r.max(next.2), bt.max(next.3)),
            None => next,
        });
    };

    let p_hdr = tf_hdr.as_raw();
    let p_big = tf_big.as_raw();
    let p_title = tf_title.as_raw();
    let p_body = tf_body.as_raw();
    let p_sub = tf_sub.as_raw();
    let p_center = tf_center.as_raw();
    let p_right = tf_right.as_raw();

    let draw_txt_clipped = |rt_t: &ID2D1RenderTarget,
                            text: &str,
                            tf: &windows::Win32::Graphics::DirectWrite::IDWriteTextFormat,
                            rc: D2D_RECT_F,
                            br: &windows::Win32::Graphics::Direct2D::ID2D1SolidColorBrush| {
        let raw = tf.as_raw();
        let slot_i = if raw == p_hdr {
            0
        } else if raw == p_big || raw == p_title || raw == p_body {
            1
        } else if raw == p_center {
            3
        } else if raw == p_sub || raw == p_right {
            2
        } else {
            2
        };
        record_slot_rc(slot_i, rc);
        let tr_el = wcfg.elem_transforms[slot_i];
        if !wcfg.is_slot_visible(slot_i) {
            return;
        }
        let mut none_boxes: Option<&mut [ClockElemBox]> = None;
        let g = begin_elem_xf_slot(rt_t, slot_i, tr_el, (rc.left, rc.top, rc.right, rc.bottom), s, &mut none_boxes);
        let eff_br = br_slot[slot_i].as_ref().unwrap_or(br);
        draw_shadowed_text(rt_t, text, tf, rc, eff_br, br_txt_sh.as_ref(), if wcfg.show_shadow { 0.8 * s } else { 0.0 }, true);
        end_elem_xf_slot(rt_t, g);
    };

    rt.PushAxisAlignedClip(&card_rect, D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);

    let x0 = x + pad;
    let x1 = x + w - pad;
    let mut y_cur = y + pad * 0.75;

    // Helper for small interactive pill buttons inside widgets (slot 3: Actions/Icons/Pills)
    let br_p3_dim = make_elem_brush(rt, wcfg.elem_transforms[3].color_rgb, 0.24);
    let br_p3_mid = make_elem_brush(rt, wcfg.elem_transforms[3].color_rgb, 0.56);
    let draw_pill = |bx0: f32, by0: f32, bx1: f32, by1: f32, label: &str, active: bool, hov: bool| {
        let rc = D2D_RECT_F { left: bx0, top: by0, right: bx1, bottom: by1 };
        record_slot_rc(3, rc);
        let tr_p = wcfg.elem_transforms[3];
        if !wcfg.is_slot_visible(3) {
            return;
        }
        let mut none_boxes: Option<&mut [ClockElemBox]> = None;
        let g = begin_elem_xf_slot(rt, 3, tr_p, (bx0, by0, bx1, by1), s, &mut none_boxes);
        let rr = D2D1_ROUNDED_RECT {
            rect: rc,
            radiusX: 5.5 * s,
            radiusY: 5.5 * s,
        };
        if active && (tr_p.color_rgb.is_some() || tr_p.color2_rgb.is_some()) {
            let (c1, c2, ang) = resolve_elem_grad_colors(Some(&tr_p), 0.98, rgba_u8(ar, ag, ab, 1.0), rgba_u8(ar, ag, ab, 0.85));
            let (gp0, gp1) = grad_endpoints_for_rect((bx0, by0, bx1, by1), ang);
            if let Some(grad) = create_linear_grad(rt, gp0, gp1, &[D2D1_GRADIENT_STOP { position: 0.0, color: c1 }, D2D1_GRADIENT_STOP { position: 1.0, color: c2 }]) {
                rt.FillRoundedRectangle(&rr, &grad);
            }
        } else {
            let fill_br = if active {
                br_slot[3].as_ref().unwrap_or(&br_acc)
            } else if hov {
                br_p3_mid.as_ref().unwrap_or(&br_acc_mid)
            } else {
                br_p3_dim.as_ref().unwrap_or(&br_acc_dim)
            };
            rt.FillRoundedRectangle(&rr, fill_br);
        }
        if tr_p.border_rgb.is_some() {
            rt.DrawRoundedRectangle(&rr, &br_box_rim, (tr_p.border_width.max(1) as f32) * 0.8 * s, None);
        }
        // Call direct text draw without re-applying begin_elem_xf_slot(3) (fixes double-transform bug!)
        draw_shadowed_text(
            rt,
            label,
            &tf_center,
            rr.rect,
            if active { &br_dark } else { br_slot[3].as_ref().unwrap_or(&br_txt) },
            None,
            0.0,
            true,
        );
        end_elem_xf_slot(rt, g);
    };

    // Helper for inner boxes, sub-cards, input boxes & checkboxes (slot 3: Boxes)
    let draw_sub_box = |bx0: f32, by0: f32, bx1: f32, by1: f32, rad_px: f32, active: bool, hov: bool, border: bool| {
        let rc = D2D_RECT_F { left: bx0, top: by0, right: bx1, bottom: by1 };
        record_slot_rc(3, rc);
        let tr_p = wcfg.elem_transforms[3];
        if !wcfg.is_slot_visible(3) {
            return;
        }
        let mut none_boxes: Option<&mut [ClockElemBox]> = None;
        let g = begin_elem_xf_slot(rt, 3, tr_p, (bx0, by0, bx1, by1), s, &mut none_boxes);
        let rr = D2D1_ROUNDED_RECT { rect: rc, radiusX: rad_px, radiusY: rad_px };
        if active && (tr_p.color_rgb.is_some() || tr_p.color2_rgb.is_some()) {
            let (c1, c2, ang) = resolve_elem_grad_colors(Some(&tr_p), 0.96, rgba_u8(ar, ag, ab, 1.0), rgba_u8(ar, ag, ab, 0.85));
            let (gp0, gp1) = grad_endpoints_for_rect((bx0, by0, bx1, by1), ang);
            if let Some(grad) = create_linear_grad(rt, gp0, gp1, &[D2D1_GRADIENT_STOP { position: 0.0, color: c1 }, D2D1_GRADIENT_STOP { position: 1.0, color: c2 }]) {
                rt.FillRoundedRectangle(&rr, &grad);
            }
        } else {
            let fill_br = if active {
                br_slot[3].as_ref().unwrap_or(&br_acc)
            } else if hov {
                br_p3_mid.as_ref().unwrap_or(&br_acc_mid)
            } else {
                br_p3_dim.as_ref().unwrap_or(&br_acc_dim)
            };
            rt.FillRoundedRectangle(&rr, fill_br);
        }
        if border || box_ov.is_some() || tr_p.border_rgb.is_some() {
            let bw = if tr_p.border_width > 0 { tr_p.border_width as f32 * 0.7 * s } else { 1.1 * s };
            rt.DrawRoundedRectangle(&rr, &br_box_rim, bw, None);
        }
        end_elem_xf_slot(rt, g);
    };

    let br_box_fill = br_slot[3].as_ref().unwrap_or(&br_acc);
    let br_div_fill = br_slot[4].as_ref().unwrap_or(&br_acc_mid);
    macro_rules! with_box_slot {
        ($bx0:expr, $by0:expr, $bx1:expr, $by1:expr, $draw:block) => {{
            let rc = D2D_RECT_F { left: $bx0, top: $by0, right: $bx1, bottom: $by1 };
            record_slot_rc(3, rc);
            let tr_p = wcfg.elem_transforms[3];
            if wcfg.is_slot_visible(3) {
                let mut none_boxes: Option<&mut [ClockElemBox]> = None;
                let g = begin_elem_xf_slot(rt, 3, tr_p, ($bx0, $by0, $bx1, $by1), s, &mut none_boxes);
                $draw
                end_elem_xf_slot(rt, g);
            }
        }};
    }
    macro_rules! with_slot4 {
        ($bx0:expr, $by0:expr, $bx1:expr, $by1:expr, $draw:block) => {{
            let rc = D2D_RECT_F { left: $bx0, top: $by0, right: $bx1, bottom: $by1 };
            record_slot_rc(4, rc);
            let tr_s4 = wcfg.elem_transforms[4];
            if wcfg.is_slot_visible(4) {
                let mut none_boxes: Option<&mut [ClockElemBox]> = None;
                let g = begin_elem_xf_slot(rt, 4, tr_s4, ($bx0, $by0, $bx1, $by1), s, &mut none_boxes);
                $draw
                end_elem_xf_slot(rt, g);
            }
        }};
    }
    let draw_ext_divider = |y_div: f32| {
        let rc = D2D_RECT_F { left: x0, top: y_div - 2.0 * s, right: x1, bottom: y_div + 3.0 * s };
        record_slot_rc(4, rc);
        let tr_d = wcfg.elem_transforms[4];
        if !wcfg.is_slot_visible(4) {
            return;
        }
        let mut none_boxes: Option<&mut [ClockElemBox]> = None;
        let g = begin_elem_xf_slot(rt, 4, tr_d, (rc.left, rc.top, rc.right, rc.bottom), s, &mut none_boxes);
        let def_c = rgba_u8(ar, ag, ab, 0.34);
        let (c1, c2, ang) = resolve_elem_grad_colors(Some(&tr_d), 0.78, def_c, def_c);
        let (gp0, gp1) = grad_endpoints_for_rect((x0, y_div, x1, y_div + 1.5 * s), ang);
        let thick = if tr_d.border_width > 0 { (tr_d.border_width as f32 * 0.6).max(1.0) * s } else { 1.2 * s };
        if let Some(g_rgb) = tr_d.glow_rgb {
            let (gr, gg, gb) = u32_to_rgb(g_rgb);
            if let Ok(gb_br) = rt.CreateSolidColorBrush(&rgba_u8(gr, gg, gb, 0.36), None) {
                rt.FillRectangle(&D2D_RECT_F { left: x0, top: y_div - 1.0 * s, right: x1, bottom: y_div + thick + 1.0 * s }, &gb_br);
            }
        }
        if let Some(grad) = create_linear_grad(rt, gp0, gp1, &[D2D1_GRADIENT_STOP { position: 0.0, color: c1 }, D2D1_GRADIENT_STOP { position: 1.0, color: c2 }]) {
            rt.FillRectangle(&D2D_RECT_F { left: x0, top: y_div, right: x1, bottom: y_div + thick }, &grad);
        } else {
            rt.FillRectangle(&D2D_RECT_F { left: x0, top: y_div, right: x1, bottom: y_div + thick }, br_div_fill);
        }
        end_elem_xf_slot(rt, g);
    };

    let is_btn_hov = |b: u8| matches!(hover_zone, WidgetHitZone::ExtBtn(hid, hb) if hid == id && hb == b);

    match id {
        // =====================================================================
        // 1. LIVE WEATHER & 7-DAY FORECAST (Standard + Celestial Sun-Dial)
        // =====================================================================
        ExtWidgetId::Weather => {
            let snap = &state.weather[(wcfg.mode_idx as usize).min(3)];
            let use_f = wcfg.feats[0];
            let to_u = |c: f32| if use_f { c * 1.8 + 32.0 } else { c };
            let unit_ch = if use_f { "°F" } else { "°C" };

            if wcfg.theme == ExtWidgetTheme::CelestialSunDial {
                // =============================================================
                // CELESTIAL SUN-DIAL WEATHER DASHBOARD (24h Orbital Dial + Solar Arc + 7-Day)
                // =============================================================
                let tf_micro_c = make_resolved_tf_named(
                    dw,
                    custom_fonts,
                    elem_fnt(3),
                    body_fams,
                    6.0 * elem_fs(3),
                    DWRITE_FONT_WEIGHT_SEMI_BOLD,
                    DWRITE_TEXT_ALIGNMENT_CENTER,
                    DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
                );
                let tf_micro = tf_micro_c.as_ref().unwrap_or(&tf_center);

                if wcfg.show_header {
                    let hdr = format!("CELESTIAL • {}", snap.city.to_uppercase());
                    draw_txt_clipped(
                        rt,
                        &hdr,
                        &tf_hdr,
                        D2D_RECT_F {
                            left: x0,
                            top: y_cur,
                            right: x1 - 110.0 * s,
                            bottom: y_cur + 16.0 * s,
                        },
                        &br_acc,
                    );
                    draw_pill(
                        x1 - 106.0 * s,
                        y_cur,
                        x1 - 44.0 * s,
                        y_cur + 17.0 * s,
                        "City ↻",
                        false,
                        is_btn_hov(1),
                    );
                    draw_pill(
                        x1 - 40.0 * s,
                        y_cur,
                        x1,
                        y_cur + 17.0 * s,
                        unit_ch,
                        use_f,
                        is_btn_hov(0),
                    );
                    y_cur += 20.0 * s;
                }

                let now = chrono_local();
                let now_min = (now.hour as u16) * 60 + (now.minute as u16);
                let sr = snap.sunrise_min.min(1430);
                let ss = snap.sunset_min.max(sr + 60).min(1439);
                let is_day = now_min >= sr && now_min <= ss;
                let solar_t = if now_min <= sr {
                    0.0
                } else if now_min >= ss {
                    1.0
                } else {
                    (now_min - sr) as f32 / (ss - sr).max(1) as f32
                };

                // Helper to draw a tiny crisp vector weather glyph (0=sun, 1=cloud, 2=rain)
                let draw_sky_glyph = |gx: f32, gy: f32, gr: f32, icon_kind: u8| {
                    if !wcfg.is_slot_visible(3) {
                        return;
                    }
                    match icon_kind {
                        0 => {
                            crate::dock_themes::with_cached_solid_brush(
                                rt,
                                rgba_u8(255, 204, 72, 0.32),
                                |br| {
                                    rt.FillEllipse(
                                        &D2D1_ELLIPSE {
                                            point: Vector2 { X: gx, Y: gy },
                                            radiusX: gr * 1.45,
                                            radiusY: gr * 1.45,
                                        },
                                        br,
                                    );
                                },
                            );
                            crate::dock_themes::with_cached_solid_brush(
                                rt,
                                rgba_u8(255, 212, 88, 0.98),
                                |br| {
                                    rt.FillEllipse(
                                        &D2D1_ELLIPSE {
                                            point: Vector2 { X: gx, Y: gy },
                                            radiusX: gr * 0.88,
                                            radiusY: gr * 0.88,
                                        },
                                        br,
                                    );
                                },
                            );
                        }
                        1 => {
                            crate::dock_themes::with_cached_solid_brush(
                                rt,
                                rgba_u8(204, 218, 236, 0.90),
                                |br| {
                                    rt.FillEllipse(
                                        &D2D1_ELLIPSE {
                                            point: Vector2 { X: gx - gr * 0.35, Y: gy + gr * 0.12 },
                                            radiusX: gr * 0.72,
                                            radiusY: gr * 0.56,
                                        },
                                        br,
                                    );
                                    rt.FillEllipse(
                                        &D2D1_ELLIPSE {
                                            point: Vector2 { X: gx + gr * 0.25, Y: gy - gr * 0.14 },
                                            radiusX: gr * 0.82,
                                            radiusY: gr * 0.64,
                                        },
                                        br,
                                    );
                                },
                            );
                        }
                        _ => {
                            crate::dock_themes::with_cached_solid_brush(
                                rt,
                                rgba_u8(168, 194, 224, 0.92),
                                |br| {
                                    rt.FillEllipse(
                                        &D2D1_ELLIPSE {
                                            point: Vector2 { X: gx, Y: gy - gr * 0.22 },
                                            radiusX: gr * 0.92,
                                            radiusY: gr * 0.58,
                                        },
                                        br,
                                    );
                                },
                            );
                            crate::dock_themes::with_cached_solid_brush(
                                rt,
                                rgba_u8(96, 196, 255, 0.96),
                                |br| {
                                    for dx in [-0.45f32, 0.0, 0.45] {
                                        rt.DrawLine(
                                            Vector2 { X: gx + dx * gr, Y: gy + gr * 0.45 },
                                            Vector2 { X: gx + dx * gr - 0.8 * s, Y: gy + gr * 1.15 },
                                            br,
                                            1.05 * s,
                                            None,
                                        );
                                    }
                                },
                            );
                        }
                    }
                };

                let show_strip = wcfg.feats[1] && !wcfg.compact_mode;
                let dial_h = if show_strip { 122.0 * s } else { (y + h - y_cur - 12.0 * s).max(96.0 * s) };
                let outer_r = (dial_h * 0.46).clamp(38.0 * s, 56.0 * s);
                let mid_r = outer_r * 0.66;
                let cx = x0 + outer_r + 6.0 * s;
                let cy = y_cur + dial_h * 0.50;

                // Left: Circular 24h Celestial Sun/Moon Orbital Clock Dial
                with_box_slot!(cx - outer_r, cy - outer_r, cx + outer_r, cy + outer_r, {
                    // Deep celestial dial backdrop & dual bezels
                    crate::dock_themes::with_cached_solid_brush(rt, rgba_u8(8, 12, 22, 0.86), |br| {
                        rt.FillEllipse(
                            &D2D1_ELLIPSE { point: Vector2 { X: cx, Y: cy }, radiusX: outer_r, radiusY: outer_r },
                            br,
                        );
                    });
                    crate::dock_themes::with_cached_solid_brush(rt, rgba_u8(16, 23, 38, 0.92), |br| {
                        rt.FillEllipse(
                            &D2D1_ELLIPSE { point: Vector2 { X: cx, Y: cy }, radiusX: mid_r, radiusY: mid_r },
                            br,
                        );
                    });
                    crate::dock_themes::with_cached_solid_brush(rt, rgba_u8(ar, ag, ab, 0.42), |br| {
                        rt.DrawEllipse(
                            &D2D1_ELLIPSE { point: Vector2 { X: cx, Y: cy }, radiusX: outer_r, radiusY: outer_r },
                            br,
                            1.1 * s,
                            None,
                        );
                    });
                    crate::dock_themes::with_cached_solid_brush(rt, rgba_u8(226, 236, 250, 0.24), |br| {
                        rt.DrawEllipse(
                            &D2D1_ELLIPSE { point: Vector2 { X: cx, Y: cy }, radiusX: mid_r, radiusY: mid_r },
                            br,
                            0.85 * s,
                            None,
                        );
                    });

                    // 12 radial sector dividers across the outer annulus
                    crate::dock_themes::with_cached_solid_brush(rt, rgba_u8(210, 224, 244, 0.16), |br| {
                        for k in 0..12 {
                            let ang = (k as f32 * 30.0 - 90.0).to_radians();
                            rt.DrawLine(
                                Vector2 { X: cx + mid_r * ang.cos(), Y: cy + mid_r * ang.sin() },
                                Vector2 { X: cx + outer_r * ang.cos(), Y: cy + outer_r * ang.sin() },
                                br,
                                0.75 * s,
                                None,
                            );
                        }
                    });

                    // 7 orbital day/temp nodes placed cleanly around the outer ring
                    let ring_r = (mid_r + outer_r) * 0.50;
                    for (i, &(_dlbl, hi, _lo, ic)) in snap.forecast_days.iter().enumerate() {
                        let ang = (-90.0 + (i as f32) * (360.0 / 7.0)).to_radians();
                        let nx = cx + ring_r * ang.cos();
                        let ny = cy + ring_r * ang.sin();
                        if i == 0 {
                            crate::dock_themes::with_cached_solid_brush(rt, rgba_u8(ar, ag, ab, 0.26), |br| {
                                rt.FillEllipse(
                                    &D2D1_ELLIPSE { point: Vector2 { X: nx, Y: ny }, radiusX: 8.6 * s, radiusY: 8.6 * s },
                                    br,
                                );
                            });
                        }
                        draw_sky_glyph(nx, ny - 3.0 * s, 2.7 * s, ic);
                        let rt_lbl = format!("{:.0}°", to_u(hi));
                        draw_txt_clipped(
                            rt,
                            &rt_lbl,
                            tf_micro,
                            D2D_RECT_F {
                                left: nx - 11.0 * s,
                                top: ny + 0.5 * s,
                                right: nx + 11.0 * s,
                                bottom: ny + 9.5 * s,
                            },
                            &br_txt,
                        );
                    }

                    // Radiant Solar Core (Day) or Luminous Crescent Moon (Night)
                    let glow_k = 0.65 + (wcfg.param2_pct as f32 / 100.0) * 0.65;
                    if is_day {
                        for (rm, al) in [(0.42f32, 0.16f32), (0.31, 0.30), (0.21, 0.88)] {
                            crate::dock_themes::with_cached_solid_brush(
                                rt,
                                rgba_u8(255, 188, 62, (al * glow_k).clamp(0.08, 0.96)),
                                |br| {
                                    rt.FillEllipse(
                                        &D2D1_ELLIPSE {
                                            point: Vector2 { X: cx, Y: cy },
                                            radiusX: outer_r * rm,
                                            radiusY: outer_r * rm,
                                        },
                                        br,
                                    );
                                },
                            );
                        }
                        crate::dock_themes::with_cached_solid_brush(rt, rgba_u8(255, 246, 206, 0.96), |br| {
                            rt.FillEllipse(
                                &D2D1_ELLIPSE {
                                    point: Vector2 { X: cx, Y: cy },
                                    radiusX: outer_r * 0.12,
                                    radiusY: outer_r * 0.12,
                                },
                                br,
                            );
                        });
                    } else {
                        crate::dock_themes::with_cached_solid_brush(
                            rt,
                            rgba_u8(144, 186, 255, (0.22 * glow_k).clamp(0.10, 0.38)),
                            |br| {
                                rt.FillEllipse(
                                    &D2D1_ELLIPSE {
                                        point: Vector2 { X: cx, Y: cy },
                                        radiusX: outer_r * 0.36,
                                        radiusY: outer_r * 0.36,
                                    },
                                    br,
                                );
                            },
                        );
                        crate::dock_themes::draw_cached_crescent(
                            rt,
                            d2d,
                            cx,
                            cy,
                            outer_r * 0.20,
                            rgba_u8(226, 238, 255, 0.95),
                        );
                    }

                    // Live Analog Clock Hands over Celestial Core
                    let hr_ang = (((now.hour % 12) as f32 + now.minute as f32 / 60.0) * 30.0 - 90.0).to_radians();
                    let min_ang = ((now.minute as f32 + now.second as f32 / 60.0) * 6.0 - 90.0).to_radians();
                    let sec_ang = (now.second as f32 * 6.0 - 90.0).to_radians();
                    crate::dock_themes::with_cached_solid_brush(rt, rgba_u8(248, 250, 252, 0.96), |br| {
                        rt.DrawLine(
                            Vector2 { X: cx, Y: cy },
                            Vector2 { X: cx + mid_r * 0.56 * hr_ang.cos(), Y: cy + mid_r * 0.56 * hr_ang.sin() },
                            br,
                            2.1 * s,
                            None,
                        );
                        rt.DrawLine(
                            Vector2 { X: cx, Y: cy },
                            Vector2 { X: cx + mid_r * 0.82 * min_ang.cos(), Y: cy + mid_r * 0.82 * min_ang.sin() },
                            br,
                            1.35 * s,
                            None,
                        );
                    });
                    crate::dock_themes::with_cached_solid_brush(rt, rgba_u8(255, 114, 76, 0.92), |br| {
                        rt.DrawLine(
                            Vector2 { X: cx - mid_r * 0.18 * sec_ang.cos(), Y: cy - mid_r * 0.18 * sec_ang.sin() },
                            Vector2 { X: cx + mid_r * 0.88 * sec_ang.cos(), Y: cy + mid_r * 0.88 * sec_ang.sin() },
                            br,
                            0.85 * s,
                            None,
                        );
                    });
                    crate::dock_themes::with_cached_solid_brush(rt, rgba_u8(ar, ag, ab, 0.98), |br| {
                        rt.FillEllipse(
                            &D2D1_ELLIPSE { point: Vector2 { X: cx, Y: cy }, radiusX: 2.4 * s, radiusY: 2.4 * s },
                            br,
                        );
                    });

                    // Inner crown temperature & day label
                    let crown_t = format!("{:.0}°", to_u(snap.temp_c));
                    draw_txt_clipped(
                        rt,
                        &crown_t,
                        tf_micro,
                        D2D_RECT_F {
                            left: cx - 16.0 * s,
                            top: cy - mid_r * 0.88,
                            right: cx + 16.0 * s,
                            bottom: cy - mid_r * 0.42,
                        },
                        &br_txt,
                    );
                    let dow_short = now.weekday_name.get(..3).unwrap_or(now.weekday_name);
                    let day_tag = format!("{} {}", dow_short, now.day);
                    draw_txt_clipped(
                        rt,
                        &day_tag,
                        tf_micro,
                        D2D_RECT_F {
                            left: cx - 22.0 * s,
                            top: cy + mid_r * 0.44,
                            right: cx + 22.0 * s,
                            bottom: cy + mid_r * 0.88,
                        },
                        &br_sub,
                    );
                });

                // Right Pane: Big Temperature + Glowing Solar Horizon Arc + Micro Telemetry
                let rx0 = cx + outer_r + 12.0 * s;
                let temp_str = format!("{:.0}{}", to_u(snap.temp_c), unit_ch);
                draw_txt_clipped(
                    rt,
                    &temp_str,
                    &tf_big,
                    D2D_RECT_F { left: rx0, top: y_cur - 2.0 * s, right: rx0 + 88.0 * s, bottom: y_cur + 26.0 * s },
                    &br_txt,
                );
                if wcfg.show_footer && wcfg.feats[3] {
                    let hl = format!("↑{:.0}° ↓{:.0}°", to_u(snap.high_c), to_u(snap.low_c));
                    draw_txt_clipped(
                        rt,
                        &hl,
                        &tf_right,
                        D2D_RECT_F { left: rx0 + 78.0 * s, top: y_cur + 3.0 * s, right: x1, bottom: y_cur + 21.0 * s },
                        &br_acc,
                    );
                }
                let cond_line = if wcfg.feats[4] {
                    format!("{} • Feels {:.0}°", snap.condition, to_u(snap.temp_c - 1.0))
                } else {
                    snap.condition.to_string()
                };
                draw_txt_clipped(
                    rt,
                    &cond_line,
                    &tf_sub,
                    D2D_RECT_F { left: rx0, top: y_cur + 24.0 * s, right: x1, bottom: y_cur + 39.0 * s },
                    &br_sub,
                );

                // Glowing Solar Horizon Arc Card
                let arc_top = y_cur + 42.0 * s;
                let arc_h = 54.0 * s;
                draw_sub_box(rx0, arc_top, x1, arc_top + arc_h, 9.0 * s, false, false, true);
                let ax0 = rx0 + 14.0 * s;
                let ax1 = x1 - 14.0 * s;
                let hy = arc_top + 35.0 * s;
                let amp_y = 24.0 * s;
                with_slot4!(rx0, arc_top, x1, arc_top + arc_h, {
                    crate::dock_themes::with_cached_solid_brush(rt, rgba_u8(216, 228, 246, 0.24), |br| {
                        rt.DrawLine(
                            Vector2 { X: ax0 - 4.0 * s, Y: hy },
                            Vector2 { X: ax1 + 4.0 * s, Y: hy },
                            br,
                            0.8 * s,
                            None,
                        );
                    });
                    const ARC_SEGS: usize = 20;
                    for seg in 0..ARC_SEGS {
                        let t0 = seg as f32 / ARC_SEGS as f32;
                        let t1 = (seg + 1) as f32 / ARC_SEGS as f32;
                        let p0 = Vector2 {
                            X: ax0 + (ax1 - ax0) * t0,
                            Y: hy - (t0 * std::f32::consts::PI).sin() * amp_y,
                        };
                        let p1 = Vector2 {
                            X: ax0 + (ax1 - ax0) * t1,
                            Y: hy - (t1 * std::f32::consts::PI).sin() * amp_y,
                        };
                        let elapsed = is_day && t1 <= solar_t + 0.03;
                        if elapsed {
                            crate::dock_themes::with_cached_solid_brush(rt, rgba_u8(255, 198, 72, 0.28), |br| {
                                rt.DrawLine(p0, p1, br, 3.6 * s, None);
                            });
                            crate::dock_themes::with_cached_solid_brush(rt, rgba_u8(255, 212, 92, 0.96), |br| {
                                rt.DrawLine(p0, p1, br, 1.45 * s, None);
                            });
                        } else {
                            crate::dock_themes::with_cached_solid_brush(rt, rgba_u8(212, 226, 246, 0.26), |br| {
                                rt.DrawLine(p0, p1, br, 1.1 * s, None);
                            });
                        }
                    }
                    // Live Solar/Lunar Orb along the horizon arc
                    let orb_t = if is_day { solar_t } else { 0.5 };
                    let orb_x = ax0 + (ax1 - ax0) * orb_t;
                    let orb_y = hy - (orb_t * std::f32::consts::PI).sin() * amp_y;
                    let (orb_glow, orb_core) = if is_day {
                        (rgba_u8(255, 196, 68, 0.36), rgba_u8(255, 232, 138, 0.98))
                    } else {
                        (rgba_u8(146, 188, 255, 0.30), rgba_u8(224, 238, 255, 0.96))
                    };
                    crate::dock_themes::with_cached_solid_brush(rt, orb_glow, |br| {
                        rt.FillEllipse(
                            &D2D1_ELLIPSE { point: Vector2 { X: orb_x, Y: orb_y }, radiusX: 6.2 * s, radiusY: 6.2 * s },
                            br,
                        );
                    });
                    crate::dock_themes::with_cached_solid_brush(rt, orb_core, |br| {
                        rt.FillEllipse(
                            &D2D1_ELLIPSE { point: Vector2 { X: orb_x, Y: orb_y }, radiusX: 3.1 * s, radiusY: 3.1 * s },
                            br,
                        );
                    });
                });
                let sr_txt = format!("{:02}:{:02}", sr / 60, sr % 60);
                let ss_txt = format!("{:02}:{:02}", ss / 60, ss % 60);
                draw_txt_clipped(
                    rt,
                    &sr_txt,
                    tf_micro,
                    D2D_RECT_F { left: rx0 + 4.0 * s, top: hy + 2.0 * s, right: rx0 + 48.0 * s, bottom: arc_top + arc_h - 2.0 * s },
                    &br_sub,
                );
                let phase_lbl = if is_day { "SOLAR ARC" } else { "LUNAR PHASE" };
                draw_txt_clipped(
                    rt,
                    phase_lbl,
                    tf_micro,
                    D2D_RECT_F { left: rx0 + 48.0 * s, top: hy + 2.0 * s, right: x1 - 48.0 * s, bottom: arc_top + arc_h - 2.0 * s },
                    &br_acc,
                );
                draw_txt_clipped(
                    rt,
                    &ss_txt,
                    tf_micro,
                    D2D_RECT_F { left: x1 - 48.0 * s, top: hy + 2.0 * s, right: x1 - 4.0 * s, bottom: arc_top + arc_h - 2.0 * s },
                    &br_sub,
                );

                // Micro-telemetry strip (Humidity / Wind / UV)
                if wcfg.show_footer && (wcfg.feats[2] || wcfg.feats[4]) {
                    let met_y = arc_top + arc_h + 5.0 * s;
                    let met_h = 19.0 * s;
                    let pill_w = ((x1 - rx0) - 8.0 * s) / 3.0;
                    let hum_s = format!("Hum {}%", snap.humidity);
                    let wnd_s = format!("{:.0} km/h", snap.wind_kmh);
                    let uv_s = format!("UV {}", snap.uv_index);
                    for (pi, lbl) in [hum_s.as_str(), wnd_s.as_str(), uv_s.as_str()].iter().enumerate() {
                        let px0 = rx0 + pi as f32 * (pill_w + 4.0 * s);
                        let px1 = px0 + pill_w;
                        draw_sub_box(px0, met_y, px1, met_y + met_h, 6.0 * s, false, false, false);
                        draw_txt_clipped(
                            rt,
                            lbl,
                            tf_micro,
                            D2D_RECT_F { left: px0, top: met_y, right: px1, bottom: met_y + met_h },
                            if pi == 2 { &br_acc } else { &br_txt },
                        );
                    }
                }

                // Bottom Full-Width Compact 7-Day Forecast Strip
                if show_strip {
                    let strip_y = y_cur + dial_h + 4.0 * s;
                    if wcfg.show_dividers {
                        draw_ext_divider(strip_y - 2.0 * s);
                    }
                    let strip_top = strip_y + 3.0 * s;
                    let strip_bot = (y + h - 8.0 * s).max(strip_top + 28.0 * s);
                    let col_w = (x1 - x0) / 7.0;
                    for (i, &(day_lbl, hi, lo, ic)) in snap.forecast_days.iter().enumerate() {
                        let cx0 = x0 + (i as f32) * col_w;
                        let cx1 = cx0 + col_w - 3.0 * s;
                        draw_sub_box(cx0, strip_top, cx1, strip_bot, 6.0 * s, false, i == 0, i == 0);
                        let lbl = if i == 0 { "Today" } else { day_lbl };
                        draw_txt_clipped(
                            rt,
                            lbl,
                            tf_micro,
                            D2D_RECT_F { left: cx0, top: strip_top + 2.0 * s, right: cx1, bottom: strip_top + 13.0 * s },
                            if i == 0 { &br_acc } else { &br_sub },
                        );
                        draw_sky_glyph((cx0 + cx1) * 0.5, strip_top + 19.5 * s, 3.0 * s, ic);
                        let t_lbl = format!("{:.0}°/{:.0}°", to_u(hi), to_u(lo));
                        draw_txt_clipped(
                            rt,
                            &t_lbl,
                            tf_micro,
                            D2D_RECT_F { left: cx0, top: strip_bot - 14.0 * s, right: cx1, bottom: strip_bot - 1.5 * s },
                            &br_txt,
                        );
                    }
                }
            } else {
                if wcfg.show_header {
                    let hdr = format!("WEATHER • {}", snap.city.to_uppercase());
                    draw_txt_clipped(
                        rt,
                        &hdr,
                        &tf_hdr,
                        D2D_RECT_F { left: x0, top: y_cur, right: x1 - 110.0 * s, bottom: y_cur + 16.0 * s },
                        &br_acc,
                    );
                    draw_pill(x1 - 106.0 * s, y_cur, x1 - 44.0 * s, y_cur + 17.0 * s, "City ↻", false, is_btn_hov(1));
                    draw_pill(x1 - 40.0 * s, y_cur, x1, y_cur + 17.0 * s, unit_ch, use_f, is_btn_hov(0));
                    y_cur += 20.0 * s;
                }

                // Sun / Cloud Icon + Big Temperature
                let mut main_x = x0;
                if wcfg.show_icon {
                    let cx = x0 + 20.0 * s;
                    let cy = y_cur + 20.0 * s;
                    let glow_r = (13.0 + (wcfg.param2_pct as f32 / 100.0) * 9.0) * s;
                    with_box_slot!(cx - glow_r, cy - glow_r, cx + glow_r, cy + glow_r, {
                        rt.FillEllipse(
                            &D2D1_ELLIPSE { point: Vector2 { X: cx, Y: cy }, radiusX: glow_r, radiusY: glow_r },
                            &br_acc_dim,
                        );
                        rt.FillEllipse(
                            &D2D1_ELLIPSE { point: Vector2 { X: cx, Y: cy }, radiusX: 10.5 * s, radiusY: 10.5 * s },
                            br_box_fill,
                        );
                    });
                    main_x += 46.0 * s;
                }

                let temp_str = format!("{:.0}{}", to_u(snap.temp_c), unit_ch);
                draw_txt_clipped(
                    rt,
                    &temp_str,
                    &tf_big,
                    D2D_RECT_F { left: main_x, top: y_cur - 2.0 * s, right: main_x + 92.0 * s, bottom: y_cur + 28.0 * s },
                    &br_txt,
                );
                let cond_line = if wcfg.feats[4] {
                    format!("{} • Feels {:.0}° • UV {}", snap.condition, to_u(snap.temp_c - 1.0), snap.uv_index)
                } else {
                    snap.condition.to_string()
                };
                draw_txt_clipped(
                    rt,
                    &cond_line,
                    &tf_sub,
                    D2D_RECT_F { left: main_x, top: y_cur + 24.0 * s, right: x1, bottom: y_cur + 40.0 * s },
                    &br_sub,
                );

                if wcfg.show_footer {
                    if wcfg.feats[2] {
                        let meta = format!("Hum {}%  •  {:.0} km/h", snap.humidity, snap.wind_kmh);
                        draw_txt_clipped(
                            rt,
                            &meta,
                            &tf_right,
                            D2D_RECT_F { left: main_x + 70.0 * s, top: y_cur + 2.0 * s, right: x1, bottom: y_cur + 20.0 * s },
                            &br_sub,
                        );
                    }
                    if wcfg.feats[3] {
                        let hl = format!("H: {:.0}°  L: {:.0}°", to_u(snap.high_c), to_u(snap.low_c));
                        draw_txt_clipped(
                            rt,
                            &hl,
                            &tf_right,
                            D2D_RECT_F { left: main_x + 70.0 * s, top: y_cur + 20.0 * s, right: x1, bottom: y_cur + 38.0 * s },
                            &br_acc,
                        );
                    }
                }
                y_cur += 44.0 * s;

                if wcfg.show_dividers {
                    draw_ext_divider(y_cur);
                    y_cur += 6.0 * s;
                }

                // Forecast Row (3..7 days)
                if wcfg.feats[1] && !wcfg.compact_mode {
                    let cols = ((wcfg.param1_pct as usize * 7) / 100).clamp(3, 7);
                    let col_w = (x1 - x0) / cols as f32;
                    for (i, &(day_lbl, hi, lo, _cond)) in snap.forecast_days.iter().take(cols).enumerate() {
                        let cx0 = x0 + (i as f32) * col_w;
                        let cx1 = cx0 + col_w - 4.0 * s;
                        draw_sub_box(cx0, y_cur, cx1, y + h - 8.0 * s, 6.0 * s, false, false, false);
                        draw_txt_clipped(
                            rt,
                            day_lbl,
                            &tf_center,
                            D2D_RECT_F { left: cx0, top: y_cur + 2.0 * s, right: cx1, bottom: y_cur + 18.0 * s },
                            &br_acc,
                        );
                        let t_lbl = format!("{:.0}°/{:.0}°", to_u(hi), to_u(lo));
                        draw_txt_clipped(
                            rt,
                            &t_lbl,
                            &tf_center,
                            D2D_RECT_F { left: cx0, top: y_cur + 20.0 * s, right: cx1, bottom: y + h - 10.0 * s },
                            &br_txt,
                        );
                    }
                }
            }
        }

        // =====================================================================
        // 2. DESKTOP SPOTLIGHT SEARCH BAR
        // =====================================================================
        ExtWidgetId::SearchBar => {
            let eng = search_engine_name(wcfg.mode_idx);
            let cy = y + h * 0.5;
            let bar_half = (10.0 + (wcfg.param1_pct as f32 / 100.0) * 6.0) * s;
            let mut text_x0 = x0;

            if state.search_focused || wcfg.param2_pct > 25 {
                let focus_a = if state.search_focused {
                    (0.35 + (wcfg.param2_pct as f32 / 100.0) * 0.55).clamp(0.2, 0.95)
                } else {
                    (wcfg.param2_pct as f32 / 100.0) * 0.25
                };
                with_slot4!(x0 - 4.0 * s, cy - bar_half - 2.0 * s, x1 + 4.0 * s, cy + bar_half + 2.0 * s, {
                    if let Ok(fr) = rt.CreateSolidColorBrush(&rgba_u8(ar, ag, ab, focus_a), None) {
                        rt.DrawRoundedRectangle(
                            &D2D1_ROUNDED_RECT {
                                rect: D2D_RECT_F { left: x0 - 4.0 * s, top: cy - bar_half - 2.0 * s, right: x1 + 4.0 * s, bottom: cy + bar_half + 2.0 * s },
                                radiusX: 9.0 * s,
                                radiusY: 9.0 * s,
                            },
                            &fr,
                            1.2 * s,
                            None,
                        );
                    }
                });
            }

            if wcfg.feats[0] && wcfg.show_header {
                let eng_w = if wcfg.feats[2] { 98.0 * s } else { 86.0 * s };
                let eng_lbl = if wcfg.feats[2] { format!("{} ▾", eng) } else { eng.to_string() };
                draw_pill(x0, cy - bar_half, x0 + eng_w, cy + bar_half, &eng_lbl, state.search_focused, is_btn_hov(1));
                text_x0 += eng_w + 10.0 * s;
            } else if wcfg.show_icon && wcfg.show_header {
                draw_txt_clipped(
                    rt,
                    "⌕",
                    &tf_hdr,
                    D2D_RECT_F { left: x0, top: cy - 12.0 * s, right: x0 + 24.0 * s, bottom: cy + 12.0 * s },
                    &br_acc,
                );
                text_x0 += 22.0 * s;
            }

            let caret = if state.search_focused && wcfg.feats[5] && (state.tick_counter % 2 == 0) {
                "|"
            } else {
                ""
            };
            let display_q = if state.search_query.is_empty() {
                if state.search_focused {
                    format!("Type to search {}...{}", eng, caret)
                } else {
                    format!("Search {} or click to type...", eng)
                }
            } else {
                format!("{}{}", state.search_query, caret)
            };

            let right_reserve = if !wcfg.feats[3] {
                if wcfg.feats[1] && state.search_query.is_empty() && wcfg.show_footer { 50.0 * s } else { 4.0 * s }
            } else if wcfg.feats[1] && state.search_query.is_empty() && wcfg.show_footer {
                86.0 * s
            } else {
                36.0 * s
            };
            draw_txt_clipped(
                rt,
                &display_q,
                &tf_body,
                D2D_RECT_F { left: text_x0, top: cy - bar_half, right: x1 - right_reserve, bottom: cy + bar_half },
                if state.search_query.is_empty() && !state.search_focused { &br_sub } else { &br_txt },
            );

            if wcfg.feats[1] && state.search_query.is_empty() && wcfg.show_footer {
                let hk_r = if wcfg.feats[3] { x1 - 38.0 * s } else { x1 };
                draw_pill(hk_r - 44.0 * s, cy - 10.0 * s, hk_r, cy + 10.0 * s, "Ctrl+K", false, false);
            }

            if wcfg.feats[3] {
                draw_pill(
                    x1 - 32.0 * s,
                    cy - bar_half,
                    x1,
                    cy + bar_half,
                    "→",
                    state.search_focused || !state.search_query.is_empty(),
                    is_btn_hov(2),
                );
            }
        }

        // =====================================================================
        // 3. DAILY GREETING & INSPIRATION QUOTE PILL
        // =====================================================================
        ExtWidgetId::Greeting => {
            let now = chrono_local();
            let greet = match now.hour {
                5..=11 => "Good morning",
                12..=16 => "Good afternoon",
                17..=21 => "Good evening",
                _ => "Late night focus",
            };
            let quotes = match wcfg.mode_idx {
                1 => &QUOTES_TECH,
                2 => &QUOTES_ZEN,
                3 => &QUOTES_LIT,
                _ => &QUOTES_STOIC,
            };
            let (quote_txt, author) = quotes[state.quote_idx % quotes.len()];

            if wcfg.feats[0] && wcfg.is_slot_visible(0) {
                let mut gx0 = x0;
                if wcfg.feats[1] && wcfg.is_slot_visible(3) {
                    let orb_cx = x0 + 7.0 * s;
                    let orb_cy = y_cur + 10.0 * s;
                    with_box_slot!(orb_cx - 7.0 * s, orb_cy - 7.0 * s, orb_cx + 7.0 * s, orb_cy + 7.0 * s, {
                        rt.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: orb_cx, Y: orb_cy }, radiusX: 7.0 * s, radiusY: 7.0 * s }, &br_acc_dim);
                        rt.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: orb_cx, Y: orb_cy }, radiusX: 4.5 * s, radiusY: 4.5 * s }, br_box_fill);
                    });
                    gx0 += 20.0 * s;
                }
                let g_line = format!("{}, {}", greet, state.user_name);
                let g_mul = (0.78 + (wcfg.param1_pct as f32 / 100.0) * 0.55).clamp(0.75, 1.35);
                let g_tf_custom = make_resolved_tf_named(
                    dw,
                    custom_fonts,
                    elem_fnt(0),
                    fam_candidates,
                    10.2 * g_mul * elem_fs(0),
                    DWRITE_FONT_WEIGHT_SEMI_BOLD,
                    align_dwrite,
                    DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
                );
                let rc_g = D2D_RECT_F { left: gx0, top: y_cur, right: x1 - 114.0 * s, bottom: y_cur + 22.0 * s };
                record_slot_rc(0, rc_g);
                let mut none_boxes: Option<&mut [ClockElemBox]> = None;
                let g0 = begin_elem_xf_slot(rt, 0, wcfg.elem_transforms[0], (rc_g.left, rc_g.top, rc_g.right, rc_g.bottom), s, &mut none_boxes);
                let eff_br0 = br_slot[0].as_ref().unwrap_or(&br_acc);
                draw_shadowed_text(
                    rt,
                    &g_line,
                    g_tf_custom.as_ref().unwrap_or(&tf_title),
                    rc_g,
                    eff_br0,
                    br_txt_sh.as_ref(),
                    if wcfg.show_shadow { 0.8 * s } else { 0.0 },
                    true,
                );
                end_elem_xf_slot(rt, g0);
            }
            if wcfg.feats[4] {
                let spec = widget_feature_spec(id);
                let topic = spec.modes[(wcfg.mode_idx as usize).min(3)];
                draw_pill(x1 - 110.0 * s, y_cur, x1 - 32.0 * s, y_cur + 18.0 * s, topic, false, is_btn_hov(1));
                draw_pill(x1 - 28.0 * s, y_cur, x1, y_cur + 18.0 * s, "↻", false, is_btn_hov(0));
            }
            y_cur += 22.0 * s;

            if wcfg.show_dividers {
                draw_ext_divider(y_cur);
                y_cur += 5.0 * s;
            }

            if wcfg.feats[2] && (wcfg.is_slot_visible(1) || wcfg.is_slot_visible(2)) {
                let show_q = wcfg.is_slot_visible(1);
                let show_a = wcfg.feats[3] && wcfg.is_slot_visible(2);
                let q_full = match (show_q, show_a) {
                    (true, true) => format!("“{}” — {}", quote_txt, author),
                    (true, false) => format!("“{}”", quote_txt),
                    (false, true) => format!("— {}", author),
                    (false, false) => String::new(),
                };
                if !q_full.is_empty() {
                    let q_slot = if show_q { 1 } else { 2 };
                    let q_alpha = (0.45 + (wcfg.param2_pct as f32 / 100.0) * 0.55).clamp(0.45, 1.0);
                    let br_q = rt.CreateSolidColorBrush(&rgba_u8(tr, tg, tb, q_alpha), None).ok();
                    let tf_q_script = if wcfg.feats[5] {
                        make_resolved_tf_named(
                            dw,
                            custom_fonts,
                            elem_fnt(q_slot),
                            &[w!("Georgia"), w!("Segoe Script"), w!("Segoe UI")],
                            8.2 * elem_fs(q_slot),
                            DWRITE_FONT_WEIGHT_REGULAR,
                            align_dwrite,
                            DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
                        )
                    } else {
                        None
                    };
                    let rc_q = D2D_RECT_F { left: x0, top: y_cur, right: x1, bottom: y + h - 6.0 * s };
                    record_slot_rc(q_slot, rc_q);
                    let mut none_boxes: Option<&mut [ClockElemBox]> = None;
                    let gq = begin_elem_xf_slot(rt, q_slot, wcfg.elem_transforms[q_slot], (rc_q.left, rc_q.top, rc_q.right, rc_q.bottom), s, &mut none_boxes);
                    let eff_br_q = br_slot[q_slot].as_ref().or(br_q.as_ref()).unwrap_or(&br_txt);
                    draw_shadowed_text(
                        rt,
                        &q_full,
                        tf_q_script.as_ref().unwrap_or(if show_q { &tf_body } else { &tf_sub }),
                        rc_q,
                        eff_br_q,
                        br_txt_sh.as_ref(),
                        if wcfg.show_shadow { 0.8 * s } else { 0.0 },
                        true,
                    );
                    end_elem_xf_slot(rt, gq);
                }
            }
        }

        // =====================================================================
        // 4. NETWORK & LIVE BANDWIDTH SPEEDOMETER
        // =====================================================================
        ExtWidgetId::Network => {
            if wcfg.show_header {
                let hdr = if wcfg.feats[3] && wcfg.show_footer {
                    format!("NETWORK • {}", state.local_ip)
                } else {
                    "NETWORK BANDWIDTH".to_string()
                };
                draw_txt_clipped(
                    rt,
                    &hdr,
                    &tf_hdr,
                    D2D_RECT_F { left: x0, top: y_cur, right: x1 - 76.0 * s, bottom: y_cur + 16.0 * s },
                    &br_acc,
                );
                if wcfg.feats[2] && wcfg.show_footer {
                    let ping_str = format!("{} ms", state.ping_ms);
                    draw_pill(x1 - 68.0 * s, y_cur, x1, y_cur + 16.0 * s, &ping_str, false, is_btn_hov(0));
                }
                y_cur += 20.0 * s;
            }

            let dn_str = if wcfg.feats[1] {
                format!("↓ {}", format_speed(state.down_bps, wcfg.mode_idx))
            } else {
                format_speed(state.down_bps, wcfg.mode_idx)
            };
            let up_str = if wcfg.feats[1] {
                format!("↑ {}", format_speed(state.up_bps, wcfg.mode_idx))
            } else {
                format_speed(state.up_bps, wcfg.mode_idx)
            };
            draw_txt_clipped(
                rt,
                &dn_str,
                &tf_title,
                D2D_RECT_F { left: x0, top: y_cur, right: x0 + (x1 - x0) * 0.55, bottom: y_cur + 22.0 * s },
                &br_txt,
            );
            draw_txt_clipped(
                rt,
                &up_str,
                &tf_right,
                D2D_RECT_F { left: x0 + (x1 - x0) * 0.45, top: y_cur, right: x1, bottom: y_cur + 22.0 * s },
                &br_acc,
            );
            y_cur += 24.0 * s;

            if wcfg.show_dividers {
                draw_ext_divider(y_cur);
                y_cur += 4.0 * s;
            }

            // Live Sparkline (bar density controlled by param1_pct, ceiling by param2_pct, area fill by feats[5])
            if wcfg.feats[0] && !wcfg.compact_mode {
                let gh = (y + h - 10.0 * s - y_cur).max(16.0 * s);
                let bars = ((wcfg.param1_pct as usize * 32) / 100).clamp(12, state.net_history.len());
                let start_i = state.net_history.len().saturating_sub(bars);
                let step = (x1 - x0) / bars as f32;
                let bw = (step * if wcfg.feats[5] { 0.88 } else { 0.68 }).max(2.0 * s);
                let scale_boost = (0.55 + (wcfg.param2_pct as f32 / 100.0) * 1.1).clamp(0.55, 1.65);
                with_box_slot!(x0, y_cur, x1, y_cur + gh, {
                    for (i, &val_raw) in state.net_history[start_i..].iter().enumerate() {
                        let val = (val_raw * scale_boost).clamp(0.04, 1.0);
                        let bx = x0 + (i as f32) * step;
                        let bh = (val * gh).clamp(3.0 * s, gh);
                        let by0 = y_cur + gh - bh;
                        let br = D2D1_ROUNDED_RECT {
                            rect: D2D_RECT_F { left: bx, top: by0, right: bx + bw, bottom: y_cur + gh },
                            radiusX: 1.5 * s,
                            radiusY: 1.5 * s,
                        };
                        rt.FillRoundedRectangle(
                            &br,
                            if val > 0.65 && wcfg.feats[4] { br_box_fill } else { &br_acc_mid },
                        );
                    }
                });
            }
        }

        // =====================================================================
        // 5. GPU & GAMING TELEMETRY CARD
        // =====================================================================
        ExtWidgetId::Gpu => {
            if wcfg.show_header {
                let hdr = if wcfg.feats[4] {
                    format!("GPU • {}", state.gpu_name.to_uppercase())
                } else {
                    "GPU TELEMETRY".to_string()
                };
                draw_txt_clipped(
                    rt,
                    &hdr,
                    &tf_hdr,
                    D2D_RECT_F { left: x0, top: y_cur, right: x1 - 74.0 * s, bottom: y_cur + 16.0 * s },
                    &br_acc,
                );
                let mode_lbl = match wcfg.mode_idx {
                    1 => "VRAM",
                    2 => "TEMP",
                    3 => "CLOCK",
                    _ => "CORE",
                };
                draw_pill(x1 - 68.0 * s, y_cur, x1, y_cur + 16.0 * s, mode_lbl, false, is_btn_hov(0));
                y_cur += 20.0 * s;
            }

            let primary_str = match wcfg.mode_idx {
                1 => format!("{:.1} / {:.0} GB", state.vram_used_mb / 1024.0, state.vram_total_mb / 1024.0),
                2 => format!("{:.0}°C Core", state.gpu_temp_c),
                3 => format!("{} MHz", state.gpu_clock_mhz),
                _ => format!("{:.0}% Load", state.gpu_load_pct),
            };
            draw_txt_clipped(
                rt,
                &primary_str,
                &tf_title,
                D2D_RECT_F { left: x0, top: y_cur, right: x0 + 140.0 * s, bottom: y_cur + 22.0 * s },
                &br_txt,
            );

            if wcfg.show_footer {
                let mut parts: Vec<String> = Vec::new();
                if wcfg.feats[1] {
                    parts.push(format!("{:.0}°C", state.gpu_temp_c));
                }
                if wcfg.feats[0] {
                    parts.push(format!("{:.1}G VRAM", state.vram_used_mb / 1024.0));
                }
                if wcfg.feats[2] {
                    parts.push(format!("{}MHz", state.gpu_clock_mhz));
                }
                let sub_stats = parts.join(" • ");
                draw_txt_clipped(
                    rt,
                    &sub_stats,
                    &tf_right,
                    D2D_RECT_F { left: x0 + 108.0 * s, top: y_cur, right: x1, bottom: y_cur + 22.0 * s },
                    &br_sub,
                );
            }
            y_cur += 24.0 * s;

            if wcfg.show_dividers {
                draw_ext_divider(y_cur);
                y_cur += 4.0 * s;
            }

            // Dual Progress Bars: Core Load & VRAM (height scaled by param1_pct, thermal alert by param2_pct + feats[5])
            let bar_w = x1 - x0;
            let bar_h = (5.0 + (wcfg.param1_pct as f32 / 100.0) * 6.0) * s;
            let warn_temp = 55.0 + (wcfg.param2_pct as f32 / 100.0) * 35.0;
            let thermal_hot = wcfg.feats[5] && state.gpu_temp_c >= warn_temp;
            let bars_h = if wcfg.feats[0] && !wcfg.compact_mode { bar_h * 2.0 + 6.0 * s } else { bar_h };
            with_box_slot!(x0, y_cur, x1, y_cur + bars_h, {
                let tr_rr = D2D1_ROUNDED_RECT {
                    rect: D2D_RECT_F { left: x0, top: y_cur, right: x1, bottom: y_cur + bar_h },
                    radiusX: bar_h * 0.5,
                    radiusY: bar_h * 0.5,
                };
                rt.FillRoundedRectangle(&tr_rr, &br_acc_dim);
                let fill_w = (bar_w * (state.gpu_load_pct / 100.0).clamp(0.04, 1.0)).max(6.0 * s);
                let fl_rr = D2D1_ROUNDED_RECT {
                    rect: D2D_RECT_F { left: x0, top: y_cur, right: x0 + fill_w, bottom: y_cur + bar_h },
                    radiusX: bar_h * 0.5,
                    radiusY: bar_h * 0.5,
                };
                rt.FillRoundedRectangle(&fl_rr, if thermal_hot && box_ov.is_none() { &br_coral } else { br_box_fill });
                y_cur += bar_h + 5.0 * s;

                if wcfg.feats[0] && !wcfg.compact_mode {
                    let vram_ratio = (state.vram_used_mb / state.vram_total_mb.max(512.0)).clamp(0.04, 1.0);
                    let vr_tr = D2D1_ROUNDED_RECT {
                        rect: D2D_RECT_F { left: x0, top: y_cur, right: x1, bottom: y_cur + bar_h * 0.85 },
                        radiusX: bar_h * 0.42,
                        radiusY: bar_h * 0.42,
                    };
                    rt.FillRoundedRectangle(&vr_tr, &br_acc_dim);
                    let vr_fl = D2D1_ROUNDED_RECT {
                        rect: D2D_RECT_F { left: x0, top: y_cur, right: x0 + bar_w * vram_ratio, bottom: y_cur + bar_h * 0.85 },
                        radiusX: bar_h * 0.42,
                        radiusY: bar_h * 0.42,
                    };
                    rt.FillRoundedRectangle(&vr_fl, if box_ov.is_some() { &br_acc_mid } else { &br_sage });
                }
            });
            if wcfg.feats[3] && !wcfg.compact_mode && y_cur + bar_h + 6.0 * s < y + h - 4.0 * s {
                let ty_sp = y + h - 10.0 * s;
                with_slot4!(x0, ty_sp, x1, ty_sp + 4.0 * s, {
                    for ti in 0..16 {
                        let tx = x0 + (ti as f32 / 16.0) * (x1 - x0);
                        let active = (ti as f32 / 16.0) <= (state.gpu_load_pct / 100.0);
                        rt.FillRectangle(
                            &D2D_RECT_F { left: tx, top: ty_sp, right: tx + (bar_w / 22.0), bottom: ty_sp + 3.0 * s },
                            if active { br_div_fill } else { &br_acc_dim },
                        );
                    }
                });
            }
        }

        // =====================================================================
        // 6. QUICK AUDIO OUTPUT & DEVICE SWITCHER
        // =====================================================================
        ExtWidgetId::AudioSwitcher => {
            let lay = audio_switcher_layout(wcfg, cc_snap.audio_outputs.len());
            let map_rc = |r: (f32, f32, f32, f32)| -> (f32, f32, f32, f32) {
                (x + r.0 * s, y + r.1 * s, x + r.2 * s, y + r.3 * s)
            };

            if let Some(hr) = lay.hdr_rect {
                let (l, t, ri, b) = map_rc(hr);
                let hdr_lbl = if wcfg.feats[4] {
                    "AUDIO OUTPUT • 24-BIT"
                } else {
                    "AUDIO OUTPUT & MIXER"
                };
                draw_txt_clipped(
                    rt,
                    hdr_lbl,
                    &tf_hdr,
                    D2D_RECT_F { left: l, top: t, right: ri, bottom: b },
                    &br_acc,
                );
                if let Some(mr) = lay.mixer_btn {
                    let (ml, mt, mri, mb) = map_rc(mr);
                    draw_pill(ml, mt, mri, mb, "Mixer", false, is_btn_hov(5));
                }
            }

            if let (Some(r0), Some(r1)) = (lay.dev_btn_0, lay.dev_btn_1) {
                let dev0 = ExtRuntimeState::audio_slot_device(cc_snap, state.audio_output_page, 0);
                let dev1 = ExtRuntimeState::audio_slot_device(cc_snap, state.audio_output_page, 1);

                let (lbl0, act0) = if let Some(d) = dev0 {
                    let text = if d.is_default {
                        format!("● {}", d.name)
                    } else {
                        d.name.clone()
                    };
                    (text, d.is_default)
                } else {
                    let is_act = state.active_audio_out == 0;
                    (
                        if is_act { "● Speakers" } else { "Speakers" }.to_string(),
                        is_act,
                    )
                };

                let (lbl1, act1) = if let Some(d) = dev1 {
                    let text = if d.is_default {
                        format!("● {}", d.name)
                    } else {
                        d.name.clone()
                    };
                    (text, d.is_default)
                } else {
                    let bt_base = if !cc_snap.bt_sub.is_empty()
                        && !cc_snap.bt_sub.eq_ignore_ascii_case("Bluetooth")
                        && !cc_snap.bt_sub.eq_ignore_ascii_case("Off")
                        && !cc_snap.bt_sub.eq_ignore_ascii_case("On")
                    {
                        cc_snap.bt_sub.as_str()
                    } else {
                        "Headphones / BT"
                    };
                    let is_act = cc_snap.audio_outputs.is_empty() && state.active_audio_out == 1;
                    let text = if is_act {
                        format!("● {}", bt_base)
                    } else {
                        bt_base.to_string()
                    };
                    (text, is_act)
                };

                let (l0, t0, ri0, b0) = map_rc(r0);
                let (l1, t1, ri1, b1) = map_rc(r1);
                draw_pill(l0, t0, ri0, b0, &lbl0, act0, is_btn_hov(3));
                draw_pill(l1, t1, ri1, b1, &lbl1, act1, is_btn_hov(4));

                if let Some(rc) = lay.dev_cycle_btn {
                    let (cl, ct, cri, cb) = map_rc(rc);
                    draw_pill(cl, ct, cri, cb, "▸", false, is_btn_hov(6));
                }
            }

            if let Some(dy) = lay.divider_y {
                draw_ext_divider(y + dy * s);
            }

            if let Some(vr) = lay.vol_label_rect {
                let (vl, vt, vri, vb) = map_rc(vr);
                let vol_lbl = if cc_snap.vol_muted {
                    format!("Muted ({}%)", cc_snap.vol_pct)
                } else {
                    format!("Master: {}%", cc_snap.vol_pct)
                };
                draw_txt_clipped(
                    rt,
                    &vol_lbl,
                    &tf_body,
                    D2D_RECT_F { left: vl, top: vt, right: vri, bottom: vb },
                    &br_txt,
                );
                if let (Some(rm), Some(ru), Some(rp)) =
                    (lay.vol_minus_btn, lay.vol_mute_btn, lay.vol_plus_btn)
                {
                    let (l, t, ri, b) = map_rc(rm);
                    draw_pill(l, t, ri, b, "−", false, is_btn_hov(0));
                    let (l, t, ri, b) = map_rc(ru);
                    draw_pill(
                        l,
                        t,
                        ri,
                        b,
                        if cc_snap.vol_muted { "Unmute" } else { "Mute" },
                        cc_snap.vol_muted,
                        is_btn_hov(1),
                    );
                    let (l, t, ri, b) = map_rc(rp);
                    draw_pill(l, t, ri, b, "+", false, is_btn_hov(2));
                }
            }

            if let Some(sr) = lay.slider_track {
                let (sl, st, sri, sb) = map_rc(sr);
                let frac = if cc_snap.vol_muted {
                    0.0
                } else {
                    (cc_snap.vol_pct as f32 / 100.0).clamp(0.0, 1.0)
                };
                let tr_h = (sb - st).max(3.5 * s);
                let rad_t = tr_h * 0.5;
                let track_hov = is_btn_hov(7);
                with_slot4!(sl, st, sri, sb, {
                    rt.FillRoundedRectangle(
                        &D2D1_ROUNDED_RECT {
                            rect: D2D_RECT_F { left: sl, top: st, right: sri, bottom: sb },
                            radiusX: rad_t,
                            radiusY: rad_t,
                        },
                        &br_acc_dim,
                    );
                    if frac > 0.005 {
                        let fill_r = (sl + (sri - sl) * frac).max(sl + tr_h);
                        rt.FillRoundedRectangle(
                            &D2D1_ROUNDED_RECT {
                                rect: D2D_RECT_F {
                                    left: sl,
                                    top: st,
                                    right: fill_r.min(sri),
                                    bottom: sb,
                                },
                                radiusX: rad_t,
                                radiusY: rad_t,
                            },
                            br_div_fill,
                        );
                    }
                    // Interactive slider thumb knob
                    let knob_cx = sl + (sri - sl) * frac;
                    let knob_cy = (st + sb) * 0.5;
                    let knob_r = if track_hov { 4.8 * s } else { 3.6 * s };
                    rt.FillEllipse(
                        &D2D1_ELLIPSE {
                            point: Vector2 { X: knob_cx, Y: knob_cy },
                            radiusX: knob_r,
                            radiusY: knob_r,
                        },
                        if track_hov { &br_txt } else { br_div_fill },
                    );
                });
            }
        }

        // =====================================================================
        // 7. FROSTED STICKY NOTES & TO-DO CHECKLIST
        // =====================================================================
        ExtWidgetId::Notes => {
            let done_cnt = cfg.notes_items.iter().filter(|(d, _)| *d).count();
            let total_cnt = cfg.notes_items.len();
            if wcfg.show_header {
                let title_base = if cfg.notes_title.trim().is_empty() {
                    "NOTES & TO-DO"
                } else {
                    cfg.notes_title.trim()
                };
                let hdr = if wcfg.feats[2] && wcfg.show_footer {
                    format!("{} ({}/{})", title_base, done_cnt, total_cnt)
                } else {
                    title_base.to_string()
                };
                draw_txt_clipped(
                    rt,
                    &hdr,
                    &tf_hdr,
                    D2D_RECT_F { left: x0, top: y_cur, right: x1 - 96.0 * s, bottom: y_cur + 16.0 * s },
                    &br_acc,
                );
                if wcfg.feats[3] {
                    draw_pill(
                        x1 - 92.0 * s,
                        y_cur,
                        x1 - 46.0 * s,
                        y_cur + 17.0 * s,
                        if state.note_focused { "Save" } else { "+ Add" },
                        state.note_focused,
                        is_btn_hov(4),
                    );
                    draw_pill(x1 - 42.0 * s, y_cur, x1, y_cur + 17.0 * s, "Clear", false, is_btn_hov(5));
                }
                y_cur += 20.0 * s;
                if wcfg.show_dividers {
                    draw_ext_divider(y_cur);
                    y_cur += 4.0 * s;
                }
            }

            if state.note_focused {
                let draft_txt = if state.note_draft.is_empty() {
                    if let Some(erow) = state.editing_note_row {
                        format!("Edit task #{} & press Enter...|", erow + 1)
                    } else {
                        "Type new task & press Enter...|".to_string()
                    }
                } else {
                    format!("{}|", state.note_draft)
                };
                draw_sub_box(x0, y_cur, x1, y_cur + 22.0 * s, 5.0 * s, false, true, true);
                draw_txt_clipped(
                    rt,
                    &draft_txt,
                    &tf_sub,
                    D2D_RECT_F { left: x0 + 6.0 * s, top: y_cur, right: x1 - 6.0 * s, bottom: y_cur + 22.0 * s },
                    &br_txt,
                );
                y_cur += 25.0 * s;
            }

            let row_h = (22.0 + (wcfg.param1_pct as f32 / 100.0) * 10.0) * s;
            for (i, (done, text)) in cfg.notes_items.iter().take(4).enumerate() {
                if y_cur + row_h > y + h - 4.0 * s {
                    break;
                }
                let hov = is_btn_hov(i as u8);
                let edit_hov = is_btn_hov(10 + i as u8);
                let mut txt_x = x0;
                if wcfg.feats[0] && wcfg.mode_idx != 1 {
                    let box_rect = D2D_RECT_F {
                        left: x0,
                        top: y_cur + (row_h - 16.0 * s) * 0.5,
                        right: x0 + 16.0 * s,
                        bottom: y_cur + (row_h + 16.0 * s) * 0.5,
                    };
                    if wcfg.mode_idx == 3 {
                        draw_sub_box(box_rect.left, box_rect.top, box_rect.right, box_rect.bottom, 4.0 * s, *done, hov, true);
                        draw_txt_clipped(rt, &format!("{}", i + 1), &tf_center, box_rect, if *done { &br_dark } else { &br_acc });
                    } else if wcfg.mode_idx == 2 || wcfg.feats[4] {
                        draw_sub_box(box_rect.left, box_rect.top, box_rect.right, box_rect.bottom, 8.0 * s, *done, hov, true);
                        draw_txt_clipped(rt, if *done { "✓" } else { "!" }, &tf_center, box_rect, if *done { &br_dark } else { &br_acc });
                    } else {
                        draw_sub_box(box_rect.left, box_rect.top, box_rect.right, box_rect.bottom, 4.0 * s, *done, hov, true);
                        if *done {
                            draw_txt_clipped(rt, "✓", &tf_center, box_rect, &br_dark);
                        }
                    }
                    txt_x = x0 + 24.0 * s;
                }
                let txt_r = if edit_hov { x1 - 24.0 * s } else { x1 - 16.0 * s };
                draw_txt_clipped(
                    rt,
                    text,
                    &tf_body,
                    D2D_RECT_F { left: txt_x, top: y_cur, right: txt_r, bottom: y_cur + row_h },
                    if *done { &br_muted } else { &br_txt },
                );
                draw_txt_clipped(
                    rt,
                    "✎",
                    &tf_sub,
                    D2D_RECT_F { left: x1 - 18.0 * s, top: y_cur, right: x1, bottom: y_cur + row_h },
                    if edit_hov { &br_acc } else { &br_muted },
                );
                if *done && wcfg.feats[1] && wcfg.is_slot_visible(1) && wcfg.is_slot_visible(2) {
                    let mid_y = y_cur + row_h * 0.52;
                    let s_l = txt_x;
                    let s_r = (s_l + (text.len() as f32 * 5.2 * s)).min(txt_r);
                    let mut nb_s: Option<&mut [ClockElemBox]> = None;
                    let gs = begin_elem_xf_slot(rt, 2, wcfg.elem_transforms[2], (s_l, mid_y, s_r, mid_y + 1.0 * s), s, &mut nb_s);
                    rt.FillRectangle(
                        &D2D_RECT_F { left: s_l, top: mid_y, right: s_r, bottom: mid_y + 1.0 * s },
                        br_slot[2].as_ref().unwrap_or(&br_muted),
                    );
                    end_elem_xf_slot(rt, gs);
                }
                y_cur += row_h;
            }
        }

        // =====================================================================
        // 8. MONTHLY CALENDAR & AGENDA GRID
        // =====================================================================
        ExtWidgetId::Calendar => {
            let now = chrono_local();
            let cur_m = state.cal_month.clamp(1, 12);
            let cur_y = state.cal_year;
            let is_this_month = cur_y == now.year && cur_m == month_name_to_num(now.month_name);

            if wcfg.show_header {
                let hdr = if wcfg.feats[4] {
                    format!("{} {} • W{}", month_num_to_name(cur_m).to_uppercase(), cur_y, ((now.day + 6) / 7).max(1))
                } else {
                    format!("{} {}", month_num_to_name(cur_m).to_uppercase(), cur_y)
                };
                draw_txt_clipped(
                    rt,
                    &hdr,
                    &tf_hdr,
                    D2D_RECT_F { left: x0, top: y_cur, right: x1 - 84.0 * s, bottom: y_cur + 18.0 * s },
                    &br_acc,
                );
                if wcfg.feats[2] {
                    draw_pill(x1 - 80.0 * s, y_cur, x1 - 56.0 * s, y_cur + 17.0 * s, "◀", false, is_btn_hov(0));
                    draw_pill(x1 - 53.0 * s, y_cur, x1 - 27.0 * s, y_cur + 17.0 * s, "•", is_this_month, is_btn_hov(1));
                    draw_pill(x1 - 24.0 * s, y_cur, x1, y_cur + 17.0 * s, "▶", false, is_btn_hov(2));
                }
                y_cur += 20.0 * s;
                if wcfg.show_dividers {
                    draw_ext_divider(y_cur);
                    y_cur += 4.0 * s;
                }
            }

            let mon_first = wcfg.mode_idx != 1;
            let dow_hdrs = if mon_first {
                ["M", "T", "W", "T", "F", "S", "S"]
            } else {
                ["S", "M", "T", "W", "T", "F", "S"]
            };
            let cell_w = (x1 - x0) / 7.0;
            if wcfg.is_slot_visible(1) {
                for (c, &dl) in dow_hdrs.iter().enumerate() {
                    let cx0 = x0 + (c as f32) * cell_w;
                    draw_txt_clipped(
                        rt,
                        dl,
                        &tf_center,
                        D2D_RECT_F { left: cx0, top: y_cur, right: cx0 + cell_w, bottom: y_cur + 16.0 * s },
                        &br_acc,
                    );
                }
            }
            y_cur += 17.0 * s;

            let first_dow_sun = first_weekday_of_month(cur_y, cur_m);
            let offset = if mon_first { (first_dow_sun + 6) % 7 } else { first_dow_sun };
            let total_days = days_in_month(cur_y, cur_m);
            let agenda_reserve = if (wcfg.feats[3] || wcfg.mode_idx == 2) && wcfg.show_footer { 18.0 * s } else { 4.0 * s };
            let base_row_h = if wcfg.mode_idx == 3 { 17.5 } else { 18.0 + (wcfg.param1_pct as f32 / 100.0) * 5.0 };
            let row_h = base_row_h * s;

            for day in 1..=total_days {
                let idx = offset + day - 1;
                let col = idx % 7;
                let row = idx / 7;
                let cx0 = x0 + (col as f32) * cell_w;
                let cy0 = y_cur + (row as f32) * row_h;
                if cy0 + row_h > y + h - agenda_reserve {
                    break;
                }
                let is_today = is_this_month && day == now.day;
                if is_today && wcfg.feats[0] {
                    draw_sub_box(
                        cx0 + 2.0 * s,
                        cy0 + 1.0 * s,
                        cx0 + cell_w - 2.0 * s,
                        cy0 + row_h - 1.0 * s,
                        5.0 * s,
                        true,
                        false,
                        wcfg.param2_pct > 40,
                    );
                }
                if wcfg.is_slot_visible(1) {
                    let is_wknd = if mon_first { col >= 5 } else { col == 0 || col == 6 };
                    let d_str = format!("{}", day);
                    draw_txt_clipped(
                        rt,
                        &d_str,
                        &tf_center,
                        D2D_RECT_F { left: cx0, top: cy0, right: cx0 + cell_w, bottom: cy0 + row_h },
                        if is_today && wcfg.feats[0] {
                            &br_dark
                        } else if is_wknd && wcfg.feats[1] {
                            &br_acc
                        } else {
                            &br_txt
                        },
                    );
                }
                if wcfg.feats[5] && (day == 5 || day == 14 || day == 22) && !is_today {
                    with_slot4!(cx0, cy0 + row_h - 5.0 * s, cx0 + cell_w, cy0 + row_h, {
                        rt.FillEllipse(
                            &D2D1_ELLIPSE {
                                point: Vector2 { X: cx0 + cell_w * 0.5, Y: cy0 + row_h - 2.5 * s },
                                radiusX: 1.5 * s,
                                radiusY: 1.5 * s,
                            },
                            br_div_fill,
                        );
                    });
                }
            }
            if (wcfg.feats[3] || wcfg.mode_idx == 2) && wcfg.show_footer {
                let ay = y + h - 18.0 * s;
                with_slot4!(x0, ay, x1, ay + 14.0 * s, {
                    draw_shadowed_text(
                        rt,
                        &format!("Today • {} {}", now.month_name, now.day),
                        &tf_sub,
                        D2D_RECT_F { left: x0, top: ay, right: x1, bottom: ay + 14.0 * s },
                        br_slot[4].as_ref().unwrap_or(&br_sub),
                        None,
                        0.0,
                        true,
                    );
                });
            }
        }

        // =====================================================================
        // 9. POMODORO FOCUS TIMER
        // =====================================================================
        ExtWidgetId::Pomodoro => {
            if wcfg.show_header {
                let phase_lbl = if state.pomo_break {
                    "POMODORO • BREAK PHASE"
                } else {
                    "POMODORO • DEEP FOCUS"
                };
                draw_txt_clipped(
                    rt,
                    phase_lbl,
                    &tf_hdr,
                    D2D_RECT_F { left: x0, top: y_cur, right: if wcfg.feats[5] { x1 - 56.0 * s } else { x1 }, bottom: y_cur + 16.0 * s },
                    if state.pomo_break { &br_sage } else { &br_acc },
                );
                if wcfg.feats[5] {
                    let b_rc = D2D_RECT_F { left: x1 - 52.0 * s, top: y_cur, right: x1, bottom: y_cur + 15.0 * s };
                    with_slot4!(b_rc.left, b_rc.top, b_rc.right, b_rc.bottom, {
                        rt.FillRoundedRectangle(
                            &D2D1_ROUNDED_RECT { rect: b_rc, radiusX: 5.0 * s, radiusY: 5.0 * s },
                            if state.pomo_break && wcfg.elem_transforms[4].color_rgb.is_none() { &br_sage } else { br_div_fill },
                        );
                        draw_shadowed_text(rt, if state.pomo_break { "BREAK" } else { "FOCUS" }, &tf_center, b_rc, &br_dark, None, 0.0, true);
                    });
                }
                y_cur += 18.0 * s;
                if wcfg.show_dividers {
                    draw_ext_divider(y_cur);
                    y_cur += 4.0 * s;
                }
            }

            let total_s = if state.pomo_break {
                ExtRuntimeState::pomo_break_secs(wcfg.mode_idx)
            } else {
                ExtRuntimeState::pomo_work_secs(wcfg.mode_idx)
            }
            .max(1);
            let frac = 1.0 - (state.pomo_rem_secs as f32 / total_s as f32).clamp(0.0, 1.0);
            let mins = state.pomo_rem_secs / 60;
            let secs = state.pomo_rem_secs % 60;
            let time_str = format!("{:02}:{:02}", mins, secs);

            let mut text_x = x0;
            if wcfg.feats[0] && wcfg.show_icon {
                let r_rad = 26.0 * s;
                let r_cx = x0 + r_rad + 4.0 * s;
                let r_cy = y_cur + r_rad + 4.0 * s;
                let ring_w = (2.5 + (wcfg.param1_pct as f32 / 100.0) * 5.0) * s;
                with_box_slot!(r_cx - r_rad, r_cy - r_rad, r_cx + r_rad, r_cy + r_rad, {
                    if wcfg.param2_pct > 20 {
                        let ga = (wcfg.param2_pct as f32 / 100.0) * 0.28;
                        if let Ok(gb) = rt.CreateSolidColorBrush(&rgba_u8(b3_r, b3_g, b3_b, ga), None) {
                            rt.DrawEllipse(&D2D1_ELLIPSE { point: Vector2 { X: r_cx, Y: r_cy }, radiusX: r_rad + 2.5 * s, radiusY: r_rad + 2.5 * s }, &gb, ring_w * 0.7, None);
                        }
                    }
                    draw_ring_arc(
                        rt,
                        d2d,
                        r_cx,
                        r_cy,
                        r_rad,
                        ring_w,
                        frac.max(0.04),
                        &br_acc_dim,
                        if state.pomo_break && box_ov.is_none() { &br_sage } else { br_box_fill },
                    );
                });
                let pct_s = format!("{:.0}%", frac * 100.0);
                draw_txt_clipped(
                    rt,
                    &pct_s,
                    &tf_center,
                    D2D_RECT_F { left: r_cx - r_rad, top: r_cy - 10.0 * s, right: r_cx + r_rad, bottom: r_cy + 10.0 * s },
                    &br_txt,
                );
                text_x += r_rad * 2.0 + 16.0 * s;
            }

            draw_txt_clipped(
                rt,
                &time_str,
                &tf_big,
                D2D_RECT_F { left: text_x, top: y_cur, right: x1, bottom: y_cur + 30.0 * s },
                &br_txt,
            );
            if wcfg.feats[1] && wcfg.show_footer {
                let streak = format!("Sessions: {}  {}", state.pomo_sessions, "● ".repeat((state.pomo_sessions as usize).min(6)));
                draw_txt_clipped(
                    rt,
                    &streak,
                    &tf_sub,
                    D2D_RECT_F { left: text_x, top: y_cur + 28.0 * s, right: x1, bottom: y_cur + 44.0 * s },
                    &br_acc,
                );
            }

            let play = pomodoro_button_layout(wcfg);
            if let Some(r0) = play.start_btn {
                draw_pill(
                    x + r0.0 * s,
                    y + r0.1 * s,
                    x + r0.2 * s,
                    y + r0.3 * s,
                    if state.pomo_running { "⏸ Pause" } else { "▶ Start" },
                    state.pomo_running,
                    is_btn_hov(0),
                );
            }
            if let Some(r1) = play.reset_btn {
                draw_pill(
                    x + r1.0 * s,
                    y + r1.1 * s,
                    x + r1.2 * s,
                    y + r1.3 * s,
                    "↻",
                    false,
                    is_btn_hov(1),
                );
            }
            if let Some(r2) = play.skip_btn {
                draw_pill(
                    x + r2.0 * s,
                    y + r2.1 * s,
                    x + r2.2 * s,
                    y + r2.3 * s,
                    "Skip →",
                    false,
                    is_btn_hov(2),
                );
            }
        }

        // =====================================================================
        // 10. MULTI-TIMEZONE WORLD CLOCK
        // =====================================================================
        ExtWidgetId::WorldClock => {
            if wcfg.show_header {
                draw_txt_clipped(
                    rt,
                    "WORLD CLOCK",
                    &tf_hdr,
                    D2D_RECT_F { left: x0, top: y_cur, right: x1 - 92.0 * s, bottom: y_cur + 16.0 * s },
                    &br_acc,
                );
                draw_pill(x1 - 88.0 * s, y_cur, x1 - 42.0 * s, y_cur + 16.0 * s, "Cities", false, is_btn_hov(1));
                draw_pill(
                    x1 - 38.0 * s,
                    y_cur,
                    x1,
                    y_cur + 16.0 * s,
                    if wcfg.feats[0] { "24H" } else { "12H" },
                    wcfg.feats[0],
                    is_btn_hov(0),
                );
                y_cur += 19.0 * s;
                if wcfg.show_dividers {
                    draw_ext_divider(y_cur);
                    y_cur += 4.0 * s;
                }
            }

            let utc = unsafe { GetSystemTime() };
            let utc_mins = (utc.wHour as i32) * 60 + (utc.wMinute as i32);
            let cities = resolve_world_clock_slots(cfg);

            let max_c = if wcfg.feats[5] { 4 } else { 3 };
            let spacing_mul = (0.78 + (wcfg.param1_pct as f32 / 100.0) * 0.42).clamp(0.75, 1.25);
            let row_h = (((y + h - 6.0 * s - y_cur) / max_c as f32) * spacing_mul).clamp(16.0 * s, 28.0 * s);
            let dial_r = (5.0 + (wcfg.param2_pct as f32 / 100.0) * 3.5) * s;
            for (row_i, wc) in cities.iter().take(max_c).enumerate() {
                if y_cur + row_h > y + h - 2.0 * s {
                    break;
                }
                if is_btn_hov(10 + row_i as u8) {
                    draw_sub_box(x0 - 3.0 * s, y_cur + 1.0 * s, x1 + 3.0 * s, y_cur + row_h - 1.0 * s, 4.0 * s, false, true, false);
                }
                let city = wc.name;
                let offset_m = wc.offset_mins;
                let tz_lbl = wc.tz_abbr;
                let raw_m = utc_mins + offset_m;
                let day_shift = if raw_m < 0 { " -1d" } else if raw_m >= 1440 { " +1d" } else { "" };
                let tot_m = raw_m.rem_euclid(1440);
                let hr24 = (tot_m / 60) as u32;
                let mn = (tot_m % 60) as u32;
                let t_str = if wcfg.feats[0] {
                    format!("{:02}:{:02}", hr24, mn)
                } else {
                    let ap = if hr24 >= 12 { "PM" } else { "AM" };
                    let h12 = match hr24 % 12 { 0 => 12, v => v };
                    format!("{:02}:{:02} {}", h12, mn, ap)
                };
                let mut cx_lbl = x0;
                if wcfg.feats[2] && wcfg.show_icon {
                    let dcx = x0 + dial_r + 1.0 * s;
                    let dcy = y_cur + row_h * 0.5;
                    with_slot4!(dcx - dial_r, dcy - dial_r, dcx + dial_r, dcy + dial_r, {
                        rt.DrawEllipse(&D2D1_ELLIPSE { point: Vector2 { X: dcx, Y: dcy }, radiusX: dial_r, radiusY: dial_r }, br_div_fill, 1.1 * s, None);
                        let ha = (((hr24 % 12) as f32 + mn as f32 / 60.0) * 30.0 - 90.0).to_radians();
                        rt.DrawLine(
                            Vector2 { X: dcx, Y: dcy },
                            Vector2 { X: dcx + dial_r * 0.62 * ha.cos(), Y: dcy + dial_r * 0.62 * ha.sin() },
                            br_div_fill,
                            1.1 * s,
                            None,
                        );
                    });
                    cx_lbl += dial_r * 2.0 + 6.0 * s;
                }
                let dn_ico = if wcfg.feats[4] { if (6..19).contains(&hr24) { "☀ " } else { "☾ " } } else { "" };
                let tz_part = if wcfg.feats[1] && wcfg.show_footer { format!(" ({})", tz_lbl) } else { String::new() };
                let d_part = if wcfg.feats[3] && wcfg.show_footer { day_shift } else { "" };
                let c_lbl = format!("{}{}{}{}", dn_ico, city, tz_part, d_part);
                draw_txt_clipped(
                    rt,
                    &c_lbl,
                    &tf_body,
                    D2D_RECT_F { left: cx_lbl, top: y_cur, right: x1 - 74.0 * s, bottom: y_cur + row_h },
                    &br_txt,
                );
                draw_txt_clipped(
                    rt,
                    &t_str,
                    &tf_right,
                    D2D_RECT_F { left: x1 - 80.0 * s, top: y_cur, right: x1, bottom: y_cur + row_h },
                    &br_acc,
                );
                y_cur += row_h;
            }
        }

        // =====================================================================
        // 11. CLIPBOARD HISTORY PILL
        // =====================================================================
        ExtWidgetId::Clipboard => {
            if wcfg.show_header {
                draw_txt_clipped(
                    rt,
                    "CLIPBOARD HISTORY",
                    &tf_hdr,
                    D2D_RECT_F { left: x0, top: y_cur, right: x1 - 54.0 * s, bottom: y_cur + 16.0 * s },
                    &br_acc,
                );
                if wcfg.feats[3] {
                    draw_pill(x1 - 48.0 * s, y_cur, x1, y_cur + 16.0 * s, "Clear", false, is_btn_hov(4));
                }
                y_cur += 19.0 * s;
                if wcfg.show_dividers {
                    draw_ext_divider(y_cur);
                    y_cur += 4.0 * s;
                }
            }

            if state.clip_items.is_empty() {
                draw_txt_clipped(
                    rt,
                    "Clipboard empty — copy any text (Ctrl+C)",
                    &tf_body,
                    D2D_RECT_F { left: x0, top: y_cur, right: x1, bottom: y_cur + 28.0 * s },
                    &br_sub,
                );
            } else {
                let max_items = match wcfg.mode_idx {
                    0 | 3 => 4,
                    2 => 2,
                    _ => 3,
                };
                let row_h = (20.0 + (wcfg.param1_pct as f32 / 100.0) * 10.0) * s;
                for (i, (snippet, char_cnt)) in state.clip_items.iter().take(max_items).enumerate() {
                    if y_cur + row_h > y + h - 4.0 * s {
                        break;
                    }
                    let hov = is_btn_hov(i as u8);
                    let flashed = matches!(state.clip_copied_flash, Some((fi, _)) if fi == i);
                    draw_sub_box(x0, y_cur + 1.0 * s, x1, y_cur + row_h - 2.0 * s, 5.0 * s, flashed, hov, false);
                    let disp_snip = if wcfg.feats[4] && snippet.len() >= 20 && !snippet.contains(' ') {
                        "••••••••••••••••".to_string()
                    } else {
                        snippet.clone()
                    };
                    let shown = if flashed {
                        "✓ Copied to clipboard!".to_string()
                    } else if wcfg.feats[2] && wcfg.show_footer {
                        format!("{} ({}c)", disp_snip, char_cnt)
                    } else {
                        disp_snip
                    };
                    draw_txt_clipped(
                        rt,
                        &shown,
                        &tf_body,
                        D2D_RECT_F { left: x0 + 8.0 * s, top: y_cur, right: x1 - 8.0 * s, bottom: y_cur + row_h },
                        if flashed { &br_dark } else { &br_txt },
                    );
                    y_cur += row_h;
                }
            }
        }

        // =====================================================================
        // 12. DRIVE / DISK STORAGE RINGS
        // =====================================================================
        ExtWidgetId::Drives => {
            if wcfg.show_header {
                draw_txt_clipped(
                    rt,
                    "STORAGE DRIVES",
                    &tf_hdr,
                    D2D_RECT_F { left: x0, top: y_cur, right: x1 - 62.0 * s, bottom: y_cur + 16.0 * s },
                    &br_acc,
                );
                let style_lbl = match wcfg.mode_idx {
                    1 => "Bars",
                    2 => "Capsules",
                    3 => "Table",
                    _ => "Rings",
                };
                draw_pill(x1 - 58.0 * s, y_cur, x1, y_cur + 16.0 * s, style_lbl, false, is_btn_hov(4));
                y_cur += 19.0 * s;
                if wcfg.show_dividers {
                    draw_ext_divider(y_cur);
                    y_cur += 4.0 * s;
                }
            }

            if state.drives.is_empty() {
                draw_txt_clipped(
                    rt,
                    "Scanning storage drives...",
                    &tf_body,
                    D2D_RECT_F { left: x0, top: y_cur + 8.0 * s, right: x1, bottom: y_cur + 32.0 * s },
                    &br_sub,
                );
            }
            let count = state.drives.len().clamp(1, 2);
            let slot_w = (x1 - x0) / count as f32;
            let warn_thr = 60.0 + (wcfg.param2_pct as f32 / 100.0) * 35.0;
            let thick = (3.0 + (wcfg.param1_pct as f32 / 100.0) * 5.0) * s;
            for (i, d) in state.drives.iter().take(2).enumerate() {
                let sx0 = x0 + (i as f32) * slot_w;
                let frac = (d.pct / 100.0).clamp(0.02, 0.99);
                let warn = d.pct >= warn_thr && wcfg.feats[5];
                let ring_br = if warn && box_ov.is_none() { &br_coral } else { br_box_fill };

                if wcfg.mode_idx == 1 || wcfg.mode_idx == 2 {
                    let lbl = if wcfg.feats[0] && wcfg.feats[1] {
                        format!("{}: {:.0}/{:.0} GB ({:.0}%)", d.letter, d.used_gb, d.total_gb, d.pct)
                    } else if wcfg.feats[1] {
                        format!("{}: {:.0}% used", d.letter, d.pct)
                    } else {
                        format!("{}: {:.0} GB free", d.letter, (d.total_gb - d.used_gb).max(0.0))
                    };
                    draw_txt_clipped(
                        rt,
                        &lbl,
                        &tf_sub,
                        D2D_RECT_F { left: x0, top: y_cur, right: x1, bottom: y_cur + 18.0 * s },
                        &br_txt,
                    );
                    let bh = thick.clamp(5.0 * s, 11.0 * s);
                    let br_rad = if wcfg.mode_idx == 2 { bh * 0.5 } else { 2.0 * s };
                    with_box_slot!(x0, y_cur + 19.0 * s, x1, y_cur + 19.0 * s + bh, {
                        rt.FillRoundedRectangle(
                            &D2D1_ROUNDED_RECT { rect: D2D_RECT_F { left: x0, top: y_cur + 19.0 * s, right: x1, bottom: y_cur + 19.0 * s + bh }, radiusX: br_rad, radiusY: br_rad },
                            &br_acc_dim,
                        );
                        rt.FillRoundedRectangle(
                            &D2D1_ROUNDED_RECT { rect: D2D_RECT_F { left: x0, top: y_cur + 19.0 * s, right: x0 + (x1 - x0) * frac, bottom: y_cur + 19.0 * s + bh }, radiusX: br_rad, radiusY: br_rad },
                            ring_br,
                        );
                    });
                    y_cur += bh + 26.0 * s;
                } else if wcfg.mode_idx == 3 {
                    let line = format!("{}:  {:.0}%  •  {:.0}G / {:.0}G", d.letter, d.pct, d.used_gb, d.total_gb);
                    draw_sub_box(x0, y_cur, x1, y_cur + 24.0 * s, 5.0 * s, false, false, false);
                    draw_txt_clipped(rt, &line, &tf_body, D2D_RECT_F { left: x0 + 8.0 * s, top: y_cur, right: x1 - 8.0 * s, bottom: y_cur + 24.0 * s }, &br_txt);
                    y_cur += 28.0 * s;
                } else {
                    let rad_r = 24.0 * s;
                    let r_cx = sx0 + rad_r + 4.0 * s;
                    let r_cy = y_cur + rad_r + 6.0 * s;
                    with_box_slot!(r_cx - rad_r, r_cy - rad_r, r_cx + rad_r, r_cy + rad_r, {
                        draw_ring_arc(rt, d2d, r_cx, r_cy, rad_r, thick, frac, &br_acc_dim, ring_br);
                    });
                    if wcfg.feats[3] && wcfg.is_slot_visible(3) {
                        with_box_slot!(r_cx + rad_r - 5.0 * s, r_cy - rad_r + 1.0 * s, r_cx + rad_r + 1.0 * s, r_cy - rad_r + 7.0 * s, {
                            rt.FillEllipse(
                                &D2D1_ELLIPSE { point: Vector2 { X: r_cx + rad_r - 2.0 * s, Y: r_cy - rad_r + 4.0 * s }, radiusX: 2.5 * s, radiusY: 2.5 * s },
                                if state.tick_counter % 2 == 0 { &br_sage } else { &br_acc_dim },
                            );
                        });
                    }
                    let dl = format!("{}:", d.letter);
                    draw_txt_clipped(
                        rt,
                        &dl,
                        &tf_center,
                        D2D_RECT_F { left: r_cx - rad_r, top: r_cy - 10.0 * s, right: r_cx + rad_r, bottom: r_cy + 10.0 * s },
                        &br_txt,
                    );
                    if wcfg.feats[1] {
                        let pct_s = format!("{:.0}%", d.pct);
                        draw_txt_clipped(
                            rt,
                            &pct_s,
                            &tf_title,
                            D2D_RECT_F { left: r_cx + rad_r + 8.0 * s, top: r_cy - 18.0 * s, right: sx0 + slot_w, bottom: r_cy + 2.0 * s },
                            &br_txt,
                        );
                    }
                    if wcfg.feats[0] && wcfg.show_footer {
                        let gb_s = format!("{:.0}G free", (d.total_gb - d.used_gb).max(0.0));
                        draw_txt_clipped(
                            rt,
                            &gb_s,
                            &tf_sub,
                            D2D_RECT_F { left: r_cx + rad_r + 8.0 * s, top: r_cy + 2.0 * s, right: sx0 + slot_w, bottom: r_cy + 20.0 * s },
                            &br_sub,
                        );
                    }
                }
            }
        }

        // =====================================================================
        // 13. TOP PROCESS RESOURCE KILLER
        // =====================================================================
        ExtWidgetId::Processes => {
            if wcfg.show_header {
                let hdr = if wcfg.mode_idx == 1 { "TOP PROCESSES (CPU)" } else { "TOP PROCESSES (RAM)" };
                draw_txt_clipped(
                    rt,
                    hdr,
                    &tf_hdr,
                    D2D_RECT_F { left: x0, top: y_cur, right: x1 - 54.0 * s, bottom: y_cur + 16.0 * s },
                    &br_acc,
                );
                draw_pill(x1 - 50.0 * s, y_cur, x1, y_cur + 16.0 * s, "TaskMgr", false, is_btn_hov(4));
                y_cur += 19.0 * s;
                if wcfg.show_dividers {
                    draw_ext_divider(y_cur);
                    y_cur += 4.0 * s;
                }
            }

            if state.top_procs.is_empty() {
                draw_txt_clipped(
                    rt,
                    "Scanning running processes...",
                    &tf_body,
                    D2D_RECT_F { left: x0, top: y_cur + 8.0 * s, right: x1, bottom: y_cur + 32.0 * s },
                    &br_sub,
                );
            }
            let max_p = if wcfg.mode_idx == 3 { 4 } else { 3 };
            let row_h = ((y + h - 6.0 * s - y_cur) / max_p as f32).clamp(20.0 * s, 28.0 * s);
            let max_mb = state.top_procs.iter().map(|p| p.mem_mb).fold(128.0f32, f32::max);
            for (i, p) in state.top_procs.iter().take(max_p).enumerate() {
                if y_cur + row_h > y + h - 4.0 * s {
                    break;
                }
                if wcfg.param1_pct > 10 {
                    let bw_max = (x1 - x0) * (wcfg.param1_pct as f32 / 100.0) * 0.72;
                    let bw = (bw_max * (p.mem_mb / max_mb).clamp(0.08, 1.0)).max(6.0 * s);
                    with_box_slot!(x0, y_cur + row_h - 3.5 * s, x0 + bw, y_cur + row_h - 1.0 * s, {
                        rt.FillRoundedRectangle(
                            &D2D1_ROUNDED_RECT { rect: D2D_RECT_F { left: x0, top: y_cur + row_h - 3.5 * s, right: x0 + bw, bottom: y_cur + row_h - 1.0 * s }, radiusX: 1.2 * s, radiusY: 1.2 * s },
                            &br_acc_dim,
                        );
                    });
                }
                let p_lbl = if wcfg.feats[3] {
                    format!("{} ({})", p.name, p.pid)
                } else {
                    p.name.clone()
                };
                draw_txt_clipped(
                    rt,
                    &p_lbl,
                    &tf_body,
                    D2D_RECT_F { left: x0, top: y_cur, right: x1 - 88.0 * s, bottom: y_cur + row_h },
                    &br_txt,
                );
                if wcfg.show_footer {
                    let mb_s = format!("{:.0} MB", p.mem_mb);
                    draw_txt_clipped(
                        rt,
                        &mb_s,
                        &tf_right,
                        D2D_RECT_F {
                            left: x1 - 92.0 * s,
                            top: y_cur,
                            right: if wcfg.feats[2] { x1 - 26.0 * s } else { x1 },
                            bottom: y_cur + row_h,
                        },
                        &br_acc,
                    );
                }
                if wcfg.feats[2] {
                    draw_pill(x1 - 22.0 * s, y_cur + 3.0 * s, x1, y_cur + row_h - 3.0 * s, "×", false, is_btn_hov(i as u8));
                }
                y_cur += row_h;
            }
        }

        // =====================================================================
        // 14. TRASH BIN / RECYCLE CLEANER
        // =====================================================================
        ExtWidgetId::RecycleBin => {
            let cy = y + h * 0.5;
            let rad_r = 25.0 * s;
            let r_cx = x0 + rad_r + 2.0 * s;
            let mb = state.bin_bytes as f64 / (1024.0 * 1024.0);
            let frac = ((mb / 1024.0) as f32).clamp(if state.bin_items > 0 { 0.08 } else { 0.0 }, 0.99);
            let ring_w = (2.8 + (wcfg.param1_pct as f32 / 100.0) * 4.2) * s;

            let mut tx0 = x0;
            if wcfg.show_icon {
                with_box_slot!(r_cx - rad_r, cy - rad_r, r_cx + rad_r, cy + rad_r, {
                    if wcfg.feats[4] {
                        draw_ring_arc(rt, d2d, r_cx, cy, rad_r, ring_w, frac, &br_acc_dim, br_box_fill);
                    }
                    rt.FillRoundedRectangle(
                        &D2D1_ROUNDED_RECT {
                            rect: D2D_RECT_F { left: r_cx - 8.0 * s, top: cy - 8.5 * s, right: r_cx + 8.0 * s, bottom: cy - 6.0 * s },
                            radiusX: 1.0 * s,
                            radiusY: 1.0 * s,
                        },
                        br_box_fill,
                    );
                    rt.DrawRoundedRectangle(
                        &D2D1_ROUNDED_RECT {
                            rect: D2D_RECT_F { left: r_cx - 6.5 * s, top: cy - 5.0 * s, right: r_cx + 6.5 * s, bottom: cy + 9.0 * s },
                            radiusX: 1.8 * s,
                            radiusY: 1.8 * s,
                        },
                        br_box_fill,
                        1.4 * s,
                        None,
                    );
                });
                tx0 = r_cx + rad_r + 12.0 * s;
            }

            if wcfg.show_header {
                draw_txt_clipped(
                    rt,
                    "RECYCLE BIN",
                    &tf_hdr,
                    D2D_RECT_F { left: tx0, top: cy - 24.0 * s, right: x1 - 76.0 * s, bottom: cy - 6.0 * s },
                    &br_acc,
                );
            }
            if wcfg.show_primary || wcfg.show_footer {
                let size_s = if mb >= 1024.0 {
                    format!("{:.2} GB", mb / 1024.0)
                } else {
                    format!("{:.1} MB", mb)
                };
                let info_s = match (wcfg.show_primary, wcfg.show_footer) {
                    (true, true) => format!("{} items • {}", state.bin_items, size_s),
                    (true, false) => format!("{} items", state.bin_items),
                    (false, true) => size_s,
                    (false, false) => String::new(),
                };
                let tf_rb = if wcfg.show_primary { &tf_body } else { &tf_sub };
                draw_txt_clipped(
                    rt,
                    &info_s,
                    tf_rb,
                    D2D_RECT_F { left: tx0, top: cy - 4.0 * s, right: x1 - 76.0 * s, bottom: cy + 20.0 * s },
                    &br_txt,
                );
            }

            if wcfg.feats[2] {
                let b_rc = D2D_RECT_F { left: x1 - 72.0 * s, top: cy - 13.0 * s, right: x1, bottom: cy + 13.0 * s };
                with_slot4!(b_rc.left, b_rc.top, b_rc.right, b_rc.bottom, {
                    let act = state.bin_items > 0;
                    rt.FillRoundedRectangle(
                        &D2D1_ROUNDED_RECT { rect: b_rc, radiusX: 6.0 * s, radiusY: 6.0 * s },
                        if act { br_slot[4].as_ref().unwrap_or(&br_acc) } else { &br_acc_dim },
                    );
                    draw_shadowed_text(rt, "Empty", &tf_center, b_rc, if act { &br_dark } else { &br_txt }, None, 0.0, true);
                });
            }
        }
    }

    rt.PopAxisAlignedClip();
    };

    if out_boxes.is_none() {
        if let Some((slot, cache_gen)) = fg_cache {
            slot.draw_cached(rt, cache_gen, fg_key, x, y, w, h, 2.0 * s, |fg_rt| {
                draw_fg(fg_rt);
            });
        } else {
            draw_fg(rt);
        }
    } else {
        draw_fg(rt);
    }

    if id == ExtWidgetId::Weather && wcfg.feats[5] {
        let x0 = x + pad;
        let x1 = x + w - pad;
        let y_sp = y + pad * 0.75 + if wcfg.show_header { 20.0 * s } else { 0.0 };
        let (sp_r, sp_g, sp_b, sp_a) = match wcfg.elem_transforms[3].color_rgb {
            Some(c) => {
                let (r, g, b) = u32_to_rgb(c);
                (r, g, b, 0.68)
            }
            None => (ar, ag, ab, 0.55),
        };
        if let Ok(sp_br) = rt.CreateSolidColorBrush(&rgba_u8(sp_r, sp_g, sp_b, sp_a), None) {
            rt.PushAxisAlignedClip(&card_rect, D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
            let phase = (state.tick_counter % 20) as f32;
            for pi in 0..5 {
                let px = x0 + 24.0 * s + ((pi as f32 * 58.0 + phase * 6.0) % (x1 - x0 - 30.0 * s).max(20.0));
                let py = y_sp + ((pi as f32 * 9.0 + phase * 2.5) % 36.0) * s;
                rt.FillEllipse(
                    &D2D1_ELLIPSE { point: Vector2 { X: px, Y: py }, radiusX: 1.5 * s, radiusY: 1.5 * s },
                    &sp_br,
                );
            }
            rt.PopAxisAlignedClip();
        }
    }

    if let Some(boxes) = out_boxes.as_deref_mut() {
        let tracked = slot_bounds.borrow();
        let x0 = x + pad;
        let x1 = x + w - pad;
        let fallback_rects = [
            (x0, y + pad * 0.65, x1, y + pad * 0.65 + 20.0 * s),
            (x0, y + h * 0.24, x1, y + h * 0.56),
            (x0, y + h * 0.52, x1, y + h * 0.78),
            (x0, y + h * 0.68, x1, y + h - pad * 0.65),
            (x0, y + h * 0.48, x1, y + h * 0.52),
        ];
        for i in 0..5 {
            let (l, t, r, bt) = tracked[i].unwrap_or(fallback_rects[i]);
            let tr = wcfg.elem_transforms[i];
            let base_cx = (l + r) * 0.5;
            let base_cy = (t + bt) * 0.5;
            let hw = ((r - l) * 0.5).max(8.0 * s);
            let hh = ((bt - t) * 0.5).max(8.0 * s);
            let sc = (tr.scale_pct as f32 / 100.0).clamp(0.35, 2.60);
            boxes[i] = ClockElemBox {
                visible: tracked[i].is_some(),
                elem_hidden: !tr.visible,
                base_cx,
                base_cy,
                cx: base_cx + tr.dx as f32 * s,
                cy: base_cy + tr.dy as f32 * s,
                tr_half_w: hw * sc,
                tr_half_h: hh * sc,
                rot_deg: tr.rot_deg as f32,
            };
        }
        let mut slice_opt = Some(&mut boxes[..]);
        record_elem_box_slot(&mut slice_opt, 5, tr_card, (x, y, x + w, y + h), s);
    }
}

// =============================================================================
// NATIVE WINHTTP AUTO-LOCATION WEATHER (IP Geolocation + Open-Meteo + wttr.in)
// =============================================================================

fn spawn_auto_weather_fetch(
    target: Arc<Mutex<Option<WeatherSnap>>>,
    in_flight: Arc<std::sync::atomic::AtomicBool>,
) {
    use std::sync::atomic::Ordering;
    if in_flight
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return;
    }
    std::thread::spawn(move || {
        let fetched = fetch_auto_location_weather();
        if let Some(snap) = fetched {
            if let Ok(mut g) = target.lock() {
                *g = Some(snap);
            }
        }
        in_flight.store(false, Ordering::Release);
    });
}

fn fetch_auto_location_weather() -> Option<WeatherSnap> {
    let today_dow = local_day_of_week_sun0();

    // 1. Primary: IP Geolocation -> Open-Meteo (fast, reliable, 5-day forecast + WMO codes)
    let geo = winhttp_get_text("ipwho.is", "/", true, 3500)
        .and_then(|body| parse_ip_geo_json(&body))
        .or_else(|| {
            winhttp_get_text("ip-api.com", "/json/?fields=status,city,lat,lon", false, 3500)
                .and_then(|body| parse_ip_geo_json(&body))
        });

    if let Some((city, lat, lon)) = geo {
        let path = format!(
            "/v1/forecast?latitude={:.4}&longitude={:.4}&current=temperature_2m,relative_humidity_2m,weather_code,wind_speed_10m&daily=weather_code,temperature_2m_max,temperature_2m_min,sunrise,sunset,uv_index_max&timezone=auto&forecast_days=7",
            lat, lon
        );
        if let Some(body) = winhttp_get_text("api.open-meteo.com", &path, true, 4000) {
            if let Some(snap) = parse_open_meteo_json(&city, &body, today_dow) {
                return Some(snap);
            }
        }
    }

    // 2. Fallback: wttr.in JSON directly via WinHTTP (zero PowerShell)
    if let Some(body) = winhttp_get_text("wttr.in", "/?format=j1", true, 4500) {
        if let Some(snap) = parse_wttr_json(&body) {
            return Some(snap);
        }
    }
    None
}

fn local_day_of_week_sun0() -> usize {
    #[repr(C)]
    #[derive(Default)]
    struct SysTime {
        year: u16,
        month: u16,
        day_of_week: u16,
        day: u16,
        hour: u16,
        minute: u16,
        second: u16,
        millis: u16,
    }
    #[link(name = "kernel32")]
    extern "system" {
        fn GetLocalTime(lp_system_time: *mut SysTime);
    }
    let mut st = SysTime::default();
    unsafe {
        GetLocalTime(&mut st);
    }
    (st.day_of_week as usize) % 7
}

fn winhttp_get_text(host: &str, path: &str, use_https: bool, timeout_ms: i32) -> Option<String> {
    use crate::config::to_wide;
    use std::ptr::{null, null_mut};

    #[link(name = "winhttp")]
    extern "system" {
        fn WinHttpOpen(
            agent: *const u16,
            access_type: u32,
            proxy: *const u16,
            proxy_bypass: *const u16,
            flags: u32,
        ) -> *mut c_void;
        fn WinHttpSetTimeouts(
            handle: *mut c_void,
            resolve_ms: i32,
            connect_ms: i32,
            send_ms: i32,
            receive_ms: i32,
        ) -> i32;
        fn WinHttpConnect(
            session: *mut c_void,
            server_name: *const u16,
            server_port: u16,
            reserved: u32,
        ) -> *mut c_void;
        fn WinHttpOpenRequest(
            connect: *mut c_void,
            verb: *const u16,
            object_name: *const u16,
            version: *const u16,
            referrer: *const u16,
            accept_types: *const *const u16,
            flags: u32,
        ) -> *mut c_void;
        fn WinHttpSendRequest(
            request: *mut c_void,
            headers: *const u16,
            headers_len: u32,
            optional: *mut c_void,
            optional_len: u32,
            total_len: u32,
            context: usize,
        ) -> i32;
        fn WinHttpReceiveResponse(request: *mut c_void, reserved: *mut c_void) -> i32;
        fn WinHttpQueryDataAvailable(request: *mut c_void, avail: *mut u32) -> i32;
        fn WinHttpReadData(
            request: *mut c_void,
            buffer: *mut c_void,
            to_read: u32,
            read: *mut u32,
        ) -> i32;
        fn WinHttpCloseHandle(handle: *mut c_void) -> i32;
    }

    struct HttpGuard(*mut c_void);
    impl Drop for HttpGuard {
        fn drop(&mut self) {
            if !self.0.is_null() {
                unsafe {
                    let _ = WinHttpCloseHandle(self.0);
                }
            }
        }
    }

    const WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY: u32 = 4;
    const WINHTTP_FLAG_SECURE: u32 = 0x0080_0000;
    const MAX_BODY_BYTES: usize = 128 * 1024;

    let agent_w = to_wide("AutumnDeck/2.0");
    let host_w = to_wide(host);
    let path_w = to_wide(path);
    let get_w = to_wide("GET");

    unsafe {
        let session = WinHttpOpen(
            agent_w.as_ptr(),
            WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
            null(),
            null(),
            0,
        );
        if session.is_null() {
            return None;
        }
        let _sg = HttpGuard(session);
        let _ = WinHttpSetTimeouts(session, timeout_ms, timeout_ms, timeout_ms, timeout_ms);

        let port: u16 = if use_https { 443 } else { 80 };
        let conn = WinHttpConnect(session, host_w.as_ptr(), port, 0);
        if conn.is_null() {
            return None;
        }
        let _cg = HttpGuard(conn);

        let req_flags = if use_https { WINHTTP_FLAG_SECURE } else { 0 };
        let req = WinHttpOpenRequest(
            conn,
            get_w.as_ptr(),
            path_w.as_ptr(),
            null(),
            null(),
            null(),
            req_flags,
        );
        if req.is_null() {
            return None;
        }
        let _rg = HttpGuard(req);

        if WinHttpSendRequest(req, null(), 0, null_mut(), 0, 0, 0) == 0 {
            return None;
        }
        if WinHttpReceiveResponse(req, null_mut()) == 0 {
            return None;
        }

        let mut body = Vec::<u8>::with_capacity(4096);
        loop {
            let mut avail = 0u32;
            if WinHttpQueryDataAvailable(req, &mut avail) == 0 || avail == 0 {
                break;
            }
            let remaining = MAX_BODY_BYTES.saturating_sub(body.len());
            if remaining == 0 {
                break;
            }
            let to_read = (avail as usize).min(remaining).min(8192);
            let old_len = body.len();
            body.resize(old_len + to_read, 0);
            let mut got = 0u32;
            if WinHttpReadData(
                req,
                body[old_len..].as_mut_ptr() as *mut c_void,
                to_read as u32,
                &mut got,
            ) == 0
                || got == 0
            {
                body.truncate(old_len);
                break;
            }
            body.truncate(old_len + got as usize);
        }

        if body.is_empty() {
            None
        } else {
            Some(String::from_utf8_lossy(&body).into_owned())
        }
    }
}

fn wmo_code_to_condition(code: i32) -> (&'static str, u8) {
    match code {
        0 => ("Clear Sky", 0),
        1 => ("Mainly Clear", 0),
        2 => ("Partly Cloudy", 1),
        3 => ("Overcast Sky", 1),
        45 | 48 => ("Misty Fog", 1),
        51 | 53 | 55 | 56 | 57 => ("Light Drizzle", 2),
        61 | 63 | 65 | 66 | 67 => ("Rain Showers", 2),
        71 | 73 | 75 | 77 | 85 | 86 => ("Snowfall", 2),
        80 | 81 | 82 => ("Passing Showers", 2),
        95 | 96 | 99 => ("Thunderstorm", 2),
        _ => ("Fair Weather", 0),
    }
}

fn json_find_value<'a>(src: &'a str, key: &str) -> Option<&'a str> {
    let needle = format!("\"{}\"", key);
    let mut pos = 0usize;
    while let Some(rel) = src[pos..].find(&needle) {
        let after = &src[pos + rel + needle.len()..];
        let trimmed = after.trim_start();
        if let Some(rest) = trimmed.strip_prefix(':') {
            return Some(rest.trim_start());
        }
        pos += rel + needle.len();
    }
    None
}

fn json_extract_str(src: &str, key: &str) -> Option<String> {
    let rest = json_find_value(src, key)?.strip_prefix('"')?;
    let mut out = String::new();
    let mut chars = rest.chars();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            if let Some(esc) = chars.next() {
                out.push(esc);
            }
        } else if ch == '"' {
            return Some(out);
        } else {
            out.push(ch);
        }
    }
    None
}

fn json_extract_f32(src: &str, key: &str) -> Option<f32> {
    let rest = json_find_value(src, key)?;
    let rest = rest.strip_prefix('"').unwrap_or(rest);
    let end = rest
        .find(|c: char| !matches!(c, '0'..='9' | '-' | '+' | '.'))
        .unwrap_or(rest.len());
    rest[..end].parse::<f32>().ok()
}

fn json_extract_object<'a>(src: &'a str, key: &str) -> Option<&'a str> {
    let rest = json_find_value(src, key)?;
    if !rest.starts_with('{') {
        return None;
    }
    let mut depth = 0i32;
    let mut in_str = false;
    let mut esc = false;
    for (idx, ch) in rest.char_indices() {
        if in_str {
            if esc {
                esc = false;
            } else if ch == '\\' {
                esc = true;
            } else if ch == '"' {
                in_str = false;
            }
            continue;
        }
        match ch {
            '"' => in_str = true,
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&rest[..=idx]);
                }
            }
            _ => {}
        }
    }
    None
}

fn json_extract_f32_array(src: &str, key: &str) -> Vec<f32> {
    let Some(rest) = json_find_value(src, key).and_then(|r| r.strip_prefix('[')) else {
        return Vec::new();
    };
    let end = rest.find(']').unwrap_or(rest.len());
    rest[..end]
        .split(',')
        .filter_map(|tok| tok.trim().parse::<f32>().ok())
        .collect()
}

fn parse_ip_geo_json(json: &str) -> Option<(String, f32, f32)> {
    if json.contains("\"success\":false")
        || json.contains("\"success\": false")
        || json.contains("\"status\":\"fail\"")
    {
        return None;
    }
    let city = json_extract_str(json, "city")
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())?;
    let lat = json_extract_f32(json, "latitude").or_else(|| json_extract_f32(json, "lat"))?;
    let lon = json_extract_f32(json, "longitude").or_else(|| json_extract_f32(json, "lon"))?;
    if !lat.is_finite() || !lon.is_finite() || lat.abs() > 90.0 || lon.abs() > 180.0 {
        return None;
    }
    Some((city, lat, lon))
}

fn json_extract_first_str_in_array(src: &str, key: &str) -> Option<String> {
    let rest = json_find_value(src, key)?.strip_prefix('[')?.trim_start();
    let rest = rest.strip_prefix('"')?;
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

fn parse_iso_hhmm_minutes(iso: &str) -> Option<u16> {
    let time_part = iso.split('T').nth(1)?;
    let mut parts = time_part.split(':');
    let hh = parts.next()?.parse::<u16>().ok()?;
    let mm = parts.next()?.get(0..2)?.parse::<u16>().ok()?;
    if hh < 24 && mm < 60 {
        Some(hh * 60 + mm)
    } else {
        None
    }
}

fn parse_open_meteo_json(city: &str, json: &str, today_dow_sun0: usize) -> Option<WeatherSnap> {
    const DOW_LABELS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
    let cur_obj = json_extract_object(json, "current")?;
    let temp_c = json_extract_f32(cur_obj, "temperature_2m")?;
    let humidity = json_extract_f32(cur_obj, "relative_humidity_2m")
        .unwrap_or(55.0)
        .round()
        .clamp(0.0, 100.0) as u8;
    let wind_kmh = json_extract_f32(cur_obj, "wind_speed_10m").unwrap_or(10.0).max(0.0);
    let cur_code = json_extract_f32(cur_obj, "weather_code").unwrap_or(0.0).round() as i32;
    let (condition, cur_icon) = wmo_code_to_condition(cur_code);

    let mut snap = WeatherSnap {
        city: city.to_string(),
        temp_c,
        high_c: temp_c + 3.0,
        low_c: temp_c - 4.0,
        humidity,
        wind_kmh,
        condition,
        sunrise_min: 6 * 60 + 24,
        sunset_min: 18 * 60 + 48,
        uv_index: 5,
        forecast_days: [
            (DOW_LABELS[today_dow_sun0 % 7], temp_c + 3.0, temp_c - 4.0, cur_icon),
            (DOW_LABELS[(today_dow_sun0 + 1) % 7], temp_c + 2.0, temp_c - 4.0, cur_icon),
            (DOW_LABELS[(today_dow_sun0 + 2) % 7], temp_c + 1.0, temp_c - 5.0, cur_icon),
            (DOW_LABELS[(today_dow_sun0 + 3) % 7], temp_c + 3.0, temp_c - 3.0, cur_icon),
            (DOW_LABELS[(today_dow_sun0 + 4) % 7], temp_c + 4.0, temp_c - 3.0, cur_icon),
            (DOW_LABELS[(today_dow_sun0 + 5) % 7], temp_c + 2.0, temp_c - 4.0, cur_icon),
            (DOW_LABELS[(today_dow_sun0 + 6) % 7], temp_c + 3.0, temp_c - 3.0, cur_icon),
        ],
    };

    if let Some(daily) = json_extract_object(json, "daily") {
        let highs = json_extract_f32_array(daily, "temperature_2m_max");
        let lows = json_extract_f32_array(daily, "temperature_2m_min");
        let codes = json_extract_f32_array(daily, "weather_code");
        let uvs = json_extract_f32_array(daily, "uv_index_max");
        if let Some(sr) = json_extract_first_str_in_array(daily, "sunrise")
            .as_deref()
            .and_then(parse_iso_hhmm_minutes)
        {
            snap.sunrise_min = sr;
        }
        if let Some(ss) = json_extract_first_str_in_array(daily, "sunset")
            .as_deref()
            .and_then(parse_iso_hhmm_minutes)
        {
            snap.sunset_min = ss;
        }
        if let Some(&uv0) = uvs.first() {
            snap.uv_index = uv0.round().clamp(0.0, 15.0) as u8;
        }
        if let Some(&h0) = highs.first() {
            snap.high_c = h0;
        }
        if let Some(&l0) = lows.first() {
            snap.low_c = l0;
        }
        for i in 0..7 {
            let lbl = DOW_LABELS[(today_dow_sun0 + i) % 7];
            let hi = highs.get(i).copied().unwrap_or(snap.high_c);
            let lo = lows.get(i).copied().unwrap_or(snap.low_c);
            let ic = codes
                .get(i)
                .map(|&c| wmo_code_to_condition(c.round() as i32).1)
                .unwrap_or(cur_icon);
            snap.forecast_days[i] = (lbl, hi, lo, ic);
        }
    }
    Some(snap)
}

fn parse_wttr_json(json: &str) -> Option<WeatherSnap> {
    let temp_c = json_extract_f32(json, "temp_C")?;
    let mut snap = WeatherSnap::default();
    if let Some(area_pos) = json.find("\"areaName\"") {
        if let Some(city) = json_extract_str(&json[area_pos..], "value") {
            let trimmed = city.trim();
            if !trimmed.is_empty() {
                snap.city = trimmed.to_string();
            }
        }
    }
    snap.temp_c = temp_c;
    if let Some(h) = json_extract_f32(json, "maxtempC") {
        snap.high_c = h;
    }
    if let Some(l) = json_extract_f32(json, "mintempC") {
        snap.low_c = l;
    }
    if let Some(hm) = json_extract_f32(json, "humidity") {
        snap.humidity = hm.round().clamp(0.0, 100.0) as u8;
    }
    if let Some(wk) = json_extract_f32(json, "windspeedKmph") {
        snap.wind_kmh = wk.max(0.0);
    }
    Some(snap)
}

#[cfg(test)]
mod weather_geo_tests {
    use super::*;

    #[test]
    fn parse_ip_geo_and_open_meteo_payloads() {
        let ipwho = r#"{"ip":"1.2.3.4","success":true,"city":"Bengaluru","latitude":12.9716,"longitude":77.5946}"#;
        let (city, lat, lon) = parse_ip_geo_json(ipwho).expect("ipwho.is payload must parse");
        assert_eq!(city, "Bengaluru");
        assert!((lat - 12.9716).abs() < 0.001);
        assert!((lon - 77.5946).abs() < 0.001);

        let ipapi = r#"{"status":"success","city":"Pune","lat":18.52,"lon":73.85}"#;
        let (city2, _, _) = parse_ip_geo_json(ipapi).expect("ip-api payload must parse");
        assert_eq!(city2, "Pune");
        assert!(parse_ip_geo_json(r#"{"success":false,"message":"rate limit"}"#).is_none());

        let meteo = r#"{
            "current":{"temperature_2m":26.4,"relative_humidity_2m":61,"weather_code":2,"wind_speed_10m":13.2},
            "daily":{
                "weather_code":[2,61,0,95,3,0,1],
                "temperature_2m_max":[29.5,27.0,30.2,26.1,28.4,29.0,27.6],
                "temperature_2m_min":[19.1,18.4,19.8,17.9,18.7,19.0,18.2],
                "sunrise":["2026-11-21T06:21","2026-11-22T06:22"],
                "sunset":["2026-11-21T17:51","2026-11-22T17:51"],
                "uv_index_max":[7.4,5.2,8.1]
            }
        }"#;
        let snap = parse_open_meteo_json(&city, meteo, 5).expect("open-meteo payload must parse");
        assert_eq!(snap.city, "Bengaluru");
        assert!((snap.temp_c - 26.4).abs() < 0.01);
        assert_eq!(snap.humidity, 61);
        assert_eq!(snap.condition, "Partly Cloudy");
        assert!((snap.high_c - 29.5).abs() < 0.01);
        assert!((snap.low_c - 19.1).abs() < 0.01);
        assert_eq!(snap.sunrise_min, 6 * 60 + 21);
        assert_eq!(snap.sunset_min, 17 * 60 + 51);
        assert_eq!(snap.uv_index, 7);
        assert_eq!(snap.forecast_days[0].0, "Fri");
        assert_eq!(snap.forecast_days[1].0, "Sat");
        assert_eq!(snap.forecast_days[1].3, 2); // Rain icon
        assert_eq!(snap.forecast_days[2].3, 0); // Clear Sun icon
        assert_eq!(snap.forecast_days[3].3, 2); // Thunderstorm icon
        assert_eq!(snap.forecast_days[6].0, "Thu"); // Full 7th day present

        // Verify Celestial Sun-Dial theme serialization & sizing
        let mut wcfg = ExtWidgetConfig::default_for(ExtWidgetId::Weather);
        wcfg.apply_theme_preset(ExtWidgetTheme::CelestialSunDial);
        assert_eq!(wcfg.theme, ExtWidgetTheme::CelestialSunDial);
        assert_eq!(wcfg.logical_size(ExtWidgetId::Weather), (372.0, 214.0));
        let csv = wcfg.to_csv();
        assert!(csv.contains("celestial_sun_dial"));
        let round = ExtWidgetConfig::from_csv(ExtWidgetId::Weather, &csv);
        assert_eq!(round.theme, ExtWidgetTheme::CelestialSunDial);
    }

    #[test]
    fn audio_switcher_layout_hit_tests_and_controls_align_across_configs() {
        let center = |r: (f32, f32, f32, f32)| -> (f32, f32) {
            ((r.0 + r.2) * 0.5, (r.1 + r.3) * 0.5)
        };
        let mut cfg = AppConfig::default();

        // Test default, large padding, compact mode, and header-off configurations
        for (pad, compact, show_hdr, dev_cnt) in [
            (12u8, false, true, 2usize),
            (22u8, false, true, 3usize),
            (12u8, true, true, 2usize),
            (14u8, false, false, 2usize),
        ] {
            let mut wcfg = ExtWidgetConfig::default_for(ExtWidgetId::AudioSwitcher);
            wcfg.padding = pad;
            wcfg.compact_mode = compact;
            wcfg.show_header = show_hdr;

            let lay = audio_switcher_layout(&wcfg, dev_cnt);
            if let Some(r) = lay.mixer_btn {
                let (cx, cy) = center(r);
                assert_eq!(
                    hit_test_ext_widget_with_audio(ExtWidgetId::AudioSwitcher, &wcfg, dev_cnt, cx, cy),
                    WidgetHitZone::ExtBtn(ExtWidgetId::AudioSwitcher, 5)
                );
            }
            let (cx0, cy0) = center(lay.dev_btn_0.expect("dev 0 rect"));
            assert_eq!(
                hit_test_ext_widget_with_audio(ExtWidgetId::AudioSwitcher, &wcfg, dev_cnt, cx0, cy0),
                WidgetHitZone::ExtBtn(ExtWidgetId::AudioSwitcher, 3)
            );
            let (cx1, cy1) = center(lay.dev_btn_1.expect("dev 1 rect"));
            assert_eq!(
                hit_test_ext_widget_with_audio(ExtWidgetId::AudioSwitcher, &wcfg, dev_cnt, cx1, cy1),
                WidgetHitZone::ExtBtn(ExtWidgetId::AudioSwitcher, 4)
            );
            if dev_cnt > 2 {
                let (ccx, ccy) = center(lay.dev_cycle_btn.expect("cycle btn for 3+ outputs"));
                assert_eq!(
                    hit_test_ext_widget_with_audio(ExtWidgetId::AudioSwitcher, &wcfg, dev_cnt, ccx, ccy),
                    WidgetHitZone::ExtBtn(ExtWidgetId::AudioSwitcher, 6)
                );
            }
            let (mx, my) = center(lay.vol_minus_btn.expect("vol - rect"));
            assert_eq!(
                hit_test_ext_widget_with_audio(ExtWidgetId::AudioSwitcher, &wcfg, dev_cnt, mx, my),
                WidgetHitZone::ExtBtn(ExtWidgetId::AudioSwitcher, 0)
            );
            let (ux, uy) = center(lay.vol_mute_btn.expect("mute rect"));
            assert_eq!(
                hit_test_ext_widget_with_audio(ExtWidgetId::AudioSwitcher, &wcfg, dev_cnt, ux, uy),
                WidgetHitZone::ExtBtn(ExtWidgetId::AudioSwitcher, 1)
            );
            let (px, py) = center(lay.vol_plus_btn.expect("vol + rect"));
            assert_eq!(
                hit_test_ext_widget_with_audio(ExtWidgetId::AudioSwitcher, &wcfg, dev_cnt, px, py),
                WidgetHitZone::ExtBtn(ExtWidgetId::AudioSwitcher, 2)
            );
            if let Some(sr) = lay.slider_hit {
                let (sx, sy) = center(sr);
                assert_eq!(
                    hit_test_ext_widget_with_audio(ExtWidgetId::AudioSwitcher, &wcfg, dev_cnt, sx, sy),
                    WidgetHitZone::ExtBtn(ExtWidgetId::AudioSwitcher, 7)
                );
            }
        }

        // Verify buttons 0 (-), 1 (Mute), 2 (+), 3/4 (Outputs), 6 (Page) are wired in handle_desktop_click
        let mut st = ExtRuntimeState::new();
        st.audio_vol_preview = 50;
        st.audio_muted_preview = false;
        assert!(st.handle_desktop_click(ExtWidgetId::AudioSwitcher, 0, &mut cfg));
        assert!(st.audio_vol_preview < 50 && !st.audio_muted_preview);
        assert!(st.handle_desktop_click(ExtWidgetId::AudioSwitcher, 1, &mut cfg));
        assert!(st.audio_muted_preview);
        let after_dec = st.audio_vol_preview;
        assert!(st.handle_desktop_click(ExtWidgetId::AudioSwitcher, 2, &mut cfg));
        assert!(st.audio_vol_preview > after_dec && !st.audio_muted_preview);
        assert!(st.handle_desktop_click(ExtWidgetId::AudioSwitcher, 4, &mut cfg));
        assert_eq!(st.active_audio_out, 1);
        assert!(st.handle_desktop_click(ExtWidgetId::AudioSwitcher, 3, &mut cfg));
        assert_eq!(st.active_audio_out, 0);
        assert!(st.handle_desktop_click(ExtWidgetId::AudioSwitcher, 6, &mut cfg));
        assert_eq!(st.audio_output_page, 1);

        // Verify audio_slot_device paging across 3 devices
        let snap3 = CcSnapshot {
            audio_outputs: vec![
                AudioOutputDevice { id: "1".into(), name: "Speakers".into(), is_default: true },
                AudioOutputDevice { id: "2".into(), name: "realme Buds Air7".into(), is_default: false },
                AudioOutputDevice { id: "3".into(), name: "HDMI Monitor".into(), is_default: false },
            ],
            ..CcSnapshot::default()
        };
        assert_eq!(ExtRuntimeState::audio_slot_device(&snap3, 0, 0).unwrap().id, "1");
        assert_eq!(ExtRuntimeState::audio_slot_device(&snap3, 0, 1).unwrap().id, "2");
        assert_eq!(ExtRuntimeState::audio_slot_device(&snap3, 1, 0).unwrap().id, "2");
        assert_eq!(ExtRuntimeState::audio_slot_device(&snap3, 1, 1).unwrap().id, "3");
    }

    #[test]
    fn pomodoro_button_hit_tests_and_state_transitions_match_rendered_layout() {
        let center = |r: (f32, f32, f32, f32)| -> (f32, f32) {
            ((r.0 + r.2) * 0.5, (r.1 + r.3) * 0.5)
        };
        for (show_ring, compact, show_skip, pad) in [
            (true, false, true, 12u8),
            (false, false, true, 12u8),
            (true, true, true, 12u8),
            (true, false, false, 18u8),
        ] {
            let mut wcfg = ExtWidgetConfig::default_for(ExtWidgetId::Pomodoro);
            wcfg.feats[0] = show_ring;
            wcfg.show_icon = show_ring;
            wcfg.compact_mode = compact;
            wcfg.feats[3] = show_skip;
            wcfg.padding = pad;

            let lay = pomodoro_button_layout(&wcfg);
            let r0 = lay.start_btn.expect("Start button must exist");
            let r1 = lay.reset_btn.expect("Reset button must exist");
            assert!(r0.2 < r1.0, "Start and Reset must not overlap");

            let (sx, sy) = center(r0);
            assert_eq!(
                hit_test_ext_widget(ExtWidgetId::Pomodoro, &wcfg, sx, sy),
                WidgetHitZone::ExtBtn(ExtWidgetId::Pomodoro, 0)
            );
            let (rx, ry) = center(r1);
            assert_eq!(
                hit_test_ext_widget(ExtWidgetId::Pomodoro, &wcfg, rx, ry),
                WidgetHitZone::ExtBtn(ExtWidgetId::Pomodoro, 1)
            );
            if show_skip {
                let r2 = lay.skip_btn.expect("Skip button must exist");
                assert!(r1.2 < r2.0, "Reset and Skip must not overlap");
                let (kx, ky) = center(r2);
                assert_eq!(
                    hit_test_ext_widget(ExtWidgetId::Pomodoro, &wcfg, kx, ky),
                    WidgetHitZone::ExtBtn(ExtWidgetId::Pomodoro, 2)
                );
            } else {
                assert!(lay.skip_btn.is_none());
            }
        }

        // Verify Start -> Pause -> Skip -> Reset state transitions
        let mut cfg = AppConfig::default();
        let mut st = ExtRuntimeState::new();
        assert!(!st.pomo_running);
        assert!(!st.pomo_break);
        assert_eq!(st.pomo_rem_secs, 25 * 60);

        // Start
        assert!(st.handle_desktop_click(ExtWidgetId::Pomodoro, 0, &mut cfg));
        assert!(st.pomo_running);
        // Pause
        assert!(st.handle_desktop_click(ExtWidgetId::Pomodoro, 0, &mut cfg));
        assert!(!st.pomo_running);
        // Skip to break phase
        assert!(st.handle_desktop_click(ExtWidgetId::Pomodoro, 2, &mut cfg));
        assert!(st.pomo_break);
        assert_eq!(st.pomo_rem_secs, 5 * 60);
        // Reset back to 25:00 focus
        st.pomo_rem_secs = 123;
        st.pomo_running = true;
        assert!(st.handle_desktop_click(ExtWidgetId::Pomodoro, 1, &mut cfg));
        assert!(!st.pomo_running);
        assert!(!st.pomo_break);
        assert_eq!(st.pomo_rem_secs, 25 * 60);
    }

    #[test]
    fn daily_quote_pool_size_and_midnight_rollover_change_every_day() {
        // 1. All 4 quote categories must have 31 distinct, non-empty quotes
        for pool in [&QUOTES_STOIC, &QUOTES_TECH, &QUOTES_ZEN, &QUOTES_LIT] {
            assert_eq!(pool.len(), DAILY_QUOTE_POOL_LEN);
            assert_eq!(pool.len(), 31);
            let mut seen = std::collections::HashSet::new();
            for &(q, author) in pool {
                assert!(!q.trim().is_empty());
                assert!(!author.trim().is_empty());
                assert!(seen.insert(q), "Duplicate quote found: {}", q);
            }
        }

        // 2. Every consecutive day across an entire year changes the quote index
        let mut prev_idx = daily_quote_index_for_date(2025, 12, 31);
        for m in 1..=12u32 {
            let dim = days_in_month(2026, m);
            for d in 1..=dim {
                let idx = daily_quote_index_for_date(2026, m, d);
                assert_ne!(
                    idx, prev_idx,
                    "Quote index must change on consecutive date 2026-{:02}-{:02}",
                    m, d
                );
                prev_idx = idx;
            }
        }

        // 3. Midnight rollover automatically updates quote_idx and signals repaint
        let mut st = ExtRuntimeState::new();
        st.quote_date_key = (2026, 11, 24);
        st.quote_idx = daily_quote_index_for_date(2026, 11, 24);
        assert!(!st.sync_daily_quote_for_date(2026, 11, 24));
        let idx_day1 = st.quote_idx;
        assert!(st.sync_daily_quote_for_date(2026, 11, 25));
        assert_ne!(st.quote_idx, idx_day1);
        assert_eq!(st.quote_date_key, (2026, 11, 25));

        // 4. Manual click still cycles across all 31 quotes cleanly
        let mut cfg = AppConfig::default();
        let before = st.quote_idx;
        assert!(st.handle_desktop_click(ExtWidgetId::Greeting, 0, &mut cfg));
        assert_eq!(st.quote_idx, (before + 1) % DAILY_QUOTE_POOL_LEN);
    }

    #[test]
    fn world_clock_48_cities_lookup_ini_roundtrip_and_slot_clicks() {
        assert_eq!(WORLD_CLOCK_CITIES.len(), 48);
        let mut names = std::collections::HashSet::new();
        for c in WORLD_CLOCK_CITIES {
            assert!(!c.name.is_empty());
            assert!(!c.tz_abbr.is_empty());
            assert!(names.insert(c.name.to_ascii_lowercase()), "Duplicate city: {}", c.name);
        }

        // Case-insensitive lookup + offset formatting + unknown fallback to UTC
        let blr = resolve_world_clock_city("bengaluru");
        assert_eq!(blr.name, "Bengaluru");
        assert_eq!(blr.offset_mins, 330);
        assert_eq!(format_utc_offset(blr.offset_mins), "UTC+5:30");
        assert_eq!(format_utc_offset(-300), "UTC-5");
        assert_eq!(format_utc_offset(345), "UTC+5:45");
        assert_eq!(resolve_world_clock_city("Atlantis").name, "UTC");

        // INI save/load roundtrip preserves user-chosen cities
        let mut cfg = AppConfig::default();
        cfg.world_clock_cities = [
            "Bengaluru".into(),
            "Tokyo".into(),
            "San Francisco".into(),
            "Zurich".into(),
        ];
        let ini = crate::config::serialize_config_ini(&cfg);
        assert!(ini.contains("world_clock_cities=Bengaluru|Tokyo|San Francisco|Zurich"));
        let parsed = crate::config::parse_config_ini(&ini).expect("INI must parse");
        assert_eq!(parsed.world_clock_cities, cfg.world_clock_cities);

        // Desktop clicking a city row cycles that slot cleanly without colliding with other slots
        let mut st = ExtRuntimeState::new();
        let before0 = cfg.world_clock_cities[0].clone();
        assert!(st.handle_desktop_click(ExtWidgetId::WorldClock, 10, &mut cfg));
        assert_ne!(cfg.world_clock_cities[0], before0);
        assert!(
            !cfg.world_clock_cities[1..]
                .iter()
                .any(|other| other.eq_ignore_ascii_case(&cfg.world_clock_cities[0])),
            "Cycling slot 0 must not duplicate another active slot"
        );
        // Clicking Cities pack button updates slots from preset pack
        assert!(st.handle_desktop_click(ExtWidgetId::WorldClock, 1, &mut cfg));
        assert_eq!(cfg.world_clock_cities[0], "San Francisco");
    }

    #[test]
    fn drives_and_processes_preview_state_and_hit_tests_match_all_modes() {
        // 1. Studio preview state must be pre-populated with non-empty drives, processes & bin
        let preview = ExtRuntimeState::new_studio_preview();
        assert!(preview.drives.len() >= 2, "Preview must contain sample drives");
        assert!(preview.top_procs.len() >= 3, "Preview must contain sample processes");
        assert!(preview.bin_items > 0 && preview.bin_bytes > 0);

        // 2. Drives hit-tests: Rings (horizontal) vs Bars/Table (vertical) + header pill
        let mut dcfg = ExtWidgetConfig::default_for(ExtWidgetId::Drives);
        dcfg.padding = 12;
        let (dlw, _) = dcfg.logical_size(ExtWidgetId::Drives);
        let dx1 = dlw - 12.0;
        assert_eq!(
            hit_test_ext_widget(ExtWidgetId::Drives, &dcfg, dx1 - 20.0, 14.0),
            WidgetHitZone::ExtBtn(ExtWidgetId::Drives, 4)
        );
        // Rings mode: left half = drive 0, right half = drive 1
        dcfg.mode_idx = 0;
        assert_eq!(
            hit_test_ext_widget(ExtWidgetId::Drives, &dcfg, 50.0, 65.0),
            WidgetHitZone::ExtBtn(ExtWidgetId::Drives, 0)
        );
        assert_eq!(
            hit_test_ext_widget(ExtWidgetId::Drives, &dcfg, 180.0, 65.0),
            WidgetHitZone::ExtBtn(ExtWidgetId::Drives, 1)
        );
        // Table mode: top row = drive 0, second row = drive 1 (even on left half!)
        dcfg.mode_idx = 3;
        assert_eq!(
            hit_test_ext_widget(ExtWidgetId::Drives, &dcfg, 50.0, 42.0),
            WidgetHitZone::ExtBtn(ExtWidgetId::Drives, 0)
        );
        assert_eq!(
            hit_test_ext_widget(ExtWidgetId::Drives, &dcfg, 50.0, 70.0),
            WidgetHitZone::ExtBtn(ExtWidgetId::Drives, 1)
        );

        // 3. Process Killer hit-tests: title area drags card, TaskMgr pill hits btn 4, × hits rows 0..=3
        let mut pcfg = ExtWidgetConfig::default_for(ExtWidgetId::Processes);
        pcfg.padding = 12;
        pcfg.mode_idx = 3; // 4 processes
        let (plw, plh) = pcfg.logical_size(ExtWidgetId::Processes);
        let px1 = plw - 12.0;
        assert_eq!(
            hit_test_ext_widget(ExtWidgetId::Processes, &pcfg, 40.0, 14.0),
            WidgetHitZone::ExtCard(ExtWidgetId::Processes),
            "Clicking Process Killer title must NOT open TaskMgr (must allow dragging)"
        );
        assert_eq!(
            hit_test_ext_widget(ExtWidgetId::Processes, &pcfg, px1 - 20.0, 14.0),
            WidgetHitZone::ExtBtn(ExtWidgetId::Processes, 4)
        );
        let y_body = 12.0 * 0.75 + 19.0 + 4.0;
        let row_h = ((plh - 6.0 - y_body) / 4.0).clamp(20.0, 28.0);
        for row in 0..4u8 {
            let cy = y_body + (row as f32 + 0.5) * row_h;
            assert_eq!(
                hit_test_ext_widget(ExtWidgetId::Processes, &pcfg, px1 - 10.0, cy),
                WidgetHitZone::ExtBtn(ExtWidgetId::Processes, row)
            );
        }
    }

    #[test]
    fn bg_telemetry_worker_filters_drives_and_publishes_non_blocking() {
        // 1. Drive filter must probe only local volumes (fixed/removable/RAM) and skip
        // disconnected network shares (4), optical drives (5), and unknown/unmounted roots (0/1).
        assert!(!should_probe_drive_type(0));
        assert!(!should_probe_drive_type(1));
        assert!(should_probe_drive_type(2));
        assert!(should_probe_drive_type(3));
        assert!(!should_probe_drive_type(4));
        assert!(!should_probe_drive_type(5));
        assert!(should_probe_drive_type(6));

        // 2. Request coalescing merges flags cleanly
        let mut r = BgTelemetryRequest {
            want_net: true,
            want_drives: false,
            want_procs: false,
            want_bin: false,
        };
        r.merge(BgTelemetryRequest {
            want_net: false,
            want_drives: true,
            want_procs: true,
            want_bin: false,
        });
        assert!(r.want_net && r.want_drives && r.want_procs && !r.want_bin);

        // 3. ExtRuntimeState::tick dispatches drives/procs to the reusable background worker
        // without blocking UI thread, then applies published snapshot on a subsequent tick.
        let mut st = ExtRuntimeState::new();
        assert!(st.bg_telemetry.is_none());
        let mut cfg = AppConfig::default();
        cfg.ext_widgets[ExtWidgetId::Drives.idx()].enabled = true;
        cfg.ext_widgets[ExtWidgetId::Processes.idx()].enabled = true;

        let t0 = Instant::now();
        let _ = st.tick(&cfg, 18.0);
        assert!(
            t0.elapsed() < Duration::from_millis(200),
            "ExtRuntimeState::tick must return non-blockingly without running OS probes on UI thread"
        );
        assert!(st.bg_telemetry.is_some(), "Reusable background telemetry worker must be spawned");

        let deadline = Instant::now() + Duration::from_secs(4);
        while Instant::now() < deadline {
            let _ = st.tick(&cfg, 18.0);
            if !st.drives.is_empty() && !st.top_procs.is_empty() {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(
            !st.drives.is_empty(),
            "Background telemetry worker must publish local drives snapshot"
        );
        assert!(
            !st.top_procs.is_empty(),
            "Background telemetry worker must publish top processes snapshot"
        );
    }
}
