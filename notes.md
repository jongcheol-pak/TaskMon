# 프로젝트 진행 기록 (Notes)

## 최근 변경
- 2026-04-27: NSIS 설치 파일명을 `TaskMon_{버전}_x64-setup.exe`(Tauri 기본) → `TaskMon-Setup-v{버전}.exe`로 변경. (1) `scripts/rename-installer.cjs` 신규 — `tauri.conf.json`에서 버전 읽고 NSIS 출력 폴더의 `TaskMon_*-setup.exe` 패턴을 와일드카드 매칭하여 새 이름으로 rename(복수 후보 시 최신 mtime 선택, 동일 이름 결과물은 덮어쓰기), (2) `build.bat` 단계 [3/3] → [4/4]로 확장하여 빌드 직후 `node scripts/rename-installer.cjs` 자동 실행. 기존 NSIS 산출물로 스크립트 실행 검증 완료.
- 2026-04-27: WebView2 사용자 데이터 디렉터리를 `%LocalAppData%\com.jongc.taskmon`(bundle identifier 기본값) → `%LocalAppData%\TaskMon`(앱 설치 폴더와 동일)로 변경. (1) `tauri.conf.json`에서 메인 윈도우 정의 제거(`"windows": []`), (2) `lib.rs`에 `webview_data_directory()` 헬퍼 추가(LOCALAPPDATA + "TaskMon"), (3) `setup()` 클로저에서 메인 윈도우를 `WebviewWindowBuilder`로 코드 생성하면서 `.data_directory()` 적용, (4) `open_or_focus_settings()`의 설정 윈도우 빌더에도 동일 `.data_directory()` 적용. 메인 윈도우 옵션 6개(transparent/decorations/shadow/resizable/alwaysOnTop/skipTaskbar/visible/title/inner_size) 모두 빌더로 이전. 마이그레이션 코드 없음 — 기존 사용자는 첫 실행 시 모든 설정이 초기화됨(기존 `com.jongc.taskmon` 폴더는 자동 삭제 안 됨, 수동 정리 필요). README의 설정 저장 위치 표 갱신. `cargo check` 통과.
- 2026-04-27: README에 정보 탭 + 펫 말풍선 알림 캡처 추가 — 알림 등록 단계에 `bubble-notification.png`(펫 머리 위 말풍선 표시 예시) 인라인 추가, 일반 설정 다음에 `정보` 섹션 신규(`settings-about.png` 표시 + 정보 탭 설명).
- 2026-04-27: README.md를 MailTrayNotifier 형식으로 전면 개편 — 중앙 정렬 로고/타이틀/설명, 주요 기능 불릿, 시스템 요구 사항, 설치(Releases/소스 빌드), 사용 방법(스크린샷 8장 포함), 폰트/일반 설정 별도 섹션, 설정 저장 위치 표, 주요 의존성 링크, 알려진 제한 사항, 라이선스/Credits/저작권 섹션. `docs/screenshots/` 디렉터리에 캡처 10장 배치(`pet-running`, `tray-menu`, `settings-pet/movement/monitoring/messages/alarm/timer/font/general`). 파일명 오타 2건 정정(`settings-about.png` → `settings-font.png`, `bubble-notification.png` → `settings-general.png`, 실제 캡처 내용에 맞춤).
- 2026-04-27: 몬스터(1)/몬스터(2)/자동차(1) 펫 삭제 + Finn 이름 변경 + 정보 탭 제작자 표기 — `PET_TYPES`에서 `monster1`/`monster2`/`car1` 항목 제거, `finn`의 `name`을 'Finn' → '사람(2)'로 변경(id는 기존 사용자 설정 호환을 위해 'finn' 유지). locales.ts 한/영 키 정리(`pet.monster1`/`pet.monster2`/`pet.car1` 삭제, `pet.finn` 값 '사람(2)'/'Human (2)'로 변경). 이미지 파일 10개(`Monster1_*.png` 4개, `Monster2_*.png` 4개, `Car1_*.png` 2개) 삭제. 정보 탭 itch.io 링크 아래에 제작자 X 태그(@ArksDigital, @LazyHamsters) 표기 추가(`about.assetsCredits` 키). App.css에 `.about-credits` 스타일 추가.
- 2026-04-27: 설정 화면에 "정보" 탭 추가 — 앱 버전, 프로젝트 라이선스(MIT 기반 + 출처 표시 의무), GitHub 저장소 링크, 이미지 에셋 출처(itch.io) 표시. 외부 링크는 `@tauri-apps/plugin-opener`의 `openUrl`로 기본 브라우저 실행. 루트에 `LICENSE` 파일 신규 작성(코드 재사용 시 출처 표시 필수, 이미지 에셋은 별도 라이선스 명시). README에 라이선스/이미지 에셋 섹션 추가. locales.ts에 `sidebar.about`/`about.*` 한·영 번역 키 추가, App.css에 `.about-block`/`.about-row`/`.about-link` 등 정보 탭 전용 스타일 추가.
- 2026-04-27: 사람(2)/사람(4)/UFO(1) 펫 삭제 — `PET_TYPES`에서 `human2`/`human4`/`ufo1` 항목 제거. types.ts import 정리, locales.ts 한/영 번역 키(`pet.human2`/`pet.human4`/`pet.ufo1`) 삭제. 관련 이미지 파일 6개(`Human2_Hurt.png`/`Human2_Idle.png`/`Human2_Run.png`/`Human4_Idle.png`/`Human4_Run.png`/`UFO1_Run.png`) 삭제.
- 2026-03-24: 사람(4) 펫 추가 — Run(36x40, 12프레임)/Idle(34x40, 4프레임). hurt/variant/우클릭 동작 없음.
- 2026-03-24: UFO(1) 펫 추가 — 46x45px 스프라이트(17프레임). 이동/idle 동일 이미지 사용(idle 시 이동하지 않음). hurt/variant/우클릭 동작 없음. types.ts에 펫 정의, locales.ts에 한/영 번역 추가.
- 2026-03-19: 마우스 사용 비활성 상태에서 앱 시작 시 클릭 투과 미적용 버그 수정 — Rust 백엔드의 `mouse_enabled`가 항상 `true`로 초기화되어 localStorage의 `false` 값과 불일치하던 문제. MainWindow 마운트 시 `update_mouse_enabled` invoke로 Rust 상태 동기화 추가.
- 2026-03-18: Rust 성능 최적화 4건 적용 — (1) MonitorInfo 구조체 필드 재배치(f64 선두 배치)로 패딩 4바이트 제거, (2) 전체화면 감지 zip 루프를 `.any()` 이터레이터로 변환(조기 종료 가능), (3) Thread 2 60fps 루프 내 3개 모니터 순회 루프를 단일 루프로 병합, (4) 등반 모드 매 프레임 `Vec<&MonitorInfo>` 힙 할당을 루프 외부 `Vec<usize>` 재사용으로 제거.
- 2026-03-18: 타이머 폰트 크기 설정 추가 — 타이머 탭에 폰트 크기 콤보박스(10~20px, 기본 14px) 추가. Rust `update_timer_font_size` 커맨드 + `timer-font-size-update` 이벤트로 동기화.
- 2026-03-18: 타이머 기능 추가 — 설정에 "타이머" 메뉴 탭 추가. 1~60분 슬라이더로 시간 설정, 시작/중지 버튼. 타이머 진행 중에는 캐릭터 위에 MM:SS 카운트다운 표시, 모든 알림/모니터링 메시지 숨김. idle 상태에서는 기존 모니터링 표시(타이머 숨김). Rust `update_timer_state` 커맨드 + `timer-state-update` 이벤트로 설정↔메인 윈도우 동기화.
- 2026-03-17: 마우스 사용 비활성 시 캐릭터 윈도우 클릭 투과 — `AppState`에 `mouse_enabled` 필드 추가. `update_mouse_enabled` 커맨드에서 AtomicBool로 상태 저장, Thread 2 히트테스트 루프에서 `mouse_enabled`가 false이면 `cursor_on_pet`을 항상 false로 처리하여 `set_ignore_cursor_events(true)` 유지. 캐릭터 클릭 시 아래쪽 바탕화면/앱으로 클릭 통과.
- 2026-03-17: 사람(3) 펫 추가 — 64x95px 스프라이트. Idle(8프레임)/Walk(10프레임)/Run(8프레임) 적용. Hurt 이미지는 Idle 공용. 우클릭으로 Walk↔Run 이동 이미지 토글(hasVariants: true). flipX: true(원본 왼쪽 향함).
- 2026-03-16: '기본 이동 (반복)' 모드 추가 (mode 5) — 오른쪽→왼쪽→오른쪽 반복 이동. 모니터 끝 도달 시 워프 대신 방향 전환. `random_dir_left`로 동적 방향 관리, `move-direction` 이벤트로 프론트엔드 scaleX 동기화.
- 2026-03-16: 랜덤 모드 모니터 건너가기 순간이동 수정 — Phase 3 하단/Phase 1 상단에서 건너간 후 직접 Phase 1/3으로 전환하면 먼 벽까지 순간이동하는 문제. Phase 0/2(하단/상단 이동)로 변경하여 진입 경계에서 출발 → 자연스럽게 걸어서 먼 벽에 도달 → 경계 판정에서 등반/하강으로 자동 전환.
- 2026-03-16: 캐릭터 외 빈 영역 클릭 투과 기능 추가 — 200px 윈도우에서 캐릭터 스프라이트 외 투명 영역 클릭 시 뒤쪽 바탕화면/앱으로 클릭이 통과됨. Rust Thread 2에서 `GetCursorPos`로 커서 위치를 매 프레임 확인, `pet_visual_w` 기반 캐릭터 영역 판정 후 `set_ignore_cursor_events` 토글. 상태 변경 시에만 호출하여 성능 영향 최소화.
- 2026-03-16: 랜덤 모드 하강 완료 후 방향/좌우반전 수정 — Phase 3 하강 완료 시 `random_dir_left = !is_left`로 방향을 뒤집어 캐릭터가 같은 벽 재등반하거나 이웃 모니터에서 잘못된 방향으로 이동하던 문제 해결. (1) `!crossed_at_bottom`: `had_neighbor` 분기 제거, 항상 `random_dir_left = is_left`로 방향 유지하여 하강 벽 반대쪽(모니터 내부)으로 이동, (2) crossed→Phase 0: `random_dir_left = is_left`로 방향 유지 + 위치를 이웃 모니터 먼 쪽에서 출발하도록 swap하여 진입 경계 방향으로 이동. 디버그 console.log 제거.
- 2026-03-16: 펫 높이 오프셋 기능 추가 — 펫 메뉴에 "펫 높이" 슬라이더(-10~10, 기본 0) 추가. 양수=표면에서 멀어짐, 음수=표면으로 가까워짐. 펫별 개별 설정 유지(localStorage). DPI 스케일 적용하여 물리 픽셀 단위로 오프셋 반영. 모든 Phase에 적용: Phase 0(하단 Y), Phase 1(등반 벽면 X), Phase 2(상단 Y), Phase 3(하강 벽면 X), 전환 시점 및 모니터 간 이동 포함.
- 2026-03-16: 랜덤 모드(mode 4) 좌우반전 미적용 수정 — Thread 2의 초기 `move-direction` 이벤트가 프론트엔드 리스너 등록 전에 발행되어 유실되는 문제. `randomDirLeft`가 초기값 `false`에 고정되어 실제 방향(`random_dir_left=true`)과 불일치. 싱글 모니터에서는 방향이 변경되지 않아 자체 복구 불가. (1) 초기 3초간 0.5초마다 방향 재발행으로 유실 복구, (2) Phase 변경 시 방향도 함께 재발행하여 동기화 보장, (3) 비랜덤→랜덤 모드 전환 시 동기화 카운터 리셋.
- 2026-03-16: 배터리 폴링 주기 변경 — 기존 tick 카운터 → `battery_elapsed_ms` 경과 시간 기반으로 변경. 최소 3분 간격, 폴링 간격이 3분 이상이면 폴링 간격으로 자동 확장. 첫 폴링은 ~2초 후 트리거.
- 2026-03-16: 듀얼 모니터(다른 DPI) 모니터 경계·작업표시줄 위치 오류 수정 — (1) `SetProcessDpiAwarenessContext(PER_MONITOR_AWARE_V2)` 호출 추가로 Win32 API 좌표 가상화 방지, (2) Tauri `available_monitors()` + `MonitorFromPoint` 조합을 Win32 `EnumDisplayMonitors` + `GetMonitorInfoW` + `GetDpiForMonitor` 직접 수집으로 교체하여 SetWindowPos와 동일 좌표 공간 보장, (3) DPI 스케일 팩터를 Tauri `scale_factor()` 대신 Win32 `GetDpiForMonitor(MDT_EFFECTIVE_DPI)`로 취득, (4) `actual_win_h`를 `outer_size()`(이전 모니터 DPI 기준) 대신 대상 모니터의 `scale_factor`로 직접 계산하고, 모니터 탐색→높이 계산→Y 계산 순서로 재구성하여 DPI가 다른 모니터에서 Y 위치가 틀어지는 문제 해결.
- 2026-03-15: 등반/하강 위치 오류 수정 — 모니터 좌표(x, y, width, height)를 Tauri API 대신 Win32 GetMonitorInfoW에서 직접 취득하여 SetWindowPos와 동일 좌표 공간(물리 픽셀) 보장. DPI 스케일링 시 Tauri가 반환하는 좌표와 SetWindowPos 좌표의 불일치로 등반 위치가 화면 중간에 표시되던 문제 해결. pet_visual_w도 CSS→물리 픽셀 변환(×prev_scale) 추가.
- 2026-03-15: 전체화면 감지 하이브리드 방식으로 수정 — SHQueryUserNotificationState O(1) 프리체크 + true일 때만 EnumWindows로 펫이 있는 모니터의 전체화면 여부 확인. 기존 시스템 전체 감지(모든 모니터 NOTOPMOST)로 인한 등반/하강 표시 오류 해결. 평상시 EnumWindows 호출 제거, 전체화면 앱 존재 시에만 per-monitor 판정.
- 2026-03-15: available_monitors() IPC 호출 빈도 감소 — 모니터 정보 갱신을 매 1초 → 10초 간격으로 변경. IPC 왕복 + get_work_area_for_monitor Win32 API 호출 90% 감소. 전체화면 감지(EnumWindows)는 매 폴링 유지.
- 2026-03-15: SetWindowPos 추가 최적화 — (1) 위치 미변경 시 호출 생략: 이전 좌표 캐시로 동일 위치 프레임에서 SetWindowPos 완전 제거 (호버 시 100%, 저속 이동 시 부분 생략), (2) SWP_NOSENDCHANGING 추가: WM_WINDOWPOSCHANGING 메시지 생략으로 메시지 체인 오버헤드 감소.
- 2026-03-15: SetWindowPos Z-order 분리 최적화 — 일반 프레임은 SWP_NOZORDER로 위치만 이동, ~500ms(30프레임)마다 또는 전체화면 전환/모니터 변경/윈도우 복구 시에만 HWND_TOPMOST/NOTOPMOST 재적용. DWM Z-order 재계산 빈도를 60fps→2fps로 감소시켜 마우스 응답성 개선.
- 2026-03-15: 좌우/상단 말풍선 표시 체크박스 추가 — 설정 > 말풍선 사용 하위에 "좌우 말풍선 표시"(Phase 1,3), "상단 말풍선 표시"(Phase 2) 체크박스 추가. 기본 체크. Rust 커맨드(update_bubble_side, update_bubble_top) + Tauri 이벤트로 설정↔메인 윈도우 동기화.
- 2026-03-15: Phase 1→건너가기→Phase 2 방향/위치 버그 수정 — 등반 후 인접 모니터로 건너가서 상단 이동 시, 진입 쪽에서 출발하여 반대 방향으로 이동하도록 `random_dir_left = !is_left` 추가 및 위치 분기 교정.
- 2026-03-15: '아무 곳으로 이동' 랜덤 모드 개선 — 등반 상단 건너가기 시 건너간 모니터에서 상단 이동/하강 50% 랜덤 결정. 하강 완료 시 인접 모니터 건너가기 옵션 추가, 건너간 모니터에서 하단 이동/등반 50% 랜덤 결정.
- 2026-03-15: '아무 곳으로 이동' 랜덤 모드 추가 — mode 4. 시작 방향 랜덤, 모니터 경계마다 등반/건너가기 50% 랜덤, 등반 상단 도달 시 인접 모니터 건너가기 50% 랜덤, Phase 2 끝에서 하강/건너가기 50% 랜덤, 하강 완료 시 다음 방향 랜덤 재결정. `move-direction` 이벤트로 프론트엔드 CSS 방향(scaleX) 동적 동기화.
- 2026-03-15: 왼쪽 방향 이동 모드 추가 — '기본 이동'→'기본 이동 (오른쪽)', '등반 이동'→'등반 이동 (오른쪽)'으로 이름 변경. '기본 이동 (왼쪽)', '등반 이동 (왼쪽)' 2개 모드 신규 추가. 왼쪽 모드: CSS scaleX(-1)로 펫 좌우 반전, Rust 이동 방향 역전(Phase 0/2), 등반/하강 벽면 좌우 교체(Phase 1→왼쪽 벽, Phase 3→오른쪽 벽). types.ts MOVE_MODES 4항목, locales.ts 한/영 번역 추가.
- 2026-03-15: 등반/건너가기 50% 확률 수정 — `rand` 크레이트 `rng.gen_bool(0.5)` 사용으로 교체.
- 2026-03-15: 등반 이동 모드 경계 판정 개선 — pet_visual_w로 4개 phase 경계 판정 보정. Phase 전환 시 SetWindowPos를 1프레임 건너뛰어(continue) 프론트엔드 CSS 회전 적용 전 깜빡임 방지.
- 2026-03-15: 등반 이동 모드 추가 — 설정 > "펫 이동" 탭에서 "기본 이동"/"등반 이동" 라디오버튼으로 선택 가능. 등반 모드: 모니터 하단→우측 등반→상단(거꾸로)→좌측 하강 4-phase 순회. 멀티모니터 지원(모니터 간 전환 시 50% 확률 등반/건너가기). CSS transform으로 phase별 스프라이트 회전(rotate). Rust Thread 2에 상태 머신 구현, `move-phase` 이벤트로 프론트엔드 동기화. `MOVE_MODES` 상수 배열로 확장 가능 설계.
- 2026-03-15: 듀얼 모니터 연결 시 캐릭터 미표시(테두리만 이동) 버그 수정 — 모니터 핫플러그 시 WebView GPU 렌더링 컨텍스트 손실로 투명도가 깨지는 문제 해결. Thread 1에서 모니터 수 변경 감지 시 `set_size()` 재호출로 WebView 렌더링 표면 갱신 트리거 + `needs_redraw` 공유 플래그로 Thread 2에 전달. Thread 2에서 `SWP_FRAMECHANGED` 플래그 포함 `SetWindowPos` + `RedrawWindow`(RDW_INVALIDATE|RDW_ERASE|RDW_ALLCHILDREN|RDW_UPDATENOW|RDW_FRAME)로 윈도우 프레임 속성 재적용 및 전체 렌더링 표면 강제 갱신. 높이 캐시 무효화로 DPI 변경 대응.
- 2026-03-14: "아무거나" 랜덤 펫 선택 기능 추가 — 펫 목록 첫 번째에 "아무거나" 항목 추가. 선택 시 앱 실행마다 PET_TYPES에서 무작위 펫 표시. `RANDOM_PET_ID`/`resolveRandomPetId` 상수·함수 추가(types.ts). MainWindow에서 시작 시 1회 해결(`resolvedInitialPetId` ref), 설정 변경 시 즉시 랜덤 해결. localStorage에는 'random' 유지.
- 2026-03-14: 자동차(1) 펫 추가 — 5개 개별 프레임(184x68px)을 스프라이트 스트립으로 합성. 이동/idle 공용 5프레임, hurt 1프레임. frameWidth: 184, frameHeight: 68.
- 2026-03-14: 성능 최적화 3건(4차 리뷰) — (1) `petStyle` useMemo 의존성 정리: `eslint-disable` 제거, `makeBgSize`/`baseSize`/`flipStyle`/`opacityStyle`/`isDefaultColor`를 useMemo 내부에서 직접 계산하여 의존성 완전 명시, (2) 알림 동기화 핸들러 Date 인스턴스 4회→1회 통합(`nowDate` 변수로 자정/정시 경계 논리 불일치 방지), (3) localStorage 디바운스 확대: `monitorConfig`/`bubbleEnabled`/`alarms`의 즉시 쓰기 제거, 기존 500ms 디바운스 useEffect에 `alarms` 통합하여 대형 JSON 직렬화 메인 스레드 차단 최소화.
- 2026-03-14: 펫 속도 조절 기능 추가 — 펫 메뉴의 펫 크기 아래에 슬라이더(0~200%, 기본 100%) 추가. 펫별 개별 속도 설정, localStorage에 `petSpeed_{petId}` 키로 저장. 이동 속도(Rust Thread 2)와 애니메이션 playbackRate 모두 사용자 속도 반영. Rust `update_pet_speed` 커맨드, `pet-speed-update` 이벤트 추가. `update_pet_type`에 `user_speed` 파라미터 추가.
- 2026-03-14: 사람(2) 펫 추가 — Idle(8프레임)/Run(12프레임)/Jump(8프레임, 좌클릭) 적용. 원본 스프라이트 프레임 간격 32px + 좌측 16px 오프셋을 크롭하여 정렬. frameWidth: 32, frameHeight: 30.
- 2026-03-14: 사람(1) 펫 추가 — Female Adventurer 스프라이트에서 Idle(8프레임)/Walk(8프레임)/Death(8프레임)/Dash(8프레임) 적용. 높이 불일치(30/64px) 상단 패딩으로 통일(64px). 우클릭으로 Walk↔Dash 이동 이미지 토글. frameWidth: 48.
- 2026-03-14: 몬스터(6) 펫 추가 — Mushroom 스프라이트에서 Idle(7프레임)/Run(8프레임)/Hit(5프레임)/Stun(18프레임) 적용. 각 프레임 개별 좌우반전 처리. 우클릭 시 Stun 1회 재생. frameWidth: 80, frameHeight: 50.
- 2026-03-14: 몬스터(5) 펫 추가 — Slime3 스프라이트에서 Idle(6프레임)/Walk(8프레임)/Hurt(5프레임)/Run(8프레임)/Attack(9프레임) 적용. 높이 불일치(35/40/54px) 상단 패딩으로 통일(54px). 우클릭 시 Attack 1회 재생 + Walk↔Run 토글.
- 2026-03-14: 몬스터(4) 펫 추가 — Slime2 스프라이트에서 Idle(6프레임)/Walk(8프레임)/Hurt(5프레임)/Run(8프레임)/Death(10프레임) 적용. 높이 불일치(31/35/40px) 상단 패딩으로 통일(40px). 우클릭 시 Death 1회 재생 + Walk↔Run 토글.
- 2026-03-14: 몬스터(3) 펫 추가 — Slime1 스프라이트에서 Idle(6프레임)/Walk(8프레임)/Hurt(5프레임)/Run(8프레임)/Attack(10프레임) 적용. 높이 불일치(30px vs 33px) 상단 패딩으로 통일(33px). 우클릭 시 Attack 1회 재생 + Walk↔Run 이동 이미지 토글 동시 적용. `handleContextMenu`에서 rightClickImage와 hasVariants 동시 지원 추가.
- 2026-03-14: 몬스터(2) 펫 추가 — Orc3 스프라이트에서 Idle(4프레임)/Walk(6프레임)/Hurt(6프레임)/Run(8프레임) 적용. 높이 불일치(40px vs 44px) 상단 패딩으로 통일(44px). 우클릭으로 이동 이미지 전환(Walk↔Run). hasVariants: true.
- 2026-03-14: 몬스터(1) 펫 추가 — Orc2 스프라이트에서 Idle(4프레임)/Walk(6프레임)/Hurt(6프레임)/Run(8프레임) 적용. 높이 불일치(35px vs 40px) 상단 패딩으로 통일(40px). 우클릭으로 이동 이미지 전환(Walk↔Run, variant 순환). hasVariants: true.
- 2026-03-14: Finn 펫 추가 — FinnSprite.png에서 Idle(9프레임)/Run(6프레임)/Hurt(2프레임)/RightClick(5프레임) 분리. `idleFrames`를 `number[]`로 변경하여 variant별 idle 프레임 수 지원. 펫 이름/설명 다국어 처리 추가. 우클릭 시 rightClickImage 1회 재생 후 idle 복귀(hurt 패턴 동일).
- 2026-03-14: 마우스 사용 설정 추가 — 설정 > 설정 탭에 '마우스 사용' 체크박스 추가. 체크 해제 시 캐릭터가 마우스 hover/클릭을 무시(idle 전환·모니터링 표시·이동 정지 비활성). Rust `update_mouse_enabled` 커맨드, `mouse-enabled-update` 이벤트 추가.
- 2026-03-14: 듀얼 모니터 제거 시 캐릭터 사라지는 버그 수정 — Thread 1의 `window.show()`(비동기 Tauri API)가 윈도우가 제거된 모니터 좌표에 있을 때 실행되어 OS가 다시 숨기는 문제. Thread 2에서 텔레포트/랩어라운드 후 `SetWindowPos` 직후 Win32 `ShowWindow(SW_SHOWNA)`를 동기적으로 호출하여 유효 좌표 이동과 표시를 동일 스레드에서 보장. 텔레포트/랩어라운드 시 `smooth_y_init` 초기화 추가.
- 2026-03-14: 배터리 충전 상태 감지 수정 — `starship_battery`의 `bat.state()`가 `Unknown`을 반환하여 충전 상태를 감지하지 못하는 문제 해결. Win32 `GetSystemPowerStatus` API로 AC 전원 연결 여부를 직접 확인하도록 변경(`is_ac_connected()`). 충전 상태를 매 폴링(1초)마다 확인하되 변경 시에만 이벤트 발송하여 플러그 탈착 즉시 반영. 배터리 잔량은 3분 간격 유지.
- 2026-03-14: 충전 아이콘 크기/거리 설정 추가 — 모니터링 메뉴의 '펫 충전 아이콘 표시' 아래에 '충전 아이콘 크기'(크게50%/보통40%/작게30%) 콤보박스와 '충전 아이콘 거리'(-10~10) 콤보박스 추가. 체크 해제 시 비활성화. `MonitorConfig`에 `chargingIconSize`/`chargingIconDistance` 필드 추가, Rust `update_monitor_config`에 대응 파라미터 추가. 기존 localStorage에 새 필드 누락 시 invoke 실패하는 버그 수정(defaults spread 병합).
- 2026-03-14: 펫 충전 아이콘 기능 추가 — 모니터링 메뉴에 '펫 충전 아이콘 표시' 체크박스 추가(배터리 아래 들여쓰기). 체크 시 배터리 충전 중에 캐릭터 왼쪽에 ⚡ 아이콘 표시(캐릭터 중앙 높이, 캐릭터 위치 영향 없음). 데스크탑(배터리 없음)에서는 배터리·충전 아이콘 항목 언체크+비활성화. `MonitorConfig`에 `showChargingIcon` 필드 추가, Rust `update_monitor_config`에 `show_charging_icon` 파라미터 추가.
- 2026-03-14: NSIS 시작메뉴 바로가기 한글화 보완 — 재설치 시 기존 한글 바로가기 충돌 방지(`Delete` 후 `Rename`), 제거 시 한글 바로가기 삭제 누락 해결(`NSIS_HOOK_PREUNINSTALL` 추가).
- 2026-03-14: 성능 최적화 4건 — (1) `getTextShadow` useMemo 캐싱으로 렌더링당 재계산 제거, (2) 알림 동기화 시 `safeParse` → `alarmsRef.current`로 불필요한 JSON 파싱 제거, (3) `evaluateMessages` 정렬을 `useMemo`로 사전 정렬하여 매 모니터링 값 변경 시 sort 제거, (4) `petStyle` IIFE → `useMemo`로 매 렌더링 객체 생성 방지.
- 2026-03-14: 말풍선 높이 설정 추가 — 설정 > 말풍선 사용 아래에 콤보박스(0~30) 추가. 기본값 0(캐릭터 머리 위). Rust `update_bubble_height` 커맨드 추가.
- 2026-03-14: 펫 크기 조절 기능 추가 — 펫 메뉴에 슬라이더(0~200%, 기본 100%)로 펫별 개별 크기 설정. localStorage에 `petScale_{petId}` 키로 저장. Rust `update_pet_scale` 커맨드 추가. 공룡 펫 displayScale 1.3→1.0 변경.
- 2026-03-14: 알림 발화 시간 범위 체크 수정 — daily/absolute 알림이 `targetTime + notificationDuration` 범위를 초과하면 발화 없이 만료 처리. `checkAlarms`에 `notificationDurationSec` 파라미터 추가. 앱 시작·가져오기 핸들러에도 동일 로직 적용. 가져오기 시 이미 지난 알림이 즉시 표시되는 버그 해결.
- 2026-03-14: 폰트 메뉴에 모니터링/알림 메시지 폰트 색상 설정 추가 — 20색 팔레트 UI로 각각 독립 설정. `FONT_COLOR_PALETTE` 상수 추가(types.ts). `update_app_settings` 커맨드에 색상 파라미터 추가. localStorage 저장 + 이벤트 동기화.
- 2026-03-14: 자동 실행 설정 UI 프리징 수정 — `get_auto_start`/`set_auto_start` Tauri 커맨드를 동기→async로 전환하여 레지스트리 조작 시 메인 스레드 블로킹 방지.
- 2026-03-14: 프로젝트 이름 변경 — `TaskBone` → `TaskMon`. 트레이 아이콘 툴팁 한글 "테스크몬"/영문 "TaskMon"으로 수정. package.json, tauri.conf.json, Cargo.toml, main.rs, lib.rs(레지스트리·트레이), SettingsWindow.tsx(확인 팝업), nsis-hooks.nsi(설치 경로·표시 이름), README.md 일괄 변경.
- 2026-03-14: 공룡 우클릭 동작 변경 — 일회성 애니메이션 재생에서 variant 순환(이동 이미지 변경) 방식으로 전환. `runFrames`를 `number[]`로 변경하여 variant별 프레임 수 지원. `rightClickImage`/`rightClickFrames` 필드 제거, RightClick 이미지를 `runImages`의 두 번째 variant로 통합. 미사용 `isRightClickAnim` 상태 및 `rightClickTimerRef` 제거.
- 2026-03-13: 공룡 펫 4종 추가 — DinoSprites 1~4 스프라이트시트에서 Idle(4프레임)/Run(6프레임)/Hurt(4프레임)/RightClick(7프레임) 분리. `PetType`에 `idleFrames`/`runFrames`/`rightClickImage`/`rightClickFrames` 필드 추가. idle 애니메이션도 CSS→Web Animations API 이전하여 프레임 수 동적 처리. 우클릭(idle 상태) 시 전용 애니메이션 재생 지원.
- 2026-03-13: "좀비" 펫 추가 — Walk/Idle/Hurt(6프레임) 스프라이트 추가. `PetType`에 `hurtImage`/`hurtFrames`/`hasVariants`/`flipX`/`speedFactor`/`frameWidth`/`frameHeight`/`bottomPadding` 필드 추가. 펫별 hurt 애니메이션 동적 처리(CSS→Web Animations API 이전), 좌우 반전·프레임 크기·하단 여백·이동 속도 모두 펫별 동적 제어. 좀비 이동 속도 30% 감소(`speedFactor: 0.7`), Rust `AppState`에 `pet_speed_factor` 추가하여 Thread 2 이동 계산에 반영. 스프라이트 playbackRate에도 speedFactor 적용.
- 2026-03-13: 펫 메뉴 개선 — 사이드바 "펫 색상" → "펫"으로 이름 변경. 펫 색상 커스텀 위에 펫 선택 콤보박스 추가. "해골" 펫을 기본 목록으로 등록 (run/idle 전 애니메이션 한 세트). `PetType` 인터페이스 및 `PET_TYPES` 배열 추가.
- 2026-03-13: 알림 가져오기 즉시 발화 버그 수정 — 가져오기로 신규 알림 등록 시 이미 지난 시간의 알림이 즉시 발화되는 문제 해결. `alarm-list-update` 리스너에서 신규 알림에 대해 현재 시간 기준 발화 완료 처리 적용 (daily/absolute/hourly/relative/interval 전 타입 대응).
- 2026-03-13: 성능 최적화 — Thread 2의 매 프레임 `outer_size()` IPC 호출을 DPI 변경 시에만 갱신하도록 캐싱. 알림 타이머의 매초 localStorage JSON 파싱을 `useRef`로 교체. 네트워크 인터페이스 목록 재구성을 30초 주기로 제한. 설정 윈도우 생성 코드 중복 제거(`open_or_focus_settings` 함수 추출). RwLock Vec clone을 Arc swap 패턴으로 교체. App.tsx(~1980줄) 컴포넌트 분리: `types.ts`(261줄) + `MainWindow.tsx`(556줄) + `SettingsWindow.tsx`(1279줄) + `App.tsx`(21줄, 라우터). 설정 윈도우에서 불필요한 모니터링 이벤트 리스너/알림 타이머/애니메이션 제거. 알림 useEffect 의존성 최적화: 리셋 로직을 마운트 전용으로 분리, interval은 `displayConfigRef`/`bubbleEnabledRef`로 최신 설정 참조하여 설정 변경 시 재생성 방지.
- 2026-03-13: 트레이 아이콘 툴팁 추가 — 시스템 언어가 한국어면 "작업뼈다귀", 그 외 "TaskBone" 표시. `sys-locale` 크레이트 추가.
- 2026-03-13: 알림/모니터링 메시지 목록 저장/가져오기 기능 추가 — 등록된 목록을 JSON 파일로 내보내기/가져오기. `tauri-plugin-dialog`, `tauri-plugin-fs` 의존성 추가. 파일 유효성 검증, 기존 목록 삭제 확인 팝업, 빈 목록 시 저장 버튼 비활성화.
- 2026-03-13: 자동 실행 기능 추가 — 설정 > 설정 탭에 "자동 실행" 체크박스. 레지스트리(`HKCU\...\Run\TaskBone`)에 등록/해제. Rust `get_auto_start`/`set_auto_start` 커맨드 추가.
- 2026-03-13: 알림 중복 표시 모드 추가 — 모두 표시/먼저 표시된 메시지 우선/최근 메시지 우선 3가지 라디오 버튼으로 선택. 알림 메뉴 표시 설정에 UI 추가. `checkAlarms` 모든 발화 반환으로 변경, `DisplayConfig`에 `notificationMode` 필드 추가, Rust `update_display_config`에 `notification_mode` 파라미터 추가.
- 2026-03-13: 트레이 아이콘 더블클릭 시 설정 창 열기 기능 추가 (`on_tray_icon_event` 핸들러).

