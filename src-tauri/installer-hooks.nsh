;; NSIS installer hooks for the Go Magic shell.
;;
;; The bundled backend (`go-magic.exe`) is an independent background process:
;; it can outlive the shell as an orphan (shell crash, forced exit, or the
;; restart bug where the shell killed itself while shutting the backend down),
;; and while it lives it locks files inside the install directory — the NSIS
;; installer then fails with "file in use". The stock Tauri template only
;; knows about the main shell binary (`go-magic-desktop.exe`), never about the
;; backend child, so both processes are terminated here before any file is
;; touched.
;;
;; The Tauri bundler scans this file for `NSIS_HOOK_*` macro names and only
;; wires up the ones defined here.

!macro _GOMAGIC_KILL_PROCESSES
  ; /F force, /T kill the whole process tree (the backend may spawn helpers),
  ; /IM match by image name. Both calls always push an exit code: pop it
  ; immediately so the NSIS stack cannot grow across repeated runs.
  ; NOTE: the main shell binary is `go-magic-desktop.exe` — the Tauri
  ; productName ("Go Magic") is only the display name, the installed file
  ; keeps the crate name (verified via the running process command line).
  nsExec::Exec 'taskkill /F /T /IM go-magic.exe'
  Pop $0
  nsExec::Exec 'taskkill /F /T /IM go-magic-desktop.exe'
  Pop $0
  ; Give the OS a moment to release the file locks before the installer
  ; starts copying over them.
  Sleep 500
!macroend

!macro NSIS_HOOK_PREINSTALL
  !insertmacro _GOMAGIC_KILL_PROCESSES
!macroend

!macro NSIS_HOOK_POSTINSTALL
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  !insertmacro _GOMAGIC_KILL_PROCESSES
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
!macroend
