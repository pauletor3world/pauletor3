#!/usr/bin/env bash
# Atualiza o servidor: server.jar vanilla (Mojang, confere SHA-1), launcher do Fabric
# (meta.fabricmc.net) e Lithium (Modrinth, confere SHA-512). Cada arquivo é trocado de
# forma atômica e o anterior fica em .bak.
# Uso: update-server.sh [--version 1.21.11] [--dir /opt/minecraft/server]
#                       [--allow-experimental] [--force]
set -Eeuo pipefail

MOJANG_MANIFEST="https://piston-meta.mojang.com/mc/game/version_manifest_v2.json"
FABRIC_META="https://meta.fabricmc.net/v2/versions"
MODRINTH="https://api.modrinth.com/v2"

log()  { echo "[update] $*"; }
warn() { echo "[update] AVISO: $*" >&2; }
die()  { echo "[update] ERRO: $*" >&2; exit 1; }

if [[ -f /opt/minecraft/config.env ]]; then
  # shellcheck source=/dev/null
  source /opt/minecraft/config.env
fi

VERSION="${MC_VERSION:-1.21.11}"
DIR="/opt/minecraft/server"
ALLOW_EXP="${ALLOW_EXPERIMENTAL:-0}"
FORCE=0

while (($#)); do
  case $1 in
    --version)            VERSION=$2; shift 2 ;;
    --dir)                DIR=$2; shift 2 ;;
    --allow-experimental) ALLOW_EXP=1; shift ;;
    --force)              FORCE=1; shift ;;
    -h|--help)            sed -n '2,7p' "$0"; exit 0 ;;
    *)                    die "opção desconhecida: $1" ;;
  esac
done

# O Modrinth pede um User-Agent que identifique o projeto (e, de preferência, um contato)
UA="mc-server-setup/2.0 (servidor pessoal${UPDATE_CONTACT:+; $UPDATE_CONTACT})"

for cmd in curl jq sha1sum sha512sum python3; do
  command -v "$cmd" >/dev/null || die "comando ausente: $cmd"
done

