use std::sync::atomic::{AtomicBool, AtomicI32, AtomicI64, AtomicU8, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, RwLock};
use rand::Rng;
use tauri::{AppHandle, Emitter, Manager, State};

/// 전체화면 앱 실행 여부를 SHQueryUserNotificationState로 O(1) 판단
/// EnumWindows 순회 없이 Windows 셸이 관리하는 상태를 직접 조회
#[cfg(target_os = "windows")]
fn is_fullscreen_app_running() -> bool {
    #[link(name = "shell32")]
    extern "system" {
        fn SHQueryUserNotificationState(pquns: *mut u32) -> i32;
    }

    // QUNS_RUNNING_D3D_FULL_SCREEN = 3, QUNS_PRESENTATION_MODE = 4
    const QUNS_RUNNING_D3D_FULL_SCREEN: u32 = 3;
    const QUNS_PRESENTATION_MODE: u32 = 4;

    unsafe {
        let mut state: u32 = 0;
        let hr = SHQueryUserNotificationState(&mut state);
        if hr == 0 {
            state == QUNS_RUNNING_D3D_FULL_SCREEN || state == QUNS_PRESENTATION_MODE
        } else {
            false
        }
    }
}

#[cfg(not(target_os = "windows"))]
fn is_fullscreen_app_running() -> bool {
    false
}

/// 한 번의 EnumWindows 순회로 여러 모니터의 전체화면 여부를 동시에 체크
/// monitors 슬라이스와 같은 길이의 bool 배열 반환 (인덱스 대응)
/// is_fullscreen_app_running()이 true일 때만 호출하여 불필요한 EnumWindows 제거
#[cfg(target_os = "windows")]
fn check_fullscreen_all(monitors: &[MonitorInfo]) -> Vec<bool> {
    if monitors.is_empty() {
        return Vec::new();
    }

    #[link(name = "user32")]
    extern "system" {
        fn EnumWindows(
            lpEnumFunc: Option<unsafe extern "system" fn(isize, isize) -> i32>,
            lParam: isize,
        ) -> i32;
        fn IsWindowVisible(hwnd: isize) -> i32;
        fn GetWindowRect(hwnd: isize, rect: *mut [i32; 4]) -> i32;
        fn GetClassNameW(hwnd: isize, lpClassName: *mut u16, nMaxCount: i32) -> i32;
        fn GetWindowLongW(hwnd: isize, nIndex: i32) -> i32;
    }

    // 확장 스타일 상수
    const GWL_EXSTYLE: i32 = -20;
    const WS_EX_LAYERED: u32 = 0x00080000; // 투명/레이어드 창 (오버레이 앱)
    const WS_EX_TOOLWINDOW: u32 = 0x00000080; // 도구 창 (ALT+TAB 미표시 플로팅 창)

    // EnumWindows 콜백 컨텍스트 (모니터 목록과 결과 배열을 raw ptr로 공유)
    struct Ctx {
        monitors: *const MonitorInfo,
        count: usize,
        fullscreen: *mut bool,
    }

    unsafe extern "system" fn enum_proc(hwnd: isize, lparam: isize) -> i32 {
        let ctx = &*(lparam as *const Ctx);

        if IsWindowVisible(hwnd) == 0 {
            return 1;
        }

        // 시스템 창 제외 (바탕화면, 작업표시줄, UWP 시스템 UI)
        let mut cls = [0u16; 64];
        GetClassNameW(hwnd, cls.as_mut_ptr(), 64);
        fn cls_eq(buf: &[u16], name: &[u8]) -> bool {
            name.iter()
                .enumerate()
                .all(|(i, &b)| buf.get(i).copied() == Some(b as u16))
                && buf.get(name.len()).copied() == Some(0)
        }
        if cls_eq(&cls, b"Progman")
            || cls_eq(&cls, b"WorkerW")
            || cls_eq(&cls, b"Shell_TrayWnd")
            || cls_eq(&cls, b"Windows.UI.Core.CoreWindow")
        {
            return 1;
        }

        // 투명 오버레이 및 도구 창 제외
        let ex_style = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
        if ex_style & WS_EX_LAYERED != 0 || ex_style & WS_EX_TOOLWINDOW != 0 {
            return 1;
        }

        let mut r = [0i32; 4];
        if GetWindowRect(hwnd, &mut r) == 0 {
            return 1;
        }

        // 이 창이 덮는 모니터 표시 (한 번에 여러 모니터 처리)
        let monitors = std::slice::from_raw_parts(ctx.monitors, ctx.count);
        let fullscreen = std::slice::from_raw_parts_mut(ctx.fullscreen, ctx.count);
        let mut all_found = true;
        for (i, m) in monitors.iter().enumerate() {
            if !fullscreen[i] {
                let mr = m.x + m.width;
                let mb = m.y + m.height;
                if r[0] <= m.x && r[1] <= m.y && r[2] >= mr && r[3] >= mb {
                    fullscreen[i] = true;
                }
            }
            if !fullscreen[i] {
                all_found = false;
            }
        }
        // 모든 모니터가 전체화면으로 확정되면 조기 종료
        if all_found { 0 } else { 1 }
    }

    let mut result = vec![false; monitors.len()];
    unsafe {
        let ctx = Ctx {
            monitors: monitors.as_ptr(),
            count: monitors.len(),
            fullscreen: result.as_mut_ptr(),
        };
        EnumWindows(Some(enum_proc), &ctx as *const _ as isize);
    }
    result
}

#[cfg(not(target_os = "windows"))]
fn check_fullscreen_all(monitors: &[MonitorInfo]) -> Vec<bool> {
    vec![false; monitors.len()]
}

// 윈도우 논리 크기 상수 (set_size에서 Logical으로 설정하는 값)
const LOGICAL_WIN_W: f64 = 200.0;
const LOGICAL_WIN_H: f64 = 200.0;

#[derive(Clone, Default)]
struct MonitorInfo {
    scale_factor: f64, // f64를 먼저 배치하여 구조체 패딩 최소화
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    work_bottom: i32, // 작업영역 하단 Y 절대좌표 (물리 픽셀) = 작업표시줄 상단
}

/// 모니터의 전체 영역(rc_monitor)과 작업영역(rc_work)을 Win32 API로 직접 조회
/// SetWindowPos와 동일한 좌표 공간(물리 픽셀) 보장 — Tauri API 좌표와의 불일치 방지
#[cfg(target_os = "windows")]
fn get_monitor_rects(monitor_x: i32, monitor_y: i32) -> Option<([i32; 4], [i32; 4])> {
    #[link(name = "user32")]
    extern "system" {
        fn MonitorFromPoint(pt_x: i32, pt_y: i32, flags: u32) -> isize;
        fn GetMonitorInfoW(hmonitor: isize, lpmi: *mut MonitorInfoW) -> i32;
    }

    #[repr(C)]
    struct MonitorInfoW {
        cb_size: u32,
        rc_monitor: [i32; 4],
        rc_work: [i32; 4],
        dw_flags: u32,
    }

    const MONITOR_DEFAULTTONEAREST: u32 = 2;

    unsafe {
        let hmon = MonitorFromPoint(monitor_x + 1, monitor_y + 1, MONITOR_DEFAULTTONEAREST);
        if hmon == 0 {
            return None;
        }
        let mut mi = MonitorInfoW {
            cb_size: std::mem::size_of::<MonitorInfoW>() as u32,
            rc_monitor: [0; 4],
            rc_work: [0; 4],
            dw_flags: 0,
        };
        if GetMonitorInfoW(hmon, &mut mi) == 0 {
            return None;
        }
        Some((mi.rc_monitor, mi.rc_work)) // ([left,top,right,bottom], [left,top,right,bottom])
    }
}

#[cfg(not(target_os = "windows"))]
fn get_monitor_rects(_monitor_x: i32, _monitor_y: i32) -> Option<([i32; 4], [i32; 4])> {
    None
}

/// Per-Monitor DPI Awareness v2 설정
/// 이 설정이 없으면 Win32 API가 보조 모니터(DPI가 다른)의 좌표를 가상화하여
/// rc_monitor, rc_work 등의 값이 실제 물리 픽셀과 달라진다.
#[cfg(target_os = "windows")]
fn set_dpi_awareness() {
    #[link(name = "user32")]
    extern "system" {
        fn SetProcessDpiAwarenessContext(value: isize) -> i32;
    }
    // DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2 = -4
    const DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2: isize = -4;
    unsafe {
        // 이미 설정된 경우 실패하지만 무해 (WebView2가 먼저 설정할 수 있음)
        SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
}

#[cfg(not(target_os = "windows"))]
fn set_dpi_awareness() {}

/// Win32 EnumDisplayMonitors + GetMonitorInfoW + GetDpiForMonitor로
/// 모든 모니터의 경계·작업영역·DPI를 직접 수집
/// Tauri API를 경유하지 않으므로 SetWindowPos와 동일한 좌표 공간(물리 픽셀) 보장
#[cfg(target_os = "windows")]
fn enumerate_all_monitors() -> Vec<MonitorInfo> {
    #[link(name = "user32")]
    extern "system" {
        fn EnumDisplayMonitors(
            hdc: isize,
            lprc_clip: *const [i32; 4],
            lpfn_enum: Option<unsafe extern "system" fn(isize, isize, *mut [i32; 4], isize) -> i32>,
            dw_data: isize,
        ) -> i32;
        fn GetMonitorInfoW(hmonitor: isize, lpmi: *mut MonitorInfoW) -> i32;
    }

    #[link(name = "shcore")]
    extern "system" {
        fn GetDpiForMonitor(hmonitor: isize, dpi_type: u32, dpi_x: *mut u32, dpi_y: *mut u32) -> i32;
    }

    #[repr(C)]
    struct MonitorInfoW {
        cb_size: u32,
        rc_monitor: [i32; 4],
        rc_work: [i32; 4],
        dw_flags: u32,
    }

    struct Ctx {
        monitors: Vec<MonitorInfo>,
    }

    // MDT_EFFECTIVE_DPI = 0
    const MDT_EFFECTIVE_DPI: u32 = 0;

    unsafe extern "system" fn enum_callback(
        hmonitor: isize,
        _hdc: isize,
        _rect: *mut [i32; 4],
        lparam: isize,
    ) -> i32 {
        let ctx = &mut *(lparam as *mut Ctx);

        let mut mi = MonitorInfoW {
            cb_size: std::mem::size_of::<MonitorInfoW>() as u32,
            rc_monitor: [0; 4],
            rc_work: [0; 4],
            dw_flags: 0,
        };
        if GetMonitorInfoW(hmonitor, &mut mi) == 0 {
            return 1; // 실패해도 열거 계속
        }

        // Per-Monitor DPI 취득 (실패 시 96 DPI = 100% 스케일)
        let mut dpi_x: u32 = 96;
        let mut dpi_y: u32 = 96;
        GetDpiForMonitor(hmonitor, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y);
        let scale = dpi_x as f64 / 96.0;

        ctx.monitors.push(MonitorInfo {
            x: mi.rc_monitor[0],
            y: mi.rc_monitor[1],
            width: mi.rc_monitor[2] - mi.rc_monitor[0],
            height: mi.rc_monitor[3] - mi.rc_monitor[1],
            scale_factor: scale,
            work_bottom: mi.rc_work[3],
        });
        1 // 열거 계속
    }

    let mut ctx = Ctx { monitors: Vec::new() };
    unsafe {
        EnumDisplayMonitors(
            0,
            std::ptr::null(),
            Some(enum_callback),
            &mut ctx as *mut Ctx as isize,
        );
    }
    ctx.monitors
}

#[cfg(not(target_os = "windows"))]
fn enumerate_all_monitors() -> Vec<MonitorInfo> {
    Vec::new()
}

/// AC 전원 연결 여부를 Win32 GetSystemPowerStatus API로 직접 확인
/// starship_battery의 State가 Unknown을 반환하는 환경에서도 안정적으로 동작
#[cfg(target_os = "windows")]
fn is_ac_connected() -> bool {
    #[repr(C)]
    struct SystemPowerStatus {
        ac_line_status: u8,
        battery_flag: u8,
        battery_life_percent: u8,
        system_status_flag: u8,
        battery_life_time: u32,
        battery_full_life_time: u32,
    }

    #[link(name = "kernel32")]
    extern "system" {
        fn GetSystemPowerStatus(lp: *mut SystemPowerStatus) -> i32;
    }

    unsafe {
        let mut status = std::mem::zeroed::<SystemPowerStatus>();
        if GetSystemPowerStatus(&mut status) != 0 {
            status.ac_line_status == 1
        } else {
            false
        }
    }
}

#[cfg(not(target_os = "windows"))]
fn is_ac_connected() -> bool {
    false
}
use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, TrayIconBuilder, TrayIconEvent},
};

