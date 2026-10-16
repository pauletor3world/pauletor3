<#
.SYNOPSIS
  Cria uma chave SSH ed25519 para a VM do Google Cloud e mostra a chave pública para colar no painel.
.DESCRIPTION
  A chave fica em %USERPROFILE%\.ssh\gcp-mc (privada) e gcp-mc.pub (pública). O comentário da chave
  é o nome de usuário: o Google Cloud cria esse usuário na VM a partir dele.
  Também salva o usuário e o caminho da chave em windows\local.json para os outros scripts.
.EXAMPLE
  .\windows\new-key.ps1 -User machen
#>
param(
    [Parameter(Mandatory = $true)]
    [string]$User,
    [switch]$Force
)
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'common.ps1')

if ($User -cnotmatch '^[a-z_][a-z0-9_-]{0,31}$') {
    throw 'Usuário inválido. Use só letras minúsculas, números, "_" ou "-", começando com letra (ex.: machen).'
}

$sshDir = Join-Path $env:USERPROFILE '.ssh'
$key = Join-Path $sshDir 'gcp-mc'
$pub = "$key.pub"
New-Item -ItemType Directory -Force -Path $sshDir | Out-Null

if ((Test-Path -LiteralPath $key) -and -not $Force) {
    Write-Host "A chave já existe em $key (use -Force para criar outra). Reaproveitando." -ForegroundColor Yellow
    $comment = ((Get-Content -LiteralPath $pub -Raw).Trim() -split '\s+')[2]
    if ($comment -ne $User) {
        throw "A chave existente é do usuário '$comment', não '$User'. Use -User $comment ou -Force para recriar."
    }
} else {
    if (Test-Path -LiteralPath $key) { Remove-Item -LiteralPath $key, $pub -Force }
    # Senha vazia: o PowerShell 5.1 descarta '' ao chamar programas, por isso o '""'
    if ($PSVersionTable.PSVersion -ge [version]'7.3') { $empty = '' } else { $empty = '""' }
    & ssh-keygen -q -t ed25519 -f $key -C $User -N $empty
    Assert-ExitCode 'ssh-keygen'
    Write-Host "Chave criada: $key" -ForegroundColor Green
}

Repair-KeyPermission $key
Save-LocalConfig -Key $key -User $User

$publicKey = (Get-Content -LiteralPath $pub -Raw).Trim()
try { Set-Clipboard -Value $publicKey; $copied = $true } catch { $copied = $false }

Write-Host ''
Write-Host 'Cole a linha abaixo no Google Cloud: VM > Editar > Chaves SSH > Adicionar item > Salvar' -ForegroundColor Cyan
Write-Host '--------------------------------------------------------------------------------'
Write-Host $publicKey
Write-Host '--------------------------------------------------------------------------------'
if ($copied) { Write-Host '(já copiada para a área de transferência: é só dar Ctrl+V)' -ForegroundColor Green }
Write-Host "Usuário na VM: $User   (salvo em windows\local.json junto com a chave)"
