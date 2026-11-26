use crate::config::to_wide;
use std::ffi::c_void;
use std::mem::size_of;
use windows::core::{BOOL, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, RECT, WPARAM};
use windows::Win32::Graphics::Dwm::{
    DwmGetWindowAttribute, DwmSetWindowAttribute, DWMWA_CLOAK, DWMWA_CLOAKED,
    DWMWA_EXTENDED_FRAME_BOUNDS,
};
use windows::Win32::Graphics::Gdi::{
    EnumDisplayMonitors, GetMonitorInfoW, InvalidateRect, MonitorFromPoint, MonitorFromWindow,
    RedrawWindow, SetWindowRgn, HDC, HMONITOR, MONITORINFO, MONITOR_DEFAULTTOPRIMARY,
    MONITOR_DEFAULTTONEAREST, RDW_ALLCHILDREN, RDW_FRAME, RDW_INVALIDATE,
};
use windows::Win32::UI::Shell::{
    SHQueryUserNotificationState, QUNS_BUSY, QUNS_RUNNING_D3D_FULL_SCREEN,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, FindWindowW, GetClassNameW, GetForegroundWindow, GetGUIThreadInfo, GetWindow,
    GetWindowLongW, GetWindowRect, GetWindowTextLengthW,
    GetWindowThreadProcessId, IsIconic, IsWindowVisible, IsZoomed, SendMessageTimeoutW,
    SendNotifyMessageW, SetForegroundWindow, SetWindowPos, ShowWindow,
    SystemParametersInfoW, GUITHREADINFO, GUI_INMENUMODE, GUI_POPUPMENUMODE, GWL_EXSTYLE,
    GWL_STYLE, GW_OWNER, HWND_BROADCAST, HWND_TOPMOST, SMTO_ABORTIFHUNG, SPI_SETWORKAREA,
    SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSENDCHANGING, SWP_NOSIZE, SWP_NOZORDER,
    SWP_SHOWWINDOW, SW_HIDE, SW_SHOWNA, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS,
    WM_SETTINGCHANGE, WS_CAPTION, WS_EX_APPWINDOW, WS_EX_NOACTIVATE,
    WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT,
};

const SPI_SETWORKAREA_PARAM: usize = 47;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TaskbarEdge {
    Left,
    Right,
    Top,
    Bottom,
}

#[derive(Clone, Copy)]
pub struct TaskbarEdgeGeometry {
    pub edge: TaskbarEdge,
    pub mon_rect: RECT,
    pub bar_rect: RECT,
}

#[derive(Clone, Copy)]
pub struct MonitorWorkState {
    pub orig_work: RECT,
    pub rc_monitor: RECT,
}

pub struct TaskbarHider {
    pub is_hidden: bool,
    pub peek_visible: bool,
    monitors: Vec<MonitorWorkState>,
    last_geom: Option<TaskbarEdgeGeometry>,
    last_geoms: Vec<TaskbarEdgeGeometry>,
}

impl TaskbarHider {
    pub fn new() -> Self {
        Self {
            is_hidden: false,
            peek_visible: false,
            monitors: Vec::new(),
            last_geom: None,
            last_geoms: Vec::new(),
        }
    }

    pub fn last_edge_geometry(&mut self) -> TaskbarEdgeGeometry {
        if !self.is_hidden || self.peek_visible || self.last_geom.is_none() {
            if let Some(g) = Self::detect_current_geometry() {
                self.last_geom = Some(g);
                return g;
            }
        }
        self.last_geom.unwrap_or_else(|| {
            let mon = Self::query_all_monitors()
                .into_iter()
                .find(|m| (m.dwFlags & 1) != 0)
                .map(|m| m.rcMonitor)
                .unwrap_or(RECT {
                    left: 0,
                    top: 0,
                    right: 1920,
                    bottom: 1200,
                });
            TaskbarEdgeGeometry {
                edge: TaskbarEdge::Bottom,
                mon_rect: mon,
                bar_rect: RECT {
                    left: mon.left,
                    top: (mon.bottom - 48).max(mon.top),
                    right: mon.right,
                    bottom: mon.bottom,
                },
            }
        })
    }

    pub fn all_edge_geometries(&mut self) -> Vec<TaskbarEdgeGeometry> {
        if !self.is_hidden || self.peek_visible || self.last_geoms.is_empty() {
            let detected = Self::detect_all_geometries();
            if !detected.is_empty() {
                self.last_geom = detected.first().copied();
                self.last_geoms = detected.clone();
                return detected;
            }
        }
        if !self.last_geoms.is_empty() {
            return self.last_geoms.clone();
        }
        vec![self.last_edge_geometry()]
    }

    fn detect_hwnd_geometry(hw: HWND) -> Option<TaskbarEdgeGeometry> {
        unsafe {
            if hw.0.is_null() {
                return None;
            }
            let mut bar = RECT::default();
            if GetWindowRect(hw, &mut bar).is_err() {
                return None;
            }
            let bw = bar.right - bar.left;
            let bh = bar.bottom - bar.top;
            if bw <= 6 || bh <= 6 {
                return None;
            }
            let hmon = MonitorFromWindow(hw, MONITOR_DEFAULTTONEAREST);
            let mut mi = MONITORINFO {
                cbSize: size_of::<MONITORINFO>() as u32,
                ..Default::default()
            };
            let mon = if GetMonitorInfoW(hmon, &mut mi).as_bool() {
                mi.rcMonitor
            } else {
                RECT {
                    left: 0,
                    top: 0,
                    right: 1920,
                    bottom: 1200,
                }
            };
            let edge = if bh > bw * 2 {
                if (bar.left - mon.left).abs() <= (mon.right - bar.right).abs() {
                    TaskbarEdge::Left
                } else {
                    TaskbarEdge::Right
                }
            } else if (bar.top - mon.top).abs() <= (mon.bottom - bar.bottom).abs() {
                TaskbarEdge::Top
            } else {
                TaskbarEdge::Bottom
            };
            Some(TaskbarEdgeGeometry {
                edge,
                mon_rect: mon,
                bar_rect: bar,
            })
        }
    }

