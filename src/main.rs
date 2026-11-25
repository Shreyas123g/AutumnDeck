#![windows_subsystem = "windows"]

mod config;
mod dcomp_surface;
mod desktop_widgets;
mod diagnostics;
mod dock_themes;
mod mac_dock;
mod realistic_pbr;
mod settings_ui;
mod suites;
mod system_media;
mod taskbar_hide;
mod taskbar_styler;
mod theme;
mod video_wallpaper;
mod widgets_extra;
mod widgets_phase2;

use std::fs;
use std::path::PathBuf;
use std::ptr::null_mut;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{
    CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, HANDLE, HINSTANCE, HWND, LPARAM, LRESULT,
    POINT, WAIT_ABANDONED, WAIT_OBJECT_0, WPARAM,
};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, CreateBitmap, CreateDIBSection, DeleteObject, EndPaint, FillRect, GetStockObject,
    BITMAPINFO, BITMAPINFOHEADER, BI_RGB, BLACK_BRUSH, DIB_RGB_COLORS, HBITMAP, HBRUSH, HGDIOBJ,
    PAINTSTRUCT,
};
use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::SystemInformation::GetTickCount64;
use windows::Win32::System::Threading::{
    CreateEventW, CreateMutexW, OpenEventW, OpenProcess, ReleaseMutex, SetEvent,
    WaitForSingleObject, EVENT_ALL_ACCESS, INFINITE, PROCESS_SYNCHRONIZE,
};
use windows::Win32::UI::Accessibility::{SetWinEventHook, UnhookWinEvent, HWINEVENTHOOK};
use windows::Win32::UI::HiDpi::{
    SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    RegisterHotKey, UnregisterHotKey, HOT_KEY_MODIFIERS, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT,
};
use windows::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY,
    NOTIFYICONDATAW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreateIconIndirect, CreatePopupMenu, CreateWindowExW, DefWindowProcW,
    DestroyIcon, DestroyMenu, DestroyWindow, DispatchMessageW, FindWindowW, GetCursorPos,
    GetForegroundWindow, GetMessageW, GetWindowLongW, IsWindowVisible, KillTimer,
    PostMessageW, PostQuitMessage, RegisterClassExW, RegisterWindowMessageW, SendMessageTimeoutW,
    SetForegroundWindow, SetTimer, SetWindowPos, TrackPopupMenu, TranslateMessage, GWL_EXSTYLE,
    GWL_STYLE, HICON, HMENU, HWND_TOP, ICONINFO, MF_CHECKED, MF_SEPARATOR, MF_STRING, MF_UNCHECKED,
    MSG, SMTO_ABORTIFHUNG, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, TPM_BOTTOMALIGN, TPM_LEFTALIGN,
    TPM_RETURNCMD, TPM_RIGHTBUTTON, WM_APP, WM_CLOSE, WM_DESTROY, WM_DISPLAYCHANGE, WM_ENDSESSION,
    WM_HOTKEY, WM_LBUTTONDBLCLK, WM_LBUTTONUP, WM_QUERYENDSESSION, WM_RBUTTONUP, WM_SETTINGCHANGE,
    WM_TIMER, WNDCLASSEXW, WS_CHILD, WS_EX_TOOLWINDOW, WS_POPUP,
};

use config::{
    flush_pending_config_save, get_native_autohide, load_config, save_config_async,
    start_explorer_pref_watcher, to_wide, AppConfig, MacDockHideMode, PillMode, TaskbarMode,
};
use mac_dock::{MacDockAction, MacDockEngine};
use settings_ui::{SettingsCommand, SettingsController, StudioTab, WidgetPage};
use system_media::CcAction;
use taskbar_hide::{
    has_maximized_or_fullscreen_window, is_fullscreen_game_or_app_active, is_shell_flyout_active,
    TaskbarEdge, TaskbarHider,
};
use taskbar_styler::TaskbarStyler;

const WM_APP_TRAY: u32 = WM_APP + 1;
const WM_APP_TOGGLE_HIDE: u32 = WM_APP + 10;
const WM_APP_FORCE_HIDE: u32 = WM_APP + 11;
const WM_APP_FORCE_SHOW: u32 = WM_APP + 12;
const WM_APP_OPEN_SETTINGS: u32 = WM_APP + 13;
const WM_APP_QUIT: u32 = WM_APP + 14;
const WM_APP_SCHED_EVAL: u32 = WM_APP + 15;
const WM_APP_TASKBAR_SHOW_EVT: u32 = WM_APP + 16;
const WM_APP_PILLS_READY: u32 = WM_APP + 17;
const WM_APP_PING: u32 = WM_APP + 18;
const WM_APP_WIDGET_CLOCK_24H: u32 = WM_APP + 19;
const WM_APP_ENSURE_WIDGETS_Z: u32 = WM_APP + 20;
const WM_APP_WIDGETS_SYNC: u32 = WM_APP + 21;
const WM_APP_MEDIA_REDRAW: u32 = WM_APP + 22;
const WM_APP_TAP_READY: u32 = WM_APP + 23;
const WM_APP_DEFERRED_INIT: u32 = WM_APP + 24;
const WM_APP_PALETTE_UPDATED: u32 = WM_APP + 25;
const WM_APP_EXPLORER_PREF_CHANGED: u32 = WM_APP + 26;
const WM_APP_MAC_DOCK_SYNC: u32 = WM_APP + 27;
const WM_APP_VIZ_WAKE: u32 = WM_APP + 28;
const WM_APP_EXPORT_LOGS: u32 = WM_APP + 29;
const WM_APP_OPEN_ONBOARDING: u32 = WM_APP + 30;
const WM_APP_DOCK_ICON_READY: u32 = WM_APP + 31;
const WM_APP_WP_STATE_CHANGED: u32 = WM_APP + 32;

const TIMER_SETTLE_HIDE: usize = 101;
const TIMER_EVAL_DEBOUNCE: usize = 102;
const TIMER_EDGE_PEEK: usize = 103;
const TIMER_ENSURE_TASKBAR: usize = 104;
const TIMER_PILL_SYNC: usize = 105;
const TIMER_PILL_RECHECK_1: usize = 106;
const TIMER_PILL_RECHECK_2: usize = 107;
const TIMER_WIDGETS_TICK: usize = 108;
const TIMER_VIZ_TICK: usize = 109;
const TIMER_BOOT_SETTLE: usize = 110;
const TIMER_PILL_RECHECK_3: usize = 111;
const TIMER_DOCK_TICK: usize = 112;
const TIMER_BOOT_WP: usize = 113;
const TIMER_DOCK_SYNC_DEBOUNCE: usize = 114;
const TIMER_CONFIG_SAVE_DEBOUNCE: usize = 115;

static DOCK_SYNC_QUEUED: AtomicBool = AtomicBool::new(false);
static EVAL_SCHED_QUEUED: AtomicBool = AtomicBool::new(false);
static WIDGETS_Z_QUEUED: AtomicBool = AtomicBool::new(false);
static LAST_FG_LOC_EVAL_MS: AtomicU64 = AtomicU64::new(0);

const HOTKEY_TOGGLE_ID: i32 = 1;
const VK_T: u32 = 0x54;

const TRAY_ICON_ID: u32 = 1001;
const IDM_TRAY_TOGGLE_NOW: usize = 3001;
const IDM_TRAY_MODE_SMART: usize = 3002;
const IDM_TRAY_MODE_HOVER: usize = 3003;
const IDM_TRAY_MODE_STEALTH: usize = 3004;
const IDM_TRAY_MODE_ALWAYS: usize = 3005;
const IDM_TRAY_PILL_SPLIT: usize = 3010;
const IDM_TRAY_PILL_UNIFIED: usize = 3011;
const IDM_TRAY_PILL_FULL: usize = 3012;
const IDM_TRAY_PILL_NATIVE: usize = 3013;
const IDM_TRAY_AMBER_RIM: usize = 3014;
const IDM_TRAY_DESKTOP_WIDGETS: usize = 3015;
const IDM_TRAY_WIDGETS_LOCK: usize = 3016;
const IDM_TRAY_SHOW_MUSIC: usize = 3017;
const IDM_TRAY_SHOW_CC: usize = 3018;
const IDM_TRAY_VIDEO_WALLPAPER: usize = 3019;
const IDM_TRAY_GAME_MODE: usize = 3020;
const IDM_TRAY_EXPORT_LOGS: usize = 3021;
const IDM_TRAY_ONBOARDING: usize = 3022;
const IDM_TRAY_APP_SETTINGS: usize = 3023;
const IDM_TRAY_SETTINGS: usize = 3008;
const IDM_TRAY_QUIT: usize = 3009;

const EVENT_OBJECT_SHOW: u32 = 0x8002;
const EVENT_OBJECT_HIDE: u32 = 0x8003;
const EVENT_OBJECT_LOCATIONCHANGE: u32 = 0x800B;
const EVENT_SYSTEM_FOREGROUND: u32 = 0x0003;
const EVENT_SYSTEM_MOVESIZEEND: u32 = 0x000B;
const EVENT_SYSTEM_MINIMIZESTART: u32 = 0x0016;
const EVENT_SYSTEM_MINIMIZEEND: u32 = 0x0017;
const WINEVENT_OUTOFCONTEXT: u32 = 0x0000;
const WINEVENT_SKIPOWNPROCESS: u32 = 0x0002;

struct AppState {
    hidden_hwnd: HWND,
    config: AppConfig,
    hider: TaskbarHider,
    styler: TaskbarStyler,
    mac_dock: MacDockEngine,
    settings: SettingsController,
    widgets: desktop_widgets::DesktopWidgets,
    wallpaper: video_wallpaper::VideoWallpaperEngine,
    tray_icon: HICON,
    taskbar_created_msg: u32,
    peek_leave_ticks: u8,
    preview_hold_ticks: u8,
    boot_settle_ticks: u8,
    viz_timer_ms: u32,
    dock_timer_ms: u32,
    game_mode_active: bool,
    shutting_down: bool,
    os_session_ending: bool,
    maintenance_ticks: u32,
    self_workarea_broadcast_until: Option<Instant>,
    mode_switch_settle_until: Option<Instant>,
    last_eval_recheck: Option<Instant>,
    last_applied_24h: bool,
    config_dirty: bool,
    clean_exit_event: HANDLE,
    hooks: [HWINEVENTHOOK; 5],
}

impl AppState {
    fn unhook_all(&mut self) {
        unsafe {
            for h in &mut self.hooks {
                if !h.0.is_null() {
                    let _ = UnhookWinEvent(*h);
                    *h = HWINEVENTHOOK::default();
                }
            }
        }
    }
}

static mut GLOBAL_STATE_PTR: *mut AppState = null_mut();

impl Drop for AppState {
    fn drop(&mut self) {
        if self.os_session_ending {
            fast_os_session_shutdown(self);
        } else {
            restore_on_exit(self);
        }
        if !self.clean_exit_event.0.is_null() {
            unsafe {
                let _ = CloseHandle(self.clean_exit_event);
            }
            self.clean_exit_event = HANDLE::default();
        }
    }
}

#[link(name = "kernel32")]
extern "system" {
    fn SetConsoleCtrlHandler(
        handler_routine: Option<unsafe extern "system" fn(u32) -> i32>,
        add: i32,
    ) -> i32;
}

fn run_emergency_restore(allow_explorer_restart: bool) -> bool {
    let mut need_explorer = TaskbarStyler::emergency_restore_taskbar_surface();
    TaskbarHider::emergency_restore_windows_taskbar();
    if unsafe { FindWindowW(w!("Shell_TrayWnd"), PCWSTR::null()) }
        .map(|hw| !hw.0.is_null() && !TaskbarHider::is_main_taskbar_visible())
        .unwrap_or(false)
    {
        need_explorer = true;
    }
    if allow_explorer_restart && need_explorer {
        TaskbarStyler::restart_shell_explorer_for_restore();
        return true;
    }
    false
}

unsafe extern "system" fn console_ctrl_handler(ctrl_type: u32) -> i32 {
    const CTRL_LOGOFF_EVENT: u32 = 5;
    const CTRL_SHUTDOWN_EVENT: u32 = 6;
    if matches!(ctrl_type, 0 | 1 | 2 | CTRL_LOGOFF_EVENT | CTRL_SHUTDOWN_EVENT) {
        let is_os_exit = matches!(ctrl_type, CTRL_LOGOFF_EVENT | CTRL_SHUTDOWN_EVENT);
        if !GLOBAL_STATE_PTR.is_null() {
            let st = &mut *GLOBAL_STATE_PTR;
            if !st.clean_exit_event.0.is_null() {
                let _ = SetEvent(st.clean_exit_event);
            }
            if is_os_exit {
                st.os_session_ending = true;
                taskbar_hide::mark_os_session_ending();
            }
            let hw = st.hidden_hwnd;
            if !hw.0.is_null() {
                let _ = PostMessageW(Some(hw), WM_APP_QUIT, WPARAM(0), LPARAM(0));
            }
        }
        if !is_os_exit {
            run_emergency_restore(false);
        }
    }
    0
}

fn parse_watchdog_arg() -> Option<u32> {
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        if a.eq_ignore_ascii_case("--watchdog") {
            if let Some(pid_s) = args.next() {
                if let Ok(pid) = pid_s.parse::<u32>() {
                    return Some(pid);
                }
            }
        }
    }
    None
}

