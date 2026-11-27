use windows::core::{w, Interface};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D1_FIGURE_BEGIN_HOLLOW, D2D1_FIGURE_END_OPEN, D2D1_GRADIENT_STOP, D2D_RECT_F, D2D_SIZE_F,
};
use windows::Win32::Graphics::Direct2D::{
    ID2D1Bitmap, ID2D1Brush, ID2D1RenderTarget, ID2D1Factory, ID2D1Geometry, ID2D1Layer,
    ID2D1StrokeStyle, D2D1_ANTIALIAS_MODE_PER_PRIMITIVE, D2D1_ARC_SEGMENT, D2D1_ARC_SIZE_LARGE,
    D2D1_ARC_SIZE_SMALL, D2D1_BITMAP_INTERPOLATION_MODE_LINEAR, D2D1_CAP_STYLE_ROUND, D2D1_ELLIPSE,
    D2D1_LAYER_OPTIONS_NONE, D2D1_LAYER_PARAMETERS, D2D1_LINE_JOIN_ROUND, D2D1_ROUNDED_RECT,
    D2D1_STROKE_STYLE_PROPERTIES, D2D1_SWEEP_DIRECTION_CLOCKWISE,
};
use windows::Win32::Graphics::DirectWrite::{
    IDWriteFactory, IDWriteFontCollection, DWRITE_FONT_WEIGHT_BOLD, DWRITE_FONT_WEIGHT_MEDIUM,
    DWRITE_FONT_WEIGHT_REGULAR, DWRITE_FONT_WEIGHT_SEMI_BOLD, DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
    DWRITE_PARAGRAPH_ALIGNMENT_NEAR, DWRITE_TEXT_ALIGNMENT_CENTER, DWRITE_TEXT_ALIGNMENT_LEADING,
    DWRITE_TEXT_ALIGNMENT_TRAILING, DWRITE_WORD_WRAPPING_NO_WRAP,
};
use windows::Win32::Graphics::Imaging::{
    GUID_WICPixelFormat32bppPBGRA, IWICImagingFactory, WICBitmapDitherTypeNone,
    WICBitmapPaletteTypeCustom, WICDecodeMetadataCacheOnDemand,
};
use windows::Win32::UI::Shell::SHCreateMemStream;
use windows_numerics::Vector2;

use crate::config::{AppConfig, ClockElemTransform, MusicStyle, TaskbarVizStyle};
use crate::desktop_widgets::{
    begin_elem_xf_slot, create_linear_grad, current_active_elem_tr, draw_shadowed_text,
    elem_col_rgba, end_elem_xf_slot, grad_endpoints_for_rect, make_elem_brush, make_resolved_tf,
    make_resolved_tf_named, record_elem_box_slot, resolve_card_plate_colors,
    resolve_elem_grad_colors, rgba_u8, rim_gradient_stops, ClockElemBox, FgKeyHasher,
    WidgetFgCacheSlot, WidgetHitZone, PT_TO_PX,
};
use crate::system_media::{CcSnapshot, MediaSnapshot};
use crate::taskbar_styler::{paint_widget_elemental_fx, VizSnapshot};
use crate::theme::{resolve_palette_for, ThemePalette};

pub struct CoverBitmapCache {
    last_seq: u64,
    bmp: Option<ID2D1Bitmap>,
}

impl CoverBitmapCache {
    pub fn new() -> Self {
        Self {
            last_seq: u64::MAX,
            bmp: None,
        }
    }

    pub fn invalidate(&mut self) {
        self.last_seq = u64::MAX;
        self.bmp = None;
    }

    pub unsafe fn resolve(
        &mut self,
        rt: &ID2D1RenderTarget,
        wic: Option<&IWICImagingFactory>,
        media: &MediaSnapshot,
    ) -> Option<&ID2D1Bitmap> {
        if media.cover_seq != self.last_seq {
            self.last_seq = media.cover_seq;
            self.bmp = media.cover_bytes.as_ref().and_then(|bytes| {
                let w = wic?;
                let stream = SHCreateMemStream(Some(bytes.as_slice()))?;
                let dec = w
                    .CreateDecoderFromStream(&stream, std::ptr::null(), WICDecodeMetadataCacheOnDemand)
                    .ok()?;
                let frame = dec.GetFrame(0).ok()?;
                let conv = w.CreateFormatConverter().ok()?;
                conv.Initialize(
                    &frame,
                    &GUID_WICPixelFormat32bppPBGRA,
                    WICBitmapDitherTypeNone,
                    None,
                    0.0,
                    WICBitmapPaletteTypeCustom,
                )
                .ok()?;
                rt.CreateBitmapFromWicBitmap(&conv, None).ok()
            });
        }
        self.bmp.as_ref()
    }
}

unsafe fn round_stroke_style(d2d: Option<&ID2D1Factory>) -> Option<ID2D1StrokeStyle> {
    d2d?.CreateStrokeStyle(
        &D2D1_STROKE_STYLE_PROPERTIES {
            startCap: D2D1_CAP_STYLE_ROUND,
            endCap: D2D1_CAP_STYLE_ROUND,
            lineJoin: D2D1_LINE_JOIN_ROUND,
            ..Default::default()
        },
        None,
    )
    .ok()
}

unsafe fn draw_round_arc(
    rt: &ID2D1RenderTarget,
    d2d: Option<&ID2D1Factory>,
    p0: Vector2,
    p1: Vector2,
    radius: f32,
    br: &windows::Win32::Graphics::Direct2D::ID2D1Brush,
    width: f32,
    cap: Option<&ID2D1StrokeStyle>,
) {
    let Some(factory) = d2d else { return };
    if let Ok(geo) = factory.CreatePathGeometry() {
        if let Ok(sink) = geo.Open() {
            sink.BeginFigure(p0, D2D1_FIGURE_BEGIN_HOLLOW);
            sink.AddArc(&D2D1_ARC_SEGMENT {
                point: p1,
                size: D2D_SIZE_F { width: radius, height: radius },
                rotationAngle: 0.0,
                sweepDirection: D2D1_SWEEP_DIRECTION_CLOCKWISE,
                arcSize: D2D1_ARC_SIZE_SMALL,
            });
            sink.EndFigure(D2D1_FIGURE_END_OPEN);
            if sink.Close().is_ok() {
                rt.DrawGeometry(&geo, br, width, cap);
            }
        }
    }
}

unsafe fn draw_polyline(
    rt: &ID2D1RenderTarget,
    d2d: Option<&ID2D1Factory>,
    pts: &[Vector2],
    closed: bool,
    fill_br: Option<&windows::Win32::Graphics::Direct2D::ID2D1Brush>,
    stroke_br: Option<&windows::Win32::Graphics::Direct2D::ID2D1Brush>,
    stroke_w: f32,
    cap: Option<&ID2D1StrokeStyle>,
) {
    let Some(factory) = d2d else { return };
    if pts.len() < 2 {
        return;
    }
    if let Ok(geo) = factory.CreatePathGeometry() {
        if let Ok(sink) = geo.Open() {
            let begin = if fill_br.is_some() {
                windows::Win32::Graphics::Direct2D::Common::D2D1_FIGURE_BEGIN_FILLED
            } else {
                D2D1_FIGURE_BEGIN_HOLLOW
            };
            sink.BeginFigure(pts[0], begin);
            for pt in &pts[1..] {
                sink.AddLine(*pt);
            }
            sink.EndFigure(if closed {
                windows::Win32::Graphics::Direct2D::Common::D2D1_FIGURE_END_CLOSED
            } else {
                D2D1_FIGURE_END_OPEN
            });
            if sink.Close().is_ok() {
                if let Some(fb) = fill_br {
                    rt.FillGeometry(&geo, fb, None);
                }
                if let Some(sb) = stroke_br {
                    rt.DrawGeometry(&geo, sb, stroke_w, cap);
                }
            }
        }
    }
}

static MUSIC_EPOCH: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();

pub fn music_logical_size(cfg: &AppConfig) -> (f32, f32) {
    match cfg.music_style {
        MusicStyle::TurntablePro => (416.0, 114.0),
        MusicStyle::DualFloatOrb => (414.0, 106.0),
        MusicStyle::EditorialSleeve => (408.0, 106.0),
        MusicStyle::RadialHalo => (398.0, 116.0),
        MusicStyle::OscilloscopeCrt => (412.0, 106.0),
        MusicStyle::RibbonFlow => (418.0, 96.0),
        MusicStyle::EclipsePlayer => (408.0, 112.0),
        MusicStyle::CassetteDeck => (412.0, 116.0),
        MusicStyle::AnalogVuMeter => (416.0, 114.0),
        MusicStyle::HifiAmplifier => (414.0, 110.0),
        MusicStyle::WinampRack => (412.0, 112.0),
        MusicStyle::FlipJukebox => (416.0, 108.0),
        MusicStyle::VacuumTubeAmp => (420.0, 116.0),
        MusicStyle::BrushedTitaniumDeck => (416.0, 112.0),
        MusicStyle::RainWindowPlayer => (414.0, 110.0),
        MusicStyle::CalmWaterRipple => (416.0, 112.0),
        MusicStyle::LeatherFieldAudio => (416.0, 114.0),
        MusicStyle::WalnutHiFiConsole => (416.0, 112.0),
        MusicStyle::AbyssalTrenchAudio => (416.0, 112.0),
        MusicStyle::GlacierIceAudio => (416.0, 112.0),
        MusicStyle::CarbonSteelDeck => (416.0, 112.0),
        MusicStyle::GoldenHourAudio => (416.0, 112.0),
        MusicStyle::DynamicIslandCapsule => (444.0, 74.0),
        s if s.horology_theme().is_some() => (424.0, 118.0),
        _ => (410.0, 92.0),
    }
}

/// Logical [Prev, Play/Pause, Next] button centers + radii: (cx, cy, r)
fn music_button_layout(style: MusicStyle) -> [(f32, f32, f32); 3] {
    match style {
        MusicStyle::DynamicIslandCapsule => [(252.0, 37.0, 14.0), (288.0, 37.0, 17.0), (324.0, 37.0, 14.0)],
        MusicStyle::TurntablePro => [(302.0, 81.0, 13.0), (342.0, 81.0, 16.0), (382.0, 81.0, 13.0)],
        MusicStyle::DualFloatOrb => [(308.0, 53.0, 13.5), (346.0, 53.0, 17.5), (384.0, 53.0, 13.5)],
        MusicStyle::EditorialSleeve => [(304.0, 53.0, 13.5), (342.0, 53.0, 17.5), (380.0, 53.0, 13.5)],
        MusicStyle::RadialHalo => [(296.0, 58.0, 13.5), (334.0, 58.0, 17.5), (372.0, 58.0, 13.5)],
        MusicStyle::OscilloscopeCrt => [(114.0, 77.0, 12.5), (152.0, 77.0, 15.5), (190.0, 77.0, 12.5)],
        MusicStyle::RibbonFlow => [(316.0, 48.0, 13.5), (354.0, 48.0, 17.5), (392.0, 48.0, 13.5)],
        MusicStyle::EclipsePlayer => [(286.0, 56.0, 13.5), (326.0, 56.0, 17.5), (366.0, 56.0, 13.5)],
        MusicStyle::CassetteDeck => [(284.0, 72.0, 13.0), (326.0, 72.0, 16.5), (368.0, 72.0, 13.0)],
        MusicStyle::AnalogVuMeter => [(98.0, 84.0, 13.0), (138.0, 84.0, 16.0), (178.0, 84.0, 13.0)],
        MusicStyle::HifiAmplifier => [(302.0, 72.0, 13.5), (344.0, 72.0, 16.5), (386.0, 72.0, 13.5)],
        MusicStyle::WinampRack => [(296.0, 78.0, 13.0), (338.0, 78.0, 16.0), (380.0, 78.0, 13.0)],
        MusicStyle::FlipJukebox => [(308.0, 79.0, 13.0), (348.0, 79.0, 16.0), (388.0, 79.0, 13.0)],
        MusicStyle::VacuumTubeAmp => [(306.0, 78.0, 13.0), (346.0, 78.0, 16.5), (386.0, 78.0, 13.0)],
        MusicStyle::BrushedTitaniumDeck => [(304.0, 74.0, 13.0), (344.0, 74.0, 16.0), (384.0, 74.0, 13.0)],
        MusicStyle::RainWindowPlayer => [(302.0, 72.0, 13.0), (342.0, 72.0, 16.0), (382.0, 72.0, 13.0)],
        MusicStyle::CalmWaterRipple => [(304.0, 74.0, 13.0), (344.0, 74.0, 16.0), (384.0, 74.0, 13.0)],
        MusicStyle::LeatherFieldAudio => [(304.0, 76.0, 13.0), (344.0, 76.0, 16.0), (384.0, 76.0, 13.0)],
        MusicStyle::WalnutHiFiConsole => [(304.0, 74.0, 13.0), (344.0, 74.0, 16.0), (384.0, 74.0, 13.0)],
        MusicStyle::AbyssalTrenchAudio => [(304.0, 74.0, 13.0), (344.0, 74.0, 16.0), (384.0, 74.0, 13.0)],
        MusicStyle::GlacierIceAudio => [(304.0, 74.0, 13.0), (344.0, 74.0, 16.0), (384.0, 74.0, 13.0)],
        MusicStyle::CarbonSteelDeck => [(304.0, 74.0, 13.0), (344.0, 74.0, 16.0), (384.0, 74.0, 13.0)],
        MusicStyle::GoldenHourAudio => [(304.0, 74.0, 13.0), (344.0, 74.0, 16.0), (384.0, 74.0, 13.0)],
        s if s.horology_theme().is_some() => [(306.0, 82.0, 13.0), (346.0, 82.0, 16.5), (386.0, 82.0, 13.0)],
        _ => [(308.0, 46.0, 16.0), (348.0, 46.0, 20.0), (388.0, 46.0, 16.0)],
    }
}

pub fn music_transport_rects(cfg: &AppConfig) -> [(f32, f32, f32, f32); 3] {
    let layout = music_button_layout(cfg.music_style);
    [
        (layout[0].0 - layout[0].2 - 2.0, layout[0].1 - layout[0].2 - 2.0, layout[0].0 + layout[0].2 + 2.0, layout[0].1 + layout[0].2 + 2.0),
        (layout[1].0 - layout[1].2 - 2.0, layout[1].1 - layout[1].2 - 2.0, layout[1].0 + layout[1].2 + 2.0, layout[1].1 + layout[1].2 + 2.0),
        (layout[2].0 - layout[2].2 - 2.0, layout[2].1 - layout[2].2 - 2.0, layout[2].0 + layout[2].2 + 2.0, layout[2].1 + layout[2].2 + 2.0),
    ]
}

unsafe fn draw_music_arc(
    rt: &ID2D1RenderTarget,
    d2d: Option<&ID2D1Factory>,
    cx: f32,
    cy: f32,
    r: f32,
    start_deg: f32,
    sweep_deg: f32,
    width: f32,
    br: &ID2D1Brush,
    cap: Option<&ID2D1StrokeStyle>,
) {
    if sweep_deg.abs() < 0.3 || r < 0.5 {
        return;
    }
    let (a_beg, sw) = if sweep_deg < 0.0 {
        (start_deg + sweep_deg, (-sweep_deg).min(359.4))
    } else {
        (start_deg, sweep_deg.min(359.4))
    };
    let a0 = a_beg.to_radians();
    let a1 = (a_beg + sw).to_radians();
    let p0 = Vector2 { X: cx + r * a0.cos(), Y: cy + r * a0.sin() };
    let p1 = Vector2 { X: cx + r * a1.cos(), Y: cy + r * a1.sin() };
    if let Some(factory) = d2d {
        if let Ok(geo) = factory.CreatePathGeometry() {
            if let Ok(sink) = geo.Open() {
                sink.BeginFigure(p0, D2D1_FIGURE_BEGIN_HOLLOW);
                sink.AddArc(&D2D1_ARC_SEGMENT {
                    point: p1,
                    size: D2D_SIZE_F { width: r, height: r },
                    rotationAngle: 0.0,
                    sweepDirection: D2D1_SWEEP_DIRECTION_CLOCKWISE,
                    arcSize: if sw > 180.0 { D2D1_ARC_SIZE_LARGE } else { D2D1_ARC_SIZE_SMALL },
                });
                sink.EndFigure(D2D1_FIGURE_END_OPEN);
                if sink.Close().is_ok() {
                    rt.DrawGeometry(&geo, br, width, cap);
                    return;
                }
            }
        }
    }
    let steps = 20;
    for i in 0..steps {
        let t0 = a0 + (a1 - a0) * (i as f32 / steps as f32);
        let t1 = a0 + (a1 - a0) * ((i + 1) as f32 / steps as f32);
        rt.DrawLine(
            Vector2 { X: cx + r * t0.cos(), Y: cy + r * t0.sin() },
            Vector2 { X: cx + r * t1.cos(), Y: cy + r * t1.sin() },
            br,
            width,
            None,
        );
    }
}

#[allow(clippy::too_many_arguments)]
unsafe fn draw_music_plate_rr_with_fx(
    rt: &ID2D1RenderTarget,
    d2d: Option<&ID2D1Factory>,
    rect: D2D_RECT_F,
    radius: f32,
    canvas_box: (f32, f32, f32, f32),
    s: f32,
    cfg: &AppConfig,
    pal: &ThemePalette,
    is_hov: bool,
    glass_mult: f32,
    rim_w: f32,
    fx_viz: Option<&VizSnapshot>,
    paint_fx: bool,
) {
    let tr_card = cfg.music_elem_transforms[5];
    if !tr_card.visible {
        return;
    }
    let rad_scale = (cfg.music_corner_radius as f32 / 22.0).clamp(0.0, 2.2);
    let eff_radius = radius * rad_scale;
    let rr = D2D1_ROUNDED_RECT { rect, radiusX: eff_radius, radiusY: eff_radius };
    let r_base = if is_hov {
        ((cfg.music_rim_alpha as f32 / 255.0) + 0.22).clamp(0.25, 0.96)
    } else {
        (cfg.music_rim_alpha as f32 / 255.0).clamp(0.0, 1.0)
    };
    let g_top = ((cfg.music_glass_alpha as f32 / 255.0) * glass_mult).clamp(0.02, 0.94);
    let g_bot = (g_top * 0.60).clamp(0.02, 0.84);
    let (top_c, bot_c, rim_c, ang) = resolve_card_plate_colors(&tr_card, pal, g_top, g_bot, r_base);
    if cfg.music_show_plate {
        let (gp0, gp1) = grad_endpoints_for_rect((rect.left, rect.top, rect.right, rect.bottom), ang);
        let bg_stops = [
            D2D1_GRADIENT_STOP { position: 0.0, color: top_c },
            D2D1_GRADIENT_STOP { position: 1.0, color: bot_c },
        ];
        if let Some(bg) = create_linear_grad(rt, gp0, gp1, &bg_stops) {
            rt.FillRoundedRectangle(&rr, &bg);
        }
    }
    if paint_fx && cfg.music_show_plate {
        paint_widget_elemental_fx(
            rt,
            d2d,
            rect.left,
            rect.top,
            rect.right,
            rect.bottom,
            canvas_box,
            eff_radius,
            s,
            cfg.music_hover_fx,
            88,
            pal,
            !cfg.music_match_wallpaper && !cfg.music_use_custom_color && tr_card.color_rgb.is_none(),
            fx_viz,
        );
    }
    if cfg.music_show_plate {
        if rim_w > 0.05 && (r_base > 0.01 || tr_card.border_rgb.is_some() || tr_card.color_rgb.is_some()) {
            let rim_stops = rim_gradient_stops(&tr_card, pal, rim_c, r_base);
            if let Some(rb) = create_linear_grad(rt, (rect.left, rect.top), (rect.right, rect.bottom), &rim_stops) {
                rt.DrawRoundedRectangle(&rr, &rb, rim_w, None);
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
unsafe fn draw_music_plate_circle_with_fx(
    rt: &ID2D1RenderTarget,
    d2d: Option<&ID2D1Factory>,
    cx: f32,
    cy: f32,
    r: f32,
    canvas_box: (f32, f32, f32, f32),
    s: f32,
    cfg: &AppConfig,
    pal: &ThemePalette,
    is_hov: bool,
    glass_mult: f32,
    rim_w: f32,
    fx_viz: Option<&VizSnapshot>,
    paint_fx: bool,
) {
    let tr_card = cfg.music_elem_transforms[5];
    if !tr_card.visible {
        return;
    }
    let ell = D2D1_ELLIPSE { point: Vector2 { X: cx, Y: cy }, radiusX: r, radiusY: r };
    let r_base = if is_hov {
        ((cfg.music_rim_alpha as f32 / 255.0) + 0.22).clamp(0.25, 0.96)
    } else {
        (cfg.music_rim_alpha as f32 / 255.0).clamp(0.0, 1.0)
    };
    let g_top = ((cfg.music_glass_alpha as f32 / 255.0) * glass_mult).clamp(0.02, 0.94);
    let g_bot = (g_top * 0.60).clamp(0.02, 0.84);
    let (top_c, bot_c, rim_c, ang) = resolve_card_plate_colors(&tr_card, pal, g_top, g_bot, r_base);
    if cfg.music_show_plate {
        let (gp0, gp1) = grad_endpoints_for_rect((cx - r, cy - r, cx + r, cy + r), ang);
        let bg_stops = [
            D2D1_GRADIENT_STOP { position: 0.0, color: top_c },
            D2D1_GRADIENT_STOP { position: 1.0, color: bot_c },
        ];
        if let Some(bg) = create_linear_grad(rt, gp0, gp1, &bg_stops) {
            rt.FillEllipse(&ell, &bg);
        }
    }
    if paint_fx && cfg.music_show_plate {
        paint_widget_elemental_fx(
            rt,
            d2d,
            cx - r,
            cy - r,
            cx + r,
            cy + r,
            canvas_box,
            r,
            s,
            cfg.music_hover_fx,
            88,
            pal,
            !cfg.music_match_wallpaper && !cfg.music_use_custom_color && tr_card.color_rgb.is_none(),
            fx_viz,
        );
    }
    if cfg.music_show_plate {
        if rim_w > 0.05 && (r_base > 0.01 || tr_card.border_rgb.is_some() || tr_card.color_rgb.is_some()) {
            let rim_stops = rim_gradient_stops(&tr_card, pal, rim_c, r_base);
            if let Some(rb) = create_linear_grad(rt, (cx - r, cy - r), (cx + r, cy + r), &rim_stops) {
                rt.DrawEllipse(&ell, &rb, rim_w, None);
            }
        }
    }
}

/// Theme-adaptive procedural Direct2D vinyl record jacket / center label fallback.
/// Automatically inherits the active suite accent, custom Music Accent, Wallpaper Chameleon,
/// or Pen Canvas / Music Colors Vinyl override (`tr0.color_rgb` / `tr0.color2_rgb`) —
/// replacing the old hardcoded orange-yellow static `cover.png`.
unsafe fn draw_procedural_vinyl_art(
    rt: &ID2D1RenderTarget,
    d2d: Option<&ID2D1Factory>,
    rect: D2D_RECT_F,
    radius: f32,
    pal: &ThemePalette,
    s: f32,
) {
    let cx = (rect.left + rect.right) * 0.5;
    let cy = (rect.top + rect.bottom) * 0.5;
    let min_d = (rect.right - rect.left).min(rect.bottom - rect.top).max(16.0);
    let active_tr = current_active_elem_tr();
    let rr = D2D1_ROUNDED_RECT { rect, radiusX: radius, radiusY: radius };

    // Optional jacket backdrop gradient (respects Pen Canvas secondary color `tr0.color2_rgb`)
    let (bg_top, bg_bot) = if let Some(c2) = active_tr.and_then(|t| t.color2_rgb) {
        let (r2, g2, b2) = crate::theme::u32_to_rgb(c2);
        (rgba_u8(r2, g2, b2, 0.96), rgba_u8(r2 / 2, g2 / 2, b2 / 2, 0.98))
    } else {
        (pal.glass_top_col(0.96), pal.glass_bot_col(0.98))
    };
    let bg_stops = [
        D2D1_GRADIENT_STOP { position: 0.0, color: bg_top },
        D2D1_GRADIENT_STOP { position: 1.0, color: bg_bot },
    ];
    if let Some(bg_br) = create_linear_grad(rt, (rect.left, rect.top), (rect.right, rect.bottom), &bg_stops) {
        rt.FillRoundedRectangle(&rr, &bg_br);
    }

    let (lbl_hi, lbl_lo) = if let Some(c1) = active_tr.and_then(|t| t.color_rgb) {
        let (r1, g1, b1) = crate::theme::u32_to_rgb(c1);
        (
            rgba_u8(r1.saturating_add(32), g1.saturating_add(32), b1.saturating_add(32), 0.98),
            rgba_u8(r1, g1, b1, 0.98),
        )
    } else {
        (pal.light_col(0.96), pal.accent_col(0.96))
    };

    // Circular platter center label (e.g. TurntablePro inner label) vs full vinyl jacket
    let is_center_label_only = radius >= min_d * 0.44 && min_d < 54.0 * s;
    if is_center_label_only {
        let lr = min_d * 0.48;
        let lbl_stops = [
            D2D1_GRADIENT_STOP { position: 0.0, color: lbl_hi },
            D2D1_GRADIENT_STOP { position: 1.0, color: lbl_lo },
        ];
        if let Some(lb) = create_linear_grad(rt, (cx - lr, cy - lr), (cx + lr, cy + lr), &lbl_stops) {
            rt.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: cx, Y: cy }, radiusX: lr, radiusY: lr }, &lb);
        }
        if let Ok(ring) = rt.CreateSolidColorBrush(&rgba_u8(0, 0, 0, 0.22), None) {
            rt.DrawEllipse(&D2D1_ELLIPSE { point: Vector2 { X: cx, Y: cy }, radiusX: lr * 0.72, radiusY: lr * 0.72 }, &ring, 0.85 * s, None);
        }
        return;
    }

    // Full matte 12" vinyl disc nested in dark theme-matched sleeve
    let disc_r = min_d * 0.425;
    if let Ok(vb) = rt.CreateSolidColorBrush(&rgba_u8(15, 14, 17, 0.96), None) {
        rt.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: cx, Y: cy }, radiusX: disc_r, radiusY: disc_r }, &vb);
    }
    if let Ok(vrim) = rt.CreateSolidColorBrush(&pal.accent_col(0.28), None) {
        rt.DrawEllipse(&D2D1_ELLIPSE { point: Vector2 { X: cx, Y: cy }, radiusX: disc_r, radiusY: disc_r }, &vrim, 0.9 * s, None);
    }
    if let Ok(gr) = rt.CreateSolidColorBrush(&pal.text_col(0.14), None) {
        for f in [0.38f32, 0.33, 0.28, 0.23] {
            let gr_r = min_d * f;
            rt.DrawEllipse(&D2D1_ELLIPSE { point: Vector2 { X: cx, Y: cy }, radiusX: gr_r, radiusY: gr_r }, &gr, 0.75 * s, None);
        }
    }
    // Subtle 135° glossy vinyl reflection arcs
    if let Ok(sh_br) = rt.CreateSolidColorBrush(&rgba_u8(255, 255, 255, 0.14), None) {
        let cap_r = round_stroke_style(d2d);
        let b: &ID2D1Brush = &sh_br;
        draw_music_arc(rt, d2d, cx, cy, min_d * 0.32, -55.0, 34.0, 1.8 * s, b, cap_r.as_ref());
        draw_music_arc(rt, d2d, cx, cy, min_d * 0.32, 125.0, 34.0, 1.8 * s, b, cap_r.as_ref());
    }
    // Theme-matched center record label
    let lbl_r = min_d * 0.185;
    let lbl_stops = [
        D2D1_GRADIENT_STOP { position: 0.0, color: lbl_hi },
        D2D1_GRADIENT_STOP { position: 1.0, color: lbl_lo },
    ];
    if let Some(lb) = create_linear_grad(rt, (cx - lbl_r, cy - lbl_r), (cx + lbl_r, cy + lbl_r), &lbl_stops) {
        rt.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: cx, Y: cy }, radiusX: lbl_r, radiusY: lbl_r }, &lb);
    }
    if let Ok(ir) = rt.CreateSolidColorBrush(&rgba_u8(0, 0, 0, 0.20), None) {
        rt.DrawEllipse(&D2D1_ELLIPSE { point: Vector2 { X: cx, Y: cy }, radiusX: min_d * 0.132, radiusY: min_d * 0.132 }, &ir, 0.75 * s, None);
    }
    // Center spindle hole + subtle metallic collar
    let hole_r = (min_d * 0.048).max(2.2 * s);
    if let Ok(hb) = rt.CreateSolidColorBrush(&rgba_u8(12, 11, 13, 0.98), None) {
        rt.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: cx, Y: cy }, radiusX: hole_r, radiusY: hole_r }, &hb);
    }
    if let Ok(hr) = rt.CreateSolidColorBrush(&pal.text_col(0.28), None) {
        rt.DrawEllipse(&D2D1_ELLIPSE { point: Vector2 { X: cx, Y: cy }, radiusX: hole_r + 0.5 * s, radiusY: hole_r + 0.5 * s }, &hr, 0.6 * s, None);
    }
}

unsafe fn draw_music_cover_art(
    rt: &ID2D1RenderTarget,
    d2d: Option<&ID2D1Factory>,
    rect: D2D_RECT_F,
    radius: f32,
    cover_bmp: Option<&ID2D1Bitmap>,
    pal: &ThemePalette,
    border_w: f32,
    s: f32,
) {
    let rr = D2D1_ROUNDED_RECT { rect, radiusX: radius, radiusY: radius };
    if let Ok(fb) = rt.CreateSolidColorBrush(&pal.glass_top_col(1.0), None) {
        rt.FillRoundedRectangle(&rr, &fb);
    }
    if let Some(bmp) = cover_bmp {
        let mut drew_clipped = false;
        if let Some(factory) = d2d {
            if let Ok(clip_geo) = factory.CreateRoundedRectangleGeometry(&rr) {
                if let Ok(layer) = rt.CreateLayer(None) {
                    let mask_geo: ID2D1Geometry = clip_geo.into();
                    let params = D2D1_LAYER_PARAMETERS {
                        contentBounds: rr.rect,
                        geometricMask: std::mem::ManuallyDrop::new(Some(mask_geo.clone())),
                        maskAntialiasMode: D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
                        maskTransform: windows_numerics::Matrix3x2::identity(),
                        opacity: 1.0,
                        opacityBrush: std::mem::ManuallyDrop::new(None),
                        layerOptions: D2D1_LAYER_OPTIONS_NONE,
                    };
                    rt.PushLayer(&params, &layer);
                    rt.DrawBitmap(bmp, Some(&rr.rect), 1.0, D2D1_BITMAP_INTERPOLATION_MODE_LINEAR, None);
                    rt.PopLayer();
                    std::mem::ManuallyDrop::into_inner(params.geometricMask);
                    drew_clipped = true;
                }
            }
        }
        if !drew_clipped {
            rt.DrawBitmap(bmp, Some(&rr.rect), 1.0, D2D1_BITMAP_INTERPOLATION_MODE_LINEAR, None);
        }
    } else {
        draw_procedural_vinyl_art(rt, d2d, rect, radius, pal, s);
    }
    let active_tr = current_active_elem_tr();
    if border_w > 0.05 {
        let rim_c = elem_col_rgba(
            active_tr.and_then(|t| t.border_rgb.or(t.color_rgb)),
            0.76,
            pal.accent_col(0.62),
        );
        let eff_bw = active_tr
            .filter(|t| t.border_width > 0)
            .map(|t| (t.border_width as f32 * 0.7).max(border_w))
            .unwrap_or(border_w);
        if let Ok(cr) = rt.CreateSolidColorBrush(&rim_c, None) {
            rt.DrawRoundedRectangle(&rr, &cr, eff_bw * s, None);
        }
    }
}

unsafe fn draw_music_transport(
    rt: &ID2D1RenderTarget,
    d2d: Option<&ID2D1Factory>,
    x: f32,
    y: f32,
    s: f32,
    layout: [(f32, f32, f32); 3],
    rectangular: bool,
    pal: &ThemePalette,
    media: &MediaSnapshot,
    hover: WidgetHitZone,
    br_cream: Option<&windows::Win32::Graphics::Direct2D::ID2D1SolidColorBrush>,
) {
    let active_tr = current_active_elem_tr();
    let prv_hov = hover == WidgetHitZone::MusicPrev;
    let ply_hov = hover == WidgetHitZone::MusicPlayPause;
    let nxt_hov = hover == WidgetHitZone::MusicNext;

    let draw_side_shell = |cx: f32, cy: f32, r: f32, hov: bool| {
        let fill_col = pal.accent_col(if rectangular { if hov { 0.36 } else { 0.18 } } else if hov { 0.34 } else { 0.16 });
        let default_sc = pal.accent_col(if rectangular { if hov { 0.85 } else { 0.34 } } else if hov { 0.80 } else { 0.30 });
        let stroke_col = elem_col_rgba(
            active_tr.and_then(|t| t.border_rgb),
            if hov { 0.88 } else { 0.56 },
            default_sc,
        );
        if rectangular {
            let rr = D2D1_ROUNDED_RECT {
                rect: D2D_RECT_F { left: cx - r, top: cy - r * 0.85, right: cx + r, bottom: cy + r * 0.85 },
                radiusX: 4.5 * s,
                radiusY: 4.5 * s,
            };
            if let Ok(fb) = rt.CreateSolidColorBrush(&fill_col, None) {
                rt.FillRoundedRectangle(&rr, &fb);
            }
            if let Ok(sb) = rt.CreateSolidColorBrush(&stroke_col, None) {
                rt.DrawRoundedRectangle(&rr, &sb, 1.0 * s, None);
            }
        } else {
            let pt = Vector2 { X: cx, Y: cy };
            if let Ok(fb) = rt.CreateSolidColorBrush(&fill_col, None) {
                rt.FillEllipse(&D2D1_ELLIPSE { point: pt, radiusX: r, radiusY: r }, &fb);
            }
            if let Ok(sb) = rt.CreateSolidColorBrush(&stroke_col, None) {
                rt.DrawEllipse(&D2D1_ELLIPSE { point: pt, radiusX: r, radiusY: r }, &sb, 1.0 * s, None);
            }
        }
    };

    // Prev
    let (px, py, pr) = (x + layout[0].0 * s, y + layout[0].1 * s, layout[0].2 * s);
    draw_side_shell(px, py, pr, prv_hov);
    if let Some(cbr) = br_cream {
        let sc = pr / 15.0;
        let bar = D2D1_ROUNDED_RECT {
            rect: D2D_RECT_F { left: px - 5.0 * sc, top: py - 4.8 * sc, right: px - 3.0 * sc, bottom: py + 4.8 * sc },
            radiusX: 0.9 * sc,
            radiusY: 0.9 * sc,
        };
        rt.FillRoundedRectangle(&bar, cbr);
        let pts = [
            Vector2 { X: px - 2.4 * sc, Y: py },
            Vector2 { X: px + 4.8 * sc, Y: py - 4.8 * sc },
            Vector2 { X: px + 4.8 * sc, Y: py + 4.8 * sc },
        ];
        draw_polyline(rt, d2d, &pts, true, Some(cbr.into()), None, 0.0, None);
    }

    // Play/Pause
    let (kx, ky, kr) = (x + layout[1].0 * s, y + layout[1].1 * s, layout[1].2 * s);
    let (pc1, pc2, pang) = resolve_elem_grad_colors(
        active_tr.as_ref(),
        1.0,
        if ply_hov { pal.text_col(1.0) } else { pal.light_col(1.0) },
        pal.deep_col(1.0),
    );
    let (gp0, gp1) = grad_endpoints_for_rect((kx - kr, ky - kr, kx + kr, ky + kr), pang);
    let ply_stops = [
        D2D1_GRADIENT_STOP { position: 0.0, color: pc1 },
        D2D1_GRADIENT_STOP { position: 1.0, color: pc2 },
    ];
    let ply_rim = elem_col_rgba(active_tr.and_then(|t| t.border_rgb), 0.90, pal.light_col(0.88));
    if let Some(pg) = create_linear_grad(rt, gp0, gp1, &ply_stops) {
        if rectangular {
            let rr = D2D1_ROUNDED_RECT {
                rect: D2D_RECT_F { left: kx - kr, top: ky - kr * 0.88, right: kx + kr, bottom: ky + kr * 0.88 },
                radiusX: 5.5 * s,
                radiusY: 5.5 * s,
            };
            rt.FillRoundedRectangle(&rr, &pg);
            if let Ok(rb) = rt.CreateSolidColorBrush(&ply_rim, None) {
                rt.DrawRoundedRectangle(&rr, &rb, 1.15 * s, None);
            }
        } else {
            let pt = Vector2 { X: kx, Y: ky };
            rt.FillEllipse(&D2D1_ELLIPSE { point: pt, radiusX: kr, radiusY: kr }, &pg);
            if let Ok(rb) = rt.CreateSolidColorBrush(&ply_rim, None) {
                rt.DrawEllipse(&D2D1_ELLIPSE { point: pt, radiusX: kr, radiusY: kr }, &rb, 1.15 * s, None);
            }
        }
    }
    if let Ok(dark) = rt.CreateSolidColorBrush(&rgba_u8(18, 14, 12, 1.0), None) {
        let sc = kr / 19.0;
        if media.is_playing {
            let b1 = D2D1_ROUNDED_RECT {
                rect: D2D_RECT_F { left: kx - 4.8 * sc, top: ky - 6.2 * sc, right: kx - 1.6 * sc, bottom: ky + 6.2 * sc },
                radiusX: 1.4 * sc,
                radiusY: 1.4 * sc,
            };
            let b2 = D2D1_ROUNDED_RECT {
                rect: D2D_RECT_F { left: kx + 1.6 * sc, top: ky - 6.2 * sc, right: kx + 4.8 * sc, bottom: ky + 6.2 * sc },
                radiusX: 1.4 * sc,
                radiusY: 1.4 * sc,
            };
            rt.FillRoundedRectangle(&b1, &dark);
            rt.FillRoundedRectangle(&b2, &dark);
        } else {
            let pts = [
                Vector2 { X: kx - 3.6 * sc, Y: ky - 6.2 * sc },
                Vector2 { X: kx + 6.4 * sc, Y: ky },
                Vector2 { X: kx - 3.6 * sc, Y: ky + 6.2 * sc },
            ];
            let dark_b: &ID2D1Brush = &dark;
            draw_polyline(rt, d2d, &pts, true, Some(dark_b), None, 0.0, None);
        }
    }

    // Next
    let (nx, ny, nr) = (x + layout[2].0 * s, y + layout[2].1 * s, layout[2].2 * s);
    draw_side_shell(nx, ny, nr, nxt_hov);
    if let Some(cbr) = br_cream {
        let sc = nr / 15.0;
        let pts = [
            Vector2 { X: nx - 4.8 * sc, Y: ny - 4.8 * sc },
            Vector2 { X: nx + 2.4 * sc, Y: ny },
            Vector2 { X: nx - 4.8 * sc, Y: ny + 4.8 * sc },
        ];
        draw_polyline(rt, d2d, &pts, true, Some(cbr.into()), None, 0.0, None);
        let bar = D2D1_ROUNDED_RECT {
            rect: D2D_RECT_F { left: nx + 3.0 * sc, top: ny - 4.8 * sc, right: nx + 5.0 * sc, bottom: ny + 4.8 * sc },
            radiusX: 0.9 * sc,
            radiusY: 0.9 * sc,
        };
        rt.FillRoundedRectangle(&bar, cbr);
    }
}