TMP_FILES=()
cleanup() { if ((${#TMP_FILES[@]})); then rm -f "${TMP_FILES[@]}"; fi; }
trap cleanup EXIT

api() { curl -fsS --retry 3 --retry-delay 2 -A "$UA" "$@"; }

# Baixa $1 para um temporário na pasta de $2 e devolve o caminho
download_tmp() {
  local url=$1 dest=$2 tmp
  mkdir -p "$(dirname "$dest")"
  tmp=$(mktemp "$(dirname "$dest")/.download.XXXXXX")
  TMP_FILES+=("$tmp")
  curl -fsSL --retry 3 --retry-delay 2 -A "$UA" -o "$tmp" "$url" || die "falha no download: $url"
  echo "$tmp"
}

# Troca atômica: guarda o atual em .bak e renomeia o temporário
install_atomic() {
  local tmp=$1 dest=$2 label=$3
  chmod 644 "$tmp"
  if [[ -f $dest ]]; then cp -p "$dest" "$dest.bak"; fi
  mv -f "$tmp" "$dest"
  echo "$label" >"$dest.version"
  log "instalado: $label -> $dest"
}

current_label() { cat "$1.version" 2>/dev/null || true; }

# --- 1. server.jar vanilla (Mojang informa o SHA-1) -------------------------
update_vanilla() {
  local dest="$DIR/server.jar" vurl meta url sha got tmp
  vurl=$(api "$MOJANG_MANIFEST" | jq -r --arg v "$VERSION" '.versions[] | select(.id == $v) | .url') \
    || die "falha ao consultar o manifesto da Mojang"
  [[ -n $vurl ]] || die "versão $VERSION não existe no manifesto da Mojang"
  meta=$(api "$vurl") || die "falha ao consultar $vurl"
  url=$(jq -r '.downloads.server.url // empty' <<<"$meta")
  sha=$(jq -r '.downloads.server.sha1 // empty' <<<"$meta")
  [[ -n $url && -n $sha ]] || die "a Mojang não informou URL/SHA-1 do servidor $VERSION"

  if [[ $FORCE != 1 && -f $dest ]] && [[ $(sha1sum "$dest" | cut -d' ' -f1) == "$sha" ]]; then
    log "vanilla $VERSION: já atualizado"
    echo "minecraft-server-$VERSION" >"$dest.version"
    return
  fi
  log "vanilla $VERSION: baixando"
  tmp=$(download_tmp "$url" "$dest")
  got=$(sha1sum "$tmp" | cut -d' ' -f1)
  [[ $got == "$sha" ]] || die "SHA-1 do server.jar não confere (esperado $sha, recebido $got)"
  log "vanilla $VERSION: SHA-1 ok"
  install_atomic "$tmp" "$dest" "minecraft-server-$VERSION"
}

# --- 2. Launcher do Fabric ---------------------------------------------------
# O meta.fabricmc.net não publica hash do launcher: validamos que é um jar íntegro
# com a classe principal do launcher do Fabric.
pick_fabric() { # $1 = JSON, $2 = filtro jq que devolve a lista de {version, stable}
  local v
  v=$(jq -r "$2 | map(select(.stable == true)) | .[0].version // empty" <<<"$1")
  if [[ -z $v && $ALLOW_EXP == 1 ]]; then
    v=$(jq -r "$2 | .[0].version // empty" <<<"$1")
    [[ -z $v ]] || warn "sem versão estável; usando $v (--allow-experimental)"
  fi
  echo "$v"
}

update_fabric() {
  local dest="$DIR/fabric-server-launch.jar" loaders installers loader installer label tmp
  loaders=$(api "$FABRIC_META/loader/$VERSION") || die "falha ao consultar o Fabric Meta"
  [[ $(jq 'length' <<<"$loaders") -gt 0 ]] || die "o Fabric não suporta o Minecraft $VERSION"
  installers=$(api "$FABRIC_META/installer") || die "falha ao consultar o Fabric Meta"

  loader=$(pick_fabric "$loaders" '[.[] | .loader]')
  installer=$(pick_fabric "$installers" '.')
  [[ -n $loader ]] || die "não há loader ESTÁVEL do Fabric para $VERSION (use --allow-experimental)"
  [[ -n $installer ]] || die "não há installer ESTÁVEL do Fabric (use --allow-experimental)"

  label="fabric-mc$VERSION-loader$loader-installer$installer"
  if [[ $FORCE != 1 && -f $dest && $(current_label "$dest") == "$label" ]]; then
    log "Fabric loader $loader: já atualizado"
    return
  fi
  log "Fabric loader $loader (installer $installer): baixando"
  tmp=$(download_tmp "$FABRIC_META/loader/$VERSION/$loader/$installer/server/jar" "$dest")
  python3 - "$tmp" <<'PY' || die "o arquivo baixado do Fabric não é um launcher válido"
import sys, zipfile
z = zipfile.ZipFile(sys.argv[1])
assert z.testzip() is None
assert "Main-Class: net.fabricmc.installer.ServerLauncher" in z.read("META-INF/MANIFEST.MF").decode()
PY
  log "Fabric: jar íntegro"
  install_atomic "$tmp" "$dest" "$label"
}

# --- 3. Lithium (Modrinth informa o SHA-512) --------------------------------
update_lithium() {
  local dest="$DIR/mods/lithium.jar" versions ver file name url sha got tmp vtype
  versions=$(api -G "$MODRINTH/project/lithium/version" \
      --data-urlencode "game_versions=[\"$VERSION\"]" \
      --data-urlencode 'loaders=["fabric"]') || die "falha ao consultar o Modrinth"

  ver=$(jq -c 'sort_by(.date_published) | reverse | map(select(.version_type == "release")) | .[0] // empty' <<<"$versions")
  if [[ -z $ver && $ALLOW_EXP == 1 ]]; then
    ver=$(jq -c 'sort_by(.date_published) | reverse | .[0] // empty' <<<"$versions")
    [[ -z $ver ]] || warn "sem Lithium release para $VERSION; usando $(jq -r .version_number <<<"$ver") (--allow-experimental)"
  fi
  [[ -n $ver ]] || die "não há Lithium (release) para Fabric $VERSION no Modrinth (use --allow-experimental)"

  file=$(jq -c '(.files | map(select(.primary)) | .[0]) // .files[0]' <<<"$ver")
  name=$(jq -r '.filename' <<<"$file")
  url=$(jq -r '.url' <<<"$file")
  sha=$(jq -r '.hashes.sha512 // empty' <<<"$file")
  vtype=$(jq -r '.version_type' <<<"$ver")
  [[ -n $sha && $url == https://* ]] || die "o Modrinth não informou URL/SHA-512 do Lithium"

  if [[ $FORCE != 1 && -f $dest ]] && [[ $(sha512sum "$dest" | cut -d' ' -f1) == "$sha" ]]; then
    log "Lithium $name: já atualizado"
    echo "$name" >"$dest.version"
    return
  fi
  log "Lithium $name ($vtype): baixando"
  tmp=$(download_tmp "$url" "$dest")
  got=$(sha512sum "$tmp" | cut -d' ' -f1)
  [[ $got == "$sha" ]] || die "SHA-512 do Lithium não confere. Arquivo atual mantido."
  log "Lithium: SHA-512 ok"
  install_atomic "$tmp" "$dest" "$name"
}

log "Minecraft $VERSION em $DIR"
update_vanilla
update_fabric
update_lithium
log "pronto"
