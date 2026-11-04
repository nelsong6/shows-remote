[CmdletBinding(PositionalBinding = $false)]
param(
  [ValidateSet("stable", "dev")]
  [string]$Channel = "stable",
  [string]$NasRoot = "\\NelsonNAS\files\shows-remote-app",
  [string]$InstallRoot = (Join-Path $env:LOCALAPPDATA "shows-remote"),
  [switch]$NoPathUpdate
)

$ErrorActionPreference = "Stop"
$versions = Join-Path $InstallRoot "versions"
$statePath = Join-Path $InstallRoot "launcher-state-$Channel.json"
New-Item -ItemType Directory -Force -Path $versions | Out-Null

$state = if (Test-Path -LiteralPath $statePath) {
  Get-Content -Raw -LiteralPath $statePath | ConvertFrom-Json
} else { $null }

function Save-State([object]$nextState) {
  $temp = "$statePath.$([guid]::NewGuid().ToString('N')).tmp"
  [IO.File]::WriteAllText($temp, ($nextState | ConvertTo-Json -Depth 4), (New-Object Text.UTF8Encoding $false))
  Move-Item -Force -LiteralPath $temp -Destination $statePath
}

function Assert-File([string]$Path, [object]$Metadata) {
  if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) { throw "Missing remote file: $Path" }
  if ((Get-Item -LiteralPath $Path).Length -ne $Metadata.length) { throw 'Remote length mismatch' }
  if ((Get-FileHash -Algorithm SHA256 -LiteralPath $Path).Hash.ToLowerInvariant() -ne $Metadata.sha256) { throw 'Remote hash mismatch' }
  if ($Channel -eq 'stable') {
    $signature = Get-AuthenticodeSignature -LiteralPath $Path
    if ($signature.Status -ne 'Valid' -or $signature.SignerCertificate.Thumbprint -ne 'FE35227FED82F2429AFE763A36358ADDD79C7A4F') { throw 'The stable remote is not signed by the home identity' }
  }
}

function Install-Version([object]$manifest) {
  if ($manifest.schema -ne 1) { throw "Unsupported channel manifest schema: $($manifest.schema)" }
  if ($manifest.channel -ne $Channel) { throw "Channel manifest is for $($manifest.channel), expected $Channel" }
  if ($manifest.version -notmatch '^[A-Za-z0-9._-]+$') { throw "Unsafe version in manifest" }
  $fileNames = @($manifest.files | ForEach-Object { $_.name })
  if ($manifest.product -ne 'shows-remote' -or $fileNames.Count -ne 1) { throw 'This is not a standalone Shows remote channel' }
  foreach ($required in @("shows-remote.exe")) {
    if ($required -notin $fileNames) { throw "Manifest is missing $required" }
  }

  $destination = Join-Path $versions $manifest.version
  if (-not (Test-Path -LiteralPath $destination -PathType Container)) {
    $source = Join-Path (Join-Path $NasRoot "versions") $manifest.version
    $stage = Join-Path $InstallRoot ".staging-$([guid]::NewGuid().ToString('N'))"
    New-Item -ItemType Directory -Path $stage | Out-Null
    try {
      foreach ($file in $manifest.files) {
        if ($file.name -notmatch '^[A-Za-z0-9._-]+$') { throw "Unsafe file name in manifest" }
        $sourceFile = Join-Path $source $file.name
        $targetFile = Join-Path $stage $file.name
        Copy-Item -LiteralPath $sourceFile -Destination $targetFile
        Assert-File $targetFile $file
      }
      Move-Item -LiteralPath $stage -Destination $destination
    } finally {
      if (Test-Path -LiteralPath $stage) {
        $resolved = [IO.Path]::GetFullPath($stage)
        if (-not $resolved.StartsWith([IO.Path]::GetFullPath($InstallRoot) + [IO.Path]::DirectorySeparatorChar,[StringComparison]::OrdinalIgnoreCase)) { throw 'Unsafe installation cleanup path' }
        Remove-Item -Recurse -Force -LiteralPath $resolved
      }
    }
  }
  foreach ($file in $manifest.files) {
    $installedFile = Join-Path $destination $file.name
    Assert-File $installedFile $file
  }
  return $destination
}

$oldVersion = if ($state -and $state.current) { [string]$state.current } else { $null }
$manifestPath = Join-Path $NasRoot "channels\$Channel.json"
$manifest = Get-Content -Raw -LiteralPath $manifestPath | ConvertFrom-Json
$null = Install-Version $manifest
$selectedVersion = [string]$manifest.version
$nextState = [ordered]@{schema=1;channel=$Channel;current=$selectedVersion;previous=$oldVersion;updated_at=(Get-Date).ToUniversalTime().ToString('o')}

