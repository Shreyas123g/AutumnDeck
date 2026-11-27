//! Bespoke Photorealistic Haute Horlogerie Instrument Renderers for All 14 Extended Desktop Widgets.
//!
//! Each widget renders a purpose-built mechanical/physical horology instrument (barometer dial,
//! dual-needle tachymeter, motorsport GPU cluster, mechanical stopwatch chronograph, GMT world
//! watch dials, aviation fuel/drive gauges, Hi-Fi rotary audio console, perpetual calendar,
//! atelier folio clip-pad, sapphire search pill, engraved desk nameplate, clipboard vault,
//! telemetry process rack, and precision capacity vault) while strictly honoring:
//! - Exact hitbox sync with `widgets_extra::hit_test_ext_widget_with_audio`,
//! - All 6 Pen & Canvas 2.0 element slots (`begin_elem_xf_slot` + `out_boxes`),
//! - Granular visibility toggles (`show_plate`, `show_header`, `show_primary`, `show_icon`,
//!   `show_dividers`, `show_footer`, and `is_all_elements_off()` -> 0 pixels drawn),
//! - Independent per-widget `HorologyPartConfig` (Bezel, Dial, Hands, Lume, Glare, Screws, Shadows).

use windows::core::w;
use windows::Win32::Graphics::Direct2D::Common::{D2D1_COLOR_F, D2D_RECT_F};
use windows::Win32::Graphics::Direct2D::{
    ID2D1Factory, ID2D1RenderTarget, ID2D1SolidColorBrush, D2D1_DRAW_TEXT_OPTIONS_CLIP,
    D2D1_ELLIPSE, D2D1_ROUNDED_RECT,
};
use windows::Win32::Graphics::DirectWrite::{
    IDWriteFactory, IDWriteFontCollection, IDWriteTextFormat, DWRITE_FONT_WEIGHT_BOLD,
    DWRITE_FONT_WEIGHT_MEDIUM, DWRITE_FONT_WEIGHT_SEMI_BOLD,
    DWRITE_MEASURING_MODE_NATURAL, DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
    DWRITE_TEXT_ALIGNMENT_CENTER, DWRITE_TEXT_ALIGNMENT_LEADING, DWRITE_TEXT_ALIGNMENT_TRAILING,
};
use windows_numerics::Vector2;

use crate::config::AppConfig;
use crate::desktop_widgets::{
    begin_elem_xf_slot, chrono_local, end_elem_xf_slot, make_resolved_tf_named,
    record_elem_box_slot, ClockElemBox, WidgetHitZone,
};
use crate::horology_pbr::{
    darken_rgb, draw_horology_aperture, draw_horology_case_plate, draw_horology_moonphase,
    draw_horology_pusher, draw_horology_slider, draw_horology_subdial,
    draw_horology_tourbillon_cage, draw_horology_watch_dial, draw_sapphire_ar_glare, lighten_rgb,
    lunar_phase_0_to_1, rgb_u32, HorologyTheme, ResolvedHorologyPalette,
};
use crate::system_media::CcSnapshot;
use crate::taskbar_styler::{paint_widget_elemental_fx, VizSnapshot};
use crate::theme::resolve_palette_for;
use crate::widgets_extra::{
    audio_switcher_layout, days_in_month_pub, first_weekday_of_month_pub, format_speed_pub,
    format_utc_offset, month_name_to_num_pub, month_num_to_name_pub, pomodoro_button_layout,
    quote_for_category, resolve_world_clock_slots, search_engine_name, ExtFontStyle,
    ExtRuntimeState, ExtWidgetConfig, ExtWidgetId,
};

const PT_TO_PX: f32 = 96.0 / 72.0;

unsafe fn draw_txt(
    rt: &ID2D1RenderTarget,
    txt: &str,
    tf: &IDWriteTextFormat,
    rect: D2D_RECT_F,
    br: &ID2D1SolidColorBrush,
) {
    if txt.is_empty() || rect.right <= rect.left || rect.bottom <= rect.top {
        return;
    }
    let wide: Vec<u16> = txt.encode_utf16().collect();
    rt.DrawText(
        &wide,
        tf,
        &rect,
        br,
        D2D1_DRAW_TEXT_OPTIONS_CLIP,
        DWRITE_MEASURING_MODE_NATURAL,
    );
}

const FAMS_SERIF_H: &[windows::core::PCWSTR] = &[w!("Bodoni MT"), w!("Georgia"), w!("Segoe UI")];
const FAMS_SERIF_B: &[windows::core::PCWSTR] = &[w!("Georgia"), w!("Palatino Linotype"), w!("Segoe UI")];
const FAMS_COND: &[windows::core::PCWSTR] = &[w!("Bahnschrift"), w!("Segoe UI")];
const FAMS_MONO: &[windows::core::PCWSTR] = &[w!("Cascadia Mono"), w!("Consolas"), w!("Segoe UI")];
const FAMS_SCRIPT: &[windows::core::PCWSTR] = &[w!("Georgia"), w!("Segoe UI")];
const FAMS_GEO_H: &[windows::core::PCWSTR] = &[w!("Bahnschrift"), w!("SF Pro Display"), w!("Segoe UI")];
const FAMS_SANS_H: &[windows::core::PCWSTR] = &[w!("SF Pro Display"), w!("Bahnschrift"), w!("Segoe UI")];
const FAMS_SANS_B: &[windows::core::PCWSTR] = &[w!("SF Pro Display"), w!("Segoe UI")];

fn font_fams(style: ExtFontStyle) -> (&'static [windows::core::PCWSTR], &'static [windows::core::PCWSTR]) {
    match style {
        ExtFontStyle::EditorialSerif => (FAMS_SERIF_H, FAMS_SERIF_B),
        ExtFontStyle::CondensedBold => (FAMS_COND, FAMS_COND),
        ExtFontStyle::CyberMono => (FAMS_MONO, FAMS_MONO),
        ExtFontStyle::CalligraphyScript => (FAMS_SCRIPT, FAMS_SCRIPT),
        ExtFontStyle::GeometricBold => (FAMS_GEO_H, FAMS_SANS_B),
        ExtFontStyle::DefaultSans => (FAMS_SANS_H, FAMS_SANS_B),
    }
}

fn truncate_str(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(max_chars.saturating_sub(1)).collect();
        out.push('…');
        out
    }
}

/// Resolves the per-widget Horology palette honoring `HorologyPartConfig`, `use_custom_color`,
/// `match_wallpaper`, and Pen & Canvas Card slot overrides.
pub fn resolve_ext_horology_palette(
    cfg: &AppConfig,
    wcfg: &ExtWidgetConfig,
    ht: HorologyTheme,
) -> ResolvedHorologyPalette {
    let pal = resolve_palette_for(cfg, wcfg.match_wallpaper);
    let tr_card = wcfg.elem_transforms[5];
    let ca = if wcfg.match_wallpaper {
        Some(crate::theme::rgb_to_u32(pal.accent))
    } else if wcfg.use_custom_color {
        Some(wcfg.accent_rgb)
    } else {
        None
    };
    let ct = if wcfg.use_custom_color {
        Some(wcfg.text_rgb)
    } else {
        None
    };
    let cb = tr_card.color_rgb.or_else(|| {
        if wcfg.match_wallpaper {
            Some(crate::theme::rgb_to_u32(pal.glass_top))
        } else if wcfg.use_custom_color {
            Some(wcfg.bg_rgb)
        } else {
            None
        }
    });
    let mut hpal = wcfg.horology.resolve(ht, ca, ct, cb);
    if let Some(bc) = tr_card.border_rgb {
        hpal.bezel_rgb = bc;
        hpal.bezel_hi_rgb = lighten_rgb(bc, 0.36);
        hpal.bezel_lo_rgb = darken_rgb(bc, 0.45);
    }
    hpal
}

