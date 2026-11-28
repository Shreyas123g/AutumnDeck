use std::collections::{HashMap, HashSet};
use std::f32::consts::TAU;
use std::ffi::c_void;
use std::mem::size_of;
use std::path::{Path, PathBuf};
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU32, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use windows::core::{w, BOOL, GUID, HRESULT, HSTRING, Interface, PCWSTR, PCSTR, PWSTR};
use windows::Devices::Radios::{Radio, RadioAccessStatus, RadioKind, RadioState};
use windows::Foundation::{IPropertyValue, PropertyType, PropertyValue};
use windows::Media::Control::{
    GlobalSystemMediaTransportControlsSessionManager,
    GlobalSystemMediaTransportControlsSessionPlaybackStatus,
};
use windows::Networking::Connectivity::NetworkInformation;
use windows::Networking::NetworkOperators::{
    NetworkOperatorTetheringManager, TetheringOperationalState, TetheringOperationStatus,
};
use windows::Storage::Streams::DataReader;
use windows::Win32::Devices::DeviceAndDriverInstallation::{
    SetupDiDestroyDeviceInfoList, SetupDiEnumDeviceInfo, SetupDiGetClassDevsW,
    SetupDiGetDeviceInstanceIdW, SetupDiGetDevicePropertyW, DIGCF_ALLCLASSES, DIGCF_PRESENT,
    SP_DEVINFO_DATA,
};
use windows::Win32::Devices::Properties::DEVPROPTYPE;
use windows::Win32::Foundation::{CloseHandle, DEVPROPKEY, HWND, LPARAM, POINT, PROPERTYKEY, SIZE, WPARAM};
use windows::Win32::Graphics::Gdi::{
    DeleteObject, GetDC, GetDIBits, GetObjectW, ReleaseDC, BITMAP, BITMAPINFO,
    BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, HBITMAP, HGDIOBJ,
};
use windows::Win32::Media::Audio::Endpoints::{IAudioEndpointVolume, IAudioMeterInformation};
use windows::Win32::Media::Audio::{
    eCommunications, eConsole, eMultimedia, eRender, AudioSessionStateActive, ERole,
    IAudioCaptureClient, IAudioClient, IAudioSessionControl2, IAudioSessionManager2,
    IMMDeviceEnumerator, MMDeviceEnumerator, AUDCLNT_SHAREMODE_SHARED,
    AUDCLNT_STREAMFLAGS_LOOPBACK, DEVICE_STATE_ACTIVE, WAVEFORMATEXTENSIBLE,
};
use windows::Win32::NetworkManagement::WiFi::{
    dot11_radio_state_off, dot11_radio_state_on, wlan_intf_opcode_current_connection,
    WlanCloseHandle, WlanEnumInterfaces, WlanFreeMemory, WlanOpenHandle, WlanQueryInterface,
    WlanSetInterface, WLAN_CONNECTION_ATTRIBUTES, WLAN_INTERFACE_INFO_LIST, WLAN_PHY_RADIO_STATE,
};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
};
use windows::Win32::System::Com::StructuredStorage::{PropVariantClear, PropVariantToStringAlloc};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, CLSCTX_ALL,
    CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, STGM_READ,
};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::IO::DeviceIoControl;
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
use windows::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
use windows::Win32::System::Registry::{
    RegCloseKey, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW, HKEY, HKEY_CURRENT_USER,
    HKEY_LOCAL_MACHINE, KEY_READ, KEY_WRITE, REG_BINARY, REG_DWORD,
};
use windows::Win32::System::SystemInformation::GetTickCount64;
use windows::Win32::System::Threading::{
    GetCurrentThread, OpenProcess, QueryFullProcessImageNameW, SetThreadPriority,
    PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, THREAD_PRIORITY_BELOW_NORMAL,
};
use windows::Win32::System::WinRT::{RoInitialize, RoUninitialize, RO_INIT_MULTITHREADED};
use windows::Win32::UI::Accessibility::{
    CUIAutomation, IUIAutomation, IUIAutomationElement, IUIAutomationInvokePattern,
    IUIAutomationScrollItemPattern, IUIAutomationTogglePattern, TreeScope_Descendants,
    UIA_InvokePatternId, UIA_ScrollItemPatternId, UIA_TogglePatternId,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    keybd_event, mouse_event, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT,
    KEYBD_EVENT_FLAGS, KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP, MOUSEEVENTF_LEFTDOWN,
    MOUSEEVENTF_LEFTUP, VIRTUAL_KEY, VK_K, VK_LWIN, VK_MEDIA_NEXT_TRACK, VK_MEDIA_PLAY_PAUSE,
    VK_MEDIA_PREV_TRACK,
};
use windows::Win32::UI::Shell::PropertiesSystem::IPropertyStore;
use windows::Win32::UI::Shell::{
    IShellItemImageFactory, SHCreateItemFromParsingName, ShellExecuteW, SIIGBF_BIGGERSIZEOK,
    SIIGBF_THUMBNAILONLY,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumChildWindows, EnumWindows, FindWindowW, GetClassNameW, GetCursorPos, GetSystemMetrics,
    GetWindowTextW, GetWindowThreadProcessId, IsWindow, IsWindowVisible, PostMessageW,
    SendMessageTimeoutW, SetCursorPos, SMTO_ABORTIFHUNG, SM_CMONITORS, SW_SHOWNORMAL,
};

#[derive(Clone)]
pub struct MediaSnapshot {
    pub title: String,
    pub artist: String,
    pub is_playing: bool,
    pub cover_bytes: Option<Arc<Vec<u8>>>,
    pub cover_seq: u64,
}

impl Default for MediaSnapshot {
    fn default() -> Self {
        Self {
            title: "Autumn Sanctuary".to_string(),
            artist: "Play any audio or music track".to_string(),
            is_playing: false,
            cover_bytes: None,
            cover_seq: 0,
        }
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct AudioOutputDevice {
    pub id: String,
    pub name: String,
    pub is_default: bool,
}

#[derive(Clone, PartialEq, Eq)]
pub struct CcSnapshot {
    pub wifi_on: bool,
    pub wifi_name: String,
    pub wifi_signal: u8,
    pub bt_on: bool,
    pub bt_sub: String,
    pub air_on: bool,
    pub acc_on: bool,
    pub saver_on: bool,
    pub cap_on: bool,
    pub night_on: bool,
    pub hot_on: bool,
    pub near_on: bool,
    pub disp_on: bool,
    pub bright_pct: u8,
    pub vol_pct: u8,
    pub vol_muted: bool,
    pub audio_outputs: Vec<AudioOutputDevice>,
}

impl Default for CcSnapshot {
    fn default() -> Self {
        Self {
            wifi_on: true,
            wifi_name: "Wi-Fi".to_string(),
            wifi_signal: 85,
            bt_on: false,
            bt_sub: "Off".to_string(),
            air_on: false,
            acc_on: false,
            saver_on: false,
            cap_on: false,
            night_on: false,
            hot_on: false,
            near_on: false,
            disp_on: false,
            bright_pct: 65,
            vol_pct: 50,
            vol_muted: false,
            audio_outputs: Vec::new(),
        }
    }
}

#[derive(Clone, Debug)]
pub enum CcAction {
    MediaPrev,
    MediaPlayPause,
    MediaNext,
    ToggleWifi,
    OpenWifiList,
    ToggleBt,
    OpenBtList,
    ToggleAirplane,
    OpenAccessibility,
    ToggleEnergySaver,
    ToggleCaptions,
    ToggleNightLight,
    ToggleHotspot,
    ToggleNearby,
    OpenCast,
    OpenProject,
    SetBrightness(u8),
    SetVolume(u8),
    ToggleMute,
    SetAudioOutput(String),
    OpenTileSettings(usize),
}

struct SharedState {
    media: MediaSnapshot,
    cc: CcSnapshot,
    bands: [f32; 12],
}

#[derive(Default)]
struct ActionSignal {
    stop: bool,
    pending: bool,
}

pub struct SystemMediaEngine {
    shared: Arc<Mutex<SharedState>>,
    tx: Sender<CcAction>,
    stop: Arc<AtomicBool>,
    stop_wake: Arc<(Mutex<bool>, Condvar)>,
    action_wake: Arc<(Mutex<ActionSignal>, Condvar)>,
    cc_active: Arc<AtomicBool>,
    audio_outputs_active: Arc<AtomicBool>,
    audio_action_seq: Arc<AtomicU64>,
    viz_frame_ms: Arc<AtomicU32>,
    viz_needed: Arc<AtomicBool>,
    media_thread: Option<JoinHandle<()>>,
    audio_thread: Option<JoinHandle<()>>,
}

fn sleep_or_stop(wake: &(Mutex<bool>, Condvar), dur: Duration) -> bool {
    let Ok(guard) = wake.0.lock() else {
        return false;
    };
    if *guard {
        return false;
    }
    let res = wake.1.wait_timeout_while(guard, dur, |stopped| !*stopped);
    match res {
        Ok((g, _)) => !*g,
        Err(_) => false,
    }
}

fn wait_for_action_or_timeout(wake: &(Mutex<ActionSignal>, Condvar), dur: Duration) -> bool {
    let Ok(mut guard) = wake.0.lock() else {
        return false;
    };
    if guard.stop {
        return false;
    }
    if guard.pending {
        guard.pending = false;
        return true;
    }
    let res = wake
        .1
        .wait_timeout_while(guard, dur, |s| !s.stop && !s.pending);
    match res {
        Ok((mut g, _)) => {
            if g.stop {
                false
            } else {
                g.pending = false;
                true
            }
        }
        Err(_) => false,
    }
}

impl SystemMediaEngine {
    pub fn new(notify_hwnd: HWND, notify_msg: u32) -> Self {
        let shared = Arc::new(Mutex::new(SharedState {
            media: MediaSnapshot::default(),
            cc: CcSnapshot::default(),
            bands: [0.0; 12],
        }));
        let stop = Arc::new(AtomicBool::new(false));
        let stop_wake = Arc::new((Mutex::new(false), Condvar::new()));
        let action_wake = Arc::new((Mutex::new(ActionSignal::default()), Condvar::new()));
        let cc_active = Arc::new(AtomicBool::new(false));
        let audio_outputs_active = Arc::new(AtomicBool::new(false));
        let audio_action_seq = Arc::new(AtomicU64::new(0));
        let viz_frame_ms = Arc::new(AtomicU32::new(crate::config::AppConfig::anim_frame_ms_for(
            crate::config::AppConfig::DEFAULT_ANIM_FPS,
        )));
        let viz_needed = Arc::new(AtomicBool::new(true));
        let (tx, rx) = mpsc::channel();
        let hwnd_raw = notify_hwnd.0 as isize;

        let sh1 = Arc::clone(&shared);
        let st1 = Arc::clone(&stop);
        let awk1 = Arc::clone(&action_wake);
        let cca1 = Arc::clone(&cc_active);
        let aoa1 = Arc::clone(&audio_outputs_active);
        let aseq1 = Arc::clone(&audio_action_seq);
        let media_thread = thread::Builder::new()
            .name("autumn-media-cc".to_string())
            .spawn(move || {
                run_smtc_and_cc_worker(sh1, rx, st1, awk1, cca1, aoa1, aseq1, hwnd_raw, notify_msg);
            })
            .ok();

        let sh2 = Arc::clone(&shared);
        let st2 = Arc::clone(&stop);
        let wk2 = Arc::clone(&stop_wake);
        let fms2 = Arc::clone(&viz_frame_ms);
        let need2 = Arc::clone(&viz_needed);
        let audio_thread = thread::Builder::new()
            .name("autumn-wasapi-viz".to_string())
            .spawn(move || {
                run_wasapi_loopback_worker(sh2, st2, wk2, fms2, need2, hwnd_raw, notify_msg);
            })
            .ok();

        Self {
            shared,
            tx,
            stop,
            stop_wake,
            action_wake,
            cc_active,
            audio_outputs_active,
            audio_action_seq,
            viz_frame_ms,
            viz_needed,
            media_thread,
            audio_thread,
        }
    }

    pub fn set_anim_fps(&self, fps: u32) {
        let ms = crate::config::AppConfig::anim_frame_ms_for(fps);
        self.viz_frame_ms.store(ms, Ordering::Relaxed);
    }

    pub fn set_viz_needed(&self, needed: bool) {
        self.viz_needed.store(needed, Ordering::Relaxed);
    }

    pub fn set_cc_active(&self, active: bool) {
        let prev = self.cc_active.swap(active, Ordering::Relaxed);
        if active && !prev {
            if let Ok(mut g) = self.action_wake.0.lock() {
                g.pending = true;
                self.action_wake.1.notify_one();
            }
        }
    }

    pub fn set_audio_outputs_active(&self, active: bool) {
        let prev = self.audio_outputs_active.swap(active, Ordering::Relaxed);
        if active && !prev {
            if let Ok(mut g) = self.action_wake.0.lock() {
                g.pending = true;
                self.action_wake.1.notify_one();
            }
        }
    }

    pub fn media_snapshot(&self) -> MediaSnapshot {
        self.shared
            .lock()
            .map(|g| g.media.clone())
            .unwrap_or_default()
    }

    pub fn cc_snapshot(&self) -> CcSnapshot {
        self.shared
            .lock()
            .map(|g| g.cc.clone())
            .unwrap_or_default()
    }

    pub fn bands(&self) -> [f32; 12] {
        self.shared
            .lock()
            .map(|g| g.bands)
            .unwrap_or([0.0; 12])
    }

    pub fn is_audio_active(&self) -> bool {
        if let Ok(g) = self.shared.lock() {
            let muted_or_zero = g.cc.vol_muted || g.cc.vol_pct == 0;
            let has_bands = g.bands.iter().any(|&b| b > 0.005);
            return has_bands || (g.media.is_playing && !muted_or_zero);
        }
        false
    }

    pub fn send_action(&self, act: CcAction) {
        if matches!(
            act,
            CcAction::SetVolume(_) | CcAction::ToggleMute | CcAction::SetAudioOutput(_)
        ) {
            self.audio_action_seq.fetch_add(1, Ordering::AcqRel);
        }
        // Optimistic UI update so toggles, audio outputs, and sliders respond at 0ms latency
        if let Ok(mut g) = self.shared.lock() {
            match &act {
                CcAction::ToggleWifi => {
                    g.cc.wifi_on = !g.cc.wifi_on;
                    if !g.cc.wifi_on {
                        g.cc.wifi_name = "Off".to_string();
                    } else {
                        g.cc.wifi_name = "Wi-Fi".to_string();
                    }
                }
                CcAction::ToggleBt => {
                    g.cc.bt_on = !g.cc.bt_on;
                    g.cc.bt_sub = if g.cc.bt_on { "On" } else { "Off" }.to_string();
                }
                CcAction::ToggleAirplane => g.cc.air_on = !g.cc.air_on,
                CcAction::ToggleEnergySaver => g.cc.saver_on = !g.cc.saver_on,
                CcAction::ToggleCaptions => g.cc.cap_on = !g.cc.cap_on,
                CcAction::ToggleNightLight => g.cc.night_on = !g.cc.night_on,
                CcAction::ToggleHotspot => g.cc.hot_on = !g.cc.hot_on,
                CcAction::ToggleNearby => g.cc.near_on = !g.cc.near_on,
                CcAction::SetBrightness(b) => g.cc.bright_pct = (*b).min(100),
                CcAction::SetVolume(v) => {
                    g.cc.vol_pct = (*v).min(100);
                    g.cc.vol_muted = false;
                }
                CcAction::ToggleMute => g.cc.vol_muted = !g.cc.vol_muted,
                CcAction::SetAudioOutput(target_id) => {
                    for dev in &mut g.cc.audio_outputs {
                        dev.is_default = dev.id == *target_id;
                    }
                }
                CcAction::MediaPlayPause => g.media.is_playing = !g.media.is_playing,
                _ => {}
            }
        }
        let _ = self.tx.send(act);
        if let Ok(mut g) = self.action_wake.0.lock() {
            g.pending = true;
            self.action_wake.1.notify_one();
        }
    }

    fn signal_stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Ok(mut g) = self.stop_wake.0.lock() {
            *g = true;
            self.stop_wake.1.notify_all();
        }
        if let Ok(mut g) = self.action_wake.0.lock() {
            g.stop = true;
            self.action_wake.1.notify_all();
        }
    }

    /// Signals background media/audio workers to stop without blocking on `.join()`.
    /// Used on OS session shutdown (`WM_ENDSESSION`) so stuck WinRT/WASAPI COM calls
    /// never cause `AppHangTransient`.
    pub fn request_stop_no_block(&mut self) {
        self.signal_stop();
        let _ = self.media_thread.take();
        let _ = self.audio_thread.take();
    }

    pub fn shutdown(&mut self) {
        self.signal_stop();
        let deadline = Instant::now() + Duration::from_millis(250);
        for maybe_h in [self.media_thread.take(), self.audio_thread.take()] {
            if let Some(h) = maybe_h {
                while !h.is_finished() && Instant::now() < deadline {
                    std::thread::sleep(Duration::from_millis(10));
                }
                if h.is_finished() {
                    let _ = h.join();
                }
            }
        }
    }
}

impl Drop for SystemMediaEngine {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Gently defers non-essential WinRT/WASAPI worker initialization during the first 90s of Windows
/// boot so AutumnDeck never contends with Explorer, Audio Service, or logon startup tasks.
fn defer_on_fresh_boot(stop: &AtomicBool) {
    if unsafe { GetTickCount64() } >= 90_000 {
        return;
    }
    for _ in 0..15 {
        if stop.load(Ordering::Relaxed) {
            return;
        }
        thread::sleep(Duration::from_millis(100));
    }
}

fn run_smtc_and_cc_worker(
    shared: Arc<Mutex<SharedState>>,
    rx: Receiver<CcAction>,
    stop: Arc<AtomicBool>,
    action_wake: Arc<(Mutex<ActionSignal>, Condvar)>,
    cc_active: Arc<AtomicBool>,
    audio_outputs_active: Arc<AtomicBool>,
    audio_action_seq: Arc<AtomicU64>,
    hwnd_raw: isize,
    notify_msg: u32,
) {
    defer_on_fresh_boot(&stop);
    if stop.load(Ordering::Relaxed) {
        return;
    }
    unsafe {
        let _ = SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_BELOW_NORMAL);
        let _ = RoInitialize(RO_INIT_MULTITHREADED);
    }

    let manager = GlobalSystemMediaTransportControlsSessionManager::RequestAsync()
        .ok()
        .and_then(|op| op.join().ok());

    let mut cached_vol = CachedAudioVolume::new();
    let mut cached_sess_mgr: Option<IAudioSessionManager2> = None;
    let active_desktop_hwnd = Arc::new(AtomicIsize::new(0));
    let mut prefer_desktop = false;
    let mut prev_media_key = (String::new(), String::new(), false, 0u64);

