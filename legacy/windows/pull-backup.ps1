<#
.SYNOPSIS
  Baixa o backup mais recente da VM para .\backups-local\
.EXAMPLE
  .\windows\pull-backup.ps1
#>
param(
    [string]$Ip,
    [string]$Key,
    [string]$User
)
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'common.ps1')
$Root = Split-Path -Parent $PSScriptRoot

$conn = Resolve-Connection -Ip $Ip -Key $Key -User $User
Repair-KeyPermission $conn.Key
$target = "$($conn.User)@$($conn.Ip)"
$ssh = Get-SshOptions $conn

$remote = & ssh @ssh $target 'ls -1 /opt/minecraft/backups/world-*.tar.zst 2>/dev/null | sort | tail -n 1'
Assert-ExitCode 'Consulta dos backups'
$remote = "$remote".Trim()
if (-not $remote) { throw 'Nenhum backup encontrado na VM (rode "mc backup" na VM).' }

$destDir = Join-Path $Root 'backups-local'
New-Item -ItemType Directory -Force -Path $destDir | Out-Null
$name = Split-Path -Leaf $remote
$dest = Join-Path $destDir $name

if (Test-Path -LiteralPath $dest) {
    Write-Host "Já baixado: $dest" -ForegroundColor Yellow
    exit 0
}

Write-Host "==> Baixando $name" -ForegroundColor Cyan
Push-Location $destDir
try {
    & scp @ssh "${target}:$remote" $name
    Assert-ExitCode 'Download do backup'
} finally {
    Pop-Location
}

# Confere se o arquivo chegou inteiro: integridade do zstd na VM + mesmo SHA-256 dos dois lados
Write-Host "==> Conferindo o arquivo" -ForegroundColor Cyan
$remoteHash = & ssh @ssh $target "zstd -tq '$remote' && sha256sum '$remote' | cut -d' ' -f1"
Assert-ExitCode 'Teste de integridade na VM'
$localHash = (Get-FileHash -LiteralPath $dest -Algorithm SHA256).Hash.ToLower()
if ("$remoteHash".Trim() -ne $localHash) {
    Remove-Item -LiteralPath $dest -Force
    throw 'O arquivo baixado não bate com o da VM (SHA-256 diferente). Apaguei a cópia local; rode de novo.'
}
$size = '{0:N1} MB' -f ((Get-Item -LiteralPath $dest).Length / 1MB)
Write-Host "Backup salvo e conferido (SHA-256 igual ao da VM): $dest ($size)" -ForegroundColor Green
