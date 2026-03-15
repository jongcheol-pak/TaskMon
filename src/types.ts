// Vite가 빌드 시 올바른 URL로 변환하도록 import로 참조
// 해골 스프라이트
import runUnarmed from './assets/Skeleton_Default_Run_Unarmed.png';
import runSword from './assets/Skeleton_Default_Run_Sword.png';
import runSwordShield from './assets/Skeleton_Default_X_Sword+Shield.png';
import idleUnarmed from './assets/Skeleton_Default_Idle_Unarmed.png';
import idleSword from './assets/Skeleton_Default_Idle_Sword.png';
import idleSwordShield from './assets/idle.png';
import skeletonHurt from './assets/Skeleton_Default_Hurt.png';
// 좀비 스프라이트
import zombieWalk from './assets/Zombie_Default_Walk.png';
import zombieIdle from './assets/Zombie_Default_Idle.png';
import zombieHurt from './assets/Zombie_Default_Hurt.png';
// 공룡 스프라이트 (4종)
import dino1Run from './assets/Dino_1_Run.png';
import dino1Idle from './assets/Dino_1_Idle.png';
import dino1Hurt from './assets/Dino_1_Hurt.png';
import dino1RightClick from './assets/Dino_1_RightClick.png';
import dino2Run from './assets/Dino_2_Run.png';
import dino2Idle from './assets/Dino_2_Idle.png';
import dino2Hurt from './assets/Dino_2_Hurt.png';
import dino2RightClick from './assets/Dino_2_RightClick.png';
import dino3Run from './assets/Dino_3_Run.png';
import dino3Idle from './assets/Dino_3_Idle.png';
import dino3Hurt from './assets/Dino_3_Hurt.png';
import dino3RightClick from './assets/Dino_3_RightClick.png';
import dino4Run from './assets/Dino_4_Run.png';
import dino4Idle from './assets/Dino_4_Idle.png';
import dino4Hurt from './assets/Dino_4_Hurt.png';
import dino4RightClick from './assets/Dino_4_RightClick.png';
// Finn 스프라이트
import finnRun from './assets/Finn_Run.png';
import finnIdle from './assets/Finn_Idle.png';
import finnHurt from './assets/Finn_Hurt.png';
import finnRightClick from './assets/Finn_RightClick.png';
// 몬스터(1) 스프라이트
import monster1Walk from './assets/Monster1_Walk.png';
import monster1Idle from './assets/Monster1_Idle.png';
import monster1Hurt from './assets/Monster1_Hurt.png';
import monster1Run from './assets/Monster1_Run.png';
// 몬스터(2) 스프라이트
import monster2Walk from './assets/Monster2_Walk.png';
import monster2Idle from './assets/Monster2_Idle.png';
import monster2Hurt from './assets/Monster2_Hurt.png';
import monster2Run from './assets/Monster2_Run.png';
// 몬스터(3) 스프라이트
import monster3Walk from './assets/Monster3_Walk.png';
import monster3Idle from './assets/Monster3_Idle.png';
import monster3Hurt from './assets/Monster3_Hurt.png';
import monster3Run from './assets/Monster3_Run.png';
import monster3Attack from './assets/Monster3_Attack.png';
// 몬스터(4) 스프라이트
import monster4Walk from './assets/Monster4_Walk.png';
import monster4Idle from './assets/Monster4_Idle.png';
import monster4Hurt from './assets/Monster4_Hurt.png';
import monster4Run from './assets/Monster4_Run.png';
import monster4Death from './assets/Monster4_Death.png';
// 몬스터(5) 스프라이트
import monster5Walk from './assets/Monster5_Walk.png';
import monster5Idle from './assets/Monster5_Idle.png';
import monster5Hurt from './assets/Monster5_Hurt.png';
import monster5Run from './assets/Monster5_Run.png';
import monster5Attack from './assets/Monster5_Attack.png';
// 몬스터(6) 스프라이트
import monster6Run from './assets/Monster6_Run.png';
import monster6Idle from './assets/Monster6_Idle.png';
import monster6Hurt from './assets/Monster6_Hurt.png';
import monster6Stun from './assets/Monster6_Stun.png';
// 사람(1) 스프라이트
import human1Walk from './assets/Human1_Walk.png';
import human1Idle from './assets/Human1_Idle.png';
import human1Hurt from './assets/Human1_Hurt.png';
import human1Dash from './assets/Human1_Dash.png';
// 사람(2) 스프라이트
import human2Run from './assets/Human2_Run.png';
import human2Idle from './assets/Human2_Idle.png';
import human2Hurt from './assets/Human2_Hurt.png';
// 자동차(1) 스프라이트
import car1Run from './assets/Car1_Run.png';
import car1Hurt from './assets/Car1_Hurt.png';

