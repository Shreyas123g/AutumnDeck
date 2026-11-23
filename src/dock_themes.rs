use std::cell::RefCell;
use std::f32::consts::{FRAC_PI_2, PI};

use windows::core::{Interface, PCWSTR};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D1_COLOR_F, D2D1_FIGURE_BEGIN_FILLED, D2D1_FIGURE_BEGIN_HOLLOW, D2D1_FIGURE_END_CLOSED,
    D2D1_FIGURE_END_OPEN, D2D1_GRADIENT_STOP, D2D_RECT_F, D2D_SIZE_F,
};
use windows::Win32::Graphics::Direct2D::{
    ID2D1Bitmap, ID2D1BitmapBrush, ID2D1DCRenderTarget, ID2D1Factory, ID2D1Geometry, ID2D1Layer,
    ID2D1LinearGradientBrush, ID2D1PathGeometry, ID2D1RenderTarget, ID2D1SolidColorBrush,
    ID2D1StrokeStyle, D2D1_ANTIALIAS_MODE_PER_PRIMITIVE, D2D1_ARC_SEGMENT, D2D1_ARC_SIZE_LARGE,
    D2D1_ARC_SIZE_SMALL, D2D1_BITMAP_BRUSH_PROPERTIES, D2D1_BITMAP_INTERPOLATION_MODE_LINEAR,
    D2D1_CAP_STYLE_ROUND, D2D1_DRAW_TEXT_OPTIONS_CLIP, D2D1_DRAW_TEXT_OPTIONS_NONE, D2D1_ELLIPSE,
    D2D1_EXTEND_MODE_CLAMP, D2D1_GAMMA_2_2, D2D1_LAYER_OPTIONS_NONE, D2D1_LAYER_PARAMETERS,
    D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES, D2D1_ROUNDED_RECT, D2D1_STROKE_STYLE_PROPERTIES,
    D2D1_SWEEP_DIRECTION_CLOCKWISE,
};
use windows::Win32::Graphics::DirectWrite::{
    IDWriteFactory, IDWriteFontCollection, IDWriteTextFormat, IDWriteTextLayout,
    DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_WEIGHT,
    DWRITE_FONT_WEIGHT_BOLD, DWRITE_FONT_WEIGHT_REGULAR, DWRITE_FONT_WEIGHT_SEMI_BOLD,
    DWRITE_MEASURING_MODE_NATURAL, DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
    DWRITE_PARAGRAPH_ALIGNMENT_NEAR, DWRITE_TEXT_ALIGNMENT_CENTER,
    DWRITE_TEXT_ALIGNMENT_LEADING, DWRITE_TRIMMING, DWRITE_TRIMMING_GRANULARITY_CHARACTER,
    DWRITE_WORD_WRAPPING_NO_WRAP,
};
use windows_numerics::{Matrix3x2, Vector2};

use crate::config::{
    AppConfig, DockClockEngine, DockDividerStyle, DockSlotCardStyle, HudWingMode, MacDockEdge,
    TaskbarTheme,
};
pub use crate::config::{
    DockArtLayer, DockIndicatorStyle, DockLayoutKind, DockShelfShape,
};
use crate::theme::ThemePalette;