// =========================================================================
// PHASE 2A: NOWPLAYING MUSIC BAR (18 Distinct Music Themes Direct2D Renderer)
// =========================================================================
unsafe fn draw_m_text(
    rt: &ID2D1RenderTarget,
    tf: &windows::Win32::Graphics::DirectWrite::IDWriteTextFormat,
    txt: &str,
    rect: D2D_RECT_F,
    br: &windows::Win32::Graphics::Direct2D::ID2D1SolidColorBrush,
    sh: Option<&windows::Win32::Graphics::Direct2D::ID2D1SolidColorBrush>,
    sh_off: f32,
) {
    let _ = tf.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP);
    let h = (rect.bottom - rect.top).abs().max(8.0);
    let clip_rc = D2D_RECT_F {
        left: rect.left,
        top: rect.top - h * 1.5,
        right: rect.right,
        bottom: rect.bottom + h * 1.5,
    };
    rt.PushAxisAlignedClip(&clip_rc, D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
    draw_shadowed_text(rt, txt, tf, rect, br, sh, sh_off, false);
    rt.PopAxisAlignedClip();
}

#[allow(clippy::too_many_arguments)]
unsafe fn draw_music_custom_styles(
    rt: &ID2D1RenderTarget,
    dw: &IDWriteFactory,
    d2d: Option<&ID2D1Factory>,
    custom_fonts: Option<&IDWriteFontCollection>,
    x: f32,
    y: f32,
    s: f32,
    cfg: &AppConfig,
    media: &MediaSnapshot,
    eff_bands: &[f32; 12],
    has_audio: bool,
    anim_t: f32,
    cover_bmp: Option<&ID2D1Bitmap>,
    hover: WidgetHitZone,
    pal: &ThemePalette,
    fx_viz: Option<&VizSnapshot>,
    mut out_boxes: Option<&mut [ClockElemBox; 6]>,
    skip_plate_and_fx: bool,
    paint_fx: bool,
) -> bool {
    if matches!(
        cfg.music_style,
        MusicStyle::Sanctuary
            | MusicStyle::VinylDeck
            | MusicStyle::MinimalPill
            | MusicStyle::SpectrumDeck
            | MusicStyle::NeonWave
            | MusicStyle::CyberHud
    ) {
        return false;
    }
    let style = cfg.music_style;

    let (lw, lh) = music_logical_size(cfg);
    let w = lw * s;
    let h = lh * s;
    let tr_card = cfg.music_elem_transforms[5];
    {
        let mut slice_opt = out_boxes.as_deref_mut().map(|b| &mut b[..]);
        record_elem_box_slot(&mut slice_opt, 5, tr_card, (x, y, x + w, y + h), s);
    }
    let draw_music_plate_rr = |rt_p: &ID2D1RenderTarget,
                               rect: D2D_RECT_F,
                               radius: f32,
                               cfg_p: &AppConfig,
                               pal_p: &ThemePalette,
                               is_hov: bool,
                               glass_mult: f32,
                               rim_w: f32| {
        let mut nb: Option<&mut [ClockElemBox]> = None;
        let g = begin_elem_xf_slot(rt_p, 5, tr_card, (x, y, x + w, y + h), s, &mut nb);
        draw_music_plate_rr_with_fx(
            rt_p,
            d2d,
            rect,
            radius,
            (x, y, w, h),
            s,
            cfg_p,
            pal_p,
            is_hov,
            glass_mult,
            rim_w,
            fx_viz,
            paint_fx,
        );
        end_elem_xf_slot(rt_p, g);
    };
    let draw_music_plate_circle = |rt_p: &ID2D1RenderTarget,
                                   cx: f32,
                                   cy: f32,
                                   r: f32,
                                   cfg_p: &AppConfig,
                                   pal_p: &ThemePalette,
                                   is_hov: bool,
                                   glass_mult: f32,
                                   rim_w: f32| {
        let mut nb: Option<&mut [ClockElemBox]> = None;
        let g = begin_elem_xf_slot(rt_p, 5, tr_card, (x, y, x + w, y + h), s, &mut nb);
        draw_music_plate_circle_with_fx(
            rt_p,
            d2d,
            cx,
            cy,
            r,
            (x, y, w, h),
            s,
            cfg_p,
            pal_p,
            is_hov,
            glass_mult,
            rim_w,
            fx_viz,
            paint_fx,
        );
        end_elem_xf_slot(rt_p, g);
    };
    let is_card_hov = matches!(
        hover,
        WidgetHitZone::MusicCard
            | WidgetHitZone::MusicPrev
            | WidgetHitZone::MusicPlayPause
            | WidgetHitZone::MusicNext
    );
    let layout = music_button_layout(style);

    let br_amber = rt.CreateSolidColorBrush(&pal.accent_col(1.0), None).ok();
    let br_gold = rt.CreateSolidColorBrush(&pal.light_col(1.0), None).ok();
    let br_cream = rt.CreateSolidColorBrush(&pal.text_col(1.0), None).ok();
    let br_sand = rt.CreateSolidColorBrush(&pal.muted_col(0.92), None).ok();
    let br_sh = rt.CreateSolidColorBrush(&rgba_u8(0, 0, 0, 0.70), None).ok();
    let cap_round = round_stroke_style(d2d);

    let fs = (cfg.music_font_scale_pct.clamp(70, 140) as f32) / 100.0;
    let tr0 = cfg.music_elem_transforms[0];
    let tr1 = cfg.music_elem_transforms[1];
    let tr2 = cfg.music_elem_transforms[2];
    let tr3 = cfg.music_elem_transforms[3];
    let tr4 = cfg.music_elem_transforms[4];
    let viz_pal = tint_cc_pal(pal, tr4.color_rgb);
    let cov_pal = tint_cc_pal(pal, tr0.color_rgb);
    let t_fnt = if !cfg.music_elem_fonts[1].is_empty() { &cfg.music_elem_fonts[1] } else { &cfg.music_font_title };
    let a_fnt = if !cfg.music_elem_fonts[2].is_empty() { &cfg.music_elem_fonts[2] } else { &cfg.music_font_artist };
    let t_fs = fs * (tr1.font_scale_pct as f32 / 100.0).clamp(0.50, 2.20);
    let a_fs = fs * (tr2.font_scale_pct as f32 / 100.0).clamp(0.50, 2.20);

    let tf_badge = if cfg.music_show_badge {
        make_resolved_tf(
            dw,
            custom_fonts,
            &[w!("Cascadia Mono"), w!("SF Pro Display"), w!("Segoe UI")],
            7.5 * PT_TO_PX * s * fs,
            DWRITE_FONT_WEIGHT_BOLD,
            DWRITE_TEXT_ALIGNMENT_LEADING,
            DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
        )
    } else {
        None
    };
    let tf_title = if cfg.music_show_title {
        make_resolved_tf_named(
            dw,
            custom_fonts,
            t_fnt,
            &[w!("SF Pro Display"), w!("Segoe UI Semibold"), w!("Segoe UI")],
            12.0 * PT_TO_PX * s * t_fs,
            DWRITE_FONT_WEIGHT_SEMI_BOLD,
            DWRITE_TEXT_ALIGNMENT_LEADING,
            DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
        )
    } else {
        None
    };
    let tf_art = if cfg.music_show_artist {
        make_resolved_tf_named(
            dw,
            custom_fonts,
            a_fnt,
            &[w!("SF Pro Display"), w!("Segoe UI")],
            9.2 * PT_TO_PX * s * a_fs,
            DWRITE_FONT_WEIGHT_REGULAR,
            DWRITE_TEXT_ALIGNMENT_LEADING,
            DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
        )
    } else {
        None
    };

    let boxes_cell: std::cell::RefCell<Option<&mut [ClockElemBox; 6]>> = std::cell::RefCell::new(out_boxes.take());
    let title_raw = tf_title.as_ref().map(windows::core::Interface::as_raw);
    let art_raw = tf_art.as_ref().map(windows::core::Interface::as_raw);
    let badge_raw = tf_badge.as_ref().map(windows::core::Interface::as_raw);
    let br_t_ov = make_elem_brush(rt, tr1.color_rgb, 1.0);
    let br_a_ov = make_elem_brush(rt, tr2.color_rgb, 1.0);
    let br_c_ov = make_elem_brush(rt, tr3.color_rgb, 1.0);
    let br_v_ov = make_elem_brush(rt, tr4.color_rgb, 1.0);

    macro_rules! with_viz_slot {
        ($vl:expr, $vt:expr, $vr:expr, $vb:expr, $body:block) => {{
            if cfg.music_show_viz && tr4.visible {
                let mut b_ref = boxes_cell.borrow_mut();
                let mut slice_opt = b_ref.as_deref_mut().map(|bx| &mut bx[..]);
                let g_v = begin_elem_xf_slot(rt, 4, tr4, ($vl, $vt, $vr, $vb), s, &mut slice_opt);
                drop(b_ref);
                $body
                end_elem_xf_slot(rt, g_v);
            } else {
                let mut b_ref = boxes_cell.borrow_mut();
                let mut slice_opt = b_ref.as_deref_mut().map(|bx| &mut bx[..]);
                record_elem_box_slot(&mut slice_opt, 4, tr4, ($vl, $vt, $vr, $vb), s);
            }
        }};
    }

    let draw_m_text = |rt_m: &ID2D1RenderTarget,
                       tf_m: &windows::Win32::Graphics::DirectWrite::IDWriteTextFormat,
                       txt_m: &str,
                       rect_m: D2D_RECT_F,
                       br_m: &windows::Win32::Graphics::Direct2D::ID2D1SolidColorBrush,
                       sh_m: Option<&windows::Win32::Graphics::Direct2D::ID2D1SolidColorBrush>,
                       sh_off_m: f32| {
        let raw = windows::core::Interface::as_raw(tf_m);
        let is_badge = Some(raw) == badge_raw;
        let is_title = Some(raw) == title_raw;
        let slot_idx = if is_title || is_badge {
            Some(1usize)
        } else if Some(raw) == art_raw {
            Some(2usize)
        } else {
            None
        };
        if let Some(idx) = slot_idx {
            let mut tr = cfg.music_elem_transforms[idx];
            let elem_vis = if is_badge {
                cfg.music_show_badge
            } else if idx == 1 {
                cfg.music_show_title && tr.visible
            } else if idx == 2 {
                cfg.music_show_artist && tr.visible
            } else {
                tr.visible
            };
            let has_title_box = {
                let b_ref = boxes_cell.borrow();
                b_ref.as_ref().is_some_and(|b| b[idx].visible && b[idx].tr_half_w > 0.0)
            };
            if !elem_vis {
                if !is_badge || !has_title_box {
                    let mut b_ref = boxes_cell.borrow_mut();
                    let mut slice_opt = b_ref.as_deref_mut().map(|b| &mut b[..]);
                    record_elem_box_slot(&mut slice_opt, idx, tr, (rect_m.left, rect_m.top, rect_m.right, rect_m.bottom), s);
                }
                return;
            }
            tr.visible = true;
            let mut b_ref = boxes_cell.borrow_mut();
            let mut slice_opt = if is_badge && has_title_box {
                None
            } else {
                b_ref.as_deref_mut().map(|b| &mut b[..])
            };
            let g = begin_elem_xf_slot(rt_m, idx, tr, (rect_m.left, rect_m.top, rect_m.right, rect_m.bottom), s, &mut slice_opt);
            drop(b_ref);
            let eff_br = match idx {
                1 => br_t_ov.as_ref().unwrap_or(br_m),
                2 => br_a_ov.as_ref().unwrap_or(br_m),
                _ => br_v_ov.as_ref().unwrap_or(br_m),
            };
            draw_m_text(rt_m, tf_m, txt_m, rect_m, eff_br, sh_m, sh_off_m);
            end_elem_xf_slot(rt_m, g);
        } else {
            draw_m_text(rt_m, tf_m, txt_m, rect_m, br_m, sh_m, sh_off_m);
        }
    };

    let draw_music_cover_art = |rt_c: &ID2D1RenderTarget,
                                d2d_c: Option<&ID2D1Factory>,
                                rect_c: D2D_RECT_F,
                                radius_c: f32,
                                bmp_c: Option<&ID2D1Bitmap>,
                                _pal_c: &ThemePalette,
                                bw_c: f32,
                                s_c: f32| {
        if cfg.music_show_cover {
            let mut b_ref = boxes_cell.borrow_mut();
            let mut slice_opt = b_ref.as_deref_mut().map(|b| &mut b[..]);
            let g = begin_elem_xf_slot(rt_c, 0, tr0, (rect_c.left, rect_c.top, rect_c.right, rect_c.bottom), s, &mut slice_opt);
            draw_music_cover_art(rt_c, d2d_c, rect_c, radius_c, bmp_c, &cov_pal, bw_c, s_c);
            end_elem_xf_slot(rt_c, g);
        }
    };
    let draw_music_transport = |rt_t: &ID2D1RenderTarget,
                                d2d_t: Option<&ID2D1Factory>,
                                x_t: f32,
                                y_t: f32,
                                s_t: f32,
                                layout_t: [(f32, f32, f32); 3],
                                rect_t: bool,
                                pal_t: &ThemePalette,
                                media_t: &MediaSnapshot,
                                hov_t: WidgetHitZone,
                                br_c: Option<&windows::Win32::Graphics::Direct2D::ID2D1SolidColorBrush>| {
        if cfg.music_show_controls {
            let mut l = f32::MAX;
            let mut t = f32::MAX;
            let mut r = f32::MIN;
            let mut b = f32::MIN;
            for (cx, cy, rr) in layout_t {
                l = l.min(x_t + (cx - rr) * s_t);
                t = t.min(y_t + (cy - rr) * s_t);
                r = r.max(x_t + (cx + rr) * s_t);
                b = b.max(y_t + (cy + rr) * s_t);
            }
            let mut b_ref = boxes_cell.borrow_mut();
            let mut slice_opt = b_ref.as_deref_mut().map(|bx| &mut bx[..]);
            let g = begin_elem_xf_slot(rt_t, 3, tr3, (l, t, r, b), s, &mut slice_opt);
            let mut eff_pal = *pal_t;
            if let Some(rgb) = tr3.color_rgb {
                let c = crate::theme::u32_to_rgb(rgb);
                eff_pal.accent = c;
                eff_pal.accent_light = c;
                eff_pal.accent_deep = (c.0 / 2, c.1 / 2, c.2 / 2);
            }
            let eff_c = br_c_ov.as_ref().or(br_c);
            draw_music_transport(rt_t, d2d_t, x_t, y_t, s_t, layout_t, rect_t, &eff_pal, media_t, hov_t, eff_c);
            end_elem_xf_slot(rt_t, g);
        }
    };

    match style {
        MusicStyle::TurntablePro => {
            // Right Walnut/Obsidian Plinth Slab leaving left platter overhang open
            let plinth = D2D_RECT_F {
                left: x + 52.0 * s,
                top: y + 10.0 * s,
                right: x + w - 4.0 * s,
                bottom: y + h - 8.0 * s,
            };
            draw_music_plate_rr(rt, plinth, 14.0 * s, cfg, pal, is_card_hov, 1.08, 1.2 * s);

            // Protruding 12" LP Platter + Circular Cover Art + Machined Tonearm Assembly (all in Cover slot 0!)
            if cfg.music_show_cover {
                let vc = Vector2 { X: x + 56.0 * s, Y: y + 57.0 * s };
                let pr = 50.0 * s;
                let plat_box = (vc.X - pr, vc.Y - pr, x + 123.0 * s, vc.Y + pr);
                let mut b_ref = boxes_cell.borrow_mut();
                let mut slice_opt = b_ref.as_deref_mut().map(|bx| &mut bx[..]);
                let g_cov = begin_elem_xf_slot(rt, 0, tr0, plat_box, s, &mut slice_opt);
                drop(b_ref);

                if let Ok(plat) = rt.CreateSolidColorBrush(&rgba_u8(16, 15, 16, 0.96), None) {
                    rt.FillEllipse(&D2D1_ELLIPSE { point: vc, radiusX: pr, radiusY: pr }, &plat);
                }
                let rim_c = elem_col_rgba(tr0.border_rgb.or(tr0.color_rgb), 0.78, cov_pal.accent_col(0.58));
                if let Ok(rim) = rt.CreateSolidColorBrush(&rim_c, None) {
                    rt.DrawEllipse(&D2D1_ELLIPSE { point: vc, radiusX: pr, radiusY: pr }, &rim, 1.4 * s, None);
                }
                if let Ok(gr) = rt.CreateSolidColorBrush(&cov_pal.text_col(0.14), None) {
                    for r_step in [45.0, 39.0, 33.0, 27.0] {
                        rt.DrawEllipse(&D2D1_ELLIPSE { point: vc, radiusX: r_step * s, radiusY: r_step * s }, &gr, 0.85 * s, None);
                    }
                }
                // Subtle glossy specular vinyl sheen wedges rotating subtly when playing
                let sheen_deg = -40.0 + anim_t * 28.0;
                if let Ok(sh_br) = rt.CreateSolidColorBrush(&rgba_u8(255, 255, 255, 0.16), None) {
                    let b: &ID2D1Brush = &sh_br;
                    draw_music_arc(rt, d2d, vc.X, vc.Y, 41.0 * s, sheen_deg, 32.0, 2.2 * s, b, cap_round.as_ref());
                    draw_music_arc(rt, d2d, vc.X, vc.Y, 41.0 * s, sheen_deg + 180.0, 32.0, 2.2 * s, b, cap_round.as_ref());
                }
                // Circular Cover Center Label
                let cov_r = 22.0 * s;
                let cov_rect = D2D_RECT_F {
                    left: vc.X - cov_r,
                    top: vc.Y - cov_r,
                    right: vc.X + cov_r,
                    bottom: vc.Y + cov_r,
                };
                self::draw_music_cover_art(rt, d2d, cov_rect, cov_r, cover_bmp, &cov_pal, 1.2, s);
                if let Ok(sp) = rt.CreateSolidColorBrush(&rgba_u8(230, 224, 210, 0.92), None) {
                    rt.FillEllipse(&D2D1_ELLIPSE { point: vc, radiusX: 3.2 * s, radiusY: 3.2 * s }, &sp);
                }

                // Realistic Machined Tonearm Assembly pivoted top-middle onto record groove
                let piv = Vector2 { X: x + 114.0 * s, Y: y + 20.0 * s };
                let tip_sway = if has_audio && media.is_playing { (anim_t * 2.5).sin() * 1.2 * s } else { 0.0 };
                let elbow = Vector2 { X: x + 109.0 * s, Y: y + 52.0 * s };
                let tip = Vector2 { X: x + 91.0 * s + tip_sway, Y: y + 67.0 * s };
                let arm_c = elem_col_rgba(tr0.color2_rgb.or(tr0.color_rgb), 0.92, cov_pal.light_col(0.88));
                if let Ok(arm_br) = rt.CreateSolidColorBrush(&arm_c, None) {
                    rt.FillEllipse(&D2D1_ELLIPSE { point: piv, radiusX: 6.0 * s, radiusY: 6.0 * s }, &arm_br);
                    let arm_pts = [Vector2 { X: x + 116.5 * s, Y: y + 13.0 * s }, piv, elbow, tip];
                    draw_polyline(rt, d2d, &arm_pts, false, None, Some(&arm_br), 2.1 * s, cap_round.as_ref());
                    rt.FillRoundedRectangle(
                        &D2D1_ROUNDED_RECT { rect: D2D_RECT_F { left: tip.X - 3.0 * s, top: tip.Y - 2.0 * s, right: tip.X + 3.0 * s, bottom: tip.Y + 4.5 * s }, radiusX: 1.5 * s, radiusY: 1.5 * s },
                        &arm_br,
                    );
                }
                end_elem_xf_slot(rt, g_cov);
            }

            // Right Typography + Subtle Groove EQ
            if let (Some(tf), Some(br)) = (&tf_badge, &br_amber) {
                draw_m_text(rt, tf, "33⅓ RPM  •  DIRECT DRIVE", D2D_RECT_F { left: x + 128.0 * s, top: y + 16.0 * s, right: x + 398.0 * s, bottom: y + 32.0 * s }, br, br_sh.as_ref(), 0.9 * s);
            }
            if let (Some(tf), Some(br)) = (&tf_title, &br_cream) {
                draw_m_text(rt, tf, &media.title, D2D_RECT_F { left: x + 128.0 * s, top: y + 31.0 * s, right: x + 398.0 * s, bottom: y + 53.0 * s }, br, br_sh.as_ref(), 1.0 * s);
            }
            if let (Some(tf), Some(br)) = (&tf_art, &br_sand) {
                draw_m_text(rt, tf, &media.artist, D2D_RECT_F { left: x + 128.0 * s, top: y + 51.0 * s, right: x + 398.0 * s, bottom: y + 68.0 * s }, br, br_sh.as_ref(), 0.9 * s);
            }
            if cfg.music_show_viz {
                with_viz_slot!(x + 128.0 * s, y + 71.0 * s, x + 275.0 * s, y + 93.0 * s, {
                    let (vc1, vc2, vang) = resolve_elem_grad_colors(Some(&tr4), if has_audio { 0.88 } else { 0.36 }, viz_pal.accent_col(if has_audio { 0.88 } else { 0.36 }), viz_pal.light_col(if has_audio { 0.88 } else { 0.36 }));
                    let (gp0, gp1) = grad_endpoints_for_rect((x + 128.0 * s, y + 71.0 * s, x + 275.0 * s, y + 93.0 * s), vang);
                    let vg_opt = create_linear_grad(rt, gp0, gp1, &[D2D1_GRADIENT_STOP { position: 0.0, color: vc1 }, D2D1_GRADIENT_STOP { position: 1.0, color: vc2 }]);
                    for i in 0..14 {
                        let bx = x + (128.0 + i as f32 * 10.5) * s;
                        let bh = if has_audio { (3.5 + eff_bands[i % 12] * 14.0).min(17.0) } else { 2.8 };
                        let rrb = D2D1_ROUNDED_RECT { rect: D2D_RECT_F { left: bx, top: y + (82.0 - bh * 0.5) * s, right: bx + 4.0 * s, bottom: y + (82.0 + bh * 0.5) * s }, radiusX: 2.0 * s, radiusY: 2.0 * s };
                        if let Some(ref vg) = vg_opt {
                            rt.FillRoundedRectangle(&rrb, vg);
                        }
                    }
                });
            }
            draw_music_transport(rt, d2d, x, y, s, layout, false, pal, media, hover, br_cream.as_ref());
        }

        MusicStyle::DualFloatOrb => {
            // Left Floating Cover Medallion + Outer 24-Bar Radial Pulse Ring
            let oc = Vector2 { X: x + 53.0 * s, Y: y + 53.0 * s };
            draw_music_plate_circle(rt, oc.X, oc.Y, 48.0 * s, cfg, pal, is_card_hov, 0.95, 1.0 * s);
            if cfg.music_show_viz {
                with_viz_slot!(oc.X - 46.0 * s, oc.Y - 46.0 * s, oc.X + 46.0 * s, oc.Y + 46.0 * s, {
                    let va = if has_audio { 0.94 } else { 0.42 };
                    let (vc1, vc2, _) = resolve_elem_grad_colors(Some(&tr4), va, viz_pal.light_col(va), viz_pal.accent_col(va));
                    for i in 0..24 {
                        let a = (i as f32 * 15.0 - 90.0).to_radians();
                        let r_in = 36.5 * s;
                        let len = if has_audio { 3.0 + eff_bands[i % 12] * 9.5 } else { 2.5 };
                        let r_out = r_in + len * s;
                        let col = if i % 2 == 0 { vc1 } else { vc2 };
                        if let Ok(rb) = rt.CreateSolidColorBrush(&col, None) {
                            rt.DrawLine(
                                Vector2 { X: oc.X + r_in * a.cos(), Y: oc.Y + r_in * a.sin() },
                                Vector2 { X: oc.X + r_out * a.cos(), Y: oc.Y + r_out * a.sin() },
                                &rb,
                                if has_audio { 2.1 * s } else { 1.4 * s },
                                cap_round.as_ref(),
                            );
                        }
                    }
                });
            }
            draw_music_cover_art(
                rt,
                d2d,
                D2D_RECT_F { left: oc.X - 33.0 * s, top: oc.Y - 33.0 * s, right: oc.X + 33.0 * s, bottom: oc.Y + 33.0 * s },
                33.0 * s,
                cover_bmp,
                pal,
                1.3,
                s,
            );

            // Detached Floating Aerodynamic Console Capsule on Right
            let cap_rect = D2D_RECT_F { left: x + 110.0 * s, top: y + 12.0 * s, right: x + w - 4.0 * s, bottom: y + h - 12.0 * s };
            draw_music_plate_rr(rt, cap_rect, 41.0 * s, cfg, pal, is_card_hov, 1.04, 1.15 * s);
            if let (Some(tf), Some(br)) = (&tf_title, &br_cream) {
                draw_m_text(rt, tf, &media.title, D2D_RECT_F { left: x + 132.0 * s, top: y + 22.0 * s, right: x + 286.0 * s, bottom: y + 44.0 * s }, br, br_sh.as_ref(), 1.0 * s);
            }
            if let (Some(tf), Some(br)) = (&tf_art, &br_sand) {
                draw_m_text(rt, tf, &media.artist, D2D_RECT_F { left: x + 132.0 * s, top: y + 43.0 * s, right: x + 286.0 * s, bottom: y + 61.0 * s }, br, br_sh.as_ref(), 0.9 * s);
            }
            if cfg.music_show_viz && tr4.visible {
                let fy = y + 71.0 * s;
                let mut nb: Option<&mut [ClockElemBox]> = None;
                let g_v2 = begin_elem_xf_slot(rt, 4, tr4, (oc.X - 46.0 * s, oc.Y - 46.0 * s, oc.X + 46.0 * s, oc.Y + 46.0 * s), s, &mut nb);
                let tr_col = elem_col_rgba(tr4.border_rgb, 0.34, pal.text_col(0.16));
                if let Ok(tr) = rt.CreateSolidColorBrush(&tr_col, None) {
                    rt.DrawLine(Vector2 { X: x + 132.0 * s, Y: fy }, Vector2 { X: x + 276.0 * s, Y: fy }, &tr, 1.6 * s, cap_round.as_ref());
                }
                let amp: f32 = if has_audio { (eff_bands.iter().sum::<f32>() / 12.0).clamp(0.25, 1.0) } else { 0.30 };
                let fw = 144.0 * amp * s;
                let fa = if has_audio { 0.94 } else { 0.48 };
                let (fc1, fc2, fang) = resolve_elem_grad_colors(Some(&tr4), fa, viz_pal.accent_col(fa), viz_pal.light_col(fa));
                let (gp0, gp1) = grad_endpoints_for_rect((x + 132.0 * s, fy - 2.0 * s, x + 132.0 * s + fw, fy + 2.0 * s), fang);
                if let Some(fg) = create_linear_grad(rt, gp0, gp1, &[D2D1_GRADIENT_STOP { position: 0.0, color: fc1 }, D2D1_GRADIENT_STOP { position: 1.0, color: fc2 }]) {
                    let fb: &ID2D1Brush = &fg;
                    rt.DrawLine(Vector2 { X: x + 132.0 * s, Y: fy }, Vector2 { X: x + 132.0 * s + fw, Y: fy }, fb, 2.4 * s, cap_round.as_ref());
                }
                end_elem_xf_slot(rt, g_v2);
            }
            draw_music_transport(rt, d2d, x, y, s, layout, false, pal, media, hover, br_cream.as_ref());
        }

        MusicStyle::EditorialSleeve => {
            // 1. Right Gallery Plaque
            if !skip_plate_and_fx {
                draw_music_plate_rr(
                    rt,
                    D2D_RECT_F { left: x + 82.0 * s, top: y + 14.0 * s, right: x + w - 4.0 * s, bottom: y + h - 14.0 * s },
                    14.0 * s,
                    cfg,
                    pal,
                    is_card_hov,
                    1.05,
                    1.1 * s,
                );
            }
            // 2. Protruding Grooved 12" Black LP peeking halfway right behind Jacket + Front Jacket (all in Cover slot 0!)
            if cfg.music_show_cover {
                let vc = Vector2 { X: x + 76.0 * s, Y: y + 53.0 * s };
                let cov_box = (x + 8.0 * s, y + 11.0 * s, vc.X + 39.0 * s, y + 95.0 * s);
                let mut b_ref = boxes_cell.borrow_mut();
                let mut slice_opt = b_ref.as_deref_mut().map(|bx| &mut bx[..]);
                let g_cov = begin_elem_xf_slot(rt, 0, tr0, cov_box, s, &mut slice_opt);
                drop(b_ref);

                if let Ok(lp) = rt.CreateSolidColorBrush(&rgba_u8(16, 14, 14, 0.96), None) {
                    rt.FillEllipse(&D2D1_ELLIPSE { point: vc, radiusX: 39.0 * s, radiusY: 39.0 * s }, &lp);
                }
                if let Some(b_rgb) = tr0.border_rgb {
                    if let Ok(lrim) = rt.CreateSolidColorBrush(&elem_col_rgba(Some(b_rgb), 0.78, cov_pal.accent_col(0.65)), None) {
                        rt.DrawEllipse(&D2D1_ELLIPSE { point: vc, radiusX: 39.0 * s, radiusY: 39.0 * s }, &lrim, 1.3 * s, None);
                    }
                }
                if let Ok(gr) = rt.CreateSolidColorBrush(&cov_pal.text_col(0.20), None) {
                    for r_step in [36.0, 31.0, 26.0] {
                        rt.DrawEllipse(&D2D1_ELLIPSE { point: vc, radiusX: r_step * s, radiusY: r_step * s }, &gr, 0.85 * s, None);
                    }
                }
                if let Ok(lbl) = rt.CreateSolidColorBrush(&cov_pal.accent_col(0.85), None) {
                    rt.FillEllipse(&D2D1_ELLIPSE { point: vc, radiusX: 14.0 * s, radiusY: 14.0 * s }, &lbl);
                }
                // 3. Square Record Jacket in Front
                self::draw_music_cover_art(
                    rt,
                    d2d,
                    D2D_RECT_F { left: x + 8.0 * s, top: y + 11.0 * s, right: x + 92.0 * s, bottom: y + 95.0 * s },
                    5.0 * s,
                    cover_bmp,
                    &cov_pal,
                    1.4,
                    s,
                );
                end_elem_xf_slot(rt, g_cov);
            }

            if let (Some(tf), Some(br)) = (&tf_badge, &br_amber) {
                draw_m_text(rt, tf, "EDITION  //  LP SLEEVE", D2D_RECT_F { left: x + 124.0 * s, top: y + 20.0 * s, right: x + 286.0 * s, bottom: y + 34.0 * s }, br, br_sh.as_ref(), 0.8 * s);
            }
            if let (Some(tf), Some(br)) = (&tf_title, &br_cream) {
                draw_m_text(rt, tf, &media.title, D2D_RECT_F { left: x + 124.0 * s, top: y + 34.0 * s, right: x + 286.0 * s, bottom: y + 55.0 * s }, br, br_sh.as_ref(), 1.0 * s);
            }
            if let (Some(tf), Some(br)) = (&tf_art, &br_sand) {
                draw_m_text(rt, tf, &media.artist, D2D_RECT_F { left: x + 124.0 * s, top: y + 54.0 * s, right: x + 286.0 * s, bottom: y + 71.0 * s }, br, br_sh.as_ref(), 0.9 * s);
            }
            if cfg.music_show_viz {
                with_viz_slot!(x + 125.0 * s, y + 71.0 * s, x + 215.0 * s, y + 85.0 * s, {
                    for i in 0..10 {
                        let bx = x + (125.0 + i as f32 * 9.0) * s;
                        let bh = if has_audio { (3.0 + eff_bands[i] * 8.0).min(11.0) } else { 2.2 };
                        if let Ok(b) = rt.CreateSolidColorBrush(&viz_pal.accent_col(if has_audio { 0.88 } else { 0.36 }), None) {
                            rt.FillRoundedRectangle(
                                &D2D1_ROUNDED_RECT { rect: D2D_RECT_F { left: bx, top: y + (78.0 - bh * 0.5) * s, right: bx + 3.5 * s, bottom: y + (78.0 + bh * 0.5) * s }, radiusX: 1.5 * s, radiusY: 1.5 * s },
                                &b,
                            );
                        }
                    }
                });
            }
            draw_music_transport(rt, d2d, x, y, s, layout, false, pal, media, hover, br_cream.as_ref());
        }

        MusicStyle::RadialHalo => {
            // Slim Frosted Console Extension Behind Open 360 Radial Bursts
            draw_music_plate_rr(
                rt,
                D2D_RECT_F { left: x + 56.0 * s, top: y + 22.0 * s, right: x + w - 4.0 * s, bottom: y + h - 22.0 * s },
                36.0 * s,
                cfg,
                pal,
                is_card_hov,
                1.02,
                1.1 * s,
            );

            let rc_pt = Vector2 { X: x + 58.0 * s, Y: y + 58.0 * s };
            if cfg.music_show_viz {
                with_viz_slot!(rc_pt.X - 52.0 * s, rc_pt.Y - 52.0 * s, rc_pt.X + 52.0 * s, rc_pt.Y + 52.0 * s, {
                    let ring_c = elem_col_rgba(tr4.border_rgb.or(tr4.color_rgb), if has_audio { 0.48 } else { 0.22 }, viz_pal.accent_col(if has_audio { 0.42 } else { 0.18 }));
                    if let Ok(orb) = rt.CreateSolidColorBrush(&ring_c, None) {
                        rt.DrawEllipse(&D2D1_ELLIPSE { point: rc_pt, radiusX: 43.0 * s, radiusY: 43.0 * s }, &orb, 1.0 * s, None);
                    }
                    let va1 = if has_audio { 0.94 } else { 0.40 };
                    let va2 = if has_audio { 0.84 } else { 0.34 };
                    let (vc1, vc2, _) = resolve_elem_grad_colors(Some(&tr4), va1, viz_pal.light_col(va1), viz_pal.accent_col(va2));
                    for i in 0..32 {
                        let ang = (i as f32 * 11.25 - 90.0).to_radians();
                        let r0 = 35.5 * s;
                        let reach = if has_audio { 4.0 + eff_bands[i % 12] * 16.5 } else { 3.2 };
                        let r1 = r0 + reach * s;
                        let col = if i % 2 == 0 { vc1 } else { vc2 };
                        if let Ok(rb) = rt.CreateSolidColorBrush(&col, None) {
                            rt.DrawLine(
                                Vector2 { X: rc_pt.X + r0 * ang.cos(), Y: rc_pt.Y + r0 * ang.sin() },
                                Vector2 { X: rc_pt.X + r1 * ang.cos(), Y: rc_pt.Y + r1 * ang.sin() },
                                &rb,
                                2.0 * s,
                                cap_round.as_ref(),
                            );
                        }
                    }
                });
            }
            draw_music_cover_art(
                rt,
                d2d,
                D2D_RECT_F { left: rc_pt.X - 31.0 * s, top: rc_pt.Y - 31.0 * s, right: rc_pt.X + 31.0 * s, bottom: rc_pt.Y + 31.0 * s },
                31.0 * s,
                cover_bmp,
                pal,
                1.5,
                s,
            );
            if let (Some(tf), Some(br)) = (&tf_badge, &br_amber) {
                draw_m_text(rt, tf, "360° HALO AUDIO", D2D_RECT_F { left: x + 122.0 * s, top: y + 29.0 * s, right: x + 276.0 * s, bottom: y + 43.0 * s }, br, br_sh.as_ref(), 0.8 * s);
            }
            if let (Some(tf), Some(br)) = (&tf_title, &br_cream) {
                draw_m_text(rt, tf, &media.title, D2D_RECT_F { left: x + 122.0 * s, top: y + 43.0 * s, right: x + 276.0 * s, bottom: y + 65.0 * s }, br, br_sh.as_ref(), 1.0 * s);
            }
            if let (Some(tf), Some(br)) = (&tf_art, &br_sand) {
                draw_m_text(rt, tf, &media.artist, D2D_RECT_F { left: x + 122.0 * s, top: y + 64.0 * s, right: x + 276.0 * s, bottom: y + 83.0 * s }, br, br_sh.as_ref(), 0.9 * s);
            }
            draw_music_transport(rt, d2d, x, y, s, layout, false, pal, media, hover, br_cream.as_ref());
        }

        MusicStyle::OscilloscopeCrt => {
            draw_music_plate_rr(rt, D2D_RECT_F { left: x, top: y, right: x + w, bottom: y + h }, 14.0 * s, cfg, pal, is_card_hov, 1.12, 1.25 * s);
            draw_music_cover_art(
                rt,
                d2d,
                D2D_RECT_F { left: x + 12.0 * s, top: y + 18.0 * s, right: x + 80.0 * s, bottom: y + 86.0 * s },
                7.0 * s,
                cover_bmp,
                pal,
                1.3,
                s,
            );
            if let (Some(tf), Some(br)) = (&tf_badge, &br_amber) {
                draw_m_text(rt, tf, "CRT // HARMONIC SCOPE", D2D_RECT_F { left: x + 94.0 * s, top: y + 14.0 * s, right: x + 298.0 * s, bottom: y + 29.0 * s }, br, br_sh.as_ref(), 0.8 * s);
            }
            if let (Some(tf), Some(br)) = (&tf_title, &br_cream) {
                draw_m_text(rt, tf, &media.title, D2D_RECT_F { left: x + 94.0 * s, top: y + 28.0 * s, right: x + 298.0 * s, bottom: y + 48.0 * s }, br, br_sh.as_ref(), 1.0 * s);
            }
            if let (Some(tf), Some(br)) = (&tf_art, &br_sand) {
                draw_m_text(rt, tf, &media.artist, D2D_RECT_F { left: x + 94.0 * s, top: y + 46.0 * s, right: x + 298.0 * s, bottom: y + 62.0 * s }, br, br_sh.as_ref(), 0.9 * s);
            }
            draw_music_transport(rt, d2d, x, y, s, layout, false, pal, media, hover, br_cream.as_ref());

            if cfg.music_show_badge {
                let badge_rr = D2D_RECT_F { left: x + 216.0 * s, top: y + 68.0 * s, right: x + 294.0 * s, bottom: y + 86.0 * s };
                let mut nb: Option<&mut [ClockElemBox]> = None;
                let mut tr_b = tr1;
                tr_b.visible = true;
                let g_b = begin_elem_xf_slot(rt, 1, tr_b, (badge_rr.left, badge_rr.top, badge_rr.right, badge_rr.bottom), s, &mut nb);
                let (pill_c1, pill_c2, pill_stop) = resolve_elem_grad_colors(
                    Some(&tr1),
                    if has_audio { 0.24 } else { 0.08 },
                    pal.accent_col(if has_audio { 0.24 } else { 0.08 }),
                    pal.light_col(if has_audio { 0.16 } else { 0.06 }),
                );
                if tr1.color2_rgb.is_some() {
                    let stops = [
                        D2D1_GRADIENT_STOP { position: 0.0, color: pill_c1 },
                        D2D1_GRADIENT_STOP { position: (pill_stop as f32 / 100.0).clamp(0.05, 0.95), color: pill_c1 },
                        D2D1_GRADIENT_STOP { position: 1.0, color: pill_c2 },
                    ];
                    if let Some(sb) = create_linear_grad(rt, (badge_rr.left, badge_rr.top), (badge_rr.right, badge_rr.bottom), &stops) {
                        rt.FillRoundedRectangle(&D2D1_ROUNDED_RECT { rect: badge_rr, radiusX: 5.0 * s, radiusY: 5.0 * s }, &sb);
                    }
                } else if let Ok(sb) = rt.CreateSolidColorBrush(&pill_c1, None) {
                    rt.FillRoundedRectangle(&D2D1_ROUNDED_RECT { rect: badge_rr, radiusX: 5.0 * s, radiusY: 5.0 * s }, &sb);
                }
                if let Some(br_rgb) = tr1.border_rgb {
                    if let Ok(rb) = rt.CreateSolidColorBrush(&elem_col_rgba(Some(br_rgb), 0.72, pal.accent_col(0.55)), None) {
                        rt.DrawRoundedRectangle(&D2D1_ROUNDED_RECT { rect: badge_rr, radiusX: 5.0 * s, radiusY: 5.0 * s }, &rb, 1.0 * s, None);
                    }
                }
                end_elem_xf_slot(rt, g_b);
            }
            if let (Some(tf), Some(br)) = (&tf_badge, if has_audio { &br_gold } else { &br_sand }) {
                draw_m_text(rt, tf, if has_audio { "LIVE WAVE" } else { "IDLE" }, D2D_RECT_F { left: x + 224.0 * s, top: y + 68.0 * s, right: x + 290.0 * s, bottom: y + 86.0 * s }, br, None, 0.0);
            }

            // Right Circular CRT Polar Oscilloscope (slot 4: Prog/EQ)
            let sc_pt = Vector2 { X: x + 354.0 * s, Y: y + 53.0 * s };
            let sc_r = 40.0 * s;
            with_viz_slot!(sc_pt.X - sc_r, sc_pt.Y - sc_r, sc_pt.X + sc_r, sc_pt.Y + sc_r, {
                let crt_col = if tr4.color_rgb.is_some() || cfg.music_elem_transforms[5].color_rgb.is_some() || cfg.music_use_custom_color {
                    viz_pal.glass_bot_col(0.88)
                } else {
                    rgba_u8(10, 16, 14, 0.94)
                };
                if let Ok(crt_bg) = rt.CreateSolidColorBrush(&crt_col, None) {
                    rt.FillEllipse(&D2D1_ELLIPSE { point: sc_pt, radiusX: sc_r, radiusY: sc_r }, &crt_bg);
                }
                if let Ok(gr) = rt.CreateSolidColorBrush(&elem_col_rgba(tr4.color2_rgb, 0.24, viz_pal.text_col(0.18)), None) {
                    rt.DrawLine(Vector2 { X: sc_pt.X - sc_r + 4.0 * s, Y: sc_pt.Y }, Vector2 { X: sc_pt.X + sc_r - 4.0 * s, Y: sc_pt.Y }, &gr, 0.8 * s, None);
                    rt.DrawLine(Vector2 { X: sc_pt.X, Y: sc_pt.Y - sc_r + 4.0 * s }, Vector2 { X: sc_pt.X, Y: sc_pt.Y + sc_r - 4.0 * s }, &gr, 0.8 * s, None);
                    rt.DrawEllipse(&D2D1_ELLIPSE { point: sc_pt, radiusX: 25.0 * s, radiusY: 25.0 * s }, &gr, 0.7 * s, None);
                }
                let rim_c = elem_col_rgba(tr4.border_rgb, 0.74, viz_pal.accent_col(0.58));
                if let Ok(rim) = rt.CreateSolidColorBrush(&rim_c, None) {
                    rt.DrawEllipse(&D2D1_ELLIPSE { point: sc_pt, radiusX: sc_r, radiusY: sc_r }, &rim, 1.4 * s, None);
                }
                let n_pts = 48usize;
                let mut ring_pts = Vec::with_capacity(n_pts);
                for i in 0..n_pts {
                    let th = (i as f32 / n_pts as f32) * std::f32::consts::TAU;
                    let band = eff_bands[(i * 12) / n_pts];
                    let mod_r = if has_audio && cfg.music_show_viz {
                        20.0 + band * 14.0 + (th * 4.0 + anim_t * 4.0).sin() * 4.0
                    } else {
                        20.0
                    };
                    ring_pts.push(Vector2 {
                        X: sc_pt.X + mod_r * s * th.cos(),
                        Y: sc_pt.Y + mod_r * s * th.sin(),
                    });
                }
                let (w_c1, w_c2, _w_stop) = resolve_elem_grad_colors(
                    Some(&tr4),
                    if has_audio { 0.94 } else { 0.42 },
                    viz_pal.light_col(if has_audio { 0.94 } else { 0.42 }),
                    viz_pal.accent_col(if has_audio { 0.92 } else { 0.40 }),
                );
                if let Some(gl_rgb) = tr4.glow_rgb {
                    if let Ok(gl_b) = rt.CreateSolidColorBrush(&elem_col_rgba(Some(gl_rgb), 0.34, w_c1), None) {
                        let sb: &ID2D1Brush = &gl_b;
                        draw_polyline(rt, d2d, &ring_pts, true, None, Some(sb), 4.6 * s, cap_round.as_ref());
                    }
                }
                if tr4.color2_rgb.is_some() {
                    let stops = [
                        D2D1_GRADIENT_STOP { position: 0.0, color: w_c1 },
                        D2D1_GRADIENT_STOP { position: 1.0, color: w_c2 },
                    ];
                    if let Some(gb) = create_linear_grad(rt, (sc_pt.X - sc_r, sc_pt.Y - sc_r), (sc_pt.X + sc_r, sc_pt.Y + sc_r), &stops) {
                        let sb: &ID2D1Brush = &gb;
                        draw_polyline(rt, d2d, &ring_pts, true, None, Some(sb), 1.9 * s, cap_round.as_ref());
                    }
                } else if let Ok(glow_w) = rt.CreateSolidColorBrush(&w_c1, None) {
                    let sb: &ID2D1Brush = &glow_w;
                    draw_polyline(rt, d2d, &ring_pts, true, None, Some(sb), 1.9 * s, cap_round.as_ref());
                }
            });
        }

        MusicStyle::RibbonFlow => {
            draw_music_plate_rr(rt, D2D_RECT_F { left: x, top: y, right: x + w, bottom: y + h }, 48.0 * s, cfg, pal, is_card_hov, 0.96, 1.15 * s);
            let cov_pt = Vector2 { X: x + 46.0 * s, Y: y + 48.0 * s };
            if cfg.music_show_cover {
                let mut b_ref = boxes_cell.borrow_mut();
                let mut slice_opt = b_ref.as_deref_mut().map(|bx| &mut bx[..]);
                let g_c = begin_elem_xf_slot(rt, 0, tr0, (cov_pt.X - 40.0 * s, cov_pt.Y - 40.0 * s, cov_pt.X + 40.0 * s, cov_pt.Y + 40.0 * s), s, &mut slice_opt);
                drop(b_ref);
                if tr0.visible {
                    let arc_col = elem_col_rgba(tr0.border_rgb.or(tr0.color_rgb), if has_audio { 0.85 } else { 0.35 }, cov_pal.accent_col(if has_audio { 0.85 } else { 0.35 }));
                    if let Ok(ring) = rt.CreateSolidColorBrush(&arc_col, None) {
                        let b: &ID2D1Brush = &ring;
                        draw_music_arc(rt, d2d, cov_pt.X, cov_pt.Y, 40.0 * s, -90.0, if has_audio { 265.0 } else { 140.0 }, 2.4 * s, b, cap_round.as_ref());
                    }
                    self::draw_music_cover_art(
                        rt,
                        d2d,
                        D2D_RECT_F { left: cov_pt.X - 35.0 * s, top: cov_pt.Y - 35.0 * s, right: cov_pt.X + 35.0 * s, bottom: cov_pt.Y + 35.0 * s },
                        35.0 * s,
                        cover_bmp,
                        &cov_pal,
                        1.2,
                        s,
                    );
                }
                end_elem_xf_slot(rt, g_c);
            }
            if let (Some(tf), Some(br)) = (&tf_title, &br_cream) {
                draw_m_text(rt, tf, &media.title, D2D_RECT_F { left: x + 98.0 * s, top: y + 14.0 * s, right: x + 292.0 * s, bottom: y + 36.0 * s }, br, br_sh.as_ref(), 1.0 * s);
            }
            if let (Some(tf), Some(br)) = (&tf_art, &br_sand) {
                draw_m_text(rt, tf, &media.artist, D2D_RECT_F { left: x + 98.0 * s, top: y + 34.0 * s, right: x + 292.0 * s, bottom: y + 51.0 * s }, br, br_sh.as_ref(), 0.9 * s);
            }
            let wave_y = y + 70.0 * s;
            with_viz_slot!(x + 98.0 * s, wave_y - 17.0 * s, x + 290.0 * s, wave_y + 17.0 * s, {
                let ax_col = elem_col_rgba(tr4.border_rgb, 0.32, pal.text_col(0.18));
                if let Ok(ax) = rt.CreateSolidColorBrush(&ax_col, None) {
                    rt.DrawLine(Vector2 { X: x + 98.0 * s, Y: wave_y }, Vector2 { X: x + 290.0 * s, Y: wave_y }, &ax, 0.9 * s, None);
                }
                if cfg.music_show_viz {
                    let (vc1, vc2, _vstop) = resolve_elem_grad_colors(
                        Some(&tr4),
                        if has_audio { 0.88 } else { 0.36 },
                        viz_pal.light_col(if has_audio { 0.88 } else { 0.36 }),
                        viz_pal.accent_col(if has_audio { 0.88 } else { 0.36 }),
                    );
                    for i in 0..24 {
                        let dist = (i as f32 - 11.5).abs() / 11.5;
                        let env = 1.0 - dist * 0.45;
                        let amp = if has_audio { (3.0 + eff_bands[i % 12] * 14.5 * env).min(17.0) } else { 2.0 };
                        let bx = x + (99.0 + i as f32 * 8.0) * s;
                        let bar_rr = D2D1_ROUNDED_RECT { rect: D2D_RECT_F { left: bx, top: wave_y - amp * s, right: bx + 3.4 * s, bottom: wave_y + amp * s }, radiusX: 1.7 * s, radiusY: 1.7 * s };
                        if tr4.color2_rgb.is_some() {
                            let stops = [
                                D2D1_GRADIENT_STOP { position: 0.0, color: vc1 },
                                D2D1_GRADIENT_STOP { position: 1.0, color: vc2 },
                            ];
                            if let Some(wb) = create_linear_grad(rt, (bx, wave_y - amp * s), (bx, wave_y + amp * s), &stops) {
                                rt.FillRoundedRectangle(&bar_rr, &wb);
                            }
                        } else if let Ok(wb) = rt.CreateSolidColorBrush(&vc1, None) {
                            rt.FillRoundedRectangle(&bar_rr, &wb);
                        }
                    }
                }
            });
            draw_music_transport(rt, d2d, x, y, s, layout, false, pal, media, hover, br_cream.as_ref());
        }

        MusicStyle::EclipsePlayer => {
            draw_music_plate_rr(
                rt,
                D2D_RECT_F { left: x + 6.0 * s, top: y + 22.0 * s, right: x + w - 6.0 * s, bottom: y + h - 22.0 * s },
                34.0 * s,
                cfg,
                pal,
                is_card_hov,
                1.0,
                1.1 * s,
            );
            if let (Some(tf), Some(br)) = (&tf_title, &br_cream) {
                draw_m_text(rt, tf, &media.title, D2D_RECT_F { left: x + 20.0 * s, top: y + 28.0 * s, right: x + 154.0 * s, bottom: y + 50.0 * s }, br, br_sh.as_ref(), 1.0 * s);
            }
            if let (Some(tf), Some(br)) = (&tf_art, &br_sand) {
                draw_m_text(rt, tf, &media.artist, D2D_RECT_F { left: x + 20.0 * s, top: y + 48.0 * s, right: x + 154.0 * s, bottom: y + 66.0 * s }, br, br_sh.as_ref(), 0.9 * s);
            }
            if cfg.music_show_viz {
                with_viz_slot!(x + 21.0 * s, y + 68.0 * s, x + 96.0 * s, y + 82.0 * s, {
                    let (vc1, vc2, _) = resolve_elem_grad_colors(
                        Some(&tr4),
                        if has_audio { 0.88 } else { 0.36 },
                        viz_pal.accent_col(if has_audio { 0.88 } else { 0.36 }),
                        viz_pal.light_col(if has_audio { 0.92 } else { 0.42 }),
                    );
                    for i in 0..10 {
                        let bx = x + (21.0 + i as f32 * 7.2) * s;
                        let bh = if has_audio { (3.0 + eff_bands[i] * 8.0).min(11.0) } else { 2.2 };
                        let r_bar = D2D1_ROUNDED_RECT { rect: D2D_RECT_F { left: bx, top: y + (75.0 - bh * 0.5) * s, right: bx + 3.0 * s, bottom: y + (75.0 + bh * 0.5) * s }, radiusX: 1.5 * s, radiusY: 1.5 * s };
                        if tr4.color2_rgb.is_some() {
                            let stops = [
                                D2D1_GRADIENT_STOP { position: 0.0, color: vc2 },
                                D2D1_GRADIENT_STOP { position: 1.0, color: vc1 },
                            ];
                            if let Some(b) = create_linear_grad(rt, (bx, y + 70.0 * s), (bx, y + 80.0 * s), &stops) {
                                rt.FillRoundedRectangle(&r_bar, &b);
                            }
                        } else if let Ok(b) = rt.CreateSolidColorBrush(&vc1, None) {
                            rt.FillRoundedRectangle(&r_bar, &b);
                        }
                    }
                });
            }

            let ec = Vector2 { X: x + 204.0 * s, Y: y + 56.0 * s };
            if cfg.music_show_cover {
                let mut b_ref = boxes_cell.borrow_mut();
                let mut slice_opt = b_ref.as_deref_mut().map(|bx| &mut bx[..]);
                let g_c = begin_elem_xf_slot(rt, 0, tr0, (ec.X - 44.0 * s, ec.Y - 44.0 * s, ec.X + 44.0 * s, ec.Y + 44.0 * s), s, &mut slice_opt);
                drop(b_ref);
                if tr0.visible {
                    if has_audio && cfg.music_show_viz {
                        let pulse = eff_bands[2] * 6.0 * s;
                        let halo_col = elem_col_rgba(tr0.glow_rgb.or(tr0.color_rgb), 0.28, cov_pal.accent_col(0.28));
                        if let Ok(halo) = rt.CreateSolidColorBrush(&halo_col, None) {
                            rt.FillEllipse(&D2D1_ELLIPSE { point: ec, radiusX: 46.0 * s + pulse, radiusY: 46.0 * s + pulse }, &halo);
                        }
                    }
                    let cor_col = elem_col_rgba(tr0.border_rgb.or(tr0.color_rgb), 0.72, cov_pal.light_col(0.72));
                    if let Ok(corona) = rt.CreateSolidColorBrush(&cor_col, None) {
                        let b: &ID2D1Brush = &corona;
                        draw_music_arc(rt, d2d, ec.X, ec.Y, 44.0 * s, -65.0, 140.0, 2.5 * s, b, cap_round.as_ref());
                    }
                    self::draw_music_cover_art(
                        rt,
                        d2d,
                        D2D_RECT_F { left: ec.X - 40.0 * s, top: ec.Y - 40.0 * s, right: ec.X + 40.0 * s, bottom: ec.Y + 40.0 * s },
                        40.0 * s,
                        cover_bmp,
                        &cov_pal,
                        1.6,
                        s,
                    );
                }
                end_elem_xf_slot(rt, g_c);
            }
            draw_music_transport(rt, d2d, x, y, s, layout, false, pal, media, hover, br_cream.as_ref());
        }

        MusicStyle::CassetteDeck => {
            let shell = D2D_RECT_F { left: x + 4.0 * s, top: y + 4.0 * s, right: x + w - 4.0 * s, bottom: y + h - 14.0 * s };
            let lbl_r = D2D_RECT_F { left: x + 18.0 * s, top: y + 10.0 * s, right: x + w - 18.0 * s, bottom: y + 42.0 * s };
            draw_music_plate_rr(rt, shell, 10.0 * s, cfg, pal, is_card_hov, 1.16, 1.3 * s);
            if cfg.music_show_plate && tr_card.visible {
                let mut nb: Option<&mut [ClockElemBox]> = None;
                let g_c = begin_elem_xf_slot(rt, 5, tr_card, (x, y, x + w, y + h), s, &mut nb);
                let bridge = [
                    Vector2 { X: x + 64.0 * s, Y: y + h - 14.0 * s },
                    Vector2 { X: x + w - 64.0 * s, Y: y + h - 14.0 * s },
                    Vector2 { X: x + w - 48.0 * s, Y: y + h - 2.0 * s },
                    Vector2 { X: x + 48.0 * s, Y: y + h - 2.0 * s },
                ];
                let br_fill = elem_col_rgba(tr_card.color2_rgb.or(tr_card.color_rgb), 0.90, pal.glass_bot_col(0.90));
                let br_stroke = elem_col_rgba(tr_card.border_rgb.or(tr_card.color_rgb), 0.62, pal.accent_col(0.55));
                if let Ok(br_bg) = rt.CreateSolidColorBrush(&br_fill, None) {
                    if let Ok(br_st) = rt.CreateSolidColorBrush(&br_stroke, None) {
                        draw_polyline(rt, d2d, &bridge, true, Some(&br_bg), Some(&br_st), 1.1 * s, None);
                    }
                }
                let screw_c = elem_col_rgba(tr_card.border_rgb, 0.42, pal.text_col(0.28));
                if let Ok(sc) = rt.CreateSolidColorBrush(&screw_c, None) {
                    for (sx, sy) in [
                        (x + 12.0 * s, y + 12.0 * s),
                        (x + w - 12.0 * s, y + 12.0 * s),
                        (x + 12.0 * s, y + h - 22.0 * s),
                        (x + w - 12.0 * s, y + h - 22.0 * s),
                    ] {
                        rt.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: sx, Y: sy }, radiusX: 2.2 * s, radiusY: 2.2 * s }, &sc);
                    }
                }
                let lbl_fill = elem_col_rgba(tr_card.color_rgb, 0.22, pal.accent_col(0.16));
                if let Ok(lbg) = rt.CreateSolidColorBrush(&lbl_fill, None) {
                    rt.FillRoundedRectangle(&D2D1_ROUNDED_RECT { rect: lbl_r, radiusX: 4.5 * s, radiusY: 4.5 * s }, &lbg);
                }
                let lbl_rim = elem_col_rgba(tr_card.border_rgb.or(tr_card.color_rgb), 0.56, pal.accent_col(0.52));
                if let Ok(lrim) = rt.CreateSolidColorBrush(&lbl_rim, None) {
                    rt.DrawRoundedRectangle(&D2D1_ROUNDED_RECT { rect: lbl_r, radiusX: 4.5 * s, radiusY: 4.5 * s }, &lrim, 0.9 * s, None);
                }
                end_elem_xf_slot(rt, g_c);
            }
            draw_music_cover_art(
                rt,
                d2d,
                D2D_RECT_F { left: lbl_r.left + 3.0 * s, top: lbl_r.top + 3.0 * s, right: lbl_r.left + 29.0 * s, bottom: lbl_r.bottom - 3.0 * s },
                3.0 * s,
                cover_bmp,
                pal,
                0.8,
                s,
            );
            if let (Some(tf), Some(br)) = (&tf_title, &br_cream) {
                draw_m_text(rt, tf, &media.title, D2D_RECT_F { left: lbl_r.left + 36.0 * s, top: lbl_r.top, right: lbl_r.right - 72.0 * s, bottom: lbl_r.top + 19.0 * s }, br, br_sh.as_ref(), 0.8 * s);
            }
            if let (Some(tf), Some(br)) = (&tf_art, &br_sand) {
                draw_m_text(rt, tf, &media.artist, D2D_RECT_F { left: lbl_r.left + 36.0 * s, top: lbl_r.top + 16.0 * s, right: lbl_r.right - 72.0 * s, bottom: lbl_r.bottom }, br, None, 0.0);
            }
            if cfg.music_show_badge {
                let c90_r = D2D_RECT_F { left: lbl_r.right - 72.0 * s, top: lbl_r.top + 6.0 * s, right: lbl_r.right - 8.0 * s, bottom: lbl_r.top + 22.0 * s };
                let tf_c90 = make_resolved_tf_named(
                    dw,
                    custom_fonts,
                    t_fnt,
                    &[w!("Cascadia Mono"), w!("SF Pro Display")],
                    7.5 * PT_TO_PX * s * fs,
                    DWRITE_FONT_WEIGHT_BOLD,
                    DWRITE_TEXT_ALIGNMENT_TRAILING,
                    DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
                );
                if let (Some(tf), Some(br)) = (&tf_c90, br_t_ov.as_ref().or(br_gold.as_ref())) {
                    let mut nb: Option<&mut [ClockElemBox]> = None;
                    let mut tr_b = tr1;
                    tr_b.visible = true;
                    let g_b = begin_elem_xf_slot(rt, 1, tr_b, (c90_r.left, c90_r.top, c90_r.right, c90_r.bottom), s, &mut nb);
                    self::draw_m_text(rt, tf, "C-90 II", c90_r, br, None, 0.0);
                    end_elem_xf_slot(rt, g_b);
                }
            }

            let win_r = D2D_RECT_F { left: x + 22.0 * s, top: y + 48.0 * s, right: x + 252.0 * s, bottom: y + 92.0 * s };
            with_viz_slot!(win_r.left, win_r.top, win_r.right, win_r.bottom, {
                let win_col = if tr4.color_rgb.is_some() || cfg.music_elem_transforms[5].color_rgb.is_some() || cfg.music_use_custom_color {
                    viz_pal.glass_bot_col(0.86)
                } else {
                    rgba_u8(10, 9, 9, 0.90)
                };
                if let Ok(wbg) = rt.CreateSolidColorBrush(&win_col, None) {
                    rt.FillRoundedRectangle(&D2D1_ROUNDED_RECT { rect: win_r, radiusX: 6.0 * s, radiusY: 6.0 * s }, &wbg);
                }
                if let Ok(wrim) = rt.CreateSolidColorBrush(&elem_col_rgba(tr4.border_rgb.or(tr4.color_rgb), 0.45, pal.text_col(0.24)), None) {
                    rt.DrawRoundedRectangle(&D2D1_ROUNDED_RECT { rect: win_r, radiusX: 6.0 * s, radiusY: 6.0 * s }, &wrim, 1.0 * s, None);
                }
                if let Ok(tp) = rt.CreateSolidColorBrush(&elem_col_rgba(tr4.color_rgb, 0.85, rgba_u8(72, 42, 26, 0.85)), None) {
                    rt.DrawLine(Vector2 { X: x + 76.0 * s, Y: y + 84.0 * s }, Vector2 { X: x + 198.0 * s, Y: y + 84.0 * s }, &tp, 2.2 * s, None);
                }
                let (reel_c1, reel_c2, _rs) = resolve_elem_grad_colors(
                    Some(&tr4),
                    0.92,
                    viz_pal.light_col(0.92),
                    viz_pal.accent_col(0.92),
                );
                for rx_off in [76.0f32, 198.0] {
                    let rc_pt = Vector2 { X: x + rx_off * s, Y: y + 70.0 * s };
                    if let Ok(tape) = rt.CreateSolidColorBrush(&elem_col_rgba(tr4.color_rgb, 0.45, rgba_u8(54, 34, 24, 0.92)), None) {
                        rt.FillEllipse(&D2D1_ELLIPSE { point: rc_pt, radiusX: 16.0 * s, radiusY: 16.0 * s }, &tape);
                    }
                    let hub_c = if tr4.color2_rgb.is_some() { reel_c2 } else { reel_c1 };
                    if let Ok(hub) = rt.CreateSolidColorBrush(&hub_c, None) {
                        rt.DrawEllipse(&D2D1_ELLIPSE { point: rc_pt, radiusX: 8.5 * s, radiusY: 8.5 * s }, &hub, 1.4 * s, None);
                        for sp in 0..6 {
                            let a = (sp as f32 * 60.0).to_radians() + anim_t * 2.2;
                            rt.DrawLine(
                                Vector2 { X: rc_pt.X + 3.0 * s * a.cos(), Y: rc_pt.Y + 3.0 * s * a.sin() },
                                Vector2 { X: rc_pt.X + 8.5 * s * a.cos(), Y: rc_pt.Y + 8.5 * s * a.sin() },
                                &hub,
                                1.3 * s,
                                None,
                            );
                        }
                    }
                }
            });
            draw_music_transport(rt, d2d, x, y, s, layout, false, pal, media, hover, br_cream.as_ref());
        }

        MusicStyle::AnalogVuMeter => {
            draw_music_plate_rr(rt, D2D_RECT_F { left: x, top: y, right: x + w, bottom: y + h }, 10.0 * s, cfg, pal, is_card_hov, 1.14, 1.3 * s);
            draw_music_cover_art(
                rt,
                d2d,
                D2D_RECT_F { left: x + 12.0 * s, top: y + 14.0 * s, right: x + 70.0 * s, bottom: y + 72.0 * s },
                6.0 * s,
                cover_bmp,
                pal,
                1.2,
                s,
            );
            if let (Some(tf), Some(br)) = (&tf_title, &br_cream) {
                draw_m_text(rt, tf, &media.title, D2D_RECT_F { left: x + 80.0 * s, top: y + 14.0 * s, right: x + 236.0 * s, bottom: y + 36.0 * s }, br, br_sh.as_ref(), 1.0 * s);
            }
            if let (Some(tf), Some(br)) = (&tf_art, &br_sand) {
                draw_m_text(rt, tf, &media.artist, D2D_RECT_F { left: x + 80.0 * s, top: y + 35.0 * s, right: x + 236.0 * s, bottom: y + 53.0 * s }, br, br_sh.as_ref(), 0.9 * s);
            }
            if let (Some(tf), Some(br)) = (&tf_badge, &br_amber) {
                draw_m_text(rt, tf, "STEREO VU", D2D_RECT_F { left: x + 12.0 * s, top: y + 78.0 * s, right: x + 74.0 * s, bottom: y + 96.0 * s }, br, None, 0.0);
            }
            draw_music_transport(rt, d2d, x, y, s, layout, false, pal, media, hover, br_cream.as_ref());

            let l_lvl: f32 = if has_audio && cfg.music_show_viz { (eff_bands[0..6].iter().sum::<f32>() / 6.0).clamp(0.08, 1.0) } else { 0.02 };
            let r_lvl: f32 = if has_audio && cfg.music_show_viz { (eff_bands[6..12].iter().sum::<f32>() / 6.0).clamp(0.08, 1.0) } else { 0.02 };
            with_viz_slot!(x + 246.0 * s, y + 14.0 * s, x + 402.0 * s, y + 100.0 * s, {
                for (mx, lbl, lvl) in [(246.0f32, "VU L", l_lvl), (327.0f32, "VU R", r_lvl)] {
                    let meter_r = D2D_RECT_F { left: x + mx * s, top: y + 14.0 * s, right: x + (mx + 75.0) * s, bottom: y + 100.0 * s };
                    let vu_col = viz_pal.glass_bot_col(0.90);
                    if let Ok(mbg) = rt.CreateSolidColorBrush(&vu_col, None) {
                        rt.FillRoundedRectangle(&D2D1_ROUNDED_RECT { rect: meter_r, radiusX: 6.0 * s, radiusY: 6.0 * s }, &mbg);
                    }
                    let (vc1, vc2, _) = resolve_elem_grad_colors(
                        Some(&tr4),
                        0.92,
                        viz_pal.light_col(0.88),
                        rgba_u8(236, 86, 68, 0.95),
                    );
                    if let Ok(glow) = rt.CreateSolidColorBrush(&elem_col_rgba(tr4.glow_rgb.or(tr4.color_rgb), 0.20, viz_pal.accent_col(0.18)), None) {
                        rt.FillRoundedRectangle(
                            &D2D1_ROUNDED_RECT { rect: D2D_RECT_F { left: meter_r.left + 2.0 * s, top: meter_r.top + 2.0 * s, right: meter_r.right - 2.0 * s, bottom: meter_r.bottom - 14.0 * s }, radiusX: 4.0 * s, radiusY: 4.0 * s },
                            &glow,
                        );
                    }
                    if let Ok(mrim) = rt.CreateSolidColorBrush(&elem_col_rgba(tr4.border_rgb, 0.72, viz_pal.accent_col(0.60)), None) {
                        rt.DrawRoundedRectangle(&D2D1_ROUNDED_RECT { rect: meter_r, radiusX: 6.0 * s, radiusY: 6.0 * s }, &mrim, 1.1 * s, None);
                    }
                    let piv = Vector2 { X: x + (mx + 37.5) * s, Y: y + 80.0 * s };
                    let arc_r = 43.0 * s;
                    if let Ok(s_br) = rt.CreateSolidColorBrush(&vc1, None) {
                        let b: &ID2D1Brush = &s_br;
                        draw_music_arc(rt, d2d, piv.X, piv.Y, arc_r, -130.0, 58.0, 1.5 * s, b, cap_round.as_ref());
                    }
                    if let Ok(red_br) = rt.CreateSolidColorBrush(&vc2, None) {
                        let b: &ID2D1Brush = &red_br;
                        draw_music_arc(rt, d2d, piv.X, piv.Y, arc_r, -72.0, 22.0, 2.1 * s, b, cap_round.as_ref());
                    }
                    let n_ang = (-128.0 + lvl * 74.0).to_radians();
                    if let Ok(nd_br) = rt.CreateSolidColorBrush(&elem_col_rgba(tr4.color_rgb, 0.98, viz_pal.text_col(0.98)), None) {
                        rt.DrawLine(
                            piv,
                            Vector2 { X: piv.X + (arc_r + 3.0 * s) * n_ang.cos(), Y: piv.Y + (arc_r + 3.0 * s) * n_ang.sin() },
                            &nd_br,
                            1.55 * s,
                            cap_round.as_ref(),
                        );
                    }
                    if let Ok(ab) = rt.CreateSolidColorBrush(&elem_col_rgba(tr4.border_rgb.or(tr4.color2_rgb).or(tr4.color_rgb), 0.98, viz_pal.accent_col(0.98)), None) {
                        rt.FillEllipse(&D2D1_ELLIPSE { point: piv, radiusX: 3.3 * s, radiusY: 3.3 * s }, &ab);
                    }
                    let tf_vu = make_resolved_tf(
                        dw,
                        custom_fonts,
                        &[w!("Cascadia Mono"), w!("SF Pro Display")],
                        7.2 * PT_TO_PX * s,
                        DWRITE_FONT_WEIGHT_BOLD,
                        DWRITE_TEXT_ALIGNMENT_CENTER,
                        DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
                    );
                    if let (Some(tf), Some(br)) = (&tf_vu, br_v_ov.as_ref().or(br_sand.as_ref())) {
                        draw_m_text(rt, tf, lbl, D2D_RECT_F { left: meter_r.left + 6.0 * s, top: meter_r.bottom - 15.5 * s, right: meter_r.right - 6.0 * s, bottom: meter_r.bottom - 1.5 * s }, br, None, 0.0);
                    }
                }
            });
        }

        MusicStyle::HifiAmplifier => {
            draw_music_plate_rr(rt, D2D_RECT_F { left: x, top: y, right: x + w, bottom: y + h }, 10.0 * s, cfg, pal, is_card_hov, 1.10, 1.2 * s);
            draw_music_cover_art(
                rt,
                d2d,
                D2D_RECT_F { left: x + 12.0 * s, top: y + 14.0 * s, right: x + 78.0 * s, bottom: y + 80.0 * s },
                7.0 * s,
                cover_bmp,
                pal,
                1.2,
                s,
            );
            if let (Some(tf), Some(br)) = (&tf_badge, &br_amber) {
                draw_m_text(rt, tf, "HI-RES DAC", D2D_RECT_F { left: x + 12.0 * s, top: y + 83.0 * s, right: x + 84.0 * s, bottom: y + 100.0 * s }, br, None, 0.0);
            }
            if let (Some(tf), Some(br)) = (&tf_title, &br_cream) {
                draw_m_text(rt, tf, &media.title, D2D_RECT_F { left: x + 92.0 * s, top: y + 16.0 * s, right: x + 278.0 * s, bottom: y + 38.0 * s }, br, br_sh.as_ref(), 1.0 * s);
            }
            if let (Some(tf), Some(br)) = (&tf_art, &br_sand) {
                draw_m_text(rt, tf, &media.artist, D2D_RECT_F { left: x + 92.0 * s, top: y + 38.0 * s, right: x + 278.0 * s, bottom: y + 56.0 * s }, br, br_sh.as_ref(), 0.9 * s);
            }
            if cfg.music_show_viz {
                with_viz_slot!(x + 92.0 * s, y + 69.0 * s, x + 266.0 * s, y + 83.0 * s, {
                    let rms: f32 = if has_audio { (eff_bands.iter().sum::<f32>() / 12.0).clamp(0.15, 1.0) } else { 0.0 };
                    let lit_cnt = (rms * 12.0).round() as usize;
                    let (vc1, vc2, _) = resolve_elem_grad_colors(
                        Some(&tr4),
                        0.94,
                        viz_pal.accent_col(0.90),
                        viz_pal.light_col(0.95),
                    );
                    for i in 0..12 {
                        let lx = x + (92.0 + i as f32 * 14.5) * s;
                        let is_on = i < lit_cnt;
                        let led_col = if is_on {
                            if tr4.color2_rgb.is_some() && i >= 7 {
                                vc2
                            } else if i >= 10 && tr4.color_rgb.is_none() {
                                rgba_u8(242, 92, 68, 0.96)
                            } else if i >= 7 {
                                viz_pal.light_col(0.95)
                            } else {
                                vc1
                            }
                        } else {
                            elem_col_rgba(tr4.border_rgb, 0.22, pal.text_col(0.11))
                        };
                        let led_rr = D2D1_ROUNDED_RECT { rect: D2D_RECT_F { left: lx, top: y + 69.0 * s, right: lx + 10.5 * s, bottom: y + 83.0 * s }, radiusX: 2.0 * s, radiusY: 2.0 * s };
                        if let Ok(lb) = rt.CreateSolidColorBrush(&led_col, None) {
                            rt.FillRoundedRectangle(&led_rr, &lb);
                        }
                        if is_on && tr4.border_rgb.is_some() {
                            if let Ok(lbr) = rt.CreateSolidColorBrush(&elem_col_rgba(tr4.border_rgb, 0.72, led_col), None) {
                                rt.DrawRoundedRectangle(&led_rr, &lbr, 0.9 * s, None);
                            }
                        }
                    }
                });
            }
            if let (Some(tf), Some(br)) = (&tf_badge, &br_sand) {
                draw_m_text(rt, tf, "48kHz PCM  •  GAIN", D2D_RECT_F { left: x + 286.0 * s, top: y + 16.0 * s, right: x + 402.0 * s, bottom: y + 34.0 * s }, br, None, 0.0);
            }
            draw_music_transport(rt, d2d, x, y, s, layout, true, pal, media, hover, br_cream.as_ref());
        }

        MusicStyle::WinampRack => {
            draw_music_plate_rr(rt, D2D_RECT_F { left: x + 4.0 * s, top: y + 4.0 * s, right: x + w - 4.0 * s, bottom: y + 23.0 * s }, 5.0 * s, cfg, pal, is_card_hov, 1.18, 1.1 * s);
            draw_music_plate_rr(rt, D2D_RECT_F { left: x + 4.0 * s, top: y + 26.0 * s, right: x + w - 4.0 * s, bottom: y + h - 4.0 * s }, 8.0 * s, cfg, pal, is_card_hov, 1.10, 1.2 * s);
            if let (Some(tf), Some(br)) = (&tf_badge, &br_amber) {
                draw_m_text(rt, tf, "AUTUMNAMP  //  STUDIO", D2D_RECT_F { left: x + 14.0 * s, top: y + 4.0 * s, right: x + 220.0 * s, bottom: y + 23.0 * s }, br, None, 0.0);
            }
            if cfg.music_show_plate && tr_card.visible {
                let mut nb: Option<&mut [ClockElemBox]> = None;
                let g_c = begin_elem_xf_slot(rt, 5, tr_card, (x, y, x + w, y + h), s, &mut nb);
                let g_col = elem_col_rgba(tr_card.border_rgb.or(tr_card.color_rgb), 0.48, pal.accent_col(0.35));
                if let Ok(gp) = rt.CreateSolidColorBrush(&g_col, None) {
                    rt.DrawLine(Vector2 { X: x + 206.0 * s, Y: y + 11.0 * s }, Vector2 { X: x + 362.0 * s, Y: y + 11.0 * s }, &gp, 1.0 * s, None);
                    rt.DrawLine(Vector2 { X: x + 206.0 * s, Y: y + 16.0 * s }, Vector2 { X: x + 362.0 * s, Y: y + 16.0 * s }, &gp, 1.0 * s, None);
                    for d in 0..3 {
                        rt.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: x + (376.0 + d as f32 * 10.0) * s, Y: y + 13.5 * s }, radiusX: 2.3 * s, radiusY: 2.3 * s }, &gp);
                    }
                }
                end_elem_xf_slot(rt, g_c);
            }
            draw_music_cover_art(
                rt,
                d2d,
                D2D_RECT_F { left: x + 12.0 * s, top: y + 34.0 * s, right: x + 76.0 * s, bottom: y + 98.0 * s },
                4.5 * s,
                cover_bmp,
                pal,
                1.1,
                s,
            );
            if let (Some(tf), Some(br)) = (&tf_title, &br_cream) {
                draw_m_text(rt, tf, &media.title, D2D_RECT_F { left: x + 86.0 * s, top: y + 31.0 * s, right: x + 268.0 * s, bottom: y + 51.0 * s }, br, br_sh.as_ref(), 0.9 * s);
            }
            if let (Some(tf), Some(br)) = (&tf_art, &br_sand) {
                draw_m_text(rt, tf, &media.artist, D2D_RECT_F { left: x + 86.0 * s, top: y + 49.0 * s, right: x + 268.0 * s, bottom: y + 66.0 * s }, br, br_sh.as_ref(), 0.8 * s);
            }
            let spec_r = D2D_RECT_F { left: x + 86.0 * s, top: y + 68.0 * s, right: x + 266.0 * s, bottom: y + 99.0 * s };
            with_viz_slot!(spec_r.left, spec_r.top, spec_r.right, spec_r.bottom, {
                let spec_col = if tr4.color_rgb.is_some() || cfg.music_elem_transforms[5].color_rgb.is_some() || cfg.music_use_custom_color {
                    viz_pal.glass_bot_col(0.86)
                } else {
                    rgba_u8(10, 9, 9, 0.88)
                };
                if let Ok(sbg) = rt.CreateSolidColorBrush(&spec_col, None) {
                    rt.FillRoundedRectangle(&D2D1_ROUNDED_RECT { rect: spec_r, radiusX: 4.0 * s, radiusY: 4.0 * s }, &sbg);
                }
                if let Ok(srim) = rt.CreateSolidColorBrush(&elem_col_rgba(tr4.border_rgb, 0.65, pal.accent_col(0.24)), None) {
                    rt.DrawRoundedRectangle(&D2D1_ROUNDED_RECT { rect: spec_r, radiusX: 4.0 * s, radiusY: 4.0 * s }, &srim, 0.9 * s, None);
                }
                if cfg.music_show_viz {
                    let (vc1, vc2, _vstop) = resolve_elem_grad_colors(
                        Some(&tr4),
                        if has_audio { 0.92 } else { 0.38 },
                        viz_pal.accent_col(if has_audio { 0.90 } else { 0.35 }),
                        viz_pal.light_col(0.98),
                    );
                    for i in 0..16 {
                        let bx = x + (92.0 + i as f32 * 10.6) * s;
                        let bh = if has_audio { (4.0 + eff_bands[i % 12] * 21.0).min(25.0) } else { 2.5 };
                        let bar_r = D2D_RECT_F { left: bx, top: spec_r.bottom - 3.0 * s - bh * s, right: bx + 7.0 * s, bottom: spec_r.bottom - 3.0 * s };
                        if tr4.color2_rgb.is_some() {
                            let stops = [
                                D2D1_GRADIENT_STOP { position: 0.0, color: vc2 },
                                D2D1_GRADIENT_STOP { position: 1.0, color: vc1 },
                            ];
                            if let Some(bb) = create_linear_grad(rt, (bx, bar_r.top), (bx, bar_r.bottom), &stops) {
                                rt.FillRectangle(&bar_r, &bb);
                            }
                        } else if let Ok(bb) = rt.CreateSolidColorBrush(&vc1, None) {
                            rt.FillRectangle(&bar_r, &bb);
                        }
                        if has_audio {
                            let peak_c = if tr4.color2_rgb.is_some() { vc2 } else { viz_pal.light_col(0.98) };
                            if let Ok(gb) = rt.CreateSolidColorBrush(&peak_c, None) {
                                rt.FillRectangle(&D2D_RECT_F { left: bx, top: spec_r.bottom - 5.5 * s - bh * s, right: bx + 7.0 * s, bottom: spec_r.bottom - 3.8 * s - bh * s }, &gb);
                            }
                        }
                    }
                }
            });
            if let (Some(tf), Some(br)) = (&tf_badge, &br_gold) {
                draw_m_text(rt, tf, "320 KBPS • STEREO", D2D_RECT_F { left: x + 276.0 * s, top: y + 34.0 * s, right: x + 402.0 * s, bottom: y + 50.0 * s }, br, None, 0.0);
            }
            draw_music_transport(rt, d2d, x, y, s, layout, true, pal, media, hover, br_cream.as_ref());
        }

        MusicStyle::FlipJukebox => {
            let cov_r = D2D_RECT_F { left: x + 6.0 * s, top: y + 10.0 * s, right: x + 94.0 * s, bottom: y + 98.0 * s };
            draw_music_cover_art(rt, d2d, cov_r, 12.0 * s, cover_bmp, pal, 1.5, s);

            let top_f = D2D_RECT_F { left: x + 104.0 * s, top: y + 8.0 * s, right: x + w - 4.0 * s, bottom: y + 50.0 * s };
            draw_music_plate_rr(rt, top_f, 9.0 * s, cfg, pal, is_card_hov, 1.15, 1.15 * s);
            if let (Some(tf), Some(br)) = (&tf_title, &br_cream) {
                draw_m_text(rt, tf, &media.title, D2D_RECT_F { left: top_f.left + 14.0 * s, top: top_f.top + 3.0 * s, right: top_f.right - 14.0 * s, bottom: top_f.bottom - 3.0 * s }, br, br_sh.as_ref(), 1.2 * s);
            }

            let bot_f = D2D_RECT_F { left: x + 104.0 * s, top: y + 58.0 * s, right: x + w - 4.0 * s, bottom: y + 100.0 * s };
            draw_music_plate_rr(rt, bot_f, 9.0 * s, cfg, pal, is_card_hov, 1.12, 1.15 * s);
            if cfg.music_show_plate && tr_card.visible {
                let mut nb: Option<&mut [ClockElemBox]> = None;
                let g_c = begin_elem_xf_slot(rt, 5, tr_card, (x, y, x + w, y + h), s, &mut nb);
                if let Ok(gap) = rt.CreateSolidColorBrush(&rgba_u8(6, 5, 5, 0.72), None) {
                    let my = (top_f.top + top_f.bottom) * 0.5;
                    rt.DrawLine(Vector2 { X: top_f.left + 3.0 * s, Y: my }, Vector2 { X: top_f.right - 3.0 * s, Y: my }, &gap, 1.4 * s, None);
                    let my2 = (bot_f.top + bot_f.bottom) * 0.5;
                    rt.DrawLine(Vector2 { X: bot_f.left + 3.0 * s, Y: my2 }, Vector2 { X: bot_f.right - 3.0 * s, Y: my2 }, &gap, 1.2 * s, None);
                }
                let pin_c = elem_col_rgba(tr_card.border_rgb.or(tr_card.color_rgb), 0.78, pal.accent_col(0.75));
                if let Ok(pin) = rt.CreateSolidColorBrush(&pin_c, None) {
                    for r_box in [top_f, bot_f] {
                        let my = (r_box.top + r_box.bottom) * 0.5;
                        rt.FillRoundedRectangle(
                            &D2D1_ROUNDED_RECT { rect: D2D_RECT_F { left: r_box.left - 1.5 * s, top: my - 4.0 * s, right: r_box.left + 3.5 * s, bottom: my + 4.0 * s }, radiusX: 1.2 * s, radiusY: 1.2 * s },
                            &pin,
                        );
                        rt.FillRoundedRectangle(
                            &D2D1_ROUNDED_RECT { rect: D2D_RECT_F { left: r_box.right - 3.5 * s, top: my - 4.0 * s, right: r_box.right + 1.5 * s, bottom: my + 4.0 * s }, radiusX: 1.2 * s, radiusY: 1.2 * s },
                            &pin,
                        );
                    }
                }
                end_elem_xf_slot(rt, g_c);
            }
            if let (Some(tf), Some(br)) = (&tf_art, &br_sand) {
                draw_m_text(rt, tf, &media.artist, D2D_RECT_F { left: bot_f.left + 14.0 * s, top: bot_f.top + 2.0 * s, right: x + 282.0 * s, bottom: bot_f.top + 26.0 * s }, br, br_sh.as_ref(), 0.9 * s);
            }
            if cfg.music_show_viz {
                with_viz_slot!(bot_f.left + 14.0 * s, bot_f.top + 24.0 * s, bot_f.left + 124.0 * s, bot_f.top + 40.0 * s, {
                    let (vc1, vc2, _vstop) = resolve_elem_grad_colors(
                        Some(&tr4),
                        if has_audio { 0.88 } else { 0.36 },
                        viz_pal.accent_col(if has_audio { 0.88 } else { 0.36 }),
                        viz_pal.light_col(if has_audio { 0.92 } else { 0.42 }),
                    );
                    for i in 0..10 {
                        let bx = bot_f.left + (14.0 + i as f32 * 11.0) * s;
                        let bh = if has_audio { (3.0 + eff_bands[i] * 7.0).min(10.0) } else { 2.2 };
                        let r_bar = D2D1_ROUNDED_RECT { rect: D2D_RECT_F { left: bx, top: bot_f.top + (32.0 - bh * 0.5) * s, right: bx + 5.0 * s, bottom: bot_f.top + (32.0 + bh * 0.5) * s }, radiusX: 1.5 * s, radiusY: 1.5 * s };
                        if tr4.color2_rgb.is_some() {
                            let stops = [
                                D2D1_GRADIENT_STOP { position: 0.0, color: vc2 },
                                D2D1_GRADIENT_STOP { position: 1.0, color: vc1 },
                            ];
                            if let Some(b) = create_linear_grad(rt, (bx, bot_f.top + 27.0 * s), (bx, bot_f.top + 37.0 * s), &stops) {
                                rt.FillRoundedRectangle(&r_bar, &b);
                            }
                        } else if let Ok(b) = rt.CreateSolidColorBrush(&vc1, None) {
                            rt.FillRoundedRectangle(&r_bar, &b);
                        }
                    }
                });
            }
            draw_music_transport(rt, d2d, x, y, s, layout, false, pal, media, hover, br_cream.as_ref());
        }

        // ═══════════════════════════════════════════════════════════════════
        // REALISTIC & CALM MATERIALS MUSIC PLAYERS (6)
        // ═══════════════════════════════════════════════════════════════════
        MusicStyle::VacuumTubeAmp => {
            // Dual 12AX7 Glowing Glass Vacuum Triode Amplifier + Champagne Brass Chassis + Analog Needle VU
            let rad_s = (cfg.music_corner_radius as f32 / 14.0).clamp(0.15, 2.5);
            let deck_r = D2D_RECT_F { left: x + 2.0 * s, top: y + 20.0 * s, right: x + w - 2.0 * s, bottom: y + h - 2.0 * s };
            let deck_rr = D2D1_ROUNDED_RECT { rect: deck_r, radiusX: 12.0 * s * rad_s, radiusY: 12.0 * s * rad_s };
            let glass_a = (cfg.music_glass_alpha as f32 / 255.0).clamp(0.02, 0.99);
            let rim_a = (cfg.music_rim_alpha as f32 / 255.0).clamp(0.0, 1.0);
            let use_real_base = !cfg.music_match_wallpaper
                && !cfg.music_use_custom_color
                && tr_card.color_rgb.is_none()
                && tr_card.color2_rgb.is_none();
            if cfg.music_show_plate && tr_card.visible {
                let mut nb: Option<&mut [ClockElemBox]> = None;
                let g = begin_elem_xf_slot(rt, 5, tr_card, (x, y, x + w, y + h), s, &mut nb);
                // Dual protruding 12AX7 glass vacuum triode tubes rising cleanly inside widget headroom
                let bass = if has_audio { (eff_bands[0] + eff_bands[1] + eff_bands[2]) / 3.0 } else { 0.25 };
                for ti in 0..2 {
                    let tcx = x + (120.0 + ti as f32 * 36.0) * s;
                    let t_top = y + 4.0 * s;
                    let t_bot = y + 25.0 * s;
                    let tube_rr = D2D1_ROUNDED_RECT {
                        rect: D2D_RECT_F { left: tcx - 10.5 * s, top: t_top, right: tcx + 10.5 * s, bottom: t_bot },
                        radiusX: 9.5 * s,
                        radiusY: 8.5 * s,
                    };
                    let gl_col = elem_col_rgba(tr_card.glow_rgb.or(tr_card.color_rgb), 0.20 + 0.32 * bass, rgba_u8(255, 134, 30, 0.20 + 0.32 * bass));
                    if let Ok(gl) = rt.CreateSolidColorBrush(&gl_col, None) {
                        rt.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: tcx, Y: t_top + 11.5 * s }, radiusX: (15.0 + 4.0 * bass) * s, radiusY: (9.5 + 2.0 * bass) * s }, &gl);
                    }
                    if let Ok(gb) = rt.CreateSolidColorBrush(&rgba_u8(46, 30, 20, 0.90), None) {
                        rt.FillRoundedRectangle(&tube_rr, &gb);
                    }
                    if let Ok(pl) = rt.CreateSolidColorBrush(&rgba_u8(102, 92, 86, 0.72), None) {
                        rt.FillRectangle(&D2D_RECT_F { left: tcx - 5.0 * s, top: t_top + 6.5 * s, right: tcx + 5.0 * s, bottom: t_bot - 4.0 * s }, &pl);
                    }
                    let fil_col = elem_col_rgba(tr_card.border_rgb.or(tr_card.color2_rgb), 0.74 + 0.24 * bass, rgba_u8(255, 198, 86, 0.74 + 0.24 * bass));
                    if let Ok(fil) = rt.CreateSolidColorBrush(&fil_col, None) {
                        rt.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: tcx, Y: t_top + 10.0 * s }, radiusX: 3.2 * s, radiusY: 2.4 * s }, &fil);
                        rt.DrawLine(Vector2 { X: tcx, Y: t_top + 7.5 * s }, Vector2 { X: tcx, Y: t_bot - 4.0 * s }, &fil, 1.35 * s, None);
                    }
                    if let Ok(gr) = rt.CreateSolidColorBrush(&rgba_u8(255, 240, 214, 0.66), None) {
                        rt.DrawRoundedRectangle(&tube_rr, &gr, 1.0 * s, None);
                        rt.DrawLine(Vector2 { X: tcx - 6.5 * s, Y: t_top + 4.5 * s }, Vector2 { X: tcx - 5.8 * s, Y: t_bot - 5.5 * s }, &gr, 1.05 * s, None);
                    }
                }
                let inter = crate::realistic_pbr::pbr_input_from_viz(fx_viz, anim_t, is_card_hov);
                let tint_ov = tr_card.color_rgb.map(|c| (c, tr_card.color2_rgb)).or_else(|| {
                    if !use_real_base {
                        Some((crate::theme::rgb_to_u32(pal.glass_top), Some(crate::theme::rgb_to_u32(pal.glass_bot))))
                    } else {
                        Some((0x3A2616, Some(0x18100A)))
                    }
                });
                crate::realistic_pbr::draw_pbr_drop_shadows(rt, deck_r, deck_rr.radiusX, s, false);
                crate::realistic_pbr::draw_pbr_surface_clipped(
                    rt,
                    d2d,
                    crate::realistic_pbr::PbrMaterial::BrushedTitanium,
                    deck_r,
                    deck_rr.radiusX,
                    s,
                    (0.42 + glass_a * 0.58).clamp(0.30, 0.99),
                    tint_ov,
                    None,
                    &inter,
                );
                if paint_fx {
                    paint_widget_elemental_fx(rt, d2d, deck_r.left, deck_r.top, deck_r.right, deck_r.bottom, (x, y, w, h), deck_rr.radiusX, s, cfg.music_hover_fx, 88, pal, false, fx_viz);
                }
                crate::realistic_pbr::draw_pbr_bevel_rim(
                    rt,
                    deck_r,
                    deck_rr.radiusX,
                    s,
                    crate::realistic_pbr::PbrMaterial::VintageWalnutBrass,
                    tr_card.border_rgb,
                    rim_a.max(0.75),
                    inter.hover_mix,
                    None,
                );
                // Machined brass tube socket collars seated on upper chassis rim
                if let Ok(sock) = rt.CreateSolidColorBrush(&rgba_u8(204, 158, 78, 0.92), None) {
                    if let Ok(sock_hi) = rt.CreateSolidColorBrush(&rgba_u8(252, 230, 178, 0.72), None) {
                        for ti in 0..2 {
                            let tcx = x + (120.0 + ti as f32 * 36.0) * s;
                            let collar = D2D1_ROUNDED_RECT {
                                rect: D2D_RECT_F {
                                    left: tcx - 12.0 * s,
                                    top: deck_r.top - 1.5 * s,
                                    right: tcx + 12.0 * s,
                                    bottom: deck_r.top + 3.0 * s,
                                },
                                radiusX: 2.0 * s,
                                radiusY: 2.0 * s,
                            };
                            rt.FillRoundedRectangle(&collar, &sock);
                            rt.DrawLine(
                                Vector2 { X: collar.rect.left + 2.0 * s, Y: collar.rect.top + 0.8 * s },
                                Vector2 { X: collar.rect.right - 2.0 * s, Y: collar.rect.top + 0.8 * s },
                                &sock_hi,
                                0.9 * s,
                                None,
                            );
                        }
                    }
                }
                end_elem_xf_slot(rt, g);
            }
            let cov_r = D2D_RECT_F { left: x + 14.0 * s, top: y + 29.0 * s, right: x + 90.0 * s, bottom: y + 105.0 * s };
            draw_music_cover_art(rt, d2d, cov_r, 10.0 * s, cover_bmp, pal, 1.4, s);
            if let (Some(tf), Some(br)) = (&tf_badge, &br_gold) {
                draw_m_text(rt, tf, "12AX7 CLASS-A TUBE AMP", D2D_RECT_F { left: x + 188.0 * s, top: y + 23.5 * s, right: x + 402.0 * s, bottom: y + 37.5 * s }, br, None, 0.0);
            }
            if let (Some(tf), Some(br)) = (&tf_title, &br_cream) {
                draw_m_text(rt, tf, &media.title, D2D_RECT_F { left: x + 102.0 * s, top: y + 35.0 * s, right: x + 404.0 * s, bottom: y + 57.0 * s }, br, br_sh.as_ref(), 1.0 * s);
            }
            if let (Some(tf), Some(br)) = (&tf_art, &br_sand) {
                draw_m_text(rt, tf, &media.artist, D2D_RECT_F { left: x + 102.0 * s, top: y + 56.0 * s, right: x + 284.0 * s, bottom: y + 73.0 * s }, br, br_sh.as_ref(), 0.8 * s);
            }
            if cfg.music_show_viz {
                let vu_r = D2D_RECT_F { left: x + 102.0 * s, top: y + 76.0 * s, right: x + 276.0 * s, bottom: y + 104.0 * s };
                with_viz_slot!(vu_r.left, vu_r.top, vu_r.right, vu_r.bottom, {
                    let vu_rr = D2D1_ROUNDED_RECT { rect: vu_r, radiusX: 5.0 * s, radiusY: 5.0 * s };
                    let (vc1, vc2, _) = resolve_elem_grad_colors(Some(&tr4), 0.94, rgba_u8(255, 224, 152, 0.85), rgba_u8(255, 96, 38, 0.96));
                    if let Ok(vb) = rt.CreateSolidColorBrush(&elem_col_rgba(tr4.glow_rgb.or(tr4.color_rgb), 0.24, rgba_u8(255, 212, 128, 0.22)), None) {
                        rt.FillRoundedRectangle(&vu_rr, &vb);
                    }
                    if let Ok(vr) = rt.CreateSolidColorBrush(&elem_col_rgba(tr4.border_rgb.or(tr4.color_rgb), 0.76, rgba_u8(232, 186, 108, 0.72)), None) {
                        rt.DrawRoundedRectangle(&vu_rr, &vr, 1.0 * s, None);
                    }
                    let lv: f32 = if has_audio { (eff_bands[0..6].iter().sum::<f32>() / 6.0).clamp(0.05, 1.0) } else { 0.14 };
                    let rv: f32 = if has_audio { (eff_bands[6..12].iter().sum::<f32>() / 6.0).clamp(0.05, 1.0) } else { 0.14 };
                    for (idx, val) in [lv, rv].into_iter().enumerate() {
                        let pivot = Vector2 { X: vu_r.left + (44.0 + idx as f32 * 86.0) * s, Y: vu_r.bottom - 2.0 * s };
                        let ang = (-138.0 + val * 96.0).to_radians();
                        if let Ok(tk) = rt.CreateSolidColorBrush(&vc1, None) {
                            for t in 0..7 {
                                let a = (-140.0 + t as f32 * 16.5).to_radians();
                                rt.DrawLine(
                                    Vector2 { X: pivot.X + 18.0 * s * a.cos(), Y: pivot.Y + 18.0 * s * a.sin() },
                                    Vector2 { X: pivot.X + 22.0 * s * a.cos(), Y: pivot.Y + 22.0 * s * a.sin() },
                                    &tk,
                                    0.9 * s,
                                    None,
                                );
                            }
                        }
                        let nd_c = if tr4.color2_rgb.is_some() || tr4.color_rgb.is_some() { vc2 } else { rgba_u8(255, 96, 38, 0.96) };
                        if let Ok(nd) = rt.CreateSolidColorBrush(&nd_c, None) {
                            rt.DrawLine(pivot, Vector2 { X: pivot.X + 22.0 * s * ang.cos(), Y: pivot.Y + 22.0 * s * ang.sin() }, &nd, 1.5 * s, None);
                        }
                    }
                });
            }
            draw_music_transport(rt, d2d, x, y, s, layout, true, pal, media, hover, br_cream.as_ref());
        }

        MusicStyle::BrushedTitaniumDeck => {
            // Anisotropic CNC Brushed Titanium Hi-Fi + Cursor Fresnel Glint + Backlit Analog VU Meters
            let rad_s = (cfg.music_corner_radius as f32 / 14.0).clamp(0.15, 2.5);
            let main_r = D2D_RECT_F { left: x + 2.0 * s, top: y + 2.0 * s, right: x + w - 2.0 * s, bottom: y + h - 2.0 * s };
            let r_card = 11.0 * s * rad_s;
            let glass_a = (cfg.music_glass_alpha as f32 / 255.0).clamp(0.02, 0.99);
            let rim_a = (cfg.music_rim_alpha as f32 / 255.0).clamp(0.0, 1.0);
            let inter = crate::realistic_pbr::pbr_input_from_viz(fx_viz, anim_t, is_card_hov);
            let tint_ov = tr_card.color_rgb.map(|c| (c, tr_card.color2_rgb)).or_else(|| {
                if cfg.music_use_custom_color || cfg.music_match_wallpaper {
                    Some((crate::theme::rgb_to_u32(pal.glass_top), Some(crate::theme::rgb_to_u32(pal.glass_bot))))
                } else {
                    None
                }
            });
            if cfg.music_show_plate && tr_card.visible {
                let mut nb: Option<&mut [ClockElemBox]> = None;
                let g = begin_elem_xf_slot(rt, 5, tr_card, (x, y, x + w, y + h), s, &mut nb);
                crate::realistic_pbr::draw_pbr_drop_shadows(rt, main_r, r_card, s, false);
                crate::realistic_pbr::draw_pbr_surface_clipped(
                    rt,
                    d2d,
                    crate::realistic_pbr::PbrMaterial::BrushedTitanium,
                    main_r,
                    r_card,
                    s,
                    (0.45 + glass_a * 0.55).clamp(0.32, 0.99),
                    tint_ov,
                    None,
                    &inter,
                );
                if paint_fx {
                    paint_widget_elemental_fx(rt, d2d, main_r.left, main_r.top, main_r.right, main_r.bottom, (x, y, w, h), r_card, s, cfg.music_hover_fx, 88, pal, false, fx_viz);
                }
                crate::realistic_pbr::draw_pbr_bevel_rim(
                    rt,
                    main_r,
                    r_card,
                    s,
                    crate::realistic_pbr::PbrMaterial::BrushedTitanium,
                    tr_card.border_rgb,
                    rim_a.max(0.75),
                    inter.hover_mix,
                    None,
                );
                end_elem_xf_slot(rt, g);
            }
            let cov_r = D2D_RECT_F { left: x + 16.0 * s, top: y + 16.0 * s, right: x + 94.0 * s, bottom: y + 94.0 * s };
            draw_music_cover_art(rt, d2d, cov_r, 8.0 * s, cover_bmp, pal, 1.3, s);
            let badge_c = if cfg.music_use_custom_color || cfg.music_match_wallpaper {
                pal.accent_col(0.96)
            } else {
                rgba_u8(255, 167, 38, 0.96)
            };
            if let (Some(tf), Ok(orng)) = (&tf_badge, rt.CreateSolidColorBrush(&badge_c, None)) {
                draw_m_text(rt, tf, "BRUSHED TITANIUM • HI-FI VU", D2D_RECT_F { left: x + 106.0 * s, top: y + 12.0 * s, right: x + 392.0 * s, bottom: y + 26.0 * s }, &orng, None, 0.0);
            }
            if let (Some(tf), Some(br)) = (&tf_title, &br_cream) {
                draw_m_text(rt, tf, &media.title, D2D_RECT_F { left: x + 106.0 * s, top: y + 26.0 * s, right: x + 394.0 * s, bottom: y + 48.0 * s }, br, br_sh.as_ref(), 1.0 * s);
            }
            if let (Some(tf), Some(br)) = (&tf_art, &br_sand) {
                draw_m_text(rt, tf, &media.artist, D2D_RECT_F { left: x + 106.0 * s, top: y + 47.0 * s, right: x + 282.0 * s, bottom: y + 64.0 * s }, br, br_sh.as_ref(), 0.8 * s);
            }
            if cfg.music_show_viz {
                // Twin warm amber incandescent analog VU meter bays with ballistic spring needles & redline ticks
                let vu_r = D2D_RECT_F { left: x + 106.0 * s, top: y + 67.0 * s, right: x + 278.0 * s, bottom: y + 98.0 * s };
                with_viz_slot!(vu_r.left, vu_r.top, vu_r.right, vu_r.bottom, {
                    let vu_l = if has_audio { ((eff_bands[0] + eff_bands[2] + eff_bands[4]) / 3.0).clamp(0.08, 1.0) } else { 0.24 + 0.14 * inter.hover_mix };
                    let vu_rv = if has_audio { ((eff_bands[1] + eff_bands[3] + eff_bands[5]) / 3.0).clamp(0.08, 1.0) } else { 0.20 + 0.18 * inter.hover_mix };
                    let def_vu = if cfg.music_use_custom_color || cfg.music_match_wallpaper {
                        viz_pal.accent_col(0.94)
                    } else {
                        rgba_u8(255, 167, 38, 0.92)
                    };
                    let (vc1, vc2, _) = resolve_elem_grad_colors(Some(&tr4), 0.95, def_vu, rgba_u8(239, 83, 80, 0.96));
                    for (vi, &val) in [vu_l, vu_rv].iter().enumerate() {
                        let bx0 = vu_r.left + vi as f32 * 88.0 * s;
                        let bay = D2D_RECT_F { left: bx0, top: vu_r.top, right: bx0 + 83.0 * s, bottom: vu_r.bottom };
                        let bay_rr = D2D1_ROUNDED_RECT { rect: bay, radiusX: 4.5 * s, radiusY: 4.5 * s };
                        if let Some(bg) = create_linear_grad(
                            rt,
                            (bay.left, bay.top),
                            (bay.left, bay.bottom),
                            &[
                                D2D1_GRADIENT_STOP { position: 0.0, color: rgba_u8(54, 34, 14, 0.94) },
                                D2D1_GRADIENT_STOP { position: 0.5, color: rgba_u8(36, 22, 10, 0.96) },
                                D2D1_GRADIENT_STOP { position: 1.0, color: rgba_u8(20, 12, 6, 0.98) },
                            ],
                        ) {
                            rt.FillRoundedRectangle(&bay_rr, &bg);
                        }
                        if let Ok(br_b) = rt.CreateSolidColorBrush(&elem_col_rgba(tr4.border_rgb, 0.72, rgba_u8(186, 194, 204, 0.56)), None) {
                            rt.DrawRoundedRectangle(&bay_rr, &br_b, 0.9 * s, None);
                        }
                        let pivot = Vector2 { X: (bay.left + bay.right) * 0.5, Y: bay.bottom + 5.0 * s };
                        for tk_i in 0..9 {
                            let a = (-138.0 + tk_i as f32 * 12.0).to_radians();
                            let col = if tk_i >= 6 { vc2 } else { vc1 };
                            if let Ok(tb) = rt.CreateSolidColorBrush(&col, None) {
                                rt.DrawLine(
                                    Vector2 { X: pivot.X + 20.0 * s * a.cos(), Y: pivot.Y + 20.0 * s * a.sin() },
                                    Vector2 { X: pivot.X + 25.0 * s * a.cos(), Y: pivot.Y + 25.0 * s * a.sin() },
                                    &tb,
                                    if tk_i >= 6 { 1.3 * s } else { 0.9 * s },
                                    None,
                                );
                            }
                        }
                        let ang = (-136.0 + val * 92.0).to_radians();
                        if let Ok(sh_n) = rt.CreateSolidColorBrush(&rgba_u8(0, 0, 0, 0.45), None) {
                            rt.DrawLine(
                                Vector2 { X: pivot.X + 1.0 * s, Y: pivot.Y + 1.0 * s },
                                Vector2 { X: pivot.X + 25.0 * s * ang.cos() + 1.0 * s, Y: pivot.Y + 25.0 * s * ang.sin() + 1.0 * s },
                                &sh_n,
                                1.6 * s,
                                None,
                            );
                        }
                        if let Ok(nd) = rt.CreateSolidColorBrush(&rgba_u8(255, 224, 166, 0.98), None) {
                            rt.DrawLine(pivot, Vector2 { X: pivot.X + 25.0 * s * ang.cos(), Y: pivot.Y + 25.0 * s * ang.sin() }, &nd, 1.4 * s, None);
                        }
                    }
                });
            }
            draw_music_transport(rt, d2d, x, y, s, layout, true, pal, media, hover, br_cream.as_ref());
        }

        MusicStyle::RainWindowPlayer => {
            // Moody Twilight Rainy Windowpane PBR Condensation Glass + Interactive Cursor Wiper & Droplets
            let rad_s = (cfg.music_corner_radius as f32 / 14.0).clamp(0.15, 2.5);
            let main_r = D2D_RECT_F { left: x + 2.0 * s, top: y + 2.0 * s, right: x + w - 2.0 * s, bottom: y + h - 2.0 * s };
            let r_card = 18.0 * s * rad_s;
            let glass_a = (cfg.music_glass_alpha as f32 / 255.0).clamp(0.02, 0.99);
            let rim_a = (cfg.music_rim_alpha as f32 / 255.0).clamp(0.0, 1.0);
            let inter = crate::realistic_pbr::pbr_input_from_viz(fx_viz, anim_t, is_card_hov);
            let tint_ov = tr_card.color_rgb.map(|c| (c, tr_card.color2_rgb)).or_else(|| {
                if cfg.music_use_custom_color || cfg.music_match_wallpaper {
                    Some((crate::theme::rgb_to_u32(pal.glass_top), Some(crate::theme::rgb_to_u32(pal.glass_bot))))
                } else {
                    None
                }
            });
            if cfg.music_show_plate && tr_card.visible {
                let mut nb: Option<&mut [ClockElemBox]> = None;
                let g = begin_elem_xf_slot(rt, 5, tr_card, (x, y, x + w, y + h), s, &mut nb);
                crate::realistic_pbr::draw_pbr_drop_shadows(rt, main_r, r_card, s, false);
                crate::realistic_pbr::draw_pbr_surface_clipped(
                    rt,
                    d2d,
                    crate::realistic_pbr::PbrMaterial::RainyWindowpane,
                    main_r,
                    r_card,
                    s,
                    (0.42 + glass_a * 0.58).clamp(0.32, 0.99),
                    tint_ov,
                    None,
                    &inter,
                );
                if paint_fx {
                    paint_widget_elemental_fx(rt, d2d, main_r.left, main_r.top, main_r.right, main_r.bottom, (x, y, w, h), r_card, s, cfg.music_hover_fx, 88, pal, false, fx_viz);
                }
                crate::realistic_pbr::draw_pbr_bevel_rim(
                    rt,
                    main_r,
                    r_card,
                    s,
                    crate::realistic_pbr::PbrMaterial::RainyWindowpane,
                    tr_card.border_rgb,
                    rim_a.max(0.75),
                    inter.hover_mix,
                    None,
                );
                end_elem_xf_slot(rt, g);
            }
            let cov_r = D2D_RECT_F { left: x + 14.0 * s, top: y + 16.0 * s, right: x + 92.0 * s, bottom: y + 94.0 * s };
            draw_music_cover_art(rt, d2d, cov_r, 12.0 * s, cover_bmp, pal, 1.3, s);
            if let (Some(tf), Some(br)) = (&tf_badge, &br_gold) {
                draw_m_text(rt, tf, "RAINY WINDOW • AMBIENT", D2D_RECT_F { left: x + 104.0 * s, top: y + 12.0 * s, right: x + 392.0 * s, bottom: y + 26.0 * s }, br, None, 0.0);
            }
            if let (Some(tf), Some(br)) = (&tf_title, &br_cream) {
                draw_m_text(rt, tf, &media.title, D2D_RECT_F { left: x + 104.0 * s, top: y + 27.0 * s, right: x + 394.0 * s, bottom: y + 48.0 * s }, br, br_sh.as_ref(), 1.0 * s);
            }
            if let (Some(tf), Some(br)) = (&tf_art, &br_sand) {
                draw_m_text(rt, tf, &media.artist, D2D_RECT_F { left: x + 104.0 * s, top: y + 47.0 * s, right: x + 282.0 * s, bottom: y + 64.0 * s }, br, br_sh.as_ref(), 0.8 * s);
            }
            if cfg.music_show_viz {
                let vz = D2D_RECT_F { left: x + 104.0 * s, top: y + 68.0 * s, right: x + 276.0 * s, bottom: y + 96.0 * s };
                with_viz_slot!(vz.left, vz.top, vz.right, vz.bottom, {
                    let va = if has_audio { 0.86 } else { 0.36 };
                    let def_c = if cfg.music_use_custom_color || cfg.music_match_wallpaper { viz_pal.light_col(va) } else { rgba_u8(152, 212, 252, va) };
                    let (vc1, vc2, _) = resolve_elem_grad_colors(Some(&tr4), va, def_c, viz_pal.accent_col(va));
                    for i in 0..12 {
                        let bx = vz.left + (4.0 + i as f32 * 14.0) * s;
                        let bh = if has_audio { (4.0 + eff_bands[i] * 20.0).min(24.0) } else { 3.5 };
                        let br_rr = D2D1_ROUNDED_RECT { rect: D2D_RECT_F { left: bx, top: vz.bottom - bh * s, right: bx + 6.0 * s, bottom: vz.bottom }, radiusX: 3.0 * s, radiusY: 3.0 * s };
                        if tr4.color2_rgb.is_some() {
                            let stops = [
                                D2D1_GRADIENT_STOP { position: 0.0, color: vc2 },
                                D2D1_GRADIENT_STOP { position: 1.0, color: vc1 },
                            ];
                            if let Some(bb) = create_linear_grad(rt, (bx, vz.bottom - bh * s), (bx, vz.bottom), &stops) {
                                rt.FillRoundedRectangle(&br_rr, &bb);
                            }
                        } else if let Ok(bb) = rt.CreateSolidColorBrush(&vc1, None) {
                            rt.FillRoundedRectangle(&br_rr, &bb);
                        }
                    }
                });
            }
            draw_music_transport(rt, d2d, x, y, s, layout, false, pal, media, hover, br_cream.as_ref());
        }

        MusicStyle::CalmWaterRipple => {
            // Photorealistic Calm Ocean Lagoon PBR Voronoi Water Caustics + Audio & Interactive Cursor Ripples
            let rad_s = (cfg.music_corner_radius as f32 / 14.0).clamp(0.15, 2.5);
            let main_r = D2D_RECT_F { left: x + 2.0 * s, top: y + 2.0 * s, right: x + w - 2.0 * s, bottom: y + h - 2.0 * s };
            let r_card = 22.0 * s * rad_s;
            let glass_a = (cfg.music_glass_alpha as f32 / 255.0).clamp(0.02, 0.99);
            let rim_a = (cfg.music_rim_alpha as f32 / 255.0).clamp(0.0, 1.0);
            let inter = crate::realistic_pbr::pbr_input_from_viz(fx_viz, anim_t, is_card_hov);
            let tint_ov = tr_card.color_rgb.map(|c| (c, tr_card.color2_rgb)).or_else(|| {
                if cfg.music_use_custom_color || cfg.music_match_wallpaper {
                    Some((crate::theme::rgb_to_u32(pal.glass_top), Some(crate::theme::rgb_to_u32(pal.glass_bot))))
                } else {
                    None
                }
            });
            let is_sage_mist = (cfg.music_use_custom_color || cfg.music_match_wallpaper)
                && pal.glass_top.1 >= pal.glass_top.2
                && (pal.glass_top.1 as i32 - pal.glass_top.0 as i32) >= 6;
            let pbr_mat = if is_sage_mist {
                crate::realistic_pbr::PbrMaterial::AlpineForestMist
            } else {
                crate::realistic_pbr::PbrMaterial::CalmOceanLagoon
            };
            if cfg.music_show_plate && tr_card.visible {
                let mut nb: Option<&mut [ClockElemBox]> = None;
                let g = begin_elem_xf_slot(rt, 5, tr_card, (x, y, x + w, y + h), s, &mut nb);
                crate::realistic_pbr::draw_pbr_drop_shadows(rt, main_r, r_card, s, false);
                crate::realistic_pbr::draw_pbr_surface_clipped(
                    rt,
                    d2d,
                    pbr_mat,
                    main_r,
                    r_card,
                    s,
                    (0.42 + glass_a * 0.58).clamp(0.32, 0.99),
                    tint_ov,
                    None,
                    &inter,
                );
                if paint_fx {
                    paint_widget_elemental_fx(rt, d2d, main_r.left, main_r.top, main_r.right, main_r.bottom, (x, y, w, h), r_card, s, cfg.music_hover_fx, 88, pal, false, fx_viz);
                }
                crate::realistic_pbr::draw_pbr_bevel_rim(
                    rt,
                    main_r,
                    r_card,
                    s,
                    pbr_mat,
                    tr_card.border_rgb,
                    rim_a.max(0.75),
                    inter.hover_mix,
                    None,
                );
                end_elem_xf_slot(rt, g);
            }
            let cov_r = D2D_RECT_F { left: x + 16.0 * s, top: y + 17.0 * s, right: x + 94.0 * s, bottom: y + 95.0 * s };
            draw_music_cover_art(rt, d2d, cov_r, 39.0 * s, cover_bmp, pal, 1.4, s);
            if let (Some(tf), Some(br)) = (&tf_badge, &br_cream) {
                let badge_lbl = if is_sage_mist {
                    "ZEN MIST • CALM SOUND"
                } else {
                    "CRYSTAL WATER • CALM SOUND"
                };
                draw_m_text(rt, tf, badge_lbl, D2D_RECT_F { left: x + 106.0 * s, top: y + 13.0 * s, right: x + 394.0 * s, bottom: y + 27.0 * s }, br, None, 0.0);
            }
            if let (Some(tf), Some(br)) = (&tf_title, &br_cream) {
                draw_m_text(rt, tf, &media.title, D2D_RECT_F { left: x + 106.0 * s, top: y + 28.0 * s, right: x + 394.0 * s, bottom: y + 50.0 * s }, br, br_sh.as_ref(), 1.0 * s);
            }
            if let (Some(tf), Some(br)) = (&tf_art, &br_sand) {
                draw_m_text(rt, tf, &media.artist, D2D_RECT_F { left: x + 106.0 * s, top: y + 48.0 * s, right: x + 284.0 * s, bottom: y + 65.0 * s }, br, br_sh.as_ref(), 0.8 * s);
            }
            if cfg.music_show_viz {
                let vz = D2D_RECT_F { left: x + 106.0 * s, top: y + 67.0 * s, right: x + 278.0 * s, bottom: y + 98.0 * s };
                with_viz_slot!(vz.left, vz.top, vz.right, vz.bottom, {
                    let energy: f32 = if has_audio { (eff_bands.iter().sum::<f32>() / 12.0).clamp(0.1, 1.0) } else { 0.25 };
                    let rcx = (vz.left + vz.right) * 0.5;
                    let rcy = (vz.top + vz.bottom) * 0.5;
                    let (vc1, vc2, _) = resolve_elem_grad_colors(Some(&tr4), 0.90, if cfg.music_use_custom_color || cfg.music_match_wallpaper { viz_pal.light_col(0.90) } else { rgba_u8(216, 254, 255, 0.90) }, viz_pal.accent_col(0.90));
                    for ri in 0..4 {
                        let p = ((anim_t * 0.55 + ri as f32 * 0.25) % 1.0).clamp(0.0, 1.0);
                        let mut rc = if tr4.color2_rgb.is_some() && ri % 2 == 1 { vc2 } else { vc1 };
                        rc.a = (1.0 - p) * (0.35 + 0.55 * energy);
                        if let Ok(rb) = rt.CreateSolidColorBrush(&rc, None) {
                            rt.DrawEllipse(&D2D1_ELLIPSE { point: Vector2 { X: rcx, Y: rcy }, radiusX: (10.0 + p * 72.0) * s, radiusY: (3.0 + p * 11.0) * s }, &rb, 1.25 * s, None);
                        }
                    }
                });
            }
            draw_music_transport(rt, d2d, x, y, s, layout, false, pal, media, hover, br_cream.as_ref());
        }

        MusicStyle::LeatherFieldAudio => {
            // Studio Executive Saddle-Stitched Pebble Leather Field Recorder + Waxed Stitches + Brass Rivets
            let rad_s = (cfg.music_corner_radius as f32 / 14.0).clamp(0.15, 2.5);
            let main_r = D2D_RECT_F { left: x + 2.0 * s, top: y + 2.0 * s, right: x + w - 2.0 * s, bottom: y + h - 2.0 * s };
            let r_card = 14.0 * s * rad_s;
            let glass_a = (cfg.music_glass_alpha as f32 / 255.0).clamp(0.02, 0.99);
            let rim_a = (cfg.music_rim_alpha as f32 / 255.0).clamp(0.0, 1.0);
            let inter = crate::realistic_pbr::pbr_input_from_viz(fx_viz, anim_t, is_card_hov);
            let tint_ov = tr_card.color_rgb.map(|c| (c, tr_card.color2_rgb)).or_else(|| {
                if cfg.music_use_custom_color || cfg.music_match_wallpaper {
                    Some((crate::theme::rgb_to_u32(pal.glass_top), Some(crate::theme::rgb_to_u32(pal.glass_bot))))
                } else {
                    None
                }
            });
            if cfg.music_show_plate && tr_card.visible {
                let mut nb: Option<&mut [ClockElemBox]> = None;
                let g = begin_elem_xf_slot(rt, 5, tr_card, (x, y, x + w, y + h), s, &mut nb);
                crate::realistic_pbr::draw_pbr_drop_shadows(rt, main_r, r_card, s, false);
                crate::realistic_pbr::draw_pbr_surface_clipped(
                    rt,
                    d2d,
                    crate::realistic_pbr::PbrMaterial::ExecutiveLeatherLinen,
                    main_r,
                    r_card,
                    s,
                    (0.45 + glass_a * 0.55).clamp(0.32, 0.99),
                    tint_ov,
                    None,
                    &inter,
                );
                if paint_fx {
                    paint_widget_elemental_fx(rt, d2d, main_r.left, main_r.top, main_r.right, main_r.bottom, (x, y, w, h), r_card, s, cfg.music_hover_fx, 88, pal, false, fx_viz);
                }
                crate::realistic_pbr::draw_pbr_bevel_rim(
                    rt,
                    main_r,
                    r_card,
                    s,
                    crate::realistic_pbr::PbrMaterial::ExecutiveLeatherLinen,
                    tr_card.border_rgb,
                    rim_a.max(0.75),
                    inter.hover_mix,
                    None,
                );
                end_elem_xf_slot(rt, g);
            }
            let cov_r = D2D_RECT_F { left: x + 16.0 * s, top: y + 18.0 * s, right: x + 94.0 * s, bottom: y + 96.0 * s };
            draw_music_cover_art(rt, d2d, cov_r, 8.0 * s, cover_bmp, pal, 1.3, s);
            if let (Some(tf), Some(br)) = (&tf_badge, &br_gold) {
                draw_m_text(rt, tf, "FIELD RECORDER • CALFSKIN", D2D_RECT_F { left: x + 106.0 * s, top: y + 14.0 * s, right: x + 394.0 * s, bottom: y + 28.0 * s }, br, None, 0.0);
            }
            if let (Some(tf), Some(br)) = (&tf_title, &br_cream) {
                draw_m_text(rt, tf, &media.title, D2D_RECT_F { left: x + 106.0 * s, top: y + 29.0 * s, right: x + 394.0 * s, bottom: y + 51.0 * s }, br, br_sh.as_ref(), 1.0 * s);
            }
            if let (Some(tf), Some(br)) = (&tf_art, &br_sand) {
                draw_m_text(rt, tf, &media.artist, D2D_RECT_F { left: x + 106.0 * s, top: y + 50.0 * s, right: x + 282.0 * s, bottom: y + 66.0 * s }, br, br_sh.as_ref(), 0.8 * s);
            }
            if cfg.music_show_viz {
                let vz = D2D_RECT_F { left: x + 106.0 * s, top: y + 71.0 * s, right: x + 278.0 * s, bottom: y + 96.0 * s };
                with_viz_slot!(vz.left, vz.top, vz.right, vz.bottom, {
                    let tray_col = if tr4.color2_rgb.is_some() { elem_col_rgba(tr4.color2_rgb, 0.30, rgba_u8(248, 242, 226, 0.90)) } else { rgba_u8(248, 242, 226, 0.90) };
                    if let Ok(pb) = rt.CreateSolidColorBrush(&tray_col, None) {
                        rt.FillRoundedRectangle(&D2D1_ROUNDED_RECT { rect: vz, radiusX: 4.0 * s, radiusY: 4.0 * s }, &pb);
                    }
                    if let Some(br_rgb) = tr4.border_rgb {
                        if let Ok(rb) = rt.CreateSolidColorBrush(&elem_col_rgba(Some(br_rgb), 0.78, tray_col), None) {
                            rt.DrawRoundedRectangle(&D2D1_ROUNDED_RECT { rect: vz, radiusX: 4.0 * s, radiusY: 4.0 * s }, &rb, 1.0 * s, None);
                        }
                    }
                    let ink_col = elem_col_rgba(tr4.color_rgb, 0.92, if cfg.music_use_custom_color { viz_pal.accent_col(0.92) } else { rgba_u8(94, 52, 26, 0.88) });
                    for i in 0..12 {
                        let bx = vz.left + (7.0 + i as f32 * 13.5) * s;
                        let bh = if has_audio { (3.5 + eff_bands[i] * 15.0).min(18.0) } else { 3.0 };
                        if let Ok(ink) = rt.CreateSolidColorBrush(&ink_col, None) {
                            rt.FillRoundedRectangle(&D2D1_ROUNDED_RECT { rect: D2D_RECT_F { left: bx, top: vz.bottom - 3.5 * s - bh * s, right: bx + 7.5 * s, bottom: vz.bottom - 3.5 * s }, radiusX: 1.5 * s, radiusY: 1.5 * s }, &ink);
                        }
                    }
                });
            }
            draw_music_transport(rt, d2d, x, y, s, layout, true, pal, media, hover, br_cream.as_ref());
        }

        MusicStyle::WalnutHiFiConsole => {
            // Hand-Oiled American Black Walnut Stereo Console + PBR Cathedral Grain + Slotted Brass Screws
            let rad_s = (cfg.music_corner_radius as f32 / 14.0).clamp(0.15, 2.5);
            let main_r = D2D_RECT_F { left: x + 2.0 * s, top: y + 2.0 * s, right: x + w - 2.0 * s, bottom: y + h - 2.0 * s };
            let r_card = 12.0 * s * rad_s;
            let glass_a = (cfg.music_glass_alpha as f32 / 255.0).clamp(0.02, 0.99);
            let rim_a = (cfg.music_rim_alpha as f32 / 255.0).clamp(0.0, 1.0);
            let inter = crate::realistic_pbr::pbr_input_from_viz(fx_viz, anim_t, is_card_hov);
            let tint_ov = tr_card.color_rgb.map(|c| (c, tr_card.color2_rgb)).or_else(|| {
                if cfg.music_use_custom_color || cfg.music_match_wallpaper {
                    Some((crate::theme::rgb_to_u32(pal.glass_top), Some(crate::theme::rgb_to_u32(pal.glass_bot))))
                } else {
                    None
                }
            });
            if cfg.music_show_plate && tr_card.visible {
                let mut nb: Option<&mut [ClockElemBox]> = None;
                let g = begin_elem_xf_slot(rt, 5, tr_card, (x, y, x + w, y + h), s, &mut nb);
                crate::realistic_pbr::draw_pbr_drop_shadows(rt, main_r, r_card, s, false);
                crate::realistic_pbr::draw_pbr_surface_clipped(
                    rt,
                    d2d,
                    crate::realistic_pbr::PbrMaterial::VintageWalnutBrass,
                    main_r,
                    r_card,
                    s,
                    (0.45 + glass_a * 0.55).clamp(0.32, 0.99),
                    tint_ov,
                    None,
                    &inter,
                );
                if paint_fx {
                    paint_widget_elemental_fx(rt, d2d, main_r.left, main_r.top, main_r.right, main_r.bottom, (x, y, w, h), r_card, s, cfg.music_hover_fx, 88, pal, false, fx_viz);
                }
                crate::realistic_pbr::draw_pbr_bevel_rim(
                    rt,
                    main_r,
                    r_card,
                    s,
                    crate::realistic_pbr::PbrMaterial::VintageWalnutBrass,
                    tr_card.border_rgb,
                    rim_a.max(0.75),
                    inter.hover_mix,
                    None,
                );
                end_elem_xf_slot(rt, g);
            }
            let cov_r = D2D_RECT_F { left: x + 16.0 * s, top: y + 17.0 * s, right: x + 94.0 * s, bottom: y + 95.0 * s };
            draw_music_cover_art(rt, d2d, cov_r, 8.0 * s, cover_bmp, pal, 1.4, s);
            if let (Some(tf), Some(br)) = (&tf_badge, &br_gold) {
                draw_m_text(rt, tf, "WALNUT STUDIO HI-FI • STEREO", D2D_RECT_F { left: x + 106.0 * s, top: y + 13.0 * s, right: x + 394.0 * s, bottom: y + 27.0 * s }, br, None, 0.0);
            }
            if let (Some(tf), Some(br)) = (&tf_title, &br_cream) {
                draw_m_text(rt, tf, &media.title, D2D_RECT_F { left: x + 106.0 * s, top: y + 28.0 * s, right: x + 394.0 * s, bottom: y + 50.0 * s }, br, br_sh.as_ref(), 1.0 * s);
            }
            if let (Some(tf), Some(br)) = (&tf_art, &br_sand) {
                draw_m_text(rt, tf, &media.artist, D2D_RECT_F { left: x + 106.0 * s, top: y + 48.0 * s, right: x + 282.0 * s, bottom: y + 65.0 * s }, br, br_sh.as_ref(), 0.8 * s);
            }
            if cfg.music_show_viz {
                let vz = D2D_RECT_F { left: x + 106.0 * s, top: y + 70.0 * s, right: x + 278.0 * s, bottom: y + 96.0 * s };
                with_viz_slot!(vz.left, vz.top, vz.right, vz.bottom, {
                    let va = if has_audio { 0.90 } else { 0.36 };
                    let def_c = if cfg.music_use_custom_color || cfg.music_match_wallpaper { viz_pal.light_col(va) } else { rgba_u8(232, 184, 98, va) };
                    let (vc1, vc2, _) = resolve_elem_grad_colors(Some(&tr4), va, def_c, viz_pal.accent_col(va));
                    for i in 0..12 {
                        let bx = vz.left + (4.0 + i as f32 * 14.0) * s;
                        let bh = if has_audio { (4.0 + eff_bands[i] * 20.0).min(22.0) } else { 3.2 };
                        let br_rr = D2D1_ROUNDED_RECT { rect: D2D_RECT_F { left: bx, top: vz.bottom - bh * s, right: bx + 8.0 * s, bottom: vz.bottom }, radiusX: 2.0 * s, radiusY: 2.0 * s };
                        if tr4.color2_rgb.is_some() {
                            let stops = [
                                D2D1_GRADIENT_STOP { position: 0.0, color: vc2 },
                                D2D1_GRADIENT_STOP { position: 1.0, color: vc1 },
                            ];
                            if let Some(bb) = create_linear_grad(rt, (bx, vz.bottom - bh * s), (bx, vz.bottom), &stops) {
                                rt.FillRoundedRectangle(&br_rr, &bb);
                            }
                        } else if let Ok(bb) = rt.CreateSolidColorBrush(&vc1, None) {
                            rt.FillRoundedRectangle(&br_rr, &bb);
                        }
                    }
                });
            }
            draw_music_transport(rt, d2d, x, y, s, layout, true, pal, media, hover, br_cream.as_ref());
        }

        MusicStyle::AbyssalTrenchAudio
        | MusicStyle::GlacierIceAudio
        | MusicStyle::CarbonSteelDeck
        | MusicStyle::GoldenHourAudio => {
            let pbr_mat = cfg
                .music_style
                .pbr_material()
                .unwrap_or(crate::realistic_pbr::PbrMaterial::DeepAbyssalTrench);
            let rad_s = (cfg.music_corner_radius as f32 / 14.0).clamp(0.15, 2.5);
            let main_r = D2D_RECT_F { left: x + 2.0 * s, top: y + 2.0 * s, right: x + w - 2.0 * s, bottom: y + h - 2.0 * s };
            let r_card = match cfg.music_style {
                MusicStyle::CarbonSteelDeck => 10.0 * s * rad_s,
                MusicStyle::GlacierIceAudio => 12.0 * s * rad_s,
                _ => 20.0 * s * rad_s,
            };
            let glass_a = (cfg.music_glass_alpha as f32 / 255.0).clamp(0.02, 0.99);
            let rim_a = (cfg.music_rim_alpha as f32 / 255.0).clamp(0.0, 1.0);
            let inter = crate::realistic_pbr::pbr_input_from_viz(fx_viz, anim_t, is_card_hov);
            let tint_ov = tr_card.color_rgb.map(|c| (c, tr_card.color2_rgb)).or_else(|| {
                if cfg.music_use_custom_color || cfg.music_match_wallpaper {
                    Some((crate::theme::rgb_to_u32(pal.glass_top), Some(crate::theme::rgb_to_u32(pal.glass_bot))))
                } else {
                    None
                }
            });
            if cfg.music_show_plate && tr_card.visible {
                let mut nb: Option<&mut [ClockElemBox]> = None;
                let g = begin_elem_xf_slot(rt, 5, tr_card, (x, y, x + w, y + h), s, &mut nb);
                crate::realistic_pbr::draw_pbr_drop_shadows(rt, main_r, r_card, s, pbr_mat.is_light_surface());
                crate::realistic_pbr::draw_pbr_surface_clipped(
                    rt,
                    d2d,
                    pbr_mat,
                    main_r,
                    r_card,
                    s,
                    (0.45 + glass_a * 0.55).clamp(0.32, 0.99),
                    tint_ov,
                    None,
                    &inter,
                );
                if paint_fx {
                    paint_widget_elemental_fx(rt, d2d, main_r.left, main_r.top, main_r.right, main_r.bottom, (x, y, w, h), r_card, s, cfg.music_hover_fx, 88, pal, false, fx_viz);
                }
                crate::realistic_pbr::draw_pbr_bevel_rim(
                    rt,
                    main_r,
                    r_card,
                    s,
                    pbr_mat,
                    tr_card.border_rgb,
                    rim_a.max(0.75),
                    inter.hover_mix,
                    None,
                );
                end_elem_xf_slot(rt, g);
            }
            let (badge_txt, acc_col, sq_btn, cov_rad) = match cfg.music_style {
                MusicStyle::AbyssalTrenchAudio => ("DEEP ABYSSAL • BIOLUM SONAR", rgba_u8(0, 255, 204, 0.94), false, 39.0 * s),
                MusicStyle::GlacierIceAudio => ("NORDIC GLACIAL • PRISM AUDIO", rgba_u8(160, 231, 229, 0.94), false, 12.0 * s),
                MusicStyle::CarbonSteelDeck => ("CARBON & STEEL • AERO DAC", rgba_u8(255, 107, 53, 0.96), true, 8.0 * s),
                _ => ("GOLDEN HOUR • COASTAL WAVE", rgba_u8(255, 209, 102, 0.96), false, 14.0 * s),
            };
            let cov_r = D2D_RECT_F { left: x + 16.0 * s, top: y + 17.0 * s, right: x + 94.0 * s, bottom: y + 95.0 * s };
            draw_music_cover_art(rt, d2d, cov_r, cov_rad, cover_bmp, pal, 1.4, s);
            if let (Some(tf), Ok(ab)) = (&tf_badge, rt.CreateSolidColorBrush(&if cfg.music_use_custom_color { pal.accent_col(0.95) } else { acc_col }, None)) {
                draw_m_text(rt, tf, badge_txt, D2D_RECT_F { left: x + 106.0 * s, top: y + 13.0 * s, right: x + 394.0 * s, bottom: y + 27.0 * s }, &ab, None, 0.0);
            }
            if let (Some(tf), Some(br)) = (&tf_title, &br_cream) {
                draw_m_text(rt, tf, &media.title, D2D_RECT_F { left: x + 106.0 * s, top: y + 28.0 * s, right: x + 394.0 * s, bottom: y + 50.0 * s }, br, br_sh.as_ref(), 1.0 * s);
            }
            if let (Some(tf), Some(br)) = (&tf_art, &br_sand) {
                draw_m_text(rt, tf, &media.artist, D2D_RECT_F { left: x + 106.0 * s, top: y + 48.0 * s, right: x + 284.0 * s, bottom: y + 65.0 * s }, br, br_sh.as_ref(), 0.8 * s);
            }
            if cfg.music_show_viz {
                let vz = D2D_RECT_F { left: x + 106.0 * s, top: y + 68.0 * s, right: x + 278.0 * s, bottom: y + 97.0 * s };
                with_viz_slot!(vz.left, vz.top, vz.right, vz.bottom, {
                    let va = if has_audio { 0.90 } else { 0.42 };
                    let def_c = if cfg.music_use_custom_color || cfg.music_match_wallpaper {
                        viz_pal.light_col(va)
                    } else {
                        windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F {
                            r: acc_col.r,
                            g: acc_col.g,
                            b: acc_col.b,
                            a: va,
                        }
                    };
                    let (vc1, vc2, _) = resolve_elem_grad_colors(Some(&tr4), va, def_c, viz_pal.accent_col(va));
                    for i in 0..12 {
                        let bx = vz.left + (4.0 + i as f32 * 14.0) * s;
                        let bh = if has_audio { (4.0 + eff_bands[i] * 21.0).min(24.0) } else { 4.0 + ((anim_t * 2.2 + i as f32 * 0.6).sin().abs() * 6.0) };
                        let br_rr = D2D1_ROUNDED_RECT {
                            rect: D2D_RECT_F { left: bx, top: vz.bottom - bh * s, right: bx + 7.5 * s, bottom: vz.bottom },
                            radiusX: if sq_btn { 1.5 * s } else { 3.5 * s },
                            radiusY: if sq_btn { 1.5 * s } else { 3.5 * s },
                        };
                        if tr4.color2_rgb.is_some() {
                            let stops = [
                                D2D1_GRADIENT_STOP { position: 0.0, color: vc2 },
                                D2D1_GRADIENT_STOP { position: 1.0, color: vc1 },
                            ];
                            if let Some(bb) = create_linear_grad(rt, (bx, vz.bottom - bh * s), (bx, vz.bottom), &stops) {
                                rt.FillRoundedRectangle(&br_rr, &bb);
                            }
                        } else if let Ok(bb) = rt.CreateSolidColorBrush(&vc1, None) {
                            rt.FillRoundedRectangle(&br_rr, &bb);
                        }
                    }
                });
            }
            draw_music_transport(rt, d2d, x, y, s, layout, sq_btn, pal, media, hover, br_cream.as_ref());
        }

        horo_m if horo_m.horology_theme().is_some() => {
            let ht = horo_m.horology_theme().unwrap();
            let custom_acc = if cfg.music_use_custom_color || cfg.music_match_wallpaper {
                Some(crate::theme::rgb_to_u32(pal.accent))
            } else {
                None
            };
            let custom_txt = if cfg.music_use_custom_color {
                Some(cfg.music_text_rgb)
            } else {
                None
            };
            let custom_bg = tr_card.color_rgb.or_else(|| {
                if cfg.music_use_custom_color || cfg.music_match_wallpaper {
                    Some(crate::theme::rgb_to_u32(pal.glass_top))
                } else {
                    None
                }
            });
            let mut hpal = cfg.music_horology.resolve(ht, custom_acc, custom_txt, custom_bg);
            if let Some(bc) = tr_card.border_rgb {
                hpal.bezel_rgb = bc;
                hpal.bezel_hi_rgb = crate::horology_pbr::lighten_rgb(bc, 0.36);
                hpal.bezel_lo_rgb = crate::horology_pbr::darken_rgb(bc, 0.45);
            }
            let main_r = D2D_RECT_F {
                left: x + 2.0 * s,
                top: y + 2.0 * s,
                right: x + w - 2.0 * s,
                bottom: y + h - 2.0 * s,
            };
            let r_card = ((cfg.music_corner_radius as f32).clamp(4.0, 32.0)) * s;
            let op = (0.55 + (cfg.music_glass_alpha as f32 / 220.0)).clamp(0.55, 0.99);
            let rim_a = (cfg.music_rim_alpha as f32 / 255.0).clamp(0.0, 1.0);
            let inter = crate::realistic_pbr::pbr_input_from_viz(fx_viz, anim_t, is_card_hov);

            // 1. Machined Horology Turntable Case (Card slot 5)
            if cfg.music_show_plate && tr_card.visible && !skip_plate_and_fx {
                let mut nb: Option<&mut [ClockElemBox]> = None;
                let g = begin_elem_xf_slot(rt, 5, tr_card, (x, y, x + w, y + h), s, &mut nb);
                crate::horology_pbr::draw_horology_case_plate(
                    rt,
                    d2d,
                    main_r,
                    r_card,
                    s,
                    &hpal,
                    op,
                    rim_a,
                    true,
                    inter.hover_x,
                    inter.hover_y,
                    inter.hover_mix,
                );
                if paint_fx {
                    paint_widget_elemental_fx(
                        rt,
                        d2d,
                        main_r.left,
                        main_r.top,
                        main_r.right,
                        main_r.bottom,
                        (x, y, w, h),
                        r_card,
                        s,
                        cfg.music_hover_fx,
                        88,
                        pal,
                        false,
                        fx_viz,
                    );
                }
                end_elem_xf_slot(rt, g);
            }

            // 2. Left Bespoke Horology Cover Platter (Cover slot 0 — unique bezel per archetype)
            let cov_r = D2D_RECT_F {
                left: x + 16.0 * s,
                top: y + 17.0 * s,
                right: x + 98.0 * s,
                bottom: y + 99.0 * s,
            };
            let cov_cx = (cov_r.left + cov_r.right) * 0.5;
            let cov_cy = (cov_r.top + cov_r.bottom) * 0.5;
            let cov_rad = 41.0 * s;
            let spin_deg = if media.is_playing { (anim_t * 48.0) % 360.0 } else { -90.0 };
            if cfg.music_show_cover && tr0.visible {
                match ht {
                    crate::horology_pbr::HorologyTheme::RoyalOakSteel => {
                        // Brushed steel circular bezel + 8 white-gold miniature bezel bolts
                        if let Ok(bz) = rt.CreateSolidColorBrush(&crate::horology_pbr::rgb_u32(hpal.bezel_hi_rgb, 0.95), None) {
                            rt.DrawEllipse(&D2D1_ELLIPSE { point: Vector2 { X: cov_cx, Y: cov_cy }, radiusX: cov_rad + 3.2 * s, radiusY: cov_rad + 3.2 * s }, &bz, 3.2 * s, None);
                        }
                        draw_music_cover_art(rt, d2d, cov_r, cov_rad, cover_bmp, pal, 1.5, s);
                        if hpal.show_screws {
                            for bi in 0..8 {
                                let a = (bi as f32 * 45.0 + 22.5).to_radians();
                                crate::horology_pbr::draw_horology_hex_screw(rt, d2d, cov_cx + (cov_rad + 3.0 * s) * a.cos(), cov_cy + (cov_rad + 3.0 * s) * a.sin(), 2.2 * s, a + 1.57, s, 0xEEF3FA);
                            }
                        }
                    }
                    crate::horology_pbr::HorologyTheme::TourbillonGold => {
                        // Exposed rotating skeleton brass drive gear behind + live tourbillon on center
                        crate::horology_pbr::draw_horology_gear_wheel(rt, cov_cx + 28.0 * s, cov_cy - 18.0 * s, 26.0 * s, 14, anim_t * 0.45, s, 0xD8A842, 0.48);
                        if let Ok(bz) = rt.CreateSolidColorBrush(&crate::horology_pbr::rgb_u32(0xFFD678, 0.96), None) {
                            rt.DrawEllipse(&D2D1_ELLIPSE { point: Vector2 { X: cov_cx, Y: cov_cy }, radiusX: cov_rad + 2.6 * s, radiusY: cov_rad + 2.6 * s }, &bz, 2.8 * s, None);
                        }
                        draw_music_cover_art(rt, d2d, cov_r, cov_rad, cover_bmp, pal, 1.5, s);
                        crate::horology_pbr::draw_horology_tourbillon_cage(rt, cov_cx, cov_cy, 15.0 * s, s, &hpal, anim_t);
                    }
                    crate::horology_pbr::HorologyTheme::DaytonaCeramic => {
                        // Thick jet-black ceramic tachymeter ring + crimson racing sweep
                        if let Ok(cer) = rt.CreateSolidColorBrush(&crate::horology_pbr::rgb_u32(0x0B0C10, 0.98), None) {
                            rt.DrawEllipse(&D2D1_ELLIPSE { point: Vector2 { X: cov_cx, Y: cov_cy }, radiusX: cov_rad + 3.5 * s, radiusY: cov_rad + 3.5 * s }, &cer, 5.5 * s, None);
                        }
                        draw_music_cover_art(rt, d2d, cov_r, cov_rad, cover_bmp, pal, 1.5, s);
                        if let Ok(rb) = rt.CreateSolidColorBrush(&crate::horology_pbr::rgb_u32(0xFF2828, 0.96), None) {
                            let rb_b: &windows::Win32::Graphics::Direct2D::ID2D1Brush = &rb;
                            draw_music_arc(rt, d2d, cov_cx, cov_cy, cov_rad + 3.5 * s, spin_deg, spin_deg + 145.0, 2.4 * s, rb_b, cap_round.as_ref());
                        }
                    }
                    crate::horology_pbr::HorologyTheme::SubmarinerEmerald => {
                        // Emerald serrated diver bezel + glowing 12-o'clock lume pearl
                        if let Ok(em) = rt.CreateSolidColorBrush(&crate::horology_pbr::rgb_u32(0x0B7A4B, 0.98), None) {
                            rt.DrawEllipse(&D2D1_ELLIPSE { point: Vector2 { X: cov_cx, Y: cov_cy }, radiusX: cov_rad + 3.5 * s, radiusY: cov_rad + 3.5 * s }, &em, 5.2 * s, None);
                        }
                        draw_music_cover_art(rt, d2d, cov_r, cov_rad, cover_bmp, pal, 1.5, s);
                        if let Ok(lp) = rt.CreateSolidColorBrush(&crate::horology_pbr::rgb_u32(0x52FFA8, 0.98), None) {
                            rt.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: cov_cx, Y: cov_cy - cov_rad - 3.5 * s }, radiusX: 2.8 * s, radiusY: 2.8 * s }, &lp);
                        }
                    }
                    crate::horology_pbr::HorologyTheme::IwcPilotTitanium => {
                        // Square bolted titanium avionics cover frame
                        let sq_r = D2D1_ROUNDED_RECT { rect: D2D_RECT_F { left: cov_r.left - 2.0 * s, top: cov_r.top - 2.0 * s, right: cov_r.right + 2.0 * s, bottom: cov_r.bottom + 2.0 * s }, radiusX: 6.0 * s, radiusY: 6.0 * s };
                        if let Ok(ti) = rt.CreateSolidColorBrush(&crate::horology_pbr::rgb_u32(0x88929C, 0.90), None) {
                            rt.DrawRoundedRectangle(&sq_r, &ti, 2.4 * s, None);
                        }
                        draw_music_cover_art(rt, d2d, cov_r, 6.0 * s, cover_bmp, pal, 1.5, s);
                    }
                    crate::horology_pbr::HorologyTheme::BreguetPorcelain => {
                        // Thin silver-and-cobalt railway ring around cover
                        if let Ok(cb) = rt.CreateSolidColorBrush(&crate::horology_pbr::rgb_u32(0x1B4FD8, 0.88), None) {
                            rt.DrawEllipse(&D2D1_ELLIPSE { point: Vector2 { X: cov_cx, Y: cov_cy }, radiusX: cov_rad + 2.5 * s, radiusY: cov_rad + 2.5 * s }, &cb, 1.6 * s, None);
                        }
                        draw_music_cover_art(rt, d2d, cov_r, cov_rad, cover_bmp, pal, 1.5, s);
                    }
                    crate::horology_pbr::HorologyTheme::ForgedCarbonMoto => {
                        // Angular carbon bezel + blazing neon-orange arc
                        if let Ok(ob) = rt.CreateSolidColorBrush(&crate::horology_pbr::rgb_u32(0xFF5E1A, 0.96), None) {
                            let ob_b: &windows::Win32::Graphics::Direct2D::ID2D1Brush = &ob;
                            draw_music_arc(rt, d2d, cov_cx, cov_cy, cov_rad + 3.2 * s, 135.0, 405.0, 2.6 * s, ob_b, cap_round.as_ref());
                        }
                        draw_music_cover_art(rt, d2d, cov_r, 12.0 * s, cover_bmp, pal, 1.5, s);
                    }
                    crate::horology_pbr::HorologyTheme::WalnutRegulator => {
                        // Warm double antique brass vintage turntable bezel
                        if let Ok(br) = rt.CreateSolidColorBrush(&crate::horology_pbr::rgb_u32(0xE8BE5E, 0.96), None) {
                            rt.DrawEllipse(&D2D1_ELLIPSE { point: Vector2 { X: cov_cx, Y: cov_cy }, radiusX: cov_rad + 3.4 * s, radiusY: cov_rad + 3.4 * s }, &br, 3.0 * s, None);
                            rt.DrawEllipse(&D2D1_ELLIPSE { point: Vector2 { X: cov_cx, Y: cov_cy }, radiusX: cov_rad + 0.5 * s, radiusY: cov_rad + 0.5 * s }, &br, 0.9 * s, None);
                        }
                        draw_music_cover_art(rt, d2d, cov_r, cov_rad, cover_bmp, pal, 1.5, s);
                    }
                    crate::horology_pbr::HorologyTheme::SapphireObsidian => {
                        // Frameless prismatic cyan-to-violet optical halo
                        if let Ok(cy_b) = rt.CreateSolidColorBrush(&crate::horology_pbr::rgb_u32(0x66EEFF, 0.94), None) {
                            let cb_b: &windows::Win32::Graphics::Direct2D::ID2D1Brush = &cy_b;
                            draw_music_arc(rt, d2d, cov_cx, cov_cy, cov_rad + 3.2 * s, spin_deg, spin_deg + 165.0, 2.2 * s, cb_b, cap_round.as_ref());
                        }
                        if let Ok(vi_b) = rt.CreateSolidColorBrush(&crate::horology_pbr::rgb_u32(0xA066FF, 0.80), None) {
                            let vb_b: &windows::Win32::Graphics::Direct2D::ID2D1Brush = &vi_b;
                            draw_music_arc(rt, d2d, cov_cx, cov_cy, cov_rad + 3.2 * s, spin_deg + 180.0, spin_deg + 335.0, 1.8 * s, vb_b, cap_round.as_ref());
                        }
                        draw_music_cover_art(rt, d2d, cov_r, cov_rad, cover_bmp, pal, 1.5, s);
                    }
                    crate::horology_pbr::HorologyTheme::PatekMoonphase => {
                        // Rose-gold double dress rim + golden spin arc
                        if let Ok(rg) = rt.CreateSolidColorBrush(&crate::horology_pbr::rgb_u32(0xF0AE82, 0.96), None) {
                            rt.DrawEllipse(&D2D1_ELLIPSE { point: Vector2 { X: cov_cx, Y: cov_cy }, radiusX: cov_rad + 2.8 * s, radiusY: cov_rad + 2.8 * s }, &rg, 2.6 * s, None);
                            let rg_b: &windows::Win32::Graphics::Direct2D::ID2D1Brush = &rg;
                            draw_music_arc(rt, d2d, cov_cx, cov_cy, cov_rad + 4.8 * s, spin_deg, spin_deg + 120.0, 1.4 * s, rg_b, cap_round.as_ref());
                        }
                        draw_music_cover_art(rt, d2d, cov_r, cov_rad, cover_bmp, pal, 1.5, s);
                    }
                }
            }

            // 3. Engraved Caliber Header, Title & Artist (slots 1 & 2)
            let br_htext = rt.CreateSolidColorBrush(&crate::horology_pbr::rgb_u32(hpal.text_rgb, 0.98), None).ok();
            let br_hsub = rt.CreateSolidColorBrush(&crate::horology_pbr::rgb_u32(hpal.subtext_rgb, 0.92), None).ok();
            let br_hlume = rt.CreateSolidColorBrush(&crate::horology_pbr::rgb_u32(hpal.lume_rgb, 0.96), None).ok();
            let sh_m = if hpal.is_light_dial { None } else { br_sh.as_ref() };

            let badge_txt = match ht {
                crate::horology_pbr::HorologyTheme::RoyalOakSteel => "ROYAL OAK  •  CALIBRE 4401",
                crate::horology_pbr::HorologyTheme::PatekMoonphase => "GENÈVE  •  MOONPHASE AUDIO",
                crate::horology_pbr::HorologyTheme::TourbillonGold => "TOURBILLON  •  LIVE ESCAPEMENT",
                crate::horology_pbr::HorologyTheme::DaytonaCeramic => "DAYTONA  •  PANDA CHRONO AUDIO",
                crate::horology_pbr::HorologyTheme::SubmarinerEmerald => "300M / 1000FT  •  OYSTER AUDIO",
                crate::horology_pbr::HorologyTheme::IwcPilotTitanium => "FLIGHT DECK  •  AVIONICS COMMS",
                crate::horology_pbr::HorologyTheme::BreguetPorcelain => "GRAND FEU  •  MAISON AUDIO",
                crate::horology_pbr::HorologyTheme::ForgedCarbonMoto => "FORGED CARBON  •  RPM TELEMETRY",
                crate::horology_pbr::HorologyTheme::WalnutRegulator => "HERITAGE HI-FI  •  ANALOG VU",
                crate::horology_pbr::HorologyTheme::SapphireObsidian => "SAPPHIRE CRYSTAL  •  PRISM DECK",
            };
            if let (Some(tf), Some(br)) = (&tf_badge, &br_hlume) {
                draw_m_text(
                    rt,
                    tf,
                    badge_txt,
                    D2D_RECT_F { left: x + 112.0 * s, top: y + 12.0 * s, right: x + 404.0 * s, bottom: y + 25.0 * s },
                    br,
                    None,
                    0.0,
                );
            }
            if let (Some(tf), Some(br)) = (&tf_title, &br_htext) {
                draw_m_text(
                    rt,
                    tf,
                    &media.title,
                    D2D_RECT_F { left: x + 112.0 * s, top: y + 26.0 * s, right: x + 404.0 * s, bottom: y + 48.0 * s },
                    br,
                    sh_m,
                    0.9 * s,
                );
            }
            if let (Some(tf), Some(br)) = (&tf_art, &br_hsub) {
                draw_m_text(
                    rt,
                    tf,
                    &media.artist,
                    D2D_RECT_F { left: x + 112.0 * s, top: y + 46.0 * s, right: x + 286.0 * s, bottom: y + 62.0 * s },
                    br,
                    sh_m,
                    0.6 * s,
                );
            }

            // 4. Bespoke Theme-Specific Audio Visualizer & Meters (Prog/EQ slot 4)
            if cfg.music_show_viz {
                let vz = D2D_RECT_F {
                    left: x + 112.0 * s,
                    top: y + 63.0 * s,
                    right: x + 282.0 * s,
                    bottom: y + 107.0 * s,
                };
                with_viz_slot!(vz.left, vz.top, vz.right, vz.bottom, {
                    let lv = if has_audio {
                        ((eff_bands[0] + eff_bands[1] + eff_bands[2] + eff_bands[3]) * 0.25).clamp(0.05, 1.0)
                    } else if media.is_playing {
                        0.32 + (anim_t * 2.4).sin().abs() * 0.28
                    } else {
                        0.12
                    };
                    let rv = if has_audio {
                        ((eff_bands[6] + eff_bands[7] + eff_bands[8] + eff_bands[9]) * 0.25).clamp(0.05, 1.0)
                    } else if media.is_playing {
                        0.28 + (anim_t * 2.1 + 1.1).sin().abs() * 0.30
                    } else {
                        0.10
                    };
                    let eq_col = tr4.color_rgb.unwrap_or(hpal.lume_rgb);
                    let vu_cy = (vz.top + vz.bottom) * 0.5;
                    match ht {
                        crate::horology_pbr::HorologyTheme::WalnutRegulator => {
                            // Iconic Vintage Hi-Fi Dual Rectangular Warm-Lit Parchment Analog VU Windows!
                            for (vi, val) in [lv, rv].iter().enumerate() {
                                let vx0 = vz.left + vi as f32 * 86.0 * s;
                                let wr = D2D_RECT_F { left: vx0, top: vz.top + 4.0 * s, right: vx0 + 81.0 * s, bottom: vz.bottom - 3.0 * s };
                                let wrr = D2D1_ROUNDED_RECT { rect: wr, radiusX: 4.0 * s, radiusY: 4.0 * s };
                                if let Ok(pb) = rt.CreateSolidColorBrush(&crate::horology_pbr::rgb_u32(0xF6EAD0, 0.97), None) {
                                    rt.FillRoundedRectangle(&wrr, &pb);
                                }
                                if let Ok(bb) = rt.CreateSolidColorBrush(&crate::horology_pbr::rgb_u32(0xDFB158, 0.96), None) {
                                    rt.DrawRoundedRectangle(&wrr, &bb, 1.8 * s, None);
                                }
                                // Scale arc ticks + pivoting espresso analog needle from bottom-center
                                let ncx = (wr.left + wr.right) * 0.5;
                                let ncy = wr.bottom + 5.0 * s;
                                let nrad = 30.0 * s;
                                for ti in 0..=8 {
                                    let ang = (-142.0 + ti as f32 * 13.0).to_radians();
                                    let tc = if ti >= 6 { 0xC82820 } else { 0x3A2416 };
                                    if let Ok(tb) = rt.CreateSolidColorBrush(&crate::horology_pbr::rgb_u32(tc, 0.88), None) {
                                        rt.DrawLine(
                                            Vector2 { X: ncx + (nrad - 3.5 * s) * ang.cos(), Y: ncy + (nrad - 3.5 * s) * ang.sin() },
                                            Vector2 { X: ncx + nrad * ang.cos(), Y: ncy + nrad * ang.sin() },
                                            &tb,
                                            (1.0 * s).max(0.8),
                                            None,
                                        );
                                    }
                                }
                                let nang = (-138.0 + val.clamp(0.0, 1.0) * 96.0).to_radians();
                                if let Ok(nb) = rt.CreateSolidColorBrush(&crate::horology_pbr::rgb_u32(0x1E1008, 0.96), None) {
                                    rt.DrawLine(
                                        Vector2 { X: ncx, Y: wr.bottom - 1.5 * s },
                                        Vector2 { X: ncx + (nrad - 2.0 * s) * nang.cos(), Y: ncy + (nrad - 2.0 * s) * nang.sin() },
                                        &nb,
                                        1.5 * s,
                                        None,
                                    );
                                }
                            }
                        }
                        crate::horology_pbr::HorologyTheme::ForgedCarbonMoto => {
                            // Hypercar Twin Segmented Glowing Neon-Orange RPM Arcs + Redline Boost Bar
                            crate::horology_pbr::draw_horology_rpm_arc(rt, vz.left + 26.0 * s, vu_cy + 3.0 * s, 20.0 * s, 150.0, 240.0, 18, lv, s, eq_col);
                            crate::horology_pbr::draw_horology_rpm_arc(rt, vz.left + 76.0 * s, vu_cy + 3.0 * s, 20.0 * s, 150.0, 240.0, 18, rv, s, eq_col);
                            if let Ok(ob) = rt.CreateSolidColorBrush(&crate::horology_pbr::rgb_u32(eq_col, 0.95), None) {
                                for i in 0..8 {
                                    let val = if has_audio { eff_bands[(i * 12 / 8).min(11)].clamp(0.08, 1.0) } else if media.is_playing { 0.25 + ((anim_t * 3.1 + i as f32 * 0.7).sin().abs() * 0.55) } else { 0.16 };
                                    let bx = vz.left + 104.0 * s + i as f32 * 8.2 * s;
                                    let bh = (32.0 * s * val).max(3.0 * s);
                                    rt.FillRectangle(&D2D_RECT_F { left: bx, top: vz.bottom - 4.0 * s - bh, right: bx + 5.2 * s, bottom: vz.bottom - 4.0 * s }, &ob);
                                }
                            }
                        }
                        crate::horology_pbr::HorologyTheme::IwcPilotTitanium => {
                            // Cockpit Avionics Dual Horizontal Segmented dB Telemetry Bars + Amber Carets
                            for (ci, val) in [lv, rv].iter().enumerate() {
                                let by = vz.top + 6.0 * s + ci as f32 * 18.0 * s;
                                let bar_r = D2D_RECT_F { left: vz.left, top: by, right: vz.right, bottom: by + 13.0 * s };
                                crate::horology_pbr::draw_horology_aperture(rt, bar_r, 3.0 * s, s, &hpal, false);
                                let segs = 16usize;
                                let lit = ((val * segs as f32).ceil() as usize).clamp(1, segs);
                                let step = (bar_r.right - bar_r.left - 8.0 * s) / segs as f32;
                                for si in 0..segs {
                                    let col = if si < lit {
                                        if si >= 13 { 0xFF6A1A } else { eq_col }
                                    } else {
                                        0x222930
                                    };
                                    if let Ok(sb) = rt.CreateSolidColorBrush(&crate::horology_pbr::rgb_u32(col, if si < lit { 0.96 } else { 0.55 }), None) {
                                        let sx = bar_r.left + 4.0 * s + si as f32 * step;
                                        rt.FillRectangle(&D2D_RECT_F { left: sx, top: by + 3.0 * s, right: sx + step * 0.72, bottom: by + 10.0 * s }, &sb);
                                    }
                                }
                            }
                        }
                        crate::horology_pbr::HorologyTheme::SapphireObsidian => {
                            // Frameless Floating 16-Bar Holographic Cyan-to-Violet Crystal Spectrum (no box!)
                            let bars = 16usize;
                            let step = ((vz.right - vz.left) / bars as f32).max(5.0);
                            for i in 0..bars {
                                let val = if has_audio { eff_bands[(i * 12 / bars).min(11)].clamp(0.10, 1.0) } else if media.is_playing { 0.18 + ((anim_t * 3.0 + i as f32 * 0.52).sin().abs() * 0.65) } else { 0.14 };
                                let col = if i % 3 == 0 { 0xA86BFF } else { eq_col };
                                if let Ok(sb) = rt.CreateSolidColorBrush(&crate::horology_pbr::rgb_u32(col, 0.90), None) {
                                    let bh = (34.0 * s * val).max(3.0 * s);
                                    let bx = vz.left + i as f32 * step + 1.0 * s;
                                    let cy_b = (vz.top + vz.bottom) * 0.5;
                                    rt.FillRoundedRectangle(
                                        &D2D1_ROUNDED_RECT {
                                            rect: D2D_RECT_F { left: bx, top: cy_b - bh * 0.5, right: bx + step * 0.58, bottom: cy_b + bh * 0.5 },
                                            radiusX: 2.0 * s,
                                            radiusY: 2.0 * s,
                                        },
                                        &sb,
                                    );
                                }
                            }
                        }
                        crate::horology_pbr::HorologyTheme::BreguetPorcelain => {
                            // Grand Feu Open Porcelain Twin Blued-Steel Meters + Fine Cobalt Filigree Wave
                            crate::horology_pbr::draw_horology_subdial(rt, d2d, vz.left + 22.0 * s, vu_cy, 19.0 * s, s, &hpal, lv, Some(0.80), tr4.color_rgb);
                            crate::horology_pbr::draw_horology_subdial(rt, d2d, vz.left + 68.0 * s, vu_cy, 19.0 * s, s, &hpal, rv, Some(0.80), tr4.color2_rgb.or(tr4.color_rgb));
                            // Open cobalt acoustic wave directly on porcelain (no dark box)
                            if let Ok(cb) = rt.CreateSolidColorBrush(&crate::horology_pbr::rgb_u32(0x1B4FD8, 0.85), None) {
                                let wx0 = vz.left + 94.0 * s;
                                let wx1 = vz.right;
                                let mut prev = None;
                                for i in 0..=18 {
                                    let f = i as f32 / 18.0;
                                    let amp = (0.35 + (lv + rv) * 0.45) * 12.0 * s * (f * std::f32::consts::PI).sin();
                                    let pt = Vector2 { X: wx0 + (wx1 - wx0) * f, Y: vu_cy + (anim_t * 4.0 + f * 12.0).sin() * amp };
                                    if let Some(p0) = prev {
                                        rt.DrawLine(p0, pt, &cb, 1.5 * s, None);
                                    }
                                    prev = Some(pt);
                                }
                            }
                        }
                        _ => {
                            // Twin Ballistic Analog Horology VU Sub-Dials + Recessed Spectrum (automatically Panda white on Daytona, Diver dot on Submariner, Gear on Tourbillon, Rose Gold on Patek, Octagonal on Royal Oak!)
                            let vu_r = 18.5 * s;
                            crate::horology_pbr::draw_horology_subdial(rt, d2d, vz.left + 21.0 * s, vu_cy, vu_r, s, &hpal, lv, Some(0.78), tr4.color_rgb);
                            crate::horology_pbr::draw_horology_subdial(rt, d2d, vz.left + 64.0 * s, vu_cy, vu_r, s, &hpal, rv, Some(0.78), tr4.color2_rgb.or(tr4.color_rgb));
                            let eq_r = D2D_RECT_F {
                                left: vz.left + 90.0 * s,
                                top: vz.top + 6.0 * s,
                                right: vz.right,
                                bottom: vz.bottom - 5.0 * s,
                            };
                            crate::horology_pbr::draw_horology_aperture(rt, eq_r, 5.0 * s, s, &hpal, ht == crate::horology_pbr::HorologyTheme::SubmarinerEmerald);
                            if let Ok(eb) = rt.CreateSolidColorBrush(&crate::horology_pbr::rgb_u32(eq_col, 0.92), None) {
                                let inner_h = (eq_r.bottom - eq_r.top - 6.0 * s).max(4.0);
                                let step = ((eq_r.right - eq_r.left - 8.0 * s) / 8.0).max(4.0);
                                for i in 0..8 {
                                    let val = if has_audio {
                                        eff_bands[(i * 12 / 8).min(11)].clamp(0.08, 1.0)
                                    } else if media.is_playing {
                                        0.20 + ((anim_t * 2.8 + i as f32 * 0.7).sin().abs() * 0.45)
                                    } else {
                                        0.14
                                    };
                                    let bh = (inner_h * val).max(2.5 * s);
                                    let bx = eq_r.left + 4.0 * s + i as f32 * step;
                                    rt.FillRoundedRectangle(
                                        &D2D1_ROUNDED_RECT {
                                            rect: D2D_RECT_F {
                                                left: bx,
                                                top: eq_r.bottom - 3.0 * s - bh,
                                                right: bx + (step * 0.62).max(2.0),
                                                bottom: eq_r.bottom - 3.0 * s,
                                            },
                                            radiusX: 1.4 * s,
                                            radiusY: 1.4 * s,
                                        },
                                        &eb,
                                    );
                                }
                            }
                        }
                    }
                });
            }

            // 5. Machined Horology Crown Transport Pushers (Controls slot 3 — exact music_button_layout sync)
            let ctrl_pal = ThemePalette::for_horology(ht);
            draw_music_transport(
                rt,
                d2d,
                x,
                y,
                s,
                layout,
                false,
                &ctrl_pal,
                media,
                hover,
                br_htext.as_ref().or(br_cream.as_ref()),
            );

            // 6. Curved Sapphire AR Glare across the case
            if cfg.music_show_plate && tr_card.visible && !skip_plate_and_fx {
                crate::horology_pbr::draw_sapphire_ar_glare(
                    rt,
                    d2d,
                    main_r,
                    r_card,
                    s,
                    &hpal,
                    inter.hover_x,
                    inter.hover_mix,
                );
            }
        }

        _ => {}
    }

    true
}