    let mut last_cc_sync = Instant::now() - Duration::from_secs(20);
    let mut last_audio_enum = Instant::now() - Duration::from_secs(10);
    let mut last_media_sync = Instant::now() - Duration::from_secs(10);
    let mut cached_player_pids: HashMap<u32, DesktopPlayerKind> = HashMap::new();
    let mut cached_pid_classes: HashMap<u32, Option<DesktopPlayerKind>> = HashMap::new();
    let mut last_player_pid_scan = Instant::now() - Duration::from_secs(20);
    let mut cc_heavy_cache = CcHeavyCache::default();
    let mut last_thumb_key = String::new();
    let mut last_cover_seq = 0u64;
    let mut cached_cover: Option<Arc<Vec<u8>>> = None;
    let mut prev_cc: Option<CcSnapshot> = None;

    while !stop.load(Ordering::Relaxed) {
        // Drain queued commands: coalesce rapid brightness/volume drags, preserve relative
        // order of fast audio actions (SetAudioOutput / ToggleMute / SetVolume) so switching
        // devices and muting execute in <5ms without triggering a slow Control Center scan.
        let mut latest_bright: Option<u8> = None;
        let mut fast_audio_cmds: Vec<CcAction> = Vec::new();
        let mut other_cmds: Vec<CcAction> = Vec::new();
        while let Ok(cmd) = rx.try_recv() {
            match cmd {
                CcAction::SetBrightness(b) => latest_bright = Some(b),
                CcAction::SetVolume(v) => {
                    if let Some(CcAction::SetVolume(prev_v)) = fast_audio_cmds.last_mut() {
                        *prev_v = v;
                    } else {
                        fast_audio_cmds.push(CcAction::SetVolume(v));
                    }
                }
                CcAction::ToggleMute => fast_audio_cmds.push(CcAction::ToggleMute),
                CcAction::SetAudioOutput(id) => {
                    if let Some(CcAction::SetAudioOutput(prev_id)) = fast_audio_cmds.last_mut() {
                        *prev_id = id;
                    } else {
                        fast_audio_cmds.push(CcAction::SetAudioOutput(id));
                    }
                }
                other => other_cmds.push(other),
            }
        }

        let mut slider_dragged = false;
        let mut audio_fast_changed = false;
        if let Some(b) = latest_bright {
            set_lcd_brightness(b);
            if let Ok(mut g) = shared.lock() {
                g.cc.bright_pct = b.min(100);
            }
            slider_dragged = true;
        }
        if !fast_audio_cmds.is_empty() {
            let seq_at_drain = audio_action_seq.load(Ordering::Acquire);
            let mut switched_endpoint = false;
            for acmd in fast_audio_cmds {
                match acmd {
                    CcAction::SetVolume(v) => {
                        cached_vol.set_volume(v, Some(false));
                        slider_dragged = true;
                    }
                    CcAction::ToggleMute => {
                        cached_vol.toggle_mute();
                    }
                    CcAction::SetAudioOutput(target_id) => {
                        let _ = set_default_audio_endpoint(&target_id);
                        cached_vol.invalidate();
                        switched_endpoint = true;
                    }
                    _ => {}
                }
            }
            let (vol_pct, vol_muted, ep_changed) = cached_vol.read_volume();
            let refreshed_outputs = if switched_endpoint || ep_changed {
                last_audio_enum = Instant::now();
                Some(cached_vol.enumerate_outputs())
            } else {
                None
            };
            // Only overwrite shared audio state if no newer UI click arrived while COM ran
            if audio_action_seq.load(Ordering::Acquire) == seq_at_drain {
                if let Ok(mut g) = shared.lock() {
                    g.cc.vol_pct = vol_pct;
                    g.cc.vol_muted = vol_muted;
                    if let Some(ref outs) = refreshed_outputs {
                        g.cc.audio_outputs = outs.clone();
                    }
                }
                if let Some(ref mut prev) = prev_cc {
                    prev.vol_pct = vol_pct;
                    prev.vol_muted = vol_muted;
                    if let Some(outs) = refreshed_outputs {
                        prev.audio_outputs = outs;
                    }
                }
            }
            audio_fast_changed = true;
        }

        let mut had_action = false;
        let mut had_media_cmd = false;
        for cmd in other_cmds {
            if matches!(
                cmd,
                CcAction::MediaPrev | CcAction::MediaPlayPause | CcAction::MediaNext
            ) {
                had_media_cmd = true;
            }
            execute_single_action(cmd, manager.as_ref(), active_desktop_hwnd.load(Ordering::Relaxed));
            had_action = true;
        }
        if had_action {
            if had_media_cmd && active_desktop_hwnd.load(Ordering::Relaxed) != 0 {
                thread::sleep(Duration::from_millis(75));
            }
            last_cc_sync = Instant::now() - Duration::from_secs(10);
            last_media_sync = Instant::now() - Duration::from_secs(10);
        } else if slider_dragged {
            // Confirm hardware state calmly after dragging stops instead of blocking drag frames
            last_cc_sync = Instant::now() - Duration::from_millis(850);
        }
        if audio_fast_changed && hwnd_raw != 0 && notify_msg != 0 {
            unsafe {
                let _ = PostMessageW(
                    Some(HWND(hwnd_raw as *mut c_void)),
                    notify_msg,
                    WPARAM(0),
                    LPARAM(0),
                );
            }
        }

        // Poll SMTC + Desktop Media Players calmly (500ms while playing, 1000ms while idle)
        // so background media monitoring uses ~0% CPU instead of hammering Win32 snapshots.
        let media_poll_ms = if prev_media_key.2 || active_desktop_hwnd.load(Ordering::Relaxed) != 0 {
            500
        } else {
            1000
        };
        if last_media_sync.elapsed() >= Duration::from_millis(media_poll_ms) {
            last_media_sync = Instant::now();

            // 1. Read SMTC session state
            let mut smtc_has_session = false;
            let mut smtc_playing = false;
            let mut smtc_title = String::new();
            let mut smtc_artist = String::new();
            let mut smtc_props_opt = None;

            if let Some(ref mgr) = manager {
                if let Ok(sess) = mgr.GetCurrentSession() {
                    if let Ok(info) = sess.GetPlaybackInfo() {
                        if let Ok(st) = info.PlaybackStatus() {
                            smtc_playing =
                                st == GlobalSystemMediaTransportControlsSessionPlaybackStatus::Playing;
                        }
                    }
                    if let Ok(op) = sess.TryGetMediaPropertiesAsync() {
                        if let Ok(props) = op.join() {
                            let t = props.Title().map(|h| h.to_string_lossy()).unwrap_or_default();
                            let a = props.Artist().map(|h| h.to_string_lossy()).unwrap_or_default();
                            if !t.trim().is_empty() || !a.trim().is_empty() {
                                smtc_has_session = true;
                                smtc_title = t;
                                smtc_artist = a;
                                smtc_props_opt = Some(props);
                            }
                        }
                    }
                }
            }

            // 2. Read WASAPI active audio sessions & desktop media player windows
            // Skip full process snapshot while SMTC is already actively playing.
            let desk_opt = if smtc_playing {
                None
            } else {
                detect_desktop_player(
                    &mut cached_sess_mgr,
                    &mut cached_player_pids,
                    &mut cached_pid_classes,
                    &mut last_player_pid_scan,
                )
            };

            if smtc_playing {
                prefer_desktop = false;
            } else if let Some(ref desk) = desk_opt {
                if desk.is_playing {
                    prefer_desktop = true;
                }
            } else {
                prefer_desktop = false;
            }

            let use_desktop = !smtc_playing
                && desk_opt.as_ref().is_some_and(|d| d.is_playing || !smtc_has_session || prefer_desktop);

            let mut next_media = MediaSnapshot::default();
            if use_desktop {
                if let Some(desk) = desk_opt {
                    active_desktop_hwnd.store(desk.hwnd, Ordering::Relaxed);
                    next_media.title = desk.title;
                    next_media.artist = desk.artist;
                    next_media.is_playing = desk.is_playing;

                    let thumb_key = format!(
                        "DESK|{}|{}|{}",
                        desk.kind.exe_tag(),
                        next_media.title,
                        next_media.artist
                    );
                    if thumb_key != last_thumb_key {
                        last_thumb_key = thumb_key;
                        cached_cover = read_desktop_player_cover(
                            desk.kind,
                            &desk.raw_title,
                            &next_media.title,
                            &next_media.artist,
                        )
                        .map(Arc::new);
                        last_cover_seq += 1;
                    }
                }
            } else if smtc_has_session {
                active_desktop_hwnd.store(0, Ordering::Relaxed);
                next_media.is_playing = smtc_playing;
                if !smtc_title.trim().is_empty() {
                    next_media.title = smtc_title;
                }
                if !smtc_artist.trim().is_empty() {
                    next_media.artist = smtc_artist;
                }
                let thumb_key = format!("SMTC|{}|{}", next_media.title, next_media.artist);
                if thumb_key != last_thumb_key {
                    last_thumb_key = thumb_key;
                    cached_cover = smtc_props_opt
                        .as_ref()
                        .and_then(read_smtc_thumbnail)
                        .map(Arc::new);
                    last_cover_seq += 1;
                }
            } else {
                active_desktop_hwnd.store(0, Ordering::Relaxed);
                if last_thumb_key != "IDLE" {
                    last_thumb_key = "IDLE".to_string();
                    cached_cover = None;
                    last_cover_seq += 1;
                }
            }

            next_media.cover_bytes = cached_cover.clone();
            next_media.cover_seq = last_cover_seq;

            let cur_media_key = (
                next_media.title.clone(),
                next_media.artist.clone(),
                next_media.is_playing,
                next_media.cover_seq,
            );
            let media_changed = cur_media_key != prev_media_key;
            prev_media_key = cur_media_key;

            if let Ok(mut g) = shared.lock() {
                g.media = next_media;
            }
            if (had_action || media_changed) && hwnd_raw != 0 && notify_msg != 0 {
                unsafe {
                    let _ = PostMessageW(Some(HWND(hwnd_raw as *mut c_void)), notify_msg, WPARAM(0), LPARAM(0));
                }
            }
        }

        // Poll Control Center hardware/OS state calmly (6s when CC visible, 12s when covered,
        // or immediately on any user toggle), plus ultra-lightweight cached COM volume/mute check
        // on each wake (~450ms, <15us with zero endpoint re-enumeration RPCs).
        let want_outputs = audio_outputs_active.load(Ordering::Relaxed);
        let cc_vis = cc_active.load(Ordering::Relaxed);
        let cc_interval_ms = if cc_vis { 6000 } else { 12_000 };
        if !stop.load(Ordering::Relaxed)
            && (had_action
                || prev_cc.is_none()
                || last_cc_sync.elapsed() >= Duration::from_millis(cc_interval_ms))
        {
            let seq_before = audio_action_seq.load(Ordering::Acquire);
            last_cc_sync = Instant::now();
            let mut cc = if had_action || cc_vis {
                collect_cc_snapshot(&mut cached_vol, want_outputs, &mut cc_heavy_cache, had_action)
            } else {
                let seed = prev_cc.as_ref().cloned().unwrap_or_default();
                collect_cc_snapshot_light(&seed, &mut cached_vol, want_outputs)
            };
            if want_outputs {
                last_audio_enum = Instant::now();
            }
            if stop.load(Ordering::Relaxed) {
                break;
            }
            let cc_changed = {
                if let Ok(mut g) = shared.lock() {
                    // Never clobber newer optimistic volume/mute/endpoint clicks with a stale snapshot
                    if audio_action_seq.load(Ordering::Acquire) != seq_before {
                        cc.vol_pct = g.cc.vol_pct;
                        cc.vol_muted = g.cc.vol_muted;
                        cc.audio_outputs = g.cc.audio_outputs.clone();
                    }
                    let changed = prev_cc.as_ref() != Some(&cc);
                    g.cc = cc.clone();
                    if changed {
                        prev_cc = Some(cc);
                    }
                    changed
                } else {
                    false
                }
            };
            if (had_action || cc_changed) && hwnd_raw != 0 && notify_msg != 0 {
                unsafe {
                    let _ = PostMessageW(Some(HWND(hwnd_raw as *mut c_void)), notify_msg, WPARAM(0), LPARAM(0));
                }
            }
        } else if !stop.load(Ordering::Relaxed) {
            if let Some(ref mut prev) = prev_cc {
                let seq_before = audio_action_seq.load(Ordering::Acquire);
                let (vol_pct, vol_muted, ep_changed) = cached_vol.read_volume();
                let need_enum = want_outputs
                    && (ep_changed
                        || prev.audio_outputs.is_empty()
                        || last_audio_enum.elapsed() >= Duration::from_millis(2000));
                let new_outputs = if need_enum {
                    last_audio_enum = Instant::now();
                    Some(cached_vol.enumerate_outputs())
                } else {
                    None
                };
                if audio_action_seq.load(Ordering::Acquire) == seq_before {
                    let outputs_changed = new_outputs
                        .as_ref()
                        .is_some_and(|outs| *outs != prev.audio_outputs);
                    if prev.vol_pct != vol_pct || prev.vol_muted != vol_muted || outputs_changed {
                        prev.vol_pct = vol_pct;
                        prev.vol_muted = vol_muted;
                        if let Some(outs) = new_outputs {
                            prev.audio_outputs = outs;
                        }
                        if let Ok(mut g) = shared.lock() {
                            g.cc.vol_pct = prev.vol_pct;
                            g.cc.vol_muted = prev.vol_muted;
                            if outputs_changed {
                                g.cc.audio_outputs = prev.audio_outputs.clone();
                            }
                        }
                        if hwnd_raw != 0 && notify_msg != 0 {
                            unsafe {
                                let _ = PostMessageW(
                                    Some(HWND(hwnd_raw as *mut c_void)),
                                    notify_msg,
                                    WPARAM(0),
                                    LPARAM(0),
                                );
                            }
                        }
                    }
                }
            }
        }

        if !wait_for_action_or_timeout(&action_wake, Duration::from_millis(450)) {
            break;
        }
    }

