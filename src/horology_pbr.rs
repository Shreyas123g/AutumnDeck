//! Photorealistic Haute Horlogerie PBR Material Bakers & 3D Direct2D Instrument Primitives.
//!
//! Provides 10 curated Photorealistic Horology themes (`HorologyTheme`), independent per-part
//! customization (`HorologyPartConfig`: Bezel/Case, Dial Face, Hands/Needles, Lume/Jewels,
//! Sapphire AR Glare, Bezel Screws, Cast Needle Shadows), lazy `512x256` per-pixel normal-mapped
//! PBR dial surface bakers (`OnceLock` + bounded LRU GPU cache), and reusable 3D-shaded Direct2D
//! watchmaking primitives (machined cases, octagonal/fluted/ceramic/diver bezels, recessed
//! azurage sub-dials, real-time cast-shadow hands, animated tourbillon escapements, live
//! astronomical moonphase discs, cyclops apertures, tactile crown pushers, and knurled sliders).

use std::cell::RefCell;
use std::collections::HashMap;
use std::f32::consts::PI;
use std::ffi::c_void;
use std::sync::OnceLock;

use windows::core::Interface;
use windows::Win32::Graphics::Direct2D::Common::{
    D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_COLOR_F, D2D1_FIGURE_BEGIN_FILLED,
    D2D1_FIGURE_BEGIN_HOLLOW, D2D1_FIGURE_END_CLOSED, D2D1_FIGURE_END_OPEN, D2D1_GRADIENT_STOP,
    D2D1_PIXEL_FORMAT, D2D_RECT_F, D2D_SIZE_F, D2D_SIZE_U,
};
use windows::Win32::Graphics::Direct2D::{
    ID2D1Bitmap, ID2D1BitmapBrush, ID2D1Factory, ID2D1Geometry, ID2D1Layer,
    ID2D1LinearGradientBrush, ID2D1PathGeometry, ID2D1RadialGradientBrush, ID2D1RenderTarget,
    D2D1_ANTIALIAS_MODE_PER_PRIMITIVE, D2D1_ARC_SEGMENT, D2D1_ARC_SIZE_LARGE,
    D2D1_ARC_SIZE_SMALL, D2D1_BITMAP_BRUSH_PROPERTIES, D2D1_BITMAP_INTERPOLATION_MODE_LINEAR,
    D2D1_BITMAP_PROPERTIES, D2D1_BRUSH_PROPERTIES, D2D1_ELLIPSE, D2D1_EXTEND_MODE_CLAMP,
    D2D1_GAMMA_2_2, D2D1_LAYER_OPTIONS_NONE, D2D1_LAYER_PARAMETERS,
    D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES, D2D1_RADIAL_GRADIENT_BRUSH_PROPERTIES,
    D2D1_ROUNDED_RECT, D2D1_SWEEP_DIRECTION_CLOCKWISE,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows_numerics::{Matrix3x2, Vector2};

const TEX_W: u32 = 512;
const TEX_H: u32 = 256;
const MAX_RT_CACHE_ENTRIES: usize = 16;

// ============================================================================
// 1. 10 Photorealistic Horology Themes & Per-Part Material Finishes
// ============================================================================

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
#[repr(usize)]
pub enum HorologyTheme {
    RoyalOakSteel = 0,
    PatekMoonphase = 1,
    TourbillonGold = 2,
    DaytonaCeramic = 3,
    SubmarinerEmerald = 4,
    IwcPilotTitanium = 5,
    BreguetPorcelain = 6,
    ForgedCarbonMoto = 7,
    WalnutRegulator = 8,
    SapphireObsidian = 9,
}

impl HorologyTheme {
    pub const COUNT: usize = 10;

    pub const ALL: [HorologyTheme; 10] = [
        HorologyTheme::RoyalOakSteel,
        HorologyTheme::PatekMoonphase,
        HorologyTheme::TourbillonGold,
        HorologyTheme::DaytonaCeramic,
        HorologyTheme::SubmarinerEmerald,
        HorologyTheme::IwcPilotTitanium,
        HorologyTheme::BreguetPorcelain,
        HorologyTheme::ForgedCarbonMoto,
        HorologyTheme::WalnutRegulator,
        HorologyTheme::SapphireObsidian,
    ];

    #[inline]
    pub const fn idx(self) -> usize {
        self as usize
    }

    #[inline]
    pub fn from_idx(i: usize) -> Option<Self> {
        Self::ALL.get(i).copied()
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            HorologyTheme::RoyalOakSteel => "horo_royal_oak",
            HorologyTheme::PatekMoonphase => "horo_patek_moon",
            HorologyTheme::TourbillonGold => "horo_tourbillon_gold",
            HorologyTheme::DaytonaCeramic => "horo_daytona_ceramic",
            HorologyTheme::SubmarinerEmerald => "horo_submariner_emerald",
            HorologyTheme::IwcPilotTitanium => "horo_iwc_titanium",
            HorologyTheme::BreguetPorcelain => "horo_breguet_porcelain",
            HorologyTheme::ForgedCarbonMoto => "horo_forged_carbon",
            HorologyTheme::WalnutRegulator => "horo_walnut_regulator",
            HorologyTheme::SapphireObsidian => "horo_sapphire_obsidian",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "horo_royal_oak" | "royal_oak" | "royal_oak_steel" => Some(HorologyTheme::RoyalOakSteel),
            "horo_patek_moon" | "patek_moon" | "patek_moonphase" => {
                Some(HorologyTheme::PatekMoonphase)
            }
            "horo_tourbillon_gold" | "tourbillon_gold" | "tourbillon" => {
                Some(HorologyTheme::TourbillonGold)
            }
            "horo_daytona_ceramic" | "daytona_ceramic" | "daytona" => {
                Some(HorologyTheme::DaytonaCeramic)
            }
            "horo_submariner_emerald" | "submariner_emerald" | "submariner" => {
                Some(HorologyTheme::SubmarinerEmerald)
            }
            "horo_iwc_titanium" | "iwc_titanium" | "pilot_titanium" => {
                Some(HorologyTheme::IwcPilotTitanium)
            }
            "horo_breguet_porcelain" | "breguet_porcelain" | "porcelain" => {
                Some(HorologyTheme::BreguetPorcelain)
            }
            "horo_forged_carbon" | "forged_carbon" | "forged_carbon_moto" => {
                Some(HorologyTheme::ForgedCarbonMoto)
            }
            "horo_walnut_regulator" | "walnut_regulator" => Some(HorologyTheme::WalnutRegulator),
            "horo_sapphire_obsidian" | "sapphire_obsidian" | "obsidian_stealth" => {
                Some(HorologyTheme::SapphireObsidian)
            }
            _ => None,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            HorologyTheme::RoyalOakSteel => "Royal Oak Guilloché Steel",
            HorologyTheme::PatekMoonphase => "Patek Midnight Moonphase",
            HorologyTheme::TourbillonGold => "Tourbillon Skeleton Gold",
            HorologyTheme::DaytonaCeramic => "Daytona Ceramic Tachymeter",
            HorologyTheme::SubmarinerEmerald => "Submariner Emerald Oyster",
            HorologyTheme::IwcPilotTitanium => "IWC Pilot Titanium",
            HorologyTheme::BreguetPorcelain => "Breguet Porcelain Enamel",
            HorologyTheme::ForgedCarbonMoto => "Forged Carbon Motorsport",
            HorologyTheme::WalnutRegulator => "Walnut Desk Regulator",
            HorologyTheme::SapphireObsidian => "Sapphire Obsidian Stealth",
        }
    }

    pub const fn subtitle(self) -> &'static str {
        match self {
            HorologyTheme::RoyalOakSteel => {
                "Octagonal 316L steel • 8 hex screws & tapisserie waffle dial"
            }
            HorologyTheme::PatekMoonphase => {
                "18k rose gold • Cobalt sunburst enamel & live lunar disc"
            }
            HorologyTheme::TourbillonGold => {
                "Exposed live brass escapement • Smoked sapphire & ruby pivot"
            }
            HorologyTheme::DaytonaCeramic => {
                "Matte ceramic tachymeter • Panda sub-dials & red sweep hands"
            }
            HorologyTheme::SubmarinerEmerald => {
                "Knurled diver bezel • Sunburst emerald & cyclops aperture"
            }
            HorologyTheme::IwcPilotTitanium => {
                "Bead-blasted Grade-5 Ti • Tritium green & blue AR crystal"
            }
            HorologyTheme::BreguetPorcelain => {
                "Grand Feu ivory porcelain • Fluted coin rim & blued hands"
            }
            HorologyTheme::ForgedCarbonMoto => {
                "Marbled forged carbon • Allen bolts & racing-orange telemetry"
            }
            HorologyTheme::WalnutRegulator => {
                "Cathedral black walnut • Machined solid brass bezel & glass"
            }
            HorologyTheme::SapphireObsidian => {
                "Smoked sapphire crystal • Côtes de Genève gunmetal & platinum"
            }
        }
    }

    pub const fn badge(self) -> &'static str {
        match self {
            HorologyTheme::RoyalOakSteel => "316L",
            HorologyTheme::PatekMoonphase => "18K",
            HorologyTheme::TourbillonGold => "Live Gear",
            HorologyTheme::DaytonaCeramic => "Tachy",
            HorologyTheme::SubmarinerEmerald => "Diver",
            HorologyTheme::IwcPilotTitanium => "Ti-5",
            HorologyTheme::BreguetPorcelain => "Enamel",
            HorologyTheme::ForgedCarbonMoto => "Carbon",
            HorologyTheme::WalnutRegulator => "Walnut",
            HorologyTheme::SapphireObsidian => "Sapphire",
        }
    }

    /// Only Tourbillon Skeleton Gold runs an ambient escapement gear animation while idle.
    /// All other 9 Horology themes idle at 0 FPS.
    #[inline]
    pub const fn has_idle_motion(self) -> bool {
        matches!(self, HorologyTheme::TourbillonGold)
    }

    #[inline]
    pub const fn is_light_dial(self) -> bool {
        matches!(self, HorologyTheme::BreguetPorcelain)
    }

    pub const fn default_bezel_finish(self) -> HorologyBezelFinish {
        match self {
            HorologyTheme::RoyalOakSteel => HorologyBezelFinish::BrushedSteel,
            HorologyTheme::PatekMoonphase => HorologyBezelFinish::RoseGold,
            HorologyTheme::TourbillonGold => HorologyBezelFinish::SolidBrass,
            HorologyTheme::DaytonaCeramic => HorologyBezelFinish::MatteCeramic,
            HorologyTheme::SubmarinerEmerald => HorologyBezelFinish::DiverKnurled,
            HorologyTheme::IwcPilotTitanium => HorologyBezelFinish::BeadTitanium,
            HorologyTheme::BreguetPorcelain => HorologyBezelFinish::FlutedCoin,
            HorologyTheme::ForgedCarbonMoto => HorologyBezelFinish::ForgedCarbon,
            HorologyTheme::WalnutRegulator => HorologyBezelFinish::SolidBrass,
            HorologyTheme::SapphireObsidian => HorologyBezelFinish::Platinum,
        }
    }

    pub const fn default_dial_finish(self) -> HorologyDialFinish {
        match self {
            HorologyTheme::RoyalOakSteel => HorologyDialFinish::TapisserieGuilloche,
            HorologyTheme::PatekMoonphase => HorologyDialFinish::SunburstEnamel,
            HorologyTheme::TourbillonGold => HorologyDialFinish::SmokedSkeleton,
            HorologyTheme::DaytonaCeramic => HorologyDialFinish::MatteAviation,
            HorologyTheme::SubmarinerEmerald => HorologyDialFinish::SunburstEnamel,
            HorologyTheme::IwcPilotTitanium => HorologyDialFinish::MatteAviation,
            HorologyTheme::BreguetPorcelain => HorologyDialFinish::PorcelainIvory,
            HorologyTheme::ForgedCarbonMoto => HorologyDialFinish::ForgedCarbonFlake,
            HorologyTheme::WalnutRegulator => HorologyDialFinish::CathedralWalnut,
            HorologyTheme::SapphireObsidian => HorologyDialFinish::CotesDeGeneve,
        }
    }

    /// Default (bezel_rgb, dial_rgb, hands_rgb, lume_rgb, jewel_rgb) for each Horology theme.
    pub const fn default_colors(self) -> (u32, u32, u32, u32, u32) {
        match self {
            HorologyTheme::RoyalOakSteel => (0xCFD8E3, 0x152746, 0xF4F8FF, 0x7CE0FF, 0xD8244E),
            HorologyTheme::PatekMoonphase => (0xE8AC78, 0x0E1F44, 0xFCE0B0, 0xF7C972, 0xDE2452),
            HorologyTheme::TourbillonGold => (0xE2BC5E, 0x10131A, 0xFFE082, 0xF7BE4E, 0xF02454),
            HorologyTheme::DaytonaCeramic => (0x1B1E26, 0x0D0F14, 0xF4F7FC, 0xF23044, 0xF23044),
            HorologyTheme::SubmarinerEmerald => (0xD2DBE5, 0x0B543C, 0xF4FCF8, 0x5CF2E0, 0x32F2B4),
            HorologyTheme::IwcPilotTitanium => (0x6B7480, 0x14191D, 0xEEF5FA, 0x8EF56C, 0xFF7426),
            HorologyTheme::BreguetPorcelain => (0xCFD7E2, 0xFAF5E8, 0x16428A, 0x1B4EA0, 0xC8224A),
            HorologyTheme::ForgedCarbonMoto => (0x20232B, 0x0E1015, 0xF2F6FA, 0xFF5E1A, 0xFF5E1A),
            HorologyTheme::WalnutRegulator => (0xDFB452, 0x3D210E, 0xFFF3D6, 0xE8BE5E, 0xCC3B2E),
            HorologyTheme::SapphireObsidian => (0x8FD8FF, 0x0B1528, 0xECF6FF, 0x4BE8FF, 0x9C78FF),
        }
    }

    pub const fn default_fonts(self) -> (&'static str, &'static str, &'static str, &'static str) {
        match self {
            HorologyTheme::RoyalOakSteel => {
                ("Bahnschrift SemiBold", "Georgia", "Bahnschrift Bold", "SF Pro Display")
            }
            HorologyTheme::PatekMoonphase => {
                ("Bodoni MT", "Edwardian Script ITC", "Georgia", "Palatino Linotype")
            }
            HorologyTheme::TourbillonGold => {
                ("Bodoni MT", "Georgia", "Bahnschrift SemiBold", "Palatino Linotype")
            }
            HorologyTheme::DaytonaCeramic => {
                ("Bahnschrift Bold", "SF Pro Display", "Bahnschrift Bold", "Cascadia Mono")
            }
            HorologyTheme::SubmarinerEmerald => {
                ("Bahnschrift SemiBold", "SF Pro Display", "Bahnschrift Bold", "SF Pro Display")
            }
            HorologyTheme::IwcPilotTitanium => {
                ("Bahnschrift Bold", "Cascadia Mono", "Bahnschrift Bold", "Cascadia Mono")
            }
            HorologyTheme::BreguetPorcelain => {
                ("Bodoni MT", "Edwardian Script ITC", "Georgia", "Palatino Linotype")
            }
            HorologyTheme::ForgedCarbonMoto => {
                ("Bahnschrift Bold", "Cascadia Mono", "Bahnschrift Bold", "Cascadia Mono")
            }
            HorologyTheme::WalnutRegulator => {
                ("Bodoni MT", "Georgia", "Segoe UI Semibold", "Palatino Linotype")
            }
            HorologyTheme::SapphireObsidian => {
                ("SF Pro Display", "Segoe UI Light", "Bahnschrift SemiBold", "Cascadia Mono")
            }
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
#[repr(u8)]
pub enum HorologyBezelFinish {
    BrushedSteel = 0,
    RoseGold = 1,
    MatteCeramic = 2,
    DiverKnurled = 3,
    BeadTitanium = 4,
    FlutedCoin = 5,
    ForgedCarbon = 6,
    SolidBrass = 7,
    Platinum = 8,
}

impl HorologyBezelFinish {
    pub const ALL: [HorologyBezelFinish; 9] = [
        HorologyBezelFinish::BrushedSteel,
        HorologyBezelFinish::RoseGold,
        HorologyBezelFinish::MatteCeramic,
        HorologyBezelFinish::DiverKnurled,
        HorologyBezelFinish::BeadTitanium,
        HorologyBezelFinish::FlutedCoin,
        HorologyBezelFinish::ForgedCarbon,
        HorologyBezelFinish::SolidBrass,
        HorologyBezelFinish::Platinum,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            HorologyBezelFinish::BrushedSteel => "steel",
            HorologyBezelFinish::RoseGold => "rose_gold",
            HorologyBezelFinish::MatteCeramic => "ceramic",
            HorologyBezelFinish::DiverKnurled => "diver",
            HorologyBezelFinish::BeadTitanium => "titanium",
            HorologyBezelFinish::FlutedCoin => "fluted",
            HorologyBezelFinish::ForgedCarbon => "carbon",
            HorologyBezelFinish::SolidBrass => "brass",
            HorologyBezelFinish::Platinum => "platinum",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "steel" | "brushed_steel" => Some(HorologyBezelFinish::BrushedSteel),
            "rose_gold" | "rosegold" => Some(HorologyBezelFinish::RoseGold),
            "ceramic" | "matte_ceramic" => Some(HorologyBezelFinish::MatteCeramic),
            "diver" | "diver_knurled" => Some(HorologyBezelFinish::DiverKnurled),
            "titanium" | "bead_titanium" => Some(HorologyBezelFinish::BeadTitanium),
            "fluted" | "fluted_coin" => Some(HorologyBezelFinish::FlutedCoin),
            "carbon" | "forged_carbon" => Some(HorologyBezelFinish::ForgedCarbon),
            "brass" | "solid_brass" => Some(HorologyBezelFinish::SolidBrass),
            "platinum" => Some(HorologyBezelFinish::Platinum),
            _ => None,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            HorologyBezelFinish::BrushedSteel => "316L Steel",
            HorologyBezelFinish::RoseGold => "Rose Gold",
            HorologyBezelFinish::MatteCeramic => "Ceramic",
            HorologyBezelFinish::DiverKnurled => "Diver",
            HorologyBezelFinish::BeadTitanium => "Titanium",
            HorologyBezelFinish::FlutedCoin => "Fluted",
            HorologyBezelFinish::ForgedCarbon => "Carbon",
            HorologyBezelFinish::SolidBrass => "Brass",
            HorologyBezelFinish::Platinum => "Platinum",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
#[repr(usize)]
pub enum HorologyDialFinish {
    TapisserieGuilloche = 0,
    SunburstEnamel = 1,
    CotesDeGeneve = 2,
    PorcelainIvory = 3,
    ForgedCarbonFlake = 4,
    MatteAviation = 5,
    CathedralWalnut = 6,
    SmokedSkeleton = 7,
}

impl HorologyDialFinish {
    pub const COUNT: usize = 8;

    pub const ALL: [HorologyDialFinish; 8] = [
        HorologyDialFinish::TapisserieGuilloche,
        HorologyDialFinish::SunburstEnamel,
        HorologyDialFinish::CotesDeGeneve,
        HorologyDialFinish::PorcelainIvory,
        HorologyDialFinish::ForgedCarbonFlake,
        HorologyDialFinish::MatteAviation,
        HorologyDialFinish::CathedralWalnut,
        HorologyDialFinish::SmokedSkeleton,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            HorologyDialFinish::TapisserieGuilloche => "tapisserie",
            HorologyDialFinish::SunburstEnamel => "sunburst",
            HorologyDialFinish::CotesDeGeneve => "geneve",
            HorologyDialFinish::PorcelainIvory => "porcelain",
            HorologyDialFinish::ForgedCarbonFlake => "carbon_flake",
            HorologyDialFinish::MatteAviation => "aviation",
            HorologyDialFinish::CathedralWalnut => "walnut",
            HorologyDialFinish::SmokedSkeleton => "skeleton",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "tapisserie" | "guilloche" => Some(HorologyDialFinish::TapisserieGuilloche),
            "sunburst" | "enamel" => Some(HorologyDialFinish::SunburstEnamel),
            "geneve" | "cotes" => Some(HorologyDialFinish::CotesDeGeneve),
            "porcelain" | "ivory" => Some(HorologyDialFinish::PorcelainIvory),
            "carbon_flake" | "carbon" => Some(HorologyDialFinish::ForgedCarbonFlake),
            "aviation" | "matte" => Some(HorologyDialFinish::MatteAviation),
            "walnut" | "wood" => Some(HorologyDialFinish::CathedralWalnut),
            "skeleton" | "smoked" => Some(HorologyDialFinish::SmokedSkeleton),
            _ => None,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            HorologyDialFinish::TapisserieGuilloche => "Guilloché",
            HorologyDialFinish::SunburstEnamel => "Sunburst",
            HorologyDialFinish::CotesDeGeneve => "Genève",
            HorologyDialFinish::PorcelainIvory => "Porcelain",
            HorologyDialFinish::ForgedCarbonFlake => "Carbon",
            HorologyDialFinish::MatteAviation => "Aviation",
            HorologyDialFinish::CathedralWalnut => "Walnut",
            HorologyDialFinish::SmokedSkeleton => "Skeleton",
        }
    }
}

/// Independent per-widget Horology material & hardware customization config.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct HorologyPartConfig {
    pub bezel_finish: Option<HorologyBezelFinish>,
    pub bezel_rgb: Option<u32>,
    pub dial_finish: Option<HorologyDialFinish>,
    pub dial_rgb: Option<u32>,
    pub hands_rgb: Option<u32>,
    pub lume_rgb: Option<u32>,
    pub glare_pct: u8,
    pub show_screws: bool,
    pub show_needle_shadows: bool,
}

impl Default for HorologyPartConfig {
    fn default() -> Self {
        Self {
            bezel_finish: None,
            bezel_rgb: None,
            dial_finish: None,
            dial_rgb: None,
            hands_rgb: None,
            lume_rgb: None,
            glare_pct: 75,
            show_screws: true,
            show_needle_shadows: true,
        }
    }
}

impl HorologyPartConfig {
    pub fn is_default(self) -> bool {
        self == Self::default()
    }

    pub fn to_csv(self) -> String {
        let fmt_opt_rgb = |opt: Option<u32>| match opt {
            Some(c) => format!("0x{:06X}", c & 0x00FF_FFFF),
            None => "-".to_string(),
        };
        format!(
            "{},{},{},{},{},{},{},{},{}",
            self.bezel_finish.map(|f| f.as_str()).unwrap_or("-"),
            fmt_opt_rgb(self.bezel_rgb),
            self.dial_finish.map(|f| f.as_str()).unwrap_or("-"),
            fmt_opt_rgb(self.dial_rgb),
            fmt_opt_rgb(self.hands_rgb),
            fmt_opt_rgb(self.lume_rgb),
            self.glare_pct.min(100),
            if self.show_screws { 1 } else { 0 },
            if self.show_needle_shadows { 1 } else { 0 },
        )
    }

    pub fn from_csv(s: &str) -> Self {
        let mut out = Self::default();
        let parts: Vec<&str> = s.split(',').collect();
        if parts.is_empty() {
            return out;
        }
        let parse_rgb = |raw: Option<&&str>| -> Option<u32> {
            let t = raw?.trim();
            if t.is_empty() || t == "-" || t.eq_ignore_ascii_case("none") {
                return None;
            }
            let hex = t.trim_start_matches("0x").trim_start_matches("0X");
            u32::from_str_radix(hex, 16).ok().map(|c| c & 0x00FF_FFFF)
        };
        if let Some(bf) = parts.first().and_then(|v| HorologyBezelFinish::from_str(v)) {
            out.bezel_finish = Some(bf);
        }
        out.bezel_rgb = parse_rgb(parts.get(1));
        if let Some(df) = parts.get(2).and_then(|v| HorologyDialFinish::from_str(v)) {
            out.dial_finish = Some(df);
        }
        out.dial_rgb = parse_rgb(parts.get(3));
        out.hands_rgb = parse_rgb(parts.get(4));
        out.lume_rgb = parse_rgb(parts.get(5));
        if let Some(g) = parts.get(6).and_then(|v| v.trim().parse::<u8>().ok()) {
            out.glare_pct = g.min(100);
        }
        if let Some(sc) = parts.get(7) {
            out.show_screws = sc.trim() != "0";
        }
        if let Some(sh) = parts.get(8) {
            out.show_needle_shadows = sh.trim() != "0";
        }
        out
    }

    /// Resolve final per-part colors, finishes, and optical flags for rendering.
    pub fn resolve(
        self,
        theme: HorologyTheme,
        custom_accent: Option<u32>,
        custom_text: Option<u32>,
        custom_bg: Option<u32>,
    ) -> ResolvedHorologyPalette {
        let (def_bez, def_dial, def_hands, def_lume, def_jewel) = theme.default_colors();
        let bezel_finish = self.bezel_finish.unwrap_or_else(|| theme.default_bezel_finish());
        let dial_finish = self.dial_finish.unwrap_or_else(|| theme.default_dial_finish());
        let bezel_rgb = self.bezel_rgb.unwrap_or(def_bez);
        let dial_rgb = self.dial_rgb.or(custom_bg).unwrap_or(def_dial);
        let hands_rgb = self.hands_rgb.or(custom_text).unwrap_or(def_hands);
        let lume_rgb = self.lume_rgb.or(custom_accent).unwrap_or(def_lume);

        let is_light_dial = rgb_luma(dial_rgb) > 0.56
            || (self.dial_rgb.is_none()
                && custom_bg.is_none()
                && matches!(dial_finish, HorologyDialFinish::PorcelainIvory));

        let text_rgb = if let Some(ct) = custom_text {
            ct
        } else if is_light_dial {
            0x18202C
        } else {
            0xF2F6FC
        };
        let subtext_rgb = if is_light_dial { 0x4E5B6E } else { 0xA8B6CA };

        ResolvedHorologyPalette {
            theme,
            bezel_finish,
            dial_finish,
            bezel_rgb,
            bezel_hi_rgb: lighten_rgb(bezel_rgb, 0.38),
            bezel_lo_rgb: darken_rgb(bezel_rgb, 0.48),
            dial_rgb,
            dial_hi_rgb: lighten_rgb(dial_rgb, 0.16),
            dial_lo_rgb: darken_rgb(dial_rgb, 0.42),
            hands_rgb,
            lume_rgb,
            text_rgb,
            subtext_rgb,
            jewel_rgb: def_jewel,
            is_light_dial,
            glare_alpha: (self.glare_pct as f32 / 100.0).clamp(0.0, 1.0),
            show_screws: self.show_screws,
            show_needle_shadows: self.show_needle_shadows,
        }
    }
}

/// Curated swatches exposed in Studio's Horology Parts & Materials UI.
pub const HOROLOGY_BEZEL_SWATCHES: [u32; 8] = [
    0xC8D0DA, // 316L Steel
    0xDDA674, // 18k Rose Gold
    0xD8B35B, // Yellow Gold / Brass
    0x242830, // Matte Ceramic
    0x7D8692, // Bead Titanium
    0xBCC6D4, // Platinum
    0x3A2414, // Aged Bronze
    0x1B1D22, // Forged Carbon
];

pub const HOROLOGY_DIAL_SWATCHES: [u32; 8] = [
    0x162640, // Royal Slate Blue
    0x0C1A36, // Midnight Cobalt
    0x093A2B, // Emerald Sunburst
    0x12151C, // Obsidian Black
    0xF2EEE4, // Grand Feu Ivory
    0x26170B, // Cathedral Walnut
    0x33121E, // Oxblood Enamel
    0x1E262E, // Gunmetal Slate
];

pub const HOROLOGY_ACCENT_SWATCHES: [u32; 8] = [
    0x7CE0FF, // Ice Chromalight
    0xF5C26B, // Champagne Gold
    0xE63946, // Chrono Red
    0x95F279, // Tritium Green
    0xFF5E1A, // Racing Orange
    0x1D4E8C, // Heat-Blued Steel
    0xF2F6FC, // Rhodium White
    0xA882FF, // Sapphire Violet
];

#[derive(Clone, Copy, Debug)]
pub struct ResolvedHorologyPalette {
    pub theme: HorologyTheme,
    pub bezel_finish: HorologyBezelFinish,
    pub dial_finish: HorologyDialFinish,
    pub bezel_rgb: u32,
    pub bezel_hi_rgb: u32,
    pub bezel_lo_rgb: u32,
    pub dial_rgb: u32,
    pub dial_hi_rgb: u32,
    pub dial_lo_rgb: u32,
    pub hands_rgb: u32,
    pub lume_rgb: u32,
    pub text_rgb: u32,
    pub subtext_rgb: u32,
    pub jewel_rgb: u32,
    pub is_light_dial: bool,
    pub glare_alpha: f32,
    pub show_screws: bool,
    pub show_needle_shadows: bool,
}

// ============================================================================
// 2. Color & Noise Math Helpers
// ============================================================================

#[inline]
pub fn unpack_rgb(rgb: u32) -> (u8, u8, u8) {
    (((rgb >> 16) & 0xFF) as u8, ((rgb >> 8) & 0xFF) as u8, (rgb & 0xFF) as u8)
}

#[inline]
pub fn rgb_u32(rgb: u32, a: f32) -> D2D1_COLOR_F {
    let (r, g, b) = unpack_rgb(rgb);
    D2D1_COLOR_F {
        r: r as f32 / 255.0,
        g: g as f32 / 255.0,
        b: b as f32 / 255.0,
        a: a.clamp(0.0, 1.0),
    }
}

#[inline]
fn rgba(r: u8, g: u8, b: u8, a: f32) -> D2D1_COLOR_F {
    D2D1_COLOR_F {
        r: r as f32 / 255.0,
        g: g as f32 / 255.0,
        b: b as f32 / 255.0,
        a: a.clamp(0.0, 1.0),
    }
}

#[inline]
fn rgb_luma(rgb: u32) -> f32 {
    let (r, g, b) = unpack_rgb(rgb);
    (0.2126 * r as f32 + 0.7152 * g as f32 + 0.0722 * b as f32) / 255.0
}

#[inline]
pub fn lighten_rgb(rgb: u32, amt: f32) -> u32 {
    let (r, g, b) = unpack_rgb(rgb);
    let k = amt.clamp(0.0, 1.0);
    let rr = (r as f32 + (255.0 - r as f32) * k).round().clamp(0.0, 255.0) as u32;
    let gg = (g as f32 + (255.0 - g as f32) * k).round().clamp(0.0, 255.0) as u32;
    let bb = (b as f32 + (255.0 - b as f32) * k).round().clamp(0.0, 255.0) as u32;
    (rr << 16) | (gg << 8) | bb
}

#[inline]
pub fn darken_rgb(rgb: u32, amt: f32) -> u32 {
    let (r, g, b) = unpack_rgb(rgb);
    let k = 1.0 - amt.clamp(0.0, 1.0);
    let rr = (r as f32 * k).round().clamp(0.0, 255.0) as u32;
    let gg = (g as f32 * k).round().clamp(0.0, 255.0) as u32;
    let bb = (b as f32 * k).round().clamp(0.0, 255.0) as u32;
    (rr << 16) | (gg << 8) | bb
}

#[inline]
fn hash21(ix: i32, iy: i32, seed: u32) -> f32 {
    let mut n = (ix.wrapping_mul(374_761_393))
        .wrapping_add(iy.wrapping_mul(668_265_263))
        .wrapping_add((seed as i32).wrapping_mul(1_442_695_041)) as u32;
    n = (n ^ (n >> 13)).wrapping_mul(1_274_126_177);
    n ^= n >> 16;
    (n & 0x00FF_FFFF) as f32 / 16_777_215.0
}

#[inline]
fn smoothstep(t: f32) -> f32 {
    let c = t.clamp(0.0, 1.0);
    c * c * (3.0 - 2.0 * c)
}

#[inline]
fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

#[inline]
fn value_noise(x: f32, y: f32, seed: u32) -> f32 {
    let ix = x.floor() as i32;
    let iy = y.floor() as i32;
    let fx = smoothstep(x - ix as f32);
    let fy = smoothstep(y - iy as f32);
    let a = hash21(ix, iy, seed);
    let b = hash21(ix + 1, iy, seed);
    let c = hash21(ix, iy + 1, seed);
    let d = hash21(ix + 1, iy + 1, seed);
    lerp(lerp(a, b, fx), lerp(c, d, fx), fy)
}

fn fbm(mut x: f32, mut y: f32, seed: u32, octaves: usize) -> f32 {
    let mut sum = 0.0f32;
    let mut amp = 0.5f32;
    let mut norm = 0.0f32;
    for o in 0..octaves {
        sum += amp * value_noise(x, y, seed.wrapping_add(o as u32 * 41));
        norm += amp;
        x = x * 2.03 + 13.1;
        y = y * 2.01 + 7.7;
        amp *= 0.5;
    }
    if norm > 0.0 { sum / norm } else { 0.5 }
}

fn voronoi_flake(x: f32, y: f32, seed: u32) -> (f32, f32, f32) {
    let ix = x.floor() as i32;
    let iy = y.floor() as i32;
    let fx = x - ix as f32;
    let fy = y - iy as f32;
    let mut d1 = 9.0f32;
    let mut d2 = 9.0f32;
    let mut cell_h = 0.5f32;
    for oy in -1..=1 {
        for ox in -1..=1 {
            let rx = hash21(ix + ox, iy + oy, seed);
            let ry = hash21(ix + ox, iy + oy, seed.wrapping_add(91_813));
            let dx = ox as f32 + rx - fx;
            let dy = oy as f32 + ry - fy;
            let d = (dx * dx + dy * dy).sqrt();
            if d < d1 {
                d2 = d1;
                d1 = d;
                cell_h = hash21(ix + ox, iy + oy, seed.wrapping_add(555));
            } else if d < d2 {
                d2 = d;
            }
        }
    }
    (d1, d2, cell_h)
}

#[inline]
fn put_px(buf: &mut [u8], x: u32, y: u32, r: f32, g: f32, b: f32, a: f32) {
    let idx = ((y * TEX_W + x) * 4) as usize;
    let af = a.clamp(0.0, 1.0);
    buf[idx] = (b.clamp(0.0, 1.0) * af * 255.0).round() as u8;
    buf[idx + 1] = (g.clamp(0.0, 1.0) * af * 255.0).round() as u8;
    buf[idx + 2] = (r.clamp(0.0, 1.0) * af * 255.0).round() as u8;
    buf[idx + 3] = (af * 255.0).round() as u8;
}

// ============================================================================
// 3. Per-Pixel Normal-Mapped 512x256 Horology Dial PBR Surface Bakers
// ============================================================================

/// Bakes an optical lighting/relief overlay (normal-mapped micro-geometry, specular highlights,
/// and ambient occlusion) for each `HorologyDialFinish`. Rendered cleanly over the resolved dial
/// color so theme defaults AND custom user dial colors both preserve crisp 3D photorealistic grain.
fn bake_dial_finish_bgra(finish: HorologyDialFinish) -> Vec<u8> {
    let mut buf = vec![0u8; (TEX_W * TEX_H * 4) as usize];
    let wf = TEX_W as f32;
    let hf = TEX_H as f32;

    for py in 0..TEX_H {
        let v = py as f32 / (hf - 1.0);
        for px in 0..TEX_W {
            let u = px as f32 / (wf - 1.0);
            let (r, g, b, a) = match finish {
                HorologyDialFinish::TapisserieGuilloche => {
                    // Grande Tapisserie waffle pyramids with 135° directional facet lighting + fine hairlines
                    let cols = 32.0f32;
                    let rows = 16.0f32;
                    let gx = (u * cols).fract();
                    let gy = (v * rows).fract();
                    let groove = gx < 0.10 || gy < 0.10 || gx > 0.94 || gy > 0.94;
                    let grain = value_noise(u * 260.0, v * 36.0, 101) * 0.08;
                    if groove {
                        // Deep recessed micro-channel between clous de Paris squares
                        (0.04, 0.06, 0.10, 0.56)
                    } else {
                        // Faceted truncated pyramid bevel: bright top-left, shaded bottom-right
                        let tl_bevel = ((0.24 - gx).max(0.0) + (0.24 - gy).max(0.0)) * 2.8;
                        let br_bevel = ((gx - 0.76).max(0.0) + (gy - 0.76).max(0.0)) * 2.8;
                        if tl_bevel > 0.06 {
                            let h = (0.76 + tl_bevel * 0.24 + grain).min(1.0);
                            (h, h, (h * 1.03).min(1.0), 0.34)
                        } else if br_bevel > 0.06 {
                            (0.03, 0.04, 0.07, (0.38 + br_bevel * 0.28).min(0.62))
                        } else {
                            // Top plateau with subtle directional metallic satin sheen
                            let sheen = 0.65 + (1.0 - (u * 0.6 + v * 0.4)) * 0.28 + grain;
                            (sheen, sheen, sheen, 0.13)
                        }
                    }
                }
                HorologyDialFinish::SunburstEnamel => {
                    // Radial anisotropic metallic sunburst rays + butterfly specular lobes
                    let dx = (u - 0.36) * 1.5;
                    let dy = v - 0.5;
                    let dist = (dx * dx + dy * dy).sqrt();
                    let ang = dy.atan2(dx);
                    // Twin butterfly specular lobes (real brushed sunburst lacquer physics)
                    let lobe1 = ((ang + 0.78).cos()).abs().powf(3.6);
                    let lobe2 = ((ang - 0.78).cos()).abs().powf(5.0) * 0.55;
                    let micro_ray =
                        (ang * 140.0).sin() * 0.5 + 0.5 + value_noise(ang * 36.0, dist * 14.0, 211) * 0.5;
                    let ray = (micro_ray * 0.45).clamp(0.0, 1.0);
                    let spec = (lobe1 * 0.58 + lobe2 * 0.34) * (0.55 + ray * 0.45);
                    let vig = (dist * 0.95).clamp(0.0, 1.0).powf(2.0);
                    if spec > 0.22 {
                        let s = (0.78 + spec * 0.22).min(1.0);
                        (s, s, s, (spec * 0.44).min(0.42))
                    } else {
                        (0.02, 0.03, 0.06, (vig * 0.46 + (1.0 - ray) * 0.10).min(0.55))
                    }
                }
                HorologyDialFinish::CotesDeGeneve => {
                    // Authentic Swiss Côtes de Genève vertical brushed movement ribs + perlage border
                    let stripe_w = 18.0f32;
                    let sx = (u * stripe_w).fract();
                    let rib_arc = ((sx - 0.5) * PI).cos().max(0.0);
                    // Curved polishing striations inside each Geneva rib
                    let striation = ((v * 22.0 + (sx - 0.5) * 2.4).sin() * 0.5 + 0.5) * 0.45
                        + value_noise(u * 140.0, v * 90.0, 307) * 0.25;
                    if sx < 0.055 {
                        // Crisp bright edge catchlight on left bevel of stripe
                        (0.92, 0.95, 1.0, 0.34)
                    } else if sx > 0.945 {
                        // Crisp dark valley shadow on right bevel of stripe
                        (0.02, 0.03, 0.05, 0.54)
                    } else {
                        let lum = rib_arc * 0.65 + striation * 0.35;
                        if lum > 0.52 {
                            (0.86, 0.90, 0.96, ((lum - 0.52) * 0.48).min(0.28))
                        } else {
                            (0.03, 0.04, 0.06, ((0.52 - lum) * 0.52).min(0.32))
                        }
                    }
                }
                HorologyDialFinish::PorcelainIvory => {
                    // Grand Feu kiln-fired warm porcelain enamel glaze (subtle organic depth + dome)
                    let dx = (u - 0.42) * 1.15;
                    let dy = (v - 0.38) * 1.15;
                    let d = (dx * dx + dy * dy).sqrt();
                    let micro = fbm(u * 18.0, v * 9.0, 409, 3) * 0.06;
                    if d < 0.45 {
                        let glow = (1.0 - d / 0.45).powf(1.8);
                        (1.0, 0.99, 0.96, (glow * 0.26 + micro).min(0.32))
                    } else {
                        let shade = ((d - 0.45) * 1.4).clamp(0.0, 1.0).powf(1.7);
                        (0.52, 0.46, 0.38, (shade * 0.22 + micro * 0.5).min(0.28))
                    }
                }
                HorologyDialFinish::ForgedCarbonFlake => {
                    // Marbled forged carbon composite: Voronoi shards with randomized anisotropic fiber angle
                    let (d1, d2, ch) = voronoi_flake(u * 14.0, v * 7.5, 521);
                    let edge = smoothstep((d2 - d1) * 7.0);
                    let ang = ch * PI * 2.0;
                    let fiber = ((u * 95.0 * ang.cos() + v * 55.0 * ang.sin()).sin() * 0.5 + 0.5)
                        * 0.55
                        + value_noise(u * 60.0 + ch * 11.0, v * 35.0, 523) * 0.45;
                    // Incident 135° light reflection depends on each shard's fiber orientation
                    let spec = ((ang - 0.78).cos().abs()).powf(2.4) * fiber;
                    if edge < 0.22 {
                        (0.02, 0.02, 0.03, 0.52)
                    } else if spec > 0.36 {
                        let s = (0.72 + spec * 0.28).min(1.0);
                        (s, s, (s * 1.04).min(1.0), (spec * 0.36).min(0.38))
                    } else {
                        (0.03, 0.03, 0.05, ((1.0 - spec) * 0.38).min(0.45))
                    }
                }
                HorologyDialFinish::MatteAviation => {
                    // Bead-blasted Grade-5 matte micro-stipple + subtle cockpit radial vignette
                    let grain = hash21(px as i32, py as i32, 613) * 0.55
                        + value_noise(u * 180.0, v * 90.0, 617) * 0.45;
                    let dx = (u - 0.5) * 1.2;
                    let dy = (v - 0.5) * 1.2;
                    let vig = (dx * dx + dy * dy).sqrt().clamp(0.0, 1.0).powf(2.0);
                    if grain > 0.62 {
                        (0.84, 0.88, 0.94, ((grain - 0.62) * 0.26).min(0.14))
                    } else {
                        (0.02, 0.03, 0.04, (vig * 0.36 + (0.62 - grain) * 0.14).min(0.46))
                    }
                }
                HorologyDialFinish::CathedralWalnut => {
                    // Oiled cathedral-arch black walnut woodgrain with open pores + warm satin sheen
                    let warp = fbm(u * 3.8, v * 2.2, 701, 4) * 0.55;
                    let ring_x = (u - 0.48) * 1.6;
                    let ring_y = (v - 0.52) * 4.4;
                    let ring_d = (ring_x * ring_x + ring_y * ring_y).sqrt() + warp;
                    let rings = ((ring_d * 26.0).sin() * 0.5 + 0.5).powf(1.3);
                    let pores = value_noise(u * 210.0, v * 26.0, 709);
                    let pore_dark = if pores < 0.22 { 0.38 } else { 0.0 };
                    let oil_sheen = (1.0 - ((u * 0.6 + v * 0.4) - 0.35).abs() * 2.1).clamp(0.0, 1.0);
                    if rings > 0.55 && pore_dark < 0.1 {
                        (
                            0.78,
                            0.52,
                            0.28,
                            ((rings - 0.55) * 0.42 + oil_sheen * 0.14).min(0.36),
                        )
                    } else {
                        (0.06, 0.03, 0.01, ((1.0 - rings) * 0.44 + pore_dark).min(0.62))
                    }
                }
                HorologyDialFinish::SmokedSkeleton => {
                    // Perlage circular jewel-spotting over dark gunmetal movement plate
                    let cell_u = (u * 24.0).fract() - 0.5;
                    let cell_v = (v * 12.0).fract() - 0.5;
                    let r_cell = (cell_u * cell_u + cell_v * cell_v).sqrt();
                    let swirl = ((r_cell * 36.0).sin() * 0.5 + 0.5) * (1.0 - (r_cell * 1.7).min(1.0));
                    let brush = value_noise(u * 160.0, v * 24.0, 811) * 0.22;
                    if swirl > 0.35 {
                        (0.84, 0.88, 0.94, ((swirl - 0.35) * 0.34 + brush * 0.15).min(0.26))
                    } else {
                        (0.02, 0.03, 0.06, (0.34 - swirl * 0.30).clamp(0.06, 0.42))
                    }
                }
            };
            put_px(&mut buf, px, py, r, g, b, a);
        }
    }
    buf
}

fn dial_finish_pixels(finish: HorologyDialFinish) -> &'static [u8] {
    static CACHES: [OnceLock<Vec<u8>>; HorologyDialFinish::COUNT] = [
        OnceLock::new(),
        OnceLock::new(),
        OnceLock::new(),
        OnceLock::new(),
        OnceLock::new(),
        OnceLock::new(),
        OnceLock::new(),
        OnceLock::new(),
    ];
    CACHES[finish as usize]
        .get_or_init(|| bake_dial_finish_bgra(finish))
        .as_slice()
}

// ============================================================================
// 4. Bounded LRU GPU Bitmap/Brush Cache (Evicted on Resize & COM Teardown)
// ============================================================================

struct RtHorologyCache {
    bitmaps: [Option<ID2D1Bitmap>; HorologyDialFinish::COUNT],
    brushes: [Option<ID2D1BitmapBrush>; HorologyDialFinish::COUNT],
    layer: Option<ID2D1Layer>,
    last_used: u64,
}

impl RtHorologyCache {
    fn new(stamp: u64) -> Self {
        Self {
            bitmaps: std::array::from_fn(|_| None),
            brushes: std::array::from_fn(|_| None),
            layer: None,
            last_used: stamp,
        }
    }
}

struct RtHorologyStore {
    entries: HashMap<usize, RtHorologyCache>,
    stamp: u64,
}

impl RtHorologyStore {
    fn new() -> Self {
        Self {
            entries: HashMap::new(),
            stamp: 0,
        }
    }

    fn next_stamp(&mut self) -> u64 {
        self.stamp = self.stamp.wrapping_add(1);
        self.stamp
    }

    fn touch_get(&mut self, key: usize) -> Option<&RtHorologyCache> {
        if self.entries.contains_key(&key) {
            let st = self.next_stamp();
            if let Some(e) = self.entries.get_mut(&key) {
                e.last_used = st;
            }
            self.entries.get(&key)
        } else {
            None
        }
    }

    fn touch_or_insert(&mut self, key: usize) -> &mut RtHorologyCache {
        let st = self.next_stamp();
        if !self.entries.contains_key(&key) && self.entries.len() >= MAX_RT_CACHE_ENTRIES {
            if let Some(oldest) = self
                .entries
                .iter()
                .min_by_key(|(_, e)| e.last_used)
                .map(|(&k, _)| k)
            {
                self.entries.remove(&oldest);
            }
        }
        let e = self
            .entries
            .entry(key)
            .or_insert_with(|| RtHorologyCache::new(st));
        e.last_used = st;
        e
    }
}

thread_local! {
    static RT_HORO_CACHE: RefCell<RtHorologyStore> = RefCell::new(RtHorologyStore::new());
}

pub fn evict_rt(rt: &ID2D1RenderTarget) {
    let key = rt.as_raw() as usize;
    RT_HORO_CACHE.with(|c| {
        c.borrow_mut().entries.remove(&key);
    });
}

pub fn evict_all() {
    RT_HORO_CACHE.with(|c| {
        c.borrow_mut().entries.clear();
    });
}

#[cfg(test)]
pub fn cache_len_for_tests() -> usize {
    RT_HORO_CACHE.with(|c| c.borrow().entries.len())
}

unsafe fn get_or_create_dial_brush(
    rt: &ID2D1RenderTarget,
    finish: HorologyDialFinish,
) -> Option<ID2D1BitmapBrush> {
    let key = rt.as_raw() as usize;
    let idx = finish as usize;
    let cached = RT_HORO_CACHE.with(|c| {
        let mut store = c.borrow_mut();
        store.touch_get(key).and_then(|e| e.brushes[idx].clone())
    });
    if cached.is_some() {
        return cached;
    }
    let bmp = {
        let cached_bmp = RT_HORO_CACHE.with(|c| {
            let mut store = c.borrow_mut();
            store.touch_get(key).and_then(|e| e.bitmaps[idx].clone())
        });
        if let Some(b) = cached_bmp {
            b
        } else {
            let pixels = dial_finish_pixels(finish);
            let props = D2D1_BITMAP_PROPERTIES {
                pixelFormat: D2D1_PIXEL_FORMAT {
                    format: DXGI_FORMAT_B8G8R8A8_UNORM,
                    alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
                },
                dpiX: 96.0,
                dpiY: 96.0,
            };
            let b = rt
                .CreateBitmap(
                    D2D_SIZE_U {
                        width: TEX_W,
                        height: TEX_H,
                    },
                    Some(pixels.as_ptr() as *const c_void),
                    TEX_W * 4,
                    &props,
                )
                .ok()?;
            RT_HORO_CACHE.with(|c| {
                let mut store = c.borrow_mut();
                store.touch_or_insert(key).bitmaps[idx] = Some(b.clone());
            });
            b
        }
    };
    let bprops = D2D1_BITMAP_BRUSH_PROPERTIES {
        extendModeX: D2D1_EXTEND_MODE_CLAMP,
        extendModeY: D2D1_EXTEND_MODE_CLAMP,
        interpolationMode: D2D1_BITMAP_INTERPOLATION_MODE_LINEAR,
    };
    let brush_props = D2D1_BRUSH_PROPERTIES {
        opacity: 1.0,
        transform: Matrix3x2::identity(),
    };
    let brush = rt
        .CreateBitmapBrush(&bmp, Some(&bprops), Some(&brush_props))
        .ok()?;
    RT_HORO_CACHE.with(|c| {
        let mut store = c.borrow_mut();
        store.touch_or_insert(key).brushes[idx] = Some(brush.clone());
    });
    Some(brush)
}

unsafe fn get_or_create_layer(rt: &ID2D1RenderTarget) -> Option<ID2D1Layer> {
    let key = rt.as_raw() as usize;
    let cached = RT_HORO_CACHE.with(|c| {
        let mut store = c.borrow_mut();
        store.touch_get(key).and_then(|e| e.layer.clone())
    });
    if cached.is_some() {
        return cached;
    }
    let layer = rt.CreateLayer(None).ok()?;
    RT_HORO_CACHE.with(|c| {
        let mut store = c.borrow_mut();
        store.touch_or_insert(key).layer = Some(layer.clone());
    });
    Some(layer)
}

// ============================================================================
// 5. Direct2D Gradient & Geometry Builders
// ============================================================================

unsafe fn make_lin_grad(
    rt: &ID2D1RenderTarget,
    p0: (f32, f32),
    p1: (f32, f32),
    stops: &[D2D1_GRADIENT_STOP],
) -> Option<ID2D1LinearGradientBrush> {
    let sc = rt
        .CreateGradientStopCollection(stops, D2D1_GAMMA_2_2, D2D1_EXTEND_MODE_CLAMP)
        .ok()?;
    rt.CreateLinearGradientBrush(
        &D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES {
            startPoint: Vector2 { X: p0.0, Y: p0.1 },
            endPoint: Vector2 { X: p1.0, Y: p1.1 },
        },
        None,
        &sc,
    )
    .ok()
}

unsafe fn make_rad_grad(
    rt: &ID2D1RenderTarget,
    cx: f32,
    cy: f32,
    ox: f32,
    oy: f32,
    rx: f32,
    ry: f32,
    stops: &[D2D1_GRADIENT_STOP],
) -> Option<ID2D1RadialGradientBrush> {
    let sc = rt
        .CreateGradientStopCollection(stops, D2D1_GAMMA_2_2, D2D1_EXTEND_MODE_CLAMP)
        .ok()?;
    rt.CreateRadialGradientBrush(
        &D2D1_RADIAL_GRADIENT_BRUSH_PROPERTIES {
            center: Vector2 { X: cx, Y: cy },
            gradientOriginOffset: Vector2 { X: ox, Y: oy },
            radiusX: rx.max(1.0),
            radiusY: ry.max(1.0),
        },
        None,
        &sc,
    )
    .ok()
}

unsafe fn build_polygon_geo(d2d: &ID2D1Factory, pts: &[Vector2]) -> Option<ID2D1PathGeometry> {
    if pts.len() < 3 {
        return None;
    }
    let geo = d2d.CreatePathGeometry().ok()?;
    let sink = geo.Open().ok()?;
    sink.BeginFigure(pts[0], D2D1_FIGURE_BEGIN_FILLED);
    for &p in &pts[1..] {
        sink.AddLine(p);
    }
    sink.EndFigure(D2D1_FIGURE_END_CLOSED);
    let _ = sink.Close();
    Some(geo)
}

unsafe fn draw_arc_stroke(
    rt: &ID2D1RenderTarget,
    d2d: Option<&ID2D1Factory>,
    cx: f32,
    cy: f32,
    r: f32,
    start_deg: f32,
    sweep_deg: f32,
    stroke_w: f32,
    color: D2D1_COLOR_F,
) {
    let clamped = sweep_deg.clamp(0.0, 359.5);
    if clamped < 0.8 || r < 1.5 {
        return;
    }
    let Ok(br) = rt.CreateSolidColorBrush(&color, None) else {
        return;
    };
    if let Some(factory) = d2d {
        if let Ok(geo) = factory.CreatePathGeometry() {
            if let Ok(sink) = geo.Open() {
                let a0 = start_deg.to_radians();
                let a1 = (start_deg + clamped).to_radians();
                let p0 = Vector2 {
                    X: cx + r * a0.cos(),
                    Y: cy + r * a0.sin(),
                };
                let p1 = Vector2 {
                    X: cx + r * a1.cos(),
                    Y: cy + r * a1.sin(),
                };
                sink.BeginFigure(p0, D2D1_FIGURE_BEGIN_HOLLOW);
                sink.AddArc(&D2D1_ARC_SEGMENT {
                    point: p1,
                    size: D2D_SIZE_F { width: r, height: r },
                    rotationAngle: 0.0,
                    sweepDirection: D2D1_SWEEP_DIRECTION_CLOCKWISE,
                    arcSize: if clamped > 180.0 {
                        D2D1_ARC_SIZE_LARGE
                    } else {
                        D2D1_ARC_SIZE_SMALL
                    },
                });
                sink.EndFigure(D2D1_FIGURE_END_OPEN);
                if sink.Close().is_ok() {
                    rt.DrawGeometry(&geo, &br, stroke_w, None);
                    return;
                }
            }
        }
    }
    // Fallback segmented arc if no factory was supplied
    let segs = ((clamped / 8.0).ceil() as usize).clamp(4, 48);
    let mut prev = {
        let a = start_deg.to_radians();
        Vector2 {
            X: cx + r * a.cos(),
            Y: cy + r * a.sin(),
        }
    };
    for i in 1..=segs {
        let a = (start_deg + clamped * (i as f32 / segs as f32)).to_radians();
        let cur = Vector2 {
            X: cx + r * a.cos(),
            Y: cy + r * a.sin(),
        };
        rt.DrawLine(prev, cur, &br, stroke_w, None);
        prev = cur;
    }
}

/// Computes realistic astronomical lunar phase in `0.0..1.0` (0.0 = New Moon, 0.5 = Full Moon)
/// from a calendar `(year, month, day)`.
pub fn lunar_phase_0_to_1(year: i32, month: u32, day: u32) -> f32 {
    let mut y = year;
    let mut m = month as i32;
    if m <= 2 {
        y -= 1;
        m += 12;
    }
    let a = y / 100;
    let b = 2 - a + (a / 4);
    let jd = (365.25 * (y + 4716) as f64).floor()
        + (30.6001 * (m + 1) as f64).floor()
        + day as f64
        + b as f64
        - 1524.5;
    // Known new moon epoch JD 2451550.1, synodic period 29.53058867 days
    let cycles = (jd - 2451550.1) / 29.53058867;
    (cycles - cycles.floor()) as f32
}

// ============================================================================
// 6. Photorealistic 3D Direct2D Horology Hardware Primitives
// ============================================================================

/// Draws a miniature countersunk hexagonal watchmaker bezel screw (Royal Oak / motorsport style)
/// with metallic top highlight, recessed socket shadow, and tangent screw slot.
pub unsafe fn draw_horology_hex_screw(
    rt: &ID2D1RenderTarget,
    d2d: Option<&ID2D1Factory>,
    cx: f32,
    cy: f32,
    r: f32,
    slot_angle: f32,
    s: f32,
    metal_rgb: u32,
) {
    // Countersunk recess shadow
    if let Ok(sh) = rt.CreateSolidColorBrush(&rgba(0, 0, 0, 0.52), None) {
        rt.FillEllipse(
            &D2D1_ELLIPSE {
                point: Vector2 { X: cx, Y: cy + 0.5 * s },
                radiusX: r + 0.85 * s,
                radiusY: r + 0.85 * s,
            },
            &sh,
        );
    }
    let stops = [
        D2D1_GRADIENT_STOP {
            position: 0.0,
            color: rgb_u32(lighten_rgb(metal_rgb, 0.52), 0.98),
        },
        D2D1_GRADIENT_STOP {
            position: 0.55,
            color: rgb_u32(metal_rgb, 0.96),
        },
        D2D1_GRADIENT_STOP {
            position: 1.0,
            color: rgb_u32(darken_rgb(metal_rgb, 0.55), 0.98),
        },
    ];
    let head_grad = make_lin_grad(rt, (cx - r, cy - r), (cx + r, cy + r), &stops);
    let mut drew_hex = false;
    if let (Some(factory), Some(ref g)) = (d2d, &head_grad) {
        let mut pts = [Vector2::default(); 6];
        for i in 0..6 {
            let a = (i as f32 * 60.0 - 30.0).to_radians();
            pts[i] = Vector2 {
                X: cx + r * a.cos(),
                Y: cy + r * a.sin(),
            };
        }
        if let Some(geo) = build_polygon_geo(factory, &pts) {
            rt.FillGeometry(&geo, g, None);
            drew_hex = true;
        }
    }
    if !drew_hex {
        if let Some(ref g) = head_grad {
            rt.FillEllipse(
                &D2D1_ELLIPSE {
                    point: Vector2 { X: cx, Y: cy },
                    radiusX: r,
                    radiusY: r,
                },
                g,
            );
        }
    }
    // Crisp tangent milled slot
    let dx = (r * 0.72) * slot_angle.cos();
    let dy = (r * 0.72) * slot_angle.sin();
    if let Ok(sl) = rt.CreateSolidColorBrush(&rgba(8, 10, 14, 0.78), None) {
        rt.DrawLine(
            Vector2 { X: cx - dx, Y: cy - dy },
            Vector2 { X: cx + dx, Y: cy + dy },
            &sl,
            (0.9 * s).max(0.75),
            None,
        );
    }
    if let Ok(hi) = rt.CreateSolidColorBrush(&rgba(255, 255, 255, 0.42), None) {
        rt.DrawLine(
            Vector2 {
                X: cx - dx,
                Y: cy - dy + 0.65 * s,
            },
            Vector2 {
                X: cx + dx,
                Y: cy + dy + 0.65 * s,
            },
            &hi,
            (0.55 * s).max(0.5),
            None,
        );
    }
}

/// Builds an 8-sided chamfered rectangular polygon (used for Royal Oak octagonal steel,
/// Forged Carbon hypercar monocoque, and bolted aviation cockpit panels).
unsafe fn build_chamfered_rect_geo(
    factory: &ID2D1Factory,
    rect: D2D_RECT_F,
    cut: f32,
) -> Option<ID2D1PathGeometry> {
    let w = (rect.right - rect.left).max(4.0);
    let h = (rect.bottom - rect.top).max(4.0);
    let c = cut.clamp(1.0, (w * 0.34).min(h * 0.34));
    let pts = [
        Vector2 { X: rect.left + c, Y: rect.top },
        Vector2 { X: rect.right - c, Y: rect.top },
        Vector2 { X: rect.right, Y: rect.top + c },
        Vector2 { X: rect.right, Y: rect.bottom - c },
        Vector2 { X: rect.right - c, Y: rect.bottom },
        Vector2 { X: rect.left + c, Y: rect.bottom },
        Vector2 { X: rect.left, Y: rect.bottom - c },
        Vector2 { X: rect.left, Y: rect.top + c },
    ];
    build_polygon_geo(factory, &pts)
}

/// Draws a precision mechanical skeleton gear wheel (rim, teeth, internal spokes, ruby/gold hub).
#[allow(clippy::too_many_arguments)]
pub unsafe fn draw_horology_gear_wheel(
    rt: &ID2D1RenderTarget,
    cx: f32,
    cy: f32,
    r: f32,
    teeth: usize,
    angle: f32,
    s: f32,
    color_rgb: u32,
    alpha: f32,
) {
    if r < 4.0 {
        return;
    }
    let t_cnt = teeth.clamp(6, 24);
    if let Ok(br) = rt.CreateSolidColorBrush(&rgb_u32(color_rgb, alpha), None) {
        rt.DrawEllipse(
            &D2D1_ELLIPSE {
                point: Vector2 { X: cx, Y: cy },
                radiusX: r * 0.78,
                radiusY: r * 0.78,
            },
            &br,
            (2.0 * s).max(1.2),
            None,
        );
        rt.DrawEllipse(
            &D2D1_ELLIPSE {
                point: Vector2 { X: cx, Y: cy },
                radiusX: r * 0.24,
                radiusY: r * 0.24,
            },
            &br,
            (1.4 * s).max(0.9),
            None,
        );
        let step = (2.0 * PI) / (t_cnt as f32);
        for i in 0..t_cnt {
            let a = angle + (i as f32) * step;
            rt.DrawLine(
                Vector2 { X: cx + r * 0.76 * a.cos(), Y: cy + r * 0.76 * a.sin() },
                Vector2 { X: cx + r * 1.00 * a.cos(), Y: cy + r * 1.00 * a.sin() },
                &br,
                (1.6 * s).max(1.0),
                None,
            );
        }
        for sp in 0..5 {
            let a = angle * 1.15 + (sp as f32) * (2.0 * PI / 5.0);
            rt.DrawLine(
                Vector2 { X: cx + r * 0.24 * a.cos(), Y: cy + r * 0.24 * a.sin() },
                Vector2 { X: cx + r * 0.74 * a.cos(), Y: cy + r * 0.74 * a.sin() },
                &br,
                (1.4 * s).max(0.9),
                None,
            );
        }
    }
}

/// Draws a hypercar segmented RPM tachometer arc (used by Forged Carbon Moto clocks & gauges).
#[allow(clippy::too_many_arguments)]
pub unsafe fn draw_horology_rpm_arc(
    rt: &ID2D1RenderTarget,
    cx: f32,
    cy: f32,
    r: f32,
    start_deg: f32,
    sweep_deg: f32,
    segments: usize,
    norm_fill: f32,
    s: f32,
    accent_rgb: u32,
) {
    let segs = segments.clamp(8, 36);
    let active_n = ((segs as f32) * norm_fill.clamp(0.0, 1.0)).round() as usize;
    let r_in = r * 0.80;
    let r_out = r;
    for i in 0..segs {
        let frac = (i as f32 + 0.5) / (segs as f32);
        let a = (start_deg + sweep_deg * frac).to_radians();
        let is_lit = i < active_n;
        let is_red = frac >= 0.78;
        let col = if is_lit {
            if is_red {
                rgba(255, 58, 40, 0.98)
            } else {
                rgb_u32(accent_rgb, 0.96)
            }
        } else if is_red {
            rgba(255, 64, 42, 0.24)
        } else {
            rgba(220, 232, 245, 0.14)
        };
        if let Ok(br) = rt.CreateSolidColorBrush(&col, None) {
            rt.DrawLine(
                Vector2 { X: cx + r_in * a.cos(), Y: cy + r_in * a.sin() },
                Vector2 { X: cx + r_out * a.cos(), Y: cy + r_out * a.sin() },
                &br,
                (2.4 * s).max(1.4),
                None,
            );
        }
    }
}

/// Draws a full photorealistic Horology case plate with 10 radically distinct physical chassis
/// silhouettes: Octagonal Royal Oak Steel, 18K Rose-Gold Lugs (Patek), Exposed Skeleton Bridges
/// (Tourbillon), Ceramic Tachymeter + Pushers (Daytona), Coin-Edge Diver + Pearl Pip (Submariner),
/// Bolted Square Cockpit Bulkhead (IWC Pilot), Grand Feu Ivory Porcelain (Breguet), Angular
/// Hypercar Carbon Monocoque (Forged Carbon), Thick Walnut Hardwood Cabinet + Brass (Walnut),
/// and Frameless Prismatic Translucent Sapphire Crystal Monolith (Sapphire Obsidian).
#[allow(clippy::too_many_arguments)]
pub unsafe fn draw_horology_case_plate(
    rt: &ID2D1RenderTarget,
    d2d: Option<&ID2D1Factory>,
    rect: D2D_RECT_F,
    radius: f32,
    s: f32,
    pal: &ResolvedHorologyPalette,
    opacity: f32,
    rim_alpha: f32,
    show_shadow: bool,
    hover_x: f32,
    hover_y: f32,
    hover_mix: f32,
) {
    let w = (rect.right - rect.left).max(4.0);
    let h = (rect.bottom - rect.top).max(4.0);
    let op = opacity.clamp(0.18, 0.99);

    // Theme-driven silhouette geometry parameters
    let (use_chamfer, chamfer_cut, eff_rad, bezel_th) = match pal.theme {
        HorologyTheme::RoyalOakSteel => (true, (16.5 * s).min(h * 0.24), 6.0 * s, 6.2 * s),
        HorologyTheme::ForgedCarbonMoto => (true, (15.0 * s).min(h * 0.23), 5.0 * s, 5.4 * s),
        HorologyTheme::IwcPilotTitanium => (true, (5.5 * s).min(h * 0.10), 4.0 * s, 5.2 * s),
        HorologyTheme::WalnutRegulator => (false, 0.0, radius.clamp(6.0 * s, 14.0 * s), 8.2 * s),
        HorologyTheme::DaytonaCeramic => (false, 0.0, radius.clamp(12.0 * s, 26.0 * s), 6.8 * s),
        HorologyTheme::SubmarinerEmerald => (false, 0.0, radius.clamp(14.0 * s, 28.0 * s), 6.2 * s),
        HorologyTheme::BreguetPorcelain => (false, 0.0, radius.clamp(10.0 * s, 18.0 * s), 3.6 * s),
        HorologyTheme::SapphireObsidian => (false, 0.0, radius.clamp(14.0 * s, 24.0 * s), 2.4 * s),
        HorologyTheme::PatekMoonphase => (false, 0.0, radius.clamp(16.0 * s, 28.0 * s), 5.6 * s),
        HorologyTheme::TourbillonGold => (false, 0.0, radius.clamp(12.0 * s, 22.0 * s), 4.8 * s),
    };
    let rad = eff_rad.min(w * 0.48).min(h * 0.48);

    // 1. Dual-layer physical elevation + contact drop shadow
    if show_shadow {
        crate::realistic_pbr::draw_pbr_drop_shadows(rt, rect, rad, s, pal.is_light_dial);
    }

    // Optional protruding physical case lugs / chronograph pushers BEFORE outer bezel fill
    if pal.theme == HorologyTheme::PatekMoonphase && w >= 110.0 * s && h >= 58.0 * s {
        // Sculpted 18K Rose-Gold stepped corner watch lugs
        let lug_w = (12.0 * s).min(w * 0.07);
        let lug_h = (4.2 * s).min(h * 0.06);
        let lug_stops = [
            D2D1_GRADIENT_STOP { position: 0.0, color: rgb_u32(pal.bezel_hi_rgb, op) },
            D2D1_GRADIENT_STOP { position: 1.0, color: rgb_u32(pal.bezel_lo_rgb, op) },
        ];
        if let Some(lg) = make_lin_grad(rt, (rect.left, rect.top), (rect.right, rect.bottom), &lug_stops) {
            for &(lx, ly) in &[
                (rect.left + rad * 0.72, rect.top - lug_h * 0.45),
                (rect.right - rad * 0.72 - lug_w, rect.top - lug_h * 0.45),
                (rect.left + rad * 0.72, rect.bottom - lug_h * 0.55),
                (rect.right - rad * 0.72 - lug_w, rect.bottom - lug_h * 0.55),
            ] {
                let lrr = D2D1_ROUNDED_RECT {
                    rect: D2D_RECT_F { left: lx, top: ly, right: lx + lug_w, bottom: ly + lug_h },
                    radiusX: 2.0 * s,
                    radiusY: 2.0 * s,
                };
                rt.FillRoundedRectangle(&lrr, &lg);
            }
        }
    } else if pal.theme == HorologyTheme::DaytonaCeramic && w >= 120.0 * s && h >= 66.0 * s {
        // Protruding stainless steel screw-down chronograph pushers on right case flank
        let pw = 3.2 * s;
        let ph = (11.0 * s).min(h * 0.14);
        let p_stops = [
            D2D1_GRADIENT_STOP { position: 0.0, color: rgba(238, 244, 252, op) },
            D2D1_GRADIENT_STOP { position: 0.5, color: rgba(152, 162, 176, op) },
            D2D1_GRADIENT_STOP { position: 1.0, color: rgba(74, 82, 94, op) },
        ];
        if let Some(pg) = make_lin_grad(rt, (rect.right - pw, rect.top), (rect.right + pw, rect.bottom), &p_stops) {
            for &py in &[rect.top + h * 0.24 - ph * 0.5, rect.top + h * 0.76 - ph * 0.5] {
                let prr = D2D1_ROUNDED_RECT {
                    rect: D2D_RECT_F { left: rect.right - 1.5 * s, top: py, right: rect.right + pw * 0.65, bottom: py + ph },
                    radiusX: 1.4 * s,
                    radiusY: 1.4 * s,
                };
                rt.FillRoundedRectangle(&prr, &pg);
            }
        }
    }

    // 2. Outer machined bezel chassis (with per-theme geometry: octagonal chamfer, walnut wood cabinet, frameless prism glass, etc.)
    let outer_rr = D2D1_ROUNDED_RECT { rect, radiusX: rad, radiusY: rad };
    let outer_geo = if use_chamfer {
        d2d.and_then(|f| build_chamfered_rect_geo(f, rect, chamfer_cut))
    } else {
        None
    };

    let inner_rect = D2D_RECT_F {
        left: rect.left + bezel_th,
        top: rect.top + bezel_th,
        right: (rect.right - bezel_th).max(rect.left + 4.0),
        bottom: (rect.bottom - bezel_th).max(rect.top + 4.0),
    };
    let inner_rad = (rad - bezel_th * 0.72).max(2.0 * s);
    let inner_rr = D2D1_ROUNDED_RECT {
        rect: inner_rect,
        radiusX: inner_rad,
        radiusY: inner_rad,
    };
    let inner_geo = if use_chamfer {
        d2d.and_then(|f| build_chamfered_rect_geo(f, inner_rect, (chamfer_cut - bezel_th * 0.58).max(2.0 * s)))
    } else {
        None
    };

    if pal.theme == HorologyTheme::WalnutRegulator {
        // Thick handcrafted American Walnut hardwood cabinet outer bezel!
        let wood_stops = [
            D2D1_GRADIENT_STOP { position: 0.0, color: rgba(138, 86, 44, op) },
            D2D1_GRADIENT_STOP { position: 0.28, color: rgba(96, 56, 26, op) },
            D2D1_GRADIENT_STOP { position: 0.68, color: rgba(58, 32, 14, op) },
            D2D1_GRADIENT_STOP { position: 1.0, color: rgba(114, 68, 32, op) },
        ];
        if let Some(wg) = make_lin_grad(rt, (rect.left, rect.top), (rect.right, rect.bottom), &wood_stops) {
            rt.FillRoundedRectangle(&outer_rr, &wg);
        }
        // Subtle cathedral wood grain streaks across outer timber frame
        if let Ok(gr) = rt.CreateSolidColorBrush(&rgba(34, 16, 6, 0.28 * op), None) {
            let mut gy = rect.top + 2.5 * s;
            while gy < rect.bottom - 2.0 * s {
                rt.DrawLine(
                    Vector2 { X: rect.left + 3.0 * s, Y: gy },
                    Vector2 { X: rect.right - 3.0 * s, Y: gy },
                    &gr,
                    0.75 * s,
                    None,
                );
                gy += 4.2 * s;
            }
        }
        // Inset gleaming solid brushed antique brass inner frame molding
        let brass_rr = D2D1_ROUNDED_RECT {
            rect: D2D_RECT_F {
                left: inner_rect.left - 1.6 * s,
                top: inner_rect.top - 1.6 * s,
                right: inner_rect.right + 1.6 * s,
                bottom: inner_rect.bottom + 1.6 * s,
            },
            radiusX: inner_rad + 1.4 * s,
            radiusY: inner_rad + 1.4 * s,
        };
        let b_stops = [
            D2D1_GRADIENT_STOP { position: 0.0, color: rgb_u32(pal.bezel_hi_rgb, op) },
            D2D1_GRADIENT_STOP { position: 0.5, color: rgb_u32(pal.bezel_rgb, op) },
            D2D1_GRADIENT_STOP { position: 1.0, color: rgb_u32(pal.bezel_lo_rgb, op) },
        ];
        if let Some(bg) = make_lin_grad(rt, (rect.left, rect.top), (rect.right, rect.bottom), &b_stops) {
            rt.DrawRoundedRectangle(&brass_rr, &bg, 2.6 * s, None);
        }
    } else if pal.theme == HorologyTheme::SapphireObsidian {
        // Frameless translucent optical sapphire crystal slab with prismatic cyan-violet bevel!
        let prism_stops = [
            D2D1_GRADIENT_STOP { position: 0.0, color: rgba(150, 232, 255, 0.54 * op) },
            D2D1_GRADIENT_STOP { position: 0.35, color: rgba(34, 58, 96, 0.42 * op) },
            D2D1_GRADIENT_STOP { position: 0.72, color: rgba(94, 72, 168, 0.44 * op) },
            D2D1_GRADIENT_STOP { position: 1.0, color: rgba(92, 224, 255, 0.62 * op) },
        ];
        if let Some(pg) = make_lin_grad(rt, (rect.left, rect.top), (rect.right, rect.bottom), &prism_stops) {
            rt.FillRoundedRectangle(&outer_rr, &pg);
        }
    } else {
        let case_stops = [
            D2D1_GRADIENT_STOP { position: 0.0, color: rgb_u32(pal.bezel_hi_rgb, op) },
            D2D1_GRADIENT_STOP { position: 0.32, color: rgb_u32(pal.bezel_rgb, op) },
            D2D1_GRADIENT_STOP { position: 0.70, color: rgb_u32(pal.bezel_lo_rgb, op) },
            D2D1_GRADIENT_STOP { position: 1.0, color: rgb_u32(pal.bezel_hi_rgb, op) },
        ];
        if let Some(cg) = make_lin_grad(rt, (rect.left, rect.top), (rect.right, rect.bottom), &case_stops) {
            if let Some(ref geo) = outer_geo {
                rt.FillGeometry(geo, &cg, None);
            } else {
                rt.FillRoundedRectangle(&outer_rr, &cg);
            }
        }
        // Apply per-widget user-selectable HorologyBezelFinish micro-machining
        match pal.bezel_finish {
            HorologyBezelFinish::FlutedCoin if pal.theme != HorologyTheme::BreguetPorcelain => {
                if let Ok(fl) = rt.CreateSolidColorBrush(&rgba(255, 250, 236, 0.28 * op), None) {
                    let step = (5.0 * s).max(3.5);
                    let mut fx = rect.left + rad + 2.0 * s;
                    while fx < rect.right - rad - 2.0 * s {
                        rt.DrawLine(Vector2 { X: fx, Y: rect.top + 0.5 * s }, Vector2 { X: fx, Y: rect.top + bezel_th }, &fl, 0.8 * s, None);
                        rt.DrawLine(Vector2 { X: fx, Y: rect.bottom - bezel_th }, Vector2 { X: fx, Y: rect.bottom - 0.5 * s }, &fl, 0.8 * s, None);
                        fx += step;
                    }
                }
            }
            HorologyBezelFinish::DiverKnurled if pal.theme != HorologyTheme::SubmarinerEmerald => {
                if let Ok(dk) = rt.CreateSolidColorBrush(&rgba(255, 255, 255, 0.26 * op), None) {
                    let step = (6.0 * s).max(4.0);
                    let mut fx = rect.left + rad + 2.0 * s;
                    while fx < rect.right - rad - 2.0 * s {
                        rt.DrawLine(Vector2 { X: fx, Y: rect.top + 0.5 * s }, Vector2 { X: fx, Y: rect.top + bezel_th * 0.85 }, &dk, 1.3 * s, None);
                        rt.DrawLine(Vector2 { X: fx, Y: rect.bottom - bezel_th * 0.85 }, Vector2 { X: fx, Y: rect.bottom - 0.5 * s }, &dk, 1.3 * s, None);
                        fx += step;
                    }
                }
            }
            _ => {}
        }
    }

    // 3. Recessed dial base fill (radial dome depth, translucent glass for Sapphire Obsidian)
    let dial_op = if pal.theme == HorologyTheme::SapphireObsidian {
        (op * 0.78).clamp(0.42, 0.84)
    } else {
        op
    };
    let dial_stops = [
        D2D1_GRADIENT_STOP { position: 0.0, color: rgb_u32(pal.dial_hi_rgb, dial_op) },
        D2D1_GRADIENT_STOP { position: 0.62, color: rgb_u32(pal.dial_rgb, dial_op) },
        D2D1_GRADIENT_STOP { position: 1.0, color: rgb_u32(pal.dial_lo_rgb, dial_op) },
    ];
    let iw = (inner_rect.right - inner_rect.left).max(2.0);
    let ih = (inner_rect.bottom - inner_rect.top).max(2.0);
    if let Some(dg) = make_rad_grad(
        rt,
        inner_rect.left + iw * 0.42,
        inner_rect.top + ih * 0.36,
        -iw * 0.12,
        -ih * 0.14,
        iw * 0.78,
        ih * 0.78,
        &dial_stops,
    ) {
        if let Some(ref geo) = inner_geo {
            rt.FillGeometry(geo, &dg, None);
        } else {
            rt.FillRoundedRectangle(&inner_rr, &dg);
        }
    }

    // 4. Cached 512x256 Per-Pixel Normal-Mapped Dial Surface Texture Overlay
    if let Some(brush) = get_or_create_dial_brush(rt, pal.dial_finish) {
        let sx = iw / TEX_W as f32;
        let sy = ih / TEX_H as f32;
        brush.SetTransform(&Matrix3x2 {
            M11: sx,
            M12: 0.0,
            M21: 0.0,
            M22: sy,
            M31: inner_rect.left,
            M32: inner_rect.top,
        });
        let tex_op = if pal.theme == HorologyTheme::SapphireObsidian {
            (0.45 * op).clamp(0.15, 0.55)
        } else {
            (0.92 * op).clamp(0.18, 0.98)
        };
        brush.SetOpacity(tex_op);
        if let Some(ref geo) = inner_geo {
            rt.FillGeometry(geo, &brush, None);
        } else {
            rt.FillRoundedRectangle(&inner_rr, &brush);
        }
    }

    // 5. SIGNATURE PER-THEME INTERIOR & BEZEL ARCHITECTURE (unmistakable across all 19 widgets!)
    match pal.theme {
        HorologyTheme::PatekMoonphase => {
            // Double-stepped rose-gold beaded perlage inner rim + celestial starfield speckles
            if let Ok(gp) = rt.CreateSolidColorBrush(&rgb_u32(pal.bezel_hi_rgb, 0.68 * op), None) {
                let p_rr = D2D1_ROUNDED_RECT {
                    rect: D2D_RECT_F {
                        left: inner_rect.left + 2.2 * s,
                        top: inner_rect.top + 2.2 * s,
                        right: (inner_rect.right - 2.2 * s).max(inner_rect.left + 2.0),
                        bottom: (inner_rect.bottom - 2.2 * s).max(inner_rect.top + 2.0),
                    },
                    radiusX: (inner_rad - 1.5 * s).max(1.0),
                    radiusY: (inner_rad - 1.5 * s).max(1.0),
                };
                rt.DrawRoundedRectangle(&p_rr, &gp, 0.9 * s, None);
            }
            if let Ok(st) = rt.CreateSolidColorBrush(&rgba(252, 224, 152, 0.48 * op), None) {
                for &(fx, fy, sr) in &[
                    (0.14f32, 0.22f32, 0.9f32),
                    (0.28, 0.78, 0.7),
                    (0.46, 0.16, 0.8),
                    (0.68, 0.20, 0.9),
                    (0.84, 0.30, 0.75),
                    (0.76, 0.82, 0.85),
                    (0.90, 0.72, 0.7),
                ] {
                    rt.FillEllipse(
                        &D2D1_ELLIPSE {
                            point: Vector2 { X: inner_rect.left + iw * fx, Y: inner_rect.top + ih * fy },
                            radiusX: sr * s,
                            radiusY: sr * s,
                        },
                        &st,
                    );
                }
            }
        }
        HorologyTheme::TourbillonGold => {
            // Openworked skeleton mechanical clockwork gears + diagonal Côtes de Genève gold bridges!
            let g_r = (ih * 0.34).min(iw * 0.20).clamp(12.0 * s, 44.0 * s);
            draw_horology_gear_wheel(
                rt,
                inner_rect.left + iw * 0.22,
                inner_rect.top + ih * 0.56,
                g_r,
                16,
                0.35,
                s,
                pal.bezel_rgb,
                0.24 * op,
            );
            draw_horology_gear_wheel(
                rt,
                inner_rect.right - iw * 0.20,
                inner_rect.top + ih * 0.36,
                g_r * 0.78,
                12,
                -0.55,
                s,
                pal.hands_rgb,
                0.22 * op,
            );
            // Machined gold skeleton bridge struts spanning upper/lower corners
            if let Ok(brg) = rt.CreateSolidColorBrush(&rgb_u32(pal.bezel_rgb, 0.42 * op), None) {
                rt.DrawLine(
                    Vector2 { X: inner_rect.left + 8.0 * s, Y: inner_rect.top + ih * 0.18 },
                    Vector2 { X: inner_rect.right - 8.0 * s, Y: inner_rect.top + ih * 0.18 },
                    &brg,
                    1.6 * s,
                    None,
                );
                rt.DrawLine(
                    Vector2 { X: inner_rect.left + 8.0 * s, Y: inner_rect.bottom - ih * 0.18 },
                    Vector2 { X: inner_rect.right - 8.0 * s, Y: inner_rect.bottom - ih * 0.18 },
                    &brg,
                    1.6 * s,
                    None,
                );
            }
        }
        HorologyTheme::DaytonaCeramic => {
            // Crisp white engraved tachymeter bezel ticks + crimson red accent markers!
            if let Ok(tk) = rt.CreateSolidColorBrush(&rgba(242, 246, 252, 0.74 * op), None) {
                let mut fx = rect.left + rad.max(12.0 * s);
                let mut idx = 0usize;
                while fx < rect.right - rad.max(12.0 * s) {
                    let major = idx % 4 == 0;
                    let len = if major { 4.2 * s } else { 2.2 * s };
                    rt.DrawLine(
                        Vector2 { X: fx, Y: rect.top + 1.2 * s },
                        Vector2 { X: fx, Y: rect.top + 1.2 * s + len },
                        &tk,
                        if major { 1.15 * s } else { 0.75 * s },
                        None,
                    );
                    rt.DrawLine(
                        Vector2 { X: fx, Y: rect.bottom - 1.2 * s - len },
                        Vector2 { X: fx, Y: rect.bottom - 1.2 * s },
                        &tk,
                        if major { 1.15 * s } else { 0.75 * s },
                        None,
                    );
                    fx += 7.2 * s;
                    idx += 1;
                }
            }
            // Racing crimson top pinstripe inside ceramic bezel
            if let Ok(rd) = rt.CreateSolidColorBrush(&rgb_u32(pal.lume_rgb, 0.88 * op), None) {
                rt.DrawLine(
                    Vector2 { X: inner_rect.left + rad, Y: inner_rect.top + 1.2 * s },
                    Vector2 { X: inner_rect.right - rad, Y: inner_rect.top + 1.2 * s },
                    &rd,
                    1.5 * s,
                    None,
                );
            }
        }
        HorologyTheme::SubmarinerEmerald => {
            // Bold coin-edge diver bezel serrations + glowing cyan-mint 12-o'clock pearl pip!
            if let Ok(kn) = rt.CreateSolidColorBrush(&rgba(4, 18, 12, 0.56 * op), None) {
                let mut fx = rect.left + rad.max(10.0 * s);
                while fx < rect.right - rad.max(10.0 * s) {
                    rt.DrawLine(
                        Vector2 { X: fx, Y: rect.top + 0.5 * s },
                        Vector2 { X: fx, Y: rect.top + bezel_th * 0.84 },
                        &kn,
                        1.5 * s,
                        None,
                    );
                    rt.DrawLine(
                        Vector2 { X: fx, Y: rect.bottom - bezel_th * 0.84 },
                        Vector2 { X: fx, Y: rect.bottom - 0.5 * s },
                        &kn,
                        1.5 * s,
                        None,
                    );
                    fx += 6.0 * s;
                }
            }
            // Luminous emerald-mint diver pearl pip at top center of bezel
            let mid_x = (rect.left + rect.right) * 0.5;
            if let Ok(pip_glow) = rt.CreateSolidColorBrush(&rgb_u32(pal.lume_rgb, 0.36 * op), None) {
                rt.FillEllipse(
                    &D2D1_ELLIPSE {
                        point: Vector2 { X: mid_x, Y: rect.top + bezel_th * 0.5 },
                        radiusX: 4.5 * s,
                        radiusY: 4.5 * s,
                    },
                    &pip_glow,
                );
            }
            if let Ok(pip) = rt.CreateSolidColorBrush(&rgb_u32(pal.lume_rgb, 0.98 * op), None) {
                rt.FillEllipse(
                    &D2D1_ELLIPSE {
                        point: Vector2 { X: mid_x, Y: rect.top + bezel_th * 0.5 },
                        radiusX: 2.3 * s,
                        radiusY: 2.3 * s,
                    },
                    &pip,
                );
            }
        }
        HorologyTheme::IwcPilotTitanium => {
            // Avionics amber-orange corner calibration brackets + subtle cockpit horizon crosshair
            if let Ok(av) = rt.CreateSolidColorBrush(&rgb_u32(pal.jewel_rgb, 0.82 * op), None) {
                let b = 9.0 * s;
                let il = inner_rect.left + 3.0 * s;
                let it = inner_rect.top + 3.0 * s;
                let ir = inner_rect.right - 3.0 * s;
                let ib = inner_rect.bottom - 3.0 * s;
                for &(x0, y0, dx, dy) in &[
                    (il, it, b, b),
                    (ir, it, -b, b),
                    (il, ib, b, -b),
                    (ir, ib, -b, -b),
                ] {
                    rt.DrawLine(Vector2 { X: x0, Y: y0 }, Vector2 { X: x0 + dx, Y: y0 }, &av, 1.4 * s, None);
                    rt.DrawLine(Vector2 { X: x0, Y: y0 }, Vector2 { X: x0, Y: y0 + dy }, &av, 1.4 * s, None);
                }
            }
            // Faint tritium-green cockpit pitch line
            if let Ok(pl) = rt.CreateSolidColorBrush(&rgb_u32(pal.lume_rgb, 0.14 * op), None) {
                let mid_y = inner_rect.top + ih * 0.5;
                rt.DrawLine(
                    Vector2 { X: inner_rect.left + 12.0 * s, Y: mid_y },
                    Vector2 { X: inner_rect.right - 12.0 * s, Y: mid_y },
                    &pl,
                    0.8 * s,
                    None,
                );
            }
        }
        HorologyTheme::BreguetPorcelain => {
            // Fine silver fluted coin-edge + delicate sapphire-blue railway chemin-de-fer inner frame!
            if let Ok(fl_br) = rt.CreateSolidColorBrush(&rgba(255, 255, 255, 0.55 * op), None) {
                let mut fx = rect.left + rad.max(8.0 * s);
                while fx < rect.right - rad.max(8.0 * s) {
                    rt.DrawLine(
                        Vector2 { X: fx, Y: rect.top + 0.5 * s },
                        Vector2 { X: fx, Y: rect.top + bezel_th * 0.9 },
                        &fl_br,
                        0.75 * s,
                        None,
                    );
                    rt.DrawLine(
                        Vector2 { X: fx, Y: rect.bottom - bezel_th * 0.9 },
                        Vector2 { X: fx, Y: rect.bottom - 0.5 * s },
                        &fl_br,
                        0.75 * s,
                        None,
                    );
                    fx += 3.6 * s;
                }
            }
            if let Ok(bl) = rt.CreateSolidColorBrush(&rgb_u32(pal.hands_rgb, 0.34 * op), None) {
                let rail_rr = D2D1_ROUNDED_RECT {
                    rect: D2D_RECT_F {
                        left: inner_rect.left + 4.0 * s,
                        top: inner_rect.top + 4.0 * s,
                        right: (inner_rect.right - 4.0 * s).max(inner_rect.left + 2.0),
                        bottom: (inner_rect.bottom - 4.0 * s).max(inner_rect.top + 2.0),
                    },
                    radiusX: (inner_rad - 2.5 * s).max(1.0),
                    radiusY: (inner_rad - 2.5 * s).max(1.0),
                };
                rt.DrawRoundedRectangle(&rail_rr, &bl, 1.0 * s, None);
            }
        }
        HorologyTheme::ForgedCarbonMoto => {
            // Blazing neon-orange anodized gasket stripe + sculpted side air-intake louver slots!
            if let Ok(gs) = rt.CreateSolidColorBrush(&rgb_u32(pal.lume_rgb, 0.92 * op), None) {
                if let Some(ref geo) = inner_geo {
                    rt.DrawGeometry(geo, &gs, 1.8 * s, None);
                } else {
                    rt.DrawRoundedRectangle(&inner_rr, &gs, 1.8 * s, None);
                }
                // Angled hypercar side intake vent accents on left & right walls
                if h >= 68.0 * s {
                    for k in -1i32..=1 {
                        let vy = inner_rect.top + ih * 0.5 + (k as f32) * 8.0 * s;
                        rt.DrawLine(
                            Vector2 { X: rect.left + 1.5 * s, Y: vy - 2.5 * s },
                            Vector2 { X: rect.left + bezel_th + 1.5 * s, Y: vy + 2.5 * s },
                            &gs,
                            1.5 * s,
                            None,
                        );
                        rt.DrawLine(
                            Vector2 { X: rect.right - 1.5 * s, Y: vy - 2.5 * s },
                            Vector2 { X: rect.right - bezel_th - 1.5 * s, Y: vy + 2.5 * s },
                            &gs,
                            1.5 * s,
                            None,
                        );
                    }
                }
            }
        }
        HorologyTheme::SapphireObsidian => {
            // Internal diagonal optical prism light caustic refraction beam + double-bevel inner crystal glow
            let beam_stops = [
                D2D1_GRADIENT_STOP { position: 0.0, color: rgba(96, 232, 255, 0.0) },
                D2D1_GRADIENT_STOP { position: 0.42, color: rgba(112, 236, 255, 0.16 * op) },
                D2D1_GRADIENT_STOP { position: 0.54, color: rgba(176, 128, 255, 0.12 * op) },
                D2D1_GRADIENT_STOP { position: 1.0, color: rgba(96, 232, 255, 0.0) },
            ];
            if let Some(bg) = make_lin_grad(
                rt,
                (inner_rect.left, inner_rect.top),
                (inner_rect.right, inner_rect.bottom),
                &beam_stops,
            ) {
                rt.FillRoundedRectangle(&inner_rr, &bg);
            }
            if let Ok(ir) = rt.CreateSolidColorBrush(&rgba(130, 234, 255, 0.42 * op), None) {
                rt.DrawRoundedRectangle(&inner_rr, &ir, 1.0 * s, None);
            }
        }
        _ => {}
    }

    // 6. Recessed inner rehaut / top-left ambient occlusion depth inside bezel lip
    if pal.theme != HorologyTheme::SapphireObsidian {
        let rehaut_stops = [
            D2D1_GRADIENT_STOP {
                position: 0.0,
                color: rgba(0, 0, 0, if pal.is_light_dial { 0.18 } else { 0.48 }),
            },
            D2D1_GRADIENT_STOP { position: 0.14, color: rgba(0, 0, 0, 0.0) },
            D2D1_GRADIENT_STOP { position: 0.86, color: rgba(255, 255, 255, 0.0) },
            D2D1_GRADIENT_STOP {
                position: 1.0,
                color: rgba(255, 255, 255, if pal.is_light_dial { 0.35 } else { 0.14 }),
            },
        ];
        if let Some(rg) = make_lin_grad(
            rt,
            (inner_rect.left, inner_rect.top),
            (inner_rect.right, inner_rect.bottom),
            &rehaut_stops,
        ) {
            if let Some(ref geo) = inner_geo {
                rt.DrawGeometry(geo, &rg, 2.0 * s, None);
            } else {
                rt.DrawRoundedRectangle(&inner_rr, &rg, 2.0 * s, None);
            }
        }
    }

    // 7. Theme-specific Corner Hardware (honoring `pal.show_screws` toggle!)
    if pal.show_screws && w >= 72.0 * s && h >= 44.0 * s {
        match pal.theme {
            HorologyTheme::RoyalOakSteel => {
                // Signature 8 gleaming white-gold hex bolts positioned on the chamfered octagonal steel flats
                let inset_c = (chamfer_cut * 0.62).max(9.0 * s);
                let sr = (2.8 * s).clamp(1.8, 4.0);
                for (sx_p, sy_p, ang) in [
                    (rect.left + inset_c, rect.top + inset_c, 0.78f32),
                    (rect.right - inset_c, rect.top + inset_c, -0.78f32),
                    (rect.left + inset_c, rect.bottom - inset_c, -0.78f32),
                    (rect.right - inset_c, rect.bottom - inset_c, 0.78f32),
                ] {
                    draw_horology_hex_screw(rt, d2d, sx_p, sy_p, sr, ang, s, 0xE6EEF7);
                }
                if w >= 190.0 * s && h >= 76.0 * s {
                    let mid_x = (rect.left + rect.right) * 0.5;
                    let mid_y = (rect.top + rect.bottom) * 0.5;
                    draw_horology_hex_screw(rt, d2d, mid_x, rect.top + bezel_th * 0.56, sr * 0.9, 0.0, s, 0xE6EEF7);
                    draw_horology_hex_screw(rt, d2d, mid_x, rect.bottom - bezel_th * 0.56, sr * 0.9, 0.0, s, 0xE6EEF7);
                    draw_horology_hex_screw(rt, d2d, rect.left + bezel_th * 0.56, mid_y, sr * 0.9, PI * 0.5, s, 0xE6EEF7);
                    draw_horology_hex_screw(rt, d2d, rect.right - bezel_th * 0.56, mid_y, sr * 0.9, PI * 0.5, s, 0xE6EEF7);
                }
            }
            HorologyTheme::TourbillonGold => {
                // 4 Crimson Ruby Jewel Bearings mounted inside 24K gold chatons at corners!
                let ins = (bezel_th + 5.5 * s).min(w * 0.10).min(h * 0.20);
                let jr = (3.0 * s).clamp(2.0, 4.2);
                if let (Ok(gold), Ok(ruby)) = (
                    rt.CreateSolidColorBrush(&rgb_u32(pal.bezel_hi_rgb, 0.96), None),
                    rt.CreateSolidColorBrush(&rgb_u32(pal.jewel_rgb, 0.98), None),
                ) {
                    for (cx_p, cy_p) in [
                        (rect.left + ins, rect.top + ins),
                        (rect.right - ins, rect.top + ins),
                        (rect.left + ins, rect.bottom - ins),
                        (rect.right - ins, rect.bottom - ins),
                    ] {
                        rt.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: cx_p, Y: cy_p }, radiusX: jr, radiusY: jr }, &gold);
                        rt.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: cx_p, Y: cy_p }, radiusX: jr * 0.62, radiusY: jr * 0.62 }, &ruby);
                    }
                }
            }
            HorologyTheme::IwcPilotTitanium => {
                // 4 Oversized recessed aviation cockpit cross-slotted mounting bolts!
                let ins = (bezel_th + 3.2 * s).min(w * 0.10).min(h * 0.18);
                let sr = (3.4 * s).clamp(2.2, 4.6);
                for (sx_p, sy_p) in [
                    (rect.left + ins, rect.top + ins),
                    (rect.right - ins, rect.top + ins),
                    (rect.left + ins, rect.bottom - ins),
                    (rect.right - ins, rect.bottom - ins),
                ] {
                    draw_horology_hex_screw(rt, d2d, sx_p, sy_p, sr, 0.0, s, 0x8C96A4);
                    if let Ok(sl) = rt.CreateSolidColorBrush(&rgba(8, 10, 14, 0.78), None) {
                        rt.DrawLine(
                            Vector2 { X: sx_p, Y: sy_p - sr * 0.68 },
                            Vector2 { X: sx_p, Y: sy_p + sr * 0.68 },
                            &sl,
                            (0.95 * s).max(0.75),
                            None,
                        );
                    }
                }
            }
            HorologyTheme::WalnutRegulator => {
                // 4 Polished antique solid-brass dome cabinet rivets on the walnut frame!
                let ins = bezel_th * 0.52;
                let rr_b = (2.4 * s).clamp(1.6, 3.4);
                if let (Ok(br), Ok(hi)) = (
                    rt.CreateSolidColorBrush(&rgb_u32(pal.bezel_rgb, 0.96), None),
                    rt.CreateSolidColorBrush(&rgba(255, 248, 224, 0.65), None),
                ) {
                    for (bx_p, by_p) in [
                        (rect.left + ins + 2.0 * s, rect.top + ins + 2.0 * s),
                        (rect.right - ins - 2.0 * s, rect.top + ins + 2.0 * s),
                        (rect.left + ins + 2.0 * s, rect.bottom - ins - 2.0 * s),
                        (rect.right - ins - 2.0 * s, rect.bottom - ins - 2.0 * s),
                    ] {
                        rt.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: bx_p, Y: by_p }, radiusX: rr_b, radiusY: rr_b }, &br);
                        rt.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: bx_p - 0.6 * s, Y: by_p - 0.6 * s }, radiusX: rr_b * 0.38, radiusY: rr_b * 0.38 }, &hi);
                    }
                }
            }
            HorologyTheme::SapphireObsidian => {
                // Laser-etched ice-cyan optical corner micro-crosshairs (frameless glass look!)
                let ins = 10.0 * s;
                let arm = 3.2 * s;
                if let Ok(ch) = rt.CreateSolidColorBrush(&rgb_u32(pal.lume_rgb, 0.65), None) {
                    for (cx_p, cy_p) in [
                        (rect.left + ins, rect.top + ins),
                        (rect.right - ins, rect.top + ins),
                        (rect.left + ins, rect.bottom - ins),
                        (rect.right - ins, rect.bottom - ins),
                    ] {
                        rt.DrawLine(Vector2 { X: cx_p - arm, Y: cy_p }, Vector2 { X: cx_p + arm, Y: cy_p }, &ch, 0.85 * s, None);
                        rt.DrawLine(Vector2 { X: cx_p, Y: cy_p - arm }, Vector2 { X: cx_p, Y: cy_p + arm }, &ch, 0.85 * s, None);
                    }
                }
            }
            HorologyTheme::BreguetPorcelain | HorologyTheme::PatekMoonphase => {
                // Pure dress watch elegance — NO clunky bolts; subtle blued-steel / rose-gold corner pins
                let ins = (bezel_th + 5.0 * s).min(w * 0.10).min(h * 0.20);
                let pr = (1.6 * s).clamp(1.1, 2.4);
                let p_col = if pal.theme == HorologyTheme::BreguetPorcelain {
                    pal.hands_rgb
                } else {
                    pal.bezel_hi_rgb
                };
                if let Ok(pb) = rt.CreateSolidColorBrush(&rgb_u32(p_col, 0.78), None) {
                    for (cx_p, cy_p) in [
                        (rect.left + ins, rect.top + ins),
                        (rect.right - ins, rect.top + ins),
                        (rect.left + ins, rect.bottom - ins),
                        (rect.right - ins, rect.bottom - ins),
                    ] {
                        rt.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: cx_p, Y: cy_p }, radiusX: pr, radiusY: pr }, &pb);
                    }
                }
            }
            _ => {
                // ForgedCarbonMoto, DaytonaCeramic, SubmarinerEmerald: anodized / titanium hex bolts
                let inset = (bezel_th + 5.0 * s).min(w * 0.12).min(h * 0.22);
                let sr = (2.6 * s).clamp(1.8, 3.8);
                let screw_rgb = if pal.theme == HorologyTheme::ForgedCarbonMoto {
                    pal.lume_rgb
                } else {
                    pal.bezel_hi_rgb
                };
                for (sx_p, sy_p, ang) in [
                    (rect.left + inset, rect.top + inset, 0.78f32),
                    (rect.right - inset, rect.top + inset, -0.78f32),
                    (rect.left + inset, rect.bottom - inset, -0.78f32),
                    (rect.right - inset, rect.bottom - inset, 0.78f32),
                ] {
                    draw_horology_hex_screw(rt, d2d, sx_p, sy_p, sr, ang, s, screw_rgb);
                }
            }
        }
    }

    // 8. Outer 135° incident-light metallic / prismatic catchlight rim
    let ra = (rim_alpha + hover_mix * 0.16).clamp(0.0, 1.0);
    if ra > 0.02 {
        let rim_stops = if pal.theme == HorologyTheme::SapphireObsidian {
            [
                D2D1_GRADIENT_STOP { position: 0.0, color: rgba(185, 244, 255, (0.95 * ra).min(0.98)) },
                D2D1_GRADIENT_STOP { position: 0.40, color: rgba(75, 232, 255, (0.76 * ra).min(0.92)) },
                D2D1_GRADIENT_STOP { position: 0.74, color: rgba(168, 122, 255, (0.72 * ra).min(0.88)) },
                D2D1_GRADIENT_STOP { position: 1.0, color: rgba(155, 240, 255, (0.90 * ra).min(0.96)) },
            ]
        } else {
            [
                D2D1_GRADIENT_STOP { position: 0.0, color: rgba(255, 255, 255, (0.76 * ra).min(0.90)) },
                D2D1_GRADIENT_STOP { position: 0.36, color: rgb_u32(pal.bezel_hi_rgb, (0.56 * ra).min(0.80)) },
                D2D1_GRADIENT_STOP { position: 0.74, color: rgb_u32(pal.bezel_lo_rgb, (0.45 * ra).min(0.68)) },
                D2D1_GRADIENT_STOP { position: 1.0, color: rgb_u32(pal.lume_rgb, (0.34 * ra + hover_mix * 0.22).min(0.76)) },
            ]
        };
        if let Some(rim_g) = make_lin_grad(rt, (rect.left, rect.top), (rect.right, rect.bottom), &rim_stops) {
            let rim_w = if pal.theme == HorologyTheme::SapphireObsidian {
                (1.8 * s).max(1.2)
            } else {
                (1.3 * s).max(1.0)
            };
            if let Some(ref geo) = outer_geo {
                rt.DrawGeometry(geo, &rim_g, rim_w, None);
            } else {
                rt.DrawRoundedRectangle(&outer_rr, &rim_g, rim_w, None);
            }
        }
    }

    // 9. Interactive cursor specular sheen across case bezel on hover
    if hover_mix > 0.02 && pal.glare_alpha > 0.01 {
        let gx = rect.left + w * hover_x.clamp(0.0, 1.0);
        let gy = rect.top + h * hover_y.clamp(0.0, 1.0);
        let h_stops = [
            D2D1_GRADIENT_STOP {
                position: 0.0,
                color: rgba(255, 255, 255, (0.16 * hover_mix * pal.glare_alpha).min(0.24)),
            },
            D2D1_GRADIENT_STOP { position: 1.0, color: rgba(255, 255, 255, 0.0) },
        ];
        if let Some(hg) = make_rad_grad(rt, gx, gy, 0.0, 0.0, w * 0.42, h * 0.55, &h_stops) {
            if let Some(ref geo) = inner_geo {
                rt.FillGeometry(geo, &hg, None);
            } else {
                rt.FillRoundedRectangle(&inner_rr, &hg);
            }
        }
    }
}