/// Draws any of the 14 Extended Desktop Widgets using its bespoke Photorealistic Horology
/// instrument layout.
#[allow(clippy::too_many_arguments)]
pub unsafe fn draw_ext_horology_widget(
    rt: &ID2D1RenderTarget,
    dw: &IDWriteFactory,
    d2d: Option<&ID2D1Factory>,
    custom_fonts: Option<&IDWriteFontCollection>,
    x: f32,
    y: f32,
    s: f32,
    id: ExtWidgetId,
    ht: HorologyTheme,
    cfg: &AppConfig,
    state: &ExtRuntimeState,
    cc_snap: &CcSnapshot,
    hover_zone: WidgetHitZone,
    fx_viz: Option<&VizSnapshot>,
    mut out_boxes: Option<&mut [ClockElemBox; 6]>,
) {
    if let Some(boxes) = out_boxes.as_deref_mut() {
        *boxes = [ClockElemBox::default(); 6];
    }
    let wcfg = &cfg.ext_widgets[id.idx()];
    if wcfg.is_all_elements_off() {
        return;
    }

    let (lw, lh) = wcfg.logical_size(id);
    let w = lw * s;
    let h = lh * s;
    let pad = (wcfg.padding as f32).clamp(4.0, 28.0) * s;
    let rad = (wcfg.corner_radius as f32).clamp(2.0, 36.0) * s;
    let tr_card = wcfg.elem_transforms[5];
    let card_tuple = (x, y, x + w, y + h);
    {
        let mut slice_opt = out_boxes.as_deref_mut().map(|b| &mut b[..]);
        record_elem_box_slot(&mut slice_opt, 5, tr_card, card_tuple, s);
    }

    let hpal = resolve_ext_horology_palette(cfg, wcfg, ht);
    let card_hov = matches!(hover_zone, WidgetHitZone::ExtCard(hid) if hid == id)
        || matches!(hover_zone, WidgetHitZone::ExtBtn(hid, _) if hid == id);
    let inter = crate::realistic_pbr::pbr_input_from_viz(fx_viz, 0.0, card_hov);
    let card_rect = D2D_RECT_F {
        left: x,
        top: y,
        right: x + w,
        bottom: y + h,
    };

    // 1. Machined Horology Case Plate (Card slot 5)
    if wcfg.show_plate && tr_card.visible {
        let mut nb: Option<&mut [ClockElemBox]> = None;
        let g = begin_elem_xf_slot(rt, 5, tr_card, card_tuple, s, &mut nb);
        let op = (0.62 + wcfg.glass_alpha as f32 / 240.0).clamp(0.58, 0.99);
        let ra = (wcfg.rim_alpha as f32 / 255.0).clamp(0.25, 1.0);
        draw_horology_case_plate(
            rt,
            d2d,
            card_rect,
            rad,
            s,
            &hpal,
            op,
            ra,
            wcfg.show_shadow,
            inter.hover_x,
            inter.hover_y,
            inter.hover_mix,
        );
        let fx_pal = resolve_palette_for(cfg, wcfg.match_wallpaper);
        paint_widget_elemental_fx(
            rt,
            d2d,
            x,
            y,
            x + w,
            y + h,
            (x, y, w, h),
            rad,
            s,
            wcfg.hover_fx,
            82,
            &fx_pal,
            false,
            fx_viz,
        );
        end_elem_xf_slot(rt, g);
    }

    // Typography + Per-Element Fonts & Colors (slots 0..=4)
    let fsc = (wcfg.font_scale_pct as f32 / 100.0).clamp(0.75, 1.35) * s;
    let (hdr_fams, body_fams) = font_fams(wcfg.font_style);
    let elem_fnt = |slot: usize| -> &str {
        if !wcfg.elem_fonts[slot].is_empty() {
            wcfg.elem_fonts[slot].as_str()
        } else {
            wcfg.custom_font.as_str()
        }
    };
    let elem_fs = |slot: usize| -> f32 {
        fsc * (wcfg.elem_transforms[slot].font_scale_pct as f32 / 100.0).clamp(0.50, 2.20)
    };
    let elem_col = |slot: usize, fallback_rgb: u32, a: f32| -> D2D1_COLOR_F {
        rgb_u32(wcfg.elem_transforms[slot].color_rgb.unwrap_or(fallback_rgb), a)
    };

    let Some(tf_hdr) = make_resolved_tf_named(
        dw,
        custom_fonts,
        elem_fnt(0),
        hdr_fams,
        7.6 * PT_TO_PX * elem_fs(0),
        DWRITE_FONT_WEIGHT_BOLD,
        DWRITE_TEXT_ALIGNMENT_LEADING,
        DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
    ) else {
        return;
    };
    let Some(tf_big) = make_resolved_tf_named(
        dw,
        custom_fonts,
        elem_fnt(1),
        hdr_fams,
        18.0 * PT_TO_PX * elem_fs(1),
        DWRITE_FONT_WEIGHT_BOLD,
        DWRITE_TEXT_ALIGNMENT_LEADING,
        DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
    ) else {
        return;
    };
    let Some(tf_big_c) = make_resolved_tf_named(
        dw,
        custom_fonts,
        elem_fnt(1),
        hdr_fams,
        16.5 * PT_TO_PX * elem_fs(1),
        DWRITE_FONT_WEIGHT_BOLD,
        DWRITE_TEXT_ALIGNMENT_CENTER,
        DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
    ) else {
        return;
    };
    let Some(tf_body) = make_resolved_tf_named(
        dw,
        custom_fonts,
        elem_fnt(2),
        body_fams,
        8.6 * PT_TO_PX * elem_fs(2),
        DWRITE_FONT_WEIGHT_MEDIUM,
        DWRITE_TEXT_ALIGNMENT_LEADING,
        DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
    ) else {
        return;
    };
    let Some(tf_body_r) = make_resolved_tf_named(
        dw,
        custom_fonts,
        elem_fnt(2),
        body_fams,
        8.2 * PT_TO_PX * elem_fs(2),
        DWRITE_FONT_WEIGHT_SEMI_BOLD,
        DWRITE_TEXT_ALIGNMENT_TRAILING,
        DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
    ) else {
        return;
    };
    let Some(tf_btn) = make_resolved_tf_named(
        dw,
        custom_fonts,
        elem_fnt(3),
        body_fams,
        7.4 * PT_TO_PX * elem_fs(3),
        DWRITE_FONT_WEIGHT_BOLD,
        DWRITE_TEXT_ALIGNMENT_CENTER,
        DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
    ) else {
        return;
    };
    let Some(tf_micro) = make_resolved_tf_named(
        dw,
        custom_fonts,
        elem_fnt(2),
        body_fams,
        6.6 * PT_TO_PX * elem_fs(2),
        DWRITE_FONT_WEIGHT_SEMI_BOLD,
        DWRITE_TEXT_ALIGNMENT_CENTER,
        DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
    ) else {
        return;
    };

    let Ok(br_hdr) = rt.CreateSolidColorBrush(&elem_col(0, hpal.lume_rgb, 0.96), None) else {
        return;
    };
    let Ok(br_txt) = rt.CreateSolidColorBrush(&elem_col(1, hpal.text_rgb, 0.98), None) else {
        return;
    };
    let Ok(br_sub) = rt.CreateSolidColorBrush(&elem_col(2, hpal.subtext_rgb, 0.94), None) else {
        return;
    };
    let Ok(br_btn_txt) = rt.CreateSolidColorBrush(&elem_col(3, hpal.text_rgb, 0.98), None) else {
        return;
    };
    let Ok(br_lume) = rt.CreateSolidColorBrush(&rgb_u32(hpal.lume_rgb, 0.96), None) else {
        return;
    };
    let Ok(br_div) = rt.CreateSolidColorBrush(&elem_col(4, hpal.bezel_rgb, 0.42), None) else {
        return;
    };

    let is_btn_hov = |b: u8| matches!(hover_zone, WidgetHitZone::ExtBtn(hid, hb) if hid == id && hb == b);

    let draw_crown_pill = |rx0: f32, ry0: f32, rx1: f32, ry1: f32, lbl: &str, act: bool, hov: bool| {
        let rc = D2D_RECT_F {
            left: rx0,
            top: ry0,
            right: rx1,
            bottom: ry1,
        };
        draw_horology_pusher(rt, rc, 5.0 * s, s, &hpal, act, hov, false);
        draw_txt(rt, lbl, &tf_btn, rc, if act { &br_lume } else { &br_btn_txt });
    };

    let x0 = x + pad;
    let x1 = (x + w - pad).max(x0 + 60.0 * s);
    let mut y_cur = y + pad * 0.75;

    let draw_horo_divider = |y_div: f32, boxes: &mut Option<&mut [ClockElemBox; 6]>| {
        if !wcfg.is_slot_visible(4) {
            return;
        }
        let tr = wcfg.elem_transforms[4];
        let mut slice_opt = boxes.as_deref_mut().map(|b| &mut b[..]);
        let g = begin_elem_xf_slot(rt, 4, tr, (x0, y_div, x1, y_div + 2.0 * s), s, &mut slice_opt);
        rt.DrawLine(
            Vector2 { X: x0, Y: y_div + 0.5 * s },
            Vector2 { X: x1, Y: y_div + 0.5 * s },
            &br_div,
            (1.0 * s).max(0.8),
            None,
        );
        if let Ok(jb) = rt.CreateSolidColorBrush(&rgb_u32(hpal.jewel_rgb, 0.90), None) {
            rt.FillEllipse(
                &D2D1_ELLIPSE {
                    point: Vector2 { X: (x0 + x1) * 0.5, Y: y_div + 0.5 * s },
                    radiusX: 1.6 * s,
                    radiusY: 1.6 * s,
                },
                &jb,
            );
        }
        end_elem_xf_slot(rt, g);
    };

    match id {
        // =====================================================================
        // 1. LIVE WEATHER — ANEROID BAROMETER & MOONPHASE WATCH DASHBOARD
        // =====================================================================
        ExtWidgetId::Weather => {
            let snap = &state.weather[(wcfg.mode_idx as usize).min(3)];
            let use_f = wcfg.feats[0];
            let to_u = |c: f32| if use_f { c * 1.8 + 32.0 } else { c };
            let unit_ch = if use_f { "°F" } else { "°C" };

            if wcfg.is_slot_visible(0) {
                let mut slice_opt = out_boxes.as_deref_mut().map(|b| &mut b[..]);
                let g = begin_elem_xf_slot(rt, 0, wcfg.elem_transforms[0], (x0, y_cur, x1, y_cur + 17.0 * s), s, &mut slice_opt);
                let hdr = format!("{}  •  {}", ht.badge().to_ascii_uppercase(), snap.city.to_uppercase());
                draw_txt(rt, &hdr, &tf_hdr, D2D_RECT_F { left: x0, top: y_cur, right: x1 - 108.0 * s, bottom: y_cur + 16.0 * s }, &br_hdr);
                draw_crown_pill(x1 - 104.0 * s, y_cur, x1 - 44.0 * s, y_cur + 17.0 * s, "City ↻", false, is_btn_hov(1));
                draw_crown_pill(x1 - 40.0 * s, y_cur, x1, y_cur + 17.0 * s, unit_ch, use_f, is_btn_hov(0));
                end_elem_xf_slot(rt, g);
                y_cur += 21.0 * s;
            }
            draw_horo_divider(y_cur - 2.0 * s, &mut out_boxes);

            let dial_r = if wcfg.compact_mode { 38.0 * s } else { 46.0 * s };
            let dial_cx = x0 + dial_r + 6.0 * s;
            let dial_cy = y_cur + dial_r + 6.0 * s;
            let now = chrono_local();
            let lunar = lunar_phase_0_to_1(now.year, month_name_to_num_pub(now.month_name), now.day);

            if wcfg.is_slot_visible(3) {
                let mut slice_opt = out_boxes.as_deref_mut().map(|b| &mut b[..]);
                let g = begin_elem_xf_slot(
                    rt,
                    3,
                    wcfg.elem_transforms[3],
                    (dial_cx - dial_r, dial_cy - dial_r, dial_cx + dial_r, dial_cy + dial_r),
                    s,
                    &mut slice_opt,
                );
                let temp_norm = ((snap.temp_c + 10.0) / 55.0).clamp(0.0, 1.0);
                draw_horology_subdial(rt, d2d, dial_cx, dial_cy, dial_r, s, &hpal, temp_norm, Some(0.82), None);
                draw_horology_moonphase(rt, dial_cx, dial_cy + dial_r * 0.38, dial_r * 0.28, s, &hpal, lunar);
                if ht == HorologyTheme::TourbillonGold {
                    draw_horology_tourbillon_cage(rt, dial_cx, dial_cy - dial_r * 0.36, dial_r * 0.22, s, &hpal, inter.time_s);
                }
                end_elem_xf_slot(rt, g);
            }

            let rx0 = dial_cx + dial_r + 14.0 * s;
            if wcfg.is_slot_visible(1) {
                let ap = D2D_RECT_F {
                    left: rx0,
                    top: y_cur + 4.0 * s,
                    right: x1,
                    bottom: y_cur + 44.0 * s,
                };
                let mut slice_opt = out_boxes.as_deref_mut().map(|b| &mut b[..]);
                let g = begin_elem_xf_slot(rt, 1, wcfg.elem_transforms[1], (ap.left, ap.top, ap.right, ap.bottom), s, &mut slice_opt);
                draw_horology_aperture(rt, ap, 7.0 * s, s, &hpal, true);
                let t_str = format!("{:.0}{}", to_u(snap.temp_c), unit_ch);
                draw_txt(rt, &t_str, &tf_big, D2D_RECT_F { left: ap.left + 8.0 * s, top: ap.top, right: ap.left + 92.0 * s, bottom: ap.bottom }, &br_txt);
                let cond = truncate_str(snap.condition, 15);
                draw_txt(rt, &cond, &tf_body_r, D2D_RECT_F { left: ap.left + 84.0 * s, top: ap.top + 4.0 * s, right: ap.right - 8.0 * s, bottom: ap.top + 22.0 * s }, &br_sub);
                let hl = format!("H {:.0}°  L {:.0}°", to_u(snap.high_c), to_u(snap.low_c));
                draw_txt(rt, &hl, &tf_body_r, D2D_RECT_F { left: ap.left + 84.0 * s, top: ap.top + 21.0 * s, right: ap.right - 8.0 * s, bottom: ap.bottom - 3.0 * s }, &br_hdr);
                end_elem_xf_slot(rt, g);
            }

            if wcfg.is_slot_visible(2) {
                let fy0 = y_cur + 50.0 * s;
                let mut slice_opt = out_boxes.as_deref_mut().map(|b| &mut b[..]);
                let g = begin_elem_xf_slot(rt, 2, wcfg.elem_transforms[2], (rx0, fy0, x1, y + h - pad), s, &mut slice_opt);
                let meta = format!("HUM {}%   •   WIND {:.0} km/h   •   UV {}", snap.humidity, snap.wind_kmh, snap.uv_index);
                draw_txt(rt, &meta, &tf_body, D2D_RECT_F { left: rx0, top: fy0, right: x1, bottom: fy0 + 18.0 * s }, &br_sub);

                // Engraved 7-Day Horology Forecast Strip along lower edge
                let strip_y0 = (y + h - pad - 44.0 * s).max(fy0 + 24.0 * s);
                let strip_y1 = y + h - pad;
                if strip_y1 - strip_y0 >= 28.0 * s {
                    let count = if wcfg.compact_mode { 5 } else { 7 };
                    let gap = 4.0 * s;
                    let col_w = ((x1 - x0 - gap * (count as f32 - 1.0)) / count as f32).max(20.0 * s);
                    for i in 0..count {
                        let (day_s, hi_c, lo_c, _) = snap.forecast_days[i];
                        let cx0 = x0 + i as f32 * (col_w + gap);
                        let rc = D2D_RECT_F { left: cx0, top: strip_y0, right: cx0 + col_w, bottom: strip_y1 };
                        draw_horology_aperture(rt, rc, 5.0 * s, s, &hpal, false);
                        draw_txt(rt, day_s, &tf_micro, D2D_RECT_F { left: cx0, top: strip_y0 + 2.0 * s, right: cx0 + col_w, bottom: strip_y0 + 15.0 * s }, &br_hdr);
                        let t_hl = format!("{:.0}°/{:.0}°", to_u(hi_c), to_u(lo_c));
                        draw_txt(rt, &t_hl, &tf_micro, D2D_RECT_F { left: cx0, top: strip_y0 + 16.0 * s, right: cx0 + col_w, bottom: strip_y1 - 2.0 * s }, &br_txt);
                    }
                }
                end_elem_xf_slot(rt, g);
            }
        }

        // =====================================================================
        // 2. DESKTOP SEARCH BAR — MACHINED HOROLOGY PILL CONSOLE
        // =====================================================================
        ExtWidgetId::SearchBar => {
            let cy_top = y + 7.0 * s;
            let cy_bot = y + h - 7.0 * s;
            let mut q_left = x + 10.0 * s;
            let q_right = if wcfg.feats[3] { x + w - 46.0 * s } else { x + w - 10.0 * s };

            if wcfg.feats[0] && wcfg.is_slot_visible(0) {
                let eng_rc = D2D_RECT_F { left: x + 8.0 * s, top: cy_top, right: x + 108.0 * s, bottom: cy_bot };
                let mut slice_opt = out_boxes.as_deref_mut().map(|b| &mut b[..]);
                let g = begin_elem_xf_slot(rt, 0, wcfg.elem_transforms[0], (eng_rc.left, eng_rc.top, eng_rc.right, eng_rc.bottom), s, &mut slice_opt);
                draw_crown_pill(eng_rc.left, eng_rc.top, eng_rc.right, eng_rc.bottom, search_engine_name(wcfg.mode_idx), state.search_focused, is_btn_hov(1));
                end_elem_xf_slot(rt, g);
                q_left = x + 114.0 * s;
            }

            if wcfg.is_slot_visible(1) {
                let q_rc = D2D_RECT_F { left: q_left, top: cy_top, right: q_right.max(q_left + 40.0 * s), bottom: cy_bot };
                let mut slice_opt = out_boxes.as_deref_mut().map(|b| &mut b[..]);
                let g = begin_elem_xf_slot(rt, 1, wcfg.elem_transforms[1], (q_rc.left, q_rc.top, q_rc.right, q_rc.bottom), s, &mut slice_opt);
                draw_horology_aperture(rt, q_rc, (cy_bot - cy_top) * 0.45, s, &hpal, true);
                let disp = if !state.search_query.is_empty() {
                    if state.search_focused { format!("{}|", state.search_query) } else { state.search_query.clone() }
                } else if state.search_focused {
                    "Type to search…|".to_string()
                } else {
                    format!("Search {}…", search_engine_name(wcfg.mode_idx))
                };
                draw_txt(
                    rt,
                    &disp,
                    &tf_body,
                    D2D_RECT_F { left: q_rc.left + 10.0 * s, top: q_rc.top, right: q_rc.right - 8.0 * s, bottom: q_rc.bottom },
                    if state.search_query.is_empty() { &br_sub } else { &br_txt },
                );
                end_elem_xf_slot(rt, g);
            }

            if wcfg.feats[3] && wcfg.is_slot_visible(3) {
                let go_rc = D2D_RECT_F { left: x + w - 42.0 * s, top: cy_top, right: x + w - 8.0 * s, bottom: cy_bot };
                let mut slice_opt = out_boxes.as_deref_mut().map(|b| &mut b[..]);
                let g = begin_elem_xf_slot(rt, 3, wcfg.elem_transforms[3], (go_rc.left, go_rc.top, go_rc.right, go_rc.bottom), s, &mut slice_opt);
                draw_crown_pill(go_rc.left, go_rc.top, go_rc.right, go_rc.bottom, "Go →", false, is_btn_hov(2));
                end_elem_xf_slot(rt, g);
            }
        }

        // =====================================================================
        // 3. DAILY GREETING & QUOTE — ENGRAVED HOROLOGY DESK NAMEPLATE
        // =====================================================================
        ExtWidgetId::Greeting => {
            let now = chrono_local();
            let sal = if now.hour < 12 {
                "Good morning"
            } else if now.hour < 17 {
                "Good afternoon"
            } else {
                "Good evening"
            };
            let cat_lbl = match wcfg.mode_idx {
                1 => "Tech",
                2 => "Zen",
                3 => "Lit",
                _ => "Stoic",
            };
            if wcfg.is_slot_visible(0) {
                let mut slice_opt = out_boxes.as_deref_mut().map(|b| &mut b[..]);
                let g = begin_elem_xf_slot(rt, 0, wcfg.elem_transforms[0], (x0, y_cur, x1, y_cur + 17.0 * s), s, &mut slice_opt);
                let greet = format!("◆  {}, {}", sal, state.user_name);
                draw_txt(rt, &greet, &tf_hdr, D2D_RECT_F { left: x0, top: y_cur, right: x1 - 112.0 * s, bottom: y_cur + 17.0 * s }, &br_hdr);
                if wcfg.feats[4] {
                    draw_crown_pill(x1 - 108.0 * s, y_cur, x1 - 32.0 * s, y_cur + 17.0 * s, cat_lbl, false, is_btn_hov(1));
                    draw_crown_pill(x1 - 28.0 * s, y_cur, x1, y_cur + 17.0 * s, "↻", false, is_btn_hov(0));
                }
                end_elem_xf_slot(rt, g);
                y_cur += 19.0 * s;
            }
            draw_horo_divider(y_cur - 1.5 * s, &mut out_boxes);
            let (q_txt, q_auth) = quote_for_category(wcfg.mode_idx, state.quote_idx);
            if wcfg.is_slot_visible(1) {
                let mut slice_opt = out_boxes.as_deref_mut().map(|b| &mut b[..]);
                let g = begin_elem_xf_slot(rt, 1, wcfg.elem_transforms[1], (x0, y_cur + 1.0 * s, x1, y + h - pad * 0.5), s, &mut slice_opt);
                let line = format!("“{}”  — {}", truncate_str(q_txt, 64), q_auth);
                draw_txt(rt, &line, &tf_body, D2D_RECT_F { left: x0, top: y_cur + 1.0 * s, right: x1, bottom: y + h - pad * 0.4 }, &br_txt);
                end_elem_xf_slot(rt, g);
            }
        }

        // =====================================================================
        // 4. NETWORK SPEEDOMETER — DUAL-NEEDLE MOTORSPORT TACHYMETER
        // =====================================================================
        ExtWidgetId::Network => {
            let unit_lbl = match wcfg.mode_idx {
                1 => "Mbps",
                2 => "KB/s",
                _ => "Auto",
            };
            if wcfg.is_slot_visible(0) {
                let mut slice_opt = out_boxes.as_deref_mut().map(|b| &mut b[..]);
                let g = begin_elem_xf_slot(rt, 0, wcfg.elem_transforms[0], (x0, y_cur, x1, y_cur + 17.0 * s), s, &mut slice_opt);
                draw_txt(rt, &format!("{}  •  NET TACHY", ht.badge().to_ascii_uppercase()), &tf_hdr, D2D_RECT_F { left: x0, top: y_cur, right: x1 - 78.0 * s, bottom: y_cur + 16.0 * s }, &br_hdr);
                draw_crown_pill(x1 - 74.0 * s, y_cur, x1, y_cur + 17.0 * s, unit_lbl, false, is_btn_hov(0));
                end_elem_xf_slot(rt, g);
                y_cur += 20.0 * s;
            }
            draw_horo_divider(y_cur - 2.0 * s, &mut out_boxes);

            let dial_r = 25.5 * s;
            let d_cy = y_cur + dial_r + 4.0 * s;
            let down_norm = ((state.down_bps / 12_500_000.0).sqrt()).clamp(0.0, 1.0);
            let up_norm = ((state.up_bps / 5_000_000.0).sqrt()).clamp(0.0, 1.0);

            if wcfg.is_slot_visible(1) {
                let mut slice_opt = out_boxes.as_deref_mut().map(|b| &mut b[..]);
                let g = begin_elem_xf_slot(rt, 1, wcfg.elem_transforms[1], (x0, y_cur, x0 + 132.0 * s, y + h - pad), s, &mut slice_opt);
                let c1 = x0 + 30.0 * s;
                let c2 = x0 + 94.0 * s;
                draw_horology_subdial(rt, d2d, c1, d_cy, dial_r, s, &hpal, down_norm, Some(0.85), None);
                draw_horology_subdial(rt, d2d, c2, d_cy, dial_r, s, &hpal, up_norm, Some(0.85), Some(hpal.hands_rgb));
                draw_txt(rt, &format!("↓ {}", format_speed_pub(state.down_bps, wcfg.mode_idx)), &tf_micro, D2D_RECT_F { left: c1 - 31.0 * s, top: d_cy + dial_r - 2.0 * s, right: c1 + 31.0 * s, bottom: d_cy + dial_r + 12.0 * s }, &br_txt);
                draw_txt(rt, &format!("↑ {}", format_speed_pub(state.up_bps, wcfg.mode_idx)), &tf_micro, D2D_RECT_F { left: c2 - 31.0 * s, top: d_cy + dial_r - 2.0 * s, right: c2 + 31.0 * s, bottom: d_cy + dial_r + 12.0 * s }, &br_sub);
                end_elem_xf_slot(rt, g);
            }

            if wcfg.is_slot_visible(3) {
                let sp_rc = D2D_RECT_F {
                    left: x0 + 134.0 * s,
                    top: y_cur + 4.0 * s,
                    right: x1,
                    bottom: (y + h - pad - 20.0 * s).max(y_cur + 28.0 * s),
                };
                let mut slice_opt = out_boxes.as_deref_mut().map(|b| &mut b[..]);
                let g = begin_elem_xf_slot(rt, 3, wcfg.elem_transforms[3], (sp_rc.left, sp_rc.top, sp_rc.right, sp_rc.bottom), s, &mut slice_opt);
                draw_horology_aperture(rt, sp_rc, 5.0 * s, s, &hpal, false);
                let max_v = state.net_history.iter().copied().fold(65_536.0f32, f32::max);
                let sw = (sp_rc.right - sp_rc.left - 8.0 * s).max(8.0);
                let sh = (sp_rc.bottom - sp_rc.top - 8.0 * s).max(6.0);
                let mut prev = None;
                for (i, &val) in state.net_history.iter().enumerate() {
                    let px = sp_rc.left + 4.0 * s + (i as f32 / 31.0) * sw;
                    let py = sp_rc.bottom - 4.0 * s - (val / max_v).clamp(0.0, 1.0) * sh;
                    let cur = Vector2 { X: px, Y: py };
                    if let Some(p0) = prev {
                        rt.DrawLine(p0, cur, &br_lume, (1.4 * s).max(1.0), None);
                    }
                    prev = Some(cur);
                }
                end_elem_xf_slot(rt, g);
            }

            if wcfg.is_slot_visible(2) {
                let fy = y + h - pad - 16.0 * s;
                let mut slice_opt = out_boxes.as_deref_mut().map(|b| &mut b[..]);
                let g = begin_elem_xf_slot(rt, 2, wcfg.elem_transforms[2], (x0 + 134.0 * s, fy, x1, fy + 16.0 * s), s, &mut slice_opt);
                draw_txt(rt, &format!("PING {}ms", state.ping_ms), &tf_micro, D2D_RECT_F { left: x0 + 134.0 * s, top: fy, right: x1 - 66.0 * s, bottom: fy + 16.0 * s }, &br_sub);
                draw_crown_pill(x1 - 64.0 * s, fy, x1, fy + 16.0 * s, "Net ↗", false, is_btn_hov(1));
                end_elem_xf_slot(rt, g);
            }
        }

        // =====================================================================
        // 5. GPU & GAMING TELEMETRY — TWIN RACING INSTRUMENT CLUSTER
        // =====================================================================
        ExtWidgetId::Gpu => {
            if wcfg.is_slot_visible(0) {
                let mut slice_opt = out_boxes.as_deref_mut().map(|b| &mut b[..]);
                let g = begin_elem_xf_slot(rt, 0, wcfg.elem_transforms[0], (x0, y_cur, x1, y_cur + 17.0 * s), s, &mut slice_opt);
                draw_txt(rt, &truncate_str(&state.gpu_name, 22), &tf_hdr, D2D_RECT_F { left: x0, top: y_cur, right: x1 - 82.0 * s, bottom: y_cur + 16.0 * s }, &br_hdr);
                draw_crown_pill(x1 - 78.0 * s, y_cur, x1, y_cur + 17.0 * s, "Focus ↻", false, is_btn_hov(0));
                end_elem_xf_slot(rt, g);
                y_cur += 20.0 * s;
            }
            draw_horo_divider(y_cur - 2.0 * s, &mut out_boxes);

            let dial_r = 26.0 * s;
            let d_cy = y_cur + dial_r + 4.0 * s;
            let load_n = (state.gpu_load_pct / 100.0).clamp(0.0, 1.0);
            let temp_n = ((state.gpu_temp_c - 20.0) / 80.0).clamp(0.0, 1.0);
            let vram_n = if state.vram_total_mb > 1.0 {
                (state.vram_used_mb / state.vram_total_mb).clamp(0.0, 1.0)
            } else {
                0.0
            };

            if wcfg.is_slot_visible(1) {
                let mut slice_opt = out_boxes.as_deref_mut().map(|b| &mut b[..]);
                let g = begin_elem_xf_slot(rt, 1, wcfg.elem_transforms[1], (x0, y_cur, x0 + 132.0 * s, y + h - pad), s, &mut slice_opt);
                let c1 = x0 + 30.0 * s;
                let c2 = x0 + 94.0 * s;
                draw_horology_subdial(rt, d2d, c1, d_cy, dial_r, s, &hpal, load_n, Some(0.80), None);
                draw_horology_subdial(rt, d2d, c2, d_cy, dial_r, s, &hpal, temp_n, Some(0.75), Some(hpal.jewel_rgb));
                draw_txt(rt, &format!("LOAD {:.0}%", state.gpu_load_pct), &tf_micro, D2D_RECT_F { left: c1 - 31.0 * s, top: d_cy + dial_r - 2.0 * s, right: c1 + 31.0 * s, bottom: d_cy + dial_r + 12.0 * s }, &br_txt);
                draw_txt(rt, &format!("TEMP {:.0}°C", state.gpu_temp_c), &tf_micro, D2D_RECT_F { left: c2 - 31.0 * s, top: d_cy + dial_r - 2.0 * s, right: c2 + 31.0 * s, bottom: d_cy + dial_r + 12.0 * s }, &br_sub);
                end_elem_xf_slot(rt, g);
            }

            if wcfg.is_slot_visible(3) {
                let bx0 = x0 + 136.0 * s;
                let mut slice_opt = out_boxes.as_deref_mut().map(|b| &mut b[..]);
                let g = begin_elem_xf_slot(rt, 3, wcfg.elem_transforms[3], (bx0, y_cur + 4.0 * s, x1, y + h - pad - 20.0 * s), s, &mut slice_opt);
                let ap = D2D_RECT_F { left: bx0, top: y_cur + 4.0 * s, right: x1, bottom: y_cur + 28.0 * s };
                draw_horology_aperture(rt, ap, 5.0 * s, s, &hpal, true);
                draw_txt(rt, &format!("VRAM {:.1}/{:.0}G", state.vram_used_mb / 1024.0, (state.vram_total_mb / 1024.0).max(1.0)), &tf_micro, ap, &br_txt);
                draw_horology_slider(
                    rt,
                    D2D_RECT_F { left: bx0, top: y_cur + 32.0 * s, right: x1, bottom: y_cur + 46.0 * s },
                    s,
                    &hpal,
                    vram_n,
                    false,
                );
                end_elem_xf_slot(rt, g);
            }

            if wcfg.is_slot_visible(2) {
                let fy = y + h - pad - 16.0 * s;
                let mut slice_opt = out_boxes.as_deref_mut().map(|b| &mut b[..]);
                let g = begin_elem_xf_slot(rt, 2, wcfg.elem_transforms[2], (x0 + 136.0 * s, fy, x1, fy + 16.0 * s), s, &mut slice_opt);
                draw_txt(rt, &format!("{} MHz", state.gpu_clock_mhz), &tf_micro, D2D_RECT_F { left: x0 + 136.0 * s, top: fy, right: x1 - 68.0 * s, bottom: fy + 16.0 * s }, &br_sub);
                draw_crown_pill(x1 - 66.0 * s, fy, x1, fy + 16.0 * s, "TaskMgr", false, is_btn_hov(1));
                end_elem_xf_slot(rt, g);
            }
        }

        // =====================================================================
        // 6. AUDIO OUTPUT SWITCHER — HI-FI HOROLOGY SELECTOR & KNURLED FADER
        // =====================================================================
        ExtWidgetId::AudioSwitcher => {
            let outputs = &cc_snap.audio_outputs;
            let lay = audio_switcher_layout(wcfg, outputs.len());
            let map_rc = |r: (f32, f32, f32, f32)| D2D_RECT_F {
                left: x + r.0 * s,
                top: y + r.1 * s,
                right: x + r.2 * s,
                bottom: y + r.3 * s,
            };

            if wcfg.is_slot_visible(0) {
                if let Some(hr) = lay.hdr_rect {
                    let rc = map_rc(hr);
                    let mut slice_opt = out_boxes.as_deref_mut().map(|b| &mut b[..]);
                    let g = begin_elem_xf_slot(rt, 0, wcfg.elem_transforms[0], (rc.left, rc.top, rc.right, rc.bottom), s, &mut slice_opt);
                    draw_txt(rt, &format!("{}  •  AUDIO DAC", ht.badge().to_ascii_uppercase()), &tf_hdr, rc, &br_hdr);
                    end_elem_xf_slot(rt, g);
                }
                if let Some(mr) = lay.mixer_btn {
                    let rc = map_rc(mr);
                    draw_crown_pill(rc.left, rc.top, rc.right, rc.bottom, "Mixer", false, is_btn_hov(5));
                }
            }

            if wcfg.is_slot_visible(2) {
                let page_off = state.audio_output_page * 2;
                let dev_name = |slot: usize| -> (String, bool) {
                    if let Some(dev) = outputs.get(page_off + slot) {
                        (truncate_str(&dev.name, 14), dev.is_default)
                    } else if slot == 0 {
                        ("Speakers".to_string(), state.active_audio_out == 0)
                    } else {
                        ("Headphones".to_string(), state.active_audio_out == 1)
                    }
                };
                if let Some(r0) = lay.dev_btn_0 {
                    let rc = map_rc(r0);
                    let (nm, act) = dev_name(0);
                    let mut slice_opt = out_boxes.as_deref_mut().map(|b| &mut b[..]);
                    let g = begin_elem_xf_slot(rt, 2, wcfg.elem_transforms[2], (rc.left, rc.top, x + lay.x1 * s, rc.bottom), s, &mut slice_opt);
                    draw_horology_pusher(rt, rc, 6.0 * s, s, &hpal, act, is_btn_hov(3), true);
                    draw_txt(rt, &nm, &tf_btn, rc, if act { &br_lume } else { &br_txt });
                    if let Some(r1) = lay.dev_btn_1 {
                        let rc1 = map_rc(r1);
                        let (nm1, act1) = dev_name(1);
                        draw_horology_pusher(rt, rc1, 6.0 * s, s, &hpal, act1, is_btn_hov(4), true);
                        draw_txt(rt, &nm1, &tf_btn, rc1, if act1 { &br_lume } else { &br_txt });
                    }
                    if let Some(rcyc) = lay.dev_cycle_btn {
                        let rcc = map_rc(rcyc);
                        draw_crown_pill(rcc.left, rcc.top, rcc.right, rcc.bottom, "›", false, is_btn_hov(6));
                    }
                    end_elem_xf_slot(rt, g);
                }
            }

            if let Some(dy) = lay.divider_y {
                draw_horo_divider(y + dy * s, &mut out_boxes);
            }

            let vol_pct = state.audio_vol_preview;
            let muted = state.audio_muted_preview;
            if wcfg.is_slot_visible(1) {
                if let Some(vr) = lay.vol_label_rect {
                    let rc = map_rc(vr);
                    let mut slice_opt = out_boxes.as_deref_mut().map(|b| &mut b[..]);
                    let g = begin_elem_xf_slot(rt, 1, wcfg.elem_transforms[1], (rc.left, rc.top, rc.right, rc.bottom), s, &mut slice_opt);
                    draw_horology_aperture(rt, rc, 5.0 * s, s, &hpal, false);
                    let vtxt = if muted { "MUTED".to_string() } else { format!("VOL {}%", vol_pct) };
                    draw_txt(rt, &vtxt, &tf_body, D2D_RECT_F { left: rc.left + 8.0 * s, top: rc.top, right: rc.right - 4.0 * s, bottom: rc.bottom }, if muted { &br_sub } else { &br_txt });
                    end_elem_xf_slot(rt, g);
                }
            }

            if wcfg.is_slot_visible(3) {
                if let Some(mr) = lay.vol_minus_btn {
                    let r_m = map_rc(mr);
                    let r_end = lay.vol_plus_btn.map(map_rc).unwrap_or(r_m);
                    let mut slice_opt = out_boxes.as_deref_mut().map(|b| &mut b[..]);
                    let g = begin_elem_xf_slot(rt, 3, wcfg.elem_transforms[3], (r_m.left, r_m.top, r_end.right, r_end.bottom), s, &mut slice_opt);
                    draw_crown_pill(r_m.left, r_m.top, r_m.right, r_m.bottom, "−", false, is_btn_hov(0));
                    if let Some(mu) = lay.vol_mute_btn {
                        let rmu = map_rc(mu);
                        draw_crown_pill(rmu.left, rmu.top, rmu.right, rmu.bottom, if muted { "Unm" } else { "Mute" }, muted, is_btn_hov(1));
                    }
                    if let Some(pl) = lay.vol_plus_btn {
                        let rpl = map_rc(pl);
                        draw_crown_pill(rpl.left, rpl.top, rpl.right, rpl.bottom, "+", false, is_btn_hov(2));
                    }
                    end_elem_xf_slot(rt, g);
                }
            }

            if let Some(tr_r) = lay.slider_track {
                let rc = map_rc(tr_r);
                let frac = if muted { 0.0 } else { (vol_pct as f32 / 100.0).clamp(0.0, 1.0) };
                draw_horology_slider(
                    rt,
                    D2D_RECT_F { left: rc.left, top: rc.top - 3.0 * s, right: rc.right, bottom: rc.bottom + 3.0 * s },
                    s,
                    &hpal,
                    frac,
                    is_btn_hov(7),
                );
            }
        }

        // =====================================================================
        // 7. STICKY NOTES & TO-DO — ATELIER HOROLOGY FOLIO CLIP-PAD
        // =====================================================================
        ExtWidgetId::Notes => {
            if wcfg.is_slot_visible(0) {
                let mut slice_opt = out_boxes.as_deref_mut().map(|b| &mut b[..]);
                let g = begin_elem_xf_slot(rt, 0, wcfg.elem_transforms[0], (x0, y_cur, x1, y_cur + 17.0 * s), s, &mut slice_opt);
                draw_txt(rt, &truncate_str(&cfg.notes_title, 18), &tf_hdr, D2D_RECT_F { left: x0, top: y_cur, right: x1 - 100.0 * s, bottom: y_cur + 17.0 * s }, &br_hdr);
                if wcfg.feats[3] {
                    draw_crown_pill(x + (lw - 106.0) * s, y_cur, x + (lw - 58.0) * s, y_cur + 17.0 * s, "+ Add", state.note_focused, is_btn_hov(4));
                    draw_crown_pill(x + (lw - 54.0) * s, y_cur, x1, y_cur + 17.0 * s, "Clear", false, is_btn_hov(5));
                }
                end_elem_xf_slot(rt, g);
            }
            draw_horo_divider(y + 32.0 * s, &mut out_boxes);

            if wcfg.is_slot_visible(1) {
                let folio_rc = D2D_RECT_F { left: x0, top: y + 36.0 * s, right: x1, bottom: y + h - pad * 0.7 };
                let mut slice_opt = out_boxes.as_deref_mut().map(|b| &mut b[..]);
                let g = begin_elem_xf_slot(rt, 1, wcfg.elem_transforms[1], (folio_rc.left, folio_rc.top, folio_rc.right, folio_rc.bottom), s, &mut slice_opt);
                draw_horology_aperture(rt, folio_rc, 7.0 * s, s, &hpal, false);
                for (i, (done, txt)) in cfg.notes_items.iter().take(4).enumerate() {
                    let ry = y + (38.0 + i as f32 * 30.0) * s;
                    let rh = 27.0 * s;
                    if ry + rh > y + h - 4.0 * s {
                        break;
                    }
                    // Jewel bullet checkbox
                    let bx = x0 + 13.0 * s;
                    let by = ry + rh * 0.5;
                    if let Ok(jb) = rt.CreateSolidColorBrush(&rgb_u32(if *done { hpal.lume_rgb } else { hpal.bezel_rgb }, 0.90), None) {
                        if *done {
                            rt.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: bx, Y: by }, radiusX: 4.5 * s, radiusY: 4.5 * s }, &jb);
                        } else {
                            rt.DrawEllipse(&D2D1_ELLIPSE { point: Vector2 { X: bx, Y: by }, radiusX: 4.5 * s, radiusY: 4.5 * s }, &jb, 1.2 * s, None);
                        }
                    }
                    let line_txt = if state.editing_note_row == Some(i) {
                        format!("{}|", state.note_draft)
                    } else {
                        truncate_str(txt, 26)
                    };
                    draw_txt(
                        rt,
                        &line_txt,
                        &tf_body,
                        D2D_RECT_F { left: x0 + 24.0 * s, top: ry, right: x1 - 24.0 * s, bottom: ry + rh },
                        if *done { &br_sub } else { &br_txt },
                    );
                    if is_btn_hov(i as u8) || is_btn_hov(10 + i as u8) {
                        draw_txt(rt, "✎", &tf_micro, D2D_RECT_F { left: x1 - 22.0 * s, top: ry, right: x1 - 4.0 * s, bottom: ry + rh }, &br_hdr);
                    }
                    if i < 3 {
                        rt.DrawLine(
                            Vector2 { X: x0 + 8.0 * s, Y: ry + rh + 1.5 * s },
                            Vector2 { X: x1 - 8.0 * s, Y: ry + rh + 1.5 * s },
                            &br_div,
                            0.6 * s,
                            None,
                        );
                    }
                }
                end_elem_xf_slot(rt, g);
            }
        }

        // =====================================================================
        // 8. MONTHLY CALENDAR — PERPETUAL CALENDAR COMPLICATION
        // =====================================================================
        ExtWidgetId::Calendar => {
            let now = chrono_local();
            let now_m = month_name_to_num_pub(now.month_name);
            let m_name = month_num_to_name_pub(state.cal_month);

            if wcfg.is_slot_visible(0) {
                let mut slice_opt = out_boxes.as_deref_mut().map(|b| &mut b[..]);
                let g = begin_elem_xf_slot(rt, 0, wcfg.elem_transforms[0], (x0, y_cur, x1, y_cur + 18.0 * s), s, &mut slice_opt);
                let title = format!("{} {}", m_name.to_uppercase(), state.cal_year);
                draw_txt(rt, &title, &tf_hdr, D2D_RECT_F { left: x0, top: y_cur, right: x + (lw - 94.0) * s, bottom: y_cur + 18.0 * s }, &br_hdr);
                if wcfg.feats[2] {
                    draw_crown_pill(x + (lw - 92.0) * s, y_cur, x + (lw - 66.0) * s, y_cur + 17.0 * s, "‹", false, is_btn_hov(0));
                    draw_crown_pill(x + (lw - 63.0) * s, y_cur, x + (lw - 34.0) * s, y_cur + 17.0 * s, "●", false, is_btn_hov(1));
                    draw_crown_pill(x + (lw - 32.0) * s, y_cur, x + (lw - 6.0) * s, y_cur + 17.0 * s, "›", false, is_btn_hov(2));
                }
                end_elem_xf_slot(rt, g);
                y_cur += 21.0 * s;
            }
            draw_horo_divider(y_cur - 2.0 * s, &mut out_boxes);

            let col_w = (x1 - x0) / 7.0;
            if wcfg.is_slot_visible(1) {
                let mut slice_opt = out_boxes.as_deref_mut().map(|b| &mut b[..]);
                let g = begin_elem_xf_slot(rt, 1, wcfg.elem_transforms[1], (x0, y_cur, x1, y_cur + 15.0 * s), s, &mut slice_opt);
                for (i, d) in ["S", "M", "T", "W", "T", "F", "S"].iter().enumerate() {
                    let cx0 = x0 + i as f32 * col_w;
                    draw_txt(rt, d, &tf_micro, D2D_RECT_F { left: cx0, top: y_cur, right: cx0 + col_w, bottom: y_cur + 14.0 * s }, &br_hdr);
                }
                end_elem_xf_slot(rt, g);
                y_cur += 16.0 * s;
            }

            if wcfg.is_slot_visible(2) {
                let grid_h = (y + h - pad - y_cur).max(60.0 * s);
                let row_h = grid_h / 6.0;
                let days = days_in_month_pub(state.cal_year, state.cal_month);
                let first_wd = first_weekday_of_month_pub(state.cal_year, state.cal_month);
                let mut slice_opt = out_boxes.as_deref_mut().map(|b| &mut b[..]);
                let g = begin_elem_xf_slot(rt, 2, wcfg.elem_transforms[2], (x0, y_cur, x1, y_cur + grid_h), s, &mut slice_opt);
                for day in 1..=days {
                    let slot = first_wd + day - 1;
                    let col = slot % 7;
                    let row = slot / 7;
                    if row >= 6 {
                        break;
                    }
                    let cx0 = x0 + col as f32 * col_w;
                    let cy0 = y_cur + row as f32 * row_h;
                    let rc = D2D_RECT_F { left: cx0 + 1.5 * s, top: cy0 + 1.0 * s, right: cx0 + col_w - 1.5 * s, bottom: cy0 + row_h - 1.0 * s };
                    let is_today = state.cal_year == now.year && state.cal_month == now_m && day == now.day;
                    if is_today {
                        draw_horology_aperture(rt, rc, 4.5 * s, s, &hpal, true);
                        if let Ok(tr) = rt.CreateSolidColorBrush(&rgb_u32(hpal.lume_rgb, 0.92), None) {
                            rt.DrawRoundedRectangle(&D2D1_ROUNDED_RECT { rect: rc, radiusX: 4.5 * s, radiusY: 4.5 * s }, &tr, 1.3 * s, None);
                        }
                    }
                    let ds = format!("{}", day);
                    draw_txt(rt, &ds, &tf_micro, rc, if is_today { &br_lume } else { &br_txt });
                }
                end_elem_xf_slot(rt, g);
            }
        }

        // =====================================================================
        // 9. POMODORO FOCUS TIMER — MECHANICAL STOPWATCH CHRONOGRAPH
        // =====================================================================
        ExtWidgetId::Pomodoro => {
            let total_secs = if state.pomo_break { 5 * 60 } else { 25 * 60 };
            let frac = (state.pomo_rem_secs as f32 / total_secs as f32).clamp(0.0, 1.0);
            let mins = state.pomo_rem_secs / 60;
            let secs = state.pomo_rem_secs % 60;
            let play = pomodoro_button_layout(wcfg);

            if wcfg.is_slot_visible(0) {
                let mut slice_opt = out_boxes.as_deref_mut().map(|b| &mut b[..]);
                let g = begin_elem_xf_slot(rt, 0, wcfg.elem_transforms[0], (x0, y_cur, x1, y_cur + 16.0 * s), s, &mut slice_opt);
                let phase_lbl = if state.pomo_break { "CHRONO BREAK" } else { "CHRONO FOCUS" };
                draw_txt(rt, &format!("{}  •  {}", ht.badge().to_ascii_uppercase(), phase_lbl), &tf_hdr, D2D_RECT_F { left: x0, top: y_cur, right: x1, bottom: y_cur + 16.0 * s }, &br_hdr);
                end_elem_xf_slot(rt, g);
                y_cur += 19.0 * s;
            }

            let mut text_x = x0;
            if wcfg.feats[0] && wcfg.is_slot_visible(3) {
                let r_rad = 26.0 * s;
                let cx = x0 + r_rad + 2.0 * s;
                let cy = (y_cur + y + h - pad) * 0.5;
                let mut slice_opt = out_boxes.as_deref_mut().map(|b| &mut b[..]);
                let g = begin_elem_xf_slot(rt, 3, wcfg.elem_transforms[3], (cx - r_rad, cy - r_rad, cx + r_rad, cy + r_rad), s, &mut slice_opt);
                draw_horology_subdial(rt, d2d, cx, cy, r_rad, s, &hpal, frac, Some(0.85), Some(hpal.jewel_rgb));
                end_elem_xf_slot(rt, g);
                text_x = x0 + r_rad * 2.0 + 16.0 * s;
            }

            if wcfg.is_slot_visible(1) {
                let ap = D2D_RECT_F { left: text_x, top: y_cur + 2.0 * s, right: x1, bottom: y_cur + 38.0 * s };
                let mut slice_opt = out_boxes.as_deref_mut().map(|b| &mut b[..]);
                let g = begin_elem_xf_slot(rt, 1, wcfg.elem_transforms[1], (ap.left, ap.top, ap.right, ap.bottom), s, &mut slice_opt);
                draw_horology_aperture(rt, ap, 6.0 * s, s, &hpal, true);
                draw_txt(rt, &format!("{:02}:{:02}", mins, secs), &tf_big_c, ap, &br_txt);
                end_elem_xf_slot(rt, g);
            }

            if wcfg.is_slot_visible(2) {
                let sy = y_cur + 41.0 * s;
                let mut slice_opt = out_boxes.as_deref_mut().map(|b| &mut b[..]);
                let g = begin_elem_xf_slot(rt, 2, wcfg.elem_transforms[2], (text_x, sy, x1, sy + 14.0 * s), s, &mut slice_opt);
                draw_txt(rt, &format!("SESSIONS: {}  •  {}", state.pomo_sessions, if state.pomo_running { "RUNNING" } else { "READY" }), &tf_micro, D2D_RECT_F { left: text_x, top: sy, right: x1, bottom: sy + 14.0 * s }, &br_sub);
                end_elem_xf_slot(rt, g);
            }

            let map_p = |r: (f32, f32, f32, f32)| (x + r.0 * s, y + r.1 * s, x + r.2 * s, y + r.3 * s);
            if let Some(r0) = play.start_btn {
                let b = map_p(r0);
                draw_crown_pill(b.0, b.1, b.2, b.3, if state.pomo_running { "⏸ Pause" } else { "▶ Start" }, state.pomo_running, is_btn_hov(0));
            }
            if let Some(r1) = play.reset_btn {
                let b = map_p(r1);
                draw_crown_pill(b.0, b.1, b.2, b.3, "↺", false, is_btn_hov(1));
            }
            if let Some(r2) = play.skip_btn {
                let b = map_p(r2);
                draw_crown_pill(b.0, b.1, b.2, b.3, "Skip →", false, is_btn_hov(2));
            }
        }

        // =====================================================================
        // 10. MULTI-TIMEZONE WORLD CLOCK — TRUE GMT 24-HOUR MASTERPIECE
        // =====================================================================
        ExtWidgetId::WorldClock => {
            let use_24 = wcfg.feats[0] || cfg.widgets_24h;
            if wcfg.is_slot_visible(0) {
                let mut slice_opt = out_boxes.as_deref_mut().map(|b| &mut b[..]);
                let g = begin_elem_xf_slot(rt, 0, wcfg.elem_transforms[0], (x0, y_cur, x1, y_cur + 17.0 * s), s, &mut slice_opt);
                draw_txt(rt, "GMT WORLD", &tf_hdr, D2D_RECT_F { left: x0, top: y_cur, right: x1 - 90.0 * s, bottom: y_cur + 17.0 * s }, &br_hdr);
                draw_crown_pill(x1 - 88.0 * s, y_cur, x1 - 42.0 * s, y_cur + 17.0 * s, "Cities", false, is_btn_hov(1));
                draw_crown_pill(x1 - 38.0 * s, y_cur, x1, y_cur + 17.0 * s, if use_24 { "24h" } else { "12h" }, use_24, is_btn_hov(0));
                end_elem_xf_slot(rt, g);
                y_cur += 19.0 * s;
            }
            if wcfg.show_dividers {
                draw_horo_divider(y_cur, &mut out_boxes);
                y_cur += 4.0 * s;
            }

            let slots = resolve_world_clock_slots(cfg);
            let max_c = if wcfg.feats[5] { 4 } else { 3 };
            let spacing_mul = (0.78 + (wcfg.param1_pct as f32 / 100.0) * 0.42).clamp(0.75, 1.25);
            let row_h = ((((lh - 6.0) - (y_cur - y) / s) / max_c as f32) * spacing_mul).clamp(16.0, 28.0) * s;
            let st_utc = windows::Win32::System::SystemInformation::GetSystemTime();
            let utc_mins = (st_utc.wHour as i32) * 60 + (st_utc.wMinute as i32);

            if wcfg.is_slot_visible(1) {
                let mut slice_opt = out_boxes.as_deref_mut().map(|b| &mut b[..]);
                let g = begin_elem_xf_slot(rt, 1, wcfg.elem_transforms[1], (x0, y_cur, x1, y + h - 4.0 * s), s, &mut slice_opt);
                for (i, city) in slots.iter().take(max_c).enumerate() {
                    let ry = y_cur + i as f32 * row_h;
                    if ry + row_h > y + h - 2.0 * s {
                        break;
                    }
                    let loc_m = (utc_mins + city.offset_mins).rem_euclid(1440);
                    let hh = (loc_m / 60) as u32;
                    let mm = (loc_m % 60) as u32;
                    // Miniature mechanical GMT watch dial on the left of each city row
                    if wcfg.is_slot_visible(3) {
                        let dr = (row_h * 0.42).clamp(6.5 * s, 11.0 * s);
                        let dcx = x0 + dr + 2.0 * s;
                        let dcy = ry + row_h * 0.5;
                        draw_horology_watch_dial(rt, d2d, dcx, dcy, dr, s * 0.65, &hpal, (hh % 12) as f32, mm as f32, None, 0.0, 0.5);
                    }
                    let c_left = if wcfg.is_slot_visible(3) { x0 + 26.0 * s } else { x0 };
                    let nm = format!("{} ({})", truncate_str(city.name, 11), format_utc_offset(city.offset_mins));
                    draw_txt(rt, &nm, &tf_body, D2D_RECT_F { left: c_left, top: ry, right: x1 - 56.0 * s, bottom: ry + row_h }, if is_btn_hov(10 + i as u8) { &br_lume } else { &br_txt });
                    let t_s = if use_24 {
                        format!("{:02}:{:02}", hh, mm)
                    } else {
                        let h12 = match hh % 12 { 0 => 12, v => v };
                        format!("{}:{:02}{}", h12, mm, if hh >= 12 { "p" } else { "a" })
                    };
                    let ap = D2D_RECT_F { left: x1 - 54.0 * s, top: ry + 2.0 * s, right: x1, bottom: ry + row_h - 2.0 * s };
                    draw_horology_aperture(rt, ap, 4.0 * s, s, &hpal, false);
                    draw_txt(rt, &t_s, &tf_micro, ap, &br_hdr);
                }
                end_elem_xf_slot(rt, g);
            }
        }

        // =====================================================================
        // 11. CLIPBOARD HISTORY — RECESSED INSTRUMENT VAULT
        // =====================================================================
        ExtWidgetId::Clipboard => {
            if wcfg.is_slot_visible(0) {
                let mut slice_opt = out_boxes.as_deref_mut().map(|b| &mut b[..]);
                let g = begin_elem_xf_slot(rt, 0, wcfg.elem_transforms[0], (x0, y_cur, x1, y_cur + 17.0 * s), s, &mut slice_opt);
                draw_txt(rt, "CLIPBOARD VAULT", &tf_hdr, D2D_RECT_F { left: x0, top: y_cur, right: x1 - 54.0 * s, bottom: y_cur + 17.0 * s }, &br_hdr);
                if wcfg.feats[3] {
                    draw_crown_pill(x + (lw - 54.0) * s, y_cur, x1, y_cur + 17.0 * s, "Clear", false, is_btn_hov(4));
                }
                end_elem_xf_slot(rt, g);
            }
            draw_horo_divider(y + 29.0 * s, &mut out_boxes);

            if wcfg.is_slot_visible(1) {
                let mut slice_opt = out_boxes.as_deref_mut().map(|b| &mut b[..]);
                let g = begin_elem_xf_slot(rt, 1, wcfg.elem_transforms[1], (x0, y + 32.0 * s, x1, y + h - pad * 0.5), s, &mut slice_opt);
                for i in 0..3 {
                    let ry = y + (32.0 + i as f32 * 24.0) * s;
                    let rc = D2D_RECT_F { left: x0, top: ry, right: x1, bottom: ry + 21.0 * s };
                    let flashed = state.clip_copied_flash.map(|(fi, t)| fi == i && t.elapsed().as_millis() < 900).unwrap_or(false);
                    draw_horology_pusher(rt, rc, 5.0 * s, s, &hpal, flashed, is_btn_hov(i as u8), flashed);
                    let (label, len) = state
                        .clip_items
                        .get(i)
                        .map(|(t, l)| (truncate_str(t, 22), *l))
                        .unwrap_or_else(|| ("Empty vault slot".to_string(), 0));
                    draw_txt(rt, &format!("#{}  {}", i + 1, label), &tf_body, D2D_RECT_F { left: rc.left + 6.0 * s, top: rc.top, right: rc.right - 34.0 * s, bottom: rc.bottom }, if state.clip_items.get(i).is_some() { &br_txt } else { &br_sub });
                    if len > 0 {
                        draw_txt(rt, &format!("{}c", len), &tf_micro, D2D_RECT_F { left: rc.right - 32.0 * s, top: rc.top, right: rc.right - 4.0 * s, bottom: rc.bottom }, &br_hdr);
                    }
                }
                end_elem_xf_slot(rt, g);
            }
        }

        // =====================================================================
        // 12. DRIVE & STORAGE RINGS — MECHANICAL AVIATION FUEL GAUGES
        // =====================================================================
        ExtWidgetId::Drives => {
            if wcfg.is_slot_visible(0) {
                let mut slice_opt = out_boxes.as_deref_mut().map(|b| &mut b[..]);
                let g = begin_elem_xf_slot(rt, 0, wcfg.elem_transforms[0], (x0, y_cur, x1, y_cur + 17.0 * s), s, &mut slice_opt);
                draw_txt(rt, "STORAGE GAUGES", &tf_hdr, D2D_RECT_F { left: x0, top: y_cur, right: x1 - 58.0 * s, bottom: y_cur + 17.0 * s }, &br_hdr);
                draw_crown_pill(x1 - 56.0 * s, y_cur, x1, y_cur + 17.0 * s, "Mode", false, is_btn_hov(4));
                end_elem_xf_slot(rt, g);
                y_cur += 19.0 * s;
            }
            if wcfg.show_dividers {
                draw_horo_divider(y_cur, &mut out_boxes);
                y_cur += 4.0 * s;
            }

            if wcfg.is_slot_visible(1) {
                let mut slice_opt = out_boxes.as_deref_mut().map(|b| &mut b[..]);
                let g = begin_elem_xf_slot(rt, 1, wcfg.elem_transforms[1], (x0, y_cur, x1, y + h - pad * 0.5), s, &mut slice_opt);
                let drives_len = state.drives.len().min(2);
                if drives_len == 0 {
                    draw_txt(rt, "No drives detected", &tf_body, D2D_RECT_F { left: x0, top: y_cur + 10.0 * s, right: x1, bottom: y_cur + 32.0 * s }, &br_sub);
                } else if wcfg.mode_idx == 1 || wcfg.mode_idx == 2 {
                    let thick = (3.0 + (wcfg.param1_pct as f32 / 100.0) * 5.0).clamp(5.0, 11.0);
                    let row_step = (thick + 26.0) * s;
                    for (i, d) in state.drives.iter().take(2).enumerate() {
                        let ry = y_cur + i as f32 * row_step;
                        draw_txt(rt, &format!("{}:  {:.0}/{:.0} GB ({:.0}%)", d.letter, d.used_gb, d.total_gb, d.pct), &tf_body, D2D_RECT_F { left: x0, top: ry, right: x1, bottom: ry + 16.0 * s }, if is_btn_hov(i as u8) { &br_lume } else { &br_txt });
                        draw_horology_slider(rt, D2D_RECT_F { left: x0, top: ry + 17.0 * s, right: x1, bottom: ry + 17.0 * s + thick * s }, s, &hpal, (d.pct / 100.0).clamp(0.0, 1.0), is_btn_hov(i as u8));
                    }
                } else {
                    let half = (x1 - x0) * 0.5;
                    let gr = ((y + h - pad - y_cur) * 0.34).clamp(18.0 * s, 29.0 * s);
                    let gcy = y_cur + gr + 4.0 * s;
                    for (i, d) in state.drives.iter().take(2).enumerate() {
                        let cx = x0 + half * (i as f32 + 0.5);
                        draw_horology_subdial(rt, d2d, cx, gcy, gr, s, &hpal, (d.pct / 100.0).clamp(0.0, 1.0), Some(0.85), None);
                        let lbl = format!("{}: {:.0}%", d.letter, d.pct);
                        draw_txt(rt, &lbl, &tf_micro, D2D_RECT_F { left: cx - half * 0.48, top: gcy + gr - 1.0 * s, right: cx + half * 0.48, bottom: gcy + gr + 13.0 * s }, if is_btn_hov(i as u8) { &br_lume } else { &br_txt });
                    }
                }
                end_elem_xf_slot(rt, g);
            }
        }

        // =====================================================================
        // 13. TOP PROCESS KILLER — MACHINED TELEMETRY RACK
        // =====================================================================
        ExtWidgetId::Processes => {
            if wcfg.is_slot_visible(0) {
                let mut slice_opt = out_boxes.as_deref_mut().map(|b| &mut b[..]);
                let g = begin_elem_xf_slot(rt, 0, wcfg.elem_transforms[0], (x0, y_cur, x1, y_cur + 17.0 * s), s, &mut slice_opt);
                draw_txt(rt, "PROCESS RACK", &tf_hdr, D2D_RECT_F { left: x0, top: y_cur, right: x1 - 52.0 * s, bottom: y_cur + 17.0 * s }, &br_hdr);
                draw_crown_pill(x1 - 50.0 * s, y_cur, x1, y_cur + 17.0 * s, "Mgr", false, is_btn_hov(4));
                end_elem_xf_slot(rt, g);
                y_cur += 19.0 * s;
            }
            if wcfg.show_dividers {
                draw_horo_divider(y_cur, &mut out_boxes);
                y_cur += 4.0 * s;
            }

            let max_p = if wcfg.mode_idx == 3 { 4 } else { 3 };
            let row_h = (((lh - 6.0) - (y_cur - y) / s) / max_p as f32).clamp(20.0, 28.0) * s;
            let max_mem = state.top_procs.iter().map(|p| p.mem_mb).fold(256.0f32, f32::max);

            if wcfg.is_slot_visible(1) {
                let mut slice_opt = out_boxes.as_deref_mut().map(|b| &mut b[..]);
                let g = begin_elem_xf_slot(rt, 1, wcfg.elem_transforms[1], (x0, y_cur, x1, y + h - 4.0 * s), s, &mut slice_opt);
                for (i, p) in state.top_procs.iter().take(max_p).enumerate() {
                    let ry = y_cur + i as f32 * row_h;
                    if ry + row_h > y + h - 3.0 * s {
                        break;
                    }
                    let bar_right = if wcfg.feats[2] { x1 - 24.0 * s } else { x1 };
                    let ap = D2D_RECT_F { left: x0, top: ry + 1.5 * s, right: bar_right, bottom: ry + row_h - 2.0 * s };
                    draw_horology_aperture(rt, ap, 4.0 * s, s, &hpal, false);
                    let frac = (p.mem_mb / max_mem).clamp(0.05, 1.0);
                    if let Ok(fb) = rt.CreateSolidColorBrush(&rgb_u32(hpal.lume_rgb, 0.22), None) {
                        rt.FillRoundedRectangle(
                            &D2D1_ROUNDED_RECT {
                                rect: D2D_RECT_F { left: ap.left + 1.0 * s, top: ap.top + 1.0 * s, right: ap.left + (ap.right - ap.left - 2.0 * s) * frac, bottom: ap.bottom - 1.0 * s },
                                radiusX: 3.0 * s,
                                radiusY: 3.0 * s,
                            },
                            &fb,
                        );
                    }
                    draw_txt(rt, &truncate_str(&p.name, 14), &tf_body, D2D_RECT_F { left: ap.left + 6.0 * s, top: ap.top, right: ap.right - 46.0 * s, bottom: ap.bottom }, &br_txt);
                    draw_txt(rt, &format!("{:.0}M", p.mem_mb), &tf_micro, D2D_RECT_F { left: ap.right - 46.0 * s, top: ap.top, right: ap.right - 4.0 * s, bottom: ap.bottom }, &br_hdr);
                    if wcfg.feats[2] {
                        let krc = D2D_RECT_F { left: x1 - 21.0 * s, top: ry + 2.0 * s, right: x1, bottom: ry + row_h - 2.0 * s };
                        draw_horology_pusher(rt, krc, 4.0 * s, s, &hpal, is_btn_hov(i as u8), is_btn_hov(i as u8), false);
                        if let Ok(kb) = rt.CreateSolidColorBrush(&rgb_u32(hpal.jewel_rgb, 0.96), None) {
                            draw_txt(rt, "×", &tf_btn, krc, &kb);
                        }
                    }
                }
                end_elem_xf_slot(rt, g);
            }
        }

        // =====================================================================
        // 14. RECYCLE BIN — PRECISION CAPACITY VAULT
        // =====================================================================
        ExtWidgetId::RecycleBin => {
            let cap_norm = ((state.bin_bytes as f32) / (500.0 * 1024.0 * 1024.0)).clamp(0.0, 1.0);
            let mb = state.bin_bytes as f32 / (1024.0 * 1024.0);
            let size_str = if mb >= 1024.0 {
                format!("{:.2} GB", mb / 1024.0)
            } else {
                format!("{:.1} MB", mb)
            };

            if wcfg.is_slot_visible(3) {
                let r = 27.0 * s;
                let cx = x0 + r + 4.0 * s;
                let cy = y + h * 0.5;
                let mut slice_opt = out_boxes.as_deref_mut().map(|b| &mut b[..]);
                let g = begin_elem_xf_slot(rt, 3, wcfg.elem_transforms[3], (cx - r, cy - r, cx + r, cy + r), s, &mut slice_opt);
                draw_horology_subdial(rt, d2d, cx, cy, r, s, &hpal, cap_norm, Some(0.80), None);
                end_elem_xf_slot(rt, g);
            }

            let tx0 = x0 + 66.0 * s;
            let tx1 = x + (lw - 98.0) * s;
            if wcfg.is_slot_visible(0) {
                let mut slice_opt = out_boxes.as_deref_mut().map(|b| &mut b[..]);
                let g = begin_elem_xf_slot(rt, 0, wcfg.elem_transforms[0], (tx0, y + 14.0 * s, tx1, y + 32.0 * s), s, &mut slice_opt);
                draw_txt(rt, "RECYCLE VAULT", &tf_hdr, D2D_RECT_F { left: tx0, top: y + 14.0 * s, right: tx1, bottom: y + 31.0 * s }, &br_hdr);
                end_elem_xf_slot(rt, g);
            }
            if wcfg.is_slot_visible(1) {
                let ap = D2D_RECT_F { left: tx0, top: y + 33.0 * s, right: tx1, bottom: y + 60.0 * s };
                let mut slice_opt = out_boxes.as_deref_mut().map(|b| &mut b[..]);
                let g = begin_elem_xf_slot(rt, 1, wcfg.elem_transforms[1], (ap.left, ap.top, ap.right, ap.bottom), s, &mut slice_opt);
                draw_horology_aperture(rt, ap, 5.0 * s, s, &hpal, true);
                draw_txt(rt, &format!("{} items • {}", state.bin_items, size_str), &tf_body, D2D_RECT_F { left: ap.left + 6.0 * s, top: ap.top, right: ap.right - 4.0 * s, bottom: ap.bottom }, &br_txt);
                end_elem_xf_slot(rt, g);
            }
            if wcfg.is_slot_visible(2) {
                draw_txt(rt, if state.bin_items == 0 { "Vault empty" } else { "Click left to open" }, &tf_micro, D2D_RECT_F { left: tx0, top: y + 62.0 * s, right: tx1, bottom: y + 78.0 * s }, &br_sub);
            }
            if wcfg.feats[2] && wcfg.is_slot_visible(4) {
                let brc = D2D_RECT_F { left: x + (lw - 92.0) * s, top: y + 26.0 * s, right: x1, bottom: y + 66.0 * s };
                let mut slice_opt = out_boxes.as_deref_mut().map(|b| &mut b[..]);
                let g = begin_elem_xf_slot(rt, 4, wcfg.elem_transforms[4], (brc.left, brc.top, brc.right, brc.bottom), s, &mut slice_opt);
                draw_crown_pill(brc.left, brc.top, brc.right, brc.bottom, "Empty", false, is_btn_hov(1));
                end_elem_xf_slot(rt, g);
            }
        }
    }

    // Final Curved Sapphire Crystal AR Glare Overlay (scaled by glare_pct)
    if wcfg.show_plate && tr_card.visible {
        draw_sapphire_ar_glare(rt, d2d, card_rect, rad, s, &hpal, inter.hover_x, inter.hover_mix);
    }
}
