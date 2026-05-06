use std::sync::atomic::{AtomicBool, AtomicI32, AtomicI64, AtomicU8, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, RwLock};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;
use rand::Rng;
use tauri::{AppHandle, Emitter, Manager, State};

mod mail;

/// Windows에서 GUI 앱이 콘솔 자식 프로세스(reg, curl 등)를 spawn할 때
/// 콘솔 창이 깜빡 뜨는 현상을 막기 위한 CreateProcess 플래그.
/// `CommandExt::creation_flags`에 전달하여 자식에 콘솔이 할당되지 않도록 한다.
#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// 모니터링 항목 활성 비트마스크.
/// 사용자가 끈 항목은 백엔드 폴링 스레드에서 측정·emit을 모두 건너뛴다.
/// CPU/메모리는 펫 이동 속도 계산 + 메시지 평가에 항상 필요하므로 flag 대상에서 제외.
const MONITOR_FLAG_GPU: u8 = 1 << 0;
const MONITOR_FLAG_NETWORK: u8 = 1 << 1;
const MONITOR_FLAG_BATTERY: u8 = 1 << 2;
/// 기본값: 모든 항목 활성. frontend가 init sync로 실제 사용자 설정을 즉시 반영한다.
const MONITOR_FLAGS_DEFAULT: u8 =
    MONITOR_FLAG_GPU | MONITOR_FLAG_NETWORK | MONITOR_FLAG_BATTERY;

/// WebView2 사용자 데이터 디렉터리 경로 반환.
/// 앱 설치 폴더(`%LocalAppData%\TaskMon`)와 동일한 위치를 사용하여
/// 설치 폴더와 사용자 설정이 한 곳에서 관리되도록 한다.
/// 결과적으로 LocalStorage 경로는 `%LocalAppData%\TaskMon\EBWebView\Default\Local Storage\` 가 된다.
fn webview_data_directory() -> PathBuf {
    let local_app_data = std::env::var("LOCALAPPDATA")
        .unwrap_or_else(|_| String::from(r"C:\Users\Default\AppData\Local"));
    PathBuf::from(local_app_data).join("TaskMon")
}

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

/// GPU 사용률 모니터링 모듈 (Windows 전용).
/// PDH(Performance Data Helper)의 `\GPU Engine(*)\Utilization Percentage` 카운터를 사용해
/// 모든 GPU 어댑터·엔진 인스턴스의 사용률을 합산하고 100%로 캡한다.
/// Windows 작업 관리자(taskmgr.exe)와 동일한 방식이며, NVIDIA/AMD/Intel 등 제조사 무관하게 동작한다.
#[cfg(target_os = "windows")]
mod gpu_monitor {
    use windows::core::PCWSTR;
    use windows::Win32::System::Performance::{
        PdhAddEnglishCounterW, PdhCloseQuery, PdhCollectQueryData,
        PdhGetFormattedCounterArrayW, PdhOpenQueryW,
        PDH_FMT_DOUBLE, PDH_FMT_COUNTERVALUE_ITEM_W,
    };

    // PDH 반환 코드: ERROR_SUCCESS = 0, PDH_MORE_DATA = 0x800007D2
    const ERROR_SUCCESS: u32 = 0;
    const PDH_MORE_DATA: u32 = 0x800007D2;

    pub struct GpuMonitor {
        query: isize,    // PDH_HQUERY는 isize의 newtype이지만 0.58에서 직접 isize 호환
        counter: isize,  // PDH_HCOUNTER 동일
    }

    impl GpuMonitor {
        /// PDH 쿼리를 열고 GPU Engine 카운터를 등록한다.
        /// 첫 호출은 baseline 수집(첫 poll 직후 ~1초간 0%로 표시되는 PDH 동작 특성).
        /// 실패 시 None을 반환하며 패닉하지 않는다(GPU 미지원 환경 호환).
        pub fn new() -> Option<Self> {
            unsafe {
                let mut query: isize = 0;
                if PdhOpenQueryW(PCWSTR::null(), 0, &mut query) != ERROR_SUCCESS {
                    return None;
                }
                // 모든 GPU 어댑터의 모든 엔진 인스턴스를 매칭하는 와일드카드 경로
                let path: Vec<u16> = "\\GPU Engine(*)\\Utilization Percentage\0"
                    .encode_utf16()
                    .collect();
                let mut counter: isize = 0;
                if PdhAddEnglishCounterW(query, PCWSTR(path.as_ptr()), 0, &mut counter)
                    != ERROR_SUCCESS
                {
                    let _ = PdhCloseQuery(query);
                    return None;
                }
                // baseline 수집 — 반환값 무시(첫 호출은 의미 없는 값)
                let _ = PdhCollectQueryData(query);
                Some(Self { query, counter })
            }
        }

        /// 현재 시점의 GPU 사용률을 0.0~100.0 범위로 반환.
        /// 모든 인스턴스의 사용률을 합산한 뒤 100%로 캡한다(작업 관리자 표시 방식).
        /// 카운터 인스턴스가 없거나 측정 실패 시 None.
        pub fn poll(&self) -> Option<f64> {
            unsafe {
                if PdhCollectQueryData(self.query) != ERROR_SUCCESS {
                    return None;
                }
                let mut buffer_size: u32 = 0;
                let mut item_count: u32 = 0;
                // 1차: 필요한 버퍼 크기를 조회 (PDH_MORE_DATA 반환 예상)
                let rc1 = PdhGetFormattedCounterArrayW(
                    self.counter,
                    PDH_FMT_DOUBLE,
                    &mut buffer_size,
                    &mut item_count,
                    None,
                );
                // item_count가 0이면 GPU 카운터 인스턴스가 없는 환경(가상 머신 등)
                if rc1 == ERROR_SUCCESS && item_count == 0 {
                    return Some(0.0);
                }
                if rc1 != PDH_MORE_DATA && rc1 != ERROR_SUCCESS {
                    return None;
                }
                if buffer_size == 0 || item_count == 0 {
                    return Some(0.0);
                }
                // 2차: 실제 값 수집
                let mut buffer = vec![0u8; buffer_size as usize];
                let items_ptr = buffer.as_mut_ptr() as *mut PDH_FMT_COUNTERVALUE_ITEM_W;
                let rc2 = PdhGetFormattedCounterArrayW(
                    self.counter,
                    PDH_FMT_DOUBLE,
                    &mut buffer_size,
                    &mut item_count,
                    Some(items_ptr),
                );
                if rc2 != ERROR_SUCCESS {
                    return None;
                }
                let slice = std::slice::from_raw_parts(items_ptr, item_count as usize);
                let mut total: f64 = 0.0;
                for item in slice {
                    let v = item.FmtValue.Anonymous.doubleValue;
                    if v.is_finite() && v > 0.0 {
                        total += v;
                    }
                }
                // 작업 관리자와 동일하게 100%로 캡 (다중 엔진 합산 시 100%를 초과할 수 있음)
                Some(total.min(100.0))
            }
        }
    }

    impl Drop for GpuMonitor {
        fn drop(&mut self) {
            unsafe {
                let _ = PdhCloseQuery(self.query);
            }
        }
    }

    // 단일 폴링 스레드에서만 사용하지만, 스레드 간 이동 가능하도록 Send 구현.
    // PDH 핸들은 OS 레벨이므로 스레드 이동 자체는 안전하다.
    unsafe impl Send for GpuMonitor {}
}

/// 비-Windows 환경용 더미 stub (모든 호출이 None 반환).
#[cfg(not(target_os = "windows"))]
mod gpu_monitor {
    pub struct GpuMonitor;
    impl GpuMonitor {
        pub fn new() -> Option<Self> { None }
        pub fn poll(&self) -> Option<f64> { None }
    }
}

