//! True Physically-Based Direct2D Material & Interactive Physics Engine.
//!
//! Synthesizes cached per-pixel normal-mapped PBR surface textures (Voronoi water caustics,
//! 2x2 diagonal twill carbon fiber, anisotropic CNC brushed titanium, cathedral walnut woodgrain,
//! pebble-grain full-grain saddle leather, faceted glacial ice, Carrara marble with kintsugi gold,
//! wet twilight condensation glass, abyssal marine depth, alpine forest mist, golden-hour coastal
//! horizon, and woven linen) and layers real-time cursor/click-reactive optical & fluid physics,
//! dual-layer drop shadows, 45° specular glass sheen, and 135° incident-light bevel rims.

use std::cell::RefCell;
use std::collections::HashMap;
use std::f32::consts::PI;
use std::ffi::c_void;
use std::sync::OnceLock;

use windows::core::Interface;
use windows::Win32::Graphics::Direct2D::Common::{
    D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_COLOR_F, D2D1_GRADIENT_STOP, D2D1_PIXEL_FORMAT,
    D2D_RECT_F, D2D_SIZE_U,
};
use windows::Win32::Graphics::Direct2D::{
    ID2D1Bitmap, ID2D1BitmapBrush, ID2D1Brush, ID2D1RenderTarget, ID2D1Factory,
    ID2D1Geometry, ID2D1Layer, ID2D1LinearGradientBrush, ID2D1RadialGradientBrush,
    D2D1_ANTIALIAS_MODE_PER_PRIMITIVE, D2D1_BITMAP_BRUSH_PROPERTIES,
    D2D1_BITMAP_INTERPOLATION_MODE_LINEAR, D2D1_BITMAP_PROPERTIES, D2D1_BRUSH_PROPERTIES,
    D2D1_ELLIPSE, D2D1_EXTEND_MODE_CLAMP, D2D1_GAMMA_2_2, D2D1_LAYER_OPTIONS_NONE,
    D2D1_LAYER_PARAMETERS, D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES,
    D2D1_RADIAL_GRADIENT_BRUSH_PROPERTIES, D2D1_ROUNDED_RECT,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows_numerics::{Matrix3x2, Vector2};

use crate::taskbar_styler::VizSnapshot;

const TEX_W: u32 = 256;
const TEX_H: u32 = 128;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum PbrMaterial {
    CalmOceanLagoon = 0,
    RainyWindowpane = 1,
    BrushedTitanium = 2,
    VintageWalnutBrass = 3,
    DeepAbyssalTrench = 4,
    NordicGlacialIce = 5,
    ExecutiveLeatherLinen = 6,
    AlpineForestMist = 7,
    IndustrialCarbonSteel = 8,
    GoldenHourHorizon = 9,
    CarraraMarbleGold = 10,
    WovenLinenCalm = 11,
}

impl PbrMaterial {
    pub const COUNT: usize = 12;

    #[allow(dead_code)]
    pub const ALL: [PbrMaterial; 12] = [
        PbrMaterial::CalmOceanLagoon,
        PbrMaterial::RainyWindowpane,
        PbrMaterial::BrushedTitanium,
        PbrMaterial::VintageWalnutBrass,
        PbrMaterial::DeepAbyssalTrench,
        PbrMaterial::NordicGlacialIce,
        PbrMaterial::ExecutiveLeatherLinen,
        PbrMaterial::AlpineForestMist,
        PbrMaterial::IndustrialCarbonSteel,
        PbrMaterial::GoldenHourHorizon,
        PbrMaterial::CarraraMarbleGold,
        PbrMaterial::WovenLinenCalm,
    ];

    pub fn is_light_surface(self) -> bool {
        matches!(self, PbrMaterial::CarraraMarbleGold | PbrMaterial::WovenLinenCalm)
    }

    /// Authentic native base surface color for each PBR material.
    /// Used so matching suite palettes keep full PBR texture richness while explicit
    /// custom Card BG / Pen Canvas colors tint vividly on top.
    pub const fn native_base_rgb(self) -> u32 {
        match self {
            PbrMaterial::CalmOceanLagoon => 0x061826,
            PbrMaterial::RainyWindowpane => 0x1A1D20,
            PbrMaterial::BrushedTitanium => 0x1C1E22,
            PbrMaterial::VintageWalnutBrass => 0x231709,
            PbrMaterial::DeepAbyssalTrench => 0x020B14,
            PbrMaterial::NordicGlacialIce => 0x0B192C,
            PbrMaterial::ExecutiveLeatherLinen => 0x2B1810,
            PbrMaterial::AlpineForestMist => 0x0B1D13,
            PbrMaterial::IndustrialCarbonSteel => 0x121212,
            PbrMaterial::GoldenHourHorizon => 0x2B0938,
            PbrMaterial::CarraraMarbleGold => 0xECEAE4,
            PbrMaterial::WovenLinenCalm => 0xE8E0D4,
        }
    }

    pub fn spawns_cursor_ripples(self) -> bool {
        matches!(
            self,
            PbrMaterial::CalmOceanLagoon
                | PbrMaterial::RainyWindowpane
                | PbrMaterial::DeepAbyssalTrench
                | PbrMaterial::NordicGlacialIce
                | PbrMaterial::GoldenHourHorizon
        )
    }

    /// True only for fluid/atmospheric surfaces that continuously animate even while idle.
    /// Static tactile surfaces (titanium, walnut, leather, carbon, marble, linen) idle at 0 FPS
    /// and animate smoothly only on hover or click ripples.
    pub fn has_ambient_idle_motion(self) -> bool {
        matches!(
            self,
            PbrMaterial::CalmOceanLagoon
                | PbrMaterial::RainyWindowpane
                | PbrMaterial::DeepAbyssalTrench
                | PbrMaterial::NordicGlacialIce
                | PbrMaterial::AlpineForestMist
                | PbrMaterial::GoldenHourHorizon
        )
    }

    /// True when the interactive layer draws unclipped particles/ripples/rays that benefit from
    /// rounded geometry clipping.
    #[inline]
    pub fn needs_edge_clip_layer(self) -> bool {
        self.has_ambient_idle_motion()
    }

    pub fn has_glass_sheen(self) -> bool {
        matches!(
            self,
            PbrMaterial::CalmOceanLagoon
                | PbrMaterial::RainyWindowpane
                | PbrMaterial::DeepAbyssalTrench
                | PbrMaterial::NordicGlacialIce
                | PbrMaterial::AlpineForestMist
                | PbrMaterial::GoldenHourHorizon
                | PbrMaterial::CarraraMarbleGold
        )
    }

    pub fn accent_rim_rgb(self) -> (u8, u8, u8) {
        match self {
            PbrMaterial::CalmOceanLagoon => (144, 224, 239),
            PbrMaterial::RainyWindowpane => (182, 206, 220),
            PbrMaterial::BrushedTitanium => (220, 225, 227),
            PbrMaterial::VintageWalnutBrass => (212, 175, 55),
            PbrMaterial::DeepAbyssalTrench => (0, 255, 204),
            PbrMaterial::NordicGlacialIce => (160, 231, 229),
            PbrMaterial::ExecutiveLeatherLinen => (229, 184, 105),
            PbrMaterial::AlpineForestMist => (163, 177, 138),
            PbrMaterial::IndustrialCarbonSteel => (141, 153, 174),
            PbrMaterial::GoldenHourHorizon => (255, 209, 102),
            PbrMaterial::CarraraMarbleGold => (212, 175, 55),
            PbrMaterial::WovenLinenCalm => (176, 136, 104),
        }
    }
}

#[derive(Clone, Copy)]
pub struct PbrInteractiveInput {
    pub time_s: f32,
    pub hover_x: f32,
    pub hover_y: f32,
    pub hover_mix: f32,
    pub hover_vx: f32,
    pub hover_vy: f32,
    pub audio_energy: f32,
    /// Up to 4 active ripples: (nx, ny, progress 0..1, strength).
    pub ripples: [(f32, f32, f32, f32); 4],
    pub ripple_count: usize,
    pub compact_dock: bool,
}

impl Default for PbrInteractiveInput {
    fn default() -> Self {
        Self {
            time_s: 0.0,
            hover_x: 0.5,
            hover_y: 0.5,
            hover_mix: 0.0,
            hover_vx: 0.0,
            hover_vy: 0.0,
            audio_energy: 0.0,
            ripples: [(0.5, 0.5, 1.0, 0.0); 4],
            ripple_count: 0,
            compact_dock: false,
        }
    }
}

pub fn pbr_input_from_viz(
    viz: Option<&VizSnapshot>,
    fallback_phase: f32,
    is_hovered: bool,
) -> PbrInteractiveInput {
    let mut out = PbrInteractiveInput {
        time_s: fallback_phase,
        hover_mix: if is_hovered { 0.85 } else { 0.0 },
        ..Default::default()
    };
    if let Some(v) = viz {
        out.time_s = if v.phase.is_finite() && v.phase > 0.0001 {
            v.phase
        } else {
            fallback_phase
        };
        out.hover_x = v.hover_x.clamp(0.0, 1.0);
        out.hover_y = v.hover_y.clamp(0.0, 1.0);
        out.hover_mix = v.hover_mix.max(if is_hovered { 0.72 } else { 0.0 }).clamp(0.0, 1.0);
        out.hover_vx = v.hover_vx;
        out.hover_vy = v.hover_vy;
        out.audio_energy = v.energy.clamp(0.0, 1.5);
        let mut cnt = 0usize;
        for r in &v.ripples {
            if r.active && r.max_age > 0.01 && cnt < 4 {
                let p = (r.age / r.max_age).clamp(0.0, 1.0);
                out.ripples[cnt] = (r.x, r.y, p, r.strength);
                cnt += 1;
            }
        }
        out.ripple_count = cnt;
    }
    out
}

pub fn pbr_input_for_dock(
    anim_phase: f32,
    hover_mix: f32,
    cursor_axis_norm: f32,
    is_vertical: bool,
    click_ripple: Option<(f32, f32)>,
) -> PbrInteractiveInput {
    let hx = if is_vertical { 0.5 } else { cursor_axis_norm.clamp(0.0, 1.0) };
    let hy = if is_vertical { cursor_axis_norm.clamp(0.0, 1.0) } else { 0.5 };
    let mut out = PbrInteractiveInput {
        time_s: anim_phase,
        hover_x: hx,
        hover_y: hy,
        hover_mix: hover_mix.clamp(0.0, 1.0),
        hover_vx: 0.0,
        hover_vy: 0.0,
        audio_energy: 0.0,
        ripples: [(0.5, 0.5, 1.0, 0.0); 4],
        ripple_count: 0,
        compact_dock: true,
    };
    if let Some((axis, progress)) = click_ripple {
        if progress >= 0.0 && progress < 1.0 {
            let rx = if is_vertical { 0.5 } else { axis.clamp(0.0, 1.0) };
            let ry = if is_vertical { axis.clamp(0.0, 1.0) } else { 0.5 };
            out.ripples[0] = (rx, ry, progress, 1.15);
            out.ripple_count = 1;
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Deterministic procedural noise & Worley/Voronoi math (zero external crates)
// ---------------------------------------------------------------------------

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
fn hash22(ix: i32, iy: i32, seed: u32) -> (f32, f32) {
    (hash21(ix, iy, seed), hash21(ix, iy, seed.wrapping_add(91_813)))
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
fn lerp3(a: (f32, f32, f32), b: (f32, f32, f32), t: f32) -> (f32, f32, f32) {
    let k = t.clamp(0.0, 1.0);
    (lerp(a.0, b.0, k), lerp(a.1, b.1, k), lerp(a.2, b.2, k))
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
        sum += amp * value_noise(x, y, seed.wrapping_add(o as u32 * 31));
        norm += amp;
        x = x * 2.03 + 11.7;
        y = y * 2.01 + 5.3;
        amp *= 0.5;
    }
    if norm > 0.0 { sum / norm } else { 0.5 }
}

/// 2D Worley / Voronoi distance field returning (d1, d2, cell_hash, vec_dx, vec_dy).
fn voronoi(x: f32, y: f32, seed: u32) -> (f32, f32, f32, f32, f32) {
    let ix = x.floor() as i32;
    let iy = y.floor() as i32;
    let fx = x - ix as f32;
    let fy = y - iy as f32;
    let mut d1 = 9.0f32;
    let mut d2 = 9.0f32;
    let mut best_h = 0.5f32;
    let mut best_dx = 0.0f32;
    let mut best_dy = 0.0f32;
    for oy in -1..=1 {
        for ox in -1..=1 {
            let (rx, ry) = hash22(ix + ox, iy + oy, seed);
            let dx = ox as f32 + rx - fx;
            let dy = oy as f32 + ry - fy;
            let d = (dx * dx + dy * dy).sqrt();
            if d < d1 {
                d2 = d1;
                d1 = d;
                best_h = hash21(ix + ox, iy + oy, seed.wrapping_add(777));
                best_dx = dx;
                best_dy = dy;
            } else if d < d2 {
                d2 = d;
            }
        }
    }
    (d1, d2, best_h, best_dx, best_dy)
}

#[inline]
fn put_px(buf: &mut [u8], x: u32, y: u32, r: f32, g: f32, b: f32, a: f32) {
    let idx = ((y * TEX_W + x) * 4) as usize;
    let af = a.clamp(0.0, 1.0);
    // Direct2D premultiplied BGRA
    let bv = (b.clamp(0.0, 1.0) * af * 255.0).round() as u8;
    let gv = (g.clamp(0.0, 1.0) * af * 255.0).round() as u8;
    let rv = (r.clamp(0.0, 1.0) * af * 255.0).round() as u8;
    let av = (af * 255.0).round() as u8;
    buf[idx] = bv;
    buf[idx + 1] = gv;
    buf[idx + 2] = rv;
    buf[idx + 3] = av;
}

#[inline]
fn hex_f(rgb: u32) -> (f32, f32, f32) {
    (
        ((rgb >> 16) & 0xFF) as f32 / 255.0,
        ((rgb >> 8) & 0xFF) as f32 / 255.0,
        (rgb & 0xFF) as f32 / 255.0,
    )
}

// ---------------------------------------------------------------------------
// Per-Pixel PBR Material Texture Bakers (computed ONCE per material)
// ---------------------------------------------------------------------------

fn bake_material_bgra(mat: PbrMaterial) -> Vec<u8> {
    let mut buf = vec![0u8; (TEX_W * TEX_H * 4) as usize];
    let wf = TEX_W as f32;
    let hf = TEX_H as f32;

    for py in 0..TEX_H {
        let v = py as f32 / (hf - 1.0);
        for px in 0..TEX_W {
            let u = px as f32 / (wf - 1.0);
            let (r, g, b, a) = match mat {
                PbrMaterial::CalmOceanLagoon => {
                    // Beer-Lambert shallow sunlit turquoise lagoon + dual-layer Voronoi water caustics
                    let deep = hex_f(0x061826);
                    let azure = hex_f(0x0077B6);
                    let turq = hex_f(0x00B4D8);
                    let caust_c = hex_f(0x90E0EF);
                    let crest_c = hex_f(0xCAF0F8);

                    let depth_t = (v * 0.88 + (u - 0.5).abs() * 0.16).clamp(0.0, 1.0);
                    let mut col = if depth_t < 0.42 {
                        lerp3(turq, azure, depth_t / 0.42)
                    } else {
                        lerp3(azure, deep, (depth_t - 0.42) / 0.58)
                    };
                    // Subtle underwater refracted caustic web (2 displaced Voronoi ridges)
                    let warp = fbm(u * 4.5, v * 2.5, 11, 3) * 0.45;
                    let (d1a, d2a, _, _, _) = voronoi(u * 7.2 + warp, v * 4.2 - warp * 0.7, 17);
                    let ridge_a = (1.0 - ((d2a - d1a) * 4.4).clamp(0.0, 1.0)).powf(3.2);
                    let (d1b, d2b, _, _, _) = voronoi(u * 10.5 - warp * 0.8 + 2.3, v * 5.8 + warp, 43);
                    let ridge_b = (1.0 - ((d2b - d1b) * 4.8).clamp(0.0, 1.0)).powf(3.6);
                    let caustic = (ridge_a * 0.62 + ridge_b * 0.42) * (1.15 - v * 0.48);
                    col = lerp3(col, caust_c, (caustic * 0.65).clamp(0.0, 0.85));
                    let hot = (ridge_a * ridge_b * 3.4).clamp(0.0, 1.0);
                    col = lerp3(col, crest_c, hot * 0.75);
                    // Top surface backlit sun glow
                    let surf = (1.0 - (v * 3.4).clamp(0.0, 1.0)).powf(2.0);
                    col = lerp3(col, crest_c, surf * 0.24);
                    (col.0, col.1, col.2, 0.94)
                }

                PbrMaterial::RainyWindowpane => {
                    // Wet twilight glass + out-of-focus bokeh streetlamps + fine condensation mist
                    let slate = hex_f(0x1A1D20);
                    let storm = hex_f(0x2C3539);
                    let drop_c = hex_f(0x8A9EA7);
                    let mut col = lerp3(storm, slate, smoothstep(v * 0.92 + u * 0.15));

                    // Warm twilight bokeh + cool rain bokeh behind blurred glass
                    let bokehs = [
                        (0.18f32, 0.34f32, 0.22f32, hex_f(0xF6B067), 0.22f32),
                        (0.54, 0.62, 0.28, hex_f(0x6FAEE6), 0.20),
                        (0.82, 0.36, 0.21, hex_f(0xE89D68), 0.19),
                        (0.36, 0.22, 0.16, hex_f(0x9BC4DE), 0.14),
                    ];
                    for &(bx, by, br, bc, ba) in &bokehs {
                        let dx = (u - bx) * 1.45;
                        let dy = v - by;
                        let d = (dx * dx + dy * dy).sqrt();
                        let glow = (1.0 - (d / br).clamp(0.0, 1.0)).powf(1.8);
                        col = lerp3(col, bc, glow * ba);
                    }
                    // High-frequency frost/condensation micro-bead mist across pane
                    let fog_macro = fbm(u * 5.0, v * 3.2, 101, 3);
                    let micro_bead = value_noise(u * 96.0, v * 48.0, 107);
                    let cond = (fog_macro * 0.48 + micro_bead * 0.52) * (0.34 + 0.18 * (1.0 - v));
                    col = lerp3(col, drop_c, (cond * 0.36).clamp(0.0, 0.34));
                    (col.0, col.1, col.2, 0.94)
                }

                PbrMaterial::BrushedTitanium => {
                    // Authentic CNC anodized brushed titanium: 6-stop anisotropic light bands + micro-grain
                    let dark_ti = hex_f(0x1C1E22);
                    let mid_ti = hex_f(0x424852);
                    let silver = hex_f(0x6C747C);
                    let hi = hex_f(0xCBD2D6);

                    // 6-stop anisotropic metallic reflection profile at [0.0, 0.18, 0.35, 0.52, 0.74, 1.0]
                    let diag = (u * 0.78 + v * 0.22).clamp(0.0, 1.0);
                    let band = if diag < 0.18 {
                        lerp3(dark_ti, mid_ti, diag / 0.18)
                    } else if diag < 0.35 {
                        lerp3(mid_ti, silver, (diag - 0.18) / 0.17)
                    } else if diag < 0.52 {
                        lerp3(silver, hi, ((diag - 0.35) / 0.17) * 0.42)
                    } else if diag < 0.74 {
                        lerp3(lerp3(silver, hi, 0.42), mid_ti, (diag - 0.52) / 0.22)
                    } else {
                        lerp3(mid_ti, dark_ti, (diag - 0.74) / 0.26)
                    };
                    // Anisotropic horizontal CNC brushing micro-scratches (stretched x, sharp y)
                    let grain1 = value_noise(u * 5.0, v * 112.0, 211);
                    let grain2 = value_noise(u * 14.0, v * 218.0, 223);
                    let brush = (grain1 * 0.65 + grain2 * 0.35 - 0.5) * 0.22;
                    // Subtle CNC machined upper/lower chamfer bevel shading
                    let edge_v = if v < 0.08 {
                        0.12 * (1.0 - v / 0.08)
                    } else if v > 0.91 {
                        -0.16 * ((v - 0.91) / 0.09)
                    } else {
                        0.0
                    };
                    let l = brush + edge_v;
                    (
                        (band.0 + l).clamp(0.0, 1.0),
                        (band.1 + l).clamp(0.0, 1.0),
                        (band.2 + l * 1.04).clamp(0.0, 1.0),
                        0.98,
                    )
                }

                PbrMaterial::VintageWalnutBrass => {
                    // Hand-oiled American Black Walnut cathedral grain + fine open pores + satin finish
                    let dark_w = hex_f(0x231709);
                    let teak = hex_f(0x4A2E12);
                    let heart = hex_f(0x6C4321);
                    let sheen_c = hex_f(0xD4AF37);

                    let warp = fbm(u * 2.6, v * 1.9, 307, 4);
                    let ring_coord = v * 11.5 + warp * 4.4 + (u * 2.4).sin() * 0.65;
                    let ring = (ring_coord * PI).sin() * 0.5 + 0.5;
                    let sharp_ring = ring.powf(0.68);
                    let mut col = if sharp_ring < 0.55 {
                        lerp3(dark_w, teak, sharp_ring / 0.55)
                    } else {
                        lerp3(teak, heart, (sharp_ring - 0.55) / 0.45)
                    };
                    // Fine dark wood-vessel micro-pores
                    let pore = value_noise(u * 16.0, v * 92.0, 313);
                    if pore < 0.22 {
                        col = lerp3(col, dark_w, (0.22 - pore) * 2.8);
                    }
                    // Hand-rubbed tung oil 135° satin warmth
                    let sat = (1.0 - ((u - 0.32).powi(2) * 0.9 + (v - 0.28).powi(2) * 1.4).sqrt()).clamp(0.0, 1.0);
                    col = lerp3(col, sheen_c, sat.powf(2.4) * 0.14);
                    (col.0, col.1, col.2, 0.98)
                }

                PbrMaterial::DeepAbyssalTrench => {
                    // Pitch-deep oceanic trench with edge-lit cyan total internal reflection
                    let trench = hex_f(0x020B14);
                    let mid = hex_f(0x081C30);
                    let deep_glow = hex_f(0x0072BB);
                    let biolum = hex_f(0x00FFCC);

                    let edge_x = (u.min(1.0 - u) * 5.2).clamp(0.0, 1.0);
                    let edge_y = (v.min(1.0 - v) * 3.8).clamp(0.0, 1.0);
                    let edge_inv = 1.0 - (edge_x * edge_y).powf(0.55);

                    let turb = fbm(u * 5.5, v * 3.5, 409, 3);
                    let mut col = lerp3(trench, mid, (0.35 + turb * 0.45) * (1.0 - v * 0.35));
                    // Edge-lit total internal reflection halo
                    col = lerp3(col, deep_glow, edge_inv.powf(2.0) * 0.52);
                    col = lerp3(col, biolum, edge_inv.powf(4.6) * 0.38);
                    // Faint suspended marine snow particles in deep density filter
                    let snow = value_noise(u * 58.0, v * 32.0, 419);
                    if snow > 0.89 {
                        let sp = ((snow - 0.89) / 0.11).powf(2.0);
                        col = lerp3(col, biolum, sp * 0.32);
                    }
                    (col.0, col.1, col.2, 0.96)
                }

                PbrMaterial::NordicGlacialIce => {
                    // Faceted Voronoi crystalline glacial ice with normal-mapped sparkling planes
                    let crevasse = hex_f(0x0B192C);
                    let core = hex_f(0x1E3E62);
                    let frost = hex_f(0xE0F4FF);
                    let prism = hex_f(0xA0E7E5);

                    let (d1, d2, cell_h, dx, dy) = voronoi(u * 6.8, v * 3.6, 521);
                    let facet_nx = (cell_h - 0.5) * 1.4 + dx * 0.45;
                    let facet_ny = ((cell_h * 7.3).fract() - 0.5) * 1.4 + dy * 0.45;
                    // 135° arctic sunlight vector (-0.707, -0.707)
                    let ndotl = ((-facet_nx * 0.707 - facet_ny * 0.707) * 0.5 + 0.5).clamp(0.0, 1.0);
                    let crack = (1.0 - ((d2 - d1) * 9.0).clamp(0.0, 1.0)).powf(2.2);
                    let micro_frost = fbm(u * 22.0, v * 14.0, 541, 3);

                    let mut col = lerp3(crevasse, core, 0.35 + 0.55 * (1.0 - v * 0.45));
                    col = lerp3(col, prism, ndotl.powf(1.8) * 0.42);
                    col = lerp3(col, frost, (crack * 0.64 + micro_frost * 0.18 + (1.0 - d1).powf(4.0) * 0.14).clamp(0.0, 0.88));
                    (col.0, col.1, col.2, 0.92)
                }

                PbrMaterial::ExecutiveLeatherLinen => {
                    // Voronoi pebble-grain full-grain saddle brown leather shaded at 135° + burnished edge dye
                    let saddle = hex_f(0x2B1810);
                    let patina = hex_f(0x47281B);
                    let edge_dye = hex_f(0x140A07);

                    let (d1, _d2, cell_h, dx, dy) = voronoi(u * 38.0, v * 20.0, 613);
                    // Dome normal of each leather pebble grain cell
                    let dome_l = ((-dx * 0.707 - dy * 0.707) * 0.85 + (1.0 - d1 * 1.25) * 0.55).clamp(-0.4, 1.0);
                    let grain_var = (cell_h - 0.5) * 0.12;
                    let mut col = lerp3(saddle, patina, (dome_l * 0.48 + 0.34 + grain_var).clamp(0.0, 1.0));
                    // Subtle larger organic hide crease variation
                    let crease = fbm(u * 6.0, v * 4.0, 619, 3);
                    col = lerp3(col, patina, (crease - 0.45).max(0.0) * 0.35);
                    // Burnished edge dye & depressed perimeter stitch channel
                    let ex = u.min(1.0 - u);
                    let ey = v.min(1.0 - v);
                    let border_d = ex.min(ey * 0.65);
                    let burnish = (1.0 - (border_d / 0.09).clamp(0.0, 1.0)).powf(1.6);
                    col = lerp3(col, edge_dye, burnish * 0.72);
                    (col.0, col.1, col.2, 0.99)
                }

                PbrMaterial::AlpineForestMist => {
                    // Evergreen pine canopy + damp alpine slate + soft sunlit morning mist layers
                    let pine = hex_f(0x0B1D13);
                    let earth = hex_f(0x1C2826);
                    let dew = hex_f(0xA3B18A);
                    let sun = hex_f(0xDAD7CD);

                    let mut col = lerp3(earth, pine, smoothstep(v * 0.85 + 0.1));
                    let mist1 = fbm(u * 3.2, v * 2.4, 709, 4);
                    let mist2 = fbm(u * 6.5 + 4.1, v * 4.2, 719, 3);
                    let horizon_band = (-(v - 0.36).powi(2) * 14.0).exp();
                    col = lerp3(col, dew, (mist1 * 0.38 + horizon_band * 0.24).clamp(0.0, 0.55));
                    // Soft top-left 135° morning sunlight bloom
                    let ray = (1.0 - ((u * 0.85 + v * 0.65) * 0.95).clamp(0.0, 1.0)).powf(2.1);
                    col = lerp3(col, sun, (ray * 0.28 + mist2 * ray * 0.14).clamp(0.0, 0.42));
                    (col.0, col.1, col.2, 0.95)
                }

                PbrMaterial::IndustrialCarbonSteel => {
                    // Authentic 2x2 diagonal twill dry-matte aerospace carbon-fiber weave!
                    let c_blk = hex_f(0x121212);
                    let c_mid = hex_f(0x22242A);
                    let c_hi = hex_f(0x424752);

                    let cell_px = 6.0f32;
                    let gx = px as f32 / cell_px;
                    let gy = py as f32 / cell_px;
                    let col_i = gx.floor() as i32;
                    let row_i = gy.floor() as i32;
                    let fx = gx.fract();
                    let fy = gy.fract();
                    // 2-over-2 diagonal twill stagger
                    let is_warp = (col_i + row_i).rem_euclid(4) < 2;
                    let (cyl_t, cross_t) = if is_warp { (fx, fy) } else { (fy, fx) };
                    // Cylindrical curvature normal of each woven carbon fiber tow lit at 135°
                    let arch = (cyl_t * PI).sin();
                    let ridge_light = if is_warp {
                        arch * (0.58 + 0.42 * (1.0 - cyl_t))
                    } else {
                        arch * (0.44 + 0.36 * (1.0 - cyl_t))
                    };
                    // Tiny micro-filament groove separating adjacent tows
                    let groove = ((cross_t * PI).sin()).powf(0.28);
                    let spec = (ridge_light * groove).clamp(0.0, 1.0);
                    let mut col = if spec < 0.48 {
                        lerp3(c_blk, c_mid, spec / 0.48)
                    } else {
                        lerp3(c_mid, c_hi, (spec - 0.48) / 0.52)
                    };
                    // Subtle overall diagonal clear-coat sheen
                    let coat = (1.0 - ((u * 0.7 + v * 0.3 - 0.35).abs() * 2.4).clamp(0.0, 1.0)).powf(2.0);
                    col = lerp3(col, c_hi, coat * 0.14);
                    (col.0, col.1, col.2, 0.99)
                }

                PbrMaterial::GoldenHourHorizon => {
                    // Coastal dusk sky-to-sea reflection + warm golden solar scattering + wavelets
                    let dusk = hex_f(0x2B0938);
                    let drift = hex_f(0x4E342E);
                    let amber = hex_f(0xFF9E00);
                    let gold = hex_f(0xFFD166);
                    let foam = hex_f(0xFFE3D8);

                    let horizon = 0.54f32;
                    let mut col = if v < horizon {
                        let t = v / horizon;
                        if t < 0.55 {
                            lerp3(dusk, drift, t / 0.55)
                        } else {
                            lerp3(drift, amber, ((t - 0.55) / 0.45).powf(1.15))
                        }
                    } else {
                        // Sea surface reflecting sunset sky with horizontal micro-wavelets
                        let t = (v - horizon) / (1.0 - horizon);
                        let wave = value_noise(u * 14.0, v * 52.0, 823);
                        let base_sea = lerp3(drift, dusk, (t * 0.85).clamp(0.0, 1.0));
                        let sun_col = (-(u - 0.52).powi(2) * 16.0).exp();
                        let glit = (sun_col * (0.45 + 0.55 * wave.powf(2.0)) * (1.0 - t * 0.55)).clamp(0.0, 1.0);
                        lerp3(base_sea, gold, glit * 0.72)
                    };
                    // Glowing solar disc aura right on coastal horizon
                    let sun_d = (((u - 0.52) * 1.65).powi(2) + ((v - horizon) * 2.6).powi(2)).sqrt();
                    let aura = (1.0 - (sun_d / 0.48).clamp(0.0, 1.0)).powf(2.4);
                    col = lerp3(col, gold, aura * 0.48);
                    col = lerp3(col, foam, aura.powf(3.0) * 0.36);
                    (col.0, col.1, col.2, 0.95)
                }

                PbrMaterial::CarraraMarbleGold => {
                    // Polished Italian Carrara white marble with domain-warped grey strata & kintsugi gold veins
                    let ivory = hex_f(0xF1ECE3);
                    let cool_stone = hex_f(0xDCD8CF);
                    let vein_grey = hex_f(0x8D8B86);
                    let kintsugi = hex_f(0xD4AF37);

                    let qx = fbm(u * 3.4, v * 2.2, 907, 4);
                    let qy = fbm(u * 3.4 + 5.2, v * 2.2 + 1.7, 919, 4);
                    let cloud = fbm(u * 4.2 + qx * 2.4, v * 2.8 + qy * 2.2, 929, 4);
                    let mut col = lerp3(ivory, cool_stone, cloud * 0.65);

                    // Primary mineral vein ridge
                    let vein_val = ((u * 2.6 + v * 1.9 + qx * 2.8) * PI).sin().abs();
                    let vein = (1.0 - (vein_val * 5.2).clamp(0.0, 1.0)).powf(3.2);
                    col = lerp3(col, vein_grey, vein * 0.52);

                    // Delicate golden kintsugi crack seam
                    let gold_val = ((u * 2.1 - v * 1.7 + qy * 3.1 + 0.6) * PI).sin().abs();
                    let gold_seam = (1.0 - (gold_val * 14.0).clamp(0.0, 1.0)).powf(3.4);
                    col = lerp3(col, kintsugi, gold_seam * 0.85);
                    (col.0, col.1, col.2, 0.98)
                }

                PbrMaterial::WovenLinenCalm => {
                    // Tactile orthogonal woven oatmeal linen warp/weft fabric normal shading
                    let linen_hi = hex_f(0xECE4D8);
                    let linen_mid = hex_f(0xDDD3C4);
                    let linen_sh = hex_f(0xBFB3A1);

                    let wx = (px as f32 * 0.85 * PI).sin();
                    let wy = (py as f32 * 0.85 * PI).sin();
                    let weave = if ((px / 2) + (py / 2)) % 2 == 0 {
                        0.5 + 0.28 * wx - 0.10 * wy
                    } else {
                        0.5 + 0.28 * wy - 0.10 * wx
                    };
                    let slub = (fbm(u * 16.0, v * 16.0, 991, 2) - 0.5) * 0.16;
                    let t = (weave + slub).clamp(0.0, 1.0);
                    let mut col = if t < 0.5 {
                        lerp3(linen_sh, linen_mid, t * 2.0)
                    } else {
                        lerp3(linen_mid, linen_hi, (t - 0.5) * 2.0)
                    };
                    let dusk_v = (v * 0.18).clamp(0.0, 0.2);
                    col = lerp3(col, linen_sh, dusk_v);
                    (col.0, col.1, col.2, 0.97)
                }
            };
            put_px(&mut buf, px, py, r, g, b, a);
        }
    }
    buf
}

fn material_pixels(mat: PbrMaterial) -> &'static [u8] {
    static CACHES: [OnceLock<Vec<u8>>; PbrMaterial::COUNT] = [
        OnceLock::new(),
        OnceLock::new(),
        OnceLock::new(),
        OnceLock::new(),
        OnceLock::new(),
        OnceLock::new(),
        OnceLock::new(),
        OnceLock::new(),
        OnceLock::new(),
        OnceLock::new(),
        OnceLock::new(),
        OnceLock::new(),
    ];
    CACHES[mat as usize].get_or_init(|| bake_material_bgra(mat)).as_slice()
}

// Thread-local GPU bitmap/brush/layer cache keyed by render target raw pointer (STA UI thread).
struct RtPbrCache {
    bitmaps: [Option<ID2D1Bitmap>; PbrMaterial::COUNT],
    brushes: [Option<ID2D1BitmapBrush>; PbrMaterial::COUNT],
    layer: Option<ID2D1Layer>,
}

impl RtPbrCache {
    fn new() -> Self {
        Self {
            bitmaps: std::array::from_fn(|_| None),
            brushes: std::array::from_fn(|_| None),
            layer: None,
        }
    }
}

thread_local! {
    static RT_BITMAP_CACHE: RefCell<HashMap<usize, RtPbrCache>> =
        RefCell::new(HashMap::new());
}

/// Evict cached Direct2D PBR bitmaps when a render target is released/recreated.
pub fn evict_rt(rt: &ID2D1RenderTarget) {
    let key = rt.as_raw() as usize;
    RT_BITMAP_CACHE.with(|c| {
        c.borrow_mut().remove(&key);
    });
}

/// Evict all thread-local cached Direct2D PBR bitmaps/brushes/layers before COM teardown (`CoUninitialize`).
pub fn evict_all() {
    RT_BITMAP_CACHE.with(|c| {
        c.borrow_mut().clear();
    });
}

unsafe fn get_or_create_bitmap(
    rt: &ID2D1RenderTarget,
    mat: PbrMaterial,
) -> Option<ID2D1Bitmap> {
    let key = rt.as_raw() as usize;
    let idx = mat as usize;
    let cached = RT_BITMAP_CACHE.with(|c| {
        let map = c.borrow();
        map.get(&key).and_then(|entry| entry.bitmaps[idx].clone())
    });
    if cached.is_some() {
        return cached;
    }

    let pixels = material_pixels(mat);
    let props = D2D1_BITMAP_PROPERTIES {
        pixelFormat: D2D1_PIXEL_FORMAT {
            format: DXGI_FORMAT_B8G8R8A8_UNORM,
            alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
        },
        dpiX: 96.0,
        dpiY: 96.0,
    };
    let bmp = rt
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

    RT_BITMAP_CACHE.with(|c| {
        let mut map = c.borrow_mut();
        if map.len() >= 4 && !map.contains_key(&key) {
            map.clear();
        }
        let entry = map.entry(key).or_insert_with(RtPbrCache::new);
        entry.bitmaps[idx] = Some(bmp.clone());
    });
    Some(bmp)
}

unsafe fn get_or_create_bitmap_brush(
    rt: &ID2D1RenderTarget,
    mat: PbrMaterial,
) -> Option<ID2D1BitmapBrush> {
    let key = rt.as_raw() as usize;
    let idx = mat as usize;
    let cached = RT_BITMAP_CACHE.with(|c| {
        let map = c.borrow();
        map.get(&key).and_then(|entry| entry.brushes[idx].clone())
    });
    if cached.is_some() {
        return cached;
    }
    let bmp = get_or_create_bitmap(rt, mat)?;
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
    RT_BITMAP_CACHE.with(|c| {
        let mut map = c.borrow_mut();
        let entry = map.entry(key).or_insert_with(RtPbrCache::new);
        entry.brushes[idx] = Some(brush.clone());
    });
    Some(brush)
}

unsafe fn get_or_create_layer(rt: &ID2D1RenderTarget) -> Option<ID2D1Layer> {
    let key = rt.as_raw() as usize;
    let cached = RT_BITMAP_CACHE.with(|c| {
        let map = c.borrow();
        map.get(&key).and_then(|entry| entry.layer.clone())
    });
    if cached.is_some() {
        return cached;
    }
    let layer = rt.CreateLayer(None).ok()?;
    RT_BITMAP_CACHE.with(|c| {
        let mut map = c.borrow_mut();
        let entry = map.entry(key).or_insert_with(RtPbrCache::new);
        entry.layer = Some(layer.clone());
    });
    Some(layer)
}

#[inline]
unsafe fn fill_card(
    rt: &ID2D1RenderTarget,
    rr: &D2D1_ROUNDED_RECT,
    chamfer_geo: Option<&ID2D1Geometry>,
    brush: &ID2D1Brush,
) {
    if let Some(cg) = chamfer_geo {
        rt.FillGeometry(cg, brush, None);
    } else if rr.radiusX <= 0.5 && rr.radiusY <= 0.5 {
        rt.FillRectangle(&rr.rect, brush);
    } else {
        rt.FillRoundedRectangle(rr, brush);
    }
}

// ---------------------------------------------------------------------------
// Direct2D Helper Builders
// ---------------------------------------------------------------------------

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
fn rgb_u32(rgb: u32, a: f32) -> D2D1_COLOR_F {
    rgba(((rgb >> 16) & 0xFF) as u8, ((rgb >> 8) & 0xFF) as u8, (rgb & 0xFF) as u8, a)
}

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

// ---------------------------------------------------------------------------
// 1. Dual-Layer Physical Drop Shadows
//    Layer A: Tight Contact Shadow (offset 0, 2*s, 45% black)
//    Layer B: Soft Ambient Occlusion (offset 0, 10*s, diffused ~30% black)
// ---------------------------------------------------------------------------

pub unsafe fn draw_pbr_drop_shadows(
    rt: &ID2D1RenderTarget,
    rect: D2D_RECT_F,
    radius: f32,
    s: f32,
    light_surface: bool,
) {
    let mult = if light_surface { 1.15 } else { 1.0 };
    // Layer B: Soft Ambient Occlusion (3 feathered passes around offset (0, 8..10*s))
    for &(expand, oy, a) in &[
        (9.5 * s, 9.5 * s, 0.085 * mult),
        (5.5 * s, 6.5 * s, 0.115 * mult),
        (2.4 * s, 4.0 * s, 0.135 * mult),
    ] {
        let rr = D2D1_ROUNDED_RECT {
            rect: D2D_RECT_F {
                left: rect.left - expand * 0.55,
                top: rect.top + oy - expand * 0.25,
                right: rect.right + expand * 0.55,
                bottom: rect.bottom + oy + expand * 0.45,
            },
            radiusX: radius + expand * 0.45,
            radiusY: radius + expand * 0.45,
        };
        if let Ok(br) = rt.CreateSolidColorBrush(&rgba(0, 0, 0, a), None) {
            rt.FillRoundedRectangle(&rr, &br);
        }
    }
    // Layer A: Crisp Contact Shadow right under physical edge (offset (0, 2*s), ~42% peak)
    let contact = D2D1_ROUNDED_RECT {
        rect: D2D_RECT_F {
            left: rect.left - 0.6 * s,
            top: rect.top + 1.8 * s,
            right: rect.right + 0.6 * s,
            bottom: rect.bottom + 2.4 * s,
        },
        radiusX: radius + 0.6 * s,
        radiusY: radius + 0.6 * s,
    };
    if let Ok(br) = rt.CreateSolidColorBrush(&rgba(0, 0, 0, (0.36 * mult).min(0.48)), None) {
        rt.FillRoundedRectangle(&contact, &br);
    }
}

// ---------------------------------------------------------------------------
// 2. Full PBR Surface + Interactive Real-Time Physics + 45° Glass Sheen
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
pub unsafe fn draw_pbr_surface_clipped(
    rt: &ID2D1RenderTarget,
    d2d: Option<&ID2D1Factory>,
    mat: PbrMaterial,
    rect: D2D_RECT_F,
    radius: f32,
    s: f32,
    opacity: f32,
    tint_override: Option<(u32, Option<u32>)>,
    chamfer_geo: Option<&ID2D1Geometry>,
    inter: &PbrInteractiveInput,
) {
    let w = (rect.right - rect.left).max(8.0);
    let h = (rect.bottom - rect.top).max(8.0);
    let rr = D2D1_ROUNDED_RECT {
        rect,
        radiusX: radius,
        radiusY: radius,
    };

    // A. Draw cached PBR material directly via BitmapBrush + FillRoundedRectangle / FillGeometry
    // (eliminates per-frame offscreen ID2D1Layer creation!).
    let op = opacity.clamp(0.18, 1.0);
    if let Some(bmp_brush) = get_or_create_bitmap_brush(rt, mat) {
        bmp_brush.SetOpacity(op);
        bmp_brush.SetTransform(&Matrix3x2 {
            M11: w / TEX_W as f32,
            M12: 0.0,
            M21: 0.0,
            M22: h / TEX_H as f32,
            M31: rect.left,
            M32: rect.top,
        });
        if let Ok(br) = bmp_brush.cast::<ID2D1Brush>() {
            fill_card(rt, &rr, chamfer_geo, &br);
        }
    } else if let Some(bmp) = get_or_create_bitmap(rt, mat) {
        rt.DrawBitmap(
            &bmp,
            Some(&rect),
            op,
            D2D1_BITMAP_INTERPOLATION_MODE_LINEAR,
            None,
        );
    }

    // B. Optional custom color / gradient tint overlay (keeps PBR micro-texture vivid!)
    // When tint_override closely matches this material's authentic native base RGB (e.g. from a
    // matching Realistic suite palette), use a gentle harmony glaze so 90%+ of the PBR texture
    // remains crisp; when the user picks a custom color or Wallpaper Match, blend vividly.
    if let Some((c1, c2_opt)) = tint_override {
        let c2 = c2_opt.unwrap_or(c1);
        let native = mat.native_base_rgb();
        let dr = (((c1 >> 16) & 0xFF) as i32 - ((native >> 16) & 0xFF) as i32).abs();
        let dg = (((c1 >> 8) & 0xFF) as i32 - ((native >> 8) & 0xFF) as i32).abs();
        let db = ((c1 & 0xFF) as i32 - (native & 0xFF) as i32).abs();
        let delta = dr.max(dg).max(db) as f32;
        let tint_strength = if delta <= 28.0 {
            0.08
        } else {
            let t = ((delta - 28.0) / 56.0).clamp(0.0, 1.0);
            0.12 + t * 0.46
        };
        let tint_a = (tint_strength * op).clamp(0.04, 0.72);
        if let Some(tg) = make_lin_grad(
            rt,
            (rect.left, rect.top),
            (rect.right, rect.bottom),
            &[
                D2D1_GRADIENT_STOP { position: 0.0, color: rgb_u32(c1, tint_a) },
                D2D1_GRADIENT_STOP { position: 1.0, color: rgb_u32(c2, (tint_a * 0.92).min(0.76)) },
            ],
        ) {
            if let Ok(br) = tg.cast::<ID2D1Brush>() {
                fill_card(rt, &rr, chamfer_geo, &br);
            }
        }
    }

    // C. Real-time interactive material physics: only clip with a layer when shapes can cross corners
    let need_clip = mat.needs_edge_clip_layer() || chamfer_geo.is_some();
    let mut pushed_layer = false;
    let mut pushed_axis_clip = false;
    if need_clip {
        if chamfer_geo.is_none() && radius <= 2.0 {
            rt.PushAxisAlignedClip(&rect, D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
            pushed_axis_clip = true;
        } else {
            let clip_geo: Option<ID2D1Geometry> = if let Some(cg) = chamfer_geo {
                Some(cg.clone())
            } else if let Some(fac) = d2d {
                fac.CreateRoundedRectangleGeometry(&rr)
                    .ok()
                    .and_then(|g| g.cast::<ID2D1Geometry>().ok())
            } else {
                None
            };
            if let (Some(ref geo), Some(ref layer)) = (clip_geo, get_or_create_layer(rt)) {
                let params = D2D1_LAYER_PARAMETERS {
                    contentBounds: rect,
                    geometricMask: std::mem::ManuallyDrop::new(Some(geo.clone())),
                    maskAntialiasMode: D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
                    maskTransform: Matrix3x2::identity(),
                    opacity: 1.0,
                    opacityBrush: std::mem::ManuallyDrop::new(None),
                    layerOptions: D2D1_LAYER_OPTIONS_NONE,
                };
                rt.PushLayer(&params, layer);
                std::mem::ManuallyDrop::into_inner(params.geometricMask);
                std::mem::ManuallyDrop::into_inner(params.opacityBrush);
                pushed_layer = true;
            } else {
                rt.PushAxisAlignedClip(&rect, D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
                pushed_axis_clip = true;
            }
        }
    }

    draw_pbr_interactive_layer(rt, mat, rect, &rr, chamfer_geo, w, h, s, inter);

    if pushed_layer {
        rt.PopLayer();
    } else if pushed_axis_clip {
        rt.PopAxisAlignedClip();
    }

    // D. 45° 2-Pass Specular Glass / Lacquer Sheen strip (peak 18% -> 0%, gently shifts with cursor!)
    if mat.has_glass_sheen() {
        let shift = (inter.hover_x - 0.5) * w * 0.22 * inter.hover_mix;
        let peak_a = if mat.is_light_surface() {
            0.22 + 0.08 * inter.hover_mix
        } else {
            0.16 + 0.08 * inter.hover_mix
        };
        let sx0 = rect.left + w * 0.06 + shift;
        let sy0 = rect.top - h * 0.15;
        let sx1 = rect.left + w * 0.72 + shift;
        let sy1 = rect.bottom + h * 0.15;
        let sheen_stops = [
            D2D1_GRADIENT_STOP { position: 0.0, color: rgba(255, 255, 255, 0.0) },
            D2D1_GRADIENT_STOP { position: 0.34, color: rgba(255, 255, 255, peak_a * 0.42) },
            D2D1_GRADIENT_STOP { position: 0.48, color: rgba(255, 255, 255, peak_a) },
            D2D1_GRADIENT_STOP { position: 0.56, color: rgba(255, 255, 255, peak_a * 0.32) },
            D2D1_GRADIENT_STOP { position: 1.0, color: rgba(255, 255, 255, 0.0) },
        ];
        if let Some(sg) = make_lin_grad(rt, (sx0, sy0), (sx1, sy1), &sheen_stops) {
            if let Ok(br) = sg.cast::<ID2D1Brush>() {
                fill_card(rt, &rr, chamfer_geo, &br);
            }
        }
        // Pass 2: soft upper skylight reflection dome (clamped to 0.0 at bottom of dome fade)
        let dome_bot = rect.top + (h * 0.38).min(46.0 * s);
        if let Some(dg) = make_lin_grad(
            rt,
            (rect.left, rect.top),
            (rect.left, dome_bot),
            &[
                D2D1_GRADIENT_STOP { position: 0.0, color: rgba(255, 255, 255, 0.16) },
                D2D1_GRADIENT_STOP { position: 1.0, color: rgba(255, 255, 255, 0.0) },
            ],
        ) {
            if let Ok(br) = dg.cast::<ID2D1Brush>() {
                fill_card(rt, &rr, chamfer_geo, &br);
            }
        }
    }
}

/// Interactive per-material physical & optical layer (drawn inside clipped surface when needed).
unsafe fn draw_pbr_interactive_layer(
    rt: &ID2D1RenderTarget,
    mat: PbrMaterial,
    rect: D2D_RECT_F,
    rr: &D2D1_ROUNDED_RECT,
    chamfer_geo: Option<&ID2D1Geometry>,
    w: f32,
    h: f32,
    s: f32,
    inter: &PbrInteractiveInput,
) {
    let t = inter.time_s;
    let hx = rect.left + w * inter.hover_x;
    let hy = rect.top + h * inter.hover_y;
    let hm = inter.hover_mix;

    match mat {
        PbrMaterial::CalmOceanLagoon => {
            // Live caustic shimmer wavelets responding to time & cursor
            for band in 0..3 {
                let bf = band as f32;
                let cx = rect.left + w * (((0.18 + bf * 0.31 + t * 0.035 * (1.0 + bf * 0.25)) % 1.1) - 0.05);
                let cy = rect.top + h * (0.28 + bf * 0.22 + (t * 0.9 + bf * 1.9).sin() * 0.06);
                let rx = (w * (0.24 - bf * 0.03)).max(24.0 * s);
                let ry = (h * 0.22).max(12.0 * s);
                if let Some(cg) = make_rad_grad(
                    rt,
                    cx,
                    cy,
                    0.0,
                    0.0,
                    rx,
                    ry,
                    &[
                        D2D1_GRADIENT_STOP { position: 0.0, color: rgba(202, 240, 248, 0.24) },
                        D2D1_GRADIENT_STOP { position: 0.55, color: rgba(144, 224, 239, 0.09) },
                        D2D1_GRADIENT_STOP { position: 1.0, color: rgba(0, 180, 216, 0.0) },
                    ],
                ) {
                    rt.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: cx, Y: cy }, radiusX: rx, radiusY: ry }, &cg);
                }
            }

            // Rising refracted oxygen micro-bubbles gently pushed away from cursor
            let bub_n = if inter.compact_dock { 10 } else { 14 };
            if let (Ok(b_rim), Ok(b_glint)) = (
                rt.CreateSolidColorBrush(&rgba(198, 250, 255, 0.42), None),
                rt.CreateSolidColorBrush(&rgba(255, 255, 255, 0.86), None),
            ) {
                for i in 0..bub_n {
                    let fi = i as f32;
                    let mut bx = rect.left + w * (((fi * 0.61803 + (t * 0.14 + fi).sin() * 0.025) % 0.90) + 0.05);
                    let rise = (fi * 0.37 + t * (0.11 + (i % 3) as f32 * 0.035)) % 1.0;
                    let mut by = rect.bottom - h * (0.08 + 0.84 * rise);
                    if hm > 0.02 {
                        let dx = bx - hx;
                        let dy = by - hy;
                        let dist = (dx * dx + dy * dy).sqrt().max(1.0);
                        let push_r = 56.0 * s;
                        if dist < push_r {
                            let f = (1.0 - dist / push_r) * 14.0 * s * hm;
                            bx += (dx / dist) * f;
                            by += (dy / dist) * f;
                        }
                    }
                    let rad = (1.4 + (i % 4) as f32 * 0.72) * s;
                    rt.DrawEllipse(&D2D1_ELLIPSE { point: Vector2 { X: bx, Y: by }, radiusX: rad, radiusY: rad }, &b_rim, 0.85 * s, None);
                    rt.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: bx - rad * 0.32, Y: by - rad * 0.32 }, radiusX: (rad * 0.30).max(0.6), radiusY: (rad * 0.28).max(0.6) }, &b_glint);
                }
            }

            // Interactive cursor hover concentric water ripples (bright refractive crest + dark trough)
            if hm > 0.02 {
                if let Some(hg) = make_rad_grad(
                    rt,
                    hx,
                    hy,
                    0.0,
                    0.0,
                    58.0 * s,
                    32.0 * s,
                    &[
                        D2D1_GRADIENT_STOP { position: 0.0, color: rgba(202, 240, 248, 0.22 * hm) },
                        D2D1_GRADIENT_STOP { position: 1.0, color: rgba(0, 180, 216, 0.0) },
                    ],
                ) {
                    rt.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: hx, Y: hy }, radiusX: 58.0 * s, radiusY: 32.0 * s }, &hg);
                }
                for ri in 0..3 {
                    let p = ((t * 0.72 + ri as f32 * 0.333) % 1.0).clamp(0.0, 1.0);
                    let rx = (7.0 + p * 48.0) * s;
                    let ry = (3.5 + p * 22.0) * s;
                    let fade = (1.0 - p).powf(1.3) * hm;
                    if let Ok(tr_br) = rt.CreateSolidColorBrush(&rgba(4, 28, 48, 0.30 * fade), None) {
                        rt.DrawEllipse(&D2D1_ELLIPSE { point: Vector2 { X: hx, Y: hy + 1.2 * s }, radiusX: rx, radiusY: ry }, &tr_br, 1.5 * s, None);
                    }
                    if let Ok(cr_br) = rt.CreateSolidColorBrush(&rgba(214, 252, 255, 0.62 * fade), None) {
                        rt.DrawEllipse(&D2D1_ELLIPSE { point: Vector2 { X: hx, Y: hy }, radiusX: rx, radiusY: ry }, &cr_br, 1.15 * s, None);
                    }
                }
            }

            // Click / movement impulse splash rings
            for i in 0..inter.ripple_count {
                let (rx_n, ry_n, p, strength) = inter.ripples[i];
                let cx = rect.left + w * rx_n;
                let cy = rect.top + h * ry_n;
                let fade = (1.0 - p).powf(1.4) * strength.min(1.3);
                for ring in 0..2 {
                    let rp = (p - ring as f32 * 0.14).clamp(0.0, 1.0);
                    if rp <= 0.001 { continue; }
                    let rad_x = (8.0 + rp * 74.0) * s;
                    let rad_y = (4.5 + rp * 38.0) * s;
                    if let Ok(sh_br) = rt.CreateSolidColorBrush(&rgba(2, 20, 38, 0.32 * fade), None) {
                        rt.DrawEllipse(&D2D1_ELLIPSE { point: Vector2 { X: cx, Y: cy + 1.3 * s }, radiusX: rad_x, radiusY: rad_y }, &sh_br, 1.7 * s, None);
                    }
                    if let Ok(cr_br) = rt.CreateSolidColorBrush(&rgba(224, 254, 255, (0.76 * fade).min(0.9)), None) {
                        rt.DrawEllipse(&D2D1_ELLIPSE { point: Vector2 { X: cx, Y: cy }, radiusX: rad_x, radiusY: rad_y }, &cr_br, 1.3 * s, None);
                    }
                }
            }
        }

        PbrMaterial::RainyWindowpane => {
            // Interactive finger-wipe condensation clear streak following cursor!
            if hm > 0.03 {
                if let Some(wipe) = make_rad_grad(
                    rt,
                    hx,
                    hy,
                    0.0,
                    0.0,
                    36.0 * s,
                    28.0 * s,
                    &[
                        D2D1_GRADIENT_STOP { position: 0.0, color: rgba(10, 16, 22, 0.34 * hm) },
                        D2D1_GRADIENT_STOP { position: 0.65, color: rgba(20, 28, 36, 0.14 * hm) },
                        D2D1_GRADIENT_STOP { position: 1.0, color: rgba(20, 28, 36, 0.0) },
                    ],
                ) {
                    rt.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: hx, Y: hy }, radiusX: 36.0 * s, radiusY: 28.0 * s }, &wipe);
                }
            }

            // 20 optically refracting 3D convex raindrops + meandering gravity streams
            let drop_n = if inter.compact_dock { 15 } else { 22 };
            for i in 0..drop_n {
                let fi = i as f32;
                let sliding = i % 4 == 0;
                let mut dx_n = ((fi * 0.61803 + 0.07) % 0.88) + 0.06;
                let mut dy_n = if sliding {
                    let p = (t * (0.06 + (i % 3) as f32 * 0.025) + fi * 0.27) % 1.0;
                    dx_n += (p * 11.0 + fi).sin() * 0.008;
                    0.06 + p * 0.88
                } else {
                    ((fi * 0.4317 + 0.11) % 0.82) + 0.09
                };
                let mut px = rect.left + w * dx_n;
                let mut py = rect.top + h * dy_n;

                // Cursor physically pushes & parts droplets out of its wipe path!
                if hm > 0.02 {
                    let ddx = px - hx;
                    let ddy = py - hy;
                    let dist = (ddx * ddx + ddy * ddy).sqrt().max(1.0);
                    let wipe_r = 34.0 * s;
                    if dist < wipe_r {
                        let push = (1.0 - dist / wipe_r) * 16.0 * s * hm;
                        px += (ddx / dist) * push;
                        py += (ddy / dist) * push * 0.8;
                        dy_n = ((py - rect.top) / h).clamp(0.05, 0.95);
                    }
                }
                let rx = (1.6 + (i % 4) as f32 * 0.92) * s;
                let ry = rx * (if sliding { 1.32 } else { 1.12 });

                // Meandering wet condensation-cleared gravity trail above sliding drop
                if sliding && py > rect.top + 8.0 * s {
                    if let Ok(tr_br) = rt.CreateSolidColorBrush(&rgba(14, 22, 30, 0.26), None) {
                        let segs = 6;
                        for sg in 0..segs {
                            let f0 = sg as f32 / segs as f32;
                            let f1 = (sg + 1) as f32 / segs as f32;
                            let y0 = rect.top + (py - ry - rect.top) * f0;
                            let y1 = rect.top + (py - ry - rect.top) * f1;
                            let x0 = px + ((dy_n * f0 * 11.0 + fi).sin() - (dy_n * 11.0 + fi).sin()) * 2.2 * s;
                            let x1 = px + ((dy_n * f1 * 11.0 + fi).sin() - (dy_n * 11.0 + fi).sin()) * 2.2 * s;
                            rt.DrawLine(Vector2 { X: x0, Y: y0 }, Vector2 { X: x1, Y: y1 }, &tr_br, (rx * 0.55).max(0.8), None);
                        }
                    }
                }
                draw_optical_raindrop(rt, px, py, rx, ry, s);
            }

            // Click spawns fresh glistening droplet + splash ring
            for i in 0..inter.ripple_count {
                let (rx_n, ry_n, p, strg) = inter.ripples[i];
                let cx = rect.left + w * rx_n;
                let cy = rect.top + h * (ry_n + p * 0.14).min(0.95);
                let fade = (1.0 - p).clamp(0.0, 1.0) * strg.min(1.2);
                draw_optical_raindrop(rt, cx, cy, (3.6 * s) * (0.65 + 0.35 * fade), (4.5 * s) * (0.65 + 0.35 * fade), s);
                if let Ok(sr_br) = rt.CreateSolidColorBrush(&rgba(230, 236, 239, 0.42 * fade), None) {
                    rt.DrawEllipse(&D2D1_ELLIPSE { point: Vector2 { X: cx, Y: cy }, radiusX: (6.0 + p * 26.0) * s, radiusY: (6.0 + p * 26.0) * s }, &sr_br, 0.9 * s, None);
                }
            }
        }

        PbrMaterial::BrushedTitanium => {
            // Cursor-tracking 6-stop anisotropic brushed metal reflection sweep!
            let sweep_x = rect.left + w * (0.20 + 0.60 * (if hm > 0.02 { inter.hover_x } else { 0.5 + 0.25 * (t * 0.38).sin() }));
            let span = (w * 0.40).max(56.0 * s);
            let boost = 1.0 + 0.45 * hm + 0.22 * inter.audio_energy;
            let aniso = [
                D2D1_GRADIENT_STOP { position: 0.0, color: rgba(255, 255, 255, 0.0) },
                D2D1_GRADIENT_STOP { position: 0.18, color: rgba(220, 225, 227, (0.09 * boost).min(0.28)) },
                D2D1_GRADIENT_STOP { position: 0.35, color: rgba(255, 255, 255, (0.20 * boost).min(0.42)) },
                D2D1_GRADIENT_STOP { position: 0.52, color: rgba(140, 150, 162, (0.07 * boost).min(0.20)) },
                D2D1_GRADIENT_STOP { position: 0.74, color: rgba(235, 240, 246, (0.14 * boost).min(0.32)) },
                D2D1_GRADIENT_STOP { position: 1.0, color: rgba(255, 255, 255, 0.0) },
            ];
            if let Some(mg) = make_lin_grad(rt, (sweep_x - span, rect.top), (sweep_x + span, rect.bottom), &aniso) {
                if let Ok(br) = mg.cast::<ID2D1Brush>() {
                    fill_card(rt, rr, chamfer_geo, &br);
                }
            }
            // Click incandescent CNC glint pulse
            for i in 0..inter.ripple_count {
                let (rx_n, _, p, strg) = inter.ripples[i];
                let cx = rect.left + w * rx_n;
                let fade = (1.0 - p).powf(1.5) * strg.min(1.2);
                if let Some(fl) = make_rad_grad(
                    rt,
                    cx,
                    rect.top + h * 0.5,
                    0.0,
                    0.0,
                    (18.0 + p * 68.0) * s,
                    h * 0.65,
                    &[
                        D2D1_GRADIENT_STOP { position: 0.0, color: rgba(255, 167, 38, 0.28 * fade) },
                        D2D1_GRADIENT_STOP { position: 1.0, color: rgba(255, 167, 38, 0.0) },
                    ],
                ) {
                    if let Ok(br) = fl.cast::<ID2D1Brush>() {
                        fill_card(rt, rr, chamfer_geo, &br);
                    }
                }
            }
            // 4 CNC Recessed Torx Corner Bolts
            if w > 56.0 * s && h > 24.0 * s {
                let pad = if inter.compact_dock { 7.5 * s } else { 10.5 * s };
                let br = if inter.compact_dock { 2.5 * s } else { 3.4 * s };
                for &(bx, by) in &[
                    (rect.left + pad, rect.top + pad),
                    (rect.right - pad, rect.top + pad),
                    (rect.left + pad, rect.bottom - pad),
                    (rect.right - pad, rect.bottom - pad),
                ] {
                    draw_cnc_torx_bolt(rt, bx, by, br, s, false);
                }
            }
        }

        PbrMaterial::VintageWalnutBrass => {
            // Warm lacquer sheen smoothly tracking cursor across walnut grain
            let sx = rect.left + w * (0.24 + 0.52 * (if hm > 0.02 { inter.hover_x } else { 0.45 + 0.18 * (t * 0.25).sin() }));
            if let Some(lg) = make_rad_grad(
                rt,
                sx,
                rect.top + h * 0.28,
                0.0,
                0.0,
                w * 0.46,
                h * 0.65,
                &[
                    D2D1_GRADIENT_STOP { position: 0.0, color: rgba(243, 229, 171, 0.14 + 0.10 * hm) },
                    D2D1_GRADIENT_STOP { position: 1.0, color: rgba(212, 175, 55, 0.0) },
                ],
            ) {
                if let Ok(br) = lg.cast::<ID2D1Brush>() {
                    fill_card(rt, rr, chamfer_geo, &br);
                }
            }
            // Chamfered recessed antique brass hairline inlay frame
            let ins = if inter.compact_dock { 3.2 * s } else { 5.2 * s };
            if w > 40.0 * s && h > 20.0 * s {
                let inlay_rr = D2D1_ROUNDED_RECT {
                    rect: D2D_RECT_F {
                        left: rect.left + ins,
                        top: rect.top + ins,
                        right: rect.right - ins,
                        bottom: rect.bottom - ins,
                    },
                    radiusX: (8.0 * s).min(h * 0.28),
                    radiusY: (8.0 * s).min(h * 0.28),
                };
                if let Some(bg) = make_lin_grad(
                    rt,
                    (rect.left, rect.top),
                    (rect.right, rect.bottom),
                    &[
                        D2D1_GRADIENT_STOP { position: 0.0, color: rgba(243, 229, 171, 0.68) },
                        D2D1_GRADIENT_STOP { position: 0.35, color: rgba(212, 175, 55, 0.45) },
                        D2D1_GRADIENT_STOP { position: 0.70, color: rgba(138, 104, 32, 0.52) },
                        D2D1_GRADIENT_STOP { position: 1.0, color: rgba(243, 229, 171, 0.62) },
                    ],
                ) {
                    rt.DrawRoundedRectangle(&inlay_rr, &bg, 1.05 * s, None);
                }
            }
            // 4 Slotted Antique Brass Corner Screws (slot catches interactive specular gleam)
            if w > 64.0 * s && h > 28.0 * s {
                let pad = if inter.compact_dock { 8.0 * s } else { 11.5 * s };
                let sr_r = if inter.compact_dock { 2.5 * s } else { 3.5 * s };
                for (si, &(bx, by)) in [
                    (rect.left + pad, rect.top + pad),
                    (rect.right - pad, rect.top + pad),
                    (rect.left + pad, rect.bottom - pad),
                    (rect.right - pad, rect.bottom - pad),
                ]
                .iter()
                .enumerate()
                {
                    let slot_ang = 0.52 + si as f32 * 0.78 + hm * 0.15;
                    draw_slotted_brass_screw(rt, bx, by, sr_r, slot_ang, s);
                }
            }
        }

        PbrMaterial::DeepAbyssalTrench => {
            // Cursor-reactive Total Internal Reflection cyan edge bloom
            if hm > 0.02 {
                if let Some(gl) = make_rad_grad(
                    rt,
                    hx,
                    hy,
                    0.0,
                    0.0,
                    72.0 * s,
                    48.0 * s,
                    &[
                        D2D1_GRADIENT_STOP { position: 0.0, color: rgba(0, 255, 204, 0.24 * hm) },
                        D2D1_GRADIENT_STOP { position: 0.50, color: rgba(0, 114, 187, 0.14 * hm) },
                        D2D1_GRADIENT_STOP { position: 1.0, color: rgba(0, 114, 187, 0.0) },
                    ],
                ) {
                    rt.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: hx, Y: hy }, radiusX: 72.0 * s, radiusY: 48.0 * s }, &gl);
                }
            }
            // Bioluminescent dinoflagellate plankton swarm that flares & swirls around cursor!
            let p_count = if inter.compact_dock { 16 } else { 26 };
            for i in 0..p_count {
                let fi = i as f32;
                let base_x = ((fi * 0.61803 + t * 0.018 * (if i % 2 == 0 { 1.0 } else { -0.7 })) % 0.92 + 0.92) % 0.92 + 0.04;
                let base_y = ((fi * 0.3712 + (t * 0.42 + fi * 1.3).sin() * 0.06) % 0.86 + 0.86) % 0.86 + 0.07;
                let mut px = rect.left + w * base_x;
                let mut py = rect.top + h * base_y;
                let mut excite = 0.26 + 0.16 * (t * 1.4 + fi * 2.1).sin().abs();
                if hm > 0.02 {
                    let dx = px - hx;
                    let dy = py - hy;
                    let d = (dx * dx + dy * dy).sqrt().max(1.0);
                    let r_inf = 68.0 * s;
                    if d < r_inf {
                        let k = (1.0 - d / r_inf) * hm;
                        // Gentle tangential swirl + luminous ignition!
                        px += (-dy / d * 8.0 + dx / d * 5.0) * s * k;
                        py += (dx / d * 8.0 + dy / d * 5.0) * s * k;
                        excite = (excite + k * 0.92).min(1.0);
                    }
                }
                let pr = (1.1 + (i % 3) as f32 * 0.65 + excite * 0.9) * s;
                if let Ok(gb) = rt.CreateSolidColorBrush(&rgba(0, 255, 204, (0.20 * excite).min(0.45)), None) {
                    rt.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: px, Y: py }, radiusX: pr * 3.0, radiusY: pr * 3.0 }, &gb);
                }
                if let Ok(cb) = rt.CreateSolidColorBrush(&rgba(168, 255, 240, (0.88 * excite).min(0.98)), None) {
                    rt.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: px, Y: py }, radiusX: pr, radiusY: pr }, &cb);
                }
            }
            // Click abyssal sonar pulse ring
            for i in 0..inter.ripple_count {
                let (rx_n, ry_n, p, strg) = inter.ripples[i];
                let cx = rect.left + w * rx_n;
                let cy = rect.top + h * ry_n;
                let fade = (1.0 - p).powf(1.2) * strg.min(1.3);
                let rad = (8.0 + p * 82.0) * s;
                if let Ok(sb) = rt.CreateSolidColorBrush(&rgba(0, 255, 204, 0.72 * fade), None) {
                    rt.DrawEllipse(&D2D1_ELLIPSE { point: Vector2 { X: cx, Y: cy }, radiusX: rad, radiusY: rad * 0.72 }, &sb, 1.4 * s, None);
                }
            }
        }

        PbrMaterial::NordicGlacialIce => {
            // 45° Chromatic Prism Edge Dispersion (cyan + magenta spectral fringe along top-left bevel)
            let prism_len = (w * 0.68).min(w - 16.0 * s);
            if let Ok(cy_br) = rt.CreateSolidColorBrush(&rgba(0, 240, 255, 0.34 + 0.22 * hm), None) {
                rt.DrawLine(
                    Vector2 { X: rect.left + 8.0 * s, Y: rect.top + 2.2 * s },
                    Vector2 { X: rect.left + 8.0 * s + prism_len, Y: rect.top + 2.2 * s },
                    &cy_br,
                    1.3 * s,
                    None,
                );
            }
            if let Ok(mg_br) = rt.CreateSolidColorBrush(&rgba(255, 118, 232, 0.22 + 0.16 * hm), None) {
                rt.DrawLine(
                    Vector2 { X: rect.left + 12.0 * s, Y: rect.top + 3.8 * s },
                    Vector2 { X: rect.left + 8.0 * s + prism_len * 0.72, Y: rect.top + 3.8 * s },
                    &mg_br,
                    1.0 * s,
                    None,
                );
            }
            // Interactive crystalline prism flare & frost sparkle under cursor
            let fx = if hm > 0.02 { hx } else { rect.left + w * (0.30 + 0.40 * (t * 0.35).sin().abs()) };
            let fy = if hm > 0.02 { hy } else { rect.top + h * 0.35 };
            if let Some(pg) = make_rad_grad(
                rt,
                fx,
                fy,
                0.0,
                0.0,
                54.0 * s,
                36.0 * s,
                &[
                    D2D1_GRADIENT_STOP { position: 0.0, color: rgba(224, 244, 255, 0.26 + 0.22 * hm) },
                    D2D1_GRADIENT_STOP { position: 0.45, color: rgba(160, 231, 229, 0.12 + 0.12 * hm) },
                    D2D1_GRADIENT_STOP { position: 1.0, color: rgba(160, 231, 229, 0.0) },
                ],
            ) {
                rt.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: fx, Y: fy }, radiusX: 54.0 * s, radiusY: 36.0 * s }, &pg);
            }
            // Crisp diamond ice facet glints sparkling near cursor
            if let Ok(sp_br) = rt.CreateSolidColorBrush(&rgba(244, 252, 255, 0.76), None) {
                for i in 0..8 {
                    let fi = i as f32;
                    let sx = rect.left + w * (((fi * 0.618 + 0.11) % 0.84) + 0.08);
                    let sy = rect.top + h * (((fi * 0.419 + 0.15) % 0.70) + 0.15);
                    let dist = ((sx - fx).powi(2) + (sy - fy).powi(2)).sqrt();
                    let near = (1.0 - (dist / (78.0 * s)).clamp(0.0, 1.0)).powf(1.5);
                    let twinkle = (0.35 + 0.65 * (t * 2.4 + fi * 1.9).sin().abs()) * (0.28 + 0.72 * near);
                    let len = (2.8 + 4.2 * twinkle) * s;
                    if twinkle > 0.22 {
                        rt.DrawLine(Vector2 { X: sx - len, Y: sy }, Vector2 { X: sx + len, Y: sy }, &sp_br, 0.85 * s, None);
                        rt.DrawLine(Vector2 { X: sx, Y: sy - len }, Vector2 { X: sx, Y: sy + len }, &sp_br, 0.85 * s, None);
                    }
                }
            }
            for i in 0..inter.ripple_count {
                let (rx_n, ry_n, p, strg) = inter.ripples[i];
                let cx = rect.left + w * rx_n;
                let cy = rect.top + h * ry_n;
                let fade = (1.0 - p).powf(1.3) * strg.min(1.2);
                if let Ok(cb) = rt.CreateSolidColorBrush(&rgba(224, 244, 255, 0.74 * fade), None) {
                    let rad = (5.0 + p * 34.0) * s;
                    for a_i in 0..6 {
                        let ang = a_i as f32 * (PI / 3.0) + p * 0.4;
                        rt.DrawLine(
                            Vector2 { X: cx + ang.cos() * rad * 0.3, Y: cy + ang.sin() * rad * 0.3 },
                            Vector2 { X: cx + ang.cos() * rad, Y: cy + ang.sin() * rad },
                            &cb,
                            1.0 * s,
                            None,
                        );
                    }
                }
            }
        }

        PbrMaterial::ExecutiveLeatherLinen => {
            // Warm interactive directional leather patina sheen tracking cursor
            if hm > 0.02 {
                if let Some(pat) = make_rad_grad(
                    rt,
                    hx,
                    hy,
                    0.0,
                    0.0,
                    w * 0.42,
                    h * 0.60,
                    &[
                        D2D1_GRADIENT_STOP { position: 0.0, color: rgba(229, 184, 105, 0.14 * hm) },
                        D2D1_GRADIENT_STOP { position: 1.0, color: rgba(229, 184, 105, 0.0) },
                    ],
                ) {
                    if let Ok(br) = pat.cast::<ID2D1Brush>() {
                        fill_card(rt, rr, chamfer_geo, &br);
                    }
                }
            }
            // Depressed perimeter stitching channel + realistic angled waxed linen stitches with needle-hole shadows
            let ins = if inter.compact_dock { 4.2 * s } else { 6.2 * s };
            if w > 40.0 * s && h > 18.0 * s {
                if let (Ok(hole_br), Ok(thread_br)) = (
                    rt.CreateSolidColorBrush(&rgba(10, 4, 2, 0.65), None),
                    rt.CreateSolidColorBrush(&rgba(247, 244, 235, 0.80), None),
                ) {
                    let step = 8.2 * s;
                    let slen = 4.4 * s;
                    let mut sx = rect.left + ins + 8.0 * s;
                    while sx < rect.right - ins - 10.0 * s {
                        // Top & bottom stitches
                        for &sy in &[rect.top + ins, rect.bottom - ins] {
                            rt.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: sx, Y: sy + 0.5 * s }, radiusX: 0.85 * s, radiusY: 0.85 * s }, &hole_br);
                            rt.DrawLine(Vector2 { X: sx, Y: sy + 0.4 * s }, Vector2 { X: sx + slen, Y: sy - 0.5 * s }, &thread_br, 1.05 * s, None);
                        }
                        sx += step;
                    }
                    let mut sy = rect.top + ins + 8.0 * s;
                    while sy < rect.bottom - ins - 10.0 * s {
                        for &sx_e in &[rect.left + ins, rect.right - ins] {
                            rt.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: sx_e + 0.4 * s, Y: sy }, radiusX: 0.85 * s, radiusY: 0.85 * s }, &hole_br);
                            rt.DrawLine(Vector2 { X: sx_e - 0.4 * s, Y: sy }, Vector2 { X: sx_e + 0.4 * s, Y: sy + slen }, &thread_br, 1.05 * s, None);
                        }
                        sy += step;
                    }
                }
            }
            // 4 Antique Hammered Brass Corner Rivets
            if w > 60.0 * s && h > 24.0 * s {
                let pad = if inter.compact_dock { 8.0 * s } else { 12.0 * s };
                let rr = if inter.compact_dock { 2.5 * s } else { 3.3 * s };
                for &(bx, by) in &[
                    (rect.left + pad, rect.top + pad),
                    (rect.right - pad, rect.top + pad),
                    (rect.left + pad, rect.bottom - pad),
                    (rect.right - pad, rect.bottom - pad),
                ] {
                    draw_brass_rivet(rt, bx, by, rr, s);
                }
            }
        }

        PbrMaterial::AlpineForestMist => {
            // Volumetric 135° diagonal morning sunbeams streaming across mossy canopy
            for ray_i in 0..4 {
                let rf = ray_i as f32;
                let rx0 = rect.left + w * (0.04 + rf * 0.24) + (inter.hover_x - 0.5) * 10.0 * s * hm;
                let ray_a = 0.055 + 0.03 * (t * 0.6 + rf * 1.4).sin().abs() + 0.03 * hm;
                if let Some(rg) = make_lin_grad(
                    rt,
                    (rx0, rect.top),
                    (rx0 + h * 0.55, rect.bottom),
                    &[
                        D2D1_GRADIENT_STOP { position: 0.0, color: rgba(218, 215, 205, ray_a * 1.4) },
                        D2D1_GRADIENT_STOP { position: 0.65, color: rgba(163, 177, 138, ray_a * 0.6) },
                        D2D1_GRADIENT_STOP { position: 1.0, color: rgba(163, 177, 138, 0.0) },
                    ],
                ) {
                    rt.DrawLine(
                        Vector2 { X: rx0, Y: rect.top - 4.0 * s },
                        Vector2 { X: rx0 + h * 0.52, Y: rect.bottom + 4.0 * s },
                        &rg,
                        (18.0 + rf * 4.0) * s,
                        None,
                    );
                }
            }
            // Drifting mist banks that gently part away from cursor
            for m in 0..3 {
                let mf = m as f32;
                let mut mx = rect.left + w * (((0.18 + mf * 0.34 + t * 0.014 * (1.0 + mf * 0.25)) % 1.2) - 0.1);
                let my = rect.top + h * (0.36 + mf * 0.22);
                if hm > 0.02 {
                    let dx = mx - hx;
                    let push = if dx >= 0.0 { 16.0 } else { -16.0 };
                    mx += push * s * hm * (1.0 - (dx.abs() / w).clamp(0.0, 1.0));
                }
                if let Some(mg) = make_rad_grad(
                    rt,
                    mx,
                    my,
                    0.0,
                    0.0,
                    w * 0.30,
                    h * 0.42,
                    &[
                        D2D1_GRADIENT_STOP { position: 0.0, color: rgba(218, 215, 205, 0.13) },
                        D2D1_GRADIENT_STOP { position: 1.0, color: rgba(163, 177, 138, 0.0) },
                    ],
                ) {
                    rt.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: mx, Y: my }, radiusX: w * 0.30, radiusY: h * 0.42 }, &mg);
                }
            }
            // Floating sunlit golden pollen motes swirling gently in cursor breeze
            if let Ok(pol_br) = rt.CreateSolidColorBrush(&rgba(242, 232, 184, 0.62), None) {
                for i in 0..10 {
                    let fi = i as f32;
                    let mut px = rect.left + w * (((fi * 0.618 + t * 0.022 + (t * 0.6 + fi).sin() * 0.03) % 0.90) + 0.05);
                    let mut py = rect.top + h * (((fi * 0.41 + (t * 0.45 + fi * 1.7).cos() * 0.06) % 0.80) + 0.10);
                    if hm > 0.02 {
                        px += inter.hover_vx * 4.5 * s * hm;
                        py += inter.hover_vy * 4.5 * s * hm;
                    }
                    let pr = (0.95 + (i % 3) as f32 * 0.45) * s;
                    rt.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: px, Y: py }, radiusX: pr, radiusY: pr }, &pol_br);
                }
            }
        }

        PbrMaterial::IndustrialCarbonSteel => {
            // Anisotropic diagonal carbon weave reflection sweep tracking cursor
            let sx = rect.left + w * (0.18 + 0.64 * (if hm > 0.02 { inter.hover_x } else { 0.48 + 0.22 * (t * 0.32).sin() }));
            let span = (w * 0.34).max(46.0 * s);
            if let Some(cg) = make_lin_grad(
                rt,
                (sx - span, rect.top),
                (sx + span, rect.bottom),
                &[
                    D2D1_GRADIENT_STOP { position: 0.0, color: rgba(255, 255, 255, 0.0) },
                    D2D1_GRADIENT_STOP { position: 0.48, color: rgba(212, 224, 240, 0.14 + 0.14 * hm) },
                    D2D1_GRADIENT_STOP { position: 1.0, color: rgba(255, 255, 255, 0.0) },
                ],
            ) {
                if let Ok(br) = cg.cast::<ID2D1Brush>() {
                    fill_card(rt, rr, chamfer_geo, &br);
                }
            }
            // High-visibility safety orange (#FF6B35) precision telemetry micro-strip along top/left
            let strip_w = (w * (0.14 + 0.08 * hm)).clamp(14.0 * s, 54.0 * s);
            if let Ok(ob) = rt.CreateSolidColorBrush(&rgba(255, 107, 53, 0.85 + 0.15 * hm), None) {
                rt.FillRoundedRectangle(
                    &D2D1_ROUNDED_RECT {
                        rect: D2D_RECT_F {
                            left: rect.left + 16.0 * s,
                            top: rect.top + 1.5 * s,
                            right: rect.left + 16.0 * s + strip_w,
                            bottom: rect.top + 3.6 * s,
                        },
                        radiusX: 1.0 * s,
                        radiusY: 1.0 * s,
                    },
                    &ob,
                );
            }
            // 4 Recessed 304 Stainless Hex Socket Bolts
            if w > 56.0 * s && h > 24.0 * s {
                let pad = if inter.compact_dock { 7.5 * s } else { 10.5 * s };
                let br = if inter.compact_dock { 2.5 * s } else { 3.3 * s };
                for &(bx, by) in &[
                    (rect.left + pad, rect.top + pad),
                    (rect.right - pad, rect.top + pad),
                    (rect.left + pad, rect.bottom - pad),
                    (rect.right - pad, rect.bottom - pad),
                ] {
                    draw_cnc_torx_bolt(rt, bx, by, br, s, true);
                }
            }
        }

        PbrMaterial::GoldenHourHorizon => {
            // Live shimmering golden sun-track reflections shifting horizontally with cursor parallax!
            let horizon_y = rect.top + h * 0.54;
            let sun_x = rect.left + w * (0.50 + (inter.hover_x - 0.5) * 0.34 * hm + (t * 0.22).sin() * 0.04);
            // Glowing solar horizon halo
            if let Some(sg) = make_rad_grad(
                rt,
                sun_x,
                horizon_y,
                0.0,
                0.0,
                w * 0.36,
                h * 0.38,
                &[
                    D2D1_GRADIENT_STOP { position: 0.0, color: rgba(255, 227, 216, 0.34 + 0.16 * hm) },
                    D2D1_GRADIENT_STOP { position: 0.45, color: rgba(255, 158, 0, 0.18 + 0.10 * hm) },
                    D2D1_GRADIENT_STOP { position: 1.0, color: rgba(255, 158, 0, 0.0) },
                ],
            ) {
                rt.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: sun_x, Y: horizon_y }, radiusX: w * 0.36, radiusY: h * 0.38 }, &sg);
            }
            // Shimmering sea-surface solar glitter lines
            let rows = if inter.compact_dock { 6 } else { 10 };
            for row in 0..rows {
                let rf = row as f32 / rows as f32;
                let gy = horizon_y + 2.5 * s + rf * (rect.bottom - horizon_y - 5.0 * s);
                let spread = (14.0 + rf * 44.0) * s;
                let shimmer = 0.45 + 0.55 * (t * 2.8 + row as f32 * 1.7).sin().abs();
                let gx = sun_x + (t * 1.3 + row as f32 * 2.1).sin() * spread * 0.35;
                let len = (10.0 + (1.0 - rf) * 18.0) * s * (0.65 + 0.35 * shimmer);
                if let Ok(gb) = rt.CreateSolidColorBrush(&rgba(255, 209, 102, (0.56 - rf * 0.28) * shimmer), None) {
                    rt.DrawLine(Vector2 { X: gx - len * 0.5, Y: gy }, Vector2 { X: gx + len * 0.5, Y: gy }, &gb, (1.35 - rf * 0.5).max(0.6) * s, None);
                }
            }
            // Click golden horizon ripple burst
            for i in 0..inter.ripple_count {
                let (rx_n, ry_n, p, strg) = inter.ripples[i];
                let cx = rect.left + w * rx_n;
                let cy = rect.top + h * ry_n;
                let fade = (1.0 - p).powf(1.3) * strg.min(1.2);
                if let Ok(gb) = rt.CreateSolidColorBrush(&rgba(255, 209, 102, 0.65 * fade), None) {
                    rt.DrawEllipse(&D2D1_ELLIPSE { point: Vector2 { X: cx, Y: cy }, radiusX: (8.0 + p * 60.0) * s, radiusY: (4.0 + p * 26.0) * s }, &gb, 1.2 * s, None);
                }
            }
        }

        PbrMaterial::CarraraMarbleGold => {
            // Glinting metallic kintsugi gold sweep following cursor across polished marble
            let sx = rect.left + w * (0.22 + 0.56 * (if hm > 0.02 { inter.hover_x } else { 0.5 + 0.25 * (t * 0.3).sin() }));
            if let Some(kg) = make_rad_grad(
                rt,
                sx,
                rect.top + h * 0.45,
                0.0,
                0.0,
                w * 0.34,
                h * 0.52,
                &[
                    D2D1_GRADIENT_STOP { position: 0.0, color: rgba(255, 232, 162, 0.24 + 0.14 * hm) },
                    D2D1_GRADIENT_STOP { position: 1.0, color: rgba(212, 175, 55, 0.0) },
                ],
            ) {
                if let Ok(br) = kg.cast::<ID2D1Brush>() {
                    fill_card(rt, rr, chamfer_geo, &br);
                }
            }
        }

        PbrMaterial::WovenLinenCalm => {
            // Warm soft lamp falloff tracking cursor over woven linen + delicate stitched dusk hem
            if hm > 0.02 {
                if let Some(lg) = make_rad_grad(
                    rt,
                    hx,
                    hy,
                    0.0,
                    0.0,
                    w * 0.45,
                    h * 0.65,
                    &[
                        D2D1_GRADIENT_STOP { position: 0.0, color: rgba(255, 244, 224, 0.24 * hm) },
                        D2D1_GRADIENT_STOP { position: 1.0, color: rgba(255, 244, 224, 0.0) },
                    ],
                ) {
                    if let Ok(br) = lg.cast::<ID2D1Brush>() {
                        fill_card(rt, rr, chamfer_geo, &br);
                    }
                }
            }
        }
    }
}

