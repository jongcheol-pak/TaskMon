import { useEffect, useRef, useState, useCallback } from "react";
import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import "./App.css";

// Vite가 빌드 시 올바른 URL로 변환하도록 import로 참조
import runUnarmed from './assets/Skeleton_Default_Run_Unarmed.png';
import runSword from './assets/Skeleton_Default_Run_Sword.png';
import runSwordShield from './assets/Skeleton_Default_X_Sword+Shield.png';
import idleUnarmed from './assets/Skeleton_Default_Idle_Unarmed.png';
import idleSword from './assets/Skeleton_Default_Idle_Sword.png';
import idleSwordShield from './assets/idle.png';

// 달리기 이미지 3종 (우클릭으로 순환: 0→1→2→0)
const RUN_IMAGES = [runUnarmed, runSword, runSwordShield] as const;
// 아이들 이미지 3종 (runVariant와 동일 인덱스)
const IDLE_IMAGES = [idleUnarmed, idleSword, idleSwordShield] as const;

interface MonitorConfig {
  cpu: boolean;
  memory: boolean;
  network: boolean;
  battery: boolean;
}

// condition: 오타 방지를 위해 명확한 단어 사용
type MessageCondition = "less_than" | "greater_than" | "less_equal" | "greater_equal" | "equal";

interface PetMessage {
  target: string;         // "cpu" | "memory" | "battery" | "network_down" | "network_up"
  condition: MessageCondition;
  value: number;
  priority: number;       // 높을수록 우선
  text: string;
}