pub unsafe fn draw_d2d_text(
    rt: &ID2D1DCRenderTarget,
    text: &str,
    tf: &IDWriteTextFormat,
    rect: &D2D_RECT_F,
    br: &ID2D1SolidColorBrush,
) {
    let mut stack = [0u16; 96];
    let mut n = 0usize;
    let mut overflow: Option<Vec<u16>> = None;
    for cu in text.encode_utf16() {
        if let Some(ref mut v) = overflow {
            v.push(cu);
        } else if n < stack.len() {
            stack[n] = cu;
            n += 1;
        } else {
            let mut v = Vec::with_capacity(128);
            v.extend_from_slice(&stack[..n]);
            v.push(cu);
            overflow = Some(v);
        }
    }
    let slice = match overflow {
        Some(ref v) => v.as_slice(),
        None => &stack[..n],
    };
    rt.DrawText(
        slice,
        tf,
        rect,
        br,
        windows::Win32::Graphics::Direct2D::D2D1_DRAW_TEXT_OPTIONS_CLIP,
        DWRITE_MEASURING_MODE_NATURAL,
    );
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum TaskbarWidgetId {
    ShelfFrame,
    AppDock,
    HudWing,
    SystemTray,
    Clock,
    Utilities,
}

#[derive(Clone, Copy)]
pub struct DockVisualSpec {
    pub layout: DockLayoutKind,
    pub shape: DockShelfShape,
    pub indicator: DockIndicatorStyle,
    pub art: DockArtLayer,
    pub bg_top: (u8, u8, u8),
    pub bg_mid: (u8, u8, u8),
    pub bg_bot: (u8, u8, u8),
    pub bg_alpha_bias: f32,
    pub rim_outer: (u8, u8, u8, f32),
    pub rim_inner: (u8, u8, u8, f32),
    pub divider: (u8, u8, u8, f32),
    pub dot_active: (u8, u8, u8),
    pub dot_idle: (u8, u8, u8),
    pub dot_glow: (u8, u8, u8),
    pub tip_bg: (u8, u8, u8, f32),
    pub tip_border: (u8, u8, u8, f32),
    pub tip_fg: (u8, u8, u8, f32),
    pub ambient_animated: bool,
    pub light_mode_paper: bool,
    pub mechanical_lift: bool,
}

#[inline]
pub fn unpack_rgb(rgb: u32) -> (u8, u8, u8) {
    (
        ((rgb >> 16) & 0xFF) as u8,
        ((rgb >> 8) & 0xFF) as u8,
        (rgb & 0xFF) as u8,
    )
}

pub fn resolve_shelf_spec(cfg: &AppConfig) -> DockVisualSpec {
    let base_theme = cfg.w_shelf.theme_override.unwrap_or(cfg.tb_theme);
    let mut s = dock_spec(base_theme);
    if let Some(lo) = cfg.w_shelf.layout_override {
        s.layout = lo;
    }
    if let Some(sh) = cfg.w_shelf.shape_override {
        s.shape = sh;
    }
    if let Some(art) = cfg.w_shelf.art_override {
        s.art = art;
    }
    let glass_k = (cfg.w_shelf.glass_alpha_pct as f32 / 100.0).clamp(0.10, 1.0);
    s.bg_alpha_bias = (s.bg_alpha_bias * glass_k).clamp(0.0, 0.98);
    let rim_k = (cfg.w_shelf.rim_alpha_pct as f32 / 100.0).clamp(0.0, 1.0);
    s.rim_outer.3 = (s.rim_outer.3 * rim_k).clamp(0.0, 1.0);
    s.rim_inner.3 = (s.rim_inner.3 * rim_k).clamp(0.0, 1.0);
    s
}

pub fn resolve_widget_spec(cfg: &AppConfig, widget: TaskbarWidgetId) -> (TaskbarTheme, DockVisualSpec) {
    let shelf = resolve_shelf_spec(cfg);
    match widget {
        TaskbarWidgetId::ShelfFrame => {
            let t = cfg.w_shelf.theme_override.unwrap_or(cfg.tb_theme);
            (t, shelf)
        }
        TaskbarWidgetId::AppDock => {
            let t = cfg.w_apps.theme_override.unwrap_or(cfg.tb_theme);
            let mut s = dock_spec(t);
            s.layout = shelf.layout;
            if let Some(ind) = cfg.w_apps.indicator_override {
                s.indicator = ind;
            }
            if let Some(rgb) = cfg.w_apps.custom_glow_rgb {
                let c = unpack_rgb(rgb);
                s.dot_active = c;
                s.dot_glow = c;
            }
            (t, s)
        }
        TaskbarWidgetId::HudWing => {
            let t = cfg.w_hud.theme_override.unwrap_or(cfg.tb_theme);
            let s = dock_spec(t);
            (t, s)
        }
        TaskbarWidgetId::SystemTray => {
            let t = cfg.w_tray.theme_override.unwrap_or(cfg.tb_theme);
            let mut s = dock_spec(t);
            if let Some(rgb) = cfg.w_tray.custom_accent_rgb {
                let c = unpack_rgb(rgb);
                s.dot_active = c;
                s.dot_glow = c;
            }
            (t, s)
        }
        TaskbarWidgetId::Clock => {
            let t = cfg.w_clock.theme_override.unwrap_or(cfg.tb_theme);
            let mut s = dock_spec(t);
            if let Some(rgb) = cfg.w_clock.custom_accent_rgb {
                let c = unpack_rgb(rgb);
                s.dot_active = c;
                s.dot_glow = c;
            }
            (t, s)
        }
        TaskbarWidgetId::Utilities => {
            let t = cfg.w_util.theme_override.unwrap_or(cfg.tb_theme);
            let mut s = dock_spec(t);
            s.layout = shelf.layout;
            (t, s)
        }
    }
}

fn is_art_ambient_live(art: DockArtLayer) -> bool {
    matches!(
        art,
        DockArtLayer::CyberHud
            | DockArtLayer::AquaReflect
            | DockArtLayer::AuroraSilk
            | DockArtLayer::LiquidChrome
            | DockArtLayer::SakuraPetals
            | DockArtLayer::PhosphorCrt
            | DockArtLayer::ObsidianMagma
            | DockArtLayer::StarfieldWarp
            | DockArtLayer::CrystalWater
            | DockArtLayer::RainWindowGlass
            | DockArtLayer::OceanTideFoam
            | DockArtLayer::ZenCalmMist
            | DockArtLayer::TranquilKoiPond
            | DockArtLayer::ForestSunbeams
            | DockArtLayer::GlacierCrystalIce
            | DockArtLayer::AbyssalTrench
            | DockArtLayer::GoldenHourHorizon
    )
}

pub fn is_ambient_animated(theme: TaskbarTheme) -> bool {
    let s = dock_spec(theme);
    s.ambient_animated && is_art_ambient_live(s.art)
}

pub fn is_ambient_animated_cfg(cfg: &AppConfig) -> bool {
    if !cfg.tb_ambient_anim {
        return false;
    }
    let s = resolve_shelf_spec(cfg);
    let is_horizontal = !matches!(cfg.mac_dock_edge, MacDockEdge::Left | MacDockEdge::Right);
    let hud_wing_visible = is_horizontal
        && (cfg.w_hud.mode == HudWingMode::AlwaysEnabled
            || (cfg.w_hud.mode == HudWingMode::InheritTheme
                && s.layout == DockLayoutKind::HudConsoleDeck));
    is_ambient_animated(cfg.tb_theme)
        || (s.ambient_animated && is_art_ambient_live(s.art))
        || (cfg.tb_theme == TaskbarTheme::BiolumAbyssDock
            && matches!(s.layout, DockLayoutKind::IndependentOrbs))
        || (hud_wing_visible && cfg.w_hud.marquee_ticker)
}

pub fn dock_palette(theme: TaskbarTheme) -> Option<ThemePalette> {
    let p = match theme {
        TaskbarTheme::QuartzShellDock => ThemePalette {
            accent: (206, 228, 252),
            accent_light: (250, 252, 255),
            accent_deep: (92, 138, 204),
            rim: (232, 240, 252),
            text_primary: (246, 249, 255),
            text_muted: (196, 210, 232),
            glass_top: (34, 40, 52),
            glass_bot: (18, 22, 32),
            preview_a: (20, 38, 70),
            preview_b: (56, 102, 162),
            preview_c: (14, 24, 46),
            swatches: [0xCEE4FC, 0xFAFCFF, 0x5C8ACC, 0x3866A2, 0x0E182E],
        },
        TaskbarTheme::CyberpunkHudDock => ThemePalette {
            accent: (0, 240, 255),
            accent_light: (255, 58, 128),
            accent_deep: (255, 184, 0),
            rim: (0, 240, 255),
            text_primary: (238, 252, 255),
            text_muted: (154, 214, 226),
            glass_top: (14, 18, 28),
            glass_bot: (8, 10, 18),
            preview_a: (16, 12, 30),
            preview_b: (64, 18, 52),
            preview_c: (8, 16, 30),
            swatches: [0x00F0FF, 0xFF3A80, 0xFFB800, 0x401234, 0x08101E],
        },
        TaskbarTheme::DynamicCapsuleDock => ThemePalette {
            accent: (94, 234, 212),
            accent_light: (224, 246, 255),
            accent_deep: (56, 78, 106),
            rim: (188, 206, 230),
            text_primary: (246, 248, 252),
            text_muted: (166, 180, 202),
            glass_top: (12, 14, 20),
            glass_bot: (6, 8, 12),
            preview_a: (14, 18, 30),
            preview_b: (28, 42, 66),
            preview_c: (8, 10, 16),
            swatches: [0x5EEAD4, 0xE0F6FF, 0x384E6A, 0x1C2A42, 0x080A10],
        },
        TaskbarTheme::FrutigerAquaDock => ThemePalette {
            accent: (82, 224, 255),
            accent_light: (234, 253, 255),
            accent_deep: (18, 134, 206),
            rim: (186, 244, 255),
            text_primary: (244, 252, 255),
            text_muted: (184, 232, 250),
            glass_top: (18, 92, 142),
            glass_bot: (10, 48, 94),
            preview_a: (14, 96, 164),
            preview_b: (58, 190, 238),
            preview_c: (10, 56, 108),
            swatches: [0x52E0FF, 0xEAFDFF, 0x1286CE, 0x3ABEEE, 0x0A386C],
        },
        TaskbarTheme::InvisibleGhostDock => ThemePalette {
            accent: (212, 234, 255),
            accent_light: (255, 255, 255),
            accent_deep: (120, 158, 206),
            rim: (224, 240, 255),
            text_primary: (248, 251, 255),
            text_muted: (202, 218, 238),
            glass_top: (22, 28, 42),
            glass_bot: (12, 16, 26),
            preview_a: (20, 32, 54),
            preview_b: (48, 72, 114),
            preview_c: (12, 18, 34),
            swatches: [0xD4EAFF, 0xFFFFFF, 0x789ECE, 0x304872, 0x0C1222],
        },
        TaskbarTheme::AuroraRibbonDock => ThemePalette {
            accent: (64, 242, 186),
            accent_light: (168, 120, 255),
            accent_deep: (42, 186, 255),
            rim: (132, 245, 212),
            text_primary: (242, 252, 250),
            text_muted: (176, 224, 214),
            glass_top: (16, 28, 36),
            glass_bot: (10, 14, 26),
            preview_a: (12, 42, 52),
            preview_b: (48, 28, 86),
            preview_c: (8, 22, 34),
            swatches: [0x40F2BA, 0xA878FF, 0x2ABAFF, 0x301C56, 0x081622],
        },
        TaskbarTheme::NextSlatePlankDock => ThemePalette {
            accent: (162, 184, 214),
            accent_light: (236, 242, 250),
            accent_deep: (74, 86, 108),
            rim: (192, 204, 224),
            text_primary: (242, 245, 250),
            text_muted: (174, 184, 200),
            glass_top: (42, 47, 56),
            glass_bot: (24, 28, 34),
            preview_a: (30, 36, 46),
            preview_b: (56, 64, 80),
            preview_c: (18, 22, 28),
            swatches: [0xA2B8D6, 0xECF2FA, 0x4A566C, 0x384050, 0x12161C],
        },
        TaskbarTheme::KumikoWoodDock => ThemePalette {
            accent: (234, 186, 108),
            accent_light: (255, 232, 188),
            accent_deep: (224, 82, 56),
            rim: (226, 178, 98),
            text_primary: (252, 242, 224),
            text_muted: (212, 184, 146),
            glass_top: (52, 32, 21),
            glass_bot: (28, 17, 11),
            preview_a: (58, 34, 20),
            preview_b: (108, 64, 34),
            preview_c: (30, 18, 12),
            swatches: [0xEABA6C, 0xFFE8BC, 0xE05238, 0x6C4022, 0x1E120C],
        },
        TaskbarTheme::LiquidChromeDock => ThemePalette {
            accent: (216, 226, 242),
            accent_light: (255, 255, 255),
            accent_deep: (110, 124, 148),
            rim: (232, 240, 252),
            text_primary: (248, 250, 255),
            text_muted: (192, 202, 220),
            glass_top: (56, 62, 74),
            glass_bot: (20, 24, 30),
            preview_a: (38, 44, 56),
            preview_b: (104, 116, 136),
            preview_c: (18, 22, 28),
            swatches: [0xD8E2F2, 0xFFFFFF, 0x6E7C94, 0x687488, 0x12161C],
        },
        TaskbarTheme::SakuraBreezeDock => ThemePalette {
            accent: (255, 162, 202),
            accent_light: (255, 224, 238),
            accent_deep: (188, 76, 134),
            rim: (255, 186, 216),
            text_primary: (255, 242, 248),
            text_muted: (224, 184, 206),
            glass_top: (46, 22, 42),
            glass_bot: (24, 12, 24),
            preview_a: (48, 20, 44),
            preview_b: (106, 42, 78),
            preview_c: (26, 12, 26),
            swatches: [0xFFA2CA, 0xFFE0EE, 0xBC4C86, 0x6A2A4E, 0x1A0C1A],
        },
        TaskbarTheme::SolsticeAstrolabeDock => ThemePalette {
            accent: (226, 182, 92),
            accent_light: (255, 236, 178),
            accent_deep: (92, 134, 212),
            rim: (224, 178, 88),
            text_primary: (250, 244, 226),
            text_muted: (194, 184, 160),
            glass_top: (18, 28, 56),
            glass_bot: (10, 15, 32),
            preview_a: (16, 28, 58),
            preview_b: (42, 62, 108),
            preview_c: (10, 16, 34),
            swatches: [0xE2B65C, 0xFFECB2, 0x5C86D4, 0x2A3E6C, 0x0A1022],
        },
        TaskbarTheme::VaporwaveHorizonDock => ThemePalette {
            accent: (255, 92, 198),
            accent_light: (255, 184, 96),
            accent_deep: (74, 226, 255),
            rim: (255, 108, 196),
            text_primary: (255, 240, 252),
            text_muted: (218, 166, 232),
            glass_top: (44, 14, 68),
            glass_bot: (20, 8, 38),
            preview_a: (52, 14, 78),
            preview_b: (158, 42, 114),
            preview_c: (20, 8, 42),
            swatches: [0xFF5CC6, 0xFFB860, 0x4AE2FF, 0x9E2A72, 0x14082A],
        },
        TaskbarTheme::EditorialPaperDock => ThemePalette {
            accent: (34, 32, 28),
            accent_light: (242, 238, 228),
            accent_deep: (196, 62, 42),
            rim: (44, 42, 38),
            text_primary: (28, 26, 23),
            text_muted: (96, 90, 82),
            glass_top: (242, 238, 228),
            glass_bot: (224, 218, 205),
            preview_a: (216, 210, 196),
            preview_b: (238, 234, 224),
            preview_c: (184, 178, 166),
            swatches: [0x22201C, 0xF2EEE4, 0xC43E2A, 0xDDD6C8, 0xB8B2A6],
        },
        TaskbarTheme::BiolumAbyssDock => ThemePalette {
            accent: (52, 244, 232),
            accent_light: (164, 134, 255),
            accent_deep: (22, 128, 196),
            rim: (72, 240, 228),
            text_primary: (234, 252, 255),
            text_muted: (154, 206, 222),
            glass_top: (8, 20, 42),
            glass_bot: (4, 10, 24),
            preview_a: (6, 22, 48),
            preview_b: (26, 72, 116),
            preview_c: (4, 12, 28),
            swatches: [0x34F4E8, 0xA486FF, 0x1680C4, 0x1A4874, 0x040C1C],
        },
        TaskbarTheme::TerminalPhosphorDock => ThemePalette {
            accent: (76, 255, 132),
            accent_light: (178, 255, 202),
            accent_deep: (28, 148, 72),
            rim: (72, 242, 124),
            text_primary: (206, 255, 220),
            text_muted: (116, 204, 144),
            glass_top: (8, 24, 14),
            glass_bot: (4, 12, 7),
            preview_a: (8, 26, 14),
            preview_b: (18, 64, 34),
            preview_c: (4, 14, 8),
            swatches: [0x4CFF84, 0xB2FFCA, 0x1C9448, 0x124022, 0x040E08],
        },
        TaskbarTheme::PrismRefractionDock => ThemePalette {
            accent: (128, 238, 255),
            accent_light: (255, 168, 236),
            accent_deep: (255, 214, 118),
            rim: (236, 246, 255),
            text_primary: (250, 252, 255),
            text_muted: (196, 214, 232),
            glass_top: (32, 38, 52),
            glass_bot: (18, 22, 32),
            preview_a: (24, 42, 72),
            preview_b: (72, 54, 102),
            preview_c: (16, 24, 42),
            swatches: [0x80EEFF, 0xFFA8EC, 0xFFD676, 0x483666, 0x10182A],
        },
        TaskbarTheme::ObsidianEmberDock => ThemePalette {
            accent: (255, 126, 48),
            accent_light: (255, 206, 98),
            accent_deep: (216, 46, 24),
            rim: (255, 118, 44),
            text_primary: (255, 242, 232),
            text_muted: (218, 174, 152),
            glass_top: (28, 18, 16),
            glass_bot: (14, 9, 8),
            preview_a: (36, 16, 12),
            preview_b: (96, 36, 18),
            preview_c: (16, 8, 8),
            swatches: [0xFF7E30, 0xFFCE62, 0xD82E18, 0x602412, 0x100808],
        },
        TaskbarTheme::BauhausGeometricDock => ThemePalette {
            accent: (244, 196, 48),
            accent_light: (62, 134, 242),
            accent_deep: (224, 62, 48),
            rim: (214, 212, 204),
            text_primary: (248, 246, 240),
            text_muted: (188, 186, 180),
            glass_top: (34, 34, 40),
            glass_bot: (20, 20, 24),
            preview_a: (32, 36, 48),
            preview_b: (64, 66, 82),
            preview_c: (20, 20, 26),
            swatches: [0xF4C430, 0x3E86F2, 0xE03E30, 0x404252, 0x14141A],
        },
        TaskbarTheme::StarfieldWarpDock => ThemePalette {
            accent: (136, 216, 255),
            accent_light: (246, 250, 255),
            accent_deep: (106, 104, 242),
            rim: (158, 216, 255),
            text_primary: (244, 248, 255),
            text_muted: (174, 196, 226),
            glass_top: (10, 14, 30),
            glass_bot: (5, 7, 16),
            preview_a: (8, 14, 34),
            preview_b: (34, 42, 86),
            preview_c: (4, 6, 18),
            swatches: [0x88D8FF, 0xF6FAFF, 0x6A68F2, 0x222A56, 0x040612],
        },
        TaskbarTheme::KatanaGlassDock => ThemePalette {
            accent: (220, 228, 238),
            accent_light: (246, 249, 252),
            accent_deep: (138, 148, 162),
            rim: (226, 232, 240),
            text_primary: (246, 248, 252),
            text_muted: (186, 194, 206),
            glass_top: (30, 33, 38),
            glass_bot: (15, 17, 21),
            preview_a: (28, 30, 34),
            preview_b: (72, 76, 84),
            preview_c: (14, 16, 20),
            swatches: [0xDCE4EE, 0xF6F9FC, 0x8A94A2, 0x484C54, 0x0F1115],
        },
        TaskbarTheme::BrushedTitaniumDock => ThemePalette {
            accent: (255, 118, 36),
            accent_light: (236, 240, 246),
            accent_deep: (96, 106, 120),
            rim: (218, 224, 232),
            text_primary: (246, 249, 252),
            text_muted: (188, 196, 208),
            glass_top: (76, 82, 92),
            glass_bot: (42, 46, 54),
            preview_a: (48, 54, 64),
            preview_b: (116, 124, 138),
            preview_c: (28, 32, 40),
            swatches: [0xFF7624, 0xECF0F6, 0x606A78, 0x747C8A, 0x1C2028],
        },
        TaskbarTheme::VacuumTubeHiFiDock => ThemePalette {
            accent: (255, 166, 62),
            accent_light: (255, 224, 154),
            accent_deep: (168, 92, 34),
            rim: (234, 192, 126),
            text_primary: (255, 242, 220),
            text_muted: (214, 186, 146),
            glass_top: (48, 34, 24),
            glass_bot: (22, 15, 10),
            preview_a: (38, 24, 16),
            preview_b: (114, 72, 36),
            preview_c: (18, 12, 8),
            swatches: [0xFFA63E, 0xFFE09A, 0xA85C22, 0x724824, 0x120C08],
        },
        TaskbarTheme::SwissHorologyDock => ThemePalette {
            accent: (112, 174, 255),
            accent_light: (236, 242, 252),
            accent_deep: (196, 26, 66),
            rim: (210, 222, 240),
            text_primary: (244, 248, 255),
            text_muted: (172, 190, 218),
            glass_top: (18, 30, 52),
            glass_bot: (10, 17, 32),
            preview_a: (14, 26, 48),
            preview_b: (42, 68, 114),
            preview_c: (8, 14, 28),
            swatches: [0x70AEFF, 0xECF2FC, 0xC41A42, 0x2A4472, 0x080E1C],
        },
        TaskbarTheme::SaddleLeatherDock => ThemePalette {
            accent: (224, 174, 92),
            accent_light: (250, 238, 214),
            accent_deep: (128, 74, 38),
            rim: (218, 176, 118),
            text_primary: (252, 244, 230),
            text_muted: (216, 190, 158),
            glass_top: (94, 54, 30),
            glass_bot: (54, 30, 16),
            preview_a: (68, 38, 22),
            preview_b: (132, 82, 46),
            preview_c: (38, 20, 12),
            swatches: [0xE0AE5C, 0xFAEED6, 0x804A26, 0x84522E, 0x26140C],
        },
        TaskbarTheme::ObsidianCarbonDock => ThemePalette {
            accent: (228, 26, 56),
            accent_light: (244, 246, 250),
            accent_deep: (74, 78, 86),
            rim: (148, 154, 166),
            text_primary: (246, 247, 250),
            text_muted: (166, 172, 182),
            glass_top: (26, 28, 32),
            glass_bot: (14, 15, 18),
            preview_a: (20, 22, 26),
            preview_b: (52, 56, 64),
            preview_c: (10, 11, 14),
            swatches: [0xE41A38, 0xF4F6FA, 0x4A4E56, 0x343840, 0x0A0B0E],
        },
        TaskbarTheme::VintageCassetteDock => ThemePalette {
            accent: (255, 148, 36),
            accent_light: (242, 236, 224),
            accent_deep: (220, 62, 48),
            rim: (212, 204, 190),
            text_primary: (248, 244, 236),
            text_muted: (198, 190, 176),
            glass_top: (48, 46, 44),
            glass_bot: (28, 26, 26),
            preview_a: (42, 40, 38),
            preview_b: (110, 96, 82),
            preview_c: (22, 21, 20),
            swatches: [0xFF9424, 0xF2ECE0, 0xDC3E30, 0x6E6052, 0x161514],
        },
        TaskbarTheme::RealisticWaterDock => ThemePalette {
            accent: (64, 232, 248),
            accent_light: (226, 254, 255),
            accent_deep: (16, 128, 178),
            rim: (178, 248, 255),
            text_primary: (244, 254, 255),
            text_muted: (182, 234, 246),
            glass_top: (16, 108, 146),
            glass_bot: (8, 56, 92),
            preview_a: (10, 88, 134),
            preview_b: (48, 196, 224),
            preview_c: (6, 48, 84),
            swatches: [0x40E8F8, 0xE2FEFF, 0x1080B2, 0x30C4E0, 0x063054],
        },
        TaskbarTheme::RainWindowDock => ThemePalette {
            accent: (132, 196, 246),
            accent_light: (230, 242, 255),
            accent_deep: (66, 98, 138),
            rim: (184, 216, 244),
            text_primary: (240, 246, 254),
            text_muted: (176, 198, 222),
            glass_top: (28, 40, 56),
            glass_bot: (15, 22, 34),
            preview_a: (22, 34, 52),
            preview_b: (64, 92, 126),
            preview_c: (12, 18, 30),
            swatches: [0x84C4F6, 0xE6F2FF, 0x42628A, 0x405C7E, 0x0C121E],
        },
        TaskbarTheme::OceanTideDock => ThemePalette {
            accent: (96, 228, 216),
            accent_light: (236, 252, 250),
            accent_deep: (16, 88, 122),
            rim: (196, 244, 242),
            text_primary: (244, 252, 252),
            text_muted: (178, 222, 224),
            glass_top: (14, 62, 88),
            glass_bot: (8, 34, 56),
            preview_a: (10, 52, 82),
            preview_b: (34, 134, 158),
            preview_c: (6, 28, 48),
            swatches: [0x60E4D8, 0xECFCFA, 0x10587A, 0x22869E, 0x061C30],
        },
        TaskbarTheme::ZenCalmMistDock => ThemePalette {
            accent: (162, 206, 188),
            accent_light: (240, 246, 242),
            accent_deep: (96, 126, 116),
            rim: (206, 224, 214),
            text_primary: (242, 248, 244),
            text_muted: (188, 206, 198),
            glass_top: (42, 56, 54),
            glass_bot: (28, 38, 38),
            preview_a: (36, 52, 50),
            preview_b: (96, 128, 120),
            preview_c: (24, 34, 34),
            swatches: [0xA2CEBC, 0xF0F6F2, 0x607E74, 0x608078, 0x182222],
        },
        TaskbarTheme::CalmKoiPondDock => ThemePalette {
            accent: (248, 146, 86),
            accent_light: (252, 236, 220),
            accent_deep: (42, 128, 108),
            rim: (146, 204, 188),
            text_primary: (242, 250, 246),
            text_muted: (178, 212, 202),
            glass_top: (18, 56, 52),
            glass_bot: (10, 32, 32),
            preview_a: (14, 48, 46),
            preview_b: (44, 108, 96),
            preview_c: (8, 26, 26),
            swatches: [0xF89256, 0xFCECDC, 0x2A806C, 0x2C6C60, 0x081A1A],
        },
        TaskbarTheme::SunlitForestCanopyDock => ThemePalette {
            accent: (224, 206, 120),
            accent_light: (250, 246, 218),
            accent_deep: (74, 118, 66),
            rim: (188, 206, 148),
            text_primary: (246, 248, 236),
            text_muted: (192, 206, 174),
            glass_top: (32, 52, 34),
            glass_bot: (18, 30, 20),
            preview_a: (26, 46, 28),
            preview_b: (84, 118, 66),
            preview_c: (14, 24, 16),
            swatches: [0xE0CE78, 0xFAF6DA, 0x4A7642, 0x547642, 0x0E1810],
        },
        TaskbarTheme::CarraraMarbleDock => ThemePalette {
            accent: (186, 146, 72),
            accent_light: (252, 250, 246),
            accent_deep: (118, 116, 112),
            rim: (204, 176, 116),
            text_primary: (34, 34, 38),
            text_muted: (104, 104, 110),
            glass_top: (246, 244, 240),
            glass_bot: (228, 224, 218),
            preview_a: (224, 220, 214),
            preview_b: (248, 246, 242),
            preview_c: (196, 192, 186),
            swatches: [0xBA9248, 0xFCFAF6, 0x767470, 0xF8F6F2, 0xC4C0BA],
        },
        TaskbarTheme::WarmWalnutDeskDock => ThemePalette {
            accent: (218, 172, 92),
            accent_light: (248, 232, 198),
            accent_deep: (104, 62, 34),
            rim: (202, 158, 94),
            text_primary: (250, 240, 224),
            text_muted: (208, 184, 152),
            glass_top: (76, 46, 28),
            glass_bot: (42, 24, 14),
            preview_a: (58, 34, 20),
            preview_b: (116, 72, 42),
            preview_c: (30, 18, 10),
            swatches: [0xDAAC5C, 0xF8E8C6, 0x683E22, 0x74482A, 0x1E120A],
        },
        TaskbarTheme::FrostedGlacierIceDock => ThemePalette {
            accent: (132, 224, 255),
            accent_light: (244, 252, 255),
            accent_deep: (66, 148, 196),
            rim: (216, 246, 255),
            text_primary: (244, 252, 255),
            text_muted: (188, 224, 242),
            glass_top: (42, 86, 114),
            glass_bot: (20, 50, 76),
            preview_a: (34, 78, 110),
            preview_b: (136, 206, 236),
            preview_c: (18, 44, 68),
            swatches: [0x84E0FF, 0xF4FCFF, 0x4294C4, 0x88CEEC, 0x122C44],
        },
        TaskbarTheme::SilkLinenCalmDock => ThemePalette {
            accent: (168, 122, 86),
            accent_light: (252, 248, 240),
            accent_deep: (126, 104, 88),
            rim: (192, 168, 142),
            text_primary: (46, 38, 32),
            text_muted: (118, 104, 92),
            glass_top: (238, 230, 218),
            glass_bot: (220, 210, 194),
            preview_a: (214, 202, 188),
            preview_b: (242, 236, 224),
            preview_c: (188, 174, 158),
            swatches: [0xA87A56, 0xFCF8F0, 0x7E6858, 0xF2ECE0, 0xBCAE9E],
        },
        TaskbarTheme::AbyssalTrenchDock => ThemePalette::for_realistic_theme("deep_abyssal_trench")
            .unwrap_or_else(ThemePalette::autumn),
        TaskbarTheme::GoldenHourHorizonDock => {
            ThemePalette::for_realistic_theme("golden_hour_horizon")
                .unwrap_or_else(ThemePalette::autumn)
        }
        _ => return None,
    };
    Some(p)
}

pub fn dock_spec(theme: TaskbarTheme) -> DockVisualSpec {
    match theme {
        TaskbarTheme::CyberpunkHudDock => DockVisualSpec {
            layout: DockLayoutKind::HudConsoleDeck,
            shape: DockShelfShape::Chamfer,
            indicator: DockIndicatorStyle::Diamond,
            art: DockArtLayer::CyberHud,
            bg_top: (18, 24, 38),
            bg_mid: (11, 15, 26),
            bg_bot: (8, 10, 18),
            bg_alpha_bias: 0.92,
            rim_outer: (0, 240, 255, 0.82),
            rim_inner: (255, 56, 128, 0.36),
            divider: (0, 240, 255, 0.44),
            dot_active: (0, 244, 255),
            dot_idle: (255, 86, 148),
            dot_glow: (0, 240, 255),
            tip_bg: (10, 14, 24, 0.94),
            tip_border: (0, 240, 255, 0.72),
            tip_fg: (230, 252, 255, 0.98),
            ambient_animated: true,
            light_mode_paper: false,
            mechanical_lift: false,
        },
        TaskbarTheme::DynamicCapsuleDock => DockVisualSpec {
            layout: DockLayoutKind::MultiIslandBar,
            shape: DockShelfShape::Pill,
            indicator: DockIndicatorStyle::PillBar,
            art: DockArtLayer::IslandTelemetry,
            bg_top: (16, 18, 24),
            bg_mid: (10, 12, 16),
            bg_bot: (6, 8, 11),
            bg_alpha_bias: 0.90,
            rim_outer: (196, 212, 232, 0.46),
            rim_inner: (255, 255, 255, 0.16),
            divider: (206, 222, 240, 0.24),
            dot_active: (94, 234, 212),
            dot_idle: (170, 184, 204),
            dot_glow: (94, 234, 212),
            tip_bg: (10, 12, 16, 0.96),
            tip_border: (94, 234, 212, 0.42),
            tip_fg: (246, 250, 255, 0.98),
            ambient_animated: true,
            light_mode_paper: false,
            mechanical_lift: false,
        },
        TaskbarTheme::FrutigerAquaDock => DockVisualSpec {
            layout: DockLayoutKind::FullEdgeTaskbar,
            shape: DockShelfShape::Tile,
            indicator: DockIndicatorStyle::AquaOrb,
            art: DockArtLayer::AquaReflect,
            bg_top: (32, 126, 186),
            bg_mid: (16, 82, 138),
            bg_bot: (10, 48, 92),
            bg_alpha_bias: 0.85,
            rim_outer: (146, 238, 255, 0.76),
            rim_inner: (240, 252, 255, 0.44),
            divider: (196, 244, 255, 0.44),
            dot_active: (168, 248, 255),
            dot_idle: (140, 210, 240),
            dot_glow: (74, 224, 255),
            tip_bg: (12, 46, 86, 0.94),
            tip_border: (158, 240, 255, 0.60),
            tip_fg: (244, 252, 255, 0.98),
            ambient_animated: true,
            light_mode_paper: false,
            mechanical_lift: false,
        },
        TaskbarTheme::InvisibleGhostDock => DockVisualSpec {
            layout: DockLayoutKind::IndependentOrbs,
            shape: DockShelfShape::FloatingZero,
            indicator: DockIndicatorStyle::Diamond,
            art: DockArtLayer::GhostAura,
            bg_top: (20, 24, 36),
            bg_mid: (16, 20, 30),
            bg_bot: (12, 16, 24),
            bg_alpha_bias: 0.0,
            rim_outer: (255, 255, 255, 0.0),
            rim_inner: (255, 255, 255, 0.0),
            divider: (224, 236, 255, 0.28),
            dot_active: (255, 255, 255),
            dot_idle: (210, 226, 248),
            dot_glow: (180, 220, 255),
            tip_bg: (14, 18, 28, 0.88),
            tip_border: (224, 238, 255, 0.28),
            tip_fg: (248, 252, 255, 0.98),
            ambient_animated: false,
            light_mode_paper: false,
            mechanical_lift: false,
        },
        TaskbarTheme::AuroraRibbonDock => DockVisualSpec {
            layout: DockLayoutKind::MultiIslandBar,
            shape: DockShelfShape::Pill,
            indicator: DockIndicatorStyle::CircleDot,
            art: DockArtLayer::AuroraSilk,
            bg_top: (18, 30, 42),
            bg_mid: (14, 20, 34),
            bg_bot: (10, 14, 26),
            bg_alpha_bias: 0.86,
            rim_outer: (116, 244, 206, 0.62),
            rim_inner: (188, 240, 255, 0.24),
            divider: (158, 236, 224, 0.32),
            dot_active: (92, 252, 198),
            dot_idle: (168, 220, 236),
            dot_glow: (72, 236, 196),
            tip_bg: (12, 22, 34, 0.92),
            tip_border: (106, 242, 202, 0.45),
            tip_fg: (240, 255, 250, 0.98),
            ambient_animated: true,
            light_mode_paper: false,
            mechanical_lift: false,
        },
        TaskbarTheme::NextSlatePlankDock => DockVisualSpec {
            layout: DockLayoutKind::FullEdgeTaskbar,
            shape: DockShelfShape::Tile,
            indicator: DockIndicatorStyle::PillBar,
            art: DockArtLayer::SlateTiles,
            bg_top: (48, 54, 66),
            bg_mid: (34, 38, 48),
            bg_bot: (22, 26, 34),
            bg_alpha_bias: 0.90,
            rim_outer: (178, 192, 214, 0.56),
            rim_inner: (238, 244, 255, 0.26),
            divider: (178, 192, 214, 0.36),
            dot_active: (132, 212, 255),
            dot_idle: (174, 184, 200),
            dot_glow: (120, 196, 255),
            tip_bg: (28, 32, 40, 0.96),
            tip_border: (186, 198, 218, 0.44),
            tip_fg: (244, 248, 252, 0.98),
            ambient_animated: false,
            light_mode_paper: false,
            mechanical_lift: true,
        },
        TaskbarTheme::KumikoWoodDock => DockVisualSpec {
            layout: DockLayoutKind::InlineMorphRail,
            shape: DockShelfShape::Tile,
            indicator: DockIndicatorStyle::SquareStamp,
            art: DockArtLayer::KumikoLattice,
            bg_top: (56, 35, 22),
            bg_mid: (38, 23, 15),
            bg_bot: (24, 15, 10),
            bg_alpha_bias: 0.90,
            rim_outer: (230, 182, 102, 0.72),
            rim_inner: (255, 224, 170, 0.28),
            divider: (230, 182, 102, 0.44),
            dot_active: (236, 86, 62),
            dot_idle: (222, 178, 114),
            dot_glow: (242, 128, 64),
            tip_bg: (36, 22, 14, 0.95),
            tip_border: (230, 182, 102, 0.56),
            tip_fg: (254, 244, 226, 0.98),
            ambient_animated: false,
            light_mode_paper: false,
            mechanical_lift: false,
        },
        TaskbarTheme::LiquidChromeDock => DockVisualSpec {
            layout: DockLayoutKind::FishEyeShelf,
            shape: DockShelfShape::Pill,
            indicator: DockIndicatorStyle::AquaOrb,
            art: DockArtLayer::LiquidChrome,
            bg_top: (68, 76, 92),
            bg_mid: (32, 37, 46),
            bg_bot: (18, 21, 28),
            bg_alpha_bias: 0.88,
            rim_outer: (234, 242, 254, 0.72),
            rim_inner: (255, 255, 255, 0.36),
            divider: (224, 234, 248, 0.38),
            dot_active: (252, 254, 255),
            dot_idle: (186, 198, 216),
            dot_glow: (206, 226, 255),
            tip_bg: (22, 26, 34, 0.94),
            tip_border: (228, 238, 252, 0.56),
            tip_fg: (248, 250, 255, 0.98),
            ambient_animated: true,
            light_mode_paper: false,
            mechanical_lift: false,
        },
        TaskbarTheme::SakuraBreezeDock => DockVisualSpec {
            layout: DockLayoutKind::FishEyeShelf,
            shape: DockShelfShape::Pill,
            indicator: DockIndicatorStyle::CircleDot,
            art: DockArtLayer::SakuraPetals,
            bg_top: (48, 24, 44),
            bg_mid: (32, 16, 32),
            bg_bot: (20, 10, 22),
            bg_alpha_bias: 0.84,
            rim_outer: (255, 174, 210, 0.58),
            rim_inner: (255, 224, 238, 0.24),
            divider: (255, 184, 216, 0.32),
            dot_active: (255, 168, 208),
            dot_idle: (236, 184, 210),
            dot_glow: (255, 134, 188),
            tip_bg: (34, 16, 32, 0.93),
            tip_border: (255, 172, 208, 0.48),
            tip_fg: (255, 244, 248, 0.98),
            ambient_animated: true,
            light_mode_paper: false,
            mechanical_lift: false,
        },
        TaskbarTheme::SolsticeAstrolabeDock => DockVisualSpec {
            layout: DockLayoutKind::HudConsoleDeck,
            shape: DockShelfShape::Tile,
            indicator: DockIndicatorStyle::Star4,
            art: DockArtLayer::AstrolabeTicks,
            bg_top: (20, 32, 62),
            bg_mid: (13, 21, 44),
            bg_bot: (8, 13, 28),
            bg_alpha_bias: 0.93,
            rim_outer: (226, 182, 92, 0.78),
            rim_inner: (255, 228, 158, 0.32),
            divider: (226, 182, 92, 0.44),
            dot_active: (250, 214, 126),
            dot_idle: (192, 168, 114),
            dot_glow: (234, 188, 92),
            tip_bg: (14, 22, 46, 0.96),
            tip_border: (226, 182, 92, 0.66),
            tip_fg: (252, 246, 226, 0.98),
            ambient_animated: true,
            light_mode_paper: false,
            mechanical_lift: false,
        },
        TaskbarTheme::VaporwaveHorizonDock => DockVisualSpec {
            layout: DockLayoutKind::HudConsoleDeck,
            shape: DockShelfShape::Tile,
            indicator: DockIndicatorStyle::PillBar,
            art: DockArtLayer::VaporGrid,
            bg_top: (50, 16, 76),
            bg_mid: (32, 10, 54),
            bg_bot: (16, 6, 34),
            bg_alpha_bias: 0.92,
            rim_outer: (255, 104, 194, 0.78),
            rim_inner: (255, 194, 102, 0.34),
            divider: (255, 118, 196, 0.44),
            dot_active: (255, 106, 210),
            dot_idle: (116, 228, 255),
            dot_glow: (255, 92, 198),
            tip_bg: (28, 10, 48, 0.95),
            tip_border: (255, 116, 202, 0.64),
            tip_fg: (255, 242, 252, 0.98),
            ambient_animated: true,
            light_mode_paper: false,
            mechanical_lift: false,
        },
        TaskbarTheme::EditorialPaperDock => DockVisualSpec {
            layout: DockLayoutKind::InlineMorphRail,
            shape: DockShelfShape::Tile,
            indicator: DockIndicatorStyle::SquareStamp,
            art: DockArtLayer::PaperHalftone,
            bg_top: (244, 240, 232),
            bg_mid: (236, 231, 220),
            bg_bot: (226, 220, 206),
            bg_alpha_bias: 0.96,
            rim_outer: (34, 32, 28, 0.88),
            rim_inner: (120, 112, 100, 0.32),
            divider: (38, 36, 32, 0.42),
            dot_active: (208, 62, 42),
            dot_idle: (44, 42, 38),
            dot_glow: (208, 62, 42),
            tip_bg: (24, 23, 20, 0.96),
            tip_border: (220, 214, 200, 0.34),
            tip_fg: (248, 245, 238, 0.98),
            ambient_animated: false,
            light_mode_paper: true,
            mechanical_lift: true,
        },
        TaskbarTheme::BiolumAbyssDock => DockVisualSpec {
            layout: DockLayoutKind::IndependentOrbs,
            shape: DockShelfShape::FloatingZero,
            indicator: DockIndicatorStyle::AquaOrb,
            art: DockArtLayer::BiolumTendrils,
            bg_top: (10, 24, 48),
            bg_mid: (6, 16, 34),
            bg_bot: (3, 9, 22),
            bg_alpha_bias: 0.89,
            rim_outer: (58, 242, 232, 0.70),
            rim_inner: (166, 136, 255, 0.32),
            divider: (72, 236, 228, 0.36),
            dot_active: (72, 252, 240),
            dot_idle: (162, 146, 255),
            dot_glow: (46, 236, 232),
            tip_bg: (6, 16, 36, 0.94),
            tip_border: (58, 242, 232, 0.56),
            tip_fg: (234, 254, 255, 0.98),
            ambient_animated: true,
            light_mode_paper: false,
            mechanical_lift: false,
        },
        TaskbarTheme::TerminalPhosphorDock => DockVisualSpec {
            layout: DockLayoutKind::HudConsoleDeck,
            shape: DockShelfShape::Tile,
            indicator: DockIndicatorStyle::PhosphorCaret,
            art: DockArtLayer::PhosphorCrt,
            bg_top: (8, 26, 14),
            bg_mid: (5, 18, 10),
            bg_bot: (3, 10, 6),
            bg_alpha_bias: 0.94,
            rim_outer: (76, 255, 132, 0.78),
            rim_inner: (128, 255, 168, 0.24),
            divider: (76, 255, 132, 0.44),
            dot_active: (108, 255, 152),
            dot_idle: (56, 196, 102),
            dot_glow: (76, 255, 132),
            tip_bg: (6, 22, 12, 0.96),
            tip_border: (76, 255, 132, 0.78),
            tip_fg: (178, 255, 200, 0.98),
            ambient_animated: true,
            light_mode_paper: false,
            mechanical_lift: false,
        },
        TaskbarTheme::PrismRefractionDock => DockVisualSpec {
            layout: DockLayoutKind::IndependentOrbs,
            shape: DockShelfShape::FloatingZero,
            indicator: DockIndicatorStyle::AquaOrb,
            art: DockArtLayer::PrismSpectral,
            bg_top: (36, 44, 62),
            bg_mid: (24, 28, 42),
            bg_bot: (16, 20, 32),
            bg_alpha_bias: 0.86,
            rim_outer: (232, 246, 255, 0.74),
            rim_inner: (255, 184, 240, 0.34),
            divider: (224, 240, 255, 0.36),
            dot_active: (255, 255, 255),
            dot_idle: (152, 232, 255),
            dot_glow: (132, 238, 255),
            tip_bg: (20, 24, 36, 0.93),
            tip_border: (168, 238, 255, 0.56),
            tip_fg: (248, 252, 255, 0.98),
            ambient_animated: true,
            light_mode_paper: false,
            mechanical_lift: false,
        },
        TaskbarTheme::ObsidianEmberDock => DockVisualSpec {
            layout: DockLayoutKind::HudConsoleDeck,
            shape: DockShelfShape::Chamfer,
            indicator: DockIndicatorStyle::EmberSpark,
            art: DockArtLayer::ObsidianMagma,
            bg_top: (32, 20, 16),
            bg_mid: (20, 12, 10),
            bg_bot: (12, 7, 6),
            bg_alpha_bias: 0.93,
            rim_outer: (255, 118, 44, 0.72),
            rim_inner: (255, 204, 104, 0.26),
            divider: (255, 134, 62, 0.42),
            dot_active: (255, 186, 84),
            dot_idle: (236, 86, 38),
            dot_glow: (255, 118, 40),
            tip_bg: (22, 13, 11, 0.95),
            tip_border: (255, 128, 52, 0.62),
            tip_fg: (255, 244, 234, 0.98),
            ambient_animated: true,
            light_mode_paper: false,
            mechanical_lift: false,
        },
        TaskbarTheme::BauhausGeometricDock => DockVisualSpec {
            layout: DockLayoutKind::InlineMorphRail,
            shape: DockShelfShape::Tile,
            indicator: DockIndicatorStyle::GeometricMixed,
            art: DockArtLayer::BauhausPrimary,
            bg_top: (38, 38, 44),
            bg_mid: (28, 28, 34),
            bg_bot: (20, 20, 24),
            bg_alpha_bias: 0.92,
            rim_outer: (226, 222, 212, 0.68),
            rim_inner: (255, 255, 255, 0.16),
            divider: (228, 68, 48, 0.72),
            dot_active: (246, 198, 52),
            dot_idle: (92, 152, 246),
            dot_glow: (246, 198, 52),
            tip_bg: (24, 24, 28, 0.96),
            tip_border: (244, 196, 48, 0.68),
            tip_fg: (250, 248, 242, 0.98),
            ambient_animated: false,
            light_mode_paper: false,
            mechanical_lift: true,
        },
        TaskbarTheme::StarfieldWarpDock => DockVisualSpec {
            layout: DockLayoutKind::MultiIslandBar,
            shape: DockShelfShape::Pill,
            indicator: DockIndicatorStyle::PillBar,
            art: DockArtLayer::StarfieldWarp,
            bg_top: (12, 16, 34),
            bg_mid: (7, 10, 24),
            bg_bot: (4, 6, 14),
            bg_alpha_bias: 0.90,
            rim_outer: (146, 212, 255, 0.66),
            rim_inner: (184, 198, 255, 0.24),
            divider: (146, 212, 255, 0.36),
            dot_active: (184, 236, 255),
            dot_idle: (126, 154, 232),
            dot_glow: (122, 206, 255),
            tip_bg: (8, 12, 26, 0.95),
            tip_border: (146, 212, 255, 0.56),
            tip_fg: (244, 250, 255, 0.98),
            ambient_animated: true,
            light_mode_paper: false,
            mechanical_lift: false,
        },
        TaskbarTheme::KatanaGlassDock => DockVisualSpec {
            layout: DockLayoutKind::FishEyeShelf,
            shape: DockShelfShape::Pill,
            indicator: DockIndicatorStyle::PillBar,
            art: DockArtLayer::Quartz,
            bg_top: (32, 34, 38),
            bg_mid: (22, 24, 28),
            bg_bot: (14, 16, 19),
            bg_alpha_bias: 0.72,
            rim_outer: (228, 234, 242, 0.40),
            rim_inner: (255, 255, 255, 0.18),
            divider: (228, 234, 242, 0.22),
            dot_active: (246, 249, 252),
            dot_idle: (176, 184, 196),
            dot_glow: (220, 230, 244),
            tip_bg: (14, 16, 20, 0.94),
            tip_border: (228, 234, 242, 0.30),
            tip_fg: (246, 249, 252, 0.98),
            ambient_animated: false,
            light_mode_paper: false,
            mechanical_lift: false,
        },
        TaskbarTheme::BrushedTitaniumDock => DockVisualSpec {
            layout: DockLayoutKind::InlineMorphRail,
            shape: DockShelfShape::Tile,
            indicator: DockIndicatorStyle::PillBar,
            art: DockArtLayer::BrushedTitanium,
            bg_top: (94, 100, 110),
            bg_mid: (62, 68, 78),
            bg_bot: (42, 46, 54),
            bg_alpha_bias: 0.96,
            rim_outer: (224, 230, 240, 0.78),
            rim_inner: (255, 255, 255, 0.34),
            divider: (218, 224, 234, 0.36),
            dot_active: (255, 118, 36),
            dot_idle: (192, 200, 212),
            dot_glow: (255, 134, 54),
            tip_bg: (32, 36, 44, 0.96),
            tip_border: (214, 222, 232, 0.62),
            tip_fg: (246, 249, 252, 0.98),
            ambient_animated: false,
            light_mode_paper: false,
            mechanical_lift: true,
        },
        TaskbarTheme::VacuumTubeHiFiDock => DockVisualSpec {
            layout: DockLayoutKind::HudConsoleDeck,
            shape: DockShelfShape::Tile,
            indicator: DockIndicatorStyle::CircleDot,
            art: DockArtLayer::VacuumTubeHiFi,
            bg_top: (54, 38, 26),
            bg_mid: (34, 23, 15),
            bg_bot: (20, 14, 9),
            bg_alpha_bias: 0.96,
            rim_outer: (232, 192, 122, 0.78),
            rim_inner: (255, 180, 74, 0.32),
            divider: (228, 184, 110, 0.38),
            dot_active: (255, 166, 54),
            dot_idle: (218, 174, 110),
            dot_glow: (255, 148, 38),
            tip_bg: (26, 18, 12, 0.96),
            tip_border: (236, 190, 112, 0.68),
            tip_fg: (255, 242, 218, 0.98),
            ambient_animated: true,
            light_mode_paper: false,
            mechanical_lift: true,
        },
        TaskbarTheme::SwissHorologyDock => DockVisualSpec {
            layout: DockLayoutKind::FishEyeShelf,
            shape: DockShelfShape::Pill,
            indicator: DockIndicatorStyle::Diamond,
            art: DockArtLayer::SwissGuilloche,
            bg_top: (22, 36, 62),
            bg_mid: (14, 24, 44),
            bg_bot: (9, 15, 30),
            bg_alpha_bias: 0.95,
            rim_outer: (220, 230, 244, 0.82),
            rim_inner: (146, 186, 255, 0.34),
            divider: (214, 224, 240, 0.34),
            dot_active: (214, 32, 74),
            dot_idle: (196, 212, 236),
            dot_glow: (116, 174, 255),
            tip_bg: (12, 20, 38, 0.96),
            tip_border: (210, 222, 242, 0.68),
            tip_fg: (244, 248, 255, 0.98),
            ambient_animated: true,
            light_mode_paper: false,
            mechanical_lift: false,
        },
        TaskbarTheme::SaddleLeatherDock => DockVisualSpec {
            layout: DockLayoutKind::FishEyeShelf,
            shape: DockShelfShape::Pill,
            indicator: DockIndicatorStyle::SquareStamp,
            art: DockArtLayer::SaddleLeather,
            bg_top: (106, 62, 34),
            bg_mid: (82, 46, 24),
            bg_bot: (56, 30, 15),
            bg_alpha_bias: 0.97,
            rim_outer: (218, 176, 114, 0.74),
            rim_inner: (248, 228, 192, 0.28),
            divider: (224, 186, 128, 0.38),
            dot_active: (236, 190, 102),
            dot_idle: (216, 186, 150),
            dot_glow: (230, 176, 86),
            tip_bg: (46, 26, 14, 0.96),
            tip_border: (224, 180, 112, 0.66),
            tip_fg: (252, 244, 228, 0.98),
            ambient_animated: false,
            light_mode_paper: false,
            mechanical_lift: true,
        },
        TaskbarTheme::ObsidianCarbonDock => DockVisualSpec {
            layout: DockLayoutKind::MultiIslandBar,
            shape: DockShelfShape::Chamfer,
            indicator: DockIndicatorStyle::CircleDot,
            art: DockArtLayer::CarbonWeave,
            bg_top: (28, 30, 35),
            bg_mid: (19, 20, 24),
            bg_bot: (13, 14, 17),
            bg_alpha_bias: 0.96,
            rim_outer: (144, 150, 162, 0.62),
            rim_inner: (255, 255, 255, 0.16),
            divider: (126, 132, 144, 0.34),
            dot_active: (228, 22, 52),
            dot_idle: (176, 182, 192),
            dot_glow: (228, 26, 56),
            tip_bg: (16, 17, 20, 0.96),
            tip_border: (228, 26, 56, 0.62),
            tip_fg: (246, 248, 252, 0.98),
            ambient_animated: false,
            light_mode_paper: false,
            mechanical_lift: true,
        },
        TaskbarTheme::VintageCassetteDock => DockVisualSpec {
            layout: DockLayoutKind::HudConsoleDeck,
            shape: DockShelfShape::Tile,
            indicator: DockIndicatorStyle::SquareStamp,
            art: DockArtLayer::CassetteDeck1984,
            bg_top: (58, 55, 50),
            bg_mid: (40, 38, 35),
            bg_bot: (28, 26, 24),
            bg_alpha_bias: 0.97,
            rim_outer: (218, 210, 196, 0.72),
            rim_inner: (255, 164, 58, 0.24),
            divider: (208, 198, 182, 0.36),
            dot_active: (255, 148, 36),
            dot_idle: (218, 208, 194),
            dot_glow: (255, 132, 24),
            tip_bg: (30, 28, 26, 0.96),
            tip_border: (255, 148, 36, 0.64),
            tip_fg: (248, 244, 236, 0.98),
            ambient_animated: true,
            light_mode_paper: false,
            mechanical_lift: true,
        },
        TaskbarTheme::RealisticWaterDock => DockVisualSpec {
            layout: DockLayoutKind::FishEyeShelf,
            shape: DockShelfShape::Pill,
            indicator: DockIndicatorStyle::AquaOrb,
            art: DockArtLayer::CrystalWater,
            bg_top: (22, 134, 176),
            bg_mid: (12, 92, 136),
            bg_bot: (8, 56, 94),
            bg_alpha_bias: 0.84,
            rim_outer: (192, 250, 255, 0.84),
            rim_inner: (238, 254, 255, 0.44),
            divider: (174, 242, 255, 0.38),
            dot_active: (182, 254, 255),
            dot_idle: (116, 224, 248),
            dot_glow: (64, 232, 255),
            tip_bg: (10, 56, 88, 0.94),
            tip_border: (166, 244, 255, 0.72),
            tip_fg: (242, 254, 255, 0.98),
            ambient_animated: true,
            light_mode_paper: false,
            mechanical_lift: false,
        },
        TaskbarTheme::RainWindowDock => DockVisualSpec {
            layout: DockLayoutKind::FishEyeShelf,
            shape: DockShelfShape::Pill,
            indicator: DockIndicatorStyle::AquaOrb,
            art: DockArtLayer::RainWindowGlass,
            bg_top: (34, 48, 66),
            bg_mid: (22, 34, 50),
            bg_bot: (14, 22, 34),
            bg_alpha_bias: 0.86,
            rim_outer: (186, 218, 246, 0.68),
            rim_inner: (226, 242, 255, 0.30),
            divider: (172, 206, 236, 0.32),
            dot_active: (164, 218, 255),
            dot_idle: (146, 176, 206),
            dot_glow: (126, 192, 246),
            tip_bg: (16, 24, 38, 0.94),
            tip_border: (172, 212, 244, 0.60),
            tip_fg: (238, 246, 255, 0.98),
            ambient_animated: true,
            light_mode_paper: false,
            mechanical_lift: false,
        },
        TaskbarTheme::OceanTideDock => DockVisualSpec {
            layout: DockLayoutKind::FullEdgeTaskbar,
            shape: DockShelfShape::Tile,
            indicator: DockIndicatorStyle::AquaOrb,
            art: DockArtLayer::OceanTideFoam,
            bg_top: (18, 78, 108),
            bg_mid: (11, 52, 80),
            bg_bot: (7, 30, 52),
            bg_alpha_bias: 0.90,
            rim_outer: (210, 248, 246, 0.74),
            rim_inner: (142, 236, 232, 0.34),
            divider: (174, 236, 236, 0.34),
            dot_active: (166, 250, 242),
            dot_idle: (112, 206, 212),
            dot_glow: (86, 226, 222),
            tip_bg: (8, 36, 56, 0.95),
            tip_border: (172, 240, 238, 0.66),
            tip_fg: (242, 252, 252, 0.98),
            ambient_animated: true,
            light_mode_paper: false,
            mechanical_lift: false,
        },
        TaskbarTheme::ZenCalmMistDock => DockVisualSpec {
            layout: DockLayoutKind::MultiIslandBar,
            shape: DockShelfShape::Pill,
            indicator: DockIndicatorStyle::PillBar,
            art: DockArtLayer::ZenCalmMist,
            bg_top: (48, 64, 60),
            bg_mid: (36, 48, 46),
            bg_bot: (28, 38, 38),
            bg_alpha_bias: 0.88,
            rim_outer: (204, 224, 214, 0.58),
            rim_inner: (238, 246, 242, 0.24),
            divider: (192, 214, 204, 0.30),
            dot_active: (196, 232, 214),
            dot_idle: (152, 180, 170),
            dot_glow: (164, 210, 192),
            tip_bg: (30, 42, 40, 0.94),
            tip_border: (196, 218, 208, 0.52),
            tip_fg: (242, 248, 244, 0.98),
            ambient_animated: true,
            light_mode_paper: false,
            mechanical_lift: false,
        },
        TaskbarTheme::CalmKoiPondDock => DockVisualSpec {
            layout: DockLayoutKind::FishEyeShelf,
            shape: DockShelfShape::Pill,
            indicator: DockIndicatorStyle::CircleDot,
            art: DockArtLayer::TranquilKoiPond,
            bg_top: (20, 62, 56),
            bg_mid: (13, 44, 42),
            bg_bot: (9, 28, 30),
            bg_alpha_bias: 0.90,
            rim_outer: (162, 216, 198, 0.66),
            rim_inner: (248, 174, 118, 0.24),
            divider: (154, 206, 190, 0.32),
            dot_active: (250, 148, 86),
            dot_idle: (166, 212, 196),
            dot_glow: (248, 154, 92),
            tip_bg: (12, 34, 34, 0.95),
            tip_border: (168, 218, 200, 0.58),
            tip_fg: (244, 250, 246, 0.98),
            ambient_animated: true,
            light_mode_paper: false,
            mechanical_lift: false,
        },
        TaskbarTheme::SunlitForestCanopyDock => DockVisualSpec {
            layout: DockLayoutKind::FishEyeShelf,
            shape: DockShelfShape::Pill,
            indicator: DockIndicatorStyle::CircleDot,
            art: DockArtLayer::ForestSunbeams,
            bg_top: (38, 60, 38),
            bg_mid: (26, 44, 28),
            bg_bot: (18, 30, 20),
            bg_alpha_bias: 0.91,
            rim_outer: (208, 220, 152, 0.64),
            rim_inner: (248, 238, 174, 0.28),
            divider: (186, 204, 146, 0.34),
            dot_active: (244, 224, 128),
            dot_idle: (166, 194, 144),
            dot_glow: (232, 214, 122),
            tip_bg: (20, 34, 22, 0.95),
            tip_border: (204, 218, 148, 0.56),
            tip_fg: (246, 250, 236, 0.98),
            ambient_animated: true,
            light_mode_paper: false,
            mechanical_lift: false,
        },
        TaskbarTheme::CarraraMarbleDock => DockVisualSpec {
            layout: DockLayoutKind::InlineMorphRail,
            shape: DockShelfShape::Tile,
            indicator: DockIndicatorStyle::Diamond,
            art: DockArtLayer::CarraraMarble,
            bg_top: (248, 246, 242),
            bg_mid: (238, 235, 230),
            bg_bot: (224, 220, 214),
            bg_alpha_bias: 0.96,
            rim_outer: (196, 162, 92, 0.78),
            rim_inner: (255, 255, 255, 0.65),
            divider: (176, 154, 108, 0.42),
            dot_active: (186, 142, 62),
            dot_idle: (116, 114, 110),
            dot_glow: (206, 166, 86),
            tip_bg: (248, 246, 242, 0.96),
            tip_border: (192, 158, 92, 0.72),
            tip_fg: (32, 32, 36, 0.98),
            ambient_animated: false,
            light_mode_paper: true,
            mechanical_lift: false,
        },
        TaskbarTheme::WarmWalnutDeskDock => DockVisualSpec {
            layout: DockLayoutKind::FullEdgeTaskbar,
            shape: DockShelfShape::Tile,
            indicator: DockIndicatorStyle::PillBar,
            art: DockArtLayer::OiledWalnutWood,
            bg_top: (84, 50, 30),
            bg_mid: (62, 36, 21),
            bg_bot: (42, 24, 13),
            bg_alpha_bias: 0.97,
            rim_outer: (212, 168, 98, 0.72),
            rim_inner: (246, 212, 152, 0.24),
            divider: (206, 164, 98, 0.40),
            dot_active: (232, 184, 98),
            dot_idle: (194, 166, 132),
            dot_glow: (224, 174, 88),
            tip_bg: (38, 22, 13, 0.96),
            tip_border: (212, 168, 98, 0.68),
            tip_fg: (250, 240, 224, 0.98),
            ambient_animated: false,
            light_mode_paper: false,
            mechanical_lift: true,
        },
        TaskbarTheme::FrostedGlacierIceDock => DockVisualSpec {
            layout: DockLayoutKind::FishEyeShelf,
            shape: DockShelfShape::Chamfer,
            indicator: DockIndicatorStyle::Star4,
            art: DockArtLayer::GlacierCrystalIce,
            bg_top: (64, 118, 152),
            bg_mid: (38, 82, 114),
            bg_bot: (22, 54, 82),
            bg_alpha_bias: 0.84,
            rim_outer: (224, 248, 255, 0.86),
            rim_inner: (255, 255, 255, 0.48),
            divider: (206, 242, 255, 0.42),
            dot_active: (226, 250, 255),
            dot_idle: (152, 216, 244),
            dot_glow: (144, 228, 255),
            tip_bg: (22, 52, 78, 0.94),
            tip_border: (214, 246, 255, 0.74),
            tip_fg: (244, 252, 255, 0.98),
            ambient_animated: true,
            light_mode_paper: false,
            mechanical_lift: false,
        },
        TaskbarTheme::SilkLinenCalmDock => DockVisualSpec {
            layout: DockLayoutKind::MultiIslandBar,
            shape: DockShelfShape::Pill,
            indicator: DockIndicatorStyle::PillBar,
            art: DockArtLayer::WarmLinenDusk,
            bg_top: (242, 234, 222),
            bg_mid: (230, 220, 206),
            bg_bot: (216, 205, 190),
            bg_alpha_bias: 0.95,
            rim_outer: (184, 158, 132, 0.74),
            rim_inner: (255, 252, 246, 0.62),
            divider: (174, 150, 126, 0.42),
            dot_active: (168, 116, 78),
            dot_idle: (134, 118, 102),
            dot_glow: (188, 138, 98),
            tip_bg: (244, 238, 228, 0.96),
            tip_border: (182, 154, 128, 0.68),
            tip_fg: (44, 36, 30, 0.98),
            ambient_animated: true,
            light_mode_paper: true,
            mechanical_lift: false,
        },
        TaskbarTheme::AbyssalTrenchDock => DockVisualSpec {
            layout: DockLayoutKind::FishEyeShelf,
            shape: DockShelfShape::Pill,
            indicator: DockIndicatorStyle::AquaOrb,
            art: DockArtLayer::AbyssalTrench,
            bg_top: (8, 28, 48),
            bg_mid: (4, 18, 32),
            bg_bot: (2, 11, 20),
            bg_alpha_bias: 0.94,
            rim_outer: (0, 255, 204, 0.86),
            rim_inner: (0, 114, 187, 0.45),
            divider: (0, 255, 204, 0.34),
            dot_active: (0, 255, 204),
            dot_idle: (0, 114, 187),
            dot_glow: (0, 255, 204),
            tip_bg: (4, 16, 28, 0.96),
            tip_border: (0, 255, 204, 0.72),
            tip_fg: (224, 255, 248, 0.98),
            ambient_animated: true,
            light_mode_paper: false,
            mechanical_lift: false,
        },
        TaskbarTheme::GoldenHourHorizonDock => DockVisualSpec {
            layout: DockLayoutKind::FishEyeShelf,
            shape: DockShelfShape::Pill,
            indicator: DockIndicatorStyle::PillBar,
            art: DockArtLayer::GoldenHourHorizon,
            bg_top: (78, 52, 46),
            bg_mid: (62, 34, 48),
            bg_bot: (43, 9, 56),
            bg_alpha_bias: 0.92,
            rim_outer: (255, 209, 102, 0.88),
            rim_inner: (255, 227, 216, 0.42),
            divider: (255, 209, 102, 0.38),
            dot_active: (255, 209, 102),
            dot_idle: (255, 158, 0),
            dot_glow: (255, 158, 0),
            tip_bg: (48, 24, 42, 0.95),
            tip_border: (255, 209, 102, 0.75),
            tip_fg: (255, 227, 216, 0.98),
            ambient_animated: true,
            light_mode_paper: false,
            mechanical_lift: false,
        },
        _ => DockVisualSpec {
            // QuartzShellDock (authentic floating curved shelf + parabolic wave)
            layout: DockLayoutKind::FishEyeShelf,
            shape: DockShelfShape::Pill,
            indicator: DockIndicatorStyle::CircleDot,
            art: DockArtLayer::Quartz,
            bg_top: (34, 40, 52),
            bg_mid: (24, 30, 42),
            bg_bot: (18, 22, 32),
            bg_alpha_bias: 0.82,
            rim_outer: (206, 228, 252, 0.55),
            rim_inner: (255, 255, 255, 0.22),
            divider: (255, 255, 255, 0.24),
            dot_active: (255, 255, 255),
            dot_idle: (218, 228, 242),
            dot_glow: (206, 228, 252),
            tip_bg: (16, 20, 28, 0.92),
            tip_border: (255, 255, 255, 0.22),
            tip_fg: (246, 250, 255, 0.96),
            ambient_animated: false,
            light_mode_paper: false,
            mechanical_lift: false,
        },
    }
}

#[inline]
pub fn rgba(r: u8, g: u8, b: u8, a: f32) -> D2D1_COLOR_F {
    D2D1_COLOR_F {
        r: r as f32 / 255.0,
        g: g as f32 / 255.0,
        b: b as f32 / 255.0,
        a: a.clamp(0.0, 1.0),
    }
}

unsafe fn create_grad(
    rt: &ID2D1DCRenderTarget,
    start: Vector2,
    end: Vector2,
    stops: &[D2D1_GRADIENT_STOP],
) -> Option<ID2D1LinearGradientBrush> {
    let col = rt
        .CreateGradientStopCollection(stops, D2D1_GAMMA_2_2, D2D1_EXTEND_MODE_CLAMP)
        .ok()?;
    let props = D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES {
        startPoint: start,
        endPoint: end,
    };
    rt.CreateLinearGradientBrush(&props, None, &col).ok()
}

pub unsafe fn chamfer_geo(
    d2d: &ID2D1Factory,
    r: D2D_RECT_F,
    cut: f32,
) -> Option<ID2D1PathGeometry> {
    let w = (r.right - r.left).max(6.0);
    let h = (r.bottom - r.top).max(6.0);
    let c = cut.clamp(2.0, (w * 0.30).min(h * 0.36));
    let pts = [
        Vector2 { X: r.left + c, Y: r.top },
        Vector2 { X: r.right - c, Y: r.top },
        Vector2 { X: r.right, Y: r.top + c },
        Vector2 { X: r.right, Y: r.bottom - c },
        Vector2 { X: r.right - c, Y: r.bottom },
        Vector2 { X: r.left + c, Y: r.bottom },
        Vector2 { X: r.left, Y: r.bottom - c },
        Vector2 { X: r.left, Y: r.top + c },
    ];
    let geo = d2d.CreatePathGeometry().ok()?;
    let sink = geo.Open().ok()?;
    sink.BeginFigure(pts[0], D2D1_FIGURE_BEGIN_FILLED);
    sink.AddLines(&pts[1..]);
    sink.EndFigure(D2D1_FIGURE_END_CLOSED);
    sink.Close().ok()?;
    Some(geo)
}

unsafe fn push_shelf_clip(
    rt: &ID2D1DCRenderTarget,
    d2d: Option<&ID2D1Factory>,
    rect: D2D_RECT_F,
    rad: f32,
    shape: DockShelfShape,
    cut: f32,
) -> Option<(ID2D1Layer, ID2D1Geometry)> {
    if let Some(factory) = d2d {
        let mask_geo: Option<ID2D1Geometry> = match shape {
            DockShelfShape::Chamfer => chamfer_geo(factory, rect, cut).map(|g| g.into()),
            _ if rad > 0.5 => {
                let rr = D2D1_ROUNDED_RECT {
                    rect,
                    radiusX: rad,
                    radiusY: rad,
                };
                factory.CreateRoundedRectangleGeometry(&rr).ok().map(|g| g.into())
            }
            _ => None,
        };
        if let Some(geo) = mask_geo {
            if let Ok(layer) = rt.CreateLayer(None) {
                let params = D2D1_LAYER_PARAMETERS {
                    contentBounds: rect,
                    geometricMask: std::mem::ManuallyDrop::new(Some(geo.clone())),
                    maskAntialiasMode: D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
                    maskTransform: Matrix3x2::identity(),
                    opacity: 1.0,
                    opacityBrush: std::mem::ManuallyDrop::new(None),
                    layerOptions: D2D1_LAYER_OPTIONS_NONE,
                };
                rt.PushLayer(&params, &layer);
                std::mem::ManuallyDrop::into_inner(params.geometricMask);
                return Some((layer, geo));
            }
        }
    }
    rt.PushAxisAlignedClip(&rect, D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
    None
}

unsafe fn pop_shelf_clip(rt: &ID2D1DCRenderTarget, layer_opt: Option<(ID2D1Layer, ID2D1Geometry)>) {
    if layer_opt.is_some() {
        rt.PopLayer();
    } else {
        rt.PopAxisAlignedClip();
    }
}

#[allow(clippy::too_many_arguments)]
pub unsafe fn draw_dock_shelf_and_art(
    rt: &ID2D1DCRenderTarget,
    d2d: Option<&ID2D1Factory>,
    br: &ID2D1SolidColorBrush,
    shelf_r: D2D_RECT_F,
    cfg: &AppConfig,
    pal: &ThemePalette,
    anim_phase: f32,
    hover_mix: f32,
    cursor_axis_norm: f32,
    is_vertical: bool,
    u: f32,
    is_preview: bool,
) {
    draw_dock_shelf_and_art_with_ripple(
        rt,
        d2d,
        br,
        shelf_r,
        cfg,
        pal,
        anim_phase,
        hover_mix,
        cursor_axis_norm,
        is_vertical,
        u,
        is_preview,
        None,
    );
}

#[allow(clippy::too_many_arguments)]
pub unsafe fn draw_dock_shelf_and_art_with_ripple(
    rt: &ID2D1DCRenderTarget,
    d2d: Option<&ID2D1Factory>,
    br: &ID2D1SolidColorBrush,
    shelf_r: D2D_RECT_F,
    cfg: &AppConfig,
    pal: &ThemePalette,
    anim_phase: f32,
    hover_mix: f32,
    cursor_axis_norm: f32,
    is_vertical: bool,
    u: f32,
    is_preview: bool,
    click_ripple: Option<(f32, f32)>,
) {
    let spec = if is_preview {
        dock_spec(cfg.tb_theme)
    } else {
        resolve_shelf_spec(cfg)
    };
    if spec.shape == DockShelfShape::FloatingZero {
        return;
    }
    let sw = (shelf_r.right - shelf_r.left).max(8.0);
    let sh = (shelf_r.bottom - shelf_r.top).max(8.0);
    let min_dim = sw.min(sh);
    let rad = match spec.layout {
        DockLayoutKind::FullEdgeTaskbar => 0.0,
        DockLayoutKind::InlineMorphRail => (2.0 * u).min(min_dim * 0.2),
        _ => match spec.shape {
            DockShelfShape::FloatingZero => 0.0,
            DockShelfShape::Chamfer => 2.0 * u,
            DockShelfShape::Tile => (cfg.corner_radius as f32 * u)
                .clamp(3.0 * u, 9.0 * u)
                .min(min_dim * 0.30),
            DockShelfShape::Pill => (cfg.corner_radius as f32 * u)
                .clamp(12.0 * u, 26.0 * u)
                .min(min_dim * 0.48),
        },
    };

    let cut = if is_preview { 6.0 * u } else { 11.0 * u };
    let cham_geo = if spec.shape == DockShelfShape::Chamfer {
        d2d.and_then(|f| chamfer_geo(f, shelf_r, cut))
    } else {
        None
    };

    // Photorealistic PBR Material Path for all 18 Realistic Material & Nature docks
    if let Some(pbr_mat) = spec.art.pbr_material() {
        if spec.layout != DockLayoutKind::FullEdgeTaskbar {
            crate::realistic_pbr::draw_pbr_drop_shadows(
                rt,
                shelf_r,
                rad,
                u,
                spec.light_mode_paper,
            );
        }
        let alpha_user = (cfg.tint_alpha as f32 / 255.0).clamp(0.0, 1.0);
        let base_a = (spec.bg_alpha_bias * 0.72 + alpha_user * 0.28).clamp(0.55, 0.99);
        let tint_override = if cfg.tb_match_wallpaper {
            Some((
                crate::theme::rgb_to_u32(pal.glass_top),
                Some(crate::theme::rgb_to_u32(pal.glass_bot)),
            ))
        } else {
            None
        };
        let cham_base: Option<ID2D1Geometry> = cham_geo
            .as_ref()
            .and_then(|g| g.cast::<ID2D1Geometry>().ok());
        let inter = crate::realistic_pbr::pbr_input_for_dock(
            anim_phase,
            hover_mix,
            cursor_axis_norm,
            is_vertical,
            click_ripple,
        );
        crate::realistic_pbr::draw_pbr_surface_clipped(
            rt,
            d2d,
            pbr_mat,
            shelf_r,
            rad,
            u,
            base_a,
            tint_override,
            cham_base.as_ref(),
            &inter,
        );

        // Layer signature mechanical/aquatic details for specialty variants on top of PBR texture
        if matches!(
            spec.art,
            DockArtLayer::VacuumTubeHiFi
                | DockArtLayer::SwissGuilloche
                | DockArtLayer::CassetteDeck1984
                | DockArtLayer::OceanTideFoam
                | DockArtLayer::TranquilKoiPond
        ) {
            let clip_layer = push_shelf_clip(rt, d2d, shelf_r, rad, spec.shape, cut);
            draw_internal_art(
                rt,
                br,
                spec.art,
                shelf_r,
                anim_phase,
                hover_mix,
                cursor_axis_norm,
                is_vertical,
                u,
            );
            pop_shelf_clip(rt, clip_layer);
        }

        if spec.layout == DockLayoutKind::FullEdgeTaskbar {
            draw_shelf_rims(
                rt,
                br,
                shelf_r,
                rad,
                &spec,
                cfg,
                pal,
                cham_geo.as_ref(),
                u,
                is_preview,
            );
        } else {
            let custom_border = if cfg.amber_border && cfg.tb_match_wallpaper {
                Some(crate::theme::rgb_to_u32(pal.rim))
            } else {
                None
            };
            crate::realistic_pbr::draw_pbr_bevel_rim(
                rt,
                shelf_r,
                rad,
                u,
                pbr_mat,
                custom_border,
                spec.rim_outer.3.max(0.75),
                hover_mix,
                cham_base.as_ref(),
            );
        }
        return;
    }

    if spec.layout != DockLayoutKind::FullEdgeTaskbar {
        let shadow_col = if spec.light_mode_paper { 0.22 } else { 0.14 };
        for (expand, a) in [(5.0 * u, shadow_col), (2.2 * u, shadow_col + 0.11)] {
            let sh_rr = D2D1_ROUNDED_RECT {
                rect: D2D_RECT_F {
                    left: shelf_r.left - expand,
                    top: shelf_r.top - expand * 0.45 + 1.6 * u,
                    right: shelf_r.right + expand,
                    bottom: shelf_r.bottom + expand,
                },
                radiusX: rad + expand * 0.45,
                radiusY: rad + expand * 0.45,
            };
            br.SetColor(&rgba(0, 0, 0, a));
            rt.FillRoundedRectangle(&sh_rr, br);
        }
    }

    let alpha_user = (cfg.tint_alpha as f32 / 255.0).clamp(0.0, 1.0);
    let base_a = (spec.bg_alpha_bias * 0.68 + alpha_user * 0.32).clamp(0.40, 0.96);
    let (gt, gm, gb) = if cfg.tb_match_wallpaper {
        (
            pal.glass_top,
            (
                ((pal.glass_top.0 as u16 + pal.glass_bot.0 as u16) / 2) as u8,
                ((pal.glass_top.1 as u16 + pal.glass_bot.1 as u16) / 2) as u8,
                ((pal.glass_top.2 as u16 + pal.glass_bot.2 as u16) / 2) as u8,
            ),
            pal.glass_bot,
        )
    } else {
        (spec.bg_top, spec.bg_mid, spec.bg_bot)
    };
    let stops = [
        D2D1_GRADIENT_STOP {
            position: 0.0,
            color: rgba(gt.0, gt.1, gt.2, (base_a + 0.06).min(0.97)),
        },
        D2D1_GRADIENT_STOP {
            position: 0.52,
            color: rgba(gm.0, gm.1, gm.2, base_a),
        },
        D2D1_GRADIENT_STOP {
            position: 1.0,
            color: rgba(gb.0, gb.1, gb.2, (base_a + 0.04).min(0.97)),
        },
    ];

    let shelf_rr = D2D1_ROUNDED_RECT {
        rect: shelf_r,
        radiusX: rad,
        radiusY: rad,
    };
    if let Some(grad) = create_grad(
        rt,
        Vector2 { X: shelf_r.left, Y: shelf_r.top },
        Vector2 { X: shelf_r.left, Y: shelf_r.bottom },
        &stops,
    ) {
        if let Some(ref geo) = cham_geo {
            rt.FillGeometry(geo, &grad, None);
        } else {
            rt.FillRoundedRectangle(&shelf_rr, &grad);
        }
    } else {
        br.SetColor(&rgba(gm.0, gm.1, gm.2, base_a));
        if let Some(ref geo) = cham_geo {
            rt.FillGeometry(geo, br, None);
        } else {
            rt.FillRoundedRectangle(&shelf_rr, br);
        }
    }

    let clip_layer = push_shelf_clip(rt, d2d, shelf_r, rad, spec.shape, cut);
    draw_internal_art(
        rt,
        br,
        spec.art,
        shelf_r,
        anim_phase,
        hover_mix,
        cursor_axis_norm,
        is_vertical,
        u,
    );
    pop_shelf_clip(rt, clip_layer);

    draw_shelf_rims(
        rt,
        br,
        shelf_r,
        rad,
        &spec,
        cfg,
        pal,
        cham_geo.as_ref(),
        u,
        is_preview,
    );
}

#[allow(clippy::too_many_arguments)]
pub unsafe fn draw_standalone_orb_node(
    rt: &ID2D1DCRenderTarget,
    d2d: Option<&ID2D1Factory>,
    br: &ID2D1SolidColorBrush,
    cx: f32,
    cy: f32,
    sz: f32,
    theme: TaskbarTheme,
    is_hovered: bool,
    anim_phase: f32,
    idx: usize,
    u: f32,
) {
    let spec = dock_spec(theme);
    let node_r = sz * 0.58;
    let rect = D2D_RECT_F {
        left: cx - node_r,
        top: cy - node_r,
        right: cx + node_r,
        bottom: cy + node_r,
    };

    match theme {
        TaskbarTheme::BiolumAbyssDock => {
            // Standalone floating deep-sea jellyfish bubble orb
            br.SetColor(&rgba(0, 0, 0, 0.36));
            rt.FillEllipse(
                &D2D1_ELLIPSE {
                    point: Vector2 { X: cx, Y: cy + node_r * 0.92 },
                    radiusX: node_r * 0.72,
                    radiusY: 3.0 * u,
                },
                br,
            );
            let pulse = 0.5 + 0.5 * (anim_phase * 1.8 + idx as f32 * 1.2).sin();
            if is_hovered {
                br.SetColor(&rgba(58, 244, 232, 0.24));
                rt.FillEllipse(
                    &D2D1_ELLIPSE { point: Vector2 { X: cx, Y: cy }, radiusX: node_r * 1.24, radiusY: node_r * 1.24 },
                    br,
                );
            }
            let stops = [
                D2D1_GRADIENT_STOP { position: 0.0, color: rgba(24, 72, 118, 0.78) },
                D2D1_GRADIENT_STOP { position: 0.65, color: rgba(8, 26, 54, 0.84) },
                D2D1_GRADIENT_STOP { position: 1.0, color: rgba(48, 240, 230, 0.28 + 0.14 * pulse) },
            ];
            if let Some(g) = create_grad(rt, Vector2 { X: cx, Y: rect.top }, Vector2 { X: cx, Y: rect.bottom }, &stops) {
                rt.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: cx, Y: cy }, radiusX: node_r, radiusY: node_r }, &g);
            }
            br.SetColor(&rgba(72, 244, 236, if is_hovered { 0.92 } else { 0.48 + 0.20 * pulse }));
            rt.DrawEllipse(&D2D1_ELLIPSE { point: Vector2 { X: cx, Y: cy }, radiusX: node_r, radiusY: node_r }, br, 1.35 * u, None);
            // Tiny bioluminescent spec dot near upper-left bubble highlight
            br.SetColor(&rgba(218, 255, 255, 0.62));
            rt.FillEllipse(
                &D2D1_ELLIPSE {
                    point: Vector2 { X: cx - node_r * 0.42, Y: cy - node_r * 0.42 },
                    radiusX: (node_r * 0.14).max(1.8 * u),
                    radiusY: (node_r * 0.09).max(1.2 * u),
                },
                br,
            );
        }
        TaskbarTheme::PrismRefractionDock => {
            // Cut octagonal faceted prism crystal jewel floating standalone
            br.SetColor(&rgba(0, 0, 0, 0.34));
            rt.FillEllipse(
                &D2D1_ELLIPSE {
                    point: Vector2 { X: cx, Y: cy + node_r * 0.92 },
                    radiusX: node_r * 0.74,
                    radiusY: 3.0 * u,
                },
                br,
            );
            if let Some(f) = d2d {
                if let Some(geo) = chamfer_geo(f, rect, node_r * 0.56) {
                    let stops = [
                        D2D1_GRADIENT_STOP { position: 0.0, color: rgba(68, 86, 128, 0.72) },
                        D2D1_GRADIENT_STOP { position: 0.5, color: rgba(28, 34, 54, 0.78) },
                        D2D1_GRADIENT_STOP { position: 1.0, color: rgba(58, 42, 86, 0.74) },
                    ];
                    if let Some(g) = create_grad(rt, Vector2 { X: rect.left, Y: rect.top }, Vector2 { X: rect.right, Y: rect.bottom }, &stops) {
                        rt.FillGeometry(&geo, &g, None);
                    }
                    // RGB chromatic facet borders
                    if let Some(c_geo) = chamfer_geo(f, D2D_RECT_F { left: rect.left - 1.2 * u, top: rect.top, right: rect.right - 1.2 * u, bottom: rect.bottom }, node_r * 0.56) {
                        br.SetColor(&rgba(86, 238, 255, if is_hovered { 0.85 } else { 0.45 }));
                        rt.DrawGeometry(&c_geo, br, 1.1 * u, None);
                    }
                    if let Some(m_geo) = chamfer_geo(f, D2D_RECT_F { left: rect.left + 1.2 * u, top: rect.top, right: rect.right + 1.2 * u, bottom: rect.bottom }, node_r * 0.56) {
                        br.SetColor(&rgba(255, 118, 224, if is_hovered { 0.85 } else { 0.40 }));
                        rt.DrawGeometry(&m_geo, br, 1.1 * u, None);
                    }
                    br.SetColor(&rgba(244, 250, 255, if is_hovered { 0.92 } else { 0.56 }));
                    rt.DrawGeometry(&geo, br, 1.2 * u, None);
                }
            }
        }
        _ => {
            // InvisibleGhostDock: Pure naked icons with soft drop shadow & vertical moonbeam spotlight when hovered
            if sz >= 26.0 * u {
                br.SetColor(&rgba(0, 0, 0, 0.30));
                rt.FillEllipse(
                    &D2D1_ELLIPSE {
                        point: Vector2 { X: cx, Y: cy + sz * 0.44 },
                        radiusX: sz * 0.31,
                        radiusY: 2.3 * u,
                    },
                    br,
                );
            }
            if is_hovered {
                for (rm, al) in [(0.78, 0.09), (0.58, 0.16), (0.42, 0.22)] {
                    br.SetColor(&rgba(spec.dot_glow.0, spec.dot_glow.1, spec.dot_glow.2, al));
                    rt.FillEllipse(
                        &D2D1_ELLIPSE { point: Vector2 { X: cx, Y: cy }, radiusX: sz * rm, radiusY: sz * rm },
                        br,
                    );
                }
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
unsafe fn draw_internal_art(
    rt: &ID2D1DCRenderTarget,
    br: &ID2D1SolidColorBrush,
    art: DockArtLayer,
    sr: D2D_RECT_F,
    t: f32,
    hover_mix: f32,
    cx_norm: f32,
    is_vertical: bool,
    u: f32,
) {
    let w = sr.right - sr.left;
    let h = sr.bottom - sr.top;
    let focus_axis = cx_norm.clamp(0.05, 0.95);

    match art {
        DockArtLayer::Quartz => {
            let sheen = D2D_RECT_F {
                left: sr.left + 8.0 * u,
                top: sr.top + 1.0 * u,
                right: sr.right - 8.0 * u,
                bottom: sr.top + h * 0.42,
            };
            let stops = [
                D2D1_GRADIENT_STOP { position: 0.0, color: rgba(255, 255, 255, 0.22) },
                D2D1_GRADIENT_STOP { position: 1.0, color: rgba(255, 255, 255, 0.0) },
            ];
            if let Some(g) = create_grad(rt, Vector2 { X: sheen.left, Y: sheen.top }, Vector2 { X: sheen.left, Y: sheen.bottom }, &stops) {
                rt.FillRectangle(&sheen, &g);
            }
        }
        DockArtLayer::CyberHud => {
            br.SetColor(&rgba(0, 240, 255, 0.09));
            let step = 18.0 * u;
            let mut gx = sr.left - h;
            while gx < sr.right + h {
                rt.DrawLine(
                    Vector2 { X: gx, Y: sr.bottom },
                    Vector2 { X: gx + h * 0.75, Y: sr.top },
                    br,
                    0.8 * u,
                    None,
                );
                gx += step;
            }
            let scan_pos = (t * 0.36).sin() * 0.45 + 0.5;
            if is_vertical {
                let sy = sr.top + h * scan_pos;
                let s_stops = [
                    D2D1_GRADIENT_STOP { position: 0.0, color: rgba(0, 240, 255, 0.0) },
                    D2D1_GRADIENT_STOP { position: 0.5, color: rgba(0, 240, 255, 0.34) },
                    D2D1_GRADIENT_STOP { position: 1.0, color: rgba(255, 62, 132, 0.0) },
                ];
                if let Some(g) = create_grad(rt, Vector2 { X: sr.left, Y: sy - 12.0 * u }, Vector2 { X: sr.left, Y: sy + 12.0 * u }, &s_stops) {
                    rt.FillRectangle(&D2D_RECT_F { left: sr.left, top: sy - 12.0 * u, right: sr.right, bottom: sy + 12.0 * u }, &g);
                }
            } else {
                let sx = sr.left + w * scan_pos;
                let s_stops = [
                    D2D1_GRADIENT_STOP { position: 0.0, color: rgba(0, 240, 255, 0.0) },
                    D2D1_GRADIENT_STOP { position: 0.5, color: rgba(0, 240, 255, 0.34) },
                    D2D1_GRADIENT_STOP { position: 1.0, color: rgba(255, 62, 132, 0.0) },
                ];
                if let Some(g) = create_grad(rt, Vector2 { X: sx - 18.0 * u, Y: sr.top }, Vector2 { X: sx + 18.0 * u, Y: sr.top }, &s_stops) {
                    rt.FillRectangle(&D2D_RECT_F { left: sx - 18.0 * u, top: sr.top, right: sx + 18.0 * u, bottom: sr.bottom }, &g);
                }
            }
        }
        DockArtLayer::IslandTelemetry => {
            let crown_h = (h * 0.34).clamp(6.0 * u, 20.0 * u);
            let crown = D2D_RECT_F {
                left: sr.left,
                top: sr.top,
                right: sr.right,
                bottom: sr.top + crown_h,
            };
            let crown_stops = [
                D2D1_GRADIENT_STOP { position: 0.0, color: rgba(255, 255, 255, 0.14) },
                D2D1_GRADIENT_STOP { position: 0.55, color: rgba(255, 255, 255, 0.04) },
                D2D1_GRADIENT_STOP { position: 1.0, color: rgba(255, 255, 255, 0.0) },
            ];
            if let Some(g) = create_grad(rt, Vector2 { X: crown.left, Y: crown.top }, Vector2 { X: crown.left, Y: crown.bottom }, &crown_stops) {
                rt.FillRectangle(&crown, &g);
            }
            let glint_pad = (h * 0.22).min(w * 0.16);
            let hl_y = sr.top + 0.9 * u;
            let hl_stops = [
                D2D1_GRADIENT_STOP { position: 0.0, color: rgba(255, 255, 255, 0.0) },
                D2D1_GRADIENT_STOP { position: 0.22, color: rgba(242, 248, 255, 0.26 + 0.14 * hover_mix) },
                D2D1_GRADIENT_STOP { position: 0.5, color: rgba(255, 255, 255, 0.42 + 0.16 * hover_mix) },
                D2D1_GRADIENT_STOP { position: 0.78, color: rgba(242, 248, 255, 0.26 + 0.14 * hover_mix) },
                D2D1_GRADIENT_STOP { position: 1.0, color: rgba(255, 255, 255, 0.0) },
            ];
            if let Some(hg) = create_grad(
                rt,
                Vector2 { X: sr.left + glint_pad, Y: hl_y },
                Vector2 { X: sr.right - glint_pad, Y: hl_y },
                &hl_stops,
            ) {
                rt.DrawLine(
                    Vector2 { X: sr.left + glint_pad, Y: hl_y },
                    Vector2 { X: sr.right - glint_pad, Y: hl_y },
                    &hg,
                    1.0 * u,
                    None,
                );
            }
        }
        DockArtLayer::AquaReflect => {
            let dome = D2D_RECT_F {
                left: sr.left,
                top: sr.top,
                right: sr.right,
                bottom: sr.top + h * 0.48,
            };
            let g_stops = [
                D2D1_GRADIENT_STOP { position: 0.0, color: rgba(232, 252, 255, 0.52) },
                D2D1_GRADIENT_STOP { position: 0.85, color: rgba(164, 238, 255, 0.16) },
                D2D1_GRADIENT_STOP { position: 1.0, color: rgba(84, 214, 255, 0.04) },
            ];
            if let Some(g) = create_grad(rt, Vector2 { X: dome.left, Y: dome.top }, Vector2 { X: dome.left, Y: dome.bottom }, &g_stops) {
                rt.FillRectangle(&dome, &g);
            }
            let r_axis = if is_vertical { sr.top + h * focus_axis } else { sr.left + w * focus_axis };
            let rc_pt = if is_vertical {
                Vector2 { X: (sr.left + sr.right) * 0.5, Y: r_axis }
            } else {
                Vector2 { X: r_axis, Y: sr.bottom - h * 0.25 }
            };
            for r_i in 0..2 {
                let phase = ((t * 0.85 + r_i as f32 * 0.5) % 1.0).clamp(0.0, 1.0);
                br.SetColor(&rgba(186, 248, 255, (1.0 - phase) * (0.14 + 0.22 * hover_mix)));
                rt.DrawEllipse(
                    &D2D1_ELLIPSE { point: rc_pt, radiusX: (10.0 + phase * 32.0) * u, radiusY: (4.0 + phase * 10.0) * u },
                    br,
                    1.1 * u,
                    None,
                );
            }
        }
        DockArtLayer::GhostAura => {}
        DockArtLayer::AuroraSilk => {
            let segs = 24;
            let wave_cols = [(46u8, 244u8, 188u8, 0.28), (64, 198, 255, 0.24), (176, 118, 255, 0.22)];
            for (wi, &(r, g, b, al)) in wave_cols.iter().enumerate() {
                let phase_off = wi as f32 * 1.9;
                let amp = h * (0.20 + wi as f32 * 0.05) * (1.0 + 0.25 * hover_mix);
                for s in 0..segs {
                    let f0 = s as f32 / segs as f32;
                    let f1 = (s + 1) as f32 / segs as f32;
                    let boost = 1.0 + 0.5 * hover_mix * (1.0 - ((f0 - focus_axis) * 3.0).abs().min(1.0));
                    br.SetColor(&rgba(r, g, b, (al * boost).min(0.55)));
                    if is_vertical {
                        let y0 = sr.top + h * f0;
                        let y1 = sr.top + h * f1;
                        let x0 = (sr.left + sr.right) * 0.5 + (f0 * 5.5 + t * 1.2 + phase_off).sin() * amp;
                        let x1 = (sr.left + sr.right) * 0.5 + (f1 * 5.5 + t * 1.2 + phase_off).sin() * amp;
                        rt.DrawLine(Vector2 { X: x0, Y: y0 }, Vector2 { X: x1, Y: y1 }, br, 8.0 * u, None);
                    } else {
                        let x0 = sr.left + w * f0;
                        let x1 = sr.left + w * f1;
                        let y0 = (sr.top + sr.bottom) * 0.5 + (f0 * 6.0 + t * 1.2 + phase_off).sin() * amp;
                        let y1 = (sr.top + sr.bottom) * 0.5 + (f1 * 6.0 + t * 1.2 + phase_off).sin() * amp;
                        rt.DrawLine(Vector2 { X: x0, Y: y0 }, Vector2 { X: x1, Y: y1 }, br, 9.5 * u, None);
                    }
                }
            }
        }
        DockArtLayer::SlateTiles => {
            br.SetColor(&rgba(255, 255, 255, 0.05));
            let mut y = sr.top + 3.0 * u;
            while y < sr.bottom {
                rt.DrawLine(Vector2 { X: sr.left, Y: y }, Vector2 { X: sr.right, Y: y }, br, 0.7 * u, None);
                y += 3.5 * u;
            }
        }
        DockArtLayer::KumikoLattice => {
            br.SetColor(&rgba(232, 184, 106, 0.16));
            let step = 16.0 * u;
            let mut x = sr.left - h;
            while x < sr.right + h {
                rt.DrawLine(Vector2 { X: x, Y: sr.top }, Vector2 { X: x + h, Y: sr.bottom }, br, 0.9 * u, None);
                rt.DrawLine(Vector2 { X: x + h, Y: sr.top }, Vector2 { X: x, Y: sr.bottom }, br, 0.9 * u, None);
                x += step;
            }
        }
        DockArtLayer::LiquidChrome => {
            let sweep = ((t * 0.42).sin() * 0.38 + 0.5).clamp(0.1, 0.9);
            let center_n = sweep * (1.0 - 0.5 * hover_mix) + focus_axis * 0.5 * hover_mix;
            let p_x = sr.left + w * center_n;
            let m_stops = [
                D2D1_GRADIENT_STOP { position: 0.0, color: rgba(255, 255, 255, 0.0) },
                D2D1_GRADIENT_STOP { position: 0.42, color: rgba(226, 236, 252, 0.28) },
                D2D1_GRADIENT_STOP { position: 0.50, color: rgba(255, 255, 255, 0.44) },
                D2D1_GRADIENT_STOP { position: 0.58, color: rgba(196, 212, 236, 0.24) },
                D2D1_GRADIENT_STOP { position: 1.0, color: rgba(255, 255, 255, 0.0) },
            ];
            let span = 44.0 * u;
            if let Some(g) = create_grad(rt, Vector2 { X: p_x - span, Y: sr.top }, Vector2 { X: p_x + span, Y: sr.bottom }, &m_stops) {
                rt.FillRectangle(&sr, &g);
            }
        }
        DockArtLayer::SakuraPetals => {
            for i in 0..14 {
                let fi = i as f32;
                let seed = fi * 0.6180339;
                let u_pos = (seed + t * (0.045 + (i % 3) as f32 * 0.015) * (1.0 + hover_mix * 0.9)) % 1.0;
                let v_pos = (seed * 2.3 + (t * 0.9 + fi).sin() * 0.28).fract().abs();
                let px = if is_vertical { sr.left + w * (0.14 + 0.72 * v_pos) } else { sr.left + w * u_pos };
                let py = if is_vertical { sr.top + h * u_pos } else { sr.top + h * (0.14 + 0.72 * v_pos) };
                let (sa, ca) = (t * 1.4 + fi * 1.1).sin_cos();
                let sz = (2.8 + (i % 3) as f32 * 1.1) * u;
                br.SetColor(&rgba(255, 178 + (i as u8 % 4) * 14, 212, 0.65));
                rt.FillEllipse(
                    &D2D1_ELLIPSE {
                        point: Vector2 { X: px, Y: py },
                        radiusX: (sz * (0.65 + 0.35 * ca.abs())).max(1.2),
                        radiusY: (sz * 0.55 * (0.6 + 0.4 * sa.abs())).max(1.0),
                    },
                    br,
                );
            }
        }
        DockArtLayer::AstrolabeTicks => {
            br.SetColor(&rgba(228, 184, 92, 0.38));
            if is_vertical {
                let mut ty = sr.top + 8.0 * u;
                let mut idx = 0usize;
                while ty < sr.bottom - 8.0 * u {
                    let len = if idx % 5 == 0 { 7.5 * u } else { 3.5 * u };
                    rt.DrawLine(Vector2 { X: sr.left + 2.0 * u, Y: ty }, Vector2 { X: sr.left + 2.0 * u + len, Y: ty }, br, 0.9 * u, None);
                    ty += 7.0 * u;
                    idx += 1;
                }
            } else {
                let mut tx = sr.left + 10.0 * u;
                let mut idx = 0usize;
                while tx < sr.right - 10.0 * u {
                    let len = if idx % 5 == 0 { 6.5 * u } else { 3.0 * u };
                    rt.DrawLine(Vector2 { X: tx, Y: sr.bottom - 2.0 * u }, Vector2 { X: tx, Y: sr.bottom - 2.0 * u - len }, br, 0.9 * u, None);
                    tx += 7.0 * u;
                    idx += 1;
                }
            }
        }
        DockArtLayer::VaporGrid => {
            let sun_band = D2D_RECT_F { left: sr.left, top: sr.top, right: sr.right, bottom: sr.top + h * 0.32 };
            let s_stops = [
                D2D1_GRADIENT_STOP { position: 0.0, color: rgba(255, 168, 86, 0.32) },
                D2D1_GRADIENT_STOP { position: 1.0, color: rgba(255, 76, 174, 0.0) },
            ];
            if let Some(g) = create_grad(rt, Vector2 { X: sr.left, Y: sun_band.top }, Vector2 { X: sr.left, Y: sun_band.bottom }, &s_stops) {
                rt.FillRectangle(&sun_band, &g);
            }
            br.SetColor(&rgba(255, 96, 196, 0.30));
            let vp_x = sr.left + w * (0.5 + (focus_axis - 0.5) * 0.25 * hover_mix);
            let vp_y = sr.top + h * 0.20;
            if !is_vertical {
                for r_i in 0..4 {
                    let yf = sr.top + h * (0.44 + r_i as f32 * 0.16);
                    rt.DrawLine(Vector2 { X: sr.left, Y: yf }, Vector2 { X: sr.right, Y: yf }, br, 0.9 * u, None);
                }
                for i in 0..=14 {
                    let frac = i as f32 / 14.0;
                    let bx = sr.left + (frac * 1.4 - 0.2) * w;
                    let top_x = vp_x + (bx - vp_x) * 0.42;
                    rt.DrawLine(Vector2 { X: top_x, Y: vp_y }, Vector2 { X: bx, Y: sr.bottom }, br, 0.9 * u, None);
                }
            } else {
                for r_i in 0..3 {
                    let xf = sr.left + w * (0.28 + r_i as f32 * 0.25);
                    rt.DrawLine(Vector2 { X: xf, Y: sr.top }, Vector2 { X: xf, Y: sr.bottom }, br, 0.9 * u, None);
                }
            }
        }
        DockArtLayer::PaperHalftone => {
            br.SetColor(&rgba(34, 32, 28, 0.09));
            let gap = 7.0 * u;
            let mut y = sr.top + 4.5 * u;
            let mut row = 0usize;
            while y < sr.bottom - 3.0 * u {
                let mut x = sr.left + if row % 2 == 0 { 5.0 * u } else { 8.5 * u };
                while x < sr.right - 3.0 * u {
                    rt.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: x, Y: y }, radiusX: 0.75 * u, radiusY: 0.75 * u }, br);
                    x += gap;
                }
                y += gap;
                row += 1;
            }
        }
        DockArtLayer::BiolumTendrils | DockArtLayer::PrismSpectral => {}
        DockArtLayer::PhosphorCrt => {
            br.SetColor(&rgba(76, 255, 132, 0.11));
            let mut y = sr.top + 2.0 * u;
            while y < sr.bottom {
                rt.DrawLine(Vector2 { X: sr.left, Y: y }, Vector2 { X: sr.right, Y: y }, br, 0.8 * u, None);
                y += 3.0 * u;
            }
            if !is_vertical {
                br.SetColor(&rgba(76, 255, 132, 0.34));
                let base_y = sr.bottom - 5.5 * u;
                for s in 0..32 {
                    let f0 = s as f32 / 32.0;
                    let f1 = (s + 1) as f32 / 32.0;
                    let y0 = base_y + (f0 * 14.0 - t * 5.0).sin() * 2.2 * u;
                    let y1 = base_y + (f1 * 14.0 - t * 5.0).sin() * 2.2 * u;
                    rt.DrawLine(Vector2 { X: sr.left + w * f0, Y: y0 }, Vector2 { X: sr.left + w * f1, Y: y1 }, br, 1.0 * u, None);
                }
            }
        }
        DockArtLayer::ObsidianMagma => {
            let pulse = 0.65 + 0.35 * (t * 2.6).sin();
            let seam_stops = [
                D2D1_GRADIENT_STOP { position: 0.0, color: rgba(255, 76, 22, 0.0) },
                D2D1_GRADIENT_STOP { position: 1.0, color: rgba(255, 114, 34, 0.32 * pulse) },
            ];
            if let Some(g) = create_grad(rt, Vector2 { X: sr.left, Y: sr.top + h * 0.5 }, Vector2 { X: sr.left, Y: sr.bottom }, &seam_stops) {
                rt.FillRectangle(&D2D_RECT_F { left: sr.left, top: sr.top + h * 0.5, right: sr.right, bottom: sr.bottom }, &g);
            }
            for i in 0..14 {
                let fi = i as f32;
                let rise = (fi * 0.41 + t * (0.28 + (i % 3) as f32 * 0.08)) % 1.0;
                let n = (fi * 0.273 + (rise * 5.0 + fi).sin() * 0.04).fract().abs();
                let (px, py) = if is_vertical {
                    (sr.left + w * (1.0 - rise), sr.top + h * (0.08 + 0.84 * n))
                } else {
                    (sr.left + w * (0.08 + 0.84 * n), sr.bottom - h * rise)
                };
                br.SetColor(&rgba(255, 146 + ((i * 29) as u8 % 80), 46, (1.0 - rise) * 0.85));
                rt.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: px, Y: py }, radiusX: 1.8 * u, radiusY: 1.8 * u }, br);
            }
        }
        DockArtLayer::BauhausPrimary => {
            br.SetColor(&rgba(62, 134, 242, 0.88));
            rt.FillEllipse(
                &D2D1_ELLIPSE { point: Vector2 { X: sr.left + 2.0 * u, Y: sr.bottom }, radiusX: 15.0 * u, radiusY: 15.0 * u },
                br,
            );
            br.SetColor(&rgba(244, 196, 48, 0.92));
            rt.FillEllipse(
                &D2D1_ELLIPSE { point: Vector2 { X: sr.right - 12.0 * u, Y: sr.top + 10.0 * u }, radiusX: 5.0 * u, radiusY: 5.0 * u },
                br,
            );
            br.SetColor(&rgba(228, 68, 48, 0.88));
            rt.FillRectangle(
                &D2D_RECT_F { left: sr.left + 20.0 * u, top: sr.top, right: sr.left + 34.0 * u, bottom: sr.top + 4.5 * u },
                br,
            );
        }
        DockArtLayer::StarfieldWarp => {
            for i in 0..16 {
                let fi = i as f32;
                let sx_n = ((fi * 0.618 + (i % 4) as f32 * 0.13) % 0.92) + 0.04;
                let sy_n = ((fi * 0.382 + (i % 3) as f32 * 0.19) % 0.84) + 0.08;
                let px = sr.left + w * sx_n;
                let py = sr.top + h * sy_n;
                let tw = 0.4 + 0.6 * ((t * 2.4 + fi * 1.7).sin() * 0.5 + 0.5);
                let streak = hover_mix * (4.0 + (i % 3) as f32 * 4.0) * u;
                br.SetColor(&rgba(196, 232, 255, tw * 0.85));
                if streak > 1.2 * u {
                    let dx = (sx_n - focus_axis).clamp(-0.5, 0.5) * streak * 2.0;
                    rt.DrawLine(Vector2 { X: px - dx, Y: py }, Vector2 { X: px + dx, Y: py }, br, 1.3 * u, None);
                } else {
                    rt.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: px, Y: py }, radiusX: 1.2 * u, radiusY: 1.2 * u }, br);
                }
            }
        }
        DockArtLayer::BrushedTitanium => {
            // Anisotropic brushed titanium sheen reacting gently to cursor focus
            let glint_x = sr.left + w * (0.28 + 0.44 * focus_axis);
            let m_stops = [
                D2D1_GRADIENT_STOP { position: 0.0, color: rgba(255, 255, 255, 0.0) },
                D2D1_GRADIENT_STOP { position: 0.36, color: rgba(224, 232, 242, 0.16) },
                D2D1_GRADIENT_STOP { position: 0.50, color: rgba(255, 255, 255, 0.28) },
                D2D1_GRADIENT_STOP { position: 0.64, color: rgba(206, 214, 226, 0.14) },
                D2D1_GRADIENT_STOP { position: 1.0, color: rgba(255, 255, 255, 0.0) },
            ];
            let span = (w * 0.32).max(46.0 * u);
            if let Some(g) = create_grad(rt, Vector2 { X: glint_x - span, Y: sr.top }, Vector2 { X: glint_x + span, Y: sr.bottom }, &m_stops) {
                rt.FillRectangle(&sr, &g);
            }
            // Ultra-fine brushed hairline metal scratches
            let mut y = sr.top + 1.8 * u;
            let mut row = 0usize;
            while y < sr.bottom - 1.5 * u {
                let a = if row % 3 == 0 { 0.085 } else if row % 2 == 0 { 0.04 } else { 0.025 };
                if row % 4 == 1 {
                    br.SetColor(&rgba(16, 20, 26, a * 1.4));
                } else {
                    br.SetColor(&rgba(242, 248, 255, a));
                }
                rt.DrawLine(Vector2 { X: sr.left, Y: y }, Vector2 { X: sr.right, Y: y }, br, 0.65 * u, None);
                y += 2.2 * u;
                row += 1;
            }
            // CNC chamfered top specular strip + milled bottom shadow lip
            br.SetColor(&rgba(255, 255, 255, 0.32));
            rt.DrawLine(Vector2 { X: sr.left + 4.0 * u, Y: sr.top + 1.4 * u }, Vector2 { X: sr.right - 4.0 * u, Y: sr.top + 1.4 * u }, br, 1.1 * u, None);
            br.SetColor(&rgba(10, 12, 16, 0.45));
            rt.DrawLine(Vector2 { X: sr.left + 4.0 * u, Y: sr.bottom - 1.6 * u }, Vector2 { X: sr.right - 4.0 * u, Y: sr.bottom - 1.6 * u }, br, 1.2 * u, None);
            // 4 CNC recessed corner Torx bolts
            let pad = 7.0 * u;
            let bolt_r = 2.4 * u;
            if w > 48.0 * u && h > 22.0 * u {
                for &(bx, by) in &[
                    (sr.left + pad, sr.top + pad),
                    (sr.right - pad, sr.top + pad),
                    (sr.left + pad, sr.bottom - pad),
                    (sr.right - pad, sr.bottom - pad),
                ] {
                    br.SetColor(&rgba(18, 20, 26, 0.72));
                    rt.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: bx, Y: by }, radiusX: bolt_r, radiusY: bolt_r }, br);
                    br.SetColor(&rgba(220, 228, 238, 0.55));
                    rt.DrawEllipse(&D2D1_ELLIPSE { point: Vector2 { X: bx, Y: by }, radiusX: bolt_r, radiusY: bolt_r }, br, 0.7 * u, None);
                    br.SetColor(&rgba(180, 190, 204, 0.45));
                    rt.DrawLine(Vector2 { X: bx - 1.2 * u, Y: by }, Vector2 { X: bx + 1.2 * u, Y: by }, br, 0.65 * u, None);
                    rt.DrawLine(Vector2 { X: bx, Y: by - 1.2 * u }, Vector2 { X: bx, Y: by + 1.2 * u }, br, 0.65 * u, None);
                }
            }
        }
        DockArtLayer::VacuumTubeHiFi => {
            // Champagne brushed brass horizontal faceplate highlight
            let brass_stops = [
                D2D1_GRADIENT_STOP { position: 0.0, color: rgba(232, 188, 114, 0.14) },
                D2D1_GRADIENT_STOP { position: 0.45, color: rgba(164, 112, 54, 0.05) },
                D2D1_GRADIENT_STOP { position: 1.0, color: rgba(255, 148, 42, 0.18) },
            ];
            if let Some(g) = create_grad(rt, Vector2 { X: sr.left, Y: sr.top }, Vector2 { X: sr.left, Y: sr.bottom }, &brass_stops) {
                rt.FillRectangle(&sr, &g);
            }
            // Warm glowing 12AX7 glass triode filament blooms distributed cleanly
            let tube_n = if w > 240.0 * u { 4usize } else { 2usize };
            for i in 0..tube_n {
                let frac = (i as f32 + 0.5) / tube_n as f32;
                let tx = if is_vertical { (sr.left + sr.right) * 0.5 } else { sr.left + w * (0.14 + 0.72 * frac) };
                let ty = if is_vertical { sr.top + h * (0.14 + 0.72 * frac) } else { sr.top + h * 0.24 };
                let heat = 0.74 + 0.26 * (t * 2.1 + i as f32 * 1.4).sin() + 0.12 * hover_mix;
                // Outer warm tungsten bloom
                br.SetColor(&rgba(255, 134, 28, (0.14 * heat).min(0.32)));
                rt.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: tx, Y: ty + 4.0 * u }, radiusX: 16.0 * u, radiusY: 10.0 * u }, br);
                // Glass dome envelope
                br.SetColor(&rgba(255, 186, 86, (0.28 * heat).min(0.55)));
                rt.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: tx, Y: ty + 2.0 * u }, radiusX: 6.5 * u, radiusY: 5.5 * u }, br);
                // Glowing incandescent filament core
                br.SetColor(&rgba(255, 224, 148, (0.85 * heat).min(0.98)));
                rt.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: tx, Y: ty + 2.5 * u }, radiusX: 2.2 * u, radiusY: 1.6 * u }, br);
                // Specular glass glint
                br.SetColor(&rgba(255, 248, 232, 0.45));
                rt.DrawLine(Vector2 { X: tx - 3.6 * u, Y: ty - 0.5 * u }, Vector2 { X: tx - 2.4 * u, Y: ty + 2.0 * u }, br, 0.8 * u, None);
            }
            // Incandescent engraved brass trim rail along bottom
            br.SetColor(&rgba(232, 186, 108, 0.42));
            rt.DrawLine(Vector2 { X: sr.left + 8.0 * u, Y: sr.bottom - 3.0 * u }, Vector2 { X: sr.right - 8.0 * u, Y: sr.bottom - 3.0 * u }, br, 0.9 * u, None);
        }
        DockArtLayer::SwissGuilloche => {
            // Engine-turned Clous de Paris / wave guilloché texture
            br.SetColor(&rgba(148, 184, 236, 0.11));
            let row_step = 5.5 * u;
            let segs = 28;
            let mut ry = sr.top + 4.0 * u;
            let mut row_i = 0usize;
            while ry < sr.bottom - 2.0 * u {
                let phase = if row_i % 2 == 0 { 0.0 } else { PI * 0.5 };
                for s in 0..segs {
                    let f0 = s as f32 / segs as f32;
                    let f1 = (s + 1) as f32 / segs as f32;
                    let x0 = sr.left + w * f0;
                    let x1 = sr.left + w * f1;
                    let y0 = ry + (f0 * 24.0 + phase).sin() * 1.4 * u;
                    let y1 = ry + (f1 * 24.0 + phase).sin() * 1.4 * u;
                    rt.DrawLine(Vector2 { X: x0, Y: y0 }, Vector2 { X: x1, Y: y1 }, br, 0.65 * u, None);
                }
                ry += row_step;
                row_i += 1;
            }
            // Sapphire crystal diagonal anti-reflective (AR) cobalt glare streak
            let ar_x = sr.left + w * (0.25 + 0.5 * ((t * 0.25).sin() * 0.5 + 0.5));
            let ar_stops = [
                D2D1_GRADIENT_STOP { position: 0.0, color: rgba(96, 166, 255, 0.0) },
                D2D1_GRADIENT_STOP { position: 0.48, color: rgba(154, 206, 255, 0.20) },
                D2D1_GRADIENT_STOP { position: 0.52, color: rgba(236, 246, 255, 0.28) },
                D2D1_GRADIENT_STOP { position: 1.0, color: rgba(96, 166, 255, 0.0) },
            ];
            if let Some(g) = create_grad(rt, Vector2 { X: ar_x - 36.0 * u, Y: sr.top }, Vector2 { X: ar_x + 36.0 * u, Y: sr.bottom }, &ar_stops) {
                rt.FillRectangle(&sr, &g);
            }
            // Miniature watchmaker synthetic ruby pivot jewels at corners
            let jp = 8.0 * u;
            if w > 54.0 * u && h > 22.0 * u {
                for &(jx, jy) in &[(sr.left + jp, (sr.top + sr.bottom) * 0.5), (sr.right - jp, (sr.top + sr.bottom) * 0.5)] {
                    br.SetColor(&rgba(218, 226, 240, 0.72));
                    rt.DrawEllipse(&D2D1_ELLIPSE { point: Vector2 { X: jx, Y: jy }, radiusX: 3.0 * u, radiusY: 3.0 * u }, br, 0.9 * u, None);
                    br.SetColor(&rgba(212, 24, 68, 0.92));
                    rt.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: jx, Y: jy }, radiusX: 1.9 * u, radiusY: 1.9 * u }, br);
                    br.SetColor(&rgba(255, 196, 212, 0.85));
                    rt.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: jx - 0.5 * u, Y: jy - 0.5 * u }, radiusX: 0.6 * u, radiusY: 0.6 * u }, br);
                }
            }
        }
        DockArtLayer::SaddleLeather => {
            // Subtle warm burnished calfskin sheen & organic stipple grain
            let l_stops = [
                D2D1_GRADIENT_STOP { position: 0.0, color: rgba(168, 108, 62, 0.22) },
                D2D1_GRADIENT_STOP { position: 0.5, color: rgba(84, 46, 24, 0.0) },
                D2D1_GRADIENT_STOP { position: 1.0, color: rgba(24, 12, 6, 0.34) },
            ];
            if let Some(g) = create_grad(rt, Vector2 { X: sr.left, Y: sr.top }, Vector2 { X: sr.left, Y: sr.bottom }, &l_stops) {
                rt.FillRectangle(&sr, &g);
            }
            // Subtle leather pore grain dots
            br.SetColor(&rgba(34, 16, 8, 0.16));
            let mut gy = sr.top + 6.0 * u;
            let mut grow = 0usize;
            while gy < sr.bottom - 5.0 * u {
                let mut gx = sr.left + 8.0 * u + if grow % 2 == 0 { 0.0 } else { 5.5 * u };
                while gx < sr.right - 8.0 * u {
                    rt.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: gx, Y: gy }, radiusX: 0.8 * u, radiusY: 0.65 * u }, br);
                    gx += 11.0 * u;
                }
                gy += 7.0 * u;
                grow += 1;
            }
            // Authentic saddle perimeter stitching (tilted cream waxed thread + shadow)
            let ins = 4.6 * u;
            let step = 8.0 * u;
            let st_len = 4.4 * u;
            let mut sx = sr.left + ins + 4.0 * u;
            while sx < sr.right - ins - 4.0 * u {
                for &sy in &[sr.top + ins, sr.bottom - ins] {
                    br.SetColor(&rgba(22, 10, 4, 0.55));
                    rt.DrawLine(Vector2 { X: sx, Y: sy + 0.9 * u }, Vector2 { X: sx + st_len, Y: sy + 0.4 * u }, br, 1.15 * u, None);
                    br.SetColor(&rgba(246, 230, 198, 0.86));
                    rt.DrawLine(Vector2 { X: sx, Y: sy + 0.2 * u }, Vector2 { X: sx + st_len, Y: sy - 0.3 * u }, br, 1.05 * u, None);
                }
                sx += step;
            }
            // Antique brass rivets on left & right strap ends
            if w > 54.0 * u && h > 22.0 * u {
                let rp = 10.0 * u;
                for &(rx, ry) in &[(sr.left + rp, (sr.top + sr.bottom) * 0.5), (sr.right - rp, (sr.top + sr.bottom) * 0.5)] {
                    br.SetColor(&rgba(24, 12, 6, 0.60));
                    rt.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: rx, Y: ry + 0.9 * u }, radiusX: 3.1 * u, radiusY: 3.1 * u }, br);
                    br.SetColor(&rgba(198, 152, 74, 0.94));
                    rt.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: rx, Y: ry }, radiusX: 2.7 * u, radiusY: 2.7 * u }, br);
                    br.SetColor(&rgba(255, 240, 196, 0.80));
                    rt.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: rx - 0.8 * u, Y: ry - 0.8 * u }, radiusX: 0.8 * u, radiusY: 0.8 * u }, br);
                }
            }
        }
        DockArtLayer::CarbonWeave => {
            // Genuine 2x2 diagonal twill carbon fiber micro-weave
            let cell = 6.0 * u;
            let mut cy = sr.top;
            let mut row = 0usize;
            while cy < sr.bottom {
                let mut cx = sr.left;
                let mut col = 0usize;
                while cx < sr.right {
                    let phase = (col + row) % 4;
                    if phase < 2 {
                        br.SetColor(&rgba(64, 68, 76, 0.26));
                    } else {
                        br.SetColor(&rgba(8, 9, 12, 0.34));
                    }
                    rt.FillRectangle(
                        &D2D_RECT_F { left: cx, top: cy, right: cx + cell - 0.6 * u, bottom: cy + cell * 0.55 },
                        br,
                    );
                    cx += cell;
                    col += 1;
                }
                cy += cell * 0.65;
                row += 1;
            }
            // Soft vulcanite matte top edge sheen
            let v_stops = [
                D2D1_GRADIENT_STOP { position: 0.0, color: rgba(255, 255, 255, 0.11) },
                D2D1_GRADIENT_STOP { position: 0.35, color: rgba(255, 255, 255, 0.0) },
            ];
            if let Some(g) = create_grad(rt, Vector2 { X: sr.left, Y: sr.top }, Vector2 { X: sr.left, Y: sr.bottom }, &v_stops) {
                rt.FillRectangle(&sr, &g);
            }
            // Iconic ruby-red Leica enamel studio dot on left shoulder
            if w > 48.0 * u {
                let dx = sr.left + 9.0 * u;
                let dy = sr.top + 8.5 * u;
                br.SetColor(&rgba(228, 22, 52, 0.96));
                rt.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: dx, Y: dy }, radiusX: 2.5 * u, radiusY: 2.5 * u }, br);
                br.SetColor(&rgba(255, 196, 206, 0.72));
                rt.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: dx - 0.7 * u, Y: dy - 0.7 * u }, radiusX: 0.7 * u, radiusY: 0.7 * u }, br);
            }
        }
        DockArtLayer::CassetteDeck1984 => {
            // Two-tone 1984 molded ABS casing stripe (oatmeal upper bevel + charcoal lower bay)
            br.SetColor(&rgba(224, 214, 196, 0.14));
            rt.FillRectangle(&D2D_RECT_F { left: sr.left, top: sr.top, right: sr.right, bottom: sr.top + h * 0.26 }, br);
            br.SetColor(&rgba(255, 148, 36, 0.55));
            rt.DrawLine(Vector2 { X: sr.left + 4.0 * u, Y: sr.top + h * 0.26 }, Vector2 { X: sr.right - 4.0 * u, Y: sr.top + h * 0.26 }, br, 1.1 * u, None);
            // Rotating dual cassette tape reels subtly visible in background window
            let cy = sr.top + h * 0.62;
            let r_hub = (h * 0.24).clamp(4.0 * u, 10.5 * u);
            let left_cx = sr.left + w * 0.24;
            let right_cx = sr.right - w * 0.24;
            if !is_vertical && w > 90.0 * u {
                // Dark magnetic tape ribbon linking the two spools
                br.SetColor(&rgba(22, 18, 16, 0.55));
                rt.FillRectangle(&D2D_RECT_F { left: left_cx, top: cy - r_hub * 0.65, right: right_cx, bottom: cy + r_hub * 0.65 }, br);
                for (ri, &rcx) in [left_cx, right_cx].iter().enumerate() {
                    br.SetColor(&rgba(16, 14, 12, 0.65));
                    rt.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: rcx, Y: cy }, radiusX: r_hub * 1.15, radiusY: r_hub * 1.15 }, br);
                    br.SetColor(&rgba(232, 222, 206, 0.36));
                    rt.DrawEllipse(&D2D1_ELLIPSE { point: Vector2 { X: rcx, Y: cy }, radiusX: r_hub, radiusY: r_hub }, br, 1.0 * u, None);
                    for sp in 0..3 {
                        let ang = t * 1.6 + ri as f32 * 0.9 + (sp as f32) * (PI * 2.0 / 3.0);
                        let (sa, ca) = ang.sin_cos();
                        rt.DrawLine(
                            Vector2 { X: rcx + ca * r_hub * 0.28, Y: cy + sa * r_hub * 0.28 },
                            Vector2 { X: rcx + ca * r_hub * 0.88, Y: cy + sa * r_hub * 0.88 },
                            br,
                            1.1 * u,
                            None,
                        );
                    }
                }
            }
        }
        DockArtLayer::CrystalWater => {
            // Sunlit crystal-clear lagoon water: underwater caustic light webs + meniscus + bubbles + hover ripples
            let meniscus = D2D_RECT_F { left: sr.left, top: sr.top, right: sr.right, bottom: sr.top + h * 0.44 };
            let m_stops = [
                D2D1_GRADIENT_STOP { position: 0.0, color: rgba(232, 254, 255, 0.46) },
                D2D1_GRADIENT_STOP { position: 0.55, color: rgba(112, 236, 252, 0.16) },
                D2D1_GRADIENT_STOP { position: 1.0, color: rgba(24, 178, 224, 0.0) },
            ];
            if let Some(g) = create_grad(rt, Vector2 { X: meniscus.left, Y: meniscus.top }, Vector2 { X: meniscus.left, Y: meniscus.bottom }, &m_stops) {
                rt.FillRectangle(&meniscus, &g);
            }
            // Layered animated underwater caustic light interference ribbons
            let segs = 26;
            for layer in 0..3 {
                let lf = layer as f32;
                let col_a = 0.22 - lf * 0.04;
                br.SetColor(&rgba(168 + layer as u8 * 24, 248, 255, col_a));
                for s in 0..segs {
                    let f0 = s as f32 / segs as f32;
                    let f1 = (s + 1) as f32 / segs as f32;
                    let y0 = sr.top + h * (0.34 + lf * 0.20) + (f0 * (7.0 + lf * 2.5) + t * (1.1 - lf * 0.22) + lf * 1.8).sin() * (h * 0.11);
                    let y1 = sr.top + h * (0.34 + lf * 0.20) + (f1 * (7.0 + lf * 2.5) + t * (1.1 - lf * 0.22) + lf * 1.8).sin() * (h * 0.11);
                    rt.DrawLine(Vector2 { X: sr.left + w * f0, Y: y0 }, Vector2 { X: sr.left + w * f1, Y: y1 }, br, (2.2 - lf * 0.4) * u, None);
                }
            }
            // Rising 3D refractive water bubbles
            for i in 0..10 {
                let fi = i as f32;
                let bx_n = ((fi * 0.618 + (t * 0.05 + fi).sin() * 0.03) % 0.90) + 0.05;
                let rise = (fi * 0.37 + t * (0.14 + (i % 3) as f32 * 0.05)) % 1.0;
                let px = sr.left + w * bx_n;
                let py = sr.bottom - h * (0.10 + 0.80 * rise);
                let rad = (1.6 + (i % 3) as f32 * 1.0) * u;
                br.SetColor(&rgba(186, 250, 255, 0.36));
                rt.DrawEllipse(&D2D1_ELLIPSE { point: Vector2 { X: px, Y: py }, radiusX: rad, radiusY: rad }, br, 0.85 * u, None);
                br.SetColor(&rgba(246, 255, 255, 0.78));
                rt.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: px - rad * 0.32, Y: py - rad * 0.32 }, radiusX: (rad * 0.28).max(0.6), radiusY: (rad * 0.28).max(0.6) }, br);
            }
            // Concentric water ripple rings radiating smoothly from cursor position
            let rx = if is_vertical { (sr.left + sr.right) * 0.5 } else { sr.left + w * focus_axis };
            let ry = if is_vertical { sr.top + h * focus_axis } else { sr.top + h * 0.56 };
            for r_i in 0..3 {
                let p = ((t * 0.65 + r_i as f32 * 0.33) % 1.0).clamp(0.0, 1.0);
                br.SetColor(&rgba(218, 254, 255, (1.0 - p) * (0.24 + 0.28 * hover_mix)));
                rt.DrawEllipse(
                    &D2D1_ELLIPSE { point: Vector2 { X: rx, Y: ry }, radiusX: (8.0 + p * 38.0) * u, radiusY: (3.2 + p * 12.0) * u },
                    br,
                    1.05 * u,
                    None,
                );
            }
        }
        DockArtLayer::RainWindowGlass => {
            // Soft bokeh glow orbs seen through wet twilight glass
            for &(bx_n, by_n, rad, col) in &[
                (0.18f32, 0.36f32, 16.0f32, (255u8, 198u8, 120u8)),
                (0.52, 0.58, 20.0, (112, 184, 248)),
                (0.82, 0.40, 15.0, (244, 178, 126)),
            ] {
                br.SetColor(&rgba(col.0, col.1, col.2, 0.10));
                rt.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: sr.left + w * bx_n, Y: sr.top + h * by_n }, radiusX: rad * u, radiusY: rad * 0.72 * u }, br);
            }
            // Sliding rainwater trails + 3D refractive glass droplets
            for i in 0..14 {
                let fi = i as f32;
                let dx_n = ((fi * 0.61803 + 0.08) % 0.88) + 0.06;
                let slide = if i % 4 == 0 { (t * 0.09 + fi * 0.29) % 1.0 } else { (fi * 0.431) % 0.82 + 0.10 };
                let px = sr.left + w * dx_n;
                let py = sr.top + h * (0.12 + 0.76 * slide);
                let rx = (1.8 + (i % 3) as f32 * 0.95) * u;
                let ry = rx * (1.12 + if i % 4 == 0 { 0.24 } else { 0.0 });
                if i % 4 == 0 {
                    br.SetColor(&rgba(196, 228, 255, 0.14));
                    rt.DrawLine(Vector2 { X: px, Y: sr.top }, Vector2 { X: px, Y: py - ry }, br, 0.9 * u, None);
                }
                // Drop shadow
                br.SetColor(&rgba(4, 10, 20, 0.42));
                rt.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: px + 0.6 * u, Y: py + 1.1 * u }, radiusX: rx, radiusY: ry }, br);
                // Translucent body + bright lower caustic crescent + white upper-left glint
                br.SetColor(&rgba(156, 206, 246, 0.24));
                rt.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: px, Y: py }, radiusX: rx, radiusY: ry }, br);
                br.SetColor(&rgba(214, 242, 255, 0.66));
                rt.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: px, Y: py + ry * 0.44 }, radiusX: rx * 0.56, radiusY: ry * 0.24 }, br);
                br.SetColor(&rgba(255, 255, 255, 0.86));
                rt.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: px - rx * 0.34, Y: py - ry * 0.32 }, radiusX: (rx * 0.26).max(0.6), radiusY: (ry * 0.24).max(0.6) }, br);
            }
        }
        DockArtLayer::OceanTideFoam => {
            // Slanted underwater sunlit ocean shafts
            for i in 0..5 {
                let sx = sr.left + w * (0.10 + i as f32 * 0.21);
                br.SetColor(&rgba(136, 238, 242, 0.07));
                rt.DrawLine(Vector2 { X: sx, Y: sr.top }, Vector2 { X: sx + 18.0 * u, Y: sr.bottom }, br, 10.0 * u, None);
            }
            // Rolling multi-layered ocean tide swell + lacy sea-foam crest bubbles
            let segs = 30;
            for wave_i in 0..2 {
                let wf = wave_i as f32;
                let base_y = sr.top + h * (0.28 + wf * 0.28);
                for s in 0..segs {
                    let f0 = s as f32 / segs as f32;
                    let f1 = (s + 1) as f32 / segs as f32;
                    let y0 = base_y + (f0 * 8.5 + t * (1.25 - wf * 0.35) + wf * 2.1).sin() * (3.6 * u);
                    let y1 = base_y + (f1 * 8.5 + t * (1.25 - wf * 0.35) + wf * 2.1).sin() * (3.6 * u);
                    br.SetColor(&rgba(220, 252, 250, 0.34 - wf * 0.12));
                    rt.DrawLine(Vector2 { X: sr.left + w * f0, Y: y0 }, Vector2 { X: sr.left + w * f1, Y: y1 }, br, (2.4 - wf * 0.6) * u, None);
                    if s % 3 == 0 {
                        br.SetColor(&rgba(244, 255, 255, 0.42 - wf * 0.14));
                        rt.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: sr.left + w * f0, Y: y0 + 1.5 * u }, radiusX: 2.2 * u, radiusY: 1.2 * u }, br);
                    }
                }
            }
        }
        DockArtLayer::ZenCalmMist => {
            // Ultra-calm drifting sage morning mist veils
            let breathe = 0.5 + 0.5 * (t * 0.48).sin();
            for m in 0..3 {
                let mf = m as f32;
                let cx = sr.left + w * (((0.20 + mf * 0.32 + t * 0.015 * (1.0 + mf * 0.3)) % 1.2) - 0.1);
                let cy = sr.top + h * (0.35 + mf * 0.20);
                br.SetColor(&rgba(218, 236, 228, 0.08 + 0.05 * breathe));
                rt.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: cx, Y: cy }, radiusX: (w * 0.24).max(36.0 * u), radiusY: h * 0.44 }, br);
            }
            // Delicate concentric Zen raked-sand calming curves along bottom edge
            br.SetColor(&rgba(206, 226, 218, 0.16));
            for ring in 0..3 {
                let rf = ring as f32;
                rt.DrawLine(
                    Vector2 { X: sr.left + 12.0 * u, Y: sr.bottom - (3.5 + rf * 3.0) * u },
                    Vector2 { X: sr.right - 12.0 * u, Y: sr.bottom - (3.5 + rf * 3.0) * u },
                    br,
                    0.75 * u,
                    None,
                );
            }
        }
        DockArtLayer::TranquilKoiPond => {
            // Expanding calm pond surface ripples
            for r_i in 0..3 {
                let p = (t * 0.34 + r_i as f32 * 0.33) % 1.0;
                let rcx = sr.left + w * (0.24 + r_i as f32 * 0.27);
                let rcy = sr.top + h * (0.38 + (r_i % 2) as f32 * 0.26);
                br.SetColor(&rgba(182, 236, 222, (1.0 - p) * 0.24));
                rt.DrawEllipse(&D2D1_ELLIPSE { point: Vector2 { X: rcx, Y: rcy }, radiusX: (6.0 + p * 22.0) * u, radiusY: (3.0 + p * 9.0) * u }, br, 0.9 * u, None);
            }
            // 2 graceful translucent koi silhouettes gliding serenely beneath surface
            if !is_vertical {
                for ki in 0..2 {
                    let kf = ki as f32;
                    let dir = if ki == 0 { 1.0 } else { -1.0 };
                    let prog = ((t * (0.045 + kf * 0.015) + kf * 0.52) % 1.24) - 0.12;
                    let kx = sr.left + w * if dir > 0.0 { prog } else { 1.0 - prog };
                    let ky = sr.top + h * (0.36 + kf * 0.30) + (t * 1.4 + kf * 2.0).sin() * 2.6 * u;
                    let wag = (t * 4.2 + kf * 1.7).sin() * 2.0 * u;
                    // Koi body + bright calico orange patch + swaying tail
                    br.SetColor(&rgba(248, 128, 62, 0.62));
                    rt.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: kx, Y: ky }, radiusX: 7.5 * u, radiusY: 3.1 * u }, br);
                    br.SetColor(&rgba(255, 242, 226, 0.56));
                    rt.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: kx + dir * 2.2 * u, Y: ky }, radiusX: 3.6 * u, radiusY: 2.2 * u }, br);
                    br.SetColor(&rgba(252, 166, 108, 0.48));
                    rt.DrawLine(
                        Vector2 { X: kx - dir * 6.5 * u, Y: ky },
                        Vector2 { X: kx - dir * 12.0 * u, Y: ky + wag },
                        br,
                        2.6 * u,
                        None,
                    );
                }
            }
            // Serene floating emerald lily pads at outer corners
            for &(lx_n, ly_n, lr) in &[(0.08f32, 0.72f32, 7.5f32), (0.91, 0.28, 6.8)] {
                let lx = sr.left + w * lx_n;
                let ly = sr.top + h * ly_n;
                br.SetColor(&rgba(6, 20, 18, 0.38));
                rt.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: lx + 1.0 * u, Y: ly + 1.2 * u }, radiusX: lr * u, radiusY: lr * 0.72 * u }, br);
                br.SetColor(&rgba(52, 134, 88, 0.68));
                rt.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: lx, Y: ly }, radiusX: lr * u, radiusY: lr * 0.72 * u }, br);
            }
        }
        DockArtLayer::ForestSunbeams => {
            // Diagonal volumetric golden sunlight god-rays through calm canopy
            for i in 0..4 {
                let fi = i as f32;
                let sway = (t * 0.35 + fi * 1.3).sin() * 5.0 * u;
                let rx = sr.left + w * (0.14 + fi * 0.24) + sway;
                let ray_stops = [
                    D2D1_GRADIENT_STOP { position: 0.0, color: rgba(252, 238, 162, 0.22) },
                    D2D1_GRADIENT_STOP { position: 0.65, color: rgba(220, 214, 122, 0.08) },
                    D2D1_GRADIENT_STOP { position: 1.0, color: rgba(120, 162, 86, 0.0) },
                ];
                if let Some(g) = create_grad(rt, Vector2 { X: rx, Y: sr.top }, Vector2 { X: rx + 26.0 * u, Y: sr.bottom }, &ray_stops) {
                    rt.FillRectangle(&D2D_RECT_F { left: rx - 14.0 * u, top: sr.top, right: rx + 30.0 * u, bottom: sr.bottom }, &g);
                }
            }
            // Gently floating golden sunlit forest pollen motes
            for i in 0..12 {
                let fi = i as f32;
                let mx_n = ((fi * 0.618 + (t * 0.025 + fi).sin() * 0.04) % 0.90) + 0.05;
                let my_n = ((fi * 0.382 - t * 0.04 + 10.0) % 0.84) + 0.08;
                let tw = 0.35 + 0.65 * ((t * 1.4 + fi * 1.9).sin() * 0.5 + 0.5);
                br.SetColor(&rgba(252, 240, 174, tw * 0.62));
                rt.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: sr.left + w * mx_n, Y: sr.top + h * my_n }, radiusX: 1.35 * u, radiusY: 1.35 * u }, br);
            }
        }
        DockArtLayer::CarraraMarble => {
            // Organic smoky-gray Italian Carrara marble mineral veining
            let segs = 24;
            for v in 0..3 {
                let vf = v as f32;
                br.SetColor(&rgba(128, 126, 122, 0.22 - vf * 0.04));
                for s in 0..segs {
                    let f0 = s as f32 / segs as f32;
                    let f1 = (s + 1) as f32 / segs as f32;
                    let y0 = sr.top + h * (0.18 + vf * 0.28) + (f0 * 5.2 + vf * 2.4).sin() * (h * 0.22) + (f0 * 14.0).cos() * (2.2 * u);
                    let y1 = sr.top + h * (0.18 + vf * 0.28) + (f1 * 5.2 + vf * 2.4).sin() * (h * 0.22) + (f1 * 14.0).cos() * (2.2 * u);
                    rt.DrawLine(Vector2 { X: sr.left + w * f0, Y: y0 }, Vector2 { X: sr.left + w * f1, Y: y1 }, br, (2.0 - vf * 0.4) * u, None);
                }
            }
            // Delicate kintsugi champagne-gold hairline vein
            br.SetColor(&rgba(196, 156, 74, 0.58));
            for s in 0..segs {
                let f0 = s as f32 / segs as f32;
                let f1 = (s + 1) as f32 / segs as f32;
                let y0 = sr.top + h * 0.54 + (f0 * 4.8 + 1.1).sin() * (h * 0.20) + (f0 * 11.0).cos() * (1.8 * u);
                let y1 = sr.top + h * 0.54 + (f1 * 4.8 + 1.1).sin() * (h * 0.20) + (f1 * 11.0).cos() * (1.8 * u);
                rt.DrawLine(Vector2 { X: sr.left + w * f0, Y: y0 }, Vector2 { X: sr.left + w * f1, Y: y1 }, br, 0.95 * u, None);
            }
            // Polished stone specular reflection sheen
            let p_stops = [
                D2D1_GRADIENT_STOP { position: 0.0, color: rgba(255, 255, 255, 0.42) },
                D2D1_GRADIENT_STOP { position: 0.42, color: rgba(255, 255, 255, 0.0) },
            ];
            if let Some(g) = create_grad(rt, Vector2 { X: sr.left, Y: sr.top }, Vector2 { X: sr.left, Y: sr.bottom }, &p_stops) {
                rt.FillRectangle(&sr, &g);
            }
        }
        DockArtLayer::OiledWalnutWood => {
            // Flowing hand-oiled American black walnut wood grain fibers
            let segs = 26;
            let mut ry = sr.top + 2.5 * u;
            let mut idx = 0usize;
            while ry < sr.bottom - 1.5 * u {
                let fi = idx as f32;
                if idx % 3 == 0 {
                    br.SetColor(&rgba(24, 12, 6, 0.34));
                } else {
                    br.SetColor(&rgba(148, 94, 52, 0.20));
                }
                for s in 0..segs {
                    let f0 = s as f32 / segs as f32;
                    let f1 = (s + 1) as f32 / segs as f32;
                    let y0 = ry + (f0 * 4.2 + fi * 0.45).sin() * 2.0 * u;
                    let y1 = ry + (f1 * 4.2 + fi * 0.45).sin() * 2.0 * u;
                    rt.DrawLine(Vector2 { X: sr.left + w * f0, Y: y0 }, Vector2 { X: sr.left + w * f1, Y: y1 }, br, 1.05 * u, None);
                }
                ry += 3.6 * u;
                idx += 1;
            }
            // Satin lacquer sheen + inlaid champagne-brass studio accent groove
            let oil_stops = [
                D2D1_GRADIENT_STOP { position: 0.0, color: rgba(255, 224, 178, 0.16) },
                D2D1_GRADIENT_STOP { position: 0.38, color: rgba(255, 224, 178, 0.0) },
            ];
            if let Some(g) = create_grad(rt, Vector2 { X: sr.left, Y: sr.top }, Vector2 { X: sr.left, Y: sr.bottom }, &oil_stops) {
                rt.FillRectangle(&sr, &g);
            }
            br.SetColor(&rgba(218, 174, 96, 0.56));
            rt.DrawLine(Vector2 { X: sr.left + 8.0 * u, Y: sr.top + 3.0 * u }, Vector2 { X: sr.right - 8.0 * u, Y: sr.top + 3.0 * u }, br, 0.9 * u, None);
        }
        DockArtLayer::GlacierCrystalIce => {
            // Translucent alpine glacial subsurface glow + crystalline internal fractures
            let ice_stops = [
                D2D1_GRADIENT_STOP { position: 0.0, color: rgba(232, 250, 255, 0.34) },
                D2D1_GRADIENT_STOP { position: 0.48, color: rgba(124, 210, 244, 0.12) },
                D2D1_GRADIENT_STOP { position: 1.0, color: rgba(34, 118, 168, 0.24) },
            ];
            if let Some(g) = create_grad(rt, Vector2 { X: sr.left, Y: sr.top }, Vector2 { X: sr.left, Y: sr.bottom }, &ice_stops) {
                rt.FillRectangle(&sr, &g);
            }
            // Angular crystal fracture hairlines
            br.SetColor(&rgba(224, 248, 255, 0.34));
            let cracks = [
                (0.14f32, 0.0f32, 0.22f32, 0.52f32, 0.19f32, 1.0f32),
                (0.48, 0.0, 0.43, 0.44, 0.52, 1.0),
                (0.78, 0.0, 0.84, 0.58, 0.76, 1.0),
            ];
            for &(x0, y0, x1, y1, x2, y2) in &cracks {
                rt.DrawLine(Vector2 { X: sr.left + w * x0, Y: sr.top + h * y0 }, Vector2 { X: sr.left + w * x1, Y: sr.top + h * y1 }, br, 0.85 * u, None);
                rt.DrawLine(Vector2 { X: sr.left + w * x1, Y: sr.top + h * y1 }, Vector2 { X: sr.left + w * x2, Y: sr.top + h * y2 }, br, 0.75 * u, None);
            }
            // Animated alpine frost sparkle glints
            for i in 0..8 {
                let fi = i as f32;
                let tw = (0.5 + 0.5 * (t * 2.2 + fi * 1.9).sin()).powi(2);
                let gx = sr.left + w * (0.09 + fi * 0.115);
                let gy = sr.top + h * (0.22 + ((i * 37) % 56) as f32 * 0.01);
                br.SetColor(&rgba(244, 254, 255, 0.22 + 0.62 * tw));
                rt.DrawLine(Vector2 { X: gx - 2.6 * u, Y: gy }, Vector2 { X: gx + 2.6 * u, Y: gy }, br, 0.8 * u, None);
                rt.DrawLine(Vector2 { X: gx, Y: gy - 2.6 * u }, Vector2 { X: gx, Y: gy + 2.6 * u }, br, 0.8 * u, None);
            }
        }
        DockArtLayer::WarmLinenDusk => {
            // Authentic woven natural oatmeal linen fabric warp & weft micro-weave
            br.SetColor(&rgba(148, 128, 108, 0.14));
            let mut y = sr.top + 2.5 * u;
            while y < sr.bottom - 1.5 * u {
                rt.DrawLine(Vector2 { X: sr.left, Y: y }, Vector2 { X: sr.right, Y: y }, br, 0.65 * u, None);
                y += 3.2 * u;
            }
            br.SetColor(&rgba(166, 146, 124, 0.11));
            let mut x = sr.left + 3.0 * u;
            while x < sr.right - 1.5 * u {
                rt.DrawLine(Vector2 { X: x, Y: sr.top }, Vector2 { X: x, Y: sr.bottom }, br, 0.65 * u, None);
                x += 4.2 * u;
            }
            // Warm dusk reading-lamp calming glow sweep
            let dusk_x = sr.left + w * (0.5 + 0.22 * (t * 0.35).sin());
            let d_stops = [
                D2D1_GRADIENT_STOP { position: 0.0, color: rgba(255, 226, 184, 0.0) },
                D2D1_GRADIENT_STOP { position: 0.5, color: rgba(255, 224, 182, 0.22) },
                D2D1_GRADIENT_STOP { position: 1.0, color: rgba(255, 226, 184, 0.0) },
            ];
            if let Some(g) = create_grad(rt, Vector2 { X: dusk_x - 60.0 * u, Y: sr.top }, Vector2 { X: dusk_x + 60.0 * u, Y: sr.bottom }, &d_stops) {
                rt.FillRectangle(&sr, &g);
            }
            // Tailored stitched linen edge hem
            br.SetColor(&rgba(154, 126, 100, 0.38));
            let mut hx = sr.left + 8.0 * u;
            while hx < sr.right - 8.0 * u {
                rt.DrawLine(Vector2 { X: hx, Y: sr.bottom - 3.6 * u }, Vector2 { X: hx + 3.5 * u, Y: sr.bottom - 3.6 * u }, br, 0.85 * u, None);
                hx += 6.5 * u;
            }
        }
        DockArtLayer::AbyssalTrench | DockArtLayer::GoldenHourHorizon => {
            // Rendered natively via realistic_pbr::draw_pbr_surface_clipped
        }
    }
}