/// Draws a curved 45° anti-reflective sapphire crystal glass reflection over a case/dial.
#[allow(clippy::too_many_arguments)]
pub unsafe fn draw_sapphire_ar_glare(
    rt: &ID2D1RenderTarget,
    d2d: Option<&ID2D1Factory>,
    rect: D2D_RECT_F,
    radius: f32,
    s: f32,
    pal: &ResolvedHorologyPalette,
    hover_x: f32,
    hover_mix: f32,
) {
    if pal.glare_alpha <= 0.01 {
        return;
    }
    let w = (rect.right - rect.left).max(4.0);
    let _h = (rect.bottom - rect.top).max(4.0);
    let shift = (hover_x - 0.5) * w * 0.14 * hover_mix;
    let a = pal.glare_alpha * if pal.is_light_dial { 0.16 } else { 0.22 };
    let ar_tint = match pal.theme {
        HorologyTheme::IwcPilotTitanium | HorologyTheme::SapphireObsidian => (175u8, 215u8, 255u8),
        HorologyTheme::PatekMoonphase | HorologyTheme::TourbillonGold => (255u8, 240u8, 212u8),
        _ => (235u8, 246u8, 255u8),
    };
    let stops = [
        D2D1_GRADIENT_STOP {
            position: 0.0,
            color: rgba(ar_tint.0, ar_tint.1, ar_tint.2, a * 0.82),
        },
        D2D1_GRADIENT_STOP {
            position: 0.34,
            color: rgba(ar_tint.0, ar_tint.1, ar_tint.2, a * 0.22),
        },
        D2D1_GRADIENT_STOP {
            position: 0.48,
            color: rgba(255, 255, 255, 0.0),
        },
        D2D1_GRADIENT_STOP {
            position: 0.88,
            color: rgba(255, 255, 255, 0.0),
        },
        D2D1_GRADIENT_STOP {
            position: 1.0,
            color: rgba(ar_tint.0, ar_tint.1, ar_tint.2, a * 0.28),
        },
    ];
    let rr = D2D1_ROUNDED_RECT {
        rect: D2D_RECT_F {
            left: rect.left + 2.0 * s,
            top: rect.top + 2.0 * s,
            right: (rect.right - 2.0 * s).max(rect.left + 2.0),
            bottom: (rect.bottom - 2.0 * s).max(rect.top + 2.0),
        },
        radiusX: (radius - 1.5 * s).max(1.0),
        radiusY: (radius - 1.5 * s).max(1.0),
    };
    let Some(grad) = make_lin_grad(
        rt,
        (rect.left + shift, rect.top),
        (rect.right + shift, rect.bottom),
        &stops,
    ) else {
        return;
    };
    if let (Some(factory), Some(layer)) = (d2d, get_or_create_layer(rt)) {
        if let Ok(clip_geo) = factory.CreateRoundedRectangleGeometry(&rr) {
            let clip_base: ID2D1Geometry = clip_geo.into();
            let lp = D2D1_LAYER_PARAMETERS {
                contentBounds: rr.rect,
                geometricMask: std::mem::ManuallyDrop::new(Some(clip_base)),
                maskAntialiasMode: D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
                maskTransform: Matrix3x2::identity(),
                opacity: 1.0,
                opacityBrush: std::mem::ManuallyDrop::new(None),
                layerOptions: D2D1_LAYER_OPTIONS_NONE,
            };
            rt.PushLayer(&lp, &layer);
            rt.FillRoundedRectangle(&rr, &grad);
            rt.PopLayer();
            let _ = std::mem::ManuallyDrop::into_inner(lp.geometricMask);
            return;
        }
    }
    rt.FillRoundedRectangle(&rr, &grad);
}