// 펫 종류 정의
export interface PetType {
  id: string;
  name: string;
  runImages: readonly string[];   // 이동 이미지 (variant별)
  idleImages: readonly string[];  // 대기 이미지 (variant별)
  hurtImage: string;              // 피격 이미지
  hurtFrames: number;             // 피격 프레임 수
  idleFrames: number[];            // idle 프레임 수 (variant별, idleImages와 1:1 대응)
  runFrames: number[];            // 이동 프레임 수 (variant별, runImages와 1:1 대응)
  frameWidth: number;             // 1프레임 너비 (px)
  frameHeight: number;            // 1프레임 높이 (px, 실제 이미지 높이)
  bottomPadding: number;          // 하단 투명 여백 (px, 위치 보정용)
  hasVariants: boolean;           // 우클릭 variant 전환 지원 여부
  rightClickImage?: string;       // 우클릭 1회 재생 이미지 (hasVariants: false일 때 사용)
  rightClickFrames?: number;      // 우클릭 1회 재생 프레임 수
  flipX: boolean;                 // 좌우 반전 여부 (원본이 왼쪽 향하면 true)
  speedFactor: number;            // 이동 속도 배율 (1.0 = 기본)
  displayScale: number;           // 표시 배율 (1.0 = 원본 크기)
}

