use std::collections::HashSet;
use std::ffi::c_void;
use std::mem::size_of;
use std::path::Path;
use std::sync::{Mutex, OnceLock};

use windows::core::{w, Interface, PCWSTR};
use windows::Win32::Foundation::SIZE;
use windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F;
use windows::Win32::Graphics::DirectWrite::{
    IDWriteFactory, IDWriteFontCollection, IDWriteFontFamily, IDWriteLocalizedStrings,
};
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, DeleteDC, DeleteObject, GetDIBits, GetObjectW, BITMAP, BITMAPINFO,
    BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, HGDIOBJ,
};
use windows::Win32::Graphics::Imaging::{
    CLSID_WICImagingFactory, GUID_WICPixelFormat32bppBGRA, IWICBitmapSource, IWICImagingFactory,
    WICBitmapInterpolationModeFant, WICDecodeMetadataCacheOnDemand,
};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED,
};
use windows::Win32::UI::Shell::{
    IShellItemImageFactory, SHCreateItemFromParsingName, SIIGBF_BIGGERSIZEOK, SIIGBF_THUMBNAILONLY,
};
use windows::Win32::UI::WindowsAndMessaging::{
    SystemParametersInfoW, SPI_GETDESKWALLPAPER, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS,
};

use crate::config::AppConfig;

#[inline]
pub fn rgb_to_d2d(rgb: (u8, u8, u8), alpha: f32) -> D2D1_COLOR_F {
    D2D1_COLOR_F {
        r: rgb.0 as f32 / 255.0,
        g: rgb.1 as f32 / 255.0,
        b: rgb.2 as f32 / 255.0,
        a: alpha.clamp(0.0, 1.0),
    }
}

#[inline]
pub fn u32_to_rgb(c: u32) -> (u8, u8, u8) {
    (((c >> 16) & 0xFF) as u8, ((c >> 8) & 0xFF) as u8, (c & 0xFF) as u8)
}

#[inline]
pub fn rgb_to_u32(rgb: (u8, u8, u8)) -> u32 {
    ((rgb.0 as u32) << 16) | ((rgb.1 as u32) << 8) | (rgb.2 as u32)
}

pub fn blend_rgb(a: (u8, u8, u8), b: (u8, u8, u8), t: f32) -> (u8, u8, u8) {
    let k = t.clamp(0.0, 1.0);
    (
        (a.0 as f32 * (1.0 - k) + b.0 as f32 * k).round() as u8,
        (a.1 as f32 * (1.0 - k) + b.1 as f32 * k).round() as u8,
        (a.2 as f32 * (1.0 - k) + b.2 as f32 * k).round() as u8,
    )
}

pub fn rgb_to_hsl(rgb: (u8, u8, u8)) -> (f32, f32, f32) {
    let r = rgb.0 as f32 / 255.0;
    let g = rgb.1 as f32 / 255.0;
    let b = rgb.2 as f32 / 255.0;
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let l = (max + min) * 0.5;
    let d = max - min;
    if d < 1e-5 {
        return (0.0, 0.0, l);
    }
    let s = if l > 0.5 { d / (2.0 - max - min) } else { d / (max + min) };
    let mut h = if (max - r).abs() < 1e-5 {
        (g - b) / d + if g < b { 6.0 } else { 0.0 }
    } else if (max - g).abs() < 1e-5 {
        (b - r) / d + 2.0
    } else {
        (r - g) / d + 4.0
    };
    h /= 6.0;
    (h * 360.0, s, l)
}

pub fn hsl_to_rgb(h_deg: f32, s: f32, l: f32) -> (u8, u8, u8) {
    let h = ((h_deg % 360.0) + 360.0) % 360.0 / 360.0;
    let s = s.clamp(0.0, 1.0);
    let l = l.clamp(0.0, 1.0);
    if s < 1e-5 {
        let v = (l * 255.0).round() as u8;
        return (v, v, v);
    }
    let q = if l < 0.5 { l * (1.0 + s) } else { l + s - l * s };
    let p = 2.0 * l - q;
    let hue2rgb = |mut t: f32| -> f32 {
        if t < 0.0 {
            t += 1.0;
        }
        if t > 1.0 {
            t -= 1.0;
        }
        if t < 1.0 / 6.0 {
            return p + (q - p) * 6.0 * t;
        }
        if t < 0.5 {
            return q;
        }
        if t < 2.0 / 3.0 {
            return p + (q - p) * (2.0 / 3.0 - t) * 6.0;
        }
        p
    };
    (
        (hue2rgb(h + 1.0 / 3.0) * 255.0).round().clamp(0.0, 255.0) as u8,
        (hue2rgb(h) * 255.0).round().clamp(0.0, 255.0) as u8,
        (hue2rgb(h - 1.0 / 3.0) * 255.0).round().clamp(0.0, 255.0) as u8,
    )
}

#[inline]
fn srgb_u8_to_linear(c: u8) -> f32 {
    let v = c as f32 / 255.0;
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

#[inline]
fn linear_to_srgb_u8(x: f32) -> u8 {
    let v = x.clamp(0.0, 1.0);
    let s = if v <= 0.0031308 {
        v * 12.92
    } else {
        1.055 * v.powf(1.0 / 2.4) - 0.055
    };
    (s * 255.0).round().clamp(0.0, 255.0) as u8
}

/// Converts sRGB `(r, g, b)` to Björn Ottosson's perceptual OKLab `(L, a, b)`.
pub fn rgb_to_oklab(rgb: (u8, u8, u8)) -> (f32, f32, f32) {
    let r = srgb_u8_to_linear(rgb.0);
    let g = srgb_u8_to_linear(rgb.1);
    let b = srgb_u8_to_linear(rgb.2);

    let l = (0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b).max(0.0).cbrt();
    let m = (0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b).max(0.0).cbrt();
    let s = (0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b).max(0.0).cbrt();

    let ok_l = 0.2104542553 * l + 0.7936177850 * m - 0.0040720468 * s;
    let ok_a = 1.9779984951 * l - 2.4285922050 * m + 0.4505937099 * s;
    let ok_b = 0.0259040371 * l + 0.7827717662 * m - 0.8086757660 * s;
    (ok_l, ok_a, ok_b)
}

#[inline]
fn oklab_to_linear_rgb(ok_l: f32, ok_a: f32, ok_b: f32) -> (f32, f32, f32) {
    let l_ = ok_l + 0.3963377774 * ok_a + 0.2158037573 * ok_b;
    let m_ = ok_l - 0.1055613458 * ok_a - 0.0638541728 * ok_b;
    let s_ = ok_l - 0.0894841775 * ok_a - 1.2914855480 * ok_b;

    let l = l_ * l_ * l_;
    let m = m_ * m_ * m_;
    let s = s_ * s_ * s_;

    let r = 4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s;
    let g = -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s;
    let b = -0.0041960863 * l - 0.7034186147 * m + 1.7076147010 * s;
    (r, g, b)
}

#[inline]
fn in_srgb_gamut(r: f32, g: f32, b: f32) -> bool {
    const EPS: f32 = 0.0005;
    r >= -EPS && r <= 1.0 + EPS && g >= -EPS && g <= 1.0 + EPS && b >= -EPS && b <= 1.0 + EPS
}

/// Converts OKLab `(L, a, b)` to sRGB `(r, g, b)` with gamut-preserving chroma clamp.
pub fn oklab_to_rgb(ok_l: f32, ok_a: f32, ok_b: f32) -> (u8, u8, u8) {
    let l = ok_l.clamp(0.0, 1.0);
    let (r, g, b) = oklab_to_linear_rgb(l, ok_a, ok_b);
    if in_srgb_gamut(r, g, b) {
        return (linear_to_srgb_u8(r), linear_to_srgb_u8(g), linear_to_srgb_u8(b));
    }
    let mut lo = 0.0f32;
    let mut hi = 1.0f32;
    for _ in 0..10 {
        let mid = 0.5 * (lo + hi);
        let (mr, mg, mb) = oklab_to_linear_rgb(l, ok_a * mid, ok_b * mid);
        if in_srgb_gamut(mr, mg, mb) {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    let (fr, fg, fb) = oklab_to_linear_rgb(l, ok_a * lo, ok_b * lo);
    (linear_to_srgb_u8(fr), linear_to_srgb_u8(fg), linear_to_srgb_u8(fb))
}

/// Converts OKLab `(L, a, b)` to cylindrical OKLCH `(L, C, h_deg)` with `h_deg` in `0..360`.
#[inline]
pub fn oklab_to_oklch(ok_l: f32, ok_a: f32, ok_b: f32) -> (f32, f32, f32) {
    let c = (ok_a * ok_a + ok_b * ok_b).sqrt();
    let h = ok_b.atan2(ok_a).to_degrees().rem_euclid(360.0);
    (ok_l, c, h)
}

/// Converts OKLCH `(L, C, h_deg)` into exact gamut-safe sRGB without hue distortion.
#[inline]
pub fn oklch_to_rgb(ok_l: f32, ok_c: f32, h_deg: f32) -> (u8, u8, u8) {
    let rad = h_deg.to_radians();
    let c = ok_c.max(0.0);
    oklab_to_rgb(ok_l, c * rad.cos(), c * rad.sin())
}

/// Perceptually uniform OKLab interpolation between two sRGB colors (eliminates muddy grey midpoints).
pub fn blend_oklab(a: (u8, u8, u8), b: (u8, u8, u8), t: f32) -> (u8, u8, u8) {
    let k = t.clamp(0.0, 1.0);
    if k <= 0.0 {
        return a;
    }
    if k >= 1.0 {
        return b;
    }
    let (l1, a1, b1) = rgb_to_oklab(a);
    let (l2, a2, b2) = rgb_to_oklab(b);
    oklab_to_rgb(
        l1 + (l2 - l1) * k,
        a1 + (a2 - a1) * k,
        b1 + (b2 - b1) * k,
    )
}

/// WCAG 2.1 relative luminance (`0.0..=1.0`).
#[inline]
pub fn wcag_relative_luminance(rgb: (u8, u8, u8)) -> f32 {
    0.2126 * srgb_u8_to_linear(rgb.0)
        + 0.7152 * srgb_u8_to_linear(rgb.1)
        + 0.0722 * srgb_u8_to_linear(rgb.2)
}

/// WCAG 2.1 contrast ratio (`1.0..=21.0`).
#[inline]
pub fn wcag_contrast_ratio(a: (u8, u8, u8), b: (u8, u8, u8)) -> f32 {
    let la = wcag_relative_luminance(a);
    let lb = wcag_relative_luminance(b);
    let (hi, lo) = if la >= lb { (la, lb) } else { (lb, la) };
    (hi + 0.05) / (lo + 0.05)
}

/// Adjusts `fg` in OKLCH lightness (preserving hue) until WCAG contrast against `bg` >= `min_ratio`.
fn ensure_contrast_oklch(
    fg: (u8, u8, u8),
    bg: (u8, u8, u8),
    min_ratio: f32,
    prefer_lighter: bool,
) -> (u8, u8, u8) {
    if wcag_contrast_ratio(fg, bg) >= min_ratio {
        return fg;
    }
    let (mut l, c, h) = {
        let (ol, oa, ob) = rgb_to_oklab(fg);
        oklab_to_oklch(ol, oa, ob)
    };
    let step = if prefer_lighter { 0.04 } else { -0.04 };
    let mut cur = fg;
    for _ in 0..16 {
        l = (l + step).clamp(0.06, 0.98);
        cur = oklch_to_rgb(l, c * (1.0 - (l - 0.5).abs() * 0.45).max(0.15), h);
        if wcag_contrast_ratio(cur, bg) >= min_ratio {
            return cur;
        }
    }
    cur
}

#[inline]
fn hue_dist_deg(a: f32, b: f32) -> f32 {
    let d = (a - b).abs().rem_euclid(360.0);
    d.min(360.0 - d)
}

/// Classifies the perceptual OKLCH hue structure of a wallpaper.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ColorHarmonyMood {
    Monochrome,
    Analogous,
    Complementary,
    Triadic,
    VibrantMulti,
}

impl ColorHarmonyMood {
    #[inline]
    pub fn label(self) -> &'static str {
        match self {
            ColorHarmonyMood::Monochrome => "Monochrome",
            ColorHarmonyMood::Analogous => "Analogous",
            ColorHarmonyMood::Complementary => "Complementary",
            ColorHarmonyMood::Triadic => "Triadic",
            ColorHarmonyMood::VibrantMulti => "Vibrant Multi",
        }
    }
}

/// Rich perceptual + spatial + visual-saliency wallpaper color profile extracted via 8-cluster
/// OKLab K-Means++. Drives edge-adaptive glass tinting, multi-hue accents, WCAG contrast, and
/// light/dark atmospheric harmony across the Dock, Top Menu Bar, Keycaps, Taskbar, and Widgets.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct WallpaperColorProfile {
    /// Global perceptual lightness in OKLab (`0.0` pitch dark .. `1.0` bright paper/sky).
    pub scene_lightness: f32,
    /// Mean perceptual OKLCH chroma across the scene (`0.0` grayscale .. `~0.28` vivid neon).
    pub scene_chroma: f32,
    /// Dynamic lightness spread (`p90 - p10` in OKLab `L`), capturing noir vs flat pastel contrast.
    pub dynamic_range: f32,
    /// True when the wallpaper has an airy/bright daylight mood (`scene_lightness >= 0.60`).
    pub is_light_scene: bool,
    /// Perceptual warmth (`-1.0` icy cool .. `+1.0` warm golden/sunset).
    pub warmth: f32,
    /// Classified multi-hue harmony relationship extracted from the scene clusters.
    pub harmony: ColorHarmonyMood,
    /// Primary focal accent color (WCAG >= 4.6:1 on dark glass).
    pub primary_accent: (u8, u8, u8),
    /// Distinct secondary harmonic color extracted from the wallpaper (or analogous companion).
    pub secondary_accent: (u8, u8, u8),
    /// Third distinct hue / micro-accent extracted from the wallpaper (or triadic companion).
    pub tertiary_accent: (u8, u8, u8),
    /// Deeper saturated variant of `primary_accent` guaranteed WCAG >= 4.5:1 over light glass.
    pub on_light_accent: (u8, u8, u8),
    /// True dominant ambient atmosphere color of the wallpaper background.
    pub ambient_tint: (u8, u8, u8),
    /// Perceptual averages of all 4 screen-edge strips (Top Bar, Bottom Dock, Left/Right Docks).
    pub top_zone_rgb: (u8, u8, u8),
    pub bottom_zone_rgb: (u8, u8, u8),
    pub left_zone_rgb: (u8, u8, u8),
    pub right_zone_rgb: (u8, u8, u8),
    /// Airy wallpaper-tinted glass gradient for light glass surfaces (`OS26 Liquid Glass Light`, etc.).
    pub light_glass_top: (u8, u8, u8),
    pub light_glass_bot: (u8, u8, u8),
    /// Deep atmospheric wallpaper-tinted glass gradient for dark glass surfaces.
    pub dark_glass_top: (u8, u8, u8),
    pub dark_glass_bot: (u8, u8, u8),
}

impl Default for WallpaperColorProfile {
    fn default() -> Self {
        let a = ThemePalette::autumn();
        Self {
            scene_lightness: 0.25,
            scene_chroma: 0.09,
            dynamic_range: 0.40,
            is_light_scene: false,
            warmth: 0.45,
            harmony: ColorHarmonyMood::Analogous,
            primary_accent: a.accent,
            secondary_accent: a.accent_light,
            tertiary_accent: (242, 198, 124),
            on_light_accent: (168, 86, 16),
            ambient_tint: (46, 31, 22),
            top_zone_rgb: (56, 38, 26),
            bottom_zone_rgb: (28, 20, 16),
            left_zone_rgb: (42, 28, 20),
            right_zone_rgb: (42, 28, 20),
            light_glass_top: (244, 238, 230),
            light_glass_bot: (232, 222, 208),
            dark_glass_top: a.glass_top,
            dark_glass_bot: a.glass_bot,
        }
    }
}

impl WallpaperColorProfile {
    /// Returns the extracted wallpaper color right behind `edge` (`Bottom`, `Top`, `Left`, `Right`).
    #[inline]
    pub fn edge_zone_rgb(self, edge: crate::config::MacDockEdge) -> (u8, u8, u8) {
        match edge {
            crate::config::MacDockEdge::Top => self.top_zone_rgb,
            crate::config::MacDockEdge::Bottom => self.bottom_zone_rgb,
            crate::config::MacDockEdge::Left => self.left_zone_rgb,
            crate::config::MacDockEdge::Right => self.right_zone_rgb,
        }
    }

    /// Computes a rich, edge-aware 3-stop `(top, mid, bot)` glass card tint tailored to the
    /// exact screen edge (`Bottom`/`Top`/`Left`/`Right`) and surface mode (`is_light`).
    pub fn glass_gradient_for_edge(
        self,
        edge: crate::config::MacDockEdge,
        is_light: bool,
    ) -> ((u8, u8, u8), (u8, u8, u8), (u8, u8, u8)) {
        let zone = self.edge_zone_rgb(edge);
        let (_, zc, zh) = {
            let (l, a, b) = rgb_to_oklab(zone);
            oklab_to_oklch(l, a, b)
        };
        let (_, pc, ph) = {
            let (l, a, b) = rgb_to_oklab(self.primary_accent);
            oklab_to_oklch(l, a, b)
        };
        let (_, sc, sh) = {
            let (l, a, b) = rgb_to_oklab(self.secondary_accent);
            oklab_to_oklch(l, a, b)
        };
        let mono = self.harmony == ColorHarmonyMood::Monochrome;
        let edge_h = if zc >= 0.032 { zh } else { ph };
        let sec_h = if sc >= 0.038 { sh } else { edge_h };

        if is_light {
            let c_hi = if mono {
                0.008
            } else {
                (zc.max(pc * 0.55) * 0.42).clamp(0.026, 0.068)
            };
            let c_lo = if mono {
                0.008
            } else {
                (zc.max(sc * 0.48) * 0.46).clamp(0.030, 0.078)
            };
            let gt = oklch_to_rgb(0.935, c_hi, edge_h);
            let gb = oklch_to_rgb(0.865, c_lo, sec_h);
            let gm = blend_oklab(gt, gb, 0.48);
            (gt, gm, gb)
        } else {
            let c_hi = if mono {
                0.010
            } else {
                (zc.max(pc * 0.58) * 0.54).clamp(0.038, 0.102)
            };
            let c_lo = if mono {
                0.008
            } else {
                (zc.max(sc * 0.50) * 0.48).clamp(0.032, 0.088)
            };
            let l_hi = if self.is_light_scene { 0.235 } else { 0.195 };
            let l_lo = if self.is_light_scene { 0.165 } else { 0.130 };
            let gt = oklch_to_rgb(l_hi, c_hi, edge_h);
            let gb = oklch_to_rgb(l_lo, c_lo, sec_h);
            let gm = blend_oklab(gt, gb, 0.52);
            (gt, gm, gb)
        }
    }