/// Draws a tapered dauphine/baton watch needle with optional real-time directional cast shadow.
#[allow(clippy::too_many_arguments)]
pub unsafe fn draw_horology_needle(
    rt: &ID2D1RenderTarget,
    cx: f32,
    cy: f32,
    angle_rad: f32,
    length: f32,
    tail: f32,
    width: f32,
    color_rgb: u32,
    cast_shadow: bool,
    s: f32,
) {
    let ca = angle_rad.cos();
    let sa = angle_rad.sin();
    let x0 = cx - tail * ca;
    let y0 = cy - tail * sa;
    let x1 = cx + length * ca;
    let y1 = cy + length * sa;

    if cast_shadow {
        let sox = 1.6 * s;
        let soy = 2.2 * s;
        if let Ok(sh) = rt.CreateSolidColorBrush(&rgba(0, 0, 0, 0.36), None) {
            rt.DrawLine(
                Vector2 { X: x0 + sox, Y: y0 + soy },
                Vector2 { X: x1 + sox, Y: y1 + soy },
                &sh,
                (width + 0.7 * s).max(1.2),
                None,
            );
        }
    }
    if let Ok(br) = rt.CreateSolidColorBrush(&rgb_u32(color_rgb, 0.98), None) {
        rt.DrawLine(
            Vector2 { X: x0, Y: y0 },
            Vector2 { X: x1, Y: y1 },
            &br,
            width.max(1.0),
            None,
        );
    }
    // Fine luminous/specular centerline along needle
    if width >= 1.8 * s {
        if let Ok(hi) = rt.CreateSolidColorBrush(&rgba(255, 255, 255, 0.55), None) {
            rt.DrawLine(
                Vector2 {
                    X: cx + (length * 0.22) * ca,
                    Y: cy + (length * 0.22) * sa,
                },
                Vector2 {
                    X: cx + (length * 0.84) * ca,
                    Y: cy + (length * 0.84) * sa,
                },
                &hi,
                (width * 0.36).max(0.6),
                None,
            );
        }
    }
}