    fn detect_all_geometries() -> Vec<TaskbarEdgeGeometry> {
        let mut out = Vec::new();
        for hw in Self::collect_taskbar_hwnds() {
            if let Some(g) = Self::detect_hwnd_geometry(hw) {
                out.push(g);
            }
        }
        out
    }

    fn detect_current_geometry() -> Option<TaskbarEdgeGeometry> {
        unsafe {
            let cls = to_wide("Shell_TrayWnd");
            let main_hw = FindWindowW(PCWSTR(cls.as_ptr()), PCWSTR::null()).ok()?;
            Self::detect_hwnd_geometry(main_hw)
        }
    }

    pub fn collect_taskbar_hwnds() -> Vec<HWND> {
        let mut result = Vec::new();
        unsafe {
            let cls_main = to_wide("Shell_TrayWnd");
            if let Ok(main_hw) = FindWindowW(PCWSTR(cls_main.as_ptr()), PCWSTR::null()) {
                if !main_hw.0.is_null() {
                    result.push(main_hw);
                }
            }

            unsafe extern "system" fn enum_sec(hwnd: HWND, lparam: LPARAM) -> BOOL {
                let vec = &mut *(lparam.0 as *mut Vec<HWND>);
                let mut buf = [0u16; 128];
                let len = GetClassNameW(hwnd, &mut buf);
                if len > 0 {
                    let cls = String::from_utf16_lossy(&buf[..len as usize]);
                    if cls == "Shell_SecondaryTrayWnd" && !vec.contains(&hwnd) {
                        vec.push(hwnd);
                    }
                }
                BOOL(1)
            }
            let _ = EnumWindows(Some(enum_sec), LPARAM((&mut result as *mut Vec<HWND>) as isize));
        }
        result
    }

    pub fn query_all_monitors() -> Vec<MONITORINFO> {
        let mut list = Vec::new();
        unsafe {
            unsafe extern "system" fn cb(
                hmon: HMONITOR,
                _hdc: HDC,
                _rc: *mut RECT,
                lp: LPARAM,
            ) -> BOOL {
                let vec = &mut *(lp.0 as *mut Vec<MONITORINFO>);
                let mut mi = MONITORINFO {
                    cbSize: size_of::<MONITORINFO>() as u32,
                    ..Default::default()
                };
                if GetMonitorInfoW(hmon, &mut mi).as_bool() {
                    vec.push(mi);
                }
                BOOL(1)
            }
            let _ = EnumDisplayMonitors(
                None,
                None,
                Some(cb),
                LPARAM((&mut list as *mut Vec<MONITORINFO>) as isize),
            );
        }
        list
    }

    fn infer_work_from_bar(mon: RECT, bar: RECT) -> RECT {
        if bar.right <= mon.left
            || bar.left >= mon.right
            || bar.bottom <= mon.top
            || bar.top >= mon.bottom
        {
            return mon;
        }
        let bw = bar.right - bar.left;
        let bh = bar.bottom - bar.top;
        let mw = mon.right - mon.left;
        let mh = mon.bottom - mon.top;
        let mut out = mon;
        if bh >= mh * 3 / 4 && bw > 10 && bw < mw / 2 {
            if (bar.left - mon.left).abs() <= bw {
                out.left = bar.right;
            } else {
                out.right = bar.left;
            }
        } else if bw >= mw * 3 / 4 && bh > 10 && bh < mh / 2 {
            if (bar.top - mon.top).abs() <= bh {
                out.top = bar.bottom;
            } else {
                out.bottom = bar.top;
            }
        }
        out
    }

    pub fn main_taskbar_exists(&self) -> bool {
        unsafe {
            let cls = to_wide("Shell_TrayWnd");
            FindWindowW(PCWSTR(cls.as_ptr()), PCWSTR::null())
                .map(|h| !h.0.is_null())
                .unwrap_or(false)
        }
    }

    pub fn is_main_taskbar_visible() -> bool {
        unsafe {
            let cls = to_wide("Shell_TrayWnd");
            FindWindowW(PCWSTR(cls.as_ptr()), PCWSTR::null())
                .map(|h| !h.0.is_null() && IsWindowVisible(h).as_bool())
                .unwrap_or(false)
        }
    }

    pub fn is_main_taskbar_cloaked() -> bool {
        unsafe {
            let cls = to_wide("Shell_TrayWnd");
            let Ok(h) = FindWindowW(PCWSTR(cls.as_ptr()), PCWSTR::null()) else {
                return false;
            };
            if h.0.is_null() {
                return false;
            }
            let mut cloaked: u32 = 0;
            DwmGetWindowAttribute(
                h,
                DWMWA_CLOAKED,
                (&mut cloaked as *mut u32) as *mut c_void,
                size_of::<u32>() as u32,
            )
            .is_ok()
                && cloaked != 0
        }
    }

    pub fn uncloak_taskbar_windows() {
        unsafe {
            let uncloak: u32 = 0;
            for hw in Self::collect_taskbar_hwnds() {
                if !hw.0.is_null() {
                    let _ = DwmSetWindowAttribute(
                        hw,
                        DWMWA_CLOAK,
                        (&uncloak as *const u32) as *const c_void,
                        size_of::<u32>() as u32,
                    );
                }
            }
        }
    }

    unsafe fn collect_bar_rects() -> Vec<RECT> {
        let mut bar_rects = Vec::new();
        for hw in Self::collect_taskbar_hwnds() {
            let mut r = RECT::default();
            if GetWindowRect(hw, &mut r).is_ok() {
                bar_rects.push(r);
            }
        }
        bar_rects
    }

    fn resolve_monitor_work(
        &self,
        mi: &MONITORINFO,
        bar_rects: &[RECT],
        prefer_current_work: bool,
    ) -> RECT {
        for br in bar_rects {
            let inf = Self::infer_work_from_bar(mi.rcMonitor, *br);
            if inf != mi.rcMonitor {
                return inf;
            }
        }
        if prefer_current_work && mi.rcWork != mi.rcMonitor {
            return mi.rcWork;
        }
        if let Some(saved) = self.monitors.iter().find(|m| m.rc_monitor == mi.rcMonitor) {
            if saved.orig_work != mi.rcMonitor {
                return saved.orig_work;
            }
        }
        mi.rcMonitor
    }

