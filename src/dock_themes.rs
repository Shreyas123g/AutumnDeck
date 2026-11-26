use std::cell::RefCell;
use std::f32::consts::{FRAC_PI_2, PI};

use windows::core::{Interface, PCWSTR};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D1_COLOR_F, D2D1_FIGURE_BEGIN_FILLED, D2D1_FIGURE_BEGIN_HOLLOW, D2D1_FIGURE_END_CLOSED,
    D2D1_FIGURE_END_OPEN, D2D1_GRADIENT_STOP, D2D_RECT_F, D2D_SIZE_F,
};
use windows::Win32::Graphics::Direct2D::{
    ID2D1Bitmap, ID2D1BitmapBrush, ID2D1DCRenderTarget, ID2D1Factory, ID2D1Geometry, ID2D1Layer,
    ID2D1LinearGradientBrush, ID2D1PathGeometry, ID2D1RadialGradientBrush, ID2D1RenderTarget,
    ID2D1SolidColorBrush, ID2D1StrokeStyle, D2D1_ANTIALIAS_MODE_PER_PRIMITIVE, D2D1_ARC_SEGMENT,
    D2D1_ARC_SIZE_LARGE, D2D1_ARC_SIZE_SMALL, D2D1_BITMAP_BRUSH_PROPERTIES,
    D2D1_BITMAP_INTERPOLATION_MODE_LINEAR, D2D1_CAP_STYLE_ROUND, D2D1_DRAW_TEXT_OPTIONS_CLIP,
    D2D1_DRAW_TEXT_OPTIONS_NONE, D2D1_ELLIPSE, D2D1_EXTEND_MODE_CLAMP, D2D1_GAMMA_2_2,
    D2D1_LAYER_OPTIONS_NONE, D2D1_LAYER_PARAMETERS, D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES,
    D2D1_RADIAL_GRADIENT_BRUSH_PROPERTIES, D2D1_ROUNDED_RECT, D2D1_STROKE_STYLE_PROPERTIES,
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
    if !cfg.tb_ambient_anim || cfg.is_dock_all_elements_off() {
        return false;
    }
    let s = resolve_shelf_spec(cfg);
    let is_horizontal = !matches!(cfg.mac_dock_edge, MacDockEdge::Left | MacDockEdge::Right);
    let hud_wing_visible = is_horizontal
        && cfg.w_hud.show_telemetry
        && (cfg.w_hud.mode == HudWingMode::AlwaysEnabled
            || (cfg.w_hud.mode == HudWingMode::InheritTheme
                && s.layout == DockLayoutKind::HudConsoleDeck));
    let shelf_art_live = cfg.w_shelf.show_art
        && (is_ambient_animated(cfg.tb_theme) || (s.ambient_animated && is_art_ambient_live(s.art)));
    let orb_live = !cfg.w_apps.is_all_elements_off()
        && cfg.tb_theme == TaskbarTheme::BiolumAbyssDock
        && matches!(s.layout, DockLayoutKind::IndependentOrbs);
    shelf_art_live || orb_live || (hud_wing_visible && cfg.w_hud.marquee_ticker)
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
        // Clean & Minimal (20 authentic Windhawk-inspired animated Virtual Docks)
        TaskbarTheme::Os26LiquidGlassLight => ThemePalette {
            accent: (94, 168, 250),
            accent_light: (255, 255, 255),
            accent_deep: (48, 112, 206),
            rim: (255, 255, 255),
            text_primary: (22, 30, 46),
            text_muted: (84, 98, 122),
            glass_top: (252, 254, 255),
            glass_bot: (228, 238, 252),
            preview_a: (96, 162, 224),
            preview_b: (172, 212, 246),
            preview_c: (74, 134, 198),
            swatches: [0xFCFEFF, 0x5EA8FA, 0xD8E8FA, 0x60A2E0, 0x3070CE],
        },
        TaskbarTheme::Os26LiquidGlassDark => ThemePalette {
            accent: (146, 206, 255),
            accent_light: (246, 251, 255),
            accent_deep: (26, 78, 128),
            rim: (232, 244, 255),
            text_primary: (245, 250, 255),
            text_muted: (182, 202, 226),
            glass_top: (28, 46, 68),
            glass_bot: (12, 22, 36),
            preview_a: (8, 36, 62),
            preview_b: (26, 88, 136),
            preview_c: (6, 24, 44),
            swatches: [0x92CEFF, 0xF6FBFF, 0x1C2E44, 0x1A5888, 0x06182C],
        },
        TaskbarTheme::MatterTilesLight => ThemePalette {
            accent: (224, 72, 132),
            accent_light: (255, 255, 255),
            accent_deep: (132, 64, 186),
            rim: (255, 255, 255),
            text_primary: (24, 20, 34),
            text_muted: (94, 86, 110),
            glass_top: (248, 246, 253),
            glass_bot: (236, 232, 246),
            preview_a: (244, 76, 186),
            preview_b: (192, 122, 242),
            preview_c: (166, 146, 248),
            swatches: [0xF8F6FD, 0xF44CBA, 0xC07AF2, 0xE04884, 0x181422],
        },
        TaskbarTheme::MatterTilesDark => ThemePalette {
            accent: (236, 108, 214),
            accent_light: (255, 232, 252),
            accent_deep: (122, 46, 148),
            rim: (248, 226, 248),
            text_primary: (246, 242, 250),
            text_muted: (182, 168, 194),
            glass_top: (46, 36, 52),
            glass_bot: (34, 26, 40),
            preview_a: (232, 74, 184),
            preview_b: (196, 108, 232),
            preview_c: (174, 118, 238),
            swatches: [0x2E2434, 0xE84AB8, 0xC46CE8, 0xFFFFFF, 0x221A28],
        },
        TaskbarTheme::OneUiLightChips => ThemePalette {
            accent: (168, 92, 255),
            accent_light: (255, 255, 255),
            accent_deep: (84, 52, 182),
            rim: (236, 242, 252),
            text_primary: (248, 250, 255),
            text_muted: (190, 198, 214),
            glass_top: (22, 26, 34),
            glass_bot: (14, 16, 22),
            preview_a: (24, 34, 44),
            preview_b: (54, 72, 86),
            preview_c: (16, 24, 32),
            swatches: [0xF4F6FA, 0xA85CFF, 0x161A22, 0x364856, 0x101820],
        },
        TaskbarTheme::OneUiDarkChips => ThemePalette {
            accent: (76, 142, 246),
            accent_light: (255, 255, 255),
            accent_deep: (36, 78, 168),
            rim: (255, 255, 255),
            text_primary: (24, 28, 38),
            text_muted: (88, 96, 114),
            glass_top: (246, 248, 254),
            glass_bot: (228, 234, 246),
            preview_a: (118, 158, 214),
            preview_b: (186, 208, 238),
            preview_c: (92, 132, 190),
            swatches: [0x1A1D26, 0xF6F8FE, 0x4C8EF6, 0xBACCEE, 0x5C84BE],
        },
        TaskbarTheme::SquircleFrosted => ThemePalette {
            accent: (218, 164, 132),
            accent_light: (252, 240, 230),
            accent_deep: (102, 68, 52),
            rim: (232, 206, 190),
            text_primary: (248, 244, 240),
            text_muted: (198, 182, 172),
            glass_top: (56, 44, 38),
            glass_bot: (38, 30, 26),
            preview_a: (148, 86, 62),
            preview_b: (196, 128, 96),
            preview_c: (112, 64, 48),
            swatches: [0x382C26, 0xDAA484, 0xC48060, 0x94563E, 0x261E1A],
        },
        TaskbarTheme::BubblesDark => ThemePalette {
            accent: (134, 168, 231),
            accent_light: (232, 242, 255),
            accent_deep: (52, 74, 128),
            rim: (134, 168, 231),
            text_primary: (242, 245, 252),
            text_muted: (166, 174, 190),
            glass_top: (42, 44, 50),
            glass_bot: (28, 30, 36),
            preview_a: (42, 14, 24),
            preview_b: (136, 48, 22),
            preview_c: (18, 12, 20),
            swatches: [0x2A2C32, 0x86A8E7, 0x883016, 0x1C1E24, 0x120C14],
        },
        TaskbarTheme::BubblesLight => ThemePalette {
            accent: (84, 152, 246),
            accent_light: (255, 255, 255),
            accent_deep: (42, 96, 188),
            rim: (255, 255, 255),
            text_primary: (24, 30, 44),
            text_muted: (88, 98, 116),
            glass_top: (250, 252, 255),
            glass_bot: (232, 238, 248),
            preview_a: (106, 156, 214),
            preview_b: (178, 208, 240),
            preview_c: (82, 130, 192),
            swatches: [0xFAFCFF, 0x5498F6, 0xE8EEF8, 0xB2D0F0, 0x5282C0],
        },
        TaskbarTheme::PillsDetached => ThemePalette {
            accent: (166, 202, 238),
            accent_light: (242, 248, 255),
            accent_deep: (58, 82, 112),
            rim: (210, 224, 240),
            text_primary: (242, 246, 252),
            text_muted: (172, 184, 200),
            glass_top: (42, 52, 64),
            glass_bot: (26, 34, 44),
            preview_a: (104, 126, 144),
            preview_b: (158, 174, 188),
            preview_c: (74, 96, 118),
            swatches: [0x2A3440, 0xA6CAEE, 0x9EAEBC, 0x687E90, 0x1A222C],
        },
        TaskbarTheme::RosePineTiles => ThemePalette {
            accent: (235, 188, 186),
            accent_light: (246, 193, 119),
            accent_deep: (196, 167, 231),
            rim: (235, 188, 186),
            text_primary: (224, 222, 244),
            text_muted: (144, 140, 170),
            glass_top: (40, 36, 60),
            glass_bot: (30, 27, 46),
            preview_a: (26, 24, 42),
            preview_b: (56, 48, 78),
            preview_c: (20, 18, 32),
            swatches: [0xEBBCBA, 0x28243C, 0xC4A7E7, 0x38304E, 0x141220],
        },
        TaskbarTheme::CatppuccinTiles => ThemePalette {
            accent: (180, 190, 254),
            accent_light: (203, 166, 247),
            accent_deep: (137, 180, 250),
            rim: (180, 190, 254),
            text_primary: (205, 214, 244),
            text_muted: (147, 153, 178),
            glass_top: (34, 34, 52),
            glass_bot: (24, 24, 38),
            preview_a: (24, 24, 38),
            preview_b: (54, 52, 82),
            preview_c: (17, 17, 27),
            swatches: [0xB4BEFE, 0x222234, 0xCBA6F7, 0x363452, 0x11111B],
        },
        TaskbarTheme::LiquidGlassClear => ThemePalette {
            accent: (196, 236, 164),
            accent_light: (255, 255, 255),
            accent_deep: (66, 118, 52),
            rim: (246, 250, 255),
            text_primary: (248, 252, 255),
            text_muted: (206, 218, 224),
            glass_top: (64, 72, 66),
            glass_bot: (34, 40, 36),
            preview_a: (92, 132, 42),
            preview_b: (146, 168, 96),
            preview_c: (68, 96, 38),
            swatches: [0xF6FAFF, 0x92A860, 0x5C842A, 0x404842, 0x222824],
        },
        TaskbarTheme::DockLikeLight => ThemePalette {
            accent: (96, 148, 232),
            accent_light: (255, 255, 255),
            accent_deep: (48, 88, 168),
            rim: (255, 255, 255),
            text_primary: (24, 30, 44),
            text_muted: (88, 98, 118),
            glass_top: (248, 250, 254),
            glass_bot: (234, 239, 248),
            preview_a: (116, 162, 224),
            preview_b: (188, 208, 242),
            preview_c: (84, 128, 202),
            swatches: [0xF8FAFE, 0x6094E8, 0xEAEFF8, 0xBCD0F2, 0x5480CA],
        },
        TaskbarTheme::DockLikeDark => ThemePalette {
            accent: (116, 168, 248),
            accent_light: (236, 244, 255),
            accent_deep: (32, 68, 148),
            rim: (148, 174, 214),
            text_primary: (242, 246, 254),
            text_muted: (164, 178, 202),
            glass_top: (29, 36, 52),
            glass_bot: (20, 26, 38),
            preview_a: (10, 48, 148),
            preview_b: (26, 98, 214),
            preview_c: (8, 32, 98),
            swatches: [0x1D2434, 0x1A62D6, 0x74A8F8, 0x141A26, 0x082062],
        },
        TaskbarTheme::BlobLight => ThemePalette {
            accent: (64, 134, 242),
            accent_light: (255, 255, 255),
            accent_deep: (36, 86, 178),
            rim: (255, 255, 255),
            text_primary: (22, 28, 38),
            text_muted: (84, 94, 110),
            glass_top: (226, 234, 246),
            glass_bot: (212, 222, 238),
            preview_a: (146, 178, 218),
            preview_b: (206, 222, 242),
            preview_c: (118, 152, 198),
            swatches: [0xFFFFFF, 0xE2EAF6, 0x4086F2, 0xCEDEF2, 0x7698C6],
        },
        TaskbarTheme::LuminosityFloat => ThemePalette {
            accent: (244, 112, 156),
            accent_light: (255, 218, 172),
            accent_deep: (132, 34, 96),
            rim: (238, 176, 210),
            text_primary: (248, 244, 252),
            text_muted: (196, 178, 208),
            glass_top: (42, 22, 46),
            glass_bot: (24, 14, 34),
            preview_a: (32, 10, 48),
            preview_b: (158, 36, 92),
            preview_c: (20, 8, 34),
            swatches: [0xF4709C, 0x2A162E, 0x9E245C, 0x180E22, 0x140822],
        },
        TaskbarTheme::BorderlessGhost => ThemePalette {
            accent: (214, 228, 246),
            accent_light: (250, 252, 255),
            accent_deep: (82, 102, 134),
            rim: (214, 228, 246),
            text_primary: (246, 249, 255),
            text_muted: (184, 196, 214),
            glass_top: (20, 24, 34),
            glass_bot: (12, 14, 22),
            preview_a: (24, 48, 78),
            preview_b: (64, 112, 152),
            preview_c: (18, 34, 58),
            swatches: [0xD6E4F6, 0x407098, 0x18304E, 0x12223A, 0x0C0E16],
        },
        TaskbarTheme::AerisGlassLight => ThemePalette {
            accent: (96, 176, 246),
            accent_light: (255, 255, 255),
            accent_deep: (42, 116, 194),
            rim: (246, 252, 255),
            text_primary: (20, 30, 44),
            text_muted: (78, 96, 118),
            glass_top: (224, 242, 254),
            glass_bot: (206, 232, 248),
            preview_a: (62, 178, 236),
            preview_b: (186, 228, 248),
            preview_c: (116, 196, 232),
            swatches: [0xE0F2FE, 0x60B0F6, 0x9AD2F8, 0x8CE0C4, 0x2A74C2],
        },
        TaskbarTheme::CleanSlateCards => ThemePalette {
            accent: (132, 226, 240),
            accent_light: (224, 250, 255),
            accent_deep: (26, 86, 106),
            rim: (132, 226, 240),
            text_primary: (238, 248, 252),
            text_muted: (164, 198, 212),
            glass_top: (20, 56, 72),
            glass_bot: (14, 40, 54),
            preview_a: (18, 66, 84),
            preview_b: (42, 116, 134),
            preview_c: (12, 46, 62),
            swatches: [0x84E2F0, 0x143848, 0x2A7486, 0x0E2836, 0x0C2E3E],
        },
        _ => return None,
    };
    Some(p)
}

