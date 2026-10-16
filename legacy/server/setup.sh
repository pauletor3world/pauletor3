#!/usr/bin/env bash
# Instalação completa e idempotente do servidor Minecraft (Fabric + Lithium).
# Funciona no Google Cloud (PROVIDER=gcp, x86_64) e na Oracle Cloud (PROVIDER=oracle, ARM).
# Uso (na VM, dentro da pasta enviada pelo deploy): sudo bash setup.sh
set -Eeuo pipefail

SRC="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BASE=/opt/minecraft
SERVER="$BASE/server"
TOOLS="$BASE/tools"
BACKUPS="$BASE/backups"
SECRET="$BASE/rcon.secret"
TS_LOG=/var/log/minecraft-tailscale-up.log
TOOL_FILES=(rcon.py backup.sh update-server.sh stop.sh worldinfo.py mc)

c_step=$'\e[1;36m'; c_ok=$'\e[32m'; c_warn=$'\e[1;33m'; c_err=$'\e[1;31m'; c_off=$'\e[0m'
step() { printf '\n%s==> %s%s\n' "$c_step" "$*" "$c_off"; }
ok()   { printf '%s    ok%s  %s\n' "$c_ok" "$c_off" "$*"; }
warn() { printf '%s    aviso%s  %s\n' "$c_warn" "$c_off" "$*"; }
die()  { printf '%sERRO:%s %s\n' "$c_err" "$c_off" "$*" >&2; exit 1; }
trap 'die "falhou na linha $LINENO: $BASH_COMMAND"' ERR

[[ $EUID -eq 0 ]] || die "rode com sudo: sudo bash setup.sh"

# Sem systemd (ex.: teste em container) os passos de serviço, firewall e Tailscale são pulados
HAS_SYSTEMD=0
if [[ -d /run/systemd/system ]]; then HAS_SYSTEMD=1; fi

# Impressão digital do que exige reiniciar o servidor (para só reiniciar se mudou)
fingerprint() {
  cat "$BASE/start.sh" "$BASE/config.env" "$TOOLS/stop.sh" "$SERVER/server.jar" \
      "$SERVER/fabric-server-launch.jar" "$SERVER/mods/lithium.jar" \
      "$SERVER/server.properties" /etc/systemd/system/minecraft.service 2>/dev/null \
    | sha256sum | cut -d' ' -f1 || true
}
FP_BEFORE=$(fingerprint)

# ---------------------------------------------------------------------------
step "1/12 Configuração"
CONFIG_SRC=""
if [[ -f $SRC/config.env ]]; then
  CONFIG_SRC="$SRC/config.env"
elif [[ -f $BASE/config.env ]]; then
  CONFIG_SRC="$BASE/config.env"
fi
if [[ -n $CONFIG_SRC ]]; then
  # shellcheck source=/dev/null
  source "$CONFIG_SRC"
  ok "lido $CONFIG_SRC"
else
  warn "config.env não encontrado; usando valores padrão"
fi
PROVIDER="${PROVIDER:-gcp}"
MC_VERSION="${MC_VERSION:-1.21.11}"
ALLOW_EXPERIMENTAL="${ALLOW_EXPERIMENTAL:-0}"
MOTD="${MOTD:-Servidor Minecraft}"
MC_PORT="${MC_PORT:-25565}"
PUBLIC_PORT="${PUBLIC_PORT:-0}"
TS_HOSTNAME="${TS_HOSTNAME:-minecraft}"
TS_AUTHKEY="${TS_AUTHKEY:-}"

case $PROVIDER in
  gcp)    expected_arch=x86_64 ;;
  oracle) expected_arch=aarch64 ;;
  *)      die "PROVIDER inválido: '$PROVIDER' (use gcp ou oracle)" ;;
esac
[[ $MC_PORT =~ ^[0-9]+$ ]] || die "MC_PORT inválida: $MC_PORT"
[[ $MC_PORT != 25575 ]] || die "MC_PORT não pode ser 25575 (porta do RCON)"
arch=$(uname -m)
ok "provedor: $PROVIDER"
if [[ $arch == "$expected_arch" ]]; then
  ok "arquitetura $arch"
