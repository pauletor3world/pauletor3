# Gera o instalador do Machen Craft (build pessoal) e publica no SEU Worker de coordenação.
# Os amigos baixam pelo link do convite, e quem já tem o app recebe a atualização sozinho.
# Nada vai para um site público.
#
#   powershell -ExecutionPolicy Bypass -File scripts\publicar-app.ps1               # versão atual
#   powershell -ExecutionPolicy Bypass -File scripts\publicar-app.ps1 -Versao 2.0.1 # nova versão
#
# Requer: scripts\publicar-coordenacao.ps1 já rodado (endereço e chave em scripts\.cache),
# e a chave de assinatura das atualizações (padrão: D:\DevTools\pauletor3-signing).
param(
  [string]$Versao = "",
  [string]$Notas = "",
  [string]$PastaChave = "D:\DevTools\pauletor3-signing",
  [switch]$SemCompilar
)
$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

function Fail($t) { Write-Host ""; Write-Host "ERRO: $t" -ForegroundColor Red; exit 1 }

$saved = Join-Path $PSScriptRoot ".cache\coordenacao.txt"
if (-not (Test-Path $saved)) { Fail "Rode antes scripts\publicar-coordenacao.ps1." }
$cfg = @{}
Get-Content $saved | ForEach-Object { $k, $v = $_ -split "=", 2; $cfg[$k] = $v }
$url = $cfg["ENDERECO"]; $admin = $cfg["CHAVE"]
if (-not $url -or -not $admin) { Fail "scripts\.cache\coordenacao.txt incompleto; rode publicar-coordenacao.ps1 de novo." }

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

if (-not $SemCompilar) {
  Write-Host "==> Compilando o instalador (build pessoal) v$Versao" -ForegroundColor Green
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

Write-Host "==> Enviando $($exe.Name) ($([math]::Round($exe.Length / 1MB, 1)) MB) para $url" -ForegroundColor Green
$headers = @{
  "x-p3-admin" = $admin
  "x-p3-version" = $Versao
  "x-p3-updater-signature" = (Get-Content $sig -Raw).Trim()
  "x-p3-notes" = $Notas
}
$r = Invoke-RestMethod -Method Put -Uri "$url/v1/app/installer" -Headers $headers -InFile $exe.FullName -ContentType "application/octet-stream" -TimeoutSec 600
if (-not $r.ok) { Fail "O Worker não confirmou a publicação." }
Write-Host ""
Write-Host "Pronto: versão $Versao publicada. Convites novos já baixam esta versão, e os apps abertos se atualizam em até 6 horas." -ForegroundColor Green
