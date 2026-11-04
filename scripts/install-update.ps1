#Requires -Version 7.0
[CmdletBinding()]
param(
  [string]$Target = 'local',
  [ValidateSet('stable', 'dev')][string]$Channel = 'stable',
  [string]$NasRoot = '\\NelsonNAS\files\shows-remote-app',
  [string]$ResultPath
)

$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'

function Invoke-Native([string]$File, [string[]]$NativeArguments) {
  $start = [Diagnostics.ProcessStartInfo]::new($File)
  $start.UseShellExecute = $false
  $start.CreateNoWindow = $true
  $start.RedirectStandardOutput = $true
  $start.RedirectStandardError = $true
  foreach ($argument in $NativeArguments) { $start.ArgumentList.Add($argument) }
  $process = [Diagnostics.Process]::Start($start)
  $output = $process.StandardOutput.ReadToEndAsync()
  $errors = $process.StandardError.ReadToEndAsync()
  if (-not $process.WaitForExit(120000)) { $process.Kill($true); throw "$File timed out" }
  if ($process.ExitCode -ne 0) { throw "$File failed: $($errors.GetAwaiter().GetResult())" }
  $output.GetAwaiter().GetResult()
}

if ($Target -ne 'local') {
  if ($Target -notmatch '^[A-Za-z0-9][A-Za-z0-9.-]{0,252}$') { throw 'Target must be a computer hostname or SSH alias' }
  $request = @{
    channel = $Channel
    nas = $NasRoot
    id = [guid]::NewGuid().ToString('N')
  } | ConvertTo-Json -Compress
  $requestObject = $request | ConvertFrom-Json
  $remoteTemp = (Invoke-Native 'ssh.exe' @('-T','-o','BatchMode=yes','-o','StrictHostKeyChecking=yes','-o','ConnectTimeout=8',$Target,'echo %TEMP%')).Trim()
  if ($remoteTemp -notmatch '^[A-Za-z]:\\[^\r\n]+$') { throw 'Target did not provide a Windows temporary directory' }
  $remoteScript = $remoteTemp.TrimEnd('\') + '\shows-remote-update-' + $requestObject.id + '.ps1'
  Invoke-Native 'scp.exe' @('-q','-o','BatchMode=yes','-o','StrictHostKeyChecking=yes','-o','ConnectTimeout=8',$PSCommandPath, ('{0}:{1}' -f $Target, $remoteScript.Replace('\','/'))) | Out-Null
  $request64 = [Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes($request))
  $bootstrap = @'
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
$request = [Text.Encoding]::UTF8.GetString([Convert]::FromBase64String('REQUEST64')) | ConvertFrom-Json
$script = Join-Path $env:TEMP ('shows-remote-update-' + $request.id + '.ps1')
$result = $script + '.json'
$name = 'Shows remote update ' + $request.id
$arguments = '-NoProfile -NonInteractive -WindowStyle Hidden -File "' + $script + '" -Channel ' + $request.channel + ' -NasRoot "' + $request.nas + '" -ResultPath "' + $result + '"'
$action = New-ScheduledTaskAction -Execute 'C:\Program Files\PowerShell\7\pwsh.exe' -Argument $arguments
$principal = New-ScheduledTaskPrincipal -UserId ($env:COMPUTERNAME + '\' + $env:USERNAME) -LogonType Interactive -RunLevel Limited
Register-ScheduledTask -TaskName $name -Action $action -Principal $principal -Force | Out-Null
Start-ScheduledTask -TaskName $name
try {
  $deadline = (Get-Date).AddSeconds(90)
  while (-not (Test-Path -LiteralPath $result)) {
    if ((Get-Date) -gt $deadline) { throw 'Interactive update timed out; inspect the owned Shows update task' }
    Start-Sleep -Milliseconds 250
  }
  Get-Content -Raw -LiteralPath $result
} finally {
  if (Test-Path -LiteralPath $result) {
    Unregister-ScheduledTask -TaskName $name -Confirm:$false
  }
}
'@
  $bootstrap = $bootstrap.Replace('REQUEST64', $request64)
  $encoded = [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($bootstrap))
  $output = Invoke-Native 'ssh.exe' @('-T','-o','BatchMode=yes','-o','StrictHostKeyChecking=yes','-o','ConnectTimeout=8',$Target,'"C:\Program Files\PowerShell\7\pwsh.exe"','-NoProfile','-NonInteractive','-EncodedCommand',$encoded)
  $result = $output | ConvertFrom-Json
  if (-not $result.ok) { throw "$Target update failed: $($result.message)" }
  $result | ConvertTo-Json -Depth 6
  return
}

try {
  $install = Join-Path $env:LOCALAPPDATA 'shows-remote'
  $installer = Join-Path $NasRoot 'install.ps1'
  if ($Channel -eq 'stable') {
    $signature = Get-AuthenticodeSignature -LiteralPath $installer
    if ($signature.Status -ne 'Valid' -or $signature.SignerCertificate.Thumbprint -ne 'FE35227FED82F2429AFE763A36358ADDD79C7A4F') { throw 'Standalone installer signature is invalid' }
  }
  & $installer -Channel $Channel -NasRoot $NasRoot
  $state = Get-Content -Raw -LiteralPath (Join-Path $install "launcher-state-$Channel.json") | ConvertFrom-Json
  $manifest = Get-Content -Raw -LiteralPath (Join-Path $NasRoot "channels\$Channel.json") | ConvertFrom-Json
  if ($state.current -ne $manifest.version) { throw 'Installed remote does not match its channel' }
  $result = @{ok=$true;computer=$env:COMPUTERNAME;version=$state.current;player_restarted=$false}
} catch {
  $result = @{ok=$false; computer=$env:COMPUTERNAME; message=$_.Exception.Message}
}
$json = $result | ConvertTo-Json -Depth 6
if ($ResultPath) {
  $temp = $ResultPath + '.tmp'
  [IO.File]::WriteAllText($temp, $json, [Text.UTF8Encoding]::new($false))
  Move-Item -LiteralPath $temp -Destination $ResultPath -Force
} else { $json }
if (-not $result.ok) { throw $result.message }
