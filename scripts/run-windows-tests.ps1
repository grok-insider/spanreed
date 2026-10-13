[CmdletBinding()]
param(
    [ValidateSet('cross','desktop','probe','timeout-probe')][string]$Suite = 'cross',
    [string]$SourceRoot = (Get-Location).Path,
    [switch]$Child,
    [string]$Configuration,
    [int]$TimeoutSeconds = 2400
)
$ErrorActionPreference = 'Stop'
if ([Environment]::OSVersion.Platform -ne [PlatformID]::Win32NT) { throw 'Windows is required' }
if ($Child) {
    $config = Get-Content -LiteralPath $Configuration -Raw | ConvertFrom-Json
    $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
    $admin = ([Security.Principal.WindowsPrincipal]$identity).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
    if ($admin -or $identity.User.Value -ne $config.sid) { throw 'Tests require the dedicated non-administrator account' }
    $probe = Join-Path $env:TEMP ('owner-' + [Guid]::NewGuid().ToString('N'))
    New-Item -ItemType Directory -Path $probe | Out-Null
    $owner = (Get-Acl -LiteralPath $probe).GetOwner([Security.Principal.SecurityIdentifier]).Value
    if ($owner -ne $identity.User.Value) { throw 'The test token must create current-user-owned files' }
    if ([Environment]::GetEnvironmentVariable('SPANREED_CI_PARENT_SENTINEL')) { throw 'Parent-only environment leaked to the test user' }
    $sourceWritable = $false
    try { [IO.File]::WriteAllText((Join-Path $config.source 'must-remain-read-only'), 'unexpected'); $sourceWritable=$true } catch [UnauthorizedAccessException] {}
    if ($sourceWritable) { throw 'The test source snapshot is writable by the test user' }
    $report = [ordered]@{schema=1;identity=$identity.Name;sid=$identity.User.Value;administratorRoleActive=$admin;createdOwner=$owner;sourceCommit=$config.sourceCommit;suite=$config.suite;sourceReadOnly=$true;parentEnvironmentExcluded=$true;checks=@();result='running'}
    try {
        Set-Location -LiteralPath $config.source
        if ($config.suite -in @('probe','timeout-probe')) {
            $start = New-Object Diagnostics.ProcessStartInfo
            $start.FileName = Join-Path $env:SystemRoot 'System32\WindowsPowerShell\v1.0\powershell.exe'
            $escaped = (Join-Path $config.logs 'descendant-escaped').Replace("'", "''")
            $start.Arguments = '-NoProfile -NonInteractive -Command "Start-Sleep -Seconds 3; [IO.File]::WriteAllText(''' + $escaped + ''',''escaped''); Start-Sleep -Seconds 60"'
            $start.UseShellExecute = $false
            $start.CreateNoWindow = $true
            $descendant = [Diagnostics.Process]::Start($start)
            $report.descendantPid = $descendant.Id
            $report.descendantStarted = $descendant.StartTime.ToUniversalTime().Ticks
            $report.checks += @{name='limited-user-owner-and-environment';exitCode=0}
            if ($config.suite -eq 'timeout-probe') {
                $report | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $config.logs 'probe-before-timeout.json') -Encoding utf8
                Start-Sleep -Seconds 60
            }
        } else {
            $commands = if ($config.suite -eq 'cross') {
                @(@('check','--locked','--all-targets'), @('test','--locked','--all'), @('test','--locked','--workspace','--manifest-path','vendor/fabrials-libs/Cargo.toml'), @('check','--locked','--all-targets','--features','tray'), @('test','--locked','--all','--features','tray'))
            } else {
                @(@('test','--locked','--features','contracts'), @('test','--locked','--lib','secret::platform::tests::credential_roundtrip','--','--ignored','--exact'))
            }
            $index = 0
            foreach ($arguments in $commands) {
                $log = Join-Path $config.logs ('cargo-' + $index + '.log')
                $start = New-Object Diagnostics.ProcessStartInfo
                $start.FileName = $config.cargo
                $start.Arguments = $arguments -join ' '
                $start.UseShellExecute = $false
                $start.CreateNoWindow = $true
                $start.RedirectStandardOutput = $true
                $start.RedirectStandardError = $true
                $process = New-Object Diagnostics.Process
                $process.StartInfo = $start
                if (-not $process.Start()) { throw 'Could not start cargo' }
                $stdout = $process.StandardOutput.ReadToEndAsync()
                $stderr = $process.StandardError.ReadToEndAsync()
                $process.WaitForExit()
                $code = $process.ExitCode
                [IO.File]::WriteAllText($log, $stdout.GetAwaiter().GetResult() + $stderr.GetAwaiter().GetResult())
                $process.Dispose()
                $report.checks += @{command=('cargo ' + ($arguments -join ' '));exitCode=$code;log=[IO.Path]::GetFileName($log)}
                if ($code -ne 0) { throw ('Cargo check failed; see ' + [IO.Path]::GetFileName($log)) }
                $index++
            }
        }
        $report.result = 'passed'
    } catch { $report.result='failed';$report.failure=$_.Exception.Message;throw }
    finally { $report | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $config.logs 'report.json') -Encoding utf8 }
    exit 0
}
$source = (Resolve-Path -LiteralPath $SourceRoot).Path
$parentAdmin = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
if (-not $parentAdmin) { throw 'Fixture setup requires the elevated CI runner' }
$parentDirectory = if ($env:RUNNER_TEMP) { $env:RUNNER_TEMP } else { [IO.Path]::GetTempPath() }
$identifier = [Guid]::NewGuid().ToString('N')
$root = Join-Path $parentDirectory ('spanreed-limited-' + $identifier)
$evidence = Join-Path $parentDirectory ('spanreed-limited-evidence-' + $identifier)
$accountName = 'spanqa' + $identifier.Substring(0,10)
$passwordBytes = New-Object byte[] 32
$rng = [Security.Cryptography.RandomNumberGenerator]::Create()
$rng.GetBytes($passwordBytes)
$password = ConvertTo-SecureString ([Convert]::ToBase64String($passwordBytes) + 'aA1!') -AsPlainText -Force
[Array]::Clear($passwordBytes, 0, $passwordBytes.Length)
$rng.Dispose()
$account = $null
$process = $null
$toolchain = $null
$exitCode = 1
try {
    New-Item -ItemType Directory -Path $root,$evidence | Out-Null
    $account = New-LocalUser -Name $accountName -Password $password -AccountNeverExpires -PasswordNeverExpires -UserMayNotChangePassword -Description 'Disposable Spanreed CI test account'
    Add-LocalGroupMember -SID 'S-1-5-32-545' -Member $account
    $sid = $account.SID.Value
    $writeGrant = '*' + $sid + ':(OI)(CI)(F)'
    $readGrant = '*' + $sid + ':(OI)(CI)(RX)'
    foreach ($directory in @('source','target','cargo','profile','temp','logs')) { New-Item -ItemType Directory -Path (Join-Path $root $directory) | Out-Null }
    & icacls.exe $root /inheritance:r /grant '*S-1-5-18:(OI)(CI)(F)' '*S-1-5-32-544:(OI)(CI)(F)' $readGrant | Out-Null
    if ($LASTEXITCODE -ne 0) { throw 'Cannot authorize test workspace traversal' }
    foreach ($directory in @('target','cargo','profile','temp','logs')) {
        & icacls.exe (Join-Path $root $directory) /grant $writeGrant | Out-Null
        if ($LASTEXITCODE -ne 0) { throw 'Cannot authorize private test output directory' }
    }
    $commit = 'synthetic-probe'
    $cargo = ''
    if ($Suite -in @('cross','desktop')) {
        $commit = (& git -C $source rev-parse HEAD).Trim()
        if ($LASTEXITCODE -ne 0 -or $commit -notmatch '^[a-f0-9]{40}$') { throw 'Cannot identify checkout commit' }
        & git -C $source archive --format=tar ('--output=' + (Join-Path $root 'source.tar')) HEAD
        if ($LASTEXITCODE -ne 0) { throw 'Cannot snapshot committed source' }
        & tar.exe -xf (Join-Path $root 'source.tar') -C (Join-Path $root 'source')
        if ($LASTEXITCODE -ne 0) { throw 'Cannot extract committed source' }
        $cargo = (& rustup which cargo).Trim()
        if ($LASTEXITCODE -ne 0 -or -not (Test-Path -LiteralPath $cargo)) { throw 'Cannot locate installed Cargo' }
        $toolchain = Split-Path -Parent (Split-Path -Parent $cargo)
        & icacls.exe $toolchain /grant $readGrant | Out-Null
        if ($LASTEXITCODE -ne 0) { throw 'Cannot grant read access to the installed toolchain' }
        $cache = if ($env:CARGO_HOME) { $env:CARGO_HOME } else { Join-Path ([Environment]::GetFolderPath('UserProfile')) '.cargo' }
        foreach ($relative in @('registry\cache','registry\index')) {
            $cached = Join-Path $cache $relative
            if (Test-Path -LiteralPath $cached) {
                $destination = Join-Path (Join-Path $root 'cargo') $relative
                New-Item -ItemType Directory -Path $destination -Force | Out-Null
                Copy-Item -Path (Join-Path $cached '*') -Destination $destination -Recurse -Force
            }
        }
    }
    $script = Join-Path $root 'run-windows-tests.ps1'
    Copy-Item -LiteralPath $PSCommandPath -Destination $script
    $configurationPath = Join-Path $root 'configuration.json'
    @{sid=$sid;suite=$Suite;source=(Join-Path $root 'source');sourceCommit=$commit;logs=(Join-Path $root 'logs');cargo=$cargo} | ConvertTo-Json | Set-Content -LiteralPath $configurationPath -Encoding utf8
    $variables = @{}
    foreach ($name in @('SystemRoot','WINDIR','ComSpec','OS','PROCESSOR_ARCHITECTURE','NUMBER_OF_PROCESSORS','ProgramFiles','ProgramFiles(x86)','ProgramW6432','INCLUDE','LIB','LIBPATH','VCINSTALLDIR','VCToolsInstallDir','WindowsSdkDir','WindowsSDKVersion','UniversalCRTSdkDir','UCRTVersion','VSCMD_ARG_TGT_ARCH')) {
        $value = [Environment]::GetEnvironmentVariable($name)
        if ($value) { $variables[$name]=$value }
    }
    $variables.PATH = if ($cargo) { (Split-Path -Parent $cargo) + ';' + $env:PATH } else { $env:PATH }
    $variables.USERPROFILE = Join-Path $root 'profile'
    $variables.HOME = $variables.USERPROFILE
    $variables.TEMP = Join-Path $root 'temp'
    $variables.TMP = $variables.TEMP
    $variables.CARGO_HOME = Join-Path $root 'cargo'
    $variables.CARGO_TARGET_DIR = Join-Path $root 'target'
    $variables.CARGO_BUILD_JOBS = '2'
    $variables.SPANREED_OFFLINE = '1'
    $environmentBlock = (($variables.GetEnumerator() | Sort-Object Name | ForEach-Object { $_.Name + '=' + $_.Value }) -join [char]0) + [char]0 + [char]0
    Add-Type -Path (Join-Path $PSScriptRoot 'windows-test-process.cs')
    $powershell = Join-Path $env:SystemRoot 'System32\WindowsPowerShell\v1.0\powershell.exe'
    $arguments = '"' + $powershell + '" -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "' + $script + '" -Child -Configuration "' + $configurationPath + '"'
    $oldSentinel = [Environment]::GetEnvironmentVariable('SPANREED_CI_PARENT_SENTINEL')
    try {
        [Environment]::SetEnvironmentVariable('SPANREED_CI_PARENT_SENTINEL', 'synthetic-parent-only')
        $process = [WindowsTestProcess]::Start($accountName,$password,$powershell,$arguments,$environmentBlock,$root)
    } finally { [Environment]::SetEnvironmentVariable('SPANREED_CI_PARENT_SENTINEL', $oldSentinel) }
    $watch = [Diagnostics.Stopwatch]::StartNew()
    while (-not $process.Wait(1000)) {
        if ($watch.Elapsed.TotalSeconds -ge $TimeoutSeconds) { throw 'Windows tests exceeded their process lifetime limit' }
    }
    $exitCode = [int]$process.ExitCode
    if ($exitCode -eq 0) {
        $completed = Get-Content -LiteralPath (Join-Path $root 'logs\report.json') -Raw | ConvertFrom-Json
        $expected = if ($Suite -eq 'cross') { 5 } elseif ($Suite -eq 'desktop') { 2 } else { 1 }
        if ($completed.result -ne 'passed' -or $completed.sid -ne $sid -or $completed.createdOwner -ne $sid -or $completed.administratorRoleActive -ne $false -or $completed.sourceCommit -ne $commit -or @($completed.checks).Count -ne $expected -or @($completed.checks | Where-Object exitCode -ne 0).Count) { throw 'Windows test report did not prove the complete requested suite' }
    }
} finally {
    $cleanupErrors = New-Object 'System.Collections.Generic.List[string]'
    if ($process) { try { $process.Dispose() } catch { $cleanupErrors.Add($_.Exception.Message) } }
    if (Test-Path -LiteralPath (Join-Path $root 'logs')) { Copy-Item -Path (Join-Path $root 'logs\*') -Destination $evidence -Force -ErrorAction SilentlyContinue }
    if ($toolchain -and $account) {
        try { & icacls.exe $toolchain /remove:g ('*' + $account.SID.Value) | Out-Null; if ($LASTEXITCODE -ne 0) { throw 'Cannot remove temporary toolchain access' } }
        catch { $cleanupErrors.Add($_.Exception.Message) }
    }
    if ($account) {
        try { Remove-LocalUser -SID $account.SID } catch { $cleanupErrors.Add($_.Exception.Message) }
        try {
            $profileWait = [Diagnostics.Stopwatch]::StartNew()
            $profile = Get-CimInstance Win32_UserProfile -Filter ("SID='" + $account.SID.Value + "'")
            while ($profile -and $profile.Loaded -and $profileWait.ElapsedMilliseconds -lt 5000) {
                Start-Sleep -Milliseconds 100
                $profile = Get-CimInstance Win32_UserProfile -Filter ("SID='" + $account.SID.Value + "'")
            }
            if ($profile) { if ($profile.Loaded) { throw 'Temporary account profile remained loaded after Job termination' }; $profile | Remove-CimInstance }
        } catch { $cleanupErrors.Add($_.Exception.Message) }
    }
    $password.Dispose()
    try { if (Test-Path -LiteralPath $root) { Remove-Item -LiteralPath $root -Recurse -Force } } catch { $cleanupErrors.Add($_.Exception.Message) }
    if ($Suite -in @('probe','timeout-probe')) {
        Start-Sleep -Milliseconds 3500
        if (Test-Path -LiteralPath (Join-Path $evidence 'descendant-escaped')) { $cleanupErrors.Add('A descendant escaped its Job') }
        foreach ($reportPath in @((Join-Path $evidence 'report.json'),(Join-Path $evidence 'probe-before-timeout.json'))) {
            if (Test-Path -LiteralPath $reportPath) {
                $probeReport = Get-Content -LiteralPath $reportPath -Raw | ConvertFrom-Json
                $descendant = Get-Process -Id $probeReport.descendantPid -ErrorAction SilentlyContinue
                if ($descendant -and $descendant.StartTime.ToUniversalTime().Ticks -eq $probeReport.descendantStarted) { $cleanupErrors.Add('The original descendant survived Job cleanup') }
            }
        }
    }
    @{accountRemoved=($account -and -not (Get-LocalUser -SID $account.SID -ErrorAction SilentlyContinue));privateRootRemoved=(-not (Test-Path -LiteralPath $root));errors=@($cleanupErrors)} | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $evidence 'cleanup.json') -Encoding utf8
    Write-Output ('Windows limited-user evidence: ' + $evidence)
    Get-ChildItem -LiteralPath $evidence -Filter '*.log' | ForEach-Object { Write-Output $_.Name; Get-Content -LiteralPath $_.FullName -Tail 100 }
    if (Test-Path -LiteralPath (Join-Path $evidence 'report.json')) { Get-Content -LiteralPath (Join-Path $evidence 'report.json') -Raw | Write-Output }
    if ($cleanupErrors.Count) { throw ('Windows test cleanup failed: ' + ($cleanupErrors -join '; ')) }
}
if ($exitCode -ne 0) { throw ('Windows test process failed with exit code ' + $exitCode) }
