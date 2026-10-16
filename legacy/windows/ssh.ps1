<#
.SYNOPSIS
  Conecta na VM por SSH (usa o IP, a chave e o usuário salvos em windows\local.json).
.EXAMPLE
  .\windows\ssh.ps1                  # abre um terminal na VM
.EXAMPLE
  .\windows\ssh.ps1 mc status        # roda um comando e sai
#>
[CmdletBinding(PositionalBinding = $false)]
param(
    [string]$Ip,
    [string]$Key,
    [string]$User,
    [Parameter(ValueFromRemainingArguments = $true)]
    [string[]]$Command
)
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'common.ps1')

$conn = Resolve-Connection -Ip $Ip -Key $Key -User $User
Repair-KeyPermission $conn.Key
$ssh = Get-SshOptions $conn

if ($Command) {
    & ssh @ssh -t "$($conn.User)@$($conn.Ip)" ($Command -join ' ')
} else {
    & ssh @ssh "$($conn.User)@$($conn.Ip)"
}
exit $LASTEXITCODE
