<#
.SYNOPSIS
  The full gate on Windows: a shim that runs verify.sh through Git Bash.

.DESCRIPTION
  One definition of green. Keeping a parallel PowerShell gate in step by hand
  is how sibling projects shipped a release that passed on one platform only.
  Git's own bash is located explicitly: with WSL installed, `bash` on PATH is
  the WSL launcher, which has nothing to do with this repository.
#>
[CmdletBinding()]
param(
    # Also run the packaged Tauri build. Release/handoff only; needs AGENT_RELEASE=1.
    [switch]$Package
)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot

function Find-GitBash {
    $git = (Get-Command git -ErrorAction SilentlyContinue).Source
    if ($git) {
        $candidate = Join-Path (Split-Path -Parent (Split-Path -Parent $git)) 'bin\bash.exe'
        if (Test-Path $candidate) { return $candidate }
    }
    foreach ($found in @(Get-Command bash -All -ErrorAction SilentlyContinue)) {
        if ($found.Source -notmatch '\\WindowsApps\\') { return $found.Source }
    }
    return $null
}

$bash = Find-GitBash
if (-not $bash) {
    throw 'Git Bash was not found. It ships with Git for Windows; install Git or put its bin\bash.exe on PATH.'
}

$gate = (Join-Path $PSScriptRoot 'verify.sh') -replace '\\', '/'
$gateArgs = @($gate)
if ($Package) { $gateArgs += '--package' }

Push-Location $root
try {
    & $bash @gateArgs
    if ($LASTEXITCODE -ne 0) { throw "the gate failed (exit $LASTEXITCODE)" }
}
finally {
    Pop-Location
}