else
  warn "arquitetura $arch (esperado $expected_arch para $PROVIDER); seguindo mesmo assim"
fi
if [[ $HAS_SYSTEMD == 0 ]]; then warn "systemd ausente: modo de teste (sem serviços, firewall e Tailscale)"; fi

# ---------------------------------------------------------------------------
step "2/12 Pacotes"
export DEBIAN_FRONTEND=noninteractive NEEDRESTART_MODE=a
packages=(openjdk-21-jre-headless python3 zstd curl jq ca-certificates tzdata unattended-upgrades)
if [[ $PROVIDER == oracle ]]; then
  # Evita a pergunta interativa do iptables-persistent
  echo 'iptables-persistent iptables-persistent/autosave_v4 boolean false' | debconf-set-selections
  echo 'iptables-persistent iptables-persistent/autosave_v6 boolean false' | debconf-set-selections
  packages+=(iptables-persistent netfilter-persistent)
else
  packages+=(ufw)
fi
apt-get update -q
apt-get install -y -q "${packages[@]}"
cat >/etc/apt/apt.conf.d/20auto-upgrades <<'EOF'
APT::Periodic::Update-Package-Lists "1";
APT::Periodic::Unattended-Upgrade "1";
EOF
if [[ $HAS_SYSTEMD == 1 ]]; then
  systemctl enable --now unattended-upgrades >/dev/null 2>&1
  ok "unattended-upgrades ativo"
fi
java_bin=""
for candidate in /usr/lib/jvm/java-21-openjdk-*/bin/java; do
  if [[ -x $candidate ]]; then java_bin=$candidate; break; fi
done
[[ -n $java_bin ]] || die "Java 21 não encontrado após instalar openjdk-21-jre-headless"
ok "$("$java_bin" -version 2>&1 | sed -n 1p)"

# ---------------------------------------------------------------------------
step "3/12 Fuso horário America/Sao_Paulo"
if [[ $HAS_SYSTEMD == 1 ]]; then
  if [[ $(timedatectl show -p Timezone --value 2>/dev/null || true) != America/Sao_Paulo ]]; then
    timedatectl set-timezone America/Sao_Paulo
  fi
else
  ln -sf /usr/share/zoneinfo/America/Sao_Paulo /etc/localtime
  echo America/Sao_Paulo >/etc/timezone
fi
ok "$(date)"

# ---------------------------------------------------------------------------
step "4/12 Usuário minecraft e pastas"
if ! id minecraft >/dev/null 2>&1; then
  useradd --system --user-group --home-dir "$BASE" --no-create-home --shell /usr/sbin/nologin minecraft
  ok "usuário minecraft criado"
else
  ok "usuário minecraft já existe"
fi
install -d -m 755 -o root -g root "$BASE" "$TOOLS"
install -d -m 750 -o minecraft -g minecraft "$SERVER" "$SERVER/mods"
install -d -m 755 -o minecraft -g minecraft "$BACKUPS"   # legível para o pull-backup.ps1

install -m 755 "$SRC/start.sh" "$BASE/start.sh"
for f in "${TOOL_FILES[@]}"; do
  install -m 755 "$SRC/tools/$f" "$TOOLS/$f"
done
if [[ -n $CONFIG_SRC && $CONFIG_SRC != "$BASE/config.env" ]]; then
  install -m 640 -o root -g minecraft "$CONFIG_SRC" "$BASE/config.env"
fi
ok "scripts em $TOOLS"

# ---------------------------------------------------------------------------
step "5/12 Minecraft $MC_VERSION: vanilla + Fabric + Lithium"
upd_args=(--version "$MC_VERSION" --dir "$SERVER")
if [[ $ALLOW_EXPERIMENTAL == 1 ]]; then upd_args+=(--allow-experimental); fi
"$TOOLS/update-server.sh" "${upd_args[@]}"
chown -R minecraft:minecraft "$SERVER/mods" "$SERVER"/server.jar* "$SERVER"/fabric-server-launch.jar*