    pub fn hide(&mut self, expand_workarea: bool) -> bool {
        let detected = Self::detect_all_geometries();
        if !detected.is_empty() {
            self.last_geom = detected.first().copied();
            self.last_geoms = detected;
        } else if let Some(g) = Self::detect_current_geometry() {
            self.last_geom = Some(g);
        }
        self.is_hidden = true;
        self.peek_visible = false;
        let mut wa_changed = false;
        unsafe {
            let cls = to_wide("Shell_TrayWnd");
            let Ok(main_hw) = FindWindowW(PCWSTR(cls.as_ptr()), PCWSTR::null()) else {
                return false;
            };
            if main_hw.0.is_null() {
                return false;
            }

            let bar_rects = Self::collect_bar_rects();

            for mi in Self::query_all_monitors() {
                let orig = self.resolve_monitor_work(&mi, &bar_rects, true);
                if let Some(existing) = self
                    .monitors
                    .iter_mut()
                    .find(|m| m.rc_monitor == mi.rcMonitor)
                {
                    if orig != mi.rcMonitor {
                        existing.orig_work = orig;
                    }
                } else {
                    self.monitors.push(MonitorWorkState {
                        orig_work: orig,
                        rc_monitor: mi.rcMonitor,
                    });
                }
            }

            // Hide taskbar windows FIRST so Explorer does not bounce WorkArea back
            for hw in Self::collect_taskbar_hwnds() {
                if IsWindowVisible(hw).as_bool() {
                    let _ = ShowWindow(hw, SW_HIDE);
                }
            }

            if expand_workarea {
                // Keep fWinIni = 0 and do NOT broadcast WM_SETTINGCHANGE here;
                // broadcasting causes Explorer to re-assert its AppBar workarea back to (72,0).
                for mi in Self::query_all_monitors() {
                    if mi.rcWork != mi.rcMonitor {
                        let mut full = mi.rcMonitor;
                        let _ = SystemParametersInfoW(
                            SPI_SETWORKAREA,
                            0,
                            Some((&mut full as *mut RECT) as *mut c_void),
                            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
                        );
                        wa_changed = true;
                    }
                }
                if wa_changed {
                    Self::refit_maximized_windows();
                }
            }
        }
        wa_changed
    }

    pub fn settle_hidden(&mut self, expand_workarea: bool) -> bool {
        if !self.is_hidden || self.peek_visible {
            return false;
        }
        let mut wa_changed = false;
        unsafe {
            for hw in Self::collect_taskbar_hwnds() {
                if IsWindowVisible(hw).as_bool() {
                    let _ = ShowWindow(hw, SW_HIDE);
                }
            }
            if expand_workarea {
                for mi in Self::query_all_monitors() {
                    if mi.rcWork != mi.rcMonitor {
                        let mut full = mi.rcMonitor;
                        let _ = SystemParametersInfoW(
                            SPI_SETWORKAREA,
                            0,
                            Some((&mut full as *mut RECT) as *mut c_void),
                            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
                        );
                        wa_changed = true;
                    }
                }
                if wa_changed {
                    Self::refit_maximized_windows();
                }
            }
        }
        wa_changed
    }

    /// Reserves (or releases, if None / thickness <= 0) WorkArea on the primary monitor for
    pub fn apply_edge_reserve(mon_rect: RECT, edge: TaskbarEdge, thick: i32) -> RECT {
        let mut target = mon_rect;
        let t = thick.max(0);
        if t > 0 {
            match edge {
                TaskbarEdge::Bottom => {
                    target.bottom = (target.bottom - t).max(target.top + 240);
                }
                TaskbarEdge::Top => {
                    target.top = (target.top + t).min(target.bottom - 240);
                }
                TaskbarEdge::Left => {
                    target.left = (target.left + t).min(target.right - 320);
                }
                TaskbarEdge::Right => {
                    target.right = (target.right - t).max(target.left + 320);
                }
            }
        }
        target
    }

    /// Always-Visible Interactive Docks so maximized windows never extend behind any monitor's dock.
    pub fn apply_dock_work_area(reserves: &[(RECT, TaskbarEdge, i32)]) -> bool {
        unsafe {
            let mut changed = false;
            for mi in Self::query_all_monitors() {
                let target = reserves
                    .iter()
                    .filter(|(rc, _, _)| *rc == mi.rcMonitor)
                    .fold(mi.rcMonitor, |acc, &(_, edge, thick)| {
                        Self::apply_edge_reserve(acc, edge, thick)
                    });
                if mi.rcWork != target {
                    let mut wr = target;
                    let _ = SystemParametersInfoW(
                        SPI_SETWORKAREA,
                        0,
                        Some((&mut wr as *mut RECT) as *mut c_void),
                        SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
                    );
                    changed = true;
                }
            }
            if changed {
                // Keep fWinIni = 0 and do NOT broadcast WM_SETTINGCHANGE(SPI_SETWORKAREA) to
                // HWND_BROADCAST here: Shell_TrayWnd (explorer.exe) reacts to that broadcast by
                // immediately re-asserting its hidden native AppBar work area (72px bottom / 0px top),
                // wiping out our custom dock & top-bar reserves and causing docks to overlap apps.
                Self::refit_maximized_windows();
            }
            changed
        }
    }