## 최근 변경 요약 (2026-03-12)
- **메시지 순환 표시 기능 추가**: '조건에 맞는 모든 메시지 표시' 체크박스 추가. 활성화 시 매칭된 메시지를 설정된 간격(초)마다 순환 표시. 비활성화 시 기존 우선순위 방식. Rust `update_msg_rotate` 커맨드 추가.
- **이모지 피커 추가**: 모니터링 메시지, 알림 문구 입력란에 이모지 선택 버튼 추가. 8개 카테고리(표정/사람/동물/음식/활동/여행/사물/기호) 약 480개 이모지 지원. `src/emojis.ts` 신규 생성.
- **모니터링 메시지 localStorage 이전**: messages.json 파일 기반 → localStorage 기반으로 변경. Rust 백엔드의 `load_messages`/`save_messages` 제거, `update_messages` 이벤트 릴레이로 대체.
- **설정 메뉴 재구성**: 폴링 간격을 모니터링 탭으로 이동, 폰트 설정을 별도 '폰트' 탭으로 분리, '모니터링 문구 사용' 체크박스를 모니터링 메시지 탭으로 이동.
- **다국어 지원 추가**: 한글/영문 번역 리소스(`src/locales.ts`) 생성. 설정 화면의 모든 텍스트를 `t()` 번역 함수로 교체. 언어 선택(시스템 언어/한국어/영어) 콤보박스 추가. 시스템 언어가 한/영이 아니면 영문 표시.
- **폰트 설정 추가**: 설정 탭에 폰트 크기(8~20px 콤보박스), 폰트 변경(맑은 고딕/굴림/돋움/바탕/Arial/Segoe UI/Consolas) 추가. 메인 윈도우 말풍선에 적용.
- **프로젝트 이름 변경**: `my-pet` → `TaskBone`
- **매시 알림 타입 추가**: 매 시간 지정 분(0~59)에 반복 발화하는 `hourly` 알림 타입 추가.
- **알림 즉시 발화 방지**: 앱 시작 시/알림 추가 시 이미 지난 시간의 알림이 즉시 발화되지 않도록 기준 시점 초기화.
- **모니터링 메시지 설정 UI 추가**: 설정 화면에 "모니터링 메시지" 탭 추가. messages.json을 직접 편집하지 않고 설정 UI에서 메시지 추가/삭제 가능. Rust 백엔드에 `save_messages` 커맨드 추가하여 파일 저장 및 윈도우 간 동기화.
- **알림 시스템 추가**: 4가지 알림 타입(반복/특정시간/매일/타이머) 지원. 설정 화면에 알림 탭 추가하여 CRUD 관리. 모니터링/알림 문구 개별 표시 제어, 알림 표시 시간 설정 가능. 알림 문구 비활성화 시 타이머 미동작.
- **시작 시 검은 창 깜빡임 방지**: 메인 윈도우를 `visible: false`로 생성하고, 프론트엔드 렌더링 완료 후 `show_main_window` 커맨드로 표시. PC가 느릴 때 검은 창이 이동하다가 이미지가 나타나는 문제 해결.