#[allow(clippy::too_many_arguments)]
unsafe fn draw_shelf_rims(
    rt: &ID2D1DCRenderTarget,
    br: &ID2D1SolidColorBrush,
    sr: D2D_RECT_F,
    rad: f32,
    spec: &DockVisualSpec,
    cfg: &AppConfig,
    pal: &ThemePalette,
    cham_geo: Option<&ID2D1PathGeometry>,
    u: f32,
    is_preview: bool,
) {
    if cham_geo.is_none() && spec.layout != DockLayoutKind::FullEdgeTaskbar {
        let inner_rr = D2D1_ROUNDED_RECT {
            rect: D2D_RECT_F {
                left: sr.left + 1.0 * u,
                top: sr.top + 1.0 * u,
                right: sr.right - 1.0 * u,
                bottom: sr.bottom - 1.0 * u,
            },
            radiusX: (rad - 1.0 * u).max(1.0),
            radiusY: (rad - 1.0 * u).max(1.0),
        };
        br.SetColor(&rgba(spec.rim_inner.0, spec.rim_inner.1, spec.rim_inner.2, spec.rim_inner.3));
        rt.DrawRoundedRectangle(&inner_rr, br, 1.0 * u, None);
    }

    let (or, og, ob, oa) = if cfg.amber_border && cfg.tb_match_wallpaper {
        (pal.rim.0, pal.rim.1, pal.rim.2, 0.65)
    } else if !cfg.amber_border && !spec.light_mode_paper {
        (218, 230, 245, 0.25)
    } else {
        spec.rim_outer
    };
    br.SetColor(&rgba(or, og, ob, oa));
    if let Some(geo) = cham_geo {
        rt.DrawGeometry(geo, br, 1.35 * u, None);
        if spec.art == DockArtLayer::CyberHud {
            let bl = if is_preview { 6.5 * u } else { 13.0 * u };
            let cut = if is_preview { 6.0 * u } else { 11.0 * u };
            br.SetColor(&rgba(255, 62, 134, 0.92));
            rt.DrawLine(Vector2 { X: sr.left, Y: sr.top + cut + bl }, Vector2 { X: sr.left, Y: sr.top + cut }, br, 2.0 * u, None);
            rt.DrawLine(Vector2 { X: sr.left + cut, Y: sr.top }, Vector2 { X: sr.left + cut + bl, Y: sr.top }, br, 2.0 * u, None);
            rt.DrawLine(Vector2 { X: sr.right, Y: sr.bottom - cut - bl }, Vector2 { X: sr.right, Y: sr.bottom - cut }, br, 2.0 * u, None);
            rt.DrawLine(Vector2 { X: sr.right - cut, Y: sr.bottom }, Vector2 { X: sr.right - cut - bl, Y: sr.bottom }, br, 2.0 * u, None);
        }
    } else if spec.layout == DockLayoutKind::FullEdgeTaskbar {
        // Crisp horizon line along open interior edge (top line when docked at bottom, bottom when top, etc.)
        match cfg.mac_dock_edge {
            MacDockEdge::Bottom => {
                rt.DrawLine(Vector2 { X: sr.left, Y: sr.top + 0.8 * u }, Vector2 { X: sr.right, Y: sr.top + 0.8 * u }, br, 1.4 * u, None);
            }
            MacDockEdge::Top => {
                rt.DrawLine(Vector2 { X: sr.left, Y: sr.bottom - 0.8 * u }, Vector2 { X: sr.right, Y: sr.bottom - 0.8 * u }, br, 1.4 * u, None);
            }
            MacDockEdge::Left => {
                rt.DrawLine(Vector2 { X: sr.right - 0.8 * u, Y: sr.top }, Vector2 { X: sr.right - 0.8 * u, Y: sr.bottom }, br, 1.4 * u, None);
            }
            MacDockEdge::Right => {
                rt.DrawLine(Vector2 { X: sr.left + 0.8 * u, Y: sr.top }, Vector2 { X: sr.left + 0.8 * u, Y: sr.bottom }, br, 1.4 * u, None);
            }
        }
    } else {
        let outer_rr = D2D1_ROUNDED_RECT { rect: sr, radiusX: rad, radiusY: rad };
        rt.DrawRoundedRectangle(&outer_rr, br, if spec.light_mode_paper { 1.4 * u } else { 1.2 * u }, None);
    }
}

