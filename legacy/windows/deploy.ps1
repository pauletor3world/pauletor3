<#
.SYNOPSIS
  Envia server\ e config.env para a VM e roda o setup.sh. Pode rodar de novo para aplicar mudanças.
.EXAMPLE
  .\windows\deploy.ps1 -Ip 34.95.10.20                 # usuário e chave salvos pelo new-key.ps1
.EXAMPLE
  .\windows\deploy.ps1 -Ip 34.95.10.20 -User machen -Key C:\Users\voce\.ssh\gcp-mc
.EXAMPLE
  .\windows\deploy.ps1                                 # tudo salvo em windows\local.json
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
$target = "$($conn.User)@$($conn.Ip)"
$ssh = Get-SshOptions $conn

Write-Host "==> Ajustando permissões da chave" -ForegroundColor Cyan
Repair-KeyPermission $conn.Key

$config = Join-Path $Root 'config.env'
if (-not (Test-Path -LiteralPath $config)) {
    Copy-Item -LiteralPath (Join-Path $Root 'config.env.example') -Destination $config
    Write-Host "    config.env criado a partir do exemplo" -ForegroundColor Yellow
}

Write-Host "==> Testando conexão com $target" -ForegroundColor Cyan
& ssh @ssh $target 'rm -rf ~/mc-deploy'
if ($LASTEXITCODE -ne 0) {
    Write-Host ''
    Write-Host 'Não conectou. Confira:' -ForegroundColor Yellow
    Write-Host '  - O IP é o "IP externo" da VM e ela está ligada.'
    Write-Host "  - O usuário '$($conn.User)' é o mesmo nome que aparece no fim da chave pública colada no GCP."
    Write-Host '  - A chave pública foi salva na VM (Editar > Chaves SSH) e você clicou em Salvar.'
    Write-Host '  - O OS Login está desligado (com ele ligado, o GCP ignora as chaves). Veja o README.'
    throw "Conexão SSH falhou (código $LASTEXITCODE)."
}
Save-LocalConfig -Ip $conn.Ip -Key $conn.Key -User $conn.User

Write-Host "==> Enviando arquivos" -ForegroundColor Cyan
Push-Location $Root
try {
    & scp @ssh -r 'server' "${target}:mc-deploy"
    Assert-ExitCode 'Envio da pasta server'
    & scp @ssh 'config.env' "${target}:mc-deploy/config.env"
    Assert-ExitCode 'Envio do config.env'
} finally {
    Pop-Location
}

Write-Host "==> Rodando setup.sh na VM (saída ao vivo)" -ForegroundColor Cyan
# Remove CRLF caso algum arquivo tenha sido editado no Windows
& ssh @ssh -t $target 'find ~/mc-deploy -type f -exec sed -i ''s/\r$//'' {} + && sudo bash ~/mc-deploy/setup.sh'
Assert-ExitCode 'setup.sh'

Write-Host ""
Write-Host "Deploy concluído. Para entrar na VM: .\windows\ssh.ps1" -ForegroundColor Green