pub fn dock_spec(theme: TaskbarTheme) -> DockVisualSpec {
    match theme {
        // Clean & Minimal (20 authentic Windhawk-inspired animated Virtual Docks)
        TaskbarTheme::Os26LiquidGlassLight => DockVisualSpec {
            layout: DockLayoutKind::FishEyeShelf,
            shape: DockShelfShape::Pill,
            indicator: DockIndicatorStyle::CircleDot,
            art: DockArtLayer::LiquidGlassRefract,
            bg_top: (252, 254, 255),
            bg_mid: (240, 247, 255),
            bg_bot: (224, 236, 252),
            bg_alpha_bias: 0.24,
            rim_outer: (255, 255, 255, 0.88),
            rim_inner: (255, 255, 255, 0.54),
            divider: (82, 116, 164, 0.34),
            dot_active: (42, 126, 238),
            dot_idle: (84, 108, 146),
            dot_glow: (94, 168, 250),
            tip_bg: (248, 252, 255, 0.94),
            tip_border: (186, 208, 236, 0.72),
            tip_fg: (22, 30, 46, 0.98),
            ambient_animated: false,
            light_mode_paper: true,
            mechanical_lift: false,
        },
        TaskbarTheme::Os26LiquidGlassDark => DockVisualSpec {
            layout: DockLayoutKind::FishEyeShelf,
            shape: DockShelfShape::Pill,
            indicator: DockIndicatorStyle::CircleDot,
            art: DockArtLayer::LiquidGlassRefract,
            bg_top: (24, 44, 68),
            bg_mid: (14, 28, 46),
            bg_bot: (8, 18, 32),
            bg_alpha_bias: 0.20,
            rim_outer: (236, 246, 255, 0.68),
            rim_inner: (255, 255, 255, 0.26),
            divider: (218, 236, 255, 0.30),
            dot_active: (242, 248, 255),
            dot_idle: (168, 194, 224),
            dot_glow: (146, 206, 255),
            tip_bg: (12, 24, 40, 0.92),
            tip_border: (220, 238, 255, 0.44),
            tip_fg: (246, 251, 255, 0.98),
            ambient_animated: false,
            light_mode_paper: false,
            mechanical_lift: false,
        },
        TaskbarTheme::MatterTilesLight => DockVisualSpec {
            layout: DockLayoutKind::IndependentOrbs,
            shape: DockShelfShape::FloatingZero,
            indicator: DockIndicatorStyle::PillBar,
            art: DockArtLayer::GhostAura,
            bg_top: (254, 252, 255),
            bg_mid: (248, 246, 253),
            bg_bot: (238, 234, 248),
            bg_alpha_bias: 0.0,
            rim_outer: (255, 255, 255, 0.94),
            rim_inner: (255, 255, 255, 0.60),
            divider: (124, 112, 146, 0.28),
            dot_active: (224, 72, 132),
            dot_idle: (112, 102, 134),
            dot_glow: (224, 72, 132),
            tip_bg: (250, 248, 255, 0.96),
            tip_border: (216, 204, 232, 0.72),
            tip_fg: (24, 20, 34, 0.98),
            ambient_animated: false,
            light_mode_paper: true,
            mechanical_lift: false,
        },
        TaskbarTheme::MatterTilesDark => DockVisualSpec {
            layout: DockLayoutKind::IndependentOrbs,
            shape: DockShelfShape::FloatingZero,
            indicator: DockIndicatorStyle::PillBar,
            art: DockArtLayer::GhostAura,
            bg_top: (46, 36, 52),
            bg_mid: (38, 29, 44),
            bg_bot: (30, 22, 36),
            bg_alpha_bias: 0.0,
            rim_outer: (248, 224, 248, 0.34),
            rim_inner: (255, 255, 255, 0.14),
            divider: (220, 198, 230, 0.28),
            dot_active: (255, 255, 255),
            dot_idle: (214, 196, 226),
            dot_glow: (236, 108, 214),
            tip_bg: (34, 26, 40, 0.95),
            tip_border: (236, 108, 214, 0.44),
            tip_fg: (248, 244, 252, 0.98),
            ambient_animated: false,
            light_mode_paper: false,
            mechanical_lift: false,
        },
        TaskbarTheme::OneUiLightChips => DockVisualSpec {
            layout: DockLayoutKind::FishEyeShelf,
            shape: DockShelfShape::Pill,
            indicator: DockIndicatorStyle::PillBar,
            art: DockArtLayer::CleanFrost,
            bg_top: (24, 28, 38),
            bg_mid: (17, 20, 28),
            bg_bot: (12, 14, 20),
            bg_alpha_bias: 0.84,
            rim_outer: (228, 236, 250, 0.34),
            rim_inner: (255, 255, 255, 0.14),
            divider: (206, 216, 234, 0.26),
            dot_active: (168, 92, 255),
            dot_idle: (206, 214, 228),
            dot_glow: (168, 92, 255),
            tip_bg: (16, 19, 27, 0.95),
            tip_border: (168, 92, 255, 0.48),
            tip_fg: (248, 250, 255, 0.98),
            ambient_animated: false,
            light_mode_paper: false,
            mechanical_lift: false,
        },
        TaskbarTheme::OneUiDarkChips => DockVisualSpec {
            layout: DockLayoutKind::FishEyeShelf,
            shape: DockShelfShape::Pill,
            indicator: DockIndicatorStyle::PillBar,
            art: DockArtLayer::CleanFrost,
            bg_top: (248, 250, 255),
            bg_mid: (238, 242, 252),
            bg_bot: (226, 232, 246),
            bg_alpha_bias: 0.86,
            rim_outer: (255, 255, 255, 0.92),
            rim_inner: (192, 204, 226, 0.42),
            divider: (110, 124, 150, 0.30),
            dot_active: (62, 128, 242),
            dot_idle: (94, 108, 134),
            dot_glow: (76, 142, 246),
            tip_bg: (248, 250, 255, 0.96),
            tip_border: (184, 198, 226, 0.72),
            tip_fg: (24, 28, 38, 0.98),
            ambient_animated: false,
            light_mode_paper: true,
            mechanical_lift: false,
        },
        TaskbarTheme::SquircleFrosted => DockVisualSpec {
            layout: DockLayoutKind::IndependentOrbs,
            shape: DockShelfShape::FloatingZero,
            indicator: DockIndicatorStyle::PillBar,
            art: DockArtLayer::GhostAura,
            bg_top: (56, 45, 39),
            bg_mid: (44, 35, 30),
            bg_bot: (34, 26, 22),
            bg_alpha_bias: 0.0,
            rim_outer: (248, 226, 210, 0.42),
            rim_inner: (255, 242, 232, 0.16),
            divider: (224, 196, 178, 0.28),
            dot_active: (255, 236, 222),
            dot_idle: (214, 184, 166),
            dot_glow: (218, 164, 132),
            tip_bg: (40, 31, 26, 0.94),
            tip_border: (232, 204, 186, 0.46),
            tip_fg: (250, 245, 240, 0.98),
            ambient_animated: false,
            light_mode_paper: false,
            mechanical_lift: false,
        },
        TaskbarTheme::BubblesDark => DockVisualSpec {
            layout: DockLayoutKind::IndependentOrbs,
            shape: DockShelfShape::FloatingZero,
            indicator: DockIndicatorStyle::CircleDot,
            art: DockArtLayer::GhostAura,
            bg_top: (46, 48, 56),
            bg_mid: (35, 37, 44),
            bg_bot: (26, 28, 34),
            bg_alpha_bias: 0.0,
            rim_outer: (134, 168, 231, 0.62),
            rim_inner: (255, 255, 255, 0.16),
            divider: (174, 188, 214, 0.28),
            dot_active: (134, 168, 231),
            dot_idle: (186, 194, 212),
            dot_glow: (134, 168, 231),
            tip_bg: (30, 32, 38, 0.95),
            tip_border: (134, 168, 231, 0.56),
            tip_fg: (244, 247, 254, 0.98),
            ambient_animated: false,
            light_mode_paper: false,
            mechanical_lift: false,
        },
        TaskbarTheme::BubblesLight => DockVisualSpec {
            layout: DockLayoutKind::IndependentOrbs,
            shape: DockShelfShape::FloatingZero,
            indicator: DockIndicatorStyle::CircleDot,
            art: DockArtLayer::GhostAura,
            bg_top: (254, 255, 255),
            bg_mid: (246, 249, 255),
            bg_bot: (232, 238, 248),
            bg_alpha_bias: 0.0,
            rim_outer: (255, 255, 255, 0.94),
            rim_inner: (184, 206, 238, 0.50),
            divider: (106, 124, 152, 0.30),
            dot_active: (64, 134, 242),
            dot_idle: (106, 122, 148),
            dot_glow: (84, 152, 246),
            tip_bg: (250, 252, 255, 0.96),
            tip_border: (184, 204, 234, 0.72),
            tip_fg: (24, 30, 44, 0.98),
            ambient_animated: false,
            light_mode_paper: true,
            mechanical_lift: false,
        },
        TaskbarTheme::PillsDetached => DockVisualSpec {
            layout: DockLayoutKind::IndependentOrbs,
            shape: DockShelfShape::FloatingZero,
            indicator: DockIndicatorStyle::PillBar,
            art: DockArtLayer::GhostAura,
            bg_top: (44, 54, 66),
            bg_mid: (33, 41, 52),
            bg_bot: (24, 31, 40),
            bg_alpha_bias: 0.0,
            rim_outer: (218, 232, 248, 0.48),
            rim_inner: (255, 255, 255, 0.16),
            divider: (186, 204, 224, 0.28),
            dot_active: (178, 214, 248),
            dot_idle: (172, 186, 204),
            dot_glow: (166, 202, 238),
            tip_bg: (28, 36, 46, 0.95),
            tip_border: (204, 222, 240, 0.50),
            tip_fg: (244, 248, 254, 0.98),
            ambient_animated: false,
            light_mode_paper: false,
            mechanical_lift: false,
        },
        TaskbarTheme::RosePineTiles => DockVisualSpec {
            layout: DockLayoutKind::IndependentOrbs,
            shape: DockShelfShape::FloatingZero,
            indicator: DockIndicatorStyle::PillBar,
            art: DockArtLayer::GhostAura,
            bg_top: (44, 40, 66),
            bg_mid: (38, 35, 58),
            bg_bot: (31, 29, 46),
            bg_alpha_bias: 0.0,
            rim_outer: (235, 188, 186, 0.74),
            rim_inner: (196, 167, 231, 0.26),
            divider: (196, 167, 231, 0.30),
            dot_active: (235, 188, 186),
            dot_idle: (196, 167, 231),
            dot_glow: (235, 188, 186),
            tip_bg: (31, 29, 46, 0.96),
            tip_border: (235, 188, 186, 0.64),
            tip_fg: (224, 222, 244, 0.98),
            ambient_animated: false,
            light_mode_paper: false,
            mechanical_lift: false,
        },
        TaskbarTheme::CatppuccinTiles => DockVisualSpec {
            layout: DockLayoutKind::IndependentOrbs,
            shape: DockShelfShape::FloatingZero,
            indicator: DockIndicatorStyle::PillBar,
            art: DockArtLayer::GhostAura,
            bg_top: (38, 38, 56),
            bg_mid: (30, 30, 46),
            bg_bot: (24, 24, 37),
            bg_alpha_bias: 0.0,
            rim_outer: (180, 190, 254, 0.74),
            rim_inner: (203, 166, 247, 0.24),
            divider: (180, 190, 254, 0.30),
            dot_active: (180, 190, 254),
            dot_idle: (166, 173, 200),
            dot_glow: (180, 190, 254),
            tip_bg: (24, 24, 37, 0.96),
            tip_border: (180, 190, 254, 0.64),
            tip_fg: (205, 214, 244, 0.98),
            ambient_animated: false,
            light_mode_paper: false,
            mechanical_lift: false,
        },
        TaskbarTheme::LiquidGlassClear => DockVisualSpec {
            layout: DockLayoutKind::MultiIslandBar,
            shape: DockShelfShape::Pill,
            indicator: DockIndicatorStyle::PillBar,
            art: DockArtLayer::LiquidGlassRefract,
            bg_top: (172, 186, 178),
            bg_mid: (84, 96, 90),
            bg_bot: (42, 50, 46),
            bg_alpha_bias: 0.14,
            rim_outer: (255, 255, 255, 0.84),
            rim_inner: (255, 255, 255, 0.36),
            divider: (244, 252, 248, 0.32),
            dot_active: (255, 255, 255),
            dot_idle: (224, 236, 230),
            dot_glow: (224, 248, 212),
            tip_bg: (22, 28, 24, 0.90),
            tip_border: (244, 252, 248, 0.56),
            tip_fg: (250, 254, 252, 0.98),
            ambient_animated: false,
            light_mode_paper: false,
            mechanical_lift: false,
        },
        TaskbarTheme::DockLikeLight => DockVisualSpec {
            layout: DockLayoutKind::MultiIslandBar,
            shape: DockShelfShape::Pill,
            indicator: DockIndicatorStyle::CircleDot,
            art: DockArtLayer::CleanFrost,
            bg_top: (250, 252, 255),
            bg_mid: (242, 246, 252),
            bg_bot: (232, 238, 248),
            bg_alpha_bias: 0.90,
            rim_outer: (255, 255, 255, 0.94),
            rim_inner: (186, 202, 228, 0.42),
            divider: (122, 138, 166, 0.34),
            dot_active: (64, 126, 234),
            dot_idle: (116, 130, 156),
            dot_glow: (96, 148, 232),
            tip_bg: (250, 252, 255, 0.96),
            tip_border: (188, 204, 230, 0.76),
            tip_fg: (24, 30, 44, 0.98),
            ambient_animated: false,
            light_mode_paper: true,
            mechanical_lift: false,
        },
        TaskbarTheme::DockLikeDark => DockVisualSpec {
            layout: DockLayoutKind::MultiIslandBar,
            shape: DockShelfShape::Pill,
            indicator: DockIndicatorStyle::CircleDot,
            art: DockArtLayer::CleanFrost,
            bg_top: (32, 40, 58),
            bg_mid: (24, 30, 46),
            bg_bot: (18, 23, 35),
            bg_alpha_bias: 0.88,
            rim_outer: (148, 178, 224, 0.50),
            rim_inner: (255, 255, 255, 0.16),
            divider: (148, 174, 214, 0.30),
            dot_active: (116, 168, 248),
            dot_idle: (168, 184, 210),
            dot_glow: (116, 168, 248),
            tip_bg: (18, 24, 36, 0.95),
            tip_border: (142, 174, 224, 0.52),
            tip_fg: (242, 246, 254, 0.98),
            ambient_animated: false,
            light_mode_paper: false,
            mechanical_lift: false,
        },
        TaskbarTheme::BlobLight => DockVisualSpec {
            layout: DockLayoutKind::FullEdgeTaskbar,
            shape: DockShelfShape::Tile,
            indicator: DockIndicatorStyle::PillBar,
            art: DockArtLayer::CleanFrost,
            bg_top: (228, 236, 248),
            bg_mid: (220, 229, 244),
            bg_bot: (210, 221, 238),
            bg_alpha_bias: 0.90,
            rim_outer: (255, 255, 255, 0.90),
            rim_inner: (184, 198, 222, 0.36),
            divider: (132, 148, 174, 0.34),
            dot_active: (54, 126, 240),
            dot_idle: (112, 126, 148),
            dot_glow: (64, 134, 242),
            tip_bg: (252, 254, 255, 0.96),
            tip_border: (188, 202, 226, 0.76),
            tip_fg: (22, 28, 38, 0.98),
            ambient_animated: false,
            light_mode_paper: true,
            mechanical_lift: false,
        },
        TaskbarTheme::LuminosityFloat => DockVisualSpec {
            layout: DockLayoutKind::FishEyeShelf,
            shape: DockShelfShape::Pill,
            indicator: DockIndicatorStyle::PillBar,
            art: DockArtLayer::LuminosityBloom,
            bg_top: (48, 24, 52),
            bg_mid: (32, 17, 40),
            bg_bot: (20, 12, 30),
            bg_alpha_bias: 0.56,
            rim_outer: (244, 186, 218, 0.52),
            rim_inner: (255, 232, 244, 0.22),
            divider: (238, 176, 210, 0.30),
            dot_active: (255, 196, 218),
            dot_idle: (214, 176, 206),
            dot_glow: (244, 112, 156),
            tip_bg: (28, 14, 34, 0.94),
            tip_border: (244, 142, 188, 0.52),
            tip_fg: (250, 244, 252, 0.98),
            ambient_animated: false,
            light_mode_paper: false,
            mechanical_lift: false,
        },
        TaskbarTheme::BorderlessGhost => DockVisualSpec {
            layout: DockLayoutKind::IndependentOrbs,
            shape: DockShelfShape::FloatingZero,
            indicator: DockIndicatorStyle::CircleDot,
            art: DockArtLayer::GhostAura,
            bg_top: (20, 24, 34),
            bg_mid: (15, 19, 28),
            bg_bot: (11, 14, 22),
            bg_alpha_bias: 0.0,
            rim_outer: (218, 234, 252, 0.26),
            rim_inner: (255, 255, 255, 0.08),
            divider: (214, 228, 246, 0.24),
            dot_active: (255, 255, 255),
            dot_idle: (204, 220, 242),
            dot_glow: (196, 224, 255),
            tip_bg: (14, 18, 28, 0.90),
            tip_border: (214, 228, 246, 0.34),
            tip_fg: (248, 252, 255, 0.98),
            ambient_animated: false,
            light_mode_paper: false,
            mechanical_lift: false,
        },
        TaskbarTheme::AerisGlassLight => DockVisualSpec {
            layout: DockLayoutKind::FullEdgeTaskbar,
            shape: DockShelfShape::Tile,
            indicator: DockIndicatorStyle::PillBar,
            art: DockArtLayer::CleanFrost,
            bg_top: (232, 246, 255),
            bg_mid: (218, 238, 252),
            bg_bot: (204, 230, 248),
            bg_alpha_bias: 0.78,
            rim_outer: (255, 255, 255, 0.92),
            rim_inner: (146, 204, 244, 0.42),
            divider: (102, 142, 178, 0.34),
            dot_active: (46, 138, 228),
            dot_idle: (94, 128, 162),
            dot_glow: (96, 176, 246),
            tip_bg: (244, 251, 255, 0.96),
            tip_border: (152, 204, 242, 0.78),
            tip_fg: (20, 30, 44, 0.98),
            ambient_animated: false,
            light_mode_paper: true,
            mechanical_lift: false,
        },
        TaskbarTheme::CleanSlateCards => DockVisualSpec {
            layout: DockLayoutKind::FullEdgeTaskbar,
            shape: DockShelfShape::Tile,
            indicator: DockIndicatorStyle::PillBar,
            art: DockArtLayer::CleanFrost,
            bg_top: (22, 58, 74),
            bg_mid: (16, 46, 60),
            bg_bot: (12, 36, 48),
            bg_alpha_bias: 0.84,
            rim_outer: (132, 226, 240, 0.52),
            rim_inner: (186, 242, 250, 0.18),
            divider: (132, 226, 240, 0.30),
            dot_active: (132, 226, 240),
            dot_idle: (154, 192, 206),
            dot_glow: (132, 226, 240),
            tip_bg: (14, 38, 50, 0.96),
            tip_border: (132, 226, 240, 0.60),
            tip_fg: (238, 248, 252, 0.98),
            ambient_animated: false,
            light_mode_paper: false,
            mechanical_lift: false,
        },
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

unsafe fn create_radial_grad(
    rt: &ID2D1DCRenderTarget,
    center: Vector2,
    offset: Vector2,
    rx: f32,
    ry: f32,
    stops: &[D2D1_GRADIENT_STOP],
) -> Option<ID2D1RadialGradientBrush> {
    let col = rt
        .CreateGradientStopCollection(stops, D2D1_GAMMA_2_2, D2D1_EXTEND_MODE_CLAMP)
        .ok()?;
    let props = D2D1_RADIAL_GRADIENT_BRUSH_PROPERTIES {
        center,
        gradientOriginOffset: offset,
        radiusX: rx.max(1.0),
        radiusY: ry.max(1.0),
    };
    rt.CreateRadialGradientBrush(&props, None, &col).ok()
}

/// Active screen-aligned Gaussian-blurred desktop wallpaper backdrop bound during a dock paint pass.
#[derive(Clone)]
pub struct DockBackdropCtx {
    pub bmp: ID2D1Bitmap,
    pub bmp_w: f32,
    pub bmp_h: f32,
    pub mon_left: f32,
    pub mon_top: f32,
    pub mon_w: f32,
    pub mon_h: f32,
    pub canvas_left: f32,
    pub canvas_top: f32,
}

thread_local! {
    static ACTIVE_DOCK_BACKDROP: RefCell<Option<DockBackdropCtx>> = const { RefCell::new(None) };
    static ACTIVE_LIVE_HW_BLUR: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

pub struct DockBackdropGuard;

impl Drop for DockBackdropGuard {
    fn drop(&mut self) {
        ACTIVE_DOCK_BACKDROP.with(|c| {
            *c.borrow_mut() = None;
        });
        ACTIVE_LIVE_HW_BLUR.with(|c| c.set(false));
    }
}

#[allow(dead_code)]
pub fn bind_active_dock_backdrop(ctx: Option<DockBackdropCtx>) -> DockBackdropGuard {
    bind_active_dock_backdrop_ex(ctx, false)
}

pub fn bind_active_dock_backdrop_ex(
    ctx: Option<DockBackdropCtx>,
    live_hw_blur: bool,
) -> DockBackdropGuard {
    ACTIVE_DOCK_BACKDROP.with(|c| {
        *c.borrow_mut() = ctx;
    });
    ACTIVE_LIVE_HW_BLUR.with(|c| c.set(live_hw_blur));
    DockBackdropGuard
}

#[inline]
pub fn is_live_hw_blur_active() -> bool {
    ACTIVE_LIVE_HW_BLUR.with(|c| c.get())
}

/// Computes the screen-aligned source sub-rectangle inside the cached blurred monitor wallpaper,
/// optionally applying convex optical lens magnification (`lens_zoom < 1.0`, e.g. `0.86`) so
/// Apple OS26 Liquid Glass keycaps & stadium pills bend the background like real curved glass.
fn backdrop_src_rect(ctx: &DockBackdropCtx, local_r: D2D_RECT_F, lens_zoom: f32) -> D2D_RECT_F {
    let mw = ctx.mon_w.max(1.0);
    let mh = ctx.mon_h.max(1.0);
    let mut u0 = (ctx.canvas_left + local_r.left - ctx.mon_left) / mw;
    let mut v0 = (ctx.canvas_top + local_r.top - ctx.mon_top) / mh;
    let mut u1 = (ctx.canvas_left + local_r.right - ctx.mon_left) / mw;
    let mut v1 = (ctx.canvas_top + local_r.bottom - ctx.mon_top) / mh;
    if lens_zoom > 0.4 && lens_zoom < 0.999 {
        let cu = (u0 + u1) * 0.5;
        let cv = (v0 + v1) * 0.5;
        let hu = (u1 - u0) * 0.5 * lens_zoom;
        let hv = (v1 - v0) * 0.5 * lens_zoom;
        u0 = cu - hu;
        u1 = cu + hu;
        v0 = cv - hv;
        v1 = cv + hv;
    }
    let sl = (u0 * ctx.bmp_w).clamp(0.0, (ctx.bmp_w - 2.0).max(0.0));
    let st = (v0 * ctx.bmp_h).clamp(0.0, (ctx.bmp_h - 2.0).max(0.0));
    let sr = (u1 * ctx.bmp_w).clamp(sl + 1.0, ctx.bmp_w);
    let sb = (v1 * ctx.bmp_h).clamp(st + 1.0, ctx.bmp_h);
    D2D_RECT_F {
        left: sl,
        top: st,
        right: sr,
        bottom: sb,
    }
}

/// Blits the screen-aligned Gaussian-blurred desktop wallpaper clipped smoothly inside a rounded rect.
/// When live Windows 11 `Windows.UI.Composition` GPU Host Backdrop Blur is active underneath this
/// window, returns `true` directly without painting an opaque wallpaper stamp so live open windows
/// and live video wallpapers gleam cleanly through the optical glass at 120Hz.
pub unsafe fn draw_backdrop_blurred_rrect(
    rt: &ID2D1DCRenderTarget,
    d2d: Option<&ID2D1Factory>,
    r: D2D_RECT_F,
    rad: f32,
    lens_zoom: f32,
    opacity: f32,
) -> bool {
    if is_live_hw_blur_active() {
        return true;
    }
    let ctx_opt = ACTIVE_DOCK_BACKDROP.with(|c| c.borrow().clone());
    let Some(ctx) = ctx_opt else {
        return false;
    };
    let src = backdrop_src_rect(&ctx, r, lens_zoom);
    if rad > 0.5 {
        if let Some(f) = d2d {
            let rr = D2D1_ROUNDED_RECT {
                rect: r,
                radiusX: rad,
                radiusY: rad,
            };
            if let Ok(geo) = f.CreateRoundedRectangleGeometry(&rr) {
                if let Ok(base_geo) = geo.cast::<ID2D1Geometry>() {
                    if let Ok(layer) = rt.CreateLayer(None) {
                        let params = D2D1_LAYER_PARAMETERS {
                            contentBounds: r,
                            geometricMask: std::mem::ManuallyDrop::new(Some(base_geo)),
                            maskAntialiasMode: D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
                            maskTransform: Matrix3x2::identity(),
                            opacity: 1.0,
                            opacityBrush: std::mem::ManuallyDrop::new(None),
                            layerOptions: D2D1_LAYER_OPTIONS_NONE,
                        };
                        rt.PushLayer(&params, &layer);
                        rt.DrawBitmap(
                            &ctx.bmp,
                            Some(&r),
                            opacity.clamp(0.0, 1.0),
                            D2D1_BITMAP_INTERPOLATION_MODE_LINEAR,
                            Some(&src),
                        );
                        rt.PopLayer();
                        std::mem::ManuallyDrop::into_inner(params.geometricMask);
                        return true;
                    }
                }
            }
        }
    }
    rt.PushAxisAlignedClip(&r, D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
    rt.DrawBitmap(
        &ctx.bmp,
        Some(&r),
        opacity.clamp(0.0, 1.0),
        D2D1_BITMAP_INTERPOLATION_MODE_LINEAR,
        Some(&src),
    );
    rt.PopAxisAlignedClip();
    true
}

/// Blits the screen-aligned Gaussian-blurred desktop wallpaper clipped smoothly inside a circle/ellipse.
/// Yields cleanly to live Windows 11 GPU Host Backdrop Blur when active behind the window.
pub unsafe fn draw_backdrop_blurred_ellipse(
    rt: &ID2D1DCRenderTarget,
    d2d: Option<&ID2D1Factory>,
    cx: f32,
    cy: f32,
    rx: f32,
    ry: f32,
    lens_zoom: f32,
    opacity: f32,
) -> bool {
    if is_live_hw_blur_active() {
        return true;
    }
    let ctx_opt = ACTIVE_DOCK_BACKDROP.with(|c| c.borrow().clone());
    let Some(ctx) = ctx_opt else {
        return false;
    };
    let r = D2D_RECT_F {
        left: cx - rx,
        top: cy - ry,
        right: cx + rx,
        bottom: cy + ry,
    };
    let src = backdrop_src_rect(&ctx, r, lens_zoom);
    let Some(f) = d2d else {
        return false;
    };
    let ell = D2D1_ELLIPSE {
        point: Vector2 { X: cx, Y: cy },
        radiusX: rx,
        radiusY: ry,
    };
    if let Ok(geo) = f.CreateEllipseGeometry(&ell) {
        if let Ok(base_geo) = geo.cast::<ID2D1Geometry>() {
            if let Ok(layer) = rt.CreateLayer(None) {
                let params = D2D1_LAYER_PARAMETERS {
                    contentBounds: r,
                    geometricMask: std::mem::ManuallyDrop::new(Some(base_geo)),
                    maskAntialiasMode: D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
                    maskTransform: Matrix3x2::identity(),
                    opacity: 1.0,
                    opacityBrush: std::mem::ManuallyDrop::new(None),
                    layerOptions: D2D1_LAYER_OPTIONS_NONE,
                };
                rt.PushLayer(&params, &layer);
                rt.DrawBitmap(
                    &ctx.bmp,
                    Some(&r),
                    opacity.clamp(0.0, 1.0),
                    D2D1_BITMAP_INTERPOLATION_MODE_LINEAR,
                    Some(&src),
                );
                rt.PopLayer();
                std::mem::ManuallyDrop::into_inner(params.geometricMask);
                return true;
            }
        }
    }
    false
}

/// Smooth 4-pass macOS Sequoia ambient + key + contact drop shadow beneath floating glass/tiles.
pub unsafe fn draw_macos_soft_shadow(
    rt: &ID2D1DCRenderTarget,
    br: &ID2D1SolidColorBrush,
    r: D2D_RECT_F,
    rad: f32,
    is_circle: bool,
    is_light: bool,
    u: f32,
) {
    let k = if is_light { 0.62 } else { 1.0 };
    let passes = [
        (5.5 * u, 4.2 * u, 0.055 * k),
        (3.4 * u, 2.8 * u, 0.085 * k),
        (1.8 * u, 1.7 * u, 0.125 * k),
        (0.6 * u, 1.0 * u, 0.165 * k),
    ];
    let cx = (r.left + r.right) * 0.5;
    let cy = (r.top + r.bottom) * 0.5;
    let rx0 = (r.right - r.left) * 0.5;
    let ry0 = (r.bottom - r.top) * 0.5;
    for &(ex, dy, a) in &passes {
        br.SetColor(&rgba(0, 0, 0, a));
        if is_circle {
            rt.FillEllipse(
                &D2D1_ELLIPSE {
                    point: Vector2 { X: cx, Y: cy + dy },
                    radiusX: rx0 + ex * 0.72,
                    radiusY: ry0 + ex * 0.72,
                },
                br,
            );
        } else {
            let sr = D2D1_ROUNDED_RECT {
                rect: D2D_RECT_F {
                    left: r.left - ex * 0.68,
                    top: r.top - ex * 0.22 + dy,
                    right: r.right + ex * 0.68,
                    bottom: r.bottom + ex * 0.62 + dy,
                },
                radiusX: rad + ex * 0.40,
                radiusY: rad + ex * 0.40,
            };
            rt.FillRoundedRectangle(&sr, br);
        }
    }
}

/// Photorealistic 5-stop vertical Fresnel optical glass rim (bright curved top catchlight,
/// translucent crystal sides, crisp secondary bottom bounce + subtle outer ambient edge).
pub unsafe fn draw_fresnel_glass_rim(
    rt: &ID2D1DCRenderTarget,
    br: &ID2D1SolidColorBrush,
    r: D2D_RECT_F,
    rad: f32,
    is_circle: bool,
    is_light: bool,
    boost: f32,
    u: f32,
) {
    // Delicate outer 0.8u ambient dark contour so glass stays crisp over bright daylight wallpapers
    let outer_a = if is_light { 0.14 } else { 0.24 };
    br.SetColor(&rgba(0, 0, 0, outer_a));
    if is_circle {
        let cx = (r.left + r.right) * 0.5;
        let cy = (r.top + r.bottom) * 0.5;
        let rx = (r.right - r.left) * 0.5 + 0.6 * u;
        let ry = (r.bottom - r.top) * 0.5 + 0.6 * u;
        rt.DrawEllipse(
            &D2D1_ELLIPSE {
                point: Vector2 { X: cx, Y: cy },
                radiusX: rx,
                radiusY: ry,
            },
            br,
            0.85 * u,
            None,
        );
    } else {
        let out_rr = D2D1_ROUNDED_RECT {
            rect: D2D_RECT_F {
                left: r.left - 0.55 * u,
                top: r.top - 0.55 * u,
                right: r.right + 0.55 * u,
                bottom: r.bottom + 0.55 * u,
            },
            radiusX: rad + 0.55 * u,
            radiusY: rad + 0.55 * u,
        };
        rt.DrawRoundedRectangle(&out_rr, br, 0.85 * u, None);
    }

    let b = boost.clamp(0.7, 1.35);
    // Signature MacBook Tahoe / OS26 micro-prism chromatic Fresnel dispersion tucked inside curved glass bevel
    let inner_r = D2D_RECT_F {
        left: r.left + 1.05 * u,
        top: r.top + 1.05 * u,
        right: (r.right - 1.05 * u).max(r.left + 2.0 * u),
        bottom: (r.bottom - 1.05 * u).max(r.top + 2.0 * u),
    };
    let prism_stops = if is_light {
        [
            D2D1_GRADIENT_STOP { position: 0.00, color: rgba(152, 224, 255, (0.34 * b).min(0.65)) },
            D2D1_GRADIENT_STOP { position: 0.36, color: rgba(255, 255, 255, 0.0) },
            D2D1_GRADIENT_STOP { position: 0.68, color: rgba(255, 255, 255, 0.0) },
            D2D1_GRADIENT_STOP { position: 1.00, color: rgba(255, 212, 238, (0.28 * b).min(0.55)) },
        ]
    } else {
        [
            D2D1_GRADIENT_STOP { position: 0.00, color: rgba(138, 218, 255, (0.32 * b).min(0.58)) },
            D2D1_GRADIENT_STOP { position: 0.34, color: rgba(255, 255, 255, 0.0) },
            D2D1_GRADIENT_STOP { position: 0.66, color: rgba(255, 255, 255, 0.0) },
            D2D1_GRADIENT_STOP { position: 1.00, color: rgba(245, 198, 232, (0.24 * b).min(0.48)) },
        ]
    };
    if let Some(pg) = create_grad(
        rt,
        Vector2 { X: r.left, Y: r.top },
        Vector2 { X: r.right, Y: r.bottom },
        &prism_stops,
    ) {
        if is_circle {
            let cx = (r.left + r.right) * 0.5;
            let cy = (r.top + r.bottom) * 0.5;
            rt.DrawEllipse(
                &D2D1_ELLIPSE {
                    point: Vector2 { X: cx, Y: cy },
                    radiusX: ((r.right - r.left) * 0.5 - 1.05 * u).max(1.0),
                    radiusY: ((r.bottom - r.top) * 0.5 - 1.05 * u).max(1.0),
                },
                &pg,
                1.35 * u,
                None,
            );
        } else {
            let irr = D2D1_ROUNDED_RECT {
                rect: inner_r,
                radiusX: (rad - 1.05 * u).max(1.0),
                radiusY: (rad - 1.05 * u).max(1.0),
            };
            rt.DrawRoundedRectangle(&irr, &pg, 1.35 * u, None);
        }
    }

    let stops = if is_light {
        [
            D2D1_GRADIENT_STOP { position: 0.00, color: rgba(255, 255, 255, (0.98 * b).min(1.0)) },
            D2D1_GRADIENT_STOP { position: 0.20, color: rgba(255, 255, 255, (0.68 * b).min(1.0)) },
            D2D1_GRADIENT_STOP { position: 0.50, color: rgba(224, 240, 255, (0.34 * b).min(1.0)) },
            D2D1_GRADIENT_STOP { position: 0.80, color: rgba(242, 248, 255, (0.66 * b).min(1.0)) },
            D2D1_GRADIENT_STOP { position: 1.00, color: rgba(255, 255, 255, (0.92 * b).min(1.0)) },
        ]
    } else {
        [
            D2D1_GRADIENT_STOP { position: 0.00, color: rgba(255, 255, 255, (0.92 * b).min(1.0)) },
            D2D1_GRADIENT_STOP { position: 0.20, color: rgba(232, 246, 255, (0.38 * b).min(1.0)) },
            D2D1_GRADIENT_STOP { position: 0.50, color: rgba(196, 224, 248, (0.14 * b).min(1.0)) },
            D2D1_GRADIENT_STOP { position: 0.80, color: rgba(220, 238, 255, (0.34 * b).min(1.0)) },
            D2D1_GRADIENT_STOP { position: 1.00, color: rgba(248, 253, 255, (0.74 * b).min(1.0)) },
        ]
    };
    let cx = (r.left + r.right) * 0.5;
    let sw = (1.15 * b).clamp(1.0, 1.55) * u;
    if let Some(g) = create_grad(rt, Vector2 { X: cx, Y: r.top }, Vector2 { X: cx, Y: r.bottom }, &stops) {
        if is_circle {
            let cy = (r.top + r.bottom) * 0.5;
            rt.DrawEllipse(
                &D2D1_ELLIPSE {
                    point: Vector2 { X: cx, Y: cy },
                    radiusX: (r.right - r.left) * 0.5,
                    radiusY: (r.bottom - r.top) * 0.5,
                },
                &g,
                sw,
                None,
            );
        } else {
            let rr = D2D1_ROUNDED_RECT { rect: r, radiusX: rad, radiusY: rad };
            rt.DrawRoundedRectangle(&rr, &g, sw, None);
        }
    }
}

/// Photorealistic Apple OS26 Liquid Glass 3D squircle keycap:
/// 4-pass soft shadow + Gaussian-blurred convex-refracted wallpaper + radial milky top dome mist
/// + lower internal light bounce + 5-stop curved pure-white Fresnel specular rim.
#[allow(clippy::too_many_arguments)]
pub unsafe fn draw_os26_liquid_glass_keycap(
    rt: &ID2D1DCRenderTarget,
    d2d: Option<&ID2D1Factory>,
    br: &ID2D1SolidColorBrush,
    k_r: D2D_RECT_F,
    rad: f32,
    is_light: bool,
    is_hovered: bool,
    is_active: bool,
    u: f32,
) {
    let w = (k_r.right - k_r.left).max(4.0);
    let h = (k_r.bottom - k_r.top).max(4.0);
    let cx = (k_r.left + k_r.right) * 0.5;
    let k_rr = D2D1_ROUNDED_RECT { rect: k_r, radiusX: rad, radiusY: rad };

    // 1. Soft multi-pass macOS elevation drop shadow
    draw_macos_soft_shadow(rt, br, k_r, rad, false, is_light, u);

    // 2. Real Gaussian-blurred + 0.84x convex-lens-refracted desktop wallpaper clipped inside keycap!
    let has_blur = draw_backdrop_blurred_rrect(rt, d2d, k_r, rad, 0.84, 0.96);

    // 3. Ultra-clear MacBook Liquid Glass optical body glaze (crystal-transparent over live GPU blur)
    let live_hw = is_live_hw_blur_active();
    let body_stops = if is_light {
        let a_hi = if live_hw {
            if is_hovered { 0.34 } else { 0.24 }
        } else if has_blur {
            if is_hovered { 0.44 } else { 0.32 }
        } else if is_hovered {
            0.84
        } else {
            0.70
        };
        let a_lo = if live_hw {
            if is_hovered { 0.22 } else { 0.14 }
        } else if has_blur {
            if is_hovered { 0.34 } else { 0.22 }
        } else if is_hovered {
            0.74
        } else {
            0.58
        };
        [
            D2D1_GRADIENT_STOP { position: 0.0, color: rgba(255, 255, 255, a_hi) },
            D2D1_GRADIENT_STOP { position: 0.52, color: rgba(238, 248, 255, a_lo) },
            D2D1_GRADIENT_STOP { position: 1.0, color: rgba(218, 236, 255, a_hi * 0.88) },
        ]
    } else {
        let a_top = if live_hw {
            if is_hovered { 0.24 } else { 0.16 }
        } else if has_blur {
            if is_hovered { 0.28 } else { 0.19 }
        } else if is_hovered {
            0.44
        } else {
            0.32
        };
        let a_mid = if live_hw {
            if is_hovered { 0.14 } else { 0.10 }
        } else if has_blur {
            if is_hovered { 0.19 } else { 0.13 }
        } else if is_hovered {
            0.42
        } else {
            0.34
        };
        let a_bot = if live_hw {
            if is_hovered { 0.22 } else { 0.16 }
        } else if has_blur {
            if is_hovered { 0.26 } else { 0.20 }
        } else if is_hovered {
            0.50
        } else {
            0.42
        };
        [
            D2D1_GRADIENT_STOP { position: 0.0, color: rgba(196, 224, 248, a_top) },
            D2D1_GRADIENT_STOP { position: 0.45, color: rgba(16, 30, 48, a_mid) },
            D2D1_GRADIENT_STOP { position: 1.0, color: rgba(8, 18, 32, a_bot) },
        ]
    };
    if let Some(g) = create_grad(rt, Vector2 { X: cx, Y: k_r.top }, Vector2 { X: cx, Y: k_r.bottom }, &body_stops) {
        rt.FillRoundedRectangle(&k_rr, &g);
    }

    // 4. Signature Apple OS26 cloudy milky-white radial top-dome mist (true radial gradient!)
    let dome_a = if is_light {
        if is_hovered { 0.66 } else { 0.52 }
    } else if is_hovered {
        0.44
    } else {
        0.34
    };
    let dome_stops = [
        D2D1_GRADIENT_STOP { position: 0.00, color: rgba(255, 255, 255, dome_a) },
        D2D1_GRADIENT_STOP { position: 0.45, color: rgba(232, 244, 255, dome_a * 0.38) },
        D2D1_GRADIENT_STOP { position: 1.00, color: rgba(255, 255, 255, 0.0) },
    ];
    if let Some(rg) = create_radial_grad(
        rt,
        Vector2 { X: cx, Y: k_r.top + h * 0.06 },
        Vector2 { X: 0.0, Y: 0.0 },
        w * 0.64,
        h * 0.48,
        &dome_stops,
    ) {
        rt.FillRoundedRectangle(&k_rr, &rg);
    }

    // 5. Lower internal optical glass bounce glow (subtle bright curve near bottom rim)
    let bot_a = if is_light { 0.34 } else if is_hovered { 0.24 } else { 0.16 };
    let bot_stops = [
        D2D1_GRADIENT_STOP { position: 0.00, color: rgba(232, 246, 255, bot_a) },
        D2D1_GRADIENT_STOP { position: 0.55, color: rgba(210, 232, 255, bot_a * 0.28) },
        D2D1_GRADIENT_STOP { position: 1.00, color: rgba(255, 255, 255, 0.0) },
    ];
    if let Some(bg) = create_radial_grad(
        rt,
        Vector2 { X: cx, Y: k_r.bottom - h * 0.02 },
        Vector2 { X: 0.0, Y: 0.0 },
        w * 0.54,
        h * 0.24,
        &bot_stops,
    ) {
        rt.FillRoundedRectangle(&k_rr, &bg);
    }

    // 6. Curved 5-stop pure-white Fresnel specular rim
    let boost = if is_hovered { 1.24 } else if is_active { 1.12 } else { 1.0 };
    draw_fresnel_glass_rim(rt, br, k_r, rad, false, is_light, boost, u);
}

/// Renders ONE sleek unified photorealistic floating tray+clock capsule for barless
/// `IndependentOrbs` Clean & Minimal themes (matching Windhawk `Matter`, `Bubbles`, `Pills`, etc.).
#[allow(clippy::too_many_arguments)]
pub unsafe fn draw_unified_floating_tray_capsule(
    rt: &ID2D1DCRenderTarget,
    d2d: Option<&ID2D1Factory>,
    br: &ID2D1SolidColorBrush,
    rect: D2D_RECT_F,
    theme: TaskbarTheme,
    is_hovered: bool,
    u: f32,
) {
    let spec = dock_spec(theme);
    let h = (rect.bottom - rect.top).max(16.0 * u);
    let rad = match theme {
        TaskbarTheme::MatterTilesLight
        | TaskbarTheme::MatterTilesDark
        | TaskbarTheme::SquircleFrosted
        | TaskbarTheme::RosePineTiles
        | TaskbarTheme::CatppuccinTiles => (h * 0.28).clamp(8.0 * u, 15.0 * u),
        _ => (h * 0.48).min(24.0 * u),
    };
    let rr = D2D1_ROUNDED_RECT { rect, radiusX: rad, radiusY: rad };
    let cx = (rect.left + rect.right) * 0.5;

    let is_ghost = theme == TaskbarTheme::BorderlessGhost;
    if !is_ghost {
        draw_macos_soft_shadow(rt, br, rect, rad, false, spec.light_mode_paper, u);
    }

    // Real Gaussian-blurred wallpaper inside frosted / glass / ghost unified capsules!
    let blur_a = match theme {
        TaskbarTheme::MatterTilesLight
        | TaskbarTheme::MatterTilesDark
        | TaskbarTheme::RosePineTiles
        | TaskbarTheme::CatppuccinTiles => 0.35,
        TaskbarTheme::BorderlessGhost => 0.88,
        _ => 0.92,
    };
    let has_blur = draw_backdrop_blurred_rrect(rt, d2d, rect, rad, 0.96, blur_a);

    let bg_a: f32 = match theme {
        TaskbarTheme::BorderlessGhost => if has_blur { 0.30 } else { 0.44 },
        TaskbarTheme::SquircleFrosted => if has_blur { 0.48 } else { 0.74 },
        TaskbarTheme::PillsDetached => if has_blur { 0.54 } else { 0.84 },
        TaskbarTheme::BubblesDark => if has_blur { 0.58 } else { 0.86 },
        TaskbarTheme::BubblesLight => if has_blur { 0.64 } else { 0.92 },
        TaskbarTheme::MatterTilesLight => 0.96,
        TaskbarTheme::MatterTilesDark | TaskbarTheme::RosePineTiles | TaskbarTheme::CatppuccinTiles => 0.94,
        _ => 0.88,
    };
    let stops = [
        D2D1_GRADIENT_STOP { position: 0.0, color: rgba(spec.bg_top.0, spec.bg_top.1, spec.bg_top.2, (bg_a + 0.03).min(0.98)) },
        D2D1_GRADIENT_STOP { position: 1.0, color: rgba(spec.bg_bot.0, spec.bg_bot.1, spec.bg_bot.2, bg_a) },
    ];
    if let Some(g) = create_grad(rt, Vector2 { X: cx, Y: rect.top }, Vector2 { X: cx, Y: rect.bottom }, &stops) {
        rt.FillRoundedRectangle(&rr, &g);
    } else {
        br.SetColor(&rgba(spec.bg_mid.0, spec.bg_mid.1, spec.bg_mid.2, bg_a));
        rt.FillRoundedRectangle(&rr, br);
    }

    // Subtle upper sheen + themed/Fresnel border
    let top_h = D2D1_ROUNDED_RECT {
        rect: D2D_RECT_F {
            left: rect.left + 1.0 * u,
            top: rect.top + 1.0 * u,
            right: rect.right - 1.0 * u,
            bottom: rect.top + h * 0.42,
        },
        radiusX: (rad - 1.0 * u).max(2.0),
        radiusY: (rad - 1.0 * u).max(2.0),
    };
    br.SetColor(&rgba(255, 255, 255, if spec.light_mode_paper { 0.30 } else { 0.09 }));
    rt.FillRoundedRectangle(&top_h, br);

    match theme {
        TaskbarTheme::RosePineTiles | TaskbarTheme::CatppuccinTiles | TaskbarTheme::BubblesDark => {
            br.SetColor(&rgba(
                spec.rim_outer.0,
                spec.rim_outer.1,
                spec.rim_outer.2,
                if is_hovered { 0.86 } else { 0.48 },
            ));
            rt.DrawRoundedRectangle(&rr, br, 1.15 * u, None);
        }
        _ => {
            draw_fresnel_glass_rim(rt, br, rect, rad, false, spec.light_mode_paper, if is_hovered { 1.15 } else { 0.92 }, u);
        }
    }
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
    let show_plate = is_preview || cfg.w_shelf.show_plate;
    let show_art = is_preview || cfg.w_shelf.show_art;
    let show_rim = is_preview || cfg.w_shelf.show_rim;
    if !show_plate && !show_art && !show_rim {
        return;
    }
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
            DockShelfShape::Pill => {
                if cfg.tb_theme.is_clean_minimal() && !is_preview {
                    // Authentic MacBook Pro 16px squircle dock radius (cleanly encloses hardware DWMWCP_ROUND Acrylic!)
                    (cfg.corner_radius as f32 * u)
                        .clamp(12.0 * u, 16.0 * u)
                        .min(min_dim * 0.44)
                } else {
                    (cfg.corner_radius as f32 * u)
                        .clamp(12.0 * u, 26.0 * u)
                        .min(min_dim * 0.48)
                }
            }
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
        let cham_base: Option<ID2D1Geometry> = cham_geo
            .as_ref()
            .and_then(|g| g.cast::<ID2D1Geometry>().ok());
        if show_plate {
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
        }

        // Layer signature mechanical/aquatic details for specialty variants on top of PBR texture
        if show_art
            && matches!(
                spec.art,
                DockArtLayer::VacuumTubeHiFi
                    | DockArtLayer::SwissGuilloche
                    | DockArtLayer::CassetteDeck1984
                    | DockArtLayer::OceanTideFoam
                    | DockArtLayer::TranquilKoiPond
            )
        {
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

        if show_rim {
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
        }
        return;
    }

    let is_clean_glass_art = matches!(
        spec.art,
        DockArtLayer::LiquidGlassRefract
            | DockArtLayer::CleanFrost
            | DockArtLayer::LuminosityBloom
    );
    let is_os26_rail = matches!(
        cfg.tb_theme,
        TaskbarTheme::Os26LiquidGlassLight | TaskbarTheme::Os26LiquidGlassDark
    );

    if show_plate {
        if spec.layout != DockLayoutKind::FullEdgeTaskbar {
            if is_os26_rail {
                // OS26 Liquid Glass outer outline rail is razor-thin & airy (keycaps cast the 3D shadows)
                br.SetColor(&rgba(0, 0, 0, if spec.light_mode_paper { 0.05 } else { 0.12 }));
                let sh_rr = D2D1_ROUNDED_RECT {
                    rect: D2D_RECT_F {
                        left: shelf_r.left - 1.5 * u,
                        top: shelf_r.top + 1.5 * u,
                        right: shelf_r.right + 1.5 * u,
                        bottom: shelf_r.bottom + 3.0 * u,
                    },
                    radiusX: rad + 1.2 * u,
                    radiusY: rad + 1.2 * u,
                };
                rt.FillRoundedRectangle(&sh_rr, br);
            } else if is_clean_glass_art {
                draw_macos_soft_shadow(rt, br, shelf_r, rad, false, spec.light_mode_paper, u);
            } else {
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
        }

        // Photorealistic Gaussian-blurred wallpaper backdrop clipped inside Clean & Minimal glass shelves!
        let mut has_live_blur = false;
        if is_clean_glass_art && !is_os26_rail && !is_preview {
            let (lens_z, blur_op) = if cfg.tb_theme == TaskbarTheme::LiquidGlassClear {
                (0.88, 0.96)
            } else {
                (0.97, 0.92)
            };
            has_live_blur = draw_backdrop_blurred_rrect(rt, d2d, shelf_r, rad, lens_z, blur_op);
        }

        let alpha_user = (cfg.tint_alpha as f32 / 255.0).clamp(0.0, 1.0);
        let live_hw = is_live_hw_blur_active();
        let base_a = if is_os26_rail {
            if spec.light_mode_paper { 0.06 } else { 0.045 }
        } else if live_hw && has_live_blur {
            // Crystal-clear MacBook Tahoe / Sequoia live GPU glass: keep veil whisper-translucent!
            if cfg.tb_theme == TaskbarTheme::LiquidGlassClear {
                0.08
            } else if spec.bg_alpha_bias < 0.30 {
                0.10
            } else if spec.light_mode_paper {
                (spec.bg_alpha_bias * 0.42).clamp(0.24, 0.46)
            } else {
                (spec.bg_alpha_bias * 0.36).clamp(0.18, 0.40)
            }
        } else if has_live_blur {
            // Let the vibrant blurred wallpaper shine through cleanly like macOS Sequoia / Windhawk Acrylic!
            if cfg.tb_theme == TaskbarTheme::LiquidGlassClear {
                0.10
            } else if spec.bg_alpha_bias < 0.30 {
                0.13
            } else if spec.light_mode_paper {
                (spec.bg_alpha_bias * 0.52).clamp(0.32, 0.56)
            } else {
                (spec.bg_alpha_bias * 0.44).clamp(0.22, 0.48)
            }
        } else if spec.bg_alpha_bias < 0.38 {
            (spec.bg_alpha_bias * 0.82 + alpha_user * 0.18).clamp(0.14, 0.56)
        } else {
            (spec.bg_alpha_bias * 0.68 + alpha_user * 0.32).clamp(0.40, 0.96)
        };
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
    }

    if show_art {
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
        // Live optical click-ripple wave across refractive glass & clean frost shelves
        if let Some((rip_axis, rip_t)) = click_ripple {
            if rip_t < 1.0
                && matches!(
                    spec.art,
                    DockArtLayer::LiquidGlassRefract
                        | DockArtLayer::CleanFrost
                        | DockArtLayer::LuminosityBloom
                )
            {
                let fade = (1.0 - rip_t).powi(2);
                let ax = rip_axis.clamp(0.04, 0.96);
                let rc = if is_vertical {
                    Vector2 {
                        X: (shelf_r.left + shelf_r.right) * 0.5,
                        Y: shelf_r.top + sh * ax,
                    }
                } else {
                    Vector2 {
                        X: shelf_r.left + sw * ax,
                        Y: (shelf_r.top + shelf_r.bottom) * 0.5,
                    }
                };
                let rad_main = (14.0 + rip_t * 116.0) * u;
                let rad_cross = (10.0 + rip_t * 36.0) * u;
                let (rx, ry) = if is_vertical {
                    (rad_cross, rad_main)
                } else {
                    (rad_main, rad_cross)
                };
                let (cr, cg, cb) = if spec.light_mode_paper {
                    spec.dot_glow
                } else {
                    (255, 255, 255)
                };
                br.SetColor(&rgba(cr, cg, cb, 0.14 * fade));
                rt.FillEllipse(
                    &D2D1_ELLIPSE {
                        point: rc,
                        radiusX: rx * 0.62,
                        radiusY: ry * 0.62,
                    },
                    br,
                );
                br.SetColor(&rgba(cr, cg, cb, 0.52 * fade));
                rt.DrawEllipse(
                    &D2D1_ELLIPSE {
                        point: rc,
                        radiusX: rx,
                        radiusY: ry,
                    },
                    br,
                    1.5 * u,
                    None,
                );
            }
        }
        pop_shelf_clip(rt, clip_layer);
    }

    if show_rim {
        if is_clean_glass_art && spec.layout != DockLayoutKind::FullEdgeTaskbar {
            let boost = if is_os26_rail {
                0.72
            } else if cfg.tb_theme == TaskbarTheme::LiquidGlassClear {
                1.16
            } else {
                0.96
            };
            draw_fresnel_glass_rim(rt, br, shelf_r, rad, false, spec.light_mode_paper, boost, u);
        } else {
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
    }
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
        TaskbarTheme::MatterTilesLight => {
            // Zero bar — crisp velvety snow-white floating squircle tile (Matter Light)
            let tr = sz * 0.50;
            let rad = tr * 0.46;
            let tile_r = D2D_RECT_F {
                left: cx - tr,
                top: cy - tr,
                right: cx + tr,
                bottom: cy + tr,
            };
            draw_macos_soft_shadow(rt, br, tile_r, rad, false, true, u);
            if is_hovered {
                let glow_rr = D2D1_ROUNDED_RECT {
                    rect: D2D_RECT_F {
                        left: tile_r.left - 2.0 * u,
                        top: tile_r.top - 2.0 * u,
                        right: tile_r.right + 2.0 * u,
                        bottom: tile_r.bottom + 2.0 * u,
                    },
                    radiusX: rad + 1.8 * u,
                    radiusY: rad + 1.8 * u,
                };
                br.SetColor(&rgba(224, 72, 132, 0.22));
                rt.FillRoundedRectangle(&glow_rr, br);
            }
            let rr = D2D1_ROUNDED_RECT { rect: tile_r, radiusX: rad, radiusY: rad };
            let stops = [
                D2D1_GRADIENT_STOP { position: 0.0, color: rgba(255, 255, 255, 0.99) },
                D2D1_GRADIENT_STOP { position: 1.0, color: rgba(242, 239, 249, 0.97) },
            ];
            if let Some(g) = create_grad(rt, Vector2 { X: cx, Y: tile_r.top }, Vector2 { X: cx, Y: tile_r.bottom }, &stops) {
                rt.FillRoundedRectangle(&rr, &g);
            } else {
                br.SetColor(&rgba(248, 246, 253, 0.97));
                rt.FillRoundedRectangle(&rr, br);
            }
            br.SetColor(&if is_hovered {
                rgba(224, 72, 132, 0.65)
            } else {
                rgba(196, 188, 218, 0.40)
            });
            rt.DrawRoundedRectangle(&rr, br, if is_hovered { 1.3 * u } else { 0.9 * u }, None);
        }
        TaskbarTheme::MatterTilesDark => {
            // EXACT Issue #1 Image #2 (`Matter` Dark): bold velvety matte #2A2130 squircle tiles over wallpaper
            let tr = sz * 0.50;
            let rad = tr * 0.46;
            let tile_r = D2D_RECT_F {
                left: cx - tr,
                top: cy - tr,
                right: cx + tr,
                bottom: cy + tr,
            };
            draw_macos_soft_shadow(rt, br, tile_r, rad, false, false, u);
            if is_hovered {
                let glow_rr = D2D1_ROUNDED_RECT {
                    rect: D2D_RECT_F {
                        left: tile_r.left - 2.0 * u,
                        top: tile_r.top - 2.0 * u,
                        right: tile_r.right + 2.0 * u,
                        bottom: tile_r.bottom + 2.0 * u,
                    },
                    radiusX: rad + 1.8 * u,
                    radiusY: rad + 1.8 * u,
                };
                br.SetColor(&rgba(236, 108, 214, 0.22));
                rt.FillRoundedRectangle(&glow_rr, br);
            }
            let rr = D2D1_ROUNDED_RECT { rect: tile_r, radiusX: rad, radiusY: rad };
            let stops = [
                D2D1_GRADIENT_STOP { position: 0.0, color: rgba(46, 37, 52, 0.97) },
                D2D1_GRADIENT_STOP { position: 1.0, color: rgba(34, 26, 40, 0.98) },
            ];
            if let Some(g) = create_grad(rt, Vector2 { X: cx, Y: tile_r.top }, Vector2 { X: cx, Y: tile_r.bottom }, &stops) {
                rt.FillRoundedRectangle(&rr, &g);
            } else {
                br.SetColor(&rgba(40, 32, 46, 0.97));
                rt.FillRoundedRectangle(&rr, br);
            }
            // Subtle tactile matte top-edge sheen
            br.SetColor(&rgba(255, 244, 255, if is_hovered { 0.26 } else { 0.09 }));
            rt.DrawLine(
                Vector2 { X: tile_r.left + rad * 0.72, Y: tile_r.top + 0.9 * u },
                Vector2 { X: tile_r.right - rad * 0.72, Y: tile_r.top + 0.9 * u },
                br,
                0.9 * u,
                None,
            );
            br.SetColor(&if is_hovered {
                rgba(246, 184, 238, 0.72)
            } else {
                rgba(255, 255, 255, 0.08)
            });
            rt.DrawRoundedRectangle(&rr, br, if is_hovered { 1.25 * u } else { 0.8 * u }, None);
        }
        TaskbarTheme::SquircleFrosted => {
            // Windhawk Squircle: photorealistic Gaussian-blurred frosted glass squircles
            let tr = sz * 0.50;
            let rad = tr * 0.48;
            let tile_r = D2D_RECT_F {
                left: cx - tr,
                top: cy - tr,
                right: cx + tr,
                bottom: cy + tr,
            };
            draw_macos_soft_shadow(rt, br, tile_r, rad, false, false, u);
            let has_blur = draw_backdrop_blurred_rrect(rt, d2d, tile_r, rad, 0.92, 0.94);
            let live_hw = is_live_hw_blur_active();
            let rr = D2D1_ROUNDED_RECT { rect: tile_r, radiusX: rad, radiusY: rad };
            let a_t = if live_hw {
                if is_hovered { 0.34 } else { 0.24 }
            } else if has_blur {
                if is_hovered { 0.44 } else { 0.32 }
            } else if is_hovered {
                0.82
            } else {
                0.72
            };
            let stops = [
                D2D1_GRADIENT_STOP { position: 0.0, color: rgba(68, 54, 46, a_t) },
                D2D1_GRADIENT_STOP { position: 0.55, color: rgba(42, 33, 28, a_t * 0.96) },
                D2D1_GRADIENT_STOP { position: 1.0, color: rgba(28, 22, 19, a_t * 1.08) },
            ];
            if let Some(g) = create_grad(rt, Vector2 { X: cx, Y: tile_r.top }, Vector2 { X: cx, Y: tile_r.bottom }, &stops) {
                rt.FillRoundedRectangle(&rr, &g);
            }
            // Radial top glass mist
            let dome_stops = [
                D2D1_GRADIENT_STOP { position: 0.0, color: rgba(255, 242, 232, if is_hovered { 0.28 } else { 0.18 }) },
                D2D1_GRADIENT_STOP { position: 1.0, color: rgba(255, 242, 232, 0.0) },
            ];
            if let Some(rg) = create_radial_grad(
                rt,
                Vector2 { X: cx, Y: tile_r.top + tr * 0.16 },
                Vector2 { X: 0.0, Y: 0.0 },
                tr * 1.25,
                tr * 0.92,
                &dome_stops,
            ) {
                rt.FillRoundedRectangle(&rr, &rg);
            }
            draw_fresnel_glass_rim(rt, br, tile_r, rad, false, false, if is_hovered { 1.18 } else { 0.86 }, u);
        }
        TaskbarTheme::BubblesDark => {
            // Windhawk Bubbles Dark: standalone photorealistic frosted circular glass bubbles + #86A8E7 ring
            let cr = sz * 0.50;
            let ell_r = D2D_RECT_F {
                left: cx - cr,
                top: cy - cr,
                right: cx + cr,
                bottom: cy + cr,
            };
            draw_macos_soft_shadow(rt, br, ell_r, cr, true, false, u);
            if is_hovered {
                br.SetColor(&rgba(134, 168, 231, 0.26));
                rt.FillEllipse(
                    &D2D1_ELLIPSE {
                        point: Vector2 { X: cx, Y: cy },
                        radiusX: cr + 3.0 * u,
                        radiusY: cr + 3.0 * u,
                    },
                    br,
                );
            }
            let has_blur = draw_backdrop_blurred_ellipse(rt, d2d, cx, cy, cr, cr, 0.92, 0.94);
            let a_b = if is_live_hw_blur_active() {
                0.34
            } else if has_blur {
                0.46
            } else {
                0.90
            };
            let stops = [
                D2D1_GRADIENT_STOP { position: 0.0, color: rgba(54, 56, 66, a_b) },
                D2D1_GRADIENT_STOP { position: 1.0, color: rgba(24, 26, 32, (a_b + 0.10).min(0.95)) },
            ];
            if let Some(g) = create_grad(rt, Vector2 { X: cx, Y: cy - cr }, Vector2 { X: cx, Y: cy + cr }, &stops) {
                rt.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: cx, Y: cy }, radiusX: cr, radiusY: cr }, &g);
            }
            // Radial glass top highlight
            let dome_stops = [
                D2D1_GRADIENT_STOP { position: 0.0, color: rgba(232, 242, 255, if is_hovered { 0.28 } else { 0.16 }) },
                D2D1_GRADIENT_STOP { position: 1.0, color: rgba(255, 255, 255, 0.0) },
            ];
            if let Some(rg) = create_radial_grad(
                rt,
                Vector2 { X: cx, Y: cy - cr * 0.62 },
                Vector2 { X: 0.0, Y: 0.0 },
                cr * 0.92,
                cr * 0.62,
                &dome_stops,
            ) {
                rt.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: cx, Y: cy }, radiusX: cr, radiusY: cr }, &rg);
            }
            br.SetColor(&if is_hovered {
                rgba(134, 168, 231, 0.96)
            } else {
                rgba(134, 168, 231, 0.44)
            });
            rt.DrawEllipse(
                &D2D1_ELLIPSE { point: Vector2 { X: cx, Y: cy }, radiusX: cr, radiusY: cr },
                br,
                if is_hovered { 1.75 * u } else { 1.1 * u },
                None,
            );
        }
        TaskbarTheme::BubblesLight => {
            // Airy milky-white frosted circular glass bubbles with real blurred wallpaper & sky-blue focus ring
            let cr = sz * 0.50;
            let ell_r = D2D_RECT_F {
                left: cx - cr,
                top: cy - cr,
                right: cx + cr,
                bottom: cy + cr,
            };
            draw_macos_soft_shadow(rt, br, ell_r, cr, true, true, u);
            if is_hovered {
                br.SetColor(&rgba(84, 152, 246, 0.24));
                rt.FillEllipse(
                    &D2D1_ELLIPSE {
                        point: Vector2 { X: cx, Y: cy },
                        radiusX: cr + 3.0 * u,
                        radiusY: cr + 3.0 * u,
                    },
                    br,
                );
            }
            let has_blur = draw_backdrop_blurred_ellipse(rt, d2d, cx, cy, cr, cr, 0.92, 0.92);
            let a_b: f32 = if is_live_hw_blur_active() {
                0.36
            } else if has_blur {
                0.48
            } else {
                0.95
            };
            let stops = [
                D2D1_GRADIENT_STOP { position: 0.0, color: rgba(255, 255, 255, (a_b + 0.08).min(0.98)) },
                D2D1_GRADIENT_STOP { position: 1.0, color: rgba(230, 239, 252, a_b) },
            ];
            if let Some(g) = create_grad(rt, Vector2 { X: cx, Y: cy - cr }, Vector2 { X: cx, Y: cy + cr }, &stops) {
                rt.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: cx, Y: cy }, radiusX: cr, radiusY: cr }, &g);
            }
            if is_hovered {
                br.SetColor(&rgba(64, 134, 242, 0.92));
                rt.DrawEllipse(
                    &D2D1_ELLIPSE { point: Vector2 { X: cx, Y: cy }, radiusX: cr, radiusY: cr },
                    br,
                    1.65 * u,
                    None,
                );
            } else {
                draw_fresnel_glass_rim(rt, br, ell_r, cr, true, true, 0.95, u);
            }
        }
        TaskbarTheme::PillsDetached => {
            // Windhawk Pills: detached frosted dark-slate rounded capsules with Gaussian-blurred glass
            let rx = sz * 0.50;
            let ry = sz * 0.50;
            let rad = ry * 0.58;
            let pill_r = D2D_RECT_F {
                left: cx - rx,
                top: cy - ry,
                right: cx + rx,
                bottom: cy + ry,
            };
            draw_macos_soft_shadow(rt, br, pill_r, rad, false, false, u);
            let has_blur = draw_backdrop_blurred_rrect(rt, d2d, pill_r, rad, 0.94, 0.92);
            let rr = D2D1_ROUNDED_RECT { rect: pill_r, radiusX: rad, radiusY: rad };
            let a_p = if is_live_hw_blur_active() {
                0.32
            } else if has_blur {
                0.44
            } else {
                0.88
            };
            let stops = [
                D2D1_GRADIENT_STOP { position: 0.0, color: rgba(46, 56, 70, a_p) },
                D2D1_GRADIENT_STOP { position: 1.0, color: rgba(24, 32, 42, (a_p + 0.08).min(0.94)) },
            ];
            if let Some(g) = create_grad(rt, Vector2 { X: cx, Y: pill_r.top }, Vector2 { X: cx, Y: pill_r.bottom }, &stops) {
                rt.FillRoundedRectangle(&rr, &g);
            }
            draw_fresnel_glass_rim(rt, br, pill_r, rad, false, false, if is_hovered { 1.24 } else { 0.94 }, u);
        }
        TaskbarTheme::RosePineTiles | TaskbarTheme::CatppuccinTiles => {
            // Windhawk RosePine (#26233A + #EBBCBA) & Catppuccin Mocha (#1E1E2E + #B4BEFE) crisp squircles
            let is_rose = theme == TaskbarTheme::RosePineTiles;
            let tr = sz * 0.50;
            let rad = tr * 0.44;
            let tile_r = D2D_RECT_F {
                left: cx - tr,
                top: cy - tr,
                right: cx + tr,
                bottom: cy + tr,
            };
            let (bg0, bg1, acc) = if is_rose {
                ((44u8, 40u8, 66u8), (31u8, 29u8, 46u8), (235u8, 188u8, 186u8))
            } else {
                ((38, 38, 56), (24, 24, 37), (180, 190, 254))
            };
            draw_macos_soft_shadow(rt, br, tile_r, rad, false, false, u);
            if is_hovered {
                let glow_rr = D2D1_ROUNDED_RECT {
                    rect: D2D_RECT_F {
                        left: tile_r.left - 2.2 * u,
                        top: tile_r.top - 2.2 * u,
                        right: tile_r.right + 2.2 * u,
                        bottom: tile_r.bottom + 2.2 * u,
                    },
                    radiusX: rad + 2.0 * u,
                    radiusY: rad + 2.0 * u,
                };
                br.SetColor(&rgba(acc.0, acc.1, acc.2, 0.24));
                rt.FillRoundedRectangle(&glow_rr, br);
            }
            let _ = draw_backdrop_blurred_rrect(rt, d2d, tile_r, rad, 0.96, 0.35);
            let rr = D2D1_ROUNDED_RECT { rect: tile_r, radiusX: rad, radiusY: rad };
            let stops = [
                D2D1_GRADIENT_STOP { position: 0.0, color: rgba(bg0.0, bg0.1, bg0.2, 0.94) },
                D2D1_GRADIENT_STOP { position: 1.0, color: rgba(bg1.0, bg1.1, bg1.2, 0.96) },
            ];
            if let Some(g) = create_grad(rt, Vector2 { X: cx, Y: tile_r.top }, Vector2 { X: cx, Y: tile_r.bottom }, &stops) {
                rt.FillRoundedRectangle(&rr, &g);
            }
            br.SetColor(&rgba(acc.0, acc.1, acc.2, if is_hovered { 0.96 } else { 0.56 }));
            rt.DrawRoundedRectangle(&rr, br, if is_hovered { 1.55 * u } else { 1.05 * u }, None);
        }
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
            // BorderlessGhost / InvisibleGhostDock: Pure naked floating icons with whisper soft shadow & silky radial spotlight
            if is_hovered {
                let rad_g = sz * 0.82;
                let stops = [
                    D2D1_GRADIENT_STOP { position: 0.0, color: rgba(spec.dot_glow.0, spec.dot_glow.1, spec.dot_glow.2, 0.30) },
                    D2D1_GRADIENT_STOP { position: 0.45, color: rgba(spec.dot_glow.0, spec.dot_glow.1, spec.dot_glow.2, 0.12) },
                    D2D1_GRADIENT_STOP { position: 1.0, color: rgba(spec.dot_glow.0, spec.dot_glow.1, spec.dot_glow.2, 0.0) },
                ];
                if let Some(rg) = create_radial_grad(
                    rt,
                    Vector2 { X: cx, Y: cy },
                    Vector2 { X: 0.0, Y: 0.0 },
                    rad_g,
                    rad_g,
                    &stops,
                ) {
                    rt.FillEllipse(
                        &D2D1_ELLIPSE { point: Vector2 { X: cx, Y: cy }, radiusX: rad_g, radiusY: rad_g },
                        &rg,
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
        DockArtLayer::LiquidGlassRefract => {
            // Optical liquid-glass refraction: luminous upper meniscus + cursor-tracking specular prism + lower light bounce
            let dome = D2D_RECT_F {
                left: sr.left + 3.0 * u,
                top: sr.top + 0.8 * u,
                right: sr.right - 3.0 * u,
                bottom: sr.top + h * 0.44,
            };
            let dome_stops = [
                D2D1_GRADIENT_STOP { position: 0.0, color: rgba(255, 255, 255, 0.28 + 0.08 * hover_mix) },
                D2D1_GRADIENT_STOP { position: 0.55, color: rgba(236, 248, 255, 0.08) },
                D2D1_GRADIENT_STOP { position: 1.0, color: rgba(255, 255, 255, 0.0) },
            ];
            if let Some(g) = create_grad(rt, Vector2 { X: dome.left, Y: dome.top }, Vector2 { X: dome.left, Y: dome.bottom }, &dome_stops) {
                rt.FillRectangle(&dome, &g);
            }
            // Cursor-tracking optical specular refraction band
            let beam_a = 0.14 + 0.22 * hover_mix;
            if is_vertical {
                let fy = sr.top + h * focus_axis;
                let span = (h * 0.24).clamp(32.0 * u, 96.0 * u);
                let s_stops = [
                    D2D1_GRADIENT_STOP { position: 0.0, color: rgba(255, 255, 255, 0.0) },
                    D2D1_GRADIENT_STOP { position: 0.38, color: rgba(186, 232, 255, beam_a * 0.65) },
                    D2D1_GRADIENT_STOP { position: 0.50, color: rgba(255, 255, 255, beam_a) },
                    D2D1_GRADIENT_STOP { position: 0.62, color: rgba(214, 238, 255, beam_a * 0.65) },
                    D2D1_GRADIENT_STOP { position: 1.0, color: rgba(255, 255, 255, 0.0) },
                ];
                if let Some(g) = create_grad(rt, Vector2 { X: sr.left, Y: fy - span }, Vector2 { X: sr.left, Y: fy + span }, &s_stops) {
                    rt.FillRectangle(&sr, &g);
                }
            } else {
                let fx = sr.left + w * focus_axis;
                let span = (w * 0.22).clamp(38.0 * u, 118.0 * u);
                let s_stops = [
                    D2D1_GRADIENT_STOP { position: 0.0, color: rgba(255, 255, 255, 0.0) },
                    D2D1_GRADIENT_STOP { position: 0.36, color: rgba(186, 232, 255, beam_a * 0.65) },
                    D2D1_GRADIENT_STOP { position: 0.50, color: rgba(255, 255, 255, beam_a) },
                    D2D1_GRADIENT_STOP { position: 0.64, color: rgba(214, 238, 255, beam_a * 0.65) },
                    D2D1_GRADIENT_STOP { position: 1.0, color: rgba(255, 255, 255, 0.0) },
                ];
                if let Some(g) = create_grad(rt, Vector2 { X: fx - span, Y: sr.top }, Vector2 { X: fx + span, Y: sr.bottom }, &s_stops) {
                    rt.FillRectangle(&sr, &g);
                }
            }
            // Crisp upper & lower optical edge refraction highlights
            let pad_e = (h * 0.35).min(w * 0.16);
            let hl_stops = [
                D2D1_GRADIENT_STOP { position: 0.0, color: rgba(255, 255, 255, 0.0) },
                D2D1_GRADIENT_STOP { position: 0.25, color: rgba(255, 255, 255, 0.44 + 0.16 * hover_mix) },
                D2D1_GRADIENT_STOP { position: 0.50, color: rgba(255, 255, 255, 0.72 + 0.18 * hover_mix) },
                D2D1_GRADIENT_STOP { position: 0.75, color: rgba(255, 255, 255, 0.44 + 0.16 * hover_mix) },
                D2D1_GRADIENT_STOP { position: 1.0, color: rgba(255, 255, 255, 0.0) },
            ];
            if let Some(hg) = create_grad(
                rt,
                Vector2 { X: sr.left + pad_e, Y: sr.top + 1.2 * u },
                Vector2 { X: sr.right - pad_e, Y: sr.top + 1.2 * u },
                &hl_stops,
            ) {
                rt.DrawLine(
                    Vector2 { X: sr.left + pad_e, Y: sr.top + 1.2 * u },
                    Vector2 { X: sr.right - pad_e, Y: sr.top + 1.2 * u },
                    &hg,
                    1.25 * u,
                    None,
                );
            }
            let bot_stops = [
                D2D1_GRADIENT_STOP { position: 0.0, color: rgba(255, 255, 255, 0.0) },
                D2D1_GRADIENT_STOP { position: 0.50, color: rgba(224, 244, 255, 0.38 + 0.14 * hover_mix) },
                D2D1_GRADIENT_STOP { position: 1.0, color: rgba(255, 255, 255, 0.0) },
            ];
            if let Some(bg) = create_grad(
                rt,
                Vector2 { X: sr.left + pad_e, Y: sr.bottom - 1.2 * u },
                Vector2 { X: sr.right - pad_e, Y: sr.bottom - 1.2 * u },
                &bot_stops,
            ) {
                rt.DrawLine(
                    Vector2 { X: sr.left + pad_e, Y: sr.bottom - 1.2 * u },
                    Vector2 { X: sr.right - pad_e, Y: sr.bottom - 1.2 * u },
                    &bg,
                    1.0 * u,
                    None,
                );
            }
        }
        DockArtLayer::CleanFrost => {
            // Refined architectural frosted satin top reflection + silky cursor-tracking sheen
            let top_band = D2D_RECT_F {
                left: sr.left,
                top: sr.top,
                right: sr.right,
                bottom: sr.top + h * 0.40,
            };
            let stops = [
                D2D1_GRADIENT_STOP { position: 0.0, color: rgba(255, 255, 255, 0.18 + 0.06 * hover_mix) },
                D2D1_GRADIENT_STOP { position: 1.0, color: rgba(255, 255, 255, 0.0) },
            ];
            if let Some(g) = create_grad(rt, Vector2 { X: sr.left, Y: sr.top }, Vector2 { X: sr.left, Y: top_band.bottom }, &stops) {
                rt.FillRectangle(&top_band, &g);
            }
            if hover_mix > 0.02 {
                let fx = if is_vertical { sr.top + h * focus_axis } else { sr.left + w * focus_axis };
                let span = if is_vertical { (h * 0.22).clamp(28.0 * u, 88.0 * u) } else { (w * 0.20).clamp(36.0 * u, 110.0 * u) };
                let h_stops = [
                    D2D1_GRADIENT_STOP { position: 0.0, color: rgba(255, 255, 255, 0.0) },
                    D2D1_GRADIENT_STOP { position: 0.50, color: rgba(255, 255, 255, 0.16 * hover_mix) },
                    D2D1_GRADIENT_STOP { position: 1.0, color: rgba(255, 255, 255, 0.0) },
                ];
                let (p0, p1) = if is_vertical {
                    (Vector2 { X: sr.left, Y: fx - span }, Vector2 { X: sr.left, Y: fx + span })
                } else {
                    (Vector2 { X: fx - span, Y: sr.top }, Vector2 { X: fx + span, Y: sr.top })
                };
                if let Some(g) = create_grad(rt, p0, p1, &h_stops) {
                    rt.FillRectangle(&sr, &g);
                }
            }
        }
        DockArtLayer::LuminosityBloom => {
            // Silky translucent frost infused with a gliding luminous warm bloom following the cursor
            let top_band = D2D_RECT_F {
                left: sr.left,
                top: sr.top,
                right: sr.right,
                bottom: sr.top + h * 0.45,
            };
            let t_stops = [
                D2D1_GRADIENT_STOP { position: 0.0, color: rgba(255, 232, 246, 0.22) },
                D2D1_GRADIENT_STOP { position: 1.0, color: rgba(255, 232, 246, 0.0) },
            ];
            if let Some(g) = create_grad(rt, Vector2 { X: sr.left, Y: sr.top }, Vector2 { X: sr.left, Y: top_band.bottom }, &t_stops) {
                rt.FillRectangle(&top_band, &g);
            }
            let cx_p = if is_vertical { (sr.left + sr.right) * 0.5 } else { sr.left + w * focus_axis };
            let cy_p = if is_vertical { sr.top + h * focus_axis } else { (sr.top + sr.bottom) * 0.5 };
            let glow_a = 0.16 + 0.16 * hover_mix;
            br.SetColor(&rgba(244, 112, 156, glow_a * 0.55));
            rt.FillEllipse(
                &D2D1_ELLIPSE {
                    point: Vector2 { X: cx_p, Y: cy_p },
                    radiusX: (w * 0.24).clamp(44.0 * u, 130.0 * u),
                    radiusY: (h * 0.95).max(20.0 * u),
                },
                br,
            );
            br.SetColor(&rgba(255, 212, 178, glow_a * 0.48));
            rt.FillEllipse(
                &D2D1_ELLIPSE {
                    point: Vector2 { X: cx_p, Y: cy_p },
                    radiusX: (w * 0.12).clamp(24.0 * u, 68.0 * u),
                    radiusY: (h * 0.65).max(14.0 * u),
                },
                br,
            );
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
    let _ = is_flashing;
    let is_flashing = false;
    let spec = dock_spec(theme);
    if spec.layout == DockLayoutKind::IndependentOrbs {
        let slot_w = (slot_rect.right - slot_rect.left).max(0.0);
        let slot_h = (slot_rect.bottom - slot_rect.top).max(0.0);
        let is_wide_non_app = (!is_vertical && slot_w > sz * 1.32) || (is_vertical && slot_h > sz * 1.32);
        if is_wide_non_app {
            let pad_x = if slot_w <= 32.0 * u { 0.5 * u } else { 1.0 * u };
            let pad_y = if slot_h <= 32.0 * u { 1.5 * u } else { 3.0 * u };
            let pill_r = D2D_RECT_F {
                left: slot_rect.left + pad_x,
                top: slot_rect.top + pad_y,
                right: slot_rect.right - pad_x,
                bottom: slot_rect.bottom - pad_y,
            };
            let pill_h = (pill_r.bottom - pill_r.top).max(8.0 * u);
            let rad_p = (pill_h * 0.48).min(15.0 * u);
            let rr = D2D1_ROUNDED_RECT {
                rect: pill_r,
                radiusX: rad_p,
                radiusY: rad_p,
            };
            // Subtle elevation drop shadow under detached tray/clock capsules
            if theme.is_clean_minimal() && theme != TaskbarTheme::BorderlessGhost {
                let sh_rr = D2D1_ROUNDED_RECT {
                    rect: D2D_RECT_F {
                        left: pill_r.left - 1.0 * u,
                        top: pill_r.top + 1.5 * u,
                        right: pill_r.right + 1.0 * u,
                        bottom: pill_r.bottom + 2.8 * u,
                    },
                    radiusX: rad_p + 1.0 * u,
                    radiusY: rad_p + 1.0 * u,
                };
                br.SetColor(&rgba(0, 0, 0, if spec.light_mode_paper { 0.16 } else { 0.30 }));
                rt.FillRoundedRectangle(&sh_rr, br);
            }
            let is_ghost_pill = matches!(
                theme,
                TaskbarTheme::InvisibleGhostDock | TaskbarTheme::BorderlessGhost
            );
            let bg_a = if is_ghost_pill {
                if is_hovered { 0.68 } else { 0.46 }
            } else if spec.light_mode_paper {
                if is_hovered { 0.98 } else { 0.94 }
            } else if theme.is_clean_minimal() {
                if is_hovered { 0.96 } else { 0.90 }
            } else if is_hovered {
                0.88
            } else {
                0.76
            };
            br.SetColor(&rgba(spec.bg_mid.0, spec.bg_mid.1, spec.bg_mid.2, bg_a));
            rt.FillRoundedRectangle(&rr, br);
            if is_ghost_pill {
                let (rr_c, rg_c, rb_c, ra_c) = if is_hovered {
                    (spec.dot_active.0, spec.dot_active.1, spec.dot_active.2, 0.54)
                } else {
                    (210, 228, 250, 0.20)
                };
                br.SetColor(&rgba(rr_c, rg_c, rb_c, ra_c));
                rt.DrawRoundedRectangle(&rr, br, 1.0 * u, None);
            } else if spec.light_mode_paper {
                br.SetColor(&if is_hovered {
                    rgba(spec.dot_active.0, spec.dot_active.1, spec.dot_active.2, 0.70)
                } else {
                    rgba(186, 192, 212, 0.54)
                });
                rt.DrawRoundedRectangle(&rr, br, 1.05 * u, None);
            } else {
                let rim_a = if is_hovered { 0.88 } else { 0.46 };
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
            if matches!(
                theme,
                TaskbarTheme::BlobLight
                    | TaskbarTheme::AerisGlassLight
                    | TaskbarTheme::CleanSlateCards
            ) {
                let sw = (slot_rect.right - slot_rect.left).max(8.0);
                let sh = (slot_rect.bottom - slot_rect.top).max(8.0);
                let min_s = sw.min(sh);
                match theme {
                    TaskbarTheme::BlobLight => {
                        // Windhawk Blob Light: elevated pure-white rounded pill blobs over ice-gray ribbon
                        let pad = 1.5 * u;
                        let br_r = D2D_RECT_F {
                            left: slot_rect.left + pad,
                            top: slot_rect.top + pad,
                            right: slot_rect.right - pad,
                            bottom: slot_rect.bottom - pad,
                        };
                        let rad_p = (min_s * 0.44).clamp(6.0 * u, 18.0 * u);
                        let sh_rr = D2D1_ROUNDED_RECT {
                            rect: D2D_RECT_F {
                                left: br_r.left,
                                top: br_r.top + 1.2 * u,
                                right: br_r.right,
                                bottom: br_r.bottom + 2.2 * u,
                            },
                            radiusX: rad_p,
                            radiusY: rad_p,
                        };
                        br.SetColor(&rgba(48, 72, 114, if is_hovered { 0.16 } else { 0.10 }));
                        rt.FillRoundedRectangle(&sh_rr, br);
                        let rr = D2D1_ROUNDED_RECT { rect: br_r, radiusX: rad_p, radiusY: rad_p };
                        br.SetColor(&rgba(255, 255, 255, if is_hovered || is_active { 0.99 } else { 0.92 }));
                        rt.FillRoundedRectangle(&rr, br);
                        if is_flashing {
                            br.SetColor(&rgba(255, 162, 36, 0.32 * flash_pulse));
                            rt.FillRoundedRectangle(&rr, br);
                        }
                        br.SetColor(&if is_active || is_hovered {
                            rgba(64, 134, 242, 0.68)
                        } else {
                            rgba(186, 200, 224, 0.42)
                        });
                        rt.DrawRoundedRectangle(&rr, br, if is_hovered || is_active { 1.25 * u } else { 0.9 * u }, None);
                    }
                    TaskbarTheme::AerisGlassLight => {
                        // Windhawk Aeris: sharp-cornered flat icy-white translucent architectural blocks
                        let pad = 1.2 * u;
                        let br_r = D2D_RECT_F {
                            left: slot_rect.left + pad,
                            top: slot_rect.top + pad,
                            right: slot_rect.right - pad,
                            bottom: slot_rect.bottom - pad,
                        };
                        br.SetColor(&rgba(
                            255,
                            255,
                            255,
                            if is_active { 0.72 } else if is_hovered { 0.64 } else { 0.36 },
                        ));
                        rt.FillRectangle(&br_r, br);
                        if is_flashing {
                            br.SetColor(&rgba(255, 162, 36, 0.34 * flash_pulse));
                            rt.FillRectangle(&br_r, br);
                        }
                        br.SetColor(&if is_active || is_hovered {
                            rgba(64, 152, 236, 0.74)
                        } else {
                            rgba(255, 255, 255, 0.72)
                        });
                        rt.DrawRectangle(&br_r, br, 1.0 * u, None);
                        if is_active || is_hovered {
                            br.SetColor(&rgba(46, 138, 228, 0.92));
                            rt.FillRectangle(
                                &D2D_RECT_F {
                                    left: br_r.left,
                                    top: br_r.top,
                                    right: br_r.right,
                                    bottom: br_r.top + 2.2 * u,
                                },
                                br,
                            );
                        }
                    }
                    TaskbarTheme::CleanSlateCards => {
                        // Windhawk CleanSlate: dark-frosted bordered rounded cards with glowing cyan rim
                        let pad = 1.5 * u;
                        let br_r = D2D_RECT_F {
                            left: slot_rect.left + pad,
                            top: slot_rect.top + pad,
                            right: slot_rect.right - pad,
                            bottom: slot_rect.bottom - pad,
                        };
                        let rr = D2D1_ROUNDED_RECT {
                            rect: br_r,
                            radiusX: 5.5 * u,
                            radiusY: 5.5 * u,
                        };
                        br.SetColor(&rgba(
                            11,
                            31,
                            43,
                            if is_active { 0.88 } else if is_hovered { 0.84 } else { 0.68 },
                        ));
                        rt.FillRoundedRectangle(&rr, br);
                        if is_flashing {
                            br.SetColor(&rgba(255, 162, 36, 0.34 * flash_pulse));
                            rt.FillRoundedRectangle(&rr, br);
                        }
                        br.SetColor(&if is_active || is_hovered {
                            rgba(132, 226, 240, 0.94)
                        } else {
                            rgba(132, 226, 240, 0.30)
                        });
                        rt.DrawRoundedRectangle(&rr, br, if is_active || is_hovered { 1.35 * u } else { 0.95 * u }, None);
                    }
                    _ => {}
                }
                return;
            }
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

    // Per-icon animated liquid-glass squircle keycaps & One UI icon chips scaling smoothly with live fish-eye `sz`
    let slot_w = (slot_rect.right - slot_rect.left).max(0.0);
    let slot_h = (slot_rect.bottom - slot_rect.top).max(0.0);
    let is_compact_icon_slot = slot_w <= sz * 1.28 && slot_h <= sz * 1.38;
    if is_compact_icon_slot
        && matches!(
            theme,
            TaskbarTheme::Os26LiquidGlassLight
                | TaskbarTheme::Os26LiquidGlassDark
                | TaskbarTheme::OneUiLightChips
                | TaskbarTheme::OneUiDarkChips
        )
    {
        match theme {
            TaskbarTheme::Os26LiquidGlassLight | TaskbarTheme::Os26LiquidGlassDark => {
                // EXACT Issue #1 Image #1: photorealistic Apple OS26 3D frosted squircle keycap!
                let is_light = theme == TaskbarTheme::Os26LiquidGlassLight;
                let kr = sz * 0.50;
                let rad = kr * 0.56;
                let k_r = D2D_RECT_F {
                    left: icon_cx - kr,
                    top: icon_cy - kr,
                    right: icon_cx + kr,
                    bottom: icon_cy + kr,
                };
                draw_os26_liquid_glass_keycap(
                    rt,
                    d2d,
                    br,
                    k_r,
                    rad,
                    is_light,
                    is_hovered,
                    is_active,
                    u,
                );
                if is_flashing {
                    let k_rr = D2D1_ROUNDED_RECT { rect: k_r, radiusX: rad, radiusY: rad };
                    br.SetColor(&rgba(255, 196, 68, 0.92 * flash_pulse));
                    rt.DrawRoundedRectangle(&k_rr, br, 1.6 * u, None);
                }
                return;
            }
            TaskbarTheme::OneUiLightChips | TaskbarTheme::OneUiDarkChips => {
                // Windhawk One UI 8.5: bold rounded snow-white / obsidian squircle chips scaling with fish-eye!
                let is_white_chip = theme == TaskbarTheme::OneUiLightChips;
                let cr = sz * 0.50;
                let rad = cr * 0.54;
                let c_r = D2D_RECT_F {
                    left: icon_cx - cr,
                    top: icon_cy - cr,
                    right: icon_cx + cr,
                    bottom: icon_cy + cr,
                };
                draw_macos_soft_shadow(rt, br, c_r, rad, false, !is_white_chip, u);
                if is_hovered {
                    let g_rr = D2D1_ROUNDED_RECT {
                        rect: D2D_RECT_F {
                            left: c_r.left - 2.0 * u,
                            top: c_r.top - 2.0 * u,
                            right: c_r.right + 2.0 * u,
                            bottom: c_r.bottom + 2.0 * u,
                        },
                        radiusX: rad + 1.8 * u,
                        radiusY: rad + 1.8 * u,
                    };
                    br.SetColor(&rgba(spec.dot_glow.0, spec.dot_glow.1, spec.dot_glow.2, 0.28));
                    rt.FillRoundedRectangle(&g_rr, br);
                }
                let c_rr = D2D1_ROUNDED_RECT { rect: c_r, radiusX: rad, radiusY: rad };
                if is_white_chip {
                    let stops = [
                        D2D1_GRADIENT_STOP { position: 0.0, color: rgba(255, 255, 255, 0.99) },
                        D2D1_GRADIENT_STOP { position: 1.0, color: rgba(238, 242, 248, 0.96) },
                    ];
                    if let Some(g) = create_grad(rt, Vector2 { X: icon_cx, Y: c_r.top }, Vector2 { X: icon_cx, Y: c_r.bottom }, &stops) {
                        rt.FillRoundedRectangle(&c_rr, &g);
                    } else {
                        br.SetColor(&rgba(250, 252, 255, 0.98));
                        rt.FillRoundedRectangle(&c_rr, br);
                    }
                    br.SetColor(&if is_hovered || is_active {
                        rgba(168, 92, 255, 0.84)
                    } else {
                        rgba(255, 255, 255, 0.90)
                    });
                    rt.DrawRoundedRectangle(&c_rr, br, if is_hovered { 1.3 * u } else { 0.85 * u }, None);
                } else {
                    let stops = [
                        D2D1_GRADIENT_STOP { position: 0.0, color: rgba(32, 36, 48, 0.96) },
                        D2D1_GRADIENT_STOP { position: 1.0, color: rgba(18, 21, 30, 0.97) },
                    ];
                    if let Some(g) = create_grad(rt, Vector2 { X: icon_cx, Y: c_r.top }, Vector2 { X: icon_cx, Y: c_r.bottom }, &stops) {
                        rt.FillRoundedRectangle(&c_rr, &g);
                    } else {
                        br.SetColor(&rgba(24, 28, 38, 0.96));
                        rt.FillRoundedRectangle(&c_rr, br);
                    }
                    br.SetColor(&if is_hovered || is_active {
                        rgba(76, 142, 246, 0.88)
                    } else {
                        rgba(255, 255, 255, 0.16)
                    });
                    rt.DrawRoundedRectangle(&c_rr, br, if is_hovered { 1.3 * u } else { 0.85 * u }, None);
                }
                if is_flashing {
                    br.SetColor(&rgba(255, 196, 68, 0.92 * flash_pulse));
                    rt.DrawRoundedRectangle(&c_rr, br, 1.5 * u, None);
                }
                return;
            }
            _ => {}
        }
    }

    // Contact shadow + highlight + attention pulse for FishEyeShelf / MultiIslandBar
    if theme.is_clean_minimal() {
        // Ultra-clean macOS / Windhawk glass docks: NO harsh gray ovals under tray/clock/icons!
        if is_flashing {
            let f_rr = D2D1_ROUNDED_RECT {
                rect: D2D_RECT_F {
                    left: slot_rect.left - 1.5 * u,
                    top: slot_rect.top - 1.5 * u,
                    right: slot_rect.right + 1.5 * u,
                    bottom: slot_rect.bottom + 1.5 * u,
                },
                radiusX: sz * 0.28,
                radiusY: sz * 0.28,
            };
            br.SetColor(&rgba(255, 156, 36, 0.28 * flash_pulse));
            rt.FillRoundedRectangle(&f_rr, br);
            br.SetColor(&rgba(255, 196, 68, 0.90 * flash_pulse));
            rt.DrawRoundedRectangle(&f_rr, br, 1.4 * u, None);
        } else if is_hovered && is_compact_icon_slot {
            // Silky soft radial glass glow bloom behind hovered icon (no boxy square!)
            let rad_g = sz * 0.64;
            let (gr, gg, gb) = if spec.light_mode_paper {
                spec.dot_glow
            } else {
                (255, 255, 255)
            };
            let stops = [
                D2D1_GRADIENT_STOP { position: 0.0, color: rgba(gr, gg, gb, if spec.light_mode_paper { 0.20 } else { 0.22 }) },
                D2D1_GRADIENT_STOP { position: 0.55, color: rgba(gr, gg, gb, 0.07) },
                D2D1_GRADIENT_STOP { position: 1.0, color: rgba(gr, gg, gb, 0.0) },
            ];
            if let Some(rg) = create_radial_grad(
                rt,
                Vector2 { X: icon_cx, Y: icon_cy },
                Vector2 { X: 0.0, Y: 0.0 },
                rad_g,
                rad_g,
                &stops,
            ) {
                rt.FillEllipse(
                    &D2D1_ELLIPSE { point: Vector2 { X: icon_cx, Y: icon_cy }, radiusX: rad_g, radiusY: rad_g },
                    &rg,
                );
            }
        }
        return;
    }

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

/// True for Windhawk squircle-tile themes (`Matter`, `Squircle`, `Pills`, `RosePine`, `Catppuccin`)
/// whose crisp running indicator pill sits cleanly INSIDE the bottom cushion of each tile.
pub fn uses_inside_tile_pill_indicator(theme: TaskbarTheme) -> bool {
    matches!(
        theme,
        TaskbarTheme::MatterTilesLight
            | TaskbarTheme::MatterTilesDark
            | TaskbarTheme::SquircleFrosted
            | TaskbarTheme::PillsDetached
            | TaskbarTheme::RosePineTiles
            | TaskbarTheme::CatppuccinTiles
    )
}

#[allow(clippy::too_many_arguments)]
unsafe fn draw_inside_tile_pill_indicator(
    rt: &ID2D1DCRenderTarget,
    br: &ID2D1SolidColorBrush,
    spec: &DockVisualSpec,
    cx: f32,
    cy: f32,
    slot_rect: D2D_RECT_F,
    edge: MacDockEdge,
    is_active: bool,
    u: f32,
) {
    let half_w = ((slot_rect.right - slot_rect.left) * 0.5).max(8.0 * u);
    let half_h = ((slot_rect.bottom - slot_rect.top) * 0.5).max(8.0 * u);
    let tile_half = half_w.min(half_h) * 0.96;
    let is_vertical = matches!(edge, MacDockEdge::Left | MacDockEdge::Right);
    let (r, g, b) = if is_active { spec.dot_active } else { spec.dot_idle };
    br.SetColor(&rgba(r, g, b, if is_active { 0.98 } else { 0.86 }));
    if !is_vertical {
        let pw = (if is_active { tile_half * 0.56 } else { tile_half * 0.42 }).clamp(6.0 * u, 15.0 * u);
        let ph = (tile_half * 0.145).clamp(2.2 * u, 3.6 * u);
        let py = if edge == MacDockEdge::Top {
            cy - tile_half + ph * 1.45
        } else {
            cy + tile_half - ph * 1.45
        };
        let rr = D2D1_ROUNDED_RECT {
            rect: D2D_RECT_F {
                left: cx - pw * 0.5,
                top: py - ph * 0.5,
                right: cx + pw * 0.5,
                bottom: py + ph * 0.5,
            },
            radiusX: ph * 0.5,
            radiusY: ph * 0.5,
        };
        rt.FillRoundedRectangle(&rr, br);
    } else {
        let ph = (if is_active { tile_half * 0.56 } else { tile_half * 0.42 }).clamp(6.0 * u, 15.0 * u);
        let pw = (tile_half * 0.145).clamp(2.2 * u, 3.6 * u);
        let px = if edge == MacDockEdge::Left {
            cx - tile_half + pw * 1.45
        } else {
            cx + tile_half - pw * 1.45
        };
        let rr = D2D1_ROUNDED_RECT {
            rect: D2D_RECT_F {
                left: px - pw * 0.5,
                top: cy - ph * 0.5,
                right: px + pw * 0.5,
                bottom: cy + ph * 0.5,
            },
            radiusX: pw * 0.5,
            radiusY: pw * 0.5,
        };
        rt.FillRoundedRectangle(&rr, br);
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

    if uses_inside_tile_pill_indicator(theme) {
        draw_inside_tile_pill_indicator(rt, br, &spec, cx, cy, slot_rect, edge, is_active, u);
        return;
    }

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
        let edge_off = if matches!(
            theme,
            TaskbarTheme::Os26LiquidGlassLight | TaskbarTheme::Os26LiquidGlassDark
        ) {
            3.2 * u
        } else {
            5.2 * u
        };
        match edge {
            MacDockEdge::Bottom => (cx, shelf_r.bottom - edge_off),
            MacDockEdge::Top => (cx, shelf_r.top + edge_off),
            MacDockEdge::Left => (shelf_r.left + edge_off, cy),
            MacDockEdge::Right => (shelf_r.right - edge_off, cy),
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
    if !cfg.w_apps.show_separators {
        return;
    }
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
    let _ = is_flashing;
    let is_flashing = false;
    if widget_id == TaskbarWidgetId::AppDock && !cfg.w_apps.show_cards {
        return;
    }
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
    if dot_count == 0 || !cfg.w_apps.show_indicators {
        return;
    }
    let (eff_theme, mut spec) = resolve_widget_spec(cfg, TaskbarWidgetId::AppDock);
    if let Some(bc) = brand_rgb {
        spec.dot_active = bc;
        spec.dot_glow = bc;
    }
    let is_vertical = matches!(edge, MacDockEdge::Left | MacDockEdge::Right);

    if cfg.w_apps.indicator_override.is_none() && uses_inside_tile_pill_indicator(eff_theme) {
        draw_inside_tile_pill_indicator(rt, br, &spec, cx, cy, slot_rect, edge, is_active, u);
        if cfg.w_apps.show_window_count_badge && dot_count > 1 {
            let bx_b = (slot_rect.right - 7.5 * u).max(cx + 8.0 * u);
            let by_b = (slot_rect.top + 6.5 * u).min(cy - 8.0 * u);
            br.SetColor(&rgba(spec.dot_active.0, spec.dot_active.1, spec.dot_active.2, 0.92));
            rt.FillEllipse(
                &D2D1_ELLIPSE {
                    point: Vector2 { X: bx_b, Y: by_b },
                    radiusX: 4.5 * u,
                    radiusY: 4.5 * u,
                },
                br,
            );
        }
        return;
    }

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
        let edge_off = if matches!(
            eff_theme,
            TaskbarTheme::Os26LiquidGlassLight | TaskbarTheme::Os26LiquidGlassDark
        ) {
            3.2 * u
        } else {
            5.2 * u
        };
        match edge {
            MacDockEdge::Bottom => (cx, shelf_r.bottom - edge_off),
            MacDockEdge::Top => (cx, shelf_r.top + edge_off),
            MacDockEdge::Left => (shelf_r.left + edge_off, cy),
            MacDockEdge::Right => (shelf_r.right - edge_off, cy),
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
    if cfg.w_clock.is_all_elements_off() {
        return;
    }
    let (eff_theme, spec) = resolve_widget_spec(cfg, TaskbarWidgetId::Clock);
    let mut txt_pri = cfg.w_clock.custom_text_rgb.map(unpack_rgb).unwrap_or(pal.text_primary);
    let mut txt_acc = cfg.w_clock.custom_accent_rgb.map(unpack_rgb).unwrap_or(spec.dot_active);
    let mut txt_mut = pal.text_muted;

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
    let is_one_ui_clock_pill = matches!(
        eff_theme,
        TaskbarTheme::OneUiLightChips | TaskbarTheme::OneUiDarkChips
    ) && matches!(
        engine,
        DockClockEngine::StackedDigital | DockClockEngine::InheritTheme | DockClockEngine::SingleLinePill
    );
    let is_self_dark_clock = matches!(
        engine,
        DockClockEngine::NixieTubeGlow | DockClockEngine::TerminalMatrix
    ) || is_one_ui_clock_pill;
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
    if is_one_ui_clock_pill {
        // Signature One UI 8.5 vibrant gradient clock pill badge (compact vertical rhythm!)
        let mid_y = (slot_rect.top + slot_rect.bottom) * 0.5;
        let slot_h = (slot_rect.bottom - slot_rect.top).max(12.0 * u);
        let ph = (slot_h * 0.72).clamp(26.0 * u, 35.0 * u);
        let pill_r = D2D_RECT_F {
            left: slot_rect.left + 2.0 * u,
            top: mid_y - ph * 0.5,
            right: slot_rect.right - 2.0 * u,
            bottom: mid_y + ph * 0.5,
        };
        let prr = D2D1_ROUNDED_RECT {
            rect: pill_r,
            radiusX: (ph * 0.44).min(14.0 * u),
            radiusY: (ph * 0.44).min(14.0 * u),
        };
        let (c0, c1) = if eff_theme == TaskbarTheme::OneUiLightChips {
            ((242u8, 62u8, 92u8), (168u8, 68u8, 255u8))
        } else {
            ((48, 112, 232), (96, 158, 252))
        };
        let stops = [
            D2D1_GRADIENT_STOP { position: 0.0, color: rgba(c0.0, c0.1, c0.2, 0.96) },
            D2D1_GRADIENT_STOP { position: 1.0, color: rgba(c1.0, c1.1, c1.2, 0.96) },
        ];
        if let Some(g) = create_grad(rt, Vector2 { X: pill_r.left, Y: pill_r.top }, Vector2 { X: pill_r.right, Y: pill_r.bottom }, &stops) {
            rt.FillRoundedRectangle(&prr, &g);
        } else {
            br.SetColor(&rgba(c0.0, c0.1, c0.2, 0.96));
            rt.FillRoundedRectangle(&prr, br);
        }
        br.SetColor(&rgba(255, 255, 255, 0.34));
        rt.DrawRoundedRectangle(&prr, br, 0.9 * u, None);
        txt_pri = (252, 253, 255);
        txt_mut = (242, 236, 255);
    }

    let h = (slot_rect.bottom - slot_rect.top).max(12.0 * u);
    let w = (slot_rect.right - slot_rect.left).max(12.0 * u);

    if !cfg.w_clock.show_time {
        if cfg.w_clock.show_date && !date_str.is_empty() {
            br.SetColor(&rgba(txt_pri.0, txt_pri.1, txt_pri.2, 0.94));
            draw_d2d_text(rt, date_str, tf, &slot_rect, br);
        }
        return;
    }

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
                let mid_y = (slot_rect.top + slot_rect.bottom) * 0.5;
                let line_h = (h * 0.30).clamp(11.5 * u, 14.5 * u);
                let top_r = D2D_RECT_F {
                    left: slot_rect.left + 2.0 * u,
                    top: mid_y - line_h - 0.5 * u,
                    right: slot_rect.right - 2.0 * u,
                    bottom: mid_y + 1.0 * u,
                };
                let bot_r = D2D_RECT_F {
                    left: slot_rect.left + 2.0 * u,
                    top: mid_y - 0.5 * u,
                    right: slot_rect.right - 2.0 * u,
                    bottom: mid_y + line_h + 1.0 * u,
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

/// Draws recognizable miniature Windows 11 app icons inside Studio Clean & Minimal dock preview cards.
unsafe fn draw_clean_preview_glyph(
    rt: &ID2D1DCRenderTarget,
    br: &ID2D1SolidColorBrush,
    cx: f32,
    cy: f32,
    half: f32,
    idx: usize,
    u: f32,
) {
    match idx % 6 {
        0 => {
            // Crisp Windows 11 4-pane azure Start logo
            let p = half * 0.40;
            let g = half * 0.14;
            let panes = [
                (-g - p, -g - p, (76u8, 194u8, 255u8)),
                (g, -g - p, (124, 214, 255)),
                (-g - p, g, (42, 152, 240)),
                (g, g, (88, 182, 250)),
            ];
            for &(ox, oy, (r, gr, b)) in &panes {
                br.SetColor(&rgba(r, gr, b, 0.98));
                rt.FillRoundedRectangle(
                    &D2D1_ROUNDED_RECT {
                        rect: D2D_RECT_F {
                            left: cx + ox,
                            top: cy + oy,
                            right: cx + ox + p,
                            bottom: cy + oy + p,
                        },
                        radiusX: 0.7 * u,
                        radiusY: 0.7 * u,
                    },
                    br,
                );
            }
        }
        1 => {
            // Golden File Explorer folder
            br.SetColor(&rgba(232, 174, 48, 0.98));
            rt.FillRoundedRectangle(
                &D2D1_ROUNDED_RECT {
                    rect: D2D_RECT_F {
                        left: cx - half * 0.80,
                        top: cy - half * 0.66,
                        right: cx - half * 0.08,
                        bottom: cy - half * 0.24,
                    },
                    radiusX: 1.2 * u,
                    radiusY: 1.2 * u,
                },
                br,
            );
            br.SetColor(&rgba(250, 204, 72, 0.98));
            rt.FillRoundedRectangle(
                &D2D1_ROUNDED_RECT {
                    rect: D2D_RECT_F {
                        left: cx - half * 0.84,
                        top: cy - half * 0.42,
                        right: cx + half * 0.84,
                        bottom: cy + half * 0.70,
                    },
                    radiusX: 1.8 * u,
                    radiusY: 1.8 * u,
                },
                br,
            );
            br.SetColor(&rgba(255, 232, 148, 0.80));
            rt.FillRectangle(
                &D2D_RECT_F {
                    left: cx - half * 0.76,
                    top: cy - half * 0.24,
                    right: cx + half * 0.76,
                    bottom: cy - half * 0.06,
                },
                br,
            );
        }
        2 => {
            // Vivid tri-color Browser orb
            br.SetColor(&rgba(236, 76, 62, 0.98));
            rt.FillEllipse(
                &D2D1_ELLIPSE {
                    point: Vector2 { X: cx, Y: cy },
                    radiusX: half * 0.84,
                    radiusY: half * 0.84,
                },
                br,
            );
            br.SetColor(&rgba(248, 198, 52, 0.98));
            rt.FillEllipse(
                &D2D1_ELLIPSE {
                    point: Vector2 { X: cx + half * 0.28, Y: cy + half * 0.22 },
                    radiusX: half * 0.56,
                    radiusY: half * 0.56,
                },
                br,
            );
            br.SetColor(&rgba(62, 188, 102, 0.96));
            rt.FillEllipse(
                &D2D1_ELLIPSE {
                    point: Vector2 { X: cx - half * 0.30, Y: cy + half * 0.28 },
                    radiusX: half * 0.52,
                    radiusY: half * 0.52,
                },
                br,
            );
            br.SetColor(&rgba(255, 255, 255, 0.98));
            rt.FillEllipse(
                &D2D1_ELLIPSE {
                    point: Vector2 { X: cx, Y: cy },
                    radiusX: half * 0.44,
                    radiusY: half * 0.44,
                },
                br,
            );
            br.SetColor(&rgba(56, 138, 246, 0.98));
            rt.FillEllipse(
                &D2D1_ELLIPSE {
                    point: Vector2 { X: cx, Y: cy },
                    radiusX: half * 0.32,
                    radiusY: half * 0.32,
                },
                br,
            );
        }
        3 => {
            // Emerald music / Spotify circle
            br.SetColor(&rgba(44, 212, 106, 0.98));
            rt.FillEllipse(
                &D2D1_ELLIPSE {
                    point: Vector2 { X: cx, Y: cy },
                    radiusX: half * 0.82,
                    radiusY: half * 0.82,
                },
                br,
            );
            br.SetColor(&rgba(16, 36, 22, 0.92));
            for (li, dy) in [-0.24f32, 0.04, 0.30].iter().enumerate() {
                let lw = half * (0.48 - li as f32 * 0.08);
                rt.DrawLine(
                    Vector2 { X: cx - lw, Y: cy + half * *dy },
                    Vector2 { X: cx + lw, Y: cy + half * (*dy + 0.08) },
                    br,
                    1.05 * u,
                    None,
                );
            }
        }
        4 => {
            // Sky-blue Code editor badge
            br.SetColor(&rgba(46, 158, 246, 0.98));
            rt.FillRoundedRectangle(
                &D2D1_ROUNDED_RECT {
                    rect: D2D_RECT_F {
                        left: cx - half * 0.80,
                        top: cy - half * 0.80,
                        right: cx + half * 0.80,
                        bottom: cy + half * 0.80,
                    },
                    radiusX: half * 0.36,
                    radiusY: half * 0.36,
                },
                br,
            );
            br.SetColor(&rgba(242, 250, 255, 0.96));
            rt.DrawLine(
                Vector2 { X: cx - half * 0.34, Y: cy - half * 0.30 },
                Vector2 { X: cx + half * 0.30, Y: cy + half * 0.30 },
                br,
                1.4 * u,
                None,
            );
            rt.DrawLine(
                Vector2 { X: cx - half * 0.34, Y: cy + half * 0.30 },
                Vector2 { X: cx + half * 0.30, Y: cy - half * 0.30 },
                br,
                1.4 * u,
                None,
            );
            rt.DrawLine(
                Vector2 { X: cx + half * 0.32, Y: cy - half * 0.36 },
                Vector2 { X: cx + half * 0.32, Y: cy + half * 0.36 },
                br,
                1.5 * u,
                None,
            );
        }
        _ => {
            // Soft lavender-slate Settings gear / Discord tile
            br.SetColor(&rgba(116, 130, 246, 0.98));
            rt.FillRoundedRectangle(
                &D2D1_ROUNDED_RECT {
                    rect: D2D_RECT_F {
                        left: cx - half * 0.80,
                        top: cy - half * 0.80,
                        right: cx + half * 0.80,
                        bottom: cy + half * 0.80,
                    },
                    radiusX: half * 0.44,
                    radiusY: half * 0.44,
                },
                br,
            );
            br.SetColor(&rgba(248, 250, 255, 0.95));
            rt.DrawEllipse(
                &D2D1_ELLIPSE {
                    point: Vector2 { X: cx, Y: cy },
                    radiusX: half * 0.36,
                    radiusY: half * 0.36,
                },
                br,
                1.3 * u,
                None,
            );
        }
    }
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

    // Dedicated high-craft animated previews for all 20 Clean & Minimal Virtual Dock themes
    if theme.is_clean_minimal() {
        let ink = pal.text_primary;
        let ink_m = pal.text_muted;
        let sweep = (anim_phase * 0.9).sin() * 1.15;
        if cfg.clean_top_bar_enabled {
            draw_clean_top_bar_preview(rt, &br, x, y, w, h, u, theme, cfg, pal);
        }
        match spec.layout {
            DockLayoutKind::IndependentOrbs => {
                // ZERO bar behind apps! Pure Mac Dock tiles when Top Bar is ON, or tiles + unified tray pill when OFF
                let pure_mode = cfg.clean_top_bar_enabled;
                let n_icons = if pure_mode { 6usize } else { 5usize };
                let inside_ind = uses_inside_tile_pill_indicator(theme);
                let tile_step = if w < 260.0 { 24.5 } else { 27.5 };
                let pill_w = if pure_mode { 0.0 } else if w < 260.0 { 48.0 } else { 58.0 };
                let total_w = (n_icons as f32) * tile_step + if pure_mode { 0.0 } else { 10.0 + pill_w };
                let start_x = x + (w - total_w) * 0.5 + tile_step * 0.5;
                let base_y = (y + h - 22.0) * u;

                for idx in 0..n_icons {
                    let dist = ((idx as f32 - 2.0) - sweep).abs();
                    let sw_k = if dist < 1.65 {
                        let c = (FRAC_PI_2 * (dist / 1.65)).cos();
                        1.0 + 0.28 * c * c
                    } else {
                        1.0
                    };
                    let ix = (start_x + idx as f32 * tile_step) * u;
                    let iy = base_y - (sw_k - 1.0) * 8.0 * u;
                    let hov = dist < 0.65;
                    let sz = 18.0 * u * sw_k;
                    draw_standalone_orb_node(rt, d2d, &br, ix, iy, sz, theme, hov, anim_phase, idx, u);
                    let glyph_y = if inside_ind { iy - 1.4 * u } else { iy };
                    let glyph_h = if theme.uses_icon_pads() { sz * 0.30 } else { sz * 0.42 };
                    draw_clean_preview_glyph(rt, &br, ix, glyph_y, glyph_h, idx, u);
                    if idx == 1 || idx == 2 || idx == 4 {
                        let s_r = D2D_RECT_F {
                            left: ix - sz * 0.56,
                            top: iy - sz * 0.56,
                            right: ix + sz * 0.56,
                            bottom: iy + sz * 0.56,
                        };
                        let p_shelf = D2D_RECT_F {
                            left: s_r.left,
                            top: s_r.top,
                            right: s_r.right,
                            bottom: s_r.bottom + 6.5 * u,
                        };
                        draw_dock_indicator(rt, &br, theme, idx, ix, iy, s_r, p_shelf, MacDockEdge::Bottom, 1, hov, u);
                    }
                }
                if !pure_mode {
                    // ONE sleek unified floating tray & clock capsule matching Windhawk Matter / Bubbles / Pills
                    let px0 = (start_x + (n_icons as f32 - 0.5) * tile_step + 10.0) * u;
                    let pill_rect = D2D_RECT_F {
                        left: px0,
                        top: base_y - 11.5 * u,
                        right: px0 + pill_w * u,
                        bottom: base_y + 11.5 * u,
                    };
                    draw_unified_floating_tray_capsule(rt, d2d, &br, pill_rect, theme, false, u);
                    br.SetColor(&rgba(ink_m.0, ink_m.1, ink_m.2, 0.80));
                    rt.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: pill_rect.left + 10.0 * u, Y: base_y }, radiusX: 1.9 * u, radiusY: 1.9 * u }, &br);
                    rt.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: pill_rect.left + 16.5 * u, Y: base_y }, radiusX: 1.9 * u, radiusY: 1.9 * u }, &br);
                    br.SetColor(&rgba(ink.0, ink.1, ink.2, 0.92));
                    rt.FillRoundedRectangle(
                        &D2D1_ROUNDED_RECT {
                            rect: D2D_RECT_F { left: pill_rect.left + 23.0 * u, top: base_y - 3.6 * u, right: pill_rect.right - 8.0 * u, bottom: base_y - 1.0 * u },
                            radiusX: 1.0 * u,
                            radiusY: 1.0 * u,
                        },
                        &br,
                    );
                    br.SetColor(&rgba(ink_m.0, ink_m.1, ink_m.2, 0.72));
                    rt.FillRoundedRectangle(
                        &D2D1_ROUNDED_RECT {
                            rect: D2D_RECT_F { left: pill_rect.left + 26.0 * u, top: base_y + 1.2 * u, right: pill_rect.right - 8.0 * u, bottom: base_y + 3.4 * u },
                            radiusX: 0.9 * u,
                            radiusY: 0.9 * u,
                        },
                        &br,
                    );
                }
            }
            DockLayoutKind::MultiIslandBar => {
                // Authentic 2-Island Windhawk layout (`LiquidGlassClear` & `DockLike`):
                // Pure Mac Dock (Bar ON): [Start + Apps] | [Trash + Studio]
                // Classic (Bar OFF):      [Start + Apps] | [Unified Tray + Clock]
                let pure_mode = cfg.clean_top_bar_enabled;
                let iy0 = (y + h - 31.0) * u;
                let iy1 = (y + h - 6.0) * u;
                let pad_x = if pure_mode {
                    if w < 260.0 { 22.0 } else { 32.0 }
                } else if w < 260.0 {
                    10.0
                } else {
                    16.0
                };
                let right_w = if pure_mode {
                    if w < 260.0 { 38.0 } else { 44.0 }
                } else if w < 260.0 {
                    56.0
                } else {
                    68.0
                };
                let gap_i = 10.0;
                let islands = [
                    D2D_RECT_F { left: (x + pad_x) * u, top: iy0, right: (x + w - pad_x - right_w - gap_i) * u, bottom: iy1 },
                    D2D_RECT_F { left: (x + w - pad_x - right_w) * u, top: iy0, right: (x + w - pad_x) * u, bottom: iy1 },
                ];
                for isl in islands {
                    draw_dock_shelf_and_art(rt, d2d, &br, isl, cfg, pal, anim_phase, 0.72, sweep_n, false, u, true);
                }
                let cy = (iy0 + iy1) * 0.5;
                // Island 1: Start + 5 app icons with silky wave magnification
                let mid_cx = (islands[0].left + islands[0].right) * 0.5;
                let n_apps = 6usize;
                let step = ((islands[0].right - islands[0].left - 18.0 * u) / n_apps as f32).min(27.0 * u);
                for idx in 0..n_apps {
                    let rel = idx as f32 - 2.5;
                    let dist = (rel - sweep * 0.85).abs();
                    let sw_k = if dist < 1.75 {
                        let c = (FRAC_PI_2 * (dist / 1.75)).cos();
                        1.0 + 0.28 * c * c
                    } else {
                        1.0
                    };
                    let ix = mid_cx + rel * step;
                    let icy = cy - (sw_k - 1.0) * 6.5 * u;
                    draw_clean_preview_glyph(rt, &br, ix, icy, 5.8 * u * sw_k, idx, u);
                    if idx == 1 || idx == 2 || idx == 4 {
                        draw_dock_indicator(rt, &br, theme, idx, ix, icy, D2D_RECT_F::default(), islands[0], MacDockEdge::Bottom, 1, dist < 0.65, u);
                    }
                }
                if pure_mode {
                    // Pure Mac Dock Island 2: Trash + Studio emblem
                    let r_cx = (islands[1].left + islands[1].right) * 0.5;
                    br.SetColor(&rgba(ink_m.0, ink_m.1, ink_m.2, 0.88));
                    rt.FillRoundedRectangle(
                        &D2D1_ROUNDED_RECT {
                            rect: D2D_RECT_F { left: r_cx - 11.5 * u, top: cy - 4.5 * u, right: r_cx - 3.5 * u, bottom: cy + 4.5 * u },
                            radiusX: 1.8 * u,
                            radiusY: 1.8 * u,
                        },
                        &br,
                    );
                    br.SetColor(&rgba(245, 166, 66, 0.96));
                    rt.FillRoundedRectangle(
                        &D2D1_ROUNDED_RECT {
                            rect: D2D_RECT_F { left: r_cx + 3.0 * u, top: cy - 4.5 * u, right: r_cx + 7.2 * u, bottom: cy + 4.5 * u },
                            radiusX: 1.8 * u,
                            radiusY: 1.8 * u,
                        },
                        &br,
                    );
                    br.SetColor(&rgba(ink.0, ink.1, ink.2, 0.88));
                    rt.FillRoundedRectangle(
                        &D2D1_ROUNDED_RECT {
                            rect: D2D_RECT_F { left: r_cx + 8.6 * u, top: cy - 2.6 * u, right: r_cx + 11.6 * u, bottom: cy + 2.6 * u },
                            radiusX: 1.3 * u,
                            radiusY: 1.3 * u,
                        },
                        &br,
                    );
                } else {
                    // Classic Island 2: crisp dark/light ink unified tray + clock
                    br.SetColor(&rgba(ink_m.0, ink_m.1, ink_m.2, 0.82));
                    rt.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: islands[1].left + 11.0 * u, Y: cy }, radiusX: 2.0 * u, radiusY: 2.0 * u }, &br);
                    rt.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: islands[1].left + 18.0 * u, Y: cy }, radiusX: 2.0 * u, radiusY: 2.0 * u }, &br);
                    br.SetColor(&rgba(ink.0, ink.1, ink.2, 0.92));
                    rt.FillRoundedRectangle(
                        &D2D1_ROUNDED_RECT {
                            rect: D2D_RECT_F { left: islands[1].left + 25.0 * u, top: cy - 3.6 * u, right: islands[1].right - 8.0 * u, bottom: cy - 1.0 * u },
                            radiusX: 1.0 * u,
                            radiusY: 1.0 * u,
                        },
                        &br,
                    );
                    br.SetColor(&rgba(ink_m.0, ink_m.1, ink_m.2, 0.72));
                    rt.FillRoundedRectangle(
                        &D2D1_ROUNDED_RECT {
                            rect: D2D_RECT_F { left: islands[1].left + 29.0 * u, top: cy + 1.4 * u, right: islands[1].right - 8.0 * u, bottom: cy + 3.4 * u },
                            radiusX: 0.9 * u,
                            radiusY: 0.9 * u,
                        },
                        &br,
                    );
                }
            }
            DockLayoutKind::FullEdgeTaskbar => {
                // BlobLight, AerisGlassLight, CleanSlateCards: crisp full-edge architectural bar + animated icon cards
                let pure_mode = cfg.clean_top_bar_enabled;
                let shelf_r = D2D_RECT_F { left: x * u, top: (y + h - 27.0) * u, right: (x + w) * u, bottom: bot_y };
                draw_dock_shelf_and_art(rt, d2d, &br, shelf_r, cfg, pal, anim_phase, 0.70, sweep_n, false, u, true);
                let cy = (shelf_r.top + shelf_r.bottom) * 0.5;
                let card_w = if theme == TaskbarTheme::BlobLight { 24.0 * u } else { 22.0 * u };
                let card_gap = if theme == TaskbarTheme::AerisGlassLight { 2.0 * u } else { 5.0 * u };
                let n_cards = 6usize;
                let row_w = n_cards as f32 * card_w + (n_cards - 1) as f32 * card_gap;
                let center_frac = if pure_mode { 0.50 } else { 0.44 };
                let start_x = (x + w * center_frac) * u - row_w * 0.5;
                let start_x = start_x.max(shelf_r.left + 8.0 * u);

                for idx in 0..n_cards {
                    let hov = idx == 2;
                    let lift = if hov { 1.8 * u } else { 0.0 };
                    let cx_i = start_x + idx as f32 * (card_w + card_gap) + card_w * 0.5;
                    let cy_i = cy - lift;
                    let btn_r = D2D_RECT_F {
                        left: cx_i - card_w * 0.5,
                        top: cy_i - 9.5 * u,
                        right: cx_i + card_w * 0.5,
                        bottom: cy_i + 9.5 * u,
                    };
                    draw_dock_slot_fx(rt, d2d, &br, btn_r, cx_i, cy_i, 14.0 * u, shelf_r, theme, hov, hov, idx == 1 || idx == 2 || idx == 4, false, false, anim_phase, idx, u);
                    draw_clean_preview_glyph(rt, &br, cx_i, cy_i, 5.2 * u, idx, u);
                    if idx == 1 || idx == 2 || idx == 4 {
                        draw_dock_indicator(rt, &br, theme, idx, cx_i, cy_i, btn_r, shelf_r, MacDockEdge::Bottom, 1, hov, u);
                    }
                }
                if !pure_mode {
                    // Right clock + tray pill card matching theme style (hidden when Top Bar holds Clock/Tray)
                    let tr_r = D2D_RECT_F {
                        left: (x + w - 58.0) * u,
                        top: cy - 9.5 * u,
                        right: (x + w - 8.0) * u,
                        bottom: cy + 9.5 * u,
                    };
                    draw_dock_slot_fx(rt, d2d, &br, tr_r, (tr_r.left + tr_r.right) * 0.5, cy, 14.0 * u, shelf_r, theme, false, false, false, false, false, anim_phase, 9, u);
                    br.SetColor(&rgba(ink.0, ink.1, ink.2, 0.90));
                    rt.FillRoundedRectangle(
                        &D2D1_ROUNDED_RECT {
                            rect: D2D_RECT_F { left: tr_r.left + 10.0 * u, top: cy - 3.6 * u, right: tr_r.right - 8.0 * u, bottom: cy - 1.0 * u },
                            radiusX: 1.0 * u,
                            radiusY: 1.0 * u,
                        },
                        &br,
                    );
                    br.SetColor(&rgba(ink_m.0, ink_m.1, ink_m.2, 0.72));
                    rt.FillRoundedRectangle(
                        &D2D1_ROUNDED_RECT {
                            rect: D2D_RECT_F { left: tr_r.left + 14.0 * u, top: cy + 1.4 * u, right: tr_r.right - 8.0 * u, bottom: cy + 3.4 * u },
                            radiusX: 0.9 * u,
                            radiusY: 0.9 * u,
                        },
                        &br,
                    );
                }
            }
            _ => {
                // FishEyeShelf: OS26 Liquid Glass Light/Dark, One UI Light/Dark Chips, LuminosityFloat
                let pure_mode = cfg.clean_top_bar_enabled;
                let margin_x = if pure_mode {
                    if w < 260.0 { 22.0 } else { 32.0 }
                } else if w < 260.0 {
                    12.0
                } else {
                    18.0
                };
                let shelf_h = if matches!(theme, TaskbarTheme::Os26LiquidGlassLight | TaskbarTheme::Os26LiquidGlassDark) {
                    25.5
                } else {
                    26.5
                };
                let shelf_r = D2D_RECT_F {
                    left: (x + margin_x) * u,
                    top: (y + h - shelf_h - 6.5) * u,
                    right: (x + w - margin_x) * u,
                    bottom: (y + h - 6.5) * u,
                };
                draw_dock_shelf_and_art(rt, d2d, &br, shelf_r, cfg, pal, anim_phase, 0.80, sweep_n, false, u, true);

                let cy = (shelf_r.top + shelf_r.bottom) * 0.5 - if theme.uses_icon_pads() { 1.4 * u } else { 0.0 };
                let has_one_ui_pill = !pure_mode
                    && matches!(theme, TaskbarTheme::OneUiLightChips | TaskbarTheme::OneUiDarkChips);
                let right_zone = if pure_mode {
                    34.0 * u
                } else if has_one_ui_pill {
                    56.0 * u
                } else {
                    42.0 * u
                };
                let apps_l = shelf_r.left + 12.0 * u;
                let apps_r = shelf_r.right - right_zone - 8.0 * u;
                let n_icons = 6usize;
                let step = ((apps_r - apps_l) / n_icons as f32).min(29.0 * u);
                let row_cx = (apps_l + apps_r) * 0.5;
                let is_padded = theme.uses_icon_pads();
                let ind_shelf = D2D_RECT_F {
                    left: shelf_r.left,
                    top: shelf_r.top,
                    right: shelf_r.right,
                    bottom: shelf_r.bottom + if is_padded { 2.4 * u } else { 0.0 },
                };

                for idx in 0..n_icons {
                    let rel = idx as f32 - 2.5;
                    let dist = (rel - sweep * 0.85).abs();
                    let sw_k = if dist < 1.85 {
                        let c = (FRAC_PI_2 * (dist / 1.85)).cos();
                        1.0 + 0.42 * c * c
                    } else {
                        1.0
                    };
                    let ix = row_cx + rel * step + (rel - sweep * 0.85).clamp(-1.0, 1.0) * 1.4 * u;
                    let icy = cy - (sw_k - 1.0) * 7.8 * u;
                    let sz = (if is_padded { 14.5 } else { 13.0 }) * u * sw_k;
                    let hov = dist < 0.65;
                    let s_rect = D2D_RECT_F {
                        left: ix - sz * 0.5,
                        top: icy - sz * 0.5,
                        right: ix + sz * 0.5,
                        bottom: icy + sz * 0.5,
                    };
                    draw_dock_slot_fx(rt, d2d, &br, s_rect, ix, icy, sz, shelf_r, theme, hov, idx == 2, true, false, false, anim_phase, idx, u);
                    draw_clean_preview_glyph(rt, &br, ix, icy, sz * (if is_padded { 0.34 } else { 0.44 }), idx, u);
                    if idx == 0 || idx == 2 || idx == 4 {
                        draw_dock_indicator(rt, &br, theme, idx, ix, icy, s_rect, ind_shelf, MacDockEdge::Bottom, 1, hov || idx == 2, u);
                    }
                }

                // Divider + right zone (Trash + Studio in Pure Mac Dock mode; Clock/Tray when Top Bar OFF)
                let cy_shelf = (shelf_r.top + shelf_r.bottom) * 0.5;
                let div_x = shelf_r.right - right_zone;
                draw_dock_divider(rt, &br, div_x, cy_shelf, shelf_r, theme, false, u);
                if pure_mode {
                    let tx = div_x + 12.0 * u;
                    let sx_u = div_x + 25.0 * u;
                    let u_sz = 12.5 * u;
                    for (ui, ux) in [(0usize, tx), (1usize, sx_u)] {
                        let ur = D2D_RECT_F {
                            left: ux - u_sz * 0.5,
                            top: cy_shelf - u_sz * 0.5,
                            right: ux + u_sz * 0.5,
                            bottom: cy_shelf + u_sz * 0.5,
                        };
                        draw_dock_slot_fx(rt, d2d, &br, ur, ux, cy_shelf, u_sz, shelf_r, theme, false, false, true, false, false, anim_phase, 8 + ui, u);
                        if ui == 0 {
                            let tc = if theme == TaskbarTheme::OneUiLightChips { (46, 52, 64) } else { ink_m };
                            br.SetColor(&rgba(tc.0, tc.1, tc.2, 0.90));
                            rt.FillRoundedRectangle(
                                &D2D1_ROUNDED_RECT {
                                    rect: D2D_RECT_F { left: ux - 3.0 * u, top: cy_shelf - 3.4 * u, right: ux + 3.0 * u, bottom: cy_shelf + 3.4 * u },
                                    radiusX: 1.2 * u,
                                    radiusY: 1.2 * u,
                                },
                                &br,
                            );
                        } else {
                            br.SetColor(&rgba(245, 166, 66, 0.96));
                            rt.FillRoundedRectangle(
                                &D2D1_ROUNDED_RECT {
                                    rect: D2D_RECT_F { left: ux - 3.2 * u, top: cy_shelf - 3.4 * u, right: ux + 0.4 * u, bottom: cy_shelf + 3.4 * u },
                                    radiusX: 1.4 * u,
                                    radiusY: 1.4 * u,
                                },
                                &br,
                            );
                        }
                    }
                } else if has_one_ui_pill {
                    // Vibrant One UI gradient clock pill on right!
                    let pill_r = D2D_RECT_F {
                        left: div_x + 6.0 * u,
                        top: cy_shelf - 7.5 * u,
                        right: shelf_r.right - 7.0 * u,
                        bottom: cy_shelf + 7.5 * u,
                    };
                    let prr = D2D1_ROUNDED_RECT { rect: pill_r, radiusX: 7.5 * u, radiusY: 7.5 * u };
                    let (c0, c1) = if theme == TaskbarTheme::OneUiLightChips {
                        ((242u8, 62u8, 92u8), (168u8, 68u8, 255u8))
                    } else {
                        ((48, 112, 232), (96, 158, 252))
                    };
                    let stops = [
                        D2D1_GRADIENT_STOP { position: 0.0, color: rgba(c0.0, c0.1, c0.2, 0.96) },
                        D2D1_GRADIENT_STOP { position: 1.0, color: rgba(c1.0, c1.1, c1.2, 0.96) },
                    ];
                    if let Some(g) = create_grad(rt, Vector2 { X: pill_r.left, Y: pill_r.top }, Vector2 { X: pill_r.right, Y: pill_r.bottom }, &stops) {
                        rt.FillRoundedRectangle(&prr, &g);
                    }
                    br.SetColor(&rgba(255, 255, 255, 0.94));
                    rt.FillRoundedRectangle(
                        &D2D1_ROUNDED_RECT {
                            rect: D2D_RECT_F { left: pill_r.left + 8.0 * u, top: cy_shelf - 1.8 * u, right: pill_r.right - 8.0 * u, bottom: cy_shelf + 1.8 * u },
                            radiusX: 1.0 * u,
                            radiusY: 1.0 * u,
                        },
                        &br,
                    );
                } else {
                    br.SetColor(&rgba(ink.0, ink.1, ink.2, 0.86));
                    rt.FillRoundedRectangle(
                        &D2D1_ROUNDED_RECT {
                            rect: D2D_RECT_F { left: div_x + 7.0 * u, top: cy_shelf - 3.2 * u, right: shelf_r.right - 9.0 * u, bottom: cy_shelf - 0.8 * u },
                            radiusX: 1.0 * u,
                            radiusY: 1.0 * u,
                        },
                        &br,
                    );
                    br.SetColor(&rgba(ink_m.0, ink_m.1, ink_m.2, 0.72));
                    rt.FillRoundedRectangle(
                        &D2D1_ROUNDED_RECT {
                            rect: D2D_RECT_F { left: div_x + 11.0 * u, top: cy_shelf + 1.4 * u, right: shelf_r.right - 9.0 * u, bottom: cy_shelf + 3.4 * u },
                            radiusX: 0.9 * u,
                            radiusY: 0.9 * u,
                        },
                        &br,
                    );
                }
            }
        }
        return;
    }

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

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CleanTopBarItem {
    StartLogo,
    ActiveApp,
    MenuFiles,
    MenuWindows,
    MenuStudio,
    MenuDock,
    HiddenTray,
    Volume,
    Wifi,
    Battery,
    Search,
    ControlCenter,
    Clock,
}

#[derive(Clone, Debug)]
pub struct CleanTopBarSlot {
    pub item: CleanTopBarItem,
    pub rect: D2D_RECT_F,
    pub label: String,
}

pub struct CleanTopBarRenderInfo<'a> {
    pub fg_app_name: &'a str,
    pub clock_label: &'a str,
    pub vol_pct: u8,
    pub vol_muted: bool,
    pub net_connected: bool,
    pub net_wifi: bool,
    pub net_strength: u8,
    pub bat_pct: Option<u8>,
    pub bat_charging: bool,
    pub bat_saver: bool,
    pub hovered: Option<CleanTopBarItem>,
    pub pressed: Option<CleanTopBarItem>,
    pub floating: bool,
    pub at_bottom: bool,
}