/// Draw a single optically realistic 3D convex water drop (contact shadow, dark top meniscus,
/// magnified bottom caustic crescent, crisp 135° specular window catchlight).
unsafe fn draw_optical_raindrop(
    rt: &ID2D1RenderTarget,
    px: f32,
    py: f32,
    rx: f32,
    ry: f32,
    s: f32,
) {
    // 1. Offset contact shadow on glass below-right
    if let Ok(sh) = rt.CreateSolidColorBrush(&rgba(4, 8, 14, 0.48), None) {
        rt.FillEllipse(
            &D2D1_ELLIPSE {
                point: Vector2 { X: px + 0.8 * s, Y: py + 1.5 * s },
                radiusX: rx * 1.05,
                radiusY: ry * 1.04,
            },
            &sh,
        );
    }
    // 2. Convex refracting body (dark upper sky -> translucent mid -> bright caustic bottom)
    if let Some(body) = make_lin_grad(
        rt,
        (px, py - ry),
        (px, py + ry),
        &[
            D2D1_GRADIENT_STOP { position: 0.0, color: rgba(14, 24, 34, 0.56) },
            D2D1_GRADIENT_STOP { position: 0.52, color: rgba(138, 158, 167, 0.24) },
            D2D1_GRADIENT_STOP { position: 0.88, color: rgba(230, 236, 239, 0.72) },
            D2D1_GRADIENT_STOP { position: 1.0, color: rgba(255, 255, 255, 0.42) },
        ],
    ) {
        rt.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: px, Y: py }, radiusX: rx, radiusY: ry }, &body);
    }
    // 3. Sharp 135° upper-left window catchlight
    if let Ok(hi) = rt.CreateSolidColorBrush(&rgba(255, 255, 255, 0.92), None) {
        rt.FillEllipse(
            &D2D1_ELLIPSE {
                point: Vector2 { X: px - rx * 0.34, Y: py - ry * 0.34 },
                radiusX: (rx * 0.27).max(0.65),
                radiusY: (ry * 0.22).max(0.55),
            },
            &hi,
        );
    }
}

