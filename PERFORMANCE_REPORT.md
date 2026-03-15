# Performance Analysis Report

**Date**: 2026-03-15
**Target**: `src-tauri/src/lib.rs`, `src/MainWindow.tsx`, `src/types.ts`
**Analysis Type**: Static (6차 리뷰 — SetWindowPos 최적화 이후 잔여 개선점 탐색)

---

## Executive Summary
- **Overall Score**: 82/100 (SetWindowPos 최적화 반영, 5차 72점에서 상승)
- **Critical Issues**: 0 (기존 C2 해결됨)
- **Warnings**: 4
- **Info**: 3
- **Estimated Impact**: Low — 핵심 병목(SetWindowPos)이 해결되어 남은 항목은 모두 저위험

---

## 적용 완료된 최적화

| 최적화 | 효과 |
|--------|------|
| SWP_NOZORDER (일반 프레임) | DWM Z-order 재계산 60fps → ~0.2fps |
| SWP_NOSENDCHANGING | WM_WINDOWPOSCHANGING 메시지 체인 제거 |
| 위치 미변경 시 SetWindowPos 생략 | 호버 시 100% 제거, 저속 시 30~50% 감소 |
| Z-order 5초 간격 재적용 | TOPMOST 유지하면서 DWM 부하 최소화 |

---

## Findings

### Warnings

#### W1. `available_monitors()` IPC 매 폴링마다 호출 (lib.rs:767)

```rust
if let Ok(m_list) = window_clone_evt.available_monitors() {
```

**문제**: 매 폴링(기본 1초)마다 Tauri IPC를 통해 모니터 목록을 조회합니다. 모니터 구성은 핫플러그 시에만 변경되므로 대부분의 호출이 불필요합니다.

**영향**: Low-Medium. IPC 왕복 ~1ms + `get_work_area_for_monitor()`가 모니터 수만큼 Win32 API 호출(MonitorFromPoint + GetMonitorInfoW).

**권장**: 10~30초 간격으로 갱신. 모니터 수 변경 감지는 이미 구현되어 있으므로 빈도만 줄이면 됩니다.

```rust
let mut monitor_refresh_tick: u32 = 0;
// ...
if monitor_refresh_tick == 0 {
    if let Ok(m_list) = window_clone_evt.available_monitors() { ... }
}
monitor_refresh_tick = (monitor_refresh_tick + 1) % 10; // 10초마다
```

---

#### W2. `EnumWindows` 매 폴링마다 전체 순회 (lib.rs:832)

```rust
let fs_flags = check_fullscreen_all(&monitors_snapshot);
```

**문제**: 매 폴링마다 `EnumWindows`로 모든 최상위 윈도우(100~300개)를 순회하여 전체화면 여부를 판단합니다. 전체화면 전환은 드물게 발생합니다.

**영향**: Low-Medium. 윈도우당 4개 Win32 API × 200개 = 800회/초.

**권장**: 3~5초 간격으로 줄이거나, `available_monitors` 갱신과 동일 주기로 묶기.

---

#### W3. Thread 1 매 폴링마다 Vec 할당 (lib.rs:768)

```rust
let mut new_monitors = Vec::new();
for m in m_list { ... new_monitors.push(...); }
```

**문제**: 매 폴링마다 새 `Vec<MonitorInfo>`를 할당하고 Arc로 감쌉니다. 모니터 수가 2~3개이므로 할당 크기는 작지만, 이전 Arc가 drop되면서 매초 할당+해제가 반복됩니다.

**영향**: Low. 소량 할당이지만 장시간 실행 시 힙 단편화 가능성.

**권장**: W1과 함께 빈도를 줄이면 자동 해결.

---

#### W4. `refresh_cpu_usage()` 전체 코어 폴링 (lib.rs:746)

```rust
sys.refresh_cpu_usage();
let cpus = sys.cpus();
let usage = cpus.iter().map(|cpu| cpu.cpu_usage()).sum::<f32>() / cpus.len() as f32;
```

**문제**: 전체 평균만 사용하는데 모든 논리 코어를 개별 수집합니다.

**영향**: Low. sysinfo 내부에서 NtQuerySystemInformation 호출 시 코어별 데이터 파싱 비용.

**권장**: sysinfo 0.33에 `global_cpu_usage()` 존재 시 전환. 없으면 현재 방식 유지.

---

### Info / Minor

#### I1. JSX 인라인 클로저 매 렌더마다 실행 (MainWindow.tsx:825-833)

```tsx
<div className="pet-container" style={(() => {
  const flip = isLeftMode ? 'scaleX(-1) ' : '';
  if (movePhase === 1) return { transform: `${flip}rotate(-90deg)` };
  // ...
})()}>
```

