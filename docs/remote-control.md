# Terminal remote control

`shows-remote` is an independently released console executable. Its
interactive UI and one-shot commands control an already running Shows instance.
The companion never initializes libmpv, opens SQLite, starts a desktop, or owns a
round. The target owns playback and all authoritative NAS transactions.

## Transport and identity

- **T0. Choose an instance.** Opening the app without a target checks the shared
  home machine inventory and displays an fzt instance picker. It includes this
  PC, explains unavailable computers, supports refresh, and never starts a player.
  Explicit CLI targets skip discovery; one-shot commands never silently choose
  a computer. The at-menu entry launches the generic app without a target.

- **T1. HTTP stays local.** The desktop listens on `127.0.0.1:45831`.
  `SHOWS_CONTROL_PORT` may select another nonzero port for isolated tests or a
  configured installation; desktop and agent must agree. The native client
  rejects non-loopback HTTP addresses and does not follow redirects.
- **T2. Existing SSH access.** Windows OpenSSH uses batch authentication, strict
  known-host verification, connect timeout and keepalives. Targets are computer
  hostnames or SSH aliases, containing letters, digits, dots and hyphens, starting
  with a letter or digit. Shows issues no credentials.
- **T3. Commands are data.** The validated channel selects the independently
  installed native `%LOCALAPPDATA%\shows-remote\channels\<channel>\shows-remote.exe --agent`
  entry point. Commands travel as bounded JSON lines on SSH stdin, never
  interpolated shell text. SSH starts the binary without a batch entry point,
  PowerShell pipeline or NAS access, preserving protocol stdin/stdout and Unicode.
- **T4. Identity precedes controls.** `GET /remote/info` must identify
  `shows-desktop` and protocol 1. A missing, stopped, or different listener fails
  explicitly. Every new connection checks identity.
- **T5. Narrow native surface.** `POST /remote/control` accepts typed playback
  commands and fullscreen toggle, requires JSON, and rejects browser Origin
  headers. It exposes no library import, filesystem browsing, shell execution,
  or other window actions.

## Playback and acknowledgement

- **C0. Discoverable actions.** `/` opens the fzt command palette, advertised on
  screen. It provides playback and current-round show selection when controls
  are available, plus reconnect, change instance, info, and quit. Cancel executes
  nothing. Switching closes the previous agent, leaves its player unchanged,
  and returns to instance selection. Status queued while the palette is open
  is consumed before another playback command is accepted.
  **Toggle fullscreen** (`f`) remains available while connected and idle even
  without a loaded episode. It acts only on the controlled desktop window.
  Success acknowledges posting the window's toggle message; the authoritative
  `window_fullscreen` SSE field confirms the resulting state. Missing windows
  or failed UI-message posting return errors; toggles are never replayed.

- **C1. Existing round semantics.** Skip, previous, defer and show navigation use
  the overlay's runner. [round-and-advance.md](https://forgejo.romaine.life/nelsong6/shows/src/branch/main/docs/feature-contracts/round-and-advance.md) invariants
  I7, D1–D3, P1–P2 and M1–M3 remain unchanged. Name selection resolves uniquely
  within the target's current round. Ambiguous or absent names issue no play.
- **C2. Explicit play/pause.** They set a state, never toggle implicitly. The
  target checks mpv's result and observed pause/volume values before success.
  Seek reports mpv command acceptance; navigation reports the runner result.
  Subsequent live status establishes which episode loaded. No loaded episode
  means failure, never a successful no-op.
- **C3. No automatic command replay.** Requests have a bounded response wait.
  A lost acknowledgement means the outcome may be unknown; inspect status before
  repeating a command, especially skip, defer, previous and relative seek.
- **C4. Target-owned live state.** The interactive UI uses `/status/stream`, not
  snapshot polling. Playback fields update only on target snapshots. Target
  alerts, waits, request failures and disconnects remain visible.
- **C5. Disconnect disables controls.** Initial replies and commands time out
  after 12 seconds. SSH keepalives detect broken peers. No status or SSE heartbeat
  activity for 65 seconds disconnects the UI; idle SSE comments arrive every 30
  seconds. `r` explicitly reconnects with a fresh agent and replays no command.
  Quitting tears down the owned SSH child and leaves TV playback running.

- **C6. Opt-in video view.** `v` and the fzt palette switch between the round
  list and an ASCII video preview. The target alone reads its already-rendered
  GPU video surface, downscaling to 160×90 before CPU readback. No extra decoder,
  filesystem capture, NAS read or playback mutation is introduced. The native
  loopback-only `GET /remote/preview` route rejects browser Origin headers;
  the SSH agent carries a bounded `preview` read through the existing protocol.
  Successful captures are shared across clients for 250 ms, and concurrent
  capture requests fail explicitly instead of queuing. The UI has at most one
  read outstanding, requests no more than four per second, backs off on errors,
  and stops requesting when the view is off or the palette owns the terminal.
  The UI hides frames after two seconds, reports loading/unavailable/disconnected
  states, and keeps playback results separate from preview results. Hidden or
  minimized target windows report unavailability instead of stale video. All
  waits for a window reply time out after 500 ms; no request is
  replayed as a playback action. Terminal sizing preserves the source surface's
  aspect ratio and validates dimensions and printable ASCII before rendering.
  Preview rows preserve the displayed video's top-to-bottom and left-to-right
  orientation. The D3D-shared surface is already in display row order; ASCII
  conversion must not reverse it.

