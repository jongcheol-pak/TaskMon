// 다국어 번역 리소스 및 번역 함수

export type Language = 'system' | 'ko' | 'en';

// 한글 리소스
const ko = {
  // 사이드바
  'sidebar.title': '설정',
  'sidebar.testMode': '테스트 모드',
  'sidebar.petColor': '펫 색상',
  'sidebar.monitoring': '모니터링',
  'sidebar.messages': '모니터링 메시지',
  'sidebar.alarm': '알림',
  'sidebar.font': '폰트',
  'sidebar.general': '설정',

  // 테스트 모드
  'test.title': '테스트 모드',
  'test.description': '실제 CPU 사용률 대신 수동으로 설정한 값을 사용합니다.',
  'test.enable': '테스트 모드 활성화',
  'test.virtualCpu': '가상 CPU 부하',

  // 펫 색상
  'color.title': '펫 색상 커스텀',
  'color.description': 'SVG 필터를 활용해 펫의 색상을 자유롭게 변경합니다.',
  'color.brightness': '추가 밝기 보정 (Brightness)',
  'color.opacity': '투명도',
  'color.reset': '초기화',

  // 모니터링
  'monitor.title': '모니터링 항목',
  'monitor.description': '마우스를 올렸을 때 말풍선에 표시할 항목을 선택합니다.',
  'monitor.cpu': 'CPU 사용률',
  'monitor.memory': '메모리 사용률',
  'monitor.network': '네트워크 속도',
  'monitor.battery': '배터리',
  'monitor.batteryNone': '없음',

  // 모니터링 메시지
  'msg.addTitle': '모니터링 메시지 추가',
  'msg.addDescription': '시스템 상태 조건에 따라 펫이 표시할 메시지를 설정합니다.',
  'msg.target': '대상',
  'msg.condition': '조건',
  'msg.value': '값',
  'msg.priority': '우선순위',
  'msg.priorityHelper': '높을수록 우선',
  'msg.message': '메시지',
  'msg.messagePlaceholder': '표시할 메시지를 입력하세요 (최대 50자)',
  'msg.add': '추가',
  'msg.listTitle': '등록된 메시지',
  'msg.empty': '등록된 모니터링 메시지가 없습니다.',
  'msg.targetCpu': 'CPU',
  'msg.targetMemory': '메모리',
  'msg.targetBattery': '배터리',
  'msg.targetNetDown': '네트워크 수신',
  'msg.targetNetUp': '네트워크 송신',
  'msg.condGt': '초과 (>)',
  'msg.condGe': '이상 (>=)',
  'msg.condLt': '미만 (<)',
  'msg.condLe': '이하 (<=)',
  'msg.condEq': '같음 (=)',
  'msg.rotateTitle': '메시지 표시 방식',
  'msg.showAll': '조건에 맞는 모든 메시지 표시',
  'msg.rotateIntervalLabel': '순환 간격 (초)',

  // 알림 - 표시 설정
  'alarm.displayTitle': '표시 설정',
  'alarm.displayDescription': '이동 중 말풍선에 표시할 문구 종류를 선택합니다.',
  'alarm.showMonitoring': '모니터링 문구 사용',
  'alarm.showNotification': '알림 문구 사용',
  'alarm.notificationPriority': '알림 우선',
  'alarm.notificationPriorityDesc': '모니터링 메시지 보다 우선 표시 됩니다.',
  'alarm.durationLabel': '알림 표시 시간 (초)',
  'alarm.modeLabel': '중복 알림 표시',
  'alarm.modeAll': '모두 표시',
  'alarm.modeFirst': '먼저 표시된 메시지 우선',
  'alarm.modeLatest': '최근 메시지 우선 표시',
  'alarm.apply': '적용',

  // 알림 - 추가
  'alarm.addTitle': '알림 추가',
  'alarm.typeLabel': '타입',
  'alarm.typeInterval': '일정 시간마다 반복',
  'alarm.typeAbsolute': '특정 시간에 알림',
  'alarm.typeDaily': '매일 특정 시간',
  'alarm.typeRelative': '지금부터 N시간 후',
  'alarm.typeHourly': '매시 N분마다',
  'alarm.intervalLabel': '간격 (분)',
  'alarm.timeLabel': '시간',
  'alarm.delayLabel': '지연',
  'alarm.hourlyLabel': '분 (0~59)',
  'alarm.messageLabel': '알림 문구',
  'alarm.messagePlaceholder': '알림 메시지를 입력하세요 (최대 50자)',
  'alarm.add': '추가',
  'alarm.unitHours': '시간',
  'alarm.unitMinutes': '분',

  // 알림 - 목록
  'alarm.listTitle': '등록된 알림',
  'alarm.empty': '등록된 알림이 없습니다.',

  // 알림 타입 라벨 (목록 배지용)
  'alarm.badge.interval': '반복',
  'alarm.badge.absolute': '특정 시간',
  'alarm.badge.daily': '매일',
  'alarm.badge.relative': '타이머',
  'alarm.badge.hourly': '매시',

  // 알림 상세 텍스트
  'alarm.detail.everyNMin': '{n}분마다',
  'alarm.detail.daily': '매일 {time}',
  'alarm.detail.hoursMinAfter': '{h}시간 {m}분 후',
  'alarm.detail.hoursAfter': '{h}시간 후',
  'alarm.detail.minAfter': '{n}분 후',
  'alarm.detail.hourlyAt': '매시 {n}분',

  // 일반 설정
  'general.title': '설정',
  'general.pollingLabel': '폴링 간격 (초)',
  'general.autoStart': '자동 실행',
  'general.bubbleLabel': '말풍선 사용',
  'general.apply': '적용',
  'font.title': '폰트',
  'general.fontSize': '폰트 크기',
  'general.fontFamily': '폰트',
  'general.language': '언어',
  'general.langSystem': '시스템 언어',
  'general.langKo': '한국어',
  'general.langEn': 'English',
  'general.fontDefault': '기본',

  // 저장/가져오기
  'export.save': '저장',
  'export.import': '가져오기',
  'export.confirmDelete': '기존 목록은 삭제 됩니다.',
} as Record<string, string>;

