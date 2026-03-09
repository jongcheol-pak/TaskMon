import { useEffect, useRef, useState } from "react";
import { listen, emit } from "@tauri-apps/api/event";
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

function App() {
  const speedRef = useRef(1);
  const [isHovered, setIsHovered] = useState(false);
  const [isHurt, setIsHurt] = useState(false);
  const [cpuUsage, setCpuUsage] = useState(0);
  const [memUsage, setMemUsage] = useState(0);
  const [isTestMode, setIsTestMode] = useState(false);
  const [testCpuValue, setTestCpuValue] = useState(50);
  const [windowLabel, setWindowLabel] = useState<string>("");
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

  const skeletonRef = useRef<HTMLDivElement>(null);
  const animRef = useRef<Animation | null>(null);
  const hurtTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

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

    return () => {
      unlisten.then((f) => f());
      unlistenMem.then((f) => f());
      unlistenTestMode.then((f) => f());
      unlistenColor.then((f) => f());
    };
  }, []);

  // runVariant 및 색상 필터 변경 시 localStorage에 저장
  useEffect(() => {
    localStorage.setItem('petRunVariant', String(runVariant));
    localStorage.setItem('petHue', String(hue));
    localStorage.setItem('petSaturation', String(saturation));
    localStorage.setItem('petBrightness', String(brightness));
  }, [runVariant, hue, saturation, brightness]);

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

  // 클래스 결정: hurt > idle > run
  let skeletonClass = 'skeleton';
  if (isHurt) skeletonClass += ' hurt';
  else if (isHovered) skeletonClass += ' idle';

  // run 또는 idle 상태일 때 inline style로 이미지 및 색상 필터 적용
  const petStyle = (() => {
    // 모든 영역에 균일하게 색을 입히기 위한 고도화된 필터 체인:
    // 1. grayscale(1): 원본 색 제거
    // 2. brightness(0.8) & contrast(1.2): 색이 잘 스며들도록 베이스 톤 조정
    // 3. sepia(1): 채색 가능한 베이스 입히기
    // 4. hue-rotate(hue - 40): sepia의 노란기를 상쇄하여 피커와 색상 일치 (sepia는 약 40도임)
    // 5. saturate & brightness: 최종 색감 및 밝기 조정
    const adjustedHue = hue - 40;
    const filter = `grayscale(1) brightness(0.8) contrast(1.2) sepia(1) hue-rotate(${adjustedHue}deg) saturate(${saturation * 2}%) brightness(${brightness / 80})`;
    
    if (isHurt) return { filter };
    if (isHovered) return { backgroundImage: `url('${IDLE_IMAGES[runVariant]}')`, filter };
    return { backgroundImage: `url('${RUN_IMAGES[runVariant]}')`, filter };
  })();

  if (windowLabel === "settings") {
    return (
      <div className="settings-container">
        <h2>환경 설정</h2>
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

        <div className="settings-section">
          <h3>펫 색상 커스텀</h3>
          <p className="description">SVG 필터를 활용해 펫의 색상을 자유롭게 변경합니다.</p>
          
          <div className="color-controls">
            {/* 2D 컬러 피커 */}
            <div 
              className="color-picker-2d"
              onMouseDown={(e) => {
                const handleMove = (moveEvent: MouseEvent) => {
                  const rect = e.currentTarget.getBoundingClientRect();
                  const x = Math.max(0, Math.min(moveEvent.clientX - rect.left, rect.width));
                  const y = Math.max(0, Math.min(moveEvent.clientY - rect.top, rect.height));
                  
                  const newHue = Math.round((x / rect.width) * 360);
                  // Y축이 아래로 갈수록 흰색이 되도록 설계됨 (Saturation 감소 혹은 Brightness 증가)
                  // 사용자 이미지의 경우 아래가 흰색이므로, Y가 커질수록 Saturation은 낮아지거나 Brightness가 높아짐
                  // 여기서는 Y축을 Saturation(100 -> 0)으로 매핑
                  const newSaturation = Math.round(100 - (y / rect.height) * 100);
                  
                  setHue(newHue);
                  setSaturation(newSaturation);
                  invoke("update_pet_color", { hue: newHue, saturation: newSaturation, brightness });
                };

                const handleUp = () => {
                  window.removeEventListener('mousemove', handleMove);
                  window.removeEventListener('mouseup', handleUp);
                };

                window.addEventListener('mousemove', handleMove);
                window.addEventListener('mouseup', handleUp);
                
                // 첫 클릭 지점도 반영
                const rect = e.currentTarget.getBoundingClientRect();
                const x = Math.max(0, Math.min(e.clientX - rect.left, rect.width));
                const y = Math.max(0, Math.min(e.clientY - rect.top, rect.height));
                const newHue = Math.round((x / rect.width) * 360);
                const newSaturation = Math.round(100 - (y / rect.height) * 100);
                setHue(newHue);
                setSaturation(newSaturation);
                invoke("update_pet_color", { hue: newHue, saturation: newSaturation, brightness });
              }}
            >
              {/* 현재 선택 위치 표시 포인터 */}
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
                  invoke("update_pet_color", { hue, saturation, brightness: val });
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
                invoke("update_pet_color", { hue: 0, saturation: 100, brightness: 100 });
              }}
            >
              색상 초기화
            </button>
          </div>
        </div>
      </div>
    );
  }

  return (
    <div className="pet-container">
      {isHovered && !isHurt && (
        <div className="speech-bubble">
          <div className="stat-row">
            <span>🖥 CPU&nbsp; {isTestMode ? `${testCpuValue}% (Test)` : `${cpuUsage}%`}</span>
            <span>💾 MEM {memUsage}%</span>
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
