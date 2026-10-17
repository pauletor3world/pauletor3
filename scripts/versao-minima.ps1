# Define (ou tira) a versão mínima do app no Worker, sem publicar app novo.
# Abaixo dela, o app bloqueia Jogar e hospedar e mostra só "Atualize para continuar jogando
# com o grupo". A coordenação fora do ar nunca bloqueia ninguém.
#
#   powershell -ExecutionPolicy Bypass -File scripts\versao-minima.ps1 -Versao 2.1.0
#   powershell -ExecutionPolicy Bypass -File scripts\versao-minima.ps1 -Tirar
#   powershell -ExecutionPolicy Bypass -File scripts\versao-minima.ps1            # mostra a atual
param(
  [string]$Versao = "",
  [switch]$Tirar
)
$ErrorActionPreference = "Stop"
function Fail($t) { Write-Host "ERRO: $t" -ForegroundColor Red; exit 1 }

$url = $env:COORDENACAO_URL; $admin = $env:ADMIN_TOKEN
if (-not $url -or -not $admin) {
  $saved = Join-Path $PSScriptRoot ".cache\coordenacao.txt"
  if (-not (Test-Path $saved)) { Fail "Rode antes scripts\publicar-coordenacao.ps1 (ou defina COORDENACAO_URL e ADMIN_TOKEN)." }
  $cfg = @{}
  Get-Content $saved | ForEach-Object { $k, $v = $_ -split "=", 2; $cfg[$k] = $v }
  $url = $cfg["ENDERECO"]; $admin = $cfg["CHAVE"]
}
$url = $url.TrimEnd("/")
$headers = @{ "x-p3-admin" = $admin }

if (-not $Versao -and -not $Tirar) {
  $r = Invoke-RestMethod -Method Get -Uri "$url/v1/app/versao-minima" -Headers $headers
  if ($r.versao_minima) { Write-Host "Versão mínima atual: $($r.versao_minima)" } else { Write-Host "Sem versão mínima." }
  exit 0
}
if ($Versao -and $Versao -notmatch '^\d+\.\d+\.\d+$') { Fail "Use o formato 2.1.0." }
$body = if ($Tirar) { '{"versao_minima":null}' } else { "{`"versao_minima`":`"$Versao`"}" }
$r = Invoke-RestMethod -Method Put -Uri "$url/v1/app/versao-minima" -Headers $headers -Body $body -ContentType "application/json"
if (-not $r.ok) { Fail "O Worker não confirmou." }
if ($r.versao_minima) { Write-Host "Pronto: versão mínima $($r.versao_minima). Vale em até 1 minuto." -ForegroundColor Green }
else { Write-Host "Pronto: sem versão mínima." -ForegroundColor Green }
