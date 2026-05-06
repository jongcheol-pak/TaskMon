import { useEffect, useRef, useState, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { getVersion } from "@tauri-apps/api/app";
import { save, open, ask } from "@tauri-apps/plugin-dialog";
import { readTextFile, writeTextFile } from "@tauri-apps/plugin-fs";
import { openUrl } from "@tauri-apps/plugin-opener";
import { type Language, resolveLanguage, createT } from "./locales";
import { EMOJI_CATEGORIES } from "./emojis";
import {
  type MonitorConfig,
  type MessageCondition,
  type PetMessage,
  type AlarmType,
  type Alarm,
  type NotificationMode,
  type DisplayConfig,
  type MailConfig,
  type MailConfigLoadResponse,
  type MailErrorPayload,
  DEFAULT_DISPLAY_CONFIG,
  DEFAULT_MONITOR_CONFIG,
  DEFAULT_MAIL_CONFIG,
  FONT_SIZE_OPTIONS,
  FONT_FAMILY_OPTIONS,
  FONT_COLOR_PALETTE,
  MOVE_MODES,
  PET_TYPES,
  RANDOM_PET_ID,
  generateAlarmId,
  isValidAlarm,
  isValidPetMessage,
  safeParse,
  formatBytes,
} from "./types";
import { listen } from "@tauri-apps/api/event";

// 업데이트 확인 상태 — 모듈 스코프에 두어 컴포넌트 매 렌더에서 재선언되지 않도록 한다.
type UpdateStatus = 'idle' | 'checking' | 'available' | 'latest' | 'error' | 'downloading';
type UpdateInfoDto = {
  latest_version: string;
  tag: string;
  download_url: string;
  asset_name: string;
  // 릴리즈 노트에서 추출된 SHA256 체크섬 (없으면 null)
  sha256: string | null;
};

// PetMessage.target → 배지 i18n 키 매핑.
// 같은 매핑이 select option/badge/평가 분기 등 여러 곳에서 반복되지 않도록 단일 테이블로 관리.
const MSG_TARGET_BADGE_KEY: Record<string, string> = {
  cpu: 'msg.badgeCpu',
  gpu: 'msg.badgeGpu',
  memory: 'msg.badgeMemory',
  battery: 'msg.badgeBattery',
  network_down: 'msg.badgeNetDown',
  network_up: 'msg.badgeNetUp',
};

// 분류된 메일 오류를 사용자 가시 텍스트로 변환.
// switch가 MailErrorPayload union 전체를 cover하도록 하여
// 새 kind가 추가되면 컴파일 에러로 누락을 차단한다.
type TranslateFn = (key: string, params?: Record<string, string | number>) => string;
function formatMailError(err: MailErrorPayload, t: TranslateFn): string {
  switch (err.kind) {
    case 'Auth':
      return t('mail.errorAuth');
    case 'Network':
      return `${t('mail.errorNetworkPrefix')}: ${err.message}`;
    case 'Protocol':
      return `${t('mail.errorProtocolPrefix')}: ${err.message}`;
  }
}

export default function SettingsWindow() {
  // 설정 탭 상태
  const isDebug = import.meta.env.DEV;
  const [settingsTab, setSettingsTab] = useState<string>(isDebug ? "test" : "color");
  // 앱 버전
  const [appVersion, setAppVersion] = useState<string>("");
  useEffect(() => { getVersion().then(setAppVersion); }, []);

  // 업데이트 확인 상태 (정보 탭 진입 시 자동 조회)
  const [updateStatus, setUpdateStatus] = useState<UpdateStatus>('idle');
  const [updateInfo, setUpdateInfo] = useState<UpdateInfoDto | null>(null);
  const updateCheckedRef = useRef(false);

  // 테스트 모드 상태
  const [isTestMode, setIsTestMode] = useState(false);
  const [testCpuValue, setTestCpuValue] = useState(50);

  // 펫 선택 상태
  const [selectedPetId, setSelectedPetId] = useState<string>(() => {
    return localStorage.getItem('selectedPetId') || RANDOM_PET_ID;
  });
  // 펫 크기 상태 (펫별 개별 저장, 0~200%, 기본 100%)
  const [petScale, setPetScale] = useState<number>(() => {
    const id = localStorage.getItem('selectedPetId') || RANDOM_PET_ID;
    const saved = localStorage.getItem(`petScale_${id}`);
    return saved ? Number(saved) : 100;
  });
  // 펫 속도 상태 (펫별 개별 저장, 0~200%, 기본 100%)
  const [petSpeed, setPetSpeed] = useState<number>(() => {
    const id = localStorage.getItem('selectedPetId') || RANDOM_PET_ID;
    const saved = localStorage.getItem(`petSpeed_${id}`);
    return saved ? Number(saved) : 100;
  });
  // 펫 높이 오프셋 (펫별 개별 저장, -10~10, 기본 0)
  const [petHeight, setPetHeight] = useState<number>(() => {
    const id = localStorage.getItem('selectedPetId') || RANDOM_PET_ID;
    const saved = localStorage.getItem(`petHeight_${id}`);
    return saved ? Number(saved) : 0;
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

  // 모니터링 설정
  const [monitorConfig, setMonitorConfig] = useState<MonitorConfig>(() => {
    return { ...DEFAULT_MONITOR_CONFIG, ...safeParse('monitorConfig', DEFAULT_MONITOR_CONFIG) };
  });

  // 모니터링 프리뷰용 기본값 (이벤트로 업데이트하지 않음)
  const cpuUsage = 0;
  const gpuUsage = 0;
  const memUsage = 0;
  const networkDown = 0;
  // 배터리 유무: MainWindow가 저장한 batteryPercent로 판정 (-1 또는 미저장 = 배터리 없음)
  const storedBatteryPercent = (() => {
    const saved = localStorage.getItem('batteryPercent');
    return saved !== null ? Number(saved) : -1;
  })();
  const hasBattery = storedBatteryPercent >= 0;

  // 이동 모드 상태
  const [moveMode, setMoveMode] = useState<number>(() => {
    const saved = localStorage.getItem('moveMode');
    return saved !== null ? Number(saved) : 0;
  });

  // 설정: 폴링 간격 (초)
  const [pollingInput, setPollingInput] = useState<string>(() => {
    const saved = localStorage.getItem('pollingInterval');
    return saved !== null ? saved : "1";
  });

  // 메시지 시스템 상태
  const [petMessages, setPetMessages] = useState<PetMessage[]>(() =>
    safeParse<PetMessage[]>('petMessages', [])
  );

  // 조건에 맞는 모든 메시지 순환 표시 설정
  const [showAllMessages, setShowAllMessages] = useState<boolean>(() => {
    const saved = localStorage.getItem('showAllMessages');
    return saved === 'true';
  });
  const [rotateInterval, setRotateInterval] = useState<number>(() => {
    const saved = localStorage.getItem('rotateInterval');
    return saved ? Number(saved) : 10;
  });
  // 순환 표시 간격 입력 폼 상태
  const [rotateIntervalInput, setRotateIntervalInput] = useState<string>(() => {
    const saved = localStorage.getItem('rotateInterval');
    return saved || '10';
  });

  // 설정: 말풍선 사용 여부
  const [bubbleHeight, setBubbleHeight] = useState<number>(() => {
    const saved = localStorage.getItem('bubbleHeight');
    return saved ? Number(saved) : 0;
  });
  // 설정: 마우스 사용 여부
  const [mouseEnabled, setMouseEnabled] = useState<boolean>(() => {
    const saved = localStorage.getItem('mouseEnabled');
    return saved !== null ? saved === 'true' : true;
  });

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

  // 알림 시스템 상태
  const [alarms, setAlarms] = useState<Alarm[]>(() =>
    safeParse<Alarm[]>('alarms', [])
  );
  const [displayConfig, setDisplayConfig] = useState<DisplayConfig>(() => ({
    ...DEFAULT_DISPLAY_CONFIG,
    ...safeParse<Partial<DisplayConfig>>('displayConfig', {}),
  }));

  // 알림 설정 폼 상태
  const [alarmFormType, setAlarmFormType] = useState<AlarmType>('interval');
  const [alarmFormMessage, setAlarmFormMessage] = useState('');
  const [alarmFormIntervalMin, setAlarmFormIntervalMin] = useState('50');
  const [alarmFormTime, setAlarmFormTime] = useState('09:00');
  const [alarmFormDelayHours, setAlarmFormDelayHours] = useState('0');
  const [alarmFormDelayMinutes, setAlarmFormDelayMinutes] = useState('30');
  const [alarmFormHourlyMinute, setAlarmFormHourlyMinute] = useState('0');
  const [alarmFormDurationInput, setAlarmFormDurationInput] = useState<string>(() => {
    const config = safeParse<Partial<DisplayConfig>>('displayConfig', {});
    return String(config.notificationDuration ?? DEFAULT_DISPLAY_CONFIG.notificationDuration);
  });
  // 저장된 displayConfig에서 mailDuration 입력 상태 초기화
  useEffect(() => {
    const config = safeParse<Partial<DisplayConfig>>('displayConfig', {});
    setMailDurationInput(String(config.mailDuration ?? DEFAULT_DISPLAY_CONFIG.mailDuration));
  }, []);

  // 메일 알림 설정 상태
  const [mailConfig, setMailConfig] = useState<MailConfig>(DEFAULT_MAIL_CONFIG);
  const [mailHasPassword, setMailHasPassword] = useState<boolean>(false);
  const [mailError, setMailError] = useState<MailErrorPayload | null>(null);
  const [mailTestStatus, setMailTestStatus] = useState<'idle' | 'success'>('idle');
  const mailTestTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const [mailDurationInput, setMailDurationInput] = useState<string>('60');
  // 백엔드에서 저장된 메일 설정 로드 (1회)
  useEffect(() => {
    invoke<MailConfigLoadResponse>('mail_load_config').then((res) => {
      setMailConfig({
        ...DEFAULT_MAIL_CONFIG,
        enabled: res.config.enabled,
        account_name: res.config.account_name,
        host: res.config.host,
        port: res.config.port,
        use_tls: res.config.use_tls,
        user_id: res.config.user_id,
        poll_minutes: res.config.poll_minutes,
        password: '',
      });
      setMailHasPassword(res.has_password);
    }).catch(() => {});
  }, []);
  // 메일 오류 이벤트 listen
  useEffect(() => {
    const unlistenP = listen<{ error: MailErrorPayload | null }>('mail-status', (e) => {
      setMailError(e.payload?.error ?? null);
    });
    return () => { unlistenP.then(fn => fn()); };
  }, []);

  // 모니터링 메시지 설정 폼 상태
  const [msgFormTarget, setMsgFormTarget] = useState<string>('cpu');
  const [msgFormCondition, setMsgFormCondition] = useState<MessageCondition>('greater_than');
  const [msgFormValue, setMsgFormValue] = useState('80');
  const [msgFormPriority, setMsgFormPriority] = useState('5');
  const [msgFormText, setMsgFormText] = useState('');

  // 이모지 피커 상태 ('msg' | 'alarm' | null)
  const [emojiPickerTarget, setEmojiPickerTarget] = useState<'msg' | 'alarm' | null>(null);
  const [emojiCategory, setEmojiCategory] = useState(0);

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
  const [monitoringFontColor, setMonitoringFontColor] = useState<string>(() => {
    return localStorage.getItem('monitoringFontColor') || '#FFFFFF';
  });
  const [alarmFontColor, setAlarmFontColor] = useState<string>(() => {
    return localStorage.getItem('alarmFontColor') || '#FFFFFF';
  });

  // 자동 실행 상태 (레지스트리 조회)
  const [autoStart, setAutoStart] = useState(false);

  // 타이머 상태
  const [timerMinutes, setTimerMinutes] = useState<number>(() => {
    const saved = localStorage.getItem('timerMinutes');
    return saved ? Number(saved) : 5;
  });
  const [timerRunning, setTimerRunning] = useState(false);
  const [timerRemaining, setTimerRemaining] = useState<number>(0);
  const timerIntervalRef = useRef<ReturnType<typeof setInterval> | null>(null);
  const [timerFontSize, setTimerFontSize] = useState<number>(() => {
    const saved = localStorage.getItem('timerFontSize');
    return saved ? Number(saved) : 14;
  });

  // 번역 함수
  const resolvedLang = resolveLanguage(language);
  const t = createT(resolvedLang);

  // Refs: 색상 업데이트 throttle
  const colorRafRef = useRef<number | null>(null);
  const pendingColorRef = useRef<{hue: number, saturation: number, brightness: number, opacity: number} | null>(null);

  // 자동 실행 상태 초기화
  useEffect(() => {
    invoke<boolean>("get_auto_start").then(setAutoStart).catch(() => {});
  }, []);

  // 색상 필터 변경 시 localStorage에 저장 (500ms 디바운스로 디스크 I/O 최소화)
  useEffect(() => {
    const timer = setTimeout(() => {
      localStorage.setItem('petHue', String(hue));
      localStorage.setItem('petSaturation', String(saturation));
      localStorage.setItem('petBrightness', String(brightness));
      localStorage.setItem('petOpacity', String(petOpacity));
      localStorage.setItem('monitorConfig', JSON.stringify(monitorConfig));
      localStorage.setItem('mouseEnabled', String(mouseEnabled));
      localStorage.setItem('bubbleEnabled', String(bubbleEnabled));
      localStorage.setItem('bubbleSide', String(bubbleSide));
      localStorage.setItem('bubbleTop', String(bubbleTop));
    }, 500);
    return () => clearTimeout(timer);
  }, [hue, saturation, brightness, petOpacity, monitorConfig, mouseEnabled, bubbleEnabled, bubbleSide, bubbleTop]);

  // 컴포넌트 언마운트 시 pending rAF 정리
  useEffect(() => {
    return () => {
      if (colorRafRef.current !== null) {
        cancelAnimationFrame(colorRafRef.current);
      }
    };
  }, []);

  // 색상 업데이트 IPC를 rAF로 throttle (드래그 중 초당 60+ 회 → 프레임당 1회)
  const scheduleColorUpdate = useCallback((h: number, s: number, b: number, o: number) => {
    pendingColorRef.current = { hue: h, saturation: s, brightness: b, opacity: o };
    if (colorRafRef.current === null) {
      colorRafRef.current = requestAnimationFrame(() => {
        if (pendingColorRef.current) {
          invoke("update_pet_color", pendingColorRef.current);
          pendingColorRef.current = null;
        }
        colorRafRef.current = null;
      });
    }
  }, []);

  // 테스트 모드 토글
  const handleTestModeToggle = (e: React.ChangeEvent<HTMLInputElement>) => {
    const enabled = e.target.checked;
    setIsTestMode(enabled);
    invoke("set_test_cpu", { usage: enabled ? testCpuValue : -1 });
  };

  // 테스트 CPU 값 변경
  const handleTestCpuChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    const val = Number.parseInt(e.target.value, 10);
    setTestCpuValue(val);
    if (isTestMode) {
      invoke("set_test_cpu", { usage: val });
    }
  };

  // 모니터링 설정 변경 핸들러
  const handleMonitorToggle = (key: keyof MonitorConfig) => {
    const updated = { ...monitorConfig, [key]: !monitorConfig[key] };
    setMonitorConfig(updated);
    invoke("update_monitor_config", { config: updated });
  };

  // 충전 아이콘 설정값 변경 (콤보박스용)
  const handleMonitorConfigChange = (key: keyof MonitorConfig, value: string | number) => {
    const updated = { ...monitorConfig, [key]: value };
    setMonitorConfig(updated);
    invoke("update_monitor_config", { config: updated });
  };

  // 알림 저장 및 메인 윈도우 동기화
  const saveAndSyncAlarms = useCallback((newAlarms: Alarm[]) => {
    setAlarms(newAlarms);
    localStorage.setItem('alarms', JSON.stringify(newAlarms));
    invoke('update_alarm_list', { alarms: newAlarms });
  }, []);

  // 타이머 시작
  const handleTimerStart = useCallback(() => {
    const endAt = Date.now() + timerMinutes * 60 * 1000;
    setTimerRemaining(timerMinutes * 60);
    setTimerRunning(true);
    localStorage.setItem('timerEndAt', String(endAt));
    localStorage.setItem('timerRunning', 'true');
    invoke('update_timer_state', { running: true, endAt });

    // 기존 인터벌 정리
    if (timerIntervalRef.current) clearInterval(timerIntervalRef.current);
    timerIntervalRef.current = setInterval(() => {
      const remaining = Math.max(0, Math.ceil((endAt - Date.now()) / 1000));
      setTimerRemaining(remaining);
      if (remaining <= 0) {
        // 타이머 종료
        if (timerIntervalRef.current) clearInterval(timerIntervalRef.current);
        timerIntervalRef.current = null;
        setTimerRunning(false);
        localStorage.setItem('timerRunning', 'false');
        invoke('update_timer_state', { running: false, endAt: 0 });
      }
    }, 500);
  }, [timerMinutes]);

  // 타이머 중지
  const handleTimerStop = useCallback(() => {
    if (timerIntervalRef.current) clearInterval(timerIntervalRef.current);
    timerIntervalRef.current = null;
    setTimerRunning(false);
    setTimerRemaining(0);
    localStorage.setItem('timerRunning', 'false');
    invoke('update_timer_state', { running: false, endAt: 0 });
  }, []);

  // 타이머 인터벌 정리
  useEffect(() => {
    return () => {
      if (timerIntervalRef.current) clearInterval(timerIntervalRef.current);
    };
  }, []);

  // 정보 탭 진입 시 한 번만 GitHub Releases에서 최신 버전 확인
  useEffect(() => {
    if (settingsTab !== 'about') return;
    if (updateCheckedRef.current) return;
    updateCheckedRef.current = true;

    setUpdateStatus('checking');
    invoke<UpdateInfoDto | null>('check_update')
      .then((info) => {
        if (info) {
          setUpdateInfo(info);
          setUpdateStatus('available');
        } else {
          setUpdateStatus('latest');
        }
      })
      .catch(() => {
        // 네트워크 오류·API 차단 등으로 확인 실패 시에도 최신 버전으로 표시
        setUpdateStatus('latest');
      });
  }, [settingsTab]);

  // 업데이트 다운로드 및 인스톨러 실행 (백엔드에서 SHA256 검증 후 앱 종료까지 처리).
  // 체크섬이 릴리즈 노트에 없으면 사용자에게 검증 생략 여부 확인 후 진행한다.
  const handleUpdateClick = useCallback(async () => {
    if (!updateInfo || updateStatus === 'downloading') return;

    // 체크섬이 없는 경우 사용자 확인 (취소 시 다운로드 중단)
    if (!updateInfo.sha256) {
      const proceed = await ask(t('about.updateNoChecksumPrompt'), {
        title: 'TaskMon',
        kind: 'warning',
      });
      if (!proceed) return;
    }

    setUpdateStatus('downloading');
    try {
      await invoke('download_and_install_update', {
        url: updateInfo.download_url,
        fileName: updateInfo.asset_name,
        expectedSha256: updateInfo.sha256,
      });
    } catch (e) {
      console.error('업데이트 다운로드 실패:', e);
      setUpdateStatus('error');
    }
  }, [updateInfo, updateStatus, t]);

  // 표시 설정 저장 및 메인 윈도우 동기화
  const saveAndSyncDisplayConfig = useCallback((newConfig: DisplayConfig) => {
    setDisplayConfig(newConfig);
    localStorage.setItem('displayConfig', JSON.stringify(newConfig));
    invoke('update_display_config', {
      showMonitoring: newConfig.showMonitoringText,
      showNotification: newConfig.showNotificationText,
      notificationPriority: newConfig.notificationPriority,
      notificationMode: newConfig.notificationMode,
      notificationDuration: newConfig.notificationDuration,
      mailDuration: newConfig.mailDuration,
    });
  }, []);

  // 모니터링 메시지 저장 및 메인 윈도우 동기화
  const saveAndSyncMessages = useCallback((newMessages: PetMessage[]) => {
    setPetMessages(newMessages);
    localStorage.setItem('petMessages', JSON.stringify(newMessages));
    invoke('update_messages', { messages: newMessages });
  }, []);

  // 폰트/언어 설정 저장 및 메인 윈도우 동기화
  const saveAndSyncAppSettings = useCallback((lang: Language, size: number, family: string, monColor: string, almColor: string) => {
    setLanguage(lang);
    setFontSize(size);
    setFontFamily(family);
    setMonitoringFontColor(monColor);
    setAlarmFontColor(almColor);
    localStorage.setItem('language', lang);
    localStorage.setItem('fontSize', String(size));
    localStorage.setItem('fontFamily', family);
    localStorage.setItem('monitoringFontColor', monColor);
    localStorage.setItem('alarmFontColor', almColor);
    invoke('update_app_settings', { language: lang, fontSize: size, fontFamily: family, monitoringFontColor: monColor, alarmFontColor: almColor });
  }, []);

  // 모니터링 메시지 추가
  const handleAddMessage = () => {
    if (!msgFormText.trim()) return;
    const value = parseFloat(msgFormValue) || 0;
    // 대상, 조건, 값이 동일한 항목이 이미 있으면 추가하지 않음
    const duplicate = petMessages.some(
      m => m.target === msgFormTarget && m.condition === msgFormCondition && m.value === value
    );
    if (duplicate) return;
    const newMsg: PetMessage = {
      target: msgFormTarget,
      condition: msgFormCondition,
      value,
      priority: parseInt(msgFormPriority) || 5,
      text: msgFormText.trim(),
    };
    saveAndSyncMessages([...petMessages, newMsg]);
    setMsgFormText('');
  };

  // 모니터링 메시지 삭제 (index 기반, id 없음)
  const handleRemoveMessage = (index: number) => {
    saveAndSyncMessages(petMessages.filter((_, i) => i !== index));
  };

  // 알림 추가
  const handleAddAlarm = () => {
    if (!alarmFormMessage.trim()) return;

    const newAlarm: Alarm = {
      id: generateAlarmId(),
      type: alarmFormType,
      enabled: true,
      message: alarmFormMessage.trim(),
    };

    switch (alarmFormType) {
      case 'interval':
        newAlarm.intervalMinutes = Math.max(1, parseInt(alarmFormIntervalMin) || 50);
        newAlarm.startedAt = Date.now();
        break;
      case 'absolute': {
        newAlarm.targetTime = alarmFormTime;
        // 이미 지난 시간이면 오늘 발화 완료 처리
        if (new Date().toTimeString().slice(0, 5) >= alarmFormTime) {
          newAlarm.firedToday = new Date().toISOString().slice(0, 10);
          newAlarm.enabled = false;
        }
        break;
      }
      case 'daily': {
        newAlarm.targetTime = alarmFormTime;
        // 이미 지난 시간이면 오늘 발화 완료 처리 (내일부터 동작)
        if (new Date().toTimeString().slice(0, 5) >= alarmFormTime) {
          newAlarm.firedToday = new Date().toISOString().slice(0, 10);
        }
        break;
      }
      case 'relative': {
        const hours = Math.max(0, parseInt(alarmFormDelayHours) || 0);
        const mins = Math.max(0, parseInt(alarmFormDelayMinutes) || 0);
        const totalMin = hours * 60 + mins;
        if (totalMin <= 0) return;
        newAlarm.delayMinutes = totalMin;
        newAlarm.absoluteTarget = Date.now() + totalMin * 60000;
        break;
      }
      case 'hourly': {
        const minute = Math.min(59, Math.max(0, parseInt(alarmFormHourlyMinute) || 0));
        newAlarm.hourlyMinute = minute;
        // 이번 시간의 설정 분이 이미 지났으면 발화 완료 처리
        if (new Date().getMinutes() >= minute) {
          newAlarm.lastFiredHour = new Date().getHours();
        }
        break;
      }
    }

    saveAndSyncAlarms([...alarms, newAlarm]);
    setAlarmFormMessage('');
  };

  // 알림 삭제
  const handleRemoveAlarm = (id: string) => {
    saveAndSyncAlarms(alarms.filter(a => a.id !== id));
  };

  // 알림 활성화/비활성화 토글
  const handleToggleAlarm = (id: string) => {
    const updated = alarms.map(a => {
      if (a.id !== id) return a;
      const newEnabled = !a.enabled;
      // interval 타입: 활성화 시 시작 시점 갱신
      if (a.type === 'interval' && newEnabled) {
        return { ...a, enabled: true, startedAt: Date.now(), lastFiredAt: undefined };
      }
      return { ...a, enabled: newEnabled };
    });
    saveAndSyncAlarms(updated);
  };

  // 알림 목록 파일 저장
  const handleExportAlarms = useCallback(async () => {
    try {
      const filePath = await save({
        filters: [{ name: 'JSON', extensions: ['json'] }],
        defaultPath: 'alarms.json',
      });
      if (!filePath) return;
      await writeTextFile(filePath, JSON.stringify(alarms, null, 2));
    } catch (e) {
      console.error('알림 저장 실패:', e);
    }
  }, [alarms]);

  // 알림 목록 파일에서 가져오기
  const handleImportAlarms = useCallback(async () => {
    try {
      const filePath = await open({
        filters: [{ name: 'JSON', extensions: ['json'] }],
        multiple: false,
        directory: false,
      });
      if (!filePath) return;

      const content = await readTextFile(filePath as string);
      let parsed: unknown;
      try { parsed = JSON.parse(content); } catch { return; }

      if (!Array.isArray(parsed) || parsed.length === 0) return;
      const valid = parsed.filter(isValidAlarm);
      if (valid.length === 0) return;

      // 기존 목록이 있으면 확인 팝업
      if (alarms.length > 0) {
        const confirmed = await ask(t('export.confirmDelete'), {
          title: 'TaskMon',
          kind: 'warning',
        });
        if (!confirmed) return;
      }
      saveAndSyncAlarms(valid);
    } catch (e) {
      console.error('알림 가져오기 실패:', e);
    }
  }, [alarms, t, saveAndSyncAlarms]);

  // 모니터링 메시지 목록 파일 저장
  const handleExportMessages = useCallback(async () => {
    try {
      const filePath = await save({
        filters: [{ name: 'JSON', extensions: ['json'] }],
        defaultPath: 'messages.json',
      });
      if (!filePath) return;
      await writeTextFile(filePath, JSON.stringify(petMessages, null, 2));
    } catch (e) {
      console.error('메시지 저장 실패:', e);
    }
  }, [petMessages]);

  // 모니터링 메시지 목록 파일에서 가져오기
  const handleImportMessages = useCallback(async () => {
    try {
      const filePath = await open({
        filters: [{ name: 'JSON', extensions: ['json'] }],
        multiple: false,
        directory: false,
      });
      if (!filePath) return;

      const content = await readTextFile(filePath as string);
      let parsed: unknown;
      try { parsed = JSON.parse(content); } catch { return; }

      if (!Array.isArray(parsed) || parsed.length === 0) return;
      const valid = parsed.filter(isValidPetMessage);
      if (valid.length === 0) return;

      // 기존 목록이 있으면 확인 팝업
      if (petMessages.length > 0) {
        const confirmed = await ask(t('export.confirmDelete'), {
          title: 'TaskMon',
          kind: 'warning',
        });
        if (!confirmed) return;
      }
      saveAndSyncMessages(valid);
    } catch (e) {
      console.error('메시지 가져오기 실패:', e);
    }
  }, [petMessages, t, saveAndSyncMessages]);

  // 이모지 선택 시 대상 입력란에 삽입
  const handleEmojiSelect = (emoji: string) => {
    if (emojiPickerTarget === 'msg') {
      setMsgFormText(prev => (prev + emoji).slice(0, 50));
    } else if (emojiPickerTarget === 'alarm') {
      setAlarmFormMessage(prev => (prev + emoji).slice(0, 50));
    }
  };

  // 이모지 피커 컴포넌트 렌더
  const renderEmojiPicker = () => (
    <div className="emoji-picker">
      <div className="emoji-tabs">
        {EMOJI_CATEGORIES.map((cat, i) => (
          <button
            key={i}
            className={`emoji-tab ${emojiCategory === i ? 'active' : ''}`}
            onClick={() => setEmojiCategory(i)}
          >
            {cat.icon}
          </button>
        ))}
      </div>
      <div className="emoji-grid">
        {EMOJI_CATEGORIES[emojiCategory].emojis.map((emoji, i) => (
          <button key={i} className="emoji-item" onClick={() => handleEmojiSelect(emoji)}>
            {emoji}
          </button>
        ))}
      </div>
    </div>
  );

  // 알림 상세 정보 텍스트
  const getAlarmDetail = (alarm: Alarm): string => {
    switch (alarm.type) {
      case 'interval': return t('alarm.detail.everyNMin', { n: alarm.intervalMinutes ?? 0 });
      case 'absolute': return `${alarm.targetTime}`;
      case 'daily': return t('alarm.detail.daily', { time: alarm.targetTime ?? '' });
      case 'relative': {
        if (alarm.delayMinutes && alarm.delayMinutes >= 60) {
          const h = Math.floor(alarm.delayMinutes / 60);
          const m = alarm.delayMinutes % 60;
          return m > 0 ? t('alarm.detail.hoursMinAfter', { h, m }) : t('alarm.detail.hoursAfter', { h });
        }
        return t('alarm.detail.minAfter', { n: alarm.delayMinutes ?? 0 });
      }
      case 'hourly':
        return t('alarm.detail.hourlyAt', { n: alarm.hourlyMinute ?? 0 });
    }
    return '';
  };

  // 알림 타입 배지 라벨
  const getAlarmBadge = (type: AlarmType): string => {
    return t(`alarm.badge.${type}`);
  };

  return (
    <div className="settings-layout">
      <nav className="settings-sidebar">
        <h2 className="sidebar-title">{t('sidebar.title')}</h2>
        <ul className="sidebar-menu">
          {isDebug && <li>
            <button className={`sidebar-item ${settingsTab === "test" ? "active" : ""}`} onClick={() => setSettingsTab("test")}>
              {t('sidebar.testMode')}
            </button>
          </li>}
          <li>
            <button className={`sidebar-item ${settingsTab === "color" ? "active" : ""}`} onClick={() => setSettingsTab("color")}>
              {t('sidebar.petColor')}
            </button>
          </li>
          <li>
            <button className={`sidebar-item ${settingsTab === "movement" ? "active" : ""}`} onClick={() => setSettingsTab("movement")}>
              {t('sidebar.movement')}
            </button>
          </li>
          <li>
            <button className={`sidebar-item ${settingsTab === "monitoring" ? "active" : ""}`} onClick={() => setSettingsTab("monitoring")}>
              {t('sidebar.monitoring')}
            </button>
          </li>
          <li>
            <button className={`sidebar-item ${settingsTab === "messages" ? "active" : ""}`} onClick={() => setSettingsTab("messages")}>
              {t('sidebar.messages')}
            </button>
          </li>
          <li>
            <button className={`sidebar-item ${settingsTab === "alarm" ? "active" : ""}`} onClick={() => setSettingsTab("alarm")}>
              {t('sidebar.alarm')}
            </button>
          </li>
          <li>
            <button className={`sidebar-item ${settingsTab === "mail" ? "active" : ""}`} onClick={() => setSettingsTab("mail")}>
              {t('sidebar.mail')}
            </button>
          </li>
          <li>
            <button className={`sidebar-item ${settingsTab === "timer" ? "active" : ""}`} onClick={() => setSettingsTab("timer")}>
              {t('sidebar.timer')}
            </button>
          </li>
          <li>
            <button className={`sidebar-item ${settingsTab === "font" ? "active" : ""}`} onClick={() => setSettingsTab("font")}>
              {t('sidebar.font')}
            </button>
          </li>
          <li>
            <button className={`sidebar-item ${settingsTab === "general" ? "active" : ""}`} onClick={() => setSettingsTab("general")}>
              {t('sidebar.general')}
            </button>
          </li>
          <li>
            <button className={`sidebar-item ${settingsTab === "about" ? "active" : ""}`} onClick={() => setSettingsTab("about")}>
              {t('sidebar.about')}
            </button>
          </li>
        </ul>
        {appVersion && <div className="sidebar-version">v{appVersion}</div>}
      </nav>
      <div className="settings-content-wrapper">
      <main className="settings-content">
        {isDebug && settingsTab === "test" && (
          <div className="settings-section">
            <h3>{t('test.title')}</h3>
            <p className="description">{t('test.description')}</p>
            <div className="test-controls-vertical">
              <label className="test-toggle">
                <input
                  type="checkbox"
                  checked={isTestMode}
                  onChange={handleTestModeToggle}
                />
                <span>{t('test.enable')}</span>
              </label>
              <div className={isTestMode ? "slider-group" : "slider-group disabled"}>
                <div className="slider-header">
                  <span>{t('test.virtualCpu')}</span>
                  <span className="value-badge">{testCpuValue}%</span>
                </div>
                <input
                  type="range"
                  min="0"
                  max="100"
                  value={testCpuValue}
                  onChange={handleTestCpuChange}
                  disabled={!isTestMode}
                  className="test-slider-large"
                />
              </div>
            </div>
          </div>
        )}

        {settingsTab === "movement" && (
          <div className="settings-section">
            <h3>{t('movement.title')}</h3>
            <p className="description">{t('movement.description')}</p>
            <div className="move-mode-list">
              {MOVE_MODES.map(mode => (
                <label key={mode.id} className={`move-mode-item${moveMode === mode.id ? ' active' : ''}`}>
                  <input
                    type="radio"
                    name="moveMode"
                    value={mode.id}
                    checked={moveMode === mode.id}
                    onChange={() => {
                      setMoveMode(mode.id);
                      localStorage.setItem('moveMode', String(mode.id));
                      invoke('update_move_mode', { mode: mode.id });
                    }}
                  />
                  <div className="move-mode-text">
                    <span className="move-mode-name">{t(mode.nameKey)}</span>
                    <span className="move-mode-desc">{t(mode.descriptionKey)}</span>
                  </div>
                </label>
              ))}
            </div>
          </div>
        )}

        {settingsTab === "color" && (
          <>
          <div className="settings-section">
            {/* 펫 선택 */}
            <h3>{t('pet.label')}</h3>
            <div className="pet-select-row">
              <select
                className="alarm-select"
                value={selectedPetId}
                onChange={(e) => {
                  const id = e.target.value;
                  setSelectedPetId(id);
                  localStorage.setItem('selectedPetId', id);
                  const pet = PET_TYPES.find(p => p.id === id);
                  // 펫별 저장된 크기/속도 로드
                  const savedScale = localStorage.getItem(`petScale_${id}`);
                  const scale = savedScale ? Number(savedScale) : 100;
                  setPetScale(scale);
                  const savedSpeed = localStorage.getItem(`petSpeed_${id}`);
                  const speed = savedSpeed ? Number(savedSpeed) : 100;
                  setPetSpeed(speed);
                  const savedHeight = localStorage.getItem(`petHeight_${id}`);
                  const height = savedHeight ? Number(savedHeight) : 0;
                  setPetHeight(height);
                  invoke("update_pet_height", { petId: id, offset: height });
                  invoke("update_pet_type", { petId: id, speedFactor: pet?.speedFactor ?? 1.0, userSpeed: speed / 100 });
                }}
              >
                <option key={RANDOM_PET_ID} value={RANDOM_PET_ID}>{t('pet.random')}</option>
                {[...PET_TYPES].sort((a, b) => t(`pet.${a.id}`).localeCompare(t(`pet.${b.id}`))).map((pet) => (
                  <option key={pet.id} value={pet.id}>{t(`pet.${pet.id}`)}</option>
                ))}
              </select>
            </div>
            <p className="setting-description">{t('pet.description')}</p>
            <div className="setting-item" style={{ marginTop: '8px' }}>
              <span className="setting-label">{t('pet.scale')}</span>
              <input
                type="range"
                min={0}
                max={300}
                value={petScale}
                onChange={(e) => {
                  const scale = Number(e.target.value);
                  setPetScale(scale);
                  localStorage.setItem(`petScale_${selectedPetId}`, String(scale));
                  invoke("update_pet_scale", { petId: selectedPetId, scale });
                }}
                style={{ flex: 1 }}
              />
              <span style={{ minWidth: '40px', textAlign: 'right', fontSize: '13px', color: '#ccc' }}>{petScale}%</span>
            </div>
            <div className="setting-item" style={{ marginTop: '8px' }}>
              <span className="setting-label">{t('pet.speed')}</span>
              <input
                type="range"
                min={0}
                max={200}
                value={petSpeed}
                onChange={(e) => {
                  const speed = Number(e.target.value);
                  setPetSpeed(speed);
                  localStorage.setItem(`petSpeed_${selectedPetId}`, String(speed));
                  const pet = PET_TYPES.find(p => p.id === selectedPetId);
                  invoke("update_pet_speed", { petId: selectedPetId, speedFactor: pet?.speedFactor ?? 1.0, userSpeed: speed / 100 });
                }}
                style={{ flex: 1 }}
              />
              <span style={{ minWidth: '40px', textAlign: 'right', fontSize: '13px', color: '#ccc' }}>{petSpeed}%</span>
            </div>
            <div className="setting-item" style={{ marginTop: '8px' }}>
              <span className="setting-label">{t('pet.height')}</span>
              <input
                type="range"
                min={-10}
                max={10}
                value={petHeight}
                onChange={(e) => {
                  const height = Number(e.target.value);
                  setPetHeight(height);
                  localStorage.setItem(`petHeight_${selectedPetId}`, String(height));
                  invoke("update_pet_height", { petId: selectedPetId, offset: height });
                }}
                style={{ flex: 1 }}
              />
              <span style={{ minWidth: '40px', textAlign: 'right', fontSize: '13px', color: '#ccc' }}>{petHeight}</span>
            </div>
          </div>

          <div className="settings-section">
            <h3>{t('color.title')}</h3>
            <p className="description">{t('color.description')}</p>

            <div className="color-controls">
              {/* 2D 컬러 피커 */}
              <div
                className="color-picker-2d"
                onMouseDown={(e) => {
                  // rect를 mousedown 시점에 한 번만 캡처 (mousemove에서 e.currentTarget 참조 버그 방지)
                  const rect = e.currentTarget.getBoundingClientRect();

                  const calcAndApply = (clientX: number, clientY: number) => {
                    const x = Math.max(0, Math.min(clientX - rect.left, rect.width));
                    const y = Math.max(0, Math.min(clientY - rect.top, rect.height));
                    const newHue = Math.round((x / rect.width) * 360);
                    const newSaturation = Math.round(100 - (y / rect.height) * 100);
                    setHue(newHue);
                    setSaturation(newSaturation);
                    scheduleColorUpdate(newHue, newSaturation, brightness, petOpacity);
                  };

                  const handleMove = (moveEvent: MouseEvent) => {
                    calcAndApply(moveEvent.clientX, moveEvent.clientY);
                  };

                  const handleUp = () => {
                    window.removeEventListener('mousemove', handleMove);
                    window.removeEventListener('mouseup', handleUp);
                  };

                  window.addEventListener('mousemove', handleMove);
                  window.addEventListener('mouseup', handleUp);

                  // 첫 클릭 지점 반영
                  calcAndApply(e.clientX, e.clientY);
                }}
              >
                <div
                  className="color-pointer"
                  style={{
                    left: `${(hue / 360) * 100}%`,
                    top: `${(1 - (saturation / 100)) * 100}%`
                  }}
                ></div>
              </div>

              <div className="slider-group">
                <div className="slider-header">
                  <span>{t('color.brightness')}</span>
                  <span className="value-badge">{brightness}%</span>
                </div>
                <input
                  type="range" min="0" max="200" value={brightness}
                  onChange={(e) => {
                    const val = Number(e.target.value);
                    setBrightness(val);
                    scheduleColorUpdate(hue, saturation, val, petOpacity);
                  }}
                  className="test-slider-large"
                />
              </div>

              <div className="slider-group">
                <div className="slider-header">
                  <span>{t('color.opacity')}</span>
                  <span className="value-badge">{petOpacity}%</span>
                </div>
                <input
                  type="range" min="0" max="100" value={petOpacity}
                  onChange={(e) => {
                    const val = Number(e.target.value);
                    setPetOpacity(val);
                    scheduleColorUpdate(hue, saturation, brightness, val);
                  }}
                  className="test-slider-large"
                />
              </div>

              <button
                className="reset-button"
                onClick={() => {
                  setHue(0);
                  setSaturation(100);
                  setBrightness(100);
                  setPetOpacity(100);
                  invoke("update_pet_color", { hue: 0, saturation: 100, brightness: 100, opacity: 100 }); // 초기화는 즉시 반영
                }}
              >
                {t('color.reset')}
              </button>
            </div>
          </div>
          </>
        )}

        {settingsTab === "monitoring" && (
          <div className="settings-section">
            <h3>{t('monitor.title')}</h3>
            <p className="description">{t('monitor.description')}</p>
            <div className="monitor-checklist">
              <label className="monitor-item">
                <input type="checkbox" checked={monitorConfig.cpu} onChange={() => handleMonitorToggle('cpu')} />
                <span className="monitor-label">{t('monitor.cpu')}</span>
                <span className="monitor-preview">{cpuUsage}%</span>
              </label>
              <label className="monitor-item">
                <input type="checkbox" checked={monitorConfig.gpu} onChange={() => handleMonitorToggle('gpu')} />
                <span className="monitor-label">{t('monitor.gpu')}</span>
                <span className="monitor-preview">{gpuUsage}%</span>
              </label>
              <label className="monitor-item">
                <input type="checkbox" checked={monitorConfig.memory} onChange={() => handleMonitorToggle('memory')} />
                <span className="monitor-label">{t('monitor.memory')}</span>
                <span className="monitor-preview">{memUsage}%</span>
              </label>
              <label className="monitor-item">
                <input type="checkbox" checked={monitorConfig.network} onChange={() => handleMonitorToggle('network')} />
                <span className="monitor-label">{t('monitor.network')}</span>
                <span className="monitor-preview">{formatBytes(networkDown)}/s</span>
              </label>
              <label className="monitor-item" style={{ opacity: hasBattery ? 1 : 0.4 }}>
                <input type="checkbox" checked={hasBattery && monitorConfig.battery} disabled={!hasBattery} onChange={() => handleMonitorToggle('battery')} />
                <span className="monitor-label">{t('monitor.battery')}</span>
                <span className="monitor-preview">{hasBattery ? `${storedBatteryPercent}%` : t('monitor.batteryNone')}</span>
              </label>
              <label className="monitor-item" style={{ paddingLeft: '24px', opacity: hasBattery ? 1 : 0.4 }}>
                <input type="checkbox" checked={hasBattery && monitorConfig.showChargingIcon} disabled={!hasBattery} onChange={() => handleMonitorToggle('showChargingIcon')} />
                <span className="monitor-label">{t('monitor.chargingIcon')}</span>
              </label>
              {/* 충전 아이콘 크기 */}
              <div className="monitor-item" style={{ paddingLeft: '48px', opacity: hasBattery && monitorConfig.showChargingIcon ? 1 : 0.4, display: 'flex', alignItems: 'center', gap: '8px' }}>
                <span className="monitor-label">{t('monitor.chargingIconSize')}</span>
                <select
                  value={monitorConfig.chargingIconSize || 'medium'}
                  disabled={!hasBattery || !monitorConfig.showChargingIcon}
                  onChange={(e) => handleMonitorConfigChange('chargingIconSize', e.target.value)}
                  className="polling-input"
                  style={{ width: '80px' }}
                >
                  <option value="large">{t('monitor.chargingIconSizeLarge')}</option>
                  <option value="medium">{t('monitor.chargingIconSizeMedium')}</option>
                  <option value="small">{t('monitor.chargingIconSizeSmall')}</option>
                  <option value="xsmall">{t('monitor.chargingIconSizeXSmall')}</option>
                </select>
              </div>
              {/* 충전 아이콘 거리 */}
              <div className="monitor-item" style={{ paddingLeft: '48px', opacity: hasBattery && monitorConfig.showChargingIcon ? 1 : 0.4, display: 'flex', alignItems: 'center', gap: '8px' }}>
                <span className="monitor-label">{t('monitor.chargingIconDistance')}</span>
                <select
                  value={monitorConfig.chargingIconDistance ?? 0}
                  disabled={!hasBattery || !monitorConfig.showChargingIcon}
                  onChange={(e) => handleMonitorConfigChange('chargingIconDistance', Number(e.target.value))}
                  className="polling-input"
                  style={{ width: '60px' }}
                >
                  {Array.from({ length: 61 }, (_, i) => i - 50).map(v => (
                    <option key={v} value={v}>{v}</option>
                  ))}
                </select>
              </div>
            </div>
            <div className="setting-item" style={{ marginTop: '12px' }}>
              <span className="setting-label">{t('general.pollingLabel')}</span>
              <div className="polling-control">
                <input
                  type="text"
                  inputMode="numeric"
                  pattern="[0-9]*"
                  value={pollingInput}
                  onChange={(e) => {
                    const v = e.target.value.replace(/[^0-9]/g, '');
                    setPollingInput(v);
                  }}
                  className="polling-input"
                />
                <button
                  className="polling-apply"
                  onClick={() => {
                    const val = parseInt(pollingInput, 10);
                    const seconds = isNaN(val) || val === 0 ? 1 : val;
                    setPollingInput(String(seconds));
                    localStorage.setItem('pollingInterval', String(seconds));
                    invoke("set_polling_interval", { seconds });
                  }}
                >
                  {t('general.apply')}
                </button>
              </div>
            </div>
          </div>
        )}

        {settingsTab === "messages" && (
          <>
            {/* 모니터링 문구 표시 설정 */}
            <div className="settings-section">
              <label className="monitor-item">
                <input
                  type="checkbox"
                  checked={displayConfig.showMonitoringText}
                  onChange={() => {
                    const updated = { ...displayConfig, showMonitoringText: !displayConfig.showMonitoringText };
                    saveAndSyncDisplayConfig(updated);
                  }}
                />
                <span className="monitor-label">{t('alarm.showMonitoring')}</span>
              </label>
            </div>

            {/* 모니터링 메시지 추가 */}
            <div className="settings-section">
              <h3>{t('msg.addTitle')}</h3>
              <p className="description">{t('msg.addDescription')}</p>
              <div className="alarm-form">
                <div className="alarm-form-row">
                  <span className="setting-label">{t('msg.target')}</span>
                  <select className="alarm-select" value={msgFormTarget} onChange={(e) => setMsgFormTarget(e.target.value)}>
                    <option value="cpu">{t('msg.targetCpu')}</option>
                    <option value="gpu">{t('msg.targetGpu')}</option>
                    <option value="memory">{t('msg.targetMemory')}</option>
                    <option value="battery">{t('msg.targetBattery')}</option>
                    <option value="network_down">{t('msg.targetNetDown')}</option>
                    <option value="network_up">{t('msg.targetNetUp')}</option>
                  </select>
                </div>
                <div className="alarm-form-row">
                  <span className="setting-label">{t('msg.condition')}</span>
                  <select className="alarm-select" value={msgFormCondition} onChange={(e) => setMsgFormCondition(e.target.value as MessageCondition)}>
                    <option value="greater_than">{t('msg.condGt')}</option>
                    <option value="greater_equal">{t('msg.condGe')}</option>
                    <option value="less_than">{t('msg.condLt')}</option>
                    <option value="less_equal">{t('msg.condLe')}</option>
                    <option value="equal">{t('msg.condEq')}</option>
                  </select>
                </div>
                <div className="alarm-form-row">
                  <span className="setting-label">{t('msg.value')}</span>
                  <input
                    type="text"
                    inputMode="numeric"
                    value={msgFormValue}
                    onChange={(e) => setMsgFormValue(e.target.value.replace(/[^0-9.]/g, ''))}
                    className="polling-input"
                  />
                </div>
                <div className="alarm-form-row">
                  <span className="setting-label">{t('msg.priority')}</span>
                  <input
                    type="text"
                    inputMode="numeric"
                    value={msgFormPriority}
                    onChange={(e) => setMsgFormPriority(e.target.value.replace(/[^0-9]/g, ''))}
                    className="polling-input"
                  />
                  <span style={{ color: '#888', fontSize: '12px' }}>{t('msg.priorityHelper')}</span>
                </div>
                <div className="alarm-form-row">
                  <span className="setting-label">{t('msg.message')}</span>
                  <div className="emoji-input-wrapper">
                    <input
                      type="text"
                      value={msgFormText}
                      onChange={(e) => setMsgFormText(e.target.value)}
                      maxLength={50}
                      placeholder={t('msg.messagePlaceholder')}
                      className="alarm-message-input"
                    />
                    <button
                      className="emoji-toggle-btn"
                      onClick={() => setEmojiPickerTarget(emojiPickerTarget === 'msg' ? null : 'msg')}
                    >
                      😀
                    </button>
                  </div>
                </div>
                {emojiPickerTarget === 'msg' && renderEmojiPicker()}
                <button className="polling-apply alarm-add-btn" onClick={handleAddMessage}>
                  {t('msg.add')}
                </button>
              </div>
            </div>

            {/* 메시지 순환 표시 설정 */}
            <div className="settings-section">
              <h3>{t('msg.rotateTitle')}</h3>
              <div className="general-settings">
                <label className="monitor-item">
                  <input
                    type="checkbox"
                    checked={showAllMessages}
                    onChange={(e) => {
                      setShowAllMessages(e.target.checked);
                      localStorage.setItem('showAllMessages', String(e.target.checked));
                      invoke("update_msg_rotate", { showAll: e.target.checked, interval: rotateInterval });
                    }}
                  />
                  <span className="monitor-label">{t('msg.showAll')}</span>
                </label>
                <div className={`setting-item ${!showAllMessages ? 'disabled' : ''}`} style={{ opacity: showAllMessages ? 1 : 0.4 }}>
                  <span className="setting-label">{t('msg.rotateIntervalLabel')}</span>
                  <div className="polling-control">
                    <input
                      type="text"
                      inputMode="numeric"
                      value={rotateIntervalInput}
                      onChange={(e) => setRotateIntervalInput(e.target.value.replace(/[^0-9]/g, ''))}
                      className="polling-input"
                      disabled={!showAllMessages}
                    />
                    <button
                      className="polling-apply"
                      disabled={!showAllMessages}
                      onClick={() => {
                        const val = Math.max(1, parseInt(rotateIntervalInput) || 10);
                        setRotateInterval(val);
                        setRotateIntervalInput(String(val));
                        localStorage.setItem('rotateInterval', String(val));
                        invoke("update_msg_rotate", { showAll: showAllMessages, interval: val });
                      }}
                    >
                      {t('general.apply')}
                    </button>
                  </div>
                </div>
              </div>
            </div>

            {/* 등록된 모니터링 메시지 목록 */}
            <div className="settings-section">
              <h3>{t('msg.listTitle')}</h3>
              {petMessages.length === 0 ? (
                <p className="description">{t('msg.empty')}</p>
              ) : (
                <div className="alarm-list">
                  {petMessages.map((msg, index) => (
                    <div key={index} className="alarm-item">
                      <div className="alarm-item-info">
                        <span className="alarm-type-badge">
                          {t(MSG_TARGET_BADGE_KEY[msg.target] ?? 'msg.badgeNetUp')}
                        </span>
                        <span className="alarm-detail">
                          {msg.condition === 'greater_than' ? '>' :
                           msg.condition === 'greater_equal' ? '>=' :
                           msg.condition === 'less_than' ? '<' :
                           msg.condition === 'less_equal' ? '<=' : '='}{msg.value}
                        </span>
                        <span className="alarm-detail" style={{ color: '#666' }}>P{msg.priority}</span>
                        <span className="alarm-message-preview">{msg.text}</span>
                      </div>
                      <div className="alarm-item-actions">
                        <button className="alarm-delete-btn" onClick={() => handleRemoveMessage(index)}>
                          ✕
                        </button>
                      </div>
                    </div>
                  ))}
                </div>
              )}
              <div className="export-import-buttons">
                <button
                  className="polling-apply"
                  disabled={petMessages.length === 0}
                  onClick={handleExportMessages}
                >
                  {t('export.save')}
                </button>
                <button
                  className="polling-apply"
                  onClick={handleImportMessages}
                >
                  {t('export.import')}
                </button>
              </div>
            </div>
          </>
        )}

        {settingsTab === "alarm" && (
          <>
            {/* 표시 설정 */}
            <div className="settings-section">
              <h3>{t('alarm.displayTitle')}</h3>
              <p className="description">{t('alarm.displayDescription')}</p>
              <div className="monitor-checklist">
                <label className="monitor-item">
                  <input
                    type="checkbox"
                    checked={displayConfig.showNotificationText}
                    onChange={() => {
                      const updated = { ...displayConfig, showNotificationText: !displayConfig.showNotificationText };
                      saveAndSyncDisplayConfig(updated);
                    }}
                  />
                  <span className="monitor-label">{t('alarm.showNotification')}</span>
                </label>
                <label className="monitor-item" style={{ marginLeft: '16px' }}>
                  <input
                    type="checkbox"
                    checked={displayConfig.notificationPriority}
                    disabled={!displayConfig.showNotificationText}
                    onChange={() => {
                      const updated = { ...displayConfig, notificationPriority: !displayConfig.notificationPriority };
                      saveAndSyncDisplayConfig(updated);
                    }}
                  />
                  <span className="monitor-label">{t('alarm.notificationPriority')}</span>
                </label>
                <p className="description" style={{ marginLeft: '16px', marginTop: '2px' }}>{t('alarm.notificationPriorityDesc')}</p>
                {/* 알림 중복 표시 모드 */}
                <div className="notification-mode-group">
                  <span className="setting-label">{t('alarm.modeLabel')}</span>
                  <div className="notification-mode-radios">
                    <label className="notification-mode-radio">
                      <input
                        type="radio"
                        name="notificationMode"
                        value="all"
                        checked={displayConfig.notificationMode === 'all'}
                        onChange={() => {
                          const updated = { ...displayConfig, notificationMode: 'all' as NotificationMode };
                          saveAndSyncDisplayConfig(updated);
                        }}
                      />
                      <span>{t('alarm.modeAll')}</span>
                    </label>
                    <label className="notification-mode-radio">
                      <input
                        type="radio"
                        name="notificationMode"
                        value="first"
                        checked={displayConfig.notificationMode === 'first'}
                        onChange={() => {
                          const updated = { ...displayConfig, notificationMode: 'first' as NotificationMode };
                          saveAndSyncDisplayConfig(updated);
                        }}
                      />
                      <span>{t('alarm.modeFirst')}</span>
                    </label>
                    <label className="notification-mode-radio">
                      <input
                        type="radio"
                        name="notificationMode"
                        value="latest"
                        checked={displayConfig.notificationMode === 'latest'}
                        onChange={() => {
                          const updated = { ...displayConfig, notificationMode: 'latest' as NotificationMode };
                          saveAndSyncDisplayConfig(updated);
                        }}
                      />
                      <span>{t('alarm.modeLatest')}</span>
                    </label>
                  </div>
                </div>
              </div>
              <div className="setting-item" style={{ marginTop: '8px' }}>
                <span className="setting-label">{t('alarm.durationLabel')}</span>
                <div className="polling-control">
                  <input
                    type="text"
                    inputMode="numeric"
                    pattern="[0-9]*"
                    value={alarmFormDurationInput}
                    onChange={(e) => {
                      setAlarmFormDurationInput(e.target.value.replace(/[^0-9]/g, ''));
                    }}
                    className="polling-input"
                  />
                  <button
                    className="polling-apply"
                    onClick={() => {
                      const val = parseInt(alarmFormDurationInput, 10);
                      const duration = isNaN(val) || val <= 0 ? 10 : val;
                      setAlarmFormDurationInput(String(duration));
                      const updated = { ...displayConfig, notificationDuration: duration };
                      saveAndSyncDisplayConfig(updated);
                    }}
                  >
                    {t('alarm.apply')}
                  </button>
                </div>
              </div>
            </div>

            {/* 알림 추가 */}
            <div className="settings-section">
              <h3>{t('alarm.addTitle')}</h3>
              <div className="alarm-form">
                <div className="alarm-form-row">
                  <span className="setting-label">{t('alarm.typeLabel')}</span>
                  <select
                    className="alarm-select"
                    value={alarmFormType}
                    onChange={(e) => setAlarmFormType(e.target.value as AlarmType)}
                  >
                    <option value="interval">{t('alarm.typeInterval')}</option>
                    <option value="absolute">{t('alarm.typeAbsolute')}</option>
                    <option value="daily">{t('alarm.typeDaily')}</option>
                    <option value="relative">{t('alarm.typeRelative')}</option>
                    <option value="hourly">{t('alarm.typeHourly')}</option>
                  </select>
                </div>

                {alarmFormType === 'interval' && (
                  <div className="alarm-form-row">
                    <span className="setting-label">{t('alarm.intervalLabel')}</span>
                    <input
                      type="text"
                      inputMode="numeric"
                      pattern="[0-9]*"
                      value={alarmFormIntervalMin}
                      onChange={(e) => setAlarmFormIntervalMin(e.target.value.replace(/[^0-9]/g, ''))}
                      className="polling-input"
                    />
                  </div>
                )}

                {(alarmFormType === 'absolute' || alarmFormType === 'daily') && (
                  <div className="alarm-form-row">
                    <span className="setting-label">{t('alarm.timeLabel')}</span>
                    <input
                      type="time"
                      value={alarmFormTime}
                      onChange={(e) => setAlarmFormTime(e.target.value)}
                      className="alarm-time-input"
                    />
                  </div>
                )}

                {alarmFormType === 'relative' && (
                  <div className="alarm-form-row">
                    <span className="setting-label">{t('alarm.delayLabel')}</span>
                    <div className="alarm-delay-inputs">
                      <input
                        type="text"
                        inputMode="numeric"
                        pattern="[0-9]*"
                        value={alarmFormDelayHours}
                        onChange={(e) => setAlarmFormDelayHours(e.target.value.replace(/[^0-9]/g, ''))}
                        className="polling-input"
                      />
                      <span>{t('alarm.unitHours')}</span>
                      <input
                        type="text"
                        inputMode="numeric"
                        pattern="[0-9]*"
                        value={alarmFormDelayMinutes}
                        onChange={(e) => setAlarmFormDelayMinutes(e.target.value.replace(/[^0-9]/g, ''))}
                        className="polling-input"
                      />
                      <span>{t('alarm.unitMinutes')}</span>
                    </div>
                  </div>
                )}

                {alarmFormType === 'hourly' && (
                  <div className="alarm-form-row">
                    <span className="setting-label">{t('alarm.hourlyLabel')}</span>
                    <input
                      type="text"
                      inputMode="numeric"
                      pattern="[0-9]*"
                      value={alarmFormHourlyMinute}
                      onChange={(e) => setAlarmFormHourlyMinute(e.target.value.replace(/[^0-9]/g, ''))}
                      className="polling-input"
                    />
                  </div>
                )}

                <div className="alarm-form-row">
                  <span className="setting-label">{t('alarm.messageLabel')}</span>
                  <div className="emoji-input-wrapper">
                    <input
                      type="text"
                      value={alarmFormMessage}
                      onChange={(e) => setAlarmFormMessage(e.target.value)}
                      maxLength={50}
                      placeholder={t('alarm.messagePlaceholder')}
                      className="alarm-message-input"
                    />
                    <button
                      className="emoji-toggle-btn"
                      onClick={() => setEmojiPickerTarget(emojiPickerTarget === 'alarm' ? null : 'alarm')}
                    >
                      😀
                    </button>
                  </div>
                </div>
                {emojiPickerTarget === 'alarm' && renderEmojiPicker()}

                <button className="polling-apply alarm-add-btn" onClick={handleAddAlarm}>
                  {t('alarm.add')}
                </button>
              </div>
            </div>

            {/* 등록된 알림 목록 */}
            <div className="settings-section">
              <h3>{t('alarm.listTitle')}</h3>
              {alarms.length === 0 ? (
                <p className="description">{t('alarm.empty')}</p>
              ) : (
                <div className="alarm-list">
                  {alarms.map((alarm) => (
                    <div key={alarm.id} className={`alarm-item ${!alarm.enabled ? 'alarm-disabled' : ''}`}>
                      <div className="alarm-item-info">
                        <span className="alarm-type-badge">{getAlarmBadge(alarm.type)}</span>
                        <span className="alarm-detail">{getAlarmDetail(alarm)}</span>
                        <span className="alarm-message-preview">{alarm.message}</span>
                      </div>
                      <div className="alarm-item-actions">
                        <label className="alarm-toggle">
                          <input
                            type="checkbox"
                            checked={alarm.enabled}
                            onChange={() => handleToggleAlarm(alarm.id)}
                          />
                        </label>
                        <button
                          className="alarm-delete-btn"
                          onClick={() => handleRemoveAlarm(alarm.id)}
                        >
                          ✕
                        </button>
                      </div>
                    </div>
                  ))}
                </div>
              )}
              <div className="export-import-buttons">
                <button
                  className="polling-apply"
                  disabled={alarms.length === 0}
                  onClick={handleExportAlarms}
                >
                  {t('export.save')}
                </button>
                <button
                  className="polling-apply"
                  onClick={handleImportAlarms}
                >
                  {t('export.import')}
                </button>
              </div>
            </div>
          </>
        )}

        {settingsTab === "mail" && (
          <div className="settings-section mail-section">
            <h3>{t('mail.title')}</h3>
            <p className="description">{t('mail.description')}</p>

            {/* 메일 알림 ON/OFF */}
            <label className="monitor-item">
              <input
                type="checkbox"
                checked={mailConfig.enabled}
                onChange={(e) => setMailConfig({ ...mailConfig, enabled: e.target.checked })}
              />
              <span className="monitor-label">{t('mail.enable')}</span>
            </label>

            {/* 입력 필드 */}
            <div className="mail-form">
              <div className="alarm-form-row">
                <span className="setting-label">{t('mail.accountName')}</span>
                <input
                  type="text"
                  className="alarm-message-input"
                  value={mailConfig.account_name}
                  onChange={(e) => setMailConfig({ ...mailConfig, account_name: e.target.value })}
                />
              </div>
              <div className="alarm-form-row">
                <span className="setting-label">{t('mail.pop3Host')}</span>
                <div className="mail-host-row">
                  <input
                    type="text"
                    className="alarm-message-input mail-host-input"
                    value={mailConfig.host}
                    onChange={(e) => setMailConfig({ ...mailConfig, host: e.target.value })}
                  />
                  <input
                    type="text"
                    inputMode="numeric"
                    pattern="[0-9]*"
                    className="polling-input mail-port-input"
                    value={String(mailConfig.port)}
                    onChange={(e) => {
                      const v = parseInt(e.target.value.replace(/[^0-9]/g, ''), 10);
                      setMailConfig({ ...mailConfig, port: isNaN(v) ? 0 : Math.min(65535, v) });
                    }}
                    aria-label={t('mail.pop3Port')}
                  />
                </div>
              </div>
              <label className="monitor-item">
                <input
                  type="checkbox"
                  checked={mailConfig.use_tls}
                  onChange={(e) => {
                    const useTls = e.target.checked;
                    // SSL 토글 시 권장 포트 자동 보정 (사용자가 이미 다른 포트로 변경한 경우는 유지)
                    let port = mailConfig.port;
                    if (useTls && (port === 110 || port === 0)) port = 995;
                    if (!useTls && port === 995) port = 110;
                    setMailConfig({ ...mailConfig, use_tls: useTls, port });
                  }}
                />
                <span className="monitor-label">{t('mail.useTls')}</span>
              </label>
              <div className="alarm-form-row">
                <span className="setting-label">{t('mail.userId')}</span>
                <input
                  type="text"
                  className="alarm-message-input"
                  value={mailConfig.user_id}
                  onChange={(e) => setMailConfig({ ...mailConfig, user_id: e.target.value })}
                />
              </div>
              <div className="alarm-form-row">
                <span className="setting-label">{t('mail.password')}</span>
                <input
                  type="password"
                  className="alarm-message-input"
                  value={mailConfig.password}
                  placeholder={mailHasPassword ? '••••••••' : ''}
                  onChange={(e) => setMailConfig({ ...mailConfig, password: e.target.value })}
                />
              </div>
              {mailHasPassword && mailConfig.password === '' && (
                <p className="description" style={{ marginLeft: '4px', marginTop: '-4px' }}>
                  {t('mail.passwordKept')}
                </p>
              )}
              <div className="alarm-form-row">
                <span className="setting-label">{t('mail.pollMinutes')}</span>
                <select
                  className="alarm-select"
                  value={mailConfig.poll_minutes}
                  onChange={(e) => setMailConfig({ ...mailConfig, poll_minutes: parseInt(e.target.value, 10) })}
                >
                  {Array.from({ length: 60 }, (_, i) => i + 1).map((m) => (
                    <option key={m} value={m}>{m}</option>
                  ))}
                </select>
              </div>
              <div className="alarm-form-row">
                <span className="setting-label">{t('mail.bubbleDuration')}</span>
                <div className="polling-control">
                  <input
                    type="text"
                    inputMode="numeric"
                    pattern="[0-9]*"
                    className="polling-input"
                    value={mailDurationInput}
                    onChange={(e) => setMailDurationInput(e.target.value.replace(/[^0-9]/g, ''))}
                  />
                </div>
              </div>
              <p className="description" style={{ marginTop: '-4px' }}>{t('mail.priorityDesc')}</p>
              <p className="description" style={{ marginTop: '-4px' }}>{t('mail.firstPollNotice')}</p>

              <div className="export-import-buttons" style={{ marginTop: '8px' }}>
                <button
                  className="polling-apply"
                  onClick={async () => {
                    try {
                      await invoke('mail_test_connection', { cfg: mailConfig });
                      // 성공 메시지를 별도 영역에 표시하고 3초 후 자동 사라짐
                      setMailTestStatus('success');
                      if (mailTestTimerRef.current) clearTimeout(mailTestTimerRef.current);
                      mailTestTimerRef.current = setTimeout(() => {
                        setMailTestStatus('idle');
                        mailTestTimerRef.current = null;
                      }, 3000);
                    } catch (e) {
                      const err = e as MailErrorPayload | string;
                      if (typeof err === 'object' && err !== null && 'kind' in err) {
                        setMailError(err as MailErrorPayload);
                      } else {
                        setMailError({ kind: 'Network', message: String(err) });
                      }
                    }
                  }}
                >
                  {t('mail.test')}
                </button>
                <button
                  className="polling-apply"
                  onClick={async () => {
                    // 표시 시간 검증 후 displayConfig 갱신
                    const dur = Math.max(1, Math.min(600, parseInt(mailDurationInput, 10) || 60));
                    setMailDurationInput(String(dur));
                    const updated = { ...displayConfig, mailDuration: dur };
                    saveAndSyncDisplayConfig(updated);
                    // 메일 설정 저장 (백엔드 DPAPI + 폴링 시작)
                    try {
                      await invoke('mail_apply_config', { cfg: mailConfig });
                      // 비밀번호 입력 필드 클리어 — 다음 저장 시 빈 값이면 기존 유지
                      if (mailConfig.password) {
                        setMailHasPassword(true);
                        setMailConfig({ ...mailConfig, password: '' });
                      }
                      setMailError(null);
                    } catch (e) {
                      setMailError({ kind: 'Network', message: String(e) });
                    }
                  }}
                >
                  {t('mail.save')}
                </button>
              </div>

              {/* 테스트 성공 메시지 (3초 자동 사라짐, 폴링 상태와 독립) */}
              {mailTestStatus === 'success' && (
                <div className="mail-test-success">{t('mail.testSuccess')}</div>
              )}
            </div>

            {/* 오류/상태 섹션 */}
            <div className="mail-error-section" style={{ marginTop: '16px' }}>
              <h4 style={{ margin: '8px 0' }}>{t('mail.errorSection')}</h4>
              {mailError ? (
                <div className={`mail-error-box mail-error-${mailError.kind.toLowerCase()}`}>
                  {formatMailError(mailError, t)}
                </div>
              ) : (
                <div className="mail-status-ok">{t('mail.statusOk')}</div>
              )}
            </div>
          </div>
        )}

        {settingsTab === "timer" && (
          <div className="settings-section">
            <h3>{t('timer.title')}</h3>
            <p className="description">{t('timer.description')}</p>
            <div className="general-settings">
              <div className="setting-item">
                <span className="setting-label">{t('timer.minutes')}</span>
                <input
                  type="range"
                  min="1"
                  max="60"
                  value={timerMinutes}
                  disabled={timerRunning}
                  onChange={(e) => {
                    const val = Number(e.target.value);
                    setTimerMinutes(val);
                    localStorage.setItem('timerMinutes', String(val));
                  }}
                  style={{ flex: 1 }}
                />
                <span className="value-badge">{timerMinutes}{t('timer.minutes')}</span>
              </div>
              <div className="setting-item">
                <span className="setting-label">{t('timer.fontSize')}</span>
                <select
                  className="alarm-select"
                  value={timerFontSize}
                  onChange={(e) => {
                    const val = Number(e.target.value);
                    setTimerFontSize(val);
                    localStorage.setItem('timerFontSize', String(val));
                    invoke('update_timer_font_size', { size: val });
                  }}
                >
                  {Array.from({ length: 11 }, (_, i) => i + 10).map(size => (
                    <option key={size} value={size}>{size}px</option>
                  ))}
                </select>
              </div>
              {timerRunning && timerRemaining > 0 && (
                <div className="setting-item" style={{ justifyContent: 'center' }}>
                  <span style={{ fontSize: '24px', fontWeight: 'bold', fontVariantNumeric: 'tabular-nums' }}>
                    {String(Math.floor(timerRemaining / 60)).padStart(2, '0')}:{String(timerRemaining % 60).padStart(2, '0')}
                  </span>
                </div>
              )}
              <div className="setting-item" style={{ gap: '8px' }}>
                <button
                  className="polling-apply"
                  disabled={timerRunning}
                  onClick={handleTimerStart}
                  style={{ flex: 1 }}
                >
                  {t('timer.start')}
                </button>
                <button
                  className="polling-apply"
                  disabled={!timerRunning}
                  onClick={handleTimerStop}
                  style={{ flex: 1 }}
                >
                  {t('timer.stop')}
                </button>
              </div>
            </div>
            <p className="description" style={{ marginTop: '12px', color: '#999', fontSize: '11px' }}>
              {t('timer.notice')}
            </p>
          </div>
        )}

        {settingsTab === "font" && (
          <div className="settings-section">
            <h3>{t('font.title')}</h3>
            <div className="general-settings">
              <div className="setting-item">
                <span className="setting-label">{t('general.fontSize')}</span>
                <select
                  className="alarm-select"
                  value={fontSize}
                  onChange={(e) => {
                    const size = Number(e.target.value);
                    saveAndSyncAppSettings(language, size, fontFamily, monitoringFontColor, alarmFontColor);
                  }}
                >
                  {FONT_SIZE_OPTIONS.map(size => (
                    <option key={size} value={size}>{size}px</option>
                  ))}
                </select>
              </div>
              <div className="setting-item">
                <span className="setting-label">{t('general.fontFamily')}</span>
                <select
                  className="alarm-select"
                  value={fontFamily}
                  onChange={(e) => {
                    saveAndSyncAppSettings(language, fontSize, e.target.value, monitoringFontColor, alarmFontColor);
                  }}
                >
                  {FONT_FAMILY_OPTIONS.map(f => (
                    <option key={f.value} value={f.value}>
                      {f.value === '' ? t('general.fontDefault') : f.label}
                    </option>
                  ))}
                </select>
              </div>
              <div className="setting-item color-setting">
                <span className="setting-label">{t('font.monitoringColor')}</span>
                <div className="font-color-palette">
                  {FONT_COLOR_PALETTE.map(color => (
                    <button
                      key={`mon-${color}`}
                      className={`font-color-swatch${monitoringFontColor === color ? ' selected' : ''}`}
                      style={{ backgroundColor: color }}
                      onClick={() => saveAndSyncAppSettings(language, fontSize, fontFamily, color, alarmFontColor)}
                    />
                  ))}
                </div>
              </div>
              <div className="setting-item color-setting">
                <span className="setting-label">{t('font.alarmColor')}</span>
                <div className="font-color-palette">
                  {FONT_COLOR_PALETTE.map(color => (
                    <button
                      key={`alm-${color}`}
                      className={`font-color-swatch${alarmFontColor === color ? ' selected' : ''}`}
                      style={{ backgroundColor: color }}
                      onClick={() => saveAndSyncAppSettings(language, fontSize, fontFamily, monitoringFontColor, color)}
                    />
                  ))}
                </div>
              </div>
            </div>
          </div>
        )}

        {settingsTab === "general" && (
          <div className="settings-section">
            <h3>{t('general.title')}</h3>
            <div className="general-settings">
              <label className="setting-item">
                <input
                  type="checkbox"
                  checked={autoStart}
                  onChange={async (e) => {
                    const checked = e.target.checked;
                    try {
                      await invoke("set_auto_start", { enabled: checked });
                      setAutoStart(checked);
                    } catch { /* 실패 시 무시 */ }
                  }}
                />
                <span className="setting-label">{t('general.autoStart')}</span>
              </label>
              <div className="setting-item" style={{ flexDirection: 'column', alignItems: 'flex-start', gap: '6px' }}>
                <label style={{ display: 'flex', alignItems: 'center', gap: '10px' }}>
                  <input
                    type="checkbox"
                    checked={mouseEnabled}
                    onChange={(e) => {
                      setMouseEnabled(e.target.checked);
                      invoke("update_mouse_enabled", { enabled: e.target.checked });
                    }}
                  />
                  <span className="setting-label">{t('general.mouseEnabled')}</span>
                </label>
                <span style={{ paddingLeft: '26px', fontSize: '11px', color: '#888', lineHeight: '1.4' }}>{t('general.mouseEnabledDesc')}</span>
              </div>
              <div className="setting-item" style={{ flexDirection: 'column', alignItems: 'flex-start', gap: '6px' }}>
                <label style={{ display: 'flex', alignItems: 'center', gap: '10px' }}>
                  <input
                    type="checkbox"
                    checked={bubbleEnabled}
                    onChange={(e) => {
                      setBubbleEnabled(e.target.checked);
                      invoke("update_bubble_enabled", { enabled: e.target.checked });
                    }}
                  />
                  <span className="setting-label">{t('general.bubbleLabel')}</span>
                </label>
                <label style={{ display: 'flex', alignItems: 'center', gap: '10px', paddingLeft: '26px', opacity: bubbleEnabled ? 1 : 0.4 }}>
                  <input
                    type="checkbox"
                    checked={bubbleSide}
                    disabled={!bubbleEnabled}
                    onChange={(e) => {
                      setBubbleSide(e.target.checked);
                      localStorage.setItem('bubbleSide', String(e.target.checked));
                      invoke("update_bubble_side", { enabled: e.target.checked });
                    }}
                  />
                  <span className="setting-label">{t('general.bubbleSide')}</span>
                </label>
                <label style={{ display: 'flex', alignItems: 'center', gap: '10px', paddingLeft: '26px', opacity: bubbleEnabled ? 1 : 0.4 }}>
                  <input
                    type="checkbox"
                    checked={bubbleTop}
                    disabled={!bubbleEnabled}
                    onChange={(e) => {
                      setBubbleTop(e.target.checked);
                      localStorage.setItem('bubbleTop', String(e.target.checked));
                      invoke("update_bubble_top", { enabled: e.target.checked });
                    }}
                  />
                  <span className="setting-label">{t('general.bubbleTop')}</span>
                </label>
                <div style={{ display: 'flex', alignItems: 'center', gap: '10px', paddingLeft: '26px', opacity: bubbleEnabled ? 1 : 0.4 }}>
                  <span className="setting-label">{t('general.bubbleHeight')}</span>
                  <select
                    className="alarm-select"
                    value={bubbleHeight}
                    disabled={!bubbleEnabled}
                    onChange={(e) => {
                      const val = Number(e.target.value);
                      setBubbleHeight(val);
                      localStorage.setItem('bubbleHeight', String(val));
                      invoke("update_bubble_height", { height: val });
                    }}
                  >
                    {Array.from({ length: 31 }, (_, i) => (
                      <option key={i} value={i}>{i}</option>
                    ))}
                  </select>
                </div>
              </div>
              <div className="setting-item">
                <span className="setting-label">{t('general.language')}</span>
                <select
                  className="alarm-select"
                  value={language}
                  onChange={(e) => {
                    saveAndSyncAppSettings(e.target.value as Language, fontSize, fontFamily, monitoringFontColor, alarmFontColor);
                  }}
                >
                  <option value="system">{t('general.langSystem')}</option>
                  <option value="ko">{t('general.langKo')}</option>
                  <option value="en">{t('general.langEn')}</option>
                  <option value="ja">{t('general.langJa')}</option>
                  <option value="zh">{t('general.langZh')}</option>
                  <option value="zh-Hant">{t('general.langZhHant')}</option>
                </select>
              </div>
            </div>
          </div>
        )}

        {settingsTab === "about" && (
          <div className="settings-section">
            <h3>{t('about.title')}</h3>
            <div className="about-block">
              <div className="about-row">
                <span className="about-label">{t('about.appVersion')}</span>
                <span className="about-value">v{appVersion}</span>
              </div>
              {/* 업데이트 상태 표시: 'available'은 클릭 가능한 버튼, 그 외는 안내 텍스트 */}
              {updateStatus === 'available' && updateInfo ? (
                <div className="about-row">
                  <span className="about-label"></span>
                  <button
                    className="about-link"
                    onClick={handleUpdateClick}
                    title={t('about.updateClickHint')}
                  >
                    {t('about.updateAvailable')} v{updateInfo.latest_version}
                  </button>
                </div>
              ) : updateStatus !== 'idle' && updateStatus !== 'available' && (
                <div className="about-row">
                  <span className="about-label"></span>
                  <span className={updateStatus === 'error' ? 'about-update-error' : 'about-update-text'}>
                    {t(`about.update${updateStatus.charAt(0).toUpperCase() + updateStatus.slice(1)}`)}
                  </span>
                </div>
              )}
            </div>

            <div className="about-block">
              <div className="about-heading">{t('about.licenseTitle')}</div>
              <div className="about-row">
                <span className="about-label">{t('about.licenseName')}</span>
              </div>
              <p className="about-desc">{t('about.licenseDesc')}</p>
              <div className="about-row">
                <span className="about-label">{t('about.repoLabel')}</span>
                <button
                  className="about-link"
                  onClick={() => openUrl('https://github.com/jongcheol-pak/TaskMon')}
                >
                  https://github.com/jongcheol-pak/TaskMon
                </button>
              </div>
            </div>

            <div className="about-block">
              <div className="about-heading">{t('about.assetsTitle')}</div>
              <p className="about-desc">{t('about.assetsDesc')}</p>
              <div className="about-row">
                <button
                  className="about-link"
                  onClick={() => openUrl('https://itch.io/')}
                >
                  {t('about.assetsLink')} (https://itch.io/)
                </button>
              </div>
              <div className="about-row">
                <span className="about-credits">{t('about.assetsCredits')}</span>
              </div>
            </div>
          </div>
        )}
      </main>
      </div>
    </div>
  );
}