# ---------------------------------------------------------------------------
step "6/12 EULA e server.properties"
echo "eula=true" >"$SERVER/eula.txt"
chown minecraft:minecraft "$SERVER/eula.txt"
ok "eula=true"
if [[ ! -f $SERVER/server.properties ]]; then
  install -m 640 -o minecraft -g minecraft "$SRC/server.properties" "$SERVER/server.properties"
  ok "server.properties criado a partir do template (modo offline, sem whitelist)"
else
  ok "server.properties já existe; ajustes manuais preservados"
fi

# ---------------------------------------------------------------------------
step "7/12 Senha do RCON"
if [[ ! -s $SECRET ]]; then
  (umask 077; python3 -c 'import secrets; print(secrets.token_hex(24))' >"$SECRET")
  ok "senha gerada em $SECRET"
else
  ok "senha já existe em $SECRET"
fi
chown minecraft:minecraft "$SECRET"
chmod 600 "$SECRET"

# Define chaves no server.properties sem tocar no resto.
# Caracteres não-ASCII viram \uXXXX, que o Java lê em qualquer encoding.
set_props() {
  python3 - "$@" <<'PY'
import sys
path, pairs = sys.argv[1], sys.argv[2:]

def esc(value):
    out = []
    for ch in value:
        code = ord(ch)
        if ch == "\\":
            out.append("\\\\")
        elif code > 0xFFFF:
            code -= 0x10000
            out.append("\\u%04x\\u%04x" % (0xD800 + (code >> 10), 0xDC00 + (code & 0x3FF)))
        elif code > 126:
            out.append("\\u%04x" % code)
        else:
            out.append(ch)
    return "".join(out)

want = {}
for pair in pairs:
    key, value = pair.split("=", 1)
    want[key] = esc(value)

with open(path, encoding="utf-8", errors="surrogateescape") as f:
    lines = f.read().splitlines()
seen, out = set(), []
for line in lines:
    key = line.split("=", 1)[0].strip()
    if "=" in line and not line.lstrip().startswith(("#", "!")) and key in want:
        out.append(f"{key}={want[key]}")
        seen.add(key)
    else:
        out.append(line)
out += [f"{k}={v}" for k, v in want.items() if k not in seen]
with open(path, "w", encoding="utf-8", errors="surrogateescape") as f:
    f.write("\n".join(out) + "\n")
PY
}
set_props "$SERVER/server.properties" \
  "motd=$MOTD" \
  "server-port=$MC_PORT" \
  "enable-rcon=true" \
  "rcon.port=25575" \
  "rcon.password=$(cat "$SECRET")"
ok "motd, porta e RCON aplicados"

offline_mode=0
if grep -q '^online-mode=false' "$SERVER/server.properties"; then offline_mode=1; fi

# ---------------------------------------------------------------------------
step "8/12 Serviços systemd"
install -m 644 "$SRC/systemd/minecraft.service" "$SRC/systemd/minecraft-backup.service" \
  "$SRC/systemd/minecraft-backup.timer" /etc/systemd/system/
if [[ $HAS_SYSTEMD == 1 ]]; then
  systemctl daemon-reload
  systemctl enable minecraft.service minecraft-backup.timer >/dev/null 2>&1
  systemctl restart minecraft-backup.timer
  ok "minecraft.service e minecraft-backup.timer habilitados"
else
  warn "units copiadas, mas não habilitadas (sem systemd)"
fi

# ---------------------------------------------------------------------------
step "9/12 Comando mc"
install -m 755 "$SRC/tools/mc" /usr/local/bin/mc
ok "/usr/local/bin/mc"

# ---------------------------------------------------------------------------
# Firewall. O RCON (25575) nunca é liberado em nenhum provedor.