pub unsafe fn draw_dock_divider(
    rt: &ID2D1DCRenderTarget,
    br: &ID2D1SolidColorBrush,
    cx: f32,
    cy: f32,
    shelf_r: D2D_RECT_F,
    theme: TaskbarTheme,
    is_vertical: bool,
    u: f32,
) {
    let spec = dock_spec(theme);
    if spec.layout == DockLayoutKind::IndependentOrbs {
        return;
    }
    br.SetColor(&rgba(spec.divider.0, spec.divider.1, spec.divider.2, spec.divider.3));
    if is_vertical {
        let span = (shelf_r.right - shelf_r.left) * 0.54;
        rt.DrawLine(
            Vector2 { X: cx - span * 0.5, Y: cy },
            Vector2 { X: cx + span * 0.5, Y: cy },
            br,
            if spec.art == DockArtLayer::BauhausPrimary { 2.0 * u } else { 1.2 * u },
            None,
        );
    } else {
        let span = (shelf_r.bottom - shelf_r.top) * 0.54;
        rt.DrawLine(
            Vector2 { X: cx, Y: cy - span * 0.5 },
            Vector2 { X: cx, Y: cy + span * 0.5 },
            br,
            if spec.art == DockArtLayer::BauhausPrimary { 2.0 * u } else { 1.2 * u },
            None,
        );
    }
}

