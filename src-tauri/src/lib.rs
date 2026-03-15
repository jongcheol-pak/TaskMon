use std::sync::atomic::{AtomicBool, AtomicI32, AtomicI64, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, RwLock};
use tauri::{AppHandle, Emitter, Manager, State};

/// 한 번의 EnumWindows 순회로 여러 모니터의 전체화면 여부를 동시에 체크
/// monitors 슬라이스와 같은 길이의 bool 배열 반환 (인덱스 대응)
/// N번 호출 → 1번으로 줄여 API 오버헤드 감소
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
        // - WS_EX_LAYERED: OBS 오버레이, Steam 오버레이, 자체 Tauri 창 등 투명 창
        // - WS_EX_TOOLWINDOW: ALT+TAB에 표시되지 않는 플로팅 도구 창
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
        if all_found {
            0
        } else {
            1
        }
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
const LOGICAL_WIN_H: f64 = 150.0;

#[derive(Clone, Default)]
struct MonitorInfo {
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    scale_factor: f64,
    work_bottom: i32, // 작업영역 하단 Y 절대좌표 (물리 픽셀) = 작업표시줄 상단
}

/// 모니터별 작업영역(work area)을 Win32 API로 조회하여 작업표시줄 높이를 정확히 산출
#[cfg(target_os = "windows")]
fn get_work_area_for_monitor(monitor_x: i32, monitor_y: i32) -> Option<[i32; 4]> {
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
        Some(mi.rc_work) // [left, top, right, bottom]
    }
}

