# shows-remote

Control an already running Shows player from a terminal. This repository owns
the terminal UI, fzt instance picker and palette, CLI, SSH agent, builds and
releases. The [Shows desktop](https://forgejo.romaine.life/nelsong6/shows) owns
playback, video capture and the authoritative NAS database.

Run `shows-remote`, or **apps → shows remote** in the at menu. `/` opens commands;
`v` switches between the playlist and live ASCII video. Quitting leaves the TV
running. See [usage](docs/usage.md) and the [client contract](docs/remote-control.md).

The target also needs the independent remote installed: SSH invokes its native
`%LOCALAPPDATA%\shows-remote\channels\stable\shows-remote.exe --agent` entry point.
No player, libmpv, database, shell wrapper or NAS media is part of this package.

## Build and release

```powershell
.\scripts\dev.ps1 test
.\scripts\dev.ps1 run upstairspc status --json
.\scripts\dev.ps1 run upstairspc
.\scripts\dev.ps1 ship
```

Debug Cargo output stays warm in `D:\build\shows-remote\cargo` with at most four
jobs. `--local` uses the freshly built agent; SSH uses the independently installed
target agent. Protocol changes are checked against the desktop's `/remote/info`
before controls. The desktop-owned `shows-control` crate is pinned by immutable
Git revision; its GitHub mirror makes source builds possible away from the home
network. Client and player release SHAs can differ while both implement protocol 1.

Windows and Linux CI run the client tests. Windows CI also checks installation,
hash/signature rejection and version switching. Only main compiles an optimized
Windows client, signs the executable and installer scripts, and publishes
`remote-<sha>` with `shows-remote-windows-amd64.zip`.

`scripts/publish.ps1 -WaitForRelease -InstallTargets local,upstairspc,treadmill`
publishes that exact signed artifact to `\\NelsonNAS\files\shows-remote-app` and
installs it in each interactive user session. Installation verifies hashes and
the home Authenticode identity before switching the native version junction;
errors leave the selected client unchanged. It never restarts the player.
There is no new service, identity, credential store or durable playback state.

## Source history

The client was extracted from `nelsong6/shows` at
`8fe9064e031e6f0893c9de134b972776c46a5eea`. The permanent
[`history/from-shows`](https://forgejo.romaine.life/nelsong6/shows-remote/src/branch/history/from-shows)
branch preserves the subtree's authors, commits and file history; main follows
the normal squash-merged PR workflow. The desktop no longer carries a client copy.