fn run_watchdog(ppid: u32) {
    let ev_name = format!("Local\\AutumnDeckCleanExit_{}", ppid);
    let ev_w = to_wide(&ev_name);
    unsafe {
        let h_clean = OpenEventW(EVENT_ALL_ACCESS, false, PCWSTR(ev_w.as_ptr())).unwrap_or_default();
        if let Ok(h_proc) = OpenProcess(PROCESS_SYNCHRONIZE, false, ppid) {
            if !h_proc.0.is_null() {
                let _ = WaitForSingleObject(h_proc, INFINITE);
                let _ = CloseHandle(h_proc);
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(35));
        let clean = !h_clean.0.is_null() && WaitForSingleObject(h_clean, 0) == WAIT_OBJECT_0;
        if !h_clean.0.is_null() {
            let _ = CloseHandle(h_clean);
        }
        if !clean {
            let restarted = run_emergency_restore(true);
            let path = std::env::temp_dir().join("autumn_deck_status.txt");
            let _ = fs::write(
                path,
                format!(
                    "tag=WATCHDOG_RESTORE ppid={} watchdog_pid={} explorer_restarted={}\r\n",
                    ppid,
                    std::process::id(),
                    restarted
                ),
            );
        }
    }
}

fn spawn_watchdog() -> HANDLE {
    let pid = std::process::id();
    let ev_name = format!("Local\\AutumnDeckCleanExit_{}", pid);
    let ev_w = to_wide(&ev_name);
    let h_ev = unsafe {
        CreateEventW(None, true, false, PCWSTR(ev_w.as_ptr())).unwrap_or_default()
    };
    let ppid_s = pid.to_string();
    TaskbarStyler::spawn_detached_self(&["--watchdog", &ppid_s]);
    h_ev
}

fn is_instance_alive(hwnd: HWND) -> bool {
    if hwnd.0.is_null() {
        return false;
    }
    let mut res = 0usize;
    unsafe {
        SendMessageTimeoutW(
            hwnd,
            WM_APP_PING,
            WPARAM(0),
            LPARAM(0),
            SMTO_ABORTIFHUNG,
            150,
            Some(&mut res),
        )
        .0 != 0
            && res == 1
    }
}

fn main() {
    if let Some(ppid) = parse_watchdog_arg() {
        run_watchdog(ppid);
        return;
    }

    let prev_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        run_emergency_restore(false);
        prev_hook(info);
    }));
    unsafe {
        let _ = SetConsoleCtrlHandler(Some(console_ctrl_handler), 1);
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }

    if let Some(exit_now) = handle_cli_ipc() {
        if exit_now {
            unsafe {
                CoUninitialize();
            }
            return;
        }
    }

    let mut mutex_handle = HANDLE::default();
    let mut owns_mutex = false;
    unsafe {
        if let Ok(hm) = CreateMutexW(None, true, w!("Global\\AutumnDeckV5Mutex")) {
            mutex_handle = hm;
            if GetLastError() != ERROR_ALREADY_EXISTS {
                owns_mutex = true;
            } else {
                // Wait cleanly if a closing instance is still finishing exit, instead of dying silently.
                for _ in 0..40 {
                    if let Ok(existing) = FindWindowW(w!("AutumnDeckHiddenWnd"), PCWSTR::null()) {
                        if !existing.0.is_null() && is_instance_alive(existing) {
                            let _ = PostMessageW(
                                Some(existing),
                                WM_APP_OPEN_SETTINGS,
                                WPARAM(0),
                                LPARAM(0),
                            );
                            let _ = CloseHandle(mutex_handle);
                            CoUninitialize();
                            return;
                        }
                    }
                    let wr = WaitForSingleObject(mutex_handle, 65);
                    if wr == WAIT_OBJECT_0 || wr == WAIT_ABANDONED {
                        owns_mutex = true;
                        break;
                    }
                }
                if !owns_mutex {
                    if let Ok(existing) = FindWindowW(w!("AutumnDeckHiddenWnd"), PCWSTR::null()) {
                        if !existing.0.is_null() && is_instance_alive(existing) {
                            let _ = PostMessageW(
                                Some(existing),
                                WM_APP_OPEN_SETTINGS,
                                WPARAM(0),
                                LPARAM(0),
                            );
                            let _ = CloseHandle(mutex_handle);
                            CoUninitialize();
                            return;
                        }
                    }
                }
            }
        }
    }

    let open_settings_on_start = std::env::args().any(|a| a.eq_ignore_ascii_case("--settings"));
    let open_onboarding_on_start = std::env::args().any(|a| a.eq_ignore_ascii_case("--onboarding"));
    let hinst: HINSTANCE = unsafe { GetModuleHandleW(None).unwrap_or_default().into() };

    register_hidden_class(hinst);
    taskbar_styler::register_styler_classes(hinst, global_wnd_proc);
    mac_dock::register_mac_dock_class(hinst, global_wnd_proc);
    settings_ui::register_settings_class(hinst, global_wnd_proc);

    let hidden_hwnd = unsafe {
        CreateWindowExW(
            WS_EX_TOOLWINDOW,
            w!("AutumnDeckHiddenWnd"),
            w!("AutumnDeckController"),
            WS_POPUP,
            0,
            0,
            0,
            0,
            None,
            None,
            Some(hinst),
            None,
        )
        .expect("Failed to create controller window")
    };

    config::migrate_legacy_appdata_if_needed();
    let config = load_config();
    let hider = TaskbarHider::new();
    let styler = TaskbarStyler::new(hinst, hidden_hwnd, WM_APP_PILLS_READY, WM_APP_TAP_READY);
    let mut mac_dock = MacDockEngine::new(hinst);
    mac_dock.set_notify_target(hidden_hwnd, WM_APP_DOCK_ICON_READY);
    let settings = SettingsController::new(hinst);
    let widgets = desktop_widgets::DesktopWidgets::new(
        hinst,
        hidden_hwnd,
        WM_APP_WIDGET_CLOCK_24H,
        WM_APP_WIDGETS_SYNC,
        WM_APP_MEDIA_REDRAW,
        WM_APP_VIZ_WAKE,
        &config,
    );
    let mut wallpaper = video_wallpaper::VideoWallpaperEngine::new(hinst);
    wallpaper.set_palette_notify(hidden_hwnd, WM_APP_PALETTE_UPDATED);
    wallpaper.set_state_notify(hidden_hwnd, WM_APP_WP_STATE_CHANGED);
    wallpaper.set_chameleon_sampling(config.any_match_wallpaper());
    let tray_icon = create_amber_pill_icon();
    let taskbar_created_msg = unsafe { RegisterWindowMessageW(w!("TaskbarCreated")) };

    let clean_exit_event = spawn_watchdog();

    let initial_24h = config.w_clock.use_24h.unwrap_or(config.widgets_24h);
    let mut boxed = Box::new(AppState {
        hidden_hwnd,
        config,
        hider,
        styler,
        mac_dock,
        settings,
        widgets,
        wallpaper,
        tray_icon,
        taskbar_created_msg,
        peek_leave_ticks: 0,
        preview_hold_ticks: 0,
        boot_settle_ticks: 8,
        viz_timer_ms: 0,
        dock_timer_ms: 0,
        game_mode_active: false,
        shutting_down: false,
        os_session_ending: false,
        maintenance_ticks: 0,
        self_workarea_broadcast_until: None,
        mode_switch_settle_until: None,
        last_eval_recheck: None,
        last_applied_24h: initial_24h,
        config_dirty: false,
        clean_exit_event,
        hooks: [HWINEVENTHOOK::default(); 5],
    });
    diagnostics::record_event(
        "BOOT",
        &format!(
            "suite={} dock={} game_safe={}",
            boxed.config.active_suite,
            boxed.config.mac_dock_enabled,
            boxed.config.game_mode_pause
        ),
    );

    unsafe {
        GLOBAL_STATE_PTR = &mut *boxed;
    }

    add_tray_icon(hidden_hwnd, tray_icon, boxed.hider.is_hidden, &boxed.config);

    unsafe {
        let _ = RegisterHotKey(
            Some(hidden_hwnd),
            HOTKEY_TOGGLE_ID,
            HOT_KEY_MODIFIERS(MOD_CONTROL.0 | MOD_ALT.0 | MOD_NOREPEAT.0),
            VK_T,
        );
    }

    let hook_show = unsafe {
        SetWinEventHook(
            EVENT_OBJECT_SHOW,
            EVENT_OBJECT_HIDE,
            None,
            Some(win_event_proc),
            0,
            0,
            WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
        )
    };
    let hook_loc = unsafe {
        SetWinEventHook(
            EVENT_OBJECT_LOCATIONCHANGE,
            EVENT_OBJECT_LOCATIONCHANGE,
            None,
            Some(win_event_proc),
            0,
            0,
            WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
        )
    };
    let hook_fg = unsafe {
        SetWinEventHook(
            EVENT_SYSTEM_FOREGROUND,
            EVENT_SYSTEM_FOREGROUND,
            None,
            Some(win_event_proc),
            0,
            0,
            WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
        )
    };
    let hook_move = unsafe {
        SetWinEventHook(
            EVENT_SYSTEM_MOVESIZEEND,
            EVENT_SYSTEM_MOVESIZEEND,
            None,
            Some(win_event_proc),
            0,
            0,
            WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
        )
    };
    let hook_min = unsafe {
        SetWinEventHook(
            EVENT_SYSTEM_MINIMIZESTART,
            EVENT_SYSTEM_MINIMIZEEND,
            None,
            Some(win_event_proc),
            0,
            0,
            WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
        )
    };
    boxed.hooks = [hook_show, hook_loc, hook_fg, hook_move, hook_min];
    if boxed.hider.main_taskbar_exists() {
        evaluate_mode_and_apply(&mut boxed, true);
        write_status_snapshot("INIT", &boxed);
    } else {
        unsafe {
            let _ = SetTimer(Some(hidden_hwnd), TIMER_ENSURE_TASKBAR, 600, None);
        }
    }

    if open_onboarding_on_start || !boxed.config.onboarding_done {
        sync_studio_live(&mut boxed);
        boxed
            .settings
            .open_onboarding(&boxed.config, boxed.hider.is_hidden);
    } else if open_settings_on_start {
        sync_studio_live(&mut boxed);
        boxed.settings.open(&boxed.config, boxed.hider.is_hidden);
    }

    unsafe {
        let _ = SetTimer(Some(hidden_hwnd), TIMER_EDGE_PEEK, 60, None);
        let _ = SetTimer(Some(hidden_hwnd), TIMER_PILL_SYNC, 5000, None);
        let _ = SetTimer(Some(hidden_hwnd), TIMER_WIDGETS_TICK, 400, None);
        let _ = SetTimer(Some(hidden_hwnd), TIMER_BOOT_SETTLE, 450, None);
        let _ = PostMessageW(Some(hidden_hwnd), WM_APP_DEFERRED_INIT, WPARAM(0), LPARAM(0));
    }
    start_explorer_pref_watcher(hidden_hwnd, WM_APP_EXPLORER_PREF_CHANGED);

    unsafe {
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }

    unsafe {
        boxed.unhook_all();
        let _ = UnregisterHotKey(Some(hidden_hwnd), HOTKEY_TOGGLE_ID);
        if !boxed.os_session_ending {
            remove_tray_icon(hidden_hwnd);
        }
        if !boxed.tray_icon.0.is_null() {
            let _ = DestroyIcon(boxed.tray_icon);
        }
        GLOBAL_STATE_PTR = null_mut();
        drop(boxed);
        if !mutex_handle.0.is_null() {
            if owns_mutex {
                let _ = ReleaseMutex(mutex_handle);
            }
            let _ = CloseHandle(mutex_handle);
        }
        CoUninitialize();
    }
}

fn handle_cli_ipc() -> Option<bool> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() <= 1 {
        return None;
    }
    let existing = unsafe { FindWindowW(w!("AutumnDeckHiddenWnd"), PCWSTR::null()) }.ok();
    for arg in &args[1..] {
        let low = arg.to_ascii_lowercase();
        if low == "--export-logs" {
            let silent = args[1..].iter().any(|a| a.eq_ignore_ascii_case("--silent"));
            if !silent {
                if let Some(hwnd) = existing {
                    if !hwnd.0.is_null() && is_instance_alive(hwnd) {
                        unsafe {
                            let _ = PostMessageW(Some(hwnd), WM_APP_EXPORT_LOGS, WPARAM(0), LPARAM(0));
                        }
                        return Some(true);
                    }
                }
            }
            let cfg = load_config();
            let _ = diagnostics::export_support_bundle(&cfg, false, !silent);
            return Some(true);
        }
        if low == "--repair-acl" {
            let _ = taskbar_styler::ensure_sibling_tap_dll_acl();
            return Some(true);
        }
        if low == "--uninstall-cleanup" {
            if let Some(hwnd) = existing {
                if !hwnd.0.is_null() {
                    unsafe {
                        let _ = PostMessageW(Some(hwnd), WM_APP_QUIT, WPARAM(0), LPARAM(0));
                    }
                    // Wait up to 3 seconds for clean exit & taskbar/workarea restoration
                    for _ in 0..30 {
                        std::thread::sleep(std::time::Duration::from_millis(100));
                        let still =
                            unsafe { FindWindowW(w!("AutumnDeckHiddenWnd"), PCWSTR::null()) }
                                .ok()
                                .filter(|h| !h.0.is_null());
                        if still.is_none() {
                            break;
                        }
                    }
                }
            }
            if !TaskbarHider::is_main_taskbar_visible() || TaskbarHider::is_main_taskbar_cloaked() {
                run_emergency_restore(false);
            }
            config::disable_startup_sync();
            let _ = fs::remove_file(std::env::temp_dir().join("autumn_deck_status.txt"));
            return Some(true);
        }
        if low == "--restore" {
            if let Some(hwnd) = existing {
                if !hwnd.0.is_null() && is_instance_alive(hwnd) {
                    unsafe {
                        let _ = PostMessageW(Some(hwnd), WM_APP_FORCE_SHOW, WPARAM(0), LPARAM(0));
                    }
                    return Some(true);
                }
            }
            let restarted = run_emergency_restore(true);
            let tray_vis = if TaskbarHider::is_main_taskbar_visible() { 1 } else { 0 };
            let path = std::env::temp_dir().join("autumn_deck_status.txt");
            let _ = fs::write(
                path,
                format!(
                    "tag=EMERGENCY_RESTORE pid={} explorer_restarted={} tray_vis={}\r\n",
                    std::process::id(),
                    restarted,
                    tray_vis
                ),
            );
            return Some(true);
        }
        if low == "--restore-explorer" {
            if let Some(hwnd) = existing {
                if !hwnd.0.is_null() && is_instance_alive(hwnd) {
                    unsafe {
                        let _ = PostMessageW(Some(hwnd), WM_APP_QUIT, WPARAM(0), LPARAM(0));
                    }
                    std::thread::sleep(std::time::Duration::from_millis(600));
                }
            }
            TaskbarStyler::restart_shell_explorer_for_restore();
            return Some(true);
        }
        let cmd_msg = match low.as_str() {
            "--toggle" => Some(WM_APP_TOGGLE_HIDE),
            "--hide" => Some(WM_APP_FORCE_HIDE),
            "--show" => Some(WM_APP_FORCE_SHOW),
            "--settings" => Some(WM_APP_OPEN_SETTINGS),
            "--onboarding" => Some(WM_APP_OPEN_ONBOARDING),
            "--quit" | "--exit" => Some(WM_APP_QUIT),
            _ => None,
        };
        if let Some(msg) = cmd_msg {
            if let Some(hwnd) = existing {
                if !hwnd.0.is_null() && (msg == WM_APP_QUIT || is_instance_alive(hwnd)) {
                    unsafe {
                        let _ = PostMessageW(Some(hwnd), msg, WPARAM(0), LPARAM(0));
                    }
                    return Some(true);
                }
            }
            if msg == WM_APP_QUIT {
                run_emergency_restore(true);
                return Some(true);
            }
        }
    }
    None
}

fn register_hidden_class(hinst: HINSTANCE) {
    unsafe {
        let mut wc = WNDCLASSEXW::default();
        wc.cbSize = std::mem::size_of::<WNDCLASSEXW>() as u32;
        wc.lpfnWndProc = Some(global_wnd_proc);
        wc.hInstance = hinst;
        wc.lpszClassName = w!("AutumnDeckHiddenWnd");
        let _ = RegisterClassExW(&wc);
    }
}