    /// Reveals Shell_TrayWnd while hidden.
    /// `overlay_only = true` (`Edge Hover Peek`) skips WorkArea & WM_WINDOWPOSCHANGING so
    /// maximized windows never shrink or twitch.
    /// `overlay_only = false` (`Smart Auto-Hide`) docks the taskbar cleanly and slides
    /// maximized windows aside so the peeked bar never covers app sidebars or headers.
    pub fn peek_show(&mut self, overlay_only: bool) {
        if !self.is_hidden || self.peek_visible {
            return;
        }
        self.peek_visible = true;
        unsafe {
            let cls = to_wide("Shell_TrayWnd");
            if let Ok(main_hw) = FindWindowW(PCWSTR(cls.as_ptr()), PCWSTR::null()) {
                if !main_hw.0.is_null() {
                    let mut res = 0usize;
                    let _ = SendMessageTimeoutW(
                        main_hw,
                        0x05CA,
                        WPARAM(0),
                        LPARAM(0),
                        SMTO_ABORTIFHUNG,
                        120,
                        Some(&mut res),
                    );
                }
            }
            if overlay_only {
                for hw in Self::collect_taskbar_hwnds() {
                    let _ = ShowWindow(hw, SW_SHOWNA);
                    let _ = SetWindowPos(
                        hw,
                        Some(HWND_TOPMOST),
                        0,
                        0,
                        0,
                        0,
                        SWP_NOACTIVATE
                            | SWP_NOMOVE
                            | SWP_NOSIZE
                            | SWP_SHOWWINDOW
                            | SWP_NOSENDCHANGING,
                    );
                    let _ = InvalidateRect(Some(hw), None, true);
                }
                for mi in Self::query_all_monitors() {
                    if mi.rcWork != mi.rcMonitor {
                        let mut full = mi.rcMonitor;
                        let _ = SystemParametersInfoW(
                            SPI_SETWORKAREA,
                            0,
                            Some((&mut full as *mut RECT) as *mut c_void),
                            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
                        );
                    }
                }
            } else {
                let bar_rects = Self::collect_bar_rects();
                for mi in Self::query_all_monitors() {
                    let mut target = self.resolve_monitor_work(&mi, &bar_rects, false);
                    if target != mi.rcMonitor && mi.rcWork != target {
                        let _ = SystemParametersInfoW(
                            SPI_SETWORKAREA,
                            0,
                            Some((&mut target as *mut RECT) as *mut c_void),
                            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
                        );
                    }
                }
                for hw in Self::collect_taskbar_hwnds() {
                    let _ = ShowWindow(hw, SW_SHOWNA);
                    let _ = SetWindowPos(
                        hw,
                        Some(HWND_TOPMOST),
                        0,
                        0,
                        0,
                        0,
                        SWP_NOACTIVATE
                            | SWP_NOMOVE
                            | SWP_NOSIZE
                            | SWP_SHOWWINDOW
                            | SWP_NOSENDCHANGING,
                    );
                    let _ = InvalidateRect(Some(hw), None, true);
                }
                Self::refit_maximized_windows();
            }
            let detected = Self::detect_all_geometries();
            if !detected.is_empty() {
                self.last_geom = detected.first().copied();
                self.last_geoms = detected;
            }
        }
    }

    pub fn keep_peek_topmost(&self) {
        if !self.is_hidden || !self.peek_visible {
            return;
        }
        unsafe {
            for hw in Self::collect_taskbar_hwnds() {
                if !hw.0.is_null() {
                    if !IsWindowVisible(hw).as_bool() {
                        let _ = ShowWindow(hw, SW_SHOWNA);
                    }
                    let ex = GetWindowLongW(hw, GWL_EXSTYLE) as u32;
                    if (ex & WS_EX_TOPMOST.0) == 0 {
                        let _ = SetWindowPos(
                            hw,
                            Some(HWND_TOPMOST),
                            0,
                            0,
                            0,
                            0,
                            SWP_NOACTIVATE
                                | SWP_NOMOVE
                                | SWP_NOSIZE
                                | SWP_SHOWWINDOW
                                | SWP_NOSENDCHANGING,
                        );
                    }
                }
            }
        }
    }

    pub fn peek_hide(&mut self, overlay_only: bool) {
        if !self.is_hidden || !self.peek_visible {
            return;
        }
        self.peek_visible = false;
        unsafe {
            for hw in Self::collect_taskbar_hwnds() {
                if IsWindowVisible(hw).as_bool() {
                    let _ = ShowWindow(hw, SW_HIDE);
                }
            }
            for mi in Self::query_all_monitors() {
                if mi.rcWork != mi.rcMonitor {
                    let mut full = mi.rcMonitor;
                    let _ = SystemParametersInfoW(
                        SPI_SETWORKAREA,
                        0,
                        Some((&mut full as *mut RECT) as *mut c_void),
                        SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
                    );
                }
            }
            if !overlay_only {
                Self::refit_maximized_windows();
            }
        }
    }

    unsafe fn reveal_taskbar_windows() {
        Self::uncloak_taskbar_windows();
        for hw in Self::collect_taskbar_hwnds() {
            let _ = SetWindowRgn(hw, None, true);
            let _ = ShowWindow(hw, SW_SHOWNA);
            let _ = SetWindowPos(
                hw,
                Some(HWND_TOPMOST),
                0,
                0,
                0,
                0,
                SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE | SWP_SHOWWINDOW,
            );
            let _ = InvalidateRect(Some(hw), None, true);
            let _ = RedrawWindow(
                Some(hw),
                None,
                None,
                RDW_INVALIDATE | RDW_FRAME | RDW_ALLCHILDREN,
            );
        }
        let cls = to_wide("Shell_TrayWnd");
        if let Ok(main_hw) = FindWindowW(PCWSTR(cls.as_ptr()), PCWSTR::null()) {
            if !main_hw.0.is_null() {
                let mut res = 0usize;
                let _ = SendMessageTimeoutW(
                    main_hw,
                    0x05CA,
                    WPARAM(0),
                    LPARAM(0),
                    SMTO_ABORTIFHUNG,
                    120,
                    Some(&mut res),
                );
            }
        }
    }

    unsafe fn finalize_restore_workarea() {
        // Broadcast async so Explorer re-docks its AppBar cleanly and slow apps never stall exit
        let _ = SendNotifyMessageW(
            HWND_BROADCAST,
            WM_SETTINGCHANGE,
            WPARAM(SPI_SETWORKAREA_PARAM),
            LPARAM(0),
        );
        Self::refit_maximized_windows();
    }