/// Precision recessed CNC Torx / Allen Hex bolt with countersink shadow & metallic dome.
unsafe fn draw_cnc_torx_bolt(
    rt: &ID2D1RenderTarget,
    bx: f32,
    by: f32,
    r: f32,
    s: f32,
    hex_socket: bool,
) {
    // Countersink pocket shadow
    if let Ok(cs) = rt.CreateSolidColorBrush(&rgba(8, 10, 14, 0.82), None) {
        rt.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: bx, Y: by }, radiusX: r * 1.24, radiusY: r * 1.24 }, &cs);
    }
    // Brushed metallic fastener head lit from 135°
    if let Some(hd) = make_rad_grad(
        rt,
        bx,
        by,
        -r * 0.32,
        -r * 0.32,
        r,
        r,
        &[
            D2D1_GRADIENT_STOP { position: 0.0, color: rgba(238, 244, 250, 0.92) },
            D2D1_GRADIENT_STOP { position: 0.55, color: rgba(126, 136, 148, 0.95) },
            D2D1_GRADIENT_STOP { position: 1.0, color: rgba(34, 38, 46, 0.98) },
        ],
    ) {
        rt.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: bx, Y: by }, radiusX: r, radiusY: r }, &hd);
    }
    // Dark recessed drive socket (hex/star)
    if let Ok(drv) = rt.CreateSolidColorBrush(&rgba(16, 18, 24, 0.88), None) {
        if hex_socket {
            rt.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: bx, Y: by }, radiusX: r * 0.48, radiusY: r * 0.48 }, &drv);
        } else {
            for k in 0..3 {
                let ang = k as f32 * (PI / 3.0);
                let l = r * 0.56;
                rt.DrawLine(
                    Vector2 { X: bx - ang.cos() * l, Y: by - ang.sin() * l },
                    Vector2 { X: bx + ang.cos() * l, Y: by + ang.sin() * l },
                    &drv,
                    0.75 * s,
                    None,
                );
            }
        }
    }
}

