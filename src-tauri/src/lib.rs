use tauri::{AppHandle, Manager, Emitter, State};
use std::sync::{Arc, Mutex, RwLock};
use std::sync::atomic::{AtomicU32, AtomicBool, AtomicI64, AtomicI32, Ordering};

/// 한 번의 EnumWindows 순회로 여러 모니터의 전체화면 여부를 동시에 체크
/// monitors 슬라이스와 같은 길이의 bool 배열 반환 (인덱스 대응)
/// N번 호출 → 1번으로 줄여 API 오버헤드 감소
#[cfg(target_os = "windows")]
fn check_fullscreen_all(monitors: &[MonitorInfo]) -> Vec<bool> {
    if monitors.is_empty() { return Vec::new(); }

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
    const WS_EX_LAYERED: u32    = 0x00080000; // 투명/레이어드 창 (오버레이 앱)
    const WS_EX_TOOLWINDOW: u32 = 0x00000080; // 도구 창 (ALT+TAB 미표시 플로팅 창)

    // EnumWindows 콜백 컨텍스트 (모니터 목록과 결과 배열을 raw ptr로 공유)
    struct Ctx {
        monitors: *const MonitorInfo,
        count: usize,
        fullscreen: *mut bool,
    }

    unsafe extern "system" fn enum_proc(hwnd: isize, lparam: isize) -> i32 {
        let ctx = &*(lparam as *const Ctx);

        if IsWindowVisible(hwnd) == 0 { return 1; }

        // 시스템 창 제외 (바탕화면, 작업표시줄, UWP 시스템 UI)
        let mut cls = [0u16; 64];
        GetClassNameW(hwnd, cls.as_mut_ptr(), 64);
        fn cls_eq(buf: &[u16], name: &[u8]) -> bool {
            name.iter().enumerate().all(|(i, &b)| buf.get(i).copied() == Some(b as u16))
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
        if GetWindowRect(hwnd, &mut r) == 0 { return 1; }

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
            if !fullscreen[i] { all_found = false; }
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

#[derive(Clone, Default)]
struct MonitorInfo {
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    scale_factor: f64,
}
use tauri::{
    menu::{CheckMenuItem, Menu, MenuItem},
    tray::TrayIconBuilder,
};

struct AppState {
    is_hovered: Arc<Mutex<bool>>,
    test_cpu: Arc<AtomicI32>,
}

// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

#[tauri::command]
fn set_hover(state: State<'_, AppState>, hovered: bool) {
    if let Ok(mut is_hovered) = state.is_hovered.lock() {
        *is_hovered = hovered;
    }
}

#[tauri::command]
fn set_test_cpu(app: AppHandle, state: State<'_, AppState>, usage: i32) {
    state.test_cpu.store(usage, Ordering::Relaxed);
    // (설정 창으로 기능을 옮겼으므로 트레이 메뉴 재빌드 불필요)
}

#[tauri::command]
fn update_pet_color(app: AppHandle, hue: i32, saturation: i32, brightness: i32) {
    let _ = app.emit("color-update", serde_json::json!({
        "hue": hue,
        "saturation": saturation,
        "brightness": brightness
    }));
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
    let app_state_hover = Arc::new(Mutex::new(false));
    let thread_hover_state = Arc::clone(&app_state_hover);

    // 실행 상태 플래그: true = 실행 중, false = 중지
    // 이 플래그 하나로 CPU 폴링 스레드와 이동 스레드를 동시에 제어한다.
    let is_running = Arc::new(AtomicBool::new(true));

    // 테스트용 CPU 값: -1 = 실제 시스템 값 사용, 0~100 = 테스트 값 사용
    let test_cpu = Arc::new(AtomicI32::new(-1));

    // 캐릭터의 현재 물리 X 좌표를 Thread 2 → Thread 1 방향으로 공유 (전체화면 감지에 사용)
    let shared_pet_x = Arc::new(AtomicI64::new(0));

    // 전체화면이 아닌 이동 가능한 모니터 목록: Thread 1(갱신) → Thread 2(참조)
    let shared_avail_monitors = Arc::new(RwLock::new(Vec::<MonitorInfo>::new()));

    // setup 클로저(move)에 넘길 clone 미리 준비
    let is_running_tray = Arc::clone(&is_running);
    let _is_running_t1   = Arc::clone(&is_running);
    let _is_running_t2   = Arc::clone(&is_running);
    let shared_pet_x_t1 = Arc::clone(&shared_pet_x);
    let shared_pet_x_t2 = Arc::clone(&shared_pet_x);
    let avail_monitors_t1 = Arc::clone(&shared_avail_monitors);
    let avail_monitors_t2 = Arc::clone(&shared_avail_monitors);

    tauri::Builder::default()
        .manage(AppState {
            is_hovered: app_state_hover,
            test_cpu: Arc::clone(&test_cpu),
        })
        .plugin(tauri_plugin_opener::init())
        // move: is_running_tray/t1/t2, thread_hover_state 를 클로저 안으로 이동
        .setup(move |app| {
            // 트레이 메뉴 초기 빌드 (실행 중 상태 → 중지 메뉴 표시)
            let menu = build_tray_menu(app.handle(), true)?;

            if let Some(icon) = app.default_window_icon().cloned() {
                TrayIconBuilder::with_id("main-tray")
                    .icon(icon)
                    .menu(&menu)
                    .show_menu_on_left_click(false)
                    .on_menu_event(move |app, event| {
                        match event.id().as_ref() {
                            "settings" => {
                                if let Some(w) = app.get_webview_window("settings") {
                                    let _ = w.set_focus();
                                } else {
                                    let _ = tauri::webview::WebviewWindowBuilder::new(app, "settings", tauri::WebviewUrl::App("index.html".into()))
                                        .title("설정")
                                        .inner_size(400.0, 550.0)
                                        .decorations(true)
                                        .resizable(true)
                                        .center()
                                        .skip_taskbar(false) // 설정 창은 작업표시줄에 표시
                                        .build();
                                }
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
                    .build(app)?;
            }

            // Get the main window
            let window = app.get_webview_window("main").unwrap();
            
            // Get screen details to know when to wrap around
            // Use logical size and position for consistent cross-DPI behavior
            let monitor = window.primary_monitor().unwrap().unwrap();
            let scale_factor = window.scale_factor().unwrap_or(1.0);
            
            // Set initial window size (width 120 for bubble space, height 150)
            window.set_size(tauri::Size::Logical(tauri::LogicalSize {
                width: 120.0,
                height: 150.0, // Height increased for speech bubble space
            })).expect("Failed to set window size");

            // Position window at the bottom of the primary monitor
            // Calculate position to snap to bottom (leaving taskbar margin if needed)
            let window_height = 150.0; // Same as above
            let y = (monitor.size().height as f64 / scale_factor) - window_height - 40.0; // Approx taskbar height
            window.set_position(tauri::Position::Logical(tauri::LogicalPosition { x: 0.0, y })).unwrap();

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
                    if m.position().x < min_x { min_x = m.position().x; }
                }
                if min_x != i32::MAX { current_x = min_x as f64 - 100.0; }
            }
            
            let mut last_update = std::time::Instant::now();

            // 글로벌 공유 CPU 상태 (f32를 원자적으로 다루기 위해 bits 변환해서 AtomicU32 사용)
            let shared_cpu_usage = Arc::new(AtomicU32::new(0f32.to_bits()));
            // 추가: 글로벌 공유 모니터 정보 캐시 (1초 갱신)
            let shared_monitors = Arc::new(RwLock::new(Vec::<MonitorInfo>::new()));
            
            // 초기 모니터 정보 캐싱 + avail_monitors도 초기화(첫 1초 동안 이동 가능하도록)
            if let Ok(monitors) = window_clone.available_monitors() {
                let info_list: Vec<MonitorInfo> = monitors.iter().map(|m| MonitorInfo {
                    x: m.position().x,
                    y: m.position().y,
                    width: m.size().width as i32,
                    height: m.size().height as i32,
                    scale_factor: m.scale_factor(),
                }).collect();
                // shared_monitors 초기화
                if let Ok(mut cache) = shared_monitors.write() {
                    *cache = info_list.clone();
                }
                // avail_monitors도 동일하게 초기화 (전체화면 없다고 가정)
                if let Ok(mut cache) = shared_avail_monitors.write() {
                    *cache = info_list;
                }
            }
            
            // --- Thread 1: CPU 폴링 & 모니터 스캔 (1초에 1번) ---
            // 중지 상태일 때는 sleep만 하고 실제 폴링/이벤트 전송을 건너뜀 → CPU 점유 없음
            let cpu_usage_clone = Arc::clone(&shared_cpu_usage);
            let monitors_clone = Arc::clone(&shared_monitors);
            let window_clone_evt = window.clone();
            let is_running_t1 = Arc::clone(&is_running);
            let pet_x_reader  = Arc::clone(&shared_pet_x_t1);
            let avail_mons_writer = Arc::clone(&avail_monitors_t1);
            std::thread::spawn(move || {
                let mut sys = sysinfo::System::new(); // new_all() 대신 new()로 초경량화
                sys.refresh_memory(); // 초기 1회 메모리 로드
                
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

                    // 메모리 폴링 (CPU와 동일 주기, 추가 크레이트 없음)
                    sys.refresh_memory();
                    let total_mem = sys.total_memory();
                    let used_mem  = sys.used_memory();
                    let mem_pct = if total_mem > 0 {
                        (used_mem as f64 / total_mem as f64 * 100.0).round() as u32
                    } else {
                        0
                    };

                    // 공유 변수에 기록 (f32 -> u32 bits)
                    cpu_usage_clone.store(usage.to_bits(), Ordering::Relaxed);
                    
                    // 2. 모니터 정보 갱신 (1초마다 OS에 질의)
                    if let Ok(m_list) = window_clone_evt.available_monitors() {
                        let mut cache = monitors_clone.write().unwrap();
                        cache.clear();
                        for m in m_list {
                            cache.push(MonitorInfo {
                                x: m.position().x,
                                y: m.position().y,
                                width: m.size().width as i32,
                                height: m.size().height as i32,
                                scale_factor: m.scale_factor(),
                            });
                        }
                    }
                    
                    // 모든 모니터 전체화면 여부를 단 1번의 EnumWindows 순회로 처리
                    // monitors 락을 짧게 잡아 스냅샷 복사 → 락 해제 후 EnumWindows 실행
                    let monitors_snapshot = monitors_clone.read()
                        .map(|g| g.clone())
                        .unwrap_or_default();

                    let pet_x = pet_x_reader.load(Ordering::Relaxed);
                    let center_x = pet_x + 60; // Adjusted for 120px window width

                    // 단일 EnumWindows 호출로 모든 모니터 결과 획득 (최적화 핵심)
                    let fs_flags = check_fullscreen_all(&monitors_snapshot);

                    let mut avail = Vec::with_capacity(monitors_snapshot.len());
                    let mut pet_on_fullscreen = false;
                    for (m, &fs) in monitors_snapshot.iter().zip(fs_flags.iter()) {
                        if !fs {
                            avail.push(m.clone());
                        }
                        if center_x >= m.x as i64 && center_x < (m.x + m.width) as i64 && fs {
                            pet_on_fullscreen = true;
                        }
                    }

                    // alwaysOnTop: pet이 전체화면 모니터 위에 있을 때만 해제
                    let _ = window_clone_evt.set_always_on_top(!pet_on_fullscreen);

                    // 이동 가능한 모니터 목록 갱신 (Thread 2 이동 제한에 사용)
                    if let Ok(mut cache) = avail_mons_writer.write() {
                        *cache = avail;
                    }

                    // React 프론트로 1초마다 안정적으로 이벤트 전송
                    let _ = window_clone_evt.emit("cpu-usage", usage);
                    let _ = window_clone_evt.emit("memory-usage", mem_pct);

                    std::thread::sleep(std::time::Duration::from_millis(1000));
                }
            });

            // --- Thread 2: 윈도우 이동 및 프레임(16ms) 업데이트 ---
            // 중지 상태일 때는 sleep만 하고 set_position 호출 없음 → GPU/CPU 점유 없음
            let cpu_usage_reader = Arc::clone(&shared_cpu_usage);
            let avail_monitors_reader = Arc::clone(&avail_monitors_t2);
            let is_running_t2 = Arc::clone(&is_running);
            let pet_x_writer  = Arc::clone(&shared_pet_x_t2);
            let test_cpu_t2   = Arc::clone(&test_cpu);
            std::thread::spawn(move || {
                loop {
                    std::thread::sleep(std::time::Duration::from_millis(16)); // ~60 FPS update rate for smooth window movement

                    // 중지 상태: 루프는 돌지만 실제 이동 로직 없음 → 사실상 idle
                    if !is_running_t2.load(Ordering::Relaxed) {
                        continue;
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
                    let delta_time = now.duration_since(last_update).as_secs_f64();
                    last_update = now;
                    
                    let mut speed_multiplier = 1.0 + (current_cpu as f64 / 10.0);
                    
                    // IF hovered, stop movement completely
                    if let Ok(hovered) = thread_hover_state.lock() {
                        if *hovered {
                            speed_multiplier = 0.0;
                        }
                    }

                    // 이동 가능한 모니터 목록(전체화면 제외)에서만 이동
                    if let Ok(monitors) = avail_monitors_reader.read() {
                        if !monitors.is_empty() {
                            let mut max_x = i32::MIN;
                            let mut min_x = i32::MAX;
                            let mut current_scale = 1.0;
                            let mut target_y = 0;
                            let mut found_monitor = false;
                            
                            // Use the center of the window (approx 60px scaled) to determine which monitor we are on.
                            let center_x = current_x + 60.0;
                            
                            // Find the total span and which monitor we are currently on
                            for m in monitors.iter() {
                                let px = m.x;
                                let py = m.y;
                                let pw = m.width;
                                let ph = m.height;
                                let scale = m.scale_factor;
                                
                                if px < min_x { min_x = px; }
                                if px + pw > max_x { max_x = px + pw; }
                                
                                // If the pet's CENTER X is within this monitor's X bounds
                                if center_x >= (px as f64) && center_x <= ((px + pw) as f64) {
                                    current_scale = scale;
                                    let physical_win_h = (150.0 * scale) as i32;
                                    let physical_margin = (40.0 * scale) as i32; // Taskbar margin
                                    target_y = py + ph - physical_win_h - physical_margin;
                                    found_monitor = true;
                                }
                            }
                            
                            // Fallback bounds
                            if min_x == i32::MAX {
                                min_x = 0;
                                max_x = 1920;
                            }

                            // Apply scaled movement speed (lowered base speed from 50 to 35)
                            let movement = 35.0 * current_scale * speed_multiplier * delta_time;
                            current_x += movement;
                            // Thread 1의 전체화면 감지가 올바른 모니터를 알 수 있도록 공유
                            pet_x_writer.store(current_x as i64, Ordering::Relaxed);

                            // 🚨 [EDGE CASE] 모니터 핫플러깅 대응 (탈출 & 텔레포트)
                            // 1) 뼈다귀가 오른쪽 끝(전체 max_x)을 넘었을 때 (정상적인 랩어라운드 또는 우측 모니터 뽑힘)
                            // 2) 뼈다귀의 위치가 왼쪽 끝(전체 min_x)보다 작을 때 (좌측 모니터 뽑힘/미아 상태)
                            if current_x > max_x as f64 || current_x < (min_x as f64 - 200.0) {
                                current_x = min_x as f64 - 100.0;
                            }
                            
                            // 공중에 떠버린 미아 모니터 처리 (발생 시 1번 모니터로 강제 소환)
                             if !found_monitor {
                                if let Some(pm) = monitors.first() {
                                    let scale = pm.scale_factor;
                                    target_y = pm.y + pm.height - (150.0 * scale) as i32 - (40.0 * scale) as i32;
                                }
                                // 베젤(모니터 여백)을 지날 때는 찾지 못할 수 있으므로 강제 x 워프는 제거함 (자연스럽게 넘어가도록 둠)
                            }

                            // Apply absolute Physical position
                            let _ = window_clone.set_position(tauri::Position::Physical(tauri::PhysicalPosition::new(
                                current_x as i32, 
                                target_y
                            )));
                        }
                    }
                }
            });

            Ok(())

        })
        .invoke_handler(tauri::generate_handler![greet, set_hover, set_test_cpu, update_pet_color])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
