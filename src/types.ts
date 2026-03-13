// Vite가 빌드 시 올바른 URL로 변환하도록 import로 참조
import runUnarmed from './assets/Skeleton_Default_Run_Unarmed.png';
import runSword from './assets/Skeleton_Default_Run_Sword.png';
import runSwordShield from './assets/Skeleton_Default_X_Sword+Shield.png';
import idleUnarmed from './assets/Skeleton_Default_Idle_Unarmed.png';
import idleSword from './assets/Skeleton_Default_Idle_Sword.png';
import idleSwordShield from './assets/idle.png';

// 달리기 이미지 3종 (우클릭으로 순환: 0→1→2→0)
export const RUN_IMAGES = [runUnarmed, runSword, runSwordShield] as const;
// 아이들 이미지 3종 (runVariant와 동일 인덱스)
export const IDLE_IMAGES = [idleUnarmed, idleSword, idleSwordShield] as const;

export interface MonitorConfig {
  cpu: boolean;
  memory: boolean;
  network: boolean;
  battery: boolean;
}

// condition: 오타 방지를 위해 명확한 단어 사용
export type MessageCondition = "less_than" | "greater_than" | "less_equal" | "greater_equal" | "equal";

export interface PetMessage {
  target: string;         // "cpu" | "memory" | "battery" | "network_down" | "network_up"
  condition: MessageCondition;
  value: number;
  priority: number;       // 높을수록 우선
  text: string;
}

// 알림 타입 정의
export type AlarmType = 'interval' | 'absolute' | 'daily' | 'relative' | 'hourly';

export interface Alarm {
  id: string;
  type: AlarmType;
  enabled: boolean;
  message: string;
  intervalMinutes?: number;  // interval: 반복 간격(분)
  targetTime?: string;       // "HH:mm" (absolute, daily 공용)
  delayMinutes?: number;     // relative: 원본 지연 시간(분, 표시용)
  startedAt?: number;        // interval: 시작 시점 timestamp
  lastFiredAt?: number;      // interval: 마지막 발화 timestamp
  firedToday?: string;       // "YYYY-MM-DD" (absolute, daily: 오늘 발화 여부)
  absoluteTarget?: number;   // relative: 발화 대상 절대 timestamp
  relativeFired?: boolean;   // relative: 발화 완료 여부
  hourlyMinute?: number;     // hourly: 매 시 N분에 발화 (0~59)
  lastFiredHour?: number;    // hourly: 마지막 발화한 시각(hour)
}

// 알림 중복 표시 모드
export type NotificationMode = 'all' | 'first' | 'latest';

// 표시 설정: 모니터링/알림 문구 표시 제어
export interface DisplayConfig {
  showMonitoringText: boolean;
  showNotificationText: boolean;
  notificationPriority: boolean; // 알림 우선: 알림 표시 중에는 모니터링 문구 숨김
  notificationMode: NotificationMode; // 알림 중복 표시 모드
  notificationDuration: number;  // 알림 표시 시간(초)
}

export const DEFAULT_DISPLAY_CONFIG: DisplayConfig = {
  showMonitoringText: true,
  showNotificationText: true,
  notificationPriority: false,
  notificationMode: 'all',
  notificationDuration: 10,
};

// 발화된 알림 정보 (메시지 + 발화 시점)
export interface FiredNotification {
  message: string;
  firedAt: number; // 발화 시점 timestamp
}

// 폰트 크기 옵션 (8~20)
export const FONT_SIZE_OPTIONS = Array.from({ length: 13 }, (_, i) => i + 8);

// 폰트 목록
export const FONT_FAMILY_OPTIONS = [
  { value: '', label: 'general.fontDefault' },
  { value: '맑은 고딕', label: '맑은 고딕' },
  { value: '굴림', label: '굴림' },
  { value: '돋움', label: '돋움' },
  { value: '바탕', label: '바탕' },
  { value: 'Arial', label: 'Arial' },
  { value: 'Segoe UI', label: 'Segoe UI' },
  { value: 'Consolas', label: 'Consolas' },
] as const;

// 알림 고유 ID 생성
export function generateAlarmId(): string {
  return Date.now().toString(36) + Math.random().toString(36).slice(2, 7);
}

// 알림 배열 유효성 검증
export function isValidAlarm(obj: unknown): obj is Alarm {
  if (typeof obj !== 'object' || obj === null) return false;
  const a = obj as Record<string, unknown>;
  return (
    typeof a.id === 'string' &&
    typeof a.message === 'string' &&
    typeof a.enabled === 'boolean' &&
    ['interval', 'absolute', 'daily', 'relative', 'hourly'].includes(a.type as string)
  );
}