/// Returns the crystal-clear Windows 11 `SetWindowCompositionAttribute` `AABBGGRR` acrylic tint
/// tuned per Clean & Minimal theme so live desktop windows & live video wallpapers shine through
/// smoothly (never black, never opaque sludge).
pub fn clean_acrylic_tint_abgr(theme: TaskbarTheme, for_top_bar: bool) -> u32 {
    let spec = dock_spec(theme);
    let pack = crate::liquid_backdrop::pack_abgr;
    if spec.light_mode_paper {
        let a = if for_top_bar { 56u8 } else { 42u8 };
        pack(244, 248, 255, a)
    } else {
        match theme {
            TaskbarTheme::LiquidGlassClear | TaskbarTheme::BorderlessGhost => {
                // Ultra-clear bright crystal tint (high transparency so live wallpaper gleams right through!)
                pack(226, 236, 250, if for_top_bar { 22 } else { 18 })
            }
            TaskbarTheme::Os26LiquidGlassDark => {
                pack(148, 178, 214, if for_top_bar { 28 } else { 22 })
            }
            TaskbarTheme::DockLikeDark => {
                pack(28, 44, 78, if for_top_bar { 48 } else { 42 })
            }
            TaskbarTheme::OneUiLightChips | TaskbarTheme::LuminosityFloat => {
                pack(32, 38, 48, if for_top_bar { 46 } else { 38 })
            }
            TaskbarTheme::MatterTilesDark
            | TaskbarTheme::RosePineTiles
            | TaskbarTheme::CatppuccinTiles => {
                pack(spec.bg_mid.0, spec.bg_mid.1, spec.bg_mid.2, 118)
            }
            _ => pack(30, 36, 48, if for_top_bar { 44 } else { 36 }),
        }
    }
}

