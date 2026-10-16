<#
.SYNOPSIS
  Envia um backup (.tar.zst baixado pelo pull-backup.ps1) para a VM e restaura com "mc restore".
  Use para migrar o mundo para outra VM (por exemplo, do Google Cloud para a Oracle).
.EXAMPLE
  .\windows\push-backup.ps1 -File .\backups-local\world-20261015-050000.tar.zst
#>
param(
    [Parameter(Mandatory = $true)]
    [string]$File,
    [string]$Ip,
    [string]$Key,
    [string]$User
)
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'common.ps1')

if (-not (Test-Path -LiteralPath $File -PathType Leaf)) { throw "Arquivo não encontrado: $File" }
if ((Split-Path -Leaf $File) -match '\s') { throw 'O nome do arquivo não pode ter espaços.' }
$conn = Resolve-Connection -Ip $Ip -Key $Key -User $User
Repair-KeyPermission $conn.Key
$ssh = Get-SshOptions $conn

Write-Host "==> Enviando $File" -ForegroundColor Cyan
$remote = Send-ToVm $conn $File

Write-Host "==> Restaurando na VM" -ForegroundColor Cyan
& ssh @ssh -t "$($conn.User)@$($conn.Ip)" "mc restore $remote && rm -f $remote"
Assert-ExitCode 'Restauração'
Write-Host 'Backup restaurado.' -ForegroundColor Green
