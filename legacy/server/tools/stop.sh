#!/usr/bin/env bash
# Parada limpa (ExecStop do minecraft.service): manda "stop" via RCON e espera o
# processo sair. Se o RCON falhar, o systemd cai no SIGTERM, que também salva.
set -u

PID="${1:-}"

/opt/minecraft/tools/rcon.py stop >/dev/null 2>&1 || exit 0
[[ -n $PID ]] || exit 0

for _ in $(seq 1 110); do
  kill -0 "$PID" 2>/dev/null || exit 0
  sleep 1
done
exit 0