unsafe extern "system" fn win_event_proc(
    _hook: HWINEVENTHOOK,
    event: u32,
    hwnd: HWND,
    id_object: i32,
    id_child: i32,
    _thread: u32,
    _time: u32,
) {
    if id_object != 0 || id_child != 0 || hwnd.0.is_null() || GLOBAL_STATE_PTR.is_null() {
        return;
    }
    let state = &*GLOBAL_STATE_PTR;
    if state.shutting_down {
        return;
    }
    let (is_glass, is_blur) = state.styler.is_glass_or_blur_hwnd(hwnd);
    if hwnd == state.hidden_hwnd
        || is_glass
        || is_blur
        || hwnd == state.settings.hwnd()
        || hwnd == state.widgets.hwnd()
        || state.mac_dock.owns_hwnd(hwnd)
    {
        return;
    }

    let eval_needed = state.config.mode == TaskbarMode::SmartMaximize
        || (state.config.mac_dock_enabled && state.config.mac_dock_hide == MacDockHideMode::SmartAutoHide)
        || (state.config.wallpaper_enabled && state.config.wallpaper_pause_maximized);

    let post_dock_sync = || {
        if !DOCK_SYNC_QUEUED.swap(true, Ordering::AcqRel) {
            let _ = PostMessageW(
                Some(state.hidden_hwnd),
                WM_APP_MAC_DOCK_SYNC,
                WPARAM(0),
                LPARAM(0),
            );
        }
    };
    let post_sched_eval = || {
        if !EVAL_SCHED_QUEUED.swap(true, Ordering::AcqRel) {
            let _ = PostMessageW(
                Some(state.hidden_hwnd),
                WM_APP_SCHED_EVAL,
                WPARAM(0),
                LPARAM(0),
            );
        }
    };
    let post_widgets_z = || {
        if !WIDGETS_Z_QUEUED.swap(true, Ordering::AcqRel) {
            let _ = PostMessageW(
                Some(state.hidden_hwnd),
                WM_APP_ENSURE_WIDGETS_Z,
                WPARAM(0),
                LPARAM(0),
            );
        }
    };

    if event == EVENT_SYSTEM_FOREGROUND || event == EVENT_SYSTEM_MINIMIZESTART || event == EVENT_SYSTEM_MINIMIZEEND {
        if state.config.mac_dock_enabled {
            post_dock_sync();
        }
        if state.config.widgets_enabled {
            post_widgets_z();
        }
        if eval_needed {
            post_sched_eval();
        }
    }

    if event == EVENT_OBJECT_SHOW {
        if state.hider.is_hidden
            && !state.hider.peek_visible
            && !mac_dock::is_tray_overflow_active()
        {
            state.hider.handle_show_event(hwnd);
            let _ = PostMessageW(
                Some(state.hidden_hwnd),
                WM_APP_TASKBAR_SHOW_EVT,
                WPARAM(hwnd.0 as usize),
                LPARAM(0),
            );
        }
        let style = GetWindowLongW(hwnd, GWL_STYLE) as u32;
        let ex = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
        let is_top_app_win = (style & WS_CHILD.0) == 0 && (ex & WS_EX_TOOLWINDOW.0) == 0;
        if is_top_app_win {
            if state.config.mac_dock_enabled {
                post_dock_sync();
            }
            if eval_needed {
                post_sched_eval();
            }
        }
        return;
    }

    if event == EVENT_OBJECT_HIDE {
        let style = GetWindowLongW(hwnd, GWL_STYLE) as u32;
        let ex = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
        let is_top_app_win = (style & WS_CHILD.0) == 0 && (ex & WS_EX_TOOLWINDOW.0) == 0;
        if is_top_app_win {
            if state.config.mac_dock_enabled {
                post_dock_sync();
            }
            if eval_needed {
                post_sched_eval();
            }
        }
        return;
    }

    let in_mode_switch_settle = state
        .mode_switch_settle_until
        .map(|t| Instant::now() < t)
        .unwrap_or(false);

    if event == EVENT_OBJECT_LOCATIONCHANGE {
        let tray_hw = state.styler.tracked_tray_hwnd();
        let on_tray = !state.config.mac_dock_enabled
            && (!state.hider.is_hidden || state.hider.peek_visible)
            && !tray_hw.0.is_null()
            && hwnd == tray_hw;
        if on_tray {
            if !in_mode_switch_settle {
                post_sched_eval();
            }
        } else if eval_needed && hwnd == GetForegroundWindow() {
            let now_ms = GetTickCount64();
            let prev = LAST_FG_LOC_EVAL_MS.load(Ordering::Relaxed);
            if now_ms.saturating_sub(prev) >= 240
                && LAST_FG_LOC_EVAL_MS
                    .compare_exchange(prev, now_ms, Ordering::AcqRel, Ordering::Relaxed)
                    .is_ok()
            {
                post_sched_eval();
            }
        }
        return;
    }

    if state.config.mac_dock_enabled {
        if eval_needed {
            post_sched_eval();
        }
    } else if state.config.mode == TaskbarMode::SmartMaximize {
        post_sched_eval();
    } else if (!state.hider.is_hidden || state.hider.peek_visible)
        && event != EVENT_SYSTEM_MOVESIZEEND
        && !in_mode_switch_settle
    {
        post_sched_eval();
    }
}