    unsafe {
        RoUninitialize();
    }
}

fn read_smtc_thumbnail(
    props: &windows::Media::Control::GlobalSystemMediaTransportControlsSessionMediaProperties,
) -> Option<Vec<u8>> {
    let thumb_ref = props.Thumbnail().ok()?;
    let stream = thumb_ref.OpenReadAsync().ok()?.join().ok()?;
    let size = stream.Size().ok()?;
    let sz = size.min(4 * 1024 * 1024) as u32;
    if sz == 0 {
        return None;
    }
    let reader = DataReader::CreateDataReader(&stream).ok()?;
    let loaded = reader.LoadAsync(sz).ok()?.join().ok()?;
    if loaded == 0 {
        return None;
    }
    let mut buf = vec![0u8; loaded as usize];
    if reader.ReadBytes(&mut buf).is_ok() {
        Some(buf)
    } else {
        None
    }
}

fn send_desktop_app_command(hwnd_raw: isize, app_cmd: isize) -> bool {
    if hwnd_raw == 0 {
        return false;
    }
    const WM_APPCOMMAND_MSG: u32 = 0x0319;
    unsafe {
        let hwnd = HWND(hwnd_raw as *mut c_void);
        if !IsWindow(Some(hwnd)).as_bool() {
            return false;
        }
        let mut res = 0usize;
        let ok = SendMessageTimeoutW(
            hwnd,
            WM_APPCOMMAND_MSG,
            WPARAM(hwnd_raw as usize),
            LPARAM(app_cmd << 16),
            SMTO_ABORTIFHUNG,
            120,
            Some(&mut res),
        );
        ok.0 != 0
    }
}

fn execute_single_action(
    act: CcAction,
    manager: Option<&GlobalSystemMediaTransportControlsSessionManager>,
    desktop_hwnd: isize,
) {
    const APPCOMMAND_MEDIA_NEXTTRACK: isize = 11;
    const APPCOMMAND_MEDIA_PREVIOUSTRACK: isize = 12;
    const APPCOMMAND_MEDIA_PLAY_PAUSE: isize = 14;

    match act {
        CcAction::MediaPrev => {
            if desktop_hwnd != 0 && send_desktop_app_command(desktop_hwnd, APPCOMMAND_MEDIA_PREVIOUSTRACK) {
                return;
            }
            let mut ok = false;
            if let Some(m) = manager {
                if let Ok(s) = m.GetCurrentSession() {
                    if let Ok(op) = s.TrySkipPreviousAsync() {
                        ok = op.join().unwrap_or(false);
                    }
                }
            }
            if !ok {
                send_vk(VK_MEDIA_PREV_TRACK.0 as u8);
            }
        }
        CcAction::MediaPlayPause => {
            if desktop_hwnd != 0 && send_desktop_app_command(desktop_hwnd, APPCOMMAND_MEDIA_PLAY_PAUSE) {
                return;
            }
            let mut ok = false;
            if let Some(m) = manager {
                if let Ok(s) = m.GetCurrentSession() {
                    if let Ok(op) = s.TryTogglePlayPauseAsync() {
                        ok = op.join().unwrap_or(false);
                    }
                }
            }
            if !ok {
                send_vk(VK_MEDIA_PLAY_PAUSE.0 as u8);
            }
        }
        CcAction::MediaNext => {
            if desktop_hwnd != 0 && send_desktop_app_command(desktop_hwnd, APPCOMMAND_MEDIA_NEXTTRACK) {
                return;
            }
            let mut ok = false;
            if let Some(m) = manager {
                if let Ok(s) = m.GetCurrentSession() {
                    if let Ok(op) = s.TrySkipNextAsync() {
                        ok = op.join().unwrap_or(false);
                    }
                }
            }
            if !ok {
                send_vk(VK_MEDIA_NEXT_TRACK.0 as u8);
            }
        }
        CcAction::ToggleWifi => {
            if !toggle_radio(RadioKind::WiFi) {
                let cur = read_wifi_state().0;
                let _ = set_wifi_radio_wlan(!cur);
            }
        }
        CcAction::OpenWifiList => {
            thread::spawn(|| {
                if !open_uri_ok("ms-availablenetworks:")
                    && !invoke_quick_settings_tile(
                        &[
                            "Manage Wi-Fi connections",
                            "Manage WLAN connections",
                            "Manage Wi-Fi",
                        ],
                        true,
                    )
                {
                    open_uri("ms-settings:network-wifi");
                }
            });
        }
        CcAction::ToggleBt => {
            if !toggle_radio(RadioKind::Bluetooth) {
                open_uri("ms-settings:bluetooth");
            }
        }
        CcAction::OpenBtList => {
            thread::spawn(|| {
                if !invoke_quick_settings_tile(
                    &[
                        "Manage Bluetooth devices",
                        "Show Bluetooth devices",
                        "Manage Bluetooth",
                        "Bluetooth devices",
                    ],
                    true,
                ) {
                    open_uri("ms-settings:bluetooth");
                }
            });
        }
        CcAction::ToggleAirplane => {
            if !try_toggle_system_setting_bool(
                "NetworkMobileSettings.dll",
                "SystemSettings_Radio_IsAirplaneModeEnabled_SettingsApp",
                100,
                300,
            ) {
                open_uri("ms-settings:network-airplanemode");
            }
        }
        CcAction::OpenAccessibility => {
            thread::spawn(|| {
                if !invoke_quick_settings_tile(&["Accessibility", "QA_Accessibility"], false) {
                    open_uri("ms-settings:easeofaccess");
                }
            });
        }
        CcAction::ToggleEnergySaver => {
            if !try_toggle_system_setting_bool(
                "SettingsHandlers_OneCore_BatterySaver.dll",
                "SystemSettings_BatterySaver_LandingPage_OverrideControl",
                60,
                200,
            ) {
                open_uri("ms-settings:batterysaver");
            }
        }
        CcAction::ToggleCaptions => toggle_live_captions(),
        CcAction::ToggleNightLight => {
            let before = read_night_light();
            if !try_toggle_system_setting_bool(
                "SettingsHandlers_Display.dll",
                "SystemSettings_Display_BlueLight_ManualToggleQuickAction",
                120,
                250,
            ) || read_night_light() == before
            {
                toggle_night_light();
            }
        }
        CcAction::ToggleHotspot => {
            if !toggle_hotspot()
                && !try_toggle_system_setting_bool(
                    "NetworkMobileSettings.dll",
                    "SystemSettings_Connections_InternetSharingEnabled",
                    150,
                    400,
                )
            {
                open_uri("ms-settings:network-mobilehotspot");
            }
        }
        CcAction::ToggleNearby => {
            if !try_toggle_system_setting_bool(
                "SettingsHandlers_SharedExperiences_Rome.dll",
                "SystemSettings_SharedExperiences_NearShareQuickAction",
                100,
                250,
            ) {
                toggle_nearby_sharing();
            }
        }
        CcAction::OpenCast => {
            if !open_uri_ok("ms-settings-connectabledevices:devicediscovery") {
                send_win_chord_ext(VK_K.0);
            }
        }
        CcAction::OpenProject => {
            if !open_uri_ok("ms-settings-displays-topology:projection") {
                unsafe {
                    let _ = ShellExecuteW(
                        None,
                        w!("open"),
                        w!("DisplaySwitch.exe"),
                        PCWSTR::null(),
                        PCWSTR::null(),
                        SW_SHOWNORMAL,
                    );
                }
            }
        }
        CcAction::ToggleMute => {
            let (cur, muted) = get_master_volume();
            set_master_volume(cur, Some(!muted));
        }
        CcAction::OpenTileSettings(idx) => {
            let uri = match idx {
                0 => "ms-settings:network-wifi",
                1 => "ms-settings:bluetooth",
                2 => "ms-settings:network-airplanemode",
                3 => "ms-settings:easeofaccess",
                4 => "ms-settings:batterysaver",
                5 => "ms-settings:easeofaccess-closedcaptioning",
                6 => "ms-settings:nightlight",
                7 => "ms-settings:network-mobilehotspot",
                8 => "ms-settings:crossdevice",
                _ => "ms-settings:display",
            };
            open_uri(uri);
        }
        _ => {}
    }
}

fn send_vk(vk: u8) {
    unsafe {
        keybd_event(vk, 0, Default::default(), 0);
        keybd_event(vk, 0, KEYEVENTF_KEYUP, 0);
    }
}

fn send_win_chord_ext(vk: u16) {
    let mk = |key: u16, flags: KEYBD_EVENT_FLAGS| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(key),
                wScan: 0,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    let seq = [
        mk(VK_LWIN.0, KEYEVENTF_EXTENDEDKEY),
        mk(vk, KEYBD_EVENT_FLAGS(0)),
        mk(vk, KEYEVENTF_KEYUP),
        mk(VK_LWIN.0, KEYEVENTF_KEYUP | KEYEVENTF_EXTENDEDKEY),
    ];
    unsafe {
        let sent = SendInput(&seq, size_of::<INPUT>() as i32);
        if sent == 0 {
            keybd_event(VK_LWIN.0 as u8, 0, Default::default(), 0);
            keybd_event(vk as u8, 0, Default::default(), 0);
            keybd_event(vk as u8, 0, KEYEVENTF_KEYUP, 0);
            keybd_event(VK_LWIN.0 as u8, 0, KEYEVENTF_KEYUP, 0);
        }
    }
}

fn open_uri_ok(uri: &str) -> bool {
    let w = to_wide(uri);
    unsafe {
        let res = ShellExecuteW(
            None,
            w!("open"),
            PCWSTR(w.as_ptr()),
            PCWSTR::null(),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        );
        res.0 as isize > 32
    }
}

fn open_uri(uri: &str) {
    let _ = open_uri_ok(uri);
}

fn toggle_radio(kind: RadioKind) -> bool {
    let Ok(op_acc) = Radio::RequestAccessAsync() else {
        return false;
    };
    let _ = op_acc.join();
    let Ok(op_rad) = Radio::GetRadiosAsync() else {
        return false;
    };
    let Ok(radios) = op_rad.join() else {
        return false;
    };
    for r in radios {
        if r.Kind().ok() == Some(kind) {
            let cur = r.State().ok() == Some(RadioState::On);
            let next = if cur { RadioState::Off } else { RadioState::On };
            if let Ok(op) = r.SetStateAsync(next) {
                return op.join().ok() == Some(RadioAccessStatus::Allowed);
            }
        }
    }
    false
}

fn set_wifi_radio_wlan(enable: bool) -> bool {
    unsafe {
        let mut neg = 0u32;
        let mut client = windows::Win32::Foundation::HANDLE::default();
        if WlanOpenHandle(2, None, &mut neg, &mut client) != 0 {
            return false;
        }
        let mut list_ptr: *mut WLAN_INTERFACE_INFO_LIST = null_mut();
        let mut changed = false;
        if WlanEnumInterfaces(client, None, &mut list_ptr) == 0 && !list_ptr.is_null() {
            let cnt = (*list_ptr).dwNumberOfItems as usize;
            let base = (*list_ptr).InterfaceInfo.as_ptr();
            for i in 0..cnt {
                let info = &*base.add(i);
                let state = WLAN_PHY_RADIO_STATE {
                    dwPhyIndex: 0,
                    dot11SoftwareRadioState: if enable { dot11_radio_state_on } else { dot11_radio_state_off },
                    dot11HardwareRadioState: dot11_radio_state_on,
                };
                if WlanSetInterface(
                    client,
                    &info.InterfaceGuid,
                    windows::Win32::NetworkManagement::WiFi::wlan_intf_opcode_radio_state,
                    size_of::<WLAN_PHY_RADIO_STATE>() as u32,
                    &state as *const _ as *const c_void,
                    None,
                ) == 0
                {
                    changed = true;
                }
            }
            WlanFreeMemory(list_ptr as *mut c_void);
        }
        let _ = WlanCloseHandle(client, None);
        changed
    }
}

fn read_radios_flags() -> (bool, bool) {
    let mut wifi_on = false;
    let mut bt_on = false;
    if let Ok(op) = Radio::GetRadiosAsync() {
        if let Ok(list) = op.join() {
            for r in list {
                match r.Kind().ok() {
                    Some(RadioKind::WiFi) => wifi_on = r.State().ok() == Some(RadioState::On),
                    Some(RadioKind::Bluetooth) => bt_on = r.State().ok() == Some(RadioState::On),
                    _ => {}
                }
            }
        }
    }
    (wifi_on, bt_on)
}

fn read_wifi_state() -> (bool, String) {
    let (mut radio_on, _) = read_radios_flags();
    let (connected, wlan_label, _) = read_wifi_wlan_fast();
    if connected {
        radio_on = true;
    }
    let label = if !radio_on {
        "Off".to_string()
    } else if connected {
        wlan_label
    } else {
        "Disconnected".to_string()
    };
    (radio_on, label)
}

fn read_bt_connected_sub() -> String {
    let key_conn = DEVPROPKEY {
        fmtid: GUID::from_u128(0x83da6326_97a6_4088_9453_a1923f573b29),
        pid: 15,
    };
    let key_name = DEVPROPKEY {
        fmtid: GUID::from_u128(0xa45c254e_df1c_4efd_8020_67d146a850e0),
        pid: 14,
    };

    let mut names = Vec::new();
    let mut seen = HashSet::new();
    unsafe {
        for enum_str in [w!("BTHENUM"), w!("BTHLE")] {
            let Ok(hdev) = SetupDiGetClassDevsW(None, enum_str, None, DIGCF_ALLCLASSES | DIGCF_PRESENT) else {
                continue;
            };
            let mut did = SP_DEVINFO_DATA {
                cbSize: size_of::<SP_DEVINFO_DATA>() as u32,
                ..Default::default()
            };
            let mut idx = 0u32;
            let mut id_buf = [0u16; 260];
            let mut flag_buf = [0u8; 4];
            let mut name_buf = [0u8; 256];
            while SetupDiEnumDeviceInfo(hdev, idx, &mut did).is_ok() {
                idx += 1;
                if SetupDiGetDeviceInstanceIdW(hdev, &did, Some(&mut id_buf), None).is_ok() {
                    let end = id_buf.iter().position(|&c| c == 0).unwrap_or(id_buf.len());
                    let iid = String::from_utf16_lossy(&id_buf[..end]).to_ascii_uppercase();
                    if !iid.starts_with("BTHENUM\\DEV_") && !iid.starts_with("BTHLE\\DEV_") {
                        continue;
                    }
                    let mut ptype = DEVPROPTYPE(0);
                    let mut req = 0u32;
                    flag_buf[0] = 0;
                    if SetupDiGetDevicePropertyW(
                        hdev,
                        &did,
                        &key_conn,
                        &mut ptype,
                        Some(&mut flag_buf[..1]),
                        Some(&mut req),
                        0,
                    )
                    .is_ok()
                        && flag_buf[0] != 0
                    {
                        let mut dev_name = "Connected".to_string();
                        if SetupDiGetDevicePropertyW(
                            hdev,
                            &did,
                            &key_name,
                            &mut ptype,
                            Some(&mut name_buf),
                            Some(&mut req),
                            0,
                        )
                        .is_ok()
                            && req >= 2
                        {
                            let u16_len = (req as usize / 2).min(name_buf.len() / 2);
                            let slice = std::slice::from_raw_parts(name_buf.as_ptr() as *const u16, u16_len);
                            let raw_n = String::from_utf16_lossy(slice).trim_end_matches('\0').trim().to_string();
                            if !raw_n.is_empty() {
                                dev_name = raw_n;
                            }
                        }
                        if seen.insert(dev_name.to_ascii_lowercase()) {
                            names.push(dev_name);
                        }
                    }
                }
            }
            let _ = SetupDiDestroyDeviceInfoList(hdev);
        }
    }
    if names.len() == 1 {
        names[0].clone()
    } else if names.len() > 1 {
        format!("Connected ({})", names.len())
    } else {
        "Not connected".to_string()
    }
}

fn read_reg_dword(root: HKEY, subkey: PCWSTR, name: PCWSTR) -> Option<u32> {
    unsafe {
        let mut hk = HKEY::default();
        if RegOpenKeyExW(root, subkey, None, KEY_READ, &mut hk).is_err() {
            return None;
        }
        let mut val = 0u32;
        let mut sz = size_of::<u32>() as u32;
        let ok = RegQueryValueExW(hk, name, None, None, Some(&mut val as *mut u32 as *mut u8), Some(&mut sz)).is_ok();
        let _ = RegCloseKey(hk);
        if ok { Some(val) } else { None }
    }
}

fn scan_process_flags() -> (bool, bool) {
    let mut captions = false;
    let mut acc = false;
    unsafe {
        if let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) {
            let mut pe = PROCESSENTRY32W {
                dwSize: size_of::<PROCESSENTRY32W>() as u32,
                ..Default::default()
            };
            if Process32FirstW(snap, &mut pe).is_ok() {
                loop {
                    let end = pe.szExeFile.iter().position(|&c| c == 0).unwrap_or(pe.szExeFile.len());
                    let exe = String::from_utf16_lossy(&pe.szExeFile[..end]).to_ascii_lowercase();
                    if exe == "livecaptions.exe" {
                        captions = true;
                    } else if matches!(exe.as_str(), "narrator.exe" | "magnify.exe" | "osk.exe" | "voiceaccess.exe") {
                        acc = true;
                    }
                    if Process32NextW(snap, &mut pe).is_err() {
                        break;
                    }
                }
            }
            let _ = CloseHandle(snap);
        }
    }
    (captions, acc)
}

const NIGHT_LIGHT_KEY: PCWSTR = w!(
    r"Software\Microsoft\Windows\CurrentVersion\CloudStore\Store\DefaultAccount\Current\default$windows.data.bluelightreduction.bluelightreductionstate\windows.data.bluelightreduction.bluelightreductionstate"
);

fn read_night_light() -> bool {
    unsafe {
        let mut hk = HKEY::default();
        if RegOpenKeyExW(HKEY_CURRENT_USER, NIGHT_LIGHT_KEY, None, KEY_READ, &mut hk).is_err() {
            return false;
        }
        let mut buf = vec![0u8; 128];
        let mut sz = buf.len() as u32;
        let ok = RegQueryValueExW(hk, w!("Data"), None, None, Some(buf.as_mut_ptr()), Some(&mut sz)).is_ok();
        let _ = RegCloseKey(hk);
        ok && sz > 18 && buf[18] == 0x15
    }
}

fn toggle_night_light() {
    unsafe {
        let mut hk = HKEY::default();
        if RegOpenKeyExW(HKEY_CURRENT_USER, NIGHT_LIGHT_KEY, None, KEY_READ | KEY_WRITE, &mut hk).is_err() {
            open_uri("ms-settings:nightlight");
            return;
        }
        let mut sz = 0u32;
        if RegQueryValueExW(hk, w!("Data"), None, None, None, Some(&mut sz)).is_err() || sz < 25 {
            let _ = RegCloseKey(hk);
            open_uri("ms-settings:nightlight");
            return;
        }
        let mut data = vec![0u8; sz as usize];
        if RegQueryValueExW(hk, w!("Data"), None, None, Some(data.as_mut_ptr()), Some(&mut sz)).is_err() {
            let _ = RegCloseKey(hk);
            return;
        }
        let is_on = data.get(18).copied() == Some(0x15);
        let mut next = Vec::with_capacity(data.len() + 4);
        if is_on {
            for (i, &b) in data.iter().enumerate() {
                if i == 18 {
                    next.push(0x13);
                } else if i == 23 || i == 24 {
                    continue;
                } else {
                    next.push(b);
                }
            }
        } else {
            for (i, &b) in data.iter().enumerate() {
                if i == 18 {
                    next.push(0x15);
                } else if i == 23 {
                    next.push(0x10);
                    next.push(0x00);
                    next.push(b);
                } else {
                    next.push(b);
                }
            }
        }
        for idx in 10..15.min(next.len()) {
            if next[idx] != 0xff {
                next[idx] = next[idx].wrapping_add(1);
                break;
            }
        }
        let _ = RegSetValueExW(hk, w!("Data"), None, REG_BINARY, Some(&next));
        let _ = RegCloseKey(hk);
    }
}

fn toggle_nearby_sharing() {
    unsafe {
        let mut hk = HKEY::default();
        if RegOpenKeyExW(
            HKEY_CURRENT_USER,
            w!(r"Software\Microsoft\Windows\CurrentVersion\CDP"),
            None,
            KEY_READ | KEY_WRITE,
            &mut hk,
        )
        .is_ok()
        {
            let cur = read_reg_dword(
                HKEY_CURRENT_USER,
                w!(r"Software\Microsoft\Windows\CurrentVersion\CDP"),
                w!("NearShareChannelUserAuthzPolicy"),
            )
            .unwrap_or(0);
            let next: u32 = if cur > 0 { 0 } else { 2 };
            let bytes = next.to_le_bytes();
            let _ = RegSetValueExW(hk, w!("NearShareChannelUserAuthzPolicy"), None, REG_DWORD, Some(&bytes));
            let _ = RegSetValueExW(hk, w!("CdpSessionUserAuthzPolicy"), None, REG_DWORD, Some(&bytes));
            let _ = RegCloseKey(hk);
        } else {
            open_uri("ms-settings:crossdevice");
        }
    }
}

fn toggle_live_captions() {
    let (cap_on, _) = scan_process_flags();
    if cap_on {
        if !try_toggle_system_setting_bool(
            "SettingsHandlers_nt.dll",
            "SystemSettings_Accessibility_Caption_LiveCaptionsQuickAction",
            50,
            150,
        ) {
            open_uri("ms-settings:easeofaccess-closedcaptioning");
        }
    } else {
        unsafe {
            let res = ShellExecuteW(
                None,
                w!("open"),
                w!("LiveCaptions.exe"),
                PCWSTR::null(),
                PCWSTR::null(),
                SW_SHOWNORMAL,
            );
            if res.0 as isize <= 32 {
                let _ = try_toggle_system_setting_bool(
                    "SettingsHandlers_nt.dll",
                    "SystemSettings_Accessibility_Caption_LiveCaptionsQuickAction",
                    50,
                    150,
                );
            }
        }
    }
}

fn read_hotspot_state() -> bool {
    if let Ok(prof) = NetworkInformation::GetInternetConnectionProfile() {
        if let Ok(tm) = NetworkOperatorTetheringManager::CreateFromConnectionProfile(&prof) {
            return matches!(
                tm.TetheringOperationalState().ok(),
                Some(TetheringOperationalState::On) | Some(TetheringOperationalState::InTransition)
            );
        }
    }
    false
}

fn toggle_hotspot() -> bool {
    if let Ok(prof) = NetworkInformation::GetInternetConnectionProfile() {
        if let Ok(tm) = NetworkOperatorTetheringManager::CreateFromConnectionProfile(&prof) {
            let st = tm.TetheringOperationalState().ok();
            let op = if st == Some(TetheringOperationalState::On)
                || st == Some(TetheringOperationalState::InTransition)
            {
                tm.StopTetheringAsync()
            } else {
                tm.StartTetheringAsync()
            };
            if let Ok(res_op) = op {
                if let Ok(res) = res_op.join() {
                    return res.Status().ok() == Some(TetheringOperationStatus::Success);
                }
            }
        }
    }
    false
}

fn get_lcd_brightness() -> u8 {
    const IOCTL_VIDEO_QUERY_DISPLAY_BRIGHTNESS: u32 = 0x230498;
    unsafe {
        if let Ok(h) = CreateFileW(
            w!(r"\\.\LCD"),
            0x8000_0000,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            None,
            OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL,
            None,
        ) {
            let mut buf = [0u8; 3];
            let mut ret = 0u32;
            let ok = DeviceIoControl(
                h,
                IOCTL_VIDEO_QUERY_DISPLAY_BRIGHTNESS,
                None,
                0,
                Some(buf.as_mut_ptr() as *mut c_void),
                3,
                Some(&mut ret),
                None,
            )
            .is_ok();
            let _ = CloseHandle(h);
            if ok && ret >= 3 {
                let pct = if buf[0] == 1 { buf[1] } else { buf[2] };
                return pct.min(100);
            }
        }
    }
    65
}

