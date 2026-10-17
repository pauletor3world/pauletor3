#!/bin/bash
# Teste de ponta a ponta da troca PLANEJADA de host, com servidor de Minecraft de verdade e um
# jogador robô (scripts/robo-jogador.py) que entra pelo proxy como um jogo de verdade.
#
#   bash scripts/e2e-troca.sh [pasta-de-trabalho]
#
# Precisa da coordenação local no ar:  cd coordinator && npx wrangler dev --local --port 8787
# (ADMIN_TOKEN=dev-admin-local em coordinator/.dev.vars) e do CLI compilado.
#
# PC A (Ana) hospeda. Bia joga do PC B pelo proxy dela. A clica em Parar com Bia dentro:
# o host tem que passar para B sem Bia ser desconectada — o jogo dela vai para a sala de
# espera ("Entrando...") e volta sozinho quando o servidor do B abre. No fim, um terceiro PC
# confere que o mundo publicado por B é idêntico ao que B tem no disco.
set -u
E="${1:-/d/DevTools/p3v2-e2e-troca}"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
CLI="$ROOT/target/debug/machenia-cli.exe"
[ -x "$CLI" ] || CLI="$ROOT/target/debug/machenia-cli"
COORD="${P3_COORD_URL:-http://127.0.0.1:8787}"
ADMIN="${P3_ADMIN:-dev-admin-local}"
curl -sf "$COORD/health" >/dev/null || { echo "coordenação fora do ar em $COORD"; exit 1; }

rm -rf "$E"; mkdir -p "$E/A" "$E/B" "$E/C"
cli() { "$CLI" --home "$E/$1" "${@:2}"; }

cli A create-world --coord "$COORD" --admin "$ADMIN" --name "Teste troca" --nick Ana || exit 1
LINK=$(cli A invite --uses 5 | tail -1)
cli B join "$LINK" --nick Bia || exit 1
cli C join "$LINK" --nick Caio || exit 1
cli A setup --accept-eula --checkpoint-minutes 5 --grace-minutes 30 >/dev/null
cli B setup --accept-eula --checkpoint-minutes 5 --grace-minutes 30 --proxy-port 25570 >/dev/null
cli C setup --proxy-port 25571 >/dev/null
cli A prepare && cli B prepare || exit 1

cli A up --play --stop-after 240 > "$E/A.log" 2>&1 &
APID=$!
cli B up --stop-after 1200 --stop-after-hosting 40 > "$E/B.log" 2>&1 &
BPID=$!
# Bia entra pelo proxy do PC dela assim que der (a sala de espera segura enquanto A abre)
sleep 10
python "$ROOT/scripts/robo-jogador.py" --porta 25570 --nick Bia --segundos 900 > "$E/robo.log" 2>&1 &
RPID=$!
wait $APID
wait $BPID
sleep 3
kill $RPID 2>/dev/null
wait $RPID 2>/dev/null

echo "--- robô"
cat "$E/robo.log"
echo "---"
PLAYS=$(grep -c "JOGANDO" "$E/robo.log")
DROPS=$(grep -c "DESCONECTADO\|recusado\|conexão caiu" "$E/robo.log")
TR=$(grep -m1 "transfer para" "$E/robo.log" | grep -o "t=[0-9]*" | grep -o "[0-9]*")
BACK=$(grep "JOGANDO" "$E/robo.log" | sed -n 2p | grep -o "t=[0-9]*" | grep -o "[0-9]*")
grep -q "hospedando=true host=Bia" "$E/B.log" || { echo "FALHOU: B não assumiu"; tail -30 "$E/B.log"; exit 1; }
[ -n "$TR" ] || { echo "FALHOU: o robô não recebeu /transfer"; tail -30 "$E/A.log"; exit 1; }
[ "$PLAYS" -ge 2 ] || { echo "FALHOU: o robô não voltou ao jogo"; exit 1; }
[ "$DROPS" -le 1 ] || { echo "FALHOU: o robô caiu $DROPS vezes"; exit 1; }
echo "troca: Bia ficou na sala de espera por $(( (BACK - TR) / 1000 )) s e voltou ao jogo sozinha"

cli C pull >/dev/null || exit 1
diff <(cli B fingerprint "$E/B/server" | sort) <(cli C fingerprint "$E/C/server" | sort) >/dev/null \
  && echo "integridade: mundo idêntico no terceiro PC (hashes iguais)" \
  || { echo "FALHOU: mundos diferentes"; exit 1; }
