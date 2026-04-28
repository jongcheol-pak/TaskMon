; 기본 설치 경로를 %LOCALAPPDATA%\TaskMon 으로 변경
!macro NSIS_HOOK_PREINSTALL
  StrCpy $INSTDIR "$LOCALAPPDATA\TaskMon"
!macroend

; Windows API로 시스템 언어 감지 후 앱 표시 이름 및 시작 메뉴 바로가기 한글화
!macro NSIS_HOOK_POSTINSTALL
  System::Call 'kernel32::GetUserDefaultUILanguage() i .r0'
  ${If} $0 == 1042
    ; 프로그램 추가/제거 표시 이름 변경
    WriteRegStr SHCTX "Software\Microsoft\Windows\CurrentVersion\Uninstall\${PRODUCTNAME}" "DisplayName" "테스크몬"
    ; 시작 메뉴 바로가기 이름 변경 (재설치 시 기존 한글 바로가기 충돌 방지)
    Delete "$SMPROGRAMS\테스크몬.lnk"
    Rename "$SMPROGRAMS\${PRODUCTNAME}.lnk" "$SMPROGRAMS\테스크몬.lnk"
  ${EndIf}
!macroend

; 제거 시 한글 바로가기 삭제 (표준 제거는 영문 이름만 삭제하므로 한글 이름도 삭제)
!macro NSIS_HOOK_PREUNINSTALL
  Delete "$SMPROGRAMS\테스크몬.lnk"
!macroend

; 제거 후 설치 폴더(%LocalAppData%\TaskMon) 내 잔여 파일/폴더 모두 정리
; WebView2 런타임이 만든 EBWebView 폴더 등 NSIS가 추적하지 않는 항목까지 함께 삭제한다.
!macro NSIS_HOOK_POSTUNINSTALL
  RMDir /r "$LOCALAPPDATA\TaskMon"
!macroend
