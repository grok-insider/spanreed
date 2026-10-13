# Bootstrap spanreed on Windows, then run interactive setup.
#
# Public one-liner (canonical — served by fabrials.com, not GitHub Releases):
#   irm https://fabrials.com/install/spanreed.ps1 | iex
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

function Stop-SpanreedCapture {
    Get-CimInstance Win32_Process -Filter "Name='spanreed.exe'" -ErrorAction SilentlyContinue |
        Where-Object { $_.CommandLine -match 'capture' } |
        ForEach-Object {
            Write-Host "Stopping capture PID $($_.ProcessId) so the binary can be updated"
            Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue
        }
    Start-Sleep -Milliseconds 400
}

function Assert-InstallDestination {
    param([string]$Destination)
    $DestinationItem = Get-Item -LiteralPath $Destination -Force -ErrorAction SilentlyContinue
    if ($DestinationItem -and ($DestinationItem.PSIsContainer -or ($DestinationItem.Attributes -band [System.IO.FileAttributes]::ReparsePoint))) {
        throw 'Install destination must be a regular file or absent'
    }
}

function Copy-SpanreedBinary {
    param([string]$Source, [string]$Destination)
    $SourceItem = Get-Item -LiteralPath $Source -Force
    if ($SourceItem.PSIsContainer -or ($SourceItem.Attributes -band [System.IO.FileAttributes]::ReparsePoint)) {
        throw 'Install source must be a regular file'
    }
    Assert-InstallDestination -Destination $Destination
    $Staged = Join-Path (Split-Path -Parent $Destination) (".spanreed-install-" + [guid]::NewGuid().ToString('n'))
    $Backup = "$Staged.previous"
    try {
        [System.IO.File]::Copy($Source, $Staged, $false)
        if ((Get-FileHash -LiteralPath $Source -Algorithm SHA256).Hash -cne (Get-FileHash -LiteralPath $Staged -Algorithm SHA256).Hash) {
            throw 'Staged binary differs from the verified source'
        }
        Assert-InstallDestination -Destination $Destination
        Stop-SpanreedCapture
        if (Test-Path -LiteralPath $Destination) {
            [System.IO.File]::Replace($Staged, $Destination, $Backup)
        } else {
            [System.IO.File]::Move($Staged, $Destination)
        }
        Remove-Item -LiteralPath $Backup -Force -ErrorAction SilentlyContinue
    } catch {
        if (-not (Test-Path -LiteralPath $Destination) -and (Test-Path -LiteralPath $Backup)) {
            [System.IO.File]::Move($Backup, $Destination)
        }
        throw
    } finally {
        Remove-Item -LiteralPath $Staged -Force -ErrorAction SilentlyContinue
    }
}

if ($FromPath) {
    if (-not (Test-Path -LiteralPath $FromPath)) {
        throw "binary not found: $FromPath"
    }
    Copy-SpanreedBinary -Source $FromPath -Destination $Dest
    Write-Host "Installed $Dest from $FromPath"
} else {
    # Asset names include the semver: spanreed-0.0.1-x86_64-pc-windows-msvc.zip
    $ResolvedTag = $Tag
    if ($Tag -eq "latest") {
        $rel = Invoke-RestMethod -Uri "https://api.github.com/repos/$Repo/releases/latest"
        $ResolvedTag = $rel.tag_name
        if ($rel.draft -or $rel.prerelease -or $ResolvedTag -cnotmatch '^v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$') {
            throw 'Latest release did not provide a stable version tag; retry later'
        }
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
        $SumPath = "$Zip.sha256"
        Invoke-WebRequest -Uri "$Url.sha256" -OutFile $SumPath
        $Fields = (Get-Content -LiteralPath $SumPath -Raw).Trim() -split '\s+'
        if ($Fields.Count -ne 2 -or $Fields[0] -notmatch '^[a-fA-F0-9]{64}$' -or $Fields[1] -cne $Asset) {
            throw "checksum file does not describe $Asset"
        }
        $Actual = (Get-FileHash -Algorithm SHA256 -LiteralPath $Zip).Hash
        if ($Actual -ine $Fields[0]) { throw "checksum mismatch for $Asset" }
        $Unpacked = Join-Path $Tmp 'unpacked'
        Expand-Archive -LiteralPath $Zip -DestinationPath $Unpacked
        $Bin = Join-Path $Unpacked 'spanreed.exe'
        if (-not (Test-Path -LiteralPath $Bin -PathType Leaf)) { throw 'Archive did not contain spanreed.exe at its root' }
        Copy-SpanreedBinary -Source $Bin -Destination $Dest
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