    pub fn show(&mut self, restore_workarea: bool) -> bool {
        self.is_hidden = false;
        self.peek_visible = false;
        let mut wa_changed = false;
        unsafe {
            Self::reveal_taskbar_windows();
            if restore_workarea {
                let bar_rects = Self::collect_bar_rects();
                for mi in Self::query_all_monitors() {
                    let mut rest = self.resolve_monitor_work(&mi, &bar_rects, false);
                    if rest != mi.rcMonitor && mi.rcWork != rest {
                        let _ = SystemParametersInfoW(
                            SPI_SETWORKAREA,
                            0,
                            Some((&mut rest as *mut RECT) as *mut c_void),
                            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
                        );
                        wa_changed = true;
                    }
                }
                if wa_changed {
                    Self::finalize_restore_workarea();
                }
            }
        }
        wa_changed
    }

    pub fn emergency_restore_windows_taskbar() {
        unsafe {
            Self::reveal_taskbar_windows();
            if !crate::config::get_native_autohide() {
                let bar_rects = Self::collect_bar_rects();
                for mi in Self::query_all_monitors() {
                    let mut rest = mi.rcMonitor;
                    for br in &bar_rects {
                        let inf = Self::infer_work_from_bar(mi.rcMonitor, *br);
                        if inf != mi.rcMonitor {
                            rest = inf;
                            break;
                        }
                    }
                    if rest != mi.rcMonitor && mi.rcWork != rest {
                        let _ = SystemParametersInfoW(
                            SPI_SETWORKAREA,
                            0,
                            Some((&mut rest as *mut RECT) as *mut c_void),
                            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
                        );
                    }
                }
                Self::finalize_restore_workarea();
            }
        }
    }

    /// Re-fits already-maximized visible windows to the current monitor workarea without
    /// changing focus or Z-order, only when their visible frame differs from rcWork.
    fn refit_maximized_windows() {
        unsafe {
            let mut list: Vec<(HWND, RECT)> = Vec::new();
            unsafe extern "system" fn enum_zoomed(hwnd: HWND, lp: LPARAM) -> BOOL {
                let out = &mut *(lp.0 as *mut Vec<(HWND, RECT)>);
                if !IsWindowVisible(hwnd).as_bool()
                    || IsIconic(hwnd).as_bool()
                    || !IsZoomed(hwnd).as_bool()
                {
                    return BOOL(1);
                }
                let ex_style = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
                if (ex_style & WS_EX_TOOLWINDOW.0) != 0 && (ex_style & WS_EX_APPWINDOW.0) == 0 {
                    return BOOL(1);
                }
                let mut cloaked: u32 = 0;
                if DwmGetWindowAttribute(
                    hwnd,
                    DWMWA_CLOAKED,
                    (&mut cloaked as *mut u32) as *mut _,
                    size_of::<u32>() as u32,
                )
                .is_ok()
                    && cloaked != 0
                {
                    return BOOL(1);
                }
                let mut buf = [0u16; 128];
                let len = GetClassNameW(hwnd, &mut buf);
                if len > 0 {
                    let cls = String::from_utf16_lossy(&buf[..len as usize]);
                    if matches!(
                        cls.as_str(),
                        "Progman"
                            | "WorkerW"
                            | "Shell_TrayWnd"
                            | "Shell_SecondaryTrayWnd"
                            | "RainmeterMeterWindow"
                            | "AutumnDeckHiddenWnd"
                            | "AutumnDeckGlassWnd"
                            | "AutumnDeckBlurWnd"
                            | "AutumnDeckGlassAcrylicWnd"
                            | "AutumnCleanTopBarWnd"
                            | "AutumnTopPopoverWnd"
                            | "AutumnTaskbarHiddenWnd"
                            | "AutumnTaskbarGlassWnd"
                            | "AutumnTaskbarBlurWnd"
                            | "AutumnMacDockWnd"
                            | "AutumnDockThumbWnd"
                            | "AutumnSettingsWnd"
                    ) {
                        return BOOL(1);
                    }
                }
                let hmon = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
                let mut mi = MONITORINFO {
                    cbSize: size_of::<MONITORINFO>() as u32,
                    ..Default::default()
                };
                if !GetMonitorInfoW(hmon, &mut mi).as_bool() {
                    return BOOL(1);
                }
                let style = GetWindowLongW(hwnd, GWL_STYLE) as u32;
                let mut frame = RECT::default();
                let ok_frame = DwmGetWindowAttribute(
                    hwnd,
                    DWMWA_EXTENDED_FRAME_BOUNDS,
                    (&mut frame as *mut RECT) as *mut _,
                    size_of::<RECT>() as u32,
                )
                .is_ok();
                if ok_frame {
                    // Never shrink genuine borderless fullscreen apps/games
                    if (style & WS_CAPTION.0) == 0 && frame == mi.rcMonitor {
                        return BOOL(1);
                    }
                    let w = mi.rcWork;
                    let differs = (frame.left - w.left).abs() > 2
                        || (frame.top - w.top).abs() > 2
                        || (frame.right - w.right).abs() > 2
                        || (frame.bottom - w.bottom).abs() > 2;
                    if differs {
                        out.push((hwnd, w));
                    }
                } else {
                    out.push((hwnd, mi.rcWork));
                }
                BOOL(1)
            }
            let _ = EnumWindows(Some(enum_zoomed), LPARAM((&mut list as *mut Vec<(HWND, RECT)>) as isize));

            let fg_before = GetForegroundWindow();
            for (hwnd, w) in list {
                let _ = SetWindowPos(
                    hwnd,
                    None,
                    0,
                    0,
                    0,
                    0,
                    SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED,
                );
                // Chromium/Electron/UWP windows sometimes keep their previous maximized frame
                // after SWP_FRAMECHANGED alone; snap their outer rect cleanly into `w` without
                // broadcasting WM_SETTINGCHANGE to explorer.exe.
                let mut frame = RECT::default();
                let mut win_rc = RECT::default();
                if DwmGetWindowAttribute(
                    hwnd,
                    DWMWA_EXTENDED_FRAME_BOUNDS,
                    (&mut frame as *mut RECT) as *mut _,
                    size_of::<RECT>() as u32,
                )
                .is_ok()
                    && GetWindowRect(hwnd, &mut win_rc).is_ok()
                {
                    let still_differs = (frame.top - w.top).abs() > 3
                        || (frame.bottom - w.bottom).abs() > 3
                        || (frame.left - w.left).abs() > 3
                        || (frame.right - w.right).abs() > 3;
                    if still_differs {
                        let bl = (frame.left - win_rc.left).clamp(0, 16);
                        let bt = (frame.top - win_rc.top).clamp(0, 16);
                        let br = (win_rc.right - frame.right).clamp(0, 16);
                        let bb = (win_rc.bottom - frame.bottom).clamp(0, 16);
                        let _ = SetWindowPos(
                            hwnd,
                            None,
                            w.left - bl,
                            w.top - bt,
                            (w.right - w.left) + bl + br,
                            (w.bottom - w.top) + bt + bb,
                            SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED,
                        );
                    }
                }
            }
            if !fg_before.0.is_null() && GetForegroundWindow() != fg_before {
                let _ = SetForegroundWindow(fg_before);
            }
        }
    }