# Oracle: a imagem Ubuntu tem uma regra REJECT no INPUT; nossas regras entram antes dela.
# Regras marcadas com comentário são removidas e recriadas a cada execução (idempotente).
firewall_oracle() {
  remove_tagged() {
    local tag=$1 rule
    while rule=$(iptables -S INPUT | grep -- "--comment $tag" | sed -n 1p) && [[ -n $rule ]]; do
      # shellcheck disable=SC2086
      iptables ${rule/-A INPUT/-D INPUT}
    done
  }
  insert_before_reject() {
    local pos
    pos=$(iptables -L INPUT --line-numbers -n | awk '$2 == "REJECT" {print $1; exit}')
    if [[ -n $pos ]]; then iptables -I INPUT "$pos" "$@"; else iptables -A INPUT "$@"; fi
  }

  remove_tagged mc-tailscale
  remove_tagged mc-public
  insert_before_reject -i tailscale0 -p tcp --dport "$MC_PORT" -m comment --comment mc-tailscale -j ACCEPT
  ok "porta $MC_PORT liberada na interface do Tailscale"
  if [[ $PUBLIC_PORT == 1 ]]; then
    insert_before_reject -p tcp --dport "$MC_PORT" -m state --state NEW -m comment --comment mc-public -j ACCEPT
    warn "porta $MC_PORT ABERTA para a internet. Libere também na Security List da VCN."
  else
    ok "porta pública fechada (PUBLIC_PORT=0)"
  fi
  netfilter-persistent save >/dev/null 2>&1
  # Não persiste as chains do Tailscale (ts-*): ele recria as dele no boot
  for f in /etc/iptables/rules.v4 /etc/iptables/rules.v6; do
    if [[ -f $f ]]; then sed -i '/ts-/d' "$f"; fi
  done
  ok "regras salvas em /etc/iptables"
}

# Google Cloud: não há regras REJECT na imagem; usamos ufw com "nega tudo que entra".
firewall_gcp() {
  ufw default deny incoming >/dev/null
  ufw default allow outgoing >/dev/null
  ufw allow 22/tcp comment 'ssh' >/dev/null
  ufw allow in on tailscale0 to any port "$MC_PORT" proto tcp comment 'minecraft via tailscale' >/dev/null
  ok "SSH (22) liberado; porta $MC_PORT liberada só na interface do Tailscale"
  if [[ $PUBLIC_PORT == 1 ]]; then
    ufw allow "$MC_PORT/tcp" comment 'minecraft publico' >/dev/null
    warn "porta $MC_PORT ABERTA para a internet. Crie também uma regra no firewall da VPC do GCP."
  else
    ufw delete allow "$MC_PORT/tcp" >/dev/null 2>&1 || true
    ok "porta pública fechada (PUBLIC_PORT=0)"
  fi
  ufw --force enable >/dev/null
  ok "ufw ativo: $(ufw status | sed -n 1p)"
}

step "10/12 Firewall ($PROVIDER)"
if [[ $HAS_SYSTEMD == 0 ]]; then
  warn "pulado (sem systemd)"
else
  "firewall_$PROVIDER"
  if [[ $PUBLIC_PORT == 1 && $offline_mode == 1 ]]; then
    warn "ATENÇÃO: modo offline e sem whitelist com a porta pública aberta: QUALQUER pessoa"
    warn "na internet entra com QUALQUER nick (inclusive o seu). Recomendado: PUBLIC_PORT=0."
  fi
fi

# ---------------------------------------------------------------------------
step "11/12 Tailscale"
TS_LOGIN_URL=""
if [[ $HAS_SYSTEMD == 0 ]]; then
  warn "pulado (sem systemd)"
