# Local testing and updating the running app

The app is Rust, compiled to one native executable. ratatui and crossterm draw the
controller; the instance picker and the `/` command palette are the shared fzt
executable home-sync installs. PowerShell below is development tooling only.

## Build and automated checks

From the feature worktree, at most four Cargo jobs (this is a gaming PC):

```powershell
.\scripts\dev.ps1 test
cargo build --locked -j 4 --target-dir D:\build\shows-remote\cargo --bins --examples
```

Check each exit code. CI runs the same tests on Linux and Windows, signs the Windows
build on every pull request, and on main builds, stamps, signs and releases it.

| Tests | Behavior covered |
| --- | --- |
| `tests/cli.rs` | One-shot commands against an in-test player: JSON output, rejected controls, fullscreen, ambiguous show names sending nothing, no implicit target, no interactive mode without a terminal, sign-in before anything goes to another PC, what the log records, an unwritable log |
| `src/ui.rs` | Palette actions, controls disabled while disconnected or waiting, the video view and its failures, the info panel, small resizes |
| `src/info.rs` | Stamped and unstamped info panels, Pacific time and elapsed wording, checkout and path failures |
| `src/log.rs` | Line format, secret scrubbing, rolling at 4 MiB, concurrent writers |
| `src/discovery.rs`, `src/picker.rs` | Inventory parsing, picker rows that target text can't forge |
| `src/transport.rs`, `src/token.rs` | Reply logging without bodies, token renewal |

Add a focused test when interaction, protocol or logging behavior changes.

## Isolated terminal check

Never point a test at one of Nelson's players: a key in the controller changes what is
on his TV. `examples/fake_player.rs` is a stand-in player on loopback with a round of
three fake shows; it applies controls to its own state, pushes them on its status
stream, prints each control it receives, and answers preview with a visible failure.
An inventory naming only this PC makes the picker use it, and `SHOWS_REMOTE_LOG`
keeps test lines out of the real log, which is shipped to the NAS.

```powershell
$qa = Join-Path $env:TEMP 'shows-remote-qa'
New-Item -ItemType Directory -Force $qa, "$qa\session-set" | Out-Null
@{ machines = @{ thispc = @{ computer = $env:COMPUTERNAME } } } | ConvertTo-Json -Depth 3 |
    Set-Content "$qa\inventory.json"
$player = Start-Process -PassThru -WindowStyle Hidden -RedirectStandardOutput "$qa\player.out" `
    D:\build\shows-remote\cargo\debug\examples\fake_player.exe 47911