    /// Concise scene classification label shown in Settings UI (e.g., `"Complementary • Warm"`).
    pub fn mood_tag(self) -> String {
        let tone = if self.harmony == ColorHarmonyMood::Monochrome {
            if self.is_light_scene { "Light" } else { "Noir" }
        } else if self.warmth >= 0.18 {
            "Warm"
        } else if self.warmth <= -0.18 {
            "Cool"
        } else if self.is_light_scene {
            "Daylight"
        } else {
            "Balanced"
        };
        format!("{} • {}", self.harmony.label(), tone)
    }

    pub fn lerp(self, target: Self, t: f32) -> Self {
        let k = t.clamp(0.0, 1.0);
        if k <= 0.0 {
            return self;
        }
        if k >= 1.0 {
            return target;
        }
        let scene_l = self.scene_lightness + (target.scene_lightness - self.scene_lightness) * k;
        Self {
            scene_lightness: scene_l,
            scene_chroma: self.scene_chroma + (target.scene_chroma - self.scene_chroma) * k,
            dynamic_range: self.dynamic_range + (target.dynamic_range - self.dynamic_range) * k,
            is_light_scene: scene_l >= 0.60,
            warmth: self.warmth + (target.warmth - self.warmth) * k,
            harmony: if k >= 0.5 { target.harmony } else { self.harmony },
            primary_accent: blend_oklab(self.primary_accent, target.primary_accent, k),
            secondary_accent: blend_oklab(self.secondary_accent, target.secondary_accent, k),
            tertiary_accent: blend_oklab(self.tertiary_accent, target.tertiary_accent, k),
            on_light_accent: blend_oklab(self.on_light_accent, target.on_light_accent, k),
            ambient_tint: blend_oklab(self.ambient_tint, target.ambient_tint, k),
            top_zone_rgb: blend_oklab(self.top_zone_rgb, target.top_zone_rgb, k),
            bottom_zone_rgb: blend_oklab(self.bottom_zone_rgb, target.bottom_zone_rgb, k),
            left_zone_rgb: blend_oklab(self.left_zone_rgb, target.left_zone_rgb, k),
            right_zone_rgb: blend_oklab(self.right_zone_rgb, target.right_zone_rgb, k),
            light_glass_top: blend_oklab(self.light_glass_top, target.light_glass_top, k),
            light_glass_bot: blend_oklab(self.light_glass_bot, target.light_glass_bot, k),
            dark_glass_top: blend_oklab(self.dark_glass_top, target.dark_glass_top, k),
            dark_glass_bot: blend_oklab(self.dark_glass_bot, target.dark_glass_bot, k),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ThemePalette {
    pub accent: (u8, u8, u8),
    pub accent_light: (u8, u8, u8),
    pub accent_deep: (u8, u8, u8),
    pub rim: (u8, u8, u8),
    pub text_primary: (u8, u8, u8),
    pub text_muted: (u8, u8, u8),
    pub glass_top: (u8, u8, u8),
    pub glass_bot: (u8, u8, u8),
    pub preview_a: (u8, u8, u8),
    pub preview_b: (u8, u8, u8),
    pub preview_c: (u8, u8, u8),
    pub swatches: [u32; 5],
}

impl ThemePalette {
    pub const fn autumn() -> Self {
        Self {
            accent: (245, 166, 66),
            accent_light: (255, 206, 112),
            accent_deep: (224, 122, 34),
            rim: (245, 166, 66),
            text_primary: (255, 243, 220),
            text_muted: (232, 210, 182),
            glass_top: (18, 14, 12),
            glass_bot: (12, 9, 8),
            preview_a: (46, 26, 14),
            preview_b: (84, 43, 17),
            preview_c: (28, 17, 12),
            swatches: [0xF5A642, 0xFFCE70, 0xE07A22, 0x542B11, 0x120E0C],
        }
    }

    /// Curated artistic native palettes for the 6 Music Taskbar Themes (eliminating monochrome amber).
    pub fn for_music_taskbar_theme(theme: crate::config::TaskbarTheme) -> Option<Self> {
        use crate::config::TaskbarTheme;
        match theme {
            // Cyber Neon Cyan & Deep Violet
            TaskbarTheme::LiveMusicBars => Some(Self {
                accent: (0, 232, 255),
                accent_light: (158, 248, 255),
                accent_deep: (132, 56, 255),
                rim: (0, 232, 255),
                text_primary: (238, 252, 255),
                text_muted: (174, 214, 240),
                glass_top: (14, 16, 36),
                glass_bot: (8, 9, 24),
                preview_a: (16, 12, 42),
                preview_b: (48, 22, 92),
                preview_c: (8, 18, 38),
                swatches: [0x00E8FF, 0x9EF8FF, 0x8438FF, 0x30165C, 0x080918],
            }),
            // Synthwave Magenta & Amber Orange
            TaskbarTheme::NeonWave => Some(Self {
                accent: (255, 56, 168),
                accent_light: (255, 168, 70),
                accent_deep: (166, 36, 206),
                rim: (255, 92, 174),
                text_primary: (255, 242, 250),
                text_muted: (238, 186, 220),
                glass_top: (28, 12, 34),
                glass_bot: (16, 7, 22),
                preview_a: (38, 12, 44),
                preview_b: (98, 28, 66),
                preview_c: (26, 12, 32),
                swatches: [0xFF38A8, 0xFFA846, 0xA624CE, 0x621C42, 0x100716],
            }),
            // Frost Ice White & Aquamarine
            TaskbarTheme::PureClearPulse => Some(Self {
                accent: (76, 240, 222),
                accent_light: (240, 252, 255),
                accent_deep: (26, 158, 184),
                rim: (192, 250, 255),
                text_primary: (244, 254, 255),
                text_muted: (184, 232, 238),
                glass_top: (12, 30, 38),
                glass_bot: (8, 18, 26),
                preview_a: (10, 32, 46),
                preview_b: (26, 78, 96),
                preview_c: (8, 22, 34),
                swatches: [0x4CF0DE, 0xF0FCFF, 0x1A9EB8, 0x1A4E60, 0x08121A],
            }),
            // Polished Metallic Chrome & Vivid Indigo
            TaskbarTheme::MirrorBeatPill => Some(Self {
                accent: (122, 134, 255),
                accent_light: (234, 242, 255),
                accent_deep: (68, 56, 214),
                rim: (206, 220, 248),
                text_primary: (246, 249, 255),
                text_muted: (188, 198, 234),
                glass_top: (18, 20, 38),
                glass_bot: (10, 12, 24),
                preview_a: (18, 20, 44),
                preview_b: (48, 52, 108),
                preview_c: (12, 14, 28),
                swatches: [0x7A86FF, 0xEAF2FF, 0x4438D6, 0x30346C, 0x0A0C18],
            }),
            // Electric Lime & Midnight Charcoal
            TaskbarTheme::BassPulseSplit => Some(Self {
                accent: (140, 255, 58),
                accent_light: (218, 255, 138),
                accent_deep: (44, 182, 56),
                rim: (152, 255, 76),
                text_primary: (244, 255, 238),
                text_muted: (184, 216, 174),
                glass_top: (16, 20, 18),
                glass_bot: (10, 12, 11),
                preview_a: (12, 18, 14),
                preview_b: (24, 52, 26),
                preview_c: (8, 12, 10),
                swatches: [0x8CFF3A, 0xDAFF8A, 0x2CB638, 0x18341A, 0x0A0C0B],
            }),
            // Studio Tri-Color LED Gradient (Green -> Yellow -> Peak Red)
            TaskbarTheme::VuPeakStudio => Some(Self {
                accent: (52, 232, 108),
                accent_light: (255, 218, 56),
                accent_deep: (255, 58, 68),
                rim: (68, 232, 126),
                text_primary: (244, 254, 248),
                text_muted: (176, 214, 192),
                glass_top: (14, 18, 18),
                glass_bot: (8, 10, 11),
                preview_a: (12, 22, 18),
                preview_b: (26, 48, 34),
                preview_c: (10, 12, 14),
                swatches: [0x34E86C, 0xFFDA38, 0xFF3A44, 0x1A3022, 0x080A0B],
            }),
            _ => None,
        }
    }

    /// Builds a custom visualizer accent palette while preserving the theme's dark glass base.
    pub fn with_custom_viz_accent(rgb_u32: u32, base: &Self) -> Self {
        let mut p = Self::from_accent_rgb(rgb_u32);
        p.glass_top = blend_rgb(base.glass_top, p.glass_top, 0.45);
        p.glass_bot = blend_rgb(base.glass_bot, p.glass_bot, 0.45);
        p
    }

    #[allow(dead_code)]
    pub const REALISTIC_THEME_IDS: [&'static str; 10] = [
        "calm_ocean_lagoon",
        "rainy_windowpane",
        "brushed_titanium_hifi",
        "vintage_walnut_brass",
        "deep_abyssal_trench",
        "nordic_glacial_ice",
        "studio_executive_leather",
        "alpine_forest_mist",
        "industrial_carbon_steel",
        "golden_hour_horizon",
    ];

    /// Exact physical material palettes for the 10 True Realism themes.
    #[allow(dead_code)]
    pub fn for_realistic_theme(id: &str) -> Option<Self> {
        let p = match id.trim().to_ascii_lowercase().as_str() {
            "calm_ocean_lagoon" | "real_crystal_water" => Self {
                accent: (0x00, 0xB4, 0xD8),
                accent_light: (0x90, 0xE0, 0xEF),
                accent_deep: (0x00, 0x77, 0xB6),
                rim: (0x90, 0xE0, 0xEF),
                text_primary: (0xCA, 0xF0, 0xF8),
                text_muted: (0x90, 0xE0, 0xEF),
                glass_top: (0x00, 0x77, 0xB6),
                glass_bot: (0x06, 0x18, 0x26),
                preview_a: (0x06, 0x18, 0x26),
                preview_b: (0x00, 0x77, 0xB6),
                preview_c: (0x00, 0xB4, 0xD8),
                swatches: [0x00B4D8, 0x90E0EF, 0x0077B6, 0xCAF0F8, 0x061826],
            },
            "rainy_windowpane" | "real_rain_window" => Self {
                accent: (0x8A, 0x9E, 0xA7),
                accent_light: (0xE6, 0xEC, 0xEF),
                accent_deep: (0x2C, 0x35, 0x39),
                rim: (0xE6, 0xEC, 0xEF),
                text_primary: (0xE6, 0xEC, 0xEF),
                text_muted: (0x8A, 0x9E, 0xA7),
                glass_top: (0x2C, 0x35, 0x39),
                glass_bot: (0x1A, 0x1D, 0x20),
                preview_a: (0x1A, 0x1D, 0x20),
                preview_b: (0x2C, 0x35, 0x39),
                preview_c: (0x8A, 0x9E, 0xA7),
                swatches: [0x8A9EA7, 0xE6ECEF, 0x2C3539, 0x1A1D20, 0x4E5C64],
            },
            "brushed_titanium_hifi" | "real_brushed_titanium" | "real_vacuum_tube_hifi" => Self {
                accent: (0xFF, 0xA7, 0x26),
                accent_light: (0xDC, 0xE1, 0xE3),
                accent_deep: (0xEF, 0x53, 0x50),
                rim: (0xDC, 0xE1, 0xE3),
                text_primary: (0xDC, 0xE1, 0xE3),
                text_muted: (0x7A, 0x82, 0x88),
                glass_top: (0x36, 0x3A, 0x42),
                glass_bot: (0x1C, 0x1E, 0x22),
                preview_a: (0x1C, 0x1E, 0x22),
                preview_b: (0x7A, 0x82, 0x88),
                preview_c: (0x2B, 0x2F, 0x36),
                swatches: [0xFFA726, 0xDCE1E3, 0x7A8288, 0xEF5350, 0x1C1E22],
            },
            "vintage_walnut_brass" | "real_warm_walnut" | "real_swiss_horology" => Self {
                accent: (0xD4, 0xAF, 0x37),
                accent_light: (0xF3, 0xE5, 0xAB),
                accent_deep: (0x1B, 0x3B, 0x6F),
                rim: (0xD4, 0xAF, 0x37),
                text_primary: (0xF3, 0xE5, 0xAB),
                text_muted: (0xC9, 0xA9, 0x6E),
                glass_top: (0x4A, 0x2E, 0x12),
                glass_bot: (0x23, 0x17, 0x09),
                preview_a: (0x23, 0x17, 0x09),
                preview_b: (0x4A, 0x2E, 0x12),
                preview_c: (0x1B, 0x3B, 0x6F),
                swatches: [0xD4AF37, 0xF3E5AB, 0x4A2E12, 0x1B3B6F, 0x231709],
            },
            "deep_abyssal_trench" | "real_abyssal_trench" => Self {
                accent: (0x00, 0xFF, 0xCC),
                accent_light: (0xA8, 0xFF, 0xEE),
                accent_deep: (0x00, 0x72, 0xBB),
                rim: (0x00, 0xFF, 0xCC),
                text_primary: (0xE0, 0xFF, 0xF8),
                text_muted: (0x7C, 0xC6, 0xD8),
                glass_top: (0x08, 0x1C, 0x30),
                glass_bot: (0x02, 0x0B, 0x14),
                preview_a: (0x02, 0x0B, 0x14),
                preview_b: (0x08, 0x1C, 0x30),
                preview_c: (0x00, 0x72, 0xBB),
                swatches: [0x00FFCC, 0x0072BB, 0x081C30, 0x020B14, 0xA8FFEE],
            },
            "nordic_glacial_ice" | "real_glacier_ice" => Self {
                accent: (0xA0, 0xE7, 0xE5),
                accent_light: (0xE0, 0xF4, 0xFF),
                accent_deep: (0x1E, 0x3E, 0x62),
                rim: (0xE0, 0xF4, 0xFF),
                text_primary: (0xE0, 0xF4, 0xFF),
                text_muted: (0xA0, 0xE7, 0xE5),
                glass_top: (0x1E, 0x3E, 0x62),
                glass_bot: (0x0B, 0x19, 0x2C),
                preview_a: (0x0B, 0x19, 0x2C),
                preview_b: (0x1E, 0x3E, 0x62),
                preview_c: (0xA0, 0xE7, 0xE5),
                swatches: [0xE0F4FF, 0xA0E7E5, 0x1E3E62, 0x0B192C, 0x7AD8F5],
            },
            "studio_executive_leather" | "real_saddle_leather" => Self {
                accent: (0xE5, 0xB8, 0x69),
                accent_light: (0xF7, 0xF4, 0xEB),
                accent_deep: (0x2B, 0x18, 0x10),
                rim: (0xE5, 0xB8, 0x69),
                text_primary: (0xF7, 0xF4, 0xEB),
                text_muted: (0xD4, 0xBC, 0x98),
                glass_top: (0x2B, 0x18, 0x10),
                glass_bot: (0x14, 0x0A, 0x07),
                preview_a: (0x14, 0x0A, 0x07),
                preview_b: (0x2B, 0x18, 0x10),
                preview_c: (0xE5, 0xB8, 0x69),
                swatches: [0xE5B869, 0xF7F4EB, 0x2B1810, 0x140A07, 0x212121],
            },
            "alpine_forest_mist" | "real_forest_sunbeams" | "real_zen_calm_mist" => Self {
                accent: (0xA3, 0xB1, 0x8A),
                accent_light: (0xDA, 0xD7, 0xCD),
                accent_deep: (0x0B, 0x1D, 0x13),
                rim: (0xA3, 0xB1, 0x8A),
                text_primary: (0xDA, 0xD7, 0xCD),
                text_muted: (0xA3, 0xB1, 0x8A),
                glass_top: (0x1C, 0x28, 0x26),
                glass_bot: (0x0B, 0x1D, 0x13),
                preview_a: (0x0B, 0x1D, 0x13),
                preview_b: (0x1C, 0x28, 0x26),
                preview_c: (0xA3, 0xB1, 0x8A),
                swatches: [0xA3B18A, 0xDAD7CD, 0x1C2826, 0x0B1D13, 0x588157],
            },
            "industrial_carbon_steel" | "real_obsidian_carbon" => Self {
                accent: (0xFF, 0x6B, 0x35),
                accent_light: (0x8D, 0x99, 0xAE),
                accent_deep: (0x1E, 0x1E, 0x1E),
                rim: (0x8D, 0x99, 0xAE),
                text_primary: (0xED, 0xF2, 0xF4),
                text_muted: (0x8D, 0x99, 0xAE),
                glass_top: (0x1E, 0x1E, 0x1E),
                glass_bot: (0x12, 0x12, 0x12),
                preview_a: (0x12, 0x12, 0x12),
                preview_b: (0x1E, 0x1E, 0x1E),
                preview_c: (0x8D, 0x99, 0xAE),
                swatches: [0xFF6B35, 0x8D99AE, 0x1E1E1E, 0x121212, 0xEDF2F4],
            },
            "golden_hour_horizon" | "real_golden_hour_coast" => Self {
                accent: (0xFF, 0x9E, 0x00),
                accent_light: (0xFF, 0xD1, 0x66),
                accent_deep: (0x4E, 0x34, 0x2E),
                rim: (0xFF, 0xD1, 0x66),
                text_primary: (0xFF, 0xE3, 0xD8),
                text_muted: (0xFF, 0xD1, 0x66),
                glass_top: (0x4E, 0x34, 0x2E),
                glass_bot: (0x2B, 0x09, 0x38),
                preview_a: (0x2B, 0x09, 0x38),
                preview_b: (0x4E, 0x34, 0x2E),
                preview_c: (0xFF, 0x9E, 0x00),
                swatches: [0xFF9E00, 0xFFD166, 0xFFE3D8, 0x4E342E, 0x2B0938],
            },
            _ => return None,
        };
        Some(p)
    }

    fn for_pbr_material(mat: crate::realistic_pbr::PbrMaterial) -> Option<Self> {
        use crate::realistic_pbr::PbrMaterial;
        let key = match mat {
            PbrMaterial::CalmOceanLagoon => "calm_ocean_lagoon",
            PbrMaterial::RainyWindowpane => "rainy_windowpane",
            PbrMaterial::BrushedTitanium => "brushed_titanium_hifi",
            PbrMaterial::VintageWalnutBrass => "vintage_walnut_brass",
            PbrMaterial::DeepAbyssalTrench => "deep_abyssal_trench",
            PbrMaterial::NordicGlacialIce => "nordic_glacial_ice",
            PbrMaterial::ExecutiveLeatherLinen
            | PbrMaterial::CarraraMarbleGold
            | PbrMaterial::WovenLinenCalm => "studio_executive_leather",
            PbrMaterial::AlpineForestMist => "alpine_forest_mist",
            PbrMaterial::IndustrialCarbonSteel => "industrial_carbon_steel",
            PbrMaterial::GoldenHourHorizon => "golden_hour_horizon",
        };
        Self::for_realistic_theme(key)
    }

    /// Photorealistic Haute Horlogerie native palette matching each `HorologyTheme`.
    pub fn for_horology(ht: crate::horology_pbr::HorologyTheme) -> Self {
        let (bez, dial, hands, lume, jewel) = ht.default_colors();
        let light = ht.is_light_dial();
        let (br, bg, bb) = u32_to_rgb(bez);
        let (dr, dg, db) = u32_to_rgb(dial);
        let (hr, hg, hb) = u32_to_rgb(hands);
        let (lr, lg, lb) = u32_to_rgb(lume);
        let (jr, jg, jb) = u32_to_rgb(jewel);
        let (dr_bot, dg_bot, db_bot) = if light {
            (dr.saturating_sub(18), dg.saturating_sub(18), db.saturating_sub(20))
        } else {
            (dr / 2, dg / 2, db / 2)
        };
        let (tp, tm) = if light {
            ((24, 32, 44), (82, 96, 114))
        } else {
            ((242, 246, 252), (168, 182, 202))
        };
        let (pa, pb, pc) = if light {
            ((216, 212, 204), (242, 238, 228), (192, 186, 176))
        } else {
            (
                (dr.saturating_add(6), dg.saturating_add(8), db.saturating_add(14)),
                (
                    dr.saturating_add((lr / 5).min(48)),
                    dg.saturating_add((lg / 5).min(48)),
                    db.saturating_add((lb / 5).min(48)),
                ),
                (dr_bot, dg_bot, db_bot),
            )
        };
        Self {
            accent: (lr, lg, lb),
            accent_light: (hr, hg, hb),
            accent_deep: (jr, jg, jb),
            rim: (br, bg, bb),
            text_primary: tp,
            text_muted: tm,
            glass_top: (dr, dg, db),
            glass_bot: (dr_bot, dg_bot, db_bot),
            preview_a: pa,
            preview_b: pb,
            preview_c: pc,
            swatches: [lume, bez, hands, dial, jewel],
        }
    }

    /// Curated artistic native palette for each Date & Time widget style (replacing flat monochrome amber).
    pub fn for_clock_style(style: crate::config::ClockStyle) -> Self {
        use crate::config::ClockStyle;
        if let Some(ht) = style.horology_theme() {
            return Self::for_horology(ht);
        }
        if let Some(mat) = style.pbr_material() {
            if let Some(p) = Self::for_pbr_material(mat) {
                return p;
            }
        }
        match style {
            ClockStyle::ArtDecoGilded => Self {
                accent: (232, 196, 114),
                accent_light: (252, 236, 186),
                accent_deep: (26, 122, 94),
                rim: (224, 184, 102),
                text_primary: (250, 245, 232),
                text_muted: (176, 214, 198),
                glass_top: (10, 34, 28),
                glass_bot: (6, 18, 16),
                preview_a: (8, 28, 24),
                preview_b: (18, 62, 50),
                preview_c: (6, 18, 15),
                swatches: [0xE8C472, 0x1A7A5E, 0xFCECBA, 0x0A221C, 0x061210],
            },
            ClockStyle::BauhausGeo => Self {
                accent: (232, 72, 58),
                accent_light: (255, 134, 112),
                accent_deep: (42, 98, 212),
                rim: (88, 146, 242),
                text_primary: (248, 246, 240),
                text_muted: (184, 198, 222),
                glass_top: (14, 20, 34),
                glass_bot: (8, 12, 22),
                preview_a: (14, 22, 42),
                preview_b: (26, 46, 86),
                preview_c: (12, 16, 28),
                swatches: [0xE8483A, 0x2A62D4, 0x5892F2, 0x0E1422, 0xF8F6F0],
            },
            ClockStyle::JapaneseZen => Self {
                accent: (228, 76, 62),
                accent_light: (252, 146, 132),
                accent_deep: (164, 38, 32),
                rim: (204, 196, 184),
                text_primary: (248, 244, 236),
                text_muted: (186, 182, 174),
                glass_top: (22, 22, 24),
                glass_bot: (12, 12, 14),
                preview_a: (24, 18, 22),
                preview_b: (56, 26, 30),
                preview_c: (14, 14, 18),
                swatches: [0xE44C3E, 0xCCC4B8, 0xA42620, 0x161618, 0xF8F4EC],
            },
            ClockStyle::TypoWatermark => Self {
                accent: (182, 138, 255),
                accent_light: (220, 196, 255),
                accent_deep: (116, 74, 212),
                rim: (164, 126, 242),
                text_primary: (248, 246, 255),
                text_muted: (192, 184, 218),
                glass_top: (20, 16, 30),
                glass_bot: (11, 9, 18),
                preview_a: (20, 14, 38),
                preview_b: (48, 30, 84),
                preview_c: (12, 10, 24),
                swatches: [0xB68AFF, 0xDCC4FF, 0x744AD4, 0x14101E, 0xF8F6FF],
            },
            ClockStyle::Editorial => Self {
                accent: (238, 158, 168),
                accent_light: (255, 208, 214),
                accent_deep: (152, 46, 66),
                rim: (224, 142, 152),
                text_primary: (254, 245, 246),
                text_muted: (212, 182, 188),
                glass_top: (30, 14, 18),
                glass_bot: (16, 8, 11),
                preview_a: (34, 14, 20),
                preview_b: (72, 26, 38),
                preview_c: (18, 10, 14),
                swatches: [0xEE9EA8, 0xFFD0D6, 0x982E42, 0x1E0E12, 0xFEF5F6],
            },
            ClockStyle::Signature => Self {
                accent: (238, 134, 74),
                accent_light: (255, 192, 132),
                accent_deep: (186, 68, 42),
                rim: (238, 134, 74),
                text_primary: (255, 246, 236),
                text_muted: (222, 198, 178),
                glass_top: (26, 16, 14),
                glass_bot: (14, 9, 9),
                preview_a: (38, 18, 18),
                preview_b: (76, 34, 24),
                preview_c: (20, 12, 14),
                swatches: [0xEE864A, 0xFFC084, 0xBA442A, 0x1A100E, 0xFFF6EC],
            },
            ClockStyle::ChronoDial => Self {
                accent: (94, 188, 255),
                accent_light: (208, 236, 255),
                accent_deep: (32, 106, 198),
                rim: (176, 208, 236),
                text_primary: (244, 250, 255),
                text_muted: (164, 194, 222),
                glass_top: (10, 20, 38),
                glass_bot: (6, 12, 24),
                preview_a: (8, 20, 42),
                preview_b: (20, 48, 88),
                preview_c: (6, 14, 28),
                swatches: [0x5EBCFF, 0xD0ECFF, 0x206AC6, 0x0A1426, 0xF4FAFF],
            },
            ClockStyle::OrbitalEclipse => Self {
                accent: (136, 166, 255),
                accent_light: (204, 222, 255),
                accent_deep: (78, 86, 224),
                rim: (126, 212, 255),
                text_primary: (242, 246, 255),
                text_muted: (174, 188, 224),
                glass_top: (12, 14, 30),
                glass_bot: (6, 8, 18),
                preview_a: (12, 12, 34),
                preview_b: (32, 28, 78),
                preview_c: (8, 10, 24),
                swatches: [0x88A6FF, 0x7ED4FF, 0x4E56E0, 0x0C0E1E, 0xF2F6FF],
            },
            ClockStyle::SunDialHorizon => Self {
                accent: (255, 118, 92),
                accent_light: (255, 184, 142),
                accent_deep: (196, 66, 116),
                rim: (255, 138, 104),
                text_primary: (255, 246, 240),
                text_muted: (220, 188, 194),
                glass_top: (24, 16, 32),
                glass_bot: (12, 10, 20),
                preview_a: (30, 16, 38),
                preview_b: (74, 32, 46),
                preview_c: (14, 12, 26),
                swatches: [0xFF765C, 0xFFB88E, 0xC44274, 0x181020, 0xFFF6F0],
            },
            ClockStyle::GlassRibbon => Self {
                accent: (72, 228, 198),
                accent_light: (176, 252, 236),
                accent_deep: (24, 148, 136),
                rim: (112, 236, 214),
                text_primary: (242, 255, 252),
                text_muted: (174, 220, 212),
                glass_top: (10, 26, 28),
                glass_bot: (6, 16, 18),
                preview_a: (8, 26, 30),
                preview_b: (18, 64, 68),
                preview_c: (6, 18, 22),
                swatches: [0x48E4C6, 0xB0FCEC, 0x189488, 0x0A1A1C, 0xF2FFFC],
            },
            ClockStyle::Minimal => Self {
                accent: (198, 216, 236),
                accent_light: (240, 246, 252),
                accent_deep: (112, 136, 164),
                rim: (184, 202, 222),
                text_primary: (252, 254, 255),
                text_muted: (182, 194, 208),
                glass_top: (20, 23, 28),
                glass_bot: (12, 14, 18),
                preview_a: (18, 22, 28),
                preview_b: (42, 50, 62),
                preview_c: (12, 15, 20),
                swatches: [0xC6D8EC, 0xF0F6FC, 0x7088A4, 0x14171C, 0xFCFEFF],
            },
            ClockStyle::Island => Self {
                accent: (76, 196, 255),
                accent_light: (174, 230, 255),
                accent_deep: (30, 122, 196),
                rim: (122, 206, 248),
                text_primary: (246, 252, 255),
                text_muted: (182, 204, 222),
                glass_top: (14, 18, 24),
                glass_bot: (8, 10, 14),
                preview_a: (10, 18, 30),
                preview_b: (24, 46, 74),
                preview_c: (8, 12, 20),
                swatches: [0x4CC4FF, 0xAEE6FF, 0x1E7AC4, 0x0E1218, 0xF6FCFF],
            },
            ClockStyle::FlipMech => Self {
                accent: (88, 216, 168),
                accent_light: (236, 244, 240),
                accent_deep: (36, 142, 108),
                rim: (148, 176, 168),
                text_primary: (248, 250, 252),
                text_muted: (176, 194, 190),
                glass_top: (22, 26, 30),
                glass_bot: (11, 14, 17),
                preview_a: (16, 24, 28),
                preview_b: (32, 52, 56),
                preview_c: (10, 14, 18),
                swatches: [0x58D8A8, 0xECF4F0, 0x248E6C, 0x161A1E, 0xF8FAFC],
            },
            ClockStyle::NixieTube => Self {
                accent: (255, 114, 42),
                accent_light: (255, 194, 124),
                accent_deep: (212, 62, 22),
                rim: (228, 118, 62),
                text_primary: (255, 226, 192),
                text_muted: (214, 166, 134),
                glass_top: (24, 13, 10),
                glass_bot: (12, 7, 6),
                preview_a: (28, 12, 10),
                preview_b: (62, 24, 16),
                preview_c: (14, 8, 8),
                swatches: [0xFF722A, 0xFFC27C, 0xD43E16, 0x180D0A, 0xFFE2C0],
            },
            ClockStyle::RetroCasio => Self {
                accent: (114, 224, 152),
                accent_light: (182, 248, 200),
                accent_deep: (46, 146, 86),
                rim: (134, 166, 148),
                text_primary: (224, 252, 232),
                text_muted: (148, 196, 164),
                glass_top: (16, 24, 20),
                glass_bot: (10, 15, 12),
                preview_a: (12, 24, 18),
                preview_b: (26, 56, 38),
                preview_c: (8, 16, 12),
                swatches: [0x72E098, 0xB6F8C8, 0x2E9256, 0x101814, 0xE0FCE8],
            },
            ClockStyle::MatrixBinary => Self {
                accent: (52, 240, 138),
                accent_light: (156, 255, 198),
                accent_deep: (18, 154, 82),
                rim: (64, 214, 132),
                text_primary: (232, 255, 242),
                text_muted: (142, 208, 170),
                glass_top: (8, 18, 14),
                glass_bot: (4, 10, 8),
                preview_a: (6, 20, 14),
                preview_b: (14, 52, 34),
                preview_c: (4, 12, 9),
                swatches: [0x34F08A, 0x9CFFC6, 0x129A52, 0x08120E, 0xE8FFF2],
            },
            ClockStyle::Cyber => Self {
                accent: (46, 230, 255),
                accent_light: (168, 246, 255),
                accent_deep: (218, 62, 164),
                rim: (56, 224, 248),
                text_primary: (236, 252, 255),
                text_muted: (152, 210, 226),
                glass_top: (10, 16, 28),
                glass_bot: (6, 10, 18),
                preview_a: (8, 16, 32),
                preview_b: (22, 46, 78),
                preview_c: (12, 10, 28),
                swatches: [0x2EE6FF, 0xDA3EA4, 0xA8F6FF, 0x0A101C, 0xECFCFF],
            },
            ClockStyle::SplitStack => Self {
                accent: (255, 92, 126),
                accent_light: (255, 168, 186),
                accent_deep: (188, 38, 78),
                rim: (242, 98, 132),
                text_primary: (255, 246, 248),
                text_muted: (218, 186, 196),
                glass_top: (22, 15, 22),
                glass_bot: (12, 8, 14),
                preview_a: (26, 14, 24),
                preview_b: (64, 24, 44),
                preview_c: (14, 10, 18),
                swatches: [0xFF5C7E, 0xFFA8BA, 0xBC264E, 0x160F16, 0xFFF6F8],
            },
            _ => Self::autumn(),
        }
    }

    /// Curated artistic native palette for each Music Bar widget style (including PBR material alignment).
    pub fn for_music_style(style: crate::config::MusicStyle) -> Self {
        use crate::config::MusicStyle;
        if let Some(ht) = style.horology_theme() {
            return Self::for_horology(ht);
        }
        if let Some(mat) = style.pbr_material() {
            if let Some(p) = Self::for_pbr_material(mat) {
                return p;
            }
        }
        match style {
            MusicStyle::DynamicIslandCapsule => Self {
                accent: (132, 154, 255),
                accent_light: (198, 212, 255),
                accent_deep: (74, 92, 216),
                rim: (156, 174, 248),
                text_primary: (248, 250, 255),
                text_muted: (178, 188, 212),
                glass_top: (16, 18, 26),
                glass_bot: (8, 10, 15),
                preview_a: (12, 16, 32),
                preview_b: (28, 36, 72),
                preview_c: (8, 10, 20),
                swatches: [0x849AFF, 0xC6D4FF, 0x4A5CD8, 0x10121A, 0xF8FAFF],
            },
            MusicStyle::TurntablePro => Self {
                accent: (242, 98, 108),
                accent_light: (255, 172, 178),
                accent_deep: (164, 42, 54),
                rim: (226, 134, 142),
                text_primary: (254, 246, 246),
                text_muted: (212, 184, 188),
                glass_top: (28, 14, 16),
                glass_bot: (14, 8, 10),
                preview_a: (30, 14, 18),
                preview_b: (68, 26, 34),
                preview_c: (16, 9, 12),
                swatches: [0xF2626C, 0xFFACB2, 0xA42A36, 0x1C0E10, 0xFEF6F6],
            },
            MusicStyle::Sanctuary => Self {
                accent: (238, 134, 74),
                accent_light: (255, 194, 138),
                accent_deep: (184, 72, 42),
                rim: (238, 134, 74),
                text_primary: (255, 246, 236),
                text_muted: (222, 198, 178),
                glass_top: (26, 16, 14),
                glass_bot: (14, 9, 8),
                preview_a: (36, 18, 16),
                preview_b: (74, 34, 24),
                preview_c: (18, 11, 12),
                swatches: [0xEE864A, 0xFFC28A, 0xB8482A, 0x1A100E, 0xFFF6EC],
            },
            MusicStyle::VinylDeck => Self {
                accent: (212, 116, 248),
                accent_light: (238, 184, 255),
                accent_deep: (138, 56, 192),
                rim: (204, 122, 238),
                text_primary: (252, 246, 255),
                text_muted: (206, 186, 218),
                glass_top: (22, 13, 28),
                glass_bot: (12, 7, 16),
                preview_a: (24, 12, 34),
                preview_b: (58, 26, 80),
                preview_c: (12, 8, 20),
                swatches: [0xD474F8, 0xEEB8FF, 0x8A38C0, 0x160D1C, 0xFCF6FF],
            },
            MusicStyle::MinimalPill => Self {
                accent: (112, 214, 255),
                accent_light: (196, 238, 255),
                accent_deep: (44, 138, 196),
                rim: (154, 224, 252),
                text_primary: (246, 252, 255),
                text_muted: (182, 208, 224),
                glass_top: (16, 22, 30),
                glass_bot: (10, 14, 20),
                preview_a: (12, 22, 34),
                preview_b: (26, 52, 78),
                preview_c: (8, 14, 22),
                swatches: [0x70D6FF, 0xC4EEFF, 0x2C8AC4, 0x10161E, 0xF6FCFF],
            },
            MusicStyle::DualFloatOrb => Self {
                accent: (62, 232, 196),
                accent_light: (166, 252, 234),
                accent_deep: (22, 148, 128),
                rim: (86, 232, 202),
                text_primary: (242, 255, 252),
                text_muted: (172, 218, 210),
                glass_top: (10, 24, 24),
                glass_bot: (6, 14, 16),
                preview_a: (8, 24, 26),
                preview_b: (18, 60, 62),
                preview_c: (6, 16, 18),
                swatches: [0x3EE8C4, 0xA6FCEA, 0x169480, 0x0A1818, 0xF2FFFC],
            },
            MusicStyle::EditorialSleeve => Self {
                accent: (234, 88, 72),
                accent_light: (255, 162, 148),
                accent_deep: (168, 42, 36),
                rim: (216, 204, 192),
                text_primary: (252, 248, 242),
                text_muted: (198, 190, 184),
                glass_top: (24, 20, 20),
                glass_bot: (13, 11, 12),
                preview_a: (28, 18, 18),
                preview_b: (62, 28, 26),
                preview_c: (14, 12, 14),
                swatches: [0xEA5848, 0xD8CCC0, 0xA82A24, 0x181414, 0xFCF8F2],
            },
            MusicStyle::EclipsePlayer => Self {
                accent: (166, 128, 255),
                accent_light: (176, 228, 255),
                accent_deep: (92, 62, 204),
                rim: (158, 134, 255),
                text_primary: (246, 246, 255),
                text_muted: (188, 188, 224),
                glass_top: (14, 12, 28),
                glass_bot: (7, 6, 16),
                preview_a: (14, 10, 34),
                preview_b: (40, 26, 82),
                preview_c: (8, 8, 22),
                swatches: [0xA680FF, 0xB0E4FF, 0x5C3ECC, 0x0E0C1C, 0xF6F6FF],
            },
            MusicStyle::OscilloscopeCrt => Self {
                accent: (48, 242, 152),
                accent_light: (158, 255, 208),
                accent_deep: (18, 152, 92),
                rim: (66, 224, 148),
                text_primary: (234, 255, 244),
                text_muted: (148, 212, 178),
                glass_top: (10, 20, 16),
                glass_bot: (6, 12, 10),
                preview_a: (8, 22, 16),
                preview_b: (16, 56, 38),
                preview_c: (6, 14, 11),
                swatches: [0x30F298, 0x9EFFD0, 0x12985C, 0x0A1410, 0xEAFFF4],
            },
            MusicStyle::SpectrumDeck => Self {
                accent: (42, 224, 255),
                accent_light: (164, 244, 255),
                accent_deep: (22, 132, 198),
                rim: (62, 218, 248),
                text_primary: (240, 252, 255),
                text_muted: (164, 212, 228),
                glass_top: (10, 18, 28),
                glass_bot: (6, 11, 18),
                preview_a: (8, 18, 32),
                preview_b: (18, 48, 78),
                preview_c: (6, 12, 22),
                swatches: [0x2AE0FF, 0xA4F4FF, 0x1684C6, 0x0A121C, 0xF0FCFF],
            },
            MusicStyle::NeonWave => Self {
                accent: (255, 84, 164),
                accent_light: (255, 166, 210),
                accent_deep: (184, 34, 114),
                rim: (248, 92, 168),
                text_primary: (255, 244, 250),
                text_muted: (222, 182, 206),
                glass_top: (24, 10, 24),
                glass_bot: (13, 6, 15),
                preview_a: (28, 10, 30),
                preview_b: (74, 22, 62),
                preview_c: (14, 8, 22),
                swatches: [0xFF54A4, 0xFFA6D2, 0xB82272, 0x180A18, 0xFFF4FA],
            },
            MusicStyle::RadialHalo => Self {
                accent: (56, 228, 220),
                accent_light: (168, 250, 246),
                accent_deep: (36, 126, 168),
                rim: (88, 228, 224),
                text_primary: (242, 254, 255),
                text_muted: (172, 218, 222),
                glass_top: (10, 20, 26),
                glass_bot: (6, 12, 18),
                preview_a: (10, 20, 30),
                preview_b: (22, 54, 68),
                preview_c: (8, 12, 22),
                swatches: [0x38E4DC, 0xA8FAF6, 0x247EA8, 0x0A141A, 0xF2FEFF],
            },
            MusicStyle::RibbonFlow => Self {
                accent: (255, 114, 96),
                accent_light: (255, 184, 166),
                accent_deep: (192, 58, 52),
                rim: (246, 124, 106),
                text_primary: (255, 246, 244),
                text_muted: (224, 190, 186),
                glass_top: (26, 14, 16),
                glass_bot: (14, 8, 10),
                preview_a: (32, 14, 18),
                preview_b: (72, 30, 34),
                preview_c: (16, 10, 14),
                swatches: [0xFF7260, 0xFFB8A6, 0xC03A34, 0x1A0E10, 0xFFF6F4],
            },
            MusicStyle::CassetteDeck => Self {
                accent: (96, 184, 255),
                accent_light: (214, 232, 248),
                accent_deep: (42, 108, 188),
                rim: (168, 196, 222),
                text_primary: (244, 248, 252),
                text_muted: (174, 194, 214),
                glass_top: (20, 24, 32),
                glass_bot: (11, 14, 20),
                preview_a: (14, 20, 34),
                preview_b: (32, 48, 76),
                preview_c: (10, 14, 24),
                swatches: [0x60B8FF, 0xD6E8F8, 0x2A6CBC, 0x141820, 0xF4F8FC],
            },
            MusicStyle::AnalogVuMeter => Self {
                accent: (84, 202, 238),
                accent_light: (186, 238, 252),
                accent_deep: (34, 126, 168),
                rim: (164, 204, 222),
                text_primary: (244, 250, 254),
                text_muted: (174, 202, 216),
                glass_top: (16, 20, 26),
                glass_bot: (9, 12, 16),
                preview_a: (12, 20, 28),
                preview_b: (26, 48, 66),
                preview_c: (8, 13, 18),
                swatches: [0x54CAEE, 0xBAEEFC, 0x227EA8, 0x10141A, 0xF4FAFE],
            },
            MusicStyle::HifiAmplifier => Self {
                accent: (68, 224, 146),
                accent_light: (214, 226, 232),
                accent_deep: (28, 146, 92),
                rim: (178, 190, 198),
                text_primary: (244, 248, 250),
                text_muted: (172, 184, 192),
                glass_top: (22, 25, 28),
                glass_bot: (12, 14, 17),
                preview_a: (16, 22, 24),
                preview_b: (36, 52, 54),
                preview_c: (10, 14, 16),
                swatches: [0x44E092, 0xD6E2E8, 0x1C925C, 0x16191C, 0xF4F8FA],
            },
            MusicStyle::WinampRack => Self {
                accent: (138, 236, 78),
                accent_light: (204, 252, 152),
                accent_deep: (62, 158, 36),
                rim: (148, 182, 144),
                text_primary: (240, 252, 236),
                text_muted: (176, 198, 174),
                glass_top: (18, 22, 20),
                glass_bot: (10, 13, 12),
                preview_a: (14, 22, 16),
                preview_b: (32, 52, 34),
                preview_c: (9, 14, 11),
                swatches: [0x8AEC4E, 0xCCFC98, 0x3E9E24, 0x121614, 0xF0FCEC],
            },
            MusicStyle::FlipJukebox => Self {
                accent: (255, 102, 88),
                accent_light: (255, 210, 182),
                accent_deep: (44, 186, 192),
                rim: (232, 162, 144),
                text_primary: (255, 248, 242),
                text_muted: (184, 210, 208),
                glass_top: (22, 16, 20),
                glass_bot: (12, 9, 13),
                preview_a: (26, 14, 22),
                preview_b: (62, 28, 36),
                preview_c: (12, 14, 20),
                swatches: [0xFF6658, 0x2CBAC0, 0xFFD2B6, 0x161014, 0xFFF8F2],
            },
            MusicStyle::CyberHud => Self {
                accent: (46, 230, 255),
                accent_light: (172, 246, 255),
                accent_deep: (218, 58, 162),
                rim: (58, 224, 248),
                text_primary: (238, 252, 255),
                text_muted: (156, 210, 228),
                glass_top: (10, 16, 28),
                glass_bot: (6, 10, 18),
                preview_a: (8, 16, 32),
                preview_b: (22, 46, 80),
                preview_c: (14, 10, 28),
                swatches: [0x2EE6FF, 0xDA3AA2, 0xACF6FF, 0x0A101C, 0xEEFCFF],
            },
            _ => Self::autumn(),
        }
    }

    /// Artistic preview backdrop palette for an Extended Widget theme.
    pub fn for_ext_widget_theme(theme: crate::widgets_extra::ExtWidgetTheme) -> Self {
        use crate::widgets_extra::ExtWidgetTheme;
        if let Some(ht) = theme.horology_theme() {
            return Self::for_horology(ht);
        }
        if let Some(mat) = theme.pbr_material() {
            if let Some(p) = Self::for_pbr_material(mat) {
                return p;
            }
        }
        match theme {
            ExtWidgetTheme::WinterSnow => Self::from_accent_rgb(0x78D4FF),
            ExtWidgetTheme::HearthFire => Self::from_accent_rgb(0xFF7622),
            ExtWidgetTheme::LiquidWater => Self::from_accent_rgb(0x2EDEF0),
            ExtWidgetTheme::SakuraWind => Self::from_accent_rgb(0xFF8AB4),
            ExtWidgetTheme::CosmicLens => Self::from_accent_rgb(0x987EFF),
            ExtWidgetTheme::TeslaPlasma => Self::from_accent_rgb(0x40F0FF),
            ExtWidgetTheme::AutumnCrystal => Self::for_clock_style(crate::config::ClockStyle::Signature),
            ExtWidgetTheme::NordicMinimal => Self::for_clock_style(crate::config::ClockStyle::Minimal),
            ExtWidgetTheme::FloatingIsland => Self::for_clock_style(crate::config::ClockStyle::Island),
            ExtWidgetTheme::CelestialSunDial => Self::for_clock_style(crate::config::ClockStyle::SunDialHorizon),
            ExtWidgetTheme::EditorialSerif => Self::for_clock_style(crate::config::ClockStyle::Editorial),
            ExtWidgetTheme::CyberHud => Self::for_clock_style(crate::config::ClockStyle::Cyber),
            ExtWidgetTheme::NeonMatrix => Self::from_accent_rgb(0xFF6B8B),
            _ => Self::autumn(),
        }
    }

    pub fn from_accent_rgb(rgb_u32: u32) -> Self {
        let raw = u32_to_rgb(rgb_u32);
        let (h, s, l) = rgb_to_hsl(raw);
        let accent = if s < 0.12 && l > 0.72 {
            // Crisp monochrome silver-white accent
            (236, 242, 248)
        } else if s < 0.12 && l < 0.30 {
            (180, 190, 205)
        } else {
            hsl_to_rgb(h, s.clamp(0.45, 0.94), l.clamp(0.50, 0.70))
        };
        let accent_light = if s < 0.12 {
            (255, 255, 255)
        } else {
            hsl_to_rgb(h + 6.0, (s * 0.96).clamp(0.50, 0.95), (l + 0.14).clamp(0.68, 0.84))
        };
        let accent_deep = if s < 0.12 {
            (130, 140, 154)
        } else {
            hsl_to_rgb(h - 6.0, (s * 1.04).clamp(0.48, 0.96), (l - 0.14).clamp(0.34, 0.56))
        };
        let text_primary = blend_rgb((255, 255, 255), accent_light, 0.14);
        let text_muted = blend_rgb((228, 228, 232), accent_light, 0.24);
        let glass_top = hsl_to_rgb(h, (s * 0.35).min(0.32), 0.08);
        let glass_bot = hsl_to_rgb(h, (s * 0.30).min(0.28), 0.05);
        let preview_a = hsl_to_rgb(h, (s * 0.48).min(0.54), 0.15);
        let preview_b = hsl_to_rgb(h + 8.0, (s * 0.62).min(0.64), 0.24);
        let preview_c = hsl_to_rgb(h - 8.0, (s * 0.36).min(0.40), 0.10);
        Self {
            accent,
            accent_light,
            accent_deep,
            rim: accent,
            text_primary,
            text_muted,
            glass_top,
            glass_bot,
            preview_a,
            preview_b,
            preview_c,
            swatches: [
                rgb_to_u32(accent),
                rgb_to_u32(accent_light),
                rgb_to_u32(accent_deep),
                rgb_to_u32(preview_b),
                rgb_to_u32(glass_top),
            ],
        }
    }

    /// Perceptual OKLab/OKLCH + local visual-saliency + 4-edge spatial + 8-cluster K-Means++
    /// wallpaper color analyzer. Extracts hero primary, distinct secondary & tertiary accents,
    /// light-surface WCAG accent, all 4 edge-zone tones, dynamic range, harmony mood, and light/dark
    /// glass atmospheres in ~1 ms.
    pub fn analyze_bgra_pixels(bgra: &[u8], w: u32, h: u32) -> (Self, WallpaperColorProfile) {
        if w == 0 || h == 0 || bgra.len() < (w as usize) * (h as usize) * 4 {
            return (Self::autumn(), WallpaperColorProfile::default());
        }

        let wi = w as usize;
        let hi = h as usize;
        let step = if wi * hi > 6000 { 2 } else { 1 };
        let gw = (wi + step - 1) / step;
        let gh = (hi + step - 1) / step;
        let cap = gw * gh;

        // First pass: convert downsampled grid into perceptual OKLab so we can measure local
        // visual-saliency contrast (highlights detailed subjects like fireworks, signs, flowers,
        // foliage & characters over flat empty backdrops!).
        let mut grid_lab = Vec::with_capacity(cap);
        for y in (0..hi).step_by(step) {
            let row_off = y * wi * 4;
            for x in (0..wi).step_by(step) {
                let off = row_off + x * 4;
                let rgb = (bgra[off + 2], bgra[off + 1], bgra[off]);
                grid_lab.push(rgb_to_oklab(rgb));
            }
        }

        #[derive(Clone, Copy)]
        struct OkSample {
            l: f32,
            a: f32,
            b: f32,
            c: f32,
            sw: f32,
            focal: f32,
        }

        let mut samples = Vec::with_capacity(cap);
        let mut l_hist = [0u32; 32];
        let mut all_l = 0.0f64;
        let mut all_a = 0.0f64;
        let mut all_b = 0.0f64;
        let mut all_c = 0.0f64;
        let mut all_w = 0.0f64;

        let mut top_l = 0.0f64;
        let mut top_a = 0.0f64;
        let mut top_b = 0.0f64;
        let mut top_w = 0.0f64;

        let mut bot_l = 0.0f64;
        let mut bot_a = 0.0f64;
        let mut bot_b = 0.0f64;
        let mut bot_w = 0.0f64;

        let mut left_l = 0.0f64;
        let mut left_a = 0.0f64;
        let mut left_b = 0.0f64;
        let mut left_w = 0.0f64;

        let mut right_l = 0.0f64;
        let mut right_a = 0.0f64;
        let mut right_b = 0.0f64;
        let mut right_w = 0.0f64;

        let top_cut = ((gh as f32) * 0.22).ceil().max(1.0) as usize;
        let bot_start = ((gh as f32) * 0.78).floor().min(gh.saturating_sub(1) as f32) as usize;
        let left_cut = ((gw as f32) * 0.20).ceil().max(1.0) as usize;
        let right_start = ((gw as f32) * 0.80).floor().min(gw.saturating_sub(1) as f32) as usize;

        for gy in 0..gh {
            let ny = (gy as f32 + 0.5) / gh as f32;
            let row = gy * gw;
            for gx in 0..gw {
                let nx = (gx as f32 + 0.5) / gw as f32;
                let (ol, oa, ob) = grid_lab[row + gx];
                let c = (oa * oa + ob * ob).sqrt();

                // Local OKLab visual saliency (edge & color contrast against immediate neighbors)
                let (rl, ra, rb) = grid_lab[row + (gx + 1).min(gw - 1)];
                let (dl, da, db) = grid_lab[((gy + 1).min(gh - 1)) * gw + gx];
                let grad = (ol - rl).abs()
                    + (ol - dl).abs()
                    + 2.2 * ((oa - ra).abs() + (ob - rb).abs() + (oa - da).abs() + (ob - db).abs());
                let detail_boost = 1.0 + (grad * 1.15).min(0.65);

                // Rule-of-thirds / center-focal composition bias + aggressive 4-corner watermark suppression
                let in_corner = (nx < 0.10 || nx > 0.90) && (ny < 0.12 || ny > 0.88);
                let dx = (nx - 0.5).abs();
                let dy = (ny - 0.5).abs();
                let pos_bias = if in_corner {
                    0.35
                } else if dx < 0.34 && dy < 0.34 {
                    1.38
                } else {
                    0.96
                };
                let focal = (pos_bias * detail_boost).clamp(0.35, 1.85);
                let sw = if in_corner { 0.45 } else { 1.0 };

                samples.push(OkSample {
                    l: ol,
                    a: oa,
                    b: ob,
                    c,
                    sw,
                    focal,
                });

                let lb_bin = ((ol.clamp(0.0, 0.999) * 32.0) as usize).min(31);
                l_hist[lb_bin] += 1;

                let dw = sw as f64;
                all_l += ol as f64 * dw;
                all_a += oa as f64 * dw;
                all_b += ob as f64 * dw;
                all_c += c as f64 * dw;
                all_w += dw;

                if gy < top_cut {
                    top_l += ol as f64 * dw;
                    top_a += oa as f64 * dw;
                    top_b += ob as f64 * dw;
                    top_w += dw;
                }
                if gy >= bot_start {
                    bot_l += ol as f64 * dw;
                    bot_a += oa as f64 * dw;
                    bot_b += ob as f64 * dw;
                    bot_w += dw;
                }
                if gx < left_cut {
                    left_l += ol as f64 * dw;
                    left_a += oa as f64 * dw;
                    left_b += ob as f64 * dw;
                    left_w += dw;
                }
                if gx >= right_start {
                    right_l += ol as f64 * dw;
                    right_a += oa as f64 * dw;
                    right_b += ob as f64 * dw;
                    right_w += dw;
                }
            }
        }

        if samples.is_empty() || all_w <= 0.0 {
            return (Self::autumn(), WallpaperColorProfile::default());
        }

        // Dynamic lightness range (`p90 - p10`)
        let total_bins: u32 = l_hist.iter().sum::<u32>().max(1);
        let p10_target = (total_bins as f32 * 0.10) as u32;
        let p90_target = (total_bins as f32 * 0.90) as u32;
        let mut acc_bins = 0u32;
        let mut l_p10 = 0.0f32;
        let mut l_p90 = 1.0f32;
        let mut got_p10 = false;
        for (bi, &cnt) in l_hist.iter().enumerate() {
            acc_bins += cnt;
            let bin_l = (bi as f32 + 0.5) / 32.0;
            if !got_p10 && acc_bins >= p10_target {
                l_p10 = bin_l;
                got_p10 = true;
            }
            if acc_bins >= p90_target {
                l_p90 = bin_l;
                break;
            }
        }
        let dynamic_range = (l_p90 - l_p10).clamp(0.0, 1.0);

        let mean_l = (all_l / all_w) as f32;
        let mean_a = (all_a / all_w) as f32;
        let mean_b = (all_b / all_w) as f32;
        let scene_chroma = (all_c / all_w) as f32;

        let zone_avg = |sl: f64, sa: f64, sb: f64, sw_tot: f64| -> (f32, f32, f32) {
            if sw_tot > 0.0 {
                ((sl / sw_tot) as f32, (sa / sw_tot) as f32, (sb / sw_tot) as f32)
            } else {
                (mean_l, mean_a, mean_b)
            }
        };
        let top_zone_ok = zone_avg(top_l, top_a, top_b, top_w);
        let bot_zone_ok = zone_avg(bot_l, bot_a, bot_b, bot_w);
        let left_zone_ok = zone_avg(left_l, left_a, left_b, left_w);
        let right_zone_ok = zone_avg(right_l, right_a, right_b, right_w);
        let top_zone_rgb = oklab_to_rgb(top_zone_ok.0, top_zone_ok.1, top_zone_ok.2);
        let bottom_zone_rgb = oklab_to_rgb(bot_zone_ok.0, bot_zone_ok.1, bot_zone_ok.2);
        let left_zone_rgb = oklab_to_rgb(left_zone_ok.0, left_zone_ok.1, left_zone_ok.2);
        let right_zone_rgb = oklab_to_rgb(right_zone_ok.0, right_zone_ok.1, right_zone_ok.2);

        // Deterministic 8-cluster weighted OKLab K-Means++ (captures fine focal accents + multi-hue scenes)
        const K: usize = 8;
        let mut cents = [(mean_l, mean_a, mean_b); K];
        let mut min_dist = vec![f32::MAX; samples.len()];
        for k_idx in 1..K {
            let prev = cents[k_idx - 1];
            let mut best_i = 0usize;
            let mut best_score = -1.0f32;
            for (i, s) in samples.iter().enumerate() {
                let dl = s.l - prev.0;
                let da = s.a - prev.1;
                let db = s.b - prev.2;
                let d2 = dl * dl + da * da + db * db;
                if d2 < min_dist[i] {
                    min_dist[i] = d2;
                }
                let sc = min_dist[i] * s.sw * (0.75 + 0.25 * s.focal) * (1.0 + 3.6 * s.c);
                if sc > best_score {
                    best_score = sc;
                    best_i = i;
                }
            }
            if best_score > 1e-6 {
                let chosen = samples[best_i];
                cents[k_idx] = (chosen.l, chosen.a, chosen.b);
            } else {
                break;
            }
        }

        let mut c_w = [0.0f64; K];
        let mut c_focal = [0.0f64; K];
        for _iter in 0..6 {
            let mut sl = [0.0f64; K];
            let mut sa = [0.0f64; K];
            let mut sb = [0.0f64; K];
            c_w = [0.0f64; K];
            c_focal = [0.0f64; K];
            for s in &samples {
                let mut best_k = 0usize;
                let mut best_d2 = f32::MAX;
                for (ki, &(cl, ca, cb)) in cents.iter().enumerate() {
                    let dl = s.l - cl;
                    let da = s.a - ca;
                    let db = s.b - cb;
                    let d2 = dl * dl + da * da + db * db;
                    if d2 < best_d2 {
                        best_d2 = d2;
                        best_k = ki;
                    }
                }
                let w_f = s.sw as f64;
                sl[best_k] += s.l as f64 * w_f;
                sa[best_k] += s.a as f64 * w_f;
                sb[best_k] += s.b as f64 * w_f;
                c_w[best_k] += w_f;
                c_focal[best_k] += (s.sw * s.focal) as f64;
            }
            for ki in 0..K {
                if c_w[ki] > 1e-5 {
                    cents[ki] = (
                        (sl[ki] / c_w[ki]) as f32,
                        (sa[ki] / c_w[ki]) as f32,
                        (sb[ki] / c_w[ki]) as f32,
                    );
                }
            }
        }

        #[derive(Clone, Copy)]
        struct ClusterInfo {
            l: f32,
            c: f32,
            h: f32,
            area: f32,
            accent_score: f32,
        }

        let total_cw: f64 = c_w.iter().sum::<f64>().max(1.0);
        let mut clusters: Vec<ClusterInfo> = Vec::with_capacity(K);
        for ki in 0..K {
            if c_w[ki] < 1e-4 {
                continue;
            }
            let (cl, ca, cb) = cents[ki];
            let (ol, oc, oh) = oklab_to_oklch(cl, ca, cb);
            let area = (c_w[ki] / total_cw) as f32;
            let focal_boost = ((c_focal[ki] / c_w[ki]) as f32).clamp(0.48, 1.65);
            // Perceptual saliency score: balances OKLCH chroma, sqrt(area) so vivid focal subjects
            // shine without losing dominant scenery, local edge/center saliency, and mid-tone clarity.
            let light_bell = (1.0 - ((ol - 0.58).abs() * 1.40).min(0.80)).max(0.20);
            let chroma_gate = if oc < 0.030 { 0.0 } else { (oc - 0.018).powf(1.28) };
            let area_term = area.sqrt().clamp(0.12, 1.0);
            let accent_score = chroma_gate * area_term * focal_boost * light_bell;
            clusters.push(ClusterInfo {
                l: ol,
                c: oc,
                h: oh,
                area,
                accent_score,
            });
        }

        // Largest ambient cluster captures real background atmosphere; highest-scoring chromatic cluster is primary accent.
        let ambient = clusters
            .iter()
            .max_by(|a, b| a.area.total_cmp(&b.area))
            .copied()
            .unwrap_or(ClusterInfo {
                l: mean_l,
                c: (mean_a * mean_a + mean_b * mean_b).sqrt(),
                h: mean_b.atan2(mean_a).to_degrees().rem_euclid(360.0),
                area: 1.0,
                accent_score: 0.0,
            });
        let ambient_tint = oklch_to_rgb(ambient.l.clamp(0.16, 0.88), ambient.c.clamp(0.0, 0.16), ambient.h);

        clusters.sort_by(|a, b| b.accent_score.total_cmp(&a.accent_score));

        let primary_cluster = clusters.first().copied().filter(|c| c.accent_score > 0.0018);
        let (pri_h, pri_c, pri_l, is_monochrome) = if let Some(pc) = primary_cluster {
            let tuned_c = pc.c.clamp(0.110, 0.220);
            let tuned_l = pc.l.clamp(0.63, 0.77);
            (pc.h, tuned_c, tuned_l, false)
        } else {
            let (_, mc, mh) = oklab_to_oklch(mean_l, mean_a, mean_b);
            if mc < 0.025 {
                (240.0, 0.022, 0.80, true)
            } else {
                (mh, mc.clamp(0.05, 0.115), 0.72, false)
            }
        };
        let primary_raw = oklch_to_rgb(pri_l, pri_c, pri_h);

        // Distinct secondary harmonic accent (`Δh >= 32°`, real area & chroma)
        let secondary_cluster = clusters.iter().skip(1).copied().find(|cl| {
            cl.accent_score > 0.0010
                && cl.c >= 0.038
                && cl.area >= 0.020
                && hue_dist_deg(cl.h, pri_h) >= 32.0
        });
        let (sec_h, sec_c, sec_l, has_real_secondary) = if let Some(sc) = secondary_cluster {
            (sc.h, sc.c.clamp(0.100, 0.210), sc.l.clamp(0.69, 0.84), true)
        } else if is_monochrome {
            (pri_h, 0.015, 0.90, false)
        } else {
            // Refined analogous companion for single-hue scenes
            ((pri_h + 24.0).rem_euclid(360.0), (pri_c * 0.88).clamp(0.088, 0.195), (pri_l + 0.10).clamp(0.75, 0.87), false)
        };
        let secondary_accent = oklch_to_rgb(sec_l, sec_c, sec_h);

        // Distinct tertiary micro-accent (`Δh >= 32°` from both primary & secondary)
        let tertiary_cluster = clusters.iter().skip(1).copied().find(|cl| {
            cl.accent_score > 0.0008
                && cl.c >= 0.036
                && cl.area >= 0.016
                && hue_dist_deg(cl.h, pri_h) >= 32.0
                && hue_dist_deg(cl.h, sec_h) >= 32.0
        });
        let (ter_h, ter_c, ter_l, has_real_tertiary) = if let Some(tc) = tertiary_cluster {
            (tc.h, tc.c.clamp(0.095, 0.200), tc.l.clamp(0.66, 0.82), true)
        } else if is_monochrome {
            (pri_h, 0.018, 0.68, false)
        } else if has_real_secondary {
            // Harmonic bridge tone between primary and secondary
            let mid_h = (pri_h + 120.0).rem_euclid(360.0);
            (mid_h, (pri_c * 0.72).clamp(0.075, 0.165), 0.74, false)
        } else {
            ((pri_h - 26.0).rem_euclid(360.0), (pri_c * 0.80).clamp(0.080, 0.175), 0.72, false)
        };
        let tertiary_raw = oklch_to_rgb(ter_l, ter_c, ter_h);

        // Classify perceptual hue harmony from extracted clusters
        let harmony = if is_monochrome {
            ColorHarmonyMood::Monochrome
        } else if has_real_secondary && has_real_tertiary {
            let d12 = hue_dist_deg(pri_h, sec_h);
            let d13 = hue_dist_deg(pri_h, ter_h);
            let d23 = hue_dist_deg(sec_h, ter_h);
            if d12 >= 80.0 && d13 >= 80.0 && d23 >= 70.0 {
                ColorHarmonyMood::Triadic
            } else {
                ColorHarmonyMood::VibrantMulti
            }
        } else if has_real_secondary {
            let d12 = hue_dist_deg(pri_h, sec_h);
            if d12 >= 130.0 {
                ColorHarmonyMood::Complementary
            } else if d12 >= 75.0 {
                ColorHarmonyMood::VibrantMulti
            } else {
                ColorHarmonyMood::Analogous
            }
        } else {
            ColorHarmonyMood::Analogous
        };

        // Deep shadow/rich tone (either a deep chromatic cluster or deepened primary/secondary blend)
        let deep_cluster = clusters
            .iter()
            .copied()
            .filter(|cl| cl.c >= 0.042 && cl.l < 0.56 && cl.area >= 0.025)
            .max_by(|a, b| (a.c * a.area.sqrt()).total_cmp(&(b.c * b.area.sqrt())));
        let accent_deep = if is_monochrome {
            oklch_to_rgb(0.50, 0.018, pri_h)
        } else if let Some(dc) = deep_cluster {
            oklch_to_rgb(dc.l.clamp(0.42, 0.54), dc.c.clamp(0.095, 0.195), dc.h)
        } else {
            let deep_h = if has_real_secondary {
                sec_h
            } else {
                (pri_h - 14.0).rem_euclid(360.0)
            };
            oklch_to_rgb((pri_l - 0.18).clamp(0.42, 0.55), (pri_c * 0.96).clamp(0.095, 0.20), deep_h)
        };

        // Spatial zone + atmospheric glass tints (visibly harmonious OKLCH chroma, never washed-out grey!)
        let (_, top_c, top_h) = oklab_to_oklch(top_zone_ok.0, top_zone_ok.1, top_zone_ok.2);
        let (_, bot_c, bot_h) = oklab_to_oklch(bot_zone_ok.0, bot_zone_ok.1, bot_zone_ok.2);
        let glass_top_h = if top_c >= 0.032 {
            top_h
        } else if ambient.c >= 0.032 {
            ambient.h
        } else {
            pri_h
        };
        let glass_bot_h = if bot_c >= 0.032 {
            bot_h
        } else if has_real_secondary {
            sec_h
        } else {
            glass_top_h
        };
        let dark_c_top = if is_monochrome {
            0.010
        } else {
            (top_c.max(ambient.c * 0.70).max(pri_c * 0.42) * 0.48).clamp(0.034, 0.092)
        };
        let dark_c_bot = if is_monochrome {
            0.008
        } else {
            (bot_c.max(ambient.c * 0.60).max(sec_c * 0.38) * 0.45).clamp(0.028, 0.084)
        };
        let dark_l_top = if mean_l >= 0.60 { 0.225 } else { 0.185 };
        let dark_l_bot = if mean_l >= 0.60 { 0.160 } else { 0.125 };
        let dark_glass_top = oklch_to_rgb(dark_l_top, dark_c_top, glass_top_h);
        let dark_glass_bot = oklch_to_rgb(dark_l_bot, dark_c_bot, glass_bot_h);

        let light_c_top = if is_monochrome {
            0.008
        } else {
            (top_c.max(pri_c * 0.50) * 0.38).clamp(0.024, 0.064)
        };
        let light_c_bot = if is_monochrome {
            0.008
        } else {
            (bot_c.max(sec_c * 0.48) * 0.42).clamp(0.026, 0.072)
        };
        let light_glass_top = oklch_to_rgb(0.935, light_c_top, glass_top_h);
        let light_glass_bot = oklch_to_rgb(0.870, light_c_bot, glass_bot_h);

        // Guarantee WCAG contrast on dark glass AND on light glass (`on_light_accent`)!
        let accent = ensure_contrast_oklch(primary_raw, dark_glass_top, 4.6, true);
        let accent_light = ensure_contrast_oklch(secondary_accent, dark_glass_top, 6.0, true);
        let tertiary_accent = ensure_contrast_oklch(tertiary_raw, dark_glass_top, 4.4, true);
        let on_light_seed = oklch_to_rgb(0.46, if is_monochrome { 0.02 } else { pri_c.clamp(0.12, 0.22) }, pri_h);
        let on_light_accent = ensure_contrast_oklch(on_light_seed, light_glass_top, 4.5, false);
        let rim = if has_real_secondary {
            ensure_contrast_oklch(blend_oklab(accent, accent_light, 0.30), dark_glass_top, 4.8, true)
        } else {
            accent
        };
        let text_primary = ensure_contrast_oklch(
            oklch_to_rgb(0.96, if is_monochrome { 0.004 } else { 0.014 }, pri_h),
            dark_glass_top,
            10.5,
            true,
        );
        let text_muted = ensure_contrast_oklch(
            oklch_to_rgb(0.83, if is_monochrome { 0.008 } else { 0.032 }, if has_real_secondary { sec_h } else { pri_h }),
            dark_glass_top,
            6.0,
            true,
        );

        let preview_a = oklch_to_rgb(0.24, dark_c_top * 1.45, glass_top_h);
        let preview_b = oklch_to_rgb(0.36, (pri_c * 0.68).clamp(0.03, 0.14), pri_h);
        let preview_c = oklch_to_rgb(0.18, dark_c_bot * 1.35, glass_bot_h);

        let warmth = ((mean_a * 2.4 + mean_b * 3.0) + (primary_raw.0 as f32 - primary_raw.2 as f32) / 510.0)
            .clamp(-1.0, 1.0);
        let is_light_scene = mean_l >= 0.60;

        let pal = Self {
            accent,
            accent_light,
            accent_deep,
            rim,
            text_primary,
            text_muted,
            glass_top: dark_glass_top,
            glass_bot: dark_glass_bot,
            preview_a,
            preview_b,
            preview_c,
            swatches: [
                rgb_to_u32(accent),
                rgb_to_u32(accent_light),
                rgb_to_u32(tertiary_accent),
                rgb_to_u32(top_zone_rgb),
                rgb_to_u32(bottom_zone_rgb),
            ],
        };
        let profile = WallpaperColorProfile {
            scene_lightness: mean_l.clamp(0.0, 1.0),
            scene_chroma: scene_chroma.clamp(0.0, 0.40),
            dynamic_range,
            is_light_scene,
            warmth,
            harmony,
            primary_accent: accent,
            secondary_accent: accent_light,
            tertiary_accent,
            on_light_accent,
            ambient_tint,
            top_zone_rgb,
            bottom_zone_rgb,
            left_zone_rgb,
            right_zone_rgb,
            light_glass_top,
            light_glass_bot,
            dark_glass_top,
            dark_glass_bot,
        };
        (pal, profile)
    }

    /// Fast perceptual OKLab/OKLCH palette extraction from BGRA pixel buffer (static or live video).
    #[allow(dead_code)]
    pub fn from_bgra_pixels(bgra: &[u8], w: u32, h: u32) -> Self {
        Self::analyze_bgra_pixels(bgra, w, h).0
    }

    pub fn delta_distance(self, other: Self) -> i32 {
        let dr = (self.accent.0 as i32 - other.accent.0 as i32).abs();
        let dg = (self.accent.1 as i32 - other.accent.1 as i32).abs();
        let db = (self.accent.2 as i32 - other.accent.2 as i32).abs();
        let gr = (self.preview_b.0 as i32 - other.preview_b.0 as i32).abs();
        dr + dg + db + gr
    }

    pub fn lerp(self, target: Self, t: f32) -> Self {
        let k = t.clamp(0.0, 1.0);
        if k <= 0.0 {
            return self;
        }
        if k >= 1.0 {
            return target;
        }
        let mut swatches = [0u32; 5];
        for i in 0..5 {
            swatches[i] = rgb_to_u32(blend_oklab(
                u32_to_rgb(self.swatches[i]),
                u32_to_rgb(target.swatches[i]),
                k,
            ));
        }
        Self {
            accent: blend_oklab(self.accent, target.accent, k),
            accent_light: blend_oklab(self.accent_light, target.accent_light, k),
            accent_deep: blend_oklab(self.accent_deep, target.accent_deep, k),
            rim: blend_oklab(self.rim, target.rim, k),
            text_primary: blend_oklab(self.text_primary, target.text_primary, k),
            text_muted: blend_oklab(self.text_muted, target.text_muted, k),
            glass_top: blend_oklab(self.glass_top, target.glass_top, k),
            glass_bot: blend_oklab(self.glass_bot, target.glass_bot, k),
            preview_a: blend_oklab(self.preview_a, target.preview_a, k),
            preview_b: blend_oklab(self.preview_b, target.preview_b, k),
            preview_c: blend_oklab(self.preview_c, target.preview_c, k),
            swatches,
        }
    }

    #[inline]
    pub fn accent_col(self, alpha: f32) -> D2D1_COLOR_F {
        rgb_to_d2d(self.accent, alpha)
    }

    #[inline]
    pub fn light_col(self, alpha: f32) -> D2D1_COLOR_F {
        rgb_to_d2d(self.accent_light, alpha)
    }

    #[inline]
    pub fn deep_col(self, alpha: f32) -> D2D1_COLOR_F {
        rgb_to_d2d(self.accent_deep, alpha)
    }

    #[inline]
    pub fn rim_col(self, alpha: f32) -> D2D1_COLOR_F {
        rgb_to_d2d(self.rim, alpha)
    }

    #[inline]
    pub fn text_col(self, alpha: f32) -> D2D1_COLOR_F {
        rgb_to_d2d(self.text_primary, alpha)
    }

    #[inline]
    pub fn muted_col(self, alpha: f32) -> D2D1_COLOR_F {
        rgb_to_d2d(self.text_muted, alpha)
    }

    #[inline]
    pub fn glass_top_col(self, alpha: f32) -> D2D1_COLOR_F {
        rgb_to_d2d(self.glass_top, alpha)
    }

    #[inline]
    pub fn glass_bot_col(self, alpha: f32) -> D2D1_COLOR_F {
        rgb_to_d2d(self.glass_bot, alpha)
    }
}

struct ExtractedState {
    source_key: String,
    palette: ThemePalette,
    target_palette: ThemePalette,
    profile: WallpaperColorProfile,
    target_profile: WallpaperColorProfile,
}

#[derive(Default)]
struct ExtractedCache {
    state: Option<ExtractedState>,
}

fn is_near_black_bgra(bgra: &[u8], w: u32, h: u32) -> bool {
    let total_px = (w as usize).saturating_mul(h as usize);
    if total_px == 0 || bgra.len() < total_px * 4 {
        return false;
    }
    let mut sum: u64 = 0;
    for i in 0..total_px {
        let off = i * 4;
        sum += bgra[off] as u64 + bgra[off + 1] as u64 + bgra[off + 2] as u64;
    }
    (sum as f64 / (total_px as f64 * 3.0)) < 6.0
}

impl ExtractedCache {
    fn update(&mut self, source_key: &str, bgra: &[u8], w: u32, h: u32, smooth: bool) -> bool {
        let (sampled, sampled_prof) = ThemePalette::analyze_bgra_pixels(bgra, w, h);
        let Some(prev) = self.state.as_mut() else {
            self.state = Some(ExtractedState {
                source_key: source_key.to_string(),
                palette: sampled,
                target_palette: sampled,
                profile: sampled_prof,
                target_profile: sampled_prof,
            });
            return true;
        };

        if prev.source_key != source_key || !smooth {
            let changed = prev.source_key != source_key || prev.palette.delta_distance(sampled) >= 12;
            prev.source_key = source_key.to_string();
            prev.palette = sampled;
            prev.target_palette = sampled;
            prev.profile = sampled_prof;
            prev.target_profile = sampled_prof;
            return changed;
        }

        // Ignore transient pitch-black loop fade frames if we already have an illuminated scene
        let prev_luma = (prev.target_palette.preview_b.0 as u16
            + prev.target_palette.preview_b.1 as u16
            + prev.target_palette.preview_b.2 as u16)
            / 3;
        if prev_luma > 16 && is_near_black_bgra(bgra, w, h) {
            return false;
        }

        // Hysteresis against codec noise before shifting the convergence target
        if prev.target_palette.delta_distance(sampled) >= 8 {
            prev.target_palette = sampled;
            prev.target_profile = sampled_prof;
        }

        let mut next = prev.palette.lerp(prev.target_palette, 0.34);
        let mut next_prof = prev.profile.lerp(prev.target_profile, 0.34);
        if next.delta_distance(prev.target_palette) <= 8 {
            next = prev.target_palette;
            next_prof = prev.target_profile;
        }

        let ui_changed = prev.palette.delta_distance(next) >= 3
            || (next == prev.target_palette && next != prev.palette);
        if ui_changed {
            prev.palette = next;
            prev.profile = next_prof;
        }
        ui_changed
    }
}

static EXTRACTED_WP: OnceLock<Mutex<ExtractedCache>> = OnceLock::new();

fn state_lock() -> &'static Mutex<ExtractedCache> {
    EXTRACTED_WP.get_or_init(|| Mutex::new(ExtractedCache::default()))
}

/// Updates cached dynamic wallpaper palette immediately (static images / explicit resamples).
/// Returns `true` if the extracted palette changed meaningfully.
pub fn update_extracted_palette_from_bgra(source_key: &str, bgra: &[u8], w: u32, h: u32) -> bool {
    let mut guard = state_lock().lock().unwrap_or_else(|e| e.into_inner());
    guard.update(source_key, bgra, w, h, false)
}

/// Updates cached dynamic wallpaper palette from a playing live-video frame with temporal
/// damping and scene-transition hysteresis so colors glide smoothly with zero flicker.
pub fn update_extracted_live_frame_from_bgra(source_key: &str, bgra: &[u8], w: u32, h: u32) -> bool {
    let mut guard = state_lock().lock().unwrap_or_else(|e| e.into_inner());
    guard.update(source_key, bgra, w, h, true)
}

pub fn extracted_wallpaper_palette() -> Option<(String, ThemePalette)> {
    let guard = state_lock().lock().unwrap_or_else(|e| e.into_inner());
    guard.state.as_ref().map(|s| (s.source_key.clone(), s.palette))
}

pub fn extracted_wallpaper_profile() -> Option<WallpaperColorProfile> {
    let guard = state_lock().lock().unwrap_or_else(|e| e.into_inner());
    guard.state.as_ref().map(|s| s.profile)
}

/// Resolves the spatial & perceptual `WallpaperColorProfile` for the active wallpaper.
pub fn resolve_wallpaper_profile(cfg: &AppConfig) -> WallpaperColorProfile {
    let _ = ensure_wallpaper_palette_for_config(cfg, false);
    extracted_wallpaper_profile().unwrap_or_default()
}

fn current_windows_desktop_wallpaper_path() -> Option<String> {
    unsafe {
        let mut buf = [0u16; 520];
        if SystemParametersInfoW(
            SPI_GETDESKWALLPAPER,
            buf.len() as u32,
            Some(buf.as_mut_ptr() as *mut c_void),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )
        .is_ok()
        {
            let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
            let s = String::from_utf16_lossy(&buf[..end]).trim().to_string();
            if !s.is_empty() && Path::new(&s).is_file() {
                return Some(s);
            }
        }
    }
    None
}

fn active_wallpaper_source_key(cfg: &AppConfig) -> String {
    if cfg.wallpaper_enabled && !cfg.wallpaper_path.is_empty() && Path::new(&cfg.wallpaper_path).is_file() {
        cfg.wallpaper_path.clone()
    } else if let Some(wp) = current_windows_desktop_wallpaper_path() {
        wp
    } else {
        "static:fallback".to_string()
    }
}

fn sample_via_wic(path: &str, target_w: u32, target_h: u32) -> Option<Vec<u8>> {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let wic: IWICImagingFactory =
            CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER).ok()?;
        let mut wide: Vec<u16> = path.encode_utf16().collect();
        wide.push(0);
        let dec = wic
            .CreateDecoderFromFilename(
                PCWSTR(wide.as_ptr()),
                None,
                windows::Win32::Foundation::GENERIC_READ,
                WICDecodeMetadataCacheOnDemand,
            )
            .ok()?;
        let frame = dec.GetFrame(0).ok()?;
        let scaler = wic.CreateBitmapScaler().ok()?;
        scaler
            .Initialize(&frame, target_w, target_h, WICBitmapInterpolationModeFant)
            .ok()?;
        let conv = wic.CreateFormatConverter().ok()?;
        let src: IWICBitmapSource = scaler.cast().ok()?;
        conv.Initialize(
            &src,
            &GUID_WICPixelFormat32bppBGRA,
            windows::Win32::Graphics::Imaging::WICBitmapDitherTypeNone,
            None,
            0.0,
            windows::Win32::Graphics::Imaging::WICBitmapPaletteTypeCustom,
        )
        .ok()?;
        let stride = target_w * 4;
        let mut buf = vec![0u8; (stride * target_h) as usize];
        conv.CopyPixels(std::ptr::null(), stride, &mut buf).ok()?;
        Some(buf)
    }
}