/// Draws a theme-authentic recessed horology sub-dial gauge (`0.0..=1.0` sweep across 270°)
/// tailored per HorologyTheme archetype: Panda White Daytona, Cream-Parchment Brass Walnut,
/// Openworked Gear Tourbillon, Segmented Neon-Orange RPM Carbon, Frameless Optical Sapphire,
/// Bright Porcelain Breguet, Tritium Aviation IWC, Scalloped Diver Submariner, Rose-Gold Patek,
/// and Octagonal Steel Royal Oak.
#[allow(clippy::too_many_arguments)]
pub unsafe fn draw_horology_subdial(
    rt: &ID2D1RenderTarget,
    d2d: Option<&ID2D1Factory>,
    cx: f32,
    cy: f32,
    r: f32,
    s: f32,
    pal: &ResolvedHorologyPalette,
    norm_val: f32,
    redline_start: Option<f32>,
    needle_rgb_override: Option<u32>,
) {
    if r < 6.0 {
        return;
    }
    let nv = norm_val.clamp(0.0, 1.0);
    let is_panda = pal.theme == HorologyTheme::DaytonaCeramic;
    let is_parchment = pal.theme == HorologyTheme::WalnutRegulator;
    let is_sapphire = pal.theme == HorologyTheme::SapphireObsidian;
    let is_carbon = pal.theme == HorologyTheme::ForgedCarbonMoto;
    let light_face = pal.is_light_dial || is_panda || is_parchment;

    // 1. Metallic / Optical chapter ring bevel
    if is_sapphire {
        // Frameless glowing prismatic cyan optical ring (no heavy opaque metal!)
        if let Ok(gl) = rt.CreateSolidColorBrush(&rgba(75, 232, 255, 0.36), None) {
            rt.DrawEllipse(
                &D2D1_ELLIPSE {
                    point: Vector2 { X: cx, Y: cy },
                    radiusX: r + 1.2 * s,
                    radiusY: r + 1.2 * s,
                },
                &gl,
                (1.6 * s).max(1.0),
                None,
            );
        }
    } else {
        let ring_stops = if is_panda {
            [
                D2D1_GRADIENT_STOP { position: 0.0, color: rgba(24, 28, 36, 0.98) },
                D2D1_GRADIENT_STOP { position: 0.5, color: rgba(8, 10, 14, 0.99) },
                D2D1_GRADIENT_STOP { position: 1.0, color: rgba(68, 76, 90, 0.96) },
            ]
        } else {
            [
                D2D1_GRADIENT_STOP { position: 0.0, color: rgb_u32(pal.bezel_lo_rgb, 0.94) },
                D2D1_GRADIENT_STOP { position: 0.5, color: rgb_u32(pal.bezel_rgb, 0.96) },
                D2D1_GRADIENT_STOP { position: 1.0, color: rgb_u32(pal.bezel_hi_rgb, 0.98) },
            ]
        };
        if let Some(rg) = make_lin_grad(rt, (cx - r, cy - r), (cx + r, cy + r), &ring_stops) {
            let mut drew_oct = false;
            if pal.theme == HorologyTheme::RoyalOakSteel {
                if let Some(factory) = d2d {
                    let mut pts = [Vector2::default(); 8];
                    for i in 0..8 {
                        let a = (i as f32 * 45.0 + 22.5).to_radians();
                        pts[i] = Vector2 {
                            X: cx + (r + 2.4 * s) * a.cos(),
                            Y: cy + (r + 2.4 * s) * a.sin(),
                        };
                    }
                    if let Some(geo) = build_polygon_geo(factory, &pts) {
                        rt.FillGeometry(&geo, &rg, None);
                        drew_oct = true;
                    }
                }
            }
            if !drew_oct {
                rt.FillEllipse(
                    &D2D1_ELLIPSE {
                        point: Vector2 { X: cx, Y: cy },
                        radiusX: r + (if is_panda { 2.4 } else { 1.8 }) * s,
                        radiusY: r + (if is_panda { 2.4 } else { 1.8 }) * s,
                    },
                    &rg,
                );
            }
        }
    }

    // 2. Recessed dial face (Bright Panda White / Antique Parchment / Translucent Sapphire / Deep Enamel)
    let sub_bg = if is_panda {
        0xEEF2F7
    } else if is_parchment {
        0xF1E6CE
    } else if pal.is_light_dial {
        darken_rgb(pal.dial_rgb, 0.05)
    } else {
        darken_rgb(pal.dial_rgb, 0.30)
    };
    let face_alpha = if is_sapphire { 0.62 } else { 0.98 };
    let sub_stops = [
        D2D1_GRADIENT_STOP { position: 0.0, color: rgb_u32(lighten_rgb(sub_bg, 0.14), face_alpha) },
        D2D1_GRADIENT_STOP { position: 0.78, color: rgb_u32(sub_bg, face_alpha) },
        D2D1_GRADIENT_STOP {
            position: 1.0,
            color: rgb_u32(darken_rgb(sub_bg, if light_face { 0.14 } else { 0.40 }), face_alpha),
        },
    ];
    if let Some(sg) = make_rad_grad(rt, cx, cy, r * 0.14, r * 0.14, r, r, &sub_stops) {
        rt.FillEllipse(
            &D2D1_ELLIPSE { point: Vector2 { X: cx, Y: cy }, radiusX: r, radiusY: r },
            &sg,
        );
    }

    // Openworked spinning brass skeleton wheel inside Tourbillon sub-dials!
    if pal.theme == HorologyTheme::TourbillonGold {
        draw_horology_gear_wheel(rt, cx, cy, r * 0.66, 10, nv * 3.2, s * 0.75, pal.bezel_rgb, 0.34);
    }

    // 3. Concentric azurage micro-grooves
    let az_col = if light_face {
        rgba(24, 32, 46, 0.12)
    } else {
        rgba(220, 234, 255, 0.08)
    };
    if let Ok(az) = rt.CreateSolidColorBrush(&az_col, None) {
        for frac in [0.82f32, 0.64, 0.46] {
            if r * frac > 5.0 {
                rt.DrawEllipse(
                    &D2D1_ELLIPSE {
                        point: Vector2 { X: cx, Y: cy },
                        radiusX: r * frac,
                        radiusY: r * frac,
                    },
                    &az,
                    (0.55 * s).max(0.45),
                    None,
                );
            }
        }
    }

    // 4. 270° instrument scale (135° bottom-left -> 405° bottom-right)
    let start_deg = 135.0f32;
    let total_deg = 270.0f32;

    if is_carbon {
        // Hypercar segmented glowing neon-orange RPM tachometer bars!
        draw_horology_rpm_arc(
            rt,
            cx,
            cy,
            r * 0.88,
            start_deg,
            total_deg,
            18,
            nv,
            s * 0.75,
            needle_rgb_override.unwrap_or(pal.lume_rgb),
        );
    } else {
        draw_arc_stroke(
            rt,
            d2d,
            cx,
            cy,
            r * 0.78,
            start_deg,
            total_deg,
            (2.0 * s).max(1.2),
            if light_face {
                rgba(20, 28, 42, 0.16)
            } else {
                rgba(255, 255, 255, 0.12)
            },
        );
        if let Some(rl) = redline_start {
            let rl_c = rl.clamp(0.5, 0.95);
            draw_arc_stroke(
                rt,
                d2d,
                cx,
                cy,
                r * 0.78,
                start_deg + total_deg * rl_c,
                total_deg * (1.0 - rl_c),
                (2.2 * s).max(1.4),
                rgba(232, 54, 72, 0.86),
            );
        }
        if nv > 0.01 {
            let sweep_col = if is_parchment {
                0xC88A2E
            } else {
                needle_rgb_override.unwrap_or(pal.lume_rgb)
            };
            draw_arc_stroke(
                rt,
                d2d,
                cx,
                cy,
                r * 0.78,
                start_deg,
                total_deg * nv,
                (2.3 * s).max(1.4),
                rgb_u32(sweep_col, 0.94),
            );
        }
        // 11 Applied metallic / diver-dot ticks across 270°
        let tick_col = if light_face {
            rgba(22, 30, 44, 0.84)
        } else {
            rgb_u32(pal.bezel_hi_rgb, 0.82)
        };
        if let Ok(tb) = rt.CreateSolidColorBrush(&tick_col, None) {
            for i in 0..=10 {
                let deg = start_deg + (i as f32 / 10.0) * total_deg;
                let a = deg.to_radians();
                let major = i % 5 == 0;
                if pal.theme == HorologyTheme::SubmarinerEmerald && major {
                    if let Ok(lb) = rt.CreateSolidColorBrush(&rgb_u32(pal.lume_rgb, 0.95), None) {
                        rt.FillEllipse(
                            &D2D1_ELLIPSE {
                                point: Vector2 { X: cx + r * 0.77 * a.cos(), Y: cy + r * 0.77 * a.sin() },
                                radiusX: (1.9 * s).max(1.2),
                                radiusY: (1.9 * s).max(1.2),
                            },
                            &lb,
                        );
                    }
                    continue;
                }
                let r_in = r * if major { 0.64 } else { 0.74 };
                let r_out = r * 0.89;
                rt.DrawLine(
                    Vector2 { X: cx + r_in * a.cos(), Y: cy + r_in * a.sin() },
                    Vector2 { X: cx + r_out * a.cos(), Y: cy + r_out * a.sin() },
                    &tb,
                    if major { (1.6 * s).max(1.0) } else { (0.85 * s).max(0.6) },
                    None,
                );
            }
        }
    }

    // 5. Sweeping tapered horology needle with real-time cast shadow
    let needle_ang = (start_deg + total_deg * nv).to_radians();
    let n_rgb = needle_rgb_override.unwrap_or(match pal.theme {
        HorologyTheme::DaytonaCeramic => 0xE6283B,
        HorologyTheme::ForgedCarbonMoto => pal.lume_rgb,
        HorologyTheme::WalnutRegulator => 0x22160C,
        HorologyTheme::BreguetPorcelain => pal.hands_rgb,
        HorologyTheme::IwcPilotTitanium => pal.lume_rgb,
        _ => pal.hands_rgb,
    });
    draw_horology_needle(
        rt,
        cx,
        cy,
        needle_ang,
        r * 0.74,
        r * 0.22,
        (1.8 * s).max(1.1),
        n_rgb,
        pal.show_needle_shadows,
        s,
    );

    // 6. Central pinion cap + ruby/metallic jewel dot
    let cap_rgb = if is_panda { 0x181C24 } else { pal.bezel_hi_rgb };
    if let Ok(cap) = rt.CreateSolidColorBrush(&rgb_u32(cap_rgb, 0.98), None) {
        rt.FillEllipse(
            &D2D1_ELLIPSE {
                point: Vector2 { X: cx, Y: cy },
                radiusX: (2.8 * s).max(1.8),
                radiusY: (2.8 * s).max(1.8),
            },
            &cap,
        );
    }
    if let Ok(jew) = rt.CreateSolidColorBrush(&rgb_u32(pal.jewel_rgb, 0.96), None) {
        rt.FillEllipse(
            &D2D1_ELLIPSE {
                point: Vector2 { X: cx, Y: cy },
                radiusX: (1.3 * s).max(0.9),
                radiusY: (1.3 * s).max(0.9),
            },
            &jew,
        );
    }
}

