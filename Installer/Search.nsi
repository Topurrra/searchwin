; The installer: what dragging Search to Applications is on the Mac.
;
; Per-user, so there is no administrator prompt and nothing outside your own
; profile is touched: the app goes to %LOCALAPPDATA%\Programs\Search, a
; shortcut to the Start menu, and Search is listed among the browsers Windows'
; Default apps can pick — the same keys the app's own "Make default" writes.
;
; The one thing Search borrows from Windows is WebView2, the engine every page
; runs in. Windows 11 and any up-to-date Windows 10 have it already; on a PC
; that doesn't, Microsoft's own small bootstrapper — carried inside this
; installer — fetches it, and only then.
;
; Built by build.ps1 -Installer, which passes these in:
;   VERSION   1.0.0
;   SOURCE    the folder build.ps1 made (build\Search)
;   WEBVIEW2  Microsoft's Evergreen bootstrapper (MicrosoftEdgeWebview2Setup.exe)
;   OUTFILE   where the installer goes

Unicode true
SetCompressor /SOLID lzma
SetCompressorDictSize 64
ManifestDPIAware true
RequestExecutionLevel user

!include "MUI2.nsh"
!include "LogicLib.nsh"
!include "x64.nsh"
!include "WinVer.nsh"
!include "FileFunc.nsh"

!ifndef VERSION
  !define VERSION "1.0.0"
!endif

!define NAME "Search"
!define PUBLISHER "Office Commun"
!define EXE "Search.exe"
!define PROGID "SearchURL"
!define CLIENT "Software\Clients\StartMenuInternet\${NAME}"
!define UNINSTALL "Software\Microsoft\Windows\CurrentVersion\Uninstall\${NAME}"
; WebView2's own id, as Microsoft documents it for detecting the runtime.
!define WEBVIEW2_ID "{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}"

Name "${NAME}"
OutFile "${OUTFILE}"
; Always the same place, never one read back from the registry: the folder is
; cleared before each install, so it must only ever be Search's own.
InstallDir "$LOCALAPPDATA\Programs\${NAME}"
BrandingText " "
ShowInstDetails nevershow
ShowUninstDetails nevershow

VIProductVersion "${VERSION}.0"
VIAddVersionKey "ProductName" "${NAME}"
VIAddVersionKey "CompanyName" "${PUBLISHER}"
VIAddVersionKey "FileDescription" "${NAME} installer"
VIAddVersionKey "FileVersion" "${VERSION}"
VIAddVersionKey "ProductVersion" "${VERSION}"
VIAddVersionKey "LegalCopyright" "MIT — ${PUBLISHER}"

!define MUI_ICON "..\Search\Assets\Search.ico"
!define MUI_UNICON "..\Search\Assets\Search.ico"
!define MUI_ABORTWARNING

; Nothing to read and nothing to choose: install, then offer to open it.
!define MUI_FINISHPAGE_RUN "$INSTDIR\${EXE}"
!define MUI_FINISHPAGE_RUN_TEXT "Open Search"
!define MUI_FINISHPAGE_TITLE "Search is installed"
!define MUI_FINISHPAGE_TEXT "It is in the Start menu. To make it the browser links open in, use Settings › General › Make default."
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH

!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES

!insertmacro MUI_LANGUAGE "English"

Function .onInit
  ; What the app itself needs: 64-bit Windows 10 1809 or later (WebView2 and
  ; the Windows App SDK both start there).
  ${IfNot} ${RunningX64}
    MessageBox MB_ICONSTOP "Search needs 64-bit Windows."
    Abort
  ${EndIf}
  ${IfNot} ${AtLeastBuild} 17763
    MessageBox MB_ICONSTOP "Search needs Windows 10 version 1809 or later."
    Abort
  ${EndIf}
FunctionEnd

; Whether WebView2 is here: machine-wide (either registry view) or for this
; user. An empty version, or 0.0.0.0, means it was removed.
Function HasWebView2
  ReadRegStr $0 HKLM "SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\${WEBVIEW2_ID}" "pv"
  ${If} $0 == ""
  ${OrIf} $0 == "0.0.0.0"
    SetRegView 64
    ReadRegStr $0 HKLM "SOFTWARE\Microsoft\EdgeUpdate\Clients\${WEBVIEW2_ID}" "pv"
    SetRegView lastused
  ${EndIf}
  ${If} $0 == ""
  ${OrIf} $0 == "0.0.0.0"
    ReadRegStr $0 HKCU "Software\Microsoft\EdgeUpdate\Clients\${WEBVIEW2_ID}" "pv"
  ${EndIf}
  ${If} $0 == "0.0.0.0"
    StrCpy $0 ""
  ${EndIf}
FunctionEnd

