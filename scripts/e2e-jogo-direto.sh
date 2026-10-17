#!/bin/bash
# Teste de ponta a ponta do Minecraft aberto direto pelo app (build pessoal), com servidor de
# verdade: o PC A hospeda; no PC B o app baixa/confere o cliente e abre o jogo, que entra
# sozinho pelo proxy do B (Quick Play) e chega ao servidor do A por P2P.
#
#   bash scripts/e2e-jogo-direto.sh [pasta-de-trabalho]
#
# Precisa da coordenação local no ar:  cd coordinator && npx wrangler dev --local --port 8787
# e do CLI compilado com --features personal. Abre uma janela do Minecraft por ~1 minuto.
set -u
E="${1:-/d/DevTools/p3v2-e2e-jogo}"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
CLI="$ROOT/target/debug/machencraft-cli.exe"
[ -x "$CLI" ] || CLI="$ROOT/target/debug/machencraft-cli"
COORD="${P3_COORD_URL:-http://127.0.0.1:8787}"
ADMIN="${P3_ADMIN:-dev-admin-local}"
curl -sf "$COORD/health" >/dev/null || { echo "coordenação fora do ar em $COORD"; exit 1; }

rm -rf "$E/A" "$E/B"; mkdir -p "$E/A" "$E/B"
cli() { "$CLI" --home "$E/$1" "${@:2}"; }

cli A create-world --coord "$COORD" --admin "$ADMIN" --name "Teste jogo direto" --nick Ana || exit 1
LINK=$(cli A invite --uses 5 | tail -1)
cli B join "$LINK" --nick Bia || exit 1
cli A setup --accept-eula --checkpoint-minutes 5 --grace-minutes 30 --proxy-port 25580 >/dev/null
cli B setup --accept-eula --proxy-port 25581 >/dev/null
cli A prepare || exit 1

MACHENCRAFT_CONSOLE=1 cli A up --play --stop-after 200 > "$E/A.log" 2>&1 &
APID=$!
cli B up --stop-after 200 > "$E/B.log" 2>&1 &
BPID=$!
for _ in $(seq 1 120); do grep -q "hospedando=true" "$E/A.log" && break; sleep 2; done
grep -q "hospedando=true" "$E/A.log" || { echo "FALHOU: A não abriu o mundo"; tail -30 "$E/A.log"; exit 1; }

# o jogo de verdade, aberto pelo app do B
cli B jogo > "$E/jogo.log" 2>&1 || { echo "FALHOU: o app não abriu o Minecraft"; tail "$E/jogo.log"; exit 1; }
for _ in $(seq 1 90); do grep -q "Bia joined the game" "$E/A.log" && break; sleep 2; done
# fecha só o jogo que o teste abriu
GPID=$(grep -h "Minecraft aberto direto" "$E"/B/logs/machencraft*.log | tail -1 | grep -o '"pid":[0-9]*' | grep -o '[0-9]*')
[ -n "$GPID" ] && taskkill //F //PID "$GPID" >/dev/null 2>&1
wait $APID $BPID 2>/dev/null
grep -q "Bia joined the game" "$E/A.log" \
  && echo "OK: o Minecraft aberto pelo app entrou sozinho no mundo do A (Quick Play → proxy → P2P → servidor)" \
  || { echo "FALHOU: Bia não entrou"; tail -20 "$E/B/client/game/logs/latest.log"; exit 1; }