    pub fn clear_monitor_cache(&mut self) {
        self.monitors.clear();
        self.last_geom = None;
        self.last_geoms.clear();
    }

    pub fn refresh_geometry_cache(&mut self) {
        let detected = Self::detect_all_geometries();
        if !detected.is_empty() {
            self.last_geom = detected.first().copied();
            self.last_geoms = detected;
        } else if let Some(g) = Self::detect_current_geometry() {
            self.last_geom = Some(g);
        }
    }

    pub fn handle_show_event(&self, hwnd: HWND) {
        if !self.is_hidden || self.peek_visible || hwnd.0.is_null() {
            return;
        }
        unsafe {
            let mut buf = [0u16; 128];
            let len = GetClassNameW(hwnd, &mut buf);
            if len > 0 {
                let cls = String::from_utf16_lossy(&buf[..len as usize]);
                if cls == "Shell_TrayWnd" || cls == "Shell_SecondaryTrayWnd" {
                    if IsWindowVisible(hwnd).as_bool() {
                        let _ = ShowWindow(hwnd, SW_HIDE);
                    }
                }
            }
        }
    }
}

struct MaxCheckCtx {
    primary_mon: HMONITOR,
    mon_rect: RECT,
    found_maximized: bool,
}

pub fn has_maximized_or_fullscreen_on_monitor(target_mon_rect: RECT) -> bool {
    unsafe {
        let cx = target_mon_rect.left + (target_mon_rect.right - target_mon_rect.left) / 2;
        let cy = target_mon_rect.top + (target_mon_rect.bottom - target_mon_rect.top) / 2;
        let target_mon = MonitorFromPoint(
            windows::Win32::Foundation::POINT { x: cx, y: cy },
            MONITOR_DEFAULTTONEAREST,
        );
        let fg = GetForegroundWindow();
        if !fg.0.is_null() && MonitorFromWindow(fg, MONITOR_DEFAULTTONEAREST) == target_mon {
            let mut buf = [0u16; 128];
            let len = GetClassNameW(fg, &mut buf);
            if len > 0 {
                let cls = String::from_utf16_lossy(&buf[..len as usize]);
                if matches!(
                    cls.as_str(),
                    "Progman"
                        | "WorkerW"
                        | "AutumnDesktopWidgetsWnd"
                        | "AutumnVideoWallpaperWnd"
                ) {
                    return false;
                }
            }
        }

        let mut ctx = MaxCheckCtx {
            primary_mon: target_mon,
            mon_rect: target_mon_rect,
            found_maximized: false,
        };
        let _ = EnumWindows(
            Some(enum_max_proc),
            LPARAM((&mut ctx as *mut MaxCheckCtx) as isize),
        );
        ctx.found_maximized
    }
}

pub fn has_maximized_or_fullscreen_window() -> bool {
    unsafe {
        let primary_mon = MonitorFromPoint(
            windows::Win32::Foundation::POINT { x: 0, y: 0 },
            MONITOR_DEFAULTTOPRIMARY,
        );
        let mut mi = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        let mon_rect = if GetMonitorInfoW(primary_mon, &mut mi).as_bool() {
            mi.rcMonitor
        } else {
            RECT { left: 0, top: 0, right: 1920, bottom: 1200 }
        };
        has_maximized_or_fullscreen_on_monitor(mon_rect)
    }
}