// 영문 리소스
const en: Record<string, string> = {
  // 사이드바
  'sidebar.title': 'Settings',
  'sidebar.testMode': 'Test Mode',
  'sidebar.petColor': 'Pet Color',
  'sidebar.monitoring': 'Monitoring',
  'sidebar.messages': 'Monitoring Messages',
  'sidebar.alarm': 'Alarm',
  'sidebar.font': 'Font',
  'sidebar.general': 'Settings',

  // 테스트 모드
  'test.title': 'Test Mode',
  'test.description': 'Use a manually set value instead of actual CPU usage.',
  'test.enable': 'Enable Test Mode',
  'test.virtualCpu': 'Virtual CPU Load',

  // 펫 색상
  'color.title': 'Pet Color Custom',
  'color.description': 'Freely change the pet\'s color using SVG filters.',
  'color.brightness': 'Brightness Adjustment',
  'color.opacity': 'Opacity',
  'color.reset': 'Reset',

  // 모니터링
  'monitor.title': 'Monitoring Items',
  'monitor.description': 'Select items to display in the speech bubble on hover.',
  'monitor.cpu': 'CPU Usage',
  'monitor.memory': 'Memory Usage',
  'monitor.network': 'Network Speed',
  'monitor.battery': 'Battery',
  'monitor.batteryNone': 'N/A',

  // 모니터링 메시지
  'msg.addTitle': 'Add Monitoring Message',
  'msg.addDescription': 'Set messages for the pet to display based on system status conditions.',
  'msg.target': 'Target',
  'msg.condition': 'Condition',
  'msg.value': 'Value',
  'msg.priority': 'Priority',
  'msg.priorityHelper': 'Higher = first',
  'msg.message': 'Message',
  'msg.messagePlaceholder': 'Enter message to display (max 50 chars)',
  'msg.add': 'Add',
  'msg.listTitle': 'Registered Messages',
  'msg.empty': 'No monitoring messages registered.',
  'msg.targetCpu': 'CPU',
  'msg.targetMemory': 'Memory',
  'msg.targetBattery': 'Battery',
  'msg.targetNetDown': 'Network Down',
  'msg.targetNetUp': 'Network Up',
  'msg.condGt': 'Greater than (>)',
  'msg.condGe': 'Greater or equal (>=)',
  'msg.condLt': 'Less than (<)',
  'msg.condLe': 'Less or equal (<=)',
  'msg.condEq': 'Equal (=)',
  'msg.rotateTitle': 'Message Display Mode',
  'msg.showAll': 'Show all matching messages',
  'msg.rotateIntervalLabel': 'Rotation interval (sec)',

  // 알림 - 표시 설정
  'alarm.displayTitle': 'Display Settings',
  'alarm.displayDescription': 'Select the type of text to display in the speech bubble while moving.',
  'alarm.showMonitoring': 'Show Monitoring Text',
  'alarm.showNotification': 'Show Notification Text',
  'alarm.notificationPriority': 'Notification Priority',
  'alarm.notificationPriorityDesc': 'Displayed with priority over monitoring messages.',
  'alarm.durationLabel': 'Notification Duration (sec)',
  'alarm.modeLabel': 'Overlap Mode',
  'alarm.modeAll': 'Show All',
  'alarm.modeFirst': 'First Message Priority',
  'alarm.modeLatest': 'Latest Message Priority',
  'alarm.apply': 'Apply',

  // 알림 - 추가
  'alarm.addTitle': 'Add Alarm',
  'alarm.typeLabel': 'Type',
  'alarm.typeInterval': 'Repeat at Interval',
  'alarm.typeAbsolute': 'At Specific Time',
  'alarm.typeDaily': 'Daily at Specific Time',
  'alarm.typeRelative': 'After N Hours',
  'alarm.typeHourly': 'Every Hour at N Min',
  'alarm.intervalLabel': 'Interval (min)',
  'alarm.timeLabel': 'Time',
  'alarm.delayLabel': 'Delay',
  'alarm.hourlyLabel': 'Min (0~59)',
  'alarm.messageLabel': 'Notification Text',
  'alarm.messagePlaceholder': 'Enter alarm message (max 50 chars)',
  'alarm.add': 'Add',
  'alarm.unitHours': 'hours',
  'alarm.unitMinutes': 'min',

  // 알림 - 목록
  'alarm.listTitle': 'Registered Alarms',
  'alarm.empty': 'No alarms registered.',

  // 알림 타입 라벨
  'alarm.badge.interval': 'Repeat',
  'alarm.badge.absolute': 'Specific',
  'alarm.badge.daily': 'Daily',
  'alarm.badge.relative': 'Timer',
  'alarm.badge.hourly': 'Hourly',

  // 알림 상세 텍스트
  'alarm.detail.everyNMin': 'Every {n} min',
  'alarm.detail.daily': 'Daily {time}',
  'alarm.detail.hoursMinAfter': 'After {h}h {m}m',
  'alarm.detail.hoursAfter': 'After {h}h',
  'alarm.detail.minAfter': 'After {n} min',
  'alarm.detail.hourlyAt': 'At {n} min/hr',

  // 일반 설정
  'general.title': 'Settings',
  'general.pollingLabel': 'Polling Interval (sec)',
  'general.autoStart': 'Auto Start',
  'general.bubbleLabel': 'Speech Bubble',
  'general.apply': 'Apply',
  'font.title': 'Font',
  'general.fontSize': 'Font Size',
  'general.fontFamily': 'Font',
  'general.language': 'Language',
  'general.langSystem': 'System Language',
  'general.langKo': '한국어',
  'general.langEn': 'English',
  'general.fontDefault': 'Default',

  // 저장/가져오기
  'export.save': 'Save',
  'export.import': 'Import',
  'export.confirmDelete': 'Existing list will be deleted.',
};

const translations: Record<string, Record<string, string>> = { ko, en };

// 시스템 언어 감지: 한국어면 'ko', 그 외 모두 'en'
export function detectSystemLanguage(): 'ko' | 'en' {
  const lang = navigator.language || '';
  if (lang.startsWith('ko')) return 'ko';
  return 'en';
}

// 실제 적용할 언어 결정
export function resolveLanguage(setting: Language): 'ko' | 'en' {
  if (setting === 'system') return detectSystemLanguage();
  return setting;
}

// 번역 함수 생성
export function createT(lang: 'ko' | 'en') {
  const dict = translations[lang] || translations['en'];
  return (key: string, params?: Record<string, string | number>): string => {
    let text = dict[key] || ko[key] || key;
    if (params) {
      for (const [k, v] of Object.entries(params)) {
        text = text.replace(`{${k}}`, String(v));
      }
    }
    return text;
  };
}