fn sample_via_shell_thumb(path: &str, size: i32) -> Option<(Vec<u8>, u32, u32)> {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let mut wide: Vec<u16> = path.encode_utf16().collect();
        wide.push(0);
        let factory: IShellItemImageFactory =
            SHCreateItemFromParsingName(PCWSTR(wide.as_ptr()), None).ok()?;
        let hbmp = factory
            .GetImage(
                SIZE { cx: size, cy: size },
                SIIGBF_BIGGERSIZEOK | SIIGBF_THUMBNAILONLY,
            )
            .or_else(|_| factory.GetImage(SIZE { cx: size, cy: size }, SIIGBF_BIGGERSIZEOK))
            .ok()?;

        let mut bm = BITMAP::default();
        let got = GetObjectW(HGDIOBJ(hbmp.0), size_of::<BITMAP>() as i32, Some(&mut bm as *mut _ as *mut c_void));
        if got == 0 || bm.bmWidth <= 0 || bm.bmHeight <= 0 {
            let _ = DeleteObject(HGDIOBJ(hbmp.0));
            return None;
        }
        let w = bm.bmWidth as u32;
        let h = bm.bmHeight as u32;
        let mut bi = BITMAPINFO::default();
        bi.bmiHeader.biSize = size_of::<BITMAPINFOHEADER>() as u32;
        bi.bmiHeader.biWidth = w as i32;
        bi.bmiHeader.biHeight = -(h as i32); // top-down
        bi.bmiHeader.biPlanes = 1;
        bi.bmiHeader.biBitCount = 32;
        bi.bmiHeader.biCompression = BI_RGB.0;

        let mut bgra = vec![0u8; (w * h * 4) as usize];
        let hdc = CreateCompatibleDC(None);
        let res = GetDIBits(
            hdc,
            hbmp,
            0,
            h,
            Some(bgra.as_mut_ptr() as *mut c_void),
            &mut bi,
            DIB_RGB_COLORS,
        );
        if !hdc.0.is_null() {
            let _ = DeleteDC(hdc);
        }
        let _ = DeleteObject(HGDIOBJ(hbmp.0));
        if res == 0 {
            return None;
        }
        Some((bgra, w, h))
    }
}