- **C7. Info panel.** The palette's **info** action is always available and
  replaces the screen until any key, which only closes it. It lists Repo, Link,
  Version (the `remote-<sha>` release tag), Built (Pacific time and plain elapsed
  language), Installed (`current_exe`), Checkout (`D:\repos\<repo-name>` when
  that folder has `.git`, else "no checkout on this PC") and Log (the app log's
  path, and why it isn't writing if it isn't). CI stamps repo, link,
  commit and build time at compile time from `GITHUB_REPOSITORY`,
  `GITHUB_SERVER_URL`, `SHOWS_BUILD_SHA` and `SHOWS_BUILD_TIME`; none is typed into
  source. Each unstamped value reads "development build, not from CI", and
  unreadable values are stated. The text is borderless and fixed once opened, and
  the app requests no mouse reporting, so the terminal's normal selection gesture
  works while status arrives. Live status stays consumed behind the panel; preview
  reads stop, as they do for the palette.

## Logging

- **L1. One file, in the pile.** Client and agent append to
  `%LOCALAPPDATA%\shows-remote\shows-remote.log` on their own PC, and log-ship
  sends it to the pile (`log-search --app shows-remote`). A line starts with UTC
  time to the millisecond, then level, `client` or `agent`, `pid=`, the event and
  `name=value` fields, so it reads by time beside that PC's `shows.log`. A remote
  session therefore has two halves: the client's on the PC you sat at, the agent's
  on the target. `child=` on a `--local` connect is the agent's `pid=`.
- **L2. What a failure leaves.** The client logs each instance it probed and why it
  was unavailable (`instance-available`, `instance-unavailable`), the connection
  (`connect`, `connect-failed`, `connection-lost` with SSH's stderr), each command
  (`send`, then `reply`, `rejected` or `timeout`, by `id`, with milliseconds),
  `closed` with how many requests went unanswered, and `disconnected` with the
  reason the UI dropped. A key that sent nothing is `not-sent`, with why. `status`
  logs playback state only when it changes, clock excluded, so a command that did
  nothing shows as a `reply` with no `status` after it. The agent logs
  `player-found` or `player-unavailable`, each `request` and its `reply` or
  `rejected`, and `watch-ended`.
- **L3. Bounded.** No line per status or heartbeat. Preview reads log only when
  their outcome changes. A value stops at 1,024 characters. Past 4 MiB the file
  rolls to `shows-remote.log.old`, replacing the last one; the listed name never
  matches the old file, so the pile gets no line twice. Lines log-ship hadn't sent
  when the file rolled are on this PC only.
- **L4. Nothing secret.** A reply's body, the round, preview frames and SSH protocol
  lines are never logged, only the summary fields above. The agent holds no keys;
  it is a relay for bounded JSON commands. Every value is still treated as outside
  text: private key blocks, long opaque strings, `name=value` secrets, URL
  credentials and query strings are removed, and control characters can't start a
  new line.
- **L5. Never in the way.** Nothing goes to stdout or stderr: those belong to the
  full-screen UI and the SSH protocol. A write that fails changes nothing about the
  session and shows on the info panel's Log row. `SHOWS_REMOTE_LOG` names another
  file for tests and isolated runs.

## Distribution and evidence

The executable has its own immutable, signed and hash-verified channel under
`shows-remote-app`. The installer exposes its native binary through a local
hard link in a real directory and adds it to PATH. The SSH path contains no
directory junction. It never installs or restarts a player.
The approved home Defender exception covers only the current user's remote
installation folder, including staging and future versions. The separately
invoked signed setup helper adds, checks or removes that exact folder entry;
normal updates do not change policy. Unrelated exclusions and protection features
remain unchanged, and signature/hash verification remains mandatory.
Channel schema 2 transfers a bounded, hash-verified executable-only ZIP from the
NAS and extracts it inside the local app folder before signer/hash checks. No raw
EXE is opened on the share; no NAS or publisher-temp-directory exclusion is needed.
The at-menu launches that binary directly. Protocol 1 is defined by the
desktop-owned `shows-control` crate, pinned by immutable Git revision in this
repository; it requires matching protocol support, not matching release SHAs.

The desktop's CI covers round/SQLite invariants, HTTP endpoints and the shared
crate's SSE parsing. This CI covers CLI-to-agent subprocesses, terminal
render/state behavior and independent channel installation. Cross-computer checks
use released executables and restore playback state after play/pause checks.