fn set_lcd_brightness(pct: u8) {
    const IOCTL_VIDEO_SET_DISPLAY_BRIGHTNESS: u32 = 0x23049C;
    let clamped = pct.min(100);
    unsafe {
        if let Ok(h) = CreateFileW(
            w!(r"\\.\LCD"),
            0xC000_0000,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            None,
            OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL,
            None,
        ) {
            let buf = [3u8, clamped, clamped];
            let mut ret = 0u32;
            let _ = DeviceIoControl(
                h,
                IOCTL_VIDEO_SET_DISPLAY_BRIGHTNESS,
                Some(buf.as_ptr() as *const c_void),
                3,
                None,
                0,
                Some(&mut ret),
                None,
            );
            let _ = CloseHandle(h);
        }
    }
}

pub fn shorten_audio_device_name(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return "Audio Output".to_string();
    }
    let Some(open) = trimmed.find('(') else {
        return trimmed.to_string();
    };
    if !trimmed.ends_with(')') || open == 0 {
        return trimmed.to_string();
    }
    let prefix = trimmed[..open].trim();
    let mut inner = trimmed[open + 1..trimmed.len() - 1].trim();
    // Strip Windows duplicate-device numeric prefix like "2- " or "3- "
    if let Some(dash) = inner.find('-') {
        if dash > 0 && dash <= 3 && inner[..dash].chars().all(|c| c.is_ascii_digit()) {
            inner = inner[dash + 1..].trim();
        }
    }
    let pref_low = prefix.to_ascii_lowercase();
    let inner_low = inner.to_ascii_lowercase();

    if matches!(
        pref_low.as_str(),
        "headphones" | "headset" | "earphones" | "earbuds" | "headset earphone"
    ) {
        if !inner.is_empty() {
            let is_hf = inner_low.contains("hands-free");
            let mut clean = inner.to_string();
            for suffix in [
                " Hands-Free AG Audio",
                " Hands-Free Audio",
                " Hands-Free",
                " Stereo",
                " High Definition Audio",
                " Audio",
            ] {
                if clean.len() > suffix.len()
                    && clean[clean.len() - suffix.len()..].eq_ignore_ascii_case(suffix)
                {
                    clean.truncate(clean.len() - suffix.len());
                    clean = clean.trim().to_string();
                }
            }
            if !clean.is_empty() {
                return if is_hf {
                    format!("{} (HF)", clean)
                } else {
                    clean
                };
            }
        }
        return prefix.to_string();
    }

    if pref_low == "speakers" || pref_low == "speaker" {
        let onboard = inner_low.is_empty()
            || inner_low.contains("realtek")
            || inner_low.contains("high definition audio")
            || inner_low.contains("intel")
            || inner_low.contains("conexant")
            || inner_low.contains("synaptics")
            || inner_low.contains("senary")
            || inner_low.contains("sst");
        if onboard {
            return "Speakers".to_string();
        }
        return inner.to_string();
    }

    if pref_low.contains("digital output") || pref_low.contains("spdif") {
        return "Digital Output".to_string();
    }

    prefix.to_string()
}

fn read_mmdevice_id(dev: &windows::Win32::Media::Audio::IMMDevice) -> Option<String> {
    unsafe {
        let pw = dev.GetId().ok()?;
        let s = pw.to_string().ok();
        CoTaskMemFree(Some(pw.0 as *const c_void));
        s.filter(|v| !v.is_empty())
    }
}

fn read_mmdevice_friendly_name(dev: &windows::Win32::Media::Audio::IMMDevice) -> String {
    const PKEY_DEVICE_FRIENDLY_NAME: PROPERTYKEY = PROPERTYKEY {
        fmtid: GUID::from_u128(0xa45c254e_df1c_4efd_8020_67d146a850e0),
        pid: 14,
    };
    unsafe {
        let Ok(store): windows::core::Result<IPropertyStore> = dev.OpenPropertyStore(STGM_READ) else {
            return "Audio Output".to_string();
        };
        let Ok(mut pv) = store.GetValue(&PKEY_DEVICE_FRIENDLY_NAME) else {
            return "Audio Output".to_string();
        };
        let out = if let Ok(pw) = PropVariantToStringAlloc(&pv) {
            let s = pw.to_string().unwrap_or_default();
            CoTaskMemFree(Some(pw.0 as *const c_void));
            s
        } else {
            String::new()
        };
        let _ = PropVariantClear(&mut pv);
        if out.trim().is_empty() {
            "Audio Output".to_string()
        } else {
            out
        }
    }
}

/// Switches the Windows default audio playback endpoint across Console, Multimedia, and
/// Communications roles using the standard `IPolicyConfig` COM interface.
pub fn set_default_audio_endpoint(device_id: &str) -> bool {
    if device_id.trim().is_empty() {
        return false;
    }
    const CLSID_POLICY_CONFIG_CLIENT: GUID =
        GUID::from_u128(0x870af99c_171d_4f9e_af0d_e63df40c2bc9);
    const IID_POLICY_CONFIG: GUID = GUID::from_u128(0xf8679f50_850a_41cf_9c72_430f290290c8);
    const IID_POLICY_CONFIG_VISTA: GUID =
        GUID::from_u128(0x568b9108_44bf_40b4_9006_86afe5b5a620);

    type SetDefaultEndpointFn =
        unsafe extern "system" fn(*mut c_void, PCWSTR, ERole) -> HRESULT;

    unsafe {
        let mut init_ok = false;
        if CoInitializeEx(None, COINIT_MULTITHREADED).is_ok() {
            init_ok = true;
        }
        let w_id = to_wide(device_id);
        let ok = (|| -> bool {
            let Ok(unk): windows::core::Result<windows::core::IUnknown> =
                CoCreateInstance(&CLSID_POLICY_CONFIG_CLIENT, None, CLSCTX_ALL)
            else {
                return false;
            };
            let mut p_iface: *mut c_void = null_mut();
            let vtbl_slot = if unk.query(&IID_POLICY_CONFIG, &mut p_iface).is_ok()
                && !p_iface.is_null()
            {
                13usize
            } else if unk.query(&IID_POLICY_CONFIG_VISTA, &mut p_iface).is_ok()
                && !p_iface.is_null()
            {
                12usize
            } else {
                return false;
            };
            // Keep RAII ownership of the queried COM interface pointer so Release runs on exit.
            let _guard = windows::core::IUnknown::from_raw(p_iface);
            let vt = *(p_iface as *const *const usize);
            let set_def: SetDefaultEndpointFn = std::mem::transmute(*vt.add(vtbl_slot));
            let r1 = set_def(p_iface, PCWSTR(w_id.as_ptr()), eConsole);
            let r2 = set_def(p_iface, PCWSTR(w_id.as_ptr()), eMultimedia);
            let r3 = set_def(p_iface, PCWSTR(w_id.as_ptr()), eCommunications);
            r1.is_ok() || r2.is_ok() || r3.is_ok()
        })();
        if init_ok {
            CoUninitialize();
        }
        ok
    }
}

/// Fast worker-local CoreAudio cache that tracks the active default render endpoint ID so
/// volume reads, volume writes, and mute toggles NEVER target a stale device after switching
/// between Speakers and Bluetooth headphones.
struct CachedAudioVolume {
    enumerator: Option<IMMDeviceEnumerator>,
    default_id: String,
    ep: Option<IAudioEndpointVolume>,
    last_ep_verify: Instant,
}

impl CachedAudioVolume {
    fn new() -> Self {
        Self {
            enumerator: None,
            default_id: String::new(),
            ep: None,
            last_ep_verify: Instant::now() - Duration::from_secs(10),
        }
    }

    fn invalidate(&mut self) {
        self.default_id.clear();
        self.ep = None;
        self.last_ep_verify = Instant::now() - Duration::from_secs(10);
    }

    fn ensure_enumerator(&mut self) -> Option<&IMMDeviceEnumerator> {
        if self.enumerator.is_none() {
            self.enumerator = unsafe {
                CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_INPROC_SERVER).ok()
            };
        }
        self.enumerator.as_ref()
    }

    /// Ensures `self.ep` points to the CURRENT default multimedia render endpoint.
    /// Reuses the live `IAudioEndpointVolume` handle directly between 2.5s endpoint-change checks
    /// so passive volume polling performs zero `GetDefaultAudioEndpoint` COM RPCs.
    fn ensure_current_ep(&mut self, force_verify: bool) -> Option<(&IAudioEndpointVolume, bool)> {
        if !force_verify
            && self.ep.is_some()
            && self.last_ep_verify.elapsed() < Duration::from_millis(2500)
        {
            return self.ep.as_ref().map(|e| (e, false));
        }
        unsafe {
            let enumerator = self.ensure_enumerator()?.clone();
            let dev = enumerator.GetDefaultAudioEndpoint(eRender, eMultimedia).ok()?;
            let cur_id = read_mmdevice_id(&dev).unwrap_or_default();
            self.last_ep_verify = Instant::now();
            if self.ep.is_some() && !cur_id.is_empty() && cur_id == self.default_id {
                return self.ep.as_ref().map(|e| (e, false));
            }
            let ep: IAudioEndpointVolume = dev.Activate(CLSCTX_INPROC_SERVER, None).ok()?;
            let changed = !self.default_id.is_empty() && cur_id != self.default_id;
            self.default_id = cur_id;
            self.ep = Some(ep);
            self.ep.as_ref().map(|e| (e, changed))
        }
    }

    /// Reads `(vol_pct, vol_muted, default_endpoint_changed)` from the current default output.
    fn read_volume(&mut self) -> (u8, bool, bool) {
        unsafe {
            if let Some((ep, changed)) = self.ensure_current_ep(false) {
                if let (Ok(scalar), Ok(muted)) = (ep.GetMasterVolumeLevelScalar(), ep.GetMute()) {
                    return (
                        (scalar * 100.0).round().clamp(0.0, 100.0) as u8,
                        muted.as_bool(),
                        changed,
                    );
                }
                self.invalidate();
            }
            if let Some((ep, changed)) = self.ensure_current_ep(true) {
                if let (Ok(scalar), Ok(muted)) = (ep.GetMasterVolumeLevelScalar(), ep.GetMute()) {
                    return (
                        (scalar * 100.0).round().clamp(0.0, 100.0) as u8,
                        muted.as_bool(),
                        changed,
                    );
                }
                self.invalidate();
            }
            (50, false, false)
        }
    }

    fn set_volume(&mut self, pct: u8, mute_opt: Option<bool>) {
        unsafe {
            let scalar = (pct.min(100) as f32) / 100.0;
            if let Some((ep, _)) = self.ensure_current_ep(true) {
                let mut ok = ep.SetMasterVolumeLevelScalar(scalar, null()).is_ok();
                if ok {
                    if let Some(m) = mute_opt {
                        ok = ep.SetMute(m, null()).is_ok();
                    }
                }
                if ok {
                    return;
                }
                self.invalidate();
            }
            if let Some((ep, _)) = self.ensure_current_ep(true) {
                let _ = ep.SetMasterVolumeLevelScalar(scalar, null());
                if let Some(m) = mute_opt {
                    let _ = ep.SetMute(m, null());
                }
            }
        }
    }

    fn toggle_mute(&mut self) {
        unsafe {
            if let Some((ep, _)) = self.ensure_current_ep(true) {
                if let Ok(muted) = ep.GetMute() {
                    if ep.SetMute(!muted.as_bool(), null()).is_ok() {
                        return;
                    }
                }
                self.invalidate();
            }
            if let Some((ep, _)) = self.ensure_current_ep(true) {
                if let Ok(muted) = ep.GetMute() {
                    let _ = ep.SetMute(!muted.as_bool(), null());
                }
            }
        }
    }

    fn enumerate_outputs(&mut self) -> Vec<AudioOutputDevice> {
        unsafe {
            let Some(enumerator) = self.ensure_enumerator().cloned() else {
                return Vec::new();
            };
            let default_id = enumerator
                .GetDefaultAudioEndpoint(eRender, eMultimedia)
                .ok()
                .and_then(|d| read_mmdevice_id(&d))
                .unwrap_or_default();
            let Ok(coll) = enumerator.EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE) else {
                return Vec::new();
            };
            let count = coll.GetCount().unwrap_or(0);
            let mut raw_list: Vec<(String, String, String, bool)> = Vec::with_capacity(count as usize);
            for i in 0..count {
                let Ok(dev) = coll.Item(i) else { continue };
                let Some(id) = read_mmdevice_id(&dev) else { continue };
                let raw_name = read_mmdevice_friendly_name(&dev);
                let short_name = shorten_audio_device_name(&raw_name);
                let is_def = !default_id.is_empty() && id == default_id;
                raw_list.push((id, short_name, raw_name, is_def));
            }
            // Disambiguate duplicate short names cleanly
            let mut counts: HashMap<String, usize> = HashMap::new();
            for (_, s, _, _) in &raw_list {
                *counts.entry(s.to_ascii_lowercase()).or_insert(0) += 1;
            }
            let mut out: Vec<AudioOutputDevice> = raw_list
                .into_iter()
                .map(|(id, short, raw, is_default)| {
                    let name = if counts.get(&short.to_ascii_lowercase()).copied().unwrap_or(0) > 1 {
                        raw
                    } else {
                        short
                    };
                    AudioOutputDevice {
                        id,
                        name,
                        is_default,
                    }
                })
                .collect();
            // Stable ordering: built-in Speakers first, then alphabetical by (name, id) so buttons
            // never swap slots underneath the cursor when switching active output.
            out.sort_by(|a, b| {
                let a_spk = a.name.to_ascii_lowercase().starts_with("speaker");
                let b_spk = b.name.to_ascii_lowercase().starts_with("speaker");
                b_spk
                    .cmp(&a_spk)
                    .then_with(|| a.name.to_ascii_lowercase().cmp(&b.name.to_ascii_lowercase()))
                    .then_with(|| a.id.cmp(&b.id))
            });
            out
        }
    }
}

fn get_master_volume() -> (u8, bool) {
    unsafe {
        let mut init_ok = false;
        if CoInitializeEx(None, COINIT_MULTITHREADED).is_ok() {
            init_ok = true;
        }
        let res = (|| -> Option<(u8, bool)> {
            let enumerator: IMMDeviceEnumerator =
                CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_INPROC_SERVER).ok()?;
            let dev = enumerator.GetDefaultAudioEndpoint(eRender, eMultimedia).ok()?;
            let ep: IAudioEndpointVolume = dev.Activate(CLSCTX_INPROC_SERVER, None).ok()?;
            let scalar = ep.GetMasterVolumeLevelScalar().ok()?;
            let muted = ep.GetMute().ok()?.as_bool();
            Some(((scalar * 100.0).round().clamp(0.0, 100.0) as u8, muted))
        })();
        if init_ok {
            CoUninitialize();
        }
        res.unwrap_or((50, false))
    }
}

fn read_wifi_wlan_fast() -> (bool, String, u8) {
    let mut connected = false;
    let mut ssid = String::new();
    let mut signal_pct = 0u8;
    unsafe {
        let mut neg = 0u32;
        let mut client = windows::Win32::Foundation::HANDLE::default();
        if WlanOpenHandle(2, None, &mut neg, &mut client) == 0 {
            let mut list_ptr: *mut WLAN_INTERFACE_INFO_LIST = null_mut();
            if WlanEnumInterfaces(client, None, &mut list_ptr) == 0 && !list_ptr.is_null() {
                let cnt = (*list_ptr).dwNumberOfItems as usize;
                let base = (*list_ptr).InterfaceInfo.as_ptr();
                for i in 0..cnt {
                    let info = &*base.add(i);
                    if info.isState
                        == windows::Win32::NetworkManagement::WiFi::wlan_interface_state_connected
                    {
                        connected = true;
                        let mut data_sz = 0u32;
                        let mut data_ptr: *mut c_void = null_mut();
                        if WlanQueryInterface(
                            client,
                            &info.InterfaceGuid,
                            wlan_intf_opcode_current_connection,
                            None,
                            &mut data_sz,
                            &mut data_ptr,
                            None,
                        ) == 0
                            && !data_ptr.is_null()
                        {
                            let attr = &*(data_ptr as *const WLAN_CONNECTION_ATTRIBUTES);
                            signal_pct = attr.wlanAssociationAttributes.wlanSignalQuality.min(100) as u8;
                            let len =
                                (attr.wlanAssociationAttributes.dot11Ssid.uSSIDLength as usize)
                                    .min(32);
                            if len > 0 {
                                ssid = String::from_utf8_lossy(
                                    &attr.wlanAssociationAttributes.dot11Ssid.ucSSID[..len],
                                )
                                .to_string();
                            }
                            WlanFreeMemory(data_ptr);
                        }
                    }
                }
                WlanFreeMemory(list_ptr as *mut c_void);
            }
            let _ = WlanCloseHandle(client, None);
        }
    }
    let label = if !connected {
        "Disconnected".to_string()
    } else if !ssid.is_empty() {
        ssid
    } else {
        "Wi-Fi".to_string()
    };
    let final_sig = if connected {
        if signal_pct > 0 { signal_pct } else { 80 }
    } else {
        0
    };
    (connected, label, final_sig)
}

fn collect_cc_snapshot_light(
    prev: &CcSnapshot,
    cached_vol: &mut CachedAudioVolume,
    include_outputs: bool,
) -> CcSnapshot {
    let mut next = prev.clone();
    let (vol_pct, vol_muted, ep_changed) = cached_vol.read_volume();
    next.vol_pct = vol_pct;
    next.vol_muted = vol_muted;
    if include_outputs || ep_changed || next.audio_outputs.is_empty() {
        next.audio_outputs = cached_vol.enumerate_outputs();
    }
    let (wlan_on, wlan_label, wlan_sig) = read_wifi_wlan_fast();
    if wlan_on {
        next.wifi_on = true;
        next.wifi_name = wlan_label;
        next.wifi_signal = wlan_sig;
    } else {
        next.wifi_signal = 0;
        if next.wifi_name.is_empty() || (prev.wifi_on && !prev.wifi_name.eq_ignore_ascii_case("Off")) {
            next.wifi_name = wlan_label;
        }
    }
    let mut ps = SYSTEM_POWER_STATUS::default();
    if unsafe { GetSystemPowerStatus(&mut ps).is_ok() } {
        next.saver_on = ps.SystemStatusFlag > 0
            || read_reg_dword(
                HKEY_LOCAL_MACHINE,
                w!(r"SYSTEM\CurrentControlSet\Control\Power"),
                w!("EnergySaverState"),
            ) == Some(1);
    }
    next.night_on = read_night_light();
    next.hot_on = read_hotspot_state();
    next.near_on = read_reg_dword(
        HKEY_CURRENT_USER,
        w!(r"Software\Microsoft\Windows\CurrentVersion\CDP"),
        w!("NearShareChannelUserAuthzPolicy"),
    )
    .unwrap_or(0)
        > 0;
    next.disp_on = unsafe { GetSystemMetrics(SM_CMONITORS) > 1 };
    next
}