/// Computes deterministic hit-test & paint slot geometry for the Clean & Minimal macOS / Windhawk Status Menu Bar.
pub fn layout_clean_top_bar(
    bar_r: D2D_RECT_F,
    info: &CleanTopBarRenderInfo<'_>,
    u: f32,
) -> Vec<CleanTopBarSlot> {
    let mut slots = Vec::with_capacity(14);
    let pad_x = if info.floating { 12.0 * u } else { 10.0 * u };
    let y0 = bar_r.top + 3.0 * u;
    let y1 = (bar_r.bottom - 3.0 * u).max(y0 + 12.0 * u);

    // Left wing: Start prism logo + Bold Active App Name + compact macOS-style functional menus
    let mut lx = bar_r.left + pad_x;
    let logo_w = 28.0 * u;
    slots.push(CleanTopBarSlot {
        item: CleanTopBarItem::StartLogo,
        rect: D2D_RECT_F {
            left: lx,
            top: y0,
            right: lx + logo_w,
            bottom: y1,
        },
        label: String::new(),
    });
    lx += logo_w + 4.0 * u;

    let app_short: String = if info.fg_app_name.trim().is_empty() {
        "Desktop".to_string()
    } else {
        let mut s = info.fg_app_name.trim().to_string();
        if s.chars().count() > 20 {
            s = s.chars().take(19).collect::<String>() + "…";
        }
        s
    };
    let app_w = ((app_short.chars().count() as f32) * 7.2 + 16.0).clamp(56.0, 162.0) * u;
    slots.push(CleanTopBarSlot {
        item: CleanTopBarItem::ActiveApp,
        rect: D2D_RECT_F {
            left: lx,
            top: y0,
            right: lx + app_w,
            bottom: y1,
        },
        label: app_short,
    });
    lx += app_w + 2.0 * u;

    let menu_items = [
        (CleanTopBarItem::MenuFiles, "Files", 42.0),
        (CleanTopBarItem::MenuWindows, "Windows", 64.0),
        (CleanTopBarItem::MenuStudio, "Studio", 50.0),
        (CleanTopBarItem::MenuDock, "Dock", 42.0),
    ];
    for (item, lbl, w_dip) in menu_items {
        let mw = w_dip * u;
        if lx + mw > (bar_r.left + bar_r.right) * 0.56 {
            break;
        }
        slots.push(CleanTopBarSlot {
            item,
            rect: D2D_RECT_F {
                left: lx,
                top: y0,
                right: lx + mw,
                bottom: y1,
            },
            label: lbl.to_string(),
        });
        lx += mw + 2.0 * u;
    }

    // Right wing (laid out right-to-left for exact macOS Menu Bar alignment)
    let mut rx = bar_r.right - pad_x;
    let clock_txt = if info.clock_label.trim().is_empty() {
        "12:00".to_string()
    } else {
        info.clock_label.trim().to_string()
    };
    let clock_w = ((clock_txt.chars().count() as f32) * 7.0 + 18.0).clamp(76.0, 178.0) * u;
    slots.push(CleanTopBarSlot {
        item: CleanTopBarItem::Clock,
        rect: D2D_RECT_F {
            left: rx - clock_w,
            top: y0,
            right: rx,
            bottom: y1,
        },
        label: clock_txt,
    });
    rx -= clock_w + 3.0 * u;

    let cc_w = 28.0 * u;
    slots.push(CleanTopBarSlot {
        item: CleanTopBarItem::ControlCenter,
        rect: D2D_RECT_F {
            left: rx - cc_w,
            top: y0,
            right: rx,
            bottom: y1,
        },
        label: String::new(),
    });
    rx -= cc_w + 2.0 * u;

    let search_w = 26.0 * u;
    slots.push(CleanTopBarSlot {
        item: CleanTopBarItem::Search,
        rect: D2D_RECT_F {
            left: rx - search_w,
            top: y0,
            right: rx,
            bottom: y1,
        },
        label: String::new(),
    });
    rx -= search_w + 2.0 * u;

    if let Some(pct) = info.bat_pct {
        let bat_w = 56.0 * u;
        slots.push(CleanTopBarSlot {
            item: CleanTopBarItem::Battery,
            rect: D2D_RECT_F {
                left: rx - bat_w,
                top: y0,
                right: rx,
                bottom: y1,
            },
            label: format!("{}%", pct),
        });
        rx -= bat_w + 3.0 * u;
    }

    let wifi_w = 26.0 * u;
    slots.push(CleanTopBarSlot {
        item: CleanTopBarItem::Wifi,
        rect: D2D_RECT_F {
            left: rx - wifi_w,
            top: y0,
            right: rx,
            bottom: y1,
        },
        label: String::new(),
    });
    rx -= wifi_w + 2.0 * u;

    let vol_w = 26.0 * u;
    slots.push(CleanTopBarSlot {
        item: CleanTopBarItem::Volume,
        rect: D2D_RECT_F {
            left: rx - vol_w,
            top: y0,
            right: rx,
            bottom: y1,
        },
        label: String::new(),
    });
    rx -= vol_w + 2.0 * u;

    let chev_w = 24.0 * u;
    if rx - chev_w > lx + 12.0 * u {
        slots.push(CleanTopBarSlot {
            item: CleanTopBarItem::HiddenTray,
            rect: D2D_RECT_F {
                left: rx - chev_w,
                top: y0,
                right: rx,
                bottom: y1,
            },
            label: String::new(),
        });
    }

    slots
}