/// Resolves the active desktop wallpaper file path (AutumnDeck custom wallpaper, Windows desktop
/// wallpaper, or `TranscodedWallpaper` fallback) along with its modification timestamp token.
pub fn active_wallpaper_image_key_and_mtime(cfg: &AppConfig) -> Option<(String, u64)> {
    let mut candidates: Vec<String> = Vec::with_capacity(3);
    if cfg.wallpaper_enabled && !cfg.wallpaper_path.is_empty() {
        let low = cfg.wallpaper_path.to_ascii_lowercase();
        let is_video = low.ends_with(".mp4")
            || low.ends_with(".mkv")
            || low.ends_with(".webm")
            || low.ends_with(".mov")
            || low.ends_with(".avi")
            || low.ends_with(".wmv");
        // Never invoke synchronous Shell video thumbnail extraction on the UI thread for live
        // video wallpapers (hardware DWM Acrylic already blurs live video at 120Hz!).
        if !is_video {
            candidates.push(cfg.wallpaper_path.clone());
        }
    }
    if let Some(wp) = current_windows_desktop_wallpaper_path() {
        candidates.push(wp);
    }
    if let Ok(appdata) = std::env::var("APPDATA") {
        candidates.push(format!(
            "{}\\Microsoft\\Windows\\Themes\\TranscodedWallpaper",
            appdata
        ));
    }
    for path in candidates {
        if let Ok(meta) = std::fs::metadata(&path) {
            if meta.is_file() {
                let mtime = meta
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                return Some((path, mtime));
            }
        }
    }
    None
}