struct AppState {
    is_hovered: Arc<AtomicBool>,
    test_cpu: Arc<AtomicI32>,
    polling_interval_ms: Arc<AtomicU64>,
    /// 펫별 이동 속도 배율 (1.0 = 기본, 0.7 = 30% 느림)
    pet_speed_factor: Arc<AtomicU32>,
    /// 이동 모드 (0=기본, 1=등반, 2+=추후 확장)
    move_mode: Arc<AtomicU8>,
    /// 펫 스프라이트의 실제 렌더링 너비 (CSS px, 경계 판정용)
    pet_visual_w: Arc<AtomicI32>,
    /// 펫 높이 오프셋 (-10~10, 양수=위, 음수=아래)
    pet_height_offset: Arc<AtomicI32>,
    /// 마우스 사용 여부 (false이면 캐릭터 윈도우 클릭 투과)
    mouse_enabled: Arc<AtomicBool>,
}

/// 펫 스프라이트의 실제 렌더링 너비 갱신 (CSS px 단위)
#[tauri::command]
fn update_pet_visual_w(state: State<'_, AppState>, width: i32) {
    state.pet_visual_w.store(width, Ordering::Relaxed);
}

/// 이동 모드 변경 (0=기본, 1=등반, 2+=추후 확장)
#[tauri::command]
fn update_move_mode(app: AppHandle, state: State<'_, AppState>, mode: u8) {
    state.move_mode.store(mode, Ordering::Relaxed);
    let _ = app.emit("move-mode-update", mode);
}

/// 모니터링 메시지 동기화 (설정 → 메인 윈도우 이벤트 릴레이)
#[tauri::command]
fn update_messages(app: AppHandle, messages: serde_json::Value) {
    let _ = app.emit("messages-update", messages);
}

/// 메시지 순환 표시 설정 동기화 (설정 → 메인 윈도우 이벤트 릴레이)
#[tauri::command]
fn update_msg_rotate(app: AppHandle, show_all: bool, interval: u32) {
    let _ = app.emit("msg-rotate-update", serde_json::json!({
        "showAll": show_all,
        "interval": interval
    }));
}

/// 프론트엔드 렌더링 완료 후 메인 윈도우 표시 (검은 창 깜빡임 방지)
#[tauri::command]
fn show_main_window(app: AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.set_always_on_top(true);
    }
}

#[tauri::command]
fn set_hover(state: State<'_, AppState>, hovered: bool) {
    state.is_hovered.store(hovered, Ordering::Relaxed);
}

#[tauri::command]
fn set_test_cpu(app: AppHandle, state: State<'_, AppState>, usage: i32) {
    state.test_cpu.store(usage, Ordering::Relaxed);
    // 테스트 모드 상태를 메인 윈도우에 동기화
    let _ = app.emit("test-mode-sync", usage);
}

/// 펫 종류 변경을 설정 윈도우 → 메인 윈도우로 동기화 + 이동 속도 배율 갱신
#[tauri::command]
fn update_pet_type(app: AppHandle, state: State<'_, AppState>, pet_id: String, speed_factor: f32, user_speed: f32) {
    // 속도 배율 = 펫 고유 속도 × 사용자 속도 배율
    let combined = speed_factor * user_speed;
    state.pet_speed_factor.store(combined.to_bits(), Ordering::Relaxed);
    let _ = app.emit("pet-type-update", &pet_id);
}

/// 펫 크기 변경을 설정 윈도우 → 메인 윈도우로 동기화
#[tauri::command]
fn update_pet_scale(app: AppHandle, pet_id: String, scale: u32) {
    let _ = app.emit("pet-scale-update", serde_json::json!({
        "petId": pet_id,
        "scale": scale
    }));
}

/// 펫 속도 변경을 설정 윈도우 → 메인 윈도우로 동기화 + 이동 속도 갱신
#[tauri::command]
fn update_pet_speed(app: AppHandle, state: State<'_, AppState>, pet_id: String, speed_factor: f32, user_speed: f32) {
    let combined = speed_factor * user_speed;
    state.pet_speed_factor.store(combined.to_bits(), Ordering::Relaxed);
    let _ = app.emit("pet-speed-update", serde_json::json!({
        "petId": pet_id,
        "userSpeed": user_speed
    }));
}

/// 펫 높이 오프셋 변경을 설정 윈도우 → 메인 윈도우로 동기화
#[tauri::command]
fn update_pet_height(app: AppHandle, state: State<'_, AppState>, pet_id: String, offset: i32) {
    state.pet_height_offset.store(offset, Ordering::Relaxed);
    let _ = app.emit("pet-height-update", serde_json::json!({
        "petId": pet_id,
        "offset": offset
    }));
}

#[tauri::command]
fn update_pet_color(app: AppHandle, hue: i32, saturation: i32, brightness: i32, opacity: i32) {
    let _ = app.emit(
        "color-update",
        serde_json::json!({
            "hue": hue,
            "saturation": saturation,
            "brightness": brightness,
            "opacity": opacity
        }),
    );
}

#[tauri::command]
fn set_polling_interval(state: State<'_, AppState>, seconds: u64) {
    let ms = if seconds == 0 { 1000 } else { seconds * 1000 };
    state.polling_interval_ms.store(ms, Ordering::Relaxed);
}

/// 알림 목록을 설정 윈도우 → 메인 윈도우로 동기화
#[tauri::command]
fn update_alarm_list(app: AppHandle, alarms: serde_json::Value) {
    let _ = app.emit("alarm-list-update", alarms);
}

/// 표시 설정(모니터링/알림 문구 표시 여부, 알림 표시 시간)을 동기화
#[tauri::command]
fn update_display_config(app: AppHandle, show_monitoring: bool, show_notification: bool, notification_priority: bool, notification_mode: String, notification_duration: u32) {
    let _ = app.emit("display-config-update", serde_json::json!({
        "showMonitoringText": show_monitoring,
        "showNotificationText": show_notification,
        "notificationPriority": notification_priority,
        "notificationMode": notification_mode,
        "notificationDuration": notification_duration
    }));
}

#[tauri::command]
fn update_mouse_enabled(app: AppHandle, state: State<'_, AppState>, enabled: bool) {
    state.mouse_enabled.store(enabled, Ordering::Relaxed);
    let _ = app.emit("mouse-enabled-update", enabled);
}

#[tauri::command]
fn update_bubble_enabled(app: AppHandle, enabled: bool) {
    let _ = app.emit("bubble-enabled-update", enabled);
}

#[tauri::command]
fn update_bubble_side(app: AppHandle, enabled: bool) {
    let _ = app.emit("bubble-side-update", enabled);
}

#[tauri::command]
fn update_bubble_top(app: AppHandle, enabled: bool) {
    let _ = app.emit("bubble-top-update", enabled);
}

#[tauri::command]
fn update_bubble_height(app: AppHandle, height: u32) {
    let _ = app.emit("bubble-height-update", height);
}

/// 타이머 상태 동기화 (설정 → 메인 윈도우)
#[tauri::command]
fn update_timer_state(app: AppHandle, running: bool, end_at: f64) {
    let _ = app.emit("timer-state-update", serde_json::json!({
        "running": running,
        "endAt": end_at
    }));
}

/// 타이머 폰트 크기 동기화 (설정 → 메인 윈도우)
#[tauri::command]
fn update_timer_font_size(app: AppHandle, size: u32) {
    let _ = app.emit("timer-font-size-update", size);
}

/// 폰트/언어 설정 동기화 (설정 → 메인 윈도우)
#[tauri::command]
fn update_app_settings(app: AppHandle, language: String, font_size: u32, font_family: String, monitoring_font_color: String, alarm_font_color: String) {
    let _ = app.emit("app-settings-update", serde_json::json!({
        "language": language,
        "fontSize": font_size,
        "fontFamily": font_family,
        "monitoringFontColor": monitoring_font_color,
        "alarmFontColor": alarm_font_color
    }));
}

