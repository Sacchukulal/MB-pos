; Magic Bill — installer hooks (tauri-bundler NSIS).
;
; A counter takes phone orders over the shop's WiFi, so Windows Firewall has to let the
; program in. This installer runs as the person (no admin), and netsh refuses to write a rule
; without admin — quietly, which is how every field install ended up with no rule at all.
; So: read first (no rights needed), and only when the rule is missing run the two netsh
; commands elevated, which is one UAC prompt on the first install and none on an update.
;
; The two commands are the ones `src-tauri/src/firewall.rs` runs from the Phones page, word
; for word; a test there reads this file and fails if they drift apart.

!macro NSIS_HOOK_POSTINSTALL
  ; What netsh prints carries the rule's program path as stored, in every language Windows
  ; speaks — so the path is what is looked for, not a translated word.
  nsExec::ExecToStack 'netsh advfirewall firewall show rule name="Magic Bill counter" verbose'
  Pop $0 ; the exit code, which is 0 whether or not a rule matched
  Pop $1 ; what it printed
  ${StrLoc} $2 $1 "$INSTDIR\magic-bill.exe" ">"
  ${If} $2 == ""
    ClearErrors
    ExecShellWait "runas" "cmd.exe" '/C netsh advfirewall firewall delete rule name=all dir=in program="$INSTDIR\magic-bill.exe" & netsh advfirewall firewall add rule name="Magic Bill counter" dir=in action=allow program="$INSTDIR\magic-bill.exe" enable=yes profile=any' SW_HIDE
    ; Refused at the prompt: the install goes on, and Settings › Phones offers the same repair.
    ClearErrors
  ${EndIf}
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  ; Best effort, never a prompt: the uninstaller also runs, unattended, in front of every
  ; update, and a rule left behind for a program that is gone does no harm.
  nsExec::ExecToLog 'netsh advfirewall firewall delete rule name="Magic Bill counter" program="$INSTDIR\magic-bill.exe"'
!macroend
