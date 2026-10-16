#!/usr/bin/env bash
# Backup dos mundos em .tar.zst. Roda como usuário minecraft (minecraft-backup.service).
# Com o servidor online: avisa no chat, save-off, save-all flush, compacta e save-on.
set -Eeuo pipefail
umask 022

BASE=/opt/minecraft
SERVER="$BASE/server"
DEST="$BASE/backups"
RCON="$BASE/tools/rcon.py"

if [[ -f $BASE/config.env ]]; then
  # shellcheck source=/dev/null
  source "$BASE/config.env"
fi
KEEP="${BACKUP_KEEP:-7}"

log() { echo "[backup] $*"; }
rcon() { "$RCON" "$1"; }

online=0
saving_off=0
partial=""

cleanup() {
  if [[ -n $partial ]]; then rm -f "$partial"; fi
  if [[ $saving_off == 1 ]]; then
    if rcon "save-on" >/dev/null; then
      log "save-on ok"
    else
      log "AVISO: falhou o save-on! Rode: mc cmd save-on"
    fi
    saving_off=0
  fi
}
trap cleanup EXIT
trap 'exit 1' INT TERM

if rcon "list" >/dev/null 2>&1; then online=1; fi

if [[ $online == 1 ]]; then
  log "servidor online: pausando gravação"
  rcon "say Backup em andamento, pode dar um leve travamento..." >/dev/null || true
  saving_off=1
  rcon "save-off" >/dev/null
  rcon "save-all flush" >/dev/null
  sleep 5
else
  log "servidor offline: copiando direto"
fi

items=()
for item in world world_nether world_the_end config server.properties ops.json banned-players.json banned-ips.json; do
  if [[ -e $SERVER/$item ]]; then items+=("$item"); fi
done
[[ ${#items[@]} -gt 0 ]] || { log "ERRO: nenhum mundo encontrado em $SERVER"; exit 1; }

out="$DEST/world-$(date +%Y%m%d-%H%M%S).tar.zst"
partial="$out.part"
log "compactando ${items[*]}"
tar -C "$SERVER" --exclude='session.lock' -cf - "${items[@]}" | zstd -q -T0 -3 -o "$partial"
mv "$partial" "$out"
partial=""

cleanup
if [[ $online == 1 ]]; then rcon "say Backup concluído." >/dev/null || true; fi
log "criado $out ($(du -h "$out" | cut -f1))"

# Mantém só os últimos $KEEP (o nome tem timestamp, então a ordem alfabética é cronológica)
mapfile -t all < <(find "$DEST" -maxdepth 1 -name 'world-*.tar.zst' | sort)
excess=$(( ${#all[@]} - KEEP ))
for ((i = 0; i < excess; i++)); do
  rm -f "${all[$i]}"
  log "removido antigo: $(basename "${all[$i]}")"
done
