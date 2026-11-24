use std::collections::{HashMap, HashSet};
use std::ffi::c_void;
use std::mem::size_of;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

use windows::core::{w, Interface, PCSTR, PCWSTR, PWSTR};
use windows::Win32::Foundation::{
    CloseHandle, COLORREF, HANDLE, HINSTANCE, HWND, LPARAM, LRESULT, POINT, PROPERTYKEY, RECT,
    SIZE, SYSTEMTIME, WPARAM,
};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_COLOR_F, D2D1_FIGURE_BEGIN_HOLLOW,
    D2D1_FIGURE_END_OPEN, D2D1_PIXEL_FORMAT, D2D_RECT_F, D2D_SIZE_F, D2D_SIZE_U,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1CreateFactory, ID2D1Bitmap, ID2D1DCRenderTarget, ID2D1Factory, ID2D1SolidColorBrush,
    D2D1_ANTIALIAS_MODE_PER_PRIMITIVE, D2D1_ARC_SEGMENT, D2D1_ARC_SIZE_LARGE, D2D1_ARC_SIZE_SMALL,
    D2D1_BITMAP_INTERPOLATION_MODE_LINEAR, D2D1_BITMAP_PROPERTIES,
    D2D1_FACTORY_TYPE_SINGLE_THREADED, D2D1_RENDER_TARGET_PROPERTIES,
    D2D1_RENDER_TARGET_TYPE_DEFAULT, D2D1_RENDER_TARGET_USAGE_GDI_COMPATIBLE, D2D1_ROUNDED_RECT,
    D2D1_SWEEP_DIRECTION_CLOCKWISE,
};
use windows::Win32::Graphics::DirectWrite::{
    DWriteCreateFactory, IDWriteFactory, IDWriteTextFormat, DWRITE_FACTORY_TYPE_SHARED,
    DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_WEIGHT_SEMI_BOLD,
    DWRITE_PARAGRAPH_ALIGNMENT_CENTER, DWRITE_TEXT_ALIGNMENT_CENTER,
    DWRITE_TEXT_ALIGNMENT_LEADING, DWRITE_WORD_WRAPPING_NO_WRAP,
};
use windows::Win32::Graphics::Dwm::{
    DwmFlush, DwmGetWindowAttribute, DwmQueryThumbnailSourceSize, DwmRegisterThumbnail,
    DwmSetWindowAttribute, DwmUnregisterThumbnail, DwmUpdateThumbnailProperties,
    DWMWA_CLOAKED, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_DONOTROUND, DWMWCP_ROUND,
    DWM_THUMBNAIL_PROPERTIES, DWM_TNP_OPACITY, DWM_TNP_RECTDESTINATION,
    DWM_TNP_SOURCECLIENTAREAONLY, DWM_TNP_VISIBLE,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Gdi::{
    BeginPaint, BitBlt, ClientToScreen, CreateCompatibleDC, CreateDIBSection, CreateRectRgn,
    DeleteDC, DeleteObject, EndPaint, GetMonitorInfoW, InvalidateRect, MonitorFromPoint,
    SelectObject, SetWindowRgn, AC_SRC_ALPHA, AC_SRC_OVER, BITMAPINFO, BITMAPINFOHEADER, BI_RGB,
    BLENDFUNCTION, DIB_RGB_COLORS, HBITMAP, HDC, HGDIOBJ, HPALETTE, MONITORINFO,
    MONITOR_DEFAULTTONEAREST, PAINTSTRUCT, SRCCOPY,
};
use windows::Win32::Graphics::Imaging::{
    CLSID_WICImagingFactory, GUID_WICPixelFormat32bppPBGRA, IWICBitmapSource, IWICImagingFactory,
    WICBitmapDitherTypeNone, WICBitmapPaletteTypeCustom, WICBitmapUseAlpha,
};
use windows::Win32::Storage::FileSystem::{FILE_ATTRIBUTE_NORMAL, WIN32_FIND_DATAW};
use windows::Win32::System::Com::StructuredStorage::{PropVariantClear, PropVariantToStringAlloc};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, IPersistFile,
    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, STGM_READ,
};
use windows::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
use windows::Win32::System::SystemInformation::GetLocalTime;
use windows::Win32::System::Threading::{
    AttachThreadInput, GetCurrentThreadId, OpenProcess, QueryFullProcessImageNameW,
    PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::System::Variant::VARIANT;
use windows::Win32::UI::Accessibility::{
    CUIAutomation, IUIAutomation, IUIAutomationCacheRequest, IUIAutomationCondition,
    IUIAutomationInvokePattern, IUIAutomationTogglePattern, TreeScope_Element, TreeScope_Subtree,
    UIA_AutomationIdPropertyId, UIA_ButtonControlTypeId, UIA_ControlTypePropertyId,
    UIA_InvokePatternId, UIA_NamePropertyId, UIA_TogglePatternId,
};
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, SendInput, TrackMouseEvent, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT,
    KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP, TME_LEAVE, TRACKMOUSEEVENT, VIRTUAL_KEY, VK_CONTROL,
    VK_LWIN, VK_SHIFT,
};
use windows::Win32::UI::Shell::PropertiesSystem::{IPropertyStore, SHGetPropertyStoreForWindow};
use windows::Win32::UI::Shell::{
    BHID_SFUIObject, IContextMenu, IShellItem, IShellItemImageFactory, IShellLinkW,
    SHCreateItemFromParsingName, SHEmptyRecycleBinW, SHGetFileInfoW, SHQueryRecycleBinW,
    ShellExecuteExW, ShellExecuteW, ShellLink, CMINVOKECOMMANDINFO, CMINVOKECOMMANDINFOEX,
    CMF_EXPLORE, CMF_NORMAL, GCS_VERBW, SEE_MASK_FLAG_NO_UI, SEE_MASK_INVOKEIDLIST,
    SEE_MASK_UNICODE, SHELLEXECUTEINFOW, SHFILEINFOW, SHGFI_ICON, SHGFI_LARGEICON,
    SHGFI_USEFILEATTRIBUTES, SHQUERYRBINFO, SIGDN_NORMALDISPLAY, SIIGBF, SIIGBF_BIGGERSIZEOK,
    SIIGBF_ICONONLY, SLGP_RAWPATH,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AllowSetForegroundWindow, AppendMenuW, BringWindowToTop, CheckMenuItem, CreatePopupMenu,
    CreateWindowExW, DeregisterShellHookWindow, DestroyIcon, DestroyMenu, DestroyWindow,
    EnumChildWindows, EnumWindows, FindWindowExW, FindWindowW, GetClassLongPtrW, GetClassNameW,
    GetCursorPos, GetForegroundWindow, GetLastActivePopup, GetMenuItemCount, GetMenuItemInfoW,
    GetWindow, GetWindowLongW, GetWindowRect, GetWindowTextLengthW, GetWindowTextW,
    GetWindowThreadProcessId, IsIconic, IsWindow, IsWindowVisible, IsZoomed, LoadCursorW,
    PostMessageW, RegisterClassExW, RegisterShellHookWindow, RegisterWindowMessageW,
    SendMessageTimeoutW, SetCursor, SetForegroundWindow, SetWindowPos, ShowWindow, TrackPopupMenu,
    UpdateLayeredWindow, ASFW_ANY, GCLP_HICON, GCLP_HICONSM, GWL_EXSTYLE, GW_OWNER, HICON, HMENU,
    HWND_TOPMOST, ICON_BIG, ICON_SMALL2, IDC_ARROW, IDC_HAND, MENUITEMINFOW, MF_BYCOMMAND,
    MF_CHECKED, MF_GRAYED, MF_POPUP,
    MF_SEPARATOR, MF_STRING, MF_UNCHECKED, MIIM_ID, MIIM_STRING, SMTO_ABORTIFHUNG, SWP_NOACTIVATE,
    SWP_NOMOVE, SWP_NOSIZE, SWP_SHOWWINDOW, SW_HIDE, SW_MAXIMIZE, SW_MINIMIZE, SW_RESTORE,
    SW_SHOWNA, SW_SHOWNORMAL, SW_SHOWNOACTIVATE, TPM_BOTTOMALIGN, TPM_LEFTALIGN, TPM_RETURNCMD,
    TPM_RIGHTALIGN, TPM_RIGHTBUTTON, TPM_TOPALIGN, ULW_ALPHA, WM_CLOSE, WM_GETICON, WM_NULL,
    WNDCLASSEXW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};
use windows_core::BOOL;
use windows_numerics::Vector2;

use crate::config::{
    read_windows_taskbar_pins, save_config, to_wide, AppConfig, AppShortcut, DockClockEngine,
    DockClockWheelAction, DockDividerStyle, DockIndicatorStyle, DockRunningLabelMode,
    DockSlotCardStyle, HudTelemetryContent, HudWingMode, HudWingShape, MacDockEdge,
    MacDockHideMode, StartBadgeStyle, TaskbarTheme, TrayAudioMeterStyle,
    TrayBatteryMeterStyle, TrayNetMeterStyle,
};
use crate::desktop_widgets::{launch_app_path, load_app_bitmap_for_path, wic_src_to_d2d};
use crate::dock_themes::{
    chamfer_geo, dock_spec, draw_d2d_text, draw_dock_clock_module, draw_dock_divider_styled,
    draw_dock_indicator_styled, draw_dock_shelf_and_art_with_ripple, draw_dock_slot_fx_styled,
    draw_dock_tooltip, is_ambient_animated_cfg, resolve_shelf_spec, resolve_widget_spec,
    rgba as dt_rgba, unpack_rgb, DockArtLayer, DockLayoutKind, DockShelfShape, DockVisualSpec,
    TaskbarWidgetId,
};
use crate::system_media::CcSnapshot;
use crate::taskbar_hide::{
    has_maximized_or_fullscreen_on_monitor, is_fullscreen_game_on_monitor,
};
use crate::taskbar_styler::effective_taskbar_palette;
use crate::theme::ThemePalette;

pub const MAC_DOCK_WND_CLASS: PCWSTR = w!("AutumnMacDockWnd");
pub const MAC_DOCK_THUMB_CLASS: PCWSTR = w!("AutumnDockThumbWnd");

const IDM_DOCK_FOCUS: usize = 4101;
const IDM_DOCK_NEW_INST: usize = 4102;
const IDM_DOCK_MINIMIZE: usize = 4103;
const IDM_DOCK_MAX_RESTORE: usize = 4104;
const IDM_DOCK_CLOSE_WIN: usize = 4105;
const IDM_DOCK_PIN_TOGGLE: usize = 4106;
const IDM_DOCK_START_PIN: usize = 4107;
const IDM_DOCK_RUN_ADMIN: usize = 4108;
const IDM_DOCK_OPEN_LOC: usize = 4109;
const IDM_DOCK_PROPERTIES: usize = 4110;
const IDM_DOCK_MORE_SHELL: usize = 4111;
const IDM_DOCK_EMPTY_TRASH: usize = 4112;
const IDM_DOCK_SETTINGS: usize = 4113;
const IDM_DOCK_WIN_TB_SETTINGS: usize = 4114;
const IDM_DOCK_EXIT_DOCK: usize = 4115;
const IDM_DOCK_TASKMGR: usize = 4116;
const IDM_DOCK_SHOW_DESKTOP: usize = 4117;
const IDM_DOCK_EXPLORER: usize = 4118;
const IDM_DOCK_RESTORE_ALL: usize = 4119;
const IDM_DOCK_EDGE_BOTTOM: usize = 4120;
const IDM_DOCK_EDGE_TOP: usize = 4126;
const IDM_DOCK_EDGE_LEFT: usize = 4121;
const IDM_DOCK_EDGE_RIGHT: usize = 4122;
const IDM_DOCK_HIDE_SMART: usize = 4123;
const IDM_DOCK_HIDE_HOVER: usize = 4124;
const IDM_DOCK_HIDE_ALWAYS: usize = 4125;
const IDM_DOCK_HIDE_STEALTH: usize = 4134;
const IDM_DOCK_DATETIME: usize = 4127;
const IDM_DOCK_TRAY_VOL_MIXER: usize = 4128;
const IDM_DOCK_TRAY_SOUND: usize = 4129;
const IDM_DOCK_TRAY_NET: usize = 4130;
const IDM_DOCK_TRAY_POWER: usize = 4131;
const IDM_DOCK_TRAY_MUTE: usize = 4132;
const IDM_DOCK_SYNC_WIN_PINS: usize = 4133;

const IDM_WINX_APPS: usize = 4140;
const IDM_WINX_POWER: usize = 4141;
const IDM_WINX_EVENTVWR: usize = 4142;
const IDM_WINX_SYSTEM: usize = 4143;
const IDM_WINX_DEVMGMT: usize = 4144;
const IDM_WINX_NETWORK: usize = 4145;
const IDM_WINX_DISKMGMT: usize = 4146;
const IDM_WINX_COMPMGMT: usize = 4147;
const IDM_WINX_TERMINAL: usize = 4148;
const IDM_WINX_TERMINAL_ADMIN: usize = 4149;
const IDM_WINX_SETTINGS: usize = 4150;
const IDM_WINX_SEARCH: usize = 4151;
const IDM_WINX_RUN: usize = 4152;
const IDM_WINX_LOCK: usize = 4153;

const IDM_WIN_SWITCH_BASE: usize = 4200;

// Phase 6: Modular Per-Widget Context Menu Command IDs (4500..4799)
const IDM_W_RESET_ALL: usize = 4500;
const IDM_W_SHELF_LAYOUT_INHERIT: usize = 4501;
const IDM_W_SHELF_LAYOUT_BASE: usize = 4502;
const IDM_W_SHELF_SHAPE_INHERIT: usize = 4510;
const IDM_W_SHELF_SHAPE_BASE: usize = 4511;

const IDM_W_APPS_CARD_BASE: usize = 4520;
const IDM_W_APPS_IND_INHERIT: usize = 4530;
const IDM_W_APPS_IND_BASE: usize = 4531;
const IDM_W_APPS_DIV_BASE: usize = 4545;
const IDM_W_APPS_LBL_BASE: usize = 4555;
const IDM_W_APPS_BRAND_GLOW: usize = 4565;
const IDM_W_APPS_COUNT_BADGE: usize = 4566;
const IDM_W_APPS_RESET: usize = 4569;

const IDM_W_CLOCK_ENG_BASE: usize = 4570;
const IDM_W_CLOCK_SECONDS: usize = 4580;
const IDM_W_CLOCK_DATE: usize = 4581;
const IDM_W_CLOCK_24H_TOGGLE: usize = 4582;
const IDM_W_CLOCK_WHEEL_BASE: usize = 4585;
const IDM_W_CLOCK_RESET: usize = 4589;

const IDM_W_TRAY_AUD_BASE: usize = 4590;
const IDM_W_TRAY_BAT_BASE: usize = 4600;
const IDM_W_TRAY_NET_BASE: usize = 4610;
const IDM_W_TRAY_RESET: usize = 4619;

const IDM_W_HUD_MODE_BASE: usize = 4620;
const IDM_W_HUD_SHAPE_BASE: usize = 4625;
const IDM_W_HUD_CONTENT_BASE: usize = 4630;
const IDM_W_HUD_MARQUEE: usize = 4635;
const IDM_W_HUD_RESET: usize = 4639;

const IDM_W_UTIL_START_BASE: usize = 4640;
const IDM_W_UTIL_TOGGLE_START: usize = 4650;
const IDM_W_UTIL_TOGGLE_SEARCH: usize = 4651;
const IDM_W_UTIL_TOGGLE_TASKVIEW: usize = 4652;
const IDM_W_UTIL_TOGGLE_TRASH: usize = 4653;
const IDM_W_UTIL_TOGGLE_STUDIO: usize = 4654;
const IDM_W_UTIL_TOGGLE_SLIVER: usize = 4655;

const IDM_W_THEME_SHELF_BASE: usize = 5000;
const IDM_W_THEME_APPS_BASE: usize = 5100;
const IDM_W_THEME_CLOCK_BASE: usize = 5200;
const IDM_W_THEME_TRAY_BASE: usize = 5300;
const IDM_W_THEME_HUD_BASE: usize = 5400;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MacDockAction {
    None,
    OpenSettingsDock,
    ConfigUpdated,
    TraySetVolume(u8),
    TrayToggleMute,
}

#[derive(Clone, PartialEq)]
struct WinEntry {
    hwnd: HWND,
    title: String,
    exe_path: String,
    aumid: String,
    icon_hwnd: HWND,
    is_iconic: bool,
}

#[derive(Clone, PartialEq)]
enum DockSlotKind {
    StartLauncher,
    SearchButton,
    TaskViewButton,
    ActiveAppChip {
        title: String,
    },
    PinnedApp {
        name: String,
        path: String,
        windows: Vec<WinEntry>,
    },
    Divider,
    IslandGap,
    FlexSpacer,
    RunningApp {
        key: String,
        name: String,
        exe_path: String,
        windows: Vec<WinEntry>,
    },
    EndDivider,
    HiddenTrayChevron,
    TrayStatusCluster,
    ClockModule {
        time_str: String,
        date_str: String,
        hour24: u32,
        minute: u32,
        second: u32,
    },
    TrashBin {
        items: i64,
    },
    StudioLauncher,
    ShowDesktopSliver,
}

#[derive(Clone)]
struct DockLayoutSlot {
    slot_idx: usize,
    is_divider: bool,
    is_spacer: bool,
    center_x: f32,
    center_y: f32,
    icon_size: f32,
    slot_rect: D2D_RECT_F,
    hit_rect: D2D_RECT_F,
    label: String,
    inline_text: String,
    inline_alpha: f32,
    dot_count: usize,
    has_windows: bool,
    is_active: bool,
    is_flashing: bool,
}

struct DibSurface {
    w: i32,
    h: i32,
    mem_dc: HDC,
    hbmp: HBITMAP,
    old_obj: HGDIOBJ,
}

impl DibSurface {
    fn new() -> Self {
        Self {
            w: 0,
            h: 0,
            mem_dc: HDC::default(),
            hbmp: HBITMAP::default(),
            old_obj: HGDIOBJ::default(),
        }
    }

    fn ensure_size(&mut self, w: i32, h: i32) -> bool {
        if w <= 0 || h <= 0 {
            return false;
        }
        if self.w == w && self.h == h && !self.mem_dc.0.is_null() && !self.hbmp.0.is_null() {
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
            let Ok(hbmp) = CreateDIBSection(None, &bmi, DIB_RGB_COLORS, &mut bits, None, 0) else {
                let _ = DeleteDC(dc);
                return false;
            };
            if hbmp.0.is_null() {
                let _ = DeleteDC(dc);
                return false;
            }
            let old = SelectObject(dc, HGDIOBJ(hbmp.0));
            self.w = w;
            self.h = h;
            self.mem_dc = dc;
            self.hbmp = hbmp;
            self.old_obj = old;
        }
        true
    }

    fn release(&mut self) {
        unsafe {
            if !self.mem_dc.0.is_null() && !self.old_obj.0.is_null() {
                let _ = SelectObject(self.mem_dc, self.old_obj);
                self.old_obj = HGDIOBJ::default();
            }
            if !self.hbmp.0.is_null() {
                let _ = DeleteObject(HGDIOBJ(self.hbmp.0));
                self.hbmp = HBITMAP::default();
            }
            if !self.mem_dc.0.is_null() {
                let _ = DeleteDC(self.mem_dc);
                self.mem_dc = HDC::default();
            }
        }
        self.w = 0;
        self.h = 0;
    }
}

impl Drop for DibSurface {
    fn drop(&mut self) {
        self.release();
    }
}

pub fn register_mac_dock_class(
    hinst: HINSTANCE,
    wnd_proc: unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT,
) {
    unsafe {
        let mut wc = WNDCLASSEXW::default();
        wc.cbSize = size_of::<WNDCLASSEXW>() as u32;
        wc.lpfnWndProc = Some(wnd_proc);
        wc.hInstance = hinst;
        wc.hCursor = LoadCursorW(None, IDC_ARROW).unwrap_or_default();
        wc.lpszClassName = MAC_DOCK_WND_CLASS;
        let _ = RegisterClassExW(&wc);

        let mut twc = WNDCLASSEXW::default();
        twc.cbSize = size_of::<WNDCLASSEXW>() as u32;
        twc.lpfnWndProc = Some(wnd_proc);
        twc.hInstance = hinst;
        twc.hCursor = LoadCursorW(None, IDC_ARROW).unwrap_or_default();
        twc.lpszClassName = MAC_DOCK_THUMB_CLASS;
        let _ = RegisterClassExW(&twc);
    }
}

struct ThumbCard {
    win_hwnd: HWND,
    title: String,
    exe_path: String,
    aumid: String,
    icon_hwnd: HWND,
    is_iconic: bool,
    card_rect: D2D_RECT_F,
    thumb_rect: D2D_RECT_F,
    close_rect: D2D_RECT_F,
    thumb_id: isize,
}

struct DockThumbPopup {
    hinst: HINSTANCE,
    hwnd: HWND,
    visible: bool,
    slot_key: String,
    popup_rect: RECT,
    bridge_rect: RECT,
    cards: Vec<ThumbCard>,
    icon_cache: HashMap<isize, Option<ID2D1Bitmap>>,
    hovered_card: Option<usize>,
    hovered_close: Option<usize>,
    tracking_leave: bool,
    surface: DibSurface,
    dc_rt: Option<ID2D1DCRenderTarget>,
    d2d: Option<ID2D1Factory>,
    tf_title: Option<IDWriteTextFormat>,
    last_u: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ThumbHoverAction {
    KeepStable,
    SwitchOrOpen,
    HideNow,
    ExpireGrace,
}

/// Computes the narrow cursor-travel corridor between a hovered dock slot and its DWM thumbnail
/// popup. Crucially, this corridor NEVER swallows sibling app icons inside the dock bar so sliding
/// horizontally from App A to App B always triggers an immediate preview switch.
fn compute_thumb_bridge_rect(
    anchor_screen: RECT,
    popup_rect: RECT,
    edge: MacDockEdge,
) -> RECT {
    match edge {
        MacDockEdge::Bottom => {
            let top = popup_rect.bottom - 2;
            let bottom = (anchor_screen.top + 4).max(top);
            RECT {
                left: anchor_screen.left.min(popup_rect.left) - 4,
                top,
                right: anchor_screen.right.max(popup_rect.right) + 4,
                bottom,
            }
        }
        MacDockEdge::Top => {
            let top = anchor_screen.bottom - 4;
            let bottom = (popup_rect.top + 2).max(top);
            RECT {
                left: anchor_screen.left.min(popup_rect.left) - 4,
                top,
                right: anchor_screen.right.max(popup_rect.right) + 4,
                bottom,
            }
        }
        MacDockEdge::Left => {
            let left = anchor_screen.right - 4;
            let right = (popup_rect.left + 2).max(left);
            RECT {
                left,
                top: anchor_screen.top.min(popup_rect.top) - 4,
                right,
                bottom: anchor_screen.bottom.max(popup_rect.bottom) + 4,
            }
        }
        MacDockEdge::Right => {
            let left = popup_rect.right - 2;
            let right = (anchor_screen.left + 4).max(left);
            RECT {
                left,
                top: anchor_screen.top.min(popup_rect.top) - 4,
                right,
                bottom: anchor_screen.bottom.max(popup_rect.bottom) + 4,
            }
        }
    }
}

/// Resolves the live thumbnail hover state transition.
/// Checks `in_thumb` first, then `in_band` BEFORE `in_bridge`, so moving the pointer directly from
/// one running app icon to an adjacent app icon on the dock immediately switches DWM thumbnails
/// without locking onto the previous app's preview.
fn decide_thumb_hover_action(
    in_thumb: bool,
    in_band: bool,
    in_bridge: bool,
    has_preview_target: bool,
    has_hovered_slot: bool,
) -> ThumbHoverAction {
    if in_thumb {
        ThumbHoverAction::KeepStable
    } else if in_band {
        if has_preview_target {
            ThumbHoverAction::SwitchOrOpen
        } else if in_bridge && !has_hovered_slot {
            ThumbHoverAction::KeepStable
        } else {
            ThumbHoverAction::HideNow
        }
    } else if in_bridge {
        ThumbHoverAction::KeepStable
    } else {
        ThumbHoverAction::ExpireGrace
    }
}

impl DockThumbPopup {
    fn new(hinst: HINSTANCE) -> Self {
        Self {
            hinst,
            hwnd: HWND::default(),
            visible: false,
            slot_key: String::new(),
            popup_rect: RECT::default(),
            bridge_rect: RECT::default(),
            cards: Vec::new(),
            icon_cache: HashMap::new(),
            hovered_card: None,
            hovered_close: None,
            tracking_leave: false,
            surface: DibSurface::new(),
            dc_rt: None,
            d2d: None,
            tf_title: None,
            last_u: 1.0,
        }
    }

    fn ensure_window(&mut self) -> bool {
        if !self.hwnd.0.is_null() && unsafe { IsWindow(Some(self.hwnd)).as_bool() } {
            return true;
        }
        unsafe {
            let Ok(hw) = CreateWindowExW(
                WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOACTIVATE,
                MAC_DOCK_THUMB_CLASS,
                w!("AutumnDockThumbs"),
                WS_POPUP,
                0,
                0,
                200,
                140,
                None,
                None,
                Some(self.hinst),
                None,
            ) else {
                return false;
            };
            if hw.0.is_null() {
                return false;
            }
            self.hwnd = hw;
            self.visible = false;
            true
        }
    }

    fn ensure_graphics(&mut self, d2d_shared: Option<&ID2D1Factory>, dw_shared: Option<&IDWriteFactory>) -> bool {
        if self.d2d.is_none() {
            if let Some(f) = d2d_shared {
                self.d2d = Some(f.clone());
            } else {
                self.d2d = unsafe {
                    D2D1CreateFactory::<ID2D1Factory>(D2D1_FACTORY_TYPE_SINGLE_THREADED, None).ok()
                };
            }
        }
        if self.tf_title.is_none() {
            if let Some(dw) = dw_shared {
                unsafe {
                    if let Ok(tf) = dw.CreateTextFormat(
                        w!("Segoe UI Variable Text"),
                        None,
                        DWRITE_FONT_WEIGHT_SEMI_BOLD,
                        DWRITE_FONT_STYLE_NORMAL,
                        DWRITE_FONT_STRETCH_NORMAL,
                        11.5,
                        w!("en-US"),
                    ) {
                        let _ = tf.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_LEADING);
                        let _ = tf.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER);
                        let _ = tf.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP);
                        self.tf_title = Some(tf);
                    }
                }
            }
        }
        if self.dc_rt.is_none() {
            if let Some(ref d2d) = self.d2d {
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
                self.dc_rt = unsafe { d2d.CreateDCRenderTarget(&props).ok() };
            }
        }
        self.d2d.is_some() && self.dc_rt.is_some()
    }

    fn unregister_thumbs(&mut self) {
        unsafe {
            for c in &mut self.cards {
                if c.thumb_id != 0 {
                    let _ = DwmUnregisterThumbnail(c.thumb_id);
                    c.thumb_id = 0;
                }
            }
        }
    }

    fn hide(&mut self) {
        self.unregister_thumbs();
        self.cards.clear();
        self.slot_key.clear();
        self.hovered_card = None;
        self.hovered_close = None;
        self.tracking_leave = false;
        if self.visible && !self.hwnd.0.is_null() {
            unsafe {
                let _ = ShowWindow(self.hwnd, SW_HIDE);
            }
        }
        self.visible = false;
    }

    fn destroy(&mut self) {
        self.hide();
        if !self.hwnd.0.is_null() {
            unsafe {
                let _ = DestroyWindow(self.hwnd);
            }
            self.hwnd = HWND::default();
        }
        self.icon_cache.clear();
        if let Some(ref rt) = self.dc_rt {
            crate::realistic_pbr::evict_rt(rt);
        }
        self.dc_rt = None;
        self.tf_title = None;
        self.d2d = None;
        self.surface.release();
    }

    fn contains_point(&self, pt: POINT) -> bool {
        if !self.visible {
            return false;
        }
        pt.x >= self.popup_rect.left
            && pt.x <= self.popup_rect.right
            && pt.y >= self.popup_rect.top
            && pt.y <= self.popup_rect.bottom
    }

    fn in_bridge(&self, pt: POINT) -> bool {
        if !self.visible {
            return false;
        }
        pt.x >= self.bridge_rect.left
            && pt.x <= self.bridge_rect.right
            && pt.y >= self.bridge_rect.top
            && pt.y <= self.bridge_rect.bottom
    }

    #[allow(clippy::too_many_arguments)]
    fn show_for_slot(
        &mut self,
        slot_key: &str,
        windows: &[WinEntry],
        anchor_screen: RECT,
        shelf_screen: RECT,
        edge: MacDockEdge,
        mon_rect: RECT,
        theme: TaskbarTheme,
        u: f32,
    ) {
        if windows.is_empty() || !self.ensure_window() {
            self.hide();
            return;
        }
        let max_cards = windows.len().min(5);
        // Reuse existing live thumbnails if slot + windows + titles didn't change
        let same_set = self.visible
            && self.slot_key == slot_key
            && self.cards.len() == max_cards
            && (self.last_u - u).abs() < 0.01
            && self.cards.iter().zip(windows.iter().take(max_cards)).all(|(c, w)| {
                c.win_hwnd == w.hwnd && c.title == w.title && c.is_iconic == w.is_iconic
            });
        if same_set {
            // Keep bridge corridor synced smoothly in case dock slid
            self.bridge_rect = compute_thumb_bridge_rect(anchor_screen, self.popup_rect, edge);
            return;
        }

        self.unregister_thumbs();
        self.cards.clear();
        self.slot_key = slot_key.to_string();
        self.hovered_card = None;
        self.hovered_close = None;
        self.last_u = u;

        let is_vertical = matches!(edge, MacDockEdge::Left | MacDockEdge::Right);
        let mon_w = (mon_rect.right - mon_rect.left) as f32;
        let mon_h = (mon_rect.bottom - mon_rect.top) as f32;
        let pad = (8.0 * u).round();
        let gap = (8.0 * u).round();

        let (card_w, card_h, pw, ph) = if is_vertical {
            let max_h = (mon_h - 32.0 * u).max(150.0);
            let ch = ((max_h - pad * 2.0 - gap * (max_cards as f32 - 1.0)) / max_cards as f32)
                .clamp(92.0 * u, 134.0 * u)
                .round();
            let cw = (196.0 * u).round();
            (
                cw,
                ch,
                (cw + pad * 2.0).round() as i32,
                (ch * max_cards as f32 + gap * (max_cards as f32 - 1.0) + pad * 2.0).round() as i32,
            )
        } else {
            let max_w = (mon_w - 32.0 * u).max(200.0);
            let cw = ((max_w - pad * 2.0 - gap * (max_cards as f32 - 1.0)) / max_cards as f32)
                .clamp(136.0 * u, 202.0 * u)
                .round();
            let ch = (cw * 0.72).clamp(112.0 * u, 144.0 * u).round();
            (
                cw,
                ch,
                (cw * max_cards as f32 + gap * (max_cards as f32 - 1.0) + pad * 2.0).round() as i32,
                (ch + pad * 2.0).round() as i32,
            )
        };

        let anchor_cx = (anchor_screen.left + anchor_screen.right) / 2;
        let anchor_cy = (anchor_screen.top + anchor_screen.bottom) / 2;
        let margin = (8.0 * u).round() as i32;
        let (mut px, mut py) = match edge {
            MacDockEdge::Bottom => (anchor_cx - pw / 2, shelf_screen.top - ph - margin),
            MacDockEdge::Top => (anchor_cx - pw / 2, shelf_screen.bottom + margin),
            MacDockEdge::Left => (shelf_screen.right + margin, anchor_cy - ph / 2),
            MacDockEdge::Right => (shelf_screen.left - pw - margin, anchor_cy - ph / 2),
        };
        px = px.clamp(mon_rect.left + 6, (mon_rect.right - pw - 6).max(mon_rect.left + 6));
        py = py.clamp(mon_rect.top + 6, (mon_rect.bottom - ph - 6).max(mon_rect.top + 6));

        self.popup_rect = RECT {
            left: px,
            top: py,
            right: px + pw,
            bottom: py + ph,
        };
        self.bridge_rect = compute_thumb_bridge_rect(anchor_screen, self.popup_rect, edge);

        let spec = dock_spec(theme);
        unsafe {
            let corner_pref = match spec.shape {
                DockShelfShape::Chamfer | DockShelfShape::Tile => DWMWCP_DONOTROUND,
                _ => DWMWCP_ROUND,
            };
            let _ = DwmSetWindowAttribute(
                self.hwnd,
                DWMWA_WINDOW_CORNER_PREFERENCE,
                (&corner_pref as *const _) as *const _,
                size_of::<windows::Win32::Graphics::Dwm::DWM_WINDOW_CORNER_PREFERENCE>() as u32,
            );
            let _ = SetWindowPos(
                self.hwnd,
                Some(HWND_TOPMOST),
                px,
                py,
                pw,
                ph,
                SWP_NOACTIVATE | SWP_SHOWWINDOW,
            );
            let _ = ShowWindow(self.hwnd, SW_SHOWNOACTIVATE);
        }
        self.visible = true;

        let hdr_h = (26.0 * u).round();
        let card_pad = (6.0 * u).round();
        for (i, w) in windows.iter().take(max_cards).enumerate() {
            let (cx0, cy0) = if is_vertical {
                (pad, pad + i as f32 * (card_h + gap))
            } else {
                (pad + i as f32 * (card_w + gap), pad)
            };
            let card_rect = D2D_RECT_F {
                left: cx0,
                top: cy0,
                right: cx0 + card_w,
                bottom: cy0 + card_h,
            };
            let close_sz = (18.0 * u).round();
            let close_rect = D2D_RECT_F {
                left: card_rect.right - close_sz - 5.0 * u,
                top: card_rect.top + (hdr_h - close_sz) * 0.5,
                right: card_rect.right - 5.0 * u,
                bottom: card_rect.top + (hdr_h - close_sz) * 0.5 + close_sz,
            };
            let thumb_rect = D2D_RECT_F {
                left: card_rect.left + card_pad,
                top: card_rect.top + hdr_h + 2.0 * u,
                right: card_rect.right - card_pad,
                bottom: card_rect.bottom - card_pad,
            };

            let mut thumb_id = 0isize;
            unsafe {
                if let Ok(tid) = DwmRegisterThumbnail(self.hwnd, w.hwnd) {
                    let tw_box = (thumb_rect.right - thumb_rect.left).max(16.0);
                    let th_box = (thumb_rect.bottom - thumb_rect.top).max(16.0);
                    let (sw, sh) = match DwmQueryThumbnailSourceSize(tid) {
                        Ok(sz) if sz.cx > 0 && sz.cy > 0 => (sz.cx as f32, sz.cy as f32),
                        _ => (tw_box, th_box),
                    };
                    let scale = (tw_box / sw).min(th_box / sh);
                    let rw = (sw * scale).max(12.0);
                    let rh = (sh * scale).max(12.0);
                    let dx = thumb_rect.left + (tw_box - rw) * 0.5;
                    let dy = thumb_rect.top + (th_box - rh) * 0.5;
                    let props = DWM_THUMBNAIL_PROPERTIES {
                        dwFlags: DWM_TNP_RECTDESTINATION
                            | DWM_TNP_VISIBLE
                            | DWM_TNP_OPACITY
                            | DWM_TNP_SOURCECLIENTAREAONLY,
                        rcDestination: RECT {
                            left: dx.round() as i32,
                            top: dy.round() as i32,
                            right: (dx + rw).round() as i32,
                            bottom: (dy + rh).round() as i32,
                        },
                        rcSource: RECT::default(),
                        opacity: 255,
                        fVisible: BOOL(1),
                        fSourceClientAreaOnly: BOOL(0),
                    };
                    if DwmUpdateThumbnailProperties(tid, &props).is_ok() {
                        thumb_id = tid;
                    } else {
                        let _ = DwmUnregisterThumbnail(tid);
                    }
                }
            }

            self.cards.push(ThumbCard {
                win_hwnd: w.hwnd,
                title: w.title.clone(),
                exe_path: w.exe_path.clone(),
                aumid: w.aumid.clone(),
                icon_hwnd: w.icon_hwnd,
                is_iconic: w.is_iconic,
                card_rect,
                thumb_rect,
                close_rect,
                thumb_id,
            });
        }
        unsafe {
            let _ = InvalidateRect(Some(self.hwnd), None, false);
        }
    }

    fn hit_test_client(&self, lx: f32, ly: f32) -> (Option<usize>, Option<usize>) {
        for (i, c) in self.cards.iter().enumerate() {
            if lx >= c.close_rect.left
                && lx <= c.close_rect.right
                && ly >= c.close_rect.top
                && ly <= c.close_rect.bottom
            {
                return (Some(i), Some(i));
            }
            if lx >= c.card_rect.left
                && lx <= c.card_rect.right
                && ly >= c.card_rect.top
                && ly <= c.card_rect.bottom
            {
                return (Some(i), None);
            }
        }
        (None, None)
    }

    unsafe fn paint(
        &mut self,
        d2d_shared: Option<&ID2D1Factory>,
        dw_shared: Option<&IDWriteFactory>,
        wic_shared: Option<&IWICImagingFactory>,
        cfg: &AppConfig,
        pal: &ThemePalette,
    ) {
        if self.hwnd.0.is_null() {
            return;
        }
        let mut ps = PAINTSTRUCT::default();
        let hdc_win = BeginPaint(self.hwnd, &mut ps);
        if hdc_win.0.is_null() {
            return;
        }
        let cw = (self.popup_rect.right - self.popup_rect.left).max(16);
        let ch = (self.popup_rect.bottom - self.popup_rect.top).max(16);
        let u = self.last_u;
        let mut paint_ok = true;
        if self.ensure_graphics(d2d_shared, dw_shared) && self.surface.ensure_size(cw, ch) {
            if let Some(ref rt) = self.dc_rt {
                // Resolve per-card bitmaps on THIS popup's own dc_rt to prevent D2D cross-domain errors
                if self.icon_cache.len() > 48 {
                    self.icon_cache.clear();
                }
                let card_bmps: Vec<Option<ID2D1Bitmap>> = self
                    .cards
                    .iter()
                    .map(|c| {
                        let key = c.win_hwnd.0 as isize;
                        if let Some(cached) = self.icon_cache.get(&key) {
                            return cached.clone();
                        }
                        let b = if let Some(wic) = wic_shared {
                            let w_tmp = WinEntry {
                                hwnd: c.win_hwnd,
                                title: c.title.clone(),
                                exe_path: c.exe_path.clone(),
                                aumid: c.aumid.clone(),
                                icon_hwnd: c.icon_hwnd,
                                is_iconic: c.is_iconic,
                            };
                            load_window_bitmap(wic, rt, &w_tmp).or_else(|| {
                                if !c.aumid.is_empty() {
                                    load_app_bitmap_for_path(
                                        wic,
                                        rt,
                                        &format!("shell:AppsFolder\\{}", c.aumid),
                                        false,
                                    )
                                } else if !c.exe_path.is_empty() {
                                    load_app_bitmap_for_path(wic, rt, &c.exe_path, false)
                                } else {
                                    None
                                }
                            })
                        } else {
                            None
                        };
                        if b.is_some() {
                            self.icon_cache.insert(key, b.clone());
                        }
                        b
                    })
                    .collect();

                let rc = RECT {
                    left: 0,
                    top: 0,
                    right: cw,
                    bottom: ch,
                };
                if rt.BindDC(self.surface.mem_dc, &rc).is_ok() {
                    let spec = dock_spec(cfg.tb_theme);
                    rt.BeginDraw();
                    rt.SetAntialiasMode(D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
                    rt.Clear(Some(&dt_rgba(spec.bg_bot.0, spec.bg_bot.1, spec.bg_bot.2, 1.0)));

                    if let Ok(br) = rt.CreateSolidColorBrush(&rgba(255, 255, 255, 1.0), None) {
                        let full_r = D2D_RECT_F {
                            left: 0.5,
                            top: 0.5,
                            right: cw as f32 - 0.5,
                            bottom: ch as f32 - 0.5,
                        };
                        br.SetColor(&dt_rgba(spec.bg_mid.0, spec.bg_mid.1, spec.bg_mid.2, 0.98));
                        rt.FillRectangle(&full_r, &br);
                        br.SetColor(&dt_rgba(
                            spec.rim_outer.0,
                            spec.rim_outer.1,
                            spec.rim_outer.2,
                            0.85,
                        ));
                        rt.DrawRectangle(&full_r, &br, 1.3 * u, None);

                        let hdr_h = (26.0 * u).round();
                        for (i, c) in self.cards.iter().enumerate() {
                            let is_hov = self.hovered_card == Some(i);
                            let is_close_hov = self.hovered_close == Some(i);
                            let card_rad = match spec.shape {
                                DockShelfShape::Chamfer | DockShelfShape::Tile => 4.0 * u,
                                _ => 8.0 * u,
                            };
                            let c_rr = D2D1_ROUNDED_RECT {
                                rect: c.card_rect,
                                radiusX: card_rad,
                                radiusY: card_rad,
                            };
                            br.SetColor(&dt_rgba(
                                spec.bg_top.0,
                                spec.bg_top.1,
                                spec.bg_top.2,
                                if is_hov { 0.96 } else { 0.72 },
                            ));
                            rt.FillRoundedRectangle(&c_rr, &br);
                            br.SetColor(&dt_rgba(
                                spec.rim_outer.0,
                                spec.rim_outer.1,
                                spec.rim_outer.2,
                                if is_hov { 0.92 } else { 0.36 },
                            ));
                            rt.DrawRoundedRectangle(&c_rr, &br, if is_hov { 1.3 * u } else { 1.0 * u }, None);

                            let ico_sz = 14.0 * u;
                            let ico_r = D2D_RECT_F {
                                left: c.card_rect.left + 6.0 * u,
                                top: c.card_rect.top + (hdr_h - ico_sz) * 0.5,
                                right: c.card_rect.left + 6.0 * u + ico_sz,
                                bottom: c.card_rect.top + (hdr_h - ico_sz) * 0.5 + ico_sz,
                            };
                            if let Some(Some(ref bmp)) = card_bmps.get(i) {
                                rt.DrawBitmap(
                                    bmp,
                                    Some(&ico_r),
                                    1.0,
                                    D2D1_BITMAP_INTERPOLATION_MODE_LINEAR,
                                    None,
                                );
                            }

                            if let Some(ref tft) = self.tf_title {
                                br.SetColor(&rgba(
                                    pal.text_primary.0,
                                    pal.text_primary.1,
                                    pal.text_primary.2,
                                    0.95,
                                ));
                                let tr = D2D_RECT_F {
                                    left: ico_r.right + 5.0 * u,
                                    top: c.card_rect.top + 1.0 * u,
                                    right: c.close_rect.left - 4.0 * u,
                                    bottom: c.card_rect.top + hdr_h,
                                };
                                let title_txt = if c.is_iconic {
                                    format!("{} (Min)", c.title)
                                } else {
                                    c.title.clone()
                                };
                                let wt = to_wide(&truncate_title(&title_txt, 22));
                                let len = wt.len().saturating_sub(1);
                                rt.DrawText(
                                    &wt[..len],
                                    tft,
                                    &tr,
                                    &br,
                                    windows::Win32::Graphics::Direct2D::D2D1_DRAW_TEXT_OPTIONS_CLIP,
                                    windows::Win32::Graphics::DirectWrite::DWRITE_MEASURING_MODE_NATURAL,
                                );
                            }

                            if is_close_hov {
                                br.SetColor(&rgba(222, 60, 62, 0.90));
                                let xrr = D2D1_ROUNDED_RECT {
                                    rect: c.close_rect,
                                    radiusX: 4.0 * u,
                                    radiusY: 4.0 * u,
                                };
                                rt.FillRoundedRectangle(&xrr, &br);
                            }
                            let x_pad = 5.0 * u;
                            br.SetColor(&if is_close_hov {
                                rgba(255, 255, 255, 0.98)
                            } else {
                                rgba(pal.text_muted.0, pal.text_muted.1, pal.text_muted.2, 0.85)
                            });
                            rt.DrawLine(
                                Vector2 { X: c.close_rect.left + x_pad, Y: c.close_rect.top + x_pad },
                                Vector2 { X: c.close_rect.right - x_pad, Y: c.close_rect.bottom - x_pad },
                                &br,
                                1.35 * u,
                                None,
                            );
                            rt.DrawLine(
                                Vector2 { X: c.close_rect.right - x_pad, Y: c.close_rect.top + x_pad },
                                Vector2 { X: c.close_rect.left + x_pad, Y: c.close_rect.bottom - x_pad },
                                &br,
                                1.35 * u,
                                None,
                            );

                            let matte_col = if spec.light_mode_paper {
                                rgba(226, 220, 206, 0.95)
                            } else {
                                dt_rgba(spec.bg_bot.0, spec.bg_bot.1, spec.bg_bot.2, 0.96)
                            };
                            br.SetColor(&matte_col);
                            rt.FillRoundedRectangle(
                                &D2D1_ROUNDED_RECT {
                                    rect: c.thumb_rect,
                                    radiusX: 4.0 * u,
                                    radiusY: 4.0 * u,
                                },
                                &br,
                            );
                            if c.thumb_id == 0 {
                                if let Some(Some(ref bmp)) = card_bmps.get(i) {
                                    let f_sz = 38.0 * u;
                                    let cx = (c.thumb_rect.left + c.thumb_rect.right) * 0.5;
                                    let cy = (c.thumb_rect.top + c.thumb_rect.bottom) * 0.5;
                                    let f_r = D2D_RECT_F {
                                        left: cx - f_sz * 0.5,
                                        top: cy - f_sz * 0.5,
                                        right: cx + f_sz * 0.5,
                                        bottom: cy + f_sz * 0.5,
                                    };
                                    rt.DrawBitmap(
                                        bmp,
                                        Some(&f_r),
                                        0.92,
                                        D2D1_BITMAP_INTERPOLATION_MODE_LINEAR,
                                        None,
                                    );
                                }
                            }
                        }
                    }
                    if rt.EndDraw(None, None).is_err() {
                        paint_ok = false;
                    }
                }
            }
            if paint_ok {
                let _ = BitBlt(hdc_win, 0, 0, cw, ch, Some(self.surface.mem_dc), 0, 0, SRCCOPY);
            } else {
                if let Some(ref rt) = self.dc_rt {
                    crate::realistic_pbr::evict_rt(rt);
                }
                self.dc_rt = None;
                self.icon_cache.clear();
            }
        }
        let _ = EndPaint(self.hwnd, &ps);
    }
}

struct DockMonitorPane {
    is_primary: bool,
    mon_rect: RECT,
    hwnd: HWND,
    surface: DibSurface,
    dpi_u: f32,
    rest_span: f32,
    last_dock_span: f32,
    last_canvas_span: i32,
    last_win_rect: RECT,
    canvas_rect: RECT,
    shelf_rect: RECT,
    layout: Vec<DockLayoutSlot>,
    island_rects_local: Vec<D2D_RECT_F>,
    island_rects_screen: Vec<RECT>,
    hud_wing_local: Option<(D2D_RECT_F, String, String)>,
    cur_scales: HashMap<String, f32>,
    slot_expands: HashMap<String, f32>,
    hover_mix: f32,
    slide_t: f32,
    cursor_axis_norm: f32,
    click_ripple_axis: f32,
    click_ripple_t: f32,
    dock_inited: bool,
    hovered_idx: Option<usize>,
    pressed_idx: Option<usize>,
    cursor_in_dock: bool,
    tracking_leave: bool,
    last_inside_time: Option<Instant>,
    dirty_paint: bool,
    motion_active: bool,
    window_shown: bool,
}

impl DockMonitorPane {
    fn new(mon_rect: RECT, is_primary: bool) -> Self {
        Self {
            is_primary,
            mon_rect,
            hwnd: HWND::default(),
            surface: DibSurface::new(),
            dpi_u: 1.0,
            rest_span: 320.0,
            last_dock_span: 320.0,
            last_canvas_span: 0,
            last_win_rect: RECT::default(),
            canvas_rect: RECT::default(),
            shelf_rect: RECT::default(),
            layout: Vec::new(),
            island_rects_local: Vec::new(),
            island_rects_screen: Vec::new(),
            hud_wing_local: None,
            cur_scales: HashMap::new(),
            slot_expands: HashMap::new(),
            hover_mix: 0.0,
            slide_t: 1.0,
            cursor_axis_norm: 0.5,
            click_ripple_axis: 0.5,
            click_ripple_t: 1.0,
            dock_inited: false,
            hovered_idx: None,
            pressed_idx: None,
            cursor_in_dock: false,
            tracking_leave: false,
            last_inside_time: None,
            dirty_paint: true,
            motion_active: false,
            window_shown: false,
        }
    }

    fn ensure_window(&mut self, hinst: HINSTANCE, register_shell_hook: bool) -> bool {
        if !self.hwnd.0.is_null() && unsafe { IsWindow(Some(self.hwnd)).as_bool() } {
            if register_shell_hook {
                unsafe {
                    let _ = RegisterShellHookWindow(self.hwnd);
                }
            }
            return true;
        }
        unsafe {
            let Ok(hw) = CreateWindowExW(
                WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOACTIVATE,
                MAC_DOCK_WND_CLASS,
                w!("AutumnMacDock"),
                WS_POPUP,
                self.mon_rect.left,
                self.mon_rect.top,
                64,
                64,
                None,
                None,
                Some(hinst),
                None,
            ) else {
                return false;
            };
            if hw.0.is_null() {
                return false;
            }
            if register_shell_hook {
                let _ = RegisterShellHookWindow(hw);
            }
            self.hwnd = hw;
            self.window_shown = false;
            self.dock_inited = false;
            true
        }
    }

    fn update_dpi(&mut self) {
        unsafe {
            let dpi = if !self.hwnd.0.is_null() {
                GetDpiForWindow(self.hwnd).max(96) as f32
            } else {
                96.0
            };
            self.dpi_u = (dpi / 96.0).clamp(1.0, 2.5);
        }
    }

    fn destroy(&mut self, deregister_shell: bool) {
        if !self.hwnd.0.is_null() {
            unsafe {
                if deregister_shell {
                    let _ = DeregisterShellHookWindow(self.hwnd);
                }
                let _ = DestroyWindow(self.hwnd);
            }
            self.hwnd = HWND::default();
        }
        self.window_shown = false;
        self.last_canvas_span = 0;
        self.last_win_rect = RECT::default();
        self.surface.release();
        self.layout.clear();
        self.island_rects_local.clear();
        self.island_rects_screen.clear();
        self.hud_wing_local = None;
        self.cur_scales.clear();
        self.slot_expands.clear();
        self.last_inside_time = None;
        self.hovered_idx = None;
        self.pressed_idx = None;
        self.cursor_in_dock = false;
        self.tracking_leave = false;
        self.hover_mix = 0.0;
    }

    fn hit_slot_at_client(&self, lx: f32, ly: f32) -> Option<usize> {
        for (idx, item) in self.layout.iter().enumerate() {
            if !item.is_divider
                && !item.is_spacer
                && lx >= item.hit_rect.left
                && lx <= item.hit_rect.right
                && ly >= item.hit_rect.top
                && ly <= item.hit_rect.bottom
            {
                return Some(idx);
            }
        }
        None
    }

    fn is_point_in_dock_band(
        &self,
        pt: POINT,
        edge: MacDockEdge,
        layout: DockLayoutKind,
        hud_wing_h: f32,
        u: f32,
    ) -> bool {
        if self.shelf_rect.right <= self.shelf_rect.left
            || self.shelf_rect.bottom <= self.shelf_rect.top
        {
            return false;
        }
        let slack = (10.0 * u).round() as i32;
        let top_pad = (hud_wing_h
            + if layout == DockLayoutKind::FishEyeShelf {
                20.0 * u
            } else {
                6.0 * u
            })
            .round() as i32;
        match edge {
            MacDockEdge::Bottom => {
                pt.x >= self.shelf_rect.left - slack
                    && pt.x <= self.shelf_rect.right + slack
                    && pt.y >= self.shelf_rect.top - top_pad
                    && pt.y <= self.mon_rect.bottom
            }
            MacDockEdge::Top => {
                pt.x >= self.shelf_rect.left - slack
                    && pt.x <= self.shelf_rect.right + slack
                    && pt.y >= self.mon_rect.top
                    && pt.y <= self.shelf_rect.bottom + top_pad
            }
            MacDockEdge::Left => {
                pt.y >= self.shelf_rect.top - slack
                    && pt.y <= self.shelf_rect.bottom + slack
                    && pt.x >= self.mon_rect.left
                    && pt.x <= self.shelf_rect.right + top_pad
            }
            MacDockEdge::Right => {
                pt.y >= self.shelf_rect.top - slack
                    && pt.y <= self.shelf_rect.bottom + slack
                    && pt.x >= self.shelf_rect.left - top_pad
                    && pt.x <= self.mon_rect.right
            }
        }
    }
}

struct DockIconReq {
    cache_key: String,
    app_name: String,
    app_path: String,
    target_exe: String,
    exe_path: String,
    aumid: String,
    hwnd_raw: isize,
    icon_hwnd_raw: isize,
    is_pinned: bool,
}

struct DockIconDecoded {
    cache_key: String,
    pbgra: Option<(u32, u32, Vec<u8>)>,
}

struct DockIconWorker {
    req_tx: mpsc::Sender<DockIconReq>,
    res_rx: mpsc::Receiver<DockIconDecoded>,
}

impl DockIconWorker {
    fn spawn(notify_hwnd: HWND, notify_msg: u32) -> Self {
        let (req_tx, req_rx) = mpsc::channel::<DockIconReq>();
        let (res_tx, res_rx) = mpsc::channel::<DockIconDecoded>();
        let hwnd_raw = notify_hwnd.0 as isize;
        std::thread::spawn(move || unsafe {
            let com_ok = CoInitializeEx(None, COINIT_APARTMENTTHREADED).is_ok();
            let wic_opt = CoCreateInstance::<_, IWICImagingFactory>(
                &CLSID_WICImagingFactory,
                None,
                CLSCTX_INPROC_SERVER,
            )
            .ok();
            while let Ok(req) = req_rx.recv() {
                let pbgra = wic_opt
                    .as_ref()
                    .and_then(|wic| decode_dock_icon_pbgra(wic, &req));
                if res_tx
                    .send(DockIconDecoded {
                        cache_key: req.cache_key,
                        pbgra,
                    })
                    .is_err()
                {
                    break;
                }
                if hwnd_raw != 0 && notify_msg != 0 {
                    let _ = PostMessageW(
                        Some(HWND(hwnd_raw as *mut c_void)),
                        notify_msg,
                        WPARAM(0),
                        LPARAM(0),
                    );
                }
            }
            drop(wic_opt);
            if com_ok {
                CoUninitialize();
            }
        });
        Self { req_tx, res_rx }
    }
}

pub struct MacDockEngine {
    hinst: HINSTANCE,
    shell_hook_msg: u32,
    d2d: Option<ID2D1Factory>,
    dw: Option<IDWriteFactory>,
    wic: Option<IWICImagingFactory>,
    dc_rt: Option<ID2D1DCRenderTarget>,
    solid_brush: Option<ID2D1SolidColorBrush>,
    tf_tip: Option<IDWriteTextFormat>,
    tf_left: Option<IDWriteTextFormat>,
    tf_mono: Option<IDWriteTextFormat>,
    icon_cache: HashMap<String, Option<ID2D1Bitmap>>,
    icon_in_flight: HashSet<String>,
    icon_worker: Option<DockIconWorker>,
    icon_notify_hwnd: HWND,
    icon_notify_msg: u32,
    lnk_target_cache: HashMap<String, String>,
    proc_cache: HashMap<u32, (String, String)>,
    hwnd_aumid_cache: HashMap<isize, String>,

    slots: Vec<DockSlotKind>,
    slot_keys: Vec<String>,
    panes: Vec<DockMonitorPane>,
    bounce_until: HashMap<String, Instant>,
    pop_until: HashMap<String, Instant>,
    flashing_hwnds: HashSet<isize>,
    fg_app_title: String,
    fg_full_title: String,
    cpu_pct: u8,
    ram_pct: u8,
    media_track: String,

    tray_vol_pct: u8,
    tray_vol_muted: bool,
    tray_wifi_on: bool,
    tray_wifi_name: String,
    tray_wifi_signal: u8,
    tray_bat_pct: Option<u8>,
    tray_bat_charging: bool,
    tray_bat_saver: bool,
    tray_snapshot_inited: bool,
    tray_bat_inited: bool,
    tray_anim: TrayAnimState,
    vol_osd_until: Option<Instant>,
    wheel_accum: i32,
    wheel_last_at: Option<Instant>,

    thumb_popup: DockThumbPopup,
    thumb_owner_hwnd: HWND,
    hover_dwell_pane: HWND,
    hover_dwell_key: String,
    hover_dwell_since: Option<Instant>,
    stable_group_order: Vec<String>,
    active_menu_target: (HWND, RECT),

    dock_anim_phase: f32,
    menu_open: bool,

    trash_items: i64,
    trash_items_async: Arc<AtomicI64>,
    trash_poll_in_flight: Arc<AtomicBool>,
    last_trash_poll: Instant,
    last_ambient_paint: Instant,
    last_full_sync: Instant,
    last_idle_check: Instant,
    last_tick: Instant,
    last_clock_bucket: u32,
    telemetry_visible: bool,
    dirty_windows: bool,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct TrayAnimState {
    pub vol_display: f32,
    pub vol_target: f32,
    pub vol_pulse_t: f32,
    pub vol_dir: i8,
    pub vol_badge_ttl: f32,
    pub mute_mix: f32,
    pub audio_vu: [f32; 4],

    pub net_pulse_t: f32,
    pub net_signal_display: f32,
    pub net_signal_target: f32,
    pub net_wave_phase: f32,

    pub bat_display: f32,
    pub bat_target: f32,
    pub bat_pulse_t: f32,
    pub bat_charge_phase: f32,
}

impl Default for TrayAnimState {
    fn default() -> Self {
        Self {
            vol_display: 0.5,
            vol_target: 0.5,
            vol_pulse_t: 1.0,
            vol_dir: 0,
            vol_badge_ttl: 0.0,
            mute_mix: 0.0,
            audio_vu: [0.0; 4],
            net_pulse_t: 1.0,
            net_signal_display: 0.85,
            net_signal_target: 0.85,
            net_wave_phase: 0.0,
            bat_display: 1.0,
            bat_target: 1.0,
            bat_pulse_t: 1.0,
            bat_charge_phase: 0.0,
        }
    }
}

impl TrayAnimState {
    pub fn seed_volume(&mut self, vol_pct: u8, muted: bool) {
        let lvl = if muted { 0.0 } else { (vol_pct as f32 / 100.0).clamp(0.0, 1.0) };
        self.vol_display = lvl;
        self.vol_target = lvl;
        self.mute_mix = if muted { 1.0 } else { 0.0 };
    }

    pub fn on_volume_change(
        &mut self,
        prev_pct: u8,
        new_pct: u8,
        prev_muted: bool,
        new_muted: bool,
        show_badge: bool,
    ) {
        self.vol_target = if new_muted {
            0.0
        } else {
            (new_pct as f32 / 100.0).clamp(0.0, 1.0)
        };
        self.vol_dir = if prev_muted != new_muted {
            0
        } else if new_pct > prev_pct {
            1
        } else {
            -1
        };
        self.vol_pulse_t = 0.0;
        if show_badge {
            self.vol_badge_ttl = 1.45;
        }
    }

    pub fn seed_net(&mut self, wifi_on: bool, signal_pct: u8) {
        let tgt = if wifi_on {
            (signal_pct as f32 / 100.0).clamp(0.18, 1.0)
        } else {
            0.0
        };
        self.net_signal_display = tgt;
        self.net_signal_target = tgt;
    }

    pub fn on_net_change(&mut self, wifi_on: bool, signal_pct: u8, trigger_pulse: bool) {
        self.net_signal_target = if wifi_on {
            (signal_pct as f32 / 100.0).clamp(0.18, 1.0)
        } else {
            0.0
        };
        if trigger_pulse {
            self.net_pulse_t = 0.0;
        }
    }

    pub fn seed_battery(&mut self, bat_pct: Option<u8>) {
        let lvl = bat_pct
            .map(|p| (p as f32 / 100.0).clamp(0.0, 1.0))
            .unwrap_or(1.0);
        self.bat_display = lvl;
        self.bat_target = lvl;
    }

    pub fn on_battery_change(&mut self, bat_pct: Option<u8>, trigger_pulse: bool) {
        self.bat_target = bat_pct
            .map(|p| (p as f32 / 100.0).clamp(0.0, 1.0))
            .unwrap_or(1.0);
        if trigger_pulse {
            self.bat_pulse_t = 0.0;
        }
    }

    pub fn update_audio_vu(&mut self, raw_bands: &[f32; 12]) {
        for g in 0..4 {
            let base = g * 3;
            let avg = (raw_bands[base].max(0.0)
                + raw_bands[base + 1].max(0.0)
                + raw_bands[base + 2].max(0.0))
                / 3.0;
            let tgt = avg.clamp(0.0, 1.0);
            if tgt > self.audio_vu[g] {
                self.audio_vu[g] = self.audio_vu[g] * 0.30 + tgt * 0.70;
            } else {
                let next = self.audio_vu[g] * 0.72 + tgt * 0.28;
                self.audio_vu[g] = if next < 0.008 { 0.0 } else { next };
            }
        }
    }

    pub fn has_audio_vu_energy(&self) -> bool {
        self.audio_vu.iter().any(|&v| v > 0.012)
    }

    pub fn is_active(
        &self,
        cfg: &AppConfig,
        muted: bool,
        bat_charging: bool,
        bat_low: bool,
    ) -> bool {
        let target_mute = if muted { 1.0 } else { 0.0 };
        let transient = self.vol_pulse_t < 0.995
            || self.vol_badge_ttl > 0.005
            || (self.vol_display - self.vol_target).abs() > 0.004
            || (self.mute_mix - target_mute).abs() > 0.004
            || self.net_pulse_t < 0.995
            || (self.net_signal_display - self.net_signal_target).abs() > 0.004
            || self.bat_pulse_t < 0.995
            || (self.bat_display - self.bat_target).abs() > 0.004;
        if transient {
            return true;
        }
        if cfg.w_tray.audio_style == TrayAudioMeterStyle::LiveVuEqualizer
            && self.has_audio_vu_energy()
        {
            return true;
        }
        cfg.tb_ambient_anim
            && cfg.w_tray.battery_style != TrayBatteryMeterStyle::Hidden
            && (bat_charging || bat_low)
    }

    pub fn step(
        &mut self,
        dt_raw: f32,
        muted: bool,
        bat_charging: bool,
        bat_low: bool,
        allow_ambient: bool,
    ) -> bool {
        let dt = dt_raw.clamp(0.004, 0.06);
        let lerp_fast = 1.0 - (-dt * 16.0).exp();
        let lerp_med = 1.0 - (-dt * 10.0).exp();
        let target_mute = if muted { 1.0 } else { 0.0 };
        let mut changed = false;

        let step_f = |cur: &mut f32, tgt: f32, k: f32| -> bool {
            let diff = tgt - *cur;
            if diff.abs() > 0.002 {
                *cur += diff * k;
                if (tgt - *cur).abs() <= 0.002 {
                    *cur = tgt;
                }
                true
            } else if (*cur - tgt).abs() > 0.0 {
                *cur = tgt;
                false
            } else {
                false
            }
        };

        changed |= step_f(&mut self.vol_display, self.vol_target, lerp_fast);
        changed |= step_f(&mut self.mute_mix, target_mute, lerp_fast);
        changed |= step_f(&mut self.net_signal_display, self.net_signal_target, lerp_med);
        changed |= step_f(&mut self.bat_display, self.bat_target, lerp_med);

        if self.vol_pulse_t < 1.0 {
            self.vol_pulse_t = (self.vol_pulse_t + dt * 2.15).min(1.0);
            changed = true;
        }
        if self.vol_badge_ttl > 0.0 {
            self.vol_badge_ttl = (self.vol_badge_ttl - dt).max(0.0);
            changed = true;
        }
        if self.net_pulse_t < 1.0 {
            self.net_pulse_t = (self.net_pulse_t + dt * 1.65).min(1.0);
            self.net_wave_phase = (self.net_wave_phase + dt * 6.0) % 628.3185;
            changed = true;
        }
        if self.bat_pulse_t < 1.0 {
            self.bat_pulse_t = (self.bat_pulse_t + dt * 1.65).min(1.0);
            changed = true;
        }
        if allow_ambient && (bat_charging || bat_low) {
            self.bat_charge_phase = (self.bat_charge_phase + dt * 2.8) % 628.3185;
            changed = true;
        }
        if self.has_audio_vu_energy() {
            changed = true;
        }
        changed
    }
}

fn compute_dock_timer_ms(
    fast_active: bool,
    hover_active: bool,
    thumb_vis: bool,
    ambient_idle: bool,
    anim_fps: u32,
) -> u32 {
    let frame_ms = AppConfig::anim_frame_ms_for(anim_fps);
    if fast_active || hover_active || ambient_idle {
        frame_ms
    } else if thumb_vis {
        (frame_ms * 2).clamp(40, 80)
    } else {
        0
    }
}

impl MacDockEngine {
    pub fn new(hinst: HINSTANCE) -> Self {
        let now = Instant::now();
        Self {
            hinst,
            shell_hook_msg: unsafe { RegisterWindowMessageW(w!("SHELLHOOK")) },
            d2d: None,
            dw: None,
            wic: None,
            dc_rt: None,
            solid_brush: None,
            tf_tip: None,
            tf_left: None,
            tf_mono: None,
            icon_cache: HashMap::new(),
            icon_in_flight: HashSet::new(),
            icon_worker: None,
            icon_notify_hwnd: HWND::default(),
            icon_notify_msg: 0,
            lnk_target_cache: HashMap::new(),
            proc_cache: HashMap::new(),
            hwnd_aumid_cache: HashMap::new(),
            slots: Vec::new(),
            slot_keys: Vec::new(),
            panes: Vec::new(),
            bounce_until: HashMap::new(),
            pop_until: HashMap::new(),
            flashing_hwnds: HashSet::new(),
            fg_app_title: "Desktop".to_string(),
            fg_full_title: "Desktop".to_string(),
            cpu_pct: 12,
            ram_pct: 42,
            media_track: String::new(),
            tray_vol_pct: 50,
            tray_vol_muted: false,
            tray_wifi_on: true,
            tray_wifi_name: "Wi-Fi".to_string(),
            tray_wifi_signal: 85,
            tray_bat_pct: None,
            tray_bat_charging: false,
            tray_bat_saver: false,
            tray_snapshot_inited: false,
            tray_bat_inited: false,
            tray_anim: TrayAnimState::default(),
            vol_osd_until: None,
            wheel_accum: 0,
            wheel_last_at: None,
            thumb_popup: DockThumbPopup::new(hinst),
            thumb_owner_hwnd: HWND::default(),
            hover_dwell_pane: HWND::default(),
            hover_dwell_key: String::new(),
            hover_dwell_since: None,
            stable_group_order: Vec::new(),
            active_menu_target: (
                HWND::default(),
                RECT { left: 0, top: 0, right: 1920, bottom: 1200 },
            ),
            dock_anim_phase: 0.0,
            menu_open: false,
            trash_items: 0,
            trash_items_async: Arc::new(AtomicI64::new(0)),
            trash_poll_in_flight: Arc::new(AtomicBool::new(false)),
            last_trash_poll: now - Duration::from_secs(60),
            last_ambient_paint: now - Duration::from_secs(1),
            last_full_sync: now - Duration::from_secs(60),
            last_idle_check: now - Duration::from_secs(60),
            last_tick: now,
            last_clock_bucket: u32::MAX,
            telemetry_visible: false,
            dirty_windows: true,
        }
    }

    pub fn hwnd(&self) -> HWND {
        self.panes.first().map(|p| p.hwnd).unwrap_or_default()
    }

    pub fn owns_hwnd(&self, hwnd: HWND) -> bool {
        !hwnd.0.is_null() && self.panes.iter().any(|p| p.hwnd == hwnd)
    }

    pub fn thumb_hwnd(&self) -> HWND {
        self.thumb_popup.hwnd
    }

    pub fn shell_hook_msg(&self) -> u32 {
        self.shell_hook_msg
    }

    pub fn is_active(&self) -> bool {
        self.panes.iter().any(|p| !p.hwnd.0.is_null())
    }

    pub fn desired_work_area_reserves(
        &mut self,
        cfg: &AppConfig,
    ) -> Vec<(RECT, crate::taskbar_hide::TaskbarEdge, i32)> {
        if !cfg.mac_dock_enabled
            || cfg.mac_dock_hide != MacDockHideMode::AlwaysVisible
            || cfg.is_dock_all_elements_off()
        {
            return Vec::new();
        }
        self.reconcile_monitors(true);
        let edge = match cfg.mac_dock_edge {
            MacDockEdge::Bottom => crate::taskbar_hide::TaskbarEdge::Bottom,
            MacDockEdge::Top => crate::taskbar_hide::TaskbarEdge::Top,
            MacDockEdge::Left => crate::taskbar_hide::TaskbarEdge::Left,
            MacDockEdge::Right => crate::taskbar_hide::TaskbarEdge::Right,
        };
        let mut out = Vec::with_capacity(self.panes.len());
        for pane in &mut self.panes {
            pane.update_dpi();
            let (_spec, _is_vertical, _base_sz, shelf_cross, edge_gap, hud_wing_h) =
                dock_cross_metrics(cfg, pane.dpi_u);
            let reserve_px = (edge_gap + shelf_cross + hud_wing_h).ceil() as i32;
            out.push((pane.mon_rect, edge, reserve_px));
        }
        out
    }

    pub fn set_notify_target(&mut self, hwnd: HWND, icon_ready_msg: u32) {
        self.icon_notify_hwnd = hwnd;
        self.icon_notify_msg = icon_ready_msg;
    }

    pub fn apply_ready_icons(&mut self) -> bool {
        let mut decoded = Vec::new();
        if let Some(ref worker) = self.icon_worker {
            while let Ok(item) = worker.res_rx.try_recv() {
                decoded.push(item);
            }
        }
        if decoded.is_empty() {
            return false;
        }
        let mut any = false;
        let props = D2D1_BITMAP_PROPERTIES {
            pixelFormat: D2D1_PIXEL_FORMAT {
                format: DXGI_FORMAT_B8G8R8A8_UNORM,
                alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
            },
            dpiX: 96.0,
            dpiY: 96.0,
        };
        for item in decoded {
            self.icon_in_flight.remove(&item.cache_key);
            let bmp = item.pbgra.and_then(|(w, h, buf)| {
                self.dc_rt.as_ref().and_then(|rt| unsafe {
                    rt.CreateBitmap(
                        D2D_SIZE_U { width: w, height: h },
                        Some(buf.as_ptr() as *const c_void),
                        w * 4,
                        &props,
                    )
                    .ok()
                })
            });
            self.icon_cache.insert(item.cache_key, bmp);
            any = true;
        }
        if any {
            self.mark_dirty_paint();
        }
        any
    }

    pub fn invalidate_windows(&mut self) {
        self.dirty_windows = true;
    }

    pub fn clear_icon_cache(&mut self) {
        self.icon_cache.clear();
        self.icon_in_flight.clear();
        self.lnk_target_cache.clear();
        self.proc_cache.clear();
        self.hwnd_aumid_cache.clear();
    }

    pub fn mark_dirty_paint(&mut self) {
        for pane in &mut self.panes {
            pane.dirty_paint = true;
        }
    }

    pub fn cancel_left_press(&mut self) {
        for pane in &mut self.panes {
            if pane.pressed_idx.take().is_some() {
                pane.dirty_paint = true;
            }
        }
    }

    fn pane_idx_for_hwnd(&self, hwnd: HWND) -> Option<usize> {
        if hwnd.0.is_null() {
            return None;
        }
        self.panes.iter().position(|p| p.hwnd == hwnd)
    }

    fn menu_hwnd(&self) -> HWND {
        if !self.active_menu_target.0.0.is_null() {
            self.active_menu_target.0
        } else {
            self.hwnd()
        }
    }

    fn menu_mon_rect(&self) -> RECT {
        if self.active_menu_target.1.right > self.active_menu_target.1.left {
            self.active_menu_target.1
        } else {
            self.panes.first().map(|p| p.mon_rect).unwrap_or(RECT {
                left: 0,
                top: 0,
                right: 1920,
                bottom: 1200,
            })
        }
    }

    pub fn sync_tray_snapshot(&mut self, cc: &CcSnapshot) {
        let osd_active = self
            .vol_osd_until
            .map(|t| t > Instant::now())
            .unwrap_or(false);
        let prev_vol = self.tray_vol_pct;
        let prev_muted = self.tray_vol_muted;
        let prev_wifi = self.tray_wifi_on;
        let prev_sig = self.tray_wifi_signal;
        let next_pct = if osd_active { self.tray_vol_pct } else { cc.vol_pct };
        let vol_changed = prev_vol != next_pct || prev_muted != cc.vol_muted;
        let net_changed = prev_wifi != cc.wifi_on
            || self.tray_wifi_name != cc.wifi_name
            || (prev_sig as i16 - cc.wifi_signal as i16).abs() >= 12;
        let changed = vol_changed
            || net_changed
            || self.tray_wifi_signal != cc.wifi_signal
            || self.tray_bat_saver != cc.saver_on;
        self.tray_vol_pct = next_pct;
        self.tray_vol_muted = cc.vol_muted;
        self.tray_wifi_on = cc.wifi_on;
        self.tray_wifi_name = cc.wifi_name.clone();
        self.tray_wifi_signal = cc.wifi_signal;
        self.tray_bat_saver = cc.saver_on;

        if !self.tray_snapshot_inited {
            self.tray_snapshot_inited = true;
            self.tray_anim.seed_volume(self.tray_vol_pct, self.tray_vol_muted);
            self.tray_anim.seed_net(self.tray_wifi_on, self.tray_wifi_signal);
        } else {
            if vol_changed {
                self.tray_anim.on_volume_change(
                    prev_vol,
                    self.tray_vol_pct,
                    prev_muted,
                    self.tray_vol_muted,
                    true,
                );
            }
            if net_changed || self.tray_wifi_signal != prev_sig {
                self.tray_anim.on_net_change(
                    self.tray_wifi_on,
                    self.tray_wifi_signal,
                    net_changed,
                );
            }
        }
        if changed {
            self.mark_dirty_paint();
        }
    }

    pub fn sync_audio_bands(&mut self, raw_bands: &[f32; 12]) {
        let had = self.tray_anim.has_audio_vu_energy();
        self.tray_anim.update_audio_vu(raw_bands);
        if had || self.tray_anim.has_audio_vu_energy() {
            self.mark_dirty_paint();
        }
    }

    pub fn sync_telemetry_snapshot(&mut self, cpu_pct: u8, ram_pct: u8, media_track: &str) {
        let clean_m = media_track.trim();
        if self.cpu_pct != cpu_pct || self.ram_pct != ram_pct || self.media_track != clean_m {
            self.cpu_pct = cpu_pct;
            self.ram_pct = ram_pct;
            self.media_track = clean_m.to_string();
            if self.telemetry_visible {
                self.mark_dirty_paint();
            }
        }
    }

    pub fn tick_idle_poll(&mut self, cfg: &AppConfig) {
        if !cfg.mac_dock_enabled {
            return;
        }
        let now = Instant::now();
        let bat_low = self.tray_bat_pct.map(|p| p <= 20).unwrap_or(false) && !self.tray_bat_charging;
        let tray_anim_active =
            self.tray_anim
                .is_active(cfg, self.tray_vol_muted, self.tray_bat_charging, bat_low);
        if self.panes.is_empty()
            || self.dirty_windows
            || self.menu_open
            || self.hover_dwell_since.is_some()
            || !self.bounce_until.is_empty()
            || !self.pop_until.is_empty()
            || !self.flashing_hwnds.is_empty()
            || self.vol_osd_until.is_some()
            || tray_anim_active
            || self.panes
                .iter()
                .any(|p| p.dirty_paint || p.motion_active || p.cursor_in_dock || !p.dock_inited)
        {
            self.tick(cfg);
            return;
        }
        let cur_bucket = clock_bucket(cfg);
        let clock_changed = cur_bucket != self.last_clock_bucket;
        let full_due = now.duration_since(self.last_full_sync) >= Duration::from_millis(4000);

        if cfg.mac_dock_hide == MacDockHideMode::StealthHide {
            if self.panes.iter().any(|p| p.window_shown || p.slide_t > 0.005) {
                self.tick(cfg);
            }
            return;
        }

        if cfg.mac_dock_hide == MacDockHideMode::AlwaysVisible {
            if self.panes.iter().all(|p| p.window_shown) {
                if !clock_changed && !full_due {
                    return;
                }
                if clock_changed && !full_due {
                    self.update_clock_slot_only(cfg, cur_bucket);
                    return;
                }
            }
            self.tick(cfg);
            return;
        }

        // Auto-hide / Hover-reveal: wake immediately on screen-edge hit, otherwise throttle hidden checks
        let mut pt = POINT::default();
        let got_cursor = unsafe { GetCursorPos(&mut pt).is_ok() };
        let on_edge = got_cursor
            && self.panes.iter().any(|p| {
                is_point_on_screen_edge(
                    pt,
                    p.mon_rect,
                    cfg.mac_dock_edge,
                    (4.0 * p.dpi_u).round().max(4.0) as i32,
                )
            });
        let any_shown = self.panes.iter().any(|p| p.window_shown);
        if on_edge
            || any_shown
            || clock_changed && any_shown
            || now.duration_since(self.last_idle_check) >= Duration::from_millis(240)
            || full_due
        {
            self.last_idle_check = now;
            self.tick(cfg);
        }
    }

    pub fn force_clock_refresh(&mut self, cfg: &AppConfig) {
        if !self.is_active() {
            return;
        }
        let bucket = clock_bucket(cfg);
        self.update_clock_slot_only(cfg, bucket);
    }

    fn update_clock_slot_only(&mut self, cfg: &AppConfig, bucket: u32) {
        self.last_clock_bucket = bucket;
        let (time_str, date_str, hour24, minute, second) = format_dock_clock(cfg);
        let mut updated = false;
        for slot in &mut self.slots {
            if let DockSlotKind::ClockModule {
                time_str: ref mut ts,
                date_str: ref mut ds,
                hour24: ref mut h,
                minute: ref mut m,
                second: ref mut s,
            } = slot
            {
                *ts = time_str.clone();
                *ds = date_str.clone();
                *h = hour24;
                *m = minute;
                *s = second;
                updated = true;
            }
        }
        if updated {
            self.mark_dirty_paint();
            self.tick_panes_only(cfg);
        }
    }

    fn tick_panes_only(&mut self, cfg: &AppConfig) {
        let now = Instant::now();
        let dt = (now - self.last_tick).as_secs_f32().clamp(0.004, 0.05);
        self.last_tick = now;
        let mut pt = POINT::default();
        let got_cursor = unsafe { GetCursorPos(&mut pt).is_ok() };
        let mut panes = std::mem::take(&mut self.panes);
        for pane in &mut panes {
            self.tick_pane(pane, cfg, pt, got_cursor, dt, now);
        }
        self.panes = panes;
    }

    pub fn handle_shell_hook(&mut self, hook_code: u32, hwnd: HWND, cfg: &AppConfig) -> bool {
        const HSHELL_WINDOWDESTROYED: u32 = 2;
        const HSHELL_WINDOWACTIVATED: u32 = 4;
        const HSHELL_RUDEAPPACTIVATED: u32 = 0x8004;
        const HSHELL_FLASH: u32 = 0x8006;

        let raw = hwnd.0 as isize;
        if raw != 0 {
            if hook_code == HSHELL_FLASH {
                let fg = unsafe { GetForegroundWindow() };
                if hwnd != fg {
                    self.flashing_hwnds.insert(raw);
                }
            } else if hook_code == HSHELL_WINDOWACTIVATED
                || hook_code == HSHELL_RUDEAPPACTIVATED
                || hook_code == HSHELL_WINDOWDESTROYED
            {
                self.flashing_hwnds.remove(&raw);
                if hook_code == HSHELL_WINDOWDESTROYED {
                    self.hwnd_aumid_cache.remove(&raw);
                }
            }
        }
        self.dirty_windows = true;
        self.mark_dirty_paint();
        // Coalesce bursts of shell events during app launch so we never synchronously re-enum
        // and repaint 10+ times while a new app's UI thread is starting up.
        if self.last_full_sync.elapsed() >= Duration::from_millis(65) {
            self.tick(cfg);
            false
        } else {
            true
        }
    }

    pub fn desired_timer_ms(&self, cfg: &AppConfig) -> u32 {
        if !cfg.mac_dock_enabled || !self.is_active() {
            return 0;
        }
        let now = Instant::now();
        let bouncing = self.bounce_until.values().any(|&t| t > now)
            || self.pop_until.values().any(|&t| t > now);
        let osd_active = self.vol_osd_until.map(|t| t > now).unwrap_or(false);
        let any_shown = self.panes.iter().any(|p| p.window_shown && p.slide_t > 0.25);
        let bat_low = self.tray_bat_pct.map(|p| p <= 20).unwrap_or(false) && !self.tray_bat_charging;
        let tray_reactive = any_shown
            && self.tray_anim.is_active(
                cfg,
                self.tray_vol_muted,
                self.tray_bat_charging,
                bat_low,
            );
        let mut fast_16 = bouncing || osd_active || tray_reactive;
        let mut slow_33 = self.hover_dwell_since.is_some()
            && cfg.tb_hover_react
            && !self.thumb_popup.visible;
        let ambient_live = is_ambient_animated_cfg(cfg);
        let ambient_idle = any_shown && (ambient_live || !self.flashing_hwnds.is_empty());
        for p in &self.panes {
            if p.motion_active
                || p.dirty_paint
                || p.click_ripple_t < 0.995
                || (p.slide_t > 0.008 && p.slide_t < 0.992)
            {
                fast_16 = true;
            } else if p.cursor_in_dock && cfg.tb_hover_react {
                if (p.hover_mix > 0.006 && p.hover_mix < 0.994) || ambient_live {
                    slow_33 = true;
                }
            }
        }
        compute_dock_timer_ms(
            fast_16,
            slow_33,
            self.thumb_popup.visible,
            ambient_idle,
            cfg.anim_fps,
        )
    }

    fn ensure_graphics(&mut self) -> bool {
        unsafe {
            if self.d2d.is_none() {
                self.d2d = D2D1CreateFactory::<ID2D1Factory>(
                    D2D1_FACTORY_TYPE_SINGLE_THREADED,
                    None,
                )
                .ok();
            }
            if self.dw.is_none() {
                self.dw = DWriteCreateFactory::<IDWriteFactory>(DWRITE_FACTORY_TYPE_SHARED).ok();
            }
            if self.wic.is_none() {
                self.wic =
                    CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER).ok();
            }
            if self.tf_tip.is_none() {
                if let Some(ref dw) = self.dw {
                    if let Ok(tf) = dw.CreateTextFormat(
                        w!("Segoe UI Variable Text"),
                        None,
                        DWRITE_FONT_WEIGHT_SEMI_BOLD,
                        DWRITE_FONT_STYLE_NORMAL,
                        DWRITE_FONT_STRETCH_NORMAL,
                        12.0,
                        w!("en-US"),
                    ) {
                        let _ = tf.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER);
                        let _ = tf.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER);
                        self.tf_tip = Some(tf);
                    }
                    if let Ok(tfl) = dw.CreateTextFormat(
                        w!("Segoe UI Variable Text"),
                        None,
                        DWRITE_FONT_WEIGHT_SEMI_BOLD,
                        DWRITE_FONT_STYLE_NORMAL,
                        DWRITE_FONT_STRETCH_NORMAL,
                        12.0,
                        w!("en-US"),
                    ) {
                        let _ = tfl.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_LEADING);
                        let _ = tfl.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER);
                        let _ = tfl.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP);
                        self.tf_left = Some(tfl);
                    }
                    if let Ok(tfm) = dw.CreateTextFormat(
                        w!("Consolas"),
                        None,
                        DWRITE_FONT_WEIGHT_SEMI_BOLD,
                        DWRITE_FONT_STYLE_NORMAL,
                        DWRITE_FONT_STRETCH_NORMAL,
                        11.5,
                        w!("en-US"),
                    ) {
                        let _ = tfm.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER);
                        let _ = tfm.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER);
                        let _ = tfm.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP);
                        self.tf_mono = Some(tfm);
                    }
                }
            }
            if self.dc_rt.is_none() {
                if let Some(ref d2d) = self.d2d {
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
                    self.dc_rt = d2d.CreateDCRenderTarget(&props).ok();
                }
            }
            self.d2d.is_some() && self.dc_rt.is_some()
        }
    }

    fn reconcile_monitors(&mut self, ensure_windows: bool) {
        let mut infos = crate::taskbar_hide::TaskbarHider::query_all_monitors();
        if infos.is_empty() {
            infos.push(windows::Win32::Graphics::Gdi::MONITORINFO {
                cbSize: size_of::<windows::Win32::Graphics::Gdi::MONITORINFO>() as u32,
                rcMonitor: RECT { left: 0, top: 0, right: 1920, bottom: 1200 },
                rcWork: RECT { left: 0, top: 0, right: 1920, bottom: 1200 },
                dwFlags: 1,
            });
        }
        infos.sort_by_key(|m| {
            let is_primary = (m.dwFlags & 1) != 0;
            (!is_primary, m.rcMonitor.left, m.rcMonitor.top)
        });
        let mut old = std::mem::take(&mut self.panes);
        let mut next = Vec::with_capacity(infos.len());
        for (idx, mi) in infos.into_iter().enumerate() {
            let is_primary = idx == 0;
            let existing_pos = old
                .iter()
                .position(|p| p.mon_rect == mi.rcMonitor)
                .or_else(|| if idx == 0 && !old.is_empty() { Some(0) } else { None });
            let mut pane = if let Some(pos) = existing_pos {
                old.swap_remove(pos)
            } else {
                DockMonitorPane::new(mi.rcMonitor, is_primary)
            };
            let prev_rect = pane.mon_rect;
            pane.is_primary = is_primary;
            pane.mon_rect = mi.rcMonitor;
            if prev_rect != mi.rcMonitor {
                pane.dirty_paint = true;
            }
            if ensure_windows {
                pane.ensure_window(self.hinst, is_primary);
                pane.update_dpi();
            }
            next.push(pane);
        }
        for mut unused in old {
            if self.thumb_owner_hwnd == unused.hwnd {
                self.thumb_popup.hide();
                self.thumb_owner_hwnd = HWND::default();
            }
            if self.hover_dwell_pane == unused.hwnd {
                self.hover_dwell_pane = HWND::default();
                self.hover_dwell_since = None;
                self.hover_dwell_key.clear();
            }
            let was_prim = unused.is_primary;
            unused.destroy(was_prim);
        }
        self.panes = next;
    }

    pub fn disable(&mut self) {
        TRAY_OVERFLOW_ABORT.store(true, std::sync::atomic::Ordering::SeqCst);
        self.thumb_popup.destroy();
        self.thumb_owner_hwnd = HWND::default();
        self.hover_dwell_pane = HWND::default();
        for mut pane in self.panes.drain(..) {
            let was_prim = pane.is_primary;
            pane.destroy(was_prim);
        }
        self.icon_cache.clear();
        self.proc_cache.clear();
        self.solid_brush = None;
        if let Some(ref rt) = self.dc_rt {
            crate::realistic_pbr::evict_rt(rt);
        }
        self.dc_rt = None;
        self.slots.clear();
        self.slot_keys.clear();
        self.bounce_until.clear();
        self.pop_until.clear();
        self.flashing_hwnds.clear();
        self.vol_osd_until = None;
        self.hover_dwell_since = None;
        self.hover_dwell_key.clear();
        self.stable_group_order.clear();
    }

    pub fn cleanup(&mut self) {
        self.icon_worker = None;
        self.icon_in_flight.clear();
        self.disable();
        self.lnk_target_cache.clear();
        self.tf_tip = None;
        self.tf_left = None;
        self.tf_mono = None;
        self.d2d = None;
        self.dw = None;
        self.wic = None;
    }

    pub fn sync(&mut self, cfg: &AppConfig) {
        if !cfg.mac_dock_enabled {
            if self.is_active() {
                self.disable();
            }
            return;
        }
        TRAY_OVERFLOW_ABORT.store(false, std::sync::atomic::Ordering::SeqCst);
        self.reconcile_monitors(true);
        self.dirty_windows = true;
        self.mark_dirty_paint();
        self.tick(cfg);
    }

    fn poll_trash_count(&mut self) {
        self.trash_items = self.trash_items_async.load(Ordering::Relaxed);
        if self.last_trash_poll.elapsed() < Duration::from_secs(8) {
            return;
        }
        if self
            .trash_poll_in_flight
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return;
        }
        self.last_trash_poll = Instant::now();
        let target = Arc::clone(&self.trash_items_async);
        let flag = Arc::clone(&self.trash_poll_in_flight);
        let _ = std::thread::Builder::new()
            .name("autumn-trash-poll".into())
            .spawn(move || {
                unsafe {
                    let mut rbi = SHQUERYRBINFO {
                        cbSize: size_of::<SHQUERYRBINFO>() as u32,
                        ..Default::default()
                    };
                    if SHQueryRecycleBinW(PCWSTR::null(), &mut rbi).is_ok() {
                        target.store(rbi.i64NumItems.max(0), Ordering::Relaxed);
                    }
                }
                flag.store(false, Ordering::Release);
            });
    }

    fn resolve_shortcut_target(&mut self, lnk_path: &str) -> String {
        let low = lnk_path.to_ascii_lowercase();
        if let Some(cached) = self.lnk_target_cache.get(&low) {
            return cached.clone();
        }
        let mut target = String::new();
        if low.ends_with(".lnk") {
            unsafe {
                if let Ok(link) = CoCreateInstance::<_, IShellLinkW>(
                    &ShellLink,
                    None,
                    CLSCTX_INPROC_SERVER,
                ) {
                    if let Ok(pf) = link.cast::<IPersistFile>() {
                        let wpath = to_wide(lnk_path);
                        if pf.Load(PCWSTR(wpath.as_ptr()), STGM_READ).is_ok() {
                            let mut buf = [0u16; 520];
                            let mut fd = WIN32_FIND_DATAW::default();
                            if link.GetPath(&mut buf, &mut fd, SLGP_RAWPATH.0 as u32).is_ok() {
                                let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
                                target = String::from_utf16_lossy(&buf[..end]).trim().to_string();
                            }
                        }
                    }
                }
            }
        } else {
            target = lnk_path.trim().to_string();
        }
        self.lnk_target_cache.insert(low, target.clone());
        target
    }

    fn rebuild_slots(&mut self, cfg: &AppConfig) -> bool {
        self.dirty_windows = false;
        self.last_full_sync = Instant::now();
        self.last_clock_bucket = clock_bucket(cfg);
        self.telemetry_visible = cfg.w_hud.mode == HudWingMode::AlwaysEnabled
            || (cfg.w_hud.mode == HudWingMode::InheritTheme
                && resolve_shelf_spec(cfg).layout == DockLayoutKind::HudConsoleDeck)
            || cfg.w_tray.net_style == TrayNetMeterStyle::LiveThroughputText;
        self.poll_trash_count();

        let mut pwr = SYSTEM_POWER_STATUS::default();
        if unsafe { GetSystemPowerStatus(&mut pwr).is_ok() } {
            let prev_bat = self.tray_bat_pct;
            let prev_chg = self.tray_bat_charging;
            if pwr.BatteryFlag != 128 && pwr.BatteryLifePercent <= 100 {
                self.tray_bat_pct = Some(pwr.BatteryLifePercent);
                self.tray_bat_charging = pwr.ACLineStatus == 1;
            } else {
                self.tray_bat_pct = None;
                self.tray_bat_charging = false;
            }
            if !self.tray_bat_inited {
                self.tray_bat_inited = true;
                self.tray_anim.seed_battery(self.tray_bat_pct);
            } else if prev_bat != self.tray_bat_pct || prev_chg != self.tray_bat_charging {
                self.tray_anim.on_battery_change(self.tray_bat_pct, true);
                self.mark_dirty_paint();
            }
            if pwr.SystemStatusFlag > 0 {
                self.tray_bat_saver = true;
            }
        }

        let running_wins = collect_running_windows(&mut self.proc_cache, &mut self.hwnd_aumid_cache);
        let fg_hwnd = unsafe { GetForegroundWindow() };
        let fg_raw = fg_hwnd.0 as isize;
        let alive_hwnds: HashSet<isize> = running_wins.iter().map(|w| w.hwnd.0 as isize).collect();
        self.flashing_hwnds
            .retain(|raw| *raw != fg_raw && alive_hwnds.contains(raw));
        self.hwnd_aumid_cache.retain(|raw, _| alive_hwnds.contains(raw));

        let mut fg_name = String::new();
        for w in &running_wins {
            if w.hwnd == fg_hwnd {
                let p = pretty_app_name(&w.exe_path, &w.title, &w.aumid);
                let wt = w.title.trim();
                fg_name = if !wt.is_empty() && !wt.eq_ignore_ascii_case(&p) {
                    format!("{} - {}", p, wt)
                } else {
                    p
                };
                break;
            }
        }
        if fg_name.is_empty() {
            fg_name = "Desktop".to_string();
        }
        self.fg_full_title = fg_name.clone();
        self.fg_app_title = truncate_title(&fg_name, 22);

        let mut pinned_slots: Vec<DockSlotKind> = Vec::with_capacity(cfg.mac_dock_pinned.len());
        let mut matched_win_hwnds: HashSet<isize> = HashSet::new();

        for app in &cfg.mac_dock_pinned {
            let target_exe = self.resolve_shortcut_target(&app.path);
            let mut attached: Vec<WinEntry> = Vec::new();
            for w in &running_wins {
                if window_matches_pinned(&app.name, &app.path, &target_exe, w) {
                    let wkey = canonical_exe_name(&w.exe_path);
                    // For Windows File Explorer, never attach invisible/desktop shell frames
                    if wkey == "explorer.exe" && !is_explorer_folder_window(w.hwnd) {
                        continue;
                    }
                    attached.push(w.clone());
                    matched_win_hwnds.insert(w.hwnd.0 as isize);
                }
            }
            if !attached.is_empty() {
                let key = format!("pin:{}", app.path.to_ascii_lowercase());
                self.bounce_until.remove(&key);
                attached.sort_by_key(|w| w.hwnd.0 as usize);
            }
            pinned_slots.push(DockSlotKind::PinnedApp {
                name: app.name.clone(),
                path: app.path.clone(),
                windows: attached,
            });
        }

        // Group remaining running (opened & not pinned) windows on the right side of divider
        let mut group_order: Vec<String> = Vec::new();
        let mut groups: HashMap<String, (String, String, Vec<WinEntry>)> = HashMap::new();
        for w in running_wins {
            if matched_win_hwnds.contains(&(w.hwnd.0 as isize)) {
                continue;
            }
            let exe_name = canonical_exe_name(&w.exe_path);
            if exe_name == "explorer.exe" && !is_explorer_folder_window(w.hwnd) {
                continue;
            }
            let exe_low = w.exe_path.to_ascii_lowercase();
            let group_key = if !w.aumid.is_empty()
                && (w.exe_path.is_empty()
                    || exe_low.starts_with("shell:")
                    || exe_low.contains("\\windowsapps\\")
                    || exe_name == "applicationframehost.exe")
            {
                format!("aumid:{}", w.aumid.to_ascii_lowercase())
            } else if !w.exe_path.is_empty() {
                exe_low
            } else {
                format!("hwnd:{}", w.hwnd.0 as isize)
            };
            if let Some(entry) = groups.get_mut(&group_key) {
                entry.2.push(w);
            } else {
                let disp = pretty_app_name(&w.exe_path, &w.title, &w.aumid);
                let launch = if !w.aumid.is_empty()
                    && (w.exe_path.is_empty()
                        || w.exe_path.to_ascii_lowercase().contains("\\windowsapps\\")
                        || w.exe_path.to_ascii_lowercase().starts_with("shell:"))
                {
                    format!("shell:AppsFolder\\{}", w.aumid)
                } else {
                    w.exe_path.clone()
                };
                group_order.push(group_key.clone());
                groups.insert(group_key, (disp, launch, vec![w]));
            }
        }

        self.stable_group_order.retain(|k| groups.contains_key(k));
        for k in group_order {
            if !self.stable_group_order.contains(&k) {
                self.stable_group_order.push(k);
            }
        }
        let mut running_slots: Vec<DockSlotKind> = Vec::with_capacity(self.stable_group_order.len());
        for k in &self.stable_group_order {
            if let Some((name, exe_path, mut windows)) = groups.remove(k) {
                windows.sort_by_key(|w| w.hwnd.0 as usize);
                running_slots.push(DockSlotKind::RunningApp {
                    key: k.clone(),
                    name,
                    exe_path,
                    windows,
                });
            }
        }

        if !cfg.w_apps.show_icons && !cfg.w_apps.show_cards && !cfg.w_apps.show_indicators {
            pinned_slots.clear();
            running_slots.clear();
        }

        let spec = resolve_shelf_spec(cfg);
        let (time_str, date_str, hour24, minute, second) = format_dock_clock(cfg);
        let clock_slot = DockSlotKind::ClockModule {
            time_str,
            date_str,
            hour24,
            minute,
            second,
        };
        let is_vertical = matches!(cfg.mac_dock_edge, MacDockEdge::Left | MacDockEdge::Right);
        let show_div = cfg.w_apps.show_separators;
        let show_clock = !cfg.w_clock.is_all_elements_off();
        let show_tray = !cfg.w_tray.is_all_elements_off();
        let show_sliver = cfg.w_util.show_desktop_sliver && !cfg.w_util.is_all_elements_off();
        let has_right_cluster = show_tray
            || show_clock
            || cfg.w_util.show_trash
            || cfg.w_util.show_studio
            || show_sliver;
        let mut new_slots: Vec<DockSlotKind> =
            Vec::with_capacity(pinned_slots.len() + running_slots.len() + 10);

        match spec.layout {
            DockLayoutKind::MultiIslandBar => {
                // Island 1: System Navigation Trio + Live Active App Status
                if cfg.w_util.show_start {
                    new_slots.push(DockSlotKind::StartLauncher);
                }
                if cfg.w_util.show_search {
                    new_slots.push(DockSlotKind::SearchButton);
                }
                if cfg.w_util.show_taskview {
                    new_slots.push(DockSlotKind::TaskViewButton);
                }
                if !is_vertical && cfg.w_hud.show_active_app {
                    new_slots.push(DockSlotKind::ActiveAppChip {
                        title: truncate_title(&self.fg_app_title, 15),
                    });
                }
                if !pinned_slots.is_empty() || !running_slots.is_empty() {
                    if !new_slots.is_empty() {
                        new_slots.push(DockSlotKind::IslandGap);
                    }
                    // Island 2: Pinned & Active Windows Hub
                    let has_pinned = !pinned_slots.is_empty();
                    new_slots.extend(pinned_slots);
                    if !running_slots.is_empty() {
                        if show_div && has_pinned {
                            new_slots.push(DockSlotKind::Divider);
                        }
                        new_slots.extend(running_slots);
                    }
                }
                if show_tray || show_clock || cfg.w_util.show_trash || cfg.w_util.show_studio {
                    if !new_slots.is_empty() {
                        new_slots.push(DockSlotKind::IslandGap);
                    }
                    // Island 3: Hidden Tray Chevron + Windows Tray + Clock + Trash + Studio
                    if show_tray {
                        new_slots.push(DockSlotKind::HiddenTrayChevron);
                        new_slots.push(DockSlotKind::TrayStatusCluster);
                    }
                    if show_clock {
                        new_slots.push(clock_slot);
                    }
                    if cfg.w_util.show_trash {
                        new_slots.push(DockSlotKind::TrashBin {
                            items: self.trash_items,
                        });
                    }
                    if cfg.w_util.show_studio {
                        new_slots.push(DockSlotKind::StudioLauncher);
                    }
                }
            }
            DockLayoutKind::FullEdgeTaskbar => {
                // Edge-to-edge native Windows taskbar with navigation trio, wide window buttons, tray & clock
                if cfg.w_util.show_start {
                    new_slots.push(DockSlotKind::StartLauncher);
                }
                if cfg.w_util.show_search {
                    new_slots.push(DockSlotKind::SearchButton);
                }
                if cfg.w_util.show_taskview {
                    new_slots.push(DockSlotKind::TaskViewButton);
                }
                let has_pinned = !pinned_slots.is_empty();
                new_slots.extend(pinned_slots);
                if !running_slots.is_empty() {
                    if show_div && has_pinned {
                        new_slots.push(DockSlotKind::Divider);
                    }
                    new_slots.extend(running_slots);
                }
                if !new_slots.is_empty() && has_right_cluster {
                    new_slots.push(DockSlotKind::FlexSpacer);
                    if show_div {
                        new_slots.push(DockSlotKind::EndDivider);
                    }
                }
                if cfg.w_util.show_trash {
                    new_slots.push(DockSlotKind::TrashBin {
                        items: self.trash_items,
                    });
                }
                if cfg.w_util.show_studio {
                    new_slots.push(DockSlotKind::StudioLauncher);
                }
                if show_tray {
                    new_slots.push(DockSlotKind::HiddenTrayChevron);
                    new_slots.push(DockSlotKind::TrayStatusCluster);
                }
                if show_clock {
                    new_slots.push(clock_slot);
                }
                if show_sliver {
                    new_slots.push(DockSlotKind::ShowDesktopSliver);
                }
            }
            DockLayoutKind::IndependentOrbs => {
                // Barless standalone orbs/jewels floating over desktop + Left Pinned | Right Opened + Tray & Clock
                if cfg.w_util.show_start {
                    new_slots.push(DockSlotKind::StartLauncher);
                }
                let has_pinned = !pinned_slots.is_empty();
                new_slots.extend(pinned_slots);
                if !running_slots.is_empty() {
                    if show_div && has_pinned {
                        new_slots.push(DockSlotKind::Divider);
                    }
                    new_slots.extend(running_slots);
                }
                if show_div && !new_slots.is_empty() && has_right_cluster {
                    new_slots.push(DockSlotKind::EndDivider);
                }
                if show_tray {
                    new_slots.push(DockSlotKind::HiddenTrayChevron);
                    new_slots.push(DockSlotKind::TrayStatusCluster);
                }
                if show_clock {
                    new_slots.push(clock_slot);
                }
                if cfg.w_util.show_trash {
                    new_slots.push(DockSlotKind::TrashBin {
                        items: self.trash_items,
                    });
                }
                if cfg.w_util.show_studio {
                    new_slots.push(DockSlotKind::StudioLauncher);
                }
            }
            DockLayoutKind::InlineMorphRail => {
                if cfg.w_util.show_start {
                    new_slots.push(DockSlotKind::StartLauncher);
                }
                if cfg.w_util.show_search {
                    new_slots.push(DockSlotKind::SearchButton);
                }
                let has_pinned = !pinned_slots.is_empty();
                new_slots.extend(pinned_slots);
                if !running_slots.is_empty() {
                    if show_div && has_pinned {
                        new_slots.push(DockSlotKind::Divider);
                    }
                    new_slots.extend(running_slots);
                }
                if show_div && !new_slots.is_empty() && has_right_cluster {
                    new_slots.push(DockSlotKind::EndDivider);
                }
                if show_tray {
                    new_slots.push(DockSlotKind::HiddenTrayChevron);
                    new_slots.push(DockSlotKind::TrayStatusCluster);
                }
                if show_clock {
                    new_slots.push(clock_slot);
                }
                if cfg.w_util.show_trash {
                    new_slots.push(DockSlotKind::TrashBin {
                        items: self.trash_items,
                    });
                }
                if cfg.w_util.show_studio {
                    new_slots.push(DockSlotKind::StudioLauncher);
                }
            }
            DockLayoutKind::HudConsoleDeck => {
                if cfg.w_util.show_start {
                    new_slots.push(DockSlotKind::StartLauncher);
                }
                if cfg.w_util.show_search {
                    new_slots.push(DockSlotKind::SearchButton);
                }
                if cfg.w_util.show_taskview {
                    new_slots.push(DockSlotKind::TaskViewButton);
                }
                let has_pinned = !pinned_slots.is_empty();
                new_slots.extend(pinned_slots);
                if !running_slots.is_empty() {
                    if show_div && has_pinned {
                        new_slots.push(DockSlotKind::Divider);
                    }
                    new_slots.extend(running_slots);
                }
                if show_div && !new_slots.is_empty() && has_right_cluster {
                    new_slots.push(DockSlotKind::EndDivider);
                }
                if show_tray {
                    new_slots.push(DockSlotKind::HiddenTrayChevron);
                    new_slots.push(DockSlotKind::TrayStatusCluster);
                }
                if show_clock {
                    new_slots.push(clock_slot);
                }
                if cfg.w_util.show_trash {
                    new_slots.push(DockSlotKind::TrashBin {
                        items: self.trash_items,
                    });
                }
                if cfg.w_util.show_studio {
                    new_slots.push(DockSlotKind::StudioLauncher);
                }
                if show_sliver {
                    new_slots.push(DockSlotKind::ShowDesktopSliver);
                }
            }
            DockLayoutKind::FishEyeShelf => {
                if cfg.w_util.show_start {
                    new_slots.push(DockSlotKind::StartLauncher);
                }
                let has_pinned = !pinned_slots.is_empty();
                new_slots.extend(pinned_slots);
                if !running_slots.is_empty() {
                    if show_div && has_pinned {
                        new_slots.push(DockSlotKind::Divider);
                    }
                    new_slots.extend(running_slots);
                }
                if show_div && !new_slots.is_empty() && has_right_cluster {
                    new_slots.push(DockSlotKind::EndDivider);
                }
                if show_tray {
                    new_slots.push(DockSlotKind::HiddenTrayChevron);
                    new_slots.push(DockSlotKind::TrayStatusCluster);
                }
                if show_clock {
                    new_slots.push(clock_slot);
                }
                if cfg.w_util.show_trash {
                    new_slots.push(DockSlotKind::TrashBin {
                        items: self.trash_items,
                    });
                }
                if cfg.w_util.show_studio {
                    new_slots.push(DockSlotKind::StudioLauncher);
                }
            }
        }
        let changed = self.slots != new_slots;
        if changed || self.slot_keys.len() != new_slots.len() {
            self.slot_keys = new_slots.iter().map(slot_id_key).collect();
        }
        self.slots = new_slots;
        changed
    }

    pub fn tick(&mut self, cfg: &AppConfig) {
        if !cfg.mac_dock_enabled {
            return;
        }
        if self.panes.is_empty() {
            self.reconcile_monitors(true);
            if self.panes.is_empty() {
                return;
            }
        }
        self.telemetry_visible = cfg.w_hud.mode == HudWingMode::AlwaysEnabled
            || (cfg.w_hud.mode == HudWingMode::InheritTheme
                && resolve_shelf_spec(cfg).layout == DockLayoutKind::HudConsoleDeck)
            || cfg.w_tray.net_style == TrayNetMeterStyle::LiveThroughputText;
        let now = Instant::now();
        let dt = (now - self.last_tick).as_secs_f32().clamp(0.004, 0.05);
        self.last_tick = now;
        let has_flashes = !self.flashing_hwnds.is_empty();
        let any_hover = cfg.tb_hover_react && self.panes.iter().any(|p| p.hover_mix > 0.01);
        let ambient_live = is_ambient_animated_cfg(cfg);
        if any_hover || ambient_live || has_flashes {
            self.dock_anim_phase = (self.dock_anim_phase + dt * 1.45) % 628.3185;
            if ambient_live || has_flashes {
                let min_gap = Duration::from_millis(
                    cfg.anim_frame_ms().saturating_sub(2).max(6) as u64,
                );
                if now.duration_since(self.last_ambient_paint) >= min_gap {
                    self.last_ambient_paint = now;
                    for pane in &mut self.panes {
                        if pane.window_shown && pane.slide_t > 0.25 {
                            pane.dirty_paint = true;
                        }
                    }
                }
            }
        }
        if let Some(until) = self.vol_osd_until {
            if until <= now {
                self.vol_osd_until = None;
                self.mark_dirty_paint();
            }
        }
        let bat_low = self.tray_bat_pct.map(|p| p <= 20).unwrap_or(false) && !self.tray_bat_charging;
        if self.tray_anim.step(
            dt,
            self.tray_vol_muted,
            self.tray_bat_charging,
            bat_low,
            cfg.tb_ambient_anim && cfg.w_tray.battery_style != TrayBatteryMeterStyle::Hidden,
        ) {
            for pane in &mut self.panes {
                if pane.window_shown && pane.slide_t > 0.25 {
                    pane.dirty_paint = true;
                }
            }
        }

        let cur_bucket = clock_bucket(cfg);
        if self.dirty_windows || now.duration_since(self.last_full_sync) >= Duration::from_millis(4000) {
            let was_dirty = self.dirty_windows;
            self.reconcile_monitors(true);
            let slots_changed = self.rebuild_slots(cfg);
            if was_dirty || slots_changed {
                self.mark_dirty_paint();
            }
        } else if cur_bucket != self.last_clock_bucket {
            self.last_clock_bucket = cur_bucket;
            let (time_str, date_str, hour24, minute, second) = format_dock_clock(cfg);
            for slot in &mut self.slots {
                if let DockSlotKind::ClockModule {
                    time_str: ref mut ts,
                    date_str: ref mut ds,
                    hour24: ref mut h,
                    minute: ref mut m,
                    second: ref mut s,
                } = slot
                {
                    *ts = time_str.clone();
                    *ds = date_str.clone();
                    *h = hour24;
                    *m = minute;
                    *s = second;
                }
            }
            self.mark_dirty_paint();
        }

        let mut pt = POINT::default();
        let got_cursor = unsafe { GetCursorPos(&mut pt).is_ok() };

        // Purge expired bounce/pop timers cleanly after panes have sampled them on previous frames
        let mut panes = std::mem::take(&mut self.panes);
        for pane in &mut panes {
            self.tick_pane(pane, cfg, pt, got_cursor, dt, now);
        }
        self.panes = panes;
        self.bounce_until.retain(|_, &mut t| t > now);
        self.pop_until.retain(|_, &mut t| t > now);
    }

    fn tick_pane(
        &mut self,
        pane: &mut DockMonitorPane,
        cfg: &AppConfig,
        pt: POINT,
        got_cursor: bool,
        dt: f32,
        now: Instant,
    ) {
        if pane.hwnd.0.is_null() {
            return;
        }
        pane.update_dpi();
        let u = pane.dpi_u;
        let (spec, is_vertical, base_sz, shelf_cross, edge_gap, hud_wing_h) =
            dock_cross_metrics(cfg, u);
        let gap = match spec.layout {
            DockLayoutKind::IndependentOrbs => (11.0 * u).max(8.0),
            DockLayoutKind::FullEdgeTaskbar => (5.0 * u).max(4.0),
            DockLayoutKind::InlineMorphRail => (6.0 * u).max(4.0),
            DockLayoutKind::HudConsoleDeck => (9.0 * u).max(6.0),
            DockLayoutKind::MultiIslandBar => (7.0 * u).max(5.0),
            DockLayoutKind::FishEyeShelf => (8.0 * u).max(5.0),
        };
        let pad_span = match spec.layout {
            DockLayoutKind::IndependentOrbs => 10.0 * u,
            DockLayoutKind::FullEdgeTaskbar => 8.0 * u,
            _ => 12.0 * u,
        };
        let right_pad_span = if spec.layout == DockLayoutKind::FullEdgeTaskbar {
            2.0 * u
        } else if spec.layout == DockLayoutKind::HudConsoleDeck {
            16.0 * u
        } else {
            pad_span
        };
        let max_scale = if !cfg.tb_hover_react {
            1.0
        } else {
            match spec.layout {
                DockLayoutKind::FishEyeShelf => (cfg.mac_dock_mag_pct as f32 / 100.0).clamp(1.0, 1.85),
                DockLayoutKind::MultiIslandBar => 1.14,
                DockLayoutKind::IndependentOrbs => 1.22,
                DockLayoutKind::HudConsoleDeck => 1.06,
                DockLayoutKind::FullEdgeTaskbar | DockLayoutKind::InlineMorphRail => 1.0,
            }
        };
        let mag_radius = base_sz * 3.05;

        let docked_open = pane.window_shown && pane.slide_t >= 0.12;
        let in_band = got_cursor
            && docked_open
            && pane.is_point_in_dock_band(pt, cfg.mac_dock_edge, spec.layout, hud_wing_h, u);
        let thumb_owned = self.thumb_owner_hwnd == pane.hwnd;
        let in_thumb = thumb_owned && got_cursor && docked_open && self.thumb_popup.contains_point(pt);
        let in_bridge = thumb_owned && got_cursor && docked_open && self.thumb_popup.in_bridge(pt);
        if got_cursor {
            pane.cursor_in_dock = in_band;
            if in_band || in_thumb || in_bridge {
                pane.last_inside_time = Some(now);
            }
        }

        let game_fs = cfg.game_mode_pause && is_fullscreen_game_on_monitor(pane.mon_rect);
        let on_edge = !game_fs
            && got_cursor
            && is_point_on_screen_edge(
                pt,
                pane.mon_rect,
                cfg.mac_dock_edge,
                (4.0 * u).round().max(4.0) as i32,
            );
        let want_visible = if game_fs {
            false
        } else {
            match cfg.mac_dock_hide {
                MacDockHideMode::AlwaysVisible => true,
                MacDockHideMode::StealthHide => false,
                MacDockHideMode::HoverReveal => {
                    self.menu_open || on_edge || (docked_open && (in_band || in_thumb || in_bridge))
                }
                MacDockHideMode::SmartAutoHide => {
                    !has_maximized_or_fullscreen_on_monitor(pane.mon_rect)
                        || self.menu_open
                        || on_edge
                        || (docked_open && (in_band || in_thumb || in_bridge))
                }
            }
        };

        let mut lerping = false;
        let target_slide = if want_visible { 1.0 } else { 0.0 };
        if !pane.dock_inited || game_fs {
            pane.slide_t = target_slide;
            pane.dock_inited = true;
            if game_fs {
                pane.dirty_paint = false;
            }
        } else if (pane.slide_t - target_slide).abs() > 0.002 {
            let k = 1.0 - (-dt * 14.0).exp();
            pane.slide_t += (target_slide - pane.slide_t) * k;
            if (pane.slide_t - target_slide).abs() < 0.006 {
                pane.slide_t = target_slide;
            }
            pane.dirty_paint = true;
            lerping = true;
        } else {
            pane.slide_t = target_slide;
        }

        if pane.slide_t <= 0.005 && !want_visible {
            if thumb_owned {
                self.thumb_popup.hide();
                self.thumb_owner_hwnd = HWND::default();
            }
            if self.hover_dwell_pane == pane.hwnd {
                self.hover_dwell_pane = HWND::default();
                self.hover_dwell_since = None;
                self.hover_dwell_key.clear();
            }
            if pane.window_shown {
                unsafe {
                    let _ = ShowWindow(pane.hwnd, SW_HIDE);
                }
                pane.window_shown = false;
            }
            pane.hover_mix = 0.0;
            pane.hovered_idx = None;
            pane.motion_active = false;
            return;
        }

        let target_hover = if cfg.tb_hover_react
            && (pane.cursor_in_dock || in_thumb || in_bridge)
            && pane.slide_t > 0.45
        {
            1.0
        } else {
            0.0
        };
        if (pane.hover_mix - target_hover).abs() > 0.003 {
            let k = 1.0 - (-dt * 14.5).exp();
            pane.hover_mix += (target_hover - pane.hover_mix) * k;
            if (pane.hover_mix - target_hover).abs() < 0.005 {
                pane.hover_mix = target_hover;
            }
            pane.dirty_paint = true;
            lerping = true;
        } else {
            pane.hover_mix = target_hover;
        }

        if pane.click_ripple_t < 1.0 {
            pane.click_ripple_t = (pane.click_ripple_t + dt * 1.25).min(1.0);
            pane.dirty_paint = true;
            lerping = true;
        }

        let mon_w = (pane.mon_rect.right - pane.mon_rect.left) as f32;
        let mon_h = (pane.mon_rect.bottom - pane.mon_rect.top) as f32;
        let mon_span = if is_vertical { mon_h } else { mon_w };
        let headroom = base_sz * (max_scale - 1.0) + hud_wing_h + 46.0 * u;
        let tip_cross = if is_vertical { 164.0 * u } else { 42.0 * u };

        let fg_hwnd = unsafe { GetForegroundWindow() };
        let divider_w = 10.0 * u;
        let air_gap = 18.0 * u;
        let island_gap_w = pad_span + air_gap + pad_span;

        let base_slot_width = |s: &DockSlotKind| -> (f32, bool, bool) {
            match s {
                DockSlotKind::Divider | DockSlotKind::EndDivider => (divider_w, true, false),
                DockSlotKind::IslandGap => (island_gap_w, false, true),
                DockSlotKind::FlexSpacer => (8.0 * u, false, true),
                DockSlotKind::SearchButton | DockSlotKind::TaskViewButton => {
                    (base_sz * 0.86, false, false)
                }
                DockSlotKind::HiddenTrayChevron => {
                    if is_vertical {
                        if spec.layout == DockLayoutKind::IndependentOrbs {
                            (26.0 * u, false, false)
                        } else {
                            (24.0 * u, false, false)
                        }
                    } else if spec.layout == DockLayoutKind::IndependentOrbs {
                        (28.0 * u, false, false)
                    } else {
                        (22.0 * u, false, false)
                    }
                }
                DockSlotKind::TrayStatusCluster => {
                    let span = tray_cluster_slot_span_cfg(
                        is_vertical,
                        base_sz,
                        self.tray_bat_pct.is_some(),
                        cfg,
                        u,
                    );
                    (span, false, false)
                }
                DockSlotKind::ActiveAppChip { .. } => {
                    if is_vertical { (base_sz, false, false) } else { (114.0 * u, false, false) }
                }
                DockSlotKind::ClockModule { .. } => {
                    if is_vertical {
                        (base_sz, false, false)
                    } else {
                        let base_clk = match cfg.w_clock.engine {
                            DockClockEngine::SingleLinePill => 118.0 * u,
                            DockClockEngine::AnalogChronograph => 98.0 * u,
                            DockClockEngine::TerminalMatrix => 88.0 * u,
                            _ => 82.0 * u,
                        };
                        let sec_extra = if cfg.w_clock.show_seconds { 14.0 * u } else { 0.0 };
                        (base_clk + sec_extra, false, false)
                    }
                }
                DockSlotKind::ShowDesktopSliver => {
                    if is_vertical { (12.0 * u, false, false) } else { (10.0 * u, false, false) }
                }
                DockSlotKind::PinnedApp { windows, .. }
                | DockSlotKind::RunningApp { windows, .. } => {
                    let is_act = windows.iter().any(|w| w.hwnd == fg_hwnd);
                    let want_wide = !is_vertical
                        && !windows.is_empty()
                        && match cfg.w_apps.label_mode {
                            DockRunningLabelMode::AllRunningApps => true,
                            DockRunningLabelMode::ActiveAppOnly => is_act,
                            DockRunningLabelMode::IconOnly | DockRunningLabelMode::HoverExpandMorph => false,
                            DockRunningLabelMode::InheritTheme => spec.layout == DockLayoutKind::FullEdgeTaskbar,
                        };
                    if want_wide {
                        (142.0 * u, false, false)
                    } else {
                        (base_sz, false, false)
                    }
                }
                _ => (base_sz, false, false),
            }
        };

        // Helper closure check: add normal inter-slot gap only between adjacent real/divider items
        let has_gap_after = |idx: usize, slots: &[DockSlotKind]| -> bool {
            let Some(next) = slots.get(idx + 1) else {
                return false;
            };
            !matches!(
                slots[idx],
                DockSlotKind::IslandGap | DockSlotKind::FlexSpacer
            ) && !matches!(next, DockSlotKind::IslandGap | DockSlotKind::FlexSpacer)
        };

        // Step 1: Compute resting centers centered around 0
        let mut rest_widths = Vec::with_capacity(self.slots.len());
        let mut rest_is_div = Vec::with_capacity(self.slots.len());
        let mut rest_is_sp = Vec::with_capacity(self.slots.len());
        let mut rest_total = pad_span + right_pad_span;
        let mut flex_idx: Option<usize> = None;
        for (idx, s) in self.slots.iter().enumerate() {
            let (w, is_d, is_s) = base_slot_width(s);
            if matches!(s, DockSlotKind::FlexSpacer) {
                flex_idx = Some(idx);
            }
            rest_widths.push(w);
            rest_is_div.push(is_d);
            rest_is_sp.push(is_s);
            rest_total += w;
            if has_gap_after(idx, &self.slots) {
                rest_total += gap;
            }
        }
        if let Some(fi) = flex_idx {
            let extra = (mon_span - rest_total).max(0.0);
            rest_widths[fi] += extra;
            rest_total += extra;
        }
        pane.rest_span = rest_total;

        let floating_centered = spec.layout != DockLayoutKind::FullEdgeTaskbar;
        let mon_w_i = (pane.mon_rect.right - pane.mon_rect.left).max(64);
        let mon_h_i = (pane.mon_rect.bottom - pane.mon_rect.top).max(64);
        let mon_span_i = if is_vertical { mon_h_i } else { mon_w_i };
        let canvas_cross = (edge_gap + shelf_cross + headroom + tip_cross).round() as i32;
        let canvas_span = if floating_centered {
            let extra_margin = if is_vertical { 140.0 * u } else { 220.0 * u };
            let raw = ((rest_total * max_scale.max(1.0) + extra_margin).ceil() as i32)
                .clamp(448.min(mon_span_i), mon_span_i);
            let bucket = (((raw + 127) / 128) * 128).min(mon_span_i);
            let kept = if pane.last_canvas_span >= bucket && pane.last_canvas_span <= bucket + 256 {
                pane.last_canvas_span.min(mon_span_i)
            } else {
                bucket
            };
            pane.last_canvas_span = kept;
            kept
        } else {
            pane.last_canvas_span = mon_span_i;
            mon_span_i
        };
        let canvas_w = if is_vertical { canvas_cross } else { canvas_span };
        let canvas_h = if is_vertical { canvas_span } else { canvas_cross };
        let canvas_l = match cfg.mac_dock_edge {
            MacDockEdge::Right => pane.mon_rect.right - canvas_w,
            MacDockEdge::Left => pane.mon_rect.left,
            MacDockEdge::Bottom | MacDockEdge::Top => {
                if floating_centered {
                    pane.mon_rect.left + (mon_w_i - canvas_w) / 2
                } else {
                    pane.mon_rect.left
                }
            }
        };
        let canvas_t = match cfg.mac_dock_edge {
            MacDockEdge::Bottom => pane.mon_rect.bottom - canvas_h,
            MacDockEdge::Top => pane.mon_rect.top,
            MacDockEdge::Left | MacDockEdge::Right => {
                if floating_centered {
                    pane.mon_rect.top + (mon_h_i - canvas_h) / 2
                } else {
                    pane.mon_rect.top
                }
            }
        };
        pane.canvas_rect = RECT {
            left: canvas_l,
            top: canvas_t,
            right: canvas_l + canvas_w,
            bottom: canvas_t + canvas_h,
        };

        let mut rest_centers = Vec::with_capacity(self.slots.len());
        let mut cursor_r = -rest_total * 0.5 + pad_span;
        for (idx, &w) in rest_widths.iter().enumerate() {
            rest_centers.push(cursor_r + w * 0.5);
            cursor_r += w;
            if has_gap_after(idx, &self.slots) {
                cursor_r += gap;
            }
        }

        // Pointer in canvas space and rest space
        let dock_center_axis = if is_vertical {
            pane.mon_rect.top as f32 + mon_h * 0.5
        } else {
            pane.mon_rect.left as f32 + mon_w * 0.5
        };
        let cursor_axis_raw = if is_vertical { pt.y as f32 } else { pt.x as f32 };
        let cursor_local_main = if is_vertical {
            (pt.y - pane.canvas_rect.top) as f32
        } else {
            (pt.x - pane.canvas_rect.left) as f32
        };
        let mouse_rel = cursor_axis_raw - dock_center_axis;
        let scale_ratio = if pane.last_dock_span > 1.0 {
            pane.rest_span / pane.last_dock_span
        } else {
            1.0
        };
        let cursor_rest = mouse_rel * scale_ratio;

        // Precompute hovered index cleanly for non-fisheye layouts using live slot spans
        let prev_layout_matches = pane.layout.len() == self.slots.len();
        let mut non_wave_hover: Option<usize> = None;
        if spec.layout != DockLayoutKind::FishEyeShelf && pane.cursor_in_dock {
            if prev_layout_matches {
                for (idx, prev) in pane.layout.iter().enumerate() {
                    if prev.is_divider || prev.is_spacer {
                        continue;
                    }
                    let (lo, hi) = if is_vertical {
                        (prev.slot_rect.top - gap * 0.45, prev.slot_rect.bottom + gap * 0.45)
                    } else {
                        (prev.slot_rect.left - gap * 0.45, prev.slot_rect.right + gap * 0.45)
                    };
                    if cursor_local_main >= lo && cursor_local_main <= hi {
                        non_wave_hover = Some(idx);
                        break;
                    }
                }
            }
            if non_wave_hover.is_none() {
                let mut best: Option<(usize, f32)> = None;
                for idx in 0..self.slots.len() {
                    if rest_is_div[idx] || rest_is_sp[idx] {
                        continue;
                    }
                    let d = (rest_centers[idx] - cursor_rest).abs();
                    if d < rest_widths[idx] * 0.60 + gap * 0.5 {
                        match best {
                            None => best = Some((idx, d)),
                            Some((_, bd)) if d < bd => best = Some((idx, d)),
                            _ => {}
                        }
                    }
                }
                non_wave_hover = best.map(|(i, _)| i);
            }
        }

        // Step 2: Compute smoothed scale, inline expansion, bounce & layout slots
        let mut slot_widths = Vec::with_capacity(self.slots.len());
        let mut icon_sizes = Vec::with_capacity(self.slots.len());
        let mut inline_alphas = Vec::with_capacity(self.slots.len());
        let mut lifts = Vec::with_capacity(self.slots.len());
        let mut best_wave_hover: Option<(usize, f32)> = None;
        let mut used_fixed_without_flex = pad_span + right_pad_span;

        for (idx, s) in self.slots.iter().enumerate() {
            if rest_is_div[idx] || rest_is_sp[idx] {
                let rw = if matches!(s, DockSlotKind::FlexSpacer) {
                    8.0 * u
                } else {
                    rest_widths[idx]
                };
                slot_widths.push(rw);
                icon_sizes.push(rw);
                inline_alphas.push(0.0);
                lifts.push(0.0);
                if !matches!(s, DockSlotKind::FlexSpacer) {
                    used_fixed_without_flex += rw;
                }
                if has_gap_after(idx, &self.slots) {
                    used_fixed_without_flex += gap;
                }
                continue;
            }
            let id_key: &str = self
                .slot_keys
                .get(idx)
                .map(String::as_str)
                .unwrap_or("");
            let rw = rest_widths[idx];
            let is_hov = if spec.layout == DockLayoutKind::FishEyeShelf {
                let d = (rest_centers[idx] - cursor_rest).abs();
                if pane.cursor_in_dock && d < (base_sz * 0.85).max(rw * 0.55) {
                    match best_wave_hover {
                        None => best_wave_hover = Some((idx, d)),
                        Some((_, bd)) if d < bd => best_wave_hover = Some((idx, d)),
                        _ => {}
                    }
                }
                false
            } else {
                non_wave_hover == Some(idx)
            };

            let steady_wide = matches!(
                s,
                DockSlotKind::ClockModule { .. }
                    | DockSlotKind::TrayStatusCluster
                    | DockSlotKind::HiddenTrayChevron
            );
            let tgt_s = if steady_wide {
                1.0
            } else {
                match spec.layout {
                    DockLayoutKind::FishEyeShelf => {
                        let d = (rest_centers[idx] - cursor_rest).abs();
                        if pane.hover_mix > 0.01 && max_scale > 1.005 && d < mag_radius {
                            let c = (std::f32::consts::FRAC_PI_2 * (d / mag_radius)).cos();
                            1.0 + pane.hover_mix * (max_scale - 1.0) * c * c
                        } else {
                            1.0
                        }
                    }
                    DockLayoutKind::MultiIslandBar
                    | DockLayoutKind::IndependentOrbs
                    | DockLayoutKind::HudConsoleDeck => {
                        if is_hov { max_scale } else { 1.0 }
                    }
                    DockLayoutKind::FullEdgeTaskbar | DockLayoutKind::InlineMorphRail => 1.0,
                }
            };

            let prev_s = pane.cur_scales.get(id_key).copied().unwrap_or(1.0);
            let k_s = 1.0 - (-dt * 18.0).exp();
            let mut sm_s = prev_s + (tgt_s - prev_s) * k_s;
            if (sm_s - tgt_s).abs() < 0.002 {
                sm_s = tgt_s;
            } else {
                pane.dirty_paint = true;
                lerping = true;
            }
            if pane.pressed_idx == Some(idx) {
                sm_s *= if spec.layout == DockLayoutKind::FishEyeShelf {
                    0.92
                } else {
                    0.95
                };
            }
            if (sm_s - 1.0).abs() < 0.0005 {
                if !id_key.is_empty() {
                    pane.cur_scales.remove(id_key);
                }
            } else if !id_key.is_empty() {
                if let Some(v) = pane.cur_scales.get_mut(id_key) {
                    *v = sm_s;
                } else {
                    pane.cur_scales.insert(id_key.to_string(), sm_s);
                }
            }

            // Inline horizontal label expansion for InlineMorphRail or HoverExpandMorph override
            let morph_capable = !is_vertical
                && cfg.tb_hover_react
                && (cfg.w_apps.label_mode == DockRunningLabelMode::HoverExpandMorph
                    || (cfg.w_apps.label_mode == DockRunningLabelMode::InheritTheme
                        && spec.layout == DockLayoutKind::InlineMorphRail))
                && matches!(
                    s,
                    DockSlotKind::StartLauncher
                        | DockSlotKind::PinnedApp { .. }
                        | DockSlotKind::RunningApp { .. }
                        | DockSlotKind::TrashBin { .. }
                        | DockSlotKind::StudioLauncher
                );
            let tgt_ex = if morph_capable && is_hov { 1.0 } else { 0.0 };
            let prev_ex = pane.slot_expands.get(id_key).copied().unwrap_or(0.0);
            let k_ex = 1.0 - (-dt * 16.0).exp();
            let mut sm_ex = prev_ex + (tgt_ex - prev_ex) * k_ex;
            if (sm_ex - tgt_ex).abs() < 0.004 {
                sm_ex = tgt_ex;
            } else {
                pane.dirty_paint = true;
                lerping = true;
            }
            if sm_ex <= 0.0005 {
                if !id_key.is_empty() {
                    pane.slot_expands.remove(id_key);
                }
            } else if !id_key.is_empty() {
                if let Some(v) = pane.slot_expands.get_mut(id_key) {
                    *v = sm_ex;
                } else {
                    pane.slot_expands.insert(id_key.to_string(), sm_ex);
                }
            }

            let mut lift_px = match spec.layout {
                DockLayoutKind::FishEyeShelf if !steady_wide => {
                    (sm_s - 1.0).max(0.0)
                        * base_sz
                        * if spec.mechanical_lift { 0.56 } else { 0.44 }
                }
                DockLayoutKind::IndependentOrbs if !steady_wide => {
                    let wave = if cfg.tb_theme == TaskbarTheme::BiolumAbyssDock {
                        ((self.dock_anim_phase * 1.15 + idx as f32 * 0.85).sin() * 0.5 + 0.5)
                            * 5.0
                            * u
                    } else {
                        0.0
                    };
                    wave + if is_hov { 3.5 * u } else { 0.0 }
                }
                _ => 0.0,
            };

            if cfg.mac_dock_bounce {
                if let Some(&until) = self.bounce_until.get(id_key) {
                    if until > now {
                        let rem = (until - now).as_secs_f32();
                        let t_el = (2.4 - rem).max(0.0);
                        let env = (rem / 2.4).clamp(0.0, 1.0);
                        lift_px += (t_el * 8.2).sin().abs() * 18.0 * u * env;
                        pane.dirty_paint = true;
                    }
                }
                if let Some(&until) = self.pop_until.get(id_key) {
                    if until > now {
                        let rem = (until - now).as_secs_f32();
                        let prog = (1.0 - rem / 0.18).clamp(0.0, 1.0);
                        lift_px += (prog * std::f32::consts::PI).sin() * 7.0 * u;
                        pane.dirty_paint = true;
                    }
                }
            }

            let expand_extra = 96.0 * u * sm_ex;
            let final_w = rw * sm_s + expand_extra;
            let ico_sz = if matches!(s, DockSlotKind::HiddenTrayChevron) {
                (base_sz * 0.50).clamp(15.0 * u, 19.0 * u)
            } else if !is_vertical && rw > base_sz * 1.35 {
                (base_sz * 0.72).clamp(20.0 * u, 30.0 * u)
            } else if spec.layout == DockLayoutKind::InlineMorphRail && !is_vertical {
                base_sz * 0.86
            } else {
                base_sz * sm_s
            };
            let label_alpha = if sm_ex > 0.03 {
                sm_ex
            } else if !is_vertical && rw > base_sz * 1.35 {
                1.0
            } else {
                0.0
            };

            slot_widths.push(final_w);
            icon_sizes.push(ico_sz);
            inline_alphas.push(label_alpha);
            lifts.push(lift_px);

            used_fixed_without_flex += final_w;
            if has_gap_after(idx, &self.slots) {
                used_fixed_without_flex += gap;
            }
        }

        if let Some(fi) = flex_idx {
            slot_widths[fi] = (mon_span - used_fixed_without_flex).max(6.0 * u);
        }

        let new_hov = if spec.layout == DockLayoutKind::FishEyeShelf {
            best_wave_hover.map(|(i, _)| i)
        } else {
            non_wave_hover
        };
        if new_hov != pane.hovered_idx {
            pane.hovered_idx = new_hov;
            pane.dirty_paint = true;
        }

        let mut dock_span = pad_span + right_pad_span;
        for (idx, &w) in slot_widths.iter().enumerate() {
            dock_span += w;
            if has_gap_after(idx, &self.slots) {
                dock_span += gap;
            }
        }
        let max_mon_span = if spec.layout == DockLayoutKind::FullEdgeTaskbar {
            mon_span
        } else {
            mon_span - 16.0 * u
        };
        let fit_k = if dock_span > max_mon_span && dock_span > 1.0 {
            max_mon_span / dock_span
        } else {
            1.0
        };
        dock_span *= fit_k;
        pane.last_dock_span = dock_span;

        let slide_s = smoothstep01(pane.slide_t);
        let slide_out = (1.0 - slide_s) * (shelf_cross + edge_gap + hud_wing_h + 12.0 * u);

        // Compute outer shelf local rect inside canvas
        let mut shelf_rect_f = match cfg.mac_dock_edge {
            MacDockEdge::Bottom => {
                let cx = canvas_w as f32 * 0.5;
                let sb = canvas_h as f32 - edge_gap + slide_out;
                let st = sb - shelf_cross;
                D2D_RECT_F {
                    left: cx - dock_span * 0.5,
                    top: st,
                    right: cx + dock_span * 0.5,
                    bottom: sb,
                }
            }
            MacDockEdge::Top => {
                let cx = canvas_w as f32 * 0.5;
                let st = edge_gap - slide_out;
                let sb = st + shelf_cross;
                D2D_RECT_F {
                    left: cx - dock_span * 0.5,
                    top: st,
                    right: cx + dock_span * 0.5,
                    bottom: sb,
                }
            }
            MacDockEdge::Left => {
                let cy = canvas_h as f32 * 0.5;
                let sl = edge_gap - slide_out;
                let sr = sl + shelf_cross;
                D2D_RECT_F {
                    left: sl,
                    top: cy - dock_span * 0.5,
                    right: sr,
                    bottom: cy + dock_span * 0.5,
                }
            }
            MacDockEdge::Right => {
                let cy = canvas_h as f32 * 0.5;
                let sr = canvas_w as f32 - edge_gap + slide_out;
                let sl = sr - shelf_cross;
                D2D_RECT_F {
                    left: sl,
                    top: cy - dock_span * 0.5,
                    right: sr,
                    bottom: cy + dock_span * 0.5,
                }
            }
        };

        let mut layout = Vec::with_capacity(self.slots.len());
        let mut span_pos = if is_vertical {
            shelf_rect_f.top + pad_span * fit_k
        } else {
            shelf_rect_f.left + pad_span * fit_k
        };
        let cross_bias = match spec.layout {
            DockLayoutKind::FishEyeShelf => 2.0 * u,
            DockLayoutKind::IndependentOrbs => 3.0 * u,
            _ => 0.0,
        };
        let base_center_cross = match cfg.mac_dock_edge {
            MacDockEdge::Bottom => (shelf_rect_f.top + shelf_rect_f.bottom) * 0.5 - cross_bias,
            MacDockEdge::Top => (shelf_rect_f.top + shelf_rect_f.bottom) * 0.5 + cross_bias,
            MacDockEdge::Left => (shelf_rect_f.left + shelf_rect_f.right) * 0.5 + cross_bias,
            MacDockEdge::Right => (shelf_rect_f.left + shelf_rect_f.right) * 0.5 - cross_bias,
        };
        let slot_cross = (shelf_cross - 10.0 * u).max(base_sz);

        for (idx, s) in self.slots.iter().enumerate() {
            let w = slot_widths[idx] * fit_k;
            let ico_sz = (icon_sizes[idx] * fit_k).min(w);
            let is_div = rest_is_div[idx];
            let is_sp = rest_is_sp[idx];
            let lift = lifts[idx];

            let (slot_rect, rest_cx, rest_cy) = match cfg.mac_dock_edge {
                MacDockEdge::Bottom => {
                    let cy = base_center_cross - lift;
                    (
                        D2D_RECT_F {
                            left: span_pos,
                            top: cy - slot_cross * 0.5,
                            right: span_pos + w,
                            bottom: cy + slot_cross * 0.5,
                        },
                        span_pos + w * 0.5,
                        cy,
                    )
                }
                MacDockEdge::Top => {
                    let cy = base_center_cross + lift;
                    (
                        D2D_RECT_F {
                            left: span_pos,
                            top: cy - slot_cross * 0.5,
                            right: span_pos + w,
                            bottom: cy + slot_cross * 0.5,
                        },
                        span_pos + w * 0.5,
                        cy,
                    )
                }
                MacDockEdge::Left => {
                    let cx = base_center_cross + lift;
                    (
                        D2D_RECT_F {
                            left: cx - slot_cross * 0.5,
                            top: span_pos,
                            right: cx + slot_cross * 0.5,
                            bottom: span_pos + w,
                        },
                        cx,
                        span_pos + w * 0.5,
                    )
                }
                MacDockEdge::Right => {
                    let cx = base_center_cross - lift;
                    (
                        D2D_RECT_F {
                            left: cx - slot_cross * 0.5,
                            top: span_pos,
                            right: cx + slot_cross * 0.5,
                            bottom: span_pos + w,
                        },
                        cx,
                        span_pos + w * 0.5,
                    )
                }
            };

            // Anchor icon cleanly on left edge whenever slot shows inline text horizontally
            let cx = if !is_vertical && !is_div && !is_sp && w > ico_sz * 1.28 {
                let left_cx = slot_rect.left + 8.0 * u + ico_sz * 0.5;
                let t_left = ((w - ico_sz * 1.1) / (26.0 * u)).clamp(0.0, 1.0);
                rest_cx * (1.0 - t_left) + left_cx * t_left
            } else {
                rest_cx
            };
            let cy = rest_cy;

            let pad_hit = 2.0 * u;
            let hit_rect = D2D_RECT_F {
                left: slot_rect.left.min(cx - ico_sz * 0.5) - pad_hit,
                top: slot_rect.top.min(cy - ico_sz * 0.5) - pad_hit,
                right: slot_rect.right.max(cx + ico_sz * 0.5) + pad_hit,
                bottom: slot_rect.bottom.max(cy + ico_sz * 0.5) + pad_hit,
            };
            let (label, dot_count, is_active) = match s {
                DockSlotKind::TrayStatusCluster => {
                    let bat_txt = match self.tray_bat_pct {
                        Some(p) => {
                            if self.tray_bat_charging {
                                format!(" • Bat {}% ⚡", p)
                            } else {
                                format!(" • Bat {}%", p)
                            }
                        }
                        None => String::new(),
                    };
                    let vol_txt = if self.tray_vol_muted {
                        "Muted".to_string()
                    } else {
                        format!("Vol {}%", self.tray_vol_pct)
                    };
                    let net_txt = if self.tray_wifi_on {
                        format!("{} ({}%)", self.tray_wifi_name, self.tray_wifi_signal)
                    } else {
                        "Wi-Fi Off".to_string()
                    };
                    (format!("{} • {}{} (Scroll: Vol)", net_txt, vol_txt, bat_txt), 0, false)
                }
                _ => slot_meta(s, fg_hwnd),
            };
            let inline_text = match s {
                DockSlotKind::StartLauncher => "Start".to_string(),
                DockSlotKind::SearchButton => "Search".to_string(),
                DockSlotKind::TaskViewButton => "Desktops".to_string(),
                DockSlotKind::StudioLauncher => "Studio".to_string(),
                DockSlotKind::TrashBin { .. } => "Trash".to_string(),
                DockSlotKind::PinnedApp { name, windows, .. }
                | DockSlotKind::RunningApp { name, windows, .. } => {
                    if let Some(w0) = windows.first() {
                        if !w0.title.trim().is_empty()
                            && (spec.layout == DockLayoutKind::FullEdgeTaskbar
                                || matches!(
                                    cfg.w_apps.label_mode,
                                    DockRunningLabelMode::AllRunningApps
                                        | DockRunningLabelMode::ActiveAppOnly
                                ))
                        {
                            truncate_title(&w0.title, 18)
                        } else {
                            truncate_title(name, 14)
                        }
                    } else {
                        truncate_title(name, 14)
                    }
                }
                _ => label.clone(),
            };
            let has_windows = dot_count > 0 || matches!(s, DockSlotKind::ActiveAppChip { .. });
            let is_flashing = match s {
                DockSlotKind::PinnedApp { windows, .. }
                | DockSlotKind::RunningApp { windows, .. } => windows
                    .iter()
                    .any(|w| self.flashing_hwnds.contains(&(w.hwnd.0 as isize))),
                _ => false,
            };

            layout.push(DockLayoutSlot {
                slot_idx: idx,
                is_divider: is_div,
                is_spacer: is_sp,
                center_x: cx,
                center_y: cy,
                icon_size: ico_sz,
                slot_rect,
                hit_rect,
                label,
                inline_text,
                inline_alpha: inline_alphas[idx],
                dot_count,
                has_windows,
                is_active,
                is_flashing,
            });
            span_pos += w;
            if has_gap_after(idx, &self.slots) {
                span_pos += gap * fit_k;
            }
        }
        pane.layout = layout;

        // Build island shelf rectangles and stepped HUD telemetry wing
        pane.island_rects_local.clear();
        pane.island_rects_screen.clear();
        pane.hud_wing_local = None;

        if spec.layout == DockLayoutKind::MultiIslandBar {
            let mut run_lo: Option<f32> = None;
            let mut run_hi = 0.0f32;
            let flush_island = |lo: f32, hi: f32, out_local: &mut Vec<D2D_RECT_F>| {
                let pad = pad_span * fit_k;
                let ir = if is_vertical {
                    D2D_RECT_F {
                        left: shelf_rect_f.left,
                        top: (lo - pad).max(shelf_rect_f.top),
                        right: shelf_rect_f.right,
                        bottom: (hi + pad).min(shelf_rect_f.bottom),
                    }
                } else {
                    D2D_RECT_F {
                        left: (lo - pad).max(shelf_rect_f.left),
                        top: shelf_rect_f.top,
                        right: (hi + pad).min(shelf_rect_f.right),
                        bottom: shelf_rect_f.bottom,
                    }
                };
                out_local.push(ir);
            };
            for item in &pane.layout {
                if item.is_spacer {
                    if let Some(lo) = run_lo.take() {
                        flush_island(lo, run_hi, &mut pane.island_rects_local);
                    }
                    continue;
                }
                let (lo_v, hi_v) = if is_vertical {
                    (item.slot_rect.top, item.slot_rect.bottom)
                } else {
                    (item.slot_rect.left, item.slot_rect.right)
                };
                run_lo = Some(match run_lo {
                    None => lo_v,
                    Some(prev) => prev.min(lo_v),
                });
                run_hi = run_hi.max(hi_v);
            }
            if let Some(lo) = run_lo {
                flush_island(lo, run_hi, &mut pane.island_rects_local);
            }
            if let (Some(first), Some(last)) = (
                pane.island_rects_local.first().copied(),
                pane.island_rects_local.last().copied(),
            ) {
                shelf_rect_f = D2D_RECT_F {
                    left: first.left.min(last.left),
                    top: first.top.min(last.top),
                    right: first.right.max(last.right),
                    bottom: first.bottom.max(last.bottom),
                };
            }
        } else if spec.layout != DockLayoutKind::IndependentOrbs && !cfg.w_shelf.is_all_elements_off() {
            pane.island_rects_local.push(shelf_rect_f);
        }

        for r in &pane.island_rects_local {
            pane.island_rects_screen.push(RECT {
                left: pane.canvas_rect.left + r.left.round() as i32,
                top: pane.canvas_rect.top + r.top.round() as i32,
                right: pane.canvas_rect.left + r.right.round() as i32,
                bottom: pane.canvas_rect.top + r.bottom.round() as i32,
            });
        }

        if hud_wing_h > 0.5 && !is_vertical {
            let w_ratio = (cfg.w_hud.width_pct.clamp(40, 95) as f32) / 100.0;
            let wing_w = (dock_span * w_ratio)
                .clamp(240.0 * u, 520.0 * u)
                .min((dock_span - 24.0 * u).max(160.0 * u));
            let cx = (shelf_rect_f.left + shelf_rect_f.right) * 0.5;
            let wing_r = match cfg.mac_dock_edge {
                MacDockEdge::Top => D2D_RECT_F {
                    left: cx - wing_w * 0.5,
                    top: shelf_rect_f.bottom - 1.0 * u,
                    right: cx + wing_w * 0.5,
                    bottom: shelf_rect_f.bottom + hud_wing_h - 1.0 * u,
                },
                _ => D2D_RECT_F {
                    left: cx - wing_w * 0.5,
                    top: shelf_rect_f.top - hud_wing_h + 1.0 * u,
                    right: cx + wing_w * 0.5,
                    bottom: shelf_rect_f.top + 1.0 * u,
                },
            };
            let (hud_theme, _) = resolve_widget_spec(cfg, TaskbarWidgetId::HudWing);
            let default_tag = match hud_theme {
                TaskbarTheme::CyberpunkHudDock => "NET.HUD",
                TaskbarTheme::TerminalPhosphorDock => "TTY0",
                TaskbarTheme::SolsticeAstrolabeDock => "ORBIT",
                TaskbarTheme::VaporwaveHorizonDock => "SYS.84",
                TaskbarTheme::ObsidianEmberDock => "FORGE",
                _ => "DECK",
            };
            let tag = if !cfg.w_hud.custom_prefix.trim().is_empty() {
                cfg.w_hud.custom_prefix.trim()
            } else {
                default_tag
            };
            let raw_title = if self.fg_full_title.trim().is_empty() {
                "DESKTOP".to_string()
            } else {
                self.fg_full_title.clone()
            };
            let active_lbl = if cfg.w_hud.marquee_ticker && raw_title.chars().count() > 22 {
                let padded = format!("{}   •   ", raw_title);
                let chars: Vec<char> = padded.chars().collect();
                let n = chars.len().max(1);
                let offset = ((self.dock_anim_phase * 2.8) as usize) % n;
                (0..24).map(|i| chars[(offset + i) % n]).collect::<String>()
            } else {
                truncate_title(&raw_title, 24)
            };
            let wing_line = match cfg.w_hud.content {
                HudTelemetryContent::ActiveWindowTitle => {
                    format!("{}  //  ACTIVE: {}", tag, active_lbl)
                }
                HudTelemetryContent::TitleAndCpuRam => {
                    format!(
                        "{}  //  {}  [CPU {}% | RAM {}%]",
                        tag,
                        truncate_title(&active_lbl, 16),
                        self.cpu_pct,
                        self.ram_pct
                    )
                }
                HudTelemetryContent::TitleAndMediaTrack => {
                    if !self.media_track.is_empty() {
                        format!(
                            "{}  //  ♫ {}  •  {}",
                            tag,
                            truncate_title(&self.media_track, 16),
                            truncate_title(&active_lbl, 12)
                        )
                    } else {
                        format!(
                            "{}  //  {}  [CPU {}% | RAM {}%]",
                            tag,
                            truncate_title(&active_lbl, 16),
                            self.cpu_pct,
                            self.ram_pct
                        )
                    }
                }
            };
            pane.hud_wing_local = Some((wing_r, tag.to_string(), wing_line));
        }

        pane.shelf_rect = RECT {
            left: pane.canvas_rect.left + shelf_rect_f.left.round() as i32,
            top: pane.canvas_rect.top + shelf_rect_f.top.round() as i32,
            right: pane.canvas_rect.left + shelf_rect_f.right.round() as i32,
            bottom: pane.canvas_rect.top + shelf_rect_f.bottom.round() as i32,
        };

        pane.motion_active = lerping;

        if pane.last_dock_span > 1.0 {
            pane.cursor_axis_norm =
                ((mouse_rel + pane.last_dock_span * 0.5) / pane.last_dock_span).clamp(0.0, 1.0);
        }

        // Update Live DWM Thumbnail Hover Popup BEFORE rendering so the pane paints at most once per tick
        let was_thumb_vis = self.thumb_popup.visible;
        let was_thumb_key = if was_thumb_vis {
            Some(self.thumb_popup.slot_key.clone())
        } else {
            None
        };
        if !cfg.tb_hover_react || self.menu_open || !pane.window_shown || pane.slide_t < 0.70 {
            if self.thumb_owner_hwnd == pane.hwnd {
                self.thumb_popup.hide();
                self.thumb_owner_hwnd = HWND::default();
            }
            if self.hover_dwell_pane == pane.hwnd {
                self.hover_dwell_pane = HWND::default();
                self.hover_dwell_since = None;
                self.hover_dwell_key.clear();
            }
        } else {
            let mut preview_target: Option<(String, Vec<WinEntry>, RECT)> = None;
            if in_band {
                if let Some(hidx) = pane.hovered_idx {
                    if let (Some(slot), Some(item)) =
                        (self.slots.get(hidx).cloned(), pane.layout.get(hidx).cloned())
                    {
                        match &slot {
                            DockSlotKind::PinnedApp { windows, .. }
                            | DockSlotKind::RunningApp { windows, .. }
                                if !windows.is_empty() =>
                            {
                                let key = self
                                    .slot_keys
                                    .get(hidx)
                                    .cloned()
                                    .unwrap_or_else(|| slot_id_key(&slot));
                                let anchor = RECT {
                                    left: pane.canvas_rect.left + item.slot_rect.left.round() as i32,
                                    top: pane.canvas_rect.top + item.slot_rect.top.round() as i32,
                                    right: pane.canvas_rect.left + item.slot_rect.right.round() as i32,
                                    bottom: pane.canvas_rect.top + item.slot_rect.bottom.round() as i32,
                                };
                                preview_target = Some((key, windows.clone(), anchor));
                            }
                            _ => {}
                        }
                    }
                }
            }
            match decide_thumb_hover_action(
                in_thumb,
                in_band,
                in_bridge,
                preview_target.is_some(),
                pane.hovered_idx.is_some(),
            ) {
                ThumbHoverAction::KeepStable => {
                    // Cursor is inside the thumbnail popup itself, or crossing the open corridor
                    // between the dock and the popup without hovering a different dock slot.
                }
                ThumbHoverAction::SwitchOrOpen => {
                    if let Some((key, windows, anchor)) = preview_target {
                        if self.hover_dwell_key != key || self.hover_dwell_pane != pane.hwnd {
                            self.hover_dwell_pane = pane.hwnd;
                            self.hover_dwell_key = key.clone();
                            self.hover_dwell_since = Some(now);
                        }
                        let ready = (self.thumb_popup.visible && self.thumb_owner_hwnd == pane.hwnd)
                            || self
                                .hover_dwell_since
                                .map(|t| now.duration_since(t) >= Duration::from_millis(160))
                                .unwrap_or(false);
                        if ready {
                            self.open_thumb_popup_for_pane(pane, &key, &windows, anchor, cfg);
                        }
                    }
                }
                ThumbHoverAction::HideNow => {
                    if self.hover_dwell_pane == pane.hwnd {
                        self.hover_dwell_pane = HWND::default();
                        self.hover_dwell_key.clear();
                        self.hover_dwell_since = None;
                    }
                    if self.thumb_owner_hwnd == pane.hwnd {
                        self.thumb_popup.hide();
                        self.thumb_owner_hwnd = HWND::default();
                    }
                }
                ThumbHoverAction::ExpireGrace => {
                    let expired = (!self.thumb_popup.visible && !in_band)
                        || pane
                            .last_inside_time
                            .map(|t| now.duration_since(t) > Duration::from_millis(180))
                            .unwrap_or(true);
                    if expired {
                        if self.hover_dwell_pane == pane.hwnd {
                            self.hover_dwell_pane = HWND::default();
                            self.hover_dwell_key.clear();
                            self.hover_dwell_since = None;
                        }
                        if self.thumb_owner_hwnd == pane.hwnd {
                            self.thumb_popup.hide();
                            self.thumb_owner_hwnd = HWND::default();
                        }
                    }
                }
            }
        }
        let now_thumb_key = if self.thumb_popup.visible {
            Some(self.thumb_popup.slot_key.as_str())
        } else {
            None
        };
        if was_thumb_vis != self.thumb_popup.visible || was_thumb_key.as_deref() != now_thumb_key {
            pane.dirty_paint = true;
        }

        let live_ambient_pane =
            is_ambient_animated_cfg(cfg) && pane.window_shown && pane.slide_t > 0.25;
        if pane.dirty_paint
            || !pane.window_shown
            || live_ambient_pane
            || (pane.click_ripple_t < 0.999)
        {
            self.render_pane(pane, cfg, shelf_rect_f);
            pane.dirty_paint = false;
        }
    }

    fn open_thumb_popup_for_pane(
        &mut self,
        pane: &DockMonitorPane,
        key: &str,
        windows: &[WinEntry],
        anchor: RECT,
        cfg: &AppConfig,
    ) {
        let wing_top_s = pane
            .hud_wing_local
            .as_ref()
            .map(|(r, _, _)| pane.canvas_rect.top + r.top.round() as i32)
            .unwrap_or(pane.shelf_rect.top);
        let wing_bot_s = pane
            .hud_wing_local
            .as_ref()
            .map(|(r, _, _)| pane.canvas_rect.top + r.bottom.round() as i32)
            .unwrap_or(pane.shelf_rect.bottom);
        let eff_shelf = RECT {
            left: pane.shelf_rect.left,
            top: pane.shelf_rect.top.min(wing_top_s),
            right: pane.shelf_rect.right,
            bottom: pane.shelf_rect.bottom.max(wing_bot_s),
        };
        self.thumb_owner_hwnd = pane.hwnd;
        self.thumb_popup.show_for_slot(
            key,
            windows,
            anchor,
            eff_shelf,
            cfg.mac_dock_edge,
            pane.mon_rect,
            cfg.tb_theme,
            pane.dpi_u,
        );
    }

    pub fn handle_nchittest(&self, hwnd: HWND, sx: i32, sy: i32) -> LRESULT {
        let Some(pidx) = self.pane_idx_for_hwnd(hwnd) else {
            return LRESULT(-1);
        };
        let pane = &self.panes[pidx];
        if !pane.window_shown || pane.slide_t < 0.15 {
            return LRESULT(-1);
        }
        for r in &pane.island_rects_screen {
            if sx >= r.left && sx <= r.right && sy >= r.top && sy <= r.bottom {
                return LRESULT(1); // HTCLIENT
            }
        }
        let lx = (sx - pane.canvas_rect.left) as f32;
        let ly = (sy - pane.canvas_rect.top) as f32;
        if let Some((wr, _, _)) = &pane.hud_wing_local {
            if lx >= wr.left && lx <= wr.right && ly >= wr.top && ly <= wr.bottom {
                return LRESULT(1);
            }
        }
        for item in &pane.layout {
            if !item.is_divider
                && !item.is_spacer
                && lx >= item.hit_rect.left
                && lx <= item.hit_rect.right
                && ly >= item.hit_rect.top
                && ly <= item.hit_rect.bottom
            {
                return LRESULT(1);
            }
        }
        LRESULT(-1) // HTTRANSPARENT
    }

    pub fn handle_set_cursor(&self, hwnd: HWND) -> LRESULT {
        let hovered = self
            .pane_idx_for_hwnd(hwnd)
            .and_then(|pi| self.panes[pi].hovered_idx)
            .is_some();
        unsafe {
            let cur = if hovered { IDC_HAND } else { IDC_ARROW };
            let _ = SetCursor(LoadCursorW(None, cur).ok());
        }
        LRESULT(1)
    }

    pub fn handle_mouse_move(&mut self, hwnd: HWND, cfg: &AppConfig) {
        let Some(pi) = self.pane_idx_for_hwnd(hwnd) else {
            return;
        };
        if !self.panes[pi].tracking_leave && !hwnd.0.is_null() {
            let mut tme = TRACKMOUSEEVENT {
                cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
                dwFlags: TME_LEAVE,
                hwndTrack: hwnd,
                dwHoverTime: 0,
            };
            unsafe {
                if TrackMouseEvent(&mut tme).is_ok() {
                    self.panes[pi].tracking_leave = true;
                }
            }
        }
        let was_in = self.panes[pi].cursor_in_dock;
        self.panes[pi].cursor_in_dock = true;
        self.panes[pi].last_inside_time = Some(Instant::now());
        if cfg.tb_hover_react || !was_in {
            self.panes[pi].dirty_paint = true;
        }
        // Only run an immediate tick on first entry when the dock was hidden;
        // subsequent mouse moves paint asynchronously via TIMER_DOCK_TICK without stalling video wallpaper.
        if !was_in && !self.panes[pi].window_shown {
            self.tick(cfg);
        }
    }

    pub fn handle_mouse_leave(&mut self, hwnd: HWND, cfg: &AppConfig) {
        let Some(pi) = self.pane_idx_for_hwnd(hwnd) else {
            return;
        };
        self.panes[pi].tracking_leave = false;
        self.panes[pi].pressed_idx = None;
        let mut pt = POINT::default();
        let got = unsafe { GetCursorPos(&mut pt).is_ok() };
        let dpi_u = self.panes[pi].dpi_u;
        let (spec, _is_vertical, _, _, _, hud_wing_h) = dock_cross_metrics(cfg, dpi_u);
        let in_band = got
            && self.panes[pi].window_shown
            && self.panes[pi].slide_t >= 0.12
            && self.panes[pi].is_point_in_dock_band(pt, cfg.mac_dock_edge, spec.layout, hud_wing_h, dpi_u);
        let thumb_owned = self.thumb_owner_hwnd == hwnd;
        let in_thumb = thumb_owned && got && self.thumb_popup.contains_point(pt);
        let in_bridge = thumb_owned && got && self.thumb_popup.in_bridge(pt);
        if !in_band {
            self.panes[pi].cursor_in_dock = false;
            if !in_thumb && !in_bridge {
                self.panes[pi].hovered_idx = None;
            }
            self.panes[pi].dirty_paint = true;
        }
    }

    pub fn handle_thumb_set_cursor(&self) -> LRESULT {
        unsafe {
            let cur = if self.thumb_popup.hovered_card.is_some() {
                IDC_HAND
            } else {
                IDC_ARROW
            };
            let _ = SetCursor(LoadCursorW(None, cur).ok());
        }
        LRESULT(1)
    }

    pub fn handle_thumb_paint(&mut self, cfg: &AppConfig) {
        let pal = effective_taskbar_palette(cfg);
        unsafe {
            self.thumb_popup.paint(
                self.d2d.as_ref(),
                self.dw.as_ref(),
                self.wic.as_ref(),
                cfg,
                &pal,
            );
        }
    }

    pub fn handle_thumb_mouse_move(&mut self, cfg: &AppConfig) {
        if !self.thumb_popup.tracking_leave && !self.thumb_popup.hwnd.0.is_null() {
            let mut tme = TRACKMOUSEEVENT {
                cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
                dwFlags: TME_LEAVE,
                hwndTrack: self.thumb_popup.hwnd,
                dwHoverTime: 0,
            };
            unsafe {
                if TrackMouseEvent(&mut tme).is_ok() {
                    self.thumb_popup.tracking_leave = true;
                }
            }
        }
        let mut pt = POINT::default();
        if unsafe { GetCursorPos(&mut pt).is_ok() } {
            if let Some(pi) = self.pane_idx_for_hwnd(self.thumb_owner_hwnd) {
                self.panes[pi].last_inside_time = Some(Instant::now());
            }
            let lx = (pt.x - self.thumb_popup.popup_rect.left) as f32;
            let ly = (pt.y - self.thumb_popup.popup_rect.top) as f32;
            let (hc, hx) = self.thumb_popup.hit_test_client(lx, ly);
            if hc != self.thumb_popup.hovered_card || hx != self.thumb_popup.hovered_close {
                self.thumb_popup.hovered_card = hc;
                self.thumb_popup.hovered_close = hx;
                unsafe {
                    let _ = InvalidateRect(Some(self.thumb_popup.hwnd), None, false);
                }
            }
        }
        let _ = cfg;
    }

    pub fn handle_thumb_mouse_leave(&mut self, cfg: &AppConfig) {
        self.thumb_popup.tracking_leave = false;
        self.thumb_popup.hovered_card = None;
        self.thumb_popup.hovered_close = None;
        if !self.thumb_popup.hwnd.0.is_null() {
            unsafe {
                let _ = InvalidateRect(Some(self.thumb_popup.hwnd), None, false);
            }
        }
        self.tick(cfg);
    }

    pub fn handle_thumb_lbutton_up(&mut self, lx: f32, ly: f32, cfg: &AppConfig) -> MacDockAction {
        let (hc, hx) = self.thumb_popup.hit_test_client(lx, ly);
        if let Some(ci) = hx {
            if let Some(card) = self.thumb_popup.cards.get(ci) {
                let target_hwnd = card.win_hwnd;
                self.flashing_hwnds.remove(&(target_hwnd.0 as isize));
                unsafe {
                    let _ = PostMessageW(Some(target_hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
                }
                self.thumb_popup.hide();
                self.thumb_owner_hwnd = HWND::default();
                self.dirty_windows = true;
                self.mark_dirty_paint();
                self.tick(cfg);
            }
            return MacDockAction::None;
        }
        if let Some(ci) = hc {
            if let Some(card) = self.thumb_popup.cards.get(ci) {
                let target_hwnd = card.win_hwnd;
                self.flashing_hwnds.remove(&(target_hwnd.0 as isize));
                self.thumb_popup.hide();
                self.thumb_owner_hwnd = HWND::default();
                bring_window_to_front(target_hwnd);
                self.dirty_windows = true;
                self.mark_dirty_paint();
            }
        }
        MacDockAction::None
    }

    pub fn handle_thumb_mbutton_up(&mut self, lx: f32, ly: f32, cfg: &AppConfig) {
        let (hc, _) = self.thumb_popup.hit_test_client(lx, ly);
        if let Some(ci) = hc {
            if let Some(card) = self.thumb_popup.cards.get(ci) {
                let target_hwnd = card.win_hwnd;
                self.flashing_hwnds.remove(&(target_hwnd.0 as isize));
                unsafe {
                    let _ = PostMessageW(Some(target_hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
                }
                self.thumb_popup.hide();
                self.thumb_owner_hwnd = HWND::default();
                self.dirty_windows = true;
                self.mark_dirty_paint();
                self.tick(cfg);
            }
        }
    }

    pub fn handle_lbutton_down(&mut self, hwnd: HWND, lx: f32, ly: f32, cfg: &AppConfig) {
        let Some(pi) = self.pane_idx_for_hwnd(hwnd) else {
            return;
        };
        let hit = self.panes[pi].hit_slot_at_client(lx, ly);
        self.panes[pi].pressed_idx = hit;
        // Interactive PBR material impulse ripple at click location (water splash, sonar, glint, droplet)
        self.panes[pi].click_ripple_axis = self.panes[pi].cursor_axis_norm;
        self.panes[pi].click_ripple_t = 0.0;
        self.panes[pi].dirty_paint = true;
        if hit.is_some() {
            self.tick(cfg);
        }
    }

    pub fn handle_lbutton_up(&mut self, hwnd: HWND, lx: f32, ly: f32, cfg: &AppConfig) -> MacDockAction {
        let Some(pi) = self.pane_idx_for_hwnd(hwnd) else {
            return MacDockAction::None;
        };
        let clicked = self.panes[pi].hit_slot_at_client(lx, ly);
        let pressed = self.panes[pi].pressed_idx.take();
        self.panes[pi].dirty_paint = true;
        let Some(idx) = clicked else {
            return MacDockAction::None;
        };
        if pressed.is_some() && pressed != Some(idx) {
            return MacDockAction::None;
        }
        let Some(slot) = self.slots.get(idx).cloned() else {
            return MacDockAction::None;
        };
        let shift_held = unsafe { GetKeyState(VK_SHIFT.0 as i32) < 0 };
        let ctrl_held = unsafe { GetKeyState(VK_CONTROL.0 as i32) < 0 };
        let id_key = slot_id_key(&slot);
        match slot {
            DockSlotKind::StartLauncher => {
                self.thumb_popup.hide();
                self.thumb_owner_hwnd = HWND::default();
                self.pop_until
                    .insert(id_key, Instant::now() + Duration::from_millis(180));
                trigger_start_menu();
                MacDockAction::None
            }
            DockSlotKind::SearchButton => {
                self.thumb_popup.hide();
                self.thumb_owner_hwnd = HWND::default();
                self.pop_until
                    .insert(id_key, Instant::now() + Duration::from_millis(180));
                send_win_chord(0x53); // Win+S Search
                MacDockAction::None
            }
            DockSlotKind::TaskViewButton | DockSlotKind::ActiveAppChip { .. } => {
                self.thumb_popup.hide();
                self.thumb_owner_hwnd = HWND::default();
                self.pop_until
                    .insert(id_key, Instant::now() + Duration::from_millis(180));
                send_win_chord(0x09); // Win+Tab Task View
                MacDockAction::None
            }
            DockSlotKind::HiddenTrayChevron => {
                self.thumb_popup.hide();
                self.thumb_owner_hwnd = HWND::default();
                self.pop_until
                    .insert(id_key, Instant::now() + Duration::from_millis(180));
                let canvas_rc = self.panes[pi].canvas_rect;
                let anchor_rc = self.panes[pi]
                    .layout
                    .iter()
                    .find(|it| it.slot_idx == idx)
                    .map(|it| RECT {
                        left: canvas_rc.left + it.slot_rect.left.round() as i32,
                        top: canvas_rc.top + it.slot_rect.top.round() as i32,
                        right: canvas_rc.left + it.slot_rect.right.round() as i32,
                        bottom: canvas_rc.top + it.slot_rect.bottom.round() as i32,
                    })
                    .unwrap_or(canvas_rc);
                toggle_hidden_tray_overflow(self.panes[pi].hwnd, anchor_rc, cfg.mac_dock_edge);
                MacDockAction::None
            }
            DockSlotKind::TrayStatusCluster => {
                self.thumb_popup.hide();
                self.thumb_owner_hwnd = HWND::default();
                self.pop_until
                    .insert(id_key, Instant::now() + Duration::from_millis(180));
                send_win_chord(0x41); // Win+A Windows Quick Settings
                MacDockAction::None
            }
            DockSlotKind::ClockModule { .. } => {
                self.thumb_popup.hide();
                self.thumb_owner_hwnd = HWND::default();
                self.pop_until
                    .insert(id_key, Instant::now() + Duration::from_millis(180));
                send_win_chord(0x4E); // Win+N Notification & Calendar flyout
                MacDockAction::None
            }
            DockSlotKind::ShowDesktopSliver => {
                self.thumb_popup.hide();
                self.thumb_owner_hwnd = HWND::default();
                self.pop_until
                    .insert(id_key, Instant::now() + Duration::from_millis(180));
                send_win_chord(0x44); // Win+D Show Desktop
                MacDockAction::None
            }
            DockSlotKind::StudioLauncher => {
                self.thumb_popup.hide();
                self.thumb_owner_hwnd = HWND::default();
                self.pop_until
                    .insert(id_key, Instant::now() + Duration::from_millis(180));
                MacDockAction::OpenSettingsDock
            }
            DockSlotKind::TrashBin { .. } => {
                self.thumb_popup.hide();
                self.thumb_owner_hwnd = HWND::default();
                self.pop_until
                    .insert(id_key, Instant::now() + Duration::from_millis(180));
                launch_app_path("shell:RecycleBinFolder");
                MacDockAction::None
            }
            DockSlotKind::PinnedApp { path, windows, .. } => {
                for w in &windows {
                    self.flashing_hwnds.remove(&(w.hwnd.0 as isize));
                }
                let target = if !path.is_empty() {
                    path.clone()
                } else {
                    windows.first().map(|w| w.exe_path.clone()).unwrap_or_default()
                };
                if ctrl_held && shift_held && !target.is_empty() {
                    let resolved = self.resolve_shortcut_target(&target);
                    let run_t = if !resolved.is_empty() { resolved } else { target };
                    unsafe { run_as_admin(&run_t); }
                } else if shift_held && !target.is_empty() {
                    self.pop_until
                        .insert(id_key, Instant::now() + Duration::from_millis(180));
                    launch_app_path(&target);
                } else if windows.is_empty() {
                    if cfg.mac_dock_bounce {
                        self.bounce_until
                            .insert(id_key, Instant::now() + Duration::from_millis(2400));
                    }
                    launch_app_path(&path);
                } else if windows.len() >= 2 {
                    if self.thumb_popup.visible
                        && self.thumb_owner_hwnd == hwnd
                        && self.thumb_popup.slot_key == id_key
                    {
                        self.thumb_popup.hide();
                        self.thumb_owner_hwnd = HWND::default();
                        activate_or_toggle_windows(&windows);
                    } else if let Some(item) = self.panes[pi].layout.iter().find(|it| it.slot_idx == idx).cloned() {
                        let pane = self.panes[pi].mon_rect;
                        let _ = pane;
                        let anchor = RECT {
                            left: self.panes[pi].canvas_rect.left + item.slot_rect.left.round() as i32,
                            top: self.panes[pi].canvas_rect.top + item.slot_rect.top.round() as i32,
                            right: self.panes[pi].canvas_rect.left + item.slot_rect.right.round() as i32,
                            bottom: self.panes[pi].canvas_rect.top + item.slot_rect.bottom.round() as i32,
                        };
                        let panes = std::mem::take(&mut self.panes);
                        self.open_thumb_popup_for_pane(&panes[pi], &id_key, &windows, anchor, cfg);
                        self.panes = panes;
                    } else {
                        activate_or_toggle_windows(&windows);
                    }
                    self.pop_until
                        .insert(id_key, Instant::now() + Duration::from_millis(180));
                    self.dirty_windows = true;
                } else {
                    self.thumb_popup.hide();
                    self.thumb_owner_hwnd = HWND::default();
                    self.pop_until
                        .insert(id_key, Instant::now() + Duration::from_millis(180));
                    activate_or_toggle_windows(&windows);
                    self.dirty_windows = true;
                }
                MacDockAction::None
            }
            DockSlotKind::RunningApp { exe_path, windows, .. } => {
                for w in &windows {
                    self.flashing_hwnds.remove(&(w.hwnd.0 as isize));
                }
                let target = running_launch_target(&exe_path, &windows);
                if ctrl_held && shift_held && !target.is_empty() {
                    unsafe { run_as_admin(&target); }
                } else if shift_held && !target.is_empty() {
                    self.pop_until
                        .insert(id_key, Instant::now() + Duration::from_millis(180));
                    launch_app_path(&target);
                } else if windows.len() >= 2 {
                    if self.thumb_popup.visible
                        && self.thumb_owner_hwnd == hwnd
                        && self.thumb_popup.slot_key == id_key
                    {
                        self.thumb_popup.hide();
                        self.thumb_owner_hwnd = HWND::default();
                        activate_or_toggle_windows(&windows);
                    } else if let Some(item) = self.panes[pi].layout.iter().find(|it| it.slot_idx == idx).cloned() {
                        let anchor = RECT {
                            left: self.panes[pi].canvas_rect.left + item.slot_rect.left.round() as i32,
                            top: self.panes[pi].canvas_rect.top + item.slot_rect.top.round() as i32,
                            right: self.panes[pi].canvas_rect.left + item.slot_rect.right.round() as i32,
                            bottom: self.panes[pi].canvas_rect.top + item.slot_rect.bottom.round() as i32,
                        };
                        let panes = std::mem::take(&mut self.panes);
                        self.open_thumb_popup_for_pane(&panes[pi], &id_key, &windows, anchor, cfg);
                        self.panes = panes;
                    } else {
                        activate_or_toggle_windows(&windows);
                    }
                    self.pop_until
                        .insert(id_key, Instant::now() + Duration::from_millis(180));
                    self.dirty_windows = true;
                } else {
                    self.thumb_popup.hide();
                    self.thumb_owner_hwnd = HWND::default();
                    self.pop_until
                        .insert(id_key, Instant::now() + Duration::from_millis(180));
                    activate_or_toggle_windows(&windows);
                    self.dirty_windows = true;
                }
                MacDockAction::None
            }
            DockSlotKind::Divider
            | DockSlotKind::EndDivider
            | DockSlotKind::IslandGap
            | DockSlotKind::FlexSpacer => MacDockAction::None,
        }
    }

    pub fn handle_mbutton_up(&mut self, hwnd: HWND, lx: f32, ly: f32, cfg: &AppConfig) -> MacDockAction {
        let Some(pi) = self.pane_idx_for_hwnd(hwnd) else {
            return MacDockAction::None;
        };
        let Some(idx) = self.panes[pi].hit_slot_at_client(lx, ly) else {
            return MacDockAction::None;
        };
        let Some(slot) = self.slots.get(idx).cloned() else {
            return MacDockAction::None;
        };
        let id_key = slot_id_key(&slot);
        match slot {
            DockSlotKind::PinnedApp { path, windows, .. } => {
                if cfg.w_apps.middle_click_action == 1 && !windows.is_empty() {
                    if let Some(w) = windows.first() {
                        unsafe {
                            let _ = PostMessageW(Some(w.hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
                        }
                        self.dirty_windows = true;
                        self.mark_dirty_paint();
                    }
                    return MacDockAction::None;
                }
                if cfg.w_apps.middle_click_action == 2 && !windows.is_empty() {
                    for w in &windows {
                        unsafe {
                            let _ = ShowWindow(w.hwnd, SW_MINIMIZE);
                        }
                    }
                    self.dirty_windows = true;
                    self.mark_dirty_paint();
                    return MacDockAction::None;
                }
                let target = if !path.is_empty() {
                    path
                } else {
                    windows.first().map(|w| w.exe_path.clone()).unwrap_or_default()
                };
                if !target.is_empty() {
                    self.pop_until
                        .insert(id_key, Instant::now() + Duration::from_millis(180));
                    launch_app_path(&target);
                    self.mark_dirty_paint();
                }
                MacDockAction::None
            }
            DockSlotKind::RunningApp { exe_path, windows, .. } => {
                if cfg.w_apps.middle_click_action == 1 && !windows.is_empty() {
                    if let Some(w) = windows.first() {
                        unsafe {
                            let _ = PostMessageW(Some(w.hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
                        }
                        self.dirty_windows = true;
                        self.mark_dirty_paint();
                    }
                    return MacDockAction::None;
                }
                if cfg.w_apps.middle_click_action == 2 && !windows.is_empty() {
                    for w in &windows {
                        unsafe {
                            let _ = ShowWindow(w.hwnd, SW_MINIMIZE);
                        }
                    }
                    self.dirty_windows = true;
                    self.mark_dirty_paint();
                    return MacDockAction::None;
                }
                let target = running_launch_target(&exe_path, &windows);
                if !target.is_empty() {
                    self.pop_until
                        .insert(id_key, Instant::now() + Duration::from_millis(180));
                    launch_app_path(&target);
                    self.mark_dirty_paint();
                }
                MacDockAction::None
            }
            DockSlotKind::TrayStatusCluster => {
                let prev_muted = self.tray_vol_muted;
                self.tray_vol_muted = !self.tray_vol_muted;
                self.tray_anim.on_volume_change(
                    self.tray_vol_pct,
                    self.tray_vol_pct,
                    prev_muted,
                    self.tray_vol_muted,
                    true,
                );
                self.vol_osd_until = Some(Instant::now() + Duration::from_millis(1450));
                self.mark_dirty_paint();
                MacDockAction::TrayToggleMute
            }
            _ => MacDockAction::None,
        }
    }

    pub fn handle_mouse_wheel(
        &mut self,
        hwnd: HWND,
        delta: i16,
        sx: i32,
        sy: i32,
        cfg: &mut AppConfig,
    ) -> MacDockAction {
        let Some(pi) = self.pane_idx_for_hwnd(hwnd) else {
            return MacDockAction::None;
        };
        if !self.panes[pi].window_shown {
            return MacDockAction::None;
        }
        let lx = (sx - self.panes[pi].canvas_rect.left) as f32;
        let ly = (sy - self.panes[pi].canvas_rect.top) as f32;
        let Some(idx) = self.panes[pi].hit_slot_at_client(lx, ly) else {
            return MacDockAction::None;
        };
        let Some(slot) = self.slots.get(idx).cloned() else {
            return MacDockAction::None;
        };
        let now = Instant::now();
        let stale = self
            .wheel_last_at
            .map(|t| now.duration_since(t) > Duration::from_millis(450))
            .unwrap_or(true);
        let d = delta as i32;
        if stale || (self.wheel_accum != 0 && self.wheel_accum.signum() != d.signum()) {
            self.wheel_accum = 0;
        }
        self.wheel_last_at = Some(now);
        self.wheel_accum = (self.wheel_accum + d).clamp(-360, 360);
        if self.wheel_accum.abs() < 120 {
            return MacDockAction::None;
        }
        let notches = (self.wheel_accum / 120).clamp(-2, 2);
        self.wheel_accum %= 120;
        match slot {
            DockSlotKind::TrayStatusCluster => {
                let unit = cfg.w_tray.wheel_step_pct.clamp(1, 10) as i32;
                let prev_v = self.tray_vol_pct;
                let prev_m = self.tray_vol_muted;
                let next_v = (self.tray_vol_pct as i32 + notches * unit).clamp(0, 100) as u8;
                self.tray_vol_pct = next_v;
                if next_v > 0 {
                    self.tray_vol_muted = false;
                }
                self.tray_anim.on_volume_change(
                    prev_v,
                    next_v,
                    prev_m,
                    self.tray_vol_muted,
                    cfg.w_tray.show_osd_on_scroll,
                );
                if cfg.w_tray.show_osd_on_scroll {
                    self.vol_osd_until = Some(Instant::now() + Duration::from_millis(1450));
                }
                self.mark_dirty_paint();
                return MacDockAction::TraySetVolume(next_v);
            }
            DockSlotKind::ClockModule { .. } => match cfg.w_clock.wheel_action {
                DockClockWheelAction::AdjustVolume => {
                    let unit = cfg.w_tray.wheel_step_pct.clamp(1, 10) as i32;
                    let prev_v = self.tray_vol_pct;
                    let prev_m = self.tray_vol_muted;
                    let next_v = (self.tray_vol_pct as i32 + notches * unit).clamp(0, 100) as u8;
                    self.tray_vol_pct = next_v;
                    if next_v > 0 {
                        self.tray_vol_muted = false;
                    }
                    self.tray_anim.on_volume_change(
                        prev_v,
                        next_v,
                        prev_m,
                        self.tray_vol_muted,
                        cfg.w_tray.show_osd_on_scroll,
                    );
                    if cfg.w_tray.show_osd_on_scroll {
                        self.vol_osd_until = Some(Instant::now() + Duration::from_millis(1450));
                    }
                    self.mark_dirty_paint();
                    return MacDockAction::TraySetVolume(next_v);
                }
                DockClockWheelAction::CycleTimezoneOffset => {
                    let next_tz = (cfg.w_clock.tz_offset_half_hours as i32 + notches).clamp(-24, 28) as i8;
                    cfg.w_clock.tz_offset_half_hours = next_tz;
                    save_config(cfg);
                    self.dirty_windows = true;
                    self.mark_dirty_paint();
                    return MacDockAction::ConfigUpdated;
                }
                DockClockWheelAction::None => {}
            },
            DockSlotKind::PinnedApp { windows, .. }
            | DockSlotKind::RunningApp { windows, .. } => {
                if cfg.w_apps.wheel_action == 0 && !windows.is_empty() {
                    activate_or_toggle_windows(&windows);
                }
            }
            _ => {}
        }
        MacDockAction::None
    }

    pub fn handle_rbutton_up(&mut self, hwnd: HWND, lx: f32, ly: f32, cfg: &mut AppConfig) -> MacDockAction {
        let Some(pi) = self.pane_idx_for_hwnd(hwnd) else {
            return MacDockAction::None;
        };
        let mut pt = POINT {
            x: lx.round() as i32,
            y: ly.round() as i32,
        };
        if unsafe { ClientToScreen(hwnd, &mut pt) }.as_bool() == false {
            if unsafe { GetCursorPos(&mut pt).is_err() } {
                return MacDockAction::None;
            }
        }
        let client_x = (pt.x - self.panes[pi].canvas_rect.left) as f32;
        let client_y = (pt.y - self.panes[pi].canvas_rect.top) as f32;
        let hit = self.panes[pi].hit_slot_at_client(client_x, client_y);
        self.active_menu_target = (hwnd, self.panes[pi].mon_rect);

        // Hide hover tooltip and thumbnail preview immediately and anchor menu outside shelf & wing
        self.thumb_popup.hide();
        self.thumb_owner_hwnd = HWND::default();
        self.menu_open = true;
        self.panes[pi].dirty_paint = true;
        self.tick(cfg);
        let wing_top_s = self.panes[pi]
            .hud_wing_local
            .as_ref()
            .map(|(r, _, _)| self.panes[pi].canvas_rect.top + r.top.round() as i32)
            .unwrap_or(self.panes[pi].shelf_rect.top);
        let wing_bot_s = self.panes[pi]
            .hud_wing_local
            .as_ref()
            .map(|(r, _, _)| self.panes[pi].canvas_rect.top + r.bottom.round() as i32)
            .unwrap_or(self.panes[pi].shelf_rect.bottom);
        let popup_pt = match cfg.mac_dock_edge {
            MacDockEdge::Bottom => POINT {
                x: pt.x,
                y: self.panes[pi].shelf_rect.top.min(wing_top_s) - 4,
            },
            MacDockEdge::Top => POINT {
                x: pt.x,
                y: self.panes[pi].shelf_rect.bottom.max(wing_bot_s) + 4,
            },
            MacDockEdge::Left => POINT {
                x: self.panes[pi].shelf_rect.right + 4,
                y: pt.y,
            },
            MacDockEdge::Right => POINT {
                x: self.panes[pi].shelf_rect.left - 4,
                y: pt.y,
            },
        };

        let Some(idx) = hit else {
            return self.show_empty_dock_menu(popup_pt, cfg);
        };
        let Some(slot) = self.slots.get(idx).cloned() else {
            self.menu_open = false;
            return MacDockAction::None;
        };
        match slot {
            DockSlotKind::StartLauncher => self.show_start_winx_menu(popup_pt, cfg),
            DockSlotKind::HiddenTrayChevron | DockSlotKind::TrayStatusCluster => {
                self.show_tray_menu(popup_pt, cfg)
            }
            DockSlotKind::ClockModule { .. } => self.show_clock_menu(popup_pt, cfg),
            DockSlotKind::SearchButton
            | DockSlotKind::TaskViewButton
            | DockSlotKind::StudioLauncher
            | DockSlotKind::ActiveAppChip { .. }
            | DockSlotKind::ShowDesktopSliver => self.show_empty_dock_menu(popup_pt, cfg),
            DockSlotKind::TrashBin { items } => self.show_trash_menu(popup_pt, items > 0, cfg),
            DockSlotKind::PinnedApp {
                name,
                path,
                windows,
                ..
            } => self.show_app_menu(popup_pt, &name, &path, &windows, true, cfg),
            DockSlotKind::RunningApp {
                name,
                exe_path,
                windows,
                ..
            } => self.show_app_menu(popup_pt, &name, &exe_path, &windows, false, cfg),
            DockSlotKind::Divider
            | DockSlotKind::EndDivider
            | DockSlotKind::IslandGap
            | DockSlotKind::FlexSpacer => self.show_empty_dock_menu(popup_pt, cfg),
        }
    }

    fn show_start_winx_menu(&mut self, pt: POINT, cfg: &mut AppConfig) -> MacDockAction {
        self.menu_open = true;
        let (menu_hw, menu_mon) = (self.menu_hwnd(), self.menu_mon_rect());
        let mut action = MacDockAction::None;
        let mut keep_focus = false;
        unsafe {
            let prev_fg = force_foreground_for_menu(menu_hw);
            if let Ok(hmenu) = CreatePopupMenu() {
                add_menu_item(hmenu, IDM_WINX_APPS, "Installed Apps");
                add_menu_item(hmenu, IDM_WINX_POWER, "Power Options");
                add_menu_item(hmenu, IDM_WINX_EVENTVWR, "Event Viewer");
                add_menu_item(hmenu, IDM_WINX_SYSTEM, "System");
                add_menu_item(hmenu, IDM_WINX_DEVMGMT, "Device Manager");
                add_menu_item(hmenu, IDM_WINX_NETWORK, "Network Connections");
                add_menu_item(hmenu, IDM_WINX_DISKMGMT, "Disk Management");
                add_menu_item(hmenu, IDM_WINX_COMPMGMT, "Computer Management");
                let _ = AppendMenuW(hmenu, MF_SEPARATOR, 0, PCWSTR::null());
                add_menu_item(hmenu, IDM_WINX_TERMINAL, "Terminal");
                add_menu_item(hmenu, IDM_WINX_TERMINAL_ADMIN, "Terminal (Admin)");
                add_menu_item(hmenu, IDM_DOCK_TASKMGR, "Task Manager");
                add_menu_item(hmenu, IDM_WINX_SETTINGS, "Settings");
                add_menu_item(hmenu, IDM_DOCK_EXPLORER, "File Explorer");
                add_menu_item(hmenu, IDM_WINX_SEARCH, "Search");
                add_menu_item(hmenu, IDM_WINX_RUN, "Run...");
                let _ = AppendMenuW(hmenu, MF_SEPARATOR, 0, PCWSTR::null());
                add_menu_item(hmenu, IDM_DOCK_SHOW_DESKTOP, "Desktop");
                add_menu_item(hmenu, IDM_WINX_LOCK, "Lock Screen");
                let _ = AppendMenuW(hmenu, MF_SEPARATOR, 0, PCWSTR::null());
                append_widget_customize_submenu(hmenu, TaskbarWidgetId::Utilities, cfg);
                append_widget_customize_submenu(hmenu, TaskbarWidgetId::ShelfFrame, cfg);
                add_menu_item(hmenu, IDM_DOCK_SYNC_WIN_PINS, "Sync Pins from Windows Taskbar");
                add_menu_item(hmenu, IDM_DOCK_SETTINGS, "Customize Dock Theme...");
                add_menu_item(hmenu, IDM_DOCK_EXIT_DOCK, "Switch to Standard Taskbar Theme");

                let cmd = track_dock_popup(menu_hw, hmenu, pt, cfg.mac_dock_edge, menu_mon);
                let _ = DestroyMenu(hmenu);
                if let Some(w_act) = self.handle_widget_menu_cmd(cmd, cfg) {
                    action = w_act;
                } else {
                match cmd {
                    IDM_WINX_APPS => { keep_focus = true; launch_app_path("ms-settings:appsfeatures"); }
                    IDM_WINX_POWER => { keep_focus = true; launch_app_path("ms-settings:powersleep"); }
                    IDM_WINX_EVENTVWR => { keep_focus = true; launch_app_path("eventvwr.msc"); }
                    IDM_WINX_SYSTEM => { keep_focus = true; launch_app_path("ms-settings:about"); }
                    IDM_WINX_DEVMGMT => { keep_focus = true; launch_app_path("devmgmt.msc"); }
                    IDM_WINX_NETWORK => { keep_focus = true; launch_app_path("ms-settings:network"); }
                    IDM_WINX_DISKMGMT => { keep_focus = true; launch_app_path("diskmgmt.msc"); }
                    IDM_WINX_COMPMGMT => { keep_focus = true; launch_app_path("compmgmt.msc"); }
                    IDM_WINX_TERMINAL => { keep_focus = true; launch_app_path("wt.exe"); }
                    IDM_WINX_TERMINAL_ADMIN => { keep_focus = true; run_as_admin("wt.exe"); }
                    IDM_DOCK_TASKMGR => { keep_focus = true; launch_app_path("taskmgr.exe"); }
                    IDM_WINX_SETTINGS => { keep_focus = true; launch_app_path("ms-settings:"); }
                    IDM_DOCK_EXPLORER => { keep_focus = true; launch_app_path("explorer.exe"); }
                    IDM_WINX_SEARCH => { keep_focus = true; send_win_chord(0x53); }
                    IDM_WINX_RUN => { keep_focus = true; send_win_chord(0x52); }
                    IDM_DOCK_SHOW_DESKTOP => { keep_focus = true; send_win_chord(0x44); }
                    IDM_WINX_LOCK => {
                        let verb = to_wide("open");
                        let file = to_wide("rundll32.exe");
                        let args = to_wide("user32.dll,LockWorkStation");
                        let _ = ShellExecuteW(
                            None,
                            PCWSTR(verb.as_ptr()),
                            PCWSTR(file.as_ptr()),
                            PCWSTR(args.as_ptr()),
                            PCWSTR::null(),
                            SW_SHOWNORMAL,
                        );
                    }
                    IDM_DOCK_SYNC_WIN_PINS => {
                        let win_pins = read_windows_taskbar_pins();
                        if !win_pins.is_empty() {
                            cfg.mac_dock_pinned = win_pins;
                        }
                        cfg.dock_sync_win_pins = true;
                        self.clear_icon_cache();
                        save_config(cfg);
                        action = MacDockAction::ConfigUpdated;
                    }
                    IDM_DOCK_SETTINGS => { keep_focus = true; action = MacDockAction::OpenSettingsDock; }
                    IDM_DOCK_EXIT_DOCK => {
                        cfg.apply_taskbar_theme_preset(TaskbarTheme::AutumnCrystal);
                        save_config(cfg);
                        action = MacDockAction::ConfigUpdated;
                    }
                    _ => {}
                }
                }
            }
            if !keep_focus {
                restore_prev_fg(menu_hw, prev_fg);
            }
        }
        self.menu_open = false;
        self.dirty_windows = true;
        self.mark_dirty_paint();
        action
    }

    fn show_empty_dock_menu(&mut self, pt: POINT, cfg: &mut AppConfig) -> MacDockAction {
        self.menu_open = true;
        let (menu_hw, menu_mon) = (self.menu_hwnd(), self.menu_mon_rect());
        let mut action = MacDockAction::None;
        let mut keep_focus = false;
        unsafe {
            let prev_fg = force_foreground_for_menu(menu_hw);
            if let Ok(hmenu) = CreatePopupMenu() {
                add_menu_item(hmenu, IDM_DOCK_TASKMGR, "Task Manager");
                add_menu_item(hmenu, IDM_DOCK_SHOW_DESKTOP, "Show Desktop");
                add_menu_item(hmenu, IDM_DOCK_EXPLORER, "File Explorer");
                let _ = AppendMenuW(hmenu, MF_SEPARATOR, 0, PCWSTR::null());

                if let Ok(edge_sub) = CreatePopupMenu() {
                    add_checked_item(edge_sub, IDM_DOCK_EDGE_BOTTOM, "Bottom Edge", cfg.mac_dock_edge == MacDockEdge::Bottom);
                    add_checked_item(edge_sub, IDM_DOCK_EDGE_TOP, "Top Edge", cfg.mac_dock_edge == MacDockEdge::Top);
                    add_checked_item(edge_sub, IDM_DOCK_EDGE_LEFT, "Left Edge", cfg.mac_dock_edge == MacDockEdge::Left);
                    add_checked_item(edge_sub, IDM_DOCK_EDGE_RIGHT, "Right Edge", cfg.mac_dock_edge == MacDockEdge::Right);
                    let lbl = to_wide("Dock Position");
                    let _ = AppendMenuW(hmenu, MF_POPUP, edge_sub.0 as usize, PCWSTR(lbl.as_ptr()));
                }
                if let Ok(hide_sub) = CreatePopupMenu() {
                    add_checked_item(hide_sub, IDM_DOCK_HIDE_SMART, "Smart Auto-Hide", cfg.mac_dock_hide == MacDockHideMode::SmartAutoHide);
                    add_checked_item(hide_sub, IDM_DOCK_HIDE_HOVER, "Edge Hover Reveal", cfg.mac_dock_hide == MacDockHideMode::HoverReveal);
                    add_checked_item(hide_sub, IDM_DOCK_HIDE_ALWAYS, "Always Visible", cfg.mac_dock_hide == MacDockHideMode::AlwaysVisible);
                    add_checked_item(hide_sub, IDM_DOCK_HIDE_STEALTH, "Full Stealth Hide (Ctrl+Alt+T)", cfg.mac_dock_hide == MacDockHideMode::StealthHide);
                    let lbl = to_wide("Auto-Hide Behavior");
                    let _ = AppendMenuW(hmenu, MF_POPUP, hide_sub.0 as usize, PCWSTR(lbl.as_ptr()));
                }

                let _ = AppendMenuW(hmenu, MF_SEPARATOR, 0, PCWSTR::null());
                append_widget_customize_submenu(hmenu, TaskbarWidgetId::ShelfFrame, cfg);
                append_widget_customize_submenu(hmenu, TaskbarWidgetId::HudWing, cfg);
                append_widget_customize_submenu(hmenu, TaskbarWidgetId::Utilities, cfg);
                add_menu_item(hmenu, IDM_W_RESET_ALL, "Reset All Widgets to Theme Default");
                let _ = AppendMenuW(hmenu, MF_SEPARATOR, 0, PCWSTR::null());
                add_menu_item(hmenu, IDM_DOCK_SYNC_WIN_PINS, "Sync Pins from Windows Taskbar");
                add_menu_item(hmenu, IDM_DOCK_SETTINGS, "Customize Dock Theme...");
                add_menu_item(hmenu, IDM_DOCK_WIN_TB_SETTINGS, "Windows Taskbar Settings...");
                add_menu_item(hmenu, IDM_DOCK_EXIT_DOCK, "Switch to Standard Taskbar Theme");

                let cmd = track_dock_popup(menu_hw, hmenu, pt, cfg.mac_dock_edge, menu_mon);
                let _ = DestroyMenu(hmenu);
                if let Some(w_act) = self.handle_widget_menu_cmd(cmd, cfg) {
                    action = w_act;
                } else {
                match cmd {
                    IDM_DOCK_TASKMGR => { keep_focus = true; launch_app_path("taskmgr.exe"); }
                    IDM_DOCK_SHOW_DESKTOP => { keep_focus = true; send_win_chord(0x44); }
                    IDM_DOCK_EXPLORER => { keep_focus = true; launch_app_path("explorer.exe"); }
                    IDM_DOCK_EDGE_BOTTOM => {
                        cfg.mac_dock_edge = MacDockEdge::Bottom;
                        save_config(cfg);
                        action = MacDockAction::ConfigUpdated;
                    }
                    IDM_DOCK_EDGE_TOP => {
                        cfg.mac_dock_edge = MacDockEdge::Top;
                        save_config(cfg);
                        action = MacDockAction::ConfigUpdated;
                    }
                    IDM_DOCK_EDGE_LEFT => {
                        cfg.mac_dock_edge = MacDockEdge::Left;
                        save_config(cfg);
                        action = MacDockAction::ConfigUpdated;
                    }
                    IDM_DOCK_EDGE_RIGHT => {
                        cfg.mac_dock_edge = MacDockEdge::Right;
                        save_config(cfg);
                        action = MacDockAction::ConfigUpdated;
                    }
                    IDM_DOCK_HIDE_SMART => {
                        cfg.mac_dock_hide = MacDockHideMode::SmartAutoHide;
                        cfg.sync_hide_mode_from_dock();
                        save_config(cfg);
                        action = MacDockAction::ConfigUpdated;
                    }
                    IDM_DOCK_HIDE_HOVER => {
                        cfg.mac_dock_hide = MacDockHideMode::HoverReveal;
                        cfg.sync_hide_mode_from_dock();
                        save_config(cfg);
                        action = MacDockAction::ConfigUpdated;
                    }
                    IDM_DOCK_HIDE_ALWAYS => {
                        cfg.mac_dock_hide = MacDockHideMode::AlwaysVisible;
                        cfg.sync_hide_mode_from_dock();
                        save_config(cfg);
                        action = MacDockAction::ConfigUpdated;
                    }
                    IDM_DOCK_HIDE_STEALTH => {
                        cfg.mac_dock_hide = MacDockHideMode::StealthHide;
                        cfg.sync_hide_mode_from_dock();
                        save_config(cfg);
                        action = MacDockAction::ConfigUpdated;
                    }
                    IDM_DOCK_SYNC_WIN_PINS => {
                        let win_pins = read_windows_taskbar_pins();
                        if !win_pins.is_empty() {
                            cfg.mac_dock_pinned = win_pins;
                        }
                        cfg.dock_sync_win_pins = true;
                        self.clear_icon_cache();
                        save_config(cfg);
                        action = MacDockAction::ConfigUpdated;
                    }
                    IDM_DOCK_SETTINGS => { keep_focus = true; action = MacDockAction::OpenSettingsDock; }
                    IDM_DOCK_WIN_TB_SETTINGS => { keep_focus = true; launch_app_path("ms-settings:taskbar"); }
                    IDM_DOCK_EXIT_DOCK => {
                        cfg.apply_taskbar_theme_preset(TaskbarTheme::AutumnCrystal);
                        save_config(cfg);
                        action = MacDockAction::ConfigUpdated;
                    }
                    _ => {}
                }
                }
            }
            if !keep_focus {
                restore_prev_fg(menu_hw, prev_fg);
            }
        }
        self.menu_open = false;
        self.dirty_windows = true;
        self.mark_dirty_paint();
        action
    }

    fn show_tray_menu(&mut self, pt: POINT, cfg: &mut AppConfig) -> MacDockAction {
        self.menu_open = true;
        let (menu_hw, menu_mon) = (self.menu_hwnd(), self.menu_mon_rect());
        let mut action = MacDockAction::None;
        let mut keep_focus = false;
        unsafe {
            let prev_fg = force_foreground_for_menu(menu_hw);
            if let Ok(hmenu) = CreatePopupMenu() {
                add_menu_item(
                    hmenu,
                    IDM_DOCK_TRAY_MUTE,
                    if self.tray_vol_muted {
                        "Unmute Master Volume"
                    } else {
                        "Mute Master Volume"
                    },
                );
                add_menu_item(hmenu, IDM_DOCK_TRAY_VOL_MIXER, "Open Volume Mixer...");
                add_menu_item(hmenu, IDM_DOCK_TRAY_SOUND, "Sound Settings...");
                let _ = AppendMenuW(hmenu, MF_SEPARATOR, 0, PCWSTR::null());
                add_menu_item(hmenu, IDM_DOCK_TRAY_NET, "Network & Internet Settings...");
                if self.tray_bat_pct.is_some() {
                    add_menu_item(hmenu, IDM_DOCK_TRAY_POWER, "Power & Battery Settings...");
                }
                let _ = AppendMenuW(hmenu, MF_SEPARATOR, 0, PCWSTR::null());
                append_widget_customize_submenu(hmenu, TaskbarWidgetId::SystemTray, cfg);
                add_menu_item(hmenu, IDM_DOCK_TASKMGR, "Task Manager");
                add_menu_item(hmenu, IDM_DOCK_SETTINGS, "Customize Dock Theme...");
                add_menu_item(hmenu, IDM_DOCK_EXIT_DOCK, "Switch to Standard Taskbar Theme");
                let cmd = track_dock_popup(menu_hw, hmenu, pt, cfg.mac_dock_edge, menu_mon);
                let _ = DestroyMenu(hmenu);
                if let Some(w_act) = self.handle_widget_menu_cmd(cmd, cfg) {
                    action = w_act;
                } else {
                match cmd {
                    IDM_DOCK_TRAY_MUTE => {
                        let prev_m = self.tray_vol_muted;
                        self.tray_vol_muted = !self.tray_vol_muted;
                        self.tray_anim.on_volume_change(
                            self.tray_vol_pct,
                            self.tray_vol_pct,
                            prev_m,
                            self.tray_vol_muted,
                            true,
                        );
                        self.vol_osd_until = Some(Instant::now() + Duration::from_millis(1450));
                        action = MacDockAction::TrayToggleMute;
                    }
                    IDM_DOCK_TRAY_VOL_MIXER => {
                        keep_focus = true;
                        launch_app_path("ms-settings:apps-volume");
                    }
                    IDM_DOCK_TRAY_SOUND => {
                        keep_focus = true;
                        launch_app_path("ms-settings:sound");
                    }
                    IDM_DOCK_TRAY_NET => {
                        keep_focus = true;
                        launch_app_path("ms-settings:network");
                    }
                    IDM_DOCK_TRAY_POWER => {
                        keep_focus = true;
                        launch_app_path("ms-settings:powersleep");
                    }
                    IDM_DOCK_TASKMGR => {
                        keep_focus = true;
                        launch_app_path("taskmgr.exe");
                    }
                    IDM_DOCK_SETTINGS => {
                        keep_focus = true;
                        action = MacDockAction::OpenSettingsDock;
                    }
                    IDM_DOCK_EXIT_DOCK => {
                        cfg.apply_taskbar_theme_preset(TaskbarTheme::AutumnCrystal);
                        save_config(cfg);
                        action = MacDockAction::ConfigUpdated;
                    }
                    _ => {}
                }
                }
            }
            if !keep_focus {
                restore_prev_fg(menu_hw, prev_fg);
            }
        }
        self.menu_open = false;
        self.dirty_windows = true;
        self.mark_dirty_paint();
        action
    }

    fn show_clock_menu(&mut self, pt: POINT, cfg: &mut AppConfig) -> MacDockAction {
        self.menu_open = true;
        let (menu_hw, menu_mon) = (self.menu_hwnd(), self.menu_mon_rect());
        let mut action = MacDockAction::None;
        let mut keep_focus = false;
        unsafe {
            let prev_fg = force_foreground_for_menu(menu_hw);
            if let Ok(hmenu) = CreatePopupMenu() {
                add_menu_item(hmenu, IDM_DOCK_DATETIME, "Adjust Date and Time...");
                add_menu_item(hmenu, IDM_DOCK_SHOW_DESKTOP, "Show Desktop");
                add_menu_item(hmenu, IDM_DOCK_TASKMGR, "Task Manager");
                let _ = AppendMenuW(hmenu, MF_SEPARATOR, 0, PCWSTR::null());
                append_widget_customize_submenu(hmenu, TaskbarWidgetId::Clock, cfg);
                add_menu_item(hmenu, IDM_DOCK_SYNC_WIN_PINS, "Sync Pins from Windows Taskbar");
                add_menu_item(hmenu, IDM_DOCK_SETTINGS, "Customize Dock Theme...");
                add_menu_item(hmenu, IDM_DOCK_EXIT_DOCK, "Switch to Standard Taskbar Theme");
                let cmd = track_dock_popup(menu_hw, hmenu, pt, cfg.mac_dock_edge, menu_mon);
                let _ = DestroyMenu(hmenu);
                if let Some(w_act) = self.handle_widget_menu_cmd(cmd, cfg) {
                    action = w_act;
                } else {
                match cmd {
                    IDM_DOCK_DATETIME => {
                        keep_focus = true;
                        launch_app_path("ms-settings:dateandtime");
                    }
                    IDM_DOCK_SHOW_DESKTOP => {
                        keep_focus = true;
                        send_win_chord(0x44);
                    }
                    IDM_DOCK_TASKMGR => {
                        keep_focus = true;
                        launch_app_path("taskmgr.exe");
                    }
                    IDM_DOCK_SYNC_WIN_PINS => {
                        let win_pins = read_windows_taskbar_pins();
                        if !win_pins.is_empty() {
                            cfg.mac_dock_pinned = win_pins;
                        }
                        cfg.dock_sync_win_pins = true;
                        self.clear_icon_cache();
                        save_config(cfg);
                        action = MacDockAction::ConfigUpdated;
                    }
                    IDM_DOCK_SETTINGS => {
                        keep_focus = true;
                        action = MacDockAction::OpenSettingsDock;
                    }
                    IDM_DOCK_EXIT_DOCK => {
                        cfg.apply_taskbar_theme_preset(TaskbarTheme::AutumnCrystal);
                        save_config(cfg);
                        action = MacDockAction::ConfigUpdated;
                    }
                    _ => {}
                }
                }
            }
            if !keep_focus {
                restore_prev_fg(menu_hw, prev_fg);
            }
        }
        self.menu_open = false;
        self.dirty_windows = true;
        self.mark_dirty_paint();
        action
    }

    fn show_trash_menu(
        &mut self,
        pt: POINT,
        has_items: bool,
        cfg: &mut AppConfig,
    ) -> MacDockAction {
        self.menu_open = true;
        let (menu_hw, menu_mon) = (self.menu_hwnd(), self.menu_mon_rect());
        let mut action = MacDockAction::None;
        let mut keep_focus = false;
        unsafe {
            let prev_fg = force_foreground_for_menu(menu_hw);
            if let Ok(hmenu) = CreatePopupMenu() {
                add_menu_item(hmenu, IDM_DOCK_FOCUS, "Open Recycle Bin");
                if has_items {
                    add_menu_item(hmenu, IDM_DOCK_EMPTY_TRASH, "Empty Recycle Bin");
                } else {
                    add_grayed_item(hmenu, IDM_DOCK_EMPTY_TRASH, "Empty Recycle Bin");
                }
                let _ = AppendMenuW(hmenu, MF_SEPARATOR, 0, PCWSTR::null());
                append_widget_customize_submenu(hmenu, TaskbarWidgetId::Utilities, cfg);
                add_menu_item(hmenu, IDM_DOCK_TASKMGR, "Task Manager");
                add_menu_item(hmenu, IDM_DOCK_SETTINGS, "Customize Dock Theme...");
                add_menu_item(hmenu, IDM_DOCK_EXIT_DOCK, "Switch to Standard Taskbar Theme");
                let cmd = track_dock_popup(menu_hw, hmenu, pt, cfg.mac_dock_edge, menu_mon);
                let _ = DestroyMenu(hmenu);
                if let Some(w_act) = self.handle_widget_menu_cmd(cmd, cfg) {
                    action = w_act;
                } else {
                match cmd {
                    IDM_DOCK_FOCUS => { keep_focus = true; launch_app_path("shell:RecycleBinFolder"); }
                    IDM_DOCK_EMPTY_TRASH => {
                        let _ = SHEmptyRecycleBinW(Some(menu_hw), PCWSTR::null(), 0);
                        self.last_trash_poll = Instant::now() - Duration::from_secs(60);
                    }
                    IDM_DOCK_TASKMGR => { keep_focus = true; launch_app_path("taskmgr.exe"); }
                    IDM_DOCK_SYNC_WIN_PINS => {
                        let win_pins = read_windows_taskbar_pins();
                        if !win_pins.is_empty() {
                            cfg.mac_dock_pinned = win_pins;
                        }
                        cfg.dock_sync_win_pins = true;
                        self.clear_icon_cache();
                        save_config(cfg);
                        action = MacDockAction::ConfigUpdated;
                    }
                    IDM_DOCK_SETTINGS => { keep_focus = true; action = MacDockAction::OpenSettingsDock; }
                    IDM_DOCK_EXIT_DOCK => {
                        cfg.apply_taskbar_theme_preset(TaskbarTheme::AutumnCrystal);
                        save_config(cfg);
                        action = MacDockAction::ConfigUpdated;
                    }
                    _ => {}
                }
                }
            }
            if !keep_focus {
                restore_prev_fg(menu_hw, prev_fg);
            }
        }
        self.menu_open = false;
        self.dirty_windows = true;
        self.mark_dirty_paint();
        action
    }

    fn show_app_menu(
        &mut self,
        pt: POINT,
        name: &str,
        launch_target: &str,
        windows: &[WinEntry],
        is_pinned: bool,
        cfg: &mut AppConfig,
    ) -> MacDockAction {
        self.menu_open = true;
        let (menu_hw, menu_mon) = (self.menu_hwnd(), self.menu_mon_rect());
        let mut action = MacDockAction::None;
        let mut keep_focus = false;

        let effective_target = if !launch_target.is_empty()
            && !launch_target.to_ascii_lowercase().contains("\\windowsapps\\")
        {
            launch_target.to_string()
        } else if let Some(w) = windows.first().filter(|w| !w.aumid.is_empty()) {
            format!("shell:AppsFolder\\{}", w.aumid)
        } else if !launch_target.is_empty() {
            launch_target.to_string()
        } else if let Some(w) = windows.first() {
            w.exe_path.clone()
        } else {
            String::new()
        };
        let resolved = if !effective_target.is_empty() {
            self.resolve_shortcut_target(&effective_target)
        } else {
            String::new()
        };
        // Prefer resolved disk .exe/.lnk for native Windows shell verbs (Pin to Start, Run as Admin, Open Location, Properties)
        let shell_disk_path = if !resolved.is_empty() && Path::new(&resolved).exists() {
            resolved.clone()
        } else if !effective_target.is_empty() && Path::new(&effective_target).exists() {
            effective_target.clone()
        } else if let Some(w) = windows.first().filter(|w| !w.exe_path.is_empty() && Path::new(&w.exe_path).exists()) {
            w.exe_path.clone()
        } else {
            String::new()
        };
        let has_disk_file = !shell_disk_file_empty(&shell_disk_path);
        let start_label = if is_in_start_programs(name) {
            "Unpin from Start Menu"
        } else {
            "Pin to Start"
        };

        unsafe {
            let prev_fg = force_foreground_for_menu(menu_hw);
            if let Ok(hmenu) = CreatePopupMenu() {
                if !windows.is_empty() {
                    if windows.len() > 1 {
                        for (wi, w) in windows.iter().take(8).enumerate() {
                            let clean_t = truncate_title(&w.title, 34);
                            add_menu_item(hmenu, IDM_WIN_SWITCH_BASE + wi, &clean_t);
                        }
                        let _ = AppendMenuW(hmenu, MF_SEPARATOR, 0, PCWSTR::null());
                        add_menu_item(hmenu, IDM_DOCK_RESTORE_ALL, "Restore All Windows");
                        add_menu_item(hmenu, IDM_DOCK_MINIMIZE, "Minimize All Windows");
                        add_menu_item(hmenu, IDM_DOCK_CLOSE_WIN, &format!("Close all {} windows", windows.len()));
                    } else if let Some(first) = windows.first() {
                        add_menu_item(hmenu, IDM_DOCK_FOCUS, &format!("Focus {}", name));
                        if first.is_iconic {
                            add_menu_item(hmenu, IDM_DOCK_MAX_RESTORE, "Restore Window");
                        } else {
                            let is_zoomed = IsZoomed(first.hwnd).as_bool();
                            add_menu_item(
                                hmenu,
                                IDM_DOCK_MAX_RESTORE,
                                if is_zoomed { "Restore Size" } else { "Maximize Window" },
                            );
                            add_menu_item(hmenu, IDM_DOCK_MINIMIZE, "Minimize Window");
                        }
                        add_menu_item(hmenu, IDM_DOCK_CLOSE_WIN, "Close Window");
                    }
                    let _ = AppendMenuW(hmenu, MF_SEPARATOR, 0, PCWSTR::null());
                    if !effective_target.is_empty() {
                        add_menu_item(hmenu, IDM_DOCK_NEW_INST, &format!("Open new {}", name));
                    }
                } else if !effective_target.is_empty() {
                    add_menu_item(hmenu, IDM_DOCK_NEW_INST, &format!("Open {}", name));
                }

                if has_disk_file {
                    add_menu_item(hmenu, IDM_DOCK_RUN_ADMIN, "Run as administrator");
                    add_menu_item(hmenu, IDM_DOCK_OPEN_LOC, "Open file location");
                }

                let _ = AppendMenuW(hmenu, MF_SEPARATOR, 0, PCWSTR::null());
                add_menu_item(
                    hmenu,
                    IDM_DOCK_PIN_TOGGLE,
                    if is_pinned {
                        "Unpin from Taskbar"
                    } else {
                        "Pin to Taskbar"
                    },
                );
                if has_disk_file || !effective_target.is_empty() {
                    add_menu_item(hmenu, IDM_DOCK_START_PIN, start_label);
                }
                if has_disk_file {
                    add_menu_item(hmenu, IDM_DOCK_PROPERTIES, "Properties");
                    add_menu_item(hmenu, IDM_DOCK_MORE_SHELL, "More Windows Options...");
                }

                let _ = AppendMenuW(hmenu, MF_SEPARATOR, 0, PCWSTR::null());
                append_widget_customize_submenu(hmenu, TaskbarWidgetId::AppDock, cfg);
                add_menu_item(hmenu, IDM_DOCK_TASKMGR, "Task Manager");
                add_menu_item(hmenu, IDM_DOCK_SETTINGS, "Customize Dock Theme...");
                add_menu_item(hmenu, IDM_DOCK_EXIT_DOCK, "Switch to Standard Taskbar Theme");

                let cmd = track_dock_popup(menu_hw, hmenu, pt, cfg.mac_dock_edge, menu_mon);
                let _ = DestroyMenu(hmenu);

                if let Some(w_act) = self.handle_widget_menu_cmd(cmd, cfg) {
                    action = w_act;
                } else if cmd >= IDM_WIN_SWITCH_BASE && cmd < IDM_WIN_SWITCH_BASE + windows.len() {
                    let w = &windows[cmd - IDM_WIN_SWITCH_BASE];
                    keep_focus = true;
                    bring_window_to_front(w.hwnd);
                } else {
                    match cmd {
                        IDM_DOCK_FOCUS => {
                            if let Some(w) = windows.first() {
                                keep_focus = true;
                                bring_window_to_front(w.hwnd);
                            }
                        }
                        IDM_DOCK_NEW_INST => {
                            if !effective_target.is_empty() {
                                keep_focus = true;
                                launch_app_path(&effective_target);
                            }
                        }
                        IDM_DOCK_RESTORE_ALL => {
                            keep_focus = true;
                            for w in windows {
                                bring_window_to_front(w.hwnd);
                            }
                        }
                        IDM_DOCK_MAX_RESTORE => {
                            if let Some(w) = windows.first() {
                                keep_focus = true;
                                if w.is_iconic {
                                    let _ = ShowWindow(w.hwnd, SW_RESTORE);
                                } else if IsZoomed(w.hwnd).as_bool() {
                                    let _ = ShowWindow(w.hwnd, SW_RESTORE);
                                } else {
                                    let _ = ShowWindow(w.hwnd, SW_MAXIMIZE);
                                }
                                bring_window_to_front(w.hwnd);
                            }
                        }
                        IDM_DOCK_MINIMIZE => {
                            for w in windows {
                                let _ = ShowWindow(w.hwnd, SW_MINIMIZE);
                            }
                        }
                        IDM_DOCK_CLOSE_WIN => {
                            for w in windows {
                                let _ = PostMessageW(Some(w.hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
                            }
                        }
                        IDM_DOCK_RUN_ADMIN => {
                            if has_disk_file {
                                keep_focus = true;
                                run_as_admin(&shell_disk_path);
                            }
                        }
                        IDM_DOCK_OPEN_LOC => {
                            if has_disk_file {
                                keep_focus = true;
                                open_file_location(&shell_disk_path);
                            }
                        }
                        IDM_DOCK_START_PIN => {
                            let ok_shell = if has_disk_file {
                                invoke_shell_start_verb(menu_hw, &shell_disk_path)
                            } else {
                                false
                            };
                            if !ok_shell && !effective_target.is_empty() {
                                toggle_start_programs_shortcut(name, &effective_target);
                            }
                        }
                        IDM_DOCK_PROPERTIES => {
                            if has_disk_file {
                                keep_focus = true;
                                show_properties_dialog(menu_hw, &shell_disk_path);
                            }
                        }
                        IDM_DOCK_MORE_SHELL => {
                            if has_disk_file {
                                keep_focus = true;
                                show_native_shell_context_menu(menu_hw, &shell_disk_path, pt, cfg.mac_dock_edge, menu_mon);
                            }
                        }
                        IDM_DOCK_PIN_TOGGLE => {
                            if is_pinned {
                                let low = launch_target.to_ascii_lowercase();
                                cfg.mac_dock_pinned
                                    .retain(|a| a.path.to_ascii_lowercase() != low);
                            } else if !effective_target.is_empty() && cfg.mac_dock_pinned.len() < 16 {
                                let low = effective_target.to_ascii_lowercase();
                                if !cfg.mac_dock_pinned.iter().any(|a| a.path.to_ascii_lowercase() == low) {
                                    cfg.mac_dock_pinned.push(AppShortcut {
                                        name: name.to_string(),
                                        path: effective_target,
                                        offset_x: 0,
                                        offset_y: 0,
                                    });
                                }
                            }
                            cfg.dock_sync_win_pins = false;
                            save_config(cfg);
                            action = MacDockAction::ConfigUpdated;
                        }
                        IDM_DOCK_TASKMGR => {
                            keep_focus = true;
                            launch_app_path("taskmgr.exe");
                        }
                        IDM_DOCK_SETTINGS => {
                            keep_focus = true;
                            action = MacDockAction::OpenSettingsDock;
                        }
                        IDM_DOCK_EXIT_DOCK => {
                            cfg.apply_taskbar_theme_preset(TaskbarTheme::AutumnCrystal);
                            save_config(cfg);
                            action = MacDockAction::ConfigUpdated;
                        }
                        _ => {}
                    }
                }
            }
            if !keep_focus {
                restore_prev_fg(menu_hw, prev_fg);
            }
        }
        self.menu_open = false;
        self.dirty_windows = true;
        self.mark_dirty_paint();
        action
    }

    fn handle_widget_menu_cmd(&mut self, cmd: usize, cfg: &mut AppConfig) -> Option<MacDockAction> {
        if !(4500..4800).contains(&cmd) {
            return None;
        }
        match cmd {
            IDM_W_RESET_ALL => cfg.reset_dock_widget_overrides(),
            IDM_W_SHELF_LAYOUT_INHERIT => cfg.w_shelf.layout_override = None,
            c if (IDM_W_SHELF_LAYOUT_BASE..IDM_W_SHELF_LAYOUT_BASE + DockLayoutKind::ALL.len()).contains(&c) => {
                cfg.w_shelf.layout_override = Some(DockLayoutKind::ALL[c - IDM_W_SHELF_LAYOUT_BASE]);
            }
            IDM_W_SHELF_SHAPE_INHERIT => cfg.w_shelf.shape_override = None,
            c if (IDM_W_SHELF_SHAPE_BASE..IDM_W_SHELF_SHAPE_BASE + DockShelfShape::ALL.len()).contains(&c) => {
                cfg.w_shelf.shape_override = Some(DockShelfShape::ALL[c - IDM_W_SHELF_SHAPE_BASE]);
            }
            c if (IDM_W_APPS_CARD_BASE..IDM_W_APPS_CARD_BASE + DockSlotCardStyle::ALL.len()).contains(&c) => {
                cfg.w_apps.card_style = DockSlotCardStyle::ALL[c - IDM_W_APPS_CARD_BASE];
            }
            IDM_W_APPS_IND_INHERIT => cfg.w_apps.indicator_override = None,
            c if (IDM_W_APPS_IND_BASE..IDM_W_APPS_IND_BASE + DockIndicatorStyle::ALL.len()).contains(&c) => {
                cfg.w_apps.indicator_override = Some(DockIndicatorStyle::ALL[c - IDM_W_APPS_IND_BASE]);
            }
            c if (IDM_W_APPS_DIV_BASE..IDM_W_APPS_DIV_BASE + DockDividerStyle::ALL.len()).contains(&c) => {
                cfg.w_apps.divider_style = DockDividerStyle::ALL[c - IDM_W_APPS_DIV_BASE];
            }
            c if (IDM_W_APPS_LBL_BASE..IDM_W_APPS_LBL_BASE + DockRunningLabelMode::ALL.len()).contains(&c) => {
                cfg.w_apps.label_mode = DockRunningLabelMode::ALL[c - IDM_W_APPS_LBL_BASE];
            }
            IDM_W_APPS_BRAND_GLOW => cfg.w_apps.per_app_brand_glow = !cfg.w_apps.per_app_brand_glow,
            IDM_W_APPS_COUNT_BADGE => cfg.w_apps.show_window_count_badge = !cfg.w_apps.show_window_count_badge,
            IDM_W_APPS_RESET => cfg.w_apps = Default::default(),

            c if (IDM_W_CLOCK_ENG_BASE..IDM_W_CLOCK_ENG_BASE + DockClockEngine::ALL.len()).contains(&c) => {
                cfg.w_clock.engine = DockClockEngine::ALL[c - IDM_W_CLOCK_ENG_BASE];
            }
            IDM_W_CLOCK_SECONDS => cfg.w_clock.show_seconds = !cfg.w_clock.show_seconds,
            IDM_W_CLOCK_DATE => cfg.w_clock.show_date = !cfg.w_clock.show_date,
            IDM_W_CLOCK_24H_TOGGLE => {
                let cur = cfg.w_clock.use_24h.unwrap_or(cfg.widgets_24h);
                cfg.set_clock_24h(!cur);
            }
            c if (IDM_W_CLOCK_WHEEL_BASE..IDM_W_CLOCK_WHEEL_BASE + DockClockWheelAction::ALL.len()).contains(&c) => {
                cfg.w_clock.wheel_action = DockClockWheelAction::ALL[c - IDM_W_CLOCK_WHEEL_BASE];
            }
            IDM_W_CLOCK_RESET => cfg.w_clock = Default::default(),

            c if (IDM_W_TRAY_AUD_BASE..IDM_W_TRAY_AUD_BASE + TrayAudioMeterStyle::ALL.len()).contains(&c) => {
                cfg.w_tray.audio_style = TrayAudioMeterStyle::ALL[c - IDM_W_TRAY_AUD_BASE];
            }
            c if (IDM_W_TRAY_BAT_BASE..IDM_W_TRAY_BAT_BASE + TrayBatteryMeterStyle::ALL.len()).contains(&c) => {
                cfg.w_tray.battery_style = TrayBatteryMeterStyle::ALL[c - IDM_W_TRAY_BAT_BASE];
            }
            c if (IDM_W_TRAY_NET_BASE..IDM_W_TRAY_NET_BASE + TrayNetMeterStyle::ALL.len()).contains(&c) => {
                cfg.w_tray.net_style = TrayNetMeterStyle::ALL[c - IDM_W_TRAY_NET_BASE];
            }
            IDM_W_TRAY_RESET => cfg.w_tray = Default::default(),

            c if (IDM_W_HUD_MODE_BASE..IDM_W_HUD_MODE_BASE + HudWingMode::ALL.len()).contains(&c) => {
                cfg.w_hud.mode = HudWingMode::ALL[c - IDM_W_HUD_MODE_BASE];
            }
            c if (IDM_W_HUD_SHAPE_BASE..IDM_W_HUD_SHAPE_BASE + HudWingShape::ALL.len()).contains(&c) => {
                cfg.w_hud.shape = HudWingShape::ALL[c - IDM_W_HUD_SHAPE_BASE];
            }
            c if (IDM_W_HUD_CONTENT_BASE..IDM_W_HUD_CONTENT_BASE + HudTelemetryContent::ALL.len()).contains(&c) => {
                cfg.w_hud.content = HudTelemetryContent::ALL[c - IDM_W_HUD_CONTENT_BASE];
            }
            IDM_W_HUD_MARQUEE => cfg.w_hud.marquee_ticker = !cfg.w_hud.marquee_ticker,
            IDM_W_HUD_RESET => cfg.w_hud = Default::default(),

            c if (IDM_W_UTIL_START_BASE..IDM_W_UTIL_START_BASE + StartBadgeStyle::ALL.len()).contains(&c) => {
                cfg.w_util.start_style = StartBadgeStyle::ALL[c - IDM_W_UTIL_START_BASE];
            }
            IDM_W_UTIL_TOGGLE_START => cfg.w_util.show_start = !cfg.w_util.show_start,
            IDM_W_UTIL_TOGGLE_SEARCH => cfg.w_util.show_search = !cfg.w_util.show_search,
            IDM_W_UTIL_TOGGLE_TASKVIEW => cfg.w_util.show_taskview = !cfg.w_util.show_taskview,
            IDM_W_UTIL_TOGGLE_TRASH => cfg.w_util.show_trash = !cfg.w_util.show_trash,
            IDM_W_UTIL_TOGGLE_STUDIO => cfg.w_util.show_studio = !cfg.w_util.show_studio,
            IDM_W_UTIL_TOGGLE_SLIVER => cfg.w_util.show_desktop_sliver = !cfg.w_util.show_desktop_sliver,

            c if (IDM_W_THEME_SHELF_BASE..=IDM_W_THEME_SHELF_BASE + TaskbarTheme::VIRTUAL_DOCK_THEMES.len()).contains(&c) => {
                let off = c - IDM_W_THEME_SHELF_BASE;
                cfg.w_shelf.theme_override = if off == 0 { None } else { Some(TaskbarTheme::VIRTUAL_DOCK_THEMES[off - 1]) };
            }
            c if (IDM_W_THEME_APPS_BASE..=IDM_W_THEME_APPS_BASE + TaskbarTheme::VIRTUAL_DOCK_THEMES.len()).contains(&c) => {
                let off = c - IDM_W_THEME_APPS_BASE;
                cfg.w_apps.theme_override = if off == 0 { None } else { Some(TaskbarTheme::VIRTUAL_DOCK_THEMES[off - 1]) };
            }
            c if (IDM_W_THEME_CLOCK_BASE..=IDM_W_THEME_CLOCK_BASE + TaskbarTheme::VIRTUAL_DOCK_THEMES.len()).contains(&c) => {
                let off = c - IDM_W_THEME_CLOCK_BASE;
                cfg.w_clock.theme_override = if off == 0 { None } else { Some(TaskbarTheme::VIRTUAL_DOCK_THEMES[off - 1]) };
            }
            c if (IDM_W_THEME_TRAY_BASE..=IDM_W_THEME_TRAY_BASE + TaskbarTheme::VIRTUAL_DOCK_THEMES.len()).contains(&c) => {
                let off = c - IDM_W_THEME_TRAY_BASE;
                cfg.w_tray.theme_override = if off == 0 { None } else { Some(TaskbarTheme::VIRTUAL_DOCK_THEMES[off - 1]) };
            }
            c if (IDM_W_THEME_HUD_BASE..=IDM_W_THEME_HUD_BASE + TaskbarTheme::VIRTUAL_DOCK_THEMES.len()).contains(&c) => {
                let off = c - IDM_W_THEME_HUD_BASE;
                cfg.w_hud.theme_override = if off == 0 { None } else { Some(TaskbarTheme::VIRTUAL_DOCK_THEMES[off - 1]) };
            }
            _ => return None,
        }
        save_config(cfg);
        self.dirty_windows = true;
        self.mark_dirty_paint();
        Some(MacDockAction::ConfigUpdated)
    }

    fn resolve_icon_bitmap(&mut self, slot_idx: usize) -> Option<ID2D1Bitmap> {
        let Some(slot) = self.slots.get(slot_idx).cloned() else {
            return None;
        };
        let cache_key = match &slot {
            DockSlotKind::PinnedApp { path, windows, .. } => {
                if !path.is_empty() {
                    format!("p:{}", path.to_ascii_lowercase())
                } else if let Some(w) = windows.first() {
                    if !w.aumid.is_empty() {
                        format!("a:{}", w.aumid.to_ascii_lowercase())
                    } else {
                        format!("e:{}", w.exe_path.to_ascii_lowercase())
                    }
                } else {
                    return None;
                }
            }
            DockSlotKind::RunningApp { exe_path, windows, .. } => {
                if let Some(w) = windows.first().filter(|w| !w.aumid.is_empty()) {
                    format!("a:{}", w.aumid.to_ascii_lowercase())
                } else if !exe_path.is_empty() {
                    format!("e:{}", exe_path.to_ascii_lowercase())
                } else if let Some(w) = windows.first() {
                    format!("h:{}", w.hwnd.0 as isize)
                } else {
                    return None;
                }
            }
            _ => return None,
        };

        if let Some(cached) = self.icon_cache.get(&cache_key) {
            return cached.clone();
        }
        if self.icon_cache.len() > 72 {
            self.icon_cache.clear();
            self.icon_in_flight.clear();
        }

        let target_exe = match &slot {
            DockSlotKind::PinnedApp { path, .. } if !path.is_empty() => {
                self.resolve_shortcut_target(path)
            }
            _ => String::new(),
        };

        // Non-blocking async path: never stall the UI thread on SHCreateItemFromParsingName /
        // IShellItemImageFactory / WM_GETICON when an app is launching for the first time!
        if !self.icon_notify_hwnd.0.is_null() && self.icon_notify_msg != 0 {
            if !self.icon_in_flight.contains(&cache_key) {
                let req = match &slot {
                    DockSlotKind::PinnedApp { name, path, windows, .. } => {
                        let w0 = windows.first();
                        Some(DockIconReq {
                            cache_key: cache_key.clone(),
                            app_name: name.clone(),
                            app_path: path.clone(),
                            target_exe: target_exe.clone(),
                            exe_path: w0.map(|w| w.exe_path.clone()).unwrap_or_default(),
                            aumid: w0.map(|w| w.aumid.clone()).unwrap_or_default(),
                            hwnd_raw: w0.map(|w| w.hwnd.0 as isize).unwrap_or(0),
                            icon_hwnd_raw: w0.map(|w| w.icon_hwnd.0 as isize).unwrap_or(0),
                            is_pinned: true,
                        })
                    }
                    DockSlotKind::RunningApp { name, exe_path, windows, .. } => {
                        let w0 = windows.first();
                        Some(DockIconReq {
                            cache_key: cache_key.clone(),
                            app_name: name.clone(),
                            app_path: String::new(),
                            target_exe: String::new(),
                            exe_path: w0.map(|w| w.exe_path.clone()).unwrap_or_else(|| exe_path.clone()),
                            aumid: w0.map(|w| w.aumid.clone()).unwrap_or_default(),
                            hwnd_raw: w0.map(|w| w.hwnd.0 as isize).unwrap_or(0),
                            icon_hwnd_raw: w0.map(|w| w.icon_hwnd.0 as isize).unwrap_or(0),
                            is_pinned: false,
                        })
                    }
                    _ => None,
                };
                if let Some(req) = req {
                    let worker = self.icon_worker.get_or_insert_with(|| {
                        DockIconWorker::spawn(self.icon_notify_hwnd, self.icon_notify_msg)
                    });
                    if worker.req_tx.send(req).is_ok() {
                        self.icon_in_flight.insert(cache_key.clone());
                    }
                }
            }
            // Optional 0ms non-blocking instant placeholder from kernel class HICON (zero IPC/disk I/O)
            if let (Some(wic), Some(rt)) = (self.wic.as_ref(), self.dc_rt.as_ref()) {
                let w0 = match &slot {
                    DockSlotKind::PinnedApp { windows, .. }
                    | DockSlotKind::RunningApp { windows, .. } => windows.first(),
                    _ => None,
                };
                if let Some(w) = w0 {
                    unsafe {
                        let hico_raw = GetClassLongPtrW(w.hwnd, GCLP_HICON);
                        if hico_raw != 0 {
                            if let Some(bmp) = hicon_to_d2d(wic, rt, HICON(hico_raw as *mut c_void)) {
                                return Some(bmp);
                            }
                        }
                    }
                }
            }
            return None;
        }

        let (wic, rt) = match (self.wic.as_ref(), self.dc_rt.as_ref()) {
            (Some(w), Some(r)) => (w, r),
            _ => return None,
        };

        let bmp = unsafe {
            match &slot {
                DockSlotKind::PinnedApp { name, path, windows, .. } => {
                    let mkey = canonical_match_key(name, path, &target_exe);
                    let mut b: Option<ID2D1Bitmap> = None;
                    if let Some(w) = windows.first() {
                        b = load_window_bitmap(wic, rt, w);
                    }
                    if b.is_none() && path.to_ascii_lowercase().starts_with("shell:") {
                        b = load_app_bitmap_for_path(wic, rt, path, false);
                    }
                    if b.is_none() && !target_exe.is_empty() {
                        let low_t = target_exe.to_ascii_lowercase();
                        if !low_t.ends_with("wt.exe")
                            && !low_t.contains("\\windowsapps\\")
                            && Path::new(&target_exe).exists()
                        {
                            b = load_app_bitmap_for_path(wic, rt, &target_exe, false);
                        }
                    }
                    if b.is_none() {
                        let direct_exe = match mkey.as_str() {
                            "explorer.exe" => Some(r"C:\Windows\explorer.exe"),
                            "msedge.exe" => [
                                r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe",
                                r"C:\Program Files\Microsoft\Edge\Application\msedge.exe",
                            ]
                            .into_iter()
                            .find(|p| Path::new(p).exists()),
                            "windowsterminal.exe" => {
                                Some(r"shell:AppsFolder\Microsoft.WindowsTerminal_8wekyb3d8bbwe!App")
                            }
                            _ => None,
                        };
                        if let Some(de) = direct_exe {
                            b = load_app_bitmap_for_path(wic, rt, de, false);
                        }
                    }
                    if b.is_none() {
                        b = load_app_bitmap_for_path(wic, rt, path, false);
                    }
                    b
                }
                DockSlotKind::RunningApp { exe_path, windows, .. } => {
                    let mut b = if let Some(w) = windows.first() {
                        load_window_bitmap(wic, rt, w)
                    } else {
                        None
                    };
                    if b.is_none() && !exe_path.is_empty() {
                        b = load_app_bitmap_for_path(wic, rt, exe_path, false);
                    }
                    b
                }
                _ => None,
            }
        };

        self.icon_cache.insert(cache_key, bmp.clone());
        bmp
    }

    fn render_pane(&mut self, pane: &mut DockMonitorPane, cfg: &AppConfig, shelf_r: D2D_RECT_F) {
        if !self.ensure_graphics() {
            return;
        }
        let cw = (pane.canvas_rect.right - pane.canvas_rect.left).max(4);
        let ch = (pane.canvas_rect.bottom - pane.canvas_rect.top).max(4);
        if !pane.surface.ensure_size(cw, ch) {
            return;
        }

        let bitmaps: Vec<Option<ID2D1Bitmap>> = (0..self.slots.len())
            .map(|i| self.resolve_icon_bitmap(i))
            .collect();
        let pal = effective_taskbar_palette(cfg);
        let u = pane.dpi_u;

        if self.solid_brush.is_none() {
            if let Some(ref rt) = self.dc_rt {
                self.solid_brush = unsafe { rt.CreateSolidColorBrush(&rgba(255, 255, 255, 1.0), None).ok() };
            }
        }
        let ok = unsafe {
            self.paint_pane_surface(
                pane,
                cw,
                ch,
                shelf_r,
                &bitmaps,
                cfg,
                &pal,
                u,
            )
        };
        if !ok {
            if let Some(ref rt) = self.dc_rt {
                crate::realistic_pbr::evict_rt(rt);
            }
            self.solid_brush = None;
            self.dc_rt = None;
            self.icon_cache.clear();
            return;
        }

        unsafe {
            let dst_pt = POINT {
                x: pane.canvas_rect.left,
                y: pane.canvas_rect.top,
            };
            let size = SIZE { cx: cw, cy: ch };
            let src_pt = POINT { x: 0, y: 0 };
            let blend = BLENDFUNCTION {
                BlendOp: AC_SRC_OVER as u8,
                BlendFlags: 0,
                SourceConstantAlpha: 255,
                AlphaFormat: AC_SRC_ALPHA as u8,
            };
            let _ = UpdateLayeredWindow(
                pane.hwnd,
                None,
                Some(&dst_pt),
                Some(&size),
                Some(pane.surface.mem_dc),
                Some(&src_pt),
                COLORREF(0),
                Some(&blend),
                ULW_ALPHA,
            );
            if !pane.window_shown {
                let _ = SetWindowPos(
                    pane.hwnd,
                    Some(HWND_TOPMOST),
                    pane.canvas_rect.left,
                    pane.canvas_rect.top,
                    cw,
                    ch,
                    SWP_NOACTIVATE | SWP_SHOWWINDOW,
                );
                pane.window_shown = true;
                pane.last_win_rect = pane.canvas_rect;
            } else if pane.last_win_rect != pane.canvas_rect {
                let _ = SetWindowPos(
                    pane.hwnd,
                    Some(HWND_TOPMOST),
                    pane.canvas_rect.left,
                    pane.canvas_rect.top,
                    cw,
                    ch,
                    SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE,
                );
                pane.last_win_rect = pane.canvas_rect;
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    unsafe fn paint_pane_surface(
        &self,
        pane: &DockMonitorPane,
        cw: i32,
        ch: i32,
        shelf_r: D2D_RECT_F,
        bitmaps: &[Option<ID2D1Bitmap>],
        cfg: &AppConfig,
        pal: &ThemePalette,
        u: f32,
    ) -> bool {
        let Some(ref rt) = self.dc_rt else {
            return false;
        };
        let rc = RECT {
            left: 0,
            top: 0,
            right: cw,
            bottom: ch,
        };
        if rt.BindDC(pane.surface.mem_dc, &rc).is_err() {
            return false;
        }
        rt.BeginDraw();
        rt.SetAntialiasMode(D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
        rt.Clear(Some(&D2D1_COLOR_F {
            r: 0.0,
            g: 0.0,
            b: 0.0,
            a: 0.0,
        }));
        if cfg.is_dock_all_elements_off() {
            return rt.EndDraw(None, None).is_ok();
        }

        let br_opt = self
            .solid_brush
            .clone()
            .or_else(|| rt.CreateSolidColorBrush(&rgba(255, 255, 255, 1.0), None).ok());
        if let Some(br) = br_opt {
            let is_vertical = matches!(cfg.mac_dock_edge, MacDockEdge::Left | MacDockEdge::Right);
            let spec = resolve_shelf_spec(cfg);

            // Render stepped HUD console telemetry wing attached to shelf
            if let Some((wing_r, ref _tag, ref wing_text)) = pane.hud_wing_local {
                let (_, hud_spec) = resolve_widget_spec(cfg, TaskbarWidgetId::HudWing);
                let wing_alpha = (cfg.w_hud.plate_alpha_pct.clamp(50, 100) as f32) / 100.0;
                let use_chamfer = match cfg.w_hud.shape {
                    HudWingShape::ChamferTech => true,
                    HudWingShape::RoundedCapsule | HudWingShape::MinimalUnderline => false,
                    HudWingShape::InheritTheme => hud_spec.shape == DockShelfShape::Chamfer,
                };
                if cfg.w_hud.shape == HudWingShape::MinimalUnderline {
                    let bg_rr = D2D1_ROUNDED_RECT {
                        rect: wing_r,
                        radiusX: 3.0 * u,
                        radiusY: 3.0 * u,
                    };
                    br.SetColor(&dt_rgba(
                        hud_spec.bg_bot.0,
                        hud_spec.bg_bot.1,
                        hud_spec.bg_bot.2,
                        wing_alpha * 0.72,
                    ));
                    rt.FillRoundedRectangle(&bg_rr, &br);
                    br.SetColor(&dt_rgba(
                        hud_spec.dot_active.0,
                        hud_spec.dot_active.1,
                        hud_spec.dot_active.2,
                        0.94,
                    ));
                    rt.DrawLine(
                        Vector2 {
                            X: wing_r.left + 8.0 * u,
                            Y: wing_r.bottom - 1.2 * u,
                        },
                        Vector2 {
                            X: wing_r.right - 8.0 * u,
                            Y: wing_r.bottom - 1.2 * u,
                        },
                        &br,
                        1.8 * u,
                        None,
                    );
                } else {
                    let filled = if use_chamfer {
                        if let Some(f) = self.d2d.as_ref() {
                            if let Some(geo) = chamfer_geo(f, wing_r, 8.0 * u) {
                                br.SetColor(&dt_rgba(
                                    hud_spec.bg_bot.0,
                                    hud_spec.bg_bot.1,
                                    hud_spec.bg_bot.2,
                                    wing_alpha,
                                ));
                                rt.FillGeometry(&geo, &br, None);
                                br.SetColor(&dt_rgba(
                                    hud_spec.rim_outer.0,
                                    hud_spec.rim_outer.1,
                                    hud_spec.rim_outer.2,
                                    0.90,
                                ));
                                rt.DrawGeometry(&geo, &br, 1.25 * u, None);
                                true
                            } else {
                                false
                            }
                        } else {
                            false
                        }
                    } else {
                        false
                    };
                    if !filled {
                        let rad_w = if cfg.w_hud.shape == HudWingShape::RoundedCapsule {
                            9.0 * u
                        } else {
                            4.0 * u
                        };
                        let rr = D2D1_ROUNDED_RECT {
                            rect: wing_r,
                            radiusX: rad_w,
                            radiusY: rad_w,
                        };
                        br.SetColor(&dt_rgba(
                            hud_spec.bg_bot.0,
                            hud_spec.bg_bot.1,
                            hud_spec.bg_bot.2,
                            wing_alpha,
                        ));
                        rt.FillRoundedRectangle(&rr, &br);
                        br.SetColor(&dt_rgba(
                            hud_spec.rim_outer.0,
                            hud_spec.rim_outer.1,
                            hud_spec.rim_outer.2,
                            0.88,
                        ));
                        rt.DrawRoundedRectangle(&rr, &br, 1.2 * u, None);
                    }
                }
                let pulse = 0.65 + 0.35 * (self.dock_anim_phase * 2.2).sin().abs();
                br.SetColor(&dt_rgba(
                    hud_spec.dot_active.0,
                    hud_spec.dot_active.1,
                    hud_spec.dot_active.2,
                    pulse,
                ));
                let dot_cx = wing_r.left + 12.0 * u;
                let dot_cy = (wing_r.top + wing_r.bottom) * 0.5;
                rt.FillEllipse(
                    &windows::Win32::Graphics::Direct2D::D2D1_ELLIPSE {
                        point: Vector2 { X: dot_cx, Y: dot_cy },
                        radiusX: 2.6 * u,
                        radiusY: 2.6 * u,
                    },
                    &br,
                );
                if let Some(ref tfm) = self.tf_mono {
                    br.SetColor(&rgba(pal.text_primary.0, pal.text_primary.1, pal.text_primary.2, 0.96));
                    let text_r = D2D_RECT_F {
                        left: wing_r.left + 19.0 * u,
                        top: wing_r.top,
                        right: wing_r.right - 12.0 * u,
                        bottom: wing_r.bottom,
                    };
                    let _ = tfm.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_LEADING);
                    draw_d2d_text(rt, wing_text, tfm, &text_r, &br);
                    let _ = tfm.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER);
                }
            }

            // Render island shelves (empty list for barless IndependentOrbs)
            let click_ripple = if pane.click_ripple_t < 1.0 {
                Some((pane.click_ripple_axis, pane.click_ripple_t))
            } else {
                None
            };
            for &island_r in &pane.island_rects_local {
                draw_dock_shelf_and_art_with_ripple(
                    rt,
                    self.d2d.as_ref(),
                    &br,
                    island_r,
                    cfg,
                    pal,
                    self.dock_anim_phase,
                    pane.hover_mix,
                    pane.cursor_axis_norm,
                    is_vertical,
                    u,
                    false,
                    click_ripple,
                );
            }

            let (util_theme, util_spec) = resolve_widget_spec(cfg, TaskbarWidgetId::Utilities);
            let (_, tray_spec) = resolve_widget_spec(cfg, TaskbarWidgetId::SystemTray);
            let draw_util_plate = util_spec.shape != DockShelfShape::FloatingZero
                || util_theme == TaskbarTheme::InvisibleGhostDock;

            // Render slots (dividers, custom containers, icons, inline labels, clock, indicator dots)
            for item in &pane.layout {
                if item.is_spacer {
                    continue;
                }
                let Some(slot) = self.slots.get(item.slot_idx) else {
                    continue;
                };
                if item.is_divider {
                    draw_dock_divider_styled(
                        rt,
                        &br,
                        item.center_x,
                        item.center_y,
                        shelf_r,
                        cfg,
                        is_vertical,
                        u,
                    );
                    continue;
                }

                let sz = item.icon_size;
                let dest = D2D_RECT_F {
                    left: item.center_x - sz * 0.5,
                    top: item.center_y - sz * 0.5,
                    right: item.center_x + sz * 0.5,
                    bottom: item.center_y + sz * 0.5,
                };
                let is_hov = pane.hovered_idx == Some(item.slot_idx);
                let slot_widget_id = match slot {
                    DockSlotKind::PinnedApp { .. } | DockSlotKind::RunningApp { .. } => {
                        TaskbarWidgetId::AppDock
                    }
                    DockSlotKind::ClockModule { .. } => TaskbarWidgetId::Clock,
                    DockSlotKind::HiddenTrayChevron | DockSlotKind::TrayStatusCluster => {
                        TaskbarWidgetId::SystemTray
                    }
                    _ => TaskbarWidgetId::Utilities,
                };
                let brand_rgb = resolve_slot_brand_rgb(slot, cfg);

                draw_dock_slot_fx_styled(
                    rt,
                    self.d2d.as_ref(),
                    &br,
                    item.slot_rect,
                    item.center_x,
                    item.center_y,
                    sz,
                    shelf_r,
                    cfg,
                    slot_widget_id,
                    brand_rgb,
                    is_hov,
                    item.is_active,
                    item.has_windows,
                    item.is_flashing,
                    is_vertical,
                    self.dock_anim_phase,
                    item.slot_idx,
                    u,
                );

                let draw_inline_text = |text: &str, alpha: f32| {
                    if alpha > 0.04 && !text.is_empty() {
                        if let Some(ref tfl) = self.tf_left {
                            br.SetColor(&rgba(
                                pal.text_primary.0,
                                pal.text_primary.1,
                                pal.text_primary.2,
                                alpha * if item.is_active || is_hov { 0.98 } else { 0.88 },
                            ));
                            let tr = D2D_RECT_F {
                                left: dest.right + 6.0 * u,
                                top: item.slot_rect.top,
                                right: item.slot_rect.right - 7.0 * u,
                                bottom: item.slot_rect.bottom,
                            };
                            if tr.right > tr.left + 12.0 * u {
                                draw_d2d_text(rt, text, tfl, &tr, &br);
                            }
                        }
                    }
                };

                match slot {
                    DockSlotKind::StartLauncher => {
                        draw_start_badge(
                            rt,
                            &br,
                            dest,
                            util_spec.dot_active,
                            util_spec.art == DockArtLayer::PhosphorCrt,
                            cfg.w_util.start_style,
                            draw_util_plate,
                            u,
                        );
                        draw_inline_text(&item.inline_text, item.inline_alpha);
                    }
                    DockSlotKind::SearchButton => {
                        draw_search_badge(rt, &br, dest, pal.text_primary, util_spec.dot_active, u);
                        draw_inline_text(&item.inline_text, item.inline_alpha);
                    }
                    DockSlotKind::TaskViewButton => {
                        draw_taskview_badge(rt, &br, dest, pal.text_primary, util_spec.dot_active, u);
                        draw_inline_text(&item.inline_text, item.inline_alpha);
                    }
                    DockSlotKind::HiddenTrayChevron => {
                        draw_hidden_tray_chevron_badge(
                            rt,
                            &br,
                            item.slot_rect,
                            pal.text_primary,
                            util_spec.dot_active,
                            is_hov,
                            cfg.mac_dock_edge,
                            u,
                        );
                    }
                    DockSlotKind::TrayStatusCluster => {
                        let show_osd = self
                            .vol_osd_until
                            .map(|t| t > Instant::now())
                            .unwrap_or(false)
                            || self.tray_anim.vol_badge_ttl > 0.01;
                        draw_tray_cluster(
                            rt,
                            self.d2d.as_ref(),
                            &br,
                            self.tf_mono.as_ref(),
                            item.slot_rect,
                            self.tray_vol_pct,
                            self.tray_vol_muted,
                            self.tray_wifi_on,
                            self.tray_wifi_signal,
                            self.tray_bat_pct,
                            self.tray_bat_charging,
                            show_osd,
                            is_hov,
                            pal.text_primary,
                            pal.text_muted,
                            tray_spec.dot_active,
                            cfg,
                            &self.tray_anim,
                            self.dock_anim_phase,
                            is_vertical,
                            u,
                        );
                    }
                    DockSlotKind::ActiveAppChip { title } => {
                        let pulse = 0.68 + 0.32 * (self.dock_anim_phase * 2.0).sin().abs();
                        br.SetColor(&dt_rgba(spec.dot_active.0, spec.dot_active.1, spec.dot_active.2, pulse));
                        let dcx = item.slot_rect.left + 12.0 * u;
                        let dcy = (item.slot_rect.top + item.slot_rect.bottom) * 0.5;
                        rt.FillEllipse(
                            &windows::Win32::Graphics::Direct2D::D2D1_ELLIPSE {
                                point: Vector2 { X: dcx, Y: dcy },
                                radiusX: 3.4 * u,
                                radiusY: 3.4 * u,
                            },
                            &br,
                        );
                        if let Some(ref tfl) = self.tf_left {
                            br.SetColor(&rgba(pal.text_primary.0, pal.text_primary.1, pal.text_primary.2, 0.94));
                            let tr = D2D_RECT_F {
                                left: item.slot_rect.left + 21.0 * u,
                                top: item.slot_rect.top,
                                right: item.slot_rect.right - 7.0 * u,
                                bottom: item.slot_rect.bottom,
                            };
                            draw_d2d_text(rt, title, tfl, &tr, &br);
                        }
                    }
                    DockSlotKind::TrashBin { items } => {
                        draw_trash_badge(rt, &br, dest, *items > 0, util_spec.light_mode_paper, u);
                        draw_inline_text(&item.inline_text, item.inline_alpha);
                    }
                    DockSlotKind::StudioLauncher => {
                        draw_studio_badge(rt, &br, dest, pal.accent, draw_util_plate, u);
                        draw_inline_text(&item.inline_text, item.inline_alpha);
                    }
                    DockSlotKind::ClockModule {
                        time_str,
                        date_str,
                        hour24,
                        minute,
                        second,
                    } => {
                        draw_dock_clock_module(
                            rt,
                            &br,
                            self.tf_mono.as_ref(),
                            item.slot_rect,
                            cfg,
                            pal,
                            time_str,
                            date_str,
                            *hour24,
                            *minute,
                            *second,
                            self.dock_anim_phase,
                            is_vertical,
                            u,
                        );
                    }
                    DockSlotKind::ShowDesktopSliver => {
                        br.SetColor(&rgba(
                            pal.rim.0,
                            pal.rim.1,
                            pal.rim.2,
                            if is_hov { 0.85 } else { 0.36 },
                        ));
                        if is_vertical {
                            rt.DrawLine(
                                Vector2 { X: item.slot_rect.left + 3.0 * u, Y: item.slot_rect.top + 1.0 * u },
                                Vector2 { X: item.slot_rect.right - 3.0 * u, Y: item.slot_rect.top + 1.0 * u },
                                &br,
                                1.1 * u,
                                None,
                            );
                        } else {
                            rt.DrawLine(
                                Vector2 { X: item.slot_rect.left + 1.5 * u, Y: item.slot_rect.top + 4.0 * u },
                                Vector2 { X: item.slot_rect.left + 1.5 * u, Y: item.slot_rect.bottom - 4.0 * u },
                                &br,
                                1.2 * u,
                                None,
                            );
                        }
                        if is_hov {
                            br.SetColor(&rgba(pal.accent_light.0, pal.accent_light.1, pal.accent_light.2, 0.22));
                            rt.FillRectangle(&item.slot_rect, &br);
                        }
                    }
                    DockSlotKind::PinnedApp { .. } | DockSlotKind::RunningApp { .. } => {
                        if cfg.w_apps.show_icons {
                            if let Some(Some(bmp)) = bitmaps.get(item.slot_idx) {
                                let pad_i = sz * 0.08;
                                let ico_r = D2D_RECT_F {
                                    left: dest.left + pad_i,
                                    top: dest.top + pad_i,
                                    right: dest.right - pad_i,
                                    bottom: dest.bottom - pad_i,
                                };
                                rt.DrawBitmap(
                                    bmp,
                                    Some(&ico_r),
                                    1.0,
                                    D2D1_BITMAP_INTERPOLATION_MODE_LINEAR,
                                    None,
                                );
                            } else {
                                draw_fallback_badge(rt, &br, dest, u);
                            }
                            draw_inline_text(&item.inline_text, item.inline_alpha);
                        }
                    }
                    DockSlotKind::Divider
                    | DockSlotKind::EndDivider
                    | DockSlotKind::IslandGap
                    | DockSlotKind::FlexSpacer => {}
                }

                if cfg.mac_dock_dots
                    && cfg.w_apps.show_indicators
                    && item.dot_count > 0
                    && !matches!(slot, DockSlotKind::ActiveAppChip { .. })
                {
                    draw_dock_indicator_styled(
                        rt,
                        &br,
                        cfg,
                        brand_rgb,
                        item.slot_idx,
                        item.center_x,
                        item.center_y,
                        item.slot_rect,
                        shelf_r,
                        cfg.mac_dock_edge,
                        item.dot_count,
                        item.is_active,
                        u,
                    );
                }
            }

            // Sleek floating tooltip pill (skipped when label is already shown inline or menu is open)
            if pane.hover_mix > 0.25 && !self.menu_open {
                if let Some(hidx) = pane.hovered_idx {
                    if let Some(hitem) = pane
                        .layout
                        .iter()
                        .find(|it| it.slot_idx == hidx && !it.is_divider && !it.is_spacer)
                    {
                        if hitem.inline_alpha < 0.45
                            && !(self.thumb_popup.visible
                                && self.thumb_owner_hwnd == pane.hwnd
                                && hitem.has_windows)
                            && !hitem.label.is_empty()
                        {
                            self.draw_tooltip_pill_for_pane(
                                pane,
                                rt,
                                &br,
                                hitem,
                                shelf_r,
                                cfg.mac_dock_edge,
                                cfg.tb_theme,
                                cw as f32,
                                ch as f32,
                                u,
                            );
                        }
                    }
                }
            }
        }

        rt.EndDraw(None, None).is_ok()
    }

    #[allow(clippy::too_many_arguments)]
    unsafe fn draw_tooltip_pill_for_pane(
        &self,
        pane: &DockMonitorPane,
        rt: &ID2D1DCRenderTarget,
        br: &ID2D1SolidColorBrush,
        item: &DockLayoutSlot,
        shelf_r: D2D_RECT_F,
        edge: MacDockEdge,
        theme: TaskbarTheme,
        cw: f32,
        ch: f32,
        u: f32,
    ) {
        let Some(ref tf) = self.tf_tip else {
            return;
        };
        let label_w = (item.label.chars().count().clamp(4, 28) as f32 * 7.2 * u + 22.0 * u).min(210.0 * u);
        let tip_h = 25.0 * u;
        let top_margin = 4.0 * u;
        let wing_top_l = pane.hud_wing_local.as_ref().map(|(r, _, _)| r.top).unwrap_or(shelf_r.top);
        let wing_bot_l = pane.hud_wing_local.as_ref().map(|(r, _, _)| r.bottom).unwrap_or(shelf_r.bottom);
        let tip_rect = match edge {
            MacDockEdge::Bottom => {
                let x = (item.center_x - label_w * 0.5).clamp(4.0 * u, (cw - label_w - 4.0 * u).max(4.0 * u));
                let y = (item.center_y - item.icon_size * 0.5 - tip_h - 10.0 * u)
                    .min(shelf_r.top.min(wing_top_l) - tip_h - 8.0 * u)
                    .max(top_margin);
                D2D_RECT_F {
                    left: x,
                    top: y,
                    right: x + label_w,
                    bottom: y + tip_h,
                }
            }
            MacDockEdge::Top => {
                let x = (item.center_x - label_w * 0.5).clamp(4.0 * u, (cw - label_w - 4.0 * u).max(4.0 * u));
                let y = (item.center_y + item.icon_size * 0.5 + 10.0 * u)
                    .max(shelf_r.bottom.max(wing_bot_l) + 8.0 * u)
                    .min((ch - tip_h - 4.0 * u).max(4.0 * u));
                D2D_RECT_F {
                    left: x,
                    top: y,
                    right: x + label_w,
                    bottom: y + tip_h,
                }
            }
            MacDockEdge::Left => {
                let x = (item.center_x + item.icon_size * 0.5 + 10.0 * u)
                    .max(shelf_r.right + 8.0 * u)
                    .min((cw - label_w - 4.0 * u).max(4.0 * u));
                let y = (item.center_y - tip_h * 0.5).clamp(4.0 * u, (ch - tip_h - 4.0 * u).max(4.0 * u));
                D2D_RECT_F {
                    left: x,
                    top: y,
                    right: x + label_w,
                    bottom: y + tip_h,
                }
            }
            MacDockEdge::Right => {
                let x = (item.center_x - item.icon_size * 0.5 - label_w - 10.0 * u)
                    .min(shelf_r.left - label_w - 8.0 * u)
                    .max(4.0 * u);
                let y = (item.center_y - tip_h * 0.5).clamp(4.0 * u, (ch - tip_h - 4.0 * u).max(4.0 * u));
                D2D_RECT_F {
                    left: x,
                    top: y,
                    right: x + label_w,
                    bottom: y + tip_h,
                }
            }
        };
        draw_dock_tooltip(rt, br, tf, theme, &item.label, tip_rect, u);
    }
}

fn shell_disk_file_empty(s: &str) -> bool {
    s.trim().is_empty()
}

fn resolve_slot_brand_rgb(slot: &DockSlotKind, cfg: &AppConfig) -> Option<(u8, u8, u8)> {
    if let Some(rgb) = cfg.w_apps.custom_glow_rgb {
        return Some(unpack_rgb(rgb));
    }
    if !cfg.w_apps.per_app_brand_glow {
        return None;
    }
    let key = match slot {
        DockSlotKind::PinnedApp { name, path, .. } => {
            format!("{} {}", name.to_ascii_lowercase(), path.to_ascii_lowercase())
        }
        DockSlotKind::RunningApp { name, exe_path, .. } => {
            format!("{} {}", name.to_ascii_lowercase(), exe_path.to_ascii_lowercase())
        }
        _ => return None,
    };
    if key.contains("explorer") {
        Some((255, 198, 56))
    } else if key.contains("edge") {
        Some((0, 218, 245))
    } else if key.contains("whatsapp") {
        Some((42, 224, 112))
    } else if key.contains("outlook") || key.contains("olk") {
        Some((52, 156, 255))
    } else if key.contains("paint") {
        Some((72, 186, 255))
    } else if key.contains("winstore") || key.contains("windowsstore") || key.contains("store") {
        Some((255, 168, 58))
    } else if key.contains("terminal") || key.contains("wt.exe") || key.contains("powershell") || key.contains("cmd") {
        Some((178, 134, 255))
    } else if key.contains("taskmgr") || key.contains("task manager") {
        Some((64, 212, 255))
    } else if key.contains("settings") || key.contains("immersivecontrolpanel") {
        Some((124, 196, 255))
    } else if key.contains("spotify") {
        Some((30, 215, 96))
    } else if key.contains("discord") {
        Some((114, 137, 255))
    } else if key.contains("chrome") {
        Some((255, 118, 88))
    } else {
        let mut h = 2166136261u32;
        for b in key.bytes() {
            h ^= b as u32;
            h = h.wrapping_mul(16777619);
        }
        let palette = [
            (74, 212, 255),
            (255, 114, 182),
            (94, 242, 178),
            (255, 184, 72),
            (168, 132, 255),
            (86, 238, 232),
        ];
        Some(palette[(h as usize) % palette.len()])
    }
}

fn clock_bucket(cfg: &AppConfig) -> u32 {
    let st: SYSTEMTIME = unsafe { GetLocalTime() };
    if cfg.w_clock.show_seconds {
        (st.wDay as u32) * 86400
            + (st.wHour as u32) * 3600
            + (st.wMinute as u32) * 60
            + (st.wSecond as u32)
    } else {
        (st.wDay as u32) * 1440 + (st.wHour as u32) * 60 + (st.wMinute as u32)
    }
}

fn format_dock_clock(cfg: &AppConfig) -> (String, String, u32, u32, u32) {
    let st: SYSTEMTIME = unsafe { GetLocalTime() };
    let total_mins = (st.wHour as i32) * 60
        + (st.wMinute as i32)
        + (cfg.w_clock.tz_offset_half_hours as i32) * 30;
    let wrapped_mins = total_mins.rem_euclid(24 * 60);
    let h24 = (wrapped_mins / 60) as u32;
    let minute = (wrapped_mins % 60) as u32;
    let second = st.wSecond as u32;

    let use_24h = cfg.w_clock.use_24h.unwrap_or(cfg.widgets_24h);
    let time_str = if use_24h {
        if cfg.w_clock.show_seconds {
            format!("{:02}:{:02}:{:02}", h24, minute, second)
        } else {
            format!("{:02}:{:02}", h24, minute)
        }
    } else {
        let h12 = match h24 % 12 {
            0 => 12,
            v => v,
        };
        let ampm = if h24 >= 12 { "PM" } else { "AM" };
        if cfg.w_clock.show_seconds {
            format!("{}:{:02}:{:02} {}", h12, minute, second, ampm)
        } else {
            format!("{}:{:02} {}", h12, minute, ampm)
        }
    };

    let dow = match st.wDayOfWeek {
        0 => "Sun",
        1 => "Mon",
        2 => "Tue",
        3 => "Wed",
        4 => "Thu",
        5 => "Fri",
        _ => "Sat",
    };
    let date_str = if !cfg.w_clock.show_date {
        String::new()
    } else if cfg.w_clock.show_weekday {
        format!("{} {:02}/{:02}", dow, st.wMonth, st.wDay)
    } else {
        format!("{:02}/{:02}", st.wMonth, st.wDay)
    };
    (time_str, date_str, h24, minute, second)
}

fn slot_id_key(s: &DockSlotKind) -> String {
    match s {
        DockSlotKind::StartLauncher => "sys:start".to_string(),
        DockSlotKind::SearchButton => "sys:search".to_string(),
        DockSlotKind::TaskViewButton => "sys:taskview".to_string(),
        DockSlotKind::HiddenTrayChevron => "sys:hidden_tray".to_string(),
        DockSlotKind::TrayStatusCluster => "sys:tray".to_string(),
        DockSlotKind::ActiveAppChip { .. } => "sys:active".to_string(),
        DockSlotKind::TrashBin { .. } => "sys:trash".to_string(),
        DockSlotKind::StudioLauncher => "sys:studio".to_string(),
        DockSlotKind::ClockModule { .. } => "sys:clock".to_string(),
        DockSlotKind::ShowDesktopSliver => "sys:show_desktop".to_string(),
        DockSlotKind::Divider => "sys:div1".to_string(),
        DockSlotKind::EndDivider => "sys:div2".to_string(),
        DockSlotKind::IslandGap => "sys:island_gap".to_string(),
        DockSlotKind::FlexSpacer => "sys:flex_spacer".to_string(),
        DockSlotKind::PinnedApp { path, .. } => format!("pin:{}", path.to_ascii_lowercase()),
        DockSlotKind::RunningApp { key, .. } => format!("run:{}", key),
    }
}

fn slot_meta(s: &DockSlotKind, fg_hwnd: HWND) -> (String, usize, bool) {
    match s {
        DockSlotKind::StartLauncher => ("Start / Apps".to_string(), 0, false),
        DockSlotKind::SearchButton => ("Search Windows".to_string(), 0, false),
        DockSlotKind::TaskViewButton => ("Task View / Desktops".to_string(), 0, false),
        DockSlotKind::HiddenTrayChevron => ("Show Hidden Icons".to_string(), 0, false),
        DockSlotKind::TrayStatusCluster => ("Quick Settings & Volume".to_string(), 0, false),
        DockSlotKind::ActiveAppChip { title } => (title.clone(), 0, true),
        DockSlotKind::TrashBin { items } => (
            if *items > 0 {
                format!("Trash ({} items)", items)
            } else {
                "Trash (Empty)".to_string()
            },
            0,
            false,
        ),
        DockSlotKind::StudioLauncher => ("Autumn Studio".to_string(), 0, false),
        DockSlotKind::ClockModule { time_str, date_str, .. } => {
            (format!("{} • {}", time_str, date_str), 0, false)
        }
        DockSlotKind::ShowDesktopSliver => ("Show Desktop".to_string(), 0, false),
        DockSlotKind::PinnedApp { name, windows, .. } => {
            let active = windows.iter().any(|w| w.hwnd == fg_hwnd);
            (name.clone(), windows.len(), active)
        }
        DockSlotKind::RunningApp { name, windows, .. } => {
            let active = windows.iter().any(|w| w.hwnd == fg_hwnd);
            (name.clone(), windows.len(), active)
        }
        DockSlotKind::Divider
        | DockSlotKind::EndDivider
        | DockSlotKind::IslandGap
        | DockSlotKind::FlexSpacer => (String::new(), 0, false),
    }
}

fn canonical_exe_name(path: &str) -> String {
    Path::new(path.trim())
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
}

fn normalize_exe_alias(exe: &str) -> String {
    match exe {
        "wt.exe" => "windowsterminal.exe".to_string(),
        other => other.to_string(),
    }
}

fn running_launch_target(exe_path: &str, windows: &[WinEntry]) -> String {
    if let Some(w) = windows.first().filter(|w| !w.aumid.is_empty()) {
        let low = exe_path.to_ascii_lowercase();
        if exe_path.is_empty()
            || low.starts_with("shell:")
            || low.contains("\\windowsapps\\")
            || canonical_exe_name(exe_path) == "applicationframehost.exe"
        {
            return format!("shell:AppsFolder\\{}", w.aumid);
        }
    }
    if !exe_path.is_empty() {
        exe_path.to_string()
    } else {
        windows.first().map(|w| w.exe_path.clone()).unwrap_or_default()
    }
}

fn aumid_from_pin_path(path: &str) -> String {
    let t = path.trim();
    if t.len() > 17 && t[..17].eq_ignore_ascii_case("shell:appsfolder\\") {
        t[17..].trim().to_string()
    } else if t.eq_ignore_ascii_case("ms-settings:") {
        "windows.immersivecontrolpanel_cw5n1h2txyewy!microsoft.windows.immersivecontrolpanel".to_string()
    } else {
        String::new()
    }
}

fn aumid_family(aumid: &str) -> String {
    aumid
        .split('!')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase()
}

fn window_matches_pinned(pin_name: &str, pin_path: &str, target_exe: &str, w: &WinEntry) -> bool {
    let pin_aumid = aumid_from_pin_path(pin_path);
    let wkey = normalize_exe_alias(&canonical_exe_name(&w.exe_path));
    if !pin_aumid.is_empty() {
        if !w.aumid.is_empty() {
            if pin_aumid.eq_ignore_ascii_case(&w.aumid) {
                return true;
            }
            let pf = aumid_family(&pin_aumid);
            if !pf.is_empty() && pf == aumid_family(&w.aumid) {
                return true;
            }
        }
        let pf = aumid_family(&pin_aumid);
        let mapped_match = match wkey.as_str() {
            "mspaint.exe" => pf.starts_with("microsoft.paint_"),
            "olk.exe" | "outlook.exe" => pf.starts_with("microsoft.outlookforwindows_"),
            "whatsapp.exe" => pf.starts_with("5319275a.whatsappdesktop_"),
            "windowsterminal.exe" => pf.starts_with("microsoft.windowsterminal_"),
            "systemsettings.exe" => pf.starts_with("windows.immersivecontrolpanel_"),
            "winstore.app.exe" => pf.starts_with("microsoft.windowsstore_"),
            "calculatorapp.exe" => pf.starts_with("microsoft.windowscalculator_"),
            _ => false,
        };
        if mapped_match {
            return true;
        }
    }
    let match_key = canonical_match_key(pin_name, pin_path, target_exe);
    !match_key.is_empty() && !wkey.is_empty() && wkey == match_key
}

fn canonical_match_key(name: &str, path: &str, resolved_target: &str) -> String {
    let tgt = normalize_exe_alias(&canonical_exe_name(resolved_target));
    if !tgt.is_empty() && tgt.ends_with(".exe") {
        return tgt;
    }
    let direct = normalize_exe_alias(&canonical_exe_name(path));
    if !direct.is_empty() && direct.ends_with(".exe") {
        return direct;
    }
    let low = name.trim().to_ascii_lowercase();
    let plow = path.trim().to_ascii_lowercase();
    if low.contains("explorer") || plow.contains("explorer") {
        "explorer.exe".to_string()
    } else if low.contains("edge") || plow.contains("edge") {
        "msedge.exe".to_string()
    } else if low.contains("chrome") {
        "chrome.exe".to_string()
    } else if low.contains("firefox") {
        "firefox.exe".to_string()
    } else if low.contains("terminal") || plow.ends_with("wt.exe") {
        "windowsterminal.exe".to_string()
    } else if low.contains("settings") || plow == "ms-settings:" {
        "systemsettings.exe".to_string()
    } else if low.contains("code") {
        "code.exe".to_string()
    } else {
        low
    }
}

fn shell_display_name(parsing_path: &str) -> Option<String> {
    if parsing_path.is_empty() {
        return None;
    }
    unsafe {
        let wide = to_wide(parsing_path);
        let item: IShellItem = SHCreateItemFromParsingName(PCWSTR(wide.as_ptr()), None).ok()?;
        let pw = item.GetDisplayName(SIGDN_NORMALDISPLAY).ok()?;
        let s = pw.to_string().ok();
        CoTaskMemFree(Some(pw.0 as *const c_void));
        s.map(|v| v.trim().to_string()).filter(|v| !v.is_empty())
    }
}

fn pretty_app_name(exe_path: &str, title: &str, aumid: &str) -> String {
    let exe = canonical_exe_name(exe_path);
    match exe.as_str() {
        "explorer.exe" => return "File Explorer".to_string(),
        "msedge.exe" => return "Microsoft Edge".to_string(),
        "chrome.exe" => return "Google Chrome".to_string(),
        "windowsterminal.exe" | "wt.exe" => return "Terminal".to_string(),
        "systemsettings.exe" => return "Settings".to_string(),
        "winstore.app.exe" => return "Microsoft Store".to_string(),
        "calculatorapp.exe" => return "Calculator".to_string(),
        "mspaint.exe" => return "Paint".to_string(),
        "code.exe" => return "VS Code".to_string(),
        "whatsapp.exe" => return "WhatsApp".to_string(),
        "olk.exe" | "outlook.exe" => return "Outlook".to_string(),
        _ => {}
    }
    let low_exe = exe_path.to_ascii_lowercase();
    if !aumid.is_empty()
        && (exe_path.is_empty()
            || low_exe.starts_with("shell:")
            || low_exe.contains("\\windowsapps\\")
            || exe == "applicationframehost.exe")
    {
        if let Some(disp) = shell_display_name(&format!("shell:AppsFolder\\{}", aumid)) {
            return disp;
        }
    }
    if !exe_path.is_empty() && !low_exe.starts_with("shell:") && exe != "applicationframehost.exe" {
        if let Some(stem) = Path::new(exe_path).file_stem().and_then(|s| s.to_str()) {
            if !stem.is_empty() {
                let mut chars = stem.chars();
                if let Some(c) = chars.next() {
                    return format!("{}{}", c.to_uppercase(), chars.as_str());
                }
            }
        }
    }
    let t = title.trim();
    if t.chars().count() > 26 {
        t.chars().take(24).collect::<String>() + ".."
    } else if !t.is_empty() {
        t.to_string()
    } else {
        "App".to_string()
    }
}

fn is_explorer_folder_window(hwnd: HWND) -> bool {
    let mut cls = [0u16; 64];
    let len = unsafe { GetClassNameW(hwnd, &mut cls) };
    if len <= 0 {
        return false;
    }
    let c = String::from_utf16_lossy(&cls[..len as usize]);
    c == "CabinetWClass"
}

fn query_process_exe(pid: u32) -> String {
    if pid == 0 {
        return String::new();
    }
    unsafe {
        let Ok(hproc) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
            return String::new();
        };
        let mut buf = [0u16; 520];
        let mut sz = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(
            hproc,
            PROCESS_NAME_WIN32,
            windows::core::PWSTR(buf.as_mut_ptr()),
            &mut sz,
        )
        .is_ok();
        let _ = CloseHandle(hproc);
        if ok && sz > 0 {
            String::from_utf16_lossy(&buf[..sz as usize])
        } else {
            String::new()
        }
    }
}

#[link(name = "kernel32")]
extern "system" {
    fn GetApplicationUserModelId(
        hprocess: HANDLE,
        application_user_model_id_length: *mut u32,
        application_user_model_id: PWSTR,
    ) -> u32;
}

fn query_process_aumid(pid: u32) -> String {
    if pid == 0 {
        return String::new();
    }
    unsafe {
        let Ok(hproc) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
            return String::new();
        };
        let mut buf = [0u16; 260];
        let mut len = buf.len() as u32;
        let rc = GetApplicationUserModelId(hproc, &mut len, PWSTR(buf.as_mut_ptr()));
        let _ = CloseHandle(hproc);
        if rc == 0 && len > 1 {
            let s_len = (len as usize).saturating_sub(1).min(buf.len());
            String::from_utf16_lossy(&buf[..s_len]).trim().to_string()
        } else {
            String::new()
        }
    }
}

fn query_window_aumid(hwnd: HWND) -> String {
    const PKEY_AUMID: PROPERTYKEY = PROPERTYKEY {
        fmtid: windows::core::GUID::from_u128(0x9F4C2855_9F79_4B39_A8D0_E1D42DE1D5F3),
        pid: 5,
    };
    unsafe {
        let Ok(ps) = SHGetPropertyStoreForWindow::<IPropertyStore>(hwnd) else {
            return String::new();
        };
        let Ok(mut pv) = ps.GetValue(&PKEY_AUMID) else {
            return String::new();
        };
        let aumid = if let Ok(pw) = PropVariantToStringAlloc(&pv) {
            let s = pw.to_string().unwrap_or_default().trim().to_string();
            CoTaskMemFree(Some(pw.0 as *const c_void));
            s
        } else {
            String::new()
        };
        let _ = PropVariantClear(&mut pv);
        aumid
    }
}

fn resolve_uwp_child(frame_hwnd: HWND, frame_pid: u32) -> Option<(HWND, u32, String)> {
    struct Ctx {
        frame_pid: u32,
        found: Option<(HWND, u32, String)>,
    }
    unsafe extern "system" fn child_cb(hwnd: HWND, lp: LPARAM) -> BOOL {
        let ctx = &mut *(lp.0 as *mut Ctx);
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid != 0 && pid != ctx.frame_pid {
            let exe = query_process_exe(pid);
            if !exe.is_empty() {
                ctx.found = Some((hwnd, pid, exe));
                return BOOL(0);
            }
        }
        BOOL(1)
    }
    let mut ctx = Ctx {
        frame_pid,
        found: None,
    };
    unsafe {
        let _ = EnumChildWindows(
            Some(frame_hwnd),
            Some(child_cb),
            LPARAM((&mut ctx as *mut Ctx) as isize),
        );
    }
    ctx.found
}

fn collect_running_windows(
    proc_cache: &mut HashMap<u32, (String, String)>,
    hwnd_aumid_cache: &mut HashMap<isize, String>,
) -> Vec<WinEntry> {
    if proc_cache.len() > 128 {
        proc_cache.clear();
    }
    if hwnd_aumid_cache.len() > 256 {
        hwnd_aumid_cache.clear();
    }
    let mut out = Vec::new();
    unsafe {
        struct EnumCtx<'a> {
            own_pid: u32,
            proc_cache: &'a mut HashMap<u32, (String, String)>,
            hwnd_aumid_cache: &'a mut HashMap<isize, String>,
            out: &'a mut Vec<WinEntry>,
        }
        unsafe extern "system" fn cb(hwnd: HWND, lp: LPARAM) -> BOOL {
            let ctx = &mut *(lp.0 as *mut EnumCtx<'_>);
            if !IsWindowVisible(hwnd).as_bool() {
                return BOOL(1);
            }
            let ex = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
            const WS_EX_TOOLWINDOW_FLAG: u32 = 0x0000_0080;
            const WS_EX_APPWINDOW_FLAG: u32 = 0x0004_0000;
            const WS_EX_NOACTIVATE_FLAG: u32 = 0x0800_0000;
            if (ex & WS_EX_TOOLWINDOW_FLAG) != 0 {
                return BOOL(1);
            }
            if (ex & WS_EX_NOACTIVATE_FLAG) != 0 && (ex & WS_EX_APPWINDOW_FLAG) == 0 {
                return BOOL(1);
            }
            let owner = GetWindow(hwnd, GW_OWNER).unwrap_or_default();
            if !owner.0.is_null() && (ex & WS_EX_APPWINDOW_FLAG) == 0 {
                return BOOL(1);
            }
            let mut cloaked = 0u32;
            if DwmGetWindowAttribute(
                hwnd,
                DWMWA_CLOAKED,
                &mut cloaked as *mut u32 as *mut c_void,
                size_of::<u32>() as u32,
            )
            .is_ok()
                && cloaked != 0
            {
                return BOOL(1);
            }
            if !IsIconic(hwnd).as_bool() {
                let mut rc = RECT::default();
                if GetWindowRect(hwnd, &mut rc).is_err()
                    || (rc.right - rc.left) <= 2
                    || (rc.bottom - rc.top) <= 2
                {
                    return BOOL(1);
                }
            }
            let mut pid = 0u32;
            GetWindowThreadProcessId(hwnd, Some(&mut pid));
            if pid == 0 || pid == ctx.own_pid {
                return BOOL(1);
            }
            let mut cls_buf = [0u16; 96];
            let cls_len = GetClassNameW(hwnd, &mut cls_buf);
            let cls = if cls_len > 0 {
                String::from_utf16_lossy(&cls_buf[..cls_len as usize])
            } else {
                String::new()
            };
            if matches!(
                cls.as_str(),
                "Progman"
                    | "WorkerW"
                    | "Shell_TrayWnd"
                    | "Shell_SecondaryTrayWnd"
                    | "Windows.UI.Core.CoreWindow"
                    | "ApplicationManager_DesktopShellWindow"
                    | "ForegroundStaging"
            ) {
                return BOOL(1);
            }
            let tlen = GetWindowTextLengthW(hwnd);
            if tlen <= 0 {
                return BOOL(1);
            }
            let mut tbuf = vec![0u16; (tlen as usize).saturating_add(2)];
            let got = GetWindowTextW(hwnd, &mut tbuf);
            let title = if got > 0 {
                String::from_utf16_lossy(&tbuf[..got as usize]).trim().to_string()
            } else {
                String::new()
            };
            if title.is_empty() {
                return BOOL(1);
            }

            let (mut exe_path, mut aumid) = if let Some((c_exe, c_aum)) = ctx.proc_cache.get(&pid) {
                (c_exe.clone(), c_aum.clone())
            } else {
                let p_exe = query_process_exe(pid);
                let p_aum = query_process_aumid(pid);
                if !p_exe.is_empty() || !p_aum.is_empty() {
                    ctx.proc_cache.insert(pid, (p_exe.clone(), p_aum.clone()));
                }
                (p_exe, p_aum)
            };
            let hw_raw = hwnd.0 as isize;
            let mut icon_hwnd = hwnd;
            if canonical_exe_name(&exe_path) == "applicationframehost.exe" {
                let win_aum = if let Some(cached) = ctx.hwnd_aumid_cache.get(&hw_raw) {
                    cached.clone()
                } else {
                    let q = query_window_aumid(hwnd);
                    ctx.hwnd_aumid_cache.insert(hw_raw, q.clone());
                    q
                };
                if !win_aum.is_empty() {
                    aumid = win_aum;
                }
                if let Some((child_hw, child_pid, child_exe)) = resolve_uwp_child(hwnd, pid) {
                    exe_path = child_exe;
                    icon_hwnd = child_hw;
                    if aumid.is_empty() {
                        aumid = query_process_aumid(child_pid);
                    }
                } else if !aumid.is_empty() {
                    exe_path = format!("shell:AppsFolder\\{}", aumid);
                } else if title.eq_ignore_ascii_case("settings") {
                    exe_path = r"C:\Windows\ImmersiveControlPanel\SystemSettings.exe".to_string();
                    aumid = "windows.immersivecontrolpanel_cw5n1h2txyewy!microsoft.windows.immersivecontrolpanel".to_string();
                } else {
                    exe_path = String::new();
                }
            } else if aumid.is_empty() {
                aumid = if let Some(cached) = ctx.hwnd_aumid_cache.get(&hw_raw) {
                    cached.clone()
                } else {
                    let q = query_window_aumid(hwnd);
                    ctx.hwnd_aumid_cache.insert(hw_raw, q.clone());
                    q
                };
            }
            if canonical_exe_name(&exe_path) == "textinputhost.exe" {
                return BOOL(1);
            }

            ctx.out.push(WinEntry {
                hwnd,
                title,
                exe_path,
                aumid,
                icon_hwnd,
                is_iconic: IsIconic(hwnd).as_bool(),
            });
            BOOL(1)
        }
        let mut ctx = EnumCtx {
            own_pid: std::process::id(),
            proc_cache,
            hwnd_aumid_cache,
            out: &mut out,
        };
        let _ = EnumWindows(Some(cb), LPARAM((&mut ctx as *mut EnumCtx<'_>) as isize));
    }
    out
}

unsafe fn load_window_bitmap(
    wic: &IWICImagingFactory,
    rt: &ID2D1DCRenderTarget,
    win: &WinEntry,
) -> Option<ID2D1Bitmap> {
    // 0. Packaged / Microsoft Store / UWP apps render official high-res logos via shell:AppsFolder\<AUMID>
    if !win.aumid.is_empty() {
        let uri = format!("shell:AppsFolder\\{}", win.aumid);
        if let Some(bmp) = load_app_bitmap_for_path(wic, rt, &uri, false) {
            return Some(bmp);
        }
    }

    // 1. Try crisp high-res shell icon via executable path first (often 64..128px PNG/alpha!)
    let low_exe = win.exe_path.to_ascii_lowercase();
    if low_exe.starts_with("shell:") {
        if let Some(bmp) = load_app_bitmap_for_path(wic, rt, &win.exe_path, false) {
            return Some(bmp);
        }
    } else if !win.exe_path.is_empty() && !low_exe.contains("\\windowsapps\\") {
        let wide = to_wide(&win.exe_path);
        if let Ok(factory) = SHCreateItemFromParsingName::<_, _, IShellItemImageFactory>(
            PCWSTR(wide.as_ptr()),
            None,
        ) {
            for flags in [SIIGBF_ICONONLY | SIIGBF_BIGGERSIZEOK, SIIGBF_BIGGERSIZEOK] {
                if let Ok(hbm) = factory.GetImage(SIZE { cx: 128, cy: 128 }, flags) {
                    let bmp = wic
                        .CreateBitmapFromHBITMAP(hbm, HPALETTE::default(), WICBitmapUseAlpha)
                        .ok()
                        .and_then(|wb| {
                            let src: IWICBitmapSource = wb.into();
                            wic_src_to_d2d(wic, rt, &src, false)
                        });
                    let _ = DeleteObject(HGDIOBJ(hbm.0));
                    if bmp.is_some() {
                        return bmp;
                    }
                }
            }
        }
    }

    // 2. Try runtime HICON (handles Electron / custom-branded windows cleanly)
    let targets: &[(HWND, bool)] = &[
        (win.hwnd, true),
        (win.icon_hwnd, win.icon_hwnd != win.hwnd && !win.icon_hwnd.0.is_null()),
    ];
    for &(target_hw, run) in targets {
        if !run || target_hw.0.is_null() {
            continue;
        }
        for idx in [GCLP_HICON, GCLP_HICONSM] {
            let hico_raw = GetClassLongPtrW(target_hw, idx);
            if hico_raw != 0 {
                let hicon = HICON(hico_raw as *mut c_void);
                if let Some(bmp) = hicon_to_d2d(wic, rt, hicon) {
                    return Some(bmp);
                }
            }
        }
        for flag in [ICON_BIG, ICON_SMALL2] {
            let mut hico_raw = 0usize;
            let ok = SendMessageTimeoutW(
                target_hw,
                WM_GETICON,
                WPARAM(flag as usize),
                LPARAM(0),
                SMTO_ABORTIFHUNG,
                15,
                Some(&mut hico_raw),
            );
            if ok.0 != 0 && hico_raw != 0 {
                let hicon = HICON(hico_raw as *mut c_void);
                if let Some(bmp) = hicon_to_d2d(wic, rt, hicon) {
                    return Some(bmp);
                }
            }
        }
    }

    // 3. Final fallback via SHGetFileInfoW / shell item on exe path
    if !win.exe_path.is_empty() && !low_exe.starts_with("shell:") {
        let wide = to_wide(&win.exe_path);
        let mut shfi = SHFILEINFOW::default();
        if SHGetFileInfoW(
            PCWSTR(wide.as_ptr()),
            FILE_ATTRIBUTE_NORMAL,
            Some(&mut shfi),
            size_of::<SHFILEINFOW>() as u32,
            SHGFI_ICON | SHGFI_LARGEICON | SHGFI_USEFILEATTRIBUTES,
        ) != 0
            && !shfi.hIcon.is_invalid()
        {
            let bmp = hicon_to_d2d(wic, rt, shfi.hIcon);
            let _ = DestroyIcon(shfi.hIcon);
            if bmp.is_some() {
                return bmp;
            }
        }
        if let Ok(factory) = SHCreateItemFromParsingName::<_, _, IShellItemImageFactory>(
            PCWSTR(wide.as_ptr()),
            None,
        ) {
            if let Ok(hbm) = factory.GetImage(SIZE { cx: 128, cy: 128 }, SIIGBF(0)) {
                let bmp = wic
                    .CreateBitmapFromHBITMAP(hbm, HPALETTE::default(), WICBitmapUseAlpha)
                    .ok()
                    .and_then(|wb| {
                        let src: IWICBitmapSource = wb.into();
                        wic_src_to_d2d(wic, rt, &src, false)
                    });
                let _ = DeleteObject(HGDIOBJ(hbm.0));
                if bmp.is_some() {
                    return bmp;
                }
            }
        }
    }
    None
}

unsafe fn wic_src_to_pbgra(
    wic: &IWICImagingFactory,
    src: &IWICBitmapSource,
) -> Option<(u32, u32, Vec<u8>)> {
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
    let mut w = 0u32;
    let mut h = 0u32;
    if converter.GetSize(&mut w, &mut h).is_err() || w == 0 || h == 0 || w > 512 || h > 512 {
        return None;
    }
    let stride = w * 4;
    let mut buf = vec![0u8; (stride * h) as usize];
    converter.CopyPixels(std::ptr::null(), stride, &mut buf).ok()?;
    Some((w, h, buf))
}

unsafe fn shell_path_to_pbgra(
    wic: &IWICImagingFactory,
    parsing_path: &str,
) -> Option<(u32, u32, Vec<u8>)> {
    if parsing_path.is_empty() {
        return None;
    }
    let wide = to_wide(parsing_path);
    if let Ok(factory) = SHCreateItemFromParsingName::<_, _, IShellItemImageFactory>(
        PCWSTR(wide.as_ptr()),
        None,
    ) {
        for flags in [
            SIIGBF_ICONONLY | SIIGBF_BIGGERSIZEOK,
            SIIGBF_BIGGERSIZEOK,
            SIIGBF(0),
        ] {
            if let Ok(hbm) = factory.GetImage(SIZE { cx: 128, cy: 128 }, flags) {
                let res = wic
                    .CreateBitmapFromHBITMAP(hbm, HPALETTE::default(), WICBitmapUseAlpha)
                    .ok()
                    .and_then(|wb| {
                        let src: IWICBitmapSource = wb.into();
                        wic_src_to_pbgra(wic, &src)
                    });
                let _ = DeleteObject(HGDIOBJ(hbm.0));
                if res.is_some() {
                    return res;
                }
            }
        }
    }
    if !parsing_path.to_ascii_lowercase().starts_with("shell:") {
        let mut shfi = SHFILEINFOW::default();
        if SHGetFileInfoW(
            PCWSTR(wide.as_ptr()),
            FILE_ATTRIBUTE_NORMAL,
            Some(&mut shfi),
            size_of::<SHFILEINFOW>() as u32,
            SHGFI_ICON | SHGFI_LARGEICON | SHGFI_USEFILEATTRIBUTES,
        ) != 0
            && !shfi.hIcon.is_invalid()
        {
            let res = wic
                .CreateBitmapFromHICON(shfi.hIcon)
                .ok()
                .and_then(|wb| {
                    let src: IWICBitmapSource = wb.into();
                    wic_src_to_pbgra(wic, &src)
                });
            let _ = DestroyIcon(shfi.hIcon);
            if res.is_some() {
                return res;
            }
        }
    }
    None
}

unsafe fn decode_dock_icon_pbgra(
    wic: &IWICImagingFactory,
    req: &DockIconReq,
) -> Option<(u32, u32, Vec<u8>)> {
    // 1. Official Store/UWP AppsFolder logo via AUMID
    if !req.aumid.is_empty() {
        let uri = format!("shell:AppsFolder\\{}", req.aumid);
        if let Some(px) = shell_path_to_pbgra(wic, &uri) {
            return Some(px);
        }
    }
    // 2. High-res exe shell icon (non-WindowsApps)
    let low_exe = req.exe_path.to_ascii_lowercase();
    if low_exe.starts_with("shell:") {
        if let Some(px) = shell_path_to_pbgra(wic, &req.exe_path) {
            return Some(px);
        }
    } else if !req.exe_path.is_empty() && !low_exe.contains("\\windowsapps\\") {
        if let Some(px) = shell_path_to_pbgra(wic, &req.exe_path) {
            return Some(px);
        }
    }
    // 3. Runtime HICON from window class or short 15ms WM_GETICON off-thread
    let hw1 = HWND(req.hwnd_raw as *mut c_void);
    let hw2 = HWND(req.icon_hwnd_raw as *mut c_void);
    for &(hw, active) in &[(hw1, !hw1.0.is_null()), (hw2, !hw2.0.is_null() && hw2 != hw1)] {
        if !active || !IsWindow(Some(hw)).as_bool() {
            continue;
        }
        for idx in [GCLP_HICON, GCLP_HICONSM] {
            let hico_raw = GetClassLongPtrW(hw, idx);
            if hico_raw != 0 {
                if let Ok(wb) = wic.CreateBitmapFromHICON(HICON(hico_raw as *mut c_void)) {
                    let src: IWICBitmapSource = wb.into();
                    if let Some(px) = wic_src_to_pbgra(wic, &src) {
                        return Some(px);
                    }
                }
            }
        }
        for flag in [ICON_BIG, ICON_SMALL2] {
            let mut hico_raw = 0usize;
            let ok = SendMessageTimeoutW(
                hw,
                WM_GETICON,
                WPARAM(flag as usize),
                LPARAM(0),
                SMTO_ABORTIFHUNG,
                15,
                Some(&mut hico_raw),
            );
            if ok.0 != 0 && hico_raw != 0 {
                if let Ok(wb) = wic.CreateBitmapFromHICON(HICON(hico_raw as *mut c_void)) {
                    let src: IWICBitmapSource = wb.into();
                    if let Some(px) = wic_src_to_pbgra(wic, &src) {
                        return Some(px);
                    }
                }
            }
        }
    }
    // 4. Pinned shortcut / target / canonical fallbacks
    if req.is_pinned {
        if req.app_path.to_ascii_lowercase().starts_with("shell:") {
            if let Some(px) = shell_path_to_pbgra(wic, &req.app_path) {
                return Some(px);
            }
        }
        if !req.target_exe.is_empty() {
            let low_t = req.target_exe.to_ascii_lowercase();
            if !low_t.ends_with("wt.exe")
                && !low_t.contains("\\windowsapps\\")
                && Path::new(&req.target_exe).exists()
            {
                if let Some(px) = shell_path_to_pbgra(wic, &req.target_exe) {
                    return Some(px);
                }
            }
        }
        let mkey = canonical_match_key(&req.app_name, &req.app_path, &req.target_exe);
        let direct_exe = match mkey.as_str() {
            "explorer.exe" => Some(r"C:\Windows\explorer.exe"),
            "msedge.exe" => [
                r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe",
                r"C:\Program Files\Microsoft\Edge\Application\msedge.exe",
            ]
            .into_iter()
            .find(|p| Path::new(p).exists()),
            "windowsterminal.exe" => {
                Some(r"shell:AppsFolder\Microsoft.WindowsTerminal_8wekyb3d8bbwe!App")
            }
            _ => None,
        };
        if let Some(de) = direct_exe {
            if let Some(px) = shell_path_to_pbgra(wic, de) {
                return Some(px);
            }
        }
        if let Some(px) = shell_path_to_pbgra(wic, &req.app_path) {
            return Some(px);
        }
    } else if !req.exe_path.is_empty() {
        if let Some(px) = shell_path_to_pbgra(wic, &req.exe_path) {
            return Some(px);
        }
    }
    None
}

unsafe fn hicon_to_d2d(
    wic: &IWICImagingFactory,
    rt: &ID2D1DCRenderTarget,
    hicon: HICON,
) -> Option<ID2D1Bitmap> {
    let wb = wic.CreateBitmapFromHICON(hicon).ok()?;
    let src: IWICBitmapSource = wb.into();
    wic_src_to_d2d(wic, rt, &src, false)
}

fn dock_cross_metrics(cfg: &AppConfig, u: f32) -> (DockVisualSpec, bool, f32, f32, f32, f32) {
    let spec = resolve_shelf_spec(cfg);
    let is_vertical = matches!(cfg.mac_dock_edge, MacDockEdge::Left | MacDockEdge::Right);
    let base_sz = (cfg.mac_dock_icon_size as f32).clamp(34.0, 56.0) * u;
    let shelf_cross = match spec.layout {
        DockLayoutKind::FullEdgeTaskbar => (base_sz + 14.0 * u).max(46.0 * u),
        DockLayoutKind::HudConsoleDeck => base_sz + 22.0 * u,
        DockLayoutKind::IndependentOrbs => base_sz + 20.0 * u,
        _ => base_sz + 18.0 * u,
    };
    let edge_gap = match spec.layout {
        DockLayoutKind::FullEdgeTaskbar => 0.0,
        DockLayoutKind::IndependentOrbs => 10.0 * u,
        _ => 6.0 * u,
    };
    let want_hud_wing = !is_vertical
        && cfg.w_hud.show_telemetry
        && match cfg.w_hud.mode {
            HudWingMode::AlwaysEnabled => true,
            HudWingMode::Hidden => false,
            HudWingMode::InheritTheme => spec.layout == DockLayoutKind::HudConsoleDeck,
        };
    let hud_wing_h = if want_hud_wing { 19.0 * u } else { 0.0 };
    (spec, is_vertical, base_sz, shelf_cross, edge_gap, hud_wing_h)
}

fn activate_or_toggle_windows(windows: &[WinEntry]) {
    if windows.is_empty() {
        return;
    }
    let fg = unsafe { GetForegroundWindow() };
    if windows.len() == 1 {
        let w = &windows[0];
        let is_iconic_now = unsafe { IsIconic(w.hwnd).as_bool() };
        let fg_owned_by_w = unsafe { GetWindow(fg, GW_OWNER).unwrap_or_default() == w.hwnd };
        let popup_matches = unsafe { GetLastActivePopup(w.hwnd) == fg };
        if !is_iconic_now && (w.hwnd == fg || fg_owned_by_w || popup_matches) {
            unsafe {
                let _ = ShowWindow(w.hwnd, SW_MINIMIZE);
            }
        } else {
            bring_window_to_front(w.hwnd);
        }
        return;
    }
    // Cycle among multiple open windows of the same app
    if let Some(pos) = windows.iter().position(|w| w.hwnd == fg) {
        let next = &windows[(pos + 1) % windows.len()];
        bring_window_to_front(next.hwnd);
    } else {
        let target = windows.iter().find(|w| !w.is_iconic).unwrap_or(&windows[0]);
        bring_window_to_front(target.hwnd);
    }
}

fn bring_window_to_front(hwnd: HWND) {
    unsafe {
        if hwnd.0.is_null() || !IsWindow(Some(hwnd)).as_bool() {
            return;
        }
        // Only restore minimized windows; never pass SW_SHOWNORMAL to a zoomed window!
        if IsIconic(hwnd).as_bool() {
            let _ = ShowWindow(hwnd, SW_RESTORE);
        }
        let popup = GetLastActivePopup(hwnd);
        let target = if !popup.0.is_null()
            && IsWindow(Some(popup)).as_bool()
            && IsWindowVisible(popup).as_bool()
        {
            popup
        } else {
            hwnd
        };
        let _ = AllowSetForegroundWindow(ASFW_ANY);
        let fg = GetForegroundWindow();
        let fg_tid = if !fg.0.is_null() {
            GetWindowThreadProcessId(fg, None)
        } else {
            0
        };
        let tgt_tid = GetWindowThreadProcessId(target, None);
        let cur_tid = GetCurrentThreadId();
        if cur_tid != 0 && fg_tid != 0 && fg_tid != cur_tid {
            let _ = AttachThreadInput(cur_tid, fg_tid, true);
        }
        if cur_tid != 0 && tgt_tid != 0 && tgt_tid != cur_tid && tgt_tid != fg_tid {
            let _ = AttachThreadInput(cur_tid, tgt_tid, true);
        }
        let _ = BringWindowToTop(target);
        let _ = SetForegroundWindow(target);
        if cur_tid != 0 && tgt_tid != 0 && tgt_tid != cur_tid && tgt_tid != fg_tid {
            let _ = AttachThreadInput(cur_tid, tgt_tid, false);
        }
        if cur_tid != 0 && fg_tid != 0 && fg_tid != cur_tid {
            let _ = AttachThreadInput(cur_tid, fg_tid, false);
        }
    }
}

fn trigger_start_menu() {
    let seq = [
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VK_LWIN,
                    wScan: 0,
                    dwFlags: KEYBD_EVENT_FLAGS(0),
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        },
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VK_LWIN,
                    wScan: 0,
                    dwFlags: KEYEVENTF_KEYUP,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        },
    ];
    unsafe {
        let _ = SendInput(&seq, size_of::<INPUT>() as i32);
    }
}

fn is_point_on_screen_edge(pt: POINT, mon: RECT, edge: MacDockEdge, margin: i32) -> bool {
    let m = margin.max(2);
    match edge {
        MacDockEdge::Bottom => pt.y >= mon.bottom - m && pt.x >= mon.left && pt.x <= mon.right,
        MacDockEdge::Top => pt.y <= mon.top + m && pt.x >= mon.left && pt.x <= mon.right,
        MacDockEdge::Left => pt.x <= mon.left + m && pt.y >= mon.top && pt.y <= mon.bottom,
        MacDockEdge::Right => pt.x >= mon.right - m && pt.y >= mon.top && pt.y <= mon.bottom,
    }
}

#[inline]
fn smoothstep01(t: f32) -> f32 {
    let x = t.clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
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

fn truncate_title(s: &str, max_chars: usize) -> String {
    let t = s.trim();
    if t.chars().count() <= max_chars {
        t.to_string()
    } else {
        t.chars().take(max_chars.saturating_sub(2)).collect::<String>() + ".."
    }
}

unsafe fn force_foreground_for_menu(hwnd: HWND) -> HWND {
    let prev_fg = GetForegroundWindow();
    let _ = AllowSetForegroundWindow(ASFW_ANY);
    if !SetForegroundWindow(hwnd).as_bool() && !prev_fg.0.is_null() {
        let fg_tid = GetWindowThreadProcessId(prev_fg, None);
        let cur_tid = GetCurrentThreadId();
        if fg_tid != 0 && cur_tid != 0 && fg_tid != cur_tid {
            let _ = AttachThreadInput(cur_tid, fg_tid, true);
            let _ = SetForegroundWindow(hwnd);
            let _ = AttachThreadInput(cur_tid, fg_tid, false);
        }
    }
    prev_fg
}

unsafe fn restore_prev_fg(dock_hwnd: HWND, prev_fg: HWND) {
    if !prev_fg.0.is_null() && prev_fg != dock_hwnd && IsWindow(Some(prev_fg)).as_bool() {
        let fg_tid = GetWindowThreadProcessId(prev_fg, None);
        let cur_tid = GetCurrentThreadId();
        if fg_tid != 0 && cur_tid != 0 && fg_tid != cur_tid {
            let _ = AttachThreadInput(cur_tid, fg_tid, true);
            let _ = SetForegroundWindow(prev_fg);
            let _ = AttachThreadInput(cur_tid, fg_tid, false);
        } else {
            let _ = SetForegroundWindow(prev_fg);
        }
    }
}

unsafe fn track_dock_popup(
    hwnd: HWND,
    hmenu: HMENU,
    pt: POINT,
    edge: MacDockEdge,
    mon: RECT,
) -> usize {
    let h_flag = match edge {
        MacDockEdge::Right => TPM_RIGHTALIGN,
        MacDockEdge::Left => TPM_LEFTALIGN,
        MacDockEdge::Bottom | MacDockEdge::Top => {
            if pt.x > mon.right - 260 {
                TPM_RIGHTALIGN
            } else {
                TPM_LEFTALIGN
            }
        }
    };
    let v_flag = match edge {
        MacDockEdge::Bottom => {
            if pt.y < mon.top + 340 {
                TPM_TOPALIGN
            } else {
                TPM_BOTTOMALIGN
            }
        }
        MacDockEdge::Top => {
            if pt.y > mon.bottom - 340 {
                TPM_BOTTOMALIGN
            } else {
                TPM_TOPALIGN
            }
        }
        MacDockEdge::Left | MacDockEdge::Right => {
            if pt.y > mon.bottom - 360 {
                TPM_BOTTOMALIGN
            } else {
                TPM_TOPALIGN
            }
        }
    };
    let cmd = TrackPopupMenu(
        hmenu,
        TPM_RETURNCMD | TPM_RIGHTBUTTON | h_flag | v_flag,
        pt.x,
        pt.y,
        Some(0),
        hwnd,
        None,
    );
    let _ = PostMessageW(Some(hwnd), WM_NULL, WPARAM(0), LPARAM(0));
    cmd.0 as usize
}

unsafe fn is_hwnd_cloaked(hw: HWND) -> bool {
    let mut cloaked: u32 = 0;
    let ok = DwmGetWindowAttribute(
        hw,
        DWMWA_CLOAKED,
        &mut cloaked as *mut u32 as *mut c_void,
        size_of::<u32>() as u32,
    )
    .is_ok();
    ok && cloaked != 0
}

unsafe fn is_overflow_actually_open(hw: HWND) -> bool {
    if hw.0.is_null() || !IsWindow(Some(hw)).as_bool() || !IsWindowVisible(hw).as_bool() {
        return false;
    }
    if is_hwnd_cloaked(hw) {
        return false;
    }
    let mut rc = RECT::default();
    if GetWindowRect(hw, &mut rc).is_err() {
        return false;
    }
    (rc.right - rc.left) > 30 && (rc.bottom - rc.top) > 25
}

unsafe fn find_tray_overflow_hwnd(only_open: bool) -> HWND {
    for cls in [
        w!("TopLevelWindowForOverflowXamlIsland"),
        w!("NotifyIconOverflowWindow"),
    ] {
        if let Ok(hw) = FindWindowW(cls, PCWSTR::null()) {
            if !hw.0.is_null() && (!only_open || is_overflow_actually_open(hw)) {
                return hw;
            }
        }
    }
    HWND::default()
}

pub fn compute_tray_overflow_pos(
    anchor_rect: RECT,
    edge: MacDockEdge,
    mon_rc: RECT,
    ov_w: i32,
    ov_h: i32,
) -> (i32, i32) {
    let cx = (anchor_rect.left + anchor_rect.right) / 2;
    let cy = (anchor_rect.top + anchor_rect.bottom) / 2;
    let (raw_x, raw_y) = match edge {
        MacDockEdge::Bottom => (cx - ov_w / 2, anchor_rect.top - ov_h - 10),
        MacDockEdge::Top => (cx - ov_w / 2, anchor_rect.bottom + 10),
        MacDockEdge::Left => (anchor_rect.right + 10, cy - ov_h / 2),
        MacDockEdge::Right => (anchor_rect.left - ov_w - 10, cy - ov_h / 2),
    };
    let min_x = mon_rc.left + 8;
    let max_x = (mon_rc.right - ov_w - 8).max(min_x);
    let min_y = mon_rc.top + 8;
    let max_y = (mon_rc.bottom - ov_h - 8).max(min_y);
    (raw_x.clamp(min_x, max_x), raw_y.clamp(min_y, max_y))
}

static TRAY_OVERFLOW_SUMMONING: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);
static TRAY_OVERFLOW_ACTIVE: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);
static TRAY_OVERFLOW_ABORT: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);
static TRAY_TEMP_RGN_ACTIVE: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

pub fn is_tray_overflow_active() -> bool {
    TRAY_OVERFLOW_SUMMONING.load(std::sync::atomic::Ordering::SeqCst)
        || TRAY_OVERFLOW_ACTIVE.load(std::sync::atomic::Ordering::SeqCst)
}

pub fn is_any_tray_overflow_open() -> bool {
    if is_tray_overflow_active() {
        return true;
    }
    unsafe { !find_tray_overflow_hwnd(true).0.is_null() }
}

/// Clips `Shell_TrayWnd` to an empty 0x0 region instead of adding `WS_EX_LAYERED` + `LWA_ALPHA=0`.
/// Mutating `WS_EX_LAYERED` on Windows 11's DirectComposition/XAML `Shell_TrayWnd` forces a GDI
/// redirection surface that flashes/sticks as an opaque white bar across the bottom of the screen.
/// `SetWindowRgn(0,0,0,0)` works cross-process without altering `GWL_EXSTYLE` or XAML composition.
unsafe fn set_tray_region_hidden(tray: HWND, hide: bool) {
    if tray.0.is_null() {
        return;
    }
    if hide {
        let rgn = CreateRectRgn(0, 0, 0, 0);
        if !rgn.is_invalid() {
            if SetWindowRgn(tray, Some(rgn), false) != 0 {
                TRAY_TEMP_RGN_ACTIVE.store(true, std::sync::atomic::Ordering::SeqCst);
            } else {
                let _ = DeleteObject(HGDIOBJ(rgn.0));
            }
        }
    } else if TRAY_TEMP_RGN_ACTIVE.swap(false, std::sync::atomic::Ordering::SeqCst) {
        let _ = SetWindowRgn(tray, None, false);
    }
}

pub fn restore_tray_cloak_and_alpha() {
    TRAY_OVERFLOW_ABORT.store(true, std::sync::atomic::Ordering::SeqCst);
    TRAY_OVERFLOW_ACTIVE.store(false, std::sync::atomic::Ordering::SeqCst);
    TRAY_TEMP_RGN_ACTIVE.store(false, std::sync::atomic::Ordering::SeqCst);
    unsafe {
        if let Ok(tray) = FindWindowW(w!("Shell_TrayWnd"), PCWSTR::null()) {
            if !tray.0.is_null() {
                let _ = SetWindowRgn(tray, None, true);
            }
        }
    }
}

unsafe fn sync_rehide_tray_window(tray: HWND, was_tray_hidden: bool) {
    if tray.0.is_null() {
        return;
    }
    if was_tray_hidden && !TRAY_OVERFLOW_ABORT.load(std::sync::atomic::Ordering::SeqCst) {
        let _ = ShowWindow(tray, SW_HIDE);
        let _ = SendMessageTimeoutW(tray, WM_NULL, WPARAM(0), LPARAM(0), SMTO_ABORTIFHUNG, 50, None);
        for _ in 0..10 {
            if !IsWindowVisible(tray).as_bool() {
                break;
            }
            std::thread::sleep(Duration::from_millis(12));
        }
        let _ = DwmFlush();
        std::thread::sleep(Duration::from_millis(12));
    }
    // Restore normal full region only AFTER Shell_TrayWnd is confirmed hidden so zero frames flash.
    set_tray_region_hidden(tray, false);
}

unsafe fn try_chevron_in_root(
    uia: &IUIAutomation,
    btn_cond: &IUIAutomationCondition,
    cache_req: &IUIAutomationCacheRequest,
    root_hwnd: HWND,
) -> bool {
    if root_hwnd.0.is_null() {
        return false;
    }
    let Ok(root) = uia.ElementFromHandle(root_hwnd) else {
        return false;
    };
    let Ok(arr) = root.FindAllBuildCache(TreeScope_Subtree, btn_cond, cache_req) else {
        return false;
    };
    let count = arr.Length().unwrap_or(0).min(100);
    for i in 0..count {
        let Ok(el) = arr.GetElement(i) else { continue };
        let name = el
            .CachedName()
            .map(|b| b.to_string().to_ascii_lowercase())
            .unwrap_or_default();
        let aid = el
            .CachedAutomationId()
            .map(|b| b.to_string().to_ascii_lowercase())
            .unwrap_or_default();
        let is_chevron = name.contains("hidden")
            || name.contains("overflow")
            || name.contains("chevron")
            || aid.contains("hidden")
            || aid.contains("chevron")
            || aid.contains("overflow");
        if !is_chevron {
            continue;
        }
        // Windows 11 22H2-26300 uses a XAML ToggleButton for Show Hidden Icons
        if let Ok(tog) = el
            .GetCachedPatternAs::<IUIAutomationTogglePattern>(UIA_TogglePatternId)
            .or_else(|_| el.GetCurrentPatternAs::<IUIAutomationTogglePattern>(UIA_TogglePatternId))
        {
            if tog.Toggle().is_ok() {
                return true;
            }
        }
        if let Ok(inv) = el
            .GetCachedPatternAs::<IUIAutomationInvokePattern>(UIA_InvokePatternId)
            .or_else(|_| el.GetCurrentPatternAs::<IUIAutomationInvokePattern>(UIA_InvokePatternId))
        {
            if inv.Invoke().is_ok() {
                return true;
            }
        }
    }
    false
}

unsafe fn trigger_tray_chevron_uia(tray: HWND) -> bool {
    let Ok(uia) = CoCreateInstance::<_, IUIAutomation>(&CUIAutomation, None, CLSCTX_INPROC_SERVER)
    else {
        return false;
    };
    let Ok(btn_cond) = uia.CreatePropertyCondition(
        UIA_ControlTypePropertyId,
        &VARIANT::from(UIA_ButtonControlTypeId.0),
    ) else {
        return false;
    };
    let Ok(cache_req) = uia.CreateCacheRequest() else {
        return false;
    };
    let _ = cache_req.AddProperty(UIA_NamePropertyId);
    let _ = cache_req.AddProperty(UIA_AutomationIdPropertyId);
    let _ = cache_req.AddPattern(UIA_TogglePatternId);
    let _ = cache_req.AddPattern(UIA_InvokePatternId);
    let _ = cache_req.SetTreeScope(TreeScope_Element);

    // Search the compact TrayNotifyWnd child subtree first (~5 buttons) before falling back to full tray.
    if let Ok(notify) = FindWindowExW(Some(tray), None, w!("TrayNotifyWnd"), PCWSTR::null()) {
        if !notify.0.is_null() && try_chevron_in_root(&uia, &btn_cond, &cache_req, notify) {
            return true;
        }
    }
    try_chevron_in_root(&uia, &btn_cond, &cache_req, tray)
}

fn toggle_hidden_tray_overflow(dock_hwnd: HWND, anchor_rect: RECT, edge: MacDockEdge) {
    unsafe {
        let vis_ov = find_tray_overflow_hwnd(true);
        if !vis_ov.0.is_null() {
            let _ = ShowWindow(vis_ov, SW_HIDE);
            let _ = PostMessageW(Some(vis_ov), WM_NULL, WPARAM(0), LPARAM(0));
            return;
        }
    }
    if TRAY_OVERFLOW_SUMMONING.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return;
    }
    TRAY_OVERFLOW_ABORT.store(false, std::sync::atomic::Ordering::SeqCst);
    let dock_raw = dock_hwnd.0 as isize;
    std::thread::spawn(move || unsafe {
        struct BusyGuard;
        impl Drop for BusyGuard {
            fn drop(&mut self) {
                TRAY_OVERFLOW_SUMMONING.store(false, std::sync::atomic::Ordering::SeqCst);
                TRAY_OVERFLOW_ACTIVE.store(false, std::sync::atomic::Ordering::SeqCst);
            }
        }
        let _guard = BusyGuard;

        let com_ok = CoInitializeEx(None, COINIT_APARTMENTTHREADED).is_ok();
        let tray = FindWindowW(w!("Shell_TrayWnd"), PCWSTR::null()).unwrap_or_default();
        let was_tray_hidden = !tray.0.is_null() && !IsWindowVisible(tray).as_bool();
        TRAY_OVERFLOW_ACTIVE.store(true, std::sync::atomic::Ordering::SeqCst);

        // 1. Temporarily wake Shell_TrayWnd clipped to a 0x0 empty window region (zero pixels drawn,
        // no WS_EX_LAYERED white-bar corruption) so Win11's XAML tray island honors overflow toggle.
        if was_tray_hidden {
            set_tray_region_hidden(tray, true);
            let _ = DwmFlush();
            let _ = ShowWindow(tray, SW_SHOWNA);
            set_tray_region_hidden(tray, true);
        }

        let mut ov = HWND::default();
        if !tray.0.is_null() {
            let mut uia_invoked = false;
            // At most 2 single-RPC cached UIA probes (avoids hammering Explorer's UI thread at boot).
            for delay_ms in [35u64, 150] {
                if TRAY_OVERFLOW_ABORT.load(std::sync::atomic::Ordering::SeqCst) {
                    break;
                }
                std::thread::sleep(Duration::from_millis(delay_ms));
                if trigger_tray_chevron_uia(tray) {
                    uia_invoked = true;
                    for _ in 0..16 {
                        ov = find_tray_overflow_hwnd(true);
                        if !ov.0.is_null() {
                            break;
                        }
                        std::thread::sleep(Duration::from_millis(20));
                    }
                    break;
                }
            }
            if ov.0.is_null() && !uia_invoked {
                if let Ok(notify) = FindWindowExW(Some(tray), None, w!("TrayNotifyWnd"), PCWSTR::null()) {
                    if !notify.0.is_null() {
                        if let Ok(btn) = FindWindowExW(Some(notify), None, w!("Button"), PCWSTR::null()) {
                            if !btn.0.is_null() {
                                const BM_CLICK_MSG: u32 = 0x00F5;
                                let _ = SendMessageTimeoutW(
                                    btn,
                                    BM_CLICK_MSG,
                                    WPARAM(0),
                                    LPARAM(0),
                                    SMTO_ABORTIFHUNG,
                                    120,
                                    None,
                                );
                                for _ in 0..10 {
                                    ov = find_tray_overflow_hwnd(true);
                                    if !ov.0.is_null() {
                                        break;
                                    }
                                    std::thread::sleep(Duration::from_millis(16));
                                }
                            }
                        }
                    }
                }
            }
        }

        if !ov.0.is_null() {
            let position_ov = |target_ov: HWND| {
                let mut ov_rc = RECT::default();
                if GetWindowRect(target_ov, &mut ov_rc).is_ok() {
                    let ov_w = (ov_rc.right - ov_rc.left).max(100);
                    let ov_h = (ov_rc.bottom - ov_rc.top).max(60);
                    let cx = (anchor_rect.left + anchor_rect.right) / 2;
                    let cy = (anchor_rect.top + anchor_rect.bottom) / 2;
                    let mut mon_rc = RECT {
                        left: 0,
                        top: 0,
                        right: 1920,
                        bottom: 1080,
                    };
                    let hmon = MonitorFromPoint(POINT { x: cx, y: cy }, MONITOR_DEFAULTTONEAREST);
                    let mut mi = MONITORINFO {
                        cbSize: size_of::<MONITORINFO>() as u32,
                        ..Default::default()
                    };
                    if GetMonitorInfoW(hmon, &mut mi).as_bool() {
                        mon_rc = mi.rcMonitor;
                    }
                    let (tx, ty) = compute_tray_overflow_pos(anchor_rect, edge, mon_rc, ov_w, ov_h);
                    let _ = SetWindowPos(
                        target_ov,
                        Some(HWND_TOPMOST),
                        tx,
                        ty,
                        0,
                        0,
                        SWP_NOSIZE | SWP_SHOWWINDOW,
                    );
                }
            };
            position_ov(ov);
            let _ = AllowSetForegroundWindow(ASFW_ANY);
            let _ = SetForegroundWindow(ov);
            // Re-anchor once after XAML finishes layout settle
            std::thread::sleep(Duration::from_millis(35));
            if is_overflow_actually_open(ov) {
                position_ov(ov);
            }

            // Keep Shell_TrayWnd clipped to 0x0 while overflow is open so Explorer does not
            // auto-dismiss its owned XAML island; synchronize SW_HIDE + DwmFlush before unclipping.
            TRAY_OVERFLOW_SUMMONING.store(false, std::sync::atomic::Ordering::SeqCst);
            if was_tray_hidden {
                let wait_start = Instant::now();
                while is_overflow_actually_open(ov)
                    && wait_start.elapsed() < Duration::from_secs(30)
                    && !TRAY_OVERFLOW_ABORT.load(std::sync::atomic::Ordering::SeqCst)
                {
                    std::thread::sleep(Duration::from_millis(80));
                }
                sync_rehide_tray_window(tray, true);
            }
            TRAY_OVERFLOW_ACTIVE.store(false, std::sync::atomic::Ordering::SeqCst);
        } else {
            sync_rehide_tray_window(tray, was_tray_hidden);
            TRAY_OVERFLOW_ACTIVE.store(false, std::sync::atomic::Ordering::SeqCst);
            // Tier 2 fallback: sleek popup anchored next to chevron if Windows has 0 hidden icons
            let dhw = HWND(dock_raw as *mut c_void);
            if !dhw.0.is_null() {
                if let Ok(menu) = CreatePopupMenu() {
                    let hdr = to_wide("Hidden Tray Icons (None Active)");
                    let _ = AppendMenuW(menu, MF_STRING | MF_GRAYED, 0, PCWSTR(hdr.as_ptr()));
                    let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
                    let open_cfg = to_wide("Open Windows Taskbar Tray Settings...");
                    let _ = AppendMenuW(menu, MF_STRING, 1, PCWSTR(open_cfg.as_ptr()));
                    let cx = (anchor_rect.left + anchor_rect.right) / 2;
                    let cy = match edge {
                        MacDockEdge::Bottom => anchor_rect.top - 4,
                        MacDockEdge::Top => anchor_rect.bottom + 4,
                        _ => (anchor_rect.top + anchor_rect.bottom) / 2,
                    };
                    let mut mon_rc = RECT { left: 0, top: 0, right: 1920, bottom: 1080 };
                    let hmon = MonitorFromPoint(POINT { x: cx, y: cy }, MONITOR_DEFAULTTONEAREST);
                    let mut mi = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
                    if GetMonitorInfoW(hmon, &mut mi).as_bool() {
                        mon_rc = mi.rcMonitor;
                    }
                    let _ = SetForegroundWindow(dhw);
                    let chosen = track_dock_popup(dhw, menu, POINT { x: cx, y: cy }, edge, mon_rc);
                    let _ = DestroyMenu(menu);
                    if chosen == 1 {
                        let verb = to_wide("open");
                        let uri = to_wide("ms-settings:taskbar");
                        let _ = ShellExecuteW(
                            None,
                            PCWSTR(verb.as_ptr()),
                            PCWSTR(uri.as_ptr()),
                            PCWSTR::null(),
                            PCWSTR::null(),
                            SW_SHOWNORMAL,
                        );
                    }
                }
            }
        }
        if com_ok {
            CoUninitialize();
        }
    });
}

fn send_win_chord(vk_byte: u16) {
    let seq = [
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VK_LWIN,
                    wScan: 0,
                    dwFlags: KEYBD_EVENT_FLAGS(0),
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        },
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VIRTUAL_KEY(vk_byte),
                    wScan: 0,
                    dwFlags: KEYBD_EVENT_FLAGS(0),
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        },
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VIRTUAL_KEY(vk_byte),
                    wScan: 0,
                    dwFlags: KEYEVENTF_KEYUP,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        },
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VK_LWIN,
                    wScan: 0,
                    dwFlags: KEYEVENTF_KEYUP,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        },
    ];
    unsafe {
        let _ = SendInput(&seq, size_of::<INPUT>() as i32);
    }
}

unsafe fn run_as_admin(target: &str) {
    let verb = to_wide("runas");
    let file = to_wide(target);
    let _ = ShellExecuteW(
        None,
        PCWSTR(verb.as_ptr()),
        PCWSTR(file.as_ptr()),
        PCWSTR::null(),
        PCWSTR::null(),
        SW_SHOWNORMAL,
    );
}

unsafe fn open_file_location(target: &str) {
    let verb = to_wide("open");
    let exp = to_wide("explorer.exe");
    let args = to_wide(&format!("/select,\"{}\"", target));
    let _ = ShellExecuteW(
        None,
        PCWSTR(verb.as_ptr()),
        PCWSTR(exp.as_ptr()),
        PCWSTR(args.as_ptr()),
        PCWSTR::null(),
        SW_SHOWNORMAL,
    );
}

unsafe fn show_properties_dialog(hwnd: HWND, target: &str) {
    let verb = to_wide("properties");
    let file = to_wide(target);
    let mut sei = SHELLEXECUTEINFOW {
        cbSize: size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_INVOKEIDLIST | SEE_MASK_FLAG_NO_UI,
        hwnd,
        lpVerb: PCWSTR(verb.as_ptr()),
        lpFile: PCWSTR(file.as_ptr()),
        nShow: SW_SHOWNORMAL.0,
        ..Default::default()
    };
    let _ = ShellExecuteExW(&mut sei);
}

unsafe fn find_context_menu_start_cmd(
    cm: &IContextMenu,
    hmenu: HMENU,
) -> Option<(u32, &'static str)> {
    let count = GetMenuItemCount(Some(hmenu)).max(0) as u32;
    for pos in 0..count {
        let mut tbuf = [0u16; 128];
        let mut mii = MENUITEMINFOW {
            cbSize: size_of::<MENUITEMINFOW>() as u32,
            fMask: MIIM_ID | MIIM_STRING,
            dwTypeData: windows::core::PWSTR(tbuf.as_mut_ptr()),
            cch: tbuf.len() as u32,
            ..Default::default()
        };
        if GetMenuItemInfoW(hmenu, pos, true, &mut mii).is_err() || mii.wID == 0 {
            continue;
        }
        let id = mii.wID;
        let offset = id.saturating_sub(1);
        let mut vbuf = [0u16; 96];
        if cm
            .GetCommandString(
                offset as usize,
                GCS_VERBW,
                None,
                windows::core::PSTR(vbuf.as_mut_ptr() as *mut u8),
                vbuf.len() as u32,
            )
            .is_ok()
        {
            let end = vbuf.iter().position(|&c| c == 0).unwrap_or(0);
            let verb = String::from_utf16_lossy(&vbuf[..end]).to_ascii_lowercase();
            if verb == "pintostartscreen" {
                return Some((id, "pintostartscreen"));
            }
            if verb == "unpinfromstart" {
                return Some((id, "unpinfromstart"));
            }
        }
        let txt_end = tbuf.iter().position(|&c| c == 0).unwrap_or(0);
        let clean: String = String::from_utf16_lossy(&tbuf[..txt_end])
            .chars()
            .filter(|&c| c != '&')
            .collect::<String>()
            .to_ascii_lowercase();
        if clean.contains("unpin from start") {
            return Some((id, "unpinfromstart"));
        }
        if clean.contains("pin to start") {
            return Some((id, "pintostartscreen"));
        }
    }
    None
}

unsafe fn invoke_shell_start_verb(hwnd: HWND, target: &str) -> bool {
    let wpath = to_wide(target);
    let Ok(item) = SHCreateItemFromParsingName::<_, _, IShellItem>(PCWSTR(wpath.as_ptr()), None) else {
        return false;
    };
    let Ok(cm) = item.BindToHandler::<_, IContextMenu>(None, &BHID_SFUIObject) else {
        return false;
    };
    let Ok(hmenu) = CreatePopupMenu() else {
        return false;
    };
    let _ = cm.QueryContextMenu(hmenu, 0, 1, 0x7FFF, CMF_NORMAL | CMF_EXPLORE);
    let res = if let Some((id, _)) = find_context_menu_start_cmd(&cm, hmenu) {
        let off = (id - 1) as usize;
        let ici = CMINVOKECOMMANDINFOEX {
            cbSize: size_of::<CMINVOKECOMMANDINFOEX>() as u32,
            fMask: SEE_MASK_UNICODE,
            hwnd,
            lpVerb: PCSTR(off as *const u8),
            lpVerbW: PCWSTR(off as *const u16),
            nShow: SW_SHOWNORMAL.0,
            ..Default::default()
        };
        cm.InvokeCommand(&ici as *const CMINVOKECOMMANDINFOEX as *const CMINVOKECOMMANDINFO)
            .is_ok()
    } else {
        false
    };
    let _ = DestroyMenu(hmenu);
    res
}

unsafe fn show_native_shell_context_menu(
    hwnd: HWND,
    target: &str,
    pt: POINT,
    edge: MacDockEdge,
    mon: RECT,
) {
    let wpath = to_wide(target);
    let Ok(item) = SHCreateItemFromParsingName::<_, _, IShellItem>(PCWSTR(wpath.as_ptr()), None) else {
        return;
    };
    let Ok(cm) = item.BindToHandler::<_, IContextMenu>(None, &BHID_SFUIObject) else {
        return;
    };
    let Ok(hmenu) = CreatePopupMenu() else {
        return;
    };
    if cm
        .QueryContextMenu(hmenu, 0, 1, 0x7FFF, CMF_NORMAL | CMF_EXPLORE)
        .is_ok()
    {
        let cmd = track_dock_popup(hwnd, hmenu, pt, edge, mon);
        if cmd >= 1 {
            let off = cmd - 1;
            let ici = CMINVOKECOMMANDINFOEX {
                cbSize: size_of::<CMINVOKECOMMANDINFOEX>() as u32,
                fMask: SEE_MASK_UNICODE,
                hwnd,
                lpVerb: PCSTR(off as *const u8),
                lpVerbW: PCWSTR(off as *const u16),
                nShow: SW_SHOWNORMAL.0,
                ..Default::default()
            };
            let _ = cm.InvokeCommand(&ici as *const CMINVOKECOMMANDINFOEX as *const CMINVOKECOMMANDINFO);
        }
    }
    let _ = DestroyMenu(hmenu);
}

fn start_programs_lnk_path(name: &str) -> Option<std::path::PathBuf> {
    let appdata = std::env::var_os("APPDATA")?;
    let clean: String = name
        .chars()
        .map(|c| if "\\/:*?\"<>|".contains(c) { '_' } else { c })
        .collect();
    let trimmed = clean.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(
        std::path::PathBuf::from(appdata)
            .join(r"Microsoft\Windows\Start Menu\Programs")
            .join(format!("{}.lnk", trimmed)),
    )
}

fn is_in_start_programs(name: &str) -> bool {
    start_programs_lnk_path(name)
        .map(|p| p.exists())
        .unwrap_or(false)
}

fn toggle_start_programs_shortcut(name: &str, target: &str) {
    let Some(lnk_path) = start_programs_lnk_path(name) else {
        return;
    };
    if lnk_path.exists() {
        let _ = std::fs::remove_file(&lnk_path);
        return;
    }
    unsafe {
        if let Ok(link) = CoCreateInstance::<_, IShellLinkW>(&ShellLink, None, CLSCTX_INPROC_SERVER) {
            let w_tgt = to_wide(target);
            let _ = link.SetPath(PCWSTR(w_tgt.as_ptr()));
            if let Ok(pf) = link.cast::<IPersistFile>() {
                if let Some(s) = lnk_path.to_str() {
                    let w_lnk = to_wide(s);
                    let _ = pf.Save(PCWSTR(w_lnk.as_ptr()), true);
                }
            }
        }
    }
}

unsafe fn add_menu_item(hmenu: HMENU, id: usize, label: &str) {
    let w = to_wide(label);
    let _ = AppendMenuW(hmenu, MF_STRING, id, PCWSTR(w.as_ptr()));
}

unsafe fn add_checked_item(hmenu: HMENU, id: usize, label: &str, checked: bool) {
    add_menu_item(hmenu, id, label);
    let _ = CheckMenuItem(
        hmenu,
        id as u32,
        MF_BYCOMMAND.0 | if checked { MF_CHECKED.0 } else { MF_UNCHECKED.0 },
    );
}

unsafe fn add_grayed_item(hmenu: HMENU, id: usize, label: &str) {
    let w = to_wide(label);
    let _ = AppendMenuW(hmenu, MF_STRING | MF_GRAYED, id, PCWSTR(w.as_ptr()));
}

unsafe fn add_theme_override_submenu(
    parent: HMENU,
    base_id: usize,
    current: Option<TaskbarTheme>,
    title: &str,
) {
    let Ok(sub) = CreatePopupMenu() else {
        return;
    };
    add_checked_item(sub, base_id, "Match Global Theme (Default)", current.is_none());
    let _ = AppendMenuW(sub, MF_SEPARATOR, 0, PCWSTR::null());
    for (idx, &t) in TaskbarTheme::VIRTUAL_DOCK_THEMES.iter().enumerate() {
        add_checked_item(sub, base_id + 1 + idx, t.label(), current == Some(t));
    }
    let w = to_wide(title);
    let _ = AppendMenuW(parent, MF_POPUP, sub.0 as usize, PCWSTR(w.as_ptr()));
}

unsafe fn append_widget_customize_submenu(
    parent: HMENU,
    widget: TaskbarWidgetId,
    cfg: &AppConfig,
) {
    let Ok(sub) = CreatePopupMenu() else {
        return;
    };
    let menu_title = match widget {
        TaskbarWidgetId::ShelfFrame => {
            add_theme_override_submenu(sub, IDM_W_THEME_SHELF_BASE, cfg.w_shelf.theme_override, "Shelf Theme Override");
            if let Ok(l_sub) = CreatePopupMenu() {
                add_checked_item(l_sub, IDM_W_SHELF_LAYOUT_INHERIT, "Theme Default", cfg.w_shelf.layout_override.is_none());
                for (i, &k) in DockLayoutKind::ALL.iter().enumerate() {
                    add_checked_item(l_sub, IDM_W_SHELF_LAYOUT_BASE + i, k.label(), cfg.w_shelf.layout_override == Some(k));
                }
                let w = to_wide("Layout Architecture");
                let _ = AppendMenuW(sub, MF_POPUP, l_sub.0 as usize, PCWSTR(w.as_ptr()));
            }
            if let Ok(s_sub) = CreatePopupMenu() {
                add_checked_item(s_sub, IDM_W_SHELF_SHAPE_INHERIT, "Theme Default", cfg.w_shelf.shape_override.is_none());
                for (i, &sh) in DockShelfShape::ALL.iter().enumerate() {
                    add_checked_item(s_sub, IDM_W_SHELF_SHAPE_BASE + i, sh.label(), cfg.w_shelf.shape_override == Some(sh));
                }
                let w = to_wide("Shelf Geometry");
                let _ = AppendMenuW(sub, MF_POPUP, s_sub.0 as usize, PCWSTR(w.as_ptr()));
            }
            "Customize Shelf & Frame"
        }
        TaskbarWidgetId::AppDock => {
            add_theme_override_submenu(sub, IDM_W_THEME_APPS_BASE, cfg.w_apps.theme_override, "App Dock Theme Override");
            if let Ok(c_sub) = CreatePopupMenu() {
                for (i, &cs) in DockSlotCardStyle::ALL.iter().enumerate() {
                    add_checked_item(c_sub, IDM_W_APPS_CARD_BASE + i, cs.label(), cfg.w_apps.card_style == cs);
                }
                let w = to_wide("Slot Card Style");
                let _ = AppendMenuW(sub, MF_POPUP, c_sub.0 as usize, PCWSTR(w.as_ptr()));
            }
            if let Ok(i_sub) = CreatePopupMenu() {
                add_checked_item(i_sub, IDM_W_APPS_IND_INHERIT, "Theme Default", cfg.w_apps.indicator_override.is_none());
                for (i, &ind) in DockIndicatorStyle::ALL.iter().enumerate() {
                    add_checked_item(i_sub, IDM_W_APPS_IND_BASE + i, ind.label(), cfg.w_apps.indicator_override == Some(ind));
                }
                let w = to_wide("Running Indicator Style");
                let _ = AppendMenuW(sub, MF_POPUP, i_sub.0 as usize, PCWSTR(w.as_ptr()));
            }
            if let Ok(d_sub) = CreatePopupMenu() {
                for (i, &dv) in DockDividerStyle::ALL.iter().enumerate() {
                    add_checked_item(d_sub, IDM_W_APPS_DIV_BASE + i, dv.label(), cfg.w_apps.divider_style == dv);
                }
                let w = to_wide("Divider Style");
                let _ = AppendMenuW(sub, MF_POPUP, d_sub.0 as usize, PCWSTR(w.as_ptr()));
            }
            if let Ok(lb_sub) = CreatePopupMenu() {
                for (i, &lm) in DockRunningLabelMode::ALL.iter().enumerate() {
                    add_checked_item(lb_sub, IDM_W_APPS_LBL_BASE + i, lm.label(), cfg.w_apps.label_mode == lm);
                }
                let w = to_wide("Window Title Labels");
                let _ = AppendMenuW(sub, MF_POPUP, lb_sub.0 as usize, PCWSTR(w.as_ptr()));
            }
            let _ = AppendMenuW(sub, MF_SEPARATOR, 0, PCWSTR::null());
            add_checked_item(sub, IDM_W_APPS_BRAND_GLOW, "Per-App Dynamic Brand Glow", cfg.w_apps.per_app_brand_glow);
            add_checked_item(sub, IDM_W_APPS_COUNT_BADGE, "Multi-Window Count Badge", cfg.w_apps.show_window_count_badge);
            let _ = AppendMenuW(sub, MF_SEPARATOR, 0, PCWSTR::null());
            add_menu_item(sub, IDM_W_APPS_RESET, "Reset App Dock Widget");
            "Customize App Dock Widget"
        }
        TaskbarWidgetId::Clock => {
            add_theme_override_submenu(sub, IDM_W_THEME_CLOCK_BASE, cfg.w_clock.theme_override, "Clock Theme Override");
            for (i, &eng) in DockClockEngine::ALL.iter().enumerate() {
                add_checked_item(sub, IDM_W_CLOCK_ENG_BASE + i, eng.label(), cfg.w_clock.engine == eng);
            }
            let _ = AppendMenuW(sub, MF_SEPARATOR, 0, PCWSTR::null());
            add_checked_item(sub, IDM_W_CLOCK_SECONDS, "Show Seconds", cfg.w_clock.show_seconds);
            add_checked_item(sub, IDM_W_CLOCK_DATE, "Show Date Line", cfg.w_clock.show_date);
            add_checked_item(
                sub,
                IDM_W_CLOCK_24H_TOGGLE,
                "24-Hour Time Format",
                cfg.w_clock.use_24h.unwrap_or(cfg.widgets_24h),
            );
            let _ = AppendMenuW(sub, MF_SEPARATOR, 0, PCWSTR::null());
            for (i, &wa) in DockClockWheelAction::ALL.iter().enumerate() {
                add_checked_item(sub, IDM_W_CLOCK_WHEEL_BASE + i, wa.label(), cfg.w_clock.wheel_action == wa);
            }
            let _ = AppendMenuW(sub, MF_SEPARATOR, 0, PCWSTR::null());
            add_menu_item(sub, IDM_W_CLOCK_RESET, "Reset Clock Widget");
            "Customize Clock Widget"
        }
        TaskbarWidgetId::SystemTray => {
            add_theme_override_submenu(sub, IDM_W_THEME_TRAY_BASE, cfg.w_tray.theme_override, "Tray Theme Override");
            if let Ok(a_sub) = CreatePopupMenu() {
                for (i, &st) in TrayAudioMeterStyle::ALL.iter().enumerate() {
                    add_checked_item(a_sub, IDM_W_TRAY_AUD_BASE + i, st.label(), cfg.w_tray.audio_style == st);
                }
                let w = to_wide("Audio / Volume Meter");
                let _ = AppendMenuW(sub, MF_POPUP, a_sub.0 as usize, PCWSTR(w.as_ptr()));
            }
            if let Ok(b_sub) = CreatePopupMenu() {
                for (i, &st) in TrayBatteryMeterStyle::ALL.iter().enumerate() {
                    add_checked_item(b_sub, IDM_W_TRAY_BAT_BASE + i, st.label(), cfg.w_tray.battery_style == st);
                }
                let w = to_wide("Battery Gauge Style");
                let _ = AppendMenuW(sub, MF_POPUP, b_sub.0 as usize, PCWSTR(w.as_ptr()));
            }
            if let Ok(n_sub) = CreatePopupMenu() {
                for (i, &st) in TrayNetMeterStyle::ALL.iter().enumerate() {
                    add_checked_item(n_sub, IDM_W_TRAY_NET_BASE + i, st.label(), cfg.w_tray.net_style == st);
                }
                let w = to_wide("Network Indicator Style");
                let _ = AppendMenuW(sub, MF_POPUP, n_sub.0 as usize, PCWSTR(w.as_ptr()));
            }
            let _ = AppendMenuW(sub, MF_SEPARATOR, 0, PCWSTR::null());
            add_menu_item(sub, IDM_W_TRAY_RESET, "Reset System Tray Widget");
            "Customize System Tray Widget"
        }
        TaskbarWidgetId::HudWing => {
            add_theme_override_submenu(sub, IDM_W_THEME_HUD_BASE, cfg.w_hud.theme_override, "HUD Wing Theme Override");
            for (i, &m) in HudWingMode::ALL.iter().enumerate() {
                add_checked_item(sub, IDM_W_HUD_MODE_BASE + i, m.label(), cfg.w_hud.mode == m);
            }
            let _ = AppendMenuW(sub, MF_SEPARATOR, 0, PCWSTR::null());
            for (i, &sh) in HudWingShape::ALL.iter().enumerate() {
                add_checked_item(sub, IDM_W_HUD_SHAPE_BASE + i, sh.label(), cfg.w_hud.shape == sh);
            }
            let _ = AppendMenuW(sub, MF_SEPARATOR, 0, PCWSTR::null());
            for (i, &ct) in HudTelemetryContent::ALL.iter().enumerate() {
                add_checked_item(sub, IDM_W_HUD_CONTENT_BASE + i, ct.label(), cfg.w_hud.content == ct);
            }
            let _ = AppendMenuW(sub, MF_SEPARATOR, 0, PCWSTR::null());
            add_checked_item(sub, IDM_W_HUD_MARQUEE, "Smooth Marquee Title Ticker", cfg.w_hud.marquee_ticker);
            add_menu_item(sub, IDM_W_HUD_RESET, "Reset HUD Wing Widget");
            "Customize HUD Telemetry Wing"
        }
        TaskbarWidgetId::Utilities => {
            for (i, &sb) in StartBadgeStyle::ALL.iter().enumerate() {
                add_checked_item(sub, IDM_W_UTIL_START_BASE + i, sb.label(), cfg.w_util.start_style == sb);
            }
            let _ = AppendMenuW(sub, MF_SEPARATOR, 0, PCWSTR::null());
            add_checked_item(sub, IDM_W_UTIL_TOGGLE_START, "Show Start Button", cfg.w_util.show_start);
            add_checked_item(sub, IDM_W_UTIL_TOGGLE_SEARCH, "Show Search Button", cfg.w_util.show_search);
            add_checked_item(sub, IDM_W_UTIL_TOGGLE_TASKVIEW, "Show Task View Button", cfg.w_util.show_taskview);
            add_checked_item(sub, IDM_W_UTIL_TOGGLE_TRASH, "Show Recycle Bin", cfg.w_util.show_trash);
            add_checked_item(sub, IDM_W_UTIL_TOGGLE_STUDIO, "Show Studio Customizer Button", cfg.w_util.show_studio);
            add_checked_item(sub, IDM_W_UTIL_TOGGLE_SLIVER, "Show Desktop Edge Sliver", cfg.w_util.show_desktop_sliver);
            "Customize Launcher & Utilities"
        }
    };
    let w = to_wide(menu_title);
    let _ = AppendMenuW(parent, MF_POPUP, sub.0 as usize, PCWSTR(w.as_ptr()));
}

unsafe fn draw_start_badge(
    rt: &ID2D1DCRenderTarget,
    br: &ID2D1SolidColorBrush,
    rect: D2D_RECT_F,
    accent: (u8, u8, u8),
    is_crt: bool,
    style: StartBadgeStyle,
    draw_plate: bool,
    u: f32,
) {
    let w = rect.right - rect.left;
    let eff_crt = style == StartBadgeStyle::TerminalPrompt
        || (style == StartBadgeStyle::InheritTheme && is_crt);
    if draw_plate {
        let pad = w * 0.10;
        let plate = D2D1_ROUNDED_RECT {
            rect: D2D_RECT_F {
                left: rect.left + pad,
                top: rect.top + pad,
                right: rect.right - pad,
                bottom: rect.bottom - pad,
            },
            radiusX: w * 0.22,
            radiusY: w * 0.22,
        };
        if eff_crt {
            br.SetColor(&rgba(10, 32, 18, 0.94));
            rt.FillRoundedRectangle(&plate, br);
            br.SetColor(&rgba(84, 255, 140, 0.72));
        } else {
            br.SetColor(&rgba(34, 48, 74, 0.92));
            rt.FillRoundedRectangle(&plate, br);
            br.SetColor(&rgba(accent.0, accent.1, accent.2, 0.62));
        }
        rt.DrawRoundedRectangle(&plate, br, 1.0 * u, None);
    }

    let cx = (rect.left + rect.right) * 0.5;
    let cy = (rect.top + rect.bottom) * 0.5;

    match style {
        StartBadgeStyle::CyberHexCore => {
            let r_out = w * 0.25;
            let r_in = w * 0.11;
            br.SetColor(&rgba(accent.0, accent.1, accent.2, 0.95));
            rt.DrawEllipse(
                &windows::Win32::Graphics::Direct2D::D2D1_ELLIPSE {
                    point: Vector2 { X: cx, Y: cy },
                    radiusX: r_out,
                    radiusY: r_out,
                },
                br,
                1.5 * u,
                None,
            );
            rt.FillEllipse(
                &windows::Win32::Graphics::Direct2D::D2D1_ELLIPSE {
                    point: Vector2 { X: cx, Y: cy },
                    radiusX: r_in,
                    radiusY: r_in,
                },
                br,
            );
        }
        StartBadgeStyle::TerminalPrompt => {
            let s = w * 0.16;
            br.SetColor(&rgba(98, 255, 152, 0.96));
            rt.DrawLine(
                Vector2 { X: cx - s, Y: cy - s * 0.75 },
                Vector2 { X: cx - s * 0.1, Y: cy },
                br,
                1.8 * u,
                None,
            );
            rt.DrawLine(
                Vector2 { X: cx - s, Y: cy + s * 0.75 },
                Vector2 { X: cx - s * 0.1, Y: cy },
                br,
                1.8 * u,
                None,
            );
            rt.DrawLine(
                Vector2 { X: cx + s * 0.15, Y: cy + s * 0.75 },
                Vector2 { X: cx + s * 1.05, Y: cy + s * 0.75 },
                br,
                1.8 * u,
                None,
            );
        }
        StartBadgeStyle::AquaOrb => {
            let r_orb = w * 0.24;
            br.SetColor(&rgba(accent.0, accent.1, accent.2, 0.88));
            rt.FillEllipse(
                &windows::Win32::Graphics::Direct2D::D2D1_ELLIPSE {
                    point: Vector2 { X: cx, Y: cy },
                    radiusX: r_orb,
                    radiusY: r_orb,
                },
                br,
            );
            br.SetColor(&rgba(255, 255, 255, 0.68));
            rt.FillEllipse(
                &windows::Win32::Graphics::Direct2D::D2D1_ELLIPSE {
                    point: Vector2 { X: cx - r_orb * 0.28, Y: cy - r_orb * 0.28 },
                    radiusX: r_orb * 0.36,
                    radiusY: r_orb * 0.24,
                },
                br,
            );
        }
        StartBadgeStyle::InheritTheme | StartBadgeStyle::Windows11Grid => {
            let tile_s = w * 0.20;
            let off = tile_s * 0.72;
            let cols = if eff_crt {
                [(78u8, 246u8, 128u8), (118, 255, 162), (62, 212, 108), (94, 242, 138)]
            } else {
                [(108u8, 198u8, 255u8), (146, 224, 255), (82, 168, 245), (120, 214, 248)]
            };
            for (idx, (ox, oy)) in [(-off, -off), (off, -off), (-off, off), (off, off)]
                .into_iter()
                .enumerate()
            {
                let (cr, cg, cb) = cols[idx];
                br.SetColor(&rgba(cr, cg, cb, 0.95));
                let t_rr = D2D1_ROUNDED_RECT {
                    rect: D2D_RECT_F {
                        left: cx + ox - tile_s * 0.5,
                        top: cy + oy - tile_s * 0.5,
                        right: cx + ox + tile_s * 0.5,
                        bottom: cy + oy + tile_s * 0.5,
                    },
                    radiusX: 2.2 * u,
                    radiusY: 2.2 * u,
                };
                rt.FillRoundedRectangle(&t_rr, br);
            }
        }
    }
}

unsafe fn draw_trash_badge(
    rt: &ID2D1DCRenderTarget,
    br: &ID2D1SolidColorBrush,
    rect: D2D_RECT_F,
    has_items: bool,
    dark_ink: bool,
    u: f32,
) {
    let w = rect.right - rect.left;
    let cx = (rect.left + rect.right) * 0.5;
    let cy = (rect.top + rect.bottom) * 0.5;
    let bw = w * 0.54;
    let bh = w * 0.58;

    if has_items {
        let p_rr = D2D1_ROUNDED_RECT {
            rect: D2D_RECT_F {
                left: cx - bw * 0.34,
                top: cy - bh * 0.42,
                right: cx + bw * 0.28,
                bottom: cy + bh * 0.08,
            },
            radiusX: 2.0 * u,
            radiusY: 2.0 * u,
        };
        br.SetColor(&if dark_ink { rgba(52, 48, 42, 0.85) } else { rgba(244, 248, 255, 0.88) });
        rt.FillRoundedRectangle(&p_rr, br);
    }

    let bin_rr = D2D1_ROUNDED_RECT {
        rect: D2D_RECT_F {
            left: cx - bw * 0.5,
            top: cy - bh * 0.30,
            right: cx + bw * 0.5,
            bottom: cy + bh * 0.54,
        },
        radiusX: 4.5 * u,
        radiusY: 4.5 * u,
    };
    br.SetColor(&if dark_ink { rgba(68, 64, 58, 0.28) } else { rgba(186, 198, 214, 0.34) });
    rt.FillRoundedRectangle(&bin_rr, br);
    br.SetColor(&if dark_ink { rgba(34, 32, 28, 0.88) } else { rgba(232, 240, 250, 0.78) });
    rt.DrawRoundedRectangle(&bin_rr, br, 1.3 * u, None);

    let rim_rr = D2D1_ROUNDED_RECT {
        rect: D2D_RECT_F {
            left: cx - bw * 0.56,
            top: cy - bh * 0.36,
            right: cx + bw * 0.56,
            bottom: cy - bh * 0.20,
        },
        radiusX: 3.0 * u,
        radiusY: 3.0 * u,
    };
    br.SetColor(&if dark_ink { rgba(38, 36, 32, 0.90) } else { rgba(220, 232, 246, 0.85) });
    rt.FillRoundedRectangle(&rim_rr, br);

    br.SetColor(&if dark_ink { rgba(34, 32, 28, 0.55) } else { rgba(245, 250, 255, 0.42) });
    for rx in [-bw * 0.20, 0.0, bw * 0.20] {
        rt.DrawLine(
            Vector2 { X: cx + rx, Y: cy - bh * 0.08 },
            Vector2 { X: cx + rx, Y: cy + bh * 0.36 },
            br,
            1.1 * u,
            None,
        );
    }
}

unsafe fn draw_studio_badge(
    rt: &ID2D1DCRenderTarget,
    br: &ID2D1SolidColorBrush,
    rect: D2D_RECT_F,
    accent: (u8, u8, u8),
    draw_plate: bool,
    u: f32,
) {
    let w = rect.right - rect.left;
    if draw_plate {
        let pad = w * 0.10;
        let plate = D2D1_ROUNDED_RECT {
            rect: D2D_RECT_F {
                left: rect.left + pad,
                top: rect.top + pad,
                right: rect.right - pad,
                bottom: rect.bottom - pad,
            },
            radiusX: w * 0.22,
            radiusY: w * 0.22,
        };
        br.SetColor(&rgba(34, 24, 18, 0.94));
        rt.FillRoundedRectangle(&plate, br);
        br.SetColor(&rgba(accent.0, accent.1, accent.2, 0.72));
        rt.DrawRoundedRectangle(&plate, br, 1.2 * u, None);
    }

    let cx = (rect.left + rect.right) * 0.5;
    let cy = (rect.top + rect.bottom) * 0.5;
    let pill = D2D1_ROUNDED_RECT {
        rect: D2D_RECT_F {
            left: cx - w * 0.21,
            top: cy - w * 0.22,
            right: cx + w * 0.01,
            bottom: cy + w * 0.22,
        },
        radiusX: 3.0 * u,
        radiusY: 3.0 * u,
    };
    br.SetColor(&rgba(245, 172, 72, 0.96));
    rt.FillRoundedRectangle(&pill, br);

    let bar2 = D2D1_ROUNDED_RECT {
        rect: D2D_RECT_F {
            left: cx + w * 0.07,
            top: cy - w * 0.14,
            right: cx + w * 0.21,
            bottom: cy + w * 0.14,
        },
        radiusX: 2.5 * u,
        radiusY: 2.5 * u,
    };
    br.SetColor(&rgba(255, 242, 220, 0.90));
    rt.FillRoundedRectangle(&bar2, br);
}

unsafe fn draw_fallback_badge(
    rt: &ID2D1DCRenderTarget,
    br: &ID2D1SolidColorBrush,
    rect: D2D_RECT_F,
    u: f32,
) {
    let w = rect.right - rect.left;
    let pad = w * 0.12;
    let rr = D2D1_ROUNDED_RECT {
        rect: D2D_RECT_F {
            left: rect.left + pad,
            top: rect.top + pad,
            right: rect.right - pad,
            bottom: rect.bottom - pad,
        },
        radiusX: w * 0.20,
        radiusY: w * 0.20,
    };
    br.SetColor(&rgba(130, 168, 214, 0.75));
    rt.FillRoundedRectangle(&rr, br);
    br.SetColor(&rgba(235, 245, 255, 0.85));
    rt.DrawRoundedRectangle(&rr, br, 1.0 * u, None);
}

unsafe fn draw_search_badge(
    rt: &ID2D1DCRenderTarget,
    br: &ID2D1SolidColorBrush,
    rect: D2D_RECT_F,
    fg_col: (u8, u8, u8),
    accent_col: (u8, u8, u8),
    u: f32,
) {
    let w = rect.right - rect.left;
    let cx = (rect.left + rect.right) * 0.5 - w * 0.06;
    let cy = (rect.top + rect.bottom) * 0.5 - w * 0.06;
    let r = (w * 0.22).max(4.5 * u);
    br.SetColor(&rgba(accent_col.0, accent_col.1, accent_col.2, 0.16));
    rt.FillEllipse(
        &windows::Win32::Graphics::Direct2D::D2D1_ELLIPSE {
            point: Vector2 { X: cx, Y: cy },
            radiusX: r,
            radiusY: r,
        },
        br,
    );
    br.SetColor(&rgba(fg_col.0, fg_col.1, fg_col.2, 0.92));
    rt.DrawEllipse(
        &windows::Win32::Graphics::Direct2D::D2D1_ELLIPSE {
            point: Vector2 { X: cx, Y: cy },
            radiusX: r,
            radiusY: r,
        },
        br,
        1.7 * u,
        None,
    );
    let d0 = r * 0.72;
    let d1 = r * 1.48;
    rt.DrawLine(
        Vector2 { X: cx + d0, Y: cy + d0 },
        Vector2 { X: cx + d1, Y: cy + d1 },
        br,
        2.1 * u,
        None,
    );
}

unsafe fn draw_taskview_badge(
    rt: &ID2D1DCRenderTarget,
    br: &ID2D1SolidColorBrush,
    rect: D2D_RECT_F,
    fg_col: (u8, u8, u8),
    accent_col: (u8, u8, u8),
    u: f32,
) {
    let w = rect.right - rect.left;
    let cx = (rect.left + rect.right) * 0.5;
    let cy = (rect.top + rect.bottom) * 0.5;
    let bw = (w * 0.42).max(8.0 * u);
    let bh = (w * 0.32).max(6.5 * u);
    let back = D2D1_ROUNDED_RECT {
        rect: D2D_RECT_F {
            left: cx - bw * 0.65,
            top: cy - bh * 0.70,
            right: cx + bw * 0.35,
            bottom: cy + bh * 0.30,
        },
        radiusX: 2.2 * u,
        radiusY: 2.2 * u,
    };
    br.SetColor(&rgba(accent_col.0, accent_col.1, accent_col.2, 0.45));
    rt.FillRoundedRectangle(&back, br);
    br.SetColor(&rgba(accent_col.0, accent_col.1, accent_col.2, 0.85));
    rt.DrawRoundedRectangle(&back, br, 1.2 * u, None);

    let front = D2D1_ROUNDED_RECT {
        rect: D2D_RECT_F {
            left: cx - bw * 0.32,
            top: cy - bh * 0.25,
            right: cx + bw * 0.68,
            bottom: cy + bh * 0.75,
        },
        radiusX: 2.2 * u,
        radiusY: 2.2 * u,
    };
    br.SetColor(&rgba(fg_col.0, fg_col.1, fg_col.2, 0.94));
    rt.DrawRoundedRectangle(&front, br, 1.45 * u, None);
}

unsafe fn draw_hidden_tray_chevron_badge(
    rt: &ID2D1DCRenderTarget,
    br: &ID2D1SolidColorBrush,
    rect: D2D_RECT_F,
    fg_col: (u8, u8, u8),
    accent_col: (u8, u8, u8),
    is_hov: bool,
    edge: MacDockEdge,
    u: f32,
) {
    let cx = (rect.left + rect.right) * 0.5;
    let cy = (rect.top + rect.bottom) * 0.5;
    let col = if is_hov { accent_col } else { fg_col };
    if is_hov {
        br.SetColor(&rgba(accent_col.0, accent_col.1, accent_col.2, 0.22));
        rt.FillRoundedRectangle(
            &D2D1_ROUNDED_RECT {
                rect: D2D_RECT_F {
                    left: cx - 8.0 * u,
                    top: cy - 8.0 * u,
                    right: cx + 8.0 * u,
                    bottom: cy + 8.0 * u,
                },
                radiusX: 5.0 * u,
                radiusY: 5.0 * u,
            },
            br,
        );
    }
    br.SetColor(&rgba(col.0, col.1, col.2, if is_hov { 0.98 } else { 0.84 }));
    let arm = 4.4 * u;
    let lift = 2.2 * u;
    let (p0, apex, p1) = match edge {
        MacDockEdge::Bottom => (
            Vector2 { X: cx - arm, Y: cy + lift },
            Vector2 { X: cx, Y: cy - lift },
            Vector2 { X: cx + arm, Y: cy + lift },
        ),
        MacDockEdge::Top => (
            Vector2 { X: cx - arm, Y: cy - lift },
            Vector2 { X: cx, Y: cy + lift },
            Vector2 { X: cx + arm, Y: cy - lift },
        ),
        MacDockEdge::Left => (
            Vector2 { X: cx - lift, Y: cy - arm },
            Vector2 { X: cx + lift, Y: cy },
            Vector2 { X: cx - lift, Y: cy + arm },
        ),
        MacDockEdge::Right => (
            Vector2 { X: cx + lift, Y: cy - arm },
            Vector2 { X: cx - lift, Y: cy },
            Vector2 { X: cx + lift, Y: cy + arm },
        ),
    };
    let w_st = 1.75 * u;
    rt.DrawLine(p0, apex, br, w_st, None);
    rt.DrawLine(apex, p1, br, w_st, None);
}

unsafe fn draw_d2d_arc(
    rt: &ID2D1DCRenderTarget,
    d2d: Option<&ID2D1Factory>,
    cx: f32,
    cy: f32,
    r: f32,
    start_deg: f32,
    sweep_deg: f32,
    stroke_w: f32,
    br: &ID2D1SolidColorBrush,
) {
    if r <= 0.5 || sweep_deg <= 1.0 {
        return;
    }
    if sweep_deg >= 359.0 {
        rt.DrawEllipse(
            &windows::Win32::Graphics::Direct2D::D2D1_ELLIPSE {
                point: Vector2 { X: cx, Y: cy },
                radiusX: r,
                radiusY: r,
            },
            br,
            stroke_w,
            None,
        );
        return;
    }
    if let Some(factory) = d2d {
        let a0 = (start_deg - 90.0).to_radians();
        let a1 = (start_deg + sweep_deg - 90.0).to_radians();
        let p0 = Vector2 { X: cx + r * a0.cos(), Y: cy + r * a0.sin() };
        let p1 = Vector2 { X: cx + r * a1.cos(), Y: cy + r * a1.sin() };
        if let Ok(geo) = factory.CreatePathGeometry() {
            if let Ok(sink) = geo.Open() {
                sink.BeginFigure(p0, D2D1_FIGURE_BEGIN_HOLLOW);
                sink.AddArc(&D2D1_ARC_SEGMENT {
                    point: p1,
                    size: D2D_SIZE_F { width: r, height: r },
                    rotationAngle: 0.0,
                    sweepDirection: D2D1_SWEEP_DIRECTION_CLOCKWISE,
                    arcSize: if sweep_deg > 180.0 {
                        D2D1_ARC_SIZE_LARGE
                    } else {
                        D2D1_ARC_SIZE_SMALL
                    },
                });
                sink.EndFigure(D2D1_FIGURE_END_OPEN);
                if sink.Close().is_ok() {
                    rt.DrawGeometry(&geo, br, stroke_w, None);
                    return;
                }
            }
        }
    }
    // Segmented polyline fallback when factory is absent
    let steps = ((sweep_deg / 18.0).ceil() as usize).clamp(4, 24);
    let mut prev = {
        let a0 = (start_deg - 90.0).to_radians();
        Vector2 { X: cx + r * a0.cos(), Y: cy + r * a0.sin() }
    };
    for s in 1..=steps {
        let a = (start_deg + sweep_deg * (s as f32 / steps as f32) - 90.0).to_radians();
        let cur = Vector2 { X: cx + r * a.cos(), Y: cy + r * a.sin() };
        rt.DrawLine(prev, cur, br, stroke_w, None);
        prev = cur;
    }
}

unsafe fn draw_charge_bolt(
    rt: &ID2D1DCRenderTarget,
    cx: f32,
    cy: f32,
    scale: f32,
    br: &ID2D1SolidColorBrush,
) {
    let p0 = Vector2 { X: cx + 0.9 * scale, Y: cy - 3.6 * scale };
    let p1 = Vector2 { X: cx - 1.5 * scale, Y: cy + 0.2 * scale };
    let p2 = Vector2 { X: cx + 1.3 * scale, Y: cy + 0.2 * scale };
    let p3 = Vector2 { X: cx - 0.9 * scale, Y: cy + 3.6 * scale };
    let sw = (1.15 * scale).max(1.0);
    rt.DrawLine(p0, p1, br, sw, None);
    rt.DrawLine(p1, p2, br, sw, None);
    rt.DrawLine(p2, p3, br, sw, None);
}

#[allow(clippy::too_many_arguments)]
unsafe fn draw_tray_cluster(
    rt: &ID2D1DCRenderTarget,
    d2d: Option<&ID2D1Factory>,
    br: &ID2D1SolidColorBrush,
    tf_mono: Option<&IDWriteTextFormat>,
    rect: D2D_RECT_F,
    vol_pct: u8,
    vol_muted: bool,
    wifi_on: bool,
    wifi_signal: u8,
    bat_pct: Option<u8>,
    bat_charging: bool,
    show_vol_osd: bool,
    hovered: bool,
    fg_col: (u8, u8, u8),
    muted_col: (u8, u8, u8),
    accent_col: (u8, u8, u8),
    cfg: &AppConfig,
    anim: &TrayAnimState,
    anim_phase: f32,
    is_vertical: bool,
    u: f32,
) {
    // Dynamic audio color reflects mute / low / normal / high volume cleanly
    let vol_ratio = anim.vol_display.clamp(0.0, 1.0);
    let audio_col: (u8, u8, u8) = if vol_muted || vol_pct == 0 {
        (238, 86, 86)
    } else if vol_pct <= 30 {
        (110, 212, 245)
    } else if vol_pct >= 80 {
        (255, 186, 72)
    } else {
        accent_col
    };

    // Reactive directional pulse helper for volume up/down/mute across ALL audio styles
    let draw_vol_pulse_ring = |vx: f32, vy: f32| {
        if anim.vol_pulse_t < 0.995 {
            let t = anim.vol_pulse_t.clamp(0.0, 1.0);
            let ease = 1.0 - (1.0 - t).powi(3);
            let fade = (1.0 - t).powf(1.35);
            let (rad, pcol) = if anim.vol_dir > 0 {
                // Volume Up: expanding bright outward pulse ring
                ((4.6 + ease * 7.8) * u, audio_col)
            } else if anim.vol_dir < 0 {
                // Volume Down: contracting inward ripple ring to visually signal reduction
                ((12.4 - ease * 7.6) * u, (124, 218, 255))
            } else {
                // Mute toggle: sharp burst ring
                ((4.2 + ease * 8.2) * u, if vol_muted { (242, 84, 84) } else { accent_col })
            };
            br.SetColor(&rgba(pcol.0, pcol.1, pcol.2, 0.78 * fade));
            rt.DrawEllipse(
                &windows::Win32::Graphics::Direct2D::D2D1_ELLIPSE {
                    point: Vector2 { X: vx, Y: vy },
                    radiusX: rad,
                    radiusY: rad,
                },
                br,
                (1.65 - 0.55 * ease) * u,
                None,
            );
        }
    };

    let draw_wifi = |wx: f32, wy: f32| {
        // State-change / activity ripple for ALL visible network styles
        if anim.net_pulse_t < 0.995 {
            let t = anim.net_pulse_t.clamp(0.0, 1.0);
            let ease = 1.0 - (1.0 - t).powi(3);
            let fade = (1.0 - t).powf(1.4);
            let rcol = if wifi_on { accent_col } else { (236, 86, 86) };
            let r_rad = (4.0 + ease * 8.0) * u;
            br.SetColor(&rgba(rcol.0, rcol.1, rcol.2, 0.72 * fade));
            rt.DrawEllipse(
                &windows::Win32::Graphics::Direct2D::D2D1_ELLIPSE {
                    point: Vector2 { X: wx, Y: wy },
                    radiusX: r_rad,
                    radiusY: r_rad,
                },
                br,
                1.4 * u,
                None,
            );
        }

        match cfg.w_tray.net_style {
            TrayNetMeterStyle::Hidden => {}
            TrayNetMeterStyle::MinimalDot => {
                let col = if wifi_on { accent_col } else { (236, 84, 84) };
                let sig = anim.net_signal_display.clamp(0.0, 1.0);
                let breath = if wifi_on {
                    0.5 + 0.5 * (anim_phase * 1.8 + anim.net_wave_phase).sin()
                } else {
                    0.0
                };
                // Outer signal ring proportional to Wi-Fi signal strength
                let ring_r = 5.8 * u;
                br.SetColor(&rgba(muted_col.0, muted_col.1, muted_col.2, 0.28));
                rt.DrawEllipse(
                    &windows::Win32::Graphics::Direct2D::D2D1_ELLIPSE {
                        point: Vector2 { X: wx, Y: wy },
                        radiusX: ring_r,
                        radiusY: ring_r,
                    },
                    br,
                    1.2 * u,
                    None,
                );
                if wifi_on {
                    br.SetColor(&rgba(col.0, col.1, col.2, 0.85));
                    draw_d2d_arc(rt, d2d, wx, wy, ring_r, 0.0, sig * 360.0, 1.45 * u, br);
                    let halo_r = (3.4 + sig * 2.1 + breath * 0.7) * u;
                    br.SetColor(&rgba(col.0, col.1, col.2, 0.22 + 0.14 * breath));
                    rt.FillEllipse(
                        &windows::Win32::Graphics::Direct2D::D2D1_ELLIPSE {
                            point: Vector2 { X: wx, Y: wy },
                            radiusX: halo_r,
                            radiusY: halo_r,
                        },
                        br,
                    );
                    br.SetColor(&rgba(col.0, col.1, col.2, 0.96));
                    rt.FillEllipse(
                        &windows::Win32::Graphics::Direct2D::D2D1_ELLIPSE {
                            point: Vector2 { X: wx, Y: wy },
                            radiusX: 2.5 * u,
                            radiusY: 2.5 * u,
                        },
                        br,
                    );
                } else {
                    br.SetColor(&rgba(col.0, col.1, col.2, 0.26));
                    rt.FillEllipse(
                        &windows::Win32::Graphics::Direct2D::D2D1_ELLIPSE {
                            point: Vector2 { X: wx, Y: wy },
                            radiusX: 4.6 * u,
                            radiusY: 4.6 * u,
                        },
                        br,
                    );
                    br.SetColor(&rgba(col.0, col.1, col.2, 0.95));
                    rt.FillEllipse(
                        &windows::Win32::Graphics::Direct2D::D2D1_ELLIPSE {
                            point: Vector2 { X: wx, Y: wy },
                            radiusX: 2.3 * u,
                            radiusY: 2.3 * u,
                        },
                        br,
                    );
                    rt.DrawLine(
                        Vector2 { X: wx - 4.4 * u, Y: wy - 4.4 * u },
                        Vector2 { X: wx + 4.4 * u, Y: wy + 4.4 * u },
                        br,
                        1.35 * u,
                        None,
                    );
                }
            }
            TrayNetMeterStyle::LiveThroughputText => {
                if let Some(tfm) = tf_mono {
                    let s = if wifi_on {
                        if wifi_signal > 0 {
                            format!("{}%", wifi_signal)
                        } else {
                            "ON".to_string()
                        }
                    } else {
                        "OFF".to_string()
                    };
                    let col = if wifi_on { accent_col } else { (236, 84, 84) };
                    br.SetColor(&rgba(col.0, col.1, col.2, 0.95));
                    draw_d2d_text(
                        rt,
                        &s,
                        tfm,
                        &D2D_RECT_F {
                            left: wx - 14.0 * u,
                            top: wy - 9.5 * u,
                            right: wx + 14.0 * u,
                            bottom: wy + 6.5 * u,
                        },
                        br,
                    );
                    // Animated micro signal strength bar underneath
                    let bw = 15.0 * u;
                    let by = wy + 6.2 * u;
                    br.SetColor(&rgba(muted_col.0, muted_col.1, muted_col.2, 0.30));
                    rt.FillRoundedRectangle(
                        &D2D1_ROUNDED_RECT {
                            rect: D2D_RECT_F {
                                left: wx - bw * 0.5,
                                top: by,
                                right: wx + bw * 0.5,
                                bottom: by + 2.0 * u,
                            },
                            radiusX: 1.0 * u,
                            radiusY: 1.0 * u,
                        },
                        br,
                    );
                    if wifi_on && anim.net_signal_display > 0.02 {
                        let pulse_a = if anim.net_pulse_t < 1.0 { 1.0 } else { 0.88 };
                        br.SetColor(&rgba(col.0, col.1, col.2, pulse_a));
                        rt.FillRoundedRectangle(
                            &D2D1_ROUNDED_RECT {
                                rect: D2D_RECT_F {
                                    left: wx - bw * 0.5,
                                    top: by,
                                    right: wx - bw * 0.5 + bw * anim.net_signal_display,
                                    bottom: by + 2.0 * u,
                                },
                                radiusX: 1.0 * u,
                                radiusY: 1.0 * u,
                            },
                            br,
                        );
                    }
                }
            }
            TrayNetMeterStyle::SignalBars => {
                let sig = anim.net_signal_display.clamp(0.0, 1.0);
                let wave_boost = if anim.net_pulse_t < 1.0 {
                    (1.0 - anim.net_pulse_t) * 1.8
                } else {
                    0.0
                };
                for i in 0..3 {
                    let thresh = (i as f32) * 0.33;
                    let bar_fill = ((sig - thresh) / 0.33).clamp(0.0, 1.0);
                    let wave_lift = if wifi_on && wave_boost > 0.01 {
                        ((anim.net_wave_phase - i as f32 * 0.9).sin().max(0.0)) * wave_boost * u
                    } else {
                        0.0
                    };
                    let bh = (3.6 + i as f32 * 3.1) * u + wave_lift;
                    let bx = wx - 5.2 * u + i as f32 * 4.1 * u;
                    let col = if wifi_on && bar_fill > 0.15 {
                        if wave_boost > 0.1 { accent_col } else { fg_col }
                    } else {
                        muted_col
                    };
                    let alpha = if wifi_on {
                        0.28 + 0.68 * bar_fill
                    } else {
                        0.38
                    };
                    br.SetColor(&rgba(col.0, col.1, col.2, alpha));
                    rt.FillRoundedRectangle(
                        &D2D1_ROUNDED_RECT {
                            rect: D2D_RECT_F {
                                left: bx,
                                top: wy + 5.2 * u - bh,
                                right: bx + 2.5 * u,
                                bottom: wy + 5.2 * u,
                            },
                            radiusX: 0.9 * u,
                            radiusY: 0.9 * u,
                        },
                        br,
                    );
                }
                if !wifi_on {
                    br.SetColor(&rgba(236, 84, 84, 0.95));
                    rt.DrawLine(
                        Vector2 { X: wx - 5.5 * u, Y: wy - 4.5 * u },
                        Vector2 { X: wx + 5.5 * u, Y: wy + 5.0 * u },
                        br,
                        1.35 * u,
                        None,
                    );
                }
            }
        }
    };

    let draw_speaker = |vx: f32, vy: f32| {
        draw_vol_pulse_ring(vx, vy);
        // Subtle directional pop/squeeze impulse when volume increases or decreases
        let bump = if anim.vol_pulse_t < 0.65 {
            let k = (anim.vol_pulse_t / 0.65 * std::f32::consts::PI).sin();
            if anim.vol_dir > 0 {
                1.0 + 0.14 * k
            } else if anim.vol_dir < 0 {
                1.0 - 0.12 * k
            } else {
                1.0 + 0.08 * k
            }
        } else {
            1.0
        };

        match cfg.w_tray.audio_style {
            TrayAudioMeterStyle::LiveVuEqualizer => {
                let has_live_vu = anim.has_audio_vu_energy() && !vol_muted;
                let pulse_wave = if anim.vol_pulse_t < 1.0 {
                    (1.0 - anim.vol_pulse_t) * 0.45
                } else {
                    0.0
                };
                for b_i in 0..4 {
                    let level = if vol_muted {
                        0.08
                    } else if has_live_vu {
                        (anim.audio_vu[b_i] * (0.35 + 0.65 * vol_ratio) + pulse_wave * 0.25)
                            .clamp(0.10, 1.0)
                    } else {
                        // Clean proportional staircase level meter reacting directly to volume adjustments
                        let bar_pos = (b_i as f32 + 1.0) / 4.0;
                        let lit = ((vol_ratio - (b_i as f32) * 0.24) / 0.28).clamp(0.0, 1.0);
                        let dir_ripple = if anim.vol_pulse_t < 1.0 {
                            let phase = if anim.vol_dir >= 0 {
                                anim.vol_pulse_t * 4.5 - b_i as f32 * 0.65
                            } else {
                                anim.vol_pulse_t * 4.5 - (3 - b_i) as f32 * 0.65
                            };
                            phase.sin().max(0.0) * pulse_wave
                        } else {
                            0.0
                        };
                        (0.12 + bar_pos * 0.78 * lit + dir_ripple).clamp(0.10, 1.0)
                    };
                    let bh = (2.2 + level * 9.8) * u * bump;
                    let bx = vx - 6.0 * u + b_i as f32 * 3.4 * u;
                    let col = if vol_muted {
                        (236, 84, 84)
                    } else if b_i % 2 == 0 {
                        audio_col
                    } else {
                        fg_col
                    };
                    let alpha = if vol_muted { 0.65 } else { 0.45 + 0.52 * level };
                    br.SetColor(&rgba(col.0, col.1, col.2, alpha));
                    rt.FillRoundedRectangle(
                        &D2D1_ROUNDED_RECT {
                            rect: D2D_RECT_F {
                                left: bx,
                                top: vy + 5.2 * u - bh,
                                right: bx + 2.2 * u,
                                bottom: vy + 5.2 * u,
                            },
                            radiusX: 0.9 * u,
                            radiusY: 0.9 * u,
                        },
                        br,
                    );
                }
            }
            TrayAudioMeterStyle::VolumePercentText => {
                if let Some(tfm) = tf_mono {
                    let txt = if vol_muted {
                        "MUTE".to_string()
                    } else {
                        format!("{}%", vol_pct)
                    };
                    let col = if vol_muted || anim.vol_pulse_t < 0.9 {
                        audio_col
                    } else {
                        fg_col
                    };
                    br.SetColor(&rgba(col.0, col.1, col.2, 0.96));
                    draw_d2d_text(
                        rt,
                        &txt,
                        tfm,
                        &D2D_RECT_F {
                            left: vx - 14.0 * u,
                            top: vy - 9.5 * u,
                            right: vx + 14.0 * u,
                            bottom: vy + 6.5 * u,
                        },
                        br,
                    );
                    // Animated proportional volume micro-bar underneath text
                    let bw = 16.0 * u;
                    let by = vy + 6.2 * u;
                    br.SetColor(&rgba(muted_col.0, muted_col.1, muted_col.2, 0.32));
                    rt.FillRoundedRectangle(
                        &D2D1_ROUNDED_RECT {
                            rect: D2D_RECT_F {
                                left: vx - bw * 0.5,
                                top: by,
                                right: vx + bw * 0.5,
                                bottom: by + 2.2 * u,
                            },
                            radiusX: 1.1 * u,
                            radiusY: 1.1 * u,
                        },
                        br,
                    );
                    if !vol_muted && vol_ratio > 0.01 {
                        br.SetColor(&rgba(audio_col.0, audio_col.1, audio_col.2, 0.95));
                        rt.FillRoundedRectangle(
                            &D2D1_ROUNDED_RECT {
                                rect: D2D_RECT_F {
                                    left: vx - bw * 0.5,
                                    top: by,
                                    right: vx - bw * 0.5 + bw * vol_ratio,
                                    bottom: by + 2.2 * u,
                                },
                                radiusX: 1.1 * u,
                                radiusY: 1.1 * u,
                            },
                            br,
                        );
                    }
                }
            }
            TrayAudioMeterStyle::RadialArc => {
                // True circular dial gauge sweeping smoothly with vol_display + glowing tip + center cue
                let r_d = 5.8 * u * bump;
                br.SetColor(&rgba(muted_col.0, muted_col.1, muted_col.2, 0.32));
                rt.DrawEllipse(
                    &windows::Win32::Graphics::Direct2D::D2D1_ELLIPSE {
                        point: Vector2 { X: vx, Y: vy },
                        radiusX: r_d,
                        radiusY: r_d,
                    },
                    br,
                    1.55 * u,
                    None,
                );
                if !vol_muted && vol_ratio > 0.01 {
                    let sweep = (vol_ratio * 360.0).clamp(4.0, 360.0);
                    br.SetColor(&rgba(audio_col.0, audio_col.1, audio_col.2, 0.96));
                    draw_d2d_arc(rt, d2d, vx, vy, r_d, 0.0, sweep, 2.0 * u, br);
                    // Leading-edge glowing tip dot on the arc
                    let tip_rad = (sweep - 90.0).to_radians();
                    let tx = vx + r_d * tip_rad.cos();
                    let ty = vy + r_d * tip_rad.sin();
                    br.SetColor(&rgba(fg_col.0, fg_col.1, fg_col.2, 0.96));
                    rt.FillEllipse(
                        &windows::Win32::Graphics::Direct2D::D2D1_ELLIPSE {
                            point: Vector2 { X: tx, Y: ty },
                            radiusX: 1.45 * u,
                            radiusY: 1.45 * u,
                        },
                        br,
                    );
                    // Soft inner energy glow scaling with level
                    br.SetColor(&rgba(audio_col.0, audio_col.1, audio_col.2, 0.25 + 0.35 * vol_ratio));
                    rt.FillEllipse(
                        &windows::Win32::Graphics::Direct2D::D2D1_ELLIPSE {
                            point: Vector2 { X: vx, Y: vy },
                            radiusX: (1.5 + 1.5 * vol_ratio) * u,
                            radiusY: (1.5 + 1.5 * vol_ratio) * u,
                        },
                        br,
                    );
                } else {
                    // Muted: crisp coral center X inside the dial
                    br.SetColor(&rgba(238, 86, 86, 0.96));
                    rt.DrawLine(
                        Vector2 { X: vx - 2.6 * u, Y: vy - 2.6 * u },
                        Vector2 { X: vx + 2.6 * u, Y: vy + 2.6 * u },
                        br,
                        1.45 * u,
                        None,
                    );
                    rt.DrawLine(
                        Vector2 { X: vx + 2.6 * u, Y: vy - 2.6 * u },
                        Vector2 { X: vx - 2.6 * u, Y: vy + 2.6 * u },
                        br,
                        1.45 * u,
                        None,
                    );
                }
            }
            TrayAudioMeterStyle::SpeakerGlyph => {
                let s = bump;
                br.SetColor(&rgba(fg_col.0, fg_col.1, fg_col.2, if vol_muted { 0.52 } else { 0.95 }));
                rt.FillRectangle(
                    &D2D_RECT_F {
                        left: vx - 6.2 * u * s,
                        top: vy - 2.5 * u * s,
                        right: vx - 2.8 * u * s,
                        bottom: vy + 2.5 * u * s,
                    },
                    br,
                );
                rt.FillRectangle(
                    &D2D_RECT_F {
                        left: vx - 3.0 * u * s,
                        top: vy - 4.6 * u * s,
                        right: vx - 0.8 * u * s,
                        bottom: vy + 4.6 * u * s,
                    },
                    br,
                );
                if vol_muted || vol_ratio < 0.01 {
                    br.SetColor(&rgba(238, 86, 86, 0.96));
                    let xoff = if anim.vol_pulse_t < 0.5 {
                        (anim.vol_pulse_t * 28.0).sin() * 1.2 * u
                    } else {
                        0.0
                    };
                    rt.DrawLine(
                        Vector2 { X: vx + 1.4 * u + xoff, Y: vy - 2.8 * u },
                        Vector2 { X: vx + 6.2 * u + xoff, Y: vy + 2.8 * u },
                        br,
                        1.45 * u,
                        None,
                    );
                    rt.DrawLine(
                        Vector2 { X: vx + 6.2 * u + xoff, Y: vy - 2.8 * u },
                        Vector2 { X: vx + 1.4 * u + xoff, Y: vy + 2.8 * u },
                        br,
                        1.45 * u,
                        None,
                    );
                } else {
                    // 3 progressive curved acoustic arcs reacting smoothly to volume up/down!
                    let wave_shift = if anim.vol_pulse_t < 1.0 {
                        let k = (1.0 - anim.vol_pulse_t) * 1.4 * u;
                        if anim.vol_dir > 0 { k } else if anim.vol_dir < 0 { -k * 0.7 } else { 0.0 }
                    } else {
                        0.0
                    };
                    for (wi, (thresh, base_r)) in [(0.02f32, 3.2f32), (0.34, 5.4), (0.68, 7.6)]
                        .into_iter()
                        .enumerate()
                    {
                        let seg_alpha = ((vol_ratio - thresh) / 0.24).clamp(0.0, 1.0);
                        if seg_alpha > 0.05 {
                            let rad = (base_r * u + wave_shift * (0.45 + wi as f32 * 0.25)).max(1.8 * u);
                            br.SetColor(&rgba(audio_col.0, audio_col.1, audio_col.2, 0.25 + 0.72 * seg_alpha));
                            draw_d2d_arc(
                                rt,
                                d2d,
                                vx - 1.2 * u,
                                vy,
                                rad,
                                42.0,
                                96.0,
                                1.35 * u,
                                br,
                            );
                        }
                    }
                }
            }
        }
    };

    let draw_battery = |bx: f32, by: f32, pct: u8| {
        let bat_ratio = anim.bat_display.clamp(0.0, 1.0);
        let bat_low = pct <= 20 && !bat_charging;
        let charge_pulse = if bat_charging {
            0.5 + 0.5 * anim.bat_charge_phase.sin()
        } else {
            0.0
        };
        let low_pulse = if bat_low {
            0.5 + 0.5 * (anim.bat_charge_phase * 1.6).sin()
        } else {
            0.0
        };
        let bat_col: (u8, u8, u8) = if bat_charging {
            (74, 228, 134)
        } else if bat_low {
            (246, 94, 72)
        } else {
            accent_col
        };

        // Plug/unplug or battery step transition ripple
        if anim.bat_pulse_t < 0.995 {
            let t = anim.bat_pulse_t.clamp(0.0, 1.0);
            let ease = 1.0 - (1.0 - t).powi(3);
            let fade = (1.0 - t).powf(1.4);
            let r_rad = (4.2 + ease * 8.0) * u;
            br.SetColor(&rgba(bat_col.0, bat_col.1, bat_col.2, 0.75 * fade));
            rt.DrawEllipse(
                &windows::Win32::Graphics::Direct2D::D2D1_ELLIPSE {
                    point: Vector2 { X: bx, Y: by },
                    radiusX: r_rad,
                    radiusY: r_rad,
                },
                br,
                1.45 * u,
                None,
            );
        }

        match cfg.w_tray.battery_style {
            TrayBatteryMeterStyle::Hidden => {}
            TrayBatteryMeterStyle::RadialRingGauge => {
                // True proportional circular arc gauge + charging bolt & shimmer + low-battery pulse
                let r_b = 5.8 * u;
                if bat_charging || bat_low {
                    let halo_a = if bat_charging {
                        0.12 + 0.18 * charge_pulse
                    } else {
                        0.14 + 0.24 * low_pulse
                    };
                    br.SetColor(&rgba(bat_col.0, bat_col.1, bat_col.2, halo_a));
                    rt.FillEllipse(
                        &windows::Win32::Graphics::Direct2D::D2D1_ELLIPSE {
                            point: Vector2 { X: bx, Y: by },
                            radiusX: (r_b + 1.6 * u),
                            radiusY: (r_b + 1.6 * u),
                        },
                        br,
                    );
                }
                br.SetColor(&rgba(muted_col.0, muted_col.1, muted_col.2, 0.32));
                rt.DrawEllipse(
                    &windows::Win32::Graphics::Direct2D::D2D1_ELLIPSE {
                        point: Vector2 { X: bx, Y: by },
                        radiusX: r_b,
                        radiusY: r_b,
                    },
                    br,
                    1.55 * u,
                    None,
                );
                let sweep = (bat_ratio * 360.0).clamp(6.0, 360.0);
                br.SetColor(&rgba(bat_col.0, bat_col.1, bat_col.2, if bat_low { 0.65 + 0.35 * low_pulse } else { 0.95 }));
                draw_d2d_arc(rt, d2d, bx, by, r_b, 0.0, sweep, 2.0 * u, br);
                if bat_charging {
                    // Traveling shimmer highlight along the charging arc + crisp lightning bolt in center
                    let shim_deg = (anim.bat_charge_phase.to_degrees() * 1.4) % sweep.max(10.0);
                    br.SetColor(&rgba(216, 255, 232, 0.85 * (0.4 + 0.6 * charge_pulse)));
                    draw_d2d_arc(rt, d2d, bx, by, r_b, shim_deg, 28.0f32.min(sweep), 2.1 * u, br);
                    br.SetColor(&rgba(244, 255, 248, 0.95));
                    draw_charge_bolt(rt, bx, by, 0.95 * u, br);
                } else {
                    let dot_r = (1.6 + 1.2 * bat_ratio) * u;
                    br.SetColor(&rgba(bat_col.0, bat_col.1, bat_col.2, 0.88));
                    rt.FillEllipse(
                        &windows::Win32::Graphics::Direct2D::D2D1_ELLIPSE {
                            point: Vector2 { X: bx, Y: by },
                            radiusX: dot_r,
                            radiusY: dot_r,
                        },
                        br,
                    );
                }
            }
            TrayBatteryMeterStyle::PercentOnly => {
                if let Some(tfm) = tf_mono {
                    let s = if bat_charging {
                        format!("{}⚡", pct)
                    } else {
                        format!("{}%", pct)
                    };
                    let fill_col = if bat_charging {
                        rgba(74, 228, 134, 0.82 + 0.18 * charge_pulse)
                    } else if bat_low {
                        rgba(246, 94, 72, 0.68 + 0.32 * low_pulse)
                    } else {
                        rgba(fg_col.0, fg_col.1, fg_col.2, 0.94)
                    };
                    br.SetColor(&fill_col);
                    draw_d2d_text(
                        rt,
                        &s,
                        tfm,
                        &D2D_RECT_F {
                            left: bx - 15.0 * u,
                            top: by - 9.5 * u,
                            right: bx + 15.0 * u,
                            bottom: by + 6.5 * u,
                        },
                        br,
                    );
                    // Animated proportional battery micro-bar underneath
                    let bw = 16.0 * u;
                    let bar_y = by + 6.2 * u;
                    br.SetColor(&rgba(muted_col.0, muted_col.1, muted_col.2, 0.30));
                    rt.FillRoundedRectangle(
                        &D2D1_ROUNDED_RECT {
                            rect: D2D_RECT_F {
                                left: bx - bw * 0.5,
                                top: bar_y,
                                right: bx + bw * 0.5,
                                bottom: bar_y + 2.1 * u,
                            },
                            radiusX: 1.0 * u,
                            radiusY: 1.0 * u,
                        },
                        br,
                    );
                    let fw = (bw * bat_ratio).max(2.0 * u);
                    br.SetColor(&rgba(bat_col.0, bat_col.1, bat_col.2, 0.94));
                    rt.FillRoundedRectangle(
                        &D2D1_ROUNDED_RECT {
                            rect: D2D_RECT_F {
                                left: bx - bw * 0.5,
                                top: bar_y,
                                right: bx - bw * 0.5 + fw,
                                bottom: bar_y + 2.1 * u,
                            },
                            radiusX: 1.0 * u,
                            radiusY: 1.0 * u,
                        },
                        br,
                    );
                }
            }
            TrayBatteryMeterStyle::HorizontalCell => {
                let bw = 15.6 * u;
                let bh = 9.0 * u;
                let bx_body = bx - 1.2 * u;
                let b_rect = D2D1_ROUNDED_RECT {
                    rect: D2D_RECT_F {
                        left: bx_body - bw * 0.5,
                        top: by - bh * 0.5,
                        right: bx_body + bw * 0.5,
                        bottom: by + bh * 0.5,
                    },
                    radiusX: 2.0 * u,
                    radiusY: 2.0 * u,
                };
                br.SetColor(&rgba(
                    if bat_low { 246 } else { fg_col.0 },
                    if bat_low { 94 } else { fg_col.1 },
                    if bat_low { 72 } else { fg_col.2 },
                    if bat_low { 0.65 + 0.35 * low_pulse } else { 0.88 },
                ));
                rt.DrawRoundedRectangle(&b_rect, br, 1.15 * u, None);
                rt.FillRectangle(
                    &D2D_RECT_F {
                        left: b_rect.rect.right + 0.8 * u,
                        top: by - 2.0 * u,
                        right: b_rect.rect.right + 2.4 * u,
                        bottom: by + 2.0 * u,
                    },
                    br,
                );
                let pad = 1.9 * u;
                let inner_w = bw - pad * 2.0;
                let fw = (inner_w * bat_ratio).max(2.0 * u);
                let fill_alpha = if bat_low {
                    0.62 + 0.36 * low_pulse
                } else {
                    0.92
                };
                br.SetColor(&rgba(bat_col.0, bat_col.1, bat_col.2, fill_alpha));
                rt.FillRoundedRectangle(
                    &D2D1_ROUNDED_RECT {
                        rect: D2D_RECT_F {
                            left: b_rect.rect.left + pad,
                            top: b_rect.rect.top + pad,
                            right: b_rect.rect.left + pad + fw,
                            bottom: b_rect.rect.bottom - pad,
                        },
                        radiusX: 0.9 * u,
                        radiusY: 0.9 * u,
                    },
                    br,
                );
                if bat_charging {
                    // Animated shimmer highlight gliding across the green charge bar + lightning bolt
                    let sweep_t = (anim.bat_charge_phase * 0.45).fract();
                    let shim_w = (fw * 0.35).clamp(2.2 * u, 4.8 * u);
                    let shim_x = b_rect.rect.left + pad + (fw - shim_w).max(0.0) * sweep_t;
                    br.SetColor(&rgba(232, 255, 240, 0.55 * (0.4 + 0.6 * charge_pulse)));
                    rt.FillRoundedRectangle(
                        &D2D1_ROUNDED_RECT {
                            rect: D2D_RECT_F {
                                left: shim_x,
                                top: b_rect.rect.top + pad,
                                right: (shim_x + shim_w).min(b_rect.rect.left + pad + fw),
                                bottom: b_rect.rect.bottom - pad,
                            },
                            radiusX: 0.8 * u,
                            radiusY: 0.8 * u,
                        },
                        br,
                    );
                    br.SetColor(&rgba(255, 255, 255, 0.95));
                    draw_charge_bolt(rt, bx_body, by, 0.85 * u, br);
                }
            }
        }
    };

    let show_net = cfg.w_tray.eff_show_net();
    let show_audio = cfg.w_tray.eff_show_audio();
    let visible_bat = bat_pct.filter(|_| cfg.w_tray.eff_show_battery());
    if !show_net && !show_audio && visible_bat.is_none() {
        return;
    }
    if show_audio {
        let pts = tray_cluster_item_centers(rect, is_vertical, show_net, visible_bat.is_some());
        if let Some((wx, wy)) = pts.wifi {
            draw_wifi(wx, wy);
        }
        draw_speaker(pts.speaker.0, pts.speaker.1);
        if let (Some((bx, by)), Some(bpct)) = (pts.battery, visible_bat) {
            draw_battery(bx, by, bpct);
        }
    } else {
        let w = (rect.right - rect.left).max(16.0);
        let h = (rect.bottom - rect.top).max(16.0);
        let cx = (rect.left + rect.right) * 0.5;
        let cy = (rect.top + rect.bottom) * 0.5;
        match (show_net, visible_bat) {
            (true, Some(bpct)) => {
                if is_vertical {
                    draw_wifi(cx, rect.top + h * 0.30);
                    draw_battery(cx, rect.top + h * 0.70, bpct);
                } else {
                    draw_wifi(rect.left + w * 0.30, cy);
                    draw_battery(rect.left + w * 0.72, cy, bpct);
                }
            }
            (true, None) => draw_wifi(cx, cy),
            (false, Some(bpct)) => draw_battery(cx, cy, bpct),
            (false, None) => {}
        }
    }

    // Non-destructive live volume badge + animated bar overlay during volume adjustment
    // (preserves all animated tray indicators underneath while clearly showing exact % / MUTE)
    if show_audio && show_vol_osd && (anim.vol_badge_ttl > 0.01 || hovered) {
        let pts = tray_cluster_item_centers(rect, is_vertical, show_net, visible_bat.is_some());
        let fade = if anim.vol_badge_ttl > 0.01 {
            (anim.vol_badge_ttl / 0.28).clamp(0.0, 1.0)
        } else {
            0.92
        };
        if let Some(tfm) = tf_mono {
            let txt = if vol_muted {
                "MUTE".to_string()
            } else if is_vertical {
                format!("{}%", vol_pct)
            } else {
                format!("VOL {}%", vol_pct)
            };
            let pill_h = if is_vertical { 17.0 * u } else { 15.0 * u };
            let pill_rc = if is_vertical {
                let py = (pts.speaker.1 + 7.5 * u).min(rect.bottom - pill_h - 1.0 * u);
                D2D_RECT_F {
                    left: rect.left + 2.0 * u,
                    top: py,
                    right: rect.right - 2.0 * u,
                    bottom: py + pill_h,
                }
            } else {
                D2D_RECT_F {
                    left: rect.left + 2.5 * u,
                    top: rect.bottom - pill_h - 1.0 * u,
                    right: rect.right - 2.5 * u,
                    bottom: rect.bottom - 1.0 * u,
                }
            };
            br.SetColor(&rgba(12, 16, 22, 0.86 * fade));
            rt.FillRoundedRectangle(
                &D2D1_ROUNDED_RECT { rect: pill_rc, radiusX: 4.5 * u, radiusY: 4.5 * u },
                br,
            );
            br.SetColor(&rgba(audio_col.0, audio_col.1, audio_col.2, 0.55 * fade));
            rt.DrawRoundedRectangle(
                &D2D1_ROUNDED_RECT { rect: pill_rc, radiusX: 4.5 * u, radiusY: 4.5 * u },
                br,
                0.9 * u,
                None,
            );
            br.SetColor(&rgba(248, 252, 255, 0.96 * fade));
            draw_d2d_text(
                rt,
                &txt,
                tfm,
                &D2D_RECT_F {
                    left: pill_rc.left + 1.5 * u,
                    top: pill_rc.top,
                    right: pill_rc.right - 1.5 * u,
                    bottom: pill_rc.bottom - 3.0 * u,
                },
                br,
            );
            let bar_bg = D2D_RECT_F {
                left: pill_rc.left + 3.5 * u,
                top: pill_rc.bottom - 3.6 * u,
                right: pill_rc.right - 3.5 * u,
                bottom: pill_rc.bottom - 1.4 * u,
            };
            br.SetColor(&rgba(muted_col.0, muted_col.1, muted_col.2, 0.35 * fade));
            rt.FillRoundedRectangle(
                &D2D1_ROUNDED_RECT { rect: bar_bg, radiusX: 1.1 * u, radiusY: 1.1 * u },
                br,
            );
            if !vol_muted && vol_ratio > 0.01 {
                let bar_fg = D2D_RECT_F {
                    left: bar_bg.left,
                    top: bar_bg.top,
                    right: bar_bg.left + (bar_bg.right - bar_bg.left) * vol_ratio,
                    bottom: bar_bg.bottom,
                };
                br.SetColor(&rgba(audio_col.0, audio_col.1, audio_col.2, 0.96 * fade));
                rt.FillRoundedRectangle(
                    &D2D1_ROUNDED_RECT { rect: bar_fg, radiusX: 1.1 * u, radiusY: 1.1 * u },
                    br,
                );
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct TrayClusterPoints {
    wifi: Option<(f32, f32)>,
    speaker: (f32, f32),
    battery: Option<(f32, f32)>,
}

fn tray_cluster_slot_span_cfg(
    is_vertical: bool,
    base_sz: f32,
    has_battery: bool,
    cfg: &AppConfig,
    u: f32,
) -> f32 {
    let show_net = cfg.w_tray.eff_show_net();
    let show_audio = cfg.w_tray.eff_show_audio();
    let show_bat = has_battery && cfg.w_tray.eff_show_battery();
    let count = usize::from(show_audio) + usize::from(show_net) + usize::from(show_bat);
    if count == 0 {
        return 0.0;
    }
    if is_vertical {
        match count {
            3 => (base_sz * 1.48).max(66.0 * u),
            2 => base_sz.max(44.0 * u),
            _ => base_sz,
        }
    } else {
        match count {
            3 => 82.0 * u,
            2 => 58.0 * u,
            _ => base_sz.min(44.0 * u).max(34.0 * u),
        }
    }
}

#[allow(dead_code)]
fn tray_cluster_slot_span(
    is_vertical: bool,
    base_sz: f32,
    has_battery: bool,
    net_style: TrayNetMeterStyle,
    bat_style: TrayBatteryMeterStyle,
    u: f32,
) -> f32 {
    let show_net = net_style != TrayNetMeterStyle::Hidden;
    let show_bat = has_battery && bat_style != TrayBatteryMeterStyle::Hidden;
    let count = 1 + usize::from(show_net) + usize::from(show_bat);
    if is_vertical {
        match count {
            3 => (base_sz * 1.48).max(66.0 * u),
            2 => base_sz.max(44.0 * u),
            _ => base_sz,
        }
    } else {
        match count {
            3 => 82.0 * u,
            2 => 58.0 * u,
            _ => base_sz.min(44.0 * u).max(34.0 * u),
        }
    }
}

fn tray_cluster_item_centers(
    rect: D2D_RECT_F,
    is_vertical: bool,
    show_net: bool,
    show_bat: bool,
) -> TrayClusterPoints {
    let w = (rect.right - rect.left).max(16.0);
    let h = (rect.bottom - rect.top).max(16.0);
    let cx = (rect.left + rect.right) * 0.5;
    let cy = (rect.top + rect.bottom) * 0.5;
    if is_vertical {
        match (show_net, show_bat) {
            (true, true) => TrayClusterPoints {
                wifi: Some((cx, rect.top + h * 0.21)),
                speaker: (cx, rect.top + h * 0.50),
                battery: Some((cx, rect.top + h * 0.79)),
            },
            (true, false) => TrayClusterPoints {
                wifi: Some((cx, rect.top + h * 0.30)),
                speaker: (cx, rect.top + h * 0.70),
                battery: None,
            },
            (false, true) => TrayClusterPoints {
                wifi: None,
                speaker: (cx, rect.top + h * 0.30),
                battery: Some((cx, rect.top + h * 0.70)),
            },
            (false, false) => TrayClusterPoints {
                wifi: None,
                speaker: (cx, cy),
                battery: None,
            },
        }
    } else {
        match (show_net, show_bat) {
            (true, true) => TrayClusterPoints {
                wifi: Some((rect.left + w * 0.20, cy)),
                speaker: (rect.left + w * 0.49, cy),
                battery: Some((rect.left + w * 0.79, cy)),
            },
            (true, false) => TrayClusterPoints {
                wifi: Some((rect.left + w * 0.30, cy)),
                speaker: (rect.left + w * 0.72, cy),
                battery: None,
            },
            (false, true) => TrayClusterPoints {
                wifi: None,
                speaker: (rect.left + w * 0.30, cy),
                battery: Some((rect.left + w * 0.72, cy)),
            },
            (false, false) => TrayClusterPoints {
                wifi: None,
                speaker: (cx, cy),
                battery: None,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dock_monitor_pane_hit_and_band_on_secondary_display() {
        let sec_mon = RECT { left: -1920, top: 0, right: 0, bottom: 1080 };
        let mut pane = DockMonitorPane::new(sec_mon, false);
        pane.shelf_rect = RECT { left: -1200, top: 1012, right: -720, bottom: 1072 };

        assert!(pane.is_point_in_dock_band(
            POINT { x: -960, y: 1050 },
            MacDockEdge::Bottom,
            DockLayoutKind::FishEyeShelf,
            0.0,
            1.0
        ));
        assert!(!pane.is_point_in_dock_band(
            POINT { x: 400, y: 1050 },
            MacDockEdge::Bottom,
            DockLayoutKind::FishEyeShelf,
            0.0,
            1.0
        ));
    }

    #[test]
    fn tray_overflow_position_clamps_for_all_four_edges_and_secondary_monitor() {
        let mon = RECT { left: 0, top: 0, right: 1920, bottom: 1080 };
        let ov_w = 220;
        let ov_h = 140;

        // Bottom edge dock near right corner -> opens above anchor & clamps inside right edge
        let (bx, by) = compute_tray_overflow_pos(
            RECT { left: 1850, top: 1020, right: 1880, bottom: 1070 },
            MacDockEdge::Bottom,
            mon,
            ov_w,
            ov_h,
        );
        assert!(bx + ov_w <= mon.right - 8 && bx >= mon.left + 8);
        assert!(by + ov_h < 1020);

        // Top edge dock -> opens below anchor
        let (tx, ty) = compute_tray_overflow_pos(
            RECT { left: 900, top: 8, right: 930, bottom: 56 },
            MacDockEdge::Top,
            mon,
            ov_w,
            ov_h,
        );
        assert!(tx >= mon.left + 8 && tx + ov_w <= mon.right - 8);
        assert!(ty > 56);

        // Left edge dock -> opens to the right of anchor
        let (lx, ly) = compute_tray_overflow_pos(
            RECT { left: 6, top: 500, right: 56, bottom: 530 },
            MacDockEdge::Left,
            mon,
            ov_w,
            ov_h,
        );
        assert!(lx > 56);
        assert!(ly >= mon.top + 8 && ly + ov_h <= mon.bottom - 8);

        // Right edge dock on negative-X secondary monitor -> opens to the left of anchor & stays on secondary monitor
        let sec = RECT { left: -1920, top: 0, right: 0, bottom: 1080 };
        let (rx, ry) = compute_tray_overflow_pos(
            RECT { left: -56, top: 980, right: -6, bottom: 1010 },
            MacDockEdge::Right,
            sec,
            ov_w,
            ov_h,
        );
        assert!(rx + ov_w < -56 && rx >= sec.left + 8);
        assert!(ry + ov_h <= sec.bottom - 8 && ry >= sec.top + 8);
    }

    #[test]
    fn dock_cross_metrics_independent_orbs_has_indicator_clearance() {
        let mut cfg = AppConfig::default();
        cfg.tb_theme = TaskbarTheme::InvisibleGhostDock;
        cfg.mac_dock_icon_size = 34;
        let (spec, _vert, base_sz, shelf_cross, edge_gap, _wing) = dock_cross_metrics(&cfg, 1.0);
        assert_eq!(spec.layout, DockLayoutKind::IndependentOrbs);
        assert!(edge_gap >= 10.0);
        assert!(shelf_cross >= base_sz + 20.0);
    }

    #[test]
    fn thumb_bridge_corridor_never_swallows_adjacent_dock_icons() {
        let anchor_a = RECT { left: 900, top: 1020, right: 944, bottom: 1064 };
        let popup_a = RECT { left: 720, top: 860, right: 1124, bottom: 1004 };
        let br = compute_thumb_bridge_rect(anchor_a, popup_a, MacDockEdge::Bottom);
        // Point in the open air gap between App A and its popup is inside the bridge
        let gap_pt = POINT { x: 922, y: 1012 };
        assert!(
            gap_pt.x >= br.left && gap_pt.x <= br.right && gap_pt.y >= br.top && gap_pt.y <= br.bottom,
            "Air-gap point between hovered icon and popup must stay in bridge corridor"
        );
        // Center of adjacent App B icon right next to App A must NOT be swallowed by bridge_rect
        let adj_app_b = POINT { x: 970, y: 1042 };
        assert!(
            !(adj_app_b.x >= br.left && adj_app_b.x <= br.right && adj_app_b.y >= br.top && adj_app_b.y <= br.bottom),
            "Adjacent dock icon center must not be locked inside previous app's bridge_rect"
        );

        // Verify Top / Left / Right edges similarly exclude adjacent icon centers
        let top_a = RECT { left: 900, top: 12, right: 944, bottom: 56 };
        let top_p = RECT { left: 720, top: 70, right: 1124, bottom: 214 };
        let br_top = compute_thumb_bridge_rect(top_a, top_p, MacDockEdge::Top);
        assert!(br_top.top >= top_a.bottom - 4 && br_top.bottom >= top_p.top);
        assert!(34 < br_top.top, "Adjacent top-dock icon center must remain above bridge");

        let left_a = RECT { left: 12, top: 500, right: 56, bottom: 544 };
        let left_p = RECT { left: 70, top: 380, right: 270, bottom: 660 };
        let br_left = compute_thumb_bridge_rect(left_a, left_p, MacDockEdge::Left);
        assert!(br_left.left >= left_a.right - 4 && br_left.right >= left_p.left);
        assert!(34 < br_left.left, "Adjacent left-dock icon center must remain left of bridge");

        let right_a = RECT { left: 1864, top: 500, right: 1908, bottom: 544 };
        let right_p = RECT { left: 1650, top: 380, right: 1850, bottom: 660 };
        let br_right = compute_thumb_bridge_rect(right_a, right_p, MacDockEdge::Right);
        assert!(br_right.right <= right_a.left + 4 && br_right.left <= right_p.right);
        assert!(1886 > br_right.right, "Adjacent right-dock icon center must remain right of bridge");
    }

    #[test]
    fn thumb_hover_switches_immediately_app_to_app_even_in_bridge_rim() {
        // Cursor directly inside popup -> keep stable
        assert_eq!(
            decide_thumb_hover_action(true, false, false, false, false),
            ThumbHoverAction::KeepStable
        );
        // Cursor moves from App A onto adjacent App B inside dock (even if inside bridge rim!)
        assert_eq!(
            decide_thumb_hover_action(false, true, true, true, true),
            ThumbHoverAction::SwitchOrOpen
        );
        // Cursor moves from App A onto adjacent App B outside bridge
        assert_eq!(
            decide_thumb_hover_action(false, true, false, true, true),
            ThumbHoverAction::SwitchOrOpen
        );
        // Cursor moves from App A onto a non-running slot (Start/Search/pinned closed app)
        assert_eq!(
            decide_thumb_hover_action(false, true, true, false, true),
            ThumbHoverAction::HideNow
        );
        // Cursor crosses empty dock rim inside bridge heading straight up to popup -> keep stable
        assert_eq!(
            decide_thumb_hover_action(false, true, true, false, false),
            ThumbHoverAction::KeepStable
        );
        // Cursor in open air gap strictly between dock and popup -> keep stable
        assert_eq!(
            decide_thumb_hover_action(false, false, true, false, false),
            ThumbHoverAction::KeepStable
        );
        // Cursor moved completely off dock & popup -> expire after grace period
        assert_eq!(
            decide_thumb_hover_action(false, false, false, false, false),
            ThumbHoverAction::ExpireGrace
        );
    }

    #[test]
    fn vertical_tray_cluster_shows_battery_with_balanced_three_item_layout() {
        let span3_v = tray_cluster_slot_span(
            true,
            44.0,
            true,
            TrayNetMeterStyle::SignalBars,
            TrayBatteryMeterStyle::HorizontalCell,
            1.0,
        );
        let span2_v = tray_cluster_slot_span(
            true,
            44.0,
            false,
            TrayNetMeterStyle::SignalBars,
            TrayBatteryMeterStyle::HorizontalCell,
            1.0,
        );
        let span_hidden_bat_v = tray_cluster_slot_span(
            true,
            44.0,
            true,
            TrayNetMeterStyle::SignalBars,
            TrayBatteryMeterStyle::Hidden,
            1.0,
        );
        assert!(span3_v >= 66.0, "Vertical tray with battery must allocate >=66px height, got {span3_v}");
        assert!(span3_v > span2_v, "3-item vertical tray span must exceed 2-item vertical span");
        assert!((span2_v - span_hidden_bat_v).abs() < 0.01);

        let v_rect = D2D_RECT_F {
            left: 8.0,
            top: 400.0,
            right: 52.0,
            bottom: 400.0 + span3_v,
        };
        let v_pts = tray_cluster_item_centers(v_rect, true, true, true);
        let wifi = v_pts.wifi.expect("Vertical 3-item tray must include Wi-Fi center");
        let bat = v_pts.battery.expect("Vertical 3-item tray must include Battery center");
        assert!((wifi.0 - 30.0).abs() < 0.01 && (v_pts.speaker.0 - 30.0).abs() < 0.01 && (bat.0 - 30.0).abs() < 0.01);
        assert!(wifi.1 < v_pts.speaker.1 && v_pts.speaker.1 < bat.1);
        assert!(
            (v_pts.speaker.1 - wifi.1) >= 16.0 && (bat.1 - v_pts.speaker.1) >= 16.0,
            "Vertical tray icons must have >=16px center-to-center spacing"
        );
        assert!(wifi.1 - v_rect.top >= 10.0 && v_rect.bottom - bat.1 >= 10.0);

        // Hidden network still keeps battery visible in vertical dock
        let v_pts_no_net = tray_cluster_item_centers(v_rect, true, false, true);
        assert!(v_pts_no_net.wifi.is_none());
        let bat2 = v_pts_no_net.battery.expect("Battery must remain visible when net meter is hidden");
        assert!(v_pts_no_net.speaker.1 < bat2.1);

        // Horizontal mode preserves 3-item (82px) & 2-item (58px) layout
        let span3_h = tray_cluster_slot_span(
            false,
            44.0,
            true,
            TrayNetMeterStyle::SignalBars,
            TrayBatteryMeterStyle::HorizontalCell,
            1.0,
        );
        let span2_h = tray_cluster_slot_span(
            false,
            44.0,
            false,
            TrayNetMeterStyle::SignalBars,
            TrayBatteryMeterStyle::HorizontalCell,
            1.0,
        );
        assert!((span3_h - 82.0).abs() < 0.01);
        assert!((span2_h - 58.0).abs() < 0.01);
        let h_rect = D2D_RECT_F { left: 100.0, top: 10.0, right: 100.0 + span3_h, bottom: 54.0 };
        let h_pts = tray_cluster_item_centers(h_rect, false, true, true);
        let hw = h_pts.wifi.unwrap();
        let hb = h_pts.battery.unwrap();
        assert!(hw.0 < h_pts.speaker.0 && h_pts.speaker.0 < hb.0);
    }

    #[test]
    fn dock_timer_cadence_drops_to_idle_and_obeys_anim_fps() {
        // Active spring/slide/bounce/ambient motion obeys max(8, 1000 / anim_fps)
        assert_eq!(compute_dock_timer_ms(true, false, false, false, 45), 22);
        assert_eq!(compute_dock_timer_ms(true, true, true, true, 24), 41);
        assert_eq!(compute_dock_timer_ms(false, true, false, false, 30), 33);
        assert_eq!(compute_dock_timer_ms(false, false, false, true, 60), 16);
        assert_eq!(compute_dock_timer_ms(false, false, false, true, 90), 11);
        assert_eq!(compute_dock_timer_ms(false, false, false, true, 120), 8);
        // Thumbnail popup polls calmly at 2x frame interval clamped 40..=80ms
        assert_eq!(compute_dock_timer_ms(false, false, true, false, 45), 44);
        assert_eq!(compute_dock_timer_ms(false, false, true, false, 24), 80);
        assert_eq!(compute_dock_timer_ms(false, false, true, false, 120), 40);
        // Settled static/tactile dock drops completely to 0ms (0% idle CPU)
        assert_eq!(compute_dock_timer_ms(false, false, false, false, 45), 0);
        assert_eq!(compute_dock_timer_ms(false, false, false, false, 120), 0);

        // Disabled dock engine always reports 0ms timer even on ambient themes
        let eng = MacDockEngine::new(HINSTANCE::default());
        let mut cfg = AppConfig::default();
        cfg.apply_taskbar_theme_preset(TaskbarTheme::OceanTideDock);
        assert_eq!(eng.desired_timer_ms(&cfg), 0);

        // Vertical dock with marquee_ticker enabled does not falsely trigger ambient animation
        let mut vert_cfg = AppConfig::default();
        vert_cfg.apply_taskbar_theme_preset(TaskbarTheme::SaddleLeatherDock);
        vert_cfg.mac_dock_edge = MacDockEdge::Left;
        vert_cfg.w_hud.marquee_ticker = true;
        assert!(
            !crate::dock_themes::is_ambient_animated_cfg(&vert_cfg),
            "Static vertical dock must not run ambient animation just because marquee_ticker is true"
        );
    }

    #[test]
    fn reactive_tray_animations_all_styles_and_idle_settling() {
        let mut cfg = AppConfig::default();
        cfg.apply_taskbar_theme_preset(TaskbarTheme::RealisticWaterDock);
        cfg.tb_ambient_anim = true;

        let mut anim = TrayAnimState::default();
        anim.seed_volume(70, false);
        anim.seed_net(true, 90);
        anim.seed_battery(Some(75));
        assert!(
            !anim.is_active(&cfg, false, false, false),
            "Freshly seeded tray must be idle (0% CPU) with no spurious startup animation"
        );

        // 1. Reducing volume triggers directional -1 animation, smooth glide, and live badge
        anim.on_volume_change(70, 40, false, false, true);
        assert_eq!(anim.vol_dir, -1);
        assert_eq!(anim.vol_pulse_t, 0.0);
        assert!(anim.vol_badge_ttl > 1.0);
        assert!(anim.is_active(&cfg, false, false, false));
        let moved = anim.step(0.022, false, false, false, true);
        assert!(moved);
        assert!(anim.vol_display < 0.70 && anim.vol_display > 0.40);
        assert!(anim.vol_pulse_t > 0.0 && anim.vol_pulse_t < 1.0);

        // Increasing volume flips direction to +1; mute toggle sets direction to 0
        anim.on_volume_change(40, 82, false, false, true);
        assert_eq!(anim.vol_dir, 1);
        anim.on_volume_change(82, 82, false, true, true);
        assert_eq!(anim.vol_dir, 0);
        assert!(anim.is_active(&cfg, true, false, false));

        // 2. Network & Battery transitions trigger ripples and settle back to exact idle
        anim.on_net_change(false, 0, true);
        assert_eq!(anim.net_pulse_t, 0.0);
        anim.on_battery_change(Some(74), true);
        assert_eq!(anim.bat_pulse_t, 0.0);

        // Run ~2.5s of frames (at 45 FPS = 0.022s) -> all transients settle cleanly to 0% idle
        for _ in 0..120 {
            anim.step(0.022, true, false, false, true);
        }
        assert!(
            !anim.is_active(&cfg, true, false, false),
            "Settled tray must drop back to idle once transient volume/net/bat pulses finish"
        );

        // Charging and low-battery states keep gentle ambient shimmer active when enabled
        assert!(anim.is_active(&cfg, false, true, false));
        assert!(anim.is_active(&cfg, false, false, true));
        // LiveVuEqualizer wakes up on real audio bands and settles when audio stops
        cfg.w_tray.audio_style = TrayAudioMeterStyle::LiveVuEqualizer;
        anim.update_audio_vu(&[0.8; 12]);
        assert!(anim.is_active(&cfg, false, false, false));
        for _ in 0..25 {
            anim.update_audio_vu(&[0.0; 12]);
        }
        assert!(!anim.has_audio_vu_energy());

        // 3. Direct2D smoke test across ALL Audio × Net × Battery styles (horizontal + vertical)
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        }
        let mut eng = MacDockEngine::new(HINSTANCE::default());
        assert!(eng.ensure_graphics(), "Direct2D graphics must initialize for tray render test");
        let mut surf = DibSurface::new();
        assert!(surf.ensure_size(120, 120));
        let rc = RECT { left: 0, top: 0, right: 120, bottom: 120 };
        let rt = eng.dc_rt.as_ref().unwrap();
        let br = unsafe { rt.CreateSolidColorBrush(&rgba(255, 255, 255, 1.0), None).unwrap() };
        unsafe {
            assert!(rt.BindDC(surf.mem_dc, &rc).is_ok());
            rt.BeginDraw();
            for &aud in &TrayAudioMeterStyle::ALL {
                for &net in &TrayNetMeterStyle::ALL {
                    for &bat in &TrayBatteryMeterStyle::ALL {
                        cfg.w_tray.audio_style = aud;
                        cfg.w_tray.net_style = net;
                        cfg.w_tray.battery_style = bat;
                        anim.on_volume_change(75, 42, false, false, true);
                        anim.step(0.05, false, true, false, true);
                        for is_vert in [false, true] {
                            let slot_rc = if is_vert {
                                D2D_RECT_F { left: 8.0, top: 8.0, right: 56.0, bottom: 92.0 }
                            } else {
                                D2D_RECT_F { left: 8.0, top: 8.0, right: 94.0, bottom: 52.0 }
                            };
                            draw_tray_cluster(
                                rt,
                                eng.d2d.as_ref(),
                                &br,
                                eng.tf_mono.as_ref(),
                                slot_rc,
                                42,
                                false,
                                true,
                                78,
                                Some(86),
                                true,
                                true,
                                true,
                                (244, 250, 255),
                                (165, 190, 210),
                                (0, 224, 240),
                                &cfg,
                                &anim,
                                1.2,
                                is_vert,
                                1.0,
                            );
                        }
                    }
                }
            }
            assert!(rt.EndDraw(None, None).is_ok());
        }
        surf.release();
        unsafe {
            CoUninitialize();
        }
    }
}