/// Fast 3-pass separable box blur (Gaussian approximation) + macOS NSVisualEffectView vibrancy
/// boost on a BGRA buffer of dimensions `w x h`. Runs in ~3–5ms on a 640x360 monitor backdrop.
pub fn apply_macos_frosted_blur_bgra(buf: &mut [u8], w: usize, h: usize, radius: usize) {
    if w < 4 || h < 4 || radius == 0 || buf.len() < w * h * 4 {
        return;
    }
    let r = radius.min(w.min(h) / 3).max(1);
    let win = (2 * r + 1) as u32;
    let mut tmp = vec![0u8; w * h * 4];

    for _pass in 0..3 {
        // Horizontal sliding-window pass: buf -> tmp
        for y in 0..h {
            let row = y * w * 4;
            let mut sb = buf[row] as u32 * (r as u32 + 1);
            let mut sg = buf[row + 1] as u32 * (r as u32 + 1);
            let mut sr = buf[row + 2] as u32 * (r as u32 + 1);
            for x in 1..=r {
                let cx = x.min(w - 1);
                let p = row + cx * 4;
                sb += buf[p] as u32;
                sg += buf[p + 1] as u32;
                sr += buf[p + 2] as u32;
            }
            for x in 0..w {
                let out = row + x * 4;
                tmp[out] = (sb / win) as u8;
                tmp[out + 1] = (sg / win) as u8;
                tmp[out + 2] = (sr / win) as u8;
                tmp[out + 3] = 255;

                let add_x = (x + r + 1).min(w - 1);
                let sub_x = x.saturating_sub(r);
                let pa = row + add_x * 4;
                let ps = row + sub_x * 4;
                sb = sb + buf[pa] as u32 - buf[ps] as u32;
                sg = sg + buf[pa + 1] as u32 - buf[ps + 1] as u32;
                sr = sr + buf[pa + 2] as u32 - buf[ps + 2] as u32;
            }
        }

        // Vertical sliding-window pass: tmp -> buf
        let stride = w * 4;
        for x in 0..w {
            let col = x * 4;
            let mut sb = tmp[col] as u32 * (r as u32 + 1);
            let mut sg = tmp[col + 1] as u32 * (r as u32 + 1);
            let mut sr = tmp[col + 2] as u32 * (r as u32 + 1);
            for y in 1..=r {
                let cy = y.min(h - 1);
                let p = cy * stride + col;
                sb += tmp[p] as u32;
                sg += tmp[p + 1] as u32;
                sr += tmp[p + 2] as u32;
            }
            for y in 0..h {
                let out = y * stride + col;
                buf[out] = (sb / win) as u8;
                buf[out + 1] = (sg / win) as u8;
                buf[out + 2] = (sr / win) as u8;
                buf[out + 3] = 255;

                let add_y = (y + r + 1).min(h - 1);
                let sub_y = y.saturating_sub(r);
                let pa = add_y * stride + col;
                let ps = sub_y * stride + col;
                sb = sb + tmp[pa] as u32 - tmp[ps] as u32;
                sg = sg + tmp[pa + 1] as u32 - tmp[ps + 1] as u32;
                sr = sr + tmp[pa + 2] as u32 - tmp[ps + 2] as u32;
            }
        }
    }

    // macOS NSVisualEffectView optical vibrancy (+40% chroma saturation lift + crystal highlight lift)
    for px in buf.chunks_exact_mut(4) {
        let b = px[0] as f32;
        let g = px[1] as f32;
        let r_c = px[2] as f32;
        let lum = 0.114 * b + 0.587 * g + 0.299 * r_c;
        let sat = 1.40f32;
        let gain = 1.04f32;
        px[0] = ((lum + (b - lum) * sat) * gain).clamp(0.0, 255.0) as u8;
        px[1] = ((lum + (g - lum) * sat) * gain).clamp(0.0, 255.0) as u8;
        px[2] = ((lum + (r_c - lum) * sat) * gain).clamp(0.0, 255.0) as u8;
        px[3] = 255;
    }
}