Section "Search"
  SetOutPath "$INSTDIR"

  ; A Search that is open holds its own files. Asked to close rather than
  ; closed for you: it may be holding something you typed.
  ${If} ${FileExists} "$INSTDIR\${EXE}"
    retry:
    ClearErrors
    Rename "$INSTDIR\${EXE}" "$INSTDIR\${EXE}.old"
    ${If} ${Errors}
      MessageBox MB_RETRYCANCEL|MB_ICONEXCLAMATION "Search is open. Close it, then try again." IDRETRY retry
      Abort
    ${EndIf}
    Delete "$INSTDIR\${EXE}.old"
  ${EndIf}

  ; The engine first: without it there is nothing for the app to show.
  Call HasWebView2
  ${If} $0 == ""
    DetailPrint "Getting WebView2 from Microsoft…"
    InitPluginsDir
    File "/oname=$PLUGINSDIR\MicrosoftEdgeWebview2Setup.exe" "${WEBVIEW2}"
    ExecWait '"$PLUGINSDIR\MicrosoftEdgeWebview2Setup.exe" /silent /install' $1
    Call HasWebView2
    ${If} $0 == ""
      MessageBox MB_ICONEXCLAMATION "Search needs Microsoft WebView2, and it couldn't be installed (code $1). Check the connection and run this again, or get it from microsoft.com/edge/webview2."
      Abort
    ${EndIf}
  ${EndIf}

  ; The app. Everything of a previous version goes first, so nothing stale
  ; is left beside the new one — but only the app: what you keep (history,
  ; sessions, passwords) lives elsewhere and is never touched here.
  RMDir /r "$INSTDIR"
  SetOutPath "$INSTDIR"
  File /r "${SOURCE}\*.*"
  WriteUninstaller "$INSTDIR\Uninstall.exe"

  CreateShortcut "$SMPROGRAMS\${NAME}.lnk" "$INSTDIR\${EXE}" "" "$INSTDIR\${EXE}" 0 SW_SHOWNORMAL "" "A small, fast, quiet web browser"

  ; Listed among the browsers in Settings › Apps › Default apps. Becoming the
  ; default is still yours to choose there — Windows doesn't let an app do it
  ; for you, and Search wouldn't if it could.
  WriteRegStr HKCU "Software\Classes\${PROGID}" "" "Search URL"
  WriteRegStr HKCU "Software\Classes\${PROGID}" "URL Protocol" ""
  WriteRegStr HKCU "Software\Classes\${PROGID}\DefaultIcon" "" "$INSTDIR\${EXE},0"
  WriteRegStr HKCU "Software\Classes\${PROGID}\shell\open\command" "" '"$INSTDIR\${EXE}" "%1"'
  WriteRegStr HKCU "${CLIENT}" "" "${NAME}"
  WriteRegStr HKCU "${CLIENT}\DefaultIcon" "" "$INSTDIR\${EXE},0"
  WriteRegStr HKCU "${CLIENT}\shell\open\command" "" '"$INSTDIR\${EXE}"'
  WriteRegStr HKCU "${CLIENT}\Capabilities" "ApplicationName" "${NAME}"
  WriteRegStr HKCU "${CLIENT}\Capabilities" "ApplicationDescription" "A small, fast, quiet web browser"
  WriteRegStr HKCU "${CLIENT}\Capabilities" "ApplicationIcon" "$INSTDIR\${EXE},0"
  WriteRegStr HKCU "${CLIENT}\Capabilities\URLAssociations" "http" "${PROGID}"
  WriteRegStr HKCU "${CLIENT}\Capabilities\URLAssociations" "https" "${PROGID}"
  WriteRegStr HKCU "${CLIENT}\Capabilities\StartMenu" "StartMenuInternet" "${NAME}"
  WriteRegStr HKCU "Software\RegisteredApplications" "${NAME}" "${CLIENT}\Capabilities"

  ; Apps & features.
  ${GetSize} "$INSTDIR" "/S=0K" $2 $3 $4
  WriteRegStr HKCU "${UNINSTALL}" "DisplayName" "${NAME}"
  WriteRegStr HKCU "${UNINSTALL}" "DisplayVersion" "${VERSION}"
  WriteRegStr HKCU "${UNINSTALL}" "Publisher" "${PUBLISHER}"
  WriteRegStr HKCU "${UNINSTALL}" "DisplayIcon" "$INSTDIR\${EXE},0"
  WriteRegStr HKCU "${UNINSTALL}" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "${UNINSTALL}" "UninstallString" '"$INSTDIR\Uninstall.exe"'
  WriteRegStr HKCU "${UNINSTALL}" "QuietUninstallString" '"$INSTDIR\Uninstall.exe" /S'
  WriteRegStr HKCU "${UNINSTALL}" "URLInfoAbout" "https://officecommun.com/search"
  WriteRegDWORD HKCU "${UNINSTALL}" "EstimatedSize" $2
  WriteRegDWORD HKCU "${UNINSTALL}" "NoModify" 1
  WriteRegDWORD HKCU "${UNINSTALL}" "NoRepair" 1

  ; Tell Explorer the list of browsers changed.
  System::Call 'shell32::SHChangeNotify(i 0x08000000, i 0, p 0, p 0)'
SectionEnd

Section "Uninstall"
  Delete "$SMPROGRAMS\${NAME}.lnk"
  RMDir /r "$INSTDIR"

  DeleteRegKey HKCU "Software\Classes\${PROGID}"
  DeleteRegKey HKCU "${CLIENT}"
  DeleteRegValue HKCU "Software\RegisteredApplications" "${NAME}"
  DeleteRegKey HKCU "${UNINSTALL}"
  System::Call 'shell32::SHChangeNotify(i 0x08000000, i 0, p 0, p 0)'

  ; What you kept — history, bookmarks, sessions, site data in
  ; %LOCALAPPDATA%\Search, passwords in Credential Manager — stays, the way
  ; dragging an app to the Bin leaves its settings. WebView2 stays too: it is
  ; Windows', and other apps use it.
SectionEnd