unsafe extern "system" fn global_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if GLOBAL_STATE_PTR.is_null() {
        return DefWindowProcW(hwnd, msg, wparam, lparam);
    }
    let state = &mut *GLOBAL_STATE_PTR;

    if !state.settings.hwnd().0.is_null() && hwnd == state.settings.hwnd() {
        const WM_ACTIVATE_MSG: u32 = 0x0006;
        const WM_MOUSELEAVE_MSG: u32 = 0x02A3;
        const WM_MOUSEMOVE_MSG: u32 = 0x0200;
        const WM_LBUTTONDOWN_MSG: u32 = 0x0201;
        const WM_LBUTTONUP_MSG: u32 = 0x0202;
        const WM_SIZE_MSG: u32 = 0x0005;
        const WM_KILLFOCUS_MSG: u32 = 0x0008;
        const WM_KEYDOWN_MSG: u32 = 0x0100;

        let prev_pill = state.config.pill_mode;
        if let Some((res, cmd)) = state.settings.handle_message(
            hwnd,
            msg,
            wparam,
            lparam,
            &mut state.config,
            state.hider.is_hidden,
        ) {
            if matches!(cmd, SettingsCommand::None) {
                if matches!(
                    msg,
                    WM_MOUSEMOVE_MSG
                        | WM_MOUSELEAVE_MSG
                        | WM_LBUTTONDOWN_MSG
                        | WM_LBUTTONUP_MSG
                        | WM_SIZE_MSG
                        | WM_CLOSE
                        | WM_KILLFOCUS_MSG
                        | WM_KEYDOWN_MSG
                ) {
                    update_viz_timer(state);
                }
            } else {
                handle_settings_command(state, cmd, prev_pill);
            }
            return res;
        }
        let res = DefWindowProcW(hwnd, msg, wparam, lparam);
        if msg == WM_ACTIVATE_MSG {
            update_viz_timer(state);
        }
        return res;
    }

    if hwnd != state.hidden_hwnd {
        if state.mac_dock.owns_hwnd(hwnd) {
            if msg == state.mac_dock.shell_hook_msg() && state.mac_dock.shell_hook_msg() != 0 {
                let needs_debounce = state.mac_dock.handle_shell_hook(
                    wparam.0 as u32,
                    HWND(lparam.0 as *mut std::ffi::c_void),
                    &state.config,
                );
                if needs_debounce {
                    let _ = SetTimer(Some(state.hidden_hwnd), TIMER_DOCK_SYNC_DEBOUNCE, 55, None);
                }
                update_dock_timer(state);
                return LRESULT(0);
            }
            match msg {
                0x0084 => {
                    let sx = (lparam.0 & 0xFFFF) as i16 as i32;
                    let sy = ((lparam.0 >> 16) & 0xFFFF) as i16 as i32;
                    return state.mac_dock.handle_nchittest(hwnd, sx, sy);
                }
                0x0021 => {
                    let m_msg = ((lparam.0 >> 16) & 0xFFFF) as u32;
                    if m_msg == 0x0204 || m_msg == 0x0205 {
                        return LRESULT(1); // MA_ACTIVATE on right-click for reliable TrackPopupMenu
                    }
                    return LRESULT(3); // WM_MOUSEACTIVATE -> MA_NOACTIVATE
                }
                0x0020 => return state.mac_dock.handle_set_cursor(hwnd),
                0x0014 => return LRESULT(1),
                0x0200 => {
                    state.mac_dock.handle_mouse_move(hwnd, &state.config);
                    update_dock_timer(state);
                    return LRESULT(0);
                }
                0x02A3 => {
                    state.mac_dock.handle_mouse_leave(hwnd, &state.config);
                    update_dock_timer(state);
                    return LRESULT(0);
                }
                0x0201 => {
                    let lx = (lparam.0 & 0xFFFF) as i16 as f32;
                    let ly = ((lparam.0 >> 16) & 0xFFFF) as i16 as f32;
                    state.mac_dock.handle_lbutton_down(hwnd, lx, ly, &state.config);
                    update_dock_timer(state);
                    return LRESULT(0);
                }
                0x0202 => {
                    let lx = (lparam.0 & 0xFFFF) as i16 as f32;
                    let ly = ((lparam.0 >> 16) & 0xFFFF) as i16 as f32;
                    let act = state.mac_dock.handle_lbutton_up(hwnd, lx, ly, &state.config);
                    handle_dock_action(state, act);
                    return LRESULT(0);
                }
                0x0208 => {
                    let lx = (lparam.0 & 0xFFFF) as i16 as f32;
                    let ly = ((lparam.0 >> 16) & 0xFFFF) as i16 as f32;
                    let act = state.mac_dock.handle_mbutton_up(hwnd, lx, ly, &state.config);
                    handle_dock_action(state, act);
                    return LRESULT(0);
                }
                0x020A => {
                    let delta = ((wparam.0 >> 16) & 0xFFFF) as i16;
                    let sx = (lparam.0 & 0xFFFF) as i16 as i32;
                    let sy = ((lparam.0 >> 16) & 0xFFFF) as i16 as i32;
                    let act = state
                        .mac_dock
                        .handle_mouse_wheel(hwnd, delta, sx, sy, &mut state.config);
                    handle_dock_action(state, act);
                    return LRESULT(0);
                }
                0x0204 => {
                    state.mac_dock.cancel_left_press();
                    return LRESULT(0);
                }
                0x0205 => {
                    let lx = (lparam.0 & 0xFFFF) as i16 as f32;
                    let ly = ((lparam.0 >> 16) & 0xFFFF) as i16 as f32;
                    let act = state.mac_dock.handle_rbutton_up(hwnd, lx, ly, &mut state.config);
                    handle_dock_action(state, act);
                    return LRESULT(0);
                }
                0x0010 => return LRESULT(0),
                0x007B => return LRESULT(0),
                _ => return DefWindowProcW(hwnd, msg, wparam, lparam),
            }
        }
        if !state.mac_dock.thumb_hwnd().0.is_null() && hwnd == state.mac_dock.thumb_hwnd() {
            match msg {
                0x0010 => return LRESULT(0),
                0x0084 => return LRESULT(1), // HTCLIENT
                0x0021 => return LRESULT(3), // MA_NOACTIVATE
                0x0020 => return state.mac_dock.handle_thumb_set_cursor(),
                0x0014 => return LRESULT(1),
                0x000F => {
                    state.mac_dock.handle_thumb_paint(&state.config);
                    return LRESULT(0);
                }
                0x0200 => {
                    state.mac_dock.handle_thumb_mouse_move(&state.config);
                    return LRESULT(0);
                }
                0x02A3 => {
                    state.mac_dock.handle_thumb_mouse_leave(&state.config);
                    update_dock_timer(state);
                    return LRESULT(0);
                }
                0x0202 => {
                    let lx = (lparam.0 & 0xFFFF) as i16 as f32;
                    let ly = ((lparam.0 >> 16) & 0xFFFF) as i16 as f32;
                    let act = state
                        .mac_dock
                        .handle_thumb_lbutton_up(lx, ly, &state.config);
                    handle_dock_action(state, act);
                    return LRESULT(0);
                }
                0x0208 => {
                    let lx = (lparam.0 & 0xFFFF) as i16 as f32;
                    let ly = ((lparam.0 >> 16) & 0xFFFF) as i16 as f32;
                    state
                        .mac_dock
                        .handle_thumb_mbutton_up(lx, ly, &state.config);
                    update_dock_timer(state);
                    return LRESULT(0);
                }
                _ => return DefWindowProcW(hwnd, msg, wparam, lparam),
            }
        }
        let (is_glass, is_blur) = state.styler.is_glass_or_blur_hwnd(hwnd);
        if is_glass || is_blur {
            match msg {
                0x0010 => return LRESULT(0),
                0x0084 => return LRESULT(-1), // WM_NCHITTEST -> HTTRANSPARENT
                0x0014 => return LRESULT(1),  // WM_ERASEBKGND -> prevent white/black flash
                0x000F if is_blur => {
                    // WM_PAINT on blur_hwnd with DwmExtendFrameIntoClientArea(-1): fill ARGB (0,0,0,0)
                    let mut ps = PAINTSTRUCT::default();
                    let hdc = BeginPaint(hwnd, &mut ps);
                    if !hdc.0.is_null() {
                        let _ = FillRect(hdc, &ps.rcPaint, HBRUSH(GetStockObject(BLACK_BRUSH).0));
                    }
                    let _ = EndPaint(hwnd, &ps);
                    return LRESULT(0);
                }
                _ => {}
            }
        }
        return DefWindowProcW(hwnd, msg, wparam, lparam);
    }

    if msg == state.taskbar_created_msg && state.taskbar_created_msg != 0 {
        if state.shutting_down {
            return LRESULT(0);
        }
        add_tray_icon(
            state.hidden_hwnd,
            state.tray_icon,
            state.hider.is_hidden,
            &state.config,
        );
        state.hider.clear_monitor_cache();
        state.styler.invalidate_cache();
        if state.config.wallpaper_enabled {
            state.wallpaper.on_explorer_restarted();
        }
        state.widgets.rebind_desktop_host();
        state.widgets.render();
        evaluate_mode_and_apply(state, true);
        schedule_pill_rechecks(state.hidden_hwnd);
        state.boot_settle_ticks = 8;
        let _ = SetTimer(Some(state.hidden_hwnd), TIMER_BOOT_SETTLE, 450, None);
        return LRESULT(0);
    }

    match msg {
        WM_HOTKEY if wparam.0 as i32 == HOTKEY_TOGGLE_ID => {
            toggle_taskbar_manual(state);
            LRESULT(0)
        }
        WM_APP_TOGGLE_HIDE => {
            toggle_taskbar_manual(state);
            LRESULT(0)
        }
        WM_APP_FORCE_HIDE => {
            state.config.mode = TaskbarMode::StealthHide;
            state.config.sync_dock_hide_from_mode();
            schedule_config_save(state);
            evaluate_mode_and_apply(state, false);
            LRESULT(0)
        }
        WM_APP_FORCE_SHOW => {
            state.config.mode = TaskbarMode::AlwaysVisible;
            state.config.sync_dock_hide_from_mode();
            schedule_config_save(state);
            evaluate_mode_and_apply(state, true);
            LRESULT(0)
        }
        WM_APP_PING => LRESULT(if state.shutting_down { 0 } else { 1 }),
        WM_APP_OPEN_SETTINGS => {
            if !state.shutting_down {
                sync_studio_live(state);
                state.settings.open(&state.config, state.hider.is_hidden);
            }
            LRESULT(0)
        }
        WM_APP_OPEN_ONBOARDING => {
            if !state.shutting_down {
                sync_studio_live(state);
                state
                    .settings
                    .open_onboarding(&state.config, state.hider.is_hidden);
            }
            LRESULT(0)
        }
        WM_APP_EXPORT_LOGS => {
            if !state.shutting_down {
                if diagnostics::export_support_bundle(&state.config, state.game_mode_active, true)
                    .is_ok()
                {
                    state.settings.mark_diagnostics_exported();
                }
            }
            LRESULT(0)
        }
        WM_APP_QUIT => {
            shutdown_app(state);
            LRESULT(0)
        }
        WM_APP_TASKBAR_SHOW_EVT => {
            let target = HWND(wparam.0 as *mut _);
            if state.hider.is_hidden
                && !state.hider.peek_visible
                && !mac_dock::is_tray_overflow_active()
            {
                state.hider.handle_show_event(target);
                state.styler.hide_overlays();
            }
            LRESULT(0)
        }
        WM_APP_PILLS_READY => {
            if !state.config.mac_dock_enabled && (!state.hider.is_hidden || state.hider.peek_visible)
            {
                state.styler.apply_async_pills(&state.config);
            }
            LRESULT(0)
        }
        WM_APP_TAP_READY => {
            if !state.shutting_down {
                let connected = wparam.0 != 0;
                if !state.config.mac_dock_enabled
                    && (!state.hider.is_hidden || state.hider.peek_visible)
                {
                    if connected {
                        state.styler.on_tap_connected(&state.config);
                        schedule_pill_rechecks(state.hidden_hwnd);
                        write_status_snapshot("TAP_READY", state);
                    } else {
                        state.styler.sync_taskbar(&state.config, false);
                        write_status_snapshot("TAP_FALLBACK", state);
                    }
                } else if connected {
                    state.styler.note_tap_ready();
                }
                sync_studio_live(state);
            }
            LRESULT(0)
        }
        WM_APP_DEFERRED_INIT => {
            if !state.shutting_down {
                std::thread::spawn(|| {
                    config::upgrade_startup_to_fast_logon();
                    config::ensure_theme_folders_on_disk();
                });
                state.widgets.attach();
                state.widgets.apply_config(&state.config);
                state.wallpaper.attach();
                if state.config.wallpaper_enabled {
                    // On fresh Windows boot (<90s uptime), defer heavy MediaFoundation/DXVA video
                    // wallpaper initialization by 2.4s so Windows Explorer & logon apps stay snappy.
                    let wp_delay_ms = if GetTickCount64() < 90_000 { 2400 } else { 120 };
                    let _ = SetTimer(Some(state.hidden_hwnd), TIMER_BOOT_WP, wp_delay_ms, None);
                }
                sync_studio_live(state);
                update_viz_timer(state);
                update_dock_timer(state);
                write_status_snapshot("READY", state);
            }
            LRESULT(0)
        }
        WM_APP_VIZ_WAKE => {
            if !state.shutting_down {
                let was_off = state.viz_timer_ms == 0;
                if was_off {
                    state.widgets.reset_fx_clock();
                }
                update_viz_timer(state);
                if was_off && (state.widgets.wants_audio_viz_tick() || state.widgets.has_active_interactive_fx()) {
                    state.widgets.tick_fast();
                }
            }
            LRESULT(0)
        }
        WM_APP_SCHED_EVAL => {
            EVAL_SCHED_QUEUED.store(false, Ordering::Release);
            let _ = SetTimer(Some(state.hidden_hwnd), TIMER_EVAL_DEBOUNCE, 40, None);
            LRESULT(0)
        }
        WM_APP_WIDGET_CLOCK_24H => {
            let use_24h = wparam.0 != 0;
            state.config.set_clock_24h(use_24h);
            schedule_config_save(state);
            sync_clock_24h_if_changed(state);
            state.settings.sync_from_config(&state.config);
            LRESULT(0)
        }
        WM_APP_ENSURE_WIDGETS_Z => {
            WIDGETS_Z_QUEUED.store(false, Ordering::Release);
            state.widgets.ensure_above_desktop();
            LRESULT(0)
        }
        WM_APP_WIDGETS_SYNC => {
            state.config = state.widgets.cfg.clone();
            schedule_config_save(state);
            state.settings.sync_from_config(&state.config);
            if wparam.0 != 0 {
                sync_studio_live(state);
                match wparam.0 {
                    1 => state.settings.open_tab(
                        &state.config,
                        state.hider.is_hidden,
                        StudioTab::DesktopWidgets,
                        WidgetPage::CustomizeApps,
                    ),
                    2 => state.settings.open_tab(
                        &state.config,
                        state.hider.is_hidden,
                        StudioTab::DesktopWidgets,
                        WidgetPage::MusicFolders,
                    ),
                    3 => state.settings.open_tab(
                        &state.config,
                        state.hider.is_hidden,
                        StudioTab::DesktopWidgets,
                        WidgetPage::CustomizeControlCenter,
                    ),
                    4 => state.settings.open_tab(
                        &state.config,
                        state.hider.is_hidden,
                        StudioTab::Wallpaper,
                        WidgetPage::Gallery,
                    ),
                    5 => state.settings.open_tab(
                        &state.config,
                        state.hider.is_hidden,
                        StudioTab::DesktopWidgets,
                        WidgetPage::PickInstalledApp,
                    ),
                    n if (10..24).contains(&n) => {
                        if let Some(id) = crate::widgets_extra::ExtWidgetId::from_idx(n - 10) {
                            state.settings.open_tab(
                                &state.config,
                                state.hider.is_hidden,
                                StudioTab::DesktopWidgets,
                                WidgetPage::ExtFolders(id),
                            );
                        }
                    }
                    _ => state.settings.open(&state.config, state.hider.is_hidden),
                }
                update_viz_timer(state);
            }
            LRESULT(0)
        }
        WM_APP_MEDIA_REDRAW => {
            state.widgets.on_media_update();
            if state.config.mac_dock_enabled {
                sync_dock_snapshots(state);
                update_dock_timer(state);
            }
            sync_studio_live(state);
            update_viz_timer(state);
            LRESULT(0)
        }
        WM_APP_PALETTE_UPDATED => {
            if state.config.any_match_wallpaper() {
                state.widgets.apply_config(&state.config);
                state.settings.sync_from_config(&state.config);
                if (!state.hider.is_hidden || state.hider.peek_visible) && state.config.tb_match_wallpaper {
                    state.styler.invalidate_style_only();
                    state.styler.sync_style_fast(&state.config);
                }
                if state.config.mac_dock_enabled && state.config.tb_match_wallpaper {
                    state.mac_dock.mark_dirty_paint();
                    state.mac_dock.tick(&state.config);
                }
            }
            LRESULT(0)
        }
        WM_APP_EXPLORER_PREF_CHANGED => {
            state.settings.refresh_win_prefs();
            if state.config.dock_sync_win_pins {
                let win_pins = config::read_windows_taskbar_pins();
                if !win_pins.is_empty() {
                    let changed = win_pins.len() != state.config.mac_dock_pinned.len()
                        || win_pins.iter().zip(&state.config.mac_dock_pinned).any(|(a, b)| {
                            a.name != b.name || !a.path.eq_ignore_ascii_case(&b.path)
                        });
                    if changed {
                        state.config.mac_dock_pinned = win_pins;
                        state.mac_dock.clear_icon_cache();
                        schedule_config_save(state);
                    }
                }
            }
            if !state.hider.is_hidden {
                state.hider.clear_monitor_cache();
            } else {
                state.hider.refresh_geometry_cache();
            }
            state.styler.invalidate_for_layout_change();
            evaluate_mode_and_apply(state, true);
            if !state.config.mac_dock_enabled {
                schedule_pill_rechecks(state.hidden_hwnd);
            }
            write_status_snapshot("PREF_CHANGED", state);
            LRESULT(0)
        }
        WM_APP_MAC_DOCK_SYNC => {
            DOCK_SYNC_QUEUED.store(false, Ordering::Release);
            if state.config.mac_dock_enabled {
                state.mac_dock.invalidate_windows();
                let _ = SetTimer(Some(state.hidden_hwnd), TIMER_DOCK_SYNC_DEBOUNCE, 55, None);
            }
            LRESULT(0)
        }
        WM_APP_DOCK_ICON_READY => {
            if !state.shutting_down && state.config.mac_dock_enabled {
                if state.mac_dock.apply_ready_icons() {
                    state.mac_dock.tick(&state.config);
                    update_dock_timer(state);
                }
            }
            LRESULT(0)
        }
        WM_APP_WP_STATE_CHANGED => {
            if !state.shutting_down {
                let _ = state.wallpaper.drain_static_load_results();
                if let Some((old_p, new_p)) = state.wallpaper.take_cached_path_update() {
                    if state.config.wallpaper_path.eq_ignore_ascii_case(&old_p) {
                        state.config.wallpaper_path = new_p.clone();
                        schedule_config_save(state);
                    }
                    state.settings.sync_cached_wallpaper_path(&old_p, &new_p);
                }
                if state.wallpaper.is_visible_on_desktop() {
                    state.widgets.rebind_desktop_host();
                }
                sync_studio_live(state);
                write_status_snapshot("WALLPAPER", state);
            }
            LRESULT(0)
        }
        WM_TIMER => {
            if state.shutting_down {
                return LRESULT(0);
            }
            match wparam.0 {
                TIMER_SETTLE_HIDE => {
                    let _ = KillTimer(Some(state.hidden_hwnd), TIMER_SETTLE_HIDE);
                    if mac_dock::is_tray_overflow_active() {
                        let _ = SetTimer(Some(state.hidden_hwnd), TIMER_SETTLE_HIDE, 240, None);
                    } else if state.hider.is_hidden && !state.hider.peek_visible {
                        let dock_reserves = if state.config.mac_dock_enabled {
                            state.mac_dock.desired_work_area_reserves(&state.config)
                        } else {
                            Vec::new()
                        };
                        let mut wa_changed = state.hider.settle_hidden(dock_reserves.is_empty());
                        if !dock_reserves.is_empty() {
                            wa_changed |= TaskbarHider::apply_dock_work_area(&dock_reserves);
                        }
                        if wa_changed {
                            state.self_workarea_broadcast_until =
                                Some(Instant::now() + Duration::from_millis(1200));
                        }
                        state.styler.hide_overlays();
                        write_status_snapshot("SETTLE", state);
                    }
                }
                TIMER_EVAL_DEBOUNCE => {
                    let _ = KillTimer(Some(state.hidden_hwnd), TIMER_EVAL_DEBOUNCE);
                    if state.config.mac_dock_enabled {
                        if state.config.mac_dock_hide == MacDockHideMode::SmartAutoHide
                            || (state.config.wallpaper_enabled
                                && state.config.wallpaper_pause_maximized)
                            || state.config.game_mode_pause
                        {
                            let has_max = has_maximized_or_fullscreen_window();
                            let game_fs = state.config.game_mode_pause
                                && is_fullscreen_game_or_app_active();
                            state.game_mode_active = game_fs;
                            state.widgets.set_covered_by_maximized(has_max || game_fs);
                            let prev_wp = state.wallpaper.is_active();
                            state.wallpaper.update_pause_for_cover(
                                (has_max && state.config.wallpaper_pause_maximized) || game_fs,
                            );
                            if prev_wp != state.wallpaper.is_active() {
                                sync_studio_live(state);
                                write_status_snapshot("WALLPAPER", state);
                            }
                            state.mac_dock.tick(&state.config);
                            update_dock_timer(state);
                            update_viz_timer(state);
                        }
                    } else if state.styler.bar_geometry_changed() {
                        if !state.hider.is_hidden {
                            state.hider.clear_monitor_cache();
                        } else {
                            state.hider.refresh_geometry_cache();
                        }
                        if state.config.mode != TaskbarMode::AlwaysVisible {
                            evaluate_mode_and_apply(state, true);
                        } else {
                            state.styler.sync_taskbar(&state.config, true);
                        }
                        schedule_pill_rechecks(state.hidden_hwnd);
                    } else if state.config.mode != TaskbarMode::AlwaysVisible {
                        if state.hider.is_hidden {
                            state.hider.refresh_geometry_cache();
                        }
                        evaluate_mode_and_apply(
                            state,
                            state.hider.is_hidden && !state.hider.peek_visible,
                        );
                        if !state.hider.is_hidden || state.hider.peek_visible {
                            schedule_pill_rechecks(state.hidden_hwnd);
                        }
                    } else {
                        if (state.config.wallpaper_enabled && state.config.wallpaper_pause_maximized)
                            || state.config.game_mode_pause
                        {
                            let has_max = has_maximized_or_fullscreen_window();
                            let game_fs = state.config.game_mode_pause
                                && is_fullscreen_game_or_app_active();
                            state.game_mode_active = game_fs;
                            state.widgets.set_covered_by_maximized(has_max || game_fs);
                            let prev_wp = state.wallpaper.is_active();
                            state.wallpaper.update_pause_for_cover(
                                (has_max && state.config.wallpaper_pause_maximized) || game_fs,
                            );
                            if prev_wp != state.wallpaper.is_active() {
                                sync_studio_live(state);
                                write_status_snapshot("WALLPAPER", state);
                            }
                            update_viz_timer(state);
                        }
                        if !state.hider.is_hidden || state.hider.peek_visible {
                            state.styler.ensure_zorder_only(&state.config);
                            if matches!(
                                state.config.pill_mode,
                                PillMode::SplitPills | PillMode::UnifiedPill
                            ) {
                                let now = Instant::now();
                                if state
                                    .last_eval_recheck
                                    .map(|t| now.duration_since(t) >= Duration::from_millis(2000))
                                    .unwrap_or(true)
                                {
                                    state.last_eval_recheck = Some(now);
                                    schedule_pill_rechecks(state.hidden_hwnd);
                                }
                            }
                        }
                    }
                }
                TIMER_EDGE_PEEK => {
                    if state.config.mac_dock_enabled {
                        if state.dock_timer_ms == 0 {
                            state.mac_dock.tick_idle_poll(&state.config);
                            update_dock_timer(state);
                        }
                    } else {
                        step_edge_peek(state);
                    }
                }
                TIMER_DOCK_SYNC_DEBOUNCE => {
                    let _ = KillTimer(Some(state.hidden_hwnd), TIMER_DOCK_SYNC_DEBOUNCE);
                    if state.config.mac_dock_enabled {
                        state.mac_dock.tick(&state.config);
                        update_dock_timer(state);
                    }
                }
                TIMER_DOCK_TICK => {
                    if state.config.mac_dock_enabled {
                        state.mac_dock.tick(&state.config);
                    }
                    update_dock_timer(state);
                }
                TIMER_ENSURE_TASKBAR => {
                    if state.hider.main_taskbar_exists() {
                        let _ = KillTimer(Some(state.hidden_hwnd), TIMER_ENSURE_TASKBAR);
                        evaluate_mode_and_apply(state, true);
                        write_status_snapshot("INIT", state);
                    } else {
                        let _ = SetTimer(Some(state.hidden_hwnd), TIMER_ENSURE_TASKBAR, 1500, None);
                    }
                }
                TIMER_PILL_SYNC => {
                    if !state.config.mac_dock_enabled
                        && (!state.hider.is_hidden || state.hider.peek_visible)
                    {
                        if state.styler.bar_geometry_changed() {
                            if !state.hider.is_hidden {
                                state.hider.clear_monitor_cache();
                            } else {
                                state.hider.refresh_geometry_cache();
                            }
                            state.styler.sync_taskbar(&state.config, true);
                            schedule_pill_rechecks(state.hidden_hwnd);
                        } else if matches!(
                            state.config.pill_mode,
                            PillMode::SplitPills | PillMode::UnifiedPill
                        ) {
                            state.styler.sync_taskbar(&state.config, false);
                        }
                    }
                }
                TIMER_PILL_RECHECK_1 => {
                    let _ = KillTimer(Some(state.hidden_hwnd), TIMER_PILL_RECHECK_1);
                    if !state.config.mac_dock_enabled
                        && (!state.hider.is_hidden || state.hider.peek_visible)
                    {
                        state.styler.request_pill_remeasure(&state.config, true);
                    }
                }
                TIMER_PILL_RECHECK_2 => {
                    let _ = KillTimer(Some(state.hidden_hwnd), TIMER_PILL_RECHECK_2);
                }
                TIMER_PILL_RECHECK_3 => {
                    let _ = KillTimer(Some(state.hidden_hwnd), TIMER_PILL_RECHECK_3);
                }
                TIMER_WIDGETS_TICK => {
                    state.maintenance_ticks = state.maintenance_ticks.wrapping_add(1);
                    if state.maintenance_ticks % 2 == 0 {
                        let has_max = has_maximized_or_fullscreen_window();
                        let game_fs =
                            state.config.game_mode_pause && is_fullscreen_game_or_app_active();
                        if game_fs != state.game_mode_active {
                            state.game_mode_active = game_fs;
                            diagnostics::record_event(
                                "GAME_MODE",
                                if game_fs { "enter_fullscreen_pause" } else { "exit_fullscreen_pause" },
                            );
                            if game_fs && state.hider.peek_visible {
                                let overlay_only = state.config.mode == TaskbarMode::HoverReveal;
                                state.hider.peek_hide(overlay_only);
                                state.styler.hide_overlays();
                            }
                            if state.config.mac_dock_enabled {
                                state.mac_dock.tick(&state.config);
                                update_dock_timer(state);
                            }
                        }
                        state
                            .widgets
                            .set_covered_by_maximized(has_max || state.game_mode_active);
                        if state.config.wallpaper_enabled {
                            let prev_act = state.wallpaper.is_active();
                            state.wallpaper.update_pause_for_cover(
                                (has_max && state.config.wallpaper_pause_maximized)
                                    || state.game_mode_active,
                            );
                            if prev_act != state.wallpaper.is_active() {
                                if state.wallpaper.is_active() {
                                    state.widgets.rebind_desktop_host();
                                }
                                write_status_snapshot("WALLPAPER", state);
                            }
                        }
                    }
                    state.widgets.tick();
                    if state.config.mac_dock_enabled {
                        sync_dock_snapshots(state);
                    }
                    if state.settings.is_visible() {
                        state.settings.tick_preview_ext(state.widgets.telemetry_extended());
                        sync_studio_live(state);
                    }
                    update_viz_timer(state);

                    if state.maintenance_ticks >= 75 {
                        state.maintenance_ticks = 0;
                        if state.config.wallpaper_enabled && !state.wallpaper.is_active() {
                            state.wallpaper.trim_gpu_memory();
                        }
                        // Only trim working set when animations & Studio are idle/covered so active
                        // rendering never suffers page-fault stutters every 30s.
                        if state.widgets.is_covered_trimmed()
                            || (!state.wallpaper.is_active()
                                && !state.widgets.has_active_interactive_fx()
                                && !state.widgets.wants_audio_viz_tick()
                                && !state.settings.is_visible())
                        {
                            config::trim_process_working_set();
                        }
                    }
                }
                TIMER_BOOT_WP => {
                    let _ = KillTimer(Some(state.hidden_hwnd), TIMER_BOOT_WP);
                    if !state.shutting_down && state.config.wallpaper_enabled {
                        state.wallpaper.attach();
                        let has_max = has_maximized_or_fullscreen_window();
                        state
                            .wallpaper
                            .apply_config_for_boot(&state.config, has_max);
                        if state.wallpaper.is_visible_on_desktop() {
                            state.widgets.rebind_desktop_host();
                        }
                        sync_studio_live(state);
                        write_status_snapshot("WALLPAPER", state);
                    }
                }
                TIMER_VIZ_TICK => {
                    if state.game_mode_active && !state.settings.is_visible() {
                        update_viz_timer(state);
                        return LRESULT(0);
                    }
                    let widgets_audio_on = state.widgets.wants_audio_viz_tick();
                    let widgets_fx = state.widgets.has_active_interactive_fx();
                    let audio_on = state.widgets.is_audio_active();
                    let tb_visible = !state.config.mac_dock_enabled
                        && (!state.hider.is_hidden || state.hider.peek_visible);
                    let tb_fx_on = state.config.tb_show_fx
                        && state.config.tb_viz_style != config::TaskbarVizStyle::Off;
                    let tb_needs_audio = tb_visible && audio_on && tb_fx_on;
                    let tb_ambient_on = tb_visible
                        && tb_fx_on
                        && state.config.tb_ambient_anim
                        && state.config.tb_viz_style.is_ambient_live();
                    let tb_active_decay = tb_visible
                        && tb_fx_on
                        && (state.styler.is_viz_hover_active()
                            || (!tb_ambient_on && state.styler.has_viz_decay()));
                    let dock_vu_active = state.config.mac_dock_enabled
                        && state.config.mac_dock_hide != MacDockHideMode::StealthHide
                        && state.config.w_tray.audio_style
                            == config::TrayAudioMeterStyle::LiveVuEqualizer;
                    let bands = if tb_needs_audio
                        || tb_ambient_on
                        || tb_active_decay
                        || (audio_on && (state.settings.is_visible() || dock_vu_active))
                    {
                        state.widgets.bands()
                    } else {
                        [0.0; 12]
                    };
                    if widgets_audio_on || widgets_fx {
                        state.widgets.tick_fast();
                    }
                    if dock_vu_active {
                        state
                            .mac_dock
                            .sync_audio_bands(if audio_on { &bands } else { &[0.0; 12] });
                        update_dock_timer(state);
                    }
                    if state.settings.is_visible() {
                        state.settings.sync_audio_bands_only(if audio_on { bands } else { [0.0; 12] });
                    }
                    if tb_ambient_on || tb_needs_audio || tb_active_decay {
                        state.styler.tick_visualizer(&state.config, &bands);
                    }
                    if state.settings.is_taskbar_themes_visible() {
                        state.settings.tick_taskbar_anim();
                    }
                    if state.settings.is_widgets_anim_visible() {
                        state.settings.tick_widgets_anim();
                    }
                    update_viz_timer(state);
                }
                TIMER_BOOT_SETTLE => {
                    // Self-heal Explorer/Progman host readiness during Windows bootup,
                    // short-circuiting immediately on tick 1 when Explorer & DWM are already settled.
                    if state.boot_settle_ticks == 0 || state.shutting_down {
                        let _ = KillTimer(Some(state.hidden_hwnd), TIMER_BOOT_SETTLE);
                    } else {
                        state.boot_settle_ticks -= 1;
                        let taskbar_ok = state.hider.main_taskbar_exists();
                        let need_styler_check = !state.config.mac_dock_enabled
                            && (!state.hider.is_hidden || state.hider.peek_visible)
                            && !mac_dock::is_any_tray_overflow_open();
                        let styler_ok = !need_styler_check || state.styler.is_settled(&state.config);
                        let widgets_ok =
                            !state.config.widgets_enabled || state.widgets.has_desktop_host();
                        let wp_ok = !state.config.wallpaper_enabled
                            || !state.wallpaper.is_visible_on_desktop()
                            || state.wallpaper.is_behind_defview();

                        if should_short_circuit_boot_settle(taskbar_ok, styler_ok, widgets_ok, wp_ok) {
                            write_status_snapshot("BOOT_SETTLED", state);
                            let _ = KillTimer(Some(state.hidden_hwnd), TIMER_BOOT_SETTLE);
                            config::trim_process_working_set();
                        } else {
                            if taskbar_ok && !styler_ok && !state.styler.is_tap_connecting() {
                                let need_dwm_only = state.styler.tap_connected
                                    && !state.styler.has_dwm_refreshed(&state.config);
                                state.styler.sync_taskbar(&state.config, need_dwm_only);
                            }
                            if !widgets_ok {
                                state.widgets.rebind_desktop_host();
                            }
                            if !wp_ok {
                                state.wallpaper.place_behind_icons();
                            }
                        }
                    }
                }
                TIMER_CONFIG_SAVE_DEBOUNCE => {
                    let _ = KillTimer(Some(state.hidden_hwnd), TIMER_CONFIG_SAVE_DEBOUNCE);
                    if state.config_dirty {
                        state.config_dirty = false;
                        save_config_async(&state.config);
                    }
                }
                _ => {}
            }
            LRESULT(0)
        }
        WM_DISPLAYCHANGE => {
            if state.shutting_down {
                return LRESULT(0);
            }
            if !state.hider.is_hidden {
                state.hider.clear_monitor_cache();
            } else {
                state.hider.refresh_geometry_cache();
            }
            state.styler.invalidate_cache();
            state.settings.refresh_win_prefs();
            state.wallpaper.handle_display_change();
            if state.config.any_match_wallpaper() {
                let _ = theme::ensure_wallpaper_palette_for_config(&state.config, true);
            }
            state.widgets.handle_display_change();
            state.widgets.rebind_desktop_host();
            state.widgets.render();
            evaluate_mode_and_apply(state, true);
            if !state.config.mac_dock_enabled {
                schedule_pill_rechecks(state.hidden_hwnd);
            }
            LRESULT(0)
        }
        WM_SETTINGCHANGE => {
            if state.shutting_down {
                return LRESULT(0);
            }
            const SPI_SETDESKWALLPAPER_WP: usize = 20;
            const SPI_SETWORKAREA_WP: usize = 47;
            let setting_name = if lparam.0 != 0 {
                PCWSTR(lparam.0 as *const u16).to_string().unwrap_or_default()
            } else {
                String::new()
            };
            let wp_or_theme_changed = wparam.0 == SPI_SETDESKWALLPAPER_WP
                || setting_name.eq_ignore_ascii_case("ImmersiveColorSet")
                || setting_name.eq_ignore_ascii_case("Wallpaper");
            let in_wa_quiet = state
                .self_workarea_broadcast_until
                .map(|t| Instant::now() < t)
                .unwrap_or(false);
            if in_wa_quiet
                && !wp_or_theme_changed
                && (wparam.0 == SPI_SETWORKAREA_WP
                    || setting_name.is_empty()
                    || setting_name.eq_ignore_ascii_case("TraySettings"))
            {
                return LRESULT(0);
            }
            if setting_name.eq_ignore_ascii_case("Environment")
                || setting_name.eq_ignore_ascii_case("intl")
                || setting_name.eq_ignore_ascii_case("Policy")
            {
                return LRESULT(0);
            }
            let is_relevant = wp_or_theme_changed
                || wparam.0 == SPI_SETWORKAREA_WP
                || setting_name.is_empty()
                || setting_name.eq_ignore_ascii_case("TraySettings")
                || setting_name.eq_ignore_ascii_case("UserPreferences");
            if !is_relevant {
                return LRESULT(0);
            }
            if !state.hider.is_hidden {
                state.hider.clear_monitor_cache();
            } else {
                state.hider.refresh_geometry_cache();
            }
            state.styler.invalidate_for_layout_change();
            state.settings.refresh_win_prefs();
            if wp_or_theme_changed && state.config.any_match_wallpaper() {
                let _ = theme::ensure_wallpaper_palette_for_config(&state.config, true);
                state.widgets.apply_config(&state.config);
                state.styler.invalidate_style_only();
                if state.config.mac_dock_enabled {
                    state.mac_dock.mark_dirty_paint();
                }
            }
            evaluate_mode_and_apply(state, false);
            if !state.config.mac_dock_enabled {
                schedule_pill_rechecks(state.hidden_hwnd);
            }
            LRESULT(0)
        }
        WM_APP_TRAY => {
            let mouse_msg = (lparam.0 & 0xFFFF) as u32;
            match mouse_msg {
                WM_LBUTTONUP | WM_LBUTTONDBLCLK => {
                    sync_studio_live(state);
                    state.settings.open(&state.config, state.hider.is_hidden);
                }
                WM_RBUTTONUP => {
                    show_tray_menu(state);
                }
                _ => {}
            }
            LRESULT(0)
        }
        WM_QUERYENDSESSION => LRESULT(1),
        WM_ENDSESSION => {
            if wparam.0 != 0 {
                if is_os_session_ending_lparam(lparam.0) {
                    state.os_session_ending = true;
                    fast_os_session_shutdown(state);
                    unsafe {
                        PostQuitMessage(0);
                    }
                } else {
                    shutdown_app(state);
                }
            }
            LRESULT(0)
        }
        WM_CLOSE => {
            shutdown_app(state);
            LRESULT(0)
        }
        WM_DESTROY => {
            restore_on_exit(state);
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

fn schedule_pill_rechecks(hwnd: HWND) {
    // Throttle background UIA pill remeasures to a single coalesced deferred frame
    // instead of firing three staggered subtree scans into Explorer.
    unsafe {
        let _ = KillTimer(Some(hwnd), TIMER_PILL_RECHECK_2);
        let _ = KillTimer(Some(hwnd), TIMER_PILL_RECHECK_3);
        let _ = SetTimer(Some(hwnd), TIMER_PILL_RECHECK_1, 220, None);
    }
}

fn should_short_circuit_boot_settle(
    taskbar_ok: bool,
    styler_ok: bool,
    widgets_ok: bool,
    wp_ok: bool,
) -> bool {
    taskbar_ok && styler_ok && widgets_ok && wp_ok
}

fn kill_pill_rechecks(hwnd: HWND) {
    unsafe {
        let _ = KillTimer(Some(hwnd), TIMER_PILL_RECHECK_1);
        let _ = KillTimer(Some(hwnd), TIMER_PILL_RECHECK_2);
        let _ = KillTimer(Some(hwnd), TIMER_PILL_RECHECK_3);
    }
}

fn sync_dock_snapshots(state: &mut AppState) {
    let cc = state.widgets.cc_snapshot();
    state.mac_dock.sync_tray_snapshot(&cc);
    if state.config.w_tray.audio_style == config::TrayAudioMeterStyle::LiveVuEqualizer {
        let bands = if state.widgets.is_audio_active() {
            state.widgets.bands()
        } else {
            [0.0; 12]
        };
        state.mac_dock.sync_audio_bands(&bands);
    }
    let (cpu, ram, _) = state.widgets.telemetry();
    let ms = state.widgets.media_snapshot();
    let track = if ms.is_playing && !ms.title.is_empty() {
        if ms.artist.is_empty() {
            ms.title.clone()
        } else {
            format!("{} - {}", ms.title, ms.artist)
        }
    } else {
        String::new()
    };
    state.mac_dock.sync_telemetry_snapshot(
        (cpu.round() as i32).clamp(0, 100) as u8,
        (ram.round() as i32).clamp(0, 100) as u8,
        &track,
    );
}

fn step_edge_peek(state: &mut AppState) {
    if state.preview_hold_ticks > 0 {
        state.preview_hold_ticks -= 1;
        if state.preview_hold_ticks == 0 && state.hider.peek_visible {
            let geoms = state.hider.all_edge_geometries();
            let mut pt = POINT::default();
            let hovering_bar = unsafe { GetCursorPos(&mut pt).is_ok() }
                && geoms
                    .iter()
                    .any(|&g| is_cursor_in_taskbar_zone(pt, g, 14));
            if !hovering_bar && !is_shell_flyout_active() {
                state.hider.peek_hide(true);
                state.styler.hide_overlays();
                update_viz_timer(state);
            }
        }
        return;
    }

    let can_peek = matches!(
        state.config.mode,
        TaskbarMode::SmartMaximize | TaskbarMode::HoverReveal
    );
    if !can_peek || !state.hider.is_hidden {
        state.peek_leave_ticks = 0;
        return;
    }

    // Fullscreen Game / Anti-Cheat Safe Mode: never pop up edge-peek taskbar over a game.
    if state.config.game_mode_pause
        && (state.game_mode_active || is_fullscreen_game_or_app_active())
    {
        if state.hider.peek_visible {
            let overlay_only = state.config.mode == TaskbarMode::HoverReveal;
            state.hider.peek_hide(overlay_only);
            state.styler.hide_overlays();
            update_viz_timer(state);
        }
        state.peek_leave_ticks = 0;
        return;
    }

    let mut pt = POINT::default();
    if unsafe { GetCursorPos(&mut pt).is_err() } {
        return;
    }
    let geoms = state.hider.all_edge_geometries();
    let overlay_only = state.config.mode == TaskbarMode::HoverReveal;

    if !state.hider.peek_visible {
        if geoms.iter().any(|&g| is_cursor_on_peek_edge(pt, g)) {
            state.hider.peek_show(overlay_only);
            state.styler.invalidate_style_only();
            state.styler.sync_taskbar(&state.config, true);
            schedule_pill_rechecks(state.hidden_hwnd);
            state.peek_leave_ticks = 0;
            update_viz_timer(state);
        }
    } else if geoms
        .iter()
        .any(|&g| is_cursor_in_taskbar_zone(pt, g, 18))
        || is_shell_flyout_active()
    {
        state.hider.keep_peek_topmost();
        state.styler.ensure_zorder_only(&state.config);
        state.peek_leave_ticks = 0;
    } else {
        state.peek_leave_ticks = state.peek_leave_ticks.saturating_add(1);
        if state.peek_leave_ticks >= 6 {
            state.hider.peek_hide(overlay_only);
            state.styler.hide_overlays();
            state.peek_leave_ticks = 0;
            update_viz_timer(state);
        }
    }
}

fn is_cursor_on_peek_edge(pt: POINT, geom: taskbar_hide::TaskbarEdgeGeometry) -> bool {
    let m = geom.mon_rect;
    match geom.edge {
        TaskbarEdge::Left => {
            pt.x >= m.left - 4
                && pt.x <= m.left + 8
                && pt.y >= m.top
                && pt.y <= m.bottom
        }
        TaskbarEdge::Right => {
            pt.x >= m.right - 8
                && pt.x <= m.right + 4
                && pt.y >= m.top
                && pt.y <= m.bottom
        }
        TaskbarEdge::Top => {
            pt.y >= m.top - 4
                && pt.y <= m.top + 8
                && pt.x >= m.left
                && pt.x <= m.right
        }
        TaskbarEdge::Bottom => {
            pt.y >= m.bottom - 8
                && pt.y <= m.bottom + 4
                && pt.x >= m.left
                && pt.x <= m.right
        }
    }
}

fn is_cursor_in_taskbar_zone(
    pt: POINT,
    geom: taskbar_hide::TaskbarEdgeGeometry,
    slack: i32,
) -> bool {
    let b = geom.bar_rect;
    let m = geom.mon_rect;
    let bw = (b.right - b.left).max(48);
    let bh = (b.bottom - b.top).max(48);
    let in_mon_x = pt.x >= m.left - slack && pt.x <= m.right + slack;
    let in_mon_y = pt.y >= m.top - slack && pt.y <= m.bottom + slack;
    match geom.edge {
        TaskbarEdge::Left => in_mon_y && pt.x >= m.left - 8 && pt.x <= m.left + bw + slack,
        TaskbarEdge::Right => in_mon_y && pt.x >= m.right - bw - slack && pt.x <= m.right + 8,
        TaskbarEdge::Top => in_mon_x && pt.y >= m.top - 8 && pt.y <= m.top + bh + slack,
        TaskbarEdge::Bottom => in_mon_x && pt.y >= m.bottom - bh - slack && pt.y <= m.bottom + 8,
    }
}

fn sync_clock_24h_if_changed(state: &mut AppState) {
    let cur = state.config.w_clock.use_24h.unwrap_or(state.config.widgets_24h);
    if cur != state.last_applied_24h {
        state.last_applied_24h = cur;
        state.config.set_clock_24h(cur);
        config::apply_win_clock_24h(cur);
        if state.config.mac_dock_enabled {
            state.mac_dock.force_clock_refresh(&state.config);
        }
    }
}

fn schedule_config_save(state: &mut AppState) {
    state.config_dirty = true;
    if !state.hidden_hwnd.0.is_null() {
        unsafe {
            let _ = SetTimer(Some(state.hidden_hwnd), TIMER_CONFIG_SAVE_DEBOUNCE, 250, None);
        }
    } else {
        state.config_dirty = false;
        save_config_async(&state.config);
    }
}

fn toggle_taskbar_manual(state: &mut AppState) {
    state.preview_hold_ticks = 0;
    if state.config.mac_dock_enabled {
        state.config.mac_dock_hide = if state.config.mac_dock_hide == MacDockHideMode::StealthHide {
            MacDockHideMode::AlwaysVisible
        } else {
            MacDockHideMode::StealthHide
        };
        state.config.sync_hide_mode_from_dock();
    } else {
        state.config.mode = if state.hider.is_hidden {
            TaskbarMode::AlwaysVisible
        } else {
            TaskbarMode::StealthHide
        };
        state.config.sync_dock_hide_from_mode();
    }
    schedule_config_save(state);
    evaluate_mode_and_apply(state, true);
    state.settings.sync_from_config(&state.config);
}

fn handle_settings_command(state: &mut AppState, cmd: SettingsCommand, prev_pill: PillMode) {
    match cmd {
        SettingsCommand::None => {}
        SettingsCommand::ModeChanged(mode) => {
            state.preview_hold_ticks = 0;
            state.config.mode = mode;
            state.config.sync_dock_hide_from_mode();
            schedule_config_save(state);
            evaluate_mode_and_apply(state, true);
            write_status_snapshot("MODE", state);
        }
        SettingsCommand::ToggleTaskbarNow => {
            toggle_taskbar_manual(state);
        }
        SettingsCommand::StylePreview => {
            if state.config.mac_dock_enabled {
                state.mac_dock.mark_dirty_paint();
                state.mac_dock.tick(&state.config);
            } else {
                if state.hider.is_hidden && state.config.mode != TaskbarMode::StealthHide {
                    state.hider.peek_show(true);
                    state.preview_hold_ticks = 30;
                }
                if !state.hider.is_hidden || state.hider.peek_visible {
                    state.styler.sync_style_fast(&state.config);
                }
            }
            return;
        }
        SettingsCommand::MacDockPreview => {
            if state.config.mac_dock_enabled {
                state.mac_dock.mark_dirty_paint();
                state.mac_dock.tick(&state.config);
            }
            return;
        }
        SettingsCommand::StyleUpdated => {
            schedule_config_save(state);
            if state.config.tb_match_wallpaper {
                if !state.wallpaper.resample_live_palette() {
                    let _ = theme::ensure_wallpaper_palette_for_config(&state.config, false);
                }
            }
            if !state.config.tb_show_fx
                || state.config.tb_viz_style == config::TaskbarVizStyle::Off
                || (!state.config.tb_viz_style.is_ambient_live() && !state.widgets.is_audio_active())
            {
                state.styler.clear_viz_energy();
            }
            let switched_dock = state.config.mac_dock_enabled != state.mac_dock.is_active();
            if switched_dock {
                state.mode_switch_settle_until =
                    Some(Instant::now() + Duration::from_millis(550));
                if !state.config.mac_dock_enabled {
                    state.styler.invalidate_for_layout_change();
                }
                evaluate_mode_and_apply(state, true);
                if state.hider.is_hidden
                    && state.config.mode != TaskbarMode::StealthHide
                    && !state.config.mac_dock_enabled
                {
                    state.hider.peek_show(true);
                    state.preview_hold_ticks = 30;
                    state.styler.sync_taskbar(&state.config, true);
                    schedule_pill_rechecks(state.hidden_hwnd);
                }
            } else if state.config.mac_dock_enabled {
                // Full dock sync so switching between Dock themes (e.g., 3-Island -> HUD Deck -> Full Edge)
                // immediately rebuilds slots, updates layout metrics, and reconciles work-area reserves.
                evaluate_mode_and_apply(state, true);
            } else {
                if state.hider.is_hidden && state.config.mode != TaskbarMode::StealthHide {
                    state.hider.peek_show(true);
                    state.preview_hold_ticks = 30;
                }
                let shape_changed = state.config.pill_mode != prev_pill;
                if shape_changed {
                    state.styler.invalidate_cache();
                    if !state.hider.is_hidden || state.hider.peek_visible {
                        state.styler.sync_taskbar(&state.config, true);
                        schedule_pill_rechecks(state.hidden_hwnd);
                    }
                } else {
                    state.styler.invalidate_style_only();
                    if !state.hider.is_hidden || state.hider.peek_visible {
                        state.styler.sync_style_fast(&state.config);
                    }
                }
            }
            if state.config.show_apps
                && !state.config.apps_match_wallpaper
                && !state.config.apps_use_custom_color
            {
                state.widgets.apply_config(&state.config);
            }
            sync_studio_live(state);
            update_viz_timer(state);
            update_dock_timer(state);
            update_tray_tooltip(state.hidden_hwnd, state.hider.is_hidden, &state.config);
            write_status_snapshot("STYLE", state);
        }
        SettingsCommand::LayoutPrefUpdated => {
            if state.hider.is_hidden && state.config.mode != TaskbarMode::StealthHide {
                state.hider.peek_show(true);
                state.preview_hold_ticks = 30;
            }
            state.styler.invalidate_for_layout_change();
            if !state.hider.is_hidden || state.hider.peek_visible {
                state.styler.sync_taskbar(&state.config, true);
                schedule_pill_rechecks(state.hidden_hwnd);
            }
            write_status_snapshot("LAYOUT", state);
        }
        SettingsCommand::WidgetsConfigUpdated => {
            sync_clock_24h_if_changed(state);
            schedule_config_save(state);
            state.widgets.apply_config(&state.config);
            if state.config.mac_dock_enabled {
                state.mac_dock.force_clock_refresh(&state.config);
            }
            update_viz_timer(state);
            sync_studio_live(state);
            write_status_snapshot("WIDGETS", state);
        }
        SettingsCommand::WallpaperConfigUpdated => {
            unsafe {
                let _ = KillTimer(Some(state.hidden_hwnd), TIMER_BOOT_WP);
            }
            let prev_wp_enabled = state.wallpaper.is_enabled();
            let prev_wp_path = state.wallpaper.active_path().to_string();
            schedule_config_save(state);
            state.wallpaper.attach();
            state
                .wallpaper
                .set_chameleon_sampling(state.config.any_match_wallpaper());
            state.wallpaper.apply_config_ex(&state.config, true);
            if state.config.wallpaper_enabled {
                let has_max = has_maximized_or_fullscreen_window();
                state.wallpaper.update_pause_for_cover(
                    (has_max && state.config.wallpaper_pause_maximized) || state.game_mode_active,
                );
            }
            let wp_media_changed = prev_wp_enabled != state.wallpaper.is_enabled()
                || !prev_wp_path.eq_ignore_ascii_case(&state.config.wallpaper_path);
            if wp_media_changed {
                if state.config.any_match_wallpaper() && !state.wallpaper.is_static_loading() {
                    if !state.wallpaper.resample_live_palette() {
                        let _ = theme::ensure_wallpaper_palette_for_config(&state.config, true);
                    }
                    state.widgets.apply_config(&state.config);
                }
                if !state.hider.is_hidden || state.hider.peek_visible {
                    state.styler.invalidate_style_only();
                    state.styler.sync_style_fast(&state.config);
                }
                state.widgets.rebind_desktop_host();
            }
            sync_studio_live(state);
            write_status_snapshot("WALLPAPER", state);
        }
        SettingsCommand::ResampleWallpaper => {
            if !state.wallpaper.resample_live_palette() {
                let _ = theme::ensure_wallpaper_palette_for_config(&state.settings.draft_cfg, true);
            }
            if state.config.any_match_wallpaper() {
                state.widgets.apply_config(&state.config);
            }
            if (!state.hider.is_hidden || state.hider.peek_visible) && state.config.tb_match_wallpaper {
                state.styler.invalidate_style_only();
                state.styler.sync_style_fast(&state.config);
            }
            sync_studio_live(state);
        }
        SettingsCommand::StopExternalLively => {
            video_wallpaper::stop_external_lively();
            if state.config.wallpaper_enabled {
                state.wallpaper.apply_config(&state.config);
                state.widgets.rebind_desktop_host();
            }
            sync_studio_live(state);
        }
        SettingsCommand::MacDockUpdated => {
            sync_clock_24h_if_changed(state);
            schedule_config_save(state);
            evaluate_mode_and_apply(state, true);
            state.widgets.apply_config(&state.config);
            sync_studio_live(state);
            write_status_snapshot("MAC_DOCK", state);
        }
        SettingsCommand::SuiteApplied => {
            schedule_config_save(state);
            if state.config.any_match_wallpaper() {
                if !state.wallpaper.resample_live_palette() {
                    let _ = theme::ensure_wallpaper_palette_for_config(&state.config, true);
                }
            }
            state.styler.invalidate_cache();
            evaluate_mode_and_apply(state, true);
            if state.config.mac_dock_enabled {
                state.mac_dock.mark_dirty_paint();
                state.mac_dock.tick(&state.config);
            }
            state.widgets.apply_config(&state.config);
            sync_studio_live(state);
            update_viz_timer(state);
            update_tray_tooltip(state.hidden_hwnd, state.hider.is_hidden, &state.config);
            diagnostics::record_event(
                "SUITE",
                &format!(
                    "applied={} dock={}",
                    state.config.active_suite, state.config.mac_dock_enabled
                ),
            );
            write_status_snapshot("SUITE", state);
        }
        SettingsCommand::ConfigImported => {
            unsafe {
                let _ = KillTimer(Some(state.hidden_hwnd), TIMER_BOOT_WP);
            }
            schedule_config_save(state);
            state
                .wallpaper
                .set_chameleon_sampling(state.config.any_match_wallpaper());
            state.wallpaper.apply_config(&state.config);
            if state.config.any_match_wallpaper() {
                if !state.wallpaper.resample_live_palette() {
                    let _ = theme::ensure_wallpaper_palette_for_config(&state.config, true);
                }
            }
            state.styler.invalidate_cache();
            evaluate_mode_and_apply(state, true);
            if state.config.mac_dock_enabled {
                state.mac_dock.mark_dirty_paint();
                state.mac_dock.tick(&state.config);
            }
            state.widgets.apply_config(&state.config);
            state.widgets.rebind_desktop_host();
            sync_studio_live(state);
            update_viz_timer(state);
            update_dock_timer(state);
            update_tray_tooltip(state.hidden_hwnd, state.hider.is_hidden, &state.config);
            diagnostics::record_event(
                "CONFIG_IMPORT",
                &format!(
                    "suite={} theme={} wp={}",
                    state.config.active_suite,
                    state.config.tb_theme.as_str(),
                    state.config.wallpaper_enabled
                ),
            );
            write_status_snapshot("IMPORT", state);
        }
        SettingsCommand::GameModeUpdated => {
            schedule_config_save(state);
            diagnostics::record_event(
                "GAME_MODE",
                if state.config.game_mode_pause { "enabled" } else { "disabled" },
            );
            evaluate_mode_and_apply(state, false);
        }
        SettingsCommand::ExportDiagnostics => {
            if diagnostics::export_support_bundle(&state.config, state.game_mode_active, true)
                .is_ok()
            {
                state.settings.mark_diagnostics_exported();
            }
        }
        SettingsCommand::OnboardingFinished => {
            schedule_config_save(state);
            diagnostics::record_event(
                "ONBOARDING",
                &format!("completed suite={}", state.config.active_suite),
            );
        }
        SettingsCommand::RefreshWeatherLocation => {
            state.widgets.trigger_weather_refresh();
            diagnostics::record_event("WEATHER", "manual_auto_locate_refresh");
        }
        SettingsCommand::QuitApp => {
            shutdown_app(state);
            return;
        }
    }
    if state.settings.is_visible() && !state.settings.hwnd().0.is_null() {
        unsafe {
            let _ = SetWindowPos(
                state.settings.hwnd(),
                Some(HWND_TOP),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            );
        }
    }
    state
        .wallpaper
        .set_chameleon_sampling(state.config.any_match_wallpaper());
    update_viz_timer(state);
    update_dock_timer(state);
}

fn handle_dock_action(state: &mut AppState, act: MacDockAction) {
    match act {
        MacDockAction::None => {}
        MacDockAction::OpenSettingsDock => {
            sync_studio_live(state);
            state
                .settings
                .open_taskbar_customize(&state.config, state.hider.is_hidden);
        }
        MacDockAction::ConfigUpdated => {
            sync_clock_24h_if_changed(state);
            evaluate_mode_and_apply(state, true);
            state.widgets.apply_config(&state.config);
            state.settings.sync_from_config(&state.config);
            sync_studio_live(state);
        }
        MacDockAction::TraySetVolume(v) => {
            state.widgets.send_media_action(CcAction::SetVolume(v));
        }
        MacDockAction::TrayToggleMute => {
            state.widgets.send_media_action(CcAction::ToggleMute);
        }
    }
    update_dock_timer(state);
}

fn update_dock_timer(state: &mut AppState) {
    let want_ms = if state.config.mac_dock_enabled {
        state.mac_dock.desired_timer_ms(&state.config)
    } else {
        0
    };
    if want_ms != state.dock_timer_ms {
        state.dock_timer_ms = want_ms;
        unsafe {
            if want_ms == 0 {
                let _ = KillTimer(Some(state.hidden_hwnd), TIMER_DOCK_TICK);
            } else {
                let _ = SetTimer(Some(state.hidden_hwnd), TIMER_DOCK_TICK, want_ms, None);
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn compute_viz_timer_ms(
    anim_fps: u32,
    game_mode_paused: bool,
    widgets_60fps: bool,
    tb_ambient: bool,
    tb_audio: bool,
    tb_decay: bool,
    widgets_audio: bool,
    settings_fast: bool,
    widgets_ambient_idle: bool,
    widgets_fx_30fps: bool,
    widgets_fx: bool,
    settings_anim: bool,
) -> u32 {
    if game_mode_paused {
        return 0;
    }
    let active_ms = AppConfig::anim_frame_ms_for(anim_fps);
    if widgets_60fps
        || tb_audio
        || tb_decay
        || widgets_audio
        || widgets_fx_30fps
        || widgets_fx
        || tb_ambient
        || widgets_ambient_idle
    {
        active_ms
    } else if settings_fast {
        active_ms.max(16)
    } else if settings_anim {
        (active_ms * 2).clamp(45, 90)
    } else {
        0
    }
}

fn update_viz_timer(state: &mut AppState) {
    let settings_vis = state.settings.is_visible();
    let settings_anim =
        state.settings.is_taskbar_themes_visible() || state.settings.is_widgets_anim_visible();
    let settings_fast = settings_anim && state.settings.wants_fast_30fps_anim();
    let tb_visible = !state.config.mac_dock_enabled
        && (!state.hider.is_hidden || state.hider.peek_visible);
    let dock_vu_enabled = state.config.mac_dock_enabled
        && state.config.mac_dock_hide != MacDockHideMode::StealthHide
        && state.config.w_tray.audio_style == config::TrayAudioMeterStyle::LiveVuEqualizer;
    let tb_fx_on = state.config.tb_show_fx
        && state.config.tb_viz_style != config::TaskbarVizStyle::Off;
    let viz_needed = (!state.game_mode_active
        && ((tb_visible && tb_fx_on)
            || dock_vu_enabled
            || state.widgets.wants_wasapi_loopback()))
        || settings_vis;
    state.widgets.set_viz_needed(viz_needed);
    state.widgets.set_anim_fps(state.config.anim_fps);

    let tb_ambient = tb_visible
        && tb_fx_on
        && state.config.tb_ambient_anim
        && state.config.tb_viz_style.is_ambient_live();
    let audio_on = state.widgets.is_audio_active();
    let tb_audio = (tb_visible && audio_on && tb_fx_on)
        || (dock_vu_enabled && audio_on);
    let tb_decay = tb_visible
        && tb_fx_on
        && (state.styler.is_viz_hover_active()
            || (!tb_ambient && state.styler.has_viz_decay()));
    let widgets_audio = state.widgets.wants_audio_viz_tick();
    let widgets_60fps = state.widgets.wants_fast_60fps();
    let widgets_fx = state.widgets.has_active_interactive_fx();
    let widgets_ambient_idle = widgets_fx && state.widgets.ambient_idle_only();
    let widgets_fx_30fps = widgets_fx && state.widgets.wants_fast_30fps();

    let want_ms = compute_viz_timer_ms(
        state.config.anim_fps,
        state.game_mode_active && !settings_vis,
        widgets_60fps,
        tb_ambient,
        tb_audio,
        tb_decay,
        widgets_audio,
        settings_fast,
        widgets_ambient_idle,
        widgets_fx_30fps,
        widgets_fx,
        settings_anim,
    );
    if want_ms != state.viz_timer_ms {
        let was_off = state.viz_timer_ms == 0;
        state.viz_timer_ms = want_ms;
        if want_ms > 0 && was_off {
            state.widgets.reset_fx_clock();
        }
        unsafe {
            if want_ms == 0 {
                let _ = KillTimer(Some(state.hidden_hwnd), TIMER_VIZ_TICK);
            } else {
                let _ = SetTimer(Some(state.hidden_hwnd), TIMER_VIZ_TICK, want_ms, None);
            }
        }
    }
}

fn sync_studio_live(state: &mut AppState) {
    if !state.settings.is_visible() && !state.settings.onboarding_active {
        return;
    }
    let (w_city, w_temp, w_cond) = state.widgets.local_weather_summary();
    state.settings.sync_weather_preview(w_city, w_temp, w_cond);
    if !state.settings.is_visible() {
        return;
    }
    let lively = if state.settings.is_wallpaper_tab_visible() {
        video_wallpaper::is_lively_running()
    } else {
        false
    };
    state.settings.sync_tap_fallback_status(state.styler.is_tap_fallback_active(&state.config));
    state.settings.sync_live_state(
        state.widgets.media_snapshot(),
        state.widgets.cc_snapshot(),
        state.widgets.bands(),
        state.wallpaper.status_text(),
        lively,
    );
}

fn evaluate_mode_and_apply(state: &mut AppState, force_style: bool) {
    let has_max = has_maximized_or_fullscreen_window();
    state.game_mode_active = state.config.game_mode_pause && is_fullscreen_game_or_app_active();
    state
        .widgets
        .set_covered_by_maximized(has_max || state.game_mode_active);
    let prev_wp = state.wallpaper.is_active();
    state.wallpaper.update_pause_for_cover(
        (has_max && state.config.wallpaper_pause_maximized) || state.game_mode_active,
    );
    if prev_wp != state.wallpaper.is_active() {
        sync_studio_live(state);
        write_status_snapshot("WALLPAPER", state);
    }

    if state.config.mac_dock_enabled {
        unsafe {
            let _ = KillTimer(Some(state.hidden_hwnd), TIMER_SETTLE_HIDE);
        }
        kill_pill_rechecks(state.hidden_hwnd);
        let dock_reserves = state.mac_dock.desired_work_area_reserves(&state.config);
        let expand_wa = dock_reserves.is_empty();
        let was_visible_tb = !state.hider.is_hidden;
        let mut wa_mutated = false;
        state.styler.hide_overlays();
        if was_visible_tb {
            state.self_workarea_broadcast_until =
                Some(Instant::now() + Duration::from_millis(1200));
            state.mode_switch_settle_until =
                Some(Instant::now() + Duration::from_millis(550));
            wa_mutated |= state.hider.hide(expand_wa);
            unsafe {
                let _ = SetTimer(Some(state.hidden_hwnd), TIMER_SETTLE_HIDE, 140, None);
            }
        } else if state.hider.peek_visible {
            state.hider.peek_hide(false);
        } else if force_style && !mac_dock::is_tray_overflow_active() {
            wa_mutated |= state.hider.settle_hidden(expand_wa);
        }
        sync_dock_snapshots(state);
        state.mac_dock.sync(&state.config);
        // Avoid duplicate SPI_SETWORKAREA pass when hide(true) already expanded full work area.
        if !expand_wa || !was_visible_tb {
            wa_mutated |= TaskbarHider::apply_dock_work_area(&dock_reserves);
        }
        if wa_mutated {
            state.self_workarea_broadcast_until =
                Some(Instant::now() + Duration::from_millis(1200));
        }
        if state.settings.is_visible() && !state.settings.hwnd().0.is_null() {
            unsafe {
                let _ = SetWindowPos(
                    state.settings.hwnd(),
                    Some(HWND_TOP),
                    0,
                    0,
                    0,
                    0,
                    SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
                );
            }
        }
        write_status_snapshot("MAC_DOCK", state);
        state
            .settings
            .sync_status_only(&state.config, state.hider.is_hidden);
        update_viz_timer(state);
        update_dock_timer(state);
        update_tray_tooltip(state.hidden_hwnd, state.hider.is_hidden, &state.config);
        return;
    }

    let should_hide = match state.config.mode {
        TaskbarMode::StealthHide | TaskbarMode::HoverReveal => true,
        TaskbarMode::AlwaysVisible => false,
        TaskbarMode::SmartMaximize => has_max,
    };

    let restore_wa_on_show = !get_native_autohide();
    let switched_from_dock = state.mac_dock.is_active();
    if switched_from_dock {
        state.self_workarea_broadcast_until =
            Some(Instant::now() + Duration::from_millis(1200));
        state.mode_switch_settle_until =
            Some(Instant::now() + Duration::from_millis(550));
        state.mac_dock.disable();
        // Avoid double WorkArea bounce + double refit_maximized_windows() when hider.show()
        // is immediately going to restore native taskbar WorkArea below.
        if should_hide || !restore_wa_on_show {
            let _ = TaskbarHider::apply_dock_work_area(&[]);
        }
    }

    if should_hide {
        if !state.hider.is_hidden {
            unsafe {
                let _ = KillTimer(Some(state.hidden_hwnd), TIMER_SETTLE_HIDE);
            }
            state.styler.hide_overlays();
            if state.hider.hide(true) {
                state.self_workarea_broadcast_until =
                    Some(Instant::now() + Duration::from_millis(1200));
            }
            unsafe {
                let _ = SetTimer(Some(state.hidden_hwnd), TIMER_SETTLE_HIDE, 140, None);
            }
            write_status_snapshot("HIDE", state);
        } else if state.config.mode == TaskbarMode::StealthHide && state.hider.peek_visible {
            state.hider.peek_hide(false);
            state.styler.hide_overlays();
            write_status_snapshot("STEALTH", state);
        } else if state.hider.peek_visible {
            state.styler.sync_taskbar(&state.config, force_style);
        } else {
            state.styler.hide_overlays();
            if force_style && state.hider.settle_hidden(true) {
                state.self_workarea_broadcast_until =
                    Some(Instant::now() + Duration::from_millis(1200));
            }
        }
    } else {
        let was_hidden = state.hider.is_hidden || !TaskbarHider::is_main_taskbar_visible();
        if was_hidden {
            unsafe {
                let _ = KillTimer(Some(state.hidden_hwnd), TIMER_SETTLE_HIDE);
            }
            state.mode_switch_settle_until =
                Some(Instant::now() + Duration::from_millis(550));
            if restore_wa_on_show {
                state.self_workarea_broadcast_until =
                    Some(Instant::now() + Duration::from_millis(1200));
            }
            let _ = state.hider.show(restore_wa_on_show);
            state.styler.invalidate_for_layout_change();
        }
        state.styler.sync_taskbar(&state.config, force_style);
        if was_hidden {
            schedule_pill_rechecks(state.hidden_hwnd);
        }
        write_status_snapshot("SHOW", state);
    }

    state
        .settings
        .sync_status_only(&state.config, state.hider.is_hidden);
    update_viz_timer(state);
    update_dock_timer(state);
    update_tray_tooltip(state.hidden_hwnd, state.hider.is_hidden, &state.config);
}

fn show_tray_menu(state: &mut AppState) {
    unsafe {
        let Ok(hmenu) = CreatePopupMenu() else {
            return;
        };
        let toggle_lbl = if state.hider.is_hidden {
            w!("Show Windows Taskbar Now (Ctrl+Alt+T)")
        } else {
            w!("Hide Windows Taskbar Now (Ctrl+Alt+T)")
        };
        let _ = AppendMenuW(hmenu, MF_STRING, IDM_TRAY_TOGGLE_NOW, toggle_lbl);
        let _ = AppendMenuW(hmenu, MF_SEPARATOR, 0, PCWSTR::null());

        for (id, mode) in [
            (IDM_TRAY_MODE_SMART, TaskbarMode::SmartMaximize),
            (IDM_TRAY_MODE_HOVER, TaskbarMode::HoverReveal),
            (IDM_TRAY_MODE_STEALTH, TaskbarMode::StealthHide),
            (IDM_TRAY_MODE_ALWAYS, TaskbarMode::AlwaysVisible),
        ] {
            add_check_item(hmenu, id, mode.label(), state.config.mode == mode);
        }

        let _ = AppendMenuW(hmenu, MF_SEPARATOR, 0, PCWSTR::null());
        add_check_item(
            hmenu,
            IDM_TRAY_PILL_SPLIT,
            "Shape: Crystal Split Pills",
            state.config.pill_mode == PillMode::SplitPills,
        );
        add_check_item(
            hmenu,
            IDM_TRAY_PILL_UNIFIED,
            "Shape: Crystal Unified Capsule",
            state.config.pill_mode == PillMode::UnifiedPill,
        );
        add_check_item(
            hmenu,
            IDM_TRAY_PILL_FULL,
            "Shape: Full Crystal Bar",
            state.config.pill_mode == PillMode::FullBar,
        );
        add_check_item(
            hmenu,
            IDM_TRAY_PILL_NATIVE,
            "Shape: Windows Default",
            state.config.pill_mode == PillMode::NativeDefault,
        );
        add_check_item(
            hmenu,
            IDM_TRAY_AMBER_RIM,
            "Amber Crystal Glow Rim (#F5A642)",
            state.config.amber_border,
        );
        add_check_item(
            hmenu,
            IDM_TRAY_DESKTOP_WIDGETS,
            "Desktop Widgets Engine",
            state.widgets.is_visible(),
        );
        add_check_item(
            hmenu,
            IDM_TRAY_SHOW_MUSIC,
            "  NowPlaying Music Bar",
            state.config.show_music,
        );
        add_check_item(
            hmenu,
            IDM_TRAY_SHOW_CC,
            "  Quick Control Center",
            state.config.show_cc,
        );
        add_check_item(
            hmenu,
            IDM_TRAY_WIDGETS_LOCK,
            "Lock Desktop Widget Positions",
            state.config.widgets_locked,
        );
        add_check_item(
            hmenu,
            IDM_TRAY_VIDEO_WALLPAPER,
            "Video Wallpaper Engine",
            state.config.wallpaper_enabled,
        );
        add_check_item(
            hmenu,
            IDM_TRAY_GAME_MODE,
            "Fullscreen Game Safe Mode (Auto-Pause)",
            state.config.game_mode_pause,
        );

        let _ = AppendMenuW(hmenu, MF_SEPARATOR, 0, PCWSTR::null());
        let _ = AppendMenuW(
            hmenu,
            MF_STRING,
            IDM_TRAY_SETTINGS,
            w!("Open AutumnDeck Studio..."),
        );
        let _ = AppendMenuW(
            hmenu,
            MF_STRING,
            IDM_TRAY_APP_SETTINGS,
            w!("App Settings (Startup && Backup)..."),
        );
        let _ = AppendMenuW(
            hmenu,
            MF_STRING,
            IDM_TRAY_ONBOARDING,
            w!("Run Welcome Setup Wizard..."),
        );
        let _ = AppendMenuW(
            hmenu,
            MF_STRING,
            IDM_TRAY_EXPORT_LOGS,
            w!("Export Support Bundle (Diagnostics)..."),
        );
        let _ = AppendMenuW(hmenu, MF_SEPARATOR, 0, PCWSTR::null());
        let _ = AppendMenuW(
            hmenu,
            MF_STRING,
            IDM_TRAY_QUIT,
            w!("Restore Default Taskbar && Exit"),
        );

        let mut pt = POINT::default();
        let _ = GetCursorPos(&mut pt);
        let _ = SetForegroundWindow(state.hidden_hwnd);
        let picked = TrackPopupMenu(
            hmenu,
            TPM_LEFTALIGN | TPM_BOTTOMALIGN | TPM_RIGHTBUTTON | TPM_RETURNCMD,
            pt.x,
            pt.y,
            Some(0),
            state.hidden_hwnd,
            None,
        );
        let _ = DestroyMenu(hmenu);

        let prev_pill = state.config.pill_mode;
        match picked.0 as usize {
            IDM_TRAY_TOGGLE_NOW => toggle_taskbar_manual(state),
            IDM_TRAY_MODE_SMART => handle_settings_command(
                state,
                SettingsCommand::ModeChanged(TaskbarMode::SmartMaximize),
                prev_pill,
            ),
            IDM_TRAY_MODE_HOVER => handle_settings_command(
                state,
                SettingsCommand::ModeChanged(TaskbarMode::HoverReveal),
                prev_pill,
            ),
            IDM_TRAY_MODE_STEALTH => handle_settings_command(
                state,
                SettingsCommand::ModeChanged(TaskbarMode::StealthHide),
                prev_pill,
            ),
            IDM_TRAY_MODE_ALWAYS => handle_settings_command(
                state,
                SettingsCommand::ModeChanged(TaskbarMode::AlwaysVisible),
                prev_pill,
            ),
            IDM_TRAY_PILL_SPLIT => {
                state.config.pill_mode = PillMode::SplitPills;
                handle_settings_command(state, SettingsCommand::StyleUpdated, prev_pill);
            }
            IDM_TRAY_PILL_UNIFIED => {
                state.config.pill_mode = PillMode::UnifiedPill;
                handle_settings_command(state, SettingsCommand::StyleUpdated, prev_pill);
            }
            IDM_TRAY_PILL_FULL => {
                state.config.pill_mode = PillMode::FullBar;
                handle_settings_command(state, SettingsCommand::StyleUpdated, prev_pill);
            }
            IDM_TRAY_PILL_NATIVE => {
                state.config.pill_mode = PillMode::NativeDefault;
                handle_settings_command(state, SettingsCommand::StyleUpdated, prev_pill);
            }
            IDM_TRAY_AMBER_RIM => {
                state.config.amber_border = !state.config.amber_border;
                handle_settings_command(state, SettingsCommand::StyleUpdated, prev_pill);
            }
            IDM_TRAY_DESKTOP_WIDGETS => {
                let vis = !state.widgets.is_visible();
                state.config.widgets_enabled = vis;
                schedule_config_save(state);
                state.widgets.apply_config(&state.config);
                state.settings.sync_from_config(&state.config);
            }
            IDM_TRAY_SHOW_MUSIC => {
                state.config.show_music = !state.config.show_music;
                schedule_config_save(state);
                state.widgets.apply_config(&state.config);
                state.settings.sync_from_config(&state.config);
            }
            IDM_TRAY_SHOW_CC => {
                state.config.show_cc = !state.config.show_cc;
                schedule_config_save(state);
                state.widgets.apply_config(&state.config);
                state.settings.sync_from_config(&state.config);
            }
            IDM_TRAY_WIDGETS_LOCK => {
                state.config.widgets_locked = !state.config.widgets_locked;
                schedule_config_save(state);
                state.widgets.apply_config(&state.config);
                state.settings.sync_from_config(&state.config);
            }
            IDM_TRAY_VIDEO_WALLPAPER => {
                state.config.wallpaper_enabled = !state.config.wallpaper_enabled;
                if state.config.wallpaper_enabled && state.config.wallpaper_path.is_empty() {
                    state.config.wallpaper_path = config::detect_default_wallpaper();
                }
                handle_settings_command(state, SettingsCommand::WallpaperConfigUpdated, prev_pill);
                state.settings.sync_from_config(&state.config);
            }
            IDM_TRAY_GAME_MODE => {
                state.config.game_mode_pause = !state.config.game_mode_pause;
                handle_settings_command(state, SettingsCommand::GameModeUpdated, prev_pill);
                state.settings.sync_from_config(&state.config);
            }
            IDM_TRAY_SETTINGS => {
                sync_studio_live(state);
                state.settings.open(&state.config, state.hider.is_hidden);
            }
            IDM_TRAY_APP_SETTINGS => {
                sync_studio_live(state);
                state.settings.open_tab(
                    &state.config,
                    state.hider.is_hidden,
                    StudioTab::Settings,
                    WidgetPage::Gallery,
                );
            }
            IDM_TRAY_ONBOARDING => {
                sync_studio_live(state);
                state
                    .settings
                    .open_onboarding(&state.config, state.hider.is_hidden);
            }
            IDM_TRAY_EXPORT_LOGS => {
                handle_settings_command(state, SettingsCommand::ExportDiagnostics, prev_pill);
            }
            IDM_TRAY_QUIT => shutdown_app(state),
            _ => {}
        }
    }
}

unsafe fn add_check_item(hmenu: HMENU, id: usize, label: &str, checked: bool) {
    let w = to_wide(label);
    let fl = if checked {
        MF_STRING | MF_CHECKED
    } else {
        MF_STRING | MF_UNCHECKED
    };
    let _ = AppendMenuW(hmenu, fl, id, PCWSTR(w.as_ptr()));
}

fn is_os_session_ending_lparam(lparam: isize) -> bool {
    const ENDSESSION_CLOSEAPP_FLAG: u32 = 0x0000_0001;
    const ENDSESSION_LOGOFF_FLAG: u32 = 0x8000_0000;
    let flags = lparam as u32;
    (flags & ENDSESSION_CLOSEAPP_FLAG) == 0 || (flags & ENDSESSION_LOGOFF_FLAG) != 0
}

fn fast_os_session_shutdown(state: &mut AppState) {
    if state.shutting_down {
        return;
    }
    state.shutting_down = true;
    state.os_session_ending = true;
    if state.config_dirty {
        state.config_dirty = false;
        flush_pending_config_save(&state.config);
    }
    state.unhook_all();
    if !state.clean_exit_event.0.is_null() {
        unsafe {
            let _ = SetEvent(state.clean_exit_event);
        }
    }
    taskbar_hide::mark_os_session_ending();
    state.styler.clear_shared_tap_only();
    state.widgets.prepare_os_shutdown();
    state.wallpaper.stop_for_os_exit();
    state.mac_dock.disable();
    state.styler.hide_overlays();
    state.settings.cleanup();
    state.mac_dock.cleanup();
    state.widgets.cleanup();
    crate::realistic_pbr::evict_all();
}

fn restore_on_exit(state: &mut AppState) {
    if state.os_session_ending {
        fast_os_session_shutdown(state);
        return;
    }
    if state.shutting_down {
        return;
    }
    state.shutting_down = true;
    if state.config_dirty {
        state.config_dirty = false;
        flush_pending_config_save(&state.config);
    }
    state.unhook_all();
    // Restore Windows taskbar, workarea, and native surface IMMEDIATELY before slow COM/thread teardown
    mac_dock::restore_tray_cloak_and_alpha();
    state.mac_dock.disable();
    state.styler.hide_overlays();
    TaskbarHider::uncloak_taskbar_windows();
    if state.config.mac_dock_enabled
        || state.hider.is_hidden
        || (state.hider.main_taskbar_exists()
            && (!TaskbarHider::is_main_taskbar_visible() || TaskbarHider::is_main_taskbar_cloaked()))
    {
        state.hider.show(!get_native_autohide());
    }
    state.styler.cleanup();
    if state.styler.needs_explorer_restart()
        || (state.hider.main_taskbar_exists()
            && (!TaskbarHider::is_main_taskbar_visible() || TaskbarHider::is_main_taskbar_cloaked()))
    {
        TaskbarStyler::spawn_background_restore_explorer();
    }
    remove_tray_icon(state.hidden_hwnd);
    write_status_snapshot("EXIT", state);
    state.settings.cleanup();
    state.mac_dock.cleanup();
    state.widgets.cleanup();
    state.wallpaper.cleanup();
    crate::realistic_pbr::evict_all();
    if !state.clean_exit_event.0.is_null() {
        unsafe {
            let _ = SetEvent(state.clean_exit_event);
        }
    }
}

fn shutdown_app(state: &mut AppState) {
    let reentrant_mf = state.wallpaper.is_in_mf_call();
    restore_on_exit(state);
    if !state.hidden_hwnd.0.is_null() {
        unsafe {
            let _ = DestroyWindow(state.hidden_hwnd);
        }
    }
    if reentrant_mf {
        // Invoked re-entrantly inside combase's STA modal PeekMessage loop under an active
        // IMFMediaEngine call; returning up into torn-down MF/COM frames would deadlock or UAF.
        std::process::exit(0);
    }
}

fn add_tray_icon(hwnd: HWND, icon: HICON, hidden: bool, cfg: &AppConfig) {
    let mut nid = build_nid(hwnd, icon, hidden, cfg);
    unsafe {
        let _ = Shell_NotifyIconW(NIM_ADD, &mut nid);
    }
}

fn update_tray_tooltip(hwnd: HWND, hidden: bool, cfg: &AppConfig) {
    let mut nid = build_nid(hwnd, HICON::default(), hidden, cfg);
    nid.uFlags = NIF_TIP;
    unsafe {
        let _ = Shell_NotifyIconW(NIM_MODIFY, &mut nid);
    }
}

fn remove_tray_icon(hwnd: HWND) {
    let mut nid = NOTIFYICONDATAW {
        cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd,
        uID: TRAY_ICON_ID,
        ..Default::default()
    };
    unsafe {
        let _ = Shell_NotifyIconW(NIM_DELETE, &mut nid);
    }
}

fn build_nid(hwnd: HWND, icon: HICON, hidden: bool, cfg: &AppConfig) -> NOTIFYICONDATAW {
    let tip = format!(
        "AutumnDeck Studio • {} • {}",
        cfg.pill_mode.label(),
        if hidden { "Hidden" } else { "Visible" }
    );
    let mut nid = NOTIFYICONDATAW {
        cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd,
        uID: TRAY_ICON_ID,
        uFlags: NIF_MESSAGE | NIF_ICON | NIF_TIP,
        uCallbackMessage: WM_APP_TRAY,
        hIcon: icon,
        ..Default::default()
    };
    let wtip = to_wide(&tip);
    let copy_len = wtip.len().min(nid.szTip.len() - 1);
    nid.szTip[..copy_len].copy_from_slice(&wtip[..copy_len]);
    nid
}

fn create_amber_pill_icon() -> HICON {
    unsafe {
        let bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: 16,
                biHeight: -16,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits_ptr: *mut std::ffi::c_void = null_mut();
        let Ok(hbm_color) = CreateDIBSection(None, &bmi, DIB_RGB_COLORS, &mut bits_ptr, None, 0)
        else {
            return HICON::default();
        };
        if !bits_ptr.is_null() {
            let px = std::slice::from_raw_parts_mut(bits_ptr as *mut u32, 256);
            for y in 0..16i32 {
                for x in 0..16i32 {
                    let inside = x >= 4 && x <= 11 && y >= 1 && y <= 14;
                    let border = inside && (x == 4 || x == 11 || y == 1 || y == 14);
                    let pip = x >= 6 && x <= 9 && (y == 4 || y == 8 || y == 11);
                    let argb = if border {
                        0xFFF5AC48u32
                    } else if pip {
                        0xFFFFF3DCu32
                    } else if inside {
                        0xEE181310u32
                    } else {
                        0x00000000u32
                    };
                    px[(y * 16 + x) as usize] = argb;
                }
            }
        }
        let mask_words = [0u16; 16];
        let hbm_mask: HBITMAP = CreateBitmap(16, 16, 1, 1, Some(mask_words.as_ptr() as *const _));
        let ii = ICONINFO {
            fIcon: true.into(),
            xHotspot: 0,
            yHotspot: 0,
            hbmMask: hbm_mask,
            hbmColor: hbm_color,
        };
        let icon = CreateIconIndirect(&ii).unwrap_or_default();
        let _ = DeleteObject(HGDIOBJ(hbm_color.0));
        let _ = DeleteObject(HGDIOBJ(hbm_mask.0));
        icon
    }
}

fn write_status_snapshot(tag: &str, state: &AppState) {
    static LAST_STATUS_WRITE: std::sync::Mutex<Option<Instant>> = std::sync::Mutex::new(None);
    let force_tag = matches!(tag, "INIT" | "EXIT" | "BOOT" | "BOOT_SETTLED");
    if !force_tag {
        if let Ok(mut guard) = LAST_STATUS_WRITE.lock() {
            let now = Instant::now();
            if let Some(prev) = *guard {
                if now.duration_since(prev) < Duration::from_millis(350) {
                    return;
                }
            }
            *guard = Some(now);
        }
    }
    let temp_dir = std::env::temp_dir();
    let path: PathBuf = temp_dir.join("autumn_deck_status.txt");
    let monitors = TaskbarHider::query_all_monitors();
    let tray_vis = unsafe {
        FindWindowW(w!("Shell_TrayWnd"), PCWSTR::null())
            .map(|h| {
                if !h.0.is_null() && IsWindowVisible(h).as_bool() {
                    1
                } else {
                    0
                }
            })
            .unwrap_or(0)
    };
    let work_str = monitors
        .first()
        .map(|m| {
            format!(
                "({},{},{},{})",
                m.rcWork.left, m.rcWork.top, m.rcWork.right, m.rcWork.bottom
            )
        })
        .unwrap_or_else(|| "(0,0,0,0)".to_string());

    let (wp_bd, wp_bvalid, wp_bvis, wp_blay, wp_vvis, wp_lwa, wp_paint) = state.wallpaper.diagnostic_flags();
    let tap_diag = state.styler.tap_diag_snapshot();
    let tap_hr_str = tap_diag
        .last_hr
        .map(|h| format!("0x{:08X}", h as u32))
        .unwrap_or_else(|| "none".to_string());
    let tap_mode_str = if tap_diag.last_stage_mode.is_empty() {
        "none"
    } else {
        tap_diag.last_stage_mode.as_str()
    };
    let content = format!(
        "tag={} hidden={} peek={} mode={} pill={} surface={} alpha={} amber={} tap={} tap_tried={} tap_hr={} tap_mode={} pills={} bar={} work={} tray_vis={} widgets={} widgets_gpu={} wp={} wp_active={} wp_bd={} wp_hw={} wp_bvis={} wp_lay={} wp_lwa={} wp_paint={} wp_vvis={} pid={}\r\n",
        tag,
        state.hider.is_hidden,
        state.hider.peek_visible,
        state.config.mode.as_str(),
        state.config.pill_mode.as_str(),
        state.config.surface_mode.as_str(),
        state.config.tint_alpha,
        state.config.amber_border,
        state.styler.tap_connected,
        tap_diag.attempted,
        tap_hr_str,
        tap_mode_str,
        state.styler.last_pill_count,
        state.styler.last_bar_rect_str,
        work_str,
        tray_vis,
        state.widgets.is_visible(),
        state.widgets.gpu_backend_label(),
        state.config.wallpaper_enabled,
        state.wallpaper.is_active(),
        wp_bd,
        wp_bvalid,
        wp_bvis,
        wp_blay,
        wp_lwa,
        wp_paint,
        wp_vvis,
        std::process::id(),
    );
    let _ = fs::write(path, content);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endsession_lparam_distinguishes_os_shutdown_from_single_app_restart() {
        assert!(is_os_session_ending_lparam(0));
        assert!(is_os_session_ending_lparam(0x4000_0000)); // ENDSESSION_CRITICAL
        assert!(is_os_session_ending_lparam(0x8000_0000u32 as isize)); // ENDSESSION_LOGOFF
        assert!(is_os_session_ending_lparam(0x8000_0001u32 as isize)); // CLOSEAPP + LOGOFF
        assert!(!is_os_session_ending_lparam(0x0000_0001)); // pure ENDSESSION_CLOSEAPP
    }

    #[test]
    fn boot_settle_short_circuits_immediately_when_ready() {
        assert!(should_short_circuit_boot_settle(true, true, true, true));
        assert!(!should_short_circuit_boot_settle(false, true, true, true));
        assert!(!should_short_circuit_boot_settle(true, false, true, true));
        assert!(!should_short_circuit_boot_settle(true, true, false, true));
        assert!(!should_short_circuit_boot_settle(true, true, true, false));
    }

    #[test]
    fn viz_timer_cadence_scales_dynamically_with_anim_fps() {
        // Default 45 FPS -> 22ms; 24 Eco -> 41ms; 30 Standard -> 33ms; 60 -> 16ms; 90 -> 11ms; 120 -> 8ms
        assert_eq!(
            compute_viz_timer_ms(45, false, false, false, false, false, true, false, false, true, true, false),
            22
        );
        assert_eq!(
            compute_viz_timer_ms(24, false, false, false, false, false, true, false, false, true, true, false),
            41
        );
        assert_eq!(
            compute_viz_timer_ms(30, false, false, false, true, false, false, false, false, false, false, false),
            33
        );
        assert_eq!(
            compute_viz_timer_ms(60, false, true, false, false, false, true, false, false, true, true, false),
            16
        );
        assert_eq!(
            compute_viz_timer_ms(90, false, false, true, false, false, false, false, false, false, false, false),
            11
        );
        assert_eq!(
            compute_viz_timer_ms(120, false, false, false, false, false, false, false, true, false, true, false),
            8
        );
        // Fullscreen game mode silences visualizer timer completely (0ms)
        assert_eq!(
            compute_viz_timer_ms(45, true, false, true, true, true, true, false, false, true, true, false),
            0
        );
        // Idle desktop with no active animations stops timer completely (0ms)
        assert_eq!(
            compute_viz_timer_ms(45, false, false, false, false, false, false, false, false, false, false, false),
            0
        );
        // Unhovered Studio preview runs calmly at 2x frame interval (clamped 45..=90ms)
        assert_eq!(
            compute_viz_timer_ms(45, false, false, false, false, false, false, false, false, false, false, true),
            45
        );
        assert_eq!(
            compute_viz_timer_ms(24, false, false, false, false, false, false, false, false, false, false, true),
            82
        );
        // Hovered Studio preview wakes to max(16, active_ms)
        assert_eq!(
            compute_viz_timer_ms(45, false, false, false, false, false, false, true, false, false, false, true),
            22
        );
        assert_eq!(
            compute_viz_timer_ms(120, false, false, false, false, false, false, true, false, false, false, true),
            16
        );
    }
}