/// Decodes `path` (static image via WIC or video via Shell thumbnail) with exact Windows `Fill`
/// aspect-ratio center-cropping into `dst_w x dst_h` BGRA and applies real frosted glass blur.
pub fn decode_blurred_wallpaper_bgra(
    path: &str,
    dst_w: u32,
    dst_h: u32,
    blur_radius: usize,
) -> Option<Vec<u8>> {
    if dst_w < 8 || dst_h < 8 || !Path::new(path).is_file() {
        return None;
    }
    let low = path.to_ascii_lowercase();
    let is_video = low.ends_with(".mp4")
        || low.ends_with(".mkv")
        || low.ends_with(".webm")
        || low.ends_with(".mov")
        || low.ends_with(".avi")
        || low.ends_with(".wmv");

    let mut out = if !is_video {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            let wic: IWICImagingFactory =
                CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER).ok()?;
            let mut wide: Vec<u16> = path.encode_utf16().collect();
            wide.push(0);
            let dec = wic
                .CreateDecoderFromFilename(
                    PCWSTR(wide.as_ptr()),
                    None,
                    windows::Win32::Foundation::GENERIC_READ,
                    WICDecodeMetadataCacheOnDemand,
                )
                .ok()?;
            let frame = dec.GetFrame(0).ok()?;
            let mut sw = 0u32;
            let mut sh = 0u32;
            frame.GetSize(&mut sw, &mut sh).ok()?;
            if sw == 0 || sh == 0 {
                return None;
            }
            // Windows "Fill" aspect-ratio center crop matching the real desktop monitor
            let src_ar = sw as f64 / sh as f64;
            let dst_ar = dst_w as f64 / dst_h as f64;
            let (crop_x, crop_y, crop_w, crop_h) = if src_ar > dst_ar {
                let cw = ((sh as f64 * dst_ar).round() as u32).clamp(1, sw);
                ((sw - cw) / 2, 0, cw, sh)
            } else {
                let ch = ((sw as f64 / dst_ar).round() as u32).clamp(1, sh);
                (0, (sh - ch) / 2, sw, ch)
            };
            let clipper = wic.CreateBitmapClipper().ok()?;
            let clip_rc = windows::Win32::Graphics::Imaging::WICRect {
                X: crop_x as i32,
                Y: crop_y as i32,
                Width: crop_w as i32,
                Height: crop_h as i32,
            };
            clipper.Initialize(&frame, &clip_rc).ok()?;
            let scaler = wic.CreateBitmapScaler().ok()?;
            scaler
                .Initialize(&clipper, dst_w, dst_h, WICBitmapInterpolationModeFant)
                .ok()?;
            let conv = wic.CreateFormatConverter().ok()?;
            let src: IWICBitmapSource = scaler.cast().ok()?;
            conv.Initialize(
                &src,
                &GUID_WICPixelFormat32bppBGRA,
                windows::Win32::Graphics::Imaging::WICBitmapDitherTypeNone,
                None,
                0.0,
                windows::Win32::Graphics::Imaging::WICBitmapPaletteTypeCustom,
            )
            .ok()?;
            let stride = dst_w * 4;
            let mut buf = vec![0u8; (stride * dst_h) as usize];
            conv.CopyPixels(std::ptr::null(), stride, &mut buf).ok()?;
            buf
        }
    } else {
        let (raw, rw, rh) = sample_via_shell_thumb(path, 512)?;
        if rw == 0 || rh == 0 {
            return None;
        }
        let src_ar = rw as f32 / rh as f32;
        let dst_ar = dst_w as f32 / dst_h as f32;
        let (crop_x, crop_y, crop_w, crop_h) = if src_ar > dst_ar {
            let cw = (rh as f32 * dst_ar).clamp(1.0, rw as f32);
            ((rw as f32 - cw) * 0.5, 0.0, cw, rh as f32)
        } else {
            let ch = (rw as f32 / dst_ar).clamp(1.0, rh as f32);
            (0.0, (rh as f32 - ch) * 0.5, rw as f32, ch)
        };
        let mut buf = vec![0u8; (dst_w * dst_h * 4) as usize];
        for y in 0..dst_h {
            let sy = (crop_y + (y as f32 + 0.5) / dst_h as f32 * crop_h)
                .floor()
                .clamp(0.0, (rh - 1) as f32) as u32;
            for x in 0..dst_w {
                let sx = (crop_x + (x as f32 + 0.5) / dst_w as f32 * crop_w)
                    .floor()
                    .clamp(0.0, (rw - 1) as f32) as u32;
                let si = ((sy * rw + sx) * 4) as usize;
                let di = ((y * dst_w + x) * 4) as usize;
                buf[di] = raw[si];
                buf[di + 1] = raw[si + 1];
                buf[di + 2] = raw[si + 2];
                buf[di + 3] = 255;
            }
        }
        buf
    };

    apply_macos_frosted_blur_bgra(&mut out, dst_w as usize, dst_h as usize, blur_radius);
    Some(out)
}

