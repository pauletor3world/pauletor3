<#
.SYNOPSIS
  Envia um mundo do seu PC (pasta com level.dat) para a VM e o coloca no servidor.
.DESCRIPTION
  Compacta a pasta, envia por scp e roda "mc import-world" na VM. O mundo atual da VM
  ganha um backup e é guardado em server/pre-import-*; nada é apagado.
.EXAMPLE
  .\windows\push-world.ps1 -World "$env:APPDATA\.minecraft\saves\Meu Mundo"
#>
param(
    [Parameter(Mandatory = $true)]
    [string]$World,
    [string]$Ip,
    [string]$Key,
    [string]$User
)
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'common.ps1')

if (-not (Test-Path -LiteralPath (Join-Path $World 'level.dat'))) {
    throw "Não achei level.dat em '$World'. Aponte para a pasta do mundo (a que tem level.dat dentro)."
}
$World = (Resolve-Path -LiteralPath $World).Path
$conn = Resolve-Connection -Ip $Ip -Key $Key -User $User
Repair-KeyPermission $conn.Key
$ssh = Get-SshOptions $conn

# tar.exe do Windows (bsdtar); o nome do arquivo não pode ter espaços para o scp
$tarExe = Join-Path $env:SystemRoot 'System32\tar.exe'
if (-not (Test-Path -LiteralPath $tarExe)) { throw 'tar.exe não encontrado (precisa do Windows 10 1803 ou mais novo).' }
$archive = Join-Path $env:TEMP ("world-upload-{0}.tar.gz" -f (Get-Date -Format 'yyyyMMdd-HHmmss'))

Write-Host "==> Compactando $World" -ForegroundColor Cyan
& $tarExe -czf $archive -C (Split-Path -Parent $World) --exclude 'session.lock' (Split-Path -Leaf $World)
Assert-ExitCode 'Compactação'
$size = '{0:N1} MB' -f ((Get-Item -LiteralPath $archive).Length / 1MB)

try {
    Write-Host "==> Enviando ($size)" -ForegroundColor Cyan
    $remote = Send-ToVm $conn $archive

    Write-Host "==> Importando na VM" -ForegroundColor Cyan
    & ssh @ssh -t "$($conn.User)@$($conn.Ip)" "mc import-world $remote && rm -f $remote"
    Assert-ExitCode 'Importação'
} finally {
    Remove-Item -LiteralPath $archive -Force -ErrorAction SilentlyContinue
}
Write-Host 'Mundo no ar. Entre no jogo e confira.' -ForegroundColor Green
