; WattClean's own additions to the installer.

; The command line, built beside the app by `beforeBundleCommand`. Defined
; here so the path is taken from this file, not the generated installer.
!define WC_CMD "${__FILEDIR__}\..\target\release\wattclean-cmd.exe"
; WattClean's licence and the open-source packages' licences go with it.
!define WC_LICENSE "${__FILEDIR__}\..\LICENSE"
!define WC_NOTICES "${__FILEDIR__}\..\THIRD-PARTY-NOTICES.txt"

!macro NSIS_HOOK_POSTINSTALL
  SetOutPath $INSTDIR
  File "${WC_CMD}"
  File "/oname=LICENSE.txt" "${WC_LICENSE}"
  File "${WC_NOTICES}"
!macroend

; WattClean's own Windows registrations, removed on a real uninstall (not
; when an upgrade runs the old uninstaller with /UPDATE): the sign-in entry
; and the Task Scheduler tasks of its schedules and sign-in start. The
; command line goes with the app's own files, every time.
!macro NSIS_HOOK_PREUNINSTALL
  Delete "$INSTDIR\wattclean-cmd.exe"
  Delete "$INSTDIR\LICENSE.txt"
  Delete "$INSTDIR\THIRD-PARTY-NOTICES.txt"
  ${If} $UpdateMode <> 1
    DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "WattClean"
    nsExec::Exec `"$SYSDIR\WindowsPowerShell\v1.0\powershell.exe" -NoProfile -NonInteractive -Command "Get-ScheduledTask -TaskPath '\WattClean\' -ErrorAction SilentlyContinue | Unregister-ScheduledTask -Confirm:$$false"`
    Pop $0
  ${EndIf}
!macroend