/// Draws an exposed animated Tourbillon escapement cage (gear teeth + oscillating balance wheel
/// + gold filigree bridge + ruby jewel bearing).
#[allow(clippy::too_many_arguments)]
pub unsafe fn draw_horology_tourbillon_cage(
    rt: &ID2D1RenderTarget,
    cx: f32,
    cy: f32,
    r: f32,
    s: f32,
    pal: &ResolvedHorologyPalette,
    phase: f32,
) {
    if r < 6.0 {
        return;
    }
    // Deep recessed aperture cavity
    if let Ok(cav) = rt.CreateSolidColorBrush(&rgba(4, 6, 10, 0.94), None) {
        rt.FillEllipse(
            &D2D1_ELLIPSE {
                point: Vector2 { X: cx, Y: cy },
                radiusX: r,
                radiusY: r,
            },
            &cav,
        );
    }
    // Rotating 12-tooth brass escapement gear
    let gear_ang = phase * 0.9;
    let brass_rgb = pal.bezel_hi_rgb;
    if let Ok(gb) = rt.CreateSolidColorBrush(&rgb_u32(brass_rgb, 0.82), None) {
        rt.DrawEllipse(
            &D2D1_ELLIPSE {
                point: Vector2 { X: cx, Y: cy },
                radiusX: r * 0.72,
                radiusY: r * 0.72,
            },
            &gb,
            (1.3 * s).max(0.9),
            None,
        );
        for t in 0..12 {
            let a = gear_ang + (t as f32) * (PI / 6.0);
            rt.DrawLine(
                Vector2 { X: cx + r * 0.68 * a.cos(), Y: cy + r * 0.68 * a.sin() },
                Vector2 { X: cx + r * 0.90 * a.cos(), Y: cy + r * 0.90 * a.sin() },
                &gb,
                (1.4 * s).max(0.9),
                None,
            );
        }
        // 3 spinning carriage spokes
        for sp in 0..3 {
            let a = -gear_ang * 1.4 + (sp as f32) * (2.0 * PI / 3.0);
            rt.DrawLine(
                Vector2 { X: cx, Y: cy },
                Vector2 { X: cx + r * 0.70 * a.cos(), Y: cy + r * 0.70 * a.sin() },
                &gb,
                (1.1 * s).max(0.8),
                None,
            );
        }
    }
    // Oscillating balance spring rim (2.5Hz horology beat feel)
    let osc = (phase * 3.8).sin() * 0.55;
    if let Ok(sp_br) = rt.CreateSolidColorBrush(&rgb_u32(pal.hands_rgb, 0.90), None) {
        rt.DrawEllipse(
            &D2D1_ELLIPSE {
                point: Vector2 { X: cx, Y: cy },
                radiusX: r * 0.44,
                radiusY: r * 0.44,
            },
            &sp_br,
            (1.1 * s).max(0.75),
            None,
        );
        let ba = osc;
        rt.DrawLine(
            Vector2 { X: cx - r * 0.44 * ba.cos(), Y: cy - r * 0.44 * ba.sin() },
            Vector2 { X: cx + r * 0.44 * ba.cos(), Y: cy + r * 0.44 * ba.sin() },
            &sp_br,
            (1.1 * s).max(0.8),
            None,
        );
    }
    // Horizontal filigree tourbillon bridge + blued screws + central ruby pivot
    if let Ok(brg) = rt.CreateSolidColorBrush(&rgb_u32(pal.bezel_rgb, 0.95), None) {
        rt.DrawLine(
            Vector2 { X: cx - r * 0.96, Y: cy },
            Vector2 { X: cx + r * 0.96, Y: cy },
            &brg,
            (2.0 * s).max(1.2),
            None,
        );
    }
    if let Ok(bl) = rt.CreateSolidColorBrush(&rgba(42, 118, 224, 0.95), None) {
        for sx in [-r * 0.80, r * 0.80] {
            rt.FillEllipse(
                &D2D1_ELLIPSE {
                    point: Vector2 { X: cx + sx, Y: cy },
                    radiusX: (1.5 * s).max(1.0),
                    radiusY: (1.5 * s).max(1.0),
                },
                &bl,
            );
        }
    }
    if let Ok(rb) = rt.CreateSolidColorBrush(&rgb_u32(pal.jewel_rgb, 0.98), None) {
        rt.FillEllipse(
            &D2D1_ELLIPSE {
                point: Vector2 { X: cx, Y: cy },
                radiusX: (2.6 * s).max(1.6),
                radiusY: (2.6 * s).max(1.6),
            },
            &rb,
        );
    }
    if let Ok(rim) = rt.CreateSolidColorBrush(&rgb_u32(pal.bezel_hi_rgb, 0.72), None) {
        rt.DrawEllipse(
            &D2D1_ELLIPSE {
                point: Vector2 { X: cx, Y: cy },
                radiusX: r,
                radiusY: r,
            },
            &rim,
            (1.1 * s).max(0.8),
            None,
        );
    }
}

