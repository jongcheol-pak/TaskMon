@echo off
chcp 65001 >nul
setlocal
set PATH=%PATH%;C:\Users\jongcheol\.cargo\bin
cd /d %~dp0

echo [1/4] 의존성 설치 확인 중... ✨
call npm install
if %errorlevel% neq 0 (
    echo [ERROR] 의존성 설치에 실패했습니다. 확인 부탁드려요! ❌
    pause
    exit /b %errorlevel%
)

echo.
echo [2/4] Tauri 프로젝트 빌드 시작... 🚀
call npm run tauri build
if %errorlevel% neq 0 (
    echo [ERROR] 빌드 중 오류가 발생했습니다. 로그를 확인해 주세요! ❌
    pause
    exit /b %errorlevel%
)

echo.
echo [3/4] 설치 파일 이름 변경 중... 📝
call node scripts\rename-installer.cjs
if %errorlevel% neq 0 (
    echo [ERROR] 설치 파일 이름 변경에 실패했습니다. ❌
    pause
    exit /b %errorlevel%
)

echo.
echo [4/4] 빌드 완료! 성공적으로 마무리되었습니다. 🎉
echo 결과물은 src-tauri\target\release\bundle\nsis 에서 확인하실 수 있습니다.
echo.
pause
