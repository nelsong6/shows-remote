#Requires -Version 7.0
param(
  [Parameter(Position=0)][ValidateSet('run','test')][string]$Mode='run',
  [Parameter(ValueFromRemainingArguments=$true)][string[]]$RemoteArgs
)
$ErrorActionPreference='Stop'
$repo = Split-Path -Parent $PSScriptRoot
$cache = if ($env:SHOWS_REMOTE_TARGET_DIR) { $env:SHOWS_REMOTE_TARGET_DIR } else { 'D:\build\shows-remote\cargo' }
$arguments = @('--manifest-path',(Join-Path $repo 'Cargo.toml'),'--locked','-j','4','--target-dir',$cache)
if ($Mode -eq 'test') { & cargo.exe test @arguments; exit $LASTEXITCODE }
& cargo.exe build @arguments
if ($LASTEXITCODE) { throw 'Shows remote debug build failed' }
& (Join-Path $cache 'debug\shows-remote.exe') @RemoteArgs
exit $LASTEXITCODE