// 모니터링 메시지 유효성 검증
export function isValidPetMessage(obj: unknown): obj is PetMessage {
  if (typeof obj !== 'object' || obj === null) return false;
  const m = obj as Record<string, unknown>;
  return (
    typeof m.target === 'string' &&
    typeof m.value === 'number' &&
    typeof m.priority === 'number' &&
    typeof m.text === 'string' &&
    ['less_than', 'greater_than', 'less_equal', 'greater_equal', 'equal'].includes(m.condition as string)
  );
}

// 알림 조건 체크: 발화된 모든 메시지와 업데이트된 알림 목록 반환
export function checkAlarms(alarms: Alarm[]): { firedMessages: FiredNotification[]; updatedAlarms: Alarm[] } {
  const now = Date.now();
  const today = new Date().toISOString().slice(0, 10);
  const currentHHmm = new Date().toTimeString().slice(0, 5);
  const firedMessages: FiredNotification[] = [];
  let changed = false;

  const updated = alarms.map(alarm => {
    if (!alarm.enabled) return alarm;

    switch (alarm.type) {
      case 'interval': {
        if (!alarm.startedAt || !alarm.intervalMinutes) return alarm;
        const lastFired = alarm.lastFiredAt || alarm.startedAt;
        if (now - lastFired >= alarm.intervalMinutes * 60000) {
          firedMessages.push({ message: alarm.message, firedAt: now });
          changed = true;
          return { ...alarm, lastFiredAt: now };
        }
        return alarm;
      }
      case 'absolute': {
        if (!alarm.targetTime || alarm.firedToday === today) return alarm;
        if (currentHHmm >= alarm.targetTime) {
          firedMessages.push({ message: alarm.message, firedAt: now });
          changed = true;
          return { ...alarm, firedToday: today, enabled: false };
        }
        return alarm;
      }
      case 'daily': {
        if (!alarm.targetTime || alarm.firedToday === today) return alarm;
        if (currentHHmm >= alarm.targetTime) {
          firedMessages.push({ message: alarm.message, firedAt: now });
          changed = true;
          return { ...alarm, firedToday: today };
        }
        return alarm;
      }
      case 'relative': {
        if (!alarm.absoluteTarget || alarm.relativeFired) return alarm;
        if (now >= alarm.absoluteTarget) {
          firedMessages.push({ message: alarm.message, firedAt: now });
          changed = true;
          return { ...alarm, relativeFired: true, enabled: false };
        }
        return alarm;
      }
      case 'hourly': {
        if (alarm.hourlyMinute == null) return alarm;
        const currentDate = new Date();
        const currentHour = currentDate.getHours();
        const currentMin = currentDate.getMinutes();
        if (currentMin >= alarm.hourlyMinute && alarm.lastFiredHour !== currentHour) {
          firedMessages.push({ message: alarm.message, firedAt: now });
          changed = true;
          return { ...alarm, lastFiredHour: currentHour };
        }
        return alarm;
      }
    }
    return alarm;
  });

  return { firedMessages, updatedAlarms: changed ? updated : alarms };
}

// 메시지 조건 평가: 매칭되는 조건의 메시지 목록 반환 (priority 내림차순 정렬)
// monitorConfig에서 체크 안 된 항목은 평가에서 제외
export function evaluateMessages(
  messages: PetMessage[],
  cpu: number,
  mem: number,
  battery: number,
  netDown: number,
  netUp: number,
  config: MonitorConfig
): string[] {
  const matched = messages.filter(msg => {
    let actual: number;
    switch (msg.target) {
      case "cpu":
        if (!config.cpu) return false;
        actual = cpu; break;
      case "memory":
        if (!config.memory) return false;
        actual = mem; break;
      case "battery":
        if (!config.battery || battery < 0) return false;
        actual = battery;
        break;
      case "network_down":
      case "network_up":
        if (!config.network) return false;
        actual = msg.target === "network_down" ? netDown : netUp;
        break;
      default: return false;
    }

    switch (msg.condition) {
      case "less_than": return actual < msg.value;
      case "greater_than": return actual > msg.value;
      case "less_equal": return actual <= msg.value;
      case "greater_equal": return actual >= msg.value;
      case "equal": return actual === msg.value;
      default: return false;
    }
  });

  matched.sort((a, b) => b.priority - a.priority);

  // 같은 대상(target)에서 조건이 겹칠 경우 우선순위가 가장 높은 메시지만 유지
  const seen = new Set<string>();
  const deduped = matched.filter(msg => {
    if (seen.has(msg.target)) return false;
    seen.add(msg.target);
    return true;
  });

  return deduped.map(m => m.text);
}

// localStorage JSON 안전 파싱 (손상된 데이터로 인한 크래시 방지)
export function safeParse<T>(key: string, fallback: T): T {
  try {
    const saved = localStorage.getItem(key);
    return saved ? JSON.parse(saved) as T : fallback;
  } catch {
    localStorage.removeItem(key);
    return fallback;
  }
}

export function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes}B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)}KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)}MB`;
}