/// Draws a miniature crystal-clear macOS Top Menu Bar inside Clean & Minimal Studio preview cards.
#[allow(clippy::too_many_arguments)]
unsafe fn draw_clean_top_bar_preview(
    rt: &ID2D1DCRenderTarget,
    br: &ID2D1SolidColorBrush,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    u: f32,
    theme: TaskbarTheme,
    cfg: &AppConfig,
    pal: &ThemePalette,
) {
    let spec = dock_spec(theme);
    let floating = cfg.resolved_clean_top_bar_floating();
    let at_bottom = cfg.clean_top_bar_edge == crate::config::CleanTopBarEdge::Bottom;
    let bar_h = 8.5 * u;
    let top_y = if at_bottom {
        (y + h) * u - bar_h - if floating { 2.0 * u } else { 0.0 }
    } else {
        y * u + if floating { 2.5 * u } else { 0.0 }
    };
    let margin_x = if floating { 8.0 * u } else { 0.0 };
    let r = D2D_RECT_F {
        left: x * u + margin_x,
        top: top_y,
        right: (x + w) * u - margin_x,
        bottom: top_y + bar_h,
    };
    let rad = if floating { bar_h * 0.5 } else { 0.0 };
    let bg_a = if spec.light_mode_paper {
        0.58
    } else if matches!(theme, TaskbarTheme::LiquidGlassClear | TaskbarTheme::Os26LiquidGlassDark | TaskbarTheme::BorderlessGhost) {
        0.26
    } else {
        0.54
    };
    br.SetColor(&rgba(spec.bg_top.0, spec.bg_top.1, spec.bg_top.2, bg_a));
    if floating {
        let rr = D2D1_ROUNDED_RECT { rect: r, radiusX: rad, radiusY: rad };
        rt.FillRoundedRectangle(&rr, br);
        br.SetColor(&rgba(255, 255, 255, if spec.light_mode_paper { 0.72 } else { 0.42 }));
        rt.DrawRoundedRectangle(&rr, br, 0.65 * u, None);
    } else {
        rt.FillRectangle(&r, br);
        let line_y = if at_bottom { r.top } else { r.bottom };
        br.SetColor(&rgba(255, 255, 255, if spec.light_mode_paper { 0.55 } else { 0.24 }));
        rt.DrawLine(
            Vector2 { X: r.left, Y: line_y },
            Vector2 { X: r.right, Y: line_y },
            br,
            0.65 * u,
            None,
        );
    }
    let ink = pal.text_primary;
    let cy = (r.top + r.bottom) * 0.5;
    // Left mini brand dot + app bar + menu ticks
    br.SetColor(&rgba(ink.0, ink.1, ink.2, 0.90));
    rt.FillRoundedRectangle(
        &D2D1_ROUNDED_RECT {
            rect: D2D_RECT_F { left: r.left + 6.0 * u, top: cy - 1.8 * u, right: r.left + 9.6 * u, bottom: cy + 1.8 * u },
            radiusX: 0.9 * u,
            radiusY: 0.9 * u,
        },
        br,
    );
    rt.FillRoundedRectangle(
        &D2D1_ROUNDED_RECT {
            rect: D2D_RECT_F { left: r.left + 13.0 * u, top: cy - 1.3 * u, right: r.left + 31.0 * u, bottom: cy + 1.3 * u },
            radiusX: 1.0 * u,
            radiusY: 1.0 * u,
        },
        br,
    );
    br.SetColor(&rgba(ink.0, ink.1, ink.2, 0.52));
    rt.FillRoundedRectangle(
        &D2D1_ROUNDED_RECT {
            rect: D2D_RECT_F { left: r.left + 35.0 * u, top: cy - 1.0 * u, right: r.left + 46.0 * u, bottom: cy + 1.0 * u },
            radiusX: 0.8 * u,
            radiusY: 0.8 * u,
        },
        br,
    );
    rt.FillRoundedRectangle(
        &D2D1_ROUNDED_RECT {
            rect: D2D_RECT_F { left: r.left + 50.0 * u, top: cy - 1.0 * u, right: r.left + 62.0 * u, bottom: cy + 1.0 * u },
            radiusX: 0.8 * u,
            radiusY: 0.8 * u,
        },
        br,
    );
    // Right mini status dots + clock bar (or vibrant One UI gradient clock pill!)
    br.SetColor(&rgba(ink.0, ink.1, ink.2, 0.78));
    for dx in [34.0f32, 28.0, 22.0] {
        rt.FillEllipse(
            &D2D1_ELLIPSE {
                point: Vector2 { X: r.right - dx * u, Y: cy },
                radiusX: 1.15 * u,
                radiusY: 1.15 * u,
            },
            br,
        );
    }
    if matches!(theme, TaskbarTheme::OneUiLightChips | TaskbarTheme::OneUiDarkChips) {
        let (c0, c1) = if theme == TaskbarTheme::OneUiLightChips {
            ((242u8, 62u8, 92u8), (168u8, 68u8, 255u8))
        } else {
            ((48, 112, 232), (96, 158, 252))
        };
        let pr = D2D_RECT_F {
            left: r.right - 19.0 * u,
            top: cy - 2.4 * u,
            right: r.right - 4.5 * u,
            bottom: cy + 2.4 * u,
        };
        if let Some(g) = create_grad(
            rt,
            Vector2 { X: pr.left, Y: pr.top },
            Vector2 { X: pr.right, Y: pr.bottom },
            &[
                D2D1_GRADIENT_STOP { position: 0.0, color: rgba(c0.0, c0.1, c0.2, 0.96) },
                D2D1_GRADIENT_STOP { position: 1.0, color: rgba(c1.0, c1.1, c1.2, 0.96) },
            ],
        ) {
            rt.FillRoundedRectangle(
                &D2D1_ROUNDED_RECT { rect: pr, radiusX: 2.4 * u, radiusY: 2.4 * u },
                &g,
            );
        }
    } else {
        br.SetColor(&rgba(ink.0, ink.1, ink.2, 0.92));
        rt.FillRoundedRectangle(
            &D2D1_ROUNDED_RECT {
                rect: D2D_RECT_F { left: r.right - 18.0 * u, top: cy - 1.3 * u, right: r.right - 5.0 * u, bottom: cy + 1.3 * u },
                radiusX: 1.0 * u,
                radiusY: 1.0 * u,
            },
            br,
        );
    }
}