#[cfg(not(target_os = "windows"))]
fn get_work_area_for_monitor(_monitor_x: i32, _monitor_y: i32) -> Option<[i32; 4]> {
    None
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
fn update_mouse_enabled(app: AppHandle, enabled: bool) {
    let _ = app.emit("mouse-enabled-update", enabled);
}

#[tauri::command]
fn update_bubble_enabled(app: AppHandle, enabled: bool) {
    let _ = app.emit("bubble-enabled-update", enabled);
}

#[tauri::command]
fn update_bubble_height(app: AppHandle, height: u32) {
    let _ = app.emit("bubble-height-update", height);
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

    tauri::Builder::default()
        .manage(AppState {
            is_hovered: app_state_hover,
            test_cpu: Arc::clone(&test_cpu),
            polling_interval_ms: Arc::clone(&polling_interval_ms),
            pet_speed_factor,
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
            let work_bottom = get_work_area_for_monitor(mon_x, mon_y)
                .map(|rc| rc[3])
                .unwrap_or(mon_y + mon_h);
            // set_size 후 실제 물리 높이 조회 (150 * scale 추정값 대신 OS가 보고하는 정확한 값)
            let actual_init_h = window
                .outer_size()
                .map(|s| s.height as i32)
                .unwrap_or((150.0 * scale_factor) as i32);
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
            if let Ok(monitors) = window_clone.available_monitors() {
                let info_list: Vec<MonitorInfo> = monitors
                    .iter()
                    .map(|m| {
                        let x = m.position().x;
                        let y = m.position().y;
                        let height = m.size().height as i32;
                        let work_bottom = get_work_area_for_monitor(x, y)
                            .map(|rc| rc[3]) // 작업영역 하단 절대 Y좌표
                            .unwrap_or(y + height);
                        MonitorInfo {
                            x,
                            y,
                            width: m.size().width as i32,
                            height,
                            scale_factor: m.scale_factor(),
                            work_bottom,
                        }
                    })
                    .collect();
                // shared_monitors 초기화
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

                // 네트워크 모니터링 초기화
                let mut networks = sysinfo::Networks::new_with_refreshed_list();
                let mut network_refresh_tick: u32 = 0;

                // 배터리 모니터링 초기화
                let battery_manager = starship_battery::Manager::new().ok();
                let mut battery_tick: u32 = 179; // 첫 폴링을 2초 후로 지연 (React 리스너 등록 대기), 이후 3분마다
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

                    // 2. 모니터 정보 갱신 (1초마다 OS에 질의, Arc swap으로 clone 비용 제거)
                    if let Ok(m_list) = window_clone_evt.available_monitors() {
                        let mut new_monitors = Vec::new();
                        for m in m_list {
                            let mx = m.position().x;
                            let my = m.position().y;
                            let mh = m.size().height as i32;
                            let work_bottom = get_work_area_for_monitor(mx, my)
                                .map(|rc| rc[3])
                                .unwrap_or(my + mh);
                            new_monitors.push(MonitorInfo {
                                x: mx,
                                y: my,
                                width: m.size().width as i32,
                                height: mh,
                                scale_factor: m.scale_factor(),
                                work_bottom,
                            });
                        }
                        if let Ok(mut cache) = monitors_clone.write() {
                            *cache = Arc::new(new_monitors);
                        }
                    }

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

                    let pet_x = pet_x_reader.load(Ordering::Relaxed);
                    // 현재 위치의 모니터 scale을 찾아 물리 너비 절반을 계산
                    let half_w = monitors_snapshot.iter()
                        .find(|m| pet_x >= m.x as i64 && pet_x < (m.x + m.width) as i64)
                        .map(|m| (LOGICAL_WIN_W / 2.0 * m.scale_factor) as i64)
                        .unwrap_or((LOGICAL_WIN_W / 2.0) as i64);
                    let center_x = pet_x + half_w;

                    let fs_flags = check_fullscreen_all(&monitors_snapshot);

                    let mut pet_on_fullscreen = false;
                    for (m, &fs) in monitors_snapshot.iter().zip(fs_flags.iter()) {
                        if center_x >= m.x as i64 && center_x < (m.x + m.width) as i64 && fs {
                            pet_on_fullscreen = true;
                        }
                    }

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

                    // 배터리 잔량: 3분마다 폴링 (starship_battery WMI 호출 비용 절감)
                    if battery_tick == 0 {
                        if let Some(ref manager) = battery_manager {
                            if let Ok(mut batteries) = manager.batteries() {
                                if let Some(Ok(bat)) = batteries.next() {
                                    cached_battery_percent = (bat.state_of_charge().get::<starship_battery::units::ratio::percent>()) as i32;
                                }
                            }
                        }
                    }
                    battery_tick = (battery_tick + 1) % 180;

                    // 충전 상태: 매 폴링마다 확인 (GetSystemPowerStatus는 매우 가벼움)
                    // 변경 시 또는 배터리 잔량 갱신 시에만 이벤트 발송
                    if cached_battery_percent >= 0 {
                        let charging = is_ac_connected();
                        if charging != prev_charging || battery_tick == 1 {
                            prev_charging = charging;
                            let _ = window_clone_evt.emit("battery-usage", serde_json::json!({
                                "percent": cached_battery_percent,
                                "charging": charging
                            }));
                        }
                    }

                    let interval = polling_ms_t1.load(Ordering::Relaxed);
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
                // 윈도우 물리 높이 캐시 (DPI 변경 시에만 갱신, 매 프레임 IPC 호출 제거)
                let mut cached_win_h: i32 = window_clone
                    .outer_size()
                    .map(|s| s.height as i32)
                    .unwrap_or((LOGICAL_WIN_H * prev_scale) as i32);
                let mut cached_scale_for_h: f64 = prev_scale;
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
                            let mut max_x = i32::MIN;
                            let mut min_x = i32::MAX;
                            let mut raw_target_y = 0i32;
                            let mut found_monitor = false;

                            // 먼저 min/max 범위만 계산
                            for m in monitors.iter() {
                                if m.x < min_x {
                                    min_x = m.x;
                                }
                                if m.x + m.width > max_x {
                                    max_x = m.x + m.width;
                                }
                            }

                            // 이전 프레임의 scale로 정확한 윈도우 중심 계산 (경계 진동 방지)
                            let center_x = current_x + (LOGICAL_WIN_W / 2.0 * prev_scale);

                            // DPI 전환 시에만 윈도우 높이 재조회 (매 프레임 IPC 호출 제거)
                            if prev_scale != cached_scale_for_h {
                                cached_win_h = window_clone
                                    .outer_size()
                                    .map(|s| s.height as i32)
                                    .unwrap_or((LOGICAL_WIN_H * prev_scale) as i32);
                                cached_scale_for_h = prev_scale;
                            }
                            let actual_win_h = cached_win_h;

                            for m in monitors.iter() {
                                let px = m.x;
                                let pw = m.width;

                                if center_x >= (px as f64) && center_x <= ((px + pw) as f64) {
                                    prev_scale = m.scale_factor;
                                    raw_target_y = m.work_bottom - actual_win_h;
                                    found_monitor = true;
                                }
                            }

                            // Fallback bounds
                            if min_x == i32::MAX {
                                min_x = 0;
                                max_x = 1920;
                            }

                            // 이동 속도 계산 (현재 모니터 scale + 펫별 속도 배율 반영)
                            let pet_factor = f32::from_bits(speed_factor_reader.load(Ordering::Relaxed)) as f64;
                            let movement = 35.0 * prev_scale * speed_multiplier * pet_factor * delta_time;
                            current_x += movement;
                            // Thread 1의 전체화면 감지가 올바른 모니터를 알 수 있도록 공유
                            pet_x_writer.store(current_x as i64, Ordering::Relaxed);

                            // 🚨 [EDGE CASE] 모니터 핫플러깅 대응 (탈출 & 텔레포트)
                            // 1) 뼈다귀가 오른쪽 끝(전체 max_x)을 넘었을 때 (정상적인 랩어라운드 또는 우측 모니터 뽑힘)
                            // 2) 뼈다귀의 위치가 왼쪽 끝(전체 min_x)보다 작을 때 (좌측 모니터 뽑힘/미아 상태)
                            if current_x > max_x as f64 || current_x < (min_x as f64 - 200.0) {
                                current_x = min_x as f64 - 100.0;
                                smooth_y_init = false;
                                needs_show = true;
                            }

                            // 모니터 미발견 시 (베젤 통과 중 등) 첫 번째 모니터 기준으로 보정
                            if !found_monitor {
                                if let Some(pm) = monitors.first() {
                                    raw_target_y = pm.work_bottom - actual_win_h;
                                }
                            }

                            // Y 위치 보간: 모니터 간 work_bottom 차이로 인한 높이 점프 방지
                            if !smooth_y_init {
                                smooth_y = raw_target_y as f64;
                                smooth_y_init = true;
                            } else {
                                let diff = raw_target_y as f64 - smooth_y;
                                if diff.abs() < 1.0 {
                                    smooth_y = raw_target_y as f64;
                                } else {
                                    smooth_y += diff * 0.15;
                                }
                            }
                            let target_y = smooth_y as i32;

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
                                const SWP_NOACTIVATE: u32 = 0x0010;
                                const SWP_FRAMECHANGED: u32 = 0x0020;
                                const SW_SHOWNA: i32 = 8; // 활성화 없이 표시
                                // RedrawWindow 플래그
                                const RDW_INVALIDATE: u32 = 0x0001;
                                const RDW_ERASE: u32 = 0x0004;
                                const RDW_ALLCHILDREN: u32 = 0x0080;
                                const RDW_UPDATENOW: u32 = 0x0100;
                                const RDW_FRAME: u32 = 0x0400;

                                let insert_after = if on_fs_reader.load(Ordering::Relaxed) {
                                    HWND_NOTOPMOST
                                } else {
                                    HWND_TOPMOST
                                };

                                if cached_hwnd != 0 {
                                    unsafe {
                                        if do_redraw {
                                            // 모니터 변경: 크기 포함 SetWindowPos + SWP_FRAMECHANGED로
                                            // 프레임 속성 재적용 (투명도/DPI 갱신 트리거)
                                            SetWindowPos(
                                                cached_hwnd,
                                                insert_after,
                                                current_x as i32,
                                                target_y,
                                                actual_win_h, // 물리 높이를 cx에 전달하지 않고 아래에서 재계산
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
                                        } else {
                                            SetWindowPos(
                                                cached_hwnd,
                                                insert_after,
                                                current_x as i32,
                                                target_y,
                                                0,
                                                0,
                                                SWP_NOSIZE | SWP_NOACTIVATE,
                                            );
                                        }
                                        // 모니터 제거 시 OS가 윈도우를 숨길 수 있음
                                        // → 유효 좌표 이동 직후 동기적으로 강제 표시
                                        if needs_show || do_redraw {
                                            ShowWindow(cached_hwnd, SW_SHOWNA);
                                        }
                                    }
                                }
                            }
                            #[cfg(not(target_os = "windows"))]
                            {
                                let _ = window_clone.set_position(tauri::Position::Physical(
                                    tauri::PhysicalPosition::new(current_x as i32, target_y),
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
            update_pet_color,
            update_monitor_config,
            set_polling_interval,
            update_mouse_enabled,
            update_bubble_enabled,
            update_bubble_height,
            update_alarm_list,
            update_display_config,
            update_messages,
            update_msg_rotate,
            update_app_settings,
            get_auto_start,
            set_auto_start
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
