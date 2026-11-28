use std::ffi::c_void;
use std::mem::size_of;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

static MF_CALL_DEPTH: AtomicU32 = AtomicU32::new(0);
static WP_OS_EXIT: AtomicBool = AtomicBool::new(false);

struct MfCallGuard;

impl MfCallGuard {
    fn enter() -> Self {
        MF_CALL_DEPTH.fetch_add(1, Ordering::SeqCst);
        Self
    }
}

impl Drop for MfCallGuard {
    fn drop(&mut self) {
        MF_CALL_DEPTH.fetch_sub(1, Ordering::SeqCst);
    }
}

/// Balances `CoInitializeEx` / `CoUninitialize` cleanly on both UI and worker threads.
struct ComInitGuard {
    should_uninit: bool,
}

impl ComInitGuard {
    unsafe fn enter() -> Self {
        let hr = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        // S_OK (0) or S_FALSE (1) incremented this thread's COM init count and require CoUninitialize.
        Self {
            should_uninit: hr.0 == 0 || hr.0 == 1,
        }
    }
}

impl Drop for ComInitGuard {
    fn drop(&mut self) {
        if self.should_uninit {
            unsafe {
                CoUninitialize();
            }
        }
    }
}

use windows::core::{implement, w, BOOL, BSTR, Interface, PCWSTR, PWSTR};
use windows::Win32::Foundation::{
    CloseHandle, COLORREF, GENERIC_READ, HANDLE, HINSTANCE, HMODULE, HWND, LPARAM, LRESULT, RECT,
    SIZE, WAIT_OBJECT_0, WPARAM,
};
use windows::Win32::Graphics::Direct3D::{
    D3D_DRIVER_TYPE_HARDWARE, D3D_DRIVER_TYPE_UNKNOWN, D3D_DRIVER_TYPE_WARP,
};
use windows::Win32::Graphics::Direct3D11::{
    D3D11CreateDevice, ID3D11Device, ID3D11Multithread, ID3D11Resource, ID3D11Texture2D,
    ID3D11VideoDevice, D3D11_BIND_RENDER_TARGET, D3D11_BIND_SHADER_RESOURCE,
    D3D11_CPU_ACCESS_READ, D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_CREATE_DEVICE_VIDEO_SUPPORT,
    D3D11_MAPPED_SUBRESOURCE, D3D11_MAP_FLAG_DO_NOT_WAIT, D3D11_MAP_READ, D3D11_SDK_VERSION,
    D3D11_SUBRESOURCE_DATA, D3D11_TEXTURE2D_DESC, D3D11_USAGE_DEFAULT, D3D11_USAGE_IMMUTABLE,
    D3D11_USAGE_STAGING,
};
use windows::Win32::Graphics::Dwm::DwmFlush;
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_ALPHA_MODE_UNSPECIFIED, DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_FORMAT_UNKNOWN,
    DXGI_SAMPLE_DESC,
};
use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory1, IDXGIAdapter, IDXGIDevice, IDXGIDevice3, IDXGIFactory1, IDXGIFactory2,
    IDXGISwapChain1, IDXGISwapChain2, DXGI_ADAPTER_FLAG_SOFTWARE, DXGI_MWA_NO_ALT_ENTER,
    DXGI_PRESENT, DXGI_PRESENT_TEST, DXGI_SCALING_STRETCH, DXGI_SWAP_CHAIN_DESC1,
    DXGI_SWAP_CHAIN_FLAG, DXGI_SWAP_CHAIN_FLAG_FRAME_LATENCY_WAITABLE_OBJECT,
    DXGI_SWAP_EFFECT_FLIP_DISCARD, DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL, DXGI_USAGE_BACK_BUFFER,
    DXGI_USAGE_RENDER_TARGET_OUTPUT,
};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, BitBlt, CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, EndPaint,
    EnumDisplaySettingsW, GetDIBits, GetMonitorInfoW, GetObjectW, InvalidateRect, MonitorFromPoint,
    RedrawWindow, SelectObject, SetStretchBltMode, StretchBlt, UpdateWindow, BITMAP, BITMAPINFO,
    BITMAPINFOHEADER, BI_RGB, DEVMODEW, DIB_RGB_COLORS, ENUM_CURRENT_SETTINGS, HALFTONE, HBITMAP,
    HGDIOBJ, MONITORINFO, MONITOR_DEFAULTTOPRIMARY, PAINTSTRUCT, RDW_ERASE, RDW_FRAME,
    RDW_INVALIDATE, RDW_UPDATENOW, SRCCOPY,
};
use windows::Win32::Graphics::Imaging::{
    CLSID_WICImagingFactory, GUID_WICPixelFormat32bppBGRA, IWICBitmapSource, IWICImagingFactory,
    WICBitmapDitherTypeNone, WICBitmapInterpolationModeFant, WICBitmapPaletteTypeCustom,
    WICBitmapTransformFlipHorizontal, WICBitmapTransformFlipVertical, WICBitmapTransformOptions,
    WICBitmapTransformRotate180, WICBitmapTransformRotate270, WICBitmapTransformRotate90,
    WICDecodeMetadataCacheOnDemand, WICRect,
};
use windows::Win32::Media::MediaFoundation::{
    CLSID_MFMediaEngineClassFactory, IMFAttributes, IMFDXGIDeviceManager, IMFMediaEngine,
    IMFMediaEngineClassFactory, IMFMediaEngineNotify, IMFMediaEngineNotify_Impl,
    MFCreateAttributes, MFCreateDXGIDeviceManager, MFShutdown, MFStartup, MFVideoNormalizedRect,
    MF_API_VERSION, MF_MEDIA_ENGINE_CALLBACK, MF_MEDIA_ENGINE_DXGI_MANAGER,
    MF_MEDIA_ENGINE_EVENT_ENDED, MF_MEDIA_ENGINE_EVENT_FIRSTFRAMEREADY,
    MF_MEDIA_ENGINE_EVENT_LOADEDDATA, MF_MEDIA_ENGINE_VIDEO_OUTPUT_FORMAT,
};
use windows::Win32::System::Com::StructuredStorage::{
    PropVariantClear, PropVariantToUInt16, PROPVARIANT,
};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER,
    COINIT_APARTMENTTHREADED,
};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::Threading::{
    OpenProcess, TerminateProcess, WaitForSingleObject, PROCESS_TERMINATE,
};
use windows::Win32::UI::Controls::Dialogs::{
    GetOpenFileNameW, OFN_FILEMUSTEXIST, OFN_NOCHANGEDIR, OPENFILENAMEW,
};
use windows::Win32::UI::Shell::{
    IShellItemImageFactory, SHCreateItemFromParsingName, SIIGBF_BIGGERSIZEOK, SIIGBF_THUMBNAILONLY,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, EnumWindows, FindWindowExW, FindWindowW,
    GetLayeredWindowAttributes, GetParent, GetWindow, GetWindowLongPtrW, GetWindowThreadProcessId,
    IsWindow, IsWindowVisible, KillTimer, PostMessageW, RegisterClassExW, SendMessageTimeoutW,
    SetLayeredWindowAttributes, SetParent, SetTimer, SetWindowLongPtrW, SetWindowPos, ShowWindow,
    GWLP_USERDATA, GWL_EXSTYLE, GWL_STYLE, GW_HWNDNEXT, HWND_BOTTOM, HWND_TOP,
    LAYERED_WINDOW_ATTRIBUTES_FLAGS, LWA_ALPHA, SMTO_NORMAL, SWP_ASYNCWINDOWPOS, SWP_FRAMECHANGED,
    SWP_HIDEWINDOW, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SWP_SHOWWINDOW, SW_HIDE,
    SW_SHOW, WM_DISPLAYCHANGE, WM_ERASEBKGND, WM_SIZE, WM_TIMER, WNDCLASSEXW, WS_CHILD,
    WS_CLIPCHILDREN, WS_CLIPSIBLINGS, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
    WS_EX_TRANSPARENT, WS_POPUP, WS_VISIBLE,
};

use crate::config::{
    cache_wallpaper_locally, is_static_image_wallpaper_path, to_wide, AppConfig, WallpaperFpsCap,
};

const WM_WP_EVENT: u32 = 0x8000 + 301;
const WM_WP_STATIC_READY: u32 = 0x8000 + 302;
const WM_WP_PUMP: u32 = 0x8000 + 303;
const TIMER_WP_FRAME: usize = 301;
const TIMER_WP_STATIC_RETRY: usize = 302;
const DXGI_STATUS_OCCLUDED_HR: i32 = 0x087A_0001u32 as i32;
const DXGI_ERROR_WAS_STILL_DRAWING_HR: i32 = 0x887A_000Au32 as i32;

/// Tracks whether live video wallpaper playback is using full D3D11/DXVA hardware decode
/// or fell back to CPU software decode / WARP software rendering.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VideoDecodeMode {
    Unknown,
    HardwareDxva,
    SoftwareCpu,
    WarpSoftware,
}

impl Default for VideoDecodeMode {
    fn default() -> Self {
        Self::Unknown
    }
}

impl VideoDecodeMode {
    pub fn is_software_fallback(&self) -> bool {
        matches!(self, Self::SoftwareCpu | Self::WarpSoftware)
    }

    pub fn badge_label(&self) -> &'static str {
        match self {
            Self::HardwareDxva => "DXVA HARDWARE",
            Self::SoftwareCpu => "CPU FALLBACK (NO DXVA)",
            Self::WarpSoftware => "WARP SOFTWARE GPU",
            Self::Unknown => "HARDWARE DECODER",
        }
    }

    pub fn warning_pill_text(&self) -> Option<&'static str> {
        match self {
            Self::SoftwareCpu => {
                Some("⚠ No DXVA HW decode — use H.264 MP4 or install HEVC/AV1 codec")
            }
            Self::WarpSoftware => {
                Some("⚠ WARP software GPU fallback — update display drivers for smooth FPS")
            }
            _ => None,
        }
    }
}

/// Pure double-buffered FIFO tracker for async `64x36` Chameleon staging texture readbacks.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct AsyncPaletteSlots {
    in_flight: [bool; 2],
    smooth: [bool; 2],
    write_slot: usize,
}

impl AsyncPaletteSlots {
    fn acquire_write_slot(&self) -> Option<usize> {
        if !self.in_flight[self.write_slot] {
            Some(self.write_slot)
        } else {
            let alt = self.write_slot ^ 1;
            if !self.in_flight[alt] {
                Some(alt)
            } else {
                None
            }
        }
    }

    fn mark_submitted(&mut self, slot: usize, smooth: bool) {
        let s = slot & 1;
        self.in_flight[s] = true;
        self.smooth[s] = smooth;
        self.write_slot = s ^ 1;
    }

    /// Returns the oldest submitted slot `(slot_idx, smooth)` waiting for non-blocking `Map(DO_NOT_WAIT)`.
    fn oldest_in_flight(&self) -> Option<(usize, bool)> {
        if self.in_flight[self.write_slot] {
            Some((self.write_slot, self.smooth[self.write_slot]))
        } else {
            let alt = self.write_slot ^ 1;
            if self.in_flight[alt] {
                Some((alt, self.smooth[alt]))
            } else {
                None
            }
        }
    }

    fn mark_completed(&mut self, slot: usize) {
        self.in_flight[slot & 1] = false;
    }

    fn any_in_flight(&self) -> bool {
        self.in_flight[0] || self.in_flight[1]
    }

    fn reset(&mut self) {
        self.in_flight = [false, false];
        self.smooth = [false, false];
        self.write_slot = 0;
    }
}

fn query_primary_monitor_refresh_hz() -> u32 {
    unsafe {
        let mut dm = DEVMODEW {
            dmSize: size_of::<DEVMODEW>() as u16,
            ..Default::default()
        };
        if EnumDisplaySettingsW(PCWSTR::null(), ENUM_CURRENT_SETTINGS, &mut dm).as_bool()
            && dm.dmDisplayFrequency >= 24
            && dm.dmDisplayFrequency <= 360
        {
            return dm.dmDisplayFrequency;
        }
    }
    60
}

/// Computes the target pacer interval in microseconds matching monitor refresh rate & FPS cap.
fn compute_target_frame_interval_us(fps_cap: WallpaperFpsCap, monitor_hz: u32) -> u64 {
    let hz = monitor_hz.clamp(24, 240) as u64;
    match fps_cap {
        WallpaperFpsCap::Fps30 => 33_333,
        WallpaperFpsCap::Fps60 => 1_000_000 / hz.min(60),
    }
}

/// Computes the effective pacer interval honoring the user's explicit 60 / 30 FPS cap verbatim.
fn compute_effective_pacer_interval_us(
    fps_cap: WallpaperFpsCap,
    monitor_hz: u32,
    _interactive_yield: bool,
) -> u64 {
    compute_target_frame_interval_us(fps_cap, monitor_hz)
}

/// Computes how long a paused, maximized-covered video holds its `IMFMediaEngine` before
/// dropping ONLY the heavyweight Media Foundation decoder/surface pool while keeping the
/// warm D3D11 Flip-Model SwapChain & frozen last frame visible (zero blanking).
pub(crate) fn compute_cover_reclaim_delay_ms(
    trim_on_pause: bool,
    native_video_w: u32,
    native_video_h: u32,
) -> u64 {
    if trim_on_pause {
        4_000
    } else if native_video_w >= 2560 || native_video_h >= 1440 {
        // 4K / UHD videos hold 300-500 MB of MF/DXVA surfaces: auto-reclaim after 8s covered
        8_000
    } else {
        15_000
    }
}