unsafe extern "system" fn enum_max_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let ctx = &mut *(lparam.0 as *mut MaxCheckCtx);

    if !IsWindowVisible(hwnd).as_bool() || IsIconic(hwnd).as_bool() {
        return BOOL(1);
    }

    let mut buf = [0u16; 128];
    let len = GetClassNameW(hwnd, &mut buf);
    if len > 0 {
        let cls = String::from_utf16_lossy(&buf[..len as usize]);
        // Reaching the desktop plane in Z-order means no window above the desktop is maximized
        if cls == "Progman" || cls == "WorkerW" {
            let def_w = to_wide("SHELLDLL_DefView");
            if let Ok(dv) = windows::Win32::UI::WindowsAndMessaging::FindWindowExW(
                Some(hwnd),
                None,
                PCWSTR(def_w.as_ptr()),
                PCWSTR::null(),
            ) {
                if !dv.0.is_null() {
                    return BOOL(0);
                }
            }
            if cls == "Progman" {
                return BOOL(0);
            }
            return BOOL(1);
        }
        if matches!(
            cls.as_str(),
            "WorkerW"
                | "Shell_TrayWnd"
                | "Shell_SecondaryTrayWnd"
                | "RainmeterMeterWindow"
                | "AutumnDeckHiddenWnd"
                | "AutumnDeckGlassWnd"
                | "AutumnDeckBlurWnd"
                | "AutumnDeckGlassAcrylicWnd"
                | "AutumnCleanTopBarWnd"
                | "AutumnTopPopoverWnd"
                | "AutumnMacDockWnd"
                | "AutumnDockThumbWnd"
                | "AutumnTaskbarHiddenWnd"
                | "AutumnTaskbarGlassWnd"
                | "AutumnTaskbarBlurWnd"
                | "AutumnSettingsWnd"
                | "AutumnDesktopWidgetsWnd"
                | "AutumnVideoWallpaperWnd"
                | "Windows.UI.Core.CoreWindow"
                | "ForegroundStaging"
                | "ApplicationManager_DesktopShellWindow"
                | "DummyDwmListenerWindow"
                | "XamlExplorerHostIslandWindow"
                | "TopLevelWindowForOverflowXamlIsland"
        ) {
            return BOOL(1);
        }
    }

    let ex_style = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
    if (ex_style & WS_EX_TOOLWINDOW.0) != 0 && (ex_style & WS_EX_APPWINDOW.0) == 0 {
        return BOOL(1);
    }
    if (ex_style & (WS_EX_TRANSPARENT.0 | WS_EX_NOACTIVATE.0)) != 0 {
        return BOOL(1);
    }

    if (ex_style & WS_EX_APPWINDOW.0) == 0 {
        if let Ok(owner) = GetWindow(hwnd, GW_OWNER) {
            if !owner.0.is_null() {
                return BOOL(1);
            }
        }
    }

    let mut cloaked: u32 = 0;
    if DwmGetWindowAttribute(
        hwnd,
        DWMWA_CLOAKED,
        (&mut cloaked as *mut u32) as *mut _,
        size_of::<u32>() as u32,
    )
    .is_ok()
        && cloaked != 0
    {
        return BOOL(1);
    }

    if GetWindowTextLengthW(hwnd) <= 0 {
        return BOOL(1);
    }

    let hmon = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
    if hmon != ctx.primary_mon {
        return BOOL(1);
    }

    if IsZoomed(hwnd).as_bool() {
        ctx.found_maximized = true;
        return BOOL(0);
    }

    let mut r = RECT::default();
    if GetWindowRect(hwnd, &mut r).is_ok() {
        if r.left <= ctx.mon_rect.left
            && r.top <= ctx.mon_rect.top
            && r.right >= ctx.mon_rect.right
            && r.bottom >= ctx.mon_rect.bottom
        {
            ctx.found_maximized = true;
            return BOOL(0);
        }

        let w = (r.right - r.left).max(0) as i64;
        let h = (r.bottom - r.top).max(0) as i64;
        let win_area = w * h;
        let mon_w = (ctx.mon_rect.right - ctx.mon_rect.left).max(1) as i64;
        let mon_h = (ctx.mon_rect.bottom - ctx.mon_rect.top).max(1) as i64;
        let mon_area = mon_w * mon_h;
        if win_area * 10 >= mon_area * 8 {
            ctx.found_maximized = true;
            return BOOL(0);
        }
    }

    BOOL(1)
}

pub fn is_shell_flyout_active() -> bool {
    unsafe {
        let mut gti = GUITHREADINFO {
            cbSize: size_of::<GUITHREADINFO>() as u32,
            ..Default::default()
        };
        if GetGUIThreadInfo(0, &mut gti).is_ok()
            && (gti.flags.0 & (GUI_INMENUMODE.0 | GUI_POPUPMENUMODE.0)) != 0
        {
            return true;
        }
        // Keep peeked native taskbar open while hovering app thumbnail previews (WS_EX_NOACTIVATE)
        let thumb_cls = to_wide("TaskListThumbnailWnd");
        if let Ok(thumb_hw) = FindWindowW(PCWSTR(thumb_cls.as_ptr()), PCWSTR::null()) {
            if !thumb_hw.0.is_null() && IsWindowVisible(thumb_hw).as_bool() {
                return true;
            }
        }
        let fg = GetForegroundWindow();
        if fg.0.is_null() {
            return false;
        }
        let mut buf = [0u16; 128];
        let len = GetClassNameW(fg, &mut buf);
        if len <= 0 {
            return false;
        }
        let cls = String::from_utf16_lossy(&buf[..len as usize]);
        matches!(
            cls.as_str(),
            "Shell_TrayWnd"
                | "Shell_SecondaryTrayWnd"
                | "Windows.UI.Core.CoreWindow"
                | "XamlExplorerHostIslandWindow"
                | "TopLevelWindowForOverflowXamlIsland"
                | "NotifyIconOverflowWindow"
                | "#32768"
                | "Xaml_WindowedPopupClass"
                | "Microsoft.UI.Content.PopupWindowSiteBridge"
                | "ControlCenterWindow"
        )
    }
}

/// Pure classifier distinguishing true fullscreen / borderless-fullscreen games & apps from
/// ordinary maximized desktop windows (even when our virtual dock expands `rcWork == rcMonitor`).
fn classify_fullscreen_window(
    quns_d3d: bool,
    quns_busy: bool,
    covers_monitor: bool,
    has_caption: bool,
    is_zoomed: bool,
) -> bool {
    if quns_d3d {
        return true;
    }
    if !covers_monitor {
        return false;
    }
    quns_busy || (!has_caption && !is_zoomed)
}

fn is_shell_or_desktop_class(cls: &str) -> bool {
    matches!(
        cls,
        "Progman"
            | "WorkerW"
            | "Shell_TrayWnd"
            | "Shell_SecondaryTrayWnd"
            | "NotifyIconOverflowWindow"
            | "TopLevelWindowForOverflowXamlIsland"
            | "TaskListThumbnailWnd"
            | "MultitaskingViewFrame"
            | "ForegroundStaging"
            | "XamlExplorerHostIslandWindow"
            | "Windows.UI.Core.CoreWindow"
            | "ControlCenterWindow"
            | "AutumnDesktopWidgetsWnd"
            | "AutumnVideoWallpaperWnd"
            | "AutumnMacDockWnd"
            | "AutumnDeckSettingsWnd"
            | "AutumnDeckHiddenWnd"
            | "AutumnTaskbarSettingsWnd"
            | "AutumnTaskbarHiddenWnd"
    )
}

