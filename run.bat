@echo off
chcp 65001 >nul
setlocal
set PATH=%PATH%;C:\Users\jongcheol\.cargo\bin
cd /d %~dp0

echo [1/2] 의존성 확인 중... ✨
if not exist "node_modules" (
    echo [INFO] node_modules가 없습니다. 설치를 진행합니다...
    call npm install
)

echo.
echo [2/2] Tauri 앱 실행 (개발 모드)... 🚀
call npm run tauri dev
if %errorlevel% neq 0 (
    echo [ERROR] 앱 실행 중 오류가 발생했습니다. ❌
    pause
    exit /b %errorlevel%
)

echo.
echo 앱이 정상적으로 종료되었습니다. 👍
pause
