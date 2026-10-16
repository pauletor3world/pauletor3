# Funções compartilhadas pelos scripts do Windows (carregado com ". .\common.ps1").

$LocalJson = Join-Path $PSScriptRoot 'local.json'

function Read-LocalConfig {
    if (Test-Path -LiteralPath $LocalJson) {
        return Get-Content -LiteralPath $LocalJson -Raw | ConvertFrom-Json
    }
    return $null
}

# Grava Ip/Key/User em windows\local.json, mantendo o que já estava salvo
function Save-LocalConfig {
    param([string]$Ip, [string]$Key, [string]$User)
    $saved = Read-LocalConfig
    $data = [ordered]@{ Ip = ''; Key = ''; User = '' }
    if ($saved) {
        foreach ($k in @('Ip', 'Key', 'User')) { if ($saved.$k) { $data[$k] = $saved.$k } }
    }
    if ($Ip)   { $data.Ip = $Ip }
    if ($Key)  { $data.Key = $Key }
    if ($User) { $data.User = $User }
    [pscustomobject]$data | ConvertTo-Json | Set-Content -LiteralPath $LocalJson -Encoding UTF8
}

# Junta os parâmetros passados com os salvos em windows\local.json
function Resolve-Connection {
    param([string]$Ip, [string]$Key, [string]$User)
    $saved = Read-LocalConfig
    if (-not $Ip   -and $saved) { $Ip   = $saved.Ip }
    if (-not $Key  -and $saved) { $Key  = $saved.Key }
    if (-not $User -and $saved) { $User = $saved.User }

    if (-not $User) {
        throw 'Informe -User (no Google Cloud é o nome que está no fim da chave pública; o new-key.ps1 salva ele).'
    }
    if (-not $Ip) {
        throw 'Informe -Ip (o IP externo da VM). Depois do primeiro deploy ele fica salvo em windows\local.json.'
    }
    if (-not $Key) {
        throw 'Informe -Key (caminho da chave privada) ou rode .\windows\new-key.ps1 para criar uma.'
    }
    if (-not (Test-Path -LiteralPath $Key -PathType Leaf)) {
        throw "Chave SSH não encontrada: $Key"
    }
    $Key = (Resolve-Path -LiteralPath $Key).Path
    return [pscustomobject]@{ Ip = $Ip; Key = $Key; User = $User }
}

# O OpenSSH do Windows recusa chave privada que outros usuários podem ler
function Repair-KeyPermission {
    param([string]$Key)
    $me = [System.Security.Principal.WindowsIdentity]::GetCurrent().Name
    icacls $Key /reset | Out-Null
    icacls $Key /inheritance:r /grant:r "${me}:(R)" | Out-Null
    if ($LASTEXITCODE -ne 0) { throw "Falha ao ajustar as permissões da chave com icacls: $Key" }
}

function Get-SshOptions {
    param($Conn)
    return @('-i', $Conn.Key, '-o', 'StrictHostKeyChecking=accept-new', '-o', 'ServerAliveInterval=30')
}

function Assert-ExitCode {
    param([string]$What)
    if ($LASTEXITCODE -ne 0) { throw "$What falhou (código $LASTEXITCODE)." }
}

# Envia um arquivo local para a home do usuário na VM e devolve o caminho remoto
function Send-ToVm {
    param($Conn, [string]$File)
    $ssh = Get-SshOptions $Conn
    $name = Split-Path -Leaf $File
    $dir = Split-Path -Parent (Resolve-Path -LiteralPath $File).Path
    Push-Location $dir
    try {
        & scp @ssh $name "$($Conn.User)@$($Conn.Ip):$name"
        Assert-ExitCode 'Envio do arquivo'
    } finally {
        Pop-Location
    }
    return "/home/$($Conn.User)/$name"
}