/// Minimum interval between GPU `Present()` calls when a hard FPS cap is selected, with
/// generous scheduling-jitter tolerance so normal UI-thread wakeup drift on 60Hz/120Hz
/// displays never halves 60 FPS down to 30 FPS while still strictly enforcing the cap.
fn compute_min_present_interval_us(fps_cap: WallpaperFpsCap) -> u64 {
    match fps_cap {
        WallpaperFpsCap::Fps60 => 13_500,
        WallpaperFpsCap::Fps30 => 29_000,
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum StaticLoadState {
    Idle,
    Loading,
    Ready,
    Failed,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum PresentOutcome {
    Presented,
    Occluded,
    Failed,
}

struct StaticLoadRequest {
    gen: u64,
    path: String,
    target_w: u32,
    target_h: u32,
    wp_hwnd_raw: usize,
    notify_hwnd_raw: usize,
    notify_msg: u32,
    cache_locally: bool,
}

struct StaticLoadResult {
    gen: u64,
    req_path: String,
    cached_path: String,
    frame: Option<(Vec<u8>, u32, u32, Vec<u8>)>,
}

struct PendingStaticFrame {
    bgra: Vec<u8>,
    w: u32,
    h: u32,
}

#[implement(IMFMediaEngineNotify)]
struct WallpaperNotifyCallback {
    hwnd: HWND,
}

impl IMFMediaEngineNotify_Impl for WallpaperNotifyCallback_Impl {
    fn EventNotify(&self, event: u32, _param1: usize, _param2: u32) -> windows::core::Result<()> {
        if !self.hwnd.0.is_null() {
            let ev = event as i32;
            if ev == MF_MEDIA_ENGINE_EVENT_FIRSTFRAMEREADY.0
                || ev == MF_MEDIA_ENGINE_EVENT_LOADEDDATA.0
                || ev == MF_MEDIA_ENGINE_EVENT_ENDED.0
            {
                unsafe {
                    let _ = PostMessageW(Some(self.hwnd), WM_WP_EVENT, WPARAM(event as usize), LPARAM(0));
                }
            }
        }
        Ok(())
    }
}

enum HostPlacement {
    /// Win11 24H2/26H2 RaisedDesktop: `Progman` holds both `SHELLDLL_DefView` and a child `WorkerW`.
    /// `worker_split_ready` is true only once `Progman`'s child `WorkerW` exists so `SHELLDLL_DefView`
    /// is layered/transparent instead of painting the native Windows wallpaper opaquely on top.
    ProgmanChild {
        progman: HWND,
        after_defview: HWND,
        worker_split_ready: bool,
    },
    WorkerWChild(HWND),
    DesktopFallback,
}

unsafe fn find_toplevel_worker_behind_defview() -> Option<HWND> {
    let mut found_worker = HWND::default();
    unsafe extern "system" fn enum_cb(hwnd: HWND, lparam: LPARAM) -> windows::core::BOOL {
        if let Ok(dv) = FindWindowExW(Some(hwnd), None, w!("SHELLDLL_DefView"), PCWSTR::null()) {
            if !dv.0.is_null() {
                if let Ok(worker) = FindWindowExW(None, Some(hwnd), w!("WorkerW"), PCWSTR::null()) {
                    if !worker.0.is_null() {
                        *(lparam.0 as *mut HWND) = worker;
                        return windows::core::BOOL(0);
                    }
                }
            }
        }
        windows::core::BOOL(1)
    }
    let _ = EnumWindows(Some(enum_cb), LPARAM(&mut found_worker as *mut _ as isize));
    if found_worker.0.is_null() {
        None
    } else {
        Some(found_worker)
    }
}

unsafe fn find_worker_behind_defview() -> HostPlacement {
    let Ok(progman) = FindWindowW(w!("Progman"), w!("Program Manager")) else {
        return HostPlacement::DesktopFallback;
    };
    if progman.0.is_null() {
        return HostPlacement::DesktopFallback;
    }

    // 1. Win11 24H2/26H2 RaisedDesktop: Progman holds BOTH SHELLDLL_DefView and a child WorkerW
    // once 0x052C has split the desktop wallpaper composition layer.
    let defview = FindWindowExW(Some(progman), None, w!("SHELLDLL_DefView"), PCWSTR::null())
        .unwrap_or_default();
    let child_worker = FindWindowExW(Some(progman), None, w!("WorkerW"), PCWSTR::null())
        .unwrap_or_default();
    if !defview.0.is_null() && !child_worker.0.is_null() {
        return HostPlacement::ProgmanChild {
            progman,
            after_defview: defview,
            worker_split_ready: true,
        };
    }

    // 2. Pre-24H2 Windows: SHELLDLL_DefView in top-level WorkerW + trailing WorkerW sibling.
    if let Some(worker) = find_toplevel_worker_behind_defview() {
        return HostPlacement::WorkerWChild(worker);
    }

    // 3. Cold boot / fresh Explorer: spawn the WorkerW wallpaper layer before attaching.
    let mut result: usize = 0;
    let _ = SendMessageTimeoutW(
        progman,
        0x052C,
        WPARAM(0xD),
        LPARAM(0x1),
        SMTO_NORMAL,
        250,
        Some(&mut result),
    );

    let mut defview_after = FindWindowExW(Some(progman), None, w!("SHELLDLL_DefView"), PCWSTR::null())
        .unwrap_or_default();
    let mut worker_after = FindWindowExW(Some(progman), None, w!("WorkerW"), PCWSTR::null())
        .unwrap_or_default();
    if !defview_after.0.is_null() && worker_after.0.is_null() && find_toplevel_worker_behind_defview().is_none() {
        // Retry with legacy 0x052C(0, 0) and short poll if Explorer hasn't finalized the split yet.
        let _ = SendMessageTimeoutW(
            progman,
            0x052C,
            WPARAM(0),
            LPARAM(0),
            SMTO_NORMAL,
            250,
            Some(&mut result),
        );
        for _ in 0..4 {
            worker_after = FindWindowExW(Some(progman), None, w!("WorkerW"), PCWSTR::null())
                .unwrap_or_default();
            if !worker_after.0.is_null() || find_toplevel_worker_behind_defview().is_some() {
                break;
            }
            std::thread::sleep(Duration::from_millis(15));
        }
        defview_after = FindWindowExW(Some(progman), None, w!("SHELLDLL_DefView"), PCWSTR::null())
            .unwrap_or_default();
    }

    if !defview_after.0.is_null() && !worker_after.0.is_null() {
        return HostPlacement::ProgmanChild {
            progman,
            after_defview: defview_after,
            worker_split_ready: true,
        };
    }
    if let Some(worker) = find_toplevel_worker_behind_defview() {
        return HostPlacement::WorkerWChild(worker);
    }
    if !defview_after.0.is_null() {
        return HostPlacement::ProgmanChild {
            progman,
            after_defview: defview_after,
            worker_split_ready: false,
        };
    }
    HostPlacement::DesktopFallback
}

pub fn pick_video_file(owner: HWND) -> Option<String> {
    let mut buf = vec![0u16; 520];
    let filter = to_wide(
        "All Wallpapers (*.mp4;*.mkv;*.webm;*.mov;*.avi;*.png;*.jpg;*.jpeg;*.webp;*.bmp)\0\
         *.mp4;*.mkv;*.webm;*.mov;*.avi;*.png;*.jpg;*.jpeg;*.webp;*.bmp\0\
         Video Files (*.mp4;*.mkv;*.webm;*.mov;*.avi)\0*.mp4;*.mkv;*.webm;*.mov;*.avi\0\
         Image Files (*.png;*.jpg;*.jpeg;*.webp;*.bmp)\0*.png;*.jpg;*.jpeg;*.webp;*.bmp\0\
         All Files (*.*)\0*.*\0",
    );
    let title = to_wide("Select Video or Image Wallpaper");
    let mut ofn = OPENFILENAMEW {
        lStructSize: size_of::<OPENFILENAMEW>() as u32,
        hwndOwner: owner,
        lpstrFilter: PCWSTR(filter.as_ptr()),
        lpstrFile: PWSTR(buf.as_mut_ptr()),
        nMaxFile: buf.len() as u32,
        lpstrTitle: PCWSTR(title.as_ptr()),
        Flags: OFN_FILEMUSTEXIST | OFN_NOCHANGEDIR,
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

/// Computes an aspect-fill center crop rectangle `(x, y, cw, ch)` inside `(src_w, src_h)`
/// matching the aspect ratio of `(dst_w, dst_h)`.
fn compute_aspect_fill_crop(src_w: u32, src_h: u32, dst_w: u32, dst_h: u32) -> (u32, u32, u32, u32) {
    if src_w == 0 || src_h == 0 || dst_w == 0 || dst_h == 0 {
        return (0, 0, src_w.max(1), src_h.max(1));
    }
    let sa = src_w as f64 / src_h as f64;
    let da = dst_w as f64 / dst_h as f64;
    if (sa - da).abs() < 0.01 {
        (0, 0, src_w, src_h)
    } else if sa > da {
        let cw = ((src_h as f64 * da).round() as u32).clamp(1, src_w);
        let x = (src_w - cw) / 2;
        (x, 0, cw, src_h)
    } else {
        let ch = ((src_w as f64 / da).round() as u32).clamp(1, src_h);
        let y = (src_h - ch) / 2;
        (0, y, src_w, ch)
    }
}

/// Bilinear aspect-fill resampler from top-down BGRA source to top-down opaque BGRA destination.
fn resample_bgra_aspect_fill(
    src: &[u8],
    src_w: u32,
    src_h: u32,
    dst_w: u32,
    dst_h: u32,
) -> Vec<u8> {
    let mut out = vec![0u8; (dst_w as usize) * (dst_h as usize) * 4];
    if src_w == 0 || src_h == 0 || dst_w == 0 || dst_h == 0 {
        return out;
    }
    if src.len() < (src_w as usize) * (src_h as usize) * 4 {
        return out;
    }
    let (cx, cy, cw, ch) = compute_aspect_fill_crop(src_w, src_h, dst_w, dst_h);
    let sx_scale = cw as f32 / dst_w as f32;
    let sy_scale = ch as f32 / dst_h as f32;
    let max_x = (src_w - 1) as f32;
    let max_y = (src_h - 1) as f32;

    for dy in 0..dst_h {
        let fy = (cy as f32 + (dy as f32 + 0.5) * sy_scale - 0.5).clamp(0.0, max_y);
        let y0 = fy.floor() as u32;
        let y1 = (y0 + 1).min(src_h - 1);
        let wy = fy - y0 as f32;
        let wy0 = 1.0 - wy;
        let row0 = (y0 * src_w) as usize;
        let row1 = (y1 * src_w) as usize;
        let dst_row = (dy * dst_w) as usize;

        for dx in 0..dst_w {
            let fx = (cx as f32 + (dx as f32 + 0.5) * sx_scale - 0.5).clamp(0.0, max_x);
            let x0 = fx.floor() as u32;
            let x1 = (x0 + 1).min(src_w - 1);
            let wx = fx - x0 as f32;
            let wx0 = 1.0 - wx;

            let i00 = (row0 + x0 as usize) * 4;
            let i10 = (row0 + x1 as usize) * 4;
            let i01 = (row1 + x0 as usize) * 4;
            let i11 = (row1 + x1 as usize) * 4;
            let di = (dst_row + dx as usize) * 4;

            for c in 0..3 {
                let top = src[i00 + c] as f32 * wx0 + src[i10 + c] as f32 * wx;
                let bot = src[i01 + c] as f32 * wx0 + src[i11 + c] as f32 * wx;
                out[di + c] = (top * wy0 + bot * wy).round().clamp(0.0, 255.0) as u8;
            }
            out[di + 3] = 255;
        }
    }
    out
}

/// Maps EXIF orientation tag (1..=8) to WIC flip/rotate transform options.
/// WIC applies rotation before flip, so 5 (Transpose) = Rotate90 | FlipHorizontal
/// and 7 (Transverse) = Rotate270 | FlipHorizontal.
fn exif_orientation_to_wic_transform(orient: u16) -> Option<WICBitmapTransformOptions> {
    match orient {
        2 => Some(WICBitmapTransformFlipHorizontal),
        3 => Some(WICBitmapTransformRotate180),
        4 => Some(WICBitmapTransformFlipVertical),
        5 => Some(WICBitmapTransformOptions(
            WICBitmapTransformRotate90.0 | WICBitmapTransformFlipHorizontal.0,
        )),
        6 => Some(WICBitmapTransformRotate90),
        7 => Some(WICBitmapTransformOptions(
            WICBitmapTransformRotate270.0 | WICBitmapTransformFlipHorizontal.0,
        )),
        8 => Some(WICBitmapTransformRotate270),
        _ => None,
    }
}

/// Fast in-memory 64x36 BGRA downsample from an already-decoded wallpaper buffer so Chameleon
/// wallpaper palette matching never decodes the image file a second time via WIC.
fn downsample_bgra_for_palette(bgra: &[u8], w: u32, h: u32) -> Vec<u8> {
    resample_bgra_aspect_fill(bgra, w, h, 64, 36)
}

/// Decodes a static image file via WIC directly to an aspect-fill opaque BGRA buffer `(out_w, out_h)`.
/// Applies EXIF orientation when present and converts to `32bppBGRA` *before* clipping/scaling so
/// indexed PNGs, CMYK JPEGs, 16-bit images, and WebP files never fail `IWICBitmapScaler`.
fn decode_image_via_wic_bgra(path: &str, out_w: u32, out_h: u32) -> Option<Vec<u8>> {
    if out_w == 0 || out_h == 0 || !Path::new(path).is_file() {
        return None;
    }
    unsafe {
        let _com = ComInitGuard::enter();
        let wic: IWICImagingFactory =
            CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER).ok()?;
        let wide = to_wide(path);
        let dec = wic
            .CreateDecoderFromFilename(
                PCWSTR(wide.as_ptr()),
                None,
                GENERIC_READ,
                WICDecodeMetadataCacheOnDemand,
            )
            .ok()?;
        let frame = dec.GetFrame(0).ok()?;

        // Apply EXIF orientation if present (portrait/rotated camera photos).
        let mut orient_opt: Option<WICBitmapTransformOptions> = None;
        if let Ok(reader) = frame.GetMetadataQueryReader() {
            for query in [w!("/app1/ifd/{ushort=274}"), w!("/ifd/{ushort=274}")] {
                let mut pv = PROPVARIANT::default();
                if reader.GetMetadataByName(query, &mut pv).is_ok() {
                    if let Ok(v) = PropVariantToUInt16(&pv) {
                        orient_opt = exif_orientation_to_wic_transform(v);
                    }
                    let _ = PropVariantClear(&mut pv);
                    if orient_opt.is_some() {
                        break;
                    }
                }
            }
        }

        let oriented_src: IWICBitmapSource = if let Some(xform) = orient_opt {
            if let Ok(rot) = wic.CreateBitmapFlipRotator() {
                if rot.Initialize(&frame, xform).is_ok() {
                    rot.cast().ok()?
                } else {
                    frame.cast().ok()?
                }
            } else {
                frame.cast().ok()?
            }
        } else {
            frame.cast().ok()?
        };

        // Convert to 32bppBGRA before clip/scale so IWICBitmapScaler supports every image format.
        let conv = wic.CreateFormatConverter().ok()?;
        conv.Initialize(
            &oriented_src,
            &GUID_WICPixelFormat32bppBGRA,
            WICBitmapDitherTypeNone,
            None,
            0.0,
            WICBitmapPaletteTypeCustom,
        )
        .ok()?;
        let bgra_src: IWICBitmapSource = conv.cast().ok()?;

        let mut iw = 0u32;
        let mut ih = 0u32;
        bgra_src.GetSize(&mut iw, &mut ih).ok()?;
        if iw == 0 || ih == 0 {
            return None;
        }
        let (cx, cy, cw, ch) = compute_aspect_fill_crop(iw, ih, out_w, out_h);
        let clipped_src: IWICBitmapSource = if cx != 0 || cy != 0 || cw != iw || ch != ih {
            let clipper = wic.CreateBitmapClipper().ok()?;
            let rc = WICRect {
                X: cx as i32,
                Y: cy as i32,
                Width: cw as i32,
                Height: ch as i32,
            };
            clipper.Initialize(&bgra_src, &rc).ok()?;
            clipper.cast().ok()?
        } else {
            bgra_src
        };
        let final_src: IWICBitmapSource = if cw != out_w || ch != out_h {
            let scaler = wic.CreateBitmapScaler().ok()?;
            scaler
                .Initialize(&clipped_src, out_w, out_h, WICBitmapInterpolationModeFant)
                .ok()?;
            scaler.cast().ok()?
        } else {
            clipped_src
        };
        let stride = out_w * 4;
        let mut buf = vec![0u8; (stride * out_h) as usize];
        final_src.CopyPixels(std::ptr::null(), stride, &mut buf).ok()?;
        for px in buf.chunks_exact_mut(4) {
            px[3] = 255;
        }
        Some(buf)
    }
}

/// Decodes a Windows Shell thumbnail/image via `IShellItemImageFactory` into top-down BGRA pixels.
fn decode_via_shell_thumb_raw(path: &str, max_dim: i32, thumb_only: bool) -> Option<(Vec<u8>, u32, u32)> {
    if !Path::new(path).is_file() {
        return None;
    }
    unsafe {
        let _com = ComInitGuard::enter();
        let wide = to_wide(path);
        let factory: IShellItemImageFactory =
            SHCreateItemFromParsingName(PCWSTR(wide.as_ptr()), None).ok()?;
        let flags = if thumb_only {
            SIIGBF_BIGGERSIZEOK | SIIGBF_THUMBNAILONLY
        } else {
            SIIGBF_BIGGERSIZEOK
        };
        let hbmp = factory.GetImage(SIZE { cx: max_dim, cy: max_dim }, flags).ok()?;

        let mut bm = BITMAP::default();
        let got = GetObjectW(
            HGDIOBJ(hbmp.0),
            size_of::<BITMAP>() as i32,
            Some(&mut bm as *mut _ as *mut c_void),
        );
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

/// Decodes a crisp aspect-fill BGRA thumbnail `(out_w x out_h, alpha = 255)` for either a static
/// image wallpaper or a video wallpaper. Safe to call from a background worker thread.
pub fn decode_wallpaper_thumbnail_bgra(path: &str, out_w: u32, out_h: u32) -> Option<Vec<u8>> {
    if out_w == 0 || out_h == 0 || path.is_empty() || !Path::new(path).is_file() {
        return None;
    }
    let max_dim = out_w.max(out_h).clamp(64, 1024) as i32;
    if is_static_image_wallpaper_path(path) {
        if let Some(bgra) = decode_image_via_wic_bgra(path, out_w, out_h) {
            return Some(bgra);
        }
        if let Some((raw, rw, rh)) = decode_via_shell_thumb_raw(path, max_dim, true) {
            return Some(resample_bgra_aspect_fill(&raw, rw, rh, out_w, out_h));
        }
        return None;
    }
    // Video file: fast Windows Shell thumbnail cache/extractor first
    if let Some((raw, rw, rh)) = decode_via_shell_thumb_raw(path, max_dim.max(256), true) {
        return Some(resample_bgra_aspect_fill(&raw, rw, rh, out_w, out_h));
    }
    // Fallback: check Lively companion thumbnail images in the same folder
    if let Some(parent) = Path::new(path).parent() {
        for candidate in ["thumbnail.jpg", "thumbnail.png", "preview.jpg", "preview.png"] {
            let cp = parent.join(candidate);
            if cp.is_file() {
                if let Some(s) = cp.to_str() {
                    if let Some(bgra) = decode_image_via_wic_bgra(s, out_w, out_h) {
                        return Some(bgra);
                    }
                }
            }
        }
    }
    None
}

/// RAII wrapper around a top-down 32-bit GDI DIBSection used for zero-GPU static image wallpapers
/// and zero-blanking frozen last video frames when Media Foundation memory is reclaimed.
/// Returns true when the BGRA image contains visible non-black pixel data (rejecting all-zero
/// uninitialized GPU surfaces or empty readbacks before tearing down the live swapchain).
fn is_non_blank_bgra(bgra: &[u8], w: u32, h: u32) -> bool {
    let Some(expected) = (w as usize).checked_mul(h as usize).and_then(|n| n.checked_mul(4)) else {
        return false;
    };
    if w == 0 || h == 0 || bgra.len() < expected {
        return false;
    }
    let sx = (w / 8).max(1) as usize;
    let sy = (h / 8).max(1) as usize;
    let mut y = sy / 2;
    while y < h as usize {
        let mut x = sx / 2;
        while x < w as usize {
            let off = (y * (w as usize) + x) * 4;
            let lum = bgra[off] as u32 + bgra[off + 1] as u32 + bgra[off + 2] as u32;
            if lum >= 12 {
                return true;
            }
            x += sx;
        }
        y += sy;
    }
    false
}

/// Promotes `backdrop_hwnd` to `WS_EX_LAYERED` (`LWA_ALPHA = 255`) so Windows 11 24H2/26H2
/// `RaisedDesktop` (`Progman`) allocates a dedicated DWM visual for the GDI backdrop that sorts
/// strictly in front of `Progman`'s layered child `WorkerW` (static Windows wallpaper) and behind
/// `SHELLDLL_DefView` (desktop icons).
unsafe fn ensure_backdrop_layered_opaque(hwnd: HWND) {
    if hwnd.0.is_null() {
        return;
    }
    let want_ex = (WS_EX_LAYERED.0 | WS_EX_TRANSPARENT.0) as isize;
    let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
    if (ex & want_ex) != want_ex {
        let _ = SetWindowLongPtrW(hwnd, GWL_EXSTYLE, ex | want_ex);
    }
    let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), 255, LWA_ALPHA);
}

struct StaticDib {
    hbmp: HBITMAP,
    w: i32,
    h: i32,
}

impl StaticDib {
    /// Builds a 24-bpp opaque top-down `BI_RGB` DIB section from 32-bpp BGRA pixels.
    /// Using 24-bpp eliminates `alpha = 0x00` transparency issues from DXVA/MF video readbacks
    /// on Windows 11 24H2/26H2 DWM redirection surfaces and reduces memory footprint by 25%.
    fn from_bgra_top_down(w: u32, h: u32, bgra: &[u8]) -> Option<Self> {
        let expected = (w as usize).checked_mul(h as usize)?.checked_mul(4)?;
        if w == 0 || h == 0 || bgra.len() < expected {
            return None;
        }
        let dst_stride = ((w as usize).checked_mul(3)? + 3) & !3;
        let total_dst = dst_stride.checked_mul(h as usize)?;
        let mut bi = BITMAPINFO::default();
        bi.bmiHeader.biSize = size_of::<BITMAPINFOHEADER>() as u32;
        bi.bmiHeader.biWidth = w as i32;
        bi.bmiHeader.biHeight = -(h as i32); // top-down
        bi.bmiHeader.biPlanes = 1;
        bi.bmiHeader.biBitCount = 24;
        bi.bmiHeader.biCompression = BI_RGB.0;

        let mut bits: *mut c_void = std::ptr::null_mut();
        let hbmp = unsafe {
            CreateDIBSection(None, &bi, DIB_RGB_COLORS, &mut bits, None, 0).ok()?
        };
        if hbmp.0.is_null() || bits.is_null() {
            if !hbmp.0.is_null() {
                unsafe {
                    let _ = DeleteObject(HGDIOBJ(hbmp.0));
                }
            }
            return None;
        }
        unsafe {
            let dst = std::slice::from_raw_parts_mut(bits as *mut u8, total_dst);
            let w_us = w as usize;
            for row in 0..(h as usize) {
                let src_row = &bgra[row * w_us * 4..(row + 1) * w_us * 4];
                let dst_row = &mut dst[row * dst_stride..row * dst_stride + w_us * 3];
                for col in 0..w_us {
                    let si = col * 4;
                    let di = col * 3;
                    dst_row[di] = src_row[si];
                    dst_row[di + 1] = src_row[si + 1];
                    dst_row[di + 2] = src_row[si + 2];
                }
            }
        }
        Some(Self {
            hbmp,
            w: w as i32,
            h: h as i32,
        })
    }
}

impl Drop for StaticDib {
    fn drop(&mut self) {
        if !self.hbmp.0.is_null() {
            unsafe {
                let _ = DeleteObject(HGDIOBJ(self.hbmp.0));
            }
            self.hbmp = HBITMAP::default();
        }
    }
}

fn load_static_image_bgra(path: &str, target_w: u32, target_h: u32) -> Option<(Vec<u8>, u32, u32)> {
    let w = target_w.clamp(2, 3840);
    let h = target_h.clamp(2, 2160);
    if let Some(bgra) = decode_image_via_wic_bgra(path, w, h) {
        return Some((bgra, w, h));
    }
    let max_dim = w.max(h).min(1920) as i32;
    let (raw, rw, rh) = decode_via_shell_thumb_raw(path, max_dim, true)?;
    let bgra = resample_bgra_aspect_fill(&raw, rw, rh, w, h);
    Some((bgra, w, h))
}

unsafe fn find_progman_mpv_pids() -> Vec<u32> {
    let mut pids = Vec::new();
    if let Ok(progman) = FindWindowW(w!("Progman"), PCWSTR::null()) {
        if !progman.0.is_null() {
            let mut cur = HWND::default();
            loop {
                cur = FindWindowExW(Some(progman), Some(cur), w!("mpv"), PCWSTR::null())
                    .unwrap_or_default();
                if cur.0.is_null() {
                    break;
                }
                let mut pid = 0u32;
                let _ = GetWindowThreadProcessId(cur, Some(&mut pid));
                if pid != 0 && !pids.contains(&pid) {
                    pids.push(pid);
                }
            }
        }
    }
    pids
}

const LIVELY_SCAN_TTL: std::time::Duration = std::time::Duration::from_millis(3000);
static LIVELY_SCAN_CACHE: std::sync::Mutex<(Option<std::time::Instant>, bool)> =
    std::sync::Mutex::new((None, false));

fn lively_running_cached_with<F: FnMut() -> bool>(
    cache: &std::sync::Mutex<(Option<std::time::Instant>, bool)>,
    now: std::time::Instant,
    ttl: std::time::Duration,
    mut scan_processes: F,
) -> bool {
    if let Ok(guard) = cache.lock() {
        if let (Some(ts), cached) = *guard {
            if now.saturating_duration_since(ts) < ttl {
                return cached;
            }
        }
    }
    let found = scan_processes();
    if let Ok(mut guard) = cache.lock() {
        *guard = (Some(now), found);
    }
    found
}

fn scan_lively_processes() -> bool {
    unsafe {
        let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else {
            return false;
        };
        let mut pe = PROCESSENTRY32W {
            dwSize: size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        let mut found = false;
        if Process32FirstW(snap, &mut pe).is_ok() {
            loop {
                let end = pe.szExeFile.iter().position(|&c| c == 0).unwrap_or(pe.szExeFile.len());
                let name = String::from_utf16_lossy(&pe.szExeFile[..end]).to_ascii_lowercase();
                if name == "lively.exe" || name == "lively.watchdog.exe" {
                    found = true;
                    break;
                }
                if Process32NextW(snap, &mut pe).is_err() {
                    break;
                }
            }
        }
        let _ = CloseHandle(snap);
        found
    }
}

pub fn is_lively_running() -> bool {
    unsafe {
        if !find_progman_mpv_pids().is_empty() {
            return true;
        }
    }
    lively_running_cached_with(
        &LIVELY_SCAN_CACHE,
        std::time::Instant::now(),
        LIVELY_SCAN_TTL,
        scan_lively_processes,
    )
}

#[cfg(test)]
mod lively_cache_tests {
    use super::lively_running_cached_with;
    use std::sync::Mutex;
    use std::time::{Duration, Instant};

    #[test]
    fn skips_redundant_process_snapshots_within_ttl() {
        let cache = Mutex::new((None, false));
        let ttl = Duration::from_millis(3000);
        let t0 = Instant::now();
        let mut calls = 0u32;

        let r1 = lively_running_cached_with(&cache, t0, ttl, || {
            calls += 1;
            true
        });
        let r2 = lively_running_cached_with(&cache, t0 + Duration::from_millis(1500), ttl, || {
            calls += 1;
            false
        });
        let r3 = lively_running_cached_with(&cache, t0 + Duration::from_millis(3000), ttl, || {
            calls += 1;
            false
        });

        assert!(r1);
        assert!(r2);
        assert!(!r3);
        assert_eq!(calls, 2);
    }
}

pub fn stop_external_lively() {
    unsafe {
        let mut extra_pids = find_progman_mpv_pids();
        let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else {
            return;
        };
        let mut pe = PROCESSENTRY32W {
            dwSize: size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        let mut to_kill = Vec::new();
        if Process32FirstW(snap, &mut pe).is_ok() {
            loop {
                let end = pe.szExeFile.iter().position(|&c| c == 0).unwrap_or(pe.szExeFile.len());
                let name = String::from_utf16_lossy(&pe.szExeFile[..end]).to_ascii_lowercase();
                if matches!(
                    name.as_str(),
                    "lively.watchdog.exe" | "lively.exe" | "lively.player.mpv.exe"
                ) {
                    to_kill.push((name, pe.th32ProcessID));
                }
                if Process32NextW(snap, &mut pe).is_err() {
                    break;
                }
            }
        }
        let _ = CloseHandle(snap);
        for p in extra_pids.drain(..) {
            to_kill.push(("mpv.exe".to_string(), p));
        }
        // Stop watchdog first so it doesn't respawn Lively or mpv
        to_kill.sort_by_key(|(n, _)| if n.contains("watchdog") { 0 } else if n.contains("lively") { 1 } else { 2 });
        for (_, pid) in to_kill {
            if let Ok(hp) = OpenProcess(PROCESS_TERMINATE, false, pid) {
                let _ = TerminateProcess(hp, 0);
                let _ = CloseHandle(hp);
            }
        }
    }
}

pub struct VideoWallpaperEngine {
    hinst: HINSTANCE,
    /// Persistent pure-GDI backdrop window behind SHELLDLL_DefView: hosts static fallback DIBs
    /// and captured last video frames when Media Foundation is paused/trimmed so the wallpaper
    /// NEVER blanks or closes when an app maximizes.
    hwnd: HWND,
    /// Non-layered sibling HWND in `Progman`/`WorkerW` dedicated to the DXGI Flip-Model SwapChain
    /// for both active video playback and zero-CPU/zero-GPU static image wallpapers.
    video_hwnd: HWND,
    backdrop_dib: Option<StaticDib>,
    static_mode: bool,
    static_load_gen: u64,
    static_load_state: StaticLoadState,
    static_req_tx: Option<mpsc::Sender<StaticLoadRequest>>,
    static_res_rx: Option<mpsc::Receiver<StaticLoadResult>>,
    pending_static_frame: Option<PendingStaticFrame>,
    static_needs_represent: bool,
    static_retry_ticks: u8,
    static_retry_timer_on: bool,
    worker_split_ready: bool,
    last_static_palette_bgra: Option<(String, Vec<u8>)>,
    cached_path_update: Option<(String, String)>,
    mf_started: bool,
    d3d_device: Option<ID3D11Device>,
    device_has_video_support: bool,
    decode_mode: VideoDecodeMode,
    adapter_name: String,
    dxgi_mgr: Option<IMFDXGIDeviceManager>,
    swap_chain: Option<IDXGISwapChain1>,
    swap_chain_flags: DXGI_SWAP_CHAIN_FLAG,
    swap_chain_buffers: u32,
    frame_latency_waitable: HANDLE,
    engine: Option<IMFMediaEngine>,
    _cb: Option<IMFMediaEngineNotify>,
    timer_on: bool,
    pacer_stop: Option<Arc<AtomicBool>>,
    pacer_interval_us: Arc<AtomicU64>,
    pump_queued: Arc<AtomicBool>,
    monitor_hz: u32,
    fps_cap: WallpaperFpsCap,
    interactive_yield: bool,
    native_video_size: (u32, u32),
    last_presented_at: Option<Instant>,
    enabled: bool,
    active_path: String,
    mute: bool,
    volume_pct: u8,
    pause_when_covered: bool,
    trim_on_pause: bool,
    paused_by_cover: bool,
    pending_cover_pause: bool,
    pause_trimmed: bool,
    pause_started_at: Option<Instant>,
    trimmed_initial: bool,
    has_presented_frame: bool,
    swap_chain_has_frame: bool,
    hosted_behind_defview: bool,
    screen_w: i32,
    screen_h: i32,
    sample_dirty: bool,
    chameleon_enabled: bool,
    initial_settle_done: bool,
    player_started_at: Option<Instant>,
    last_palette_sample: Option<Instant>,
    sample_rt_tex: Option<ID3D11Texture2D>,
    sample_st_tex: [Option<ID3D11Texture2D>; 2],
    palette_slots: AsyncPaletteSlots,
    notify_hwnd: HWND,
    notify_msg: u32,
    state_notify_hwnd: HWND,
    state_notify_msg: u32,
    frame_count: u32,
    pending_resume_sec: f64,
    backdrop_paint_count: u32,
    cached_crop: Option<(i32, i32, MFVideoNormalizedRect)>,
    end_poll_ticks: u8,
}

static mut GLOBAL_WP_PTR: *mut VideoWallpaperEngine = std::ptr::null_mut();

impl VideoWallpaperEngine {
    pub fn new(hinst: HINSTANCE) -> Self {
        Self::register_class(hinst);
        let monitor_hz = query_primary_monitor_refresh_hz();
        let initial_us = compute_target_frame_interval_us(WallpaperFpsCap::Fps60, monitor_hz);
        Self {
            hinst,
            hwnd: HWND::default(),
            video_hwnd: HWND::default(),
            backdrop_dib: None,
            static_mode: false,
            static_load_gen: 0,
            static_load_state: StaticLoadState::Idle,
            static_req_tx: None,
            static_res_rx: None,
            pending_static_frame: None,
            static_needs_represent: false,
            static_retry_ticks: 0,
            static_retry_timer_on: false,
            worker_split_ready: false,
            last_static_palette_bgra: None,
            cached_path_update: None,
            mf_started: false,
            d3d_device: None,
            device_has_video_support: false,
            decode_mode: VideoDecodeMode::Unknown,
            adapter_name: String::new(),
            dxgi_mgr: None,
            swap_chain: None,
            swap_chain_flags: DXGI_SWAP_CHAIN_FLAG(0),
            swap_chain_buffers: 2,
            frame_latency_waitable: HANDLE::default(),
            engine: None,
            _cb: None,
            timer_on: false,
            pacer_stop: None,
            pacer_interval_us: Arc::new(AtomicU64::new(initial_us)),
            pump_queued: Arc::new(AtomicBool::new(false)),
            monitor_hz,
            fps_cap: WallpaperFpsCap::Fps60,
            interactive_yield: false,
            native_video_size: (0, 0),
            last_presented_at: None,
            enabled: false,
            active_path: String::new(),
            mute: true,
            volume_pct: 0,
            pause_when_covered: true,
            trim_on_pause: false,
            paused_by_cover: false,
            pending_cover_pause: false,
            pause_trimmed: false,
            pause_started_at: None,
            trimmed_initial: false,
            has_presented_frame: false,
            swap_chain_has_frame: false,
            hosted_behind_defview: false,
            screen_w: 1920,
            screen_h: 1200,
            sample_dirty: false,
            chameleon_enabled: false,
            initial_settle_done: false,
            player_started_at: None,
            last_palette_sample: None,
            sample_rt_tex: None,
            sample_st_tex: [None, None],
            palette_slots: AsyncPaletteSlots::default(),
            notify_hwnd: HWND::default(),
            notify_msg: 0,
            state_notify_hwnd: HWND::default(),
            state_notify_msg: 0,
            frame_count: 0,
            pending_resume_sec: 0.0,
            backdrop_paint_count: 0,
            cached_crop: None,
            end_poll_ticks: 0,
        }
    }

    pub fn decode_mode(&self) -> VideoDecodeMode {
        if self.static_mode || !self.enabled {
            VideoDecodeMode::Unknown
        } else {
            self.decode_mode
        }
    }

    pub fn adapter_name(&self) -> &str {
        &self.adapter_name
    }

    pub fn set_palette_notify(&mut self, hwnd: HWND, msg: u32) {
        self.notify_hwnd = hwnd;
        self.notify_msg = msg;
    }

    pub fn set_state_notify(&mut self, hwnd: HWND, msg: u32) {
        self.state_notify_hwnd = hwnd;
        self.state_notify_msg = msg;
    }

    fn post_state_notify(&self) {
        if !self.state_notify_hwnd.0.is_null() && self.state_notify_msg != 0 {
            unsafe {
                let _ = PostMessageW(
                    Some(self.state_notify_hwnd),
                    self.state_notify_msg,
                    WPARAM(0),
                    LPARAM(0),
                );
            }
        }
    }

    pub fn take_cached_path_update(&mut self) -> Option<(String, String)> {
        self.cached_path_update.take()
    }

    pub fn is_static_loading(&self) -> bool {
        self.enabled && self.static_mode && self.static_load_state == StaticLoadState::Loading
    }

    pub fn set_chameleon_sampling(&mut self, enabled: bool) {
        if self.chameleon_enabled == enabled {
            return;
        }
        self.chameleon_enabled = enabled;
        if enabled {
            if self.last_palette_sample.is_none() {
                self.sample_dirty = true;
            }
        } else {
            self.last_palette_sample = None;
            self.clear_sample_textures();
        }
    }

    fn clear_sample_textures(&mut self) {
        self.sample_rt_tex = None;
        self.sample_st_tex = [None, None];
        self.palette_slots.reset();
    }

    pub fn is_in_mf_call(&self) -> bool {
        MF_CALL_DEPTH.load(Ordering::Acquire) > 0
    }

    pub fn resample_live_palette(&mut self) -> bool {
        if self.static_mode {
            if let Some((ref key, ref pal_bgra)) = self.last_static_palette_bgra {
                if key.eq_ignore_ascii_case(&self.active_path) && pal_bgra.len() == 64 * 36 * 4 {
                    let changed =
                        crate::theme::update_extracted_palette_from_bgra(key, pal_bgra, 64, 36);
                    self.last_palette_sample = Some(Instant::now());
                    return changed || true;
                }
            }
            return false;
        }
        // Drain any already-finished GPU staging readback first without stalling, then enqueue
        // a fresh non-blocking copy so the UI thread never blocks on GPU->CPU Map().
        let drained = self.poll_async_palette_readback();
        if let Some(eng) = self.engine.clone() {
            if self.has_presented_frame {
                let _ = self.enqueue_async_palette_sample(&eng, false);
                self.last_palette_sample = Some(Instant::now());
            }
        }
        drained
    }

    pub fn is_behind_defview(&self) -> bool {
        if !self.hosted_behind_defview || self.hwnd.0.is_null() {
            return false;
        }
        unsafe {
            if !IsWindow(Some(self.hwnd)).as_bool() {
                return false;
            }
            let Ok(parent) = GetParent(self.hwnd) else {
                return false;
            };
            if parent.0.is_null() {
                return false;
            }
            if let Ok(progman) = FindWindowW(w!("Progman"), w!("Program Manager")) {
                if parent == progman {
                    let defview = FindWindowExW(Some(progman), None, w!("SHELLDLL_DefView"), PCWSTR::null())
                        .unwrap_or_default();
                    let worker = FindWindowExW(Some(progman), None, w!("WorkerW"), PCWSTR::null())
                        .unwrap_or_default();
                    if defview.0.is_null() || worker.0.is_null() {
                        return false;
                    }
                    let top_wp = if !self.video_hwnd.0.is_null()
                        && IsWindow(Some(self.video_hwnd)).as_bool()
                        && IsWindowVisible(self.video_hwnd).as_bool()
                    {
                        self.video_hwnd
                    } else {
                        self.hwnd
                    };
                    return GetWindow(defview, GW_HWNDNEXT).unwrap_or_default() == top_wp;
                }
            }
            true
        }
    }

    /// True when the active static wallpaper surface is genuinely presented, visible, and attached
    /// behind `SHELLDLL_DefView` (no occluded/detached/missing HWND).
    fn is_static_surface_healthy(&self) -> bool {
        if !self.enabled || !self.static_mode || self.static_load_state != StaticLoadState::Ready {
            return false;
        }
        if self.static_needs_represent || !self.worker_split_ready || self.hwnd.0.is_null() {
            return false;
        }
        unsafe {
            if !IsWindow(Some(self.hwnd)).as_bool() {
                return false;
            }
            let dxgi_ok = self.swap_chain_has_frame
                && self.swap_chain.is_some()
                && !self.video_hwnd.0.is_null()
                && IsWindow(Some(self.video_hwnd)).as_bool()
                && IsWindowVisible(self.video_hwnd).as_bool();
            let dib_ok = self.backdrop_dib.is_some() && IsWindowVisible(self.hwnd).as_bool();
            (dxgi_ok || dib_ok) && self.is_behind_defview()
        }
    }

    /// Returns `(has_backdrop_dib, backdrop_hwnd_valid, backdrop_visible, backdrop_layered, video_visible, lwa_alpha, backdrop_paint_count)`.
    pub fn diagnostic_flags(&self) -> (bool, bool, bool, bool, bool, u8, u32) {
        unsafe {
            let bvalid = !self.hwnd.0.is_null() && IsWindow(Some(self.hwnd)).as_bool();
            let bvis = bvalid && IsWindowVisible(self.hwnd).as_bool();
            let blay = bvalid
                && (GetWindowLongPtrW(self.hwnd, GWL_EXSTYLE) & (WS_EX_LAYERED.0 as isize)) != 0;
            let mut lwa_alpha = 0u8;
            if blay {
                let mut key = COLORREF(0);
                let mut flags = LAYERED_WINDOW_ATTRIBUTES_FLAGS(0);
                if GetLayeredWindowAttributes(self.hwnd, Some(&mut key), Some(&mut lwa_alpha), Some(&mut flags)).is_err() {
                    lwa_alpha = 0;
                }
            }
            let vvis = !self.video_hwnd.0.is_null() && IsWindowVisible(self.video_hwnd).as_bool();
            (
                self.backdrop_dib.is_some(),
                bvalid,
                bvis,
                blay,
                vvis,
                lwa_alpha,
                self.backdrop_paint_count,
            )
        }
    }

    fn register_class(hinst: HINSTANCE) {
        unsafe {
            let mut wc = WNDCLASSEXW::default();
            wc.cbSize = size_of::<WNDCLASSEXW>() as u32;
            wc.lpfnWndProc = Some(wp_wnd_proc);
            wc.hInstance = hinst;
            wc.lpszClassName = w!("AutumnVideoWallpaperWnd");
            let _ = RegisterClassExW(&wc);

            let mut sc = WNDCLASSEXW::default();
            sc.cbSize = size_of::<WNDCLASSEXW>() as u32;
            sc.lpfnWndProc = Some(wp_surface_wnd_proc);
            sc.hInstance = hinst;
            sc.lpszClassName = w!("AutumnVideoWallpaperSurface");
            let _ = RegisterClassExW(&sc);
        }
    }

    pub fn attach(&mut self) {
        unsafe {
            GLOBAL_WP_PTR = self as *mut _;
            if !self.hwnd.0.is_null() {
                SetWindowLongPtrW(self.hwnd, GWLP_USERDATA, self as *mut _ as isize);
            }
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    pub fn active_path(&self) -> &str {
        &self.active_path
    }

    /// True only while actively decoding & presenting animated video frames.
    pub fn is_active(&self) -> bool {
        self.enabled && !self.static_mode && self.engine.is_some() && !self.paused_by_cover
    }

    /// True whenever a wallpaper surface (live video, held paused frame, or presented static image)
    /// is hosted on the desktop behind `SHELLDLL_DefView`.
    pub fn is_visible_on_desktop(&self) -> bool {
        if !self.enabled || self.hwnd.0.is_null() {
            return false;
        }
        if self.static_mode {
            return self.static_load_state == StaticLoadState::Ready
                && (self.backdrop_dib.is_some()
                    || self.has_presented_frame
                    || self.swap_chain_has_frame);
        }
        self.backdrop_dib.is_some()
            || self.has_presented_frame
            || self.swap_chain_has_frame
            || self.engine.is_some()
    }

    pub fn status_text(&self) -> &'static str {
        if !self.enabled {
            "Engine Disabled"
        } else if self.active_path.is_empty() || !Path::new(&self.active_path).exists() {
            "No Wallpaper File Found"
        } else if self.static_mode {
            match self.static_load_state {
                StaticLoadState::Loading => "Loading Static Image...",
                StaticLoadState::Failed => "Image Load Failed — Check File",
                StaticLoadState::Ready
                    if self.has_presented_frame
                        || self.swap_chain_has_frame
                        || self.backdrop_dib.is_some() =>
                {
                    if self.static_needs_represent {
                        "Active (Static Image • Syncing)"
                    } else {
                        "Active (Static Image • 0% GPU)"
                    }
                }
                StaticLoadState::Idle | StaticLoadState::Ready => "Static Surface Not Presented",
            }
        } else if self.paused_by_cover {
            if self.pause_trimmed {
                "Paused — RAM Reclaimed (SwapChain)"
            } else {
                "Paused — Holding Frame (Instant)"
            }
        } else if self.engine.is_none() {
            "Initializing..."
        } else {
            match self.decode_mode {
                VideoDecodeMode::WarpSoftware => "Playing (WARP Software Fallback — Low FPS)",
                VideoDecodeMode::SoftwareCpu => "Playing (CPU Software Decode — No DXVA)",
                VideoDecodeMode::HardwareDxva | VideoDecodeMode::Unknown => {
                    "Playing (DXVA Hardware • D3D11 Flip)"
                }
            }
        }
    }

    pub fn apply_config(&mut self, cfg: &AppConfig) {
        self.apply_config_ex(cfg, false);
    }

    fn sync_pacer_cadence(&mut self) {
        let interval_us = compute_effective_pacer_interval_us(
            self.fps_cap,
            self.monitor_hz,
            self.interactive_yield,
        );
        self.pacer_interval_us.store(interval_us, Ordering::Release);
    }

    pub fn set_interactive_yield(&mut self, yield_ui: bool) {
        if self.interactive_yield != yield_ui {
            self.interactive_yield = yield_ui;
            self.sync_pacer_cadence();
        }
    }

    /// Applies wallpaper configuration. When `force_static_refresh` is true (e.g., user explicitly
    /// clicked Apply), re-verifies or re-presents the static image even if the path did not change.
    pub fn apply_config_ex(&mut self, cfg: &AppConfig, force_static_refresh: bool) {
        let want_enabled = cfg.wallpaper_enabled
            && !cfg.wallpaper_path.is_empty()
            && Path::new(&cfg.wallpaper_path).exists();
        let path_changed = !self.active_path.eq_ignore_ascii_case(&cfg.wallpaper_path);
        let was_enabled = self.enabled;
        let prev_pause_max = self.pause_when_covered;
        self.enabled = want_enabled;
        self.mute = cfg.wallpaper_mute;
        self.volume_pct = cfg.wallpaper_volume_pct;
        self.pause_when_covered = cfg.wallpaper_pause_maximized;
        self.trim_on_pause = cfg.wallpaper_trim_memory;
        let prev_fps_cap = self.fps_cap;
        self.fps_cap = cfg.wallpaper_fps_cap;
        if prev_fps_cap != self.fps_cap {
            self.last_presented_at = None;
        }
        self.sync_pacer_cadence();

        if !want_enabled {
            self.pending_resume_sec = 0.0;
            self.stop_player();
            self.active_path = cfg.wallpaper_path.clone();
            return;
        }

        let want_static = is_static_image_wallpaper_path(&cfg.wallpaper_path);
        if want_static {
            let needs_reload = path_changed
                || !was_enabled
                || !self.static_mode
                || self.static_load_state == StaticLoadState::Failed
                || self.static_load_state == StaticLoadState::Idle
                || (force_static_refresh && !self.is_static_surface_healthy())
                || (!self.swap_chain_has_frame
                    && self.backdrop_dib.is_none()
                    && self.static_load_state != StaticLoadState::Loading);
            if needs_reload {
                self.pending_resume_sec = 0.0;
                self.active_path = cfg.wallpaper_path.clone();
                self.start_static_image(&cfg.wallpaper_path);
            } else if force_static_refresh {
                // Already healthy on the same photo: ensure desktop z-order & visibility stay locked.
                self.place_behind_icons();
                if self.static_needs_represent {
                    self.retry_static_represent();
                }
            }
            return;
        }

        if path_changed
            || !was_enabled
            || self.static_mode
            || (self.engine.is_none() && (!self.paused_by_cover || !self.pause_trimmed))
        {
            if path_changed {
                self.pending_resume_sec = 0.0;
                self.backdrop_dib = None;
            }
            self.static_load_gen = self.static_load_gen.wrapping_add(1);
            self.static_load_state = StaticLoadState::Idle;
            self.pending_static_frame = None;
            self.static_needs_represent = false;
            self.set_static_retry_timer(false);
            self.paused_by_cover = false;
            self.pause_trimmed = false;
            self.pending_cover_pause = false;
            self.pause_started_at = None;
            self.static_mode = false;
            self.active_path = cfg.wallpaper_path.clone();
            self.start_player(&cfg.wallpaper_path);
        } else {
            if prev_pause_max && !self.pause_when_covered && self.paused_by_cover {
                self.update_pause_for_cover(false);
            }
            if let Some(ref eng) = self.engine {
                let _mf = MfCallGuard::enter();
                unsafe {
                    let _ = eng.SetMuted(self.mute || self.volume_pct == 0);
                    let _ = eng.SetVolume((self.volume_pct as f64 / 100.0).clamp(0.0, 1.0));
                }
            }
        }
    }

    /// Boot-time wallpaper setup:
    /// - Static images decode off-thread and present onto the desktop Flip-Model surface (0% GPU).
    /// - Videos covered by a maximized app with `wallpaper_pause_maximized` show a lightweight
    ///   poster frame on `backdrop_hwnd` while deferring heavyweight MediaFoundation/D3D11 decode
    ///   allocation until the desktop is uncovered — zero blanking AND low boot RAM!
    pub fn apply_config_for_boot(&mut self, cfg: &AppConfig, covered_by_maximized: bool) {
        let want_enabled =
            cfg.wallpaper_enabled && !cfg.wallpaper_path.is_empty() && Path::new(&cfg.wallpaper_path).exists();
        if want_enabled
            && !is_static_image_wallpaper_path(&cfg.wallpaper_path)
            && cfg.wallpaper_pause_maximized
            && cfg.wallpaper_trim_memory
            && covered_by_maximized
            && self.engine.is_none()
        {
            self.enabled = true;
            self.static_mode = false;
            self.mute = cfg.wallpaper_mute;
            self.volume_pct = cfg.wallpaper_volume_pct;
            self.fps_cap = cfg.wallpaper_fps_cap;
            self.sync_pacer_cadence();
            self.pause_when_covered = true;
            self.trim_on_pause = cfg.wallpaper_trim_memory;
            self.paused_by_cover = true;
            self.pause_trimmed = true;
            self.active_path = cfg.wallpaper_path.clone();
            self.show_video_poster_backdrop(&cfg.wallpaper_path);
            return;
        }
        self.apply_config(cfg);
        if !self.static_mode {
            self.update_pause_for_cover(covered_by_maximized && cfg.wallpaper_pause_maximized);
        }
    }

    fn show_video_poster_backdrop(&mut self, video_path: &str) {
        self.ensure_window();
        if !self.swap_chain_has_frame && self.backdrop_dib.is_none() {
            let (sw, sh) = self.swap_dims();
            if let Some(bgra) = decode_wallpaper_thumbnail_bgra(video_path, sw, sh) {
                if is_non_blank_bgra(&bgra, sw, sh) {
                    if !matches!(
                        self.present_bgra_to_swap_chain(&bgra, sw, sh),
                        PresentOutcome::Failed
                    ) {
                        self.backdrop_dib = None;
                        self.trim_gpu_memory();
                        return;
                    }
                    self.destroy_video_surface();
                    self.backdrop_dib = StaticDib::from_bgra_top_down(sw, sh, &bgra);
                }
            }
        }
        self.place_behind_icons();
        self.paint_backdrop_now();
    }

    fn paint_backdrop_now(&mut self) {
        if self.hwnd.0.is_null() {
            return;
        }
        unsafe {
            if self.backdrop_dib.is_some() {
                ensure_backdrop_layered_opaque(self.hwnd);
                let _ = ShowWindow(self.hwnd, SW_SHOW);
                let _ = InvalidateRect(Some(self.hwnd), None, false);
                let _ = RedrawWindow(
                    Some(self.hwnd),
                    None,
                    None,
                    RDW_INVALIDATE | RDW_UPDATENOW | RDW_ERASE | RDW_FRAME,
                );
                let _ = UpdateWindow(self.hwnd);
            } else if IsWindowVisible(self.hwnd).as_bool() {
                let _ = ShowWindow(self.hwnd, SW_HIDE);
            }
        }
    }

    fn drop_swap_chain(&mut self) {
        self.swap_chain = None;
        self.swap_chain_has_frame = false;
        self.swap_chain_flags = DXGI_SWAP_CHAIN_FLAG(0);
        self.swap_chain_buffers = 2;
        if !self.frame_latency_waitable.0.is_null() {
            unsafe {
                let _ = CloseHandle(self.frame_latency_waitable);
            }
            self.frame_latency_waitable = HANDLE::default();
        }
    }

    fn destroy_video_surface(&mut self) {
        self.drop_swap_chain();
        if !self.video_hwnd.0.is_null() {
            unsafe {
                let _ = ShowWindow(self.video_hwnd, SW_HIDE);
                if !self.is_in_mf_call() {
                    if let Ok(p) = GetParent(self.video_hwnd) {
                        if !p.0.is_null() {
                            let _ = SetParent(self.video_hwnd, None);
                        }
                    }
                    let _ = DestroyWindow(self.video_hwnd);
                }
            }
            self.video_hwnd = HWND::default();
        }
    }

    fn set_static_retry_timer(&mut self, run: bool) {
        if self.hwnd.0.is_null() {
            self.static_retry_timer_on = false;
            return;
        }
        unsafe {
            if run && !self.static_retry_timer_on {
                let _ = SetTimer(Some(self.hwnd), TIMER_WP_STATIC_RETRY, 350, None);
                self.static_retry_timer_on = true;
            } else if !run && self.static_retry_timer_on {
                let _ = KillTimer(Some(self.hwnd), TIMER_WP_STATIC_RETRY);
                self.static_retry_timer_on = false;
            }
        }
    }

    fn ensure_static_worker(&mut self) {
        if self.static_req_tx.is_some() {
            return;
        }
        let (req_tx, req_rx) = mpsc::channel::<StaticLoadRequest>();
        let (res_tx, res_rx) = mpsc::channel::<StaticLoadResult>();
        let _ = std::thread::Builder::new()
            .name("autumn-static-wp".to_string())
            .spawn(move || {
                while let Ok(mut req) = req_rx.recv() {
                    // Coalesce rapid wallpaper clicks: always decode only the latest request.
                    while let Ok(newer) = req_rx.try_recv() {
                        req = newer;
                    }
                    // Decode first so corrupt/unreadable files are never copied into the local cache.
                    let frame = load_static_image_bgra(&req.path, req.target_w, req.target_h)
                        .map(|(bgra, dw, dh)| {
                            let pal_bgra = downsample_bgra_for_palette(&bgra, dw, dh);
                            (bgra, dw, dh, pal_bgra)
                        });
                    let cached_path = if frame.is_some() && req.cache_locally {
                        cache_wallpaper_locally(&req.path)
                    } else {
                        req.path.clone()
                    };
                    let wp_hwnd_raw = req.wp_hwnd_raw;
                    let notify_hwnd_raw = req.notify_hwnd_raw;
                    let notify_msg = req.notify_msg;
                    let send_res = res_tx.send(StaticLoadResult {
                        gen: req.gen,
                        req_path: req.path,
                        cached_path,
                        frame,
                    });
                    if send_res.is_err() {
                        break;
                    }
                    unsafe {
                        if wp_hwnd_raw != 0 {
                            let _ = PostMessageW(
                                Some(HWND(wp_hwnd_raw as *mut c_void)),
                                WM_WP_STATIC_READY,
                                WPARAM(0),
                                LPARAM(0),
                            );
                        } else if notify_hwnd_raw != 0 && notify_msg != 0 {
                            let _ = PostMessageW(
                                Some(HWND(notify_hwnd_raw as *mut c_void)),
                                notify_msg,
                                WPARAM(0),
                                LPARAM(0),
                            );
                        }
                    }
                }
            });
        self.static_req_tx = Some(req_tx);
        self.static_res_rx = Some(res_rx);
    }

    fn start_static_image(&mut self, img_path: &str) {
        let had_video = self.engine.is_some() || self.dxgi_mgr.is_some() || self.mf_started;
        self.release_video_decoder_only();
        if had_video {
            // Switching Video -> Static Image: drop the Media Foundation-shared swapchain & D3D11
            // device so DXVA driver heaps are reclaimed immediately with zero async lock contention.
            self.drop_swap_chain();
            self.trim_gpu_memory();
            self.d3d_device = None;
            self.device_has_video_support = false;
            self.decode_mode = VideoDecodeMode::Unknown;
        }
        self.static_mode = true;
        self.paused_by_cover = false;
        self.pause_trimmed = false;
        self.pending_cover_pause = false;
        self.pause_started_at = None;
        self.pending_static_frame = None;
        self.static_needs_represent = false;
        self.static_retry_ticks = 0;
        self.set_static_retry_timer(false);
        self.ensure_window();

        self.static_load_gen = self.static_load_gen.wrapping_add(1);
        self.static_load_state = StaticLoadState::Loading;
        let (sw, sh) = self.swap_dims();
        self.ensure_static_worker();
        let req = StaticLoadRequest {
            gen: self.static_load_gen,
            path: img_path.to_string(),
            target_w: sw,
            target_h: sh,
            wp_hwnd_raw: self.hwnd.0 as usize,
            notify_hwnd_raw: self.state_notify_hwnd.0 as usize,
            notify_msg: self.state_notify_msg,
            cache_locally: true,
        };
        if let Some(ref tx) = self.static_req_tx {
            if tx.send(req).is_err() {
                // Worker exited unexpectedly; respawn once and retry.
                self.static_req_tx = None;
                self.static_res_rx = None;
                self.ensure_static_worker();
                if let Some(ref tx2) = self.static_req_tx {
                    let retry_req = StaticLoadRequest {
                        gen: self.static_load_gen,
                        path: img_path.to_string(),
                        target_w: sw,
                        target_h: sh,
                        wp_hwnd_raw: self.hwnd.0 as usize,
                        notify_hwnd_raw: self.state_notify_hwnd.0 as usize,
                        notify_msg: self.state_notify_msg,
                        cache_locally: true,
                    };
                    if tx2.send(retry_req).is_err() {
                        self.static_load_state = StaticLoadState::Failed;
                        self.post_state_notify();
                    }
                }
            }
        }
    }

    fn present_static_bgra_frame(&mut self, bgra: Vec<u8>, dw: u32, dh: u32) {
        self.ensure_window();
        match self.present_bgra_to_swap_chain(&bgra, dw, dh) {
            PresentOutcome::Presented => {
                self.backdrop_dib = None;
                self.paint_backdrop_now();
                self.static_load_state = StaticLoadState::Ready;
                if !self.worker_split_ready {
                    // Progman hasn't finished splitting WorkerW yet: keep pixels until WorkerW
                    // appears so we can re-seat & re-present cleanly behind layered SHELLDLL_DefView.
                    self.static_needs_represent = true;
                    self.pending_static_frame = Some(PendingStaticFrame { bgra, w: dw, h: dh });
                    self.static_retry_ticks = 0;
                    self.set_static_retry_timer(true);
                } else {
                    self.static_needs_represent = false;
                    self.pending_static_frame = None;
                    self.set_static_retry_timer(false);
                    self.trim_gpu_memory();
                }
            }
            PresentOutcome::Occluded => {
                // Desktop is currently covered (e.g. maximized app) so DXGI Flip-Model returned
                // DXGI_STATUS_OCCLUDED: hold frame briefly and commit on first unoccluded tick.
                self.backdrop_dib = None;
                self.paint_backdrop_now();
                self.static_load_state = StaticLoadState::Ready;
                self.static_needs_represent = true;
                self.pending_static_frame = Some(PendingStaticFrame { bgra, w: dw, h: dh });
                self.static_retry_ticks = 0;
                self.set_static_retry_timer(true);
            }
            PresentOutcome::Failed => {
                // DXGI unavailable/failed: tear down `video_hwnd` completely so it can never cover
                // the layered GDI fallback surface, then paint `StaticDib` onto `self.hwnd`.
                self.destroy_video_surface();
                self.backdrop_dib = StaticDib::from_bgra_top_down(dw, dh, &bgra);
                self.has_presented_frame = self.backdrop_dib.is_some();
                self.static_needs_represent = false;
                self.pending_static_frame = None;
                self.set_static_retry_timer(false);
                if self.backdrop_dib.is_some() {
                    self.static_load_state = StaticLoadState::Ready;
                    self.place_behind_icons();
                    self.paint_backdrop_now();
                } else {
                    self.static_load_state = StaticLoadState::Failed;
                }
            }
        }
    }

    /// Drains any completed background static image decode and presents it on the UI thread.
    /// Returns `true` when a matching load result was processed.
    pub fn drain_static_load_results(&mut self) -> bool {
        let mut latest: Option<StaticLoadResult> = None;
        if let Some(ref rx) = self.static_res_rx {
            while let Ok(res) = rx.try_recv() {
                if res.gen == self.static_load_gen {
                    latest = Some(res);
                }
            }
        }
        let Some(res) = latest else {
            return false;
        };
        if !self.enabled || !self.static_mode {
            return false;
        }
        if !res.cached_path.is_empty()
            && !res.cached_path.eq_ignore_ascii_case(&res.req_path)
            && Path::new(&res.cached_path).is_file()
        {
            self.active_path = res.cached_path.clone();
            self.cached_path_update = Some((res.req_path.clone(), res.cached_path.clone()));
        }
        match res.frame {
            Some((bgra, dw, dh, pal_bgra)) => {
                self.present_static_bgra_frame(bgra, dw, dh);
                let pal_key = self.active_path.clone();
                self.last_static_palette_bgra = Some((pal_key.clone(), pal_bgra.clone()));
                if self.chameleon_enabled {
                    let changed =
                        crate::theme::update_extracted_palette_from_bgra(&pal_key, &pal_bgra, 64, 36);
                    self.last_palette_sample = Some(Instant::now());
                    if changed && !self.notify_hwnd.0.is_null() && self.notify_msg != 0 {
                        unsafe {
                            let _ = PostMessageW(
                                Some(self.notify_hwnd),
                                self.notify_msg,
                                WPARAM(0),
                                LPARAM(0),
                            );
                        }
                    }
                }
            }
            None => {
                self.static_load_state = StaticLoadState::Failed;
                self.pending_static_frame = None;
                self.static_needs_represent = false;
                self.set_static_retry_timer(false);
                self.backdrop_dib = None;
                self.has_presented_frame = false;
                self.destroy_video_surface();
                self.paint_backdrop_now();
            }
        }
        self.post_state_notify();
        true
    }

    fn retry_static_represent(&mut self) {
        if !self.enabled || !self.static_mode || WP_OS_EXIT.load(Ordering::Acquire) {
            self.set_static_retry_timer(false);
            self.pending_static_frame = None;
            self.static_needs_represent = false;
            return;
        }
        self.static_retry_ticks = self.static_retry_ticks.saturating_add(1);
        if !self.worker_split_ready {
            self.place_behind_icons();
        }
        // Lightweight occlusion probe before uploading anything to GPU.
        if self.static_needs_represent && self.worker_split_ready {
            if let Some(ref sc) = self.swap_chain {
                let hr = unsafe { sc.Present(0, DXGI_PRESENT_TEST) };
                if hr.0 == DXGI_STATUS_OCCLUDED_HR {
                    if self.static_retry_ticks >= 80 {
                        // Desktop remained covered for ~28s: pause timer until desktop uncovers.
                        self.set_static_retry_timer(false);
                    }
                    return;
                }
            }
        }
        if let Some(frame) = self.pending_static_frame.take() {
            match self.present_bgra_to_swap_chain(&frame.bgra, frame.w, frame.h) {
                PresentOutcome::Presented if self.worker_split_ready => {
                    self.static_needs_represent = false;
                    self.set_static_retry_timer(false);
                    self.trim_gpu_memory();
                    self.post_state_notify();
                }
                PresentOutcome::Presented | PresentOutcome::Occluded => {
                    self.pending_static_frame = Some(frame);
                    if self.static_retry_ticks >= 80 {
                        self.set_static_retry_timer(false);
                    }
                }
                PresentOutcome::Failed => {
                    self.destroy_video_surface();
                    self.backdrop_dib = StaticDib::from_bgra_top_down(frame.w, frame.h, &frame.bgra);
                    self.has_presented_frame = self.backdrop_dib.is_some();
                    self.static_needs_represent = false;
                    self.set_static_retry_timer(false);
                    if self.backdrop_dib.is_some() {
                        self.static_load_state = StaticLoadState::Ready;
                        self.place_behind_icons();
                        self.paint_backdrop_now();
                    } else {
                        self.static_load_state = StaticLoadState::Failed;
                    }
                    self.post_state_notify();
                }
            }
        } else if self.static_needs_represent && !self.active_path.is_empty() {
            let path = self.active_path.clone();
            self.start_static_image(&path);
        } else {
            self.set_static_retry_timer(false);
        }
    }

    /// Re-attaches and re-presents after Windows Explorer (`TaskbarCreated`) restarts `Progman`.
    pub fn on_explorer_restarted(&mut self) {
        if !self.enabled {
            return;
        }
        if self.static_mode {
            // Flip-Model swapchain parented to the old Progman cannot survive Explorer restart:
            // recreate swapchain and re-present asynchronously against the fresh Progman/WorkerW.
            self.destroy_video_surface();
            let path = self.active_path.clone();
            if !path.is_empty() && Path::new(&path).is_file() {
                self.start_static_image(&path);
            }
        } else {
            self.place_behind_icons();
            if self.engine.is_some() && !self.paused_by_cover {
                self.pump_frame(true);
            }
        }
    }

    fn ensure_window(&mut self) {
        let mut mon = RECT {
            left: 0,
            top: 0,
            right: 1920,
            bottom: 1200,
        };
        unsafe {
            let hmon = MonitorFromPoint(windows::Win32::Foundation::POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY);
            let mut mi = MONITORINFO {
                cbSize: size_of::<MONITORINFO>() as u32,
                ..Default::default()
            };
            if GetMonitorInfoW(hmon, &mut mi).as_bool() {
                mon = mi.rcMonitor;
            }
        }
        self.screen_w = mon.right - mon.left;
        self.screen_h = mon.bottom - mon.top;

        unsafe {
            if self.hwnd.0.is_null() || !IsWindow(Some(self.hwnd)).as_bool() {
                self.hwnd = HWND::default();
                // Create `backdrop_hwnd` unowned (`WS_POPUP`) with `WS_EX_LAYERED` first so Win32
                // never rejects `CreateWindowExW(WS_CHILD | WS_EX_LAYERED)` across process
                // boundaries (`explorer.exe`'s `Progman`), then reparent in `place_behind_icons()`.
                if let Ok(hwnd) = CreateWindowExW(
                    WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW,
                    w!("AutumnVideoWallpaperWnd"),
                    w!("Autumn Video Wallpaper"),
                    WS_POPUP | WS_CLIPSIBLINGS,
                    0,
                    0,
                    self.screen_w,
                    self.screen_h,
                    None,
                    None,
                    Some(self.hinst),
                    None,
                ) {
                    self.hwnd = hwnd;
                    SetWindowLongPtrW(hwnd, GWLP_USERDATA, self as *mut _ as isize);
                    ensure_backdrop_layered_opaque(hwnd);
                    GLOBAL_WP_PTR = self as *mut _;
                }
            }
            self.place_behind_icons();
        }
    }

    pub fn place_behind_icons(&mut self) {
        let _ = self.place_behind_icons_tracked();
    }

    /// Places `video_hwnd` and `hwnd` behind `SHELLDLL_DefView` (above `WorkerW`).
    /// Returns `true` when `video_hwnd` was reparented to a different host HWND.
    fn place_behind_icons_tracked(&mut self) -> bool {
        if self.hwnd.0.is_null() || !self.enabled {
            return false;
        }
        let mut need_repump = false;
        let mut video_reparented = false;
        unsafe {
            let placement = find_worker_behind_defview();
            self.hosted_behind_defview = !matches!(placement, HostPlacement::DesktopFallback);
            self.worker_split_ready = match placement {
                HostPlacement::ProgmanChild { worker_split_ready, .. } => worker_split_ready,
                HostPlacement::WorkerWChild(_) => true,
                HostPlacement::DesktopFallback => false,
            };
            let show_backdrop = self.backdrop_dib.is_some();
            let has_video =
                !self.video_hwnd.0.is_null() && IsWindow(Some(self.video_hwnd)).as_bool();
            let show_video = has_video && (self.has_presented_frame || self.swap_chain_has_frame);

            let mut bd_child_style = (WS_CHILD.0 | WS_CLIPSIBLINGS.0) as isize;
            if show_backdrop {
                bd_child_style |= WS_VISIBLE.0 as isize;
            }
            let mut vid_child_style = (WS_CHILD.0 | WS_CLIPSIBLINGS.0 | WS_CLIPCHILDREN.0) as isize;
            if show_video {
                vid_child_style |= WS_VISIBLE.0 as isize;
            }
            let cur_parent = GetParent(self.hwnd).unwrap_or_default();
            match placement {
                HostPlacement::ProgmanChild { progman, after_defview, .. } => {
                    // 1. Place non-layered DXGI sibling `video_hwnd` directly after SHELLDLL_DefView
                    if has_video {
                        let cur_v_style = GetWindowLongPtrW(self.video_hwnd, GWL_STYLE);
                        let mut vflags = SWP_NOACTIVATE;
                        if (cur_v_style & (WS_CHILD.0 as isize)) == 0 {
                            let _ = SetWindowLongPtrW(self.video_hwnd, GWL_STYLE, vid_child_style);
                            vflags |= SWP_FRAMECHANGED;
                        }
                        if GetParent(self.video_hwnd).unwrap_or_default() != progman {
                            let _ = SetParent(self.video_hwnd, Some(progman));
                            need_repump = true;
                            video_reparented = true;
                        }
                        if show_video {
                            vflags |= SWP_SHOWWINDOW;
                        }
                        if GetWindow(after_defview, GW_HWNDNEXT).unwrap_or_default() != self.video_hwnd {
                            need_repump = true;
                        }
                        let _ = SetWindowPos(
                            self.video_hwnd,
                            Some(after_defview),
                            0,
                            0,
                            self.screen_w,
                            self.screen_h,
                            vflags,
                        );
                    }
                    // 2. Place WS_EX_LAYERED GDI `backdrop_hwnd` directly after `video_hwnd` (or `after_defview`
                    // when no video surface is active), strictly in front of Progman's child WorkerW.
                    let mut style_flags = SWP_NOACTIVATE;
                    if cur_parent != progman {
                        let _ = SetParent(self.hwnd, Some(progman));
                        need_repump = true;
                    }
                    let cur_style = GetWindowLongPtrW(self.hwnd, GWL_STYLE);
                    if (cur_style & (WS_CHILD.0 as isize)) == 0 {
                        let _ = SetWindowLongPtrW(self.hwnd, GWL_STYLE, bd_child_style);
                        style_flags |= SWP_FRAMECHANGED;
                    }
                    ensure_backdrop_layered_opaque(self.hwnd);
                    if show_backdrop {
                        style_flags |= SWP_SHOWWINDOW;
                    }
                    let insert_after = if has_video { self.video_hwnd } else { after_defview };
                    let _ = SetWindowPos(
                        self.hwnd,
                        Some(insert_after),
                        0,
                        0,
                        self.screen_w,
                        self.screen_h,
                        style_flags,
                    );
                }
                HostPlacement::WorkerWChild(worker) => {
                    if has_video {
                        let cur_v_style = GetWindowLongPtrW(self.video_hwnd, GWL_STYLE);
                        let mut vflags = SWP_NOACTIVATE;
                        if (cur_v_style & (WS_CHILD.0 as isize)) == 0 {
                            let _ = SetWindowLongPtrW(self.video_hwnd, GWL_STYLE, vid_child_style);
                            vflags |= SWP_FRAMECHANGED;
                        }
                        if GetParent(self.video_hwnd).unwrap_or_default() != worker {
                            let _ = SetParent(self.video_hwnd, Some(worker));
                            need_repump = true;
                            video_reparented = true;
                        }
                        if show_video {
                            vflags |= SWP_SHOWWINDOW;
                        }
                        let _ = SetWindowPos(
                            self.video_hwnd,
                            Some(HWND_TOP),
                            0,
                            0,
                            self.screen_w,
                            self.screen_h,
                            vflags,
                        );
                    }
                    let mut style_flags = SWP_NOACTIVATE;
                    if cur_parent != worker {
                        let _ = SetParent(self.hwnd, Some(worker));
                        need_repump = true;
                    }
                    let cur_style = GetWindowLongPtrW(self.hwnd, GWL_STYLE);
                    if (cur_style & (WS_CHILD.0 as isize)) == 0 {
                        let _ = SetWindowLongPtrW(self.hwnd, GWL_STYLE, bd_child_style);
                        style_flags |= SWP_FRAMECHANGED;
                    }
                    ensure_backdrop_layered_opaque(self.hwnd);
                    if show_backdrop {
                        style_flags |= SWP_SHOWWINDOW;
                    }
                    let insert_after = if has_video { self.video_hwnd } else { HWND_TOP };
                    let _ = SetWindowPos(
                        self.hwnd,
                        Some(insert_after),
                        0,
                        0,
                        self.screen_w,
                        self.screen_h,
                        style_flags,
                    );
                }
                HostPlacement::DesktopFallback => {
                    if has_video {
                        let mut vflags = SWP_NOACTIVATE;
                        if show_video {
                            vflags |= SWP_SHOWWINDOW;
                        }
                        let _ = SetWindowPos(
                            self.video_hwnd,
                            Some(HWND_BOTTOM),
                            0,
                            0,
                            self.screen_w,
                            self.screen_h,
                            vflags,
                        );
                    }
                    ensure_backdrop_layered_opaque(self.hwnd);
                    let mut flags = SWP_NOACTIVATE;
                    if show_backdrop {
                        flags |= SWP_SHOWWINDOW;
                    }
                    let _ = SetWindowPos(
                        self.hwnd,
                        Some(HWND_BOTTOM),
                        0,
                        0,
                        self.screen_w,
                        self.screen_h,
                        flags,
                    );
                }
            }
            if show_backdrop && !IsWindowVisible(self.hwnd).as_bool() {
                let _ = ShowWindow(self.hwnd, SW_SHOW);
            } else if !show_backdrop && IsWindowVisible(self.hwnd).as_bool() {
                let _ = ShowWindow(self.hwnd, SW_HIDE);
            }
            if show_video && !IsWindowVisible(self.video_hwnd).as_bool() {
                let _ = ShowWindow(self.video_hwnd, SW_SHOW);
            }
        }
        if need_repump && !self.static_mode && self.engine.is_some() && !self.paused_by_cover {
            self.pump_frame(true);
        }
        video_reparented
    }

    fn ensure_video_child_window(&mut self) -> bool {
        if self.hwnd.0.is_null() {
            return false;
        }
        unsafe {
            if !self.video_hwnd.0.is_null() && IsWindow(Some(self.video_hwnd)).as_bool() {
                if self.place_behind_icons_tracked() {
                    // SwapChain HWND was reparented (e.g. Explorer restart): recreate swapchain cleanly.
                    self.drop_swap_chain();
                }
                return true;
            }
            self.video_hwnd = HWND::default();
            // Create `video_hwnd` as a non-layered SIBLING in the same desktop host (`Progman` or
            // `WorkerW`) as `backdrop_hwnd` so DXGI Flip-Model SwapChain never lives under a
            // `WS_EX_LAYERED` parent and never clips `backdrop_hwnd` via `WS_CLIPCHILDREN`.
            let placement = find_worker_behind_defview();
            let (parent_opt, style) = match placement {
                HostPlacement::ProgmanChild { progman, .. } => {
                    (Some(progman), WS_CHILD | WS_CLIPSIBLINGS | WS_CLIPCHILDREN)
                }
                HostPlacement::WorkerWChild(worker) => {
                    (Some(worker), WS_CHILD | WS_CLIPSIBLINGS | WS_CLIPCHILDREN)
                }
                HostPlacement::DesktopFallback => {
                    (None, WS_POPUP | WS_CLIPSIBLINGS | WS_CLIPCHILDREN)
                }
            };
            if let Ok(vid) = CreateWindowExW(
                WS_EX_TRANSPARENT | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW,
                w!("AutumnVideoWallpaperSurface"),
                w!("Autumn Video Surface"),
                style,
                0,
                0,
                self.screen_w,
                self.screen_h,
                parent_opt,
                None,
                Some(self.hinst),
                None,
            ) {
                self.video_hwnd = vid;
                self.place_behind_icons();
                true
            } else {
                false
            }
        }
    }

    fn create_swap_chain_for_size(&mut self, sw: u32, sh: u32) -> bool {
        if self.video_hwnd.0.is_null() {
            return false;
        }
        self.drop_swap_chain();
        unsafe {
            if let Some(ref dev) = self.d3d_device {
                if let Ok(dxgi_dev) = dev.cast::<IDXGIDevice>() {
                    if let Ok(adapter) = dxgi_dev.GetAdapter() {
                        if let Ok(factory) = adapter.GetParent::<IDXGIFactory2>() {
                            // Prefer triple-buffered FLIP_DISCARD with a frame-latency waitable
                            // object (2-frame max latency) for tear-free, stutter-free VSync
                            // video playback without ever dropping frames on minor phase jitter.
                            let candidates = [
                                (
                                    3u32,
                                    DXGI_SWAP_EFFECT_FLIP_DISCARD,
                                    DXGI_SWAP_CHAIN_FLAG_FRAME_LATENCY_WAITABLE_OBJECT,
                                ),
                                (3u32, DXGI_SWAP_EFFECT_FLIP_DISCARD, DXGI_SWAP_CHAIN_FLAG(0)),
                                (
                                    2u32,
                                    DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL,
                                    DXGI_SWAP_CHAIN_FLAG(0),
                                ),
                            ];
                            for (buf_count, swap_effect, sc_flag) in candidates {
                                let desc = DXGI_SWAP_CHAIN_DESC1 {
                                    Width: sw.max(2),
                                    Height: sh.max(2),
                                    Format: DXGI_FORMAT_B8G8R8A8_UNORM,
                                    Stereo: BOOL(0),
                                    SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
                                    BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT
                                        | DXGI_USAGE_BACK_BUFFER,
                                    BufferCount: buf_count,
                                    Scaling: DXGI_SCALING_STRETCH,
                                    SwapEffect: swap_effect,
                                    AlphaMode: DXGI_ALPHA_MODE_UNSPECIFIED,
                                    Flags: sc_flag.0 as u32,
                                };
                                if let Ok(sc) = factory.CreateSwapChainForHwnd(
                                    dev,
                                    self.video_hwnd,
                                    &desc,
                                    None,
                                    None,
                                ) {
                                    let _ = factory.MakeWindowAssociation(
                                        self.video_hwnd,
                                        DXGI_MWA_NO_ALT_ENTER,
                                    );
                                    if sc_flag == DXGI_SWAP_CHAIN_FLAG_FRAME_LATENCY_WAITABLE_OBJECT
                                    {
                                        if let Ok(sc2) = sc.cast::<IDXGISwapChain2>() {
                                            let _ = sc2.SetMaximumFrameLatency(2);
                                            let waitable = sc2.GetFrameLatencyWaitableObject();
                                            if !waitable.is_invalid() && !waitable.0.is_null() {
                                                self.frame_latency_waitable = waitable;
                                            }
                                        }
                                    }
                                    self.swap_chain_flags = sc_flag;
                                    self.swap_chain_buffers = buf_count;
                                    self.swap_chain = Some(sc);
                                    return true;
                                }
                            }
                        }
                    }
                }
            }
        }
        false
    }

    fn ensure_d3d_device_and_swap_chain(&mut self) -> bool {
        if self.hwnd.0.is_null() || !self.ensure_video_child_window() {
            return false;
        }
        unsafe {
            // If a static image created a non-VIDEO_SUPPORT device earlier and we are now starting
            // live video playback, recreate the device with VIDEO_SUPPORT so DXVA hardware decode works.
            let need_video_upgrade = !self.static_mode
                && self.d3d_device.is_some()
                && !self.device_has_video_support
                && self.decode_mode == VideoDecodeMode::Unknown;
            let device_lost = self
                .d3d_device
                .as_ref()
                .is_some_and(|dev| dev.GetDeviceRemovedReason().is_err());
            if device_lost || need_video_upgrade {
                self.drop_swap_chain();
                self.dxgi_mgr = None;
                self.clear_sample_textures();
                self.d3d_device = None;
                self.device_has_video_support = false;
                self.decode_mode = VideoDecodeMode::Unknown;
            }

            if self.d3d_device.is_none() {
                let mut dev_opt: Option<ID3D11Device> = None;
                let mut chosen_adapter: Option<IDXGIAdapter> = None;
                self.adapter_name.clear();
                if let Ok(fac) = CreateDXGIFactory1::<IDXGIFactory1>() {
                    let mut idx = 0u32;
                    while let Ok(ad1) = fac.EnumAdapters1(idx) {
                        if let Ok(desc) = ad1.GetDesc1() {
                            let is_sw = (desc.Flags & (DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32)) != 0;
                            if !is_sw {
                                let end = desc
                                    .Description
                                    .iter()
                                    .position(|&c| c == 0)
                                    .unwrap_or(desc.Description.len());
                                self.adapter_name =
                                    String::from_utf16_lossy(&desc.Description[..end]);
                                chosen_adapter = ad1.cast::<IDXGIAdapter>().ok();
                                break;
                            }
                        }
                        idx += 1;
                        if idx >= 8 {
                            break;
                        }
                    }
                }

                let primary_flags = if self.static_mode {
                    D3D11_CREATE_DEVICE_BGRA_SUPPORT
                } else {
                    D3D11_CREATE_DEVICE_BGRA_SUPPORT | D3D11_CREATE_DEVICE_VIDEO_SUPPORT
                };
                let (drv_type, adapter_ref) = if let Some(ref ad) = chosen_adapter {
                    (D3D_DRIVER_TYPE_UNKNOWN, Some(ad))
                } else {
                    (D3D_DRIVER_TYPE_HARDWARE, None)
                };

                let mut used_video_flags = primary_flags
                    == (D3D11_CREATE_DEVICE_BGRA_SUPPORT | D3D11_CREATE_DEVICE_VIDEO_SUPPORT);
                let mut used_warp = false;
                let mut res = D3D11CreateDevice(
                    adapter_ref,
                    drv_type,
                    HMODULE::default(),
                    primary_flags,
                    None,
                    D3D11_SDK_VERSION,
                    Some(&mut dev_opt),
                    None,
                    None,
                );
                if res.is_err() && used_video_flags {
                    used_video_flags = false;
                    res = D3D11CreateDevice(
                        adapter_ref,
                        drv_type,
                        HMODULE::default(),
                        D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                        None,
                        D3D11_SDK_VERSION,
                        Some(&mut dev_opt),
                        None,
                        None,
                    );
                }
                if res.is_err() {
                    used_video_flags = false;
                    used_warp = true;
                    self.adapter_name = "Microsoft Basic Render Driver (WARP)".to_string();
                    res = D3D11CreateDevice(
                        None,
                        D3D_DRIVER_TYPE_WARP,
                        HMODULE::default(),
                        D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                        None,
                        D3D11_SDK_VERSION,
                        Some(&mut dev_opt),
                        None,
                        None,
                    );
                }
                if res.is_err() {
                    return false;
                }
                let mut has_video_dev = false;
                if let Some(ref dev) = dev_opt {
                    if let Ok(mt) = dev.cast::<ID3D11Multithread>() {
                        let _ = mt.SetMultithreadProtected(true);
                    }
                    if let Ok(dxgi3) = dev.cast::<IDXGIDevice3>() {
                        let _ = dxgi3.SetMaximumFrameLatency(2);
                    }
                    has_video_dev = used_video_flags && dev.cast::<ID3D11VideoDevice>().is_ok();
                }
                self.device_has_video_support = has_video_dev;
                self.decode_mode = if self.static_mode {
                    VideoDecodeMode::Unknown
                } else if used_warp {
                    VideoDecodeMode::WarpSoftware
                } else if has_video_dev {
                    VideoDecodeMode::HardwareDxva
                } else {
                    VideoDecodeMode::SoftwareCpu
                };
                self.d3d_device = dev_opt;
            }

            if self.swap_chain.is_none() {
                let (sw, sh) = self.swap_dims();
                let _ = self.create_swap_chain_for_size(sw, sh);
            }
        }
        self.d3d_device.is_some() && self.swap_chain.is_some()
    }

    fn ensure_dxgi(&mut self) -> bool {
        if !self.ensure_d3d_device_and_swap_chain() {
            return false;
        }
        unsafe {
            if self.dxgi_mgr.is_none() {
                if let Some(ref dev) = self.d3d_device {
                    let mut reset_token = 0u32;
                    let mut mgr_opt: Option<IMFDXGIDeviceManager> = None;
                    if MFCreateDXGIDeviceManager(&mut reset_token, &mut mgr_opt).is_ok() {
                        if let Some(ref mgr) = mgr_opt {
                            if let Ok(unk) = dev.cast::<windows::core::IUnknown>() {
                                if mgr.ResetDevice(&unk, reset_token).is_ok() {
                                    self.dxgi_mgr = mgr_opt;
                                } else if self.decode_mode == VideoDecodeMode::HardwareDxva {
                                    self.decode_mode = VideoDecodeMode::SoftwareCpu;
                                }
                            }
                        }
                    } else if self.decode_mode == VideoDecodeMode::HardwareDxva {
                        self.decode_mode = VideoDecodeMode::SoftwareCpu;
                    }
                }
            }
        }
        self.d3d_device.is_some() && self.dxgi_mgr.is_some() && self.swap_chain.is_some()
    }

    /// Presents a top-down 32-bpp opaque BGRA image directly onto the Win11-proven
    /// `video_hwnd` Flip-Model DXGI SwapChain (both buffers) with zero Media Foundation
    /// commit and 0% CPU/GPU usage after upload.
    /// Always re-presents AFTER `video_hwnd` is visible and seated behind `SHELLDLL_DefView`,
    /// explicitly distinguishing `S_OK` vs `DXGI_STATUS_OCCLUDED` vs device/swapchain errors.
    fn present_bgra_to_swap_chain(&mut self, bgra: &[u8], w: u32, h: u32) -> PresentOutcome {
        let valid_len = (w as usize)
            .checked_mul(h as usize)
            .and_then(|n| n.checked_mul(4))
            .is_some_and(|n| n > 0 && bgra.len() >= n);
        if !valid_len {
            return PresentOutcome::Failed;
        }

        for attempt in 0..2 {
            if attempt > 0 {
                self.drop_swap_chain();
                self.dxgi_mgr = None;
                self.clear_sample_textures();
                self.d3d_device = None;
                self.device_has_video_support = false;
                self.decode_mode = VideoDecodeMode::Unknown;
            }
            if !self.ensure_d3d_device_and_swap_chain() {
                continue;
            }
            // Verify swapchain buffer size matches `(w, h)` so D3D11 `CopyResource` never silently drops.
            let mut need_recreate_sc = false;
            if let Some(ref sc) = self.swap_chain {
                unsafe {
                    if let Ok(sc_desc) = sc.GetDesc1() {
                        if sc_desc.Width != w || sc_desc.Height != h {
                            let resized = sc
                                .ResizeBuffers(0, w, h, DXGI_FORMAT_UNKNOWN, self.swap_chain_flags)
                                .is_ok();
                            let verified = resized
                                && sc
                                    .GetDesc1()
                                    .map(|d| d.Width == w && d.Height == h)
                                    .unwrap_or(false);
                            if !verified {
                                need_recreate_sc = true;
                            }
                        }
                    } else {
                        need_recreate_sc = true;
                    }
                }
            }
            if need_recreate_sc {
                self.drop_swap_chain();
                if !self.create_swap_chain_for_size(w, h) {
                    continue;
                }
            }

            let (Some(dev), Some(mut sc)) = (self.d3d_device.clone(), self.swap_chain.clone()) else {
                continue;
            };

            let src_res_opt = unsafe {
                let tex_desc = D3D11_TEXTURE2D_DESC {
                    Width: w,
                    Height: h,
                    MipLevels: 1,
                    ArraySize: 1,
                    Format: DXGI_FORMAT_B8G8R8A8_UNORM,
                    SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
                    Usage: D3D11_USAGE_IMMUTABLE,
                    BindFlags: D3D11_BIND_SHADER_RESOURCE.0 as u32,
                    CPUAccessFlags: 0,
                    MiscFlags: 0,
                };
                let init_data = D3D11_SUBRESOURCE_DATA {
                    pSysMem: bgra.as_ptr() as *const c_void,
                    SysMemPitch: w * 4,
                    SysMemSlicePitch: 0,
                };
                let mut tex_opt: Option<ID3D11Texture2D> = None;
                if dev
                    .CreateTexture2D(&tex_desc, Some(&init_data), Some(&mut tex_opt))
                    .is_ok()
                {
                    tex_opt.and_then(|t| t.cast::<ID3D11Resource>().ok())
                } else {
                    None
                }
            };
            let Some(src_res) = src_res_opt else {
                continue;
            };
            let Ok(ctx) = (unsafe { dev.GetImmediateContext() }) else {
                continue;
            };

            let was_visible = unsafe {
                !self.video_hwnd.0.is_null() && IsWindowVisible(self.video_hwnd).as_bool()
            };

            // Pass 1 (pre-show prime): if `video_hwnd` is hidden, fill buffers first so showing it
            // never flashes a black frame.
            let waitable_h = self.frame_latency_waitable;
            let prime_count = self.swap_chain_buffers.clamp(2, 3);
            if !was_visible {
                unsafe {
                    for _ in 0..prime_count {
                        if let Ok(back_buf) = sc.GetBuffer::<ID3D11Texture2D>(0) {
                            if let Ok(dst_res) = back_buf.cast::<ID3D11Resource>() {
                                ctx.CopyResource(&dst_res, &src_res);
                                if !waitable_h.0.is_null() {
                                    let _ = WaitForSingleObject(waitable_h, 32);
                                }
                                let _ = sc.Present(0, DXGI_PRESENT(0));
                            }
                        }
                    }
                }
            }

            // Ensure `video_hwnd` is seated behind `SHELLDLL_DefView` and VISIBLE before final Flip.
            self.swap_chain_has_frame = true;
            self.has_presented_frame = true;
            let reparented = self.place_behind_icons_tracked();
            unsafe {
                if !self.video_hwnd.0.is_null() && !IsWindowVisible(self.video_hwnd).as_bool() {
                    let _ = ShowWindow(self.video_hwnd, SW_SHOW);
                }
            }
            if reparented {
                // Flip-Model swapchain parent changed: recreate on the visible parented HWND.
                self.drop_swap_chain();
                if !self.create_swap_chain_for_size(w, h) {
                    continue;
                }
                let Some(fresh_sc) = self.swap_chain.clone() else {
                    continue;
                };
                sc = fresh_sc;
            }

            // Pass 2 (authoritative visible Flip-Model commit): present buffers while
            // `video_hwnd` is visible and inspect exact HRESULT (`S_OK` vs `DXGI_STATUS_OCCLUDED`).
            let waitable_h2 = self.frame_latency_waitable;
            let mut presented_ok = false;
            let mut saw_occluded = false;
            let mut saw_error = false;
            unsafe {
                for _ in 0..2 {
                    let Ok(back_buf) = sc.GetBuffer::<ID3D11Texture2D>(0) else {
                        saw_error = true;
                        break;
                    };
                    let Ok(dst_res) = back_buf.cast::<ID3D11Resource>() else {
                        saw_error = true;
                        break;
                    };
                    ctx.CopyResource(&dst_res, &src_res);
                    if !waitable_h2.0.is_null() {
                        let _ = WaitForSingleObject(waitable_h2, 32);
                    }
                    let hr = sc.Present(0, DXGI_PRESENT(0));
                    if hr.0 == 0 {
                        presented_ok = true;
                    } else if hr.0 == DXGI_STATUS_OCCLUDED_HR {
                        saw_occluded = true;
                    } else {
                        saw_error = true;
                        break;
                    }
                }
            }
            if saw_error {
                self.swap_chain_has_frame = false;
                self.has_presented_frame = false;
                continue;
            }
            if presented_ok {
                return PresentOutcome::Presented;
            }
            if saw_occluded {
                return PresentOutcome::Occluded;
            }
        }

        self.swap_chain_has_frame = false;
        self.has_presented_frame = false;
        PresentOutcome::Failed
    }

    fn swap_dims(&self) -> (u32, u32) {
        let w = self.screen_w.max(2) as f32;
        let h = self.screen_h.max(2) as f32;
        let scale = (1920.0f32 / w.max(h)).min(1.0);
        let sw = (((w * scale).round() as u32).max(2)) & !1;
        let sh = (((h * scale).round() as u32).max(2)) & !1;
        (sw.max(2), sh.max(2))
    }

    pub fn trim_gpu_memory(&self) {
        unsafe {
            if let Some(ref dev) = self.d3d_device {
                if let Ok(ctx) = dev.GetImmediateContext() {
                    ctx.ClearState();
                    ctx.Flush();
                }
                if let Ok(dxgi3) = dev.cast::<IDXGIDevice3>() {
                    dxgi3.Trim();
                }
            }
        }
    }

    /// Starts or stops the dedicated VSync-aligned frame pacer thread.
    /// Instead of a fixed 33ms low-priority `WM_TIMER` (which starves behind UI repaints and
    /// quantizes to 31–47ms), the pacer synchronizes with `DwmFlush()` and posts coalesced
    /// `WM_WP_PUMP` messages matching monitor refresh / `wallpaper_fps_cap` cadence.
    fn set_frame_timer(&mut self, run: bool) {
        if self.hwnd.0.is_null() {
            if let Some(stop) = self.pacer_stop.take() {
                stop.store(true, Ordering::Release);
            }
            self.timer_on = false;
            self.pump_queued.store(false, Ordering::Release);
            return;
        }
        unsafe {
            let _ = KillTimer(Some(self.hwnd), TIMER_WP_FRAME);
        }
        if run {
            self.sync_pacer_cadence();
            if self.timer_on && self.pacer_stop.is_some() {
                return;
            }
            let stop = Arc::new(AtomicBool::new(false));
            let stop_c = stop.clone();
            let interval_us = self.pacer_interval_us.clone();
            let pump_queued = self.pump_queued.clone();
            pump_queued.store(false, Ordering::Release);
            let hwnd_raw = self.hwnd.0 as usize;
            let spawned = std::thread::Builder::new()
                .name("autumn-wp-pacer".to_string())
                .spawn(move || {
                    let mut last_post = Instant::now() - Duration::from_millis(50);
                    while !stop_c.load(Ordering::Acquire) && !WP_OS_EXIT.load(Ordering::Acquire) {
                        let hw = HWND(hwnd_raw as *mut c_void);
                        unsafe {
                            if hw.0.is_null() || !IsWindow(Some(hw)).as_bool() {
                                break;
                            }
                        }
                        let target_us = interval_us.load(Ordering::Acquire).clamp(8_000, 33_333);
                        let dwm_ok = unsafe { DwmFlush().is_ok() };
                        if !dwm_ok {
                            std::thread::sleep(Duration::from_micros(target_us));
                        }
                        if stop_c.load(Ordering::Acquire) || WP_OS_EXIT.load(Ordering::Acquire) {
                            break;
                        }
                        // Allow 1.5ms VSync phase lead so 60Hz VSync (~16.6ms) never skips
                        // every other tick when target_us == 16_666us.
                        if last_post.elapsed().as_micros() as u64 + 1_500 >= target_us {
                            last_post = Instant::now();
                            if !pump_queued.swap(true, Ordering::AcqRel) {
                                unsafe {
                                    let _ = PostMessageW(
                                        Some(hw),
                                        WM_WP_PUMP,
                                        WPARAM(0),
                                        LPARAM(0),
                                    );
                                }
                            }
                        }
                    }
                })
                .is_ok();
            if spawned {
                self.pacer_stop = Some(stop);
                self.timer_on = true;
            } else {
                // Fallback if OS thread creation fails
                let ms = (self.pacer_interval_us.load(Ordering::Acquire) / 1000).clamp(16, 33) as u32;
                unsafe {
                    let _ = SetTimer(Some(self.hwnd), TIMER_WP_FRAME, ms, None);
                }
                self.timer_on = true;
            }
        } else {
            if let Some(stop) = self.pacer_stop.take() {
                stop.store(true, Ordering::Release);
            }
            self.pump_queued.store(false, Ordering::Release);
            self.timer_on = false;
        }
    }

    /// Reads a D3D11 BGRA texture (`sw x sh`) to CPU memory via a temporary staging texture,
    /// returning `Some(bgra)` only if the frame contains visible non-black pixels.
    unsafe fn readback_texture_bgra(
        dev: &ID3D11Device,
        src_tex: &ID3D11Texture2D,
        sw: u32,
        sh: u32,
    ) -> Option<Vec<u8>> {
        let st_desc = D3D11_TEXTURE2D_DESC {
            Width: sw,
            Height: sh,
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT_B8G8R8A8_UNORM,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            Usage: D3D11_USAGE_STAGING,
            BindFlags: 0,
            CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
            MiscFlags: 0,
        };
        let mut st_tex_opt: Option<ID3D11Texture2D> = None;
        if dev.CreateTexture2D(&st_desc, None, Some(&mut st_tex_opt)).is_err() {
            return None;
        }
        let st_tex = st_tex_opt?;
        let ctx = dev.GetImmediateContext().ok()?;
        let rt_res = src_tex.cast::<ID3D11Resource>().ok()?;
        let st_res = st_tex.cast::<ID3D11Resource>().ok()?;
        ctx.CopyResource(&st_res, &rt_res);
        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        if ctx.Map(&st_res, 0, D3D11_MAP_READ, 0, Some(&mut mapped)).is_err() {
            return None;
        }
        let row_bytes = (sw * 4) as usize;
        let mut bgra = vec![0u8; row_bytes * (sh as usize)];
        let pitch = mapped.RowPitch as usize;
        let src_ptr = mapped.pData as *const u8;
        let mut ok = false;
        if !src_ptr.is_null() && pitch >= row_bytes {
            for row in 0..(sh as usize) {
                let s_slice = std::slice::from_raw_parts(src_ptr.add(row * pitch), row_bytes);
                bgra[row * row_bytes..(row + 1) * row_bytes].copy_from_slice(s_slice);
            }
            ok = true;
        }
        ctx.Unmap(&st_res, 0);
        if ok && is_non_blank_bgra(&bgra, sw, sh) {
            Some(bgra)
        } else {
            None
        }
    }

    /// Captures the exact currently decoded frame into an opaque 24-bpp GDI DIB (~6.6 MB) on
    /// `backdrop_hwnd`, dropping temporary D3D11 textures immediately.
    /// Uses a 3-tier fallback (live `TransferVideoFrame` -> live DXGI backbuffer copy -> full-res
    /// Shell/WIC poster decode) and validates non-blank pixels so the wallpaper NEVER blanks.
    fn capture_current_frame_to_backdrop(&mut self, eng: &IMFMediaEngine) -> bool {
        let (sw, sh) = self.swap_dims();
        // Tier 1: Direct live `TransferVideoFrame` while decoder is still hot
        if let Some(dev) = self.d3d_device.clone() {
            let _mf = MfCallGuard::enter();
            unsafe {
                if eng.HasVideo().as_bool() {
                    let rt_desc = D3D11_TEXTURE2D_DESC {
                        Width: sw,
                        Height: sh,
                        MipLevels: 1,
                        ArraySize: 1,
                        Format: DXGI_FORMAT_B8G8R8A8_UNORM,
                        SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
                        Usage: D3D11_USAGE_DEFAULT,
                        BindFlags: D3D11_BIND_RENDER_TARGET.0 as u32,
                        CPUAccessFlags: 0,
                        MiscFlags: 0,
                    };
                    let mut rt_tex_opt: Option<ID3D11Texture2D> = None;
                    if dev.CreateTexture2D(&rt_desc, None, Some(&mut rt_tex_opt)).is_ok() {
                        if let Some(rt_tex) = rt_tex_opt {
                            let crop = self.compute_crop_rect(eng);
                            let dst = RECT {
                                left: 0,
                                top: 0,
                                right: sw as i32,
                                bottom: sh as i32,
                            };
                            if eng.TransferVideoFrame(&rt_tex, Some(&crop), &dst, None).is_ok() {
                                if let Some(bgra) = Self::readback_texture_bgra(&dev, &rt_tex, sw, sh) {
                                    if let Some(dib) = StaticDib::from_bgra_top_down(sw, sh, &bgra) {
                                        self.backdrop_dib = Some(dib);
                                        return true;
                                    }
                                }
                            }
                        }
                    }
                }
            }
            // Tier 2: Read directly from the currently presented DXGI swapchain backbuffer
            if let Some(sc) = self.swap_chain.clone() {
                unsafe {
                    if let Ok(back_buf) = sc.GetBuffer::<ID3D11Texture2D>(0) {
                        if let Some(bgra) = Self::readback_texture_bgra(&dev, &back_buf, sw, sh) {
                            if let Some(dib) = StaticDib::from_bgra_top_down(sw, sh, &bgra) {
                                self.backdrop_dib = Some(dib);
                                return true;
                            }
                        }
                    }
                }
            }
        }
        // Tier 3: Full-resolution Shell/MF poster frame fallback
        if !self.active_path.is_empty() {
            if let Some(bgra) = decode_wallpaper_thumbnail_bgra(&self.active_path, sw, sh) {
                if is_non_blank_bgra(&bgra, sw, sh) {
                    if let Some(dib) = StaticDib::from_bgra_top_down(sw, sh, &bgra) {
                        self.backdrop_dib = Some(dib);
                        return true;
                    }
                }
            }
        }
        self.backdrop_dib.is_some()
    }

    fn start_player(&mut self, video_path: &str) {
        self.trimmed_initial = false;
        self.static_mode = false;
        self.monitor_hz = query_primary_monitor_refresh_hz();
        self.sync_pacer_cadence();
        self.last_presented_at = None;
        self.ensure_window();
        if !self.mf_started {
            self.mf_started = unsafe { MFStartup(MF_API_VERSION, 0).is_ok() };
        }
        if !self.ensure_dxgi() {
            self.post_state_notify();
            return;
        }
        if let Some(eng) = self.engine.clone() {
            self.set_frame_timer(false);
            self.paused_by_cover = false;
            self.pending_cover_pause = false;
            self.pause_trimmed = false;
            self.pause_started_at = None;
            self.has_presented_frame = false;
            self.sample_dirty = true;
            self.palette_slots.reset();
            self.player_started_at = Some(Instant::now());
            let reuse_ok = {
                let _mf = MfCallGuard::enter();
                unsafe {
                    let _ = eng.Pause();
                    let _ = eng.SetLoop(true);
                    let _ = eng.SetAutoPlay(true);
                    let _ = eng.SetMuted(self.mute || self.volume_pct == 0);
                    let _ = eng.SetVolume((self.volume_pct as f64 / 100.0).clamp(0.0, 1.0));
                    let bstr = BSTR::from(video_path);
                    eng.SetSource(&bstr).is_ok() && {
                        let _ = eng.Load();
                        let _ = eng.Play();
                        true
                    }
                }
            };
            if reuse_ok {
                self.cached_crop = None;
                self.native_video_size = (0, 0);
                self.end_poll_ticks = 0;
                self.place_behind_icons();
                self.set_frame_timer(true);
                self.post_state_notify();
                return;
            }
        }
        self.stop_player_only();
        unsafe {
            let mut attrs_opt: Option<IMFAttributes> = None;
            if MFCreateAttributes(&mut attrs_opt, 4).is_err() {
                return;
            }
            let Some(attrs) = attrs_opt else {
                return;
            };
            let cb: IMFMediaEngineNotify = WallpaperNotifyCallback { hwnd: self.hwnd }.into();
            if attrs.SetUnknown(&MF_MEDIA_ENGINE_CALLBACK, &cb).is_err() {
                return;
            }
            if let Some(ref mgr) = self.dxgi_mgr {
                let _ = attrs.SetUnknown(&MF_MEDIA_ENGINE_DXGI_MANAGER, mgr);
            }
            let _ = attrs.SetUINT32(
                &MF_MEDIA_ENGINE_VIDEO_OUTPUT_FORMAT,
                DXGI_FORMAT_B8G8R8A8_UNORM.0 as u32,
            );
            // Use standard Media Engine mode (0) so Media Foundation keeps its full hardware
            // DXVA B-frame lookahead buffer for smooth, zero-stutter prerecorded MP4 playback.
            // (Memory when covered by maximized windows is reclaimed cleanly by our 8s cover timer.)
            let eng_opt: Option<IMFMediaEngine> = {
                let _mf = MfCallGuard::enter();
                CoCreateInstance::<_, IMFMediaEngineClassFactory>(
                    &CLSID_MFMediaEngineClassFactory,
                    None,
                    CLSCTX_INPROC_SERVER,
                )
                .ok()
                .and_then(|factory| factory.CreateInstance(0, &attrs).ok())
                .map(|eng| {
                    let _ = eng.SetLoop(true);
                    let _ = eng.SetAutoPlay(true);
                    let _ = eng.SetMuted(self.mute || self.volume_pct == 0);
                    let _ = eng.SetVolume((self.volume_pct as f64 / 100.0).clamp(0.0, 1.0));
                    let bstr = BSTR::from(video_path);
                    let _ = eng.SetSource(&bstr);
                    let _ = eng.Load();
                    let _ = eng.Play();
                    eng
                })
            };
            let Some(eng) = eng_opt else {
                return;
            };

            self._cb = Some(cb);
            self.engine = Some(eng);
            self.paused_by_cover = false;
            self.pending_cover_pause = false;
            self.pause_trimmed = false;
            self.pause_started_at = None;
            self.sample_dirty = true;
            self.palette_slots.reset();
            self.initial_settle_done = false;
            self.player_started_at = Some(Instant::now());
            self.last_palette_sample = None;
            self.last_presented_at = None;
            self.place_behind_icons();
            self.set_frame_timer(true);
            self.post_state_notify();
        }
    }

    fn stop_player_only(&mut self) {
        self.set_frame_timer(false);
        if let Some(eng) = self.engine.take() {
            unsafe {
                let _ = eng.Shutdown();
            }
        }
        self._cb = None;
        self.paused_by_cover = false;
        self.pending_cover_pause = false;
        self.pause_trimmed = false;
        self.pause_started_at = None;
        self.has_presented_frame = false;
        self.cached_crop = None;
        self.native_video_size = (0, 0);
        self.end_poll_ticks = 0;
        self.initial_settle_done = false;
        self.player_started_at = None;
        self.last_palette_sample = None;
        self.last_presented_at = None;
        self.palette_slots.reset();
    }

    /// Reclaims heavyweight Media Foundation & DXVA decoder commit (~300+ MB) while keeping the
    /// warm D3D11 device + Flip-Model DXGI `swap_chain` + `video_hwnd` alive on screen holding its
    /// last presented frame — zero GPU readback stall, zero GDI copy, and fast resume.
    fn release_media_engine_keep_swapchain(&mut self) {
        self.set_frame_timer(false);
        if let Some(eng) = self.engine.take() {
            unsafe {
                let _ = eng.Shutdown();
            }
        }
        self._cb = None;
        self.clear_sample_textures();
        self.cached_crop = None;
        self.native_video_size = (0, 0);
        self.end_poll_ticks = 0;
        self.pending_cover_pause = false;
        self.has_presented_frame = false;
        self.trim_gpu_memory();
        crate::config::trim_process_working_set();
    }

    /// Releases the heavyweight video pipeline (IMFMediaEngine, DXGI SwapChain, D3D11 device,
    /// staging textures, `video_hwnd`, and Media Foundation runtime) while KEEPING `backdrop_hwnd`
    /// and its `backdrop_dib` alive & visible so the desktop wallpaper NEVER closes or goes blank.
    fn release_video_pipeline_keep_backdrop(&mut self) {
        self.set_frame_timer(false);
        if let Some(eng) = self.engine.take() {
            unsafe {
                let _ = eng.Shutdown();
            }
        }
        self._cb = None;
        self.clear_sample_textures();
        self.cached_crop = None;
        self.native_video_size = (0, 0);
        self.end_poll_ticks = 0;
        self.drop_swap_chain();
        self.dxgi_mgr = None;
        self.trim_gpu_memory();
        self.d3d_device = None;
        self.device_has_video_support = false;
        self.decode_mode = VideoDecodeMode::Unknown;
        if self.mf_started && !self.is_in_mf_call() {
            unsafe {
                let _ = MFShutdown();
            }
            self.mf_started = false;
        }
        if !self.video_hwnd.0.is_null() {
            unsafe {
                let _ = ShowWindow(self.video_hwnd, SW_HIDE);
                if !self.is_in_mf_call() {
                    if let Ok(p) = GetParent(self.video_hwnd) {
                        if !p.0.is_null() {
                            let _ = SetParent(self.video_hwnd, None);
                        }
                    }
                    let _ = DestroyWindow(self.video_hwnd);
                    self.video_hwnd = HWND::default();
                }
            }
        }
        self.has_presented_frame = false;
        self.pending_cover_pause = false;
        // Now that the sibling video surface is gone, ensure the layered backdrop sits right behind
        // SHELLDLL_DefView and is freshly painted on Windows 11 24H2/26H2.
        if self.backdrop_dib.is_some() {
            self.place_behind_icons();
            self.paint_backdrop_now();
        }
        crate::config::trim_process_working_set();
    }

    /// Tears down Media Foundation decoder/timers while keeping the D3D11 device,
    /// `video_hwnd`, and Flip-Model DXGI SwapChain ready for static image presentation.
    fn release_video_decoder_only(&mut self) {
        self.stop_player_only();
        self.clear_sample_textures();
        self.dxgi_mgr = None;
        if self.mf_started && !self.is_in_mf_call() {
            unsafe {
                let _ = MFShutdown();
            }
            self.mf_started = false;
        }
        self.trim_gpu_memory();
    }

    fn release_backend_resources(&mut self) {
        self.static_load_gen = self.static_load_gen.wrapping_add(1);
        self.static_load_state = StaticLoadState::Idle;
        self.pending_static_frame = None;
        self.static_needs_represent = false;
        self.set_static_retry_timer(false);
        self.release_video_pipeline_keep_backdrop();
        self.backdrop_dib = None;
        self.static_mode = false;
        if !self.hwnd.0.is_null() {
            unsafe {
                let _ = ShowWindow(self.hwnd, SW_HIDE);
                if !self.is_in_mf_call() {
                    if let Ok(p) = GetParent(self.hwnd) {
                        if !p.0.is_null() {
                            let _ = SetParent(self.hwnd, None);
                        }
                    }
                    let _ = DestroyWindow(self.hwnd);
                    self.hwnd = HWND::default();
                }
            }
        }
        crate::config::trim_process_working_set();
    }

    pub fn stop_player(&mut self) {
        self.stop_player_only();
        self.release_backend_resources();
    }

    /// Pauses/resumes video playback when the desktop is covered (`should_pause` already reflects
    /// `(has_max && wallpaper_pause_maximized) || game_mode_active`).
    /// Never blanks or closes the desktop wallpaper: captures the last video frame to the GDI
    /// `backdrop_hwnd` before reclaiming Media Foundation / DXVA / D3D11 commit memory.
    /// In static mode, when the desktop uncovers (`!should_pause`), immediately retries any
    /// occluded Flip-Model present so photos applied while covered snap into view immediately.
    pub fn update_pause_for_cover(&mut self, should_pause: bool) {
        if !self.enabled {
            return;
        }
        if self.static_mode {
            if !should_pause && (self.static_needs_represent || !self.worker_split_ready) {
                self.static_retry_ticks = 0;
                self.retry_static_represent();
            }
            return;
        }
        if self.engine.is_none() {
            if !should_pause && !self.active_path.is_empty() && Path::new(&self.active_path).exists() {
                let path = self.active_path.clone();
                self.paused_by_cover = false;
                self.pause_trimmed = false;
                self.pause_started_at = None;
                self.start_player(&path);
            }
            return;
        }
        let Some(eng) = self.engine.clone() else {
            return;
        };
        if should_pause && !self.paused_by_cover {
            // If playback was just started and Frame #1 hasn't presented to `video_hwnd` yet,
            // wait up to 3s for Frame #1 so the DXGI SwapChain holds the real frame instead of
            // pausing before `video_hwnd` is shown (e.g., booting while maximized).
            if !self.has_presented_frame
                && self
                    .player_started_at
                    .is_some_and(|at| at.elapsed() < Duration::from_secs(3))
            {
                self.pending_cover_pause = true;
                return;
            }
            self.pending_cover_pause = false;
            self.paused_by_cover = true;
            self.pause_trimmed = false;
            self.pause_started_at = Some(Instant::now());
            self.set_frame_timer(false);
            {
                let _mf = MfCallGuard::enter();
                unsafe {
                    let _ = eng.Pause();
                }
            }
            self.trim_gpu_memory();
        } else if should_pause && self.paused_by_cover {
            let has_frozen_swapchain = self.swap_chain.is_some() && self.swap_chain_has_frame;
            let reclaim_delay_ms = compute_cover_reclaim_delay_ms(
                self.trim_on_pause,
                self.native_video_size.0,
                self.native_video_size.1,
            );
            // Automatically reclaim heavyweight IMFMediaEngine decoder/surface commit (~300-500 MB
            // on 4K videos) once covered for `reclaim_delay_ms`, whenever the Flip-Model SwapChain
            // already holds the exact frozen last frame (zero blanking, zero flash), or when the
            // user explicitly enabled `trim_on_pause` via the backdrop fallback.
            if (has_frozen_swapchain || self.trim_on_pause)
                && !self.pause_trimmed
                && self
                    .pause_started_at
                    .is_some_and(|at| at.elapsed() >= Duration::from_millis(reclaim_delay_ms))
            {
                self.pause_trimmed = true;
                {
                    let _mf = MfCallGuard::enter();
                    let cur_t = unsafe { eng.GetCurrentTime() };
                    if cur_t.is_finite() && cur_t > 0.1 {
                        self.pending_resume_sec = cur_t;
                    }
                }
                if has_frozen_swapchain {
                    // Fast Win11-safe path: drop ONLY IMFMediaEngine decoder/demuxer (~300-500 MB)
                    // while keeping the Flip-Model DXGI SwapChain & `video_hwnd` alive on screen
                    // with its exact last rendered frame — zero GPU readback stall & zero blanking!
                    drop(eng);
                    self.release_media_engine_keep_swapchain();
                } else {
                    if self.backdrop_dib.is_none() {
                        let _ = self.capture_current_frame_to_backdrop(&eng);
                    }
                    if self.backdrop_dib.is_some() {
                        drop(eng);
                        self.release_video_pipeline_keep_backdrop();
                    }
                }
            }
        } else if !should_pause {
            self.pending_cover_pause = false;
            if self.paused_by_cover {
                self.paused_by_cover = false;
                self.pause_trimmed = false;
                self.pause_started_at = None;
                {
                    let _mf = MfCallGuard::enter();
                    unsafe {
                        let _ = eng.Play();
                    }
                }
                self.set_frame_timer(true);
                self.pump_frame(true);
            }
        }
    }

    pub fn handle_display_change(&mut self) {
        if !self.enabled {
            return;
        }
        self.monitor_hz = query_primary_monitor_refresh_hz();
        self.sync_pacer_cadence();
        self.ensure_window();
        if self.static_mode {
            let path = self.active_path.clone();
            if !path.is_empty() && Path::new(&path).is_file() {
                // Re-decode and re-present asynchronously for the new monitor resolution.
                self.start_static_image(&path);
            }
            return;
        }
        let (sw, sh) = self.swap_dims();
        if let Some(ref sc) = self.swap_chain {
            unsafe {
                let _ = sc.ResizeBuffers(
                    0,
                    sw,
                    sh,
                    DXGI_FORMAT_UNKNOWN,
                    self.swap_chain_flags,
                );
            }
        }
        self.pump_frame(true);
    }

    fn compute_crop_rect(&mut self, eng: &IMFMediaEngine) -> MFVideoNormalizedRect {
        if let Some((cw, ch, rect)) = self.cached_crop {
            if cw == self.screen_w && ch == self.screen_h {
                return rect;
            }
        }
        let mut vw = 0u32;
        let mut vh = 0u32;
        unsafe {
            if eng.GetNativeVideoSize(Some(&mut vw), Some(&mut vh)).is_ok()
                && vw > 0
                && vh > 0
                && self.screen_w > 0
                && self.screen_h > 0
            {
                self.native_video_size = (vw, vh);
                let va = vw as f32 / vh as f32;
                let ma = self.screen_w as f32 / self.screen_h as f32;
                let rect = if (va - ma).abs() < 0.01 {
                    MFVideoNormalizedRect { left: 0.0, top: 0.0, right: 1.0, bottom: 1.0 }
                } else if va > ma {
                    let keep = (ma / va).clamp(0.1, 1.0);
                    let pad = (1.0 - keep) * 0.5;
                    MFVideoNormalizedRect { left: pad, top: 0.0, right: 1.0 - pad, bottom: 1.0 }
                } else {
                    let keep = (va / ma).clamp(0.1, 1.0);
                    let pad = (1.0 - keep) * 0.5;
                    MFVideoNormalizedRect { left: 0.0, top: pad, right: 1.0, bottom: 1.0 - pad }
                };
                self.cached_crop = Some((self.screen_w, self.screen_h, rect));
                return rect;
            }
        }
        MFVideoNormalizedRect { left: 0.0, top: 0.0, right: 1.0, bottom: 1.0 }
    }

    /// Presents a newly decoded GPU frame cleanly. When `OnVideoStreamTick` indicates
    /// no new frame is ready (including during loop seek/rewind), skips `Present`
    /// so the previous front-buffer frame stays visible with 0ms black flash.
    fn pump_frame(&mut self, force_current: bool) {
        if self.is_in_mf_call() {
            return;
        }
        // Drain any completed async GPU->CPU Chameleon readback first (non-blocking DO_NOT_WAIT).
        if self.chameleon_enabled && self.palette_slots.any_in_flight() {
            if self.poll_async_palette_readback()
                && !self.notify_hwnd.0.is_null()
                && self.notify_msg != 0
            {
                unsafe {
                    let _ = PostMessageW(
                        Some(self.notify_hwnd),
                        self.notify_msg,
                        WPARAM(0),
                        LPARAM(0),
                    );
                }
            }
        }

        // Enforce user FPS Cap cleanly (30 / 60 FPS) without skipping forced frames.
        if !force_current {
            let min_us = compute_min_present_interval_us(self.fps_cap);
            if min_us > 0 {
                if let Some(last) = self.last_presented_at {
                    if (last.elapsed().as_micros() as u64) < min_us {
                        return;
                    }
                }
            }
        }

        let (sw, sh) = self.swap_dims();
        let crop = match (self.engine.clone(), self.swap_chain.as_ref()) {
            (Some(eng), Some(_)) => self.compute_crop_rect(&eng),
            _ => return,
        };
        let waitable_h = self.frame_latency_waitable;
        let presented = {
            let (Some(ref eng), Some(ref sc)) = (&self.engine, &self.swap_chain) else {
                return;
            };
            let _mf = MfCallGuard::enter();
            unsafe {
                if !eng.HasVideo().as_bool() {
                    return;
                }
                let new_frame = matches!(eng.OnVideoStreamTick(), Ok(pts) if pts != i64::MIN);
                if new_frame {
                    self.end_poll_ticks = 0;
                } else if !force_current {
                    // Check IsEnded() every 4 idle ticks (~65–130ms) instead of 30 (~1s) so
                    // loops restart seamlessly even if MF's ENDED callback arrives late.
                    self.end_poll_ticks = self.end_poll_ticks.wrapping_add(1);
                    if self.end_poll_ticks >= 4 {
                        self.end_poll_ticks = 0;
                        if eng.IsEnded().as_bool() {
                            let _ = eng.SetCurrentTime(0.0);
                            let _ = eng.Play();
                        }
                    }
                    return;
                }
                let Ok(back_buf) = sc.GetBuffer::<ID3D11Texture2D>(0) else {
                    return;
                };
                let dst = RECT {
                    left: 0,
                    top: 0,
                    right: sw as i32,
                    bottom: sh as i32,
                };
                if eng.TransferVideoFrame(&back_buf, Some(&crop), &dst, None).is_err() {
                    return;
                }
                // Wait strictly 1:1 right before Present() so the waitable swapchain semaphore
                // can never leak if TransferVideoFrame returns an error on a seek frame.
                if !waitable_h.0.is_null() {
                    let wait_ms = if force_current { 16 } else { 4 };
                    if WaitForSingleObject(waitable_h, wait_ms) != WAIT_OBJECT_0 {
                        return;
                    }
                }
                sc.Present(0, DXGI_PRESENT(0)).is_ok()
            }
        };
        if presented {
            self.last_presented_at = Some(Instant::now());
            self.swap_chain_has_frame = true;
            if !self.has_presented_frame {
                self.has_presented_frame = true;
                self.place_behind_icons();
                unsafe {
                    if !self.video_hwnd.0.is_null() {
                        let _ = ShowWindow(self.video_hwnd, SW_SHOW);
                    }
                }
            }
            if self.pending_cover_pause && !self.paused_by_cover {
                self.pending_cover_pause = false;
                self.update_pause_for_cover(true);
                return;
            }
            if self.chameleon_enabled {
                let now = Instant::now();
                let do_initial = self.sample_dirty;
                let do_settle = !do_initial
                    && !self.initial_settle_done
                    && self
                        .player_started_at
                        .map(|t| now.duration_since(t) >= Duration::from_millis(950))
                        .unwrap_or(false);
                let do_periodic = !do_initial
                    && !do_settle
                    && self
                        .last_palette_sample
                        .map(|t| now.duration_since(t) >= Duration::from_millis(2200))
                        .unwrap_or(true);

                if do_initial || do_settle || do_periodic {
                    if let Some(eng) = self.engine.clone() {
                        if self.enqueue_async_palette_sample(&eng, !do_initial) {
                            self.sample_dirty = false;
                            if do_settle {
                                self.initial_settle_done = true;
                            }
                            self.last_palette_sample = Some(now);
                        }
                    }
                }
            }
            if !self.trimmed_initial {
                self.trimmed_initial = true;
                self.trim_gpu_memory();
            }
            self.frame_count = self.frame_count.wrapping_add(1);
        }
    }

    fn ensure_sample_textures(&mut self, sw: u32, sh: u32) -> bool {
        if self.sample_rt_tex.is_some()
            && self.sample_st_tex[0].is_some()
            && self.sample_st_tex[1].is_some()
        {
            return true;
        }
        let Some(ref dev) = self.d3d_device else {
            return false;
        };
        unsafe {
            let rt_desc = D3D11_TEXTURE2D_DESC {
                Width: sw,
                Height: sh,
                MipLevels: 1,
                ArraySize: 1,
                Format: DXGI_FORMAT_B8G8R8A8_UNORM,
                SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
                Usage: D3D11_USAGE_DEFAULT,
                BindFlags: D3D11_BIND_RENDER_TARGET.0 as u32,
                CPUAccessFlags: 0,
                MiscFlags: 0,
            };
            if self.sample_rt_tex.is_none() {
                let mut rt_tex_opt: Option<ID3D11Texture2D> = None;
                if dev.CreateTexture2D(&rt_desc, None, Some(&mut rt_tex_opt)).is_err() {
                    return false;
                }
                self.sample_rt_tex = rt_tex_opt;
            }
            let st_desc = D3D11_TEXTURE2D_DESC {
                Usage: D3D11_USAGE_STAGING,
                BindFlags: 0,
                CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
                ..rt_desc
            };
            for slot in 0..2 {
                if self.sample_st_tex[slot].is_none() {
                    let mut st_tex_opt: Option<ID3D11Texture2D> = None;
                    if dev.CreateTexture2D(&st_desc, None, Some(&mut st_tex_opt)).is_err() {
                        return false;
                    }
                    self.sample_st_tex[slot] = st_tex_opt;
                }
            }
            self.sample_rt_tex.is_some()
                && self.sample_st_tex[0].is_some()
                && self.sample_st_tex[1].is_some()
        }
    }

    /// Submits an asynchronous `64x36` GPU copy into the next free double-buffered staging slot
    /// without calling `Map()`, avoiding any synchronous GPU pipeline flush.
    fn enqueue_async_palette_sample(&mut self, eng: &IMFMediaEngine, smooth: bool) -> bool {
        const SW: u32 = 64;
        const SH: u32 = 36;
        if !self.ensure_sample_textures(SW, SH) {
            return false;
        }
        let Some(slot) = self.palette_slots.acquire_write_slot() else {
            return false;
        };
        let _mf = MfCallGuard::enter();
        let crop = self.compute_crop_rect(eng);
        let (Some(ref dev), Some(ref rt_tex), Some(ref st_tex)) = (
            &self.d3d_device,
            &self.sample_rt_tex,
            &self.sample_st_tex[slot],
        ) else {
            return false;
        };
        unsafe {
            let Ok(ctx) = dev.GetImmediateContext() else {
                return false;
            };
            let dst = RECT {
                left: 0,
                top: 0,
                right: SW as i32,
                bottom: SH as i32,
            };
            if eng.TransferVideoFrame(rt_tex, Some(&crop), &dst, None).is_err() {
                return false;
            }
            let Ok(rt_res) = rt_tex.cast::<ID3D11Resource>() else {
                return false;
            };
            let Ok(st_res) = st_tex.cast::<ID3D11Resource>() else {
                return false;
            };
            ctx.CopyResource(&st_res, &rt_res);
        }
        self.palette_slots.mark_submitted(slot, smooth);
        true
    }

    /// Polls the oldest in-flight double-buffered staging texture with `D3D11_MAP_FLAG_DO_NOT_WAIT`.
    /// Returns `true` only when the GPU copy completed and updated the Chameleon palette;
    /// returns `false` immediately on `DXGI_ERROR_WAS_STILL_DRAWING` with zero stall.
    fn poll_async_palette_readback(&mut self) -> bool {
        const SW: u32 = 64;
        const SH: u32 = 36;
        let Some((slot, smooth)) = self.palette_slots.oldest_in_flight() else {
            return false;
        };
        let (Some(ref dev), Some(ref st_tex)) = (&self.d3d_device, &self.sample_st_tex[slot])
        else {
            self.palette_slots.mark_completed(slot);
            return false;
        };
        unsafe {
            let Ok(ctx) = dev.GetImmediateContext() else {
                return false;
            };
            let Ok(st_res) = st_tex.cast::<ID3D11Resource>() else {
                self.palette_slots.mark_completed(slot);
                return false;
            };
            let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
            if let Err(err) = ctx.Map(
                &st_res,
                0,
                D3D11_MAP_READ,
                D3D11_MAP_FLAG_DO_NOT_WAIT.0 as u32,
                Some(&mut mapped),
            ) {
                if err.code().0 != DXGI_ERROR_WAS_STILL_DRAWING_HR {
                    self.palette_slots.mark_completed(slot);
                }
                return false;
            }

            let row_bytes = (SW * 4) as usize;
            let mut bgra = vec![0u8; row_bytes * (SH as usize)];
            let pitch = mapped.RowPitch as usize;
            let src_ptr = mapped.pData as *const u8;
            if !src_ptr.is_null() && pitch >= row_bytes {
                for row in 0..(SH as usize) {
                    let s_slice = std::slice::from_raw_parts(src_ptr.add(row * pitch), row_bytes);
                    bgra[row * row_bytes..(row + 1) * row_bytes].copy_from_slice(s_slice);
                }
            }
            ctx.Unmap(&st_res, 0);
            self.palette_slots.mark_completed(slot);
            if smooth {
                crate::theme::update_extracted_live_frame_from_bgra(&self.active_path, &bgra, SW, SH)
            } else {
                crate::theme::update_extracted_palette_from_bgra(&self.active_path, &bgra, SW, SH)
            }
        }
    }

    pub fn cleanup(&mut self) {
        self.set_frame_timer(false);
        self.set_static_retry_timer(false);
        self.static_load_gen = self.static_load_gen.wrapping_add(1);
        self.static_load_state = StaticLoadState::Idle;
        self.static_req_tx = None;
        self.static_res_rx = None;
        self.pending_static_frame = None;
        self.static_needs_represent = false;
        self.last_static_palette_bgra = None;
        self.cached_path_update = None;
        self.enabled = false;
        let reentrant_mf = self.is_in_mf_call();
        unsafe {
            if GLOBAL_WP_PTR == self as *mut _ {
                GLOBAL_WP_PTR = std::ptr::null_mut();
            }
            if !self.video_hwnd.0.is_null() {
                let _ = ShowWindow(self.video_hwnd, SW_HIDE);
                if reentrant_mf {
                    let _ = SetParent(self.video_hwnd, None);
                } else {
                    if let Ok(p) = GetParent(self.video_hwnd) {
                        if !p.0.is_null() {
                            let _ = SetParent(self.video_hwnd, None);
                        }
                    }
                    let _ = DestroyWindow(self.video_hwnd);
                }
                self.video_hwnd = HWND::default();
            }
            if !self.hwnd.0.is_null() {
                let _ = ShowWindow(self.hwnd, SW_HIDE);
                if reentrant_mf {
                    // Detach from Progman immediately so Explorer never holds a frozen child HWND.
                    let _ = SetParent(self.hwnd, None);
                } else {
                    let _ = DestroyWindow(self.hwnd);
                }
                self.hwnd = HWND::default();
            }
        }
        self.backdrop_dib = None;
        self.static_mode = false;
        self.clear_sample_textures();
        self.has_presented_frame = false;
        self.swap_chain_has_frame = false;
        self.pending_cover_pause = false;
        if reentrant_mf {
            // Entered re-entrantly via combase STA PeekMessageW while an outer IMFMediaEngine
            // call still holds its CriticalSection down-stack: skip eng.Shutdown()/MFShutdown()
            // and forget active COM/D3D handles so exit never self-deadlocks or UAFs on unwind.
            if let Some(eng) = self.engine.take() {
                std::mem::forget(eng);
            }
            if let Some(cb) = self._cb.take() {
                std::mem::forget(cb);
            }
            if let Some(sc) = self.swap_chain.take() {
                std::mem::forget(sc);
            }
            self.frame_latency_waitable = HANDLE::default();
            if let Some(mgr) = self.dxgi_mgr.take() {
                std::mem::forget(mgr);
            }
            if let Some(dev) = self.d3d_device.take() {
                std::mem::forget(dev);
            }
            self.mf_started = false;
            return;
        }
        self.stop_player_only();
        self.drop_swap_chain();
        self.dxgi_mgr = None;
        self.trim_gpu_memory();
        self.d3d_device = None;
        self.device_has_video_support = false;
        self.decode_mode = VideoDecodeMode::Unknown;
        unsafe {
            if self.mf_started {
                let _ = MFShutdown();
                self.mf_started = false;
            }
        }
    }

    /// Non-blocking stop for OS logoff/shutdown (`WM_ENDSESSION`): skips synchronous
    /// `MFShutdown()` and `DestroyWindow` cross-process messages against `WorkerW` / `explorer.exe`.
    pub fn stop_for_os_exit(&mut self) {
        WP_OS_EXIT.store(true, Ordering::Release);
        if let Some(stop) = self.pacer_stop.take() {
            stop.store(true, Ordering::Release);
        }
        self.mf_started = false;
        self.static_req_tx = None;
        self.static_res_rx = None;
        self.pending_static_frame = None;
        self.engine = None;
        self.drop_swap_chain();
        self.d3d_device = None;
        self.backdrop_dib = None;
        let vid_hw = self.video_hwnd;
        self.video_hwnd = HWND::default();
        unsafe {
            for hw in [vid_hw, self.hwnd] {
                if !hw.0.is_null() {
                    let _ = KillTimer(Some(hw), TIMER_WP_FRAME);
                    let _ = KillTimer(Some(hw), TIMER_WP_STATIC_RETRY);
                    let _ = SetWindowPos(
                        hw,
                        None,
                        0,
                        0,
                        0,
                        0,
                        SWP_HIDEWINDOW
                            | SWP_NOACTIVATE
                            | SWP_NOMOVE
                            | SWP_NOSIZE
                            | SWP_NOZORDER
                            | SWP_ASYNCWINDOWPOS,
                    );
                }
            }
            if GLOBAL_WP_PTR == self as *mut _ {
                GLOBAL_WP_PTR = std::ptr::null_mut();
            }
        }
    }
}

impl Drop for VideoWallpaperEngine {
    fn drop(&mut self) {
        if !WP_OS_EXIT.load(Ordering::Acquire) {
            self.cleanup();
        }
    }
}

unsafe extern "system" fn wp_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut VideoWallpaperEngine;
    let wp = if !ptr.is_null() {
        &mut *ptr
    } else if !GLOBAL_WP_PTR.is_null() {
        &mut *GLOBAL_WP_PTR
    } else {
        return DefWindowProcW(hwnd, msg, wparam, lparam);
    };

    match msg {
        WM_WP_PUMP => {
            wp.pump_queued.store(false, Ordering::Release);
            if wp.enabled && !wp.paused_by_cover && !WP_OS_EXIT.load(Ordering::Acquire) {
                wp.pump_frame(false);
            }
            LRESULT(0)
        }
        WM_TIMER if wparam.0 == TIMER_WP_FRAME => {
            if wp.enabled && !wp.paused_by_cover && !WP_OS_EXIT.load(Ordering::Acquire) {
                wp.pump_frame(false);
            }
            LRESULT(0)
        }
        WM_TIMER if wparam.0 == TIMER_WP_STATIC_RETRY => {
            if !WP_OS_EXIT.load(Ordering::Acquire) {
                wp.retry_static_represent();
            }
            LRESULT(0)
        }
        WM_WP_STATIC_READY => {
            if !WP_OS_EXIT.load(Ordering::Acquire) {
                let _ = wp.drain_static_load_results();
            }
            LRESULT(0)
        }
        WM_WP_EVENT => {
            if WP_OS_EXIT.load(Ordering::Acquire) || wp.is_in_mf_call() {
                return LRESULT(0);
            }
            let ev = wparam.0 as i32;
            if ev == MF_MEDIA_ENGINE_EVENT_ENDED.0 {
                if let Some(eng) = wp.engine.clone() {
                    let _mf = MfCallGuard::enter();
                    let _ = eng.SetCurrentTime(0.0);
                    let _ = eng.Play();
                }
            } else if ev == MF_MEDIA_ENGINE_EVENT_LOADEDDATA.0 {
                if wp.enabled && !wp.paused_by_cover {
                    if let Some(eng) = wp.engine.clone() {
                        let _mf = MfCallGuard::enter();
                        if wp.pending_resume_sec > 0.1 {
                            let sec = wp.pending_resume_sec;
                            wp.pending_resume_sec = 0.0;
                            let _ = eng.SetCurrentTime(sec);
                        }
                        let _ = eng.SetMuted(wp.mute || wp.volume_pct == 0);
                        let _ = eng.SetVolume((wp.volume_pct as f64 / 100.0).clamp(0.0, 1.0));
                        let _ = eng.Play();
                    }
                }
            } else if ev == MF_MEDIA_ENGINE_EVENT_FIRSTFRAMEREADY.0 {
                if wp.enabled && !wp.paused_by_cover {
                    if let Some(eng) = wp.engine.clone() {
                        let _mf = MfCallGuard::enter();
                        if wp.pending_resume_sec > 0.1 {
                            let sec = wp.pending_resume_sec;
                            wp.pending_resume_sec = 0.0;
                            let _ = eng.SetCurrentTime(sec);
                        }
                        let _ = eng.SetMuted(wp.mute || wp.volume_pct == 0);
                        let _ = eng.SetVolume((wp.volume_pct as f64 / 100.0).clamp(0.0, 1.0));
                        let _ = eng.Play();
                    }
                    wp.pump_frame(true);
                }
            }
            LRESULT(0)
        }
        WM_SIZE => {
            if !wp.static_mode {
                wp.pump_frame(true);
            }
            LRESULT(0)
        }
        WM_DISPLAYCHANGE => {
            if !WP_OS_EXIT.load(Ordering::Acquire) {
                wp.handle_display_change();
            }
            LRESULT(0)
        }
        0x000F => {
            let mut ps = PAINTSTRUCT::default();
            let hdc = BeginPaint(hwnd, &mut ps);
            if !hdc.0.is_null() {
                if let Some(ref dib) = wp.backdrop_dib {
                    let mem_dc = CreateCompatibleDC(Some(hdc));
                    if !mem_dc.0.is_null() {
                        let old = SelectObject(mem_dc, HGDIOBJ(dib.hbmp.0));
                        let dst_w = wp.screen_w.max(1);
                        let dst_h = wp.screen_h.max(1);
                        let painted = if dib.w == dst_w && dib.h == dst_h {
                            BitBlt(hdc, 0, 0, dst_w, dst_h, Some(mem_dc), 0, 0, SRCCOPY).is_ok()
                        } else {
                            let _ = SetStretchBltMode(hdc, HALFTONE);
                            StretchBlt(
                                hdc,
                                0,
                                0,
                                dst_w,
                                dst_h,
                                Some(mem_dc),
                                0,
                                0,
                                dib.w,
                                dib.h,
                                SRCCOPY,
                            )
                            .as_bool()
                        };
                        if painted {
                            wp.backdrop_paint_count = wp.backdrop_paint_count.saturating_add(1);
                        }
                        let _ = SelectObject(mem_dc, old);
                        let _ = DeleteDC(mem_dc);
                    }
                }
            }
            let _ = EndPaint(hwnd, &ps);
            LRESULT(0)
        }
        WM_ERASEBKGND => LRESULT(1),
        0x0010 => LRESULT(0),
        0x0002 => {
            if let Some(stop) = wp.pacer_stop.take() {
                stop.store(true, Ordering::Release);
            }
            wp.pump_queued.store(false, Ordering::Release);
            wp.timer_on = false;
            wp.static_retry_timer_on = false;
            wp.video_hwnd = HWND::default();
            wp.hwnd = HWND::default();
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

unsafe extern "system" fn wp_surface_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        0x000F => {
            let mut ps = PAINTSTRUCT::default();
            let _ = BeginPaint(hwnd, &mut ps);
            let _ = EndPaint(hwnd, &ps);
            LRESULT(0)
        }
        WM_ERASEBKGND => LRESULT(1),
        0x0010 => LRESULT(0),
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mf_call_guard_tracks_nested_reentrancy() {
        assert_eq!(MF_CALL_DEPTH.load(Ordering::Acquire), 0);
        {
            let _outer = MfCallGuard::enter();
            assert_eq!(MF_CALL_DEPTH.load(Ordering::Acquire), 1);
            {
                let _inner = MfCallGuard::enter();
                assert_eq!(MF_CALL_DEPTH.load(Ordering::Acquire), 2);
            }
            assert_eq!(MF_CALL_DEPTH.load(Ordering::Acquire), 1);
        }
        assert_eq!(MF_CALL_DEPTH.load(Ordering::Acquire), 0);
    }

    #[test]
    fn phase3_aspect_crop_static_image_dib_and_thumbnail_decode() {
        // 1. Media-type classifiers distinguish video vs static image formats cleanly
        assert!(crate::config::is_static_image_wallpaper_path("C:\\wp\\sunset.png"));
        assert!(crate::config::is_static_image_wallpaper_path("C:\\wp\\coast.JPG"));
        assert!(crate::config::is_static_image_wallpaper_path("C:\\wp\\forest.webp"));
        assert!(crate::config::is_video_wallpaper_path("C:\\wp\\rain.mp4"));
        assert!(!crate::config::is_static_image_wallpaper_path("C:\\wp\\rain.mp4"));
        assert!(crate::config::is_supported_wallpaper_path("C:\\wp\\forest.webp"));

        // 2. Aspect-fill crop preserves target ratio without distortion
        let (x, y, cw, ch) = compute_aspect_fill_crop(1920, 1080, 1920, 1200);
        assert_eq!(y, 0);
        assert_eq!(ch, 1080);
        assert_eq!(cw, 1728);
        assert_eq!(x, 96);

        // 3. Create a real 16x12 24-bit BMP in TEMP and verify WIC thumbnail + StaticDib decode
        let tmp = std::env::temp_dir().join("autumn_phase3_test_wp.bmp");
        let w = 16u32;
        let h = 12u32;
        let row_stride = w * 3;
        let img_bytes = row_stride * h;
        let file_size = 54 + img_bytes;
        let mut bmp = Vec::with_capacity(file_size as usize);
        bmp.extend_from_slice(b"BM");
        bmp.extend_from_slice(&file_size.to_le_bytes());
        bmp.extend_from_slice(&0u32.to_le_bytes());
        bmp.extend_from_slice(&54u32.to_le_bytes());
        bmp.extend_from_slice(&40u32.to_le_bytes());
        bmp.extend_from_slice(&(w as i32).to_le_bytes());
        bmp.extend_from_slice(&(h as i32).to_le_bytes());
        bmp.extend_from_slice(&1u16.to_le_bytes());
        bmp.extend_from_slice(&24u16.to_le_bytes());
        bmp.extend_from_slice(&0u32.to_le_bytes());
        bmp.extend_from_slice(&img_bytes.to_le_bytes());
        bmp.extend_from_slice(&2835u32.to_le_bytes());
        bmp.extend_from_slice(&2835u32.to_le_bytes());
        bmp.extend_from_slice(&0u32.to_le_bytes());
        bmp.extend_from_slice(&0u32.to_le_bytes());
        for _ in 0..(w * h) {
            bmp.extend_from_slice(&[40u8, 120u8, 220u8]); // BGR warm amber
        }
        std::fs::write(&tmp, &bmp).expect("write test bmp");

        let path_str = tmp.to_string_lossy().to_string();
        let thumb = decode_wallpaper_thumbnail_bgra(&path_str, 48, 32)
            .expect("WIC thumbnail decode must succeed on BMP");
        assert_eq!(thumb.len(), 48 * 32 * 4);
        assert_eq!(thumb[3], 255, "Thumbnail pixels must be opaque BGRA");
        assert!(thumb[2] > 180 && thumb[0] < 80, "Decoded BGR color must preserve warm amber");

        let (bgra_img, iw, ih) =
            load_static_image_bgra(&path_str, 64, 48).expect("Static image BGRA decode must succeed");
        assert_eq!((iw, ih), (64, 48));
        assert!(is_non_blank_bgra(&bgra_img, iw, ih));
        let dib = StaticDib::from_bgra_top_down(iw, ih, &bgra_img).expect("StaticDib load must succeed");
        assert_eq!(dib.w, 64);
        assert_eq!(dib.h, 48);
        drop(dib);

        // 4. Status text reflects honest static image states (never false Active when unpresented)
        let mut eng = VideoWallpaperEngine::new(HINSTANCE::default());
        assert!(!eng.trim_on_pause, "Reclaim RAM When Covered must default OFF");
        eng.enabled = true;
        eng.active_path = path_str.clone();
        eng.static_mode = true;
        eng.static_load_state = StaticLoadState::Loading;
        assert_eq!(eng.status_text(), "Loading Static Image...");
        eng.static_load_state = StaticLoadState::Failed;
        assert_eq!(eng.status_text(), "Image Load Failed — Check File");
        eng.static_load_state = StaticLoadState::Ready;
        eng.has_presented_frame = false;
        assert_eq!(eng.status_text(), "Static Surface Not Presented");
        eng.has_presented_frame = true;
        assert_eq!(eng.status_text(), "Active (Static Image • 0% GPU)");
        eng.static_needs_represent = true;
        assert_eq!(eng.status_text(), "Active (Static Image • Syncing)");
        eng.static_needs_represent = false;
        assert!(!eng.is_active(), "Static image should report is_active=false (0 video animation)");

        eng.static_mode = false;
        eng.paused_by_cover = true;
        eng.pause_trimmed = true;
        assert_eq!(eng.status_text(), "Paused — RAM Reclaimed (SwapChain)");

        // 5. Blank-frame rejection & opaque 24-bpp StaticDib conversion (even when alpha=0x00 from MF)
        let black = vec![0u8; 32 * 24 * 4];
        assert!(!is_non_blank_bgra(&black, 32, 24), "All-zero frame must be rejected");
        let mut mf_zero_alpha = vec![0u8; 13 * 9 * 4];
        for px in mf_zero_alpha.chunks_exact_mut(4) {
            px[0] = 35;
            px[1] = 110;
            px[2] = 210;
            px[3] = 0; // Media Foundation TransferVideoFrame often writes alpha=0x00
        }
        assert!(is_non_blank_bgra(&mf_zero_alpha, 13, 9), "Visible RGB with alpha=0x00 must be accepted");
        let dib24 = StaticDib::from_bgra_top_down(13, 9, &mf_zero_alpha)
            .expect("24-bpp StaticDib must handle non-4-aligned odd width");
        assert_eq!((dib24.w, dib24.h), (13, 9));
        drop(dib24);

        let _ = std::fs::remove_file(&tmp);
    }

    #[test]
    fn static_image_exif_async_error_states_and_single_pass_palette() {
        // 1. EXIF orientation mappings
        assert!(exif_orientation_to_wic_transform(1).is_none());
        assert_eq!(exif_orientation_to_wic_transform(3), Some(WICBitmapTransformRotate180));
        assert_eq!(exif_orientation_to_wic_transform(6), Some(WICBitmapTransformRotate90));
        assert_eq!(exif_orientation_to_wic_transform(8), Some(WICBitmapTransformRotate270));
        assert!(exif_orientation_to_wic_transform(5).is_some());
        assert!(exif_orientation_to_wic_transform(7).is_some());

        // 2. Corrupt image file must fail cleanly (never false "Active")
        let corrupt = std::env::temp_dir().join("autumn_corrupt_wp_test.jpg");
        std::fs::write(&corrupt, b"not a valid jpeg payload 12345").expect("write corrupt test file");
        let corrupt_str = corrupt.to_string_lossy().to_string();
        assert!(
            load_static_image_bgra(&corrupt_str, 320, 180).is_none(),
            "Corrupt JPEG must return None"
        );

        let mut eng = VideoWallpaperEngine::new(HINSTANCE::default());
        let mut cfg = AppConfig::default();
        cfg.wallpaper_enabled = true;
        cfg.wallpaper_path = corrupt_str.clone();
        eng.apply_config_ex(&cfg, true);
        assert!(eng.is_static_loading(), "apply_config_ex must enqueue async decode without blocking");
        assert_eq!(eng.status_text(), "Loading Static Image...");

        let deadline = Instant::now() + Duration::from_secs(4);
        let mut drained = false;
        while Instant::now() < deadline {
            if eng.drain_static_load_results() {
                drained = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(15));
        }
        assert!(drained, "Background worker must deliver failure result");
        assert_eq!(eng.status_text(), "Image Load Failed — Check File");
        assert!(
            !eng.is_visible_on_desktop(),
            "Failed static image must not report visible on desktop"
        );

        // 3. Valid BMP transitions from Failed -> Loading -> Ready and populates single-pass 64x36 palette
        let valid = std::env::temp_dir().join("autumn_valid_static_wp_test.bmp");
        let (w, h) = (32u32, 24u32);
        let img_bytes = w * 3 * h;
        let file_size = 54 + img_bytes;
        let mut bmp = Vec::with_capacity(file_size as usize);
        bmp.extend_from_slice(b"BM");
        bmp.extend_from_slice(&file_size.to_le_bytes());
        bmp.extend_from_slice(&0u32.to_le_bytes());
        bmp.extend_from_slice(&54u32.to_le_bytes());
        bmp.extend_from_slice(&40u32.to_le_bytes());
        bmp.extend_from_slice(&(w as i32).to_le_bytes());
        bmp.extend_from_slice(&(h as i32).to_le_bytes());
        bmp.extend_from_slice(&1u16.to_le_bytes());
        bmp.extend_from_slice(&24u16.to_le_bytes());
        bmp.extend_from_slice(&0u32.to_le_bytes());
        bmp.extend_from_slice(&img_bytes.to_le_bytes());
        bmp.extend_from_slice(&2835u32.to_le_bytes());
        bmp.extend_from_slice(&2835u32.to_le_bytes());
        bmp.extend_from_slice(&0u32.to_le_bytes());
        bmp.extend_from_slice(&0u32.to_le_bytes());
        for _ in 0..(w * h) {
            bmp.extend_from_slice(&[210u8, 140u8, 45u8]); // vivid cobalt-cerulean BGR
        }
        std::fs::write(&valid, &bmp).expect("write valid bmp");
        let valid_str = valid.to_string_lossy().to_string();

        // Dispatch with cache_locally=false directly via worker check using load_static_image_bgra + downsample
        let (bgra, dw, dh) = load_static_image_bgra(&valid_str, 160, 100)
            .expect("Valid BMP must decode via WIC");
        let pal_bgra = downsample_bgra_for_palette(&bgra, dw, dh);
        assert_eq!(pal_bgra.len(), 64 * 36 * 4);
        eng.static_mode = true;
        eng.active_path = valid_str.clone();
        eng.last_static_palette_bgra = Some((valid_str.clone(), pal_bgra));
        assert!(
            eng.resample_live_palette(),
            "Static mode resample_live_palette must work immediately from in-memory 64x36 buffer"
        );

        // 4. Turning wallpaper OFF cleans up static state cleanly
        cfg.wallpaper_enabled = false;
        eng.apply_config(&cfg);
        assert_eq!(eng.status_text(), "Engine Disabled");
        assert!(!eng.is_visible_on_desktop());

        // 5. Real user photo smoke test if cached locally (read-only)
        if let Ok(appdata) = std::env::var("APPDATA") {
            let real_jpg = std::path::PathBuf::from(appdata)
                .join("AutumnDeck")
                .join("wallpapers")
                .join("1720350943337.jpg");
            if real_jpg.is_file() {
                let p = real_jpg.to_string_lossy().to_string();
                let (bgra, rw, rh) =
                    load_static_image_bgra(&p, 640, 400).expect("Real 1720350943337.jpg must decode");
                assert_eq!((rw, rh), (640, 400));
                assert!(is_non_blank_bgra(&bgra, rw, rh));
                let pal = downsample_bgra_for_palette(&bgra, rw, rh);
                assert_eq!(pal.len(), 64 * 36 * 4);
            }
        }

        eng.cleanup();
        let _ = std::fs::remove_file(&corrupt);
        let _ = std::fs::remove_file(&valid);
    }

    #[test]
    fn video_fps_pacing_async_palette_slots_and_decode_telemetry() {
        // 1. Adaptive frame pacer cadence & minimum present guard (60 FPS default, Auto removed)
        assert_eq!(WallpaperFpsCap::default(), WallpaperFpsCap::Fps60);
        assert_eq!(WallpaperFpsCap::from_str("auto"), WallpaperFpsCap::Fps60);
        assert_eq!(compute_target_frame_interval_us(WallpaperFpsCap::Fps60, 144), 16_666);
        assert_eq!(compute_target_frame_interval_us(WallpaperFpsCap::Fps60, 60), 16_666);
        assert_eq!(compute_target_frame_interval_us(WallpaperFpsCap::Fps60, 30), 33_333);
        assert_eq!(compute_target_frame_interval_us(WallpaperFpsCap::Fps30, 144), 33_333);
        assert_eq!(compute_min_present_interval_us(WallpaperFpsCap::Fps60), 13_500);
        assert_eq!(compute_min_present_interval_us(WallpaperFpsCap::Fps30), 29_000);

        // 2. Double-buffered async staging slot FIFO lifecycle
        let mut slots = AsyncPaletteSlots::default();
        assert!(!slots.any_in_flight());
        assert_eq!(slots.oldest_in_flight(), None);
        assert_eq!(slots.acquire_write_slot(), Some(0));

        slots.mark_submitted(0, false);
        assert!(slots.any_in_flight());
        assert_eq!(slots.oldest_in_flight(), Some((0, false)));
        assert_eq!(slots.acquire_write_slot(), Some(1));

        slots.mark_submitted(1, true);
        assert_eq!(slots.acquire_write_slot(), None, "Both staging slots busy");
        assert_eq!(
            slots.oldest_in_flight(),
            Some((0, false)),
            "FIFO must drain slot 0 before slot 1"
        );

        slots.mark_completed(0);
        assert_eq!(slots.oldest_in_flight(), Some((1, true)));
        assert_eq!(slots.acquire_write_slot(), Some(0));

        slots.mark_completed(1);
        assert!(!slots.any_in_flight());
        assert_eq!(slots.oldest_in_flight(), None);

        // 3. Hardware DXVA vs software/WARP decode mode telemetry
        assert!(!VideoDecodeMode::Unknown.is_software_fallback());
        assert!(!VideoDecodeMode::HardwareDxva.is_software_fallback());
        assert!(VideoDecodeMode::SoftwareCpu.is_software_fallback());
        assert!(VideoDecodeMode::WarpSoftware.is_software_fallback());
        assert!(VideoDecodeMode::HardwareDxva.warning_pill_text().is_none());
        assert!(VideoDecodeMode::SoftwareCpu.warning_pill_text().is_some());
        assert!(VideoDecodeMode::WarpSoftware.warning_pill_text().is_some());

        let mut eng = VideoWallpaperEngine::new(HINSTANCE::default());
        eng.decode_mode = VideoDecodeMode::SoftwareCpu;
        assert_eq!(
            eng.decode_mode(),
            VideoDecodeMode::Unknown,
            "Disabled engine must report Unknown decode mode"
        );
        eng.enabled = true;
        eng.static_mode = true;
        assert_eq!(
            eng.decode_mode(),
            VideoDecodeMode::Unknown,
            "Static image mode must not trigger software video decode warnings"
        );
        eng.static_mode = false;
        assert_eq!(eng.decode_mode(), VideoDecodeMode::SoftwareCpu);

        // 4. Persisted WallpaperFpsCap INI roundtrip
        for cap in [WallpaperFpsCap::Fps60, WallpaperFpsCap::Fps30] {
            let mut cfg = AppConfig::default();
            cfg.wallpaper_fps_cap = cap;
            let ini = crate::config::serialize_config_ini(&cfg);
            let parsed = crate::config::parse_config_ini(&ini).expect("parse config ini");
            assert_eq!(parsed.wallpaper_fps_cap, cap);
        }

        // 5. 4K covered auto-reclaim delay and explicit FPS cap cadence
        assert_eq!(compute_cover_reclaim_delay_ms(true, 1920, 1080), 4_000);
        assert_eq!(compute_cover_reclaim_delay_ms(false, 3840, 2160), 8_000);
        assert_eq!(compute_cover_reclaim_delay_ms(false, 2560, 1440), 8_000);
        assert_eq!(compute_cover_reclaim_delay_ms(false, 1920, 1080), 15_000);
        assert_eq!(
            compute_effective_pacer_interval_us(WallpaperFpsCap::Fps60, 60, false),
            16_666
        );
        assert_eq!(
            compute_effective_pacer_interval_us(WallpaperFpsCap::Fps60, 60, true),
            16_666,
            "Explicit 60 FPS cap must never be overridden down to 30 FPS"
        );
        assert_eq!(
            compute_effective_pacer_interval_us(WallpaperFpsCap::Fps30, 60, true),
            33_333
        );
    }
}
