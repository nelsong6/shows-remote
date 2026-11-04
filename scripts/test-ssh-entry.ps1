#Requires -Version 7.0
param([Parameter(Mandatory)][string]$RemoteExe)
$ErrorActionPreference='Stop'
$ProgressPreference='SilentlyContinue'
if (-not $env:SSH_CONNECTION) { throw 'Run this regression inside a real Windows OpenSSH session' }
Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class RemoteEntryMitigation {
 public const int RedirectionTrustPolicy = 16;
 [DllImport("kernel32.dll")] public static extern IntPtr GetCurrentProcess();
 [DllImport("kernel32.dll",SetLastError=true)] public static extern bool GetProcessMitigationPolicy(IntPtr process,int policy,out uint flags,UIntPtr length);
}
'@
$flags=[uint32]0
if (-not [RemoteEntryMitigation]::GetProcessMitigationPolicy([RemoteEntryMitigation]::GetCurrentProcess(),[RemoteEntryMitigation]::RedirectionTrustPolicy,[ref]$flags,[UIntPtr]4) -or ($flags -band 1) -eq 0) { throw 'This session does not enforce RedirectionGuard; it cannot prove the SSH regression' }
& "$PSScriptRoot/test-distribution.ps1" -RemoteExe $RemoteExe
$tempBase=[IO.Path]::GetFullPath([IO.Path]::GetTempPath())
$fixture=Join-Path $tempBase ('shows-remote-ssh-entry-' + [guid]::NewGuid().ToString('N'))
$nas=Join-Path $fixture 'nas'
$install=Join-Path $fixture 'installed'
$child=$null
function Read-Reply([int]$Id) {
 for($attempt=0;$attempt -lt 8;$attempt++) {
  $line=$child.StandardOutput.ReadLineAsync()
  if (-not $line.Wait(5000) -or -not $line.Result) { throw 'SSH agent did not return a bounded protocol reply' }
  $frame=$line.Result|ConvertFrom-Json
  if($frame.kind -eq 'reply' -and $frame.id -eq $Id){
   if (-not $frame.result.PSObject.Properties['Ok']) { throw 'SSH agent rejected the read request' }
   return $frame.result.Ok
  }
 }
 throw 'SSH agent did not acknowledge the expected read'
}
try {
 & "$PSScriptRoot/publish.ps1" -Channel dev -NasRoot $nas -Version first -ExePath $RemoteExe
 & "$PSScriptRoot/install.ps1" -Channel dev -NasRoot $nas -InstallRoot $install -NoPathUpdate
 $native=Join-Path $install 'channels\dev\shows-remote.exe'
 $identity=& $native --version
 if ($LASTEXITCODE) { throw 'SSH could not execute the installed native file' }
 $start=[Diagnostics.ProcessStartInfo]::new($native)
 $start.ArgumentList.Add('--agent');$start.UseShellExecute=$false;$start.CreateNoWindow=$true
 $start.RedirectStandardInput=$true;$start.RedirectStandardOutput=$true;$start.RedirectStandardError=$true
 $child=[Diagnostics.Process]::Start($start)
 $child.StandardInput.WriteLine('{"id":1,"command":{"action":"watch"}}');$child.StandardInput.Flush()
 $null=Read-Reply 1
 & "$PSScriptRoot/publish.ps1" -Channel dev -NasRoot $nas -Version second -ExePath $RemoteExe
 & "$PSScriptRoot/install.ps1" -Channel dev -NasRoot $nas -InstallRoot $install -NoPathUpdate
 $child.StandardInput.WriteLine('{"id":2,"command":{"action":"status"}}');$child.StandardInput.Flush()
 $status=Read-Reply 2
 if ($child.HasExited -or -not $status.phase) { throw 'Updating the native entry broke the connected agent' }
 [pscustomobject]@{redirection_guard_enforced=$true;native_identity=$identity;active_agent_update=$true;live_status=$status.phase}|ConvertTo-Json
} finally {
 if ($child) { $child.StandardInput.Close();if(-not $child.WaitForExit(2000)){$child.Kill($true)};$child.Dispose() }
 $resolved=[IO.Path]::GetFullPath($fixture)
 if(-not $resolved.StartsWith($tempBase,[StringComparison]::OrdinalIgnoreCase) -or (Split-Path -Leaf $resolved) -notlike 'shows-remote-ssh-entry-*'){throw 'Unsafe SSH fixture cleanup'}
 if(Test-Path -LiteralPath $resolved){Remove-Item -LiteralPath $resolved -Recurse -Force}
}
$global:LASTEXITCODE=0