unsafe fn music_effective_palette(cfg: &AppConfig) -> ThemePalette {
    crate::theme::resolve_music_palette(cfg)
}

unsafe fn music_fg_key(
    s: f32,
    cfg: &AppConfig,
    pal: &ThemePalette,
    media: &MediaSnapshot,
    cover_bmp: Option<&ID2D1Bitmap>,
    hover: WidgetHitZone,
    has_custom_fonts: bool,
) -> u64 {
    let mut h = FgKeyHasher::new(0xB451_C001);
    h.mix_f32(s);
    h.mix_bool(has_custom_fonts);
    h.mix_pal(pal);
    let hov_code: u8 = match hover {
        WidgetHitZone::MusicCard => 1,
        WidgetHitZone::MusicPrev => 2,
        WidgetHitZone::MusicPlayPause => 3,
        WidgetHitZone::MusicNext => 4,
        _ => 0,
    };
    h.mix_u8(hov_code);
    h.mix_u8(cfg.music_style as u8);
    h.mix_u8(cfg.music_scale_pct);
    h.mix_u8(cfg.music_glass_alpha);
    h.mix_u8(cfg.music_rim_alpha);
    h.mix_bool(cfg.music_show_viz);
    h.mix_u8(cfg.music_corner_radius);
    h.mix_bool(cfg.music_show_plate);
    h.mix_bool(cfg.music_show_cover);
    h.mix_bool(cfg.music_show_title);
    h.mix_bool(cfg.music_show_artist);
    h.mix_bool(cfg.music_show_controls);
    h.mix_bool(cfg.music_show_badge);
    h.mix_bool(cfg.music_match_wallpaper);
    h.mix_bool(cfg.music_use_custom_color);
    h.mix_u64(cfg.music_accent_rgb as u64);
    h.mix_u64(cfg.music_text_rgb as u64);
    h.mix_u64(cfg.music_bg_rgb as u64);
    h.mix_str(&cfg.music_font_title);
    h.mix_str(&cfg.music_font_artist);
    h.mix_u8(cfg.music_font_scale_pct);
    for f in &cfg.music_elem_fonts {
        h.mix_str(f);
    }
    for tr in &cfg.music_elem_transforms {
        h.mix_tr(tr);
    }
    h.mix_horology(&cfg.music_horology);
    h.mix_str(&media.title);
    h.mix_str(&media.artist);
    h.mix_bool(media.is_playing);
    h.mix_u64(media.cover_seq);
    h.mix_usize(cover_bmp.map(|b| b.as_raw() as usize).unwrap_or(0));
    h.finish()
}