/// Ensures `extracted_wallpaper_palette()` is populated for the active wallpaper (video or static desktop).
/// Returns `true` if palette changed.
pub fn ensure_wallpaper_palette_for_config(cfg: &AppConfig, force_resample: bool) -> bool {
    let key = active_wallpaper_source_key(cfg);
    if !force_resample {
        if let Some((cached_key, _)) = extracted_wallpaper_palette() {
            if cached_key == key {
                return false;
            }
        }
    }
    if Path::new(&key).is_file() {
        let low = key.to_ascii_lowercase();
        let is_img = low.ends_with(".png")
            || low.ends_with(".jpg")
            || low.ends_with(".jpeg")
            || low.ends_with(".bmp")
            || low.ends_with(".webp")
            || !cfg.wallpaper_enabled;
        if is_img {
            if let Some(bgra) = sample_via_wic(&key, 96, 54) {
                return update_extracted_palette_from_bgra(&key, &bgra, 96, 54);
            }
        }
        if let Some((bgra, w, h)) = sample_via_shell_thumb(&key, 96) {
            return update_extracted_palette_from_bgra(&key, &bgra, w, h);
        }
        if let Some(wp) = current_windows_desktop_wallpaper_path() {
            if let Some(bgra) = sample_via_wic(&wp, 96, 54) {
                return update_extracted_palette_from_bgra(&key, &bgra, 96, 54);
            }
        }
    }
    false
}

/// Resolves palette for an individual surface or widget (`match_wallpaper` overrides Autumn amber).
pub fn resolve_palette_for(cfg: &AppConfig, match_wallpaper: bool) -> ThemePalette {
    if match_wallpaper {
        let _ = ensure_wallpaper_palette_for_config(cfg, false);
        if let Some((_, pal)) = extracted_wallpaper_palette() {
            return pal;
        }
    }
    ThemePalette::autumn()
}

/// Legacy warm amber/cream icon tint used when the active App Launchers theme is classic Autumn.
pub const AUTUMN_ICON_TINT: ((u8, u8, u8), (u8, u8, u8)) = ((224, 130, 42), (255, 243, 220));

fn palette_from_suite_preset(suite: &crate::suites::SuitePreset) -> ThemePalette {
    let mut p = ThemePalette::from_accent_rgb(suite.accent_rgb);
    p.accent = u32_to_rgb(suite.accent_rgb);
    p.accent_light = u32_to_rgb(suite.secondary_rgb);
    p.rim = u32_to_rgb(suite.rim_rgb);
    p.text_primary = u32_to_rgb(suite.text_rgb);
    p.text_muted = u32_to_rgb(suite.muted_rgb);
    let bg = u32_to_rgb(suite.bg_rgb);
    p.glass_top = bg;
    p.glass_bot = (
        bg.0.saturating_sub(12),
        bg.1.saturating_sub(12),
        bg.2.saturating_sub(12),
    );
    p.preview_a = bg;
    p.preview_b = blend_rgb(bg, p.accent, 0.28);
    p.preview_c = p.glass_bot;
    p.swatches = [
        suite.accent_rgb,
        suite.secondary_rgb,
        suite.rim_rgb,
        suite.bg_rgb,
        suite.text_rgb,
    ];
    p
}

/// Resolves palette for the App Launchers Dock:
/// Apps Wallpaper Match -> Apps Custom Colors -> Active Taskbar/Suite/Desktop Theme -> Autumn default,
/// plus optional Pen Canvas Card overrides (`apps_elem_transforms[4]`).
pub fn resolve_apps_palette(cfg: &AppConfig) -> ThemePalette {
    let tr_card = cfg.apps_elem_transforms[4];
    let mut pal = if cfg.apps_match_wallpaper {
        resolve_palette_for(cfg, true)
    } else if cfg.apps_use_custom_color {
        let acc = u32_to_rgb(cfg.apps_accent_rgb);
        let txt = u32_to_rgb(cfg.apps_text_rgb);
        let bg = u32_to_rgb(cfg.apps_bg_rgb);
        let mut p = ThemePalette::from_accent_rgb(cfg.apps_accent_rgb);
        p.accent = acc;
        p.rim = acc;
        p.accent_light = (
            acc.0.saturating_add(36),
            acc.1.saturating_add(36),
            acc.2.saturating_add(36),
        );
        p.accent_deep = (
            acc.0.saturating_sub(42),
            acc.1.saturating_sub(42),
            acc.2.saturating_sub(42),
        );
        p.text_primary = txt;
        p.text_muted = (
            ((txt.0 as u16 * 3 + bg.0 as u16) / 4) as u8,
            ((txt.1 as u16 * 3 + bg.1 as u16) / 4) as u8,
            ((txt.2 as u16 * 3 + bg.2 as u16) / 4) as u8,
        );
        p.glass_top = bg;
        p.glass_bot = (
            bg.0.saturating_sub(12),
            bg.1.saturating_sub(12),
            bg.2.saturating_sub(12),
        );
        p.preview_a = bg;
        p.preview_b = blend_rgb(bg, acc, 0.28);
        p.preview_c = p.glass_bot;
        p.swatches = [
            cfg.apps_accent_rgb,
            rgb_to_u32(p.accent_light),
            rgb_to_u32(p.accent_deep),
            cfg.apps_bg_rgb,
            cfg.apps_text_rgb,
        ];
        p
    } else if cfg.tb_match_wallpaper {
        resolve_palette_for(cfg, true)
    } else if let Some(dp) = crate::dock_themes::dock_palette(cfg.tb_theme) {
        dp
    } else if let Some(suite) = crate::suites::find_suite_by_id(&cfg.active_suite) {
        palette_from_suite_preset(suite)
    } else if cfg.dt_match_wallpaper {
        resolve_palette_for(cfg, true)
    } else if cfg.dt_use_custom_color {
        resolve_clock_palette(cfg)
    } else if cfg.music_match_wallpaper {
        resolve_palette_for(cfg, true)
    } else if cfg.music_use_custom_color {
        resolve_music_palette(cfg)
    } else {
        ThemePalette::autumn()
    };

    if let Some(card_rgb) = tr_card.color_rgb {
        let bg = u32_to_rgb(card_rgb);
        pal.glass_top = bg;
        pal.glass_bot = if let Some(c2) = tr_card.color2_rgb {
            u32_to_rgb(c2)
        } else {
            (bg.0 / 2, bg.1 / 2, bg.2 / 2)
        };
    } else if let Some(c2) = tr_card.color2_rgb {
        pal.glass_bot = u32_to_rgb(c2);
    }
    if let Some(b_rgb) = tr_card.border_rgb {
        pal.rim = u32_to_rgb(b_rgb);
    }
    pal
}

/// Resolves the `(shadow_rgb, highlight_rgb)` icon duotone tint when App Launchers "Match Theme" is enabled.
pub fn resolve_apps_icon_tint(cfg: &AppConfig) -> Option<((u8, u8, u8), (u8, u8, u8))> {
    if !cfg.apps_match_theme {
        return None;
    }
    let pal = resolve_apps_palette(cfg);
    let tr_tiles = cfg.apps_elem_transforms[0];
    if tr_tiles.color_rgb.is_none()
        && tr_tiles.color2_rgb.is_none()
        && pal == ThemePalette::autumn()
    {
        return Some(AUTUMN_ICON_TINT);
    }
    let base = tr_tiles.color_rgb.map(u32_to_rgb).unwrap_or(pal.accent);
    let dark = blend_rgb(base, pal.accent_deep, 0.26);
    let light = if let Some(c2) = tr_tiles.color2_rgb {
        u32_to_rgb(c2)
    } else {
        let (_, _, txt_l) = rgb_to_hsl(pal.text_primary);
        let hi_src = if txt_l >= 0.48 {
            pal.text_primary
        } else {
            (244, 248, 255)
        };
        blend_rgb(hi_src, pal.accent_light, 0.28)
    };
    Some((dark, light))
}

/// Injective cache key encoding for an optional duotone icon tint.
#[inline]
pub fn tint_u64_key(tint: Option<((u8, u8, u8), (u8, u8, u8))>) -> u64 {
    match tint {
        None => 0,
        Some(((r0, g0, b0), (r1, g1, b1))) => {
            (1u64 << 48)
                | ((r0 as u64) << 40)
                | ((g0 as u64) << 32)
                | ((b0 as u64) << 24)
                | ((r1 as u64) << 16)
                | ((g1 as u64) << 8)
                | (b1 as u64)
        }
    }
}

/// Resolves palette specifically for the Date & Time widget (Wallpaper Match -> Custom Colors -> Style Native Artistic Palette).
pub fn resolve_clock_palette(cfg: &AppConfig) -> ThemePalette {
    let card_tr = cfg
        .dt_elem_transforms
        .get(crate::config::ClockElemId::Card.idx())
        .copied()
        .unwrap_or_default();
    let mut pal = if cfg.dt_match_wallpaper {
        resolve_palette_for(cfg, true)
    } else if cfg.dt_use_custom_color {
        let mut p = ThemePalette::from_accent_rgb(cfg.dt_accent_rgb);
        p.text_primary = u32_to_rgb(cfg.dt_digits_rgb);
        p.rim = u32_to_rgb(cfg.dt_accent_rgb);
        let bg = u32_to_rgb(cfg.dt_bg_rgb);
        p.glass_top = bg;
        p.glass_bot = (bg.0 / 2, bg.1 / 2, bg.2 / 2);
        p
    } else {
        ThemePalette::for_clock_style(cfg.dt_style)
    };
    if let Some(card_rgb) = card_tr.color_rgb {
        let bg = u32_to_rgb(card_rgb);
        pal.glass_top = bg;
        pal.glass_bot = if let Some(c2) = card_tr.color2_rgb {
            u32_to_rgb(c2)
        } else {
            (bg.0 / 2, bg.1 / 2, bg.2 / 2)
        };
    } else if let Some(c2) = card_tr.color2_rgb {
        pal.glass_bot = u32_to_rgb(c2);
    }
    if let Some(b_rgb) = card_tr.border_rgb {
        pal.rim = u32_to_rgb(b_rgb);
    }
    pal
}

/// Resolves palette specifically for the Music Bar widget (Wallpaper Match -> Custom Colors -> Style Native Artistic Palette).
pub fn resolve_music_palette(cfg: &AppConfig) -> ThemePalette {
    let tr_card = cfg.music_elem_transforms[5];
    let mut pal = if cfg.music_match_wallpaper {
        resolve_palette_for(cfg, true)
    } else if cfg.music_use_custom_color {
        let acc = u32_to_rgb(cfg.music_accent_rgb);
        let txt = u32_to_rgb(cfg.music_text_rgb);
        let bg = u32_to_rgb(cfg.music_bg_rgb);
        let mut p = ThemePalette::from_accent_rgb(cfg.music_accent_rgb);
        p.accent = acc;
        p.rim = acc;
        p.accent_light = (
            acc.0.saturating_add(36),
            acc.1.saturating_add(36),
            acc.2.saturating_add(36),
        );
        p.accent_deep = (
            acc.0.saturating_sub(42),
            acc.1.saturating_sub(42),
            acc.2.saturating_sub(42),
        );
        p.text_primary = txt;
        p.text_muted = (
            ((txt.0 as u16 * 3 + bg.0 as u16) / 4) as u8,
            ((txt.1 as u16 * 3 + bg.1 as u16) / 4) as u8,
            ((txt.2 as u16 * 3 + bg.2 as u16) / 4) as u8,
        );
        p.glass_top = bg;
        p.glass_bot = (
            bg.0.saturating_sub(12),
            bg.1.saturating_sub(12),
            bg.2.saturating_sub(12),
        );
        p
    } else {
        ThemePalette::for_music_style(cfg.music_style)
    };
    if let Some(card_rgb) = tr_card.color_rgb {
        let bg = u32_to_rgb(card_rgb);
        pal.glass_top = bg;
        pal.glass_bot = if let Some(c2) = tr_card.color2_rgb {
            u32_to_rgb(c2)
        } else {
            (bg.0 / 2, bg.1 / 2, bg.2 / 2)
        };
    } else if let Some(c2) = tr_card.color2_rgb {
        pal.glass_bot = u32_to_rgb(c2);
    }
    if let Some(b_rgb) = tr_card.border_rgb {
        pal.rim = u32_to_rgb(b_rgb);
    }
    pal
}

// =========================================================================
// System + Embedded Font Discovery & Categorization
// =========================================================================
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FontCategory {
    All,
    Calligraphy,
    Modern,
    Serif,
    MonoTech,
}