/// Renders the photorealistic MacBook / Windhawk Liquid Glass Top (or Bottom) Status Menu Bar
/// exclusively for the 20 Clean & Minimal themes.
#[allow(clippy::too_many_arguments)]
pub unsafe fn draw_clean_top_bar(
    rt: &ID2D1DCRenderTarget,
    d2d: Option<&ID2D1Factory>,
    br: &ID2D1SolidColorBrush,
    tf_bold: &IDWriteTextFormat,
    tf_reg: &IDWriteTextFormat,
    bar_r: D2D_RECT_F,
    slots: &[CleanTopBarSlot],
    info: &CleanTopBarRenderInfo<'_>,
    cfg: &AppConfig,
    pal: &ThemePalette,
    u: f32,
) {
    let theme = cfg.tb_theme;
    let spec = dock_spec(theme);
    let is_light = spec.light_mode_paper;
    let bar_h = (bar_r.bottom - bar_r.top).max(18.0 * u);
    // Floating Top Bar uses crisp 10px macOS/Windhawk rounded corners (matches hardware DWMWCP_ROUND cleanly!)
    let rad = if info.floating {
        10.0 * u
    } else {
        0.0
    };
    let cx = (bar_r.left + bar_r.right) * 0.5;
    let live_hw = is_live_hw_blur_active();

    // 1. Soft elevation shadow beneath floating glass capsule (or whisper drop shadow beneath full bar)
    if info.floating {
        draw_macos_soft_shadow(rt, br, bar_r, rad, false, is_light, u);
    } else {
        let sh_y0 = if info.at_bottom { bar_r.top - 5.0 * u } else { bar_r.bottom };
        let sh_y1 = if info.at_bottom { bar_r.top } else { bar_r.bottom + 5.0 * u };
        let sh_stops = if info.at_bottom {
            [
                D2D1_GRADIENT_STOP { position: 0.0, color: rgba(0, 0, 0, 0.0) },
                D2D1_GRADIENT_STOP { position: 1.0, color: rgba(0, 0, 0, if is_light { 0.11 } else { 0.24 }) },
            ]
        } else {
            [
                D2D1_GRADIENT_STOP { position: 0.0, color: rgba(0, 0, 0, if is_light { 0.11 } else { 0.24 }) },
                D2D1_GRADIENT_STOP { position: 1.0, color: rgba(0, 0, 0, 0.0) },
            ]
        };
        if let Some(sg) = create_grad(rt, Vector2 { X: cx, Y: sh_y0 }, Vector2 { X: cx, Y: sh_y1 }, &sh_stops) {
            rt.FillRectangle(
                &D2D_RECT_F { left: bar_r.left, top: sh_y0, right: bar_r.right, bottom: sh_y1 },
                &sg,
            );
        }
    }

    // 2. Backdrop blur (yields transparently to real Windows 11 GPU Host Backdrop when live_hw == true)
    let has_blur = draw_backdrop_blurred_rrect(rt, d2d, bar_r, rad, 0.98, 0.95);

    // 3. Crystal-clear MacBook Liquid Glass glaze tailored to each of the 20 Clean & Minimal themes
    let is_matte_card = matches!(
        theme,
        TaskbarTheme::MatterTilesDark
            | TaskbarTheme::MatterTilesLight
            | TaskbarTheme::RosePineTiles
            | TaskbarTheme::CatppuccinTiles
    );
    let base_a: f32 = if is_matte_card {
        if live_hw { 0.72 } else { 0.88 }
    } else if matches!(theme, TaskbarTheme::LiquidGlassClear | TaskbarTheme::BorderlessGhost) {
        if live_hw { 0.09 } else if has_blur { 0.14 } else { 0.34 }
    } else if matches!(theme, TaskbarTheme::Os26LiquidGlassDark | TaskbarTheme::Os26LiquidGlassLight) {
        if is_light {
            if live_hw { 0.24 } else if has_blur { 0.34 } else { 0.76 }
        } else if live_hw {
            0.14
        } else if has_blur {
            0.22
        } else {
            0.46
        }
    } else if is_light {
        if live_hw { 0.32 } else if has_blur { 0.44 } else { 0.84 }
    } else if live_hw {
        0.22
    } else if has_blur {
        0.34
    } else {
        0.64
    };

    let (gt, gb) = if cfg.tb_match_wallpaper {
        (pal.glass_top, pal.glass_bot)
    } else {
        (spec.bg_top, spec.bg_bot)
    };
    let bar_stops = [
        D2D1_GRADIENT_STOP { position: 0.0, color: rgba(gt.0, gt.1, gt.2, (base_a + 0.04).min(0.96)) },
        D2D1_GRADIENT_STOP { position: 1.0, color: rgba(gb.0, gb.1, gb.2, base_a) },
    ];
    let bar_rr = D2D1_ROUNDED_RECT { rect: bar_r, radiusX: rad, radiusY: rad };
    if let Some(bg) = create_grad(rt, Vector2 { X: cx, Y: bar_r.top }, Vector2 { X: cx, Y: bar_r.bottom }, &bar_stops) {
        if info.floating {
            rt.FillRoundedRectangle(&bar_rr, &bg);
        } else {
            rt.FillRectangle(&bar_r, &bg);
        }
    }

    // 4. Overhead MacBook optical sky specular sheen across upper 46% of the glass bar
    let sheen_a = if is_light { 0.34 } else { 0.14 };
    let sheen_stops = [
        D2D1_GRADIENT_STOP { position: 0.0, color: rgba(255, 255, 255, sheen_a) },
        D2D1_GRADIENT_STOP { position: 1.0, color: rgba(255, 255, 255, 0.0) },
    ];
    let sheen_r = D2D_RECT_F {
        left: bar_r.left,
        top: bar_r.top,
        right: bar_r.right,
        bottom: bar_r.top + bar_h * 0.48,
    };
    if let Some(shg) = create_grad(rt, Vector2 { X: cx, Y: sheen_r.top }, Vector2 { X: cx, Y: sheen_r.bottom }, &sheen_stops) {
        if info.floating {
            let srr = D2D1_ROUNDED_RECT { rect: sheen_r, radiusX: rad, radiusY: rad * 0.6 };
            rt.FillRoundedRectangle(&srr, &shg);
        } else {
            rt.FillRectangle(&sheen_r, &shg);
        }
    }

    // 5. Fresnel rim (for Floating Glass Pill) or razor-thin luminous hairline divider (for Full-Width Bar)
    if info.floating {
        draw_fresnel_glass_rim(rt, br, bar_r, rad, false, is_light, 1.04, u);
    } else {
        let edge_y = if info.at_bottom { bar_r.top + 0.5 * u } else { bar_r.bottom - 0.5 * u };
        let line_stops = if is_light {
            [
                D2D1_GRADIENT_STOP { position: 0.0, color: rgba(0, 0, 0, 0.06) },
                D2D1_GRADIENT_STOP { position: 0.5, color: rgba(0, 0, 0, 0.14) },
                D2D1_GRADIENT_STOP { position: 1.0, color: rgba(0, 0, 0, 0.06) },
            ]
        } else {
            [
                D2D1_GRADIENT_STOP { position: 0.0, color: rgba(165, 224, 255, 0.18) },
                D2D1_GRADIENT_STOP { position: 0.5, color: rgba(255, 255, 255, 0.34) },
                D2D1_GRADIENT_STOP { position: 1.0, color: rgba(246, 206, 236, 0.18) },
            ]
        };
        if let Some(lg) = create_grad(rt, Vector2 { X: bar_r.left, Y: edge_y }, Vector2 { X: bar_r.right, Y: edge_y }, &line_stops) {
            rt.DrawLine(Vector2 { X: bar_r.left, Y: edge_y }, Vector2 { X: bar_r.right, Y: edge_y }, &lg, 1.0 * u, None);
        }
    }

    // 6. Adaptive crisp MacBook ink & subtle readability drop-shadow for dark-mode transparent glass
    let (ink, ink_m) = if is_light {
        ((18u8, 22u8, 30u8), (58u8, 66u8, 80u8))
    } else {
        ((248u8, 251u8, 255u8), (214u8, 224u8, 238u8))
    };
    let text_shadow = !is_light;

    // 7. Paint each interactive menu / status slot
    for slot in slots {
        let is_hov = info.hovered == Some(slot.item);
        let is_prs = info.pressed == Some(slot.item);
        let r = slot.rect;
        let scx = (r.left + r.right) * 0.5;
        let scy = (r.top + r.bottom) * 0.5;

        // Smooth macOS translucent rounded pill highlight on hover / press
        if is_hov || is_prs {
            let hrr = D2D1_ROUNDED_RECT { rect: r, radiusX: 6.0 * u, radiusY: 6.0 * u };
            let ha = if is_light {
                if is_prs { 0.16 } else { 0.09 }
            } else if is_prs {
                0.28
            } else {
                0.17
            };
            br.SetColor(&if is_light { rgba(16, 24, 38, ha) } else { rgba(255, 255, 255, ha) });
            rt.FillRoundedRectangle(&hrr, br);
            br.SetColor(&if is_light { rgba(0, 0, 0, 0.08) } else { rgba(255, 255, 255, 0.26) });
            rt.DrawRoundedRectangle(&hrr, br, 0.75 * u, None);
        }

        match slot.item {
            CleanTopBarItem::StartLogo => {
                // Crisp 4-pane MacBook-minimal monochrome Start Prism mark
                let s = 5.4 * u;
                let g = 1.3 * u;
                let (lr, lg, lb) = if is_hov { spec.dot_glow } else { ink };
                if text_shadow {
                    br.SetColor(&rgba(0, 0, 0, 0.28));
                    for &(dx, dy) in &[(-1.0f32, -1.0f32), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
                        let px = scx + if dx < 0.0 { -s - g * 0.5 } else { g * 0.5 };
                        let py = scy + 0.8 * u + if dy < 0.0 { -s - g * 0.5 } else { g * 0.5 };
                        rt.FillRoundedRectangle(
                            &D2D1_ROUNDED_RECT {
                                rect: D2D_RECT_F { left: px, top: py, right: px + s, bottom: py + s },
                                radiusX: 1.2 * u,
                                radiusY: 1.2 * u,
                            },
                            br,
                        );
                    }
                }
                br.SetColor(&rgba(lr, lg, lb, 0.96));
                for &(dx, dy) in &[(-1.0f32, -1.0f32), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
                    let px = scx + if dx < 0.0 { -s - g * 0.5 } else { g * 0.5 };
                    let py = scy + if dy < 0.0 { -s - g * 0.5 } else { g * 0.5 };
                    rt.FillRoundedRectangle(
                        &D2D1_ROUNDED_RECT {
                            rect: D2D_RECT_F { left: px, top: py, right: px + s, bottom: py + s },
                            radiusX: 1.2 * u,
                            radiusY: 1.2 * u,
                        },
                        br,
                    );
                }
            }
            CleanTopBarItem::ActiveApp => {
                if text_shadow {
                    br.SetColor(&rgba(0, 0, 0, 0.34));
                    let sr = D2D_RECT_F { left: r.left, top: r.top + 0.8 * u, right: r.right, bottom: r.bottom + 0.8 * u };
                    draw_d2d_text(rt, &slot.label, tf_bold, &sr, br);
                }
                br.SetColor(&rgba(ink.0, ink.1, ink.2, 0.98));
                draw_d2d_text(rt, &slot.label, tf_bold, &r, br);
            }
            CleanTopBarItem::MenuFiles
            | CleanTopBarItem::MenuWindows
            | CleanTopBarItem::MenuStudio
            | CleanTopBarItem::MenuDock => {
                if text_shadow {
                    br.SetColor(&rgba(0, 0, 0, 0.30));
                    let sr = D2D_RECT_F { left: r.left, top: r.top + 0.8 * u, right: r.right, bottom: r.bottom + 0.8 * u };
                    draw_d2d_text(rt, &slot.label, tf_reg, &sr, br);
                }
                let tc = if is_hov { ink } else { ink_m };
                br.SetColor(&rgba(tc.0, tc.1, tc.2, if is_hov { 0.98 } else { 0.88 }));
                draw_d2d_text(rt, &slot.label, tf_reg, &r, br);
            }
            CleanTopBarItem::HiddenTray => {
                let sw = 1.45 * u;
                let dir = if info.at_bottom { -1.0 } else { 1.0 };
                let p0 = Vector2 { X: scx - 4.0 * u, Y: scy - 1.8 * u * dir };
                let p1 = Vector2 { X: scx, Y: scy + 2.2 * u * dir };
                let p2 = Vector2 { X: scx + 4.0 * u, Y: scy - 1.8 * u * dir };
                br.SetColor(&rgba(ink.0, ink.1, ink.2, 0.86));
                rt.DrawLine(p0, p1, br, sw, None);
                rt.DrawLine(p1, p2, br, sw, None);
            }
            CleanTopBarItem::Volume => {
                // Crisp macOS speaker glyph + dynamic sound arcs
                let bx = scx - 5.2 * u;
                br.SetColor(&rgba(ink.0, ink.1, ink.2, 0.92));
                rt.FillRoundedRectangle(
                    &D2D1_ROUNDED_RECT {
                        rect: D2D_RECT_F { left: bx, top: scy - 2.3 * u, right: bx + 2.8 * u, bottom: scy + 2.3 * u },
                        radiusX: 0.6 * u,
                        radiusY: 0.6 * u,
                    },
                    br,
                );
                if let Some(f) = d2d {
                    if let Ok(geo) = f.CreatePathGeometry() {
                        if let Ok(sink) = geo.Open() {
                            sink.BeginFigure(Vector2 { X: bx + 2.4 * u, Y: scy - 2.3 * u }, D2D1_FIGURE_BEGIN_FILLED);
                            sink.AddLine(Vector2 { X: bx + 6.0 * u, Y: scy - 4.8 * u });
                            sink.AddLine(Vector2 { X: bx + 6.0 * u, Y: scy + 4.8 * u });
                            sink.AddLine(Vector2 { X: bx + 2.4 * u, Y: scy + 2.3 * u });
                            sink.EndFigure(D2D1_FIGURE_END_CLOSED);
                            let _ = sink.Close();
                            rt.FillGeometry(&geo, br, None);
                        }
                    }
                    if info.vol_muted || info.vol_pct == 0 {
                        br.SetColor(&rgba(245, 92, 96, 0.96));
                        rt.DrawLine(
                            Vector2 { X: bx + 7.6 * u, Y: scy - 2.8 * u },
                            Vector2 { X: bx + 11.6 * u, Y: scy + 2.8 * u },
                            br,
                            1.35 * u,
                            None,
                        );
                        rt.DrawLine(
                            Vector2 { X: bx + 11.6 * u, Y: scy - 2.8 * u },
                            Vector2 { X: bx + 7.6 * u, Y: scy + 2.8 * u },
                            br,
                            1.35 * u,
                            None,
                        );
                    } else {
                        let waves = if info.vol_pct > 62 { 2 } else { 1 };
                        for wi in 0..waves {
                            let wr = (2.8 + wi as f32 * 2.4) * u;
                            if let Ok(arc_geo) = f.CreatePathGeometry() {
                                if let Ok(sink) = arc_geo.Open() {
                                    let ox = bx + 6.4 * u;
                                    sink.BeginFigure(
                                        Vector2 { X: ox + wr * 0.55, Y: scy - wr * 0.72 },
                                        D2D1_FIGURE_BEGIN_HOLLOW,
                                    );
                                    sink.AddArc(&D2D1_ARC_SEGMENT {
                                        point: Vector2 { X: ox + wr * 0.55, Y: scy + wr * 0.72 },
                                        size: D2D_SIZE_F { width: wr, height: wr },
                                        rotationAngle: 0.0,
                                        sweepDirection: D2D1_SWEEP_DIRECTION_CLOCKWISE,
                                        arcSize: D2D1_ARC_SIZE_SMALL,
                                    });
                                    sink.EndFigure(D2D1_FIGURE_END_OPEN);
                                    let _ = sink.Close();
                                    br.SetColor(&rgba(ink.0, ink.1, ink.2, 0.88));
                                    rt.DrawGeometry(&arc_geo, br, 1.25 * u, None);
                                }
                            }
                        }
                    }
                }
            }
            CleanTopBarItem::Wifi => {
                // Authentic macOS 3-arc Wi-Fi fan icon (or crisp wired icon when Ethernet)
                if info.net_connected && !info.net_wifi {
                    br.SetColor(&rgba(ink.0, ink.1, ink.2, 0.92));
                    rt.DrawRoundedRectangle(
                        &D2D1_ROUNDED_RECT {
                            rect: D2D_RECT_F { left: scx - 5.5 * u, top: scy - 4.2 * u, right: scx + 5.5 * u, bottom: scy + 2.6 * u },
                            radiusX: 1.4 * u,
                            radiusY: 1.4 * u,
                        },
                        br,
                        1.25 * u,
                        None,
                    );
                    rt.DrawLine(Vector2 { X: scx - 2.6 * u, Y: scy + 4.4 * u }, Vector2 { X: scx + 2.6 * u, Y: scy + 4.4 * u }, br, 1.3 * u, None);
                } else {
                    let dot_y = scy + 4.0 * u;
                    let active_arcs = if !info.net_connected {
                        0usize
                    } else if info.net_strength >= 70 {
                        3
                    } else if info.net_strength >= 35 {
                        2
                    } else {
                        1
                    };
                    br.SetColor(&rgba(ink.0, ink.1, ink.2, if info.net_connected { 0.96 } else { 0.40 }));
                    rt.FillEllipse(
                        &D2D1_ELLIPSE { point: Vector2 { X: scx, Y: dot_y }, radiusX: 1.35 * u, radiusY: 1.35 * u },
                        br,
                    );
                    if let Some(f) = d2d {
                        for (ai, rad_w) in [3.8f32, 6.4, 9.0].into_iter().enumerate() {
                            let rw = rad_w * u;
                            let a = if ai < active_arcs { 0.94 } else { 0.28 };
                            if let Ok(ag) = f.CreatePathGeometry() {
                                if let Ok(sink) = ag.Open() {
                                    sink.BeginFigure(
                                        Vector2 { X: scx - rw * 0.72, Y: dot_y - rw * 0.68 },
                                        D2D1_FIGURE_BEGIN_HOLLOW,
                                    );
                                    sink.AddArc(&D2D1_ARC_SEGMENT {
                                        point: Vector2 { X: scx + rw * 0.72, Y: dot_y - rw * 0.68 },
                                        size: D2D_SIZE_F { width: rw, height: rw },
                                        rotationAngle: 0.0,
                                        sweepDirection: D2D1_SWEEP_DIRECTION_CLOCKWISE,
                                        arcSize: D2D1_ARC_SIZE_SMALL,
                                    });
                                    sink.EndFigure(D2D1_FIGURE_END_OPEN);
                                    let _ = sink.Close();
                                    br.SetColor(&rgba(ink.0, ink.1, ink.2, a));
                                    rt.DrawGeometry(&ag, br, 1.3 * u, None);
                                }
                            }
                        }
                    }
                    if !info.net_connected {
                        br.SetColor(&rgba(245, 92, 96, 0.94));
                        rt.DrawLine(
                            Vector2 { X: scx - 5.5 * u, Y: scy - 4.5 * u },
                            Vector2 { X: scx + 5.5 * u, Y: scy + 4.5 * u },
                            br,
                            1.35 * u,
                            None,
                        );
                    }
                }
            }
            CleanTopBarItem::Battery => {
                // Crisp MacBook percentage + rounded capsule battery with terminal nub & charge fill
                let pct = info.bat_pct.unwrap_or(100).min(100);
                let txt_r = D2D_RECT_F {
                    left: r.left + 2.0 * u,
                    top: r.top,
                    right: r.right - 25.0 * u,
                    bottom: r.bottom,
                };
                if text_shadow {
                    br.SetColor(&rgba(0, 0, 0, 0.30));
                    let sr = D2D_RECT_F { left: txt_r.left, top: txt_r.top + 0.8 * u, right: txt_r.right, bottom: txt_r.bottom + 0.8 * u };
                    draw_d2d_text(rt, &slot.label, tf_reg, &sr, br);
                }
                br.SetColor(&rgba(ink.0, ink.1, ink.2, 0.90));
                draw_d2d_text(rt, &slot.label, tf_reg, &txt_r, br);

                let bx0 = r.right - 23.0 * u;
                let bw = 18.0 * u;
                let bh = 9.6 * u;
                let by0 = scy - bh * 0.5;
                let bat_r = D2D_RECT_F { left: bx0, top: by0, right: bx0 + bw, bottom: by0 + bh };
                br.SetColor(&rgba(ink.0, ink.1, ink.2, 0.64));
                rt.DrawRoundedRectangle(
                    &D2D1_ROUNDED_RECT { rect: bat_r, radiusX: 2.6 * u, radiusY: 2.6 * u },
                    br,
                    1.05 * u,
                    None,
                );
                // Terminal nub
                rt.FillRoundedRectangle(
                    &D2D1_ROUNDED_RECT {
                        rect: D2D_RECT_F {
                            left: bat_r.right + 1.0 * u,
                            top: scy - 2.0 * u,
                            right: bat_r.right + 2.6 * u,
                            bottom: scy + 2.0 * u,
                        },
                        radiusX: 0.8 * u,
                        radiusY: 0.8 * u,
                    },
                    br,
                );
                // Inner fill level
                let fill_col = if info.bat_charging {
                    rgba(52, 212, 108, 0.96)
                } else if pct <= 20 {
                    rgba(246, 82, 82, 0.96)
                } else if info.bat_saver {
                    rgba(248, 192, 58, 0.96)
                } else {
                    rgba(ink.0, ink.1, ink.2, 0.92)
                };
                let fw = ((bw - 3.6 * u) * (pct as f32 / 100.0)).max(2.0 * u);
                br.SetColor(&fill_col);
                rt.FillRoundedRectangle(
                    &D2D1_ROUNDED_RECT {
                        rect: D2D_RECT_F {
                            left: bx0 + 1.8 * u,
                            top: by0 + 1.8 * u,
                            right: bx0 + 1.8 * u + fw,
                            bottom: by0 + bh - 1.8 * u,
                        },
                        radiusX: 1.3 * u,
                        radiusY: 1.3 * u,
                    },
                    br,
                );
            }
            CleanTopBarItem::Search => {
                // Crisp MacBook Spotlight magnifying glass
                let lr = 4.2 * u;
                let lx = scx - 1.2 * u;
                let ly = scy - 1.2 * u;
                br.SetColor(&rgba(ink.0, ink.1, ink.2, 0.92));
                rt.DrawEllipse(
                    &D2D1_ELLIPSE { point: Vector2 { X: lx, Y: ly }, radiusX: lr, radiusY: lr },
                    br,
                    1.35 * u,
                    None,
                );
                rt.DrawLine(
                    Vector2 { X: lx + lr * 0.72, Y: ly + lr * 0.72 },
                    Vector2 { X: scx + 5.2 * u, Y: scy + 5.2 * u },
                    br,
                    1.5 * u,
                    None,
                );
            }
            CleanTopBarItem::ControlCenter => {
                // Signature macOS Control Center twin horizontal toggle-switch glyphs
                let tw = 11.4 * u;
                let th = 4.6 * u;
                let x0 = scx - tw * 0.5;
                // Upper switch (knob on left)
                let uy0 = scy - 5.2 * u;
                br.SetColor(&rgba(ink.0, ink.1, ink.2, 0.86));
                rt.DrawRoundedRectangle(
                    &D2D1_ROUNDED_RECT {
                        rect: D2D_RECT_F { left: x0, top: uy0, right: x0 + tw, bottom: uy0 + th },
                        radiusX: th * 0.5,
                        radiusY: th * 0.5,
                    },
                    br,
                    1.05 * u,
                    None,
                );
                rt.FillEllipse(
                    &D2D1_ELLIPSE { point: Vector2 { X: x0 + th * 0.52, Y: uy0 + th * 0.5 }, radiusX: 1.55 * u, radiusY: 1.55 * u },
                    br,
                );
                // Lower switch (filled capsule, knob on right)
                let ly0 = scy + 0.6 * u;
                br.SetColor(&rgba(ink.0, ink.1, ink.2, 0.92));
                rt.FillRoundedRectangle(
                    &D2D1_ROUNDED_RECT {
                        rect: D2D_RECT_F { left: x0, top: ly0, right: x0 + tw, bottom: ly0 + th },
                        radiusX: th * 0.5,
                        radiusY: th * 0.5,
                    },
                    br,
                );
                let knob_c = if is_light { rgba(255, 255, 255, 0.96) } else { rgba(18, 22, 30, 0.92) };
                br.SetColor(&knob_c);
                rt.FillEllipse(
                    &D2D1_ELLIPSE { point: Vector2 { X: x0 + tw - th * 0.52, Y: ly0 + th * 0.5 }, radiusX: 1.55 * u, radiusY: 1.55 * u },
                    br,
                );
            }
            CleanTopBarItem::Clock => {
                // One UI themes get their iconic compact gradient clock capsule right in the Top Bar!
                if matches!(theme, TaskbarTheme::OneUiLightChips | TaskbarTheme::OneUiDarkChips) {
                    let pr = D2D_RECT_F {
                        left: r.left + 2.0 * u,
                        top: r.top + 1.5 * u,
                        right: r.right - 2.0 * u,
                        bottom: r.bottom - 1.5 * u,
                    };
                    let prr = D2D1_ROUNDED_RECT {
                        rect: pr,
                        radiusX: (pr.bottom - pr.top) * 0.5,
                        radiusY: (pr.bottom - pr.top) * 0.5,
                    };
                    let (c0, c1) = if theme == TaskbarTheme::OneUiLightChips {
                        ((242u8, 62u8, 92u8), (168u8, 68u8, 255u8))
                    } else {
                        ((48u8, 112u8, 232u8), (96u8, 158u8, 252u8))
                    };
                    let stops = [
                        D2D1_GRADIENT_STOP { position: 0.0, color: rgba(c0.0, c0.1, c0.2, 0.94) },
                        D2D1_GRADIENT_STOP { position: 1.0, color: rgba(c1.0, c1.1, c1.2, 0.94) },
                    ];
                    if let Some(g) = create_grad(rt, Vector2 { X: pr.left, Y: pr.top }, Vector2 { X: pr.right, Y: pr.bottom }, &stops) {
                        rt.FillRoundedRectangle(&prr, &g);
                    }
                    br.SetColor(&rgba(255, 255, 255, 0.98));
                    draw_d2d_text(rt, &slot.label, tf_bold, &pr, br);
                } else {
                    if text_shadow {
                        br.SetColor(&rgba(0, 0, 0, 0.34));
                        let sr = D2D_RECT_F { left: r.left, top: r.top + 0.8 * u, right: r.right, bottom: r.bottom + 0.8 * u };
                        draw_d2d_text(rt, &slot.label, tf_bold, &sr, br);
                    }
                    br.SetColor(&rgba(ink.0, ink.1, ink.2, 0.98));
                    draw_d2d_text(rt, &slot.label, tf_bold, &r, br);
                }
            }
        }
    }
}