/// 폴링 스레드(Thread 1)의 reset 가능한 thread-local 상태 묶음.
/// "중지→시작은 앱 재실행과 같다"는 불변식이 변수마다 여러 곳에서 중복되지 않도록
/// 단일 `fresh()` 생성자에 초기값을 모은다. 새 변수 추가 시에도 drift가 발생하지 않는다.
struct PollingThreadState {
    battery_elapsed_ms: u64,        // ~3분 카운터, 178_000으로 시작해 첫 측정 ~2초 후 트리거
    cached_battery_percent: i32,    // 배터리 잔량 캐시 (-1 = 배터리 없음/미측정)
    prev_charging: bool,            // 충전 상태 전환 감지용
    monitor_refresh_elapsed_ms: u64, // 10초 모니터 enumerate 카운터
    network_refresh_elapsed_ms: u64, // 30초 NIC 인터페이스 재구성 카운터
    prev_monitor_count: usize,      // 모니터 핫플러그 감지용
    prev_on_fullscreen: bool,       // 전체화면 진입/이탈 감지용
    prev_cpu_pct: i32,              // CPU emit dedup (i32::MIN = 첫 진입 sentinel)
    prev_mem_pct: i32,              // 메모리 emit dedup
    prev_gpu_pct: i32,              // GPU emit dedup
}

impl PollingThreadState {
    /// 첫 진입 + 중지→시작 reset에서 공통으로 사용하는 초기 상태.
    fn fresh() -> Self {
        Self {
            battery_elapsed_ms: 178_000,
            cached_battery_percent: -1,
            prev_charging: false,
            monitor_refresh_elapsed_ms: 10_000,
            network_refresh_elapsed_ms: 30_000,
            prev_monitor_count: 0,
            prev_on_fullscreen: false,
            prev_cpu_pct: i32::MIN,
            prev_mem_pct: i32::MIN,
            prev_gpu_pct: i32::MIN,
        }
    }
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
    /// 실행 상태 (트레이 메뉴 재빌드용)
    is_running: Arc<AtomicBool>,
    /// 현재 언어 설정 (트레이 메뉴 번역용)
    tray_language: Arc<std::sync::Mutex<String>>,
    /// 모니터링 항목 활성 비트마스크 (GPU/NETWORK/BATTERY).
    /// 비활성 항목은 폴링 스레드에서 측정 자체를 건너뛴다.
    monitor_flags: Arc<AtomicU8>,
    /// 메일 알림 폴링 런타임 상태 (await 경계를 넘기 위해 tokio Mutex 사용)
    mail_runtime: Arc<tokio::sync::Mutex<MailRuntimeState>>,
    /// 메일 폴링 즉시 트리거 (설정 변경/테스트 시)
    mail_trigger: Arc<tokio::sync::Notify>,
}

/// 메일 폴링 태스크 런타임 상태
struct MailRuntimeState {
    /// 현재 적용된 설정 (없으면 폴링 안 함)
    cfg: Option<mail::MailConfig>,
    /// baseline UIDL → seen_at(unix timestamp). STALE_DAYS 경과 시 자동 정리.
    last_seen: HashMap<String, i64>,
    /// 인증 오류로 폴링 일시 정지됨 (사용자가 자격 증명 재입력 시 false로 리셋)
    paused_due_to_auth: bool,
    /// 마지막 폴링 결과 오류
    last_error: Option<mail::MailError>,
    /// 첫 폴링 여부 (baseline만 등록, 알림 발화 안 함)
    first_poll_pending: bool,
    /// 설정 변경 카운터 — in-flight 폴링 결과 적용 시 일치 검사
    config_version: u64,
}