#[allow(clippy::too_many_arguments)]
pub unsafe fn draw_dock_slot_fx(
    rt: &ID2D1DCRenderTarget,
    d2d: Option<&ID2D1Factory>,
    br: &ID2D1SolidColorBrush,
    slot_rect: D2D_RECT_F,
    icon_cx: f32,
    icon_cy: f32,
    sz: f32,
    shelf_r: D2D_RECT_F,
    theme: TaskbarTheme,
    is_hovered: bool,
    is_active: bool,
    has_windows: bool,
    is_flashing: bool,
    is_vertical: bool,
    anim_phase: f32,
    idx: usize,
    u: f32,
) {
    let spec = dock_spec(theme);
    if spec.layout == DockLayoutKind::IndependentOrbs {
        let slot_w = (slot_rect.right - slot_rect.left).max(0.0);
        let slot_h = (slot_rect.bottom - slot_rect.top).max(0.0);
        if slot_w > sz * 1.22 || slot_h > sz * 1.22 {
            let pad_x = if slot_w <= 32.0 * u { 0.5 * u } else { 1.0 * u };
            let pad_y = if slot_h <= 32.0 * u { 1.5 * u } else { 3.0 * u };
            let pill_r = D2D_RECT_F {
                left: slot_rect.left + pad_x,
                top: slot_rect.top + pad_y,
                right: slot_rect.right - pad_x,
                bottom: slot_rect.bottom - pad_y,
            };
            let pill_h = (pill_r.bottom - pill_r.top).max(8.0 * u);
            let rr = D2D1_ROUNDED_RECT {
                rect: pill_r,
                radiusX: (pill_h * 0.48).min(15.0 * u),
                radiusY: (pill_h * 0.48).min(15.0 * u),
            };
            let bg_a = if theme == TaskbarTheme::InvisibleGhostDock {
                if is_hovered { 0.74 } else { 0.56 }
            } else if is_hovered {
                0.88
            } else {
                0.76
            };
            br.SetColor(&rgba(spec.bg_mid.0, spec.bg_mid.1, spec.bg_mid.2, bg_a));
            rt.FillRoundedRectangle(&rr, br);
            if theme == TaskbarTheme::InvisibleGhostDock {
                let (rr_c, rg_c, rb_c, ra_c) = if is_hovered {
                    (spec.dot_active.0, spec.dot_active.1, spec.dot_active.2, 0.54)
                } else {
                    (210, 228, 250, 0.20)
                };
                br.SetColor(&rgba(rr_c, rg_c, rb_c, ra_c));
                rt.DrawRoundedRectangle(&rr, br, 1.0 * u, None);
            } else {
                let rim_a = if is_hovered { 0.88 } else { 0.50 };
                br.SetColor(&rgba(
                    spec.rim_outer.0,
                    spec.rim_outer.1,
                    spec.rim_outer.2,
                    rim_a,
                ));
                rt.DrawRoundedRectangle(&rr, br, 1.15 * u, None);
            }
            return;
        }
        draw_standalone_orb_node(rt, d2d, br, icon_cx, icon_cy, sz, theme, is_hovered, anim_phase, idx, u);
        if is_flashing {
            let pulse = 0.40 + 0.60 * (anim_phase * 5.2).sin().abs();
            let node_r = sz * 0.62;
            br.SetColor(&rgba(255, 168, 44, 0.28 * pulse));
            rt.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: icon_cx, Y: icon_cy }, radiusX: node_r * 1.15, radiusY: node_r * 1.15 }, br);
            br.SetColor(&rgba(255, 196, 72, 0.92 * pulse));
            rt.DrawEllipse(&D2D1_ELLIPSE { point: Vector2 { X: icon_cx, Y: icon_cy }, radiusX: node_r, radiusY: node_r }, br, 1.8 * u, None);
        }
        return;
    }

    let flash_pulse = if is_flashing {
        0.42 + 0.58 * (anim_phase * 5.2).sin().abs()
    } else {
        0.0
    };

    // Slot-level frames / buttons per structural architecture!
    match spec.layout {
        DockLayoutKind::FullEdgeTaskbar => {
            let rad_b = if spec.art == DockArtLayer::AquaReflect { 6.0 * u } else { 3.0 * u };
            let btn_rr = D2D1_ROUNDED_RECT {
                rect: slot_rect,
                radiusX: rad_b,
                radiusY: rad_b,
            };
            if is_flashing {
                br.SetColor(&rgba(255, 152, 36, 0.42 * flash_pulse));
                rt.FillRoundedRectangle(&btn_rr, br);
                br.SetColor(&rgba(255, 196, 74, 0.92 * flash_pulse));
                rt.DrawRoundedRectangle(&btn_rr, br, 1.4 * u, None);
            } else if is_active || has_windows || is_hovered {
                let bg_a = if is_active { 0.34 } else if is_hovered { 0.24 } else { 0.14 };
                br.SetColor(&rgba(spec.dot_glow.0, spec.dot_glow.1, spec.dot_glow.2, bg_a));
                rt.FillRoundedRectangle(&btn_rr, br);
                if spec.art == DockArtLayer::AquaReflect {
                    // Upper Aero gloss sheen on taskbar tile
                    let top_r = D2D_RECT_F {
                        left: slot_rect.left + 1.0 * u,
                        top: slot_rect.top + 1.0 * u,
                        right: slot_rect.right - 1.0 * u,
                        bottom: (slot_rect.top + slot_rect.bottom) * 0.5,
                    };
                    br.SetColor(&rgba(240, 252, 255, if is_hovered { 0.30 } else { 0.16 }));
                    rt.FillRoundedRectangle(&D2D1_ROUNDED_RECT { rect: top_r, radiusX: 4.0 * u, radiusY: 4.0 * u }, br);
                }
                br.SetColor(&rgba(spec.rim_outer.0, spec.rim_outer.1, spec.rim_outer.2, if is_active { 0.72 } else { 0.36 }));
                rt.DrawRoundedRectangle(&btn_rr, br, 1.0 * u, None);
            }
            return;
        }
        DockLayoutKind::InlineMorphRail => {
            let card_rr = D2D1_ROUNDED_RECT {
                rect: slot_rect,
                radiusX: 3.0 * u,
                radiusY: 3.0 * u,
            };
            if spec.art == DockArtLayer::PaperHalftone {
                // Hard architectural stamp shadow when hovered or active
                if is_hovered || is_active {
                    let sh_rr = D2D1_ROUNDED_RECT {
                        rect: D2D_RECT_F {
                            left: slot_rect.left + 3.0 * u,
                            top: slot_rect.top + 3.0 * u,
                            right: slot_rect.right + 3.0 * u,
                            bottom: slot_rect.bottom + 3.0 * u,
                        },
                        radiusX: 3.0 * u,
                        radiusY: 3.0 * u,
                    };
                    br.SetColor(&rgba(24, 22, 18, 0.28));
                    rt.FillRoundedRectangle(&sh_rr, br);
                }
                br.SetColor(&rgba(255, 253, 248, if is_hovered { 0.96 } else { 0.56 }));
                rt.FillRoundedRectangle(&card_rr, br);
                if is_flashing {
                    br.SetColor(&rgba(255, 138, 28, 0.26 * flash_pulse));
                    rt.FillRoundedRectangle(&card_rr, br);
                }
                br.SetColor(&rgba(
                    if is_flashing { 216 } else { 34 },
                    if is_flashing { 82 } else { 32 },
                    28,
                    if is_flashing || is_hovered || is_active { 0.94 } else { 0.36 },
                ));
                rt.DrawRoundedRectangle(&card_rr, br, if is_hovered || is_flashing { 1.5 * u } else { 0.9 * u }, None);
            } else if spec.art == DockArtLayer::BauhausPrimary {
                br.SetColor(&rgba(50, 50, 58, if is_hovered { 0.92 } else { 0.45 }));
                rt.FillRoundedRectangle(&card_rr, br);
                if is_flashing {
                    br.SetColor(&rgba(255, 162, 36, 0.34 * flash_pulse));
                    rt.FillRoundedRectangle(&card_rr, br);
                }
                let col_bar = match idx % 3 {
                    0 => (62u8, 134u8, 242u8),
                    1 => (244, 196, 48),
                    _ => (228, 68, 48),
                };
                if is_hovered || has_windows {
                    br.SetColor(&rgba(col_bar.0, col_bar.1, col_bar.2, 0.92));
                    rt.FillRectangle(
                        &D2D_RECT_F {
                            left: slot_rect.left + 3.0 * u,
                            top: slot_rect.bottom - 3.5 * u,
                            right: slot_rect.right - 3.0 * u,
                            bottom: slot_rect.bottom,
                        },
                        br,
                    );
                }
            } else {
                // Kumiko shoji sliding plaque
                br.SetColor(&rgba(76, 46, 26, if is_hovered { 0.88 } else { 0.44 }));
                rt.FillRoundedRectangle(&card_rr, br);
                if is_flashing {
                    br.SetColor(&rgba(255, 158, 38, 0.32 * flash_pulse));
                    rt.FillRoundedRectangle(&card_rr, br);
                }
                br.SetColor(&rgba(232, 184, 106, if is_hovered || is_flashing { 0.92 } else { 0.30 }));
                rt.DrawRoundedRectangle(&card_rr, br, if is_flashing { 1.4 * u } else { 1.0 * u }, None);
            }
            return;
        }
        DockLayoutKind::HudConsoleDeck => {
            // Custom hardware/cockpit keycap socket per slot!
            match spec.art {
                DockArtLayer::CyberHud => {
                    if let Some(f) = d2d {
                        if let Some(geo) = chamfer_geo(f, slot_rect, 6.0 * u) {
                            if is_flashing {
                                br.SetColor(&rgba(255, 156, 36, 0.30 * flash_pulse));
                                rt.FillGeometry(&geo, br, None);
                                br.SetColor(&rgba(255, 194, 68, 0.95 * flash_pulse));
                                rt.DrawGeometry(&geo, br, 1.5 * u, None);
                            } else {
                                br.SetColor(&rgba(0, 240, 255, if is_hovered { 0.20 } else { 0.06 }));
                                rt.FillGeometry(&geo, br, None);
                                br.SetColor(&rgba(0, 240, 255, if is_hovered { 0.90 } else { 0.28 }));
                                rt.DrawGeometry(&geo, br, 1.0 * u, None);
                            }
                        }
                    }
                }
                DockArtLayer::AstrolabeTicks | DockArtLayer::VacuumTubeHiFi => {
                    // Brass circular porthole / tube bezel around every icon
                    let r_port = (sz * 0.56).max(12.0 * u);
                    br.SetColor(&rgba(8, 14, 32, 0.58));
                    rt.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: icon_cx, Y: icon_cy }, radiusX: r_port, radiusY: r_port }, br);
                    if is_flashing {
                        br.SetColor(&rgba(255, 168, 42, 0.30 * flash_pulse));
                        rt.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: icon_cx, Y: icon_cy }, radiusX: r_port, radiusY: r_port }, br);
                    } else if spec.art == DockArtLayer::VacuumTubeHiFi && is_hovered {
                        br.SetColor(&rgba(255, 164, 52, 0.24));
                        rt.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: icon_cx, Y: icon_cy }, radiusX: r_port * 1.06, radiusY: r_port * 1.06 }, br);
                    }
                    br.SetColor(&rgba(if is_flashing { 255 } else { 228 }, if is_flashing { 196 } else { 184 }, 92, if is_hovered || is_flashing { 0.94 } else { 0.48 }));
                    rt.DrawEllipse(&D2D1_ELLIPSE { point: Vector2 { X: icon_cx, Y: icon_cy }, radiusX: r_port, radiusY: r_port }, br, if is_flashing { 1.6 * u } else { 1.2 * u }, None);
                }
                DockArtLayer::CassetteDeck1984 => {
                    // Molded tactile ABS keycap bay with orange active rim
                    let k_rr = D2D1_ROUNDED_RECT { rect: slot_rect, radiusX: 4.0 * u, radiusY: 4.0 * u };
                    br.SetColor(&rgba(24, 22, 20, if is_hovered { 0.72 } else { 0.48 }));
                    rt.FillRoundedRectangle(&k_rr, br);
                    br.SetColor(&rgba(255, 148, 36, if is_hovered || is_flashing { 0.92 } else { 0.34 }));
                    rt.DrawRoundedRectangle(&k_rr, br, if is_hovered || is_flashing { 1.4 * u } else { 1.0 * u }, None);
                }
                DockArtLayer::PhosphorCrt => {
                    let l = slot_rect.left;
                    let t = slot_rect.top;
                    let r = slot_rect.right;
                    let b = slot_rect.bottom;
                    let k = 6.0 * u;
                    br.SetColor(&rgba(
                        if is_flashing { 255 } else { 76 },
                        if is_flashing { 196 } else { 255 },
                        if is_flashing { 64 } else { 132 },
                        if is_hovered || is_flashing { 0.96 } else { 0.34 },
                    ));
                    for &(x0, y0, dx, dy) in &[(l, t, k, k), (r, t, -k, k), (l, b, k, -k), (r, b, -k, -k)] {
                        rt.DrawLine(Vector2 { X: x0, Y: y0 }, Vector2 { X: x0 + dx, Y: y0 }, br, if is_hovered || is_flashing { 1.5 * u } else { 0.9 * u }, None);
                        rt.DrawLine(Vector2 { X: x0, Y: y0 }, Vector2 { X: x0, Y: y0 + dy }, br, if is_hovered || is_flashing { 1.5 * u } else { 0.9 * u }, None);
                    }
                }
                _ => {
                    let k_rr = D2D1_ROUNDED_RECT { rect: slot_rect, radiusX: 5.0 * u, radiusY: 5.0 * u };
                    if is_flashing {
                        br.SetColor(&rgba(255, 160, 42, 0.32 * flash_pulse));
                        rt.FillRoundedRectangle(&k_rr, br);
                        br.SetColor(&rgba(255, 198, 72, 0.94 * flash_pulse));
                        rt.DrawRoundedRectangle(&k_rr, br, 1.4 * u, None);
                    } else {
                        br.SetColor(&rgba(spec.dot_glow.0, spec.dot_glow.1, spec.dot_glow.2, if is_hovered { 0.22 } else { 0.08 }));
                        rt.FillRoundedRectangle(&k_rr, br);
                        br.SetColor(&rgba(spec.rim_outer.0, spec.rim_outer.1, spec.rim_outer.2, if is_hovered { 0.85 } else { 0.30 }));
                        rt.DrawRoundedRectangle(&k_rr, br, 1.0 * u, None);
                    }
                }
            }
            return;
        }
        _ => {}
    }

    // Contact shadow + highlight + attention pulse for FishEyeShelf / MultiIslandBar
    br.SetColor(&rgba(0, 0, 0, 0.28));
    rt.FillEllipse(
        &D2D1_ELLIPSE {
            point: Vector2 { X: icon_cx, Y: icon_cy + sz * 0.45 },
            radiusX: sz * 0.38,
            radiusY: 2.8 * u,
        },
        br,
    );
    let _ = is_vertical;
    let _ = shelf_r;
    if is_flashing {
        let f_rr = D2D1_ROUNDED_RECT {
            rect: D2D_RECT_F {
                left: slot_rect.left - 2.0 * u,
                top: slot_rect.top - 2.0 * u,
                right: slot_rect.right + 2.0 * u,
                bottom: slot_rect.bottom + 2.0 * u,
            },
            radiusX: sz * 0.26,
            radiusY: sz * 0.26,
        };
        br.SetColor(&rgba(255, 156, 36, 0.32 * flash_pulse));
        rt.FillRoundedRectangle(&f_rr, br);
        br.SetColor(&rgba(255, 196, 68, 0.90 * flash_pulse));
        rt.DrawRoundedRectangle(&f_rr, br, 1.4 * u, None);
    } else if is_hovered {
        let h_rr = D2D1_ROUNDED_RECT {
            rect: D2D_RECT_F {
                left: slot_rect.left - 2.0 * u,
                top: slot_rect.top - 2.0 * u,
                right: slot_rect.right + 2.0 * u,
                bottom: slot_rect.bottom + 2.0 * u,
            },
            radiusX: sz * 0.26,
            radiusY: sz * 0.26,
        };
        br.SetColor(&rgba(spec.dot_glow.0, spec.dot_glow.1, spec.dot_glow.2, 0.16));
        rt.FillRoundedRectangle(&h_rr, br);
    }
}

/// Computes indicator anchor strictly inside `shelf_r` for barless `IndependentOrbs` docks
/// so running diamonds/dots never protrude outside the layered canvas bounds.
pub fn independent_orb_indicator_anchor(
    cx: f32,
    cy: f32,
    slot_rect: D2D_RECT_F,
    shelf_r: D2D_RECT_F,
    edge: MacDockEdge,
    u: f32,
) -> (f32, f32) {
    let pad = 3.5 * u;
    match edge {
        MacDockEdge::Bottom => {
            let raw = slot_rect.bottom + 0.5 * u;
            let bound = (shelf_r.bottom - pad).max(shelf_r.top + pad);
            (cx, raw.min(bound))
        }
        MacDockEdge::Top => {
            let raw = slot_rect.top - 0.5 * u;
            let bound = (shelf_r.top + pad).min(shelf_r.bottom - pad);
            (cx, raw.max(bound))
        }
        MacDockEdge::Left => {
            let raw = slot_rect.left - 0.5 * u;
            let bound = (shelf_r.left + pad).min(shelf_r.right - pad);
            (raw.max(bound), cy)
        }
        MacDockEdge::Right => {
            let raw = slot_rect.right + 0.5 * u;
            let bound = (shelf_r.right - pad).max(shelf_r.left + pad);
            (raw.min(bound), cy)
        }
    }
}