// 메시지 조건 평가: 매칭되는 조건 중 priority가 가장 높은 1개만 반환
// monitorConfig에서 체크 안 된 항목은 평가에서 제외
function evaluateMessage(
  messages: PetMessage[],
  cpu: number,
  mem: number,
  battery: number,
  netDown: number,
  netUp: number,
  config: MonitorConfig
): string | null {
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

  if (matched.length === 0) return null;
  matched.sort((a, b) => b.priority - a.priority);
  return matched[0].text;
}

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes}B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)}KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)}MB`;
}

function App() {
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
  const [windowLabel, setWindowLabel] = useState<string>("");
  const [monitorConfig, setMonitorConfig] = useState<MonitorConfig>(() => {
    const saved = localStorage.getItem('monitorConfig');
    return saved ? JSON.parse(saved) : { cpu: true, memory: true, network: false, battery: false };
  });
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

  // 메시지 시스템 상태
  const [petMessages, setPetMessages] = useState<PetMessage[]>([]);
  const [petMessage, setPetMessage] = useState<string | null>(null);

  // 설정: 폴링 간격 (초)
  const [pollingInput, setPollingInput] = useState<string>(() => {
    const saved = localStorage.getItem('pollingInterval');
    return saved !== null ? saved : "1";
  });

  // 설정: 말풍선 사용 여부
  const [bubbleEnabled, setBubbleEnabled] = useState<boolean>(() => {
    const saved = localStorage.getItem('bubbleEnabled');
    return saved !== null ? saved === 'true' : true;
  });

  const skeletonRef = useRef<HTMLDivElement>(null);
  const animRef = useRef<Animation | null>(null);
  const hurtTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const colorRafRef = useRef<number | null>(null);
  const pendingColorRef = useRef<{hue: number, saturation: number, brightness: number} | null>(null);

  useEffect(() => {
    try {
      const win = getCurrentWindow();
      if (win) {
        setWindowLabel(win.label);
      }
    } catch (e) {
      console.warn("Tauri runtime not detected, defaulting to settings view for development.", e);
      setWindowLabel("settings");
    }
  }, []);

  // 메시지 파일 로딩 (앱 시작 시 1회)
  useEffect(() => {
    invoke<PetMessage[]>("load_messages")
      .then((msgs) => setPetMessages(msgs))
      .catch(() => setPetMessages([]));
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

  // 메시지 평가: 모니터링 값 변경 시마다 즉시 평가 (Rust 폴링 1초 간격 = 초당 최대 1회)
  useEffect(() => {
    if (petMessages.length === 0) {
      setPetMessage(null);
      return;
    }

    const effectiveCpu = isTestMode ? testCpuValue : cpuUsage;
    setPetMessage(evaluateMessage(petMessages, effectiveCpu, memUsage, batteryPercent, networkDown, networkUp, monitorConfig));
  }, [cpuUsage, memUsage, batteryPercent, networkDown, networkUp, petMessages, isTestMode, testCpuValue, monitorConfig]);

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
    const unlistenColor = listen<{hue: number, saturation: number, brightness: number}>("color-update", (event) => {
      setHue(event.payload.hue);
      setSaturation(event.payload.saturation);
      setBrightness(event.payload.brightness);
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

    return () => {
      unlisten.then((f) => f());
      unlistenMem.then((f) => f());
      unlistenTestMode.then((f) => f());
      unlistenColor.then((f) => f());
      unlistenNetwork.then((f) => f());
      unlistenBattery.then((f) => f());
      unlistenMonitorConfig.then((f) => f());
      unlistenBubble.then((f) => f());
    };
  }, []);

  // runVariant 및 색상 필터 변경 시 localStorage에 저장 (500ms 디바운스로 디스크 I/O 최소화)
  useEffect(() => {
    const timer = setTimeout(() => {
      localStorage.setItem('petRunVariant', String(runVariant));
      localStorage.setItem('petHue', String(hue));
      localStorage.setItem('petSaturation', String(saturation));
      localStorage.setItem('petBrightness', String(brightness));
      localStorage.setItem('monitorConfig', JSON.stringify(monitorConfig));
      localStorage.setItem('bubbleEnabled', String(bubbleEnabled));
    }, 500);
    return () => clearTimeout(timer);
  }, [runVariant, hue, saturation, brightness, monitorConfig, bubbleEnabled]);

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

  // 색상 업데이트 IPC를 rAF로 throttle (드래그 중 초당 60+ 회 → 프레임당 1회)
  const scheduleColorUpdate = useCallback((h: number, s: number, b: number) => {
    pendingColorRef.current = { hue: h, saturation: s, brightness: b };
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

  // 컴포넌트 언마운트 시 pending rAF 정리
  useEffect(() => {
    return () => {
      if (colorRafRef.current !== null) {
        cancelAnimationFrame(colorRafRef.current);
      }
    };
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
    invoke("update_monitor_config", updated);
  };

  // 클래스 결정: hurt > idle > run
  let skeletonClass = 'skeleton';
  if (isHurt) skeletonClass += ' hurt';
  else if (isHovered) skeletonClass += ' idle';

  // run 또는 idle 상태일 때 inline style로 이미지 및 색상 필터 적용
  const isDefaultColor = hue === 0 && saturation === 100 && brightness === 100;

  const petStyle = (() => {
    // 초기화 상태(기본값)이면 필터 없이 원본 이미지 그대로 표시
    if (isDefaultColor) {
      if (isHurt) return {};
      if (isHovered) return { backgroundImage: `url('${IDLE_IMAGES[runVariant]}')` };
      return { backgroundImage: `url('${RUN_IMAGES[runVariant]}')` };
    }

    // 색상 필터 체인:
    // 1. grayscale(1): 원본 색 제거
    // 2. sepia(1): 채색 가능한 베이스 입히기
    // 3. hue-rotate(hue - 50): sepia 기본색(~50도)을 상쇄하여 피커 색상과 일치
    // 4. saturate: 채도 강하게 적용
    // 5. brightness: 최종 밝기 조정
    const adjustedHue = hue - 50;
    const filter = `grayscale(1) sepia(1) hue-rotate(${adjustedHue}deg) saturate(${saturation * 4}%) brightness(${brightness / 100})`;

    if (isHurt) return { filter };
    if (isHovered) return { backgroundImage: `url('${IDLE_IMAGES[runVariant]}')`, filter };
    return { backgroundImage: `url('${RUN_IMAGES[runVariant]}')`, filter };
  })();

  const [settingsTab, setSettingsTab] = useState<string>("test");

  if (windowLabel === "settings") {
    return (
      <div className="settings-layout">
        <nav className="settings-sidebar">
          <h2 className="sidebar-title">설정</h2>
          <ul className="sidebar-menu">
            <li>
              <button
                className={`sidebar-item ${settingsTab === "test" ? "active" : ""}`}
                onClick={() => setSettingsTab("test")}
              >
                테스트 모드
              </button>
            </li>
            <li>
              <button
                className={`sidebar-item ${settingsTab === "color" ? "active" : ""}`}
                onClick={() => setSettingsTab("color")}
              >
                펫 색상
              </button>
            </li>
            <li>
              <button
                className={`sidebar-item ${settingsTab === "monitoring" ? "active" : ""}`}
                onClick={() => setSettingsTab("monitoring")}
              >
                모니터링
              </button>
            </li>
            <li>
              <button
                className={`sidebar-item ${settingsTab === "general" ? "active" : ""}`}
                onClick={() => setSettingsTab("general")}
              >
                설정
              </button>
            </li>
          </ul>
        </nav>
        <main className="settings-content">
          {settingsTab === "test" && (
            <div className="settings-section">
              <h3>테스트 모드</h3>
              <p className="description">실제 CPU 사용률 대신 수동으로 설정한 값을 사용합니다.</p>
              <div className="test-controls-vertical">
                <label className="test-toggle">
                  <input
                    type="checkbox"
                    checked={isTestMode}
                    onChange={handleTestModeToggle}
                  />
                  <span>테스트 모드 활성화</span>
                </label>
                <div className={isTestMode ? "slider-group" : "slider-group disabled"}>
                  <div className="slider-header">
                    <span>가상 CPU 부하</span>
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

          {settingsTab === "color" && (
            <div className="settings-section">
              <h3>펫 색상 커스텀</h3>
              <p className="description">SVG 필터를 활용해 펫의 색상을 자유롭게 변경합니다.</p>

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
                      scheduleColorUpdate(newHue, newSaturation, brightness);
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
                    <span>추가 밝기 보정 (Brightness)</span>
                    <span className="value-badge">{brightness}%</span>
                  </div>
                  <input
                    type="range" min="0" max="200" value={brightness}
                    onChange={(e) => {
                      const val = Number(e.target.value);
                      setBrightness(val);
                      scheduleColorUpdate(hue, saturation, val);
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
                    invoke("update_pet_color", { hue: 0, saturation: 100, brightness: 100 }); // 초기화는 즉시 반영
                  }}
                >
                  초기화
                </button>
              </div>
            </div>
          )}

          {settingsTab === "monitoring" && (
            <div className="settings-section">
              <h3>모니터링 항목</h3>
              <p className="description">마우스를 올렸을 때 말풍선에 표시할 항목을 선택합니다.</p>
              <div className="monitor-checklist">
                <label className="monitor-item">
                  <input type="checkbox" checked={monitorConfig.cpu} onChange={() => handleMonitorToggle('cpu')} />
                  <span className="monitor-label">CPU 사용률</span>
                  <span className="monitor-preview">{cpuUsage}%</span>
                </label>
                <label className="monitor-item">
                  <input type="checkbox" checked={monitorConfig.memory} onChange={() => handleMonitorToggle('memory')} />
                  <span className="monitor-label">메모리 사용률</span>
                  <span className="monitor-preview">{memUsage}%</span>
                </label>
                <label className="monitor-item">
                  <input type="checkbox" checked={monitorConfig.network} onChange={() => handleMonitorToggle('network')} />
                  <span className="monitor-label">네트워크 속도</span>
                  <span className="monitor-preview">{formatBytes(networkDown)}/s</span>
                </label>
                <label className="monitor-item">
                  <input type="checkbox" checked={monitorConfig.battery} onChange={() => handleMonitorToggle('battery')} />
                  <span className="monitor-label">배터리</span>
                  <span className="monitor-preview">{batteryPercent >= 0 ? `${batteryPercent}%` : '없음'}</span>
                </label>
              </div>
            </div>
          )}

          {settingsTab === "general" && (
            <div className="settings-section">
              <h3>설정</h3>
              <div className="general-settings">
                <div className="setting-item">
                  <span className="setting-label">폴링 간격 (초)</span>
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
                      적용
                    </button>
                  </div>
                </div>
                <label className="setting-item">
                  <input
                    type="checkbox"
                    checked={bubbleEnabled}
                    onChange={(e) => {
                      setBubbleEnabled(e.target.checked);
                      invoke("update_bubble_enabled", { enabled: e.target.checked });
                    }}
                  />
                  <span className="setting-label">말풍선 사용</span>
                </label>
              </div>
            </div>
          )}
        </main>
      </div>
    );
  }

  return (
    <div className="pet-container">
      {/* 이동(run) 중: 조건 메시지 표시 (말풍선 사용 시만) */}
      {bubbleEnabled && !isHovered && !isHurt && petMessage && (
        <div className="speech-bubble message-bubble">
          <div className="pet-message">{petMessage}</div>
        </div>
      )}
      {/* hover(idle) 중: 모니터링 수치 표시 */}
      {isHovered && !isHurt && (monitorConfig.cpu || monitorConfig.memory || monitorConfig.network || monitorConfig.battery) && (
        <div className="speech-bubble">
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

export default App;