/// Anti-cheat safe fullscreen game / app detector.
/// Uses only official Shell/User32 metadata (`SHQueryUserNotificationState`, `GetForegroundWindow`,
/// `GetWindowRect`, `GetWindowLongW`) — never calls `OpenProcess` or touches game memory.
fn detect_fullscreen_foreground(target_mon: Option<RECT>) -> bool {
    unsafe {
        let mut quns_d3d = false;
        let mut quns_busy = false;
        if let Ok(state) = SHQueryUserNotificationState() {
            quns_d3d = state == QUNS_RUNNING_D3D_FULL_SCREEN;
            quns_busy = state == QUNS_BUSY;
        }
        if quns_d3d && target_mon.is_none() {
            return true;
        }

        let fg = GetForegroundWindow();
        if fg.0.is_null() || !IsWindowVisible(fg).as_bool() || IsIconic(fg).as_bool() {
            return quns_d3d && target_mon.is_none();
        }

        let mut pid = 0u32;
        GetWindowThreadProcessId(fg, Some(&mut pid));
        if pid == 0 || pid == std::process::id() {
            return false;
        }

        let mut buf = [0u16; 128];
        let len = GetClassNameW(fg, &mut buf);
        if len > 0 {
            let cls = String::from_utf16_lossy(&buf[..len as usize]);
            if is_shell_or_desktop_class(&cls) {
                return false;
            }
        }

        let mut cloaked: u32 = 0;
        if DwmGetWindowAttribute(
            fg,
            DWMWA_CLOAKED,
            (&mut cloaked as *mut u32) as *mut c_void,
            size_of::<u32>() as u32,
        )
        .is_ok()
            && cloaked != 0
        {
            return false;
        }

        let fg_mon = MonitorFromWindow(fg, MONITOR_DEFAULTTONEAREST);
        let mut mi = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if !GetMonitorInfoW(fg_mon, &mut mi).as_bool() {
            return quns_d3d && target_mon.is_none();
        }
        let mon_rc = mi.rcMonitor;

        if let Some(req_rc) = target_mon {
            if mon_rc.left != req_rc.left
                || mon_rc.top != req_rc.top
                || mon_rc.right != req_rc.right
                || mon_rc.bottom != req_rc.bottom
            {
                return false;
            }
        }

        let mut rc = RECT::default();
        if GetWindowRect(fg, &mut rc).is_err() {
            return quns_d3d;
        }

        let covers_monitor = rc.left <= mon_rc.left + 2
            && rc.top <= mon_rc.top + 2
            && rc.right >= mon_rc.right - 2
            && rc.bottom >= mon_rc.bottom - 2;
        let style = GetWindowLongW(fg, GWL_STYLE) as u32;
        let has_caption = (style & WS_CAPTION.0) == WS_CAPTION.0;
        let is_zoomed = IsZoomed(fg).as_bool();

        classify_fullscreen_window(quns_d3d, quns_busy, covers_monitor, has_caption, is_zoomed)
    }
}

/// Returns `true` when an exclusive Direct3D/Vulkan game or borderless fullscreen app is focused.
pub fn is_fullscreen_game_or_app_active() -> bool {
    detect_fullscreen_foreground(None)
}

/// Returns `true` when a fullscreen game or borderless fullscreen app is focused on `target_mon_rect`.
pub fn is_fullscreen_game_on_monitor(target_mon_rect: RECT) -> bool {
    detect_fullscreen_foreground(Some(target_mon_rect))
}

static OS_SESSION_ENDING: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// Marks that the Windows user session is shutting down or logging off so
/// `TaskbarHider::drop` does not issue synchronous `SPI_SETWORKAREA` /
/// `HWND_BROADCAST` / `ShowWindow` calls against a shutting-down `explorer.exe`.
pub fn mark_os_session_ending() {
    OS_SESSION_ENDING.store(true, std::sync::atomic::Ordering::Release);
}

impl Drop for TaskbarHider {
    fn drop(&mut self) {
        if !OS_SESSION_ENDING.load(std::sync::atomic::Ordering::Acquire)
            && (self.is_hidden
                || (self.main_taskbar_exists() && !Self::is_main_taskbar_visible()))
        {
            Self::emergency_restore_windows_taskbar();
        }
        self.is_hidden = false;
        self.peek_visible = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dock_work_area_reserves_edges_across_monitors() {
        let m1 = RECT { left: 0, top: 0, right: 1920, bottom: 1200 };
        let m2 = RECT { left: -1920, top: 0, right: 0, bottom: 1080 };
        let r1 = TaskbarHider::apply_edge_reserve(m1, TaskbarEdge::Bottom, 76);
        let r2 = TaskbarHider::apply_edge_reserve(m2, TaskbarEdge::Bottom, 52);
        assert_eq!(r1.bottom, 1124);
        assert_eq!(r2.bottom, 1028);
        assert_eq!(
            TaskbarHider::apply_edge_reserve(m2, TaskbarEdge::Left, 64),
            RECT { left: -1856, top: 0, right: 0, bottom: 1080 }
        );
    }

    #[test]
    fn classify_fullscreen_distinguishes_games_from_maximized_apps() {
        // Exclusive Direct3D / Vulkan fullscreen game
        assert!(classify_fullscreen_window(true, false, false, true, false));
        // Borderless fullscreen popup game covering monitor (no caption, not zoomed)
        assert!(classify_fullscreen_window(false, false, true, false, false));
        // F11 fullscreen browser / video player (Windows reports QUNS_BUSY + covers monitor)
        assert!(classify_fullscreen_window(false, true, true, true, true));
        // Normal maximized browser/IDE even when work area == full monitor
        assert!(!classify_fullscreen_window(false, false, true, true, true));
        // Windowed game (does not cover monitor)
        assert!(!classify_fullscreen_window(false, false, false, false, false));
    }
}