/// Chamfered antique brass slotted screw with metallic radial finish & machined screwdriver slot.
unsafe fn draw_slotted_brass_screw(
    rt: &ID2D1RenderTarget,
    bx: f32,
    by: f32,
    r: f32,
    slot_ang: f32,
    s: f32,
) {
    if let Ok(sh) = rt.CreateSolidColorBrush(&rgba(12, 6, 2, 0.78), None) {
        rt.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: bx + 0.5 * s, Y: by + 0.7 * s }, radiusX: r * 1.18, radiusY: r * 1.18 }, &sh);
    }
    if let Some(bg) = make_rad_grad(
        rt,
        bx,
        by,
        -r * 0.35,
        -r * 0.35,
        r,
        r,
        &[
            D2D1_GRADIENT_STOP { position: 0.0, color: rgba(248, 236, 186, 0.98) },
            D2D1_GRADIENT_STOP { position: 0.52, color: rgba(212, 175, 55, 0.96) },
            D2D1_GRADIENT_STOP { position: 1.0, color: rgba(98, 70, 20, 0.98) },
        ],
    ) {
        rt.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: bx, Y: by }, radiusX: r, radiusY: r }, &bg);
    }
    let dx = slot_ang.cos() * r * 0.72;
    let dy = slot_ang.sin() * r * 0.72;
    if let Ok(sl) = rt.CreateSolidColorBrush(&rgba(42, 26, 8, 0.88), None) {
        rt.DrawLine(Vector2 { X: bx - dx, Y: by - dy }, Vector2 { X: bx + dx, Y: by + dy }, &sl, 0.9 * s, None);
    }
    if let Ok(sh) = rt.CreateSolidColorBrush(&rgba(255, 246, 208, 0.55), None) {
        rt.DrawLine(Vector2 { X: bx - dx, Y: by - dy + 0.65 * s }, Vector2 { X: bx + dx, Y: by + dy + 0.65 * s }, &sh, 0.5 * s, None);
    }
}

