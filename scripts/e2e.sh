#!/bin/bash
# Teste de ponta a ponta do pAuletOr 3, com uma "nuvem" numa pasta local (sem Google Drive).
#
#   bash scripts/e2e.sh <pasta-do-mundo> [pasta-de-trabalho]
#
# Importa o mundo, sobe o servidor real (Java 21 + Fabric + Lithium baixados e conferidos),
# faz um checkpoint via RCON, encerra, e confere que um "segundo PC" recebe o mundo idêntico.
# Usa 127.0.0.1 no lugar do Tailscale (PAULETOR3_TEST_IP). Precisa do rclone
# (scripts/baixar-rclone.ps1) e do CLI compilado (cargo build -p pauletor3-core).
set -u
WORLD="${1:?informe a pasta do mundo (a que tem level.dat)}"
E="${2:-${TMPDIR:-/tmp}/pauletor3-e2e}"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
CLI="$ROOT/target/debug/pauletor3-cli.exe"
[ -x "$CLI" ] || CLI="$ROOT/target/debug/pauletor3-cli"
RCLONE="$ROOT/app/src-tauri/binaries/rclone-x86_64-pc-windows-msvc.exe"
rm -rf "$E/pcA" "$E/pcB" "$E/remote"
mkdir -p "$E"
A=(--home "$E/pcA" --local-remote "$E/remote" --rclone "$RCLONE")
B=(--home "$E/pcB" --local-remote "$E/remote" --rclone "$RCLONE")
export PAULETOR3_TEST_IP=127.0.0.1
step() { echo; echo "=== $* ($(date +%T))"; }

step "1. PC A (dono)";          "$CLI" "${A[@]}" setup --player Tester --owner --accept-eula >/dev/null || exit 1
step "2. importar o mundo";     "$CLI" "${A[@]}" import "$WORLD" || exit 1
step "3. jogadores do mundo";   "$CLI" "${A[@]}" players
step "4. hospedar + checkpoint + encerrar"
"$CLI" "${A[@]}" play --checkpoint-after 40 --stop-after 15 || exit 1
step "5. linha do tempo";       "$CLI" "${A[@]}" timeline
step "6. PC B baixa o mundo"
"$CLI" "${B[@]}" setup --player Amigo --accept-eula >/dev/null
"$CLI" "${B[@]}" pull || exit 1
step "7. comparar hashes A x B"
"$CLI" "${A[@]}" fingerprint "$E/pcA/server" > "$E/fa.txt"
"$CLI" "${B[@]}" fingerprint "$E/pcB/server" > "$E/fb.txt"
if diff -q "$E/fa.txt" "$E/fb.txt"; then
  echo "IDÊNTICOS: $(wc -l < "$E/fa.txt") arquivos"
else
  echo "DIFERENTES"; diff "$E/fa.txt" "$E/fb.txt" | head; exit 1
fi
grep -h '"pause_ms"' "$E"/pcA/logs/pauletor3*.log | tail -1
echo "E2E OK"