/// macOS-style Frosted Glass Sub-UI Popover anchored directly beneath the Clean & Minimal Top Menu Bar.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TopBarPopoverKind {
    ControlCenter,
    ClockCalendar,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TopBarPopoverHit {
    WifiRow,
    BluetoothRow,
    StudioRow,
    ToggleDesktopOnly,
    ToggleBarEdge,
    ToggleMute,
    VolumeTrack,
    BatteryRow,
    OpenQuickSettings,
    OpenNotifications,
}

pub struct TopBarPopoverInfo<'a> {
    pub kind: TopBarPopoverKind,
    pub wifi_on: bool,
    pub wifi_name: &'a str,
    pub wifi_signal: u8,
    pub vol_pct: u8,
    pub vol_muted: bool,
    pub bat_pct: Option<u8>,
    pub bat_charging: bool,
    pub desktop_only: bool,
    pub bar_at_bottom: bool,
    pub time_big: &'a str,
    pub date_full: &'a str,
    pub year: i32,
    pub month: u32,
    pub today: u32,
    pub hovered: Option<TopBarPopoverHit>,
}

#[derive(Clone, Debug)]
pub struct TopBarPopoverLayout {
    pub panel_r: D2D_RECT_F,
    pub hits: Vec<(TopBarPopoverHit, D2D_RECT_F)>,
    pub vol_track_r: D2D_RECT_F,
}

/// Returns `(canvas_w_dip, canvas_h_dip, pad_dip)` for a macOS Top Bar Sub-UI Popover.
pub fn top_bar_popover_dims(kind: TopBarPopoverKind) -> (f32, f32, f32) {
    match kind {
        TopBarPopoverKind::ControlCenter => (316.0, 310.0, 10.0),
        TopBarPopoverKind::ClockCalendar => (288.0, 296.0, 10.0),
    }
}

