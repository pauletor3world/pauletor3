# Ajuda nos testes de ponta a ponta da atualização automática (roteiro em docs/atualizacao.md).
#
#   powershell -ExecutionPolicy Bypass -File scripts\e2e-atualizacao.ps1 hash-mundo [-Pasta D:\MachenCraft]
#   powershell -ExecutionPolicy Bypass -File scripts\e2e-atualizacao.ps1 publicar-teste -Versao 2.1.1-beta.1
#   powershell -ExecutionPolicy Bypass -File scripts\e2e-atualizacao.ps1 assinatura-errada -Versao 2.1.1-beta.2
#   powershell -ExecutionPolicy Bypass -File scripts\e2e-atualizacao.ps1 minimo-acima
#   powershell -ExecutionPolicy Bypass -File scripts\e2e-atualizacao.ps1 minimo-tirar
#
# Tudo que publica vai para o canal "teste" (só PCs com "canal_atualizacao": "teste" no config.json).
param(
  [Parameter(Mandatory = $true, Position = 0)]
  [ValidateSet("hash-mundo", "publicar-teste", "assinatura-errada", "minimo-acima", "minimo-tirar")]
  [string]$Acao,
  [string]$Pasta = "",
  [string]$Versao = ""
)
$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root
function Fail($t) { Write-Host "ERRO: $t" -ForegroundColor Red; exit 1 }

function Worker {
  $url = $env:COORDENACAO_URL; $admin = $env:ADMIN_TOKEN
  if (-not $url -or -not $admin) {
    $cfg = @{}
    Get-Content (Join-Path $PSScriptRoot ".cache\coordenacao.txt") | ForEach-Object { $k, $v = $_ -split "=", 2; $cfg[$k] = $v }
    $url = $cfg["ENDERECO"]; $admin = $cfg["CHAVE"]
  }
  return @{ url = $url.TrimEnd("/"); admin = $admin }
}

switch ($Acao) {
  "hash-mundo" {
    # Hash do mundo salvo (todos os arquivos, em ordem, sem o session.lock): compare antes e depois.
    if (-not $Pasta) { $Pasta = Join-Path $env:APPDATA "MachenCraft" }
    $world = Join-Path $Pasta "server\world"
    if (-not (Test-Path $world)) { Fail "Mundo não encontrado em $world (use -Pasta com a pasta de dados do app)." }
    $sha = [Security.Cryptography.SHA256]::Create()
    $files = Get-ChildItem $world -Recurse -File | Where-Object { $_.Name -ne "session.lock" } | Sort-Object FullName
    $lines = foreach ($f in $files) {
      $rel = $f.FullName.Substring($world.Length).Replace("\", "/")
      "$rel $((Get-FileHash $f.FullName -Algorithm SHA256).Hash)"
    }
    $all = [BitConverter]::ToString($sha.ComputeHash([Text.Encoding]::UTF8.GetBytes(($lines -join "`n")))).Replace("-", "").ToLower()
    Write-Host "$($files.Count) arquivos; hash do mundo: $all"
  }
  "publicar-teste" {
    if (-not $Versao) { Fail "Informe -Versao (maior que a instalada, ex.: 2.1.1-beta.1)." }
    & "$PSScriptRoot\publicar-app.ps1" -Versao $Versao -Canal teste
  }
  "assinatura-errada" {
    if (-not $Versao) { Fail "Informe -Versao (maior que a instalada)." }
    $w = Worker
    $nsis = Join-Path $root "target\release\bundle\nsis"
    $exe = Get-ChildItem $nsis -Filter "*-setup.exe" | Sort-Object LastWriteTime -Descending | Select-Object -First 1
    if (-not $exe) { Fail "Compile um instalador antes (publicar-app.ps1 ou tauri build)." }
    $sig = (Get-Content "$($exe.FullName).sig" -Raw).Trim()
    # adultera a assinatura (troca um caractere do meio do texto base64 interno)
    $raw = [Text.Encoding]::UTF8.GetString([Convert]::FromBase64String($sig))
    $i = [int]($raw.Length / 2)
    $bad = $raw.Substring(0, $i) + ($(if ($raw[$i] -eq 'A') { 'B' } else { 'A' })) + $raw.Substring($i + 1)
    $badSig = [Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes($bad))
    $headers = @{
      "x-p3-admin" = $w.admin; "x-p3-version" = $Versao; "x-p3-updater-signature" = $badSig
      "x-p3-notes-b64" = [Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes("Teste de assinatura errada: o app deve recusar."))
    }
    Invoke-RestMethod -Method Put -Uri "$($w.url)/v1/app/installer?canal=teste" -Headers $headers -InFile $exe.FullName -ContentType "application/octet-stream" -TimeoutSec 600 | Out-Null
    Write-Host "Publicado $Versao com assinatura adulterada no canal teste. O app deve recusar o download." -ForegroundColor Yellow
  }
  "minimo-acima" {
    $atual = (Get-Content app\src-tauri\tauri.conf.json -Raw | ConvertFrom-Json).version
    $p = $atual.Split("-")[0].Split(".")
    $acima = "$($p[0]).$([int]$p[1] + 1).0"
    & "$PSScriptRoot\versao-minima.ps1" -Versao $acima
  }
  "minimo-tirar" {
    & "$PSScriptRoot\versao-minima.ps1" -Tirar
  }
}
