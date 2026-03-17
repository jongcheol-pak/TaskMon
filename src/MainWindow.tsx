import { useEffect, useMemo, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import {
  type MonitorConfig,
  type PetMessage,
  type Alarm,
  type DisplayConfig,
  DEFAULT_DISPLAY_CONFIG,
  getPetType,
  RANDOM_PET_ID,
  resolveRandomPetId,
  generateAlarmId,
  checkAlarms,
  evaluateMessages,
  getTextShadow,
  safeParse,
  formatBytes,
} from "./types";

export default function MainWindow() {
  const speedRef = useRef(1);
  const [isHovered, setIsHovered] = useState(false);
  const [isHurt, setIsHurt] = useState(false);
  const [cpuUsage, setCpuUsage] = useState(0);
  const [memUsage, setMemUsage] = useState(0);
  const [networkDown, setNetworkDown] = useState(0);
  const [networkUp, setNetworkUp] = useState(0);
  const [batteryPercent, setBatteryPercent] = useState(-1); // -1 = 배터리 없음
  const [batteryCharging, setBatteryCharging] = useState(false);
  const [isTestMode, setIsTestMode] = useState(false);
  const [testCpuValue, setTestCpuValue] = useState(50);
  const [monitorConfig, setMonitorConfig] = useState<MonitorConfig>(() => {
    const defaults: MonitorConfig = { cpu: true, memory: true, network: false, battery: false, showChargingIcon: false, chargingIconSize: 'medium', chargingIconDistance: 0 };
    return { ...defaults, ...safeParse('monitorConfig', defaults) };
  });
  const [runVariant, setRunVariant] = useState<number>(() => {
    const saved = localStorage.getItem('petRunVariant');
    return saved !== null ? Number(saved) : 0;
  }); // 0=맨손, 1=검, 2=검+방패

  // 앱 시작 시 'random'이면 실제 펫 ID로 해결 (모든 초기화에서 공유)
  const resolvedInitialPetId = useRef(
    (() => {
      const saved = localStorage.getItem('selectedPetId') || RANDOM_PET_ID;
      return saved === RANDOM_PET_ID ? resolveRandomPetId() : saved;
    })()
  );
  // 선택된 펫 종류 (random이면 이미 해결된 ID로 초기화)
  const [selectedPetId, setSelectedPetId] = useState<string>(resolvedInitialPetId.current);
  // 펫별 사용자 크기 (0~200%, 기본 100%)
  const [petScale, setPetScale] = useState<number>(() => {
    const saved = localStorage.getItem(`petScale_${resolvedInitialPetId.current}`);
    return saved ? Number(saved) : 100;
  });
  // 펫별 사용자 속도 배율 (0~200%, 기본 100%)
  const [petUserSpeed, setPetUserSpeed] = useState<number>(() => {
    const saved = localStorage.getItem(`petSpeed_${resolvedInitialPetId.current}`);
    return saved ? Number(saved) : 100;
  });

  // 색상 필터 상태 (Hue: 0~360, Saturation: 0~200, Brightness: 0~200)
  const [hue, setHue] = useState<number>(() => {
    const saved = localStorage.getItem('petHue');
    return saved !== null ? Number(saved) : 0;
  });
  const [saturation, setSaturation] = useState<number>(() => {
    const saved = localStorage.getItem('petSaturation');
    return saved !== null ? Number(saved) : 100;
  });
  const [brightness, setBrightness] = useState<number>(() => {
    const saved = localStorage.getItem('petBrightness');
    return saved !== null ? Number(saved) : 100;
  });
  const [petOpacity, setPetOpacity] = useState<number>(() => {
    const saved = localStorage.getItem('petOpacity');
    return saved !== null ? Number(saved) : 100;
  });

  // 메시지 시스템 상태
  const [petMessages, setPetMessages] = useState<PetMessage[]>(() =>
    safeParse<PetMessage[]>('petMessages', [])
  );
  const [petMessage, setPetMessage] = useState<string | null>(null);

  // 조건에 맞는 모든 메시지 순환 표시 설정
  const [showAllMessages, setShowAllMessages] = useState<boolean>(() => {
    const saved = localStorage.getItem('showAllMessages');
    return saved === 'true';
  });
  const [rotateInterval, setRotateInterval] = useState<number>(() => {
    const saved = localStorage.getItem('rotateInterval');
    return saved ? Number(saved) : 10;
  });
  // 매칭된 메시지 목록 (순환 표시용)
  const matchedMessagesRef = useRef<string[]>([]);
  const rotateIndexRef = useRef(0);

  // 설정: 마우스 사용 여부
  const [mouseEnabled, setMouseEnabled] = useState<boolean>(() => {
    const saved = localStorage.getItem('mouseEnabled');
    return saved !== null ? saved === 'true' : true;
  });

  // 설정: 말풍선 사용 여부
  const [bubbleEnabled, setBubbleEnabled] = useState<boolean>(() => {
    const saved = localStorage.getItem('bubbleEnabled');
    return saved !== null ? saved === 'true' : true;
  });
  const [bubbleSide, setBubbleSide] = useState<boolean>(() => {
    const saved = localStorage.getItem('bubbleSide');
    return saved !== null ? saved === 'true' : true;
  });
  const [bubbleTop, setBubbleTop] = useState<boolean>(() => {
    const saved = localStorage.getItem('bubbleTop');
    return saved !== null ? saved === 'true' : true;
  });
  const [bubbleHeight, setBubbleHeight] = useState<number>(() => {
    const saved = localStorage.getItem('bubbleHeight');
    return saved ? Number(saved) : 0;
  });

  // 알림 시스템 상태
  const [alarms, setAlarms] = useState<Alarm[]>(() =>
    safeParse<Alarm[]>('alarms', [])
  );
  const [displayConfig, setDisplayConfig] = useState<DisplayConfig>(() => ({
    ...DEFAULT_DISPLAY_CONFIG,
    ...safeParse<Partial<DisplayConfig>>('displayConfig', {}),
  }));
  // 활성 알림 목록 (메시지 + 발화 시점 + 고유 ID)
  const [activeNotifications, setActiveNotifications] = useState<Array<{ id: string; message: string; firedAt: number }>>([]);
  // "먼저 표시된 메시지 우선" 모드: 대기열
  const notificationQueueRef = useRef<Array<{ id: string; message: string; firedAt: number }>>([]);
  // 알림 타이머에서 최신 알림 상태 참조용 (매초 localStorage 파싱 제거)
  const alarmsRef = useRef(alarms);
  const notificationTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  // 폰트 설정
  const [fontSize, setFontSize] = useState<number>(() => {
    const saved = localStorage.getItem('fontSize');
    return saved ? Number(saved) : 12;
  });
  const [fontFamily, setFontFamily] = useState<string>(() => {
    return localStorage.getItem('fontFamily') || '';
  });
  const [monitoringFontColor, setMonitoringFontColor] = useState<string>(() => {
    return localStorage.getItem('monitoringFontColor') || '#FFFFFF';
  });
  const [alarmFontColor, setAlarmFontColor] = useState<string>(() => {
    return localStorage.getItem('alarmFontColor') || '#FFFFFF';
  });

  // 등반 이동 phase (0=Bottom, 1=ClimbRight, 2=Top, 3=DescendLeft)
  const [movePhase, setMovePhase] = useState(0);
  // 이동 모드 (0=기본 오른쪽, 1=등반 오른쪽, 2=기본 왼쪽, 3=등반 왼쪽, 4=랜덤)
  const [moveMode, setMoveMode] = useState<number>(() => {
    const saved = localStorage.getItem('moveMode');
    return saved !== null ? Number(saved) : 0;
  });
  // 랜덤 모드 동적 방향 (Rust에서 move-direction 이벤트로 업데이트)
  const [randomDirLeft, setRandomDirLeft] = useState(false);

  const skeletonRef = useRef<HTMLDivElement>(null);
  const animRef = useRef<Animation | null>(null);
  const hurtTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const [isRightClickAnim, setIsRightClickAnim] = useState(false);
  const rightClickTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  // 메인 윈도우 렌더링 완료 후 표시 (검은 창 깜빡임 방지)
  useEffect(() => {
    invoke("show_main_window");
  }, []);

  // 저장된 폴링 간격을 Rust에 적용 (앱 시작 시 1회)
  useEffect(() => {
    const saved = localStorage.getItem('pollingInterval');
    if (saved) {
      const seconds = parseInt(saved, 10);
      if (!isNaN(seconds) && seconds > 0) {
        invoke("set_polling_interval", { seconds });
      }
    }
    // 저장된 펫의 이동 속도 배율을 Rust에 적용 (펫 고유 속도 × 사용자 속도)
    // 'random'이면 이미 해결된 resolvedInitialPetId 사용
    const petId = resolvedInitialPetId.current;
    const pet = getPetType(petId);
    const savedSpeed = localStorage.getItem(`petSpeed_${petId}`);
    const userSpeed = savedSpeed ? Number(savedSpeed) / 100 : 1.0;
    invoke("update_pet_type", { petId: pet.id, speedFactor: pet.speedFactor, userSpeed });
    // 저장된 펫 높이 오프셋을 Rust에 적용
    const savedHeight = localStorage.getItem(`petHeight_${petId}`);
    const heightOffset = savedHeight ? Number(savedHeight) : 0;
    invoke("update_pet_height", { petId: pet.id, offset: heightOffset });
    // 저장된 이동 모드를 Rust에 적용
    const savedMode = localStorage.getItem('moveMode');
    if (savedMode !== null) {
      invoke("update_move_mode", { mode: Number(savedMode) });
    }
  }, []);

  // 메시지 목록은 변경 시에만 정렬 (매 모니터링 값 변경 시 재정렬 방지)
  const sortedMessages = useMemo(() =>
    [...petMessages].sort((a, b) => b.priority - a.priority),
    [petMessages]
  );

  // 메시지 평가: 모니터링 값 변경 시마다 매칭 목록 갱신
  useEffect(() => {
    if (sortedMessages.length === 0) {
      matchedMessagesRef.current = [];
      setPetMessage(null);
      return;
    }

    const effectiveCpu = isTestMode ? testCpuValue : cpuUsage;
    const matched = evaluateMessages(sortedMessages, effectiveCpu, memUsage, batteryPercent, networkDown, networkUp, monitorConfig);
    matchedMessagesRef.current = matched;

    if (!showAllMessages) {
      // 우선순위 모드: 가장 높은 우선순위 1개만 표시
      setPetMessage(matched.length > 0 ? matched[0] : null);
    } else if (matched.length === 0) {
      setPetMessage(null);
      rotateIndexRef.current = 0;
    } else {
      // 순환 모드: 현재 인덱스가 범위를 벗어나면 리셋
      if (rotateIndexRef.current >= matched.length) {
        rotateIndexRef.current = 0;
      }
      setPetMessage(matched[rotateIndexRef.current]);
    }
  }, [cpuUsage, memUsage, batteryPercent, networkDown, networkUp, sortedMessages, isTestMode, testCpuValue, monitorConfig, showAllMessages]);

  // 순환 표시 타이머: showAllMessages가 켜져 있을 때만 동작
  useEffect(() => {
    if (!showAllMessages) return;

    const timer = setInterval(() => {
      const matched = matchedMessagesRef.current;
      if (matched.length <= 1) return;
      rotateIndexRef.current = (rotateIndexRef.current + 1) % matched.length;
      setPetMessage(matched[rotateIndexRef.current]);
    }, rotateInterval * 1000);

    return () => clearInterval(timer);
  }, [showAllMessages, rotateInterval]);

  // alarmsRef를 최신 상태로 동기화 (알림 타이머에서 참조)
  useEffect(() => { alarmsRef.current = alarms; }, [alarms]);

  // displayConfig와 bubbleEnabled를 ref로 추적 (알림 타이머에서 최신 값 참조, 의존성 제거)
  const displayConfigRef = useRef(displayConfig);
  useEffect(() => { displayConfigRef.current = displayConfig; }, [displayConfig]);
  const bubbleEnabledRef = useRef(bubbleEnabled);
  useEffect(() => { bubbleEnabledRef.current = bubbleEnabled; }, [bubbleEnabled]);

  // 앱 시작 시 1회만 실행: 이미 조건을 만족하는 알림이 즉시 발화되지 않도록 기준 시점 초기화
  useEffect(() => {
    const startAlarms = alarmsRef.current;
    if (startAlarms.length > 0) {
      const now = Date.now();
      const today = new Date().toISOString().slice(0, 10);
      const resetAlarms = startAlarms.map(alarm => {
        if (!alarm.enabled) return alarm;
        switch (alarm.type) {
          case 'interval':
            // 마지막 발화 시점을 현재로 리셋하여 간격을 처음부터 다시 카운트
            return { ...alarm, lastFiredAt: now, startedAt: alarm.startedAt || now };
          case 'absolute':
          case 'daily': {
            // 알림 시간 + 표시 시간 범위를 초과한 경우에만 발화 완료 처리
            if (alarm.targetTime) {
              const [th, tm] = alarm.targetTime.split(':').map(Number);
              const target = new Date();
              target.setHours(th, tm, 0, 0);
              const durationMs = displayConfigRef.current.notificationDuration * 1000;
              if (now > target.getTime() + durationMs) {
                return { ...alarm, firedToday: today };
              }
            }
            return alarm;
          }
          case 'relative':
            // 이미 지난 타이머는 발화 완료 처리
            if (alarm.absoluteTarget && now >= alarm.absoluteTarget) {
              return { ...alarm, relativeFired: true, enabled: false };
            }
            return alarm;
          case 'hourly':
            // 현재 시간의 설정 분이 이미 지났으면 이번 시간은 발화 완료 처리
            if (alarm.hourlyMinute != null && new Date().getMinutes() >= alarm.hourlyMinute) {
              return { ...alarm, lastFiredHour: new Date().getHours() };
            }
            return alarm;
        }
        return alarm;
      });
      localStorage.setItem('alarms', JSON.stringify(resetAlarms));
      setAlarms(resetAlarms);
    }
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // 알림 타이머: 마운트 시 1회 생성, ref로 최신 설정 참조 (설정 변경 시 재생성/리셋 방지)
  useEffect(() => {
    const intervalId = setInterval(() => {
      const dc = displayConfigRef.current;
      if (!dc.showNotificationText || !bubbleEnabledRef.current) return;

      const currentAlarms = alarmsRef.current;
      if (currentAlarms.length === 0) return;

      const durationMs = dc.notificationDuration * 1000;
      const mode = dc.notificationMode;

      const { firedMessages, updatedAlarms } = checkAlarms(currentAlarms, dc.notificationDuration);
      if (firedMessages.length > 0) {
        setAlarms(updatedAlarms);

        const newEntries = firedMessages.map(f => ({
          id: generateAlarmId(),
          message: f.message,
          firedAt: f.firedAt,
        }));

        if (mode === 'all') {
          // 모두 표시: 기존 알림에 추가, 각각 표시 시간 경과 시 개별 삭제
          setActiveNotifications(prev => [...prev, ...newEntries]);
        } else if (mode === 'first') {
          // 먼저 표시된 메시지 우선: 대기열에 추가 (최대 50개 제한)
          notificationQueueRef.current.push(...newEntries);
          if (notificationQueueRef.current.length > 50) {
            notificationQueueRef.current = notificationQueueRef.current.slice(-50);
          }
          setActiveNotifications(prev => {
            if (prev.length === 0) {
              // 현재 표시 중인 알림 없음 → 대기열 첫 번째 표시
              const next = notificationQueueRef.current.shift();
              return next ? [next] : [];
            }
            return prev;
          });
        } else {
          // 최근 메시지 우선: 가장 마지막 메시지만 표시
          const latest = newEntries[newEntries.length - 1];
          setActiveNotifications([latest]);
          // 타이머 초기화
          if (notificationTimerRef.current) clearTimeout(notificationTimerRef.current);
          notificationTimerRef.current = setTimeout(() => {
            setActiveNotifications([]);
          }, durationMs);
        }
      }

      // "모두 표시" 모드: 표시 시간이 지난 알림 제거
      if (mode === 'all') {
        const now = Date.now();
        setActiveNotifications(prev => {
          const filtered = prev.filter(n => now - n.firedAt < durationMs);
          return filtered.length !== prev.length ? filtered : prev;
        });
      }

      // "먼저 표시된 메시지 우선" 모드: 현재 알림 만료 시 대기열에서 다음 표시
      if (mode === 'first') {
        const now = Date.now();
        // 대기열에서 표시 시간이 지난 항목 제거
        notificationQueueRef.current = notificationQueueRef.current.filter(
          n => now - n.firedAt < durationMs
        );
        setActiveNotifications(prev => {
          if (prev.length === 0) return prev;
          const current = prev[0];
          // 현재 표시 중인 알림의 남은 시간 계산 (발화 시점 기준)
          if (now - current.firedAt >= durationMs) {
            // 현재 알림 만료 → 대기열에서 다음 알림 가져오기
            const next = notificationQueueRef.current.shift();
            return next ? [next] : [];
          }
          return prev;
        });
      }
    }, 1000);

    return () => clearInterval(intervalId);
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // 메인 윈도우 전용 이벤트 리스너: 시스템 모니터링 데이터 수신
  useEffect(() => {
    const unlisten = listen<number>("cpu-usage", (event) => {
      const newSpeed = 1 + (event.payload / 10);
      speedRef.current = newSpeed;
      if (animRef.current) {
        animRef.current.playbackRate = newSpeed * speedFactorRef.current;
      }
      setCpuUsage(Math.round(event.payload));
    });
    const unlistenMem = listen<number>("memory-usage", (event) => {
      setMemUsage(event.payload);
    });
    const unlistenTestMode = listen<number>("test-mode-sync", (event) => {
      const usage = event.payload;
      if (usage >= 0) {
        setIsTestMode(true);
        setTestCpuValue(usage);
      } else {
        setIsTestMode(false);
      }
    });
    const unlistenColor = listen<{hue: number, saturation: number, brightness: number, opacity: number}>("color-update", (event) => {
      setHue(event.payload.hue);
      setSaturation(event.payload.saturation);
      setBrightness(event.payload.brightness);
      setPetOpacity(event.payload.opacity);
    });
    const unlistenNetwork = listen<{down: number, up: number}>("network-usage", (event) => {
      setNetworkDown(event.payload.down);
      setNetworkUp(event.payload.up);
    });
    const unlistenBattery = listen<{percent: number, charging: boolean}>("battery-usage", (event) => {
      setBatteryPercent(event.payload.percent);
      setBatteryCharging(event.payload.charging);
      localStorage.setItem('batteryPercent', String(event.payload.percent));
    });
    const unlistenMonitorConfig = listen<MonitorConfig>("monitor-config-update", (event) => {
      setMonitorConfig(event.payload);
    });
    const unlistenMouse = listen<boolean>("mouse-enabled-update", (event) => {
      setMouseEnabled(event.payload);
      localStorage.setItem('mouseEnabled', String(event.payload));
    });
    const unlistenBubble = listen<boolean>("bubble-enabled-update", (event) => {
      setBubbleEnabled(event.payload);
    });
    const unlistenBubbleSide = listen<boolean>("bubble-side-update", (event) => {
      setBubbleSide(event.payload);
      localStorage.setItem('bubbleSide', String(event.payload));
    });
    const unlistenBubbleTop = listen<boolean>("bubble-top-update", (event) => {
      setBubbleTop(event.payload);
      localStorage.setItem('bubbleTop', String(event.payload));
    });
    const unlistenBubbleHeight = listen<number>("bubble-height-update", (event) => {
      setBubbleHeight(event.payload);
      localStorage.setItem('bubbleHeight', String(event.payload));
    });
    // 알림 목록 동기화 (설정 윈도우 → 메인 윈도우)
    // 설정 윈도우에서 보내는 알람에는 발화 상태(lastFiredHour 등)가 없으므로,
    // 기존 localStorage의 발화 상태를 병합하여 중복 발화를 방지
    const unlistenAlarms = listen<Alarm[]>("alarm-list-update", (event) => {
      const nowDate = new Date();
      const now = nowDate.getTime();
      const today = nowDate.toISOString().slice(0, 10);
      const currentHour = nowDate.getHours();
      const currentMin = nowDate.getMinutes();

      const existing = alarmsRef.current;
      const existingMap = new Map(existing.map(a => [a.id, a]));
      const merged = event.payload.map(alarm => {
        const prev = existingMap.get(alarm.id);
        if (prev) {
          // 기존 알림: 발화 상태 병합
          return {
            ...alarm,
            lastFiredAt: prev.lastFiredAt,
            lastFiredHour: prev.lastFiredHour,
            firedToday: prev.firedToday,
            startedAt: prev.startedAt,
            relativeFired: prev.relativeFired,
            absoluteTarget: prev.absoluteTarget ?? alarm.absoluteTarget,
          };
        }
        // 신규 알림: 알림 시간 + 표시 시간 범위를 초과한 경우에만 발화 완료 처리
        if (!alarm.enabled) return alarm;
        const durationMs = displayConfigRef.current.notificationDuration * 1000;
        switch (alarm.type) {
          case 'interval':
            return { ...alarm, lastFiredAt: now, startedAt: alarm.startedAt || now };
          case 'absolute': {
            if (alarm.targetTime) {
              const [th, tm] = alarm.targetTime.split(':').map(Number);
              const target = new Date();
              target.setHours(th, tm, 0, 0);
              if (now > target.getTime() + durationMs) {
                return { ...alarm, firedToday: today, enabled: false };
              }
            }
            return alarm;
          }
          case 'daily': {
            if (alarm.targetTime) {
              const [th, tm] = alarm.targetTime.split(':').map(Number);
              const target = new Date();
              target.setHours(th, tm, 0, 0);
              if (now > target.getTime() + durationMs) {
                return { ...alarm, firedToday: today };
              }
            }
            return alarm;
          }
          case 'relative':
            if (alarm.absoluteTarget && now >= alarm.absoluteTarget) {
              return { ...alarm, relativeFired: true, enabled: false };
            }
            return alarm;
          case 'hourly':
            if (alarm.hourlyMinute != null && currentMin >= alarm.hourlyMinute) {
              return { ...alarm, lastFiredHour: currentHour };
            }
            return alarm;
          default:
            return alarm;
        }
      });
      setAlarms(merged);
    });
    // 표시 설정 동기화 (설정 윈도우 → 메인 윈도우)
    const unlistenDisplayConfig = listen<DisplayConfig>("display-config-update", (event) => {
      setDisplayConfig(event.payload);
      localStorage.setItem('displayConfig', JSON.stringify(event.payload));
    });
    // 모니터링 메시지 동기화 (설정 윈도우 → 메인 윈도우)
    const unlistenMessages = listen<PetMessage[]>("messages-update", (event) => {
      setPetMessages(event.payload);
      localStorage.setItem('petMessages', JSON.stringify(event.payload));
    });
    // 폰트/언어 설정 동기화 (설정 윈도우 → 메인 윈도우)
    const unlistenAppSettings = listen<{language: string, fontSize: number, fontFamily: string, monitoringFontColor?: string, alarmFontColor?: string}>("app-settings-update", (event) => {
      setFontSize(event.payload.fontSize);
      setFontFamily(event.payload.fontFamily);
      if (event.payload.monitoringFontColor) {
        setMonitoringFontColor(event.payload.monitoringFontColor);
        localStorage.setItem('monitoringFontColor', event.payload.monitoringFontColor);
      }
      if (event.payload.alarmFontColor) {
        setAlarmFontColor(event.payload.alarmFontColor);
        localStorage.setItem('alarmFontColor', event.payload.alarmFontColor);
      }
      localStorage.setItem('language', event.payload.language);
      localStorage.setItem('fontSize', String(event.payload.fontSize));
      localStorage.setItem('fontFamily', event.payload.fontFamily);
    });
    // 메시지 순환 표시 설정 동기화 (설정 윈도우 → 메인 윈도우)
    const unlistenMsgRotate = listen<{showAll: boolean, interval: number}>("msg-rotate-update", (event) => {
      setShowAllMessages(event.payload.showAll);
      setRotateInterval(event.payload.interval);
      localStorage.setItem('showAllMessages', String(event.payload.showAll));
      localStorage.setItem('rotateInterval', String(event.payload.interval));
    });
    // 펫 종류 동기화 (설정 윈도우 → 메인 윈도우)
    const unlistenPetType = listen<string>("pet-type-update", (event) => {
      const isRandom = event.payload === RANDOM_PET_ID;
      const resolvedId = isRandom ? resolveRandomPetId() : event.payload;
      setSelectedPetId(resolvedId);
      setRunVariant(0); // 펫 변경 시 variant 초기화
      localStorage.setItem('selectedPetId', event.payload); // 'random' 유지
      localStorage.setItem('petRunVariant', '0');
      // 해결된 펫의 저장된 크기/속도 로드
      const savedScale = localStorage.getItem(`petScale_${resolvedId}`);
      setPetScale(savedScale ? Number(savedScale) : 100);
      const savedSpeed = localStorage.getItem(`petSpeed_${resolvedId}`);
      setPetUserSpeed(savedSpeed ? Number(savedSpeed) : 100);
      // 'random' 해결 후 실제 펫의 속도/높이를 Rust에 적용
      if (isRandom) {
        const pet = getPetType(resolvedId);
        const userSpeed = savedSpeed ? Number(savedSpeed) / 100 : 1.0;
        invoke("update_pet_speed", { petId: resolvedId, speedFactor: pet.speedFactor, userSpeed });
        const savedH = localStorage.getItem(`petHeight_${resolvedId}`);
        invoke("update_pet_height", { petId: resolvedId, offset: savedH ? Number(savedH) : 0 });
      }
    });
    // 펫 크기 동기화 (설정 윈도우 → 메인 윈도우)
    const unlistenPetScale = listen<{petId: string, scale: number}>("pet-scale-update", (event) => {
      localStorage.setItem(`petScale_${event.payload.petId}`, String(event.payload.scale));
      setPetScale(event.payload.scale);
    });
    // 펫 속도 동기화 (설정 윈도우 → 메인 윈도우)
    const unlistenPetSpeed = listen<{petId: string, userSpeed: number}>("pet-speed-update", (event) => {
      localStorage.setItem(`petSpeed_${event.payload.petId}`, String(Math.round(event.payload.userSpeed * 100)));
      setPetUserSpeed(Math.round(event.payload.userSpeed * 100));
    });
    // 펫 높이 오프셋 동기화 (설정 윈도우 → 메인 윈도우)
    const unlistenPetHeight = listen<{petId: string, offset: number}>("pet-height-update", (event) => {
      localStorage.setItem(`petHeight_${event.payload.petId}`, String(event.payload.offset));
    });
    // 등반 이동 phase 변경 수신
    const unlistenMovePhase = listen<number>("move-phase", (event) => {
      setMovePhase(event.payload);
    });
    // 이동 모드 변경 수신
    const unlistenMoveMode = listen<number>("move-mode-update", (event) => {
      setMoveMode(event.payload);
      localStorage.setItem('moveMode', String(event.payload));
    });
    // 랜덤 모드 방향 변경 수신
    const unlistenMoveDir = listen<boolean>("move-direction", (event) => {
      setRandomDirLeft(event.payload);
    });

    return () => {
      unlisten.then((f) => f());
      unlistenMem.then((f) => f());
      unlistenTestMode.then((f) => f());
      unlistenColor.then((f) => f());
      unlistenNetwork.then((f) => f());
      unlistenBattery.then((f) => f());
      unlistenMonitorConfig.then((f) => f());
      unlistenMouse.then((f) => f());
      unlistenBubble.then((f) => f());
      unlistenBubbleSide.then((f) => f());
      unlistenBubbleTop.then((f) => f());
      unlistenBubbleHeight.then((f) => f());
      unlistenAlarms.then((f) => f());
      unlistenDisplayConfig.then((f) => f());
      unlistenMessages.then((f) => f());
      unlistenAppSettings.then((f) => f());
      unlistenMsgRotate.then((f) => f());
      unlistenPetType.then((f) => f());
      unlistenPetScale.then((f) => f());
      unlistenPetSpeed.then((f) => f());
      unlistenPetHeight.then((f) => f());
      unlistenMovePhase.then((f) => f());
      unlistenMoveMode.then((f) => f());
      unlistenMoveDir.then((f) => f());
    };
  }, []);

  // 빈번히 변경되는 상태 + JSON 직렬화가 필요한 상태를 500ms 디바운스로 일괄 저장 (메인 스레드 차단 최소화)
  useEffect(() => {
    const timer = setTimeout(() => {
      localStorage.setItem('petRunVariant', String(runVariant));
      localStorage.setItem('petHue', String(hue));
      localStorage.setItem('petSaturation', String(saturation));
      localStorage.setItem('petBrightness', String(brightness));
      localStorage.setItem('petOpacity', String(petOpacity));
      localStorage.setItem('monitorConfig', JSON.stringify(monitorConfig));
      localStorage.setItem('bubbleEnabled', String(bubbleEnabled));
      localStorage.setItem('bubbleSide', String(bubbleSide));
      localStorage.setItem('bubbleTop', String(bubbleTop));
      localStorage.setItem('alarms', JSON.stringify(alarms));
    }, 500);
    return () => clearTimeout(timer);
  }, [runVariant, hue, saturation, brightness, petOpacity, monitorConfig, bubbleEnabled, bubbleSide, bubbleTop, alarms]);

  // 타이머 정리
  useEffect(() => {
    return () => {
      if (notificationTimerRef.current) clearTimeout(notificationTimerRef.current);
      if (rightClickTimerRef.current) clearTimeout(rightClickTimerRef.current);
    };
  }, []);

  // 현재 선택된 펫 정보
  const currentPet = getPetType(selectedPetId);
  const userSpeedFactor = petUserSpeed / 100;
  const speedFactorRef = useRef(currentPet.speedFactor * userSpeedFactor);
  speedFactorRef.current = currentPet.speedFactor * userSpeedFactor;
  // variant 인덱스가 범위를 넘으면 0으로 보정
  const safeVariant = runVariant < currentPet.runImages.length ? runVariant : 0;

  // 스프라이트 애니메이션 (run/idle 상태를 JS에서 제어)
  // 현재 variant의 이동/idle 프레임 수
  const currentRunFrames = currentPet.runFrames[safeVariant] ?? currentPet.runFrames[0];
  const currentIdleFrames = currentPet.idleFrames[safeVariant] ?? currentPet.idleFrames[0];
  // 스케일 적용된 애니메이션 폭 계산 헬퍼
  const userScale = petScale / 100;
  const scaledAnimWidth = (frames: number) => Math.round(currentPet.frameWidth * frames * currentPet.displayScale * userScale);

  useEffect(() => {
    if (isHurt || isRightClickAnim || !skeletonRef.current) return;

    if (isHovered) {
      // idle 애니메이션
      const totalWidth = scaledAnimWidth(currentIdleFrames);
      const anim = skeletonRef.current.animate(
        [
          { backgroundPosition: '0px 0px' },
          { backgroundPosition: `-${totalWidth}px 0px` }
        ],
        { duration: 1200, iterations: Infinity, easing: `steps(${currentIdleFrames}, end)` }
      );
      animRef.current = anim;
    } else {
      // run 애니메이션 (variant별 프레임 수 적용)
      const totalWidth = scaledAnimWidth(currentRunFrames);
      const anim = skeletonRef.current.animate(
        [
          { backgroundPosition: '0px 0px' },
          { backgroundPosition: `-${totalWidth}px 0px` }
        ],
        { duration: 1500, iterations: Infinity, easing: `steps(${currentRunFrames}, end)` }
      );
      anim.playbackRate = speedRef.current * currentPet.speedFactor * userSpeedFactor;
      animRef.current = anim;
    }
    return () => {
      if (animRef.current) { animRef.current.cancel(); animRef.current = null; }
    };
  // runVariant/selectedPetId/petScale/petUserSpeed 변경 시 애니메이션 재시작
  }, [isHovered, isHurt, isRightClickAnim, runVariant, selectedPetId, petScale, petUserSpeed]);

  // 좌클릭: hurt 애니메이션 1회 재생 (idle 상태에서만)
  const handleClick = () => {
    if (isHurt || isRightClickAnim || !isHovered || currentPet.hurtFrames === 0) return;
    if (hurtTimerRef.current) clearTimeout(hurtTimerRef.current);
    setIsHurt(true);
    const frames = currentPet.hurtFrames;
    const hurtDuration = frames * 200;
    if (skeletonRef.current) {
      const totalWidth = scaledAnimWidth(frames);
      skeletonRef.current.animate(
        [
          { backgroundPosition: '0px 0px' },
          { backgroundPosition: `-${totalWidth}px 0px` }
        ],
        { duration: hurtDuration, iterations: 1, easing: `steps(${frames}, end)`, fill: 'forwards' }
      );
    }
    hurtTimerRef.current = setTimeout(() => { setIsHurt(false); }, hurtDuration);
  };

  // 우클릭: variant 순환 또는 1회 재생 애니메이션 (idle 상태에서만)
  const handleContextMenu = (e: React.MouseEvent) => {
    e.preventDefault();
    if (!isHovered || isHurt || isRightClickAnim) return;

    // rightClickImage가 있는 펫: 1회 재생 애니메이션
    if (currentPet.rightClickImage && currentPet.rightClickFrames) {
      setIsRightClickAnim(true);
      const frames = currentPet.rightClickFrames;
      const duration = frames * 200;
      if (skeletonRef.current) {
        const totalWidth = scaledAnimWidth(frames);
        skeletonRef.current.animate(
          [
            { backgroundPosition: '0px 0px' },
            { backgroundPosition: `-${totalWidth}px 0px` }
          ],
          { duration, iterations: 1, easing: `steps(${frames}, end)`, fill: 'forwards' }
        );
      }
      // hasVariants도 있으면 variant 순환도 동시 적용
      if (currentPet.hasVariants) {
        setRunVariant((prev) => (prev + 1) % currentPet.runImages.length);
      }
      if (rightClickTimerRef.current) clearTimeout(rightClickTimerRef.current);
      rightClickTimerRef.current = setTimeout(() => { setIsRightClickAnim(false); }, duration);
      return;
    }

    // hasVariants 펫: variant 순환
    if (!currentPet.hasVariants) return;
    setRunVariant((prev) => (prev + 1) % currentPet.runImages.length);
  };

  // 클래스 결정: hurt > rightClick > idle > run
  let skeletonClass = 'skeleton';
  if (isHurt) skeletonClass += ' hurt';
  else if (isRightClickAnim) skeletonClass += ' idle';
  else if (isHovered) skeletonClass += ' idle';

  // 펫별 프레임 크기 및 하단 여백 보정 (기본 displayScale × 사용자 petScale 적용)
  const { frameWidth, frameHeight, bottomPadding, displayScale } = currentPet;
  const finalScale = displayScale * (petScale / 100);
  const scaledW = Math.round(frameWidth * finalScale);
  const scaledH = Math.round(frameHeight * finalScale);
  const scaledMargin = bottomPadding > 0 ? `-${Math.round(bottomPadding * finalScale)}px` : undefined;

  // 펫 스프라이트 실제 폭을 Rust 백엔드에 공유 (경계 판정용)
  useEffect(() => {
    invoke("update_pet_visual_w", { width: scaledW });
  }, [scaledW]);

  // 말풍선 위치: 캐릭터 머리 위 (bubbleHeight로 추가 높이 조절)
  const bubbleBottom = scaledH - (bottomPadding > 0 ? Math.round(bottomPadding * finalScale) : 0) + bubbleHeight;

  // 폰트 색상별 text-shadow 캐싱 (색상 변경 시에만 재계산)
  const monitoringShadow = useMemo(() => getTextShadow(monitoringFontColor), [monitoringFontColor]);
  const alarmShadow = useMemo(() => getTextShadow(alarmFontColor), [alarmFontColor]);

  // 상태별 스타일 객체 캐싱 (의존 값 변경 시에만 재계산, 내부에서 직접 계산하여 의존성 완전 명시)
  const petStyle = useMemo(() => {
    const runImg = currentPet.runImages[safeVariant];
    const idleImg = currentPet.idleImages[safeVariant];
    const hurtImg = currentPet.hurtImage;

    const makeBgSize = (frames: number) => `${Math.round(frameWidth * frames * finalScale)}px ${scaledH}px`;
    const baseSize = { width: `${scaledW}px`, height: `${scaledH}px`, marginBottom: scaledMargin };
    const flipStyle = currentPet.flipX ? { transform: 'scaleX(-1)' } : {};
    const opacityStyle = petOpacity < 100 ? { opacity: petOpacity / 100 } : {};
    const isDefaultColor = hue === 0 && saturation === 100 && brightness === 100;

    // 상태별 이미지/사이즈 결정
    let bgImage: string;
    let bgSize: string;
    if (isHurt) {
      bgImage = hurtImg;
      bgSize = makeBgSize(currentPet.hurtFrames);
    } else if (isRightClickAnim && currentPet.rightClickImage && currentPet.rightClickFrames) {
      bgImage = currentPet.rightClickImage;
      bgSize = makeBgSize(currentPet.rightClickFrames);
    } else if (isHovered) {
      bgImage = idleImg;
      bgSize = makeBgSize(currentIdleFrames);
    } else {
      bgImage = runImg;
      bgSize = makeBgSize(currentRunFrames);
    }

    const base = { backgroundImage: `url('${bgImage}')`, backgroundSize: bgSize, ...baseSize, ...flipStyle, ...opacityStyle };

    // 초기화 상태(기본값)이면 필터 없이 원본 이미지 그대로 표시
    if (isDefaultColor) return base;

    // 색상 필터 체인
    const adjustedHue = hue - 50;
    const filter = `grayscale(1) sepia(1) hue-rotate(${adjustedHue}deg) saturate(${saturation * 4}%) brightness(${brightness / 100})`;
    return { ...base, filter };
  }, [currentPet, isHurt, isHovered, isRightClickAnim, safeVariant, currentRunFrames, currentIdleFrames, frameWidth, finalScale, scaledH, scaledW, scaledMargin, petOpacity, hue, saturation, brightness]);

  // Phase별 말풍선 표시 허용 여부 (Phase 1,3=좌우, Phase 2=상단, Phase 0=항상)
  const phaseBubbleAllowed = movePhase === 0 || (movePhase === 2 ? bubbleTop : bubbleSide);

  // 말풍선 표시 여부 판정 (이동 중, not hovered, not hurt)
  const showNotification = bubbleEnabled && phaseBubbleAllowed && displayConfig.showNotificationText && activeNotifications.length > 0;
  // 알림 우선 모드: 알림 표시 중에는 모니터링 문구 숨김
  const showMonitoring = bubbleEnabled && phaseBubbleAllowed && displayConfig.showMonitoringText && petMessage
    && !(displayConfig.notificationPriority && showNotification);

  // 왼쪽 이동 모드 여부 (mode 2=기본 왼쪽, mode 3=등반 왼쪽, mode 4=랜덤 동적)
  const isLeftMode = (moveMode === 4 || moveMode === 5) ? randomDirLeft : moveMode >= 2;

  return (
    <div className="pet-container" style={(() => {
      // 왼쪽 모드: scaleX(-1)로 펫 좌우 반전 + phase별 회전
      const flip = isLeftMode ? 'scaleX(-1) ' : '';
      if (movePhase === 1) return { transform: `${flip}rotate(-90deg)` };
      if (movePhase === 2) return { transform: `${flip}rotate(180deg)` };
      if (movePhase === 3) return { transform: `${flip}rotate(90deg)` };
      if (isLeftMode) return { transform: 'scaleX(-1)' };
      return undefined;
    })()}>
      {/* 이동(run) 중 말풍선: 알림/모니터링 문구 표시 */}
      {!isHovered && !isHurt && (showNotification || showMonitoring) && (
        <div className="speech-bubble message-bubble" style={{
          bottom: `${bubbleBottom}px`,
          fontSize: `${fontSize}px`,
          ...(fontFamily ? { fontFamily } : {}),
          ...(() => {
            // phase 2(180° 회전) + 왼쪽 모드(scaleX 반전) 보정
            const parts: string[] = ['translateX(-50%)'];
            if (movePhase === 2) parts.push('rotate(180deg)');
            if (isLeftMode) parts.push('scaleX(-1)');
            if (parts.length > 1) return { transform: parts.join(' ') };
            return {};
          })(),
        }}>
          {showNotification && activeNotifications.map(n => (
            <div key={n.id} className="pet-message notification-text" style={{ color: alarmFontColor, textShadow: alarmShadow }}>{n.message}</div>
          ))}
          {showMonitoring && (
            <div className="pet-message" style={{ color: monitoringFontColor, textShadow: monitoringShadow }}>{petMessage}</div>
          )}
        </div>
      )}
      {/* hover(idle) 중: 모니터링 수치 표시 */}
      {isHovered && !isHurt && (monitorConfig.cpu || monitorConfig.memory || monitorConfig.network || monitorConfig.battery) && (
        <div className="speech-bubble" style={{
          bottom: `${bubbleBottom}px`,
          fontSize: `${fontSize}px`,
          color: monitoringFontColor,
          textShadow: monitoringShadow,
          ...(fontFamily ? { fontFamily } : {}),
          ...(() => {
            const parts: string[] = ['translateX(-50%)'];
            if (movePhase === 2) parts.push('rotate(180deg)');
            if (isLeftMode) parts.push('scaleX(-1)');
            if (parts.length > 1) return { transform: parts.join(' ') };
            return {};
          })(),
        }}>
          <div className="stat-row">
            {monitorConfig.cpu && <span>🖥 CPU {isTestMode ? `${testCpuValue}%` : `${cpuUsage}%`}</span>}
            {monitorConfig.memory && <span>💾 MEM {memUsage}%</span>}
            {monitorConfig.network && <span>🌐 NET {formatBytes(networkDown)}/s</span>}
            {monitorConfig.battery && batteryPercent >= 0 && <span>🔋 BAT {batteryPercent}%{batteryCharging ? ' ⚡' : ''}</span>}
          </div>
        </div>
      )}
      <div style={{ position: 'relative' }}>
        {/* 충전 아이콘: 캐릭터 왼쪽 중앙에 표시 (absolute로 캐릭터 위치 영향 없음) */}
        {monitorConfig.showChargingIcon && batteryCharging && batteryPercent >= 0 && (() => {
          // 아이콘 크기 비율: large=50%, medium=40%, small=30%
          const sizeRatio = monitorConfig.chargingIconSize === 'large' ? 0.5 : monitorConfig.chargingIconSize === 'small' ? 0.3 : 0.4;
          // 아이콘 거리: 음수=가까이, 양수=멀리
          const distance = (monitorConfig.chargingIconDistance ?? 0);
          return (
            <div className="charging-icon" style={{
              position: 'absolute',
              right: '100%',
              top: `${Math.round((scaledH - (bottomPadding > 0 ? Math.round(bottomPadding * finalScale) : 0)) / 2)}px`,
              transform: 'translateY(-50%)',
              fontSize: `${Math.max(12, Math.round(scaledH * sizeRatio))}px`,
              lineHeight: '1',
              pointerEvents: 'none',
              marginRight: `${1 + distance}px`,
            }}>⚡</div>
          );
        })()}
        <div
          ref={skeletonRef}
          className={skeletonClass}
          style={{ ...petStyle, ...(mouseEnabled ? {} : { pointerEvents: 'none' as const }) }}
          role="button"
          tabIndex={0}
          aria-label="Skeleton Pet"
          onClick={handleClick}
          onContextMenu={handleContextMenu}
          onKeyDown={(e) => e.key === 'Enter' && handleClick()}
          onMouseEnter={() => {
            if (!mouseEnabled) return;
            setIsHovered(true);
            invoke("set_hover", { hovered: true });
          }}
          onMouseLeave={() => {
            if (!mouseEnabled) return;
            setIsHovered(false);
            invoke("set_hover", { hovered: false });
          }}
        ></div>
      </div>
    </div>
  );
}