fn set_master_volume(pct: u8, mute_opt: Option<bool>) {
    unsafe {
        let mut init_ok = false;
        if CoInitializeEx(None, COINIT_MULTITHREADED).is_ok() {
            init_ok = true;
        }
        let mut vol = CachedAudioVolume::new();
        vol.set_volume(pct, mute_opt);
        if init_ok {
            CoUninitialize();
        }
    }
}

struct CcHeavyCache {
    last_refresh: Option<Instant>,
    bt_sub: String,
    cap_on: bool,
    acc_on: bool,
    saver_com: Option<bool>,
    hot_com: bool,
    near_com: Option<bool>,
}

impl Default for CcHeavyCache {
    fn default() -> Self {
        Self {
            last_refresh: None,
            bt_sub: "Not connected".to_string(),
            cap_on: false,
            acc_on: false,
            saver_com: None,
            hot_com: false,
            near_com: None,
        }
    }
}

fn collect_cc_snapshot(
    cached_vol: &mut CachedAudioVolume,
    include_outputs: bool,
    heavy: &mut CcHeavyCache,
    force_heavy: bool,
) -> CcSnapshot {
    // Single Radio::GetRadiosAsync() + single Wlan query instead of duplicating both
    let (mut wifi_on, bt_on) = read_radios_flags();
    let (wlan_conn, wlan_label, wlan_sig) = read_wifi_wlan_fast();
    if wlan_conn {
        wifi_on = true;
    }
    let wifi_name = if !wifi_on {
        "Off".to_string()
    } else if wlan_conn {
        wlan_label
    } else {
        "Disconnected".to_string()
    };
    let wifi_signal = if wifi_on {
        if wlan_conn && wlan_sig > 0 { wlan_sig } else { 85 }
    } else {
        0
    };

    let refresh_heavy = force_heavy
        || heavy
            .last_refresh
            .map(|t| t.elapsed() >= Duration::from_secs(15))
            .unwrap_or(true);
    if refresh_heavy {
        heavy.last_refresh = Some(Instant::now());
        heavy.bt_sub = if bt_on {
            read_bt_connected_sub()
        } else {
            "Off".to_string()
        };
        let (cap, acc) = scan_process_flags();
        heavy.cap_on = cap;
        heavy.acc_on = acc;
        heavy.saver_com = try_get_system_setting_bool(
            "SettingsHandlers_OneCore_BatterySaver.dll",
            "SystemSettings_BatterySaver_LandingPage_OverrideControl",
        );
        heavy.hot_com = try_get_system_setting_bool(
            "NetworkMobileSettings.dll",
            "SystemSettings_Connections_InternetSharingEnabled",
        )
        .unwrap_or(false);
        heavy.near_com = try_get_system_setting_bool(
            "SettingsHandlers_SharedExperiences_Rome.dll",
            "SystemSettings_SharedExperiences_NearShareQuickAction",
        );
    }

    let bt_sub = if !bt_on {
        "Off".to_string()
    } else if heavy.bt_sub == "Off" {
        read_bt_connected_sub()
    } else {
        heavy.bt_sub.clone()
    };
    let air_on = read_reg_dword(
        HKEY_LOCAL_MACHINE,
        w!(r"SYSTEM\CurrentControlSet\Control\RadioManagement\SystemRadioState"),
        PCWSTR::null(),
    ) == Some(1);
    let cap_on = heavy.cap_on;
    let mut acc_on = heavy.acc_on;
    if !acc_on {
        if read_reg_dword(
            HKEY_CURRENT_USER,
            w!(r"Software\Microsoft\ColorFiltering"),
            w!("Active"),
        ) == Some(1)
        {
            acc_on = true;
        }
    }
    let mut ps = SYSTEM_POWER_STATUS::default();
    let mut saver_on = unsafe { GetSystemPowerStatus(&mut ps).is_ok() } && ps.SystemStatusFlag > 0;
    if !saver_on {
        if read_reg_dword(
            HKEY_LOCAL_MACHINE,
            w!(r"SYSTEM\CurrentControlSet\Control\Power"),
            w!("EnergySaverState"),
        ) == Some(1)
        {
            saver_on = true;
        } else if let Some(v) = heavy.saver_com {
            saver_on = v;
        }
    }
    // Fresh SettingsHandlers_Display instances return unhydrated false at 0ms; CloudStore holds the live state.
    let night_on = read_night_light();
    let hot_on = read_hotspot_state() || heavy.hot_com;
    let near_on = heavy.near_com.unwrap_or_else(|| {
        read_reg_dword(
            HKEY_CURRENT_USER,
            w!(r"Software\Microsoft\Windows\CurrentVersion\CDP"),
            w!("NearShareChannelUserAuthzPolicy"),
        )
        .unwrap_or(0)
            > 0
    });
    let disp_on = unsafe { GetSystemMetrics(SM_CMONITORS) > 1 };
    let bright_pct = get_lcd_brightness();
    let (vol_pct, vol_muted, _) = cached_vol.read_volume();
    let audio_outputs = if include_outputs {
        cached_vol.enumerate_outputs()
    } else {
        Vec::new()
    };

    CcSnapshot {
        wifi_on,
        wifi_name,
        wifi_signal,
        bt_on,
        bt_sub,
        air_on,
        acc_on,
        saver_on,
        cap_on,
        night_on,
        hot_on,
        near_on,
        disp_on,
        bright_pct,
        vol_pct,
        vol_muted,
        audio_outputs,
    }
}

// =========================================================================
// Direct SystemSettings COM Engine (Night Light / Nearby / Airplane / Saver / Hotspot)
// =========================================================================
fn with_system_setting_bool<R, F>(dll_name: &str, setting_id: &str, f: F) -> Option<R>
where
    F: FnOnce(*mut c_void, *mut c_void, &dyn Fn() -> Option<bool>) -> Option<R>,
{
    type GetSettingFn = unsafe extern "system" fn(*mut c_void, *mut *mut c_void) -> HRESULT;
    type DsGetValFn = unsafe extern "system" fn(*mut c_void, *mut c_void, *mut *mut c_void) -> HRESULT;

    unsafe {
        let dll_w = to_wide(dll_name);
        let Ok(hmod) = LoadLibraryW(PCWSTR(dll_w.as_ptr())) else {
            return None;
        };
        let Some(p_fn) = GetProcAddress(hmod, PCSTR(b"GetSetting\0".as_ptr())) else {
            return None;
        };
        let get_setting: GetSettingFn = std::mem::transmute(p_fn);

        let h_sid = HSTRING::from(setting_id);
        let raw_sid = std::ptr::read((&h_sid) as *const HSTRING as *const *mut c_void);
        let mut p_obj: *mut c_void = null_mut();
        let hr = get_setting(raw_sid, &mut p_obj);
        drop(h_sid);
        if hr.is_err() || p_obj.is_null() {
            return None;
        }

        // IID IDataSetting = 40c037cc-d8bf-489e-8697-d66baa3221bf
        let obj_unk = windows::core::IUnknown::from_raw(p_obj);
        let iid_ds = GUID::from_u128(0x40c037cc_d8bf_489e_8697_d66baa3221bf);
        let mut p_ds: *mut c_void = null_mut();
        if obj_unk.query(&iid_ds, &mut p_ds).is_err() || p_ds.is_null() {
            return None;
        }
        let ds_unk = windows::core::IUnknown::from_raw(p_ds);

        let h_val = HSTRING::from("Value");
        let raw_val = std::ptr::read((&h_val) as *const HSTRING as *const *mut c_void);
        let read_cur = || -> Option<bool> {
            let vt = *(p_ds as *const *const usize);
            let get_val: DsGetValFn = std::mem::transmute(*vt.add(13));
            let mut p_prop: *mut c_void = null_mut();
            if get_val(p_ds, raw_val, &mut p_prop).is_err() || p_prop.is_null() {
                return None;
            }
            let prop_unk = windows::core::IUnknown::from_raw(p_prop);
            let pv: IPropertyValue = prop_unk.cast().ok()?;
            match pv.Type().ok()? {
                PropertyType::Boolean => pv.GetBoolean().ok(),
                PropertyType::Int32 => Some(pv.GetInt32().unwrap_or(0) != 0),
                _ => None,
            }
        };

        let out = f(p_ds, raw_val, &read_cur);
        drop(ds_unk);
        drop(h_val);
        out
    }
}

fn try_get_system_setting_bool(dll_name: &str, setting_id: &str) -> Option<bool> {
    with_system_setting_bool(dll_name, setting_id, |_, _, read_cur| read_cur())
}

fn try_toggle_system_setting_bool(
    dll_name: &str,
    setting_id: &str,
    init_delay_ms: u64,
    commit_delay_ms: u64,
) -> bool {
    type DsSetValFn = unsafe extern "system" fn(*mut c_void, *mut c_void, *mut c_void) -> HRESULT;

    with_system_setting_bool(dll_name, setting_id, |p_ds, raw_val, read_cur| unsafe {
        let mut cur_val = read_cur()?;
        if init_delay_ms > 0 {
            thread::sleep(Duration::from_millis(init_delay_ms));
            if let Some(refreshed) = read_cur() {
                cur_val = refreshed;
            }
        }
        if let Ok(new_pv) = PropertyValue::CreateBoolean(!cur_val) {
            let vt = *(p_ds as *const *const usize);
            let set_val: DsSetValFn = std::mem::transmute(*vt.add(14));
            if set_val(p_ds, raw_val, new_pv.as_raw()).is_ok() {
                if commit_delay_ms > 0 {
                    thread::sleep(Duration::from_millis(commit_delay_ms));
                }
                return Some(true);
            }
        }
        None
    })
    .unwrap_or(false)
}

// =========================================================================
// 12-Band Hann-Windowed 1024-pt WASAPI Loopback Spectrum Analyzer
// (with Fast Device-Switch Detection, Pink-Noise Tilt & AGC)
// =========================================================================
pub(crate) const WASAPI_FFT_LEN: usize = 1024;
pub(crate) const WASAPI_BAND_FREQS: [f32; 12] = [
    55.0, 95.0, 160.0, 270.0, 450.0, 750.0, 1250.0, 2100.0, 3500.0, 5800.0, 9000.0, 13000.0,
];

fn build_hann_window() -> [f32; WASAPI_FFT_LEN] {
    let mut win = [0.0f32; WASAPI_FFT_LEN];
    let denom = (WASAPI_FFT_LEN - 1) as f32;
    for (i, slot) in win.iter_mut().enumerate() {
        *slot = 0.5 * (1.0 - (TAU * i as f32 / denom).cos());
    }
    win
}

fn build_goertzel_coeffs(sample_rate: f32) -> [f32; 12] {
    let sr = sample_rate.max(8000.0);
    let mut coeffs = [0.0f32; 12];
    for (bi, &freq) in WASAPI_BAND_FREQS.iter().enumerate() {
        let clamped_f = freq.min(sr * 0.48);
        let omega = TAU * clamped_f / sr;
        coeffs[bi] = 2.0 * omega.cos();
    }
    coeffs
}

/// Smooth, strictly monotonic perceptual loudness curve derived from master endpoint volume.
/// Guarantees 0% volume or Muted always maps to exact `0.0`, while 1%..100% increases smoothly.
pub(crate) fn volume_visualizer_scale(vol_scalar: f32, muted: bool) -> f32 {
    if muted || !vol_scalar.is_finite() || vol_scalar <= 0.005 {
        return 0.0;
    }
    vol_scalar.clamp(0.0, 1.0).powf(0.55)
}

/// Computes 12 perceptually balanced, Hann-windowed Goertzel spectrum bands from a 1024-sample ring.
pub(crate) fn compute_hann_goertzel_bands(
    ring: &[f32; WASAPI_FFT_LEN],
    ring_pos: usize,
    hann: &[f32; WASAPI_FFT_LEN],
    coeffs: &[f32; 12],
    agc_ref: f32,
) -> [f32; 12] {
    let gain = (0.58 / agc_ref.max(0.08)).clamp(0.90, 5.5);
    let mut windowed = [0.0f32; WASAPI_FFT_LEN];
    let mut peak = 0.0f32;
    for i in 0..WASAPI_FFT_LEN {
        let s = (ring[(ring_pos + i) & (WASAPI_FFT_LEN - 1)] * gain).clamp(-1.4, 1.4);
        peak = peak.max(s.abs());
        windowed[i] = s * hann[i];
    }
    if peak < 0.0015 {
        return [0.0; 12];
    }
    let mut out = [0.0f32; 12];
    let inv_norm = 2.0 / (WASAPI_FFT_LEN as f32 * 0.5); // compensate 0.5 Hann coherent gain
    for (bi, &coeff) in coeffs.iter().enumerate() {
        let mut q1 = 0.0f32;
        let mut q2 = 0.0f32;
        for &s in &windowed {
            let q0 = coeff * q1 - q2 + s;
            q2 = q1;
            q1 = q0;
        }
        let mag = (q1 * q1 + q2 * q2 - q1 * q2 * coeff).max(0.0).sqrt() * inv_norm;
        // Compensate 1/f pink-noise rolloff so mid/treble instruments dance alongside kick bass
        let p = bi as f32 / 11.0;
        let tilt = 0.80 + p.powf(1.25) * 1.95;
        let level = (mag * tilt * 2.15).clamp(0.0, 1.25);
        out[bi] = if level < 0.022 {
            0.0
        } else {
            ((level - 0.022) / 0.98).powf(0.74).clamp(0.0, 1.0)
        };
    }
    out
}

/// Computes volume-aware spectrum bands so 0%/Muted is strictly flat (`0.0`) and 1%..100% scales monotonically.
pub(crate) fn compute_volume_scaled_bands(
    ring: &[f32; WASAPI_FFT_LEN],
    ring_pos: usize,
    hann: &[f32; WASAPI_FFT_LEN],
    coeffs: &[f32; 12],
    agc_ref: f32,
    vol_scalar: f32,
    muted: bool,
) -> [f32; 12] {
    let vol_scale = volume_visualizer_scale(vol_scalar, muted);
    if vol_scale <= 0.0 {
        return [0.0; 12];
    }
    let mut bands = compute_hann_goertzel_bands(ring, ring_pos, hann, coeffs, agc_ref);
    for b in &mut bands {
        *b = (*b * vol_scale).clamp(0.0, 1.0);
    }
    bands
}

unsafe fn query_endpoint_id(device: &windows::Win32::Media::Audio::IMMDevice) -> Option<String> {
    let pw = device.GetId().ok()?;
    if pw.0.is_null() {
        return None;
    }
    let mut len = 0usize;
    while *pw.0.add(len) != 0 && len < 512 {
        len += 1;
    }
    let s = String::from_utf16_lossy(std::slice::from_raw_parts(pw.0, len));
    CoTaskMemFree(Some(pw.0 as *const c_void));
    Some(s)
}

