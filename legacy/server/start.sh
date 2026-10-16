#!/usr/bin/env bash
# Inicia o servidor Fabric (com Lithium em mods/). Chamado pelo minecraft.service.
set -euo pipefail

BASE=/opt/minecraft
if [[ -f $BASE/config.env ]]; then
  # shellcheck source=/dev/null
  source "$BASE/config.env"
fi
MC_XMS="${MC_XMS:-5G}"
MC_XMX="${MC_XMX:-5G}"

# Usa o Java 21 explicitamente, mesmo se houver outro Java instalado
JAVA_BIN=java
for candidate in /usr/lib/jvm/java-21-openjdk-*/bin/java; do
  if [[ -x $candidate ]]; then JAVA_BIN=$candidate; break; fi
done

cd "$BASE/server"

# G1GC simples. O launcher do Fabric usa o server.jar vanilla desta pasta.
exec "$JAVA_BIN" \
  -Xms"$MC_XMS" -Xmx"$MC_XMX" \
  -XX:+UseG1GC \
  -XX:MaxGCPauseMillis=200 \
  -XX:+ParallelRefProcEnabled \
  -XX:+DisableExplicitGC \
  -jar fabric-server-launch.jar nogui