if (-not $selectedVersion) { throw "No verified Shows remote version was selected" }
if ($selectedVersion -notmatch '^[A-Za-z0-9._-]+$') { throw 'Invalid installed version' }
$selectedDir = Join-Path $versions $selectedVersion
$exe = Join-Path $selectedDir "shows-remote.exe"
if (-not (Test-Path -LiteralPath $exe -PathType Leaf)) { throw "Installed executable is missing: $exe" }

# SSH enforces RedirectionGuard on user-created directory junctions. A hard
# link exposes the immutable executable without introducing a reparse point.
$channels = [IO.Path]::GetFullPath((Join-Path $InstallRoot 'channels'))
New-Item -ItemType Directory -Force -Path $channels | Out-Null
$entryDir = Join-Path $channels $Channel
$entry = Join-Path $entryDir 'shows-remote.exe'
$next = Join-Path $channels "$Channel.next.exe"
$previous = Join-Path $channels "$Channel.previous.exe"
foreach ($directory in @($channels,$entryDir)) {
  $item = Get-Item -LiteralPath $directory -Force -ErrorAction SilentlyContinue
  if ($item -and ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -and ($directory -eq $channels -or $item.LinkType -ne 'Junction')) { throw "Unexpected native entry type: $directory" }
}
foreach ($path in @($entry, $next, $previous)) {
  if (-not ([IO.Path]::GetFullPath($path)).StartsWith($channels + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) { throw 'Unsafe native entry path' }
}
$directory = Get-Item -LiteralPath $entryDir -Force -ErrorAction SilentlyContinue
if ($directory -and $directory.LinkType -eq 'Junction') {
  $target = [IO.Path]::GetFullPath([string]$directory.Target)
  if (-not $target.StartsWith([IO.Path]::GetFullPath($versions) + [IO.Path]::DirectorySeparatorChar,[StringComparison]::OrdinalIgnoreCase)) { throw 'Retired entry points outside managed versions' }
  if (Test-Path -LiteralPath $previous) { [IO.File]::Delete($previous) }
  New-Item -ItemType HardLink -Path $previous -Target (Join-Path $target 'shows-remote.exe') | Out-Null
  [IO.Directory]::Delete($entryDir)
}
New-Item -ItemType Directory -Force -Path $entryDir | Out-Null
foreach ($path in @($entry,$next,$previous)) {
  $item = Get-Item -LiteralPath $path -Force -ErrorAction SilentlyContinue
  if ($item -and ($item.PSIsContainer -or $item.LinkType -ne 'HardLink')) { throw "Refusing to replace unmanaged native file: $path" }
}
if (Test-Path -LiteralPath $previous) {
  if (-not (Test-Path -LiteralPath $entry)) { [IO.File]::Move($previous, $entry) }
  else { [IO.File]::Delete($previous) }
}
if (Test-Path -LiteralPath $next) { [IO.File]::Delete($next) }
$exists = Test-Path -LiteralPath $entry
if (-not $exists -or $oldVersion -ne $selectedVersion -or (Get-FileHash -LiteralPath $entry -Algorithm SHA256).Hash.ToLowerInvariant() -ne $manifest.files[0].sha256) {
  New-Item -ItemType HardLink -Path $next -Target $exe | Out-Null
  try {
    if ($exists) { [IO.File]::Replace($next, $entry, $previous) }
    else { [IO.File]::Move($next, $entry) }
    if ($oldVersion -ne $selectedVersion) { Save-State $nextState }
  } catch {
    if (Test-Path -LiteralPath $previous) {
      if (Test-Path -LiteralPath $entry) { [IO.File]::Replace($previous, $entry, $null) }
      else { [IO.File]::Move($previous, $entry) }
    } elseif (-not $exists -and (Test-Path -LiteralPath $entry)) {
      [IO.File]::Delete($entry)
    }
    throw
  } finally { if (Test-Path -LiteralPath $next) { [IO.File]::Delete($next) } }
  if (Test-Path -LiteralPath $previous) { [IO.File]::Delete($previous) }
} elseif ($oldVersion -ne $selectedVersion) { Save-State $nextState }
if (-not $NoPathUpdate) {
  $oldEntry = Join-Path $env:LOCALAPPDATA "shows\channels\$Channel"
  $userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
  $entries = @($userPath -split ';' | Where-Object { $_ -and $_ -ine $entryDir -and $_ -ine $oldEntry })
  [Environment]::SetEnvironmentVariable('Path', ((@($entryDir) + $entries) -join ';'), 'User')
  $env:Path = $entryDir + ';' + (($env:Path -split ';' | Where-Object { $_ -and $_ -ine $entryDir -and $_ -ine $oldEntry }) -join ';')
}
Write-Host "Installed independent Shows remote $selectedVersion at $entry"