#[allow(clippy::too_many_arguments)]
unsafe fn draw_music_standard_plate(
    rt: &ID2D1RenderTarget,
    d2d: Option<&ID2D1Factory>,
    x: f32,
    y: f32,
    s: f32,
    cfg: &AppConfig,
    pal: &ThemePalette,
    fx_viz: Option<&VizSnapshot>,
    paint_fx: bool,
) {
    let w = 410.0 * s;
    let h = 92.0 * s;
    let m_rect = (x, y, x + w, y + h);
    let tr_card = cfg.music_elem_transforms[5];
    let style = cfg.music_style;
    let rad_scale = (cfg.music_corner_radius as f32 / 22.0).clamp(0.0, 2.2);
    let card_rad = match style {
        MusicStyle::MinimalPill => 46.0 * s * rad_scale.min(1.2),
        MusicStyle::CyberHud => 9.0 * s * rad_scale,
        _ => (cfg.music_corner_radius as f32) * s,
    };
    let card_rr = D2D1_ROUNDED_RECT {
        rect: D2D_RECT_F {
            left: x,
            top: y,
            right: x + w,
            bottom: y + h,
        },
        radiusX: card_rad,
        radiusY: card_rad,
    };

    let mut nb: Option<&mut [ClockElemBox]> = None;
    let g_card = begin_elem_xf_slot(rt, 5, tr_card, m_rect, s, &mut nb);
    if cfg.music_show_plate {
        let g_top = (cfg.music_glass_alpha as f32 / 255.0).clamp(0.0, 0.90);
        let g_bot = (g_top * 0.62).clamp(0.0, 0.78);
        let (top_c, bot_c, _, ang) = resolve_card_plate_colors(&tr_card, pal, g_top, g_bot, 1.0);
        let (gp0, gp1) = grad_endpoints_for_rect(m_rect, ang);
        let bg_stops = [
            D2D1_GRADIENT_STOP { position: 0.0, color: top_c },
            D2D1_GRADIENT_STOP { position: 1.0, color: bot_c },
        ];
        if let Some(bg) = create_linear_grad(rt, gp0, gp1, &bg_stops) {
            rt.FillRoundedRectangle(&card_rr, &bg);
        }
    }

    if paint_fx && cfg.music_show_plate {
        paint_widget_elemental_fx(
            rt,
            d2d,
            x,
            y,
            x + w,
            y + h,
            (x, y, w, h),
            card_rad,
            s,
            cfg.music_hover_fx,
            88,
            pal,
            !cfg.music_match_wallpaper && !cfg.music_use_custom_color && tr_card.color_rgb.is_none(),
            fx_viz,
        );
    }
    end_elem_xf_slot(rt, g_card);
}

