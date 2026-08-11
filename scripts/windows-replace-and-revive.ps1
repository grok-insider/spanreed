# Replace spanreed.exe while keeping capture alive again afterwards.
#
# Never leave the proxy dead after a binary swap: stop -> copy -> ensure -> tray.
#
# Usage (from a checkout):
#   .\scripts\windows-replace-and-revive.ps1 -FromPath .\target\release\spanreed.exe
#   .\scripts\windows-replace-and-revive.ps1 -FromPath .\target\release\spanreed.exe -NoTray
#
# Install dir (default): %LOCALAPPDATA%\spanreed\bin\spanreed.exe

[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string]$FromPath,
    [string]$Dest = "",
    [switch]$NoTray
)

$ErrorActionPreference = "Stop"

if (-not $Dest) {
    $Dest = Join-Path $env:LOCALAPPDATA "spanreed\bin\spanreed.exe"
}
if (-not (Test-Path -LiteralPath $FromPath)) {
    throw "source binary not found: $FromPath"
}

$destDir = Split-Path -Parent $Dest
New-Item -ItemType Directory -Force -Path $destDir | Out-Null

Write-Host "Stopping spanreed processes (capture + tray) so the PE can be replaced..."
Get-CimInstance Win32_Process -Filter "Name='spanreed.exe'" -ErrorAction SilentlyContinue |
    ForEach-Object {
        Write-Host ("  stop pid={0} {1}" -f $_.ProcessId, $_.CommandLine)
        Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue
    }
Start-Sleep -Milliseconds 500

Copy-Item -LiteralPath $FromPath -Destination $Dest -Force
Write-Host ("Installed {0} ({1} bytes)" -f $Dest, (Get-Item -LiteralPath $Dest).Length)

Write-Host "Reviving capture via: spanreed capture ensure"
& $Dest capture ensure
if ($LASTEXITCODE -ne 0) {
    Write-Warning ("capture ensure exited {0} - try: {1} capture ensure" -f $LASTEXITCODE, $Dest)
}

& $Dest capture status

if (-not $NoTray) {
    Write-Host "Starting tray..."
    Start-Process -FilePath $Dest -ArgumentList "tray" -WindowStyle Hidden
    Start-Sleep -Milliseconds 400
}

Write-Host "Done. Processes:"
Get-CimInstance Win32_Process -Filter "Name='spanreed.exe'" -ErrorAction SilentlyContinue |
    ForEach-Object {
        Write-Host ("  pid={0} {1}" -f $_.ProcessId, $_.CommandLine)
    }
