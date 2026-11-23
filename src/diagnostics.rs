use std::collections::VecDeque;
use std::fs;
use std::mem::size_of;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use windows::core::{w, BOOL, PCWSTR};
use windows::Win32::Foundation::{LPARAM, RECT};
use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, IDXGIFactory1};
use windows::Win32::Graphics::Gdi::{
    EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFOEXW,
};
use windows::Win32::System::Registry::{
    RegCloseKey, RegOpenKeyExW, RegQueryValueExW, HKEY, HKEY_LOCAL_MACHINE, KEY_READ, REG_DWORD,
    REG_SZ,
};
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::WindowsAndMessaging::{FindWindowW, GetWindowRect};

use crate::config::{
    app_data_dir, is_logon_task_enabled, is_startup_enabled, serialize_config_ini, AppConfig,
};
use crate::taskbar_hide::{is_fullscreen_game_or_app_active, TaskbarHider};
use crate::taskbar_styler::has_app_package_rx_acl;

const MAX_EVENT_LOG_ENTRIES: usize = 80;
static EVENT_LOG: Mutex<VecDeque<String>> = Mutex::new(VecDeque::new());

/// Appends a lightweight timestamped lifecycle entry to the in-memory diagnostic ring buffer.
pub fn record_event(tag: &str, detail: &str) {
    let ts = format_local_hms();
    let line = format!("[{}] {}: {}", ts, tag, sanitize_user_paths(detail));
    if let Ok(mut guard) = EVENT_LOG.lock() {
        if guard.len() >= MAX_EVENT_LOG_ENTRIES {
            guard.pop_front();
        }
        guard.push_back(line);
    }
}

fn snapshot_events() -> Vec<String> {
    EVENT_LOG
        .lock()
        .map(|g| g.iter().cloned().collect())
        .unwrap_or_default()
}

fn format_local_hms() -> String {
    #[repr(C)]
    #[derive(Default)]
    struct SystemTime {
        year: u16,
        month: u16,
        day_of_week: u16,
        day: u16,
        hour: u16,
        minute: u16,
        second: u16,
        millis: u16,
    }
    #[link(name = "kernel32")]
    extern "system" {
        fn GetLocalTime(lp_system_time: *mut SystemTime);
    }
    let mut st = SystemTime::default();
    unsafe {
        GetLocalTime(&mut st);
    }
    format!("{:02}:{:02}:{:02}", st.hour, st.minute, st.second)
}

fn format_local_filename_stamp() -> String {
    #[repr(C)]
    #[derive(Default)]
    struct SystemTime {
        year: u16,
        month: u16,
        day_of_week: u16,
        day: u16,
        hour: u16,
        minute: u16,
        second: u16,
        millis: u16,
    }
    #[link(name = "kernel32")]
    extern "system" {
        fn GetLocalTime(lp_system_time: *mut SystemTime);
    }
    let mut st = SystemTime::default();
    unsafe {
        GetLocalTime(&mut st);
    }
    format!(
        "{:04}{:02}{:02}_{:02}{:02}{:02}",
        st.year, st.month, st.day, st.hour, st.minute, st.second
    )
}

/// Redacts the current Windows username and profile path from diagnostic text so support bundles
/// are safe to share publicly on GitHub, Reddit, or Discord.
pub fn sanitize_user_paths(input: &str) -> String {
    let user = std::env::var("USERNAME").unwrap_or_default();
    let profile = std::env::var("USERPROFILE").unwrap_or_default();
    sanitize_with_identity(input, &user, &profile)
}

