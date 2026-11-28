use std::ffi::c_void;
use std::mem::size_of;
use std::sync::Once;
use std::time::{Duration, Instant};

use windows::core::{s, w, PCWSTR};
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Dwm::{
    DwmExtendFrameIntoClientArea, DwmSetWindowAttribute, DWMWA_BORDER_COLOR,
    DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_DONOTROUND, DWMWCP_ROUND,
};
use windows::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
use windows::Win32::UI::Controls::MARGINS;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, RegisterClassExW, SetWindowPos, ShowWindow,
    SWP_ASYNCWINDOWPOS, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SWP_SHOWWINDOW,
    SW_HIDE, WM_NCHITTEST, WNDCLASSEXW, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
    WS_EX_TRANSPARENT, WS_POPUP,
};

const ACRYLIC_BACKDROP_CLASS: PCWSTR = w!("AutumnDeckGlassAcrylicWnd");
const ACCENT_ENABLE_ACRYLICBLURBEHIND: u32 = 4;
const WCA_ACCENT_POLICY: u32 = 19;
const DWMWA_COLOR_NONE_U32: u32 = 0xFFFFFFFE;
const MAX_ACRYLIC_PILLS: usize = 4;

/// Controls which Windows 11 DWM composition tint profile sits behind a glass dock/bar:
/// - `ClearBlur`: hardware rounded-corner `ACCENT_ENABLE_ACRYLICBLURBEHIND` at alpha `1`, yielding
///   pure real-time 30px Gaussian glass blur with zero Win11 milky luminosity haze and crisp
///   `DWMWCP_ROUND` corners (no legacy `ACCENT_ENABLE_BLURBEHIND` square-corner artifacts).
/// - `FrostedAcrylic`: hardware rounded-corner `ACCENT_ENABLE_ACRYLICBLURBEHIND` ramping smoothly
///   from crystal blur (`a = 1`) through translucent macOS glass (`a = 18..48`) to rich frost (`a = 128..224`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BackdropBlurStyle {
    ClearBlur,
    FrostedAcrylic,
}

static REGISTER_ONCE: Once = Once::new();

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
    unsafe extern "system" fn(HWND, *mut WindowCompositionAttribData) -> i32;

unsafe extern "system" fn acrylic_backdrop_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    // CRITICAL: Always return HTTRANSPARENT (-1) so backdrop blur windows can NEVER eat mouse clicks
    // over desktop icons, app title bars, or status bars!
    if msg == WM_NCHITTEST {
        return LRESULT(-1);
    }
    DefWindowProcW(hwnd, msg, wparam, lparam)
}

pub fn register_liquid_backdrop_class(hinst: HINSTANCE) {
    REGISTER_ONCE.call_once(|| unsafe {
        let wc = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(acrylic_backdrop_wnd_proc),
            hInstance: hinst,
            lpszClassName: ACRYLIC_BACKDROP_CLASS,
            ..Default::default()
        };
        let _ = RegisterClassExW(&wc);
    });
}

/// Screen-space rectangle for a live Windows 11 Hardware DWM Acrylic pill/bar backdrop.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct AcrylicPillRect {
    pub screen_rect: RECT,
    pub rounded: bool,
}

/// Shared function-pointer cache for `user32!SetWindowCompositionAttribute`.
/// Pure Win32 with zero WinRT `DispatcherQueueController` hooks -> 0% idle CPU and zero app-switch hangs.
pub struct LiquidBackdropContext {
    swca_fn: Option<SetWindowCompositionAttributeFn>,
    resolved: bool,
}

impl Default for LiquidBackdropContext {
    fn default() -> Self {
        Self::new()
    }
}

impl LiquidBackdropContext {
    pub fn new() -> Self {
        Self {
            swca_fn: None,
            resolved: false,
        }
    }

    pub fn ensure_init(&mut self) -> bool {
        if !self.resolved {
            self.resolved = true;
            self.swca_fn = unsafe {
                GetModuleHandleW(w!("user32.dll"))
                    .ok()
                    .and_then(|m| GetProcAddress(m, s!("SetWindowCompositionAttribute")))
                    .map(|addr| std::mem::transmute::<_, SetWindowCompositionAttributeFn>(addr))
            };
        }
        self.swca_fn.is_some()
    }