fn run_wasapi_loopback_worker(
    shared: Arc<Mutex<SharedState>>,
    stop: Arc<AtomicBool>,
    stop_wake: Arc<(Mutex<bool>, Condvar)>,
    viz_frame_ms: Arc<AtomicU32>,
    viz_needed: Arc<AtomicBool>,
    hwnd_raw: isize,
    notify_msg: u32,
) {
    defer_on_fresh_boot(&stop);
    if stop.load(Ordering::Relaxed) {
        return;
    }
    unsafe {
        let _ = SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_BELOW_NORMAL);
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    }

    let hann = build_hann_window();
    let mut smooth = [0.0f32; 12];
    let mut agc_ref = 0.14f32;
    let mut synth_phase = 0.0f32;

    unsafe {
        let Ok(enumerator) = CoCreateInstance::<_, IMMDeviceEnumerator>(
            &MMDeviceEnumerator,
            None,
            CLSCTX_INPROC_SERVER,
        ) else {
            CoUninitialize();
            return;
        };

        while !stop.load(Ordering::Relaxed) {
            let device_opt = enumerator
                .GetDefaultAudioEndpoint(eRender, eMultimedia)
                .or_else(|_| enumerator.GetDefaultAudioEndpoint(eRender, eConsole))
                .ok();
            let Some(device) = device_opt else {
                if !sleep_or_stop(&stop_wake, Duration::from_millis(500)) {
                    break;
                }
                continue;
            };
            let active_dev_id = query_endpoint_id(&device);

            let meter_opt = device
                .Activate::<IAudioMeterInformation>(CLSCTX_INPROC_SERVER, None)
                .ok();
            let vol_ep_opt = device
                .Activate::<IAudioEndpointVolume>(CLSCTX_INPROC_SERVER, None)
                .ok();

            let Ok(client) = device.Activate::<IAudioClient>(CLSCTX_INPROC_SERVER, None) else {
                if !sleep_or_stop(&stop_wake, Duration::from_millis(500)) {
                    break;
                }
                continue;
            };
            let Ok(fmt_ptr) = client.GetMixFormat() else {
                if !sleep_or_stop(&stop_wake, Duration::from_millis(500)) {
                    break;
                }
                continue;
            };

            let sample_rate = (*fmt_ptr).nSamplesPerSec.max(8000) as f32;
            let channels = (*fmt_ptr).nChannels.max(1) as usize;
            let bits = (*fmt_ptr).wBitsPerSample;
            let block_align = (*fmt_ptr).nBlockAlign.max(1) as usize;
            let fmt_tag = (*fmt_ptr).wFormatTag;
            let is_float = fmt_tag == 3
                || (fmt_tag == 0xFFFE
                    && std::ptr::addr_of!((*fmt_ptr.cast::<WAVEFORMATEXTENSIBLE>()).SubFormat)
                        .read_unaligned()
                        == GUID::from_u128(0x00000003_0000_0010_8000_00aa00389b71));

            // Use a 100ms loopback buffer so variable FPS wakes never overflow or drop packets
            let init_ok = client
                .Initialize(
                    AUDCLNT_SHAREMODE_SHARED,
                    AUDCLNT_STREAMFLAGS_LOOPBACK,
                    1_000_000,
                    0,
                    fmt_ptr,
                    None,
                )
                .is_ok();
            CoTaskMemFree(Some(fmt_ptr as *const c_void));
            if !init_ok {
                if !sleep_or_stop(&stop_wake, Duration::from_millis(500)) {
                    break;
                }
                continue;
            }
            let Ok(capture) = client.GetService::<IAudioCaptureClient>() else {
                if !sleep_or_stop(&stop_wake, Duration::from_millis(500)) {
                    break;
                }
                continue;
            };
            if client.Start().is_err() {
                if !sleep_or_stop(&stop_wake, Duration::from_millis(500)) {
                    break;
                }
                continue;
            }

            let coeffs = build_goertzel_coeffs(sample_rate);
            let mut ring = [0.0f32; WASAPI_FFT_LEN];
            let mut ring_pos = 0usize;

            let mut silent_ticks = 0u32;
            let mut stream_started = true;
            let mut written_zero = true;

            while !stop.load(Ordering::Relaxed) {
                if !viz_needed.load(Ordering::Relaxed) {
                    if stream_started {
                        let _ = client.Stop();
                        stream_started = false;
                    }
                    if !written_zero {
                        smooth = [0.0; 12];
                        if let Ok(mut g) = shared.lock() {
                            g.bands = smooth;
                        }
                        written_zero = true;
                    }
                    if !sleep_or_stop(&stop_wake, Duration::from_millis(220)) {
                        break;
                    }
                    continue;
                }
                if !stream_started {
                    if client.Start().is_err() {
                        break;
                    }
                    stream_started = true;
                    silent_ticks = 0;
                    ring = [0.0; WASAPI_FFT_LEN];
                    ring_pos = 0;
                }
                let mut peak = 0.0f32;
                let mut pkt_err = false;
                loop {
                    match capture.GetNextPacketSize() {
                        Ok(0) => break,
                        Ok(_) => {
                            let mut data_ptr = null_mut();
                            let mut frames = 0u32;
                            let mut flags = 0u32;
                            if capture
                                .GetBuffer(&mut data_ptr, &mut frames, &mut flags, None, None)
                                .is_ok()
                            {
                                let silent_flag = (flags & 0x2) != 0;
                                if silent_flag || data_ptr.is_null() {
                                    for _ in 0..frames {
                                        ring[ring_pos] = 0.0;
                                        ring_pos = (ring_pos + 1) & (WASAPI_FFT_LEN - 1);
                                    }
                                } else if is_float && bits == 32 {
                                    let slice = std::slice::from_raw_parts(
                                        data_ptr as *const f32,
                                        frames as usize * channels,
                                    );
                                    for f in 0..frames as usize {
                                        let mut sum = 0.0f32;
                                        for c in 0..channels {
                                            sum += slice[f * channels + c];
                                        }
                                        let mono = sum / (channels as f32);
                                        peak = peak.max(mono.abs());
                                        ring[ring_pos] = mono;
                                        ring_pos = (ring_pos + 1) & (WASAPI_FFT_LEN - 1);
                                    }
                                } else if bits == 16 {
                                    let slice = std::slice::from_raw_parts(
                                        data_ptr as *const i16,
                                        frames as usize * channels,
                                    );
                                    for f in 0..frames as usize {
                                        let mut sum = 0.0f32;
                                        for c in 0..channels {
                                            sum += slice[f * channels + c] as f32 / 32768.0;
                                        }
                                        let mono = sum / (channels as f32);
                                        peak = peak.max(mono.abs());
                                        ring[ring_pos] = mono;
                                        ring_pos = (ring_pos + 1) & (WASAPI_FFT_LEN - 1);
                                    }
                                } else if bits == 32 && !is_float {
                                    let slice = std::slice::from_raw_parts(
                                        data_ptr as *const i32,
                                        frames as usize * channels,
                                    );
                                    for f in 0..frames as usize {
                                        let mut sum = 0.0f32;
                                        for c in 0..channels {
                                            sum += (slice[f * channels + c] as f32) / 2147483648.0;
                                        }
                                        let mono = sum / (channels as f32);
                                        peak = peak.max(mono.abs());
                                        ring[ring_pos] = mono;
                                        ring_pos = (ring_pos + 1) & (WASAPI_FFT_LEN - 1);
                                    }
                                } else if bits == 24 && block_align >= channels * 3 {
                                    let bytes = std::slice::from_raw_parts(
                                        data_ptr as *const u8,
                                        frames as usize * block_align,
                                    );
                                    for f in 0..frames as usize {
                                        let base = f * block_align;
                                        let mut sum = 0.0f32;
                                        for c in 0..channels {
                                            let o = base + c * 3;
                                            let raw = ((bytes[o] as i32) << 8)
                                                | ((bytes[o + 1] as i32) << 16)
                                                | ((bytes[o + 2] as i8 as i32) << 24);
                                            sum += (raw as f32) / 2147483648.0;
                                        }
                                        let mono = sum / (channels as f32);
                                        peak = peak.max(mono.abs());
                                        ring[ring_pos] = mono;
                                        ring_pos = (ring_pos + 1) & (WASAPI_FFT_LEN - 1);
                                    }
                                }
                                let _ = capture.ReleaseBuffer(frames);
                            } else {
                                break;
                            }
                        }
                        Err(_) => {
                            pkt_err = true;
                            break;
                        }
                    }
                }

                if pkt_err {
                    break;
                }

                let (vol_scalar, vol_muted) = vol_ep_opt
                    .as_ref()
                    .and_then(|ep| {
                        let s = ep.GetMasterVolumeLevelScalar().ok()?;
                        let m = ep.GetMute().ok()?.as_bool();
                        Some((s.clamp(0.0, 1.0), m))
                    })
                    .unwrap_or((1.0, false));
                let vol_scale = volume_visualizer_scale(vol_scalar, vol_muted);

                if vol_scale <= 0.0 {
                    // Master volume is 0% or Muted: guarantee flat zero bars (never fall back to pre-volume endpoint meter!)
                    silent_ticks = silent_ticks.saturating_add(1);
                    agc_ref = agc_ref * 0.92 + 0.18 * 0.08;
                    for b in &mut smooth {
                        *b *= 0.62;
                        if *b < 0.004 {
                            *b = 0.0;
                        }
                    }
                } else if peak >= 0.00035 {
                    silent_ticks = 0;
                    if peak > agc_ref {
                        agc_ref = agc_ref * 0.62 + peak * 0.38;
                    } else {
                        agc_ref = (agc_ref * 0.985 + peak * 0.015).max(0.08);
                    }
                    let raw_bands = compute_volume_scaled_bands(
                        &ring,
                        ring_pos,
                        &hann,
                        &coeffs,
                        agc_ref,
                        vol_scalar,
                        vol_muted,
                    );
                    for bi in 0..12 {
                        let target = raw_bands[bi];
                        if target > smooth[bi] {
                            smooth[bi] = smooth[bi] * 0.24 + target * 0.76;
                        } else {
                            smooth[bi] = smooth[bi] * 0.70 + target * 0.30;
                        }
                    }
                } else {
                    // Only query COM endpoint peak meter when loopback buffer was silent AND volume > 0% & unmuted.
                    // When long-silent (>30 ticks), probe every other tick to cut idle COM RPCs in half.
                    let hw_meter_peak = if silent_ticks <= 30 || silent_ticks % 2 == 0 {
                        meter_opt
                            .as_ref()
                            .and_then(|m| m.GetPeakValue().ok())
                            .unwrap_or(0.0)
                    } else {
                        0.0
                    };
                    if hw_meter_peak > 0.003 {
                        // Fallback scaled monotonically by master endpoint volume so low volume never spikes full-up
                        silent_ticks = 0;
                        synth_phase = (synth_phase + 0.22) % (TAU * 32.0);
                        let env = (hw_meter_peak * 2.0).max(0.0).powf(0.72).clamp(0.04, 1.0) * vol_scale;
                        for (bi, s) in smooth.iter_mut().enumerate() {
                            let p = bi as f32 / 11.0;
                            let w1 =
                                (synth_phase * (1.2 + p * 0.7) + bi as f32 * 0.85).sin() * 0.5 + 0.5;
                            let w2 = (synth_phase * 1.9 - bi as f32 * 1.3).cos() * 0.5 + 0.5;
                            let target =
                                (env * (0.28 + 0.72 * (w1 * 0.6 + w2 * 0.4))).clamp(0.0, 1.0);
                            *s = *s * 0.38 + target * 0.62;
                        }
                    } else {
                        silent_ticks = silent_ticks.saturating_add(1);
                        for b in &mut smooth {
                            *b *= 0.68;
                            if *b < 0.004 {
                                *b = 0.0;
                            }
                        }
                    }
                }

                let is_all_zero = smooth.iter().all(|&b| b == 0.0);
                let woke_from_silence = written_zero && !is_all_zero;
                if !(is_all_zero && written_zero) {
                    if let Ok(mut g) = shared.lock() {
                        g.bands = smooth;
                    }
                    written_zero = is_all_zero;
                    if woke_from_silence && hwnd_raw != 0 {
                        let _ = PostMessageW(
                            Some(HWND(hwnd_raw as *mut c_void)),
                            notify_msg,
                            WPARAM(0),
                            LPARAM(0),
                        );
                    }
                }

                // Fast default audio output switch detection (e.g., speakers -> Bluetooth/USB DAC)
                if silent_ticks >= 16 && silent_ticks % 16 == 0 {
                    if let Ok(cur_dev) = enumerator
                        .GetDefaultAudioEndpoint(eRender, eMultimedia)
                        .or_else(|_| enumerator.GetDefaultAudioEndpoint(eRender, eConsole))
                    {
                        let cur_id = query_endpoint_id(&cur_dev);
                        if cur_id != active_dev_id {
                            break;
                        }
                    }
                }

                let active_sleep_ms = viz_frame_ms.load(Ordering::Relaxed).clamp(8, 66) as u64;
                if !sleep_or_stop(
                    &stop_wake,
                    Duration::from_millis(if silent_ticks > 30 { 250 } else { active_sleep_ms }),
                ) {
                    break;
                }
            }

            if stream_started {
                let _ = client.Stop();
            }
        }
        CoUninitialize();
    }
}

// =========================================================================
// Native Rust UI Automation Quick Settings Flyout Helper (Accessibility / Bluetooth)
// =========================================================================
unsafe fn hwnd_class_name(hwnd: HWND) -> String {
    let mut buf = [0u16; 128];
    let n = GetClassNameW(hwnd, &mut buf);
    if n > 0 {
        String::from_utf16_lossy(&buf[..n as usize])
    } else {
        String::new()
    }
}

unsafe fn collect_shell_host_pids() -> HashSet<u32> {
    let mut pids = HashSet::new();
    if let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) {
        let mut pe = PROCESSENTRY32W {
            dwSize: size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        if Process32FirstW(snap, &mut pe).is_ok() {
            loop {
                let end = pe
                    .szExeFile
                    .iter()
                    .position(|&c| c == 0)
                    .unwrap_or(pe.szExeFile.len());
                let exe = String::from_utf16_lossy(&pe.szExeFile[..end]).to_ascii_lowercase();
                if exe == "shellhost.exe" || exe == "shellexperiencehost.exe" {
                    pids.insert(pe.th32ProcessID);
                }
                if Process32NextW(snap, &mut pe).is_err() {
                    break;
                }
            }
        }
        let _ = CloseHandle(snap);
    }
    pids
}

fn is_excluded_shell_cls(cls: &str) -> bool {
    matches!(
        cls.to_ascii_lowercase().as_str(),
        "shell_traywnd"
            | "shell_secondarytraywnd"
            | "progman"
            | "workerw"
            | "notifyiconoverflowwindow"
            | "topleveloverflowwindow"
            | "taskbarframe"
    )
}

unsafe fn push_hwnd_and_xaml_children(
    uia: &IUIAutomation,
    hwnd: HWND,
    seen: &mut HashSet<isize>,
    roots: &mut Vec<IUIAutomationElement>,
) {
    if hwnd.0.is_null() || !seen.insert(hwnd.0 as isize) {
        return;
    }
    if let Ok(el) = uia.ElementFromHandle(hwnd) {
        roots.push(el);
    }

    struct ChildCtx {
        children: Vec<HWND>,
    }
    unsafe extern "system" fn enum_child_cb(ch: HWND, lparam: LPARAM) -> BOOL {
        if IsWindowVisible(ch).as_bool() {
            let cls = hwnd_class_name(ch).to_ascii_lowercase();
            if cls.contains("inputsite")
                || cls.contains("corewindow")
                || cls.contains("bridge")
                || cls.contains("xaml")
            {
                let ctx = &mut *(lparam.0 as *mut ChildCtx);
                ctx.children.push(ch);
            }
        }
        BOOL(1)
    }

    let mut ctx = ChildCtx {
        children: Vec::new(),
    };
    let _ = EnumChildWindows(
        Some(hwnd),
        Some(enum_child_cb),
        LPARAM(&mut ctx as *mut _ as isize),
    );
    for ch in ctx.children {
        if seen.insert(ch.0 as isize) {
            if let Ok(cel) = uia.ElementFromHandle(ch) {
                roots.push(cel);
            }
        }
    }
}

