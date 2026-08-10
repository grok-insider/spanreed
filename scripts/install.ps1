# Bootstrap spanreed on Windows, then run interactive setup.
#
# Public one-liner (canonical — served by grokinsider.net, not GitHub Releases):
#   irm https://grokinsider.net/install/spanreed.ps1 | iex
#
# Local / from a checkout:
#   .\scripts\install.ps1 -FromPath .\target\release\spanreed.exe
#   .\scripts\install.ps1 -FromPath .\target\release\spanreed.exe -Yes -Service
#
# Env:
#   SPANREED_REPO  GitHub owner/repo for binary assets (default: grok-insider/spanreed)
#   SPANREED_TAG   Release tag (default: latest)
#
# Binaries always come from GitHub Releases; this script is only the bootstrapper.

[CmdletBinding()]
param(
    [string]$FromPath = "",
    [switch]$Yes,
    [switch]$Service,
    [switch]$DryRun,
    [switch]$NoWire
)

$ErrorActionPreference = "Stop"

$Repo = if ($env:SPANREED_REPO) { $env:SPANREED_REPO } else { "grok-insider/spanreed" }
$Tag = if ($env:SPANREED_TAG) { $env:SPANREED_TAG } else { "latest" }

$InstallDir = Join-Path $env:LOCALAPPDATA "spanreed\bin"
$Dest = Join-Path $InstallDir "spanreed.exe"
New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null

function Stop-OpenUsageCapture {
    Get-CimInstance Win32_Process -Filter "Name='spanreed.exe'" -ErrorAction SilentlyContinue |
        Where-Object { $_.CommandLine -match 'capture' } |
        ForEach-Object {
            Write-Host "Stopping capture PID $($_.ProcessId) so the binary can be updated"
            Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue
        }
    Start-Sleep -Milliseconds 400
}

function Copy-OpenUsageBinary {
    param([string]$Source, [string]$Destination)
    Stop-OpenUsageCapture
    try {
        Copy-Item -LiteralPath $Source -Destination $Destination -Force
    } catch {
        # Windows may still hold a handle briefly; retry once.
        Start-Sleep -Milliseconds 500
        Stop-OpenUsageCapture
        Copy-Item -LiteralPath $Source -Destination $Destination -Force
    }
}

if ($FromPath) {
    if (-not (Test-Path -LiteralPath $FromPath)) {
        throw "binary not found: $FromPath"
    }
    Copy-OpenUsageBinary -Source $FromPath -Destination $Dest
    Write-Host "Installed $Dest from $FromPath"
} else {
    # Asset names include the semver: spanreed-0.0.1-x86_64-pc-windows-msvc.zip
    $ResolvedTag = $Tag
    if ($Tag -eq "latest") {
        $rel = Invoke-RestMethod -Uri "https://api.github.com/repos/$Repo/releases/latest"
        $ResolvedTag = $rel.tag_name
        if (-not $ResolvedTag) { throw "could not resolve latest release tag for $Repo" }
    }
    $Version = $ResolvedTag.TrimStart("v")
    $Asset = "spanreed-$Version-x86_64-pc-windows-msvc.zip"
    $Url = "https://github.com/$Repo/releases/download/$ResolvedTag/$Asset"
    $Tmp = Join-Path ([System.IO.Path]::GetTempPath()) ("spanreed-install-" + [guid]::NewGuid().ToString())
    New-Item -ItemType Directory -Force -Path $Tmp | Out-Null
    try {
        $Zip = Join-Path $Tmp $Asset
        Write-Host "Downloading $Url"
        Invoke-WebRequest -Uri $Url -OutFile $Zip
        Expand-Archive -LiteralPath $Zip -DestinationPath $Tmp -Force
        $Bin = Get-ChildItem -Path $Tmp -Recurse -Filter "spanreed.exe" | Select-Object -First 1
        if (-not $Bin) { throw "archive did not contain spanreed.exe" }
        Copy-OpenUsageBinary -Source $Bin.FullName -Destination $Dest
        Write-Host "Installed $Dest (release $ResolvedTag)"
    } finally {
        Remove-Item -Recurse -Force -ErrorAction SilentlyContinue $Tmp
    }
}

# Ensure user PATH includes install dir (setup also does this; belt-and-suspenders).
$userPath = [Environment]::GetEnvironmentVariable("Path", "User")
if (-not $userPath) { $userPath = "" }
$parts = $userPath -split ";" | Where-Object { $_ -ne "" }
if ($parts -notcontains $InstallDir) {
    $newPath = if ($userPath) { "$userPath;$InstallDir" } else { $InstallDir }
    [Environment]::SetEnvironmentVariable("Path", $newPath, "User")
    $env:Path = "$env:Path;$InstallDir"
    Write-Host "Added $InstallDir to user PATH (new terminals will pick it up)"
}

$setupArgs = @("setup", "--from-current-exe")
if ($Yes) { $setupArgs += "--yes" }
if ($Service) { $setupArgs += "--service" }
if ($DryRun) { $setupArgs += "--dry-run" }
if ($NoWire) { $setupArgs += "--no-wire" }

& $Dest @setupArgs
exit $LASTEXITCODE
