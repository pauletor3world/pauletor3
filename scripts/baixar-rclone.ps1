# Baixa o rclone fixado e confere o SHA-256 antes de colocá-lo como sidecar do app.
# Uso: powershell -ExecutionPolicy Bypass -File scripts\baixar-rclone.ps1
$ErrorActionPreference = 'Stop'
$version = 'v1.75.2'
$sha256 = '8a9db2bad1dc79a65b3348fd857069598e2aab7cf4d118b77b1df23055aff818'   # rclone-v1.75.2-windows-amd64.zip (SHA256SUMS oficial)
$root = Split-Path -Parent $PSScriptRoot
$dest = Join-Path $root 'app\src-tauri\binaries\rclone-x86_64-pc-windows-msvc.exe'
if (Test-Path $dest) { Write-Host "rclone já presente: $dest"; exit 0 }
$tmp = Join-Path ([IO.Path]::GetTempPath()) ("rclone-" + [guid]::NewGuid())
New-Item -ItemType Directory $tmp | Out-Null
$zip = Join-Path $tmp 'rclone.zip'
Invoke-WebRequest "https://downloads.rclone.org/$version/rclone-$version-windows-amd64.zip" -OutFile $zip
$got = (Get-FileHash $zip -Algorithm SHA256).Hash.ToLower()
if ($got -ne $sha256) { throw "SHA-256 do rclone não confere ($got)" }
Expand-Archive $zip -DestinationPath $tmp
Copy-Item (Join-Path $tmp "rclone-$version-windows-amd64\rclone.exe") $dest
Remove-Item -Recurse -Force $tmp
Write-Host "rclone $version pronto em $dest"
