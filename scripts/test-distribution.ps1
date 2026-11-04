param(
  [string]$RemoteExe = 'D:\build\shows-remote\cargo\debug\shows-remote.exe',
  [string]$SignedBundleDirectory
)
$ErrorActionPreference='Stop'
$tempBase=[IO.Path]::GetFullPath([IO.Path]::GetTempPath())
$fixture=Join-Path $tempBase ('shows-remote-distribution-' + [guid]::NewGuid().ToString('N'))
$nas=Join-Path $fixture 'nas'
$install=Join-Path $fixture 'installed'
New-Item -ItemType Directory -Path $fixture | Out-Null
try {
  & "$PSScriptRoot/publish.ps1" -Channel dev -NasRoot $nas -Version first -ExePath $RemoteExe
  $oldVersion=Join-Path $install 'versions\old'
  New-Item -ItemType Directory -Path $oldVersion,(Join-Path $install 'channels') -Force | Out-Null
  Copy-Item -LiteralPath $RemoteExe -Destination (Join-Path $oldVersion 'shows-remote.exe')
  New-Item -ItemType Junction -Path (Join-Path $install 'channels\dev') -Target $oldVersion | Out-Null
  & "$PSScriptRoot/install.ps1" -Channel dev -NasRoot $nas -InstallRoot $install -NoPathUpdate
  $native=Join-Path $install 'channels\dev\shows-remote.exe'
  if ((Get-Item (Split-Path $native)).Attributes -band [IO.FileAttributes]::ReparsePoint) { throw 'Native entry still traverses a reparse point' }
  if ((Get-Item $native).LinkType -ne 'HardLink') { throw 'Native executable is not a hard link to its immutable version' }
  $output=& $native --version
  if ($LASTEXITCODE -or "$output" -notmatch 'shows-remote') { throw 'Native independent launch failed' }
  if (Test-Path (Join-Path $install 'agents')) { throw 'Installation created a shell agent wrapper' }
  if (Test-Path (Join-Path $install 'versions\first\shows-desktop.exe')) { throw 'Remote installation contains a player' }
  $manifest=Get-Content -Raw (Join-Path $nas 'channels\dev.json') | ConvertFrom-Json
  if ($manifest.files.Count -ne 1 -or $manifest.product -ne 'shows-remote') { throw 'Independent manifest is invalid' }
  & "$PSScriptRoot/install.ps1" -Channel dev -NasRoot $nas -InstallRoot $install -NoPathUpdate
  & "$PSScriptRoot/publish.ps1" -Channel dev -NasRoot $nas -Version second -ExePath $RemoteExe
  & "$PSScriptRoot/install.ps1" -Channel dev -NasRoot $nas -InstallRoot $install -NoPathUpdate
  $state=Get-Content -Raw (Join-Path $install 'launcher-state-dev.json') | ConvertFrom-Json
  if ($state.current -ne 'second' -or $state.previous -ne 'first') { throw 'Independent version switch failed' }
  if (-not (Test-Path (Join-Path $install 'versions\first\shows-remote.exe'))) { throw 'Update deleted a previous immutable binary' }
  & "$PSScriptRoot/publish.ps1" -Channel dev -NasRoot $nas -Version corrupt -ExePath $RemoteExe
  [IO.File]::WriteAllText((Join-Path $nas 'versions\corrupt\shows-remote.exe'),'damaged')
  $rejected=$false
  try { & "$PSScriptRoot/install.ps1" -Channel dev -NasRoot $nas -InstallRoot $install -NoPathUpdate } catch { $rejected=$true }
  if (-not $rejected) { throw 'Corrupt update was accepted' }
  $state=Get-Content -Raw (Join-Path $install 'launcher-state-dev.json') | ConvertFrom-Json
  if ($state.current -ne 'second' -or (Get-FileHash $native -Algorithm SHA256).Hash.ToLowerInvariant() -ne $manifest.files[0].sha256) { throw 'Failed update changed selection' }
  # A valid hash alone never authorizes an unsigned stable binary.
  & "$PSScriptRoot/publish.ps1" -Channel dev -NasRoot $nas -Version unsigned -ExePath $RemoteExe
  $manifest=Get-Content -Raw (Join-Path $nas 'channels\dev.json') | ConvertFrom-Json
  $stable=Join-Path $nas 'channels\stable.json'
  $manifest.channel='stable'
  $manifest | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath $stable
  $rejected=$false
  try { & "$PSScriptRoot/install.ps1" -Channel stable -NasRoot $nas -InstallRoot $install -NoPathUpdate } catch { $rejected=$true }
  if (-not $rejected -or (Test-Path (Join-Path $install 'launcher-state-stable.json')) -or (Test-Path (Join-Path $install 'versions\unsigned'))) { throw 'Unsigned stable update was selected or retained' }
  if ($SignedBundleDirectory) {
    $signed=Join-Path $SignedBundleDirectory 'shows-remote.exe'
    $manifest.version='signed'
    $manifest.files=@(@{name='shows-remote.exe';length=(Get-Item $signed).Length;sha256=(Get-FileHash $signed -Algorithm SHA256).Hash.ToLowerInvariant()})
    New-Item -ItemType Directory -Path (Join-Path $nas 'versions\signed') | Out-Null
    Copy-Item -LiteralPath $signed -Destination (Join-Path $nas 'versions\signed\shows-remote.exe')
    $manifest | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath $stable
    & (Join-Path $SignedBundleDirectory 'install.ps1') -Channel stable -NasRoot $nas -InstallRoot $install -NoPathUpdate
    $state=Get-Content -Raw (Join-Path $install 'launcher-state-stable.json') | ConvertFrom-Json
    if ($state.current -ne 'signed') { throw 'Signed stable remote was not installed' }
  }
  Write-Host 'Standalone distribution passed: native entry, independent versions, hash/signature rejection and retained selection'
} finally {
  $resolved=[IO.Path]::GetFullPath($fixture)
  if (-not $resolved.StartsWith($tempBase,[StringComparison]::OrdinalIgnoreCase) -or (Split-Path -Leaf $resolved) -notlike 'shows-remote-distribution-*') { throw 'Unsafe fixture cleanup path' }
  Remove-Item -LiteralPath $resolved -Recurse -Force
}
$global:LASTEXITCODE=0
