; Ganchos do instalador NSIS do pAuletOr 3.
; A v2 não cria regra de firewall nem abre porta (o servidor só escuta em 127.0.0.1).
; Na desinstalação, remove as regras que a v1 possa ter deixado (exige administrador;
; sem permissão, não faz nada).

!macro NSIS_HOOK_POSTUNINSTALL
  nsExec::Exec 'netsh advfirewall firewall delete rule name="pAuletOr 3 - Minecraft (somente Tailscale)"'
  nsExec::Exec 'netsh advfirewall firewall delete rule name="pAuletOr 3 - bloqueio do RCON"'
!macroend