fn sanitize_with_identity(input: &str, username: &str, userprofile: &str) -> String {
    let mut out = input.to_string();
    if !userprofile.is_empty() {
        out = replace_ascii_case_insensitive(&out, userprofile, r"C:\Users\<USER>");
        let fwd = userprofile.replace('\\', "/");
        if fwd != userprofile {
            out = replace_ascii_case_insensitive(&out, &fwd, "C:/Users/<USER>");
        }
    }
    if !username.is_empty() && username.len() >= 2 {
        out = replace_ascii_case_insensitive(
            &out,
            &format!(r"\Users\{}\", username),
            r"\Users\<USER>\",
        );
        out = replace_ascii_case_insensitive(
            &out,
            &format!("/Users/{}/", username),
            "/Users/<USER>/",
        );
        out = replace_ascii_case_insensitive(
            &out,
            &format!(r"\Users\{}", username),
            r"\Users\<USER>",
        );
    }
    out
}

fn replace_ascii_case_insensitive(haystack: &str, needle: &str, replacement: &str) -> String {
    if needle.is_empty() || haystack.len() < needle.len() {
        return haystack.to_string();
    }
    let low_h = haystack.to_ascii_lowercase();
    let low_n = needle.to_ascii_lowercase();
    let mut result = String::with_capacity(haystack.len());
    let mut cursor = 0usize;
    while let Some(rel) = low_h[cursor..].find(&low_n) {
        let start = cursor + rel;
        result.push_str(&haystack[cursor..start]);
        result.push_str(replacement);
        cursor = start + needle.len();
    }
    result.push_str(&haystack[cursor..]);
    result
}

fn read_hklm_string(subkey: PCWSTR, value_name: PCWSTR) -> Option<String> {
    unsafe {
        let mut hk = HKEY::default();
        if RegOpenKeyExW(HKEY_LOCAL_MACHINE, subkey, None, KEY_READ, &mut hk).is_err() {
            return None;
        }
        let mut typ = REG_SZ;
        let mut buf = [0u8; 512];
        let mut len = buf.len() as u32;
        let ok = RegQueryValueExW(
            hk,
            value_name,
            None,
            Some(&mut typ),
            Some(buf.as_mut_ptr()),
            Some(&mut len),
        )
        .is_ok();
        let _ = RegCloseKey(hk);
        if !ok || len < 2 {
            return None;
        }
        let u16_count = (len as usize) / 2;
        let words: Vec<u16> = buf[..u16_count * 2]
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .take_while(|&w| w != 0)
            .collect();
        Some(String::from_utf16_lossy(&words))
    }
}

fn read_hklm_dword(subkey: PCWSTR, value_name: PCWSTR) -> Option<u32> {
    unsafe {
        let mut hk = HKEY::default();
        if RegOpenKeyExW(HKEY_LOCAL_MACHINE, subkey, None, KEY_READ, &mut hk).is_err() {
            return None;
        }
        let mut typ = REG_DWORD;
        let mut val = 0u32;
        let mut len = size_of::<u32>() as u32;
        let ok = RegQueryValueExW(
            hk,
            value_name,
            None,
            Some(&mut typ),
            Some((&mut val as *mut u32) as *mut u8),
            Some(&mut len),
        )
        .is_ok();
        let _ = RegCloseKey(hk);
        if ok { Some(val) } else { None }
    }
}

fn collect_os_summary() -> String {
    let key = w!(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion");
    let product = read_hklm_string(key, w!("ProductName")).unwrap_or_else(|| "Windows".to_string());
    let disp = read_hklm_string(key, w!("DisplayVersion")).unwrap_or_else(|| "unknown".to_string());
    let build = read_hklm_string(key, w!("CurrentBuild")).unwrap_or_else(|| "0".to_string());
    let ubr = read_hklm_dword(key, w!("UBR")).unwrap_or(0);
    format!("{} ({}) Build {}.{}", product, disp, build, ubr)
}

fn collect_gpu_lines() -> Vec<String> {
    let mut lines = Vec::new();
    unsafe {
        if let Ok(factory) = CreateDXGIFactory1::<IDXGIFactory1>() {
            let mut idx = 0u32;
            while let Ok(adapter) = factory.EnumAdapters1(idx) {
                if let Ok(desc) = adapter.GetDesc1() {
                    let end = desc
                        .Description
                        .iter()
                        .position(|&c| c == 0)
                        .unwrap_or(desc.Description.len());
                    let name = String::from_utf16_lossy(&desc.Description[..end]);
                    let vram_mb = desc.DedicatedVideoMemory / (1024 * 1024);
                    let shared_mb = desc.SharedSystemMemory / (1024 * 1024);
                    lines.push(format!(
                        "gpu_{}={} (VRAM={} MB, Shared={} MB, Vendor=0x{:04X}, Device=0x{:04X})",
                        idx, name, vram_mb, shared_mb, desc.VendorId, desc.DeviceId
                    ));
                }
                idx += 1;
                if idx >= 8 {
                    break;
                }
            }
        }
    }
    if lines.is_empty() {
        lines.push("gpu_0=Unavailable".to_string());
    }
    lines
}

fn collect_monitor_lines() -> Vec<String> {
    unsafe extern "system" fn enum_mon(
        hmon: HMONITOR,
        _hdc: HDC,
        _rc: *mut RECT,
        lparam: LPARAM,
    ) -> BOOL {
        let out = &mut *(lparam.0 as *mut Vec<String>);
        let mut mi = MONITORINFOEXW::default();
        mi.monitorInfo.cbSize = size_of::<MONITORINFOEXW>() as u32;
        if GetMonitorInfoW(hmon, &mut mi.monitorInfo).as_bool() {
            let r = mi.monitorInfo.rcMonitor;
            let w = mi.monitorInfo.rcWork;
            let primary = (mi.monitorInfo.dwFlags & 1) != 0;
            let mut dpi_x = 96u32;
            let mut dpi_y = 96u32;
            let _ = GetDpiForMonitor(hmon, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y);
            let scale_pct = ((dpi_x as f32 / 96.0) * 100.0).round() as u32;
            out.push(format!(
                "monitor_{}=primary:{} size:{}x{} rect:({},{},{},{}) work:({},{},{},{}) dpi:{} ({}%)",
                out.len(),
                primary,
                r.right - r.left,
                r.bottom - r.top,
                r.left,
                r.top,
                r.right,
                r.bottom,
                w.left,
                w.top,
                w.right,
                w.bottom,
                dpi_x,
                scale_pct
            ));
        }
        BOOL(1)
    }

    let mut lines = Vec::new();
    unsafe {
        let _ = EnumDisplayMonitors(
            None,
            None,
            Some(enum_mon),
            LPARAM((&mut lines as *mut Vec<String>) as isize),
        );
    }
    lines
}

fn build_support_report(cfg: &AppConfig, game_mode_active: bool) -> String {
    let mut out = String::with_capacity(8192);
    out.push_str("====================================================================\n");
    out.push_str("AutumnDeck Diagnostic & Support Bundle (User Paths Sanitized)\n");
    out.push_str("====================================================================\n\n");

    let exe_path = std::env::current_exe().unwrap_or_default();
    let exe_dir = exe_path.parent().map(Path::to_path_buf).unwrap_or_default();
    let exe_size = fs::metadata(&exe_path).map(|m| m.len()).unwrap_or(0);
    let cores = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1);

    out.push_str("[System]\n");
    out.push_str(&format!("app_version={}\n", env!("CARGO_PKG_VERSION")));
    out.push_str(&format!("timestamp_local={}\n", format_local_filename_stamp()));
    out.push_str(&format!("pid={}\n", std::process::id()));
    out.push_str(&format!("os={}\n", collect_os_summary()));
    out.push_str(&format!("logical_cpu_cores={}\n", cores));
    out.push_str(&format!(
        "exe_path={}\n",
        sanitize_user_paths(&exe_path.to_string_lossy())
    ));
    out.push_str(&format!("exe_bytes={}\n\n", exe_size));

    out.push_str("[Hardware]\n");
    for line in collect_gpu_lines() {
        out.push_str(&line);
        out.push('\n');
    }
    for line in collect_monitor_lines() {
        out.push_str(&line);
        out.push('\n');
    }
    out.push('\n');

    out.push_str("[Components]\n");
    let guard_path = exe_dir.join("AutumnGuard.exe");
    let guard_bytes = fs::metadata(&guard_path).map(|m| m.len()).unwrap_or(0);
    out.push_str(&format!(
        "autumn_guard_present={} bytes={}\n",
        guard_path.exists(),
        guard_bytes
    ));
    let tap_path = exe_dir.join("AutumnXamlTap.dll");
    let tap_bytes = fs::metadata(&tap_path).map(|m| m.len()).unwrap_or(0);
    let tap_acl = match has_app_package_rx_acl(&tap_path) {
        Ok(true) => "OK (ALL APPLICATION PACKAGES RX)".to_string(),
        Ok(false) => "MISSING (S-1-15-2-1 RX not granted)".to_string(),
        Err(code) => format!("ERROR (win32={})", code),
    };
    out.push_str(&format!(
        "autumn_xaml_tap_present={} bytes={} acl={}\n",
        tap_path.exists(),
        tap_bytes,
        tap_acl
    ));
    out.push_str(&format!("startup_run_key={}\n", is_startup_enabled()));
    out.push_str(&format!("startup_logon_task={}\n", is_logon_task_enabled()));
    out.push_str(&format!(
        "game_mode_pause_enabled={} game_mode_active_now={} live_fullscreen_probe={}\n",
        cfg.game_mode_pause,
        game_mode_active,
        is_fullscreen_game_or_app_active()
    ));

    let tray_vis = TaskbarHider::is_main_taskbar_visible();
    let tray_cloaked = TaskbarHider::is_main_taskbar_cloaked();
    let mut tray_rc = RECT::default();
    unsafe {
        if let Ok(hw) = FindWindowW(w!("Shell_TrayWnd"), PCWSTR::null()) {
            if !hw.0.is_null() {
                let _ = GetWindowRect(hw, &mut tray_rc);
            }
        }
    }
    out.push_str(&format!(
        "shell_tray_visible={} cloaked={} rect=({},{},{},{})\n\n",
        tray_vis, tray_cloaked, tray_rc.left, tray_rc.top, tray_rc.right, tray_rc.bottom
    ));

    out.push_str("[StatusSnapshot]\n");
    let status_file = std::env::temp_dir().join("autumn_deck_status.txt");
    match fs::read_to_string(&status_file) {
        Ok(content) => out.push_str(&sanitize_user_paths(&content)),
        Err(_) => out.push_str("(no status snapshot found)\n"),
    }
    if !out.ends_with('\n') {
        out.push('\n');
    }
    out.push('\n');

    out.push_str("[RuntimeEvents]\n");
    let events = snapshot_events();
    if events.is_empty() {
        out.push_str("(no runtime events recorded in this session)\n");
    } else {
        for ev in events {
            out.push_str(&ev);
            out.push('\n');
        }
    }
    out.push('\n');

    out.push_str("[SanitizedConfig]\n");
    out.push_str(&sanitize_user_paths(&serialize_config_ini(cfg)));
    out
}

/// Writes a sanitized Support Bundle `.txt` file into `dest_dir` (defaults to
/// `%APPDATA%\AutumnDeck\Diagnostics`). Optionally highlights the file in File Explorer.
pub fn export_support_bundle_to(
    dest_dir: &Path,
    cfg: &AppConfig,
    game_mode_active: bool,
    reveal_in_explorer: bool,
) -> Result<PathBuf, String> {
    fs::create_dir_all(dest_dir).map_err(|e| format!("create_dir_all failed: {}", e))?;
    let file_name = format!("AutumnDeck_Support_{}.txt", format_local_filename_stamp());
    let out_path = dest_dir.join(file_name);
    let report = build_support_report(cfg, game_mode_active);
    fs::write(&out_path, report.as_bytes()).map_err(|e| format!("write failed: {}", e))?;
    record_event("EXPORT_LOGS", &out_path.to_string_lossy());

    if reveal_in_explorer {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let _ = std::process::Command::new("explorer.exe")
            .arg(format!("/select,{}", out_path.to_string_lossy()))
            .creation_flags(CREATE_NO_WINDOW)
            .spawn();
    }
    Ok(out_path)
}

pub fn export_support_bundle(
    cfg: &AppConfig,
    game_mode_active: bool,
    reveal_in_explorer: bool,
) -> Result<PathBuf, String> {
    let dir = app_data_dir().join("Diagnostics");
    export_support_bundle_to(&dir, cfg, game_mode_active, reveal_in_explorer)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_user_paths_redacts_username_case_insensitively() {
        let raw = r"wallpaper_path=C:\USERS\AliceDev\Videos\rain.mp4; log=c:/users/alicedev/AppData/Roaming";
        let clean = sanitize_with_identity(raw, "AliceDev", r"C:\Users\AliceDev");
        assert!(!clean.to_ascii_lowercase().contains("alicedev"));
        assert!(clean.contains(r"C:\Users\<USER>\Videos\rain.mp4"));
        assert!(clean.contains("C:/Users/<USER>/AppData/Roaming"));
    }

    #[test]
    fn export_support_bundle_writes_complete_sanitized_report() {
        let tmp_dir = std::env::temp_dir().join(format!(
            "autumn_diag_test_{}",
            std::process::id()
        ));
        let mut cfg = AppConfig::default();
        if let Ok(prof) = std::env::var("USERPROFILE") {
            cfg.wallpaper_path = format!(r"{}\Videos\demo_loop.mp4", prof);
        }
        record_event("TEST", "diagnostic bundle unit verification");

        let path = export_support_bundle_to(&tmp_dir, &cfg, false, false)
            .expect("export_support_bundle_to must succeed");
        let text = fs::read_to_string(&path).expect("bundle file must be readable");

        assert!(text.contains("[System]"));
        assert!(text.contains("[Hardware]"));
        assert!(text.contains("[Components]"));
        assert!(text.contains("[SanitizedConfig]"));
        assert!(text.contains("game_mode_pause=1"));
        assert!(text.contains("diagnostic bundle unit verification"));
        if let Ok(user) = std::env::var("USERNAME") {
            if user.len() >= 3 {
                assert!(
                    !text
                        .to_ascii_lowercase()
                        .contains(&format!(r"\users\{}", user.to_ascii_lowercase())),
                    "Exported support bundle must redact raw username path"
                );
            }
        }

        let _ = fs::remove_dir_all(&tmp_dir);
    }
}