impl FontCategory {
    pub fn label(self) -> &'static str {
        match self {
            FontCategory::All => "All Fonts",
            FontCategory::Calligraphy => "Calligraphy",
            FontCategory::Modern => "Modern & Text",
            FontCategory::Serif => "Editorial Serif",
            FontCategory::MonoTech => "Mono & Tech",
        }
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct FontEntry {
    pub family: String,
    pub category: FontCategory,
    pub is_embedded: bool,
}

fn classify_font_family(name: &str) -> FontCategory {
    let low = name.to_ascii_lowercase();
    let calligraphy_keys = [
        "tuesday night",
        "script",
        "calligraph",
        "brush",
        "corsiva",
        "handwriting",
        "cursive",
        "gabriola",
        "vivaldi",
        "pristina",
        "mistral",
        "blackadder",
        "bradley",
        "freestyle",
        "harlow",
        "informal",
        "ink free",
        "kristen",
        "magneto",
        "matura",
        "parchment",
        "rage",
        "viner",
        "curlz",
        "forte",
        "gigi",
        "juice",
        "jokerman",
        "tempus",
        "lucida handwriting",
        "chiller",
        "mv boli",
        "kaiti",
        "xingkai",
    ];
    if calligraphy_keys.iter().any(|k| low.contains(k)) {
        return FontCategory::Calligraphy;
    }

    let mono_keys = [
        "mono",
        "code",
        "console",
        "consolas",
        "courier",
        "digital",
        "lcd",
        "hooge",
        "ocr",
        "stencil",
        "typewriter",
        "terminal",
        "cascadia",
    ];
    if mono_keys.iter().any(|k| low.contains(k)) {
        return FontCategory::MonoTech;
    }

    let serif_keys = [
        "bodoni",
        "baskerville",
        "book antiqua",
        "bookman",
        "californian",
        "calisto",
        "cambria",
        "castellar",
        "centaur",
        "century schoolbook",
        "century",
        "colonna",
        "constantia",
        "elephant",
        "engravers",
        "felix",
        "footlight",
        "garamond",
        "georgia",
        "goudy",
        "harrington",
        "high tower",
        "imprint",
        "lucida bright",
        "lucida fax",
        "modern no",
        "niagara",
        "old english",
        "palatino",
        "papyrus",
        "perpetua",
        "playbill",
        "poor richard",
        "rockwell",
        "sitka",
        "sylfaen",
        "times new roman",
        "wide latin",
        "serif",
    ];
    if serif_keys.iter().any(|k| low.contains(k)) && !low.contains("century gothic") && !low.contains("sans serif") {
        return FontCategory::Serif;
    }

    FontCategory::Modern
}

fn is_skipped_symbol_font(name: &str) -> bool {
    if name.is_empty() || name.starts_with('@') {
        return true;
    }
    let low = name.to_ascii_lowercase();
    matches!(
        low.as_str(),
        "marlett"
            | "symbol"
            | "webdings"
            | "wingdings"
            | "wingdings 2"
            | "wingdings 3"
            | "mt extra"
            | "bookshelf symbol 7"
            | "segoe fluent icons"
            | "segoe mdl2 assets"
            | "segoe ui symbol"
            | "segoe ui emoji"
            | "ms outlook"
            | "ms reference specialty"
    )
}

unsafe fn extract_family_name(fam: &IDWriteFontFamily) -> Option<String> {
    let names: IDWriteLocalizedStrings = fam.GetFamilyNames().ok()?;
    let count = names.GetCount();
    if count == 0 {
        return None;
    }
    let mut idx = 0u32;
    let mut exists = windows::core::BOOL(0);
    let _ = names.FindLocaleName(w!("en-US"), &mut idx, &mut exists);
    let use_idx = if exists.as_bool() { idx } else { 0 };
    let len = names.GetStringLength(use_idx).ok()?;
    let mut buf = vec![0u16; (len + 1) as usize];
    names.GetString(use_idx, &mut buf).ok()?;
    let s = String::from_utf16_lossy(&buf[..len as usize]).trim().to_string();
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

unsafe fn collect_names_from_collection(
    coll: &IDWriteFontCollection,
    is_embedded: bool,
    seen: &mut HashSet<String>,
    out: &mut Vec<FontEntry>,
) {
    let cnt = coll.GetFontFamilyCount();
    for i in 0..cnt {
        let Ok(fam) = coll.GetFontFamily(i) else {
            continue;
        };
        let Some(name) = extract_family_name(&fam) else {
            continue;
        };
        if is_skipped_symbol_font(&name) {
            continue;
        }
        let key = name.to_ascii_lowercase();
        if !seen.insert(key) {
            continue;
        }
        let cat = classify_font_family(&name);
        out.push(FontEntry {
            family: name,
            category: cat,
            is_embedded,
        });
    }
}

static CACHED_ALL_FONTS: OnceLock<Vec<FontEntry>> = OnceLock::new();

pub unsafe fn list_all_fonts(
    dw: &IDWriteFactory,
    custom_fonts: Option<&IDWriteFontCollection>,
) -> &'static [FontEntry] {
    CACHED_ALL_FONTS.get_or_init(|| {
        let mut seen = HashSet::new();
        let mut out = Vec::new();
        if let Some(cf) = custom_fonts {
            collect_names_from_collection(cf, true, &mut seen, &mut out);
        }
        let mut sys_coll: Option<IDWriteFontCollection> = None;
        if dw.GetSystemFontCollection(&mut sys_coll, false).is_ok() {
            if let Some(ref sc) = sys_coll {
                collect_names_from_collection(sc, false, &mut seen, &mut out);
            }
        }
        // Keep embedded studio fonts near top, then sort remaining alphabetically by family name
        out.sort_by(|a, b| {
            b.is_embedded
                .cmp(&a.is_embedded)
                .then_with(|| a.family.to_ascii_lowercase().cmp(&b.family.to_ascii_lowercase()))
        });
        out
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid_bgra(r: u8, g: u8, b: u8, w: u32, h: u32) -> Vec<u8> {
        let total = (w * h) as usize;
        let mut out = Vec::with_capacity(total * 4);
        for _ in 0..total {
            out.push(b);
            out.push(g);
            out.push(r);
            out.push(255);
        }
        out
    }

    #[test]
    fn palette_lerp_endpoints_and_midpoint() {
        let a = ThemePalette::from_accent_rgb(0xFF4422);
        let b = ThemePalette::from_accent_rgb(0x2288FF);
        assert_eq!(a.lerp(b, 0.0), a);
        assert_eq!(a.lerp(b, 1.0), b);
        let mid = a.lerp(b, 0.5);
        assert!(mid.delta_distance(a) > 0);
        assert!(mid.delta_distance(b) > 0);
    }

    #[test]
    fn live_chameleon_cache_smooths_transitions_and_ignores_transient_black() {
        let mut cache = ExtractedCache::default();
        let warm = solid_bgra(235, 130, 40, 16, 9);
        let cool = solid_bgra(40, 180, 245, 16, 9);
        let black = solid_bgra(1, 1, 1, 16, 9);

        // First frame initializes immediately
        assert!(cache.update("clip.mp4", &warm, 16, 9, true));
        let start_pal = cache.state.as_ref().unwrap().palette;

        // Transient black loop-fade frame must not clobber active palette
        assert!(!cache.update("clip.mp4", &black, 16, 9, true));
        assert_eq!(cache.state.as_ref().unwrap().palette, start_pal);

        // Scene change to cool blue converges progressively without single-frame jump
        assert!(cache.update("clip.mp4", &cool, 16, 9, true));
        let step1 = cache.state.as_ref().unwrap().palette;
        let target_pal = cache.state.as_ref().unwrap().target_palette;
        assert_ne!(step1, target_pal);
        let dist1 = step1.delta_distance(target_pal);

        let mut prev_dist = dist1;
        for _ in 0..16 {
            let _ = cache.update("clip.mp4", &cool, 16, 9, true);
            let cur = cache.state.as_ref().unwrap().palette;
            let dist = cur.delta_distance(target_pal);
            assert!(dist <= prev_dist);
            prev_dist = dist;
        }
        assert_eq!(cache.state.as_ref().unwrap().palette, target_pal);
        // Steady frames after convergence emit no redundant update events
        assert!(!cache.update("clip.mp4", &cool, 16, 9, true));
    }

    #[test]
    fn clock_card_bg_and_canvas_card_override_palette() {
        let mut cfg = crate::config::AppConfig::default();
        cfg.dt_match_wallpaper = false;
        cfg.dt_use_custom_color = true;
        cfg.dt_bg_rgb = 0x204060;
        let p = resolve_clock_palette(&cfg);
        assert_eq!(p.glass_top, (0x20, 0x40, 0x60));
        assert_eq!(p.glass_bot, (0x10, 0x20, 0x30));

        // Canvas Card slot color overrides plate glass even when manual Colors toggle is off
        cfg.dt_use_custom_color = false;
        cfg.dt_elem_transforms[crate::config::ClockElemId::Card.idx()].color_rgb = Some(0x8833AA);
        let p2 = resolve_clock_palette(&cfg);
        assert_eq!(p2.glass_top, (0x88, 0x33, 0xAA));
        assert_eq!(p2.glass_bot, (0x44, 0x19, 0x55));
    }

    #[test]
    fn all_clock_and_music_styles_use_curated_artistic_palettes_not_monochrome_yellow() {
        let yellow = ThemePalette::autumn().accent;
        let mut clock_accents = HashSet::new();
        for &style in &crate::config::ClockStyle::ALL {
            let mut cfg = crate::config::AppConfig::default();
            cfg.dt_style = style;
            cfg.dt_match_wallpaper = false;
            cfg.dt_use_custom_color = false;
            let p = resolve_clock_palette(&cfg);
            assert_ne!(
                p.accent, yellow,
                "Clock style {:?} must not fall back to flat Autumn yellow (0xF5A642)",
                style
            );
            clock_accents.insert(p.accent);
        }
        assert!(
            clock_accents.len() >= 20,
            "Clock themes must span diverse artistic palettes, got only {} distinct accents",
            clock_accents.len()
        );

        let mut music_accents = HashSet::new();
        for &style in &crate::config::MusicStyle::ALL {
            let mut cfg = crate::config::AppConfig::default();
            cfg.music_style = style;
            cfg.music_match_wallpaper = false;
            cfg.music_use_custom_color = false;
            let p = resolve_music_palette(&cfg);
            assert_ne!(
                p.accent, yellow,
                "Music style {:?} must not fall back to flat Autumn yellow (0xF5A642)",
                style
            );
            music_accents.insert(p.accent);
        }
        assert!(
            music_accents.len() >= 20,
            "Music themes must span diverse artistic palettes, got only {} distinct accents",
            music_accents.len()
        );
    }

    #[test]
    fn apps_dock_match_theme_resolves_active_suite_dock_custom_and_wallpaper_palettes() {
        // 1. Default with Match Theme OFF returns None (native full-color icons)
        let mut cfg = crate::config::AppConfig::default();
        cfg.apps_match_theme = false;
        assert_eq!(resolve_apps_icon_tint(&cfg), None);

        // 2. Enabling Match Theme with Nordic Architect suite tints crisp Nordic ice-blue (not Autumn amber!)
        let nordic = crate::suites::find_suite_by_id("nordic_architect")
            .expect("nordic_architect suite must exist");
        crate::suites::apply_suite_to_config(nordic, &mut cfg);
        cfg.apps_match_theme = true;
        let pal = resolve_apps_palette(&cfg);
        assert_eq!(pal.accent, (0x56, 0xB4, 0xFF));
        let (dark, light) = resolve_apps_icon_tint(&cfg).expect("Match Theme should return tint");
        assert_ne!((dark, light), AUTUMN_ICON_TINT);
        assert!(
            dark.2 > dark.0 && light.2 >= light.0,
            "Nordic Architect icon tint should be blue-dominant, got dark={:?} light={:?}",
            dark,
            light
        );

        // 3. Turning off apps_use_custom_color still inherits active dock/suite theme instead of Autumn amber
        cfg.apps_use_custom_color = false;
        let inherited_pal = resolve_apps_palette(&cfg);
        assert_ne!(inherited_pal.accent, ThemePalette::autumn().accent);
        let (inh_dark, _) = resolve_apps_icon_tint(&cfg).expect("Inherited theme should tint");
        assert_ne!((inh_dark, light), AUTUMN_ICON_TINT);

        // 4. Pen Canvas Card & App Tiles overrides apply directly and alter cache key
        let key_before = tint_u64_key(resolve_apps_icon_tint(&cfg));
        cfg.apps_elem_transforms[0].color_rgb = Some(0xEE3355);
        cfg.apps_elem_transforms[4].color_rgb = Some(0x112233);
        let custom_pal = resolve_apps_palette(&cfg);
        assert_eq!(custom_pal.glass_top, (0x11, 0x22, 0x33));
        let (tile_dark, _) = resolve_apps_icon_tint(&cfg).expect("Canvas tile tint expected");
        assert!(tile_dark.0 > tile_dark.2, "Crimson override should be red-dominant");
        let key_after = tint_u64_key(resolve_apps_icon_tint(&cfg));
        assert_ne!(key_before, key_after);
        assert_eq!(tint_u64_key(None), 0);
    }

    #[test]
    fn perceptual_oklab_wallpaper_intelligence_extracts_zones_dual_harmony_and_wcag_contrast() {
        // 1. OKLab round-trip accuracy on representative colors
        for rgb in [(245, 166, 66), (42, 180, 250), (36, 198, 92), (242, 240, 236), (18, 22, 32)] {
            let (l, a, b) = rgb_to_oklab(rgb);
            let rt = oklab_to_rgb(l, a, b);
            assert!(
                (rt.0 as i32 - rgb.0 as i32).abs() <= 2
                    && (rt.1 as i32 - rgb.1 as i32).abs() <= 2
                    && (rt.2 as i32 - rgb.2 as i32).abs() <= 2,
                "OKLab roundtrip drifted for {:?} -> {:?}",
                rgb,
                rt
            );
        }

        // 2. Multi-zone scene: bright warm paper sky (top 65%), golden amber center burst, emerald green grass bottom 25%
        let (w, h) = (64usize, 36usize);
        let mut bgra = vec![0u8; w * h * 4];
        for y in 0..h {
            for x in 0..w {
                let (r, g, b) = if y >= 27 {
                    (18u8, 178u8, 36u8) // vibrant meadow green ground strip
                } else if (20..44).contains(&x) && (8..22).contains(&y) {
                    (236u8, 152u8, 44u8) // warm golden focal subject in center
                } else {
                    (236u8, 234u8, 228u8) // bright sketchbook paper sky
                };
                let off = (y * w + x) * 4;
                bgra[off] = b;
                bgra[off + 1] = g;
                bgra[off + 2] = r;
                bgra[off + 3] = 255;
            }
        }
        let (pal, prof) = ThemePalette::analyze_bgra_pixels(&bgra, w as u32, h as u32);
        assert!(prof.is_light_scene, "Bright paper-sky scene must detect light mood (L={})", prof.scene_lightness);
        assert!(
            prof.top_zone_rgb.0 > 210 && prof.top_zone_rgb.1 > 210 && prof.top_zone_rgb.2 > 205,
            "Top zone should reflect bright paper sky, got {:?}",
            prof.top_zone_rgb
        );
        assert!(
            prof.bottom_zone_rgb.1 > prof.bottom_zone_rgb.0 + 80
                && prof.bottom_zone_rgb.1 > prof.bottom_zone_rgb.2 + 80,
            "Bottom zone should reflect green meadow strip, got {:?}",
            prof.bottom_zone_rgb
        );
        // Primary & Secondary must capture BOTH distinct hues (gold + green), not monochrome +7 deg!
        let (_, _, h_pri) = {
            let (l, a, b) = rgb_to_oklab(prof.primary_accent);
            oklab_to_oklch(l, a, b)
        };
        let (_, _, h_sec) = {
            let (l, a, b) = rgb_to_oklab(prof.secondary_accent);
            oklab_to_oklch(l, a, b)
        };
        assert!(
            hue_dist_deg(h_pri, h_sec) >= 34.0,
            "Multi-color scene must extract distinct primary ({:.1}°) and secondary ({:.1}°) hues",
            h_pri,
            h_sec
        );
        // WCAG readability guarantees
        assert!(
            wcag_contrast_ratio(pal.text_primary, pal.glass_top) >= 10.0,
            "Primary text must pass high WCAG contrast over dark glass"
        );
        assert!(
            wcag_contrast_ratio(pal.accent, pal.glass_top) >= 4.5,
            "Primary accent must pass WCAG AA contrast over dark glass"
        );
        // Airy light glass stays bright & soft, dark glass stays deep & atmospheric
        assert!(wcag_relative_luminance(prof.light_glass_top) > 0.65);
        assert!(wcag_relative_luminance(prof.dark_glass_top) < 0.14);
        // Light-surface accent passes WCAG AA over light glass!
        assert!(
            wcag_contrast_ratio(prof.on_light_accent, prof.light_glass_top) >= 4.4,
            "on_light_accent {:?} must remain legible on light glass {:?}",
            prof.on_light_accent,
            prof.light_glass_top
        );
        // Edge-aware glass gradient differs cleanly between Top (paper sky) and Bottom (green grass)
        let (top_gt, _, _) = prof.glass_gradient_for_edge(crate::config::MacDockEdge::Top, false);
        let (bot_gt, _, _) = prof.glass_gradient_for_edge(crate::config::MacDockEdge::Bottom, false);
        assert_ne!(top_gt, bot_gt, "Top and Bottom edge glass must reflect distinct spatial wallpaper zones");
        assert!(bot_gt.1 >= bot_gt.0 && bot_gt.1 >= bot_gt.2, "Bottom edge dark glass should reflect green grass tint, got {:?}", bot_gt);
        assert!(prof.dynamic_range > 0.20, "High-contrast scene should report real dynamic range");
        assert!(!prof.mood_tag().is_empty());
    }
}
