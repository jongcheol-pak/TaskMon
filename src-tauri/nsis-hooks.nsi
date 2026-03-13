; 기본 설치 경로를 %LOCALAPPDATA%\TaskBone 으로 변경
!macro NSIS_HOOK_PREINSTALL
  StrCpy $INSTDIR "$LOCALAPPDATA\TaskBone"
!macroend

; Windows API로 시스템 언어 감지 후 앱 표시 이름 변경
!macro NSIS_HOOK_POSTINSTALL
  System::Call 'kernel32::GetUserDefaultUILanguage() i .r0'
  ${If} $0 == 1042
    ; 프로그램 추가/제거 표시 이름 변경
    WriteRegStr SHCTX "Software\Microsoft\Windows\CurrentVersion\Uninstall\${PRODUCTNAME}" "DisplayName" "작업 뼈다귀"
    ; 시작 메뉴 바로가기 이름 변경
    Rename "$SMPROGRAMS\${PRODUCTNAME}.lnk" "$SMPROGRAMS\작업 뼈다귀.lnk"
  ${EndIf}
!macroend
