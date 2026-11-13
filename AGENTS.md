# shows-remote

Independent Windows terminal client for Shows. The player lives in
`nelsong6/shows`; it owns playback, the NAS SQLite database, video capture and the
versioned HTTP/SSE protocol. This app never starts a player or reads that database.

`src/` owns the CLI, terminal UI, fzt discovery/palette, and the direct HTTP transport.
Another PC's player is reached on its network port with a token OpenBao signed
(`src/token.rs`, through `romaine-auth home-token`). The target PC needs no remote
installed.

`shows-control` is the desktop-owned protocol crate, pinned by an immutable Git
revision in Cargo.toml and Cargo.lock. Protocol 1 is checked before any action;
independent release SHAs do not have to match. Keep the protocol and behavior
contract in [docs/remote-control.md](docs/remote-control.md).

Use at most four local Cargo jobs and keep debug builds warm in
`D:\build\shows-remote\cargo`. No optimized local builds on the gaming PC.
`scripts/dev.ps1 test` runs portable client tests;
`scripts/dev.ps1 run upstairspc status --json` tries the debug client against that PC's
player. `--local` exercises it against a local player.

CI validates Windows and Linux clients; main builds, stamps and signs the Windows
release without compiling the desktop or fetching libmpv, drafts `remote-<sha>` with
`shows-remote-windows-amd64.exe`, and pipeline-templates' home-sync workflow publishes it
and starts home-sync on every PC. home-sync installs it under
`C:\Program Files\at-home\shows-remote\<tag>` behind the `current` junction, after
checking the at-terminal signature; the at menu's `system:shows-remote tool:shows-remote` runs it
in one window in the `system` session. The
retired per-user channel installer, NAS channel and its `%LOCALAPPDATA%` Defender rule
stay retired: home-sync's `C:\Program Files\at-home` exclusion covers this app as it
does every managed tool (HOMECFG-130). Never install a local build as a release.
There is no runtime PowerShell wrapper. Preserve the command, at-menu entry, fzt
palette, info panel, playback state, visible failures, bounded requests, child
cleanup and the bounded, secret-free app log ([contract L1–L5](docs/remote-control.md)). The info panel's repo, link, version and build time come only from CI's
build stamp ([contract C7](docs/remote-control.md)).

Read [TESTING.md](TESTING.md) before verifying a change or updating the remote Nelson
has open. Never point a test at one of his players: use the fake player and isolated
server it describes. A behavior change is finished when his open window runs the new
release.
