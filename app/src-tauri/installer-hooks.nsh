; Ganchos do instalador NSIS do pAuletOr 3.
; Na desinstalação, remove as regras de firewall criadas pelo app (só funcionam se o
; desinstalador tiver permissão de administrador; caso contrário, o botão
; "Remover regras do firewall" nas Configurações faz isso antes de desinstalar).

!macro NSIS_HOOK_POSTUNINSTALL
  nsExec::Exec 'netsh advfirewall firewall delete rule name="pAuletOr 3 - Minecraft (somente Tailscale)"'
  nsExec::Exec 'netsh advfirewall firewall delete rule name="pAuletOr 3 - bloqueio do RCON"'
!macroend
