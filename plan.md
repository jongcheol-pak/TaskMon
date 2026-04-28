# 작업 계획: 중지/시작을 앱 재실행과 동일한 동작으로 만들기

## 배경
사용자 보고:
- 오류로 캐릭터가 이상하게 동작하는 경우 트레이 메뉴 "중지 → 시작"으로는 복구되지 않음
- 앱을 종료한 후 재실행해야 정상 동작
- 따라서 "중지 → 시작"이 앱 재실행과 동일한 수준의 reset이 되어야 함

## 원인 분석
현재 "중지 → 시작" 동작은 단순 `is_running` 플래그 토글이라 다음이 모두 잔존함:
1. **React state**: `useState`/`useRef`로 보유한 위치/animation/sprite/충돌 등 모든 상태 (가장 흔한 corruption 원인)
2. **Rust Thread 1 thread-local 변수**: 배터리 캐시, 모니터 enumerate elapsed_ms, 네트워크 elapsed_ms, GPU PDH baseline, 충전·모니터수 prev 값
3. **Rust Thread 2 thread-local 변수**: 위치(`current_x`), 보간(`smooth_y`), 등반 phase, 이전 위치 비교용 prev_target, 커서 충돌, dir 동기화 tick 등 17개 이상
4. **Atomic 공유 변수**: `shared_teleport_x`, `shared_needs_redraw`, `shared_on_fullscreen`, `shared_pet_x` 등이 stale 상태로 남아 있을 수 있음

앱 재실행 시는 전부 재생성되어 정상화됨.

## 해결책 — 3단 reset

### 1. 메인 윈도우 webview reload
- start 핸들러에서 `webview.eval("window.location.reload()")` 실행
- React 모든 useState/useRef 초기 상태로 (LocalStorage 사용자 설정은 유지)
- React state corruption 시나리오의 90%+ 해결

### 2. Atomic 공유 변수 reset
- `AppState`에 `shared_teleport_x` / `shared_needs_redraw` / `shared_on_fullscreen` / `shared_pet_x` Arc 추가
- start 핸들러에서 모두 초기값으로 store

### 3. Thread 1·Thread 2의 thread-local 변수 reset
- 각 스레드 안에 `prev_running` 변수 추가하여 중지 → 시작 전환을 자체 감지
- 전환 감지 시 thread-local 변수를 모두 첫 진입 시점과 동일하게 reset

## 작업 범위

### Phase 1: AppState 확장 + start 핸들러 강화
- [x] `src-tauri/src/lib.rs` `AppState`에 4개 Arc 필드 추가 (`teleport_x`, `needs_redraw`, `on_fullscreen`, `pet_x`)
- [x] `manage()` 호출에 등록
- [x] start 핸들러:
  - is_running.store(true)
  - 4개 atomic 모두 초기값으로 reset (`teleport_x = i64::MIN`, `needs_redraw = false`, `on_fullscreen = false`, `pet_x = 첫 모니터 시작 위치 또는 그대로`)
  - webview show + always_on_top
  - **`webview.eval("window.location.reload()")`** ← 핵심
  - 트레이 메뉴 재빌드

### Phase 2: Thread 1 자체 reset
- [x] `prev_running: bool` 추가
- [x] 매 루프 시작 시 `is_running` load → false면 `prev_running = false; sleep; continue;`
- [x] true이고 `!prev_running`이면 thread-local 변수 reset:
  - `battery_elapsed_ms = 178_000`
  - `cached_battery_percent = -1`
  - `prev_charging = false`
  - `monitor_refresh_elapsed_ms = 10_000`
  - `network_refresh_elapsed_ms = 30_000`
  - `prev_monitor_count = 0`
  - `prev_on_fullscreen = false`
  - GPU PDH baseline throwaway: `if let Some(ref mon) = gpu_mon { let _ = mon.poll(); }`

### Phase 3: Thread 2 자체 reset
- [x] `prev_running_t2: bool` 추가
- [x] 동일하게 false→true 전환 감지 시 thread-local 변수 reset:
  - `move_phase = 0`, `prev_move_phase = 0`
  - `current_y = 0.0`, `climb_edge_x = 0.0`, `climb_top_y = 0.0`, `climb_bottom_y = 0.0`
  - `consecutive_climbs = 0`
  - `smooth_y_init = false`, `smooth_y = 0.0`
  - `prev_target_x = i32::MIN`, `prev_target_y = i32::MIN`
  - `prev_cursor_on_pet = false`
  - `prev_mouse_enabled = true`
  - `cross_pending = false`
  - `dir_resync_after_change = 0`, `dir_sync_tick = 0`, `zorder_tick = 0`
  - `prev_on_fs_t2 = false`
  - `last_update = std::time::Instant::now()` (delta_time 안전 시작)
  - `current_x` 시작 위치 재계산 (앱 init 시 로직과 동일)
  - `phaseCycleRef` 등 React 측 변수는 webview reload로 자동 reset

### Phase 4: 검증·문서
- [x] `cargo build` 통과
- [x] `tsc --noEmit` / `vite build` 통과 (frontend 변경 거의 없음)
- [x] `notes.md` 갱신
- [x] `Releases.md` v0.1.0 `### 버그 수정`에 "트레이 중지 → 시작이 앱 재실행과 동일하게 동작" 추가 (사용자 체감)

## 검증 방법
- `cargo build` 통과 + 경고 없음
- `tsc --noEmit` / `vite build` 통과
- 사용자 측 실측: 펫이 이상한 위치/모드에 빠진 상태에서 중지 → 시작 시 첫 모니터 시작 위치에서 정상 모드로 이동 재개

## 승인 필요 사항
- 사용자 사전 요청에 의한 진행이므로 추가 승인 불필요
- 본 작업은 트레이 시작 시 `window.location.reload()`를 강제 실행 — 사용자에게 "초기화" 동작이라는 의미가 명확하므로 OK

## 미진행 (별도 케이스로 분리 가능)
- 설정 윈도우는 사용자가 명시적으로 열고 닫는 윈도우라 트레이 시작 시점의 reset 대상 아님
- LocalStorage 자체 reset은 사용자 설정까지 날리므로 의도적으로 reset하지 않음
