; Magic Bill — installer hooks (tauri-bundler NSIS).
;
; A counter takes phone orders over the shop's WiFi, so Windows Firewall has to let the
; program in. Written at install, removed at uninstall. When the installer is not elevated
; netsh refuses quietly; the program then offers the same repair from Settings › Phones.

!macro NSIS_HOOK_POSTINSTALL
  nsExec::ExecToLog 'netsh advfirewall firewall delete rule name="Magic Bill counter" program="$INSTDIR\magic-bill.exe"'
  nsExec::ExecToLog 'netsh advfirewall firewall add rule name="Magic Bill counter" dir=in action=allow program="$INSTDIR\magic-bill.exe" enable=yes profile=any'
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  nsExec::ExecToLog 'netsh advfirewall firewall delete rule name="Magic Bill counter" program="$INSTDIR\magic-bill.exe"'
!macroend