unsafe fn find_qs_roots(uia: &IUIAutomation) -> Vec<IUIAutomationElement> {
    let mut roots = Vec::new();
    let mut seen = HashSet::new();

    if let Ok(cc) = FindWindowW(w!("ControlCenterWindow"), PCWSTR::null()) {
        if !cc.0.is_null() && IsWindowVisible(cc).as_bool() {
            push_hwnd_and_xaml_children(uia, cc, &mut seen, &mut roots);
        }
    }

    let host_pids = collect_shell_host_pids();
    struct TopCtx {
        pids: HashSet<u32>,
        found: Vec<HWND>,
    }
    unsafe extern "system" fn enum_top_cb(hw: HWND, lparam: LPARAM) -> BOOL {
        if !IsWindowVisible(hw).as_bool() {
            return BOOL(1);
        }
        let cls = hwnd_class_name(hw);
        if is_excluded_shell_cls(&cls) {
            return BOOL(1);
        }
        let ctx = &mut *(lparam.0 as *mut TopCtx);
        if cls.eq_ignore_ascii_case("ControlCenterWindow") {
            ctx.found.push(hw);
            return BOOL(1);
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(hw, Some(&mut pid));
        if ctx.pids.contains(&pid) {
            ctx.found.push(hw);
        }
        BOOL(1)
    }

    let mut top_ctx = TopCtx {
        pids: host_pids,
        found: Vec::new(),
    };
    let _ = EnumWindows(
        Some(enum_top_cb),
        LPARAM(&mut top_ctx as *mut _ as isize),
    );
    for hw in top_ctx.found {
        push_hwnd_and_xaml_children(uia, hw, &mut seen, &mut roots);
    }
    roots
}

unsafe fn element_has_pattern(el: &IUIAutomationElement, require_invoke_only: bool) -> bool {
    let has_tog = el.GetCurrentPattern(UIA_TogglePatternId).is_ok();
    let has_inv = el.GetCurrentPattern(UIA_InvokePatternId).is_ok();
    if require_invoke_only {
        has_inv && !has_tog
    } else {
        has_tog || has_inv
    }
}

unsafe fn resolve_qs_pattern_element(
    uia: &IUIAutomation,
    el: &IUIAutomationElement,
    require_invoke_only: bool,
) -> Option<IUIAutomationElement> {
    if let Ok(walker) = uia.ControlViewWalker() {
        let mut cur = Some(el.clone());
        for _ in 0..4 {
            let Some(c) = cur else {
                break;
            };
            if element_has_pattern(&c, require_invoke_only) {
                return Some(c);
            }
            cur = walker.GetParentElement(&c).ok();
        }
    } else if element_has_pattern(el, require_invoke_only) {
        return Some(el.clone());
    }

    if let Ok(true_cond) = uia.CreateTrueCondition() {
        if let Ok(sub) = el.FindAll(TreeScope_Descendants, &true_cond) {
            let cnt = sub.Length().unwrap_or(0);
            for i in 0..cnt {
                if let Ok(ch) = sub.GetElement(i) {
                    if element_has_pattern(&ch, require_invoke_only) {
                        return Some(ch);
                    }
                }
            }
        }
    }
    None
}

unsafe fn search_qs_target(
    uia: &IUIAutomation,
    roots: &[IUIAutomationElement],
    candidates: &[&str],
    require_invoke_only: bool,
) -> (Option<IUIAutomationElement>, Option<IUIAutomationElement>) {
    let Ok(true_cond) = uia.CreateTrueCondition() else {
        return (None, None);
    };
    let mut rect_fallback: Option<IUIAutomationElement> = None;

    for root in roots {
        let Ok(all) = root.FindAll(TreeScope_Descendants, &true_cond) else {
            continue;
        };
        let cnt = all.Length().unwrap_or(0);
        if cnt == 0 {
            continue;
        }
        for &cand in candidates {
            let cand_l = cand.to_ascii_lowercase();
            for i in 0..cnt {
                let Ok(el) = all.GetElement(i) else {
                    continue;
                };
                let name = el
                    .CurrentName()
                    .map(|b| b.to_string().to_ascii_lowercase())
                    .unwrap_or_default();
                let aid = el
                    .CurrentAutomationId()
                    .map(|b| b.to_string().to_ascii_lowercase())
                    .unwrap_or_default();
                if name.is_empty() && aid.is_empty() {
                    continue;
                }
                if name.contains(&cand_l) || aid.contains(&cand_l) {
                    if let Some(resolved) =
                        resolve_qs_pattern_element(uia, &el, require_invoke_only)
                    {
                        return (Some(resolved), rect_fallback);
                    }
                    if rect_fallback.is_none() {
                        if let Ok(r) = el.CurrentBoundingRectangle() {
                            let w = r.right - r.left;
                            let h = r.bottom - r.top;
                            if w > 10 && h > 10 && w < 420 {
                                rect_fallback = Some(el);
                            }
                        }
                    }
                }
            }
        }
    }
    (None, rect_fallback)
}

fn invoke_quick_settings_tile(candidates: &[&str], require_invoke_only: bool) -> bool {
    unsafe {
        let com_ok = CoInitializeEx(None, COINIT_MULTITHREADED).is_ok();
        let res = (|| -> Option<bool> {
            let uia: IUIAutomation =
                CoCreateInstance(&CUIAutomation, None, CLSCTX_ALL).ok()?;
            let mut roots = find_qs_roots(&uia);
            let mut has_target_already = false;
            if !roots.is_empty() {
                let (t0, rf0) = search_qs_target(&uia, &roots, candidates, require_invoke_only);
                if t0.is_some() || rf0.is_some() {
                    has_target_already = true;
                } else {
                    // If Quick Settings is currently open on another L2 subpage (e.g. Wi-Fi list),
                    // click its Back button to return to the Quick Settings home grid first.
                    let (back_btn, _) = search_qs_target(&uia, &roots, &["L2Page_BackButton"], true);
                    if let Some(btn) = back_btn {
                        if let Ok(unk) = btn.GetCurrentPattern(UIA_InvokePatternId) {
                            if let Ok(inv) = unk.cast::<IUIAutomationInvokePattern>() {
                                if inv.Invoke().is_ok() {
                                    thread::sleep(Duration::from_millis(180));
                                    roots = find_qs_roots(&uia);
                                    has_target_already = !roots.is_empty();
                                }
                            }
                        }
                    }
                }
            }
            if roots.is_empty() || !has_target_already {
                let cc_vis = FindWindowW(w!("ControlCenterWindow"), PCWSTR::null())
                    .map(|hw| !hw.0.is_null() && IsWindowVisible(hw).as_bool())
                    .unwrap_or(false);
                if !cc_vis {
                    open_uri("ms-controlcenter:");
                    for i in 0..42 {
                        thread::sleep(Duration::from_millis(30));
                        roots = find_qs_roots(&uia);
                        if !roots.is_empty() {
                            let (t, rf) =
                                search_qs_target(&uia, &roots, candidates, require_invoke_only);
                            if t.is_some() || rf.is_some() {
                                break;
                            }
                        }
                        if i == 8 && roots.is_empty() {
                            send_win_chord_ext(0x41); // Win+A fallback
                        }
                    }
                }
            }
            if roots.is_empty() {
                return Some(false);
            }

            let mut target: Option<IUIAutomationElement> = None;
            let mut rect_fb: Option<IUIAutomationElement> = None;
            for _ in 0..14 {
                roots = find_qs_roots(&uia);
                let (t, rf) = search_qs_target(&uia, &roots, candidates, require_invoke_only);
                if t.is_some() {
                    target = t;
                    rect_fb = rf;
                    break;
                }
                if rect_fb.is_none() {
                    rect_fb = rf;
                }
                thread::sleep(Duration::from_millis(30));
            }

            // If not on Page 1, try clicking Next page / Page 2
            if target.is_none() && rect_fb.is_none() {
                let (next_btn, _) =
                    search_qs_target(&uia, &roots, &["Next page", "Page 2"], true);
                if let Some(btn) = next_btn {
                    if let Ok(unk) = btn.GetCurrentPattern(UIA_InvokePatternId) {
                        if let Ok(inv) = unk.cast::<IUIAutomationInvokePattern>() {
                            if inv.Invoke().is_ok() {
                                thread::sleep(Duration::from_millis(160));
                                roots = find_qs_roots(&uia);
                                let (t, rf) = search_qs_target(
                                    &uia,
                                    &roots,
                                    candidates,
                                    require_invoke_only,
                                );
                                target = t;
                                rect_fb = rf;
                            }
                        }
                    }
                }
            }

            let el = target.or(rect_fb)?;
            if let Ok(unk) = el.GetCurrentPattern(UIA_ScrollItemPatternId) {
                if let Ok(scr) = unk.cast::<IUIAutomationScrollItemPattern>() {
                    if scr.ScrollIntoView().is_ok() {
                        thread::sleep(Duration::from_millis(45));
                    }
                }
            }

            if !require_invoke_only {
                if let Ok(unk) = el.GetCurrentPattern(UIA_TogglePatternId) {
                    if let Ok(tog) = unk.cast::<IUIAutomationTogglePattern>() {
                        if tog.Toggle().is_ok() {
                            return Some(true);
                        }
                    }
                }
            }
            if let Ok(unk) = el.GetCurrentPattern(UIA_InvokePatternId) {
                if let Ok(inv) = unk.cast::<IUIAutomationInvokePattern>() {
                    if inv.Invoke().is_ok() {
                        return Some(true);
                    }
                }
            }

            if let Ok(r) = el.CurrentBoundingRectangle() {
                let w = r.right - r.left;
                let h = r.bottom - r.top;
                if w > 0 && h > 0 {
                    let mut orig = POINT::default();
                    let _ = GetCursorPos(&mut orig);
                    let _ = SetCursorPos(r.left + w / 2, r.top + h / 2);
                    mouse_event(MOUSEEVENTF_LEFTDOWN, 0, 0, 0, 0);
                    mouse_event(MOUSEEVENTF_LEFTUP, 0, 0, 0, 0);
                    let _ = SetCursorPos(orig.x, orig.y);
                    return Some(true);
                }
            }
            Some(false)
        })()
        .unwrap_or(false);
        if com_ok {
            CoUninitialize();
        }
        res
    }
}

// =========================================================================
// Desktop Media Player Capture (VLC, MPC-HC/BE, PotPlayer, foobar2000, AIMP, MusicBee, mpv, Winamp, WMP)
// =========================================================================
#[derive(Clone, Copy, PartialEq, Eq)]
enum DesktopPlayerKind {
    Vlc,
    MpcHc,
    MpcBe,
    PotPlayer,
    Foobar,
    Aimp,
    MusicBee,
    Winamp,
    Mpv,
    Wmp,
    Kodi,
    GenericPlayer,
}

impl DesktopPlayerKind {
    fn from_exe(exe_lower: &str) -> Option<Self> {
        match exe_lower {
            "vlc.exe" => Some(Self::Vlc),
            "mpc-hc64.exe" | "mpc-hc.exe" => Some(Self::MpcHc),
            "mpc-be64.exe" | "mpc-be.exe" => Some(Self::MpcBe),
            "potplayermini64.exe" | "potplayermini.exe" => Some(Self::PotPlayer),
            "foobar2000.exe" => Some(Self::Foobar),
            "aimp.exe" => Some(Self::Aimp),
            "musicbee.exe" => Some(Self::MusicBee),
            "winamp.exe" => Some(Self::Winamp),
            "mpv.exe" | "mpvnet.exe" => Some(Self::Mpv),
            "wmplayer.exe" => Some(Self::Wmp),
            "kodi.exe" => Some(Self::Kodi),
            "kmplayer.exe" | "kmplayer64.exe" | "gom.exe" | "gom64.exe" | "smplayer.exe"
            | "clementine.exe" | "strawberry.exe" | "dopamine.exe" | "mediamonkey.exe"
            | "audacious.exe" | "itunes.exe" => Some(Self::GenericPlayer),
            _ => None,
        }
    }

    fn display_name(self) -> &'static str {
        match self {
            Self::Vlc => "VLC media player",
            Self::MpcHc => "MPC-HC",
            Self::MpcBe => "MPC-BE",
            Self::PotPlayer => "PotPlayer",
            Self::Foobar => "foobar2000",
            Self::Aimp => "AIMP",
            Self::MusicBee => "MusicBee",
            Self::Winamp => "Winamp",
            Self::Mpv => "mpv",
            Self::Wmp => "Windows Media Player",
            Self::Kodi => "Kodi",
            Self::GenericPlayer => "Desktop Media Player",
        }
    }

    fn exe_tag(self) -> &'static str {
        match self {
            Self::Vlc => "vlc",
            Self::MpcHc => "mpchc",
            Self::MpcBe => "mpcbe",
            Self::PotPlayer => "pot",
            Self::Foobar => "foobar",
            Self::Aimp => "aimp",
            Self::MusicBee => "musicbee",
            Self::Winamp => "winamp",
            Self::Mpv => "mpv",
            Self::Wmp => "wmp",
            Self::Kodi => "kodi",
            Self::GenericPlayer => "player",
        }
    }
}

struct DesktopPlayerMatch {
    hwnd: isize,
    kind: DesktopPlayerKind,
    raw_title: String,
    title: String,
    artist: String,
    is_playing: bool,
}

unsafe fn query_active_audio_pids(cached_mgr: &mut Option<IAudioSessionManager2>) -> HashSet<u32> {
    let mut active = HashSet::new();
    let acquire = || -> Option<IAudioSessionManager2> {
        let enumerator: IMMDeviceEnumerator =
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_INPROC_SERVER).ok()?;
        let dev = enumerator.GetDefaultAudioEndpoint(eRender, eMultimedia).ok()?;
        dev.Activate(CLSCTX_INPROC_SERVER, None).ok()
    };

    if cached_mgr.is_none() {
        *cached_mgr = acquire();
    }

    let collect_from = |mgr: &IAudioSessionManager2, out: &mut HashSet<u32>| -> bool {
        let Ok(enumr) = mgr.GetSessionEnumerator() else {
            return false;
        };
        let Ok(count) = enumr.GetCount() else {
            return false;
        };
        for i in 0..count {
            let Ok(ctrl) = enumr.GetSession(i) else {
                continue;
            };
            let Ok(state) = ctrl.GetState() else {
                continue;
            };
            if state != AudioSessionStateActive {
                continue;
            }
            if let Ok(ctrl2) = ctrl.cast::<IAudioSessionControl2>() {
                if let Ok(pid) = ctrl2.GetProcessId() {
                    if pid != 0 {
                        let meter_ok = ctrl
                            .cast::<IAudioMeterInformation>()
                            .ok()
                            .and_then(|m| m.GetPeakValue().ok())
                            .map(|p| p >= 0.0)
                            .unwrap_or(true);
                        if meter_ok {
                            out.insert(pid);
                        }
                    }
                }
            }
        }
        true
    };

    if let Some(ref mgr) = cached_mgr {
        if !collect_from(mgr, &mut active) {
            *cached_mgr = acquire();
            if let Some(ref mgr2) = cached_mgr {
                let _ = collect_from(mgr2, &mut active);
            }
        }
    }
    active
}

unsafe fn collect_desktop_player_pids() -> HashMap<u32, DesktopPlayerKind> {
    let mut map = HashMap::new();
    if let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) {
        let mut pe = PROCESSENTRY32W {
            dwSize: size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        if Process32FirstW(snap, &mut pe).is_ok() {
            loop {
                let end = pe
                    .szExeFile
                    .iter()
                    .position(|&c| c == 0)
                    .unwrap_or(pe.szExeFile.len());
                let exe = String::from_utf16_lossy(&pe.szExeFile[..end]).to_ascii_lowercase();
                if let Some(kind) = DesktopPlayerKind::from_exe(&exe) {
                    map.insert(pe.th32ProcessID, kind);
                }
                if Process32NextW(snap, &mut pe).is_err() {
                    break;
                }
            }
        }
        let _ = CloseHandle(snap);
    }
    map
}

fn strip_media_ext(s: &str) -> &str {
    const EXTS: [&str; 18] = [
        ".flac", ".mp3", ".wav", ".m4a", ".aac", ".ogg", ".opus", ".wma", ".alac", ".aiff",
        ".mp4", ".mkv", ".avi", ".mov", ".webm", ".wmv", ".m4v", ".ts",
    ];
    let lower = s.to_ascii_lowercase();
    for ext in EXTS {
        if lower.ends_with(ext) && s.len() > ext.len() {
            return &s[..s.len() - ext.len()];
        }
    }
    s
}

fn rstrip_ci<'a>(s: &'a str, suffix: &str) -> Option<&'a str> {
    if s.len() >= suffix.len() && s[s.len() - suffix.len()..].eq_ignore_ascii_case(suffix) {
        Some(s[..s.len() - suffix.len()].trim())
    } else {
        None
    }
}

fn parse_desktop_player_title(kind: DesktopPlayerKind, raw: &str) -> Option<(String, String)> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }

    let idle_titles = [
        "VLC media player",
        "Media Player Classic Home Cinema",
        "MPC-HC",
        "MPC-BE",
        "MPC-BE x64",
        "PotPlayer",
        "PotPlayer64",
        "foobar2000",
        "AIMP",
        "MusicBee",
        "Winamp",
        "Windows Media Player",
        "mpv",
        "mpv.net",
        "Kodi",
    ];
    if idle_titles.iter().any(|t| trimmed.eq_ignore_ascii_case(t)) {
        return None;
    }

    let mut body = trimmed;
    match kind {
        DesktopPlayerKind::Vlc => {
            body = rstrip_ci(body, " - VLC media player").unwrap_or(body);
        }
        DesktopPlayerKind::PotPlayer => {
            body = rstrip_ci(body, " - PotPlayer64")
                .or_else(|| rstrip_ci(body, " - PotPlayer"))
                .unwrap_or(body);
        }
        DesktopPlayerKind::MpcHc => {
            body = rstrip_ci(body, " - Media Player Classic Home Cinema")
                .or_else(|| rstrip_ci(body, " - MPC-HC"))
                .unwrap_or(body);
        }
        DesktopPlayerKind::MpcBe => {
            body = rstrip_ci(body, " - MPC-BE x64")
                .or_else(|| rstrip_ci(body, " - MPC-BE"))
                .unwrap_or(body);
        }
        DesktopPlayerKind::Foobar => {
            if let Some(idx) = body.rfind(" [foobar2000") {
                body = body[..idx].trim();
            } else if let Some(b) = rstrip_ci(body, " - foobar2000") {
                body = b;
            }
        }
        DesktopPlayerKind::Aimp => {
            body = rstrip_ci(body, " - AIMP").unwrap_or(body);
        }
        DesktopPlayerKind::MusicBee => {
            body = rstrip_ci(body, " - MusicBee").unwrap_or(body);
        }
        DesktopPlayerKind::Winamp => {
            body = rstrip_ci(body, " [Paused]").unwrap_or(body);
            body = rstrip_ci(body, " [Stopped]").unwrap_or(body);
            body = rstrip_ci(body, " - Winamp").unwrap_or(body);
            if let Some((num, rest)) = body.split_once(". ") {
                if !num.is_empty() && num.chars().all(|c| c.is_ascii_digit()) {
                    body = rest.trim();
                }
            }
        }
        DesktopPlayerKind::Mpv => {
            body = rstrip_ci(body, " - mpv.net")
                .or_else(|| rstrip_ci(body, " - mpv"))
                .unwrap_or(body);
        }
        DesktopPlayerKind::Wmp => {
            body = rstrip_ci(body, " - Windows Media Player").unwrap_or(body);
        }
        DesktopPlayerKind::Kodi => {
            body = rstrip_ci(body, " - Kodi").unwrap_or(body);
        }
        DesktopPlayerKind::GenericPlayer => {}
    }

    if body.is_empty() || idle_titles.iter().any(|t| body.eq_ignore_ascii_case(t)) {
        return None;
    }

    let cleaned = strip_media_ext(body).trim();
    if cleaned.is_empty() {
        return None;
    }

    if let Some((left, right)) = cleaned.split_once(" - ") {
        let a = left.trim();
        let t = strip_media_ext(right.trim()).trim();
        if !a.is_empty() && !t.is_empty() {
            return Some((t.to_string(), a.to_string()));
        }
    }

    Some((cleaned.to_string(), kind.display_name().to_string()))
}

unsafe fn query_pid_desktop_player_kind(pid: u32) -> Option<DesktopPlayerKind> {
    if pid == 0 {
        return None;
    }
    let Ok(h) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
        return None;
    };
    let mut buf = [0u16; 320];
    let mut len = buf.len() as u32;
    let ok = QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len).is_ok();
    let _ = CloseHandle(h);
    if !ok || len == 0 {
        return None;
    }
    let full = String::from_utf16_lossy(&buf[..len as usize]);
    let exe = full
        .rsplit(['\\', '/'])
        .next()
        .unwrap_or(&full)
        .to_ascii_lowercase();
    DesktopPlayerKind::from_exe(&exe)
}

/// Pure helper determining whether a full `CreateToolhelp32Snapshot` scan is due.
pub(crate) fn should_run_full_player_pid_scan(
    elapsed: Duration,
    has_known_players: bool,
) -> bool {
    let interval = if has_known_players {
        Duration::from_secs(6)
    } else {
        Duration::from_secs(12)
    };
    elapsed >= interval
}

fn detect_desktop_player(
    cached_mgr: &mut Option<IAudioSessionManager2>,
    cached_pids: &mut HashMap<u32, DesktopPlayerKind>,
    cached_pid_classes: &mut HashMap<u32, Option<DesktopPlayerKind>>,
    last_pid_scan: &mut Instant,
) -> Option<DesktopPlayerMatch> {
    unsafe {
        let active_pids = query_active_audio_pids(cached_mgr);

        // Classify any newly seen active-audio PID with a single O(1) OpenProcess query (~10us)
        // instead of triggering a full system-wide CreateToolhelp32Snapshot every 350ms whenever
        // a browser or chat app holds an audio session.
        if cached_pid_classes.len() > 128 {
            cached_pid_classes.clear();
        }
        for &pid in &active_pids {
            let kind_opt = *cached_pid_classes
                .entry(pid)
                .or_insert_with(|| query_pid_desktop_player_kind(pid));
            if let Some(kind) = kind_opt {
                cached_pids.insert(pid, kind);
            }
        }

        if should_run_full_player_pid_scan(last_pid_scan.elapsed(), !cached_pids.is_empty()) {
            *cached_pids = collect_desktop_player_pids();
            cached_pid_classes.clear();
            for (&pid, &kind) in cached_pids.iter() {
                cached_pid_classes.insert(pid, Some(kind));
            }
            for &pid in &active_pids {
                cached_pid_classes
                    .entry(pid)
                    .or_insert_with(|| cached_pids.get(&pid).copied());
            }
            *last_pid_scan = Instant::now();
        }
        let pids = &*cached_pids;

        // If ZERO known desktop media player processes exist, skip EnumWindows completely!
        if pids.is_empty() {
            return None;
        }

        struct Cand {
            hwnd: HWND,
            kind: DesktopPlayerKind,
            raw_title: String,
            title: String,
            artist: String,
            is_playing: bool,
            visible: bool,
        }
        struct EnumCtx<'a> {
            pids: &'a HashMap<u32, DesktopPlayerKind>,
            active_pids: &'a HashSet<u32>,
            cands: Vec<Cand>,
        }

        unsafe extern "system" fn enum_cb(hw: HWND, lparam: LPARAM) -> BOOL {
            let ctx = &mut *(lparam.0 as *mut EnumCtx<'_>);
            let mut pid = 0u32;
            GetWindowThreadProcessId(hw, Some(&mut pid));
            let Some(kind) = ctx.pids.get(&pid).copied() else {
                return BOOL(1);
            };

            let mut tbuf = [0u16; 512];
            let tlen = GetWindowTextW(hw, &mut tbuf);
            if tlen <= 0 {
                return BOOL(1);
            }
            let raw_title = String::from_utf16_lossy(&tbuf[..tlen as usize]);

            let cls = hwnd_class_name(hw);
            let cls_low = cls.to_ascii_lowercase();
            if matches!(
                cls_low.as_str(),
                "ime" | "msctfime ui" | "qtrayiconmessagewindowclass" | "static"
            ) {
                return BOOL(1);
            }

            if let Some((title, artist)) = parse_desktop_player_title(kind, &raw_title) {
                let vis = IsWindowVisible(hw).as_bool();
                let is_playing = ctx.active_pids.contains(&pid);
                ctx.cands.push(Cand {
                    hwnd: hw,
                    kind,
                    raw_title,
                    title,
                    artist,
                    is_playing,
                    visible: vis,
                });
            }
            BOOL(1)
        }

        let mut ctx = EnumCtx {
            pids,
            active_pids: &active_pids,
            cands: Vec::new(),
        };
        let _ = EnumWindows(Some(enum_cb), LPARAM(&mut ctx as *mut _ as isize));

        // Prefer actively outputting audio first, then visible main player window.
        ctx.cands.sort_by_key(|c| (std::cmp::Reverse(c.is_playing), std::cmp::Reverse(c.visible)));
        ctx.cands.into_iter().next().map(|c| DesktopPlayerMatch {
            hwnd: c.hwnd.0 as isize,
            kind: c.kind,
            raw_title: c.raw_title,
            title: c.title,
            artist: c.artist,
            is_playing: c.is_playing,
        })
    }
}

