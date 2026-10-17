# Gera o instalador do Machen Craft (build pessoal) e publica no SEU Worker de coordenação.
# Os amigos baixam pelo link do convite, e quem já tem o app recebe a atualização sozinho
# (o app procura a cada 1 h). Nada vai para um site público.
#
#   powershell -ExecutionPolicy Bypass -File scripts\publicar-app.ps1               # versão atual
#   powershell -ExecutionPolicy Bypass -File scripts\publicar-app.ps1 -Versao 2.1.0 # nova versão
#   ... -Canal teste          # só para quem está no canal de teste (ver docs/atualizacao.md)
#   ... -Obrigatoria          # marca a versão como obrigatória
#
# As notas vêm do CHANGELOG.md (seção da versão), ou de -Notas / -ArquivoNotas.
# Endereço e chave do Worker: variáveis COORDENACAO_URL e ADMIN_TOKEN (usadas no GitHub
# Actions) ou scripts\.cache\coordenacao.txt (gravado por publicar-coordenacao.ps1).
# Chave de assinatura das atualizações: -PastaChave (padrão D:\DevTools\pauletor3-signing).
param(
  [string]$Versao = "",
  [string]$Notas = "",
  [string]$ArquivoNotas = "",
  [ValidateSet("estavel", "teste")]
  [string]$Canal = "estavel",
  [switch]$Obrigatoria,
  [string]$PastaChave = "D:\DevTools\pauletor3-signing",
  [switch]$SemCompilar
)
$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

function Fail($t) { Write-Host ""; Write-Host "ERRO: $t" -ForegroundColor Red; exit 1 }

$url = $env:COORDENACAO_URL; $admin = $env:ADMIN_TOKEN
if (-not $url -or -not $admin) {
  $saved = Join-Path $PSScriptRoot ".cache\coordenacao.txt"
  if (-not (Test-Path $saved)) { Fail "Rode antes scripts\publicar-coordenacao.ps1 (ou defina COORDENACAO_URL e ADMIN_TOKEN)." }
  $cfg = @{}
  Get-Content $saved | ForEach-Object { $k, $v = $_ -split "=", 2; $cfg[$k] = $v }
  $url = $cfg["ENDERECO"]; $admin = $cfg["CHAVE"]
}
if (-not $url -or -not $admin) { Fail "Endereço ou chave do Worker ausentes; rode publicar-coordenacao.ps1 de novo." }
$url = $url.TrimEnd("/")

function SetVersion($path, $pattern, $replacement) {
  $full = Join-Path $root $path
  $text = [IO.File]::ReadAllText($full)
  $new = ([regex]$pattern).Replace($text, $replacement, 1)
  [IO.File]::WriteAllText($full, $new, (New-Object System.Text.UTF8Encoding($false)))
}
if ($Versao) {
  SetVersion "app\src-tauri\tauri.conf.json" '"version": "[^"]+"' "`"version`": `"$Versao`""
  SetVersion "app\package.json" '"version": "[^"]+"' "`"version`": `"$Versao`""
  SetVersion "Cargo.toml" '(?m)^version = "[^"]+"' "version = `"$Versao`""
} else {
  $Versao = (Get-Content app\src-tauri\tauri.conf.json -Raw | ConvertFrom-Json).version
}
node scripts/conferir-versao.mjs
if ($LASTEXITCODE -ne 0) { Fail "Versões diferentes entre os arquivos." }

if (-not $SemCompilar) {
  Write-Host "==> Compilando o instalador (build pessoal) v$Versao" -ForegroundColor Green
  # a chave fica só em variável de ambiente deste processo (nunca em arquivo do repositório nem no log)
  $env:TAURI_SIGNING_PRIVATE_KEY = (Get-Content (Join-Path $PastaChave "pauletor3-updater.key") -Raw).Trim()
  $env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = (Get-Content (Join-Path $PastaChave "senha-da-chave.txt") -Raw).Trim()
  Push-Location app
  npm ci --no-audit --no-fund
  if ($LASTEXITCODE -ne 0) { Pop-Location; Fail "npm ci falhou." }
  npx tauri build --features personal
  $code = $LASTEXITCODE
  Pop-Location
  Remove-Item Env:\TAURI_SIGNING_PRIVATE_KEY, Env:\TAURI_SIGNING_PRIVATE_KEY_PASSWORD
  if ($code -ne 0) { Fail "A compilação falhou." }
}

$nsis = Join-Path $root "target\release\bundle\nsis"
$exe = Get-ChildItem $nsis -Filter "*$Versao*-setup.exe" | Sort-Object LastWriteTime -Descending | Select-Object -First 1
if (-not $exe) { Fail "Instalador da versão $Versao não encontrado em $nsis." }
$sig = "$($exe.FullName).sig"
if (-not (Test-Path $sig)) { Fail "Assinatura $sig não encontrada (a chave de assinatura estava disponível?)." }

if ($ArquivoNotas) { $Notas = [IO.File]::ReadAllText($ArquivoNotas) }
if (-not $Notas) { $Notas = (node scripts/notas-da-versao.mjs $Versao) -join "`n" }
$notasB64 = [Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes($Notas.Trim()))

$destino = "$url/v1/app/installer"
if ($Canal -eq "teste") { $destino += "?canal=teste" }
Write-Host "==> Enviando $($exe.Name) ($([math]::Round($exe.Length / 1MB, 1)) MB), canal $Canal" -ForegroundColor Green
$headers = @{
  "x-p3-admin" = $admin
  "x-p3-version" = $Versao
  "x-p3-updater-signature" = (Get-Content $sig -Raw).Trim()
  "x-p3-notes-b64" = $notasB64
}
if ($Obrigatoria) { $headers["x-p3-obrigatoria"] = "1" }
$r = Invoke-RestMethod -Method Put -Uri $destino -Headers $headers -InFile $exe.FullName -ContentType "application/octet-stream" -TimeoutSec 600
if (-not $r.ok) { Fail "O Worker não confirmou a publicação." }
Write-Host ""
Write-Host "Pronto: versão $Versao publicada (canal $Canal). Convites novos já baixam esta versão, e os apps abertos se atualizam em até 1 hora." -ForegroundColor Green