#[tauri::command]
fn update_monitor_config(app: AppHandle, cpu: bool, memory: bool, network: bool, battery: bool, show_charging_icon: bool, charging_icon_size: String, charging_icon_distance: i32) {
    let _ = app.emit(
        "monitor-config-update",
        serde_json::json!({
            "cpu": cpu,
            "memory": memory,
            "network": network,
            "battery": battery,
            "showChargingIcon": show_charging_icon,
            "chargingIconSize": charging_icon_size,
            "chargingIconDistance": charging_icon_distance
        }),
    );
}

/// 자동 실행 레지스트리 조회 (async로 메인 스레드 블로킹 방지)
#[tauri::command]
async fn get_auto_start() -> bool {
    #[cfg(target_os = "windows")]
    {
        use std::process::Command;
        let output = Command::new("reg")
            .args(["query", r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run", "/v", "TaskMon"])
            .output();
        matches!(output, Ok(o) if o.status.success())
    }
    #[cfg(not(target_os = "windows"))]
    { false }
}

/// 자동 실행 레지스트리 설정/해제 (async로 메인 스레드 블로킹 방지)
#[tauri::command]
async fn set_auto_start(enabled: bool) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        use std::process::Command;
        if enabled {
            // 현재 실행 파일 경로를 레지스트리에 등록
            let exe = std::env::current_exe().map_err(|e| e.to_string())?;
            let exe_path = exe.to_string_lossy().to_string();
            let output = Command::new("reg")
                .args(["add", r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run", "/v", "TaskMon", "/t", "REG_SZ", "/d", &exe_path, "/f"])
                .output()
                .map_err(|e| e.to_string())?;
            if !output.status.success() {
                return Err(String::from_utf8_lossy(&output.stderr).to_string());
            }
        } else {
            let output = Command::new("reg")
                .args(["delete", r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run", "/v", "TaskMon", "/f"])
                .output()
                .map_err(|e| e.to_string())?;
            if !output.status.success() {
                return Err(String::from_utf8_lossy(&output.stderr).to_string());
            }
        }
        Ok(())
    }
    #[cfg(not(target_os = "windows"))]
    { Ok(()) }
}

/// 설정 윈도우 열기 (이미 열려있으면 포커스)
fn open_or_focus_settings(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("settings") {
        let _ = w.set_focus();
    } else {
        let _ = tauri::webview::WebviewWindowBuilder::new(
            app,
            "settings",
            tauri::WebviewUrl::App("index.html".into()),
        )
        .title("설정")
        .inner_size(850.0, 600.0)
        .min_inner_size(640.0, 600.0)
        .decorations(true)
        .resizable(true)
        .center()
        .skip_taskbar(false)
        .visible(false)
        .build();
        if let Some(w) = app.get_webview_window("settings") {
            let w2 = w.clone();
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis(150));
                let _ = w2.show();
                let _ = w2.set_focus();
            });
        }
    }
}

/// 현재 실행 상태에 따라 트레이 메뉴를 동적으로 빌드
fn build_tray_menu(app: &AppHandle, running: bool) -> tauri::Result<Menu<tauri::Wry>> {
    let quit_i = MenuItem::with_id(app, "quit", "종료", true, None::<&str>)?;
    let settings_i = MenuItem::with_id(app, "settings", "설정", true, None::<&str>)?;

    if running {
        let stop_i = MenuItem::with_id(app, "stop", "중지", true, None::<&str>)?;
        Menu::with_items(app, &[&settings_i, &stop_i, &quit_i])
    } else {
        let start_i = MenuItem::with_id(app, "start", "시작", true, None::<&str>)?;
        Menu::with_items(app, &[&settings_i, &start_i, &quit_i])
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // 윈도우 생성 전에 Per-Monitor DPI Awareness v2 설정 (최우선 실행)
    set_dpi_awareness();

    let app_state_hover = Arc::new(AtomicBool::new(false));
    let thread_hover_state = Arc::clone(&app_state_hover);

    // 실행 상태 플래그: true = 실행 중, false = 중지
    // 이 플래그 하나로 CPU 폴링 스레드와 이동 스레드를 동시에 제어한다.
    let is_running = Arc::new(AtomicBool::new(true));

    // 테스트용 CPU 값: -1 = 실제 시스템 값 사용, 0~100 = 테스트 값 사용
    let test_cpu = Arc::new(AtomicI32::new(-1));

    // 폴링 간격 (밀리초): 기본 1초
    let polling_interval_ms = Arc::new(AtomicU64::new(1000));

    // 캐릭터의 현재 물리 X 좌표를 Thread 2 → Thread 1 방향으로 공유 (전체화면 감지에 사용)
    let shared_pet_x = Arc::new(AtomicI64::new(0));

    // 펫이 전체화면 모니터 위에 있는지 여부: Thread 1(갱신) → Thread 2(참조)
    let shared_on_fullscreen = Arc::new(AtomicBool::new(false));

    // Thread 1 → Thread 2: 모니터 변경 시 텔레포트 좌표 (i64::MIN = 텔레포트 불필요)
    let shared_teleport_x = Arc::new(AtomicI64::new(i64::MIN));

    // Thread 1 → Thread 2: 모니터 변경 시 렌더링 갱신 요청 플래그
    let shared_needs_redraw = Arc::new(AtomicBool::new(false));

    // setup 클로저(move)에 넘길 clone 미리 준비
    let is_running_tray = Arc::clone(&is_running);
    let shared_pet_x_t1 = Arc::clone(&shared_pet_x);
    let shared_pet_x_t2 = Arc::clone(&shared_pet_x);
    let on_fullscreen_t1 = Arc::clone(&shared_on_fullscreen);
    let on_fullscreen_t2 = Arc::clone(&shared_on_fullscreen);
    let teleport_x_t1 = Arc::clone(&shared_teleport_x);
    let teleport_x_t2 = Arc::clone(&shared_teleport_x);
    let needs_redraw_t1 = Arc::clone(&shared_needs_redraw);
    let needs_redraw_t2 = Arc::clone(&shared_needs_redraw);

    // 펫별 이동 속도 배율 (기본 1.0)
    let pet_speed_factor = Arc::new(AtomicU32::new(1.0f32.to_bits()));
    let pet_speed_factor_t2 = Arc::clone(&pet_speed_factor);

    // 이동 모드 (0=기본, 1=등반)
    let shared_move_mode = Arc::new(AtomicU8::new(0));
    let move_mode_t2 = Arc::clone(&shared_move_mode);

    // 펫 스프라이트 렌더링 너비 (CSS px, 기본 64)
    let shared_pet_visual_w = Arc::new(AtomicI32::new(64));
    let pet_visual_w_t2 = Arc::clone(&shared_pet_visual_w);

    // 펫 높이 오프셋 (-10~10, 양수=위, 음수=아래)
    let shared_pet_height_offset = Arc::new(AtomicI32::new(0));
    let pet_height_offset_t2 = Arc::clone(&shared_pet_height_offset);

    // 마우스 사용 여부 (기본 true, 설정에서 변경 시 반영)
    let shared_mouse_enabled = Arc::new(AtomicBool::new(true));
    let mouse_enabled_t2 = Arc::clone(&shared_mouse_enabled);

    tauri::Builder::default()
        .manage(AppState {
            is_hovered: app_state_hover,
            test_cpu: Arc::clone(&test_cpu),
            polling_interval_ms: Arc::clone(&polling_interval_ms),
            pet_speed_factor,
            move_mode: Arc::clone(&shared_move_mode),
            pet_visual_w: Arc::clone(&shared_pet_visual_w),
            pet_height_offset: Arc::clone(&shared_pet_height_offset),
            mouse_enabled: Arc::clone(&shared_mouse_enabled),
        })
        .plugin(tauri_plugin_single_instance::init(|_app, _argv, _cwd| {
            // 이미 실행 중인 인스턴스가 있으면 새 프로세스는 자동 종료됨
        }))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        // move: is_running_tray/t1/t2, thread_hover_state 를 클로저 안으로 이동
        .setup(move |app| {
            // 트레이 메뉴 초기 빌드 (실행 중 상태 → 중지 메뉴 표시)
            let menu = build_tray_menu(app.handle(), true)?;

            if let Some(icon) = app.default_window_icon().cloned() {
                // 시스템 언어에 따라 트레이 툴팁 설정
                let tooltip = if sys_locale::get_locale()
                    .unwrap_or_default()
                    .starts_with("ko")
                {
                    "테스크몬"
                } else {
                    "TaskMon"
                };

                TrayIconBuilder::with_id("main-tray")
                    .icon(icon)
                    .menu(&menu)
                    .tooltip(tooltip)
                    .show_menu_on_left_click(false)
                    .on_menu_event(move |app, event| {
                        match event.id().as_ref() {
                            "settings" => {
                                open_or_focus_settings(app);
                            }
                            "quit" => {
                                app.exit(0);
                            }
                            "stop" => {
                                // 1. 실행 플래그 끄기 → 두 스레드 모두 loop 진입 시 로직 건너뜀
                                is_running_tray.store(false, Ordering::Relaxed);

                                // 2. 창 숨기기
                                if let Some(w) = app.get_webview_window("main") {
                                    let _ = w.hide();
                                }

                                // 3. 트레이 메뉴 재빌드 (시작 메뉴만 표시)
                                if let Ok(new_menu) = build_tray_menu(app, false) {
                                    if let Some(tray) = app.tray_by_id("main-tray") {
                                        let _ = tray.set_menu(Some(new_menu));
                                    }
                                }
                            }
                            "start" => {
                                // 1. 실행 플래그 켜기 → 두 스레드 모두 다음 루프부터 동작 재개
                                is_running_tray.store(true, Ordering::Relaxed);

                                // 2. 창 표시 및 최상위 레이어 재적용
                                if let Some(w) = app.get_webview_window("main") {
                                    let _ = w.show();
                                    let _ = w.set_always_on_top(true);
                                }

                                // 3. 트레이 메뉴 재빌드 (중지 메뉴만 표시)
                                if let Ok(new_menu) = build_tray_menu(app, true) {
                                    if let Some(tray) = app.tray_by_id("main-tray") {
                                        let _ = tray.set_menu(Some(new_menu));
                                    }
                                }
                            }
                            _ => {}
                        }
                    })
                    // 트레이 아이콘 더블클릭 시 설정 창 열기
                    .on_tray_icon_event(|tray, event| {
                        if let TrayIconEvent::DoubleClick { button: MouseButton::Left, .. } = event {
                            open_or_focus_settings(tray.app_handle());
                        }
                    })
                    .build(app)?;
            }

            // Get the main window
            let window = app.get_webview_window("main").unwrap();

            // Get screen details to know when to wrap around
            // Use logical size and position for consistent cross-DPI behavior
            let monitor = window.primary_monitor().unwrap().unwrap();
            let scale_factor = window.scale_factor().unwrap_or(1.0);

            // Set initial window size (width for bubble space, height for pet + bubble)
            window
                .set_size(tauri::Size::Logical(tauri::LogicalSize {
                    width: LOGICAL_WIN_W,
                    height: LOGICAL_WIN_H,
                }))
                .expect("Failed to set window size");

            // 초기 위치: 실제 작업영역(work area) 하단에 맞춤
            let mon_x = monitor.position().x;
            let mon_y = monitor.position().y;
            let mon_h = monitor.size().height as i32;
            let work_bottom = get_monitor_rects(mon_x, mon_y)
                .map(|(_, rc_work)| rc_work[3])
                .unwrap_or(mon_y + mon_h);
            // set_size 후 실제 물리 높이 조회 (150 * scale 추정값 대신 OS가 보고하는 정확한 값)
            let actual_init_h = window
                .outer_size()
                .map(|s| s.height as i32)
                .unwrap_or((LOGICAL_WIN_H * scale_factor) as i32);
            let initial_y = work_bottom - actual_init_h;
            window
                .set_position(tauri::Position::Physical(
                    tauri::PhysicalPosition::new(0, initial_y),
                ))
                .unwrap();

            // 작업 표시줄 아이콘 숨기기 (tauri.conf.json의 skipTaskbar와 함께 이중 적용)
            let _ = window.set_skip_taskbar(true);
            // alwaysOnTop을 코드에서 명시적으로 강제 적용 (conf 설정만으로는 일부 앱에 가려지는 문제 방지)
            let _ = window.set_always_on_top(true);

            let window_clone = window.clone(); // Clone for the thread
            let mut current_x = 0.0;

            // Get initial min_x to start from the far left monitor
            if let Ok(monitors) = window.available_monitors() {
                let mut min_x = i32::MAX;
                for m in monitors {
                    if m.position().x < min_x {
                        min_x = m.position().x;
                    }
                }
                if min_x != i32::MAX {
                    current_x = min_x as f64 - 100.0;
                }
            }

            let mut last_update = std::time::Instant::now();

            // 글로벌 공유 CPU 상태 (f32를 원자적으로 다루기 위해 bits 변환해서 AtomicU32 사용)
            let shared_cpu_usage = Arc::new(AtomicU32::new(0f32.to_bits()));
            // 추가: 글로벌 공유 모니터 정보 캐시 (1초 갱신, Arc로 감싸 clone 비용 제거)
            let shared_monitors: Arc<RwLock<Arc<Vec<MonitorInfo>>>> = Arc::new(RwLock::new(Arc::new(Vec::new())));

            // 초기 모니터 정보 캐싱 (첫 1초 동안 이동 가능하도록)
            // Win32 EnumDisplayMonitors + GetDpiForMonitor로 직접 취득 — SetWindowPos와 동일 좌표 공간 보장
            {
                let info_list = enumerate_all_monitors();
                if let Ok(mut cache) = shared_monitors.write() {
                    *cache = Arc::new(info_list);
                }
            }

            // --- Thread 1: CPU 폴링 & 모니터 스캔 (1초에 1번) ---
            // 중지 상태일 때는 sleep만 하고 실제 폴링/이벤트 전송을 건너뜀 → CPU 점유 없음
            let cpu_usage_clone = Arc::clone(&shared_cpu_usage);
            let monitors_clone = Arc::clone(&shared_monitors);
            let window_clone_evt = window.clone();
            let is_running_t1 = Arc::clone(&is_running);
            let pet_x_reader = Arc::clone(&shared_pet_x_t1);
            let on_fs_writer = Arc::clone(&on_fullscreen_t1);
            let teleport_writer = Arc::clone(&teleport_x_t1);
            let redraw_writer = Arc::clone(&needs_redraw_t1);
            let polling_ms_t1 = Arc::clone(&polling_interval_ms);
            std::thread::spawn(move || {
                let mut sys = sysinfo::System::new();
                sys.refresh_memory();
                let mut prev_on_fullscreen = false; // set_always_on_top 중복 호출 방지
                let mut prev_monitor_count: usize = 0; // 모니터 수 변경 감지용

                // 모니터 + 전체화면 갱신 주기 (10초마다, 첫 폴링은 즉시)
                let mut monitor_refresh_tick: u32 = 0;

                // 네트워크 모니터링 초기화
                let mut networks = sysinfo::Networks::new_with_refreshed_list();
                let mut network_refresh_tick: u32 = 0;

                // 배터리 모니터링 초기화
                let battery_manager = starship_battery::Manager::new().ok();
                let mut battery_elapsed_ms: u64 = 178_000; // 첫 폴링을 ~2초 후에 트리거 (180000-178000=2000ms 후)
                let mut cached_battery_percent: i32 = -1; // 배터리 잔량 캐시 (-1 = 배터리 없음)
                let mut prev_charging = false; // 이전 충전 상태 (변경 감지용)

                loop {
                    // 중지 상태: 500ms 대기 후 다음 루프 (CPU/메모리 점유 없음)
                    if !is_running_t1.load(Ordering::Relaxed) {
                        std::thread::sleep(std::time::Duration::from_millis(500));
                        continue;
                    }

                    // 1. CPU 폴링
                    sys.refresh_cpu_usage();
                    let cpus = sys.cpus();
                    let usage = if !cpus.is_empty() {
                        cpus.iter().map(|cpu| cpu.cpu_usage()).sum::<f32>() / cpus.len() as f32
                    } else {
                        0.0
                    };

                    // 메모리 폴링
                    sys.refresh_memory();
                    let total_mem = sys.total_memory();
                    let used_mem = sys.used_memory();
                    let mem_pct = if total_mem > 0 {
                        (used_mem as f64 / total_mem as f64 * 100.0).round() as u32
                    } else {
                        0
                    };

                    cpu_usage_clone.store(usage.to_bits(), Ordering::Relaxed);

                    // 2. 모니터 정보 (10초마다)
                    // Win32 EnumDisplayMonitors + GetDpiForMonitor로 직접 수집 — Tauri API 미경유
                    if monitor_refresh_tick == 0 {
                        let new_monitors = enumerate_all_monitors();
                        if let Ok(mut cache) = monitors_clone.write() {
                            *cache = Arc::new(new_monitors);
                        }
                    }
                    monitor_refresh_tick = (monitor_refresh_tick + 1) % 10;

                    // monitors 락을 짧게 잡아 Arc 참조만 복사 → 락 해제 후 EnumWindows 실행
                    let monitors_snapshot = monitors_clone.read()
                        .map(|g| Arc::clone(&g))
                        .unwrap_or_else(|_| Arc::new(Vec::new()));

                    // 모니터 수 변경 감지: 모니터 분리/연결 시 윈도우 복구
                    let cur_monitor_count = monitors_snapshot.len();
                    if prev_monitor_count != 0 && cur_monitor_count != prev_monitor_count {
                        // 펫이 유효한 모니터 범위 내에 있는지 확인
                        let pet_x_val = pet_x_reader.load(Ordering::Relaxed);
                        let in_bounds = monitors_snapshot.iter().any(|m| {
                            pet_x_val >= (m.x - 200) as i64
                                && pet_x_val < (m.x + m.width) as i64
                        });
                        if !in_bounds {
                            if let Some(first) = monitors_snapshot.first() {
                                // Thread 2에 텔레포트 좌표 전달
                                teleport_writer.store(first.x as i64, Ordering::Relaxed);
                            }
                        }
                        // OS가 윈도우를 숨겼을 수 있으므로 강제 표시 및 최상위 재적용
                        let _ = window_clone_evt.show();
                        let _ = window_clone_evt.set_always_on_top(true);
                        // 윈도우 크기를 재설정하여 WebView 렌더링 표면 갱신 강제 트리거
                        // (모니터 연결/해제 시 GPU 컨텍스트 손실로 인한 투명도 깨짐 복구)
                        let _ = window_clone_evt.set_size(tauri::Size::Logical(tauri::LogicalSize {
                            width: LOGICAL_WIN_W,
                            height: LOGICAL_WIN_H,
                        }));
                        // Thread 2에 렌더링 갱신 요청 (SetWindowPos + RedrawWindow)
                        redraw_writer.store(true, Ordering::Relaxed);
                    }
                    prev_monitor_count = cur_monitor_count;

                    // 전체화면 감지: 하이브리드 방식
                    // 1) SHQueryUserNotificationState O(1) 프리체크
                    // 2) true일 때만 EnumWindows로 펫이 있는 모니터에 전체화면 앱이 있는지 확인
                    let pet_on_fullscreen = if is_fullscreen_app_running() {
                        // 시스템에 전체화면 앱 존재 → 펫이 있는 모니터인지 확인
                        let pet_x = pet_x_reader.load(Ordering::Relaxed);
                        let half_w = monitors_snapshot.iter()
                            .find(|m| pet_x >= m.x as i64 && pet_x < (m.x + m.width) as i64)
                            .map(|m| (LOGICAL_WIN_W / 2.0 * m.scale_factor) as i64)
                            .unwrap_or((LOGICAL_WIN_W / 2.0) as i64);
                        let center_x = pet_x + half_w;
                        let fs_flags = check_fullscreen_all(&monitors_snapshot);
                        monitors_snapshot.iter().zip(fs_flags.iter()).any(|(m, &fs)| {
                            center_x >= m.x as i64 && center_x < (m.x + m.width) as i64 && fs
                        })
                    } else {
                        false
                    };

                    // Thread 2가 SetWindowPos에서 HWND_TOPMOST/NOTOPMOST를 판단할 수 있도록 공유
                    on_fs_writer.store(pet_on_fullscreen, Ordering::Relaxed);

                    // 전체화면 상태 전환 시에만 Tauri API로 즉시 반영 (Thread 2의 다음 프레임까지 대기 방지)
                    if pet_on_fullscreen && !prev_on_fullscreen {
                        let _ = window_clone_evt.set_always_on_top(false);
                    } else if !pet_on_fullscreen && prev_on_fullscreen {
                        let _ = window_clone_evt.set_always_on_top(true);
                    }
                    prev_on_fullscreen = pet_on_fullscreen;

                    let _ = window_clone_evt.emit("cpu-usage", usage);
                    let _ = window_clone_evt.emit("memory-usage", mem_pct);

                    // 네트워크: 1초간 수신/송신 바이트 (refresh 간격 = 1초이므로 곧 bytes/sec)
                    // 30초마다 인터페이스 목록 재구성, 그 외에는 통계만 갱신
                    networks.refresh(network_refresh_tick == 0);
                    network_refresh_tick = (network_refresh_tick + 1) % 30;
                    let mut down_bytes: u64 = 0;
                    let mut up_bytes: u64 = 0;
                    for (_name, data) in networks.iter() {
                        down_bytes += data.received();
                        up_bytes += data.transmitted();
                    }
                    let _ = window_clone_evt.emit("network-usage", serde_json::json!({
                        "down": down_bytes,
                        "up": up_bytes
                    }));

                    let interval = polling_ms_t1.load(Ordering::Relaxed);

                    // 배터리 잔량: 최소 3분 간격, 폴링 간격이 3분 이상이면 폴링 간격으로 체크
                    let battery_check_ms = std::cmp::max(180_000u64, interval);
                    battery_elapsed_ms += interval;
                    let mut battery_just_checked = false;
                    if battery_elapsed_ms >= battery_check_ms {
                        battery_elapsed_ms = 0;
                        battery_just_checked = true;
                        if let Some(ref manager) = battery_manager {
                            if let Ok(mut batteries) = manager.batteries() {
                                if let Some(Ok(bat)) = batteries.next() {
                                    cached_battery_percent = (bat.state_of_charge().get::<starship_battery::units::ratio::percent>()) as i32;
                                }
                            }
                        }
                    }

                    // 충전 상태: 매 폴링마다 확인 (GetSystemPowerStatus는 매우 가벼움)
                    // 변경 시 또는 배터리 잔량 갱신 시에만 이벤트 발송
                    if cached_battery_percent >= 0 {
                        let charging = is_ac_connected();
                        if charging != prev_charging || battery_just_checked {
                            prev_charging = charging;
                            let _ = window_clone_evt.emit("battery-usage", serde_json::json!({
                                "percent": cached_battery_percent,
                                "charging": charging
                            }));
                        }
                    }
                    std::thread::sleep(std::time::Duration::from_millis(interval));
                }
            });

            // --- Thread 2: 윈도우 이동 및 프레임(16ms) 업데이트 ---
            // 중지 상태일 때는 sleep만 하고 set_position 호출 없음 → GPU/CPU 점유 없음
            let cpu_usage_reader = Arc::clone(&shared_cpu_usage);
            let all_monitors_reader = Arc::clone(&shared_monitors);
            let is_running_t2 = Arc::clone(&is_running);
            let pet_x_writer = Arc::clone(&shared_pet_x_t2);
            let test_cpu_t2 = Arc::clone(&test_cpu);
            let on_fs_reader = Arc::clone(&on_fullscreen_t2);
            let teleport_reader = Arc::clone(&teleport_x_t2);
            let redraw_reader = Arc::clone(&needs_redraw_t2);
            let speed_factor_reader = Arc::clone(&pet_speed_factor_t2);
            let move_mode_reader = Arc::clone(&move_mode_t2);
            let pet_visual_w_reader = Arc::clone(&pet_visual_w_t2);
            let pet_height_reader = Arc::clone(&pet_height_offset_t2);

            // Win32 HWND 캐시 (Thread 2에서 SetWindowPos 직접 호출용)
            #[cfg(target_os = "windows")]
            let cached_hwnd: isize = window_clone
                .hwnd()
                .map(|h| h.0 as isize)
                .unwrap_or(0);
            std::thread::spawn(move || {
                let mut prev_scale: f64 = 1.0; // 이전 프레임의 모니터 scale (프레임 간 유지하여 center_x 진동 방지)
                let mut smooth_y: f64 = 0.0;   // Y 위치 보간용 (모니터 전환 시 높이 점프 방지)
                let mut smooth_y_init = false;
                // 윈도우 물리 크기 캐시 (DPI 변경 시에만 갱신, 매 프레임 IPC 호출 제거)
                let outer = window_clone.outer_size();
                let mut cached_win_h: i32 = outer.as_ref()
                    .map(|s| s.height as i32)
                    .unwrap_or((LOGICAL_WIN_H * prev_scale) as i32);
                let mut cached_win_w: i32 = outer.as_ref()
                    .map(|s| s.width as i32)
                    .unwrap_or((LOGICAL_WIN_W * prev_scale) as i32);
                let mut cached_scale_for_h: f64 = prev_scale;

                // 등반 이동 상태 머신 변수
                let mut move_phase: u8 = 0;       // 0=Bottom, 1=ClimbRight, 2=Top, 3=DescendLeft
                let mut prev_move_phase: u8 = 0;  // phase 변경 이벤트 발행용
                let mut current_y: f64 = 0.0;     // 등반/하강 시 Y 좌표 추적
                let mut climb_edge_x: f64 = 0.0;  // 등반/하강 시 X 고정값
                let mut climb_top_y: f64 = 0.0;   // 등반 구간 상단 Y
                let mut climb_bottom_y: f64 = 0.0; // 등반 구간 하단 Y
                let mut consecutive_climbs: u8 = 0; // 연속 등반 횟수 (3회 이상 방지)
                let mut rng = rand::thread_rng();
                let mut random_dir_left: bool = rng.gen_bool(0.5); // 랜덤 모드 방향
                // 초기값을 반전시켜 첫 프레임에 방향 이벤트가 즉시 발행되도록 함
                let mut prev_random_dir_left: bool = !random_dir_left;
                let mut zorder_tick: u32 = 0; // Z-order 재적용 주기 카운터 (312프레임≈5초마다)
                let mut dir_sync_tick: u32 = 0; // 방향 동기화 재발행 카운터 (초기 이벤트 유실 복구용)
                let mut prev_on_fs_t2: bool = false; // Thread 2 내 전체화면 상태 변경 감지
                let mut prev_target_x: i32 = i32::MIN; // SetWindowPos 위치 변경 감지용
                let mut prev_target_y: i32 = i32::MIN;
                let mut prev_cursor_on_pet: bool = false; // 커서-펫 충돌 상태 (클릭 투과 토글용)
                let mut sorted_indices: Vec<usize> = Vec::with_capacity(8); // 등반 모드용 정렬 인덱스 (재사용)
                loop {
                    // 중지 상태: 200ms 대기 (16ms busy-loop 방지 → CPU 점유 12배 감소)
                    if !is_running_t2.load(Ordering::Relaxed) {
                        std::thread::sleep(std::time::Duration::from_millis(200));
                        continue;
                    }

                    std::thread::sleep(std::time::Duration::from_millis(16)); // ~60 FPS

                    // Thread 1로부터 텔레포트 요청 수신 (모니터 핫플러그 시)
                    let teleport_x = teleport_reader.load(Ordering::Relaxed);
                    let mut needs_show = false; // 모니터 변경 후 윈도우 강제 표시 플래그
                    if teleport_x != i64::MIN {
                        current_x = teleport_x as f64;
                        teleport_reader.store(i64::MIN, Ordering::Relaxed);
                        smooth_y_init = false; // Y 보간 초기화 (모니터 간 work_bottom 차이 대응)
                        needs_show = true;
                        // 모니터 변경 시 등반 상태 리셋
                        move_phase = 0;
                    }

                    // 공유 메모리에서 읽기만 수행 (매우 빠름, lock 없음)
                    let current_cpu = {
                        let t = test_cpu_t2.load(Ordering::Relaxed);
                        if t >= 0 {
                            t as f32
                        } else {
                            f32::from_bits(cpu_usage_reader.load(Ordering::Relaxed))
                        }
                    };

                    // Calc movement
                    let now = std::time::Instant::now();
                    // 시스템 부하 시 큰 점프 방지: delta_time 상한 50ms (3프레임 분량)
                    let delta_time = now.duration_since(last_update).as_secs_f64().min(0.05);
                    last_update = now;

                    // CPU 사용률에 비례한 속도 (0%→1x, 50%→3x, 100%→5x)
                    // 최대 5x로 제한하여 고속 이동 시 끊김 방지
                    let mut speed_multiplier = 1.0 + (current_cpu as f64 / 25.0);

                    // IF hovered, stop movement completely
                    if thread_hover_state.load(Ordering::Relaxed) {
                        speed_multiplier = 0.0;
                    }

                    // 전체 모니터 목록 기준으로 이동 (전체화면 모니터는 topmost 해제로 뒤에 숨김)
                    if let Ok(monitors) = all_monitors_reader.read() {
                        if !monitors.is_empty() {
                            // --- 공통: 모니터 범위, DPI 캐시, Y 위치를 단일 루프로 계산 ---
                            let mut max_x = i32::MIN;
                            let mut min_x = i32::MAX;
                            let mut raw_target_y = 0i32;
                            let mut found_monitor = false;
                            let center_x = current_x + (LOGICAL_WIN_W / 2.0 * prev_scale);
                            let mut found_work_bottom = 0i32;
                            for m in monitors.iter() {
                                if m.x < min_x { min_x = m.x; }
                                if m.x + m.width > max_x { max_x = m.x + m.width; }
                                if center_x >= (m.x as f64) && center_x <= ((m.x + m.width) as f64) {
                                    prev_scale = m.scale_factor;
                                    found_monitor = true;
                                    found_work_bottom = m.work_bottom;
                                }
                            }
                            // 대상 모니터의 DPI로 윈도우 물리 크기 직접 계산
                            // (outer_size()는 이전 모니터 기준이라 DPI가 다르면 부정확)
                            if prev_scale != cached_scale_for_h {
                                cached_win_h = (LOGICAL_WIN_H * prev_scale) as i32;
                                cached_win_w = (LOGICAL_WIN_W * prev_scale) as i32;
                                cached_scale_for_h = prev_scale;
                            }
                            let actual_win_h = cached_win_h;
                            let actual_win_w = cached_win_w;
                            // Y 위치 계산 (모니터별 작업표시줄 반영)
                            // 높이 오프셋: 양수=위로, 음수=아래로 (DPI 스케일 적용)
                            let height_offset = pet_height_reader.load(Ordering::Relaxed);
                            let height_px = (height_offset as f64 * prev_scale).round() as i32;
                            if found_monitor {
                                raw_target_y = found_work_bottom - actual_win_h - height_px;
                            }
                            if min_x == i32::MAX { min_x = 0; max_x = 1920; }

                            // --- 공통: 이동량 계산 ---
                            let pet_factor = f32::from_bits(speed_factor_reader.load(Ordering::Relaxed)) as f64;
                            let movement = 35.0 * prev_scale * speed_multiplier * pet_factor * delta_time;

                            // --- 이동 모드별 분기 ---
                            // mode 0=기본(오른쪽), 1=등반(오른쪽), 2=기본(왼쪽), 3=등반(왼쪽), 4=랜덤, 5=기본(반복)
                            let mode = move_mode_reader.load(Ordering::Relaxed);
                            let is_random = mode == 4;
                            let is_bounce = mode == 5;
                            let is_climb = mode == 1 || mode == 3 || is_random;
                            let is_left = if is_random || is_bounce { random_dir_left } else { mode >= 2 };
                            let target_x: i32;
                            let target_y: i32;

                            if is_climb {
                                // ========================================
                                // 등반 이동 모드 (4-phase 상태 머신)
                                // ========================================
                                // 모니터를 X좌표로 정렬 (인덱스 재사용으로 매 프레임 할당 방지)
                                sorted_indices.clear();
                                sorted_indices.extend(0..monitors.len());
                                sorted_indices.sort_by_key(|&i| monitors[i].x);

                                // 펫 스프라이트 실제 폭 (프론트엔드 CSS 픽셀 → 물리 픽셀 변환)
                                let pet_w = pet_visual_w_reader.load(Ordering::Relaxed) as f64 * prev_scale;
                                // 윈도우 내 펫 좌우 여백 (펫은 윈도우 중앙에 배치)
                                let margin = (actual_win_w as f64 - pet_w) / 2.0;

                                match move_phase {
                                    0 => {
                                        // Bottom: 하단 이동 (오른쪽 모드→우측, 왼쪽 모드→좌측)
                                        if is_left { current_x -= movement; } else { current_x += movement; }

                                        // 현재 모니터 경계 도달 감지
                                        let center = current_x + (LOGICAL_WIN_W / 2.0 * prev_scale);
                                        let cur_idx = sorted_indices.iter().position(|&si| { let m = &monitors[si];
                                            center >= m.x as f64 && center <= (m.x + m.width) as f64
                                        });

                                        if let Some(idx) = cur_idx {
                                            let mon = &monitors[sorted_indices[idx]];
                                            prev_scale = mon.scale_factor;
                                            raw_target_y = mon.work_bottom - actual_win_h - height_px;

                                            // 이동 방향에 따른 경계 도달 판정
                                            let at_edge = if is_left {
                                                // 왼쪽 모드: 펫 좌측 끝이 모니터 좌측에 도달
                                                let pet_left = current_x + margin;
                                                pet_left <= mon.x as f64
                                            } else {
                                                // 오른쪽 모드: 펫 우측 끝이 모니터 우측에 도달
                                                let pet_right = current_x + margin + pet_w;
                                                pet_right >= (mon.x + mon.width) as f64
                                            };

                                            if at_edge {
                                                // 인접 모니터 존재 여부 (이동 방향 기준)
                                                let has_neighbor = if is_left { idx > 0 } else { idx + 1 < sorted_indices.len() };
                                                let should_climb = if !has_neighbor {
                                                    true // 끝 모니터 → 무조건 등반
                                                } else if !is_random && consecutive_climbs >= 2 {
                                                    false // 연속 2회 등반 → 강제 건너가기 (랜덤 모드 제외)
                                                } else {
                                                    rng.gen_bool(0.5)
                                                };

                                                if should_climb {
                                                    // 이동 방향 쪽 벽에서 등반 시작
                                                    move_phase = 1;
                                                    climb_top_y = mon.y as f64 - (actual_win_h as f64 - pet_w) / 2.0 + height_px as f64;
                                                    climb_bottom_y = mon.work_bottom as f64 - (actual_win_h as f64 + pet_w) / 2.0 - height_px as f64;
                                                    if is_left {
                                                        // 왼쪽 벽 등반
                                                        climb_edge_x = mon.x as f64 - (actual_win_w - actual_win_h) as f64 / 2.0;
                                                    } else {
                                                        // 오른쪽 벽 등반
                                                        climb_edge_x = (mon.x + mon.width) as f64 - (actual_win_w + actual_win_h) as f64 / 2.0;
                                                    }
                                                    // Phase 1 벽면 오프셋 (등반 벽: is_left 방향)
                                                    let wo = if is_left { height_px as f64 } else { -(height_px as f64) };
                                                    current_x = climb_edge_x + wo;
                                                    current_y = climb_bottom_y;
                                                    consecutive_climbs += 1;
                                                } else {
                                                    // 인접 모니터 하단으로 건너가기
                                                    let neighbor = if is_left { &monitors[sorted_indices[idx - 1]] } else { &monitors[sorted_indices[idx + 1]] };
                                                    if is_left {
                                                        // 왼쪽: 이전 모니터 우측 끝에서 시작
                                                        current_x = (neighbor.x + neighbor.width) as f64 - margin - pet_w;
                                                    } else {
                                                        // 오른쪽: 다음 모니터 좌측 끝에서 시작
                                                        current_x = neighbor.x as f64 - margin;
                                                    }
                                                    prev_scale = neighbor.scale_factor;
                                                    raw_target_y = neighbor.work_bottom - actual_win_h - height_px;
                                                    consecutive_climbs = 0;
                                                }
                                            }
                                        }

                                        // 범위 이탈 보정 (핫플러그 대응)
                                        if is_left {
                                            // 왼쪽 이동: 좌측 끝 벗어나면 우측에서 재등장
                                            if current_x < (min_x as f64 - actual_win_w as f64) || current_x > (max_x as f64 + actual_win_w as f64) {
                                                current_x = max_x as f64;
                                                smooth_y_init = false;
                                                needs_show = true;
                                            }
                                        } else {
                                            // 오른쪽 이동: 우측 끝 벗어나면 좌측에서 재등장
                                            if current_x > max_x as f64 || current_x < (min_x as f64 - 200.0) {
                                                current_x = min_x as f64 - 100.0;
                                                smooth_y_init = false;
                                                needs_show = true;
                                            }
                                        }

                                        // Fallback Y
                                        if !found_monitor {
                                            if let Some(&si) = sorted_indices.first() { let pm = &monitors[si];
                                                raw_target_y = pm.work_bottom - actual_win_h - height_px;
                                            }
                                        }

                                        // Y 보간 (하단 이동 중에만)
                                        if !smooth_y_init {
                                            smooth_y = raw_target_y as f64;
                                            smooth_y_init = true;
                                        } else {
                                            let diff = raw_target_y as f64 - smooth_y;
                                            if diff.abs() < 1.0 { smooth_y = raw_target_y as f64; }
                                            else { smooth_y += diff * 0.15; }
                                        }

                                        target_x = current_x as i32;
                                        target_y = smooth_y as i32;
                                    }
                                    1 => {
                                        // Climb(↑): 벽 테두리를 타고 위로 등반
                                        current_y -= movement;
                                        // 벽면 수직 방향 높이 오프셋 (양수=벽에서 멀어짐)
                                        let wall_offset = if is_left { height_px as f64 } else { -(height_px as f64) };
                                        current_x = climb_edge_x + wall_offset;

                                        if current_y <= climb_top_y {
                                            current_y = climb_top_y;

                                            // 랜덤 모드: 상단 도달 시 인접 모니터로 건너가기 가능
                                            let mut crossed_at_top = false;
                                            if is_random {
                                                let center = current_x + (LOGICAL_WIN_W / 2.0 * prev_scale);
                                                let cur_idx = sorted_indices.iter().position(|&si| { let m = &monitors[si];
                                                    center >= m.x as f64 && center <= (m.x + m.width) as f64
                                                });
                                                if let Some(idx) = cur_idx {
                                                    // 등반한 벽 방향의 인접 모니터 확인
                                                    let has_neighbor = if is_left { idx > 0 } else { idx + 1 < sorted_indices.len() };
                                                    if has_neighbor && rng.gen_bool(0.5) {
                                                        // 인접 모니터로 건너가기
                                                        let neighbor = if is_left { &monitors[sorted_indices[idx - 1]] } else { &monitors[sorted_indices[idx + 1]] };
                                                        prev_scale = neighbor.scale_factor;
                                                        // 상단 이동 (Phase 2) — 진입 경계에서 출발, 방향 전환하여 먼 벽까지 이동
                                                        // (먼 벽에 도달하면 Phase 2 경계 판정에서 자연스럽게 하강 전환)
                                                        // is_left=true → 좌측 이웃 진입 → 우측(진입 경계)에서 출발 → 좌측으로 이동
                                                        // is_left=false → 우측 이웃 진입 → 좌측(진입 경계)에서 출발 → 우측으로 이동
                                                        {
                                                            random_dir_left = !is_left;
                                                            if is_left {
                                                                current_x = (neighbor.x + neighbor.width) as f64 - margin - pet_w;
                                                            } else {
                                                                current_x = neighbor.x as f64 - margin;
                                                            }
                                                            current_y = neighbor.y as f64 + height_px as f64;
                                                            move_phase = 2;
                                                        }
                                                        crossed_at_top = true;
                                                    }
                                                }
                                            }
                                            if !crossed_at_top {
                                                // 등반 벽 가장자리에서 Phase 2 시작 (펫 위치 보정)
                                                if is_left {
                                                    // 좌측 벽 등반 → 좌측 끝에서 우측으로 이동
                                                    current_x = climb_edge_x - margin;
                                                } else {
                                                    // 우측 벽 등반 → 우측 끝에서 좌측으로 이동
                                                    current_x = climb_edge_x + actual_win_w as f64 - margin - pet_w;
                                                }
                                                move_phase = 2; // → Top (현재 모니터에서 계속)
                                            }
                                        }

                                        target_x = current_x as i32;
                                        target_y = current_y as i32;
                                    }
                                    2 => {
                                        // Top: 상단 이동 (오른쪽 모드→좌측, 왼쪽 모드→우측)
                                        if is_left { current_x += movement; } else { current_x -= movement; }

                                        // 현재 모니터 찾기 (상단 Y 결정용)
                                        let center = current_x + (LOGICAL_WIN_W / 2.0 * prev_scale);
                                        let cur_idx = sorted_indices.iter().position(|&si| { let m = &monitors[si];
                                            center >= m.x as f64 && center <= (m.x + m.width) as f64
                                        });

                                        if let Some(idx) = cur_idx {
                                            let mon = &monitors[sorted_indices[idx]];
                                            prev_scale = mon.scale_factor;
                                            // 상단 표면 수직 방향 높이 오프셋 (양수=상단에서 멀어짐=아래)
                                            current_y = mon.y as f64 + height_px as f64;

                                            // 이동 방향에 따른 경계 도달 판정
                                            let at_edge = if is_left {
                                                // 왼쪽 모드(Phase 2에서는 우측 이동): 펫 우측 끝이 모니터 우측에 도달
                                                let pet_right = current_x + margin + pet_w;
                                                pet_right >= (mon.x + mon.width) as f64
                                            } else {
                                                // 오른쪽 모드(Phase 2에서는 좌측 이동): 펫 좌측 끝이 모니터 좌측에 도달
                                                let pet_left = current_x + margin;
                                                pet_left <= mon.x as f64
                                            };

                                            if at_edge {
                                                // 인접 모니터 존재 여부 (Phase 2 이동 방향 기준)
                                                let has_neighbor = if is_left { idx + 1 < sorted_indices.len() } else { idx > 0 };
                                                // 랜덤 모드: 인접 모니터 있어도 50% 확률로 하강 선택
                                                let should_cross = has_neighbor && (!is_random || rng.gen_bool(0.5));
                                                if should_cross {
                                                    // 인접 모니터 상단으로 이동
                                                    let neighbor = if is_left { &monitors[sorted_indices[idx + 1]] } else { &monitors[sorted_indices[idx - 1]] };
                                                    if is_left {
                                                        current_x = neighbor.x as f64 - margin;
                                                    } else {
                                                        current_x = (neighbor.x + neighbor.width) as f64 - margin - pet_w;
                                                    }
                                                    current_y = neighbor.y as f64 + height_px as f64;
                                                    prev_scale = neighbor.scale_factor;
                                                } else {
                                                    // 끝 모니터 또는 랜덤 하강 → 하강 시작 (등반 반대쪽 벽)
                                                    move_phase = 3;
                                                    climb_top_y = mon.y as f64 - (actual_win_h as f64 - pet_w) / 2.0 + height_px as f64;
                                                    climb_bottom_y = mon.work_bottom as f64 - (actual_win_h as f64 + pet_w) / 2.0 - height_px as f64;
                                                    if is_left {
                                                        // 왼쪽 모드: 오른쪽 벽에서 하강
                                                        climb_edge_x = (mon.x + mon.width) as f64 - (actual_win_w + actual_win_h) as f64 / 2.0;
                                                    } else {
                                                        // 오른쪽 모드: 왼쪽 벽에서 하강
                                                        climb_edge_x = mon.x as f64 - (actual_win_w - actual_win_h) as f64 / 2.0;
                                                    }
                                                    // Phase 3 벽면 오프셋 (하강 벽: 등반 반대쪽)
                                                    let wo = if is_left { -(height_px as f64) } else { height_px as f64 };
                                                    current_x = climb_edge_x + wo;
                                                    current_y = climb_top_y;
                                                }
                                            }
                                        } else {
                                            // 모니터 못 찾음 → fallback 하강
                                            let fallback_mon = if is_left { sorted_indices.last() } else { sorted_indices.first() }.map(|&si| &monitors[si]);
                                            if let Some(fb) = fallback_mon {
                                                move_phase = 3;
                                                climb_top_y = fb.y as f64 - (actual_win_h as f64 - pet_w) / 2.0 + height_px as f64;
                                                climb_bottom_y = fb.work_bottom as f64 - (actual_win_h as f64 + pet_w) / 2.0 - height_px as f64;
                                                if is_left {
                                                    climb_edge_x = (fb.x + fb.width) as f64 - (actual_win_w + actual_win_h) as f64 / 2.0;
                                                } else {
                                                    climb_edge_x = fb.x as f64 - (actual_win_w - actual_win_h) as f64 / 2.0;
                                                }
                                                // Phase 3 벽면 오프셋
                                                let wo = if is_left { -(height_px as f64) } else { height_px as f64 };
                                                current_x = climb_edge_x + wo;
                                                current_y = climb_top_y;
                                                prev_scale = fb.scale_factor;
                                            }
                                        }

                                        target_x = current_x as i32;
                                        target_y = current_y as i32;
                                    }
                                    3 => {
                                        // Descend(↓): 벽 테두리를 타고 아래로 하강
                                        current_y += movement;
                                        // 벽면 수직 방향 높이 오프셋 (하강 벽은 등반 반대쪽)
                                        let wall_offset = if is_left { -(height_px as f64) } else { height_px as f64 };
                                        current_x = climb_edge_x + wall_offset;

                                        if current_y >= climb_bottom_y {
                                            current_y = climb_bottom_y;

                                            if is_random {
                                                // 랜덤 모드: 하강 완료 시 인접 모니터 건너가기 가능
                                                let center = current_x + (LOGICAL_WIN_W / 2.0 * prev_scale);
                                                let cur_idx = sorted_indices.iter().position(|&si| { let m = &monitors[si];
                                                    center >= m.x as f64 && center <= (m.x + m.width) as f64
                                                });
                                                let mut crossed_at_bottom = false;
                                                if let Some(idx) = cur_idx {
                                                    // 하강한 벽 방향의 인접 모니터 확인 (하강 벽은 등반 반대쪽)
                                                    let has_neighbor = if is_left { idx + 1 < sorted_indices.len() } else { idx > 0 };
                                                    if has_neighbor && rng.gen_bool(0.5) {
                                                        // 인접 모니터로 건너가기
                                                        let neighbor = if is_left { &monitors[sorted_indices[idx + 1]] } else { &monitors[sorted_indices[idx - 1]] };
                                                        prev_scale = neighbor.scale_factor;
                                                        // 하단 이동 (Phase 0) — 진입 경계에서 출발, 방향 전환하여 먼 벽까지 이동
                                                        // (먼 벽에 도달하면 Phase 0 경계 판정에서 자연스럽게 등반 전환)
                                                        // is_left=true → 우측 이웃 진입 → 좌측(진입 경계)에서 출발 → 우측으로 이동
                                                        // is_left=false → 좌측 이웃 진입 → 우측(진입 경계)에서 출발 → 좌측으로 이동
                                                        {
                                                            random_dir_left = !is_left;
                                                            if is_left {
                                                                current_x = neighbor.x as f64 - margin;
                                                            } else {
                                                                current_x = (neighbor.x + neighbor.width) as f64 - margin - pet_w;
                                                            }
                                                            smooth_y = (neighbor.work_bottom - actual_win_h - height_px) as f64;
                                                            smooth_y_init = true;
                                                            move_phase = 0;
                                                        }
                                                        crossed_at_bottom = true;
                                                    }
                                                }
                                                if !crossed_at_bottom {
                                                    // 하강 벽 가장자리에서 Phase 0 시작 (펫 위치 보정)
                                                    if is_left {
                                                        // 우측 벽 하강 → 우측 끝에서 좌측으로 이동
                                                        current_x = climb_edge_x + actual_win_w as f64 - margin - pet_w;
                                                    } else {
                                                        // 좌측 벽 하강 → 좌측 끝에서 우측으로 이동
                                                        current_x = climb_edge_x - margin;
                                                    }
                                                    move_phase = 0;
                                                    smooth_y = climb_bottom_y - (actual_win_h as f64 - pet_w) / 2.0;
                                                    smooth_y_init = true;
                                                    // 방향 유지 — 하강 벽 반대쪽(모니터 내부)으로 이동
                                                    random_dir_left = is_left;
                                                }
                                            } else {
                                                // Phase 0 복귀 — 하강 벽 가장자리에서 시작 (펫 위치 보정)
                                                if is_left {
                                                    current_x = climb_edge_x + actual_win_w as f64 - margin - pet_w;
                                                } else {
                                                    current_x = climb_edge_x - margin;
                                                }
                                                move_phase = 0;
                                                smooth_y = climb_bottom_y - (actual_win_h as f64 - pet_w) / 2.0;
                                                smooth_y_init = true;
                                            }
                                        }

                                        target_x = current_x as i32;
                                        target_y = current_y as i32;
                                    }
                                    _ => {
                                        move_phase = 0;
                                        target_x = current_x as i32;
                                        target_y = smooth_y as i32;
                                    }
                                }

                                // phase 변경 이벤트 발행 (변경 시에만, 매 프레임 아님)
                                if move_phase != prev_move_phase {
                                    // phase 변경 시 방향도 함께 재발행하여 동기화 보장
                                    if is_random || is_bounce {
                                        let _ = window_clone.emit("move-direction", random_dir_left);
                                        prev_random_dir_left = random_dir_left;
                                    }
                                    let _ = window_clone.emit("move-phase", move_phase);
                                    prev_move_phase = move_phase;
                                    // 전환 프레임: 프론트엔드 CSS 회전 적용 전까지
                                    // SetWindowPos 호출을 건너뛰어 시각적 깜빡임 방지
                                    continue;
                                }
                            } else {
                                // ========================================
                                // 기본 이동 모드 (방향에 따라 좌/우 이동)
                                // ========================================
                                if is_left { current_x -= movement; } else { current_x += movement; }

                                // 범위 이탈 보정
                                if is_bounce {
                                    // 반복 모드: 끝 도달 시 방향 전환
                                    if is_left && current_x < (min_x as f64 - actual_win_w as f64) {
                                        current_x = min_x as f64 - actual_win_w as f64;
                                        random_dir_left = false;
                                    } else if !is_left && current_x > (max_x as f64 - actual_win_w as f64 / 2.0) {
                                        current_x = max_x as f64 - actual_win_w as f64 / 2.0;
                                        random_dir_left = true;
                                    }
                                } else if is_left {
                                    // 왼쪽 이동: 좌측 끝 벗어나면 우측에서 재등장
                                    if current_x < (min_x as f64 - actual_win_w as f64) || current_x > (max_x as f64 + actual_win_w as f64) {
                                        current_x = max_x as f64;
                                        smooth_y_init = false;
                                        needs_show = true;
                                    }
                                } else {
                                    // 오른쪽 이동: 우측 끝 벗어나면 좌측에서 재등장
                                    if current_x > max_x as f64 || current_x < (min_x as f64 - 200.0) {
                                        current_x = min_x as f64 - 100.0;
                                        smooth_y_init = false;
                                        needs_show = true;
                                    }
                                }

                                // Fallback Y
                                if !found_monitor {
                                    if let Some(pm) = monitors.first() {
                                        raw_target_y = pm.work_bottom - actual_win_h - height_px;
                                    }
                                }

                                // Y 위치 보간
                                if !smooth_y_init {
                                    smooth_y = raw_target_y as f64;
                                    smooth_y_init = true;
                                } else {
                                    let diff = raw_target_y as f64 - smooth_y;
                                    if diff.abs() < 1.0 { smooth_y = raw_target_y as f64; }
                                    else { smooth_y += diff * 0.15; }
                                }

                                target_x = current_x as i32;
                                target_y = smooth_y as i32;

                                // 등반→기본 전환 시 phase 리셋 이벤트
                                if prev_move_phase != 0 {
                                    move_phase = 0;
                                    let _ = window_clone.emit("move-phase", 0u8);
                                    prev_move_phase = 0;
                                }
                            }

                            // 동적 방향 모드(랜덤/반복): 방향 변경 이벤트 발행
                            // 변경 시 즉시 + 초기 3초간 주기적 재발행 (프론트엔드 리스너 미등록 시 이벤트 유실 복구)
                            if is_random || is_bounce {
                                let dir_changed = random_dir_left != prev_random_dir_left;
                                let need_sync = dir_sync_tick < 180 && dir_sync_tick % 30 == 0; // 첫 3초간 0.5초마다
                                if dir_changed || need_sync {
                                    let _ = window_clone.emit("move-direction", random_dir_left);
                                    prev_random_dir_left = random_dir_left;
                                }
                                dir_sync_tick += 1;
                            } else {
                                // 비동적 방향 모드에서는 prev를 반전시켜서 동적 모드로 전환 시 즉시 발행되도록 준비
                                prev_random_dir_left = !random_dir_left;
                                dir_sync_tick = 0; // 동적 모드 재진입 시 동기화 재시작
                            }

                            // Thread 1의 전체화면 감지용 X좌표 공유
                            pet_x_writer.store(current_x as i64, Ordering::Relaxed);

                            // Win32 SetWindowPos로 위치 이동과 TOPMOST를 동시에 적용
                            // → 모니터 간 이동 시 다른 창 뒤로 숨는 문제 방지
                            // Thread 1에서 모니터 변경 감지 시 렌더링 갱신 요청 수신
                            let do_redraw = redraw_reader.compare_exchange(
                                true, false, Ordering::Relaxed, Ordering::Relaxed
                            ).is_ok();

                            #[cfg(target_os = "windows")]
                            {
                                #[link(name = "user32")]
                                extern "system" {
                                    fn SetWindowPos(
                                        hwnd: isize,
                                        hWndInsertAfter: isize,
                                        x: i32,
                                        y: i32,
                                        cx: i32,
                                        cy: i32,
                                        uFlags: u32,
                                    ) -> i32;
                                    fn ShowWindow(hwnd: isize, nCmdShow: i32) -> i32;
                                    fn GetCursorPos(lpPoint: *mut [i32; 2]) -> i32;
                                    fn RedrawWindow(
                                        hwnd: isize,
                                        lprcUpdate: *const u8,
                                        hrgnUpdate: isize,
                                        flags: u32,
                                    ) -> i32;
                                }
                                const HWND_TOPMOST: isize = -1;
                                const HWND_NOTOPMOST: isize = -2;
                                const SWP_NOSIZE: u32 = 0x0001;
                                const SWP_NOZORDER: u32 = 0x0004;
                                const SWP_NOACTIVATE: u32 = 0x0010;
                                const SWP_FRAMECHANGED: u32 = 0x0020;
                                const SWP_NOSENDCHANGING: u32 = 0x0400; // WM_WINDOWPOSCHANGING 메시지 생략
                                const SW_SHOWNA: i32 = 8; // 활성화 없이 표시
                                // RedrawWindow 플래그
                                const RDW_INVALIDATE: u32 = 0x0001;
                                const RDW_ERASE: u32 = 0x0004;
                                const RDW_ALLCHILDREN: u32 = 0x0080;
                                const RDW_UPDATENOW: u32 = 0x0100;
                                const RDW_FRAME: u32 = 0x0400;

                                let on_fs = on_fs_reader.load(Ordering::Relaxed);
                                let insert_after = if on_fs {
                                    HWND_NOTOPMOST
                                } else {
                                    HWND_TOPMOST
                                };

                                // Z-order 재적용 판단: 전체화면 전환 시 즉시, 그 외 ~500ms(30프레임)마다
                                let fs_changed = on_fs != prev_on_fs_t2;
                                prev_on_fs_t2 = on_fs;
                                zorder_tick += 1;
                                let apply_zorder = fs_changed || do_redraw || needs_show || zorder_tick >= 312;
                                if apply_zorder {
                                    zorder_tick = 0;
                                }

                                // 위치 변경 감지: 좌표가 이전과 같으면 SetWindowPos 생략 (DWM 부하 제거)
                                // 단, Z-order 재적용/redraw/show가 필요한 프레임은 항상 호출
                                let pos_changed = target_x != prev_target_x || target_y != prev_target_y;

                                if cached_hwnd != 0 {
                                    unsafe {
                                        if do_redraw {
                                            // 모니터 변경: 크기 포함 SetWindowPos + SWP_FRAMECHANGED로
                                            // 프레임 속성 재적용 (투명도/DPI 갱신 트리거)
                                            SetWindowPos(
                                                cached_hwnd,
                                                insert_after,
                                                target_x,
                                                target_y,
                                                actual_win_h,
                                                0,
                                                SWP_NOSIZE | SWP_NOACTIVATE | SWP_FRAMECHANGED,
                                            );
                                            // 렌더링 표면 전체 강제 갱신 (WebView + 자식 윈도우 포함)
                                            RedrawWindow(
                                                cached_hwnd,
                                                std::ptr::null(),
                                                0,
                                                RDW_INVALIDATE | RDW_ERASE | RDW_ALLCHILDREN | RDW_UPDATENOW | RDW_FRAME,
                                            );
                                            // 높이 캐시 무효화 (DPI 변경 대응)
                                            cached_scale_for_h = -1.0;
                                        } else if apply_zorder {
                                            // Z-order 재적용 프레임: TOPMOST/NOTOPMOST + 위치 이동
                                            SetWindowPos(
                                                cached_hwnd,
                                                insert_after,
                                                target_x,
                                                target_y,
                                                0,
                                                0,
                                                SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOSENDCHANGING,
                                            );
                                        } else if pos_changed {
                                            // 일반 프레임: 위치만 이동, Z-order·WM_WINDOWPOSCHANGING 생략
                                            SetWindowPos(
                                                cached_hwnd,
                                                insert_after,
                                                target_x,
                                                target_y,
                                                0,
                                                0,
                                                SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOZORDER | SWP_NOSENDCHANGING,
                                            );
                                        }
                                        // 위치 캐시 갱신
                                        prev_target_x = target_x;
                                        prev_target_y = target_y;
                                        // 모니터 제거 시 OS가 윈도우를 숨길 수 있음
                                        // → 유효 좌표 이동 직후 동기적으로 강제 표시
                                        if needs_show || do_redraw {
                                            ShowWindow(cached_hwnd, SW_SHOWNA);
                                        }
                                    }
                                }

                                // 커서가 캐릭터 영역 위에 있는지 판단하여 클릭 투과 토글
                                // 캐릭터 외 빈 영역은 클릭이 뒤쪽 윈도우/바탕화면으로 통과
                                // 마우스 사용 비활성 시 항상 클릭 투과
                                unsafe {
                                    let mut pt: [i32; 2] = [0, 0];
                                    if GetCursorPos(&mut pt) != 0 {
                                        let cursor_on_pet = if !mouse_enabled_t2.load(Ordering::Relaxed) {
                                            // 마우스 사용 꺼짐 → 항상 클릭 투과
                                            false
                                        } else if move_phase != 0 {
                                            // Phase 1/2/3: CSS rotate로 캐릭터 위치 변동 → 윈도우 전체를 히트 영역
                                            pt[0] >= target_x && pt[0] <= target_x + cached_win_w
                                                && pt[1] >= target_y && pt[1] <= target_y + cached_win_h
                                        } else {
                                            // Phase 0: 캐릭터는 윈도우 하단 중앙에 배치
                                            let pvw = pet_visual_w_reader.load(Ordering::Relaxed);
                                            let pet_phys_w = if pvw > 0 { (pvw as f64 * prev_scale) as i32 } else { cached_win_w };
                                            let margin_x = (cached_win_w - pet_phys_w).max(0) / 2;
                                            let pet_left = target_x + margin_x;
                                            let pet_right = pet_left + pet_phys_w;
                                            let pet_phys_h = pet_phys_w;
                                            let pet_bottom = target_y + cached_win_h;
                                            let pet_top = (pet_bottom - pet_phys_h).max(target_y);
                                            pt[0] >= pet_left && pt[0] <= pet_right
                                                && pt[1] >= pet_top && pt[1] <= pet_bottom
                                        };
                                        if cursor_on_pet != prev_cursor_on_pet {
                                            let _ = window_clone.set_ignore_cursor_events(!cursor_on_pet);
                                            prev_cursor_on_pet = cursor_on_pet;
                                        }
                                    }
                                }
                            }
                            #[cfg(not(target_os = "windows"))]
                            {
                                let _ = window_clone.set_position(tauri::Position::Physical(
                                    tauri::PhysicalPosition::new(target_x, target_y),
                                ));
                            }
                        }
                    }
                }
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            show_main_window,
            set_hover,
            set_test_cpu,
            update_pet_type,
            update_pet_scale,
            update_pet_speed,
            update_pet_height,
            update_pet_color,
            update_monitor_config,
            set_polling_interval,
            update_mouse_enabled,
            update_bubble_enabled,
            update_bubble_side,
            update_bubble_top,
            update_bubble_height,
            update_alarm_list,
            update_display_config,
            update_move_mode,
            update_pet_visual_w,
            update_messages,
            update_msg_rotate,
            update_app_settings,
            get_auto_start,
            set_auto_start,
            update_timer_state,
            update_timer_font_size
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