impl Default for MailRuntimeState {
    fn default() -> Self {
        Self {
            cfg: None,
            last_seen: HashMap::new(),
            paused_due_to_auth: false,
            last_error: None,
            first_poll_pending: true,
            config_version: 0,
        }
    }
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

/// 표시 설정(모니터링/알림 문구 표시 여부, 알림·메일 표시 시간)을 동기화
#[tauri::command]
fn update_display_config(app: AppHandle, show_monitoring: bool, show_notification: bool, notification_priority: bool, notification_mode: String, notification_duration: u32, mail_duration: u32) {
    let _ = app.emit("display-config-update", serde_json::json!({
        "showMonitoringText": show_monitoring,
        "showNotificationText": show_notification,
        "notificationPriority": notification_priority,
        "notificationMode": notification_mode,
        "notificationDuration": notification_duration,
        "mailDuration": mail_duration
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

/// 폰트/언어 설정 동기화 (설정 → 메인 윈도우) + 트레이 메뉴 언어 갱신
#[tauri::command]
fn update_app_settings(app: AppHandle, state: State<'_, AppState>, language: String, font_size: u32, font_family: String, monitoring_font_color: String, alarm_font_color: String) {
    // 트레이 메뉴 언어 갱신
    let prev_lang = {
        let mut lang = state.tray_language.lock().unwrap();
        let prev = lang.clone();
        *lang = language.clone();
        prev
    };
    // 언어가 변경되었으면 트레이 메뉴 재빌드
    if prev_lang != language {
        let running = state.is_running.load(Ordering::Relaxed);
        if let Ok(new_menu) = build_tray_menu(&app, running, &language) {
            if let Some(tray) = app.tray_by_id("main-tray") {
                let _ = tray.set_menu(Some(new_menu));
            }
        }
    }
    let _ = app.emit("app-settings-update", serde_json::json!({
        "language": language,
        "fontSize": font_size,
        "fontFamily": font_family,
        "monitoringFontColor": monitoring_font_color,
        "alarmFontColor": alarm_font_color
    }));
}

/// 프론트엔드 MonitorConfig와 1:1 대응되는 모니터링 설정 페이로드.
/// `update_monitor_config` 인자 sprawl 방지를 위해 단일 구조체로 받는다.
/// 직렬화 시 그대로 메인 윈도우에 emit되므로 camelCase 그대로 유지한다.
#[derive(serde::Deserialize, serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct MonitorConfigPayload {
    cpu: bool,
    gpu: bool,
    memory: bool,
    network: bool,
    battery: bool,
    show_charging_icon: bool,
    charging_icon_size: String,
    charging_icon_distance: i32,
}

#[tauri::command]
fn update_monitor_config(app: AppHandle, state: State<'_, AppState>, config: MonitorConfigPayload) {
    // 폴링 스레드가 매 루프 시작 시 load할 비트마스크 갱신.
    // 비활성 항목은 백엔드에서 측정·emit 모두 건너뛴다.
    let mut flags: u8 = 0;
    if config.gpu { flags |= MONITOR_FLAG_GPU; }
    if config.network { flags |= MONITOR_FLAG_NETWORK; }
    if config.battery { flags |= MONITOR_FLAG_BATTERY; }
    state.monitor_flags.store(flags, Ordering::Relaxed);

    let _ = app.emit("monitor-config-update", &config);
}

/// 자동 실행 레지스트리 조회 (async로 메인 스레드 블로킹 방지)
#[tauri::command]
async fn get_auto_start() -> bool {
    #[cfg(target_os = "windows")]
    {
        use std::process::Command;
        use std::os::windows::process::CommandExt;
        let output = Command::new("reg")
            .args(["query", r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run", "/v", "TaskMon"])
            .creation_flags(CREATE_NO_WINDOW)
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
        use std::os::windows::process::CommandExt;
        if enabled {
            // 현재 실행 파일 경로를 레지스트리에 등록
            let exe = std::env::current_exe().map_err(|e| e.to_string())?;
            let exe_path = exe.to_string_lossy().to_string();
            let output = Command::new("reg")
                .args(["add", r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run", "/v", "TaskMon", "/t", "REG_SZ", "/d", &exe_path, "/f"])
                .creation_flags(CREATE_NO_WINDOW)
                .output()
                .map_err(|e| e.to_string())?;
            if !output.status.success() {
                return Err(String::from_utf8_lossy(&output.stderr).to_string());
            }
        } else {
            let output = Command::new("reg")
                .args(["delete", r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run", "/v", "TaskMon", "/f"])
                .creation_flags(CREATE_NO_WINDOW)
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

/// 업데이트 정보 (프론트엔드로 전달되는 최신 릴리즈 메타데이터)
#[derive(serde::Serialize)]
struct UpdateInfo {
    /// 'v' 접두사를 제거한 버전 문자열 (예: "0.1.1")
    latest_version: String,
    /// 원본 태그 문자열 (예: "v0.1.1")
    tag: String,
    /// 인스톨러 다운로드 URL (browser_download_url)
    download_url: String,
    /// 자산 파일 이름 (예: "TaskMon-Setup-v0.1.1.exe")
    asset_name: String,
    /// 릴리즈 노트 본문에서 추출한 SHA256 체크섬(소문자 hex 64자).
    /// 노트에 체크섬이 없으면 None.
    sha256: Option<String>,
}

/// 릴리즈 노트 본문에서 SHA256 64자 hex 문자열을 추출한다.
/// 비-hex 문자(공백, 구두점 등)를 구분자로 사용해 토큰 분리 → 길이 64인 hex 토큰을 첫 번째로 반환.
/// `is_ascii_hexdigit`로 split하므로 ASCII 안전이며 외부 정규식 의존성도 없다.
fn extract_sha256_from_body(body: &str) -> Option<String> {
    body.split(|c: char| !c.is_ascii_hexdigit())
        .find(|tok| tok.len() == 64)
        .map(|s| s.to_ascii_lowercase())
}

/// 파일의 SHA256을 계산하여 소문자 hex 문자열로 반환한다.
/// 64KB 청크 단위로 읽어 큰 인스톨러(수십 MB)도 메모리 부담 없이 처리한다.
#[cfg(target_os = "windows")]
fn compute_file_sha256(path: &std::path::Path) -> Result<String, String> {
    use sha2::{Digest, Sha256};
    use std::fmt::Write as _;
    use std::io::Read;

    let mut file = std::fs::File::open(path)
        .map_err(|e| format!("파일 열기 실패: {}", e))?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf).map_err(|e| format!("파일 읽기 실패: {}", e))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    let digest = hasher.finalize();
    // String을 한 번만 할당해 32회 write로 채움 — 바이트당 format! 호출 시 발생하는 32회 미세 할당 제거.
    let mut s = String::with_capacity(64);
    for b in digest.iter() {
        let _ = write!(s, "{:02x}", b);
    }
    Ok(s)
}

/// 'v' 접두사를 무시하고 점(.)으로 구분된 정수 버전 시퀀스로 파싱한다.
/// 비교 시 사전식(lexicographic) 비교가 SemVer 사전식과 동일한 결과를 갖도록 한다.
fn parse_version_components(s: &str) -> Vec<u32> {
    s.trim_start_matches('v')
        .split('.')
        .filter_map(|p| p.parse::<u32>().ok())
        .collect()
}

/// `latest`가 `current`보다 높은 버전인지 판단한다.
fn is_newer_version(latest: &str, current: &str) -> bool {
    parse_version_components(latest) > parse_version_components(current)
}

/// GitHub Releases API에서 최신 릴리즈 메타데이터를 조회한다.
/// 반환값:
///   * `Ok(Some(UpdateInfo))` — 새 버전이 존재
///   * `Ok(None)`              — 현재 최신 버전 사용 중
///   * `Err(String)`           — 네트워크 오류 / 파싱 오류
///
/// `curl.exe`(Windows 10 1803+ 기본 포함)를 사용해 외부 의존성 추가 없이 HTTP 요청을 수행한다.
/// async tauri::command이므로 Tauri 런타임의 워커 스레드에서 실행되어 UI 스레드를 차단하지 않는다.
#[tauri::command]
async fn check_update() -> Result<Option<UpdateInfo>, String> {
    #[cfg(target_os = "windows")]
    {
        use std::process::Command;
        use std::os::windows::process::CommandExt;

        // GitHub Releases 최신 릴리즈 엔드포인트
        const REPO_API: &str = "https://api.github.com/repos/jongcheol-pak/TaskMon/releases/latest";
        let current_version = env!("CARGO_PKG_VERSION");

        // GitHub API는 User-Agent 헤더가 없으면 403을 반환한다.
        let output = Command::new("curl")
            .args([
                "-sL",
                "-H", "User-Agent: TaskMon-UpdateCheck",
                "-H", "Accept: application/vnd.github+json",
                REPO_API,
            ])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .map_err(|e| format!("curl 실행 실패: {}", e))?;

        if !output.status.success() {
            return Err(format!(
                "GitHub API 호출 실패: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }

        // 릴리즈 응답 파싱
        let json: serde_json::Value = serde_json::from_slice(&output.stdout)
            .map_err(|e| format!("응답 파싱 실패: {}", e))?;

        let tag = json
            .get("tag_name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "tag_name 필드가 없습니다".to_string())?;

        let latest_version = tag.trim_start_matches('v');

        // 버전이 더 높지 않으면 업데이트 없음
        if !is_newer_version(latest_version, current_version) {
            return Ok(None);
        }

        // assets 배열에서 TaskMon-Setup-*.exe 자산 검색
        let assets = json
            .get("assets")
            .and_then(|v| v.as_array())
            .ok_or_else(|| "assets 필드가 없습니다".to_string())?;

        let asset = assets
            .iter()
            .find(|a| {
                a.get("name")
                    .and_then(|v| v.as_str())
                    .map(|n| n.starts_with("TaskMon-Setup-") && n.ends_with(".exe"))
                    .unwrap_or(false)
            })
            .ok_or_else(|| "릴리즈에 TaskMon-Setup-*.exe 자산이 없습니다".to_string())?;

        let asset_name = asset
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "자산 이름을 찾을 수 없습니다".to_string())?
            .to_string();

        let download_url = asset
            .get("browser_download_url")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "다운로드 URL을 찾을 수 없습니다".to_string())?
            .to_string();

        // 릴리즈 노트 본문에서 SHA256 추출 (체크섬이 없으면 None)
        let sha256 = json
            .get("body")
            .and_then(|v| v.as_str())
            .and_then(extract_sha256_from_body);

        Ok(Some(UpdateInfo {
            latest_version: latest_version.to_string(),
            tag: tag.to_string(),
            download_url,
            asset_name,
            sha256,
        }))
    }
    #[cfg(not(target_os = "windows"))]
    {
        Ok(None)
    }
}

/// 인스톨러를 임시 폴더로 다운로드한 뒤 SHA256 무결성 검증을 거쳐 실행하고 현재 앱을 종료한다.
/// `expected_sha256`이 `Some`이면 다운로드 후 SHA256을 비교해 불일치 시 파일 삭제 + 에러 반환.
/// `None`이면 검증을 건너뛰며 (릴리즈 노트에 체크섬이 없는 구버전 호환), 프론트엔드는 사전에 사용자에게 안내한다.
/// async tauri::command이므로 다운로드 동안 UI 스레드가 멈추지 않는다.
#[tauri::command]
async fn download_and_install_update(
    app: AppHandle,
    url: String,
    file_name: String,
    expected_sha256: Option<String>,
) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        use std::process::Command;
        use std::os::windows::process::CommandExt;

        // 사용자 다운로드 폴더에 자산 이름 그대로 저장 (예: %USERPROFILE%\Downloads\TaskMon-Setup-v0.1.1.exe)
        // Tauri PathResolver는 Known Folder API 기반이라 사용자가 폴더 위치를 변경한 경우에도 정확한 경로 반환.
        let download_dir = app
            .path()
            .download_dir()
            .map_err(|e| format!("다운로드 폴더 경로 조회 실패: {}", e))?;
        let installer_path = download_dir.join(&file_name);
        let installer_path_str = installer_path.to_string_lossy().to_string();

        // -L: GitHub Releases 다운로드 URL이 S3로 리다이렉트되므로 필수
        let output = Command::new("curl")
            .args([
                "-sL",
                "-H", "User-Agent: TaskMon-Update",
                "-o", &installer_path_str,
                &url,
            ])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .map_err(|e| format!("curl 실행 실패: {}", e))?;

        if !output.status.success() {
            return Err(format!(
                "다운로드 실패: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }

        // SHA256 무결성 검증 (체크섬이 제공된 경우에만)
        if let Some(expected) = expected_sha256 {
            let expected_norm = expected.trim().to_ascii_lowercase();
            let actual = compute_file_sha256(&installer_path)?;
            if expected_norm != actual {
                // 무결성 실패: 손상되었거나 변조된 파일이므로 즉시 삭제
                let _ = std::fs::remove_file(&installer_path);
                return Err(format!(
                    "SHA256 무결성 검증 실패 (예상={}, 실제={})",
                    expected_norm, actual
                ));
            }
        }

        // 인스톨러를 분리된 프로세스로 실행 (사용자가 NSIS 단계 진행)
        Command::new(&installer_path)
            .spawn()
            .map_err(|e| format!("인스톨러 실행 실패: {}", e))?;

        // 인스톨러가 안정적으로 시작할 시간을 확보한 뒤 현재 앱 종료
        let app_handle = app.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(800));
            app_handle.exit(0);
        });

        Ok(())
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (app, url, file_name, expected_sha256);
        Err("Windows에서만 지원됩니다".to_string())
    }
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
        .data_directory(webview_data_directory())
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

/// 언어 문자열을 한국어 여부로 판정 ("ko", "system" + 시스템 한국어 → true)
fn is_korean(lang: &str) -> bool {
    if lang == "ko" { return true; }
    if lang == "en" { return false; }
    // "system" 또는 기타 → 시스템 로케일 기준
    sys_locale::get_locale().unwrap_or_default().starts_with("ko")
}

/// 현재 실행 상태와 언어에 따라 트레이 메뉴를 동적으로 빌드
fn build_tray_menu(app: &AppHandle, running: bool, lang: &str) -> tauri::Result<Menu<tauri::Wry>> {
    let ko = is_korean(lang);
    let quit_i = MenuItem::with_id(app, "quit", if ko { "종료" } else { "Quit" }, true, None::<&str>)?;
    let settings_i = MenuItem::with_id(app, "settings", if ko { "설정" } else { "Settings" }, true, None::<&str>)?;

    if running {
        let stop_i = MenuItem::with_id(app, "stop", if ko { "중지" } else { "Stop" }, true, None::<&str>)?;
        Menu::with_items(app, &[&settings_i, &stop_i, &quit_i])
    } else {
        let start_i = MenuItem::with_id(app, "start", if ko { "시작" } else { "Start" }, true, None::<&str>)?;
        Menu::with_items(app, &[&settings_i, &start_i, &quit_i])
    }
}

// ===== 메일 알림 (POP3) — 커맨드 + 폴링 태스크 =====

/// 메일 설정 로드 (UI 초기화용). 비밀번호는 반환하지 않고 has_password 플래그만 전달
#[tauri::command]
fn mail_load_config() -> mail::MailConfigLoad {
    if let Some(stored) = mail::load_stored() {
        mail::MailConfigLoad {
            config: stored.config,
            has_password: !stored.password_dpapi.is_empty(),
        }
    } else {
        mail::MailConfigLoad::default()
    }
}

/// 디스크 저장 데이터를 갱신한다.
/// password_plain_opt: Some(...)이면 새 평문 비밀번호로 DPAPI 갱신,
///                     None이면 기존 DPAPI 비밀번호 유지
/// new_baseline: Some(...)이면 baseline UIDL+seen_at 매핑을 갱신
fn persist_mail(
    new_meta: &mail::MailConfigMeta,
    password_plain_opt: Option<&str>,
    new_baseline: Option<&HashMap<String, i64>>,
) -> Result<(), String> {
    let mut stored = mail::load_stored().unwrap_or_default();
    stored.config = new_meta.clone();
    if let Some(plain) = password_plain_opt {
        if plain.is_empty() {
            stored.password_dpapi.clear();
        } else {
            stored.password_dpapi = mail::dpapi_protect(plain.as_bytes())?;
        }
    }
    if let Some(map) = new_baseline {
        stored.last_seen = map
            .iter()
            .map(|(uidl, seen_at)| mail::UidlEntry {
                uidl: uidl.clone(),
                seen_at: *seen_at,
            })
            .collect();
        // 구버전 필드는 더 이상 사용하지 않으므로 비움
        stored.last_seen_uidls.clear();
    }
    mail::save_stored(&stored)
}

/// 설정 적용 — UI 저장 버튼에서 호출.
/// 비밀번호가 빈 문자열이면 기존 DPAPI 값 유지, 아니면 새 값으로 DPAPI 재암호화.
/// host 또는 user_id가 직전과 다르면 baseline UIDL을 비우고 first_poll_pending을 리셋한다
/// (다른 메일 계정으로 변경 시 새 계정의 모든 메일이 신규 알림으로 폭주하는 것을 방지).
#[tauri::command]
async fn mail_apply_config(
    state: State<'_, AppState>,
    cfg: mail::MailConfig,
) -> Result<(), String> {
    mail::validate_config(&cfg)?;
    let meta = mail::meta_from_config(&cfg);

    // 직전 저장된 host/user_id와 비교해 자격 증명이 바뀌었는지 판정
    let credentials_changed = match mail::load_stored() {
        Some(prev) => prev.config.host != meta.host || prev.config.user_id != meta.user_id,
        None => false, // 최초 저장이라면 baseline 비교 대상 없음
    };

    // 비밀번호 처리 — 빈 문자열이면 기존값 유지
    let password_to_persist = if cfg.password.is_empty() {
        None
    } else {
        Some(cfg.password.as_str())
    };

    // 자격 증명이 바뀐 경우 디스크 baseline도 비움 (다음 폴링이 새 계정에 대해 baseline 등록부터 시작)
    let empty_baseline: HashMap<String, i64> = HashMap::new();
    let baseline_arg = if credentials_changed {
        Some(&empty_baseline)
    } else {
        None
    };
    persist_mail(&meta, password_to_persist, baseline_arg)?;

    // 런타임 상태 갱신
    {
        let mut runtime = state.mail_runtime.lock().await;
        runtime.cfg = Some(cfg);
        runtime.paused_due_to_auth = false;
        runtime.last_error = None;
        runtime.config_version = runtime.config_version.wrapping_add(1);
        if credentials_changed {
            runtime.last_seen.clear();
            runtime.first_poll_pending = true;
        }
    }

    // 즉시 1회 폴링 트리거
    state.mail_trigger.notify_one();
    Ok(())
}

/// 즉시 1회 연결 테스트 — 설정 화면 "테스트" 버튼용
#[tauri::command]
async fn mail_test_connection(cfg: mail::MailConfig) -> Result<(), mail::MailError> {
    mail::validate_config(&cfg).map_err(mail::MailError::Network)?;

    // 비밀번호가 빈 문자열이면 DPAPI에서 가져옴
    let mut full_cfg = cfg;
    if full_cfg.password.is_empty() {
        if let Some(stored) = mail::load_stored() {
            if !stored.password_dpapi.is_empty() {
                let plain = mail::dpapi_unprotect(&stored.password_dpapi)
                    .map_err(mail::MailError::Network)?;
                full_cfg.password = String::from_utf8_lossy(&plain).to_string();
            }
        }
    }

    // 블로킹 작업 → 별도 스레드
    let result = tokio::task::spawn_blocking(move || {
        let empty: HashMap<String, i64> = HashMap::new();
        // 테스트는 신규 메일 발화 안 함 (is_first_poll = true 효과)
        mail::check_new_mails(&full_cfg, &empty, true).map(|_| ())
    })
    .await
    .map_err(|e| mail::MailError::Network(format!("작업 실행 실패: {}", e)))?;

    result
}

/// 비밀번호가 빈 문자열이면 디스크의 DPAPI 비밀번호로 채운다.
/// 빈 문자열이 아니거나 저장된 비밀번호가 없으면 그대로 둔다.
/// DPAPI 복호화 실패 시에만 Err 반환 (사용자에게 표시할 오류).
fn decrypt_password_if_needed(cfg: &mut mail::MailConfig) -> Result<(), String> {
    if !cfg.password.is_empty() {
        return Ok(());
    }
    let stored = match mail::load_stored() {
        Some(s) => s,
        None => return Ok(()),
    };
    if stored.password_dpapi.is_empty() {
        return Ok(());
    }
    let plain = mail::dpapi_unprotect(&stored.password_dpapi)?;
    cfg.password = String::from_utf8_lossy(&plain).to_string();
    Ok(())
}

/// 폴링 루프. enabled && !paused_due_to_auth일 때만 실제 POP3 호출
async fn mail_polling_loop(
    app: AppHandle,
    runtime: Arc<tokio::sync::Mutex<MailRuntimeState>>,
    trigger: Arc<tokio::sync::Notify>,
    shutdown: Arc<tokio::sync::Notify>,
) {
    // 시작 시 디스크에서 저장된 설정/UIDL 로드 (앱 재시작 시 baseline 유지)
    {
        let mut rt = runtime.lock().await;
        if let Some(stored) = mail::load_stored() {
            // 신규 형식 baseline을 메모리 HashMap으로 적재
            rt.last_seen = stored
                .last_seen
                .iter()
                .map(|e| (e.uidl.clone(), e.seen_at))
                .collect();
            // 비밀번호가 저장되어 있고 enabled가 켜져 있으면 자동 폴링 시작
            if stored.config.enabled && !stored.password_dpapi.is_empty() {
                rt.cfg = Some(mail::MailConfig {
                    enabled: stored.config.enabled,
                    account_name: stored.config.account_name,
                    host: stored.config.host,
                    port: stored.config.port,
                    use_tls: stored.config.use_tls,
                    user_id: stored.config.user_id,
                    password: String::new(), // 폴링 시 DPAPI에서 복호화
                    poll_minutes: stored.config.poll_minutes,
                });
                // 첫 폴링 baseline은 디스크 값 그대로 사용 → first_poll_pending = false
                rt.first_poll_pending = false;
            }
        }
    }

    loop {
        // 폴링 시작 전 baseline에서 STALE_DAYS 이상 과거 항목 정리
        {
            let mut rt = runtime.lock().await;
            mail::prune_stale(&mut rt.last_seen);
        }

        // 현재 상태 스냅샷
        let snapshot = {
            let rt = runtime.lock().await;
            (
                rt.cfg.clone(),
                rt.last_seen.clone(),
                rt.paused_due_to_auth,
                rt.first_poll_pending,
                rt.config_version,
            )
        };
        let (cfg_opt, last_seen, paused, is_first_poll, version_at_start) = snapshot;

        let poll_minutes = cfg_opt.as_ref().map(|c| c.poll_minutes).unwrap_or(5);
        let should_poll = cfg_opt.as_ref().map(|c| c.enabled).unwrap_or(false) && !paused;

        if should_poll {
            if let Some(mut full_cfg) = cfg_opt {
                // 비밀번호가 빈 문자열이면 DPAPI에서 복호화. 실패 시 에러 기록 후 다음 주기로
                if let Err(e) = decrypt_password_if_needed(&mut full_cfg) {
                    let mut rt = runtime.lock().await;
                    rt.last_error = Some(mail::MailError::Network(format!(
                        "비밀번호 복호화 실패: {}",
                        e
                    )));
                    let _ = app.emit(
                        "mail-status",
                        serde_json::json!({"error": rt.last_error}),
                    );
                    drop(rt);
                    if wait_or_shutdown(
                        Duration::from_secs(poll_minutes as u64 * 60),
                        &trigger,
                        &shutdown,
                    )
                    .await
                    {
                        return;
                    }
                    continue;
                }

                let cfg_for_check = full_cfg.clone();
                let seen_for_check = last_seen.clone();
                let join_result = tokio::task::spawn_blocking(move || {
                    mail::check_new_mails(&cfg_for_check, &seen_for_check, is_first_poll)
                })
                .await;
                // cfg_for_check / full_cfg 모두 scope 종료 시점에 MailConfig::drop으로 password 자동 wipe

                let mut rt = runtime.lock().await;
                if rt.config_version != version_at_start {
                    // 설정이 바뀌었으면 결과 폐기 (다음 루프에서 새 설정으로 즉시 재시도)
                    drop(rt);
                    continue;
                }

                match join_result {
                    Ok(Ok(outcome)) => {
                        rt.last_error = None;
                        rt.first_poll_pending = false;
                        // baseline 변동 여부 판정 → 변동 시에만 디스크 저장 (불필요한 SSD 쓰기 회피)
                        let baseline_changed = rt.last_seen != outcome.next_baseline;
                        rt.last_seen = outcome.next_baseline.clone();
                        let meta = rt.cfg.as_ref().map(mail::meta_from_config).unwrap_or_default();
                        drop(rt);
                        if baseline_changed {
                            let _ = persist_mail(&meta, None, Some(&outcome.next_baseline));
                        }
                        let _ = app.emit("mail-status", serde_json::json!({"error": null}));
                        if !outcome.new_mails.is_empty() {
                            let _ = app.emit(
                                "mail-new",
                                serde_json::json!({"mails": outcome.new_mails}),
                            );
                        }
                    }
                    Ok(Err(mail::MailError::Auth)) => {
                        rt.last_error = Some(mail::MailError::Auth);
                        rt.paused_due_to_auth = true;
                        let _ = app.emit(
                            "mail-status",
                            serde_json::json!({"error": mail::MailError::Auth}),
                        );
                    }
                    Ok(Err(other)) => {
                        rt.last_error = Some(other.clone());
                        let _ = app.emit("mail-status", serde_json::json!({"error": other}));
                    }
                    Err(_) => {
                        // spawn_blocking join 오류 — 다음 루프에서 재시도
                    }
                }
            }
        }

        // 다음 폴링까지 대기 (트리거/종료 시그널 우선)
        let sleep_dur = Duration::from_secs(poll_minutes as u64 * 60);
        if wait_or_shutdown(sleep_dur, &trigger, &shutdown).await {
            return;
        }
    }
}

/// shutdown이 오면 true 반환. trigger/sleep으로 깨어나면 false 반환.
async fn wait_or_shutdown(
    dur: Duration,
    trigger: &tokio::sync::Notify,
    shutdown: &tokio::sync::Notify,
) -> bool {
    tokio::select! {
        _ = shutdown.notified() => true,
        _ = trigger.notified() => false,
        _ = tokio::time::sleep(dur) => false,
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
    // 트레이 시작 핸들러에서 atomic 공유 상태를 reset하기 위한 clone
    // (앱 재실행과 동일한 초기 상태로 되돌리기 위해 사용)
    let teleport_tray = Arc::clone(&shared_teleport_x);
    let needs_redraw_tray = Arc::clone(&shared_needs_redraw);
    let on_fullscreen_tray = Arc::clone(&shared_on_fullscreen);

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

    // 모니터링 활성 비트마스크 (default: 모두 활성, frontend가 init 시 invoke로 갱신)
    let shared_monitor_flags = Arc::new(AtomicU8::new(MONITOR_FLAGS_DEFAULT));
    let monitor_flags_t1 = Arc::clone(&shared_monitor_flags);

    // 메일 알림 폴링 런타임
    let mail_runtime = Arc::new(tokio::sync::Mutex::new(MailRuntimeState::default()));
    let mail_trigger = Arc::new(tokio::sync::Notify::new());
    let mail_shutdown = Arc::new(tokio::sync::Notify::new());
    let mail_runtime_setup = Arc::clone(&mail_runtime);
    let mail_trigger_setup = Arc::clone(&mail_trigger);
    let mail_shutdown_setup = Arc::clone(&mail_shutdown);

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
            is_running: Arc::clone(&is_running),
            tray_language: Arc::new(std::sync::Mutex::new("system".to_string())),
            monitor_flags: Arc::clone(&shared_monitor_flags),
            mail_runtime: Arc::clone(&mail_runtime),
            mail_trigger: Arc::clone(&mail_trigger),
        })
        .plugin(tauri_plugin_single_instance::init(|_app, _argv, _cwd| {
            // 이미 실행 중인 인스턴스가 있으면 새 프로세스는 자동 종료됨
        }))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        // move: is_running_tray/t1/t2, thread_hover_state 를 클로저 안으로 이동
        .setup(move |app| {
            // 메인 윈도우를 코드로 생성 (tauri.conf.json 대신 명시적 빌더 사용).
            // WebView2 데이터 디렉터리를 `%LocalAppData%\TaskMon`로 지정해 앱 설치 폴더와 동일 위치에 사용자 설정 저장.
            let _ = tauri::webview::WebviewWindowBuilder::new(
                app,
                "main",
                tauri::WebviewUrl::App("index.html".into()),
            )
            .title("TaskMon")
            .inner_size(200.0, 60.0)
            .transparent(true)
            .decorations(false)
            .shadow(false)
            .resizable(false)
            .always_on_top(true)
            .skip_taskbar(true)
            .visible(false)
            .data_directory(webview_data_directory())
            .build()?;

            // 트레이 메뉴 초기 빌드 (실행 중 상태, 시스템 언어)
            let menu = build_tray_menu(app.handle(), true, "system")?;

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
                                let lang = app.state::<AppState>().tray_language.lock().unwrap().clone();
                                if let Ok(new_menu) = build_tray_menu(app, false, &lang) {
                                    if let Some(tray) = app.tray_by_id("main-tray") {
                                        let _ = tray.set_menu(Some(new_menu));
                                    }
                                }
                            }
                            "start" => {
                                // "중지 → 시작" 동작은 앱 재실행과 동일한 초기 상태로 되돌리는 것이 목표.
                                // (오류로 캐릭터가 이상 상태에 빠졌을 때 트레이 토글만으로 복구되도록.)

                                // 1. atomic 공유 상태 reset — 두 스레드가 다음 루프에서 깨끗한 값을 보도록
                                teleport_tray.store(i64::MIN, Ordering::Relaxed);
                                needs_redraw_tray.store(false, Ordering::Relaxed);
                                on_fullscreen_tray.store(false, Ordering::Relaxed);

                                // 2. 실행 플래그 켜기 — 두 스레드는 prev_running false→true 전환을 감지해
                                // 자체적으로 thread-local 변수(위치/phase/캐시 등)를 초기값으로 reset한다
                                is_running_tray.store(true, Ordering::Relaxed);

                                // 3. 창 표시 및 최상위 레이어 재적용
                                // 4. webview reload — React 모든 useState/useRef를 초기 상태로 되돌림
                                //    (LocalStorage 사용자 설정은 유지되므로 펫·언어·알림 등 사용자 데이터는 손실 없음)
                                if let Some(w) = app.get_webview_window("main") {
                                    let _ = w.show();
                                    let _ = w.set_always_on_top(true);
                                    let _ = w.eval("window.location.reload()");
                                }

                                // 5. 트레이 메뉴 재빌드 (중지 메뉴만 표시)
                                let lang = app.state::<AppState>().tray_language.lock().unwrap().clone();
                                if let Ok(new_menu) = build_tray_menu(app, true, &lang) {
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
            let monitor_flags_reader = Arc::clone(&monitor_flags_t1);
            std::thread::spawn(move || {
                let mut sys = sysinfo::System::new();
                sys.refresh_memory();

                // 네트워크 모니터링 초기화 (Networks 객체는 재시작 시 재생성하지 않음)
                let mut networks = sysinfo::Networks::new_with_refreshed_list();

                // 배터리 매니저 초기화 (재시작 시 재생성하지 않음)
                let battery_manager = starship_battery::Manager::new().ok();

                // GPU 모니터링 초기화 (PDH 카운터 — Windows 작업 관리자 동일 방식)
                // 실패 시 None으로 두고 이후 폴링에서 GPU 측정만 건너뜀
                let gpu_mon = gpu_monitor::GpuMonitor::new();

                // reset 가능한 thread-local 상태 — 첫 진입과 중지→시작이 동일한 초기값을 갖도록 단일 fresh()로 묶음.
                let mut state = PollingThreadState::fresh();

                // 중지→시작 전환 감지용 — 트레이 시작 핸들러는 atomic만 토글하고
                // thread-local 캐시 reset은 본 스레드가 자체적으로 수행한다.
                let mut prev_running = true;

                loop {
                    // 중지 상태: 500ms 대기 후 다음 루프 (CPU/메모리 점유 없음)
                    let running = is_running_t1.load(Ordering::Relaxed);
                    if !running {
                        prev_running = false;
                        std::thread::sleep(std::time::Duration::from_millis(500));
                        continue;
                    }

                    // 중지→시작 전환 감지 시 thread-local 캐시를 첫 진입과 동일하게 reset.
                    // (앱 재실행과 동일한 초기 상태로 되돌리기 위함 — 배터리 stale 등 회피)
                    if !prev_running {
                        state = PollingThreadState::fresh();
                        if let Some(ref mon) = gpu_mon {
                            // PDH 차분 baseline throwaway — 중지 기간 누적값을 버림
                            let _ = mon.poll();
                        }
                    }
                    prev_running = true;

                    // 사용자 모니터링 토글 비트마스크 — 매 루프 시작 시 1회 load
                    let flags = monitor_flags_reader.load(Ordering::Relaxed);
                    let gpu_enabled = flags & MONITOR_FLAG_GPU != 0;
                    let network_enabled = flags & MONITOR_FLAG_NETWORK != 0;
                    let battery_enabled = flags & MONITOR_FLAG_BATTERY != 0;

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

                    // 2. 모니터 정보 (10초 절대 간격)
                    // Win32 EnumDisplayMonitors + GetDpiForMonitor로 직접 수집 — Tauri API 미경유
                    if state.monitor_refresh_elapsed_ms >= 10_000 {
                        let new_monitors = enumerate_all_monitors();
                        if let Ok(mut cache) = monitors_clone.write() {
                            *cache = Arc::new(new_monitors);
                        }
                        state.monitor_refresh_elapsed_ms = 0;
                    }

                    // monitors 락을 짧게 잡아 Arc 참조만 복사 → 락 해제 후 EnumWindows 실행
                    let monitors_snapshot = monitors_clone.read()
                        .map(|g| Arc::clone(&g))
                        .unwrap_or_else(|_| Arc::new(Vec::new()));

                    // 모니터 수 변경 감지: 모니터 분리/연결 시 윈도우 복구
                    let cur_monitor_count = monitors_snapshot.len();
                    if state.prev_monitor_count != 0 && cur_monitor_count != state.prev_monitor_count {
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
                    state.prev_monitor_count = cur_monitor_count;

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
                    if pet_on_fullscreen && !state.prev_on_fullscreen {
                        let _ = window_clone_evt.set_always_on_top(false);
                    } else if !pet_on_fullscreen && state.prev_on_fullscreen {
                        let _ = window_clone_evt.set_always_on_top(true);
                    }
                    state.prev_on_fullscreen = pet_on_fullscreen;

                    // CPU/메모리/GPU emit은 정수 단위 변화가 있을 때만 전송한다.
                    // idle 시스템에서 동일 값이 반복 emit되면 React setState → evaluateMessages가
                    // 매초 재실행되므로 dedup으로 불필요한 렌더 사이클을 차단한다.
                    let cpu_int = usage.round() as i32;
                    if cpu_int != state.prev_cpu_pct {
                        let _ = window_clone_evt.emit("cpu-usage", usage);
                        state.prev_cpu_pct = cpu_int;
                    }
                    let mem_int = mem_pct as i32;
                    if mem_int != state.prev_mem_pct {
                        let _ = window_clone_evt.emit("memory-usage", mem_pct);
                        state.prev_mem_pct = mem_int;
                    }

                    // GPU 사용률 폴링 — 사용자가 비활성화한 경우 PDH 호출 자체 건너뜀
                    if gpu_enabled {
                        if let Some(ref mon) = gpu_mon {
                            if let Some(gpu_pct) = mon.poll() {
                                let gpu_int = gpu_pct.round() as i32;
                                if gpu_int != state.prev_gpu_pct {
                                    let _ = window_clone_evt.emit("gpu-usage", gpu_int as u32);
                                    state.prev_gpu_pct = gpu_int;
                                }
                            }
                        }
                    }

                    let interval = polling_ms_t1.load(Ordering::Relaxed);

                    // 네트워크: 사용자가 비활성화한 경우 NIC 통계 조회 자체 건너뜀.
                    // 인터페이스 목록 재구성은 30초 절대 간격, 그 외에는 통계만 갱신.
                    if network_enabled {
                        let interface_refresh = state.network_refresh_elapsed_ms >= 30_000;
                        networks.refresh(interface_refresh);
                        if interface_refresh {
                            state.network_refresh_elapsed_ms = 0;
                        }
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
                    }

                    // 배터리: 사용자가 비활성화한 경우 starship_battery 호출 자체 건너뜀.
                    // 측정 주기는 최소 3분 간격, 폴링 간격이 3분 이상이면 폴링 간격으로 체크.
                    if battery_enabled {
                        let battery_check_ms = std::cmp::max(180_000u64, interval);
                        state.battery_elapsed_ms += interval;
                        let mut battery_just_checked = false;
                        if state.battery_elapsed_ms >= battery_check_ms {
                            state.battery_elapsed_ms = 0;
                            battery_just_checked = true;
                            if let Some(ref manager) = battery_manager {
                                if let Ok(mut batteries) = manager.batteries() {
                                    if let Some(Ok(bat)) = batteries.next() {
                                        state.cached_battery_percent = (bat.state_of_charge().get::<starship_battery::units::ratio::percent>()) as i32;
                                    }
                                }
                            }
                        }

                        // 충전 상태: 매 폴링마다 확인 (GetSystemPowerStatus는 매우 가벼움)
                        // 변경 시 또는 배터리 잔량 갱신 시에만 이벤트 발송
                        if state.cached_battery_percent >= 0 {
                            let charging = is_ac_connected();
                            if charging != state.prev_charging || battery_just_checked {
                                state.prev_charging = charging;
                                let _ = window_clone_evt.emit("battery-usage", serde_json::json!({
                                    "percent": state.cached_battery_percent,
                                    "charging": charging
                                }));
                            }
                        }
                    }

                    // 시간 기반 tick 누적 — 모니터 enumerate / 네트워크 인터페이스 재구성 주기 보장.
                    // (네트워크 비활성 상태에서도 누적은 유지하여 재활성 시 즉시 인터페이스 갱신되도록 한다.)
                    state.monitor_refresh_elapsed_ms = state.monitor_refresh_elapsed_ms.saturating_add(interval);
                    state.network_refresh_elapsed_ms = state.network_refresh_elapsed_ms.saturating_add(interval);

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
                let mut dir_resync_after_change: u8 = 0; // 방향 변경 직후 재발행 잔여 프레임 (IPC 누락 방어)
                let mut prev_on_fs_t2: bool = false; // Thread 2 내 전체화면 상태 변경 감지
                let mut prev_target_x: i32 = i32::MIN; // SetWindowPos 위치 변경 감지용
                let mut prev_target_y: i32 = i32::MIN;
                let mut prev_cursor_on_pet: bool = false; // 커서-펫 충돌 상태 (클릭 투과 토글용)
                let mut prev_mouse_enabled: bool = true; // 마우스 사용 상태 변경 감지용
                let mut cross_pending: bool = false; // 건너가기 보류 (화면 밖 이탈 후 전환)
                let mut cross_exit_y: f64 = 0.0; // 화면 밖 판정 Y 좌표
                let mut cross_neighbor: MonitorInfo = MonitorInfo::default(); // 건너가기 대상 모니터
                let mut sorted_indices: Vec<usize> = Vec::with_capacity(8); // 등반 모드용 정렬 인덱스 (재사용)

                // 중지→시작 전환 감지용 — 트레이 시작 핸들러는 atomic만 토글하고
                // thread-local 변수 reset은 본 스레드가 자체적으로 수행한다.
                let mut prev_running_t2 = true;

                loop {
                    // 중지 상태: 200ms 대기 (16ms busy-loop 방지 → CPU 점유 12배 감소)
                    let running = is_running_t2.load(Ordering::Relaxed);
                    if !running {
                        prev_running_t2 = false;
                        std::thread::sleep(std::time::Duration::from_millis(200));
                        continue;
                    }

                    // 중지→시작 전환 감지 시 thread-local 변수를 첫 진입과 동일하게 reset.
                    // (오류로 펫이 이상한 위치/모드에 빠진 경우 트레이 시작만으로 복구되도록.)
                    if !prev_running_t2 {
                        move_phase = 0;
                        prev_move_phase = 0;
                        current_y = 0.0;
                        climb_edge_x = 0.0;
                        climb_top_y = 0.0;
                        climb_bottom_y = 0.0;
                        consecutive_climbs = 0;
                        smooth_y = 0.0;
                        smooth_y_init = false;
                        prev_target_x = i32::MIN;
                        prev_target_y = i32::MIN;
                        prev_cursor_on_pet = false;
                        prev_mouse_enabled = true;
                        cross_pending = false;
                        cross_exit_y = 0.0;
                        cross_neighbor = MonitorInfo::default();
                        sorted_indices.clear();
                        dir_resync_after_change = 0;
                        dir_sync_tick = 0;
                        zorder_tick = 0;
                        prev_on_fs_t2 = false;
                        prev_scale = 1.0;
                        random_dir_left = rng.gen_bool(0.5);
                        prev_random_dir_left = !random_dir_left;
                        last_update = std::time::Instant::now();
                        // current_x를 첫 진입 시점과 동일하게 가장 왼쪽 모니터의 시작점으로 reset
                        if let Ok(monitors_guard) = all_monitors_reader.read() {
                            if let Some(min_m) = monitors_guard.iter().min_by_key(|m| m.x) {
                                current_x = min_m.x as f64 - 100.0;
                            }
                        }
                    }
                    prev_running_t2 = true;

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
                    // 시스템 부하 시 큰 점프 방지: delta_time 상한 25ms (1.5프레임 분량).
                    // 정상 60FPS(16ms)에는 영향 없음. 프레임 누락 시 한 번에 점프하는 양 절반 이하로 감소.
                    let delta_time = now.duration_since(last_update).as_secs_f64().min(0.025);
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

                                        if cross_pending {
                                            // 건너가기 보류: 화면 밖으로 완전히 나갈 때까지 계속 등반
                                            if current_y <= cross_exit_y {
                                                // 이웃 모니터 상단으로 전환 (Phase 2)
                                                prev_scale = cross_neighbor.scale_factor;
                                                random_dir_left = !is_left;
                                                let np_w = pet_visual_w_reader.load(Ordering::Relaxed) as f64 * prev_scale;
                                                let nm = (LOGICAL_WIN_W * prev_scale - np_w) / 2.0;
                                                let nh = (height_offset as f64 * prev_scale).round() as i32;
                                                if is_left {
                                                    current_x = (cross_neighbor.x + cross_neighbor.width) as f64 - nm - np_w;
                                                } else {
                                                    current_x = cross_neighbor.x as f64 - nm;
                                                }
                                                current_y = cross_neighbor.y as f64 + nh as f64;
                                                move_phase = 2;
                                                cross_pending = false;
                                            }
                                        } else if current_y <= climb_top_y {
                                            // 상단 도달: 건너가기 결정
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
                                                        // 건너가기 보류: 화면 밖으로 나간 후 전환
                                                        let neighbor = if is_left { &monitors[sorted_indices[idx - 1]] } else { &monitors[sorted_indices[idx + 1]] };
                                                        cross_pending = true;
                                                        cross_neighbor = neighbor.clone();
                                                        cross_exit_y = climb_top_y - actual_win_h as f64;
                                                        crossed_at_top = true;
                                                        // current_y 클램프 안 함 → 계속 등반하여 화면 밖으로
                                                    }
                                                }
                                            }
                                            if !crossed_at_top {
                                                current_y = climb_top_y;
                                                // 등반 벽 가장자리에서 Phase 2 시작 (펫 위치 보정)
                                                if is_left {
                                                    current_x = climb_edge_x - margin;
                                                } else {
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

                                        if cross_pending {
                                            // 건너가기 보류: 화면 밖으로 완전히 나갈 때까지 계속 하강
                                            if current_y >= cross_exit_y {
                                                // 이웃 모니터 하단으로 전환 (Phase 0)
                                                prev_scale = cross_neighbor.scale_factor;
                                                random_dir_left = !is_left;
                                                let np_w = pet_visual_w_reader.load(Ordering::Relaxed) as f64 * prev_scale;
                                                let nm = (LOGICAL_WIN_W * prev_scale - np_w) / 2.0;
                                                let nh = (height_offset as f64 * prev_scale).round() as i32;
                                                let nwh = (LOGICAL_WIN_H * prev_scale) as i32;
                                                if is_left {
                                                    current_x = cross_neighbor.x as f64 - nm;
                                                } else {
                                                    current_x = (cross_neighbor.x + cross_neighbor.width) as f64 - nm - np_w;
                                                }
                                                smooth_y = (cross_neighbor.work_bottom - nwh - nh) as f64;
                                                smooth_y_init = true;
                                                move_phase = 0;
                                                cross_pending = false;
                                            }
                                        } else if current_y >= climb_bottom_y {
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
                                                        // 건너가기 보류: 화면 밖으로 나간 후 전환
                                                        let neighbor = if is_left { &monitors[sorted_indices[idx + 1]] } else { &monitors[sorted_indices[idx - 1]] };
                                                        cross_pending = true;
                                                        cross_neighbor = neighbor.clone();
                                                        cross_exit_y = climb_bottom_y + actual_win_h as f64;
                                                        crossed_at_bottom = true;
                                                        // current_y 클램프 안 함 → 계속 하강하여 화면 밖으로
                                                    }
                                                }
                                                if !crossed_at_bottom {
                                                    current_y = climb_bottom_y;
                                                    // 하강 벽 가장자리에서 Phase 0 시작 (펫 위치 보정)
                                                    if is_left {
                                                        current_x = climb_edge_x + actual_win_w as f64 - margin - pet_w;
                                                    } else {
                                                        current_x = climb_edge_x - margin;
                                                    }
                                                    move_phase = 0;
                                                    smooth_y = climb_bottom_y - (actual_win_h as f64 - pet_w) / 2.0;
                                                    smooth_y_init = true;
                                                    // 방향 유지 — 하강 벽 반대쪽(모니터 내부)으로 이동
                                                    random_dir_left = is_left;
                                                }
                                            } else {
                                                current_y = climb_bottom_y;
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
                                }
                            } else {
                                // ========================================
                                // 기본 이동 모드 (방향에 따라 좌/우 이동)
                                // ========================================
                                if is_left { current_x -= movement; } else { current_x += movement; }

                                // 펫 스프라이트 실제 폭 (CSS 픽셀 → 물리 픽셀)
                                // bounce 반전을 펫의 가시 경계 기준으로 대칭화하기 위해 사용
                                let pet_w = pet_visual_w_reader.load(Ordering::Relaxed) as f64 * prev_scale;
                                let pet_margin = ((actual_win_w as f64 - pet_w) / 2.0).max(0.0);

                                // 범위 이탈 보정
                                if is_bounce {
                                    // 반복 모드: 펫의 가시 끝이 모니터 끝에 닿으면 즉시 방향 전환
                                    // (저속 CPU에서도 화면 밖 드리프트 없이 곧바로 좌우 반전이 보임)
                                    let pet_left = current_x + pet_margin;
                                    let pet_right = pet_left + pet_w;
                                    if is_left && pet_left <= min_x as f64 {
                                        current_x = min_x as f64 - pet_margin;
                                        random_dir_left = false;
                                    } else if !is_left && pet_right >= max_x as f64 {
                                        current_x = max_x as f64 - pet_margin - pet_w;
                                        random_dir_left = true;
                                    }
                                } else if is_left {
                                    // 왼쪽 이동: 좌측 끝 벗어나면 우측에서 재등장 (윈도우 완전 이탈 후)
                                    if current_x + actual_win_w as f64 <= min_x as f64
                                        || current_x > (max_x as f64 + actual_win_w as f64) {
                                        current_x = max_x as f64;
                                        smooth_y_init = false;
                                        needs_show = true;
                                    }
                                } else {
                                    // 오른쪽 이동: 우측 끝 벗어나면 좌측에서 재등장 (윈도우 완전 이탈 후)
                                    // 핫플러그 등으로 좌측 한참 밖으로 빠진 경우(< min_x - actual_win_w)도 동일하게 복구
                                    if current_x >= max_x as f64
                                        || current_x < (min_x as f64 - actual_win_w as f64) {
                                        current_x = min_x as f64 - actual_win_w as f64;
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
                            // + 변경 직후 6프레임(~100ms) 재발행 (저속 CPU에서도 React가 확실히 반영하도록 방어)
                            if is_random || is_bounce {
                                let dir_changed = random_dir_left != prev_random_dir_left;
                                if dir_changed {
                                    dir_resync_after_change = 6;
                                }
                                let need_sync = dir_sync_tick < 180 && dir_sync_tick % 30 == 0; // 첫 3초간 0.5초마다
                                let post_change_resync = dir_resync_after_change > 0;
                                if dir_changed || need_sync || post_change_resync {
                                    let _ = window_clone.emit("move-direction", random_dir_left);
                                    prev_random_dir_left = random_dir_left;
                                }
                                if dir_resync_after_change > 0 { dir_resync_after_change -= 1; }
                                dir_sync_tick += 1;
                            } else {
                                // 비동적 방향 모드에서는 prev를 반전시켜서 동적 모드로 전환 시 즉시 발행되도록 준비
                                prev_random_dir_left = !random_dir_left;
                                dir_sync_tick = 0; // 동적 모드 재진입 시 동기화 재시작
                                dir_resync_after_change = 0;
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
                                        let mouse_enabled_now = mouse_enabled_t2.load(Ordering::Relaxed);
                                        let cursor_on_pet = if !mouse_enabled_now {
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
                                        // 마우스 사용 상태 변경 시 강제 갱신 (prev_cursor_on_pet이 이미 false여도 클릭 투과 적용)
                                        let mouse_changed = mouse_enabled_now != prev_mouse_enabled;
                                        if cursor_on_pet != prev_cursor_on_pet || mouse_changed {
                                            let _ = window_clone.set_ignore_cursor_events(!cursor_on_pet);
                                            prev_cursor_on_pet = cursor_on_pet;
                                        }
                                        prev_mouse_enabled = mouse_enabled_now;
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

            // 메일 알림 폴링 태스크 시작 (Tauri 자체 tokio 런타임에서 실행)
            let mail_app_handle = app.handle().clone();
            let mail_runtime_clone = Arc::clone(&mail_runtime_setup);
            let mail_trigger_clone = Arc::clone(&mail_trigger_setup);
            let mail_shutdown_clone = Arc::clone(&mail_shutdown_setup);
            tauri::async_runtime::spawn(async move {
                mail_polling_loop(
                    mail_app_handle,
                    mail_runtime_clone,
                    mail_trigger_clone,
                    mail_shutdown_clone,
                )
                .await;
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
            update_timer_font_size,
            check_update,
            download_and_install_update,
            mail_load_config,
            mail_apply_config,
            mail_test_connection
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(move |_app_handle, event| {
            // 앱 종료 시 메일 폴링 태스크에 종료 시그널 전송
            if let tauri::RunEvent::Exit = event {
                mail_shutdown.notify_waiters();
            }
        });
}
