[CmdletBinding()]
param(
  [Parameter(Mandatory=$true)][string]$Source,
  [Parameter(Mandatory=$true)][string]$TestExecutable,
  [Parameter(Mandatory=$true)][string]$ProbeFixture,
  [Parameter(Mandatory=$true)][string]$EvidenceDirectory
)
$ErrorActionPreference='Stop'
if(Test-Path -LiteralPath $EvidenceDirectory){throw 'Evidence directory must be new'}
New-Item -ItemType Directory -Path $EvidenceDirectory | Out-Null
$env:SPANREED_UPDATE_PROBE_FIXTURE=$ProbeFixture
$report=[ordered]@{
 schema=1
 startedAt=[DateTime]::UtcNow.ToString('o')
 identity=[Security.Principal.WindowsIdentity]::GetCurrent().Name
 administratorRoleActive=([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
 platform=[Environment]::OSVersion.VersionString
 files=@()
 checks=@()
 result='running'
 notQualified=@('Real installed updater replacement and rollback','Power failure','Physical Windows hardware','Windows self-update release gate')
}
foreach($path in @($TestExecutable,$ProbeFixture,$PSCommandPath)){$report.files+=@{path=$path;sha256=(Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant()}}
foreach($relative in @('Cargo.lock','crates/spanreed-adapters/Cargo.toml','crates/spanreed-adapters/src/self_update.rs','crates/spanreed-adapters/src/self_update/windows.rs','crates/spanreed-adapters/src/self_update/durable.rs','crates/spanreed-adapters/src/self_update/install.rs','crates/spanreed-adapters/examples/update_probe_fixture.rs','vendor/fabrials-libs.source')){$report.files+=@{path=$relative;sha256=(Get-FileHash -LiteralPath (Join-Path $Source $relative) -Algorithm SHA256).Hash.ToLowerInvariant()}}
Push-Location $Source
try {
 foreach($item in @(@{name='self_update';arguments=@('self_update::','--test-threads=1');ignored=1},@{name='native_job';arguments=@('self_update::windows::tests::native_job_contains_rapid_exit_timeouts_and_descendants','--ignored','--exact','--test-threads=1');ignored=0})){
  $start=New-Object System.Diagnostics.ProcessStartInfo
  $start.FileName=$TestExecutable
  $start.Arguments=$item.arguments -join ' '
  $start.UseShellExecute=$false
  $start.CreateNoWindow=$true
  $start.RedirectStandardOutput=$true
  $start.RedirectStandardError=$true
  $process=New-Object System.Diagnostics.Process
  $process.StartInfo=$start
  if(-not $process.Start()){throw 'Test process could not start'}
  $stdout=$process.StandardOutput.ReadToEndAsync()
  $stderr=$process.StandardError.ReadToEndAsync()
  if(-not $process.WaitForExit(120000)) { & taskkill.exe /PID $process.Id /T /F | Out-Null; throw ('Test timeout: '+$item.name) }
  $code=$process.ExitCode
  $body=$stdout.GetAwaiter().GetResult()
  [IO.File]::WriteAllText((Join-Path $EvidenceDirectory ($item.name+'.log')),$body)
  [IO.File]::WriteAllText((Join-Path $EvidenceDirectory ($item.name+'.stderr')),$stderr.GetAwaiter().GetResult())
  $process.Dispose()
  $match=[regex]::Match($body,'test result: ok\. (\d+) passed; 0 failed; (\d+) ignored;')
  $passed=if($match.Success){[int]$match.Groups[1].Value}else{0}
  $ignored=if($match.Success){[int]$match.Groups[2].Value}else{-1}
  $report.checks+=@{name=$item.name;exitCode=$code;passed=$passed;ignored=$ignored}
  if($code -ne 0 -or $passed -lt 1 -or $ignored -ne $item.ignored){throw ('Test failed: '+$item.name)}
 }
 $report.result='passed'
} catch { $report.result='failed';$report.failure=$_.Exception.Message;throw }
finally { Pop-Location; $report.finishedAt=[DateTime]::UtcNow.ToString('o');$report|ConvertTo-Json -Depth 6|Set-Content -LiteralPath (Join-Path $EvidenceDirectory 'report.json') -Encoding utf8 }
