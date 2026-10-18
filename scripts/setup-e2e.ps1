<#
.SYNOPSIS
  Fetch the WebDriver pieces the end-to-end suite needs on Windows.

.DESCRIPTION
  Two moving parts, one of them version-sensitive:

  - `tauri-driver`, which speaks WebDriver and launches the app.
  - `msedgedriver`, which drives the WebView2 webview inside it. It MUST match
    the installed WebView2 runtime; a mismatch fails with an unhelpful session
    error, so the version is read from the registry rather than guessed.

  Both land in `.webdriver/`, which is not tracked.
#>
[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$dest = Join-Path $root '.webdriver'
New-Item -ItemType Directory -Force $dest | Out-Null

Write-Host '== tauri-driver ==' -ForegroundColor Cyan
$git = (Get-Command git -ErrorAction Stop).Source
$bash = Join-Path (Split-Path -Parent (Split-Path -Parent $git)) 'bin/bash.exe'
if (-not (Test-Path $bash)) { throw 'Git for Windows Bash is required to verify the prebuilt driver.' }
$setup = (Join-Path $PSScriptRoot 'setup-tauri-driver.sh') -replace '\\', '/'
& $bash $setup
if ($LASTEXITCODE -ne 0) { throw 'could not provision the verified tauri-driver' }

Write-Host '== WebView2 runtime ==' -ForegroundColor Cyan
$key = 'HKLM:\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}'
$version = (Get-ItemProperty $key -ErrorAction SilentlyContinue).pv
if (-not $version) { throw 'the WebView2 runtime is not installed, so the app cannot render at all' }
Write-Host "  installed: $version"

$driver = Join-Path $dest 'msedgedriver.exe'
if (Test-Path $driver) {
    # Verify before the version check executes it.
    & (Join-Path $PSScriptRoot 'assert-microsoft-signature.ps1') -Path $driver
    $have = (& $driver --version) -replace '.*WebDriver\s+([\d.]+).*', '$1'
    if ($have -eq $version) {
        Write-Host "  driver already matches ($have)"
        Write-Host "`nREADY" -ForegroundColor Green
        exit 0
    }
    Write-Host "  driver is $have but the runtime is $version - replacing" -ForegroundColor Yellow
}

Write-Host '== edge driver ==' -ForegroundColor Cyan
$zip = Join-Path $dest 'edgedriver.zip'
Invoke-WebRequest -Uri "https://msedgedriver.microsoft.com/$version/edgedriver_win64.zip" `
    -OutFile $zip -UseBasicParsing
Expand-Archive $zip -DestinationPath $dest -Force
& (Join-Path $PSScriptRoot 'assert-microsoft-signature.ps1') -Path $driver
Remove-Item $zip -Force
Write-Host "  installed: $(& $driver --version)"

Write-Host "`nREADY - run the suite with: npm run e2e" -ForegroundColor Green
