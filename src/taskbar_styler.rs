use std::ffi::c_void;
use std::mem::size_of;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use windows::core::{s, w, BOOL, GUID, HRESULT, Interface, PCWSTR};
use windows::Win32::Foundation::{
    CloseHandle, COLORREF, HANDLE, HINSTANCE, HWND, INVALID_HANDLE_VALUE, LPARAM, LRESULT, POINT,
    RECT, SIZE, WPARAM,
};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_COLOR_F, D2D1_FIGURE_BEGIN_FILLED,
    D2D1_FIGURE_END_CLOSED, D2D1_GRADIENT_STOP, D2D1_PIXEL_FORMAT, D2D_RECT_F,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1CreateFactory, ID2D1DCRenderTarget, ID2D1Factory, ID2D1Geometry, ID2D1Layer,
    ID2D1RenderTarget,
    ID2D1LinearGradientBrush, ID2D1PathGeometry, D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
    D2D1_ELLIPSE, D2D1_EXTEND_MODE_CLAMP, D2D1_FACTORY_TYPE_SINGLE_THREADED, D2D1_GAMMA_2_2,
    D2D1_LAYER_OPTIONS_NONE, D2D1_LAYER_PARAMETERS, D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES,
    D2D1_RENDER_TARGET_PROPERTIES, D2D1_RENDER_TARGET_TYPE_DEFAULT,
    D2D1_RENDER_TARGET_USAGE_GDI_COMPATIBLE, D2D1_ROUNDED_RECT,
};
use windows::Win32::Graphics::Dwm::{
    DwmExtendFrameIntoClientArea, DwmSetWindowAttribute, DWMWINDOWATTRIBUTE, DWMWA_BORDER_COLOR,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GetMonitorInfoW, InvalidateRect,
    MonitorFromWindow, SelectObject, SetWindowRgn, AC_SRC_ALPHA, AC_SRC_OVER, BITMAPINFO,
    BITMAPINFOHEADER, BI_RGB, BLENDFUNCTION, DIB_RGB_COLORS, HBITMAP, HDC, HGDIOBJ, MONITORINFO,
    MONITOR_DEFAULTTONEAREST,
};
use windows::Win32::Security::{
    InitializeSecurityDescriptor, SetSecurityDescriptorDacl, PSECURITY_DESCRIPTOR,
    SECURITY_ATTRIBUTES, SECURITY_DESCRIPTOR,
};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER,
    COINIT_MULTITHREADED,
};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, GetFinalPathNameByHandleW, FILE_ATTRIBUTE_NORMAL, FILE_FLAG_BACKUP_SEMANTICS,
    FILE_GENERIC_READ, FILE_NAME_NORMALIZED, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
    OPEN_EXISTING,
};
use windows::Win32::System::LibraryLoader::{
    GetModuleFileNameW, GetModuleHandleW, GetProcAddress, LoadLibraryW,
};
use windows::Win32::System::Memory::{
    CreateFileMappingW, MapViewOfFile, OpenFileMappingW, UnmapViewOfFile, FILE_MAP_ALL_ACCESS,
    FILE_MAP_WRITE, MEMORY_MAPPED_VIEW_ADDRESS, PAGE_READWRITE,
};
use windows::Win32::System::Variant::VARIANT;
use windows::Win32::UI::Accessibility::{
    CUIAutomation, IUIAutomation, IUIAutomationCacheRequest, IUIAutomationCondition,
    TreeScope_Element, TreeScope_Subtree, UIA_AutomationIdPropertyId,
    UIA_BoundingRectanglePropertyId, UIA_ButtonControlTypeId, UIA_ClassNamePropertyId,
    UIA_ControlTypePropertyId,
};
use windows::Win32::UI::Controls::MARGINS;
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DestroyWindow, FindWindowExW, FindWindowW, GetCursorPos, GetWindow,
    GetWindowLongW, GetWindowRect, GetWindowThreadProcessId, IsWindow, IsWindowVisible,
    LoadCursorW, PostMessageW, RegisterClassExW, SendMessageTimeoutW, SetWindowPos, ShowWindow,
    UpdateLayeredWindow, GWL_EXSTYLE, GW_HWNDNEXT, GW_HWNDPREV, HWND_MESSAGE, HWND_NOTOPMOST,
    HWND_TOPMOST,
    IDC_ARROW, SMTO_ABORTIFHUNG, SWP_FRAMECHANGED, SWP_HIDEWINDOW, SWP_NOACTIVATE, SWP_NOMOVE,
    SWP_NOSENDCHANGING, SWP_NOSIZE, SWP_NOZORDER, SWP_SHOWWINDOW, SW_HIDE, ULW_ALPHA, WNDCLASSEXW,
    WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
};
use windows_numerics::Vector2;

use crate::config::{
    ensure_windows_transparency_enabled, load_pill_cache, save_pill_cache, to_wide,
    windows_system_uses_light_theme, AppConfig, PillMode, SurfaceMode, TaskbarTheme,
    TaskbarVizStyle, VizColorMode,
};
use crate::theme::{resolve_palette_for, ThemePalette};

fn cached_system_uses_light_theme() -> bool {
    static CACHE: OnceLock<Mutex<(Option<Instant>, bool)>> = OnceLock::new();
    let lock = CACHE.get_or_init(|| Mutex::new((None, false)));
    if let Ok(mut guard) = lock.lock() {
        let stale = guard
            .0
            .map(|t| t.elapsed() >= Duration::from_secs(2))
            .unwrap_or(true);
        if stale {
            let val = windows_system_uses_light_theme();
            *guard = (Some(Instant::now()), val);
        }
        guard.1
    } else {
        false
    }
}

pub fn elemental_palette_for_fx(style: TaskbarVizStyle) -> Option<ThemePalette> {
    match style {
        TaskbarVizStyle::HoverSnow => Some(ThemePalette {
            accent: (120, 212, 255),
            accent_light: (230, 248, 255),
            accent_deep: (42, 128, 200),
            rim: (158, 224, 255),
            text_primary: (242, 250, 255),
            text_muted: (192, 224, 246),
            glass_top: (14, 24, 38),
            glass_bot: (10, 16, 26),
            preview_a: (12, 24, 42),
            preview_b: (22, 48, 82),
            preview_c: (10, 18, 32),
            swatches: [0x78D4FF, 0xE6F8FF, 0x2A80C8, 0x163052, 0x0A1220],
        }),
        TaskbarVizStyle::HoverFire => Some(ThemePalette {
            accent: (255, 118, 34),
            accent_light: (255, 224, 108),
            accent_deep: (198, 38, 18),
            rim: (255, 144, 52),
            text_primary: (255, 242, 224),
            text_muted: (242, 198, 168),
            glass_top: (26, 12, 10),
            glass_bot: (16, 8, 6),
            preview_a: (36, 12, 8),
            preview_b: (82, 24, 10),
            preview_c: (20, 8, 6),
            swatches: [0xFF7622, 0xFFE06C, 0xC62612, 0x52180A, 0x140806],
        }),
        TaskbarVizStyle::HoverWater => Some(ThemePalette {
            accent: (46, 222, 240),
            accent_light: (176, 252, 255),
            accent_deep: (14, 118, 168),
            rim: (86, 236, 248),
            text_primary: (232, 252, 255),
            text_muted: (176, 230, 240),
            glass_top: (10, 24, 34),
            glass_bot: (6, 16, 24),
            preview_a: (8, 26, 40),
            preview_b: (14, 62, 88),
            preview_c: (6, 18, 30),
            swatches: [0x2EDEF0, 0xB0FCFF, 0x0E76A8, 0x0E3E58, 0x06121E],
        }),
        TaskbarVizStyle::HoverSakura => Some(ThemePalette {
            accent: (255, 138, 180),
            accent_light: (255, 222, 236),
            accent_deep: (194, 58, 118),
            rim: (255, 162, 200),
            text_primary: (255, 242, 248),
            text_muted: (240, 200, 218),
            glass_top: (26, 14, 22),
            glass_bot: (18, 10, 16),
            preview_a: (36, 14, 28),
            preview_b: (72, 26, 54),
            preview_c: (22, 10, 18),
            swatches: [0xFF8AB4, 0xFFDEEC, 0xC23A76, 0x481A36, 0x160A12],
        }),
        TaskbarVizStyle::HoverCosmic => Some(ThemePalette {
            accent: (152, 126, 255),
            accent_light: (132, 240, 255),
            accent_deep: (74, 46, 184),
            rim: (158, 144, 255),
            text_primary: (242, 244, 255),
            text_muted: (196, 198, 240),
            glass_top: (14, 12, 28),
            glass_bot: (9, 8, 20),
            preview_a: (14, 12, 34),
            preview_b: (36, 24, 76),
            preview_c: (10, 8, 24),
            swatches: [0x987EFF, 0x84F0FF, 0x4A2EB8, 0x24184C, 0x0A0818],
        }),
        TaskbarVizStyle::HoverPlasma => Some(ThemePalette {
            accent: (64, 240, 255),
            accent_light: (255, 112, 242),
            accent_deep: (128, 32, 220),
            rim: (96, 238, 255),
            text_primary: (240, 250, 255),
            text_muted: (204, 212, 248),
            glass_top: (16, 10, 30),
            glass_bot: (10, 8, 22),
            preview_a: (18, 10, 38),
            preview_b: (44, 18, 78),
            preview_c: (10, 12, 30),
            swatches: [0x40F0FF, 0xFF70F2, 0x8020DC, 0x2C124E, 0x0A0C1E],
        }),
        TaskbarVizStyle::HoverMacWave => Some(ThemePalette {
            accent: (184, 220, 255),
            accent_light: (244, 250, 255),
            accent_deep: (74, 134, 212),
            rim: (224, 238, 255),
            text_primary: (245, 248, 255),
            text_muted: (196, 212, 235),
            glass_top: (28, 34, 48),
            glass_bot: (16, 20, 30),
            preview_a: (18, 34, 62),
            preview_b: (46, 86, 138),
            preview_c: (14, 24, 42),
            swatches: [0xB8DCFF, 0xF4FAFF, 0x4A86D4, 0x2E568A, 0x0E182A],
        }),
        TaskbarVizStyle::HoverMercury => Some(ThemePalette {
            accent: (214, 228, 244),
            accent_light: (252, 254, 255),
            accent_deep: (96, 114, 138),
            rim: (236, 244, 252),
            text_primary: (248, 252, 255),
            text_muted: (186, 198, 214),
            glass_top: (24, 28, 34),
            glass_bot: (12, 14, 18),
            preview_a: (18, 22, 30),
            preview_b: (52, 62, 76),
            preview_c: (12, 14, 20),
            swatches: [0xD6E4F4, 0xFCFEFF, 0x60728A, 0x343E4C, 0x0C0E14],
        }),
        TaskbarVizStyle::HoverHydroBubbles => Some(ThemePalette {
            accent: (48, 228, 244),
            accent_light: (196, 252, 255),
            accent_deep: (16, 128, 178),
            rim: (124, 244, 255),
            text_primary: (238, 253, 255),
            text_muted: (176, 232, 244),
            glass_top: (10, 34, 48),
            glass_bot: (8, 20, 32),
            preview_a: (8, 52, 82),
            preview_b: (22, 124, 162),
            preview_c: (6, 30, 52),
            swatches: [0x30E4F4, 0xC4FCFF, 0x1080B2, 0x167CA2, 0x061E34],
        }),
        TaskbarVizStyle::HoverQuantum => Some(ThemePalette {
            accent: (56, 226, 255),
            accent_light: (255, 108, 224),
            accent_deep: (116, 46, 224),
            rim: (118, 236, 255),
            text_primary: (244, 248, 255),
            text_muted: (198, 206, 244),
            glass_top: (14, 12, 30),
            glass_bot: (8, 8, 20),
            preview_a: (14, 10, 34),
            preview_b: (48, 20, 82),
            preview_c: (8, 10, 26),
            swatches: [0x38E2FF, 0xFF6CE0, 0x742EE0, 0x301452, 0x080A1A],
        }),
        TaskbarVizStyle::HoverBiolum => Some(ThemePalette {
            accent: (20, 241, 217),
            accent_light: (172, 146, 255),
            accent_deep: (14, 114, 152),
            rim: (64, 244, 228),
            text_primary: (236, 254, 252),
            text_muted: (162, 222, 224),
            glass_top: (6, 16, 30),
            glass_bot: (4, 10, 22),
            preview_a: (5, 12, 28),
            preview_b: (10, 42, 72),
            preview_c: (4, 8, 20),
            swatches: [0x14F1D9, 0xAC92FF, 0x0E7298, 0x0A2A48, 0x040814],
        }),
        TaskbarVizStyle::HoverSynthGrid => Some(ThemePalette {
            accent: (255, 52, 168),
            accent_light: (36, 240, 255),
            accent_deep: (138, 28, 198),
            rim: (255, 86, 194),
            text_primary: (255, 240, 252),
            text_muted: (232, 178, 238),
            glass_top: (22, 8, 36),
            glass_bot: (12, 6, 24),
            preview_a: (28, 8, 46),
            preview_b: (96, 22, 88),
            preview_c: (14, 6, 28),
            swatches: [0xFF34A8, 0x24F0FF, 0x8A1CC6, 0x601658, 0x0E061C],
        }),
        _ => None,
    }
}

pub fn is_vu_tricolor_active(cfg: &AppConfig) -> bool {
    cfg.tb_theme == TaskbarTheme::VuPeakStudio
        && cfg.tb_viz_color_mode == VizColorMode::ThemeNative
        && !cfg.tb_match_wallpaper
}

pub fn effective_taskbar_palette(cfg: &AppConfig) -> ThemePalette {
    if cfg.tb_match_wallpaper || cfg.tb_viz_color_mode == VizColorMode::MatchWallpaper {
        return resolve_palette_for(cfg, true);
    }
    let native_pal = if let Some(pal) = crate::dock_themes::dock_palette(cfg.tb_theme) {
        pal
    } else if let Some(pal) = ThemePalette::for_music_taskbar_theme(cfg.tb_theme) {
        pal
    } else {
        match cfg.tb_theme {
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
                glass_top: (246, 242, 252),
                glass_bot: (234, 230, 244),
                preview_a: (244, 76, 186),
                preview_b: (192, 122, 242),
                preview_c: (166, 146, 248),
                swatches: [0xF6F2FC, 0xF44CBA, 0xC07AF2, 0xE04884, 0x181422],
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
            TaskbarTheme::CupertinoDock => ThemePalette {
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
            TaskbarTheme::DynamicIslandDock => ThemePalette {
                accent: (158, 184, 224),
                accent_light: (94, 234, 212),
                accent_deep: (64, 78, 108),
                rim: (188, 204, 232),
                text_primary: (244, 247, 252),
                text_muted: (168, 180, 202),
                glass_top: (14, 16, 22),
                glass_bot: (8, 10, 14),
                preview_a: (16, 22, 38),
                preview_b: (34, 52, 82),
                preview_c: (10, 12, 22),
                swatches: [0x9EB8E0, 0x5EEAD4, 0x404E6C, 0x223452, 0x0A0C16],
            },
            TaskbarTheme::FrutigerAero7 => ThemePalette {
                accent: (84, 224, 255),
                accent_light: (232, 252, 255),
                accent_deep: (18, 126, 188),
                rim: (184, 244, 255),
                text_primary: (244, 252, 255),
                text_muted: (188, 232, 248),
                glass_top: (18, 86, 128),
                glass_bot: (10, 48, 86),
                preview_a: (14, 96, 160),
                preview_b: (56, 184, 230),
                preview_c: (12, 66, 114),
                swatches: [0x54E0FF, 0xE8FCFF, 0x127EBC, 0x38B8E6, 0x0C4272],
            },
            TaskbarTheme::UnixPolybar => ThemePalette {
                accent: (180, 190, 254),
                accent_light: (137, 220, 235),
                accent_deep: (250, 179, 135),
                rim: (180, 190, 254),
                text_primary: (205, 214, 244),
                text_muted: (166, 173, 200),
                glass_top: (24, 24, 38),
                glass_bot: (18, 18, 28),
                preview_a: (24, 24, 42),
                preview_b: (52, 46, 86),
                preview_c: (18, 18, 32),
                swatches: [0xB4BEFE, 0x89DCEB, 0xFAB387, 0x342E56, 0x121220],
            },
            TaskbarTheme::CyberArasakaDeck => ThemePalette {
                accent: (0, 240, 255),
                accent_light: (255, 58, 102),
                accent_deep: (255, 184, 0),
                rim: (0, 240, 255),
                text_primary: (238, 250, 255),
                text_muted: (162, 204, 218),
                glass_top: (14, 16, 24),
                glass_bot: (8, 10, 16),
                preview_a: (16, 12, 28),
                preview_b: (64, 18, 48),
                preview_c: (10, 16, 30),
                swatches: [0x00F0FF, 0xFF3A66, 0xFFB800, 0x401230, 0x0A101E],
            },
            TaskbarTheme::Retro95Classic => ThemePalette {
                accent: (206, 214, 226),
                accent_light: (242, 246, 252),
                accent_deep: (74, 82, 98),
                rim: (218, 224, 234),
                text_primary: (242, 246, 252),
                text_muted: (188, 196, 210),
                glass_top: (76, 82, 96),
                glass_bot: (58, 64, 76),
                preview_a: (0, 102, 104),
                preview_b: (0, 132, 132),
                preview_c: (0, 74, 78),
                swatches: [0xCED6E2, 0xF2F6FC, 0x4A5262, 0x008484, 0x004A4E],
            },
            _ => {
                if let Some(pal) = elemental_palette_for_fx(cfg.tb_viz_style) {
                    pal
                } else {
                    match cfg.tb_theme {
                        TaskbarTheme::HoverSnowfall => ThemePalette::from_accent_rgb(0x78D4FF),
                        TaskbarTheme::HoverInferno => ThemePalette::from_accent_rgb(0xFF7622),
                        TaskbarTheme::HoverWaterRipples => ThemePalette::from_accent_rgb(0x2EDEF0),
                        TaskbarTheme::HoverSakuraWind => ThemePalette::from_accent_rgb(0xFF8AB4),
                        TaskbarTheme::HoverNebulaStardust => ThemePalette::from_accent_rgb(0x987EFF),
                        TaskbarTheme::HoverNeonPlasma => ThemePalette::from_accent_rgb(0x40F0FF),
                        _ => ThemePalette::autumn(),
                    }
                }
            }
        }
    };

    if cfg.tb_viz_color_mode == VizColorMode::CustomAccent {
        return ThemePalette::with_custom_viz_accent(cfg.tb_viz_accent_rgb, &native_pal);
    }
    native_pal
}

#[allow(clippy::too_many_arguments)]
pub unsafe fn paint_widget_elemental_fx(
    rt: &ID2D1RenderTarget,
    d2d: Option<&ID2D1Factory>,
    l: f32,
    t: f32,
    r: f32,
    b: f32,
    canvas_box: (f32, f32, f32, f32),
    rad: f32,
    scale_u: f32,
    fx: TaskbarVizStyle,
    opacity_pct: u8,
    base_pal: &ThemePalette,
    use_element_colors: bool,
    fx_viz: Option<&VizSnapshot>,
) {
    if fx == TaskbarVizStyle::Off {
        return;
    }
    let Some(viz) = fx_viz else {
        return;
    };
    let fx_pal = if use_element_colors {
        elemental_palette_for_fx(fx).unwrap_or_else(|| base_pal.clone())
    } else {
        base_pal.clone()
    };
    draw_taskbar_viz_in_rect(
        rt,
        d2d,
        l,
        t,
        r,
        b,
        canvas_box,
        rad.max(2.0),
        scale_u.max(0.35),
        false,
        true,
        fx,
        opacity_pct,
        false,
        &fx_pal,
        viz,
    );
}

const SIM_PARTICLE_COUNT: usize = 36;
const SIM_RIPPLE_COUNT: usize = 10;

#[derive(Clone, Copy)]
pub struct TaskbarParticle {
    pub x: f32,
    pub y: f32,
    pub vx: f32,
    pub vy: f32,
    pub size: f32,
    pub phase: f32,
    pub life: f32,
    pub kind: u8,
}

impl Default for TaskbarParticle {
    fn default() -> Self {
        Self {
            x: 0.5,
            y: 0.5,
            vx: 0.0,
            vy: 0.0,
            size: 1.0,
            phase: 0.0,
            life: 1.0,
            kind: 0,
        }
    }
}

#[derive(Clone, Copy, Default)]
pub struct TaskbarRipple {
    pub x: f32,
    pub y: f32,
    pub age: f32,
    pub max_age: f32,
    pub strength: f32,
    pub active: bool,
}

#[derive(Clone, Copy)]
pub struct VizSnapshot {
    pub bands: [f32; 24],
    pub peaks: [f32; 24],
    pub peak_hold: [f32; 24],
    pub phase: f32,
    pub energy: f32,
    pub beat_pulse: f32,
    pub particles: [TaskbarParticle; SIM_PARTICLE_COUNT],
    pub ripples: [TaskbarRipple; SIM_RIPPLE_COUNT],
    pub sim_inited: bool,
    pub sim_kind: u8,
    pub hover_active: bool,
    pub hover_mix: f32,
    pub hover_x: f32,
    pub hover_y: f32,
    pub hover_vx: f32,
    pub hover_vy: f32,
    pub last_ripple_x: f32,
    pub last_ripple_y: f32,
    pub ripple_cursor: usize,
    pub ripple_cd: f32,
    pub auto_drop_cd: f32,
}

impl Default for VizSnapshot {
    fn default() -> Self {
        Self {
            bands: [0.0; 24],
            peaks: [0.0; 24],
            peak_hold: [0.0; 24],
            phase: 0.0,
            energy: 0.0,
            beat_pulse: 0.0,
            particles: [TaskbarParticle::default(); SIM_PARTICLE_COUNT],
            ripples: [TaskbarRipple::default(); SIM_RIPPLE_COUNT],
            sim_inited: false,
            sim_kind: 0,
            hover_active: false,
            hover_mix: 0.0,
            hover_x: 0.5,
            hover_y: 0.5,
            hover_vx: 0.0,
            hover_vy: 0.0,
            last_ripple_x: 0.5,
            last_ripple_y: 0.5,
            ripple_cursor: 0,
            ripple_cd: 0.0,
            auto_drop_cd: 0.4,
        }
    }
}

#[inline]
fn hash_frac(idx: usize, salt: f32) -> f32 {
    (((idx as f32 + 1.0) * 12.9898 + salt * 78.233).sin() * 43758.547)
        .fract()
        .abs()
}

fn style_sim_kind(style: TaskbarVizStyle) -> u8 {
    match style {
        TaskbarVizStyle::HoverSnow => 1,
        TaskbarVizStyle::HoverFire => 2,
        TaskbarVizStyle::HoverWater => 3,
        TaskbarVizStyle::HoverSakura => 4,
        TaskbarVizStyle::HoverCosmic => 5,
        TaskbarVizStyle::HoverPlasma => 6,
        TaskbarVizStyle::HoverMacWave => 7,
        TaskbarVizStyle::HoverMercury => 8,
        TaskbarVizStyle::HoverHydroBubbles => 9,
        TaskbarVizStyle::HoverQuantum => 10,
        TaskbarVizStyle::HoverBiolum => 11,
        TaskbarVizStyle::HoverSynthGrid => 12,
        _ => 0,
    }
}

impl VizSnapshot {
    pub fn spawn_impulse(&mut self, nx: f32, ny: f32, strength: f32, max_age: f32) {
        let idx = self.ripple_cursor % SIM_RIPPLE_COUNT;
        self.ripple_cursor = (idx + 1) % SIM_RIPPLE_COUNT;
        self.ripples[idx] = TaskbarRipple {
            x: nx.clamp(0.0, 1.0),
            y: ny.clamp(0.0, 1.0),
            age: 0.0,
            max_age: max_age.clamp(0.35, 2.4),
            strength: strength.clamp(0.2, 1.6),
            active: true,
        };
    }

    /// Returns `true` when hover fade, ripples, or hover-FX are still active in flight.
    pub fn is_active(&self) -> bool {
        self.hover_active
            || self.hover_mix > 0.0005
            || self.ripples.iter().any(|r| r.active)
    }

    /// Lightweight continuous physical cursor + ripple simulation for photorealistic materials
    /// even when a widget's standalone neon `hover_fx` is set to `Off`.
    /// Returns `true` if visual state changed on this step (caller needs a redraw).
    pub fn step_material_interactive(
        &mut self,
        dt_raw: f32,
        target_x: f32,
        target_y: f32,
        hover_in: bool,
        hover_react: bool,
        spawn_cursor_ripples: bool,
    ) -> bool {
        let dt = dt_raw.clamp(0.008, 0.055);
        let tx = target_x.clamp(0.0, 1.0);
        let ty = target_y.clamp(0.0, 1.0);
        let prev_mix = self.hover_mix;
        let prev_x = self.hover_x;
        let prev_y = self.hover_y;
        let had_ripples = self.ripples.iter().any(|r| r.active);
        let active_hover = hover_in && hover_react;

        if active_hover {
            if !self.hover_active {
                self.hover_x = tx;
                self.hover_y = ty;
                self.last_ripple_x = tx;
                self.last_ripple_y = ty;
                self.hover_vx = 0.0;
                self.hover_vy = 0.0;
                self.ripple_cd = 0.0;
                if spawn_cursor_ripples {
                    self.spawn_impulse(tx, ty, 0.78, 1.25);
                }
            } else {
                let raw_vx = ((tx - self.hover_x) / dt).clamp(-5.5, 5.5);
                let raw_vy = ((ty - self.hover_y) / dt).clamp(-5.5, 5.5);
                self.hover_vx = self.hover_vx * 0.58 + raw_vx * 0.42;
                self.hover_vy = self.hover_vy * 0.58 + raw_vy * 0.42;
                self.hover_x = tx;
                self.hover_y = ty;
            }
            self.hover_mix = (self.hover_mix + dt * 6.5).min(1.0);
        } else {
            self.hover_vx *= 0.85;
            self.hover_vy *= 0.85;
            self.hover_mix = (self.hover_mix - dt * 2.4).max(0.0);
            if self.hover_mix < 0.002 {
                self.hover_mix = 0.0;
                self.hover_vx = 0.0;
                self.hover_vy = 0.0;
            }
        }
        self.hover_active = active_hover;
        if !self.phase.is_finite() {
            self.phase = 0.0;
        }
        self.phase = (self.phase + dt * 1.45) % (std::f32::consts::TAU * 64.0);

        // Advance active ripples
        for r in &mut self.ripples {
            if r.active {
                r.age += dt;
                if r.age >= r.max_age {
                    r.active = false;
                }
            }
        }
        self.ripple_cd = (self.ripple_cd - dt).max(0.0);
        if spawn_cursor_ripples && active_hover && self.ripple_cd <= 0.0 {
            let dx = self.hover_x - self.last_ripple_x;
            let dy = self.hover_y - self.last_ripple_y;
            let dist = (dx * dx + dy * dy).sqrt();
            if dist >= 0.055 {
                let speed = (self.hover_vx * self.hover_vx + self.hover_vy * self.hover_vy).sqrt();
                self.spawn_impulse(
                    self.hover_x,
                    self.hover_y,
                    (0.55 + speed * 0.14).clamp(0.45, 1.15),
                    1.15,
                );
                self.last_ripple_x = self.hover_x;
                self.last_ripple_y = self.hover_y;
                self.ripple_cd = 0.11;
            }
        }
        let has_ripples = self.ripples.iter().any(|r| r.active);
        (self.hover_mix - prev_mix).abs() > 0.0005
            || (prev_mix > 0.0 && self.hover_mix == 0.0)
            || (active_hover
                && ((self.hover_x - prev_x).abs() > 0.0015
                    || (self.hover_y - prev_y).abs() > 0.0015))
            || had_ripples
            || has_ripples
    }

    pub fn step_hover_physics(
        &mut self,
        style: TaskbarVizStyle,
        dt_raw: f32,
        target_x: f32,
        target_y: f32,
        hover_in: bool,
        hover_react: bool,
        w_px: f32,
        h_px: f32,
    ) {
        let dt = dt_raw.clamp(0.008, 0.055);
        let scale_x = w_px.max(40.0);
        let scale_y = h_px.max(24.0);
        let is_tall = scale_y > scale_x * 1.8;
        let sim_kind = style_sim_kind(style);

        let tx = target_x.clamp(0.0, 1.0);
        let ty = target_y.clamp(0.0, 1.0);
        let active_hover = hover_in && hover_react;

        if active_hover {
            if !self.hover_active {
                self.hover_x = tx;
                self.hover_y = ty;
                self.last_ripple_x = tx;
                self.last_ripple_y = ty;
                self.hover_vx = 0.0;
                self.hover_vy = 0.0;
                self.ripple_cd = 0.0;
            } else {
                let raw_vx = ((tx - self.hover_x) / dt).clamp(-5.5, 5.5);
                let raw_vy = ((ty - self.hover_y) / dt).clamp(-5.5, 5.5);
                self.hover_vx = self.hover_vx * 0.55 + raw_vx * 0.45;
                self.hover_vy = self.hover_vy * 0.55 + raw_vy * 0.45;
                self.hover_x = tx;
                self.hover_y = ty;
            }
            self.hover_mix = (self.hover_mix + dt * 7.5).min(1.0);
        } else {
            self.hover_vx *= 0.84;
            self.hover_vy *= 0.84;
            self.hover_mix = (self.hover_mix - dt * 2.2).max(0.0);
            if self.hover_mix < 0.002 {
                self.hover_mix = 0.0;
                self.hover_vx = 0.0;
                self.hover_vy = 0.0;
            }
        }
        let prev_hover = self.hover_active;
        self.hover_active = active_hover;
        if !self.phase.is_finite() {
            self.phase = 0.0;
        }
        self.phase = (self.phase + dt * 1.65) % (std::f32::consts::TAU * 64.0);

        // Age any click/tactile impulse ripples cleanly for styles that don't manage ripples below
        if sim_kind != 3 && sim_kind != 9 {
            for r in &mut self.ripples {
                if r.active {
                    r.age += dt;
                    if r.age >= r.max_age {
                        r.active = false;
                    }
                }
            }
        }

        if sim_kind == 0 {
            self.sim_kind = 0;
            return;
        }

        if !self.sim_inited || self.sim_kind != sim_kind {
            self.sim_inited = true;
            self.sim_kind = sim_kind;
            self.ripples = [TaskbarRipple::default(); SIM_RIPPLE_COUNT];
            for i in 0..SIM_PARTICLE_COUNT {
                let fx = hash_frac(i, sim_kind as f32 + 1.3);
                let fy = hash_frac(i, sim_kind as f32 + 7.9);
                let fz = hash_frac(i, sim_kind as f32 + 3.7);
                self.particles[i] = match sim_kind {
                    7 => TaskbarParticle {
                        x: if i < 28 { i as f32 / 27.0 } else { (i - 28) as f32 / 7.0 },
                        y: 0.0,
                        vx: 0.0,
                        vy: 0.0,
                        size: 0.6 + fz * 0.8,
                        phase: fx * std::f32::consts::TAU,
                        life: 0.35,
                        kind: if i < 28 { 0 } else { 1 },
                    },
                    8 => TaskbarParticle {
                        x: if i < 20 { i as f32 / 19.0 } else if is_tall { 0.4 + fy * 0.2 } else { fx },
                        y: if i < 20 { 0.0 } else if is_tall { fx } else { 0.4 + fy * 0.2 },
                        vx: 0.0,
                        vy: 0.0,
                        size: 0.65 + fz * 0.9,
                        phase: fx * std::f32::consts::TAU,
                        life: 1.0,
                        kind: if i < 20 { 0 } else { 1 },
                    },
                    10 => TaskbarParticle {
                        x: fx,
                        y: fy,
                        vx: (fz - 0.5) * 0.4,
                        vy: (fx - 0.5) * 0.4,
                        size: 0.6 + fz * 0.9,
                        phase: (i as f32 / 24.0) * std::f32::consts::TAU,
                        life: fy,
                        kind: if i < 12 { 0 } else if i < 24 { 1 } else { 2 },
                    },
                    12 => TaskbarParticle {
                        x: if i == 0 { 0.5 } else { fx },
                        y: if i <= 1 { 0.5 * (1 - i) as f32 } else { fy },
                        vx: 0.0,
                        vy: 0.0,
                        size: 0.6 + fz * 0.8,
                        phase: fx * std::f32::consts::TAU,
                        life: 0.5,
                        kind: (i % 3) as u8,
                    },
                    _ => TaskbarParticle {
                        x: fx,
                        y: if sim_kind == 2 { 0.15 + fy * 0.83 } else { fy },
                        vx: (fz - 0.5) * 0.06,
                        vy: (fx - 0.5) * 0.04,
                        size: 0.55 + fz * 1.05,
                        phase: fx * std::f32::consts::TAU,
                        life: fy,
                        kind: (i % 3) as u8,
                    },
                };
            }
        }

        let react_r = (scale_x.min(scale_y) * 1.55).clamp(52.0, 132.0);
        let speed_px = (self.hover_vx * scale_x).hypot(self.hover_vy * scale_y);

        match sim_kind {
            // 1 = Snowfall (falling crystals pushed into spinning eddy gusts by cursor)
            1 => {
                for (i, p) in self.particles.iter_mut().enumerate() {
                    p.phase = (p.phase + dt * (1.2 + p.size * 0.8)) % std::f32::consts::TAU;
                    let base_vx_px = (self.phase * 0.75 + p.phase).sin() * (14.0 + p.size * 8.0);
                    let base_vy_px = 20.0 + p.size * 24.0;
                    let mut pvx = p.vx * scale_x + (base_vx_px - p.vx * scale_x) * dt * 2.2;
                    let mut pvy = p.vy * scale_y + (base_vy_px - p.vy * scale_y) * dt * 2.4;

                    if self.hover_mix > 0.02 {
                        let dx = (p.x - self.hover_x) * scale_x;
                        let dy = (p.y - self.hover_y) * scale_y;
                        let dist = dx.hypot(dy).max(1.0);
                        if dist < react_r {
                            let f = (1.0 - dist / react_r).powi(2) * self.hover_mix;
                            let inv = 1.0 / dist;
                            let nx = dx * inv;
                            let ny = dy * inv;
                            // Radial push + vortex swirl + pointer wake carry
                            pvx += (nx * 135.0 - ny * 115.0 + self.hover_vx * scale_x * 0.55) * f * dt * 4.5;
                            pvy += (ny * 125.0 + nx * 115.0 + self.hover_vy * scale_y * 0.55) * f * dt * 4.5;
                        }
                    }

                    p.vx = (pvx / scale_x).clamp(-1.8, 1.8);
                    p.vy = (pvy / scale_y).clamp(-1.8, 1.8);
                    p.x += p.vx * dt;
                    p.y += p.vy * dt;

                    if p.x < -0.04 { p.x += 1.08; }
                    if p.x > 1.04 { p.x -= 1.08; }
                    if p.y > 1.05 {
                        p.y = -0.04;
                        p.x = hash_frac(i, (self.phase * 13.7).floor() + 0.41);
                        p.vx = 0.0;
                    } else if p.y < -0.08 {
                        p.y = 1.02;
                    }
                }
            }
            // 2 = Inferno (buoyant rising embers & heat sparks fanned fiercely by mouse hover)
            2 => {
                let hearth_bands = if is_tall { 3 } else { 1 };
                for (i, p) in self.particles.iter_mut().enumerate() {
                    p.phase = (p.phase + dt * 5.2) % std::f32::consts::TAU;
                    let target_vx_px = (self.phase * 3.1 + p.phase).sin() * (22.0 + p.size * 18.0);
                    let target_vy_px = -(56.0 + p.size * 78.0);
                    let mut pvx = p.vx * scale_x + (target_vx_px - p.vx * scale_x) * dt * 3.2;
                    let mut pvy = p.vy * scale_y + (target_vy_px - p.vy * scale_y) * dt * 2.6;

                    if self.hover_mix > 0.02 {
                        let dx = (p.x - self.hover_x) * scale_x;
                        let dy = (p.y - self.hover_y) * scale_y;
                        let dist = dx.hypot(dy).max(1.0);
                        if dist < react_r * 1.25 {
                            let f = (1.0 - dist / (react_r * 1.25)).powi(2) * self.hover_mix;
                            let inv = 1.0 / dist;
                            pvx += ((dx * inv) * 140.0 + self.hover_vx * scale_x * 0.72) * f * dt * 5.0;
                            pvy += (-(170.0 + speed_px * 0.25) + self.hover_vy * scale_y * 0.45) * f * dt * 4.2;
                            p.life = (p.life + dt * 0.85 * f).min(1.0);
                        }
                    }

                    p.vx = (pvx / scale_x).clamp(-2.5, 2.5);
                    p.vy = (pvy / scale_y).clamp(-3.2, 1.5);
                    p.x += p.vx * dt;
                    p.y += p.vy * dt;
                    p.life -= dt * (0.52 + (i % 5) as f32 * 0.14);

                    if p.life <= 0.0 || p.y < -0.05 || p.x < -0.08 || p.x > 1.08 {
                        let spawn_near_cursor = active_hover
                            && (speed_px > 55.0 || i % 4 == 0)
                            && self.hover_mix > 0.35;
                        if spawn_near_cursor {
                            let jx = (hash_frac(i, self.phase * 19.3) - 0.5) * (28.0 / scale_x);
                            let jy = (hash_frac(i + 7, self.phase * 17.1) - 0.5) * (22.0 / scale_y);
                            p.x = (self.hover_x + jx).clamp(0.03, 0.97);
                            p.y = (self.hover_y + jy).clamp(0.04, 0.96);
                            p.vx = self.hover_vx * 0.45 + (hash_frac(i, self.phase * 7.7) - 0.5) * (50.0 / scale_x);
                            p.vy = -(70.0 + hash_frac(i, self.phase) * 90.0) / scale_y;
                            p.life = 0.85 + (i % 3) as f32 * 0.06;
                        } else {
                            let band_idx = i % hearth_bands;
                            let base_y = match band_idx {
                                0 => 0.96,
                                1 => 0.64,
                                _ => 0.34,
                            };
                            p.x = hash_frac(i, self.phase * 11.7 + 0.23);
                            p.y = base_y;
                            p.vx = (hash_frac(i, self.phase * 5.1) - 0.5) * (24.0 / scale_x);
                            p.vy = -(52.0 + p.size * 68.0) / scale_y;
                            p.life = 0.92;
                        }
                    }
                }
            }
            // 3 = Water Ripples & Caustics (mouse trails concentric wave rings + glowing plankton)
            3 => {
                self.ripple_cd = (self.ripple_cd - dt).max(0.0);
                self.auto_drop_cd -= dt;

                for rip in self.ripples.iter_mut() {
                    if rip.active {
                        rip.age += dt;
                        if rip.age >= rip.max_age {
                            rip.active = false;
                        }
                    }
                }

                let cur_rc = self.ripple_cursor;
                let mut spawn_ripple = |rx: f32, ry: f32, str_val: f32| {
                    let idx = self.ripple_cursor % SIM_RIPPLE_COUNT;
                    self.ripple_cursor = (self.ripple_cursor + 1) % SIM_RIPPLE_COUNT;
                    self.ripples[idx] = TaskbarRipple {
                        x: rx,
                        y: ry,
                        age: 0.0,
                        max_age: 1.45,
                        strength: str_val,
                        active: true,
                    };
                };

                if active_hover {
                    let moved_px = ((self.hover_x - self.last_ripple_x) * scale_x)
                        .hypot((self.hover_y - self.last_ripple_y) * scale_y);
                    if !prev_hover || (moved_px >= 14.0 && self.ripple_cd <= 0.0) || self.auto_drop_cd <= 0.0 {
                        let s_val = (0.72 + speed_px / 900.0).clamp(0.72, 1.35);
                        spawn_ripple(self.hover_x, self.hover_y, s_val);
                        self.last_ripple_x = self.hover_x;
                        self.last_ripple_y = self.hover_y;
                        self.ripple_cd = 0.11;
                        self.auto_drop_cd = 0.45;
                    }
                } else if self.auto_drop_cd <= 0.0 {
                    let rx = 0.12 + hash_frac(cur_rc + 3, self.phase * 5.7) * 0.76;
                    let ry = 0.25 + hash_frac(cur_rc + 11, self.phase * 3.3) * 0.55;
                    spawn_ripple(rx, ry, 0.58);
                    self.auto_drop_cd = 0.95;
                }

                for p in self.particles.iter_mut() {
                    p.phase = (p.phase + dt * 1.4) % std::f32::consts::TAU;
                    let drift_x = (self.phase * 0.8 + p.phase).sin() * (12.0 / scale_x);
                    let drift_y = (self.phase * 0.6 + p.phase * 1.3).cos() * (8.0 / scale_y);
                    let mut pvx = p.vx + (drift_x - p.vx) * dt * 2.0;
                    let mut pvy = p.vy + (drift_y - p.vy) * dt * 2.0;
                    if self.hover_mix > 0.02 {
                        let dx = (p.x - self.hover_x) * scale_x;
                        let dy = (p.y - self.hover_y) * scale_y;
                        let dist = dx.hypot(dy).max(1.0);
                        if dist < react_r {
                            let f = (1.0 - dist / react_r).powi(2) * self.hover_mix;
                            let inv = 1.0 / dist;
                            pvx += ((dx * inv * 120.0 + self.hover_vx * scale_x * 0.4) * f / scale_x) * dt * 4.0;
                            pvy += ((dy * inv * 120.0 + self.hover_vy * scale_y * 0.4) * f / scale_y) * dt * 4.0;
                        }
                    }
                    p.vx = pvx.clamp(-1.2, 1.2);
                    p.vy = pvy.clamp(-1.2, 1.2);
                    p.x = (p.x + p.vx * dt).clamp(0.02, 0.98);
                    p.y = (p.y + p.vy * dt).clamp(0.06, 0.94);
                }
            }
            // 4 = Sakura Breeze (tumbling petals lifted & swirled by cursor draft)
            4 => {
                for (i, p) in self.particles.iter_mut().enumerate() {
                    p.phase = (p.phase + dt * (2.5 + p.size * 1.2)) % std::f32::consts::TAU;
                    let breeze_x = (18.0 + (p.phase * 0.7).sin() * 14.0) / scale_x;
                    let fall_y = (22.0 + p.size * 14.0) / scale_y;
                    let mut pvx = p.vx + (breeze_x - p.vx) * dt * 2.2;
                    let mut pvy = p.vy + (fall_y - p.vy) * dt * 2.2;
                    if self.hover_mix > 0.02 {
                        let dx = (p.x - self.hover_x) * scale_x;
                        let dy = (p.y - self.hover_y) * scale_y;
                        let dist = dx.hypot(dy).max(1.0);
                        if dist < react_r * 1.2 {
                            let f = (1.0 - dist / (react_r * 1.2)).powi(2) * self.hover_mix;
                            let inv = 1.0 / dist;
                            let nx = dx * inv;
                            let ny = dy * inv;
                            pvx += ((nx * 140.0 - ny * 110.0 + self.hover_vx * scale_x * 0.65) * f / scale_x) * dt * 4.5;
                            pvy += ((ny * 120.0 + nx * 110.0 - 55.0 + self.hover_vy * scale_y * 0.55) * f / scale_y) * dt * 4.5;
                            p.phase += dt * 5.0 * f;
                        }
                    }
                    p.vx = pvx.clamp(-2.0, 2.0);
                    p.vy = pvy.clamp(-2.0, 2.0);
                    p.x += p.vx * dt;
                    p.y += p.vy * dt;
                    if p.x > 1.06 { p.x = -0.05; }
                    if p.x < -0.06 { p.x = 1.05; }
                    if p.y > 1.06 {
                        p.y = -0.05;
                        p.x = hash_frac(i, (self.phase * 7.3).floor() + 0.17);
                    } else if p.y < -0.08 {
                        p.y = 1.03;
                    }
                }
            }
            // 5 = Cosmic Gravity Lens (stardust & comets slingshot into orbits around cursor)
            5 => {
                for (i, p) in self.particles.iter_mut().enumerate() {
                    p.phase = (p.phase + dt * 1.9) % std::f32::consts::TAU;
                    let amb_x = (self.phase * 0.5 + i as f32 * 0.7).cos() * (8.0 / scale_x);
                    let amb_y = (self.phase * 0.4 + i as f32 * 0.9).sin() * (8.0 / scale_y);
                    let mut pvx = p.vx * 0.97 + amb_x * dt * 1.2;
                    let mut pvy = p.vy * 0.97 + amb_y * dt * 1.2;
                    if self.hover_mix > 0.02 {
                        let dx = (self.hover_x - p.x) * scale_x;
                        let dy = (self.hover_y - p.y) * scale_y;
                        let dist = dx.hypot(dy).max(8.0);
                        let lens_r = react_r * 1.75;
                        if dist < lens_r {
                            let f = (1.0 - dist / lens_r) * self.hover_mix;
                            let inv = 1.0 / dist;
                            let nx = dx * inv;
                            let ny = dy * inv;
                            // Gravity pull inward + tangential orbital spin
                            let pull = if dist > 18.0 { 165.0 } else { -110.0 };
                            pvx += ((nx * pull - ny * 175.0) * f / scale_x) * dt * 3.8;
                            pvy += ((ny * pull + nx * 175.0) * f / scale_y) * dt * 3.8;
                        }
                    }
                    p.vx = pvx.clamp(-1.6, 1.6);
                    p.vy = pvy.clamp(-1.6, 1.6);
                    p.x += p.vx * dt;
                    p.y += p.vy * dt;
                    if p.x < 0.01 { p.x = 0.01; p.vx = p.vx.abs() * 0.6; }
                    if p.x > 0.99 { p.x = 0.99; p.vx = -p.vx.abs() * 0.6; }
                    if p.y < 0.02 { p.y = 0.02; p.vy = p.vy.abs() * 0.6; }
                    if p.y > 0.98 { p.y = 0.98; p.vy = -p.vy.abs() * 0.6; }
                }
            }
            // 6 = Plasma Arcs (charged nodes attracted to cursor contact point)
            6 => {
                let is_card = scale_y > 46.0 && scale_x < scale_y * 4.6;
                let max_pull_r = if is_card {
                    scale_x.hypot(scale_y) * 0.82
                } else {
                    react_r * 1.4
                };
                for (i, p) in self.particles.iter_mut().enumerate() {
                    p.phase = (p.phase + dt * 4.5) % std::f32::consts::TAU;
                    let (anchor_x, anchor_y) = if is_card {
                        if i < 24 {
                            // 24 perimeter electrodes slowly drifting around the 2D card rim
                            let ang = (i as f32 / 24.0) * std::f32::consts::TAU + self.phase * 0.28;
                            let (sa, ca) = ang.sin_cos();
                            let rx = 0.46 + 0.03 * (self.phase * 1.9 + i as f32).sin();
                            let ry = 0.44 + 0.03 * (self.phase * 2.1 + i as f32 * 0.7).cos();
                            ((0.5 + ca * rx).clamp(0.03, 0.97), (0.5 + sa * ry).clamp(0.04, 0.96))
                        } else {
                            // 12 interior orbital plasma ions
                            let k = (i - 24) as f32;
                            let ang = (k / 12.0) * std::f32::consts::TAU - self.phase * 0.75;
                            let r_orb = 0.22 + 0.10 * (self.phase * 1.4 + k * 0.9).sin();
                            (
                                (0.5 + ang.cos() * r_orb).clamp(0.08, 0.92),
                                (0.5 + ang.sin() * r_orb).clamp(0.08, 0.92),
                            )
                        }
                    } else {
                        (
                            (i as f32 + 0.5) / SIM_PARTICLE_COUNT as f32,
                            0.5 + (self.phase * 1.8 + i as f32 * 0.65).sin() * 0.28,
                        )
                    };
                    let mut tx_n = anchor_x;
                    let mut ty_n = anchor_y;
                    if self.hover_mix > 0.02 {
                        let dx = (self.hover_x - anchor_x) * scale_x;
                        let dy = (self.hover_y - anchor_y) * scale_y;
                        let dist = dx.hypot(dy);
                        if dist < max_pull_r {
                            let f = (1.0 - dist / max_pull_r).powi(2) * self.hover_mix;
                            let pull_k = if i >= 24 { 0.68 } else { 0.36 };
                            tx_n += (self.hover_x - anchor_x) * f * pull_k;
                            ty_n += (self.hover_y - anchor_y) * f * pull_k;
                        }
                    }
                    p.x += (tx_n - p.x) * (dt * 7.5).min(1.0);
                    p.y += (ty_n - p.y) * (dt * 7.5).min(1.0);
                }
            }
            // 7 = Cupertino Kinetic Wave (1D coupled spring-wave shelf + magnification indicator beads)
            7 => {
                let hover_axis = if is_tall { self.hover_y } else { self.hover_x };
                let hover_v_axis = if is_tall { self.hover_vy } else { self.hover_vx };
                let mut h_prev = [0.0f32; 28];
                for i in 0..28 {
                    h_prev[i] = self.particles[i].y;
                }
                for i in 0..28usize {
                    let nx = i as f32 / 27.0;
                    self.particles[i].x = nx;
                    let left_h = if i > 0 { h_prev[i - 1] } else { h_prev[0] * 0.5 };
                    let right_h = if i + 1 < 28 { h_prev[i + 1] } else { h_prev[27] * 0.5 };
                    let lap = left_h + right_h - 2.0 * h_prev[i];

                    // Gentle ambient dock spotlight wave when idle, smooth parabolic magnification swell on hover
                    let idle_wave = ((self.phase * 1.65 - nx * 6.2).sin() * 0.5 + 0.5).powi(3) * 0.16;
                    let mut target = idle_wave * (1.0 - self.hover_mix * 0.7);
                    if self.hover_mix > 0.02 {
                        let d_ax = (nx - hover_axis).abs();
                        if d_ax < 0.22 {
                            let bell = (1.0 - d_ax / 0.22).powi(2);
                            target += (0.58 + (speed_px / 900.0).min(0.28)) * bell * self.hover_mix;
                        }
                    }
                    let spring_f = (target - h_prev[i]) * 38.0 + lap * 46.0;
                    let wake_imp = if self.hover_mix > 0.05 && (nx - hover_axis).abs() < 0.12 {
                        hover_v_axis * 0.16 * self.hover_mix
                    } else {
                        0.0
                    };
                    let nv = (self.particles[i].vy + spring_f * dt + wake_imp * dt * 12.0) * (1.0 - dt * 5.8).max(0.5);
                    self.particles[i].vy = nv.clamp(-4.5, 4.5);
                    self.particles[i].y = (h_prev[i] + self.particles[i].vy * dt).clamp(-0.35, 0.95);
                }
                for (k, p) in self.particles[28..].iter_mut().enumerate() {
                    let bx = (k as f32 + 1.0) / 9.0;
                    p.x = bx;
                    let d = (bx - hover_axis).abs();
                    let tgt = if self.hover_mix > 0.02 && d < 0.20 {
                        0.28 + 0.72 * (1.0 - d / 0.20) * self.hover_mix
                    } else {
                        0.22 + 0.12 * (self.phase * 2.1 + k as f32 * 0.9).sin().abs()
                    };
                    p.life += (tgt - p.life) * (dt * 10.0).min(1.0);
                }
            }
            // 8 = Liquid Mercury (molten chrome wave ribbon + whipped surface-tension metal droplets)
            8 => {
                let hover_axis = if is_tall { self.hover_y } else { self.hover_x };
                let hover_cross = if is_tall { self.hover_x } else { self.hover_y };
                let mut r_prev = [0.0f32; 20];
                for i in 0..20 {
                    r_prev[i] = self.particles[i].y;
                }
                for i in 0..20usize {
                    let nx = i as f32 / 19.0;
                    self.particles[i].x = nx;
                    let lh = if i > 0 { r_prev[i - 1] } else { 0.0 };
                    let rh = if i + 1 < 20 { r_prev[i + 1] } else { 0.0 };
                    let lap = lh + rh - 2.0 * r_prev[i];
                    let idle = (self.phase * 2.1 + nx * 8.0).sin() * 0.10;
                    let mut tgt = idle;
                    if self.hover_mix > 0.02 {
                        let d_ax = (nx - hover_axis).abs();
                        if d_ax < 0.24 {
                            let f = (1.0 - d_ax / 0.24).powi(2) * self.hover_mix;
                            let push_dir = if hover_cross > 0.5 { -1.0 } else { 1.0 };
                            tgt += push_dir * 0.36 * f + (self.hover_vy - self.hover_vx).clamp(-0.35, 0.35) * f * 0.5;
                        }
                    }
                    let acc = (tgt - r_prev[i]) * 44.0 + lap * 52.0;
                    let nv = (self.particles[i].vy + acc * dt) * (1.0 - dt * 4.5).max(0.5);
                    self.particles[i].vy = nv.clamp(-5.0, 5.0);
                    self.particles[i].y = (r_prev[i] + self.particles[i].vy * dt).clamp(-0.42, 0.42);
                }
                for (k, p) in self.particles[20..].iter_mut().enumerate() {
                    p.phase = (p.phase + dt * 2.4) % std::f32::consts::TAU;
                    let span_pos = (k as f32 + 0.5) / 16.0;
                    let cross_pos = 0.5 + (self.phase * 1.9 + k as f32 * 1.1).sin() * 0.14;
                    let (rest_x, rest_y, kx, ky) = if is_tall {
                        (cross_pos, span_pos, 2.8, 2.2)
                    } else {
                        (span_pos, cross_pos, 2.2, 2.8)
                    };
                    let mut vx = p.vx + (rest_x - p.x) * dt * kx;
                    let mut vy = p.vy + (rest_y - p.y) * dt * ky;
                    if self.hover_mix > 0.02 {
                        let dx = (p.x - self.hover_x) * scale_x;
                        let dy = (p.y - self.hover_y) * scale_y;
                        let dist = dx.hypot(dy).max(1.0);
                        if dist < react_r * 1.15 {
                            let f = (1.0 - dist / (react_r * 1.15)).powi(2) * self.hover_mix;
                            let inv = 1.0 / dist;
                            let whip = 165.0 + speed_px * 0.45;
                            vx += ((dx * inv * whip + self.hover_vx * scale_x * 0.85) * f / scale_x) * dt * 4.8;
                            vy += ((dy * inv * whip + self.hover_vy * scale_y * 0.85) * f / scale_y) * dt * 4.8;
                        }
                    }
                    p.vx = (vx * (1.0 - dt * 2.8).max(0.4)).clamp(-2.8, 2.8);
                    p.vy = (vy * (1.0 - dt * 2.8).max(0.4)).clamp(-2.8, 2.8);
                    p.x += p.vx * dt;
                    p.y += p.vy * dt;
                    let (min_x, max_x, min_y, max_y) = if is_tall {
                        (0.08, 0.92, 0.03, 0.97)
                    } else {
                        (0.03, 0.97, 0.08, 0.92)
                    };
                    if p.x < min_x { p.x = min_x; p.vx = p.vx.abs() * 0.72; }
                    if p.x > max_x { p.x = max_x; p.vx = -p.vx.abs() * 0.72; }
                    if p.y < min_y { p.y = min_y; p.vy = p.vy.abs() * 0.72; }
                    if p.y > max_y { p.y = max_y; p.vy = -p.vy.abs() * 0.72; }
                }
            }
            // 9 = Hydro Fluid Chamber (buoyant 3D water bubbles swirling in cursor eddies + pop ripples)
            9 => {
                self.ripple_cd = (self.ripple_cd - dt).max(0.0);
                for rip in self.ripples.iter_mut() {
                    if rip.active {
                        rip.age += dt;
                        if rip.age >= rip.max_age {
                            rip.active = false;
                        }
                    }
                }
                let mut spawn_pop = |rx: f32, ry: f32, strength: f32| {
                    let idx = self.ripple_cursor % SIM_RIPPLE_COUNT;
                    self.ripple_cursor = (self.ripple_cursor + 1) % SIM_RIPPLE_COUNT;
                    self.ripples[idx] = TaskbarRipple {
                        x: rx,
                        y: ry,
                        age: 0.0,
                        max_age: 0.95,
                        strength,
                        active: true,
                    };
                };
                for (i, p) in self.particles.iter_mut().enumerate() {
                    p.phase = (p.phase + dt * (2.2 + p.size * 1.1)) % std::f32::consts::TAU;
                    let wobble_x = (p.phase * 1.7).sin() * 14.0 / scale_x;
                    let rise_y = -(24.0 + p.size * 28.0) / scale_y;
                    let mut pvx = p.vx + (wobble_x - p.vx) * dt * 2.5;
                    let mut pvy = p.vy + (rise_y - p.vy) * dt * 2.5;
                    if self.hover_mix > 0.02 {
                        let dx = (p.x - self.hover_x) * scale_x;
                        let dy = (p.y - self.hover_y) * scale_y;
                        let dist = dx.hypot(dy).max(1.0);
                        if dist < react_r * 1.25 {
                            let f = (1.0 - dist / (react_r * 1.25)).powi(2) * self.hover_mix;
                            let inv = 1.0 / dist;
                            let nx = dx * inv;
                            let ny = dy * inv;
                            pvx += ((nx * 145.0 - ny * 125.0 + self.hover_vx * scale_x * 0.65) * f / scale_x) * dt * 4.5;
                            pvy += ((ny * 135.0 + nx * 125.0 + self.hover_vy * scale_y * 0.65) * f / scale_y) * dt * 4.5;
                        }
                    }
                    p.vx = pvx.clamp(-2.0, 2.0);
                    p.vy = pvy.clamp(-2.2, 1.5);
                    p.x = (p.x + p.vx * dt).clamp(0.02, 0.98);
                    p.y += p.vy * dt;
                    if p.y < 0.08 {
                        if i % 3 == 0 && self.ripple_cd <= 0.0 {
                            spawn_pop(p.x, 0.12, 0.55 + p.size * 0.25);
                            self.ripple_cd = 0.14;
                        }
                        p.y = 0.96;
                        p.x = hash_frac(i, self.phase * 9.1 + 0.37);
                        p.vx = 0.0;
                    } else if p.y > 1.02 {
                        p.y = 0.95;
                    }
                }
            }
            // 10 = Quantum Collider (twin opposing beams + magnetic cursor focus + collision sparks)
            10 => {
                let focus_x = if self.hover_mix > 0.05 {
                    0.5 + (self.hover_x - 0.5) * self.hover_mix
                } else {
                    0.5 + (self.phase * 0.9).sin() * 0.18
                };
                let focus_y = if self.hover_mix > 0.05 {
                    0.5 + (self.hover_y - 0.5) * self.hover_mix
                } else {
                    0.5
                };
                for (i, p) in self.particles.iter_mut().enumerate() {
                    if i < 24 {
                        let dir = if i < 12 { 1.0 } else { -1.0 };
                        let beam_spd = 2.4 + 1.6 * self.hover_mix + (speed_px / 650.0).min(1.5);
                        p.phase = (p.phase + dir * beam_spd * dt).rem_euclid(std::f32::consts::TAU);
                        let bx = 0.5 + p.phase.cos() * 0.46;
                        let by = 0.5 + p.phase.sin() * 0.34;
                        let pull = if self.hover_mix > 0.02 {
                            let d = ((bx - focus_x) * scale_x).hypot((by - focus_y) * scale_y);
                            (1.0 - (d / (react_r * 1.5)).min(1.0)) * 0.65 * self.hover_mix
                        } else {
                            0.0
                        };
                        p.x = bx + (focus_x - bx) * pull;
                        p.y = by + (focus_y - by) * pull;
                    } else {
                        p.x += p.vx * dt;
                        p.y += p.vy * dt;
                        p.life -= dt * (1.4 + (i % 4) as f32 * 0.25);
                        if p.life <= 0.0 {
                            let ang = hash_frac(i, self.phase * 13.3) * std::f32::consts::TAU;
                            let sp = 0.35 + hash_frac(i + 5, self.phase * 7.1) * (0.85 + 0.65 * self.hover_mix);
                            p.x = focus_x;
                            p.y = focus_y;
                            p.vx = ang.cos() * sp * (scale_y / scale_x).max(0.25);
                            p.vy = ang.sin() * sp;
                            p.life = 0.75 + 0.25 * self.hover_mix;
                        }
                    }
                }
            }
            // 11 = Abyssal Bioluminescence (plankton excited to full luminescence in cursor wake)
            11 => {
                for (i, p) in self.particles.iter_mut().enumerate() {
                    p.phase = (p.phase + dt * (1.5 + p.size * 0.7)) % std::f32::consts::TAU;
                    let drift_x = (self.phase * 0.55 + p.phase).sin() * 12.0 / scale_x;
                    let drift_y = (self.phase * 0.45 + p.phase * 1.4).cos() * 10.0 / scale_y;
                    let mut vx = p.vx + (drift_x - p.vx) * dt * 2.0;
                    let mut vy = p.vy + (drift_y - p.vy) * dt * 2.0;
                    let mut target_glow = 0.22 + 0.14 * (self.phase * 1.4 + i as f32 * 0.5).sin().abs();
                    if self.hover_mix > 0.02 {
                        let dx = (p.x - self.hover_x) * scale_x;
                        let dy = (p.y - self.hover_y) * scale_y;
                        let dist = dx.hypot(dy).max(1.0);
                        if dist < react_r * 1.35 {
                            let f = (1.0 - dist / (react_r * 1.35)).powi(2) * self.hover_mix;
                            let inv = 1.0 / dist;
                            vx += ((dx * inv * 95.0 - dy * inv * 75.0 + self.hover_vx * scale_x * 0.6) * f / scale_x) * dt * 4.0;
                            vy += ((dy * inv * 95.0 + dx * inv * 75.0 + self.hover_vy * scale_y * 0.6) * f / scale_y) * dt * 4.0;
                            target_glow = target_glow.max(0.45 + 0.55 * f);
                        }
                    }
                    p.life = (p.life + (target_glow - p.life) * (dt * 4.5).min(1.0)).clamp(0.14, 1.0);
                    p.vx = vx.clamp(-1.4, 1.4);
                    p.vy = vy.clamp(-1.4, 1.4);
                    p.x = (p.x + p.vx * dt).clamp(0.02, 0.98);
                    p.y = (p.y + p.vy * dt).clamp(0.05, 0.95);
                }
            }
            // 12 = Synthwave Grid Warp (spring-damper trampoline gravity well on 3D perspective wireframe)
            _ => {
                let target_wx = if self.hover_mix > 0.02 {
                    self.hover_x
                } else {
                    0.5 + (self.phase * 0.75).sin() * 0.24
                };
                let target_wy = if self.hover_mix > 0.02 {
                    self.hover_y
                } else {
                    0.52 + (self.phase * 1.25).cos() * 0.14
                };
                let ax = (target_wx - self.particles[0].x) * 42.0;
                let ay = (target_wy - self.particles[0].y) * 42.0;
                self.particles[0].vx = (self.particles[0].vx + ax * dt) * (1.0 - dt * 6.5).max(0.4);
                self.particles[0].vy = (self.particles[0].vy + ay * dt) * (1.0 - dt * 6.5).max(0.4);
                self.particles[0].x = (self.particles[0].x + self.particles[0].vx * dt).clamp(0.02, 0.98);
                self.particles[0].y = (self.particles[0].y + self.particles[0].vy * dt).clamp(0.05, 0.95);

                let target_depth = if self.hover_mix > 0.02 {
                    0.45 + 0.55 * self.hover_mix + (speed_px / 950.0).min(0.4)
                } else {
                    0.26 + 0.12 * (self.phase * 1.8).sin().abs()
                };
                let ad = (target_depth - self.particles[1].y) * 58.0;
                self.particles[1].vy = (self.particles[1].vy + ad * dt) * (1.0 - dt * 4.2).max(0.4);
                self.particles[1].y = (self.particles[1].y + self.particles[1].vy * dt).clamp(-0.25, 1.45);
            }
        }
    }

    pub fn update_from_raw(&mut self, raw: &[f32; 12], dt_sec: f32, allow_ambient_idle: bool) {
        if !self.phase.is_finite() {
            self.phase = 0.0;
        }
        if !self.energy.is_finite() {
            self.energy = 0.0;
        }
        if !self.beat_pulse.is_finite() {
            self.beat_pulse = 0.0;
        }
        let dt = dt_sec.clamp(0.008, 0.12);
        let raw_max = raw
            .iter()
            .copied()
            .filter(|v| v.is_finite())
            .fold(0.0f32, f32::max);
        let bass = (((if raw[0].is_finite() { raw[0] } else { 0.0 })
            + (if raw[1].is_finite() { raw[1] } else { 0.0 })
            + (if raw[2].is_finite() { raw[2] } else { 0.0 }))
            / 3.0)
            .clamp(0.0, 1.0);
        let has_live_audio = raw_max > 0.006 || bass > 0.012;
        let phase_step = dt * (3.8 + self.energy * 4.2);
        let next_phase = (self.phase + phase_step) % (std::f32::consts::TAU * 64.0);
        self.phase = if next_phase.is_finite() { next_phase } else { 0.0 };

        // Frame-rate independent exponential attack & release so 24/30/45/60/90/120 FPS feel identical
        let atk = 1.0 - (-dt * 36.0).exp();
        let rel = 1.0 - (-dt * 11.5).exp();
        let decay_mul = (-dt * 10.5).exp();
        let pk_gravity = 1.15 * dt;

        let mut sum = 0.0f32;
        for i in 0..24usize {
            let p = i as f32 / 23.0;
            if has_live_audio {
                // Smoothly interpolate across the 12 true FFT bands without bass-to-treble smear
                let f = p * 11.0;
                let idx0 = (f.floor() as usize).min(11);
                let idx1 = (idx0 + 1).min(11);
                let frac = f - idx0 as f32;
                let r0 = if raw[idx0].is_finite() { raw[idx0] } else { 0.0 };
                let r1 = if raw[idx1].is_finite() { raw[idx1] } else { 0.0 };
                let lin = (r0 * (1.0 - frac) + r1 * frac).clamp(0.0, 1.0);
                let target = lin.powf(0.84).clamp(0.0, 1.0);

                let prev = if self.bands[i].is_finite() { self.bands[i] } else { 0.0 };
                let alpha = if target > prev { atk } else { rel };
                let next = prev + (target - prev) * alpha;
                self.bands[i] = if next < 0.003 { 0.0 } else { next.clamp(0.0, 1.0) };

                let prev_pk = if self.peaks[i].is_finite() { self.peaks[i] } else { 0.0 };
                if self.bands[i] >= prev_pk {
                    self.peaks[i] = (self.bands[i] + 0.025).min(0.99);
                    self.peak_hold[i] = 0.24; // Hold studio peak cap briefly at crest
                } else if self.peak_hold[i] > 0.0 {
                    self.peak_hold[i] = (self.peak_hold[i] - dt).max(0.0);
                } else {
                    self.peaks[i] = (prev_pk - pk_gravity).max(self.bands[i]);
                }
            } else if allow_ambient_idle {
                let arch = (p * std::f32::consts::FRAC_PI_2).cos().max(0.0).powf(0.55);
                let w1 = (next_phase * 1.25 - p * 6.8).sin() * 0.5 + 0.5;
                let w2 = (next_phase * 0.82 + p * 11.4).cos() * 0.5 + 0.5;
                let beat = (next_phase * 1.9).sin().max(0.0).powi(2) * 0.22;
                let target = (0.16 + (0.58 * (w1 * 0.62 + w2 * 0.38) + beat) * (0.42 + 0.58 * arch))
                    .clamp(0.12, 0.90);

                let prev = if self.bands[i].is_finite() { self.bands[i] } else { 0.0 };
                let alpha = if target > prev { atk * 0.7 } else { rel };
                self.bands[i] = (prev + (target - prev) * alpha).clamp(0.0, 1.0);
                let prev_pk = if self.peaks[i].is_finite() { self.peaks[i] } else { 0.0 };
                if self.bands[i] >= prev_pk {
                    self.peaks[i] = (self.bands[i] + 0.04).min(0.98);
                    self.peak_hold[i] = 0.20;
                } else if self.peak_hold[i] > 0.0 {
                    self.peak_hold[i] = (self.peak_hold[i] - dt).max(0.0);
                } else {
                    self.peaks[i] = (prev_pk - pk_gravity).max(self.bands[i]);
                }
            } else {
                let prev = if self.bands[i].is_finite() { self.bands[i] } else { 0.0 };
                let next = prev * decay_mul;
                self.bands[i] = if next < 0.004 { 0.0 } else { next };
                self.peak_hold[i] = (self.peak_hold[i] - dt).max(0.0);
                let prev_pk = if self.peaks[i].is_finite() { self.peaks[i] } else { 0.0 };
                let next_pk = if self.peak_hold[i] > 0.0 {
                    prev_pk.max(self.bands[i])
                } else {
                    (prev_pk - pk_gravity * 1.35).max(self.bands[i])
                };
                self.peaks[i] = if next_pk < 0.005 { 0.0 } else { next_pk };
            }
            sum += self.bands[i];
        }
        let next_energy = (sum / 24.0).clamp(0.0, 1.0);
        let beat_src = if has_live_audio {
            (bass * 0.72 + raw_max * 0.28).clamp(0.0, 1.0)
        } else if allow_ambient_idle {
            next_energy * 0.75
        } else {
            0.0
        };
        if beat_src > self.beat_pulse {
            self.beat_pulse = self.beat_pulse + (beat_src - self.beat_pulse) * atk;
        } else {
            self.beat_pulse *= (-dt * 8.5).exp();
            if self.beat_pulse < 0.004 {
                self.beat_pulse = 0.0;
            }
        }
        self.energy = next_energy;
    }

    pub fn demo_wave(raw: &[f32; 12], phase: f32) -> Self {
        let phase = if phase.is_finite() { phase } else { 0.0 };
        let mut out = Self::default();
        out.phase = phase;
        let raw_energy: f32 = raw.iter().copied().filter(|v| v.is_finite()).sum::<f32>() / 12.0;
        if raw_energy > 0.012 {
            let mut sum = 0.0f32;
            let mut raw_max = 0.0f32;
            for i in 0..24usize {
                let p = i as f32 / 23.0;
                let f = p * 11.0;
                let idx0 = (f.floor() as usize).min(11);
                let idx1 = (idx0 + 1).min(11);
                let frac = f - idx0 as f32;
                let r0 = if raw[idx0].is_finite() { raw[idx0] } else { 0.0 };
                let r1 = if raw[idx1].is_finite() { raw[idx1] } else { 0.0 };
                let val = (r0 * (1.0 - frac) + r1 * frac).clamp(0.0, 1.0).powf(0.84);
                raw_max = raw_max.max(val);
                out.bands[i] = val;
                out.peaks[i] = (val + 0.04).min(0.98);
                sum += val;
            }
            out.energy = (sum / 24.0).clamp(0.0, 1.0);
            out.beat_pulse = (out.bands[0] + out.bands[1] + out.bands[2]) / 3.0 * 0.72 + raw_max * 0.28;
            return out;
        }
        let mut sum = 0.0f32;
        for i in 0..24usize {
            let p = i as f32 / 23.0;
            let arch = (p * std::f32::consts::FRAC_PI_2).cos().max(0.0).powf(0.55);
            let w1 = (phase * 1.25 - p * 6.8).sin() * 0.5 + 0.5;
            let w2 = (phase * 0.82 + p * 11.4).cos() * 0.5 + 0.5;
            let beat = (phase * 1.9).sin().max(0.0).powi(2) * 0.20;
            let val = (0.18 + (0.60 * (w1 * 0.62 + w2 * 0.38) + beat) * (0.42 + 0.58 * arch))
                .clamp(0.14, 0.94);
            out.bands[i] = val;
            out.peaks[i] = (val + 0.09).min(0.98);
            sum += val;
        }
        out.energy = (sum / 24.0).clamp(0.0, 1.0);
        out.beat_pulse = out.energy * 0.75;
        out
    }

    pub fn has_energy(&self) -> bool {
        (self.energy.is_finite() && self.energy > 0.006)
            || self.bands.iter().any(|&v| v.is_finite() && v > 0.006)
            || self.peaks.iter().any(|&v| v.is_finite() && v > 0.006)
    }
}

const WCA_ACCENT_POLICY: u32 = 19;
const ACCENT_DISABLED: u32 = 0;
const ACCENT_ENABLE_ACRYLICBLURBEHIND: u32 = 4;

const DWMWA_COLOR_DEFAULT_U32: u32 = 0xFFFFFFFF;
const DWMWA_COLOR_NONE_U32: u32 = 0xFFFFFFFE;
const DWMWA_WINDOW_CORNER_PREFERENCE_ATTR: DWMWINDOWATTRIBUTE = DWMWINDOWATTRIBUTE(33);
const DWMWCP_DONOTROUND_U32: u32 = 1;
const DWMWCP_ROUND_U32: u32 = 2;
const MAX_BLUR_PILLS: usize = 6;

const WM_APP_TAP_APPLY: u32 = 0x0400 + 77;
const WM_APP_TAP_DETACH: u32 = 0x0400 + 78;

const CLSID_AUTUMN_XAML_TAP: GUID = GUID::from_u128(0x9b4b6b71_3f5d_4b48_9b72_5c8312a0f001);

#[repr(C)]
#[derive(Clone, Copy)]
struct TapSharedConfig {
    version: u32,
    active: u32,
    tint_argb: u32,
    border_argb: u32,
}

#[repr(C)]
struct AccentPolicy {
    accent_state: u32,
    accent_flags: u32,
    gradient_color: u32,
    animation_id: u32,
}

#[repr(C)]
struct WindowCompositionAttribData {
    attrib: u32,
    pv_data: *mut c_void,
    cb_data: usize,
}

type SetWindowCompositionAttributeFn =
    unsafe extern "system" fn(HWND, *mut WindowCompositionAttribData) -> BOOL;

type InitializeXamlDiagnosticsExFn = unsafe extern "system" fn(
    PCWSTR,
    u32,
    PCWSTR,
    PCWSTR,
    GUID,
    PCWSTR,
) -> HRESULT;

#[derive(Clone, Default)]
pub struct TapDiagSnapshot {
    pub attempted: bool,
    pub last_hr: Option<i32>,
    pub last_stage_mode: String,
    pub last_dll_path: String,
    pub last_acl_status: String,
}

struct ComMtaGuard(bool);

impl ComMtaGuard {
    fn new() -> Self {
        let ok = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED).is_ok() };
        Self(ok)
    }
}

impl Drop for ComMtaGuard {
    fn drop(&mut self) {
        if self.0 {
            unsafe {
                CoUninitialize();
            }
        }
    }
}

struct TapBridge {
    h_map: HANDLE,
    view: MEMORY_MAPPED_VIEW_ADDRESS,
    msg_wnd: HWND,
    last_explorer_pid: u32,
    last_active: Option<bool>,
    last_connect_try: Option<Instant>,
    connect_tries: u32,
    notify_hwnd: isize,
    notify_tap_msg: u32,
    connecting: Arc<AtomicBool>,
    diag: Arc<Mutex<TapDiagSnapshot>>,
    pub tap_ok: bool,
    pub needs_explorer_restart: bool,
}

impl TapBridge {
    fn new(notify_hwnd: HWND, notify_tap_msg: u32) -> Self {
        let mut h_map = HANDLE::default();
        let mut view = MEMORY_MAPPED_VIEW_ADDRESS::default();
        let mut sa = SECURITY_ATTRIBUTES::default();
        let mut sd = SECURITY_DESCRIPTOR::default();
        let mut sa_ptr: Option<*const SECURITY_ATTRIBUTES> = None;
        unsafe {
            if InitializeSecurityDescriptor(
                PSECURITY_DESCRIPTOR(&mut sd as *mut _ as *mut _),
                1,
            )
            .is_ok()
            {
                // NULL DACL explicitly grants read/write access across all integrity levels
                // (Medium explorer.exe, Low/AppContainer, and High Administrator).
                if SetSecurityDescriptorDacl(
                    PSECURITY_DESCRIPTOR(&mut sd as *mut _ as *mut _),
                    true,
                    None,
                    false,
                )
                .is_ok()
                {
                    sa.nLength = size_of::<SECURITY_ATTRIBUTES>() as u32;
                    sa.lpSecurityDescriptor = &mut sd as *mut _ as *mut _;
                    sa.bInheritHandle = false.into();
                    sa_ptr = Some(&sa);
                }
            }

            if let Ok(h) = CreateFileMappingW(
                INVALID_HANDLE_VALUE,
                sa_ptr,
                PAGE_READWRITE,
                0,
                size_of::<TapSharedConfig>() as u32,
                w!("Local\\AutumnTaskbarTapCfg"),
            ) {
                h_map = h;
                view = MapViewOfFile(h_map, FILE_MAP_ALL_ACCESS, 0, 0, size_of::<TapSharedConfig>());
                if !view.Value.is_null() {
                    *(view.Value as *mut TapSharedConfig) = TapSharedConfig {
                        version: 1,
                        active: 0,
                        tint_argb: 0x00000000,
                        border_argb: 0,
                    };
                }
            }
        }
        Self {
            h_map,
            view,
            msg_wnd: HWND::default(),
            last_explorer_pid: 0,
            last_active: None,
            last_connect_try: None,
            connect_tries: 0,
            notify_hwnd: notify_hwnd.0 as isize,
            notify_tap_msg,
            connecting: Arc::new(AtomicBool::new(false)),
            diag: Arc::new(Mutex::new(TapDiagSnapshot::default())),
            tap_ok: false,
            needs_explorer_restart: false,
        }
    }

    fn has_attempted_connect(&self) -> bool {
        self.diag.lock().map(|d| d.attempted).unwrap_or(false)
    }

    fn diag_snapshot(&self) -> TapDiagSnapshot {
        self.diag.lock().map(|d| d.clone()).unwrap_or_default()
    }

    fn write_cfg(&self, active: bool, tint_argb: u32) {
        if !self.view.Value.is_null() {
            unsafe {
                *(self.view.Value as *mut TapSharedConfig) = TapSharedConfig {
                    version: 1,
                    active: if active { 1 } else { 0 },
                    tint_argb,
                    border_argb: 0,
                };
            }
        }
    }

    fn find_existing_msg_wnd(exp_pid: u32) -> Option<HWND> {
        unsafe {
            let mut cur = HWND::default();
            loop {
                cur = FindWindowExW(
                    Some(HWND_MESSAGE),
                    Some(cur),
                    w!("AutumnTapMsgWnd"),
                    PCWSTR::null(),
                )
                .unwrap_or_default();
                if cur.0.is_null() {
                    break;
                }
                let mut wpid = 0u32;
                GetWindowThreadProcessId(cur, Some(&mut wpid));
                if wpid == exp_pid {
                    return Some(cur);
                }
            }
        }
        None
    }

    fn ensure_connected(&mut self, taskbar_hwnd: HWND) -> bool {
        unsafe {
            let mut exp_pid = 0u32;
            GetWindowThreadProcessId(taskbar_hwnd, Some(&mut exp_pid));
            if exp_pid == 0 {
                return false;
            }

            if !self.msg_wnd.0.is_null()
                && IsWindow(Some(self.msg_wnd)).as_bool()
                && self.last_explorer_pid == exp_pid
            {
                self.tap_ok = true;
                return true;
            }
            if self.last_explorer_pid != exp_pid {
                self.last_connect_try = None;
                self.connect_tries = 0;
                self.last_active = None;
            }
            self.msg_wnd = HWND::default();

            if let Some(existing) = Self::find_existing_msg_wnd(exp_pid) {
                self.msg_wnd = existing;
                self.last_explorer_pid = exp_pid;
                self.connect_tries = 0;
                self.last_active = None;
                self.tap_ok = true;
                if let Ok(mut d) = self.diag.lock() {
                    d.attempted = true;
                    if d.last_hr.is_none() {
                        d.last_hr = Some(0);
                    }
                    if d.last_stage_mode.is_empty() {
                        d.last_stage_mode = "attached".to_string();
                    }
                }
                return true;
            }

            if self.connecting.load(Ordering::Relaxed) {
                return false;
            }
            if let Some(last_t) = self.last_connect_try {
                let cooldown = if self.connect_tries < 3 {
                    1200
                } else if self.connect_tries < 6 {
                    6000
                } else {
                    30_000
                };
                if last_t.elapsed() < Duration::from_millis(cooldown) {
                    return false;
                }
            }
            self.connect_tries = self.connect_tries.saturating_add(1);
            self.last_connect_try = Some(Instant::now());
            self.last_explorer_pid = exp_pid;

            // Locate AutumnXamlTap.dll next to AutumnDeck.exe
            let mut exe_buf = [0u16; 1024];
            let n = GetModuleFileNameW(None, &mut exe_buf) as usize;
            let exe_path = PathBuf::from(String::from_utf16_lossy(&exe_buf[..n]));
            let Some(dir) = exe_path.parent() else {
                return false;
            };
            let dll_path = dir.join("AutumnXamlTap.dll");
            if !dll_path.exists() {
                let first_miss = self
                    .diag
                    .lock()
                    .map(|mut d| {
                        let first = !d.attempted;
                        d.attempted = true;
                        d.last_stage_mode = "missing_dll".to_string();
                        d.last_dll_path = dll_path.to_string_lossy().into_owned();
                        d.last_acl_status = "MISSING".to_string();
                        first
                    })
                    .unwrap_or(false);
                if first_miss {
                    crate::diagnostics::record_event(
                        "TAP",
                        &format!("missing sibling DLL at {}", dll_path.display()),
                    );
                }
                return false;
            }

            // Connect XAML Diagnostics on a background thread so cold start & bootup stay instant.
            self.connecting.store(true, Ordering::Relaxed);
            let conn_flag = Arc::clone(&self.connecting);
            let diag_arc = Arc::clone(&self.diag);
            let notify_raw = self.notify_hwnd;
            let notify_msg = self.notify_tap_msg;
            std::thread::spawn(move || {
                let _com = ComMtaGuard::new();
                let candidates = build_tap_injection_candidates(&dll_path);
                let mut ok_hwnd = false;
                let mut recorded_hr: Option<i32> = None;
                let mut recorded_mode = String::from("unattempted");
                let mut recorded_path = dll_path.to_string_lossy().into_owned();
                let mut recorded_acl = String::from("UNKNOWN");

                match LoadLibraryW(w!("Windows.UI.Xaml.dll")) {
                    Ok(xaml_mod) => {
                        if let Some(proc_addr) = GetProcAddress(
                            xaml_mod,
                            windows::core::s!("InitializeXamlDiagnosticsEx"),
                        ) {
                            let init_fn: InitializeXamlDiagnosticsExFn =
                                std::mem::transmute(proc_addr);
                            for (mode, cand_path, acl_status) in &candidates {
                                recorded_mode = mode.clone();
                                recorded_path = cand_path.to_string_lossy().into_owned();
                                recorded_acl = acl_status.clone();
                                let dll_w = to_wide(&recorded_path);
                                let (connected, hr_opt) =
                                    try_init_xaml_diagnostics(init_fn, exp_pid, &dll_w);
                                if let Some(hr) = hr_opt {
                                    recorded_hr = Some(hr.0);
                                }
                                if connected {
                                    ok_hwnd = true;
                                    recorded_hr = Some(0);
                                    break;
                                }
                            }
                        } else {
                            recorded_mode = "no_xaml_diag_proc".to_string();
                        }
                    }
                    Err(e) => {
                        recorded_mode = "load_xaml_dll_failed".to_string();
                        recorded_hr = Some(e.code().0);
                    }
                }

                let (first_done, hr_changed) = diag_arc
                    .lock()
                    .map(|mut d| {
                        let first = !d.attempted;
                        let changed = d.last_hr != recorded_hr || d.last_stage_mode != recorded_mode;
                        d.attempted = true;
                        d.last_hr = recorded_hr;
                        d.last_stage_mode = recorded_mode.clone();
                        d.last_dll_path = recorded_path.clone();
                        d.last_acl_status = recorded_acl.clone();
                        (first, changed)
                    })
                    .unwrap_or((false, false));

                if ok_hwnd || first_done || hr_changed {
                    let hr_hex = recorded_hr
                        .map(|h| format!("0x{:08X}", h as u32))
                        .unwrap_or_else(|| "none".to_string());
                    crate::diagnostics::record_event(
                        "TAP",
                        &format!(
                            "connected={} mode={} hr={} acl={} dll={}",
                            ok_hwnd, recorded_mode, hr_hex, recorded_acl, recorded_path
                        ),
                    );
                }

                conn_flag.store(false, Ordering::Relaxed);
                if (ok_hwnd || first_done) && notify_raw != 0 && notify_msg != 0 {
                    let _ = PostMessageW(
                        Some(HWND(notify_raw as *mut c_void)),
                        notify_msg,
                        WPARAM(if ok_hwnd { 1 } else { 0 }),
                        LPARAM(0),
                    );
                }
            });
        }
        false
    }

    fn apply(&mut self, taskbar_hwnd: HWND, active: bool, force: bool) {
        self.write_cfg(active, 0x00000000);
        if !active && self.msg_wnd.0.is_null() {
            return;
        }
        if !self.ensure_connected(taskbar_hwnd) {
            return;
        }
        if !force && self.last_active == Some(active) {
            return;
        }
        self.last_active = Some(active);
        unsafe {
            let mut res = 0usize;
            if active {
                let _ = SendMessageTimeoutW(
                    self.msg_wnd,
                    WM_APP_TAP_APPLY,
                    WPARAM(1),
                    LPARAM(0),
                    SMTO_ABORTIFHUNG,
                    150,
                    Some(&mut res),
                );
            } else {
                // Synchronous bounded detach so Explorer finishes clearing BackgroundFill before exit.
                let ok = SendMessageTimeoutW(
                    self.msg_wnd,
                    WM_APP_TAP_DETACH,
                    WPARAM(0),
                    LPARAM(0),
                    SMTO_ABORTIFHUNG,
                    150,
                    Some(&mut res),
                );
                if (ok.0 == 0 && self.tap_ok) || res == 2 {
                    self.needs_explorer_restart = true;
                }
            }
            let _ = InvalidateRect(Some(taskbar_hwnd), None, false);
        }
    }
}

impl Drop for TapBridge {
    fn drop(&mut self) {
        unsafe {
            if !self.view.Value.is_null() {
                let _ = UnmapViewOfFile(self.view);
            }
            if !self.h_map.0.is_null() {
                let _ = CloseHandle(self.h_map);
            }
        }
    }
}

struct DibSurface {
    mem_dc: HDC,
    bitmap: HBITMAP,
    old_obj: HGDIOBJ,
    width: i32,
    height: i32,
}

impl DibSurface {
    fn new() -> Self {
        Self {
            mem_dc: HDC::default(),
            bitmap: HBITMAP::default(),
            old_obj: HGDIOBJ::default(),
            width: 0,
            height: 0,
        }
    }

    fn ensure_size(&mut self, w: i32, h: i32) -> bool {
        let w = w.max(2);
        let h = h.max(2);
        if !self.mem_dc.0.is_null() && self.width == w && self.height == h {
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
            self.width = w;
            self.height = h;
            true
        }
    }

    fn release(&mut self) {
        unsafe {
            if !self.mem_dc.0.is_null() {
                if !self.old_obj.0.is_null() {
                    let _ = SelectObject(self.mem_dc, self.old_obj);
                }
                if !self.bitmap.0.is_null() {
                    let _ = DeleteObject(HGDIOBJ(self.bitmap.0));
                }
                let _ = DeleteDC(self.mem_dc);
            }
        }
        self.mem_dc = HDC::default();
        self.bitmap = HBITMAP::default();
        self.old_obj = HGDIOBJ::default();
        self.width = 0;
        self.height = 0;
    }
}

impl Drop for DibSurface {
    fn drop(&mut self) {
        self.release();
    }
}

pub fn register_styler_classes(
    hinst: HINSTANCE,
    wnd_proc: unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT,
) {
    unsafe {
        let cur = LoadCursorW(None, IDC_ARROW).unwrap_or_default();
        for cls in [w!("AutumnDeckGlassWnd"), w!("AutumnDeckBlurWnd")] {
            let mut wc = WNDCLASSEXW::default();
            wc.cbSize = size_of::<WNDCLASSEXW>() as u32;
            wc.lpfnWndProc = Some(wnd_proc);
            wc.hInstance = hinst;
            wc.hCursor = cur;
            wc.lpszClassName = cls;
            let _ = RegisterClassExW(&wc);
        }
    }
}

struct UiaJob {
    gen: u64,
    taskbar_hwnd: isize,
    bar_rect: RECT,
    u: f32,
    pill_mode: PillMode,
    wants_icons: bool,
}

pub struct UiaResult {
    pub gen: u64,
    pub taskbar_hwnd: isize,
    pub bar_rect: RECT,
    pub pill_mode: PillMode,
    pub pills: Vec<RECT>,
    pub icons: Vec<RECT>,
}

struct UiaWorker {
    tx: Sender<UiaJob>,
    results: Arc<Mutex<Vec<UiaResult>>>,
    gen_counter: Arc<AtomicU64>,
}

impl UiaWorker {
    fn new(notify_hwnd: HWND, notify_msg: u32) -> Self {
        let (tx, rx): (Sender<UiaJob>, Receiver<UiaJob>) = mpsc::channel();
        let results = Arc::new(Mutex::new(Vec::<UiaResult>::new()));
        let gen_counter = Arc::new(AtomicU64::new(0));

        let results_bg = Arc::clone(&results);
        let notify_raw = notify_hwnd.0 as isize;

        std::thread::spawn(move || unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            let ctx = build_uia_context();

            while let Ok(first_job) = rx.recv() {
                // Coalesce burst requests within a single deferred frame (35ms window) per taskbar HWND
                // so Explorer's UIA subtree is only walked once per layout change burst.
                let mut batch: Vec<UiaJob> = vec![first_job];
                let coalesce_deadline = Instant::now() + Duration::from_millis(35);
                loop {
                    let rem = coalesce_deadline.saturating_duration_since(Instant::now());
                    if rem.is_zero() {
                        break;
                    }
                    match rx.recv_timeout(rem) {
                        Ok(newer) => {
                            if let Some(pos) = batch
                                .iter()
                                .position(|j| j.taskbar_hwnd == newer.taskbar_hwnd)
                            {
                                batch[pos] = newer;
                            } else {
                                batch.push(newer);
                            }
                        }
                        Err(_) => break,
                    }
                }
                while let Ok(newer) = rx.try_recv() {
                    if let Some(pos) = batch
                        .iter()
                        .position(|j| j.taskbar_hwnd == newer.taskbar_hwnd)
                    {
                        batch[pos] = newer;
                    } else {
                        batch.push(newer);
                    }
                }
                for job in batch {
                    let is_pill_mode =
                        matches!(job.pill_mode, PillMode::SplitPills | PillMode::UnifiedPill);
                    if !is_pill_mode && !job.wants_icons {
                        continue;
                    }
                    let (pills, icons) = if let Some((ref uia, ref cond, ref cache)) = ctx {
                        measure_pills_cached(
                            uia,
                            cond,
                            cache,
                            HWND(job.taskbar_hwnd as *mut c_void),
                            job.bar_rect,
                            job.u,
                            job.pill_mode,
                        )
                    } else {
                        (vec![job.bar_rect], Vec::new())
                    };
                    let pills = if is_pill_mode {
                        pills
                    } else {
                        vec![job.bar_rect]
                    };

                    if let Ok(mut list) = results_bg.lock() {
                        let res = UiaResult {
                            gen: job.gen,
                            taskbar_hwnd: job.taskbar_hwnd,
                            bar_rect: job.bar_rect,
                            pill_mode: job.pill_mode,
                            pills,
                            icons,
                        };
                        if let Some(pos) = list.iter().position(|r| r.taskbar_hwnd == job.taskbar_hwnd) {
                            list[pos] = res;
                        } else {
                            list.push(res);
                        }
                    }
                }
                if notify_raw != 0 {
                    let _ = PostMessageW(
                        Some(HWND(notify_raw as *mut c_void)),
                        notify_msg,
                        WPARAM(0),
                        LPARAM(0),
                    );
                }
            }
            CoUninitialize();
        });

        Self {
            tx,
            results,
            gen_counter,
        }
    }

    fn request(
        &self,
        taskbar_hwnd: HWND,
        bar_rect: RECT,
        u: f32,
        pill_mode: PillMode,
        wants_icons: bool,
    ) -> u64 {
        let gen = self.gen_counter.fetch_add(1, Ordering::Relaxed) + 1;
        let _ = self.tx.send(UiaJob {
            gen,
            taskbar_hwnd: taskbar_hwnd.0 as isize,
            bar_rect,
            u,
            pill_mode,
            wants_icons,
        });
        gen
    }

    fn take_all(&self) -> Vec<UiaResult> {
        self.results
            .lock()
            .ok()
            .map(|mut list| std::mem::take(&mut *list))
            .unwrap_or_default()
    }
}

fn build_uia_context() -> Option<(IUIAutomation, IUIAutomationCondition, IUIAutomationCacheRequest)> {
    unsafe {
        let uia: IUIAutomation =
            CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER).ok()?;
        let btn_cond = uia
            .CreatePropertyCondition(
                UIA_ControlTypePropertyId,
                &VARIANT::from(UIA_ButtonControlTypeId.0),
            )
            .ok()?;
        let cache = uia.CreateCacheRequest().ok()?;
        let _ = cache.AddProperty(UIA_BoundingRectanglePropertyId);
        let _ = cache.AddProperty(UIA_AutomationIdPropertyId);
        let _ = cache.AddProperty(UIA_ClassNamePropertyId);
        let _ = cache.SetTreeScope(TreeScope_Element);
        Some((uia, btn_cond, cache))
    }
}

fn create_dc_render_target(factory: &ID2D1Factory) -> Option<ID2D1DCRenderTarget> {
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
    unsafe { factory.CreateDCRenderTarget(&props).ok() }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct StyleSig {
    pill_mode: u8,
    surface_mode: u8,
    tint_alpha: u8,
    tb_theme: u8,
    amber_border: bool,
    corner_radius: i32,
    tb_viz_style: u8,
    tb_viz_opacity: u8,
    tb_viz_mirror: bool,
    tb_viz_color_mode: u8,
    tb_viz_accent_rgb: u32,
    tb_show_backdrop: bool,
    tb_show_rim: bool,
    tb_show_fx: bool,
    accent: (u8, u8, u8),
    accent_deep: (u8, u8, u8),
    tap_connected: bool,
    sys_light: bool,
}

struct TrayOverlayState {
    hinst: HINSTANCE,
    glass_hwnd: HWND,
    blur_hwnds: Vec<HWND>,
    surface: DibSurface,
    last_tray_hwnd: HWND,
    glass_visible: bool,
    blur_visible: bool,
    swca_cleared: bool,
    border_stripped: Option<bool>,
    last_bar_rect: RECT,
    last_pills: Vec<RECT>,
    last_icons: Vec<RECT>,
    last_blur_rects: Vec<RECT>,
    last_blur_rounded: Option<bool>,
    last_blur_accent_key: Option<(u8, u8, u8, u8)>,
    last_glass_win_rect: RECT,
    last_style_sig: Option<StyleSig>,
    last_pill_mode: Option<PillMode>,
    last_uia_req_time: Option<Instant>,
    last_applied_uia_gen: u64,
    last_pill_count: usize,
    refreshed_dwm_hwnd: HWND,
    last_dwm_rect: RECT,
    pending_dwm_settle: bool,
}

fn create_glass_hwnd(hinst: HINSTANCE) -> HWND {
    unsafe {
        CreateWindowExW(
            WS_EX_TOPMOST
                | WS_EX_LAYERED
                | WS_EX_TRANSPARENT
                | WS_EX_TOOLWINDOW
                | WS_EX_NOACTIVATE,
            w!("AutumnDeckGlassWnd"),
            w!("AutumnDeck Glass"),
            WS_POPUP,
            0,
            0,
            16,
            16,
            None,
            None,
            Some(hinst),
            None,
        )
        .unwrap_or_default()
    }
}

impl TrayOverlayState {
    fn new(hinst: HINSTANCE) -> Self {
        let glass_hwnd = create_glass_hwnd(hinst);
        let mut blur_hwnds = Vec::with_capacity(2);
        let b0 = create_blur_hwnd(hinst);
        if !b0.0.is_null() {
            blur_hwnds.push(b0);
        }
        Self {
            hinst,
            glass_hwnd,
            blur_hwnds,
            surface: DibSurface::new(),
            last_tray_hwnd: HWND::default(),
            glass_visible: false,
            blur_visible: false,
            swca_cleared: false,
            border_stripped: None,
            last_bar_rect: RECT::default(),
            last_pills: Vec::new(),
            last_icons: Vec::new(),
            last_blur_rects: Vec::new(),
            last_blur_rounded: None,
            last_blur_accent_key: None,
            last_glass_win_rect: RECT::default(),
            last_style_sig: None,
            last_pill_mode: None,
            last_uia_req_time: None,
            last_applied_uia_gen: 0,
            last_pill_count: 0,
            refreshed_dwm_hwnd: HWND::default(),
            last_dwm_rect: RECT::default(),
            pending_dwm_settle: false,
        }
    }

    fn hide_overlays(&mut self) {
        unsafe {
            if !self.glass_hwnd.0.is_null() && IsWindowVisible(self.glass_hwnd).as_bool() {
                let _ = ShowWindow(self.glass_hwnd, SW_HIDE);
            }
            for &hw in &self.blur_hwnds {
                if !hw.0.is_null() && IsWindowVisible(hw).as_bool() {
                    let _ = ShowWindow(hw, SW_HIDE);
                }
            }
        }
        self.glass_visible = false;
        self.blur_visible = false;
    }

    fn destroy(&mut self) {
        self.hide_overlays();
        unsafe {
            if !self.glass_hwnd.0.is_null() {
                let _ = DestroyWindow(self.glass_hwnd);
                self.glass_hwnd = HWND::default();
            }
            for hw in self.blur_hwnds.drain(..) {
                if !hw.0.is_null() {
                    let _ = DestroyWindow(hw);
                }
            }
        }
        self.surface.release();
    }

    fn invalidate_cache(&mut self) {
        self.last_style_sig = None;
        self.last_pills.clear();
        self.last_icons.clear();
        self.last_blur_rects.clear();
        self.last_blur_rounded = None;
        self.last_blur_accent_key = None;
        self.last_uia_req_time = None;
        self.swca_cleared = false;
        self.refreshed_dwm_hwnd = HWND::default();
        self.last_dwm_rect = RECT::default();
        self.pending_dwm_settle = true;
    }

    fn invalidate_for_layout_change(&mut self) {
        self.last_style_sig = None;
        self.last_uia_req_time = None;
        self.swca_cleared = false;
        self.pending_dwm_settle = true;
    }

    fn invalidate_style_only(&mut self) {
        self.last_style_sig = None;
    }

    fn is_glass_or_blur_hwnd(&self, hwnd: HWND) -> (bool, bool) {
        if hwnd == self.glass_hwnd && !hwnd.0.is_null() {
            return (true, false);
        }
        if !hwnd.0.is_null() && self.blur_hwnds.contains(&hwnd) {
            return (false, true);
        }
        (false, false)
    }
}

pub struct TaskbarStyler {
    hinst: HINSTANCE,
    primary: TrayOverlayState,
    secondaries: Vec<TrayOverlayState>,
    tap: TapBridge,
    uia_worker: UiaWorker,
    swca_fn: Option<SetWindowCompositionAttributeFn>,
    d2d_factory: Option<ID2D1Factory>,
    dc_rt: Option<ID2D1DCRenderTarget>,

    pub last_pill_count: usize,
    pub last_bar_rect_str: String,
    pub tap_connected: bool,
    styled_any_hwnd: bool,
    viz: VizSnapshot,
    last_viz_tick: Instant,
}

fn create_blur_hwnd(hinst: HINSTANCE) -> HWND {
    unsafe {
        let hw = CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TRANSPARENT | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            w!("AutumnDeckBlurWnd"),
            w!("AutumnDeck Blur"),
            WS_POPUP,
            0,
            0,
            16,
            16,
            None,
            None,
            Some(hinst),
            None,
        )
        .unwrap_or_default();
        if !hw.0.is_null() {
            let margins = MARGINS {
                cxLeftWidth: -1,
                cxRightWidth: -1,
                cyTopHeight: -1,
                cyBottomHeight: -1,
            };
            let _ = DwmExtendFrameIntoClientArea(hw, &margins);
            let _ = DwmSetWindowAttribute(
                hw,
                DWMWA_BORDER_COLOR,
                (&DWMWA_COLOR_NONE_U32 as *const u32) as *const _,
                4,
            );
        }
        hw
    }
}

impl TaskbarStyler {
    pub fn new(
        hinst: HINSTANCE,
        notify_hwnd: HWND,
        wm_pills_ready: u32,
        wm_tap_ready: u32,
    ) -> Self {
        ensure_windows_transparency_enabled();

        let swca_fn = unsafe {
            GetModuleHandleW(w!("user32.dll"))
                .ok()
                .and_then(|hmod| {
                    GetProcAddress(hmod, windows::core::s!("SetWindowCompositionAttribute"))
                })
                .map(|addr| std::mem::transmute::<_, SetWindowCompositionAttributeFn>(addr))
        };

        let d2d_factory: Option<ID2D1Factory> =
            unsafe { D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None).ok() };
        let dc_rt = d2d_factory.as_ref().and_then(create_dc_render_target);
        let uia_worker = UiaWorker::new(notify_hwnd, wm_pills_ready);

        let me = Self {
            hinst,
            primary: TrayOverlayState::new(hinst),
            secondaries: Vec::new(),
            tap: TapBridge::new(notify_hwnd, wm_tap_ready),
            uia_worker,
            swca_fn,
            d2d_factory,
            dc_rt,
            last_pill_count: 0,
            last_bar_rect_str: String::from("(0,0,0,0)"),
            tap_connected: false,
            styled_any_hwnd: false,
            viz: VizSnapshot::default(),
            last_viz_tick: Instant::now(),
        };
        me.clear_any_legacy_region();
        me
    }

    pub fn is_glass_or_blur_hwnd(&self, hwnd: HWND) -> (bool, bool) {
        let p = self.primary.is_glass_or_blur_hwnd(hwnd);
        if p.0 || p.1 {
            return p;
        }
        for sec in &self.secondaries {
            let s = sec.is_glass_or_blur_hwnd(hwnd);
            if s.0 || s.1 {
                return s;
            }
        }
        (false, false)
    }

    pub fn tracked_tray_hwnd(&self) -> HWND {
        self.primary.last_tray_hwnd
    }

    pub fn bar_geometry_changed(&self) -> bool {
        let trays = query_all_tray_geometries();
        let Some((hw, bar_rect, _)) = trays.first().copied() else {
            return false;
        };
        if hw != self.primary.last_tray_hwnd || bar_rect != self.primary.last_bar_rect {
            return true;
        }
        let sec_trays = &trays[1..];
        if sec_trays.len() != self.secondaries.len() {
            return true;
        }
        sec_trays.iter().zip(self.secondaries.iter()).any(|((shw, srect, _), ov)| {
            *shw != ov.last_tray_hwnd || *srect != ov.last_bar_rect
        })
    }

    pub fn clear_any_legacy_region(&self) {
        unsafe {
            for hw in crate::taskbar_hide::TaskbarHider::collect_taskbar_hwnds() {
                if !hw.0.is_null() {
                    let _ = SetWindowRgn(hw, None, true);
                }
            }
        }
    }

    /// Clears the shared-memory tap override (`active = 0`) without sending
    /// cross-process `SendMessageTimeoutW` calls to `explorer.exe` (safe on OS shutdown).
    pub fn clear_shared_tap_only(&mut self) {
        self.tap.write_cfg(false, 0x00000000);
        self.tap.last_active = None;
    }

    pub fn hide_overlays(&mut self) {
        self.viz = VizSnapshot::default();
        self.primary.hide_overlays();
        for sec in &mut self.secondaries {
            sec.hide_overlays();
        }
    }

    pub fn invalidate_cache(&mut self) {
        self.primary.invalidate_cache();
        for sec in &mut self.secondaries {
            sec.invalidate_cache();
        }
        self.tap.last_active = None;
    }

    pub fn invalidate_for_layout_change(&mut self) {
        self.primary.invalidate_for_layout_change();
        for sec in &mut self.secondaries {
            sec.invalidate_for_layout_change();
        }
        self.tap.last_active = None;
    }

    pub fn invalidate_style_only(&mut self) {
        self.primary.invalidate_style_only();
        for sec in &mut self.secondaries {
            sec.invalidate_style_only();
        }
    }

    pub fn has_dwm_refreshed(&self, cfg: &AppConfig) -> bool {
        if !wants_overlay(cfg) {
            return true;
        }
        !self.primary.last_tray_hwnd.0.is_null()
            && self.primary.refreshed_dwm_hwnd == self.primary.last_tray_hwnd
    }

    pub fn is_tap_connecting(&self) -> bool {
        self.tap.connecting.load(Ordering::Relaxed)
    }

    pub fn tap_diag_snapshot(&self) -> TapDiagSnapshot {
        self.tap.diag_snapshot()
    }

    pub fn is_tap_fallback_active(&self, cfg: &AppConfig) -> bool {
        !self.tap_connected
            && !cfg.mac_dock_enabled
            && wants_overlay(cfg)
            && !self.is_tap_connecting()
            && self.tap.has_attempted_connect()
    }

    pub fn is_settled(&self, cfg: &AppConfig) -> bool {
        if !wants_overlay(cfg) {
            return true;
        }
        self.tap_connected && self.has_dwm_refreshed(cfg)
    }

    pub fn note_tap_ready(&mut self) {
        self.tap.last_active = None;
        self.primary.refreshed_dwm_hwnd = HWND::default();
        self.primary.last_dwm_rect = RECT::default();
        for sec in &mut self.secondaries {
            sec.refreshed_dwm_hwnd = HWND::default();
            sec.last_dwm_rect = RECT::default();
        }
        if !self.primary.last_tray_hwnd.0.is_null() {
            self.tap_connected = self.tap.ensure_connected(self.primary.last_tray_hwnd);
        }
    }

    /// Forces DWM to drop Win11 tray's cold-boot opaque surface cache without
    /// altering WorkArea or sending WM_WINDOWPOSCHANGING to Explorer's AppBar manager.
    fn dwm_refresh_tray_surface_hwnd(tray_hw: HWND) {
        if crate::mac_dock::is_any_tray_overflow_open() {
            return;
        }
        unsafe {
            if tray_hw.0.is_null() || !IsWindowVisible(tray_hw).as_bool() {
                return;
            }
            let was_topmost = (GetWindowLongW(tray_hw, GWL_EXSTYLE) as u32 & WS_EX_TOPMOST.0) != 0;
            let base = SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_NOSENDCHANGING;
            let _ = SetWindowPos(tray_hw, None, 0, 0, 0, 0, base | SWP_HIDEWINDOW);
            if was_topmost {
                let _ = SetWindowPos(
                    tray_hw,
                    Some(HWND_TOPMOST),
                    0,
                    0,
                    0,
                    0,
                    SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOSENDCHANGING | SWP_SHOWWINDOW,
                );
            } else {
                let _ = SetWindowPos(tray_hw, None, 0, 0, 0, 0, base | SWP_SHOWWINDOW);
            }
            let _ = InvalidateRect(Some(tray_hw), None, false);
        }
    }

    unsafe fn reset_tray_hwnd_surface(
        hw: HWND,
        swca_fn: Option<SetWindowCompositionAttributeFn>,
    ) {
        if hw.0.is_null() {
            return;
        }
        let _ = SetWindowRgn(hw, None, true);
        let _ = DwmSetWindowAttribute(
            hw,
            DWMWA_BORDER_COLOR,
            (&DWMWA_COLOR_DEFAULT_U32 as *const u32) as *const _,
            4,
        );
        if let Some(swca) = swca_fn {
            let mut p = AccentPolicy {
                accent_state: ACCENT_DISABLED,
                accent_flags: 0,
                gradient_color: 0,
                animation_id: 0,
            };
            let mut data = WindowCompositionAttribData {
                attrib: WCA_ACCENT_POLICY,
                pv_data: (&mut p as *mut AccentPolicy) as *mut c_void,
                cb_data: size_of::<AccentPolicy>(),
            };
            let _ = swca(hw, &mut data);
        }
        let mut theme_res = 0usize;
        let _ = SendMessageTimeoutW(
            hw,
            0x031A, // WM_THEMECHANGED
            WPARAM(0),
            LPARAM(0),
            SMTO_ABORTIFHUNG,
            150,
            Some(&mut theme_res),
        );
        let mut set_res = 0usize;
        let _ = SendMessageTimeoutW(
            hw,
            0x001A, // WM_SETTINGCHANGE
            WPARAM(0),
            LPARAM(w!("ImmersiveColorSet").as_ptr() as isize),
            SMTO_ABORTIFHUNG,
            150,
            Some(&mut set_res),
        );
        let _ = SetWindowPos(
            hw,
            None,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED,
        );
        let _ = InvalidateRect(Some(hw), None, true);
    }

    pub fn try_cleanup_stale_old_tap_dll() {
        let mut buf = [0u16; 1024];
        let len = unsafe { GetModuleFileNameW(None, &mut buf) } as usize;
        if len == 0 {
            return;
        }
        let exe_path = PathBuf::from(String::from_utf16_lossy(&buf[..len]));
        if let Some(dir) = exe_path.parent() {
            let old_dll = dir.join("AutumnXamlTap.old.dll");
            if old_dll.exists() {
                let _ = std::fs::remove_file(&old_dll);
            }
        }
    }

    pub fn restore_default(&mut self) {
        self.hide_overlays();
        self.primary.refreshed_dwm_hwnd = HWND::default();
        self.primary.swca_cleared = false;
        for sec in &mut self.secondaries {
            sec.refreshed_dwm_hwnd = HWND::default();
            sec.swca_cleared = false;
        }
        if self.styled_any_hwnd
            || self.tap_connected
            || !self.tap.msg_wnd.0.is_null()
            || self.tap.last_active == Some(true)
        {
            unsafe {
                let mut hws = crate::taskbar_hide::TaskbarHider::collect_taskbar_hwnds();
                if hws.is_empty() {
                    if let Ok(main_hw) = FindWindowW(w!("Shell_TrayWnd"), PCWSTR::null()) {
                        if !main_hw.0.is_null() {
                            hws.push(main_hw);
                        }
                    }
                }
                if let Some(&main_hw) = hws.first() {
                    self.tap.apply(main_hw, false, true);
                }
                for hw in hws {
                    Self::reset_tray_hwnd_surface(hw, self.swca_fn);
                }
            }
            self.styled_any_hwnd = false;
        }
        self.invalidate_cache();
    }

    pub fn cleanup(&mut self) {
        self.restore_default();
        self.primary.destroy();
        for mut sec in self.secondaries.drain(..) {
            sec.destroy();
        }
    }

    pub fn has_stale_locked_old_tap_dll() -> bool {
        Self::try_cleanup_stale_old_tap_dll();
        let mut buf = [0u16; 1024];
        let len = unsafe { GetModuleFileNameW(None, &mut buf) } as usize;
        if len == 0 {
            return false;
        }
        let exe_path = PathBuf::from(String::from_utf16_lossy(&buf[..len]));
        exe_path
            .parent()
            .map(|dir| dir.join("AutumnXamlTap.old.dll").exists())
            .unwrap_or(false)
    }

    pub fn needs_explorer_restart(&self) -> bool {
        self.tap.needs_explorer_restart || Self::has_stale_locked_old_tap_dll()
    }

    pub fn spawn_detached_self(args: &[&str]) {
        use std::os::windows::process::CommandExt;
        const CREATE_BREAKAWAY_FROM_JOB: u32 = 0x01000000;
        const DETACHED_PROCESS: u32 = 0x00000008;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x00000200;
        const CREATE_NO_WINDOW: u32 = 0x08000000;

        if let Ok(exe) = std::env::current_exe() {
            let res = std::process::Command::new(&exe)
                .args(args)
                .creation_flags(
                    CREATE_BREAKAWAY_FROM_JOB
                        | DETACHED_PROCESS
                        | CREATE_NEW_PROCESS_GROUP
                        | CREATE_NO_WINDOW,
                )
                .spawn();
            if res.is_err() {
                let _ = std::process::Command::new(&exe)
                    .args(args)
                    .creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW)
                    .spawn();
            }
        }
    }

    pub fn spawn_background_restore_explorer() {
        Self::spawn_detached_self(&["--restore-explorer"]);
    }

    /// Restores Explorer's taskbar surface settings and returns `true` if the shell
    /// still needs an Explorer restart (stale locked `.old.dll`, brush overwrite, or failed detach).
    pub fn emergency_restore_taskbar_surface() -> bool {
        let mut need_explorer = Self::has_stale_locked_old_tap_dll();
        unsafe {
            if let Ok(h_map) = OpenFileMappingW(FILE_MAP_WRITE.0, false, w!("Local\\AutumnTaskbarTapCfg")) {
                if !h_map.0.is_null() {
                    let view = MapViewOfFile(h_map, FILE_MAP_WRITE, 0, 0, size_of::<TapSharedConfig>());
                    if !view.Value.is_null() {
                        *(view.Value as *mut TapSharedConfig) = TapSharedConfig {
                            version: 1,
                            active: 0,
                            tint_argb: 0,
                            border_argb: 0,
                        };
                        let _ = UnmapViewOfFile(view);
                    }
                    let _ = CloseHandle(h_map);
                }
            }

            let swca_fn: Option<SetWindowCompositionAttributeFn> = GetModuleHandleW(w!("user32.dll"))
                .ok()
                .or_else(|| LoadLibraryW(w!("user32.dll")).ok())
                .and_then(|m| GetProcAddress(m, s!("SetWindowCompositionAttribute")))
                .map(|p| std::mem::transmute(p));

            let mut hws = crate::taskbar_hide::TaskbarHider::collect_taskbar_hwnds();
            if hws.is_empty() {
                if let Ok(main_hw) = FindWindowW(w!("Shell_TrayWnd"), PCWSTR::null()) {
                    if !main_hw.0.is_null() {
                        hws.push(main_hw);
                    }
                }
            }
            for hw in hws {
                let mut exp_pid = 0u32;
                GetWindowThreadProcessId(hw, Some(&mut exp_pid));
                if exp_pid != 0 {
                    if let Some(msg_wnd) = TapBridge::find_existing_msg_wnd(exp_pid) {
                        let mut res = 0usize;
                        let ok = SendMessageTimeoutW(
                            msg_wnd,
                            WM_APP_TAP_DETACH,
                            WPARAM(0),
                            LPARAM(0),
                            SMTO_ABORTIFHUNG,
                            200,
                            Some(&mut res),
                        );
                        if ok.0 == 0 || res == 2 {
                            need_explorer = true;
                        }
                    }
                }
                Self::reset_tray_hwnd_surface(hw, swca_fn);
            }
        }
        need_explorer
    }

    /// Restarts the Explorer shell process owning `Shell_TrayWnd` so any legacy in-memory
    /// `AutumnXamlTap.old.dll` is unloaded and Windows recreates its native XAML taskbar fill.
    pub fn restart_shell_explorer_for_restore() {
        use std::os::windows::process::CommandExt;
        let _ = Self::emergency_restore_taskbar_surface();
        crate::taskbar_hide::TaskbarHider::emergency_restore_windows_taskbar();
        let mut exp_pid = 0u32;
        unsafe {
            if let Ok(main_hw) = FindWindowW(w!("Shell_TrayWnd"), PCWSTR::null()) {
                if !main_hw.0.is_null() {
                    GetWindowThreadProcessId(main_hw, Some(&mut exp_pid));
                }
            }
        }
        if exp_pid != 0 {
            let _ = std::process::Command::new("taskkill")
                .args(["/F", "/PID", &exp_pid.to_string()])
                .creation_flags(0x08000000)
                .status();
            std::thread::sleep(Duration::from_millis(600));
            Self::try_cleanup_stale_old_tap_dll();
            let _ = std::process::Command::new("explorer.exe").spawn();
            std::thread::sleep(Duration::from_millis(1200));
            crate::taskbar_hide::TaskbarHider::emergency_restore_windows_taskbar();
        }
    }

    pub fn sync_style_fast(&mut self, cfg: &AppConfig) {
        self.invalidate_style_only();
        // Pure visual overlay update (opacity/hue/viz): avoid forcing cross-process TAP IPC
        // into explorer.exe on every slider tick when TAP state is already connected & unchanged.
        self.sync_taskbar_inner(cfg, true, false, false);
    }

    pub fn has_viz_decay(&self) -> bool {
        self.viz.has_energy()
    }

    pub fn is_viz_hover_active(&self) -> bool {
        self.viz.hover_active
            || self.viz.hover_mix > 0.01
            || self.viz.ripples.iter().any(|r| r.active)
    }

    pub fn clear_viz_energy(&mut self) {
        self.viz = VizSnapshot::default();
    }

    pub fn tick_visualizer(&mut self, cfg: &AppConfig, raw_bands: &[f32; 12]) {
        if !cfg.tb_show_fx || cfg.tb_viz_style == TaskbarVizStyle::Off || !wants_overlay(cfg) {
            self.viz = VizSnapshot::default();
            return;
        }
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_viz_tick);
        let allow_ambient = cfg.tb_ambient_anim && cfg.tb_viz_style.is_ambient_live();
        let frame_ms = cfg.anim_frame_ms();
        let active_gap = Duration::from_millis(frame_ms.saturating_sub(2).max(6) as u64);
        if elapsed < active_gap {
            return;
        }

        let mut hover_in = false;
        let mut hx = self.viz.hover_x;
        let mut hy = self.viz.hover_y;
        let mut hover_bar_w = (self.primary.last_bar_rect.right - self.primary.last_bar_rect.left) as f32;
        let mut hover_bar_h = (self.primary.last_bar_rect.bottom - self.primary.last_bar_rect.top) as f32;
        let mut pt = POINT::default();
        let got_pt = cfg.tb_hover_react && unsafe { GetCursorPos(&mut pt).is_ok() };

        if got_pt {
            for ov in std::iter::once(&self.primary).chain(self.secondaries.iter()) {
                let br = ov.last_bar_rect;
                let bw = (br.right - br.left) as f32;
                let bh = (br.bottom - br.top) as f32;
                if bw <= 2.0 || bh <= 2.0 || ov.last_tray_hwnd.0.is_null() {
                    continue;
                }
                let u = get_hwnd_dpi_scale(ov.last_tray_hwnd);
                let pad = (6.0 * u).round() as i32;
                if pt.x >= br.left - pad
                    && pt.x <= br.right + pad
                    && pt.y >= br.top - pad
                    && pt.y <= br.bottom + pad
                {
                    hover_in = true;
                    hx = ((pt.x - br.left) as f32 / bw).clamp(0.0, 1.0);
                    hy = ((pt.y - br.top) as f32 / bh).clamp(0.0, 1.0);
                    hover_bar_w = bw;
                    hover_bar_h = bh;
                    break;
                }
            }
        }

        let dt = elapsed.as_secs_f32();
        self.last_viz_tick = now;

        let prev_bands = self.viz.bands;
        let prev_peaks = self.viz.peaks;
        let prev_hover = self.viz.hover_mix;
        let prev_had_energy = self.viz.has_energy();
        self.viz.update_from_raw(raw_bands, dt, allow_ambient);
        if hover_bar_w > 2.0 && hover_bar_h > 2.0 {
            self.viz.step_hover_physics(
                cfg.tb_viz_style,
                dt,
                hx,
                hy,
                hover_in,
                cfg.tb_hover_react,
                hover_bar_w,
                hover_bar_h,
            );
        }

        if !allow_ambient && !self.viz.has_energy() {
            // Always paint the final clear frame when transitioning from active energy to silence
            // so zero frozen ghost bars or peak-hold caps ever remain stuck on the taskbar.
            let max_band_diff = self
                .viz
                .bands
                .iter()
                .zip(prev_bands.iter())
                .fold(0.0f32, |acc, (&a, &b)| acc.max((a - b).abs()));
            let max_peak_diff = self
                .viz
                .peaks
                .iter()
                .zip(prev_peaks.iter())
                .fold(0.0f32, |acc, (&a, &b)| acc.max((a - b).abs()));
            let hover_cleared = prev_hover >= 0.003 && self.viz.hover_mix < 0.003;
            if !prev_had_energy
                && !hover_cleared
                && max_band_diff < 0.003
                && max_peak_diff < 0.003
                && (self.viz.hover_mix - prev_hover).abs() < 0.003
            {
                return;
            }
        }

        let tap_ok = self.tap_connected;
        let want_blur = wants_blur_backdrop(cfg) && tap_ok;
        let d2d_opt = self.d2d_factory.as_ref();
        for ov in std::iter::once(&mut self.primary).chain(self.secondaries.iter_mut()) {
            let bar_rect = ov.last_bar_rect;
            if ov.last_tray_hwnd.0.is_null()
                || ov.last_pills.is_empty()
                || bar_rect.right <= bar_rect.left
                || bar_rect.bottom <= bar_rect.top
            {
                continue;
            }
            let was_vis = ov.glass_visible;
            let u = get_hwnd_dpi_scale(ov.last_tray_hwnd);
            let scaled_radius = if matches!(cfg.pill_mode, PillMode::FullBar | PillMode::NativeDefault) {
                0
            } else {
                ((cfg.corner_radius as f32) * u).round() as i32
            };
            let pills = ov.last_pills.clone();
            let icons = ov.last_icons.clone();
            let sys_light = cached_system_uses_light_theme();
            ov.render_glass_window(
                ov.last_tray_hwnd,
                bar_rect,
                &pills,
                &icons,
                scaled_radius,
                u,
                tap_ok,
                sys_light,
                cfg,
                d2d_opt,
                &mut self.dc_rt,
                &mut self.viz,
            );
            if !was_vis || !tap_ok {
                ov.sync_overlay_zorder(ov.last_tray_hwnd, want_blur, tap_ok);
            }
        }
    }

    pub fn sync_taskbar(&mut self, cfg: &AppConfig, force_apply: bool) {
        self.sync_taskbar_inner(cfg, force_apply, true, force_apply);
    }

    /// Cheap foreground/focus guard: enforces Z-order across primary + secondary taskbars.
    pub fn ensure_zorder_only(&mut self, cfg: &AppConfig) {
        if !wants_overlay(cfg) {
            return;
        }
        let tap_ok = self.tap_connected;
        let want_blur = wants_blur_backdrop(cfg) && tap_ok;
        for (tray_hw, _, _) in query_all_tray_geometries() {
            if let Some(ov) = self.find_overlay_for_hwnd(tray_hw) {
                ov.sync_overlay_zorder(tray_hw, want_blur, tap_ok);
            }
        }
    }

    fn find_overlay_for_hwnd(&self, tray_hw: HWND) -> Option<&TrayOverlayState> {
        if self.primary.last_tray_hwnd == tray_hw {
            return Some(&self.primary);
        }
        self.secondaries.iter().find(|s| s.last_tray_hwnd == tray_hw)
    }

    fn reconcile_secondaries(&mut self, sec_trays: &[(HWND, RECT, f32)]) {
        let mut old = std::mem::take(&mut self.secondaries);
        let mut next = Vec::with_capacity(sec_trays.len());
        for &(sec_hw, _, _) in sec_trays {
            if let Some(pos) = old.iter().position(|o| o.last_tray_hwnd == sec_hw) {
                next.push(old.swap_remove(pos));
            } else {
                next.push(TrayOverlayState::new(self.hinst));
            }
        }
        for mut unused in old {
            unused.destroy();
        }
        self.secondaries = next;
    }

    /// Triggers non-blocking background UIA measurement across primary + secondary taskbars.
    pub fn request_pill_remeasure(&mut self, cfg: &AppConfig, flush_dwm_if_settled: bool) {
        let trays = query_all_tray_geometries();
        if trays.is_empty() {
            self.hide_overlays();
            return;
        }
        let (main_hw, main_rect, _) = trays[0];
        let sec_trays = &trays[1..];
        let layout_changed = main_rect != self.primary.last_bar_rect
            || main_hw != self.primary.last_tray_hwnd
            || sec_trays.len() != self.secondaries.len()
            || sec_trays.iter().zip(self.secondaries.iter()).any(|((shw, sr, _), ov)| {
                *shw != ov.last_tray_hwnd || *sr != ov.last_bar_rect
            });
        if layout_changed {
            self.sync_taskbar_inner(cfg, true, true, true);
            return;
        }
        let use_tap = wants_overlay(cfg);
        self.tap.apply(main_hw, use_tap, false);
        self.tap_connected = self.tap.tap_ok;
        if !use_tap {
            self.primary.pending_dwm_settle = false;
            for sec in &mut self.secondaries {
                sec.pending_dwm_settle = false;
            }
        }
        let tap_ok = self.tap_connected;
        let swca = self.swca_fn;
        let now = Instant::now();
        for (idx, (tray_hw, bar_rect, u)) in trays.into_iter().enumerate() {
            let ov = if idx == 0 {
                &mut self.primary
            } else {
                &mut self.secondaries[idx - 1]
            };
            if use_tap {
                if !ov.swca_cleared || flush_dwm_if_settled {
                    clear_hw_accent_with(swca, tray_hw);
                    ov.swca_cleared = true;
                }
                if flush_dwm_if_settled
                    && tap_ok
                    && (ov.pending_dwm_settle
                        || ov.refreshed_dwm_hwnd != tray_hw
                        || ov.last_dwm_rect != bar_rect)
                {
                    Self::dwm_refresh_tray_surface_hwnd(tray_hw);
                    ov.refreshed_dwm_hwnd = tray_hw;
                    ov.last_dwm_rect = bar_rect;
                    ov.pending_dwm_settle = false;
                }
            }
            if matches!(cfg.pill_mode, PillMode::SplitPills | PillMode::UnifiedPill)
                || cfg.tb_theme.uses_icon_pads()
            {
                let cooldown_ok = ov
                    .last_uia_req_time
                    .map(|t| now.duration_since(t) >= Duration::from_millis(320))
                    .unwrap_or(true);
                if cooldown_ok {
                    ov.last_uia_req_time = Some(now);
                    self.uia_worker.request(
                        tray_hw,
                        bar_rect,
                        u,
                        cfg.pill_mode,
                        cfg.tb_theme.uses_icon_pads(),
                    );
                }
            }
        }
    }

    pub fn on_tap_connected(&mut self, cfg: &AppConfig) {
        self.invalidate_cache();
        self.sync_taskbar_inner(cfg, true, true, true);
    }

    /// Applies finished background UIA pill rects when posted back to main thread.
    pub fn apply_async_pills(&mut self, cfg: &AppConfig) -> bool {
        let results = self.uia_worker.take_all();
        let is_pill_mode = matches!(cfg.pill_mode, PillMode::SplitPills | PillMode::UnifiedPill);
        if results.is_empty() || (!is_pill_mode && !cfg.tb_theme.uses_icon_pads()) {
            return false;
        }
        let mut any_changed = false;
        let mut hide_all = false;
        let tap_ok = self.tap_connected;
        let sys_light = cached_system_uses_light_theme();
        let want_blur = wants_blur_backdrop(cfg) && tap_ok;
        {
            let TaskbarStyler {
                primary,
                secondaries,
                uia_worker,
                swca_fn,
                d2d_factory,
                dc_rt,
                last_pill_count,
                last_bar_rect_str,
                viz,
                ..
            } = self;
            let d2d_opt = d2d_factory.as_ref();
            let swca = *swca_fn;
            for res in results {
                if res.pill_mode != cfg.pill_mode || res.pills.is_empty() {
                    continue;
                }
                let target_hw = HWND(res.taskbar_hwnd as *mut c_void);
                let is_primary = target_hw == primary.last_tray_hwnd;
                let Some(ov) = (if is_primary {
                    Some(&mut *primary)
                } else {
                    secondaries.iter_mut().find(|s| s.last_tray_hwnd == target_hw)
                }) else {
                    continue;
                };
                if res.gen < ov.last_applied_uia_gen {
                    continue;
                }
                ov.last_applied_uia_gen = res.gen;
                let Some((_, cur_bar, u)) = query_single_tray_geometry(target_hw) else {
                    if is_primary {
                        hide_all = true;
                    } else {
                        ov.hide_overlays();
                    }
                    continue;
                };
                if cur_bar != res.bar_rect {
                    ov.last_uia_req_time = Some(Instant::now());
                    uia_worker.request(
                        target_hw,
                        cur_bar,
                        u,
                        cfg.pill_mode,
                        cfg.tb_theme.uses_icon_pads(),
                    );
                    continue;
                }
                if is_primary {
                    if is_pill_mode {
                        save_pill_cache(cur_bar, cfg.pill_mode, u, &res.pills);
                    }
                    *last_pill_count = res.pills.len();
                    *last_bar_rect_str = format!(
                        "({},{},{},{})",
                        cur_bar.left, cur_bar.top, cur_bar.right, cur_bar.bottom
                    );
                }
                ov.last_pill_count = res.pills.len();

                let icons_changed =
                    cfg.tb_theme.uses_icon_pads() && !pills_close(&ov.last_icons, &res.icons);
                let changed = ov.last_bar_rect != cur_bar
                    || !pills_close(&ov.last_pills, &res.pills)
                    || icons_changed
                    || !ov.glass_visible;
                if changed {
                    let scaled_radius = if is_pill_mode {
                        ((cfg.corner_radius as f32) * u).round() as i32
                    } else {
                        0
                    };
                    ov.render_glass_window(
                        target_hw,
                        cur_bar,
                        &res.pills,
                        &res.icons,
                        scaled_radius,
                        u,
                        tap_ok,
                        sys_light,
                        cfg,
                        d2d_opt,
                        dc_rt,
                        viz,
                    );
                    ov.update_blur_window(
                        target_hw,
                        cur_bar,
                        &res.pills,
                        scaled_radius,
                        u,
                        tap_ok,
                        sys_light,
                        cfg,
                        swca,
                    );
                    ov.last_pills = res.pills;
                    ov.last_icons = res.icons;
                    ov.last_bar_rect = cur_bar;
                    any_changed = true;
                } else {
                    ov.last_icons = res.icons;
                }
                ov.sync_overlay_zorder(target_hw, want_blur, tap_ok);
            }
        }
        if hide_all {
            self.hide_overlays();
        }
        any_changed
    }

    fn sync_taskbar_inner(
        &mut self,
        cfg: &AppConfig,
        force_apply: bool,
        allow_uia: bool,
        force_tap: bool,
    ) {
        let trays = query_all_tray_geometries();
        let Some((main_hw, main_rect, main_u)) = trays.first().copied() else {
            self.hide_overlays();
            return;
        };

        let sec_trays = &trays[1..];
        self.reconcile_secondaries(sec_trays);

        let main_changed = self.primary.last_tray_hwnd != main_hw
            || self.primary.last_bar_rect != main_rect;
        let use_tap = wants_overlay(cfg);
        if use_tap {
            self.styled_any_hwnd = true;
        }
        self.tap
            .apply(main_hw, use_tap, force_tap || main_changed);
        self.tap_connected = self.tap.tap_ok;

        let pal = effective_taskbar_palette(cfg);
        let sys_light = cached_system_uses_light_theme();
        let sig = StyleSig {
            pill_mode: cfg.pill_mode as u8,
            surface_mode: cfg.surface_mode as u8,
            tint_alpha: cfg.tint_alpha,
            tb_theme: cfg.tb_theme as u8,
            amber_border: cfg.amber_border,
            corner_radius: cfg.corner_radius,
            tb_viz_style: cfg.tb_viz_style as u8,
            tb_viz_opacity: cfg.tb_viz_opacity,
            tb_viz_mirror: cfg.tb_viz_mirror,
            tb_viz_color_mode: cfg.tb_viz_color_mode as u8,
            tb_viz_accent_rgb: cfg.tb_viz_accent_rgb,
            tb_show_backdrop: cfg.tb_show_backdrop,
            tb_show_rim: cfg.tb_show_rim,
            tb_show_fx: cfg.tb_show_fx,
            accent: pal.accent,
            accent_deep: pal.accent_deep,
            tap_connected: self.tap_connected,
            sys_light,
        };

        let d2d_opt = self.d2d_factory.as_ref();
        Self::sync_overlay_target(
            &mut self.primary,
            main_hw,
            main_rect,
            main_u,
            true,
            use_tap,
            self.tap_connected,
            sig,
            cfg,
            force_apply,
            allow_uia,
            self.swca_fn,
            &self.uia_worker,
            d2d_opt,
            &mut self.dc_rt,
            &mut self.viz,
        );
        self.last_pill_count = self.primary.last_pill_count;
        self.last_bar_rect_str = format!(
            "({},{},{},{})",
            main_rect.left, main_rect.top, main_rect.right, main_rect.bottom
        );

        for ((sec_hw, sec_rect, sec_u), sec_ov) in sec_trays.iter().zip(self.secondaries.iter_mut()) {
            Self::sync_overlay_target(
                sec_ov,
                *sec_hw,
                *sec_rect,
                *sec_u,
                false,
                use_tap,
                self.tap_connected,
                sig,
                cfg,
                force_apply,
                allow_uia,
                self.swca_fn,
                &self.uia_worker,
                d2d_opt,
                &mut self.dc_rt,
                &mut self.viz,
            );
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn sync_overlay_target(
        ov: &mut TrayOverlayState,
        tray_hw: HWND,
        bar_rect: RECT,
        u: f32,
        is_primary: bool,
        use_tap: bool,
        tap_connected: bool,
        sig: StyleSig,
        cfg: &AppConfig,
        force_apply: bool,
        allow_uia: bool,
        swca_fn: Option<SetWindowCompositionAttributeFn>,
        uia_worker: &UiaWorker,
        d2d_factory: Option<&ID2D1Factory>,
        dc_rt: &mut Option<ID2D1DCRenderTarget>,
        viz: &mut VizSnapshot,
    ) {
        let hwnd_changed = ov.last_tray_hwnd != tray_hw;
        let rect_moved = ov.last_bar_rect != bar_rect;
        if hwnd_changed || rect_moved {
            ov.refreshed_dwm_hwnd = HWND::default();
            ov.last_dwm_rect = RECT::default();
            ov.swca_cleared = false;
            if hwnd_changed {
                ov.border_stripped = None;
            }
            ov.last_pills.clear();
            ov.last_icons.clear();
            ov.pending_dwm_settle = true;
        }
        ov.last_tray_hwnd = tray_hw;

        let need_cold_dwm_refresh = use_tap
            && tap_connected
            && (ov.refreshed_dwm_hwnd != tray_hw || ov.last_dwm_rect != bar_rect);
        let sig_changed = force_apply
            || hwnd_changed
            || rect_moved
            || need_cold_dwm_refresh
            || ov.last_style_sig != Some(sig);

        // Strip Windows DWM border line only when taskbar HWND or overlay active state changes
        if hwnd_changed || ov.border_stripped != Some(use_tap) {
            unsafe {
                let border_color = if !use_tap {
                    DWMWA_COLOR_DEFAULT_U32
                } else {
                    DWMWA_COLOR_NONE_U32
                };
                let _ = DwmSetWindowAttribute(
                    tray_hw,
                    DWMWA_BORDER_COLOR,
                    (&border_color as *const u32) as *const _,
                    4,
                );
            }
            ov.border_stripped = Some(use_tap);
        }

        if !use_tap {
            ov.refreshed_dwm_hwnd = HWND::default();
            ov.last_dwm_rect = RECT::default();
            ov.pending_dwm_settle = false;
            if ov.swca_cleared || sig_changed {
                clear_hw_accent_with(swca_fn, tray_hw);
                ov.swca_cleared = false;
            }
            ov.hide_overlays();
            ov.last_pill_count = if cfg.pill_mode == PillMode::FullBar { 1 } else { 0 };
            ov.last_bar_rect = bar_rect;
            ov.last_pill_mode = Some(cfg.pill_mode);
            ov.last_style_sig = Some(sig);
            return;
        }

        if !ov.swca_cleared || hwnd_changed {
            clear_hw_accent_with(swca_fn, tray_hw);
            ov.swca_cleared = true;
        }
        if need_cold_dwm_refresh {
            Self::dwm_refresh_tray_surface_hwnd(tray_hw);
            ov.refreshed_dwm_hwnd = tray_hw;
            ov.last_dwm_rect = bar_rect;
            ov.pending_dwm_settle = false;
        }

        let scaled_radius = ((cfg.corner_radius as f32) * u).round() as i32;

        // 1. Full Bar / Native Viz: draw flush with bar_rect (zero SWCA drift!)
        let wants_icons = cfg.tb_theme.uses_icon_pads();
        let sys_light = sig.sys_light;
        if matches!(cfg.pill_mode, PillMode::FullBar | PillMode::NativeDefault) {
            if wants_icons
                && allow_uia
                && (ov.last_icons.is_empty()
                    || rect_moved
                    || hwnd_changed
                    || ov
                        .last_uia_req_time
                        .map(|t| t.elapsed() >= Duration::from_millis(5500))
                        .unwrap_or(true))
            {
                ov.last_uia_req_time = Some(Instant::now());
                uia_worker.request(tray_hw, bar_rect, u, cfg.pill_mode, true);
            }
            let full_pills = vec![bar_rect];
            ov.last_pill_count = 1;
            let bar_changed = sig_changed || ov.last_bar_rect != bar_rect || !ov.glass_visible;
            if bar_changed {
                let cached_icons = ov.last_icons.clone();
                ov.render_glass_window(
                    tray_hw,
                    bar_rect,
                    &full_pills,
                    &cached_icons,
                    0,
                    u,
                    tap_connected,
                    sys_light,
                    cfg,
                    d2d_factory,
                    dc_rt,
                    viz,
                );
                ov.update_blur_window(
                    tray_hw,
                    bar_rect,
                    &full_pills,
                    0,
                    u,
                    tap_connected,
                    sys_light,
                    cfg,
                    swca_fn,
                );
                ov.last_pills = full_pills;
                ov.last_bar_rect = bar_rect;
            }
            ov.sync_overlay_zorder(tray_hw, wants_blur_backdrop(cfg) && tap_connected, tap_connected);
            ov.last_pill_mode = Some(cfg.pill_mode);
            ov.last_style_sig = Some(sig);
            return;
        }

        // 2. Crystal Split Pills / Unified Capsule:
        let mode_switched = ov.last_pill_mode != Some(cfg.pill_mode);
        let can_use_cached = !ov.last_pills.is_empty() && !rect_moved && !hwnd_changed;
        let screen_pills = if can_use_cached {
            if allow_uia
                && (mode_switched
                    || rect_moved
                    || hwnd_changed
                    || (wants_icons && ov.last_icons.is_empty())
                    || ov
                        .last_uia_req_time
                        .map(|t| t.elapsed() >= Duration::from_millis(5500))
                        .unwrap_or(true))
            {
                ov.last_uia_req_time = Some(Instant::now());
                uia_worker.request(tray_hw, bar_rect, u, cfg.pill_mode, wants_icons);
            }
            ov.last_pills.clone()
        } else {
            if ov.last_uia_req_time.is_none() || allow_uia {
                ov.last_uia_req_time = Some(Instant::now());
                uia_worker.request(tray_hw, bar_rect, u, cfg.pill_mode, wants_icons);
            }
            if is_primary {
                load_pill_cache(bar_rect, cfg.pill_mode, u)
                    .unwrap_or_else(|| vec![fallback_inset_rect(bar_rect, u)])
            } else {
                vec![fallback_inset_rect(bar_rect, u)]
            }
        };

        ov.last_pill_count = screen_pills.len();

        let pills_changed = sig_changed
            || rect_moved
            || mode_switched
            || !pills_close(&ov.last_pills, &screen_pills)
            || !ov.glass_visible;

        if pills_changed {
            let cached_icons = ov.last_icons.clone();
            ov.render_glass_window(
                tray_hw,
                bar_rect,
                &screen_pills,
                &cached_icons,
                scaled_radius,
                u,
                tap_connected,
                sys_light,
                cfg,
                d2d_factory,
                dc_rt,
                viz,
            );
            ov.update_blur_window(
                tray_hw,
                bar_rect,
                &screen_pills,
                scaled_radius,
                u,
                tap_connected,
                sys_light,
                cfg,
                swca_fn,
            );
            ov.last_pills = screen_pills;
            ov.last_bar_rect = bar_rect;
        }

        ov.sync_overlay_zorder(tray_hw, wants_blur_backdrop(cfg) && tap_connected, tap_connected);
        ov.last_pill_mode = Some(cfg.pill_mode);
        ov.last_style_sig = Some(sig);
    }
}

fn clear_hw_accent_with(swca_fn: Option<SetWindowCompositionAttributeFn>, hwnd: HWND) {
    let Some(swca) = swca_fn else {
        return;
    };
    let mut p = AccentPolicy {
        accent_state: ACCENT_DISABLED,
        accent_flags: 0,
        gradient_color: 0,
        animation_id: 0,
    };
    let mut data = WindowCompositionAttribData {
        attrib: WCA_ACCENT_POLICY,
        pv_data: (&mut p as *mut AccentPolicy) as *mut c_void,
        cb_data: size_of::<AccentPolicy>(),
    };
    unsafe {
        let _ = swca(hwnd, &mut data);
    }
}

fn apply_blur_acrylic_with(
    swca_fn: Option<SetWindowCompositionAttributeFn>,
    hwnd: HWND,
    abgr: u32,
) {
    let Some(swca) = swca_fn else {
        return;
    };
    let mut p = AccentPolicy {
        accent_state: ACCENT_ENABLE_ACRYLICBLURBEHIND,
        accent_flags: 2,
        gradient_color: abgr,
        animation_id: 0,
    };
    let mut data = WindowCompositionAttribData {
        attrib: WCA_ACCENT_POLICY,
        pv_data: (&mut p as *mut AccentPolicy) as *mut c_void,
        cb_data: size_of::<AccentPolicy>(),
    };
    unsafe {
        let _ = swca(hwnd, &mut data);
    }
}

impl TrayOverlayState {
    #[allow(clippy::too_many_arguments)]
    fn update_blur_window(
        &mut self,
        tray_hw: HWND,
        bar_rect: RECT,
        screen_pills: &[RECT],
        radius: i32,
        u_bar: f32,
        tap_connected: bool,
        sys_light: bool,
        cfg: &AppConfig,
        swca_fn: Option<SetWindowCompositionAttributeFn>,
    ) {
        let want_blur = wants_blur_backdrop(cfg) && tap_connected && !screen_pills.is_empty();
        if !want_blur {
            unsafe {
                for &hw in &self.blur_hwnds {
                    if !hw.0.is_null() && IsWindowVisible(hw).as_bool() {
                        let _ = ShowWindow(hw, SW_HIDE);
                    }
                }
            }
            self.blur_visible = false;
            self.last_blur_rects.clear();
            self.last_blur_rounded = None;
            self.last_blur_accent_key = None;
            return;
        }

        let rgn_radius = if cfg.pill_mode == PillMode::FullBar {
            0
        } else {
            effective_pill_radius_for(screen_pills, radius, u_bar, cfg.tb_theme) as i32
        };
        let rounded = cfg.pill_mode != PillMode::FullBar && rgn_radius > 2;

        let mon = get_monitor_rect(tray_hw);
        let bar_w = bar_rect.right - bar_rect.left;
        let bar_h = bar_rect.bottom - bar_rect.top;
        let is_vertical = bar_h > bar_w * 2;
        let anchor_min = if is_vertical {
            (bar_rect.left - mon.left).abs() <= (mon.right - bar_rect.right).abs()
        } else {
            (bar_rect.top - mon.top).abs() <= (mon.bottom - bar_rect.bottom).abs()
        };

        let target_count = if cfg.pill_mode == PillMode::FullBar {
            1
        } else {
            screen_pills.len().min(MAX_BLUR_PILLS)
        };
        let mut target_rects = Vec::with_capacity(target_count);
        if cfg.pill_mode == PillMode::FullBar {
            target_rects.push(bar_rect);
        } else {
            for &raw_p in &screen_pills[..target_count] {
                let p = dock_adjusted_rect(raw_p, bar_rect, is_vertical, anchor_min, u_bar, cfg.tb_theme);
                let inset = if (p.right - p.left) > 20 && (p.bottom - p.top) > 20 {
                    if rgn_radius > 16 { 2 } else { 1 }
                } else {
                    0
                };
                target_rects.push(RECT {
                    left: p.left + inset,
                    top: p.top + inset,
                    right: (p.right - inset).max(p.left + inset + 6),
                    bottom: (p.bottom - inset).max(p.top + inset + 6),
                });
            }
        }

        let prev_pool_len = self.blur_hwnds.len();
        while self.blur_hwnds.len() < target_rects.len() {
            let hw = create_blur_hwnd(self.hinst);
            if hw.0.is_null() {
                break;
            }
            self.blur_hwnds.push(hw);
        }
        let active_count = target_rects.len().min(self.blur_hwnds.len());
        if active_count == 0 {
            self.blur_visible = false;
            return;
        }
        let pool_grew = self.blur_hwnds.len() > prev_pool_len;
        target_rects.truncate(active_count);

        unsafe {
            for &hw in &self.blur_hwnds[active_count..] {
                if !hw.0.is_null() && IsWindowVisible(hw).as_bool() {
                    let _ = ShowWindow(hw, SW_HIDE);
                }
            }
        }

        let need_pos = !self.blur_visible || pool_grew || self.last_blur_rects != target_rects;
        if need_pos {
            unsafe {
                for (i, &rc) in target_rects.iter().enumerate() {
                    let _ = SetWindowPos(
                        self.blur_hwnds[i],
                        None,
                        rc.left,
                        rc.top,
                        (rc.right - rc.left).max(4),
                        (rc.bottom - rc.top).max(4),
                        SWP_NOACTIVATE | SWP_NOZORDER,
                    );
                }
            }
            self.last_blur_rects = target_rects;
        }

        if pool_grew || self.last_blur_rounded != Some(rounded) {
            let pref = if rounded {
                DWMWCP_ROUND_U32
            } else {
                DWMWCP_DONOTROUND_U32
            };
            unsafe {
                for &hw in &self.blur_hwnds[..active_count] {
                    let _ = DwmSetWindowAttribute(
                        hw,
                        DWMWA_WINDOW_CORNER_PREFERENCE_ATTR,
                        (&pref as *const u32) as *const _,
                        4,
                    );
                }
            }
            self.last_blur_rounded = Some(rounded);
        }

        // Distinct Windows 11 Acrylic colors per surface mode (ABGR byte order, alpha in 4..235).
        // Light-surface Clean & Minimal themes use luminous pearl frost in Light Mode and
        // automatically protect white system-tray text on pill 1+ when Windows is in Dark Mode.
        let is_light_theme = cfg.tb_theme.is_light_surface();
        let (blur_abgr, tray_blur_abgr, accent_tag): (u32, u32, u8) = if is_light_theme {
            match cfg.surface_mode {
                SurfaceMode::ClearTint => (0, 0, 0),
                _ => {
                    let a_main = ((cfg.tint_alpha as u32 / 3).clamp(18, 86)) << 24;
                    let main_c = a_main | (244 << 16) | (242 << 8) | 240;
                    if sys_light {
                        (main_c, main_c, 4)
                    } else {
                        let a_tray = ((cfg.tint_alpha as u32 / 2).clamp(32, 108)) << 24;
                        let tray_c = a_tray | (76 << 16) | (68 << 8) | 64;
                        (main_c, tray_c, 5)
                    }
                }
            }
        } else {
            let (c, tag) = match cfg.surface_mode {
                SurfaceMode::Acrylic => {
                    let a = ((cfg.tint_alpha as u32 / 2).clamp(16, 118)) << 24;
                    (a | (10 << 16) | (13 << 8) | 18, 1)
                }
                SurfaceMode::Blur => {
                    let a = ((cfg.tint_alpha as u32 / 3).clamp(16, 86)) << 24;
                    (a | (68 << 16) | (58 << 8) | 50, 2)
                }
                SurfaceMode::SystemDefault => {
                    let a = ((cfg.tint_alpha as u32 / 2).clamp(32, 110)) << 24;
                    (a | (28 << 16) | (26 << 8) | 26, 3)
                }
                SurfaceMode::ClearTint => (0, 0),
            };
            (c, c, tag)
        };
        let blur_alpha_byte = (blur_abgr >> 24) as u8;
        let accent_key = (
            cfg.surface_mode as u8,
            accent_tag,
            blur_alpha_byte,
            active_count as u8,
        );
        if (pool_grew || self.last_blur_accent_key != Some(accent_key)) && blur_abgr != 0 {
            for (idx, &hw) in self.blur_hwnds[..active_count].iter().enumerate() {
                let col = if idx >= 1 { tray_blur_abgr } else { blur_abgr };
                apply_blur_acrylic_with(swca_fn, hw, col);
            }
            self.last_blur_accent_key = Some(accent_key);
        }
        self.blur_visible = true;
    }

    #[allow(clippy::too_many_arguments)]
    fn render_glass_window(
        &mut self,
        tray_hw: HWND,
        bar_rect: RECT,
        screen_pills: &[RECT],
        screen_icons: &[RECT],
        radius: i32,
        u: f32,
        tap_connected: bool,
        sys_light: bool,
        cfg: &AppConfig,
        d2d_opt: Option<&ID2D1Factory>,
        dc_rt_slot: &mut Option<ID2D1DCRenderTarget>,
        viz: &mut VizSnapshot,
    ) {
        if self.glass_hwnd.0.is_null() {
            return;
        }
        if cfg.tb_ambient_anim && cfg.tb_viz_style.is_ambient_live() && !viz.has_energy() {
            viz.update_from_raw(&[0.0; 12], 0.0, true);
        }

        // Keep glass_hwnd 100% flush with its taskbar HWND so shadows/rims never bleed onto apps
        let win_rect = bar_rect;
        let win_w = win_rect.right - win_rect.left;
        let win_h = win_rect.bottom - win_rect.top;
        if win_w <= 2 || win_h <= 2 || !self.surface.ensure_size(win_w, win_h) {
            return;
        }

        let mon_rect = get_monitor_rect(tray_hw);
        for attempt in 0..2 {
            if dc_rt_slot.is_none() {
                if let Some(factory) = d2d_opt {
                    *dc_rt_slot = create_dc_render_target(factory);
                }
            }
            let Some(ref dc_rt) = *dc_rt_slot else {
                return;
            };

            let ok = unsafe {
                draw_glass_surface(
                    dc_rt,
                    d2d_opt,
                    self.surface.mem_dc,
                    mon_rect,
                    win_rect,
                    bar_rect,
                    screen_pills,
                    screen_icons,
                    radius,
                    u,
                    tap_connected,
                    sys_light,
                    cfg,
                    viz,
                )
            };
            if ok {
                break;
            }
            if let Some(ref rt) = *dc_rt_slot {
                crate::realistic_pbr::evict_rt(rt);
            }
            *dc_rt_slot = None;
            if attempt == 1 {
                return;
            }
        }

        unsafe {
            let dst_pt = POINT {
                x: win_rect.left,
                y: win_rect.top,
            };
            let size = SIZE {
                cx: win_w,
                cy: win_h,
            };
            let src_pt = POINT { x: 0, y: 0 };
            let blend = BLENDFUNCTION {
                BlendOp: AC_SRC_OVER as u8,
                BlendFlags: 0,
                SourceConstantAlpha: 255,
                AlphaFormat: AC_SRC_ALPHA as u8,
            };
            let _ = UpdateLayeredWindow(
                self.glass_hwnd,
                None,
                Some(&dst_pt),
                Some(&size),
                Some(self.surface.mem_dc),
                Some(&src_pt),
                COLORREF(0),
                Some(&blend),
                ULW_ALPHA,
            );
        }

        self.last_glass_win_rect = win_rect;
        self.glass_visible = true;
    }
}

pub fn desired_overlay_is_above_taskbar(tap_connected: bool) -> bool {
    !tap_connected
}

pub fn fallback_overlay_alpha(cfg: &AppConfig, base_alpha: f32, tap_connected: bool) -> f32 {
    if tap_connected {
        return base_alpha;
    }
    if cfg.surface_mode == SurfaceMode::ClearTint && cfg.tint_alpha == 0 {
        return 0.0;
    }
    (base_alpha * 0.20).clamp(0.0, 0.11)
}

impl TrayOverlayState {
    fn sync_overlay_zorder(&self, taskbar_hwnd: HWND, show_blur: bool, tap_connected: bool) {
        if self.glass_hwnd.0.is_null() {
            return;
        }
        let valid_glass_rect = self.last_glass_win_rect.right > self.last_glass_win_rect.left
            && self.last_glass_win_rect.bottom > self.last_glass_win_rect.top;
        if !valid_glass_rect {
            return;
        }
        unsafe {
            let tray_ex = GetWindowLongW(taskbar_hwnd, GWL_EXSTYLE) as u32;
            let tray_topmost = (tray_ex & WS_EX_TOPMOST.0) != 0;
            let glass_ex = GetWindowLongW(self.glass_hwnd, GWL_EXSTYLE) as u32;
            let glass_topmost = (glass_ex & WS_EX_TOPMOST.0) != 0;

            if tray_topmost != glass_topmost {
                let insert_band = if tray_topmost {
                    HWND_TOPMOST
                } else {
                    HWND_NOTOPMOST
                };
                let _ = SetWindowPos(
                    self.glass_hwnd,
                    Some(insert_band),
                    0,
                    0,
                    0,
                    0,
                    SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE,
                );
            }

            let glass_w = (self.last_glass_win_rect.right - self.last_glass_win_rect.left).max(2);
            let glass_h = (self.last_glass_win_rect.bottom - self.last_glass_win_rect.top).max(2);
            let glass_hidden = !IsWindowVisible(self.glass_hwnd).as_bool();

            if desired_overlay_is_above_taskbar(tap_connected) {
                for &blur_hw in &self.blur_hwnds {
                    if !blur_hw.0.is_null() && IsWindowVisible(blur_hw).as_bool() {
                        let _ = ShowWindow(blur_hw, SW_HIDE);
                    }
                }
                let prev_above_tray = GetWindow(taskbar_hwnd, GW_HWNDPREV).unwrap_or_default();
                if glass_hidden || prev_above_tray != self.glass_hwnd {
                    let top_of_band = if tray_topmost {
                        HWND_TOPMOST
                    } else {
                        HWND_NOTOPMOST
                    };
                    let prev_same_band = !prev_above_tray.0.is_null()
                        && prev_above_tray != self.glass_hwnd
                        && ((GetWindowLongW(prev_above_tray, GWL_EXSTYLE) as u32 & WS_EX_TOPMOST.0)
                            != 0)
                            == tray_topmost;
                    let insert_after = if prev_same_band {
                        prev_above_tray
                    } else {
                        top_of_band
                    };
                    let pos_res = SetWindowPos(
                        self.glass_hwnd,
                        Some(insert_after),
                        self.last_glass_win_rect.left,
                        self.last_glass_win_rect.top,
                        glass_w,
                        glass_h,
                        SWP_NOACTIVATE | SWP_SHOWWINDOW,
                    );
                    if pos_res.is_err() && insert_after != top_of_band {
                        let _ = SetWindowPos(
                            self.glass_hwnd,
                            Some(top_of_band),
                            self.last_glass_win_rect.left,
                            self.last_glass_win_rect.top,
                            glass_w,
                            glass_h,
                            SWP_NOACTIVATE | SWP_SHOWWINDOW,
                        );
                    }
                }
                return;
            }

            let after_tray = GetWindow(taskbar_hwnd, GW_HWNDNEXT).unwrap_or_default();
            if glass_hidden || after_tray != self.glass_hwnd {
                let _ = SetWindowPos(
                    self.glass_hwnd,
                    Some(taskbar_hwnd),
                    self.last_glass_win_rect.left,
                    self.last_glass_win_rect.top,
                    glass_w,
                    glass_h,
                    SWP_NOACTIVATE | SWP_SHOWWINDOW,
                );
            }

            if show_blur {
                let active_blur = self.last_blur_rects.len().min(self.blur_hwnds.len());
                let mut prev_hw = self.glass_hwnd;
                for i in 0..active_blur {
                    let blur_hw = self.blur_hwnds[i];
                    let blur_rc = self.last_blur_rects[i];
                    if blur_hw.0.is_null()
                        || blur_rc.right <= blur_rc.left
                        || blur_rc.bottom <= blur_rc.top
                    {
                        continue;
                    }
                    let blur_ex = GetWindowLongW(blur_hw, GWL_EXSTYLE) as u32;
                    let blur_topmost = (blur_ex & WS_EX_TOPMOST.0) != 0;
                    if tray_topmost != blur_topmost {
                        let insert_band = if tray_topmost {
                            HWND_TOPMOST
                        } else {
                            HWND_NOTOPMOST
                        };
                        let _ = SetWindowPos(
                            blur_hw,
                            Some(insert_band),
                            0,
                            0,
                            0,
                            0,
                            SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE,
                        );
                    }

                    let blur_hidden = !IsWindowVisible(blur_hw).as_bool();
                    let after_prev = GetWindow(prev_hw, GW_HWNDNEXT).unwrap_or_default();
                    if blur_hidden || after_prev != blur_hw {
                        let _ = SetWindowPos(
                            blur_hw,
                            Some(prev_hw),
                            blur_rc.left,
                            blur_rc.top,
                            (blur_rc.right - blur_rc.left).max(4),
                            (blur_rc.bottom - blur_rc.top).max(4),
                            SWP_NOACTIVATE | SWP_SHOWWINDOW,
                        );
                    }
                    prev_hw = blur_hw;
                }
            }
        }
    }
}

fn wants_overlay(cfg: &AppConfig) -> bool {
    let has_viz = cfg.tb_viz_style != TaskbarVizStyle::Off;
    match cfg.pill_mode {
        PillMode::NativeDefault => has_viz,
        PillMode::FullBar => cfg.surface_mode != SurfaceMode::SystemDefault || cfg.amber_border || has_viz,
        PillMode::SplitPills | PillMode::UnifiedPill => true,
    }
}

fn wants_blur_backdrop(cfg: &AppConfig) -> bool {
    if !cfg.tb_show_backdrop
        || cfg.tb_theme.is_independent_tile_dock()
        || matches!(
            cfg.tb_theme,
            TaskbarTheme::CyberArasakaDeck
                | TaskbarTheme::Retro95Classic
                | TaskbarTheme::BorderlessGhost
        )
    {
        return false;
    }
    match cfg.pill_mode {
        PillMode::NativeDefault => false,
        PillMode::FullBar => matches!(cfg.surface_mode, SurfaceMode::Acrylic | SurfaceMode::Blur),
        PillMode::SplitPills | PillMode::UnifiedPill => {
            matches!(
                cfg.surface_mode,
                SurfaceMode::Acrylic | SurfaceMode::Blur | SurfaceMode::SystemDefault
            )
        }
    }
}

fn query_single_tray_geometry(hw: HWND) -> Option<(HWND, RECT, f32)> {
    unsafe {
        if hw.0.is_null() || !IsWindowVisible(hw).as_bool() {
            return None;
        }
        let mut bar_rect = RECT::default();
        if GetWindowRect(hw, &mut bar_rect).is_err() {
            return None;
        }
        let bar_w = bar_rect.right - bar_rect.left;
        let bar_h = bar_rect.bottom - bar_rect.top;
        if bar_w <= 6 || bar_h <= 6 {
            return None;
        }
        let mon_rect = get_monitor_rect(hw);
        let vis_w = (bar_rect.right.min(mon_rect.right) - bar_rect.left.max(mon_rect.left)).max(0);
        let vis_h = (bar_rect.bottom.min(mon_rect.bottom) - bar_rect.top.max(mon_rect.top)).max(0);
        if vis_w <= 6 || vis_h <= 6 {
            return None;
        }
        Some((hw, bar_rect, get_hwnd_dpi_scale(hw)))
    }
}

fn query_all_tray_geometries() -> Vec<(HWND, RECT, f32)> {
    let mut out = Vec::new();
    for hw in crate::taskbar_hide::TaskbarHider::collect_taskbar_hwnds() {
        if let Some(item) = query_single_tray_geometry(hw) {
            out.push(item);
        }
    }
    out
}

fn get_hwnd_dpi_scale(hwnd: HWND) -> f32 {
    unsafe {
        let raw_dpi = if hwnd.0.is_null() { 144 } else { GetDpiForWindow(hwnd) };
        if raw_dpi == 0 { 1.5 } else { raw_dpi as f32 / 96.0 }
    }
}

fn adaptive_cross_inset(bar_rect: RECT, u: f32) -> i32 {
    let cross_dim = (bar_rect.right - bar_rect.left).min(bar_rect.bottom - bar_rect.top);
    if cross_dim <= (38.0 * u).round() as i32 {
        (2.5 * u).round().max(2.0) as i32
    } else {
        (4.5 * u).round().max(3.0) as i32
    }
}

fn fallback_inset_rect(bar_rect: RECT, u: f32) -> RECT {
    let bar_w = bar_rect.right - bar_rect.left;
    let bar_h = bar_rect.bottom - bar_rect.top;
    let inset_cross = adaptive_cross_inset(bar_rect, u);
    let pad_edge = (2.0 * u).round().max(2.0) as i32;
    if bar_h > bar_w * 2 {
        RECT {
            left: bar_rect.left + inset_cross,
            top: bar_rect.top + pad_edge,
            right: (bar_rect.right - inset_cross).max(bar_rect.left + inset_cross + 10),
            bottom: (bar_rect.bottom - pad_edge).max(bar_rect.top + pad_edge + 10),
        }
    } else {
        RECT {
            left: bar_rect.left + pad_edge,
            top: bar_rect.top + inset_cross,
            right: (bar_rect.right - pad_edge).max(bar_rect.left + pad_edge + 10),
            bottom: (bar_rect.bottom - inset_cross).max(bar_rect.top + inset_cross + 10),
        }
    }
}

fn effective_pill_radius_for(
    screen_pills: &[RECT],
    scaled_radius: i32,
    u: f32,
    theme: TaskbarTheme,
) -> f32 {
    if theme == TaskbarTheme::Retro95Classic {
        return 0.0;
    }
    let min_dim = screen_pills
        .iter()
        .map(|r| (r.right - r.left).min(r.bottom - r.top))
        .min()
        .unwrap_or(56) as f32;
    match theme {
        TaskbarTheme::UnixPolybar => (6.0 * u).min(min_dim * 0.28).max(2.0),
        TaskbarTheme::CyberArasakaDeck => (scaled_radius as f32).min(min_dim * 0.34).max(5.0 * u),
        TaskbarTheme::Os26LiquidGlassLight
        | TaskbarTheme::Os26LiquidGlassDark
        | TaskbarTheme::OneUiLightChips
        | TaskbarTheme::OneUiDarkChips
        | TaskbarTheme::BubblesDark
        | TaskbarTheme::BubblesLight
        | TaskbarTheme::LiquidGlassClear => {
            (scaled_radius as f32).min(min_dim * 0.49).min(25.0 * u).max(12.0 * u)
        }
        TaskbarTheme::QuartzShellDock
        | TaskbarTheme::CupertinoDock
        | TaskbarTheme::KineticCupertinoWave
        | TaskbarTheme::DynamicIslandDock
        | TaskbarTheme::KineticLiquidMercury
        | TaskbarTheme::KineticHydroChamber
        | TaskbarTheme::LuminosityFloat
        | TaskbarTheme::BorderlessGhost => {
            (scaled_radius as f32).min(min_dim * 0.46).min(22.0 * u).max(8.0 * u)
        }
        TaskbarTheme::PillsDetached => {
            (scaled_radius as f32).min(min_dim * 0.42).min(16.0 * u).max(8.0 * u)
        }
        TaskbarTheme::MatterTilesLight
        | TaskbarTheme::MatterTilesDark
        | TaskbarTheme::SquircleFrosted
        | TaskbarTheme::RosePineTiles
        | TaskbarTheme::CatppuccinTiles => {
            (scaled_radius as f32).min(min_dim * 0.34).min(13.0 * u).max(6.0 * u)
        }
        TaskbarTheme::DockLikeLight | TaskbarTheme::DockLikeDark => {
            (scaled_radius as f32).min(min_dim * 0.28).min(10.5 * u).max(5.0 * u)
        }
        _ => (scaled_radius as f32).min(min_dim * 0.42).min(15.5 * u).max(4.0),
    }
}

fn dock_adjusted_rect(
    sr: RECT,
    bar_rect: RECT,
    is_vertical: bool,
    anchor_min: bool,
    u: f32,
    theme: TaskbarTheme,
) -> RECT {
    let is_lifted_dock = matches!(
        theme,
        TaskbarTheme::QuartzShellDock
            | TaskbarTheme::CupertinoDock
            | TaskbarTheme::KineticCupertinoWave
            | TaskbarTheme::Os26LiquidGlassLight
            | TaskbarTheme::Os26LiquidGlassDark
            | TaskbarTheme::OneUiLightChips
            | TaskbarTheme::OneUiDarkChips
            | TaskbarTheme::LiquidGlassClear
            | TaskbarTheme::LuminosityFloat
    );
    if !is_lifted_dock {
        return sr;
    }
    let lift = (2.2 * u).round() as i32;
    let wing = (2.5 * u).round() as i32;
    let mut r = sr;
    if is_vertical {
        r.top = (r.top - wing).max(bar_rect.top + 2);
        r.bottom = (r.bottom + wing).min(bar_rect.bottom - 2);
        if anchor_min {
            r.left = (r.left + lift).min(r.right - 14);
        } else {
            r.right = (r.right - lift).max(r.left + 14);
        }
    } else {
        r.left = (r.left - wing).max(bar_rect.left + 2);
        r.right = (r.right + wing).min(bar_rect.right - 2);
        if anchor_min {
            r.top = (r.top + lift).min(r.bottom - 14);
        } else {
            r.bottom = (r.bottom - lift).max(r.top + 14);
        }
    }
    r
}

unsafe fn create_chamfer_geo(
    d2d: &ID2D1Factory,
    l: f32,
    t: f32,
    r: f32,
    b: f32,
    cut: f32,
) -> Option<ID2D1PathGeometry> {
    let w = (r - l).max(4.0);
    let h = (b - t).max(4.0);
    let c = cut.clamp(2.0, (w * 0.35).min(h * 0.38));
    let pts = [
        Vector2 { X: l + c, Y: t },
        Vector2 { X: r - c, Y: t },
        Vector2 { X: r, Y: t + c },
        Vector2 { X: r, Y: b - c },
        Vector2 { X: r - c, Y: b },
        Vector2 { X: l + c, Y: b },
        Vector2 { X: l, Y: b - c },
        Vector2 { X: l, Y: t + c },
    ];
    let geo = d2d.CreatePathGeometry().ok()?;
    let sink = geo.Open().ok()?;
    sink.BeginFigure(pts[0], D2D1_FIGURE_BEGIN_FILLED);
    sink.AddLines(&pts[1..]);
    sink.EndFigure(D2D1_FIGURE_END_CLOSED);
    sink.Close().ok()?;
    Some(geo)
}

#[allow(clippy::too_many_arguments)]
unsafe fn draw_capsule_backdrop(
    rt: &ID2D1RenderTarget,
    d2d: Option<&ID2D1Factory>,
    l: f32,
    t: f32,
    r: f32,
    b: f32,
    rad: f32,
    u: f32,
    bounds_w: f32,
    bounds_h: f32,
    is_full_bar: bool,
    above_taskbar: bool,
    cap_idx: usize,
    is_preview: bool,
    sys_light: bool,
    cfg: &AppConfig,
    pal: &ThemePalette,
) {
    if cfg.pill_mode == PillMode::NativeDefault {
        return;
    }
    let fill_alpha = (cfg.tint_alpha as f32 / 255.0).clamp(0.0, 0.96);
    let effective_alpha = match cfg.surface_mode {
        SurfaceMode::ClearTint => fill_alpha,
        SurfaceMode::Acrylic => fill_alpha * 0.72,
        SurfaceMode::Blur => fill_alpha * 0.58,
        SurfaceMode::SystemDefault => (fill_alpha * 0.80).clamp(0.10, 0.92),
    };

    // Zero-bar Windhawk independent floating tile/circle themes:
    // App icons (`cap_idx == 0`) float as standalone tiles directly on wallpaper — NO enclosing bar!
    if cfg.tb_theme.is_independent_tile_dock() && cap_idx == 0 && !is_full_bar {
        return;
    }

    // Borderless Ghost: zero opaque bar, only a feather-soft shadow cushion behind icons
    if cfg.tb_theme == TaskbarTheme::BorderlessGhost {
        if !above_taskbar {
            let cushion_layers: [(f32, f32); 2] = [(4.0 * u, 0.08), (1.0 * u, 0.14)];
            for (exp, sh_a) in cushion_layers {
                let c_rr = D2D1_ROUNDED_RECT {
                    rect: D2D_RECT_F {
                        left: (l - exp).max(0.5),
                        top: (t - exp * 0.5).max(0.5),
                        right: (r + exp).min(bounds_w - 0.5),
                        bottom: (b + exp * 0.5).min(bounds_h - 0.5),
                    },
                    radiusX: rad + exp,
                    radiusY: rad + exp,
                };
                if let Ok(sh_br) = rt.CreateSolidColorBrush(&rgba_u8(6, 10, 18, sh_a), None) {
                    rt.FillRoundedRectangle(&c_rr, &sh_br);
                }
            }
        }
        return;
    }

    // Contained soft floating shadow for capsule pills (suppressed when layered above opaque taskbar)
    if !above_taskbar
        && !is_full_bar
        && !matches!(
            cfg.tb_theme,
            TaskbarTheme::Os26LiquidGlassLight
                | TaskbarTheme::Os26LiquidGlassDark
                | TaskbarTheme::LiquidGlassClear
        )
        && (fill_alpha > 0.04 || cfg.amber_border)
    {
        let is_mac_dock = matches!(
            cfg.tb_theme,
            TaskbarTheme::QuartzShellDock
                | TaskbarTheme::CupertinoDock
                | TaskbarTheme::KineticCupertinoWave
                | TaskbarTheme::DockLikeLight
                | TaskbarTheme::DockLikeDark
                | TaskbarTheme::LuminosityFloat
        );
        let sh_layers: [(f32, f32); 2] = if cfg.tb_theme.is_light_surface() {
            [(3.0 * u, 0.12), (1.4 * u, 0.18)]
        } else if is_mac_dock {
            [(3.6 * u, 0.22), (1.8 * u, 0.30)]
        } else {
            [(3.0 * u, 0.14), (1.5 * u, 0.20)]
        };
        for (exp, sh_a) in sh_layers {
            let sh_rr = D2D1_ROUNDED_RECT {
                rect: D2D_RECT_F {
                    left: (l - exp).max(0.5),
                    top: (t - exp + 0.5 * u).max(0.5),
                    right: (r + exp).min(bounds_w - 0.5),
                    bottom: (b + exp + 1.0 * u).min(bounds_h - 0.5),
                },
                radiusX: rad + exp,
                radiusY: rad + exp,
            };
            if let Ok(sh_br) = rt.CreateSolidColorBrush(
                &D2D1_COLOR_F { r: 0.02, g: 0.02, b: 0.03, a: sh_a },
                None,
            ) {
                rt.FillRoundedRectangle(&sh_rr, &sh_br);
            }
        }
    }

    let rr = D2D1_ROUNDED_RECT {
        rect: D2D_RECT_F { left: l, top: t, right: r, bottom: b },
        radiusX: if is_full_bar { 0.0 } else { rad },
        radiusY: if is_full_bar { 0.0 } else { rad },
    };

    // Bespoke OS & Clean/Minimal Dock surface fills
    let bespoke_alpha = |normal: f32| -> f32 {
        if above_taskbar {
            (fill_alpha * 0.16).clamp(0.05, 0.14)
        } else {
            normal
        }
    };
    // Protect white native Windows tray clock/icons when using a Light theme on Windows Dark Mode
    let dark_mode_tray_guard = !is_preview
        && !sys_light
        && cfg.tb_theme.is_light_surface()
        && (cap_idx >= 1 || cfg.pill_mode != PillMode::SplitPills);

    match cfg.tb_theme {
        // 1 & 2. OS26 Liquid Glass (Issue #1 Image #1): ultra-clear thin outer rail framing glass keycaps
        TaskbarTheme::Os26LiquidGlassLight => {
            let a = bespoke_alpha((fill_alpha * 0.42).clamp(0.14, 0.36));
            let stops = if dark_mode_tray_guard {
                [
                    D2D1_GRADIENT_STOP { position: 0.0, color: rgba_u8(62, 74, 94, 0.62) },
                    D2D1_GRADIENT_STOP { position: 1.0, color: rgba_u8(38, 46, 62, 0.72) },
                ]
            } else {
                [
                    D2D1_GRADIENT_STOP { position: 0.0, color: rgba_u8(255, 255, 255, a * 1.25) },
                    D2D1_GRADIENT_STOP { position: 1.0, color: rgba_u8(232, 242, 255, a * 0.85) },
                ]
            };
            if let Some(br) = create_grad(rt, Vector2 { X: l, Y: t }, Vector2 { X: l, Y: b }, &stops) {
                rt.FillRoundedRectangle(&rr, &br);
            }
            return;
        }
        TaskbarTheme::Os26LiquidGlassDark => {
            let a = bespoke_alpha((fill_alpha * 0.38).clamp(0.10, 0.30));
            let stops = [
                D2D1_GRADIENT_STOP { position: 0.0, color: rgba_u8(22, 36, 56, a) },
                D2D1_GRADIENT_STOP { position: 1.0, color: rgba_u8(8, 16, 28, (a * 1.35).min(0.40)) },
            ];
            if let Some(br) = create_grad(rt, Vector2 { X: l, Y: t }, Vector2 { X: l, Y: b }, &stops) {
                rt.FillRoundedRectangle(&rr, &br);
            }
            return;
        }
        // 3 & 4. Matter Tiles Light & Dark (Issue #1 Image #2): tray capsule only (cap_idx >= 1)
        TaskbarTheme::MatterTilesLight => {
            let a = bespoke_alpha(fill_alpha.clamp(0.65, 0.95));
            let col = if dark_mode_tray_guard {
                rgba_u8(54, 48, 66, a)
            } else {
                rgba_u8(244, 240, 250, a)
            };
            if let Ok(br) = rt.CreateSolidColorBrush(&col, None) {
                rt.FillRoundedRectangle(&rr, &br);
            }
            return;
        }
        TaskbarTheme::MatterTilesDark => {
            let a = bespoke_alpha(fill_alpha.clamp(0.68, 0.96));
            if let Ok(br) = rt.CreateSolidColorBrush(&rgba_u8(44, 34, 48, a), None) {
                rt.FillRoundedRectangle(&rr, &br);
            }
            return;
        }
        // 5 & 6. One UI 8.5 Light & Dark Chips: sleek glass rail + vibrant clock chip accent on tray
        TaskbarTheme::OneUiLightChips => {
            let a = bespoke_alpha(fill_alpha.clamp(0.28, 0.68));
            let stops = [
                D2D1_GRADIENT_STOP { position: 0.0, color: rgba_u8(26, 30, 40, a * 0.85) },
                D2D1_GRADIENT_STOP { position: 1.0, color: rgba_u8(14, 16, 22, a) },
            ];
            if let Some(br) = create_grad(rt, Vector2 { X: l, Y: t }, Vector2 { X: l, Y: b }, &stops) {
                rt.FillRoundedRectangle(&rr, &br);
            }
            return;
        }
        TaskbarTheme::OneUiDarkChips => {
            let a = bespoke_alpha(fill_alpha.clamp(0.32, 0.78));
            let stops = if dark_mode_tray_guard {
                [
                    D2D1_GRADIENT_STOP { position: 0.0, color: rgba_u8(62, 70, 86, 0.82) },
                    D2D1_GRADIENT_STOP { position: 1.0, color: rgba_u8(40, 46, 58, 0.88) },
                ]
            } else {
                [
                    D2D1_GRADIENT_STOP { position: 0.0, color: rgba_u8(248, 250, 254, a * 0.92) },
                    D2D1_GRADIENT_STOP { position: 1.0, color: rgba_u8(226, 234, 246, a * 0.84) },
                ]
            };
            if let Some(br) = create_grad(rt, Vector2 { X: l, Y: t }, Vector2 { X: l, Y: b }, &stops) {
                rt.FillRoundedRectangle(&rr, &br);
            }
            return;
        }
        // 7..12. Independent floating tiles/circles: tray pill surfaces (`cap_idx >= 1`)
        TaskbarTheme::SquircleFrosted => {
            let a = bespoke_alpha(fill_alpha.clamp(0.55, 0.88));
            if let Ok(br) = rt.CreateSolidColorBrush(&rgba_u8(46, 36, 30, a), None) {
                rt.FillRoundedRectangle(&rr, &br);
            }
            return;
        }
        TaskbarTheme::BubblesDark => {
            let a = bespoke_alpha(fill_alpha.clamp(0.72, 0.96));
            if let Ok(br) = rt.CreateSolidColorBrush(&rgba_u8(40, 42, 48, a), None) {
                rt.FillRoundedRectangle(&rr, &br);
            }
            return;
        }
        TaskbarTheme::BubblesLight => {
            let a = bespoke_alpha(fill_alpha.clamp(0.68, 0.94));
            let col = if dark_mode_tray_guard {
                rgba_u8(54, 62, 76, a)
            } else {
                rgba_u8(248, 250, 254, a)
            };
            if let Ok(br) = rt.CreateSolidColorBrush(&col, None) {
                rt.FillRoundedRectangle(&rr, &br);
            }
            return;
        }
        TaskbarTheme::PillsDetached => {
            let a = bespoke_alpha(fill_alpha.clamp(0.56, 0.88));
            let stops = [
                D2D1_GRADIENT_STOP { position: 0.0, color: rgba_u8(42, 52, 64, a) },
                D2D1_GRADIENT_STOP { position: 1.0, color: rgba_u8(24, 32, 42, (a + 0.06).min(0.94)) },
            ];
            if let Some(br) = create_grad(rt, Vector2 { X: l, Y: t }, Vector2 { X: l, Y: b }, &stops) {
                rt.FillRoundedRectangle(&rr, &br);
            }
            return;
        }
        TaskbarTheme::RosePineTiles => {
            let a = bespoke_alpha(fill_alpha.clamp(0.74, 0.96));
            if let Ok(br) = rt.CreateSolidColorBrush(&rgba_u8(38, 35, 58, a), None) {
                rt.FillRoundedRectangle(&rr, &br);
            }
            return;
        }
        TaskbarTheme::CatppuccinTiles => {
            let a = bespoke_alpha(fill_alpha.clamp(0.74, 0.96));
            if let Ok(br) = rt.CreateSolidColorBrush(&rgba_u8(30, 30, 46, a), None) {
                rt.FillRoundedRectangle(&rr, &br);
            }
            return;
        }
        // 13. Liquid Glass Clear (Windhawk LiquidGlass2): ultra-clear curved glass capsule
        TaskbarTheme::LiquidGlassClear => {
            let a = bespoke_alpha((fill_alpha * 0.42).clamp(0.10, 0.28));
            let stops = [
                D2D1_GRADIENT_STOP { position: 0.0, color: rgba_u8(255, 255, 255, (a * 1.45).min(0.36)) },
                D2D1_GRADIENT_STOP { position: 0.46, color: rgba_u8(26, 32, 28, a) },
                D2D1_GRADIENT_STOP { position: 1.0, color: rgba_u8(240, 248, 242, (a * 1.15).min(0.30)) },
            ];
            if let Some(br) = create_grad(rt, Vector2 { X: l, Y: t }, Vector2 { X: l, Y: b }, &stops) {
                rt.FillRoundedRectangle(&rr, &br);
            }
            return;
        }
        // 14 & 15. DockLike Light & Dark (Windhawk DockLike)
        TaskbarTheme::DockLikeLight => {
            let a = bespoke_alpha(fill_alpha.clamp(0.58, 0.94));
            let stops = if dark_mode_tray_guard {
                [
                    D2D1_GRADIENT_STOP { position: 0.0, color: rgba_u8(62, 70, 86, 0.88) },
                    D2D1_GRADIENT_STOP { position: 1.0, color: rgba_u8(42, 48, 62, 0.92) },
                ]
            } else {
                [
                    D2D1_GRADIENT_STOP { position: 0.0, color: rgba_u8(248, 250, 254, a) },
                    D2D1_GRADIENT_STOP { position: 1.0, color: rgba_u8(234, 239, 248, a) },
                ]
            };
            if let Some(br) = create_grad(rt, Vector2 { X: l, Y: t }, Vector2 { X: l, Y: b }, &stops) {
                rt.FillRoundedRectangle(&rr, &br);
            }
            return;
        }
        TaskbarTheme::DockLikeDark => {
            let a = bespoke_alpha(fill_alpha.clamp(0.72, 0.96));
            let stops = [
                D2D1_GRADIENT_STOP { position: 0.0, color: rgba_u8(30, 38, 54, a) },
                D2D1_GRADIENT_STOP { position: 1.0, color: rgba_u8(22, 28, 42, a) },
            ];
            if let Some(br) = create_grad(rt, Vector2 { X: l, Y: t }, Vector2 { X: l, Y: b }, &stops) {
                rt.FillRoundedRectangle(&rr, &br);
            }
            return;
        }
        // 16. Blob Light (Windhawk Blob/BlobLight): soft pale ice-gray ribbon with white pill blobs
        TaskbarTheme::BlobLight => {
            let a = bespoke_alpha(fill_alpha.clamp(0.62, 0.92));
            let stops = if dark_mode_tray_guard {
                [
                    D2D1_GRADIENT_STOP { position: 0.0, color: rgba_u8(64, 72, 86, 0.86) },
                    D2D1_GRADIENT_STOP { position: 1.0, color: rgba_u8(48, 54, 66, 0.90) },
                ]
            } else {
                [
                    D2D1_GRADIENT_STOP { position: 0.0, color: rgba_u8(226, 234, 246, a) },
                    D2D1_GRADIENT_STOP { position: 1.0, color: rgba_u8(212, 222, 238, a) },
                ]
            };
            if let Some(br) = create_grad(rt, Vector2 { X: l, Y: t }, Vector2 { X: l, Y: b }, &stops) {
                rt.FillRoundedRectangle(&rr, &br);
            }
            return;
        }
        // 17. Luminosity Float (Windhawk Luminosity): wide centered wallpaper-blended glass capsule
        TaskbarTheme::LuminosityFloat => {
            let a = bespoke_alpha((fill_alpha * 0.72).clamp(0.32, 0.74));
            let (tr, tg, tb) = pal.glass_top;
            let (br_c, bg_c, bb_c) = pal.glass_bot;
            let (ar, ag, ab) = pal.accent;
            let mr = ((tr as u16 * 2 + ar as u16) / 3) as u8;
            let mg = ((tg as u16 * 2 + ag as u16) / 3) as u8;
            let mb = ((tb as u16 * 2 + ab as u16) / 3) as u8;
            let stops = [
                D2D1_GRADIENT_STOP { position: 0.0, color: rgba_u8(tr, tg, tb, a) },
                D2D1_GRADIENT_STOP { position: 0.45, color: rgba_u8(mr, mg, mb, (a * 1.08).min(0.82)) },
                D2D1_GRADIENT_STOP { position: 1.0, color: rgba_u8(br_c, bg_c, bb_c, (a * 1.04).min(0.80)) },
            ];
            if let Some(br) = create_grad(rt, Vector2 { X: l, Y: t }, Vector2 { X: r, Y: b }, &stops) {
                rt.FillRoundedRectangle(&rr, &br);
            }
            return;
        }
        // 19. Aeris Glass Light (Windhawk Aeris): crisp edge-to-edge icy frosted ribbon
        TaskbarTheme::AerisGlassLight => {
            let a = bespoke_alpha(fill_alpha.clamp(0.55, 0.90));
            let stops = if dark_mode_tray_guard {
                [
                    D2D1_GRADIENT_STOP { position: 0.0, color: rgba_u8(48, 66, 86, 0.84) },
                    D2D1_GRADIENT_STOP { position: 1.0, color: rgba_u8(34, 48, 66, 0.88) },
                ]
            } else {
                [
                    D2D1_GRADIENT_STOP { position: 0.0, color: rgba_u8(222, 242, 255, a) },
                    D2D1_GRADIENT_STOP { position: 1.0, color: rgba_u8(204, 232, 248, a) },
                ]
            };
            if let Some(br) = create_grad(rt, Vector2 { X: l, Y: t }, Vector2 { X: r, Y: b }, &stops) {
                rt.FillRoundedRectangle(&rr, &br);
            }
            return;
        }
        // 20. Clean Slate Cards (Windhawk CleanSlate): deep translucent teal-slate edge glass bar
        TaskbarTheme::CleanSlateCards => {
            let a = bespoke_alpha(fill_alpha.clamp(0.46, 0.84));
            let stops = [
                D2D1_GRADIENT_STOP { position: 0.0, color: rgba_u8(22, 58, 74, a) },
                D2D1_GRADIENT_STOP { position: 1.0, color: rgba_u8(14, 42, 56, (a + 0.06).min(0.90)) },
            ];
            if let Some(br) = create_grad(rt, Vector2 { X: l, Y: t }, Vector2 { X: l, Y: b }, &stops) {
                rt.FillRoundedRectangle(&rr, &br);
            }
            return;
        }
        TaskbarTheme::Retro95Classic => {
            if let Ok(br) = rt.CreateSolidColorBrush(
                &rgba_u8(68, 74, 86, bespoke_alpha(fill_alpha.max(0.88))),
                None,
            ) {
                rt.FillRoundedRectangle(&rr, &br);
            }
            return;
        }
        TaskbarTheme::FrutigerAero7 => {
            let a = bespoke_alpha(fill_alpha.max(0.46).min(0.86));
            let aero_stops = [
                D2D1_GRADIENT_STOP { position: 0.0, color: rgba_u8(32, 146, 198, a * 0.88) },
                D2D1_GRADIENT_STOP { position: 0.48, color: rgba_u8(14, 84, 134, a * 0.92) },
                D2D1_GRADIENT_STOP { position: 0.49, color: rgba_u8(8, 52, 92, a * 0.98) },
                D2D1_GRADIENT_STOP { position: 1.0, color: rgba_u8(14, 72, 118, a * 0.92) },
            ];
            if let Some(br) = create_grad(rt, Vector2 { X: l, Y: t }, Vector2 { X: l, Y: b }, &aero_stops) {
                rt.FillRoundedRectangle(&rr, &br);
            }
            // Signature Win7 glassy white upper-half reflection band
            let clip_guard = push_capsule_clip(rt, d2d, l, t, r, b, if is_full_bar { 0.0 } else { rad });
            let mid_y = t + (b - t) * 0.48;
            let (gl_top, gl_bot) = if above_taskbar { (0.09, 0.03) } else { (0.34, 0.11) };
            let gloss_stops = [
                D2D1_GRADIENT_STOP { position: 0.0, color: rgba_u8(255, 255, 255, gl_top) },
                D2D1_GRADIENT_STOP { position: 1.0, color: rgba_u8(210, 246, 255, gl_bot) },
            ];
            if let Some(gbr) = create_grad(rt, Vector2 { X: l, Y: t }, Vector2 { X: l, Y: mid_y }, &gloss_stops) {
                rt.FillRectangle(&D2D_RECT_F { left: l, top: t, right: r, bottom: mid_y }, &gbr);
            }
            pop_capsule_clip(rt, clip_guard);
            return;
        }
        TaskbarTheme::UnixPolybar => {
            let a = bespoke_alpha(fill_alpha.max(0.78));
            let u_stops = [
                D2D1_GRADIENT_STOP { position: 0.0, color: rgba_u8(28, 28, 44, a) },
                D2D1_GRADIENT_STOP { position: 1.0, color: rgba_u8(20, 20, 32, a) },
            ];
            if let Some(br) = create_grad(rt, Vector2 { X: l, Y: t }, Vector2 { X: l, Y: b }, &u_stops) {
                rt.FillRoundedRectangle(&rr, &br);
            }
            return;
        }
        TaskbarTheme::CyberArasakaDeck => {
            let a = bespoke_alpha(fill_alpha.max(0.76));
            let c_stops = [
                D2D1_GRADIENT_STOP { position: 0.0, color: rgba_u8(16, 18, 28, a) },
                D2D1_GRADIENT_STOP { position: 1.0, color: rgba_u8(10, 12, 20, bespoke_alpha((a + 0.06).min(0.96))) },
            ];
            if let Some(br) = create_grad(rt, Vector2 { X: l, Y: t }, Vector2 { X: r, Y: b }, &c_stops) {
                if let Some(factory) = d2d {
                    if let Some(geo) = create_chamfer_geo(factory, l, t, r, b, rad.max(6.0 * u)) {
                        rt.FillGeometry(&geo, &br, None);
                        return;
                    }
                }
                rt.FillRoundedRectangle(&rr, &br);
            }
            return;
        }
        TaskbarTheme::QuartzShellDock
        | TaskbarTheme::CupertinoDock
        | TaskbarTheme::KineticCupertinoWave => {
            let a = bespoke_alpha(effective_alpha.max(0.28));
            let q_stops = [
                D2D1_GRADIENT_STOP { position: 0.0, color: rgba_u8(220, 230, 246, (a * 0.56).clamp(0.04, 0.42)) },
                D2D1_GRADIENT_STOP { position: 0.38, color: rgba_u8(36, 42, 56, (a * 0.92).min(0.78)) },
                D2D1_GRADIENT_STOP { position: 1.0, color: rgba_u8(20, 24, 34, (a * 0.98).min(0.84)) },
            ];
            if let Some(br) = create_grad(rt, Vector2 { X: l, Y: t }, Vector2 { X: l, Y: b }, &q_stops) {
                rt.FillRoundedRectangle(&rr, &br);
            }
            return;
        }
        TaskbarTheme::DynamicIslandDock => {
            let a = bespoke_alpha(fill_alpha.max(0.84));
            let d_stops = [
                D2D1_GRADIENT_STOP { position: 0.0, color: rgba_u8(18, 20, 26, a) },
                D2D1_GRADIENT_STOP { position: 1.0, color: rgba_u8(9, 10, 14, bespoke_alpha((a + 0.06).min(0.98))) },
            ];
            if let Some(br) = create_grad(rt, Vector2 { X: l, Y: t }, Vector2 { X: l, Y: b }, &d_stops) {
                rt.FillRoundedRectangle(&rr, &br);
            }
            return;
        }
        _ => {}
    }

    let surface_alpha = fallback_overlay_alpha(cfg, effective_alpha, !above_taskbar);
    if surface_alpha > 0.005 {
        let use_pal_glass = ThemePalette::for_music_taskbar_theme(cfg.tb_theme).is_some()
            || cfg.tb_viz_color_mode == VizColorMode::CustomAccent;
        let bg_stops = if use_pal_glass {
            let (tr, tg, tb) = pal.glass_top;
            let (br, bg, bb) = pal.glass_bot;
            let mr = ((tr as u16 + br as u16) / 2) as u8;
            let mg = ((tg as u16 + bg as u16) / 2) as u8;
            let mb = ((tb as u16 + bb as u16) / 2) as u8;
            [
                D2D1_GRADIENT_STOP { position: 0.0, color: rgba_u8(tr, tg, tb, (surface_alpha + 0.04).min(0.96)) },
                D2D1_GRADIENT_STOP { position: 0.55, color: rgba_u8(mr, mg, mb, surface_alpha) },
                D2D1_GRADIENT_STOP { position: 1.0, color: rgba_u8(br, bg, bb, (surface_alpha * 0.90).max(0.0)) },
            ]
        } else {
            match cfg.surface_mode {
                SurfaceMode::SystemDefault => [
                    D2D1_GRADIENT_STOP { position: 0.0, color: rgba_u8(34, 32, 30, (surface_alpha + 0.04).min(0.96)) },
                    D2D1_GRADIENT_STOP { position: 0.55, color: rgba_u8(26, 25, 25, surface_alpha) },
                    D2D1_GRADIENT_STOP { position: 1.0, color: rgba_u8(20, 19, 19, (surface_alpha * 0.90).max(0.0)) },
                ],
                SurfaceMode::Blur => [
                    D2D1_GRADIENT_STOP { position: 0.0, color: rgba_u8(46, 38, 33, (surface_alpha + 0.04).min(0.90)) },
                    D2D1_GRADIENT_STOP { position: 0.55, color: rgba_u8(30, 24, 21, surface_alpha) },
                    D2D1_GRADIENT_STOP { position: 1.0, color: rgba_u8(24, 19, 17, (surface_alpha * 0.88).max(0.0)) },
                ],
                _ => [
                    D2D1_GRADIENT_STOP { position: 0.0, color: rgba_u8(20, 15, 13, (surface_alpha + 0.04).min(0.98)) },
                    D2D1_GRADIENT_STOP { position: 0.55, color: rgba_u8(15, 11, 10, surface_alpha) },
                    D2D1_GRADIENT_STOP { position: 1.0, color: rgba_u8(12, 9, 8, (surface_alpha * 0.88).max(0.0)) },
                ],
            }
        };
        if let Some(bg_br) = create_grad(rt, Vector2 { X: l, Y: t }, Vector2 { X: r, Y: b }, &bg_stops) {
            rt.FillRoundedRectangle(&rr, &bg_br);
        }
    }
}

#[allow(clippy::too_many_arguments)]
unsafe fn draw_clean_icon_pads(
    rt: &ID2D1RenderTarget,
    d2d: Option<&ID2D1Factory>,
    theme: TaskbarTheme,
    rects: &[(f32, f32, f32, f32)],
    u: f32,
    above_taskbar: bool,
    is_preview: bool,
    active_idx: Option<usize>,
) {
    if !theme.uses_icon_pads() || rects.is_empty() {
        return;
    }
    let alpha_mul = if above_taskbar { 0.20 } else { 1.0 };
    for (idx, &(il, it, ir, ib)) in rects.iter().enumerate() {
        let iw = ir - il;
        let ih = ib - it;
        if iw < 6.0 * u || ih < 6.0 * u {
            continue;
        }
        let min_s = iw.min(ih);
        let is_featured = active_idx == Some(idx);
        let rad = match theme {
            TaskbarTheme::BubblesDark | TaskbarTheme::BubblesLight => min_s * 0.50,
            TaskbarTheme::AerisGlassLight => 0.0,
            TaskbarTheme::CleanSlateCards => (min_s * 0.18).clamp(3.0 * u, 6.0 * u),
            TaskbarTheme::BlobLight => min_s * 0.44,
            TaskbarTheme::OneUiLightChips | TaskbarTheme::OneUiDarkChips => {
                (min_s * 0.36).clamp(4.0 * u, 13.0 * u)
            }
            TaskbarTheme::Os26LiquidGlassLight | TaskbarTheme::Os26LiquidGlassDark => {
                (min_s * 0.31).clamp(4.0 * u, 13.0 * u)
            }
            TaskbarTheme::PillsDetached => (min_s * 0.36).clamp(4.0 * u, 12.0 * u),
            _ => (min_s * 0.27).clamp(3.5 * u, 11.0 * u),
        };
        let rr = D2D1_ROUNDED_RECT {
            rect: D2D_RECT_F { left: il, top: it, right: ir, bottom: ib },
            radiusX: rad,
            radiusY: rad,
        };
        match theme {
            // Exact Windhawk OS26 Liquid Glass (Issue #1 Image #1) — Light & Dark optical keycaps
            TaskbarTheme::Os26LiquidGlassDark | TaskbarTheme::Os26LiquidGlassLight => {
                let is_light = theme == TaskbarTheme::Os26LiquidGlassLight;
                if !above_taskbar {
                    let sh_a = if is_light { 0.14 } else { 0.28 };
                    if let Ok(sh) = rt.CreateSolidColorBrush(&rgba_u8(6, 12, 22, sh_a), None) {
                        let sh_rr = D2D1_ROUNDED_RECT {
                            rect: D2D_RECT_F {
                                left: il - 0.5 * u,
                                top: it + 0.9 * u,
                                right: ir + 0.5 * u,
                                bottom: ib + 1.4 * u,
                            },
                            radiusX: rad + 0.5 * u,
                            radiusY: rad + 0.5 * u,
                        };
                        rt.FillRoundedRectangle(&sh_rr, &sh);
                    }
                }
                let dome_stops = if is_light {
                    [
                        D2D1_GRADIENT_STOP { position: 0.0, color: rgba_u8(255, 255, 255, 0.88 * alpha_mul) },
                        D2D1_GRADIENT_STOP { position: 0.48, color: rgba_u8(226, 238, 252, 0.54 * alpha_mul) },
                        D2D1_GRADIENT_STOP { position: 1.0, color: rgba_u8(255, 255, 255, 0.76 * alpha_mul) },
                    ]
                } else {
                    [
                        D2D1_GRADIENT_STOP { position: 0.0, color: rgba_u8(186, 212, 240, 0.34 * alpha_mul) },
                        D2D1_GRADIENT_STOP { position: 0.46, color: rgba_u8(28, 42, 62, 0.48 * alpha_mul) },
                        D2D1_GRADIENT_STOP { position: 1.0, color: rgba_u8(146, 178, 214, 0.26 * alpha_mul) },
                    ]
                };
                if let Some(br) = create_grad(rt, Vector2 { X: il, Y: it }, Vector2 { X: il, Y: ib }, &dome_stops) {
                    rt.FillRoundedRectangle(&rr, &br);
                }
                // Clipped upper-left liquid glass glare sheen
                if !above_taskbar {
                    let clip_g = push_capsule_clip(rt, d2d, il, it, ir, ib, rad);
                    let glare_b = it + ih * 0.44;
                    let glare_stops = if is_light {
                        [
                            D2D1_GRADIENT_STOP { position: 0.0, color: rgba_u8(255, 255, 255, 0.54) },
                            D2D1_GRADIENT_STOP { position: 1.0, color: rgba_u8(255, 255, 255, 0.0) },
                        ]
                    } else {
                        [
                            D2D1_GRADIENT_STOP { position: 0.0, color: rgba_u8(240, 248, 255, 0.28) },
                            D2D1_GRADIENT_STOP { position: 1.0, color: rgba_u8(200, 224, 255, 0.0) },
                        ]
                    };
                    if let Some(gbr) = create_grad(rt, Vector2 { X: il, Y: it }, Vector2 { X: il, Y: glare_b }, &glare_stops) {
                        rt.FillRectangle(&D2D_RECT_F { left: il, top: it, right: ir, bottom: glare_b }, &gbr);
                    }
                    pop_capsule_clip(rt, clip_g);
                }
                // Signature OS26 Liquid Glass curved specular top + bottom bevel rim (transparent mid-sides)
                let rim_stops = if is_light {
                    [
                        D2D1_GRADIENT_STOP { position: 0.0, color: rgba_u8(255, 255, 255, 0.96 * alpha_mul) },
                        D2D1_GRADIENT_STOP { position: 0.50, color: rgba_u8(140, 172, 212, 0.28 * alpha_mul) },
                        D2D1_GRADIENT_STOP { position: 1.0, color: rgba_u8(255, 255, 255, 0.86 * alpha_mul) },
                    ]
                } else {
                    [
                        D2D1_GRADIENT_STOP { position: 0.0, color: rgba_u8(250, 253, 255, 0.82 * alpha_mul) },
                        D2D1_GRADIENT_STOP { position: 0.50, color: rgba_u8(160, 192, 226, 0.14 * alpha_mul) },
                        D2D1_GRADIENT_STOP { position: 1.0, color: rgba_u8(226, 240, 255, 0.58 * alpha_mul) },
                    ]
                };
                if let Some(rb) = create_grad(rt, Vector2 { X: il, Y: it }, Vector2 { X: il, Y: ib }, &rim_stops) {
                    rt.DrawRoundedRectangle(&rr, &rb, 1.1 * u, None);
                }
                // Running indicator dot right underneath keycap in Studio preview (matching OS26_Liquid_Glass.png)
                if is_preview && (idx == 1 || idx == 2 || idx == 3) {
                    let dot_col = if is_light {
                        rgba_u8(46, 62, 86, 0.82)
                    } else {
                        rgba_u8(230, 240, 252, 0.86)
                    };
                    if let Ok(dbr) = rt.CreateSolidColorBrush(&dot_col, None) {
                        rt.FillEllipse(
                            &D2D1_ELLIPSE {
                                point: Vector2 { X: (il + ir) * 0.5, Y: ib + 2.4 * u },
                                radiusX: 1.3 * u,
                                radiusY: 1.3 * u,
                            },
                            &dbr,
                        );
                    }
                }
            }
            // Exact Windhawk Matter Tiles Dark (Issue #1 Image #2) & Matter Tiles Light (Matter/TB_Light.png)
            TaskbarTheme::MatterTilesDark | TaskbarTheme::MatterTilesLight => {
                let is_light = theme == TaskbarTheme::MatterTilesLight;
                if !above_taskbar {
                    let sh_a = if is_light { 0.14 } else { 0.24 };
                    if let Ok(sh) = rt.CreateSolidColorBrush(&rgba_u8(14, 8, 22, sh_a), None) {
                        let sh_rr = D2D1_ROUNDED_RECT {
                            rect: D2D_RECT_F {
                                left: il,
                                top: it + 0.8 * u,
                                right: ir,
                                bottom: ib + 1.2 * u,
                            },
                            radiusX: rad,
                            radiusY: rad,
                        };
                        rt.FillRoundedRectangle(&sh_rr, &sh);
                    }
                }
                let fill_col = if is_light {
                    rgba_u8(245, 242, 251, 0.94 * alpha_mul)
                } else {
                    rgba_u8(44, 34, 48, 0.93 * alpha_mul)
                };
                if let Ok(br) = rt.CreateSolidColorBrush(&fill_col, None) {
                    rt.FillRoundedRectangle(&rr, &br);
                }
                if is_light {
                    if let Ok(rim) = rt.CreateSolidColorBrush(&rgba_u8(60, 42, 82, 0.10 * alpha_mul), None) {
                        rt.DrawRoundedRectangle(&rr, &rim, 0.75 * u, None);
                    }
                }
                // Crisp Matter bottom pill indicator bar inside active tiles in Studio preview
                if is_preview && idx != 0 {
                    let pw = (iw * 0.36).clamp(4.0 * u, 9.0 * u);
                    let ph = 2.0 * u;
                    let cx = (il + ir) * 0.5;
                    let py = ib - ph - 1.2 * u;
                    let ind_col = if is_light {
                        if is_featured {
                            rgba_u8(216, 36, 46, 0.96)
                        } else {
                            rgba_u8(24, 22, 30, 0.92)
                        }
                    } else {
                        rgba_u8(248, 248, 252, 0.96)
                    };
                    if let Ok(ibr) = rt.CreateSolidColorBrush(&ind_col, None) {
                        rt.FillRoundedRectangle(
                            &D2D1_ROUNDED_RECT {
                                rect: D2D_RECT_F {
                                    left: cx - pw * 0.5,
                                    top: py,
                                    right: cx + pw * 0.5,
                                    bottom: py + ph,
                                },
                                radiusX: ph * 0.5,
                                radiusY: ph * 0.5,
                            },
                            &ibr,
                        );
                    }
                }
            }
            // Exact Windhawk One UI 8.5 Light Chips (white squircles in dark rail) & Dark Chips (obsidian in frost rail)
            TaskbarTheme::OneUiLightChips | TaskbarTheme::OneUiDarkChips => {
                let white_chips = theme == TaskbarTheme::OneUiLightChips;
                let col = if white_chips {
                    rgba_u8(246, 248, 252, 0.96 * alpha_mul)
                } else {
                    rgba_u8(24, 28, 38, 0.94 * alpha_mul)
                };
                if let Ok(br) = rt.CreateSolidColorBrush(&col, None) {
                    rt.FillRoundedRectangle(&rr, &br);
                }
                let rim_col = if white_chips {
                    rgba_u8(255, 255, 255, 0.85 * alpha_mul)
                } else {
                    rgba_u8(255, 255, 255, 0.16 * alpha_mul)
                };
                if let Ok(rim) = rt.CreateSolidColorBrush(&rim_col, None) {
                    rt.DrawRoundedRectangle(&rr, &rim, 0.8 * u, None);
                }
            }
            // Exact Windhawk Squircle: smoky translucent floating squircle islands
            TaskbarTheme::SquircleFrosted => {
                if let Ok(br) = rt.CreateSolidColorBrush(&rgba_u8(46, 36, 30, 0.78 * alpha_mul), None) {
                    rt.FillRoundedRectangle(&rr, &br);
                }
                if let Ok(rim) = rt.CreateSolidColorBrush(&rgba_u8(255, 232, 214, 0.16 * alpha_mul), None) {
                    rt.DrawRoundedRectangle(&rr, &rim, 0.85 * u, None);
                }
            }
            // Exact Windhawk Bubbles Dark & Light: standalone perfect circular buttons + focus ring
            TaskbarTheme::BubblesDark | TaskbarTheme::BubblesLight => {
                let is_light = theme == TaskbarTheme::BubblesLight;
                if !above_taskbar {
                    if let Ok(sh) = rt.CreateSolidColorBrush(&rgba_u8(8, 10, 16, if is_light { 0.14 } else { 0.28 }), None) {
                        let sh_rr = D2D1_ROUNDED_RECT {
                            rect: D2D_RECT_F { left: il, top: it + 0.8 * u, right: ir, bottom: ib + 1.2 * u },
                            radiusX: rad,
                            radiusY: rad,
                        };
                        rt.FillRoundedRectangle(&sh_rr, &sh);
                    }
                }
                let col = if is_light {
                    rgba_u8(248, 250, 255, 0.94 * alpha_mul)
                } else {
                    rgba_u8(40, 42, 48, 0.95 * alpha_mul)
                };
                if let Ok(br) = rt.CreateSolidColorBrush(&col, None) {
                    rt.FillRoundedRectangle(&rr, &br);
                }
                if is_featured {
                    let ring_col = if is_light {
                        rgba_u8(74, 146, 246, 0.92 * alpha_mul)
                    } else {
                        rgba_u8(134, 168, 231, 0.94 * alpha_mul)
                    };
                    if let Ok(rbr) = rt.CreateSolidColorBrush(&ring_col, None) {
                        rt.DrawRoundedRectangle(&rr, &rbr, 1.6 * u, None);
                    }
                } else {
                    let subtle = if is_light {
                        rgba_u8(48, 68, 102, 0.14 * alpha_mul)
                    } else {
                        rgba_u8(255, 255, 255, 0.10 * alpha_mul)
                    };
                    if let Ok(rbr) = rt.CreateSolidColorBrush(&subtle, None) {
                        rt.DrawRoundedRectangle(&rr, &rbr, 0.8 * u, None);
                    }
                }
            }
            // Exact Windhawk Pills Detached: independent dark-frosted bordered pill capsules
            TaskbarTheme::PillsDetached => {
                let stops = [
                    D2D1_GRADIENT_STOP { position: 0.0, color: rgba_u8(44, 54, 66, 0.82 * alpha_mul) },
                    D2D1_GRADIENT_STOP { position: 1.0, color: rgba_u8(24, 32, 42, 0.88 * alpha_mul) },
                ];
                if let Some(br) = create_grad(rt, Vector2 { X: il, Y: it }, Vector2 { X: il, Y: ib }, &stops) {
                    rt.FillRoundedRectangle(&rr, &br);
                }
                if let Ok(rim) = rt.CreateSolidColorBrush(&rgba_u8(220, 234, 248, 0.36 * alpha_mul), None) {
                    rt.DrawRoundedRectangle(&rr, &rim, 0.95 * u, None);
                }
            }
            // Exact Windhawk RosePine Tiles & Catppuccin Tiles: standalone dark tiles + glowing accent border
            TaskbarTheme::RosePineTiles | TaskbarTheme::CatppuccinTiles => {
                let is_rp = theme == TaskbarTheme::RosePineTiles;
                let fill = if is_rp {
                    rgba_u8(38, 35, 58, 0.95 * alpha_mul)
                } else {
                    rgba_u8(30, 30, 46, 0.95 * alpha_mul)
                };
                if let Ok(br) = rt.CreateSolidColorBrush(&fill, None) {
                    rt.FillRoundedRectangle(&rr, &br);
                }
                let (rim_col, rim_w) = if is_featured {
                    if is_rp {
                        (rgba_u8(235, 188, 186, 0.94 * alpha_mul), 1.5 * u)
                    } else {
                        (rgba_u8(180, 190, 254, 0.94 * alpha_mul), 1.5 * u)
                    }
                } else if is_rp {
                    (rgba_u8(110, 106, 134, 0.28 * alpha_mul), 0.85 * u)
                } else {
                    (rgba_u8(108, 112, 134, 0.28 * alpha_mul), 0.85 * u)
                };
                if let Ok(rim) = rt.CreateSolidColorBrush(&rim_col, None) {
                    rt.DrawRoundedRectangle(&rr, &rim, rim_w, None);
                }
            }
            // Exact Windhawk Blob Light: pure white elevated pill tabs popping off pale ice ribbon
            TaskbarTheme::BlobLight => {
                let show_blob = !is_preview || is_featured || idx == 1 || idx == 3;
                if show_blob {
                    if !above_taskbar {
                        if let Ok(sh) = rt.CreateSolidColorBrush(&rgba_u8(48, 72, 114, 0.14), None) {
                            let sh_rr = D2D1_ROUNDED_RECT {
                                rect: D2D_RECT_F { left: il, top: it + 0.7 * u, right: ir, bottom: ib + 1.1 * u },
                                radiusX: rad,
                                radiusY: rad,
                            };
                            rt.FillRoundedRectangle(&sh_rr, &sh);
                        }
                    }
                    let col = if is_featured || !is_preview {
                        rgba_u8(255, 255, 255, 0.98 * alpha_mul)
                    } else {
                        rgba_u8(244, 248, 254, 0.80 * alpha_mul)
                    };
                    if let Ok(br) = rt.CreateSolidColorBrush(&col, None) {
                        rt.FillRoundedRectangle(&rr, &br);
                    }
                }
            }
            // Exact Windhawk Aeris Light: sharp-cornered flat architectural translucent blocks
            TaskbarTheme::AerisGlassLight => {
                let block_col = match idx % 4 {
                    0 => rgba_u8(182, 212, 234, 0.44 * alpha_mul),
                    1 => rgba_u8(122, 186, 246, 0.58 * alpha_mul),
                    2 => rgba_u8(136, 224, 186, 0.56 * alpha_mul),
                    _ => rgba_u8(172, 206, 232, 0.48 * alpha_mul),
                };
                if let Ok(br) = rt.CreateSolidColorBrush(&block_col, None) {
                    rt.FillRectangle(&rr.rect, &br);
                }
                if is_featured {
                    if let Ok(top_b) = rt.CreateSolidColorBrush(&rgba_u8(54, 134, 224, 0.88 * alpha_mul), None) {
                        rt.DrawLine(
                            Vector2 { X: il, Y: it + 1.0 * u },
                            Vector2 { X: ir, Y: it + 1.0 * u },
                            &top_b,
                            1.8 * u,
                            None,
                        );
                    }
                }
            }
            // Exact Windhawk CleanSlate: dark-frosted rounded cards + glowing cyan active border
            TaskbarTheme::CleanSlateCards => {
                if let Ok(br) = rt.CreateSolidColorBrush(&rgba_u8(12, 26, 36, 0.78 * alpha_mul), None) {
                    rt.FillRoundedRectangle(&rr, &br);
                }
                let (rim_col, rim_w) = if is_featured {
                    (rgba_u8(142, 232, 246, 0.94 * alpha_mul), 1.3 * u)
                } else {
                    (rgba_u8(142, 214, 230, 0.24 * alpha_mul), 0.85 * u)
                };
                if let Ok(rim) = rt.CreateSolidColorBrush(&rim_col, None) {
                    rt.DrawRoundedRectangle(&rr, &rim, rim_w, None);
                }
            }
            _ => {}
        }
    }
}

#[allow(clippy::too_many_arguments)]
unsafe fn draw_capsule_rim_and_accents(
    rt: &ID2D1RenderTarget,
    d2d: Option<&ID2D1Factory>,
    l: f32,
    t: f32,
    r: f32,
    b: f32,
    rad: f32,
    u: f32,
    is_vertical: bool,
    anchor_min: bool,
    is_full_bar: bool,
    cap_idx: usize,
    is_preview: bool,
    sys_light: bool,
    cfg: &AppConfig,
    viz: &VizSnapshot,
    pal: &ThemePalette,
) {
    if cfg.pill_mode == PillMode::NativeDefault {
        return;
    }
    // Zero-bar independent floating tiles: NO enclosing rim on app icons group (`cap_idx == 0`)!
    if cfg.tb_theme.is_independent_tile_dock() && cap_idx == 0 && !is_full_bar {
        return;
    }

    let rr = D2D1_ROUNDED_RECT {
        rect: D2D_RECT_F { left: l, top: t, right: r, bottom: b },
        radiusX: if is_full_bar { 0.0 } else { rad },
        radiusY: if is_full_bar { 0.0 } else { rad },
    };
    let dark_mode_tray_guard = !is_preview
        && !sys_light
        && cfg.tb_theme.is_light_surface()
        && (cap_idx >= 1 || cfg.pill_mode != PillMode::SplitPills);

    match cfg.tb_theme {
        TaskbarTheme::BorderlessGhost | TaskbarTheme::BlobLight | TaskbarTheme::AerisGlassLight => {
            if cfg.tb_theme == TaskbarTheme::AerisGlassLight && !is_vertical {
                if let Ok(top_l) = rt.CreateSolidColorBrush(&rgba_u8(255, 255, 255, 0.58), None) {
                    rt.DrawLine(
                        Vector2 { X: l, Y: t + 0.6 * u },
                        Vector2 { X: r, Y: t + 0.6 * u },
                        &top_l,
                        1.0 * u,
                        None,
                    );
                }
            }
            return;
        }
        // OS26 Liquid Glass outer ultra-crisp glass rail outline
        TaskbarTheme::Os26LiquidGlassLight | TaskbarTheme::Os26LiquidGlassDark => {
            let is_light = cfg.tb_theme == TaskbarTheme::Os26LiquidGlassLight && !dark_mode_tray_guard;
            let rail_stops = if is_light {
                [
                    D2D1_GRADIENT_STOP { position: 0.0, color: rgba_u8(255, 255, 255, 0.88) },
                    D2D1_GRADIENT_STOP { position: 0.50, color: rgba_u8(168, 198, 232, 0.34) },
                    D2D1_GRADIENT_STOP { position: 1.0, color: rgba_u8(255, 255, 255, 0.72) },
                ]
            } else {
                [
                    D2D1_GRADIENT_STOP { position: 0.0, color: rgba_u8(232, 244, 255, 0.44) },
                    D2D1_GRADIENT_STOP { position: 0.50, color: rgba_u8(140, 178, 222, 0.14) },
                    D2D1_GRADIENT_STOP { position: 1.0, color: rgba_u8(212, 234, 255, 0.34) },
                ]
            };
            if let Some(rb) = create_grad(rt, Vector2 { X: l, Y: t }, Vector2 { X: l, Y: b }, &rail_stops) {
                rt.DrawRoundedRectangle(&rr, &rb, 1.0 * u, None);
            }
            return;
        }
        // Matter Tiles tray capsule rim (`cap_idx >= 1`)
        TaskbarTheme::MatterTilesLight | TaskbarTheme::MatterTilesDark => {
            let is_light = cfg.tb_theme == TaskbarTheme::MatterTilesLight && !dark_mode_tray_guard;
            let col = if is_light {
                rgba_u8(60, 44, 82, 0.12)
            } else {
                rgba_u8(255, 232, 252, 0.12)
            };
            if let Ok(ob) = rt.CreateSolidColorBrush(&col, None) {
                rt.DrawRoundedRectangle(&rr, &ob, 0.9 * u, None);
            }
            return;
        }
        // One UI 8.5 outer rail rim + vibrant signature clock pill accent on tray preview
        TaskbarTheme::OneUiLightChips | TaskbarTheme::OneUiDarkChips => {
            let is_dark_rail = cfg.tb_theme == TaskbarTheme::OneUiLightChips || dark_mode_tray_guard;
            let col = if is_dark_rail {
                rgba_u8(240, 246, 255, 0.28)
            } else {
                rgba_u8(255, 255, 255, 0.84)
            };
            if let Ok(ob) = rt.CreateSolidColorBrush(&col, None) {
                rt.DrawRoundedRectangle(&rr, &ob, 1.0 * u, None);
            }
            // Signature One UI 8.5 vibrant gradient clock pill inside right end of tray capsule
            if is_preview && cap_idx >= 1 && (r - l) > 30.0 * u && !is_vertical {
                let chip_w = ((r - l) * 0.44).clamp(18.0 * u, 30.0 * u);
                let chip_rr = D2D1_ROUNDED_RECT {
                    rect: D2D_RECT_F {
                        left: r - chip_w - 3.0 * u,
                        top: t + 3.5 * u,
                        right: r - 3.0 * u,
                        bottom: b - 3.5 * u,
                    },
                    radiusX: ((b - t) - 7.0 * u) * 0.48,
                    radiusY: ((b - t) - 7.0 * u) * 0.48,
                };
                let c_stops = [
                    D2D1_GRADIENT_STOP { position: 0.0, color: rgba_u8(255, 68, 128, 0.96) },
                    D2D1_GRADIENT_STOP { position: 1.0, color: rgba_u8(122, 56, 255, 0.96) },
                ];
                if let Some(cbr) = create_grad(
                    rt,
                    Vector2 { X: chip_rr.rect.left, Y: chip_rr.rect.top },
                    Vector2 { X: chip_rr.rect.right, Y: chip_rr.rect.bottom },
                    &c_stops,
                ) {
                    rt.FillRoundedRectangle(&chip_rr, &cbr);
                }
            }
            return;
        }
        TaskbarTheme::SquircleFrosted => {
            if let Ok(ob) = rt.CreateSolidColorBrush(&rgba_u8(255, 232, 214, 0.18), None) {
                rt.DrawRoundedRectangle(&rr, &ob, 0.9 * u, None);
            }
            return;
        }
        TaskbarTheme::BubblesDark | TaskbarTheme::BubblesLight => {
            let is_light = cfg.tb_theme == TaskbarTheme::BubblesLight && !dark_mode_tray_guard;
            let col = if is_light {
                rgba_u8(48, 68, 102, 0.16)
            } else {
                rgba_u8(255, 255, 255, 0.12)
            };
            if let Ok(ob) = rt.CreateSolidColorBrush(&col, None) {
                rt.DrawRoundedRectangle(&rr, &ob, 0.95 * u, None);
            }
            return;
        }
        TaskbarTheme::PillsDetached => {
            if let Ok(ob) = rt.CreateSolidColorBrush(&rgba_u8(220, 234, 248, 0.36), None) {
                rt.DrawRoundedRectangle(&rr, &ob, 1.0 * u, None);
            }
            return;
        }
        TaskbarTheme::RosePineTiles => {
            if let Ok(ob) = rt.CreateSolidColorBrush(&rgba_u8(235, 188, 186, 0.44), None) {
                rt.DrawRoundedRectangle(&rr, &ob, 1.05 * u, None);
            }
            return;
        }
        TaskbarTheme::CatppuccinTiles => {
            if let Ok(ob) = rt.CreateSolidColorBrush(&rgba_u8(180, 190, 254, 0.42), None) {
                rt.DrawRoundedRectangle(&rr, &ob, 1.05 * u, None);
            }
            return;
        }
        // Windhawk LiquidGlass2: luminous curved crystal white top & bottom refractive rim
        TaskbarTheme::LiquidGlassClear => {
            let rim_stops = [
                D2D1_GRADIENT_STOP { position: 0.0, color: rgba_u8(255, 255, 255, 0.82) },
                D2D1_GRADIENT_STOP { position: 0.48, color: rgba_u8(220, 238, 228, 0.24) },
                D2D1_GRADIENT_STOP { position: 1.0, color: rgba_u8(255, 255, 255, 0.64) },
            ];
            if let Some(rb) = create_grad(rt, Vector2 { X: l, Y: t }, Vector2 { X: l, Y: b }, &rim_stops) {
                rt.DrawRoundedRectangle(&rr, &rb, 1.35 * u, None);
            }
            return;
        }
        // Windhawk DockLike Light & Dark
        TaskbarTheme::DockLikeLight | TaskbarTheme::DockLikeDark => {
            let is_light = cfg.tb_theme == TaskbarTheme::DockLikeLight && !dark_mode_tray_guard;
            let col = if is_light {
                rgba_u8(30, 44, 72, 0.16)
            } else {
                rgba_u8(168, 192, 232, 0.18)
            };
            if let Ok(ob) = rt.CreateSolidColorBrush(&col, None) {
                rt.DrawRoundedRectangle(&rr, &ob, 1.0 * u, None);
            }
            if is_preview && cap_idx == 0 && !is_vertical {
                let dcol = if is_light {
                    rgba_u8(62, 82, 118, 0.72)
                } else {
                    rgba_u8(192, 210, 236, 0.78)
                };
                if let Ok(dbr) = rt.CreateSolidColorBrush(&dcol, None) {
                    for k in 0..3usize {
                        let dx = l + (r - l) * (0.28 + k as f32 * 0.20);
                        let dy = b - 2.8 * u;
                        rt.FillRoundedRectangle(
                            &D2D1_ROUNDED_RECT {
                                rect: D2D_RECT_F {
                                    left: dx - 2.2 * u,
                                    top: dy - 1.0 * u,
                                    right: dx + 2.2 * u,
                                    bottom: dy + 1.0 * u,
                                },
                                radiusX: 1.0 * u,
                                radiusY: 1.0 * u,
                            },
                            &dbr,
                        );
                    }
                }
            }
            return;
        }
        // Windhawk Luminosity Float
        TaskbarTheme::LuminosityFloat => {
            let (rr_c, rg_c, rb_c) = pal.rim;
            if let Ok(ob) = rt.CreateSolidColorBrush(&rgba_u8(rr_c, rg_c, rb_c, 0.28), None) {
                rt.DrawRoundedRectangle(&rr, &ob, 1.05 * u, None);
            }
            return;
        }
        // Windhawk CleanSlate Cards
        TaskbarTheme::CleanSlateCards => {
            if !is_vertical {
                if let Ok(top_l) = rt.CreateSolidColorBrush(&rgba_u8(132, 226, 240, 0.30), None) {
                    rt.DrawLine(
                        Vector2 { X: l, Y: t + 0.6 * u },
                        Vector2 { X: r, Y: t + 0.6 * u },
                        &top_l,
                        1.0 * u,
                        None,
                    );
                }
            }
            return;
        }
        TaskbarTheme::QuartzShellDock
        | TaskbarTheme::CupertinoDock
        | TaskbarTheme::KineticCupertinoWave => {
            // Crisp dark outer hairline + bright Apple Quartz crystal top specular rim
            if let Ok(ob) = rt.CreateSolidColorBrush(&rgba_u8(10, 14, 22, 0.55), None) {
                rt.DrawRoundedRectangle(&rr, &ob, 1.4 * u, None);
            }
            let inner_rr = D2D1_ROUNDED_RECT {
                rect: D2D_RECT_F {
                    left: l + 1.0 * u,
                    top: t + 1.0 * u,
                    right: r - 1.0 * u,
                    bottom: b - 1.0 * u,
                },
                radiusX: (rad - 1.0 * u).max(2.0),
                radiusY: (rad - 1.0 * u).max(2.0),
            };
            let rim_stops = [
                D2D1_GRADIENT_STOP { position: 0.0, color: rgba_u8(255, 255, 255, 0.52) },
                D2D1_GRADIENT_STOP { position: 0.55, color: rgba_u8(216, 232, 252, 0.24) },
                D2D1_GRADIENT_STOP { position: 1.0, color: rgba_u8(168, 198, 240, 0.14) },
            ];
            if let Some(rb) = create_grad(rt, Vector2 { X: l, Y: t }, Vector2 { X: l, Y: b }, &rim_stops) {
                rt.DrawRoundedRectangle(&inner_rr, &rb, 1.1 * u, None);
            }
            // Classic Apple Dock frosted separator line + indicator glints inside app dock pill
            if cap_idx == 0 && (r - l) > 80.0 * u && (b - t) > 16.0 * u {
                if let Ok(dbr) = rt.CreateSolidColorBrush(&rgba_u8(255, 255, 255, 0.26), None) {
                    if is_vertical {
                        let sep_y = b - ((b - t) * 0.14).clamp(16.0 * u, 28.0 * u);
                        rt.DrawLine(
                            Vector2 { X: l + 8.0 * u, Y: sep_y },
                            Vector2 { X: r - 8.0 * u, Y: sep_y },
                            &dbr,
                            1.0 * u,
                            None,
                        );
                    } else {
                        let sep_x = r - ((r - l) * 0.15).clamp(16.0 * u, 28.0 * u);
                        rt.DrawLine(
                            Vector2 { X: sep_x, Y: t + 5.0 * u },
                            Vector2 { X: sep_x, Y: b - 5.0 * u },
                            &dbr,
                            1.0 * u,
                            None,
                        );
                    }
                    if is_preview && !is_vertical {
                        for k in 0..3usize {
                            let dx = l + (r - l) * (0.28 + k as f32 * 0.16);
                            let dy = b - 3.0 * u;
                            rt.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: dx, Y: dy }, radiusX: 1.5 * u, radiusY: 1.5 * u }, &dbr);
                        }
                    }
                }
            }
            return;
        }
        TaskbarTheme::DynamicIslandDock => {
            if let Ok(ob) = rt.CreateSolidColorBrush(&rgba_u8(190, 206, 232, 0.24), None) {
                rt.DrawRoundedRectangle(&rr, &ob, 1.15 * u, None);
            }
            if cap_idx == 0 && !is_vertical && (r - l) > 48.0 * u {
                // Sleek hardware status sensor pill dot on right edge of island
                if let Ok(lbr) = rt.CreateSolidColorBrush(&rgba_u8(62, 234, 188, 0.85), None) {
                    let sx = r - 11.0 * u;
                    let sy = t + (b - t) * 0.5;
                    rt.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: sx, Y: sy }, radiusX: 2.2 * u, radiusY: 2.2 * u }, &lbr);
                }
            }
            return;
        }
        TaskbarTheme::FrutigerAero7 => {
            if let Ok(br) = rt.CreateSolidColorBrush(&rgba_u8(212, 248, 255, 0.62), None) {
                if is_full_bar {
                    let y_line = if is_vertical {
                        t
                    } else if anchor_min {
                        b - 1.0 * u
                    } else {
                        t + 1.0 * u
                    };
                    if !is_vertical {
                        rt.DrawLine(Vector2 { X: l, Y: y_line }, Vector2 { X: r, Y: y_line }, &br, 1.4 * u, None);
                    } else {
                        let x_line = if anchor_min { r - 1.0 * u } else { l + 1.0 * u };
                        rt.DrawLine(Vector2 { X: x_line, Y: t }, Vector2 { X: x_line, Y: b }, &br, 1.4 * u, None);
                    }
                } else {
                    rt.DrawRoundedRectangle(&rr, &br, 1.3 * u, None);
                }
            }
            return;
        }
        TaskbarTheme::UnixPolybar => {
            if let Ok(ob) = rt.CreateSolidColorBrush(&rgba_u8(68, 72, 96, 0.72), None) {
                rt.DrawRoundedRectangle(&rr, &ob, 1.0 * u, None);
            }
            let bar_col = if cap_idx == 0 {
                rgba_u8(180, 190, 254, 0.94)
            } else {
                rgba_u8(137, 220, 235, 0.94)
            };
            if let Ok(ab) = rt.CreateSolidColorBrush(&bar_col, None) {
                if !is_vertical {
                    rt.DrawLine(
                        Vector2 { X: l + 8.0 * u, Y: t + 1.5 * u },
                        Vector2 { X: r - 8.0 * u, Y: t + 1.5 * u },
                        &ab,
                        2.0 * u,
                        None,
                    );
                    if cap_idx == 0 && (r - l) > 80.0 * u {
                        for k in 0..3usize {
                            let wx = l + (11.0 + k as f32 * 7.5) * u;
                            let wy = b - 4.0 * u;
                            if k == 0 {
                                rt.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: wx, Y: wy }, radiusX: 2.0 * u, radiusY: 2.0 * u }, &ab);
                            } else {
                                rt.DrawEllipse(&D2D1_ELLIPSE { point: Vector2 { X: wx, Y: wy }, radiusX: 1.7 * u, radiusY: 1.7 * u }, &ab, 0.9 * u, None);
                            }
                        }
                    }
                } else {
                    let x_line = if anchor_min { r - 1.5 * u } else { l + 1.5 * u };
                    rt.DrawLine(Vector2 { X: x_line, Y: t + 8.0 * u }, Vector2 { X: x_line, Y: b - 8.0 * u }, &ab, 2.0 * u, None);
                }
            }
            return;
        }
        TaskbarTheme::CyberArasakaDeck => {
            let cut = rad.max(6.0 * u).clamp(2.0, ((r - l) * 0.35).min((b - t) * 0.38));
            if let Some(factory) = d2d {
                if let Some(geo) = create_chamfer_geo(factory, l, t, r, b, cut) {
                    if let Ok(br) = rt.CreateSolidColorBrush(&rgba_u8(0, 240, 255, 0.62), None) {
                        rt.DrawGeometry(&geo, &br, 1.15 * u, None);
                    }
                }
            } else if let Ok(br) = rt.CreateSolidColorBrush(&rgba_u8(0, 240, 255, 0.62), None) {
                rt.DrawRoundedRectangle(&rr, &br, 1.15 * u, None);
            }
            // High-contrast crimson/amber diagonal corner brackets
            if let Ok(abr) = rt.CreateSolidColorBrush(&rgba_u8(255, 58, 102, 0.94), None) {
                let arm = (cut + 8.0 * u).min((r - l) * 0.3);
                rt.DrawLine(Vector2 { X: l, Y: t + arm }, Vector2 { X: l, Y: t + cut }, &abr, 2.2 * u, None);
                rt.DrawLine(Vector2 { X: l, Y: t + cut }, Vector2 { X: l + cut, Y: t }, &abr, 2.2 * u, None);
                rt.DrawLine(Vector2 { X: l + cut, Y: t }, Vector2 { X: l + arm, Y: t }, &abr, 2.2 * u, None);

                rt.DrawLine(Vector2 { X: r, Y: b - arm }, Vector2 { X: r, Y: b - cut }, &abr, 2.2 * u, None);
                rt.DrawLine(Vector2 { X: r, Y: b - cut }, Vector2 { X: r - cut, Y: b }, &abr, 2.2 * u, None);
                rt.DrawLine(Vector2 { X: r - cut, Y: b }, Vector2 { X: r - arm, Y: b }, &abr, 2.2 * u, None);
            }
            return;
        }
        TaskbarTheme::Retro95Classic => {
            if let (Ok(hi), Ok(sh)) = (
                rt.CreateSolidColorBrush(&rgba_u8(226, 232, 242, 0.86), None),
                rt.CreateSolidColorBrush(&rgba_u8(16, 18, 24, 0.90), None),
            ) {
                rt.DrawLine(Vector2 { X: l, Y: t + 0.8 * u }, Vector2 { X: r, Y: t + 0.8 * u }, &hi, 1.5 * u, None);
                rt.DrawLine(Vector2 { X: l + 0.8 * u, Y: t }, Vector2 { X: l + 0.8 * u, Y: b }, &hi, 1.5 * u, None);
                rt.DrawLine(Vector2 { X: l, Y: b - 0.8 * u }, Vector2 { X: r, Y: b - 0.8 * u }, &sh, 1.5 * u, None);
                rt.DrawLine(Vector2 { X: r - 0.8 * u, Y: t }, Vector2 { X: r - 0.8 * u, Y: b }, &sh, 1.5 * u, None);
                // Sunken inset tray pocket in preview & raised divider grip on left
                if !is_vertical && (r - l) > 90.0 * u {
                    let gx = l + 6.0 * u;
                    rt.DrawLine(Vector2 { X: gx, Y: t + 4.0 * u }, Vector2 { X: gx, Y: b - 4.0 * u }, &hi, 1.1 * u, None);
                    rt.DrawLine(Vector2 { X: gx + 1.4 * u, Y: t + 4.0 * u }, Vector2 { X: gx + 1.4 * u, Y: b - 4.0 * u }, &sh, 1.1 * u, None);
                    if is_preview {
                        let pl = r - 64.0 * u;
                        let pr = r - 6.0 * u;
                        let pt = t + 3.5 * u;
                        let pb = b - 3.5 * u;
                        rt.DrawLine(Vector2 { X: pl, Y: pt }, Vector2 { X: pr, Y: pt }, &sh, 1.0 * u, None);
                        rt.DrawLine(Vector2 { X: pl, Y: pt }, Vector2 { X: pl, Y: pb }, &sh, 1.0 * u, None);
                        rt.DrawLine(Vector2 { X: pl, Y: pb }, Vector2 { X: pr, Y: pb }, &hi, 1.0 * u, None);
                        rt.DrawLine(Vector2 { X: pr, Y: pt }, Vector2 { X: pr, Y: pb }, &hi, 1.0 * u, None);
                    }
                }
            }
            return;
        }
        _ => {}
    }

    if is_full_bar {
        if cfg.amber_border && cfg.pill_mode == PillMode::FullBar {
            let (p0, p1) = if is_vertical {
                let x = if anchor_min { r - 1.0 * u } else { l + 1.0 * u };
                (Vector2 { X: x, Y: t + 10.0 * u }, Vector2 { X: x, Y: b - 10.0 * u })
            } else {
                let y = if anchor_min { b - 1.0 * u } else { t + 1.0 * u };
                (Vector2 { X: l + 10.0 * u, Y: y }, Vector2 { X: r - 10.0 * u, Y: y })
            };
            if let Some(br) = create_palette_gradient(rt, p0, p1, 0.86, pal) {
                rt.DrawLine(p0, p1, &br, 1.6 * u, None);
            }
        }
        return;
    }

    // Standard top-left crystal frost specular sheen (tinted to active palette, never stale amber)
    let sheen_top_a = if cfg.surface_mode == SurfaceMode::Blur { 0.28 } else { 0.15 };
    let (sr, sg, sb) = pal.text_primary;
    let sheen_rr = D2D1_ROUNDED_RECT {
        rect: D2D_RECT_F {
            left: l + 1.0 * u,
            top: t + 1.0 * u,
            right: r - 1.0 * u,
            bottom: b - 1.0 * u,
        },
        radiusX: (rad - 1.0 * u).max(2.0),
        radiusY: (rad - 1.0 * u).max(2.0),
    };
    let sheen_stops = [
        D2D1_GRADIENT_STOP { position: 0.0, color: rgba_u8(sr, sg, sb, sheen_top_a) },
        D2D1_GRADIENT_STOP { position: 0.38, color: rgba_u8(sr, sg, sb, sheen_top_a * 0.16) },
        D2D1_GRADIENT_STOP { position: 1.0, color: rgba_u8(sr, sg, sb, 0.0) },
    ];
    if let Some(sh_br) = create_grad(
        rt,
        Vector2 { X: l, Y: t },
        Vector2 { X: r, Y: t + (b - t) * 0.42 },
        &sheen_stops,
    ) {
        rt.DrawRoundedRectangle(&sheen_rr, &sh_br, 1.0 * u, None);
    }

    let fill_alpha = (cfg.tint_alpha as f32 / 255.0).clamp(0.0, 0.96);
    if cfg.amber_border {
        let rim_alpha = 0.85 + viz.energy * 0.14;
        if let Some(rim_br) = create_palette_gradient(
            rt,
            Vector2 { X: l, Y: t },
            Vector2 { X: r, Y: b },
            rim_alpha.min(1.0),
            pal,
        ) {
            rt.DrawRoundedRectangle(&rr, &rim_br, 1.45 * u, None);
        }
    } else if fill_alpha > 0.04 {
        if let Ok(sub_rim) = rt.CreateSolidColorBrush(&rgba_u8(sr, sg, sb, 0.16), None) {
            rt.DrawRoundedRectangle(&rr, &sub_rim, 1.0 * u, None);
        }
    }
}

#[allow(clippy::too_many_arguments)]
unsafe fn draw_glass_surface(
    dc_rt: &ID2D1DCRenderTarget,
    d2d: Option<&ID2D1Factory>,
    mem_dc: HDC,
    mon: RECT,
    win_rect: RECT,
    bar_rect: RECT,
    screen_pills: &[RECT],
    screen_icons: &[RECT],
    radius: i32,
    u: f32,
    tap_connected: bool,
    sys_light: bool,
    cfg: &AppConfig,
    viz: &mut VizSnapshot,
) -> bool {
    let win_w = win_rect.right - win_rect.left;
    let win_h = win_rect.bottom - win_rect.top;
    let bind_rc = RECT { left: 0, top: 0, right: win_w, bottom: win_h };
    if dc_rt.BindDC(mem_dc, &bind_rc).is_err() {
        return false;
    }

    dc_rt.BeginDraw();
    dc_rt.Clear(Some(&D2D1_COLOR_F { r: 0.0, g: 0.0, b: 0.0, a: 0.0 }));
    if cfg.is_native_tb_all_elements_off() {
        return dc_rt.EndDraw(None, None).is_ok();
    }

    let pal = effective_taskbar_palette(cfg);
    let bar_w = bar_rect.right - bar_rect.left;
    let bar_h = bar_rect.bottom - bar_rect.top;
    let is_vertical = bar_h > bar_w * 2;
    let anchor_min = if is_vertical {
        (bar_rect.left - mon.left).abs() <= (mon.right - bar_rect.right).abs()
    } else {
        (bar_rect.top - mon.top).abs() <= (mon.bottom - bar_rect.bottom).abs()
    };

    let is_full_bar = matches!(cfg.pill_mode, PillMode::FullBar | PillMode::NativeDefault);
    let rad = if is_full_bar {
        0.0
    } else {
        effective_pill_radius_for(screen_pills, radius, u, cfg.tb_theme)
    };

    let collect_local_pads = |cap_l: f32, cap_t: f32, cap_r: f32, cap_b: f32| -> Vec<(f32, f32, f32, f32)> {
        if !cfg.tb_theme.uses_icon_pads() || screen_icons.is_empty() {
            return Vec::new();
        }
        let (inset_x, inset_y, margin_x, margin_y) = if cfg.tb_theme.is_independent_tile_dock() {
            (2.5 * u, 2.5 * u, 1.0 * u, 1.5 * u)
        } else if cfg.tb_theme == TaskbarTheme::AerisGlassLight {
            (1.0 * u, 0.0, 0.0, 0.0)
        } else if matches!(
            cfg.tb_theme,
            TaskbarTheme::Os26LiquidGlassLight
                | TaskbarTheme::Os26LiquidGlassDark
                | TaskbarTheme::OneUiLightChips
                | TaskbarTheme::OneUiDarkChips
        ) {
            (1.6 * u, 2.0 * u, 2.0 * u, 2.0 * u)
        } else {
            (2.0 * u, 3.0 * u, 2.0 * u, 2.5 * u)
        };
        let mut out = Vec::with_capacity(screen_icons.len());
        for &ic in screen_icons {
            let il = (ic.left - win_rect.left) as f32 + inset_x;
            let it = (ic.top - win_rect.top) as f32 + inset_y;
            let ir = (ic.right - win_rect.left) as f32 - inset_x;
            let ib = (ic.bottom - win_rect.top) as f32 - inset_y;
            let cx = (il + ir) * 0.5;
            let cy = (it + ib) * 0.5;
            if cx >= cap_l && cx <= cap_r && cy >= cap_t && cy <= cap_b {
                let cl = il.max(cap_l + margin_x);
                let ct = it.max(cap_t + margin_y);
                let cr = ir.min(cap_r - margin_x);
                let cb = ib.min(cap_b - margin_y);
                if cr - cl >= 10.0 * u && cb - ct >= 10.0 * u {
                    out.push((cl, ct, cr, cb));
                }
            }
        }
        out
    };

    let above_taskbar = !tap_connected;
    let vu_tricolor = is_vu_tricolor_active(cfg);
    if is_full_bar {
        let l = 0.0;
        let t = 0.0;
        let r = win_w as f32;
        let b = win_h as f32;
        if cfg.tb_show_backdrop {
            draw_capsule_backdrop(
                dc_rt,
                d2d,
                l,
                t,
                r,
                b,
                0.0,
                u,
                r,
                b,
                true,
                above_taskbar,
                0,
                false,
                sys_light,
                cfg,
                &pal,
            );
            let pads = collect_local_pads(l, t, r, b);
            draw_clean_icon_pads(dc_rt, d2d, cfg.tb_theme, &pads, u, above_taskbar, false, None);
        }
        if cfg.tb_show_fx {
            draw_taskbar_viz_in_rect_themed(
                dc_rt,
                d2d,
                l,
                t,
                r,
                b,
                (l, t, r, b),
                0.0,
                u,
                is_vertical,
                anchor_min,
                cfg.tb_viz_style,
                cfg.tb_viz_opacity,
                cfg.tb_viz_mirror,
                vu_tricolor,
                &pal,
                viz,
            );
        }
        if cfg.tb_show_rim {
            draw_capsule_rim_and_accents(
                dc_rt, d2d, l, t, r, b, 0.0, u, is_vertical, anchor_min, true, 0, false, sys_light,
                cfg, viz, &pal,
            );
        }
    } else {
        for (cap_idx, &raw_sr) in screen_pills.iter().enumerate() {
            let sr = dock_adjusted_rect(raw_sr, bar_rect, is_vertical, anchor_min, u, cfg.tb_theme);
            let l = (sr.left - win_rect.left) as f32;
            let t = (sr.top - win_rect.top) as f32;
            let r = (sr.right - win_rect.left) as f32;
            let b = (sr.bottom - win_rect.top) as f32;
            if cfg.tb_show_backdrop {
                draw_capsule_backdrop(
                    dc_rt,
                    d2d,
                    l,
                    t,
                    r,
                    b,
                    rad,
                    u,
                    win_w as f32,
                    win_h as f32,
                    false,
                    above_taskbar,
                    cap_idx,
                    false,
                    sys_light,
                    cfg,
                    &pal,
                );
                let pads = collect_local_pads(l, t, r, b);
                draw_clean_icon_pads(dc_rt, d2d, cfg.tb_theme, &pads, u, above_taskbar, false, None);
            }
            if cfg.tb_show_fx {
                draw_taskbar_viz_in_rect_themed(
                    dc_rt,
                    d2d,
                    l,
                    t,
                    r,
                    b,
                    (0.0, 0.0, win_w as f32, win_h as f32),
                    rad,
                    u,
                    is_vertical,
                    anchor_min,
                    cfg.tb_viz_style,
                    cfg.tb_viz_opacity,
                    cfg.tb_viz_mirror,
                    vu_tricolor,
                    &pal,
                    viz,
                );
            }
            if cfg.tb_show_rim {
                draw_capsule_rim_and_accents(
                    dc_rt, d2d, l, t, r, b, rad, u, is_vertical, anchor_min, false, cap_idx, false,
                    sys_light, cfg, viz, &pal,
                );
            }
        }
    }

    dc_rt.EndDraw(None, None).is_ok()
}

fn measure_pills_cached(
    uia: &IUIAutomation,
    btn_cond: &IUIAutomationCondition,
    cache_req: &IUIAutomationCacheRequest,
    hwnd: HWND,
    bar_rect: RECT,
    u: f32,
    pill_mode: PillMode,
) -> (Vec<RECT>, Vec<RECT>) {
    let bar_w = bar_rect.right - bar_rect.left;
    let bar_h = bar_rect.bottom - bar_rect.top;
    let is_vertical = bar_h > bar_w * 2;

    let Ok(root) = (unsafe { uia.ElementFromHandle(hwnd) }) else {
        return (vec![bar_rect], Vec::new());
    };
    let Ok(arr) = (unsafe { root.FindAllBuildCache(TreeScope_Subtree, btn_cond, cache_req) })
    else {
        return (vec![bar_rect], Vec::new());
    };
    let count = unsafe { arr.Length().unwrap_or(0) };

    let mut app_bounds: Option<RECT> = None;
    let mut tray_bounds: Option<RECT> = None;
    let mut raw_icons: Vec<RECT> = Vec::with_capacity(24);

    for i in 0..count {
        let Ok(el) = (unsafe { arr.GetElement(i) }) else {
            continue;
        };
        let Ok(r) = (unsafe { el.CachedBoundingRectangle() }) else {
            continue;
        };
        if r.right <= r.left || r.bottom <= r.top {
            continue;
        }
        // Reject off-screen / scrolled-away / overflowed controls outside bar_rect
        if r.right <= bar_rect.left + 2
            || r.left >= bar_rect.right - 2
            || r.bottom <= bar_rect.top + 2
            || r.top >= bar_rect.bottom - 2
        {
            continue;
        }
        let ew = r.right - r.left;
        let eh = r.bottom - r.top;
        if is_vertical && eh > (bar_h * 85) / 100 {
            continue;
        }
        if !is_vertical && ew > (bar_w * 85) / 100 {
            continue;
        }

        let aid = unsafe { el.CachedAutomationId().map(|b| b.to_string()).unwrap_or_default() };
        let cls = unsafe { el.CachedClassName().map(|b| b.to_string()).unwrap_or_default() };

        let is_app_item = aid == "StartButton"
            || aid == "TaskViewButton"
            || aid == "SearchButton"
            || aid == "WidgetsButton"
            || aid == "CopilotButton"
            || aid == "OverflowButton"
            || aid.starts_with("Appid:")
            || aid.contains("Search")
            || aid.contains("Overflow")
            || cls.contains("TaskListButton")
            || cls.contains("Taskbar.ExperienceToggleButton")
            || cls.contains("Widgets")
            || cls.contains("Overflow");

        let is_tray_item = aid == "SystemTrayIcon" && !cls.contains("ShowDesktopButton");

        if is_app_item {
            app_bounds = Some(merge_rect(app_bounds, r));
            if ew >= (16.0 * u) as i32 && eh >= (16.0 * u) as i32 {
                raw_icons.push(r);
            }
        } else if is_tray_item {
            tray_bounds = Some(merge_rect(tray_bounds, r));
        }
    }

    // Sort and deduplicate nested UIA sub-button rects that share the same button center
    if is_vertical {
        raw_icons.sort_by_key(|r| (r.top, r.left));
    } else {
        raw_icons.sort_by_key(|r| (r.left, r.top));
    }
    let dedup_tol = (10.0 * u).round().max(6.0) as i32;
    let mut icons: Vec<RECT> = Vec::with_capacity(raw_icons.len());
    for r in raw_icons {
        let cx = (r.left + r.right) / 2;
        let cy = (r.top + r.bottom) / 2;
        if let Some(prev) = icons.last_mut() {
            let pcx = (prev.left + prev.right) / 2;
            let pcy = (prev.top + prev.bottom) / 2;
            if (cx - pcx).abs() <= dedup_tol && (cy - pcy).abs() <= dedup_tol {
                let area_new = (r.right - r.left) * (r.bottom - r.top);
                let area_old = (prev.right - prev.left) * (prev.bottom - prev.top);
                if area_new > area_old {
                    *prev = r;
                }
                continue;
            }
        }
        icons.push(r);
    }

    let inset_cross = adaptive_cross_inset(bar_rect, u);
    let pad_main = (5.5 * u).round().max(3.0) as i32;
    let min_gap = (12.0 * u).round().max(6.0) as i32;

    let pills = if is_vertical {
        let left = bar_rect.left + inset_cross;
        let right = (bar_rect.right - inset_cross).max(left + 12);

        let app_pill = app_bounds.map(|b| RECT {
            left,
            top: (b.top - pad_main).max(bar_rect.top + 2),
            right,
            bottom: (b.bottom + pad_main).min(bar_rect.bottom - 2),
        });
        let tray_pill = tray_bounds.map(|b| RECT {
            left,
            top: (b.top - pad_main).max(bar_rect.top + 2),
            right,
            bottom: (b.bottom + pad_main).min(bar_rect.bottom - 2),
        });

        finalize_pills(app_pill, tray_pill, pill_mode, min_gap, true, bar_rect)
    } else {
        let top = bar_rect.top + inset_cross;
        let bottom = (bar_rect.bottom - inset_cross).max(top + 12);

        let app_pill = app_bounds.map(|b| RECT {
            left: (b.left - pad_main).max(bar_rect.left + 2),
            top,
            right: (b.right + pad_main).min(bar_rect.right - 2),
            bottom,
        });
        let tray_pill = tray_bounds.map(|b| RECT {
            left: (b.left - pad_main).max(bar_rect.left + 2),
            top,
            right: (b.right + pad_main).min(bar_rect.right - 2),
            bottom,
        });

        finalize_pills(app_pill, tray_pill, pill_mode, min_gap, false, bar_rect)
    };
    (pills, icons)
}

fn pills_close(a: &[RECT], b: &[RECT]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b.iter()).all(|(r1, r2)| {
        (r1.left - r2.left).abs() <= 1
            && (r1.top - r2.top).abs() <= 1
            && (r1.right - r2.right).abs() <= 1
            && (r1.bottom - r2.bottom).abs() <= 1
    })
}

fn finalize_pills(
    app_pill: Option<RECT>,
    tray_pill: Option<RECT>,
    mode: PillMode,
    min_gap: i32,
    is_vertical: bool,
    fallback: RECT,
) -> Vec<RECT> {
    match (app_pill, tray_pill) {
        (Some(a), Some(t)) => {
            if mode == PillMode::UnifiedPill {
                vec![union_rect(a, t)]
            } else if is_vertical {
                if (t.top - a.bottom) < min_gap && (a.top - t.bottom) < min_gap {
                    vec![union_rect(a, t)]
                } else {
                    vec![a, t]
                }
            } else if (t.left - a.right) < min_gap && (a.left - t.right) < min_gap {
                vec![union_rect(a, t)]
            } else {
                vec![a, t]
            }
        }
        (Some(a), None) => vec![a],
        (None, Some(t)) => vec![t],
        (None, None) => vec![fallback],
    }
}

fn merge_rect(acc: Option<RECT>, r: RECT) -> RECT {
    match acc {
        Some(a) => union_rect(a, r),
        None => r,
    }
}

fn union_rect(a: RECT, b: RECT) -> RECT {
    RECT {
        left: a.left.min(b.left),
        top: a.top.min(b.top),
        right: a.right.max(b.right),
        bottom: a.bottom.max(b.bottom),
    }
}

fn get_monitor_rect(hwnd: HWND) -> RECT {
    unsafe {
        let hmon = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
        let mut mi = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if GetMonitorInfoW(hmon, &mut mi).as_bool() {
            mi.rcMonitor
        } else {
            RECT {
                left: 0,
                top: 0,
                right: 1920,
                bottom: 1200,
            }
        }
    }
}

fn rgba_u8(r: u8, g: u8, b: u8, a: f32) -> D2D1_COLOR_F {
    D2D1_COLOR_F {
        r: r as f32 / 255.0,
        g: g as f32 / 255.0,
        b: b as f32 / 255.0,
        a: a.clamp(0.0, 1.0),
    }
}

unsafe fn create_grad(
    rt: &ID2D1RenderTarget,
    start: Vector2,
    end: Vector2,
    stops: &[D2D1_GRADIENT_STOP],
) -> Option<ID2D1LinearGradientBrush> {
    let collection = rt
        .CreateGradientStopCollection(stops, D2D1_GAMMA_2_2, D2D1_EXTEND_MODE_CLAMP)
        .ok()?;
    let props = D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES {
        startPoint: start,
        endPoint: end,
    };
    rt.CreateLinearGradientBrush(&props, None, &collection).ok()
}

unsafe fn create_palette_gradient(
    rt: &ID2D1RenderTarget,
    start: Vector2,
    end: Vector2,
    alpha: f32,
    pal: &ThemePalette,
) -> Option<ID2D1LinearGradientBrush> {
    let stops = [
        D2D1_GRADIENT_STOP {
            position: 0.0,
            color: pal.accent_col(alpha),
        },
        D2D1_GRADIENT_STOP {
            position: 0.5,
            color: pal.text_col(alpha * 0.45),
        },
        D2D1_GRADIENT_STOP {
            position: 1.0,
            color: pal.deep_col(alpha * 0.85),
        },
    ];
    create_grad(rt, start, end, &stops)
}

#[derive(Default)]
struct StylerClipCache {
    layers: Vec<(usize, ID2D1Layer)>,
    geos: Vec<((usize, i32, i32, i32, i32, i32), ID2D1Geometry)>,
}

thread_local! {
    static STYLER_CLIP_CACHE: std::cell::RefCell<StylerClipCache> =
        std::cell::RefCell::new(StylerClipCache::default());
}

pub fn evict_styler_rt(rt: &ID2D1RenderTarget) {
    let key = rt.as_raw() as usize;
    STYLER_CLIP_CACHE.with(|c| {
        c.borrow_mut().layers.retain(|(k, _)| *k != key);
    });
}

unsafe fn get_or_create_styler_layer(rt: &ID2D1RenderTarget) -> Option<ID2D1Layer> {
    let key = rt.as_raw() as usize;
    let found = STYLER_CLIP_CACHE.with(|c| {
        c.borrow()
            .layers
            .iter()
            .find(|(k, _)| *k == key)
            .map(|(_, l)| l.clone())
    });
    if found.is_some() {
        return found;
    }
    let layer = rt.CreateLayer(None).ok()?;
    STYLER_CLIP_CACHE.with(|c| {
        let mut cache = c.borrow_mut();
        if cache.layers.len() >= 8 {
            cache.layers.remove(0);
        }
        cache.layers.push((key, layer.clone()));
    });
    Some(layer)
}

unsafe fn get_or_create_styler_rr_geo(
    factory: &ID2D1Factory,
    rr: &D2D1_ROUNDED_RECT,
) -> Option<ID2D1Geometry> {
    let q = |v: f32| (v * 4.0).round() as i32;
    let gkey = (
        factory.as_raw() as usize,
        q(rr.rect.left),
        q(rr.rect.top),
        q(rr.rect.right),
        q(rr.rect.bottom),
        q(rr.radiusX),
    );
    let found = STYLER_CLIP_CACHE.with(|c| {
        c.borrow()
            .geos
            .iter()
            .find(|(k, _)| *k == gkey)
            .map(|(_, g)| g.clone())
    });
    if found.is_some() {
        return found;
    }
    let geo: ID2D1Geometry = factory.CreateRoundedRectangleGeometry(rr).ok()?.into();
    STYLER_CLIP_CACHE.with(|c| {
        let mut cache = c.borrow_mut();
        if cache.geos.len() >= 64 {
            cache.geos.remove(0);
        }
        cache.geos.push((gkey, geo.clone()));
    });
    Some(geo)
}

unsafe fn push_capsule_clip(
    rt: &ID2D1RenderTarget,
    d2d: Option<&ID2D1Factory>,
    l: f32,
    t: f32,
    r: f32,
    b: f32,
    rad: f32,
) -> Option<(ID2D1Layer, ID2D1Geometry)> {
    let rect = D2D_RECT_F { left: l, top: t, right: r, bottom: b };
    if rad > 0.5 {
        if let Some(factory) = d2d {
            let rr = D2D1_ROUNDED_RECT { rect, radiusX: rad, radiusY: rad };
            if let Some(mask_geo) = get_or_create_styler_rr_geo(factory, &rr) {
                if let Some(layer) = get_or_create_styler_layer(rt) {
                    let params = D2D1_LAYER_PARAMETERS {
                        contentBounds: rect,
                        geometricMask: std::mem::ManuallyDrop::new(Some(mask_geo.clone())),
                        maskAntialiasMode: D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
                        maskTransform: windows_numerics::Matrix3x2::identity(),
                        opacity: 1.0,
                        opacityBrush: std::mem::ManuallyDrop::new(None),
                        layerOptions: D2D1_LAYER_OPTIONS_NONE,
                    };
                    rt.PushLayer(&params, &layer);
                    std::mem::ManuallyDrop::into_inner(params.geometricMask);
                    std::mem::ManuallyDrop::into_inner(params.opacityBrush);
                    return Some((layer, mask_geo));
                }
            }
        }
    }
    rt.PushAxisAlignedClip(&rect, D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
    None
}

unsafe fn pop_capsule_clip(rt: &ID2D1RenderTarget, layer_opt: Option<(ID2D1Layer, ID2D1Geometry)>) {
    if layer_opt.is_some() {
        rt.PopLayer();
    } else {
        rt.PopAxisAlignedClip();
    }
}

#[allow(clippy::too_many_arguments)]
pub unsafe fn draw_taskbar_viz_in_rect(
    rt: &ID2D1RenderTarget,
    d2d: Option<&ID2D1Factory>,
    l: f32,
    t: f32,
    r: f32,
    b: f32,
    canvas_box: (f32, f32, f32, f32),
    rad: f32,
    u: f32,
    is_vertical: bool,
    anchor_min: bool,
    style: TaskbarVizStyle,
    opacity_pct: u8,
    mirror: bool,
    pal: &ThemePalette,
    viz: &VizSnapshot,
) {
    draw_taskbar_viz_in_rect_themed(
        rt,
        d2d,
        l,
        t,
        r,
        b,
        canvas_box,
        rad,
        u,
        is_vertical,
        anchor_min,
        style,
        opacity_pct,
        mirror,
        false,
        pal,
        viz,
    );
}

#[allow(clippy::too_many_arguments)]
pub unsafe fn draw_taskbar_viz_in_rect_themed(
    rt: &ID2D1RenderTarget,
    d2d: Option<&ID2D1Factory>,
    l: f32,
    t: f32,
    r: f32,
    b: f32,
    canvas_box: (f32, f32, f32, f32),
    rad: f32,
    u: f32,
    is_vertical: bool,
    anchor_min: bool,
    style: TaskbarVizStyle,
    opacity_pct: u8,
    mirror: bool,
    vu_tricolor: bool,
    pal: &ThemePalette,
    viz: &VizSnapshot,
) {
    let is_music_style = matches!(
        style,
        TaskbarVizStyle::EqualizerBars | TaskbarVizStyle::AuroraWave | TaskbarVizStyle::EdgePulse
    );
    if style == TaskbarVizStyle::Off
        || (!style.is_ambient_live()
            && !viz.has_energy()
            && !(is_music_style && viz.hover_mix > 0.01))
    {
        return;
    }
    let span_len = if is_vertical { b - t } else { r - l };
    let cross_len = if is_vertical { r - l } else { b - t };
    if span_len < 16.0 || cross_len < 10.0 {
        return;
    }

    let op = (opacity_pct as f32 / 100.0).clamp(0.20, 1.0);
    let (cb_l, cb_t, cb_w, cb_h) = canvas_box;
    let to_px = |nx: f32, ny: f32| -> Vector2 {
        Vector2 { X: cb_l + nx * cb_w, Y: cb_t + ny * cb_h }
    };
    let hx_px = to_px(viz.hover_x, viz.hover_y);
    let box_w = (r - l).max(16.0);
    let box_h = (b - t).max(12.0);
    let eff_u = u * (cross_len / (44.0 * u)).clamp(0.54, 1.15);
    let map_pt = |s_val: f32, c_val: f32| -> Vector2 {
        if is_vertical {
            let px = if anchor_min { l + c_val } else { r - c_val };
            Vector2 { X: px, Y: t + s_val }
        } else {
            let py = if anchor_min { t + c_val } else { b - c_val };
            Vector2 { X: l + s_val, Y: py }
        }
    };
    let map_rect = |s0: f32, s1: f32, c0: f32, c1: f32| -> D2D_RECT_F {
        let p0 = map_pt(s0, c0);
        let p1 = map_pt(s1, c1);
        D2D_RECT_F {
            left: p0.X.min(p1.X),
            top: p0.Y.min(p1.Y),
            right: p0.X.max(p1.X),
            bottom: p0.Y.max(p1.Y),
        }
    };
    // Center-out symmetrical spectrum when mirrored; authentic left-to-right bass->treble otherwise
    let sample_band = |p: f32| -> (f32, f32) {
        let freq_pos = if mirror { (p * 2.0 - 1.0).abs() } else { p.clamp(0.0, 1.0) };
        let f = freq_pos * 23.0;
        let i0 = (f.floor() as usize).min(23);
        let i1 = (i0 + 1).min(23);
        let frac = f - i0 as f32;
        let b_val = (viz.bands[i0] * (1.0 - frac) + viz.bands[i1] * frac).clamp(0.0, 1.0);
        let pk_val = (viz.peaks[i0] * (1.0 - frac) + viz.peaks[i1] * frac).clamp(0.0, 1.0);
        (b_val, pk_val)
    };

    let pad = (rad * 0.55).max(5.0 * u).min(span_len * 0.22);
    let avail = (span_len - 2.0 * pad).max(8.0);

    match style {
        TaskbarVizStyle::Off => {}
        TaskbarVizStyle::EqualizerBars => {
            let n_bars = ((avail / (10.5 * u)).round() as usize).clamp(14, 48);
            let step = avail / n_bars as f32;
            let bw = (step * 0.58).clamp(2.4 * u, 8.0 * u);
            let max_reach = if mirror {
                cross_len * 0.45
            } else {
                cross_len * 0.88
            };
            let grad_start = if mirror {
                map_pt(span_len * 0.5, cross_len * 0.5 - max_reach)
            } else {
                map_pt(span_len * 0.5, 2.0 * u)
            };
            let grad_end = if mirror {
                map_pt(span_len * 0.5, cross_len * 0.5 + max_reach)
            } else {
                map_pt(span_len * 0.5, 2.0 * u + max_reach)
            };
            let bar_br = if vu_tricolor {
                if mirror {
                    // Symmetrical Studio Tri-Color LED Equalizer Gradient (Peak Red <- Yellow <- Green -> Yellow -> Peak Red)
                    let vu_mirror_stops = [
                        D2D1_GRADIENT_STOP { position: 0.00, color: rgba_u8(255, 54, 62, 1.0 * op) },
                        D2D1_GRADIENT_STOP { position: 0.14, color: rgba_u8(255, 214, 48, 1.0 * op) },
                        D2D1_GRADIENT_STOP { position: 0.50, color: rgba_u8(34, 224, 98, 0.94 * op) },
                        D2D1_GRADIENT_STOP { position: 0.86, color: rgba_u8(255, 214, 48, 1.0 * op) },
                        D2D1_GRADIENT_STOP { position: 1.00, color: rgba_u8(255, 54, 62, 1.0 * op) },
                    ];
                    create_grad(rt, grad_start, grad_end, &vu_mirror_stops)
                } else {
                    // Authentic Studio Tri-Color LED Equalizer Gradient: Green -> Lime -> Yellow -> Peak Red
                    let vu_stops = [
                        D2D1_GRADIENT_STOP { position: 0.0, color: rgba_u8(34, 224, 98, 0.94 * op) },
                        D2D1_GRADIENT_STOP { position: 0.54, color: rgba_u8(116, 238, 72, 0.98 * op) },
                        D2D1_GRADIENT_STOP { position: 0.78, color: rgba_u8(255, 214, 48, 1.0 * op) },
                        D2D1_GRADIENT_STOP { position: 1.0, color: rgba_u8(255, 54, 62, 1.0 * op) },
                    ];
                    create_grad(rt, grad_start, grad_end, &vu_stops)
                }
            } else if mirror {
                let mirror_stops = [
                    D2D1_GRADIENT_STOP { position: 0.00, color: pal.light_col(1.0 * op) },
                    D2D1_GRADIENT_STOP { position: 0.24, color: pal.accent_col(0.98 * op) },
                    D2D1_GRADIENT_STOP { position: 0.50, color: pal.deep_col(0.90 * op) },
                    D2D1_GRADIENT_STOP { position: 0.76, color: pal.accent_col(0.98 * op) },
                    D2D1_GRADIENT_STOP { position: 1.00, color: pal.light_col(1.0 * op) },
                ];
                create_grad(rt, grad_start, grad_end, &mirror_stops)
            } else {
                let stops = [
                    D2D1_GRADIENT_STOP { position: 0.0, color: pal.deep_col(0.90 * op) },
                    D2D1_GRADIENT_STOP { position: 0.52, color: pal.accent_col(0.98 * op) },
                    D2D1_GRADIENT_STOP { position: 1.0, color: pal.light_col(1.0 * op) },
                ];
                create_grad(rt, grad_start, grad_end, &stops)
            };
            let bloom_br = rt.CreateSolidColorBrush(&pal.accent_col(0.24 * op), None).ok();
            let peak_br = rt.CreateSolidColorBrush(&pal.text_col(0.96 * op), None).ok();
            let vu_pk_green = if vu_tricolor {
                rt.CreateSolidColorBrush(&rgba_u8(190, 255, 212, 0.98 * op), None).ok()
            } else {
                None
            };
            let vu_pk_yellow = if vu_tricolor {
                rt.CreateSolidColorBrush(&rgba_u8(255, 230, 88, 0.98 * op), None).ok()
            } else {
                None
            };
            let vu_pk_red = if vu_tricolor {
                rt.CreateSolidColorBrush(&rgba_u8(255, 72, 78, 1.0 * op), None).ok()
            } else {
                None
            };
            let led_slot_br = if vu_tricolor {
                rt.CreateSolidColorBrush(&rgba_u8(8, 12, 12, 0.44), None).ok()
            } else {
                None
            };

            for i in 0..n_bars {
                let p = (i as f32 + 0.5) / n_bars as f32;
                let (amp, peak) = sample_band(p);
                if amp < 0.004 && peak < 0.008 {
                    continue;
                }
                let sc = pad + (i as f32 + 0.5) * step;
                let s0 = sc - bw * 0.5;
                let s1 = sc + bw * 0.5;
                let rr_rad = if vu_tricolor {
                    (bw * 0.25).min(2.2 * u)
                } else {
                    (bw * 0.45).min(4.0 * u)
                };

                if mirror {
                    let mid = cross_len * 0.5;
                    let half = (amp * max_reach).max(if amp > 0.008 { 1.4 * u } else { 0.0 });
                    if half > 0.5 {
                        if let Some(ref bl) = bloom_br {
                            let br = map_rect(
                                s0 - 1.3 * u,
                                s1 + 1.3 * u,
                                mid - half - 1.8 * u,
                                mid + half + 1.8 * u,
                            );
                            rt.FillRoundedRectangle(
                                &D2D1_ROUNDED_RECT { rect: br, radiusX: rr_rad, radiusY: rr_rad },
                                bl,
                            );
                        }
                        if let Some(ref gr) = bar_br {
                            let rc = map_rect(s0, s1, mid - half, mid + half);
                            rt.FillRoundedRectangle(
                                &D2D1_ROUNDED_RECT { rect: rc, radiusX: rr_rad, radiusY: rr_rad },
                                gr,
                            );
                        }
                        if let Some(ref slot_br) = led_slot_br {
                            let seg_step = (4.0 * u).max(2.6);
                            let mut off = seg_step;
                            while off < half - 1.0 * u {
                                let gap_up = map_rect(s0, s1, mid + off, mid + off + 1.05 * u);
                                let gap_dn = map_rect(s0, s1, mid - off - 1.05 * u, mid - off);
                                rt.FillRectangle(&gap_up, slot_br);
                                rt.FillRectangle(&gap_dn, slot_br);
                                off += seg_step;
                            }
                        }
                    }
                    if peak > amp + 0.025 && peak > 0.04 {
                        let chosen_pk = if vu_tricolor {
                            if peak >= 0.76 {
                                vu_pk_red.as_ref().or(peak_br.as_ref())
                            } else if peak >= 0.52 {
                                vu_pk_yellow.as_ref().or(peak_br.as_ref())
                            } else {
                                vu_pk_green.as_ref().or(peak_br.as_ref())
                            }
                        } else {
                            peak_br.as_ref()
                        };
                        if let Some(pk) = chosen_pk {
                            let ph = (peak * max_reach).min(cross_len * 0.47);
                            let prc_top = map_rect(s0, s1, mid + ph, mid + ph + 1.9 * u);
                            let prc_bot = map_rect(s0, s1, mid - ph - 1.9 * u, mid - ph);
                            rt.FillRoundedRectangle(
                                &D2D1_ROUNDED_RECT { rect: prc_top, radiusX: rr_rad * 0.6, radiusY: rr_rad * 0.6 },
                                pk,
                            );
                            rt.FillRoundedRectangle(
                                &D2D1_ROUNDED_RECT { rect: prc_bot, radiusX: rr_rad * 0.6, radiusY: rr_rad * 0.6 },
                                pk,
                            );
                        }
                    }
                } else {
                    let base = 2.0 * u;
                    let reach = (amp * max_reach).max(if amp > 0.008 { 1.6 * u } else { 0.0 });
                    if reach > 0.5 {
                        if let Some(ref bl) = bloom_br {
                            let br = map_rect(s0 - 1.3 * u, s1 + 1.3 * u, base, base + reach + 2.2 * u);
                            rt.FillRoundedRectangle(
                                &D2D1_ROUNDED_RECT { rect: br, radiusX: rr_rad, radiusY: rr_rad },
                                bl,
                            );
                        }
                        if let Some(ref gr) = bar_br {
                            let rc = map_rect(s0, s1, base, base + reach);
                            rt.FillRoundedRectangle(
                                &D2D1_ROUNDED_RECT { rect: rc, radiusX: rr_rad, radiusY: rr_rad },
                                gr,
                            );
                        }
                        // Crisp hardware LED segment dividers for VuPeakStudio
                        if let Some(ref slot_br) = led_slot_br {
                            let seg_step = (4.2 * u).max(2.6);
                            let mut c_cur = base + seg_step;
                            while c_cur < base + reach - 1.2 * u {
                                let gap_rc = map_rect(s0, s1, c_cur, c_cur + 1.15 * u);
                                rt.FillRectangle(&gap_rc, slot_br);
                                c_cur += seg_step;
                            }
                        }
                    }
                    if peak > amp + 0.02 && peak > 0.03 {
                        let chosen_pk = if vu_tricolor {
                            if peak >= 0.76 {
                                vu_pk_red.as_ref().or(peak_br.as_ref())
                            } else if peak >= 0.52 {
                                vu_pk_yellow.as_ref().or(peak_br.as_ref())
                            } else {
                                vu_pk_green.as_ref().or(peak_br.as_ref())
                            }
                        } else {
                            peak_br.as_ref()
                        };
                        if let Some(pk) = chosen_pk {
                            let pc = (base + peak * max_reach).min(cross_len - 3.2 * u);
                            let prc = map_rect(s0, s1, pc, pc + 2.2 * u);
                            rt.FillRoundedRectangle(
                                &D2D1_ROUNDED_RECT { rect: prc, radiusX: rr_rad * 0.6, radiusY: rr_rad * 0.6 },
                                pk,
                            );
                        }
                    }
                }
            }
        }
        TaskbarVizStyle::AuroraWave => {
            if let Some(factory) = d2d {
            let pts_count = 44usize;
            let max_reach = if mirror { cross_len * 0.46 } else { cross_len * 0.88 };
            let base = if mirror { cross_len * 0.5 } else { 2.0 * u };
            let env_scale = (viz.energy * 1.45 + viz.beat_pulse * 0.45).clamp(0.0, 1.0);
            if env_scale >= 0.008 {

            let mut upper = Vec::with_capacity(pts_count);
            let mut lower = Vec::with_capacity(pts_count);
            let mut harmonic = Vec::with_capacity(pts_count);
            for i in 0..pts_count {
                let p = i as f32 / (pts_count - 1) as f32;
                let s_pos = pad + p * avail;
                let taper = (p * std::f32::consts::PI).sin().max(0.0).powf(0.42);
                let (amp, pk) = sample_band(p);
                let osc1 = 0.62 + 0.38 * (viz.phase * 1.55 - p * std::f32::consts::TAU * 2.2).sin();
                let osc2 = 0.62 + 0.38 * (viz.phase * 1.10 + p * std::f32::consts::TAU * 3.1).cos();
                let sig = (amp * 0.82 + pk * 0.18 + viz.beat_pulse * 0.16).clamp(0.0, 1.0);
                let h = (sig * (0.48 + 0.52 * osc1) * taper * max_reach)
                    .max(1.2 * u * taper * env_scale);
                let h2 = ((amp * 0.78 + viz.beat_pulse * 0.12) * (0.48 + 0.52 * osc2) * taper * max_reach)
                    .max(0.9 * u * taper * env_scale);
                if mirror {
                    upper.push(map_pt(s_pos, base + h));
                    lower.push(map_pt(s_pos, base - h2));
                    harmonic.push(map_pt(s_pos, base + (h - h2) * 0.55));
                } else {
                    upper.push(map_pt(s_pos, base + h));
                    lower.push(map_pt(s_pos, base));
                    harmonic.push(map_pt(s_pos, base + h2 * 0.68));
                }
            }

            if let Ok(geo) = factory.CreatePathGeometry() {
                if let Ok(sink) = geo.Open() {
                    sink.BeginFigure(upper[0], D2D1_FIGURE_BEGIN_FILLED);
                    sink.AddLines(&upper[1..]);
                    for pt in lower.iter().rev() {
                        sink.AddLines(&[*pt]);
                    }
                    sink.EndFigure(D2D1_FIGURE_END_CLOSED);
                    if sink.Close().is_ok() {
                        let w_br = if mirror {
                            let m_stops = [
                                D2D1_GRADIENT_STOP { position: 0.00, color: pal.light_col(0.68 * op * env_scale) },
                                D2D1_GRADIENT_STOP { position: 0.26, color: pal.accent_col(0.80 * op * env_scale) },
                                D2D1_GRADIENT_STOP { position: 0.50, color: pal.deep_col(0.46 * op * env_scale) },
                                D2D1_GRADIENT_STOP { position: 0.74, color: pal.accent_col(0.80 * op * env_scale) },
                                D2D1_GRADIENT_STOP { position: 1.00, color: pal.light_col(0.68 * op * env_scale) },
                            ];
                            create_grad(
                                rt,
                                map_pt(span_len * 0.5, base - max_reach),
                                map_pt(span_len * 0.5, base + max_reach),
                                &m_stops,
                            )
                        } else {
                            let stops = [
                                D2D1_GRADIENT_STOP { position: 0.0, color: pal.deep_col(0.42 * op * env_scale) },
                                D2D1_GRADIENT_STOP { position: 0.52, color: pal.accent_col(0.80 * op * env_scale) },
                                D2D1_GRADIENT_STOP { position: 1.0, color: pal.light_col(0.66 * op * env_scale) },
                            ];
                            create_grad(
                                rt,
                                map_pt(span_len * 0.5, base),
                                map_pt(span_len * 0.5, base + max_reach),
                                &stops,
                            )
                        };
                        if let Some(w_br) = w_br {
                            rt.FillGeometry(&geo, &w_br, None);
                        }
                    }
                }
            }

            if let Ok(harm_br) = rt.CreateSolidColorBrush(&pal.accent_col(0.82 * op * env_scale), None) {
                for w in harmonic.windows(2) {
                    rt.DrawLine(w[0], w[1], &harm_br, 1.45 * u, None);
                }
            }
            if let Ok(crest_br) = rt.CreateSolidColorBrush(&pal.light_col(0.98 * op * env_scale), None) {
                for w in upper.windows(2) {
                    rt.DrawLine(w[0], w[1], &crest_br, 2.15 * u, None);
                }
                if mirror {
                    for w in lower.windows(2) {
                        rt.DrawLine(w[0], w[1], &crest_br, 1.65 * u, None);
                    }
                }
            }
            }
            }
        }
        TaskbarVizStyle::EdgePulse => {
            let pulse_env = (viz.energy * 0.65 + viz.beat_pulse * 0.65).clamp(0.0, 1.0);
            if pulse_env >= 0.008 {
            let wash_stops = [
                D2D1_GRADIENT_STOP { position: 0.0, color: pal.accent_col((0.12 + 0.48 * pulse_env) * op) },
                D2D1_GRADIENT_STOP { position: 0.55, color: pal.deep_col((0.06 + 0.24 * pulse_env) * op) },
                D2D1_GRADIENT_STOP { position: 1.0, color: pal.accent_col(0.0) },
            ];
            if let Some(w_br) = create_grad(
                rt,
                map_pt(span_len * 0.5, 0.0),
                map_pt(span_len * 0.5, cross_len * 0.68),
                &wash_stops,
            ) {
                let wash_rc = map_rect(pad, span_len - pad, 1.5 * u, cross_len * 0.68);
                rt.FillRoundedRectangle(
                    &D2D1_ROUNDED_RECT { rect: wash_rc, radiusX: rad * 0.4, radiusY: rad * 0.4 },
                    &w_br,
                );
            }
            if mirror {
                if let Some(w_top) = create_grad(
                    rt,
                    map_pt(span_len * 0.5, cross_len),
                    map_pt(span_len * 0.5, cross_len * 0.36),
                    &wash_stops,
                ) {
                    let wash_rc_top = map_rect(pad, span_len - pad, cross_len * 0.36, cross_len - 1.5 * u);
                    rt.FillRoundedRectangle(
                        &D2D1_ROUNDED_RECT { rect: wash_rc_top, radiusX: rad * 0.4, radiusY: rad * 0.4 },
                        &w_top,
                    );
                }
            }
            let segs = 22usize;
            let step = avail / segs as f32;
            let seg_len = (step * 0.74).max(4.0 * u);
            for i in 0..segs {
                let p = (i as f32 + 0.5) / segs as f32;
                let (amp, pk) = sample_band(p);
                let lvl = (amp * 0.85 + pk * 0.15).clamp(0.0, 1.0);
                if lvl < 0.01 {
                    continue;
                }
                let sc = pad + (i as f32 + 0.5) * step;
                let th = (1.6 + lvl * 6.4) * u;
                let c0 = 1.6 * u;
                let rc = map_rect(sc - seg_len * 0.5, sc + seg_len * 0.5, c0, c0 + th);
                let col = if lvl > 0.52 {
                    pal.light_col((0.75 + 0.25 * lvl) * op)
                } else {
                    pal.accent_col((0.35 + 0.62 * lvl) * op)
                };
                if let Ok(br) = rt.CreateSolidColorBrush(&col, None) {
                    rt.FillRoundedRectangle(&D2D1_ROUNDED_RECT { rect: rc, radiusX: 1.6 * u, radiusY: 1.6 * u }, &br);
                }
                if mirror {
                    let c_top = (cross_len - th * 0.78 - 1.6 * u).max(c0 + th + 2.0 * u);
                    let rc_top = map_rect(sc - seg_len * 0.5, sc + seg_len * 0.5, c_top, c_top + th * 0.78);
                    if let Ok(br2) = rt.CreateSolidColorBrush(&pal.accent_col((0.28 + 0.56 * lvl) * op), None) {
                        rt.FillRoundedRectangle(&D2D1_ROUNDED_RECT { rect: rc_top, radiusX: 1.4 * u, radiusY: 1.4 * u }, &br2);
                    }
                }
            }
            }
        }
        TaskbarVizStyle::AmbientGlow => {
            let bass = (viz.bands[0] + viz.bands[1] + viz.bands[2]) / 3.0;
            let mid = (viz.bands[6] + viz.bands[8] + viz.bands[10]) / 3.0;
            let high = (viz.bands[16] + viz.bands[19] + viz.bands[22]) / 3.0;
            let shift = (viz.phase * 0.65).sin() * 0.22;
            let stops = [
                D2D1_GRADIENT_STOP { position: 0.0, color: pal.deep_col((0.22 + 0.38 * bass) * op) },
                D2D1_GRADIENT_STOP { position: (0.5 + shift).clamp(0.18, 0.82), color: pal.accent_col((0.26 + 0.44 * mid) * op) },
                D2D1_GRADIENT_STOP { position: 1.0, color: pal.light_col((0.18 + 0.36 * high) * op) },
            ];
            if let Some(a_br) = create_grad(rt, map_pt(0.0, cross_len * 0.5), map_pt(span_len, cross_len * 0.5), &stops) {
                let rc = D2D_RECT_F {
                    left: l + 1.5 * u,
                    top: t + 1.5 * u,
                    right: r - 1.5 * u,
                    bottom: b - 1.5 * u,
                };
                let r_in = (rad - 1.5 * u).max(2.0);
                rt.FillRoundedRectangle(&D2D1_ROUNDED_RECT { rect: rc, radiusX: r_in, radiusY: r_in }, &a_br);
            }
        }
        TaskbarVizStyle::AuroraFlow => {
            // Live Theme 1: Aurora Borealis multi-ribbon silk waves (emerald-teal, gold, violet-rose)
            let Some(factory) = d2d else { return };
            let ribbons: [(D2D1_COLOR_F, D2D1_COLOR_F, f32, f32, f32); 3] = [
                (
                    rgba_u8(46, 224, 186, 0.52 * op),
                    rgba_u8(56, 245, 205, 0.92 * op),
                    1.0,
                    0.0,
                    0.78,
                ),
                (
                    pal.accent_col(0.48 * op),
                    pal.light_col(0.90 * op),
                    1.35,
                    2.1,
                    0.62,
                ),
                (
                    rgba_u8(186, 104, 248, 0.42 * op),
                    rgba_u8(232, 150, 255, 0.84 * op),
                    0.85,
                    4.2,
                    0.48,
                ),
            ];
            let pts_count = 36usize;
            for (fill_col, crest_col, spd, ph_off, height_sc) in ribbons {
                let mut top_pts = Vec::with_capacity(pts_count);
                let mut bot_pts = Vec::with_capacity(pts_count);
                for i in 0..pts_count {
                    let p = i as f32 / (pts_count - 1) as f32;
                    let s_pos = pad + p * avail;
                    let env = (p * std::f32::consts::PI).sin().max(0.0).powf(0.35);
                    let (amp, _) = sample_band(p);
                    let w1 = (viz.phase * spd + ph_off + p * 7.5).sin() * 0.5 + 0.5;
                    let w2 = (viz.phase * (spd * 0.7) - ph_off + p * 12.0).cos() * 0.5 + 0.5;
                    let c_top = (0.18 + (0.48 * w1 + 0.34 * amp) * height_sc) * env * cross_len;
                    let c_bot = (0.04 + 0.22 * w2 * height_sc) * env * cross_len;
                    top_pts.push(map_pt(s_pos, c_top.clamp(2.0 * u, cross_len - 1.5 * u)));
                    bot_pts.push(map_pt(s_pos, c_bot.clamp(1.0 * u, cross_len * 0.55)));
                }
                if let Ok(geo) = factory.CreatePathGeometry() {
                    if let Ok(sink) = geo.Open() {
                        sink.BeginFigure(top_pts[0], D2D1_FIGURE_BEGIN_FILLED);
                        sink.AddLines(&top_pts[1..]);
                        for pt in bot_pts.iter().rev() {
                            sink.AddLines(&[*pt]);
                        }
                        sink.EndFigure(D2D1_FIGURE_END_CLOSED);
                        if sink.Close().is_ok() {
                            if let Ok(fb) = rt.CreateSolidColorBrush(&fill_col, None) {
                                rt.FillGeometry(&geo, &fb, None);
                            }
                        }
                    }
                }
                if let Ok(cb) = rt.CreateSolidColorBrush(&crest_col, None) {
                    for w in top_pts.windows(2) {
                        rt.DrawLine(w[0], w[1], &cb, 1.5 * u, None);
                    }
                }
            }
        }
        TaskbarVizStyle::CyberScan => {
            // Live Theme 2: Cyberpunk HUD Dock — precision tick ruler, corner brackets & sweeping neon laser
            let cyan_hi = rgba_u8(56, 238, 255, 0.94 * op);
            let cyan_glow = rgba_u8(56, 238, 255, 0.26 * op);
            let mag_hi = pal.accent_col(0.92 * op);

            // Micro-tick ruler along bottom/dock edge
            let n_ticks = ((avail / (9.0 * u)).round() as usize).clamp(18, 52);
            let _step = avail / n_ticks as f32;
            let scan_p = ((viz.phase * 0.42).sin() * 0.5 + 0.5).clamp(0.05, 0.95);

            for i in 0..n_ticks {
                let p = (i as f32 + 0.5) / n_ticks as f32;
                let s_pos = pad + p * avail;
                let dist_scan = ((p - scan_p).abs() * 5.5).min(1.0);
                let scan_boost = (1.0 - dist_scan).powi(2);
                let (amp, _) = sample_band(p);
                let is_major = i % 4 == 0;
                let tick_h = ((if is_major { 5.0 } else { 2.8 }) + scan_boost * 9.5 + amp * 5.5) * u;
                let col = if scan_boost > 0.35 {
                    cyan_hi
                } else if is_major {
                    mag_hi
                } else {
                    rgba_u8(56, 238, 255, (0.35 + 0.40 * amp) * op)
                };
                if let Ok(br) = rt.CreateSolidColorBrush(&col, None) {
                    let p0 = map_pt(s_pos, 1.8 * u);
                    let p1 = map_pt(s_pos, (1.8 * u + tick_h).min(cross_len - 2.0 * u));
                    rt.DrawLine(p0, p1, &br, (if is_major { 1.8 } else { 1.1 }) * u, None);
                }
            }

            // Sweeping laser beam glow
            let scan_s = pad + scan_p * avail;
            let beam_w = (avail * 0.14).max(18.0 * u);
            let beam_rc = map_rect(
                (scan_s - beam_w).max(pad),
                (scan_s + beam_w).min(span_len - pad),
                1.5 * u,
                cross_len * 0.78,
            );
            if let Ok(gb) = rt.CreateSolidColorBrush(&cyan_glow, None) {
                rt.FillRoundedRectangle(
                    &D2D1_ROUNDED_RECT { rect: beam_rc, radiusX: 4.0 * u, radiusY: 4.0 * u },
                    &gb,
                );
            }
            if let Ok(lb) = rt.CreateSolidColorBrush(&cyan_hi, None) {
                rt.DrawLine(
                    map_pt((scan_s - beam_w * 0.65).max(pad), 2.2 * u),
                    map_pt((scan_s + beam_w * 0.65).min(span_len - pad), 2.2 * u),
                    &lb,
                    2.4 * u,
                    None,
                );
            }
        }
        TaskbarVizStyle::EmberDrift => {
            // Live Theme 3: Sunset Ember Drift — warm hearth horizon glow + floating firefly embers
            let hearth_stops = [
                D2D1_GRADIENT_STOP { position: 0.0, color: rgba_u8(255, 110, 48, (0.28 + 0.25 * viz.energy) * op) },
                D2D1_GRADIENT_STOP { position: 0.55, color: pal.accent_col((0.14 + 0.18 * viz.energy) * op) },
                D2D1_GRADIENT_STOP { position: 1.0, color: rgba_u8(255, 180, 72, 0.0) },
            ];
            if let Some(h_br) = create_grad(
                rt,
                map_pt(span_len * 0.5, 0.0),
                map_pt(span_len * 0.5, cross_len * 0.72),
                &hearth_stops,
            ) {
                let rc = map_rect(pad, span_len - pad, 1.0 * u, cross_len * 0.72);
                rt.FillRoundedRectangle(
                    &D2D1_ROUNDED_RECT { rect: rc, radiusX: rad * 0.4, radiusY: rad * 0.4 },
                    &h_br,
                );
            }

            let n_embers = 20usize;
            for i in 0..n_embers {
                let seed = i as f32 * 1.618034;
                let p_base = (i as f32 + 0.5) / n_embers as f32;
                let sway = (viz.phase * (0.42 + (i % 5) as f32 * 0.08) + seed * 4.0).sin() * 0.025;
                let p = (p_base + sway).clamp(0.03, 0.97);
                let (amp, _) = sample_band(p);
                let rise = ((viz.phase * (0.14 + (i % 4) as f32 * 0.04) + seed * 3.1) % 1.0 + 1.0) % 1.0;
                let fade = (rise * std::f32::consts::PI).sin();
                let s_pos = pad + p * avail;
                let c_pos = (3.0 * u + rise * (cross_len * 0.78)).min(cross_len - 3.0 * u);
                let pt = map_pt(s_pos, c_pos);
                let r_ember = (1.6 + (i % 3) as f32 * 0.8 + amp * 2.2) * u;

                if let Ok(halo) = rt.CreateSolidColorBrush(&rgba_u8(255, 132, 54, 0.34 * fade * op), None) {
                    rt.FillEllipse(
                        &windows::Win32::Graphics::Direct2D::D2D1_ELLIPSE {
                            point: pt,
                            radiusX: r_ember * 2.2,
                            radiusY: r_ember * 2.2,
                        },
                        &halo,
                    );
                }
                if let Ok(core) = rt.CreateSolidColorBrush(&rgba_u8(255, 236, 182, 0.94 * fade * op), None) {
                    rt.FillEllipse(
                        &windows::Win32::Graphics::Direct2D::D2D1_ELLIPSE {
                            point: pt,
                            radiusX: r_ember * 0.85,
                            radiusY: r_ember * 0.85,
                        },
                        &core,
                    );
                }
            }
        }
        TaskbarVizStyle::ChromeSweep => {
            // Live Theme 4: Liquid Chrome Island — metallic mirror sheen + sweeping specular highlight
            let sweep_p = ((viz.phase * 0.32).sin() * 0.5 + 0.5).clamp(0.08, 0.92);
            let stops = [
                D2D1_GRADIENT_STOP { position: 0.0, color: rgba_u8(160, 178, 205, 0.10 * op) },
                D2D1_GRADIENT_STOP {
                    position: (sweep_p - 0.14).max(0.02),
                    color: rgba_u8(200, 215, 238, 0.18 * op),
                },
                D2D1_GRADIENT_STOP {
                    position: sweep_p,
                    color: rgba_u8(255, 250, 240, (0.52 + 0.24 * viz.energy) * op),
                },
                D2D1_GRADIENT_STOP {
                    position: (sweep_p + 0.14).min(0.98),
                    color: pal.accent_col(0.28 * op),
                },
                D2D1_GRADIENT_STOP { position: 1.0, color: rgba_u8(140, 155, 180, 0.10 * op) },
            ];
            if let Some(c_br) = create_grad(
                rt,
                map_pt(pad, 0.0),
                map_pt(span_len - pad, cross_len),
                &stops,
            ) {
                let rc = D2D_RECT_F {
                    left: l + 1.5 * u,
                    top: t + 1.5 * u,
                    right: r - 1.5 * u,
                    bottom: b - 1.5 * u,
                };
                let r_in = (rad - 1.5 * u).max(2.0);
                rt.FillRoundedRectangle(
                    &D2D1_ROUNDED_RECT { rect: rc, radiusX: r_in, radiusY: r_in },
                    &c_br,
                );
            }
            // Twin chrome horizon wave lines
            let pts_count = 32usize;
            if let Ok(line_br) = rt.CreateSolidColorBrush(&rgba_u8(255, 252, 245, 0.78 * op), None) {
                let mut prev = None;
                for i in 0..pts_count {
                    let p = i as f32 / (pts_count - 1) as f32;
                    let s_pos = pad + p * avail;
                    let env = (p * std::f32::consts::PI).sin();
                    let (amp, _) = sample_band(p);
                    let c_val = (3.0 * u + (0.18 + 0.28 * amp * (viz.phase * 1.4 + p * 8.0).sin().abs()) * env * cross_len)
                        .clamp(2.0 * u, cross_len - 2.0 * u);
                    let cur = map_pt(s_pos, c_val);
                    if let Some(prv) = prev {
                        rt.DrawLine(prv, cur, &line_br, 1.5 * u, None);
                    }
                    prev = Some(cur);
                }
            }
        }
        TaskbarVizStyle::StarlightPulse => {
            // Live Theme 5: Starlight Minimal — twinkling constellation nodes & gossamer light threads
            let n_stars = 16usize;
            let mut nodes = Vec::with_capacity(n_stars);
            for i in 0..n_stars {
                let p = (i as f32 + 0.5) / n_stars as f32;
                let (amp, _) = sample_band(p);
                let s_pos = pad + p * avail;
                let wave_c = 0.25 + 0.50 * ((viz.phase * 0.75 + i as f32 * 1.35).sin() * 0.5 + 0.5);
                let c_pos = (wave_c * cross_len).clamp(3.5 * u, cross_len - 3.5 * u);
                let twinkle = ((viz.phase * 1.6 + i as f32 * 2.1).sin() * 0.5 + 0.5) * 0.7 + amp * 0.3;
                nodes.push((map_pt(s_pos, c_pos), twinkle.clamp(0.2, 1.0)));
            }

            if let Ok(thread_br) = rt.CreateSolidColorBrush(&pal.accent_col(0.36 * op), None) {
                for w in nodes.windows(2) {
                    rt.DrawLine(w[0].0, w[1].0, &thread_br, 1.05 * u, None);
                }
            }

            for (pt, tw) in nodes {
                let r_star = (1.5 + tw * 2.2) * u;
                if let Ok(glow_br) = rt.CreateSolidColorBrush(&pal.accent_col(0.32 * tw * op), None) {
                    rt.FillEllipse(
                        &windows::Win32::Graphics::Direct2D::D2D1_ELLIPSE {
                            point: pt,
                            radiusX: r_star * 2.4,
                            radiusY: r_star * 2.4,
                        },
                        &glow_br,
                    );
                }
                if let Ok(star_br) = rt.CreateSolidColorBrush(&pal.text_col(0.95 * tw * op), None) {
                    rt.FillEllipse(
                        &windows::Win32::Graphics::Direct2D::D2D1_ELLIPSE {
                            point: pt,
                            radiusX: r_star * 0.75,
                            radiusY: r_star * 0.75,
                        },
                        &star_br,
                    );
                    let flare = r_star * 1.9;
                    rt.DrawLine(
                        Vector2 { X: pt.X - flare, Y: pt.Y },
                        Vector2 { X: pt.X + flare, Y: pt.Y },
                        &star_br,
                        0.9 * u,
                        None,
                    );
                    rt.DrawLine(
                        Vector2 { X: pt.X, Y: pt.Y - flare },
                        Vector2 { X: pt.X, Y: pt.Y + flare },
                        &star_br,
                        0.9 * u,
                        None,
                    );
                }
            }
        }
        TaskbarVizStyle::HoverSnow => {
            // Interactive Element 1: Winter Snowfall — frosted mist + 6-arm branched crystalline snowflakes swept by cursor
            let clip_guard = push_capsule_clip(rt, d2d, l, t, r, b, rad);

            // Soft arctic twilight frost bed gradient
            let frost_stops = [
                D2D1_GRADIENT_STOP { position: 0.0, color: rgba_u8(120, 206, 255, 0.04 * op) },
                D2D1_GRADIENT_STOP { position: 0.72, color: rgba_u8(168, 226, 255, 0.16 * op) },
                D2D1_GRADIENT_STOP { position: 1.0, color: rgba_u8(224, 246, 255, 0.28 * op) },
            ];
            if let Some(fbr) = create_grad(rt, Vector2 { X: l, Y: t }, Vector2 { X: l, Y: b }, &frost_stops) {
                rt.FillRectangle(&D2D_RECT_F { left: l, top: t, right: r, bottom: b }, &fbr);
            }

            // Cursor arctic swirl frost aura when mouse hovers taskbar
            if viz.hover_mix > 0.02 {
                if let Ok(sw_br) = rt.CreateSolidColorBrush(
                    &rgba_u8(205, 242, 255, 0.20 * viz.hover_mix * op),
                    None,
                ) {
                    let aura_r = 26.0 * eff_u;
                    rt.FillEllipse(&D2D1_ELLIPSE { point: hx_px, radiusX: aura_r, radiusY: aura_r }, &sw_br);
                    sw_br.SetColor(&rgba_u8(240, 252, 255, 0.34 * viz.hover_mix * op));
                    rt.DrawEllipse(
                        &D2D1_ELLIPSE { point: hx_px, radiusX: aura_r * 0.65, radiusY: aura_r * 0.65 },
                        &sw_br,
                        1.1 * eff_u,
                        None,
                    );
                }
            }

            if let Ok(br) = rt.CreateSolidColorBrush(&rgba_u8(255, 255, 255, 1.0), None) {
                for p in &viz.particles {
                    let pt = to_px(p.x, p.y);
                    if pt.X < l - 16.0 || pt.X > r + 16.0 || pt.Y < t - 16.0 || pt.Y > b + 16.0 {
                        continue;
                    }
                    let rad_f = (1.6 + p.size * 2.5) * eff_u;
                    let prox = if viz.hover_mix > 0.02 {
                        let d = (pt.X - hx_px.X).hypot(pt.Y - hx_px.Y);
                        (1.0 - (d / (54.0 * eff_u)).min(1.0)) * viz.hover_mix
                    } else {
                        0.0
                    };
                    let alpha = ((0.42 + p.size * 0.34 + prox * 0.24) * op).clamp(0.15, 0.98);

                    match p.kind {
                        0 => {
                            // Soft background snow bokeh
                            br.SetColor(&rgba_u8(190, 232, 255, alpha * 0.52));
                            rt.FillEllipse(&D2D1_ELLIPSE { point: pt, radiusX: rad_f * 1.15, radiusY: rad_f * 1.15 }, &br);
                        }
                        1 => {
                            // Crisp ice diamond + 4-way star twinkle
                            br.SetColor(&rgba_u8(226, 246, 255, alpha * 0.85));
                            rt.FillEllipse(&D2D1_ELLIPSE { point: pt, radiusX: rad_f * 0.42, radiusY: rad_f * 0.42 }, &br);
                            let arm = rad_f * 1.15;
                            let (s_a, c_a) = p.phase.sin_cos();
                            rt.DrawLine(
                                Vector2 { X: pt.X - c_a * arm, Y: pt.Y - s_a * arm },
                                Vector2 { X: pt.X + c_a * arm, Y: pt.Y + s_a * arm },
                                &br,
                                0.85 * eff_u,
                                None,
                            );
                            rt.DrawLine(
                                Vector2 { X: pt.X + s_a * arm, Y: pt.Y - c_a * arm },
                                Vector2 { X: pt.X - s_a * arm, Y: pt.Y + c_a * arm },
                                &br,
                                0.85 * eff_u,
                                None,
                            );
                        }
                        _ => {
                            // Hero 6-arm branched crystalline dendrite snowflake
                            let arm = rad_f * 1.55;
                            br.SetColor(&rgba_u8(210, 242, 255, alpha * 0.28));
                            rt.FillEllipse(&D2D1_ELLIPSE { point: pt, radiusX: arm * 1.15, radiusY: arm * 1.15 }, &br);
                            br.SetColor(&rgba_u8(248, 253, 255, alpha));
                            for k in 0..3 {
                                let ang = p.phase + (k as f32) * (std::f32::consts::PI / 3.0);
                                let (sa, ca) = ang.sin_cos();
                                let p_a = Vector2 { X: pt.X - ca * arm, Y: pt.Y - sa * arm };
                                let p_b = Vector2 { X: pt.X + ca * arm, Y: pt.Y + sa * arm };
                                rt.DrawLine(p_a, p_b, &br, 0.95 * eff_u, None);
                                // Micro crystalline side-barbs on both ends
                                for side in [-1.0f32, 1.0] {
                                    let bx = pt.X + ca * arm * 0.56 * side;
                                    let by = pt.Y + sa * arm * 0.56 * side;
                                    let b_len = arm * 0.34;
                                    for twig in [-0.72f32, 0.72] {
                                        let (st, ct) = (ang + twig).sin_cos();
                                        rt.DrawLine(
                                            Vector2 { X: bx, Y: by },
                                            Vector2 { X: bx + ct * b_len * side, Y: by + st * b_len * side },
                                            &br,
                                            0.75 * eff_u,
                                            None,
                                        );
                                    }
                                }
                            }
                            rt.FillEllipse(&D2D1_ELLIPSE { point: pt, radiusX: 1.0 * eff_u, radiusY: 1.0 * eff_u }, &br);
                        }
                    }
                }
            }
            pop_capsule_clip(rt, clip_guard);
        }
        TaskbarVizStyle::HoverFire => {
            // Interactive Element 2: Hearth Inferno — layered roaring flame curtains + fanned mouse spark inferno
            let clip_guard = push_capsule_clip(rt, d2d, l, t, r, b, rad);
            let some_factory = d2d;

            // Molten hearth base gradient
            let (g_start, g_end) = if is_vertical {
                let edge_x = if anchor_min { l } else { r };
                let mid_x = if anchor_min { r } else { l };
                (Vector2 { X: edge_x, Y: t + box_h * 0.5 }, Vector2 { X: mid_x, Y: t + box_h * 0.5 })
            } else {
                (Vector2 { X: l + box_w * 0.5, Y: b }, Vector2 { X: l + box_w * 0.5, Y: t })
            };
            let coal_stops = [
                D2D1_GRADIENT_STOP { position: 0.0, color: rgba_u8(255, 84, 18, (0.42 + 0.16 * viz.energy) * op) },
                D2D1_GRADIENT_STOP { position: 0.45, color: rgba_u8(232, 52, 16, (0.20 + 0.12 * viz.energy) * op) },
                D2D1_GRADIENT_STOP { position: 1.0, color: rgba_u8(120, 18, 8, 0.02 * op) },
            ];
            if let Some(cbr) = create_grad(rt, g_start, g_end, &coal_stops) {
                rt.FillRectangle(&D2D_RECT_F { left: l, top: t, right: r, bottom: b }, &cbr);
            }

            // Layered dynamic flame ribbons leaning with mouse wind & rising smoothly
            if let Some(factory) = some_factory {
                let layers: [(D2D1_COLOR_F, f32, f32, f32); 3] = [
                    (rgba_u8(220, 44, 14, 0.46 * op), 0.74, 1.0, 0.0),
                    (rgba_u8(255, 118, 22, 0.54 * op), 0.54, 1.45, 1.7),
                    (rgba_u8(255, 206, 68, 0.58 * op), 0.34, 2.05, 3.3),
                ];
                let wind_lean = (viz.hover_vx * 0.25 * viz.hover_mix).clamp(-0.45, 0.45);
                let steps = 38usize;
                for (col, max_reach_frac, spd, seed_off) in layers {
                    let mut pts = Vec::with_capacity(steps + 2);
                    for i in 0..steps {
                        let p = i as f32 / (steps - 1) as f32;
                        let s_px = pad + p * avail;
                        let edge_env = (p * std::f32::consts::PI).sin().max(0.0).powf(0.32);
                        let t1 = (viz.phase * spd * 1.45 + p * 11.5 + seed_off).sin() * 0.5 + 0.5;
                        let t2 = (viz.phase * spd * 2.25 - p * 19.0 + seed_off * 1.3).cos() * 0.5 + 0.5;
                        let flame_n = (0.35 + 0.65 * (t1 * 0.58 + t2 * 0.42)) * max_reach_frac;

                        let base_pt = map_pt(s_px, 0.0);
                        let dist_hover = (base_pt.X - hx_px.X).hypot(base_pt.Y - hx_px.Y);
                        let hover_flare = if viz.hover_mix > 0.02 {
                            (1.0 - (dist_hover / (68.0 * eff_u)).min(1.0)).powi(2) * 0.36 * viz.hover_mix
                        } else {
                            0.0
                        };

                        let reach = ((flame_n + hover_flare) * edge_env * cross_len)
                            .clamp(2.0 * eff_u, cross_len * 0.94);
                        let tip = map_pt((s_px + wind_lean * reach * 0.9).clamp(l, r + b), reach);
                        if is_vertical {
                            let y_shifted = (tip.Y - wind_lean.abs() * 6.0 * eff_u + viz.hover_vy * 14.0 * eff_u * viz.hover_mix)
                                .clamp(t, b);
                            pts.push(Vector2 { X: tip.X, Y: y_shifted });
                        } else {
                            let x_shifted = (s_px + l + (tip.X - (s_px + l)) * 0.4 + wind_lean * reach * 1.1)
                                .clamp(l, r);
                            pts.push(Vector2 { X: x_shifted, Y: b - reach });
                        }
                    }
                    if is_vertical {
                        let base_x = if anchor_min { l } else { r };
                        pts.push(Vector2 { X: base_x, Y: b });
                        pts.push(Vector2 { X: base_x, Y: t });
                    } else {
                        pts.push(Vector2 { X: r, Y: b });
                        pts.push(Vector2 { X: l, Y: b });
                    }
                    if let Ok(geo) = factory.CreatePathGeometry() {
                        if let Ok(sink) = geo.Open() {
                            sink.BeginFigure(pts[0], D2D1_FIGURE_BEGIN_FILLED);
                            sink.AddLines(&pts[1..]);
                            sink.EndFigure(D2D1_FIGURE_END_CLOSED);
                            if sink.Close().is_ok() {
                                if let Ok(fb) = rt.CreateSolidColorBrush(&col, None) {
                                    rt.FillGeometry(&geo, &fb, None);
                                }
                            }
                        }
                    }
                }
            }

            // Mouse hover incandescent hotspot flare
            if viz.hover_mix > 0.02 {
                if let Ok(fl_br) = rt.CreateSolidColorBrush(
                    &rgba_u8(255, 224, 120, 0.32 * viz.hover_mix * op),
                    None,
                ) {
                    let fr = 24.0 * eff_u;
                    rt.FillEllipse(&D2D1_ELLIPSE { point: hx_px, radiusX: fr, radiusY: fr }, &fl_br);
                    fl_br.SetColor(&rgba_u8(255, 250, 210, 0.56 * viz.hover_mix * op));
                    rt.FillEllipse(&D2D1_ELLIPSE { point: hx_px, radiusX: fr * 0.42, radiusY: fr * 0.42 }, &fl_br);
                }
            }

            // Rising incandescent sparks & ember trails
            if let Ok(sp_br) = rt.CreateSolidColorBrush(&rgba_u8(255, 220, 120, 1.0), None) {
                for p in &viz.particles {
                    let pt = to_px(p.x, p.y);
                    if pt.X < l - 10.0 || pt.X > r + 10.0 || pt.Y < t - 10.0 || pt.Y > b + 10.0 {
                        continue;
                    }
                    let life_k = p.life.clamp(0.0, 1.0);
                    let r_sp = (1.1 + p.size * 1.45) * eff_u * (0.45 + 0.55 * life_k);
                    let hot_g = (110.0 + life_k * 135.0) as u8;
                    let hot_b = (24.0 + (life_k * life_k) * 165.0) as u8;

                    sp_br.SetColor(&rgba_u8(255, 110, 24, 0.30 * life_k * op));
                    rt.FillEllipse(&D2D1_ELLIPSE { point: pt, radiusX: r_sp * 2.5, radiusY: r_sp * 2.5 }, &sp_br);

                    // Velocity motion spark streak
                    let streak_dx = (p.vx * box_w * 0.028).clamp(-10.0, 10.0);
                    let streak_dy = (p.vy * box_h * 0.028).clamp(-12.0, 8.0);
                    sp_br.SetColor(&rgba_u8(255, hot_g, hot_b, 0.92 * life_k * op));
                    rt.DrawLine(
                        pt,
                        Vector2 { X: pt.X - streak_dx, Y: pt.Y - streak_dy },
                        &sp_br,
                        (r_sp * 1.15).max(1.0),
                        None,
                    );
                    rt.FillEllipse(&D2D1_ELLIPSE { point: pt, radiusX: r_sp * 0.8, radiusY: r_sp * 0.8 }, &sp_br);
                }
            }
            pop_capsule_clip(rt, clip_guard);
        }
        TaskbarVizStyle::HoverWater => {
            // Interactive Element 3: Liquid Caustics & Concentric Ripples — caustic filaments + mouse-carved water rings
            let clip_guard = push_capsule_clip(rt, d2d, l, t, r, b, rad);

            // Translucent deep aqua-teal water volume wash
            let pool_stops = [
                D2D1_GRADIENT_STOP { position: 0.0, color: rgba_u8(16, 128, 178, 0.20 * op) },
                D2D1_GRADIENT_STOP { position: 0.52, color: rgba_u8(28, 198, 226, 0.24 * op) },
                D2D1_GRADIENT_STOP { position: 1.0, color: rgba_u8(10, 82, 134, 0.34 * op) },
            ];
            if let Some(pbr) = create_grad(rt, Vector2 { X: l, Y: t }, Vector2 { X: l, Y: b }, &pool_stops) {
                rt.FillRectangle(&D2D_RECT_F { left: l, top: t, right: r, bottom: b }, &pbr);
            }

            // Shimmering underwater caustic light mesh (warped locally around cursor wake)
            if let Ok(caust_br) = rt.CreateSolidColorBrush(&rgba_u8(120, 248, 255, 0.32 * op), None) {
                for strand in 0..3 {
                    let s_f = strand as f32;
                    let col = if strand == 1 {
                        rgba_u8(186, 255, 255, 0.36 * op)
                    } else {
                        rgba_u8(62, 224, 246, 0.24 * op)
                    };
                    caust_br.SetColor(&col);
                    let n_pts = 32usize;
                    let mut prev_pt: Option<Vector2> = None;
                    for i in 0..n_pts {
                        let p = i as f32 / (n_pts - 1) as f32;
                        let s_pos = pad + p * avail;
                        let wave = (viz.phase * (1.1 + s_f * 0.25) + p * 12.0 + s_f * 2.1).sin() * 0.5 + 0.5;
                        let cross_base = cross_len * (0.24 + 0.16 * s_f + 0.22 * wave);
                        let mut pt = map_pt(s_pos, cross_base.clamp(2.0 * eff_u, cross_len - 2.0 * eff_u));
                        if viz.hover_mix > 0.02 {
                            let dx = pt.X - hx_px.X;
                            let dy = pt.Y - hx_px.Y;
                            let d = dx.hypot(dy).max(1.0);
                            if d < 58.0 * eff_u {
                                let push = (1.0 - d / (58.0 * eff_u)).powi(2) * 8.0 * eff_u * viz.hover_mix;
                                pt.X += (dx / d) * push;
                                pt.Y += (dy / d) * push;
                            }
                        }
                        if let Some(prv) = prev_pt {
                            rt.DrawLine(prv, pt, &caust_br, (1.15 + s_f * 0.35) * eff_u, None);
                        }
                        prev_pt = Some(pt);
                    }
                }
            }

            // Concentric aspect-accurate water ripple rings radiating from cursor & raindrops
            if let Ok(rbr) = rt.CreateSolidColorBrush(&rgba_u8(185, 252, 255, 1.0), None) {
                for rip in &viz.ripples {
                    if !rip.active {
                        continue;
                    }
                    let c = to_px(rip.x, rip.y);
                    if c.X < l - 60.0 || c.X > r + 60.0 || c.Y < t - 60.0 || c.Y > b + 60.0 {
                        continue;
                    }
                    let k = (rip.age / rip.max_age).clamp(0.0, 1.0);
                    let fade = (1.0 - k).powf(1.35) * op;
                    let r_outer = (4.0 + rip.age * 70.0 * rip.strength) * eff_u;

                    // Soft dark trough ring for refracted glass depth
                    rbr.SetColor(&rgba_u8(10, 48, 76, 0.35 * fade));
                    rt.DrawEllipse(
                        &D2D1_ELLIPSE { point: c, radiusX: (r_outer - 2.2 * eff_u).max(1.5), radiusY: (r_outer - 2.2 * eff_u).max(1.5) },
                        &rbr,
                        2.4 * eff_u,
                        None,
                    );
                    // Primary bright sunlit caustic wavefront ring
                    rbr.SetColor(&rgba_u8(192, 253, 255, 0.88 * fade));
                    rt.DrawEllipse(
                        &D2D1_ELLIPSE { point: c, radiusX: r_outer, radiusY: r_outer },
                        &rbr,
                        1.65 * eff_u * (1.0 - k * 0.45),
                        None,
                    );
                    // Secondary trailing ripple
                    if r_outer > 11.0 * eff_u {
                        let r_in = r_outer * 0.62;
                        rbr.SetColor(&rgba_u8(90, 232, 252, 0.58 * fade));
                        rt.DrawEllipse(
                            &D2D1_ELLIPSE { point: c, radiusX: r_in, radiusY: r_in },
                            &rbr,
                            1.1 * eff_u,
                            None,
                        );
                    }
                    // Center droplet glint at impact
                    if k < 0.22 {
                        let drop_a = (1.0 - k / 0.22) * 0.85 * op;
                        rbr.SetColor(&rgba_u8(236, 255, 255, drop_a));
                        rt.FillEllipse(&D2D1_ELLIPSE { point: c, radiusX: 2.6 * eff_u, radiusY: 2.6 * eff_u }, &rbr);
                    }
                }

                // Direct mouse touch caustic halo + shimmering bubbles pushed by cursor wake
                if viz.hover_mix > 0.02 {
                    rbr.SetColor(&rgba_u8(130, 248, 255, 0.26 * viz.hover_mix * op));
                    rt.FillEllipse(&D2D1_ELLIPSE { point: hx_px, radiusX: 16.0 * eff_u, radiusY: 16.0 * eff_u }, &rbr);
                    rbr.SetColor(&rgba_u8(235, 255, 255, 0.62 * viz.hover_mix * op));
                    rt.FillEllipse(&D2D1_ELLIPSE { point: hx_px, radiusX: 3.0 * eff_u, radiusY: 3.0 * eff_u }, &rbr);
                }

                for p in viz.particles.iter().take(18) {
                    let pt = to_px(p.x, p.y);
                    let b_r = (1.1 + p.size * 0.9) * eff_u;
                    rbr.SetColor(&rgba_u8(190, 250, 255, 0.46 * op));
                    rt.DrawEllipse(&D2D1_ELLIPSE { point: pt, radiusX: b_r, radiusY: b_r }, &rbr, 0.9 * eff_u, None);
                }
            }
            pop_capsule_clip(rt, clip_guard);
        }
        TaskbarVizStyle::HoverSakura => {
            // Interactive Element 4: Sakura Breeze — 3D tumbling cherry petals parted by cursor breeze
            let rect = D2D_RECT_F { left: l, top: t, right: r, bottom: b };
            let rr_r = rad.min((r - l) * 0.5).min((b - t) * 0.5).max(0.0);
            let sakura_stops = [
                D2D1_GRADIENT_STOP { position: 0.0, color: rgba_u8(255, 118, 174, 0.14 * op) },
                D2D1_GRADIENT_STOP { position: 1.0, color: rgba_u8(168, 48, 112, 0.08 * op) },
            ];
            if let Some(sbr) = create_grad(rt, Vector2 { X: l, Y: t }, Vector2 { X: r, Y: b }, &sakura_stops) {
                if rr_r > 0.5 {
                    rt.FillRoundedRectangle(
                        &D2D1_ROUNDED_RECT { rect, radiusX: rr_r, radiusY: rr_r },
                        &sbr,
                    );
                } else {
                    rt.FillRectangle(&rect, &sbr);
                }
            }
            let inside_rr = |px: f32, py: f32, inset: f32| -> bool {
                let er = (rr_r - inset).max(0.0);
                if er <= 0.5 {
                    return px >= l && px <= r && py >= t && py <= b;
                }
                let il = l + er;
                let ir = r - er;
                let it = t + er;
                let ib = b - er;
                let dx = if px < il { il - px } else if px > ir { px - ir } else { 0.0 };
                let dy = if py < it { it - py } else if py > ib { py - ib } else { 0.0 };
                (dx * dx + dy * dy) <= (er * er)
            };
            rt.PushAxisAlignedClip(&rect, D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
            if viz.hover_mix > 0.02 && inside_rr(hx_px.X, hx_px.Y, 4.0 * eff_u) {
                if let Ok(wbr) = rt.CreateSolidColorBrush(&rgba_u8(255, 186, 218, 0.22 * viz.hover_mix * op), None) {
                    rt.FillEllipse(&D2D1_ELLIPSE { point: hx_px, radiusX: 24.0 * eff_u, radiusY: 24.0 * eff_u }, &wbr);
                }
            }
            if let Ok(pbr) = rt.CreateSolidColorBrush(&rgba_u8(255, 190, 216, 1.0), None) {
                for p in &viz.particles {
                    let pt = to_px(p.x, p.y);
                    if pt.X < l - 14.0 || pt.X > r + 14.0 || pt.Y < t - 14.0 || pt.Y > b + 14.0 {
                        continue;
                    }
                    let tumble = 0.42 + 0.58 * (p.phase * 0.7).cos().abs();
                    let rx = (2.6 + p.size * 2.4) * eff_u;
                    let ry = rx * 0.54 * tumble;
                    if !inside_rr(pt.X, pt.Y, (rx * 0.45).min(rr_r * 0.45)) {
                        continue;
                    }
                    let (sa, ca) = p.phase.sin_cos();
                    let alpha = (0.48 + p.size * 0.30) * op;
                    let col = if p.kind == 0 {
                        rgba_u8(255, 148, 188, alpha)
                    } else if p.kind == 1 {
                        rgba_u8(255, 206, 224, alpha * 0.92)
                    } else {
                        rgba_u8(255, 232, 240, alpha)
                    };
                    pbr.SetColor(&col);
                    // Twin notched sakura petal silhouette along local orientation axis
                    let p_lobe1 = Vector2 { X: pt.X + ca * rx * 0.25, Y: pt.Y + sa * rx * 0.25 };
                    let p_lobe2 = Vector2 { X: pt.X - sa * ry * 0.45, Y: pt.Y + ca * ry * 0.45 };
                    rt.FillEllipse(&D2D1_ELLIPSE { point: p_lobe1, radiusX: rx * 0.72, radiusY: ry.max(1.2) }, &pbr);
                    rt.FillEllipse(&D2D1_ELLIPSE { point: p_lobe2, radiusX: (rx * 0.58).max(1.3), radiusY: (ry * 0.82).max(1.1) }, &pbr);
                    pbr.SetColor(&rgba_u8(255, 246, 250, alpha * 0.75));
                    rt.FillEllipse(&D2D1_ELLIPSE { point: pt, radiusX: 1.05 * eff_u, radiusY: 1.05 * eff_u }, &pbr);
                }
            }
            rt.PopAxisAlignedClip();
        }
        TaskbarVizStyle::HoverCosmic => {
            // Interactive Element 5: Cosmic Gravity Well — nebula haze, constellation threads & orbital comet lensing
            let clip_guard = push_capsule_clip(rt, d2d, l, t, r, b, rad);
            let neb_stops = [
                D2D1_GRADIENT_STOP { position: 0.0, color: rgba_u8(84, 52, 200, 0.22 * op) },
                D2D1_GRADIENT_STOP { position: 0.55, color: rgba_u8(48, 146, 220, 0.16 * op) },
                D2D1_GRADIENT_STOP { position: 1.0, color: rgba_u8(168, 72, 210, 0.20 * op) },
            ];
            if let Some(nbr) = create_grad(rt, Vector2 { X: l, Y: t }, Vector2 { X: r, Y: b }, &neb_stops) {
                rt.FillRectangle(&D2D_RECT_F { left: l, top: t, right: r, bottom: b }, &nbr);
            }

            if let Ok(br) = rt.CreateSolidColorBrush(&rgba_u8(220, 232, 255, 1.0), None) {
                // Gravitational lens halo + glowing core under hovered cursor
                if viz.hover_mix > 0.02 {
                    let lens_r = 22.0 * eff_u;
                    br.SetColor(&rgba_u8(138, 120, 255, 0.24 * viz.hover_mix * op));
                    rt.FillEllipse(&D2D1_ELLIPSE { point: hx_px, radiusX: lens_r, radiusY: lens_r }, &br);
                    br.SetColor(&rgba_u8(130, 242, 255, 0.65 * viz.hover_mix * op));
                    rt.DrawEllipse(&D2D1_ELLIPSE { point: hx_px, radiusX: lens_r * 0.72, radiusY: lens_r * 0.72 }, &br, 1.2 * eff_u, None);
                    br.SetColor(&rgba_u8(255, 255, 255, 0.90 * viz.hover_mix * op));
                    rt.FillEllipse(&D2D1_ELLIPSE { point: hx_px, radiusX: 2.5 * eff_u, radiusY: 2.5 * eff_u }, &br);
                }

                for p in &viz.particles {
                    let pt = to_px(p.x, p.y);
                    if pt.X < l - 12.0 || pt.X > r + 12.0 || pt.Y < t - 12.0 || pt.Y > b + 12.0 {
                        continue;
                    }
                    let tw = (p.phase.sin() * 0.5 + 0.5) * 0.55 + 0.45;
                    let r_st = (1.1 + p.size * 1.45) * eff_u;

                    // Gravity filament tether when captured inside pointer's gravity lens
                    if viz.hover_mix > 0.04 {
                        let d = (pt.X - hx_px.X).hypot(pt.Y - hx_px.Y);
                        if d < 56.0 * eff_u {
                            let tether_a = (1.0 - d / (56.0 * eff_u)) * 0.52 * viz.hover_mix * op;
                            br.SetColor(&rgba_u8(146, 226, 255, tether_a));
                            rt.DrawLine(pt, hx_px, &br, 0.85 * eff_u, None);
                        }
                    }

                    // Orbital comet streak aligned with particle velocity
                    let sx = (p.vx * box_w * 0.045).clamp(-14.0, 14.0);
                    let sy = (p.vy * box_h * 0.045).clamp(-14.0, 14.0);
                    if sx.hypot(sy) > 1.2 {
                        br.SetColor(&rgba_u8(142, 232, 255, 0.60 * tw * op));
                        rt.DrawLine(pt, Vector2 { X: pt.X - sx, Y: pt.Y - sy }, &br, 1.3 * eff_u, None);
                    }

                    let col = if p.kind == 0 {
                        rgba_u8(140, 238, 255, 0.90 * tw * op)
                    } else if p.kind == 1 {
                        rgba_u8(204, 152, 255, 0.88 * tw * op)
                    } else {
                        rgba_u8(255, 252, 244, 0.96 * tw * op)
                    };
                    br.SetColor(&col);
                    rt.FillEllipse(&D2D1_ELLIPSE { point: pt, radiusX: r_st * 0.72, radiusY: r_st * 0.72 }, &br);
                    if p.size > 1.05 {
                        let sp = r_st * 1.85;
                        rt.DrawLine(Vector2 { X: pt.X - sp, Y: pt.Y }, Vector2 { X: pt.X + sp, Y: pt.Y }, &br, 0.75 * eff_u, None);
                        rt.DrawLine(Vector2 { X: pt.X, Y: pt.Y - sp }, Vector2 { X: pt.X, Y: pt.Y + sp }, &br, 0.75 * eff_u, None);
                    }
                }
            }
            pop_capsule_clip(rt, clip_guard);
        }
        TaskbarVizStyle::HoverPlasma => {
            // Interactive Element 6: Tesla Plasma — ionized silk arcs + jagged electric tendrils snapping to cursor
            let clip_guard = push_capsule_clip(rt, d2d, l, t, r, b, rad);
            let is_card = box_h > 46.0 * u && box_w < box_h * 4.6;

            // Subtle ionized violet-cyan plasma chamber aura
            let plasma_stops = [
                D2D1_GRADIENT_STOP { position: 0.0, color: rgba_u8(56, 226, 255, 0.14 * op) },
                D2D1_GRADIENT_STOP { position: 0.5, color: rgba_u8(120, 40, 210, 0.10 * op) },
                D2D1_GRADIENT_STOP { position: 1.0, color: rgba_u8(245, 76, 240, 0.15 * op) },
            ];
            if let Some(pbr) = create_grad(rt, Vector2 { X: l, Y: t }, Vector2 { X: r, Y: b }, &plasma_stops) {
                rt.FillRectangle(&D2D_RECT_F { left: l, top: t, right: r, bottom: b }, &pbr);
            }

            if let Ok(br) = rt.CreateSolidColorBrush(&rgba_u8(72, 242, 255, 1.0), None) {
                // Helper to draw a multi-segment jagged plasma arc with optional side fork
                let draw_plasma_bolt = |rt: &ID2D1RenderTarget,
                                        br: &windows::Win32::Graphics::Direct2D::ID2D1SolidColorBrush,
                                        p_start: Vector2,
                                        p_end: Vector2,
                                        seed: f32,
                                        alpha_b: f32,
                                        is_magenta: bool,
                                        draw_fork: bool| {
                    let dx = p_end.X - p_start.X;
                    let dy = p_end.Y - p_start.Y;
                    let dist = dx.hypot(dy).max(1.0);
                    let inv = 1.0 / dist;
                    let nx = -dy * inv;
                    let ny = dx * inv;
                    let amp = (dist * 0.13).clamp(3.0 * eff_u, 14.0 * eff_u);
                    let j1 = (viz.phase * 13.5 + seed * 2.7).sin() * amp;
                    let j2 = (viz.phase * 17.2 - seed * 4.1).cos() * amp * 0.85;
                    let j3 = (viz.phase * 15.0 + seed * 5.3).sin() * amp * 0.65;

                    let m1 = Vector2 {
                        X: p_start.X + dx * 0.25 + nx * j1,
                        Y: p_start.Y + dy * 0.25 + ny * j1,
                    };
                    let m2 = Vector2 {
                        X: p_start.X + dx * 0.52 - nx * j2,
                        Y: p_start.Y + dy * 0.52 - ny * j2,
                    };
                    let m3 = Vector2 {
                        X: p_start.X + dx * 0.78 + nx * j3,
                        Y: p_start.Y + dy * 0.78 + ny * j3,
                    };

                    let glow_col = if is_magenta {
                        rgba_u8(244, 82, 255, 0.38 * alpha_b)
                    } else {
                        rgba_u8(56, 236, 255, 0.38 * alpha_b)
                    };
                    let core_col = if is_magenta {
                        rgba_u8(255, 215, 255, 0.94 * alpha_b)
                    } else {
                        rgba_u8(215, 253, 255, 0.96 * alpha_b)
                    };

                    br.SetColor(&glow_col);
                    for seg in [(p_start, m1), (m1, m2), (m2, m3), (m3, p_end)] {
                        rt.DrawLine(seg.0, seg.1, br, 3.6 * eff_u, None);
                    }
                    br.SetColor(&core_col);
                    for seg in [(p_start, m1), (m1, m2), (m2, m3), (m3, p_end)] {
                        rt.DrawLine(seg.0, seg.1, br, 1.25 * eff_u, None);
                    }

                    if draw_fork {
                        let fk_ang = (viz.phase * 8.0 + seed).sin() * 0.55 + 0.45;
                        let fk_end = Vector2 {
                            X: m2.X + dx * 0.22 + nx * amp * 1.35 * fk_ang.signum(),
                            Y: m2.Y + dy * 0.22 + ny * amp * 1.35 * fk_ang.signum(),
                        };
                        br.SetColor(&glow_col);
                        rt.DrawLine(m2, fk_end, br, 2.2 * eff_u, None);
                        br.SetColor(&core_col);
                        rt.DrawLine(m2, fk_end, br, 0.85 * eff_u, None);
                    }
                };

                if is_card {
                    // 2D Widget Card Tesla Plasma Chamber:
                    // 1) Twin living plasma filaments hugging the top & bottom rims so center text stays crystal-clear
                    let n_pts = 28usize;
                    for strand in 0..2 {
                        let s_f = strand as f32;
                        let base_y_frac = if strand == 0 { 0.12 } else { 0.88 };
                        let mut prev_pt: Option<Vector2> = None;
                        for i in 0..n_pts {
                            let p = i as f32 / (n_pts - 1) as f32;
                            let px_x = l + pad + p * (box_w - 2.0 * pad).max(8.0);
                            let osc = (viz.phase * (2.4 + s_f * 0.7) + p * 10.0 + s_f * 2.1).sin();
                            let mut pt = Vector2 {
                                X: px_x,
                                Y: (t + box_h * (base_y_frac + osc * 0.055)).clamp(t + 3.0 * eff_u, b - 3.0 * eff_u),
                            };
                            if viz.hover_mix > 0.02 {
                                let d = (pt.X - hx_px.X).hypot(pt.Y - hx_px.Y);
                                let reach = (box_w.min(box_h) * 0.65).max(72.0 * eff_u);
                                if d < reach {
                                    let pull = (1.0 - d / reach).powi(2) * 0.48 * viz.hover_mix;
                                    pt.X += (hx_px.X - pt.X) * pull;
                                    pt.Y += (hx_px.Y - pt.Y) * pull;
                                }
                            }
                            if let Some(prv) = prev_pt {
                                br.SetColor(&if strand == 0 {
                                    rgba_u8(62, 236, 255, 0.24 * op)
                                } else {
                                    rgba_u8(244, 86, 255, 0.22 * op)
                                });
                                rt.DrawLine(prv, pt, &br, 3.4 * eff_u, None);
                                br.SetColor(&if strand == 0 {
                                    rgba_u8(195, 252, 255, 0.82 * op)
                                } else {
                                    rgba_u8(255, 198, 255, 0.78 * op)
                                });
                                rt.DrawLine(prv, pt, &br, 1.15 * eff_u, None);
                            }
                            prev_pt = Some(pt);
                        }
                    }

                    // 2) Draw glowing plasma nodes & idle/hover electric arcs across the 2D card
                    let max_bolt_dist = (box_w.hypot(box_h) * 0.78).max(110.0 * eff_u);
                    let mut bolts_drawn = 0usize;

                    // Idle wandering plasma core when not hovered, or cursor electrode when hovered
                    let idle_core = Vector2 {
                        X: l + box_w * (0.5 + (viz.phase * 0.85).cos() * 0.22),
                        Y: t + box_h * (0.5 + (viz.phase * 1.15).sin() * 0.20),
                    };
                    let target_hub = if viz.hover_mix > 0.04 { hx_px } else { idle_core };
                    let hub_strength = if viz.hover_mix > 0.04 {
                        viz.hover_mix
                    } else {
                        0.42
                    };

                    for (i, p) in viz.particles.iter().enumerate() {
                        let raw_pt = to_px(p.x, p.y);
                        let pt = if cb_w > box_w * 1.35 || cb_h > box_h * 1.35 {
                            Vector2 {
                                X: l + p.x * box_w,
                                Y: t + p.y * box_h,
                            }
                        } else {
                            raw_pt
                        };
                        if pt.X < l - 8.0 || pt.X > r + 8.0 || pt.Y < t - 8.0 || pt.Y > b + 8.0 {
                            continue;
                        }
                        let is_mag = i % 2 == 1;
                        let node_r = (if i >= 24 { 2.1 } else { 1.5 }) * eff_u;
                        br.SetColor(&if is_mag {
                            rgba_u8(245, 130, 255, 0.68 * op)
                        } else {
                            rgba_u8(110, 245, 255, 0.74 * op)
                        });
                        rt.FillEllipse(&D2D1_ELLIPSE { point: pt, radiusX: node_r, radiusY: node_r }, &br);

                        // Select active electrodes to fire branching plasma tendrils into the hub
                        let slot_active = if viz.hover_mix > 0.04 {
                            i % 4 == ((viz.phase * 3.0) as usize % 4) || i >= 28
                        } else {
                            i % 7 == ((viz.phase * 1.5) as usize % 7)
                        };
                        if slot_active && bolts_drawn < 7 {
                            let d = (pt.X - target_hub.X).hypot(pt.Y - target_hub.Y);
                            if d > 10.0 * eff_u && d < max_bolt_dist {
                                bolts_drawn += 1;
                                let dist_fade = (1.0 - (d / max_bolt_dist) * 0.65).clamp(0.25, 1.0);
                                let alpha_b = dist_fade * hub_strength * op;
                                draw_plasma_bolt(
                                    rt,
                                    &br,
                                    pt,
                                    target_hub,
                                    i as f32,
                                    alpha_b,
                                    is_mag,
                                    bolts_drawn <= 4,
                                );
                            }
                        }
                    }

                    // Plasma core / cursor electrode orb
                    let orb_a = hub_strength * op;
                    br.SetColor(&rgba_u8(84, 240, 255, 0.28 * orb_a));
                    rt.FillEllipse(&D2D1_ELLIPSE { point: target_hub, radiusX: 18.0 * eff_u, radiusY: 18.0 * eff_u }, &br);
                    br.SetColor(&rgba_u8(248, 128, 255, 0.52 * orb_a));
                    rt.FillEllipse(&D2D1_ELLIPSE { point: target_hub, radiusX: 8.0 * eff_u, radiusY: 8.0 * eff_u }, &br);
                    br.SetColor(&rgba_u8(255, 255, 255, 0.95 * orb_a));
                    rt.FillEllipse(&D2D1_ELLIPSE { point: target_hub, radiusX: 3.0 * eff_u, radiusY: 3.0 * eff_u }, &br);
                } else {
                    // Continuous ionized wave filaments along taskbar axis
                    let n_pts = 36usize;
                    for strand in 0..2 {
                        let s_f = strand as f32;
                        let mut prev_pt: Option<Vector2> = None;
                        for i in 0..n_pts {
                            let p = i as f32 / (n_pts - 1) as f32;
                            let s_pos = pad + p * avail;
                            let (amp, _) = sample_band(p);
                            let osc = (viz.phase * (2.2 + s_f * 0.8) + p * 14.0 + s_f * 2.4).sin();
                            let c_val = cross_len * (0.5 + osc * (0.24 + 0.16 * amp));
                            let mut pt = map_pt(s_pos, c_val.clamp(2.5 * eff_u, cross_len - 2.5 * eff_u));
                            if viz.hover_mix > 0.02 {
                                let d = (pt.X - hx_px.X).hypot(pt.Y - hx_px.Y);
                                if d < 72.0 * eff_u {
                                    let pull = (1.0 - d / (72.0 * eff_u)).powi(2) * 0.72 * viz.hover_mix;
                                    pt.X += (hx_px.X - pt.X) * pull;
                                    pt.Y += (hx_px.Y - pt.Y) * pull;
                                }
                            }
                            if let Some(prv) = prev_pt {
                                let glow_c = if strand == 0 {
                                    rgba_u8(62, 236, 255, 0.28 * op)
                                } else {
                                    rgba_u8(244, 86, 255, 0.26 * op)
                                };
                                br.SetColor(&glow_c);
                                rt.DrawLine(prv, pt, &br, 4.2 * eff_u, None);
                                let core_c = if strand == 0 {
                                    rgba_u8(190, 252, 255, 0.92 * op)
                                } else {
                                    rgba_u8(255, 198, 255, 0.88 * op)
                                };
                                br.SetColor(&core_c);
                                rt.DrawLine(prv, pt, &br, 1.35 * eff_u, None);
                            }
                            prev_pt = Some(pt);
                        }
                    }

                    // Charged plasma anchor nodes + snapping lightning bolts toward hovered cursor
                    let mut bolts_drawn = 0usize;
                    for (i, p) in viz.particles.iter().enumerate() {
                        if i % 3 != 0 {
                            continue;
                        }
                        let pt = to_px(p.x, p.y);
                        if pt.X < l - 8.0 || pt.X > r + 8.0 || pt.Y < t - 8.0 || pt.Y > b + 8.0 {
                            continue;
                        }
                        br.SetColor(&rgba_u8(120, 245, 255, 0.72 * op));
                        rt.FillEllipse(&D2D1_ELLIPSE { point: pt, radiusX: 1.8 * eff_u, radiusY: 1.8 * eff_u }, &br);

                        if viz.hover_mix > 0.04 && bolts_drawn < 5 {
                            let d = (pt.X - hx_px.X).hypot(pt.Y - hx_px.Y);
                            if d < 96.0 * eff_u {
                                bolts_drawn += 1;
                                let alpha_b = (1.0 - d / (96.0 * eff_u)) * viz.hover_mix * op;
                                draw_plasma_bolt(rt, &br, pt, hx_px, i as f32, alpha_b, i % 2 == 1, true);
                            }
                        }
                    }

                    // Plasma electrode contact glow orb beneath mouse cursor
                    if viz.hover_mix > 0.02 {
                        br.SetColor(&rgba_u8(84, 240, 255, 0.30 * viz.hover_mix * op));
                        rt.FillEllipse(&D2D1_ELLIPSE { point: hx_px, radiusX: 16.0 * eff_u, radiusY: 16.0 * eff_u }, &br);
                        br.SetColor(&rgba_u8(255, 164, 255, 0.56 * viz.hover_mix * op));
                        rt.FillEllipse(&D2D1_ELLIPSE { point: hx_px, radiusX: 7.5 * eff_u, radiusY: 7.5 * eff_u }, &br);
                        br.SetColor(&rgba_u8(255, 255, 255, 0.96 * viz.hover_mix * op));
                        rt.FillEllipse(&D2D1_ELLIPSE { point: hx_px, radiusX: 2.8 * eff_u, radiusY: 2.8 * eff_u }, &br);
                    }
                }
            }
            pop_capsule_clip(rt, clip_guard);
        }
        TaskbarVizStyle::HoverMacWave => {
            // Kinetic Interactive 1: Cupertino Kinetic Wave — parabolic spring shelf & dock indicator beads
            let clip_guard = push_capsule_clip(rt, d2d, l, t, r, b, rad);
            let mut wave_pts = [Vector2 { X: 0.0, Y: 0.0 }; 28];
            for i in 0..28usize {
                let p = viz.particles[i];
                let s_pos = p.x * span_len;
                let h_val = (0.26 + p.y * 0.62).clamp(0.08, 0.94);
                wave_pts[i] = map_pt(s_pos, h_val * cross_len);
            }
            if let Some(factory) = d2d {
                let mut poly = Vec::with_capacity(30);
                poly.extend_from_slice(&wave_pts);
                poly.push(map_pt(span_len, 0.0));
                poly.push(map_pt(0.0, 0.0));
                if let Ok(geo) = factory.CreatePathGeometry() {
                    if let Ok(sink) = geo.Open() {
                        sink.BeginFigure(poly[0], D2D1_FIGURE_BEGIN_FILLED);
                        sink.AddLines(&poly[1..]);
                        sink.EndFigure(D2D1_FIGURE_END_CLOSED);
                        if sink.Close().is_ok() {
                            let w_stops = [
                                D2D1_GRADIENT_STOP { position: 0.0, color: rgba_u8(215, 236, 255, 0.28 * op) },
                                D2D1_GRADIENT_STOP { position: 0.5, color: rgba_u8(168, 206, 252, 0.16 * op) },
                                D2D1_GRADIENT_STOP { position: 1.0, color: rgba_u8(102, 152, 224, 0.06 * op) },
                            ];
                            if let Some(wbr) = create_grad(rt, map_pt(span_len * 0.5, cross_len), map_pt(span_len * 0.5, 0.0), &w_stops) {
                                rt.FillGeometry(&geo, &wbr, None);
                            }
                        }
                    }
                }
            }
            if let Ok(br) = rt.CreateSolidColorBrush(&rgba_u8(240, 248, 255, 1.0), None) {
                for i in 0..27usize {
                    br.SetColor(&rgba_u8(152, 212, 255, 0.34 * op));
                    rt.DrawLine(wave_pts[i], wave_pts[i + 1], &br, 3.8 * eff_u, None);
                    br.SetColor(&rgba_u8(248, 252, 255, 0.88 * op));
                    rt.DrawLine(wave_pts[i], wave_pts[i + 1], &br, 1.35 * eff_u, None);
                }
                if viz.hover_mix > 0.02 {
                    for (rad_m, a, r_c, g_c, b_c) in [
                        (30.0, 0.07, 206u8, 234u8, 255u8),
                        (19.0, 0.11, 222, 242, 255),
                        (10.0, 0.16, 244, 250, 255),
                    ] {
                        br.SetColor(&rgba_u8(r_c, g_c, b_c, a * viz.hover_mix * op));
                        rt.FillEllipse(&D2D1_ELLIPSE { point: hx_px, radiusX: rad_m * eff_u, radiusY: rad_m * eff_u }, &br);
                    }
                }
                // Luminous macOS-style dock indicator beads pulsing cleanly along the outer lip
                for p in &viz.particles[28..] {
                    let s_pos = pad + p.x * avail;
                    let lift = 2.6 * eff_u + p.life * 2.4 * eff_u;
                    let pt = map_pt(s_pos, lift);
                    let gl = p.life.clamp(0.15, 1.0);
                    br.SetColor(&rgba_u8(186, 228, 255, 0.32 * gl * op));
                    rt.FillEllipse(&D2D1_ELLIPSE { point: pt, radiusX: (3.0 + gl * 2.2) * eff_u, radiusY: (3.0 + gl * 2.2) * eff_u }, &br);
                    br.SetColor(&rgba_u8(255, 255, 255, (0.45 + gl * 0.52) * op));
                    rt.FillEllipse(&D2D1_ELLIPSE { point: pt, radiusX: (1.2 + gl * 0.85) * eff_u, radiusY: (1.2 + gl * 0.85) * eff_u }, &br);
                }
            }
            pop_capsule_clip(rt, clip_guard);
        }
        TaskbarVizStyle::HoverMercury => {
            // Kinetic Interactive 2: Liquid Quicksilver — multi-stop molten metal ribbon + chrome droplets
            let clip_guard = push_capsule_clip(rt, d2d, l, t, r, b, rad);
            let mut rib_pts = [Vector2 { X: 0.0, Y: 0.0 }; 20];
            for i in 0..20usize {
                let p = viz.particles[i];
                rib_pts[i] = map_pt(p.x * span_len, (0.5 + p.y) * cross_len);
            }
            if let Ok(br) = rt.CreateSolidColorBrush(&rgba_u8(230, 238, 248, 1.0), None) {
                // Dark obsidian mercury trough shadow, metallic silver body, sharp specular white filament
                for i in 0..19usize {
                    let p0 = rib_pts[i];
                    let p1 = rib_pts[i + 1];
                    br.SetColor(&rgba_u8(12, 16, 22, 0.65 * op));
                    rt.DrawLine(
                        Vector2 { X: p0.X, Y: p0.Y + 1.5 * eff_u },
                        Vector2 { X: p1.X, Y: p1.Y + 1.5 * eff_u },
                        &br,
                        5.4 * eff_u,
                        None,
                    );
                    br.SetColor(&rgba_u8(184, 202, 222, 0.86 * op));
                    rt.DrawLine(p0, p1, &br, 3.2 * eff_u, None);
                    br.SetColor(&rgba_u8(250, 253, 255, 0.96 * op));
                    rt.DrawLine(
                        Vector2 { X: p0.X - 0.5 * eff_u, Y: p0.Y - 0.7 * eff_u },
                        Vector2 { X: p1.X - 0.5 * eff_u, Y: p1.Y - 0.7 * eff_u },
                        &br,
                        1.15 * eff_u,
                        None,
                    );
                }
                if viz.hover_mix > 0.02 {
                    for (rad_m, a, r_c, g_c, b_c) in [
                        (24.0, 0.06, 204u8, 224u8, 248u8),
                        (14.0, 0.10, 224, 238, 254),
                        (7.0, 0.14, 246, 251, 255),
                    ] {
                        br.SetColor(&rgba_u8(r_c, g_c, b_c, a * viz.hover_mix * op));
                        rt.FillEllipse(&D2D1_ELLIPSE { point: hx_px, radiusX: rad_m * eff_u, radiusY: rad_m * eff_u }, &br);
                    }
                }
                for p in &viz.particles[20..] {
                    let pt = to_px(p.x, p.y);
                    let dr = (2.1 + p.size * 2.2) * eff_u;
                    br.SetColor(&rgba_u8(10, 12, 18, 0.62 * op));
                    rt.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: pt.X + 0.8 * eff_u, Y: pt.Y + 1.2 * eff_u }, radiusX: dr * 1.12, radiusY: dr * 1.12 }, &br);
                    br.SetColor(&rgba_u8(192, 208, 228, 0.92 * op));
                    rt.FillEllipse(&D2D1_ELLIPSE { point: pt, radiusX: dr, radiusY: dr }, &br);
                    br.SetColor(&rgba_u8(252, 254, 255, 0.96 * op));
                    rt.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: pt.X - dr * 0.30, Y: pt.Y - dr * 0.30 }, radiusX: dr * 0.35, radiusY: dr * 0.35 }, &br);
                }
            }
            pop_capsule_clip(rt, clip_guard);
        }
        TaskbarVizStyle::HoverHydroBubbles => {
            // Kinetic Interactive 3: Hydro Fluid Chamber — aqua wash + sloshing meniscus + glossy 3D air bubbles
            let clip_guard = push_capsule_clip(rt, d2d, l, t, r, b, rad);
            let h_stops = [
                D2D1_GRADIENT_STOP { position: 0.0, color: rgba_u8(28, 212, 240, 0.22 * op) },
                D2D1_GRADIENT_STOP { position: 0.55, color: rgba_u8(14, 152, 204, 0.28 * op) },
                D2D1_GRADIENT_STOP { position: 1.0, color: rgba_u8(8, 86, 138, 0.36 * op) },
            ];
            if let Some(hbr) = create_grad(rt, Vector2 { X: l, Y: t }, Vector2 { X: l, Y: b }, &h_stops) {
                rt.FillRectangle(&D2D_RECT_F { left: l, top: t, right: r, bottom: b }, &hbr);
            }
            if let Ok(br) = rt.CreateSolidColorBrush(&rgba_u8(198, 252, 255, 1.0), None) {
                for layer in 0..2 {
                    let lf = layer as f32;
                    let mut prev_pt: Option<Vector2> = None;
                    br.SetColor(&rgba_u8(164, 248, 255, (0.48 - lf * 0.16) * op));
                    for i in 0..32usize {
                        let p = i as f32 / 31.0;
                        let s_pos = p * span_len;
                        let slosh = (viz.phase * (1.8 + lf * 0.6) + p * 8.0 + lf * 1.4).sin() * (cross_len * 0.09);
                        let c_pos = cross_len * (0.68 - lf * 0.18) + slosh;
                        let mut pt = map_pt(s_pos, c_pos.clamp(2.0 * eff_u, cross_len - 2.0 * eff_u));
                        if viz.hover_mix > 0.02 {
                            let d = (pt.X - hx_px.X).hypot(pt.Y - hx_px.Y);
                            if d < 54.0 * eff_u {
                                pt.Y += (1.0 - d / (54.0 * eff_u)).powi(2) * 5.0 * eff_u * viz.hover_mix;
                            }
                        }
                        if let Some(prv) = prev_pt {
                            rt.DrawLine(prv, pt, &br, (1.3 - lf * 0.3) * eff_u, None);
                        }
                        prev_pt = Some(pt);
                    }
                }
                for rip in &viz.ripples {
                    if !rip.active { continue; }
                    let c = to_px(rip.x, rip.y);
                    let k = (rip.age / rip.max_age).clamp(0.0, 1.0);
                    let rr_s = (2.5 + k * 20.0 * rip.strength) * eff_u;
                    br.SetColor(&rgba_u8(216, 253, 255, (1.0 - k) * 0.72 * op));
                    rt.DrawEllipse(&D2D1_ELLIPSE { point: c, radiusX: rr_s, radiusY: rr_s }, &br, 1.15 * eff_u, None);
                }
                for p in &viz.particles {
                    let pt = to_px(p.x, p.y);
                    if pt.X < l - 10.0 || pt.X > r + 10.0 || pt.Y < t - 10.0 || pt.Y > b + 10.0 { continue; }
                    let rb = (1.6 + p.size * 2.3) * eff_u;
                    br.SetColor(&rgba_u8(94, 230, 252, 0.22 * op));
                    rt.FillEllipse(&D2D1_ELLIPSE { point: pt, radiusX: rb, radiusY: rb }, &br);
                    br.SetColor(&rgba_u8(196, 252, 255, 0.84 * op));
                    rt.DrawEllipse(&D2D1_ELLIPSE { point: pt, radiusX: rb, radiusY: rb }, &br, 0.95 * eff_u, None);
                    br.SetColor(&rgba_u8(255, 255, 255, 0.92 * op));
                    rt.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: pt.X - rb * 0.34, Y: pt.Y - rb * 0.34 }, radiusX: rb * 0.24, radiusY: rb * 0.24 }, &br);
                }
            }
            pop_capsule_clip(rt, clip_guard);
        }
        TaskbarVizStyle::HoverQuantum => {
            // Kinetic Interactive 4: Quantum Collider — counter-orbiting beams colliding at cursor focus
            let clip_guard = push_capsule_clip(rt, d2d, l, t, r, b, rad);
            if let Ok(br) = rt.CreateSolidColorBrush(&rgba_u8(56, 226, 255, 1.0), None) {
                br.SetColor(&rgba_u8(116, 62, 228, 0.24 * op));
                rt.DrawEllipse(
                    &D2D1_ELLIPSE {
                        point: Vector2 { X: l + box_w * 0.5, Y: t + box_h * 0.5 },
                        radiusX: box_w * 0.45,
                        radiusY: box_h * 0.34,
                    },
                    &br,
                    1.1 * eff_u,
                    None,
                );
                if viz.hover_mix > 0.02 {
                    br.SetColor(&rgba_u8(255, 128, 232, 0.26 * viz.hover_mix * op));
                    rt.FillEllipse(&D2D1_ELLIPSE { point: hx_px, radiusX: 18.0 * eff_u, radiusY: 18.0 * eff_u }, &br);
                    br.SetColor(&rgba_u8(96, 240, 255, 0.72 * viz.hover_mix * op));
                    rt.DrawEllipse(&D2D1_ELLIPSE { point: hx_px, radiusX: 10.0 * eff_u, radiusY: 10.0 * eff_u }, &br, 1.15 * eff_u, None);
                }
                for (i, p) in viz.particles.iter().enumerate() {
                    let pt = to_px(p.x, p.y);
                    if pt.X < l - 12.0 || pt.X > r + 12.0 || pt.Y < t - 12.0 || pt.Y > b + 12.0 { continue; }
                    if i < 24 {
                        let col = if p.kind == 0 {
                            rgba_u8(56, 232, 255, 0.92 * op)
                        } else {
                            rgba_u8(255, 108, 226, 0.92 * op)
                        };
                        let glow = if p.kind == 0 {
                            rgba_u8(56, 232, 255, 0.28 * op)
                        } else {
                            rgba_u8(255, 108, 226, 0.28 * op)
                        };
                        let rp = (1.6 + p.size * 1.1) * eff_u;
                        br.SetColor(&glow);
                        rt.FillEllipse(&D2D1_ELLIPSE { point: pt, radiusX: rp * 2.4, radiusY: rp * 2.4 }, &br);
                        br.SetColor(&col);
                        rt.FillEllipse(&D2D1_ELLIPSE { point: pt, radiusX: rp, radiusY: rp }, &br);
                    } else {
                        let lk = p.life.clamp(0.0, 1.0);
                        let sx = (p.vx * box_w * 0.032).clamp(-12.0, 12.0);
                        let sy = (p.vy * box_h * 0.032).clamp(-12.0, 12.0);
                        br.SetColor(&rgba_u8(255, 238, 178, lk * 0.94 * op));
                        rt.DrawLine(pt, Vector2 { X: pt.X - sx, Y: pt.Y - sy }, &br, 1.3 * eff_u, None);
                        rt.FillEllipse(&D2D1_ELLIPSE { point: pt, radiusX: 1.5 * eff_u * lk, radiusY: 1.5 * eff_u * lk }, &br);
                    }
                }
            }
            pop_capsule_clip(rt, clip_guard);
        }
        TaskbarVizStyle::HoverBiolum => {
            // Kinetic Interactive 5: Abyssal Bioluminescence — undulating sea filaments + glowing plankton wake
            let clip_guard = push_capsule_clip(rt, d2d, l, t, r, b, rad);
            let ab_stops = [
                D2D1_GRADIENT_STOP { position: 0.0, color: rgba_u8(6, 16, 36, 0.34 * op) },
                D2D1_GRADIENT_STOP { position: 1.0, color: rgba_u8(10, 42, 68, 0.26 * op) },
            ];
            if let Some(abr) = create_grad(rt, Vector2 { X: l, Y: t }, Vector2 { X: r, Y: b }, &ab_stops) {
                rt.FillRectangle(&D2D_RECT_F { left: l, top: t, right: r, bottom: b }, &abr);
            }
            if let Ok(br) = rt.CreateSolidColorBrush(&rgba_u8(20, 241, 217, 1.0), None) {
                for strand in 0..3 {
                    let sf = strand as f32;
                    let mut prev_pt: Option<Vector2> = None;
                    for i in 0..30usize {
                        let p = i as f32 / 29.0;
                        let s_pos = pad + p * avail;
                        let osc = (viz.phase * (1.1 + sf * 0.25) + p * 9.5 + sf * 2.1).sin();
                        let c_val = cross_len * (0.28 + sf * 0.20 + osc * 0.16);
                        let pt = map_pt(s_pos, c_val.clamp(2.5 * eff_u, cross_len - 2.5 * eff_u));
                        if let Some(prv) = prev_pt {
                            let prox = if viz.hover_mix > 0.02 {
                                let d = (pt.X - hx_px.X).hypot(pt.Y - hx_px.Y);
                                (1.0 - (d / (72.0 * eff_u)).min(1.0)) * viz.hover_mix
                            } else {
                                0.0
                            };
                            let col = if strand == 1 {
                                rgba_u8(168, 128, 255, (0.24 + prox * 0.62) * op)
                            } else {
                                rgba_u8(20, 241, 217, (0.22 + prox * 0.66) * op)
                            };
                            br.SetColor(&col);
                            rt.DrawLine(prv, pt, &br, (1.2 + prox * 1.4) * eff_u, None);
                        }
                        prev_pt = Some(pt);
                    }
                }
                for p in &viz.particles {
                    let pt = to_px(p.x, p.y);
                    if pt.X < l - 12.0 || pt.X > r + 12.0 || pt.Y < t - 12.0 || pt.Y > b + 12.0 { continue; }
                    let gl = p.life.clamp(0.14, 1.0);
                    let rp = (1.3 + p.size * 1.6 + gl * 1.2) * eff_u;
                    let col = if p.kind == 0 {
                        rgba_u8(20, 241, 217, gl * op)
                    } else if p.kind == 1 {
                        rgba_u8(168, 128, 255, gl * op)
                    } else {
                        rgba_u8(186, 255, 248, gl * op)
                    };
                    br.SetColor(&D2D1_COLOR_F { a: (0.24 * gl * op).clamp(0.0, 1.0), ..col });
                    rt.FillEllipse(&D2D1_ELLIPSE { point: pt, radiusX: rp * 2.5, radiusY: rp * 2.5 }, &br);
                    br.SetColor(&col);
                    rt.FillEllipse(&D2D1_ELLIPSE { point: pt, radiusX: rp * 0.65, radiusY: rp * 0.65 }, &br);
                }
            }
            pop_capsule_clip(rt, clip_guard);
        }
        TaskbarVizStyle::HoverSynthGrid => {
            // Kinetic Interactive 6: Synthwave Grid Warp — 80s neon sunset + wireframe trampoline under cursor
            let clip_guard = push_capsule_clip(rt, d2d, l, t, r, b, rad);
            let s_stops = [
                D2D1_GRADIENT_STOP { position: 0.0, color: rgba_u8(32, 8, 52, 0.36 * op) },
                D2D1_GRADIENT_STOP { position: 0.55, color: rgba_u8(84, 18, 86, 0.28 * op) },
                D2D1_GRADIENT_STOP { position: 1.0, color: rgba_u8(16, 8, 34, 0.38 * op) },
            ];
            if let Some(sbr) = create_grad(rt, Vector2 { X: l, Y: t }, Vector2 { X: l, Y: b }, &s_stops) {
                rt.FillRectangle(&D2D_RECT_F { left: l, top: t, right: r, bottom: b }, &sbr);
            }
            let well_px = to_px(viz.particles[0].x, viz.particles[0].y);
            let depth = viz.particles[1].y.clamp(-0.2, 1.4);
            if let Ok(br) = rt.CreateSolidColorBrush(&rgba_u8(255, 64, 178, 1.0), None) {
                // Retro glowing sun orb tucked in backdrop
                br.SetColor(&rgba_u8(255, 118, 72, 0.28 * op));
                rt.FillEllipse(
                    &D2D1_ELLIPSE {
                        point: Vector2 { X: l + box_w * 0.5, Y: t + box_h * 0.42 },
                        radiusX: box_h * 0.48,
                        radiusY: box_h * 0.48,
                    },
                    &br,
                );
                let warp_pt = |mut pt: Vector2| -> Vector2 {
                    let dx = pt.X - well_px.X;
                    let dy = pt.Y - well_px.Y;
                    let d = dx.hypot(dy).max(1.0);
                    let r_well = (box_w.min(box_h) * 2.2).clamp(44.0, 96.0);
                    if d < r_well {
                        let f = (1.0 - d / r_well).powi(2) * depth;
                        if is_vertical {
                            pt.X += (if anchor_min { 1.0 } else { -1.0 }) * f * 12.0 * eff_u;
                        } else {
                            pt.Y += f * 11.0 * eff_u;
                        }
                    }
                    pt
                };
                // Perspective horizontal wireframe rows
                for row in 0..5usize {
                    let rf = row as f32 / 4.0;
                    let mut prev_pt: Option<Vector2> = None;
                    br.SetColor(&if row % 2 == 0 {
                        rgba_u8(36, 240, 255, (0.36 + rf * 0.42) * op)
                    } else {
                        rgba_u8(255, 52, 168, (0.38 + rf * 0.42) * op)
                    });
                    for col in 0..19usize {
                        let cf = col as f32 / 18.0;
                        let pt = warp_pt(map_pt(cf * span_len, (0.14 + rf * 0.72) * cross_len));
                        if let Some(prv) = prev_pt {
                            rt.DrawLine(prv, pt, &br, 1.05 * eff_u, None);
                        }
                        prev_pt = Some(pt);
                    }
                }
                // Perspective vertical wireframe spokes
                let scroll = (viz.phase * 0.35).fract();
                br.SetColor(&rgba_u8(255, 86, 194, 0.48 * op));
                for spoke in 0..12usize {
                    let sp_f = ((spoke as f32 + scroll) / 12.0).clamp(0.02, 0.98);
                    let fan = (sp_f - 0.5) * 1.45 + 0.5;
                    let mut prev_pt: Option<Vector2> = None;
                    for seg in 0..7usize {
                        let tf = seg as f32 / 6.0;
                        let sx = (sp_f + (fan - sp_f) * tf).clamp(0.0, 1.0) * span_len;
                        let cy_v = (0.14 + (1.0 - tf) * 0.72) * cross_len;
                        let pt = warp_pt(map_pt(sx, cy_v));
                        if let Some(prv) = prev_pt {
                            rt.DrawLine(prv, pt, &br, 0.95 * eff_u, None);
                        }
                        prev_pt = Some(pt);
                    }
                }
                if viz.hover_mix > 0.02 {
                    br.SetColor(&rgba_u8(36, 240, 255, 0.32 * viz.hover_mix * op));
                    rt.FillEllipse(&D2D1_ELLIPSE { point: well_px, radiusX: 12.0 * eff_u, radiusY: 12.0 * eff_u }, &br);
                }
            }
            pop_capsule_clip(rt, clip_guard);
        }
    }
    // Subtle pill-clipped cursor sheen for Live Ambient FX & Music Themes when Mouse Hover React is on
    if !style.is_interactive() && (style.is_ambient_live() || is_music_style) && viz.hover_mix > 0.02 {
        let clip_guard = push_capsule_clip(rt, d2d, l, t, r, b, rad);
        if let Ok(hbr) = rt.CreateSolidColorBrush(&pal.light_col(0.24 * viz.hover_mix * op), None) {
            rt.FillEllipse(&D2D1_ELLIPSE { point: hx_px, radiusX: 28.0 * eff_u, radiusY: 28.0 * eff_u }, &hbr);
            hbr.SetColor(&pal.text_col(0.32 * viz.hover_mix * op));
            rt.FillEllipse(&D2D1_ELLIPSE { point: hx_px, radiusX: 10.0 * eff_u, radiusY: 10.0 * eff_u }, &hbr);
        }
        pop_capsule_clip(rt, clip_guard);
    }
}

/// Shared mini-taskbar renderer for Theme Gallery cards in Settings Studio.
#[allow(clippy::too_many_arguments)]
pub unsafe fn draw_taskbar_preview_card_with_viz(
    rt: &ID2D1DCRenderTarget,
    d2d: &ID2D1Factory,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    u: f32,
    theme: crate::config::TaskbarTheme,
    base_cfg: &AppConfig,
    raw_bands: &[f32; 12],
    anim_phase: f32,
    custom_viz: Option<&VizSnapshot>,
) {
    let mut cfg = base_cfg.clone();
    if theme != base_cfg.tb_theme {
        cfg.reset_dock_widget_overrides();
    }
    cfg.apply_taskbar_theme_preset(theme);
    // For the currently active theme card, reflect the user's live customizer choices;
    // for all other cards in the gallery, showcase each theme's distinct curated native palette.
    if theme == base_cfg.tb_theme {
        cfg.pill_mode = base_cfg.pill_mode;
        cfg.surface_mode = base_cfg.surface_mode;
        cfg.tint_alpha = base_cfg.tint_alpha;
        cfg.amber_border = base_cfg.amber_border;
        cfg.corner_radius = base_cfg.corner_radius;
        cfg.tb_viz_style = base_cfg.tb_viz_style;
        cfg.tb_viz_opacity = base_cfg.tb_viz_opacity;
        cfg.tb_viz_mirror = base_cfg.tb_viz_mirror;
        cfg.tb_match_wallpaper = base_cfg.tb_match_wallpaper;
        cfg.tb_viz_color_mode = base_cfg.tb_viz_color_mode;
        cfg.tb_viz_accent_rgb = base_cfg.tb_viz_accent_rgb;
        cfg.tb_show_backdrop = base_cfg.tb_show_backdrop;
        cfg.tb_show_rim = base_cfg.tb_show_rim;
        cfg.tb_show_fx = base_cfg.tb_show_fx;
        cfg.clean_top_bar_enabled = base_cfg.clean_top_bar_enabled;
        cfg.clean_top_bar_edge = base_cfg.clean_top_bar_edge;
        cfg.clean_top_bar_style = base_cfg.clean_top_bar_style;
        cfg.clean_top_bar_desktop_only = base_cfg.clean_top_bar_desktop_only;
        cfg.mac_dock_icon_size = base_cfg.mac_dock_icon_size;
        cfg.mac_dock_mag_pct = base_cfg.mac_dock_mag_pct;
        cfg.mac_dock_edge = base_cfg.mac_dock_edge;
        cfg.mac_dock_dots = base_cfg.mac_dock_dots;
        cfg.w_apps.card_opacity_pct = base_cfg.w_apps.card_opacity_pct;
        cfg.tb_hover_react = base_cfg.tb_hover_react;
    }
    let _tint_guard = crate::dock_themes::bind_active_dock_backdrop_full(
        None,
        false,
        cfg.tint_alpha,
        cfg.w_apps.card_opacity_pct,
    );
    let pal = effective_taskbar_palette(&cfg);
    let vu_tricolor = is_vu_tricolor_active(&cfg);

    let bg_rr = D2D1_ROUNDED_RECT {
        rect: D2D_RECT_F {
            left: x * u,
            top: y * u,
            right: (x + w) * u,
            bottom: (y + h) * u,
        },
        radiusX: 9.0 * u,
        radiusY: 9.0 * u,
    };

    // Rich miniature animated-style wallpaper scene behind the taskbar
    let wp_stops = [
        D2D1_GRADIENT_STOP { position: 0.0, color: rgba_u8(pal.preview_a.0, pal.preview_a.1, pal.preview_a.2, 1.0) },
        D2D1_GRADIENT_STOP { position: 0.55, color: rgba_u8(pal.preview_b.0, pal.preview_b.1, pal.preview_b.2, 1.0) },
        D2D1_GRADIENT_STOP { position: 1.0, color: rgba_u8(pal.preview_c.0, pal.preview_c.1, pal.preview_c.2, 1.0) },
    ];
    if let Some(bg) = create_grad(rt, Vector2 { X: bg_rr.rect.left, Y: bg_rr.rect.top }, Vector2 { X: bg_rr.rect.right, Y: bg_rr.rect.bottom }, &wp_stops) {
        rt.FillRoundedRectangle(&bg_rr, &bg);
    }

    rt.PushAxisAlignedClip(&bg_rr.rect, windows::Win32::Graphics::Direct2D::D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
    // Decorative sunset / orb clouds behind taskbar so glass passthrough shows clearly
    if let Ok(sun) = rt.CreateSolidColorBrush(&pal.accent_col(0.30), None) {
        rt.FillEllipse(
            &windows::Win32::Graphics::Direct2D::D2D1_ELLIPSE {
                point: Vector2 { X: (x + w * 0.55) * u, Y: (y + h * 0.58) * u },
                radiusX: 52.0 * u,
                radiusY: 32.0 * u,
            },
            &sun,
        );
    }
    if let Ok(hill) = rt.CreateSolidColorBrush(&pal.deep_col(0.26), None) {
        rt.FillEllipse(
            &windows::Win32::Graphics::Direct2D::D2D1_ELLIPSE {
                point: Vector2 { X: (x + w * 0.22) * u, Y: (y + h * 0.76) * u },
                radiusX: 66.0 * u,
                radiusY: 26.0 * u,
            },
            &hill,
        );
    }

    if theme.uses_virtual_dock() {
        crate::dock_themes::draw_dock_preview_card(
            rt,
            Some(d2d),
            x,
            y,
            w,
            h,
            u,
            theme,
            &cfg,
            &pal,
            anim_phase,
        );
        rt.PopAxisAlignedClip();
        return;
    }

    let fallback_viz = VizSnapshot::demo_wave(raw_bands, anim_phase);
    let viz = custom_viz.unwrap_or(&fallback_viz);
    let is_mac_lift = matches!(
        theme,
        TaskbarTheme::QuartzShellDock
            | TaskbarTheme::CupertinoDock
            | TaskbarTheme::KineticCupertinoWave
            | TaskbarTheme::DynamicIslandDock
            | TaskbarTheme::Os26LiquidGlassLight
            | TaskbarTheme::Os26LiquidGlassDark
            | TaskbarTheme::OneUiLightChips
            | TaskbarTheme::OneUiDarkChips
            | TaskbarTheme::LiquidGlassClear
            | TaskbarTheme::LuminosityFloat
    );
    let is_os26_glass = matches!(
        theme,
        TaskbarTheme::Os26LiquidGlassLight | TaskbarTheme::Os26LiquidGlassDark
    );
    let bar_h = 26.0;
    let bar_t = y + h - bar_h - if is_mac_lift { 7.5 } else { 5.0 };
    let bar_b = bar_t + bar_h;
    let bar_l = x + 10.0;
    let bar_r = x + w - 10.0;

    let is_full_bar = matches!(cfg.pill_mode, PillMode::FullBar | PillMode::NativeDefault);
    let split_left_start = if w < 260.0 { bar_l + 6.0 } else { bar_l + 44.0 };
    let split_left_end = if w < 260.0 { bar_r - 58.0 } else { bar_r - 78.0 };
    let split_right_start = if w < 260.0 { bar_r - 50.0 } else { bar_r - 68.0 };
    let split_caps = [(split_left_start, split_left_end), (split_right_start, bar_r)];
    let uni_caps = [(bar_l + 4.0, bar_r - 4.0)];
    let full_caps = [(x, x + w)];
    let capsules: &[(f32, f32)] = match cfg.pill_mode {
        PillMode::SplitPills => &split_caps,
        PillMode::UnifiedPill => &uni_caps,
        PillMode::FullBar | PillMode::NativeDefault => &full_caps,
    };
    let cap_top = if cfg.pill_mode == PillMode::FullBar { bar_t - 1.0 } else { bar_t };
    let cap_bot = if cfg.pill_mode == PillMode::FullBar { y + h } else { bar_b };
    let cap_rad = if cfg.pill_mode == PillMode::FullBar || theme == TaskbarTheme::Retro95Classic {
        0.0
    } else if theme == TaskbarTheme::UnixPolybar {
        4.5 * u
    } else if theme == TaskbarTheme::CyberArasakaDeck {
        6.5 * u
    } else if matches!(
        theme,
        TaskbarTheme::Os26LiquidGlassLight
            | TaskbarTheme::Os26LiquidGlassDark
            | TaskbarTheme::OneUiLightChips
            | TaskbarTheme::OneUiDarkChips
            | TaskbarTheme::BubblesDark
            | TaskbarTheme::BubblesLight
            | TaskbarTheme::LiquidGlassClear
    ) {
        13.0 * u
    } else if matches!(theme, TaskbarTheme::DockLikeLight | TaskbarTheme::DockLikeDark) {
        6.5 * u
    } else if matches!(
        theme,
        TaskbarTheme::MatterTilesLight
            | TaskbarTheme::MatterTilesDark
            | TaskbarTheme::SquircleFrosted
            | TaskbarTheme::RosePineTiles
            | TaskbarTheme::CatppuccinTiles
    ) {
        8.5 * u
    } else if theme == TaskbarTheme::PillsDetached {
        10.5 * u
    } else if is_mac_lift {
        12.5 * u
    } else {
        11.0 * u
    };
    let preview_canvas = if is_full_bar {
        (x * u, cap_top * u, w * u, (cap_bot - cap_top) * u)
    } else {
        (bar_l * u, cap_top * u, (bar_r - bar_l) * u, (cap_bot - cap_top) * u)
    };

    for (cap_idx, &(cl, cr)) in capsules.iter().enumerate() {
        let cl_px = cl * u;
        let ct_px = cap_top * u;
        let cr_px = cr * u;
        let cb_px = cap_bot * u;
        if cfg.tb_show_backdrop {
            draw_capsule_backdrop(
                rt,
                Some(d2d),
                cl_px,
                ct_px,
                cr_px,
                cb_px,
                cap_rad,
                u,
                (x + w) * u,
                (y + h) * u,
                is_full_bar,
                false,
                cap_idx,
                true,
                true,
                &cfg,
                &pal,
            );
        }
        // Keep visualizer bars clear of right-side tray dots in single-bar preview cards
        let viz_right = if capsules.len() == 1 && cfg.tb_viz_style == TaskbarVizStyle::EqualizerBars {
            (cr - 52.0).max(cl + 40.0) * u
        } else {
            cr_px
        };
        if cfg.tb_show_fx && (cap_idx == 0 || cfg.tb_viz_style != TaskbarVizStyle::EqualizerBars) {
            draw_taskbar_viz_in_rect_themed(
                rt,
                Some(d2d),
                cl_px,
                ct_px,
                viz_right,
                cb_px,
                preview_canvas,
                cap_rad,
                u,
                false,
                false,
                cfg.tb_viz_style,
                cfg.tb_viz_opacity,
                cfg.tb_viz_mirror,
                vu_tricolor,
                &pal,
                viz,
            );
        }
        if cfg.tb_show_rim {
            draw_capsule_rim_and_accents(
                rt,
                Some(d2d),
                cl_px,
                ct_px,
                cr_px,
                cb_px,
                cap_rad,
                u,
                false,
                false,
                is_full_bar,
                cap_idx,
                true,
                true,
                &cfg,
                viz,
                &pal,
            );
        }
    }

    // Mini Taskbar Icons on top of capsule + visualizer
    let cy = (cap_top + cap_bot) * 0.5;
    let dock_cx = match cfg.pill_mode {
        PillMode::SplitPills => (split_left_start + split_left_end) * 0.5,
        _ => x + w * 0.42,
    };
    let icon_cols = [
        (76, 175, 255),  // Start blue
        (245, 185, 72),  // Explorer amber
        (68, 210, 196),  // Browser teal
        (180, 188, 200), // Terminal silver
        (74, 216, 128),  // Music green
    ];
    let icon_step = if w < 260.0 {
        17.5
    } else if theme.is_independent_tile_dock() {
        26.5
    } else if is_os26_glass {
        24.5
    } else if theme == TaskbarTheme::CleanSlateCards || theme == TaskbarTheme::BlobLight {
        27.0
    } else if theme == TaskbarTheme::AerisGlassLight {
        24.0
    } else {
        22.0
    };
    let featured_idx = match theme {
        TaskbarTheme::MatterTilesLight => Some(4),
        TaskbarTheme::BubblesDark | TaskbarTheme::BubblesLight => Some(4),
        TaskbarTheme::RosePineTiles | TaskbarTheme::CatppuccinTiles => Some(1),
        TaskbarTheme::BlobLight | TaskbarTheme::CleanSlateCards => Some(2),
        TaskbarTheme::AerisGlassLight => Some(1),
        TaskbarTheme::Os26LiquidGlassLight | TaskbarTheme::Os26LiquidGlassDark => Some(2),
        _ => None,
    };
    if theme.uses_icon_pads() {
        let mut pad_rects = Vec::with_capacity(icon_cols.len());
        for idx in 0..icon_cols.len() {
            let ix = dock_cx + (idx as f32 - 2.0) * icon_step;
            if is_os26_glass {
                // Authentic OS26 Liquid Glass subtle fish-eye swell peaking at center keycap!
                let dist = (idx as f32 - 2.0).abs();
                let scale = if dist < 0.1 {
                    1.20
                } else if dist < 1.1 {
                    1.08
                } else {
                    0.96
                };
                let pw = (if w < 260.0 { 8.6 } else { 10.6 }) * scale;
                let ph = (if w < 260.0 { 9.2 } else { 10.6 }) * scale;
                let lift_y = (scale - 0.96) * 11.5;
                pad_rects.push((
                    (ix - pw) * u,
                    (cy - ph - lift_y) * u,
                    (ix + pw) * u,
                    (cy + ph - lift_y * 0.35) * u,
                ));
            } else if theme == TaskbarTheme::AerisGlassLight {
                let pw = icon_step * 0.48;
                pad_rects.push((
                    (ix - pw) * u,
                    cap_top * u,
                    (ix + pw) * u,
                    cap_bot * u,
                ));
            } else if theme == TaskbarTheme::CleanSlateCards || theme == TaskbarTheme::BlobLight {
                let pw = if w < 260.0 { 8.0 } else { 11.5 };
                let ph = (cap_bot - cap_top) * 0.39;
                pad_rects.push((
                    (ix - pw) * u,
                    (cy - ph) * u,
                    (ix + pw) * u,
                    (cy + ph) * u,
                ));
            } else if theme.is_independent_tile_dock() {
                let pad_half = if w < 260.0 { 8.0 } else { 11.2 };
                pad_rects.push((
                    (ix - pad_half) * u,
                    (cy - pad_half) * u,
                    (ix + pad_half) * u,
                    (cy + pad_half) * u,
                ));
            } else {
                let pad_half = if w < 260.0 { 7.6 } else { 9.2 };
                pad_rects.push((
                    (ix - pad_half) * u,
                    (cy - pad_half) * u,
                    (ix + pad_half) * u,
                    (cy + pad_half) * u,
                ));
            }
        }
        draw_clean_icon_pads(rt, Some(d2d), theme, &pad_rects, u, false, true, featured_idx);
    }
    for (idx, &(ir, ig, ib)) in icon_cols.iter().enumerate() {
        let mut ix = dock_cx + (idx as f32 - 2.0) * icon_step;
        let mut icy = cy;
        let mut half = if theme.uses_icon_pads() {
            5.2
        } else {
            6.0
        };
        if is_os26_glass {
            let dist = (idx as f32 - 2.0).abs();
            let scale = if dist < 0.1 {
                1.20
            } else if dist < 1.1 {
                1.08
            } else {
                0.96
            };
            half = 5.4 * scale;
            icy = cy - (scale - 0.96) * 7.5;
        } else if matches!(theme, TaskbarTheme::MatterTilesDark | TaskbarTheme::MatterTilesLight) {
            icy = cy - 1.2;
            half = 5.4;
        } else if theme.uses_virtual_dock() {
            let sweep = (anim_phase * 0.9).sin() * 1.15;
            let dist = ((idx as f32 - 2.0) - sweep).abs();
            let sw = if dist < 1.8 {
                let c = (std::f32::consts::FRAC_PI_2 * (dist / 1.8)).cos();
                1.0 + 0.52 * c * c
            } else {
                1.0
            };
            half = 6.0 * sw;
            icy = cy - (sw - 1.0) * 7.2;
            ix += ((idx as f32 - 2.0) - sweep).clamp(-1.0, 1.0) * 1.5;
        }
        let icon_rr = D2D1_ROUNDED_RECT {
            rect: D2D_RECT_F {
                left: (ix - half) * u,
                top: (icy - half) * u,
                right: (ix + half) * u,
                bottom: (icy + half) * u,
            },
            radiusX: (half * 0.5) * u,
            radiusY: (half * 0.5) * u,
        };
        if !theme.uses_icon_pads() {
            let sh_alpha = if theme.is_light_surface() { 0.28 } else { 0.65 };
            if let Ok(sh) = rt.CreateSolidColorBrush(&rgba_u8(10, 8, 6, sh_alpha), None) {
                rt.FillRoundedRectangle(
                    &D2D1_ROUNDED_RECT {
                        rect: D2D_RECT_F {
                            left: icon_rr.rect.left - 1.0 * u,
                            top: icon_rr.rect.top - 1.0 * u,
                            right: icon_rr.rect.right + 1.0 * u,
                            bottom: icon_rr.rect.bottom + 1.0 * u,
                        },
                        radiusX: (half * 0.5 + 0.8) * u,
                        radiusY: (half * 0.5 + 0.8) * u,
                    },
                    &sh,
                );
            }
        }
        if let Ok(ibr) = rt.CreateSolidColorBrush(&rgba_u8(ir, ig, ib, 0.96), None) {
            rt.FillRoundedRectangle(&icon_rr, &ibr);
        }
        if theme.uses_virtual_dock() && (idx == 0 || idx == 2 || idx == 3) {
            if let Ok(dbr) = rt.CreateSolidColorBrush(&rgba_u8(245, 250, 255, 0.92), None) {
                rt.FillEllipse(
                    &D2D1_ELLIPSE {
                        point: Vector2 {
                            X: ix * u,
                            Y: (cap_bot - 2.6) * u,
                        },
                        radiusX: 1.4 * u,
                        radiusY: 1.4 * u,
                    },
                    &dbr,
                );
            }
        }
    }
    // Right mini tray dots
    let tray_x = if w < 260.0 { bar_r - 39.0 } else { bar_r - 44.0 };
    let tray_dot_col = if theme.is_light_surface() {
        rgba_u8(36, 44, 62, 0.82)
    } else {
        rgba_u8(255, 243, 220, 0.85)
    };
    let is_one_ui = matches!(
        theme,
        TaskbarTheme::OneUiLightChips | TaskbarTheme::OneUiDarkChips
    );
    let tray_dots = if is_one_ui { 1usize } else { 3usize };
    for k in 0..tray_dots {
        if let Ok(tb) = rt.CreateSolidColorBrush(&tray_dot_col, None) {
            let tx = if is_one_ui {
                bar_r - 58.0
            } else {
                tray_x + (k as f32) * 11.0
            };
            rt.FillRoundedRectangle(
                &D2D1_ROUNDED_RECT {
                    rect: D2D_RECT_F {
                        left: tx * u,
                        top: (cy - 2.5) * u,
                        right: (tx + 7.0) * u,
                        bottom: (cy + 2.5) * u,
                    },
                    radiusX: 1.5 * u,
                    radiusY: 1.5 * u,
                },
                &tb,
            );
        }
    }

    rt.PopAxisAlignedClip();
    if let Ok(rim) = rt.CreateSolidColorBrush(&rgba_u8(255, 243, 220, 0.14), None) {
        rt.DrawRoundedRectangle(&bg_rr, &rim, 0.9 * u, None);
    }
}

/// Read-only check verifying whether `ALL APPLICATION PACKAGES` (`S-1-15-2-1`) has
fn strip_win32_verbatim_prefix(raw: &str) -> PathBuf {
    let trimmed = raw.trim_end_matches('\0');
    if let Some(unc) = trimmed.strip_prefix(r"\\?\UNC\") {
        PathBuf::from(format!(r"\\{}", unc))
    } else if let Some(plain) = trimmed.strip_prefix(r"\\?\") {
        PathBuf::from(plain)
    } else {
        PathBuf::from(trimmed)
    }
}

pub fn resolve_physical_path(path: &std::path::Path) -> PathBuf {
    let wide = to_wide(&path.to_string_lossy());
    unsafe {
        let Ok(h) = CreateFileW(
            PCWSTR(wide.as_ptr()),
            FILE_GENERIC_READ.0,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            None,
            OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL | FILE_FLAG_BACKUP_SEMANTICS,
            None,
        ) else {
            return path.to_path_buf();
        };
        let mut buf = [0u16; 1024];
        let n = GetFinalPathNameByHandleW(h, &mut buf, FILE_NAME_NORMALIZED) as usize;
        let _ = CloseHandle(h);
        if n == 0 || n >= buf.len() {
            return path.to_path_buf();
        }
        let s = String::from_utf16_lossy(&buf[..n]);
        strip_win32_verbatim_prefix(&s)
    }
}

pub fn is_packaged_windowsapps_path(path: &std::path::Path) -> bool {
    let lower = path.to_string_lossy().to_ascii_lowercase();
    lower.contains(r"\windowsapps\") || lower.contains("/windowsapps/")
}

static TAP_PORTABLE_FALLBACK_NOTE: OnceLock<Mutex<Option<String>>> = OnceLock::new();

fn set_tap_portable_fallback_note(note: Option<String>) {
    let lock = TAP_PORTABLE_FALLBACK_NOTE.get_or_init(|| Mutex::new(None));
    if let Ok(mut g) = lock.lock() {
        *g = note;
    }
}

/// Returns a human-readable diagnostic explanation when Portable Mode's `<exe_dir>\Data\TapCache`
/// could not accept Windows `ALL APPLICATION PACKAGES (S-1-15-2-1)` NTFS ACLs (e.g. FAT32/exFAT USB)
/// and gracefully fell back to a local NTFS cache folder for Explorer XAML injection.
pub fn tap_portable_fallback_reason() -> Option<String> {
    let lock = TAP_PORTABLE_FALLBACK_NOTE.get_or_init(|| Mutex::new(None));
    lock.lock().ok().and_then(|g| g.clone())
}

fn tap_non_portable_cache_dir(
    sibling_dll: &std::path::Path,
    userprofile: Option<&std::path::Path>,
    localappdata: Option<&std::path::Path>,
) -> PathBuf {
    if is_packaged_windowsapps_path(sibling_dll) {
        if let Some(profile) = userprofile.filter(|p| !p.as_os_str().is_empty()) {
            return profile.join(".autumndeck").join("TapCache");
        }
    }
    if let Some(local_app) = localappdata.filter(|p| !p.as_os_str().is_empty()) {
        return local_app.join("AutumnDeck").join("TapCache");
    }
    std::env::temp_dir().join("AutumnDeckTap")
}

pub(crate) fn resolve_tap_staging_cache_dir(
    sibling_dll: &std::path::Path,
    portable_data_dir: Option<&std::path::Path>,
    userprofile: Option<&std::path::Path>,
    localappdata: Option<&std::path::Path>,
) -> PathBuf {
    if let Some(data_dir) = portable_data_dir {
        return data_dir.join("TapCache");
    }
    tap_non_portable_cache_dir(sibling_dll, userprofile, localappdata)
}

fn tap_staging_cache_dir(sibling_dll: &std::path::Path) -> PathBuf {
    let portable_dir = if crate::config::is_portable_mode() {
        Some(crate::config::app_data_dir())
    } else {
        None
    };
    let profile = std::env::var("USERPROFILE")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .map(PathBuf::from);
    let local_app = std::env::var("LOCALAPPDATA")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .map(PathBuf::from);
    resolve_tap_staging_cache_dir(
        sibling_dll,
        portable_dir.as_deref(),
        profile.as_deref(),
        local_app.as_deref(),
    )
}

fn format_acl_summary(res: Result<bool, u32>) -> String {
    match res {
        Ok(true) => "OK (RX granted)".to_string(),
        Ok(false) => "OK (RX present)".to_string(),
        Err(code) => format!("ERROR (win32={})", code),
    }
}

fn ensure_tap_rx_acls(path: &std::path::Path) -> Result<bool, u32> {
    let a = sid_rx_acl_impl(path, "S-1-15-2-1", true)?;
    let b = sid_rx_acl_impl(path, "S-1-5-32-545", true)?;
    Ok(a || b)
}

fn stage_tap_dll_into_dir(
    cache_dir: &std::path::Path,
    src_bytes: &[u8],
) -> Result<(PathBuf, Result<bool, u32>), String> {
    std::fs::create_dir_all(cache_dir)
        .map_err(|e| format!("create cache dir {} failed: {}", cache_dir.display(), e))?;
    let real_dir = resolve_physical_path(cache_dir);
    let dir_acl = ensure_tap_rx_acls(&real_dir);

    let len = src_bytes.len();
    let mut chosen: Option<PathBuf> = None;
    for suffix in ["", "_v2", "_v3", "_v4"] {
        let name = format!("AutumnXamlTap_{}{}.dll", len, suffix);
        let cand = real_dir.join(&name);
        if cand.exists() {
            if let Ok(existing) = std::fs::read(&cand) {
                if existing == src_bytes {
                    chosen = Some(cand);
                    break;
                }
            }
            if std::fs::write(&cand, src_bytes).is_ok() {
                chosen = Some(cand);
                break;
            }
            continue;
        }
        if std::fs::write(&cand, src_bytes).is_ok() {
            chosen = Some(cand);
            break;
        }
    }
    let staged_raw =
        chosen.ok_or_else(|| "unable to write staged AutumnXamlTap DLL".to_string())?;
    let real_dll = resolve_physical_path(&staged_raw);

    // Best-effort cleanup of mismatched older cached copies not locked by explorer.exe
    if let Ok(entries) = std::fs::read_dir(&real_dir) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p != real_dll
                && p.extension()
                    .and_then(|e| e.to_str())
                    .map(|e| e.eq_ignore_ascii_case("dll"))
                    .unwrap_or(false)
            {
                let _ = std::fs::remove_file(p);
            }
        }
    }

    let dll_acl = ensure_tap_rx_acls(&real_dll);
    let combined_acl = match (dir_acl, dll_acl) {
        (Ok(a), Ok(b)) => Ok(a || b),
        (Err(e), _) | (_, Err(e)) => Err(e),
    };
    Ok((real_dll, combined_acl))
}

fn stage_tap_dll_to_writable_cache(
    sibling_dll: &std::path::Path,
) -> Result<(PathBuf, String), String> {
    let src_bytes =
        std::fs::read(sibling_dll).map_err(|e| format!("read sibling dll failed: {}", e))?;
    let primary_dir = tap_staging_cache_dir(sibling_dll);
    let portable = crate::config::is_portable_mode();

    match stage_tap_dll_into_dir(&primary_dir, &src_bytes) {
        Ok((real_dll, Ok(changed))) => {
            set_tap_portable_fallback_note(None);
            Ok((real_dll, format_acl_summary(Ok(changed))))
        }
        Ok((real_dll, Err(acl_err))) if portable => {
            // Non-NTFS portable volume (FAT32/exFAT USB): S-1-15-2-1 RX ACL cannot be applied on
            // the removable drive, so gracefully fall back to a local NTFS cache for Explorer XAML.
            let profile = std::env::var("USERPROFILE")
                .ok()
                .filter(|s| !s.trim().is_empty())
                .map(PathBuf::from);
            let local_app = std::env::var("LOCALAPPDATA")
                .ok()
                .filter(|s| !s.trim().is_empty())
                .map(PathBuf::from);
            let fb_dir =
                tap_non_portable_cache_dir(sibling_dll, profile.as_deref(), local_app.as_deref());
            set_tap_portable_fallback_note(Some(format!(
                "Portable drive lacks NTFS ACLs (win32={}); TapCache fell back to local NTFS cache for Explorer XAML.",
                acl_err
            )));
            if let Ok((fb_dll, fb_acl)) = stage_tap_dll_into_dir(&fb_dir, &src_bytes) {
                Ok((fb_dll, format!("{} [portable FAT32/exFAT fallback]", format_acl_summary(fb_acl))))
            } else {
                Ok((real_dll, format_acl_summary(Err(acl_err))))
            }
        }
        Ok((real_dll, acl_res)) => Ok((real_dll, format_acl_summary(acl_res))),
        Err(e) => Err(e),
    }
}

/// Returns `(path, acl_status)` for the currently staged writable TAP DLL if present.
pub fn tap_cache_dll_diagnostics(sibling_dll: &std::path::Path) -> Option<(String, String)> {
    let dir = resolve_physical_path(&tap_staging_cache_dir(sibling_dll));
    let entries = std::fs::read_dir(&dir).ok()?;
    for entry in entries.flatten() {
        let p = entry.path();
        if p.extension()
            .and_then(|e| e.to_str())
            .map(|e| e.eq_ignore_ascii_case("dll"))
            .unwrap_or(false)
        {
            let rp = resolve_physical_path(&p);
            let acl = format_acl_summary(sid_rx_acl_impl(&rp, "S-1-15-2-1", false));
            return Some((rp.to_string_lossy().into_owned(), acl));
        }
    }
    None
}

fn build_tap_injection_candidates(sibling_dll: &std::path::Path) -> Vec<(String, PathBuf, String)> {
    let mut out = Vec::new();
    let is_winapps = is_packaged_windowsapps_path(sibling_dll);
    if !is_winapps {
        let acl_res = ensure_tap_rx_acls(sibling_dll);
        let acl_desc = format_acl_summary(acl_res);
        if acl_res.is_ok() {
            out.push((
                "sibling".to_string(),
                resolve_physical_path(sibling_dll),
                acl_desc,
            ));
        }
        if let Ok((staged, staged_acl)) = stage_tap_dll_to_writable_cache(sibling_dll) {
            if out.iter().all(|(_, p, _)| p != &staged) {
                out.push(("staged_cache".to_string(), staged, staged_acl));
            }
        }
        if out.is_empty() {
            out.push((
                "sibling_fallback".to_string(),
                resolve_physical_path(sibling_dll),
                format_acl_summary(acl_res),
            ));
        }
    } else {
        if let Ok((staged, staged_acl)) = stage_tap_dll_to_writable_cache(sibling_dll) {
            out.push(("staged_cache_msix".to_string(), staged, staged_acl));
        }
        out.push((
            "sibling_windowsapps".to_string(),
            resolve_physical_path(sibling_dll),
            "WindowsApps_readonly".to_string(),
        ));
    }
    out
}

fn is_generic_diag_miss(hr: HRESULT) -> bool {
    let code = hr.0 as u32;
    code == 0x8007_0490 || code == 0x8007_0002 || code == 0x8000_4005
}

fn try_init_xaml_diagnostics(
    init_fn: InitializeXamlDiagnosticsExFn,
    exp_pid: u32,
    dll_w: &[u16],
) -> (bool, Option<HRESULT>) {
    let mut best_hr: Option<HRESULT> = None;
    let passes: &[(PCWSTR, u32)] = &[
        (w!("Windows.UI.Xaml.dll"), 16),
        (PCWSTR::null(), 8),
    ];
    for &(xaml_mod_name, max_idx) in passes {
        let mut any_connected = false;
        for idx in 1..=max_idx {
            let ep_w = to_wide(&format!("VisualDiagConnection{}", idx));
            let hr = unsafe {
                init_fn(
                    PCWSTR(ep_w.as_ptr()),
                    exp_pid,
                    xaml_mod_name,
                    PCWSTR(dll_w.as_ptr()),
                    CLSID_AUTUMN_XAML_TAP,
                    PCWSTR::null(),
                )
            };
            if hr.is_ok() {
                any_connected = true;
                best_hr = Some(hr);
                if TapBridge::find_existing_msg_wnd(exp_pid).is_some() {
                    return (true, Some(hr));
                }
            } else if best_hr.map(is_generic_diag_miss).unwrap_or(true)
                || !is_generic_diag_miss(hr)
            {
                best_hr = Some(hr);
            }
        }
        if any_connected {
            for _ in 0..32 {
                if TapBridge::find_existing_msg_wnd(exp_pid).is_some() {
                    return (true, best_hr);
                }
                std::thread::sleep(Duration::from_millis(25));
            }
        }
    }
    (false, best_hr)
}

/// Read & Execute (`RX`) access on `path`.
pub fn has_app_package_rx_acl(path: &std::path::Path) -> Result<bool, u32> {
    sid_rx_acl_impl(path, "S-1-15-2-1", false)
}

/// Ensures `ALL APPLICATION PACKAGES` (`S-1-15-2-1`) has Read & Execute (`RX`) permissions on
/// `path` so Windows 11's AppContainer XAML taskbar host can load `AutumnXamlTap.dll` even after
/// a plain `.zip` extraction or folder copy.
///
/// Returns:
/// - `Ok(false)` if the required ACE is already present (read-only check, no disk write).
/// - `Ok(true)` if the DACL was repaired cleanly.
/// - `Err(win32_error)` if reading or updating the file security descriptor failed.
pub fn ensure_app_package_rx_acl(path: &std::path::Path) -> Result<bool, u32> {
    sid_rx_acl_impl(path, "S-1-15-2-1", true)
}

fn sid_rx_acl_impl(
    path: &std::path::Path,
    sid_ascii: &str,
    repair_if_missing: bool,
) -> Result<bool, u32> {
    use std::ptr::null_mut;

    #[repr(C)]
    struct AceHeader {
        ace_type: u8,
        ace_flags: u8,
        ace_size: u16,
    }

    #[repr(C)]
    struct AccessAllowedAce {
        header: AceHeader,
        mask: u32,
        sid_start: u32,
    }

    #[repr(C)]
    #[derive(Default)]
    struct AclSizeInformation {
        ace_count: u32,
        acl_bytes_in_use: u32,
        acl_bytes_free: u32,
    }

    #[repr(C)]
    struct TrusteeW {
        p_multiple_trustee: *mut c_void,
        multiple_trustee_operation: u32,
        trustee_form: u32,
        trustee_type: u32,
        ptstr_name: *mut c_void,
    }

    #[repr(C)]
    struct ExplicitAccessW {
        grf_access_permissions: u32,
        grf_access_mode: u32,
        grf_inheritance: u32,
        trustee: TrusteeW,
    }

    #[link(name = "advapi32")]
    extern "system" {
        fn ConvertStringSidToSidW(string_sid: *const u16, sid: *mut *mut c_void) -> i32;
        fn GetNamedSecurityInfoW(
            object_name: *const u16,
            object_type: u32,
            security_info: u32,
            ppsid_owner: *mut *mut c_void,
            ppsid_group: *mut *mut c_void,
            pp_dacl: *mut *mut c_void,
            pp_sacl: *mut *mut c_void,
            pp_sd: *mut *mut c_void,
        ) -> u32;
        fn GetAclInformation(
            p_acl: *mut c_void,
            p_acl_information: *mut c_void,
            n_acl_information_length: u32,
            dw_acl_information_class: u32,
        ) -> i32;
        fn GetAce(p_acl: *mut c_void, dw_ace_index: u32, p_ace: *mut *mut c_void) -> i32;
        fn EqualSid(p_sid1: *mut c_void, p_sid2: *mut c_void) -> i32;
        fn SetEntriesInAclW(
            c_count_of_explicit_entries: u32,
            p_list_of_explicit_entries: *const ExplicitAccessW,
            old_acl: *mut c_void,
            new_acl: *mut *mut c_void,
        ) -> u32;
        fn SetNamedSecurityInfoW(
            object_name: *mut u16,
            object_type: u32,
            security_info: u32,
            psid_owner: *mut c_void,
            psid_group: *mut c_void,
            p_dacl: *mut c_void,
            p_sacl: *mut c_void,
        ) -> u32;
    }

    #[link(name = "kernel32")]
    extern "system" {
        fn LocalFree(hmem: *mut c_void) -> *mut c_void;
        fn GetLastError() -> u32;
    }

    struct LocalMemGuard(*mut c_void);
    impl Drop for LocalMemGuard {
        fn drop(&mut self) {
            if !self.0.is_null() {
                unsafe {
                    let _ = LocalFree(self.0);
                }
            }
        }
    }

    const SE_FILE_OBJECT: u32 = 1;
    const DACL_SECURITY_INFORMATION: u32 = 0x0000_0004;
    const ACL_SIZE_INFORMATION_CLASS: u32 = 2;
    const ACCESS_ALLOWED_ACE_TYPE: u8 = 0;
    const ACCESS_DENIED_ACE_TYPE: u8 = 1;
    const INHERIT_ONLY_ACE: u8 = 0x08;
    const GRANT_ACCESS: u32 = 1;
    const NO_INHERITANCE: u32 = 0;
    const NO_MULTIPLE_TRUSTEE: u32 = 0;
    const TRUSTEE_IS_SID: u32 = 0;
    const TRUSTEE_IS_WELL_KNOWN_GROUP: u32 = 5;
    // FILE_GENERIC_READ | FILE_GENERIC_EXECUTE = READ_CONTROL | SYNCHRONIZE |
    // FILE_READ_DATA | FILE_READ_EA | FILE_EXECUTE | FILE_READ_ATTRIBUTES
    const FILE_RX_MASK: u32 = 0x0012_00A9;
    const GENERIC_RX_MASK: u32 = 0xA000_0000;
    const GENERIC_ALL: u32 = 0x1000_0000;
    const ERROR_SUCCESS: u32 = 0;

    if !path.exists() {
        return Err(2); // ERROR_FILE_NOT_FOUND
    }

    unsafe {
        let sid_str = to_wide(sid_ascii);
        let mut app_pkg_sid: *mut c_void = null_mut();
        if ConvertStringSidToSidW(sid_str.as_ptr(), &mut app_pkg_sid) == 0 || app_pkg_sid.is_null()
        {
            return Err(GetLastError().max(1));
        }
        let _sid_guard = LocalMemGuard(app_pkg_sid);

        let mut path_w = to_wide(&path.to_string_lossy());
        let mut old_dacl: *mut c_void = null_mut();
        let mut sd: *mut c_void = null_mut();
        let rc = GetNamedSecurityInfoW(
            path_w.as_ptr(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION,
            null_mut(),
            null_mut(),
            &mut old_dacl,
            null_mut(),
            &mut sd,
        );
        if rc != ERROR_SUCCESS {
            return Err(rc);
        }
        let _sd_guard = LocalMemGuard(sd);

        // Fast read-only check: avoid rewriting DACL if S-1-15-2-1 already has RX.
        if !old_dacl.is_null() {
            let mut size_info = AclSizeInformation::default();
            if GetAclInformation(
                old_dacl,
                (&mut size_info as *mut AclSizeInformation) as *mut c_void,
                size_of::<AclSizeInformation>() as u32,
                ACL_SIZE_INFORMATION_CLASS,
            ) != 0
            {
                let mut denied = false;
                let mut granted = false;
                for idx in 0..size_info.ace_count {
                    let mut ace_ptr: *mut c_void = null_mut();
                    if GetAce(old_dacl, idx, &mut ace_ptr) == 0 || ace_ptr.is_null() {
                        continue;
                    }
                    let hdr = &*(ace_ptr as *const AceHeader);
                    if (hdr.ace_flags & INHERIT_ONLY_ACE) != 0 {
                        continue;
                    }
                    if hdr.ace_type == ACCESS_ALLOWED_ACE_TYPE
                        || hdr.ace_type == ACCESS_DENIED_ACE_TYPE
                    {
                        let ace = &*(ace_ptr as *const AccessAllowedAce);
                        let ace_sid = (&ace.sid_start as *const u32) as *mut c_void;
                        if EqualSid(ace_sid, app_pkg_sid) != 0 {
                            if hdr.ace_type == ACCESS_DENIED_ACE_TYPE {
                                denied = true;
                                break;
                            }
                            if (ace.mask & FILE_RX_MASK) == FILE_RX_MASK
                                || (ace.mask & GENERIC_RX_MASK) == GENERIC_RX_MASK
                                || (ace.mask & GENERIC_ALL) != 0
                            {
                                granted = true;
                                break;
                            }
                        }
                    }
                }
                if granted && !denied {
                    return Ok(!repair_if_missing);
                }
            }
        }
        if !repair_if_missing {
            return Ok(false);
        }

        let ea = ExplicitAccessW {
            grf_access_permissions: FILE_RX_MASK,
            grf_access_mode: GRANT_ACCESS,
            grf_inheritance: NO_INHERITANCE,
            trustee: TrusteeW {
                p_multiple_trustee: null_mut(),
                multiple_trustee_operation: NO_MULTIPLE_TRUSTEE,
                trustee_form: TRUSTEE_IS_SID,
                trustee_type: TRUSTEE_IS_WELL_KNOWN_GROUP,
                ptstr_name: app_pkg_sid,
            },
        };

        let mut new_dacl: *mut c_void = null_mut();
        let rc = SetEntriesInAclW(1, &ea, old_dacl, &mut new_dacl);
        if rc != ERROR_SUCCESS || new_dacl.is_null() {
            return Err(rc.max(1));
        }
        let _new_dacl_guard = LocalMemGuard(new_dacl);

        let rc = SetNamedSecurityInfoW(
            path_w.as_mut_ptr(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION,
            null_mut(),
            null_mut(),
            new_dacl,
            null_mut(),
        );
        if rc != ERROR_SUCCESS {
            return Err(rc);
        }
        Ok(true)
    }
}

/// Ensures `AutumnXamlTap.dll` sitting next to the current executable has the required
/// `ALL APPLICATION PACKAGES` (`S-1-15-2-1`) Read & Execute ACL.
pub fn ensure_sibling_tap_dll_acl() -> Result<bool, u32> {
    let mut exe_buf = [0u16; 1024];
    let n = unsafe { GetModuleFileNameW(None, &mut exe_buf) } as usize;
    if n == 0 {
        return Err(2);
    }
    let exe_path = PathBuf::from(String::from_utf16_lossy(&exe_buf[..n]));
    let Some(dir) = exe_path.parent() else {
        return Err(2);
    };
    let dll_path = dir.join("AutumnXamlTap.dll");
    ensure_app_package_rx_acl(&dll_path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ensure_app_package_rx_acl_heals_and_is_idempotent() {
        let tmp = std::env::temp_dir().join(format!(
            "autumn_acl_test_{}.tmp",
            std::process::id()
        ));
        let _ = std::fs::write(&tmp, b"acl_probe");

        // Fresh temp file initially lacks S-1-15-2-1:(RX)
        assert_eq!(has_app_package_rx_acl(&tmp), Ok(false));

        // First call heals missing S-1-15-2-1:(RX) on freshly created temp file
        let first = ensure_app_package_rx_acl(&tmp);
        assert!(first.is_ok(), "ACL heal failed: {:?}", first);
        assert_eq!(first.unwrap(), true, "Fresh temp file should be repaired on first pass");
        assert_eq!(has_app_package_rx_acl(&tmp), Ok(true));

        // Second call must detect existing RX ACE without rewriting DACL
        let second = ensure_app_package_rx_acl(&tmp);
        assert_eq!(second, Ok(false), "Second pass must be read-only idempotent");

        let _ = std::fs::remove_file(&tmp);
    }

    #[test]
    fn secondary_monitor_app_only_and_split_pill_layout() {
        // Negative-coordinate secondary monitor (left of primary) at 100% DPI
        let sec_bar = RECT { left: -1920, top: 1032, right: 0, bottom: 1080 };
        let inset = fallback_inset_rect(sec_bar, 1.0);
        assert!(inset.left > sec_bar.left && inset.right < sec_bar.right);
        assert!(inset.top > sec_bar.top && inset.bottom < sec_bar.bottom);

        // Secondary taskbar with apps only (no clock/tray icons)
        let app_pill = RECT { left: -1100, top: 1035, right: -820, bottom: 1077 };
        let pills = finalize_pills(Some(app_pill), None, PillMode::SplitPills, 12, false, sec_bar);
        assert_eq!(pills, vec![app_pill]);

        // Secondary taskbar with separate apps + tray groups
        let tray_pill = RECT { left: -180, top: 1035, right: -6, bottom: 1077 };
        let split = finalize_pills(Some(app_pill), Some(tray_pill), PillMode::SplitPills, 12, false, sec_bar);
        assert_eq!(split, vec![app_pill, tray_pill]);
        let unified = finalize_pills(Some(app_pill), Some(tray_pill), PillMode::UnifiedPill, 12, false, sec_bar);
        assert_eq!(unified.len(), 1);
        assert_eq!(unified[0].left, app_pill.left);
        assert_eq!(unified[0].right, tray_pill.right);
    }

    #[test]
    fn music_themes_have_distinct_curated_palettes_and_reactive_visualizer_physics() {
        let music_themes = [
            TaskbarTheme::LiveMusicBars,
            TaskbarTheme::NeonWave,
            TaskbarTheme::PureClearPulse,
            TaskbarTheme::MirrorBeatPill,
            TaskbarTheme::BassPulseSplit,
            TaskbarTheme::VuPeakStudio,
        ];
        let autumn = ThemePalette::autumn();
        let mut accents = std::collections::HashSet::new();
        for th in music_themes {
            let mut cfg = AppConfig::default();
            cfg.apply_taskbar_theme_preset(th);
            let pal = effective_taskbar_palette(&cfg);
            assert_ne!(
                pal.accent, autumn.accent,
                "Music theme {:?} must not fall back to monochrome Autumn yellow",
                th
            );
            assert!(
                accents.insert(pal.accent),
                "Music theme {:?} must have its own unique curated accent color",
                th
            );
            assert_eq!(is_vu_tricolor_active(&cfg), th == TaskbarTheme::VuPeakStudio);
        }

        // Custom color override replaces native accent & disables VU tri-color
        let mut vu_cfg = AppConfig::default();
        vu_cfg.apply_taskbar_theme_preset(TaskbarTheme::VuPeakStudio);
        assert!(is_vu_tricolor_active(&vu_cfg));
        vu_cfg.set_tb_viz_color_mode(VizColorMode::CustomAccent);
        vu_cfg.tb_viz_accent_rgb = 0xFF38A8;
        assert!(!is_vu_tricolor_active(&vu_cfg));
        let custom_pal = effective_taskbar_palette(&vu_cfg);
        assert!(custom_pal.accent.0 > 200 && custom_pal.accent.2 > 110);

        // Live FFT visualizer reactivity: bass does not smear into treble, peaks hold briefly,
        // and bands decay cleanly to exact 0.0 on silence.
        let mut viz = VizSnapshot::default();
        let bass_only = [0.95, 0.80, 0.25, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0];
        for _ in 0..8 {
            viz.update_from_raw(&bass_only, 0.022, false);
        }
        assert!(viz.bands[0] > 0.70, "Bass band must react strongly");
        assert!(
            viz.bands[20] < 0.05,
            "Treble band must stay isolated from pure bass signal: {}",
            viz.bands[20]
        );
        assert!(viz.peaks[0] >= viz.bands[0]);
        assert!(viz.beat_pulse > 0.45);
        assert!(viz.has_energy());

        // Silence decays cleanly all the way to 0.0
        for _ in 0..80 {
            viz.update_from_raw(&[0.0; 12], 0.022, false);
        }
        assert!(!viz.has_energy(), "Visualizer must return to exact idle on silence");
        assert_eq!(viz.beat_pulse, 0.0);
    }

    #[test]
    fn fallback_overlay_mode_preserves_icon_clarity_and_tracks_tap_state() {
        assert!(desired_overlay_is_above_taskbar(false));
        assert!(!desired_overlay_is_above_taskbar(true));

        let mut cfg = AppConfig::default();
        cfg.surface_mode = SurfaceMode::ClearTint;
        cfg.tint_alpha = 0;
        assert_eq!(fallback_overlay_alpha(&cfg, 0.0, false), 0.0);
        assert_eq!(fallback_overlay_alpha(&cfg, 0.65, true), 0.65);

        cfg.apply_taskbar_theme_preset(TaskbarTheme::HoverSnowfall);
        let snow_alpha = fallback_overlay_alpha(&cfg, cfg.tint_alpha as f32 / 255.0, false);
        assert!(
            snow_alpha > 0.0 && snow_alpha <= 0.11,
            "Fallback backdrop must stay ultralight so taskbar icons stay legible: {}",
            snow_alpha
        );

        let dark_heavy = fallback_overlay_alpha(&cfg, 0.85, false);
        assert!(
            dark_heavy <= 0.11,
            "Heavy tint must clamp down in above-taskbar fallback mode: {}",
            dark_heavy
        );

        let pal = effective_taskbar_palette(&cfg);
        let sig_fallback = StyleSig {
            pill_mode: cfg.pill_mode as u8,
            surface_mode: cfg.surface_mode as u8,
            tint_alpha: cfg.tint_alpha,
            tb_theme: cfg.tb_theme as u8,
            amber_border: cfg.amber_border,
            corner_radius: cfg.corner_radius,
            tb_viz_style: cfg.tb_viz_style as u8,
            tb_viz_opacity: cfg.tb_viz_opacity,
            tb_viz_mirror: cfg.tb_viz_mirror,
            tb_viz_color_mode: cfg.tb_viz_color_mode as u8,
            tb_viz_accent_rgb: cfg.tb_viz_accent_rgb,
            tb_show_backdrop: cfg.tb_show_backdrop,
            tb_show_rim: cfg.tb_show_rim,
            tb_show_fx: cfg.tb_show_fx,
            accent: pal.accent,
            accent_deep: pal.accent_deep,
            tap_connected: false,
            sys_light: false,
        };
        let mut sig_connected = sig_fallback;
        sig_connected.tap_connected = true;
        assert_ne!(
            sig_fallback, sig_connected,
            "TAP false -> true transition must invalidate StyleSig cache immediately"
        );
        let mut sig_light = sig_fallback;
        sig_light.sys_light = true;
        assert_ne!(
            sig_fallback, sig_light,
            "Windows Dark -> Light mode transition must invalidate StyleSig cache immediately"
        );
    }

    #[test]
    fn resolve_physical_path_and_windowsapps_detection() {
        assert_eq!(
            strip_win32_verbatim_prefix(r"\\?\C:\Users\Demo\AutumnXamlTap.dll"),
            PathBuf::from(r"C:\Users\Demo\AutumnXamlTap.dll")
        );
        assert_eq!(
            strip_win32_verbatim_prefix(r"\\?\UNC\server\share\AutumnXamlTap.dll"),
            PathBuf::from(r"\\server\share\AutumnXamlTap.dll")
        );
        assert!(is_packaged_windowsapps_path(std::path::Path::new(
            r"C:\Program Files\WindowsApps\Shreyas123g.AutumnDeck_2.0.2.0_x64__abc\AutumnXamlTap.dll"
        )));
        assert!(!is_packaged_windowsapps_path(std::path::Path::new(
            r"C:\Users\Demo\Downloads\AutumnDeck\AutumnXamlTap.dll"
        )));

        let tmp = std::env::temp_dir().join(format!(
            "autumn_phys_test_{}.tmp",
            std::process::id()
        ));
        let _ = std::fs::write(&tmp, b"phys_probe");
        let resolved = resolve_physical_path(&tmp);
        assert!(resolved.exists());
        assert!(!resolved.to_string_lossy().starts_with(r"\\?\"));
        let _ = std::fs::remove_file(&tmp);
    }

    #[test]
    fn ensure_tap_rx_acls_grants_app_package_and_users_sids() {
        let tmp = std::env::temp_dir().join(format!(
            "autumn_dual_acl_test_{}.tmp",
            std::process::id()
        ));
        let _ = std::fs::write(&tmp, b"dual_acl_probe");

        let res = ensure_tap_rx_acls(&tmp);
        assert!(res.is_ok(), "ensure_tap_rx_acls failed: {:?}", res);
        assert_eq!(has_app_package_rx_acl(&tmp), Ok(true));
        assert_eq!(sid_rx_acl_impl(&tmp, "S-1-5-32-545", false), Ok(true));
        assert_eq!(ensure_tap_rx_acls(&tmp), Ok(false));

        let _ = std::fs::remove_file(&tmp);
    }

    #[test]
    fn portable_tap_cache_resolves_into_data_dir_and_falls_back_cleanly() {
        let sibling = std::path::Path::new(r"E:\USB\AutumnDeck\AutumnXamlTap.dll");
        let portable_data = std::path::Path::new(r"E:\USB\AutumnDeck\Data");
        let profile = std::path::Path::new(r"C:\Users\Demo");
        let local = std::path::Path::new(r"C:\Users\Demo\AppData\Local");

        assert_eq!(
            resolve_tap_staging_cache_dir(sibling, Some(portable_data), Some(profile), Some(local)),
            PathBuf::from(r"E:\USB\AutumnDeck\Data\TapCache")
        );
        assert_eq!(
            resolve_tap_staging_cache_dir(sibling, None, Some(profile), Some(local)),
            PathBuf::from(r"C:\Users\Demo\AppData\Local\AutumnDeck\TapCache")
        );
        let msix_sibling = std::path::Path::new(
            r"C:\Program Files\WindowsApps\Shreyas123g.AutumnDeck_2.1.0.0_x64__abc\AutumnXamlTap.dll",
        );
        assert_eq!(
            resolve_tap_staging_cache_dir(msix_sibling, None, Some(profile), Some(local)),
            PathBuf::from(r"C:\Users\Demo\.autumndeck\TapCache")
        );
    }
}
