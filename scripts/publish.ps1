#Requires -Version 7.0
[CmdletBinding()]
param(
  [ValidateSet('stable','dev')][string]$Channel = 'stable',
  [string]$NasRoot = '\\NelsonNAS\files\shows-remote-app',
  [string]$Release,
  [switch]$WaitForRelease,
  [string[]]$InstallTargets = @(),
  [string]$Version,
  [string]$ExePath
)
$ErrorActionPreference = 'Stop'
$expectedSigner = 'FE35227FED82F2429AFE763A36358ADDD79C7A4F'
$previousPrompt = $env:GIT_TERMINAL_PROMPT
$previousInteractive = $env:GCM_INTERACTIVE
$download = $null
$package = $null
$stage = $null
$cacheRoot = Join-Path ([Environment]::GetFolderPath('LocalApplicationData')) 'shows-remote'
try {
  if ($Channel -eq 'stable') {
    if ($Version -or $ExePath) { throw 'Stable accepts only its signed Forgejo release' }
    $env:GIT_TERMINAL_PROMPT = '0'; $env:GCM_INTERACTIVE = 'Never'
    $credential = "protocol=https`nhost=forgejo.romaine.life`n`n" | git -c credential.interactive=never credential fill
    if ($LASTEXITCODE) { throw 'Forgejo credential lookup failed' }
    $password = $credential | Where-Object { $_.StartsWith('password=') } | Select-Object -First 1
    if (-not $password) { throw 'No stored Forgejo credential' }
    $headers = @{Authorization='token ' + $password.Substring(9)}
    $api = 'https://forgejo.romaine.life/api/v1/repos/nelsong6/shows-remote'
    if (-not $Release) {
      $sha = (Invoke-RestMethod "$api/branches/main" -Headers $headers).commit.id
      $Release = 'remote-' + $sha.Substring(0,7)
    }
    if ($Release -notmatch '^remote-([0-9a-f]{7,40})$') { throw 'Invalid standalone release tag' }
    $Version = $Matches[1]
    $deadline = (Get-Date).AddMinutes(20)
    do {
      $metadata = Invoke-RestMethod "$api/releases/tags/$Release" -Headers $headers -SkipHttpErrorCheck -StatusCodeVariable code
      if ($code -eq 200 -and -not $metadata.draft -and @($metadata.assets | Where-Object name -eq 'shows-remote-windows-amd64.zip').Count -eq 1) {
        $status = Invoke-RestMethod "$api/commits/$Version/status?limit=100" -Headers $headers
        if ($status.state -eq 'success' -and $status.total_count -gt 0) { break }
      }
      if ($code -notin @(200,404)) { throw "Release lookup returned $code" }
      if (-not $WaitForRelease -or (Get-Date) -gt $deadline) { throw "Signed release $Release is not published" }
      $status = Invoke-RestMethod "$api/commits/$Version/status" -Headers $headers
      if ($status.statuses | Where-Object { $_.status -in @('error','failure') }) { throw "Release $Release has a failed check" }
      Write-Host "Waiting for signed Shows remote release $Release"
      Start-Sleep -Seconds 10
    } while ($true)
    New-Item -ItemType Directory -Force -Path $cacheRoot | Out-Null
    $download = Join-Path $cacheRoot ('.release-' + [guid]::NewGuid().ToString('N'))
    New-Item -ItemType Directory -Path $download | Out-Null
    $asset = @($metadata.assets | Where-Object name -eq 'shows-remote-windows-amd64.zip')[0]
    Invoke-WebRequest $asset.browser_download_url -Headers $headers -OutFile (Join-Path $download 'release.zip')
    Expand-Archive -LiteralPath (Join-Path $download 'release.zip') -DestinationPath (Join-Path $download 'bundle')
    $bundle = Join-Path $download 'bundle'
    foreach ($name in @('shows-remote.exe','install.ps1','install-update.ps1','configure-defender.ps1')) {
      $signature = Get-AuthenticodeSignature -LiteralPath (Join-Path $bundle $name)
      if ($signature.Status -ne 'Valid' -or $signature.SignerCertificate.Thumbprint -ne $expectedSigner) { throw "Release signature rejected: $name" }
    }
    $ExePath = Join-Path $bundle 'shows-remote.exe'
    $tagged = Invoke-RestMethod "$api/tags/$Release" -Headers $headers
    if (-not $tagged.commit.sha.StartsWith($Version) -or $metadata.target_commitish -ne $tagged.commit.sha) { throw 'Release tag and source revision disagree' }
    $identity = & $ExePath --version
    if ($LASTEXITCODE -or "$identity" -notmatch [regex]::Escape($tagged.commit.sha)) { throw 'Signed client does not identify the released commit' }
  } else {
    if (-not $Version -or -not $ExePath) { throw 'Dev publication requires an explicit Version and ExePath' }
    $bundle = $PSScriptRoot
  }
  if ($Version -notmatch '^[A-Za-z0-9._-]+$') { throw 'Unsafe channel version' }
  $versions = Join-Path $NasRoot 'versions'
  $channels = Join-Path $NasRoot 'channels'
  New-Item -ItemType Directory -Force -Path $versions,$channels | Out-Null
  $file = Get-Item -LiteralPath $ExePath
  $entry = [ordered]@{name='shows-remote.exe';length=$file.Length;sha256=(Get-FileHash -Algorithm SHA256 -LiteralPath $file.FullName).Hash.ToLowerInvariant()}
  $manifest = [ordered]@{schema=2;product='shows-remote';channel=$Channel;version=$Version;files=@($entry);archive=$null}
  $destination = Join-Path $versions $Version
  if (Test-Path -LiteralPath $destination) {
    $existing = Get-Content -Raw -LiteralPath (Join-Path $destination 'manifest.json') | ConvertFrom-Json
    if ($existing.schema -ne 2 -or $existing.product -ne 'shows-remote' -or $existing.files.Count -ne 1 -or $existing.files[0].sha256 -ne $entry.sha256 -or $existing.archive.name -ne 'shows-remote.zip') { throw 'Immutable release content differs' }
    $existingArchive = Join-Path $destination $existing.archive.name
    if ((Get-Item -LiteralPath $existingArchive).Length -ne $existing.archive.length -or (Get-FileHash -Algorithm SHA256 -LiteralPath $existingArchive).Hash.ToLowerInvariant() -ne $existing.archive.sha256) { throw 'Immutable release archive differs' }
    $manifest.archive = $existing.archive
  } else {
    New-Item -ItemType Directory -Force -Path $cacheRoot | Out-Null
    $package = Join-Path $cacheRoot ('.package-' + [guid]::NewGuid().ToString('N'))
    New-Item -ItemType Directory -Path (Join-Path $package 'payload') | Out-Null
    Copy-Item -LiteralPath $ExePath -Destination (Join-Path $package 'payload\shows-remote.exe')
    $archive = Join-Path $package 'shows-remote.zip'
    Compress-Archive -LiteralPath (Join-Path $package 'payload\shows-remote.exe') -DestinationPath $archive
    $manifest.archive = @{name='shows-remote.zip';length=(Get-Item -LiteralPath $archive).Length;sha256=(Get-FileHash -Algorithm SHA256 -LiteralPath $archive).Hash.ToLowerInvariant()}
    $stage = Join-Path $NasRoot ('.staging-' + [guid]::NewGuid().ToString('N'))
    New-Item -ItemType Directory -Path $stage | Out-Null
    Copy-Item -LiteralPath $archive -Destination (Join-Path $stage 'shows-remote.zip')
    [IO.File]::WriteAllText((Join-Path $stage 'manifest.json'),($manifest | ConvertTo-Json -Depth 5))
    Move-Item -LiteralPath $stage -Destination $destination
  }
  foreach ($name in @('install.ps1','install-update.ps1','configure-defender.ps1')) {
    $temporary = Join-Path $NasRoot ($name + '.' + [guid]::NewGuid().ToString('N') + '.tmp')
    Copy-Item -LiteralPath (Join-Path $bundle $name) -Destination $temporary
    Move-Item -LiteralPath $temporary -Destination (Join-Path $NasRoot $name) -Force
  }
  $pointer = Join-Path $channels ($Channel + '.' + [guid]::NewGuid().ToString('N') + '.tmp')
  [IO.File]::WriteAllText($pointer,($manifest | ConvertTo-Json -Depth 5))
  Move-Item -LiteralPath $pointer -Destination (Join-Path $channels "$Channel.json") -Force
  Write-Host "Published standalone Shows remote $Version on $Channel"
  foreach ($target in $InstallTargets) { & (Join-Path $bundle 'install-update.ps1') -Target $target -Channel $Channel -NasRoot $NasRoot }
} finally {
  $env:GIT_TERMINAL_PROMPT = $previousPrompt
  $env:GCM_INTERACTIVE = $previousInteractive
  if ($stage -and (Test-Path -LiteralPath $stage)) {
    $resolved = [IO.Path]::GetFullPath($stage)
    if (-not $resolved.StartsWith([IO.Path]::GetFullPath($NasRoot) + [IO.Path]::DirectorySeparatorChar,[StringComparison]::OrdinalIgnoreCase) -or (Split-Path -Leaf $resolved) -notlike '.staging-*') { throw 'Unsafe publication cleanup path' }
    Remove-Item -LiteralPath $resolved -Recurse -Force
  }
  foreach ($cache in @($download,$package)) {
    if ($cache -and (Test-Path -LiteralPath $cache)) {
      $resolved = [IO.Path]::GetFullPath($cache)
      if (-not $resolved.StartsWith([IO.Path]::GetFullPath($cacheRoot) + [IO.Path]::DirectorySeparatorChar,[StringComparison]::OrdinalIgnoreCase) -or (Split-Path -Leaf $resolved) -notmatch '^\.(release|package)-[0-9a-f]{32}$') { throw 'Unsafe release cleanup path' }
      Remove-Item -LiteralPath $resolved -Recurse -Force
    }
  }
}