#[allow(clippy::too_many_arguments)]
pub unsafe fn draw_music_widget_with_viz(
    rt: &ID2D1RenderTarget,
    dw: &IDWriteFactory,
    d2d: Option<&ID2D1Factory>,
    custom_fonts: Option<&IDWriteFontCollection>,
    x: f32,
    y: f32,
    s: f32,
    cfg: &AppConfig,
    media: &MediaSnapshot,
    bands: &[f32; 12],
    cover_bmp: Option<&ID2D1Bitmap>,
    hover: WidgetHitZone,
    fx_viz: Option<&VizSnapshot>,
) {
    draw_music_widget_with_boxes_inner(
        rt, dw, d2d, custom_fonts, x, y, s, cfg, media, bands, cover_bmp, hover, fx_viz, None,
        false, true,
    );
}

#[allow(clippy::too_many_arguments)]
pub unsafe fn draw_music_widget_with_viz_cached(
    rt: &ID2D1RenderTarget,
    dw: &IDWriteFactory,
    d2d: Option<&ID2D1Factory>,
    custom_fonts: Option<&IDWriteFontCollection>,
    x: f32,
    y: f32,
    s: f32,
    cfg: &AppConfig,
    media: &MediaSnapshot,
    bands: &[f32; 12],
    cover_bmp: Option<&ID2D1Bitmap>,
    hover: WidgetHitZone,
    fx_viz: Option<&VizSnapshot>,
    fg_cache: &mut WidgetFgCacheSlot,
    cache_gen: u64,
) {
    if cfg.is_music_all_elements_off() {
        return;
    }
    let raw_sum: f32 = bands.iter().copied().sum();
    let has_audio = raw_sum > 0.04;
    let is_pbr_anim = cfg.widgets_live_material
        && fx_viz
            .map(|v| {
                v.hover_mix > 0.004
                    || v.hover_active
                    || v.ripples.iter().any(|r| r.active)
                    || (v.phase > 0.0001
                        && matches!(
                            cfg.music_style,
                            MusicStyle::RainWindowPlayer | MusicStyle::CalmWaterRipple
                        ))
            })
            .unwrap_or(false)
        && matches!(
            cfg.music_style,
            MusicStyle::BrushedTitaniumDeck
                | MusicStyle::RainWindowPlayer
                | MusicStyle::CalmWaterRipple
                | MusicStyle::LeatherFieldAudio
                | MusicStyle::WalnutHiFiConsole
                | MusicStyle::AbyssalTrenchAudio
                | MusicStyle::GlacierIceAudio
                | MusicStyle::CarbonSteelDeck
                | MusicStyle::GoldenHourAudio
        );

    let is_decaying_tail = cfg.music_style == MusicStyle::DynamicIslandCapsule
        && raw_sum > 0.002;
    if has_audio || media.is_playing || is_pbr_anim || is_decaying_tail {
        draw_music_widget_with_boxes_inner(
            rt, dw, d2d, custom_fonts, x, y, s, cfg, media, bands, cover_bmp, hover, fx_viz,
            None, false, true,
        );
        return;
    }

    let pal = music_effective_palette(cfg);
    let (lw, lh) = music_logical_size(cfg);
    let w = lw * s;
    let h = lh * s;
    let key = music_fg_key(s, cfg, &pal, media, cover_bmp, hover, custom_fonts.is_some());
    let is_std = matches!(
        cfg.music_style,
        MusicStyle::Sanctuary
            | MusicStyle::VinylDeck
            | MusicStyle::MinimalPill
            | MusicStyle::SpectrumDeck
            | MusicStyle::NeonWave
            | MusicStyle::CyberHud
    );

    if cfg.music_style == MusicStyle::EditorialSleeve {
        let is_card_hov = matches!(
            hover,
            WidgetHitZone::MusicCard
                | WidgetHitZone::MusicPrev
                | WidgetHitZone::MusicPlayPause
                | WidgetHitZone::MusicNext
        );
        let tr_card = cfg.music_elem_transforms[5];
        let mut nb: Option<&mut [ClockElemBox]> = None;
        let g = begin_elem_xf_slot(rt, 5, tr_card, (x, y, x + w, y + h), s, &mut nb);
        draw_music_plate_rr_with_fx(
            rt,
            d2d,
            D2D_RECT_F {
                left: x + 82.0 * s,
                top: y + 14.0 * s,
                right: x + w - 4.0 * s,
                bottom: y + h - 14.0 * s,
            },
            14.0 * s,
            (x, y, w, h),
            s,
            cfg,
            &pal,
            is_card_hov,
            1.05,
            1.1 * s,
            fx_viz,
            true,
        );
        end_elem_xf_slot(rt, g);
        fg_cache.draw_cached(rt, cache_gen, key, x, y, w, h, 14.0 * s, |fg_rt| {
            draw_music_widget_with_boxes_inner(
                fg_rt, dw, d2d, custom_fonts, x, y, s, cfg, media, bands, cover_bmp, hover,
                fx_viz, None, true, false,
            );
        });
    } else if is_std {
        draw_music_standard_plate(rt, d2d, x, y, s, cfg, &pal, fx_viz, true);
        fg_cache.draw_cached(rt, cache_gen, key, x, y, w, h, 14.0 * s, |fg_rt| {
            draw_music_widget_with_boxes_inner(
                fg_rt, dw, d2d, custom_fonts, x, y, s, cfg, media, bands, cover_bmp, hover,
                fx_viz, None, true, false,
            );
        });
    } else {
        fg_cache.draw_cached(rt, cache_gen, key, x, y, w, h, 14.0 * s, |fg_rt| {
            draw_music_widget_with_boxes_inner(
                fg_rt, dw, d2d, custom_fonts, x, y, s, cfg, media, bands, cover_bmp, hover,
                fx_viz, None, false, false,
            );
        });
        if cfg.music_show_plate && cfg.music_elem_transforms[5].visible && cfg.music_hover_fx != TaskbarVizStyle::Off {
            let tr_card = cfg.music_elem_transforms[5];
            let mut nb: Option<&mut [ClockElemBox]> = None;
            let g = begin_elem_xf_slot(rt, 5, tr_card, (x, y, x + w, y + h), s, &mut nb);
            let fx_rad = if cfg.music_style == MusicStyle::DynamicIslandCapsule {
                h * 0.5
            } else {
                (cfg.music_corner_radius as f32) * s
            };
            paint_widget_elemental_fx(
                rt,
                d2d,
                x,
                y,
                x + w,
                y + h,
                (x, y, w, h),
                fx_rad,
                s,
                cfg.music_hover_fx,
                66,
                &pal,
                !cfg.music_match_wallpaper && !cfg.music_use_custom_color && tr_card.color_rgb.is_none(),
                fx_viz,
            );
            end_elem_xf_slot(rt, g);
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub unsafe fn draw_music_widget_with_boxes(
    rt: &ID2D1RenderTarget,
    dw: &IDWriteFactory,
    d2d: Option<&ID2D1Factory>,
    custom_fonts: Option<&IDWriteFontCollection>,
    x: f32,
    y: f32,
    s: f32,
    cfg: &AppConfig,
    media: &MediaSnapshot,
    bands: &[f32; 12],
    cover_bmp: Option<&ID2D1Bitmap>,
    hover: WidgetHitZone,
    fx_viz: Option<&VizSnapshot>,
    out_boxes: Option<&mut [ClockElemBox; 6]>,
) {
    draw_music_widget_with_boxes_inner(
        rt, dw, d2d, custom_fonts, x, y, s, cfg, media, bands, cover_bmp, hover, fx_viz,
        out_boxes, false, true,
    );
}

#[allow(clippy::too_many_arguments)]
unsafe fn draw_music_widget_with_boxes_inner(
    rt: &ID2D1RenderTarget,
    dw: &IDWriteFactory,
    d2d: Option<&ID2D1Factory>,
    custom_fonts: Option<&IDWriteFontCollection>,
    x: f32,
    y: f32,
    s: f32,
    cfg: &AppConfig,
    media: &MediaSnapshot,
    bands: &[f32; 12],
    cover_bmp: Option<&ID2D1Bitmap>,
    hover: WidgetHitZone,
    fx_viz: Option<&VizSnapshot>,
    mut out_boxes: Option<&mut [ClockElemBox; 6]>,
    skip_plate_and_fx: bool,
    paint_fx: bool,
) {
    if let Some(boxes) = out_boxes.as_deref_mut() {
        *boxes = [ClockElemBox::default(); 6];
    }
    if cfg.is_music_all_elements_off() {
        return;
    }
    let raw_sum: f32 = bands.iter().copied().sum();
    let mut eff_bands = [0.0f32; 12];
    let has_audio = raw_sum > 0.04;
    if has_audio {
        for i in 0..12 {
            eff_bands[i] = (bands[i] * 1.45).clamp(0.06, 1.0);
        }
    }
    let tr_card = cfg.music_elem_transforms[5];
    let pal = music_effective_palette(cfg);
    if cfg.music_style == MusicStyle::DynamicIslandCapsule {
        crate::desktop_widgets::draw_dynamic_island_capsule_player(
            rt,
            dw,
            d2d,
            custom_fonts,
            x,
            y,
            s,
            cfg,
            media,
            bands,
            cover_bmp,
            hover,
            &pal,
            fx_viz,
            out_boxes,
            skip_plate_and_fx,
            paint_fx,
        );
        return;
    }
    let anim_t = if has_audio && media.is_playing {
        MUSIC_EPOCH.get_or_init(std::time::Instant::now).elapsed().as_secs_f32()
    } else {
        0.0
    };
    if draw_music_custom_styles(
        rt,
        dw,
        d2d,
        custom_fonts,
        x,
        y,
        s,
        cfg,
        media,
        &eff_bands,
        has_audio,
        anim_t,
        cover_bmp,
        hover,
        &pal,
        fx_viz,
        out_boxes.as_deref_mut(),
        skip_plate_and_fx,
        paint_fx,
    ) {
        return;
    }

    let w = 410.0 * s;
    let h = 92.0 * s;
    let m_rect = (x, y, x + w, y + h);
    let tr_viz = cfg.music_elem_transforms[4];
    {
        let mut slice_opt = out_boxes.as_deref_mut().map(|b| &mut b[..]);
        record_elem_box_slot(&mut slice_opt, 5, tr_card, m_rect, s);
    }
    let style = cfg.music_style;
    let rad_scale = (cfg.music_corner_radius as f32 / 22.0).clamp(0.0, 2.2);
    let card_rad = match style {
        MusicStyle::MinimalPill => 46.0 * s * rad_scale.min(1.2),
        MusicStyle::CyberHud => 9.0 * s * rad_scale,
        _ => (cfg.music_corner_radius as f32) * s,
    };
    let card_rr = D2D1_ROUNDED_RECT {
        rect: D2D_RECT_F {
            left: x,
            top: y,
            right: x + w,
            bottom: y + h,
        },
        radiusX: card_rad,
        radiusY: card_rad,
    };

    if !skip_plate_and_fx {
        draw_music_standard_plate(rt, d2d, x, y, s, cfg, &pal, fx_viz, paint_fx);
    }

    // Full-card background visualizers for SpectrumDeck and NeonWave (slot 4: Prog/EQ)
    if cfg.music_show_viz && has_audio && tr_viz.visible {
        let viz_pal = tint_cc_pal(&pal, tr_viz.color_rgb);
        let bg_viz_rect = (x + 84.0 * s, y + 10.0 * s, x + 396.0 * s, y + h - 8.0 * s);
        let mut nb_viz: Option<&mut [ClockElemBox]> = None;
        let g_bgviz = begin_elem_xf_slot(rt, 4, tr_viz, bg_viz_rect, s, &mut nb_viz);
        match style {
            MusicStyle::SpectrumDeck => {
                let n_bg = 24usize;
                let area_l = x + 86.0 * s;
                let area_r = x + 396.0 * s;
                let step = (area_r - area_l) / n_bg as f32;
                let bw = (step * 0.62).max(2.5 * s);
                let (vc1, vc2, _) = resolve_elem_grad_colors(Some(&tr_viz), 0.55, viz_pal.light_col(0.55), viz_pal.deep_col(0.18));
                let v_stops = [
                    D2D1_GRADIENT_STOP { position: 0.0, color: vc1 },
                    D2D1_GRADIENT_STOP { position: 0.55, color: viz_pal.accent_col(0.38) },
                    D2D1_GRADIENT_STOP { position: 1.0, color: vc2 },
                ];
                if let Some(vg) = create_linear_grad(rt, (area_l, y + 12.0 * s), (area_l, y + h - 8.0 * s), &v_stops) {
                    for i in 0..n_bg {
                        let b_val = eff_bands[i % 12];
                        let bh = (10.0 + b_val * 62.0) * s;
                        let bx = area_l + i as f32 * step;
                        let br = D2D1_ROUNDED_RECT {
                            rect: D2D_RECT_F {
                                left: bx,
                                top: y + h - 8.0 * s - bh,
                                right: bx + bw,
                                bottom: y + h - 8.0 * s,
                            },
                            radiusX: bw * 0.45,
                            radiusY: bw * 0.45,
                        };
                        rt.FillRoundedRectangle(&br, &vg);
                    }
                }
            }
            MusicStyle::NeonWave => {
                let pts_n = 28usize;
                let area_l = x + 84.0 * s;
                let area_w = 312.0 * s;
                let mid_y = y + 66.0 * s;
                let mut wave1 = Vec::with_capacity(pts_n);
                let mut wave2 = Vec::with_capacity(pts_n);
                for i in 0..pts_n {
                    let p = i as f32 / (pts_n - 1) as f32;
                    let taper = (p * std::f32::consts::PI).sin();
                    let b_val = eff_bands[(i * 11) / (pts_n - 1)];
                    let dy1 = (p * 12.5).sin() * (6.0 + b_val * 16.0) * taper * s;
                    let dy2 = (p * 9.5 + 1.4).cos() * (5.0 + b_val * 12.0) * taper * s;
                    let px = area_l + p * area_w;
                    wave1.push(Vector2 { X: px, Y: mid_y + dy1 });
                    wave2.push(Vector2 { X: px, Y: mid_y + dy2 });
                }
                let (vc1, vc2, _) = resolve_elem_grad_colors(Some(&tr_viz), 0.88, viz_pal.light_col(0.88), viz_pal.deep_col(0.55));
                if let Some(gl_rgb) = tr_viz.glow_rgb {
                    if let Ok(gb) = rt.CreateSolidColorBrush(&elem_col_rgba(Some(gl_rgb), 0.30, vc1), None) {
                        draw_polyline(rt, d2d, &wave1, false, None, Some(&gb), 5.0 * s, None);
                    }
                }
                if let Ok(b2) = rt.CreateSolidColorBrush(&vc2, None) {
                    draw_polyline(rt, d2d, &wave2, false, None, Some(&b2), 2.0 * s, None);
                }
                if let Ok(b1) = rt.CreateSolidColorBrush(&vc1, None) {
                    draw_polyline(rt, d2d, &wave1, false, None, Some(&b1), 2.4 * s, None);
                }
            }
            _ => {}
        }
        end_elem_xf_slot(rt, g_bgviz);
    }

    if cfg.music_show_plate {
        let mut nb: Option<&mut [ClockElemBox]> = None;
        let g_rim = begin_elem_xf_slot(rt, 5, tr_card, m_rect, s, &mut nb);
        let r_base = if matches!(hover, WidgetHitZone::MusicCard | WidgetHitZone::MusicPrev | WidgetHitZone::MusicPlayPause | WidgetHitZone::MusicNext) {
            ((cfg.music_rim_alpha as f32 / 255.0) + 0.22).clamp(0.25, 0.96)
        } else {
            (cfg.music_rim_alpha as f32 / 255.0).clamp(0.0, 1.0)
        };
        let (_, _, base_rim_c, _) = resolve_card_plate_colors(&tr_card, &pal, 0.5, 0.3, r_base);
        let r_first = if tr_card.border_rgb.is_some() || tr_card.color_rgb.is_some() {
            base_rim_c
        } else {
            pal.accent_col(r_base)
        };
        let rim_stops = rim_gradient_stops(&tr_card, &pal, r_first, r_base);
        if let Some(rb) = create_linear_grad(rt, (x, y), (x + w, y + h), &rim_stops) {
            rt.DrawRoundedRectangle(&card_rr, &rb, 1.25 * s, None);
        }

        // CyberHud corner brackets
        if style == MusicStyle::CyberHud {
            if let Ok(cb) = rt.CreateSolidColorBrush(&elem_col_rgba(tr_card.border_rgb.or(tr_card.color_rgb), 0.92, pal.light_col(0.92)), None) {
                let cl = 14.0 * s;
                let pad = 3.0 * s;
                // Top-left & bottom-right HUD brackets
                rt.DrawLine(Vector2 { X: x + pad, Y: y + pad }, Vector2 { X: x + pad + cl, Y: y + pad }, &cb, 2.0 * s, None);
                rt.DrawLine(Vector2 { X: x + pad, Y: y + pad }, Vector2 { X: x + pad, Y: y + pad + cl }, &cb, 2.0 * s, None);
                rt.DrawLine(Vector2 { X: x + w - pad - cl, Y: y + h - pad }, Vector2 { X: x + w - pad, Y: y + h - pad }, &cb, 2.0 * s, None);
                rt.DrawLine(Vector2 { X: x + w - pad, Y: y + h - pad - cl }, Vector2 { X: x + w - pad, Y: y + h - pad }, &cb, 2.0 * s, None);
            }
        }

        let spec_stops = [
            D2D1_GRADIENT_STOP { position: 0.0, color: pal.text_col(0.0) },
            D2D1_GRADIENT_STOP { position: 0.5, color: pal.text_col(0.22) },
            D2D1_GRADIENT_STOP { position: 1.0, color: pal.text_col(0.0) },
        ];
        if let Some(sp) = create_linear_grad(rt, (x + 28.0 * s, y + 1.0 * s), (x + 382.0 * s, y + 1.0 * s), &spec_stops) {
            rt.DrawLine(
                Vector2 { X: x + 28.0 * s, Y: y + 1.0 * s },
                Vector2 { X: x + 382.0 * s, Y: y + 1.0 * s },
                &sp,
                1.0 * s,
                None,
            );
        }
        end_elem_xf_slot(rt, g_rim);
    }

    let tr_cov = cfg.music_elem_transforms[0];
    let tr_tit = cfg.music_elem_transforms[1];
    let tr_art = cfg.music_elem_transforms[2];
    let tr_ctl = cfg.music_elem_transforms[3];

    // Album Cover / Vinyl Turntable (13, 13, 66x66)
    if cfg.music_show_cover {
        let mut slice_opt = out_boxes.as_deref_mut().map(|b| &mut b[..]);
        let g_cov = begin_elem_xf_slot(
            rt,
            0,
            tr_cov,
            (x + 13.0 * s, y + 13.0 * s, x + 79.0 * s, y + 79.0 * s),
            s,
            &mut slice_opt,
        );
        let is_vinyl = matches!(style, MusicStyle::VinylDeck | MusicStyle::MinimalPill);
        let cov_rad = if is_vinyl {
            33.0 * s
        } else if style == MusicStyle::CyberHud {
            6.0 * s
        } else {
            15.0 * s
        };
        let cov_rr = D2D1_ROUNDED_RECT {
            rect: D2D_RECT_F {
                left: x + 13.0 * s,
                top: y + 13.0 * s,
                right: x + 79.0 * s,
                bottom: y + 79.0 * s,
            },
            radiusX: cov_rad,
            radiusY: cov_rad,
        };
        if let Ok(fb) = rt.CreateSolidColorBrush(&pal.glass_top_col(1.0), None) {
            rt.FillRoundedRectangle(&cov_rr, &fb);
        }
        if let Some(bmp) = cover_bmp {
            let mut drew_clipped = false;
            if let Some(factory) = d2d {
                if let Ok(clip_geo) = factory.CreateRoundedRectangleGeometry(&cov_rr) {
                    let layer_res: windows::core::Result<ID2D1Layer> = rt.CreateLayer(None);
                    if let Ok(layer) = layer_res {
                        let mask_geo: ID2D1Geometry = clip_geo.into();
                        let params = D2D1_LAYER_PARAMETERS {
                            contentBounds: cov_rr.rect,
                            geometricMask: std::mem::ManuallyDrop::new(Some(mask_geo.clone())),
                            maskAntialiasMode: D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
                            maskTransform: windows_numerics::Matrix3x2::identity(),
                            opacity: 1.0,
                            opacityBrush: std::mem::ManuallyDrop::new(None),
                            layerOptions: D2D1_LAYER_OPTIONS_NONE,
                        };
                        rt.PushLayer(&params, &layer);
                        rt.DrawBitmap(bmp, Some(&cov_rr.rect), 1.0, D2D1_BITMAP_INTERPOLATION_MODE_LINEAR, None);
                        rt.PopLayer();
                        std::mem::ManuallyDrop::into_inner(params.geometricMask);
                        drew_clipped = true;
                    }
                }
            }
            if !drew_clipped {
                rt.DrawBitmap(bmp, Some(&cov_rr.rect), 1.0, D2D1_BITMAP_INTERPOLATION_MODE_LINEAR, None);
            }
        } else {
            draw_procedural_vinyl_art(rt, d2d, cov_rr.rect, cov_rad, &pal, s);
        }
        if style == MusicStyle::VinylDeck {
            let vc = Vector2 { X: x + 46.0 * s, Y: y + 46.0 * s };
            if let Ok(gr) = rt.CreateSolidColorBrush(&rgba_u8(12, 10, 9, 0.55), None) {
                rt.DrawEllipse(&D2D1_ELLIPSE { point: vc, radiusX: 26.0 * s, radiusY: 26.0 * s }, &gr, 1.2 * s, None);
                rt.DrawEllipse(&D2D1_ELLIPSE { point: vc, radiusX: 19.0 * s, radiusY: 19.0 * s }, &gr, 1.0 * s, None);
            }
            if let Ok(lbl) = rt.CreateSolidColorBrush(&elem_col_rgba(tr_cov.color_rgb, 0.92, pal.accent_col(0.92)), None) {
                rt.FillEllipse(&D2D1_ELLIPSE { point: vc, radiusX: 9.0 * s, radiusY: 9.0 * s }, &lbl);
                let piv = Vector2 { X: x + 80.0 * s, Y: y + 18.0 * s };
                rt.FillEllipse(&D2D1_ELLIPSE { point: piv, radiusX: 3.5 * s, radiusY: 3.5 * s }, &lbl);
                rt.DrawLine(piv, Vector2 { X: x + 68.0 * s, Y: y + 54.0 * s }, &lbl, 1.8 * s, None);
            }
            if let Ok(hole) = rt.CreateSolidColorBrush(&rgba_u8(16, 12, 10, 1.0), None) {
                rt.FillEllipse(&D2D1_ELLIPSE { point: vc, radiusX: 3.2 * s, radiusY: 3.2 * s }, &hole);
            }
        }
        let cov_rim_c = elem_col_rgba(tr_cov.border_rgb.or(tr_cov.color_rgb), 0.76, pal.accent_col(0.55));
        let cov_bw = if tr_cov.border_width > 0 { (tr_cov.border_width as f32 * 0.7).max(1.2) } else { 1.2 };
        if let Ok(cr) = rt.CreateSolidColorBrush(&cov_rim_c, None) {
            rt.DrawRoundedRectangle(&cov_rr, &cr, cov_bw * s, None);
        }
        end_elem_xf_slot(rt, g_cov);
    }

    // Track Typography
    let br_amber = rt.CreateSolidColorBrush(&elem_col_rgba(tr_tit.color_rgb, 1.0, pal.accent_col(1.0)), None).ok();
    let br_cream = rt.CreateSolidColorBrush(&elem_col_rgba(tr_tit.color_rgb, 1.0, pal.text_col(1.0)), None).ok();
    let br_sand = rt.CreateSolidColorBrush(&elem_col_rgba(tr_art.color_rgb, 0.92, pal.muted_col(0.92)), None).ok();
    let br_ctl_cream = rt.CreateSolidColorBrush(&elem_col_rgba(tr_ctl.color_rgb, 1.0, pal.text_col(1.0)), None).ok();
    let br_sh = rt.CreateSolidColorBrush(&rgba_u8(0, 0, 0, 0.66), None).ok();

    let badge_lbl = match style {
        MusicStyle::VinylDeck => "VINYL DECK • HI-FI",
        MusicStyle::MinimalPill => "FLOATING ISLAND",
        MusicStyle::SpectrumDeck => "LIVE SPECTRUM 24-BAND",
        MusicStyle::NeonWave => "NEON WAVE STUDIO",
        MusicStyle::CyberHud => "SYS // AUDIO_STREAM",
        MusicStyle::Sanctuary => "NOW PLAYING",
        _ => "NOW PLAYING",
    };

    let fs = (cfg.music_font_scale_pct.clamp(70, 140) as f32) / 100.0;
    let txt_left = if cfg.music_show_cover { x + 92.0 * s } else { x + 18.0 * s };
    let txt_right = if cfg.music_show_controls { x + 284.0 * s } else { x + 392.0 * s };

    // Elem 1: Badge + Track Title
    let show_tit = cfg.music_show_title && tr_tit.visible;
    let show_bdg = cfg.music_show_badge;
    if show_tit || show_bdg {
        let mut slice_opt = out_boxes.as_deref_mut().map(|b| &mut b[..]);
        let tit_y0 = if show_bdg { y + 14.0 * s } else { y + 29.0 * s };
        let mut tr_tit_eff = tr_tit;
        tr_tit_eff.visible = true;
        let g_tit = begin_elem_xf_slot(
            rt,
            1,
            tr_tit_eff,
            (txt_left, tit_y0, txt_right, y + 51.0 * s),
            s,
            &mut slice_opt,
        );
        let t_fnt = if !cfg.music_elem_fonts[1].is_empty() { &cfg.music_elem_fonts[1] } else { &cfg.music_font_title };
        let t_fs = fs * (tr_tit.font_scale_pct as f32 / 100.0).clamp(0.50, 2.20);
        if show_bdg {
            let tf_badge = make_resolved_tf_named(
                dw,
                custom_fonts,
                t_fnt,
                &[w!("SF Pro Display"), w!("Segoe UI")],
                7.5 * PT_TO_PX * s * t_fs,
                DWRITE_FONT_WEIGHT_BOLD,
                DWRITE_TEXT_ALIGNMENT_LEADING,
                DWRITE_PARAGRAPH_ALIGNMENT_NEAR,
            );
            if let (Some(tf), Some(br)) = (&tf_badge, &br_amber) {
                let rc = D2D_RECT_F { left: txt_left, top: y + 14.0 * s, right: txt_right, bottom: y + 30.0 * s };
                draw_shadowed_text(rt, badge_lbl, tf, rc, br, br_sh.as_ref(), 1.0 * s, false);
            }
        }
        if show_tit {
            let tf_title = make_resolved_tf_named(
                dw,
                custom_fonts,
                t_fnt,
                &[w!("SF Pro Display"), w!("Segoe UI Semibold"), w!("Segoe UI")],
                12.5 * PT_TO_PX * s * t_fs,
                DWRITE_FONT_WEIGHT_SEMI_BOLD,
                DWRITE_TEXT_ALIGNMENT_LEADING,
                DWRITE_PARAGRAPH_ALIGNMENT_NEAR,
            );
            if let (Some(tf), Some(br)) = (&tf_title, &br_cream) {
                let rc = D2D_RECT_F { left: txt_left, top: y + 29.0 * s, right: txt_right, bottom: y + 51.0 * s };
                draw_m_text(rt, tf, &media.title, rc, br, br_sh.as_ref(), 1.0 * s);
            }
        }
        end_elem_xf_slot(rt, g_tit);
    }

    // Elem 2: Artist
    if cfg.music_show_artist {
        let mut slice_opt = out_boxes.as_deref_mut().map(|b| &mut b[..]);
        let g_art = begin_elem_xf_slot(
            rt,
            2,
            tr_art,
            (txt_left, y + 49.0 * s, txt_right, y + 67.0 * s),
            s,
            &mut slice_opt,
        );
        let a_fnt = if !cfg.music_elem_fonts[2].is_empty() { &cfg.music_elem_fonts[2] } else { &cfg.music_font_artist };
        let a_fs = fs * (tr_art.font_scale_pct as f32 / 100.0).clamp(0.50, 2.20);
        let tf_art = make_resolved_tf_named(
            dw,
            custom_fonts,
            a_fnt,
            &[w!("SF Pro Display"), w!("Segoe UI")],
            9.5 * PT_TO_PX * s * a_fs,
            DWRITE_FONT_WEIGHT_REGULAR,
            DWRITE_TEXT_ALIGNMENT_LEADING,
            DWRITE_PARAGRAPH_ALIGNMENT_NEAR,
        );
        if let (Some(tf), Some(br)) = (&tf_art, &br_sand) {
            let rc = D2D_RECT_F { left: txt_left, top: y + 49.0 * s, right: txt_right, bottom: y + 67.0 * s };
            draw_m_text(rt, tf, &media.artist, rc, br, br_sh.as_ref(), 1.0 * s);
        }
        end_elem_xf_slot(rt, g_art);
    }

    // Elem 4: Progress / Equalizer Bars
    if cfg.music_show_viz {
        let viz_box = (txt_left, y + 67.0 * s, txt_right, y + 85.0 * s);
        let mut slice_opt = out_boxes.as_deref_mut().map(|b| &mut b[..]);
        let g_viz = begin_elem_xf_slot(rt, 4, tr_viz, viz_box, s, &mut slice_opt);
        if tr_viz.visible {
            let v_alpha = if has_audio { 0.98 } else { 0.42 };
            let (vc1, vc2, vang) = resolve_elem_grad_colors(
                Some(&tr_viz),
                v_alpha,
                pal.light_col(v_alpha),
                pal.deep_col(if has_audio { 0.68 } else { 0.32 }),
            );
            let (gp0, gp1) = grad_endpoints_for_rect(viz_box, vang);
            let v_stops = [
                D2D1_GRADIENT_STOP { position: 0.0, color: vc1 },
                D2D1_GRADIENT_STOP { position: 1.0, color: vc2 },
            ];
            if let Some(vg) = create_linear_grad(rt, gp0, gp1, &v_stops) {
                let count = if style == MusicStyle::CyberHud { 18usize } else { 12usize };
                let step = if style == MusicStyle::CyberHud { 8.5 } else { 6.5 };
                for i in 0..count {
                    let bx = txt_left + ((i as f32) * step) * s;
                    let b_val = eff_bands[i % 12];
                    let hh = if has_audio { (3.5 + 11.5 * b_val).min(14.0) } else { 2.8 };
                    let rr = D2D1_ROUNDED_RECT {
                        rect: D2D_RECT_F {
                            left: bx,
                            top: y + (76.0 - hh * 0.5) * s,
                            right: bx + 3.4 * s,
                            bottom: y + (76.0 + hh * 0.5) * s,
                        },
                        radiusX: if style == MusicStyle::CyberHud { 0.5 * s } else { 1.6 * s },
                        radiusY: if style == MusicStyle::CyberHud { 0.5 * s } else { 1.6 * s },
                    };
                    rt.FillRoundedRectangle(&rr, &vg);
                }
            }
        }
        end_elem_xf_slot(rt, g_viz);
    }

    // Elem 3: Transport Controls
    if cfg.music_show_controls {
        let mut slice_opt = out_boxes.as_deref_mut().map(|b| &mut b[..]);
        let g_ctl = begin_elem_xf_slot(
            rt,
            3,
            tr_ctl,
            (x + 292.0 * s, y + 26.0 * s, x + 404.0 * s, y + 66.0 * s),
            s,
            &mut slice_opt,
        );
        let prv_hov = hover == WidgetHitZone::MusicPrev;
        let prv_c = Vector2 { X: x + 308.0 * s, Y: y + 46.0 * s };
        let prv_fill = elem_col_rgba(tr_ctl.color_rgb, if prv_hov { 0.36 } else { 0.22 }, pal.text_col(if prv_hov { 0.22 } else { 0.09 }));
        if let Ok(fb) = rt.CreateSolidColorBrush(&prv_fill, None) {
            rt.FillEllipse(&D2D1_ELLIPSE { point: prv_c, radiusX: 15.0 * s, radiusY: 15.0 * s }, &fb);
        }
        if let Ok(sb) = rt.CreateSolidColorBrush(&elem_col_rgba(tr_ctl.border_rgb.or(tr_ctl.color_rgb), if prv_hov { 0.78 } else { 0.38 }, pal.accent_col(if prv_hov { 0.78 } else { 0.22 })), None) {
            rt.DrawEllipse(&D2D1_ELLIPSE { point: prv_c, radiusX: 15.0 * s, radiusY: 15.0 * s }, &sb, 1.0 * s, None);
        }
        if let Some(ref cbr) = br_ctl_cream {
            let bar = D2D1_ROUNDED_RECT {
                rect: D2D_RECT_F { left: x + 303.0 * s, top: y + 41.0 * s, right: x + 305.0 * s, bottom: y + 51.0 * s },
                radiusX: 1.0 * s,
                radiusY: 1.0 * s,
            };
            rt.FillRoundedRectangle(&bar, cbr);
            let pts = [
                Vector2 { X: x + 305.5 * s, Y: y + 46.0 * s },
                Vector2 { X: x + 312.5 * s, Y: y + 41.0 * s },
                Vector2 { X: x + 312.5 * s, Y: y + 51.0 * s },
            ];
            draw_polyline(rt, d2d, &pts, true, Some(cbr.into()), None, 0.0, None);
        }

        let ply_hov = hover == WidgetHitZone::MusicPlayPause;
        let ply_c = Vector2 { X: x + 348.0 * s, Y: y + 46.0 * s };
        let (pc1, pc2, pstop) = resolve_elem_grad_colors(
            Some(&tr_ctl),
            1.0,
            if ply_hov { pal.text_col(1.0) } else { pal.light_col(1.0) },
            pal.deep_col(1.0),
        );
        let ply_stops = [
            D2D1_GRADIENT_STOP { position: 0.0, color: pc1 },
            D2D1_GRADIENT_STOP { position: (pstop as f32 / 100.0).clamp(0.05, 0.95), color: pc1 },
            D2D1_GRADIENT_STOP { position: 1.0, color: pc2 },
        ];
        if let Some(pg) = create_linear_grad(rt, (x + 328.0 * s, y + 26.0 * s), (x + 368.0 * s, y + 66.0 * s), &ply_stops) {
            rt.FillEllipse(&D2D1_ELLIPSE { point: ply_c, radiusX: 19.0 * s, radiusY: 19.0 * s }, &pg);
        }
        if let Ok(rb) = rt.CreateSolidColorBrush(&elem_col_rgba(tr_ctl.border_rgb.or(tr_ctl.color_rgb), 0.86, pal.light_col(0.86)), None) {
            rt.DrawEllipse(&D2D1_ELLIPSE { point: ply_c, radiusX: 19.0 * s, radiusY: 19.0 * s }, &rb, 1.2 * s, None);
        }
        if let Ok(dark) = rt.CreateSolidColorBrush(&rgba_u8(18, 14, 12, 1.0), None) {
            if media.is_playing {
                let b1 = D2D1_ROUNDED_RECT {
                    rect: D2D_RECT_F { left: x + 343.0 * s, top: y + 39.5 * s, right: x + 346.2 * s, bottom: y + 52.5 * s },
                    radiusX: 1.5 * s,
                    radiusY: 1.5 * s,
                };
                let b2 = D2D1_ROUNDED_RECT {
                    rect: D2D_RECT_F { left: x + 349.8 * s, top: y + 39.5 * s, right: x + 353.0 * s, bottom: y + 52.5 * s },
                    radiusX: 1.5 * s,
                    radiusY: 1.5 * s,
                };
                rt.FillRoundedRectangle(&b1, &dark);
                rt.FillRoundedRectangle(&b2, &dark);
            } else {
                let pts = [
                    Vector2 { X: x + 344.5 * s, Y: y + 39.5 * s },
                    Vector2 { X: x + 354.5 * s, Y: y + 46.0 * s },
                    Vector2 { X: x + 344.5 * s, Y: y + 52.5 * s },
                ];
                let dark_b: &windows::Win32::Graphics::Direct2D::ID2D1Brush = &dark;
                draw_polyline(rt, d2d, &pts, true, Some(dark_b), None, 0.0, None);
            }
        }

        let nxt_hov = hover == WidgetHitZone::MusicNext;
        let nxt_c = Vector2 { X: x + 388.0 * s, Y: y + 46.0 * s };
        let nxt_fill = elem_col_rgba(tr_ctl.color_rgb, if nxt_hov { 0.36 } else { 0.22 }, pal.text_col(if nxt_hov { 0.22 } else { 0.09 }));
        if let Ok(fb) = rt.CreateSolidColorBrush(&nxt_fill, None) {
            rt.FillEllipse(&D2D1_ELLIPSE { point: nxt_c, radiusX: 15.0 * s, radiusY: 15.0 * s }, &fb);
        }
        if let Ok(sb) = rt.CreateSolidColorBrush(&elem_col_rgba(tr_ctl.border_rgb.or(tr_ctl.color_rgb), if nxt_hov { 0.78 } else { 0.38 }, pal.accent_col(if nxt_hov { 0.78 } else { 0.22 })), None) {
            rt.DrawEllipse(&D2D1_ELLIPSE { point: nxt_c, radiusX: 15.0 * s, radiusY: 15.0 * s }, &sb, 1.0 * s, None);
        }
        if let Some(ref cbr) = br_ctl_cream {
            let pts = [
                Vector2 { X: x + 383.5 * s, Y: y + 41.0 * s },
                Vector2 { X: x + 390.5 * s, Y: y + 46.0 * s },
                Vector2 { X: x + 383.5 * s, Y: y + 51.0 * s },
            ];
            draw_polyline(rt, d2d, &pts, true, Some(cbr.into()), None, 0.0, None);
            let bar = D2D1_ROUNDED_RECT {
                rect: D2D_RECT_F { left: x + 391.0 * s, top: y + 41.0 * s, right: x + 393.0 * s, bottom: y + 51.0 * s },
                radiusX: 1.0 * s,
                radiusY: 1.0 * s,
            };
            rt.FillRoundedRectangle(&bar, cbr);
        }
        end_elem_xf_slot(rt, g_ctl);
    }
}

// =========================================================================
// PHASE 2B: QUICK CONTROL CENTER (ControlCenter.ini 1:1 Direct2D Renderer)
// =========================================================================
pub const CC_COLS: [f32; 3] = [4.0, 120.0, 236.0];
pub const CC_ROWS: [f32; 4] = [4.0, 98.0, 196.0, 290.0];
pub const CC_MAX_SCROLL: f32 = 192.0;

pub fn cc_tile_pos(idx: usize) -> (f32, f32) {
    match idx {
        0 => (CC_COLS[0], CC_ROWS[0]),
        1 => (CC_COLS[1], CC_ROWS[0]),
        2 => (CC_COLS[2], CC_ROWS[0]),
        3 => (CC_COLS[0], CC_ROWS[1]),
        4 => (CC_COLS[1], CC_ROWS[1]),
        5 => (CC_COLS[2], CC_ROWS[1]),
        6 => (CC_COLS[0], CC_ROWS[2]),
        7 => (CC_COLS[1], CC_ROWS[2]),
        8 => (CC_COLS[2], CC_ROWS[2]),
        9 => (CC_COLS[0], CC_ROWS[3]),
        _ => (CC_COLS[1], CC_ROWS[3]),
    }
}

#[allow(clippy::too_many_arguments)]
pub unsafe fn draw_control_center_widget_live_with_viz(
    rt: &ID2D1RenderTarget,
    dw: &IDWriteFactory,
    d2d: Option<&ID2D1Factory>,
    custom_fonts: Option<&IDWriteFontCollection>,
    x: f32,
    y: f32,
    s: f32,
    cfg: &AppConfig,
    cc: &CcSnapshot,
    scroll_y: f32,
    hover: WidgetHitZone,
    bright_t_override: Option<f32>,
    vol_t_override: Option<(f32, bool)>,
    fx_viz: Option<&VizSnapshot>,
) {
    draw_control_center_widget_with_boxes(
        rt,
        dw,
        d2d,
        custom_fonts,
        x,
        y,
        s,
        cfg,
        cc,
        scroll_y,
        hover,
        bright_t_override,
        vol_t_override,
        fx_viz,
        None,
    );
}

#[allow(clippy::too_many_arguments)]
fn cc_fg_key(
    s: f32,
    cfg: &AppConfig,
    pal: &ThemePalette,
    cc: &CcSnapshot,
    scroll_y: f32,
    hover: WidgetHitZone,
    bright_t_override: Option<f32>,
    vol_t_override: Option<(f32, bool)>,
    has_custom_fonts: bool,
) -> u64 {
    let mut h = FgKeyHasher::new(0xC2);
    h.mix_f32(s);
    h.mix_pal(pal);
    h.mix_u8(cfg.cc_tile_style);
    h.mix_u8(cfg.cc_glass_alpha);
    h.mix_u8(cfg.cc_rim_alpha);
    h.mix_u8(cfg.cc_corner_radius);
    h.mix_bool(cfg.cc_show_plate);
    h.mix_bool(cfg.cc_show_tiles);
    h.mix_bool(cfg.cc_show_bright);
    h.mix_bool(cfg.cc_show_vol);
    h.mix_bool(cfg.cc_show_labels);
    h.mix_bool(cfg.cc_show_scrollbar);
    h.mix_bool(cfg.cc_show_dividers);
    h.mix_str(&cfg.cc_font_label);
    h.mix_str(&cfg.cc_font_val);
    h.mix_u8(cfg.cc_font_scale_pct);
    h.mix_bool(cfg.cc_use_custom_color);
    h.mix_bool(cfg.cc_match_wallpaper);
    h.mix_u64(cfg.cc_accent_rgb as u64);
    h.mix_u64(cfg.cc_text_rgb as u64);
    h.mix_u64(cfg.cc_bg_rgb as u64);
    for tr in &cfg.cc_elem_transforms {
        h.mix_tr(tr);
    }
    for f in &cfg.cc_elem_fonts {
        h.mix_str(f);
    }
    h.mix_horology(&cfg.cc_horology);
    h.mix_bool(cc.wifi_on);
    h.mix_str(&cc.wifi_name);
    h.mix_u8(cc.wifi_signal);
    h.mix_bool(cc.bt_on);
    h.mix_str(&cc.bt_sub);
    h.mix_bool(cc.air_on);
    h.mix_bool(cc.acc_on);
    h.mix_bool(cc.saver_on);
    h.mix_bool(cc.cap_on);
    h.mix_bool(cc.night_on);
    h.mix_bool(cc.hot_on);
    h.mix_bool(cc.near_on);
    h.mix_bool(cc.disp_on);
    h.mix_u8(cc.bright_pct);
    h.mix_u8(cc.vol_pct);
    h.mix_bool(cc.vol_muted);
    h.mix_f32(scroll_y);
    // Only include hover state when targeting Control Center so hovering other widgets hits cache
    let cc_hov_id: u32 = match hover {
        WidgetHitZone::CcBar => 1,
        WidgetHitZone::CcTile(i, sub) => 10 + (i as u32) * 2 + u32::from(sub),
        WidgetHitZone::CcBrightSlider => 40,
        WidgetHitZone::CcVolSlider => 41,
        WidgetHitZone::CcMuteBtn => 42,
        WidgetHitZone::CcScrollUp => 43,
        WidgetHitZone::CcScrollDown => 44,
        WidgetHitZone::CcScrollTrack => 45,
        _ => 0,
    };
    h.mix_u64(cc_hov_id as u64);
    h.mix_u64(bright_t_override.map(|v| (v.to_bits() as u64) | (1u64 << 32)).unwrap_or(0));
    h.mix_u64(
        vol_t_override
            .map(|(v, m)| ((v.to_bits() as u64) << 1) | u64::from(m) | (1u64 << 40))
            .unwrap_or(0),
    );
    h.mix_bool(has_custom_fonts);
    h.finish()
}

/// Cached Control Center renderer: composites the offscreen `ID2D1Bitmap` whenever toggles,
/// sliders, hover state, and interactive hover FX are idle instead of re-tessellating every tick.
#[allow(clippy::too_many_arguments)]
pub unsafe fn draw_control_center_widget_live_with_viz_cached(
    rt: &ID2D1RenderTarget,
    dw: &IDWriteFactory,
    d2d: Option<&ID2D1Factory>,
    custom_fonts: Option<&IDWriteFontCollection>,
    x: f32,
    y: f32,
    s: f32,
    cfg: &AppConfig,
    cc: &CcSnapshot,
    scroll_y: f32,
    hover: WidgetHitZone,
    bright_t_override: Option<f32>,
    vol_t_override: Option<(f32, bool)>,
    fx_viz: Option<&VizSnapshot>,
    fg_cache: &mut WidgetFgCacheSlot,
    cache_gen: u64,
) {
    if cfg.is_cc_all_elements_off() {
        return;
    }
    let has_live_fx = fx_viz
        .map(|v| {
            v.hover_mix > 0.004
                || v.hover_active
                || v.ripples.iter().any(|r| r.active)
                || (cfg.widgets_live_material
                    && v.phase > 0.0001
                    && crate::config::cc_style_horology(cfg.cc_tile_style)
                        .map(|ht| ht.has_idle_motion())
                        .unwrap_or(false))
        })
        .unwrap_or(false);
    let max_elem_pad = crate::desktop_widgets::elem_transforms_pad(&cfg.cc_elem_transforms, s);
    if has_live_fx || max_elem_pad > 64.0 * s {
        draw_control_center_widget_live_with_viz(
            rt,
            dw,
            d2d,
            custom_fonts,
            x,
            y,
            s,
            cfg,
            cc,
            scroll_y,
            hover,
            bright_t_override,
            vol_t_override,
            fx_viz,
        );
        return;
    }
    let pal = resolve_palette_for(cfg, cfg.cc_match_wallpaper);
    let key = cc_fg_key(
        s,
        cfg,
        &pal,
        cc,
        scroll_y,
        hover,
        bright_t_override,
        vol_t_override,
        custom_fonts.is_some(),
    );
    let pad = (18.0 * s + max_elem_pad).min(80.0 * s);
    fg_cache.draw_cached(rt, cache_gen, key, x, y, 388.0 * s, 298.0 * s, pad, |fg_rt| {
        draw_control_center_widget_live_with_viz(
            fg_rt,
            dw,
            d2d,
            custom_fonts,
            x,
            y,
            s,
            cfg,
            cc,
            scroll_y,
            hover,
            bright_t_override,
            vol_t_override,
            fx_viz,
        );
    });
}

#[inline]
fn tint_cc_pal(base: &ThemePalette, color_rgb: Option<u32>) -> ThemePalette {
    let mut p = *base;
    if let Some(rgb) = color_rgb {
        let c = crate::theme::u32_to_rgb(rgb);
        p.accent = c;
        p.accent_light = c;
        p.accent_deep = (c.0 / 2, c.1 / 2, c.2 / 2);
        p.rim = c;
        p.glass_top = (c.0 / 3, c.1 / 3, c.2 / 3);
        p.glass_bot = (c.0 / 4, c.1 / 4, c.2 / 4);
    }
    p
}

#[allow(clippy::too_many_arguments)]
pub unsafe fn draw_control_center_widget_with_boxes(
    rt: &ID2D1RenderTarget,
    dw: &IDWriteFactory,
    d2d: Option<&ID2D1Factory>,
    custom_fonts: Option<&IDWriteFontCollection>,
    x: f32,
    y: f32,
    s: f32,
    cfg: &AppConfig,
    cc: &CcSnapshot,
    scroll_y: f32,
    hover: WidgetHitZone,
    bright_t_override: Option<f32>,
    vol_t_override: Option<(f32, bool)>,
    fx_viz: Option<&VizSnapshot>,
    mut out_boxes: Option<&mut [ClockElemBox; 6]>,
) {
    if let Some(boxes) = out_boxes.as_deref_mut() {
        *boxes = [ClockElemBox::default(); 6];
    }
    if cfg.is_cc_all_elements_off() {
        return;
    }
    let w = 388.0 * s;
    let h = 298.0 * s;
    let cc_rad = (cfg.cc_corner_radius as f32) * s;
    let card_rr = D2D1_ROUNDED_RECT {
        rect: D2D_RECT_F { left: x, top: y, right: x + w, bottom: y + h },
        radiusX: cc_rad,
        radiusY: cc_rad,
    };

    let tr_card = cfg.cc_elem_transforms[5];
    let tr_div = cfg.cc_elem_transforms[4];
    let mut pal = resolve_palette_for(cfg, cfg.cc_match_wallpaper);
    if cfg.cc_use_custom_color {
        let unpack = |rgb: u32| -> (u8, u8, u8) {
            (((rgb >> 16) & 0xFF) as u8, ((rgb >> 8) & 0xFF) as u8, (rgb & 0xFF) as u8)
        };
        let acc = unpack(cfg.cc_accent_rgb);
        let txt = unpack(cfg.cc_text_rgb);
        let bg = unpack(cfg.cc_bg_rgb);
        pal.accent = acc;
        pal.rim = acc;
        pal.accent_light = (
            acc.0.saturating_add(36),
            acc.1.saturating_add(36),
            acc.2.saturating_add(36),
        );
        pal.accent_deep = (
            acc.0.saturating_sub(42),
            acc.1.saturating_sub(42),
            acc.2.saturating_sub(42),
        );
        pal.text_primary = txt;
        pal.text_muted = (
            ((txt.0 as u16 * 3 + bg.0 as u16) / 4) as u8,
            ((txt.1 as u16 * 3 + bg.1 as u16) / 4) as u8,
            ((txt.2 as u16 * 3 + bg.2 as u16) / 4) as u8,
        );
        pal.glass_top = bg;
        pal.glass_bot = (
            bg.0.saturating_sub(12),
            bg.1.saturating_sub(12),
            bg.2.saturating_sub(12),
        );
    }
    if let Some(card_rgb) = tr_card.color_rgb {
        let bg = crate::theme::u32_to_rgb(card_rgb);
        pal.glass_top = bg;
        pal.glass_bot = if let Some(c2) = tr_card.color2_rgb {
            crate::theme::u32_to_rgb(c2)
        } else {
            (bg.0 / 2, bg.1 / 2, bg.2 / 2)
        };
    } else if let Some(c2) = tr_card.color2_rgb {
        pal.glass_bot = crate::theme::u32_to_rgb(c2);
    }
    if let Some(b_rgb) = tr_card.border_rgb {
        pal.rim = crate::theme::u32_to_rgb(b_rgb);
    }

    let horo_cc = crate::config::cc_style_horology(cfg.cc_tile_style);
    let horo_cc_pal = horo_cc.map(|ht| {
        let ca = if cfg.cc_use_custom_color || cfg.cc_match_wallpaper {
            Some(crate::theme::rgb_to_u32(pal.accent))
        } else {
            None
        };
        let ct = if cfg.cc_use_custom_color {
            Some(cfg.cc_text_rgb)
        } else {
            None
        };
        let cb = tr_card.color_rgb.or_else(|| {
            if cfg.cc_use_custom_color || cfg.cc_match_wallpaper {
                Some(crate::theme::rgb_to_u32(pal.glass_top))
            } else {
                None
            }
        });
        let mut hp = cfg.cc_horology.resolve(ht, ca, ct, cb);
        if let Some(bc) = tr_card.border_rgb {
            hp.bezel_rgb = bc;
            hp.bezel_hi_rgb = crate::horology_pbr::lighten_rgb(bc, 0.36);
            hp.bezel_lo_rgb = crate::horology_pbr::darken_rgb(bc, 0.45);
        }
        hp
    });
    if let Some(ref hp) = horo_cc_pal {
        pal = ThemePalette::for_horology(hp.theme);
        pal.accent = crate::horology_pbr::unpack_rgb(hp.lume_rgb);
        pal.accent_light = crate::horology_pbr::unpack_rgb(hp.hands_rgb);
        pal.rim = crate::horology_pbr::unpack_rgb(hp.bezel_rgb);
        pal.text_primary = crate::horology_pbr::unpack_rgb(hp.text_rgb);
        pal.text_muted = crate::horology_pbr::unpack_rgb(hp.subtext_rgb);
        pal.glass_top = crate::horology_pbr::unpack_rgb(hp.dial_hi_rgb);
        pal.glass_bot = crate::horology_pbr::unpack_rgb(hp.dial_lo_rgb);
    }

    let cc_rect = (x, y, x + w, y + h);
    {
        let mut slice_opt = out_boxes.as_deref_mut().map(|b| &mut b[..]);
        record_elem_box_slot(&mut slice_opt, 5, tr_card, cc_rect, s);
    }
    let mut no_cc_boxes: Option<&mut [ClockElemBox]> = None;
    let g_card = begin_elem_xf_slot(rt, 5, tr_card, cc_rect, s, &mut no_cc_boxes);

    if cfg.cc_show_plate {
        if let Some(ref hp) = horo_cc_pal {
            let is_cc_hov = matches!(
                hover,
                WidgetHitZone::CcBar
                    | WidgetHitZone::CcTile(_, _)
                    | WidgetHitZone::CcBrightSlider
                    | WidgetHitZone::CcVolSlider
            );
            let inter = crate::realistic_pbr::pbr_input_from_viz(fx_viz, 0.0, is_cc_hov);
            let op = (0.64 + cfg.cc_glass_alpha as f32 / 240.0).clamp(0.60, 0.99);
            let ra = (cfg.cc_rim_alpha as f32 / 255.0).clamp(0.25, 1.0);
            crate::horology_pbr::draw_horology_case_plate(
                rt,
                d2d,
                card_rr.rect,
                cc_rad,
                s,
                hp,
                op,
                ra,
                true,
                inter.hover_x,
                inter.hover_y,
                inter.hover_mix,
            );
        } else {
            let g_top = (cfg.cc_glass_alpha as f32 / 255.0).clamp(0.0, 0.88);
            let g_bot = (g_top * 0.66).clamp(0.0, 0.78);
            let (top_c, bot_c, _, ang) = resolve_card_plate_colors(&tr_card, &pal, g_top, g_bot, 1.0);
            let (gp0, gp1) = grad_endpoints_for_rect(cc_rect, ang);
            let bg_stops = [
                D2D1_GRADIENT_STOP { position: 0.0, color: top_c },
                D2D1_GRADIENT_STOP { position: 1.0, color: bot_c },
            ];
            if let Some(bg) = create_linear_grad(rt, gp0, gp1, &bg_stops) {
                rt.FillRoundedRectangle(&card_rr, &bg);
            }
        }

        paint_widget_elemental_fx(
            rt,
            d2d,
            x,
            y,
            x + w,
            y + h,
            (x, y, w, h),
            cc_rad,
            s,
            cfg.cc_hover_fx,
            88,
            &pal,
            !cfg.cc_match_wallpaper && !cfg.cc_use_custom_color && tr_card.color_rgb.is_none(),
            fx_viz,
        );
    }

    if cfg.cc_show_plate && horo_cc_pal.is_none() {
        let r_base = if matches!(hover, WidgetHitZone::CcBar | WidgetHitZone::CcTile(_, _) | WidgetHitZone::CcBrightSlider | WidgetHitZone::CcVolSlider) {
            ((cfg.cc_rim_alpha as f32 / 255.0) + 0.18).clamp(0.25, 0.96)
        } else {
            (cfg.cc_rim_alpha as f32 / 255.0).clamp(0.0, 1.0)
        };
        let (_, _, base_rim_c, _) = resolve_card_plate_colors(&tr_card, &pal, 0.5, 0.3, r_base);
        let r_first = if tr_card.border_rgb.is_some() || tr_card.color_rgb.is_some() {
            base_rim_c
        } else {
            pal.accent_col(r_base)
        };
        let rim_stops = rim_gradient_stops(&tr_card, &pal, r_first, r_base);
        if let Some(rb) = create_linear_grad(rt, (x, y), (x + w, y + h), &rim_stops) {
            rt.DrawRoundedRectangle(&card_rr, &rb, 1.2 * s, None);
        }

        let spec_stops = [
            D2D1_GRADIENT_STOP { position: 0.0, color: pal.text_col(0.0) },
            D2D1_GRADIENT_STOP { position: 0.5, color: pal.text_col(0.25) },
            D2D1_GRADIENT_STOP { position: 1.0, color: pal.text_col(0.0) },
        ];
        if let Some(sp) = create_linear_grad(rt, (x + 28.0 * s, y + 1.0 * s), (x + 360.0 * s, y + 1.0 * s), &spec_stops) {
            rt.DrawLine(
                Vector2 { X: x + 28.0 * s, Y: y + 1.0 * s },
                Vector2 { X: x + 360.0 * s, Y: y + 1.0 * s },
                &sp,
                1.0 * s,
                None,
            );
        }
    }
    end_elem_xf_slot(rt, g_card);

    // Elem 4: Independent Horizontal Divider (y + 210)
    if cfg.cc_show_dividers {
        let div_box = (x + 20.0 * s, y + 206.0 * s, x + 368.0 * s, y + 214.0 * s);
        let mut slice_opt = out_boxes.as_deref_mut().map(|b| &mut b[..]);
        let g_div = begin_elem_xf_slot(rt, 4, tr_div, div_box, s, &mut slice_opt);
        if tr_div.visible {
            let def_div = elem_col_rgba(tr_card.border_rgb, 0.34, pal.text_col(0.18));
            let (dc1, dc2, dang) = resolve_elem_grad_colors(Some(&tr_div), 0.72, def_div, def_div);
            let (gp0, gp1) = grad_endpoints_for_rect(div_box, dang);
            let d_stops = [
                D2D1_GRADIENT_STOP { position: 0.0, color: dc1 },
                D2D1_GRADIENT_STOP { position: 1.0, color: dc2 },
            ];
            let lw = if tr_div.border_width > 0 { tr_div.border_width as f32 * 0.6 * s } else { 1.2 * s };
            if let Some(g_col) = tr_div.glow_rgb {
                let (gr, gg, gb) = crate::theme::u32_to_rgb(g_col);
                if let Ok(glow_br) = rt.CreateSolidColorBrush(&rgba_u8(gr, gg, gb, 0.36), None) {
                    rt.DrawLine(
                        Vector2 { X: x + 20.0 * s, Y: y + 210.0 * s },
                        Vector2 { X: x + 368.0 * s, Y: y + 210.0 * s },
                        &glow_br,
                        lw + 3.2 * s,
                        None,
                    );
                }
            }
            if let Some(div_g) = create_linear_grad(rt, gp0, gp1, &d_stops) {
                rt.DrawLine(
                    Vector2 { X: x + 20.0 * s, Y: y + 210.0 * s },
                    Vector2 { X: x + 368.0 * s, Y: y + 210.0 * s },
                    &div_g,
                    lw,
                    None,
                );
            }
        }
        end_elem_xf_slot(rt, g_div);
    }

    let round_cap = round_stroke_style(d2d);
    let fs = (cfg.cc_font_scale_pct.clamp(70, 140) as f32) / 100.0;
    let tr_tiles = cfg.cc_elem_transforms[0];
    let tr_brt = cfg.cc_elem_transforms[1];
    let tr_vol = cfg.cc_elem_transforms[2];
    let tr_scr = cfg.cc_elem_transforms[3];

    let lbl_fnt = if !cfg.cc_elem_fonts[0].is_empty() { &cfg.cc_elem_fonts[0] } else { &cfg.cc_font_label };
    let lbl_fs = fs * (tr_tiles.font_scale_pct as f32 / 100.0).clamp(0.50, 2.20);
    let tf_lbl = make_resolved_tf_named(
        dw,
        custom_fonts,
        lbl_fnt,
        &[w!("SF Pro Display"), w!("Segoe UI")],
        8.5 * PT_TO_PX * s * lbl_fs,
        DWRITE_FONT_WEIGHT_MEDIUM,
        DWRITE_TEXT_ALIGNMENT_CENTER,
        DWRITE_PARAGRAPH_ALIGNMENT_NEAR,
    );
    let brt_fnt = if !cfg.cc_elem_fonts[1].is_empty() { &cfg.cc_elem_fonts[1] } else { &cfg.cc_font_val };
    let brt_fs = fs * (tr_brt.font_scale_pct as f32 / 100.0).clamp(0.50, 2.20);
    let tf_val_brt = make_resolved_tf_named(
        dw,
        custom_fonts,
        brt_fnt,
        &[w!("SF Pro Display"), w!("Segoe UI Semibold")],
        8.5 * PT_TO_PX * s * brt_fs,
        DWRITE_FONT_WEIGHT_SEMI_BOLD,
        DWRITE_TEXT_ALIGNMENT_TRAILING,
        DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
    );
    let vol_fnt = if !cfg.cc_elem_fonts[2].is_empty() { &cfg.cc_elem_fonts[2] } else { &cfg.cc_font_val };
    let vol_fs = fs * (tr_vol.font_scale_pct as f32 / 100.0).clamp(0.50, 2.20);
    let tf_val_vol = make_resolved_tf_named(
        dw,
        custom_fonts,
        vol_fnt,
        &[w!("SF Pro Display"), w!("Segoe UI Semibold")],
        8.5 * PT_TO_PX * s * vol_fs,
        DWRITE_FONT_WEIGHT_SEMI_BOLD,
        DWRITE_TEXT_ALIGNMENT_TRAILING,
        DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
    );
    let br_cream = rt.CreateSolidColorBrush(&pal.text_col(0.96), None).ok();
    let br_sand = rt.CreateSolidColorBrush(&pal.muted_col(0.96), None).ok();
    let br_gold = rt.CreateSolidColorBrush(&pal.light_col(1.0), None).ok();
    let br_sh = rt.CreateSolidColorBrush(&rgba_u8(0, 0, 0, 0.55), None).ok();
    let br_t_ov = make_elem_brush(rt, tr_tiles.color_rgb, 0.98);

    if cfg.cc_show_tiles {
        let mut slice_opt = out_boxes.as_deref_mut().map(|b| &mut b[..]);
        let g_tiles = begin_elem_xf_slot(
            rt,
            0,
            tr_tiles,
            (x + 16.0 * s, y + 18.0 * s, x + 358.0 * s, y + 204.0 * s),
            s,
            &mut slice_opt,
        );
        let tile_pal = tint_cc_pal(&pal, tr_tiles.color_rgb);
        let vp_rc = D2D_RECT_F {
            left: x + 16.0 * s,
            top: y + 18.0 * s,
            right: x + 358.0 * s,
            bottom: y + 204.0 * s,
        };
        rt.PushAxisAlignedClip(&vp_rc, D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);

        let tile_states: [(bool, &str); 11] = [
            (cc.wifi_on, &cc.wifi_name),
            (cc.bt_on, &cc.bt_sub),
            (cc.air_on, "Airplane mode"),
            (cc.acc_on, "Accessibility"),
            (cc.saver_on, "Energy saver"),
            (cc.cap_on, "Live captions"),
            (cc.night_on, "Night light"),
            (cc.hot_on, "Mobile hotspot"),
            (cc.near_on, "Nearby sharing"),
            (cc.disp_on, "Wired display"),
            (cc.disp_on, "Project"),
        ];

        let tile_rad = match cfg.cc_tile_style {
            1 => 25.0 * s, // Pill
            2 => 3.0 * s,  // Chamfer
            3 => 6.0 * s,  // Minimal Flat
            _ => 10.0 * s, // Rounded
        };

        for (idx, (is_on, label)) in tile_states.iter().enumerate() {
            let (rx, ry_raw) = cc_tile_pos(idx);
            let ry = ry_raw - scroll_y;
            if ry + 80.0 < 0.0 || ry > 190.0 {
                continue;
            }
            let tx = vp_rc.left + rx * s;
            let ty = vp_rc.top + ry * s;
            let tw = 102.0 * s;
            let th = 50.0 * s;
            let hovered = matches!(hover, WidgetHitZone::CcTile(t, _) if t == idx);

            let bg_c = match (*is_on, hovered) {
                (true, true) => tile_pal.text_col(0.96),
                (true, false) => tile_pal.accent_col(0.92),
                (false, true) => {
                    if tr_tiles.color_rgb.is_some() {
                        tile_pal.accent_col(0.42)
                    } else {
                        tile_pal.glass_top_col(0.72)
                    }
                }
                (false, false) => {
                    if tr_tiles.color_rgb.is_some() {
                        tile_pal.accent_col(if cfg.cc_tile_style == 3 { 0.18 } else { 0.28 })
                    } else {
                        tile_pal.glass_bot_col(if cfg.cc_tile_style == 3 { 0.22 } else { 0.46 })
                    }
                }
            };
            let stroke_c = match (*is_on, hovered) {
                (true, true) => tile_pal.text_col(1.0),
                (true, false) => tile_pal.light_col(0.84),
                (false, true) => tile_pal.accent_col(0.76),
                (false, false) => {
                    if tr_tiles.color_rgb.is_some() {
                        tile_pal.accent_col(0.35)
                    } else {
                        tile_pal.text_col(if cfg.cc_tile_style == 3 { 0.08 } else { 0.18 })
                    }
                }
            };
            let fg_c = if *is_on { rgba_u8(20, 15, 12, 1.0) } else { tile_pal.text_col(1.0) };
            let acc_c = if *is_on { rgba_u8(20, 15, 12, 1.0) } else { tile_pal.light_col(1.0) };
            let div_c = if *is_on { rgba_u8(20, 15, 12, 0.25) } else { tile_pal.text_col(0.12) };

            let rr = D2D1_ROUNDED_RECT {
                rect: D2D_RECT_F { left: tx, top: ty, right: tx + tw, bottom: ty + th },
                radiusX: tile_rad,
                radiusY: tile_rad,
            };
            let (fg_c, acc_c, div_c) = if let Some(ref hp) = horo_cc_pal {
                crate::horology_pbr::draw_horology_pusher(
                    rt,
                    rr.rect,
                    tile_rad,
                    s,
                    hp,
                    *is_on,
                    hovered,
                    true,
                );
                let fgc = crate::horology_pbr::rgb_u32(hp.text_rgb, 0.98);
                let acc = crate::horology_pbr::rgb_u32(if *is_on { hp.lume_rgb } else { hp.hands_rgb }, 0.96);
                let dvc = crate::horology_pbr::rgb_u32(hp.bezel_rgb, 0.36);
                (fgc, acc, dvc)
            } else {
                if let Ok(bb) = rt.CreateSolidColorBrush(&bg_c, None) {
                    rt.FillRoundedRectangle(&rr, &bb);
                }
                if let Ok(sb) = rt.CreateSolidColorBrush(&stroke_c, None) {
                    rt.DrawRoundedRectangle(&rr, &sb, 1.0 * s, None);
                }
                (fg_c, acc_c, div_c)
            };

            let br_fg = rt.CreateSolidColorBrush(&fg_c, None).ok();
            let br_acc = rt.CreateSolidColorBrush(&acc_c, None).ok();
            let br_div = rt.CreateSolidColorBrush(&div_c, None).ok();

            if let (Some(ref fg), Some(ref acc)) = (&br_fg, &br_acc) {
                let fgb: &windows::Win32::Graphics::Direct2D::ID2D1Brush = fg;
                let accb: &windows::Win32::Graphics::Direct2D::ID2D1Brush = acc;
                let cap = round_cap.as_ref();
                match idx {
                    0 => {
                        if let Some(ref dv) = br_div {
                            rt.DrawLine(Vector2 { X: tx + 52.0 * s, Y: ty + 8.0 * s }, Vector2 { X: tx + 52.0 * s, Y: ty + 42.0 * s }, dv, 1.0 * s, None);
                        }
                        rt.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: tx + 26.0 * s, Y: ty + 31.0 * s }, radiusX: 2.0 * s, radiusY: 2.0 * s }, accb);
                        draw_round_arc(rt, d2d, Vector2 { X: tx + 20.5 * s, Y: ty + 26.5 * s }, Vector2 { X: tx + 31.5 * s, Y: ty + 26.5 * s }, 8.0 * s, fgb, 1.8 * s, cap);
                        draw_round_arc(rt, d2d, Vector2 { X: tx + 17.0 * s, Y: ty + 22.5 * s }, Vector2 { X: tx + 35.0 * s, Y: ty + 22.5 * s }, 13.0 * s, fgb, 1.8 * s, cap);
                        draw_polyline(rt, d2d, &[Vector2 { X: tx + 74.0 * s, Y: ty + 20.0 * s }, Vector2 { X: tx + 79.0 * s, Y: ty + 25.0 * s }, Vector2 { X: tx + 74.0 * s, Y: ty + 30.0 * s }], false, None, Some(fgb), 1.8 * s, cap);
                    }
                    1 => {
                        if let Some(ref dv) = br_div {
                            rt.DrawLine(Vector2 { X: tx + 52.0 * s, Y: ty + 8.0 * s }, Vector2 { X: tx + 52.0 * s, Y: ty + 42.0 * s }, dv, 1.0 * s, None);
                        }
                        let pts = [
                            Vector2 { X: tx + 22.0 * s, Y: ty + 20.0 * s },
                            Vector2 { X: tx + 30.0 * s, Y: ty + 29.0 * s },
                            Vector2 { X: tx + 26.0 * s, Y: ty + 33.0 * s },
                            Vector2 { X: tx + 26.0 * s, Y: ty + 17.0 * s },
                            Vector2 { X: tx + 30.0 * s, Y: ty + 21.0 * s },
                            Vector2 { X: tx + 22.0 * s, Y: ty + 30.0 * s },
                        ];
                        draw_polyline(rt, d2d, &pts, false, None, Some(fgb), 1.7 * s, cap);
                        draw_polyline(rt, d2d, &[Vector2 { X: tx + 74.0 * s, Y: ty + 20.0 * s }, Vector2 { X: tx + 79.0 * s, Y: ty + 25.0 * s }, Vector2 { X: tx + 74.0 * s, Y: ty + 30.0 * s }], false, None, Some(fgb), 1.8 * s, cap);
                    }
                    2 => {
                        rt.DrawLine(Vector2 { X: tx + 43.0 * s, Y: ty + 25.0 * s }, Vector2 { X: tx + 59.0 * s, Y: ty + 25.0 * s }, fgb, 2.2 * s, cap);
                        draw_polyline(rt, d2d, &[Vector2 { X: tx + 49.0 * s, Y: ty + 17.0 * s }, Vector2 { X: tx + 53.0 * s, Y: ty + 25.0 * s }, Vector2 { X: tx + 49.0 * s, Y: ty + 33.0 * s }], false, None, Some(accb), 1.8 * s, cap);
                        draw_polyline(rt, d2d, &[Vector2 { X: tx + 44.0 * s, Y: ty + 21.0 * s }, Vector2 { X: tx + 46.0 * s, Y: ty + 25.0 * s }, Vector2 { X: tx + 44.0 * s, Y: ty + 29.0 * s }], false, None, Some(fgb), 1.6 * s, cap);
                    }
                    3 => {
                        rt.DrawEllipse(&D2D1_ELLIPSE { point: Vector2 { X: tx + 42.0 * s, Y: ty + 18.0 * s }, radiusX: 2.5 * s, radiusY: 2.5 * s }, accb, 1.5 * s, None);
                        rt.DrawLine(Vector2 { X: tx + 35.0 * s, Y: ty + 22.5 * s }, Vector2 { X: tx + 49.0 * s, Y: ty + 22.5 * s }, fgb, 1.6 * s, cap);
                        rt.DrawLine(Vector2 { X: tx + 42.0 * s, Y: ty + 22.5 * s }, Vector2 { X: tx + 42.0 * s, Y: ty + 27.5 * s }, fgb, 1.6 * s, cap);
                        draw_polyline(rt, d2d, &[Vector2 { X: tx + 38.5 * s, Y: ty + 33.0 * s }, Vector2 { X: tx + 42.0 * s, Y: ty + 27.5 * s }, Vector2 { X: tx + 45.5 * s, Y: ty + 33.0 * s }], false, None, Some(fgb), 1.6 * s, cap);
                        draw_polyline(rt, d2d, &[Vector2 { X: tx + 59.0 * s, Y: ty + 21.0 * s }, Vector2 { X: tx + 63.0 * s, Y: ty + 25.0 * s }, Vector2 { X: tx + 59.0 * s, Y: ty + 29.0 * s }], false, None, Some(fgb), 1.6 * s, cap);
                    }
                    4 => {
                        rt.DrawRoundedRectangle(&D2D1_ROUNDED_RECT { rect: D2D_RECT_F { left: tx + 41.0 * s, top: ty + 20.0 * s, right: tx + 59.0 * s, bottom: ty + 30.0 * s }, radiusX: 2.5 * s, radiusY: 2.5 * s }, fgb, 1.6 * s, None);
                        rt.FillRoundedRectangle(&D2D1_ROUNDED_RECT { rect: D2D_RECT_F { left: tx + 59.5 * s, top: ty + 22.5 * s, right: tx + 61.5 * s, bottom: ty + 27.5 * s }, radiusX: 1.0 * s, radiusY: 1.0 * s }, fgb);
                        rt.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: tx + 47.0 * s, Y: ty + 25.0 * s }, radiusX: 3.4 * s, radiusY: 2.2 * s }, accb);
                    }
                    5 => {
                        rt.DrawRoundedRectangle(&D2D1_ROUNDED_RECT { rect: D2D_RECT_F { left: tx + 41.0 * s, top: ty + 18.0 * s, right: tx + 61.0 * s, bottom: ty + 32.0 * s }, radiusX: 3.0 * s, radiusY: 3.0 * s }, fgb, 1.5 * s, None);
                        rt.DrawLine(Vector2 { X: tx + 44.0 * s, Y: ty + 26.0 * s }, Vector2 { X: tx + 51.0 * s, Y: ty + 26.0 * s }, accb, 1.5 * s, cap);
                        rt.DrawLine(Vector2 { X: tx + 53.0 * s, Y: ty + 26.0 * s }, Vector2 { X: tx + 58.0 * s, Y: ty + 26.0 * s }, fgb, 1.5 * s, cap);
                        rt.DrawLine(Vector2 { X: tx + 44.0 * s, Y: ty + 29.0 * s }, Vector2 { X: tx + 55.0 * s, Y: ty + 29.0 * s }, fgb, 1.5 * s, cap);
                    }
                    6 => {
                        rt.DrawEllipse(&D2D1_ELLIPSE { point: Vector2 { X: tx + 51.0 * s, Y: ty + 25.0 * s }, radiusX: 5.5 * s, radiusY: 5.5 * s }, accb, 1.6 * s, None);
                        for &(dx0, dy0, dx1, dy1) in &[
                            (51.0, 15.0, 51.0, 17.5),
                            (51.0, 32.5, 51.0, 35.0),
                            (41.0, 25.0, 43.5, 25.0),
                            (58.5, 25.0, 61.0, 25.0),
                        ] {
                            rt.DrawLine(Vector2 { X: tx + dx0 * s, Y: ty + dy0 * s }, Vector2 { X: tx + dx1 * s, Y: ty + dy1 * s }, fgb, 1.5 * s, cap);
                        }
                    }
                    7 => {
                        rt.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: tx + 51.0 * s, Y: ty + 23.0 * s }, radiusX: 2.0 * s, radiusY: 2.0 * s }, accb);
                        rt.DrawLine(Vector2 { X: tx + 51.0 * s, Y: ty + 25.0 * s }, Vector2 { X: tx + 51.0 * s, Y: ty + 33.0 * s }, fgb, 1.6 * s, cap);
                        rt.DrawEllipse(&D2D1_ELLIPSE { point: Vector2 { X: tx + 51.0 * s, Y: ty + 23.0 * s }, radiusX: 7.5 * s, radiusY: 7.5 * s }, fgb, 1.5 * s, None);
                    }
                    8 => {
                        rt.DrawRoundedRectangle(&D2D1_ROUNDED_RECT { rect: D2D_RECT_F { left: tx + 43.0 * s, top: ty + 19.0 * s, right: tx + 59.0 * s, bottom: ty + 32.0 * s }, radiusX: 3.0 * s, radiusY: 3.0 * s }, fgb, 1.5 * s, None);
                        draw_polyline(rt, d2d, &[Vector2 { X: tx + 48.0 * s, Y: ty + 26.0 * s }, Vector2 { X: tx + 55.0 * s, Y: ty + 19.0 * s }, Vector2 { X: tx + 51.0 * s, Y: ty + 19.0 * s }], false, None, Some(accb), 1.6 * s, cap);
                    }
                    9 => {
                        rt.DrawRoundedRectangle(&D2D1_ROUNDED_RECT { rect: D2D_RECT_F { left: tx + 33.0 * s, top: ty + 19.0 * s, right: tx + 50.0 * s, bottom: ty + 31.0 * s }, radiusX: 2.5 * s, radiusY: 2.5 * s }, fgb, 1.6 * s, None);
                        rt.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: tx + 36.0 * s, Y: ty + 28.0 * s }, radiusX: 2.2 * s, radiusY: 2.2 * s }, accb);
                        draw_polyline(rt, d2d, &[Vector2 { X: tx + 60.0 * s, Y: ty + 21.0 * s }, Vector2 { X: tx + 64.0 * s, Y: ty + 25.0 * s }, Vector2 { X: tx + 60.0 * s, Y: ty + 29.0 * s }], false, None, Some(fgb), 1.7 * s, cap);
                    }
                    _ => {
                        rt.DrawRoundedRectangle(&D2D1_ROUNDED_RECT { rect: D2D_RECT_F { left: tx + 34.0 * s, top: ty + 21.0 * s, right: tx + 47.0 * s, bottom: ty + 30.0 * s }, radiusX: 2.0 * s, radiusY: 2.0 * s }, fgb, 1.5 * s, None);
                        rt.DrawRoundedRectangle(&D2D1_ROUNDED_RECT { rect: D2D_RECT_F { left: tx + 40.0 * s, top: ty + 18.0 * s, right: tx + 53.0 * s, bottom: ty + 27.0 * s }, radiusX: 2.0 * s, radiusY: 2.0 * s }, accb, 1.5 * s, None);
                        draw_polyline(rt, d2d, &[Vector2 { X: tx + 62.0 * s, Y: ty + 21.0 * s }, Vector2 { X: tx + 66.0 * s, Y: ty + 25.0 * s }, Vector2 { X: tx + 62.0 * s, Y: ty + 29.0 * s }], false, None, Some(fgb), 1.6 * s, cap);
                    }
                }
            }

            if cfg.cc_show_labels {
                let lbl_br = br_t_ov.as_ref().or(br_cream.as_ref());
                if let (Some(tf), Some(br)) = (&tf_lbl, lbl_br) {
                    let l_rc = D2D_RECT_F {
                        left: tx - 1.0 * s,
                        top: ty + 54.0 * s,
                        right: tx + 103.0 * s,
                        bottom: ty + 74.0 * s,
                    };
                    draw_shadowed_text(rt, label, tf, l_rc, br, br_sh.as_ref(), 0.8 * s, true);
                }
            }
        }

        rt.PopAxisAlignedClip();
        end_elem_xf_slot(rt, g_tiles);
    }

    // Right Scrollbar Track & Arrows (X=360..378, Y=50..190) -> Elem 3
    if cfg.cc_show_tiles && cfg.cc_show_scrollbar {
        let mut slice_opt = out_boxes.as_deref_mut().map(|b| &mut b[..]);
        let g_scr = begin_elem_xf_slot(
            rt,
            3,
            tr_scr,
            (x + 358.0 * s, y + 48.0 * s, x + 380.0 * s, y + 192.0 * s),
            s,
            &mut slice_opt,
        );
        let scr_pal = tint_cc_pal(&pal, tr_scr.color_rgb);
        let br_scr_arr = make_elem_brush(rt, tr_scr.color_rgb, 0.96);
        let up_hov = hover == WidgetHitZone::CcScrollUp;
        let dn_hov = hover == WidgetHitZone::CcScrollDown;
        let up_rr = D2D1_ROUNDED_RECT {
            rect: D2D_RECT_F { left: x + 360.0 * s, top: y + 50.0 * s, right: x + 378.0 * s, bottom: y + 70.0 * s },
            radiusX: 6.0 * s,
            radiusY: 6.0 * s,
        };
        let dn_rr = D2D1_ROUNDED_RECT {
            rect: D2D_RECT_F { left: x + 360.0 * s, top: y + 170.0 * s, right: x + 378.0 * s, bottom: y + 190.0 * s },
            radiusX: 6.0 * s,
            radiusY: 6.0 * s,
        };
        if let Ok(fb) = rt.CreateSolidColorBrush(&rgba_u8(255, 243, 220, if up_hov { 0.16 } else { 0.05 }), None) {
            rt.FillRoundedRectangle(&up_rr, &fb);
        }
        if let Ok(fb) = rt.CreateSolidColorBrush(&rgba_u8(255, 243, 220, if dn_hov { 0.16 } else { 0.05 }), None) {
            rt.FillRoundedRectangle(&dn_rr, &fb);
        }
        if let Some(sb) = br_scr_arr.as_ref().or(br_sand.as_ref()) {
            let s_brush: &windows::Win32::Graphics::Direct2D::ID2D1Brush = sb;
            draw_polyline(rt, d2d, &[Vector2 { X: x + 369.0 * s, Y: y + 55.0 * s }, Vector2 { X: x + 374.0 * s, Y: y + 63.0 * s }, Vector2 { X: x + 364.0 * s, Y: y + 63.0 * s }], true, Some(s_brush), None, 0.0, None);
            draw_polyline(rt, d2d, &[Vector2 { X: x + 364.0 * s, Y: y + 177.0 * s }, Vector2 { X: x + 374.0 * s, Y: y + 177.0 * s }, Vector2 { X: x + 369.0 * s, Y: y + 185.0 * s }], true, Some(s_brush), None, 0.0, None);
        }
        if let Ok(tr) = rt.CreateSolidColorBrush(&rgba_u8(255, 243, 220, 0.11), None) {
            rt.FillRoundedRectangle(&D2D1_ROUNDED_RECT { rect: D2D_RECT_F { left: x + 366.0 * s, top: y + 74.0 * s, right: x + 372.0 * s, bottom: y + 166.0 * s }, radiusX: 3.0 * s, radiusY: 3.0 * s }, &tr);
        }
        let thumb_y = 74.0 + (scroll_y / CC_MAX_SCROLL).clamp(0.0, 1.0) * 46.0;
        let th_stops = [
            D2D1_GRADIENT_STOP { position: 0.0, color: scr_pal.light_col(1.0) },
            D2D1_GRADIENT_STOP { position: 1.0, color: scr_pal.accent_col(0.92) },
        ];
        if let Some(tg) = create_linear_grad(rt, (x + 366.0 * s, y + thumb_y * s), (x + 366.0 * s, y + (thumb_y + 46.0) * s), &th_stops) {
            rt.FillRoundedRectangle(&D2D1_ROUNDED_RECT { rect: D2D_RECT_F { left: x + 366.0 * s, top: y + thumb_y * s, right: x + 372.0 * s, bottom: y + (thumb_y + 46.0) * s }, radiusX: 3.0 * s, radiusY: 3.0 * s }, &tg);
        }
        end_elem_xf_slot(rt, g_scr);
    }

    // Bottom Sliders: Brightness (Elem 1) + Volume (Elem 2)
    if cfg.cc_show_bright {
        let mut slice_opt = out_boxes.as_deref_mut().map(|b| &mut b[..]);
        let g_brt = begin_elem_xf_slot(
            rt,
            1,
            tr_brt,
            (x + 18.0 * s, y + 218.0 * s, x + 372.0 * s, y + 248.0 * s),
            s,
            &mut slice_opt,
        );
        let brt_pal = tint_cc_pal(&pal, tr_brt.color_rgb);
        let br_brt_ov = make_elem_brush(rt, tr_brt.color_rgb, 0.98);
        let icon_g = br_brt_ov.as_ref().or(br_gold.as_ref());
        let icon_c = br_brt_ov.as_ref().or(br_cream.as_ref());
        if let (Some(gold), Some(cream)) = (icon_g, icon_c) {
            let sc = Vector2 { X: x + 34.0 * s, Y: y + 233.0 * s };
            rt.FillEllipse(&D2D1_ELLIPSE { point: sc, radiusX: 4.5 * s, radiusY: 4.5 * s }, gold);
            for &(dx0, dy0, dx1, dy1) in &[
                (34.0, 224.5, 34.0, 226.5),
                (34.0, 239.5, 34.0, 241.5),
                (25.5, 233.0, 27.5, 233.0),
                (40.5, 233.0, 42.5, 233.0),
            ] {
                rt.DrawLine(Vector2 { X: x + dx0 * s, Y: y + dy0 * s }, Vector2 { X: x + dx1 * s, Y: y + dy1 * s }, cream, 1.5 * s, round_cap.as_ref());
            }
        }

        let bpct = bright_t_override
            .unwrap_or_else(|| cc.bright_pct as f32 / 100.0)
            .clamp(0.0, 1.0);
        let bright_label_pct = (bpct * 100.0).round() as u8;
        if let Some(ref hp) = horo_cc_pal {
            crate::horology_pbr::draw_horology_slider(
                rt,
                D2D_RECT_F {
                    left: x + 54.0 * s,
                    top: y + 225.0 * s,
                    right: x + 328.0 * s,
                    bottom: y + 241.0 * s,
                },
                s,
                hp,
                bpct,
                hover == WidgetHitZone::CcBrightSlider,
            );
        } else {
            draw_cc_slider(rt, x + 54.0 * s, y + 230.0 * s, 274.0 * s, s, bpct, hover == WidgetHitZone::CcBrightSlider, &brt_pal, &tr_brt);
        }
        if let (Some(tf), Some(sb)) = (&tf_val_brt, br_brt_ov.as_ref().or(br_sand.as_ref())) {
            draw_shadowed_text(rt, &format!("{}%", bright_label_pct), tf, D2D_RECT_F { left: x + 328.0 * s, top: y + 220.0 * s, right: x + 370.0 * s, bottom: y + 244.0 * s }, sb, br_sh.as_ref(), 0.8 * s, false);
        }
        end_elem_xf_slot(rt, g_brt);
    }

    // Volume Speaker Icon + Slider (Elem 2)
    if cfg.cc_show_vol {
        let mut slice_opt = out_boxes.as_deref_mut().map(|b| &mut b[..]);
        let g_vol = begin_elem_xf_slot(
            rt,
            2,
            tr_vol,
            (x + 18.0 * s, y + 252.0 * s, x + 372.0 * s, y + 282.0 * s),
            s,
            &mut slice_opt,
        );
        let vol_pal = tint_cc_pal(&pal, tr_vol.color_rgb);
        let br_vol_ov = make_elem_brush(rt, tr_vol.color_rgb, 0.98);
        let (vpct, vol_muted_eff, vol_label_pct) = match vol_t_override {
            Some((t, m)) => {
                let clamped = t.clamp(0.0, 1.0);
                (if m { 0.0 } else { clamped }, m, (clamped * 100.0).round() as u8)
            }
            None => (
                if cc.vol_muted { 0.0 } else { (cc.vol_pct as f32 / 100.0).clamp(0.0, 1.0) },
                cc.vol_muted,
                cc.vol_pct,
            ),
        };
        let mute_hov = hover == WidgetHitZone::CcMuteBtn;
        let spk_bg = D2D1_ROUNDED_RECT {
            rect: D2D_RECT_F { left: x + 22.0 * s, top: y + 255.0 * s, right: x + 46.0 * s, bottom: y + 279.0 * s },
            radiusX: 6.0 * s,
            radiusY: 6.0 * s,
        };
        if let Ok(sb) = rt.CreateSolidColorBrush(&pal.text_col(if mute_hov { 0.18 } else { 0.05 }), None) {
            rt.FillRoundedRectangle(&spk_bg, &sb);
        }
        if let Some(cr) = br_vol_ov.as_ref().or(br_cream.as_ref()) {
            let pts = [
                Vector2 { X: x + 26.0 * s, Y: y + 264.5 * s },
                Vector2 { X: x + 29.5 * s, Y: y + 264.5 * s },
                Vector2 { X: x + 34.0 * s, Y: y + 260.5 * s },
                Vector2 { X: x + 34.0 * s, Y: y + 273.5 * s },
                Vector2 { X: x + 29.5 * s, Y: y + 269.5 * s },
                Vector2 { X: x + 26.0 * s, Y: y + 269.5 * s },
            ];
            draw_polyline(rt, d2d, &pts, true, Some(cr.into()), None, 0.0, None);
        }
        if vol_muted_eff {
            if let Ok(cor) = rt.CreateSolidColorBrush(&rgba_u8(242, 108, 82, 0.96), None) {
                rt.DrawLine(Vector2 { X: x + 36.5 * s, Y: y + 263.5 * s }, Vector2 { X: x + 42.5 * s, Y: y + 270.5 * s }, &cor, 1.8 * s, round_cap.as_ref());
                rt.DrawLine(Vector2 { X: x + 42.5 * s, Y: y + 263.5 * s }, Vector2 { X: x + 36.5 * s, Y: y + 270.5 * s }, &cor, 1.8 * s, round_cap.as_ref());
            }
        } else if let Some(gld) = br_vol_ov.as_ref().or(br_gold.as_ref()) {
            draw_round_arc(rt, d2d, Vector2 { X: x + 37.0 * s, Y: y + 263.5 * s }, Vector2 { X: x + 37.0 * s, Y: ty_clamp_vol(y + 270.5 * s) }, 5.0 * s, gld, 1.6 * s, round_cap.as_ref());
        }

        if let Some(ref hp) = horo_cc_pal {
            crate::horology_pbr::draw_horology_slider(
                rt,
                D2D_RECT_F {
                    left: x + 54.0 * s,
                    top: y + 259.0 * s,
                    right: x + 328.0 * s,
                    bottom: y + 275.0 * s,
                },
                s,
                hp,
                vpct,
                hover == WidgetHitZone::CcVolSlider,
            );
        } else {
            draw_cc_slider(rt, x + 54.0 * s, y + 264.0 * s, 274.0 * s, s, vpct, hover == WidgetHitZone::CcVolSlider, &vol_pal, &tr_vol);
        }
        if let (Some(tf), Some(sb)) = (&tf_val_vol, br_vol_ov.as_ref().or(br_sand.as_ref())) {
            let txt = if vol_muted_eff { "Mute".to_string() } else { format!("{}%", vol_label_pct) };
            draw_shadowed_text(rt, &txt, tf, D2D_RECT_F { left: x + 328.0 * s, top: y + 254.0 * s, right: x + 370.0 * s, bottom: y + 278.0 * s }, sb, br_sh.as_ref(), 0.8 * s, false);
        }
        end_elem_xf_slot(rt, g_vol);
    }

    if cfg.cc_show_plate {
        if let Some(ref hp) = horo_cc_pal {
            let inter = crate::realistic_pbr::pbr_input_from_viz(fx_viz, 0.0, false);
            crate::horology_pbr::draw_sapphire_ar_glare(
                rt,
                d2d,
                card_rr.rect,
                cc_rad,
                s,
                hp,
                inter.hover_x,
                inter.hover_mix,
            );
        }
    }
}

