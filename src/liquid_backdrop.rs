use std::ffi::c_void;
use std::mem::size_of;
use std::sync::Once;

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
    SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SWP_SHOWWINDOW, SW_HIDE, WM_NCHITTEST,
    WNDCLASSEXW, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
};

const ACRYLIC_BACKDROP_CLASS: PCWSTR = w!("AutumnDeckGlassAcrylicWnd");
const ACCENT_ENABLE_ACRYLICBLURBEHIND: u32 = 4;
const WCA_ACCENT_POLICY: u32 = 19;
const DWMWA_COLOR_NONE_U32: u32 = 0xFFFFFFFE;
const MAX_ACRYLIC_PILLS: usize = 4;

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

    fn apply_acrylic(&self, hwnd: HWND, abgr: u32) {
        let Some(swca) = self.swca_fn else {
            return;
        };
        // Windows 11 DWM Acrylic ignores alpha == 0; clamp alpha into [8, 220] for crystal translucency.
        let a = ((abgr >> 24) & 0xFF).clamp(8, 220);
        let safe_abgr = (a << 24) | (abgr & 0x00FF_FFFF);
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
    last_owner: HWND,
    live_active: bool,
}

impl Default for LiquidBackdropWindow {
    fn default() -> Self {
        Self::new()
    }
}

impl LiquidBackdropWindow {
    pub fn new() -> Self {
        Self {
            hwnds: Vec::new(),
            visible_count: 0,
            last_pills: Vec::new(),
            last_abgr: 0,
            last_owner: HWND::default(),
            live_active: false,
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
        let need_accent = grew || self.last_abgr != abgr_tint || !self.live_active;
        let need_layout = grew
            || self.visible_count != active
            || self.last_pills.as_slice() != slice
            || self.last_owner != owner_fg_hwnd;

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
                ctx.apply_acrylic(hw, abgr_tint);
            }
            if need_layout {
                let rc = pill.screen_rect;
                let w = (rc.right - rc.left).max(4);
                let h = (rc.bottom - rc.top).max(4);
                let was_shown = i < self.visible_count;
                let flags = if was_shown && self.last_owner == owner_fg_hwnd {
                    // Preserve z-order on smooth resize/move without triggering a DWM z-order storm
                    SWP_NOACTIVATE | SWP_NOZORDER
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
        self.last_pills.clear();
        self.last_pills.extend_from_slice(slice);
        self.last_abgr = abgr_tint;
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
        win.destroy();
        unsafe {
            let _ = DestroyWindow(owner);
        }
        assert!(!win.is_live_active());
    }
}
