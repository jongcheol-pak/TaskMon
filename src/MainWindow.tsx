import { useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import { type Language, resolveLanguage, createT } from "./locales";
import {
  type MonitorConfig,
  type PetMessage,
  type Alarm,
  type DisplayConfig,
  DEFAULT_DISPLAY_CONFIG,
  RUN_IMAGES,
  IDLE_IMAGES,
  generateAlarmId,
  checkAlarms,
  evaluateMessages,
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
  const [monitorConfig, setMonitorConfig] = useState<MonitorConfig>(() =>
    safeParse('monitorConfig', { cpu: true, memory: true, network: false, battery: false })
  );
  const [runVariant, setRunVariant] = useState<number>(() => {
    const saved = localStorage.getItem('petRunVariant');
    return saved !== null ? Number(saved) : 0;
  }); // 0=맨손, 1=검, 2=검+방패

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

  // 설정: 말풍선 사용 여부
  const [bubbleEnabled, setBubbleEnabled] = useState<boolean>(() => {
    const saved = localStorage.getItem('bubbleEnabled');
    return saved !== null ? saved === 'true' : true;
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

  // 폰트/언어 설정
  const [language, setLanguage] = useState<Language>(() => {
    const saved = localStorage.getItem('language');
    return (saved as Language) || 'system';
  });
  const [fontSize, setFontSize] = useState<number>(() => {
    const saved = localStorage.getItem('fontSize');
    return saved ? Number(saved) : 12;
  });
  const [fontFamily, setFontFamily] = useState<string>(() => {
    return localStorage.getItem('fontFamily') || '';
  });

  // 번역 함수
  const resolvedLang = resolveLanguage(language);
  const t = createT(resolvedLang);
  // t는 hover 말풍선에서는 사용하지 않지만, 향후 확장을 위해 유지
  void t;

  const skeletonRef = useRef<HTMLDivElement>(null);
  const animRef = useRef<Animation | null>(null);
  const hurtTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

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
  }, []);

  // 메시지 평가: 모니터링 값 변경 시마다 매칭 목록 갱신
  useEffect(() => {
    if (petMessages.length === 0) {
      matchedMessagesRef.current = [];
      setPetMessage(null);
      return;
    }

    const effectiveCpu = isTestMode ? testCpuValue : cpuUsage;
    const matched = evaluateMessages(petMessages, effectiveCpu, memUsage, batteryPercent, networkDown, networkUp, monitorConfig);
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
  }, [cpuUsage, memUsage, batteryPercent, networkDown, networkUp, petMessages, isTestMode, testCpuValue, monitorConfig, showAllMessages]);

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
          case 'daily':
            // 이미 지난 시간이면 오늘 발화 완료로 표시
            if (alarm.targetTime && new Date().toTimeString().slice(0, 5) >= alarm.targetTime) {
              return { ...alarm, firedToday: today };
            }
            return alarm;
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

      const { firedMessages, updatedAlarms } = checkAlarms(currentAlarms);
      if (firedMessages.length > 0) {
        localStorage.setItem('alarms', JSON.stringify(updatedAlarms));
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
        animRef.current.playbackRate = newSpeed;
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
    });
    const unlistenMonitorConfig = listen<MonitorConfig>("monitor-config-update", (event) => {
      setMonitorConfig(event.payload);
      localStorage.setItem('monitorConfig', JSON.stringify(event.payload));
    });
    const unlistenBubble = listen<boolean>("bubble-enabled-update", (event) => {
      setBubbleEnabled(event.payload);
      localStorage.setItem('bubbleEnabled', String(event.payload));
    });
    // 알림 목록 동기화 (설정 윈도우 → 메인 윈도우)
    // 설정 윈도우에서 보내는 알람에는 발화 상태(lastFiredHour 등)가 없으므로,
    // 기존 localStorage의 발화 상태를 병합하여 중복 발화를 방지
    const unlistenAlarms = listen<Alarm[]>("alarm-list-update", (event) => {
      const now = Date.now();
      const today = new Date().toISOString().slice(0, 10);
      const currentHHmm = new Date().toTimeString().slice(0, 5);
      const currentHour = new Date().getHours();
      const currentMin = new Date().getMinutes();

      const existing = safeParse<Alarm[]>('alarms', []);
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
        // 신규 알림: 이미 지난 시간이면 발화 완료 처리 (가져오기 등으로 추가 시 즉시 발화 방지)
        if (!alarm.enabled) return alarm;
        switch (alarm.type) {
          case 'interval':
            return { ...alarm, lastFiredAt: now, startedAt: alarm.startedAt || now };
          case 'absolute':
            if (alarm.targetTime && currentHHmm >= alarm.targetTime) {
              return { ...alarm, firedToday: today, enabled: false };
            }
            return alarm;
          case 'daily':
            if (alarm.targetTime && currentHHmm >= alarm.targetTime) {
              return { ...alarm, firedToday: today };
            }
            return alarm;
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
      localStorage.setItem('alarms', JSON.stringify(merged));
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
    const unlistenAppSettings = listen<{language: Language, fontSize: number, fontFamily: string}>("app-settings-update", (event) => {
      setLanguage(event.payload.language);
      setFontSize(event.payload.fontSize);
      setFontFamily(event.payload.fontFamily);
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

    return () => {
      unlisten.then((f) => f());
      unlistenMem.then((f) => f());
      unlistenTestMode.then((f) => f());
      unlistenColor.then((f) => f());
      unlistenNetwork.then((f) => f());
      unlistenBattery.then((f) => f());
      unlistenMonitorConfig.then((f) => f());
      unlistenBubble.then((f) => f());
      unlistenAlarms.then((f) => f());
      unlistenDisplayConfig.then((f) => f());
      unlistenMessages.then((f) => f());
      unlistenAppSettings.then((f) => f());
      unlistenMsgRotate.then((f) => f());
    };
  }, []);

  // runVariant 및 색상 필터 변경 시 localStorage에 저장 (500ms 디바운스로 디스크 I/O 최소화)
  useEffect(() => {
    const timer = setTimeout(() => {
      localStorage.setItem('petRunVariant', String(runVariant));
      localStorage.setItem('petHue', String(hue));
      localStorage.setItem('petSaturation', String(saturation));
      localStorage.setItem('petBrightness', String(brightness));
      localStorage.setItem('petOpacity', String(petOpacity));
      localStorage.setItem('monitorConfig', JSON.stringify(monitorConfig));
      localStorage.setItem('bubbleEnabled', String(bubbleEnabled));
    }, 500);
    return () => clearTimeout(timer);
  }, [runVariant, hue, saturation, brightness, petOpacity, monitorConfig, bubbleEnabled]);

  // 알림 표시 타이머 정리
  useEffect(() => {
    return () => {
      if (notificationTimerRef.current) clearTimeout(notificationTimerRef.current);
    };
  }, []);

  // 스프라이트 애니메이션
  useEffect(() => {
    // hurt 또는 hover 중이면 run 애니메이션 중단
    if (!isHovered && !isHurt && skeletonRef.current) {
      const anim = skeletonRef.current.animate(
        [
          { backgroundPosition: '0px 0px' },
          { backgroundPosition: '-384px 0px' }
        ],
        { duration: 1500, iterations: Infinity, easing: 'steps(6, end)' }
      );
      anim.playbackRate = speedRef.current;
      animRef.current = anim;
    }
    return () => {
      if (animRef.current) { animRef.current.cancel(); animRef.current = null; }
    };
  // runVariant 변경 시 애니메이션 재시작해야 새 이미지에 스프라이트가 적용됨
  }, [isHovered, isHurt, runVariant]);

  // 좌클릭: hurt 애니메이션 1회 재생
  const handleClick = () => {
    if (isHurt) return;
    if (hurtTimerRef.current) clearTimeout(hurtTimerRef.current);
    setIsHurt(true);
    hurtTimerRef.current = setTimeout(() => { setIsHurt(false); }, 400);
  };

  // 우클릭: 달리기 이미지 순환 (0→1→2→0)
  const handleContextMenu = (e: React.MouseEvent) => {
    e.preventDefault();
    setRunVariant((prev) => (prev + 1) % RUN_IMAGES.length);
  };

  // 클래스 결정: hurt > idle > run
  let skeletonClass = 'skeleton';
  if (isHurt) skeletonClass += ' hurt';
  else if (isHovered) skeletonClass += ' idle';

  // run 또는 idle 상태일 때 inline style로 이미지 및 색상 필터 적용
  const isDefaultColor = hue === 0 && saturation === 100 && brightness === 100;
  const opacityStyle = petOpacity < 100 ? { opacity: petOpacity / 100 } : {};

  const petStyle = (() => {
    // 초기화 상태(기본값)이면 필터 없이 원본 이미지 그대로 표시
    if (isDefaultColor) {
      if (isHurt) return { ...opacityStyle };
      if (isHovered) return { backgroundImage: `url('${IDLE_IMAGES[runVariant]}')`, ...opacityStyle };
      return { backgroundImage: `url('${RUN_IMAGES[runVariant]}')`, ...opacityStyle };
    }

    // 색상 필터 체인:
    // 1. grayscale(1): 원본 색 제거
    // 2. sepia(1): 채색 가능한 베이스 입히기
    // 3. hue-rotate(hue - 50): sepia 기본색(~50도)을 상쇄하여 피커 색상과 일치
    // 4. saturate: 채도 강하게 적용
    // 5. brightness: 최종 밝기 조정
    const adjustedHue = hue - 50;
    const filter = `grayscale(1) sepia(1) hue-rotate(${adjustedHue}deg) saturate(${saturation * 4}%) brightness(${brightness / 100})`;

    if (isHurt) return { filter, ...opacityStyle };
    if (isHovered) return { backgroundImage: `url('${IDLE_IMAGES[runVariant]}')`, filter, ...opacityStyle };
    return { backgroundImage: `url('${RUN_IMAGES[runVariant]}')`, filter, ...opacityStyle };
  })();

  // 말풍선 표시 여부 판정 (이동 중, not hovered, not hurt)
  const showNotification = bubbleEnabled && displayConfig.showNotificationText && activeNotifications.length > 0;
  // 알림 우선 모드: 알림 표시 중에는 모니터링 문구 숨김
  const showMonitoring = bubbleEnabled && displayConfig.showMonitoringText && petMessage
    && !(displayConfig.notificationPriority && showNotification);

  return (
    <div className="pet-container">
      {/* 이동(run) 중 말풍선: 알림/모니터링 문구 표시 */}
      {!isHovered && !isHurt && (showNotification || showMonitoring) && (
        <div className="speech-bubble message-bubble" style={{
          fontSize: `${fontSize}px`,
          ...(fontFamily ? { fontFamily } : {}),
        }}>
          {showNotification && activeNotifications.map(n => (
            <div key={n.id} className="pet-message notification-text">{n.message}</div>
          ))}
          {showMonitoring && (
            <div className="pet-message">{petMessage}</div>
          )}
        </div>
      )}
      {/* hover(idle) 중: 모니터링 수치 표시 */}
      {isHovered && !isHurt && (monitorConfig.cpu || monitorConfig.memory || monitorConfig.network || monitorConfig.battery) && (
        <div className="speech-bubble" style={{
          fontSize: `${fontSize}px`,
          ...(fontFamily ? { fontFamily } : {}),
        }}>
          <div className="stat-row">
            {monitorConfig.cpu && <span>🖥 CPU {isTestMode ? `${testCpuValue}%` : `${cpuUsage}%`}</span>}
            {monitorConfig.memory && <span>💾 MEM {memUsage}%</span>}
            {monitorConfig.network && <span>🌐 NET {formatBytes(networkDown)}/s</span>}
            {monitorConfig.battery && batteryPercent >= 0 && <span>🔋 BAT {batteryPercent}%{batteryCharging ? ' ⚡' : ''}</span>}
          </div>
        </div>
      )}
      <div
        ref={skeletonRef}
        className={skeletonClass}
        style={petStyle}
        role="button"
        tabIndex={0}
        aria-label="Skeleton Pet"
        onClick={handleClick}
        onContextMenu={handleContextMenu}
        onKeyDown={(e) => e.key === 'Enter' && handleClick()}
        onMouseEnter={() => {
          setIsHovered(true);
          invoke("set_hover", { hovered: true });
        }}
        onMouseLeave={() => {
          setIsHovered(false);
          invoke("set_hover", { hovered: false });
        }}
      ></div>
    </div>
  );
}