#[allow(clippy::too_many_arguments)]
unsafe fn draw_indicator_dots_or_diamonds(
    rt: &ID2D1DCRenderTarget,
    br: &ID2D1SolidColorBrush,
    spec: &DockVisualSpec,
    slot_idx: usize,
    bx: f32,
    by: f32,
    sep: f32,
    count: usize,
    is_active: bool,
    is_vertical: bool,
    u: f32,
) {
    for d_i in 0..count {
        let off = (d_i as f32 - (count - 1) as f32 * 0.5) * sep;
        let pt = if is_vertical { Vector2 { X: bx, Y: by + off } } else { Vector2 { X: bx + off, Y: by } };
        let (r, g, b) = if is_active { spec.dot_active } else { spec.dot_idle };
        if is_active && !spec.light_mode_paper {
            br.SetColor(&rgba(spec.dot_glow.0, spec.dot_glow.1, spec.dot_glow.2, 0.44));
            rt.FillEllipse(&D2D1_ELLIPSE { point: pt, radiusX: 4.2 * u, radiusY: 4.2 * u }, br);
        }
        br.SetColor(&rgba(r, g, b, if is_active { 0.98 } else { 0.80 }));

        let effective = if spec.indicator == DockIndicatorStyle::GeometricMixed {
            match slot_idx % 3 {
                0 => DockIndicatorStyle::CircleDot,
                1 => DockIndicatorStyle::SquareStamp,
                _ => DockIndicatorStyle::Diamond,
            }
        } else {
            spec.indicator
        };

        match effective {
            DockIndicatorStyle::PillBar => {
                let (rx, ry) = if is_vertical { (1.4 * u, 3.2 * u) } else { (3.2 * u, 1.4 * u) };
                rt.FillRoundedRectangle(
                    &D2D1_ROUNDED_RECT {
                        rect: D2D_RECT_F { left: pt.X - rx, top: pt.Y - ry, right: pt.X + rx, bottom: pt.Y + ry },
                        radiusX: 1.2 * u,
                        radiusY: 1.2 * u,
                    },
                    br,
                );
            }
            DockIndicatorStyle::SquareStamp | DockIndicatorStyle::PhosphorCaret => {
                let (rx, ry) = if effective == DockIndicatorStyle::PhosphorCaret {
                    if is_vertical { (1.4 * u, 2.8 * u) } else { (2.8 * u, 1.4 * u) }
                } else {
                    let s = if is_active { 2.3 * u } else { 1.8 * u };
                    (s, s)
                };
                rt.FillRectangle(&D2D_RECT_F { left: pt.X - rx, top: pt.Y - ry, right: pt.X + rx, bottom: pt.Y + ry }, br);
            }
            DockIndicatorStyle::Diamond => {
                let s = if is_active { 2.4 * u } else { 1.9 * u };
                let rot = Matrix3x2::rotation_around(45.0, pt);
                rt.SetTransform(&rot);
                rt.FillRoundedRectangle(
                    &D2D1_ROUNDED_RECT {
                        rect: D2D_RECT_F { left: pt.X - s, top: pt.Y - s, right: pt.X + s, bottom: pt.Y + s },
                        radiusX: 0.45 * u,
                        radiusY: 0.45 * u,
                    },
                    br,
                );
                rt.SetTransform(&Matrix3x2::identity());
            }
            DockIndicatorStyle::Star4 => {
                let s = if is_active { 3.0 * u } else { 2.3 * u };
                let core = s * 0.48;
                let rot = Matrix3x2::rotation_around(45.0, pt);
                rt.SetTransform(&rot);
                rt.FillRoundedRectangle(
                    &D2D1_ROUNDED_RECT {
                        rect: D2D_RECT_F { left: pt.X - core, top: pt.Y - core, right: pt.X + core, bottom: pt.Y + core },
                        radiusX: 0.35 * u,
                        radiusY: 0.35 * u,
                    },
                    br,
                );
                rt.SetTransform(&Matrix3x2::identity());
                rt.DrawLine(Vector2 { X: pt.X - s, Y: pt.Y }, Vector2 { X: pt.X + s, Y: pt.Y }, br, 1.15 * u, None);
                rt.DrawLine(Vector2 { X: pt.X, Y: pt.Y - s }, Vector2 { X: pt.X, Y: pt.Y + s }, br, 1.15 * u, None);
            }
            DockIndicatorStyle::CircleDot
            | DockIndicatorStyle::AquaOrb
            | DockIndicatorStyle::EmberSpark
            | DockIndicatorStyle::GeometricMixed => {
                let rad_d = if is_active { 2.3 * u } else { 1.8 * u };
                rt.FillEllipse(&D2D1_ELLIPSE { point: pt, radiusX: rad_d, radiusY: rad_d }, br);
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub unsafe fn draw_dock_indicator(
    rt: &ID2D1DCRenderTarget,
    br: &ID2D1SolidColorBrush,
    theme: TaskbarTheme,
    slot_idx: usize,
    cx: f32,
    cy: f32,
    slot_rect: D2D_RECT_F,
    shelf_r: D2D_RECT_F,
    edge: MacDockEdge,
    dot_count: usize,
    is_active: bool,
    u: f32,
) {
    if dot_count == 0 {
        return;
    }
    let spec = dock_spec(theme);
    let is_vertical = matches!(edge, MacDockEdge::Left | MacDockEdge::Right);

    // Wide FullEdge & InlineMorph tiles render clean edge-bar underline instead of small dots
    if matches!(spec.layout, DockLayoutKind::FullEdgeTaskbar | DockLayoutKind::InlineMorphRail) {
        let bar_w = if is_active {
            ((slot_rect.right - slot_rect.left) * 0.56).max(16.0 * u)
        } else {
            (10.0 * u).min((slot_rect.right - slot_rect.left) * 0.42)
        };
        let (r, g, b) = if is_active { spec.dot_active } else { spec.dot_idle };
        br.SetColor(&rgba(r, g, b, if is_active { 0.96 } else { 0.72 }));
        if !is_vertical {
            let y_bar = if edge == MacDockEdge::Top { slot_rect.top + 1.0 * u } else { slot_rect.bottom - 3.2 * u };
            rt.FillRoundedRectangle(
                &D2D1_ROUNDED_RECT {
                    rect: D2D_RECT_F {
                        left: (slot_rect.left + slot_rect.right) * 0.5 - bar_w * 0.5,
                        top: y_bar,
                        right: (slot_rect.left + slot_rect.right) * 0.5 + bar_w * 0.5,
                        bottom: y_bar + 2.4 * u,
                    },
                    radiusX: 1.2 * u,
                    radiusY: 1.2 * u,
                },
                br,
            );
        } else {
            let x_bar = if edge == MacDockEdge::Left { slot_rect.left + 1.2 * u } else { slot_rect.right - 3.6 * u };
            rt.FillRoundedRectangle(
                &D2D1_ROUNDED_RECT {
                    rect: D2D_RECT_F {
                        left: x_bar,
                        top: cy - 6.0 * u,
                        right: x_bar + 2.4 * u,
                        bottom: cy + 6.0 * u,
                    },
                    radiusX: 1.2 * u,
                    radiusY: 1.2 * u,
                },
                br,
            );
        }
        return;
    }

    let count = dot_count.min(3);
    let sep = if spec.indicator == DockIndicatorStyle::PillBar { 7.4 * u } else { 6.2 * u };
    let (bx, by) = if spec.layout == DockLayoutKind::IndependentOrbs {
        independent_orb_indicator_anchor(cx, cy, slot_rect, shelf_r, edge, u)
    } else {
        match edge {
            MacDockEdge::Bottom => (cx, shelf_r.bottom - 5.2 * u),
            MacDockEdge::Top => (cx, shelf_r.top + 5.2 * u),
            MacDockEdge::Left => (shelf_r.left + 5.2 * u, cy),
            MacDockEdge::Right => (shelf_r.right - 5.2 * u, cy),
        }
    };

    draw_indicator_dots_or_diamonds(rt, br, &spec, slot_idx, bx, by, sep, count, is_active, is_vertical, u);
}

#[allow(clippy::too_many_arguments)]
pub unsafe fn draw_dock_divider_styled(
    rt: &ID2D1DCRenderTarget,
    br: &ID2D1SolidColorBrush,
    cx: f32,
    cy: f32,
    shelf_r: D2D_RECT_F,
    cfg: &AppConfig,
    is_vertical: bool,
    u: f32,
) {
    let (_, spec) = resolve_widget_spec(cfg, TaskbarWidgetId::AppDock);
    let div_style = cfg.w_apps.divider_style;
    if div_style == DockDividerStyle::BlankGap {
        return;
    }
    if div_style == DockDividerStyle::InheritTheme && spec.layout == DockLayoutKind::IndependentOrbs {
        return;
    }
    br.SetColor(&rgba(spec.divider.0, spec.divider.1, spec.divider.2, spec.divider.3.max(0.42)));
    let span = if is_vertical {
        (shelf_r.right - shelf_r.left) * 0.54
    } else {
        (shelf_r.bottom - shelf_r.top) * 0.54
    };
    match div_style {
        DockDividerStyle::GlowingDiamond => {
            br.SetColor(&rgba(spec.dot_glow.0, spec.dot_glow.1, spec.dot_glow.2, 0.28));
            rt.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: cx, Y: cy }, radiusX: 5.5 * u, radiusY: 5.5 * u }, br);
            br.SetColor(&rgba(spec.dot_active.0, spec.dot_active.1, spec.dot_active.2, 0.92));
            let s = 3.2 * u;
            rt.DrawLine(Vector2 { X: cx - s, Y: cy }, Vector2 { X: cx + s, Y: cy }, br, 1.5 * u, None);
            rt.DrawLine(Vector2 { X: cx, Y: cy - s }, Vector2 { X: cx, Y: cy + s }, br, 1.5 * u, None);
        }
        DockDividerStyle::CyberNotch => {
            let half = span * 0.46;
            let k = 2.6 * u;
            br.SetColor(&rgba(spec.dot_active.0, spec.dot_active.1, spec.dot_active.2, 0.78));
            if is_vertical {
                rt.DrawLine(Vector2 { X: cx - half, Y: cy }, Vector2 { X: cx + half, Y: cy }, br, 1.3 * u, None);
                rt.DrawLine(Vector2 { X: cx - half, Y: cy - k }, Vector2 { X: cx - half, Y: cy + k }, br, 1.3 * u, None);
                rt.DrawLine(Vector2 { X: cx + half, Y: cy - k }, Vector2 { X: cx + half, Y: cy + k }, br, 1.3 * u, None);
            } else {
                rt.DrawLine(Vector2 { X: cx, Y: cy - half }, Vector2 { X: cx, Y: cy + half }, br, 1.3 * u, None);
                rt.DrawLine(Vector2 { X: cx - k, Y: cy - half }, Vector2 { X: cx + k, Y: cy - half }, br, 1.3 * u, None);
                rt.DrawLine(Vector2 { X: cx - k, Y: cy + half }, Vector2 { X: cx + k, Y: cy + half }, br, 1.3 * u, None);
            }
        }
        _ => {
            let w_line = if spec.art == DockArtLayer::BauhausPrimary { 2.0 * u } else { 1.2 * u };
            if is_vertical {
                rt.DrawLine(Vector2 { X: cx - span * 0.5, Y: cy }, Vector2 { X: cx + span * 0.5, Y: cy }, br, w_line, None);
            } else {
                rt.DrawLine(Vector2 { X: cx, Y: cy - span * 0.5 }, Vector2 { X: cx, Y: cy + span * 0.5 }, br, w_line, None);
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub unsafe fn draw_dock_slot_fx_styled(
    rt: &ID2D1DCRenderTarget,
    d2d: Option<&ID2D1Factory>,
    br: &ID2D1SolidColorBrush,
    slot_rect: D2D_RECT_F,
    icon_cx: f32,
    icon_cy: f32,
    sz: f32,
    shelf_r: D2D_RECT_F,
    cfg: &AppConfig,
    widget_id: TaskbarWidgetId,
    brand_rgb: Option<(u8, u8, u8)>,
    is_hovered: bool,
    is_active: bool,
    has_windows: bool,
    is_flashing: bool,
    is_vertical: bool,
    anim_phase: f32,
    idx: usize,
    u: f32,
) {
    let (eff_theme, mut spec) = resolve_widget_spec(cfg, widget_id);
    if let Some(bc) = brand_rgb {
        spec.dot_glow = bc;
        spec.dot_active = bc;
    }

    let card_style = if widget_id == TaskbarWidgetId::AppDock {
        cfg.w_apps.card_style
    } else {
        DockSlotCardStyle::InheritTheme
    };

    // Per-App Brand Glow aura only on carded slots; MinimalNone keeps icons truly clean at rest
    if card_style != DockSlotCardStyle::MinimalNone {
        if let Some((br_r, br_g, br_b)) = brand_rgb {
            if is_active || is_hovered || has_windows {
                let aura_a = if is_hovered { 0.28 } else if is_active { 0.20 } else { 0.11 };
                br.SetColor(&rgba(br_r, br_g, br_b, aura_a * 0.45));
                rt.FillEllipse(
                    &D2D1_ELLIPSE {
                        point: Vector2 { X: icon_cx, Y: icon_cy },
                        radiusX: sz * 0.66,
                        radiusY: sz * 0.66,
                    },
                    br,
                );
                br.SetColor(&rgba(br_r, br_g, br_b, aura_a * 0.68));
                rt.FillEllipse(
                    &D2D1_ELLIPSE {
                        point: Vector2 { X: icon_cx, Y: icon_cy },
                        radiusX: sz * 0.48,
                        radiusY: sz * 0.48,
                    },
                    br,
                );
            }
        }
    }

    match card_style {
        DockSlotCardStyle::MinimalNone => {
            if is_hovered {
                let glow = brand_rgb.unwrap_or(spec.dot_glow);
                br.SetColor(&rgba(glow.0, glow.1, glow.2, 0.09));
                rt.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: icon_cx, Y: icon_cy }, radiusX: sz * 0.62, radiusY: sz * 0.62 }, br);
                br.SetColor(&rgba(glow.0, glow.1, glow.2, 0.16));
                rt.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: icon_cx, Y: icon_cy }, radiusX: sz * 0.46, radiusY: sz * 0.46 }, br);
            }
            if is_flashing {
                let pulse = 0.40 + 0.60 * (anim_phase * 5.2).sin().abs();
                let node_r = sz * 0.56;
                br.SetColor(&rgba(255, 196, 72, 0.85 * pulse));
                rt.DrawEllipse(&D2D1_ELLIPSE { point: Vector2 { X: icon_cx, Y: icon_cy }, radiusX: node_r, radiusY: node_r }, br, 1.5 * u, None);
            }
            return;
        }
        DockSlotCardStyle::GlassPill => {
            let rr = D2D1_ROUNDED_RECT {
                rect: slot_rect,
                radiusX: 11.0 * u,
                radiusY: 11.0 * u,
            };
            br.SetColor(&rgba(spec.bg_top.0, spec.bg_top.1, spec.bg_top.2, if is_hovered { 0.72 } else { 0.42 }));
            rt.FillRoundedRectangle(&rr, br);
            let rim = brand_rgb.unwrap_or((spec.rim_outer.0, spec.rim_outer.1, spec.rim_outer.2));
            br.SetColor(&rgba(rim.0, rim.1, rim.2, if is_hovered || is_active { 0.88 } else { 0.36 }));
            rt.DrawRoundedRectangle(&rr, br, 1.15 * u, None);
            return;
        }
        DockSlotCardStyle::ChamferOctagon => {
            if let Some(f) = d2d {
                if let Some(geo) = chamfer_geo(f, slot_rect, 6.0 * u) {
                    let glow = brand_rgb.unwrap_or(spec.dot_glow);
                    br.SetColor(&rgba(glow.0, glow.1, glow.2, if is_hovered { 0.24 } else { 0.08 }));
                    rt.FillGeometry(&geo, br, None);
                    br.SetColor(&rgba(glow.0, glow.1, glow.2, if is_hovered || is_active { 0.92 } else { 0.34 }));
                    rt.DrawGeometry(&geo, br, 1.15 * u, None);
                }
            }
            return;
        }
        DockSlotCardStyle::FloatingOrb => {
            draw_standalone_orb_node(rt, d2d, br, icon_cx, icon_cy, sz, eff_theme, is_hovered, anim_phase, idx, u);
            return;
        }
        DockSlotCardStyle::FlatTile => {
            let rr = D2D1_ROUNDED_RECT { rect: slot_rect, radiusX: 3.0 * u, radiusY: 3.0 * u };
            let glow = brand_rgb.unwrap_or(spec.dot_glow);
            br.SetColor(&rgba(glow.0, glow.1, glow.2, if is_hovered { 0.28 } else if is_active { 0.18 } else { 0.08 }));
            rt.FillRoundedRectangle(&rr, br);
            br.SetColor(&rgba(spec.rim_outer.0, spec.rim_outer.1, spec.rim_outer.2, if is_hovered || is_active { 0.78 } else { 0.32 }));
            rt.DrawRoundedRectangle(&rr, br, 1.0 * u, None);
            return;
        }
        DockSlotCardStyle::InheritTheme => {}
    }

    draw_dock_slot_fx(
        rt,
        d2d,
        br,
        slot_rect,
        icon_cx,
        icon_cy,
        sz,
        shelf_r,
        eff_theme,
        is_hovered,
        is_active,
        has_windows,
        is_flashing,
        is_vertical,
        anim_phase,
        idx,
        u,
    );
}

#[allow(clippy::too_many_arguments)]
pub unsafe fn draw_dock_indicator_styled(
    rt: &ID2D1DCRenderTarget,
    br: &ID2D1SolidColorBrush,
    cfg: &AppConfig,
    brand_rgb: Option<(u8, u8, u8)>,
    slot_idx: usize,
    cx: f32,
    cy: f32,
    slot_rect: D2D_RECT_F,
    shelf_r: D2D_RECT_F,
    edge: MacDockEdge,
    dot_count: usize,
    is_active: bool,
    u: f32,
) {
    if dot_count == 0 {
        return;
    }
    let (_, mut spec) = resolve_widget_spec(cfg, TaskbarWidgetId::AppDock);
    if let Some(bc) = brand_rgb {
        spec.dot_active = bc;
        spec.dot_glow = bc;
    }
    let is_vertical = matches!(edge, MacDockEdge::Left | MacDockEdge::Right);

    // Wide FullEdge & InlineMorph tiles render clean edge-bar underline unless user explicitly overrode indicator
    if cfg.w_apps.indicator_override.is_none()
        && matches!(spec.layout, DockLayoutKind::FullEdgeTaskbar | DockLayoutKind::InlineMorphRail)
    {
        let bar_w = if is_active {
            ((slot_rect.right - slot_rect.left) * 0.56).max(16.0 * u)
        } else {
            (10.0 * u).min((slot_rect.right - slot_rect.left) * 0.42)
        };
        let (r, g, b) = if is_active { spec.dot_active } else { spec.dot_idle };
        br.SetColor(&rgba(r, g, b, if is_active { 0.96 } else { 0.72 }));
        if !is_vertical {
            let y_bar = if edge == MacDockEdge::Top { slot_rect.top + 1.0 * u } else { slot_rect.bottom - 3.2 * u };
            rt.FillRoundedRectangle(
                &D2D1_ROUNDED_RECT {
                    rect: D2D_RECT_F {
                        left: (slot_rect.left + slot_rect.right) * 0.5 - bar_w * 0.5,
                        top: y_bar,
                        right: (slot_rect.left + slot_rect.right) * 0.5 + bar_w * 0.5,
                        bottom: y_bar + 2.4 * u,
                    },
                    radiusX: 1.2 * u,
                    radiusY: 1.2 * u,
                },
                br,
            );
        } else {
            let x_bar = if edge == MacDockEdge::Left { slot_rect.left + 1.2 * u } else { slot_rect.right - 3.6 * u };
            rt.FillRoundedRectangle(
                &D2D1_ROUNDED_RECT {
                    rect: D2D_RECT_F {
                        left: x_bar,
                        top: cy - 6.0 * u,
                        right: x_bar + 2.4 * u,
                        bottom: cy + 6.0 * u,
                    },
                    radiusX: 1.2 * u,
                    radiusY: 1.2 * u,
                },
                br,
            );
        }
        return;
    }

    let count = dot_count.min(3);
    let sep = if spec.indicator == DockIndicatorStyle::PillBar { 7.4 * u } else { 6.2 * u };
    let (bx, by) = if spec.layout == DockLayoutKind::IndependentOrbs {
        independent_orb_indicator_anchor(cx, cy, slot_rect, shelf_r, edge, u)
    } else {
        match edge {
            MacDockEdge::Bottom => (cx, shelf_r.bottom - 5.2 * u),
            MacDockEdge::Top => (cx, shelf_r.top + 5.2 * u),
            MacDockEdge::Left => (shelf_r.left + 5.2 * u, cy),
            MacDockEdge::Right => (shelf_r.right - 5.2 * u, cy),
        }
    };

    draw_indicator_dots_or_diamonds(rt, br, &spec, slot_idx, bx, by, sep, count, is_active, is_vertical, u);

    // Optional Multi-Window Numeric Badge on upper-right of slot
    if cfg.w_apps.show_window_count_badge && dot_count > 1 {
        let bx_b = (slot_rect.right - 7.5 * u).max(cx + 8.0 * u);
        let by_b = (slot_rect.top + 7.5 * u).min(cy - 8.0 * u);
        br.SetColor(&rgba(spec.bg_bot.0, spec.bg_bot.1, spec.bg_bot.2, 0.94));
        rt.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: bx_b, Y: by_b }, radiusX: 6.2 * u, radiusY: 6.2 * u }, br);
        br.SetColor(&rgba(spec.dot_active.0, spec.dot_active.1, spec.dot_active.2, 0.95));
        rt.DrawEllipse(&D2D1_ELLIPSE { point: Vector2 { X: bx_b, Y: by_b }, radiusX: 6.2 * u, radiusY: 6.2 * u }, br, 1.2 * u, None);
        // Mini pip marks inside badge for 2..=3+ windows
        let pips = dot_count.min(3);
        for p_i in 0..pips {
            let px = bx_b + (p_i as f32 - (pips - 1) as f32 * 0.5) * 3.0 * u;
            rt.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: px, Y: by_b }, radiusX: 1.1 * u, radiusY: 1.1 * u }, br);
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub unsafe fn draw_dock_clock_module(
    rt: &ID2D1DCRenderTarget,
    br: &ID2D1SolidColorBrush,
    tfm: Option<&IDWriteTextFormat>,
    slot_rect: D2D_RECT_F,
    cfg: &AppConfig,
    pal: &ThemePalette,
    time_str: &str,
    date_str: &str,
    hour24: u32,
    minute: u32,
    second: u32,
    anim_phase: f32,
    is_vertical: bool,
    u: f32,
) {
    let Some(tf) = tfm else {
        return;
    };
    let (eff_theme, spec) = resolve_widget_spec(cfg, TaskbarWidgetId::Clock);
    let mut txt_pri = cfg.w_clock.custom_text_rgb.map(unpack_rgb).unwrap_or(pal.text_primary);
    let mut txt_acc = cfg.w_clock.custom_accent_rgb.map(unpack_rgb).unwrap_or(spec.dot_active);
    let txt_mut = pal.text_muted;

    let engine = match cfg.w_clock.engine {
        DockClockEngine::InheritTheme => match eff_theme {
            TaskbarTheme::TerminalPhosphorDock => DockClockEngine::TerminalMatrix,
            TaskbarTheme::SolsticeAstrolabeDock => DockClockEngine::AnalogChronograph,
            _ => DockClockEngine::StackedDigital,
        },
        other => other,
    };

    // Guard legibility on bright/light-surface docks (EditorialPaper, CarraraMarble, SilkLinen):
    // if a bright white/cream custom_text_rgb override is applied over a light dock surface,
    // fall back to the dock palette's dark ink text/accent unless the clock draws its own dark tube/CRT backing.
    let dock_bg_lum = (pal.glass_top.0 as u32 * 299
        + pal.glass_top.1 as u32 * 587
        + pal.glass_top.2 as u32 * 114)
        / 1000;
    let is_self_dark_clock = matches!(
        engine,
        DockClockEngine::NixieTubeGlow | DockClockEngine::TerminalMatrix
    );
    if dock_bg_lum > 155 && !is_self_dark_clock {
        let pri_lum = (txt_pri.0 as u32 * 299 + txt_pri.1 as u32 * 587 + txt_pri.2 as u32 * 114) / 1000;
        if pri_lum > 165 {
            txt_pri = pal.text_primary;
        }
        let acc_lum = (txt_acc.0 as u32 * 299 + txt_acc.1 as u32 * 587 + txt_acc.2 as u32 * 114) / 1000;
        if acc_lum > 185 {
            txt_acc = spec.dot_active;
        }
    }

    let h = (slot_rect.bottom - slot_rect.top).max(12.0 * u);
    let w = (slot_rect.right - slot_rect.left).max(12.0 * u);

    if is_vertical {
        br.SetColor(&rgba(txt_pri.0, txt_pri.1, txt_pri.2, 0.94));
        let short_t = time_str.split(' ').next().unwrap_or(time_str);
        draw_d2d_text(rt, short_t, tf, &slot_rect, br);
        return;
    }

    match engine {
        DockClockEngine::NixieTubeGlow => {
            // Warm cathode tube capsule with glowing amber/accent filament aura
            let tube_r = D2D_RECT_F {
                left: slot_rect.left + 2.0 * u,
                top: slot_rect.top + 3.0 * u,
                right: slot_rect.right - 2.0 * u,
                bottom: slot_rect.bottom - 3.0 * u,
            };
            let rr = D2D1_ROUNDED_RECT { rect: tube_r, radiusX: 6.0 * u, radiusY: 6.0 * u };
            br.SetColor(&rgba(18, 10, 6, 0.88));
            rt.FillRoundedRectangle(&rr, br);
            let nix_col = cfg.w_clock.custom_accent_rgb.map(unpack_rgb).unwrap_or((255, 142, 44));
            let flicker = 0.88 + 0.12 * (anim_phase * 4.5).sin().abs();
            // Cathode mesh lines
            br.SetColor(&rgba(nix_col.0, nix_col.1, nix_col.2, 0.14));
            let mut gx = tube_r.left + 5.0 * u;
            while gx < tube_r.right - 4.0 * u {
                rt.DrawLine(Vector2 { X: gx, Y: tube_r.top + 2.0 * u }, Vector2 { X: gx, Y: tube_r.bottom - 2.0 * u }, br, 0.7 * u, None);
                gx += 5.0 * u;
            }
            br.SetColor(&rgba(nix_col.0, nix_col.1, nix_col.2, 0.78 * flicker));
            rt.DrawRoundedRectangle(&rr, br, 1.15 * u, None);

            let top_r = D2D_RECT_F {
                left: tube_r.left + 2.0 * u,
                top: tube_r.top + 1.0 * u,
                right: tube_r.right - 2.0 * u,
                bottom: if cfg.w_clock.show_date { tube_r.top + (tube_r.bottom - tube_r.top) * 0.58 } else { tube_r.bottom },
            };
            br.SetColor(&rgba(255, 210, 145, 0.98 * flicker));
            draw_d2d_text(rt, time_str, tf, &top_r, br);
            if cfg.w_clock.show_date && !date_str.is_empty() {
                let bot_r = D2D_RECT_F {
                    left: tube_r.left + 2.0 * u,
                    top: tube_r.top + (tube_r.bottom - tube_r.top) * 0.48,
                    right: tube_r.right - 2.0 * u,
                    bottom: tube_r.bottom - 1.0 * u,
                };
                br.SetColor(&rgba(nix_col.0, nix_col.1, nix_col.2, 0.86));
                draw_d2d_text(rt, date_str, tf, &bot_r, br);
            }
        }
        DockClockEngine::AnalogChronograph => {
            // Precision analog chronograph dial on left + digital readout on right
            let dial_r = (h * 0.36).clamp(9.0 * u, 15.0 * u);
            let dcx = slot_rect.left + dial_r + 5.0 * u;
            let dcy = (slot_rect.top + slot_rect.bottom) * 0.5;
            br.SetColor(&rgba(spec.bg_bot.0, spec.bg_bot.1, spec.bg_bot.2, 0.82));
            rt.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: dcx, Y: dcy }, radiusX: dial_r, radiusY: dial_r }, br);
            br.SetColor(&rgba(txt_acc.0, txt_acc.1, txt_acc.2, 0.90));
            rt.DrawEllipse(&D2D1_ELLIPSE { point: Vector2 { X: dcx, Y: dcy }, radiusX: dial_r, radiusY: dial_r }, br, 1.2 * u, None);

            let h_ang = ((hour24 % 12) as f32 + minute as f32 / 60.0) * (PI / 6.0) - FRAC_PI_2;
            let m_ang = (minute as f32 + second as f32 / 60.0) * (PI / 30.0) - FRAC_PI_2;
            let s_ang = (second as f32) * (PI / 30.0) - FRAC_PI_2;
            br.SetColor(&rgba(txt_pri.0, txt_pri.1, txt_pri.2, 0.96));
            rt.DrawLine(
                Vector2 { X: dcx, Y: dcy },
                Vector2 { X: dcx + h_ang.cos() * dial_r * 0.52, Y: dcy + h_ang.sin() * dial_r * 0.52 },
                br,
                1.5 * u,
                None,
            );
            rt.DrawLine(
                Vector2 { X: dcx, Y: dcy },
                Vector2 { X: dcx + m_ang.cos() * dial_r * 0.76, Y: dcy + m_ang.sin() * dial_r * 0.76 },
                br,
                1.2 * u,
                None,
            );
            br.SetColor(&rgba(txt_acc.0, txt_acc.1, txt_acc.2, 0.92));
            rt.DrawLine(
                Vector2 { X: dcx, Y: dcy },
                Vector2 { X: dcx + s_ang.cos() * dial_r * 0.84, Y: dcy + s_ang.sin() * dial_r * 0.84 },
                br,
                0.9 * u,
                None,
            );

            let text_left = dcx + dial_r + 3.0 * u;
            if text_left < slot_rect.right - 12.0 * u {
                let top_r = D2D_RECT_F {
                    left: text_left,
                    top: slot_rect.top + 3.0 * u,
                    right: slot_rect.right - 2.0 * u,
                    bottom: if cfg.w_clock.show_date { slot_rect.top + h * 0.56 } else { slot_rect.bottom - 3.0 * u },
                };
                br.SetColor(&rgba(txt_pri.0, txt_pri.1, txt_pri.2, 0.96));
                draw_d2d_text(rt, time_str, tf, &top_r, br);
                if cfg.w_clock.show_date && !date_str.is_empty() {
                    let bot_r = D2D_RECT_F {
                        left: text_left,
                        top: slot_rect.top + h * 0.46,
                        right: slot_rect.right - 2.0 * u,
                        bottom: slot_rect.bottom - 3.0 * u,
                    };
                    br.SetColor(&rgba(txt_mut.0, txt_mut.1, txt_mut.2, 0.86));
                    draw_d2d_text(rt, date_str, tf, &bot_r, br);
                }
            }
        }
        DockClockEngine::TerminalMatrix => {
            let term_t = format!("[{}]", time_str);
            let top_r = D2D_RECT_F {
                left: slot_rect.left + 1.0 * u,
                top: slot_rect.top + 3.0 * u,
                right: slot_rect.right - 1.0 * u,
                bottom: if cfg.w_clock.show_date { slot_rect.top + h * 0.56 } else { slot_rect.bottom - 3.0 * u },
            };
            br.SetColor(&rgba(txt_acc.0, txt_acc.1, txt_acc.2, 0.98));
            draw_d2d_text(rt, &term_t, tf, &top_r, br);
            if cfg.w_clock.show_date && !date_str.is_empty() {
                let bot_r = D2D_RECT_F {
                    left: slot_rect.left + 1.0 * u,
                    top: slot_rect.top + h * 0.46,
                    right: slot_rect.right - 1.0 * u,
                    bottom: slot_rect.bottom - 3.0 * u,
                };
                br.SetColor(&rgba(txt_acc.0, txt_acc.1, txt_acc.2, 0.72));
                draw_d2d_text(rt, date_str, tf, &bot_r, br);
            }
        }
        DockClockEngine::BinaryMatrix => {
            // 4-column BCD binary indicator dots on top + digital time below
            let digits = [hour24 / 10, hour24 % 10, minute / 10, minute % 10];
            let grid_w = 34.0 * u;
            let gx0 = slot_rect.left + (w - grid_w) * 0.5;
            let gy0 = slot_rect.top + 5.5 * u;
            for (ci, &d) in digits.iter().enumerate() {
                for bit in 0..4 {
                    let on = ((d >> (3 - bit)) & 1) == 1;
                    let px = gx0 + ci as f32 * 10.0 * u;
                    let py = gy0 + bit as f32 * 4.2 * u;
                    if on {
                        br.SetColor(&rgba(txt_acc.0, txt_acc.1, txt_acc.2, 0.96));
                    } else {
                        br.SetColor(&rgba(txt_mut.0, txt_mut.1, txt_mut.2, 0.28));
                    }
                    rt.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: px, Y: py }, radiusX: 1.5 * u, radiusY: 1.5 * u }, br);
                }
            }
            let bot_r = D2D_RECT_F {
                left: slot_rect.left + 2.0 * u,
                top: slot_rect.top + h * 0.48,
                right: slot_rect.right - 2.0 * u,
                bottom: slot_rect.bottom - 2.0 * u,
            };
            br.SetColor(&rgba(txt_pri.0, txt_pri.1, txt_pri.2, 0.92));
            draw_d2d_text(rt, time_str, tf, &bot_r, br);
        }
        DockClockEngine::SingleLinePill => {
            let line = if cfg.w_clock.show_date && !date_str.is_empty() {
                format!("{} • {}", time_str, date_str)
            } else {
                time_str.to_string()
            };
            br.SetColor(&rgba(txt_pri.0, txt_pri.1, txt_pri.2, 0.96));
            draw_d2d_text(rt, &line, tf, &slot_rect, br);
        }
        DockClockEngine::StackedDigital | DockClockEngine::InheritTheme => {
            if cfg.w_clock.show_date && !date_str.is_empty() {
                let top_r = D2D_RECT_F {
                    left: slot_rect.left + 2.0 * u,
                    top: slot_rect.top + 3.0 * u,
                    right: slot_rect.right - 2.0 * u,
                    bottom: slot_rect.top + h * 0.56,
                };
                let bot_r = D2D_RECT_F {
                    left: slot_rect.left + 2.0 * u,
                    top: slot_rect.top + h * 0.46,
                    right: slot_rect.right - 2.0 * u,
                    bottom: slot_rect.bottom - 3.0 * u,
                };
                br.SetColor(&rgba(txt_pri.0, txt_pri.1, txt_pri.2, 0.96));
                draw_d2d_text(rt, time_str, tf, &top_r, br);
                br.SetColor(&rgba(txt_mut.0, txt_mut.1, txt_mut.2, 0.88));
                draw_d2d_text(rt, date_str, tf, &bot_r, br);
            } else {
                br.SetColor(&rgba(txt_pri.0, txt_pri.1, txt_pri.2, 0.96));
                draw_d2d_text(rt, time_str, tf, &slot_rect, br);
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub unsafe fn draw_dock_tooltip(
    rt: &ID2D1DCRenderTarget,
    br: &ID2D1SolidColorBrush,
    tf: &IDWriteTextFormat,
    theme: TaskbarTheme,
    label: &str,
    tip_rect: D2D_RECT_F,
    u: f32,
) {
    let spec = dock_spec(theme);
    let rad = match spec.shape {
        DockShelfShape::Chamfer | DockShelfShape::Tile => 4.0 * u,
        _ => 7.0 * u,
    };
    let rr = D2D1_ROUNDED_RECT { rect: tip_rect, radiusX: rad, radiusY: rad };
    br.SetColor(&rgba(spec.tip_bg.0, spec.tip_bg.1, spec.tip_bg.2, spec.tip_bg.3));
    rt.FillRoundedRectangle(&rr, br);
    br.SetColor(&rgba(spec.tip_border.0, spec.tip_border.1, spec.tip_border.2, spec.tip_border.3));
    rt.DrawRoundedRectangle(&rr, br, 1.0 * u, None);
    br.SetColor(&rgba(spec.tip_fg.0, spec.tip_fg.1, spec.tip_fg.2, spec.tip_fg.3));
    draw_d2d_text(rt, label, tf, &tip_rect, br);
}

/// Structurally accurate Studio preview thumbnails reflecting each theme's real desktop architecture!
#[allow(clippy::too_many_arguments)]
pub unsafe fn draw_dock_preview_card(
    rt: &ID2D1DCRenderTarget,
    d2d: Option<&ID2D1Factory>,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    u: f32,
    theme: TaskbarTheme,
    cfg: &AppConfig,
    pal: &ThemePalette,
    anim_phase: f32,
) {
    let Ok(br) = rt.CreateSolidColorBrush(&rgba(255, 255, 255, 1.0), None) else {
        return;
    };
    let spec = dock_spec(theme);
    let icon_cols = [
        (76u8, 175u8, 255u8),
        (245, 185, 72),
        (68, 210, 196),
        (196, 206, 224),
        (74, 216, 128),
    ];
    let sweep_n = ((anim_phase * 0.9).sin() * 0.22 + 0.5).clamp(0.15, 0.85);
    let bot_y = (y + h) * u;

    match spec.layout {
        DockLayoutKind::MultiIslandBar => {
            // 3 distinctly separated floating islands!
            let iy0 = (y + h - 30.0) * u;
            let iy1 = (y + h - 6.0) * u;
            let islands = [
                D2D_RECT_F { left: (x + 10.0) * u, top: iy0, right: (x + 76.0) * u, bottom: iy1 },
                D2D_RECT_F { left: (x + 86.0) * u, top: iy0, right: (x + 216.0) * u, bottom: iy1 },
                D2D_RECT_F { left: (x + 226.0) * u, top: iy0, right: (x + w - 10.0) * u, bottom: iy1 },
            ];
            for isl in islands {
                draw_dock_shelf_and_art(rt, d2d, &br, isl, cfg, pal, anim_phase, 0.7, sweep_n, false, u, true);
            }
            let cy = (iy0 + iy1) * 0.5;
            // Left island: Start dot + active-app bar stub
            br.SetColor(&rgba(spec.dot_active.0, spec.dot_active.1, spec.dot_active.2, 0.92));
            rt.FillRoundedRectangle(
                &D2D1_ROUNDED_RECT { rect: D2D_RECT_F { left: (x + 17.0) * u, top: cy - 5.0 * u, right: (x + 27.0) * u, bottom: cy + 5.0 * u }, radiusX: 3.0 * u, radiusY: 3.0 * u },
                &br,
            );
            br.SetColor(&rgba(spec.tip_fg.0, spec.tip_fg.1, spec.tip_fg.2, 0.65));
            rt.FillRoundedRectangle(
                &D2D1_ROUNDED_RECT { rect: D2D_RECT_F { left: (x + 32.0) * u, top: cy - 2.2 * u, right: (x + 68.0) * u, bottom: cy + 2.2 * u }, radiusX: 1.1 * u, radiusY: 1.1 * u },
                &br,
            );
            // Center island icons (no mac fish-eye bulge upward!)
            for (idx, &(ir, ig, ib)) in icon_cols.iter().enumerate() {
                let ix = (x + 101.0 + idx as f32 * 25.0) * u;
                let h_s = if idx == 2 { 6.2 * u } else { 5.2 * u };
                br.SetColor(&rgba(ir, ig, ib, 0.96));
                rt.FillRoundedRectangle(
                    &D2D1_ROUNDED_RECT { rect: D2D_RECT_F { left: ix - h_s, top: cy - h_s, right: ix + h_s, bottom: cy + h_s }, radiusX: 3.0 * u, radiusY: 3.0 * u },
                    &br,
                );
                if idx == 1 || idx == 2 {
                    draw_dock_indicator(rt, &br, theme, idx, ix, cy, D2D_RECT_F::default(), islands[1], MacDockEdge::Bottom, 1, idx == 2, u);
                }
            }
            // Right island: live clock pill + sys dot
            br.SetColor(&rgba(spec.tip_fg.0, spec.tip_fg.1, spec.tip_fg.2, 0.80));
            rt.FillRoundedRectangle(
                &D2D1_ROUNDED_RECT { rect: D2D_RECT_F { left: (x + 235.0) * u, top: cy - 2.6 * u, right: (x + 266.0) * u, bottom: cy + 2.6 * u }, radiusX: 1.2 * u, radiusY: 1.2 * u },
                &br,
            );
            br.SetColor(&rgba(spec.dot_active.0, spec.dot_active.1, spec.dot_active.2, 0.85));
            rt.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: (x + w - 22.0) * u, Y: cy }, radiusX: 4.0 * u, radiusY: 4.0 * u }, &br);
        }
        DockLayoutKind::FullEdgeTaskbar => {
            // Edge-to-edge flush taskbar anchored at bottom + wide labeled window buttons + right clock!
            let shelf_r = D2D_RECT_F { left: x * u, top: (y + h - 25.0) * u, right: (x + w) * u, bottom: bot_y };
            draw_dock_shelf_and_art(rt, d2d, &br, shelf_r, cfg, pal, anim_phase, 0.65, sweep_n, false, u, true);
            let cy = (shelf_r.top + shelf_r.bottom) * 0.5;
            // Start tile flush left
            br.SetColor(&rgba(spec.dot_active.0, spec.dot_active.1, spec.dot_active.2, 0.92));
            rt.FillRoundedRectangle(
                &D2D1_ROUNDED_RECT { rect: D2D_RECT_F { left: (x + 9.0) * u, top: cy - 6.0 * u, right: (x + 21.0) * u, bottom: cy + 6.0 * u }, radiusX: 3.0 * u, radiusY: 3.0 * u },
                &br,
            );
            // Wide labeled window buttons on left side
            let mut bx = (x + 30.0) * u;
            for (idx, &(ir, ig, ib)) in icon_cols.iter().take(4).enumerate() {
                let is_wide = idx < 2;
                let bw = if is_wide { 54.0 * u } else { 20.0 * u };
                let btn_r = D2D_RECT_F { left: bx, top: cy - 8.5 * u, right: bx + bw, bottom: cy + 8.5 * u };
                draw_dock_slot_fx(rt, d2d, &br, btn_r, bx + 9.0 * u, cy, 12.0 * u, shelf_r, theme, idx == 0, idx == 0, is_wide, false, false, anim_phase, idx, u);
                br.SetColor(&rgba(ir, ig, ib, 0.95));
                rt.FillRoundedRectangle(
                    &D2D1_ROUNDED_RECT { rect: D2D_RECT_F { left: bx + 4.5 * u, top: cy - 4.8 * u, right: bx + 14.1 * u, bottom: cy + 4.8 * u }, radiusX: 2.2 * u, radiusY: 2.2 * u },
                    &br,
                );
                if is_wide {
                    br.SetColor(&rgba(244, 248, 255, 0.78));
                    rt.FillRoundedRectangle(
                        &D2D1_ROUNDED_RECT { rect: D2D_RECT_F { left: bx + 18.0 * u, top: cy - 1.8 * u, right: bx + bw - 6.0 * u, bottom: cy + 1.8 * u }, radiusX: 1.0 * u, radiusY: 1.0 * u },
                        &br,
                    );
                }
                bx += bw + 4.0 * u;
            }
            // Right-end Clock + Show Desktop sliver
            br.SetColor(&rgba(240, 248, 255, 0.72));
            rt.FillRoundedRectangle(
                &D2D1_ROUNDED_RECT { rect: D2D_RECT_F { left: (x + w - 42.0) * u, top: cy - 2.2 * u, right: (x + w - 12.0) * u, bottom: cy + 2.2 * u }, radiusX: 1.1 * u, radiusY: 1.1 * u },
                &br,
            );
            br.SetColor(&rgba(spec.rim_outer.0, spec.rim_outer.1, spec.rim_outer.2, 0.55));
            rt.DrawLine(Vector2 { X: (x + w - 6.0) * u, Y: shelf_r.top + 3.0 * u }, Vector2 { X: (x + w - 6.0) * u, Y: bot_y - 3.0 * u }, &br, 1.0 * u, None);
        }
        DockLayoutKind::IndependentOrbs => {
            // ZERO shelf bar! Pure standalone orbs / octagonal crystals floating individually
            let cx_mid = (x + w * 0.5) * u;
            let base_y = (y + h - 22.0) * u;
            // Subtle connective thread on Biolum/Prism
            if theme != TaskbarTheme::InvisibleGhostDock {
                br.SetColor(&rgba(spec.dot_glow.0, spec.dot_glow.1, spec.dot_glow.2, 0.24));
                rt.DrawLine(Vector2 { X: cx_mid - 74.0 * u, Y: base_y }, Vector2 { X: cx_mid + 74.0 * u, Y: base_y }, &br, 0.9 * u, None);
            }
            for (idx, &(ir, ig, ib)) in icon_cols.iter().enumerate() {
                let ix = cx_mid + (idx as f32 - 2.0) * 36.0 * u;
                let bob = if theme == TaskbarTheme::BiolumAbyssDock {
                    (anim_phase * 2.0 + idx as f32 * 1.1).sin() * 2.8 * u
                } else {
                    0.0
                };
                let iy = base_y + bob;
                let hov = idx == 2;
                let sz = if hov { 18.0 * u } else { 14.0 * u };
                draw_standalone_orb_node(rt, d2d, &br, ix, iy, sz, theme, hov, anim_phase, idx, u);
                let h_s = sz * 0.38;
                br.SetColor(&rgba(ir, ig, ib, 0.96));
                rt.FillRoundedRectangle(
                    &D2D1_ROUNDED_RECT { rect: D2D_RECT_F { left: ix - h_s, top: iy - h_s, right: ix + h_s, bottom: iy + h_s }, radiusX: h_s * 0.45, radiusY: h_s * 0.45 },
                    &br,
                );
                if idx == 0 || idx == 2 || idx == 4 {
                    let s_r = D2D_RECT_F { left: ix - h_s, top: iy - h_s, right: ix + h_s, bottom: iy + sz * 0.54 };
                    let p_shelf = D2D_RECT_F { left: s_r.left, top: s_r.top, right: s_r.right, bottom: iy + sz * 0.54 + 5.5 * u };
                    draw_dock_indicator(rt, &br, theme, idx, ix, iy, s_r, p_shelf, MacDockEdge::Bottom, 1, hov, u);
                }
            }
        }
        DockLayoutKind::InlineMorphRail => {
            // Flat architectural strip + one slot morphed open horizontally [Icon | Label] inline!
            let shelf_r = D2D_RECT_F { left: (x + 18.0) * u, top: (y + h - 30.0) * u, right: (x + w - 18.0) * u, bottom: (y + h - 6.0) * u };
            draw_dock_shelf_and_art(rt, d2d, &br, shelf_r, cfg, pal, anim_phase, 0.65, sweep_n, false, u, true);
            let cy = (shelf_r.top + shelf_r.bottom) * 0.5;
            let mut tx = shelf_r.left + 10.0 * u;
            for (idx, &(ir, ig, ib)) in icon_cols.iter().enumerate() {
                let is_expanded = idx == 2;
                let tw = if is_expanded { 60.0 * u } else { 19.0 * u };
                let t_rect = D2D_RECT_F { left: tx, top: cy - 8.5 * u, right: tx + tw, bottom: cy + 8.5 * u };
                draw_dock_slot_fx(rt, d2d, &br, t_rect, tx + 9.0 * u, cy, 12.0 * u, shelf_r, theme, is_expanded, is_expanded, is_expanded, false, false, anim_phase, idx, u);
                br.SetColor(&rgba(ir, ig, ib, 0.96));
                rt.FillRoundedRectangle(
                    &D2D1_ROUNDED_RECT { rect: D2D_RECT_F { left: tx + 4.5 * u, top: cy - 4.8 * u, right: tx + 14.1 * u, bottom: cy + 4.8 * u }, radiusX: 2.0 * u, radiusY: 2.0 * u },
                    &br,
                );
                if is_expanded {
                    br.SetColor(&rgba(spec.rim_outer.0, spec.rim_outer.1, spec.rim_outer.2, 0.85));
                    rt.FillRoundedRectangle(
                        &D2D1_ROUNDED_RECT { rect: D2D_RECT_F { left: tx + 18.0 * u, top: cy - 2.0 * u, right: tx + tw - 6.0 * u, bottom: cy + 2.0 * u }, radiusX: 1.0 * u, radiusY: 1.0 * u },
                        &br,
                    );
                }
                tx += tw + 5.0 * u;
            }
            // Integrated clock badge on right of strip
            br.SetColor(&rgba(spec.rim_outer.0, spec.rim_outer.1, spec.rim_outer.2, 0.72));
            rt.FillRoundedRectangle(
                &D2D1_ROUNDED_RECT { rect: D2D_RECT_F { left: shelf_r.right - 34.0 * u, top: cy - 2.4 * u, right: shelf_r.right - 8.0 * u, bottom: cy + 2.4 * u }, radiusX: 1.0 * u, radiusY: 1.0 * u },
                &br,
            );
        }
        DockLayoutKind::HudConsoleDeck => {
            // Stepped telemetry header deck + socketed icons (chamfered/portholes/reticles)
            let hdr_r = D2D_RECT_F { left: (x + 68.0) * u, top: (y + h - 39.0) * u, right: (x + w - 68.0) * u, bottom: (y + h - 27.0) * u };
            let shelf_r = D2D_RECT_F { left: (x + 24.0) * u, top: (y + h - 29.0) * u, right: (x + w - 24.0) * u, bottom: (y + h - 5.0) * u };
            // Header telemetry wing
            br.SetColor(&rgba(spec.bg_bot.0, spec.bg_bot.1, spec.bg_bot.2, 0.90));
            rt.FillRoundedRectangle(&D2D1_ROUNDED_RECT { rect: hdr_r, radiusX: 3.0 * u, radiusY: 3.0 * u }, &br);
            br.SetColor(&rgba(spec.rim_outer.0, spec.rim_outer.1, spec.rim_outer.2, 0.62));
            rt.DrawRoundedRectangle(&D2D1_ROUNDED_RECT { rect: hdr_r, radiusX: 3.0 * u, radiusY: 3.0 * u }, &br, 1.0 * u, None);
            br.SetColor(&rgba(spec.dot_active.0, spec.dot_active.1, spec.dot_active.2, 0.80));
            rt.FillRoundedRectangle(
                &D2D1_ROUNDED_RECT { rect: D2D_RECT_F { left: hdr_r.left + 8.0 * u, top: hdr_r.top + 3.0 * u, right: hdr_r.left + 58.0 * u, bottom: hdr_r.top + 6.5 * u }, radiusX: 1.0 * u, radiusY: 1.0 * u },
                &br,
            );
            rt.FillRoundedRectangle(
                &D2D1_ROUNDED_RECT { rect: D2D_RECT_F { left: hdr_r.right - 34.0 * u, top: hdr_r.top + 3.0 * u, right: hdr_r.right - 8.0 * u, bottom: hdr_r.top + 6.5 * u }, radiusX: 1.0 * u, radiusY: 1.0 * u },
                &br,
            );

            draw_dock_shelf_and_art(rt, d2d, &br, shelf_r, cfg, pal, anim_phase, 0.72, sweep_n, false, u, true);
            let cy = (shelf_r.top + shelf_r.bottom) * 0.5;
            let dock_cx = (shelf_r.left + shelf_r.right) * 0.5;
            for (idx, &(ir, ig, ib)) in icon_cols.iter().enumerate() {
                let ix = dock_cx + (idx as f32 - 2.0) * 32.0 * u;
                let hov = idx == 2;
                let s_rect = D2D_RECT_F { left: ix - 8.5 * u, top: cy - 8.5 * u, right: ix + 8.5 * u, bottom: cy + 8.5 * u };
                draw_dock_slot_fx(rt, d2d, &br, s_rect, ix, cy, 13.0 * u, shelf_r, theme, hov, hov, true, false, false, anim_phase, idx, u);
                br.SetColor(&rgba(ir, ig, ib, 0.96));
                rt.FillRoundedRectangle(
                    &D2D1_ROUNDED_RECT { rect: D2D_RECT_F { left: ix - 5.0 * u, top: cy - 5.0 * u, right: ix + 5.0 * u, bottom: cy + 5.0 * u }, radiusX: 2.2 * u, radiusY: 2.2 * u },
                    &br,
                );
                if idx == 0 || idx == 2 || idx == 3 {
                    draw_dock_indicator(rt, &br, theme, idx, ix, cy, s_rect, shelf_r, MacDockEdge::Bottom, 1, hov, u);
                }
            }
        }
        DockLayoutKind::FishEyeShelf => {
            // Classic floating curved shelf + parabolic wave magnification
            let shelf_r = D2D_RECT_F {
                left: (x + 26.0) * u,
                top: (y + h - 33.5) * u,
                right: (x + w - 26.0) * u,
                bottom: (y + h - 7.5) * u,
            };
            draw_dock_shelf_and_art(rt, d2d, &br, shelf_r, cfg, pal, anim_phase, 0.75, sweep_n, false, u, true);
            let dock_cx = (shelf_r.left + shelf_r.right) * 0.5;
            let cy = (shelf_r.top + shelf_r.bottom) * 0.5;
            let sweep = (anim_phase * 0.9).sin() * 1.15;
            for (idx, &(ir, ig, ib)) in icon_cols.iter().enumerate() {
                let dist = ((idx as f32 - 2.0) - sweep).abs();
                let sw_k = if dist < 1.8 {
                    let c = (FRAC_PI_2 * (dist / 1.8)).cos();
                    1.0 + 0.50 * c * c
                } else {
                    1.0
                };
                let half = 5.8 * u * sw_k;
                let ix = dock_cx + (idx as f32 - 2.0) * 23.0 * u + ((idx as f32 - 2.0) - sweep).clamp(-1.0, 1.0) * 1.6 * u;
                let icy = cy - (sw_k - 1.0) * 7.0 * u;
                let s_rect = D2D_RECT_F { left: ix - half, top: icy - half, right: ix + half, bottom: icy + half };
                draw_dock_slot_fx(rt, d2d, &br, s_rect, ix, icy, half * 2.0, shelf_r, theme, dist < 0.65, idx == 2, true, false, false, anim_phase, idx, u);
                br.SetColor(&rgba(ir, ig, ib, 0.96));
                rt.FillRoundedRectangle(&D2D1_ROUNDED_RECT { rect: s_rect, radiusX: half * 0.48, radiusY: half * 0.48 }, &br);
                if idx == 0 || idx == 2 || idx == 3 {
                    draw_dock_indicator(rt, &br, theme, idx, ix, icy, s_rect, shelf_r, MacDockEdge::Bottom, 1, idx == 2, u);
                }
            }
            draw_dock_divider(rt, &br, dock_cx + 76.0 * u, cy, shelf_r, theme, false, u);
            br.SetColor(&rgba(spec.dot_idle.0, spec.dot_idle.1, spec.dot_idle.2, 0.72));
            rt.FillRoundedRectangle(
                &D2D1_ROUNDED_RECT {
                    rect: D2D_RECT_F { left: dock_cx + 86.0 * u, top: cy - 5.0 * u, right: dock_cx + 96.0 * u, bottom: cy + 5.0 * u },
                    radiusX: 2.5 * u,
                    radiusY: 2.5 * u,
                },
                &br,
            );
        }
    }
    let _ = PI;
}

