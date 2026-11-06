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
.\scripts\dev.ps1 ship
```

Debug Cargo output stays warm in `D:\build\shows-remote\cargo` with at most four
jobs. Protocol changes are checked against the desktop's `/remote/info`
before controls. The desktop-owned `shows-control` crate is pinned by immutable
Git revision; its GitHub mirror makes source builds possible away from the home
network. Client and player release SHAs can differ while both implement protocol 1.

Windows and Linux CI run the client tests. Windows CI also checks installation,
hash/signature rejection and version switching. Only main compiles an optimized
Windows client, signs the executable and installer scripts, and publishes
`remote-<sha>` with `shows-remote-windows-amd64.zip`. That build stamps the repo,
link, commit and build time the palette's **info** panel shows.

`scripts/publish.ps1 -WaitForRelease -InstallTargets local,upstairspc,treadmill`
publishes that exact signed artifact to `\\NelsonNAS\files\shows-remote-app` and
installs it in each interactive user session. Installation verifies hashes and
the home Authenticode identity before atomically switching a native executable
hard link in a real channel directory;
Rejected binaries leave the selected client unchanged. It never restarts the player.
It adds no service, credential store or durable playback state.

Home PCs use a persistent Defender exclusion limited to the current user's
`%LOCALAPPDATA%\shows-remote` installation folder, approved because real-time
protection quarantined the independently signed client. It covers staging and
future versions without per-build submissions. Defender skips everything in that
folder; the exclusion itself does not enforce signatures. The installer still
requires the pinned signer and published SHA-256. No process, extension, source,
build-directory or other-app exclusions are added.

The release includes signed `configure-defender.ps1`. Run it elevated as the
interactive user with `-Action Apply`, `-Action Check` (the default), or
`-Action Remove` to reverse the rule. The helper accepts no path override, changes
only this app's exact folder entry, preserves unrelated exclusions and reports
Defender's protection state. Installation and updates never change antivirus
policy silently; the one-time folder rule persists independently of releases.

Channel schema 2 stores a hash-verified ZIP containing only the executable on
the NAS. Publishing downloads and verifies CI assets inside the app folder;
installation copies the ZIP there before extracting and verifying the signed
executable. This avoids opening a flagged raw EXE on the share and needs no NAS
exclusion. Archive size and contents are bounded and checked before extraction.
The retired unpacked channel format is not accepted.

## Source history

The client was extracted from `nelsong6/shows` at
`8fe9064e031e6f0893c9de134b972776c46a5eea`. The permanent
[`history/from-shows`](https://forgejo.romaine.life/nelsong6/shows-remote/src/branch/history/from-shows)
branch preserves the subtree's authors, commits and file history; main follows
the normal squash-merged PR workflow. The desktop no longer carries a client copy.