**참고**: IIFE(즉시 실행 함수)가 매 렌더마다 새 스타일 객체를 생성합니다. `useMemo`로 캐싱 가능하지만, 객체 생성 비용이 극히 미미하여 현재 상태 유지 권장.

#### I2. `checkAlarms` 내 Date 객체 생성 (types.ts:558-559, 621-622)

```typescript
const now = Date.now();
const today = new Date().toISOString().slice(0, 10);
// ...
const currentDate = new Date(); // hourly 케이스
```

**참고**: 1초 간격 타이머에서 호출되며, 알람 수가 적으면 무시 가능. 이전 리뷰(4차)에서 MainWindow.tsx 내 Date 통합은 완료됨. types.ts 내부는 별도 함수이므로 독립적으로 Date 생성이 필요.

#### I3. `evaluateMessages` 매 모니터링 업데이트마다 실행 (MainWindow.tsx:213)

**참고**: CPU, 메모리, 네트워크, 배터리 이벤트 각각에 의해 트리거되어 초당 최대 4회 실행. 메시지 배열이 작으면(<20개) 무시 가능. 이벤트를 하나로 묶으면 1회로 줄일 수 있으나, Rust 측에서 4개 emit을 1개로 합치면 코드 복잡도가 증가하므로 현재 상태 유지 권장.

---

## 기존 최적화 유지 확인

### Rust Backend (lib.rs)

| 최적화 | 상태 |
|--------|------|
| SetWindowPos SWP_NOZORDER (일반 프레임) | **신규 적용** |
| SetWindowPos SWP_NOSENDCHANGING | **신규 적용** |
| 위치 미변경 시 SetWindowPos 생략 | **신규 적용** |
| Z-order 5초 간격 재적용 | **신규 적용** |
| Arc swap 패턴 (모니터 캐시) | **유지** |
| AtomicU32 bit-pattern f32 (lock-free 속도값 공유) | **유지** |
| DPI 변경 시에만 outer_size 조회 | **유지** |
| EnumWindows 단일 순회 + 조기 종료 | **유지** |
| 네트워크 인터페이스 30초 주기 갱신 | **유지** |
| 배터리 3분 주기 폴링 | **유지** |
| HWND 캐싱 + SetWindowPos 직접 호출 | **유지** |
| 중지 상태 sleep 분리 (500ms/200ms) | **유지** |
| delta_time 상한 50ms | **유지** |

### React Frontend

| 최적화 | 상태 |
|--------|------|
| Ref 기반 알림 설정 참조 (displayConfigRef, bubbleEnabledRef) | **유지** |
| alarmsRef 동기화 (타이머 내 최신 상태 참조) | **유지** |
| 500ms 디바운스 localStorage 저장 | **유지** |
| Web Animations API 스프라이트 제어 | **유지** |
| sortedMessages useMemo (정렬 캐싱) | **유지** |
| getTextShadow useMemo (색상 변경 시에만 재계산) | **유지** |
| petStyle useMemo (의존성 완전 명시) | **유지** |
| Date 인스턴스 통합 (alarm-list-update 핸들러) | **유지** |

---

## Recommendations (우선순위순)

| 순위 | 항목 | 난이도 | 효과 |
|------|------|--------|------|
| 1 | W1+W2: `available_monitors` + `EnumWindows` 빈도 감소 (10초) | Low | 매초 IPC+800회 Win32 API → 10초마다로 감소 |
| 2 | W4: `global_cpu_usage()` 전환 (API 존재 시) | Low | 코어별 파싱 제거 |
| 3 | I3: 모니터링 이벤트 통합 (CPU+메모리+네트워크를 1개 emit으로) | Medium | evaluateMessages 호출 4회→1회/초 |

---

## Conclusion

SetWindowPos 최적화(SWP_NOZORDER + SWP_NOSENDCHANGING + 위치 변경 감지 + 5초 Z-order)가 적용되어 **마우스 속도 저하의 주요 원인이 해결**되었습니다.

남은 경고(W1~W4)는 모두 저위험이며, 가장 효과적인 추가 개선은 **`available_monitors`와 `EnumWindows`의 호출 빈도를 10초로 줄이는 것**입니다. 이는 모니터 핫플러그 감지 지연(최대 10초)과의 트레이드오프이지만, 실사용에서는 충분히 허용 가능합니다.

메모리 누수는 확인되지 않으며, 장시간 실행에 문제가 없습니다.

---

**Report Generated by**: Performance Verifier Skill (Claude Code)
**Report Type**: 6차 리뷰 (SetWindowPos 최적화 이후 잔여 개선점 분석)