/// Draws a live astronomical lunar phase aperture (starry cobalt sky + shaded gold moon disc).
pub unsafe fn draw_horology_moonphase(
    rt: &ID2D1RenderTarget,
    cx: f32,
    cy: f32,
    r: f32,
    s: f32,
    pal: &ResolvedHorologyPalette,
    lunar_phase: f32,
) {
    if r < 6.0 {
        return;
    }
    // Starry midnight sky cavity
    let sky_stops = [
        D2D1_GRADIENT_STOP {
            position: 0.0,
            color: rgba(16, 36, 78, 0.98),
        },
        D2D1_GRADIENT_STOP {
            position: 1.0,
            color: rgba(6, 12, 28, 0.99),
        },
    ];
    if let Some(sg) = make_rad_grad(rt, cx, cy, 0.0, -r * 0.2, r, r, &sky_stops) {
        rt.FillEllipse(
            &D2D1_ELLIPSE {
                point: Vector2 { X: cx, Y: cy },
                radiusX: r,
                radiusY: r,
            },
            &sg,
        );
    }
    // Pinpoint gold stars
    if let Ok(st) = rt.CreateSolidColorBrush(&rgba(246, 214, 140, 0.82), None) {
        for &(ox, oy, sr) in &[
            (-0.48f32, -0.34f32, 0.85f32),
            (0.46, -0.26, 0.75),
            (-0.30, 0.36, 0.70),
            (0.38, 0.30, 0.80),
            (0.04, -0.56, 0.65),
        ] {
            rt.FillEllipse(
                &D2D1_ELLIPSE {
                    point: Vector2 { X: cx + ox * r, Y: cy + oy * r },
                    radiusX: sr * s,
                    radiusY: sr * s,
                },
                &st,
            );
        }
    }
    // Gold moon orb positioned according to synodic phase (0.5 = full centered moon)
    let phase_norm = lunar_phase.rem_euclid(1.0);
    let moon_r = r * 0.42;
    let moon_stops = [
        D2D1_GRADIENT_STOP {
            position: 0.0,
            color: rgba(255, 240, 188, 0.98),
        },
        D2D1_GRADIENT_STOP {
            position: 1.0,
            color: rgba(212, 162, 74, 0.98),
        },
    ];
    if let Some(mg) = make_rad_grad(
        rt,
        cx,
        cy,
        -moon_r * 0.25,
        -moon_r * 0.25,
        moon_r,
        moon_r,
        &moon_stops,
    ) {
        rt.FillEllipse(
            &D2D1_ELLIPSE {
                point: Vector2 { X: cx, Y: cy },
                radiusX: moon_r,
                radiusY: moon_r,
            },
            &mg,
        );
    }
    // Shadow mask disc creating authentic waxing/waning crescent
    let shadow_shift = (phase_norm - 0.5) * 2.0 * (moon_r * 2.1);
    if shadow_shift.abs() < moon_r * 1.95 {
        if let Ok(sh) = rt.CreateSolidColorBrush(&rgba(8, 16, 36, 0.96), None) {
            rt.FillEllipse(
                &D2D1_ELLIPSE {
                    point: Vector2 { X: cx + shadow_shift, Y: cy },
                    radiusX: moon_r * 0.98,
                    radiusY: moon_r * 0.98,
                },
                &sh,
            );
        }
    }
    // Beveled gold/steel aperture ring
    if let Ok(rim) = rt.CreateSolidColorBrush(&rgb_u32(pal.bezel_hi_rgb, 0.85), None) {
        rt.DrawEllipse(
            &D2D1_ELLIPSE {
                point: Vector2 { X: cx, Y: cy },
                radiusX: r,
                radiusY: r,
            },
            &rim,
            (1.1 * s).max(0.8),
            None,
        );
    }
}

