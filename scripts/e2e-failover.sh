#!/bin/bash
# Teste de ponta a ponta da troca automática de host, com servidor de Minecraft de verdade.
#
#   bash scripts/e2e-failover.sh [pasta-de-trabalho] [pasta-de-mundo-para-importar]
#
# Precisa da coordenação local no ar:  cd coordinator && npx wrangler dev --local --port 8787
# (ADMIN_TOKEN=dev-admin-local em coordinator/.dev.vars) e do CLI compilado.
#
# PC A (dono) cria o mundo, convida B, hospeda, salva e "perde a energia". PC B (jogando)
# assume sozinho. Mede o tempo da queda até B hospedar e confere, num terceiro PC, que o
# mundo publicado por B é idêntico (hash a hash) ao que B tem no disco.
set -u
E="${1:-/d/DevTools/p3v2-e2e}"
WORLD="${2:-}"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
CLI="$ROOT/target/debug/pauletor3-cli.exe"
[ -x "$CLI" ] || CLI="$ROOT/target/debug/pauletor3-cli"
COORD="${P3_COORD_URL:-http://127.0.0.1:8787}"
ADMIN="${P3_ADMIN:-dev-admin-local}"
curl -sf "$COORD/health" >/dev/null || { echo "coordenação fora do ar em $COORD"; exit 1; }

rm -rf "$E"; mkdir -p "$E/A" "$E/B" "$E/C"
cli() { "$CLI" --home "$E/$1" "${@:2}"; }

if [ -n "$WORLD" ]; then
  cli A create-world --coord "$COORD" --admin "$ADMIN" --name "Teste E2E" --nick Ana --import "$WORLD" || exit 1
else
  cli A create-world --coord "$COORD" --admin "$ADMIN" --name "Teste E2E" --nick Ana || exit 1
fi
LINK=$(cli A invite --uses 5 | tail -1)
cli B join "$LINK" --nick Bia || exit 1
cli C join "$LINK" --nick Caio || exit 1
cli A setup --accept-eula --checkpoint-minutes 1 --grace-minutes 30 >/dev/null
cli B setup --accept-eula --checkpoint-minutes 1 --grace-minutes 30 --proxy-port 25570 >/dev/null
cli C setup --proxy-port 25571 >/dev/null
cli A prepare && cli B prepare || exit 1

PAULETOR3_PRETEND_PLAYING=1 cli B up --stop-after 900 --stop-after-hosting 20 > "$E/B.log" 2>&1 &
BPID=$!
sleep 5
cli A up --play --checkpoint-after 60 --crash-after 120 > "$E/A.log" 2>&1
wait $BPID

CRASH=$(grep -o "SIMULANDO QUEDA DE ENERGIA t=[0-9]*" "$E/A.log" | grep -o "[0-9]*$")
HOSTED=$(grep "hospedando=true host=Bia" "$E/B.log" | head -1 | grep -o "t=[0-9]*" | grep -o "[0-9]*")
TAKEOVER=$(grep "failover=Trocando" "$E/B.log" | head -1 | grep -o "t=[0-9]*" | grep -o "[0-9]*")
if [ -z "$CRASH" ] || [ -z "$HOSTED" ]; then echo "FALHOU: B não assumiu"; tail -20 "$E/B.log"; exit 1; fi
echo "queda → B começa a assumir: $(( (TAKEOVER - CRASH) / 1000 )) s"
echo "queda → B hospedando:       $(( (HOSTED - CRASH) / 1000 )) s"

# terceiro PC baixa o HEAD final e compara com o mundo do B
cli C pull >/dev/null || exit 1
diff <(cli B fingerprint "$E/B/server" | sort) <(cli C fingerprint "$E/C/server" | sort) >/dev/null \
  && echo "integridade: mundo idêntico no terceiro PC (hashes iguais)" \
  || { echo "FALHOU: mundos diferentes"; exit 1; }
