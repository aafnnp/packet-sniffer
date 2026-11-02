; Packet Sniffer 的 Windows 安装包（NSIS 3）。
;
; 用法（makensis 的相对路径以脚本所在目录为基准，因此下面一律传绝对路径）：
;   makensis \
;     -DAPP_VERSION=0.1.0 \
;     -DOUTFILE=<绝对路径>/PacketSniffer-setup.exe \
;     -DSTAGE_DIR=<仓库根>/dist/staging \
;     -DDOCS_DIR=<仓库根> \
;     [-DSEP=/] \
;     packaging/windows/installer.nsi
;
; dist/staging/ 内需先放好 packet-sniffer.exe 与 packet-sniffer-ui.exe。
; Windows 下路径用反斜杠（FindFirstFile 不认正斜杠）；在 macOS/Linux 上验证时加 -DSEP=/。

!ifndef APP_VERSION
  !define APP_VERSION "0.0.0"
!endif

!ifndef OUTFILE
  !error "缺少 OUTFILE：请用 -DOUTFILE=<绝对路径> 指定安装包输出位置"
!endif

!ifndef STAGE_DIR
  !error "缺少 STAGE_DIR：请用 -DSTAGE_DIR=<绝对路径> 指向存放 packet-sniffer.exe 的目录"
!endif

!ifndef DOCS_DIR
  !error "缺少 DOCS_DIR：请用 -DDOCS_DIR=<绝对路径> 指向仓库根目录（README.md / LICENSE / assets 所在处）"
!endif

; Windows 的 FindFirstFile 不认正斜杠，所以默认用 \；POSIX 版 makensis 本地验证时传 -DSEP=/
!ifndef SEP
  !define SEP "\"
!endif

!define APP_NAME "Packet Sniffer"
!define APP_PUBLISHER "aafnnp"
!define APP_EXE "packet-sniffer-ui.exe"
!define APP_CLI "packet-sniffer.exe"
!define APP_ICON "${DOCS_DIR}${SEP}assets${SEP}icons${SEP}packet-sniffer.ico"
!define UNINST_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\PacketSniffer"

Name "${APP_NAME} ${APP_VERSION}"
OutFile "${OUTFILE}"
InstallDir "$PROGRAMFILES64\${APP_NAME}"
InstallDirRegKey HKLM "${UNINST_KEY}" "InstallLocation"
RequestExecutionLevel admin
Unicode true
SetCompressor /SOLID lzma

!include "MUI2.nsh"

!define MUI_ABORTWARNING
!define MUI_ICON "${APP_ICON}"
!define MUI_UNICON "${APP_ICON}"
!define MUI_FINISHPAGE_RUN "$INSTDIR\${APP_EXE}"
!define MUI_FINISHPAGE_RUN_TEXT "立即启动 ${APP_NAME}"

!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH

!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES

!insertmacro MUI_LANGUAGE "English"
!insertmacro MUI_LANGUAGE "SimpChinese"

Section "Install"
  SetOutPath "$INSTDIR"
  File "${STAGE_DIR}${SEP}packet-sniffer.exe"
  File "${STAGE_DIR}${SEP}packet-sniffer-ui.exe"
  File "${DOCS_DIR}${SEP}README.md"
  File "${DOCS_DIR}${SEP}LICENSE"

  CreateDirectory "$SMPROGRAMS\${APP_NAME}"
  CreateShortCut "$SMPROGRAMS\${APP_NAME}\${APP_NAME}.lnk" "$INSTDIR\${APP_EXE}"
  CreateShortCut "$DESKTOP\${APP_NAME}.lnk" "$INSTDIR\${APP_EXE}"

  WriteUninstaller "$INSTDIR\uninstall.exe"

  WriteRegStr HKLM "${UNINST_KEY}" "DisplayName" "${APP_NAME}"
  WriteRegStr HKLM "${UNINST_KEY}" "DisplayVersion" "${APP_VERSION}"
  WriteRegStr HKLM "${UNINST_KEY}" "Publisher" "${APP_PUBLISHER}"
  WriteRegStr HKLM "${UNINST_KEY}" "DisplayIcon" "$INSTDIR\${APP_EXE}"
  WriteRegStr HKLM "${UNINST_KEY}" "InstallLocation" "$INSTDIR"
  WriteRegStr HKLM "${UNINST_KEY}" "UninstallString" '"$INSTDIR\uninstall.exe"'
  WriteRegDWORD HKLM "${UNINST_KEY}" "NoModify" 1
  WriteRegDWORD HKLM "${UNINST_KEY}" "NoRepair" 1
SectionEnd

Section "Uninstall"
  Delete "$SMPROGRAMS\${APP_NAME}\${APP_NAME}.lnk"
  RMDir "$SMPROGRAMS\${APP_NAME}"
  Delete "$DESKTOP\${APP_NAME}.lnk"

  Delete "$INSTDIR\${APP_EXE}"
  Delete "$INSTDIR\${APP_CLI}"
  Delete "$INSTDIR\README.md"
  Delete "$INSTDIR\LICENSE"
  Delete "$INSTDIR\uninstall.exe"
  RMDir "$INSTDIR"

  DeleteRegKey HKLM "${UNINST_KEY}"
SectionEnd