#[inline]
fn ty_clamp_vol(v: f32) -> f32 {
    v
}

#[allow(clippy::too_many_arguments)]
unsafe fn draw_cc_slider(
    rt: &ID2D1RenderTarget,
    x0: f32,
    y_track: f32,
    w: f32,
    s: f32,
    t: f32,
    hov: bool,
    pal: &ThemePalette,
    tr: &ClockElemTransform,
) {
    let tr_rr = D2D1_ROUNDED_RECT {
        rect: D2D_RECT_F { left: x0, top: y_track, right: x0 + w, bottom: y_track + 6.0 * s },
        radiusX: 3.0 * s,
        radiusY: 3.0 * s,
    };
    let track_col = elem_col_rgba(tr.border_rgb, 0.34, pal.accent_col(0.22));
    if let Ok(tbr) = rt.CreateSolidColorBrush(&track_col, None) {
        rt.FillRoundedRectangle(&tr_rr, &tbr);
    }
    let fill_w = (w * t).max(3.0 * s);
    let fl_rr = D2D1_ROUNDED_RECT {
        rect: D2D_RECT_F { left: x0, top: y_track, right: x0 + fill_w, bottom: y_track + 6.0 * s },
        radiusX: 3.0 * s,
        radiusY: 3.0 * s,
    };
    let (fc1, fc2, fang) = resolve_elem_grad_colors(Some(tr), 1.0, pal.deep_col(1.0), pal.light_col(1.0));
    let (gp0, gp1) = grad_endpoints_for_rect((x0, y_track, x0 + w, y_track + 6.0 * s), fang);
    let stops = [
        D2D1_GRADIENT_STOP { position: 0.0, color: fc1 },
        D2D1_GRADIENT_STOP { position: 1.0, color: fc2 },
    ];
    if let Some(fg) = create_linear_grad(rt, gp0, gp1, &stops) {
        rt.FillRoundedRectangle(&fl_rr, &fg);
    }
    let kx = x0 + (w * t);
    let ky = y_track + 3.0 * s;
    let r = if hov { 7.5 * s } else { 6.5 * s };
    if let Some(g_rgb) = tr.glow_rgb {
        let (gr, gg, gb) = crate::theme::u32_to_rgb(g_rgb);
        if let Ok(glow_br) = rt.CreateSolidColorBrush(&rgba_u8(gr, gg, gb, 0.42), None) {
            rt.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: kx, Y: ky }, radiusX: r + 4.0 * s, radiusY: r + 4.0 * s }, &glow_br);
        }
    }
    if let Ok(cb) = rt.CreateSolidColorBrush(&pal.text_col(1.0), None) {
        rt.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: kx, Y: ky }, radiusX: r, radiusY: r }, &cb);
    }
    if let Ok(ab) = rt.CreateSolidColorBrush(&fc2, None) {
        rt.DrawEllipse(&D2D1_ELLIPSE { point: Vector2 { X: kx, Y: ky }, radiusX: r, radiusY: r }, &ab, 2.0 * s, None);
    }
}
