# shows-remote

Independent Windows terminal client and SSH agent for Shows. The player lives in
`nelsong6/shows`; it owns playback, the NAS SQLite database, video capture and the
versioned loopback HTTP/SSE protocol. This app never starts a player or reads that
database.

`src/` owns the CLI, terminal UI, fzt discovery/palette, and bounded SSH transport.
The target runs this same independently installed binary with `--agent`; its
native path is `%LOCALAPPDATA%\shows-remote\channels\stable\shows-remote.exe`.
`--channel dev` selects the independently installed remote's dev channel.

`shows-control` is the desktop-owned protocol crate, pinned by an immutable Git
revision in Cargo.toml and Cargo.lock. Protocol 1 is checked before any action;
independent release SHAs do not have to match. Keep the protocol and behavior
contract in [docs/remote-control.md](docs/remote-control.md).

Use at most four local Cargo jobs and keep debug builds warm in
`D:\build\shows-remote\cargo`. No optimized local builds on the gaming PC.
`scripts/dev.ps1 test` runs portable client/agent tests;
`scripts/dev.ps1 run upstairspc status --json` tries the debug client against the
installed SSH agent. `--local` exercises the new agent against a local player.

CI validates Windows and Linux clients; main builds a signed Windows release
without compiling the desktop or fetching libmpv. Publish and install that exact
release with `scripts/publish.ps1 -WaitForRelease -InstallTargets local,upstairspc,treadmill`.
Stable accepts only signed CI artifacts. The separate NAS channel is
`\\NelsonNAS\files\shows-remote-app`; installation atomically switches a native
executable hard link in a real channel directory (no junction on the SSH path).
There is no runtime PowerShell wrapper. Preserve the command, at-menu entry, fzt
palette, info panel, playback state, visible failures, bounded requests, child
cleanup and the bounded, secret-free app log ([contract L1–L5](docs/remote-control.md)). The info panel's repo, link, version and build time come only from CI's
build stamp ([contract C7](docs/remote-control.md)).

The approved home Defender exclusion is limited to the current user's
`%LOCALAPPDATA%\shows-remote` folder. The signed `scripts/configure-defender.ps1`
checks it by default; explicit elevated Apply/Remove changes only that exact
entry. Keep normal installs free of implicit antivirus policy changes, preserve
other exclusions/protection features and require signer/hash verification.
Channel schema 2 transports an executable-only ZIP; copy it into the local app
folder before extraction and verify archive metadata, the exact entry and the
signed executable. Publish downloads/packages inside that app folder too. Do not
restore unpacked NAS executables or add NAS/source/build-folder exclusions.