fn percent_decode_utf8(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(hi), Some(lo)) = (hex_val(bytes[i + 1]), hex_val(bytes[i + 2])) {
                out.push((hi << 4) | lo);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

fn sanitize_vlc_dir_name(s: &str) -> String {
    s.chars()
        .map(|ch| match ch {
            '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            _ => ch,
        })
        .collect::<String>()
        .trim_end_matches('.')
        .to_string()
}

unsafe fn hbitmap_to_bmp_bytes(hbm: HBITMAP) -> Option<Vec<u8>> {
    let mut bm = BITMAP::default();
    if GetObjectW(
        HGDIOBJ(hbm.0),
        size_of::<BITMAP>() as i32,
        Some(&mut bm as *mut _ as *mut c_void),
    ) == 0
        || bm.bmWidth <= 0
        || bm.bmHeight <= 0
    {
        return None;
    }

    let w = bm.bmWidth;
    let h = bm.bmHeight;
    let pixel_bytes = (w as usize) * (h as usize) * 4;
    let total_size = 54 + pixel_bytes;
    let mut out = vec![0u8; total_size];

    // 14-byte BITMAPFILEHEADER
    out[0] = b'B';
    out[1] = b'M';
    out[2..6].copy_from_slice(&(total_size as u32).to_le_bytes());
    out[10..14].copy_from_slice(&54u32.to_le_bytes());

    let mut bmi = BITMAPINFO::default();
    bmi.bmiHeader.biSize = size_of::<BITMAPINFOHEADER>() as u32;
    bmi.bmiHeader.biWidth = w;
    bmi.bmiHeader.biHeight = h;
    bmi.bmiHeader.biPlanes = 1;
    bmi.bmiHeader.biBitCount = 32;
    bmi.bmiHeader.biCompression = BI_RGB.0;
    bmi.bmiHeader.biSizeImage = pixel_bytes as u32;

    let hdr_bytes = std::slice::from_raw_parts(
        (&bmi.bmiHeader as *const BITMAPINFOHEADER) as *const u8,
        size_of::<BITMAPINFOHEADER>(),
    );
    out[14..54].copy_from_slice(hdr_bytes);

    let hdc = GetDC(None);
    let copied = GetDIBits(
        hdc,
        hbm,
        0,
        h as u32,
        Some(out[54..].as_mut_ptr() as *mut c_void),
        &mut bmi,
        DIB_RGB_COLORS,
    );
    let _ = ReleaseDC(None, hdc);
    if copied <= 0 {
        return None;
    }
    Some(out)
}

fn extract_shell_thumbnail_bmp(path: &Path) -> Option<Vec<u8>> {
    if !path.exists() {
        return None;
    }
    unsafe {
        let wide = to_wide(&path.to_string_lossy());
        let factory: IShellItemImageFactory =
            SHCreateItemFromParsingName(PCWSTR(wide.as_ptr()), None).ok()?;
        let hbm = factory
            .GetImage(
                SIZE { cx: 160, cy: 160 },
                SIIGBF_THUMBNAILONLY | SIIGBF_BIGGERSIZEOK,
            )
            .ok()?;
        let out = hbitmap_to_bmp_bytes(hbm);
        let _ = DeleteObject(HGDIOBJ(hbm.0));
        out
    }
}

fn find_folder_cover(dir: &Path) -> Option<Vec<u8>> {
    for name in ["cover.jpg", "cover.png", "folder.jpg", "folder.png", "front.jpg"] {
        let p = dir.join(name);
        if p.is_file() {
            if let Ok(bytes) = std::fs::read(&p) {
                if !bytes.is_empty() {
                    return Some(bytes);
                }
            }
        }
    }
    None
}

fn find_vlc_recent_matching_path(title: &str, raw_title: &str) -> Option<PathBuf> {
    let appdata = std::env::var("APPDATA").ok()?;
    let ini_path = PathBuf::from(appdata).join("vlc").join("vlc-qt-interface.ini");
    let text = std::fs::read_to_string(&ini_path).ok()?;
    let mut in_recents = false;
    let title_low = title.to_ascii_lowercase();
    let raw_low = raw_title.to_ascii_lowercase();

    for line in text.lines() {
        let l = line.trim();
        if l.starts_with('[') {
            in_recents = l.eq_ignore_ascii_case("[RecentsMRL]");
            continue;
        }
        if in_recents {
            if let Some(rest) = l.strip_prefix("list=") {
                for item in rest.split(',') {
                    let mrl = item.trim();
                    if let Some(file_part) = mrl.strip_prefix("file:///") {
                        let decoded = percent_decode_utf8(file_part).replace('/', "\\");
                        let pb = PathBuf::from(&decoded);
                        if pb.exists() {
                            if let Some(stem) = pb.file_stem().and_then(|s| s.to_str()) {
                                let stem_low = stem.to_ascii_lowercase();
                                if (!title_low.is_empty()
                                    && (stem_low.contains(&title_low) || title_low.contains(&stem_low)))
                                    || (!raw_low.is_empty() && raw_low.contains(&stem_low))
                                {
                                    return Some(pb);
                                }
                            }
                        }
                    }
                }
                break;
            }
        }
    }
    None
}

fn find_vlc_cached_artist_art(artist: &str) -> Option<Vec<u8>> {
    if artist.trim().is_empty() || artist.eq_ignore_ascii_case("VLC media player") {
        return None;
    }
    let appdata = std::env::var("APPDATA").ok()?;
    let art_root = PathBuf::from(appdata).join("vlc").join("art").join("artistalbum");
    let safe_artist = sanitize_vlc_dir_name(artist);
    let mut artist_dir = art_root.join(&safe_artist);
    if !artist_dir.is_dir() {
        let target_low = safe_artist.to_ascii_lowercase();
        if let Ok(entries) = std::fs::read_dir(&art_root) {
            for e in entries.flatten() {
                let name = e.file_name().to_string_lossy().to_ascii_lowercase();
                if name == target_low {
                    artist_dir = e.path();
                    break;
                }
            }
        }
    }
    if !artist_dir.is_dir() {
        return None;
    }

    let mut best: Option<(std::time::SystemTime, PathBuf)> = None;
    if let Ok(albums) = std::fs::read_dir(&artist_dir) {
        for album in albums.flatten() {
            let ap = album.path();
            if !ap.is_dir() {
                continue;
            }
            for fname in ["art.jpg", "art.png", "art.jpeg", "art.bmp"] {
                let cp = ap.join(fname);
                if let Ok(meta) = std::fs::metadata(&cp) {
                    let mod_t = meta.modified().unwrap_or(std::time::UNIX_EPOCH);
                    if best.as_ref().is_none_or(|(bt, _)| mod_t > *bt) {
                        best = Some((mod_t, cp));
                    }
                }
            }
        }
    }
    best.and_then(|(_, p)| std::fs::read(p).ok())
}

fn read_desktop_player_cover(
    kind: DesktopPlayerKind,
    raw_title: &str,
    title: &str,
    artist: &str,
) -> Option<Vec<u8>> {
    if kind == DesktopPlayerKind::Vlc {
        if let Some(media_path) = find_vlc_recent_matching_path(title, raw_title) {
            if let Some(bmp) = extract_shell_thumbnail_bmp(&media_path) {
                return Some(bmp);
            }
            if let Some(parent) = media_path.parent() {
                if let Some(fc) = find_folder_cover(parent) {
                    return Some(fc);
                }
            }
        }
        if let Some(art) = find_vlc_cached_artist_art(artist) {
            return Some(art);
        }
    }
    let pb = Path::new(title);
    if pb.exists() {
        if let Some(bmp) = extract_shell_thumbnail_bmp(pb) {
            return Some(bmp);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_stop_no_block_returns_immediately_even_if_workers_stall() {
        let gate = Arc::new((Mutex::new(false), Condvar::new()));
        let g1 = Arc::clone(&gate);
        let g2 = Arc::clone(&gate);
        let h1 = thread::spawn(move || {
            let guard = g1.0.lock().unwrap();
            let _wait_guard = g1.1.wait_while(guard, |release| !*release);
        });
        let h2 = thread::spawn(move || {
            let guard = g2.0.lock().unwrap();
            let _wait_guard = g2.1.wait_while(guard, |release| !*release);
        });
        let mut eng = SystemMediaEngine {
            shared: Arc::new(Mutex::new(SharedState {
                media: MediaSnapshot::default(),
                cc: CcSnapshot::default(),
                bands: [0.0; 12],
            })),
            tx: mpsc::channel().0,
            stop: Arc::new(AtomicBool::new(false)),
            stop_wake: Arc::new((Mutex::new(false), Condvar::new())),
            action_wake: Arc::new((Mutex::new(ActionSignal::default()), Condvar::new())),
            media_thread: Some(h1),
            audio_thread: Some(h2),
            cc_active: Arc::new(AtomicBool::new(false)),
            audio_outputs_active: Arc::new(AtomicBool::new(false)),
            audio_action_seq: Arc::new(AtomicU64::new(0)),
            viz_frame_ms: Arc::new(AtomicU32::new(22)),
            viz_needed: Arc::new(AtomicBool::new(true)),
        };
        let t0 = Instant::now();
        eng.request_stop_no_block();
        let dt_fast = t0.elapsed();
        assert!(dt_fast < Duration::from_millis(50), "fast stop stalled: {:?}", dt_fast);
        let t1 = Instant::now();
        eng.shutdown();
        assert!(t1.elapsed() < Duration::from_millis(20), "follow-up shutdown should be instant");
        // Release test worker threads cleanly after measurement.
        *gate.0.lock().unwrap() = true;
        gate.1.notify_all();
    }

    #[test]
    fn hann_goertzel_separates_bass_mid_treble_and_returns_zero_on_silence() {
        let sr = 48_000.0f32;
        let hann = build_hann_window();
        let coeffs = build_goertzel_coeffs(sr);

        // 1. Pure silence -> exact zero across all 12 bands
        let silent = [0.0f32; WASAPI_FFT_LEN];
        assert_eq!(
            compute_hann_goertzel_bands(&silent, 0, &hann, &coeffs, 0.2),
            [0.0; 12]
        );

        let make_sine = |freq_hz: f32| -> [f32; WASAPI_FFT_LEN] {
            let mut buf = [0.0f32; WASAPI_FFT_LEN];
            for (i, s) in buf.iter_mut().enumerate() {
                *s = 0.5 * (TAU * freq_hz * (i as f32) / sr).sin();
            }
            buf
        };

        // 2. 95 Hz bass tone -> peaks on band 1, treble stays silent
        let bass = compute_hann_goertzel_bands(&make_sine(95.0), 0, &hann, &coeffs, 0.5);
        assert!(bass[1] > 0.55, "95 Hz should strongly excite band 1: {:?}", bass);
        assert!(bass[10] < 0.05, "95 Hz must not leak into 9 kHz band: {:?}", bass);

        // 3. 1250 Hz mid tone -> peaks on band 6
        let mid = compute_hann_goertzel_bands(&make_sine(1250.0), 0, &hann, &coeffs, 0.5);
        assert!(mid[6] > 0.65, "1250 Hz should strongly excite band 6: {:?}", mid);
        assert!(mid[0] < 0.05 && mid[11] < 0.05, "1250 Hz should stay focused on mid band: {:?}", mid);

        // 4. 9000 Hz treble tone -> peaks on band 10, bass stays silent
        let treble = compute_hann_goertzel_bands(&make_sine(9000.0), 0, &hann, &coeffs, 0.5);
        assert!(treble[10] > 0.65, "9000 Hz should strongly excite band 10: {:?}", treble);
        assert!(treble[1] < 0.05, "9000 Hz must not leak into bass band: {:?}", treble);
    }

    #[test]
    fn volume_scaling_zero_is_silent_and_increases_monotonically_to_100() {
        let sr = 48_000.0f32;
        let hann = build_hann_window();
        let coeffs = build_goertzel_coeffs(sr);

        let make_sine = |amp: f32, freq_hz: f32| -> [f32; WASAPI_FFT_LEN] {
            let mut buf = [0.0f32; WASAPI_FFT_LEN];
            for (i, s) in buf.iter_mut().enumerate() {
                *s = amp * (TAU * freq_hz * (i as f32) / sr).sin();
            }
            buf
        };

        // 1. Volume 0% OR Muted at 100% must ALWAYS produce strict zero bands (never jump full up)
        let full_pcm = make_sine(0.75, 160.0);
        assert_eq!(
            compute_volume_scaled_bands(&full_pcm, 0, &hann, &coeffs, 0.75, 0.0, false),
            [0.0; 12]
        );
        assert_eq!(
            compute_volume_scaled_bands(&full_pcm, 0, &hann, &coeffs, 0.75, 1.0, true),
            [0.0; 12]
        );

        // 2. Pre-volume loopback driver (constant PCM amplitude): strictly increases 10% < 40% < 75% < 100%
        let b10 = compute_volume_scaled_bands(&full_pcm, 0, &hann, &coeffs, 0.75, 0.10, false)[2];
        let b40 = compute_volume_scaled_bands(&full_pcm, 0, &hann, &coeffs, 0.75, 0.40, false)[2];
        let b75 = compute_volume_scaled_bands(&full_pcm, 0, &hann, &coeffs, 0.75, 0.75, false)[2];
        let b100 = compute_volume_scaled_bands(&full_pcm, 0, &hann, &coeffs, 0.75, 1.00, false)[2];
        assert!(
            0.0 < b10 && b10 < b40 && b40 < b75 && b75 < b100,
            "Pre-volume loopback must scale monotonically with volume: 10%={b10}, 40%={b40}, 75%={b75}, 100%={b100}"
        );

        // 3. Post-volume loopback driver (PCM scales with volume): strictly increases 10% < 40% < 75% < 100%
        let p10 = make_sine(0.75 * 0.10, 160.0);
        let p40 = make_sine(0.75 * 0.40, 160.0);
        let p75 = make_sine(0.75 * 0.75, 160.0);
        let p100 = make_sine(0.75 * 1.00, 160.0);
        let v10 = compute_volume_scaled_bands(&p10, 0, &hann, &coeffs, (0.75 * 0.10_f32).max(0.08), 0.10, false)[2];
        let v40 = compute_volume_scaled_bands(&p40, 0, &hann, &coeffs, 0.75 * 0.40, 0.40, false)[2];
        let v75 = compute_volume_scaled_bands(&p75, 0, &hann, &coeffs, 0.75 * 0.75, 0.75, false)[2];
        let v100 = compute_volume_scaled_bands(&p100, 0, &hann, &coeffs, 0.75 * 1.00, 1.00, false)[2];
        assert!(
            0.0 < v10 && v10 < v40 && v40 < v75 && v75 < v100,
            "Post-volume loopback must scale monotonically (never reverse!): 10%={v10}, 40%={v40}, 75%={v75}, 100%={v100}"
        );
    }

    #[test]
    fn shorten_audio_device_name_formats_windows_endpoints_cleanly() {
        assert_eq!(
            shorten_audio_device_name("Speakers (Realtek(R) Audio)"),
            "Speakers"
        );
        assert_eq!(
            shorten_audio_device_name("Headphones (realme Buds Air7 Stereo)"),
            "realme Buds Air7"
        );
        assert_eq!(
            shorten_audio_device_name("Headset (realme Buds Air7 Hands-Free AG Audio)"),
            "realme Buds Air7 (HF)"
        );
        assert_eq!(
            shorten_audio_device_name("Headphones (2- Sony WH-1000XM5)"),
            "Sony WH-1000XM5"
        );
        assert_eq!(
            shorten_audio_device_name("DELL U2720Q (NVIDIA High Definition Audio)"),
            "DELL U2720Q"
        );
        assert_eq!(
            shorten_audio_device_name("Realtek Digital Output (Realtek(R) Audio)"),
            "Digital Output"
        );
        assert_eq!(
            shorten_audio_device_name("Speakers (JBL Flip 6)"),
            "JBL Flip 6"
        );
    }

    #[test]
    fn optimistic_audio_actions_update_snapshot_without_blocking() {
        let shared = Arc::new(Mutex::new(SharedState {
            media: MediaSnapshot::default(),
            cc: CcSnapshot {
                vol_pct: 40,
                vol_muted: false,
                audio_outputs: vec![
                    AudioOutputDevice {
                        id: "ep-spk".to_string(),
                        name: "Speakers".to_string(),
                        is_default: true,
                    },
                    AudioOutputDevice {
                        id: "ep-buds".to_string(),
                        name: "realme Buds Air7".to_string(),
                        is_default: false,
                    },
                ],
                ..CcSnapshot::default()
            },
            bands: [0.0; 12],
        }));
        let (tx, _rx) = mpsc::channel();
        let engine = SystemMediaEngine {
            shared: Arc::clone(&shared),
            tx,
            stop: Arc::new(AtomicBool::new(true)),
            stop_wake: Arc::new((Mutex::new(true), Condvar::new())),
            action_wake: Arc::new((Mutex::new(ActionSignal::default()), Condvar::new())),
            cc_active: Arc::new(AtomicBool::new(false)),
            audio_outputs_active: Arc::new(AtomicBool::new(true)),
            audio_action_seq: Arc::new(AtomicU64::new(0)),
            viz_frame_ms: Arc::new(AtomicU32::new(22)),
            viz_needed: Arc::new(AtomicBool::new(false)),
            media_thread: None,
            audio_thread: None,
        };

        engine.send_action(CcAction::SetVolume(72));
        let s1 = engine.cc_snapshot();
        assert_eq!(s1.vol_pct, 72);
        assert!(!s1.vol_muted);

        engine.send_action(CcAction::ToggleMute);
        let s2 = engine.cc_snapshot();
        assert!(s2.vol_muted);

        engine.send_action(CcAction::SetAudioOutput("ep-buds".to_string()));
        let s3 = engine.cc_snapshot();
        assert!(!s3.audio_outputs[0].is_default);
        assert!(s3.audio_outputs[1].is_default);
        assert_eq!(engine.audio_action_seq.load(Ordering::Acquire), 3);
    }

    #[test]
    fn desktop_player_full_pid_scan_is_throttled_when_idle() {
        assert!(!should_run_full_player_pid_scan(Duration::from_millis(350), false));
        assert!(!should_run_full_player_pid_scan(Duration::from_secs(11), false));
        assert!(should_run_full_player_pid_scan(Duration::from_secs(12), false));
        assert!(!should_run_full_player_pid_scan(Duration::from_secs(5), true));
        assert!(should_run_full_player_pid_scan(Duration::from_secs(6), true));
    }
}