## 최근 변경 요약 (2026-03-11)
- **항상 앞쪽 표시 수정**: 모니터 간 이동 시 다른 창 뒤로 숨는 문제 해결. Thread 2에서 `set_position` 대신 Win32 `SetWindowPos`를 직접 호출하여 매 프레임 위치 이동과 `HWND_TOPMOST`를 동시에 적용.
- **모니터 핫플러그 수정**: 듀얼→싱글 시 캐릭터 사라짐, 싱글→듀얼 시 1번 모니터에서만 이동하는 문제 해결. Thread 2의 이동 범위를 `shared_avail_monitors`(전체화면 제외) → `shared_monitors`(전체 모니터)로 변경. Thread 1에서 모니터 수 변경 감지 시 `show()` + 텔레포트 좌표 전달로 윈도우 복구.
- **모니터 전환 부드러움 개선**: `current_scale`을 프레임 간 유지하여 경계 진동(크기/높이 변화, 앞뒤 점프) 방지. `target_y` 보간(lerp)으로 높이 점프 완화. 속도 공식 변경(`/10.0`→`/25.0`, 최대 5x)으로 고속 이동 끊김 감소.

## 최근 변경 요약 (2026-03-10)
- **메시지 시스템 추가**: 시스템 상태(CPU, 메모리, 배터리 등)에 따라 펫이 말풍선으로 메시지를 표시. 외부 `messages.json` 파일로 사용자 커스텀 가능.
- **설정 메뉴 추가**: 폴링 간격(초 단위) 설정, 말풍선 사용 토글 기능.
- **윈도우 너비 확장**: 120px → 200px (메시지 표시 공간 확보).
- **배터리 폴링 타이밍 수정**: 첫 폴링을 2초 후로 지연하여 React 리스너 등록 후 이벤트 수신 보장.

## 최근 변경 요약 (2026-03-06)
- 전체화면 감지 최적화, 인터랙션(좌클릭 Hurt/우클릭 무기교체) 추가, Vite 동적 이미지 로딩 수정.

## 미해결 및 향후 과제
- 없음 (현재 모든 주요 기능 구현 완료)

## 주의사항
- `lib.rs` 수정 시 `cargo build`를 통해 Win32 API 호출부의 안정성을 항상 확인해야 함.
- 애니메이션 프레임 추가 시 `App.css`의 `background-size`와 `steps()` 값을 이미지 너비에 맞춰 갱신해야 함.