// 펫 목록
export const PET_TYPES: PetType[] = [
  {
    id: 'skeleton',
    name: '해골',
    runImages: [runUnarmed, runSword, runSwordShield],
    idleImages: [idleUnarmed, idleSword, idleSwordShield],
    hurtImage: skeletonHurt,
    hurtFrames: 2,
    idleFrames: [6, 6, 6],
    runFrames: [6, 6, 6],
    frameWidth: 64,
    frameHeight: 48,
    bottomPadding: 0,
    hasVariants: true,
    flipX: true,
    speedFactor: 1.0,
    displayScale: 1.0,
  },
  {
    id: 'zombie',
    name: '좀비',
    runImages: [zombieWalk],
    idleImages: [zombieIdle],
    hurtImage: zombieHurt,
    hurtFrames: 6,
    idleFrames: [6],
    runFrames: [6],
    frameWidth: 64,
    frameHeight: 64,
    bottomPadding: 16,
    hasVariants: false,
    flipX: false,
    speedFactor: 0.5,
    displayScale: 1.0,
  },
  {
    id: 'dino1',
    name: '공룡(1)',
    runImages: [dino1Run, dino1RightClick],
    idleImages: [dino1Idle, dino1Idle],
    hurtImage: dino1Hurt,
    hurtFrames: 4,
    idleFrames: [4, 4],
    runFrames: [6, 7],
    frameWidth: 24,
    frameHeight: 21,
    bottomPadding: 0,
    hasVariants: true,
    flipX: false,
    speedFactor: 1.0,
    displayScale: 1.0,
  },
  {
    id: 'dino2',
    name: '공룡(2)',
    runImages: [dino2Run, dino2RightClick],
    idleImages: [dino2Idle, dino2Idle],
    hurtImage: dino2Hurt,
    hurtFrames: 4,
    idleFrames: [4, 4],
    runFrames: [6, 7],
    frameWidth: 24,
    frameHeight: 21,
    bottomPadding: 0,
    hasVariants: true,
    flipX: false,
    speedFactor: 1.0,
    displayScale: 1.0,
  },
  {
    id: 'dino3',
    name: '공룡(3)',
    runImages: [dino3Run, dino3RightClick],
    idleImages: [dino3Idle, dino3Idle],
    hurtImage: dino3Hurt,
    hurtFrames: 4,
    idleFrames: [4, 4],
    runFrames: [6, 7],
    frameWidth: 24,
    frameHeight: 21,
    bottomPadding: 0,
    hasVariants: true,
    flipX: false,
    speedFactor: 1.0,
    displayScale: 1.0,
  },
  {
    id: 'dino4',
    name: '공룡(4)',
    runImages: [dino4Run, dino4RightClick],
    idleImages: [dino4Idle, dino4Idle],
    hurtImage: dino4Hurt,
    hurtFrames: 4,
    idleFrames: [4, 4],
    runFrames: [6, 7],
    frameWidth: 24,
    frameHeight: 21,
    bottomPadding: 0,
    hasVariants: true,
    flipX: false,
    speedFactor: 1.0,
    displayScale: 1.0,
  },
  {
    id: 'finn',
    name: 'Finn',
    runImages: [finnRun],
    idleImages: [finnIdle],
    hurtImage: finnHurt,
    hurtFrames: 2,
    idleFrames: [9],
    runFrames: [6],
    frameWidth: 32,
    frameHeight: 24,
    bottomPadding: 0,
    hasVariants: false,
    rightClickImage: finnRightClick,
    rightClickFrames: 5,
    flipX: false,
    speedFactor: 1.0,
    displayScale: 1.0,
  },
  {
    id: 'monster1',
    name: '몬스터(1)',
    runImages: [monster1Walk, monster1Run],
    idleImages: [monster1Idle, monster1Idle],
    hurtImage: monster1Hurt,
    hurtFrames: 6,
    idleFrames: [4, 4],
    runFrames: [6, 8],
    frameWidth: 64,
    frameHeight: 40,
    bottomPadding: 0,
    hasVariants: true,
    flipX: false,
    speedFactor: 1.0,
    displayScale: 1.0,
  },
  {
    id: 'monster2',
    name: '몬스터(2)',
    runImages: [monster2Walk, monster2Run],
    idleImages: [monster2Idle, monster2Idle],
    hurtImage: monster2Hurt,
    hurtFrames: 6,
    idleFrames: [4, 4],
    runFrames: [6, 8],
    frameWidth: 64,
    frameHeight: 44,
    bottomPadding: 0,
    hasVariants: true,
    flipX: false,
    speedFactor: 1.0,
    displayScale: 1.0,
  },
  {
    id: 'monster3',
    name: '몬스터(3)',
    runImages: [monster3Walk, monster3Run],
    idleImages: [monster3Idle, monster3Idle],
    hurtImage: monster3Hurt,
    hurtFrames: 5,
    idleFrames: [6, 6],
    runFrames: [8, 8],
    frameWidth: 64,
    frameHeight: 33,
    bottomPadding: 0,
    hasVariants: true,
    rightClickImage: monster3Attack,
    rightClickFrames: 10,
    flipX: false,
    speedFactor: 1.0,
    displayScale: 1.0,
  },
  {
    id: 'monster4',
    name: '몬스터(4)',
    runImages: [monster4Walk, monster4Run],
    idleImages: [monster4Idle, monster4Idle],
    hurtImage: monster4Hurt,
    hurtFrames: 5,
    idleFrames: [6, 6],
    runFrames: [8, 8],
    frameWidth: 64,
    frameHeight: 40,
    bottomPadding: 0,
    hasVariants: true,
    rightClickImage: monster4Death,
    rightClickFrames: 10,
    flipX: false,
    speedFactor: 1.0,
    displayScale: 1.0,
  },
  {
    id: 'monster5',
    name: '몬스터(5)',
    runImages: [monster5Walk, monster5Run],
    idleImages: [monster5Idle, monster5Idle],
    hurtImage: monster5Hurt,
    hurtFrames: 5,
    idleFrames: [6, 6],
    runFrames: [8, 8],
    frameWidth: 64,
    frameHeight: 54,
    bottomPadding: 0,
    hasVariants: true,
    rightClickImage: monster5Attack,
    rightClickFrames: 9,
    flipX: false,
    speedFactor: 1.0,
    displayScale: 1.0,
  },
  {
    id: 'monster6',
    name: '몬스터(6)',
    runImages: [monster6Run],
    idleImages: [monster6Idle],
    hurtImage: monster6Hurt,
    hurtFrames: 5,
    idleFrames: [7],
    runFrames: [8],
    frameWidth: 80,
    frameHeight: 50,
    bottomPadding: 0,
    hasVariants: false,
    rightClickImage: monster6Stun,
    rightClickFrames: 18,
    flipX: false,
    speedFactor: 1.0,
    displayScale: 1.0,
  },
  {
    id: 'human1',
    name: '사람(1)',
    runImages: [human1Walk, human1Dash],
    idleImages: [human1Idle, human1Idle],
    hurtImage: human1Hurt,
    hurtFrames: 8,
    idleFrames: [8, 8],
    runFrames: [8, 8],
    frameWidth: 48,
    frameHeight: 30,
    bottomPadding: 0,
    hasVariants: true,
    flipX: false,
    speedFactor: 1.0,
    displayScale: 1.0,
  },
  {
    id: 'human2',
    name: '사람(2)',
    runImages: [human2Run],
    idleImages: [human2Idle],
    hurtImage: human2Hurt,
    hurtFrames: 8,
    idleFrames: [8],
    runFrames: [12],
    frameWidth: 32,
    frameHeight: 30,
    bottomPadding: 0,
    hasVariants: false,
    flipX: false,
    speedFactor: 1.0,
    displayScale: 1.0,
  },
  {
    id: 'car1',
    name: '자동차(1)',
    runImages: [car1Run],
    idleImages: [car1Run],
    hurtImage: car1Hurt,
    hurtFrames: 1,
    idleFrames: [5],
    runFrames: [5],
    frameWidth: 184,
    frameHeight: 68,
    bottomPadding: 0,
    hasVariants: false,
    flipX: false,
    speedFactor: 1.0,
    displayScale: 1.0,
  },
];