pub fn layout_top_bar_popover(kind: TopBarPopoverKind, u: f32) -> TopBarPopoverLayout {
    let (cw, ch, pad) = top_bar_popover_dims(kind);
    let panel_r = D2D_RECT_F {
        left: pad * u,
        top: pad * u,
        right: (cw - pad) * u,
        bottom: (ch - pad) * u,
    };
    let mut hits = Vec::with_capacity(10);
    let mut vol_track_r = D2D_RECT_F::default();
    let ix = panel_r.left + 12.0 * u;
    let ir = panel_r.right - 12.0 * u;
    let iw = ir - ix;

    match kind {
        TopBarPopoverKind::ControlCenter => {
            let gap = 10.0 * u;
            let half = (iw - gap) * 0.5;
            let y0 = panel_r.top + 12.0 * u;
            // Left Connectivity card (3 rows)
            let row_h = 38.0 * u;
            hits.push((
                TopBarPopoverHit::WifiRow,
                D2D_RECT_F { left: ix, top: y0, right: ix + half, bottom: y0 + row_h },
            ));
            hits.push((
                TopBarPopoverHit::BluetoothRow,
                D2D_RECT_F { left: ix, top: y0 + row_h, right: ix + half, bottom: y0 + row_h * 2.0 },
            ));
            hits.push((
                TopBarPopoverHit::StudioRow,
                D2D_RECT_F { left: ix, top: y0 + row_h * 2.0, right: ix + half, bottom: y0 + row_h * 3.0 },
            ));
            // Right 2 stacked mode toggles
            let rx0 = ix + half + gap;
            let tog_h = (row_h * 3.0 - gap) * 0.5;
            hits.push((
                TopBarPopoverHit::ToggleDesktopOnly,
                D2D_RECT_F { left: rx0, top: y0, right: ir, bottom: y0 + tog_h },
            ));
            hits.push((
                TopBarPopoverHit::ToggleBarEdge,
                D2D_RECT_F { left: rx0, top: y0 + tog_h + gap, right: ir, bottom: y0 + row_h * 3.0 },
            ));
            // Sound glossy slider card
            let sy0 = y0 + row_h * 3.0 + gap;
            let sh = 62.0 * u;
            let mute_r = D2D_RECT_F {
                left: ix + 8.0 * u,
                top: sy0 + 28.0 * u,
                right: ix + 34.0 * u,
                bottom: sy0 + 54.0 * u,
            };
            vol_track_r = D2D_RECT_F {
                left: ix + 40.0 * u,
                top: sy0 + 30.0 * u,
                right: ir - 12.0 * u,
                bottom: sy0 + 52.0 * u,
            };
            hits.push((TopBarPopoverHit::ToggleMute, mute_r));
            hits.push((TopBarPopoverHit::VolumeTrack, vol_track_r));
            // Bottom Battery + Quick Settings row
            let by0 = sy0 + sh + gap;
            let bh = (panel_r.bottom - 12.0 * u - by0).max(46.0 * u);
            let split_x = ix + iw * 0.57;
            hits.push((
                TopBarPopoverHit::BatteryRow,
                D2D_RECT_F { left: ix, top: by0, right: split_x - 4.0 * u, bottom: by0 + bh },
            ));
            hits.push((
                TopBarPopoverHit::OpenQuickSettings,
                D2D_RECT_F { left: split_x + 4.0 * u, top: by0, right: ir, bottom: by0 + bh },
            ));
        }
        TopBarPopoverKind::ClockCalendar => {
            let by1 = panel_r.bottom - 12.0 * u;
            let by0 = by1 - 30.0 * u;
            hits.push((
                TopBarPopoverHit::OpenNotifications,
                D2D_RECT_F { left: ix, top: by0, right: ir, bottom: by1 },
            ));
        }
    }

    TopBarPopoverLayout {
        panel_r,
        hits,
        vol_track_r,
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

/// Sakamoto's algorithm: returns `0 = Sun .. 6 = Sat` for `(year, month 1..12, day 1..31)`.
fn weekday_sakamoto(year: i32, month: u32, day: u32) -> u32 {
    let t = [0i32, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4];
    let mut y = year;
    let m = month.clamp(1, 12) as usize;
    if m < 3 {
        y -= 1;
    }
    ((y + y / 4 - y / 100 + y / 400 + t[m - 1] + day as i32).rem_euclid(7)) as u32
}

/// Renders the macOS-style Frosted Glass Top Bar Sub-UI Popover (Control Center or Clock/Calendar).
#[allow(clippy::too_many_arguments)]
pub unsafe fn draw_top_bar_popover(
    rt: &ID2D1DCRenderTarget,
    d2d: Option<&ID2D1Factory>,
    br: &ID2D1SolidColorBrush,
    tf_bold: &IDWriteTextFormat,
    tf_reg: &IDWriteTextFormat,
    tf_left: &IDWriteTextFormat,
    layout: &TopBarPopoverLayout,
    info: &TopBarPopoverInfo<'_>,
    cfg: &AppConfig,
    u: f32,
) {
    let spec = dock_spec(cfg.tb_theme);
    let is_light = spec.light_mode_paper;
    let pr = layout.panel_r;
    let rad = 14.0 * u;
    let live_hw = is_live_hw_blur_active();

    // 1. Soft macOS ambient + contact elevation shadow
    draw_macos_soft_shadow(rt, br, pr, rad, false, is_light, u);

    // 2. Frosted backdrop blur (yields cleanly to live Windows 11 DWM Acrylic when live_hw == true)
    let has_blur = draw_backdrop_blurred_rrect(rt, d2d, pr, rad, 0.98, 0.95);

    // 3. Crystal-clear macOS Control Center popover glass glaze
    let base_a = if is_light {
        if live_hw { 0.56 } else if has_blur { 0.70 } else { 0.92 }
    } else if live_hw {
        0.44
    } else if has_blur {
        0.58
    } else {
        0.88
    };
    let (gt, gb) = if is_light {
        ((248u8, 251u8, 255u8), (232u8, 240u8, 250u8))
    } else {
        ((28u8, 34u8, 46u8), (18u8, 22u8, 32u8))
    };
    let prr = D2D1_ROUNDED_RECT { rect: pr, radiusX: rad, radiusY: rad };
    let cx = (pr.left + pr.right) * 0.5;
    if let Some(g) = create_grad(
        rt,
        Vector2 { X: cx, Y: pr.top },
        Vector2 { X: cx, Y: pr.bottom },
        &[
            D2D1_GRADIENT_STOP { position: 0.0, color: rgba(gt.0, gt.1, gt.2, base_a) },
            D2D1_GRADIENT_STOP { position: 1.0, color: rgba(gb.0, gb.1, gb.2, (base_a + 0.06).min(0.96)) },
        ],
    ) {
        rt.FillRoundedRectangle(&prr, &g);
    }
    draw_fresnel_glass_rim(rt, br, pr, rad, false, is_light, 1.05, u);

    let (ink, ink_m) = if is_light {
        ((18u8, 22u8, 32u8), (74u8, 84u8, 100u8))
    } else {
        ((246u8, 250u8, 255u8), (178u8, 192u8, 212u8))
    };
    let card_bg = if is_light {
        rgba(255, 255, 255, 0.56)
    } else {
        rgba(255, 255, 255, 0.09)
    };
    let card_hov = if is_light {
        rgba(255, 255, 255, 0.78)
    } else {
        rgba(255, 255, 255, 0.16)
    };
    let card_rim = if is_light {
        rgba(0, 0, 0, 0.08)
    } else {
        rgba(255, 255, 255, 0.16)
    };
    let accent = if spec.dot_active.0 == 0 && spec.dot_active.1 == 0 && spec.dot_active.2 == 0 {
        (58u8, 146u8, 255u8)
    } else {
        spec.dot_active
    };

    let ix = pr.left + 12.0 * u;
    let ir = pr.right - 12.0 * u;
    let iw = ir - ix;

    match info.kind {
        TopBarPopoverKind::ControlCenter => {
            let gap = 10.0 * u;
            let half = (iw - gap) * 0.5;
            let y0 = pr.top + 12.0 * u;
            let row_h = 38.0 * u;
            let left_card = D2D_RECT_F { left: ix, top: y0, right: ix + half, bottom: y0 + row_h * 3.0 };
            let lc_rr = D2D1_ROUNDED_RECT { rect: left_card, radiusX: 11.0 * u, radiusY: 11.0 * u };
            br.SetColor(&card_bg);
            rt.FillRoundedRectangle(&lc_rr, br);
            br.SetColor(&card_rim);
            rt.DrawRoundedRectangle(&lc_rr, br, 0.9 * u, None);

            // 3 Connectivity rows inside Left Card
            let wifi_sub = if info.wifi_on {
                let nm = if info.wifi_name.trim().is_empty() { "Connected" } else { info.wifi_name.trim() };
                if nm.chars().count() > 11 {
                    nm.chars().take(10).collect::<String>() + "…"
                } else {
                    nm.to_string()
                }
            } else {
                "Off".to_string()
            };
            let rows = [
                (TopBarPopoverHit::WifiRow, "Wi-Fi", wifi_sub.as_str(), info.wifi_on, (52u8, 142u8, 255u8)),
                (TopBarPopoverHit::BluetoothRow, "Bluetooth", "Devices", true, (66, 134, 248)),
                (TopBarPopoverHit::StudioRow, "AutumnDeck", "Open Studio", true, (245, 166, 66)),
            ];
            for (ri, (hit_id, title, sub, active, badge_c)) in rows.into_iter().enumerate() {
                let ry0 = y0 + (ri as f32) * row_h;
                let rr_row = D2D_RECT_F {
                    left: ix + 4.0 * u,
                    top: ry0 + 3.0 * u,
                    right: ix + half - 4.0 * u,
                    bottom: ry0 + row_h - 3.0 * u,
                };
                if info.hovered == Some(hit_id) {
                    br.SetColor(&card_hov);
                    rt.FillRoundedRectangle(
                        &D2D1_ROUNDED_RECT { rect: rr_row, radiusX: 8.0 * u, radiusY: 8.0 * u },
                        br,
                    );
                }
                let bcx = rr_row.left + 16.0 * u;
                let bcy = (rr_row.top + rr_row.bottom) * 0.5;
                br.SetColor(&if active {
                    rgba(badge_c.0, badge_c.1, badge_c.2, 0.94)
                } else {
                    rgba(128, 136, 148, 0.35)
                });
                rt.FillEllipse(
                    &D2D1_ELLIPSE { point: Vector2 { X: bcx, Y: bcy }, radiusX: 11.5 * u, radiusY: 11.5 * u },
                    br,
                );
                // Crisp mini glyph inside circle
                br.SetColor(&rgba(255, 255, 255, 0.96));
                match ri {
                    0 => {
                        let dot_y = bcy + 3.8 * u;
                        let active_arcs = if !info.wifi_on {
                            0usize
                        } else if info.wifi_signal >= 70 {
                            3
                        } else if info.wifi_signal >= 35 {
                            2
                        } else {
                            1
                        };
                        rt.FillEllipse(
                            &D2D1_ELLIPSE { point: Vector2 { X: bcx, Y: dot_y }, radiusX: 1.25 * u, radiusY: 1.25 * u },
                            br,
                        );
                        if let Some(f) = d2d {
                            for (ai, rad_w) in [3.4f32, 5.6, 7.8].into_iter().enumerate() {
                                let rw = rad_w * u;
                                let a = if ai < active_arcs { 0.96 } else { 0.34 };
                                if let Ok(ag) = f.CreatePathGeometry() {
                                    if let Ok(sink) = ag.Open() {
                                        sink.BeginFigure(
                                            Vector2 { X: bcx - rw * 0.72, Y: dot_y - rw * 0.68 },
                                            D2D1_FIGURE_BEGIN_HOLLOW,
                                        );
                                        sink.AddArc(&D2D1_ARC_SEGMENT {
                                            point: Vector2 { X: bcx + rw * 0.72, Y: dot_y - rw * 0.68 },
                                            size: D2D_SIZE_F { width: rw, height: rw },
                                            rotationAngle: 0.0,
                                            sweepDirection: D2D1_SWEEP_DIRECTION_CLOCKWISE,
                                            arcSize: D2D1_ARC_SIZE_SMALL,
                                        });
                                        sink.EndFigure(D2D1_FIGURE_END_OPEN);
                                        let _ = sink.Close();
                                        br.SetColor(&rgba(255, 255, 255, a));
                                        rt.DrawGeometry(&ag, br, 1.25 * u, None);
                                    }
                                }
                            }
                        }
                    }
                    1 => {
                        rt.DrawLine(Vector2 { X: bcx, Y: bcy - 4.8 * u }, Vector2 { X: bcx, Y: bcy + 4.8 * u }, br, 1.4 * u, None);
                        rt.DrawLine(Vector2 { X: bcx, Y: bcy - 4.8 * u }, Vector2 { X: bcx + 3.4 * u, Y: bcy - 2.0 * u }, br, 1.3 * u, None);
                        rt.DrawLine(Vector2 { X: bcx + 3.4 * u, Y: bcy - 2.0 * u }, Vector2 { X: bcx - 2.6 * u, Y: bcy + 2.4 * u }, br, 1.3 * u, None);
                        rt.DrawLine(Vector2 { X: bcx, Y: bcy + 4.8 * u }, Vector2 { X: bcx + 3.4 * u, Y: bcy + 2.0 * u }, br, 1.3 * u, None);
                    }
                    _ => {
                        rt.FillRoundedRectangle(
                            &D2D1_ROUNDED_RECT {
                                rect: D2D_RECT_F { left: bcx - 4.0 * u, top: bcy - 4.5 * u, right: bcx - 0.5 * u, bottom: bcy + 4.5 * u },
                                radiusX: 1.4 * u,
                                radiusY: 1.4 * u,
                            },
                            br,
                        );
                        rt.FillRoundedRectangle(
                            &D2D1_ROUNDED_RECT {
                                rect: D2D_RECT_F { left: bcx + 1.0 * u, top: bcy - 2.5 * u, right: bcx + 4.2 * u, bottom: bcy + 2.5 * u },
                                radiusX: 1.3 * u,
                                radiusY: 1.3 * u,
                            },
                            br,
                        );
                    }
                }
                let tx0 = rr_row.left + 33.0 * u;
                br.SetColor(&rgba(ink.0, ink.1, ink.2, 0.96));
                draw_d2d_text(rt, title, tf_left, &D2D_RECT_F { left: tx0, top: rr_row.top + 1.0 * u, right: rr_row.right - 2.0 * u, bottom: bcy + 1.0 * u }, br);
                br.SetColor(&rgba(ink_m.0, ink_m.1, ink_m.2, 0.84));
                draw_d2d_text(rt, sub, tf_left, &D2D_RECT_F { left: tx0, top: bcy - 1.0 * u, right: rr_row.right - 2.0 * u, bottom: rr_row.bottom - 1.0 * u }, br);
            }

            // Right 2 stacked mode toggle cards (Desktop Only + Bar Position)
            let rx0 = ix + half + gap;
            let tog_h = (row_h * 3.0 - gap) * 0.5;
            let togs = [
                (
                    TopBarPopoverHit::ToggleDesktopOnly,
                    D2D_RECT_F { left: rx0, top: y0, right: ir, bottom: y0 + tog_h },
                    "Desktop Only",
                    if info.desktop_only { "Hidden in Apps" } else { "Shown in Apps" },
                    info.desktop_only,
                ),
                (
                    TopBarPopoverHit::ToggleBarEdge,
                    D2D_RECT_F { left: rx0, top: y0 + tog_h + gap, right: ir, bottom: y0 + row_h * 3.0 },
                    "Bar Position",
                    if info.bar_at_bottom { "Bottom (Down ▼)" } else { "Top (Up ▲)" },
                    true,
                ),
            ];
            for (hit_id, tr, title, sub, active) in togs {
                let trr = D2D1_ROUNDED_RECT { rect: tr, radiusX: 11.0 * u, radiusY: 11.0 * u };
                let hov = info.hovered == Some(hit_id);
                br.SetColor(&if hov { card_hov } else { card_bg });
                rt.FillRoundedRectangle(&trr, br);
                br.SetColor(&if active && hit_id == TopBarPopoverHit::ToggleDesktopOnly {
                    rgba(accent.0, accent.1, accent.2, 0.72)
                } else {
                    card_rim
                });
                rt.DrawRoundedRectangle(&trr, br, if active && hit_id == TopBarPopoverHit::ToggleDesktopOnly { 1.2 * u } else { 0.9 * u }, None);

                let dot_x = tr.left + 15.0 * u;
                let mid_y = (tr.top + tr.bottom) * 0.5;
                br.SetColor(&if active && hit_id == TopBarPopoverHit::ToggleDesktopOnly {
                    rgba(52, 212, 108, 0.96)
                } else if hit_id == TopBarPopoverHit::ToggleBarEdge {
                    rgba(accent.0, accent.1, accent.2, 0.90)
                } else {
                    rgba(ink_m.0, ink_m.1, ink_m.2, 0.38)
                });
                rt.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: dot_x, Y: mid_y }, radiusX: 5.0 * u, radiusY: 5.0 * u }, br);
                br.SetColor(&rgba(ink.0, ink.1, ink.2, 0.96));
                draw_d2d_text(rt, title, tf_left, &D2D_RECT_F { left: tr.left + 27.0 * u, top: tr.top + 8.0 * u, right: tr.right - 6.0 * u, bottom: mid_y + 1.0 * u }, br);
                br.SetColor(&rgba(ink_m.0, ink_m.1, ink_m.2, 0.84));
                draw_d2d_text(rt, sub, tf_left, &D2D_RECT_F { left: tr.left + 27.0 * u, top: mid_y - 1.0 * u, right: tr.right - 6.0 * u, bottom: tr.bottom - 7.0 * u }, br);
            }

            // Full-width Glossy macOS Sound Slider Card
            let sy0 = y0 + row_h * 3.0 + gap;
            let sh = 62.0 * u;
            let scard = D2D_RECT_F { left: ix, top: sy0, right: ir, bottom: sy0 + sh };
            let sc_rr = D2D1_ROUNDED_RECT { rect: scard, radiusX: 11.0 * u, radiusY: 11.0 * u };
            br.SetColor(&card_bg);
            rt.FillRoundedRectangle(&sc_rr, br);
            br.SetColor(&card_rim);
            rt.DrawRoundedRectangle(&sc_rr, br, 0.9 * u, None);

            br.SetColor(&rgba(ink.0, ink.1, ink.2, 0.94));
            draw_d2d_text(rt, "Sound", tf_left, &D2D_RECT_F { left: ix + 12.0 * u, top: sy0 + 6.0 * u, right: ix + 120.0 * u, bottom: sy0 + 25.0 * u }, br);
            let vol_lbl = if info.vol_muted {
                "Muted".to_string()
            } else {
                format!("{}%", info.vol_pct)
            };
            br.SetColor(&if info.vol_muted { rgba(246, 94, 96, 0.96) } else { rgba(ink_m.0, ink_m.1, ink_m.2, 0.90) });
            draw_d2d_text(rt, &vol_lbl, tf_bold, &D2D_RECT_F { left: ir - 48.0 * u, top: sy0 + 6.0 * u, right: ir - 8.0 * u, bottom: sy0 + 25.0 * u }, br);

            // Mute button circle on left of slider
            let mr = D2D_RECT_F { left: ix + 8.0 * u, top: sy0 + 28.0 * u, right: ix + 34.0 * u, bottom: sy0 + 54.0 * u };
            let mcx = (mr.left + mr.right) * 0.5;
            let mcy = (mr.top + mr.bottom) * 0.5;
            if info.hovered == Some(TopBarPopoverHit::ToggleMute) {
                br.SetColor(&card_hov);
                rt.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: mcx, Y: mcy }, radiusX: 12.0 * u, radiusY: 12.0 * u }, br);
            }
            br.SetColor(&if info.vol_muted { rgba(246, 94, 96, 0.96) } else { rgba(ink.0, ink.1, ink.2, 0.92) });
            rt.FillRoundedRectangle(
                &D2D1_ROUNDED_RECT {
                    rect: D2D_RECT_F { left: mcx - 5.5 * u, top: mcy - 2.6 * u, right: mcx - 1.5 * u, bottom: mcy + 2.6 * u },
                    radiusX: 0.8 * u,
                    radiusY: 0.8 * u,
                },
                br,
            );
            rt.FillRoundedRectangle(
                &D2D1_ROUNDED_RECT {
                    rect: D2D_RECT_F { left: mcx - 2.0 * u, top: mcy - 5.0 * u, right: mcx + 2.4 * u, bottom: mcy + 5.0 * u },
                    radiusX: 1.2 * u,
                    radiusY: 1.2 * u,
                },
                br,
            );
            if info.vol_muted {
                rt.DrawLine(Vector2 { X: mcx + 3.5 * u, Y: mcy - 3.0 * u }, Vector2 { X: mcx + 7.5 * u, Y: mcy + 3.0 * u }, br, 1.4 * u, None);
                rt.DrawLine(Vector2 { X: mcx + 7.5 * u, Y: mcy - 3.0 * u }, Vector2 { X: mcx + 3.5 * u, Y: mcy + 3.0 * u }, br, 1.4 * u, None);
            }

            // Glossy macOS pill slider track + knob
            let tr = layout.vol_track_r;
            let tr_h = tr.bottom - tr.top;
            let tr_rad = tr_h * 0.5;
            br.SetColor(&if is_light { rgba(0, 0, 0, 0.12) } else { rgba(255, 255, 255, 0.16) });
            rt.FillRoundedRectangle(&D2D1_ROUNDED_RECT { rect: tr, radiusX: tr_rad, radiusY: tr_rad }, br);
            let frac = if info.vol_muted { 0.0 } else { (info.vol_pct as f32 / 100.0).clamp(0.0, 1.0) };
            let fill_w = (tr_h + (tr.right - tr.left - tr_h) * frac).clamp(tr_h, tr.right - tr.left);
            let fr = D2D_RECT_F { left: tr.left, top: tr.top, right: tr.left + fill_w, bottom: tr.bottom };
            br.SetColor(&if is_light { rgba(255, 255, 255, 0.96) } else { rgba(246, 250, 255, 0.92) });
            rt.FillRoundedRectangle(&D2D1_ROUNDED_RECT { rect: fr, radiusX: tr_rad, radiusY: tr_rad }, br);
            let knob_cx = fr.right - tr_rad;
            let knob_cy = (tr.top + tr.bottom) * 0.5;
            br.SetColor(&rgba(0, 0, 0, 0.18));
            rt.DrawEllipse(&D2D1_ELLIPSE { point: Vector2 { X: knob_cx, Y: knob_cy }, radiusX: tr_rad - 1.0 * u, radiusY: tr_rad - 1.0 * u }, br, 1.0 * u, None);

            // Bottom Row: Battery card + Quick Settings pill button
            let by0 = sy0 + sh + gap;
            let bh = (pr.bottom - 12.0 * u - by0).max(46.0 * u);
            let split_x = ix + iw * 0.57;
            let bat_card = D2D_RECT_F { left: ix, top: by0, right: split_x - 4.0 * u, bottom: by0 + bh };
            let qs_card = D2D_RECT_F { left: split_x + 4.0 * u, top: by0, right: ir, bottom: by0 + bh };
            br.SetColor(&if info.hovered == Some(TopBarPopoverHit::BatteryRow) { card_hov } else { card_bg });
            rt.FillRoundedRectangle(&D2D1_ROUNDED_RECT { rect: bat_card, radiusX: 11.0 * u, radiusY: 11.0 * u }, br);
            br.SetColor(&card_rim);
            rt.DrawRoundedRectangle(&D2D1_ROUNDED_RECT { rect: bat_card, radiusX: 11.0 * u, radiusY: 11.0 * u }, br, 0.9 * u, None);

            let bat_title = match info.bat_pct {
                Some(p) if info.bat_charging => format!("{}% • Charging", p),
                Some(p) => format!("{}% • Battery", p),
                None => "AC Power".to_string(),
            };
            br.SetColor(&rgba(ink.0, ink.1, ink.2, 0.94));
            draw_d2d_text(rt, &bat_title, tf_left, &D2D_RECT_F { left: bat_card.left + 10.0 * u, top: bat_card.top + 6.0 * u, right: bat_card.right - 8.0 * u, bottom: bat_card.top + 24.0 * u }, br);
            let bar_r = D2D_RECT_F {
                left: bat_card.left + 10.0 * u,
                top: bat_card.bottom - 15.0 * u,
                right: bat_card.right - 10.0 * u,
                bottom: bat_card.bottom - 8.0 * u,
            };
            br.SetColor(&if is_light { rgba(0, 0, 0, 0.12) } else { rgba(255, 255, 255, 0.16) });
            rt.FillRoundedRectangle(&D2D1_ROUNDED_RECT { rect: bar_r, radiusX: 3.5 * u, radiusY: 3.5 * u }, br);
            let bp = (info.bat_pct.unwrap_or(100) as f32 / 100.0).clamp(0.05, 1.0);
            let bfill_col = if info.bat_charging {
                rgba(52, 212, 108, 0.96)
            } else if info.bat_pct.unwrap_or(100) <= 20 {
                rgba(246, 86, 86, 0.96)
            } else {
                rgba(accent.0, accent.1, accent.2, 0.94)
            };
            br.SetColor(&bfill_col);
            rt.FillRoundedRectangle(
                &D2D1_ROUNDED_RECT {
                    rect: D2D_RECT_F { left: bar_r.left, top: bar_r.top, right: bar_r.left + (bar_r.right - bar_r.left) * bp, bottom: bar_r.bottom },
                    radiusX: 3.5 * u,
                    radiusY: 3.5 * u,
                },
                br,
            );

            br.SetColor(&if info.hovered == Some(TopBarPopoverHit::OpenQuickSettings) { card_hov } else { card_bg });
            rt.FillRoundedRectangle(&D2D1_ROUNDED_RECT { rect: qs_card, radiusX: 11.0 * u, radiusY: 11.0 * u }, br);
            br.SetColor(&card_rim);
            rt.DrawRoundedRectangle(&D2D1_ROUNDED_RECT { rect: qs_card, radiusX: 11.0 * u, radiusY: 11.0 * u }, br, 0.9 * u, None);
            br.SetColor(&rgba(ink.0, ink.1, ink.2, 0.94));
            draw_d2d_text(rt, "Quick Settings →", tf_bold, &qs_card, br);
        }
        TopBarPopoverKind::ClockCalendar => {
            // Large Time + Date Header
            let hy0 = pr.top + 12.0 * u;
            br.SetColor(&rgba(ink.0, ink.1, ink.2, 0.98));
            draw_d2d_text(rt, info.time_big, tf_bold, &D2D_RECT_F { left: ix, top: hy0, right: ir, bottom: hy0 + 26.0 * u }, br);
            br.SetColor(&rgba(ink_m.0, ink_m.1, ink_m.2, 0.88));
            draw_d2d_text(rt, info.date_full, tf_reg, &D2D_RECT_F { left: ix, top: hy0 + 24.0 * u, right: ir, bottom: hy0 + 44.0 * u }, br);

            let div_y = hy0 + 50.0 * u;
            br.SetColor(&card_rim);
            rt.DrawLine(Vector2 { X: ix, Y: div_y }, Vector2 { X: ir, Y: div_y }, br, 0.9 * u, None);

            // 7-column macOS Month Calendar Grid
            let col_w = iw / 7.0;
            let dow_y = div_y + 6.0 * u;
            let dows = ["Su", "Mo", "Tu", "We", "Th", "Fr", "Sa"];
            br.SetColor(&rgba(ink_m.0, ink_m.1, ink_m.2, 0.80));
            for (ci, &dl) in dows.iter().enumerate() {
                let x0 = ix + (ci as f32) * col_w;
                draw_d2d_text(rt, dl, tf_reg, &D2D_RECT_F { left: x0, top: dow_y, right: x0 + col_w, bottom: dow_y + 18.0 * u }, br);
            }
            let first_dow = weekday_sakamoto(info.year, info.month, 1);
            let total_days = days_in_month(info.year, info.month);
            let grid_y0 = dow_y + 20.0 * u;
            let cell_h = 22.0 * u;
            for day in 1..=total_days {
                let idx = (first_dow + day - 1) as usize;
                let row = idx / 7;
                let col = idx % 7;
                let x0 = ix + (col as f32) * col_w;
                let y0 = grid_y0 + (row as f32) * cell_h;
                let cell_r = D2D_RECT_F { left: x0, top: y0, right: x0 + col_w, bottom: y0 + cell_h };
                if day == info.today {
                    // Use vibrant macOS blue if theme accent is near-white/pale so "today" ALWAYS pops crisply
                    let max_c = accent.0.max(accent.1).max(accent.2);
                    let min_c = accent.0.min(accent.1).min(accent.2);
                    let accent_lum = 0.299 * (accent.0 as f32) + 0.587 * (accent.1 as f32) + 0.114 * (accent.2 as f32);
                    let pill_c = if max_c.saturating_sub(min_c) < 36 || accent_lum > 188.0 {
                        (30u8, 134u8, 255u8)
                    } else {
                        accent
                    };
                    let pill_lum = 0.299 * (pill_c.0 as f32) + 0.587 * (pill_c.1 as f32) + 0.114 * (pill_c.2 as f32);
                    br.SetColor(&rgba(pill_c.0, pill_c.1, pill_c.2, 0.96));
                    rt.FillEllipse(
                        &D2D1_ELLIPSE {
                            point: Vector2 { X: (cell_r.left + cell_r.right) * 0.5, Y: (cell_r.top + cell_r.bottom) * 0.5 },
                            radiusX: 10.0 * u,
                            radiusY: 10.0 * u,
                        },
                        br,
                    );
                    if pill_lum > 165.0 {
                        br.SetColor(&rgba(16, 20, 28, 0.98));
                    } else {
                        br.SetColor(&rgba(255, 255, 255, 0.98));
                    }
                    draw_d2d_text(rt, &day.to_string(), tf_bold, &cell_r, br);
                } else {
                    br.SetColor(&rgba(ink.0, ink.1, ink.2, 0.90));
                    draw_d2d_text(rt, &day.to_string(), tf_reg, &cell_r, br);
                }
            }

            // Bottom Notifications button
            let by1 = pr.bottom - 12.0 * u;
            let by0 = by1 - 30.0 * u;
            let nbtn = D2D_RECT_F { left: ix, top: by0, right: ir, bottom: by1 };
            let nrr = D2D1_ROUNDED_RECT { rect: nbtn, radiusX: 9.0 * u, radiusY: 9.0 * u };
            br.SetColor(&if info.hovered == Some(TopBarPopoverHit::OpenNotifications) { card_hov } else { card_bg });
            rt.FillRoundedRectangle(&nrr, br);
            br.SetColor(&card_rim);
            rt.DrawRoundedRectangle(&nrr, br, 0.9 * u, None);
            br.SetColor(&rgba(ink.0, ink.1, ink.2, 0.94));
            draw_d2d_text(rt, "Notifications & Alerts →", tf_bold, &nbtn, br);
        }
    }
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
