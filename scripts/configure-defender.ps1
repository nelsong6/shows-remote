#Requires -Version 7.0
#Requires -RunAsAdministrator
[CmdletBinding()]
param([ValidateSet('Check','Apply','Remove')][string]$Action = 'Check')

$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
$local = [Environment]::GetFolderPath('LocalApplicationData')
if (-not $local -or -not [IO.Path]::IsPathFullyQualified($local)) { throw 'The current user has no local application-data folder' }
$root = [IO.Path]::GetFullPath((Join-Path $local 'shows-remote'))
$directory = Get-Item -LiteralPath $root -Force -ErrorAction SilentlyContinue
if ($directory -and ($directory.Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw 'The remote installation root must be a real directory' }
$before = @(Get-MpPreference | Select-Object -ExpandProperty ExclusionPath)
$configured = $root -in $before
switch ($Action) {
  'Apply' { if (-not $configured) { Add-MpPreference -ExclusionPath $root } }
  'Remove' { if ($configured) { Remove-MpPreference -ExclusionPath $root } }
}
$after = @(Get-MpPreference | Select-Object -ExpandProperty ExclusionPath)
$configured = $root -in $after
if ($Action -eq 'Apply' -and -not $configured) { throw 'Defender did not retain the app-folder exclusion' }
if ($Action -eq 'Remove' -and $configured) { throw 'Defender did not remove the app-folder exclusion' }
$unrelatedBefore = @($before | Where-Object { $_ -and $_ -ine $root } | Sort-Object)
$unrelatedAfter = @($after | Where-Object { $_ -and $_ -ine $root } | Sort-Object)
if (Compare-Object $unrelatedBefore $unrelatedAfter) { throw 'Unrelated Defender exclusions changed during configuration' }
$status = Get-MpComputerStatus
[pscustomobject]@{
  computer = $env:COMPUTERNAME
  path = $root
  excluded = $configured
  real_time_protection = $status.RealTimeProtectionEnabled
  behavior_monitoring = $status.BehaviorMonitorEnabled
  antivirus = $status.AntivirusEnabled
} | ConvertTo-Json -Compress