// =========================================================================
// ZERO-ALLOCATION LUXURY CAPSULE & TELEMETRY RING-PILL DIRECT2D ENGINE
// =========================================================================

#[inline]
fn lux_fnv1a_step(h: u64, v: u64) -> u64 {
    let mut x = h ^ v;
    x = x.wrapping_mul(0x100000001b3);
    x
}

#[inline]
fn lux_hash_bytes(mut h: u64, bytes: &[u8]) -> u64 {
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

#[inline]
fn lux_col_key(c: D2D1_COLOR_F) -> u64 {
    let r = (c.r.clamp(0.0, 1.0) * 255.0).round() as u64;
    let g = (c.g.clamp(0.0, 1.0) * 255.0).round() as u64;
    let b = (c.b.clamp(0.0, 1.0) * 255.0).round() as u64;
    let a = (c.a.clamp(0.0, 1.0) * 255.0).round() as u64;
    r | (g << 8) | (b << 16) | (a << 24)
}

#[inline]
pub fn lerp_d2d_color(a: D2D1_COLOR_F, b: D2D1_COLOR_F, t: f32) -> D2D1_COLOR_F {
    let t = t.clamp(0.0, 1.0);
    D2D1_COLOR_F {
        r: a.r + (b.r - a.r) * t,
        g: a.g + (b.g - a.g) * t,
        b: a.b + (b.b - a.b) * t,
        a: a.a + (b.a - a.a) * t,
    }
}

/// Formats `0..=999` + `'%'` directly into a stack UTF-16 buffer with zero heap allocation.
#[inline]
pub fn fmt_pct_u16(pct: f32, buf: &mut [u16; 16]) -> usize {
    let v = pct.clamp(0.0, 999.0).round() as u32;
    let mut n = fmt_u32_u16(v, buf);
    if n < buf.len() {
        buf[n] = '%' as u16;
        n += 1;
    }
    n
}

/// Formats temperature `0..=999` + `'°'` directly into a stack UTF-16 buffer with zero heap allocation.
#[inline]
pub fn fmt_temp_c_u16(temp_c: f32, with_unit: bool, buf: &mut [u16; 16]) -> usize {
    let v = temp_c.clamp(0.0, 999.0).round() as u32;
    let mut n = fmt_u32_u16(v, buf);
    if n < buf.len() {
        buf[n] = 0x00B0; // degree symbol '°'
        n += 1;
    }
    if with_unit && n < buf.len() {
        buf[n] = 'C' as u16;
        n += 1;
    }
    n
}

#[inline]
fn fmt_u32_u16(mut v: u32, buf: &mut [u16; 16]) -> usize {
    let mut rev = [0u16; 8];
    let mut r_len = 0usize;
    if v == 0 {
        rev[0] = '0' as u16;
        r_len = 1;
    } else {
        while v > 0 && r_len < rev.len() {
            rev[r_len] = ('0' as u16) + (v % 10) as u16;
            v /= 10;
            r_len += 1;
        }
    }
    let count = r_len.min(buf.len());
    for i in 0..count {
        buf[i] = rev[r_len - 1 - i];
    }
    count
}

#[derive(Default)]
struct CachedGradSlot {
    key: u64,
    brush: Option<ID2D1LinearGradientBrush>,
}

#[derive(Default)]
struct CachedArcSlot {
    key: u64,
    geo: Option<ID2D1PathGeometry>,
}

#[derive(Default)]
struct CachedFmtSlot {
    key: u64,
    tf: Option<IDWriteTextFormat>,
}

#[derive(Default)]
struct CachedLayoutSlot {
    key: u64,
    layout: Option<IDWriteTextLayout>,
}

struct LuxuryRtEntry {
    rt_ptr: usize,
    last_used: u64,
    factory_ptr: usize,
    solid_br: ID2D1SolidColorBrush,
    round_cap: Option<ID2D1StrokeStyle>,
    cached_bmp_ptr: usize,
    cached_bmp_brush: Option<ID2D1BitmapBrush>,
    grad_slots: [CachedGradSlot; 10],
    arc_slots: [CachedArcSlot; 18],
    fmt_slots: [CachedFmtSlot; 12],
    layout_slots: [CachedLayoutSlot; 8],
}

impl LuxuryRtEntry {
    unsafe fn new(rt: &ID2D1RenderTarget, stamp: u64) -> Option<Self> {
        let solid_br = rt.CreateSolidColorBrush(&rgba(255, 255, 255, 1.0), None).ok()?;
        Some(Self {
            rt_ptr: rt.as_raw() as usize,
            last_used: stamp,
            factory_ptr: 0,
            solid_br,
            round_cap: None,
            cached_bmp_ptr: 0,
            cached_bmp_brush: None,
            grad_slots: Default::default(),
            arc_slots: Default::default(),
            fmt_slots: Default::default(),
            layout_slots: Default::default(),
        })
    }

    unsafe fn get_or_build_bmp_brush(
        &mut self,
        rt: &ID2D1RenderTarget,
        bmp: &ID2D1Bitmap,
    ) -> Option<ID2D1BitmapBrush> {
        let bp = bmp.as_raw() as usize;
        if self.cached_bmp_ptr != bp || self.cached_bmp_brush.is_none() {
            self.cached_bmp_ptr = bp;
            let props = D2D1_BITMAP_BRUSH_PROPERTIES {
                extendModeX: D2D1_EXTEND_MODE_CLAMP,
                extendModeY: D2D1_EXTEND_MODE_CLAMP,
                interpolationMode: D2D1_BITMAP_INTERPOLATION_MODE_LINEAR,
            };
            self.cached_bmp_brush = rt.CreateBitmapBrush(bmp, Some(&props), None).ok();
        }
        self.cached_bmp_brush.clone()
    }

    unsafe fn ensure_round_cap(&mut self, fac: &ID2D1Factory) -> Option<ID2D1StrokeStyle> {
        let fp = fac.as_raw() as usize;
        if self.factory_ptr != fp || self.round_cap.is_none() {
            self.factory_ptr = fp;
            self.round_cap = fac
                .CreateStrokeStyle(
                    &D2D1_STROKE_STYLE_PROPERTIES {
                        startCap: D2D1_CAP_STYLE_ROUND,
                        endCap: D2D1_CAP_STYLE_ROUND,
                        ..Default::default()
                    },
                    None,
                )
                .ok();
            // Clear factory-bound geometries when factory identity changes
            for s in &mut self.arc_slots {
                s.key = 0;
                s.geo = None;
            }
        }
        self.round_cap.clone()
    }

    unsafe fn get_or_build_grad(
        &mut self,
        rt: &ID2D1RenderTarget,
        slot: usize,
        p0: Vector2,
        p1: Vector2,
        stops: &[D2D1_GRADIENT_STOP],
    ) -> Option<ID2D1LinearGradientBrush> {
        let slot = slot % self.grad_slots.len();
        let mut h = 0xcbf29ce484222325u64;
        h = lux_fnv1a_step(h, stops.len() as u64);
        for st in stops {
            h = lux_fnv1a_step(h, lux_col_key(st.color));
            h = lux_fnv1a_step(h, (st.position * 1000.0).round() as u64);
        }
        if self.grad_slots[slot].key == h {
            if let Some(ref br) = self.grad_slots[slot].brush {
                br.SetStartPoint(p0);
                br.SetEndPoint(p1);
                return Some(br.clone());
            }
        }
        let col = rt
            .CreateGradientStopCollection(stops, D2D1_GAMMA_2_2, D2D1_EXTEND_MODE_CLAMP)
            .ok()?;
        let props = D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES {
            startPoint: p0,
            endPoint: p1,
        };
        let br = rt.CreateLinearGradientBrush(&props, None, &col).ok()?;
        self.grad_slots[slot].key = h;
        self.grad_slots[slot].brush = Some(br.clone());
        Some(br)
    }

    unsafe fn get_or_build_arc(
        &mut self,
        fac: &ID2D1Factory,
        slot: usize,
        cx: f32,
        cy: f32,
        r: f32,
        start_deg: f32,
        sweep_deg: f32,
    ) -> Option<ID2D1PathGeometry> {
        let slot = slot % self.arc_slots.len();
        let (a_beg, sw) = if sweep_deg < 0.0 {
            (start_deg + sweep_deg, (-sweep_deg).min(359.4))
        } else {
            (start_deg, sweep_deg.min(359.4))
        };
        if sw < 0.25 || r < 0.5 {
            return None;
        }
        let fp = fac.as_raw() as usize;
        let mut key = 0xcbf29ce484222325u64;
        key = lux_fnv1a_step(key, fp as u64);
        key = lux_fnv1a_step(key, (cx * 8.0).round() as i64 as u64);
        key = lux_fnv1a_step(key, (cy * 8.0).round() as i64 as u64);
        key = lux_fnv1a_step(key, (r * 8.0).round() as i64 as u64);
        key = lux_fnv1a_step(key, (a_beg * 8.0).round() as i64 as u64);
        key = lux_fnv1a_step(key, (sw * 8.0).round() as i64 as u64);

        if self.arc_slots[slot].key == key {
            if let Some(ref geo) = self.arc_slots[slot].geo {
                return Some(geo.clone());
            }
        }
        let a0 = a_beg.to_radians();
        let a1 = (a_beg + sw).to_radians();
        let p0 = Vector2 { X: cx + r * a0.cos(), Y: cy + r * a0.sin() };
        let p1 = Vector2 { X: cx + r * a1.cos(), Y: cy + r * a1.sin() };
        let geo = fac.CreatePathGeometry().ok()?;
        let sink = geo.Open().ok()?;
        sink.BeginFigure(p0, D2D1_FIGURE_BEGIN_HOLLOW);
        sink.AddArc(&D2D1_ARC_SEGMENT {
            point: p1,
            size: D2D_SIZE_F { width: r, height: r },
            rotationAngle: 0.0,
            sweepDirection: D2D1_SWEEP_DIRECTION_CLOCKWISE,
            arcSize: if sw > 180.0 { D2D1_ARC_SIZE_LARGE } else { D2D1_ARC_SIZE_SMALL },
        });
        sink.EndFigure(D2D1_FIGURE_END_OPEN);
        sink.Close().ok()?;
        self.arc_slots[slot].key = key;
        self.arc_slots[slot].geo = Some(geo.clone());
        Some(geo)
    }

    unsafe fn get_or_build_top_rim(
        &mut self,
        fac: &ID2D1Factory,
        slot: usize,
        rect: D2D_RECT_F,
        rad: f32,
    ) -> Option<ID2D1PathGeometry> {
        let slot = slot % self.arc_slots.len();
        let w = (rect.right - rect.left).max(4.0);
        let h = (rect.bottom - rect.top).max(4.0);
        let r = rad.clamp(2.0, (w * 0.5).min(h * 0.5));
        let inset = 0.75f32;
        let top_y = rect.top + inset;
        let ri = (r - inset).max(1.5);
        let fp = fac.as_raw() as usize;
        let mut key = 0x811c9dc5u64;
        key = lux_fnv1a_step(key, fp as u64);
        key = lux_fnv1a_step(key, (rect.left * 4.0).round() as i64 as u64);
        key = lux_fnv1a_step(key, (rect.top * 4.0).round() as i64 as u64);
        key = lux_fnv1a_step(key, (w * 4.0).round() as i64 as u64);
        key = lux_fnv1a_step(key, (h * 4.0).round() as i64 as u64);
        key = lux_fnv1a_step(key, (ri * 4.0).round() as i64 as u64);
        if self.arc_slots[slot].key == key {
            if let Some(ref geo) = self.arc_slots[slot].geo {
                return Some(geo.clone());
            }
        }
        // Trace open top-lip contour from left upper shoulder (~205 deg) -> crown -> right upper shoulder (~335 deg)
        let lcx = rect.left + inset + ri;
        let rcx = rect.right - inset - ri;
        let cy = rect.top + inset + ri;
        let a_start = 204.0f32.to_radians();
        let a_end = 336.0f32.to_radians();
        let p_start = Vector2 { X: lcx + ri * a_start.cos(), Y: cy + ri * a_start.sin() };
        let p_top_l = Vector2 { X: lcx, Y: top_y };
        let p_top_r = Vector2 { X: rcx, Y: top_y };
        let p_end = Vector2 { X: rcx + ri * a_end.cos(), Y: cy + ri * a_end.sin() };
        let geo = fac.CreatePathGeometry().ok()?;
        let sink = geo.Open().ok()?;
        sink.BeginFigure(p_start, D2D1_FIGURE_BEGIN_HOLLOW);
        sink.AddArc(&D2D1_ARC_SEGMENT {
            point: p_top_l,
            size: D2D_SIZE_F { width: ri, height: ri },
            rotationAngle: 0.0,
            sweepDirection: D2D1_SWEEP_DIRECTION_CLOCKWISE,
            arcSize: D2D1_ARC_SIZE_SMALL,
        });
        if p_top_r.X > p_top_l.X + 0.5 {
            sink.AddLine(p_top_r);
        }
        sink.AddArc(&D2D1_ARC_SEGMENT {
            point: p_end,
            size: D2D_SIZE_F { width: ri, height: ri },
            rotationAngle: 0.0,
            sweepDirection: D2D1_SWEEP_DIRECTION_CLOCKWISE,
            arcSize: D2D1_ARC_SIZE_SMALL,
        });
        sink.EndFigure(D2D1_FIGURE_END_OPEN);
        sink.Close().ok()?;
        self.arc_slots[slot].key = key;
        self.arc_slots[slot].geo = Some(geo.clone());
        Some(geo)
    }

    unsafe fn get_or_build_tf(
        &mut self,
        dw: &IDWriteFactory,
        custom_fonts: Option<&IDWriteFontCollection>,
        slot: usize,
        preferred: &str,
        size_dip: f32,
        weight: DWRITE_FONT_WEIGHT,
        align_center: bool,
    ) -> Option<IDWriteTextFormat> {
        let slot = slot % self.fmt_slots.len();
        let dw_ptr = dw.as_raw() as usize;
        let cf_ptr = custom_fonts.map(|c| c.as_raw() as usize).unwrap_or(0);
        let q_sz = (size_dip.clamp(5.0, 96.0) * 10.0).round() as u64;
        let mut key = 0xcbf29ce484222325u64;
        key = lux_fnv1a_step(key, dw_ptr as u64);
        key = lux_fnv1a_step(key, cf_ptr as u64);
        key = lux_fnv1a_step(key, q_sz);
        key = lux_fnv1a_step(key, weight.0 as u64);
        key = lux_fnv1a_step(key, u64::from(align_center));
        key = lux_hash_bytes(key, preferred.trim().as_bytes());

        if self.fmt_slots[slot].key == key {
            if let Some(ref tf) = self.fmt_slots[slot].tf {
                return Some(tf.clone());
            }
        }

        let try_create = |name_w: PCWSTR, coll: Option<&IDWriteFontCollection>| -> Option<IDWriteTextFormat> {
            dw.CreateTextFormat(
                name_w,
                coll,
                weight,
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                (q_sz as f32) / 10.0,
                windows::core::w!("en-US"),
            )
            .ok()
        };

        let mut tf_opt: Option<IDWriteTextFormat> = None;
        let trimmed = preferred.trim();
        if !trimmed.is_empty() {
            let mut fbuf = [0u16; 64];
            let mut fn_len = 0usize;
            for cu in trimmed.encode_utf16() {
                if fn_len + 1 >= fbuf.len() {
                    break;
                }
                fbuf[fn_len] = cu;
                fn_len += 1;
            }
            fbuf[fn_len] = 0;
            let name_pw = PCWSTR(fbuf.as_ptr());
            if let Some(cf) = custom_fonts {
                let mut idx = 0u32;
                let mut exists = windows::core::BOOL(0);
                if cf.FindFamilyName(name_pw, &mut idx, &mut exists).is_ok() && exists.as_bool() {
                    tf_opt = try_create(name_pw, Some(cf));
                }
            }
            if tf_opt.is_none() {
                let mut sys_col = None;
                if dw.GetSystemFontCollection(&mut sys_col, false).is_ok() {
                    if let Some(ref sc) = sys_col {
                        let mut idx = 0u32;
                        let mut exists = windows::core::BOOL(0);
                        if sc.FindFamilyName(name_pw, &mut idx, &mut exists).is_ok() && exists.as_bool() {
                            tf_opt = try_create(name_pw, None);
                        }
                    }
                }
            }
        }
        if tf_opt.is_none() {
            if let Some(cf) = custom_fonts {
                let sf = windows::core::w!("SF Pro Display");
                let mut idx = 0u32;
                let mut exists = windows::core::BOOL(0);
                if cf.FindFamilyName(sf, &mut idx, &mut exists).is_ok() && exists.as_bool() {
                    tf_opt = try_create(sf, Some(cf));
                }
            }
        }
        if tf_opt.is_none() {
            tf_opt = try_create(windows::core::w!("Segoe UI"), None);
        }

        if let Some(ref tf) = tf_opt {
            let _ = tf.SetTextAlignment(if align_center {
                DWRITE_TEXT_ALIGNMENT_CENTER
            } else {
                DWRITE_TEXT_ALIGNMENT_LEADING
            });
            let _ = tf.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER);
            let _ = tf.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP);
            let trim = DWRITE_TRIMMING {
                granularity: DWRITE_TRIMMING_GRANULARITY_CHARACTER,
                delimiter: 0,
                delimiterCount: 0,
            };
            if let Ok(sign) = dw.CreateEllipsisTrimmingSign(tf) {
                let _ = tf.SetTrimming(&trim, &sign);
            }
            self.fmt_slots[slot].key = key;
            self.fmt_slots[slot].tf = Some(tf.clone());
        }
        tf_opt
    }

    unsafe fn get_or_build_layout(
        &mut self,
        dw: &IDWriteFactory,
        custom_fonts: Option<&IDWriteFontCollection>,
        slot: usize,
        preferred_font: &str,
        size_dip: f32,
        weight: DWRITE_FONT_WEIGHT,
        align_center: bool,
        text: &str,
        max_w: f32,
        max_h: f32,
    ) -> Option<IDWriteTextLayout> {
        let lslot = slot % self.layout_slots.len();
        let fslot = 4 + (slot % 8);
        let qw = (max_w.max(4.0) * 4.0).round() as u64;
        let qh = (max_h.max(4.0) * 4.0).round() as u64;
        let mut key = 0xcbf29ce484222325u64;
        key = lux_fnv1a_step(key, dw.as_raw() as usize as u64);
        key = lux_fnv1a_step(key, (size_dip * 10.0).round() as u64);
        key = lux_fnv1a_step(key, weight.0 as u64);
        key = lux_fnv1a_step(key, u64::from(align_center));
        key = lux_fnv1a_step(key, qw);
        key = lux_fnv1a_step(key, qh);
        key = lux_hash_bytes(key, preferred_font.as_bytes());
        key = lux_hash_bytes(key, text.as_bytes());

        if self.layout_slots[lslot].key == key {
            if let Some(ref lay) = self.layout_slots[lslot].layout {
                return Some(lay.clone());
            }
        }
        let tf = self.get_or_build_tf(dw, custom_fonts, fslot, preferred_font, size_dip, weight, align_center)?;
        let mut tbuf = [0u16; 192];
        let mut tlen = 0usize;
        for cu in text.encode_utf16() {
            if tlen >= tbuf.len() {
                break;
            }
            tbuf[tlen] = cu;
            tlen += 1;
        }
        let layout = dw
            .CreateTextLayout(&tbuf[..tlen], &tf, max_w.max(4.0), max_h.max(4.0))
            .ok()?;
        let _ = layout.SetParagraphAlignment(if align_center {
            DWRITE_PARAGRAPH_ALIGNMENT_CENTER
        } else {
            DWRITE_PARAGRAPH_ALIGNMENT_NEAR
        });
        let trim = DWRITE_TRIMMING {
            granularity: DWRITE_TRIMMING_GRANULARITY_CHARACTER,
            delimiter: 0,
            delimiterCount: 0,
        };
        if let Ok(sign) = dw.CreateEllipsisTrimmingSign(&tf) {
            let _ = layout.SetTrimming(&trim, &sign);
        }
        self.layout_slots[lslot].key = key;
        self.layout_slots[lslot].layout = Some(layout.clone());
        Some(layout)
    }
}

