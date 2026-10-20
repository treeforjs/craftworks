Unicode True
!include "MUI2.nsh"

!ifndef VERSION
  !error "Pass /DVERSION=<version>"
!endif
!ifndef PACKAGE_DIR
  !error "Pass /DPACKAGE_DIR=<staging folder>"
!endif
!ifndef OUT_FILE
  !error "Pass /DOUT_FILE=<installer path>"
!endif

Name "Craftworks"
OutFile "${OUT_FILE}"
InstallDir "$LOCALAPPDATA\Programs\Craftworks"
InstallDirRegKey HKCU "Software\Craftworks" "InstallDir"
RequestExecutionLevel user
SetCompressor /SOLID lzma
BrandingText "Craftworks"

!define MUI_ABORTWARNING
!define MUI_FINISHPAGE_RUN "$INSTDIR\craftworks.exe"
!define MUI_FINISHPAGE_RUN_TEXT "Open Craftworks"
!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "English"

Section "Craftworks"
  SetShellVarContext current
  SetOutPath "$INSTDIR"
  ClearErrors
  File "${PACKAGE_DIR}\craftworks.exe"
  IfErrors 0 +3
    MessageBox MB_OK|MB_ICONSTOP "Close Craftworks, then run this installer again."
    Abort
  File "${PACKAGE_DIR}\LICENSE-MIT"
  File "${PACKAGE_DIR}\LICENSE-APACHE"
  File "${PACKAGE_DIR}\README.md"
  WriteUninstaller "$INSTDIR\Uninstall.exe"
  CreateShortcut "$SMPROGRAMS\Craftworks.lnk" "$INSTDIR\craftworks.exe"
  WriteRegStr HKCU "Software\Craftworks" "InstallDir" "$INSTDIR"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\Craftworks" "DisplayName" "Craftworks"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\Craftworks" "DisplayVersion" "${VERSION}"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\Craftworks" "Publisher" "Craftworks contributors"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\Craftworks" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\Craftworks" "UninstallString" '$\"$INSTDIR\Uninstall.exe$\"'
  WriteRegDWORD HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\Craftworks" "NoModify" 1
  WriteRegDWORD HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\Craftworks" "NoRepair" 1
SectionEnd

Section "Uninstall"
  SetShellVarContext current
  ClearErrors
  Delete "$INSTDIR\craftworks.exe"
  IfErrors 0 +3
    MessageBox MB_OK|MB_ICONSTOP "Close Craftworks, then run this uninstaller again."
    Abort
  Delete "$SMPROGRAMS\Craftworks.lnk"
  Delete "$INSTDIR\LICENSE-MIT"
  Delete "$INSTDIR\LICENSE-APACHE"
  Delete "$INSTDIR\README.md"
  Delete "$INSTDIR\THIRD-PARTY-NOTICES.txt"
  Delete "$INSTDIR\Uninstall.exe"
  RMDir "$INSTDIR"
  DeleteRegKey HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\Craftworks"
  DeleteRegKey HKCU "Software\Craftworks"
  ; Managed apps and user preferences are deliberately retained.
SectionEnd