/// Hammered antique brass rivet dome for full-grain leathercraft.
unsafe fn draw_brass_rivet(rt: &ID2D1RenderTarget, bx: f32, by: f32, r: f32, s: f32) {
    if let Ok(sh) = rt.CreateSolidColorBrush(&rgba(8, 3, 1, 0.72), None) {
        rt.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: bx + 0.6 * s, Y: by + 0.8 * s }, radiusX: r * 1.15, radiusY: r * 1.15 }, &sh);
    }
    if let Some(rg) = make_rad_grad(
        rt,
        bx,
        by,
        -r * 0.35,
        -r * 0.35,
        r,
        r,
        &[
            D2D1_GRADIENT_STOP { position: 0.0, color: rgba(255, 236, 178, 0.98) },
            D2D1_GRADIENT_STOP { position: 0.55, color: rgba(214, 166, 82, 0.96) },
            D2D1_GRADIENT_STOP { position: 1.0, color: rgba(94, 58, 18, 0.98) },
        ],
    ) {
        rt.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: bx, Y: by }, radiusX: r, radiusY: r }, &rg);
    }
}

// ---------------------------------------------------------------------------
// 3. 135° Incident Light Model Bevel Rim
//    Top & Left: Crisp 1.0–1.5px specular highlight (white 0.45..0.75)
//    Bottom & Right: Deep contact shadow / AO rim (black 0.55..0.85)
//    + Pen Canvas custom border_rgb override support!
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
pub unsafe fn draw_pbr_bevel_rim(
    rt: &ID2D1RenderTarget,
    rect: D2D_RECT_F,
    radius: f32,
    s: f32,
    mat: PbrMaterial,
    custom_border_rgb: Option<u32>,
    rim_alpha: f32,
    hover_mix: f32,
    chamfer_geo: Option<&ID2D1Geometry>,
) {
    let rr = D2D1_ROUNDED_RECT {
        rect,
        radiusX: radius,
        radiusY: radius,
    };

    if let Some(brgb) = custom_border_rgb {
        let ra = (rim_alpha.max(0.72) + hover_mix * 0.16).clamp(0.45, 1.0);
        if let Some(cg) = make_lin_grad(
            rt,
            (rect.left, rect.top),
            (rect.right, rect.bottom),
            &[
                D2D1_GRADIENT_STOP { position: 0.0, color: rgb_u32(brgb, ra) },
                D2D1_GRADIENT_STOP { position: 0.52, color: rgb_u32(brgb, ra * 0.62) },
                D2D1_GRADIENT_STOP { position: 1.0, color: rgb_u32(brgb, ra * 0.85) },
            ],
        ) {
            if let Some(geo) = chamfer_geo {
                rt.DrawGeometry(geo, &cg, 1.55 * s, None);
            } else {
                rt.DrawRoundedRectangle(&rr, &cg, 1.55 * s, None);
            }
        }
        return;
    }

    if rim_alpha <= 0.01 {
        return;
    }

    let ar = mat.accent_rim_rgb();
    let hi_a = (0.52 + 0.22 * hover_mix).clamp(0.45, 0.78) * rim_alpha.max(0.65);
    let ao_a = (0.62 + 0.16 * (1.0 - hover_mix)).clamp(0.55, 0.84) * rim_alpha.max(0.65);

    // Outer 135° physical bevel gradient: bright top-left specular -> material mid -> dark bottom-right AO
    let bevel_stops = [
        D2D1_GRADIENT_STOP { position: 0.0, color: rgba(255, 255, 255, hi_a) },
        D2D1_GRADIENT_STOP { position: 0.34, color: rgba(ar.0, ar.1, ar.2, (0.58 + 0.20 * hover_mix) * rim_alpha.max(0.65)) },
        D2D1_GRADIENT_STOP { position: 0.68, color: rgba(ar.0 / 3, ar.1 / 3, ar.2 / 3, 0.42 * rim_alpha.max(0.65)) },
        D2D1_GRADIENT_STOP { position: 1.0, color: rgba(0, 0, 0, ao_a) },
    ];
    if let Some(bg) = make_lin_grad(rt, (rect.left, rect.top), (rect.right, rect.bottom), &bevel_stops) {
        if let Some(geo) = chamfer_geo {
            rt.DrawGeometry(geo, &bg, 1.45 * s, None);
        } else {
            rt.DrawRoundedRectangle(&rr, &bg, 1.45 * s, None);
        }
    }

    // Inner recessed 1px specular hairline along top-left & dark shadow along bottom-right
    let inset = 1.25 * s;
    let in_rr = D2D1_ROUNDED_RECT {
        rect: D2D_RECT_F {
            left: rect.left + inset,
            top: rect.top + inset,
            right: rect.right - inset,
            bottom: rect.bottom - inset,
        },
        radiusX: (radius - inset).max(1.0),
        radiusY: (radius - inset).max(1.0),
    };
    let in_stops = [
        D2D1_GRADIENT_STOP { position: 0.0, color: rgba(255, 255, 255, (0.34 + 0.14 * hover_mix).min(0.55)) },
        D2D1_GRADIENT_STOP { position: 0.48, color: rgba(255, 255, 255, 0.0) },
        D2D1_GRADIENT_STOP { position: 1.0, color: rgba(0, 0, 0, 0.32) },
    ];
    if chamfer_geo.is_none() {
        if let Some(ig) = make_lin_grad(rt, (in_rr.rect.left, in_rr.rect.top), (in_rr.rect.right, in_rr.rect.bottom), &in_stops) {
            rt.DrawRoundedRectangle(&in_rr, &ig, 0.9 * s, None);
        }
    }
}

