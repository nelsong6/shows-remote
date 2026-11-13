# Shows Remote

Control Shows on the other TV from a terminal. The target must already have
Shows open; the remote never starts a local player. The menu invokes its installed
binary directly and the app sets its own window title to **Shows remote**.

```powershell
shows-remote
```

Open **apps → shows remote** in the at menu, or run the command above. The fzt
picker checks the computers in the shared home inventory, lists available Shows
instances first, and explains unavailable PCs. Select an instance to control it;
**Refresh instances** checks again. No player starts as a side effect.

Press `/` in the controller for its fzt command palette: playback controls,
**Toggle fullscreen** on the controlled TV (`f` is the shortcut),
play a show from the current round, reconnect, change instance, **info**, or quit.
Arrows select a show; Enter plays it. Space sets play/pause, `s` plays, `p` pauses,
`n` skips **and marks watched**, `b` goes back, `d` defers without marking watched,
left/right seek 10 seconds, and `+`/`-` change volume. `q` leaves Shows running;
`r` reconnects after a disconnect.

**info** shows this build's repo and link, its release and when it was built, the
running executable's path, and its checkout under `D:\repos` if this PC has one.
It also names the log, and says so if the log isn't being written.
Drag to select text as usual; any key closes the panel. A local build says
"development build, not from CI" for the values only CI can stamp.

Press **v** or choose **Show ASCII video** in the palette to replace the round
list with a live monochrome rendering of the target's video. Press **v** again
or choose **Show playlist** to return. Playback controls and connection messages
remain visible; choosing a show is still available through the palette.

The visualizer samples the existing GPU video surface, downscales to 160×90
before readback, and transfers at most four frames per second (about 60 KB/s
before transport overhead). It starts no decoder and saves no images. Turning it off,
opening the palette or info panel, changing instance or quitting stops requests. Only one
preview request is outstanding; failures back off and remain visible without
disabling playback controls. Frames older than two seconds are hidden. A
minimized or fully covered target window reports that its video is unavailable
rather than showing the stale surface from before it was hidden.

Every session logs what it tried, sent and got back to
`%LOCALAPPDATA%\shows-remote\shows-remote.log`. Read it from any PC with
`log-search <words> --app shows-remote`; the target's half is under `--pc <target>`.

```powershell
shows-remote upstairspc status
shows-remote upstairspc queue
shows-remote upstairspc play
shows-remote upstairspc pause
shows-remote upstairspc fullscreen
shows-remote upstairspc preview
shows-remote upstairspc play "The Simpsons"
shows-remote upstairspc seek -30
shows-remote upstairspc volume 50
shows-remote upstairspc status --json
```

An explicit target skips the picker. One-shot commands require a target or
`--local`; there is no default computer. `--local` controls this PC;
`--channel dev` selects the target's installed dev channel. Failures exit nonzero
and print to stderr. The interactive remote names the target, shows its alerts
and disables controls while disconnected or waiting for a reply. A timed-out
command may already have applied; inspect status before retrying.

## Installation

home-sync installs each signed release from main on every home PC, at
`C:\Program Files\at-home\shows-remote\current\shows-remote.exe`, and puts that
folder on PATH. The at menu runs it as `system:shows-remote tool:shows-remote`, one window in the
`system` session that relaunches on exit, directly, with no
PowerShell or batch wrapper. A release reaches every PC without anyone running an
installer; the remote does not check for updates on every opening. Defender skips
that folder through home-sync's exclusion for every managed tool, and home-sync
refuses a release not signed by the at-terminal certificate.

The instance picker and command palette use the managed fzt executable at
`%ProgramFiles%\at-home\fzt\current\fzt.exe`, installed by home-sync. A missing
fzt installation explains how to install it and fails explicitly.

Discovery reads `\\NelsonNAS\files\shell-config-profile-home\machine\state.json`.
`SHOWS_REMOTE_INVENTORY` can select another inventory file with the same
`machines` shape for an isolated test or another installation. Names come from
that inventory, never a hardcoded list. This PC connects over loopback; other
PCs connect to the player's network port, 45832. Discovery probes concurrently and
uses the normal bounded status request.

Each home PC signs in with its own key: `romaine-auth home-token` logs in to OpenBao and
saves a five-minute token, which an open remote replaces before it runs out. A PC needs a
login in `nelsong6/openbao` with the `shows-token` policy. home-sync opens the port to
the local subnet. Traffic is plain HTTP on the home network. The loopback port is 45831;
leave `SHOWS_CONTROL_PORT` and `SHOWS_REMOTE_PORT` unset normally.

Release target requests log to `%APPDATA%\shows\shows.log`. The remote stores no
library, history, playback state or secrets.

## Home-service checklist applicability

- Tickets, source, CI and releases belong to the independent shows-remote project.
- Installation is home-sync's signed managed-tool release, with no desktop bundle.
- Sign-in is the PC's OpenBao login. No new hosted service, browser login, identity
  issuer, secret store, DNS record, TLS listener or homepage URL is introduced.
- No new durable business state needs backup; the NAS database stays authoritative.
- The app is on demand. Connection state and exit status report unavailability;
  an always-on down alert would flag a normally closed TV app as a fault.

See the [remote-control contract](remote-control.md).