$exe = 'D:\build\shows-remote\cargo\debug\shows-remote.exe'
tmux-throwaway start srqa -- -d -s system -x 120 -y 35
tmux -L srqa set-environment -g SHOWS_CONTROL_PORT 47911
tmux -L srqa set-environment -g SHOWS_REMOTE_LOG "$qa\remote.log"
tmux -L srqa set-environment -g SHOWS_REMOTE_INVENTORY "$qa\inventory.json"
tmux -L srqa set -g '@session-set-dir' ("$qa\session-set" -replace '\\', '/')
tmux -L srqa new-window -d -M shows-remote -t system: -o remain-on-exit=relaunch $exe
tmux -L srqa list-windows -t system -F '#{window_index} #{window_name} #{@shows-remote-window} #{remain-on-exit}'
tmux -L srqa capture-pane -p -t system:shows-remote
```

Shows remote is a system-session app: the at menu's entry is
`system:shows-remote tool:shows-remote`, which fzt-automate runs as the `new-window -M`
line above (D:
epost\README.md, `system:`), with no arguments. The test's variables go
in the server's environment, not `-e`: a `reopen-window` rerun drops `-e` variables and
would reach his real inventory. To check a release instead of a development build, set
`$exe` to the managed program `tool:shows-remote` names, `C:\Program Files\at-home\shows-remote\current\shows-remote.exe`; never copy it
anywhere. Keys go to whatever program has the pane, so capture before every key, and
read `capture-pane -e` to see which row fzt has highlighted before Enter.

Check what the change affects:

- **Picker:** this PC listed with the fake show and paused; `Down` puts the cursor on
  it, typing highlights the best match, Enter connects. With the fake player stopped,
  the picker's title says no instance is running and the row reads `Unavailable: Open
  Shows on this PC`; Refresh instances checks again.
- **Controller:** the title reads `Shows Remote → this PC`, the round and the key line
  show, and the window title is `Shows remote` (`tmux -L srqa display -p -t system:shows-remote
  '#{pane_title}'`; the window itself is named `shows-remote` after its `system:` mark). Space, `n`, `b`, arrows and `+`/`-` show their result from the
  player's status, and `player.out` lists each control. `v` shows the preview failure
  without disabling controls. Stopping the fake player shows Disconnected; `r` after
  restarting it reconnects.
- **Palette:** `/` opens it with every shortcut's command; type `info`, Enter. Escape
  clears a query, then closes the palette without running anything. A `/` inside the
  palette opens fzt's own key help; Escape steps back out.
- **Info:** Repo, Link, Version, Built, Installed, Checkout and Log. A local build reads
  `development build, not from CI` on the stamped lines; a release shows
  `remote-<sha>` and the versioned path. Any key closes it.
- **Resize:** `tmux -L srqa resize-window -t system:shows-remote -x 40 -y 12`, then back.
- **Selection:** the app requests no mouse reporting; drag across text in a real
  at-terminal and paste it. A tmux capture does not exercise selection.
- **Log:** `$qa\remote.log` has `instance-chosen`, `connect`, `send` and `reply` lines,
  each starting with an RFC 3339 UTC time.
- **System window:** the window is marked `@shows-remote-window` 1 with
  `remain-on-exit` `relaunch`; running the `new-window -M` line again leaves one window
  and one process.
- **Quit:** `q` ends the app and no `shows-remote.exe` remains; the window stays with
  `shows-remote stopped (exit 0). Press Enter to relaunch.`, and Enter starts it again.

### Restore and reopen-window

A restore and `reopen-window` rerun the pane's start command in its folder. The
throwaway has its own session-set folder (above), so it records closed windows:

```powershell
tmux -L srqa kill-window -t system:shows-remote
Get-Content "$qa\session-set\closed-windows"
tmux -L srqa reopen-window -d
tmux -L srqa list-windows -t system -F '#{window_index} #{window_name} #{@shows-remote-window} #{remain-on-exit}'
tmux -L srqa capture-pane -p -t system:shows-remote
```

`closed-windows` holds the app's line, its process is gone, and the reopened window is
named `shows-remote`, still marked `@shows-remote-window` 1 with `remain-on-exit`
`relaunch`, and shows the picker. Choose this PC and send Space: the result
must come back from the fake player's status. The app keeps no target between runs, so
a restore always returns to the picker.

### Cleanup

`tmux-throwaway end srqa`, `Stop-Process -Id $player.Id`, then remove the
`shows-remote-qa` folder under `%TEMP%` by its full path. Never end the `at` server or
every `shows-remote.exe`.

## Previewing a change for Nelson

A change to what he sees is shown to him before it ships, in his own window: run the
test build on the throwaway server from the isolated check above, then put it in the
bottom half of his pane with `tmux-throwaway show <server>` (its `-t` names a session
on that server, not his pane). Before telling him what it shows, read what he
will see: the shown pane is alive (`tmux display -p -t <shown pane> '#{pane_dead}'`
prints `0`), `tmux -L <server> capture-pane -p` shows the case you're describing, and
every pane of your own window on `at` is captured and read. The turn his answer
settles it, end the server with `tmux-throwaway end <server>` and close the shown pane
if it stays (AHT-114).

## Updating the window Nelson has open

A merge's release is installed on every PC by home-sync, but the remote already open
in his window keeps running the old program. A behavior change is finished only when
that window runs the new release, without him closing or reopening it.
Documentation-only changes need no restart.

1. Confirm the install: `home-sync tools` lists `shows-remote` at the new
   `remote-<sha>`, and on the managed program `--version` prints that commit and
   `Get-AuthenticodeSignature` is `Valid`, `CN=at-terminal`.
2. Find his pane and its process. Match `pane_pid` to the process or its parent; never
   reuse a pane ID from an earlier session.

   ```powershell
   tmux list-panes -a -F '#{pane_id}|#{pane_current_command}|#{pane_pid}|#{pane_start_command}'
   Get-CimInstance Win32_Process -Filter "Name='shows-remote.exe'" |
       Select-Object ProcessId, ParentProcessId, ExecutablePath, CreationDate
   ```

3. Capture it (`tmux capture-pane -e -p -t <pane>`) and note his place: the picker or
   the controller, which PC (`Shows Remote → <name>`), and whether the video view is on.
   If the palette, a search or the info panel is open, he is in the middle of
   something: leave the pane until it is closed.
4. Replace only that pane, with the program and no arguments, as the menu starts it,
   so a later restore still opens the picker. Give the path as its own argument;
   one string goes through a shell, which splits it at the space.

   ```powershell
   tmux respawn-pane -k -t <pane> 'C:\Program Files\at-home\shows-remote\current\shows-remote.exe'
   ```

   Ending the remote never touches the player, so the TV keeps playing.
5. Put him back: on the picker, `Down`, type the PC's name, confirm the highlighted row
   with `capture-pane -e`, Enter; then `v` if the video view was on. Send no playback
   key: those change his TV.
6. Read it back: the process's `ExecutablePath` and `CreationDate` are the new program,
   started after the install, and the capture shows the controller on the same PC and
   view.

Use the live `at` server only for these steps and reading back his window: never for
QA keys or the fake player.