    fn apply_backdrop(&self, hwnd: HWND, abgr: u32, style: BackdropBlurStyle) {
        let Some(swca) = self.swca_fn else {
            return;
        };
        // Windows 11 DWM Acrylic disables transparency only when alpha == 0; alpha == 1 gives pure
        // real-time Gaussian blur with crisp `DWMWCP_ROUND` corners and zero milky luminosity wash!
        let safe_abgr = match style {
            BackdropBlurStyle::ClearBlur => (1u32 << 24) | (abgr & 0x00FF_FFFF),
            BackdropBlurStyle::FrostedAcrylic => {
                let a = ((abgr >> 24) & 0xFF).clamp(1, 224);
                (a << 24) | (abgr & 0x00FF_FFFF)
            }
        };
        let mut p = AccentPolicy {
            accent_state: ACCENT_ENABLE_ACRYLICBLURBEHIND,
            accent_flags: 2,
            gradient_color: safe_abgr,
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
}

/// Pool of 1..4 native Windows 11 DWM Acrylic hardware-blur windows positioned directly behind a
/// foreground Direct2D glass dock, top bar, or popover window.
///
/// Each blur HWND is strictly bounded to its visible glass pill (never oversized canvas headroom),
/// uses `WS_EX_TRANSPARENT | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW` + `WM_NCHITTEST => HTTRANSPARENT`,
/// and only touches `SetWindowPos` when geometry/visibility actually changes (zero z-order storms).
pub struct LiquidBackdropWindow {
    hwnds: Vec<HWND>,
    visible_count: usize,
    last_pills: Vec<AcrylicPillRect>,
    last_abgr: u32,
    last_style: BackdropBlurStyle,
    last_owner: HWND,
    live_active: bool,
    last_resize_at: Option<Instant>,
}

impl Default for LiquidBackdropWindow {
    fn default() -> Self {
        Self::new()
    }
}

/// Returns `true` when an intermediate Fish-Eye hover frame can safely defer resizing
/// Windows 11 `ACCENT_ENABLE_ACRYLICBLURBEHIND` HWNDs so `dwm.exe` does not synchronously
/// rebuild the Acrylic blur graph 45-60 times per second during hover motion.
pub(crate) fn should_defer_acrylic_motion_resize(
    motion_active: bool,
    prev: &[AcrylicPillRect],
    next: &[AcrylicPillRect],
    elapsed_since_resize: Option<Duration>,
) -> bool {
    if !motion_active || prev.len() != next.len() || prev.is_empty() {
        return false;
    }
    let mut max_delta = 0i32;
    for (a, b) in prev.iter().zip(next.iter()) {
        if a.rounded != b.rounded {
            return false;
        }
        max_delta = max_delta
            .max((a.screen_rect.left - b.screen_rect.left).abs())
            .max((a.screen_rect.top - b.screen_rect.top).abs())
            .max((a.screen_rect.right - b.screen_rect.right).abs())
            .max((a.screen_rect.bottom - b.screen_rect.bottom).abs());
    }
    if max_delta == 0 {
        return true;
    }
    if max_delta < 4 {
        return true;
    }
    elapsed_since_resize.is_some_and(|e| e < Duration::from_millis(48))
}

impl LiquidBackdropWindow {
    pub fn new() -> Self {
        Self {
            hwnds: Vec::new(),
            visible_count: 0,
            last_pills: Vec::new(),
            last_abgr: 0,
            last_style: BackdropBlurStyle::FrostedAcrylic,
            last_owner: HWND::default(),
            live_active: false,
            last_resize_at: None,
        }
    }

    #[allow(dead_code)]
    pub fn is_live_active(&self) -> bool {
        self.live_active && self.visible_count > 0
    }

    fn create_pill_hwnd(hinst: HINSTANCE) -> HWND {
        register_liquid_backdrop_class(hinst);
        unsafe {
            // Note: Do NOT add WS_EX_LAYERED on ACCENT_ENABLE_ACRYLICBLURBEHIND windows because
            // UpdateLayeredWindow/SetLayeredWindowAttributes disables DWM Acrylic on some builds;
            // click-through is guaranteed by WS_EX_TRANSPARENT + explicit WM_NCHITTEST => -1!
            let hw = CreateWindowExW(
                WS_EX_TOPMOST | WS_EX_TRANSPARENT | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
                ACRYLIC_BACKDROP_CLASS,
                w!("AutumnDeck Glass Blur"),
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
                    (&DWMWA_COLOR_NONE_U32 as *const u32) as *const c_void,
                    4,
                );
            }
            hw
        }
    }

    /// Synchronizes 1..4 real-time Windows 11 DWM Acrylic windows directly behind `owner_fg_hwnd`.
    /// Returns `true` when hardware DWM Acrylic is active behind `owner_fg_hwnd`.
    #[allow(clippy::too_many_arguments)]
    pub fn sync_pills(
        &mut self,
        hinst: HINSTANCE,
        ctx: &mut LiquidBackdropContext,
        owner_fg_hwnd: HWND,
        pills: &[AcrylicPillRect],
        abgr_tint: u32,
        want_visible: bool,
    ) -> bool {
        self.sync_pills_ex(hinst, ctx, owner_fg_hwnd, pills, abgr_tint, want_visible, false)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn sync_pills_ex(
        &mut self,
        hinst: HINSTANCE,
        ctx: &mut LiquidBackdropContext,
        owner_fg_hwnd: HWND,
        pills: &[AcrylicPillRect],
        abgr_tint: u32,
        want_visible: bool,
        motion_active: bool,
    ) -> bool {
        self.sync_pills_styled(
            hinst,
            ctx,
            owner_fg_hwnd,
            pills,
            abgr_tint,
            BackdropBlurStyle::FrostedAcrylic,
            want_visible,
            motion_active,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn sync_pills_styled(
        &mut self,
        hinst: HINSTANCE,
        ctx: &mut LiquidBackdropContext,
        owner_fg_hwnd: HWND,
        pills: &[AcrylicPillRect],
        abgr_tint: u32,
        blur_style: BackdropBlurStyle,
        want_visible: bool,
        motion_active: bool,
    ) -> bool {
        let count = pills.len().min(MAX_ACRYLIC_PILLS);
        if !want_visible || count == 0 || owner_fg_hwnd.0.is_null() || !ctx.ensure_init() {
            self.hide();
            return false;
        }
        let prev_pool = self.hwnds.len();
        while self.hwnds.len() < count {
            let hw = Self::create_pill_hwnd(hinst);
            if hw.0.is_null() {
                break;
            }
            self.hwnds.push(hw);
        }
        let active = count.min(self.hwnds.len());
        if active == 0 {
            self.hide();
            return false;
        }

        // Hide any surplus pooled HWNDs
        for &hw in &self.hwnds[active..] {
            if !hw.0.is_null() {
                unsafe {
                    let _ = ShowWindow(hw, SW_HIDE);
                }
            }
        }

        let slice = &pills[..active];
        let grew = self.hwnds.len() > prev_pool;
        let need_accent =
            grew || self.last_abgr != abgr_tint || self.last_style != blur_style || !self.live_active;
        let structural_change =
            grew || self.visible_count != active || self.last_owner != owner_fg_hwnd || !self.live_active;
        let defer_resize = !structural_change
            && should_defer_acrylic_motion_resize(
                motion_active,
                self.last_pills.as_slice(),
                slice,
                self.last_resize_at.map(|t| t.elapsed()),
            );
        let need_layout = structural_change || (!defer_resize && self.last_pills.as_slice() != slice);

        for (i, pill) in slice.iter().enumerate() {
            let hw = self.hwnds[i];
            let prev_pill = self.last_pills.get(i).copied();
            if grew || prev_pill.map(|p| p.rounded) != Some(pill.rounded) || !self.live_active {
                let pref = if pill.rounded {
                    DWMWCP_ROUND
                } else {
                    DWMWCP_DONOTROUND
                };
                unsafe {
                    let _ = DwmSetWindowAttribute(
                        hw,
                        DWMWA_WINDOW_CORNER_PREFERENCE,
                        (&pref as *const _) as *const c_void,
                        size_of::<u32>() as u32,
                    );
                }
            }
            if need_accent {
                ctx.apply_backdrop(hw, abgr_tint, blur_style);
            }
            if need_layout {
                let rc = pill.screen_rect;
                let w = (rc.right - rc.left).max(4);
                let h = (rc.bottom - rc.top).max(4);
                let was_shown = i < self.visible_count;
                let flags = if was_shown && self.last_owner == owner_fg_hwnd {
                    // Use SWP_ASYNCWINDOWPOS on resize/move so Windows 11 DWM Acrylic never
                    // synchronously stalls the dock UI thread during Fish-Eye magnification.
                    SWP_NOACTIVATE | SWP_NOZORDER | SWP_ASYNCWINDOWPOS
                } else {
                    SWP_NOACTIVATE | SWP_SHOWWINDOW
                };
                unsafe {
                    let _ = SetWindowPos(
                        hw,
                        Some(owner_fg_hwnd),
                        rc.left,
                        rc.top,
                        w,
                        h,
                        flags,
                    );
                }
            }
        }

        self.visible_count = active;
        if need_layout {
            self.last_pills.clear();
            self.last_pills.extend_from_slice(slice);
            self.last_resize_at = Some(Instant::now());
        }
        self.last_abgr = abgr_tint;
        self.last_style = blur_style;
        self.last_owner = owner_fg_hwnd;
        self.live_active = true;
        true
    }

    /// Ensures the blur windows sit strictly beneath `owner_fg_hwnd` (called only when `owner_fg_hwnd`
    /// itself is newly shown, NOT on every repaint!).
    pub fn ensure_behind(&mut self, owner_fg_hwnd: HWND) {
        if self.visible_count == 0 || owner_fg_hwnd.0.is_null() {
            return;
        }
        for &hw in &self.hwnds[..self.visible_count] {
            if !hw.0.is_null() {
                unsafe {
                    let _ = SetWindowPos(
                        hw,
                        Some(owner_fg_hwnd),
                        0,
                        0,
                        0,
                        0,
                        SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE,
                    );
                }
            }
        }
        self.last_owner = owner_fg_hwnd;
    }

    pub fn hide(&mut self) {
        if self.visible_count > 0 {
            for &hw in &self.hwnds {
                if !hw.0.is_null() {
                    unsafe {
                        let _ = ShowWindow(hw, SW_HIDE);
                    }
                }
            }
        }
        self.visible_count = 0;
        self.live_active = false;
    }

    pub fn destroy(&mut self) {
        self.hide();
        for hw in self.hwnds.drain(..) {
            if !hw.0.is_null() {
                unsafe {
                    let _ = DestroyWindow(hw);
                }
            }
        }
        self.last_pills.clear();
        self.last_abgr = 0;
        self.last_owner = HWND::default();
    }
}

impl Drop for LiquidBackdropWindow {
    fn drop(&mut self) {
        self.destroy();
    }
}

/// Packs `(r, g, b, alpha)` into Windows `SetWindowCompositionAttribute` `AABBGGRR` byte order.
#[inline]
pub const fn pack_abgr(r: u8, g: u8, b: u8, a: u8) -> u32 {
    ((a as u32) << 24) | ((b as u32) << 16) | ((g as u32) << 8) | (r as u32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;

    #[test]
    fn dwm_acrylic_backdrop_is_click_through_and_initializes_cleanly() {
        let hinst = unsafe {
            GetModuleHandleW(None)
                .map(|m| HINSTANCE(m.0))
                .unwrap_or_default()
        };
        let mut ctx = LiquidBackdropContext::new();
        assert!(
            ctx.ensure_init(),
            "user32!SetWindowCompositionAttribute must resolve on Windows 11"
        );
        // Verify click-through WM_NCHITTEST returns HTTRANSPARENT (-1)
        let ht = unsafe {
            acrylic_backdrop_wnd_proc(HWND::default(), WM_NCHITTEST, WPARAM(0), LPARAM(0))
        };
        assert_eq!(ht, LRESULT(-1), "Acrylic backdrop window MUST always return HTTRANSPARENT");

        let mut win = LiquidBackdropWindow::new();
        let owner = LiquidBackdropWindow::create_pill_hwnd(hinst);
        assert!(!owner.0.is_null());
        let pills = [AcrylicPillRect {
            screen_rect: RECT {
                left: -4000,
                top: -4000,
                right: -3600,
                bottom: -3960,
            },
            rounded: true,
        }];
        let ok = win.sync_pills(hinst, &mut ctx, owner, &pills, pack_abgr(240, 246, 255, 28), true);
        assert!(ok);
        assert!(win.is_live_active());
        // Verify smooth style switch to ClearBlur (ACCENT_ENABLE_BLURBEHIND) for crystal macOS glass
        let ok_clear = win.sync_pills_styled(
            hinst,
            &mut ctx,
            owner,
            &pills,
            pack_abgr(228, 234, 244, 16),
            BackdropBlurStyle::ClearBlur,
            true,
            false,
        );
        assert!(ok_clear);
        assert_eq!(win.last_style, BackdropBlurStyle::ClearBlur);
        win.destroy();
        unsafe {
            let _ = DestroyWindow(owner);
        }
        assert!(!win.is_live_active());
    }

    #[test]
    fn acrylic_motion_resize_defers_intermediate_hover_frames_and_snaps_on_settle() {
        let p0 = [AcrylicPillRect {
            screen_rect: RECT { left: 100, top: 900, right: 700, bottom: 964 },
            rounded: true,
        }];
        let p_small = [AcrylicPillRect {
            screen_rect: RECT { left: 98, top: 900, right: 702, bottom: 964 },
            rounded: true,
        }];
        let p_big = [AcrylicPillRect {
            screen_rect: RECT { left: 80, top: 900, right: 720, bottom: 964 },
            rounded: true,
        }];

        assert!(should_defer_acrylic_motion_resize(
            true,
            &p0,
            &p_small,
            Some(Duration::from_millis(100)),
        ));
        assert!(should_defer_acrylic_motion_resize(
            true,
            &p0,
            &p_big,
            Some(Duration::from_millis(16)),
        ));
        assert!(!should_defer_acrylic_motion_resize(
            true,
            &p0,
            &p_big,
            Some(Duration::from_millis(60)),
        ));
        assert!(!should_defer_acrylic_motion_resize(
            false,
            &p0,
            &p_small,
            Some(Duration::from_millis(5)),
        ));
    }
}
