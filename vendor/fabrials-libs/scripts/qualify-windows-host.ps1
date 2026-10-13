[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$Source,
    [Parameter(Mandatory = $true)][string]$EvidenceDirectory,
    [Parameter(Mandatory = $true)][string]$ForeignOwnerFixture,
    [string]$TestExecutable
)
$ErrorActionPreference = 'Stop'
if ([Environment]::OSVersion.Platform -ne [PlatformID]::Win32NT) { throw 'Windows is required' }
if (Test-Path -LiteralPath $EvidenceDirectory) { throw 'Evidence directory must be new' }
if (-not (Test-Path -LiteralPath $ForeignOwnerFixture -PathType Container)) { throw 'Foreign owner fixture is required' }
$env:FABRIALS_WINDOWS_FOREIGN_OWNER_FIXTURE = $ForeignOwnerFixture
New-Item -ItemType Directory -Path $EvidenceDirectory | Out-Null
$sourceRoot = (Resolve-Path -LiteralPath $Source).Path
$report = [ordered]@{
    schema = 1
    startedAt = [DateTime]::UtcNow.ToString('o')
    platform = [Environment]::OSVersion.VersionString
    identity = [Security.Principal.WindowsIdentity]::GetCurrent().Name
    administratorRoleActive = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
    foreignOwnerFixture = @{path=$ForeignOwnerFixture;owner=(Get-Acl -LiteralPath $ForeignOwnerFixture).Owner}
    rustc = ((& rustc --version) -join ' ')
    cargo = ((& cargo --version) -join ' ')
    source = $sourceRoot
    files = @()
    checks = @()
    testExecutable = if ($TestExecutable) { @{path=$TestExecutable;sha256=(Get-FileHash -LiteralPath $TestExecutable -Algorithm SHA256).Hash.ToLowerInvariant()} } else { $null }
    result = 'running'
    notQualified = @('Physical Windows hardware', 'Power loss', 'Interactive current-user service lifecycle', 'Production Grok inference', 'Shared Grok leader termination', 'Windows self-update')
}
foreach ($relative in @('Cargo.lock', 'crates/fabrials-agent-host/Cargo.toml', 'crates/fabrials-agent-host/src/acp.rs', 'crates/fabrials-agent-host/src/instance.rs', 'crates/fabrials-agent-host/src/control/windows.rs', 'crates/fabrials-agent-host/src/live_media.rs', 'crates/fabrials-agent-host/src/server.rs', 'crates/fabrials-agent-host/src/journal.rs', 'scripts/qualify-windows-host.ps1', 'crates/fabrials-agent-host/src/win_acl.rs', 'crates/fabrials-agent-host/src/win_job.rs', 'crates/fabrials-agent-host/src/win_fs.rs', 'crates/fabrials-agent-host/src/remote_projects.rs')) {
    $report.files += @{path=$relative;sha256=(Get-FileHash -LiteralPath (Join-Path $sourceRoot $relative) -Algorithm SHA256).Hash.ToLowerInvariant()}
}
Push-Location $sourceRoot
try {
    foreach ($filter in @('win_acl::tests', 'win_job::tests', 'win_fs::tests', 'remote_projects::windows_tests', 'control::windows::windows_tests', 'instance::tests')) {
        $log = Join-Path $EvidenceDirectory (($filter -replace '::','-') + '.log')
        $executable = if ($TestExecutable) { $TestExecutable } else { (Get-Command cargo.exe).Source }
        $arguments = if ($TestExecutable) { @($filter,'--test-threads=1','--include-ignored') } else { @('test','--locked','-p','fabrials-agent-host','--lib',$filter,'--','--test-threads=1','--include-ignored') }
        $start = New-Object System.Diagnostics.ProcessStartInfo
        $start.FileName = $executable
        $start.Arguments = $arguments -join ' '
        $start.UseShellExecute = $false
        $start.CreateNoWindow = $true
        $start.RedirectStandardOutput = $true
        $start.RedirectStandardError = $true
        $process = New-Object System.Diagnostics.Process
        $process.StartInfo = $start
        if (-not $process.Start()) { throw 'Could not start Windows qualification' }
        $stdout = $process.StandardOutput.ReadToEndAsync()
        $stderr = $process.StandardError.ReadToEndAsync()
        if (-not $process.WaitForExit(1200000)) {
            & taskkill.exe /PID $process.Id /T /F | Out-Null
            throw ('Windows qualification timed out: ' + $filter)
        }
        $code = $process.ExitCode
        [IO.File]::WriteAllText($log, $stdout.GetAwaiter().GetResult())
        [IO.File]::WriteAllText(($log + '.stderr'), $stderr.GetAwaiter().GetResult())
        $process.Dispose()
        $body = Get-Content -LiteralPath $log -Raw
        $matched = [regex]::Match($body, 'test result: ok\. (\d+) passed; 0 failed; (\d+) ignored;')
        $passed = if ($matched.Success) { [int]$matched.Groups[1].Value } else { 0 }
        $ignored = if ($matched.Success) { [int]$matched.Groups[2].Value } else { 0 }
        $report.checks += @{filter=$filter;exitCode=$code;passed=$passed;ignored=$ignored;log=(Split-Path -Leaf $log)}
        if ($code -ne 0 -or $passed -lt 1 -or $ignored -ne 0) { throw ('Windows qualification failed: ' + $filter) }
    }
    $report.result = 'passed'
} catch {
    $report.result = 'failed'
    $report.failure = $_.Exception.Message
    throw
} finally {
    Pop-Location
    $report.finishedAt = [DateTime]::UtcNow.ToString('o')
    $report | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath (Join-Path $EvidenceDirectory 'report.json') -Encoding utf8
}
