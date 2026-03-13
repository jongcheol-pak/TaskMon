import { useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import "./App.css";
import MainWindow from "./MainWindow";
import SettingsWindow from "./SettingsWindow";

// 윈도우 라벨로 메인/설정 컴포넌트 분기 (각 WebView 인스턴스에서 독립 실행)
function App() {
  const [windowLabel] = useState<string>(() => {
    try {
      return getCurrentWindow().label;
    } catch {
      return "settings"; // Tauri 런타임 미감지 시 개발용 기본값
    }
  });

  if (windowLabel === "settings") return <SettingsWindow />;
  return <MainWindow />;
}

export default App;
