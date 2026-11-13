# shows-remote

Control an already running Shows player from a terminal. This repository owns
the terminal UI, fzt instance picker and palette, CLI, builds and releases. The [Shows desktop](https://forgejo.romaine.life/nelsong6/shows) owns
playback, video capture and the authoritative NAS database.

Run `shows-remote`, or **apps → shows remote** in the at menu. `/` opens commands;
`v` switches between the playlist and live ASCII video. Quitting leaves the TV
running. See [usage](docs/usage.md) and the [client contract](docs/remote-control.md).

It reaches another PC's player directly on port 45832, signing in with a token OpenBao
signs for this PC, so the target needs only the player. No player, libmpv, database,
shell wrapper or NAS media is part of this package.

## Build and release

```powershell
.\scripts\dev.ps1 test
.\scripts\dev.ps1 run upstairspc status --json
.\scripts\dev.ps1 run upstairspc
```

Debug Cargo output stays warm in `D:\build\shows-remote\cargo` with at most four
jobs. Protocol changes are checked against the desktop's `/remote/info`
before controls. The desktop-owned `shows-control` crate is pinned by immutable
Git revision and read from Forgejo's `nelsong6/shows` with the git CLI
(`.cargo/config.toml`), signed in by git's credential helper here and by
`PIPELINE_DISPATCH_TOKEN` in CI (FORGEJO-29); Forgejo answers only on the home
network, so a source build needs it. Client and player release SHAs can differ while both implement protocol 1.

Windows and Linux CI run the client tests, and Windows CI signs every build. Only
main compiles an optimized Windows client, stamping the repo, link, commit and build
time the palette's **info** panel shows. It drafts `remote-<sha>` with the signed
`shows-remote-windows-amd64.exe`, and pipeline-templates' home-sync workflow
publishes it and starts home-sync on every home PC.

home-sync installs it like every managed home tool: `shows-remote` in
`machine/state.json`'s `tools`, checked against the at-terminal certificate, under
`C:\Program Files\at-home\shows-remote\<tag>` with a `current` junction that running
copies don't block. That folder is covered by home-sync's Defender exclusion
(HOMECFG-130), so the client needs no exclusion of its own. Installing never
restarts the player; an open remote is updated as [TESTING.md](TESTING.md) describes.
Roll back by pinning a tag in `state.json`. It adds no service, credential store or
durable playback state.

The earlier per-user channel installer, its NAS channel
(`\\NelsonNAS\files\shows-remote-app`) and its `%LOCALAPPDATA%\shows-remote` Defender
exclusion (SHREMOTE-3) are retired; the log stays in that folder.

## Source history

The client was extracted from `nelsong6/shows` at
`8fe9064e031e6f0893c9de134b972776c46a5eea`. The permanent
[`history/from-shows`](https://forgejo.romaine.life/nelsong6/shows-remote/src/branch/history/from-shows)
branch preserves the subtree's authors, commits and file history; main follows
the normal squash-merged PR workflow. The desktop no longer carries a client copy.