/// Ivory woven-linen paper card with realistic curled bottom-right corner shadow & brass pin
/// (used by Executive Leather & Linen widgets for tactile layered contrast).
pub unsafe fn draw_linen_paper_inset(
    rt: &ID2D1RenderTarget,
    d2d: Option<&ID2D1Factory>,
    rect: D2D_RECT_F,
    s: f32,
    hover_mix: f32,
) {
    let rad = 5.0 * s;
    let lift = hover_mix * 1.4 * s;
    // Curled paper corner drop shadow (lifts subtly on hover!)
    let sh_rr = D2D1_ROUNDED_RECT {
        rect: D2D_RECT_F {
            left: rect.left + 1.5 * s,
            top: rect.top + 2.2 * s + lift * 0.4,
            right: rect.right + 2.8 * s + lift,
            bottom: rect.bottom + 3.4 * s + lift,
        },
        radiusX: rad,
        radiusY: rad,
    };
    if let Ok(sh) = rt.CreateSolidColorBrush(&rgba(10, 4, 2, 0.44 + 0.10 * hover_mix), None) {
        rt.FillRoundedRectangle(&sh_rr, &sh);
    }
    // Woven ivory linen texture clipped inside paper sheet
    let sheet_r = D2D_RECT_F {
        left: rect.left,
        top: rect.top - lift * 0.35,
        right: rect.right,
        bottom: rect.bottom - lift * 0.35,
    };
    let inter = PbrInteractiveInput {
        hover_mix,
        ..Default::default()
    };
    draw_pbr_surface_clipped(
        rt,
        d2d,
        PbrMaterial::WovenLinenCalm,
        sheet_r,
        rad,
        s,
        0.99,
        Some((0xF7F4EB, Some(0xEDE6D6))),
        None,
        &inter,
    );
    // Warm sunlit ivory top-left paper bevel & graphite edge
    let rr = D2D1_ROUNDED_RECT { rect: sheet_r, radiusX: rad, radiusY: rad };
    if let Ok(pb) = rt.CreateSolidColorBrush(&rgba(255, 255, 255, 0.72), None) {
        rt.DrawRoundedRectangle(&rr, &pb, 0.9 * s, None);
    }
    // Brass push-pin cleanly tucked in top-right corner of linen sheet (never overlaps title)
    draw_brass_rivet(rt, sheet_r.right - 9.0 * s, sheet_r.top + 8.5 * s, 2.7 * s, s);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verify_all_12_pbr_materials_bake_non_flat_textures() {
        for mat in PbrMaterial::ALL {
            let px = material_pixels(mat);
            assert_eq!(px.len(), (TEX_W * TEX_H * 4) as usize);
            let mut min_luma = 255u8;
            let mut max_luma = 0u8;
            for chunk in px.chunks_exact(4).step_by(7) {
                let l = ((chunk[2] as u16 * 77 + chunk[1] as u16 * 150 + chunk[0] as u16 * 29) >> 8) as u8;
                min_luma = min_luma.min(l);
                max_luma = max_luma.max(l);
            }
            assert!(
                max_luma.saturating_sub(min_luma) >= 18,
                "PBR material {:?} texture must have rich luminance variation (got {}..{})",
                mat,
                min_luma,
                max_luma
            );
        }
    }
}
