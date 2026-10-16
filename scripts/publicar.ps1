# Publica o pAuletOr 3 no GitHub (uma vez) e lança uma versão.
#
#   powershell -ExecutionPolicy Bypass -File scripts\publicar.ps1            # primeira vez
#   powershell -ExecutionPolicy Bypass -File scripts\publicar.ps1 -Versao 0.2.0
#
# Requer o GitHub CLI logado (gh auth status) na conta pauletor3world.
# Cria o repositório público pauletor3world/pauletor3, cadastra os segredos de assinatura
# das atualizações (lidos de D:\DevTools\pauletor3-signing, nunca do repositório) e envia a tag.
param(
  [string]$Versao = "",
  [string]$Repo = "pauletor3world/pauletor3",
  [string]$PastaChave = "D:\DevTools\pauletor3-signing"
)
$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

gh auth status | Out-Null

if (-not (Test-Path .git)) {
  git init -b main | Out-Null
}

# Nada de segredo pode estar no repositório.
$suspeitos = git ls-files --others --cached --exclude-standard | Select-String -Pattern "\.key$|secrets\.bin|rclone\.conf|config\.env$"
if ($suspeitos) { throw "Arquivos sensíveis no repositório: $suspeitos" }

if ($Versao) {
  $conf = Get-Content app\src-tauri\tauri.conf.json -Raw | ConvertFrom-Json
  $conf.version = $Versao
  $conf | ConvertTo-Json -Depth 20 | Set-Content -Encoding utf8 app\src-tauri\tauri.conf.json
  (Get-Content app\package.json -Raw) -replace '"version": "[^"]+"', "`"version`": `"$Versao`"" | Set-Content -Encoding utf8 app\package.json
  (Get-Content Cargo.toml -Raw) -replace '(?m)^version = "[^"]+"', "version = `"$Versao`"" | Set-Content -Encoding utf8 Cargo.toml
} else {
  $Versao = (Get-Content app\src-tauri\tauri.conf.json -Raw | ConvertFrom-Json).version
}

git add -A
git commit -m "pAuletOr 3 v$Versao" | Out-Null

# Comandos externos não disparam "catch" no PowerShell 5.1: usa o código de saída.
gh repo view $Repo 2>$null | Out-Null
if ($LASTEXITCODE -ne 0) {
  gh repo create $Repo --public --source . --remote origin --description "pAuletOr 3 — o nosso mundo de Minecraft, hospedado por turnos"
}
if (-not (git remote | Select-String origin)) { git remote add origin "https://github.com/$Repo.git" }

Get-Content (Join-Path $PastaChave "pauletor3-updater.key") -Raw | gh secret set TAURI_SIGNING_PRIVATE_KEY --repo $Repo
Get-Content (Join-Path $PastaChave "senha-da-chave.txt") -Raw | gh secret set TAURI_SIGNING_PRIVATE_KEY_PASSWORD --repo $Repo

git push -u origin main
if ($LASTEXITCODE -ne 0) { throw "git push falhou" }

# GitHub Pages (pasta docs/): página inicial e política de privacidade exigidas pelo Google OAuth.
gh api -X POST "repos/$Repo/pages" -f "source[branch]=main" -f "source[path]=/docs" 2>$null | Out-Null
if ($LASTEXITCODE -ne 0) { Write-Host "GitHub Pages já estava ligado (ou será ligado manualmente)." }
git tag "v$Versao"
git push origin "v$Versao"
Write-Host ""
Write-Host "Versão v$Versao enviada. Acompanhe a build em https://github.com/$Repo/actions"
Write-Host "O instalador aparece em https://github.com/$Repo/releases/latest"
