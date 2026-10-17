; Ganchos do instalador NSIS do Machen Craft.
; A v2 não cria regra de firewall nem abre porta (o servidor só escuta em 127.0.0.1).
; Na desinstalação, remove as regras que a v1 possa ter deixado (exige administrador;
; sem permissão, não faz nada).

!define LEGACY_NAME "pAuletOr 3"
!define LEGACY_UNINSTKEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\${LEGACY_NAME}"
!define RUNKEY "Software\Microsoft\Windows\CurrentVersion\Run"

; Até a 2.0.3 o app se chamava "pAuletOr 3" (outra pasta de instalação, outro registro).
; Na primeira instalação do Machen Craft, o antigo é desinstalado em silêncio (os dados do
; mundo ficam: o desinstalador só apaga dados com a caixa marcada) e o "Iniciar com o
; Windows" passa para o nome novo. A pasta de dados é renomeada pelo próprio app.
!macro NSIS_HOOK_PREINSTALL
  ReadRegStr $R5 HKCU "${LEGACY_UNINSTKEY}" "InstallLocation"
  ${If} $R5 != ""
    nsis_tauri_utils::StrReplace "$R5" '"' ""
    Pop $R5
    ReadRegStr $R6 HKCU "${RUNKEY}" "${LEGACY_NAME}"
    ${If} ${FileExists} "$R5\uninstall.exe"
      !insertmacro CheckIfAppIsRunning "$R5\pauletor3.exe" "${LEGACY_NAME}"
      ExecWait '"$R5\uninstall.exe" /S _?=$R5'
      Delete "$R5\uninstall.exe"
      Delete "$R5\hs_err_pid*.log"
      RMDir "$R5"
    ${EndIf}
    DeleteRegKey HKCU "${LEGACY_UNINSTKEY}"
    DeleteRegKey HKCU "Software\${LEGACY_NAME}"
    DeleteRegValue HKCU "${RUNKEY}" "${LEGACY_NAME}"
    ${If} $R6 != ""
      WriteRegStr HKCU "${RUNKEY}" "${PRODUCTNAME}" '"$INSTDIR\${MAINBINARYNAME}.exe" --minimized'
    ${EndIf}
  ${EndIf}
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  nsExec::Exec 'netsh advfirewall firewall delete rule name="pAuletOr 3 - Minecraft (somente Tailscale)"'
  nsExec::Exec 'netsh advfirewall firewall delete rule name="pAuletOr 3 - bloqueio do RCON"'
!macroend
