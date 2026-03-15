# 작업 계획: 등반 이동 모드 추가

## 목표
"기본 이동" / "등반 이동" 선택 가능한 이동 모드 시스템을 추가한다.
추후 이동 모드 확장을 고려하여 설계한다.

## 작업 범위

### Phase 1: Rust 백엔드
- [x] `AppState`에 `move_mode: Arc<AtomicU8>` 추가 (0=기본, 1=등반)
- [x] `update_move_mode` 커맨드 추가
- [x] Thread 2에 4-phase 상태 머신 구현 (Bottom→ClimbRight→Top→DescendLeft)
- [x] `move-phase` 이벤트 발행 (phase 변경 시에만)
- [x] `cached_win_w` 추가 (등반 시 우측 가장자리 위치 계산용)

### Phase 2: 프론트엔드
- [x] `types.ts`: `MOVE_MODES` 상수 배열 추가
- [x] `locales.ts`: 번역 키 추가
- [x] `SettingsWindow.tsx`: "펫 이동" 탭 + 라디오버튼 UI
- [x] `MainWindow.tsx`: `move-phase` 리스너 + CSS transform 적용
- [x] `App.css`: 이동 설정 라디오 UI 스타일

### Phase 3: 검증 및 문서
- [x] `cargo check` 통과
- [x] `notes.md`, `README.md` 갱신

## 상세 설계

### 이동 모드 (확장 가능)
| mode | 이름 | 설명 |
|------|------|------|
| 0 | 기본 이동 | 작업표시줄 위로만 이동 (현재 동작) |
| 1 | 등반 이동 | 모니터 테두리를 타고 순회 |
| 2+ | 추후 추가 | 새 모드 추가 시 match arm + 라디오버튼만 추가 |

### 등반 이동 상태 머신
```
Phase 0 (Bottom →): 하단 좌→우 이동
  경계 도달 → 다음 모니터 있으면 50% 확률로 건너가기/등반, 없으면 등반
Phase 1 (ClimbRight ↑): 우측 테두리 아래→위 등반
  상단 도달 → Top
Phase 2 (Top ←): 상단 우→좌 이동 (거꾸로)
  이전 모니터 있으면 이전 모니터 상단 계속, 좌측 끝 → DescendLeft
Phase 3 (DescendLeft ↓): 좌측 테두리 위→아래 하강
  하단 도달 → Bottom
```

### CSS 회전
| Phase | transform |
|-------|-----------|
| 0 Bottom | 없음 |
| 1 ClimbRight | `rotate(-90deg)` |
| 2 Top | `rotate(180deg)` |
| 3 DescendLeft | `rotate(90deg)` |

## 검증 방법
- `cargo check` 성공
- 기본 모드: 기존 동작 변화 없음
- 등반 모드: 4-phase 순환 정상 동작

## 승인 필요 사항
- 없음 (기존 기능에 영향 없는 추가 기능)