struct LuxHudCacheStore {
    stamp: u64,
    entries: Vec<LuxuryRtEntry>,
}

impl LuxHudCacheStore {
    const fn new() -> Self {
        Self {
            stamp: 0,
            entries: Vec::new(),
        }
    }
}

thread_local! {
    static LUX_HUD_CACHE: RefCell<LuxHudCacheStore> = const { RefCell::new(LuxHudCacheStore::new()) };
}

pub fn evict_luxury_hud_rt(rt: &ID2D1RenderTarget) {
    let ptr = rt.as_raw() as usize;
    LUX_HUD_CACHE.with(|c| {
        if let Ok(mut store) = c.try_borrow_mut() {
            store.entries.retain(|e| e.rt_ptr != ptr);
        }
    });
}

#[cfg(test)]
pub fn clear_luxury_hud_caches() {
    LUX_HUD_CACHE.with(|c| {
        if let Ok(mut store) = c.try_borrow_mut() {
            store.entries.clear();
        }
    });
}

unsafe fn with_lux_rt_entry<F, R>(rt: &ID2D1RenderTarget, f: F) -> Option<R>
where
    F: FnOnce(&mut LuxuryRtEntry) -> R,
{
    let ptr = rt.as_raw() as usize;
    LUX_HUD_CACHE.with(|c| {
        let mut store = c.try_borrow_mut().ok()?;
        store.stamp = store.stamp.wrapping_add(1);
        let stamp = store.stamp;
        if let Some(pos) = store.entries.iter().position(|e| e.rt_ptr == ptr) {
            store.entries[pos].last_used = stamp;
            return Some(f(&mut store.entries[pos]));
        }
        if store.entries.len() >= 8 {
            if let Some((oldest_i, _)) = store
                .entries
                .iter()
                .enumerate()
                .min_by_key(|(_, e)| e.last_used)
            {
                store.entries.swap_remove(oldest_i);
            }
        }
        let mut entry = LuxuryRtEntry::new(rt, stamp)?;
        let out = f(&mut entry);
        store.entries.push(entry);
        Some(out)
    })
}

/// Draws a bitmap clipped smoothly to a rounded rectangle using a cached `ID2D1BitmapBrush`
/// with zero `ID2D1Layer` or geometry allocations per frame.
pub unsafe fn draw_cached_rounded_bitmap(
    rt: &ID2D1RenderTarget,
    bmp: &ID2D1Bitmap,
    rr: &D2D1_ROUNDED_RECT,
) -> bool {
    let bsz = bmp.GetSize();
    if bsz.width <= 0.5 || bsz.height <= 0.5 {
        return false;
    }
    let dw = (rr.rect.right - rr.rect.left).max(1.0);
    let dh = (rr.rect.bottom - rr.rect.top).max(1.0);
    let mut drawn = false;
    let _ = with_lux_rt_entry(rt, |cache| {
        if let Some(br) = cache.get_or_build_bmp_brush(rt, bmp) {
            let xf = Matrix3x2 {
                M11: dw / bsz.width,
                M12: 0.0,
                M21: 0.0,
                M22: dh / bsz.height,
                M31: rr.rect.left,
                M32: rr.rect.top,
            };
            br.SetTransform(&xf);
            rt.FillRoundedRectangle(rr, &br);
            drawn = true;
        }
    });
    drawn
}

/// Renders an original smoked frosted-glass floating capsule (`rx = h / 2`) with Chameleon
/// ambient glass tinting, soft internal frosted sheen, and a curved 1px subtle top rim highlight.
#[allow(clippy::too_many_arguments)]
pub unsafe fn draw_luxury_glass_capsule(
    rt: &ID2D1RenderTarget,
    d2d: Option<&ID2D1Factory>,
    rect: D2D_RECT_F,
    rx_opt: Option<f32>,
    pal: &ThemePalette,
    glass_alpha_pct: u8,
    rim_alpha_pct: u8,
    s: f32,
    hovered: bool,
    custom_bg: Option<(D2D1_COLOR_F, D2D1_COLOR_F)>,
    custom_rim: Option<D2D1_COLOR_F>,
) {
    let w = (rect.right - rect.left).max(16.0);
    let h = (rect.bottom - rect.top).max(16.0);
    let rad = rx_opt.unwrap_or(h * 0.5).clamp(2.0, (h * 0.5).min(w * 0.5));
    let pill_rr = D2D1_ROUNDED_RECT {
        rect,
        radiusX: rad,
        radiusY: rad,
    };
    let ga = (glass_alpha_pct as f32 / 255.0).clamp(0.18, 0.94);
    let ra = (rim_alpha_pct as f32 / 255.0).clamp(0.08, 1.0);

    let _ = with_lux_rt_entry(rt, |cache| {
        // 1. Soft luxury ambient elevation shadow
        for (expand, off_y, alpha) in [(5.5 * s, 2.2 * s, 0.22f32), (2.2 * s, 1.0 * s, 0.28f32)] {
            let sh = D2D1_ROUNDED_RECT {
                rect: D2D_RECT_F {
                    left: rect.left - expand * 0.5,
                    top: rect.top + off_y,
                    right: rect.right + expand * 0.5,
                    bottom: rect.bottom + expand + off_y * 0.4,
                },
                radiusX: rad + expand * 0.5,
                radiusY: rad + expand * 0.5,
            };
            cache.solid_br.SetColor(&rgba(4, 6, 10, alpha));
            rt.FillRoundedRectangle(&sh, &cache.solid_br);
        }

        // 2. Frosted dark glass body with Live Chameleon ambient color inheritance
        let obsidian_top = rgba(18, 21, 28, 1.0);
        let obsidian_bot = rgba(9, 11, 16, 1.0);
        let (bg_top, bg_bot) = if let Some((c1, c2)) = custom_bg {
            (c1, c2)
        } else {
            let ct = lerp_d2d_color(obsidian_top, pal.glass_top_col(1.0), 0.34);
            let cb = lerp_d2d_color(obsidian_bot, pal.glass_bot_col(1.0), 0.28);
            (
                D2D1_COLOR_F { a: (ga + 0.32).clamp(0.68, 0.94), ..ct },
                D2D1_COLOR_F { a: (ga + 0.38).clamp(0.76, 0.96), ..cb },
            )
        };
        let bg_mid = lerp_d2d_color(bg_top, bg_bot, 0.52);
        let bg_stops = [
            D2D1_GRADIENT_STOP { position: 0.0, color: bg_top },
            D2D1_GRADIENT_STOP { position: 0.46, color: bg_mid },
            D2D1_GRADIENT_STOP { position: 1.0, color: bg_bot },
        ];
        if let Some(bg_br) = cache.get_or_build_grad(
            rt,
            0,
            Vector2 { X: rect.left, Y: rect.top },
            Vector2 { X: rect.left, Y: rect.bottom },
            &bg_stops,
        ) {
            rt.FillRoundedRectangle(&pill_rr, &bg_br);
        }

        // 3. Subtle diagonal Chameleon ambient bloom (subtle studio warmth/tint)
        if custom_bg.is_none() {
            let acc_c = pal.accent_col(if hovered { 0.14 } else { 0.09 });
            let acc_fade = pal.accent_col(0.0);
            let acc_stops = [
                D2D1_GRADIENT_STOP { position: 0.0, color: acc_c },
                D2D1_GRADIENT_STOP { position: 0.42, color: acc_fade },
                D2D1_GRADIENT_STOP { position: 1.0, color: pal.deep_col(0.04) },
            ];
            if let Some(ab) = cache.get_or_build_grad(
                rt,
                1,
                Vector2 { X: rect.left, Y: rect.top },
                Vector2 { X: rect.right, Y: rect.bottom },
                &acc_stops,
            ) {
                rt.FillRoundedRectangle(&pill_rr, &ab);
            }
        }

        // 4. Inner frosted crown sheen (zero-allocation, no ID2D1Layer)
        let inset = 1.0 * s;
        let sheen_rr = D2D1_ROUNDED_RECT {
            rect: D2D_RECT_F {
                left: pill_rr.rect.left + inset,
                top: pill_rr.rect.top + inset,
                right: pill_rr.rect.right - inset,
                bottom: pill_rr.rect.bottom - inset,
            },
            radiusX: (pill_rr.radiusX - inset).max(0.0),
            radiusY: (pill_rr.radiusY - inset).max(0.0),
        };
        let sheen_stops = [
            D2D1_GRADIENT_STOP { position: 0.0, color: rgba(255, 255, 255, if hovered { 0.095 } else { 0.065 }) },
            D2D1_GRADIENT_STOP { position: 0.24, color: rgba(255, 255, 255, if hovered { 0.028 } else { 0.018 }) },
            D2D1_GRADIENT_STOP { position: 0.46, color: rgba(255, 255, 255, 0.0) },
            D2D1_GRADIENT_STOP { position: 1.0, color: rgba(255, 255, 255, 0.0) },
        ];
        if let Some(sb) = cache.get_or_build_grad(
            rt,
            2,
            Vector2 { X: rect.left, Y: rect.top },
            Vector2 { X: rect.left, Y: rect.bottom },
            &sheen_stops,
        ) {
            rt.FillRoundedRectangle(&sheen_rr, &sb);
        }

        // 5. Outer fine glass rim outline tinted subtly by Chameleon rim/accent
        let rim_base = custom_rim.unwrap_or_else(|| lerp_d2d_color(rgba(218, 228, 244, 1.0), pal.rim_col(1.0), 0.45));
        let outer_col = D2D1_COLOR_F {
            a: (ra * if hovered { 0.42 } else { 0.26 }).clamp(0.08, 0.65),
            ..rim_base
        };
        cache.solid_br.SetColor(&outer_col);
        rt.DrawRoundedRectangle(&pill_rr, &cache.solid_br, 0.9 * s, None);

        // 6. Signature 1px subtle top edge highlight curved smoothly along capsule upper lip
        let top_mid = custom_rim.unwrap_or_else(|| lerp_d2d_color(rgba(255, 255, 255, 1.0), pal.light_col(1.0), 0.22));
        let hi_edge = D2D1_COLOR_F { a: 0.0, ..top_mid };
        let hi_mid = D2D1_COLOR_F {
            a: (0.28 + ra * 0.24 + if hovered { 0.12 } else { 0.0 }).clamp(0.18, 0.62),
            ..top_mid
        };
        let hi_crown = D2D1_COLOR_F {
            a: (0.44 + ra * 0.28 + if hovered { 0.14 } else { 0.0 }).clamp(0.28, 0.78),
            ..top_mid
        };
        let hi_stops = [
            D2D1_GRADIENT_STOP { position: 0.0, color: hi_edge },
            D2D1_GRADIENT_STOP { position: 0.22, color: hi_mid },
            D2D1_GRADIENT_STOP { position: 0.5, color: hi_crown },
            D2D1_GRADIENT_STOP { position: 0.78, color: hi_mid },
            D2D1_GRADIENT_STOP { position: 1.0, color: hi_edge },
        ];
        let stroke_w = 1.0 * s.clamp(0.85, 1.45);
        if let Some(hg) = cache.get_or_build_grad(
            rt,
            3,
            Vector2 { X: rect.left, Y: rect.top },
            Vector2 { X: rect.right, Y: rect.top },
            &hi_stops,
        ) {
            if let Some(fac) = d2d {
                if let Some(rim_geo) = cache.get_or_build_top_rim(fac, 16, rect, rad) {
                    let cap = cache.ensure_round_cap(fac);
                    rt.DrawGeometry(&rim_geo, &hg, stroke_w, cap.as_ref());
                    return;
                }
            }
            let inset_x = (rad * 0.52).clamp(8.0 * s, w * 0.25);
            let hl_y = rect.top + 0.85 * s;
            rt.DrawLine(
                Vector2 { X: rect.left + inset_x, Y: hl_y },
                Vector2 { X: rect.right - inset_x, Y: hl_y },
                &hg,
                stroke_w,
                None,
            );
        }
    });
}

/// Renders a precision circular arc telemetry meter using cached Direct2D `ID2D1PathGeometry`
/// (`D2D1_ARC_SEGMENT`) + cached `D2D1_CAP_STYLE_ROUND` stroke with zero steady-state allocations.
#[allow(clippy::too_many_arguments)]
pub unsafe fn draw_telemetry_arc_ring(
    rt: &ID2D1RenderTarget,
    d2d: Option<&ID2D1Factory>,
    slot_idx: usize,
    cx: f32,
    cy: f32,
    radius: f32,
    stroke_w: f32,
    start_deg: f32,
    max_sweep_deg: f32,
    ratio: f32,
    track_col: D2D1_COLOR_F,
    accent_col: D2D1_COLOR_F,
    glow_col: D2D1_COLOR_F,
    s: f32,
) {
    let r = radius.max(3.0);
    let max_sw = max_sweep_deg.clamp(30.0, 359.4);
    let act_sw = (ratio.clamp(0.02, 1.0) * max_sw).min(359.4);
    let _ = with_lux_rt_entry(rt, |cache| {
        if let Some(fac) = d2d {
            let cap = cache.ensure_round_cap(fac);
            let track_slot = (slot_idx % 6) * 2;
            let val_slot = track_slot + 1;

            if let Some(tr_geo) = cache.get_or_build_arc(fac, track_slot, cx, cy, r, start_deg, max_sw) {
                // Subtle recessed instrument bezel shadow inside the arc ring
                cache.solid_br.SetColor(&rgba(0, 0, 0, 0.20));
                rt.DrawEllipse(
                    &D2D1_ELLIPSE {
                        point: Vector2 { X: cx, Y: cy },
                        radiusX: (r - stroke_w * 0.72).max(2.0),
                        radiusY: (r - stroke_w * 0.72).max(2.0),
                    },
                    &cache.solid_br,
                    (stroke_w * 0.42).max(0.75 * s),
                    None,
                );
                cache.solid_br.SetColor(&track_col);
                rt.DrawGeometry(&tr_geo, &cache.solid_br, stroke_w, cap.as_ref());
            }

            if let Some(val_geo) = cache.get_or_build_arc(fac, val_slot, cx, cy, r, start_deg, act_sw) {
                if glow_col.a > 0.01 {
                    cache.solid_br.SetColor(&glow_col);
                    rt.DrawGeometry(&val_geo, &cache.solid_br, stroke_w * 2.05, cap.as_ref());
                }
                cache.solid_br.SetColor(&accent_col);
                rt.DrawGeometry(&val_geo, &cache.solid_br, stroke_w, cap.as_ref());

                // Crisp specular highlight pip at live arc endpoint
                let end_rad = (start_deg + act_sw).to_radians();
                let pip = Vector2 {
                    X: cx + r * end_rad.cos(),
                    Y: cy + r * end_rad.sin(),
                };
                cache.solid_br.SetColor(&rgba(255, 255, 255, (accent_col.a * 0.72).clamp(0.15, 0.85)));
                let pip_r = (stroke_w * 0.42).max(1.1 * s);
                rt.FillEllipse(&D2D1_ELLIPSE { point: pip, radiusX: pip_r, radiusY: pip_r }, &cache.solid_br);
            }
            return;
        }

        // GDI/fallback elliptical track when factory handle is absent
        cache.solid_br.SetColor(&track_col);
        rt.DrawEllipse(
            &D2D1_ELLIPSE {
                point: Vector2 { X: cx, Y: cy },
                radiusX: r,
                radiusY: r,
            },
            &cache.solid_br,
            stroke_w,
            None,
        );
    });
}

/// Renders pulsing rounded vertical equalizer waveform pills hooked to live audio bands
/// with zero heap/COM allocations per frame.
#[allow(clippy::too_many_arguments)]
pub unsafe fn draw_pill_equalizer_waveform(
    rt: &ID2D1RenderTarget,
    rect: D2D_RECT_F,
    bands: &[f32; 12],
    is_playing: bool,
    pal: &ThemePalette,
    c1_ov: Option<D2D1_COLOR_F>,
    c2_ov: Option<D2D1_COLOR_F>,
    s: f32,
) {
    const BAR_COUNT: usize = 10;
    const BAND_MAP: [usize; BAR_COUNT] = [1, 2, 3, 5, 6, 7, 8, 9, 10, 11];
    const SHAPE_ENV: [f32; BAR_COUNT] = [0.58, 0.76, 0.92, 1.0, 0.96, 1.0, 0.92, 0.80, 0.66, 0.54];

    let avail_w = (rect.right - rect.left).max(12.0);
    let avail_h = (rect.bottom - rect.top).max(8.0);
    let cy = (rect.top + rect.bottom) * 0.5;
    let step = avail_w / BAR_COUNT as f32;
    let bar_w = (step * 0.54).clamp(2.2 * s, 3.6 * s);
    let rad = bar_w * 0.5;
    let raw_sum: f32 = bands.iter().copied().sum();
    let active = is_playing || raw_sum > 0.025;

    let col_root = c2_ov.unwrap_or_else(|| pal.accent_col(0.90));
    let col_peak = c1_ov.unwrap_or_else(|| lerp_d2d_color(pal.light_col(1.0), rgba(255, 255, 255, 1.0), 0.22));
    let col_idle = lerp_d2d_color(pal.muted_col(0.42), pal.accent_col(0.36), 0.45);

    let _ = with_lux_rt_entry(rt, |cache| {
        for i in 0..BAR_COUNT {
            let bx = rect.left + (i as f32 + 0.5) * step - bar_w * 0.5;
            let amp = bands[BAND_MAP[i]].clamp(0.0, 1.0);
            let scaled = (amp * SHAPE_ENV[i] * 1.18).clamp(0.0, 1.0);
            let bh = if active {
                (bar_w + (avail_h - bar_w) * (0.14 + scaled * 0.86)).clamp(bar_w, avail_h)
            } else {
                (bar_w * (0.95 + 0.35 * SHAPE_ENV[i])).clamp(2.4 * s, avail_h * 0.25)
            };
            let bar_col = if active {
                lerp_d2d_color(col_root, col_peak, scaled)
            } else {
                col_idle
            };
            if active && scaled > 0.14 {
                let glow_expand = 1.15 * s;
                let glow_rr = D2D1_ROUNDED_RECT {
                    rect: D2D_RECT_F {
                        left: bx - glow_expand,
                        top: cy - bh * 0.5 - glow_expand * 0.65,
                        right: bx + bar_w + glow_expand,
                        bottom: cy + bh * 0.5 + glow_expand * 0.65,
                    },
                    radiusX: rad + glow_expand,
                    radiusY: rad + glow_expand,
                };
                cache.solid_br.SetColor(&D2D1_COLOR_F {
                    a: (0.12 + scaled * 0.14).clamp(0.08, 0.26),
                    ..bar_col
                });
                rt.FillRoundedRectangle(&glow_rr, &cache.solid_br);
            }
            let rr = D2D1_ROUNDED_RECT {
                rect: D2D_RECT_F {
                    left: bx,
                    top: cy - bh * 0.5,
                    right: bx + bar_w,
                    bottom: cy + bh * 0.5,
                },
                radiusX: rad,
                radiusY: rad,
            };
            cache.solid_br.SetColor(&bar_col);
            rt.FillRoundedRectangle(&rr, &cache.solid_br);
            if active && scaled > 0.42 {
                let cap_r = (bar_w * 0.34).max(0.85 * s);
                cache.solid_br.SetColor(&rgba(255, 255, 255, (0.35 + scaled * 0.35).clamp(0.3, 0.72)));
                rt.FillEllipse(
                    &D2D1_ELLIPSE {
                        point: Vector2 { X: bx + bar_w * 0.5, Y: cy - bh * 0.5 + cap_r + 0.5 * s },
                        radiusX: cap_r,
                        radiusY: cap_r,
                    },
                    &cache.solid_br,
                );
            }
        }
    });
}

/// Zero-allocation stack UTF-16 DirectWrite draw helper using cached `IDWriteTextFormat`.
#[allow(clippy::too_many_arguments)]
pub unsafe fn draw_cached_u16_text(
    rt: &ID2D1RenderTarget,
    dw: &IDWriteFactory,
    custom_fonts: Option<&IDWriteFontCollection>,
    fmt_slot: usize,
    preferred_font: &str,
    size_dip: f32,
    bold: bool,
    align_center: bool,
    rect: D2D_RECT_F,
    col: D2D1_COLOR_F,
    shadow_col: Option<D2D1_COLOR_F>,
    shadow_off: f32,
    u16_slice: &[u16],
) {
    let weight = if bold {
        DWRITE_FONT_WEIGHT_BOLD
    } else {
        DWRITE_FONT_WEIGHT_SEMI_BOLD
    };
    let _ = with_lux_rt_entry(rt, |cache| {
        if let Some(tf) = cache.get_or_build_tf(dw, custom_fonts, fmt_slot, preferred_font, size_dip, weight, align_center) {
            if let Some(sc) = shadow_col {
                let s_rc = D2D_RECT_F {
                    left: rect.left + shadow_off,
                    top: rect.top + shadow_off,
                    right: rect.right + shadow_off,
                    bottom: rect.bottom + shadow_off,
                };
                cache.solid_br.SetColor(&sc);
                rt.DrawText(u16_slice, &tf, &s_rc, &cache.solid_br, D2D1_DRAW_TEXT_OPTIONS_CLIP, DWRITE_MEASURING_MODE_NATURAL);
            }
            cache.solid_br.SetColor(&col);
            rt.DrawText(u16_slice, &tf, &rect, &cache.solid_br, D2D1_DRAW_TEXT_OPTIONS_CLIP, DWRITE_MEASURING_MODE_NATURAL);
        }
    });
}

/// Draws single-line text with cached `IDWriteTextLayout` + ellipsis truncation (`...`),
/// incurring zero UTF-16 or layout allocations across steady 60 FPS animation frames.
#[allow(clippy::too_many_arguments)]
pub unsafe fn draw_cached_ellipsis_text(
    rt: &ID2D1RenderTarget,
    dw: &IDWriteFactory,
    custom_fonts: Option<&IDWriteFontCollection>,
    slot: usize,
    preferred_font: &str,
    size_dip: f32,
    semibold: bool,
    text: &str,
    rect: D2D_RECT_F,
    col: D2D1_COLOR_F,
    shadow_col: Option<D2D1_COLOR_F>,
    shadow_off: f32,
) {
    let max_w = (rect.right - rect.left).max(4.0);
    let max_h = (rect.bottom - rect.top).max(4.0);
    let weight = if semibold {
        DWRITE_FONT_WEIGHT_SEMI_BOLD
    } else {
        DWRITE_FONT_WEIGHT_REGULAR
    };
    let _ = with_lux_rt_entry(rt, |cache| {
        if let Some(layout) = cache.get_or_build_layout(
            dw,
            custom_fonts,
            slot,
            preferred_font,
            size_dip,
            weight,
            false,
            text,
            max_w,
            max_h,
        ) {
            if let Some(sc) = shadow_col {
                cache.solid_br.SetColor(&sc);
                rt.DrawTextLayout(
                    Vector2 { X: rect.left + shadow_off, Y: rect.top + shadow_off },
                    &layout,
                    &cache.solid_br,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                );
            }
            cache.solid_br.SetColor(&col);
            rt.DrawTextLayout(
                Vector2 { X: rect.left, Y: rect.top },
                &layout,
                &cache.solid_br,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
            );
        }
    });
}

/// Draws a reusable Direct2D vector transport glyph (`prev`, `play`, `pause`, `next`) or filled/stroked circle
/// using the cached solid brush in `dock_themes`.
pub unsafe fn with_cached_solid_brush<F>(rt: &ID2D1RenderTarget, col: D2D1_COLOR_F, f: F)
where
    F: FnOnce(&ID2D1SolidColorBrush),
{
    let _ = with_lux_rt_entry(rt, |cache| {
        cache.solid_br.SetColor(&col);
        f(&cache.solid_br);
    });
}

/// Zero-allocation cached filled triangle geometry (`ID2D1PathGeometry` cached by quantized vertices).
pub unsafe fn draw_cached_triangle(
    rt: &ID2D1RenderTarget,
    d2d: Option<&ID2D1Factory>,
    slot_idx: usize,
    p0: Vector2,
    p1: Vector2,
    p2: Vector2,
    col: D2D1_COLOR_F,
) {
    let Some(fac) = d2d else { return; };
    let slot = 12 + (slot_idx % 4);
    let _ = with_lux_rt_entry(rt, |cache| {
        let mut key = 0x811c9dc5u64;
        key = lux_fnv1a_step(key, fac.as_raw() as usize as u64);
        for p in [p0, p1, p2] {
            key = lux_fnv1a_step(key, (p.X * 4.0).round() as i64 as u64);
            key = lux_fnv1a_step(key, (p.Y * 4.0).round() as i64 as u64);
        }
        let geo_opt = if cache.arc_slots[slot].key == key {
            cache.arc_slots[slot].geo.clone()
        } else {
            let built = (|| -> Option<ID2D1PathGeometry> {
                let g = fac.CreatePathGeometry().ok()?;
                let sink = g.Open().ok()?;
                sink.BeginFigure(p0, D2D1_FIGURE_BEGIN_FILLED);
                sink.AddLine(p1);
                sink.AddLine(p2);
                sink.EndFigure(D2D1_FIGURE_END_CLOSED);
                sink.Close().ok()?;
                Some(g)
            })();
            if let Some(ref g) = built {
                cache.arc_slots[slot].key = key;
                cache.arc_slots[slot].geo = Some(g.clone());
            }
            built
        };
        if let Some(ref g) = geo_opt {
            cache.solid_br.SetColor(&col);
            rt.FillGeometry(g, &cache.solid_br, None);
        }
    });
}

/// Zero-allocation cached crescent moon glyph (`ID2D1PathGeometry` cached by center/radius).
pub unsafe fn draw_cached_crescent(
    rt: &ID2D1RenderTarget,
    d2d: Option<&ID2D1Factory>,
    cx: f32,
    cy: f32,
    r: f32,
    col: D2D1_COLOR_F,
) {
    let Some(fac) = d2d else { return; };
    use windows::Win32::Graphics::Direct2D::Common::ID2D1SimplifiedGeometrySink;
    use windows::Win32::Graphics::Direct2D::D2D1_COMBINE_MODE_EXCLUDE;
    let _ = with_lux_rt_entry(rt, |cache| {
        const SLOT: usize = 15;
        let mut key = 0x811c9dc5u64;
        key = lux_fnv1a_step(key, fac.as_raw() as usize as u64);
        key = lux_fnv1a_step(key, (cx * 4.0).round() as i64 as u64);
        key = lux_fnv1a_step(key, (cy * 4.0).round() as i64 as u64);
        key = lux_fnv1a_step(key, (r * 4.0).round() as i64 as u64);
        let geo_opt = if cache.arc_slots[SLOT].key == key {
            cache.arc_slots[SLOT].geo.clone()
        } else {
            let built = (|| -> Option<ID2D1PathGeometry> {
                let outer = fac
                    .CreateEllipseGeometry(&D2D1_ELLIPSE {
                        point: Vector2 { X: cx, Y: cy },
                        radiusX: r,
                        radiusY: r,
                    })
                    .ok()?;
                let inner = fac
                    .CreateEllipseGeometry(&D2D1_ELLIPSE {
                        point: Vector2 { X: cx + r * 0.43, Y: cy - r * 0.34 },
                        radiusX: r * 0.83,
                        radiusY: r * 0.83,
                    })
                    .ok()?;
                let path = fac.CreatePathGeometry().ok()?;
                let sink = path.Open().ok()?;
                let simp: ID2D1SimplifiedGeometrySink = sink.cast().ok()?;
                let og: ID2D1Geometry = outer.cast().ok()?;
                let ig: ID2D1Geometry = inner.cast().ok()?;
                og.CombineWithGeometry(&ig, D2D1_COMBINE_MODE_EXCLUDE, None, 0.1, &simp).ok()?;
                sink.Close().ok()?;
                Some(path)
            })();
            if let Some(ref g) = built {
                cache.arc_slots[SLOT].key = key;
                cache.arc_slots[SLOT].geo = Some(g.clone());
            }
            built
        };
        if let Some(ref g) = geo_opt {
            cache.solid_br.SetColor(&col);
            rt.FillGeometry(g, &cache.solid_br, None);
        }
    });
}

#[cfg(test)]
mod luxury_engine_tests {
    use super::*;

    #[test]
    fn stack_formatters_produce_expected_utf16_without_heap_alloc() {
        let mut buf = [0u16; 16];
        let n0 = fmt_pct_u16(0.0, &mut buf);
        assert_eq!(String::from_utf16_lossy(&buf[..n0]), "0%");
        let n1 = fmt_pct_u16(42.4, &mut buf);
        assert_eq!(String::from_utf16_lossy(&buf[..n1]), "42%");
        let n2 = fmt_pct_u16(100.0, &mut buf);
        assert_eq!(String::from_utf16_lossy(&buf[..n2]), "100%");

        let t0 = fmt_temp_c_u16(58.6, false, &mut buf);
        assert_eq!(String::from_utf16_lossy(&buf[..t0]), "59°");
        let t1 = fmt_temp_c_u16(64.2, true, &mut buf);
        assert_eq!(String::from_utf16_lossy(&buf[..t1]), "64°C");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn independent_orb_indicator_stays_inside_shelf_on_all_edges() {
        let shelf = D2D_RECT_F {
            left: 100.0,
            top: 40.0,
            right: 500.0,
            bottom: 94.0,
        };
        let slot_bot = D2D_RECT_F {
            left: 200.0,
            top: 48.0,
            right: 234.0,
            bottom: 92.5,
        };
        let (_, by) = independent_orb_indicator_anchor(217.0, 70.0, slot_bot, shelf, MacDockEdge::Bottom, 1.0);
        assert!(by <= shelf.bottom - 3.5 && by >= shelf.top + 3.5);

        let slot_top = D2D_RECT_F {
            left: 200.0,
            top: 41.5,
            right: 234.0,
            bottom: 85.0,
        };
        let (_, ty) = independent_orb_indicator_anchor(217.0, 63.0, slot_top, shelf, MacDockEdge::Top, 1.0);
        assert!(ty >= shelf.top + 3.5 && ty <= shelf.bottom - 3.5);

        let slot_left = D2D_RECT_F {
            left: 101.0,
            top: 50.0,
            right: 145.0,
            bottom: 84.0,
        };
        let (lx, _) = independent_orb_indicator_anchor(123.0, 67.0, slot_left, shelf, MacDockEdge::Left, 1.0);
        assert!(lx >= shelf.left + 3.5 && lx <= shelf.right - 3.5);

        let slot_right = D2D_RECT_F {
            left: 455.0,
            top: 50.0,
            right: 499.0,
            bottom: 84.0,
        };
        let (rx, _) = independent_orb_indicator_anchor(477.0, 67.0, slot_right, shelf, MacDockEdge::Right, 1.0);
        assert!(rx <= shelf.right - 3.5 && rx >= shelf.left + 3.5);
    }
}