else
  if ! command -v tailscale >/dev/null 2>&1; then
    curl -fsSL https://tailscale.com/install.sh | sh
  fi
  systemctl enable --now tailscaled >/dev/null 2>&1
  ts_state=$(tailscale status --json 2>/dev/null | jq -r '.BackendState // empty' || true)
  if [[ $ts_state == Running ]]; then
    ok "já conectado à sua rede Tailscale"
  elif [[ -n $TS_AUTHKEY ]]; then
    tailscale up --hostname="$TS_HOSTNAME" --authkey="$TS_AUTHKEY"
    ok "conectado com auth key"
  else
    # "tailscale up" fica esperando o login: roda em segundo plano e só mostramos o link
    pkill -f '^tailscale up' 2>/dev/null || true
    setsid nohup tailscale up --hostname="$TS_HOSTNAME" </dev/null >"$TS_LOG" 2>&1 &
    for _ in $(seq 1 30); do
      TS_LOGIN_URL=$(grep -oE 'https://login\.tailscale\.com/[^[:space:]]+' "$TS_LOG" 2>/dev/null | sed -n 1p || true)
      if [[ -z $TS_LOGIN_URL ]]; then
        TS_LOGIN_URL=$(tailscale status --json 2>/dev/null | jq -r '.AuthURL // empty' || true)
      fi
      [[ -n $TS_LOGIN_URL ]] && break
      sleep 1
    done
    if [[ -n $TS_LOGIN_URL ]]; then
      printf '\n%s  ┌────────────────────────────────────────────────────────────────┐%s\n' "$c_warn" "$c_off"
      printf '%s  │  ABRA ESTE LINK NO NAVEGADOR PARA LIGAR A VM AO SEU TAILSCALE  │%s\n' "$c_warn" "$c_off"
      printf '%s  └────────────────────────────────────────────────────────────────┘%s\n' "$c_warn" "$c_off"
      printf '\n      %s\n\n' "$TS_LOGIN_URL"
    else
      warn "link de login ainda não apareceu. Veja: sudo cat $TS_LOG  (ou rode: sudo tailscale up)"
    fi
  fi
fi

# ---------------------------------------------------------------------------
step "12/12 Iniciando o servidor"
players=""
if [[ $HAS_SYSTEMD == 0 ]]; then
  warn "pulado (sem systemd). Para testar: runuser -u minecraft -- $BASE/start.sh"
else
  FP_AFTER=$(fingerprint)
  if systemctl is-active --quiet minecraft; then
    if [[ $FP_BEFORE != "$FP_AFTER" ]]; then
      warn "configuração mudou: reiniciando o servidor"
      if "$TOOLS/rcon.py" "say Servidor reiniciando em 10s para atualização." >/dev/null 2>&1; then sleep 10; fi
      systemctl restart minecraft
    else
      ok "servidor já rodando e nada mudou; sem reiniciar"
    fi
  else
    systemctl start minecraft
    ok "servidor iniciado (o primeiro boot baixa bibliotecas e gera o mundo: 1-3 min)"
  fi

  echo "    aguardando o servidor responder no RCON..."
  for _ in $(seq 1 150); do
    if players=$("$TOOLS/rcon.py" list 2>/dev/null); then break; fi
    players=""
    if ! systemctl is-active --quiet minecraft; then break; fi
    sleep 2
  done
fi

# ---------------------------------------------------------------------------
ts_ip=$(tailscale ip -4 2>/dev/null | sed -n 1p || true)
printf '\n%s================ RESUMO ================%s\n' "$c_step" "$c_off"
echo "Provedor:    $PROVIDER ($arch)"
echo "Serviço:     $(systemctl is-active minecraft 2>/dev/null || echo '-')"
echo "Servidor:    $(cat "$SERVER/fabric-server-launch.jar.version" 2>/dev/null || echo '?')"
echo "Lithium:     $(cat "$SERVER/mods/lithium.jar.version" 2>/dev/null || echo '?')"
echo "Jogadores:   ${players:-(ainda iniciando; acompanhe com: mc logs)}"
if [[ -n $ts_ip ]]; then
  echo "Endereço:    $ts_ip:$MC_PORT   (pelo Tailscale)"
else
  echo "Endereço:    Tailscale ainda não logado${TS_LOGIN_URL:+. Link: $TS_LOGIN_URL}"
  echo "             depois do login, veja o IP com: tailscale ip -4"
fi
if [[ $PUBLIC_PORT == 1 ]]; then echo "Pública:     <IP público da VM>:$MC_PORT"; fi
if [[ $offline_mode == 1 ]]; then echo "Modo:        offline (aceita TLauncher), sem whitelist"; fi
echo "Backup:      todo dia às 05:00 (Brasília), mantém ${BACKUP_KEEP:-7}"
cat <<'EOF'

Próximos comandos (na VM):
  mc status            situação geral
  mc tps               desempenho (tempo de tick)
  mc logs              log ao vivo
  mc cmd "say oi"      comando no console
  mc op <nick>         dá operador
  mc backup            backup agora
  mc update            atualiza vanilla/Fabric/Lithium
EOF
