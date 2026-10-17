; Ganchos do instalador NSIS do Machenia.
; A v2 não cria regra de firewall nem abre porta (o servidor só escuta em 127.0.0.1).
; Na desinstalação, remove as regras que a v1 possa ter deixado (exige administrador;
; sem permissão, não faz nada).

!define RUNKEY "Software\Microsoft\Windows\CurrentVersion\Run"

; O app já se chamou "pAuletOr 3" (até a 2.0.3) e "Machen Craft" (até a 2.1.x): outra pasta
; de instalação, outro registro. Na primeira instalação do Machenia, o antigo é desinstalado
; em silêncio (os dados do mundo ficam: o desinstalador só apaga dados com a caixa marcada)
; e o "Iniciar com o Windows" passa para o nome novo. A pasta de dados é renomeada pelo
; próprio app.
!macro REMOVE_LEGACY NAME EXE
  ReadRegStr $R5 HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\${NAME}" "InstallLocation"
  ${If} $R5 != ""
    nsis_tauri_utils::StrReplace "$R5" '"' ""
    Pop $R5
    ReadRegStr $R6 HKCU "${RUNKEY}" "${NAME}"
    ${If} ${FileExists} "$R5\uninstall.exe"
      !insertmacro CheckIfAppIsRunning "$R5\${EXE}" "${NAME}"
      ExecWait '"$R5\uninstall.exe" /S _?=$R5'
      Delete "$R5\uninstall.exe"
      Delete "$R5\hs_err_pid*.log"
      RMDir "$R5"
    ${EndIf}
    DeleteRegKey HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\${NAME}"
    DeleteRegKey HKCU "Software\${NAME}"
    DeleteRegKey HKCU "Software\Machen\${NAME}"
    DeleteRegValue HKCU "${RUNKEY}" "${NAME}"
    ${If} $R6 != ""
      WriteRegStr HKCU "${RUNKEY}" "${PRODUCTNAME}" '"$INSTDIR\${MAINBINARYNAME}.exe" --minimized'
    ${EndIf}
  ${EndIf}
!macroend

!macro NSIS_HOOK_PREINSTALL
  !insertmacro REMOVE_LEGACY "Machen Craft" "machen-craft.exe"
  !insertmacro REMOVE_LEGACY "pAuletOr 3" "pauletor3.exe"
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  nsExec::Exec 'netsh advfirewall firewall delete rule name="pAuletOr 3 - Minecraft (somente Tailscale)"'
  nsExec::Exec 'netsh advfirewall firewall delete rule name="pAuletOr 3 - bloqueio do RCON"'
!macroend