// "아무거나" 선택용 상수
export const RANDOM_PET_ID = 'random';

// 랜덤 펫 ID 선택 (PET_TYPES에서 무작위 1개)
export function resolveRandomPetId(): string {
  const idx = Math.floor(Math.random() * PET_TYPES.length);
  return PET_TYPES[idx].id;
}

// 펫 ID로 PetType 조회 (없으면 해골 기본값)
export function getPetType(id: string): PetType {
  return PET_TYPES.find(p => p.id === id) || PET_TYPES[0];
}

export interface MonitorConfig {
  cpu: boolean;
  memory: boolean;
  network: boolean;
  battery: boolean;
  showChargingIcon: boolean; // 펫 충전 아이콘 표시
  chargingIconSize: string;  // 충전 아이콘 크기: 'large'(50%) | 'medium'(40%) | 'small'(30%)
  chargingIconDistance: number; // 충전 아이콘 거리: -5 ~ 5 (음수=가까이, 양수=멀리)
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

// 폰트 색상 팔레트 (20색)
export const FONT_COLOR_PALETTE = [
  '#000000', '#808080', '#800000', '#FF0000', '#FF8C00',
  '#FFFF00', '#008000', '#00BFFF', '#0000FF', '#800080',
  '#FFFFFF', '#C0C0C0', '#A0522D', '#FFB6C1', '#FFC107',
  '#F5DEB3', '#7CFC00', '#008080', '#000080', '#404040',
];

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

// 폰트 색상의 밝기를 판단하여 아웃라인(text-shadow) 색상 반환
export function getOutlineColor(fontColor: string): string {
  const hex = fontColor.replace('#', '');
  const r = parseInt(hex.substring(0, 2), 16);
  const g = parseInt(hex.substring(2, 4), 16);
  const b = parseInt(hex.substring(4, 6), 16);
  // 상대 휘도 계산 (ITU-R BT.709)
  const luminance = (0.2126 * r + 0.7152 * g + 0.0722 * b) / 255;
  return luminance > 0.5 ? '#000000' : '#FFFFFF';
}

// 폰트 색상에 맞는 text-shadow 문자열 생성
export function getTextShadow(fontColor: string): string {
  const outline = getOutlineColor(fontColor);
  const shadowAlpha = outline === '#000000' ? '0.8' : '0.6';
  return `-1px -1px 0 ${outline}, 1px -1px 0 ${outline}, -1px 1px 0 ${outline}, 1px 1px 0 ${outline}, 0px 1px 2px rgba(${outline === '#000000' ? '0,0,0' : '255,255,255'}, ${shadowAlpha})`;
}

// 알림 조건 체크: 발화된 모든 메시지와 업데이트된 알림 목록 반환
export function checkAlarms(alarms: Alarm[], notificationDurationSec: number): { firedMessages: FiredNotification[]; updatedAlarms: Alarm[] } {
  const now = Date.now();
  const today = new Date().toISOString().slice(0, 10);
  const durationMs = notificationDurationSec * 1000;
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
        const targetMsAbs = timeToTodayMs(alarm.targetTime);
        if (now >= targetMsAbs && now <= targetMsAbs + durationMs) {
          // 알림 시간 범위 내: 발화
          firedMessages.push({ message: alarm.message, firedAt: now });
          changed = true;
          return { ...alarm, firedToday: today, enabled: false };
        }
        if (now > targetMsAbs + durationMs) {
          // 알림 시간 범위 초과: 발화 없이 만료 처리
          changed = true;
          return { ...alarm, firedToday: today, enabled: false };
        }
        return alarm;
      }
      case 'daily': {
        if (!alarm.targetTime || alarm.firedToday === today) return alarm;
        const targetMsDaily = timeToTodayMs(alarm.targetTime);
        if (now >= targetMsDaily && now <= targetMsDaily + durationMs) {
          // 알림 시간 범위 내: 발화
          firedMessages.push({ message: alarm.message, firedAt: now });
          changed = true;
          return { ...alarm, firedToday: today };
        }
        if (now > targetMsDaily + durationMs) {
          // 알림 시간 범위 초과: 발화 없이 만료 처리
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

// "HH:mm" 형식의 시간을 오늘 날짜 기준 밀리초(timestamp)로 변환
function timeToTodayMs(hhmm: string): number {
  const [h, m] = hhmm.split(':').map(Number);
  const d = new Date();
  d.setHours(h, m, 0, 0);
  return d.getTime();
}

// 메시지 조건 평가: 매칭되는 조건의 메시지 목록 반환
// 입력 배열은 priority 내림차순 정렬된 상태여야 함 (호출부에서 useMemo로 사전 정렬)
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
