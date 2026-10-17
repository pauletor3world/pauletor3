# Publica (ou atualiza) a coordenação do Machen Craft na SUA conta gratuita da Cloudflare.
# Feito uma vez pelo dono. Os amigos nunca precisam disso.
#
#   powershell -ExecutionPolicy Bypass -File scripts\publicar-coordenacao.ps1
#
# O que faz:
#   1. instala as dependências do Worker (pasta coordinator/);
#   2. abre o navegador para você entrar na Cloudflare (só na primeira vez);
#   3. publica o Worker + Durable Objects (plano grátis, sem cartão);
#   4. cria a chave de administração (que permite criar mundos) e guarda como segredo do Worker;
#   5. confere se está no ar e mostra o ENDEREÇO e a CHAVE para colar no app.
#
# A chave fica também em scripts\.cache\coordenacao.txt (fora do Git). Não compartilhe.

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$coord = Join-Path $root "coordinator"
$cache = Join-Path $PSScriptRoot ".cache"
$saved = Join-Path $cache "coordenacao.txt"

function Step($t) { Write-Host ""; Write-Host "==> $t" -ForegroundColor Green }
function Fail($t) { Write-Host ""; Write-Host "ERRO: $t" -ForegroundColor Red; exit 1 }

if (-not (Get-Command node -ErrorAction SilentlyContinue)) {
  Fail "Node.js não encontrado. Instale o Node 22 LTS (nodejs.org) e rode de novo."
}

Push-Location $coord
try {
  if (-not (Test-Path (Join-Path $coord "node_modules\.bin\wrangler.cmd"))) {
    Step "Instalando as dependências do Worker"
    npm ci --no-audit --no-fund
    if ($LASTEXITCODE -ne 0) { Fail "npm ci falhou." }
  }

  Step "Conferindo o login na Cloudflare"
  $who = (npx wrangler whoami 2>&1 | Out-String)
  if ($who -match "not authenticated|You are not logged in") {
    Write-Host "O navegador vai abrir: entre na sua conta Cloudflare (ou crie uma, é grátis) e clique em Allow."
    npx wrangler login
    if ($LASTEXITCODE -ne 0) { Fail "Login na Cloudflare não concluído." }
  }

  Step "Publicando a coordenação"
  Write-Host "(Na primeira vez a Cloudflare pode pedir para escolher um subdomínio workers.dev: escolha qualquer nome.)"
  $out = (npx wrangler deploy 2>&1 | Tee-Object -Variable lines | Out-String)
  $lines | ForEach-Object { Write-Host $_ }
  if ($LASTEXITCODE -ne 0) { Fail "A publicação falhou (veja a mensagem acima)." }
  $m = [regex]::Match($out, "https://[a-z0-9.-]+\.workers\.dev")
  if (-not $m.Success) { Fail "Não achei o endereço publicado na saída do wrangler." }
  $url = $m.Value

  Step "Chave de administração"
  New-Item -ItemType Directory -Force -Path $cache | Out-Null
  $token = $null
  if (Test-Path $saved) {
    $token = (Get-Content $saved | Where-Object { $_ -like "CHAVE=*" } | Select-Object -First 1) -replace "^CHAVE=", ""
  }
  if (-not $token) {
    $bytes = New-Object byte[] 32
    [System.Security.Cryptography.RandomNumberGenerator]::Create().GetBytes($bytes)
    $token = -join ($bytes | ForEach-Object { $_.ToString("x2") })
  }
  $token | npx wrangler secret put ADMIN_TOKEN
  if ($LASTEXITCODE -ne 0) { Fail "Não consegui gravar a chave como segredo do Worker." }
  Set-Content -Path $saved -Encoding ascii -Value @("ENDERECO=$url", "CHAVE=$token")

  Step "Conferindo se está no ar"
  $ok = $false
  for ($i = 0; $i -lt 10 -and -not $ok; $i++) {
    try {
      $h = Invoke-RestMethod -Uri "$url/health" -TimeoutSec 10
      if ($h.ok) { $ok = $true }
    } catch { Start-Sleep -Seconds 3 }
  }
  if (-not $ok) { Fail "O Worker foi publicado, mas $url/health não respondeu ainda. Espere 1 minuto e abra o endereço no navegador." }

  Write-Host ""
  Write-Host "Pronto! No app, em 'Vou criar o mundo', cole:" -ForegroundColor Green
  Write-Host ""
  Write-Host "  Endereço da coordenação:  $url"
  Write-Host "  Chave de administração:   $token"
  Write-Host ""
  Write-Host "(Guardado em $saved — não compartilhe a chave.)"
} finally {
  Pop-Location
}