/// Draws a full precision analog horology watch dial (bezel, railway minute track, 12 applied
/// indices, optional moonphase or tourbillon complication, swept cast-shadow hands, ruby pivot).
#[allow(clippy::too_many_arguments)]
pub unsafe fn draw_horology_watch_dial(
    rt: &ID2D1RenderTarget,
    d2d: Option<&ID2D1Factory>,
    cx: f32,
    cy: f32,
    r: f32,
    s: f32,
    pal: &ResolvedHorologyPalette,
    hour_12: f32,
    minute: f32,
    second: Option<f32>,
    phase: f32,
    lunar_phase: f32,
) {
    if r < 10.0 {
        return;
    }
    let is_sapphire = pal.theme == HorologyTheme::SapphireObsidian;
    let is_parchment = pal.theme == HorologyTheme::WalnutRegulator;
    let is_emerald = pal.theme == HorologyTheme::SubmarinerEmerald;
    let light_face = pal.is_light_dial || is_parchment;

    // 1. Bezel ring (octagonal 8-bolt for Royal Oak, frameless glowing prism for Sapphire,
    //    serrated emerald diver bezel for Submariner, circular machined for others)
    let outer_r = r + if is_emerald { 5.4 * s } else { 3.8 * s };
    if is_sapphire {
        // Frameless glowing prismatic orbital halo (NO heavy metal ring!)
        if let Ok(halo) = rt.CreateSolidColorBrush(&rgba(75, 232, 255, 0.44), None) {
            rt.DrawEllipse(
                &D2D1_ELLIPSE { point: Vector2 { X: cx, Y: cy }, radiusX: r + 1.5 * s, radiusY: r + 1.5 * s },
                &halo,
                (1.8 * s).max(1.1),
                None,
            );
        }
        if let Ok(halo2) = rt.CreateSolidColorBrush(&rgba(156, 120, 255, 0.28), None) {
            rt.DrawEllipse(
                &D2D1_ELLIPSE { point: Vector2 { X: cx, Y: cy }, radiusX: r + 4.0 * s, radiusY: r + 4.0 * s },
                &halo2,
                (0.9 * s).max(0.7),
                None,
            );
        }
    } else {
        let bez_stops = [
            D2D1_GRADIENT_STOP { position: 0.0, color: rgb_u32(pal.bezel_hi_rgb, 0.98) },
            D2D1_GRADIENT_STOP { position: 0.48, color: rgb_u32(pal.bezel_rgb, 0.96) },
            D2D1_GRADIENT_STOP { position: 1.0, color: rgb_u32(pal.bezel_lo_rgb, 0.98) },
        ];
        if let Some(bg) = make_lin_grad(
            rt,
            (cx - outer_r, cy - outer_r),
            (cx + outer_r, cy + outer_r),
            &bez_stops,
        ) {
            let mut drew_oct = false;
            if pal.theme == HorologyTheme::RoyalOakSteel {
                if let Some(factory) = d2d {
                    let mut pts = [Vector2::default(); 8];
                    for i in 0..8 {
                        let a = (i as f32 * 45.0 + 22.5).to_radians();
                        pts[i] = Vector2 {
                            X: cx + (outer_r * 1.08) * a.cos(),
                            Y: cy + (outer_r * 1.08) * a.sin(),
                        };
                    }
                    if let Some(geo) = build_polygon_geo(factory, &pts) {
                        rt.FillGeometry(&geo, &bg, None);
                        drew_oct = true;
                        // 8 tiny signature bezel screws on the Royal Oak watch ring!
                        if r >= 26.0 * s && pal.show_screws {
                            for i in 0..8 {
                                let a = (i as f32 * 45.0).to_radians();
                                draw_horology_hex_screw(
                                    rt,
                                    d2d,
                                    cx + (r + 2.0 * s) * a.cos(),
                                    cy + (r + 2.0 * s) * a.sin(),
                                    (1.5 * s).max(1.0),
                                    a + PI * 0.5,
                                    s * 0.7,
                                    0xEDF4FC,
                                );
                            }
                        }
                    }
                }
            }
            if !drew_oct {
                rt.FillEllipse(
                    &D2D1_ELLIPSE { point: Vector2 { X: cx, Y: cy }, radiusX: outer_r, radiusY: outer_r },
                    &bg,
                );
            }
        }
        // Emerald Diver ceramic insert ring + serrated coin-edge notches + 12 o'clock pearl pip!
        if is_emerald {
            if let Ok(em) = rt.CreateSolidColorBrush(&rgba(8, 72, 50, 0.96), None) {
                rt.DrawEllipse(
                    &D2D1_ELLIPSE { point: Vector2 { X: cx, Y: cy }, radiusX: r + 2.6 * s, radiusY: r + 2.6 * s },
                    &em,
                    3.6 * s,
                    None,
                );
            }
            if let Ok(pip) = rt.CreateSolidColorBrush(&rgb_u32(pal.lume_rgb, 0.98), None) {
                rt.FillEllipse(
                    &D2D1_ELLIPSE { point: Vector2 { X: cx, Y: cy - r - 2.6 * s }, radiusX: 2.4 * s, radiusY: 2.4 * s },
                    &pip,
                );
            }
        }
    }

    // 2. Dial face interior (warm parchment on Walnut Regulator, translucent glass on Sapphire)
    let face_r = r - 0.8 * s;
    let (fd_hi, fd_mid, fd_lo, fd_a) = if is_parchment {
        (0xFBF4E4, 0xF0E4C8, 0xD8C69E, 0.99)
    } else if is_sapphire {
        (0x162A4A, 0x0C182E, 0x060D1A, 0.62)
    } else {
        (pal.dial_hi_rgb, pal.dial_rgb, pal.dial_lo_rgb, 0.99)
    };
    let face_stops = [
        D2D1_GRADIENT_STOP { position: 0.0, color: rgb_u32(fd_hi, fd_a) },
        D2D1_GRADIENT_STOP { position: 0.72, color: rgb_u32(fd_mid, fd_a) },
        D2D1_GRADIENT_STOP { position: 1.0, color: rgb_u32(fd_lo, fd_a) },
    ];
    if let Some(fg) = make_rad_grad(
        rt,
        cx,
        cy,
        -face_r * 0.18,
        -face_r * 0.18,
        face_r,
        face_r,
        &face_stops,
    ) {
        rt.FillEllipse(
            &D2D1_ELLIPSE { point: Vector2 { X: cx, Y: cy }, radiusX: face_r, radiusY: face_r },
            &fg,
        );
    }

    // Inner minute railway track
    let trk_col = if light_face {
        rgba(24, 34, 48, 0.36)
    } else {
        rgba(225, 236, 250, 0.28)
    };
    if let Ok(tb) = rt.CreateSolidColorBrush(&trk_col, None) {
        rt.DrawEllipse(
            &D2D1_ELLIPSE {
                point: Vector2 { X: cx, Y: cy },
                radiusX: face_r * 0.90,
                radiusY: face_r * 0.90,
            },
            &tb,
            (0.75 * s).max(0.6),
            None,
        );
    }

    // 3. 12 Applied Metallic / Luminous Indices (theme-authentic diver dots, aviation triangle, parchment ink)
    let idx_col = if is_parchment {
        rgba(34, 22, 12, 0.92)
    } else if pal.is_light_dial {
        rgb_u32(pal.text_rgb, 0.92)
    } else {
        rgb_u32(pal.bezel_hi_rgb, 0.94)
    };
    if let Ok(ib) = rt.CreateSolidColorBrush(&idx_col, None) {
        for m in 0..12 {
            let a = (m as f32 * 30.0 - 90.0).to_radians();
            if m == 0 && (pal.theme == HorologyTheme::IwcPilotTitanium || is_emerald) {
                // Iconic 12 o'clock luminous triangle (IWC Pilot with twin dots, or Submariner diver triangle)
                if let Ok(trit) = rt.CreateSolidColorBrush(&rgb_u32(pal.lume_rgb, 0.96), None) {
                    if let Some(factory) = d2d {
                        let pts = [
                            Vector2 { X: cx, Y: cy - face_r * 0.66 },
                            Vector2 { X: cx - 4.0 * s, Y: cy - face_r * 0.87 },
                            Vector2 { X: cx + 4.0 * s, Y: cy - face_r * 0.87 },
                        ];
                        if let Some(geo) = build_polygon_geo(factory, &pts) {
                            rt.FillGeometry(&geo, &trit, None);
                        }
                    }
                    if pal.theme == HorologyTheme::IwcPilotTitanium {
                        for dx in [-6.0 * s, 6.0 * s] {
                            rt.FillEllipse(
                                &D2D1_ELLIPSE {
                                    point: Vector2 { X: cx + dx, Y: cy - face_r * 0.77 },
                                    radiusX: 1.4 * s,
                                    radiusY: 1.4 * s,
                                },
                                &trit,
                            );
                        }
                    }
                }
                continue;
            }
            let cardinal = m % 3 == 0;
            if is_emerald && !cardinal {
                // Iconic Submariner circular luminous dot hour markers!
                if let Ok(dot) = rt.CreateSolidColorBrush(&rgb_u32(pal.lume_rgb, 0.94), None) {
                    rt.FillEllipse(
                        &D2D1_ELLIPSE {
                            point: Vector2 { X: cx + face_r * 0.77 * a.cos(), Y: cy + face_r * 0.77 * a.sin() },
                            radiusX: (2.3 * s).max(1.3),
                            radiusY: (2.3 * s).max(1.3),
                        },
                        &dot,
                    );
                }
                continue;
            }
            let r_in = face_r * if cardinal { 0.68 } else { 0.77 };
            let r_out = face_r * 0.89;
            rt.DrawLine(
                Vector2 { X: cx + r_in * a.cos(), Y: cy + r_in * a.sin() },
                Vector2 { X: cx + r_out * a.cos(), Y: cy + r_out * a.sin() },
                &ib,
                if cardinal { (2.4 * s).max(1.4) } else { (1.15 * s).max(0.8) },
                None,
            );
            // Luminous Super-LumiNova pip on cardinal indices
            if cardinal && !light_face && r >= 26.0 * s {
                if let Ok(lb) = rt.CreateSolidColorBrush(&rgb_u32(pal.lume_rgb, 0.82), None) {
                    rt.FillEllipse(
                        &D2D1_ELLIPSE {
                            point: Vector2 {
                                X: cx + (face_r * 0.61) * a.cos(),
                                Y: cy + (face_r * 0.61) * a.sin(),
                            },
                            radiusX: (1.3 * s).max(0.85),
                            radiusY: (1.3 * s).max(0.85),
                        },
                        &lb,
                    );
                }
            }
        }
    }

    // 4. Signature complication (Moonphase for Patek, Live Tourbillon for TourbillonGold, Panda Ring for Daytona)
    if r >= 28.0 * s {
        match pal.theme {
            HorologyTheme::PatekMoonphase => {
                draw_horology_moonphase(rt, cx, cy + face_r * 0.34, face_r * 0.28, s, pal, lunar_phase);
            }
            HorologyTheme::TourbillonGold => {
                draw_horology_tourbillon_cage(rt, cx, cy + face_r * 0.34, face_r * 0.30, s, pal, phase);
            }
            HorologyTheme::DaytonaCeramic => {
                // Bright white Panda 6-o'clock chronograph register inside dial
                if let Ok(pw) = rt.CreateSolidColorBrush(&rgba(238, 242, 247, 0.94), None) {
                    rt.FillEllipse(
                        &D2D1_ELLIPSE {
                            point: Vector2 { X: cx, Y: cy + face_r * 0.36 },
                            radiusX: face_r * 0.23,
                            radiusY: face_r * 0.23,
                        },
                        &pw,
                    );
                }
            }
            _ => {}
        }
    }

    // 5. Swept Hour, Minute, and Optional Seconds Hands with Cast Drop Shadows
    let h_ang = (((hour_12 % 12.0) + minute / 60.0) * 30.0 - 90.0).to_radians();
    let m_ang = (minute * 6.0 - 90.0).to_radians();
    let dial_hands_rgb = if is_parchment { 0x1E140B } else { pal.hands_rgb };
    draw_horology_needle(
        rt,
        cx,
        cy,
        h_ang,
        face_r * 0.50,
        face_r * 0.14,
        (2.8 * s).max(1.8),
        dial_hands_rgb,
        pal.show_needle_shadows,
        s,
    );
    draw_horology_needle(
        rt,
        cx,
        cy,
        m_ang,
        face_r * 0.72,
        face_r * 0.18,
        (2.1 * s).max(1.4),
        dial_hands_rgb,
        pal.show_needle_shadows,
        s,
    );
    if pal.theme == HorologyTheme::BreguetPorcelain && r >= 24.0 * s {
        // Signature Breguet open-tipped "pomme" hollow circles near the blued-steel hand tips!
        if let Ok(bl) = rt.CreateSolidColorBrush(&rgb_u32(dial_hands_rgb, 0.96), None) {
            rt.DrawEllipse(
                &D2D1_ELLIPSE {
                    point: Vector2 { X: cx + face_r * 0.36 * h_ang.cos(), Y: cy + face_r * 0.36 * h_ang.sin() },
                    radiusX: 2.3 * s,
                    radiusY: 2.3 * s,
                },
                &bl,
                1.2 * s,
                None,
            );
            rt.DrawEllipse(
                &D2D1_ELLIPSE {
                    point: Vector2 { X: cx + face_r * 0.54 * m_ang.cos(), Y: cy + face_r * 0.54 * m_ang.sin() },
                    radiusX: 2.0 * s,
                    radiusY: 2.0 * s,
                },
                &bl,
                1.1 * s,
                None,
            );
        }
    }
    if let Some(sec) = second {
        let s_ang = (sec * 6.0 - 90.0).to_radians();
        draw_horology_needle(
            rt,
            cx,
            cy,
            s_ang,
            face_r * 0.80,
            face_r * 0.24,
            (1.1 * s).max(0.8),
            pal.lume_rgb,
            pal.show_needle_shadows,
            s,
        );
    }

    // 6. Central metallic pinion & ruby jewel cap
    if let Ok(cap) = rt.CreateSolidColorBrush(&rgb_u32(pal.bezel_hi_rgb, 0.98), None) {
        rt.FillEllipse(
            &D2D1_ELLIPSE {
                point: Vector2 { X: cx, Y: cy },
                radiusX: (3.2 * s).max(2.0),
                radiusY: (3.2 * s).max(2.0),
            },
            &cap,
        );
    }
    if let Ok(jew) = rt.CreateSolidColorBrush(&rgb_u32(pal.jewel_rgb, 0.96), None) {
        rt.FillEllipse(
            &D2D1_ELLIPSE {
                point: Vector2 { X: cx, Y: cy },
                radiusX: (1.4 * s).max(0.9),
                radiusY: (1.4 * s).max(0.9),
            },
            &jew,
        );
    }

    // 7. Curved sapphire dome specular crescent over watch dial
    if pal.glare_alpha > 0.02 {
        let g_stops = [
            D2D1_GRADIENT_STOP {
                position: 0.0,
                color: rgba(255, 255, 255, (0.28 * pal.glare_alpha).min(0.32)),
            },
            D2D1_GRADIENT_STOP {
                position: 0.45,
                color: rgba(255, 255, 255, 0.0),
            },
            D2D1_GRADIENT_STOP {
                position: 1.0,
                color: rgba(160, 205, 255, (0.10 * pal.glare_alpha).min(0.14)),
            },
        ];
        if let Some(gg) = make_lin_grad(
            rt,
            (cx - face_r, cy - face_r),
            (cx + face_r, cy + face_r),
            &g_stops,
        ) {
            rt.FillEllipse(
                &D2D1_ELLIPSE {
                    point: Vector2 { X: cx, Y: cy },
                    radiusX: face_r,
                    radiusY: face_r,
                },
                &gg,
            );
        }
    }
}

