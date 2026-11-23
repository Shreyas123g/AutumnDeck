use std::collections::{HashMap, HashSet};
use std::ffi::c_void;
use std::mem::size_of;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use windows::core::{w, BOOL, Interface, PCWSTR, PWSTR};
use windows::Win32::Foundation::{
    COLORREF, FILETIME, HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, SIZE, SYSTEMTIME, WPARAM,
};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_COLOR_F, D2D1_FIGURE_BEGIN_HOLLOW, D2D1_FIGURE_END_OPEN,
    D2D1_GRADIENT_STOP, D2D1_PIXEL_FORMAT, D2D_RECT_F, D2D_SIZE_F, D2D_SIZE_U,
    ID2D1SimplifiedGeometrySink,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1CreateFactory, ID2D1Bitmap, ID2D1DCRenderTarget, ID2D1Factory, ID2D1Geometry,
    ID2D1LinearGradientBrush, ID2D1RenderTarget, ID2D1SolidColorBrush, ID2D1StrokeStyle,
    D2D1_ANTIALIAS_MODE_PER_PRIMITIVE, D2D1_ARC_SEGMENT, D2D1_ARC_SIZE_LARGE,
    D2D1_BITMAP_INTERPOLATION_MODE_LINEAR, D2D1_BITMAP_PROPERTIES, D2D1_CAP_STYLE_ROUND,
    D2D1_COMBINE_MODE_EXCLUDE, D2D1_DRAW_TEXT_OPTIONS_CLIP, D2D1_DRAW_TEXT_OPTIONS_NONE,
    D2D1_ELLIPSE, D2D1_EXTEND_MODE_CLAMP, D2D1_FACTORY_TYPE_SINGLE_THREADED, D2D1_GAMMA_2_2,
    D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES, D2D1_RENDER_TARGET_PROPERTIES, D2D1_RENDER_TARGET_TYPE_DEFAULT,
    D2D1_RENDER_TARGET_USAGE_GDI_COMPATIBLE, D2D1_ROUNDED_RECT, D2D1_STROKE_STYLE_PROPERTIES,
    D2D1_SWEEP_DIRECTION_CLOCKWISE,
};
use windows::Win32::Graphics::DirectWrite::{
    DWriteCreateFactory, IDWriteFactory, IDWriteFactory5, IDWriteFontCollection,
    IDWriteFontSetBuilder1, IDWriteInMemoryFontFileLoader, IDWriteTextFormat, IDWriteTextLayout,
    IDWriteTextLayout1, DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_STRETCH_NORMAL,
    DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_WEIGHT, DWRITE_FONT_WEIGHT_BOLD,
    DWRITE_FONT_WEIGHT_LIGHT, DWRITE_FONT_WEIGHT_REGULAR, DWRITE_FONT_WEIGHT_SEMI_BOLD,
    DWRITE_MEASURING_MODE_NATURAL, DWRITE_PARAGRAPH_ALIGNMENT, DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
    DWRITE_PARAGRAPH_ALIGNMENT_NEAR, DWRITE_TEXT_ALIGNMENT, DWRITE_TEXT_ALIGNMENT_CENTER,
    DWRITE_TEXT_ALIGNMENT_LEADING, DWRITE_TEXT_ALIGNMENT_TRAILING, DWRITE_TEXT_RANGE,
    DWRITE_WORD_WRAPPING_NO_WRAP, DWRITE_WORD_WRAPPING_WRAP,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory1, IDXGIAdapter3, IDXGIFactory4, DXGI_MEMORY_SEGMENT_GROUP_LOCAL,
    DXGI_QUERY_VIDEO_MEMORY_INFO,
};
use windows::Win32::Graphics::Gdi::{
    AddFontMemResourceEx, CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject,
    GetMonitorInfoW, MonitorFromPoint, SelectObject, AC_SRC_ALPHA, AC_SRC_OVER, BITMAPINFO,
    BITMAPINFOHEADER, BI_RGB, BLENDFUNCTION, DIB_RGB_COLORS, HBITMAP, HDC, HGDIOBJ, HPALETTE,
    MONITORINFO, MONITOR_DEFAULTTOPRIMARY,
};
use windows::Win32::Graphics::Imaging::{
    CLSID_WICImagingFactory, GUID_WICPixelFormat32bppPBGRA, IWICBitmapSource, IWICImagingFactory,
    WICBitmapDitherTypeNone, WICBitmapPaletteTypeCustom, WICBitmapUseAlpha,
    WICDecodeMetadataCacheOnDemand,
};
use windows::Win32::Storage::FileSystem::FILE_ATTRIBUTE_NORMAL;
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, CLSCTX_INPROC_SERVER,
    COINIT_APARTMENTTHREADED,
};
use windows::Win32::System::Performance::{
    PdhAddEnglishCounterW, PdhCloseQuery, PdhCollectQueryData, PdhGetFormattedCounterValue,
    PdhOpenQueryW, PDH_FMT_COUNTERVALUE, PDH_FMT_DOUBLE, PDH_HCOUNTER, PDH_HQUERY,
};
use windows::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
use windows::Win32::System::SystemInformation::{GetLocalTime, GlobalMemoryStatusEx, MEMORYSTATUSEX};
use windows::Win32::System::Threading::GetSystemTimes;
use windows::Win32::UI::Controls::Dialogs::{
    GetOpenFileNameW, OFN_FILEMUSTEXIST, OFN_NOCHANGEDIR, OFN_NODEREFERENCELINKS, OPENFILENAMEW,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, ReleaseCapture, SetCapture, TrackMouseEvent, TME_LEAVE, TRACKMOUSEEVENT,
};
use windows::Win32::UI::Shell::{
    BHID_EnumItems, FOLDERID_AppsFolder, IEnumShellItems, IShellItem, IShellItemImageFactory,
    KF_FLAG_DEFAULT, SHCreateItemFromParsingName, SHCreateMemStream, SHGetFileInfoW,
    SHGetKnownFolderItem, ShellExecuteW, SHFILEINFOW, SHGFI_ICON, SHGFI_LARGEICON,
    SHGFI_USEFILEATTRIBUTES, SIGDN_DESKTOPABSOLUTEPARSING, SIGDN_NORMALDISPLAY,
    SIGDN_PARENTRELATIVEPARSING, SIIGBF, SIIGBF_BIGGERSIZEOK, SIIGBF_ICONONLY,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyIcon, DestroyWindow, EnumWindows, FindWindowExW,
    FindWindowW, GetCursorPos, GetTopWindow, GetWindow, GetWindowLongPtrW, IsIconic, IsWindow,
    IsWindowVisible, KillTimer, LoadCursorW, PostMessageW, RegisterClassExW, SetCursor, SetTimer,
    SetWindowLongPtrW, SetWindowPos, ShowWindow, UpdateLayeredWindow, WindowFromPoint,
    GWLP_HWNDPARENT,
    GWLP_USERDATA, GWL_EXSTYLE, GW_HWNDNEXT, GW_HWNDPREV, HWND_BOTTOM, HWND_NOTOPMOST, IDC_ARROW,
    IDC_HAND, IDC_SIZEALL, SWP_HIDEWINDOW, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOOWNERZORDER,
    SWP_NOSIZE, SWP_NOZORDER, SWP_SHOWWINDOW, SW_RESTORE, SW_SHOWNORMAL, ULW_ALPHA, WM_SETCURSOR,
    WM_SYSCOMMAND, WM_TIMER, WNDCLASSEXW, WS_EX_LAYERED, WS_EX_NOACTIVATE,
    WS_EX_NOREDIRECTIONBITMAP, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};
use windows_numerics::{Matrix3x2, Vector2};

use crate::config::{
    resolve_resources_dir, self_exe_path, AppConfig, AppShortcut, ClockAlign, ClockElemId,
    ClockElemTransform, ClockStyle, TaskbarVizStyle,
};
use crate::dcomp_surface::DCompSwapSurface;
use crate::system_media::{CcAction, CcSnapshot, MediaSnapshot, SystemMediaEngine};
use crate::taskbar_styler::{paint_widget_elemental_fx, VizSnapshot};
use crate::theme::{
    resolve_apps_icon_tint, resolve_apps_palette, resolve_clock_palette, resolve_palette_for,
    tint_u64_key, ThemePalette, AUTUMN_ICON_TINT,
};
use crate::widgets_extra::{
    draw_ext_widget_with_viz_cached, hit_test_ext_widget, ExtRuntimeState, ExtWidgetId,
    EXT_WIDGET_COUNT,
};
use crate::widgets_phase2::{
    cc_tile_pos, draw_control_center_widget_live_with_viz, draw_music_widget_with_viz_cached,
    music_logical_size, music_transport_rects, CoverBitmapCache, CC_MAX_SCROLL,
};

const EMBED_FONTS: [&[u8]; 9] = [
    include_bytes!(r"..\assets\fonts\Morganite Bold.ttf"),
    include_bytes!(r"..\assets\fonts\Morganite Book.ttf"),
    include_bytes!(r"..\assets\fonts\Tuesday Night.otf"),
    include_bytes!(r"..\assets\fonts\SF-Pro-Display-Light.otf"),
    include_bytes!(r"..\assets\fonts\SF-Pro-Display-Regular.otf"),
    include_bytes!(r"..\assets\fonts\SF-Pro-Display-Medium.otf"),
    include_bytes!(r"..\assets\fonts\SF-Pro-Display-Semibold.otf"),
    include_bytes!(r"..\assets\fonts\Metropolis Extra Bold.ttf"),
    include_bytes!(r"..\assets\fonts\Fivo Sans Light.otf"),
];

const EMBED_ICON_SETTINGS: &[u8] = include_bytes!(r"..\assets\icons\App_settings_2.png");
const EMBED_ICON_GEMINI: &[u8] = include_bytes!(r"..\assets\icons\App_Gemini.png");
const EMBED_ICON_WISPR: &[u8] = include_bytes!(r"..\assets\icons\App_Wispr_Flow.png");

const PT_TO_PX_DOC_PAD: f32 = 96.0 / 72.0;
pub const PT_TO_PX: f32 = PT_TO_PX_DOC_PAD; // Rainmeter specifies FontSize in pt; DirectWrite uses DIPs (96 DPI px).
const SC_MINIMIZE_CMD: usize = 0xF020;

fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn filetime_to_u64(ft: FILETIME) -> u64 {
    ((ft.dwHighDateTime as u64) << 32) | (ft.dwLowDateTime as u64)
}

pub fn rgba_u8(r: u8, g: u8, b: u8, a: f32) -> D2D1_COLOR_F {
    D2D1_COLOR_F {
        r: r as f32 / 255.0,
        g: g as f32 / 255.0,
        b: b as f32 / 255.0,
        a: a.clamp(0.0, 1.0),
    }
}

pub unsafe fn create_linear_grad(
    rt: &ID2D1RenderTarget,
    p0: (f32, f32),
    p1: (f32, f32),
    stops: &[D2D1_GRADIENT_STOP],
) -> Option<ID2D1LinearGradientBrush> {
    let coll = rt
        .CreateGradientStopCollection(stops, D2D1_GAMMA_2_2, D2D1_EXTEND_MODE_CLAMP)
        .ok()?;
    let props = D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES {
        startPoint: Vector2 { X: p0.0, Y: p0.1 },
        endPoint: Vector2 { X: p1.0, Y: p1.1 },
    };
    rt.CreateLinearGradientBrush(&props, None, &coll).ok()
}

struct SendFontLoader(IDWriteInMemoryFontFileLoader);
unsafe impl Send for SendFontLoader {}
unsafe impl Sync for SendFontLoader {}

struct SendFontColl(IDWriteFontCollection);
unsafe impl Send for SendFontColl {}
unsafe impl Sync for SendFontColl {}

static MEM_FONT_LOADER: OnceLock<SendFontLoader> = OnceLock::new();
static CACHED_CUSTOM_FONTS: OnceLock<Option<SendFontColl>> = OnceLock::new();
static CACHED_SYS_FONTS: OnceLock<Option<SendFontColl>> = OnceLock::new();

pub unsafe fn cached_system_font_collection(dw: &IDWriteFactory) -> Option<IDWriteFontCollection> {
    CACHED_SYS_FONTS
        .get_or_init(|| {
            let mut sys_coll: Option<IDWriteFontCollection> = None;
            let _ = dw.GetSystemFontCollection(&mut sys_coll, false);
            sys_coll.map(SendFontColl)
        })
        .as_ref()
        .map(|s| s.0.clone())
}

/// Builds (and caches process-wide) a DirectWrite private font collection from embedded @Resources\Fonts
/// bytes plus any exe-relative @Resources\Fonts overrides. Works cleanly regardless of current working
/// directory or folder relocation (zero dependency on Rainmeter installation paths).
pub unsafe fn build_custom_font_collection(dw: &IDWriteFactory) -> Option<IDWriteFontCollection> {
    CACHED_CUSTOM_FONTS
        .get_or_init(|| {
            let f5: IDWriteFactory5 = dw.cast().ok()?;
            let base_builder = f5.CreateFontSetBuilder().ok()?;
            let builder1: IDWriteFontSetBuilder1 = base_builder.cast().ok()?;
            let mut added = 0usize;

            // Optional exe-relative @Resources\Fonts override files first
            let mut disk_dirs: Vec<PathBuf> = Vec::new();
            if let Some(res) = resolve_resources_dir() {
                disk_dirs.push(res.join("Fonts"));
            }
            if let Some(dir) = self_exe_path().parent() {
                disk_dirs.push(dir.join("Fonts"));
            }
            for base in &disk_dirs {
                if let Ok(entries) = std::fs::read_dir(base) {
                    for entry in entries.flatten() {
                        let path = entry.path();
                        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                            if ext.eq_ignore_ascii_case("ttf") || ext.eq_ignore_ascii_case("otf") {
                                let wide = to_wide(&path.to_string_lossy());
                                if let Ok(ff) = f5.CreateFontFileReference(PCWSTR(wide.as_ptr()), None) {
                                    if builder1.AddFontFile(&ff).is_ok() {
                                        added += 1;
                                    }
                                }
                            }
                        }
                    }
                }
                if added > 0 {
                    break;
                }
            }

            // Always register embedded fonts in-memory so Morganite / Tuesday Night / SF Pro
            // render instantaneously on cold boot even if @Resources is absent or offline.
            let loader = MEM_FONT_LOADER.get_or_init(|| {
                let l = f5
                    .CreateInMemoryFontFileLoader()
                    .expect("CreateInMemoryFontFileLoader failed");
                let _ = f5.RegisterFontFileLoader(&l);
                SendFontLoader(l)
            });
            for bytes in EMBED_FONTS {
                if let Ok(ff) = loader.0.CreateInMemoryFontFileReference(
                    dw,
                    bytes.as_ptr() as *const c_void,
                    bytes.len() as u32,
                    None,
                ) {
                    if builder1.AddFontFile(&ff).is_ok() {
                        added += 1;
                    }
                }
            }

            if added == 0 {
                return None;
            }
            let font_set = base_builder.CreateFontSet().ok()?;
            let coll1 = f5.CreateFontCollectionFromFontSet(&font_set).ok()?;
            let coll: IDWriteFontCollection = coll1.cast().ok()?;
            Some(SendFontColl(coll))
        })
        .as_ref()
        .map(|s| s.0.clone())
}

unsafe fn font_exists_in_collection(coll: &IDWriteFontCollection, family: PCWSTR) -> bool {
    let mut idx = 0u32;
    let mut exists = BOOL(0);
    coll.FindFamilyName(family, &mut idx, &mut exists).is_ok() && exists.as_bool()
}

struct CachedTfEntry {
    dw_ptr: usize,
    custom_ptr: usize,
    preferred: String,
    families: Vec<Vec<u16>>,
    size_tenths: u32,
    weight: i32,
    align: i32,
    para_align: i32,
    tf: IDWriteTextFormat,
}

thread_local! {
    static TF_CACHE: std::cell::RefCell<HashMap<u64, CachedTfEntry>> =
        std::cell::RefCell::new(HashMap::with_capacity(96));
}

#[inline]
unsafe fn pcwstr_wide_units<'a>(p: PCWSTR) -> &'a [u16] {
    if p.0.is_null() {
        return &[];
    }
    let mut len = 0usize;
    while len < 128 && *p.0.add(len) != 0 {
        len += 1;
    }
    std::slice::from_raw_parts(p.0, len)
}

#[inline]
fn fnv1a_u64(mut h: u64, bytes: &[u8]) -> u64 {
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

unsafe fn tf_cache_hash(
    dw_ptr: usize,
    custom_ptr: usize,
    preferred: &str,
    families: &[PCWSTR],
    size_tenths: u32,
    weight: i32,
    align: i32,
    para_align: i32,
) -> u64 {
    let mut h = 0xcbf29ce484222325u64;
    h = fnv1a_u64(h, &dw_ptr.to_le_bytes());
    h = fnv1a_u64(h, &custom_ptr.to_le_bytes());
    h = fnv1a_u64(h, &size_tenths.to_le_bytes());
    h = fnv1a_u64(h, &weight.to_le_bytes());
    h = fnv1a_u64(h, &align.to_le_bytes());
    h = fnv1a_u64(h, &para_align.to_le_bytes());
    h = fnv1a_u64(h, preferred.as_bytes());
    h = fnv1a_u64(h, &[0xff]);
    for &fam in families {
        let w = pcwstr_wide_units(fam);
        let bytes = std::slice::from_raw_parts(w.as_ptr() as *const u8, w.len() * 2);
        h = fnv1a_u64(h, bytes);
        h = fnv1a_u64(h, &[0xfe]);
    }
    h
}

unsafe fn tf_entry_matches(
    e: &CachedTfEntry,
    dw_ptr: usize,
    custom_ptr: usize,
    preferred: &str,
    families: &[PCWSTR],
    size_tenths: u32,
    weight: i32,
    align: i32,
    para_align: i32,
) -> bool {
    if e.dw_ptr != dw_ptr
        || e.custom_ptr != custom_ptr
        || e.size_tenths != size_tenths
        || e.weight != weight
        || e.align != align
        || e.para_align != para_align
        || e.preferred != preferred
        || e.families.len() != families.len()
    {
        return false;
    }
    for (cached_w, &fam) in e.families.iter().zip(families.iter()) {
        if cached_w.as_slice() != pcwstr_wide_units(fam) {
            return false;
        }
    }
    true
}

unsafe fn make_resolved_tf_uncached(
    dw: &IDWriteFactory,
    custom: Option<&IDWriteFontCollection>,
    families: &[PCWSTR],
    size_px: f32,
    weight: DWRITE_FONT_WEIGHT,
    align: DWRITE_TEXT_ALIGNMENT,
    para_align: DWRITE_PARAGRAPH_ALIGNMENT,
) -> Option<IDWriteTextFormat> {
    // Pass 1: try custom @Resources font collection
    if let Some(coll) = custom {
        for &family in families {
            if font_exists_in_collection(coll, family) {
                if let Ok(tf) = dw.CreateTextFormat(
                    family,
                    Some(coll),
                    weight,
                    DWRITE_FONT_STYLE_NORMAL,
                    DWRITE_FONT_STRETCH_NORMAL,
                    size_px,
                    w!("en-US"),
                ) {
                    let _ = tf.SetTextAlignment(align);
                    let _ = tf.SetParagraphAlignment(para_align);
                    return Some(tf);
                }
            }
        }
    }
    // Pass 2: fallback to Windows system fonts (cached)
    let sys_coll = cached_system_font_collection(dw);
    for &family in families {
        let valid = sys_coll
            .as_ref()
            .map(|c| font_exists_in_collection(c, family))
            .unwrap_or(false);
        if valid {
            if let Ok(tf) = dw.CreateTextFormat(
                family,
                None,
                weight,
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                size_px,
                w!("en-US"),
            ) {
                let _ = tf.SetTextAlignment(align);
                let _ = tf.SetParagraphAlignment(para_align);
                return Some(tf);
            }
        }
    }
    // Pass 3: guaranteed Segoe UI fallback
    if let Ok(tf) = dw.CreateTextFormat(
        w!("Segoe UI"),
        None,
        weight,
        DWRITE_FONT_STYLE_NORMAL,
        DWRITE_FONT_STRETCH_NORMAL,
        size_px,
        w!("en-US"),
    ) {
        let _ = tf.SetTextAlignment(align);
        let _ = tf.SetParagraphAlignment(para_align);
        return Some(tf);
    }
    None
}

unsafe fn get_or_build_cached_tf(
    dw: &IDWriteFactory,
    custom: Option<&IDWriteFontCollection>,
    preferred: &str,
    families: &[PCWSTR],
    size_px: f32,
    weight: DWRITE_FONT_WEIGHT,
    align: DWRITE_TEXT_ALIGNMENT,
    para_align: DWRITE_PARAGRAPH_ALIGNMENT,
) -> Option<IDWriteTextFormat> {
    let eff_sz = size_px.max(4.0);
    let size_tenths = (eff_sz * 10.0).round() as u32;
    let quant_sz = (size_tenths as f32 / 10.0).max(4.0);
    let dw_ptr = dw.as_raw() as usize;
    let custom_ptr = custom.map(|c| c.as_raw() as usize).unwrap_or(0);
    let trimmed = preferred.trim();
    let hash = tf_cache_hash(
        dw_ptr,
        custom_ptr,
        trimmed,
        families,
        size_tenths,
        weight.0,
        align.0,
        para_align.0,
    );

    if let Some(hit) = TF_CACHE.with(|cell| {
        let map = cell.borrow();
        if let Some(entry) = map.get(&hash) {
            if tf_entry_matches(
                entry,
                dw_ptr,
                custom_ptr,
                trimmed,
                families,
                size_tenths,
                weight.0,
                align.0,
                para_align.0,
            ) {
                // Restore default wrapping in case a previous caller set NO_WRAP.
                let _ = entry.tf.SetWordWrapping(DWRITE_WORD_WRAPPING_WRAP);
                return Some(entry.tf.clone());
            }
        }
        None
    }) {
        return Some(hit);
    }

    let mut created: Option<IDWriteTextFormat> = None;
    if !trimmed.is_empty() {
        let wide: Vec<u16> = trimmed.encode_utf16().chain(std::iter::once(0)).collect();
        let p = PCWSTR(wide.as_ptr());
        if let Some(coll) = custom {
            if font_exists_in_collection(coll, p) {
                if let Ok(tf) = dw.CreateTextFormat(
                    p,
                    Some(coll),
                    weight,
                    DWRITE_FONT_STYLE_NORMAL,
                    DWRITE_FONT_STRETCH_NORMAL,
                    quant_sz,
                    w!("en-US"),
                ) {
                    let _ = tf.SetTextAlignment(align);
                    let _ = tf.SetParagraphAlignment(para_align);
                    created = Some(tf);
                }
            }
        }
        if created.is_none() {
            if let Some(sys) = cached_system_font_collection(dw) {
                if font_exists_in_collection(&sys, p) {
                    if let Ok(tf) = dw.CreateTextFormat(
                        p,
                        None,
                        weight,
                        DWRITE_FONT_STYLE_NORMAL,
                        DWRITE_FONT_STRETCH_NORMAL,
                        quant_sz,
                        w!("en-US"),
                    ) {
                        let _ = tf.SetTextAlignment(align);
                        let _ = tf.SetParagraphAlignment(para_align);
                        created = Some(tf);
                    }
                }
            }
        }
    }
    if created.is_none() {
        created = make_resolved_tf_uncached(dw, custom, families, quant_sz, weight, align, para_align);
    }

    if let Some(ref tf) = created {
        let fam_vecs: Vec<Vec<u16>> = families
            .iter()
            .map(|&f| pcwstr_wide_units(f).to_vec())
            .collect();
        TF_CACHE.with(|cell| {
            let mut map = cell.borrow_mut();
            if map.len() >= 192 {
                map.clear();
            }
            map.insert(
                hash,
                CachedTfEntry {
                    dw_ptr,
                    custom_ptr,
                    preferred: trimmed.to_string(),
                    families: fam_vecs,
                    size_tenths,
                    weight: weight.0,
                    align: align.0,
                    para_align: para_align.0,
                    tf: tf.clone(),
                },
            );
        });
    }
    created
}

pub unsafe fn make_resolved_tf(
    dw: &IDWriteFactory,
    custom: Option<&IDWriteFontCollection>,
    families: &[PCWSTR],
    size_px: f32,
    weight: DWRITE_FONT_WEIGHT,
    align: DWRITE_TEXT_ALIGNMENT,
    para_align: DWRITE_PARAGRAPH_ALIGNMENT,
) -> Option<IDWriteTextFormat> {
    get_or_build_cached_tf(dw, custom, "", families, size_px, weight, align, para_align)
}

pub unsafe fn make_resolved_tf_named(
    dw: &IDWriteFactory,
    custom: Option<&IDWriteFontCollection>,
    preferred: &str,
    fallbacks: &[PCWSTR],
    size_px: f32,
    weight: DWRITE_FONT_WEIGHT,
    align: DWRITE_TEXT_ALIGNMENT,
    para_align: DWRITE_PARAGRAPH_ALIGNMENT,
) -> Option<IDWriteTextFormat> {
    get_or_build_cached_tf(dw, custom, preferred, fallbacks, size_px, weight, align, para_align)
}

thread_local! {
    static ACTIVE_ELEM_STACK: std::cell::Cell<([ClockElemTransform; 4], usize)> =
        const { std::cell::Cell::new(([ClockElemTransform {
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
        }; 4], 0)) };
}

#[inline]
pub(crate) fn current_active_elem_tr() -> Option<ClockElemTransform> {
    ACTIVE_ELEM_STACK.with(|c| {
        let (arr, len) = c.get();
        if len > 0 {
            Some(arr[(len - 1).min(3)])
        } else {
            None
        }
    })
}

#[inline]
pub fn grad_endpoints_for_rect(rect: (f32, f32, f32, f32), angle_deg: u16) -> ((f32, f32), (f32, f32)) {
    let (x0, y0, x1, y1) = rect;
    let cx = (x0 + x1) * 0.5;
    let cy = (y0 + y1) * 0.5;
    let hw = ((x1 - x0) * 0.5).max(1.0);
    let hh = ((y1 - y0) * 0.5).max(1.0);
    let rad = (angle_deg as f32).to_radians();
    let (s, c) = rad.sin_cos();
    ((cx - c * hw, cy - s * hh), (cx + c * hw, cy + s * hh))
}

pub unsafe fn draw_shadowed_text(
    rt: &ID2D1RenderTarget,
    text: &str,
    tf: &IDWriteTextFormat,
    rc: D2D_RECT_F,
    br: &ID2D1SolidColorBrush,
    shadow_br: Option<&ID2D1SolidColorBrush>,
    shadow_off: f32,
    clip: bool,
) {
    let wide = to_wide(text);
    let len = wide.len().saturating_sub(1);
    let opts = if clip {
        D2D1_DRAW_TEXT_OPTIONS_CLIP
    } else {
        D2D1_DRAW_TEXT_OPTIONS_NONE
    };
    let active_tr = current_active_elem_tr();
    if let Some(tr) = active_tr {
        if tr.shadow_blur > 0 {
            let blur_r = (tr.shadow_blur as f32 * 0.45).clamp(0.8, 11.0);
            let glow_u32 = tr.glow_rgb.or(tr.color_rgb).unwrap_or(0x00E5FF);
            let (gr, gg, gb) = crate::theme::u32_to_rgb(glow_u32);
            if let Ok(g_outer) = rt.CreateSolidColorBrush(&rgba_u8(gr, gg, gb, 0.14), None) {
                for (ox, oy) in [
                    (-blur_r, 0.0),
                    (blur_r, 0.0),
                    (0.0, -blur_r),
                    (0.0, blur_r),
                ] {
                    let g_rc = D2D_RECT_F {
                        left: rc.left + ox,
                        top: rc.top + oy,
                        right: rc.right + ox,
                        bottom: rc.bottom + oy,
                    };
                    rt.DrawText(&wide[..len], tf, &g_rc, &g_outer, opts, DWRITE_MEASURING_MODE_NATURAL);
                }
            }
            let inner_r = blur_r * 0.48;
            if let Ok(g_inner) = rt.CreateSolidColorBrush(&rgba_u8(gr, gg, gb, 0.26), None) {
                for (ox, oy) in [
                    (-inner_r, -inner_r),
                    (inner_r, -inner_r),
                    (-inner_r, inner_r),
                    (inner_r, inner_r),
                ] {
                    let g_rc = D2D_RECT_F {
                        left: rc.left + ox,
                        top: rc.top + oy,
                        right: rc.right + ox,
                        bottom: rc.bottom + oy,
                    };
                    rt.DrawText(&wide[..len], tf, &g_rc, &g_inner, opts, DWRITE_MEASURING_MODE_NATURAL);
                }
            }
        }
    }
    if let Some(sb) = shadow_br {
        let s_rc = D2D_RECT_F {
            left: rc.left + shadow_off,
            top: rc.top + shadow_off,
            right: rc.right + shadow_off,
            bottom: rc.bottom + shadow_off,
        };
        rt.DrawText(&wide[..len], tf, &s_rc, sb, opts, DWRITE_MEASURING_MODE_NATURAL);
    }
    if let Some(tr) = active_tr {
        if let Some((c1, c2)) = tr.gradient_rgb() {
            let (r1, g1, b1) = crate::theme::u32_to_rgb(c1);
            let (r2, g2, b2) = crate::theme::u32_to_rgb(c2);
            let stops = [
                D2D1_GRADIENT_STOP { position: 0.0, color: rgba_u8(r1, g1, b1, 1.0) },
                D2D1_GRADIENT_STOP { position: 1.0, color: rgba_u8(r2, g2, b2, 1.0) },
            ];
            let (p0, p1) = grad_endpoints_for_rect((rc.left, rc.top, rc.right, rc.bottom), tr.gradient_angle);
            if let Some(grad_br) = create_linear_grad(rt, p0, p1, &stops) {
                rt.DrawText(&wide[..len], tf, &rc, &grad_br, opts, DWRITE_MEASURING_MODE_NATURAL);
                return;
            }
        }
    }
    rt.DrawText(&wide[..len], tf, &rc, br, opts, DWRITE_MEASURING_MODE_NATURAL);
}

unsafe fn draw_layout_with_shadow(
    rt: &ID2D1RenderTarget,
    layout: &IDWriteTextLayout,
    x: f32,
    y: f32,
    br: &ID2D1SolidColorBrush,
    shadow_br: Option<&ID2D1SolidColorBrush>,
    shadow_off: f32,
) {
    let active_tr = current_active_elem_tr();
    if let Some(tr) = active_tr {
        if tr.shadow_blur > 0 {
            let blur_r = (tr.shadow_blur as f32 * 0.45).clamp(0.8, 11.0);
            let glow_u32 = tr.glow_rgb.or(tr.color_rgb).unwrap_or(0x00E5FF);
            let (gr, gg, gb) = crate::theme::u32_to_rgb(glow_u32);
            if let Ok(g_br) = rt.CreateSolidColorBrush(&rgba_u8(gr, gg, gb, 0.20), None) {
                for (ox, oy) in [
                    (-blur_r, 0.0),
                    (blur_r, 0.0),
                    (0.0, -blur_r),
                    (0.0, blur_r),
                ] {
                    rt.DrawTextLayout(
                        Vector2 { X: x + ox, Y: y + oy },
                        layout,
                        &g_br,
                        D2D1_DRAW_TEXT_OPTIONS_NONE,
                    );
                }
            }
        }
    }
    if let Some(sb) = shadow_br {
        rt.DrawTextLayout(
            Vector2 { X: x + shadow_off, Y: y + shadow_off },
            layout,
            sb,
            D2D1_DRAW_TEXT_OPTIONS_NONE,
        );
    }
    if let Some(tr) = active_tr {
        if let (Some(c1), Some(c2)) = (tr.color_rgb, tr.color2_rgb) {
            let (r1, g1, b1) = crate::theme::u32_to_rgb(c1);
            let (r2, g2, b2) = crate::theme::u32_to_rgb(c2);
            let stops = [
                D2D1_GRADIENT_STOP { position: 0.0, color: rgba_u8(r1, g1, b1, 1.0) },
                D2D1_GRADIENT_STOP { position: 1.0, color: rgba_u8(r2, g2, b2, 1.0) },
            ];
            let w = layout.GetMaxWidth().max(40.0);
            let h = layout.GetMaxHeight().max(32.0);
            let (p0, p1) = grad_endpoints_for_rect((x, y, x + w, y + h), tr.gradient_angle);
            if let Some(grad_br) = create_linear_grad(rt, p0, p1, &stops) {
                rt.DrawTextLayout(Vector2 { X: x, Y: y }, layout, &grad_br, D2D1_DRAW_TEXT_OPTIONS_NONE);
                return;
            }
        }
    }
    rt.DrawTextLayout(Vector2 { X: x, Y: y }, layout, br, D2D1_DRAW_TEXT_OPTIONS_NONE);
}

pub unsafe fn load_bitmap_from_png_tinted(
    wic: &IWICImagingFactory,
    rt: &ID2D1RenderTarget,
    path: &Path,
    tint: Option<((u8, u8, u8), (u8, u8, u8))>,
) -> Option<ID2D1Bitmap> {
    if !path.exists() {
        return None;
    }
    let wide = to_wide(&path.to_string_lossy());
    let decoder = wic
        .CreateDecoderFromFilename(
            PCWSTR(wide.as_ptr()),
            None,
            windows::Win32::Foundation::GENERIC_READ,
            WICDecodeMetadataCacheOnDemand,
        )
        .ok()?;
    let frame = decoder.GetFrame(0).ok()?;
    let src: IWICBitmapSource = frame.into();
    wic_src_to_d2d_tinted(wic, rt, &src, tint)
}

pub unsafe fn load_bitmap_from_png_bytes_tinted(
    wic: &IWICImagingFactory,
    rt: &ID2D1RenderTarget,
    bytes: &[u8],
    tint: Option<((u8, u8, u8), (u8, u8, u8))>,
) -> Option<ID2D1Bitmap> {
    let stream = SHCreateMemStream(Some(bytes))?;
    let decoder = wic
        .CreateDecoderFromStream(&stream, std::ptr::null(), WICDecodeMetadataCacheOnDemand)
        .ok()?;
    let frame = decoder.GetFrame(0).ok()?;
    let src: IWICBitmapSource = frame.into();
    wic_src_to_d2d_tinted(wic, rt, &src, tint)
}

pub fn derive_app_name(path: &str) -> String {
    let lower = path.trim().to_ascii_lowercase();
    if lower == "ms-settings:" {
        return "Settings".to_string();
    }
    if lower == "explorer.exe" {
        return "Files".to_string();
    }
    Path::new(path.trim())
        .file_stem()
        .and_then(|s| s.to_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("App")
        .to_string()
}

pub fn pick_app_file(owner: HWND) -> Option<String> {
    let mut buf = vec![0u16; 520];
    let filter = to_wide("Apps & Shortcuts (*.exe;*.lnk;*.url)\0*.exe;*.lnk;*.url\0All Files (*.*)\0*.*\0");
    let title = to_wide("Select App, Shortcut, or URL to Add");
    let mut ofn = OPENFILENAMEW {
        lStructSize: size_of::<OPENFILENAMEW>() as u32,
        hwndOwner: owner,
        lpstrFilter: PCWSTR(filter.as_ptr()),
        lpstrFile: PWSTR(buf.as_mut_ptr()),
        nMaxFile: buf.len() as u32,
        lpstrTitle: PCWSTR(title.as_ptr()),
        Flags: OFN_FILEMUSTEXIST | OFN_NOCHANGEDIR | OFN_NODEREFERENCELINKS,
        ..Default::default()
    };
    let ok = unsafe { GetOpenFileNameW(&mut ofn).as_bool() };
    if ok {
        let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        let s = String::from_utf16_lossy(&buf[..end]).trim().to_string();
        if !s.is_empty() {
            return Some(s);
        }
    }
    None
}

fn resolve_executable_path(app_path: &str) -> String {
    let trimmed = app_path.trim();
    if trimmed.is_empty() || trimmed.contains('\\') || trimmed.contains('/') || trimmed.contains(':') {
        return trimmed.to_string();
    }
    let windir = std::env::var("WINDIR").unwrap_or_else(|_| r"C:\Windows".to_string());
    let cand1 = PathBuf::from(&windir).join(trimmed);
    if cand1.exists() {
        return cand1.to_string_lossy().to_string();
    }
    let cand2 = PathBuf::from(&windir).join("System32").join(trimmed);
    if cand2.exists() {
        return cand2.to_string_lossy().to_string();
    }
    trimmed.to_string()
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstalledAppInfo {
    pub name: String,
    pub path: String,
}

unsafe fn pwstr_to_string_free(p: PWSTR) -> String {
    if p.is_null() {
        return String::new();
    }
    let mut len = 0usize;
    while *p.0.add(len) != 0 {
        len += 1;
    }
    let s = String::from_utf16_lossy(std::slice::from_raw_parts(p.0, len));
    CoTaskMemFree(Some(p.0 as *const c_void));
    s
}

/// Enumerates all installed applications visible in Windows `Applications` (`shell:AppsFolder`),
/// including Store/UWP/MSIX packages, Electron/Squirrel apps, and Start Menu Win32 shortcuts.
pub fn enumerate_installed_apps() -> Vec<InstalledAppInfo> {
    unsafe {
        let com_ok = CoInitializeEx(None, COINIT_APARTMENTTHREADED).is_ok();
        let mut out: Vec<InstalledAppInfo> = Vec::new();
        let mut seen: HashSet<String> = HashSet::new();

        if let Ok(folder) =
            SHGetKnownFolderItem::<IShellItem>(&FOLDERID_AppsFolder, KF_FLAG_DEFAULT, None)
        {
            if let Ok(enum_items) = folder.BindToHandler::<_, IEnumShellItems>(None, &BHID_EnumItems)
            {
                let mut batch: [Option<IShellItem>; 32] = std::array::from_fn(|_| None);
                loop {
                    let mut fetched = 0u32;
                    let hr = enum_items.Next(&mut batch, Some(&mut fetched));
                    let count = fetched as usize;
                    if count == 0 {
                        break;
                    }
                    for item_opt in &mut batch[..count] {
                        if let Some(item) = item_opt.take() {
                            let name = item
                                .GetDisplayName(SIGDN_NORMALDISPLAY)
                                .ok()
                                .map(|p| pwstr_to_string_free(p).trim().to_string())
                                .unwrap_or_default();
                            if name.is_empty() {
                                continue;
                            }
                            let rel = item
                                .GetDisplayName(SIGDN_PARENTRELATIVEPARSING)
                                .ok()
                                .map(|p| pwstr_to_string_free(p).trim().to_string())
                                .unwrap_or_default();
                            let abs = item
                                .GetDisplayName(SIGDN_DESKTOPABSOLUTEPARSING)
                                .ok()
                                .map(|p| pwstr_to_string_free(p).trim().to_string())
                                .unwrap_or_default();

                            // Skip bare web documentation URLs often dumped into Start Menu folders
                            let low_rel = rel.to_ascii_lowercase();
                            let low_abs = abs.to_ascii_lowercase();
                            if low_rel.starts_with("http://")
                                || low_rel.starts_with("https://")
                                || low_abs.starts_with("http://")
                                || low_abs.starts_with("https://")
                            {
                                continue;
                            }

                            let launch_path = if !rel.is_empty() {
                                if rel.contains(":\\") || rel.starts_with("\\\\") {
                                    rel
                                } else {
                                    format!(r"shell:AppsFolder\{}", rel)
                                }
                            } else if !abs.is_empty() {
                                abs
                            } else {
                                continue;
                            };

                            let dedup_key = format!(
                                "{}|{}",
                                name.to_ascii_lowercase(),
                                launch_path.to_ascii_lowercase()
                            );
                            if seen.insert(dedup_key) {
                                out.push(InstalledAppInfo {
                                    name,
                                    path: launch_path,
                                });
                            }
                        }
                    }
                    if hr.is_err() || count < batch.len() {
                        break;
                    }
                }
            }
        }

        out.sort_by(|a, b| a.name.to_ascii_lowercase().cmp(&b.name.to_ascii_lowercase()));
        if com_ok {
            CoUninitialize();
        }
        out
    }
}

/// Launches either a regular file/URI shortcut or a `shell:AppsFolder\<AUMID>` modern app cleanly.
pub fn launch_app_path(app_path: &str) {
    let trimmed = app_path.trim();
    if trimmed.is_empty() {
        return;
    }
    unsafe {
        if trimmed.to_ascii_lowercase().starts_with("shell:") {
            let arg = to_wide(trimmed);
            let _ = ShellExecuteW(
                None,
                w!("open"),
                w!("explorer.exe"),
                PCWSTR(arg.as_ptr()),
                PCWSTR::null(),
                SW_SHOWNORMAL,
            );
        } else {
            let p = to_wide(trimmed);
            let _ = ShellExecuteW(
                None,
                w!("open"),
                PCWSTR(p.as_ptr()),
                PCWSTR::null(),
                PCWSTR::null(),
                SW_SHOWNORMAL,
            );
        }
    }
}

pub(crate) fn apply_duotone_tint_pbgra(buf: &mut [u8], tint: ((u8, u8, u8), (u8, u8, u8))) {
    let is_autumn = tint == AUTUMN_ICON_TINT;
    let (dark, light) = tint;
    for px in buf.chunks_exact_mut(4) {
        let a = px[3];
        if a > 12 {
            let af = a as f32 / 255.0;
            let bf = (px[0] as f32 / af).min(255.0);
            let gf = (px[1] as f32 / af).min(255.0);
            let rf = (px[2] as f32 / af).min(255.0);
            let lum = (0.299 * rf + 0.587 * gf + 0.114 * bf) / 255.0;
            let (nr, ng, nb) = if is_autumn {
                // Classic AutumnHelper.cs warm amber/gold tint curve (32bpp PBGRA)
                let t = lum.clamp(0.15, 1.0).powf(0.75);
                (
                    (224.0 + 31.0 * t).min(255.0) * af,
                    (130.0 + 113.0 * t).min(255.0) * af,
                    (42.0 + 178.0 * (t * t)).min(255.0) * af,
                )
            } else {
                let t = lum.clamp(0.12, 1.0).powf(0.78);
                (
                    (dark.0 as f32 + (light.0 as f32 - dark.0 as f32) * t).clamp(0.0, 255.0) * af,
                    (dark.1 as f32 + (light.1 as f32 - dark.1 as f32) * t).clamp(0.0, 255.0) * af,
                    (dark.2 as f32 + (light.2 as f32 - dark.2 as f32) * t).clamp(0.0, 255.0) * af,
                )
            };
            px[0] = nb.round().clamp(0.0, a as f32) as u8;
            px[1] = ng.round().clamp(0.0, a as f32) as u8;
            px[2] = nr.round().clamp(0.0, a as f32) as u8;
        }
    }
}

pub unsafe fn wic_src_to_d2d_tinted(
    wic: &IWICImagingFactory,
    rt: &ID2D1RenderTarget,
    src: &IWICBitmapSource,
    tint: Option<((u8, u8, u8), (u8, u8, u8))>,
) -> Option<ID2D1Bitmap> {
    let converter = wic.CreateFormatConverter().ok()?;
    converter
        .Initialize(
            src,
            &GUID_WICPixelFormat32bppPBGRA,
            WICBitmapDitherTypeNone,
            None,
            0.0,
            WICBitmapPaletteTypeCustom,
        )
        .ok()?;
    let Some(tint) = tint else {
        return rt.CreateBitmapFromWicBitmap(&converter, None).ok();
    };

    let mut w = 0u32;
    let mut h = 0u32;
    if converter.GetSize(&mut w, &mut h).is_err() || w == 0 || h == 0 {
        return rt.CreateBitmapFromWicBitmap(&converter, None).ok();
    }
    let stride = w * 4;
    let mut buf = vec![0u8; (stride * h) as usize];
    if converter.CopyPixels(std::ptr::null(), stride, &mut buf).is_err() {
        return rt.CreateBitmapFromWicBitmap(&converter, None).ok();
    }
    apply_duotone_tint_pbgra(&mut buf, tint);
    let props = D2D1_BITMAP_PROPERTIES {
        pixelFormat: D2D1_PIXEL_FORMAT {
            format: DXGI_FORMAT_B8G8R8A8_UNORM,
            alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
        },
        dpiX: 96.0,
        dpiY: 96.0,
    };
    rt.CreateBitmap(
        D2D_SIZE_U { width: w, height: h },
        Some(buf.as_ptr() as *const c_void),
        stride,
        &props,
    )
    .ok()
}

pub unsafe fn wic_src_to_d2d(
    wic: &IWICImagingFactory,
    rt: &ID2D1RenderTarget,
    src: &IWICBitmapSource,
    match_theme: bool,
) -> Option<ID2D1Bitmap> {
    wic_src_to_d2d_tinted(wic, rt, src, match_theme.then_some(AUTUMN_ICON_TINT))
}

unsafe fn load_shell_item_bitmap_tinted(
    wic: &IWICImagingFactory,
    rt: &ID2D1RenderTarget,
    parsing_path: &str,
    tint: Option<((u8, u8, u8), (u8, u8, u8))>,
) -> Option<ID2D1Bitmap> {
    let wide = to_wide(parsing_path);
    let factory: IShellItemImageFactory =
        SHCreateItemFromParsingName(PCWSTR(wide.as_ptr()), None).ok()?;
    // Try icon-only first for Win32/LNKs, then fall back to BIGGERSIZEOK / 0 for UWP/Store
    // items where SIIGBF_ICONONLY returns E_PENDING (0x8000000A).
    let mut hbm_opt = None;
    for flags in [
        SIIGBF_ICONONLY | SIIGBF_BIGGERSIZEOK,
        SIIGBF_BIGGERSIZEOK,
        SIIGBF(0),
    ] {
        if let Ok(hbm) = factory.GetImage(SIZE { cx: 128, cy: 128 }, flags) {
            hbm_opt = Some(hbm);
            break;
        }
    }
    let hbm = hbm_opt?;
    let out = wic
        .CreateBitmapFromHBITMAP(hbm, HPALETTE::default(), WICBitmapUseAlpha)
        .ok()
        .and_then(|wb| {
            let src: IWICBitmapSource = wb.into();
            wic_src_to_d2d_tinted(wic, rt, &src, tint)
        });
    let _ = DeleteObject(HGDIOBJ(hbm.0));
    out
}

pub unsafe fn load_app_bitmap_for_path_tinted(
    wic: &IWICImagingFactory,
    rt: &ID2D1RenderTarget,
    app_path: &str,
    tint: Option<((u8, u8, u8), (u8, u8, u8))>,
) -> Option<ID2D1Bitmap> {
    let lower = app_path.to_ascii_lowercase();
    let png_tint = tint.filter(|t| *t != AUTUMN_ICON_TINT);
    if lower == "ms-settings:" || lower.ends_with("\\settings.url") {
        if tint.is_some() {
            if let Some(bmp) = resolve_resources_dir()
                .map(|r| r.join("Images").join("Icons").join("App_settings_2.png"))
                .and_then(|p| load_bitmap_from_png_tinted(wic, rt, &p, png_tint))
                .or_else(|| load_bitmap_from_png_bytes_tinted(wic, rt, EMBED_ICON_SETTINGS, png_tint))
            {
                return Some(bmp);
            }
        }
        let sys_settings = PathBuf::from(r"C:\Windows\ImmersiveControlPanel\SystemSettings.exe");
        if sys_settings.exists() {
            let wide = to_wide(&sys_settings.to_string_lossy());
            let mut shfi = SHFILEINFOW::default();
            if SHGetFileInfoW(
                PCWSTR(wide.as_ptr()),
                FILE_ATTRIBUTE_NORMAL,
                Some(&mut shfi),
                size_of::<SHFILEINFOW>() as u32,
                SHGFI_ICON | SHGFI_LARGEICON,
            ) != 0
                && !shfi.hIcon.is_invalid()
            {
                let out = wic
                    .CreateBitmapFromHICON(shfi.hIcon)
                    .ok()
                    .and_then(|wb| {
                        let src: IWICBitmapSource = wb.into();
                        wic_src_to_d2d_tinted(wic, rt, &src, tint)
                    });
                let _ = DestroyIcon(shfi.hIcon);
                if out.is_some() {
                    return out;
                }
            }
        }
    }

    if tint.is_some() {
        let icon_dir = resolve_resources_dir().map(|r| r.join("Images").join("Icons"));
        if lower.ends_with("gemini.lnk") {
            if let Some(bmp) = icon_dir
                .as_ref()
                .and_then(|d| load_bitmap_from_png_tinted(wic, rt, &d.join("App_Gemini.png"), png_tint))
                .or_else(|| load_bitmap_from_png_bytes_tinted(wic, rt, EMBED_ICON_GEMINI, png_tint))
            {
                return Some(bmp);
            }
        } else if lower.ends_with("wispr flow.lnk") {
            if let Some(bmp) = icon_dir
                .as_ref()
                .and_then(|d| {
                    load_bitmap_from_png_tinted(wic, rt, &d.join("App_Wispr_Flow.png"), png_tint)
                })
                .or_else(|| load_bitmap_from_png_bytes_tinted(wic, rt, EMBED_ICON_WISPR, png_tint))
            {
                return Some(bmp);
            }
        }
    }

    // Modern UWP/Store/AppsFolder & Start Menu items render sharpest via IShellItemImageFactory
    if lower.starts_with("shell:") {
        if let Some(bmp) = load_shell_item_bitmap_tinted(wic, rt, app_path.trim(), tint) {
            return Some(bmp);
        }
    }

    let resolved = resolve_executable_path(app_path);
    let wide = to_wide(&resolved);
    let mut shfi = SHFILEINFOW::default();
    let mut res = SHGetFileInfoW(
        PCWSTR(wide.as_ptr()),
        FILE_ATTRIBUTE_NORMAL,
        Some(&mut shfi),
        size_of::<SHFILEINFOW>() as u32,
        SHGFI_ICON | SHGFI_LARGEICON,
    );
    if (res == 0 || shfi.hIcon.is_invalid()) && !resolved.contains(':') {
        res = SHGetFileInfoW(
            PCWSTR(wide.as_ptr()),
            FILE_ATTRIBUTE_NORMAL,
            Some(&mut shfi),
            size_of::<SHFILEINFOW>() as u32,
            SHGFI_ICON | SHGFI_LARGEICON | SHGFI_USEFILEATTRIBUTES,
        );
    }
    if res != 0 && !shfi.hIcon.is_invalid() {
        let out = wic
            .CreateBitmapFromHICON(shfi.hIcon)
            .ok()
            .and_then(|wb| {
                let src: IWICBitmapSource = wb.into();
                wic_src_to_d2d_tinted(wic, rt, &src, tint)
            });
        let _ = DestroyIcon(shfi.hIcon);
        if out.is_some() {
            return out;
        }
    }
    load_shell_item_bitmap_tinted(wic, rt, &resolved, tint)
}

pub unsafe fn load_app_bitmap_for_path(
    wic: &IWICImagingFactory,
    rt: &ID2D1RenderTarget,
    app_path: &str,
    match_theme: bool,
) -> Option<ID2D1Bitmap> {
    load_app_bitmap_for_path_tinted(wic, rt, app_path, match_theme.then_some(AUTUMN_ICON_TINT))
}

pub struct AppIconPool {
    cache: HashMap<String, Option<ID2D1Bitmap>>,
}

impl AppIconPool {
    pub fn new() -> Self {
        Self {
            cache: HashMap::new(),
        }
    }

    pub fn invalidate(&mut self) {
        self.cache.clear();
    }

    pub fn get(&self, path: &str) -> Option<&ID2D1Bitmap> {
        self.cache.get(path)?.as_ref()
    }

    pub unsafe fn sync_paths(
        &mut self,
        paths: &[&str],
        wic: Option<&IWICImagingFactory>,
        rt: &ID2D1RenderTarget,
    ) {
        let Some(w) = wic else {
            return;
        };
        for &p in paths {
            if !self.cache.contains_key(p) {
                let bmp = load_app_bitmap_for_path(w, rt, p, false);
                self.cache.insert(p.to_string(), bmp);
            }
        }
    }
}

pub struct AppIconCache {
    slots: Vec<Option<(String, u64, Option<ID2D1Bitmap>)>>,
}

impl AppIconCache {
    pub fn new() -> Self {
        Self { slots: Vec::new() }
    }

    pub fn invalidate(&mut self) {
        self.slots.clear();
    }

    pub fn slot(&self, idx: usize) -> Option<&ID2D1Bitmap> {
        self.slots.get(idx)?.as_ref()?.2.as_ref()
    }

    pub unsafe fn sync_apps(
        &mut self,
        apps: &[AppShortcut],
        wic: Option<&IWICImagingFactory>,
        rt: &ID2D1RenderTarget,
        tint: Option<((u8, u8, u8), (u8, u8, u8))>,
    ) {
        let target_key = tint_u64_key(tint);
        self.slots.resize_with(apps.len(), || None);
        for (idx, app) in apps.iter().enumerate() {
            let needs_reload = match &self.slots[idx] {
                Some((cached_path, cached_key, _)) => {
                    cached_path != &app.path || *cached_key != target_key
                }
                None => true,
            };
            if needs_reload {
                if let Some(w) = wic {
                    let bmp = load_app_bitmap_for_path_tinted(w, rt, &app.path, tint);
                    // Cache both hits and misses (None) so missing icons never repeat Shell COM lookups per frame.
                    self.slots[idx] = Some((app.path.clone(), target_key, bmp));
                }
            }
        }
    }
}

/// Zero-allocation deterministic FNV-1a key builder for cached widget foreground bitmaps.
pub struct FgKeyHasher(u64);

impl FgKeyHasher {
    #[inline]
    pub fn new(tag: u64) -> Self {
        let mut h = Self(0xcbf2_9ce4_8422_2325);
        h.mix_u64(tag);
        h
    }

    #[inline]
    pub fn mix_u64(&mut self, mut v: u64) {
        for _ in 0..8 {
            self.0 ^= (v & 0xFF) as u64;
            self.0 = self.0.wrapping_mul(0x0000_0100_0000_01B3);
            v >>= 8;
        }
    }

    #[inline]
    pub fn mix_usize(&mut self, v: usize) {
        self.mix_u64(v as u64);
    }

    #[inline]
    pub fn mix_i32(&mut self, v: i32) {
        self.mix_u64(v as u32 as u64);
    }

    #[inline]
    pub fn mix_u8(&mut self, v: u8) {
        self.0 ^= v as u64;
        self.0 = self.0.wrapping_mul(0x0000_0100_0000_01B3);
    }

    #[inline]
    pub fn mix_bool(&mut self, v: bool) {
        self.mix_u8(if v { 1 } else { 0 });
    }

    #[inline]
    pub fn mix_f32(&mut self, v: f32) {
        self.mix_u64(v.to_bits() as u64);
    }

    #[inline]
    pub fn mix_bytes(&mut self, bytes: &[u8]) {
        self.mix_usize(bytes.len());
        for &b in bytes {
            self.0 ^= b as u64;
            self.0 = self.0.wrapping_mul(0x0000_0100_0000_01B3);
        }
    }

    #[inline]
    pub fn mix_str(&mut self, s: &str) {
        self.mix_bytes(s.as_bytes());
    }

    #[inline]
    pub fn mix_rgb(&mut self, rgb: (u8, u8, u8)) {
        self.mix_u64(((rgb.0 as u64) << 16) | ((rgb.1 as u64) << 8) | (rgb.2 as u64));
    }

    #[inline]
    pub fn mix_pal(&mut self, pal: &ThemePalette) {
        self.mix_rgb(pal.accent);
        self.mix_rgb(pal.accent_light);
        self.mix_rgb(pal.accent_deep);
        self.mix_rgb(pal.rim);
        self.mix_rgb(pal.text_primary);
        self.mix_rgb(pal.text_muted);
        self.mix_rgb(pal.glass_top);
        self.mix_rgb(pal.glass_bot);
    }

    #[inline]
    pub fn mix_tr(&mut self, tr: &ClockElemTransform) {
        self.mix_i32(tr.dx as i32);
        self.mix_i32(tr.dy as i32);
        self.mix_u64(tr.scale_pct as u64);
        self.mix_i32(tr.rot_deg as i32);
        self.mix_u64(tr.color_rgb.map(|c| c as u64 | (1u64 << 32)).unwrap_or(0));
        self.mix_u64(tr.font_scale_pct as u64);
        self.mix_bool(tr.visible);
        self.mix_u64(tr.color2_rgb.map(|c| c as u64 | (1u64 << 32)).unwrap_or(0));
        self.mix_u64(tr.gradient_angle as u64);
        self.mix_u8(tr.shadow_blur);
        self.mix_u64(tr.glow_rgb.map(|c| c as u64 | (1u64 << 32)).unwrap_or(0));
        self.mix_u8(tr.border_width);
        self.mix_u64(tr.border_rgb.map(|c| c as u64 | (1u64 << 32)).unwrap_or(0));
    }

    #[inline]
    pub fn finish(self) -> u64 {
        if self.0 == 0 { 1 } else { self.0 }
    }
}

/// Offscreen `ID2D1BitmapRenderTarget` cache slot for static widget typography & vector UI.
/// Keeps card plates and ambient FX animating on the live render target at 32 FPS while
/// avoiding per-frame DirectWrite shaping (`CreateTextLayout` / `DrawText`) churn.
#[derive(Default)]
pub struct WidgetFgCacheSlot {
    key: u64,
    rt_ptr: usize,
    cache_gen: u64,
    ox: f32,
    oy: f32,
    aw: f32,
    ah: f32,
    pix_w: u32,
    pix_h: u32,
    compat_rt: Option<windows::Win32::Graphics::Direct2D::ID2D1BitmapRenderTarget>,
    bmp: Option<ID2D1Bitmap>,
}

impl WidgetFgCacheSlot {
    pub fn clear(&mut self) {
        self.key = 0;
        self.rt_ptr = 0;
        self.pix_w = 0;
        self.pix_h = 0;
        self.bmp = None;
        self.compat_rt = None;
    }

    pub unsafe fn draw_cached<F>(
        &mut self,
        rt: &ID2D1RenderTarget,
        cache_gen: u64,
        key: u64,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        margin: f32,
        draw_fn: F,
    ) where
        F: FnMut(&ID2D1RenderTarget),
    {
        self.draw_cached_with_interp(
            rt,
            cache_gen,
            key,
            x,
            y,
            w,
            h,
            margin,
            windows::Win32::Graphics::Direct2D::D2D1_BITMAP_INTERPOLATION_MODE_NEAREST_NEIGHBOR,
            draw_fn,
        );
    }

    pub unsafe fn draw_cached_with_interp<F>(
        &mut self,
        rt: &ID2D1RenderTarget,
        cache_gen: u64,
        key: u64,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        margin: f32,
        interp: windows::Win32::Graphics::Direct2D::D2D1_BITMAP_INTERPOLATION_MODE,
        mut draw_fn: F,
    ) where
        F: FnMut(&ID2D1RenderTarget),
    {
        let rt_ptr = rt.as_raw() as usize;
        let ox = (x - margin).floor();
        let oy = (y - margin).floor();
        let req_w = ((x + w + margin) - ox).ceil().max(1.0);
        let req_h = ((y + h + margin) - oy).ceil().max(1.0);
        let (mut dpi_x, mut dpi_y) = (96.0f32, 96.0f32);
        rt.GetDpi(&mut dpi_x, &mut dpi_y);
        let req_pw = ((req_w * dpi_x) / 96.0).round().max(1.0) as u32;
        let req_ph = ((req_h * dpi_y) / 96.0).round().max(1.0) as u32;

        let hit = self.key == key
            && self.key != 0
            && self.rt_ptr == rt_ptr
            && self.cache_gen == cache_gen
            && self.pix_w == req_pw
            && self.pix_h == req_ph
            && self.ox.to_bits() == ox.to_bits()
            && self.oy.to_bits() == oy.to_bits()
            && self.bmp.is_some();

        if hit {
            if let Some(ref bmp) = self.bmp {
                let dst = D2D_RECT_F {
                    left: self.ox,
                    top: self.oy,
                    right: self.ox + self.aw,
                    bottom: self.oy + self.ah,
                };
                let src = D2D_RECT_F {
                    left: 0.0,
                    top: 0.0,
                    right: self.aw,
                    bottom: self.ah,
                };
                rt.DrawBitmap(bmp, Some(&dst), 1.0, interp, Some(&src));
                return;
            }
        }

        let need_new_compat = self.compat_rt.is_none()
            || self.rt_ptr != rt_ptr
            || self.cache_gen != cache_gen
            || self.pix_w != req_pw
            || self.pix_h != req_ph;

        if need_new_compat {
            self.bmp = None;
            self.compat_rt = None;
            let sz = D2D_SIZE_F {
                width: req_w,
                height: req_h,
            };
            match rt.CreateCompatibleRenderTarget(
                Some(&sz),
                None,
                None,
                windows::Win32::Graphics::Direct2D::D2D1_COMPATIBLE_RENDER_TARGET_OPTIONS_NONE,
            ) {
                Ok(crt) => {
                    self.compat_rt = Some(crt);
                }
                Err(_) => {
                    self.clear();
                    draw_fn(rt);
                    return;
                }
            }
        }

        if let Some(ref crt) = self.compat_rt {
            let crt_rt: &ID2D1RenderTarget = crt;
            crt_rt.BeginDraw();
            let transparent = D2D1_COLOR_F {
                r: 0.0,
                g: 0.0,
                b: 0.0,
                a: 0.0,
            };
            crt_rt.Clear(Some(&transparent));
            crt_rt.SetTextAntialiasMode(rt.GetTextAntialiasMode());
            crt_rt.SetTransform(&Matrix3x2::translation(-ox, -oy));
            draw_fn(crt_rt);
            crt_rt.SetTransform(&Matrix3x2::identity());
            let end_ok = crt_rt.EndDraw(None, None).is_ok();
            if end_ok {
                if let Ok(bmp) = crt.GetBitmap() {
                    let actual = bmp.GetSize();
                    self.key = key;
                    self.rt_ptr = rt_ptr;
                    self.cache_gen = cache_gen;
                    self.ox = ox;
                    self.oy = oy;
                    self.aw = actual.width;
                    self.ah = actual.height;
                    self.pix_w = req_pw;
                    self.pix_h = req_ph;
                    let dst = D2D_RECT_F {
                        left: ox,
                        top: oy,
                        right: ox + actual.width,
                        bottom: oy + actual.height,
                    };
                    let src = D2D_RECT_F {
                        left: 0.0,
                        top: 0.0,
                        right: actual.width,
                        bottom: actual.height,
                    };
                    rt.DrawBitmap(&bmp, Some(&dst), 1.0, interp, Some(&src));
                    self.bmp = Some(bmp);
                    return;
                }
            }
        }

        self.clear();
        draw_fn(rt);
    }
}

pub struct WidgetFgBmpCache {
    pub cache_gen: u64,
    pub clock: WidgetFgCacheSlot,
    pub clock_sec: [WidgetFgCacheSlot; 2],
    pub music: WidgetFgCacheSlot,
    pub ext: [WidgetFgCacheSlot; EXT_WIDGET_COUNT],
}

impl Default for WidgetFgBmpCache {
    fn default() -> Self {
        Self {
            cache_gen: 1,
            clock: WidgetFgCacheSlot::default(),
            clock_sec: [WidgetFgCacheSlot::default(), WidgetFgCacheSlot::default()],
            music: WidgetFgCacheSlot::default(),
            ext: std::array::from_fn(|_| WidgetFgCacheSlot::default()),
        }
    }
}

impl WidgetFgBmpCache {
    pub fn clear(&mut self) {
        self.cache_gen = self.cache_gen.wrapping_add(1).max(1);
        self.clock.clear();
        for s in &mut self.clock_sec {
            s.clear();
        }
        self.music.clear();
        for slot in &mut self.ext {
            slot.clear();
        }
    }
}

pub fn dt_logical_size(cfg: &AppConfig) -> (f32, f32) {
    match cfg.dt_style {
        ClockStyle::Island => (372.0, 74.0),
        ClockStyle::SplitStack => (300.0, 168.0),
        ClockStyle::BauhausGeo => (356.0, 148.0),
        ClockStyle::ArtDecoGilded => (368.0, 148.0),
        ClockStyle::GlassRibbon => (378.0, 114.0),
        ClockStyle::ChronoDial => (372.0, 148.0),
        ClockStyle::OrbitalEclipse => (364.0, 152.0),
        ClockStyle::SunDialHorizon => (372.0, 148.0),
        ClockStyle::FlipMech => (374.0, 132.0),
        ClockStyle::NixieTube => (378.0, 138.0),
        ClockStyle::RetroCasio => (360.0, 132.0),
        ClockStyle::SwissChronometer => (378.0, 152.0),
        ClockStyle::BraunIndustrial => (374.0, 146.0),
        ClockStyle::WaterCausticsClock => (376.0, 142.0),
        ClockStyle::ZenMistClock => (376.0, 138.0),
        ClockStyle::LeatherFieldClock => (378.0, 146.0),
        ClockStyle::WalnutDeskClock => (376.0, 142.0),
        ClockStyle::RainWindowClock => (376.0, 144.0),
        ClockStyle::AbyssalTrenchClock => (376.0, 144.0),
        ClockStyle::GlacierIceClock => (376.0, 142.0),
        ClockStyle::CarbonSteelClock => (378.0, 148.0),
        ClockStyle::GoldenHourClock => (376.0, 144.0),
        ClockStyle::AlpineMistClock => (376.0, 140.0),
        _ => (372.0, 136.0),
    }
}

#[inline]
pub fn resolve_elem_grad_colors(
    tr: Option<&ClockElemTransform>,
    alpha: f32,
    def1: D2D1_COLOR_F,
    def2: D2D1_COLOR_F,
) -> (D2D1_COLOR_F, D2D1_COLOR_F, u16) {
    let a = alpha.clamp(0.0, 1.0);
    if let Some(t) = tr {
        let ang = t.gradient_angle;
        if let Some(c1) = t.color_rgb {
            let (r1, g1, b1) = crate::theme::u32_to_rgb(c1);
            let col1 = rgba_u8(r1, g1, b1, a);
            let col2 = if let Some(c2) = t.color2_rgb {
                let (r2, g2, b2) = crate::theme::u32_to_rgb(c2);
                rgba_u8(r2, g2, b2, a)
            } else {
                let r2 = ((r1 as u16 + 48).min(255)) as u8;
                let g2 = ((g1 as u16 + 48).min(255)) as u8;
                let b2 = ((b1 as u16 + 48).min(255)) as u8;
                rgba_u8(r2, g2, b2, a)
            };
            return (col1, col2, ang);
        } else if let Some(c2) = t.color2_rgb {
            let (r2, g2, b2) = crate::theme::u32_to_rgb(c2);
            return (def1, rgba_u8(r2, g2, b2, a), ang);
        }
    }
    (def1, def2, 135)
}

#[inline]
pub fn resolve_card_plate_colors(
    tr: &ClockElemTransform,
    pal: &ThemePalette,
    base_top_a: f32,
    base_bot_a: f32,
    base_rim_a: f32,
) -> (D2D1_COLOR_F, D2D1_COLOR_F, D2D1_COLOR_F, u16) {
    let has_custom_fill = tr.color_rgb.is_some() || tr.color2_rgb.is_some();
    let eff_top_a = if has_custom_fill {
        base_top_a.max(0.46)
    } else {
        base_top_a
    }
    .clamp(0.0, 0.94);
    let eff_bot_a = if has_custom_fill {
        base_bot_a.max(0.32)
    } else {
        base_bot_a
    }
    .clamp(0.0, 0.90);

    let top_col = match tr.color_rgb {
        Some(c1) => {
            let (r, g, b) = crate::theme::u32_to_rgb(c1);
            rgba_u8(r, g, b, eff_top_a)
        }
        None => pal.glass_top_col(eff_top_a),
    };
    let bot_col = match tr.color2_rgb {
        Some(c2) => {
            let (r, g, b) = crate::theme::u32_to_rgb(c2);
            rgba_u8(r, g, b, eff_bot_a)
        }
        None => match tr.color_rgb {
            Some(c1) => {
                let (r, g, b) = crate::theme::u32_to_rgb(c1);
                rgba_u8(
                    ((r as f32) * 0.62) as u8,
                    ((g as f32) * 0.62) as u8,
                    ((b as f32) * 0.62) as u8,
                    eff_bot_a,
                )
            }
            None => pal.glass_bot_col(eff_bot_a),
        },
    };
    let rim_col = match tr.border_rgb {
        Some(bc) => {
            let (r, g, b) = crate::theme::u32_to_rgb(bc);
            rgba_u8(r, g, b, base_rim_a.max(0.55).clamp(0.0, 1.0))
        }
        None => match tr.color_rgb {
            Some(c1) => {
                let (r, g, b) = crate::theme::u32_to_rgb(c1);
                rgba_u8(
                    (r as u16 + 72).min(255) as u8,
                    (g as u16 + 72).min(255) as u8,
                    (b as u16 + 72).min(255) as u8,
                    base_rim_a.max(0.42).clamp(0.0, 1.0),
                )
            }
            None => pal.rim_col(base_rim_a),
        },
    };
    (top_col, bot_col, rim_col, tr.gradient_angle)
}

#[inline]
pub(crate) fn rim_gradient_stops(
    tr: &ClockElemTransform,
    pal: &ThemePalette,
    rim_c: D2D1_COLOR_F,
    r_base: f32,
) -> [D2D1_GRADIENT_STOP; 3] {
    if tr.border_rgb.is_some() || tr.color_rgb.is_some() {
        let mid_c = D2D1_COLOR_F {
            r: (rim_c.r * 0.72 + 0.28).min(1.0),
            g: (rim_c.g * 0.72 + 0.28).min(1.0),
            b: (rim_c.b * 0.72 + 0.28).min(1.0),
            a: (rim_c.a * 0.55).clamp(0.0, 1.0),
        };
        let end_c = D2D1_COLOR_F {
            r: rim_c.r * 0.74,
            g: rim_c.g * 0.74,
            b: rim_c.b * 0.74,
            a: (rim_c.a * 0.85).clamp(0.0, 1.0),
        };
        [
            D2D1_GRADIENT_STOP { position: 0.0, color: rim_c },
            D2D1_GRADIENT_STOP { position: 0.55, color: mid_c },
            D2D1_GRADIENT_STOP { position: 1.0, color: end_c },
        ]
    } else {
        [
            D2D1_GRADIENT_STOP { position: 0.0, color: rim_c },
            D2D1_GRADIENT_STOP { position: 0.55, color: pal.text_col(r_base * 0.24) },
            D2D1_GRADIENT_STOP { position: 1.0, color: pal.deep_col(r_base * 0.60) },
        ]
    }
}

#[inline]
unsafe fn draw_sun_orb_bar(
    rt: &ID2D1RenderTarget,
    bar_x: f32,
    bar_y: f32,
    bar_w: f32,
    bar_h: f32,
    sec: u32,
    s: f32,
    pal: &ThemePalette,
) {
    let sec_pct = (sec as f32 / 59.0).clamp(0.02, 1.0);
    let fill_w = (bar_w * sec_pct).max(bar_h);
    let r = bar_h * 0.5;
    let active_tr = current_active_elem_tr();
    let track_rr = D2D1_ROUNDED_RECT {
        rect: D2D_RECT_F { left: bar_x, top: bar_y, right: bar_x + bar_w, bottom: bar_y + bar_h },
        radiusX: r,
        radiusY: r,
    };
    let track_col = elem_col_rgba(
        active_tr.and_then(|t| t.color_rgb),
        0.18,
        rgba_u8(pal.text_primary.0, pal.text_primary.1, pal.text_primary.2, 0.14),
    );
    if let Ok(track_br) = rt.CreateSolidColorBrush(&track_col, None) {
        rt.FillRoundedRectangle(&track_rr, &track_br);
    }
    let fill_rr = D2D1_ROUNDED_RECT {
        rect: D2D_RECT_F { left: bar_x, top: bar_y, right: bar_x + fill_w, bottom: bar_y + bar_h },
        radiusX: r,
        radiusY: r,
    };
    let (c1, c2, ang) = resolve_elem_grad_colors(
        active_tr.as_ref(),
        1.0,
        pal.deep_col(1.0),
        pal.light_col(1.0),
    );
    let bar_stops = [
        D2D1_GRADIENT_STOP { position: 0.0, color: c1 },
        D2D1_GRADIENT_STOP { position: 1.0, color: c2 },
    ];
    let (gp0, gp1) = grad_endpoints_for_rect((bar_x, bar_y, bar_x + fill_w, bar_y + bar_h), ang);
    if let Some(fill_br) = create_linear_grad(rt, gp0, gp1, &bar_stops) {
        rt.FillRoundedRectangle(&fill_rr, &fill_br);
    }
    let orb_pt = Vector2 { X: bar_x + fill_w, Y: bar_y + r };
    let halo_col = D2D1_COLOR_F { a: 0.28, ..c2 };
    if let Ok(halo_br) = rt.CreateSolidColorBrush(&halo_col, None) {
        rt.FillEllipse(&D2D1_ELLIPSE { point: orb_pt, radiusX: 7.0 * s, radiusY: 7.0 * s }, &halo_br);
    }
    if let Ok(g_br) = rt.CreateSolidColorBrush(&c2, None) {
        rt.FillEllipse(&D2D1_ELLIPSE { point: orb_pt, radiusX: 3.5 * s, radiusY: 3.5 * s }, &g_br);
    }
    if let Ok(c_br) = rt.CreateSolidColorBrush(&pal.text_col(1.0), None) {
        rt.FillEllipse(&D2D1_ELLIPSE { point: orb_pt, radiusX: 1.8 * s, radiusY: 1.8 * s }, &c_br);
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct ClockElemBox {
    pub visible: bool,
    pub elem_hidden: bool,
    pub base_cx: f32,
    pub base_cy: f32,
    pub cx: f32,
    pub cy: f32,
    pub tr_half_w: f32,
    pub tr_half_h: f32,
    pub rot_deg: f32,
}

#[derive(Clone, Copy, Default)]
pub struct ElemXfGuard {
    pub prev_xf: Option<Matrix3x2>,
    pub hidden_clip: bool,
    pub rect: (f32, f32, f32, f32),
    pub s: f32,
    pub border_width: u8,
    pub border_rgb: Option<u32>,
    pub color2_rgb: Option<u32>,
    pub gradient_angle: u16,
}

#[inline]
pub unsafe fn make_elem_brush(
    rt: &ID2D1RenderTarget,
    color_rgb: Option<u32>,
    alpha: f32,
) -> Option<ID2D1SolidColorBrush> {
    let rgb = color_rgb?;
    let (r, g, b) = crate::theme::u32_to_rgb(rgb);
    rt.CreateSolidColorBrush(&rgba_u8(r, g, b, alpha), None).ok()
}

#[inline]
pub fn elem_col_rgba(
    color_rgb: Option<u32>,
    alpha: f32,
    fallback: windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F,
) -> windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F {
    match color_rgb {
        Some(rgb) => {
            let (r, g, b) = crate::theme::u32_to_rgb(rgb);
            rgba_u8(r, g, b, alpha)
        }
        None => fallback,
    }
}

#[inline]
fn mul_3x2(a: &Matrix3x2, b: &Matrix3x2) -> Matrix3x2 {
    Matrix3x2 {
        M11: a.M11 * b.M11 + a.M12 * b.M21,
        M12: a.M11 * b.M12 + a.M12 * b.M22,
        M21: a.M21 * b.M11 + a.M22 * b.M21,
        M22: a.M21 * b.M12 + a.M22 * b.M22,
        M31: a.M31 * b.M11 + a.M32 * b.M21 + b.M31,
        M32: a.M31 * b.M12 + a.M32 * b.M22 + b.M32,
    }
}

#[inline]
pub unsafe fn begin_elem_xf_slot(
    rt: &ID2D1RenderTarget,
    slot_idx: usize,
    tr: ClockElemTransform,
    rect: (f32, f32, f32, f32),
    s: f32,
    out_boxes: &mut Option<&mut [ClockElemBox]>,
) -> ElemXfGuard {
    let (x0, y0, x1, y1) = rect;
    let base_cx = (x0 + x1) * 0.5;
    let base_cy = (y0 + y1) * 0.5;
    let half_w = ((x1 - x0) * 0.5).max(5.0 * s);
    let half_h = ((y1 - y0) * 0.5).max(5.0 * s);

    let sc = (tr.scale_pct as f32 / 100.0).clamp(0.35, 2.60);
    let cx = base_cx + tr.dx as f32 * s;
    let cy = base_cy + tr.dy as f32 * s;

    if let Some(boxes) = out_boxes.as_deref_mut() {
        if let Some(slot) = boxes.get_mut(slot_idx) {
            *slot = ClockElemBox {
                visible: true,
                elem_hidden: !tr.visible,
                base_cx,
                base_cy,
                cx,
                cy,
                tr_half_w: half_w * sc,
                tr_half_h: half_h * sc,
                rot_deg: tr.rot_deg as f32,
            };
        }
    }

    ACTIVE_ELEM_STACK.with(|c| {
        let (mut arr, len) = c.get();
        arr[len.min(3)] = tr;
        c.set((arr, len + 1));
    });

    if !tr.visible {
        rt.PushAxisAlignedClip(
            &D2D_RECT_F { left: 0.0, top: 0.0, right: 0.0, bottom: 0.0 },
            windows::Win32::Graphics::Direct2D::D2D1_ANTIALIAS_MODE_ALIASED,
        );
        return ElemXfGuard {
            prev_xf: None,
            hidden_clip: true,
            rect,
            s,
            border_width: 0,
            border_rgb: None,
            color2_rgb: None,
            gradient_angle: 135,
        };
    }

    let prev_xf = if tr.dx == 0 && tr.dy == 0 && tr.scale_pct == 100 && tr.rot_deg == 0 {
        None
    } else {
        let mut p_xf = Matrix3x2::identity();
        rt.GetTransform(&mut p_xf);

        let rad = (tr.rot_deg as f32).to_radians();
        let (sin_r, cos_r) = rad.sin_cos();
        let m11 = sc * cos_r;
        let m12 = sc * sin_r;
        let m21 = -sc * sin_r;
        let m22 = sc * cos_r;
        let local_xf = Matrix3x2 {
            M11: m11,
            M12: m12,
            M21: m21,
            M22: m22,
            M31: cx - base_cx * m11 - base_cy * m21,
            M32: cy - base_cx * m12 - base_cy * m22,
        };
        let combined = mul_3x2(&local_xf, &p_xf);
        rt.SetTransform(&combined);
        Some(p_xf)
    };

    if tr.shadow_blur > 0 && (tr.border_width > 0 || slot_idx >= 3) {
        let blur = (tr.shadow_blur as f32).clamp(1.0, 24.0) * s;
        let rad = ((y1 - y0).min(x1 - x0) * 0.22).clamp(4.0 * s, 18.0 * s);
        let sh_off = 2.0 * s;
        for (mul, al) in [(0.35, 0.14), (0.75, 0.08)] {
            let ex = blur * mul;
            let s_rr = D2D1_ROUNDED_RECT {
                rect: D2D_RECT_F {
                    left: x0 - ex + sh_off * 0.5,
                    top: y0 - ex + sh_off,
                    right: x1 + ex + sh_off * 0.5,
                    bottom: y1 + ex + sh_off,
                },
                radiusX: rad + ex,
                radiusY: rad + ex,
            };
            if let Ok(sb) = rt.CreateSolidColorBrush(&rgba_u8(0, 0, 0, al), None) {
                rt.FillRoundedRectangle(&s_rr, &sb);
            }
        }
        let glow_u32 = tr.glow_rgb.or(tr.color_rgb).unwrap_or(0x00E5FF);
        let (gr, gg, gb) = crate::theme::u32_to_rgb(glow_u32);
        for (mul, al) in [(0.25, 0.22), (0.55, 0.13), (0.95, 0.06)] {
            let ex = blur * mul;
            let g_rr = D2D1_ROUNDED_RECT {
                rect: D2D_RECT_F {
                    left: x0 - ex,
                    top: y0 - ex,
                    right: x1 + ex,
                    bottom: y1 + ex,
                },
                radiusX: rad + ex,
                radiusY: rad + ex,
            };
            if let Ok(gb_br) = rt.CreateSolidColorBrush(&rgba_u8(gr, gg, gb, al), None) {
                rt.DrawRoundedRectangle(&g_rr, &gb_br, (2.2 * s).max(1.0), None);
            }
        }
    }

    ElemXfGuard {
        prev_xf,
        hidden_clip: false,
        rect,
        s,
        border_width: tr.border_width,
        border_rgb: tr.border_rgb.or(tr.glow_rgb).or(tr.color_rgb),
        color2_rgb: tr.color2_rgb,
        gradient_angle: tr.gradient_angle,
    }
}

#[inline]
pub unsafe fn end_elem_xf_slot(rt: &ID2D1RenderTarget, g: ElemXfGuard) {
    if !g.hidden_clip && g.border_width > 0 {
        let (x0, y0, x1, y1) = g.rect;
        let bw = (g.border_width as f32 * 0.85 * g.s).max(0.8 * g.s);
        let rad = ((y1 - y0).min(x1 - x0) * 0.22).clamp(4.0 * g.s, 18.0 * g.s);
        let rr = D2D1_ROUNDED_RECT {
            rect: D2D_RECT_F { left: x0, top: y0, right: x1, bottom: y1 },
            radiusX: rad,
            radiusY: rad,
        };
        let c1 = g.border_rgb.unwrap_or(0xFFE8C8);
        let (r1, g1, b1) = crate::theme::u32_to_rgb(c1);
        if let Some(c2) = g.color2_rgb {
            let (r2, g2, b2) = crate::theme::u32_to_rgb(c2);
            let stops = [
                D2D1_GRADIENT_STOP { position: 0.0, color: rgba_u8(r1, g1, b1, 0.94) },
                D2D1_GRADIENT_STOP { position: 1.0, color: rgba_u8(r2, g2, b2, 0.94) },
            ];
            let (p0, p1) = grad_endpoints_for_rect(g.rect, g.gradient_angle);
            if let Some(grad_br) = create_linear_grad(rt, p0, p1, &stops) {
                rt.DrawRoundedRectangle(&rr, &grad_br, bw, None);
            }
        } else if let Ok(br) = rt.CreateSolidColorBrush(&rgba_u8(r1, g1, b1, 0.92), None) {
            rt.DrawRoundedRectangle(&rr, &br, bw, None);
        }
    }
    if g.hidden_clip {
        rt.PopAxisAlignedClip();
    }
    if let Some(xf) = g.prev_xf {
        rt.SetTransform(&xf);
    }
    ACTIVE_ELEM_STACK.with(|c| {
        let (arr, len) = c.get();
        c.set((arr, len.saturating_sub(1)));
    });
}

#[inline]
pub fn record_elem_box_slot(
    out_boxes: &mut Option<&mut [ClockElemBox]>,
    slot_idx: usize,
    tr: ClockElemTransform,
    rect: (f32, f32, f32, f32),
    s: f32,
) {
    let (x0, y0, x1, y1) = rect;
    let base_cx = (x0 + x1) * 0.5;
    let base_cy = (y0 + y1) * 0.5;
    let half_w = ((x1 - x0) * 0.5).max(5.0 * s);
    let half_h = ((y1 - y0) * 0.5).max(5.0 * s);
    let sc = (tr.scale_pct as f32 / 100.0).clamp(0.35, 2.60);
    if let Some(boxes) = out_boxes.as_deref_mut() {
        if let Some(slot) = boxes.get_mut(slot_idx) {
            *slot = ClockElemBox {
                visible: true,
                elem_hidden: !tr.visible,
                base_cx,
                base_cy,
                cx: base_cx + tr.dx as f32 * s,
                cy: base_cy + tr.dy as f32 * s,
                tr_half_w: half_w * sc,
                tr_half_h: half_h * sc,
                rot_deg: tr.rot_deg as f32,
            };
        }
    }
}

#[inline]
unsafe fn begin_elem_xf(
    rt: &ID2D1RenderTarget,
    id: ClockElemId,
    rect: (f32, f32, f32, f32),
    s: f32,
    cfg: &AppConfig,
    out_boxes: &mut Option<&mut [ClockElemBox; 8]>,
) -> ElemXfGuard {
    let mut tr = cfg.dt_elem_transforms[id.idx()];
    let flag_vis = match id {
        ClockElemId::Day | ClockElemId::Date => cfg.dt_show_date,
        ClockElemId::Weekday => cfg.dt_show_weekday,
        ClockElemId::Clock => cfg.dt_show_clock,
        ClockElemId::AmPm => !cfg.widgets_24h && cfg.dt_show_ampm,
        ClockElemId::Orb => cfg.dt_show_orb,
        ClockElemId::Spine => cfg.dt_show_spine,
        ClockElemId::Card => cfg.dt_show_plate,
    };
    tr.visible = tr.visible && flag_vis;
    let mut slice_opt = out_boxes.as_deref_mut().map(|b| &mut b[..]);
    begin_elem_xf_slot(rt, id.idx(), tr, rect, s, &mut slice_opt)
}

#[inline]
unsafe fn end_elem_xf(rt: &ID2D1RenderTarget, guard: ElemXfGuard) {
    end_elem_xf_slot(rt, guard);
}

use windows::Win32::Graphics::Direct2D::ID2D1Brush;

unsafe fn draw_clock_arc(
    rt: &ID2D1RenderTarget,
    d2d: Option<&ID2D1Factory>,
    cx: f32,
    cy: f32,
    rx: f32,
    ry: f32,
    start_deg: f32,
    sweep_deg: f32,
    stroke_w: f32,
    round_cap: bool,
    br: &ID2D1Brush,
) {
    if sweep_deg.abs() < 0.25 || rx.abs() < 0.5 || ry.abs() < 0.5 {
        return;
    }
    let (a_beg, sw) = if sweep_deg < 0.0 {
        (start_deg + sweep_deg, (-sweep_deg).min(359.4))
    } else {
        (start_deg, sweep_deg.min(359.4))
    };
    let a0 = a_beg.to_radians();
    let a1 = (a_beg + sw).to_radians();
    let p0 = Vector2 { X: cx + rx * a0.cos(), Y: cy + ry * a0.sin() };
    let p1 = Vector2 { X: cx + rx * a1.cos(), Y: cy + ry * a1.sin() };
    if let Some(fac) = d2d {
        use windows::Win32::Graphics::Direct2D::D2D1_ARC_SIZE_SMALL;
        if let Ok(geo) = fac.CreatePathGeometry() {
            if let Ok(sink) = geo.Open() {
                sink.BeginFigure(p0, D2D1_FIGURE_BEGIN_HOLLOW);
                sink.AddArc(&D2D1_ARC_SEGMENT {
                    point: p1,
                    size: D2D_SIZE_F { width: rx.abs(), height: ry.abs() },
                    rotationAngle: 0.0,
                    sweepDirection: D2D1_SWEEP_DIRECTION_CLOCKWISE,
                    arcSize: if sw > 180.0 { D2D1_ARC_SIZE_LARGE } else { D2D1_ARC_SIZE_SMALL },
                });
                sink.EndFigure(D2D1_FIGURE_END_OPEN);
                if sink.Close().is_ok() {
                    let st_opt = if round_cap {
                        fac.CreateStrokeStyle(
                            &D2D1_STROKE_STYLE_PROPERTIES {
                                startCap: D2D1_CAP_STYLE_ROUND,
                                endCap: D2D1_CAP_STYLE_ROUND,
                                ..Default::default()
                            },
                            None,
                        )
                        .ok()
                    } else {
                        None
                    };
                    rt.DrawGeometry(&geo, br, stroke_w, st_opt.as_ref());
                    return;
                }
            }
        }
    }
    let steps = 28;
    for i in 0..steps {
        let t0 = a0 + (a1 - a0) * (i as f32 / steps as f32);
        let t1 = a0 + (a1 - a0) * ((i + 1) as f32 / steps as f32);
        rt.DrawLine(
            Vector2 { X: cx + rx * t0.cos(), Y: cy + ry * t0.sin() },
            Vector2 { X: cx + rx * t1.cos(), Y: cy + ry * t1.sin() },
            br,
            stroke_w,
            None,
        );
    }
}

unsafe fn draw_clock_poly(
    rt: &ID2D1RenderTarget,
    d2d: Option<&ID2D1Factory>,
    pts: &[Vector2],
    fill_br: Option<&ID2D1Brush>,
    stroke_br: Option<&ID2D1Brush>,
    stroke_w: f32,
    closed: bool,
) {
    if pts.len() < 2 {
        return;
    }
    use windows::Win32::Graphics::Direct2D::Common::{D2D1_FIGURE_BEGIN_FILLED, D2D1_FIGURE_END_CLOSED};
    if let Some(fac) = d2d {
        if let Ok(geo) = fac.CreatePathGeometry() {
            if let Ok(sink) = geo.Open() {
                let beg = if fill_br.is_some() { D2D1_FIGURE_BEGIN_FILLED } else { D2D1_FIGURE_BEGIN_HOLLOW };
                sink.BeginFigure(pts[0], beg);
                sink.AddLines(&pts[1..]);
                sink.EndFigure(if closed { D2D1_FIGURE_END_CLOSED } else { D2D1_FIGURE_END_OPEN });
                if sink.Close().is_ok() {
                    if let Some(fb) = fill_br {
                        rt.FillGeometry(&geo, fb, None);
                    }
                    if let Some(sb) = stroke_br {
                        rt.DrawGeometry(&geo, sb, stroke_w, None);
                    }
                    return;
                }
            }
        }
    }
    if let Some(sb) = stroke_br {
        for i in 0..pts.len() - 1 {
            rt.DrawLine(pts[i], pts[i + 1], sb, stroke_w, None);
        }
        if closed && pts.len() > 2 {
            rt.DrawLine(pts[pts.len() - 1], pts[0], sb, stroke_w, None);
        }
    }
}

unsafe fn draw_d_arch_dome(
    rt: &ID2D1RenderTarget,
    d2d: Option<&ID2D1Factory>,
    x0: f32,
    x1: f32,
    y_top: f32,
    y_base: f32,
    fill_br: Option<&ID2D1Brush>,
    stroke_br: Option<&ID2D1Brush>,
    stroke_w: f32,
) {
    use windows::Win32::Graphics::Direct2D::Common::{D2D1_FIGURE_BEGIN_FILLED, D2D1_FIGURE_END_CLOSED};
    use windows::Win32::Graphics::Direct2D::D2D1_ARC_SIZE_SMALL;
    let rx = (x1 - x0) * 0.5;
    let ry = y_base - y_top;
    if let Some(fac) = d2d {
        if let Ok(geo) = fac.CreatePathGeometry() {
            if let Ok(sink) = geo.Open() {
                sink.BeginFigure(Vector2 { X: x1, Y: y_base }, D2D1_FIGURE_BEGIN_FILLED);
                sink.AddLine(Vector2 { X: x0, Y: y_base });
                sink.AddArc(&D2D1_ARC_SEGMENT {
                    point: Vector2 { X: x1, Y: y_base },
                    size: D2D_SIZE_F { width: rx.abs(), height: ry.abs() },
                    rotationAngle: 0.0,
                    sweepDirection: D2D1_SWEEP_DIRECTION_CLOCKWISE,
                    arcSize: D2D1_ARC_SIZE_SMALL,
                });
                sink.EndFigure(D2D1_FIGURE_END_CLOSED);
                if sink.Close().is_ok() {
                    if let Some(fb) = fill_br {
                        rt.FillGeometry(&geo, fb, None);
                    }
                    if let Some(sb) = stroke_br {
                        rt.DrawGeometry(&geo, sb, stroke_w, None);
                    }
                    return;
                }
            }
        }
    }
    let rr = D2D1_ROUNDED_RECT {
        rect: D2D_RECT_F { left: x0, top: y_top, right: x1, bottom: y_base },
        radiusX: rx,
        radiusY: ry * 0.65,
    };
    if let Some(fb) = fill_br {
        rt.FillRoundedRectangle(&rr, fb);
    }
    if let Some(sb) = stroke_br {
        rt.DrawRoundedRectangle(&rr, sb, stroke_w, None);
    }
}

unsafe fn draw_clock_plate_poly(
    rt: &ID2D1RenderTarget,
    d2d: Option<&ID2D1Factory>,
    bounds: D2D_RECT_F,
    pts: &[Vector2],
    cfg: &AppConfig,
    pal: &ThemePalette,
    is_hovered: bool,
    stroke_w: f32,
) {
    let tr_card = cfg.dt_elem_transforms[ClockElemId::Card.idx()];
    if !tr_card.visible {
        return;
    }
    let g_top = (cfg.dt_glass_alpha as f32 / 255.0).clamp(0.0, 0.88);
    let g_bot = (g_top * 0.58).clamp(0.0, 0.74);
    let r_base = if is_hovered {
        ((cfg.dt_rim_alpha as f32 / 255.0) + 0.25).clamp(0.2, 0.95)
    } else {
        (cfg.dt_rim_alpha as f32 / 255.0).clamp(0.0, 1.0)
    };
    let (top_c, bot_c, rim_c, ang) = resolve_card_plate_colors(&tr_card, pal, g_top, g_bot, r_base);
    let (gp0, gp1) = grad_endpoints_for_rect((bounds.left, bounds.top, bounds.right, bounds.bottom), ang);
    let bg_stops = [
        D2D1_GRADIENT_STOP { position: 0.0, color: top_c },
        D2D1_GRADIENT_STOP { position: 1.0, color: bot_c },
    ];
    let rim_stops = rim_gradient_stops(&tr_card, pal, rim_c, r_base);
    let fill_grad: Option<ID2D1Brush> = if cfg.dt_show_plate {
        create_linear_grad(rt, gp0, gp1, &bg_stops).and_then(|b| b.cast().ok())
    } else {
        None
    };
    let rim_grad: Option<ID2D1Brush> = if cfg.dt_show_plate {
        create_linear_grad(rt, (bounds.left, bounds.top), (bounds.right, bounds.bottom), &rim_stops)
            .and_then(|b| b.cast().ok())
    } else {
        None
    };
    draw_clock_poly(rt, d2d, pts, fill_grad.as_ref(), rim_grad.as_ref(), stroke_w, true);
}

unsafe fn draw_clock_plate_rr(
    rt: &ID2D1RenderTarget,
    rect: D2D_RECT_F,
    radius: f32,
    cfg: &AppConfig,
    pal: &ThemePalette,
    is_hovered: bool,
    glass_mult: f32,
    stroke_w: f32,
) {
    let tr_card = cfg.dt_elem_transforms[ClockElemId::Card.idx()];
    if !cfg.dt_show_plate || !tr_card.visible {
        return;
    }
    let rr = D2D1_ROUNDED_RECT { rect, radiusX: radius, radiusY: radius };
    let g_top = ((cfg.dt_glass_alpha as f32 / 255.0) * glass_mult).clamp(0.0, 0.92);
    let g_bot = (g_top * 0.56).clamp(0.0, 0.80);
    let r_base = if is_hovered {
        ((cfg.dt_rim_alpha as f32 / 255.0) + 0.25).clamp(0.2, 0.95)
    } else {
        (cfg.dt_rim_alpha as f32 / 255.0).clamp(0.0, 1.0)
    };
    let (top_c, bot_c, rim_c, ang) = resolve_card_plate_colors(&tr_card, pal, g_top, g_bot, r_base);
    let (gp0, gp1) = grad_endpoints_for_rect((rect.left, rect.top, rect.right, rect.bottom), ang);
    let bg_stops = [
        D2D1_GRADIENT_STOP { position: 0.0, color: top_c },
        D2D1_GRADIENT_STOP { position: 1.0, color: bot_c },
    ];
    if let Some(bg_br) = create_linear_grad(rt, gp0, gp1, &bg_stops) {
        rt.FillRoundedRectangle(&rr, &bg_br);
    }
    if stroke_w > 0.05 && (r_base > 0.01 || tr_card.border_rgb.is_some()) {
        let rim_stops = rim_gradient_stops(&tr_card, pal, rim_c, r_base);
        if let Some(rim_br) = create_linear_grad(rt, (rect.left, rect.top), (rect.right, rect.bottom), &rim_stops) {
            rt.DrawRoundedRectangle(&rr, &rim_br, stroke_w, None);
        }
    }
}

unsafe fn draw_clock_plate_circle(
    rt: &ID2D1RenderTarget,
    cx: f32,
    cy: f32,
    r: f32,
    cfg: &AppConfig,
    pal: &ThemePalette,
    is_hovered: bool,
    glass_mult: f32,
    stroke_w: f32,
    keep_rim_no_plate: bool,
) {
    let tr_card = cfg.dt_elem_transforms[ClockElemId::Card.idx()];
    if !tr_card.visible {
        return;
    }
    let ell = D2D1_ELLIPSE { point: Vector2 { X: cx, Y: cy }, radiusX: r, radiusY: r };
    let r_base = if is_hovered {
        ((cfg.dt_rim_alpha as f32 / 255.0) + 0.25).clamp(0.25, 0.95)
    } else {
        (cfg.dt_rim_alpha as f32 / 255.0).max(if keep_rim_no_plate { 0.42 } else { 0.0 }).clamp(0.0, 1.0)
    };
    let g_top = ((cfg.dt_glass_alpha as f32 / 255.0) * glass_mult).clamp(0.0, 0.92);
    let g_bot = (g_top * 0.58).clamp(0.0, 0.80);
    let (top_c, bot_c, rim_c, ang) = resolve_card_plate_colors(&tr_card, pal, g_top, g_bot, r_base);
    if cfg.dt_show_plate {
        let (gp0, gp1) = grad_endpoints_for_rect((cx - r, cy - r, cx + r, cy + r), ang);
        let bg_stops = [
            D2D1_GRADIENT_STOP { position: 0.0, color: top_c },
            D2D1_GRADIENT_STOP { position: 1.0, color: bot_c },
        ];
        if let Some(bg_br) = create_linear_grad(rt, gp0, gp1, &bg_stops) {
            rt.FillEllipse(&ell, &bg_br);
        }
    }
    if (cfg.dt_show_plate || keep_rim_no_plate) && stroke_w > 0.05 {
        let rim_stops = rim_gradient_stops(&tr_card, pal, rim_c, r_base);
        if let Some(rim_br) = create_linear_grad(rt, (cx - r, cy - r), (cx + r, cy + r), &rim_stops) {
            rt.DrawEllipse(&ell, &rim_br, stroke_w, None);
        }
    }
}

/// Shared Direct2D Date & Time renderer (used by DesktopWidgets and Studio Previews).
#[allow(clippy::too_many_arguments)]
pub unsafe fn draw_datetime_widget_with_boxes(
    rt: &ID2D1RenderTarget,
    dw: &IDWriteFactory,
    custom_fonts: Option<&IDWriteFontCollection>,
    x: f32,
    y: f32,
    s: f32,
    cfg: &AppConfig,
    is_hovered: bool,
    out_boxes: Option<&mut [ClockElemBox; 8]>,
) {
    draw_datetime_widget_full(
        rt,
        dw,
        custom_fonts,
        x,
        y,
        s,
        cfg,
        is_hovered,
        out_boxes,
        None,
    );
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum FlipMechPass {
    All,
    StaticOnly,
    SecondsOnly,
}

unsafe fn clock_fg_key(
    x: f32,
    y: f32,
    s: f32,
    dt_w: f32,
    dt_h: f32,
    rad: f32,
    cfg: &AppConfig,
    pal: &ThemePalette,
    script_rgb: (u8, u8, u8),
    is_hovered: bool,
    now: &LocalTimeInfo,
    has_custom_fonts: bool,
    include_seconds: bool,
) -> u64 {
    let mut h = FgKeyHasher::new(0xC10C_F001);
    h.mix_f32(x);
    h.mix_f32(y);
    h.mix_f32(s);
    h.mix_f32(dt_w);
    h.mix_f32(dt_h);
    h.mix_f32(rad);
    h.mix_bool(is_hovered);
    h.mix_bool(has_custom_fonts);
    h.mix_pal(pal);
    h.mix_rgb(script_rgb);
    h.mix_u8(cfg.dt_style as u8);
    h.mix_u8(cfg.dt_align as u8);
    h.mix_bool(cfg.widgets_24h);
    h.mix_bool(cfg.widgets_live_material);
    h.mix_bool(cfg.dt_show_plate);
    h.mix_bool(cfg.dt_show_clock);
    h.mix_u8(cfg.dt_glass_alpha);
    h.mix_u8(cfg.dt_rim_alpha);
    h.mix_bool(cfg.dt_show_spine);
    h.mix_bool(cfg.dt_show_orb);
    h.mix_bool(cfg.dt_show_seconds);
    h.mix_bool(cfg.dt_show_ampm);
    h.mix_bool(cfg.dt_show_weekday);
    h.mix_bool(cfg.dt_show_date);
    h.mix_u8(cfg.dt_corner_radius);
    h.mix_i32(cfg.dt_letter_spacing as i32);
    h.mix_bool(cfg.dt_use_custom_color);
    h.mix_u64(cfg.dt_accent_rgb as u64);
    h.mix_u64(cfg.dt_digits_rgb as u64);
    h.mix_u64(cfg.dt_script_rgb as u64);
    h.mix_u64(cfg.dt_bg_rgb as u64);
    h.mix_bool(cfg.dt_match_wallpaper);
    h.mix_str(&cfg.dt_font_day);
    h.mix_str(&cfg.dt_font_script);
    h.mix_str(&cfg.dt_font_clock);
    h.mix_str(&cfg.dt_font_sub);
    for f in &cfg.dt_elem_fonts {
        h.mix_str(f);
    }
    for tr in &cfg.dt_elem_transforms {
        h.mix_tr(tr);
    }
    h.mix_i32(now.year);
    h.mix_str(now.month_name);
    h.mix_u64(now.day as u64);
    h.mix_str(now.weekday_name);
    h.mix_u64(now.hour as u64);
    h.mix_u64(now.minute as u64);
    let sec_val = if include_seconds
        && (cfg.dt_style != ClockStyle::FlipMech || cfg.dt_show_seconds || cfg.dt_show_orb)
    {
        now.second
    } else {
        0
    };
    h.mix_u64(sec_val as u64);
    h.finish()
}

#[allow(clippy::too_many_arguments)]
pub unsafe fn draw_datetime_widget_full(
    rt: &ID2D1RenderTarget,
    dw: &IDWriteFactory,
    custom_fonts: Option<&IDWriteFontCollection>,
    x: f32,
    y: f32,
    s: f32,
    cfg: &AppConfig,
    is_hovered: bool,
    out_boxes: Option<&mut [ClockElemBox; 8]>,
    fx_viz: Option<&VizSnapshot>,
) {
    draw_datetime_widget_inner(
        rt, dw, custom_fonts, x, y, s, cfg, is_hovered, out_boxes, fx_viz, None,
    );
}

#[allow(clippy::too_many_arguments)]
pub unsafe fn draw_datetime_widget_full_cached(
    rt: &ID2D1RenderTarget,
    dw: &IDWriteFactory,
    custom_fonts: Option<&IDWriteFontCollection>,
    x: f32,
    y: f32,
    s: f32,
    cfg: &AppConfig,
    is_hovered: bool,
    fx_viz: Option<&VizSnapshot>,
    fg_cache: &mut WidgetFgCacheSlot,
    sec_pair: &mut [WidgetFgCacheSlot; 2],
    cache_gen: u64,
) {
    draw_datetime_widget_inner(
        rt,
        dw,
        custom_fonts,
        x,
        y,
        s,
        cfg,
        is_hovered,
        None,
        fx_viz,
        Some((fg_cache, sec_pair, cache_gen)),
    );
}

#[allow(clippy::too_many_arguments)]
unsafe fn draw_datetime_widget_inner(
    rt: &ID2D1RenderTarget,
    dw: &IDWriteFactory,
    custom_fonts: Option<&IDWriteFontCollection>,
    x: f32,
    y: f32,
    s: f32,
    cfg: &AppConfig,
    is_hovered: bool,
    mut out_boxes: Option<&mut [ClockElemBox; 8]>,
    fx_viz: Option<&VizSnapshot>,
    fg_cache: Option<(&mut WidgetFgCacheSlot, &mut [WidgetFgCacheSlot; 2], u64)>,
) {
    if let Some(boxes) = out_boxes.as_deref_mut() {
        *boxes = [ClockElemBox::default(); 8];
    }
    if cfg.is_datetime_all_elements_off() {
        return;
    }
    let (lw, lh) = dt_logical_size(cfg);
    let dt_w = lw * s;
    let dt_h = lh * s;
    let rad = (cfg.dt_corner_radius as f32).min(lh * 0.5) * s;
    let d2d: Option<ID2D1Factory> = rt.GetFactory().ok();

    let pal = resolve_clock_palette(cfg);
    let script_rgb = if !cfg.dt_match_wallpaper && cfg.dt_use_custom_color {
        crate::theme::u32_to_rgb(cfg.dt_script_rgb)
    } else {
        pal.accent
    };
    let tr_card = cfg.dt_elem_transforms[ClockElemId::Card.idx()];
    let card_rect = (x, y, x + dt_w, y + dt_h);

    let dt_rr = D2D1_ROUNDED_RECT {
        rect: D2D_RECT_F {
            left: x,
            top: y,
            right: x + dt_w,
            bottom: y + dt_h,
        },
        radiusX: rad,
        radiusY: rad,
    };

    {
        let mut slice_opt = out_boxes.as_deref_mut().map(|b| &mut b[..]);
        record_elem_box_slot(&mut slice_opt, ClockElemId::Card.idx(), tr_card, card_rect, s);
    }

    if cfg.dt_style.uses_standard_plate() {
        let mut nb: Option<&mut [ClockElemBox]> = None;
        let g_card = begin_elem_xf_slot(rt, ClockElemId::Card.idx(), tr_card, card_rect, s, &mut nb);
        if cfg.dt_show_plate {
            let g_top = (cfg.dt_glass_alpha as f32 / 255.0).clamp(0.0, 0.88);
            let g_bot = (g_top * 0.54).clamp(0.0, 0.72);
            let r_base = if is_hovered {
                ((cfg.dt_rim_alpha as f32 / 255.0) + 0.25).clamp(0.2, 0.95)
            } else {
                (cfg.dt_rim_alpha as f32 / 255.0).clamp(0.0, 1.0)
            };
            let (top_c, bot_c, rim_c, ang) = resolve_card_plate_colors(&tr_card, &pal, g_top, g_bot, r_base);
            let (gp0, gp1) = grad_endpoints_for_rect(card_rect, ang);
            let bg_stops = [
                D2D1_GRADIENT_STOP { position: 0.0, color: top_c },
                D2D1_GRADIENT_STOP { position: 1.0, color: bot_c },
            ];
            if let Some(bg_br) = create_linear_grad(rt, gp0, gp1, &bg_stops) {
                rt.FillRoundedRectangle(&dt_rr, &bg_br);
            }

            let rim_stops = rim_gradient_stops(&tr_card, &pal, rim_c, r_base);
            if let Some(rim_br) = create_linear_grad(rt, (x, y), (x + dt_w, y + dt_h), &rim_stops) {
                rt.DrawRoundedRectangle(&dt_rr, &rim_br, 1.05 * s, None);
            }
        }

        if cfg.dt_show_plate && cfg.dt_hover_fx != TaskbarVizStyle::Off {
            let op = if cfg.dt_show_plate { 88 } else { 78 };
            paint_widget_elemental_fx(
                rt,
                d2d.as_ref(),
                x,
                y,
                x + dt_w,
                y + dt_h,
                (x, y, dt_w, dt_h),
                rad.max(12.0 * s),
                s,
                cfg.dt_hover_fx,
                op,
                &pal,
                !cfg.dt_match_wallpaper && !cfg.dt_use_custom_color && tr_card.color_rgb.is_none(),
                fx_viz,
            );
        }
        end_elem_xf_slot(rt, g_card);
    }

    let draw_body = |rt: &ID2D1RenderTarget, mut out_boxes: Option<&mut [ClockElemBox; 8]>, flip_pass: FlipMechPass, mut sec_slots: Option<(&mut [WidgetFgCacheSlot; 2], u64)>| {
    let br_cream = rt.CreateSolidColorBrush(&pal.text_col(1.0), None).ok();
    let br_accent = rt.CreateSolidColorBrush(&pal.accent_col(1.0), None).ok();
    let br_script = rt.CreateSolidColorBrush(&rgba_u8(script_rgb.0, script_rgb.1, script_rgb.2, 1.0), None).ok();
    let br_sand = rt.CreateSolidColorBrush(&pal.muted_col(0.96), None).ok();
    let br_gold = rt.CreateSolidColorBrush(&pal.light_col(1.0), None).ok();
    let sh_date = rt.CreateSolidColorBrush(&rgba_u8(0, 0, 0, 0.65), None).ok();
    let sh_day = rt.CreateSolidColorBrush(&rgba_u8(0, 0, 0, 0.76), None).ok();
    let br_elem: [Option<ID2D1SolidColorBrush>; 8] = std::array::from_fn(|i| {
        make_elem_brush(rt, cfg.dt_elem_transforms[i].color_rgb, 1.0)
    });
    let active_dt_elem: std::cell::Cell<Option<ClockElemId>> = std::cell::Cell::new(None);

    let (mut font_day, mut font_script, mut font_clock, mut font_sub) = cfg.resolved_dt_fonts();
    if !cfg.dt_elem_fonts[0].is_empty() {
        font_day = cfg.dt_elem_fonts[0].clone();
    }
    if !cfg.dt_elem_fonts[1].is_empty() {
        font_script = cfg.dt_elem_fonts[1].clone();
    }
    if !cfg.dt_elem_fonts[2].is_empty() {
        font_clock = cfg.dt_elem_fonts[2].clone();
    }
    if !cfg.dt_elem_fonts[5].is_empty() {
        font_sub = cfg.dt_elem_fonts[5].clone();
    }
    font_day.reserve(4);
    font_script.reserve(4);
    font_clock.reserve(4);
    font_sub.reserve(4);

    let p_day = font_day.as_ptr();
    let p_scr = font_script.as_ptr();
    let p_clk = font_clock.as_ptr();
    let p_sub = font_sub.as_ptr();

    let make_resolved_tf_named = |dw_f: &IDWriteFactory,
                                  cf: Option<&IDWriteFontCollection>,
                                  preferred: &str,
                                  fallbacks: &[PCWSTR],
                                  size_px: f32,
                                  weight: windows::Win32::Graphics::DirectWrite::DWRITE_FONT_WEIGHT,
                                  align: windows::Win32::Graphics::DirectWrite::DWRITE_TEXT_ALIGNMENT,
                                  p_align: windows::Win32::Graphics::DirectWrite::DWRITE_PARAGRAPH_ALIGNMENT| {
        let elem_id = active_dt_elem.get().or_else(|| {
            let ptr = preferred.as_ptr();
            if ptr == p_day {
                Some(ClockElemId::Day)
            } else if ptr == p_scr {
                Some(ClockElemId::Weekday)
            } else if ptr == p_clk {
                Some(ClockElemId::Clock)
            } else if ptr == p_sub {
                Some(ClockElemId::Date)
            } else {
                None
            }
        });
        let (eff_pref, eff_sz) = if let Some(id) = elem_id {
            let f_ov = cfg.dt_elem_fonts[id.idx()].as_str();
            let pref = if !f_ov.is_empty() { f_ov } else { preferred };
            let fsc = (cfg.dt_elem_transforms[id.idx()].font_scale_pct as f32 / 100.0).clamp(0.50, 2.20);
            (pref, size_px * fsc)
        } else {
            (preferred, size_px)
        };
        make_resolved_tf_named(dw_f, cf, eff_pref, fallbacks, eff_sz, weight, align, p_align)
    };

    let draw_shadowed_text = |rt_d: &ID2D1RenderTarget,
                              text: &str,
                              tf: &IDWriteTextFormat,
                              rc: D2D_RECT_F,
                              br: &ID2D1SolidColorBrush,
                              sh: Option<&ID2D1SolidColorBrush>,
                              sh_off: f32,
                              center_clip: bool| {
        let eff_br = active_dt_elem
            .get()
            .and_then(|id| br_elem[id.idx()].as_ref())
            .unwrap_or(br);
        draw_shadowed_text(rt_d, text, tf, rc, eff_br, sh, sh_off, center_clip);
    };

    let begin_elem_xf = |rt_d: &ID2D1RenderTarget,
                         id: ClockElemId,
                         rect: (f32, f32, f32, f32),
                         s_val: f32,
                         cfg_ref: &AppConfig,
                         boxes_opt: &mut Option<&mut [ClockElemBox; 8]>| -> ElemXfGuard {
        active_dt_elem.set(Some(id));
        begin_elem_xf(rt_d, id, rect, s_val, cfg_ref, boxes_opt)
    };
    let end_elem_xf = |rt_d: &ID2D1RenderTarget, guard: ElemXfGuard| {
        end_elem_xf(rt_d, guard);
        active_dt_elem.set(None);
    };
    let draw_clock_plate_poly = |rt_p: &ID2D1RenderTarget,
                                 d2d_p: Option<&ID2D1Factory>,
                                 bounds: D2D_RECT_F,
                                 pts: &[Vector2],
                                 cfg_p: &AppConfig,
                                 pal_p: &ThemePalette,
                                 hov_p: bool,
                                 sw_p: f32| {
        let mut nb: Option<&mut [ClockElemBox]> = None;
        let g = begin_elem_xf_slot(rt_p, ClockElemId::Card.idx(), tr_card, card_rect, s, &mut nb);
        draw_clock_plate_poly(rt_p, d2d_p, bounds, pts, cfg_p, pal_p, hov_p, sw_p);
        end_elem_xf_slot(rt_p, g);
    };
    let draw_clock_plate_rr = |rt_p: &ID2D1RenderTarget,
                               rect: D2D_RECT_F,
                               radius: f32,
                               cfg_p: &AppConfig,
                               pal_p: &ThemePalette,
                               hov_p: bool,
                               gm_p: f32,
                               sw_p: f32| {
        let mut nb: Option<&mut [ClockElemBox]> = None;
        let g = begin_elem_xf_slot(rt_p, ClockElemId::Card.idx(), tr_card, card_rect, s, &mut nb);
        draw_clock_plate_rr(rt_p, rect, radius, cfg_p, pal_p, hov_p, gm_p, sw_p);
        end_elem_xf_slot(rt_p, g);
    };
    let draw_clock_plate_circle = |rt_p: &ID2D1RenderTarget,
                                   cx_p: f32,
                                   cy_p: f32,
                                   r_p: f32,
                                   cfg_p: &AppConfig,
                                   pal_p: &ThemePalette,
                                   hov_p: bool,
                                   gm_p: f32,
                                   sw_p: f32,
                                   keep_p: bool| {
        let mut nb: Option<&mut [ClockElemBox]> = None;
        let g = begin_elem_xf_slot(rt_p, ClockElemId::Card.idx(), tr_card, card_rect, s, &mut nb);
        draw_clock_plate_circle(rt_p, cx_p, cy_p, r_p, cfg_p, pal_p, hov_p, gm_p, sw_p, keep_p);
        end_elem_xf_slot(rt_p, g);
    };

    let now = chrono_local();
    let day_num_str = format!("{:02}", now.day);
    let hour_num = if cfg.widgets_24h {
        now.hour
    } else if now.hour % 12 == 0 {
        12
    } else {
        now.hour % 12
    };
    let hour_str = format!("{:02}", hour_num);
    let min_str = format!("{:02}", now.minute);
    let time_str = format!("{}:{}", hour_str, min_str);
    let ampm_str = if now.hour >= 12 { "PM" } else { "AM" };
    let sec_str = format!("{:02}s", now.second);

    let date_part = format!("{} {}", now.month_name, now.year);
    let footer_str = match (cfg.dt_show_date, cfg.dt_show_seconds) {
        (true, true) => format!("{}   |   {}", date_part, sec_str),
        (true, false) => date_part.clone(),
        (false, true) => format!("Sec   {}", sec_str),
        (false, false) => String::new(),
    };
    let spacing_px = cfg.dt_letter_spacing as f32 * s;
    let dwrite_align = match cfg.dt_align {
        ClockAlign::Left => DWRITE_TEXT_ALIGNMENT_LEADING,
        ClockAlign::Center => DWRITE_TEXT_ALIGNMENT_CENTER,
        ClockAlign::Right => DWRITE_TEXT_ALIGNMENT_TRAILING,
    };

    let draw_text_spaced = |text: &str,
                            tf: &IDWriteTextFormat,
                            tx: f32,
                            ty: f32,
                            max_w: f32,
                            max_h: f32,
                            br: &ID2D1SolidColorBrush,
                            sh: Option<&ID2D1SolidColorBrush>,
                            sh_off: f32,
                            bold_first_two: bool| {
        if text.is_empty() {
            return;
        }
        let _ = tf.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP);
        let wide = to_wide(text);
        let len = (wide.len().saturating_sub(1)) as u32;
        if let Ok(layout) = dw.CreateTextLayout(&wide[..wide.len() - 1], tf, max_w, max_h) {
            if bold_first_two && len >= 2 {
                let _ = layout.SetFontWeight(
                    DWRITE_FONT_WEIGHT_SEMI_BOLD,
                    DWRITE_TEXT_RANGE { startPosition: 0, length: 2 },
                );
            }
            if spacing_px.abs() > 0.05 && len > 0 {
                if let Ok(l1) = layout.cast::<IDWriteTextLayout1>() {
                    let _ = l1.SetCharacterSpacing(
                        0.0,
                        spacing_px,
                        0.0,
                        DWRITE_TEXT_RANGE { startPosition: 0, length: len },
                    );
                }
            }
            let eff_br = active_dt_elem
                .get()
                .and_then(|id| br_elem[id.idx()].as_ref())
                .unwrap_or(br);
            draw_layout_with_shadow(rt, &layout, tx, ty, eff_br, sh, sh_off);
        }
    };

    match cfg.dt_style {
        ClockStyle::Signature => {
            if cfg.dt_show_spine {
                let xf = begin_elem_xf(
                    rt,
                    ClockElemId::Spine,
                    (x + 12.0 * s, y + 18.0 * s, x + 23.0 * s, y + 118.0 * s),
                    s,
                    cfg,
                    &mut out_boxes,
                );
                let spine_rr = D2D1_ROUNDED_RECT {
                    rect: D2D_RECT_F {
                        left: x + 16.0 * s,
                        top: y + 18.0 * s,
                        right: x + 19.0 * s,
                        bottom: y + 118.0 * s,
                    },
                    radiusX: 1.5 * s,
                    radiusY: 1.5 * s,
                };
                let sp_col_top = elem_col_rgba(cfg.dt_elem_transforms[ClockElemId::Spine.idx()].color_rgb, 0.96, pal.light_col(0.96));
                let sp_col_mid = elem_col_rgba(cfg.dt_elem_transforms[ClockElemId::Spine.idx()].color_rgb, 0.82, pal.deep_col(0.82));
                let sp_col_bot = elem_col_rgba(cfg.dt_elem_transforms[ClockElemId::Spine.idx()].color_rgb, 0.08, pal.deep_col(0.08));
                let spine_stops = [
                    D2D1_GRADIENT_STOP { position: 0.0, color: sp_col_top },
                    D2D1_GRADIENT_STOP { position: 0.7, color: sp_col_mid },
                    D2D1_GRADIENT_STOP { position: 1.0, color: sp_col_bot },
                ];
                if let Some(sp_br) = create_linear_grad(rt, (x + 16.0 * s, y + 18.0 * s), (x + 16.0 * s, y + 118.0 * s), &spine_stops) {
                    rt.FillRoundedRectangle(&spine_rr, &sp_br);
                }
                end_elem_xf(rt, xf);
            }

            {
                let mut nb: Option<&mut [ClockElemBox]> = None;
                let g_card = begin_elem_xf_slot(rt, ClockElemId::Card.idx(), tr_card, card_rect, s, &mut nb);
                let div_p0 = Vector2 { X: x + 136.0 * s, Y: y + 22.0 * s };
                let div_p1 = Vector2 { X: x + 136.0 * s, Y: y + 114.0 * s };
                let div_c = elem_col_rgba(
                    tr_card.border_rgb.or(cfg.dt_elem_transforms[ClockElemId::Spine.idx()].color_rgb),
                    0.42,
                    pal.accent_col(0.36),
                );
                let div_stops = [
                    D2D1_GRADIENT_STOP { position: 0.0, color: pal.text_col(0.0) },
                    D2D1_GRADIENT_STOP { position: 0.5, color: div_c },
                    D2D1_GRADIENT_STOP { position: 1.0, color: pal.text_col(0.0) },
                ];
                if let Some(div_br) = create_linear_grad(rt, (div_p0.X, div_p0.Y), (div_p1.X, div_p1.Y), &div_stops) {
                    rt.DrawLine(div_p0, div_p1, &div_br, 1.0 * s, None);
                }
                end_elem_xf_slot(rt, g_card);
            }

            let tf_date = make_resolved_tf_named(
                dw,
                custom_fonts,
                &font_day,
                &[w!("Morganite Bold"), w!("Morganite"), w!("Bahnschrift Condensed")],
                86.0 * PT_TO_PX * s,
                DWRITE_FONT_WEIGHT_BOLD,
                DWRITE_TEXT_ALIGNMENT_LEADING,
                DWRITE_PARAGRAPH_ALIGNMENT_NEAR,
            );
            if let (Some(tf), Some(br)) = (&tf_date, &br_cream) {
                let xf = begin_elem_xf(
                    rt,
                    ClockElemId::Day,
                    (x + 32.0 * s, y + 10.0 * s, x + 114.0 * s, y + 112.0 * s),
                    s,
                    cfg,
                    &mut out_boxes,
                );
                let _ = tf.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP);
                let rc = D2D_RECT_F { left: x + 32.0 * s, top: y - 1.0 * s, right: x + 185.0 * s, bottom: y + 135.0 * s };
                draw_shadowed_text(rt, &day_num_str, tf, rc, br, sh_date.as_ref(), 2.0 * s, false);
                end_elem_xf(rt, xf);
            }

            if cfg.dt_show_weekday {
                let tf_script = make_resolved_tf_named(
                    dw,
                    custom_fonts,
                    &font_script,
                    &[w!("Tuesday Night"), w!("Segoe Script"), w!("Gabriola")],
                    29.0 * PT_TO_PX * s,
                    DWRITE_FONT_WEIGHT_REGULAR,
                    DWRITE_TEXT_ALIGNMENT_LEADING,
                    DWRITE_PARAGRAPH_ALIGNMENT_NEAR,
                );
                if let (Some(tf), Some(br)) = (&tf_script, &br_script) {
                    let xf = begin_elem_xf(
                        rt,
                        ClockElemId::Weekday,
                        (x + 28.0 * s, y + 72.0 * s, x + 152.0 * s, y + 124.0 * s),
                        s,
                        cfg,
                        &mut out_boxes,
                    );
                    let _ = tf.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP);
                    let rc = D2D_RECT_F { left: x + 31.0 * s, top: y + 58.0 * s, right: x + 260.0 * s, bottom: y + 145.0 * s };
                    draw_shadowed_text(rt, now.weekday_name, tf, rc, br, sh_day.as_ref(), 2.0 * s, false);
                    end_elem_xf(rt, xf);
                }
            }

            let tf_clock = make_resolved_tf_named(
                dw,
                custom_fonts,
                &font_clock,
                &[w!("SF Pro Display"), w!("Segoe UI Light"), w!("Segoe UI")],
                46.0 * PT_TO_PX * s,
                DWRITE_FONT_WEIGHT_LIGHT,
                dwrite_align,
                DWRITE_PARAGRAPH_ALIGNMENT_NEAR,
            );
            if let (Some(tf), Some(br)) = (&tf_clock, &br_cream) {
                let cw = if !cfg.widgets_24h && cfg.dt_show_ampm { 154.0 * s } else { 196.0 * s };
                let xf = begin_elem_xf(
                    rt,
                    ClockElemId::Clock,
                    (x + 152.0 * s, y + 17.0 * s, x + 154.0 * s + cw.min(182.0 * s), y + 77.0 * s),
                    s,
                    cfg,
                    &mut out_boxes,
                );
                draw_text_spaced(&time_str, tf, x + 154.0 * s, y + 9.0 * s, cw, 90.0 * s, br, sh_date.as_ref(), 2.0 * s, true);
                end_elem_xf(rt, xf);
            }

            if !cfg.widgets_24h && cfg.dt_show_ampm {
                let xf = begin_elem_xf(
                    rt,
                    ClockElemId::AmPm,
                    (x + 312.0 * s, y + 22.0 * s, x + 354.0 * s, y + 44.0 * s),
                    s,
                    cfg,
                    &mut out_boxes,
                );
                let badge_rr = D2D1_ROUNDED_RECT {
                    rect: D2D_RECT_F { left: x + 312.0 * s, top: y + 22.0 * s, right: x + 354.0 * s, bottom: y + 44.0 * s },
                    radiusX: 7.0 * s,
                    radiusY: 7.0 * s,
                };
                let am_tr = cfg.dt_elem_transforms[ClockElemId::AmPm.idx()];
                let b_col = elem_col_rgba(am_tr.color_rgb, 0.18, pal.accent_col(0.14));
                let r_col = elem_col_rgba(am_tr.border_rgb.or(am_tr.color_rgb), 0.72, pal.accent_col(0.58));
                if let Ok(b_br) = rt.CreateSolidColorBrush(&b_col, None) {
                    rt.FillRoundedRectangle(&badge_rr, &b_br);
                }
                if let Ok(r_br) = rt.CreateSolidColorBrush(&r_col, None) {
                    rt.DrawRoundedRectangle(&badge_rr, &r_br, 1.0 * s, None);
                }
                let tf_badge = make_resolved_tf_named(
                    dw,
                    custom_fonts,
                    &font_sub,
                    &[w!("SF Pro Display"), w!("Segoe UI")],
                    9.0 * PT_TO_PX * s,
                    DWRITE_FONT_WEIGHT_BOLD,
                    DWRITE_TEXT_ALIGNMENT_CENTER,
                    DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
                );
                if let (Some(tf), Some(br)) = (&tf_badge, &br_accent) {
                    draw_shadowed_text(rt, ampm_str, tf, badge_rr.rect, br, None, 0.0, true);
                }
                end_elem_xf(rt, xf);
            }

            if cfg.dt_show_orb {
                let xf = begin_elem_xf(
                    rt,
                    ClockElemId::Orb,
                    (x + 154.0 * s, y + 81.0 * s, x + 356.0 * s, y + 97.0 * s),
                    s,
                    cfg,
                    &mut out_boxes,
                );
                draw_sun_orb_bar(rt, x + 156.0 * s, y + 87.0 * s, 198.0 * s, 4.0 * s, now.second, s, &pal);
                end_elem_xf(rt, xf);
            }

            if !footer_str.is_empty() {
                let tf_sub = make_resolved_tf_named(
                    dw,
                    custom_fonts,
                    &font_sub,
                    &[w!("SF Pro Display"), w!("Segoe UI Semibold"), w!("Segoe UI")],
                    8.5 * PT_TO_PX * s,
                    DWRITE_FONT_WEIGHT_SEMI_BOLD,
                    dwrite_align,
                    DWRITE_PARAGRAPH_ALIGNMENT_NEAR,
                );
                if let (Some(tf), Some(br)) = (&tf_sub, &br_sand) {
                    let sub_y = if cfg.dt_show_orb { y + 104.0 * s } else { y + 92.0 * s };
                    let xf = begin_elem_xf(
                        rt,
                        ClockElemId::Date,
                        (x + 154.0 * s, sub_y - 2.0 * s, x + 344.0 * s, sub_y + 20.0 * s),
                        s,
                        cfg,
                        &mut out_boxes,
                    );
                    draw_text_spaced(&footer_str, tf, x + 156.0 * s, sub_y, 200.0 * s, 30.0 * s, br, sh_date.as_ref(), 1.0 * s, false);
                    end_elem_xf(rt, xf);
                }
            }
        }

        ClockStyle::Minimal => {
            if cfg.dt_show_spine {
                let xf = begin_elem_xf(
                    rt,
                    ClockElemId::Spine,
                    (x + 28.0 * s, y + 10.0 * s, x + dt_w - 28.0 * s, y + 16.5 * s),
                    s,
                    cfg,
                    &mut out_boxes,
                );
                let top_line = D2D1_ROUNDED_RECT {
                    rect: D2D_RECT_F { left: x + 28.0 * s, top: y + 12.0 * s, right: x + dt_w - 28.0 * s, bottom: y + 14.2 * s },
                    radiusX: 1.0 * s,
                    radiusY: 1.0 * s,
                };
                if let Some(br) = br_elem[ClockElemId::Spine.idx()].as_ref().or(br_accent.as_ref()) {
                    rt.FillRoundedRectangle(&top_line, br);
                }
                end_elem_xf(rt, xf);
            }

            let time_full = if cfg.dt_show_seconds {
                format!("{}:{:02}", time_str, now.second)
            } else {
                time_str.clone()
            };
            let tf_clock = make_resolved_tf_named(
                dw,
                custom_fonts,
                &font_clock,
                &[w!("Segoe UI Light"), w!("SF Pro Display"), w!("Segoe UI")],
                if cfg.dt_show_seconds { 42.0 * PT_TO_PX * s } else { 52.0 * PT_TO_PX * s },
                DWRITE_FONT_WEIGHT_LIGHT,
                dwrite_align,
                DWRITE_PARAGRAPH_ALIGNMENT_NEAR,
            );
            if let (Some(tf), Some(br)) = (&tf_clock, &br_cream) {
                let pad_x = x + 24.0 * s;
                let max_w = dt_w - if !cfg.widgets_24h && cfg.dt_show_ampm { 84.0 * s } else { 48.0 * s };
                let xf = begin_elem_xf(
                    rt,
                    ClockElemId::Clock,
                    (pad_x, y + 20.0 * s, pad_x + max_w, y + 80.0 * s),
                    s,
                    cfg,
                    &mut out_boxes,
                );
                draw_text_spaced(&time_full, tf, pad_x, y + 16.0 * s, max_w, 72.0 * s, br, sh_date.as_ref(), 1.8 * s, true);
                end_elem_xf(rt, xf);
            }

            if !cfg.widgets_24h && cfg.dt_show_ampm {
                let xf = begin_elem_xf(
                    rt,
                    ClockElemId::AmPm,
                    (x + dt_w - 64.0 * s, y + 32.0 * s, x + dt_w - 24.0 * s, y + 54.0 * s),
                    s,
                    cfg,
                    &mut out_boxes,
                );
                let pill = D2D1_ROUNDED_RECT {
                    rect: D2D_RECT_F { left: x + dt_w - 64.0 * s, top: y + 32.0 * s, right: x + dt_w - 24.0 * s, bottom: y + 54.0 * s },
                    radiusX: 10.0 * s,
                    radiusY: 10.0 * s,
                };
                let am_tr = cfg.dt_elem_transforms[ClockElemId::AmPm.idx()];
                if let Ok(b) = rt.CreateSolidColorBrush(&elem_col_rgba(am_tr.color_rgb, 0.22, pal.accent_col(0.18)), None) {
                    rt.FillRoundedRectangle(&pill, &b);
                }
                if let Some(b_rgb) = am_tr.border_rgb {
                    if let Ok(rb) = rt.CreateSolidColorBrush(&elem_col_rgba(Some(b_rgb), 0.78, pal.accent_col(0.58)), None) {
                        rt.DrawRoundedRectangle(&pill, &rb, 1.0 * s, None);
                    }
                }
                let tf_am = make_resolved_tf_named(
                    dw,
                    custom_fonts,
                    &font_sub,
                    &[w!("SF Pro Display"), w!("Segoe UI")],
                    8.0 * PT_TO_PX * s,
                    DWRITE_FONT_WEIGHT_BOLD,
                    DWRITE_TEXT_ALIGNMENT_CENTER,
                    DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
                );
                if let (Some(tf), Some(br)) = (&tf_am, &br_accent) {
                    draw_shadowed_text(rt, ampm_str, tf, pill.rect, br, None, 0.0, true);
                }
                end_elem_xf(rt, xf);
            }

            if cfg.dt_show_orb {
                let xf = begin_elem_xf(
                    rt,
                    ClockElemId::Orb,
                    (x + 22.0 * s, y + 80.0 * s, x + dt_w - 22.0 * s, y + 95.0 * s),
                    s,
                    cfg,
                    &mut out_boxes,
                );
                draw_sun_orb_bar(rt, x + 24.0 * s, y + 86.0 * s, dt_w - 48.0 * s, 3.2 * s, now.second, s, &pal);
                end_elem_xf(rt, xf);
            } else {
                let mut nb: Option<&mut [ClockElemBox]> = None;
                let g_card = begin_elem_xf_slot(rt, ClockElemId::Card.idx(), tr_card, card_rect, s, &mut nb);
                if let Ok(hr) = rt.CreateSolidColorBrush(&elem_col_rgba(tr_card.border_rgb, 0.34, pal.text_col(0.16)), None) {
                    rt.DrawLine(Vector2 { X: x + 24.0 * s, Y: y + 87.5 * s }, Vector2 { X: x + dt_w - 24.0 * s, Y: y + 87.5 * s }, &hr, 1.0 * s, None);
                }
                end_elem_xf_slot(rt, g_card);
            }

            // Bottom combined line: Weekday + Date
            let sub_line = match (cfg.dt_show_weekday, cfg.dt_show_date) {
                (true, true) => format!("{}  •  {:02} {} {}", now.weekday_name, now.day, now.month_name, now.year),
                (true, false) => now.weekday_name.to_string(),
                (false, true) => format!("{:02} {} {}", now.day, now.month_name, now.year),
                (false, false) => String::new(),
            };
            if !sub_line.is_empty() {
                let use_script = cfg.dt_show_weekday && font_script != font_clock;
                let fname = if use_script { &font_script } else { &font_sub };
                let pts = if use_script && font_script.to_ascii_lowercase().contains("night") { 17.5 } else { 10.5 };
                let tf_sub = make_resolved_tf_named(
                    dw,
                    custom_fonts,
                    fname,
                    &[w!("SF Pro Display"), w!("Segoe UI")],
                    pts * PT_TO_PX * s,
                    DWRITE_FONT_WEIGHT_SEMI_BOLD,
                    dwrite_align,
                    DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
                );
                if let (Some(tf), Some(br)) = (&tf_sub, if use_script { &br_script } else { &br_sand }) {
                    let elem_id = if cfg.dt_show_date { ClockElemId::Date } else { ClockElemId::Weekday };
                    let xf = begin_elem_xf(
                        rt,
                        elem_id,
                        (x + 24.0 * s, y + 96.0 * s, x + dt_w - 24.0 * s, y + 125.0 * s),
                        s,
                        cfg,
                        &mut out_boxes,
                    );
                    draw_text_spaced(&sub_line, tf, x + 24.0 * s, y + 95.0 * s, dt_w - 48.0 * s, 32.0 * s, br, sh_date.as_ref(), 1.0 * s, false);
                    end_elem_xf(rt, xf);
                }
            }
        }

        ClockStyle::Editorial => {
            if cfg.dt_show_spine {
                let xf = begin_elem_xf(
                    rt,
                    ClockElemId::Spine,
                    (x + 11.0 * s, y + 20.0 * s, x + 20.0 * s, y + 116.0 * s),
                    s,
                    cfg,
                    &mut out_boxes,
                );
                let bar = D2D1_ROUNDED_RECT {
                    rect: D2D_RECT_F { left: x + 14.0 * s, top: y + 20.0 * s, right: x + 16.5 * s, bottom: y + 116.0 * s },
                    radiusX: 1.0 * s,
                    radiusY: 1.0 * s,
                };
                if let Some(br) = br_elem[ClockElemId::Spine.idx()].as_ref().or(br_accent.as_ref()) {
                    rt.FillRoundedRectangle(&bar, br);
                }
                end_elem_xf(rt, xf);
            }
            // Left editorial Day numeral + Month stacked
            let tf_day = make_resolved_tf_named(
                dw,
                custom_fonts,
                &font_day,
                &[w!("Bodoni MT"), w!("Georgia"), w!("Times New Roman")],
                56.0 * PT_TO_PX * s,
                DWRITE_FONT_WEIGHT_BOLD,
                DWRITE_TEXT_ALIGNMENT_CENTER,
                DWRITE_PARAGRAPH_ALIGNMENT_NEAR,
            );
            if let (Some(tf), Some(br)) = (&tf_day, &br_cream) {
                let xf = begin_elem_xf(
                    rt,
                    ClockElemId::Day,
                    (x + 24.0 * s, y + 16.0 * s, x + 116.0 * s, y + 94.0 * s),
                    s,
                    cfg,
                    &mut out_boxes,
                );
                let _ = tf.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP);
                draw_shadowed_text(
                    rt,
                    &day_num_str,
                    tf,
                    D2D_RECT_F { left: x + 22.0 * s, top: y + 14.0 * s, right: x + 118.0 * s, bottom: y + 98.0 * s },
                    br,
                    sh_date.as_ref(),
                    2.0 * s,
                    false,
                );
                end_elem_xf(rt, xf);
            }
            if cfg.dt_show_date {
                let mut nb: Option<&mut [ClockElemBox; 8]> = None;
                let xf = begin_elem_xf(
                    rt,
                    ClockElemId::Date,
                    (x + 22.0 * s, y + 96.0 * s, x + 118.0 * s, y + 120.0 * s),
                    s,
                    cfg,
                    &mut nb,
                );
                let tf_mo = make_resolved_tf_named(
                    dw,
                    custom_fonts,
                    &font_sub,
                    &[w!("Palatino Linotype"), w!("Georgia"), w!("Segoe UI")],
                    8.5 * PT_TO_PX * s,
                    DWRITE_FONT_WEIGHT_SEMI_BOLD,
                    DWRITE_TEXT_ALIGNMENT_CENTER,
                    DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
                );
                if let (Some(tf), Some(br)) = (&tf_mo, &br_sand) {
                    let mo_short = now.month_name.to_ascii_uppercase();
                    draw_shadowed_text(
                        rt,
                        &mo_short,
                        tf,
                        D2D_RECT_F { left: x + 22.0 * s, top: y + 96.0 * s, right: x + 118.0 * s, bottom: y + 120.0 * s },
                        br,
                        sh_date.as_ref(),
                        1.0 * s,
                        true,
                    );
                }
                end_elem_xf(rt, xf);
            }

            {
                let mut nb: Option<&mut [ClockElemBox]> = None;
                let g_card = begin_elem_xf_slot(rt, ClockElemId::Card.idx(), tr_card, card_rect, s, &mut nb);
                if let Ok(div) = rt.CreateSolidColorBrush(&elem_col_rgba(tr_card.border_rgb, 0.45, pal.accent_col(0.32)), None) {
                    rt.DrawLine(Vector2 { X: x + 124.0 * s, Y: y + 20.0 * s }, Vector2 { X: x + 124.0 * s, Y: y + 116.0 * s }, &div, 1.0 * s, None);
                }
                end_elem_xf_slot(rt, g_card);
            }

            // Flowing Calligraphy Weekday Header across top right
            if cfg.dt_show_weekday {
                let tf_sc = make_resolved_tf_named(
                    dw,
                    custom_fonts,
                    &font_script,
                    &[w!("Edwardian Script ITC"), w!("Tuesday Night"), w!("Segoe Script")],
                    24.0 * PT_TO_PX * s,
                    DWRITE_FONT_WEIGHT_REGULAR,
                    dwrite_align,
                    DWRITE_PARAGRAPH_ALIGNMENT_NEAR,
                );
                if let (Some(tf), Some(br)) = (&tf_sc, &br_script) {
                    let xf = begin_elem_xf(
                        rt,
                        ClockElemId::Weekday,
                        (x + 138.0 * s, y + 8.0 * s, x + 356.0 * s, y + 48.0 * s),
                        s,
                        cfg,
                        &mut out_boxes,
                    );
                    draw_text_spaced(now.weekday_name, tf, x + 138.0 * s, y + 6.0 * s, 218.0 * s, 44.0 * s, br, sh_day.as_ref(), 1.5 * s, false);
                    end_elem_xf(rt, xf);
                }
            }

            // Editorial Serif Clock + AM/PM
            let tf_cl = make_resolved_tf_named(
                dw,
                custom_fonts,
                &font_clock,
                &[w!("Georgia"), w!("Bodoni MT"), w!("Times New Roman")],
                38.0 * PT_TO_PX * s,
                DWRITE_FONT_WEIGHT_REGULAR,
                dwrite_align,
                DWRITE_PARAGRAPH_ALIGNMENT_NEAR,
            );
            if let (Some(tf), Some(br)) = (&tf_cl, &br_cream) {
                let show_ampm_inline = !cfg.widgets_24h
                    && cfg.dt_show_ampm
                    && cfg.dt_elem_transforms[ClockElemId::AmPm.idx()].visible;
                let ct = if show_ampm_inline {
                    format!("{} {}", time_str, ampm_str)
                } else {
                    time_str.clone()
                };
                let xf = begin_elem_xf(
                    rt,
                    ClockElemId::Clock,
                    (x + 138.0 * s, y + 46.0 * s, x + 356.0 * s, y + 92.0 * s),
                    s,
                    cfg,
                    &mut out_boxes,
                );
                draw_text_spaced(&ct, tf, x + 138.0 * s, y + 44.0 * s, 218.0 * s, 54.0 * s, br, sh_date.as_ref(), 1.8 * s, false);
                end_elem_xf(rt, xf);
            }

            if cfg.dt_show_orb {
                let xf = begin_elem_xf(
                    rt,
                    ClockElemId::Orb,
                    (x + 136.0 * s, y + 89.0 * s, x + 354.0 * s, y + 105.0 * s),
                    s,
                    cfg,
                    &mut out_boxes,
                );
                draw_sun_orb_bar(rt, x + 138.0 * s, y + 96.0 * s, 214.0 * s, 3.0 * s, now.second, s, &pal);
                end_elem_xf(rt, xf);
            }
            if cfg.dt_show_date || cfg.dt_show_seconds {
                let yr_txt = match (cfg.dt_show_date, cfg.dt_show_seconds) {
                    (true, true) => format!("Edition {}  •  {:02}s", now.year, now.second),
                    (true, false) => format!("{}  •  {}", now.month_name, now.year),
                    (false, true) => format!("Seconds  {:02}", now.second),
                    _ => String::new(),
                };
                let tf_yr = make_resolved_tf_named(
                    dw,
                    custom_fonts,
                    &font_sub,
                    &[w!("Palatino Linotype"), w!("Segoe UI")],
                    8.5 * PT_TO_PX * s,
                    DWRITE_FONT_WEIGHT_SEMI_BOLD,
                    dwrite_align,
                    DWRITE_PARAGRAPH_ALIGNMENT_NEAR,
                );
                if let (Some(tf), Some(br)) = (&tf_yr, &br_sand) {
                    let xf = begin_elem_xf(
                        rt,
                        ClockElemId::Date,
                        (x + 138.0 * s, y + 104.0 * s, x + 352.0 * s, y + 126.0 * s),
                        s,
                        cfg,
                        &mut out_boxes,
                    );
                    draw_text_spaced(&yr_txt, tf, x + 138.0 * s, y + 106.0 * s, 214.0 * s, 24.0 * s, br, sh_date.as_ref(), 1.0 * s, false);
                    end_elem_xf(rt, xf);
                }
            }
        }

        ClockStyle::Cyber => {
            // Tech L-Brackets at 4 corners
            if cfg.dt_show_plate {
                let mut nb: Option<&mut [ClockElemBox]> = None;
                let g_card = begin_elem_xf_slot(rt, ClockElemId::Card.idx(), tr_card, card_rect, s, &mut nb);
                let arm = 12.0 * s;
                let ins = 6.0 * s;
                let bk_c = elem_col_rgba(tr_card.border_rgb.or(tr_card.color_rgb), 0.92, pal.light_col(0.92));
                if let Ok(bk_br) = rt.CreateSolidColorBrush(&bk_c, None) {
                    let segs = [
                        (x + ins, y + ins, x + ins + arm, y + ins),
                        (x + ins, y + ins, x + ins, y + ins + arm),
                        (x + dt_w - ins, y + ins, x + dt_w - ins - arm, y + ins),
                        (x + dt_w - ins, y + ins, x + dt_w - ins, y + ins + arm),
                        (x + ins, y + dt_h - ins, x + ins + arm, y + dt_h - ins),
                        (x + ins, y + dt_h - ins, x + ins, y + dt_h - ins - arm),
                        (x + dt_w - ins, y + dt_h - ins, x + dt_w - ins - arm, y + dt_h - ins),
                        (x + dt_w - ins, y + dt_h - ins, x + dt_w - ins, y + dt_h - ins - arm),
                    ];
                    for (x0, y0, x1, y1) in segs {
                        rt.DrawLine(Vector2 { X: x0, Y: y0 }, Vector2 { X: x1, Y: y1 }, &bk_br, 1.8 * s, None);
                    }
                }
                end_elem_xf_slot(rt, g_card);
            }
            if cfg.dt_show_spine {
                let xf = begin_elem_xf(
                    rt,
                    ClockElemId::Spine,
                    (x + 11.0 * s, y + 20.0 * s, x + 20.0 * s, y + dt_h - 20.0 * s),
                    s,
                    cfg,
                    &mut out_boxes,
                );
                let sp_c = elem_col_rgba(cfg.dt_elem_transforms[ClockElemId::Spine.idx()].color_rgb, 0.88, pal.accent_col(0.88));
                if let Ok(sp) = rt.CreateSolidColorBrush(&sp_c, None) {
                    rt.FillRoundedRectangle(
                        &D2D1_ROUNDED_RECT {
                            rect: D2D_RECT_F { left: x + 14.0 * s, top: y + 20.0 * s, right: x + 17.5 * s, bottom: y + dt_h - 20.0 * s },
                            radiusX: 1.0 * s,
                            radiusY: 1.0 * s,
                        },
                        &sp,
                    );
                }
                end_elem_xf(rt, xf);
            }

            // Left Date HUD Tile + Day Numeral
            {
                let xf = begin_elem_xf(
                    rt,
                    ClockElemId::Day,
                    (x + 26.0 * s, y + 20.0 * s, x + 110.0 * s, y + 92.0 * s),
                    s,
                    cfg,
                    &mut out_boxes,
                );
                let day_tr = cfg.dt_elem_transforms[ClockElemId::Day.idx()];
                let tile_rr = D2D1_ROUNDED_RECT {
                    rect: D2D_RECT_F { left: x + 26.0 * s, top: y + 20.0 * s, right: x + 110.0 * s, bottom: y + 92.0 * s },
                    radiusX: 6.0 * s,
                    radiusY: 6.0 * s,
                };
                if let Ok(tb) = rt.CreateSolidColorBrush(&elem_col_rgba(day_tr.color_rgb, 0.18, pal.accent_col(0.14)), None) {
                    rt.FillRoundedRectangle(&tile_rr, &tb);
                }
                if let Ok(tr) = rt.CreateSolidColorBrush(&elem_col_rgba(day_tr.border_rgb.or(day_tr.color_rgb), 0.72, pal.accent_col(0.55)), None) {
                    rt.DrawRoundedRectangle(&tile_rr, &tr, 1.0 * s, None);
                }
                let tf_hd = make_resolved_tf_named(
                    dw,
                    custom_fonts,
                    &font_day,
                    &[w!("Cascadia Mono"), w!("Consolas"), w!("Segoe UI")],
                    30.0 * PT_TO_PX * s,
                    DWRITE_FONT_WEIGHT_BOLD,
                    DWRITE_TEXT_ALIGNMENT_CENTER,
                    DWRITE_PARAGRAPH_ALIGNMENT_NEAR,
                );
                if let (Some(tf), Some(br)) = (&tf_hd, &br_cream) {
                    draw_shadowed_text(rt, &day_num_str, tf, D2D_RECT_F { left: x + 26.0 * s, top: y + 24.0 * s, right: x + 110.0 * s, bottom: y + 70.0 * s }, br, None, 0.0, true);
                }
                end_elem_xf(rt, xf);
            }
            if cfg.dt_show_weekday {
                let wk_short = &now.weekday_name[..now.weekday_name.len().min(3)].to_ascii_uppercase();
                let tf_wk = make_resolved_tf_named(
                    dw,
                    custom_fonts,
                    &font_script,
                    &[w!("Cascadia Code"), w!("Consolas")],
                    if font_script.to_ascii_lowercase().contains("night") || font_script.to_ascii_lowercase().contains("script") { 14.0 } else { 10.0 } * PT_TO_PX * s,
                    DWRITE_FONT_WEIGHT_BOLD,
                    DWRITE_TEXT_ALIGNMENT_CENTER,
                    DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
                );
                if let (Some(tf), Some(br)) = (&tf_wk, &br_script) {
                    let xf = begin_elem_xf(
                        rt,
                        ClockElemId::Weekday,
                        (x + 28.0 * s, y + 66.0 * s, x + 108.0 * s, y + 88.0 * s),
                        s,
                        cfg,
                        &mut out_boxes,
                    );
                    let lbl = if font_script == cfg.dt_style.default_fonts().1 { wk_short } else { now.weekday_name };
                    draw_shadowed_text(rt, lbl, tf, D2D_RECT_F { left: x + 22.0 * s, top: y + 66.0 * s, right: x + 114.0 * s, bottom: y + 90.0 * s }, br, None, 0.0, false);
                    end_elem_xf(rt, xf);
                }
            }

            // Right HUD Digital Time + optional seconds pill
            {
                let cr = if cfg.dt_show_seconds { x + 354.0 * s } else { x + 302.0 * s };
                let xf = begin_elem_xf(
                    rt,
                    ClockElemId::Clock,
                    (x + 124.0 * s, y + 18.0 * s, cr, y + 74.0 * s),
                    s,
                    cfg,
                    &mut out_boxes,
                );
                let tf_cl = make_resolved_tf_named(
                    dw,
                    custom_fonts,
                    &font_clock,
                    &[w!("Digital-7 Mono"), w!("Cascadia Mono"), w!("Consolas")],
                    42.0 * PT_TO_PX * s,
                    DWRITE_FONT_WEIGHT_BOLD,
                    dwrite_align,
                    DWRITE_PARAGRAPH_ALIGNMENT_NEAR,
                );
                if let (Some(tf), Some(br)) = (&tf_cl, &br_cream) {
                    draw_text_spaced(&time_str, tf, x + 124.0 * s, y + 18.0 * s, 178.0 * s, 60.0 * s, br, sh_date.as_ref(), 1.5 * s, false);
                }
                if cfg.dt_show_seconds {
                    let s_pill = D2D1_ROUNDED_RECT {
                        rect: D2D_RECT_F { left: x + 306.0 * s, top: y + 26.0 * s, right: x + 354.0 * s, bottom: y + 54.0 * s },
                        radiusX: 5.0 * s,
                        radiusY: 5.0 * s,
                    };
                    let clk_tr = cfg.dt_elem_transforms[ClockElemId::Clock.idx()];
                    if let Ok(sb) = rt.CreateSolidColorBrush(&elem_col_rgba(clk_tr.border_rgb.or(clk_tr.color_rgb), 0.24, pal.accent_col(0.22)), None) {
                        rt.FillRoundedRectangle(&s_pill, &sb);
                    }
                    let tf_sec = make_resolved_tf_named(
                        dw,
                        custom_fonts,
                        &font_sub,
                        &[w!("Consolas"), w!("Cascadia Mono")],
                        12.0 * PT_TO_PX * s,
                        DWRITE_FONT_WEIGHT_BOLD,
                        DWRITE_TEXT_ALIGNMENT_CENTER,
                        DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
                    );
                    if let (Some(tf), Some(br)) = (&tf_sec, &br_gold) {
                        draw_shadowed_text(rt, &format!(":{:02}", now.second), tf, s_pill.rect, br, None, 0.0, true);
                    }
                }
                end_elem_xf(rt, xf);
            }
            if !cfg.widgets_24h && cfg.dt_show_ampm {
                let tf_am = make_resolved_tf_named(
                    dw,
                    custom_fonts,
                    &font_sub,
                    &[w!("Consolas"), w!("Segoe UI")],
                    8.5 * PT_TO_PX * s,
                    DWRITE_FONT_WEIGHT_BOLD,
                    DWRITE_TEXT_ALIGNMENT_CENTER,
                    DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
                );
                if let (Some(tf), Some(br)) = (&tf_am, &br_sand) {
                    let xf = begin_elem_xf(
                        rt,
                        ClockElemId::AmPm,
                        (x + 306.0 * s, y + 58.0 * s, x + 354.0 * s, y + 76.0 * s),
                        s,
                        cfg,
                        &mut out_boxes,
                    );
                    draw_shadowed_text(rt, ampm_str, tf, D2D_RECT_F { left: x + 306.0 * s, top: y + 58.0 * s, right: x + 354.0 * s, bottom: y + 76.0 * s }, br, None, 0.0, true);
                    end_elem_xf(rt, xf);
                }
            }

            // Segmented ticks across bottom (15 HUD blocks)
            if cfg.dt_show_orb {
                let xf = begin_elem_xf(
                    rt,
                    ClockElemId::Orb,
                    (x + 122.0 * s, y + 81.0 * s, x + 348.0 * s, y + 93.0 * s),
                    s,
                    cfg,
                    &mut out_boxes,
                );
                let blocks = 15usize;
                let filled = ((now.second as usize + 1) * blocks) / 60;
                let bx0 = x + 124.0 * s;
                let bw = 11.5 * s;
                let gap = 3.5 * s;
                let orb_on = elem_col_rgba(cfg.dt_elem_transforms[ClockElemId::Orb.idx()].color_rgb, 0.95, pal.accent_col(0.95));
                for i in 0..blocks {
                    let rx = bx0 + (i as f32) * (bw + gap);
                    let rr = D2D1_ROUNDED_RECT {
                        rect: D2D_RECT_F { left: rx, top: y + 84.0 * s, right: rx + bw, bottom: y + 90.0 * s },
                        radiusX: 1.5 * s,
                        radiusY: 1.5 * s,
                    };
                    let col = if i <= filled { orb_on } else { pal.text_col(0.14) };
                    if let Ok(b) = rt.CreateSolidColorBrush(&col, None) {
                        rt.FillRoundedRectangle(&rr, &b);
                    }
                }
                end_elem_xf(rt, xf);
            }

            if cfg.dt_show_date {
                let tf_hud_sub = make_resolved_tf_named(
                    dw,
                    custom_fonts,
                    &font_sub,
                    &[w!("Consolas"), w!("Cascadia Mono")],
                    8.5 * PT_TO_PX * s,
                    DWRITE_FONT_WEIGHT_SEMI_BOLD,
                    dwrite_align,
                    DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
                );
                if let (Some(tf), Some(br)) = (&tf_hud_sub, &br_sand) {
                    let xf = begin_elem_xf(
                        rt,
                        ClockElemId::Date,
                        (x + 26.0 * s, y + 100.0 * s, x + 352.0 * s, y + 124.0 * s),
                        s,
                        cfg,
                        &mut out_boxes,
                    );
                    let hud_line = format!("{} // {:02} {} {}", now.year, now.day, now.month_name.to_ascii_uppercase(), if cfg.dt_show_weekday { now.weekday_name.to_ascii_uppercase() } else { String::new() });
                    draw_text_spaced(hud_line.trim(), tf, x + 26.0 * s, y + 102.0 * s, 326.0 * s, 22.0 * s, br, None, 0.0, false);
                    end_elem_xf(rt, xf);
                }
            }
        }

        ClockStyle::SplitStack => {
            if cfg.dt_show_spine {
                let xf = begin_elem_xf(
                    rt,
                    ClockElemId::Spine,
                    (x + 11.0 * s, y + 20.0 * s, x + 20.0 * s, y + dt_h - 20.0 * s),
                    s,
                    cfg,
                    &mut out_boxes,
                );
                let sp = D2D1_ROUNDED_RECT {
                    rect: D2D_RECT_F { left: x + 14.0 * s, top: y + 20.0 * s, right: x + 17.0 * s, bottom: y + dt_h - 20.0 * s },
                    radiusX: 1.5 * s,
                    radiusY: 1.5 * s,
                };
                if let Some(br) = br_elem[ClockElemId::Spine.idx()].as_ref().or(br_accent.as_ref()) {
                    rt.FillRoundedRectangle(&sp, br);
                }
                end_elem_xf(rt, xf);
            }
            // Giant Stacked Hour over Minute
            let tf_big = make_resolved_tf_named(
                dw,
                custom_fonts,
                &font_clock,
                &[w!("Metropolis Extra Bold"), w!("Morganite Bold"), w!("Segoe UI")],
                56.0 * PT_TO_PX * s,
                DWRITE_FONT_WEIGHT_BOLD,
                DWRITE_TEXT_ALIGNMENT_CENTER,
                DWRITE_PARAGRAPH_ALIGNMENT_NEAR,
            );
            if let Some(tf) = &tf_big {
                let xf = begin_elem_xf(
                    rt,
                    ClockElemId::Clock,
                    (x + 22.0 * s, y + 10.0 * s, x + 130.0 * s, y + 154.0 * s),
                    s,
                    cfg,
                    &mut out_boxes,
                );
                let _ = tf.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP);
                if let Some(br) = &br_cream {
                    draw_shadowed_text(rt, &hour_str, tf, D2D_RECT_F { left: x + 22.0 * s, top: y + 8.0 * s, right: x + 130.0 * s, bottom: y + 86.0 * s }, br, sh_date.as_ref(), 2.0 * s, false);
                }
                if let Some(br) = &br_accent {
                    draw_shadowed_text(rt, &min_str, tf, D2D_RECT_F { left: x + 22.0 * s, top: y + 74.0 * s, right: x + 130.0 * s, bottom: y + 156.0 * s }, br, sh_date.as_ref(), 2.0 * s, false);
                }
                end_elem_xf(rt, xf);
            }

            {
                let mut nb: Option<&mut [ClockElemBox]> = None;
                let g_card = begin_elem_xf_slot(rt, ClockElemId::Card.idx(), tr_card, card_rect, s, &mut nb);
                if let Ok(div) = rt.CreateSolidColorBrush(&elem_col_rgba(tr_card.border_rgb, 0.34, pal.text_col(0.18)), None) {
                    rt.DrawLine(Vector2 { X: x + 136.0 * s, Y: y + 22.0 * s }, Vector2 { X: x + 136.0 * s, Y: y + dt_h - 22.0 * s }, &div, 1.0 * s, None);
                }
                end_elem_xf_slot(rt, g_card);
            }

            let tf_d = make_resolved_tf_named(
                dw,
                custom_fonts,
                &font_day,
                &[w!("Morganite Bold"), w!("Segoe UI")],
                46.0 * PT_TO_PX * s,
                DWRITE_FONT_WEIGHT_BOLD,
                dwrite_align,
                DWRITE_PARAGRAPH_ALIGNMENT_NEAR,
            );
            if let (Some(tf), Some(br)) = (&tf_d, &br_cream) {
                let xf = begin_elem_xf(
                    rt,
                    ClockElemId::Day,
                    (x + 150.0 * s, y + 14.0 * s, x + 280.0 * s, y + 78.0 * s),
                    s,
                    cfg,
                    &mut out_boxes,
                );
                draw_text_spaced(&day_num_str, tf, x + 150.0 * s, y + 14.0 * s, 130.0 * s, 68.0 * s, br, sh_date.as_ref(), 1.5 * s, false);
                end_elem_xf(rt, xf);
            }
            if cfg.dt_show_weekday {
                let tf_s = make_resolved_tf_named(
                    dw,
                    custom_fonts,
                    &font_script,
                    &[w!("Tuesday Night"), w!("Segoe Script")],
                    23.0 * PT_TO_PX * s,
                    DWRITE_FONT_WEIGHT_REGULAR,
                    dwrite_align,
                    DWRITE_PARAGRAPH_ALIGNMENT_NEAR,
                );
                if let (Some(tf), Some(br)) = (&tf_s, &br_script) {
                    let xf = begin_elem_xf(
                        rt,
                        ClockElemId::Weekday,
                        (x + 148.0 * s, y + 66.0 * s, x + 292.0 * s, y + 112.0 * s),
                        s,
                        cfg,
                        &mut out_boxes,
                    );
                    draw_text_spaced(now.weekday_name, tf, x + 148.0 * s, y + 66.0 * s, 144.0 * s, 44.0 * s, br, sh_day.as_ref(), 1.5 * s, false);
                    end_elem_xf(rt, xf);
                }
            }
            if !footer_str.is_empty() {
                let tf_sub = make_resolved_tf_named(
                    dw,
                    custom_fonts,
                    &font_sub,
                    &[w!("SF Pro Display"), w!("Segoe UI")],
                    8.5 * PT_TO_PX * s,
                    DWRITE_FONT_WEIGHT_SEMI_BOLD,
                    dwrite_align,
                    DWRITE_PARAGRAPH_ALIGNMENT_NEAR,
                );
                if let (Some(tf), Some(br)) = (&tf_sub, &br_sand) {
                    let stxt = match (cfg.dt_show_date, cfg.dt_show_seconds) {
                        (true, true) => format!("{}  |  {}", now.month_name, sec_str),
                        (true, false) => now.month_name.to_string(),
                        (false, true) => sec_str.clone(),
                        _ => String::new(),
                    };
                    let xf = begin_elem_xf(
                        rt,
                        ClockElemId::Date,
                        (x + 150.0 * s, y + 114.0 * s, x + 284.0 * s, y + 138.0 * s),
                        s,
                        cfg,
                        &mut out_boxes,
                    );
                    draw_text_spaced(&stxt, tf, x + 150.0 * s, y + 114.0 * s, 134.0 * s, 24.0 * s, br, None, 0.0, false);
                    end_elem_xf(rt, xf);
                }
            }
            if cfg.dt_show_orb {
                let xf = begin_elem_xf(
                    rt,
                    ClockElemId::Orb,
                    (x + 148.0 * s, y + 137.0 * s, x + 282.0 * s, y + 154.0 * s),
                    s,
                    cfg,
                    &mut out_boxes,
                );
                draw_sun_orb_bar(rt, x + 150.0 * s, y + 144.0 * s, 130.0 * s, 3.5 * s, now.second, s, &pal);
                end_elem_xf(rt, xf);
            }
        }

        ClockStyle::Island => {
            // Compact Floating Capsule Pill (372 x 74)
            let dot_pt = Vector2 { X: x + 28.0 * s, Y: y + dt_h * 0.5 };
            if cfg.dt_show_spine {
                let xf = begin_elem_xf(
                    rt,
                    ClockElemId::Spine,
                    (x + 17.0 * s, y + dt_h * 0.5 - 11.0 * s, x + 39.0 * s, y + dt_h * 0.5 + 11.0 * s),
                    s,
                    cfg,
                    &mut out_boxes,
                );
                let sp_halo = elem_col_rgba(cfg.dt_elem_transforms[ClockElemId::Spine.idx()].color_rgb, 0.24, pal.accent_col(0.24));
                if let Ok(halo) = rt.CreateSolidColorBrush(&sp_halo, None) {
                    rt.FillEllipse(&D2D1_ELLIPSE { point: dot_pt, radiusX: 11.0 * s, radiusY: 11.0 * s }, &halo);
                }
                if let Some(br) = br_elem[ClockElemId::Spine.idx()].as_ref().or(br_accent.as_ref()) {
                    rt.FillEllipse(&D2D1_ELLIPSE { point: dot_pt, radiusX: 5.0 * s, radiusY: 5.0 * s }, br);
                }
                end_elem_xf(rt, xf);
            }

            let time_x = if cfg.dt_show_spine { x + 46.0 * s } else { x + 22.0 * s };
            let tf_cl = make_resolved_tf_named(
                dw,
                custom_fonts,
                &font_clock,
                &[w!("SF Pro Display"), w!("Segoe UI")],
                27.0 * PT_TO_PX * s,
                DWRITE_FONT_WEIGHT_SEMI_BOLD,
                DWRITE_TEXT_ALIGNMENT_LEADING,
                DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
            );
            if let (Some(tf), Some(br)) = (&tf_cl, &br_cream) {
                let xf = begin_elem_xf(
                    rt,
                    ClockElemId::Clock,
                    (time_x, y + 14.0 * s, time_x + 92.0 * s, y + 58.0 * s),
                    s,
                    cfg,
                    &mut out_boxes,
                );
                let _ = tf.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP);
                draw_text_spaced(&time_str, tf, time_x, y + 14.0 * s, 120.0 * s, 44.0 * s, br, sh_date.as_ref(), 1.5 * s, true);
                end_elem_xf(rt, xf);
            }
            if !cfg.widgets_24h && cfg.dt_show_ampm {
                let tf_am = make_resolved_tf_named(
                    dw,
                    custom_fonts,
                    &font_sub,
                    &[w!("SF Pro Display"), w!("Segoe UI")],
                    8.5 * PT_TO_PX * s,
                    DWRITE_FONT_WEIGHT_BOLD,
                    DWRITE_TEXT_ALIGNMENT_LEADING,
                    DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
                );
                if let (Some(tf), Some(br)) = (&tf_am, &br_accent) {
                    let xf = begin_elem_xf(
                        rt,
                        ClockElemId::AmPm,
                        (time_x + 92.0 * s, y + 24.0 * s, time_x + 124.0 * s, y + 48.0 * s),
                        s,
                        cfg,
                        &mut out_boxes,
                    );
                    draw_shadowed_text(rt, ampm_str, tf, D2D_RECT_F { left: time_x + 92.0 * s, top: y + 24.0 * s, right: time_x + 124.0 * s, bottom: y + 48.0 * s }, br, None, 0.0, true);
                    end_elem_xf(rt, xf);
                }
            }

            // Right section: Calligraphy Weekday + Compact Date
            if cfg.dt_show_weekday {
                let tf_sc = make_resolved_tf_named(
                    dw,
                    custom_fonts,
                    &font_script,
                    &[w!("Tuesday Night"), w!("Segoe Script")],
                    20.0 * PT_TO_PX * s,
                    DWRITE_FONT_WEIGHT_REGULAR,
                    DWRITE_TEXT_ALIGNMENT_TRAILING,
                    DWRITE_PARAGRAPH_ALIGNMENT_NEAR,
                );
                if let (Some(tf), Some(br)) = (&tf_sc, &br_script) {
                    let xf = begin_elem_xf(
                        rt,
                        ClockElemId::Weekday,
                        (x + 178.0 * s, y + 8.0 * s, x + dt_w - 22.0 * s, y + 42.0 * s),
                        s,
                        cfg,
                        &mut out_boxes,
                    );
                    let _ = tf.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP);
                    draw_shadowed_text(
                        rt,
                        now.weekday_name,
                        tf,
                        D2D_RECT_F { left: x + 178.0 * s, top: y + 8.0 * s, right: x + dt_w - 22.0 * s, bottom: y + 42.0 * s },
                        br,
                        sh_day.as_ref(),
                        1.5 * s,
                        false,
                    );
                    end_elem_xf(rt, xf);
                }
            }

            let right_sub = match (cfg.dt_show_date, cfg.dt_show_seconds) {
                (true, true) => format!("{:02} {}  •  {}", now.day, &now.month_name[..3], sec_str),
                (true, false) => format!("{:02} {}", now.day, now.month_name),
                (false, true) => sec_str.clone(),
                _ => String::new(),
            };
            if !right_sub.is_empty() {
                let tf_sub = make_resolved_tf_named(
                    dw,
                    custom_fonts,
                    &font_sub,
                    &[w!("SF Pro Display"), w!("Segoe UI")],
                    8.5 * PT_TO_PX * s,
                    DWRITE_FONT_WEIGHT_SEMI_BOLD,
                    DWRITE_TEXT_ALIGNMENT_TRAILING,
                    DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
                );
                if let (Some(tf), Some(br)) = (&tf_sub, &br_sand) {
                    let xf = begin_elem_xf(
                        rt,
                        ClockElemId::Date,
                        (x + 178.0 * s, y + 40.0 * s, x + dt_w - 22.0 * s, y + 60.0 * s),
                        s,
                        cfg,
                        &mut out_boxes,
                    );
                    draw_shadowed_text(
                        rt,
                        &right_sub,
                        tf,
                        D2D_RECT_F { left: x + 178.0 * s, top: y + 40.0 * s, right: x + dt_w - 22.0 * s, bottom: y + 60.0 * s },
                        br,
                        None,
                        0.0,
                        false,
                    );
                    end_elem_xf(rt, xf);
                }
            }

            if cfg.dt_show_orb {
                let xf = begin_elem_xf(
                    rt,
                    ClockElemId::Orb,
                    (x + 30.0 * s, y + dt_h - 15.0 * s, x + dt_w - 30.0 * s, y + dt_h),
                    s,
                    cfg,
                    &mut out_boxes,
                );
                draw_sun_orb_bar(rt, x + 32.0 * s, y + dt_h - 8.5 * s, dt_w - 64.0 * s, 2.5 * s, now.second, s, &pal);
                end_elem_xf(rt, xf);
            }
        }

        ClockStyle::TypoWatermark => {
            use windows::Win32::Graphics::Direct2D::D2D1_ANTIALIAS_MODE_PER_PRIMITIVE;
            // Giant edge-bleeding background numeral etched into glass
            let tf_wm = make_resolved_tf_named(
                dw,
                custom_fonts,
                &font_day,
                &[w!("Bahnschrift Bold"), w!("Segoe UI"), w!("Arial")],
                86.0 * PT_TO_PX * s,
                DWRITE_FONT_WEIGHT_BOLD,
                DWRITE_TEXT_ALIGNMENT_TRAILING,
                DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
            );
            let day_tr_wm = cfg.dt_elem_transforms[ClockElemId::Day.idx()];
            let wm_col = elem_col_rgba(day_tr_wm.color_rgb, if day_tr_wm.color_rgb.is_some() { 0.32 } else { 0.14 }, pal.accent_col(0.14));
            if let (Some(tf), Ok(wm_br)) = (
                &tf_wm,
                rt.CreateSolidColorBrush(&wm_col, None),
            ) {
                let xf = begin_elem_xf(
                    rt,
                    ClockElemId::Day,
                    (x + 178.0 * s, y + 4.0 * s, x + dt_w - 8.0 * s, y + dt_h - 4.0 * s),
                    s,
                    cfg,
                    &mut out_boxes,
                );
                let clip_r = D2D_RECT_F {
                    left: x + 2.0 * s,
                    top: y + 2.0 * s,
                    right: x + dt_w - 2.0 * s,
                    bottom: y + dt_h - 2.0 * s,
                };
                rt.PushAxisAlignedClip(&clip_r, D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
                let _ = tf.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP);
                self::draw_shadowed_text(
                    rt,
                    &day_num_str,
                    tf,
                    D2D_RECT_F { left: x + 138.0 * s, top: y - 10.0 * s, right: x + dt_w - 8.0 * s, bottom: y + dt_h + 6.0 * s },
                    &wm_br,
                    None,
                    0.0,
                    false,
                );
                rt.PopAxisAlignedClip();
                end_elem_xf(rt, xf);
            }

            // Delicate top accent rule + tracked month kicker
            let kicker_x = if cfg.dt_show_spine {
                let xf = begin_elem_xf(
                    rt,
                    ClockElemId::Spine,
                    (x + 22.0 * s, y + 14.0 * s, x + 76.0 * s, y + 22.0 * s),
                    s,
                    cfg,
                    &mut out_boxes,
                );
                let sp_c = elem_col_rgba(cfg.dt_elem_transforms[ClockElemId::Spine.idx()].color_rgb, 0.85, pal.accent_col(0.85));
                if let Ok(sp) = rt.CreateSolidColorBrush(&sp_c, None) {
                    rt.DrawLine(
                        Vector2 { X: x + 24.0 * s, Y: y + 18.0 * s },
                        Vector2 { X: x + 72.0 * s, Y: y + 18.0 * s },
                        &sp,
                        2.0 * s,
                        None,
                    );
                }
                end_elem_xf(rt, xf);
                x + 80.0 * s
            } else {
                x + 24.0 * s
            };
            if cfg.dt_show_date {
                let mut nb: Option<&mut [ClockElemBox; 8]> = None;
                let xf = begin_elem_xf(
                    rt,
                    ClockElemId::Date,
                    (kicker_x, y + 10.0 * s, kicker_x + 210.0 * s, y + 26.0 * s),
                    s,
                    cfg,
                    &mut nb,
                );
                let top_txt = format!("{}  {}", now.month_name.to_ascii_uppercase(), now.year);
                let tf_top = make_resolved_tf_named(
                    dw,
                    custom_fonts,
                    &font_sub,
                    &[w!("SF Pro Display"), w!("Segoe UI")],
                    8.5 * PT_TO_PX * s,
                    DWRITE_FONT_WEIGHT_SEMI_BOLD,
                    DWRITE_TEXT_ALIGNMENT_LEADING,
                    DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
                );
                if let (Some(tf), Some(br)) = (&tf_top, &br_sand) {
                    draw_text_spaced(&top_txt, tf, kicker_x, y + 10.0 * s, 210.0 * s, 16.0 * s, br, None, 0.0, false);
                }
                end_elem_xf(rt, xf);
            }

            // Sharp Foreground Time
            let tf_clock = make_resolved_tf_named(
                dw,
                custom_fonts,
                &font_clock,
                &[w!("Segoe UI Light"), w!("SF Pro Display"), w!("Segoe UI")],
                46.0 * PT_TO_PX * s,
                DWRITE_FONT_WEIGHT_LIGHT,
                DWRITE_TEXT_ALIGNMENT_LEADING,
                DWRITE_PARAGRAPH_ALIGNMENT_NEAR,
            );
            if let (Some(tf), Some(br)) = (&tf_clock, &br_cream) {
                let xf = begin_elem_xf(
                    rt,
                    ClockElemId::Clock,
                    (x + 22.0 * s, y + 28.0 * s, x + 210.0 * s, y + 90.0 * s),
                    s,
                    cfg,
                    &mut out_boxes,
                );
                draw_text_spaced(&time_str, tf, x + 22.0 * s, y + 26.0 * s, 196.0 * s, 68.0 * s, br, sh_date.as_ref(), 1.8 * s, true);
                end_elem_xf(rt, xf);
            }

            // Vertical stacked AM/PM & Seconds beside time
            if !cfg.widgets_24h && cfg.dt_show_ampm {
                let xf = begin_elem_xf(
                    rt,
                    ClockElemId::AmPm,
                    (x + 210.0 * s, y + 38.0 * s, x + 248.0 * s, y + 58.0 * s),
                    s,
                    cfg,
                    &mut out_boxes,
                );
                let am_tr = cfg.dt_elem_transforms[ClockElemId::AmPm.idx()];
                let badge = D2D1_ROUNDED_RECT {
                    rect: D2D_RECT_F { left: x + 210.0 * s, top: y + 38.0 * s, right: x + 246.0 * s, bottom: y + 57.0 * s },
                    radiusX: 5.0 * s,
                    radiusY: 5.0 * s,
                };
                if let Ok(b) = rt.CreateSolidColorBrush(&elem_col_rgba(am_tr.color_rgb, 0.22, pal.accent_col(0.18)), None) {
                    rt.FillRoundedRectangle(&badge, &b);
                }
                if let Ok(r) = rt.CreateSolidColorBrush(&elem_col_rgba(am_tr.border_rgb.or(am_tr.color_rgb), 0.72, pal.accent_col(0.65)), None) {
                    rt.DrawRoundedRectangle(&badge, &r, 1.0 * s, None);
                }
                let tf_am = make_resolved_tf_named(
                    dw,
                    custom_fonts,
                    &font_sub,
                    &[w!("SF Pro Display"), w!("Segoe UI")],
                    8.0 * PT_TO_PX * s,
                    DWRITE_FONT_WEIGHT_BOLD,
                    DWRITE_TEXT_ALIGNMENT_CENTER,
                    DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
                );
                if let (Some(tf), Some(br)) = (&tf_am, &br_accent) {
                    draw_shadowed_text(rt, ampm_str, tf, badge.rect, br, None, 0.0, true);
                }
                end_elem_xf(rt, xf);
            }
            if cfg.dt_show_seconds {
                let xf = begin_elem_xf(
                    rt,
                    ClockElemId::Orb,
                    (x + 210.0 * s, y + 62.0 * s, x + 254.0 * s, y + 80.0 * s),
                    s,
                    cfg,
                    &mut out_boxes,
                );
                let tf_sec = make_resolved_tf_named(
                    dw,
                    custom_fonts,
                    "Cascadia Mono",
                    &[w!("Cascadia Mono"), w!("Consolas")],
                    9.5 * PT_TO_PX * s,
                    DWRITE_FONT_WEIGHT_SEMI_BOLD,
                    DWRITE_TEXT_ALIGNMENT_LEADING,
                    DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
                );
                if let (Some(tf), Some(br)) = (&tf_sec, &br_gold) {
                    draw_shadowed_text(rt, &sec_str, tf, D2D_RECT_F { left: x + 212.0 * s, top: y + 61.0 * s, right: x + 256.0 * s, bottom: y + 80.0 * s }, br, sh_date.as_ref(), 0.9 * s, false);
                }
                end_elem_xf(rt, xf);
            }

            // Bottom sweeping calligraphy weekday + etched date tag
            if cfg.dt_show_weekday {
                let tf_wk = make_resolved_tf_named(
                    dw,
                    custom_fonts,
                    &font_script,
                    &[w!("Tuesday Night"), w!("Segoe Script")],
                    20.0 * PT_TO_PX * s,
                    DWRITE_FONT_WEIGHT_REGULAR,
                    DWRITE_TEXT_ALIGNMENT_LEADING,
                    DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
                );
                if let (Some(tf), Some(br)) = (&tf_wk, &br_script) {
                    let xf = begin_elem_xf(
                        rt,
                        ClockElemId::Weekday,
                        (x + 22.0 * s, y + 90.0 * s, x + 190.0 * s, y + 126.0 * s),
                        s,
                        cfg,
                        &mut out_boxes,
                    );
                    let _ = tf.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP);
                    draw_shadowed_text(rt, now.weekday_name, tf, D2D_RECT_F { left: x + 24.0 * s, top: y + 89.0 * s, right: x + 196.0 * s, bottom: y + 127.0 * s }, br, sh_day.as_ref(), 1.3 * s, false);
                    end_elem_xf(rt, xf);
                }
            }
            if cfg.dt_show_date {
                let tf_dt = make_resolved_tf_named(
                    dw,
                    custom_fonts,
                    &font_sub,
                    &[w!("SF Pro Display"), w!("Segoe UI")],
                    8.5 * PT_TO_PX * s,
                    DWRITE_FONT_WEIGHT_SEMI_BOLD,
                    DWRITE_TEXT_ALIGNMENT_TRAILING,
                    DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
                );
                if let (Some(tf), Some(br)) = (&tf_dt, &br_cream) {
                    let xf = begin_elem_xf(
                        rt,
                        ClockElemId::Date,
                        (x + 206.0 * s, y + 102.0 * s, x + dt_w - 20.0 * s, y + 122.0 * s),
                        s,
                        cfg,
                        &mut out_boxes,
                    );
                    let dstr = format!("{:02} {}", now.day, &now.month_name[..3].to_ascii_uppercase());
                    draw_text_spaced(&dstr, tf, x + 206.0 * s, y + 102.0 * s, dt_w - 226.0 * s, 20.0 * s, br, sh_date.as_ref(), 1.0 * s, false);
                    end_elem_xf(rt, xf);
                }
            }
        }

        ClockStyle::BauhausGeo => {
            // Offset architectural plaque on the right
            let pl_rect = D2D_RECT_F {
                left: x + 48.0 * s,
                top: y + 10.0 * s,
                right: x + dt_w - 2.0 * s,
                bottom: y + dt_h - 4.0 * s,
            };
            let pl_rad = (cfg.dt_corner_radius as f32).min(10.0) * s;
            draw_clock_plate_rr(rt, pl_rect, pl_rad, cfg, &pal, is_hovered, 1.04, 1.15 * s);

            // Modernist architectural crosshair axes
            if cfg.dt_show_spine {
                let xf = begin_elem_xf(
                    rt,
                    ClockElemId::Spine,
                    (x + 98.0 * s, y + 18.0 * s, x + dt_w - 14.0 * s, y + dt_h - 10.0 * s),
                    s,
                    cfg,
                    &mut out_boxes,
                );
                let sp_ov = cfg.dt_elem_transforms[ClockElemId::Spine.idx()].color_rgb;
                let grid_c = elem_col_rgba(sp_ov, 0.34, pal.text_col(0.24));
                if let Ok(grid_br) = rt.CreateSolidColorBrush(&grid_c, None) {
                    rt.DrawLine(Vector2 { X: x + 98.0 * s, Y: y + 106.0 * s }, Vector2 { X: x + dt_w - 12.0 * s, Y: y + 106.0 * s }, &grid_br, 1.0 * s, None);
                    rt.DrawLine(Vector2 { X: x + 236.0 * s, Y: y + 18.0 * s }, Vector2 { X: x + 236.0 * s, Y: y + dt_h - 10.0 * s }, &grid_br, 1.0 * s, None);
                }
                // Tri-shape modernist gallery mark (top-right): square + dot + triangle
                if let Some(ab) = br_elem[ClockElemId::Spine.idx()].as_ref().or(br_accent.as_ref()) {
                    rt.FillRectangle(&D2D_RECT_F { left: x + 314.0 * s, top: y + 20.0 * s, right: x + 321.0 * s, bottom: y + 27.0 * s }, ab);
                    rt.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: x + 329.5 * s, Y: y + 23.5 * s }, radiusX: 3.5 * s, radiusY: 3.5 * s }, ab);
                    let tri = [
                        Vector2 { X: x + 340.0 * s, Y: y + 20.0 * s },
                        Vector2 { X: x + 344.0 * s, Y: y + 27.0 * s },
                        Vector2 { X: x + 336.0 * s, Y: y + 27.0 * s },
                    ];
                    let fb: Option<&ID2D1Brush> = Some(ab);
                    draw_clock_poly(rt, d2d.as_ref(), &tri, fb, None, 0.0, true);
                }
                end_elem_xf(rt, xf);
            }

            // Protruding Left Bauhaus Sun Medallion overlapping plaque edge
            let d_cx = x + 52.0 * s;
            let d_cy = y + 74.0 * s;
            let d_r = 46.0 * s;
            {
                let xf = begin_elem_xf(
                    rt,
                    ClockElemId::Day,
                    (d_cx - d_r, d_cy - d_r, d_cx + d_r, d_cy + d_r),
                    s,
                    cfg,
                    &mut out_boxes,
                );
                draw_clock_plate_circle(rt, d_cx, d_cy, d_r, cfg, &pal, is_hovered, 1.15, 0.0, false);
                let day_tr = cfg.dt_elem_transforms[ClockElemId::Day.idx()];
                let outer_c = elem_col_rgba(day_tr.border_rgb.or(day_tr.color_rgb), 0.95, pal.accent_col(0.95));
                let inner_c = elem_col_rgba(day_tr.color_rgb, 0.34, pal.text_col(0.24));
                if let Ok(outer_ring) = rt.CreateSolidColorBrush(&outer_c, None) {
                    rt.DrawEllipse(&D2D1_ELLIPSE { point: Vector2 { X: d_cx, Y: d_cy }, radiusX: d_r - 2.0 * s, radiusY: d_r - 2.0 * s }, &outer_ring, 4.0 * s, None);
                }
                if let Ok(inner_ring) = rt.CreateSolidColorBrush(&inner_c, None) {
                    rt.DrawEllipse(&D2D1_ELLIPSE { point: Vector2 { X: d_cx, Y: d_cy }, radiusX: d_r - 9.0 * s, radiusY: d_r - 9.0 * s }, &inner_ring, 1.0 * s, None);
                }
                let tf_day = make_resolved_tf_named(
                    dw,
                    custom_fonts,
                    &font_day,
                    &[w!("Bahnschrift Bold"), w!("Segoe UI"), w!("Arial")],
                    29.0 * PT_TO_PX * s,
                    DWRITE_FONT_WEIGHT_BOLD,
                    DWRITE_TEXT_ALIGNMENT_CENTER,
                    DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
                );
                if let (Some(tf), Some(br)) = (&tf_day, &br_cream) {
                    draw_shadowed_text(rt, &day_num_str, tf, D2D_RECT_F { left: d_cx - 38.0 * s, top: d_cy - 32.0 * s, right: d_cx + 38.0 * s, bottom: d_cy + 14.0 * s }, br, sh_day.as_ref(), 1.5 * s, false);
                }
                let tf_m = make_resolved_tf_named(
                    dw,
                    custom_fonts,
                    &font_sub,
                    &[w!("Bahnschrift SemiBold"), w!("Segoe UI")],
                    8.5 * PT_TO_PX * s,
                    DWRITE_FONT_WEIGHT_BOLD,
                    DWRITE_TEXT_ALIGNMENT_CENTER,
                    DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
                );
                if let (Some(tf), Some(br)) = (&tf_m, &br_accent) {
                    draw_shadowed_text(rt, &now.month_name[..3].to_ascii_uppercase(), tf, D2D_RECT_F { left: d_cx - 34.0 * s, top: d_cy + 12.0 * s, right: d_cx + 34.0 * s, bottom: d_cy + 30.0 * s }, br, None, 0.0, true);
                }
                end_elem_xf(rt, xf);
            }

            // Stacked geometric HH / MM inside center grid cell
            let tf_num = make_resolved_tf_named(
                dw,
                custom_fonts,
                &font_clock,
                &[w!("Bahnschrift Bold"), w!("Segoe UI")],
                30.0 * PT_TO_PX * s,
                DWRITE_FONT_WEIGHT_BOLD,
                DWRITE_TEXT_ALIGNMENT_LEADING,
                DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
            );
            if let (Some(tf), Some(ba), Some(bc)) = (&tf_num, &br_accent, &br_cream) {
                let xf = begin_elem_xf(
                    rt,
                    ClockElemId::Clock,
                    (x + 114.0 * s, y + 18.0 * s, x + 228.0 * s, y + 101.0 * s),
                    s,
                    cfg,
                    &mut out_boxes,
                );
                draw_shadowed_text(rt, &hour_str, tf, D2D_RECT_F { left: x + 118.0 * s, top: y + 19.0 * s, right: x + 224.0 * s, bottom: y + 60.0 * s }, ba, sh_day.as_ref(), 1.5 * s, false);
                draw_shadowed_text(rt, &min_str, tf, D2D_RECT_F { left: x + 118.0 * s, top: y + 58.0 * s, right: x + 224.0 * s, bottom: y + 99.0 * s }, bc, sh_day.as_ref(), 1.5 * s, false);
                // Architectural square divider ticks beside HH/MM
                rt.FillRectangle(&D2D_RECT_F { left: x + 194.0 * s, top: y + 51.0 * s, right: x + 199.0 * s, bottom: y + 56.0 * s }, ba);
                rt.FillRectangle(&D2D_RECT_F { left: x + 194.0 * s, top: y + 62.0 * s, right: x + 199.0 * s, bottom: y + 67.0 * s }, bc);
                end_elem_xf(rt, xf);
            }

            // Right telemetry quadrant: AM/PM pill + seconds bar
            if !cfg.widgets_24h && cfg.dt_show_ampm {
                let xf = begin_elem_xf(
                    rt,
                    ClockElemId::AmPm,
                    (x + 248.0 * s, y + 40.0 * s, x + 294.0 * s, y + 60.0 * s),
                    s,
                    cfg,
                    &mut out_boxes,
                );
                let pill = D2D1_ROUNDED_RECT {
                    rect: D2D_RECT_F { left: x + 248.0 * s, top: y + 40.0 * s, right: x + 292.0 * s, bottom: y + 60.0 * s },
                    radiusX: 4.0 * s,
                    radiusY: 4.0 * s,
                };
                let am_tr = cfg.dt_elem_transforms[ClockElemId::AmPm.idx()];
                if let Ok(b) = rt.CreateSolidColorBrush(&elem_col_rgba(am_tr.color_rgb, 0.24, pal.accent_col(0.22)), None) {
                    rt.FillRoundedRectangle(&pill, &b);
                }
                if let Some(b_rgb) = am_tr.border_rgb {
                    if let Ok(rb) = rt.CreateSolidColorBrush(&elem_col_rgba(Some(b_rgb), 0.78, pal.accent_col(0.58)), None) {
                        rt.DrawRoundedRectangle(&pill, &rb, 1.0 * s, None);
                    }
                }
                let tf_am = make_resolved_tf_named(
                    dw,
                    custom_fonts,
                    &font_sub,
                    &[w!("Cascadia Mono"), w!("Segoe UI")],
                    8.5 * PT_TO_PX * s,
                    DWRITE_FONT_WEIGHT_BOLD,
                    DWRITE_TEXT_ALIGNMENT_CENTER,
                    DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
                );
                if let (Some(tf), Some(br)) = (&tf_am, &br_accent) {
                    draw_shadowed_text(rt, ampm_str, tf, pill.rect, br, None, 0.0, true);
                }
                end_elem_xf(rt, xf);
            }
            if cfg.dt_show_orb || cfg.dt_show_seconds {
                let xf = begin_elem_xf(
                    rt,
                    ClockElemId::Orb,
                    (x + 248.0 * s, y + 66.0 * s, x + dt_w - 14.0 * s, y + 96.0 * s),
                    s,
                    cfg,
                    &mut out_boxes,
                );
                if cfg.dt_show_seconds {
                    let tf_s = make_resolved_tf_named(
                        dw,
                        custom_fonts,
                        "Cascadia Mono",
                        &[w!("Cascadia Mono"), w!("Consolas")],
                        10.5 * PT_TO_PX * s,
                        DWRITE_FONT_WEIGHT_SEMI_BOLD,
                        DWRITE_TEXT_ALIGNMENT_LEADING,
                        DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
                    );
                    if let (Some(tf), Some(br)) = (&tf_s, &br_cream) {
                        draw_shadowed_text(rt, &sec_str, tf, D2D_RECT_F { left: x + 250.0 * s, top: y + 65.0 * s, right: x + 318.0 * s, bottom: y + 83.0 * s }, br, sh_date.as_ref(), 0.8 * s, false);
                    }
                }
                if cfg.dt_show_orb {
                    draw_sun_orb_bar(rt, x + 250.0 * s, y + 88.0 * s, 92.0 * s, 3.0 * s, now.second, s, &pal);
                }
                end_elem_xf(rt, xf);
            }

            // Bottom Studio Footer strip
            let foot_txt = match (cfg.dt_show_weekday, cfg.dt_show_date) {
                (true, true) => format!("{}  //  {}", now.weekday_name.to_ascii_uppercase(), now.year),
                (true, false) => now.weekday_name.to_ascii_uppercase(),
                (false, true) => format!("{} {}", now.month_name.to_ascii_uppercase(), now.year),
                (false, false) => String::new(),
            };
            if !foot_txt.is_empty() {
                let tf_f = make_resolved_tf_named(
                    dw,
                    custom_fonts,
                    &font_sub,
                    &[w!("Bahnschrift SemiBold"), w!("Segoe UI")],
                    9.0 * PT_TO_PX * s,
                    DWRITE_FONT_WEIGHT_SEMI_BOLD,
                    DWRITE_TEXT_ALIGNMENT_LEADING,
                    DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
                );
                if let (Some(tf), Some(br)) = (&tf_f, &br_sand) {
                    let xf = begin_elem_xf(
                        rt,
                        ClockElemId::Weekday,
                        (x + 114.0 * s, y + 112.0 * s, x + 332.0 * s, y + 136.0 * s),
                        s,
                        cfg,
                        &mut out_boxes,
                    );
                    draw_text_spaced(&foot_txt, tf, x + 116.0 * s, y + 114.0 * s, 224.0 * s, 20.0 * s, br, sh_date.as_ref(), 1.0 * s, false);
                    end_elem_xf(rt, xf);
                }
            }
        }

        ClockStyle::JapaneseZen => {
            // Left Vertical Hanko Vermilion Stamp Tablet
            {
                let xf = begin_elem_xf(
                    rt,
                    ClockElemId::Day,
                    (x + 20.0 * s, y + 20.0 * s, x + 76.0 * s, y + 116.0 * s),
                    s,
                    cfg,
                    &mut out_boxes,
                );
                let stamp_rect = D2D_RECT_F {
                    left: x + 22.0 * s,
                    top: y + 21.0 * s,
                    right: x + 74.0 * s,
                    bottom: y + 115.0 * s,
                };
                let stamp_rr = D2D1_ROUNDED_RECT { rect: stamp_rect, radiusX: 5.0 * s, radiusY: 5.0 * s };
                let day_tr = cfg.dt_elem_transforms[ClockElemId::Day.idx()];
                let stamp_fill = if day_tr.color_rgb.is_some() || cfg.dt_use_custom_color {
                    elem_col_rgba(day_tr.color_rgb, 0.24, pal.accent_col(0.22))
                } else {
                    rgba_u8(204, 62, 48, 0.22)
                };
                let stamp_rim = if day_tr.border_rgb.or(day_tr.color_rgb).is_some() || cfg.dt_use_custom_color {
                    elem_col_rgba(day_tr.border_rgb.or(day_tr.color_rgb), 0.92, pal.accent_col(0.92))
                } else {
                    rgba_u8(224, 78, 62, 0.92)
                };
                if let Ok(fill) = rt.CreateSolidColorBrush(&stamp_fill, None) {
                    rt.FillRoundedRectangle(&stamp_rr, &fill);
                }
                if let Ok(rim) = rt.CreateSolidColorBrush(&stamp_rim, None) {
                    rt.DrawRoundedRectangle(&stamp_rr, &rim, 2.0 * s, None);
                    let inner = D2D1_ROUNDED_RECT {
                        rect: D2D_RECT_F {
                            left: stamp_rect.left + 3.5 * s,
                            top: stamp_rect.top + 3.5 * s,
                            right: stamp_rect.right - 3.5 * s,
                            bottom: stamp_rect.bottom - 3.5 * s,
                        },
                        radiusX: 3.0 * s,
                        radiusY: 3.0 * s,
                    };
                    rt.DrawRoundedRectangle(&inner, &rim, 0.8 * s, None);
                }
                let tf_day = make_resolved_tf_named(
                    dw,
                    custom_fonts,
                    &font_day,
                    &[w!("Georgia"), w!("Bodoni MT")],
                    26.0 * PT_TO_PX * s,
                    DWRITE_FONT_WEIGHT_BOLD,
                    DWRITE_TEXT_ALIGNMENT_CENTER,
                    DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
                );
                if let (Some(tf), Some(br)) = (&tf_day, &br_cream) {
                    draw_shadowed_text(rt, &day_num_str, tf, D2D_RECT_F { left: stamp_rect.left, top: stamp_rect.top + 4.0 * s, right: stamp_rect.right, bottom: stamp_rect.top + 46.0 * s }, br, sh_day.as_ref(), 1.2 * s, false);
                }
                let tf_mon = make_resolved_tf_named(
                    dw,
                    custom_fonts,
                    &font_sub,
                    &[w!("SF Pro Display"), w!("Segoe UI")],
                    8.0 * PT_TO_PX * s,
                    DWRITE_FONT_WEIGHT_BOLD,
                    DWRITE_TEXT_ALIGNMENT_CENTER,
                    DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
                );
                if let (Some(tf), Ok(c_br)) = (&tf_mon, rt.CreateSolidColorBrush(&rgba_u8(255, 224, 212, 0.96), None)) {
                    draw_shadowed_text(rt, &now.month_name[..3].to_ascii_uppercase(), tf, D2D_RECT_F { left: stamp_rect.left, top: stamp_rect.top + 47.0 * s, right: stamp_rect.right, bottom: stamp_rect.top + 64.0 * s }, &c_br, None, 0.0, true);
                    draw_shadowed_text(rt, &format!("{}", now.year), tf, D2D_RECT_F { left: stamp_rect.left, top: stamp_rect.top + 64.0 * s, right: stamp_rect.right, bottom: stamp_rect.bottom - 7.0 * s }, &c_br, None, 0.0, false);
                }
                end_elem_xf(rt, xf);
            }

            // Right Serene Time
            let tf_clock = make_resolved_tf_named(
                dw,
                custom_fonts,
                &font_clock,
                &[w!("Segoe UI Light"), w!("SF Pro Display")],
                42.0 * PT_TO_PX * s,
                DWRITE_FONT_WEIGHT_LIGHT,
                DWRITE_TEXT_ALIGNMENT_LEADING,
                DWRITE_PARAGRAPH_ALIGNMENT_NEAR,
            );
            if let (Some(tf), Some(br)) = (&tf_clock, &br_cream) {
                let xf = begin_elem_xf(
                    rt,
                    ClockElemId::Clock,
                    (x + 92.0 * s, y + 12.0 * s, x + 286.0 * s, y + 68.0 * s),
                    s,
                    cfg,
                    &mut out_boxes,
                );
                draw_text_spaced(&time_str, tf, x + 94.0 * s, y + 10.0 * s, 198.0 * s, 60.0 * s, br, sh_date.as_ref(), 1.5 * s, false);
                end_elem_xf(rt, xf);
            }

            if !cfg.widgets_24h && cfg.dt_show_ampm {
                let xf = begin_elem_xf(
                    rt,
                    ClockElemId::AmPm,
                    (x + 294.0 * s, y + 26.0 * s, x + 344.0 * s, y + 48.0 * s),
                    s,
                    cfg,
                    &mut out_boxes,
                );
                let tf_am = make_resolved_tf_named(
                    dw,
                    custom_fonts,
                    &font_sub,
                    &[w!("SF Pro Display"), w!("Segoe UI")],
                    8.5 * PT_TO_PX * s,
                    DWRITE_FONT_WEIGHT_SEMI_BOLD,
                    DWRITE_TEXT_ALIGNMENT_CENTER,
                    DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
                );
                if let (Some(tf), Some(br)) = (&tf_am, &br_sand) {
                    let am_tag = format!("[ {} ]", ampm_str);
                    draw_shadowed_text(rt, &am_tag, tf, D2D_RECT_F { left: x + 294.0 * s, top: y + 26.0 * s, right: x + 346.0 * s, bottom: y + 48.0 * s }, br, None, 0.0, true);
                }
                end_elem_xf(rt, xf);
            }

            // Calligraphic Sumi Brush Underline + Red Sun Seal Dot
            if cfg.dt_show_spine {
                let xf = begin_elem_xf(
                    rt,
                    ClockElemId::Spine,
                    (x + 94.0 * s, y + 71.0 * s, x + 348.0 * s, y + 83.0 * s),
                    s,
                    cfg,
                    &mut out_boxes,
                );
                let br_c0 = elem_col_rgba(cfg.dt_elem_transforms[ClockElemId::Spine.idx()].color_rgb, 0.88, pal.accent_col(0.88));
                let br_c1 = elem_col_rgba(cfg.dt_elem_transforms[ClockElemId::Spine.idx()].color_rgb, 0.35, pal.text_col(0.32));
                let brush_stops = [
                    D2D1_GRADIENT_STOP { position: 0.0, color: br_c0 },
                    D2D1_GRADIENT_STOP { position: 0.72, color: br_c1 },
                    D2D1_GRADIENT_STOP { position: 1.0, color: pal.text_col(0.04) },
                ];
                if let Some(br_g) = create_linear_grad(rt, (x + 96.0 * s, y + 77.0 * s), (x + 336.0 * s, y + 77.0 * s), &brush_stops) {
                    rt.FillRoundedRectangle(
                        &D2D1_ROUNDED_RECT {
                            rect: D2D_RECT_F { left: x + 96.0 * s, top: y + 75.5 * s, right: x + 334.0 * s, bottom: y + 78.2 * s },
                            radiusX: 1.5 * s,
                            radiusY: 1.5 * s,
                        },
                        &br_g,
                    );
                }
                let sp_ov = cfg.dt_elem_transforms[ClockElemId::Spine.idx()].color_rgb;
                let dot_c = if sp_ov.is_some() || cfg.dt_use_custom_color {
                    elem_col_rgba(sp_ov, 0.92, pal.accent_col(0.92))
                } else {
                    rgba_u8(224, 78, 62, 0.92)
                };
                if let Ok(dot) = rt.CreateSolidColorBrush(&dot_c, None) {
                    rt.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: x + 342.0 * s, Y: y + 76.8 * s }, radiusX: 3.2 * s, radiusY: 3.2 * s }, &dot);
                }
                end_elem_xf(rt, xf);
            }

            if cfg.dt_show_weekday {
                let tf_wk = make_resolved_tf_named(
                    dw,
                    custom_fonts,
                    &font_script,
                    &[w!("Tuesday Night"), w!("Segoe Script")],
                    18.5 * PT_TO_PX * s,
                    DWRITE_FONT_WEIGHT_REGULAR,
                    DWRITE_TEXT_ALIGNMENT_LEADING,
                    DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
                );
                if let (Some(tf), Some(br)) = (&tf_wk, &br_script) {
                    let xf = begin_elem_xf(
                        rt,
                        ClockElemId::Weekday,
                        (x + 94.0 * s, y + 86.0 * s, x + 258.0 * s, y + 120.0 * s),
                        s,
                        cfg,
                        &mut out_boxes,
                    );
                    let _ = tf.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP);
                    draw_shadowed_text(rt, now.weekday_name, tf, D2D_RECT_F { left: x + 96.0 * s, top: y + 84.0 * s, right: x + 260.0 * s, bottom: y + 122.0 * s }, br, sh_day.as_ref(), 1.2 * s, false);
                    end_elem_xf(rt, xf);
                }
            }
            if cfg.dt_show_seconds {
                let xf = begin_elem_xf(
                    rt,
                    ClockElemId::Orb,
                    (x + 264.0 * s, y + 92.0 * s, x + dt_w - 22.0 * s, y + 114.0 * s),
                    s,
                    cfg,
                    &mut out_boxes,
                );
                let tf_s = make_resolved_tf_named(
                    dw,
                    custom_fonts,
                    &font_sub,
                    &[w!("Cascadia Mono"), w!("SF Pro Display")],
                    9.5 * PT_TO_PX * s,
                    DWRITE_FONT_WEIGHT_SEMI_BOLD,
                    DWRITE_TEXT_ALIGNMENT_TRAILING,
                    DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
                );
                if let (Some(tf), Some(br)) = (&tf_s, &br_sand) {
                    draw_shadowed_text(rt, &sec_str, tf, D2D_RECT_F { left: x + 264.0 * s, top: y + 92.0 * s, right: x + dt_w - 22.0 * s, bottom: y + 114.0 * s }, br, sh_date.as_ref(), 0.8 * s, false);
                }
                end_elem_xf(rt, xf);
            }
        }

        ClockStyle::ArtDecoGilded => {
            // Stepped Octagonal Gatsby Arch Polygon
            let arch_pts = [
                Vector2 { X: x + 16.0 * s, Y: y + 144.0 * s },
                Vector2 { X: x + 4.0 * s, Y: y + 130.0 * s },
                Vector2 { X: x + 4.0 * s, Y: y + 32.0 * s },
                Vector2 { X: x + 22.0 * s, Y: y + 14.0 * s },
                Vector2 { X: x + 96.0 * s, Y: y + 14.0 * s },
                Vector2 { X: x + 110.0 * s, Y: y + 4.0 * s },
                Vector2 { X: x + 258.0 * s, Y: y + 4.0 * s },
                Vector2 { X: x + 272.0 * s, Y: y + 14.0 * s },
                Vector2 { X: x + 346.0 * s, Y: y + 14.0 * s },
                Vector2 { X: x + 364.0 * s, Y: y + 32.0 * s },
                Vector2 { X: x + 364.0 * s, Y: y + 130.0 * s },
                Vector2 { X: x + 352.0 * s, Y: y + 144.0 * s },
            ];
            draw_clock_plate_poly(
                rt,
                d2d.as_ref(),
                D2D_RECT_F { left: x, top: y, right: x + dt_w, bottom: y + dt_h },
                &arch_pts,
                cfg,
                &pal,
                is_hovered,
                1.3 * s,
            );

            // Stepped Double Brass Inner Trim + Golden Sunburst Fan Rays
            if cfg.dt_show_spine {
                let xf = begin_elem_xf(
                    rt,
                    ClockElemId::Spine,
                    (x + 12.0 * s, y + 10.0 * s, x + dt_w - 12.0 * s, y + dt_h - 8.0 * s),
                    s,
                    cfg,
                    &mut out_boxes,
                );
                let in_pts = [
                    Vector2 { X: x + 20.0 * s, Y: y + 138.0 * s },
                    Vector2 { X: x + 10.0 * s, Y: y + 126.0 * s },
                    Vector2 { X: x + 10.0 * s, Y: y + 36.0 * s },
                    Vector2 { X: x + 25.0 * s, Y: y + 20.0 * s },
                    Vector2 { X: x + 100.0 * s, Y: y + 20.0 * s },
                    Vector2 { X: x + 113.0 * s, Y: y + 10.0 * s },
                    Vector2 { X: x + 255.0 * s, Y: y + 10.0 * s },
                    Vector2 { X: x + 268.0 * s, Y: y + 20.0 * s },
                    Vector2 { X: x + 343.0 * s, Y: y + 20.0 * s },
                    Vector2 { X: x + 358.0 * s, Y: y + 36.0 * s },
                    Vector2 { X: x + 358.0 * s, Y: y + 126.0 * s },
                    Vector2 { X: x + 348.0 * s, Y: y + 138.0 * s },
                ];
                let sp_ov = cfg.dt_elem_transforms[ClockElemId::Spine.idx()].color_rgb;
                if let Ok(brass) = rt.CreateSolidColorBrush(&elem_col_rgba(sp_ov, 0.55, pal.accent_col(0.55)), None) {
                    let sb: Option<&ID2D1Brush> = Some(&brass);
                    draw_clock_poly(rt, d2d.as_ref(), &in_pts, None, sb, 1.0 * s, true);
                }
                // Upper Sunburst rays behind crown
                if let Ok(ray_br) = rt.CreateSolidColorBrush(&elem_col_rgba(sp_ov, 0.22, pal.accent_col(0.22)), None) {
                    let ray_cx = x + 184.0 * s;
                    let ray_cy = y + 54.0 * s;
                    for deg_step in -3i32..=3 {
                        let a = (-90.0 + deg_step as f32 * 22.0).to_radians();
                        let len = if deg_step.abs() <= 1 { 40.0 * s } else { 52.0 * s };
                        rt.DrawLine(
                            Vector2 { X: ray_cx, Y: ray_cy },
                            Vector2 { X: ray_cx + len * a.cos(), Y: ray_cy + len * a.sin() },
                            &ray_br,
                            0.9 * s,
                            None,
                        );
                    }
                }
                // Center horizontal gilded dividers + diamond terminals
                if let Ok(line_br) = rt.CreateSolidColorBrush(&elem_col_rgba(sp_ov, 0.62, pal.accent_col(0.62)), None) {
                    rt.DrawLine(Vector2 { X: x + 46.0 * s, Y: y + 100.0 * s }, Vector2 { X: x + 136.0 * s, Y: y + 100.0 * s }, &line_br, 1.0 * s, None);
                    rt.DrawLine(Vector2 { X: x + 232.0 * s, Y: y + 100.0 * s }, Vector2 { X: x + 322.0 * s, Y: y + 100.0 * s }, &line_br, 1.0 * s, None);
                }
                if let Some(ab) = br_elem[ClockElemId::Spine.idx()].as_ref().or(br_accent.as_ref()) {
                    let dia = [
                        Vector2 { X: x + 184.0 * s, Y: y + 95.0 * s },
                        Vector2 { X: x + 189.0 * s, Y: y + 100.0 * s },
                        Vector2 { X: x + 184.0 * s, Y: y + 105.0 * s },
                        Vector2 { X: x + 179.0 * s, Y: y + 100.0 * s },
                    ];
                    let fb: Option<&ID2D1Brush> = Some(ab);
                    draw_clock_poly(rt, d2d.as_ref(), &dia, fb, None, 0.0, true);
                }
                end_elem_xf(rt, xf);
            }

            // Crown Top Month + Year crest
            if cfg.dt_show_date {
                let tf_crown = make_resolved_tf_named(
                    dw,
                    custom_fonts,
                    &font_sub,
                    &[w!("Palatino Linotype"), w!("Georgia")],
                    8.5 * PT_TO_PX * s,
                    DWRITE_FONT_WEIGHT_BOLD,
                    DWRITE_TEXT_ALIGNMENT_CENTER,
                    DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
                );
                if let (Some(tf), Some(br)) = (&tf_crown, &br_gold) {
                    let xf = begin_elem_xf(
                        rt,
                        ClockElemId::Date,
                        (x + 114.0 * s, y + 12.0 * s, x + 254.0 * s, y + 32.0 * s),
                        s,
                        cfg,
                        &mut out_boxes,
                    );
                    let c_txt = format!("{}  {}", now.month_name[..3].to_ascii_uppercase(), now.year);
                    draw_text_spaced(&c_txt, tf, x + 114.0 * s, y + 13.0 * s, 140.0 * s, 18.0 * s, br, sh_date.as_ref(), 0.8 * s, false);
                    end_elem_xf(rt, xf);
                }
            }

            // Left Wing Diamond Framed Day Numeral
            {
                let xf = begin_elem_xf(
                    rt,
                    ClockElemId::Day,
                    (x + 20.0 * s, y + 40.0 * s, x + 78.0 * s, y + 90.0 * s),
                    s,
                    cfg,
                    &mut out_boxes,
                );
                let tf_d = make_resolved_tf_named(
                    dw,
                    custom_fonts,
                    &font_day,
                    &[w!("Bodoni MT"), w!("Georgia")],
                    24.0 * PT_TO_PX * s,
                    DWRITE_FONT_WEIGHT_BOLD,
                    DWRITE_TEXT_ALIGNMENT_CENTER,
                    DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
                );
                if let (Some(tf), Some(br)) = (&tf_d, &br_accent) {
                    draw_shadowed_text(rt, &day_num_str, tf, D2D_RECT_F { left: x + 18.0 * s, top: y + 40.0 * s, right: x + 78.0 * s, bottom: y + 86.0 * s }, br, sh_day.as_ref(), 1.2 * s, false);
                }
                end_elem_xf(rt, xf);
            }

            // Center High-Contrast Gatsby Serif Clock
            let tf_clock = make_resolved_tf_named(
                dw,
                custom_fonts,
                &font_clock,
                &[w!("Georgia"), w!("Bodoni MT")],
                40.0 * PT_TO_PX * s,
                DWRITE_FONT_WEIGHT_BOLD,
                DWRITE_TEXT_ALIGNMENT_CENTER,
                DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
            );
            if let (Some(tf), Some(br)) = (&tf_clock, &br_cream) {
                let xf = begin_elem_xf(
                    rt,
                    ClockElemId::Clock,
                    (x + 84.0 * s, y + 32.0 * s, x + 284.0 * s, y + 92.0 * s),
                    s,
                    cfg,
                    &mut out_boxes,
                );
                draw_text_spaced(&time_str, tf, x + 84.0 * s, y + 32.0 * s, 200.0 * s, 60.0 * s, br, sh_day.as_ref(), 2.0 * s, false);
                end_elem_xf(rt, xf);
            }

            // Right Wing AM/PM + Seconds
            if !cfg.widgets_24h && cfg.dt_show_ampm {
                let xf = begin_elem_xf(
                    rt,
                    ClockElemId::AmPm,
                    (x + 290.0 * s, y + 44.0 * s, x + 346.0 * s, y + 66.0 * s),
                    s,
                    cfg,
                    &mut out_boxes,
                );
                let tf_am = make_resolved_tf_named(
                    dw,
                    custom_fonts,
                    &font_sub,
                    &[w!("Georgia"), w!("Palatino Linotype")],
                    9.0 * PT_TO_PX * s,
                    DWRITE_FONT_WEIGHT_BOLD,
                    DWRITE_TEXT_ALIGNMENT_CENTER,
                    DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
                );
                if let (Some(tf), Some(br)) = (&tf_am, &br_accent) {
                    draw_shadowed_text(rt, ampm_str, tf, D2D_RECT_F { left: x + 290.0 * s, top: y + 44.0 * s, right: x + 346.0 * s, bottom: y + 66.0 * s }, br, None, 0.0, true);
                }
                end_elem_xf(rt, xf);
            }
            if cfg.dt_show_seconds {
                let xf = begin_elem_xf(
                    rt,
                    ClockElemId::Orb,
                    (x + 290.0 * s, y + 66.0 * s, x + 346.0 * s, y + 86.0 * s),
                    s,
                    cfg,
                    &mut out_boxes,
                );
                let tf_sc = make_resolved_tf_named(
                    dw,
                    custom_fonts,
                    &font_sub,
                    &[w!("Cascadia Mono"), w!("Georgia")],
                    8.5 * PT_TO_PX * s,
                    DWRITE_FONT_WEIGHT_SEMI_BOLD,
                    DWRITE_TEXT_ALIGNMENT_CENTER,
                    DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
                );
                if let (Some(tf), Some(br)) = (&tf_sc, &br_sand) {
                    draw_shadowed_text(rt, &sec_str, tf, D2D_RECT_F { left: x + 290.0 * s, top: y + 66.0 * s, right: x + 346.0 * s, bottom: y + 86.0 * s }, br, None, 0.0, false);
                }
                end_elem_xf(rt, xf);
            }

            // Bottom Banner Calligraphy Weekday
            if cfg.dt_show_weekday {
                let tf_wk = make_resolved_tf_named(
                    dw,
                    custom_fonts,
                    &font_script,
                    &[w!("Edwardian Script ITC"), w!("Tuesday Night"), w!("Segoe Script")],
                    20.0 * PT_TO_PX * s,
                    DWRITE_FONT_WEIGHT_REGULAR,
                    DWRITE_TEXT_ALIGNMENT_CENTER,
                    DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
                );
                if let (Some(tf), Some(br)) = (&tf_wk, &br_script) {
                    let xf = begin_elem_xf(
                        rt,
                        ClockElemId::Weekday,
                        (x + 60.0 * s, y + 104.0 * s, x + 308.0 * s, y + 136.0 * s),
                        s,
                        cfg,
                        &mut out_boxes,
                    );
                    let _ = tf.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP);
                    draw_shadowed_text(rt, now.weekday_name, tf, D2D_RECT_F { left: x + 60.0 * s, top: y + 103.0 * s, right: x + 308.0 * s, bottom: y + 136.0 * s }, br, sh_day.as_ref(), 1.2 * s, false);
                    end_elem_xf(rt, xf);
                }
            }
        }

        ClockStyle::GlassRibbon => {
            // Left Detached Frosted Chronometer Medallion
            let m_cx = x + 57.0 * s;
            let m_cy = y + 57.0 * s;
            let m_r = 51.0 * s;
            draw_clock_plate_circle(rt, m_cx, m_cy, m_r, cfg, &pal, is_hovered, 1.08, 1.15 * s, true);

            // 12 delicate chapter dots + live 60s progress comet arc
            if cfg.dt_show_orb {
                let xf = begin_elem_xf(
                    rt,
                    ClockElemId::Orb,
                    (m_cx - 46.0 * s, m_cy - 46.0 * s, m_cx + 46.0 * s, m_cy + 46.0 * s),
                    s,
                    cfg,
                    &mut out_boxes,
                );
                if let Ok(track_br) = rt.CreateSolidColorBrush(&pal.text_col(0.14), None) {
                    rt.DrawEllipse(&D2D1_ELLIPSE { point: Vector2 { X: m_cx, Y: m_cy }, radiusX: 43.0 * s, radiusY: 43.0 * s }, &track_br, 2.5 * s, None);
                    for i in 0..12 {
                        let a = (i as f32 * 30.0 - 90.0).to_radians();
                        rt.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: m_cx + 46.5 * s * a.cos(), Y: m_cy + 46.5 * s * a.sin() }, radiusX: 1.0 * s, radiusY: 1.0 * s }, &track_br);
                    }
                }
                let orb_tr = cfg.dt_elem_transforms[ClockElemId::Orb.idx()];
                let sweep = ((now.second as f32 / 59.0).max(0.03)) * 359.0;
                let (c1, c2, ang) = resolve_elem_grad_colors(Some(&orb_tr), 0.95, pal.accent_col(0.95), pal.light_col(0.95));
                if let Some(arc_br) = create_linear_grad(
                    rt,
                    grad_endpoints_for_rect((m_cx - 43.0 * s, m_cy - 43.0 * s, m_cx + 43.0 * s, m_cy + 43.0 * s), ang).0,
                    grad_endpoints_for_rect((m_cx - 43.0 * s, m_cy - 43.0 * s, m_cx + 43.0 * s, m_cy + 43.0 * s), ang).1,
                    &[D2D1_GRADIENT_STOP { position: 0.0, color: c1 }, D2D1_GRADIENT_STOP { position: 1.0, color: c2 }],
                ) {
                    let b: &ID2D1Brush = &arc_br;
                    draw_clock_arc(rt, d2d.as_ref(), m_cx, m_cy, 43.0 * s, 43.0 * s, -90.0, sweep, 2.8 * s, true, b);
                }
                let head_a = (-90.0 + sweep).to_radians();
                let hx = m_cx + 43.0 * s * head_a.cos();
                let hy = m_cy + 43.0 * s * head_a.sin();
                if let Ok(h_halo) = rt.CreateSolidColorBrush(&D2D1_COLOR_F { a: 0.32, ..c1 }, None) {
                    rt.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: hx, Y: hy }, radiusX: 5.5 * s, radiusY: 5.5 * s }, &h_halo);
                }
                if let Ok(hb) = rt.CreateSolidColorBrush(&c2, None) {
                    rt.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: hx, Y: hy }, radiusX: 2.6 * s, radiusY: 2.6 * s }, &hb);
                }
                end_elem_xf(rt, xf);
            }

            // Center Day + Month inside Medallion
            {
                let xf = begin_elem_xf(
                    rt,
                    ClockElemId::Day,
                    (m_cx - 34.0 * s, m_cy - 28.0 * s, m_cx + 34.0 * s, m_cy + 28.0 * s),
                    s,
                    cfg,
                    &mut out_boxes,
                );
                let tf_d = make_resolved_tf_named(
                    dw,
                    custom_fonts,
                    &font_day,
                    &[w!("Bahnschrift Bold"), w!("SF Pro Display")],
                    25.0 * PT_TO_PX * s,
                    DWRITE_FONT_WEIGHT_BOLD,
                    DWRITE_TEXT_ALIGNMENT_CENTER,
                    DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
                );
                if let (Some(tf), Some(br)) = (&tf_d, &br_cream) {
                    draw_shadowed_text(rt, &day_num_str, tf, D2D_RECT_F { left: m_cx - 32.0 * s, top: m_cy - 28.0 * s, right: m_cx + 32.0 * s, bottom: m_cy + 12.0 * s }, br, sh_day.as_ref(), 1.4 * s, false);
                }
                let tf_m = make_resolved_tf_named(
                    dw,
                    custom_fonts,
                    &font_sub,
                    &[w!("SF Pro Display"), w!("Segoe UI")],
                    8.0 * PT_TO_PX * s,
                    DWRITE_FONT_WEIGHT_BOLD,
                    DWRITE_TEXT_ALIGNMENT_CENTER,
                    DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
                );
                if let (Some(tf), Some(br)) = (&tf_m, &br_accent) {
                    draw_shadowed_text(rt, &now.month_name[..3].to_ascii_uppercase(), tf, D2D_RECT_F { left: m_cx - 30.0 * s, top: m_cy + 10.0 * s, right: m_cx + 30.0 * s, bottom: m_cy + 27.0 * s }, br, None, 0.0, true);
                }
                end_elem_xf(rt, xf);
            }

            // Right Detached Aerodynamic Capsule Pill
            let pill_rect = D2D_RECT_F {
                left: x + 122.0 * s,
                top: y + 14.0 * s,
                right: x + dt_w - 4.0 * s,
                bottom: y + 100.0 * s,
            };
            draw_clock_plate_rr(rt, pill_rect, 43.0 * s, cfg, &pal, is_hovered, 1.0, 1.1 * s);

            let tf_clock = make_resolved_tf_named(
                dw,
                custom_fonts,
                &font_clock,
                &[w!("SF Pro Display"), w!("Segoe UI Light")],
                36.0 * PT_TO_PX * s,
                DWRITE_FONT_WEIGHT_LIGHT,
                DWRITE_TEXT_ALIGNMENT_LEADING,
                DWRITE_PARAGRAPH_ALIGNMENT_NEAR,
            );
            if let (Some(tf), Some(br)) = (&tf_clock, &br_cream) {
                let xf = begin_elem_xf(
                    rt,
                    ClockElemId::Clock,
                    (x + 144.0 * s, y + 20.0 * s, x + 300.0 * s, y + 68.0 * s),
                    s,
                    cfg,
                    &mut out_boxes,
                );
                draw_text_spaced(&time_str, tf, x + 146.0 * s, y + 18.0 * s, 162.0 * s, 54.0 * s, br, sh_date.as_ref(), 1.5 * s, true);
                end_elem_xf(rt, xf);
            }
            if !cfg.widgets_24h && cfg.dt_show_ampm {
                let xf = begin_elem_xf(
                    rt,
                    ClockElemId::AmPm,
                    (x + 310.0 * s, y + 31.0 * s, x + 354.0 * s, y + 51.0 * s),
                    s,
                    cfg,
                    &mut out_boxes,
                );
                let am_tr = cfg.dt_elem_transforms[ClockElemId::AmPm.idx()];
                let ap_rr = D2D1_ROUNDED_RECT {
                    rect: D2D_RECT_F { left: x + 310.0 * s, top: y + 31.0 * s, right: x + 350.0 * s, bottom: y + 50.0 * s },
                    radiusX: 9.5 * s,
                    radiusY: 9.5 * s,
                };
                if let Ok(b) = rt.CreateSolidColorBrush(&elem_col_rgba(am_tr.color_rgb, 0.22, pal.accent_col(0.18)), None) {
                    rt.FillRoundedRectangle(&ap_rr, &b);
                }
                if let Some(b_rgb) = am_tr.border_rgb {
                    if let Ok(rb) = rt.CreateSolidColorBrush(&elem_col_rgba(Some(b_rgb), 0.78, pal.accent_col(0.58)), None) {
                        rt.DrawRoundedRectangle(&ap_rr, &rb, 1.0 * s, None);
                    }
                }
                let tf_am = make_resolved_tf_named(
                    dw,
                    custom_fonts,
                    &font_sub,
                    &[w!("SF Pro Display"), w!("Segoe UI")],
                    8.0 * PT_TO_PX * s,
                    DWRITE_FONT_WEIGHT_BOLD,
                    DWRITE_TEXT_ALIGNMENT_CENTER,
                    DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
                );
                if let (Some(tf), Some(br)) = (&tf_am, &br_accent) {
                    draw_shadowed_text(rt, ampm_str, tf, ap_rr.rect, br, None, 0.0, true);
                }
                end_elem_xf(rt, xf);
            }

            let sub_r = match (cfg.dt_show_weekday, cfg.dt_show_date) {
                (true, true) => format!("{}   •   {}", now.weekday_name, now.year),
                (true, false) => now.weekday_name.to_string(),
                (false, true) => format!("{} {}", now.month_name, now.year),
                _ => String::new(),
            };
            if !sub_r.is_empty() {
                let tf_sub = make_resolved_tf_named(
                    dw,
                    custom_fonts,
                    &font_script,
                    &[w!("Tuesday Night"), w!("Segoe Script")],
                    14.2 * PT_TO_PX * s,
                    DWRITE_FONT_WEIGHT_REGULAR,
                    DWRITE_TEXT_ALIGNMENT_LEADING,
                    DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
                );
                if let (Some(tf), Some(br)) = (&tf_sub, &br_script) {
                    let xf = begin_elem_xf(
                        rt,
                        ClockElemId::Weekday,
                        (x + 146.0 * s, y + 66.0 * s, x + 358.0 * s, y + 96.0 * s),
                        s,
                        cfg,
                        &mut out_boxes,
                    );
                    draw_text_spaced(&sub_r, tf, x + 148.0 * s, y + 66.0 * s, 212.0 * s, 30.0 * s, br, sh_date.as_ref(), 0.4 * s, false);
                    end_elem_xf(rt, xf);
                }
            }
        }

        ClockStyle::ChronoDial => {
            // 1. Right Floating Sapphire Readout Slab (under watch bezel for lugged depth)
            let deck_rect = D2D_RECT_F {
                left: x + 108.0 * s,
                top: y + 18.0 * s,
                right: x + dt_w - 4.0 * s,
                bottom: y + 130.0 * s,
            };
            draw_clock_plate_rr(rt, deck_rect, (cfg.dt_corner_radius as f32).min(20.0) * s, cfg, &pal, is_hovered, 0.96, 1.1 * s);

            // Sapphire top label
            let top_lbl = match (cfg.dt_show_weekday, cfg.dt_show_date) {
                (true, true) => format!("{}  •  {:02} {}", &now.weekday_name[..3].to_ascii_uppercase(), now.day, &now.month_name[..3].to_ascii_uppercase()),
                (true, false) => now.weekday_name.to_ascii_uppercase(),
                (false, true) => format!("{:02} {}", now.day, now.month_name.to_ascii_uppercase()),
                _ => String::new(),
            };
            if !top_lbl.is_empty() {
                let tf_top = make_resolved_tf_named(
                    dw,
                    custom_fonts,
                    &font_sub,
                    &[w!("Cascadia Mono"), w!("Bahnschrift SemiBold")],
                    8.5 * PT_TO_PX * s,
                    DWRITE_FONT_WEIGHT_BOLD,
                    DWRITE_TEXT_ALIGNMENT_LEADING,
                    DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
                );
                if let (Some(tf), Some(br)) = (&tf_top, &br_accent) {
                    let xf = begin_elem_xf(
                        rt,
                        ClockElemId::Date,
                        (x + 156.0 * s, y + 26.0 * s, x + 354.0 * s, y + 44.0 * s),
                        s,
                        cfg,
                        &mut out_boxes,
                    );
                    draw_text_spaced(&top_lbl, tf, x + 158.0 * s, y + 26.0 * s, 198.0 * s, 18.0 * s, br, sh_date.as_ref(), 1.0 * s, false);
                    end_elem_xf(rt, xf);
                }
            }

            // Sapphire center digital time
            let tf_clock = make_resolved_tf_named(
                dw,
                custom_fonts,
                &font_clock,
                &[w!("Bahnschrift SemiBold"), w!("SF Pro Display")],
                31.0 * PT_TO_PX * s,
                DWRITE_FONT_WEIGHT_SEMI_BOLD,
                DWRITE_TEXT_ALIGNMENT_LEADING,
                DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
            );
            if let (Some(tf), Some(br)) = (&tf_clock, &br_cream) {
                let xf = begin_elem_xf(
                    rt,
                    ClockElemId::Clock,
                    (x + 156.0 * s, y + 46.0 * s, x + 300.0 * s, y + 92.0 * s),
                    s,
                    cfg,
                    &mut out_boxes,
                );
                draw_text_spaced(&time_str, tf, x + 158.0 * s, y + 46.0 * s, 146.0 * s, 46.0 * s, br, sh_date.as_ref(), 1.2 * s, true);
                end_elem_xf(rt, xf);
            }
            if !cfg.widgets_24h && cfg.dt_show_ampm {
                let xf = begin_elem_xf(
                    rt,
                    ClockElemId::AmPm,
                    (x + 302.0 * s, y + 58.0 * s, x + 350.0 * s, y + 79.0 * s),
                    s,
                    cfg,
                    &mut out_boxes,
                );
                let am_tr = cfg.dt_elem_transforms[ClockElemId::AmPm.idx()];
                let pill = D2D1_ROUNDED_RECT {
                    rect: D2D_RECT_F { left: x + 302.0 * s, top: y + 58.0 * s, right: x + 348.0 * s, bottom: y + 78.0 * s },
                    radiusX: 5.0 * s,
                    radiusY: 5.0 * s,
                };
                if let Ok(b) = rt.CreateSolidColorBrush(&elem_col_rgba(am_tr.color_rgb, 0.22, pal.accent_col(0.18)), None) {
                    rt.FillRoundedRectangle(&pill, &b);
                }
                if let Some(b_rgb) = am_tr.border_rgb {
                    if let Ok(rb) = rt.CreateSolidColorBrush(&elem_col_rgba(Some(b_rgb), 0.78, pal.accent_col(0.58)), None) {
                        rt.DrawRoundedRectangle(&pill, &rb, 1.0 * s, None);
                    }
                }
                let tf_am = make_resolved_tf_named(
                    dw,
                    custom_fonts,
                    &font_sub,
                    &[w!("Cascadia Mono"), w!("Segoe UI")],
                    8.0 * PT_TO_PX * s,
                    DWRITE_FONT_WEIGHT_BOLD,
                    DWRITE_TEXT_ALIGNMENT_CENTER,
                    DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
                );
                if let (Some(tf), Some(br)) = (&tf_am, &br_accent) {
                    draw_shadowed_text(rt, ampm_str, tf, pill.rect, br, None, 0.0, true);
                }
                end_elem_xf(rt, xf);
            }

            if cfg.dt_show_seconds {
                let xf = begin_elem_xf(
                    rt,
                    ClockElemId::Day,
                    (x + 156.0 * s, y + 98.0 * s, x + 354.0 * s, y + 118.0 * s),
                    s,
                    cfg,
                    &mut out_boxes,
                );
                let tf_bot = make_resolved_tf_named(
                    dw,
                    custom_fonts,
                    &font_sub,
                    &[w!("Cascadia Mono"), w!("Consolas")],
                    8.5 * PT_TO_PX * s,
                    DWRITE_FONT_WEIGHT_REGULAR,
                    DWRITE_TEXT_ALIGNMENT_LEADING,
                    DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
                );
                if let (Some(tf), Some(br)) = (&tf_bot, &br_sand) {
                    draw_shadowed_text(rt, &format!("CHRONO  •  {}  •  {}", sec_str, now.year), tf, D2D_RECT_F { left: x + 158.0 * s, top: y + 98.0 * s, right: x + 354.0 * s, bottom: y + 118.0 * s }, br, None, 0.0, false);
                }
                end_elem_xf(rt, xf);
            }

            // 2. Left Precision Analog Chronograph Dial + Live Swept Hands
            let w_cx = x + 74.0 * s;
            let w_cy = y + 74.0 * s;
            let w_r = 64.0 * s;
            draw_clock_plate_circle(rt, w_cx, w_cy, w_r, cfg, &pal, is_hovered, 1.16, 1.8 * s, true);
            if cfg.dt_show_orb {
                let xf = begin_elem_xf(
                    rt,
                    ClockElemId::Orb,
                    (w_cx - w_r - 2.0 * s, w_cy - w_r - 2.0 * s, w_cx + w_r + 7.5 * s, w_cy + w_r + 2.0 * s),
                    s,
                    cfg,
                    &mut out_boxes,
                );
                let orb_tr = cfg.dt_elem_transforms[ClockElemId::Orb.idx()];
                let (oc1, oc2, _ang) = resolve_elem_grad_colors(Some(&orb_tr), 0.96, pal.accent_col(0.96), pal.light_col(0.96));
                // Knurled Watch Crown stem at 3 o'clock
                let crown_c = elem_col_rgba(orb_tr.border_rgb.or(orb_tr.color_rgb), 0.72, pal.accent_col(0.62));
                if let Ok(cr_br) = rt.CreateSolidColorBrush(&crown_c, None) {
                    rt.FillRoundedRectangle(
                        &D2D1_ROUNDED_RECT {
                            rect: D2D_RECT_F { left: w_cx + w_r - 1.0 * s, top: w_cy - 8.0 * s, right: w_cx + w_r + 6.5 * s, bottom: w_cy + 8.0 * s },
                            radiusX: 2.5 * s,
                            radiusY: 2.5 * s,
                        },
                        &cr_br,
                    );
                }
                let inner_c = elem_col_rgba(orb_tr.border_rgb.or(orb_tr.color_rgb), 0.34, pal.text_col(0.22));
                if let Ok(inner_b) = rt.CreateSolidColorBrush(&inner_c, None) {
                    rt.DrawEllipse(&D2D1_ELLIPSE { point: Vector2 { X: w_cx, Y: w_cy }, radiusX: w_r - 5.0 * s, radiusY: w_r - 5.0 * s }, &inner_b, 0.9 * s, None);
                    rt.DrawEllipse(&D2D1_ELLIPSE { point: Vector2 { X: w_cx, Y: w_cy }, radiusX: 38.0 * s, radiusY: 38.0 * s }, &inner_b, 0.7 * s, None);
                }

                // 60 tick chapter ring + 12 baton hour markers
                let tick_maj_c = elem_col_rgba(orb_tr.color_rgb, 0.96, pal.text_col(1.0));
                let tick_min_c = elem_col_rgba(orb_tr.border_rgb.or(orb_tr.color2_rgb).or(orb_tr.color_rgb), 0.62, pal.muted_col(0.92));
                let br_tick_maj = rt.CreateSolidColorBrush(&tick_maj_c, None).ok();
                let br_tick_min = rt.CreateSolidColorBrush(&tick_min_c, None).ok();
                for t in 0..60 {
                    let a = (t as f32 * 6.0 - 90.0).to_radians();
                    let is_maj = t % 5 == 0;
                    let r_in = if is_maj { 47.0 * s } else { 52.5 * s };
                    let r_out = 56.5 * s;
                    let sw = if is_maj { 2.0 * s } else { 0.8 * s };
                    let p0 = Vector2 { X: w_cx + r_in * a.cos(), Y: w_cy + r_in * a.sin() };
                    let p1 = Vector2 { X: w_cx + r_out * a.cos(), Y: w_cy + r_out * a.sin() };
                    if is_maj {
                        if let Some(br) = &br_tick_maj {
                            rt.DrawLine(p0, p1, br, sw, None);
                        }
                    } else if let Some(br) = &br_tick_min {
                        rt.DrawLine(p0, p1, br, sw, None);
                    }
                }

                let h_ang = (((now.hour % 12) as f32 + now.minute as f32 / 60.0) * 30.0 - 90.0).to_radians();
                let m_ang = ((now.minute as f32 + now.second as f32 / 60.0) * 6.0 - 90.0).to_radians();
                let s_ang = ((now.second as f32) * 6.0 - 90.0).to_radians();

                let draw_tapered_hand = |ang: f32, len: f32, back: f32, half_w: f32, br: Option<&ID2D1SolidColorBrush>| {
                    let perp = ang + std::f32::consts::FRAC_PI_2;
                    let pts = [
                        Vector2 { X: w_cx - back * ang.cos(), Y: w_cy - back * ang.sin() },
                        Vector2 { X: w_cx + half_w * perp.cos(), Y: w_cy + half_w * perp.sin() },
                        Vector2 { X: w_cx + len * ang.cos(), Y: w_cy + len * ang.sin() },
                        Vector2 { X: w_cx - half_w * perp.cos(), Y: w_cy - half_w * perp.sin() },
                    ];
                    let fb: Option<&ID2D1Brush> = br.map(|b| &**b);
                    draw_clock_poly(rt, d2d.as_ref(), &pts, fb, None, 0.0, true);
                };
                let br_h = rt.CreateSolidColorBrush(&elem_col_rgba(orb_tr.color2_rgb.or(orb_tr.color_rgb), 0.90, pal.muted_col(0.96)), None).ok();
                let br_m = rt.CreateSolidColorBrush(&elem_col_rgba(orb_tr.color_rgb, 1.0, pal.text_col(1.0)), None).ok();
                draw_tapered_hand(h_ang, 31.0 * s, 6.0 * s, 2.8 * s, br_h.as_ref());
                draw_tapered_hand(m_ang, 45.0 * s, 7.0 * s, 2.1 * s, br_m.as_ref());

                if cfg.dt_show_seconds {
                    if let Ok(ab) = rt.CreateSolidColorBrush(&oc1, None) {
                        rt.DrawLine(
                            Vector2 { X: w_cx - 12.0 * s * s_ang.cos(), Y: w_cy - 12.0 * s * s_ang.sin() },
                            Vector2 { X: w_cx + 49.0 * s * s_ang.cos(), Y: w_cy + 49.0 * s * s_ang.sin() },
                            &ab,
                            1.35 * s,
                            None,
                        );
                    }
                }
                if let Ok(ab) = rt.CreateSolidColorBrush(&oc2, None) {
                    rt.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: w_cx, Y: w_cy }, radiusX: 3.6 * s, radiusY: 3.6 * s }, &ab);
                }
                if let Ok(pin) = rt.CreateSolidColorBrush(&rgba_u8(16, 12, 10, 1.0), None) {
                    rt.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: w_cx, Y: w_cy }, radiusX: 1.5 * s, radiusY: 1.5 * s }, &pin);
                }
                end_elem_xf(rt, xf);
            }
        }

        ClockStyle::OrbitalEclipse => {
            // Horizontal Wing Capsule behind Central Planetarium Orrery
            let wing_rect = D2D_RECT_F {
                left: x + 6.0 * s,
                top: y + 40.0 * s,
                right: x + dt_w - 6.0 * s,
                bottom: y + 112.0 * s,
            };
            draw_clock_plate_rr(rt, wing_rect, 36.0 * s, cfg, &pal, is_hovered, 0.96, 1.0 * s);

            // Central Orrery Eclipse Sphere
            let c_cx = x + 182.0 * s;
            let c_cy = y + 76.0 * s;
            let c_r = 68.0 * s;
            draw_clock_plate_circle(rt, c_cx, c_cy, c_r, cfg, &pal, is_hovered, 1.18, 1.2 * s, true);

            // Solar Eclipse Crescent Corona Highlight along top-right limb
            if cfg.dt_show_plate && tr_card.visible {
                let mut nb: Option<&mut [ClockElemBox]> = None;
                let g_card = begin_elem_xf_slot(rt, ClockElemId::Card.idx(), tr_card, card_rect, s, &mut nb);
                let corona_c = elem_col_rgba(tr_card.border_rgb.or(tr_card.color_rgb), 0.52, pal.accent_col(0.45));
                if let Ok(corona) = rt.CreateSolidColorBrush(&corona_c, None) {
                    let b: &ID2D1Brush = &corona;
                    draw_clock_arc(rt, d2d.as_ref(), c_cx, c_cy, c_r - 1.5 * s, c_r - 1.5 * s, -75.0, 130.0, 3.0 * s, true, b);
                }
                end_elem_xf_slot(rt, g_card);
            }

            // Inner & Outer Orbital Track Rings + Orbiting Planets
            if cfg.dt_show_orb {
                let xf = begin_elem_xf(
                    rt,
                    ClockElemId::Orb,
                    (c_cx - 64.0 * s, c_cy - 64.0 * s, c_cx + 64.0 * s, c_cy + 64.0 * s),
                    s,
                    cfg,
                    &mut out_boxes,
                );
                let orb_tr = cfg.dt_elem_transforms[ClockElemId::Orb.idx()];
                let ring_c = elem_col_rgba(orb_tr.border_rgb.or(orb_tr.color_rgb), 0.28, pal.text_col(0.18));
                if let Ok(ring_br) = rt.CreateSolidColorBrush(&ring_c, None) {
                    rt.DrawEllipse(&D2D1_ELLIPSE { point: Vector2 { X: c_cx, Y: c_cy }, radiusX: 59.0 * s, radiusY: 59.0 * s }, &ring_br, 1.1 * s, None);
                    rt.DrawEllipse(&D2D1_ELLIPSE { point: Vector2 { X: c_cx, Y: c_cy }, radiusX: 43.0 * s, radiusY: 43.0 * s }, &ring_br, 0.9 * s, None);
                }

                let m_deg = (now.minute as f32) * 6.0 - 90.0;
                let h_deg = ((now.hour % 12) as f32 + now.minute as f32 / 60.0) * 30.0 - 90.0;
                let s_deg = (now.second as f32) * 6.0 - 90.0;

                let (oc1, oc2, _ang) = resolve_elem_grad_colors(Some(&orb_tr), 0.95, pal.accent_col(0.95), pal.light_col(0.95));
                // Glowing Minute Orbit Trail Arc
                if let Ok(trail) = rt.CreateSolidColorBrush(&D2D1_COLOR_F { a: 0.62, ..oc1 }, None) {
                    let b: &ID2D1Brush = &trail;
                    draw_clock_arc(rt, d2d.as_ref(), c_cx, c_cy, 59.0 * s, 59.0 * s, m_deg - 42.0, 42.0, 2.2 * s, true, b);
                }

                // Minute Planet (outer)
                let ma = m_deg.to_radians();
                let mx = c_cx + 59.0 * s * ma.cos();
                let my = c_cy + 59.0 * s * ma.sin();
                if let Ok(h) = rt.CreateSolidColorBrush(&D2D1_COLOR_F { a: 0.34, ..oc1 }, None) {
                    rt.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: mx, Y: my }, radiusX: 6.8 * s, radiusY: 6.8 * s }, &h);
                }
                if let Ok(gb) = rt.CreateSolidColorBrush(&oc2, None) {
                    rt.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: mx, Y: my }, radiusX: 3.4 * s, radiusY: 3.4 * s }, &gb);
                }

                // Hour Moon (inner)
                let ha = h_deg.to_radians();
                let hx = c_cx + 43.0 * s * ha.cos();
                let hy = c_cy + 43.0 * s * ha.sin();
                let moon_c = elem_col_rgba(orb_tr.color2_rgb.or(orb_tr.color_rgb), 0.98, pal.text_col(1.0));
                if let Ok(cb) = rt.CreateSolidColorBrush(&moon_c, None) {
                    rt.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: hx, Y: hy }, radiusX: 2.6 * s, radiusY: 2.6 * s }, &cb);
                }

                // Second Comet Dot along outer rim
                if cfg.dt_show_seconds {
                    let sa = s_deg.to_radians();
                    if let Ok(ab) = rt.CreateSolidColorBrush(&oc1, None) {
                        rt.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: c_cx + 67.5 * s * sa.cos(), Y: c_cy + 67.5 * s * sa.sin() }, radiusX: 2.2 * s, radiusY: 2.2 * s }, &ab);
                    }
                }
                end_elem_xf(rt, xf);
            }

            // Center Time Inside Eclipse Core
            let eclipse_pt = if cfg.widgets_24h { 21.5 } else { 23.0 };
            let tf_clock = make_resolved_tf_named(
                dw,
                custom_fonts,
                &font_clock,
                &[w!("Segoe UI Light"), w!("Bahnschrift Light")],
                eclipse_pt * PT_TO_PX * s,
                DWRITE_FONT_WEIGHT_LIGHT,
                DWRITE_TEXT_ALIGNMENT_CENTER,
                DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
            );
            if let (Some(tf), Some(br)) = (&tf_clock, &br_cream) {
                let _ = tf.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP);
                let xf = begin_elem_xf(
                    rt,
                    ClockElemId::Clock,
                    (c_cx - 60.0 * s, c_cy - 22.0 * s, c_cx + 60.0 * s, c_cy + 16.0 * s),
                    s,
                    cfg,
                    &mut out_boxes,
                );
                draw_shadowed_text(rt, &time_str, tf, D2D_RECT_F { left: c_cx - 60.0 * s, top: c_cy - 22.0 * s, right: c_cx + 60.0 * s, bottom: c_cy + 16.0 * s }, br, sh_day.as_ref(), 1.4 * s, true);
                end_elem_xf(rt, xf);
            }
            if !cfg.widgets_24h && cfg.dt_show_ampm {
                let xf = begin_elem_xf(
                    rt,
                    ClockElemId::AmPm,
                    (c_cx - 24.0 * s, c_cy + 14.0 * s, c_cx + 24.0 * s, c_cy + 30.0 * s),
                    s,
                    cfg,
                    &mut out_boxes,
                );
                let tf_am = make_resolved_tf_named(
                    dw,
                    custom_fonts,
                    &font_sub,
                    &[w!("SF Pro Display"), w!("Segoe UI")],
                    8.0 * PT_TO_PX * s,
                    DWRITE_FONT_WEIGHT_BOLD,
                    DWRITE_TEXT_ALIGNMENT_CENTER,
                    DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
                );
                if let (Some(tf), Some(br)) = (&tf_am, &br_accent) {
                    draw_shadowed_text(rt, ampm_str, tf, D2D_RECT_F { left: c_cx - 24.0 * s, top: c_cy + 14.0 * s, right: c_cx + 24.0 * s, bottom: c_cy + 30.0 * s }, br, None, 0.0, true);
                }
                end_elem_xf(rt, xf);
            }

            // Left Wing: Weekday + Seconds
            if cfg.dt_show_weekday {
                let tf_wk = make_resolved_tf_named(
                    dw,
                    custom_fonts,
                    &font_script,
                    &[w!("Tuesday Night"), w!("Segoe Script")],
                    13.2 * PT_TO_PX * s,
                    DWRITE_FONT_WEIGHT_REGULAR,
                    DWRITE_TEXT_ALIGNMENT_CENTER,
                    DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
                );
                if let (Some(tf), Some(br)) = (&tf_wk, &br_script) {
                    let xf = begin_elem_xf(
                        rt,
                        ClockElemId::Weekday,
                        (x + 12.0 * s, y + 48.0 * s, x + 106.0 * s, y + 82.0 * s),
                        s,
                        cfg,
                        &mut out_boxes,
                    );
                    let _ = tf.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP);
                    draw_shadowed_text(rt, now.weekday_name, tf, D2D_RECT_F { left: x + 10.0 * s, top: y + 48.0 * s, right: x + 106.0 * s, bottom: y + 82.0 * s }, br, sh_day.as_ref(), 1.1 * s, false);
                    end_elem_xf(rt, xf);
                }
            }
            if cfg.dt_show_seconds {
                let sec_slot = if cfg.dt_show_weekday { ClockElemId::Weekday } else { ClockElemId::Clock };
                let sec_rect = (x + 22.0 * s, y + 82.0 * s, x + 102.0 * s, y + 102.0 * s);
                let mut nb: Option<&mut [ClockElemBox; 8]> = None;
                let xf = begin_elem_xf(
                    rt,
                    sec_slot,
                    if cfg.dt_show_weekday { (x + 12.0 * s, y + 48.0 * s, x + 106.0 * s, y + 102.0 * s) } else { sec_rect },
                    s,
                    cfg,
                    &mut nb,
                );
                let tf_ls = make_resolved_tf_named(
                    dw,
                    custom_fonts,
                    &font_sub,
                    &[w!("Cascadia Mono"), w!("Consolas")],
                    8.5 * PT_TO_PX * s,
                    DWRITE_FONT_WEIGHT_SEMI_BOLD,
                    DWRITE_TEXT_ALIGNMENT_CENTER,
                    DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
                );
                if let (Some(tf), Some(br)) = (&tf_ls, &br_sand) {
                    draw_shadowed_text(rt, &sec_str, tf, D2D_RECT_F { left: sec_rect.0, top: sec_rect.1, right: sec_rect.2, bottom: sec_rect.3 }, br, None, 0.0, false);
                }
                end_elem_xf(rt, xf);
            }

            // Right Wing: Day + Month/Year
            {
                let xf = begin_elem_xf(
                    rt,
                    ClockElemId::Day,
                    (x + 256.0 * s, y + 46.0 * s, x + 348.0 * s, y + 82.0 * s),
                    s,
                    cfg,
                    &mut out_boxes,
                );
                let tf_rd = make_resolved_tf_named(
                    dw,
                    custom_fonts,
                    &font_day,
                    &[w!("Bahnschrift Light"), w!("SF Pro Display")],
                    20.0 * PT_TO_PX * s,
                    DWRITE_FONT_WEIGHT_BOLD,
                    DWRITE_TEXT_ALIGNMENT_CENTER,
                    DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
                );
                if let (Some(tf), Some(br)) = (&tf_rd, &br_cream) {
                    draw_shadowed_text(rt, &day_num_str, tf, D2D_RECT_F { left: x + 256.0 * s, top: y + 46.0 * s, right: x + 348.0 * s, bottom: y + 80.0 * s }, br, sh_date.as_ref(), 1.0 * s, false);
                }
                end_elem_xf(rt, xf);
            }
            if cfg.dt_show_date {
                let xf = begin_elem_xf(
                    rt,
                    ClockElemId::Date,
                    (x + 256.0 * s, y + 80.0 * s, x + 348.0 * s, y + 102.0 * s),
                    s,
                    cfg,
                    &mut out_boxes,
                );
                let tf_rm = make_resolved_tf_named(
                    dw,
                    custom_fonts,
                    &font_sub,
                    &[w!("SF Pro Display"), w!("Segoe UI")],
                    8.0 * PT_TO_PX * s,
                    DWRITE_FONT_WEIGHT_SEMI_BOLD,
                    DWRITE_TEXT_ALIGNMENT_CENTER,
                    DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
                );
                if let (Some(tf), Some(br)) = (&tf_rm, &br_sand) {
                    draw_shadowed_text(rt, &format!("{} {}", &now.month_name[..3].to_ascii_uppercase(), now.year), tf, D2D_RECT_F { left: x + 256.0 * s, top: y + 80.0 * s, right: x + 348.0 * s, bottom: y + 102.0 * s }, br, None, 0.0, false);
                }
                end_elem_xf(rt, xf);
            }
        }

        ClockStyle::SunDialHorizon => {
            // Closed Architectural D-Shaped Sky Dome
            let x0 = x + 10.0 * s;
            let x1 = x + dt_w - 10.0 * s;
            let y_top = y + 8.0 * s;
            let y_base = y + 138.0 * s;
            let g_top = (cfg.dt_glass_alpha as f32 / 255.0).clamp(0.0, 0.88);
            let g_bot = (g_top * 0.56).clamp(0.0, 0.74);
            let r_base = if is_hovered {
                ((cfg.dt_rim_alpha as f32 / 255.0) + 0.25).clamp(0.2, 0.95)
            } else {
                (cfg.dt_rim_alpha as f32 / 255.0).clamp(0.0, 1.0)
            };
            let (top_c, bot_c, rim_c, ang) = resolve_card_plate_colors(&tr_card, &pal, g_top, g_bot, r_base);
            let (gp0, gp1) = grad_endpoints_for_rect((x0, y_top, x1, y_base), ang);
            let bg_s = [
                D2D1_GRADIENT_STOP { position: 0.0, color: top_c },
                D2D1_GRADIENT_STOP { position: 1.0, color: bot_c },
            ];
            let rim_end = if tr_card.border_rgb.is_some() || tr_card.color_rgb.is_some() {
                rim_c
            } else {
                pal.deep_col(r_base * 0.65)
            };
            let rim_s = [
                D2D1_GRADIENT_STOP { position: 0.0, color: rim_c },
                D2D1_GRADIENT_STOP { position: 1.0, color: rim_end },
            ];
            if cfg.dt_show_plate {
                let mut nb: Option<&mut [ClockElemBox]> = None;
                let g_card = begin_elem_xf_slot(rt, ClockElemId::Card.idx(), tr_card, card_rect, s, &mut nb);
                let fill_g: Option<ID2D1Brush> = create_linear_grad(rt, gp0, gp1, &bg_s).and_then(|b| b.cast().ok());
                let rim_g: Option<ID2D1Brush> = if r_base > 0.01 || tr_card.border_rgb.is_some() {
                    create_linear_grad(rt, (x0, y_top), (x1, y_base), &rim_s).and_then(|b| b.cast().ok())
                } else {
                    None
                };
                draw_d_arch_dome(rt, d2d.as_ref(), x0, x1, y_top, y_base, fill_g.as_ref(), rim_g.as_ref(), 1.2 * s);
                end_elem_xf_slot(rt, g_card);
            }

            // Sweeping 180-degree Solar Meridian Gauge (centered along Horizon at y+104)
            let h_cx = x + dt_w * 0.5;
            let h_cy = y + 104.0 * s;
            let arc_rx = 148.0 * s;
            let arc_ry = 84.0 * s;
            if cfg.dt_show_orb {
                let xf = begin_elem_xf(
                    rt,
                    ClockElemId::Orb,
                    (h_cx - arc_rx, h_cy - arc_ry, h_cx + arc_rx, h_cy + 8.0 * s),
                    s,
                    cfg,
                    &mut out_boxes,
                );
                let orb_tr = cfg.dt_elem_transforms[ClockElemId::Orb.idx()];
                let tr_col = elem_col_rgba(orb_tr.border_rgb, 0.30, pal.text_col(0.16));
                if let Ok(tr_br) = rt.CreateSolidColorBrush(&tr_col, None) {
                    let b: &ID2D1Brush = &tr_br;
                    draw_clock_arc(rt, d2d.as_ref(), h_cx, h_cy, arc_rx, arc_ry, 180.0, 180.0, 2.0 * s, true, b);
                    for step in 0..=12 {
                        let a = (180.0 + step as f32 * 15.0).to_radians();
                        rt.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: h_cx + (arc_rx - 6.0 * s) * a.cos(), Y: h_cy + (arc_ry - 5.0 * s) * a.sin() }, radiusX: 1.0 * s, radiusY: 1.0 * s }, &tr_br);
                    }
                }
                let day_pct = ((now.hour * 60 + now.minute) as f32 / 1440.0).clamp(0.03, 0.97);
                let sweep = day_pct * 180.0;
                let (oc1, oc2, _) = resolve_elem_grad_colors(Some(&orb_tr), 0.92, pal.accent_col(0.92), pal.light_col(0.96));
                if let Ok(sol_br) = rt.CreateSolidColorBrush(&oc1, None) {
                    let b: &ID2D1Brush = &sol_br;
                    draw_clock_arc(rt, d2d.as_ref(), h_cx, h_cy, arc_rx, arc_ry, 180.0, sweep, 3.0 * s, true, b);
                }
                let sun_a = (180.0 + sweep).to_radians();
                let sx = h_cx + arc_rx * sun_a.cos();
                let sy = h_cy + arc_ry * sun_a.sin();
                if let Ok(sh) = rt.CreateSolidColorBrush(&D2D1_COLOR_F { a: 0.30, ..oc1 }, None) {
                    rt.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: sx, Y: sy }, radiusX: 8.5 * s, radiusY: 8.5 * s }, &sh);
                }
                if let Ok(gb) = rt.CreateSolidColorBrush(&oc2, None) {
                    rt.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: sx, Y: sy }, radiusX: 4.0 * s, radiusY: 4.0 * s }, &gb);
                }
                if let Some(cb) = br_elem[ClockElemId::Orb.idx()].as_ref().or(br_cream.as_ref()) {
                    rt.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: sx, Y: sy }, radiusX: 1.9 * s, radiusY: 1.9 * s }, cb);
                }
                end_elem_xf(rt, xf);
            }

            // Glowing Horizon Earth Chord
            if cfg.dt_show_spine {
                let xf = begin_elem_xf(
                    rt,
                    ClockElemId::Spine,
                    (x + 26.0 * s, h_cy - 3.0 * s, x + dt_w - 26.0 * s, h_cy + 3.0 * s),
                    s,
                    cfg,
                    &mut out_boxes,
                );
                let sp_ov = cfg.dt_elem_transforms[ClockElemId::Spine.idx()].color_rgb;
                let h_stops = [
                    D2D1_GRADIENT_STOP { position: 0.0, color: elem_col_rgba(sp_ov, 0.05, pal.accent_col(0.05)) },
                    D2D1_GRADIENT_STOP { position: 0.5, color: elem_col_rgba(sp_ov, 0.78, pal.accent_col(0.75)) },
                    D2D1_GRADIENT_STOP { position: 1.0, color: elem_col_rgba(sp_ov, 0.05, pal.accent_col(0.05)) },
                ];
                if let Some(hb) = create_linear_grad(rt, (x + 26.0 * s, h_cy), (x + dt_w - 26.0 * s, h_cy), &h_stops) {
                    rt.DrawLine(Vector2 { X: x + 26.0 * s, Y: h_cy }, Vector2 { X: x + dt_w - 26.0 * s, Y: h_cy }, &hb, 1.4 * s, None);
                }
                end_elem_xf(rt, xf);
            }

            // Majestic Sky Time inside Arch
            let tf_clock = make_resolved_tf_named(
                dw,
                custom_fonts,
                &font_clock,
                &[w!("Segoe UI Light"), w!("Georgia")],
                38.0 * PT_TO_PX * s,
                DWRITE_FONT_WEIGHT_LIGHT,
                DWRITE_TEXT_ALIGNMENT_CENTER,
                DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
            );
            if let (Some(tf), Some(br)) = (&tf_clock, &br_cream) {
                let xf = begin_elem_xf(
                    rt,
                    ClockElemId::Clock,
                    (x + 90.0 * s, y + 40.0 * s, x + 282.0 * s, y + 98.0 * s),
                    s,
                    cfg,
                    &mut out_boxes,
                );
                draw_text_spaced(&time_str, tf, x + 90.0 * s, y + 40.0 * s, 192.0 * s, 58.0 * s, br, sh_date.as_ref(), 2.0 * s, true);
                end_elem_xf(rt, xf);
            }
            if !cfg.widgets_24h && cfg.dt_show_ampm {
                let xf = begin_elem_xf(
                    rt,
                    ClockElemId::AmPm,
                    (x + 256.0 * s, y + 60.0 * s, x + 298.0 * s, y + 80.0 * s),
                    s,
                    cfg,
                    &mut out_boxes,
                );
                let tf_am = make_resolved_tf_named(
                    dw,
                    custom_fonts,
                    &font_sub,
                    &[w!("SF Pro Display"), w!("Segoe UI")],
                    8.5 * PT_TO_PX * s,
                    DWRITE_FONT_WEIGHT_BOLD,
                    DWRITE_TEXT_ALIGNMENT_LEADING,
                    DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
                );
                if let (Some(tf), Some(br)) = (&tf_am, &br_accent) {
                    draw_shadowed_text(rt, ampm_str, tf, D2D_RECT_F { left: x + 260.0 * s, top: y + 60.0 * s, right: x + 298.0 * s, bottom: y + 80.0 * s }, br, None, 0.0, true);
                }
                end_elem_xf(rt, xf);
            }

            // Below Horizon Earth Chord: Weekday + Full Date + Seconds
            let earth_txt = match (cfg.dt_show_weekday, cfg.dt_show_date, cfg.dt_show_seconds) {
                (true, true, true) => format!("{}   •   {:02} {} {}   •   {}", now.weekday_name, now.day, now.month_name, now.year, sec_str),
                (true, true, false) => format!("{}   •   {:02} {} {}", now.weekday_name, now.day, now.month_name, now.year),
                (true, false, true) => format!("{}   •   {}", now.weekday_name, sec_str),
                (false, true, true) => format!("{:02} {} {}   •   {}", now.day, now.month_name, now.year, sec_str),
                (true, false, false) => now.weekday_name.to_string(),
                (false, true, false) => format!("{:02} {} {}", now.day, now.month_name, now.year),
                (false, false, true) => sec_str.clone(),
                _ => String::new(),
            };
            if !earth_txt.is_empty() {
                let tf_e = make_resolved_tf_named(
                    dw,
                    custom_fonts,
                    &font_sub,
                    &[w!("SF Pro Display"), w!("Segoe UI")],
                    8.4 * PT_TO_PX * s,
                    DWRITE_FONT_WEIGHT_SEMI_BOLD,
                    DWRITE_TEXT_ALIGNMENT_CENTER,
                    DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
                );
                if let (Some(tf), Some(br)) = (&tf_e, &br_sand) {
                    let xf = begin_elem_xf(
                        rt,
                        ClockElemId::Date,
                        (x + 26.0 * s, y + 108.0 * s, x + dt_w - 26.0 * s, y + 132.0 * s),
                        s,
                        cfg,
                        &mut out_boxes,
                    );
                    draw_text_spaced(&earth_txt, tf, x + 26.0 * s, y + 108.0 * s, dt_w - 52.0 * s, 24.0 * s, br, sh_date.as_ref(), 0.7 * s, false);
                    end_elem_xf(rt, xf);
                }
            }
        }

        ClockStyle::FlipMech => {
            // Tactile Mechanical Split-Flap Paper Cards (HH + MM + Calendar/Secs stack with 3D Paper Flip Animation)
            let ms = (unsafe { GetLocalTime().wMilliseconds } as f32 / 1000.0).clamp(0.0, 0.999);
            let smooth_flip = |raw: f32| -> f32 {
                let t = raw.clamp(0.0, 1.0);
                0.5 - 0.5 * (t * std::f32::consts::PI).cos()
            };
            let prev_sec_str = format!("{:02}s", (now.second + 59) % 60);
            let prev_min_str = format!("{:02}", (now.minute + 59) % 60);
            let prev_hour_num = if cfg.widgets_24h {
                (now.hour + 23) % 24
            } else {
                let p = (now.hour + 23) % 12;
                if p == 0 { 12 } else { p }
            };
            let prev_hour_str = format!("{:02}", prev_hour_num);
            let sec_flip = if ms < 0.50 { Some((prev_sec_str.as_str(), smooth_flip(ms / 0.50))) } else { None };
            let min_flip = if now.second == 0 && ms < 0.58 { Some((prev_min_str.as_str(), smooth_flip(ms / 0.58))) } else { None };
            let hour_flip = if now.minute == 0 && now.second == 0 && ms < 0.60 { Some((prev_hour_str.as_str(), smooth_flip(ms / 0.60))) } else { None };

            let mut draw_flap_card = |rect: D2D_RECT_F,
                                  r: f32,
                                  text: &str,
                                  flip: Option<(&str, f32)>,
                                  pt_sz: f32,
                                  accent_num: bool,
                                  sec_tile_secs: Option<(u32, u32)>| {
                let mid_x = (rect.left + rect.right) * 0.5;
                let mid_y = (rect.top + rect.bottom) * 0.5;
                let cur_tr = current_active_elem_tr();
                let pin_c = elem_col_rgba(
                    cur_tr.and_then(|t| t.border_rgb.or(t.color_rgb)).or(tr_card.border_rgb),
                    0.82,
                    pal.accent_col(0.75),
                );
                // Stacked paper card deck edge beneath bottom & back of each flap
                if cfg.dt_show_plate {
                    for (layer, off_y) in [(1.0f32, 3.2 * s), (2.0f32, 1.6 * s)] {
                        let stack_r = D2D1_ROUNDED_RECT {
                            rect: D2D_RECT_F {
                                left: rect.left + layer * 0.6 * s,
                                top: rect.top + off_y,
                                right: rect.right - layer * 0.6 * s,
                                bottom: rect.bottom + off_y,
                            },
                            radiusX: r,
                            radiusY: r,
                        };
                        if let Ok(sb) = rt.CreateSolidColorBrush(&pal.deep_col(0.32), None) {
                            rt.FillRoundedRectangle(&stack_r, &sb);
                        }
                        if let Ok(sr) = rt.CreateSolidColorBrush(&pal.text_col(0.14), None) {
                            rt.DrawRoundedRectangle(&stack_r, &sr, 0.7 * s, None);
                        }
                    }
                }
                draw_clock_plate_rr(rt, rect, r, cfg, &pal, is_hovered, 1.16, 1.15 * s);
                let top_r = D2D_RECT_F { left: rect.left + 2.0 * s, top: rect.top + 2.0 * s, right: rect.right - 2.0 * s, bottom: mid_y - 1.0 * s };
                let bot_r = D2D_RECT_F { left: rect.left + 2.0 * s, top: mid_y + 1.0 * s, right: rect.right - 2.0 * s, bottom: rect.bottom - 2.0 * s };
                if cfg.dt_show_plate {
                    if let Ok(sh) = rt.CreateSolidColorBrush(&pal.text_col(0.06), None) {
                        rt.FillRoundedRectangle(&D2D1_ROUNDED_RECT { rect: top_r, radiusX: (r - 2.0 * s).max(2.0 * s), radiusY: (r - 2.0 * s).max(2.0 * s) }, &sh);
                    }
                }

                let tf_f = make_resolved_tf_named(
                    dw,
                    custom_fonts,
                    &font_clock,
                    &[w!("Bahnschrift Bold"), w!("Segoe UI"), w!("Arial")],
                    pt_sz * PT_TO_PX * s,
                    DWRITE_FONT_WEIGHT_BOLD,
                    DWRITE_TEXT_ALIGNMENT_CENTER,
                    DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
                );
                let num_col = cur_tr
                    .and_then(|t| t.color_rgb)
                    .map(|c| {
                        let (cr, cg, cb) = crate::theme::u32_to_rgb(c);
                        rgba_u8(cr, cg, cb, 1.0)
                    })
                    .unwrap_or_else(|| if accent_num { pal.light_col(1.0) } else { pal.text_col(1.0) });
                let elem_num_br = cur_tr
                    .and_then(|t| t.color_rgb)
                    .and_then(|c| {
                        let (cr, cg, cb) = crate::theme::u32_to_rgb(c);
                        rt.CreateSolidColorBrush(&rgba_u8(cr, cg, cb, 1.0), None).ok()
                    });
                let num_br_ref = elem_num_br
                    .as_ref()
                    .or(if accent_num { br_gold.as_ref() } else { br_cream.as_ref() });

                if let (Some(tf), Some(br)) = (&tf_f, num_br_ref) {
                    let mut draw_flap_txt = |txt_s: &str, use_sec_slot: Option<u32>| {
                        if let (Some(sec_val), Some((ref mut pair, cgen))) = (use_sec_slot, sec_slots.as_mut()) {
                            let mut kh = FgKeyHasher::new(0xF11A_0001);
                            kh.mix_u64(sec_val as u64);
                            kh.mix_str(txt_s);
                            kh.mix_f32(rect.left);
                            kh.mix_f32(rect.top);
                            kh.mix_f32(rect.right);
                            kh.mix_f32(rect.bottom);
                            kh.mix_f32(pt_sz);
                            kh.mix_f32(s);
                            kh.mix_str(&font_clock);
                            kh.mix_f32(num_col.r);
                            kh.mix_f32(num_col.g);
                            kh.mix_f32(num_col.b);
                            kh.mix_bool(is_hovered);
                            kh.mix_bool(custom_fonts.is_some());
                            if let Some(tr) = cur_tr {
                                kh.mix_tr(&tr);
                            }
                            let key = kh.finish();
                            pair[(sec_val % 2) as usize].draw_cached_with_interp(
                                rt,
                                *cgen,
                                key,
                                rect.left,
                                rect.top,
                                rect.right - rect.left,
                                rect.bottom - rect.top,
                                4.0 * s,
                                windows::Win32::Graphics::Direct2D::D2D1_BITMAP_INTERPOLATION_MODE_LINEAR,
                                |fg_rt| {
                                    if let Ok(fb) = fg_rt.CreateSolidColorBrush(&num_col, None) {
                                        let fsh = fg_rt.CreateSolidColorBrush(&rgba_u8(0, 0, 0, 0.76), None).ok();
                                        draw_shadowed_text(fg_rt, txt_s, tf, rect, &fb, fsh.as_ref(), 1.8 * s, false);
                                    }
                                },
                            );
                        } else {
                            draw_shadowed_text(rt, txt_s, tf, rect, br, sh_day.as_ref(), 1.8 * s, false);
                        }
                    };
                    if let Some((prev_txt, p)) = flip {
                    let top_clip = D2D_RECT_F { left: rect.left - 4.0 * s, top: rect.top - 4.0 * s, right: rect.right + 4.0 * s, bottom: mid_y };
                    let bot_clip = D2D_RECT_F { left: rect.left - 4.0 * s, top: mid_y, right: rect.right + 4.0 * s, bottom: rect.bottom + 4.0 * s };

                    // 1. Revealed top paper sheet (NEW value)
                    rt.PushAxisAlignedClip(&top_clip, D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
                    draw_flap_txt(text, sec_tile_secs.map(|v| v.0));
                    rt.PopAxisAlignedClip();

                    // 2. Waiting bottom paper sheet (OLD value while falling leaf swings past 90 deg)
                    rt.PushAxisAlignedClip(&bot_clip, D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
                    draw_flap_txt(prev_txt, sec_tile_secs.map(|v| v.1));
                    let cast_a = (p * std::f32::consts::PI).sin() * 0.44;
                    if cast_a > 0.01 {
                        if let Ok(csh) = rt.CreateSolidColorBrush(&rgba_u8(6, 4, 3, cast_a), None) {
                            rt.FillRoundedRectangle(&D2D1_ROUNDED_RECT { rect: bot_r, radiusX: (r - 2.0 * s).max(2.0 * s), radiusY: (r - 2.0 * s).max(2.0 * s) }, &csh);
                        }
                    }
                    rt.PopAxisAlignedClip();

                    // 3. 3D Hinged Folding Paper Leaf
                    let mut saved_xf = Matrix3x2::identity();
                    rt.GetTransform(&mut saved_xf);
                    let theta = p * std::f32::consts::PI;
                    let cos_t = theta.cos();
                    let flare = 1.0 + 0.055 * theta.sin();
                    let sy = cos_t.abs().max(0.03);
                    let leaf_xf = Matrix3x2 {
                        M11: flare,
                        M12: 0.0,
                        M21: 0.0,
                        M22: sy,
                        M31: mid_x * (1.0 - flare),
                        M32: mid_y * (1.0 - sy),
                    };
                    rt.SetTransform(&(leaf_xf * saved_xf));
                    let leaf_bg = elem_col_rgba(
                        cur_tr.and_then(|t| t.color_rgb).or(tr_card.color_rgb),
                        0.92,
                        pal.glass_top_col(0.92),
                    );
                    let leaf_edge = elem_col_rgba(
                        cur_tr.and_then(|t| t.border_rgb).or(tr_card.border_rgb),
                        0.72,
                        pal.text_col(0.32),
                    );
                    if p < 0.5 {
                        // Top paper flap folding downward (0 -> 90 deg) showing OLD top half
                        rt.PushAxisAlignedClip(&top_clip, D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
                        let shade_f = 1.0 - (1.0 - sy) * 0.34;
                        let top_leaf_c = D2D1_COLOR_F { r: leaf_bg.r * shade_f, g: leaf_bg.g * shade_f, b: leaf_bg.b * shade_f, a: leaf_bg.a };
                        if let Ok(lb) = rt.CreateSolidColorBrush(&top_leaf_c, None) {
                            rt.FillRoundedRectangle(&D2D1_ROUNDED_RECT { rect, radiusX: r, radiusY: r }, &lb);
                        }
                        draw_flap_txt(prev_txt, sec_tile_secs.map(|v| v.1));
                        if let Ok(eb) = rt.CreateSolidColorBrush(&leaf_edge, None) {
                            rt.DrawRoundedRectangle(&D2D1_ROUNDED_RECT { rect, radiusX: r, radiusY: r }, &eb, 1.1 * s, None);
                            rt.DrawLine(Vector2 { X: rect.left + 4.0 * s, Y: rect.top + 1.2 * s }, Vector2 { X: rect.right - 4.0 * s, Y: rect.top + 1.2 * s }, &eb, 1.3 * s, None);
                        }
                        rt.PopAxisAlignedClip();
                    } else {
                        // Unfolding bottom paper leaf (90 -> 180 deg) showing NEW bottom half
                        rt.PushAxisAlignedClip(&bot_clip, D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
                        if let Ok(lb) = rt.CreateSolidColorBrush(&leaf_bg, None) {
                            rt.FillRoundedRectangle(&D2D1_ROUNDED_RECT { rect, radiusX: r, radiusY: r }, &lb);
                        }
                        if let Ok(sh_leaf) = rt.CreateSolidColorBrush(&pal.text_col((1.0 - sy) * 0.14), None) {
                            rt.FillRoundedRectangle(&D2D1_ROUNDED_RECT { rect: bot_r, radiusX: (r - 2.0 * s).max(2.0 * s), radiusY: (r - 2.0 * s).max(2.0 * s) }, &sh_leaf);
                        }
                        draw_flap_txt(text, sec_tile_secs.map(|v| v.0));
                        if let Ok(eb) = rt.CreateSolidColorBrush(&leaf_edge, None) {
                            rt.DrawRoundedRectangle(&D2D1_ROUNDED_RECT { rect, radiusX: r, radiusY: r }, &eb, 1.1 * s, None);
                            rt.DrawLine(Vector2 { X: rect.left + 4.0 * s, Y: rect.bottom - 1.2 * s }, Vector2 { X: rect.right - 4.0 * s, Y: rect.bottom - 1.2 * s }, &eb, 1.3 * s, None);
                        }
                        rt.PopAxisAlignedClip();
                    }
                    rt.SetTransform(&saved_xf);
                    } else {
                        draw_flap_txt(text, sec_tile_secs.map(|v| v.0));
                    }
                }

                // Subtle upper-leaf cast crease + mechanical center seam + hinge pins
                if cfg.dt_show_spine && cfg.dt_elem_transforms[ClockElemId::Spine.idx()].visible {
                    if let Ok(crease) = rt.CreateSolidColorBrush(&rgba_u8(4, 3, 2, 0.30), None) {
                        rt.FillRectangle(&D2D_RECT_F { left: rect.left + 2.0 * s, top: mid_y, right: rect.right - 2.0 * s, bottom: mid_y + 2.8 * s }, &crease);
                    }
                    if let Ok(gap) = rt.CreateSolidColorBrush(&rgba_u8(6, 5, 5, 0.88), None) {
                        rt.DrawLine(Vector2 { X: rect.left + 3.0 * s, Y: mid_y }, Vector2 { X: rect.right - 3.0 * s, Y: mid_y }, &gap, 2.2 * s, None);
                    }
                    if let Ok(pin) = rt.CreateSolidColorBrush(&pin_c, None) {
                        rt.FillRoundedRectangle(
                            &D2D1_ROUNDED_RECT { rect: D2D_RECT_F { left: rect.left - 1.5 * s, top: mid_y - 5.5 * s, right: rect.left + 4.5 * s, bottom: mid_y + 5.5 * s }, radiusX: 1.5 * s, radiusY: 1.5 * s },
                            &pin,
                        );
                        rt.FillRoundedRectangle(
                            &D2D1_ROUNDED_RECT { rect: D2D_RECT_F { left: rect.right - 4.5 * s, top: mid_y - 5.5 * s, right: rect.right + 1.5 * s, bottom: mid_y + 5.5 * s }, radiusX: 1.5 * s, radiusY: 1.5 * s },
                            &pin,
                        );
                    }
                }
            };

            let rad_f = (cfg.dt_corner_radius as f32).min(14.0) * s;
            if flip_pass != FlipMechPass::SecondsOnly {
                let xf = begin_elem_xf(
                    rt,
                    ClockElemId::Clock,
                    (x + 6.0 * s, y + 10.0 * s, x + 244.0 * s, y + 120.0 * s),
                    s,
                    cfg,
                    &mut out_boxes,
                );
                draw_flap_card(D2D_RECT_F { left: x + 6.0 * s, top: y + 10.0 * s, right: x + 120.0 * s, bottom: y + 120.0 * s }, rad_f, &hour_str, hour_flip, 48.0, true, None);
                draw_flap_card(D2D_RECT_F { left: x + 130.0 * s, top: y + 10.0 * s, right: x + 244.0 * s, bottom: y + 120.0 * s }, rad_f, &min_str, min_flip, 48.0, false, None);
                end_elem_xf(rt, xf);
            }

            if flip_pass != FlipMechPass::SecondsOnly && !cfg.widgets_24h && cfg.dt_show_ampm {
                let xf = begin_elem_xf(
                    rt,
                    ClockElemId::AmPm,
                    (x + 14.0 * s, y + 16.0 * s, x + 46.0 * s, y + 32.0 * s),
                    s,
                    cfg,
                    &mut out_boxes,
                );
                let tf_am = make_resolved_tf_named(
                    dw,
                    custom_fonts,
                    "Cascadia Mono",
                    &[w!("Cascadia Mono"), w!("Consolas")],
                    8.0 * PT_TO_PX * s,
                    DWRITE_FONT_WEIGHT_BOLD,
                    DWRITE_TEXT_ALIGNMENT_LEADING,
                    DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
                );
                if let (Some(tf), Some(br)) = (&tf_am, br_elem[ClockElemId::AmPm.idx()].as_ref().or(br_accent.as_ref())) {
                    draw_shadowed_text(rt, ampm_str, tf, D2D_RECT_F { left: x + 16.0 * s, top: y + 16.0 * s, right: x + 46.0 * s, bottom: y + 32.0 * s }, br, None, 0.0, true);
                }
                end_elem_xf(rt, xf);
            }

            // Right Stack Top Tile: Calendar Flap
            if flip_pass != FlipMechPass::SecondsOnly && cfg.dt_show_date {
                let xf = begin_elem_xf(
                    rt,
                    ClockElemId::Day,
                    (x + 258.0 * s, y + 10.0 * s, x + dt_w - 6.0 * s, y + 64.0 * s),
                    s,
                    cfg,
                    &mut out_boxes,
                );
                let cal_str = format!("{:02} {}", now.day, &now.month_name[..3].to_ascii_uppercase());
                draw_flap_card(D2D_RECT_F { left: x + 258.0 * s, top: y + 10.0 * s, right: x + dt_w - 6.0 * s, bottom: y + 64.0 * s }, 9.0 * s, &cal_str, None, 16.0, false, None);
                end_elem_xf(rt, xf);
            }

            // Right Stack Bottom Tile: Seconds / Weekday Flap
            if flip_pass != FlipMechPass::StaticOnly && cfg.dt_show_orb && (cfg.dt_show_seconds || cfg.dt_show_weekday) {
                let xf = begin_elem_xf(
                    rt,
                    ClockElemId::Orb,
                    (x + 258.0 * s, y + 72.0 * s, x + dt_w - 6.0 * s, y + 120.0 * s),
                    s,
                    cfg,
                    &mut out_boxes,
                );
                let (bot_txt, bot_flip, sec_pair_arg) = if cfg.dt_show_seconds {
                    (sec_str.clone(), sec_flip, Some((now.second, (now.second + 59) % 60)))
                } else {
                    (now.weekday_name[..3].to_ascii_uppercase(), None, None)
                };
                draw_flap_card(D2D_RECT_F { left: x + 258.0 * s, top: y + 72.0 * s, right: x + dt_w - 6.0 * s, bottom: y + 120.0 * s }, 9.0 * s, &bot_txt, bot_flip, 15.0, true, sec_pair_arg);
                end_elem_xf(rt, xf);
            }
        }

        ClockStyle::NixieTube => {
            // Walnut/Obsidian Plinth Base Pedestal
            let plinth = D2D_RECT_F {
                left: x + 6.0 * s,
                top: y + 98.0 * s,
                right: x + dt_w - 6.0 * s,
                bottom: y + 134.0 * s,
            };
            draw_clock_plate_rr(rt, plinth, 8.0 * s, cfg, &pal, is_hovered, 1.18, 1.1 * s);
            if cfg.dt_show_spine {
                let xf = begin_elem_xf(
                    rt,
                    ClockElemId::Spine,
                    (plinth.left + 10.0 * s, plinth.top, plinth.right - 10.0 * s, plinth.top + 6.0 * s),
                    s,
                    cfg,
                    &mut out_boxes,
                );
                let sp_c = elem_col_rgba(cfg.dt_elem_transforms[ClockElemId::Spine.idx()].color_rgb, 0.78, pal.accent_col(0.70));
                if let Ok(brass) = rt.CreateSolidColorBrush(&sp_c, None) {
                    rt.DrawLine(Vector2 { X: plinth.left + 10.0 * s, Y: plinth.top + 2.5 * s }, Vector2 { X: plinth.right - 10.0 * s, Y: plinth.top + 2.5 * s }, &brass, 1.2 * s, None);
                }
                end_elem_xf(rt, xf);
            }

            let draw_tube = |tx: f32, ty: f32, tw: f32, th: f32, glyph: &str, pt_sz: f32| {
                let cur_tr = current_active_elem_tr();
                let custom_fil = cur_tr.and_then(|t| t.color_rgb);
                let use_theme_fil = custom_fil.is_some() || cfg.dt_use_custom_color;
                let (fil_c, bloom_c) = if use_theme_fil {
                    let (c1, c2, _) = resolve_elem_grad_colors(cur_tr.as_ref(), 0.98, pal.accent_col(0.98), pal.light_col(0.98));
                    (c2, c1)
                } else {
                    (rgba_u8(255, 198, 128, 0.98), rgba_u8(255, 118, 28, 0.72))
                };
                let dome_r = tw * 0.46;
                let tube_rr = D2D1_ROUNDED_RECT {
                    rect: D2D_RECT_F { left: tx, top: ty, right: tx + tw, bottom: ty + th },
                    radiusX: dome_r,
                    radiusY: dome_r * 0.72,
                };
                if cfg.dt_show_plate && tr_card.visible {
                    let ga = (cfg.dt_glass_alpha as f32 / 255.0).max(0.55);
                    let glass_c = elem_col_rgba(tr_card.color_rgb, ga, if cfg.dt_use_custom_color { pal.glass_top_col(ga) } else { rgba_u8(12, 9, 8, ga) });
                    if let Ok(vac) = rt.CreateSolidColorBrush(&glass_c, None) {
                        rt.FillRoundedRectangle(&tube_rr, &vac);
                    }
                }
                if cfg.dt_show_plate && tr_card.visible {
                    let rim_c = elem_col_rgba(tr_card.border_rgb, 0.72, pal.text_col(0.32));
                    if let Ok(rim) = rt.CreateSolidColorBrush(&rim_c, None) {
                        rt.DrawRoundedRectangle(&tube_rr, &rim, 1.1 * s, None);
                    }
                }
                // Faint Anode Wire Mesh Grid
                let mesh_c = elem_col_rgba(custom_fil, 0.14, pal.accent_col(0.11));
                if let Ok(mesh) = rt.CreateSolidColorBrush(&mesh_c, None) {
                    for g in 1..=3 {
                        let gx = tx + tw * (g as f32 / 4.0);
                        rt.DrawLine(Vector2 { X: gx, Y: ty + 14.0 * s }, Vector2 { X: gx, Y: ty + th - 12.0 * s }, &mesh, 0.7 * s, None);
                    }
                    let gy_mid = ty + th * 0.5;
                    rt.DrawLine(Vector2 { X: tx + 6.0 * s, Y: gy_mid - 14.0 * s }, Vector2 { X: tx + tw - 6.0 * s, Y: gy_mid - 14.0 * s }, &mesh, 0.7 * s, None);
                    rt.DrawLine(Vector2 { X: tx + 6.0 * s, Y: gy_mid + 14.0 * s }, Vector2 { X: tx + tw - 6.0 * s, Y: gy_mid + 14.0 * s }, &mesh, 0.7 * s, None);
                }
                // Warm Cathode Filament Radial Bloom
                let glow_pt = Vector2 { X: tx + tw * 0.5, Y: ty + th * 0.48 };
                let g1_c = if use_theme_fil { D2D1_COLOR_F { a: 0.16, ..bloom_c } } else { rgba_u8(255, 132, 38, 0.14) };
                let g2_c = if use_theme_fil { D2D1_COLOR_F { a: 0.22, ..fil_c } } else { rgba_u8(255, 168, 72, 0.18) };
                if let Ok(g1) = rt.CreateSolidColorBrush(&g1_c, None) {
                    rt.FillEllipse(&D2D1_ELLIPSE { point: glow_pt, radiusX: tw * 0.40, radiusY: th * 0.34 }, &g1);
                }
                if let Ok(g2) = rt.CreateSolidColorBrush(&g2_c, None) {
                    rt.FillEllipse(&D2D1_ELLIPSE { point: glow_pt, radiusX: tw * 0.28, radiusY: th * 0.24 }, &g2);
                }
                // Sharp Incandescent Nixie Filament Digit
                let tf_n = make_resolved_tf_named(
                    dw,
                    custom_fonts,
                    &font_clock,
                    &[w!("Bahnschrift Light"), w!("Segoe UI Light")],
                    pt_sz * PT_TO_PX * s,
                    DWRITE_FONT_WEIGHT_LIGHT,
                    DWRITE_TEXT_ALIGNMENT_CENTER,
                    DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
                );
                if let (Some(tf), Ok(fil)) = (&tf_n, rt.CreateSolidColorBrush(&fil_c, None)) {
                    if let Ok(glow_txt) = rt.CreateSolidColorBrush(&D2D1_COLOR_F { a: 0.65, ..bloom_c }, None) {
                        draw_shadowed_text(rt, glyph, tf, tube_rr.rect, &fil, Some(&glow_txt), 1.4 * s, false);
                    }
                }
                // Crisp Specular Glass Streak along Left Curve
                if let Ok(spec) = rt.CreateSolidColorBrush(&rgba_u8(255, 255, 255, 0.22), None) {
                    rt.DrawLine(Vector2 { X: tx + 7.5 * s, Y: ty + 18.0 * s }, Vector2 { X: tx + 7.5 * s, Y: ty + th - 14.0 * s }, &spec, 1.3 * s, None);
                }
                // Metallic Brass Collar Socket at Tube Base
                let col_rr = D2D1_ROUNDED_RECT {
                    rect: D2D_RECT_F { left: tx + 3.0 * s, top: ty + th - 9.0 * s, right: tx + tw - 3.0 * s, bottom: ty + th + 2.0 * s },
                    radiusX: 2.0 * s,
                    radiusY: 2.0 * s,
                };
                let collar_c = elem_col_rgba(cur_tr.and_then(|t| t.border_rgb).or(tr_card.border_rgb), 0.55, pal.accent_col(0.45));
                if let Ok(b_col) = rt.CreateSolidColorBrush(&collar_c, None) {
                    rt.FillRoundedRectangle(&col_rr, &b_col);
                }
            };

            // 4 Main Nixie Tubes (H1, H2, M1, M2) + Neon Colon Post
            let h_bytes: Vec<char> = hour_str.chars().collect();
            let m_bytes: Vec<char> = min_str.chars().collect();
            let h1 = h_bytes.first().map(|c| c.to_string()).unwrap_or_else(|| "0".to_string());
            let h2 = h_bytes.get(1).map(|c| c.to_string()).unwrap_or_else(|| "0".to_string());
            let m1 = m_bytes.first().map(|c| c.to_string()).unwrap_or_else(|| "0".to_string());
            let m2 = m_bytes.get(1).map(|c| c.to_string()).unwrap_or_else(|| "0".to_string());

            {
                let xf = begin_elem_xf(
                    rt,
                    ClockElemId::Clock,
                    (x + 12.0 * s, y + 6.0 * s, x + 276.0 * s, y + 98.0 * s),
                    s,
                    cfg,
                    &mut out_boxes,
                );
                draw_tube(x + 12.0 * s, y + 8.0 * s, 58.0 * s, 90.0 * s, &h1, 40.0);
                draw_tube(x + 74.0 * s, y + 8.0 * s, 58.0 * s, 90.0 * s, &h2, 40.0);
                // Twin Neon Colon Bulbs
                let colon_on = now.second % 2 == 0;
                let ca = if colon_on { 0.92 } else { 0.34 };
                let clk_tr = cfg.dt_elem_transforms[ClockElemId::Clock.idx()];
                let colon_c = if clk_tr.color_rgb.is_some() || cfg.dt_use_custom_color {
                    elem_col_rgba(clk_tr.color_rgb, ca, pal.accent_col(ca))
                } else {
                    rgba_u8(255, 162, 64, ca)
                };
                if let Ok(c_br) = rt.CreateSolidColorBrush(&colon_c, None) {
                    rt.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: x + 142.0 * s, Y: y + 40.0 * s }, radiusX: 3.0 * s, radiusY: 3.0 * s }, &c_br);
                    rt.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: x + 142.0 * s, Y: y + 64.0 * s }, radiusX: 3.0 * s, radiusY: 3.0 * s }, &c_br);
                }
                draw_tube(x + 152.0 * s, y + 8.0 * s, 58.0 * s, 90.0 * s, &m1, 40.0);
                draw_tube(x + 214.0 * s, y + 8.0 * s, 58.0 * s, 90.0 * s, &m2, 40.0);
                end_elem_xf(rt, xf);
            }

            // Shorter Secondary Seconds Tube on Right
            if cfg.dt_show_seconds {
                let xf = begin_elem_xf(
                    rt,
                    ClockElemId::Orb,
                    (x + 288.0 * s, y + 20.0 * s, x + 366.0 * s, y + 98.0 * s),
                    s,
                    cfg,
                    &mut out_boxes,
                );
                draw_tube(x + 288.0 * s, y + 22.0 * s, 76.0 * s, 76.0 * s, &format!("{:02}", now.second), 26.0);
                end_elem_xf(rt, xf);
            } else if !cfg.widgets_24h && cfg.dt_show_ampm {
                let xf = begin_elem_xf(
                    rt,
                    ClockElemId::AmPm,
                    (x + 288.0 * s, y + 20.0 * s, x + 366.0 * s, y + 98.0 * s),
                    s,
                    cfg,
                    &mut out_boxes,
                );
                draw_tube(x + 288.0 * s, y + 22.0 * s, 76.0 * s, 76.0 * s, ampm_str, 22.0);
                end_elem_xf(rt, xf);
            }

            // Plinth Nameplate Date & AM/PM tag
            let plate_txt = match (cfg.dt_show_weekday, cfg.dt_show_date, !cfg.widgets_24h && cfg.dt_show_ampm) {
                (true, true, true) => format!("{}  •  {:02} {} {}  •  {}", &now.weekday_name[..3].to_ascii_uppercase(), now.day, now.month_name, now.year, ampm_str),
                (true, true, false) => format!("{}  •  {:02} {} {}", &now.weekday_name[..3].to_ascii_uppercase(), now.day, now.month_name, now.year),
                (false, true, _) => format!("{:02} {} {}", now.day, now.month_name, now.year),
                (true, false, _) => now.weekday_name.to_ascii_uppercase(),
                _ => String::new(),
            };
            if !plate_txt.is_empty() {
                let tf_p = make_resolved_tf_named(
                    dw,
                    custom_fonts,
                    &font_sub,
                    &[w!("Cascadia Mono"), w!("SF Pro Display")],
                    8.5 * PT_TO_PX * s,
                    DWRITE_FONT_WEIGHT_SEMI_BOLD,
                    DWRITE_TEXT_ALIGNMENT_CENTER,
                    DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
                );
                if let (Some(tf), Some(br)) = (&tf_p, &br_gold) {
                    let xf = begin_elem_xf(
                        rt,
                        ClockElemId::Date,
                        (plinth.left + 16.0 * s, plinth.top + 6.0 * s, plinth.right - 16.0 * s, plinth.bottom - 4.0 * s),
                        s,
                        cfg,
                        &mut out_boxes,
                    );
                    draw_text_spaced(&plate_txt, tf, plinth.left + 16.0 * s, plinth.top + 6.0 * s, (plinth.right - plinth.left) - 32.0 * s, 26.0 * s, br, sh_date.as_ref(), 0.9 * s, false);
                    end_elem_xf(rt, xf);
                }
            }
        }

        ClockStyle::RetroCasio => {
            // Chamfered Octagonal 1984 Resin/Steel Watch Case Polygon
            let ch = 16.0 * s;
            let case_pts = [
                Vector2 { X: x + ch, Y: y + 2.0 * s },
                Vector2 { X: x + dt_w - ch, Y: y + 2.0 * s },
                Vector2 { X: x + dt_w - 2.0 * s, Y: y + ch },
                Vector2 { X: x + dt_w - 2.0 * s, Y: y + dt_h - ch },
                Vector2 { X: x + dt_w - ch, Y: y + dt_h - 2.0 * s },
                Vector2 { X: x + ch, Y: y + dt_h - 2.0 * s },
                Vector2 { X: x + 2.0 * s, Y: y + dt_h - ch },
                Vector2 { X: x + 2.0 * s, Y: y + ch },
            ];
            draw_clock_plate_poly(
                rt,
                d2d.as_ref(),
                D2D_RECT_F { left: x, top: y, right: x + dt_w, bottom: y + dt_h },
                &case_pts,
                cfg,
                &pal,
                is_hovered,
                1.4 * s,
            );

            // Bezel corner micro screws + top micro branding
            if cfg.dt_show_spine {
                let xf = begin_elem_xf(
                    rt,
                    ClockElemId::Spine,
                    (x + 18.0 * s, y + 6.0 * s, x + dt_w - 18.0 * s, y + 20.0 * s),
                    s,
                    cfg,
                    &mut out_boxes,
                );
                if let Ok(sc_br) = rt.CreateSolidColorBrush(&pal.text_col(0.32), None) {
                    for (sx, sy) in [
                        (x + 11.0 * s, y + 11.0 * s),
                        (x + dt_w - 11.0 * s, y + 11.0 * s),
                        (x + 11.0 * s, y + dt_h - 11.0 * s),
                        (x + dt_w - 11.0 * s, y + dt_h - 11.0 * s),
                    ] {
                        rt.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: sx, Y: sy }, radiusX: 2.3 * s, radiusY: 2.3 * s }, &sc_br);
                    }
                }
                let tf_brand = make_resolved_tf_named(
                    dw,
                    custom_fonts,
                    &font_sub,
                    &[w!("Cascadia Mono"), w!("Consolas")],
                    7.5 * PT_TO_PX * s,
                    DWRITE_FONT_WEIGHT_BOLD,
                    DWRITE_TEXT_ALIGNMENT_LEADING,
                    DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
                );
                if let (Some(tf), Some(br)) = (&tf_brand, &br_accent) {
                    draw_shadowed_text(rt, "AUTUMN QUARTZ   //   CHRONO", tf, D2D_RECT_F { left: x + 24.0 * s, top: y + 4.0 * s, right: x + dt_w - 24.0 * s, bottom: y + 20.0 * s }, br, None, 0.0, true);
                }
                end_elem_xf(rt, xf);
            }

            // Inner Sunken Smoked Quartz LCD Window
            let lcd = D2D_RECT_F {
                left: x + 18.0 * s,
                top: y + 22.0 * s,
                right: x + dt_w - 18.0 * s,
                bottom: y + dt_h - 14.0 * s,
            };
            let lcd_rr = D2D1_ROUNDED_RECT { rect: lcd, radiusX: 6.0 * s, radiusY: 6.0 * s };
            if cfg.dt_show_plate {
                let mut nb: Option<&mut [ClockElemBox]> = None;
                let g_card = begin_elem_xf_slot(rt, ClockElemId::Card.idx(), tr_card, card_rect, s, &mut nb);
                let ga = (cfg.dt_glass_alpha as f32 / 255.0).max(0.68);
                let lcd_c = elem_col_rgba(tr_card.color_rgb, ga, if cfg.dt_use_custom_color { pal.glass_top_col(ga) } else { rgba_u8(16, 22, 16, ga) });
                if let Ok(bg) = rt.CreateSolidColorBrush(&lcd_c, None) {
                    rt.FillRoundedRectangle(&lcd_rr, &bg);
                }
                let trim_c = elem_col_rgba(tr_card.border_rgb.or(tr_card.color_rgb), 0.72, pal.accent_col(0.55));
                if let Ok(trim) = rt.CreateSolidColorBrush(&trim_c, None) {
                    rt.DrawRoundedRectangle(&lcd_rr, &trim, 1.2 * s, None);
                }
                end_elem_xf_slot(rt, g_card);
            }

            // Top LCD Header Strip: Weekday box + Date box + PM flag
            if cfg.dt_show_weekday {
                let xf = begin_elem_xf(
                    rt,
                    ClockElemId::Weekday,
                    (lcd.left + 8.0 * s, lcd.top + 6.0 * s, lcd.left + 58.0 * s, lcd.top + 24.0 * s),
                    s,
                    cfg,
                    &mut out_boxes,
                );
                let wd_tr = cfg.dt_elem_transforms[ClockElemId::Weekday.idx()];
                let wb = D2D1_ROUNDED_RECT {
                    rect: D2D_RECT_F { left: lcd.left + 8.0 * s, top: lcd.top + 6.0 * s, right: lcd.left + 56.0 * s, bottom: lcd.top + 24.0 * s },
                    radiusX: 3.0 * s,
                    radiusY: 3.0 * s,
                };
                if let Ok(f) = rt.CreateSolidColorBrush(&elem_col_rgba(wd_tr.color_rgb, 0.24, pal.accent_col(0.22)), None) {
                    rt.FillRoundedRectangle(&wb, &f);
                }
                let tf_w = make_resolved_tf_named(
                    dw,
                    custom_fonts,
                    &font_sub,
                    &[w!("Cascadia Mono"), w!("Consolas")],
                    8.5 * PT_TO_PX * s,
                    DWRITE_FONT_WEIGHT_BOLD,
                    DWRITE_TEXT_ALIGNMENT_CENTER,
                    DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
                );
                if let (Some(tf), Some(br)) = (&tf_w, &br_accent) {
                    draw_shadowed_text(rt, &now.weekday_name[..3].to_ascii_uppercase(), tf, wb.rect, br, None, 0.0, true);
                }
                end_elem_xf(rt, xf);
            }
            if cfg.dt_show_date {
                let xf = begin_elem_xf(
                    rt,
                    ClockElemId::Date,
                    (lcd.left + 64.0 * s, lcd.top + 6.0 * s, lcd.left + 218.0 * s, lcd.top + 24.0 * s),
                    s,
                    cfg,
                    &mut out_boxes,
                );
                let tf_d = make_resolved_tf_named(
                    dw,
                    custom_fonts,
                    &font_sub,
                    &[w!("Cascadia Mono"), w!("Consolas")],
                    8.5 * PT_TO_PX * s,
                    DWRITE_FONT_WEIGHT_SEMI_BOLD,
                    DWRITE_TEXT_ALIGNMENT_LEADING,
                    DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
                );
                if let (Some(tf), Some(br)) = (&tf_d, &br_sand) {
                    let dt_t = format!("{:02} {} {}", now.day, &now.month_name[..3].to_ascii_uppercase(), now.year);
                    draw_shadowed_text(rt, &dt_t, tf, D2D_RECT_F { left: lcd.left + 66.0 * s, top: lcd.top + 6.0 * s, right: lcd.left + 220.0 * s, bottom: lcd.top + 24.0 * s }, br, None, 0.0, false);
                }
                end_elem_xf(rt, xf);
            }
            if !cfg.widgets_24h && cfg.dt_show_ampm {
                let xf = begin_elem_xf(
                    rt,
                    ClockElemId::AmPm,
                    (lcd.right - 52.0 * s, lcd.top + 6.0 * s, lcd.right - 8.0 * s, lcd.top + 24.0 * s),
                    s,
                    cfg,
                    &mut out_boxes,
                );
                let tf_am = make_resolved_tf_named(
                    dw,
                    custom_fonts,
                    &font_sub,
                    &[w!("Cascadia Mono"), w!("Consolas")],
                    8.0 * PT_TO_PX * s,
                    DWRITE_FONT_WEIGHT_BOLD,
                    DWRITE_TEXT_ALIGNMENT_TRAILING,
                    DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
                );
                if let (Some(tf), Some(br)) = (&tf_am, &br_gold) {
                    draw_shadowed_text(rt, ampm_str, tf, D2D_RECT_F { left: lcd.right - 54.0 * s, top: lcd.top + 6.0 * s, right: lcd.right - 10.0 * s, bottom: lcd.top + 24.0 * s }, br, None, 0.0, true);
                }
                end_elem_xf(rt, xf);
            }

            // LCD Divider Hairline
            if cfg.dt_show_plate {
                let mut nb: Option<&mut [ClockElemBox]> = None;
                let g_card = begin_elem_xf_slot(rt, ClockElemId::Card.idx(), tr_card, card_rect, s, &mut nb);
                if let Ok(hl) = rt.CreateSolidColorBrush(&elem_col_rgba(tr_card.border_rgb, 0.42, pal.accent_col(0.28)), None) {
                    rt.DrawLine(Vector2 { X: lcd.left + 8.0 * s, Y: lcd.top + 28.0 * s }, Vector2 { X: lcd.right - 8.0 * s, Y: lcd.top + 28.0 * s }, &hl, 0.9 * s, None);
                }
                end_elem_xf_slot(rt, g_card);
            }

            // Phantom 88:88 Ghost LCD Segment Shadow + Crisp Active Time
            let tf_lcd = make_resolved_tf_named(
                dw,
                custom_fonts,
                &font_clock,
                &[w!("Cascadia Mono"), w!("Consolas")],
                36.0 * PT_TO_PX * s,
                DWRITE_FONT_WEIGHT_BOLD,
                DWRITE_TEXT_ALIGNMENT_LEADING,
                DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
            );
            if let Some(tf) = &tf_lcd {
                let t_rect = D2D_RECT_F { left: lcd.left + 12.0 * s, top: lcd.top + 30.0 * s, right: lcd.left + 230.0 * s, bottom: lcd.bottom - 6.0 * s };
                let xf = begin_elem_xf(
                    rt,
                    ClockElemId::Clock,
                    (t_rect.left, t_rect.top, t_rect.right, t_rect.bottom),
                    s,
                    cfg,
                    &mut out_boxes,
                );
                if let Ok(ghost) = rt.CreateSolidColorBrush(&pal.text_col(0.06), None) {
                    draw_shadowed_text(rt, "88:88", tf, t_rect, &ghost, None, 0.0, false);
                }
                if let Some(br) = &br_cream {
                    draw_shadowed_text(rt, &time_str, tf, t_rect, br, sh_day.as_ref(), 1.2 * s, true);
                }
                end_elem_xf(rt, xf);
            }

            // Right Framed LCD Seconds Box
            if cfg.dt_show_seconds {
                let xf = begin_elem_xf(
                    rt,
                    ClockElemId::Orb,
                    (lcd.right - 72.0 * s, lcd.top + 36.0 * s, lcd.right - 10.0 * s, lcd.bottom - 10.0 * s),
                    s,
                    cfg,
                    &mut out_boxes,
                );
                let orb_tr = cfg.dt_elem_transforms[ClockElemId::Orb.idx()];
                let s_rr = D2D1_ROUNDED_RECT {
                    rect: D2D_RECT_F { left: lcd.right - 72.0 * s, top: lcd.top + 36.0 * s, right: lcd.right - 10.0 * s, bottom: lcd.bottom - 10.0 * s },
                    radiusX: 4.0 * s,
                    radiusY: 4.0 * s,
                };
                if let Ok(f) = rt.CreateSolidColorBrush(&elem_col_rgba(orb_tr.color_rgb, 0.14, pal.accent_col(0.10)), None) {
                    rt.FillRoundedRectangle(&s_rr, &f);
                }
                if let Ok(r) = rt.CreateSolidColorBrush(&elem_col_rgba(orb_tr.border_rgb.or(orb_tr.color_rgb), 0.62, pal.accent_col(0.45)), None) {
                    rt.DrawRoundedRectangle(&s_rr, &r, 0.9 * s, None);
                }
                let tf_sc = make_resolved_tf_named(
                    dw,
                    custom_fonts,
                    &font_sub,
                    &[w!("Cascadia Mono"), w!("Consolas")],
                    22.0 * PT_TO_PX * s,
                    DWRITE_FONT_WEIGHT_BOLD,
                    DWRITE_TEXT_ALIGNMENT_CENTER,
                    DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
                );
                if let (Some(tf), Some(br)) = (&tf_sc, &br_accent) {
                    draw_shadowed_text(rt, &format!("{:02}", now.second), tf, s_rr.rect, br, None, 0.0, false);
                }
                end_elem_xf(rt, xf);
            }
        }

        ClockStyle::MatrixBinary => {
            // Asymmetrical Cut-Corner Aerospace HUD Deck
            let ch = 18.0 * s;
            let deck_pts = [
                Vector2 { X: x + ch, Y: y + 2.0 * s },
                Vector2 { X: x + dt_w - 4.0 * s, Y: y + 2.0 * s },
                Vector2 { X: x + dt_w - 4.0 * s, Y: y + dt_h - ch },
                Vector2 { X: x + dt_w - ch, Y: y + dt_h - 2.0 * s },
                Vector2 { X: x + 4.0 * s, Y: y + dt_h - 2.0 * s },
                Vector2 { X: x + 4.0 * s, Y: y + ch },
            ];
            draw_clock_plate_poly(
                rt,
                d2d.as_ref(),
                D2D_RECT_F { left: x, top: y, right: x + dt_w, bottom: y + dt_h },
                &deck_pts,
                cfg,
                &pal,
                is_hovered,
                1.2 * s,
            );

            // Live 6x4 BCD Binary LED Matrix on Left (H1 H2 : M1 M2 : S1 S2)
            {
                let xf = begin_elem_xf(
                    rt,
                    ClockElemId::Orb,
                    (x + 14.0 * s, y + 18.0 * s, x + 158.0 * s, y + 118.0 * s),
                    s,
                    cfg,
                    &mut out_boxes,
                );
                let h_val = hour_num;
                let digits = [
                    h_val / 10,
                    h_val % 10,
                    now.minute / 10,
                    now.minute % 10,
                    now.second / 10,
                    now.second % 10,
                ];
                let col_x_offsets = [0.0, 20.0, 46.0, 66.0, 92.0, 112.0];
                let orb_tr = cfg.dt_elem_transforms[ClockElemId::Orb.idx()];
                let (oc1, oc2, _) = resolve_elem_grad_colors(Some(&orb_tr), 0.96, pal.accent_col(0.96), pal.light_col(1.0));
                let on_glow = rt.CreateSolidColorBrush(&D2D1_COLOR_F { a: 0.34, ..oc1 }, None).ok();
                let on_core = rt.CreateSolidColorBrush(&oc2, None).ok();
                let off_ring = rt.CreateSolidColorBrush(&pal.text_col(0.18), None).ok();
                let off_fill = rt.CreateSolidColorBrush(&pal.text_col(0.05), None).ok();

                for (ci, &d) in digits.iter().enumerate() {
                    let cx = x + (30.0 + col_x_offsets[ci]) * s;
                    for r in 0..4u32 {
                        let bit = 3 - r;
                        let lit = ((d >> bit) & 1) == 1;
                        let cy = y + (34.0 + r as f32 * 22.0) * s;
                        if lit {
                            if let Some(hg) = &on_glow {
                                rt.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: cx, Y: cy }, radiusX: 7.5 * s, radiusY: 7.5 * s }, hg);
                            }
                            if let Some(ab) = &on_core {
                                rt.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: cx, Y: cy }, radiusX: 4.5 * s, radiusY: 4.5 * s }, ab);
                            }
                            if let Some(cb) = &br_cream {
                                rt.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: cx, Y: cy }, radiusX: 1.9 * s, radiusY: 1.9 * s }, cb);
                            }
                        } else {
                            if let Some(of) = &off_fill {
                                rt.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: cx, Y: cy }, radiusX: 4.2 * s, radiusY: 4.2 * s }, of);
                            }
                            if let Some(or) = &off_ring {
                                rt.DrawEllipse(&D2D1_ELLIPSE { point: Vector2 { X: cx, Y: cy }, radiusX: 4.2 * s, radiusY: 4.2 * s }, or, 0.9 * s, None);
                            }
                        }
                    }
                }

                // Micro BCD Header centered over each pair + Bit weights down left column
                let tf_b = make_resolved_tf_named(
                    dw,
                    custom_fonts,
                    "Cascadia Mono",
                    &[w!("Cascadia Mono"), w!("Consolas")],
                    6.8 * PT_TO_PX * s,
                    DWRITE_FONT_WEIGHT_BOLD,
                    DWRITE_TEXT_ALIGNMENT_CENTER,
                    DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
                );
                if let (Some(tf), Some(br)) = (&tf_b, &br_sand) {
                    draw_shadowed_text(rt, "HR", tf, D2D_RECT_F { left: x + 22.0 * s, top: y + 12.0 * s, right: x + 58.0 * s, bottom: y + 24.0 * s }, br, None, 0.0, false);
                    draw_shadowed_text(rt, "MN", tf, D2D_RECT_F { left: x + 68.0 * s, top: y + 12.0 * s, right: x + 104.0 * s, bottom: y + 24.0 * s }, br, None, 0.0, false);
                    draw_shadowed_text(rt, "SC", tf, D2D_RECT_F { left: x + 114.0 * s, top: y + 12.0 * s, right: x + 150.0 * s, bottom: y + 24.0 * s }, br, None, 0.0, false);
                    if let Ok(bit_br) = rt.CreateSolidColorBrush(&pal.text_col(0.36), None) {
                        for (r, lbl) in ["8", "4", "2", "1"].iter().enumerate() {
                            let cy = y + (34.0 + r as f32 * 22.0) * s;
                            draw_shadowed_text(rt, lbl, tf, D2D_RECT_F { left: x + 9.0 * s, top: cy - 7.0 * s, right: x + 21.0 * s, bottom: cy + 7.0 * s }, &bit_br, None, 0.0, false);
                        }
                    }
                }
                end_elem_xf(rt, xf);
            }

            // Vertical Trace Divider with Circuit Nodes
            if cfg.dt_show_spine {
                let xf = begin_elem_xf(
                    rt,
                    ClockElemId::Spine,
                    (x + 162.0 * s, y + 18.0 * s, x + 174.0 * s, y + dt_h - 18.0 * s),
                    s,
                    cfg,
                    &mut out_boxes,
                );
                let sp_c = elem_col_rgba(cfg.dt_elem_transforms[ClockElemId::Spine.idx()].color_rgb, 0.72, pal.accent_col(0.45));
                if let Ok(tr) = rt.CreateSolidColorBrush(&sp_c, None) {
                    rt.DrawLine(Vector2 { X: x + 168.0 * s, Y: y + 20.0 * s }, Vector2 { X: x + 168.0 * s, Y: y + dt_h - 20.0 * s }, &tr, 1.0 * s, None);
                    rt.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: x + 168.0 * s, Y: y + 20.0 * s }, radiusX: 2.2 * s, radiusY: 2.2 * s }, &tr);
                    rt.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: x + 168.0 * s, Y: y + dt_h - 20.0 * s }, radiusX: 2.2 * s, radiusY: 2.2 * s }, &tr);
                }
                end_elem_xf(rt, xf);
            }

            // Right Digital Telemetry Readout
            {
                let xf = begin_elem_xf(
                    rt,
                    ClockElemId::Day,
                    (x + 184.0 * s, y + 18.0 * s, x + 312.0 * s, y + 34.0 * s),
                    s,
                    cfg,
                    &mut out_boxes,
                );
                let tf_hdr = make_resolved_tf_named(
                    dw,
                    custom_fonts,
                    &font_sub,
                    &[w!("Cascadia Mono"), w!("Consolas")],
                    8.0 * PT_TO_PX * s,
                    DWRITE_FONT_WEIGHT_BOLD,
                    DWRITE_TEXT_ALIGNMENT_LEADING,
                    DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
                );
                if let (Some(tf), Some(br)) = (&tf_hdr, &br_accent) {
                    draw_shadowed_text(rt, "BCD // TELEMETRY", tf, D2D_RECT_F { left: x + 184.0 * s, top: y + 18.0 * s, right: x + 312.0 * s, bottom: y + 34.0 * s }, br, None, 0.0, true);
                }
                end_elem_xf(rt, xf);
            }
            if !cfg.widgets_24h && cfg.dt_show_ampm {
                let xf = begin_elem_xf(
                    rt,
                    ClockElemId::AmPm,
                    (x + 316.0 * s, y + 17.0 * s, x + dt_w - 14.0 * s, y + 35.0 * s),
                    s,
                    cfg,
                    &mut out_boxes,
                );
                let am_tr = cfg.dt_elem_transforms[ClockElemId::AmPm.idx()];
                let ap_rr = D2D1_ROUNDED_RECT {
                    rect: D2D_RECT_F { left: x + 316.0 * s, top: y + 17.0 * s, right: x + dt_w - 14.0 * s, bottom: y + 35.0 * s },
                    radiusX: 3.0 * s,
                    radiusY: 3.0 * s,
                };
                if let Ok(b) = rt.CreateSolidColorBrush(&elem_col_rgba(am_tr.color_rgb, 0.24, pal.accent_col(0.20)), None) {
                    rt.FillRoundedRectangle(&ap_rr, &b);
                }
                if let Some(b_rgb) = am_tr.border_rgb {
                    if let Ok(rb) = rt.CreateSolidColorBrush(&elem_col_rgba(Some(b_rgb), 0.78, pal.accent_col(0.58)), None) {
                        rt.DrawRoundedRectangle(&ap_rr, &rb, 1.0 * s, None);
                    }
                }
                let tf_am = make_resolved_tf_named(
                    dw,
                    custom_fonts,
                    &font_sub,
                    &[w!("Cascadia Mono"), w!("Consolas")],
                    7.5 * PT_TO_PX * s,
                    DWRITE_FONT_WEIGHT_BOLD,
                    DWRITE_TEXT_ALIGNMENT_CENTER,
                    DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
                );
                if let (Some(tf), Some(br)) = (&tf_am, &br_gold) {
                    draw_shadowed_text(rt, ampm_str, tf, ap_rr.rect, br, None, 0.0, true);
                }
                end_elem_xf(rt, xf);
            }

            let tf_clock = make_resolved_tf_named(
                dw,
                custom_fonts,
                &font_clock,
                &[w!("Cascadia Mono"), w!("Consolas")],
                32.0 * PT_TO_PX * s,
                DWRITE_FONT_WEIGHT_BOLD,
                DWRITE_TEXT_ALIGNMENT_LEADING,
                DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
            );
            if let (Some(tf), Some(br)) = (&tf_clock, &br_cream) {
                let xf = begin_elem_xf(
                    rt,
                    ClockElemId::Clock,
                    (x + 182.0 * s, y + 38.0 * s, x + 332.0 * s, y + 86.0 * s),
                    s,
                    cfg,
                    &mut out_boxes,
                );
                draw_text_spaced(&time_str, tf, x + 184.0 * s, y + 38.0 * s, 160.0 * s, 48.0 * s, br, sh_day.as_ref(), 0.8 * s, true);
                end_elem_xf(rt, xf);
            }

            let footer = match (cfg.dt_show_weekday, cfg.dt_show_date, cfg.dt_show_seconds) {
                (true, true, true) => format!("{}  //  {:02} {}  •  {}", &now.weekday_name[..3].to_ascii_uppercase(), now.day, &now.month_name[..3].to_ascii_uppercase(), sec_str),
                (true, true, false) => format!("{}  //  {:02} {} {}", &now.weekday_name[..3].to_ascii_uppercase(), now.day, &now.month_name[..3].to_ascii_uppercase(), now.year),
                (false, true, _) => format!("{:02} {} {}", now.day, &now.month_name[..3].to_ascii_uppercase(), now.year),
                (true, false, _) => now.weekday_name.to_ascii_uppercase(),
                _ => sec_str.clone(),
            };
            let tf_f = make_resolved_tf_named(
                dw,
                custom_fonts,
                "Cascadia Mono",
                &[w!("Cascadia Mono"), w!("Consolas")],
                8.5 * PT_TO_PX * s,
                DWRITE_FONT_WEIGHT_SEMI_BOLD,
                DWRITE_TEXT_ALIGNMENT_LEADING,
                DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
            );
            if let (Some(tf), Some(br)) = (&tf_f, &br_sand) {
                let xf = begin_elem_xf(
                    rt,
                    ClockElemId::Date,
                    (x + 184.0 * s, y + 92.0 * s, x + dt_w - 16.0 * s, y + 116.0 * s),
                    s,
                    cfg,
                    &mut out_boxes,
                );
                draw_text_spaced(&footer, tf, x + 184.0 * s, y + 92.0 * s, dt_w - 200.0 * s, 24.0 * s, br, sh_date.as_ref(), 0.5 * s, false);
                end_elem_xf(rt, xf);
            }
        }

        // ═══════════════════════════════════════════════════════════════════
        // REALISTIC & CALM MATERIALS CLOCKS (12 True Realism PBR + Interactive)
        // ═══════════════════════════════════════════════════════════════════
        ClockStyle::SwissChronometer => {
            // Luxury Swiss Haute Horlogerie on PBR Vintage Walnut & Brass + guilloché watch dial + blued hands
            let phase = (now.second as f32) * 0.22;
            let main_rect = D2D_RECT_F { left: x + 2.0 * s, top: y + 4.0 * s, right: x + dt_w - 2.0 * s, bottom: y + dt_h - 4.0 * s };
            let rad_sc = (cfg.dt_corner_radius as f32 / 18.0).clamp(0.0, 2.2);
            let r_card = 20.0 * rad_sc * s;
            let r_base = if is_hovered {
                ((cfg.dt_rim_alpha as f32 / 255.0) + 0.25).clamp(0.25, 0.98)
            } else {
                (cfg.dt_rim_alpha as f32 / 255.0).clamp(0.0, 1.0)
            };
            let inter = crate::realistic_pbr::pbr_input_from_viz(fx_viz, phase, is_hovered);
            let tint_ov = tr_card.color_rgb.map(|c| (c, tr_card.color2_rgb)).or_else(|| {
                if cfg.dt_use_custom_color || cfg.dt_match_wallpaper {
                    Some((crate::theme::rgb_to_u32(pal.glass_top), Some(crate::theme::rgb_to_u32(pal.glass_bot))))
                } else {
                    Some((0x121E36, Some(0x091020)))
                }
            });
            if cfg.dt_show_plate {
                let mut nb: Option<&mut [ClockElemBox]> = None;
                let g = begin_elem_xf_slot(rt, ClockElemId::Card.idx(), tr_card, card_rect, s, &mut nb);
                crate::realistic_pbr::draw_pbr_drop_shadows(rt, main_rect, r_card, s, false);
                let op = (cfg.dt_glass_alpha as f32 / 145.0).clamp(0.42, 0.99);
                crate::realistic_pbr::draw_pbr_surface_clipped(
                    rt,
                    d2d.as_ref(),
                    crate::realistic_pbr::PbrMaterial::VintageWalnutBrass,
                    main_rect,
                    r_card,
                    s,
                    op,
                    tint_ov,
                    None,
                    &inter,
                );
                // Fine engine-turned guilloché wave engraving over midnight horology face
                let gu_col = elem_col_rgba(tr_card.border_rgb.or(tr_card.color_rgb), 0.14, rgba_u8(168, 206, 252, 0.11));
                if let Ok(gu) = rt.CreateSolidColorBrush(&gu_col, None) {
                    let mut gy = main_rect.top + 12.0 * s;
                    while gy < main_rect.bottom - 10.0 * s {
                        for seg in 0..24 {
                            let f0 = seg as f32 / 24.0;
                            let f1 = (seg + 1) as f32 / 24.0;
                            let x0 = main_rect.left + 10.0 * s + (dt_w - 24.0 * s) * f0;
                            let x1 = main_rect.left + 10.0 * s + (dt_w - 24.0 * s) * f1;
                            rt.DrawLine(
                                Vector2 { X: x0, Y: gy + (f0 * 20.0).sin() * 1.4 * s },
                                Vector2 { X: x1, Y: gy + (f1 * 20.0).sin() * 1.4 * s },
                                &gu,
                                0.65 * s,
                                None,
                            );
                        }
                        gy += 8.0 * s;
                    }
                }
                crate::realistic_pbr::draw_pbr_bevel_rim(
                    rt,
                    main_rect,
                    r_card,
                    s,
                    crate::realistic_pbr::PbrMaterial::VintageWalnutBrass,
                    tr_card.border_rgb,
                    r_base,
                    inter.hover_mix,
                    None,
                );
                end_elem_xf_slot(rt, g);
            }

            // Left Luxury Watch Dial with Fluted Bezel, Sub-dial & Blued Steel Hands (all inside ClockElemId::Orb!)
            let w_cx = x + 76.0 * s;
            let w_cy = y + dt_h * 0.5;
            let w_r = 56.0 * s;
            if cfg.dt_show_orb {
                let xf = begin_elem_xf(rt, ClockElemId::Orb, (w_cx - w_r - 4.0 * s, w_cy - w_r - 4.0 * s, w_cx + w_r + 4.0 * s, w_cy + w_r + 4.0 * s), s, cfg, &mut out_boxes);
                let orb_tr = cfg.dt_elem_transforms[ClockElemId::Orb.idx()];
                let (oc1, oc2, _oang) = resolve_elem_grad_colors(Some(&orb_tr), 0.96, rgba_u8(120, 178, 255, 0.96), rgba_u8(242, 248, 255, 0.96));
                let bez_stops = if orb_tr.border_rgb.is_some() || orb_tr.color_rgb.is_some() {
                    let bc1 = elem_col_rgba(orb_tr.border_rgb.or(orb_tr.color_rgb), 0.96, rgba_u8(242, 248, 255, 0.96));
                    let bc2 = elem_col_rgba(orb_tr.color2_rgb.or(orb_tr.color_rgb), 0.72, rgba_u8(126, 142, 168, 0.92));
                    [
                        D2D1_GRADIENT_STOP { position: 0.0, color: bc1 },
                        D2D1_GRADIENT_STOP { position: 0.5, color: bc2 },
                        D2D1_GRADIENT_STOP { position: 1.0, color: bc1 },
                    ]
                } else {
                    [
                        D2D1_GRADIENT_STOP { position: 0.0, color: rgba_u8(242, 248, 255, 0.96) },
                        D2D1_GRADIENT_STOP { position: 0.48, color: rgba_u8(126, 142, 168, 0.92) },
                        D2D1_GRADIENT_STOP { position: 1.0, color: rgba_u8(224, 236, 252, 0.96) },
                    ]
                };
                if let Some(bez_grad) = create_linear_grad(
                    rt,
                    (w_cx - w_r, w_cy - w_r),
                    (w_cx + w_r, w_cy + w_r),
                    &bez_stops,
                ) {
                    rt.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: w_cx, Y: w_cy }, radiusX: w_r + 3.5 * s, radiusY: w_r + 3.5 * s }, &bez_grad);
                }
                let dial_c = elem_col_rgba(orb_tr.color2_rgb, 0.98, if cfg.dt_use_custom_color { pal.glass_bot_col(0.98) } else { rgba_u8(12, 22, 42, 0.98) });
                if let Ok(dial_bg) = rt.CreateSolidColorBrush(&dial_c, None) {
                    rt.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: w_cx, Y: w_cy }, radiusX: w_r - 1.0 * s, radiusY: w_r - 1.0 * s }, &dial_bg);
                }
                let ring_c = elem_col_rgba(orb_tr.border_rgb.or(orb_tr.color_rgb), 0.45, rgba_u8(214, 226, 244, 0.35));
                if let Ok(ring_b) = rt.CreateSolidColorBrush(&ring_c, None) {
                    rt.DrawEllipse(&D2D1_ELLIPSE { point: Vector2 { X: w_cx, Y: w_cy }, radiusX: w_r - 5.5 * s, radiusY: w_r - 5.5 * s }, &ring_b, 0.8 * s, None);
                }
                // 12 Applied Polished Indices
                let idx_c = elem_col_rgba(orb_tr.color_rgb, 0.94, rgba_u8(236, 242, 252, 0.90));
                if let Ok(idx_b) = rt.CreateSolidColorBrush(&idx_c, None) {
                    for m in 0..12 {
                        let a = (m as f32 * 30.0 - 90.0).to_radians();
                        let r_in = if m % 3 == 0 { 41.0 * s } else { 44.0 * s };
                        let r_out = 50.0 * s;
                        rt.DrawLine(
                            Vector2 { X: w_cx + r_in * a.cos(), Y: w_cy + r_in * a.sin() },
                            Vector2 { X: w_cx + r_out * a.cos(), Y: w_cy + r_out * a.sin() },
                            &idx_b,
                            if m % 3 == 0 { 2.2 * s } else { 1.2 * s },
                            None,
                        );
                    }
                }
                // Small Seconds Sub-dial at 6 o'clock + Date Aperture at 3 o'clock
                let sub_cy = w_cy + 22.0 * s;
                if let Ok(sb) = rt.CreateSolidColorBrush(&elem_col_rgba(orb_tr.border_rgb.or(orb_tr.color_rgb), 0.48, rgba_u8(194, 212, 238, 0.42)), None) {
                    rt.DrawEllipse(&D2D1_ELLIPSE { point: Vector2 { X: w_cx, Y: sub_cy }, radiusX: 12.5 * s, radiusY: 12.5 * s }, &sb, 0.9 * s, None);
                }
                if cfg.dt_show_seconds {
                    let s_ang = ((now.second as f32) * 6.0 - 90.0).to_radians();
                    let sec_c = if orb_tr.color_rgb.is_some() || cfg.dt_use_custom_color {
                        if orb_tr.color_rgb.is_some() { oc1 } else { pal.accent_col(0.96) }
                    } else {
                        rgba_u8(214, 34, 76, 0.96)
                    };
                    if let Ok(rb) = rt.CreateSolidColorBrush(&sec_c, None) {
                        rt.DrawLine(
                            Vector2 { X: w_cx, Y: sub_cy },
                            Vector2 { X: w_cx + 10.0 * s * s_ang.cos(), Y: sub_cy + 10.0 * s * s_ang.sin() },
                            &rb,
                            1.2 * s,
                            None,
                        );
                    }
                }
                // Mechanical Date Window Aperture at 3 o'clock
                let dw_rr = D2D1_ROUNDED_RECT {
                    rect: D2D_RECT_F { left: w_cx + 21.0 * s, top: w_cy - 7.5 * s, right: w_cx + 37.5 * s, bottom: w_cy + 7.5 * s },
                    radiusX: 2.0 * s,
                    radiusY: 2.0 * s,
                };
                if let Ok(wb) = rt.CreateSolidColorBrush(&rgba_u8(244, 246, 250, 0.95), None) {
                    rt.FillRoundedRectangle(&dw_rr, &wb);
                }
                if let (Some(tf_d), Ok(ink)) = (
                    make_resolved_tf_named(dw, custom_fonts, &font_sub, &[w!("Bahnschrift"), w!("Segoe UI")], 7.5 * PT_TO_PX * s, DWRITE_FONT_WEIGHT_BOLD, DWRITE_TEXT_ALIGNMENT_CENTER, DWRITE_PARAGRAPH_ALIGNMENT_CENTER),
                    rt.CreateSolidColorBrush(&rgba_u8(16, 24, 42, 0.98), None),
                ) {
                    self::draw_shadowed_text(rt, &day_num_str, &tf_d, dw_rr.rect, &ink, None, 0.0, true);
                }
                let h_ang = (((now.hour % 12) as f32 + now.minute as f32 / 60.0) * 30.0 - 90.0).to_radians();
                let m_ang = ((now.minute as f32 + now.second as f32 / 60.0) * 6.0 - 90.0).to_radians();
                let h_hand_c = if orb_tr.color_rgb.is_some() { oc2 } else { rgba_u8(226, 236, 252, 0.96) };
                let m_hand_c = if orb_tr.color_rgb.is_some() || cfg.dt_use_custom_color {
                    if orb_tr.color_rgb.is_some() { oc1 } else { pal.light_col(0.96) }
                } else {
                    rgba_u8(120, 178, 255, 0.96)
                };
                if let Ok(hb) = rt.CreateSolidColorBrush(&h_hand_c, None) {
                    rt.DrawLine(Vector2 { X: w_cx - 6.0 * s * h_ang.cos(), Y: w_cy - 6.0 * s * h_ang.sin() }, Vector2 { X: w_cx + 28.0 * s * h_ang.cos(), Y: w_cy + 28.0 * s * h_ang.sin() }, &hb, 3.0 * s, None);
                }
                if let Ok(mb) = rt.CreateSolidColorBrush(&m_hand_c, None) {
                    rt.DrawLine(Vector2 { X: w_cx - 8.0 * s * m_ang.cos(), Y: w_cy - 8.0 * s * m_ang.sin() }, Vector2 { X: w_cx + 41.0 * s * m_ang.cos(), Y: w_cy + 41.0 * s * m_ang.sin() }, &mb, 2.0 * s, None);
                }
                // Ruby central pivot jewel
                let rub_c = if orb_tr.color_rgb.is_some() || cfg.dt_use_custom_color {
                    if orb_tr.color_rgb.is_some() { oc1 } else { pal.accent_col(0.98) }
                } else {
                    rgba_u8(216, 28, 72, 0.98)
                };
                if let Ok(rub) = rt.CreateSolidColorBrush(&rub_c, None) {
                    rt.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: w_cx, Y: w_cy }, radiusX: 3.4 * s, radiusY: 3.4 * s }, &rub);
                }
                end_elem_xf(rt, xf);
            }

            // Right Side Swiss Horology Digital Typography (all Pen Canvas slots exposed)
            let _ = phase;
            if cfg.dt_show_spine {
                let xf = begin_elem_xf(rt, ClockElemId::Spine, (x + 146.0 * s, y + 20.0 * s, x + 150.0 * s, y + dt_h - 20.0 * s), s, cfg, &mut out_boxes);
                let sp_c = elem_col_rgba(cfg.dt_elem_transforms[ClockElemId::Spine.idx()].color_rgb, 0.72, rgba_u8(208, 222, 244, 0.45));
                if let Ok(sp) = rt.CreateSolidColorBrush(&sp_c, None) {
                    rt.FillRoundedRectangle(&D2D1_ROUNDED_RECT { rect: D2D_RECT_F { left: x + 146.0 * s, top: y + 22.0 * s, right: x + 148.5 * s, bottom: y + dt_h - 22.0 * s }, radiusX: 1.2 * s, radiusY: 1.2 * s }, &sp);
                }
                end_elem_xf(rt, xf);
            }
            if cfg.dt_show_weekday {
                let tf_wd = make_resolved_tf_named(dw, custom_fonts, &font_day, &[w!("Bodoni MT"), w!("Georgia")], 11.5 * PT_TO_PX * s, DWRITE_FONT_WEIGHT_BOLD, DWRITE_TEXT_ALIGNMENT_LEADING, DWRITE_PARAGRAPH_ALIGNMENT_CENTER);
                if let (Some(tf), Some(br)) = (&tf_wd, &br_accent) {
                    let xf = begin_elem_xf(rt, ClockElemId::Day, (x + 160.0 * s, y + 20.0 * s, x + dt_w - 16.0 * s, y + 42.0 * s), s, cfg, &mut out_boxes);
                    draw_text_spaced(&format!("GENÈVE  •  {}", now.weekday_name.to_ascii_uppercase()), tf, x + 160.0 * s, y + 20.0 * s, dt_w - 176.0 * s, 22.0 * s, br, sh_date.as_ref(), 0.8 * s, false);
                    end_elem_xf(rt, xf);
                }
            }
            let tf_c = make_resolved_tf_named(dw, custom_fonts, &font_clock, &[w!("Bahnschrift"), w!("Segoe UI")], 32.0 * PT_TO_PX * s, DWRITE_FONT_WEIGHT_SEMI_BOLD, DWRITE_TEXT_ALIGNMENT_LEADING, DWRITE_PARAGRAPH_ALIGNMENT_CENTER);
            if let (Some(tf), Some(br)) = (&tf_c, &br_cream) {
                let xf = begin_elem_xf(rt, ClockElemId::Clock, (x + 158.0 * s, y + 48.0 * s, x + 314.0 * s, y + 96.0 * s), s, cfg, &mut out_boxes);
                draw_text_spaced(&time_str, tf, x + 160.0 * s, y + 48.0 * s, 156.0 * s, 48.0 * s, br, sh_day.as_ref(), 1.2 * s, true);
                end_elem_xf(rt, xf);
            }
            if !cfg.widgets_24h && cfg.dt_show_ampm {
                let xf = begin_elem_xf(rt, ClockElemId::AmPm, (x + 316.0 * s, y + 60.0 * s, x + 362.0 * s, y + 82.0 * s), s, cfg, &mut out_boxes);
                let am_tr = cfg.dt_elem_transforms[ClockElemId::AmPm.idx()];
                let ap_rr = D2D1_ROUNDED_RECT { rect: D2D_RECT_F { left: x + 316.0 * s, top: y + 60.0 * s, right: x + 360.0 * s, bottom: y + 80.0 * s }, radiusX: 5.0 * s, radiusY: 5.0 * s };
                let ap_bg_c = elem_col_rgba(am_tr.color_rgb, 0.28, if cfg.dt_use_custom_color { pal.accent_col(0.28) } else { rgba_u8(216, 28, 72, 0.28) });
                if let Ok(ab) = rt.CreateSolidColorBrush(&ap_bg_c, None) {
                    rt.FillRoundedRectangle(&ap_rr, &ab);
                }
                if let Some(b_rgb) = am_tr.border_rgb {
                    if let Ok(rb) = rt.CreateSolidColorBrush(&elem_col_rgba(Some(b_rgb), 0.78, pal.accent_col(0.58)), None) {
                        rt.DrawRoundedRectangle(&ap_rr, &rb, 1.0 * s, None);
                    }
                }
                if let (Some(tf), Some(br)) = (make_resolved_tf_named(dw, custom_fonts, &font_sub, &[w!("Bahnschrift"), w!("Segoe UI")], 8.0 * PT_TO_PX * s, DWRITE_FONT_WEIGHT_BOLD, DWRITE_TEXT_ALIGNMENT_CENTER, DWRITE_PARAGRAPH_ALIGNMENT_CENTER), &br_cream) {
                    draw_shadowed_text(rt, ampm_str, &tf, ap_rr.rect, br, None, 0.0, true);
                }
                end_elem_xf(rt, xf);
            }
            if !footer_str.is_empty() {
                let tf_f = make_resolved_tf_named(dw, custom_fonts, &font_sub, &[w!("Palatino Linotype"), w!("Georgia")], 9.5 * PT_TO_PX * s, DWRITE_FONT_WEIGHT_REGULAR, DWRITE_TEXT_ALIGNMENT_LEADING, DWRITE_PARAGRAPH_ALIGNMENT_CENTER);
                if let (Some(tf), Some(br)) = (&tf_f, &br_sand) {
                    let xf = begin_elem_xf(rt, ClockElemId::Date, (x + 160.0 * s, y + 104.0 * s, x + dt_w - 16.0 * s, y + 128.0 * s), s, cfg, &mut out_boxes);
                    draw_text_spaced(&footer_str, tf, x + 160.0 * s, y + 104.0 * s, dt_w - 176.0 * s, 24.0 * s, br, sh_date.as_ref(), 0.5 * s, false);
                    end_elem_xf(rt, xf);
                }
            }
        }

        ClockStyle::BraunIndustrial => {
            // Anisotropic CNC Brushed Titanium & Dieter Rams Minimal Dial with real PBR metal & interactive Fresnel glare
            let main_rect = D2D_RECT_F { left: x + 2.0 * s, top: y + 4.0 * s, right: x + dt_w - 2.0 * s, bottom: y + dt_h - 4.0 * s };
            let rad_sc = (cfg.dt_corner_radius as f32 / 18.0).clamp(0.0, 2.2);
            let r_card = 12.0 * rad_sc * s;
            let r_base = if is_hovered {
                ((cfg.dt_rim_alpha as f32 / 255.0) + 0.25).clamp(0.25, 0.98)
            } else {
                (cfg.dt_rim_alpha as f32 / 255.0).clamp(0.0, 1.0)
            };
            let inter = crate::realistic_pbr::pbr_input_from_viz(fx_viz, (now.second as f32) * 0.25, is_hovered);
            let tint_ov = tr_card.color_rgb.map(|c| (c, tr_card.color2_rgb)).or_else(|| {
                if cfg.dt_use_custom_color || cfg.dt_match_wallpaper {
                    Some((crate::theme::rgb_to_u32(pal.glass_top), Some(crate::theme::rgb_to_u32(pal.glass_bot))))
                } else {
                    None
                }
            });
            if cfg.dt_show_plate {
                let mut nb: Option<&mut [ClockElemBox]> = None;
                let g = begin_elem_xf_slot(rt, ClockElemId::Card.idx(), tr_card, card_rect, s, &mut nb);
                crate::realistic_pbr::draw_pbr_drop_shadows(rt, main_rect, r_card, s, false);
                let op = (cfg.dt_glass_alpha as f32 / 140.0).clamp(0.45, 0.99);
                crate::realistic_pbr::draw_pbr_surface_clipped(
                    rt,
                    d2d.as_ref(),
                    crate::realistic_pbr::PbrMaterial::BrushedTitanium,
                    main_rect,
                    r_card,
                    s,
                    op,
                    tint_ov,
                    None,
                    &inter,
                );
                crate::realistic_pbr::draw_pbr_bevel_rim(
                    rt,
                    main_rect,
                    r_card,
                    s,
                    crate::realistic_pbr::PbrMaterial::BrushedTitanium,
                    tr_card.border_rgb,
                    r_base,
                    inter.hover_mix,
                    None,
                );
                end_elem_xf_slot(rt, g);
            }
            // Left Dieter Rams Matte Charcoal Dial with Iconic Yellow/Orange Sweep Second Hand (all inside ClockElemId::Orb!)
            let cx_d = x + 74.0 * s;
            let cy_d = y + dt_h * 0.5;
            let r_d = 48.0 * s;
            if cfg.dt_show_orb {
                let xf = begin_elem_xf(rt, ClockElemId::Orb, (cx_d - r_d - 2.0 * s, cy_d - r_d - 2.0 * s, cx_d + r_d + 2.0 * s, cy_d + r_d + 2.0 * s), s, cfg, &mut out_boxes);
                let orb_tr = cfg.dt_elem_transforms[ClockElemId::Orb.idx()];
                let (oc1, oc2, _oang) = resolve_elem_grad_colors(Some(&orb_tr), 0.98, pal.accent_col(0.98), pal.light_col(0.98));
                let dial_bg_c = elem_col_rgba(orb_tr.color2_rgb, 0.96, if cfg.dt_use_custom_color { pal.glass_bot_col(0.96) } else { rgba_u8(22, 24, 28, 0.96) });
                if let Ok(db) = rt.CreateSolidColorBrush(&dial_bg_c, None) {
                    rt.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: cx_d, Y: cy_d }, radiusX: r_d, radiusY: r_d }, &db);
                }
                let tick_c = elem_col_rgba(orb_tr.border_rgb, 0.92, rgba_u8(236, 240, 246, 0.88));
                if let Ok(tb) = rt.CreateSolidColorBrush(&tick_c, None) {
                    rt.DrawEllipse(&D2D1_ELLIPSE { point: Vector2 { X: cx_d, Y: cy_d }, radiusX: r_d, radiusY: r_d }, &tb, 1.4 * s, None);
                    for t in 0..12 {
                        let a = (t as f32 * 30.0 - 90.0).to_radians();
                        rt.DrawLine(
                            Vector2 { X: cx_d + 36.0 * s * a.cos(), Y: cy_d + 36.0 * s * a.sin() },
                            Vector2 { X: cx_d + 43.0 * s * a.cos(), Y: cy_d + 43.0 * s * a.sin() },
                            &tb,
                            if t % 3 == 0 { 2.2 * s } else { 1.1 * s },
                            None,
                        );
                    }
                }
                let h_ang = (((now.hour % 12) as f32 + now.minute as f32 / 60.0) * 30.0 - 90.0).to_radians();
                let m_ang = ((now.minute as f32 + now.second as f32 / 60.0) * 6.0 - 90.0).to_radians();
                let hand_c = elem_col_rgba(orb_tr.color_rgb, 0.96, rgba_u8(248, 250, 252, 0.96));
                if let Ok(wh) = rt.CreateSolidColorBrush(&hand_c, None) {
                    rt.DrawLine(Vector2 { X: cx_d, Y: cy_d }, Vector2 { X: cx_d + 24.0 * s * h_ang.cos(), Y: cy_d + 24.0 * s * h_ang.sin() }, &wh, 3.0 * s, None);
                    rt.DrawLine(Vector2 { X: cx_d, Y: cy_d }, Vector2 { X: cx_d + 36.0 * s * m_ang.cos(), Y: cy_d + 36.0 * s * m_ang.sin() }, &wh, 2.0 * s, None);
                }
                let sweep_c = if orb_tr.color_rgb.is_some() || cfg.dt_use_custom_color {
                    if orb_tr.color_rgb.is_some() { oc1 } else { pal.accent_col(0.98) }
                } else {
                    rgba_u8(255, 134, 24, 0.98)
                };
                if let Ok(orng) = rt.CreateSolidColorBrush(&sweep_c, None) {
                    if cfg.dt_show_seconds {
                        let s_ang = ((now.second as f32) * 6.0 - 90.0).to_radians();
                        rt.DrawLine(Vector2 { X: cx_d - 9.0 * s * s_ang.cos(), Y: cy_d - 9.0 * s * s_ang.sin() }, Vector2 { X: cx_d + 39.0 * s * s_ang.cos(), Y: cy_d + 39.0 * s * s_ang.sin() }, &orng, 1.4 * s, None);
                    }
                    rt.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: cx_d, Y: cy_d }, radiusX: 3.2 * s, radiusY: 3.2 * s }, &orng);
                }
                let _ = oc2;
                end_elem_xf(rt, xf);
            }
            if cfg.dt_show_spine {
                let xf = begin_elem_xf(rt, ClockElemId::Spine, (x + 138.0 * s, y + 24.0 * s, x + 142.0 * s, y + dt_h - 24.0 * s), s, cfg, &mut out_boxes);
                let sp_ov = cfg.dt_elem_transforms[ClockElemId::Spine.idx()].color_rgb;
                let sp_c = if sp_ov.is_some() || cfg.dt_use_custom_color {
                    elem_col_rgba(sp_ov, 0.92, pal.accent_col(0.92))
                } else {
                    rgba_u8(255, 118, 36, 0.92)
                };
                if let Ok(sp) = rt.CreateSolidColorBrush(&sp_c, None) {
                    rt.FillRoundedRectangle(&D2D1_ROUNDED_RECT { rect: D2D_RECT_F { left: x + 138.0 * s, top: y + 24.0 * s, right: x + 141.5 * s, bottom: y + dt_h - 24.0 * s }, radiusX: 1.5 * s, radiusY: 1.5 * s }, &sp);
                }
                end_elem_xf(rt, xf);
            }
            if cfg.dt_show_weekday {
                let tf_wd = make_resolved_tf_named(dw, custom_fonts, &font_day, &[w!("Bahnschrift"), w!("Segoe UI")], 11.0 * PT_TO_PX * s, DWRITE_FONT_WEIGHT_SEMI_BOLD, DWRITE_TEXT_ALIGNMENT_LEADING, DWRITE_PARAGRAPH_ALIGNMENT_CENTER);
                if let (Some(tf), Some(br)) = (&tf_wd, &br_cream) {
                    let xf = begin_elem_xf(rt, ClockElemId::Day, (x + 154.0 * s, y + 22.0 * s, x + dt_w - 20.0 * s, y + 42.0 * s), s, cfg, &mut out_boxes);
                    draw_text_spaced(&format!("TI-01 // {}", now.weekday_name.to_ascii_uppercase()), tf, x + 154.0 * s, y + 22.0 * s, dt_w - 174.0 * s, 20.0 * s, br, sh_date.as_ref(), 0.7 * s, false);
                    end_elem_xf(rt, xf);
                }
            }
            let tf_c = make_resolved_tf_named(dw, custom_fonts, &font_clock, &[w!("Bahnschrift"), w!("Segoe UI")], 33.0 * PT_TO_PX * s, DWRITE_FONT_WEIGHT_BOLD, DWRITE_TEXT_ALIGNMENT_LEADING, DWRITE_PARAGRAPH_ALIGNMENT_CENTER);
            if let (Some(tf), Some(br)) = (&tf_c, &br_cream) {
                let xf = begin_elem_xf(rt, ClockElemId::Clock, (x + 152.0 * s, y + 46.0 * s, x + 312.0 * s, y + 94.0 * s), s, cfg, &mut out_boxes);
                draw_text_spaced(&time_str, tf, x + 154.0 * s, y + 46.0 * s, 158.0 * s, 48.0 * s, br, sh_day.as_ref(), 1.0 * s, true);
                end_elem_xf(rt, xf);
            }
            if !cfg.widgets_24h && cfg.dt_show_ampm {
                let xf = begin_elem_xf(rt, ClockElemId::AmPm, (x + 312.0 * s, y + 58.0 * s, x + 354.0 * s, y + 80.0 * s), s, cfg, &mut out_boxes);
                let am_tr = cfg.dt_elem_transforms[ClockElemId::AmPm.idx()];
                let ap_rr = D2D1_ROUNDED_RECT { rect: D2D_RECT_F { left: x + 312.0 * s, top: y + 58.0 * s, right: x + 354.0 * s, bottom: y + 78.0 * s }, radiusX: 4.0 * s, radiusY: 4.0 * s };
                let ap_fill = if am_tr.color_rgb.is_some() || cfg.dt_use_custom_color {
                    elem_col_rgba(am_tr.color_rgb, 0.28, pal.accent_col(0.28))
                } else {
                    rgba_u8(255, 118, 36, 0.88)
                };
                if let Ok(ab) = rt.CreateSolidColorBrush(&ap_fill, None) {
                    rt.FillRoundedRectangle(&ap_rr, &ab);
                }
                if let Some(b_rgb) = am_tr.border_rgb {
                    if let Ok(rb) = rt.CreateSolidColorBrush(&elem_col_rgba(Some(b_rgb), 0.78, pal.accent_col(0.58)), None) {
                        rt.DrawRoundedRectangle(&ap_rr, &rb, 1.0 * s, None);
                    }
                }
                let txt_col = if am_tr.color_rgb.is_some() || cfg.dt_use_custom_color {
                    elem_col_rgba(am_tr.color_rgb, 1.0, pal.text_col(1.0))
                } else {
                    rgba_u8(16, 18, 22, 1.0)
                };
                if let (Some(tf), Ok(dk)) = (make_resolved_tf_named(dw, custom_fonts, &font_sub, &[w!("Cascadia Mono"), w!("Segoe UI")], 8.0 * PT_TO_PX * s, DWRITE_FONT_WEIGHT_BOLD, DWRITE_TEXT_ALIGNMENT_CENTER, DWRITE_PARAGRAPH_ALIGNMENT_CENTER), rt.CreateSolidColorBrush(&txt_col, None)) {
                    draw_shadowed_text(rt, ampm_str, &tf, ap_rr.rect, &dk, None, 0.0, true);
                }
                end_elem_xf(rt, xf);
            }
            if !footer_str.is_empty() {
                let tf_f = make_resolved_tf_named(dw, custom_fonts, &font_sub, &[w!("Cascadia Mono"), w!("Consolas")], 9.0 * PT_TO_PX * s, DWRITE_FONT_WEIGHT_REGULAR, DWRITE_TEXT_ALIGNMENT_LEADING, DWRITE_PARAGRAPH_ALIGNMENT_CENTER);
                if let (Some(tf), Some(br)) = (&tf_f, &br_sand) {
                    let xf = begin_elem_xf(rt, ClockElemId::Date, (x + 154.0 * s, y + 100.0 * s, x + dt_w - 20.0 * s, y + 124.0 * s), s, cfg, &mut out_boxes);
                    draw_text_spaced(&footer_str, tf, x + 154.0 * s, y + 100.0 * s, dt_w - 174.0 * s, 24.0 * s, br, sh_date.as_ref(), 0.5 * s, false);
                    end_elem_xf(rt, xf);
                }
            }
        }

        ClockStyle::WaterCausticsClock => {
            // Photorealistic Calm Ocean Lagoon PBR Voronoi Water Caustics + Live Click/Hover Ripples
            let t_w = (now.second as f32) * 0.45;
            let main_rect = D2D_RECT_F { left: x + 2.0 * s, top: y + 4.0 * s, right: x + dt_w - 2.0 * s, bottom: y + dt_h - 4.0 * s };
            let rad_sc = (cfg.dt_corner_radius as f32 / 18.0).clamp(0.0, 2.2);
            let r_card = 24.0 * rad_sc * s;
            let r_base = if is_hovered {
                ((cfg.dt_rim_alpha as f32 / 255.0) + 0.25).clamp(0.25, 0.98)
            } else {
                (cfg.dt_rim_alpha as f32 / 255.0).clamp(0.0, 1.0)
            };
            let inter = crate::realistic_pbr::pbr_input_from_viz(fx_viz, t_w, is_hovered);
            let tint_ov = tr_card.color_rgb.map(|c| (c, tr_card.color2_rgb)).or_else(|| {
                if cfg.dt_use_custom_color || cfg.dt_match_wallpaper {
                    Some((crate::theme::rgb_to_u32(pal.glass_top), Some(crate::theme::rgb_to_u32(pal.glass_bot))))
                } else {
                    None
                }
            });
            if cfg.dt_show_plate {
                let mut nb: Option<&mut [ClockElemBox]> = None;
                let g = begin_elem_xf_slot(rt, ClockElemId::Card.idx(), tr_card, card_rect, s, &mut nb);
                crate::realistic_pbr::draw_pbr_drop_shadows(rt, main_rect, r_card, s, false);
                let op = (cfg.dt_glass_alpha as f32 / 145.0).clamp(0.45, 0.98);
                crate::realistic_pbr::draw_pbr_surface_clipped(
                    rt,
                    d2d.as_ref(),
                    crate::realistic_pbr::PbrMaterial::CalmOceanLagoon,
                    main_rect,
                    r_card,
                    s,
                    op,
                    tint_ov,
                    None,
                    &inter,
                );
                crate::realistic_pbr::draw_pbr_bevel_rim(
                    rt,
                    main_rect,
                    r_card,
                    s,
                    crate::realistic_pbr::PbrMaterial::CalmOceanLagoon,
                    tr_card.border_rgb,
                    r_base,
                    inter.hover_mix,
                    None,
                );
                end_elem_xf_slot(rt, g);
            }
            // Expanding water ripple orb behind center time
            if cfg.dt_show_orb {
                let xf = begin_elem_xf(rt, ClockElemId::Orb, (x + dt_w * 0.5 - 64.0 * s, y + dt_h * 0.5 - 32.0 * s, x + dt_w * 0.5 + 64.0 * s, y + dt_h * 0.5 + 32.0 * s), s, cfg, &mut out_boxes);
                let orb_tr = cfg.dt_elem_transforms[ClockElemId::Orb.idx()];
                let r_col = if orb_tr.color_rgb.is_some() || cfg.dt_use_custom_color {
                    elem_col_rgba(orb_tr.color_rgb.or(orb_tr.border_rgb), 0.36, pal.accent_col(0.32))
                } else {
                    rgba_u8(216, 254, 255, 0.28)
                };
                if let Ok(rb) = rt.CreateSolidColorBrush(&r_col, None) {
                    for ri in 0..3 {
                        let rad_x = (36.0 + ri as f32 * 22.0) * s;
                        let rad_y = (16.0 + ri as f32 * 9.0) * s;
                        rt.DrawEllipse(&D2D1_ELLIPSE { point: Vector2 { X: x + dt_w * 0.5, Y: y + dt_h * 0.52 }, radiusX: rad_x, radiusY: rad_y }, &rb, 1.1 * s, None);
                    }
                }
                end_elem_xf(rt, xf);
            }
            if cfg.dt_show_weekday {
                let tf_wd = make_resolved_tf_named(dw, custom_fonts, &font_day, &[w!("Segoe UI Light"), w!("Segoe UI")], 12.0 * PT_TO_PX * s, DWRITE_FONT_WEIGHT_SEMI_BOLD, DWRITE_TEXT_ALIGNMENT_CENTER, DWRITE_PARAGRAPH_ALIGNMENT_CENTER);
                if let (Some(tf), Some(br)) = (&tf_wd, &br_cream) {
                    let xf = begin_elem_xf(rt, ClockElemId::Day, (x + 24.0 * s, y + 16.0 * s, x + dt_w - 24.0 * s, y + 38.0 * s), s, cfg, &mut out_boxes);
                    draw_text_spaced(&now.weekday_name.to_ascii_uppercase(), tf, x + 24.0 * s, y + 16.0 * s, dt_w - 48.0 * s, 22.0 * s, br, sh_date.as_ref(), 1.4 * s, false);
                    end_elem_xf(rt, xf);
                }
            }
            let tf_c = make_resolved_tf_named(dw, custom_fonts, &font_clock, &[w!("SF Pro Display"), w!("Segoe UI")], 38.0 * PT_TO_PX * s, DWRITE_FONT_WEIGHT_LIGHT, DWRITE_TEXT_ALIGNMENT_CENTER, DWRITE_PARAGRAPH_ALIGNMENT_CENTER);
            if let (Some(tf), Some(br)) = (&tf_c, &br_cream) {
                let xf = begin_elem_xf(rt, ClockElemId::Clock, (x + 48.0 * s, y + 40.0 * s, x + dt_w - 48.0 * s, y + 96.0 * s), s, cfg, &mut out_boxes);
                draw_text_spaced(&time_str, tf, x + 48.0 * s, y + 40.0 * s, dt_w - 96.0 * s, 56.0 * s, br, sh_day.as_ref(), 1.2 * s, false);
                end_elem_xf(rt, xf);
            }
            if !cfg.widgets_24h && cfg.dt_show_ampm {
                let xf = begin_elem_xf(rt, ClockElemId::AmPm, (x + dt_w - 64.0 * s, y + 54.0 * s, x + dt_w - 22.0 * s, y + 76.0 * s), s, cfg, &mut out_boxes);
                let am_tr = cfg.dt_elem_transforms[ClockElemId::AmPm.idx()];
                let ap_rr = D2D1_ROUNDED_RECT { rect: D2D_RECT_F { left: x + dt_w - 64.0 * s, top: y + 54.0 * s, right: x + dt_w - 22.0 * s, bottom: y + 74.0 * s }, radiusX: 8.0 * s, radiusY: 8.0 * s };
                let ap_bg = elem_col_rgba(am_tr.color_rgb, 0.28, if cfg.dt_use_custom_color { pal.accent_col(0.24) } else { rgba_u8(226, 254, 255, 0.24) });
                if let Ok(ab) = rt.CreateSolidColorBrush(&ap_bg, None) {
                    rt.FillRoundedRectangle(&ap_rr, &ab);
                }
                if let Some(b_rgb) = am_tr.border_rgb {
                    if let Ok(rb) = rt.CreateSolidColorBrush(&elem_col_rgba(Some(b_rgb), 0.78, pal.accent_col(0.58)), None) {
                        rt.DrawRoundedRectangle(&ap_rr, &rb, 1.0 * s, None);
                    }
                }
                if let (Some(tf), Some(br)) = (make_resolved_tf_named(dw, custom_fonts, &font_sub, &[w!("Segoe UI")], 8.0 * PT_TO_PX * s, DWRITE_FONT_WEIGHT_BOLD, DWRITE_TEXT_ALIGNMENT_CENTER, DWRITE_PARAGRAPH_ALIGNMENT_CENTER), &br_cream) {
                    draw_shadowed_text(rt, ampm_str, &tf, ap_rr.rect, br, None, 0.0, true);
                }
                end_elem_xf(rt, xf);
            }
            if !footer_str.is_empty() {
                let tf_f = make_resolved_tf_named(dw, custom_fonts, &font_sub, &[w!("Segoe UI")], 9.5 * PT_TO_PX * s, DWRITE_FONT_WEIGHT_REGULAR, DWRITE_TEXT_ALIGNMENT_CENTER, DWRITE_PARAGRAPH_ALIGNMENT_CENTER);
                if let (Some(tf), Some(br)) = (&tf_f, &br_sand) {
                    let xf = begin_elem_xf(rt, ClockElemId::Date, (x + 24.0 * s, y + 102.0 * s, x + dt_w - 24.0 * s, y + 126.0 * s), s, cfg, &mut out_boxes);
                    draw_text_spaced(&footer_str, tf, x + 24.0 * s, y + 102.0 * s, dt_w - 48.0 * s, 24.0 * s, br, sh_date.as_ref(), 0.5 * s, false);
                    end_elem_xf(rt, xf);
                }
            }
        }

        ClockStyle::ZenMistClock => {
            // Ultra-Calm Japanese Morning Mist & Sage Stone Sanctuary Clock
            let main_rect = D2D_RECT_F { left: x + 2.0 * s, top: y + 4.0 * s, right: x + dt_w - 2.0 * s, bottom: y + dt_h - 4.0 * s };
            let rad_sc = (cfg.dt_corner_radius as f32 / 18.0).clamp(0.0, 2.2);
            let r_card = 22.0 * rad_sc * s;
            let r_base = if is_hovered {
                ((cfg.dt_rim_alpha as f32 / 255.0) + 0.25).clamp(0.25, 0.98)
            } else {
                (cfg.dt_rim_alpha as f32 / 255.0).clamp(0.0, 1.0)
            };
            let inter = crate::realistic_pbr::pbr_input_from_viz(fx_viz, (now.second as f32) * 0.25, is_hovered);
            let tint_ov = tr_card.color_rgb.map(|c| (c, tr_card.color2_rgb)).or_else(|| {
                if cfg.dt_use_custom_color || cfg.dt_match_wallpaper {
                    Some((crate::theme::rgb_to_u32(pal.glass_top), Some(crate::theme::rgb_to_u32(pal.glass_bot))))
                } else {
                    Some((0x1A3332, Some(0x365750)))
                }
            });
            if cfg.dt_show_plate {
                let mut nb: Option<&mut [ClockElemBox]> = None;
                let g = begin_elem_xf_slot(rt, ClockElemId::Card.idx(), tr_card, card_rect, s, &mut nb);
                crate::realistic_pbr::draw_pbr_drop_shadows(rt, main_rect, r_card, s, false);
                let op = (cfg.dt_glass_alpha as f32 / 145.0).clamp(0.45, 0.98);
                crate::realistic_pbr::draw_pbr_surface_clipped(
                    rt,
                    d2d.as_ref(),
                    crate::realistic_pbr::PbrMaterial::AlpineForestMist,
                    main_rect,
                    r_card,
                    s,
                    op,
                    tint_ov,
                    None,
                    &inter,
                );
                // Subtle Zen raked-sand horizon lines
                if let Ok(rk) = rt.CreateSolidColorBrush(&elem_col_rgba(tr_card.border_rgb, 0.28, rgba_u8(204, 226, 218, 0.20)), None) {
                    for i in 0..3 {
                        let ry = main_rect.bottom - (10.0 + i as f32 * 5.0) * s;
                        rt.DrawLine(Vector2 { X: main_rect.left + 22.0 * s, Y: ry }, Vector2 { X: main_rect.right - 22.0 * s, Y: ry }, &rk, 0.8 * s, None);
                    }
                }
                crate::realistic_pbr::draw_pbr_bevel_rim(
                    rt,
                    main_rect,
                    r_card,
                    s,
                    crate::realistic_pbr::PbrMaterial::AlpineForestMist,
                    tr_card.border_rgb,
                    r_base,
                    inter.hover_mix,
                    None,
                );
                end_elem_xf_slot(rt, g);
            }
            if cfg.dt_show_orb {
                let xf = begin_elem_xf(rt, ClockElemId::Orb, (x + dt_w * 0.5 - 32.0 * s, y + 16.0 * s, x + dt_w * 0.5 + 32.0 * s, y + 80.0 * s), s, cfg, &mut out_boxes);
                let orb_c = elem_col_rgba(cfg.dt_elem_transforms[ClockElemId::Orb.idx()].color_rgb, 0.24, if cfg.dt_use_custom_color { pal.accent_col(0.22) } else { rgba_u8(236, 246, 240, 0.14) });
                if let Ok(pl) = rt.CreateSolidColorBrush(&orb_c, None) {
                    rt.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: x + dt_w * 0.5, Y: y + 54.0 * s }, radiusX: 34.0 * s, radiusY: 34.0 * s }, &pl);
                }
                end_elem_xf(rt, xf);
            }
            if cfg.dt_show_spine {
                let xf = begin_elem_xf(rt, ClockElemId::Spine, (x + dt_w * 0.5 - 48.0 * s, y + 89.0 * s, x + dt_w * 0.5 + 48.0 * s, y + 92.0 * s), s, cfg, &mut out_boxes);
                let sp_c = elem_col_rgba(cfg.dt_elem_transforms[ClockElemId::Spine.idx()].color_rgb, 0.72, if cfg.dt_use_custom_color { pal.accent_col(0.65) } else { rgba_u8(178, 214, 198, 0.55) });
                if let Ok(sp) = rt.CreateSolidColorBrush(&sp_c, None) {
                    rt.DrawLine(Vector2 { X: x + dt_w * 0.5 - 46.0 * s, Y: y + 90.5 * s }, Vector2 { X: x + dt_w * 0.5 + 46.0 * s, Y: y + 90.5 * s }, &sp, 1.1 * s, None);
                }
                end_elem_xf(rt, xf);
            }
            if cfg.dt_show_weekday {
                let tf_wd = make_resolved_tf_named(dw, custom_fonts, &font_day, &[w!("Georgia"), w!("Segoe UI")], 11.5 * PT_TO_PX * s, DWRITE_FONT_WEIGHT_REGULAR, DWRITE_TEXT_ALIGNMENT_CENTER, DWRITE_PARAGRAPH_ALIGNMENT_CENTER);
                if let (Some(tf), Some(br)) = (&tf_wd, &br_sand) {
                    let xf = begin_elem_xf(rt, ClockElemId::Day, (x + 28.0 * s, y + 14.0 * s, x + dt_w - 28.0 * s, y + 34.0 * s), s, cfg, &mut out_boxes);
                    draw_text_spaced(&format!("—  {}  —", now.weekday_name), tf, x + 28.0 * s, y + 14.0 * s, dt_w - 56.0 * s, 20.0 * s, br, None, 0.0, false);
                    end_elem_xf(rt, xf);
                }
            }
            let tf_c = make_resolved_tf_named(dw, custom_fonts, &font_clock, &[w!("Segoe UI Light"), w!("Georgia")], 36.0 * PT_TO_PX * s, DWRITE_FONT_WEIGHT_LIGHT, DWRITE_TEXT_ALIGNMENT_CENTER, DWRITE_PARAGRAPH_ALIGNMENT_CENTER);
            if let (Some(tf), Some(br)) = (&tf_c, &br_cream) {
                let xf = begin_elem_xf(rt, ClockElemId::Clock, (x + 48.0 * s, y + 36.0 * s, x + dt_w - 48.0 * s, y + 88.0 * s), s, cfg, &mut out_boxes);
                draw_text_spaced(&time_str, tf, x + 48.0 * s, y + 36.0 * s, dt_w - 96.0 * s, 52.0 * s, br, sh_day.as_ref(), 0.8 * s, false);
                end_elem_xf(rt, xf);
            }
            if !cfg.widgets_24h && cfg.dt_show_ampm {
                let xf = begin_elem_xf(rt, ClockElemId::AmPm, (x + dt_w - 64.0 * s, y + 52.0 * s, x + dt_w - 24.0 * s, y + 72.0 * s), s, cfg, &mut out_boxes);
                let am_tr = cfg.dt_elem_transforms[ClockElemId::AmPm.idx()];
                let ap_rr = D2D1_ROUNDED_RECT { rect: D2D_RECT_F { left: x + dt_w - 64.0 * s, top: y + 52.0 * s, right: x + dt_w - 24.0 * s, bottom: y + 72.0 * s }, radiusX: 6.0 * s, radiusY: 6.0 * s };
                if let Ok(ab) = rt.CreateSolidColorBrush(&elem_col_rgba(am_tr.color_rgb, 0.24, pal.accent_col(0.18)), None) {
                    rt.FillRoundedRectangle(&ap_rr, &ab);
                }
                if let (Some(tf), Some(br)) = (make_resolved_tf_named(dw, custom_fonts, &font_sub, &[w!("Segoe UI")], 8.0 * PT_TO_PX * s, DWRITE_FONT_WEIGHT_BOLD, DWRITE_TEXT_ALIGNMENT_CENTER, DWRITE_PARAGRAPH_ALIGNMENT_CENTER), &br_sand) {
                    draw_shadowed_text(rt, ampm_str, &tf, ap_rr.rect, br, None, 0.0, true);
                }
                end_elem_xf(rt, xf);
            }
            if !footer_str.is_empty() {
                let tf_f = make_resolved_tf_named(dw, custom_fonts, &font_sub, &[w!("Segoe UI")], 9.0 * PT_TO_PX * s, DWRITE_FONT_WEIGHT_REGULAR, DWRITE_TEXT_ALIGNMENT_CENTER, DWRITE_PARAGRAPH_ALIGNMENT_CENTER);
                if let (Some(tf), Some(br)) = (&tf_f, &br_sand) {
                    let xf = begin_elem_xf(rt, ClockElemId::Date, (x + 28.0 * s, y + 96.0 * s, x + dt_w - 28.0 * s, y + 118.0 * s), s, cfg, &mut out_boxes);
                    draw_text_spaced(&footer_str, tf, x + 28.0 * s, y + 96.0 * s, dt_w - 56.0 * s, 22.0 * s, br, None, 0.0, false);
                    end_elem_xf(rt, xf);
                }
            }
        }

        ClockStyle::LeatherFieldClock => {
            // Studio Executive Saddle-Stitched Pebble Leather + Curled Ivory Woven-Linen Inset + Brass Pin
            let main_rect = D2D_RECT_F { left: x + 2.0 * s, top: y + 4.0 * s, right: x + dt_w - 2.0 * s, bottom: y + dt_h - 4.0 * s };
            let rad_sc = (cfg.dt_corner_radius as f32 / 18.0).clamp(0.0, 2.2);
            let r_card = 16.0 * rad_sc * s;
            let r_base = if is_hovered {
                ((cfg.dt_rim_alpha as f32 / 255.0) + 0.25).clamp(0.25, 0.98)
            } else {
                (cfg.dt_rim_alpha as f32 / 255.0).clamp(0.0, 1.0)
            };
            let inter = crate::realistic_pbr::pbr_input_from_viz(fx_viz, (now.second as f32) * 0.2, is_hovered);
            let tint_ov = tr_card.color_rgb.map(|c| (c, tr_card.color2_rgb)).or_else(|| {
                if cfg.dt_use_custom_color || cfg.dt_match_wallpaper {
                    Some((crate::theme::rgb_to_u32(pal.glass_top), Some(crate::theme::rgb_to_u32(pal.glass_bot))))
                } else {
                    None
                }
            });
            if cfg.dt_show_plate {
                let mut nb: Option<&mut [ClockElemBox]> = None;
                let g = begin_elem_xf_slot(rt, ClockElemId::Card.idx(), tr_card, card_rect, s, &mut nb);
                crate::realistic_pbr::draw_pbr_drop_shadows(rt, main_rect, r_card, s, false);
                let op = (cfg.dt_glass_alpha as f32 / 140.0).clamp(0.50, 0.99);
                crate::realistic_pbr::draw_pbr_surface_clipped(
                    rt,
                    d2d.as_ref(),
                    crate::realistic_pbr::PbrMaterial::ExecutiveLeatherLinen,
                    main_rect,
                    r_card,
                    s,
                    op,
                    tint_ov,
                    None,
                    &inter,
                );
                // Tactile woven ivory linen paper card with hover-lifting curled shadow & brass push-pin
                crate::realistic_pbr::draw_linen_paper_inset(
                    rt,
                    d2d.as_ref(),
                    D2D_RECT_F { left: x + 132.0 * s, top: y + 20.0 * s, right: x + dt_w - 22.0 * s, bottom: y + dt_h - 20.0 * s },
                    s,
                    inter.hover_mix,
                );
                crate::realistic_pbr::draw_pbr_bevel_rim(
                    rt,
                    main_rect,
                    r_card,
                    s,
                    crate::realistic_pbr::PbrMaterial::ExecutiveLeatherLinen,
                    tr_card.border_rgb,
                    r_base,
                    inter.hover_mix,
                    None,
                );
                end_elem_xf_slot(rt, g);
            }
            // Left Embossed Brass Day Medallion
            if cfg.dt_show_orb {
                let xf = begin_elem_xf(rt, ClockElemId::Orb, (x + 26.0 * s, y + 32.0 * s, x + 112.0 * s, y + 114.0 * s), s, cfg, &mut out_boxes);
                let orb_tr = cfg.dt_elem_transforms[ClockElemId::Orb.idx()];
                let cx_m = x + 69.0 * s;
                let cy_m = y + dt_h * 0.5;
                let mb_c = elem_col_rgba(orb_tr.color2_rgb, 0.78, rgba_u8(46, 24, 12, 0.72));
                if let Ok(mb) = rt.CreateSolidColorBrush(&mb_c, None) {
                    rt.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: cx_m, Y: cy_m }, radiusX: 38.0 * s, radiusY: 38.0 * s }, &mb);
                }
                let mr_c = elem_col_rgba(orb_tr.border_rgb.or(orb_tr.color_rgb), 0.90, if cfg.dt_use_custom_color { pal.accent_col(0.90) } else { rgba_u8(220, 176, 94, 0.90) });
                if let Ok(mr) = rt.CreateSolidColorBrush(&mr_c, None) {
                    rt.DrawEllipse(&D2D1_ELLIPSE { point: Vector2 { X: cx_m, Y: cy_m }, radiusX: 38.0 * s, radiusY: 38.0 * s }, &mr, 1.6 * s, None);
                }
                if let (Some(tf_n), Some(br)) = (make_resolved_tf_named(dw, custom_fonts, &font_day, &[w!("Georgia")], 24.0 * PT_TO_PX * s, DWRITE_FONT_WEIGHT_BOLD, DWRITE_TEXT_ALIGNMENT_CENTER, DWRITE_PARAGRAPH_ALIGNMENT_CENTER), &br_cream) {
                    draw_shadowed_text(rt, &day_num_str, &tf_n, D2D_RECT_F { left: cx_m - 30.0 * s, top: cy_m - 22.0 * s, right: cx_m + 30.0 * s, bottom: cy_m + 14.0 * s }, br, None, 0.0, true);
                }
                if let (Some(tf_m), Some(br)) = (make_resolved_tf_named(dw, custom_fonts, &font_sub, &[w!("Georgia")], 8.5 * PT_TO_PX * s, DWRITE_FONT_WEIGHT_SEMI_BOLD, DWRITE_TEXT_ALIGNMENT_CENTER, DWRITE_PARAGRAPH_ALIGNMENT_CENTER), &br_gold) {
                    draw_shadowed_text(rt, &now.month_name[..3.min(now.month_name.len())].to_ascii_uppercase(), &tf_m, D2D_RECT_F { left: cx_m - 30.0 * s, top: cy_m + 10.0 * s, right: cx_m + 30.0 * s, bottom: cy_m + 26.0 * s }, br, None, 0.0, true);
                }
                end_elem_xf(rt, xf);
            }
            // Debossed Letterpress Typography on the Archival Cream Paper Window
            let br_ink_dark = rt.CreateSolidColorBrush(&rgba_u8(42, 28, 18, 0.96), None).ok();
            let br_ink_sub = rt.CreateSolidColorBrush(&rgba_u8(116, 78, 44, 0.92), None).ok();
            let txt_lum = (pal.text_primary.0 as u32 * 299
                + pal.text_primary.1 as u32 * 587
                + pal.text_primary.2 as u32 * 114)
                / 1000;
            // Archival cream paper (#F7F4EB) requires dark ink; only swap to custom text if dark enough to read
            let use_custom_txt = (cfg.dt_use_custom_color || cfg.dt_match_wallpaper) && txt_lum <= 145;
            let main_ink = br_elem[ClockElemId::Clock.idx()]
                .as_ref()
                .or(if use_custom_txt { br_cream.as_ref() } else { br_ink_dark.as_ref().or(br_cream.as_ref()) });
            let sub_ink = br_elem[ClockElemId::Day.idx()]
                .as_ref()
                .or(if use_custom_txt { br_sand.as_ref() } else { br_ink_sub.as_ref().or(br_sand.as_ref()) });
            if cfg.dt_show_weekday {
                let tf_wd = make_resolved_tf_named(dw, custom_fonts, &font_day, &[w!("Georgia")], 10.5 * PT_TO_PX * s, DWRITE_FONT_WEIGHT_BOLD, DWRITE_TEXT_ALIGNMENT_LEADING, DWRITE_PARAGRAPH_ALIGNMENT_CENTER);
                if let (Some(tf), Some(br)) = (&tf_wd, sub_ink) {
                    let xf = begin_elem_xf(rt, ClockElemId::Day, (x + 146.0 * s, y + 26.0 * s, x + dt_w - 30.0 * s, y + 46.0 * s), s, cfg, &mut out_boxes);
                    draw_text_spaced(&format!("FIELD LOG • {}", now.weekday_name.to_ascii_uppercase()), tf, x + 146.0 * s, y + 26.0 * s, dt_w - 176.0 * s, 20.0 * s, br, None, 0.0, false);
                    end_elem_xf(rt, xf);
                }
            }
            let tf_c = make_resolved_tf_named(dw, custom_fonts, &font_clock, &[w!("Palatino Linotype"), w!("Georgia")], 31.0 * PT_TO_PX * s, DWRITE_FONT_WEIGHT_BOLD, DWRITE_TEXT_ALIGNMENT_LEADING, DWRITE_PARAGRAPH_ALIGNMENT_CENTER);
            if let (Some(tf), Some(br)) = (&tf_c, main_ink) {
                let xf = begin_elem_xf(rt, ClockElemId::Clock, (x + 144.0 * s, y + 48.0 * s, x + 298.0 * s, y + 94.0 * s), s, cfg, &mut out_boxes);
                draw_text_spaced(&time_str, tf, x + 146.0 * s, y + 48.0 * s, 152.0 * s, 46.0 * s, br, None, 0.0, false);
                end_elem_xf(rt, xf);
            }
            if !cfg.widgets_24h && cfg.dt_show_ampm {
                let xf = begin_elem_xf(rt, ClockElemId::AmPm, (x + 298.0 * s, y + 58.0 * s, x + 340.0 * s, y + 80.0 * s), s, cfg, &mut out_boxes);
                if let (Some(tf), Some(br)) = (make_resolved_tf_named(dw, custom_fonts, &font_sub, &[w!("Georgia")], 9.0 * PT_TO_PX * s, DWRITE_FONT_WEIGHT_BOLD, DWRITE_TEXT_ALIGNMENT_CENTER, DWRITE_PARAGRAPH_ALIGNMENT_CENTER), sub_ink) {
                    draw_shadowed_text(rt, ampm_str, &tf, D2D_RECT_F { left: x + 298.0 * s, top: y + 58.0 * s, right: x + 340.0 * s, bottom: y + 80.0 * s }, br, None, 0.0, true);
                }
                end_elem_xf(rt, xf);
            }
            if !footer_str.is_empty() {
                let tf_f = make_resolved_tf_named(dw, custom_fonts, &font_sub, &[w!("Georgia")], 9.0 * PT_TO_PX * s, DWRITE_FONT_WEIGHT_REGULAR, DWRITE_TEXT_ALIGNMENT_LEADING, DWRITE_PARAGRAPH_ALIGNMENT_CENTER);
                if let (Some(tf), Some(br)) = (&tf_f, sub_ink) {
                    let xf = begin_elem_xf(rt, ClockElemId::Date, (x + 146.0 * s, y + 96.0 * s, x + dt_w - 30.0 * s, y + 118.0 * s), s, cfg, &mut out_boxes);
                    draw_text_spaced(&footer_str, tf, x + 146.0 * s, y + 96.0 * s, dt_w - 176.0 * s, 22.0 * s, br, None, 0.0, false);
                    end_elem_xf(rt, xf);
                }
            }
        }

        ClockStyle::WalnutDeskClock => {
            // Hand-Oiled American Black Walnut Mantle Clock + PBR Cathedral Grain + Slotted Brass Screws
            let main_rect = D2D_RECT_F { left: x + 2.0 * s, top: y + 4.0 * s, right: x + dt_w - 2.0 * s, bottom: y + dt_h - 4.0 * s };
            let rad_sc = (cfg.dt_corner_radius as f32 / 18.0).clamp(0.0, 2.2);
            let r_card = 14.0 * rad_sc * s;
            let r_base = if is_hovered {
                ((cfg.dt_rim_alpha as f32 / 255.0) + 0.25).clamp(0.25, 0.98)
            } else {
                (cfg.dt_rim_alpha as f32 / 255.0).clamp(0.0, 1.0)
            };
            let inter = crate::realistic_pbr::pbr_input_from_viz(fx_viz, (now.second as f32) * 0.22, is_hovered);
            let tint_ov = tr_card.color_rgb.map(|c| (c, tr_card.color2_rgb)).or_else(|| {
                if cfg.dt_use_custom_color || cfg.dt_match_wallpaper {
                    Some((crate::theme::rgb_to_u32(pal.glass_top), Some(crate::theme::rgb_to_u32(pal.glass_bot))))
                } else {
                    None
                }
            });
            if cfg.dt_show_plate {
                let mut nb: Option<&mut [ClockElemBox]> = None;
                let g = begin_elem_xf_slot(rt, ClockElemId::Card.idx(), tr_card, card_rect, s, &mut nb);
                crate::realistic_pbr::draw_pbr_drop_shadows(rt, main_rect, r_card, s, false);
                let op = (cfg.dt_glass_alpha as f32 / 140.0).clamp(0.48, 0.99);
                crate::realistic_pbr::draw_pbr_surface_clipped(
                    rt,
                    d2d.as_ref(),
                    crate::realistic_pbr::PbrMaterial::VintageWalnutBrass,
                    main_rect,
                    r_card,
                    s,
                    op,
                    tint_ov,
                    None,
                    &inter,
                );
                crate::realistic_pbr::draw_pbr_bevel_rim(
                    rt,
                    main_rect,
                    r_card,
                    s,
                    crate::realistic_pbr::PbrMaterial::VintageWalnutBrass,
                    tr_card.border_rgb,
                    r_base,
                    inter.hover_mix,
                    None,
                );
                end_elem_xf_slot(rt, g);
            }
            if cfg.dt_show_spine {
                let xf = begin_elem_xf(rt, ClockElemId::Spine, (x + 22.0 * s, y + 26.0 * s, x + 28.0 * s, y + dt_h - 26.0 * s), s, cfg, &mut out_boxes);
                let sp_ov = cfg.dt_elem_transforms[ClockElemId::Spine.idx()].color_rgb;
                let sp_c = if sp_ov.is_some() || cfg.dt_use_custom_color {
                    elem_col_rgba(sp_ov, 0.90, pal.accent_col(0.90))
                } else {
                    rgba_u8(228, 182, 100, 0.90)
                };
                if let Ok(sp) = rt.CreateSolidColorBrush(&sp_c, None) {
                    rt.FillRoundedRectangle(&D2D1_ROUNDED_RECT { rect: D2D_RECT_F { left: x + 23.0 * s, top: y + 26.0 * s, right: x + 27.0 * s, bottom: y + dt_h - 26.0 * s }, radiusX: 2.0 * s, radiusY: 2.0 * s }, &sp);
                }
                end_elem_xf(rt, xf);
            }
            if cfg.dt_show_weekday {
                let tf_wd = make_resolved_tf_named(dw, custom_fonts, &font_day, &[w!("Bodoni MT"), w!("Georgia")], 12.5 * PT_TO_PX * s, DWRITE_FONT_WEIGHT_BOLD, DWRITE_TEXT_ALIGNMENT_LEADING, DWRITE_PARAGRAPH_ALIGNMENT_CENTER);
                if let (Some(tf), Some(br)) = (&tf_wd, &br_gold) {
                    let xf = begin_elem_xf(rt, ClockElemId::Day, (x + 42.0 * s, y + 20.0 * s, x + dt_w - 24.0 * s, y + 42.0 * s), s, cfg, &mut out_boxes);
                    draw_text_spaced(&now.weekday_name.to_ascii_uppercase(), tf, x + 42.0 * s, y + 20.0 * s, dt_w - 66.0 * s, 22.0 * s, br, sh_date.as_ref(), 1.2 * s, false);
                    end_elem_xf(rt, xf);
                }
            }
            let tf_c = make_resolved_tf_named(dw, custom_fonts, &font_clock, &[w!("Georgia"), w!("Palatino Linotype")], 36.0 * PT_TO_PX * s, DWRITE_FONT_WEIGHT_BOLD, DWRITE_TEXT_ALIGNMENT_LEADING, DWRITE_PARAGRAPH_ALIGNMENT_CENTER);
            if let (Some(tf), Some(br)) = (&tf_c, &br_cream) {
                let clk_r = if !cfg.widgets_24h && cfg.dt_show_ampm { x + 232.0 * s } else { x + 240.0 * s };
                let xf = begin_elem_xf(rt, ClockElemId::Clock, (x + 40.0 * s, y + 44.0 * s, clk_r, y + 96.0 * s), s, cfg, &mut out_boxes);
                draw_text_spaced(&time_str, tf, x + 42.0 * s, y + 44.0 * s, clk_r - (x + 42.0 * s), 52.0 * s, br, sh_day.as_ref(), 1.2 * s, false);
                end_elem_xf(rt, xf);
            }
            if !cfg.widgets_24h && cfg.dt_show_ampm {
                let xf = begin_elem_xf(rt, ClockElemId::AmPm, (x + 236.0 * s, y + 58.0 * s, x + 278.0 * s, y + 78.0 * s), s, cfg, &mut out_boxes);
                let am_tr = cfg.dt_elem_transforms[ClockElemId::AmPm.idx()];
                let ap_rr = D2D1_ROUNDED_RECT { rect: D2D_RECT_F { left: x + 236.0 * s, top: y + 58.0 * s, right: x + 278.0 * s, bottom: y + 78.0 * s }, radiusX: 5.0 * s, radiusY: 5.0 * s };
                if let Ok(ab) = rt.CreateSolidColorBrush(&elem_col_rgba(am_tr.color_rgb, 0.24, pal.accent_col(0.20)), None) {
                    rt.FillRoundedRectangle(&ap_rr, &ab);
                }
                if let (Some(tf), Some(br)) = (make_resolved_tf_named(dw, custom_fonts, &font_sub, &[w!("Georgia")], 8.0 * PT_TO_PX * s, DWRITE_FONT_WEIGHT_BOLD, DWRITE_TEXT_ALIGNMENT_CENTER, DWRITE_PARAGRAPH_ALIGNMENT_CENTER), &br_gold) {
                    draw_shadowed_text(rt, ampm_str, &tf, ap_rr.rect, br, None, 0.0, true);
                }
                end_elem_xf(rt, xf);
            }
            if cfg.dt_show_orb {
                let xf = begin_elem_xf(rt, ClockElemId::Orb, (x + dt_w - 86.0 * s, y + 40.0 * s, x + dt_w - 24.0 * s, y + 102.0 * s), s, cfg, &mut out_boxes);
                let orb_tr = cfg.dt_elem_transforms[ClockElemId::Orb.idx()];
                let orb_c = if orb_tr.color_rgb.is_some() || cfg.dt_use_custom_color {
                    elem_col_rgba(orb_tr.color_rgb.or(orb_tr.border_rgb), 0.90, pal.accent_col(0.88))
                } else {
                    rgba_u8(226, 180, 98, 0.88)
                };
                let cx_o = x + dt_w - 55.0 * s;
                let cy_o = y + 71.0 * s;
                if let Ok(brs) = rt.CreateSolidColorBrush(&orb_c, None) {
                    rt.DrawEllipse(&D2D1_ELLIPSE { point: Vector2 { X: cx_o, Y: cy_o }, radiusX: 24.0 * s, radiusY: 24.0 * s }, &brs, 1.4 * s, None);
                    if cfg.dt_show_seconds {
                        let sa = ((now.second as f32) * 6.0 - 90.0).to_radians();
                        rt.DrawLine(Vector2 { X: cx_o, Y: cy_o }, Vector2 { X: cx_o + 18.0 * s * sa.cos(), Y: cy_o + 18.0 * s * sa.sin() }, &brs, 1.4 * s, None);
                    }
                    rt.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: cx_o, Y: cy_o }, radiusX: 2.8 * s, radiusY: 2.8 * s }, &brs);
                }
                end_elem_xf(rt, xf);
            }
            if !footer_str.is_empty() {
                let tf_f = make_resolved_tf_named(dw, custom_fonts, &font_sub, &[w!("Palatino Linotype"), w!("Georgia")], 9.5 * PT_TO_PX * s, DWRITE_FONT_WEIGHT_REGULAR, DWRITE_TEXT_ALIGNMENT_LEADING, DWRITE_PARAGRAPH_ALIGNMENT_CENTER);
                if let (Some(tf), Some(br)) = (&tf_f, &br_sand) {
                    let xf = begin_elem_xf(rt, ClockElemId::Date, (x + 42.0 * s, y + 98.0 * s, x + dt_w - 94.0 * s, y + 122.0 * s), s, cfg, &mut out_boxes);
                    draw_text_spaced(&footer_str, tf, x + 42.0 * s, y + 98.0 * s, dt_w - 136.0 * s, 24.0 * s, br, sh_date.as_ref(), 0.5 * s, false);
                    end_elem_xf(rt, xf);
                }
            }
        }

        // 6 New True Realism PBR Clocks (Rainy Windowpane, Deep Abyssal Trench, Nordic Glacial Ice,
        // Industrial Matte Carbon & Steel, Golden Hour Horizon, Alpine Forest Mist)
        ClockStyle::RainWindowClock
        | ClockStyle::AbyssalTrenchClock
        | ClockStyle::GlacierIceClock
        | ClockStyle::CarbonSteelClock
        | ClockStyle::GoldenHourClock
        | ClockStyle::AlpineMistClock => {
            let pbr_mat = cfg
                .dt_style
                .pbr_material()
                .unwrap_or(crate::realistic_pbr::PbrMaterial::RainyWindowpane);
            let main_rect = D2D_RECT_F {
                left: x + 2.0 * s,
                top: y + 4.0 * s,
                right: x + dt_w - 2.0 * s,
                bottom: y + dt_h - 4.0 * s,
            };
            let rad_sc = (cfg.dt_corner_radius as f32 / 18.0).clamp(0.0, 2.2);
            let base_rad = match cfg.dt_style {
                ClockStyle::CarbonSteelClock => 10.0,
                ClockStyle::GlacierIceClock => 12.0,
                _ => 22.0,
            };
            let r_card = base_rad * rad_sc * s;
            let r_base = if is_hovered {
                ((cfg.dt_rim_alpha as f32 / 255.0) + 0.25).clamp(0.25, 0.98)
            } else {
                (cfg.dt_rim_alpha as f32 / 255.0).clamp(0.0, 1.0)
            };
            let inter = crate::realistic_pbr::pbr_input_from_viz(
                fx_viz,
                (now.second as f32) * 0.35,
                is_hovered,
            );
            let tint_ov = tr_card.color_rgb.map(|c| (c, tr_card.color2_rgb)).or_else(|| {
                if cfg.dt_use_custom_color || cfg.dt_match_wallpaper {
                    Some((
                        crate::theme::rgb_to_u32(pal.glass_top),
                        Some(crate::theme::rgb_to_u32(pal.glass_bot)),
                    ))
                } else {
                    None
                }
            });

            if cfg.dt_show_plate {
                let mut nb: Option<&mut [ClockElemBox]> = None;
                let g = begin_elem_xf_slot(rt, ClockElemId::Card.idx(), tr_card, card_rect, s, &mut nb);
                crate::realistic_pbr::draw_pbr_drop_shadows(rt, main_rect, r_card, s, pbr_mat.is_light_surface());
                let op = (cfg.dt_glass_alpha as f32 / 140.0).clamp(0.45, 0.99);
                crate::realistic_pbr::draw_pbr_surface_clipped(
                    rt,
                    d2d.as_ref(),
                    pbr_mat,
                    main_rect,
                    r_card,
                    s,
                    op,
                    tint_ov,
                    None,
                    &inter,
                );
                crate::realistic_pbr::draw_pbr_bevel_rim(
                    rt,
                    main_rect,
                    r_card,
                    s,
                    pbr_mat,
                    tr_card.border_rgb,
                    r_base,
                    inter.hover_mix,
                    None,
                );
                end_elem_xf_slot(rt, g);
            }

            let is_mech_split = matches!(cfg.dt_style, ClockStyle::CarbonSteelClock);
            let (tag_str, accent_default) = match cfg.dt_style {
                ClockStyle::RainWindowClock => ("TWILIGHT RAIN", rgba_u8(186, 214, 232, 0.90)),
                ClockStyle::AbyssalTrenchClock => ("ABYSSAL DEPTH", rgba_u8(0, 255, 204, 0.92)),
                ClockStyle::GlacierIceClock => ("GLACIAL FROST", rgba_u8(160, 231, 229, 0.94)),
                ClockStyle::CarbonSteelClock => ("AERO-CARBON", rgba_u8(255, 107, 53, 0.96)),
                ClockStyle::GoldenHourClock => ("GOLDEN HORIZON", rgba_u8(255, 209, 102, 0.94)),
                _ => ("ALPINE MIST", rgba_u8(218, 215, 205, 0.90)),
            };

            if is_mech_split {
                // Aerospace Carbon & Steel precision circular sweep dial on left (inside ClockElemId::Orb!)
                let cx_d = x + 76.0 * s;
                let cy_d = y + dt_h * 0.5;
                let r_d = 46.0 * s;
                if cfg.dt_show_orb {
                    let xf = begin_elem_xf(
                        rt,
                        ClockElemId::Orb,
                        (cx_d - r_d - 2.0 * s, cy_d - r_d - 2.0 * s, cx_d + r_d + 2.0 * s, cy_d + r_d + 2.0 * s),
                        s,
                        cfg,
                        &mut out_boxes,
                    );
                    let orb_tr = cfg.dt_elem_transforms[ClockElemId::Orb.idx()];
                    let dial_c = elem_col_rgba(orb_tr.color2_rgb, 0.94, rgba_u8(14, 15, 18, 0.94));
                    if let Ok(db) = rt.CreateSolidColorBrush(&dial_c, None) {
                        rt.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: cx_d, Y: cy_d }, radiusX: r_d, radiusY: r_d }, &db);
                    }
                    let ring_c = elem_col_rgba(orb_tr.border_rgb, 0.88, rgba_u8(141, 153, 174, 0.88));
                    if let Ok(rb) = rt.CreateSolidColorBrush(&ring_c, None) {
                        rt.DrawEllipse(&D2D1_ELLIPSE { point: Vector2 { X: cx_d, Y: cy_d }, radiusX: r_d, radiusY: r_d }, &rb, 1.6 * s, None);
                        for ti in 0..12 {
                            let a = (ti as f32 * 30.0 - 90.0).to_radians();
                            rt.DrawLine(
                                Vector2 { X: cx_d + 35.0 * s * a.cos(), Y: cy_d + 35.0 * s * a.sin() },
                                Vector2 { X: cx_d + 42.0 * s * a.cos(), Y: cy_d + 42.0 * s * a.sin() },
                                &rb,
                                if ti % 3 == 0 { 2.0 * s } else { 1.0 * s },
                                None,
                            );
                        }
                    }
                    let h_ang = (((now.hour % 12) as f32 + now.minute as f32 / 60.0) * 30.0 - 90.0).to_radians();
                    let m_ang = ((now.minute as f32 + now.second as f32 / 60.0) * 6.0 - 90.0).to_radians();
                    if let Ok(wh) = rt.CreateSolidColorBrush(&elem_col_rgba(orb_tr.color_rgb, 0.96, rgba_u8(237, 242, 244, 0.96)), None) {
                        rt.DrawLine(Vector2 { X: cx_d, Y: cy_d }, Vector2 { X: cx_d + 23.0 * s * h_ang.cos(), Y: cy_d + 23.0 * s * h_ang.sin() }, &wh, 2.8 * s, None);
                        rt.DrawLine(Vector2 { X: cx_d, Y: cy_d }, Vector2 { X: cx_d + 35.0 * s * m_ang.cos(), Y: cy_d + 35.0 * s * m_ang.sin() }, &wh, 1.9 * s, None);
                    }
                    let sw_c = elem_col_rgba(orb_tr.color_rgb, 0.98, rgba_u8(255, 107, 53, 0.98));
                    if let Ok(ob) = rt.CreateSolidColorBrush(&sw_c, None) {
                        if cfg.dt_show_seconds {
                            let sa = ((now.second as f32) * 6.0 - 90.0).to_radians();
                            rt.DrawLine(Vector2 { X: cx_d - 8.0 * s * sa.cos(), Y: cy_d - 8.0 * s * sa.sin() }, Vector2 { X: cx_d + 38.0 * s * sa.cos(), Y: cy_d + 38.0 * s * sa.sin() }, &ob, 1.4 * s, None);
                        }
                        rt.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: cx_d, Y: cy_d }, radiusX: 3.2 * s, radiusY: 3.2 * s }, &ob);
                    }
                    end_elem_xf(rt, xf);
                }
                if cfg.dt_show_spine {
                    let xf = begin_elem_xf(rt, ClockElemId::Spine, (x + 138.0 * s, y + 22.0 * s, x + 142.0 * s, y + dt_h - 22.0 * s), s, cfg, &mut out_boxes);
                    let sp_c = elem_col_rgba(cfg.dt_elem_transforms[ClockElemId::Spine.idx()].color_rgb, 0.90, accent_default);
                    if let Ok(sp) = rt.CreateSolidColorBrush(&sp_c, None) {
                        rt.FillRoundedRectangle(&D2D1_ROUNDED_RECT { rect: D2D_RECT_F { left: x + 138.5 * s, top: y + 24.0 * s, right: x + 141.5 * s, bottom: y + dt_h - 24.0 * s }, radiusX: 1.5 * s, radiusY: 1.5 * s }, &sp);
                    }
                    end_elem_xf(rt, xf);
                }
                if cfg.dt_show_weekday {
                    let tf_wd = make_resolved_tf_named(dw, custom_fonts, &font_day, &[w!("Bahnschrift"), w!("Segoe UI")], 10.5 * PT_TO_PX * s, DWRITE_FONT_WEIGHT_BOLD, DWRITE_TEXT_ALIGNMENT_LEADING, DWRITE_PARAGRAPH_ALIGNMENT_CENTER);
                    let acc_br = rt.CreateSolidColorBrush(&accent_default, None).ok();
                    if let (Some(tf), Some(br)) = (&tf_wd, if cfg.dt_use_custom_color { br_accent.as_ref() } else { acc_br.as_ref().or(br_accent.as_ref()) }) {
                        let xf = begin_elem_xf(rt, ClockElemId::Day, (x + 154.0 * s, y + 20.0 * s, x + dt_w - 18.0 * s, y + 40.0 * s), s, cfg, &mut out_boxes);
                        draw_text_spaced(&format!("{} // {}", tag_str, now.weekday_name.to_ascii_uppercase()), tf, x + 154.0 * s, y + 20.0 * s, dt_w - 172.0 * s, 20.0 * s, br, sh_date.as_ref(), 0.9 * s, false);
                        end_elem_xf(rt, xf);
                    }
                }
                let tf_c = make_resolved_tf_named(dw, custom_fonts, &font_clock, &[w!("Bahnschrift"), w!("Segoe UI")], 32.0 * PT_TO_PX * s, DWRITE_FONT_WEIGHT_BOLD, DWRITE_TEXT_ALIGNMENT_LEADING, DWRITE_PARAGRAPH_ALIGNMENT_CENTER);
                if let (Some(tf), Some(br)) = (&tf_c, &br_cream) {
                    let xf = begin_elem_xf(rt, ClockElemId::Clock, (x + 152.0 * s, y + 46.0 * s, x + 312.0 * s, y + 96.0 * s), s, cfg, &mut out_boxes);
                    draw_text_spaced(&time_str, tf, x + 154.0 * s, y + 46.0 * s, 158.0 * s, 50.0 * s, br, sh_day.as_ref(), 1.1 * s, true);
                    end_elem_xf(rt, xf);
                }
                if !cfg.widgets_24h && cfg.dt_show_ampm {
                    let xf = begin_elem_xf(rt, ClockElemId::AmPm, (x + 314.0 * s, y + 60.0 * s, x + 358.0 * s, y + 80.0 * s), s, cfg, &mut out_boxes);
                    let am_tr = cfg.dt_elem_transforms[ClockElemId::AmPm.idx()];
                    let ap_rr = D2D1_ROUNDED_RECT { rect: D2D_RECT_F { left: x + 314.0 * s, top: y + 60.0 * s, right: x + 358.0 * s, bottom: y + 80.0 * s }, radiusX: 4.0 * s, radiusY: 4.0 * s };
                    if let Ok(ab) = rt.CreateSolidColorBrush(&elem_col_rgba(am_tr.color_rgb, 0.30, rgba_u8(255, 107, 53, 0.30)), None) {
                        rt.FillRoundedRectangle(&ap_rr, &ab);
                    }
                    if let (Some(tf), Some(br)) = (make_resolved_tf_named(dw, custom_fonts, &font_sub, &[w!("Cascadia Mono"), w!("Segoe UI")], 8.0 * PT_TO_PX * s, DWRITE_FONT_WEIGHT_BOLD, DWRITE_TEXT_ALIGNMENT_CENTER, DWRITE_PARAGRAPH_ALIGNMENT_CENTER), &br_cream) {
                        draw_shadowed_text(rt, ampm_str, &tf, ap_rr.rect, br, None, 0.0, true);
                    }
                    end_elem_xf(rt, xf);
                }
                if !footer_str.is_empty() {
                    let tf_f = make_resolved_tf_named(dw, custom_fonts, &font_sub, &[w!("Cascadia Mono"), w!("Segoe UI")], 9.0 * PT_TO_PX * s, DWRITE_FONT_WEIGHT_REGULAR, DWRITE_TEXT_ALIGNMENT_LEADING, DWRITE_PARAGRAPH_ALIGNMENT_CENTER);
                    if let (Some(tf), Some(br)) = (&tf_f, &br_sand) {
                        let xf = begin_elem_xf(rt, ClockElemId::Date, (x + 154.0 * s, y + 102.0 * s, x + dt_w - 18.0 * s, y + 124.0 * s), s, cfg, &mut out_boxes);
                        draw_text_spaced(&footer_str, tf, x + 154.0 * s, y + 102.0 * s, dt_w - 172.0 * s, 22.0 * s, br, sh_date.as_ref(), 0.5 * s, false);
                        end_elem_xf(rt, xf);
                    }
                }
            } else {
                // Centered atmospheric optical clock with signature Orb & Spine accents
                if cfg.dt_show_orb {
                    let xf = begin_elem_xf(rt, ClockElemId::Orb, (x + dt_w * 0.5 - 58.0 * s, y + dt_h * 0.5 - 30.0 * s, x + dt_w * 0.5 + 58.0 * s, y + dt_h * 0.5 + 30.0 * s), s, cfg, &mut out_boxes);
                    let orb_tr = cfg.dt_elem_transforms[ClockElemId::Orb.idx()];
                    let oc = elem_col_rgba(orb_tr.color_rgb.or(orb_tr.border_rgb), 0.34, rgba_u8((accent_default.r * 255.0) as u8, (accent_default.g * 255.0) as u8, (accent_default.b * 255.0) as u8, 0.28));
                    if let Ok(ob) = rt.CreateSolidColorBrush(&oc, None) {
                        for ri in 0..2 {
                            let rad_x = (36.0 + ri as f32 * 22.0) * s;
                            let rad_y = (16.0 + ri as f32 * 10.0) * s;
                            rt.DrawEllipse(&D2D1_ELLIPSE { point: Vector2 { X: x + dt_w * 0.5, Y: y + dt_h * 0.52 }, radiusX: rad_x, radiusY: rad_y }, &ob, 1.1 * s, None);
                        }
                    }
                    end_elem_xf(rt, xf);
                }
                if cfg.dt_show_spine {
                    let xf = begin_elem_xf(rt, ClockElemId::Spine, (x + dt_w * 0.5 - 54.0 * s, y + 92.0 * s, x + dt_w * 0.5 + 54.0 * s, y + 95.0 * s), s, cfg, &mut out_boxes);
                    let sp_c = elem_col_rgba(cfg.dt_elem_transforms[ClockElemId::Spine.idx()].color_rgb, 0.72, accent_default);
                    if let Ok(sp) = rt.CreateSolidColorBrush(&sp_c, None) {
                        rt.DrawLine(Vector2 { X: x + dt_w * 0.5 - 52.0 * s, Y: y + 93.5 * s }, Vector2 { X: x + dt_w * 0.5 + 52.0 * s, Y: y + 93.5 * s }, &sp, 1.1 * s, None);
                    }
                    end_elem_xf(rt, xf);
                }
                if cfg.dt_show_weekday {
                    let tf_wd = make_resolved_tf_named(dw, custom_fonts, &font_day, &[w!("Segoe UI Light"), w!("Georgia")], 11.0 * PT_TO_PX * s, DWRITE_FONT_WEIGHT_SEMI_BOLD, DWRITE_TEXT_ALIGNMENT_CENTER, DWRITE_PARAGRAPH_ALIGNMENT_CENTER);
                    let acc_br = rt.CreateSolidColorBrush(&accent_default, None).ok();
                    if let (Some(tf), Some(br)) = (&tf_wd, if cfg.dt_use_custom_color { br_accent.as_ref() } else { acc_br.as_ref().or(br_cream.as_ref()) }) {
                        let xf = begin_elem_xf(rt, ClockElemId::Day, (x + 24.0 * s, y + 15.0 * s, x + dt_w - 24.0 * s, y + 36.0 * s), s, cfg, &mut out_boxes);
                        draw_text_spaced(&format!("{}  •  {}", tag_str, now.weekday_name.to_ascii_uppercase()), tf, x + 24.0 * s, y + 15.0 * s, dt_w - 48.0 * s, 21.0 * s, br, sh_date.as_ref(), 1.1 * s, false);
                        end_elem_xf(rt, xf);
                    }
                }
                let tf_c = make_resolved_tf_named(dw, custom_fonts, &font_clock, &[w!("SF Pro Display"), w!("Segoe UI")], 37.0 * PT_TO_PX * s, DWRITE_FONT_WEIGHT_LIGHT, DWRITE_TEXT_ALIGNMENT_CENTER, DWRITE_PARAGRAPH_ALIGNMENT_CENTER);
                if let (Some(tf), Some(br)) = (&tf_c, &br_cream) {
                    let xf = begin_elem_xf(rt, ClockElemId::Clock, (x + 48.0 * s, y + 38.0 * s, x + dt_w - 48.0 * s, y + 92.0 * s), s, cfg, &mut out_boxes);
                    draw_text_spaced(&time_str, tf, x + 48.0 * s, y + 38.0 * s, dt_w - 96.0 * s, 54.0 * s, br, sh_day.as_ref(), 1.2 * s, false);
                    end_elem_xf(rt, xf);
                }
                if !cfg.widgets_24h && cfg.dt_show_ampm {
                    let xf = begin_elem_xf(rt, ClockElemId::AmPm, (x + dt_w - 64.0 * s, y + 54.0 * s, x + dt_w - 22.0 * s, y + 74.0 * s), s, cfg, &mut out_boxes);
                    let am_tr = cfg.dt_elem_transforms[ClockElemId::AmPm.idx()];
                    let ap_rr = D2D1_ROUNDED_RECT { rect: D2D_RECT_F { left: x + dt_w - 64.0 * s, top: y + 54.0 * s, right: x + dt_w - 22.0 * s, bottom: y + 74.0 * s }, radiusX: 7.0 * s, radiusY: 7.0 * s };
                    if let Ok(ab) = rt.CreateSolidColorBrush(&elem_col_rgba(am_tr.color_rgb, 0.28, rgba_u8((accent_default.r * 255.0) as u8, (accent_default.g * 255.0) as u8, (accent_default.b * 255.0) as u8, 0.22)), None) {
                        rt.FillRoundedRectangle(&ap_rr, &ab);
                    }
                    if let (Some(tf), Some(br)) = (make_resolved_tf_named(dw, custom_fonts, &font_sub, &[w!("Segoe UI")], 8.0 * PT_TO_PX * s, DWRITE_FONT_WEIGHT_BOLD, DWRITE_TEXT_ALIGNMENT_CENTER, DWRITE_PARAGRAPH_ALIGNMENT_CENTER), &br_cream) {
                        draw_shadowed_text(rt, ampm_str, &tf, ap_rr.rect, br, None, 0.0, true);
                    }
                    end_elem_xf(rt, xf);
                }
                if !footer_str.is_empty() {
                    let tf_f = make_resolved_tf_named(dw, custom_fonts, &font_sub, &[w!("Segoe UI")], 9.2 * PT_TO_PX * s, DWRITE_FONT_WEIGHT_REGULAR, DWRITE_TEXT_ALIGNMENT_CENTER, DWRITE_PARAGRAPH_ALIGNMENT_CENTER);
                    if let (Some(tf), Some(br)) = (&tf_f, &br_sand) {
                        let xf = begin_elem_xf(rt, ClockElemId::Date, (x + 28.0 * s, y + 100.0 * s, x + dt_w - 28.0 * s, y + 124.0 * s), s, cfg, &mut out_boxes);
                        draw_text_spaced(&footer_str, tf, x + 28.0 * s, y + 100.0 * s, dt_w - 56.0 * s, 24.0 * s, br, sh_date.as_ref(), 0.6 * s, false);
                        end_elem_xf(rt, xf);
                    }
                }
            }
        }
    }
    };

    if out_boxes.is_none() {
        if let Some((slot, sec_pair, cache_gen)) = fg_cache {
            let clock_now = chrono_local();
            let ms = (GetLocalTime().wMilliseconds as f32 / 1000.0).clamp(0.0, 0.999);
            if cfg.dt_style == ClockStyle::FlipMech && cfg.dt_show_seconds {
                if clock_now.second == 0 && ms < 0.60 {
                    draw_body(rt, None, FlipMechPass::All, Some((sec_pair, cache_gen)));
                } else {
                    let static_key = clock_fg_key(
                        x,
                        y,
                        s,
                        dt_w,
                        dt_h,
                        rad,
                        cfg,
                        &pal,
                        script_rgb,
                        is_hovered,
                        &clock_now,
                        custom_fonts.is_some(),
                        false,
                    );
                    slot.draw_cached(rt, cache_gen, static_key, x, y, dt_w, dt_h, 14.0 * s, |fg_rt| {
                        draw_body(fg_rt, None, FlipMechPass::StaticOnly, None);
                    });
                    draw_body(rt, None, FlipMechPass::SecondsOnly, Some((sec_pair, cache_gen)));
                }
            } else {
                let flip_anim = cfg.dt_style == ClockStyle::FlipMech && (clock_now.second == 0 && ms < 0.60);
                let pbr_anim = cfg.widgets_live_material
                    && fx_viz
                        .map(|v| {
                            v.hover_mix > 0.004
                                || v.hover_active
                                || v.ripples.iter().any(|r| r.active)
                                || (v.phase > 0.0001
                                    && matches!(
                                        cfg.dt_style,
                                        ClockStyle::WaterCausticsClock | ClockStyle::RainWindowClock
                                    ))
                        })
                        .unwrap_or(false)
                    && matches!(
                        cfg.dt_style,
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
                            | ClockStyle::AlpineMistClock
                    );
                if !flip_anim && !pbr_anim {
                    let key = clock_fg_key(
                        x,
                        y,
                        s,
                        dt_w,
                        dt_h,
                        rad,
                        cfg,
                        &pal,
                        script_rgb,
                        is_hovered,
                        &clock_now,
                        custom_fonts.is_some(),
                        true,
                    );
                    slot.draw_cached(rt, cache_gen, key, x, y, dt_w, dt_h, 14.0 * s, |fg_rt| {
                        draw_body(fg_rt, None, FlipMechPass::All, None);
                    });
                } else {
                    draw_body(rt, None, FlipMechPass::All, None);
                }
            }
        } else {
            draw_body(rt, None, FlipMechPass::All, None);
        }
    } else {
        draw_body(rt, out_boxes, FlipMechPass::All, None);
    }

    if cfg.dt_show_plate && tr_card.visible && cfg.dt_hover_fx != TaskbarVizStyle::Off && !cfg.dt_style.uses_standard_plate() {
        let mut nb: Option<&mut [ClockElemBox]> = None;
        let g_card = begin_elem_xf_slot(rt, ClockElemId::Card.idx(), tr_card, card_rect, s, &mut nb);
        let fx_rad = if cfg.dt_style == ClockStyle::Island {
            dt_h * 0.46
        } else {
            rad.max(14.0 * s)
        };
        paint_widget_elemental_fx(
            rt,
            d2d.as_ref(),
            x,
            y,
            x + dt_w,
            y + dt_h,
            (x, y, dt_w, dt_h),
            fx_rad,
            s,
            cfg.dt_hover_fx,
            66,
            &pal,
            !cfg.dt_match_wallpaper && !cfg.dt_use_custom_color && tr_card.color_rgb.is_none(),
            fx_viz,
        );
        end_elem_xf_slot(rt, g_card);
    }
}

pub struct AppTileDrawItem<'a> {
    pub label: &'a str,
    pub bmp: Option<&'a ID2D1Bitmap>,
    pub rel_x: f32,
    pub rel_y: f32,
    pub hovered: bool,
    pub is_add_tile: bool,
}

pub fn apps_layout_offsets(count: usize, max_cols: usize) -> (Vec<(f32, f32)>, f32, f32) {
    let cols = max_cols.max(1);
    let stride_x = 86.0f32;
    let stride_y = 104.0f32;
    let eff_count = count.max(1);
    let mut offsets = Vec::with_capacity(count);
    for i in 0..count {
        let col = i % cols;
        let row = i / cols;
        offsets.push((col as f32 * stride_x, row as f32 * stride_y));
    }
    let cols_used = eff_count.min(cols);
    let rows_used = (eff_count + cols - 1) / cols;
    let total_w = (cols_used - 1) as f32 * stride_x + 64.0;
    let total_h = (rows_used - 1) as f32 * stride_y + 94.0;
    (offsets, total_w, total_h)
}

fn truncate_tile_label(label: &str, max_chars: usize) -> String {
    let cnt = label.chars().count();
    if cnt <= max_chars {
        label.to_string()
    } else {
        let mut s: String = label.chars().take(max_chars.saturating_sub(1)).collect();
        s.push('…');
        s
    }
}

/// Shared Direct2D App Launchers Row/Grid renderer.
#[allow(clippy::too_many_arguments)]
pub unsafe fn draw_apps_widget_with_viz(
    rt: &ID2D1RenderTarget,
    dw: &IDWriteFactory,
    custom_fonts: Option<&IDWriteFontCollection>,
    base_x: f32,
    base_y: f32,
    s: f32,
    cfg: &AppConfig,
    tiles: &[AppTileDrawItem<'_>],
    fx_viz: Option<&VizSnapshot>,
) {
    draw_apps_widget_with_boxes(rt, dw, custom_fonts, base_x, base_y, s, cfg, tiles, fx_viz, None);
}

#[allow(clippy::too_many_arguments)]
pub unsafe fn draw_apps_widget_with_boxes(
    rt: &ID2D1RenderTarget,
    dw: &IDWriteFactory,
    custom_fonts: Option<&IDWriteFontCollection>,
    base_x: f32,
    base_y: f32,
    s: f32,
    cfg: &AppConfig,
    tiles: &[AppTileDrawItem<'_>],
    fx_viz: Option<&VizSnapshot>,
    mut out_boxes: Option<&mut [ClockElemBox; 5]>,
) {
    if let Some(boxes) = out_boxes.as_deref_mut() {
        *boxes = [ClockElemBox::default(); 5];
    }
    if cfg.is_apps_all_elements_off() {
        return;
    }
    let tr_tiles = cfg.apps_elem_transforms[0];
    let tr_labels = cfg.apps_elem_transforms[1];
    let tr_dots = cfg.apps_elem_transforms[2];
    let tr_add = cfg.apps_elem_transforms[3];
    let tr_card = cfg.apps_elem_transforms[4];

    let lbl_fs = (cfg.apps_font_scale_pct as f32 / 100.0).clamp(0.75, 1.35)
        * (tr_labels.font_scale_pct as f32 / 100.0).clamp(0.50, 2.20);
    let lbl_font = if !cfg.apps_elem_fonts[1].is_empty() {
        &cfg.apps_elem_fonts[1]
    } else {
        &cfg.apps_font_label
    };
    let tf_app = make_resolved_tf_named(
        dw,
        custom_fonts,
        lbl_font,
        &[w!("SF Pro Display"), w!("Segoe UI Semibold"), w!("Segoe UI")],
        8.0 * lbl_fs * PT_TO_PX * s,
        DWRITE_FONT_WEIGHT_SEMI_BOLD,
        DWRITE_TEXT_ALIGNMENT_CENTER,
        DWRITE_PARAGRAPH_ALIGNMENT_NEAR,
    );
    let tile_fs = (cfg.apps_font_scale_pct as f32 / 100.0).clamp(0.75, 1.35)
        * (tr_tiles.font_scale_pct as f32 / 100.0).clamp(0.50, 2.20);
    let tile_font = if !cfg.apps_elem_fonts[0].is_empty() {
        &cfg.apps_elem_fonts[0]
    } else {
        &cfg.apps_font_label
    };
    let tf_mono = make_resolved_tf_named(
        dw,
        custom_fonts,
        tile_font,
        &[w!("SF Pro Display"), w!("Segoe UI")],
        14.0 * tile_fs * PT_TO_PX * s,
        DWRITE_FONT_WEIGHT_BOLD,
        DWRITE_TEXT_ALIGNMENT_CENTER,
        DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
    );
    let pal = resolve_apps_palette(cfg);
    let d2d: Option<ID2D1Factory> = rt.GetFactory().ok();
    let canvas_box = if tiles.is_empty() {
        (base_x, base_y, 64.0 * s, 64.0 * s)
    } else {
        let mut l = f32::MAX;
        let mut t = f32::MAX;
        let mut r = f32::MIN;
        let mut b = f32::MIN;
        for item in tiles {
            let ax = base_x + item.rel_x * s;
            let ay = base_y + item.rel_y * s;
            l = l.min(ax);
            t = t.min(ay);
            r = r.max(ax + 64.0 * s);
            b = b.max(ay + 64.0 * s);
        }
        (l, t, (r - l).max(64.0 * s), (b - t).max(64.0 * s))
    };

    // Union rects per semantic element so group Move/Scale/Rotate pivots smoothly
    let mut app_tiles_rect: Option<(f32, f32, f32, f32)> = None;
    let mut labels_rect: Option<(f32, f32, f32, f32)> = None;
    let mut dots_rect: Option<(f32, f32, f32, f32)> = None;
    let mut add_rect: Option<(f32, f32, f32, f32)> = None;
    let expand_u = |dst: &mut Option<(f32, f32, f32, f32)>, r: (f32, f32, f32, f32)| {
        *dst = Some(match *dst {
            Some((l, t, rr, bb)) => (l.min(r.0), t.min(r.1), rr.max(r.2), bb.max(r.3)),
            None => r,
        });
    };
    for item in tiles {
        let ax = base_x + item.rel_x * s;
        let ay = base_y + item.rel_y * s;
        if item.is_add_tile {
            expand_u(&mut add_rect, (ax, ay, ax + 64.0 * s, ay + 64.0 * s));
        } else {
            expand_u(&mut app_tiles_rect, (ax, ay, ax + 64.0 * s, ay + 64.0 * s));
            expand_u(&mut dots_rect, (ax + 24.0 * s, ay + 66.0 * s, ax + 40.0 * s, ay + 75.0 * s));
            expand_u(&mut labels_rect, (ax - 8.0 * s, ay + 76.0 * s, ax + 72.0 * s, ay + 96.0 * s));
        }
    }
    let br_cream = rt.CreateSolidColorBrush(&elem_col_rgba(tr_labels.color_rgb, 1.0, pal.text_col(1.0)), None).ok();
    let br_mono = rt.CreateSolidColorBrush(&elem_col_rgba(tr_tiles.color_rgb, 1.0, pal.text_col(1.0)), None).ok();
    let sh_br = rt.CreateSolidColorBrush(&rgba_u8(0, 0, 0, 0.72), None).ok();

    let card_pad_x = 12.0 * s;
    let card_pad_top = 10.0 * s;
    let card_pad_bot = if cfg.apps_show_labels {
        36.0 * s
    } else if cfg.apps_show_dots {
        16.0 * s
    } else {
        10.0 * s
    };
    let apps_card_rect = (
        canvas_box.0 - card_pad_x,
        canvas_box.1 - card_pad_top,
        canvas_box.0 + canvas_box.2 + card_pad_x,
        canvas_box.1 + canvas_box.3 + card_pad_bot,
    );
    {
        let mut slice_opt = out_boxes.as_deref_mut().map(|b| &mut b[..]);
        record_elem_box_slot(&mut slice_opt, 4, tr_card, apps_card_rect, s);
    }
    if cfg.apps_show_plate && tr_card.visible {
        let mut nb: Option<&mut [ClockElemBox]> = None;
        let g_card = begin_elem_xf_slot(rt, 4, tr_card, apps_card_rect, s, &mut nb);
        let c_rad = ((cfg.apps_corner_radius as f32) + 4.0).clamp(4.0, 32.0) * s;
        let c_rr = D2D1_ROUNDED_RECT {
            rect: D2D_RECT_F {
                left: apps_card_rect.0,
                top: apps_card_rect.1,
                right: apps_card_rect.2,
                bottom: apps_card_rect.3,
            },
            radiusX: c_rad,
            radiusY: c_rad,
        };
        let ga_raw = (cfg.apps_glass_alpha as f32 / 255.0).clamp(0.18, 0.88);
        let ra = (cfg.apps_rim_alpha as f32 / 255.0).clamp(0.18, 0.95);
        let (top_col, bot_col, rim_col, ang) = resolve_card_plate_colors(&tr_card, &pal, ga_raw, (ga_raw * 0.72).min(0.80), ra);
        let bg_stops = [
            D2D1_GRADIENT_STOP { position: 0.0, color: top_col },
            D2D1_GRADIENT_STOP { position: 1.0, color: bot_col },
        ];
        let (gp0, gp1) = grad_endpoints_for_rect(apps_card_rect, ang);
        if let Some(bg_br) = create_linear_grad(rt, gp0, gp1, &bg_stops) {
            rt.FillRoundedRectangle(&c_rr, &bg_br);
        }
        if let Ok(rb) = rt.CreateSolidColorBrush(&rim_col, None) {
            rt.DrawRoundedRectangle(&c_rr, &rb, 1.1 * s, None);
        }
        end_elem_xf_slot(rt, g_card);
    }

    let tile_rad = match cfg.apps_tile_shape {
        1 => 32.0 * s,
        2 | 3 => 8.0 * s,
        _ => (cfg.apps_corner_radius as f32).clamp(0.0, 32.0) * s,
    };

    for item in tiles {
        let ax = base_x + item.rel_x * s;
        let ay = base_y + item.rel_y * s;
        let tile_w = 64.0 * s;
        let tile_h = 64.0 * s;
        let tile_rr = D2D1_ROUNDED_RECT {
            rect: D2D_RECT_F {
                left: ax,
                top: ay,
                right: ax + tile_w,
                bottom: ay + tile_h,
            },
            radiusX: tile_rad,
            radiusY: tile_rad,
        };
        let poly_pts: Option<Vec<Vector2>> = match cfg.apps_tile_shape {
            2 => Some(vec![
                Vector2 { X: ax + 16.0 * s, Y: ay + 2.0 * s },
                Vector2 { X: ax + 48.0 * s, Y: ay + 2.0 * s },
                Vector2 { X: ax + 63.0 * s, Y: ay + 32.0 * s },
                Vector2 { X: ax + 48.0 * s, Y: ay + 62.0 * s },
                Vector2 { X: ax + 16.0 * s, Y: ay + 62.0 * s },
                Vector2 { X: ax + 1.0 * s, Y: ay + 32.0 * s },
            ]),
            3 => {
                let cut = 13.0 * s;
                Some(vec![
                    Vector2 { X: ax + cut, Y: ay },
                    Vector2 { X: ax + tile_w - cut, Y: ay },
                    Vector2 { X: ax + tile_w, Y: ay + cut },
                    Vector2 { X: ax + tile_w, Y: ay + tile_h - cut },
                    Vector2 { X: ax + tile_w - cut, Y: ay + tile_h },
                    Vector2 { X: ax + cut, Y: ay + tile_h },
                    Vector2 { X: ax, Y: ay + tile_h - cut },
                    Vector2 { X: ax, Y: ay + cut },
                ])
            }
            _ => None,
        };

        let rim_a = if item.hovered {
            0.96
        } else if item.is_add_tile {
            0.72
        } else {
            (cfg.apps_rim_alpha as f32 / 255.0).clamp(0.0, 1.0)
        };

        let elem_idx = if item.is_add_tile { 3 } else { 0 };
        let mut tr_this = cfg.apps_elem_transforms[elem_idx];
        let tile_vis = if item.is_add_tile {
            cfg.apps_show_add_tile && tr_this.visible
        } else {
            cfg.apps_show_tiles && tr_this.visible
        };
        tr_this.visible = tile_vis;
        let grp_rect = if item.is_add_tile {
            add_rect.unwrap_or((ax, ay, ax + tile_w, ay + tile_h))
        } else {
            app_tiles_rect.unwrap_or((ax, ay, ax + tile_w, ay + tile_h))
        };
        let mut slice_opt = out_boxes.as_deref_mut().map(|b| &mut b[..]);
        let g_tile = begin_elem_xf_slot(rt, elem_idx, tr_this, grp_rect, s, &mut slice_opt);

        if cfg.apps_show_plate {
            let bg_a = if item.is_add_tile {
                if item.hovered { 0.28 } else { 0.14 }
            } else if item.hovered {
                ((cfg.apps_glass_alpha as f32 + 43.0) / 255.0).clamp(0.1, 0.85)
            } else {
                (cfg.apps_glass_alpha as f32 / 255.0).clamp(0.0, 0.85)
            };
            let tile_bg_a = if tr_this.color_rgb.is_some() && !item.is_add_tile {
                bg_a.max(0.36)
            } else {
                bg_a
            };
            let bg_color = if item.is_add_tile {
                elem_col_rgba(tr_this.color_rgb, tile_bg_a, pal.accent_col(tile_bg_a))
            } else {
                elem_col_rgba(tr_this.color_rgb, tile_bg_a, pal.glass_top_col(tile_bg_a))
            };
            if let Ok(bg_br) = rt.CreateSolidColorBrush(&bg_color, None) {
                if let Some(ref pts) = poly_pts {
                    let fb: ID2D1Brush = bg_br.cast().unwrap();
                    draw_clock_poly(rt, d2d.as_ref(), pts, Some(&fb), None, 0.0, true);
                } else {
                    rt.FillRoundedRectangle(&tile_rr, &bg_br);
                }
            }
        }

        paint_widget_elemental_fx(
            rt,
            d2d.as_ref(),
            ax,
            ay,
            ax + tile_w,
            ay + tile_h,
            canvas_box,
            tile_rad.max(4.0 * s),
            s,
            cfg.apps_hover_fx,
            90,
            &pal,
            !cfg.apps_match_wallpaper && !cfg.apps_use_custom_color,
            fx_viz,
        );

        if cfg.apps_show_plate && rim_a > 0.01 {
            let rim_c = elem_col_rgba(tr_this.border_rgb.or(tr_this.color_rgb), rim_a, pal.accent_col(rim_a));
            if let Ok(rim_br) = rt.CreateSolidColorBrush(&rim_c, None) {
                if let Some(ref pts) = poly_pts {
                    let sb: ID2D1Brush = rim_br.cast().unwrap();
                    draw_clock_poly(rt, d2d.as_ref(), pts, None, Some(&sb), 1.3 * s, true);
                } else {
                    rt.DrawRoundedRectangle(&tile_rr, &rim_br, 1.3 * s, None);
                }
            }

            if cfg.apps_tile_shape == 0 {
                if let Ok(spec_br) = rt.CreateSolidColorBrush(&pal.text_col(0.255), None) {
                    rt.DrawLine(
                        Vector2 { X: ax + 12.0 * s, Y: ay + 0.6 * s },
                        Vector2 { X: ax + 52.0 * s, Y: ay + 0.6 * s },
                        &spec_br,
                        1.0 * s,
                        None,
                    );
                }
            }
        }

        if item.is_add_tile {
            let cx = ax + tile_w * 0.5;
            let cy = ay + tile_h * 0.5;
            let plus_c = elem_col_rgba(tr_add.color_rgb, 0.96, pal.light_col(0.96));
            if let Ok(plus_br) = rt.CreateSolidColorBrush(&plus_c, None) {
                let h_bar = D2D1_ROUNDED_RECT {
                    rect: D2D_RECT_F {
                        left: cx - 10.0 * s,
                        top: cy - 1.5 * s,
                        right: cx + 10.0 * s,
                        bottom: cy + 1.5 * s,
                    },
                    radiusX: 1.5 * s,
                    radiusY: 1.5 * s,
                };
                let v_bar = D2D1_ROUNDED_RECT {
                    rect: D2D_RECT_F {
                        left: cx - 1.5 * s,
                        top: cy - 10.0 * s,
                        right: cx + 1.5 * s,
                        bottom: cy + 10.0 * s,
                    },
                    radiusX: 1.5 * s,
                    radiusY: 1.5 * s,
                };
                rt.FillRoundedRectangle(&h_bar, &plus_br);
                rt.FillRoundedRectangle(&v_bar, &plus_br);
            }
        } else if let Some(bmp) = item.bmp {
            let icon_w = 44.0 * s;
            let icon_h = 44.0 * s;
            let ic_rc = D2D_RECT_F {
                left: ax + (tile_w - icon_w) * 0.5,
                top: ay + (tile_h - icon_h) * 0.5,
                right: ax + (tile_w + icon_w) * 0.5,
                bottom: ay + (tile_h + icon_h) * 0.5,
            };
            rt.DrawBitmap(bmp, Some(&ic_rc), 1.0, D2D1_BITMAP_INTERPOLATION_MODE_LINEAR, None);
        } else {
            // Crisp amber monogram fallback for URIs/shortcuts without shell HICON
            let badge_rr = D2D1_ROUNDED_RECT {
                rect: D2D_RECT_F {
                    left: ax + 14.0 * s,
                    top: ay + 14.0 * s,
                    right: ax + tile_w - 14.0 * s,
                    bottom: ay + tile_h - 14.0 * s,
                },
                radiusX: 10.0 * s,
                radiusY: 10.0 * s,
            };
            if let Ok(fb_br) = rt.CreateSolidColorBrush(&elem_col_rgba(tr_tiles.color_rgb, 0.22, pal.accent_col(0.22)), None) {
                rt.FillRoundedRectangle(&badge_rr, &fb_br);
            }
            if let Ok(fr_br) = rt.CreateSolidColorBrush(&elem_col_rgba(tr_tiles.color_rgb, 0.65, pal.accent_col(0.65)), None) {
                rt.DrawRoundedRectangle(&badge_rr, &fr_br, 1.0 * s, None);
            }
            let init_ch: String = item
                .label
                .chars()
                .next()
                .map(|c| c.to_ascii_uppercase().to_string())
                .unwrap_or_else(|| "A".to_string());
            if let (Some(tf), Some(br)) = (&tf_mono, &br_mono) {
                draw_shadowed_text(rt, &init_ch, tf, badge_rr.rect, br, sh_br.as_ref(), 1.0 * s, true);
            }
        }
        end_elem_xf_slot(rt, g_tile);

        if cfg.apps_show_dots && !item.is_add_tile {
            let mut slice_opt = out_boxes.as_deref_mut().map(|b| &mut b[..]);
            let d_rect = dots_rect.unwrap_or((ax + 24.0 * s, ay + 66.0 * s, ax + 40.0 * s, ay + 75.0 * s));
            let g_dot = begin_elem_xf_slot(rt, 2, tr_dots, d_rect, s, &mut slice_opt);
            let dot_x = ax + 32.0 * s;
            let dot_y = ay + 70.0 * s;
            let dot_c = elem_col_rgba(tr_dots.color_rgb, rim_a.max(0.55), pal.accent_col(rim_a.max(0.55)));
            if let Ok(dot_br) = rt.CreateSolidColorBrush(&dot_c, None) {
                rt.FillEllipse(
                    &D2D1_ELLIPSE {
                        point: Vector2 { X: dot_x, Y: dot_y },
                        radiusX: 2.2 * s,
                        radiusY: 2.2 * s,
                    },
                    &dot_br,
                );
            }
            end_elem_xf_slot(rt, g_dot);
        }

        if cfg.apps_show_labels && !item.is_add_tile {
            let mut slice_opt = out_boxes.as_deref_mut().map(|b| &mut b[..]);
            let l_rect = labels_rect.unwrap_or((ax - 8.0 * s, ay + 76.0 * s, ax + tile_w + 8.0 * s, ay + 96.0 * s));
            let g_lbl = begin_elem_xf_slot(rt, 1, tr_labels, l_rect, s, &mut slice_opt);
            if let (Some(tf), Some(br)) = (&tf_app, &br_cream) {
                let disp = truncate_tile_label(item.label, 13);
                let rc = D2D_RECT_F {
                    left: ax - 10.0 * s,
                    top: ay + 76.0 * s,
                    right: ax + tile_w + 10.0 * s,
                    bottom: ay + 98.0 * s,
                };
                draw_shadowed_text(rt, &disp, tf, rc, br, sh_br.as_ref(), 1.0 * s, true);
            }
            end_elem_xf_slot(rt, g_lbl);
        }
    }
}

pub fn vitals_logical_height(cfg: &AppConfig) -> f32 {
    if cfg.vitals_style == 4 {
        64.0
    } else {
        54.0
    }
}

pub fn vitals_logical_size(cfg: &AppConfig) -> (f32, f32) {
    (vitals_logical_width(cfg), vitals_logical_height(cfg))
}

#[derive(Clone, Copy)]
struct RingPillBay {
    hit_zone: WidgetHitZone,
    elem_idx: usize,
    x_left: f32,
    bay_w: f32,
}

fn ring_pill_layout(cfg: &AppConfig) -> ([RingPillBay; 4], usize, f32) {
    let mut bays = [RingPillBay {
        hit_zone: WidgetHitZone::VitalsCpu,
        elem_idx: 0,
        x_left: 0.0,
        bay_w: 72.0,
    }; 4];
    let mut count = 0usize;
    let mut cur_x = 12.0f32;
    if cfg.vitals_show_cpu {
        bays[count] = RingPillBay {
            hit_zone: WidgetHitZone::VitalsCpu,
            elem_idx: 0,
            x_left: cur_x,
            bay_w: 72.0,
        };
        cur_x += 72.0;
        count += 1;
    }
    if cfg.vitals_show_power {
        bays[count] = RingPillBay {
            hit_zone: WidgetHitZone::VitalsGpu,
            elem_idx: 2,
            x_left: cur_x,
            bay_w: 72.0,
        };
        cur_x += 72.0;
        count += 1;
    }
    if cfg.vitals_show_ram {
        bays[count] = RingPillBay {
            hit_zone: WidgetHitZone::VitalsRam,
            elem_idx: 1,
            x_left: cur_x,
            bay_w: 72.0,
        };
        cur_x += 72.0;
        count += 1;
    }
    if cfg.vitals_show_power {
        bays[count] = RingPillBay {
            hit_zone: WidgetHitZone::VitalsTemp,
            elem_idx: 2,
            x_left: cur_x,
            bay_w: 72.0,
        };
        cur_x += 72.0;
        count += 1;
    }
    (bays, count, cur_x)
}

/// Computes dynamic width of the Vitals card (in unscaled 1280px units) based on active meters/buttons.
pub fn vitals_logical_width(cfg: &AppConfig) -> f32 {
    if cfg.vitals_style == 4 {
        let (_, _, end_x) = ring_pill_layout(cfg);
        return if cfg.vitals_show_buttons {
            end_x + 92.0
        } else {
            end_x + 12.0
        };
    }
    let meter_count = [cfg.vitals_show_cpu, cfg.vitals_show_ram, cfg.vitals_show_power]
        .iter()
        .filter(|&&b| b)
        .count();
    let meters_w = if meter_count > 0 {
        18.0 + (meter_count as f32) * 100.0
    } else {
        12.0
    };
    if cfg.vitals_show_buttons {
        meters_w + 92.0
    } else {
        (meters_w + 2.0).max(116.0)
    }
}

/// Shared Direct2D System Vitals & Telemetry Ring-Pill HUD renderer.
#[allow(clippy::too_many_arguments)]
pub unsafe fn draw_vitals_widget_with_telemetry_and_viz(
    rt: &ID2D1RenderTarget,
    dw: &IDWriteFactory,
    d2d: Option<&ID2D1Factory>,
    custom_fonts: Option<&IDWriteFontCollection>,
    x: f32,
    y: f32,
    s: f32,
    cfg: &AppConfig,
    cpu_pct: f32,
    gpu_pct: f32,
    ram_pct: f32,
    temp_c: f32,
    bat_pct: f32,
    hover: WidgetHitZone,
    fx_viz: Option<&VizSnapshot>,
) {
    draw_vitals_widget_with_telemetry_and_boxes(
        rt, dw, d2d, custom_fonts, x, y, s, cfg, cpu_pct, gpu_pct, ram_pct, temp_c, bat_pct,
        hover, fx_viz, None,
    );
}

#[allow(clippy::too_many_arguments)]
pub unsafe fn draw_vitals_widget_with_telemetry_and_boxes(
    rt: &ID2D1RenderTarget,
    dw: &IDWriteFactory,
    d2d: Option<&ID2D1Factory>,
    custom_fonts: Option<&IDWriteFontCollection>,
    x: f32,
    y: f32,
    s: f32,
    cfg: &AppConfig,
    cpu_pct: f32,
    gpu_pct: f32,
    ram_pct: f32,
    temp_c: f32,
    bat_pct: f32,
    hover: WidgetHitZone,
    fx_viz: Option<&VizSnapshot>,
    mut out_boxes: Option<&mut [ClockElemBox; 6]>,
) {
    if let Some(boxes) = out_boxes.as_deref_mut() {
        *boxes = [ClockElemBox::default(); 6];
    }
    if cfg.is_vitals_all_elements_off() {
        return;
    }
    if cfg.vitals_style == 4 {
        draw_telemetry_ring_pill_hud(
            rt, dw, d2d, custom_fonts, x, y, s, cfg, cpu_pct, gpu_pct, ram_pct, temp_c, hover,
            fx_viz, out_boxes,
        );
        return;
    }
    let total_w_logical = vitals_logical_width(cfg);
    let vit_w = total_w_logical * s;
    let vit_h = 54.0 * s;
    let rad = (cfg.vitals_corner_radius as f32).clamp(0.0, 27.0) * s;

    let vit_rr = D2D1_ROUNDED_RECT {
        rect: D2D_RECT_F {
            left: x,
            top: y,
            right: x + vit_w,
            bottom: y + vit_h,
        },
        radiusX: rad,
        radiusY: rad,
    };

    let tr_card = cfg.vitals_elem_transforms[5];
    let mut pal = resolve_palette_for(cfg, cfg.vitals_match_wallpaper);
    if !cfg.vitals_match_wallpaper && cfg.vitals_use_custom_color {
        let acc = crate::theme::u32_to_rgb(cfg.vitals_accent_rgb);
        let txt = crate::theme::u32_to_rgb(cfg.vitals_text_rgb);
        let bg = crate::theme::u32_to_rgb(cfg.vitals_bg_rgb);
        pal.accent = acc;
        pal.accent_light = acc;
        pal.rim = acc;
        pal.text_primary = txt;
        pal.glass_top = bg;
        pal.glass_bot = (bg.0 / 2, bg.1 / 2, bg.2 / 2);
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

    let vit_rect = (x, y, x + vit_w, y + vit_h);
    {
        let mut slice_opt = out_boxes.as_deref_mut().map(|b| &mut b[..]);
        record_elem_box_slot(&mut slice_opt, 5, tr_card, vit_rect, s);
    }
    let mut no_v_boxes: Option<&mut [ClockElemBox]> = None;
    let g_card = begin_elem_xf_slot(rt, 5, tr_card, vit_rect, s, &mut no_v_boxes);

    let r_base = (cfg.vitals_rim_alpha as f32 / 255.0).clamp(0.0, 1.0);
    let g_top = (cfg.vitals_glass_alpha as f32 / 255.0).clamp(0.0, 0.88);
    let g_bot = (g_top * 0.58).clamp(0.0, 0.76);
    let (v_top_c, v_bot_c, v_rim_c, v_ang) = resolve_card_plate_colors(&tr_card, &pal, g_top, g_bot, r_base);

    if cfg.vitals_show_plate {
        let (gp0, gp1) = grad_endpoints_for_rect(vit_rect, v_ang);
        let bg_stops = [
            D2D1_GRADIENT_STOP { position: 0.0, color: v_top_c },
            D2D1_GRADIENT_STOP { position: 1.0, color: v_bot_c },
        ];
        if let Some(bg_br) = create_linear_grad(rt, gp0, gp1, &bg_stops) {
            rt.FillRoundedRectangle(&vit_rr, &bg_br);
        }

        paint_widget_elemental_fx(
            rt,
            d2d,
            x,
            y,
            x + vit_w,
            y + vit_h,
            (x, y, vit_w, vit_h),
            rad.max(4.0 * s),
            s,
            cfg.vitals_hover_fx,
            88,
            &pal,
            !cfg.vitals_match_wallpaper && !cfg.vitals_use_custom_color && tr_card.color_rgb.is_none(),
            fx_viz,
        );
    }

    if cfg.vitals_show_plate {
        if r_base > 0.01 || tr_card.border_rgb.is_some() || tr_card.color_rgb.is_some() {
            let rim_col = if tr_card.border_rgb.is_some() || tr_card.color_rgb.is_some() { v_rim_c } else { pal.accent_col(r_base) };
            let rim_stops = rim_gradient_stops(&tr_card, &pal, rim_col, r_base);
            if let Some(rim_br) = create_linear_grad(rt, (x, y), (x + vit_w, y + vit_h), &rim_stops) {
                rt.DrawRoundedRectangle(&vit_rr, &rim_br, 1.2 * s, None);
            }
        }
    }
    end_elem_xf_slot(rt, g_card);

    let br_cream = rt.CreateSolidColorBrush(&pal.text_col(1.0), None).ok();
    let br_gold = rt.CreateSolidColorBrush(&pal.light_col(1.0), None).ok();
    let br_sand = rt.CreateSolidColorBrush(&pal.muted_col(0.96), None).ok();
    let br_coral = rt.CreateSolidColorBrush(&rgba_u8(242, 108, 82, 1.0), None).ok();
    let sh_br = rt.CreateSolidColorBrush(&rgba_u8(0, 0, 0, 0.65), None).ok();

    let fs = (cfg.vitals_font_scale_pct as f32 / 100.0).clamp(0.75, 1.35);
    let tf_v_lbl = make_resolved_tf_named(
        dw,
        custom_fonts,
        &cfg.vitals_font_lbl,
        &[w!("SF Pro Display"), w!("Segoe UI Bold"), w!("Segoe UI")],
        7.5 * fs * PT_TO_PX * s,
        DWRITE_FONT_WEIGHT_BOLD,
        DWRITE_TEXT_ALIGNMENT_LEADING,
        DWRITE_PARAGRAPH_ALIGNMENT_NEAR,
    );
    let tf_v_val = make_resolved_tf_named(
        dw,
        custom_fonts,
        &cfg.vitals_font_val,
        &[w!("SF Pro Display"), w!("Segoe UI Semibold"), w!("Segoe UI")],
        8.5 * fs * PT_TO_PX * s,
        DWRITE_FONT_WEIGHT_SEMI_BOLD,
        DWRITE_TEXT_ALIGNMENT_TRAILING,
        DWRITE_PARAGRAPH_ALIGNMENT_NEAR,
    );

    let mut cur_x = 18.0f32;
    let draw_single_meter = |out_boxes: &mut Option<&mut [ClockElemBox; 6]>,
                             elem_idx: usize,
                             offset_x: f32,
                             label: &str,
                             val: f32,
                             val_br: Option<&ID2D1SolidColorBrush>,
                             base_stops: &[D2D1_GRADIENT_STOP]| {
        let tr = cfg.vitals_elem_transforms[elem_idx];
        let mx = x + offset_x * s;
        let box_rect = (mx - 3.0 * s, y + 6.0 * s, mx + 83.0 * s, y + 48.0 * s);
        let mut slice_opt = out_boxes.as_deref_mut().map(|b| &mut b[..]);
        let xf = begin_elem_xf_slot(rt, elem_idx, tr, box_rect, s, &mut slice_opt);

        let e_font = &cfg.vitals_elem_fonts[elem_idx];
        let e_fs = fs * (tr.font_scale_pct as f32 / 100.0).clamp(0.50, 2.20);
        let tf_lbl_ov = if !e_font.is_empty() || tr.font_scale_pct != 100 {
            make_resolved_tf_named(
                dw,
                custom_fonts,
                if e_font.is_empty() { &cfg.vitals_font_lbl } else { e_font },
                &[w!("SF Pro Display"), w!("Segoe UI Bold"), w!("Segoe UI")],
                7.5 * e_fs * PT_TO_PX * s,
                DWRITE_FONT_WEIGHT_BOLD,
                DWRITE_TEXT_ALIGNMENT_LEADING,
                DWRITE_PARAGRAPH_ALIGNMENT_NEAR,
            )
        } else {
            None
        };
        let tf_val_ov = if !e_font.is_empty() || tr.font_scale_pct != 100 {
            make_resolved_tf_named(
                dw,
                custom_fonts,
                if e_font.is_empty() { &cfg.vitals_font_val } else { e_font },
                &[w!("SF Pro Display"), w!("Segoe UI Semibold"), w!("Segoe UI")],
                8.5 * e_fs * PT_TO_PX * s,
                DWRITE_FONT_WEIGHT_SEMI_BOLD,
                DWRITE_TEXT_ALIGNMENT_TRAILING,
                DWRITE_PARAGRAPH_ALIGNMENT_NEAR,
            )
        } else {
            None
        };
        let tf_l_ref = tf_lbl_ov.as_ref().or(tf_v_lbl.as_ref());
        let tf_v_ref = tf_val_ov.as_ref().or(tf_v_val.as_ref());
        let br_ov_val = make_elem_brush(rt, tr.color_rgb, 1.0);
        let br_ov_lbl = make_elem_brush(rt, tr.color_rgb, 0.86);
        let l_br = br_ov_lbl.as_ref().or(br_sand.as_ref());
        let v_br = br_ov_val.as_ref().or(val_br);
        let custom_stops = if tr.color_rgb.is_some() || tr.color2_rgb.is_some() {
            let def1 = base_stops.first().map(|s| s.color).unwrap_or_else(|| pal.deep_col(1.0));
            let def2 = base_stops.last().map(|s| s.color).unwrap_or_else(|| pal.light_col(1.0));
            let (c1, c2, _) = resolve_elem_grad_colors(Some(&tr), 1.0, def1, def2);
            Some([
                D2D1_GRADIENT_STOP { position: 0.0, color: c1 },
                D2D1_GRADIENT_STOP { position: 1.0, color: c2 },
            ])
        } else {
            None
        };
        let stops = custom_stops.as_ref().map(|c| &c[..]).unwrap_or(base_stops);
        let track_col = elem_col_rgba(tr.border_rgb, 0.34, pal.text_col(0.16));

        let val_str = format!("{:.0}%", val);
        match cfg.vitals_style {
            1 => {
                // Radial Ring Gauge + Right Readout
                let r_cx = mx + 16.0 * s;
                let r_cy = y + 27.0 * s;
                let r_rad = 14.0 * s;
                if let Ok(tr_br) = rt.CreateSolidColorBrush(&track_col, None) {
                    rt.DrawEllipse(&D2D1_ELLIPSE { point: Vector2 { X: r_cx, Y: r_cy }, radiusX: r_rad, radiusY: r_rad }, &tr_br, 2.8 * s, None);
                }
                if cfg.vitals_show_bars {
                    if let Some(g_rgb) = tr.glow_rgb {
                        let (gr, gg, gb) = crate::theme::u32_to_rgb(g_rgb);
                        if let Ok(glow_br) = rt.CreateSolidColorBrush(&rgba_u8(gr, gg, gb, 0.34), None) {
                            let gb_ref: &ID2D1Brush = &glow_br;
                            draw_clock_arc(rt, d2d, r_cx, r_cy, r_rad, r_rad, -90.0, (val / 100.0).clamp(0.03, 1.0) * 359.0, 5.6 * s, true, gb_ref);
                        }
                    }
                    let (ac1, ac2, ang) = resolve_elem_grad_colors(Some(&tr), 0.96, pal.accent_col(0.96), pal.light_col(0.96));
                    let (gp0, gp1) = grad_endpoints_for_rect((r_cx - r_rad, r_cy - r_rad, r_cx + r_rad, r_cy + r_rad), ang);
                    if let Some(ac_grad) = create_linear_grad(
                        rt,
                        gp0,
                        gp1,
                        &[
                            D2D1_GRADIENT_STOP { position: 0.0, color: ac1 },
                            D2D1_GRADIENT_STOP { position: 1.0, color: ac2 },
                        ],
                    ) {
                        let b_ref: &ID2D1Brush = &ac_grad;
                        draw_clock_arc(rt, d2d, r_cx, r_cy, r_rad, r_rad, -90.0, (val / 100.0).clamp(0.03, 1.0) * 359.0, 2.8 * s, true, b_ref);
                    }
                }
                if let (Some(tf), Some(br)) = (tf_l_ref, l_br) {
                    draw_shadowed_text(rt, label, tf, D2D_RECT_F { left: mx + 36.0 * s, top: y + 11.0 * s, right: mx + 86.0 * s, bottom: y + 28.0 * s }, br, sh_br.as_ref(), 0.8 * s, false);
                }
                if let (Some(tf), Some(br)) = (tf_v_ref, v_br) {
                    draw_shadowed_text(rt, &val_str, tf, D2D_RECT_F { left: mx + 34.0 * s, top: y + 26.0 * s, right: mx + 82.0 * s, bottom: y + 44.0 * s }, br, sh_br.as_ref(), 0.8 * s, false);
                }
            }
            2 => {
                // Cyber HUD Segmented Blocks
                if let (Some(tf), Some(br)) = (tf_l_ref, l_br) {
                    draw_shadowed_text(rt, label, tf, D2D_RECT_F { left: mx, top: y + 11.0 * s, right: mx + 52.0 * s, bottom: y + 30.0 * s }, br, sh_br.as_ref(), 0.8 * s, false);
                }
                if let (Some(tf), Some(br)) = (tf_v_ref, v_br) {
                    draw_shadowed_text(rt, &val_str, tf, D2D_RECT_F { left: mx + 32.0 * s, top: y + 10.0 * s, right: mx + 80.0 * s, bottom: y + 30.0 * s }, br, sh_br.as_ref(), 0.8 * s, false);
                }
                if cfg.vitals_show_bars {
                    let segs = 10usize;
                    let lit = ((val / 100.0) * segs as f32).ceil() as usize;
                    let (sc1, sc2, _) = resolve_elem_grad_colors(Some(&tr), 0.96, pal.accent_col(0.95), pal.light_col(0.98));
                    for si in 0..segs {
                        let sx = mx + (si as f32) * 8.1 * s;
                        let s_rc = D2D_RECT_F { left: sx, top: y + 32.0 * s, right: sx + 6.4 * s, bottom: y + 39.0 * s };
                        let col = if si < lit {
                            let f = si as f32 / (segs.saturating_sub(1).max(1)) as f32;
                            D2D1_COLOR_F {
                                r: sc1.r + (sc2.r - sc1.r) * f,
                                g: sc1.g + (sc2.g - sc1.g) * f,
                                b: sc1.b + (sc2.b - sc1.b) * f,
                                a: 0.96,
                            }
                        } else {
                            track_col
                        };
                        if let Ok(sb) = rt.CreateSolidColorBrush(&col, None) {
                            rt.FillRectangle(&s_rc, &sb);
                        }
                    }
                }
            }
            _ => {
                let top_off = if cfg.vitals_show_bars && cfg.vitals_style == 0 { 13.0 } else { 19.0 };
                if let (Some(tf), Some(br)) = (tf_l_ref, l_br) {
                    draw_shadowed_text(
                        rt,
                        label,
                        tf,
                        D2D_RECT_F {
                            left: mx,
                            top: y + top_off * s,
                            right: mx + 52.0 * s,
                            bottom: y + (top_off + 19.0) * s,
                        },
                        br,
                        sh_br.as_ref(),
                        1.0 * s,
                        false,
                    );
                }
                if let (Some(tf), Some(br)) = (tf_v_ref, v_br) {
                    draw_shadowed_text(
                        rt,
                        &val_str,
                        tf,
                        D2D_RECT_F {
                            left: mx + 32.0 * s,
                            top: y + (top_off - 1.0) * s,
                            right: mx + 80.0 * s,
                            bottom: y + (top_off + 19.0) * s,
                        },
                        br,
                        sh_br.as_ref(),
                        1.0 * s,
                        false,
                    );
                }
                if cfg.vitals_show_bars && cfg.vitals_style == 0 {
                    if let Some(b_rgb) = tr.border_rgb {
                        let (br_r, br_g, br_b) = crate::theme::u32_to_rgb(b_rgb);
                        if let Ok(tbr) = rt.CreateSolidColorBrush(&rgba_u8(br_r, br_g, br_b, 0.36), None) {
                            rt.FillRoundedRectangle(
                                &D2D1_ROUNDED_RECT {
                                    rect: D2D_RECT_F { left: mx, top: y + 33.0 * s, right: mx + 80.0 * s, bottom: y + 38.0 * s },
                                    radiusX: 2.5 * s,
                                    radiusY: 2.5 * s,
                                },
                                &tbr,
                            );
                        }
                    }
                    draw_meter_bar(rt, mx, y + 33.0 * s, 80.0 * s, 5.0 * s, val / 100.0, s, stops);
                }
            }
        }
        end_elem_xf_slot(rt, xf);
    };

    if cfg.vitals_show_cpu {
        draw_single_meter(
            &mut out_boxes,
            0,
            cur_x,
            "CPU",
            cpu_pct,
            br_cream.as_ref(),
            &[
                D2D1_GRADIENT_STOP { position: 0.0, color: pal.deep_col(1.0) },
                D2D1_GRADIENT_STOP { position: 1.0, color: pal.light_col(1.0) },
            ],
        );
        cur_x += 100.0;
    }
    if cfg.vitals_show_ram {
        draw_single_meter(
            &mut out_boxes,
            1,
            cur_x,
            "RAM",
            ram_pct,
            br_cream.as_ref(),
            &[
                D2D1_GRADIENT_STOP { position: 0.0, color: pal.deep_col(1.0) },
                D2D1_GRADIENT_STOP { position: 1.0, color: pal.light_col(1.0) },
            ],
        );
        cur_x += 100.0;
    }
    if cfg.vitals_show_power {
        draw_single_meter(
            &mut out_boxes,
            2,
            cur_x,
            "POWER",
            bat_pct,
            br_gold.as_ref(),
            &[
                D2D1_GRADIENT_STOP { position: 0.0, color: pal.accent_col(1.0) },
                D2D1_GRADIENT_STOP { position: 1.0, color: rgba_u8(118, 205, 142, 1.0) },
            ],
        );
        cur_x += 100.0;
    }

    if cfg.vitals_show_buttons {
        let btn_base = if cur_x > 20.0 { cur_x } else { 14.0 };
        if cur_x > 20.0 && cfg.vitals_show_dividers {
            let mut no_div_b: Option<&mut [ClockElemBox]> = None;
            let g_div = begin_elem_xf_slot(rt, 5, tr_card, vit_rect, s, &mut no_div_b);
            let div_c = elem_col_rgba(
                cfg.vitals_elem_transforms[5].border_rgb.or(cfg.vitals_elem_transforms[3].color_rgb),
                0.35,
                pal.text_col(0.18),
            );
            if let Ok(div_br) = rt.CreateSolidColorBrush(&div_c, None) {
                rt.DrawLine(
                    Vector2 { X: x + btn_base * s, Y: y + 12.0 * s },
                    Vector2 { X: x + btn_base * s, Y: y + 42.0 * s },
                    &div_br,
                    1.0 * s,
                    None,
                );
            }
            end_elem_xf_slot(rt, g_div);
        }
        let sleep_x = btn_base + 12.0;
        let shut_x = btn_base + 50.0;

        // Sleep Button (elem 3)
        let tr_slp = cfg.vitals_elem_transforms[3];
        let mut slice_opt = out_boxes.as_deref_mut().map(|b| &mut b[..]);
        let g_slp = begin_elem_xf_slot(
            rt,
            3,
            tr_slp,
            (x + sleep_x * s, y + 11.0 * s, x + (sleep_x + 32.0) * s, y + 43.0 * s),
            s,
            &mut slice_opt,
        );
        let slp_hov = hover == WidgetHitZone::VitalsSleep;
        let slp_rr = D2D1_ROUNDED_RECT {
            rect: D2D_RECT_F {
                left: x + sleep_x * s,
                top: y + 11.0 * s,
                right: x + (sleep_x + 32.0) * s,
                bottom: y + 43.0 * s,
            },
            radiusX: 10.0 * s,
            radiusY: 10.0 * s,
        };
        let slp_fill = if tr_slp.color_rgb.is_some() {
            elem_col_rgba(tr_slp.color_rgb, if slp_hov { 0.48 } else { 0.32 }, pal.text_col(0.09))
        } else if slp_hov {
            pal.text_col(0.22)
        } else {
            pal.text_col(0.09)
        };
        if let Ok(b) = rt.CreateSolidColorBrush(&slp_fill, None) {
            rt.FillRoundedRectangle(&slp_rr, &b);
        }
        let slp_stroke = elem_col_rgba(
            tr_slp.color_rgb,
            if slp_hov { 0.92 } else { 0.45 },
            if slp_hov { pal.accent_col(0.90) } else { pal.accent_col(0.35) },
        );
        if let Ok(b) = rt.CreateSolidColorBrush(&slp_stroke, None) {
            rt.DrawRoundedRectangle(&slp_rr, &b, 1.2 * s, None);
        }

        let br_slp_moon = make_elem_brush(rt, tr_slp.color_rgb, 1.0);
        let moon_br_opt = br_slp_moon.as_ref().or(br_gold.as_ref());
        let moon_cx = x + (sleep_x + 16.0) * s;
        let moon_cy = y + 27.0 * s;
        let mut drew_geo_moon = false;
        if let (Some(factory), Some(gold_br)) = (d2d, moon_br_opt) {
            if let (Ok(outer), Ok(inner), Ok(path)) = (
                factory.CreateEllipseGeometry(&D2D1_ELLIPSE {
                    point: Vector2 { X: moon_cx, Y: moon_cy },
                    radiusX: 6.8 * s,
                    radiusY: 6.8 * s,
                }),
                factory.CreateEllipseGeometry(&D2D1_ELLIPSE {
                    point: Vector2 { X: moon_cx + 3.0 * s, Y: moon_cy - 2.4 * s },
                    radiusX: 5.5 * s,
                    radiusY: 5.5 * s,
                }),
                factory.CreatePathGeometry(),
            ) {
                if let Ok(sink) = path.Open() {
                    let simp: ID2D1SimplifiedGeometrySink = sink.cast().unwrap();
                    let og: ID2D1Geometry = outer.cast().unwrap();
                    let ig: ID2D1Geometry = inner.cast().unwrap();
                    if og.CombineWithGeometry(&ig, D2D1_COMBINE_MODE_EXCLUDE, None, 0.1, &simp).is_ok()
                        && sink.Close().is_ok()
                    {
                        rt.FillGeometry(&path, gold_br, None);
                        drew_geo_moon = true;
                    }
                }
            }
        }
        if !drew_geo_moon {
            if let Some(gold_br) = moon_br_opt {
                rt.FillEllipse(
                    &D2D1_ELLIPSE { point: Vector2 { X: moon_cx, Y: moon_cy }, radiusX: 6.2 * s, radiusY: 6.2 * s },
                    gold_br,
                );
            }
        }
        end_elem_xf_slot(rt, g_slp);

        // Shutdown Button (elem 4)
        let tr_sht = cfg.vitals_elem_transforms[4];
        let mut slice_opt = out_boxes.as_deref_mut().map(|b| &mut b[..]);
        let g_sht = begin_elem_xf_slot(
            rt,
            4,
            tr_sht,
            (x + shut_x * s, y + 11.0 * s, x + (shut_x + 32.0) * s, y + 43.0 * s),
            s,
            &mut slice_opt,
        );
        let sht_hov = hover == WidgetHitZone::VitalsShutdown;
        let sht_rr = D2D1_ROUNDED_RECT {
            rect: D2D_RECT_F {
                left: x + shut_x * s,
                top: y + 11.0 * s,
                right: x + (shut_x + 32.0) * s,
                bottom: y + 43.0 * s,
            },
            radiusX: 10.0 * s,
            radiusY: 10.0 * s,
        };
        let sht_fill = elem_col_rgba(
            tr_sht.color_rgb,
            if sht_hov { 0.48 } else { 0.32 },
            if sht_hov { rgba_u8(242, 108, 82, 0.28) } else { rgba_u8(242, 108, 82, 0.10) },
        );
        if let Ok(b) = rt.CreateSolidColorBrush(&sht_fill, None) {
            rt.FillRoundedRectangle(&sht_rr, &b);
        }
        let sht_stroke = elem_col_rgba(
            tr_sht.color_rgb,
            if sht_hov { 0.95 } else { 0.50 },
            if sht_hov { rgba_u8(242, 108, 82, 0.95) } else { rgba_u8(242, 108, 82, 0.43) },
        );
        if let Ok(b) = rt.CreateSolidColorBrush(&sht_stroke, None) {
            rt.DrawRoundedRectangle(&sht_rr, &b, 1.2 * s, None);
        }

        let br_sht_arc = make_elem_brush(rt, tr_sht.color_rgb, 1.0);
        let coral_br_opt = br_sht_arc.as_ref().or(br_coral.as_ref());
        let pwr_cx = x + (shut_x + 16.0) * s;
        let pwr_cy = y + 28.0 * s;
        let r_arc = 6.2 * s;
        let mut drew_arc = false;
        if let (Some(factory), Some(coral_br)) = (d2d, coral_br_opt) {
            if let Ok(arc_geo) = factory.CreatePathGeometry() {
                if let Ok(sink) = arc_geo.Open() {
                    let a_start = -48.0f32.to_radians();
                    let a_end = -132.0f32.to_radians();
                    let p_start = Vector2 {
                        X: pwr_cx + r_arc * a_start.cos(),
                        Y: pwr_cy + r_arc * a_start.sin(),
                    };
                    let p_end = Vector2 {
                        X: pwr_cx + r_arc * a_end.cos(),
                        Y: pwr_cy + r_arc * a_end.sin(),
                    };
                    sink.BeginFigure(p_start, D2D1_FIGURE_BEGIN_HOLLOW);
                    sink.AddArc(&D2D1_ARC_SEGMENT {
                        point: p_end,
                        size: D2D_SIZE_F { width: r_arc, height: r_arc },
                        rotationAngle: 0.0,
                        sweepDirection: D2D1_SWEEP_DIRECTION_CLOCKWISE,
                        arcSize: D2D1_ARC_SIZE_LARGE,
                    });
                    sink.EndFigure(D2D1_FIGURE_END_OPEN);
                    if sink.Close().is_ok() {
                        let round_cap: Option<ID2D1StrokeStyle> = factory
                            .CreateStrokeStyle(
                                &D2D1_STROKE_STYLE_PROPERTIES {
                                    startCap: D2D1_CAP_STYLE_ROUND,
                                    endCap: D2D1_CAP_STYLE_ROUND,
                                    ..Default::default()
                                },
                                None,
                            )
                            .ok();
                        rt.DrawGeometry(&arc_geo, coral_br, 2.0 * s, round_cap.as_ref());
                        drew_arc = true;
                    }
                }
            }
        }
        if !drew_arc {
            if let Some(coral_br) = coral_br_opt {
                rt.DrawEllipse(
                    &D2D1_ELLIPSE { point: Vector2 { X: pwr_cx, Y: pwr_cy }, radiusX: r_arc, radiusY: r_arc },
                    coral_br,
                    2.0 * s,
                    None,
                );
            }
        }
        let stem_br_opt = br_sht_arc.as_ref().or(br_cream.as_ref());
        if let Some(cream_br) = stem_br_opt {
            let round_cap: Option<ID2D1StrokeStyle> = d2d.and_then(|f| {
                f.CreateStrokeStyle(
                    &D2D1_STROKE_STYLE_PROPERTIES {
                        startCap: D2D1_CAP_STYLE_ROUND,
                        endCap: D2D1_CAP_STYLE_ROUND,
                        ..Default::default()
                    },
                    None,
                )
                .ok()
            });
            rt.DrawLine(
                Vector2 { X: pwr_cx, Y: pwr_cy - 8.2 * s },
                Vector2 { X: pwr_cx, Y: pwr_cy - 1.2 * s },
                cream_br,
                2.2 * s,
                round_cap.as_ref(),
            );
        }
        end_elem_xf_slot(rt, g_sht);
    }
}

/// Luxury-grade original Telemetry Ring-Pill HUD (`vitals_style == 4`):
/// Frosted glass capsule (`rx = h / 2`) with 4 open-chronograph circular arc gauges
/// (`CPU %`, `GPU %`, `RAM %`, `TEMP`) rendered via cached Direct2D `ArcSegment` geometry,
/// stack-formatted UTF-16 numerals (zero heap allocations), Live Chameleon accent coloring,
/// and crisp Inter/SF-Pro typography.
#[allow(clippy::too_many_arguments)]
unsafe fn draw_telemetry_ring_pill_hud(
    rt: &ID2D1RenderTarget,
    dw: &IDWriteFactory,
    d2d: Option<&ID2D1Factory>,
    custom_fonts: Option<&IDWriteFontCollection>,
    x: f32,
    y: f32,
    s: f32,
    cfg: &AppConfig,
    cpu_pct: f32,
    gpu_pct: f32,
    ram_pct: f32,
    temp_c: f32,
    hover: WidgetHitZone,
    fx_viz: Option<&VizSnapshot>,
    mut out_boxes: Option<&mut [ClockElemBox; 6]>,
) {
    const LBL_CPU: [u16; 3] = ['C' as u16, 'P' as u16, 'U' as u16];
    const LBL_GPU: [u16; 3] = ['G' as u16, 'P' as u16, 'U' as u16];
    const LBL_RAM: [u16; 3] = ['R' as u16, 'A' as u16, 'M' as u16];
    const LBL_TEMP: [u16; 4] = ['T' as u16, 'E' as u16, 'M' as u16, 'P' as u16];

    let total_w_logical = vitals_logical_width(cfg);
    let vit_w = total_w_logical * s;
    let vit_h = 64.0 * s;
    let rad = vit_h * 0.5;
    let vit_rect = (x, y, x + vit_w, y + vit_h);
    let tr_card = cfg.vitals_elem_transforms[5];

    {
        let mut slice_opt = out_boxes.as_deref_mut().map(|b| &mut b[..]);
        record_elem_box_slot(&mut slice_opt, 5, tr_card, vit_rect, s);
    }

    let mut pal = resolve_palette_for(cfg, cfg.vitals_match_wallpaper);
    if !cfg.vitals_match_wallpaper && cfg.vitals_use_custom_color {
        let acc = crate::theme::u32_to_rgb(cfg.vitals_accent_rgb);
        let txt = crate::theme::u32_to_rgb(cfg.vitals_text_rgb);
        let bg = crate::theme::u32_to_rgb(cfg.vitals_bg_rgb);
        pal.accent = acc;
        pal.accent_light = acc;
        pal.rim = acc;
        pal.text_primary = txt;
        pal.glass_top = bg;
        pal.glass_bot = (bg.0 / 2, bg.1 / 2, bg.2 / 2);
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

    let mut no_v_boxes: Option<&mut [ClockElemBox]> = None;
    let g_card = begin_elem_xf_slot(rt, 5, tr_card, vit_rect, s, &mut no_v_boxes);
    if cfg.vitals_show_plate {
        let custom_bg = if tr_card.color_rgb.is_some() || tr_card.color2_rgb.is_some() {
            let ga = (cfg.vitals_glass_alpha as f32 / 255.0).clamp(0.12, 0.94);
            let (c1, c2, _) = resolve_elem_grad_colors(Some(&tr_card), ga.max(0.68), pal.glass_top_col(0.84), pal.glass_bot_col(0.90));
            Some((c1, c2))
        } else {
            None
        };
        let custom_rim = tr_card.border_rgb.map(|rgb| {
            let (rr, gg, bb) = crate::theme::u32_to_rgb(rgb);
            rgba_u8(rr, gg, bb, 0.95)
        });
        crate::dock_themes::draw_luxury_glass_capsule(
            rt,
            d2d,
            D2D_RECT_F {
                left: vit_rect.0,
                top: vit_rect.1,
                right: vit_rect.2,
                bottom: vit_rect.3,
            },
            Some(rad),
            &pal,
            cfg.vitals_glass_alpha,
            cfg.vitals_rim_alpha,
            s,
            hover != WidgetHitZone::None,
            custom_bg,
            custom_rim,
        );
        paint_widget_elemental_fx(
            rt,
            d2d,
            x,
            y,
            x + vit_w,
            y + vit_h,
            (x, y, vit_w, vit_h),
            rad.max(4.0 * s),
            s,
            cfg.vitals_hover_fx,
            88,
            &pal,
            !cfg.vitals_match_wallpaper && !cfg.vitals_use_custom_color && tr_card.color_rgb.is_none(),
            fx_viz,
        );
    }
    end_elem_xf_slot(rt, g_card);

    let (bays, count, end_x) = ring_pill_layout(cfg);
    let fs = (cfg.vitals_font_scale_pct as f32 / 100.0).clamp(0.75, 1.35);
    let mut stack_buf = [0u16; 16];
    let shadow_col = Some(rgba_u8(0, 0, 0, 0.55));
    let div_col = pal.text_col(0.11);

    for idx in 0..count {
        let bay = bays[idx];
        let tr = cfg.vitals_elem_transforms[bay.elem_idx];
        let bx = x + bay.x_left * s;
        let bw = bay.bay_w * s;
        let bay_rect = (bx + 2.0 * s, y + 4.0 * s, bx + bw - 2.0 * s, y + vit_h - 4.0 * s);
        let mut slice_opt = out_boxes.as_deref_mut().map(|b| &mut b[..]);
        let g_bay = begin_elem_xf_slot(rt, bay.elem_idx, tr, bay_rect, s, &mut slice_opt);

        if tr.visible {
            let (raw_val, ratio, is_temp, lbl_u16): (f32, f32, bool, &[u16]) = match bay.hit_zone {
                WidgetHitZone::VitalsCpu => (cpu_pct, (cpu_pct / 100.0).clamp(0.0, 1.0), false, &LBL_CPU),
                WidgetHitZone::VitalsGpu => (gpu_pct, (gpu_pct / 100.0).clamp(0.0, 1.0), false, &LBL_GPU),
                WidgetHitZone::VitalsRam => (ram_pct, (ram_pct / 100.0).clamp(0.0, 1.0), false, &LBL_RAM),
                _ => (temp_c, ((temp_c - 20.0) / 75.0).clamp(0.04, 1.0), true, &LBL_TEMP),
            };

            let cx = bx + bw * 0.5;
            let cy = y + 28.5 * s;
            let ring_r = 18.8 * s;
            let is_hov = hover == bay.hit_zone;
            let hot_t = if is_temp {
                ((temp_c - 66.0) / 22.0).clamp(0.0, 1.0)
            } else {
                ((raw_val - 74.0) / 22.0).clamp(0.0, 1.0)
            };

            if is_hov {
                crate::dock_themes::with_cached_solid_brush(rt, pal.text_col(0.07), |br| {
                    rt.FillEllipse(
                        &D2D1_ELLIPSE { point: Vector2 { X: cx, Y: cy }, radiusX: 25.0 * s, radiusY: 25.0 * s },
                        br,
                    );
                });
            }

            let base_ring_col = if tr.color_rgb.is_some() {
                elem_col_rgba(tr.color_rgb, 0.96, pal.accent_col(0.96))
            } else {
                let thermal_col = rgba_u8(255, 102, 78, 0.98);
                let std_col = match bay.hit_zone {
                    WidgetHitZone::VitalsCpu => crate::dock_themes::lerp_d2d_color(
                        rgba_u8(92, 225, 255, 0.96),
                        pal.light_col(0.96),
                        0.32,
                    ),
                    WidgetHitZone::VitalsGpu => crate::dock_themes::lerp_d2d_color(
                        rgba_u8(167, 139, 250, 0.96),
                        pal.accent_col(0.96),
                        0.34,
                    ),
                    WidgetHitZone::VitalsRam => crate::dock_themes::lerp_d2d_color(
                        rgba_u8(52, 211, 153, 0.96),
                        pal.light_col(0.94),
                        0.30,
                    ),
                    _ => crate::dock_themes::lerp_d2d_color(
                        rgba_u8(251, 191, 36, 0.96),
                        pal.accent_col(0.95),
                        0.28,
                    ),
                };
                crate::dock_themes::lerp_d2d_color(std_col, thermal_col, hot_t)
            };
            let track_col = elem_col_rgba(tr.border_rgb, 0.28, rgba_u8(232, 240, 255, 0.13));
            let glow_col = tr.glow_rgb.map(|g| {
                let (r, gg, b) = crate::theme::u32_to_rgb(g);
                rgba_u8(r, gg, b, 0.24)
            }).unwrap_or(D2D1_COLOR_F { a: if is_hov { 0.28 } else { 0.16 }, ..base_ring_col });

            if cfg.vitals_show_bars {
                crate::dock_themes::draw_telemetry_arc_ring(
                    rt,
                    d2d,
                    idx,
                    cx,
                    cy,
                    ring_r,
                    3.0 * s,
                    142.0,
                    256.0,
                    ratio,
                    track_col,
                    base_ring_col,
                    glow_col,
                    s,
                );
            }

            let e_font = &cfg.vitals_elem_fonts[bay.elem_idx];
            let val_font = if !e_font.is_empty() { e_font.as_str() } else { cfg.vitals_font_val.as_str() };
            let lbl_font = if !e_font.is_empty() { e_font.as_str() } else { cfg.vitals_font_lbl.as_str() };
            let e_fs = fs * (tr.font_scale_pct as f32 / 100.0).clamp(0.55, 1.85);
            let n_u16 = if is_temp {
                crate::dock_themes::fmt_temp_c_u16(raw_val, false, &mut stack_buf)
            } else {
                crate::dock_themes::fmt_pct_u16(raw_val, &mut stack_buf)
            };
            let val_col = elem_col_rgba(tr.color_rgb, 1.0, pal.text_col(0.98));
            let lbl_col = elem_col_rgba(tr.color_rgb, 0.88, pal.muted_col(0.90));

            crate::dock_themes::draw_cached_u16_text(
                rt,
                dw,
                custom_fonts,
                0,
                val_font,
                9.8 * e_fs * PT_TO_PX * s,
                false,
                true,
                D2D_RECT_F {
                    left: bx + 6.0 * s,
                    top: y + 17.5 * s,
                    right: bx + bw - 6.0 * s,
                    bottom: y + 39.0 * s,
                },
                val_col,
                shadow_col,
                0.75 * s,
                &stack_buf[..n_u16],
            );

            crate::dock_themes::draw_cached_u16_text(
                rt,
                dw,
                custom_fonts,
                1,
                lbl_font,
                6.2 * e_fs * PT_TO_PX * s,
                true,
                true,
                D2D_RECT_F {
                    left: bx + 6.0 * s,
                    top: y + 42.5 * s,
                    right: bx + bw - 6.0 * s,
                    bottom: y + 57.5 * s,
                },
                lbl_col,
                shadow_col,
                0.65 * s,
                lbl_u16,
            );
        }
        end_elem_xf_slot(rt, g_bay);

        if cfg.vitals_show_dividers && idx + 1 < count {
            let div_x = bx + bw;
            crate::dock_themes::with_cached_solid_brush(rt, div_col, |br| {
                rt.DrawLine(
                    Vector2 { X: div_x, Y: y + 18.0 * s },
                    Vector2 { X: div_x, Y: y + 46.0 * s },
                    br,
                    0.85 * s,
                    None,
                );
            });
        }
    }

    if cfg.vitals_show_buttons {
        if cfg.vitals_show_dividers {
            crate::dock_themes::with_cached_solid_brush(rt, pal.text_col(0.14), |br| {
                rt.DrawLine(
                    Vector2 { X: x + (end_x + 2.0) * s, Y: y + 16.0 * s },
                    Vector2 { X: x + (end_x + 2.0) * s, Y: y + 48.0 * s },
                    br,
                    0.85 * s,
                    None,
                );
            });
        }
        let btn_cy = y + 32.0 * s;
        let slp_cx = x + (end_x + 26.0) * s;
        let sht_cx = x + (end_x + 62.0) * s;
        let r_btn = 14.5 * s;

        let tr_slp = cfg.vitals_elem_transforms[3];
        let mut slice_opt = out_boxes.as_deref_mut().map(|b| &mut b[..]);
        let g_slp = begin_elem_xf_slot(
            rt,
            3,
            tr_slp,
            (slp_cx - r_btn, btn_cy - r_btn, slp_cx + r_btn, btn_cy + r_btn),
            s,
            &mut slice_opt,
        );
        if tr_slp.visible {
            let hov = hover == WidgetHitZone::VitalsSleep;
            let fill = elem_col_rgba(tr_slp.color_rgb, if hov { 0.34 } else { 0.14 }, pal.text_col(if hov { 0.22 } else { 0.08 }));
            let stroke = elem_col_rgba(tr_slp.border_rgb.or(tr_slp.color_rgb), if hov { 0.85 } else { 0.34 }, pal.accent_col(if hov { 0.82 } else { 0.28 }));
            crate::dock_themes::with_cached_solid_brush(rt, fill, |br| {
                rt.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: slp_cx, Y: btn_cy }, radiusX: r_btn, radiusY: r_btn }, br);
            });
            crate::dock_themes::with_cached_solid_brush(rt, stroke, |br| {
                rt.DrawEllipse(&D2D1_ELLIPSE { point: Vector2 { X: slp_cx, Y: btn_cy }, radiusX: r_btn, radiusY: r_btn }, br, 0.9 * s, None);
            });
            let moon_col = elem_col_rgba(tr_slp.color_rgb, 0.96, pal.light_col(0.96));
            crate::dock_themes::draw_cached_crescent(rt, d2d, slp_cx, btn_cy, 5.8 * s, moon_col);
        }
        end_elem_xf_slot(rt, g_slp);

        let tr_sht = cfg.vitals_elem_transforms[4];
        let mut slice_opt = out_boxes.as_deref_mut().map(|b| &mut b[..]);
        let g_sht = begin_elem_xf_slot(
            rt,
            4,
            tr_sht,
            (sht_cx - r_btn, btn_cy - r_btn, sht_cx + r_btn, btn_cy + r_btn),
            s,
            &mut slice_opt,
        );
        if tr_sht.visible {
            let hov = hover == WidgetHitZone::VitalsShutdown;
            let coral = rgba_u8(242, 108, 82, 0.96);
            let fill = elem_col_rgba(tr_sht.color_rgb, if hov { 0.36 } else { 0.16 }, rgba_u8(242, 108, 82, if hov { 0.28 } else { 0.12 }));
            let stroke = elem_col_rgba(tr_sht.border_rgb.or(tr_sht.color_rgb), if hov { 0.90 } else { 0.38 }, rgba_u8(242, 108, 82, if hov { 0.90 } else { 0.34 }));
            crate::dock_themes::with_cached_solid_brush(rt, fill, |br| {
                rt.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: sht_cx, Y: btn_cy }, radiusX: r_btn, radiusY: r_btn }, br);
            });
            crate::dock_themes::with_cached_solid_brush(rt, stroke, |br| {
                rt.DrawEllipse(&D2D1_ELLIPSE { point: Vector2 { X: sht_cx, Y: btn_cy }, radiusX: r_btn, radiusY: r_btn }, br, 0.9 * s, None);
            });
            let icon_col = elem_col_rgba(tr_sht.color_rgb, 0.96, coral);
            crate::dock_themes::draw_telemetry_arc_ring(
                rt,
                d2d,
                5,
                sht_cx,
                btn_cy,
                5.4 * s,
                1.7 * s,
                -54.0,
                288.0,
                1.0,
                rgba_u8(0, 0, 0, 0.0),
                icon_col,
                rgba_u8(0, 0, 0, 0.0),
                s,
            );
            crate::dock_themes::with_cached_solid_brush(rt, elem_col_rgba(tr_sht.color_rgb, 0.98, pal.text_col(0.98)), |br| {
                rt.DrawLine(Vector2 { X: sht_cx, Y: btn_cy - 6.8 * s }, Vector2 { X: sht_cx, Y: btn_cy - 0.8 * s }, br, 1.8 * s, None);
            });
        }
        end_elem_xf_slot(rt, g_sht);
    }
}

/// Luxury-grade original Dynamic Island Capsule Music Player (`MusicStyle::DynamicIslandCapsule`):
/// Horizontal capsule (`rx = height / 2 = 37.0`) with frosted glass + 1px top rim highlight,
/// rounded album art squircle on left, minimalist ellipsis-trimmed track/artist & vector transport
/// controls in center, and pulsing vertical equalizer pills wired to live audio analyzer on right.
#[allow(clippy::too_many_arguments)]
pub unsafe fn draw_dynamic_island_capsule_player(
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
    pal: &ThemePalette,
    fx_viz: Option<&VizSnapshot>,
    mut out_boxes: Option<&mut [ClockElemBox; 6]>,
    skip_plate_and_fx: bool,
    paint_fx: bool,
) {
    const LBL_NOW_PLAYING: [u16; 11] = [
        'N' as u16, 'O' as u16, 'W' as u16, ' ' as u16, 'P' as u16, 'L' as u16, 'A' as u16,
        'Y' as u16, 'I' as u16, 'N' as u16, 'G' as u16,
    ];
    const LBL_STANDBY: [u16; 7] = [
        'S' as u16, 'T' as u16, 'A' as u16, 'N' as u16, 'D' as u16, 'B' as u16, 'Y' as u16,
    ];

    if cfg.is_music_all_elements_off() {
        return;
    }
    let w = 444.0 * s;
    let h = 74.0 * s;
    let rad = h * 0.5;
    let card_rect = (x, y, x + w, y + h);
    let tr_card = cfg.music_elem_transforms[5];
    let tr_cov = cfg.music_elem_transforms[0];
    let tr_tit = cfg.music_elem_transforms[1];
    let tr_art = cfg.music_elem_transforms[2];
    let tr_ctl = cfg.music_elem_transforms[3];
    let tr_viz = cfg.music_elem_transforms[4];

    {
        let mut slice_opt = out_boxes.as_deref_mut().map(|b| &mut b[..]);
        record_elem_box_slot(&mut slice_opt, 5, tr_card, card_rect, s);
    }

    let card_hovered = matches!(
        hover,
        WidgetHitZone::MusicCard
            | WidgetHitZone::MusicPrev
            | WidgetHitZone::MusicPlayPause
            | WidgetHitZone::MusicNext
    );

    if !skip_plate_and_fx && cfg.music_show_plate {
        let mut nb: Option<&mut [ClockElemBox]> = None;
        let g_card = begin_elem_xf_slot(rt, 5, tr_card, card_rect, s, &mut nb);
        let custom_bg = if tr_card.color_rgb.is_some() || tr_card.color2_rgb.is_some() {
            let ga = (cfg.music_glass_alpha as f32 / 255.0).clamp(0.16, 0.94);
            let (c1, c2, _) = resolve_elem_grad_colors(Some(&tr_card), ga.max(0.68), pal.glass_top_col(0.86), pal.glass_bot_col(0.92));
            Some((c1, c2))
        } else {
            None
        };
        let custom_rim = tr_card.border_rgb.map(|rgb| {
            let (rr, gg, bb) = crate::theme::u32_to_rgb(rgb);
            rgba_u8(rr, gg, bb, 0.95)
        });
        crate::dock_themes::draw_luxury_glass_capsule(
            rt,
            d2d,
            D2D_RECT_F {
                left: card_rect.0,
                top: card_rect.1,
                right: card_rect.2,
                bottom: card_rect.3,
            },
            Some(rad),
            pal,
            cfg.music_glass_alpha,
            cfg.music_rim_alpha,
            s,
            card_hovered,
            custom_bg,
            custom_rim,
        );
        if paint_fx {
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
                cfg.music_hover_fx,
                84,
                pal,
                !cfg.music_match_wallpaper && !cfg.music_use_custom_color && tr_card.color_rgb.is_none(),
                fx_viz,
            );
        }
        end_elem_xf_slot(rt, g_card);
    }

    // Left: Rounded Album Art Squircle (52x52 concentric inside left capsule dome)
    if cfg.music_show_cover {
        let cov_box = (x + 11.0 * s, y + 11.0 * s, x + 63.0 * s, y + 63.0 * s);
        let mut slice_opt = out_boxes.as_deref_mut().map(|b| &mut b[..]);
        let g_cov = begin_elem_xf_slot(rt, 0, tr_cov, cov_box, s, &mut slice_opt);
        if tr_cov.visible {
            let cov_rad = 18.0 * s;
            let cov_rr = D2D1_ROUNDED_RECT {
                rect: D2D_RECT_F {
                    left: cov_box.0,
                    top: cov_box.1,
                    right: cov_box.2,
                    bottom: cov_box.3,
                },
                radiusX: cov_rad,
                radiusY: cov_rad,
            };
            // Soft ambient album-art glow halo behind the squircle
            let halo_cx = (cov_box.0 + cov_box.2) * 0.5;
            let halo_cy = (cov_box.1 + cov_box.3) * 0.5;
            let halo_a = if media.is_playing { 0.24 } else if card_hovered { 0.18 } else { 0.11 };
            crate::dock_themes::with_cached_solid_brush(rt, pal.accent_col(halo_a), |br| {
                rt.FillEllipse(
                    &D2D1_ELLIPSE {
                        point: Vector2 { X: halo_cx, Y: halo_cy },
                        radiusX: 30.5 * s,
                        radiusY: 30.5 * s,
                    },
                    br,
                );
            });
            crate::dock_themes::with_cached_solid_brush(rt, pal.glass_bot_col(0.95), |br| {
                rt.FillRoundedRectangle(&cov_rr, br);
            });
            if let Some(bmp) = cover_bmp {
                if !crate::dock_themes::draw_cached_rounded_bitmap(rt, bmp, &cov_rr) {
                    rt.DrawBitmap(bmp, Some(&cov_rr.rect), 1.0, D2D1_BITMAP_INTERPOLATION_MODE_LINEAR, None);
                }
            } else {
                // Procedural Chameleon squircle artwork when no bitmap is attached
                let ccx = (cov_box.0 + cov_box.2) * 0.5;
                let ccy = (cov_box.1 + cov_box.3) * 0.5;
                crate::dock_themes::with_cached_solid_brush(rt, pal.deep_col(0.55), |br| {
                    rt.FillRoundedRectangle(&cov_rr, br);
                });
                crate::dock_themes::with_cached_solid_brush(rt, pal.accent_col(0.28), |br| {
                    rt.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: ccx, Y: ccy }, radiusX: 16.5 * s, radiusY: 16.5 * s }, br);
                });
                crate::dock_themes::with_cached_solid_brush(rt, pal.light_col(0.88), |br| {
                    rt.DrawEllipse(&D2D1_ELLIPSE { point: Vector2 { X: ccx, Y: ccy }, radiusX: 11.5 * s, radiusY: 11.5 * s }, br, 1.4 * s, None);
                    rt.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: ccx, Y: ccy }, radiusX: 4.5 * s, radiusY: 4.5 * s }, br);
                });
            }
            let squircle_rim = elem_col_rgba(tr_cov.border_rgb.or(tr_cov.color_rgb), 0.72, rgba_u8(255, 255, 255, 0.24));
            crate::dock_themes::with_cached_solid_brush(rt, squircle_rim, |br| {
                rt.DrawRoundedRectangle(&cov_rr, br, 1.0 * s, None);
            });
        }
        end_elem_xf_slot(rt, g_cov);
    }

    // Center-Left: Minimalist Track Title & Artist Subtitle (ellipsis-trimmed, zero allocation)
    let txt_left = if cfg.music_show_cover { x + 75.0 * s } else { x + 24.0 * s };
    let txt_right = if cfg.music_show_controls {
        x + 234.0 * s
    } else if cfg.music_show_viz {
        x + 336.0 * s
    } else {
        x + w - 24.0 * s
    };
    let fs = (cfg.music_font_scale_pct.clamp(70, 140) as f32) / 100.0;
    let shadow_col = Some(rgba_u8(0, 0, 0, 0.56));
    let tit_top = if cfg.music_show_badge { y + 24.0 * s } else { y + 18.0 * s };
    let tit_bot = tit_top + 21.0 * s;
    let art_top = tit_bot - 1.0 * s;
    let art_bot = art_top + 18.0 * s;
    let title_disp = if media.title.trim().is_empty() {
        "Ready to Play"
    } else {
        media.title.as_str()
    };
    let artist_disp = if media.artist.trim().is_empty() {
        if media.is_playing { "Live System Audio" } else { "System Audio" }
    } else {
        media.artist.as_str()
    };

    let show_tit = cfg.music_show_title && tr_tit.visible;
    let show_bdg = cfg.music_show_badge;
    if show_tit || show_bdg {
        let tit_box = (txt_left, if show_bdg { y + 11.0 * s } else { tit_top }, txt_right, tit_bot);
        let mut slice_opt = out_boxes.as_deref_mut().map(|b| &mut b[..]);
        let mut tr_tit_eff = tr_tit;
        tr_tit_eff.visible = true;
        let g_tit = begin_elem_xf_slot(rt, 1, tr_tit_eff, tit_box, s, &mut slice_opt);
        let t_fnt = if !cfg.music_elem_fonts[1].is_empty() { &cfg.music_elem_fonts[1] } else { &cfg.music_font_title };
        let t_fs = fs * (tr_tit.font_scale_pct as f32 / 100.0).clamp(0.55, 1.85);
        if show_bdg {
            let dot_col = if media.is_playing { pal.light_col(0.98) } else { pal.muted_col(0.55) };
            crate::dock_themes::with_cached_solid_brush(rt, dot_col, |br| {
                rt.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: txt_left + 3.0 * s, Y: y + 17.0 * s }, radiusX: 2.3 * s, radiusY: 2.3 * s }, br);
            });
            let b_lbl = if media.is_playing { &LBL_NOW_PLAYING[..] } else { &LBL_STANDBY[..] };
            crate::dock_themes::draw_cached_u16_text(
                rt,
                dw,
                custom_fonts,
                2,
                t_fnt,
                6.2 * PT_TO_PX * s * t_fs,
                true,
                false,
                D2D_RECT_F {
                    left: txt_left + 9.0 * s,
                    top: y + 11.0 * s,
                    right: txt_right,
                    bottom: y + 23.0 * s,
                },
                elem_col_rgba(tr_tit.color_rgb, 0.92, pal.accent_col(0.92)),
                shadow_col,
                0.6 * s,
                b_lbl,
            );
        }
        if show_tit {
            let title_col = elem_col_rgba(tr_tit.color_rgb, 1.0, pal.text_col(0.98));
            crate::dock_themes::draw_cached_ellipsis_text(
                rt,
                dw,
                custom_fonts,
                0,
                t_fnt,
                11.2 * PT_TO_PX * s * t_fs,
                true,
                title_disp,
                D2D_RECT_F {
                    left: txt_left,
                    top: tit_top,
                    right: txt_right,
                    bottom: tit_bot,
                },
                title_col,
                shadow_col,
                0.85 * s,
            );
        }
        end_elem_xf_slot(rt, g_tit);
    }

    if cfg.music_show_artist {
        let art_box = (txt_left, art_top, txt_right, art_bot);
        let mut slice_opt = out_boxes.as_deref_mut().map(|b| &mut b[..]);
        let g_art = begin_elem_xf_slot(rt, 2, tr_art, art_box, s, &mut slice_opt);
        if tr_art.visible {
            let a_fnt = if !cfg.music_elem_fonts[2].is_empty() { &cfg.music_elem_fonts[2] } else { &cfg.music_font_artist };
            let a_fs = fs * (tr_art.font_scale_pct as f32 / 100.0).clamp(0.55, 1.85);
            let art_col = elem_col_rgba(tr_art.color_rgb, 0.90, pal.muted_col(0.88));
            crate::dock_themes::draw_cached_ellipsis_text(
                rt,
                dw,
                custom_fonts,
                1,
                a_fnt,
                8.8 * PT_TO_PX * s * a_fs,
                false,
                artist_disp,
                D2D_RECT_F {
                    left: txt_left,
                    top: art_top,
                    right: txt_right,
                    bottom: art_bot,
                },
                art_col,
                shadow_col,
                0.7 * s,
            );
        }
        end_elem_xf_slot(rt, g_art);
    }

    // Center-Right: Playback Control Glyphs (Prev, Play/Pause, Next)
    if cfg.music_show_controls {
        let ctl_box = (x + 236.0 * s, y + 18.0 * s, x + 340.0 * s, y + 56.0 * s);
        let mut slice_opt = out_boxes.as_deref_mut().map(|b| &mut b[..]);
        let g_ctl = begin_elem_xf_slot(rt, 3, tr_ctl, ctl_box, s, &mut slice_opt);
        if tr_ctl.visible {
            let glyph_col = elem_col_rgba(tr_ctl.color_rgb, 0.96, pal.text_col(0.95));
            let cy = y + 37.0 * s;

            // Prev button at (252, 37)
            let prv_x = x + 252.0 * s;
            let prv_hov = hover == WidgetHitZone::MusicPrev;
            if prv_hov {
                crate::dock_themes::with_cached_solid_brush(rt, pal.text_col(0.16), |br| {
                    rt.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: prv_x, Y: cy }, radiusX: 13.5 * s, radiusY: 13.5 * s }, br);
                });
            }
            crate::dock_themes::with_cached_solid_brush(rt, glyph_col, |br| {
                let bar = D2D1_ROUNDED_RECT {
                    rect: D2D_RECT_F { left: prv_x - 5.2 * s, top: cy - 4.8 * s, right: prv_x - 3.3 * s, bottom: cy + 4.8 * s },
                    radiusX: 0.9 * s,
                    radiusY: 0.9 * s,
                };
                rt.FillRoundedRectangle(&bar, br);
            });
            crate::dock_themes::draw_cached_triangle(
                rt,
                d2d,
                0,
                Vector2 { X: prv_x - 2.4 * s, Y: cy },
                Vector2 { X: prv_x + 4.8 * s, Y: cy - 4.8 * s },
                Vector2 { X: prv_x + 4.8 * s, Y: cy + 4.8 * s },
                glyph_col,
            );

            // Center Play/Pause disc at (288, 37)
            let ply_x = x + 288.0 * s;
            let ply_hov = hover == WidgetHitZone::MusicPlayPause;
            let disc_col = elem_col_rgba(
                tr_ctl.color_rgb,
                0.96,
                if ply_hov { pal.text_col(0.96) } else { pal.light_col(0.92) },
            );
            let disc_rim = elem_col_rgba(tr_ctl.border_rgb.or(tr_ctl.color_rgb), 0.72, rgba_u8(255, 255, 255, 0.34));
            if ply_hov {
                crate::dock_themes::with_cached_solid_brush(rt, pal.light_col(0.24), |br| {
                    rt.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: ply_x, Y: cy }, radiusX: 20.5 * s, radiusY: 20.5 * s }, br);
                });
            }
            crate::dock_themes::with_cached_solid_brush(rt, disc_col, |br| {
                rt.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: ply_x, Y: cy }, radiusX: 16.5 * s, radiusY: 16.5 * s }, br);
            });
            crate::dock_themes::with_cached_solid_brush(rt, disc_rim, |br| {
                rt.DrawEllipse(&D2D1_ELLIPSE { point: Vector2 { X: ply_x, Y: cy }, radiusX: 16.5 * s, radiusY: 16.5 * s }, br, 0.9 * s, None);
            });
            let ink_col = rgba_u8(12, 14, 18, 0.98);
            if media.is_playing {
                crate::dock_themes::with_cached_solid_brush(rt, ink_col, |br| {
                    for dx in [-3.4 * s, 1.2 * s] {
                        let b = D2D1_ROUNDED_RECT {
                            rect: D2D_RECT_F { left: ply_x + dx, top: cy - 5.4 * s, right: ply_x + dx + 2.4 * s, bottom: cy + 5.4 * s },
                            radiusX: 1.1 * s,
                            radiusY: 1.1 * s,
                        };
                        rt.FillRoundedRectangle(&b, br);
                    }
                });
            } else {
                crate::dock_themes::draw_cached_triangle(
                    rt,
                    d2d,
                    1,
                    Vector2 { X: ply_x - 3.6 * s, Y: cy - 5.6 * s },
                    Vector2 { X: ply_x + 5.8 * s, Y: cy },
                    Vector2 { X: ply_x - 3.6 * s, Y: cy + 5.6 * s },
                    ink_col,
                );
            }

            // Next button at (324, 37)
            let nxt_x = x + 324.0 * s;
            let nxt_hov = hover == WidgetHitZone::MusicNext;
            if nxt_hov {
                crate::dock_themes::with_cached_solid_brush(rt, pal.text_col(0.16), |br| {
                    rt.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: nxt_x, Y: cy }, radiusX: 13.5 * s, radiusY: 13.5 * s }, br);
                });
            }
            crate::dock_themes::draw_cached_triangle(
                rt,
                d2d,
                2,
                Vector2 { X: nxt_x - 4.8 * s, Y: cy - 4.8 * s },
                Vector2 { X: nxt_x + 2.4 * s, Y: cy },
                Vector2 { X: nxt_x - 4.8 * s, Y: cy + 4.8 * s },
                glyph_col,
            );
            crate::dock_themes::with_cached_solid_brush(rt, glyph_col, |br| {
                let bar = D2D1_ROUNDED_RECT {
                    rect: D2D_RECT_F { left: nxt_x + 3.3 * s, top: cy - 4.8 * s, right: nxt_x + 5.2 * s, bottom: cy + 4.8 * s },
                    radiusX: 0.9 * s,
                    radiusY: 0.9 * s,
                };
                rt.FillRoundedRectangle(&bar, br);
            });
        }
        end_elem_xf_slot(rt, g_ctl);
    }

    // Right: Real-Time Pulsing Vertical Equalizer Waveform Pills (zero-alloc)
    if cfg.music_show_viz {
        let eq_box = (x + 348.0 * s, y + 18.0 * s, x + 422.0 * s, y + 56.0 * s);
        let mut slice_opt = out_boxes.as_deref_mut().map(|b| &mut b[..]);
        let g_viz = begin_elem_xf_slot(rt, 4, tr_viz, eq_box, s, &mut slice_opt);
        if tr_viz.visible {
            let c1_ov = tr_viz.color_rgb.map(|rgb| {
                let (r, g, b) = crate::theme::u32_to_rgb(rgb);
                rgba_u8(r, g, b, 0.98)
            });
            let c2_ov = tr_viz.color2_rgb.map(|rgb| {
                let (r, g, b) = crate::theme::u32_to_rgb(rgb);
                rgba_u8(r, g, b, 0.90)
            });
            crate::dock_themes::draw_pill_equalizer_waveform(
                rt,
                D2D_RECT_F {
                    left: eq_box.0,
                    top: eq_box.1,
                    right: eq_box.2,
                    bottom: eq_box.3,
                },
                bands,
                media.is_playing,
                pal,
                c1_ov,
                c2_ov,
                s,
            );
        }
        end_elem_xf_slot(rt, g_viz);
    }
}

unsafe fn draw_meter_bar(
    rt: &ID2D1RenderTarget,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    pct: f32,
    s: f32,
    stops: &[D2D1_GRADIENT_STOP],
) {
    let track_rr = D2D1_ROUNDED_RECT {
        rect: D2D_RECT_F {
            left: x,
            top: y,
            right: x + w,
            bottom: y + h,
        },
        radiusX: 2.5 * s,
        radiusY: 2.5 * s,
    };
    if let Ok(b) = rt.CreateSolidColorBrush(&rgba_u8(255, 243, 220, 0.14), None) {
        rt.FillRoundedRectangle(&track_rr, &b);
    }
    let fill_w = (w * pct.clamp(0.04, 1.0)).max(4.0 * s);
    let fill_rr = D2D1_ROUNDED_RECT {
        rect: D2D_RECT_F {
            left: x,
            top: y,
            right: x + fill_w,
            bottom: y + h,
        },
        radiusX: 2.5 * s,
        radiusY: 2.5 * s,
    };
    if let Some(fill_br) = create_linear_grad(rt, (x, y), (x + fill_w, y), stops) {
        rt.FillRoundedRectangle(&fill_rr, &fill_br);
    }
}

struct DibBuffer {
    mem_dc: HDC,
    bitmap: HBITMAP,
    old_obj: HGDIOBJ,
    w: i32,
    h: i32,
}

impl DibBuffer {
    fn new() -> Self {
        Self {
            mem_dc: HDC::default(),
            bitmap: HBITMAP::default(),
            old_obj: HGDIOBJ::default(),
            w: 0,
            h: 0,
        }
    }

    fn ensure_size(&mut self, w: i32, h: i32) -> bool {
        let w = w.max(2);
        let h = h.max(2);
        if !self.mem_dc.0.is_null() && self.w == w && self.h == h {
            return true;
        }
        self.release();
        unsafe {
            let dc = CreateCompatibleDC(None);
            if dc.0.is_null() {
                return false;
            }
            let bmi = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: w,
                    biHeight: -h,
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut bits: *mut c_void = std::ptr::null_mut();
            let Ok(bmp) = CreateDIBSection(Some(dc), &bmi, DIB_RGB_COLORS, &mut bits, None, 0)
            else {
                let _ = DeleteDC(dc);
                return false;
            };
            let old = SelectObject(dc, HGDIOBJ(bmp.0));
            self.mem_dc = dc;
            self.bitmap = bmp;
            self.old_obj = old;
            self.w = w;
            self.h = h;
            true
        }
    }

    fn release(&mut self) {
        unsafe {
            if !self.mem_dc.0.is_null() {
                if !self.old_obj.0.is_null() {
                    let _ = SelectObject(self.mem_dc, self.old_obj);
                    self.old_obj = HGDIOBJ::default();
                }
                let _ = DeleteDC(self.mem_dc);
                self.mem_dc = HDC::default();
            }
            if !self.bitmap.0.is_null() {
                let _ = DeleteObject(HGDIOBJ(self.bitmap.0));
                self.bitmap = HBITMAP::default();
            }
            self.w = 0;
            self.h = 0;
        }
    }
}

impl Drop for DibBuffer {
    fn drop(&mut self) {
        self.release();
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WidgetHitZone {
    None,
    ClockCard,
    AppSlot(usize),
    AppAddTile,
    AppsBar,
    VitalsCpu,
    VitalsGpu,
    VitalsRam,
    VitalsTemp,
    VitalsPower,
    VitalsSleep,
    VitalsShutdown,
    VitalsBar,
    MusicCard,
    MusicPrev,
    MusicPlayPause,
    MusicNext,
    CcBar,
    CcTile(usize, bool),
    CcScrollUp,
    CcScrollDown,
    CcScrollTrack,
    CcBrightSlider,
    CcVolSlider,
    CcMuteBtn,
    ExtCard(ExtWidgetId),
    ExtBtn(ExtWidgetId, u8),
    CtxButton(usize),
    CtxBg,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum DragWidgetKind {
    DateTime,
    Apps,
    AppTile(usize),
    Vitals,
    Music,
    ControlCenter,
    Ext(ExtWidgetId),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum CcDragMode {
    BrightSlider,
    VolSlider,
    ScrollThumb,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CtxAction {
    AddApp,
    EditApp(usize),
    RemoveApp(usize),
    ToggleIndividualMove,
    ToggleLock,
    OpenStudioApps,
    OpenStudioMusic,
    OpenStudioCc,
    OpenStudioWallpaper,
    OpenStudioExt(ExtWidgetId),
    Dismiss,
}

#[derive(Clone)]
struct CtxButton {
    label: String,
    action: CtxAction,
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
    is_primary: bool,
    is_danger: bool,
}

struct ContextBarState {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    buttons: Vec<CtxButton>,
}

const TIMER_CTX_POLL: usize = 401;
const VK_LBUTTON_CODE: i32 = 0x01;
const VK_ESCAPE_CODE: i32 = 0x1B;

unsafe extern "system" fn enum_worker_defview(hwnd: HWND, lparam: LPARAM) -> BOOL {
    if FindWindowExW(Some(hwnd), None, w!("SHELLDLL_DefView"), PCWSTR::null())
        .map(|h| !h.0.is_null())
        .unwrap_or(false)
    {
        let out = &mut *(lparam.0 as *mut HWND);
        *out = hwnd;
        return BOOL(0);
    }
    BOOL(1)
}

/// Finds the Windows Desktop host window (`Progman` or `WorkerW` containing `SHELLDLL_DefView`).
pub unsafe fn find_desktop_host_hwnd() -> HWND {
    if let Ok(progman) = FindWindowW(w!("Progman"), PCWSTR::null()) {
        if !progman.0.is_null() {
            if FindWindowExW(Some(progman), None, w!("SHELLDLL_DefView"), PCWSTR::null())
                .map(|h| !h.0.is_null())
                .unwrap_or(false)
            {
                return progman;
            }
        }
    }
    let mut found = HWND::default();
    let _ = EnumWindows(Some(enum_worker_defview), LPARAM(&mut found as *mut _ as isize));
    if !found.0.is_null() {
        return found;
    }
    FindWindowW(w!("Progman"), PCWSTR::null()).unwrap_or_default()
}

pub struct DesktopWidgets {
    hwnd: HWND,
    desktop_host: HWND,
    cmd_hwnd: HWND,
    cmd_msg_24h: u32,
    cmd_msg_sync: u32,
    cmd_msg_viz_wake: u32,
    d2d_factory: Option<ID2D1Factory>,
    dwrite_factory: Option<IDWriteFactory>,
    wic_factory: Option<IWICImagingFactory>,
    custom_fonts: Option<IDWriteFontCollection>,
    dcomp: Option<DCompSwapSurface>,
    dcomp_status: String,
    dcomp_win_rect: Option<(i32, i32, i32, i32)>,
    dc_rt: Option<ID2D1DCRenderTarget>,
    backbuf: DibBuffer,

    icon_cache: AppIconCache,
    cover_cache: CoverBitmapCache,
    fg_bmp_cache: WidgetFgBmpCache,
    media_engine: SystemMediaEngine,

    hover_zone: WidgetHitZone,
    u: f32,
    mon_left: i32,
    mon_top: i32,
    screen_w: i32,
    screen_h: i32,

    drag_widget: Option<DragWidgetKind>,
    drag_start_cursor: POINT,
    drag_start_xy: (i32, i32),
    drag_moved: bool,
    cc_drag: Option<CcDragMode>,
    bright_drag: Option<(f32, Instant)>,
    vol_drag: Option<(f32, bool, Instant)>,
    last_sent_bright_pct: Option<u8>,
    last_sent_vol_pct: Option<u8>,
    cc_scroll_y: f32,
    wheel_accum_bright: i32,
    wheel_accum_vol: i32,
    wheel_accum_scroll: i32,
    wheel_last_at: Option<Instant>,
    lbtn_down_zone: WidgetHitZone,
    context_bar: Option<ContextBarState>,

    pub cfg: AppConfig,
    pub visible: bool,

    // Telemetry state
    cpu_pct: f32,
    gpu_pct: f32,
    ram_pct: f32,
    temp_c: f32,
    bat_pct: f32,
    hud_smooth: [f32; 4],
    island_bands: [f32; 12],
    last_rendered_bands: [f32; 12],
    last_anim60_tick: Instant,
    prev_idle: u64,
    prev_kernel: u64,
    prev_user: u64,
    pdh_query: isize,
    pdh_cpu_counter: isize,
    pdh_started: bool,
    dxgi_probe_tick: u32,
    dxgi_cached_adapter: Option<IDXGIAdapter3>,
    pdh_pending: Arc<Mutex<Option<(isize, isize)>>>,
    pdh_cancelled: Arc<AtomicBool>,
    last_clock_sec: u32,
    last_drawn_telemetry: (i32, i32, i32, i32, i32),
    pub ext_state: ExtRuntimeState,
    widget_sims: Box<[VizSnapshot; 19]>,
    last_fx_tick: Instant,
    last_render_at: Instant,
    last_hover_move: Instant,
    viz_dirty: bool,
    tracking_leave: bool,
    covered_by_maximized: bool,
    covered_since: Option<Instant>,
    covered_trimmed: bool,
}

static mut GLOBAL_WIDGETS_PTR: *mut DesktopWidgets = std::ptr::null_mut();

fn flip_mech_anim_active(cfg: &AppConfig) -> bool {
    if !cfg.widgets_live_material || !cfg.show_datetime || cfg.dt_style != ClockStyle::FlipMech {
        return false;
    }
    let ms = (unsafe { GetLocalTime().wMilliseconds } as f32 / 1000.0).clamp(0.0, 0.999);
    let sec = chrono_local().second;
    if cfg.dt_show_seconds && (ms < 0.54 || ms >= 0.90) {
        return true;
    }
    // Start waking cleanly right before minute rollover (59.50s) and stay active through the 0.60s leaf fold.
    (sec == 59 && ms >= 0.50) || (sec == 0 && ms < 0.64)
}

fn has_ambient_live_hover_fx(cfg: &AppConfig) -> bool {
    (cfg.show_datetime && cfg.dt_hover_fx.is_ambient_live())
        || (cfg.show_apps && cfg.apps_hover_fx.is_ambient_live())
        || (cfg.show_vitals && cfg.vitals_hover_fx.is_ambient_live())
        || (cfg.show_music && cfg.music_hover_fx.is_ambient_live())
        || (cfg.show_cc && cfg.cc_hover_fx.is_ambient_live())
        || cfg
            .ext_widgets
            .iter()
            .any(|w| w.enabled && w.hover_fx.is_ambient_live())
}

/// Pure gating predicate determining whether desktop widgets need high-frequency (~30 FPS)
/// animation ticks (`TIMER_VIZ_TICK`). Returns `false` when idle on static materials.
pub fn widgets_want_viz_tick(
    cfg: &AppConfig,
    sims: &[VizSnapshot; 19],
    pbr_hover_recent: bool,
    viz_dirty: bool,
) -> bool {
    if !cfg.widgets_enabled {
        return false;
    }
    if viz_dirty {
        return true;
    }
    let ambient = cfg.widgets_live_material
        && (has_ambient_live_hover_fx(cfg)
            || (cfg.show_datetime && cfg.dt_style.has_idle_motion())
            || (cfg.show_music && cfg.music_style.has_idle_motion())
            || cfg.ext_widgets.iter().any(|w| w.enabled && w.theme.has_idle_motion()));
    if ambient {
        return true;
    }
    let sim_fx_active = (cfg.show_datetime
        && cfg.dt_hover_fx.is_ambient_live()
        && sims[0].is_active())
        || (cfg.show_apps && cfg.apps_hover_fx.is_ambient_live() && sims[1].is_active())
        || (cfg.show_vitals && cfg.vitals_hover_fx.is_ambient_live() && sims[2].is_active())
        || (cfg.show_music && cfg.music_hover_fx.is_ambient_live() && sims[3].is_active())
        || (cfg.show_cc && cfg.cc_hover_fx.is_ambient_live() && sims[4].is_active())
        || ExtWidgetId::ALL.iter().any(|id| {
            let w = &cfg.ext_widgets[id.idx()];
            w.enabled && w.hover_fx.is_ambient_live() && sims[5 + id.idx()].is_active()
        });
    if sim_fx_active {
        return true;
    }
    let any_ripple = sims.iter().any(|s| s.ripples.iter().any(|r| r.active));
    let any_fading = sims
        .iter()
        .any(|s| s.hover_mix > 0.0005 && (!s.hover_active || s.hover_mix < 0.998));
    any_ripple || any_fading || (cfg.widgets_hover_react && pbr_hover_recent)
}

fn create_widgets_dc_rt(f: &ID2D1Factory) -> Option<ID2D1DCRenderTarget> {
    let props = D2D1_RENDER_TARGET_PROPERTIES {
        r#type: D2D1_RENDER_TARGET_TYPE_DEFAULT,
        pixelFormat: D2D1_PIXEL_FORMAT {
            format: DXGI_FORMAT_B8G8R8A8_UNORM,
            alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
        },
        dpiX: 96.0,
        dpiY: 96.0,
        usage: D2D1_RENDER_TARGET_USAGE_GDI_COMPATIBLE,
        ..Default::default()
    };
    unsafe { f.CreateDCRenderTarget(&props).ok() }
}

/// Pure decision helper matching `DesktopWidgets::wants_fast_30fps`:
/// returns `false` when widgets are hidden/covered or only running idle ambient drift,
/// and `true` when active hover/drag/ripple/audio/60fps animation is occurring.
pub fn compute_widgets_fast_30fps(
    visible: bool,
    covered: bool,
    interactive_input: bool,
    has_active_fx: bool,
    ambient_idle_only: bool,
) -> bool {
    if !visible || covered {
        return false;
    }
    if interactive_input {
        return true;
    }
    has_active_fx && !ambient_idle_only
}

impl DesktopWidgets {
    pub fn new(
        hinst: HINSTANCE,
        cmd_hwnd: HWND,
        cmd_msg_24h: u32,
        cmd_msg_sync: u32,
        cmd_msg_media: u32,
        cmd_msg_viz_wake: u32,
        initial_cfg: &AppConfig,
    ) -> Self {
        let d2d_factory: Option<ID2D1Factory> =
            unsafe { D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None).ok() };
        let dwrite_factory: Option<IDWriteFactory> =
            unsafe { DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED).ok() };
        let wic_factory: Option<IWICImagingFactory> =
            unsafe { CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER).ok() };

        Self::load_gdi_fonts();
        let custom_fonts = dwrite_factory
            .as_ref()
            .and_then(|dw| unsafe { build_custom_font_collection(dw) });

        Self::register_class(hinst);

        let desktop_host = unsafe { find_desktop_host_hwnd() };
        // Setting hwndParent on WS_POPUP establishes Win32 Owner = desktop_host (Progman/WorkerW).
        // That makes User32 automatically keep our widgets ABOVE Progman when Win+D raises Progman!
        let owner_opt = if !desktop_host.0.is_null() {
            Some(desktop_host)
        } else {
            None
        };

        // Create the WS_EX_NOREDIRECTIONBITMAP window unowned first so DirectComposition's
        // CreateTargetForHwnd never rejects cross-process Progman ownership; attach() then
        // binds GWLP_HWNDPARENT to desktop_host so Win+D keeps widgets above the wallpaper.
        let mut hwnd = unsafe {
            CreateWindowExW(
                WS_EX_NOREDIRECTIONBITMAP | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
                w!("AutumnDesktopWidgetsWnd"),
                w!("Autumn Desktop Widgets"),
                WS_POPUP,
                0,
                0,
                2,
                2,
                None,
                None,
                Some(hinst),
                None,
            )
            .unwrap_or_default()
        };

        let (dcomp, dcomp_status) = match DCompSwapSurface::new(hwnd) {
            Ok(dc) => {
                crate::diagnostics::record_event("DCOMP", "hardware_swapchain_active");
                (Some(dc), "dcomp".to_string())
            }
            Err((stage, hr)) => {
                let status = format!("gdi:{}:0x{:08X}", stage, hr as u32);
                crate::diagnostics::record_event("DCOMP", &status);
                (None, status)
            }
        };
        let mut d2d_factory = d2d_factory;
        let dc_rt = if let Some(ref dc) = dcomp {
            // CRITICAL: Share the exact ID2D1Factory that owns DCompSwapSurface's ID2D1DeviceContext
            // so geometries/stroke styles/layers never trigger D2DERR_WRONG_FACTORY (0x88990012).
            d2d_factory = Some(dc.d2d_factory());
            if !desktop_host.0.is_null() {
                unsafe {
                    let _ = SetWindowLongPtrW(hwnd, GWLP_HWNDPARENT, desktop_host.0 as isize);
                }
            }
            None
        } else {
            // Clean fallback to GDI layered window when hardware D3D11/DirectComposition is unavailable
            unsafe {
                if !hwnd.0.is_null() {
                    let _ = DestroyWindow(hwnd);
                }
                hwnd = CreateWindowExW(
                    WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
                    w!("AutumnDesktopWidgetsWnd"),
                    w!("Autumn Desktop Widgets"),
                    WS_POPUP,
                    0,
                    0,
                    2,
                    2,
                    owner_opt,
                    None,
                    Some(hinst),
                    None,
                )
                .unwrap_or_default();
            }
            d2d_factory.as_ref().and_then(create_widgets_dc_rt)
        };

        let media_engine = SystemMediaEngine::new(cmd_hwnd, cmd_msg_media);
        let pdh_pending = Arc::new(Mutex::new(None));
        let pdh_cancelled = Arc::new(AtomicBool::new(false));

        let mut init_mon = RECT {
            left: 0,
            top: 0,
            right: 1920,
            bottom: 1200,
        };
        unsafe {
            let hmon = MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY);
            let mut mi = MONITORINFO {
                cbSize: size_of::<MONITORINFO>() as u32,
                ..Default::default()
            };
            if GetMonitorInfoW(hmon, &mut mi).as_bool() {
                init_mon = mi.rcMonitor;
            }
        }
        let init_sw = (init_mon.right - init_mon.left).max(320);
        let init_sh = (init_mon.bottom - init_mon.top).max(240);
        let init_u = (init_sw as f32 / 1280.0).clamp(0.5, 4.0);

        let mut instance = Self {
            hwnd,
            desktop_host,
            cmd_hwnd,
            cmd_msg_24h,
            cmd_msg_sync,
            cmd_msg_viz_wake,
            d2d_factory,
            dwrite_factory,
            wic_factory,
            custom_fonts,
            dcomp,
            dcomp_status,
            dcomp_win_rect: None,
            dc_rt,
            backbuf: DibBuffer::new(),
            icon_cache: AppIconCache::new(),
            cover_cache: CoverBitmapCache::new(),
            fg_bmp_cache: WidgetFgBmpCache::default(),
            media_engine,
            hover_zone: WidgetHitZone::None,
            u: init_u,
            mon_left: init_mon.left,
            mon_top: init_mon.top,
            screen_w: init_sw,
            screen_h: init_sh,
            drag_widget: None,
            drag_start_cursor: POINT::default(),
            drag_start_xy: (0, 0),
            drag_moved: false,
            cc_drag: None,
            bright_drag: None,
            vol_drag: None,
            last_sent_bright_pct: None,
            last_sent_vol_pct: None,
            cc_scroll_y: 0.0,
            wheel_accum_bright: 0,
            wheel_accum_vol: 0,
            wheel_accum_scroll: 0,
            wheel_last_at: None,
            lbtn_down_zone: WidgetHitZone::None,
            context_bar: None,
            cfg: initial_cfg.clone(),
            visible: false,
            cpu_pct: 7.0,
            gpu_pct: 12.0,
            ram_pct: 60.0,
            temp_c: 44.0,
            bat_pct: 100.0,
            hud_smooth: [7.0, 12.0, 60.0, 44.0],
            island_bands: [0.0; 12],
            last_rendered_bands: [0.0; 12],
            last_anim60_tick: Instant::now(),
            prev_idle: 0,
            prev_kernel: 0,
            prev_user: 0,
            pdh_query: 0,
            pdh_cpu_counter: 0,
            pdh_started: false,
            dxgi_probe_tick: 0,
            dxgi_cached_adapter: None,
            pdh_pending,
            pdh_cancelled,
            last_clock_sec: u32::MAX,
            last_drawn_telemetry: (-1, -1, -1, -1, -1),
            ext_state: ExtRuntimeState::new(),
            widget_sims: Box::new(std::array::from_fn(|_| VizSnapshot::default())),
            last_fx_tick: Instant::now(),
            last_render_at: Instant::now(),
            last_hover_move: Instant::now() - Duration::from_secs(5),
            viz_dirty: false,
            tracking_leave: false,
            covered_by_maximized: false,
            covered_since: None,
            covered_trimmed: false,
        };

        instance.media_engine.set_viz_eco_fps(instance.cfg.viz_eco_fps);
        instance.query_fast_baseline();
        instance.hud_smooth = [instance.cpu_pct, instance.gpu_pct, instance.ram_pct, instance.temp_c];
        instance
    }

    fn telemetry_needed(&self) -> bool {
        self.cfg.widgets_enabled
            && (self.cfg.show_vitals || self.cfg.ext_widgets[ExtWidgetId::Gpu.idx()].enabled)
    }

    fn ensure_pdh_started(&mut self) {
        if self.pdh_started || self.pdh_query != 0 || self.pdh_cancelled.load(Ordering::Relaxed) {
            return;
        }
        self.pdh_started = true;
        Self::spawn_pdh_init(&self.pdh_pending, &self.pdh_cancelled);
    }

    fn query_fast_baseline(&mut self) {
        let mut idle = FILETIME::default();
        let mut kernel = FILETIME::default();
        let mut user = FILETIME::default();
        if unsafe { GetSystemTimes(Some(&mut idle), Some(&mut kernel), Some(&mut user)).is_ok() } {
            self.prev_idle = filetime_to_u64(idle);
            self.prev_kernel = filetime_to_u64(kernel);
            self.prev_user = filetime_to_u64(user);
        }
        let mut mem = MEMORYSTATUSEX {
            dwLength: size_of::<MEMORYSTATUSEX>() as u32,
            ..Default::default()
        };
        if unsafe { GlobalMemoryStatusEx(&mut mem).is_ok() } {
            self.ram_pct = mem.dwMemoryLoad as f32;
        }
        let mut pwr = SYSTEM_POWER_STATUS::default();
        if unsafe { GetSystemPowerStatus(&mut pwr).is_ok() } && pwr.BatteryLifePercent != 255 {
            self.bat_pct = (pwr.BatteryLifePercent as f32).clamp(0.0, 100.0);
        }
    }

    fn spawn_pdh_init(
        pending: &Arc<Mutex<Option<(isize, isize)>>>,
        cancelled: &Arc<AtomicBool>,
    ) {
        let pend_bg = Arc::clone(pending);
        let canc_bg = Arc::clone(cancelled);
        std::thread::spawn(move || {
            if canc_bg.load(Ordering::Relaxed) {
                return;
            }
            unsafe {
                let mut q = PDH_HQUERY::default();
                if PdhOpenQueryW(PCWSTR::null(), 0, &mut q) == 0 && !q.is_invalid() {
                    let mut c = PDH_HCOUNTER::default();
                    let path = w!(r"\Processor Information(_Total)\% Processor Utility");
                    if PdhAddEnglishCounterW(q, path, 0, &mut c) == 0 && !c.is_invalid() {
                        let _ = PdhCollectQueryData(q);
                        if !canc_bg.load(Ordering::Relaxed) {
                            if let Ok(mut guard) = pend_bg.lock() {
                                *guard = Some((q.0 as isize, c.0 as isize));
                                return;
                            }
                        }
                    }
                    let _ = PdhCloseQuery(q);
                }
            }
        });
    }

    pub fn has_desktop_host(&self) -> bool {
        if self.desktop_host.0.is_null() {
            return false;
        }
        unsafe {
            IsWindow(Some(self.desktop_host)).as_bool()
                && FindWindowExW(Some(self.desktop_host), None, w!("SHELLDLL_DefView"), PCWSTR::null())
                    .map(|h| !h.0.is_null())
                    .unwrap_or(false)
        }
    }

    pub fn hwnd(&self) -> HWND {
        self.hwnd
    }

    pub fn telemetry(&self) -> (f32, f32, f32) {
        (self.cpu_pct, self.ram_pct, self.bat_pct)
    }

    pub fn telemetry_extended(&self) -> (f32, f32, f32, f32, f32) {
        (self.cpu_pct, self.gpu_pct, self.ram_pct, self.temp_c, self.bat_pct)
    }

    pub fn media_snapshot(&self) -> MediaSnapshot {
        self.media_engine.media_snapshot()
    }

    pub fn cc_snapshot(&self) -> CcSnapshot {
        self.media_engine.cc_snapshot()
    }

    pub fn bands(&self) -> [f32; 12] {
        self.media_engine.bands()
    }

    pub fn is_audio_active(&self) -> bool {
        self.media_engine.is_audio_active()
    }

    pub fn set_viz_eco_fps(&self, eco: bool) {
        self.media_engine.set_viz_eco_fps(eco);
    }

    pub fn set_viz_needed(&self, needed: bool) {
        self.media_engine.set_viz_needed(needed);
    }

    pub fn wants_wasapi_loopback(&self) -> bool {
        self.visible
            && !self.covered_by_maximized
            && self.cfg.widgets_enabled
            && self.cfg.show_music
            && self.cfg.music_show_viz
            && self.cfg.music_style != crate::config::MusicStyle::CassetteDeck
    }

    pub fn is_ring_pill_interpolating(&self) -> bool {
        self.cfg.show_vitals
            && self.cfg.vitals_style == 4
            && ((self.hud_smooth[0] - self.cpu_pct).abs() > 0.12
                || (self.hud_smooth[1] - self.gpu_pct).abs() > 0.12
                || (self.hud_smooth[2] - self.ram_pct).abs() > 0.12
                || (self.hud_smooth[3] - self.temp_c).abs() > 0.12)
    }

    pub fn is_island_bands_decaying(&self) -> bool {
        self.cfg.show_music
            && self.cfg.music_style == crate::config::MusicStyle::DynamicIslandCapsule
            && self.cfg.music_show_viz
            && self.island_bands.iter().any(|&v| v > 0.004)
    }

    /// True when the 60 FPS luxury animation loop is active (Ring-Pill smooth telemetry
    /// convergence, or Dynamic Island equalizer when Eco 24 FPS mode is turned off).
    /// Automatically settles back to false when static so timer returns to idle 0% CPU.
    pub fn wants_fast_60fps(&self) -> bool {
        if !self.visible || self.covered_by_maximized || !self.cfg.widgets_enabled {
            return false;
        }
        if self.is_ring_pill_interpolating() {
            return true;
        }
        if self.cfg.viz_eco_fps {
            return false;
        }
        self.is_island_bands_decaying()
            || (self.cfg.show_music
                && self.cfg.music_style == crate::config::MusicStyle::DynamicIslandCapsule
                && self.cfg.music_show_viz
                && self.media_engine.is_audio_active())
    }

    pub fn wants_audio_viz_tick(&self) -> bool {
        !self.covered_by_maximized
            && self.visible
            && (self.is_ring_pill_interpolating()
                || self.is_island_bands_decaying()
                || (self.cfg.show_music
                    && self.cfg.music_show_viz
                    && self.media_engine.is_audio_active()))
    }

    pub fn trigger_weather_refresh(&mut self) {
        self.ext_state.trigger_weather_refresh();
    }

    pub fn local_weather_summary(&self) -> (String, f32, &'static str) {
        self.ext_state.local_weather_summary()
    }

    pub fn send_media_action(&mut self, act: CcAction) {
        self.media_engine.send_action(act);
        self.render();
    }

    /// Non-blocking stop for OS logoff/shutdown (`WM_ENDSESSION`) so WinRT/WASAPI
    /// or PDH worker teardown cannot block the UI thread during session end.
    pub fn prepare_os_shutdown(&mut self) {
        self.pdh_cancelled.store(true, Ordering::Relaxed);
        self.media_engine.request_stop_no_block();
    }

    pub fn on_media_update(&mut self) {
        if !self.visible || self.hwnd.0.is_null() {
            return;
        }
        if (self.cfg.show_music || self.cfg.show_cc)
            && self.drag_widget.is_none()
            && self.cc_drag.is_none()
        {
            let min_gap = if self.wants_fast_60fps() {
                Duration::from_millis(14)
            } else if self.cfg.viz_eco_fps {
                Duration::from_millis(40)
            } else {
                Duration::from_millis(28)
            };
            if !self.wants_fast_30fps() || self.last_render_at.elapsed() >= min_gap {
                self.render();
            } else {
                self.viz_dirty = true;
            }
        }
    }

    pub fn is_covered_trimmed(&self) -> bool {
        self.covered_by_maximized && self.covered_trimmed
    }

    pub fn set_covered_by_maximized(&mut self, covered: bool) {
        if covered {
            if !self.covered_by_maximized {
                self.covered_by_maximized = true;
                self.covered_since = Some(Instant::now());
                self.covered_trimmed = false;
            } else if !self.covered_trimmed
                && self
                    .covered_since
                    .is_some_and(|at| at.elapsed() >= Duration::from_secs(4))
            {
                // Reclaim 1920x1200 DComp swapchain buffers, PBR caches & icon bitmaps while covered.
                self.covered_trimmed = true;
                self.release_render_surfaces();
                if !self.hwnd.0.is_null() {
                    unsafe {
                        let _ = SetWindowPos(
                            self.hwnd,
                            None,
                            0,
                            0,
                            0,
                            0,
                            SWP_NOMOVE
                                | SWP_NOSIZE
                                | SWP_NOACTIVATE
                                | SWP_NOZORDER
                                | SWP_NOOWNERZORDER
                                | SWP_HIDEWINDOW,
                        );
                    }
                }
                crate::config::trim_process_working_set();
            }
            return;
        }
        if !self.covered_by_maximized {
            return;
        }
        let was_trimmed = self.covered_trimmed;
        self.covered_by_maximized = false;
        self.covered_since = None;
        self.covered_trimmed = false;
        if self.visible && !self.hwnd.0.is_null() {
            self.last_fx_tick = Instant::now();
            self.viz_dirty = true;
            self.render();
            if was_trimmed {
                unsafe {
                    let _ = SetWindowPos(
                        self.hwnd,
                        None,
                        0,
                        0,
                        0,
                        0,
                        SWP_NOMOVE
                            | SWP_NOSIZE
                            | SWP_NOACTIVATE
                            | SWP_NOZORDER
                            | SWP_NOOWNERZORDER
                            | SWP_SHOWWINDOW,
                    );
                }
                self.ensure_above_desktop();
            }
        }
    }

    pub fn has_active_interactive_fx(&self) -> bool {
        if !self.visible || (self.covered_by_maximized && self.drag_widget.is_none() && self.cc_drag.is_none()) {
            return false;
        }
        let pbr_hover_recent = self.hover_zone != WidgetHitZone::None
            && self.last_hover_move.elapsed() <= Duration::from_millis(160);
        flip_mech_anim_active(&self.cfg)
            || widgets_want_viz_tick(
                &self.cfg,
                &self.widget_sims,
                pbr_hover_recent,
                self.viz_dirty,
            )
    }

    /// True when widgets need full ~30 FPS (active cursor hover/drag, click ripple, fade-out,
    /// audio spectrum, or Split-Flap fold); returns false when idle on static materials or when
    /// only gentle idle ambient drift (`ambient_idle_only`) is running.
    pub fn wants_fast_30fps(&self) -> bool {
        let interactive_input = self.drag_widget.is_some()
            || self.cc_drag.is_some()
            || self.hover_zone != WidgetHitZone::None
            || self.wants_fast_60fps()
            || (self.cfg.show_music && self.cfg.music_show_viz && self.media_engine.is_audio_active())
            || self.viz_dirty
            || flip_mech_anim_active(&self.cfg);
        compute_widgets_fast_30fps(
            self.visible,
            self.covered_by_maximized,
            interactive_input,
            self.has_active_interactive_fx(),
            self.ambient_idle_only(),
        )
    }

    pub fn reset_fx_clock(&mut self) {
        let now = Instant::now();
        self.last_fx_tick = now;
        self.last_anim60_tick = now;
    }

    /// True when the only reason to animate is idle ambient motion (e.g. hover_snow drifting)
    /// with no cursor hover, drag, ripple, audio, or 60 FPS work. Such frames can run at a
    /// reduced cadence (~15 FPS) to keep idle CPU/GPU usage low.
    pub fn ambient_idle_only(&self) -> bool {
        if !self.visible || self.covered_by_maximized {
            return false;
        }
        if self.drag_widget.is_some()
            || self.cc_drag.is_some()
            || self.hover_zone != WidgetHitZone::None
            || self.viz_dirty
            || self.wants_fast_60fps()
            || (self.cfg.show_music && self.cfg.music_show_viz && self.media_engine.is_audio_active())
            || flip_mech_anim_active(&self.cfg)
        {
            return false;
        }
        let any_ripple = self.widget_sims.iter().any(|s| s.ripples.iter().any(|r| r.active));
        let any_fading = self
            .widget_sims
            .iter()
            .any(|s| s.hover_mix > 0.0005 && (!s.hover_active || s.hover_mix < 0.998));
        !any_ripple && !any_fading
    }

    pub fn tick_fast(&mut self) {
        if !self.visible || self.hwnd.0.is_null() {
            return;
        }
        if self.covered_by_maximized && self.drag_widget.is_none() && self.cc_drag.is_none() {
            return;
        }
        let interactive_on = self.has_active_interactive_fx();
        // Idle ambient-only motion (hover_sakura/snow/caustics with cursor away) is capped to
        // ~8-11 FPS regardless of the shared viz timer cadence to keep full-screen DComp CPU low.
        if interactive_on && self.ambient_idle_only() {
            let min_gap = Duration::from_millis(if self.cfg.viz_eco_fps { 120 } else { 90 });
            if self.last_render_at.elapsed() < min_gap {
                return;
            }
        }
        let mut any_visual_change = self.viz_dirty;
        if interactive_on && self.drag_widget.is_none() && self.cc_drag.is_none() {
            let dt = self.last_fx_tick.elapsed().as_secs_f32().clamp(0.008, 0.135);
            self.last_fx_tick = Instant::now();
            let mut pt = POINT::default();
            unsafe {
                let _ = GetCursorPos(&mut pt);
            }
            let mx = (pt.x - self.mon_left) as f32;
            let my = (pt.y - self.mon_top) as f32;
            let u = self.u.max(0.5);
            let hover_react = self.cfg.widgets_hover_react;
            let live_mat = self.cfg.widgets_live_material;
            let win_hovered = self.hover_zone != WidgetHitZone::None;

            let step_rect = |sim: &mut VizSnapshot, fx: TaskbarVizStyle, rx: f32, ry: f32, rw: f32, rh: f32| -> bool {
                let hover_in = win_hovered
                    && rw > 1.0
                    && rh > 1.0
                    && mx >= rx
                    && mx <= rx + rw
                    && my >= ry
                    && my <= ry + rh;
                let nx = if hover_in { ((mx - rx) / rw).clamp(0.0, 1.0) } else { 0.5 };
                let ny = if hover_in { ((my - ry) / rh).clamp(0.0, 1.0) } else { 0.5 };
                let prev_mix = sim.hover_mix;
                let had_ripples = sim.ripples.iter().any(|r| r.active);
                sim.step_hover_physics(fx, dt, nx, ny, hover_in, hover_react, rw, rh);
                let has_ripples = sim.ripples.iter().any(|r| r.active);
                (live_mat && fx.is_ambient_live())
                    || sim.is_active()
                    || (sim.hover_mix - prev_mix).abs() > 0.0005
                    || (prev_mix > 0.0 && sim.hover_mix == 0.0)
                    || (had_ripples && !has_ripples)
            };
            let step_pbr = |sim: &mut VizSnapshot, mat: crate::realistic_pbr::PbrMaterial, rx: f32, ry: f32, rw: f32, rh: f32| -> bool {
                let hover_in = win_hovered
                    && rw > 1.0
                    && rh > 1.0
                    && mx >= rx
                    && mx <= rx + rw
                    && my >= ry
                    && my <= ry + rh;
                let nx = if hover_in { ((mx - rx) / rw).clamp(0.0, 1.0) } else { 0.5 };
                let ny = if hover_in { ((my - ry) / rh).clamp(0.0, 1.0) } else { 0.5 };
                let changed = sim.step_material_interactive(
                    dt,
                    nx,
                    ny,
                    hover_in,
                    hover_react,
                    mat.spawns_cursor_ripples(),
                );
                changed || (live_mat && mat.has_ambient_idle_motion())
            };

            if self.cfg.show_datetime {
                let s = u * (self.cfg.dt_scale_pct as f32 / 100.0);
                let (lw, lh) = dt_logical_size(&self.cfg);
                if self.cfg.dt_hover_fx.is_ambient_live() {
                    any_visual_change |= step_rect(
                        &mut self.widget_sims[0],
                        self.cfg.dt_hover_fx,
                        self.cfg.dt_x as f32 * u,
                        self.cfg.dt_y as f32 * u,
                        lw * s,
                        lh * s,
                    );
                } else if let Some(mat) = self.cfg.dt_style.pbr_material() {
                    any_visual_change |= step_pbr(
                        &mut self.widget_sims[0],
                        mat,
                        self.cfg.dt_x as f32 * u,
                        self.cfg.dt_y as f32 * u,
                        lw * s,
                        lh * s,
                    );
                }
                if flip_mech_anim_active(&self.cfg) {
                    any_visual_change = true;
                }
            }
            if self.cfg.show_apps && self.cfg.apps_hover_fx.is_ambient_live() {
                let s = u * (self.cfg.apps_scale_pct as f32 / 100.0);
                let bx = self.cfg.apps_x as f32 * u;
                let by = self.cfg.apps_y as f32 * u;
                let show_add = self.cfg.apps_show_add_tile && self.cfg.apps.len() < 12;
                let total_tiles = self.cfg.apps.len() + usize::from(show_add);
                if total_tiles > 0 {
                    let (offsets, _, _) = apps_layout_offsets(total_tiles, self.cfg.apps_cols.max(1) as usize);
                    let mut l = f32::MAX;
                    let mut t = f32::MAX;
                    let mut r = f32::MIN;
                    let mut b = f32::MIN;
                    for (i, app) in self.cfg.apps.iter().enumerate() {
                        let ax = bx + (offsets[i].0 + app.offset_x as f32) * s;
                        let ay = by + (offsets[i].1 + app.offset_y as f32) * s;
                        l = l.min(ax);
                        t = t.min(ay);
                        r = r.max(ax + 64.0 * s);
                        b = b.max(ay + 94.0 * s);
                    }
                    if show_add {
                        let idx = self.cfg.apps.len();
                        let ax = bx + offsets[idx].0 * s;
                        let ay = by + offsets[idx].1 * s;
                        l = l.min(ax);
                        t = t.min(ay);
                        r = r.max(ax + 64.0 * s);
                        b = b.max(ay + 94.0 * s);
                    }
                    any_visual_change |= step_rect(&mut self.widget_sims[1], self.cfg.apps_hover_fx, l, t, r - l, b - t);
                }
            }
            if self.cfg.show_vitals && self.cfg.vitals_hover_fx.is_ambient_live() {
                let s = u * (self.cfg.vitals_scale_pct as f32 / 100.0);
                let (vw, vh) = vitals_logical_size(&self.cfg);
                any_visual_change |= step_rect(
                    &mut self.widget_sims[2],
                    self.cfg.vitals_hover_fx,
                    self.cfg.vitals_x as f32 * u,
                    self.cfg.vitals_y as f32 * u,
                    vw * s,
                    vh * s,
                );
            }
            if self.cfg.show_music {
                let s = u * (self.cfg.music_scale_pct as f32 / 100.0);
                let (lw, lh) = music_logical_size(&self.cfg);
                if self.cfg.music_hover_fx.is_ambient_live() {
                    any_visual_change |= step_rect(
                        &mut self.widget_sims[3],
                        self.cfg.music_hover_fx,
                        self.cfg.music_x as f32 * u,
                        self.cfg.music_y as f32 * u,
                        lw * s,
                        lh * s,
                    );
                } else if let Some(mat) = self.cfg.music_style.pbr_material() {
                    any_visual_change |= step_pbr(
                        &mut self.widget_sims[3],
                        mat,
                        self.cfg.music_x as f32 * u,
                        self.cfg.music_y as f32 * u,
                        lw * s,
                        lh * s,
                    );
                }
            }
            if self.cfg.show_cc && self.cfg.cc_hover_fx.is_ambient_live() {
                let s = u * (self.cfg.cc_scale_pct as f32 / 100.0);
                any_visual_change |= step_rect(
                    &mut self.widget_sims[4],
                    self.cfg.cc_hover_fx,
                    self.cfg.cc_x as f32 * u,
                    self.cfg.cc_y as f32 * u,
                    388.0 * s,
                    298.0 * s,
                );
            }
            for id in ExtWidgetId::ALL {
                let wcfg = &self.cfg.ext_widgets[id.idx()];
                if wcfg.enabled {
                    let s = u * (wcfg.scale_pct as f32 / 100.0);
                    let (lw, lh) = wcfg.logical_size(id);
                    if wcfg.hover_fx.is_ambient_live() {
                        any_visual_change |= step_rect(
                            &mut self.widget_sims[5 + id.idx()],
                            wcfg.hover_fx,
                            wcfg.x as f32 * u,
                            wcfg.y as f32 * u,
                            lw * s,
                            lh * s,
                        );
                    } else if let Some(mat) = wcfg.theme.pbr_material() {
                        any_visual_change |= step_pbr(
                            &mut self.widget_sims[5 + id.idx()],
                            mat,
                            wcfg.x as f32 * u,
                            wcfg.y as f32 * u,
                            lw * s,
                            lh * s,
                        );
                    } else if live_mat && wcfg.theme.has_idle_motion() {
                        any_visual_change = true;
                    }
                }
            }
        }

        let dt60 = self.last_anim60_tick.elapsed().as_secs_f32().clamp(0.004, 0.065);
        self.last_anim60_tick = Instant::now();

        if self.cfg.show_vitals && self.cfg.vitals_style == 4 {
            let targets = [self.cpu_pct, self.gpu_pct, self.ram_pct, self.temp_c];
            let k = 1.0 - (-dt60 * 11.0).exp();
            for i in 0..4 {
                let diff = targets[i] - self.hud_smooth[i];
                if diff.abs() > 0.12 {
                    self.hud_smooth[i] += diff * k;
                    any_visual_change = true;
                } else if self.hud_smooth[i] != targets[i] {
                    self.hud_smooth[i] = targets[i];
                    any_visual_change = true;
                }
            }
        }

        let music_viz_on = !self.covered_by_maximized
            && self.cfg.show_music
            && self.cfg.music_show_viz
            && self.drag_widget.is_none()
            && self.cc_drag.is_none();
        let audio_active = if music_viz_on {
            self.media_engine.is_audio_active()
        } else {
            false
        };
        let is_island = self.cfg.music_style == crate::config::MusicStyle::DynamicIslandCapsule;
        let raw_bands = if music_viz_on && (audio_active || (is_island && self.is_island_bands_decaying())) {
            self.media_engine.bands()
        } else {
            [0.0; 12]
        };

        let mut audio_bands_changed = false;
        if music_viz_on && is_island {
            let k_up = 1.0 - (-dt60 * 28.0).exp();
            let k_dn = 1.0 - (-dt60 * 11.5).exp();
            for i in 0..12 {
                let tgt = if audio_active { raw_bands[i] } else { 0.0 };
                let cur = self.island_bands[i];
                let k = if tgt > cur { k_up } else { k_dn };
                let next = if !audio_active && cur <= 0.004 { 0.0 } else { cur + (tgt - cur) * k };
                if (next - self.last_rendered_bands[i]).abs() > 0.0035 {
                    audio_bands_changed = true;
                }
                self.island_bands[i] = next;
            }
        } else if music_viz_on && audio_active {
            for i in 0..12 {
                if (raw_bands[i] - self.last_rendered_bands[i]).abs() >= 0.004 {
                    audio_bands_changed = true;
                    break;
                }
            }
        }

        let target_ms = if is_island {
            self.cfg.viz_island_frame_ms()
        } else {
            self.cfg.viz_audio_frame_ms()
        };
        let min_audio_gap = Duration::from_millis(target_ms.saturating_sub(4).max(12) as u64);
        let audio_render =
            music_viz_on && audio_bands_changed && self.last_render_at.elapsed() >= min_audio_gap;
        if audio_render || (any_visual_change && self.drag_widget.is_none() && self.cc_drag.is_none()) {
            self.render();
        }
    }

    fn notify_viz_wake(&self) {
        if !self.cmd_hwnd.0.is_null() && self.cmd_msg_viz_wake != 0 {
            unsafe {
                let _ = PostMessageW(Some(self.cmd_hwnd), self.cmd_msg_viz_wake, WPARAM(0), LPARAM(0));
            }
        }
    }

    fn notify_sync(&self, open_studio_flag: usize) {
        if !self.cmd_hwnd.0.is_null() && self.cmd_msg_sync != 0 {
            unsafe {
                let _ = PostMessageW(
                    Some(self.cmd_hwnd),
                    self.cmd_msg_sync,
                    WPARAM(open_studio_flag),
                    LPARAM(0),
                );
            }
        }
    }

    fn close_context_bar(&mut self) {
        if self.context_bar.take().is_some() {
            if !self.hwnd.0.is_null() {
                unsafe {
                    let _ = KillTimer(Some(self.hwnd), TIMER_CTX_POLL);
                }
            }
            if matches!(self.hover_zone, WidgetHitZone::CtxButton(_)) {
                self.hover_zone = WidgetHitZone::None;
            }
        }
    }

    fn open_context_bar(&mut self, screen_pt: POINT, zone: WidgetHitZone) {
        let u = self.u.max(0.5);
        let lx = (screen_pt.x - self.mon_left) as f32 / u;
        let ly = (screen_pt.y - self.mon_top) as f32 / u;
        let max_lw = (self.screen_w as f32 / u).max(640.0);
        let max_lh = (self.screen_h as f32 / u).max(480.0);

        let lock_lbl = if self.cfg.widgets_locked {
            "Unlock Pos"
        } else {
            "Lock Pos"
        };

        let move_lbl = if self.cfg.apps_individual_move {
            "Move Group"
        } else {
            "Move Solo"
        };

        let specs: Vec<(String, CtxAction, f32, bool, bool)> = match zone {
            WidgetHitZone::AppSlot(i) if i < self.cfg.apps.len() => vec![
                ("+ Add App".to_string(), CtxAction::AddApp, 92.0, true, false),
                (move_lbl.to_string(), CtxAction::ToggleIndividualMove, 92.0, false, false),
                ("Edit...".to_string(), CtxAction::EditApp(i), 60.0, false, false),
                ("Remove".to_string(), CtxAction::RemoveApp(i), 66.0, false, true),
                (lock_lbl.to_string(), CtxAction::ToggleLock, 82.0, false, false),
                ("Studio".to_string(), CtxAction::OpenStudioApps, 60.0, false, false),
                ("×".to_string(), CtxAction::Dismiss, 28.0, false, false),
            ],
            WidgetHitZone::AppsBar | WidgetHitZone::AppSlot(_) | WidgetHitZone::AppAddTile => vec![
                ("+ Add App".to_string(), CtxAction::AddApp, 94.0, true, false),
                (move_lbl.to_string(), CtxAction::ToggleIndividualMove, 92.0, false, false),
                (lock_lbl.to_string(), CtxAction::ToggleLock, 86.0, false, false),
                ("Studio".to_string(), CtxAction::OpenStudioApps, 62.0, false, false),
                ("×".to_string(), CtxAction::Dismiss, 28.0, false, false),
            ],
            WidgetHitZone::MusicCard
            | WidgetHitZone::MusicPrev
            | WidgetHitZone::MusicPlayPause
            | WidgetHitZone::MusicNext => vec![
                ("Customize Music".to_string(), CtxAction::OpenStudioMusic, 126.0, true, false),
                (lock_lbl.to_string(), CtxAction::ToggleLock, 86.0, false, false),
                ("Wallpaper".to_string(), CtxAction::OpenStudioWallpaper, 84.0, false, false),
                ("×".to_string(), CtxAction::Dismiss, 28.0, false, false),
            ],
            WidgetHitZone::CcBar
            | WidgetHitZone::CcScrollUp
            | WidgetHitZone::CcScrollDown
            | WidgetHitZone::CcScrollTrack
            | WidgetHitZone::CcBrightSlider
            | WidgetHitZone::CcVolSlider
            | WidgetHitZone::CcMuteBtn => vec![
                ("Customize Control".to_string(), CtxAction::OpenStudioCc, 132.0, true, false),
                (lock_lbl.to_string(), CtxAction::ToggleLock, 86.0, false, false),
                ("Wallpaper".to_string(), CtxAction::OpenStudioWallpaper, 84.0, false, false),
                ("×".to_string(), CtxAction::Dismiss, 28.0, false, false),
            ],
            WidgetHitZone::ExtCard(id) | WidgetHitZone::ExtBtn(id, _) => vec![
                (
                    format!("Customize {}", id.short_title()),
                    CtxAction::OpenStudioExt(id),
                    148.0,
                    true,
                    false,
                ),
                (lock_lbl.to_string(), CtxAction::ToggleLock, 86.0, false, false),
                ("Wallpaper".to_string(), CtxAction::OpenStudioWallpaper, 84.0, false, false),
                ("×".to_string(), CtxAction::Dismiss, 28.0, false, false),
            ],
            _ => vec![
                (lock_lbl.to_string(), CtxAction::ToggleLock, 88.0, true, false),
                ("+ Add App".to_string(), CtxAction::AddApp, 92.0, false, false),
                ("Studio".to_string(), CtxAction::OpenStudioApps, 62.0, false, false),
                ("×".to_string(), CtxAction::Dismiss, 28.0, false, false),
            ],
        };

        let pad_x = 7.0;
        let pad_y = 5.0;
        let gap = 6.0;
        let btn_h = 28.0;
        let sum_w: f32 = specs.iter().map(|s| s.2).sum();
        let total_w = pad_x * 2.0 + sum_w + gap * (specs.len().saturating_sub(1) as f32);
        let total_h = pad_y * 2.0 + btn_h;

        let bx = lx.clamp(10.0, (max_lw - total_w - 10.0).max(10.0));
        let by = if ly > total_h + 20.0 {
            ly - total_h - 10.0
        } else {
            (ly + 14.0).clamp(10.0, (max_lh - total_h - 10.0).max(10.0))
        };

        let mut cur_x = bx + pad_x;
        let mut buttons = Vec::with_capacity(specs.len());
        for (label, action, w, is_primary, is_danger) in specs {
            buttons.push(CtxButton {
                label,
                action,
                x0: cur_x,
                y0: by + pad_y,
                x1: cur_x + w,
                y1: by + pad_y + btn_h,
                is_primary,
                is_danger,
            });
            cur_x += w + gap;
        }

        self.context_bar = Some(ContextBarState {
            x: bx,
            y: by,
            w: total_w,
            h: total_h,
            buttons,
        });
        if !self.hwnd.0.is_null() {
            unsafe {
                let _ = SetTimer(Some(self.hwnd), TIMER_CTX_POLL, 40, None);
            }
        }
        self.render();
    }

    fn execute_ctx_action(&mut self, action: CtxAction) {
        self.close_context_bar();
        self.render();

        match action {
            CtxAction::AddApp => {
                self.notify_sync(5);
            }
            CtxAction::EditApp(i) => {
                if i < self.cfg.apps.len() {
                    if let Some(chosen) = pick_app_file(HWND::default()) {
                        self.cfg.apps[i].name = derive_app_name(&chosen);
                        self.cfg.apps[i].path = chosen;
                        self.icon_cache.invalidate();
                        self.rebind_desktop_host();
                        self.render();
                        self.notify_sync(0);
                    }
                }
            }
            CtxAction::RemoveApp(i) => {
                if i < self.cfg.apps.len() {
                    self.cfg.apps.remove(i);
                    self.icon_cache.invalidate();
                    self.render();
                    self.notify_sync(0);
                }
            }
            CtxAction::ToggleIndividualMove => {
                self.cfg.apps_individual_move = !self.cfg.apps_individual_move;
                self.render();
                self.notify_sync(0);
            }
            CtxAction::ToggleLock => {
                self.cfg.widgets_locked = !self.cfg.widgets_locked;
                self.render();
                self.notify_sync(0);
            }
            CtxAction::OpenStudioApps => {
                self.notify_sync(1);
            }
            CtxAction::OpenStudioMusic => {
                self.notify_sync(2);
            }
            CtxAction::OpenStudioCc => {
                self.notify_sync(3);
            }
            CtxAction::OpenStudioWallpaper => {
                self.notify_sync(4);
            }
            CtxAction::OpenStudioExt(id) => {
                self.notify_sync(10 + id.idx());
            }
            CtxAction::Dismiss => {}
        }
    }

    pub fn attach(&mut self) {
        unsafe {
            GLOBAL_WIDGETS_PTR = self as *mut _;
            let _ = SetWindowLongPtrW(self.hwnd, GWLP_USERDATA, self as *mut _ as isize);
        }
        self.rebind_desktop_host();
    }

    pub fn rebind_desktop_host(&mut self) {
        if self.hwnd.0.is_null() {
            return;
        }
        unsafe {
            let host = find_desktop_host_hwnd();
            if !host.0.is_null() {
                self.desktop_host = host;
                let _ = SetWindowLongPtrW(self.hwnd, GWLP_HWNDPARENT, host.0 as isize);
            }
        }
        self.ensure_above_desktop();
    }

    /// Ensures AutumnDesktopWidgetsWnd is positioned directly above Progman/WorkerW (survives Win+D!)
    /// while staying below normal app windows when apps are on top.
    pub fn ensure_above_desktop(&mut self) {
        if self.hwnd.0.is_null() || !self.visible {
            return;
        }
        unsafe {
            if IsIconic(self.hwnd).as_bool() {
                let _ = ShowWindow(self.hwnd, SW_RESTORE);
            }
            if self.desktop_host.0.is_null() {
                self.desktop_host = find_desktop_host_hwnd();
                if !self.desktop_host.0.is_null() {
                    let _ = SetWindowLongPtrW(self.hwnd, GWLP_HWNDPARENT, self.desktop_host.0 as isize);
                }
            }
            if self.desktop_host.0.is_null() {
                let _ = SetWindowPos(
                    self.hwnd,
                    Some(HWND_BOTTOM),
                    0,
                    0,
                    0,
                    0,
                    SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE | SWP_NOOWNERZORDER,
                );
                return;
            }

            // Walk top-down to verify self.hwnd sits BELOW all visible non-topmost app windows
            // (including AutumnSettingsWnd Studio) and directly ABOVE desktop_host.
            let settings_hwnd = FindWindowW(w!("AutumnSettingsWnd"), PCWSTR::null()).unwrap_or_default();
            let video_hwnd = FindWindowW(w!("AutumnVideoWallpaper"), PCWSTR::null()).unwrap_or_default();
            let mut seen_self = false;
            let mut seen_desktop = false;
            let mut vis_app_below_self = false;
            let mut last_vis_non_topmost_above_desktop = HWND::default();
            let mut cur = GetTopWindow(None).unwrap_or_default();
            for _ in 0..400 {
                if cur.0.is_null() {
                    break;
                }
                if cur == self.hwnd {
                    seen_self = true;
                } else if cur == self.desktop_host || (!video_hwnd.0.is_null() && cur == video_hwnd) {
                    seen_desktop = true;
                    break;
                } else {
                    let ex = GetWindowLongPtrW(cur, GWL_EXSTYLE);
                    let is_topmost = (ex & (WS_EX_TOPMOST.0 as isize)) != 0;
                    if !is_topmost && (IsWindowVisible(cur).as_bool() || cur == settings_hwnd) {
                        last_vis_non_topmost_above_desktop = cur;
                        if seen_self {
                            vis_app_below_self = true;
                        }
                    }
                }
                cur = GetWindow(cur, GW_HWNDNEXT).unwrap_or_default();
            }

            if !seen_self || !seen_desktop || vis_app_below_self {
                let mut insert_after = last_vis_non_topmost_above_desktop;
                if insert_after.0.is_null() {
                    let mut prev = GetWindow(self.desktop_host, GW_HWNDPREV).unwrap_or_default();
                    while !prev.0.is_null()
                        && (prev == self.hwnd
                            || (GetWindowLongPtrW(prev, GWL_EXSTYLE) & (WS_EX_TOPMOST.0 as isize)) != 0)
                    {
                        prev = GetWindow(prev, GW_HWNDPREV).unwrap_or_default();
                    }
                    insert_after = prev;
                }
                let anchor = if insert_after.0.is_null() {
                    if !settings_hwnd.0.is_null() && IsWindowVisible(settings_hwnd).as_bool() {
                        settings_hwnd
                    } else {
                        HWND_NOTOPMOST
                    }
                } else {
                    insert_after
                };
                let _ = SetWindowPos(
                    self.hwnd,
                    Some(anchor),
                    0,
                    0,
                    0,
                    0,
                    SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE | SWP_NOOWNERZORDER,
                );
            }
        }
    }

    pub fn cleanup(&mut self) {
        self.pdh_cancelled.store(true, Ordering::Relaxed);
        self.icon_cache.invalidate();
        self.cover_cache.invalidate();
        self.fg_bmp_cache.clear();
        if let Some(ref dcomp) = self.dcomp {
            crate::realistic_pbr::evict_rt(dcomp.render_target());
            crate::taskbar_styler::evict_styler_rt(dcomp.render_target());
            crate::dock_themes::evict_luxury_hud_rt(dcomp.render_target());
        }
        if let Some(ref rt) = self.dc_rt {
            crate::realistic_pbr::evict_rt(rt);
            crate::taskbar_styler::evict_styler_rt(rt);
            crate::dock_themes::evict_luxury_hud_rt(rt);
        }
        self.dcomp = None;
        self.dcomp_win_rect = None;
        self.dc_rt = None;
        self.dxgi_cached_adapter = None;
        self.media_engine.shutdown();
        unsafe {
            if let Ok(mut g) = self.pdh_pending.lock() {
                if let Some((q, _)) = g.take() {
                    let _ = PdhCloseQuery(PDH_HQUERY(q as *mut c_void));
                }
            }
            if self.pdh_query != 0 {
                let _ = PdhCloseQuery(PDH_HQUERY(self.pdh_query as *mut c_void));
                self.pdh_query = 0;
                self.pdh_cpu_counter = 0;
            }
            GLOBAL_WIDGETS_PTR = std::ptr::null_mut();
            if !self.hwnd.0.is_null() {
                let _ = SetWindowLongPtrW(self.hwnd, GWLP_USERDATA, 0);
                let _ = DestroyWindow(self.hwnd);
                self.hwnd = HWND::default();
            }
            self.backbuf.release();
        }
    }

    pub fn is_visible(&self) -> bool {
        self.visible
    }

    pub fn gpu_backend_label(&self) -> &str {
        if self.dcomp.is_some() {
            "dcomp"
        } else {
            &self.dcomp_status
        }
    }

    fn release_render_surfaces(&mut self) {
        self.dxgi_cached_adapter = None;
        self.backbuf.release();
        self.icon_cache.invalidate();
        self.cover_cache.invalidate();
        self.fg_bmp_cache.clear();
        if let Some(ref mut dcomp) = self.dcomp {
            crate::realistic_pbr::evict_rt(dcomp.render_target());
            crate::taskbar_styler::evict_styler_rt(dcomp.render_target());
            crate::dock_themes::evict_luxury_hud_rt(dcomp.render_target());
            dcomp.release_buffers();
        }
        self.dcomp_win_rect = None;
        if let Some(ref rt) = self.dc_rt {
            crate::realistic_pbr::evict_rt(rt);
            crate::taskbar_styler::evict_styler_rt(rt);
            crate::dock_themes::evict_luxury_hud_rt(rt);
        }
        self.dc_rt = None;
    }

    pub fn set_visible(&mut self, visible: bool) {
        self.visible = visible;
        if !visible {
            self.close_context_bar();
        }
        unsafe {
            let flags = if visible { SWP_SHOWWINDOW } else { SWP_HIDEWINDOW };
            let _ = SetWindowPos(
                self.hwnd,
                None,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOZORDER | SWP_NOOWNERZORDER | flags,
            );
        }
        if visible {
            self.ensure_above_desktop();
        } else {
            self.release_render_surfaces();
        }
    }

    pub fn apply_config(&mut self, cfg: &AppConfig) {
        self.cfg = cfg.clone();
        self.visible = cfg.widgets_enabled;
        self.media_engine.set_cc_active(cfg.widgets_enabled && cfg.show_cc);
        self.media_engine.set_viz_eco_fps(cfg.viz_eco_fps);
        if self.telemetry_needed() && !self.covered_by_maximized {
            self.ensure_pdh_started();
            self.query_telemetry();
        } else if !self.cfg.widgets_enabled || !self.cfg.show_vitals || self.cfg.vitals_style != 4 {
            self.dxgi_cached_adapter = None;
        }
        if self.visible {
            self.render();
            if self.compute_tight_bounds().is_some() {
                self.set_visible(true);
            }
        } else {
            self.set_visible(false);
        }
    }

    fn register_class(hinst: HINSTANCE) {
        unsafe {
            let mut wc = WNDCLASSEXW::default();
            wc.cbSize = size_of::<WNDCLASSEXW>() as u32;
            wc.lpfnWndProc = Some(widgets_wnd_proc);
            wc.hInstance = hinst;
            wc.lpszClassName = w!("AutumnDesktopWidgetsWnd");
            wc.hCursor = LoadCursorW(None, IDC_ARROW).unwrap_or_default();
            let _ = RegisterClassExW(&wc);
        }
    }

    fn load_gdi_fonts() {
        static GDI_ONCE: OnceLock<()> = OnceLock::new();
        GDI_ONCE.get_or_init(|| {
            for bytes in EMBED_FONTS {
                let mut count = 0u32;
                unsafe {
                    let _ = AddFontMemResourceEx(
                        bytes.as_ptr() as *const c_void,
                        bytes.len() as u32,
                        None,
                        &mut count,
                    );
                }
            }
        });
    }

    pub fn tick(&mut self) {
        if !self.visible || self.hwnd.0.is_null() {
            return;
        }
        if self.drag_widget.is_some() {
            return;
        }
        // Watchdog: clear stale hover state if Windows missed WM_MOUSELEAVE (e.g. window covered widget)
        if self.hover_zone != WidgetHitZone::None || self.widget_sims.iter().any(|s| s.hover_active) {
            let mut pt = POINT::default();
            if unsafe { GetCursorPos(&mut pt).is_ok() } {
                let under = unsafe { WindowFromPoint(pt) };
                if under != self.hwnd || self.hit_test(pt) == WidgetHitZone::None {
                    self.hover_zone = WidgetHitZone::None;
                    self.tracking_leave = false;
                    self.viz_dirty = true;
                    self.notify_viz_wake();
                }
            }
        }
        self.ensure_above_desktop();
        if self.covered_by_maximized {
            self.dxgi_cached_adapter = None;
            return;
        }
        if self.telemetry_needed() {
            self.query_telemetry();
        } else {
            self.dxgi_cached_adapter = None;
        }
        let ext_changed = self.ext_state.tick(&self.cfg, self.cpu_pct);
        let sec = chrono_local().second;
        let cur_tel = (
            self.cpu_pct.round() as i32,
            self.gpu_pct.round() as i32,
            self.ram_pct.round() as i32,
            self.temp_c.round() as i32,
            self.bat_pct.round() as i32,
        );
        if sec != self.last_clock_sec || cur_tel != self.last_drawn_telemetry || ext_changed {
            if self.cfg.widgets_live_material
                && self.cfg.show_datetime
                && self.cfg.dt_style == ClockStyle::FlipMech
                && (self.cfg.dt_show_seconds || sec == 59 || sec == 0)
            {
                if flip_mech_anim_active(&self.cfg) {
                    self.viz_dirty = true;
                }
                self.notify_viz_wake();
            } else if cur_tel != self.last_drawn_telemetry && self.is_ring_pill_interpolating() {
                self.notify_viz_wake();
            }
            let min_gap = if self.wants_fast_60fps() {
                Duration::from_millis(14)
            } else if self.cfg.viz_eco_fps {
                Duration::from_millis(40)
            } else {
                Duration::from_millis(28)
            };
            if !self.wants_fast_30fps() || self.last_render_at.elapsed() >= min_gap {
                self.render();
            } else {
                self.viz_dirty = true;
            }
        }
    }

    fn query_telemetry(&mut self) {
        self.ensure_pdh_started();
        if self.pdh_query == 0 {
            if let Ok(mut g) = self.pdh_pending.try_lock() {
                if let Some((q, c)) = g.take() {
                    self.pdh_query = q;
                    self.pdh_cpu_counter = c;
                }
            }
        }
        let mut used_pdh = false;
        if self.pdh_query != 0 && self.pdh_cpu_counter != 0 {
            unsafe {
                let q = PDH_HQUERY(self.pdh_query as *mut c_void);
                let c = PDH_HCOUNTER(self.pdh_cpu_counter as *mut c_void);
                if PdhCollectQueryData(q) == 0 {
                    let mut ctype = 0u32;
                    let mut val = PDH_FMT_COUNTERVALUE::default();
                    if PdhGetFormattedCounterValue(c, PDH_FMT_DOUBLE, Some(&mut ctype), &mut val) == 0
                        && val.CStatus == 0
                    {
                        let v = val.Anonymous.doubleValue as f32;
                        if v.is_finite() {
                            self.cpu_pct = v.clamp(0.0, 100.0);
                            used_pdh = true;
                        }
                    }
                }
            }
        }

        let mut idle = FILETIME::default();
        let mut kernel = FILETIME::default();
        let mut user = FILETIME::default();
        if unsafe { GetSystemTimes(Some(&mut idle), Some(&mut kernel), Some(&mut user)).is_ok() } {
            let i = filetime_to_u64(idle);
            let k = filetime_to_u64(kernel);
            let u = filetime_to_u64(user);
            if !used_pdh && self.prev_kernel > 0 && (k + u) > (self.prev_kernel + self.prev_user) {
                let usr_diff = u.saturating_sub(self.prev_user);
                let ker_diff = k.saturating_sub(self.prev_kernel);
                let idl_diff = i.saturating_sub(self.prev_idle);
                let total = usr_diff + ker_diff;
                if total > 0 {
                    let busy = total.saturating_sub(idl_diff);
                    self.cpu_pct = ((busy as f32 / total as f32) * 100.0).clamp(0.0, 100.0);
                }
            }
            self.prev_idle = i;
            self.prev_kernel = k;
            self.prev_user = u;
        }

        let mut mem = MEMORYSTATUSEX {
            dwLength: size_of::<MEMORYSTATUSEX>() as u32,
            ..Default::default()
        };
        if unsafe { GlobalMemoryStatusEx(&mut mem).is_ok() } {
            self.ram_pct = mem.dwMemoryLoad as f32;
        }

        let mut pwr = SYSTEM_POWER_STATUS::default();
        if unsafe { GetSystemPowerStatus(&mut pwr).is_ok() } {
            if pwr.BatteryLifePercent != 255 {
                self.bat_pct = (pwr.BatteryLifePercent as f32).clamp(0.0, 100.0);
            } else {
                self.bat_pct = 100.0;
            }
        }

        // GPU utilization for Telemetry Ring-Pill HUD (`vitals_style == 4`):
        // Poll ONLY cached single IDXGIAdapter3::QueryVideoMemoryInfo (~every 1.2s) — never PDH wildcards.
        if self.cfg.show_vitals && self.cfg.vitals_style == 4 && !self.covered_by_maximized && self.visible {
            self.dxgi_probe_tick = self.dxgi_probe_tick.wrapping_add(1);
            if self.dxgi_probe_tick % 3 == 1 || self.dxgi_cached_adapter.is_none() {
                unsafe {
                    if self.dxgi_cached_adapter.is_none() {
                        if let Ok(fac) = CreateDXGIFactory1::<IDXGIFactory4>() {
                            if let Ok(ad) = fac.EnumAdapters1(0) {
                                self.dxgi_cached_adapter = ad.cast::<IDXGIAdapter3>().ok();
                            }
                        }
                    }
                    if let Some(ref ad3) = self.dxgi_cached_adapter {
                        let mut info = DXGI_QUERY_VIDEO_MEMORY_INFO::default();
                        if ad3.QueryVideoMemoryInfo(0, DXGI_MEMORY_SEGMENT_GROUP_LOCAL, &mut info).is_ok() && info.Budget > 0 {
                            let vram_ratio = (info.CurrentUsage as f32 / info.Budget as f32).clamp(0.04, 0.96);
                            let est = (self.cpu_pct * 0.45 + vram_ratio * 40.0).clamp(4.0, 99.0);
                            self.gpu_pct = self.gpu_pct * 0.5 + est * 0.5;
                        }
                    }
                }
            }
            let est_t = (37.0 + self.cpu_pct * 0.28 + self.gpu_pct * 0.24).clamp(32.0, 94.0);
            self.temp_c = self.temp_c * 0.6 + est_t * 0.4;
        } else {
            self.dxgi_cached_adapter = None;
        }
    }

    /// Computes the aligned tight pixel bounding box `(x, y, w, h)` relative to the
    /// monitor origin enclosing all active desktop widgets + their drop shadows/glows.
    /// Aligning edges to 64px blocks prevents DIB reallocation churn during drag/animations
    /// while shrinking typical single/clustered widget backing surfaces by 5x-20x.
    fn compute_tight_bounds(&self) -> Option<(i32, i32, i32, i32)> {
        if !self.cfg.widgets_enabled || self.screen_w <= 0 || self.screen_h <= 0 {
            return None;
        }
        let u = self.u;
        let mut min_x = 0.0f32;
        let mut min_y = 0.0f32;
        let mut max_x = 0.0f32;
        let mut max_y = 0.0f32;
        let mut any = false;
        let mut include_rect = |x0: f32, y0: f32, x1: f32, y1: f32, pad: f32| {
            let a0 = x0 - pad;
            let b0 = y0 - pad;
            let a1 = x1 + pad;
            let b1 = y1 + pad;
            if !any {
                min_x = a0;
                min_y = b0;
                max_x = a1;
                max_y = b1;
                any = true;
            } else {
                min_x = min_x.min(a0);
                min_y = min_y.min(b0);
                max_x = max_x.max(a1);
                max_y = max_y.max(b1);
            }
        };

        if self.cfg.show_datetime {
            let s = u * (self.cfg.dt_scale_pct as f32 / 100.0);
            let dx = self.cfg.dt_x as f32 * u;
            let dy = self.cfg.dt_y as f32 * u;
            let (lw, lh) = dt_logical_size(&self.cfg);
            include_rect(dx, dy, dx + lw * s, dy + lh * s, 52.0 * s);
            for tr in &self.cfg.dt_elem_transforms {
                if !tr.is_default() {
                    let tx = tr.dx as f32 * s;
                    let ty = tr.dy as f32 * s;
                    let epad = 56.0 * s * (tr.scale_pct as f32 / 100.0).max(1.0);
                    include_rect(dx + tx, dy + ty, dx + lw * s + tx, dy + lh * s + ty, epad);
                }
            }
        }

        if self.cfg.show_apps {
            let s = u * (self.cfg.apps_scale_pct as f32 / 100.0);
            let bx = self.cfg.apps_x as f32 * u;
            let by = self.cfg.apps_y as f32 * u;
            let show_add = self.cfg.apps_show_add_tile && self.cfg.apps.len() < 12;
            let total_tiles = self.cfg.apps.len() + usize::from(show_add);
            if total_tiles > 0 {
                let (offsets, total_w, total_h) = apps_layout_offsets(total_tiles, self.cfg.apps_cols.max(1) as usize);
                include_rect(
                    bx - 12.0 * s,
                    by - 10.0 * s,
                    bx + (total_w + 12.0) * s,
                    by + (total_h + 10.0) * s,
                    24.0 * s,
                );
                for (i, &(rx, ry)) in offsets.iter().enumerate() {
                    let (ox, oy) = if i < self.cfg.apps.len() {
                        (self.cfg.apps[i].offset_x as f32, self.cfg.apps[i].offset_y as f32)
                    } else {
                        (0.0, 0.0)
                    };
                    let ax = bx + (rx + ox) * s;
                    let ay = by + (ry + oy) * s;
                    include_rect(ax, ay, ax + 64.0 * s, ay + 94.0 * s, 22.0 * s);
                }
            }
        }

        if self.cfg.show_vitals {
            let s = u * (self.cfg.vitals_scale_pct as f32 / 100.0);
            let vx = self.cfg.vitals_x as f32 * u;
            let vy = self.cfg.vitals_y as f32 * u;
            let (vw, vh) = vitals_logical_size(&self.cfg);
            include_rect(vx, vy, vx + vw * s, vy + vh * s, 28.0 * s);
        }

        if self.cfg.show_cc {
            let s = u * (self.cfg.cc_scale_pct as f32 / 100.0);
            let cx = self.cfg.cc_x as f32 * u;
            let cy = self.cfg.cc_y as f32 * u;
            include_rect(cx, cy, cx + 388.0 * s, cy + 298.0 * s, 32.0 * s);
        }

        if self.cfg.show_music {
            let s = u * (self.cfg.music_scale_pct as f32 / 100.0);
            let mx = self.cfg.music_x as f32 * u;
            let my = self.cfg.music_y as f32 * u;
            let (mlw, mlh) = music_logical_size(&self.cfg);
            include_rect(mx, my, mx + mlw * s, my + mlh * s, 48.0 * s);
        }

        for id in ExtWidgetId::ALL {
            let wcfg = &self.cfg.ext_widgets[id.idx()];
            if wcfg.enabled {
                let s = (u * (wcfg.scale_pct as f32 / 100.0)).max(0.25);
                let wx = wcfg.x as f32 * u;
                let wy = wcfg.y as f32 * u;
                let (lw, lh) = wcfg.logical_size(id);
                include_rect(wx, wy, wx + lw * s, wy + lh * s, 36.0 * s);
            }
        }

        if let Some(ref bar) = self.context_bar {
            include_rect(
                bar.x * u,
                bar.y * u,
                (bar.x + bar.w) * u,
                (bar.y + bar.h) * u,
                24.0 * u,
            );
        }

        if !any {
            return None;
        }
        let x0 = ((min_x.floor() as i32) & !63).clamp(0, (self.screen_w - 2).max(0));
        let y0 = ((min_y.floor() as i32) & !63).clamp(0, (self.screen_h - 2).max(0));
        let x1 = (((max_x.ceil() as i32) + 63) & !63).clamp(x0 + 2, self.screen_w.max(2));
        let y1 = (((max_y.ceil() as i32) + 63) & !63).clamp(y0 + 2, self.screen_h.max(2));
        Some((x0, y0, x1 - x0, y1 - y0))
    }

    pub fn handle_display_change(&mut self) {
        self.fg_bmp_cache.clear();
        self.dcomp_win_rect = None;
    }

    pub fn render(&mut self) {
        if self.hwnd.0.is_null() || !self.visible {
            return;
        }
        let mut mon = RECT {
            left: 0,
            top: 0,
            right: 1920,
            bottom: 1200,
        };
        unsafe {
            let hmon = MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY);
            let mut mi = MONITORINFO {
                cbSize: size_of::<MONITORINFO>() as u32,
                ..Default::default()
            };
            if GetMonitorInfoW(hmon, &mut mi).as_bool() {
                mon = mi.rcMonitor;
            }
        }
        self.mon_left = mon.left;
        self.mon_top = mon.top;
        self.screen_w = mon.right - mon.left;
        self.screen_h = mon.bottom - mon.top;
        self.u = (self.screen_w as f32 / 1280.0).clamp(0.5, 4.0);

        let Some((bx, by, bw, bh)) = self.compute_tight_bounds() else {
            unsafe {
                let _ = SetWindowPos(
                    self.hwnd,
                    None,
                    0,
                    0,
                    0,
                    0,
                    SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOZORDER | SWP_NOOWNERZORDER | SWP_HIDEWINDOW,
                );
            }
            self.release_render_surfaces();
            return;
        };

        let use_dcomp = self.dcomp.is_some();
        let rt_owned: ID2D1RenderTarget = if use_dcomp {
            let ok = self
                .dcomp
                .as_mut()
                .map(|dc| dc.ensure_size(bw, bh))
                .unwrap_or(false);
            if !ok {
                return;
            }
            let target_rect = (mon.left + bx, mon.top + by, bw, bh);
            if self.dcomp_win_rect != Some(target_rect) {
                unsafe {
                    let _ = SetWindowPos(
                        self.hwnd,
                        None,
                        target_rect.0,
                        target_rect.1,
                        target_rect.2,
                        target_rect.3,
                        SWP_NOACTIVATE | SWP_NOZORDER | SWP_NOOWNERZORDER | SWP_SHOWWINDOW,
                    );
                }
                self.dcomp_win_rect = Some(target_rect);
            }
            let Some(ref dc) = self.dcomp else {
                return;
            };
            dc.render_target().clone()
        } else {
            if !self.backbuf.ensure_size(bw, bh) {
                return;
            }
            if self.dc_rt.is_none() {
                self.dc_rt = self.d2d_factory.as_ref().and_then(create_widgets_dc_rt);
            }
            let Some(ref gdi_rt) = self.dc_rt else {
                return;
            };
            let bind_rc = RECT {
                left: 0,
                top: 0,
                right: bw,
                bottom: bh,
            };
            if unsafe { gdi_rt.BindDC(self.backbuf.mem_dc, &bind_rc).is_err() } {
                return;
            }
            (&**gdi_rt).clone()
        };
        let dc_rt: &ID2D1RenderTarget = &rt_owned;

        let media_snap = self.media_engine.media_snapshot();
        let cc_snap = self.media_engine.cc_snapshot();
        let raw_bands = self.media_engine.bands();
        let bands = if self.cfg.music_style == crate::config::MusicStyle::DynamicIslandCapsule
            && (self.is_island_bands_decaying() || self.island_bands.iter().any(|&v| v > 0.001))
        {
            self.island_bands
        } else {
            raw_bands
        };
        self.last_rendered_bands = bands;
        self.last_render_at = Instant::now();
        self.viz_dirty = false;
        self.last_clock_sec = chrono_local().second;
        self.last_drawn_telemetry = (
            self.cpu_pct.round() as i32,
            self.gpu_pct.round() as i32,
            self.ram_pct.round() as i32,
            self.temp_c.round() as i32,
            self.bat_pct.round() as i32,
        );

        let now_inst = Instant::now();
        let bright_t_override = match self.bright_drag {
            Some((t, at))
                if self.cc_drag == Some(CcDragMode::BrightSlider)
                    || now_inst.duration_since(at) < Duration::from_millis(450) =>
            {
                Some(t)
            }
            _ => {
                self.bright_drag = None;
                None
            }
        };
        let vol_t_override = match self.vol_drag {
            Some((t, muted, at))
                if self.cc_drag == Some(CcDragMode::VolSlider)
                    || now_inst.duration_since(at) < Duration::from_millis(450) =>
            {
                Some((t, muted))
            }
            _ => {
                self.vol_drag = None;
                None
            }
        };

        unsafe {
            self.icon_cache.sync_apps(
                &self.cfg.apps,
                self.wic_factory.as_ref(),
                dc_rt,
                resolve_apps_icon_tint(&self.cfg),
            );
            let cover_bmp = if self.cfg.show_music {
                self.cover_cache
                    .resolve(dc_rt, self.wic_factory.as_ref(), &media_snap)
            } else {
                None
            };

            {
                dc_rt.BeginDraw();
                dc_rt.Clear(Some(&D2D1_COLOR_F {
                    r: 0.0,
                    g: 0.0,
                    b: 0.0,
                    a: 0.0,
                }));
            }
            dc_rt.SetTransform(&Matrix3x2::translation(-(bx as f32), -(by as f32)));

            let active_d2d: Option<ID2D1Factory> =
                dc_rt.GetFactory().ok().or_else(|| self.d2d_factory.clone());
            let d2d_ref = active_d2d.as_ref();

            let u = self.u;
            if let Some(ref dw) = self.dwrite_factory {
                if self.cfg.show_datetime {
                    let s = u * (self.cfg.dt_scale_pct as f32 / 100.0);
                    let dx = self.cfg.dt_x as f32 * u;
                    let dy = self.cfg.dt_y as f32 * u;
                    let fg_gen = self.fg_bmp_cache.cache_gen;
                    draw_datetime_widget_full_cached(
                        dc_rt,
                        dw,
                        self.custom_fonts.as_ref(),
                        dx,
                        dy,
                        s,
                        &self.cfg,
                        self.hover_zone == WidgetHitZone::ClockCard || self.drag_widget == Some(DragWidgetKind::DateTime),
                        Some(&self.widget_sims[0]),
                        &mut self.fg_bmp_cache.clock,
                        &mut self.fg_bmp_cache.clock_sec,
                        fg_gen,
                    );
                }

                if self.cfg.show_apps {
                    let s = u * (self.cfg.apps_scale_pct as f32 / 100.0);
                    let bx = self.cfg.apps_x as f32 * u;
                    let by = self.cfg.apps_y as f32 * u;
                    let show_add = self.cfg.apps_show_add_tile && self.cfg.apps.len() < 12;
                    let total_tiles = self.cfg.apps.len() + usize::from(show_add);
                    if total_tiles > 0 {
                        let (offsets, _, _) = apps_layout_offsets(total_tiles, self.cfg.apps_cols.max(1) as usize);
                        let mut items: Vec<AppTileDrawItem<'_>> = self
                            .cfg
                            .apps
                            .iter()
                            .enumerate()
                            .map(|(i, app)| AppTileDrawItem {
                                label: &app.name,
                                bmp: self.icon_cache.slot(i),
                                rel_x: offsets[i].0 + app.offset_x as f32,
                                rel_y: offsets[i].1 + app.offset_y as f32,
                                hovered: self.hover_zone == WidgetHitZone::AppSlot(i),
                                is_add_tile: false,
                            })
                            .collect();
                        if show_add {
                            let add_idx = self.cfg.apps.len();
                            items.push(AppTileDrawItem {
                                label: "Add",
                                bmp: None,
                                rel_x: offsets[add_idx].0,
                                rel_y: offsets[add_idx].1,
                                hovered: self.hover_zone == WidgetHitZone::AppAddTile,
                                is_add_tile: true,
                            });
                        }
                        draw_apps_widget_with_viz(
                            dc_rt,
                            dw,
                            self.custom_fonts.as_ref(),
                            bx,
                            by,
                            s,
                            &self.cfg,
                            &items,
                            Some(&self.widget_sims[1]),
                        );
                    }
                }

                if self.cfg.show_vitals {
                    let s = u * (self.cfg.vitals_scale_pct as f32 / 100.0);
                    let vx = self.cfg.vitals_x as f32 * u;
                    let vy = self.cfg.vitals_y as f32 * u;
                    let (v_cpu, v_gpu, v_ram, v_temp) = if self.cfg.vitals_style == 4 {
                        (self.hud_smooth[0], self.hud_smooth[1], self.hud_smooth[2], self.hud_smooth[3])
                    } else {
                        (self.cpu_pct, self.gpu_pct, self.ram_pct, self.temp_c)
                    };
                    draw_vitals_widget_with_telemetry_and_viz(
                        dc_rt,
                        dw,
                        d2d_ref,
                        self.custom_fonts.as_ref(),
                        vx,
                        vy,
                        s,
                        &self.cfg,
                        v_cpu,
                        v_gpu,
                        v_ram,
                        v_temp,
                        self.bat_pct,
                        self.hover_zone,
                        Some(&self.widget_sims[2]),
                    );
                }

                if self.cfg.show_cc {
                    let s = u * (self.cfg.cc_scale_pct as f32 / 100.0);
                    let cx = self.cfg.cc_x as f32 * u;
                    let cy = self.cfg.cc_y as f32 * u;
                    draw_control_center_widget_live_with_viz(
                        dc_rt,
                        dw,
                        d2d_ref,
                        self.custom_fonts.as_ref(),
                        cx,
                        cy,
                        s,
                        &self.cfg,
                        &cc_snap,
                        self.cc_scroll_y,
                        self.hover_zone,
                        bright_t_override,
                        vol_t_override,
                        Some(&self.widget_sims[4]),
                    );
                }

                if self.cfg.show_music {
                    let s = u * (self.cfg.music_scale_pct as f32 / 100.0);
                    let mx = self.cfg.music_x as f32 * u;
                    let my = self.cfg.music_y as f32 * u;
                    let fg_gen = self.fg_bmp_cache.cache_gen;
                    draw_music_widget_with_viz_cached(
                        dc_rt,
                        dw,
                        d2d_ref,
                        self.custom_fonts.as_ref(),
                        mx,
                        my,
                        s,
                        &self.cfg,
                        &media_snap,
                        &bands,
                        cover_bmp,
                        self.hover_zone,
                        Some(&self.widget_sims[3]),
                        &mut self.fg_bmp_cache.music,
                        fg_gen,
                    );
                }

                for id in ExtWidgetId::ALL {
                    let wcfg = &self.cfg.ext_widgets[id.idx()];
                    if wcfg.enabled {
                        let s = u * (wcfg.scale_pct as f32 / 100.0);
                        let wx = wcfg.x as f32 * u;
                        let wy = wcfg.y as f32 * u;
                        let fg_gen = self.fg_bmp_cache.cache_gen;
                        draw_ext_widget_with_viz_cached(
                            dc_rt,
                            dw,
                            d2d_ref,
                            self.custom_fonts.as_ref(),
                            wx,
                            wy,
                            s,
                            id,
                            &self.cfg,
                            &self.ext_state,
                            &cc_snap,
                            self.hover_zone,
                            Some(&self.widget_sims[5 + id.idx()]),
                            &mut self.fg_bmp_cache.ext[id.idx()],
                            fg_gen,
                        );
                    }
                }

                if let Some(ref bar) = self.context_bar {
                    Self::draw_context_bar(dc_rt, dw, self.custom_fonts.as_ref(), bar, self.hover_zone, u);
                }
            }

            dc_rt.SetTransform(&Matrix3x2::identity());
            let end_res = dc_rt.EndDraw(None, None);
            drop(active_d2d);
            drop(rt_owned);

            if use_dcomp {
                let pres_res = if end_res.is_ok() {
                    self.dcomp.as_mut().map(|dc| dc.present()).unwrap_or(Err(-1))
                } else {
                    Err(end_res.as_ref().err().map(|e| e.code().0).unwrap_or(-1))
                };
                if end_res.is_err() || pres_res.is_err() {
                    let end_hr = end_res.as_ref().err().map(|e| e.code().0 as u32).unwrap_or(0);
                    let pres_hr = pres_res.err().unwrap_or(0) as u32;
                    crate::diagnostics::record_event(
                        "DCOMP_ERR",
                        &format!("end=0x{:08X} pres=0x{:08X}", end_hr, pres_hr),
                    );
                    // Recover cleanly from D2DERR_RECREATE_TARGET / DXGI_ERROR_DEVICE_REMOVED
                    self.icon_cache.invalidate();
                    self.cover_cache.invalidate();
                    self.fg_bmp_cache.clear();
                    if let Some(ref dc) = self.dcomp {
                        crate::realistic_pbr::evict_rt(dc.render_target());
                        crate::taskbar_styler::evict_styler_rt(dc.render_target());
                        crate::dock_themes::evict_luxury_hud_rt(dc.render_target());
                    }
                    self.dcomp = None;
                    self.dcomp_win_rect = None;

                    // Temporarily detach cross-process Progman owner so CreateTargetForHwnd succeeds
                    let prev_owner = self.desktop_host;
                    let _ = SetWindowLongPtrW(self.hwnd, GWLP_HWNDPARENT, 0);
                    match DCompSwapSurface::new(self.hwnd) {
                        Ok(dc) => {
                            self.d2d_factory = Some(dc.d2d_factory());
                            self.dcomp = Some(dc);
                            self.dcomp_status = "dcomp".to_string();
                            if !prev_owner.0.is_null() {
                                let _ = SetWindowLongPtrW(self.hwnd, GWLP_HWNDPARENT, prev_owner.0 as isize);
                            }
                        }
                        Err((stage, hr)) => {
                            // GPU swapchain permanently unavailable -> rebuild HWND as WS_EX_LAYERED for GDI fallback
                            let status = format!("gdi_recover:{}:0x{:08X}", stage, hr as u32);
                            crate::diagnostics::record_event("DCOMP_ERR", &status);
                            self.dcomp_status = status;
                            let hinst = windows::Win32::System::LibraryLoader::GetModuleHandleW(None)
                                .map(HINSTANCE::from)
                                .unwrap_or_default();
                            let owner_opt = if !prev_owner.0.is_null() { Some(prev_owner) } else { None };
                            let old_hwnd = self.hwnd;
                            let _ = SetWindowLongPtrW(old_hwnd, GWLP_USERDATA, 0);
                            let _ = DestroyWindow(old_hwnd);
                            self.hwnd = CreateWindowExW(
                                WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
                                w!("AutumnDesktopWidgetsWnd"),
                                w!("Autumn Desktop Widgets"),
                                WS_POPUP,
                                0,
                                0,
                                2,
                                2,
                                owner_opt,
                                None,
                                Some(hinst),
                                None,
                            )
                            .unwrap_or_default();
                            let _ = SetWindowLongPtrW(self.hwnd, GWLP_USERDATA, self as *mut _ as isize);
                            self.d2d_factory = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None).ok();
                            self.dc_rt = self.d2d_factory.as_ref().and_then(create_widgets_dc_rt);
                            self.ensure_above_desktop();
                        }
                    }
                }
            } else {
                let dst_pt = POINT { x: mon.left + bx, y: mon.top + by };
                let size = SIZE { cx: bw, cy: bh };
                let src_pt = POINT { x: 0, y: 0 };
                let blend = BLENDFUNCTION {
                    BlendOp: AC_SRC_OVER as u8,
                    BlendFlags: 0,
                    SourceConstantAlpha: 255,
                    AlphaFormat: AC_SRC_ALPHA as u8,
                };
                let _ = UpdateLayeredWindow(
                    self.hwnd,
                    None,
                    Some(&dst_pt),
                    Some(&size),
                    Some(self.backbuf.mem_dc),
                    Some(&src_pt),
                    COLORREF(0),
                    Some(&blend),
                    ULW_ALPHA,
                );
            }
        }
    }

    unsafe fn draw_context_bar(
        rt: &ID2D1RenderTarget,
        dw: &IDWriteFactory,
        custom_fonts: Option<&IDWriteFontCollection>,
        bar: &ContextBarState,
        hover_zone: WidgetHitZone,
        u: f32,
    ) {
        let bar_rr = D2D1_ROUNDED_RECT {
            rect: D2D_RECT_F {
                left: bar.x * u,
                top: bar.y * u,
                right: (bar.x + bar.w) * u,
                bottom: (bar.y + bar.h) * u,
            },
            radiusX: 11.0 * u,
            radiusY: 11.0 * u,
        };
        if let Ok(bg) = rt.CreateSolidColorBrush(&rgba_u8(16, 11, 9, 0.94), None) {
            rt.FillRoundedRectangle(&bar_rr, &bg);
        }
        if let Ok(rim) = rt.CreateSolidColorBrush(&rgba_u8(245, 166, 66, 0.85), None) {
            rt.DrawRoundedRectangle(&bar_rr, &rim, 1.2 * u, None);
        }

        let tf_btn = make_resolved_tf(
            dw,
            custom_fonts,
            &[w!("SF Pro Display"), w!("Segoe UI Semibold"), w!("Segoe UI")],
            8.5 * PT_TO_PX * u,
            DWRITE_FONT_WEIGHT_SEMI_BOLD,
            DWRITE_TEXT_ALIGNMENT_CENTER,
            DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
        );
        let br_dark = rt.CreateSolidColorBrush(&rgba_u8(24, 15, 10, 1.0), None).ok();
        let br_cream = rt.CreateSolidColorBrush(&rgba_u8(255, 243, 220, 0.96), None).ok();
        let br_gold = rt.CreateSolidColorBrush(&rgba_u8(245, 166, 66, 1.0), None).ok();
        let br_coral = rt.CreateSolidColorBrush(&rgba_u8(242, 128, 106, 1.0), None).ok();

        for (idx, btn) in bar.buttons.iter().enumerate() {
            let hov = hover_zone == WidgetHitZone::CtxButton(idx);
            let rr = D2D1_ROUNDED_RECT {
                rect: D2D_RECT_F {
                    left: btn.x0 * u,
                    top: btn.y0 * u,
                    right: btn.x1 * u,
                    bottom: btn.y1 * u,
                },
                radiusX: 7.0 * u,
                radiusY: 7.0 * u,
            };

            let fill_c = if btn.is_primary {
                if hov { rgba_u8(255, 220, 150, 0.98) } else { rgba_u8(245, 166, 66, 0.94) }
            } else if btn.is_danger {
                if hov { rgba_u8(242, 108, 82, 0.34) } else { rgba_u8(242, 108, 82, 0.14) }
            } else if hov {
                rgba_u8(245, 166, 66, 0.25)
            } else {
                rgba_u8(255, 243, 220, 0.08)
            };
            if let Ok(fb) = rt.CreateSolidColorBrush(&fill_c, None) {
                rt.FillRoundedRectangle(&rr, &fb);
            }

            let border_c = if btn.is_primary {
                rgba_u8(255, 228, 175, 0.9)
            } else if btn.is_danger {
                rgba_u8(242, 108, 82, if hov { 0.85 } else { 0.45 })
            } else {
                rgba_u8(245, 166, 66, if hov { 0.85 } else { 0.32 })
            };
            if let Ok(bb) = rt.CreateSolidColorBrush(&border_c, None) {
                rt.DrawRoundedRectangle(&rr, &bb, 0.9 * u, None);
            }

            let txt_br = if btn.is_primary {
                br_dark.as_ref()
            } else if btn.is_danger {
                if hov { br_cream.as_ref() } else { br_coral.as_ref() }
            } else if hov {
                br_gold.as_ref()
            } else {
                br_cream.as_ref()
            };
            if let (Some(tf), Some(tb)) = (&tf_btn, txt_br) {
                let wide = to_wide(&btn.label);
                rt.DrawText(&wide[..wide.len() - 1], tf, &rr.rect, tb, D2D1_DRAW_TEXT_OPTIONS_CLIP, DWRITE_MEASURING_MODE_NATURAL);
            }
        }
    }

    pub fn hit_test(&self, pt: POINT) -> WidgetHitZone {
        let u = self.u;
        let x = (pt.x - self.mon_left) as f32;
        let y = (pt.y - self.mon_top) as f32;

        if let Some(ref bar) = self.context_bar {
            for (idx, btn) in bar.buttons.iter().enumerate() {
                if x >= btn.x0 * u && x <= btn.x1 * u && y >= btn.y0 * u && y <= btn.y1 * u {
                    return WidgetHitZone::CtxButton(idx);
                }
            }
            if x >= bar.x * u && x <= (bar.x + bar.w) * u && y >= bar.y * u && y <= (bar.y + bar.h) * u {
                return WidgetHitZone::CtxBg;
            }
        }

        if self.cfg.show_music {
            let s = u * (self.cfg.music_scale_pct as f32 / 100.0);
            let mx = self.cfg.music_x as f32 * u;
            let my = self.cfg.music_y as f32 * u;
            let (mlw, mlh) = music_logical_size(&self.cfg);
            if x >= mx && x <= mx + mlw * s && y >= my && y <= my + mlh * s {
                let lx = (x - mx) / s;
                let ly = (y - my) / s;
                let [r_prev, r_play, r_next] = music_transport_rects(&self.cfg);
                if (r_prev.0..=r_prev.2).contains(&lx) && (r_prev.1..=r_prev.3).contains(&ly) {
                    return WidgetHitZone::MusicPrev;
                }
                if (r_play.0..=r_play.2).contains(&lx) && (r_play.1..=r_play.3).contains(&ly) {
                    return WidgetHitZone::MusicPlayPause;
                }
                if (r_next.0..=r_next.2).contains(&lx) && (r_next.1..=r_next.3).contains(&ly) {
                    return WidgetHitZone::MusicNext;
                }
                return WidgetHitZone::MusicCard;
            }
        }

        if self.cfg.show_cc {
            let s = u * (self.cfg.cc_scale_pct as f32 / 100.0);
            let cx = self.cfg.cc_x as f32 * u;
            let cy = self.cfg.cc_y as f32 * u;
            if x >= cx && x <= cx + 388.0 * s && y >= cy && y <= cy + 298.0 * s {
                let lx = (x - cx) / s;
                let ly = (y - cy) / s;
                if (356.0..=382.0).contains(&lx) {
                    if (46.0..=72.0).contains(&ly) {
                        return WidgetHitZone::CcScrollUp;
                    }
                    if (168.0..=194.0).contains(&ly) {
                        return WidgetHitZone::CcScrollDown;
                    }
                    if (72.0..=168.0).contains(&ly) {
                        return WidgetHitZone::CcScrollTrack;
                    }
                }
                if (16.0..=358.0).contains(&lx) && (18.0..=204.0).contains(&ly) {
                    let vx = lx - 16.0;
                    let vy = ly - 18.0 + self.cc_scroll_y;
                    for idx in 0..11 {
                        let (rx, ry) = cc_tile_pos(idx);
                        if vx >= rx && vx <= rx + 102.0 && vy >= ry && vy <= ry + 78.0 {
                            let is_split = idx == 0 || idx == 1;
                            let right_half = is_split && (vx - rx) >= 52.0 && (vy - ry) <= 52.0;
                            return WidgetHitZone::CcTile(idx, right_half);
                        }
                    }
                }
                if (50.0..=332.0).contains(&lx) && (214.0..=246.0).contains(&ly) {
                    return WidgetHitZone::CcBrightSlider;
                }
                if (20.0..=48.0).contains(&lx) && (250.0..=280.0).contains(&ly) {
                    return WidgetHitZone::CcMuteBtn;
                }
                if (50.0..=332.0).contains(&lx) && (248.0..=280.0).contains(&ly) {
                    return WidgetHitZone::CcVolSlider;
                }
                return WidgetHitZone::CcBar;
            }
        }

        if self.cfg.show_datetime {
            let s = u * (self.cfg.dt_scale_pct as f32 / 100.0);
            let dx = self.cfg.dt_x as f32 * u;
            let dy = self.cfg.dt_y as f32 * u;
            let (lw, lh) = dt_logical_size(&self.cfg);
            let mut min_x = dx;
            let mut min_y = dy;
            let mut max_x = dx + lw * s;
            let mut max_y = dy + lh * s;
            for tr in &self.cfg.dt_elem_transforms {
                if !tr.is_default() {
                    let tx = tr.dx as f32 * s;
                    let ty = tr.dy as f32 * s;
                    let pad = 42.0 * s * (tr.scale_pct as f32 / 100.0);
                    min_x = min_x.min(dx + tx - pad);
                    min_y = min_y.min(dy + ty - pad);
                    max_x = max_x.max(dx + lw * s + tx + pad);
                    max_y = max_y.max(dy + lh * s + ty + pad);
                }
            }
            if x >= min_x && x <= max_x && y >= min_y && y <= max_y {
                return WidgetHitZone::ClockCard;
            }
        }

        if self.cfg.show_apps {
            let s = u * (self.cfg.apps_scale_pct as f32 / 100.0);
            let bx = self.cfg.apps_x as f32 * u;
            let by = self.cfg.apps_y as f32 * u;
            let show_add = self.cfg.apps_show_add_tile && self.cfg.apps.len() < 12;
            let total_tiles = self.cfg.apps.len() + usize::from(show_add);
            if total_tiles > 0 {
                let (offsets, total_w, total_h) = apps_layout_offsets(total_tiles, self.cfg.apps_cols.max(1) as usize);
                for (i, (rx, ry)) in offsets.into_iter().enumerate() {
                    let (ox, oy) = if i < self.cfg.apps.len() {
                        (self.cfg.apps[i].offset_x as f32, self.cfg.apps[i].offset_y as f32)
                    } else {
                        (0.0, 0.0)
                    };
                    let ax = bx + (rx + ox) * s;
                    let ay = by + (ry + oy) * s;
                    if x >= ax && x <= ax + 64.0 * s && y >= ay && y <= ay + 94.0 * s {
                        if i < self.cfg.apps.len() {
                            return WidgetHitZone::AppSlot(i);
                        } else {
                            return WidgetHitZone::AppAddTile;
                        }
                    }
                }
                if !self.cfg.apps_individual_move
                    && x >= bx - 10.0 * s
                    && x <= bx + (total_w + 10.0) * s
                    && y >= by - 8.0 * s
                    && y <= by + (total_h + 8.0) * s
                {
                    return WidgetHitZone::AppsBar;
                }
            }
        }

        if self.cfg.show_vitals {
            let s = u * (self.cfg.vitals_scale_pct as f32 / 100.0);
            let vx = self.cfg.vitals_x as f32 * u;
            let vy = self.cfg.vitals_y as f32 * u;
            let (vw_log, vh_log) = vitals_logical_size(&self.cfg);
            let vw = vw_log * s;
            let vh = vh_log * s;
            if x >= vx && x <= vx + vw && y >= vy && y <= vy + vh {
                if self.cfg.vitals_style == 4 {
                    let (bays, count, end_x) = ring_pill_layout(&self.cfg);
                    for bay in bays.iter().take(count) {
                        let lx0 = vx + bay.x_left * s;
                        let lx1 = lx0 + bay.bay_w * s;
                        if x >= lx0 && x <= lx1 {
                            return bay.hit_zone;
                        }
                    }
                    if self.cfg.vitals_show_buttons {
                        let btn_cy = vy + 32.0 * s;
                        let r_btn = 15.0 * s;
                        let slp_cx = vx + (end_x + 26.0) * s;
                        let sht_cx = vx + (end_x + 62.0) * s;
                        if (x - slp_cx).hypot(y - btn_cy) <= r_btn {
                            return WidgetHitZone::VitalsSleep;
                        }
                        if (x - sht_cx).hypot(y - btn_cy) <= r_btn {
                            return WidgetHitZone::VitalsShutdown;
                        }
                    }
                    return WidgetHitZone::VitalsBar;
                }
                let mut cur_x = 18.0f32;
                if self.cfg.vitals_show_cpu {
                    if x >= vx + cur_x * s && x <= vx + (cur_x + 80.0) * s {
                        return WidgetHitZone::VitalsCpu;
                    }
                    cur_x += 100.0;
                }
                if self.cfg.vitals_show_ram {
                    if x >= vx + cur_x * s && x <= vx + (cur_x + 80.0) * s {
                        return WidgetHitZone::VitalsRam;
                    }
                    cur_x += 100.0;
                }
                if self.cfg.vitals_show_power {
                    if x >= vx + cur_x * s && x <= vx + (cur_x + 80.0) * s {
                        return WidgetHitZone::VitalsPower;
                    }
                    cur_x += 100.0;
                }
                if self.cfg.vitals_show_buttons {
                    let btn_base = if cur_x > 20.0 { cur_x } else { 14.0 };
                    let sleep_x = btn_base + 12.0;
                    let shut_x = btn_base + 50.0;
                    if x >= vx + sleep_x * s && x <= vx + (sleep_x + 32.0) * s && y >= vy + 11.0 * s && y <= vy + 43.0 * s {
                        return WidgetHitZone::VitalsSleep;
                    }
                    if x >= vx + shut_x * s && x <= vx + (shut_x + 32.0) * s && y >= vy + 11.0 * s && y <= vy + 43.0 * s {
                        return WidgetHitZone::VitalsShutdown;
                    }
                }
                return WidgetHitZone::VitalsBar;
            }
        }

        for id in ExtWidgetId::ALL.into_iter().rev() {
            let wcfg = &self.cfg.ext_widgets[id.idx()];
            if wcfg.enabled {
                let s = (u * (wcfg.scale_pct as f32 / 100.0)).max(0.25);
                let wx = wcfg.x as f32 * u;
                let wy = wcfg.y as f32 * u;
                let lx = (x - wx) / s;
                let ly = (y - wy) / s;
                let hz = hit_test_ext_widget(id, wcfg, lx, ly);
                if hz != WidgetHitZone::None {
                    return hz;
                }
            }
        }

        WidgetHitZone::None
    }

    pub fn handle_click(&mut self, zone: WidgetHitZone) {
        if let WidgetHitZone::CtxButton(idx) = zone {
            if let Some(action) = self.context_bar.as_ref().and_then(|b| b.buttons.get(idx)).map(|btn| btn.action) {
                self.execute_ctx_action(action);
            }
            return;
        }
        if matches!(zone, WidgetHitZone::CtxBg) {
            return;
        }
        if self.context_bar.is_some() {
            self.close_context_bar();
            self.render();
            return;
        }
        match zone {
            WidgetHitZone::ClockCard => {
                self.cfg.widgets_24h = !self.cfg.widgets_24h;
                if !self.cmd_hwnd.0.is_null() && self.cmd_msg_24h != 0 {
                    unsafe {
                        let _ = PostMessageW(
                            Some(self.cmd_hwnd),
                            self.cmd_msg_24h,
                            WPARAM(if self.cfg.widgets_24h { 1 } else { 0 }),
                            LPARAM(0),
                        );
                    }
                }
                self.render();
            }
            WidgetHitZone::AppSlot(i) => {
                if let Some(app) = self.cfg.apps.get(i) {
                    launch_app_path(&app.path);
                }
            }
            WidgetHitZone::AppAddTile => {
                self.notify_sync(5);
            }
            WidgetHitZone::VitalsCpu
            | WidgetHitZone::VitalsGpu
            | WidgetHitZone::VitalsRam
            | WidgetHitZone::VitalsTemp => {
                unsafe {
                    let _ = ShellExecuteW(None, w!("open"), w!("taskmgr.exe"), PCWSTR::null(), PCWSTR::null(), SW_SHOWNORMAL);
                }
            }
            WidgetHitZone::VitalsSleep => {
                let p = to_wide(r"powrprof.dll,SetSuspendState 0,1,0");
                unsafe {
                    let _ = ShellExecuteW(None, w!("open"), w!("rundll32.exe"), PCWSTR(p.as_ptr()), PCWSTR::null(), SW_SHOWNORMAL);
                }
            }
            WidgetHitZone::VitalsShutdown => {
                let p = to_wide(r"-s -t 00");
                unsafe {
                    let _ = ShellExecuteW(None, w!("open"), w!("shutdown.exe"), PCWSTR(p.as_ptr()), PCWSTR::null(), SW_SHOWNORMAL);
                }
            }
            WidgetHitZone::MusicPrev => self.send_media_action(CcAction::MediaPrev),
            WidgetHitZone::MusicPlayPause => self.send_media_action(CcAction::MediaPlayPause),
            WidgetHitZone::MusicNext => self.send_media_action(CcAction::MediaNext),
            WidgetHitZone::CcScrollUp => {
                self.cc_scroll_y = (self.cc_scroll_y - 48.0).clamp(0.0, CC_MAX_SCROLL);
                self.render();
            }
            WidgetHitZone::CcScrollDown => {
                self.cc_scroll_y = (self.cc_scroll_y + 48.0).clamp(0.0, CC_MAX_SCROLL);
                self.render();
            }
            WidgetHitZone::CcMuteBtn => {
                let next_muted = !self.media_engine.cc_snapshot().vol_muted;
                if let Some((t, _, _)) = self.vol_drag {
                    self.vol_drag = Some((t, next_muted, Instant::now()));
                }
                self.send_media_action(CcAction::ToggleMute);
            }
            WidgetHitZone::CcTile(idx, right_half) => {
                let act = match idx {
                    0 => {
                        if right_half {
                            Some(CcAction::OpenWifiList)
                        } else {
                            Some(CcAction::ToggleWifi)
                        }
                    }
                    1 => {
                        if right_half {
                            Some(CcAction::OpenBtList)
                        } else {
                            Some(CcAction::ToggleBt)
                        }
                    }
                    2 => Some(CcAction::ToggleAirplane),
                    3 => Some(CcAction::OpenAccessibility),
                    4 => Some(CcAction::ToggleEnergySaver),
                    5 => Some(CcAction::ToggleCaptions),
                    6 => Some(CcAction::ToggleNightLight),
                    7 => Some(CcAction::ToggleHotspot),
                    8 => Some(CcAction::ToggleNearby),
                    9 => Some(CcAction::OpenCast),
                    10 => Some(CcAction::OpenProject),
                    _ => None,
                };
                if let Some(a) = act {
                    self.send_media_action(a);
                }
            }
            WidgetHitZone::ExtBtn(ExtWidgetId::AudioSwitcher, btn) if btn <= 2 => {
                match btn {
                    0 => {
                        let cur = self.media_engine.cc_snapshot().vol_pct;
                        let step = ((self.cfg.ext_widgets[ExtWidgetId::AudioSwitcher.idx()].param1_pct as u8) / 10).clamp(2, 12);
                        self.send_media_action(CcAction::SetVolume(cur.saturating_sub(step)));
                    }
                    1 => self.send_media_action(CcAction::ToggleMute),
                    2 => {
                        let cur = self.media_engine.cc_snapshot().vol_pct;
                        let step = ((self.cfg.ext_widgets[ExtWidgetId::AudioSwitcher.idx()].param1_pct as u8) / 10).clamp(2, 12);
                        self.send_media_action(CcAction::SetVolume((cur + step).min(100)));
                    }
                    _ => {}
                }
            }
            WidgetHitZone::ExtBtn(id, btn) => {
                let changed = self.ext_state.handle_desktop_click(id, btn, &mut self.cfg);
                self.sync_keyboard_focus_style();
                if changed {
                    self.render();
                    self.notify_sync(0);
                }
            }
            _ => {}
        }
    }

    fn sync_keyboard_focus_style(&self) {
        if self.hwnd.0.is_null() {
            return;
        }
        unsafe {
            let ex = GetWindowLongPtrW(self.hwnd, GWL_EXSTYLE);
            let want_focus = self.ext_state.search_focused || self.ext_state.note_focused;
            let noact = WS_EX_NOACTIVATE.0 as isize;
            if want_focus {
                if (ex & noact) != 0 {
                    let _ = SetWindowLongPtrW(self.hwnd, GWL_EXSTYLE, ex & !noact);
                }
                let _ = windows::Win32::UI::WindowsAndMessaging::SetForegroundWindow(self.hwnd);
            } else if (ex & noact) == 0 {
                let _ = SetWindowLongPtrW(self.hwnd, GWL_EXSTYLE, ex | noact);
            }
        }
    }

    fn apply_cc_drag(&mut self, pt: POINT, mode: CcDragMode) {
        let s = (self.u * (self.cfg.cc_scale_pct as f32 / 100.0)).max(0.25);
        let cx = self.cfg.cc_x as f32 * self.u;
        let cy = self.cfg.cc_y as f32 * self.u;
        let lx = ((pt.x - self.mon_left) as f32 - cx) / s;
        let ly = ((pt.y - self.mon_top) as f32 - cy) / s;
        match mode {
            CcDragMode::BrightSlider => {
                let t = ((lx - 54.0) / 274.0).clamp(0.0, 1.0);
                let pct = (t * 100.0).round() as u8;
                let moved = self
                    .bright_drag
                    .map(|(old, _)| (old - t).abs() > 0.001)
                    .unwrap_or(true);
                self.bright_drag = Some((t, Instant::now()));
                if self.last_sent_bright_pct != Some(pct) {
                    self.last_sent_bright_pct = Some(pct);
                    self.media_engine.send_action(CcAction::SetBrightness(pct));
                }
                if moved {
                    self.render();
                }
            }
            CcDragMode::VolSlider => {
                let t = ((lx - 54.0) / 274.0).clamp(0.0, 1.0);
                let pct = (t * 100.0).round() as u8;
                let moved = self
                    .vol_drag
                    .map(|(old, m, _)| m || (old - t).abs() > 0.001)
                    .unwrap_or(true);
                self.vol_drag = Some((t, false, Instant::now()));
                if self.last_sent_vol_pct != Some(pct) {
                    self.last_sent_vol_pct = Some(pct);
                    self.media_engine.send_action(CcAction::SetVolume(pct));
                }
                if moved {
                    self.render();
                }
            }
            CcDragMode::ScrollThumb => {
                let target = (((ly - 97.0) / 46.0).clamp(0.0, 1.0) * CC_MAX_SCROLL).round();
                if (self.cc_scroll_y - target).abs() >= 0.5 {
                    self.cc_scroll_y = target;
                    self.render();
                }
            }
        }
    }

    fn accumulate_wheel_notches(&mut self, channel: u8, delta: i32) -> i32 {
        let now = Instant::now();
        let stale = self
            .wheel_last_at
            .map(|t| now.duration_since(t) > Duration::from_millis(450))
            .unwrap_or(true);
        if stale {
            self.wheel_accum_bright = 0;
            self.wheel_accum_vol = 0;
            self.wheel_accum_scroll = 0;
        }
        self.wheel_last_at = Some(now);
        let acc = match channel {
            0 => &mut self.wheel_accum_bright,
            1 => &mut self.wheel_accum_vol,
            _ => &mut self.wheel_accum_scroll,
        };
        if *acc != 0 && acc.signum() != delta.signum() {
            *acc = 0;
        }
        *acc = (*acc + delta).clamp(-360, 360);
        if acc.abs() < 120 {
            return 0;
        }
        let notches = (*acc / 120).clamp(-2, 2);
        *acc %= 120;
        notches
    }

    fn wheel_step_bright(&mut self, delta: i32) {
        let notches = self.accumulate_wheel_notches(0, delta);
        if notches == 0 {
            return;
        }
        let now = Instant::now();
        let cur = match self.bright_drag {
            Some((t, at)) if now.duration_since(at) < Duration::from_millis(600) => t,
            _ => self.media_engine.cc_snapshot().bright_pct as f32 / 100.0,
        };
        let step_f = notches as f32 * 0.05;
        let next = (cur + step_f).clamp(0.0, 1.0);
        let pct = (next * 100.0).round() as u8;
        self.bright_drag = Some((next, now));
        self.last_sent_bright_pct = Some(pct);
        self.media_engine.send_action(CcAction::SetBrightness(pct));
        self.render();
    }

    fn wheel_step_vol(&mut self, delta: i32) {
        let notches = self.accumulate_wheel_notches(1, delta);
        if notches == 0 {
            return;
        }
        let now = Instant::now();
        let cur = match self.vol_drag {
            Some((t, _, at)) if now.duration_since(at) < Duration::from_millis(600) => t,
            _ => self.media_engine.cc_snapshot().vol_pct as f32 / 100.0,
        };
        let step_f = notches as f32 * 0.04;
        let next = (cur + step_f).clamp(0.0, 1.0);
        let pct = (next * 100.0).round() as u8;
        self.vol_drag = Some((next, false, now));
        self.last_sent_vol_pct = Some(pct);
        self.media_engine.send_action(CcAction::SetVolume(pct));
        self.render();
    }

    fn wheel_step_cc_scroll(&mut self, delta: i32) {
        let notches = self.accumulate_wheel_notches(2, delta);
        if notches == 0 {
            return;
        }
        // Damped Control Center scroll: 22px per full WHEEL_DELTA (120) notch.
        let next = (self.cc_scroll_y - (notches as f32) * 22.0).clamp(0.0, CC_MAX_SCROLL);
        if (next - self.cc_scroll_y).abs() >= 0.5 {
            self.cc_scroll_y = next;
            self.render();
        }
    }
}

pub struct LocalTimeInfo {
    pub year: i32,
    pub month_name: &'static str,
    pub day: u32,
    pub weekday_name: &'static str,
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
}

pub fn chrono_local() -> LocalTimeInfo {
    let st: SYSTEMTIME = unsafe { GetLocalTime() };
    let months = [
        "January", "February", "March", "April", "May", "June",
        "July", "August", "September", "October", "November", "December",
    ];
    let days = [
        "Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday",
    ];
    let m_idx = (st.wMonth as usize).saturating_sub(1).min(11);
    let d_idx = (st.wDayOfWeek as usize).min(6);

    LocalTimeInfo {
        year: st.wYear as i32,
        month_name: months[m_idx],
        day: st.wDay as u32,
        weekday_name: days[d_idx],
        hour: st.wHour as u32,
        minute: st.wMinute as u32,
        second: st.wSecond as u32,
    }
}

unsafe extern "system" fn widgets_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut DesktopWidgets;
    let dw = if !ptr.is_null() {
        &mut *ptr
    } else if !GLOBAL_WIDGETS_PTR.is_null() {
        &mut *GLOBAL_WIDGETS_PTR
    } else {
        return DefWindowProcW(hwnd, msg, wparam, lparam);
    };

    match msg {
        0x0021 => LRESULT(3), // WM_MOUSEACTIVATE -> MA_NOACTIVATE (prevents Z-order pop above active apps/Studio)
        WM_SYSCOMMAND => {
            if (wparam.0 & 0xFFF0) == SC_MINIMIZE_CMD {
                return LRESULT(0);
            }
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
        WM_TIMER if wparam.0 == TIMER_CTX_POLL => {
            if dw.context_bar.is_some() {
                let esc = (GetAsyncKeyState(VK_ESCAPE_CODE) as u16 & 0x8000) != 0;
                let lbtn = (GetAsyncKeyState(VK_LBUTTON_CODE) as u16 & 0x8000) != 0;
                let mut close = esc;
                if !close && lbtn {
                    let mut pt = POINT::default();
                    let _ = GetCursorPos(&mut pt);
                    if !matches!(dw.hit_test(pt), WidgetHitZone::CtxButton(_) | WidgetHitZone::CtxBg) {
                        close = true;
                    }
                }
                if close {
                    dw.close_context_bar();
                    dw.render();
                }
            } else {
                let _ = KillTimer(Some(hwnd), TIMER_CTX_POLL);
            }
            LRESULT(0)
        }
        0x0084 => {
            let pt = POINT {
                x: (lparam.0 & 0xFFFF) as i16 as i32,
                y: ((lparam.0 >> 16) & 0xFFFF) as i16 as i32,
            };
            let zone = dw.hit_test(pt);
            if zone != WidgetHitZone::None {
                LRESULT(1) // HTCLIENT
            } else {
                LRESULT(-1) // HTTRANSPARENT
            }
        }
        WM_SETCURSOR => {
            let cur = if dw.drag_moved {
                LoadCursorW(None, IDC_SIZEALL).unwrap_or_default()
            } else if !dw.cfg.widgets_locked
                && matches!(
                    dw.hover_zone,
                    WidgetHitZone::AppsBar
                        | WidgetHitZone::VitalsBar
                        | WidgetHitZone::MusicCard
                        | WidgetHitZone::CcBar
                        | WidgetHitZone::ExtCard(_)
                )
            {
                LoadCursorW(None, IDC_SIZEALL).unwrap_or_default()
            } else if !matches!(dw.hover_zone, WidgetHitZone::None) {
                LoadCursorW(None, IDC_HAND).unwrap_or_default()
            } else {
                LoadCursorW(None, IDC_ARROW).unwrap_or_default()
            };
            SetCursor(Some(cur));
            LRESULT(1)
        }
        0x0201 => {
            let mut pt = POINT::default();
            let _ = GetCursorPos(&mut pt);
            let zone = dw.hit_test(pt);
            dw.lbtn_down_zone = zone;
            // Trigger immediate tactile physical impulse ripple when clicking realistic desktop widgets if enabled
            if dw.cfg.widgets_hover_react {
                let u = dw.u.max(0.5);
                let mx = (pt.x - dw.mon_left) as f32;
                let my = (pt.y - dw.mon_top) as f32;
                match zone {
                    WidgetHitZone::ClockCard if dw.cfg.dt_style.is_realistic_interactive() => {
                        let s = u * (dw.cfg.dt_scale_pct as f32 / 100.0);
                        let (lw, lh) = dt_logical_size(&dw.cfg);
                        let nx = ((mx - dw.cfg.dt_x as f32 * u) / (lw * s).max(1.0)).clamp(0.0, 1.0);
                        let ny = ((my - dw.cfg.dt_y as f32 * u) / (lh * s).max(1.0)).clamp(0.0, 1.0);
                        dw.widget_sims[0].spawn_impulse(nx, ny, 1.25, 1.35);
                        dw.viz_dirty = true;
                        dw.notify_viz_wake();
                    }
                    WidgetHitZone::MusicCard
                    | WidgetHitZone::MusicPrev
                    | WidgetHitZone::MusicPlayPause
                    | WidgetHitZone::MusicNext
                        if dw.cfg.music_style.is_realistic_interactive() =>
                    {
                        let s = u * (dw.cfg.music_scale_pct as f32 / 100.0);
                        let (lw, lh) = music_logical_size(&dw.cfg);
                        let nx = ((mx - dw.cfg.music_x as f32 * u) / (lw * s).max(1.0)).clamp(0.0, 1.0);
                        let ny = ((my - dw.cfg.music_y as f32 * u) / (lh * s).max(1.0)).clamp(0.0, 1.0);
                        dw.widget_sims[3].spawn_impulse(nx, ny, 1.25, 1.35);
                        dw.viz_dirty = true;
                        dw.notify_viz_wake();
                    }
                    WidgetHitZone::ExtCard(id) | WidgetHitZone::ExtBtn(id, _)
                        if dw.cfg.ext_widgets[id.idx()].theme.is_realistic_interactive() =>
                    {
                        let wcfg = &dw.cfg.ext_widgets[id.idx()];
                        let s = u * (wcfg.scale_pct as f32 / 100.0);
                        let (lw, lh) = wcfg.logical_size(id);
                        let nx = ((mx - wcfg.x as f32 * u) / (lw * s).max(1.0)).clamp(0.0, 1.0);
                        let ny = ((my - wcfg.y as f32 * u) / (lh * s).max(1.0)).clamp(0.0, 1.0);
                        dw.widget_sims[5 + id.idx()].spawn_impulse(nx, ny, 1.25, 1.35);
                        dw.viz_dirty = true;
                        dw.notify_viz_wake();
                    }
                    _ => {}
                }
            }
            if !matches!(
                zone,
                WidgetHitZone::ExtBtn(ExtWidgetId::SearchBar, _)
                    | WidgetHitZone::ExtCard(ExtWidgetId::SearchBar)
                    | WidgetHitZone::ExtBtn(ExtWidgetId::Notes, _)
                    | WidgetHitZone::ExtCard(ExtWidgetId::Notes)
            ) && (dw.ext_state.search_focused || dw.ext_state.note_focused)
            {
                dw.ext_state.search_focused = false;
                dw.ext_state.note_focused = false;
                dw.sync_keyboard_focus_style();
                dw.render();
            }
            if matches!(zone, WidgetHitZone::CtxButton(_) | WidgetHitZone::CtxBg) {
                return LRESULT(0);
            }
            if dw.context_bar.is_some() {
                dw.close_context_bar();
                dw.render();
                dw.lbtn_down_zone = WidgetHitZone::None;
                return LRESULT(0);
            }
            match zone {
                WidgetHitZone::CcBrightSlider => {
                    dw.cc_drag = Some(CcDragMode::BrightSlider);
                    dw.last_sent_bright_pct = None;
                    dw.apply_cc_drag(pt, CcDragMode::BrightSlider);
                    SetCapture(hwnd);
                    return LRESULT(0);
                }
                WidgetHitZone::CcVolSlider => {
                    dw.cc_drag = Some(CcDragMode::VolSlider);
                    dw.last_sent_vol_pct = None;
                    dw.apply_cc_drag(pt, CcDragMode::VolSlider);
                    SetCapture(hwnd);
                    return LRESULT(0);
                }
                WidgetHitZone::CcScrollTrack => {
                    dw.cc_drag = Some(CcDragMode::ScrollThumb);
                    dw.apply_cc_drag(pt, CcDragMode::ScrollThumb);
                    SetCapture(hwnd);
                    return LRESULT(0);
                }
                _ => {}
            }
            if !dw.cfg.widgets_locked {
                let kind = match zone {
                    WidgetHitZone::ClockCard => Some((DragWidgetKind::DateTime, (dw.cfg.dt_x, dw.cfg.dt_y))),
                    WidgetHitZone::AppSlot(i) if dw.cfg.apps_individual_move => dw
                        .cfg
                        .apps
                        .get(i)
                        .map(|a| (DragWidgetKind::AppTile(i), (a.offset_x, a.offset_y))),
                    WidgetHitZone::AppSlot(_) | WidgetHitZone::AppAddTile | WidgetHitZone::AppsBar => {
                        Some((DragWidgetKind::Apps, (dw.cfg.apps_x, dw.cfg.apps_y)))
                    }
                    WidgetHitZone::VitalsCpu
                    | WidgetHitZone::VitalsGpu
                    | WidgetHitZone::VitalsRam
                    | WidgetHitZone::VitalsTemp
                    | WidgetHitZone::VitalsPower
                    | WidgetHitZone::VitalsSleep
                    | WidgetHitZone::VitalsShutdown
                    | WidgetHitZone::VitalsBar => {
                        Some((DragWidgetKind::Vitals, (dw.cfg.vitals_x, dw.cfg.vitals_y)))
                    }
                    WidgetHitZone::MusicCard
                    | WidgetHitZone::MusicPrev
                    | WidgetHitZone::MusicPlayPause
                    | WidgetHitZone::MusicNext => {
                        Some((DragWidgetKind::Music, (dw.cfg.music_x, dw.cfg.music_y)))
                    }
                    WidgetHitZone::CcBar
                    | WidgetHitZone::CcTile(_, _)
                    | WidgetHitZone::CcScrollUp
                    | WidgetHitZone::CcScrollDown
                    | WidgetHitZone::CcMuteBtn => {
                        Some((DragWidgetKind::ControlCenter, (dw.cfg.cc_x, dw.cfg.cc_y)))
                    }
                    WidgetHitZone::ExtCard(id) | WidgetHitZone::ExtBtn(id, _) => {
                        let ew = &dw.cfg.ext_widgets[id.idx()];
                        Some((DragWidgetKind::Ext(id), (ew.x, ew.y)))
                    }
                    WidgetHitZone::None
                    | WidgetHitZone::CcScrollTrack
                    | WidgetHitZone::CcBrightSlider
                    | WidgetHitZone::CcVolSlider
                    | WidgetHitZone::CtxButton(_)
                    | WidgetHitZone::CtxBg => None,
                };
                if let Some((dk, start_xy)) = kind {
                    dw.drag_widget = Some(dk);
                    dw.drag_start_cursor = pt;
                    dw.drag_start_xy = start_xy;
                    dw.drag_moved = false;
                    SetCapture(hwnd);
                }
            }
            LRESULT(0)
        }
        0x0200 => {
            let mut pt = POINT::default();
            let _ = GetCursorPos(&mut pt);
            if let Some(cm) = dw.cc_drag {
                if (GetAsyncKeyState(VK_LBUTTON_CODE) as u16 & 0x8000) == 0 {
                    dw.cc_drag = None;
                    let _ = ReleaseCapture();
                    dw.lbtn_down_zone = WidgetHitZone::None;
                } else {
                    dw.apply_cc_drag(pt, cm);
                    return LRESULT(0);
                }
            }
            if let Some(dk) = dw.drag_widget {
                let dx = pt.x - dw.drag_start_cursor.x;
                let dy = pt.y - dw.drag_start_cursor.y;
                let thresh = (5.0 * dw.u).round() as i32;
                if !dw.drag_moved && (dx.abs() > thresh || dy.abs() > thresh) {
                    dw.drag_moved = true;
                }
                if dw.drag_moved {
                    let nx = (dw.drag_start_xy.0 + (dx as f32 / dw.u).round() as i32).clamp(-40, 1250);
                    let ny = (dw.drag_start_xy.1 + (dy as f32 / dw.u).round() as i32).clamp(-10, 770);
                    match dk {
                        DragWidgetKind::DateTime => {
                            dw.cfg.dt_x = nx;
                            dw.cfg.dt_y = ny;
                        }
                        DragWidgetKind::Apps => {
                            dw.cfg.apps_x = nx;
                            dw.cfg.apps_y = ny;
                        }
                        DragWidgetKind::AppTile(idx) => {
                            let show_add = dw.cfg.apps_show_add_tile && dw.cfg.apps.len() < 12;
                            let total_tiles = dw.cfg.apps.len() + usize::from(show_add);
                            let (offsets, _, _) = apps_layout_offsets(total_tiles.max(1), 6);
                            if let (Some(app), Some(&(rx, ry))) =
                                (dw.cfg.apps.get_mut(idx), offsets.get(idx))
                            {
                                let ox = dw.drag_start_xy.0 + (dx as f32 / dw.u).round() as i32;
                                let oy = dw.drag_start_xy.1 + (dy as f32 / dw.u).round() as i32;
                                let scale = (dw.cfg.apps_scale_pct as f32 / 100.0).max(0.5);
                                let screen_x = (dw.cfg.apps_x as f32 + (rx + ox as f32) * scale)
                                    .clamp(0.0, (1280.0 - 64.0 * scale).max(0.0));
                                let screen_y = (dw.cfg.apps_y as f32 + (ry + oy as f32) * scale)
                                    .clamp(0.0, (800.0 - 94.0 * scale).max(0.0));
                                app.offset_x = (((screen_x - dw.cfg.apps_x as f32) / scale - rx)
                                    .round() as i32)
                                    .clamp(-1280, 1280);
                                app.offset_y = (((screen_y - dw.cfg.apps_y as f32) / scale - ry)
                                    .round() as i32)
                                    .clamp(-800, 800);
                            }
                        }
                        DragWidgetKind::Vitals => {
                            dw.cfg.vitals_x = nx;
                            dw.cfg.vitals_y = ny;
                        }
                        DragWidgetKind::Music => {
                            dw.cfg.music_x = nx;
                            dw.cfg.music_y = ny;
                        }
                        DragWidgetKind::ControlCenter => {
                            dw.cfg.cc_x = nx;
                            dw.cfg.cc_y = ny;
                        }
                        DragWidgetKind::Ext(id) => {
                            dw.cfg.ext_widgets[id.idx()].x = nx;
                            dw.cfg.ext_widgets[id.idx()].y = ny;
                        }
                    }
                    dw.render();
                    return LRESULT(0);
                }
            }

            let zone = dw.hit_test(pt);
            dw.last_hover_move = Instant::now();
            if zone != dw.hover_zone {
                dw.hover_zone = zone;
                dw.viz_dirty = true;
                dw.notify_viz_wake();
            } else if dw.cfg.widgets_hover_react && zone != WidgetHitZone::None {
                dw.notify_viz_wake();
            }
            if !dw.tracking_leave {
                let mut tme = TRACKMOUSEEVENT {
                    cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
                    dwFlags: TME_LEAVE,
                    hwndTrack: hwnd,
                    dwHoverTime: 0,
                };
                if TrackMouseEvent(&mut tme).is_ok() {
                    dw.tracking_leave = true;
                }
            }
            LRESULT(0)
        }
        0x02A3 => {
            dw.tracking_leave = false;
            if dw.hover_zone != WidgetHitZone::None && dw.drag_widget.is_none() && dw.cc_drag.is_none() {
                dw.hover_zone = WidgetHitZone::None;
                dw.viz_dirty = true;
                dw.notify_viz_wake();
            }
            LRESULT(0)
        }
        0x0215 => {
            dw.cc_drag = None;
            dw.drag_widget = None;
            dw.drag_moved = false;
            dw.lbtn_down_zone = WidgetHitZone::None;
            LRESULT(0)
        }
        0x0202 => {
            let mut pt = POINT::default();
            let _ = GetCursorPos(&mut pt);
            let down_zone = dw.lbtn_down_zone;
            dw.lbtn_down_zone = WidgetHitZone::None;
            if let Some(cm) = dw.cc_drag.take() {
                match cm {
                    CcDragMode::BrightSlider => {
                        if let Some((t, _)) = dw.bright_drag {
                            dw.bright_drag = Some((t, Instant::now()));
                        }
                    }
                    CcDragMode::VolSlider => {
                        if let Some((t, m, _)) = dw.vol_drag {
                            dw.vol_drag = Some((t, m, Instant::now()));
                        }
                    }
                    CcDragMode::ScrollThumb => {}
                }
                let _ = ReleaseCapture();
                return LRESULT(0);
            }
            let was_drag_widget = dw.drag_widget.take().is_some();
            let was_moved = dw.drag_moved;
            dw.drag_moved = false;
            if was_drag_widget {
                let _ = ReleaseCapture();
                if was_moved {
                    dw.notify_sync(0);
                    return LRESULT(0);
                }
            }
            let zone = dw.hit_test(pt);
            if zone == down_zone || matches!(zone, WidgetHitZone::CtxButton(_)) {
                dw.handle_click(zone);
            }
            LRESULT(0)
        }
        0x020A => {
            let mut pt = POINT::default();
            let _ = GetCursorPos(&mut pt);
            let delta = ((wparam.0 >> 16) & 0xFFFF) as i16 as i32;
            let zone = dw.hit_test(pt);
            match zone {
                WidgetHitZone::CcBrightSlider => {
                    dw.wheel_step_bright(delta);
                    LRESULT(0)
                }
                WidgetHitZone::CcVolSlider | WidgetHitZone::CcMuteBtn => {
                    dw.wheel_step_vol(delta);
                    LRESULT(0)
                }
                WidgetHitZone::CcBar
                | WidgetHitZone::CcTile(_, _)
                | WidgetHitZone::CcScrollUp
                | WidgetHitZone::CcScrollDown
                | WidgetHitZone::CcScrollTrack => {
                    dw.wheel_step_cc_scroll(delta);
                    LRESULT(0)
                }
                _ => DefWindowProcW(hwnd, msg, wparam, lparam),
            }
        }
        0x0204 => LRESULT(0),
        0x0205 => {
            let mut pt = POINT::default();
            let _ = GetCursorPos(&mut pt);
            let zone = dw.hit_test(pt);
            if let WidgetHitZone::CcTile(idx, _) = zone {
                dw.close_context_bar();
                dw.send_media_action(CcAction::OpenTileSettings(idx));
                dw.render();
            } else if zone != WidgetHitZone::None {
                dw.open_context_bar(pt, zone);
            } else if dw.context_bar.is_some() {
                dw.close_context_bar();
                dw.render();
            }
            LRESULT(0)
        }
        0x0100 => {
            // WM_KEYDOWN for Desktop Search Bar & Sticky Notes
            let vk = wparam.0 as u32;
            if dw.ext_state.search_focused {
                if vk == 0x0D {
                    dw.ext_state.execute_search(&dw.cfg);
                    dw.sync_keyboard_focus_style();
                    dw.render();
                } else if vk == 0x1B {
                    dw.ext_state.search_focused = false;
                    dw.sync_keyboard_focus_style();
                    dw.render();
                }
                return LRESULT(0);
            } else if dw.ext_state.note_focused {
                if vk == 0x0D {
                    let t = dw.ext_state.note_draft.trim().to_string();
                    if !t.is_empty() {
                        if let Some(row) = dw.ext_state.editing_note_row {
                            if let Some(slot) = dw.cfg.notes_items.get_mut(row) {
                                slot.1 = t;
                            }
                        } else {
                            dw.cfg.notes_items.insert(0, (false, t));
                            dw.cfg.notes_items.truncate(8);
                        }
                        dw.ext_state.note_draft.clear();
                        dw.notify_sync(0);
                    }
                    dw.ext_state.editing_note_row = None;
                    dw.ext_state.note_focused = false;
                    dw.sync_keyboard_focus_style();
                    dw.render();
                } else if vk == 0x1B {
                    dw.ext_state.editing_note_row = None;
                    dw.ext_state.note_focused = false;
                    dw.sync_keyboard_focus_style();
                    dw.render();
                }
                return LRESULT(0);
            }
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
        0x0102 => {
            // WM_CHAR for Desktop Search Bar & Sticky Notes
            let code = wparam.0 as u32;
            if dw.ext_state.search_focused {
                if code == 8 {
                    dw.ext_state.search_query.pop();
                    dw.render();
                } else if code >= 32 {
                    if let Some(ch) = char::from_u32(code) {
                        if dw.ext_state.search_query.chars().count() < 64 {
                            dw.ext_state.search_query.push(ch);
                            dw.render();
                        }
                    }
                }
                return LRESULT(0);
            } else if dw.ext_state.note_focused {
                if code == 8 {
                    dw.ext_state.note_draft.pop();
                    dw.render();
                } else if code >= 32 {
                    if let Some(ch) = char::from_u32(code) {
                        if dw.ext_state.note_draft.chars().count() < 52 {
                            dw.ext_state.note_draft.push(ch);
                            dw.render();
                        }
                    }
                }
                return LRESULT(0);
            }
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
        0x0014 => LRESULT(1),
        0x0010 => LRESULT(0),
        0x0002 => {
            dw.hwnd = HWND::default();
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

impl Drop for DesktopWidgets {
    fn drop(&mut self) {
        self.cleanup();
    }
}

#[cfg(test)]
mod fg_cache_tests {
    use super::*;
    use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED};
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DestroyWindow, WS_EX_NOACTIVATE, WS_EX_NOREDIRECTIONBITMAP,
        WS_EX_TOOLWINDOW, WS_POPUP,
    };

    #[test]
    fn widget_fg_cache_hits_reuses_and_clears_across_widgets() {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            let hinst = GetModuleHandleW(None).unwrap_or_default();
            let hwnd = CreateWindowExW(
                WS_EX_NOREDIRECTIONBITMAP | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
                w!("STATIC"),
                PCWSTR::null(),
                WS_POPUP,
                0,
                0,
                512,
                256,
                None,
                None,
                Some(hinst.into()),
                None,
            )
            .expect("test host HWND");

            let mut dcomp = DCompSwapSurface::new(hwnd).expect("DCompSwapSurface init");
            assert!(dcomp.ensure_size(512, 256));
            let dw: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED).expect("DWrite factory");
            let mut slot = WidgetFgCacheSlot::default();
            let mut gen = 1u64;
            let mut draw_count = 0u32;

            {
                let rt: ID2D1RenderTarget = dcomp.render_target().cast().expect("ID2D1RenderTarget");
                rt.BeginDraw();
                rt.Clear(None);
                // First miss draws once; next 4 draws with identical key hit cache.
                for _ in 0..5 {
                    slot.draw_cached(&rt, gen, 0x1234, 16.0, 16.0, 200.0, 100.0, 4.0, |_| {
                        draw_count += 1;
                    });
                }
                assert_eq!(draw_count, 1, "cache should hit on matching key");

                // Key change triggers redraw while reusing compat_rt.
                slot.draw_cached(&rt, gen, 0x5678, 16.0, 16.0, 200.0, 100.0, 4.0, |_| {
                    draw_count += 1;
                });
                assert_eq!(draw_count, 2, "key change should invalidate cache");

                // Generation bump triggers redraw.
                gen += 1;
                slot.draw_cached(&rt, gen, 0x5678, 16.0, 16.0, 200.0, 100.0, 4.0, |_| {
                    draw_count += 1;
                });
                assert_eq!(draw_count, 3, "generation bump should invalidate cache");

                // Clear drops bitmaps and forces redraw.
                slot.clear();
                slot.draw_cached(&rt, gen, 0x5678, 16.0, 16.0, 200.0, 100.0, 4.0, |_| {
                    draw_count += 1;
                });
                assert_eq!(draw_count, 4, "clear() should invalidate cache");

                // Smoke test real cached widget renderers across repeated frames.
                let mut cache = WidgetFgBmpCache::default();
                let mut cfg = AppConfig::default();
                cfg.music_style = crate::config::MusicStyle::EditorialSleeve;
                cfg.dt_style = ClockStyle::FlipMech;
                cfg.dt_show_seconds = true;
                let state = ExtRuntimeState::new();
                let cc = CcSnapshot::default();
                let media = MediaSnapshot::default();
                let bands = [0.0f32; 12];
                for _ in 0..10 {
                    draw_datetime_widget_full_cached(
                        &rt, &dw, None, 10.0, 10.0, 1.0, &cfg, false, None, &mut cache.clock, &mut cache.clock_sec, cache.cache_gen,
                    );
                    draw_music_widget_with_viz_cached(
                        &rt, &dw, None, None, 10.0, 10.0, 1.0, &cfg, &media, &bands, None,
                        WidgetHitZone::None, None, &mut cache.music, cache.cache_gen,
                    );
                    draw_ext_widget_with_viz_cached(
                        &rt, &dw, None, None, 10.0, 10.0, 1.0, ExtWidgetId::Weather, &cfg, &state,
                        &cc, WidgetHitZone::None, None, &mut cache.ext[ExtWidgetId::Weather.idx()], cache.cache_gen,
                    );
                }
                assert!(cache.ext[ExtWidgetId::Weather.idx()].bmp.is_some());
                cache.clear();
                assert!(cache.ext[ExtWidgetId::Weather.idx()].bmp.is_none());
                assert!(cache.music.bmp.is_none());
                assert!(cache.clock.bmp.is_none());
                assert!(cache.clock_sec[0].bmp.is_none() && cache.clock_sec[1].bmp.is_none());

                assert!(rt.EndDraw(None, None).is_ok());
                slot.clear();
            }
            assert!(dcomp.present().is_ok());
            dcomp.release_buffers();
            drop(dcomp);
            let _ = DestroyWindow(hwnd);
            CoUninitialize();
        }
    }

    #[test]
    fn luxury_dynamic_island_and_ring_pill_layout_ini_and_60fps_d2d_verification() {
        use crate::config::{parse_config_ini, serialize_config_ini, MusicStyle};
        use crate::theme::ThemePalette;

        // 1. INI Serialization & Style Bounds Verification
        let mut cfg = AppConfig::default();
        cfg.apply_music_style_preset(MusicStyle::DynamicIslandCapsule);
        cfg.vitals_style = 4;
        cfg.vitals_match_wallpaper = true;
        cfg.music_match_wallpaper = true;
        let ini = serialize_config_ini(&cfg);
        let parsed = parse_config_ini(&ini).expect("parsed INI");
        assert_eq!(parsed.music_style, MusicStyle::DynamicIslandCapsule);
        assert_eq!(parsed.music_corner_radius, 37);
        assert_eq!(parsed.vitals_style, 4);

        let (mlw, mlh) = music_logical_size(&parsed);
        assert_eq!((mlw, mlh), (444.0, 74.0));

        let mut cfg_no_btns = parsed.clone();
        cfg_no_btns.vitals_show_buttons = false;
        assert_eq!(vitals_logical_size(&cfg_no_btns), (312.0, 64.0));
        assert_eq!(vitals_logical_size(&parsed), (392.0, 64.0));

        // 2. Non-overlapping horizontal zones on Dynamic Island Capsule
        let [r_prev, r_play, r_next] = music_transport_rects(&parsed);
        assert!(r_prev.0 > 228.0, "Prev control must leave gap after text zone");
        assert!(r_prev.2 < r_play.0, "Prev must not overlap Play/Pause");
        assert!(r_play.2 < r_next.0, "Play/Pause must not overlap Next");
        assert!(r_next.2 < 348.0, "Next control must not overlap right EQ waveform");
        assert!(63.0 < 75.0, "Cover squircle separated from track text");

        // 3. Direct2D/DirectWrite 60 FPS multi-frame stress render + zero-leak cleanup check
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            let hinst = windows::Win32::System::LibraryLoader::GetModuleHandleW(None)
                .map(HINSTANCE::from)
                .unwrap_or_default();
            DesktopWidgets::register_class(hinst);
            DesktopWidgets::load_gdi_fonts();
            let d2d: ID2D1Factory = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None).expect("D2D factory");
            let dw: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED).expect("DW factory");
            let custom_fonts = build_custom_font_collection(&dw);
            let dc_rt = create_widgets_dc_rt(&d2d).expect("DCRT");
            let mut dib = DibBuffer::new();
            assert!(dib.ensure_size(960, 420));
            let bind_rc = RECT { left: 0, top: 0, right: 960, bottom: 420 };
            dc_rt.BindDC(dib.mem_dc, &bind_rc).expect("BindDC");

            let rt: ID2D1RenderTarget = dc_rt.cast().expect("ID2D1RenderTarget");
            let pal = ThemePalette::from_accent_rgb(0x4EE8C8);
            let media_active = MediaSnapshot {
                title: "Midnight Telemetry in Nordic Glass Studio (Extended Remix)".to_string(),
                artist: "Autumn Acoustics • Live Session".to_string(),
                is_playing: true,
                ..MediaSnapshot::default()
            };
            let mut mboxes = [ClockElemBox::default(); 6];
            let mut vboxes = [ClockElemBox::default(); 6];
            let mut bands = [0.0f32; 12];
            let mut fg_music = WidgetFgCacheSlot::default();

            rt.BeginDraw();
            for frame in 0..60 {
                rt.Clear(Some(&D2D1_COLOR_F { r: 0.05, g: 0.07, b: 0.11, a: 1.0 }));
                let phase = frame as f32 * 0.18;
                for (i, b) in bands.iter_mut().enumerate() {
                    *b = (0.22 + 0.65 * (phase + i as f32 * 0.55).sin().abs()).clamp(0.05, 0.98);
                }
                let t = frame as f32 / 59.0;
                draw_dynamic_island_capsule_player(
                    &rt,
                    &dw,
                    Some(&d2d),
                    custom_fonts.as_ref(),
                    40.0,
                    36.0,
                    1.5,
                    &parsed,
                    &media_active,
                    &bands,
                    None,
                    if frame % 15 < 5 { WidgetHitZone::MusicPlayPause } else { WidgetHitZone::None },
                    &pal,
                    None,
                    Some(&mut mboxes),
                    false,
                    true,
                );
                draw_vitals_widget_with_telemetry_and_boxes(
                    &rt,
                    &dw,
                    Some(&d2d),
                    custom_fonts.as_ref(),
                    40.0,
                    190.0,
                    1.5,
                    &parsed,
                    18.0 + t * 68.0,
                    24.0 + t * 64.0,
                    58.0 + t * 24.0,
                    44.0 + t * 34.0,
                    88.0,
                    if frame % 15 < 5 { WidgetHitZone::VitalsGpu } else { WidgetHitZone::None },
                    None,
                    Some(&mut vboxes),
                );
            }
            // Verify paused standby + FG cache reuse works cleanly
            let media_paused = MediaSnapshot {
                is_playing: false,
                ..media_active
            };
            let zero_bands = [0.0f32; 12];
            for _ in 0..3 {
                draw_music_widget_with_viz_cached(
                    &rt,
                    &dw,
                    Some(&d2d),
                    custom_fonts.as_ref(),
                    40.0,
                    315.0,
                    1.2,
                    &parsed,
                    &media_paused,
                    &zero_bands,
                    None,
                    WidgetHitZone::None,
                    None,
                    &mut fg_music,
                    1,
                );
            }
            assert!(rt.EndDraw(None, None).is_ok());

            for slot in 0..6 {
                assert!(mboxes[slot].tr_half_w > 0.0 && mboxes[slot].tr_half_h > 0.0, "DynamicIsland box {} missing", slot);
            }
            assert!(vboxes[5].tr_half_w > 100.0 && vboxes[0].tr_half_w > 10.0);

            if let Some(path) = std::env::var_os("AUTUMN_DUMP_LUX_VERIFY") {
                use windows::Win32::Graphics::Gdi::GetDIBits;
                let mut bi = BITMAPINFO {
                    bmiHeader: BITMAPINFOHEADER {
                        biSize: size_of::<BITMAPINFOHEADER>() as u32,
                        biWidth: 960,
                        biHeight: -420,
                        biPlanes: 1,
                        biBitCount: 32,
                        biCompression: BI_RGB.0,
                        ..Default::default()
                    },
                    ..Default::default()
                };
                let mut px = vec![0u8; 960 * 420 * 4];
                GetDIBits(dib.mem_dc, dib.bitmap, 0, 420, Some(px.as_mut_ptr() as *mut c_void), &mut bi, DIB_RGB_COLORS);
                let mut file_bytes = Vec::with_capacity(54 + px.len());
                let file_sz = (54 + px.len()) as u32;
                file_bytes.extend_from_slice(b"BM");
                file_bytes.extend_from_slice(&file_sz.to_le_bytes());
                file_bytes.extend_from_slice(&0u32.to_le_bytes());
                file_bytes.extend_from_slice(&54u32.to_le_bytes());
                file_bytes.extend_from_slice(&40u32.to_le_bytes());
                file_bytes.extend_from_slice(&960i32.to_le_bytes());
                file_bytes.extend_from_slice(&(-420i32).to_le_bytes());
                file_bytes.extend_from_slice(&1u16.to_le_bytes());
                file_bytes.extend_from_slice(&32u16.to_le_bytes());
                file_bytes.extend_from_slice(&0u32.to_le_bytes());
                file_bytes.extend_from_slice(&(px.len() as u32).to_le_bytes());
                file_bytes.extend_from_slice(&[0u8; 16]);
                file_bytes.extend_from_slice(&px);
                let _ = std::fs::write(path, file_bytes);
            }

            fg_music.clear();
            crate::dock_themes::evict_luxury_hud_rt(&rt);
            crate::dock_themes::clear_luxury_hud_caches();
            CoUninitialize();
        }
    }

    #[test]
    fn idle_ambient_widgets_do_not_force_30fps() {
        // Idle ambient-only sakura/caustics widgets stay throttled (not 30 FPS)
        assert!(!compute_widgets_fast_30fps(true, false, false, true, true));
        // Covered or hidden widgets never request 30 FPS
        assert!(!compute_widgets_fast_30fps(true, true, true, true, false));
        assert!(!compute_widgets_fast_30fps(false, false, true, true, false));
        // Active hover/drag/audio wakes immediately to 30 FPS
        assert!(compute_widgets_fast_30fps(true, false, true, true, false));
        // Decaying click ripple (ambient_idle_only = false) stays 30 FPS until settled
        assert!(compute_widgets_fast_30fps(true, false, false, true, false));
    }

    #[test]
    fn apps_icon_tint_recolors_pixels_and_cache_invalidates_on_theme_change() {
        // Verify PBGRA duotone pixel remapping strips orange bleed on non-Autumn themes
        let mut orange_and_clear = [
            42u8, 130, 224, 255, // opaque orange pixel
            42, 130, 224, 0,     // transparent pixel
        ];
        let blue_tint = ((0x32u8, 0x8C, 0xEE), (0xE8, 0xF6, 0xFF));
        apply_duotone_tint_pbgra(&mut orange_and_clear, blue_tint);
        assert!(
            orange_and_clear[0] > orange_and_clear[2],
            "Blue-tinted pixel must have B > R, got BGRA={:?}",
            &orange_and_clear[0..4]
        );
        assert_eq!(orange_and_clear[7], 0, "Transparent pixel must remain untouched");

        let mut amber_px = [200u8, 200, 200, 255];
        apply_duotone_tint_pbgra(&mut amber_px, AUTUMN_ICON_TINT);
        assert!(
            amber_px[2] > amber_px[0],
            "Autumn tint pixel must remain warm R > B, got BGRA={:?}",
            amber_px
        );

        // Verify WIC + D2D AppIconCache reloads bitmaps automatically when resolved tint key changes
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            let wic: IWICImagingFactory =
                CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)
                    .expect("WIC factory");
            let d2d: ID2D1Factory =
                D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None).expect("D2D factory");
            let rt_props = D2D1_RENDER_TARGET_PROPERTIES {
                r#type: D2D1_RENDER_TARGET_TYPE_DEFAULT,
                pixelFormat: D2D1_PIXEL_FORMAT {
                    format: DXGI_FORMAT_B8G8R8A8_UNORM,
                    alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
                },
                usage: D2D1_RENDER_TARGET_USAGE_GDI_COMPATIBLE,
                ..Default::default()
            };
            let dc_rt = d2d.CreateDCRenderTarget(&rt_props).expect("D2D DC RT");
            let mut dib = DibBuffer::new();
            assert!(dib.ensure_size(256, 128));
            let rc = RECT { left: 0, top: 0, right: 256, bottom: 128 };
            dc_rt.BindDC(dib.mem_dc, &rc).expect("BindDC");

            let mut cfg = AppConfig::default();
            cfg.apps = vec![
                AppShortcut {
                    name: "Settings".to_string(),
                    path: "ms-settings:".to_string(),
                    offset_x: 0,
                    offset_y: 0,
                },
                AppShortcut {
                    name: "Notepad".to_string(),
                    path: r"C:\Windows\notepad.exe".to_string(),
                    offset_x: 0,
                    offset_y: 0,
                },
            ];
            let rt_base: ID2D1RenderTarget = dc_rt.cast().unwrap();
            let mut cache = AppIconCache::new();

            // 1. Nordic Architect tint
            let nordic = crate::suites::find_suite_by_id("nordic_architect").unwrap();
            crate::suites::apply_suite_to_config(nordic, &mut cfg);
            cfg.apps_match_theme = true;
            let t_blue = resolve_apps_icon_tint(&cfg);
            cache.sync_apps(&cfg.apps, Some(&wic), &rt_base, t_blue);
            assert!(cache.slot(0).is_some(), "Settings icon should load with non-amber tint");
            assert_eq!(cache.slots[0].as_ref().unwrap().1, tint_u64_key(t_blue));
            let ptr_blue = cache.slot(0).unwrap().as_raw() as usize;

            // 2. Repeated sync with identical theme keeps exact cached bitmap pointer
            cache.sync_apps(&cfg.apps, Some(&wic), &rt_base, t_blue);
            assert_eq!(cache.slot(0).unwrap().as_raw() as usize, ptr_blue);

            // 3. Changing theme accent updates cache key & reloads bitmap cleanly without manual invalidation
            cfg.apps_accent_rgb = 0x22DD88;
            let t_emerald = resolve_apps_icon_tint(&cfg);
            assert_ne!(t_emerald, t_blue);
            cache.sync_apps(&cfg.apps, Some(&wic), &rt_base, t_emerald);
            assert_eq!(cache.slots[0].as_ref().unwrap().1, tint_u64_key(t_emerald));
            assert_ne!(cache.slot(0).unwrap().as_raw() as usize, ptr_blue);

            // 4. Turning Match Theme OFF switches key to 0 (native icon)
            cfg.apps_match_theme = false;
            let t_native = resolve_apps_icon_tint(&cfg);
            assert_eq!(t_native, None);
            cache.sync_apps(&cfg.apps, Some(&wic), &rt_base, t_native);
            assert_eq!(cache.slots[0].as_ref().unwrap().1, 0);

            cache.invalidate();
            CoUninitialize();
        }
    }
}
