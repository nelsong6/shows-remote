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
  if ($manifest.schema -ne 2 -or $manifest.files.Count -ne 1 -or $manifest.product -ne 'shows-remote' -or $manifest.archive.name -ne 'shows-remote.zip' -or (Test-Path (Join-Path $nas 'versions\first\shows-remote.exe'))) { throw 'Independent archive manifest is invalid' }
  & "$PSScriptRoot/install.ps1" -Channel dev -NasRoot $nas -InstallRoot $install -NoPathUpdate
  & "$PSScriptRoot/publish.ps1" -Channel dev -NasRoot $nas -Version second -ExePath $RemoteExe
  & "$PSScriptRoot/install.ps1" -Channel dev -NasRoot $nas -InstallRoot $install -NoPathUpdate
  $state=Get-Content -Raw (Join-Path $install 'launcher-state-dev.json') | ConvertFrom-Json
  if ($state.current -ne 'second' -or $state.previous -ne 'first') { throw 'Independent version switch failed' }
  if (-not (Test-Path (Join-Path $install 'versions\first\shows-remote.exe'))) { throw 'Update deleted a previous immutable binary' }
  & "$PSScriptRoot/publish.ps1" -Channel dev -NasRoot $nas -Version corrupt -ExePath $RemoteExe
  [IO.File]::WriteAllText((Join-Path $nas 'versions\corrupt\shows-remote.zip'),'damaged')
  $rejected=$false
  try { & "$PSScriptRoot/install.ps1" -Channel dev -NasRoot $nas -InstallRoot $install -NoPathUpdate } catch { $rejected=$true }
  if (-not $rejected) { throw 'Corrupt update was accepted' }
  $state=Get-Content -Raw (Join-Path $install 'launcher-state-dev.json') | ConvertFrom-Json
  if ($state.current -ne 'second' -or (Get-FileHash $native -Algorithm SHA256).Hash.ToLowerInvariant() -ne $manifest.files[0].sha256) { throw 'Failed update changed selection' }
  & "$PSScriptRoot/publish.ps1" -Channel dev -NasRoot $nas -Version unexpected -ExePath $RemoteExe
  $unexpected=Join-Path $nas 'versions\unexpected\shows-remote.zip'
  $zip=[IO.Compression.ZipFile]::Open($unexpected,[IO.Compression.ZipArchiveMode]::Update)
  try { $null=$zip.CreateEntry('../escaped.exe') } finally { $zip.Dispose() }
  $unexpectedManifest=Get-Content -Raw (Join-Path $nas 'channels\dev.json') | ConvertFrom-Json
  $unexpectedManifest.archive.length=(Get-Item $unexpected).Length
  $unexpectedManifest.archive.sha256=(Get-FileHash $unexpected -Algorithm SHA256).Hash.ToLowerInvariant()
  $unexpectedManifest | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $nas 'channels\dev.json')
  $rejected=$false
  try { & "$PSScriptRoot/install.ps1" -Channel dev -NasRoot $nas -InstallRoot $install -NoPathUpdate } catch { $rejected=$true }
  $state=Get-Content -Raw (Join-Path $install 'launcher-state-dev.json') | ConvertFrom-Json
  if (-not $rejected -or $state.current -ne 'second' -or (Test-Path (Join-Path $install 'versions\unexpected')) -or (Test-Path (Join-Path $install 'escaped.exe'))) { throw 'Unexpected archive entries were accepted' }
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
    foreach ($name in @('shows-remote.exe','install.ps1','install-update.ps1','configure-defender.ps1')) {
      $signature=Get-AuthenticodeSignature -LiteralPath (Join-Path $SignedBundleDirectory $name)
      if ($signature.Status -ne 'Valid' -or $signature.SignerCertificate.Thumbprint -ne 'FE35227FED82F2429AFE763A36358ADDD79C7A4F') { throw "Signed setup file rejected: $name" }
    }
    $exclusionsBefore=@((Get-MpPreference).ExclusionPath | Sort-Object)
    $policy=& (Join-Path $PSHOME 'pwsh.exe') -NoProfile -File (Join-Path $SignedBundleDirectory 'configure-defender.ps1') | ConvertFrom-Json
    if ($LASTEXITCODE -or $policy.path -ne (Join-Path ([Environment]::GetFolderPath('LocalApplicationData')) 'shows-remote')) { throw 'Signed Defender check failed' }
    if (Compare-Object $exclusionsBefore @((Get-MpPreference).ExclusionPath | Sort-Object)) { throw 'Read-only setup changed Defender exclusions' }
    $signed=Join-Path $SignedBundleDirectory 'shows-remote.exe'
    $manifest.version='signed'
    $manifest.files=@(@{name='shows-remote.exe';length=(Get-Item $signed).Length;sha256=(Get-FileHash $signed -Algorithm SHA256).Hash.ToLowerInvariant()})
    New-Item -ItemType Directory -Path (Join-Path $nas 'versions\signed') | Out-Null
    $signedArchive=Join-Path $nas 'versions\signed\shows-remote.zip'
    Compress-Archive -LiteralPath $signed -DestinationPath $signedArchive
    $manifest.archive=@{name='shows-remote.zip';length=(Get-Item $signedArchive).Length;sha256=(Get-FileHash $signedArchive -Algorithm SHA256).Hash.ToLowerInvariant()}
    $manifest | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath $stable
    & (Join-Path $SignedBundleDirectory 'install.ps1') -Channel stable -NasRoot $nas -InstallRoot $install -NoPathUpdate
    $state=Get-Content -Raw (Join-Path $install 'launcher-state-stable.json') | ConvertFrom-Json
    if ($state.current -ne 'signed') { throw 'Signed stable remote was not installed' }
  }
  Write-Host 'Standalone distribution passed: native entry, archived versions, archive/executable integrity, entry/signature rejection and retained selection'
} finally {
  $resolved=[IO.Path]::GetFullPath($fixture)
  if (-not $resolved.StartsWith($tempBase,[StringComparison]::OrdinalIgnoreCase) -or (Split-Path -Leaf $resolved) -notlike 'shows-remote-distribution-*') { throw 'Unsafe fixture cleanup path' }
  Remove-Item -LiteralPath $resolved -Recurse -Force
}
$global:LASTEXITCODE=0