/// Draws a recessed beveled horology aperture (date window / stat readout cutout) with
/// theme-authentic framing (porcelain cartouche, neon-orange carbon cutout, prism glass,
/// brass-rimmed walnut plaque, aviation instrument drum, or cyclops magnifying dome).
pub unsafe fn draw_horology_aperture(
    rt: &ID2D1RenderTarget,
    rect: D2D_RECT_F,
    radius: f32,
    s: f32,
    pal: &ResolvedHorologyPalette,
    cyclops_glint: bool,
) {
    let eff_r = match pal.theme {
        HorologyTheme::IwcPilotTitanium | HorologyTheme::ForgedCarbonMoto => (radius * 0.35).max(1.5 * s),
        HorologyTheme::BreguetPorcelain | HorologyTheme::WalnutRegulator => (radius * 0.55).max(2.0 * s),
        _ => radius,
    };
    let rr = D2D1_ROUNDED_RECT { rect, radiusX: eff_r, radiusY: eff_r };
    let (ap_bg, ap_a) = match pal.theme {
        HorologyTheme::SapphireObsidian => (0x10223A, 0.58),
        HorologyTheme::BreguetPorcelain => (0xF3ECDC, 0.96),
        HorologyTheme::WalnutRegulator => (0x241307, 0.94),
        HorologyTheme::DaytonaCeramic => (0x0A0C10, 0.96),
        _ if pal.is_light_dial => (darken_rgb(pal.dial_rgb, 0.08), 0.95),
        _ => (darken_rgb(pal.dial_rgb, 0.48), 0.94),
    };
    if let Ok(bg) = rt.CreateSolidColorBrush(&rgb_u32(ap_bg, ap_a), None) {
        rt.FillRoundedRectangle(&rr, &bg);
    }
    let rim_stops = match pal.theme {
        HorologyTheme::SapphireObsidian => [
            D2D1_GRADIENT_STOP { position: 0.0, color: rgba(110, 232, 255, 0.72) },
            D2D1_GRADIENT_STOP { position: 1.0, color: rgba(156, 120, 255, 0.65) },
        ],
        HorologyTheme::ForgedCarbonMoto => [
            D2D1_GRADIENT_STOP { position: 0.0, color: rgb_u32(pal.lume_rgb, 0.92) },
            D2D1_GRADIENT_STOP { position: 1.0, color: rgba(70, 76, 88, 0.85) },
        ],
        HorologyTheme::IwcPilotTitanium => [
            D2D1_GRADIENT_STOP { position: 0.0, color: rgb_u32(pal.lume_rgb, 0.62) },
            D2D1_GRADIENT_STOP { position: 1.0, color: rgb_u32(pal.bezel_hi_rgb, 0.80) },
        ],
        _ => [
            D2D1_GRADIENT_STOP { position: 0.0, color: rgb_u32(pal.bezel_lo_rgb, 0.90) },
            D2D1_GRADIENT_STOP { position: 1.0, color: rgb_u32(pal.bezel_hi_rgb, 0.88) },
        ],
    };
    if let Some(rg) = make_lin_grad(rt, (rect.left, rect.top), (rect.right, rect.bottom), &rim_stops)
    {
        rt.DrawRoundedRectangle(&rr, &rg, (1.15 * s).max(0.85), None);
    }
    if pal.theme == HorologyTheme::ForgedCarbonMoto {
        // Blazing orange motorsport bottom underline bar
        if let Ok(ul) = rt.CreateSolidColorBrush(&rgb_u32(pal.lume_rgb, 0.90), None) {
            rt.DrawLine(
                Vector2 { X: rect.left + 4.0 * s, Y: rect.bottom - 1.2 * s },
                Vector2 { X: rect.right - 4.0 * s, Y: rect.bottom - 1.2 * s },
                &ul,
                1.6 * s,
                None,
            );
        }
    }
    if cyclops_glint && pal.glare_alpha > 0.05 {
        let lens = D2D1_ROUNDED_RECT {
            rect: D2D_RECT_F {
                left: rect.left + 1.5 * s,
                top: rect.top + 1.2 * s,
                right: (rect.right - 1.5 * s).max(rect.left + 2.0),
                bottom: rect.top + (rect.bottom - rect.top) * 0.46,
            },
            radiusX: (eff_r - 1.0 * s).max(1.0),
            radiusY: (eff_r - 1.0 * s).max(1.0),
        };
        let gl_a = if pal.theme == HorologyTheme::SubmarinerEmerald {
            (0.26 * pal.glare_alpha).min(0.32)
        } else {
            (0.16 * pal.glare_alpha).min(0.22)
        };
        if let Ok(gl) = rt.CreateSolidColorBrush(&rgba(255, 255, 255, gl_a), None) {
            rt.FillRoundedRectangle(&lens, &gl);
        }
    }
}

/// Draws a tactile chamfered horology push-button / crown tile with 3D metallic top face,
/// press depression on active/hover, and optional luminous jewel status indicator pip.
#[allow(clippy::too_many_arguments)]
pub unsafe fn draw_horology_pusher(
    rt: &ID2D1RenderTarget,
    rect: D2D_RECT_F,
    radius: f32,
    s: f32,
    pal: &ResolvedHorologyPalette,
    active: bool,
    hovered: bool,
    show_jewel: bool,
) {
    let press = if active { 0.8 * s } else { 0.0 };
    let r_btn = D2D_RECT_F {
        left: rect.left,
        top: rect.top + press,
        right: rect.right,
        bottom: rect.bottom + press,
    };
    let eff_rad = match pal.theme {
        HorologyTheme::IwcPilotTitanium => radius.min(4.0 * s),
        HorologyTheme::ForgedCarbonMoto => radius.min(5.0 * s),
        HorologyTheme::WalnutRegulator => radius.min(6.0 * s),
        _ => radius,
    };
    let rr = D2D1_ROUNDED_RECT {
        rect: r_btn,
        radiusX: eff_rad,
        radiusY: eff_rad,
    };
    // Subtle physical drop shadow beneath unpressed crown pusher (skipped for frameless crystal)
    if !active && pal.theme != HorologyTheme::SapphireObsidian {
        let sh_rr = D2D1_ROUNDED_RECT {
            rect: D2D_RECT_F {
                left: rect.left,
                top: rect.top + 1.4 * s,
                right: rect.right,
                bottom: rect.bottom + 2.0 * s,
            },
            radiusX: eff_rad,
            radiusY: eff_rad,
        };
        if let Ok(sh) = rt.CreateSolidColorBrush(&rgba(0, 0, 0, if pal.is_light_dial { 0.16 } else { 0.36 }), None) {
            rt.FillRoundedRectangle(&sh_rr, &sh);
        }
    }
    let (top_c, bot_c, a_top, a_bot) = match pal.theme {
        HorologyTheme::SapphireObsidian => {
            if active {
                (pal.lume_rgb, 0x7A48FF, 0.34, 0.22)
            } else if hovered {
                (0x284A7A, 0x162242, 0.42, 0.30)
            } else {
                (0x162442, 0x0C1328, 0.32, 0.22)
            }
        }
        HorologyTheme::BreguetPorcelain => {
            if active {
                (0xD8E4F8, 0xC4D6F2, 0.96, 0.96)
            } else if hovered {
                (0xFFFFFF, 0xEFE8D6, 0.98, 0.98)
            } else {
                (0xFCF9F0, 0xEDE5D2, 0.96, 0.96)
            }
        }
        HorologyTheme::WalnutRegulator => {
            if active {
                (0x4E3422, 0x2F1E12, 0.96, 0.98)
            } else if hovered {
                (0x422B1B, 0x28180E, 0.96, 0.98)
            } else {
                (0x342114, 0x1E120B, 0.96, 0.98)
            }
        }
        HorologyTheme::DaytonaCeramic => {
            if active {
                (0x2A1418, 0x160A0D, 0.96, 0.98)
            } else if hovered {
                (0x282A30, 0x16181C, 0.96, 0.98)
            } else {
                (0x1B1D22, 0x0F1014, 0.96, 0.98)
            }
        }
        _ => {
            if active {
                (lighten_rgb(pal.lume_rgb, 0.14), darken_rgb(pal.lume_rgb, 0.28), 0.34, 0.24)
            } else if hovered {
                (lighten_rgb(pal.dial_hi_rgb, 0.22), pal.dial_rgb, 0.94, 0.96)
            } else if pal.is_light_dial {
                (0xFAF7F0, 0xE2DCCF, 0.96, 0.96)
            } else {
                (lighten_rgb(pal.dial_rgb, 0.14), darken_rgb(pal.dial_rgb, 0.32), 0.94, 0.96)
            }
        }
    };
    let stops = [
        D2D1_GRADIENT_STOP { position: 0.0, color: rgb_u32(top_c, a_top) },
        D2D1_GRADIENT_STOP { position: 1.0, color: rgb_u32(bot_c, a_bot) },
    ];
    if let Some(bg) = make_lin_grad(rt, (r_btn.left, r_btn.top), (r_btn.left, r_btn.bottom), &stops) {
        rt.FillRoundedRectangle(&rr, &bg);
    }
    let rim_col = if active || hovered {
        rgb_u32(pal.lume_rgb, 0.94)
    } else if pal.theme == HorologyTheme::SapphireObsidian {
        rgb_u32(0x66EEFF, 0.42)
    } else if pal.theme == HorologyTheme::BreguetPorcelain {
        rgb_u32(0x1B4FD8, 0.48)
    } else if pal.theme == HorologyTheme::WalnutRegulator {
        rgb_u32(0xDFB158, 0.78)
    } else if pal.theme == HorologyTheme::DaytonaCeramic {
        rgb_u32(0xCBD2DC, 0.78)
    } else {
        rgb_u32(pal.bezel_rgb, 0.65)
    };
    let rim_w = if pal.theme == HorologyTheme::WalnutRegulator {
        (1.6 * s).max(1.1)
    } else {
        (1.15 * s).max(0.85)
    };
    if let Ok(rb) = rt.CreateSolidColorBrush(&rim_col, None) {
        rt.DrawRoundedRectangle(&rr, &rb, rim_w, None);
    }
    // Theme-signature accent strips on tactile pushers
    match pal.theme {
        HorologyTheme::ForgedCarbonMoto => {
            if let Ok(ob) = rt.CreateSolidColorBrush(&rgb_u32(0xFF5E1A, if active { 0.98 } else { 0.48 }), None) {
                rt.FillRectangle(
                    &D2D_RECT_F {
                        left: r_btn.left + 3.0 * s,
                        top: r_btn.bottom - 2.4 * s,
                        right: r_btn.right - 3.0 * s,
                        bottom: r_btn.bottom - 0.8 * s,
                    },
                    &ob,
                );
            }
        }
        HorologyTheme::IwcPilotTitanium if active => {
            if let Ok(ab) = rt.CreateSolidColorBrush(&rgb_u32(0xFF7A1A, 0.95), None) {
                rt.FillRectangle(
                    &D2D_RECT_F {
                        left: r_btn.left + 1.5 * s,
                        top: r_btn.top + 3.5 * s,
                        right: r_btn.left + 3.8 * s,
                        bottom: r_btn.bottom - 3.5 * s,
                    },
                    &ab,
                );
            }
        }
        _ => {}
    }
    if show_jewel {
        let jx = r_btn.right - 6.5 * s;
        let jy = r_btn.top + 6.5 * s;
        let jr = (2.2 * s).max(1.4);
        let j_col = if active {
            rgb_u32(
                if matches!(pal.theme, HorologyTheme::DaytonaCeramic | HorologyTheme::TourbillonGold) {
                    pal.jewel_rgb
                } else {
                    pal.lume_rgb
                },
                0.98,
            )
        } else {
            rgb_u32(pal.bezel_lo_rgb, 0.55)
        };
        if let Ok(jb) = rt.CreateSolidColorBrush(&j_col, None) {
            rt.FillEllipse(
                &D2D1_ELLIPSE {
                    point: Vector2 { X: jx, Y: jy },
                    radiusX: jr,
                    radiusY: jr,
                },
                &jb,
            );
        }
    }
}

/// Draws a recessed machined horology linear slider track with luminous fill channel
/// and a knurled metallic fader thumb with grip ribs & directional shadow.
#[allow(clippy::too_many_arguments)]
pub unsafe fn draw_horology_slider(
    rt: &ID2D1RenderTarget,
    rect: D2D_RECT_F,
    s: f32,
    pal: &ResolvedHorologyPalette,
    frac: f32,
    hovered: bool,
) {
    let f = frac.clamp(0.0, 1.0);
    let w = (rect.right - rect.left).max(8.0);
    let h = (rect.bottom - rect.top).max(4.0);
    let trk_h = (h * 0.42).clamp(4.0 * s, 9.0 * s);
    let cy = (rect.top + rect.bottom) * 0.5;
    let trk_rr = D2D1_ROUNDED_RECT {
        rect: D2D_RECT_F {
            left: rect.left,
            top: cy - trk_h * 0.5,
            right: rect.right,
            bottom: cy + trk_h * 0.5,
        },
        radiusX: trk_h * 0.5,
        radiusY: trk_h * 0.5,
    };
    // Recessed channel cavity
    if let Ok(cav) = rt.CreateSolidColorBrush(
        &rgba(0, 0, 0, if pal.is_light_dial { 0.16 } else { 0.56 }),
        None,
    ) {
        rt.FillRoundedRectangle(&trk_rr, &cav);
    }
    // Luminous accent fill
    let fill_w = (w * f).max(trk_h * 0.5);
    if f > 0.01 {
        let fill_rr = D2D1_ROUNDED_RECT {
            rect: D2D_RECT_F {
                left: rect.left,
                top: cy - trk_h * 0.5,
                right: (rect.left + fill_w).min(rect.right),
                bottom: cy + trk_h * 0.5,
            },
            radiusX: trk_h * 0.5,
            radiusY: trk_h * 0.5,
        };
        let f_stops = [
            D2D1_GRADIENT_STOP {
                position: 0.0,
                color: rgb_u32(darken_rgb(pal.lume_rgb, 0.25), 0.94),
            },
            D2D1_GRADIENT_STOP {
                position: 1.0,
                color: rgb_u32(lighten_rgb(pal.lume_rgb, 0.20), 0.98),
            },
        ];
        if let Some(fg) = make_lin_grad(rt, (rect.left, cy), (rect.left + fill_w, cy), &f_stops) {
            rt.FillRoundedRectangle(&fill_rr, &fg);
        }
    }
    // Beveled channel rim
    if let Ok(rb) = rt.CreateSolidColorBrush(&rgb_u32(pal.bezel_rgb, 0.52), None) {
        rt.DrawRoundedRectangle(&trk_rr, &rb, (0.85 * s).max(0.65), None);
    }

    // Knurled metallic cylindrical slider thumb
    let tw = (12.0 * s).min(w * 0.22);
    let th = (trk_h + 6.5 * s).min(h.max(11.0 * s));
    let tx = (rect.left + w * f).clamp(rect.left + tw * 0.5, rect.right - tw * 0.5);
    let t_rect = D2D_RECT_F {
        left: tx - tw * 0.5,
        top: cy - th * 0.5,
        right: tx + tw * 0.5,
        bottom: cy + th * 0.5,
    };
    let t_rr = D2D1_ROUNDED_RECT {
        rect: t_rect,
        radiusX: 3.0 * s,
        radiusY: 3.0 * s,
    };
    if let Ok(sh) = rt.CreateSolidColorBrush(&rgba(0, 0, 0, 0.44), None) {
        rt.FillRoundedRectangle(
            &D2D1_ROUNDED_RECT {
                rect: D2D_RECT_F {
                    left: t_rect.left + 0.8 * s,
                    top: t_rect.top + 1.4 * s,
                    right: t_rect.right + 1.0 * s,
                    bottom: t_rect.bottom + 1.8 * s,
                },
                radiusX: 3.0 * s,
                radiusY: 3.0 * s,
            },
            &sh,
        );
    }
    let th_stops = [
        D2D1_GRADIENT_STOP {
            position: 0.0,
            color: rgb_u32(pal.bezel_hi_rgb, 0.99),
        },
        D2D1_GRADIENT_STOP {
            position: 0.5,
            color: rgb_u32(pal.bezel_rgb, 0.98),
        },
        D2D1_GRADIENT_STOP {
            position: 1.0,
            color: rgb_u32(pal.bezel_lo_rgb, 0.99),
        },
    ];
    if let Some(tg) = make_lin_grad(rt, (t_rect.left, t_rect.top), (t_rect.left, t_rect.bottom), &th_stops)
    {
        rt.FillRoundedRectangle(&t_rr, &tg);
    }
    // 3 vertical knurled grip ribs
    if let Ok(rib) = rt.CreateSolidColorBrush(&rgba(0, 0, 0, 0.45), None) {
        for dx in [-2.4 * s, 0.0, 2.4 * s] {
            rt.DrawLine(
                Vector2 { X: tx + dx, Y: t_rect.top + 2.5 * s },
                Vector2 { X: tx + dx, Y: t_rect.bottom - 2.5 * s },
                &rib,
                (0.75 * s).max(0.6),
                None,
            );
        }
    }
    if hovered {
        if let Ok(hb) = rt.CreateSolidColorBrush(&rgb_u32(pal.lume_rgb, 0.92), None) {
            rt.DrawRoundedRectangle(&t_rr, &hb, (1.1 * s).max(0.8), None);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn horology_theme_and_parts_round_trip() {
        assert_eq!(HorologyTheme::ALL.len(), 10);
        for (i, t) in HorologyTheme::ALL.iter().enumerate() {
            assert_eq!(t.idx(), i);
            assert_eq!(HorologyTheme::from_idx(i), Some(*t));
            assert_eq!(HorologyTheme::from_str(t.as_str()), Some(*t));
            assert!(!t.label().is_empty());
            assert!(!t.subtitle().is_empty());
            assert_eq!(t.has_idle_motion(), *t == HorologyTheme::TourbillonGold);
        }
        let custom = HorologyPartConfig {
            bezel_finish: Some(HorologyBezelFinish::RoseGold),
            bezel_rgb: Some(0xDDA674),
            dial_finish: Some(HorologyDialFinish::TapisserieGuilloche),
            dial_rgb: Some(0x112233),
            hands_rgb: Some(0xFFEEDD),
            lume_rgb: Some(0x55EEFF),
            glare_pct: 42,
            show_screws: false,
            show_needle_shadows: false,
        };
        let csv = custom.to_csv();
        let parsed = HorologyPartConfig::from_csv(&csv);
        assert_eq!(parsed, custom);
        let pal = parsed.resolve(HorologyTheme::RoyalOakSteel, None, None, None);
        assert_eq!(pal.bezel_finish, HorologyBezelFinish::RoseGold);
        assert_eq!(pal.dial_finish, HorologyDialFinish::TapisserieGuilloche);
        assert_eq!(pal.dial_rgb, 0x112233);
        assert!(!pal.show_screws);
        assert!(!pal.show_needle_shadows);
    }

    #[test]
    fn horology_bakers_produce_valid_premultiplied_bgra() {
        for fin in HorologyDialFinish::ALL {
            let px = dial_finish_pixels(fin);
            assert_eq!(px.len(), (TEX_W * TEX_H * 4) as usize);
            let mut non_zero = 0usize;
            for chunk in px.chunks_exact(4) {
                let (b, g, r, a) = (chunk[0], chunk[1], chunk[2], chunk[3]);
                assert!(b <= a && g <= a && r <= a, "premultiplied invariant violated on {:?}", fin);
                if a > 0 {
                    non_zero += 1;
                }
            }
            assert!(non_zero > 1000, "finish {:?} baked empty", fin);
        }
        let lp = lunar_phase_0_to_1(2026, 11, 27);
        assert!((0.0..=1.0).contains(&lp));
    }
}
