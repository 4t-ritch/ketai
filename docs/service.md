**English** | [Espanol](es/service.md)

# The daemon service

`ketaid` is installed as a systemd user service so you never have to start
it by hand.

## Start modes

| Mode | How to enable it | When it starts |
|------|------------------|----------------|
| Socket activation (recommended) | `systemctl --user enable --now ketaid.socket` | When a client connects for the first time |
| Always on | `systemctl --user enable --now ketaid.service` | When the user session starts |
| Manual | `ketaid` | When it is run |

With socket activation, systemd creates the sockets when the session starts
and only starts the daemon when the TUI, waybar or another client connects.
It uses no resources while nobody uses it.

## Daemon contract

- **Inherited sockets**: if `LISTEN_FDS` exists, the daemon uses the sockets
  systemd passes it in the order of the `.socket` (first `ctl.sock`, then
  `viz.sock`). If it does not exist, it creates the sockets itself in
  `$XDG_RUNTIME_DIR/ketai/`. They are only taken if `LISTEN_PID` is the
  daemon's pid; otherwise they are ignored. Those sockets belong to systemd:
  the daemon does not delete them on exit.
- **Ready to use**: it sends `READY=1` through `sd_notify` once it accepts
  connections (`Type=notify`), and `STOPPING=1` when it starts shutting
  down. It is implemented without libsystemd: it is a text datagram to the
  Unix socket in `NOTIFY_SOCKET` (with a leading `@` it is an abstract
  socket).
- **systemd is the recommended way**: it supervises, restarts on failure,
  shuts down cleanly on logout (the session is always saved) and only starts
  the daemon when a client needs it. Manual mode is for development and for
  systems without systemd.
- **Manual mode yields to systemd**: if a manual `ketaid` finds a `ctl.sock`
  that accepts connections while no ketaid holds the lock, that socket
  belongs to `ketaid.socket`. It does not delete it: that connection makes
  systemd start the daemon, and the manual instance exits with code 0. A
  `ctl.sock` that does not accept connections is from an earlier run and is
  replaced.
- **Clients' start button**: if the unit is installed, it uses
  `systemctl --user start ketaid.socket`; only if it is not, it runs
  `ketaid` directly.
- **Single instance**: if `ctl.sock` already answers, a second manual
  instance exits with code 0 without opening another audio output.
- **Clean shutdown**: on `SIGTERM` or `SIGINT` it saves the session (see
  "Session persistence") and releases the audio output before exiting.
- **Restart on failure**: `Restart=on-failure` with a limit of 5 attempts
  per minute to avoid loops.

## Session persistence

The goal is that when ketai opens again, the music session is as it was: the
same queue, track, position and volume.

### What is saved

| Field | Example | Notes |
|-------|---------|-------|
| Queue | ordered list of URIs | Includes the current order even with `shuffle` on |
| Current index | `4` | Track that was playing or paused |
| Position | `83400` ms | Position within the current track |
| Volume | `0.65` | |
| Mute | `false` | |
| Repeat / shuffle | `"queue"` / `true` | |
| Audio output | `"alsa_output.usb-headset"` or `null` | `null` = follow the system default |
| State | `"playing"` / `"paused"` | Informational only, see "Restoring" |

### Where

`$XDG_STATE_HOME/ketai/session.json` (by default
`~/.local/state/ketai/session.json`). It is session state, not
configuration, which is why it does not go in `~/.config`.

The file has a `version` field so it can be migrated in the future.

### When it is saved

Saving only on shutdown is not enough: if the machine loses power or the
daemon hangs, `SIGTERM` never arrives. So it is saved at three moments:

1. **On shutdown** (`SIGTERM`/`SIGINT`): final save, with the exact
   position.
2. **When something changes**: queue, current track, volume, modes or
   output, and the position when not playing (for example a seek while
   paused). Consecutive changes are grouped and written at most once per
   second.
3. **Periodically while playing**: the position is saved every 10 seconds.
   In the worst case, after a power loss 10 seconds of position are lost.
   The `KETAI_SESSION_SAVE_INTERVAL_MS` variable changes this interval (used
   in the tests).

`daemon.shutdown` also does the final save. Signals are handled on their own
thread with `sigwait`, so the final save runs on the main loop like any other
message.

### Safe writing

So that a power loss halfway through a write never leaves the file broken:

1. Write to `session.json.tmp`.
2. `fsync` the temporary file.
3. Atomic `rename` to `session.json`.

If `session.json` cannot be used at startup, the error is logged and an
empty session starts. Two cases are told apart:

- **Read error** (permissions, disk): it is retried up to 5 times, every
  100 ms, in case it was temporary. If it keeps failing, it is renamed to
  `session.json.unreadable` without touching its contents, so the next save
  does not overwrite it and it can be recovered.
- **Invalid content** (broken JSON, unknown version): it is renamed to
  `session.json.bad`. Since saving is atomic, this only happens if someone
  edited or damaged the file.

### Restoring

- On startup the queue, current track, position, volume, modes and output
  are rebuilt.
- Playback **stays paused** at the saved position, even if it was playing at
  shutdown. That way audio never starts by itself when the session starts;
  the user resumes with `player.play`. If it was stopped (`stopped`) at
  shutdown, it stays stopped with the same track selected.
- Restoring reads the metadata off the main loop, so the daemon answers
  `daemon.hello` from the very first moment. Meanwhile, `player.*` and
  `queue.*` requests wait and run as soon as it finishes, so they always see
  the restored session.
- Nothing is saved while restoring, so the session being read is not
  overwritten.
- If the saved output no longer exists, the system default is used.
- If a file in the queue no longer exists, that track is removed from the
  queue. If it was the current track, the session moves to the next
  available track, at position `0`.
- Remote tracks are restored by URI; they are downloaded again or read from
  the cache when played.

## Clients

When a client (for example the TUI) cannot talk to the daemon, it does not
show commands to copy: it offers two functions.

### 1. Find the service

It diagnoses the daemon's state, in this order:

1. Connect to `ctl.sock` and send `daemon.hello`.
2. Ask systemd over D-Bus (`org.freedesktop.systemd1`, user bus) for the
   state of `ketaid.socket` and `ketaid.service`: whether they exist, are
   active and are enabled.
3. Look for a user `ketaid` process that is not under systemd.

Possible results:

| State | What it means | What the client offers |
|-------|---------------|------------------------|
| `connected` | The daemon answers | Nothing, it connects |
| `starting` | The socket or service is active but the daemon does not answer yet | Retry for a few seconds |
| `stopped` | The units are installed but inactive | A "Start" button |
| `not_installed` | There are no user systemd units | An "Install and start" button |
| `unresponsive` | There is a `ketaid` process but it does not answer on the socket | A "Restart" button (with confirmation) |
| `no_systemd` | No user systemd is available | A "Start" button in manual mode |

### 2. Start the service with a button

The client always starts the daemon **through systemd**, never as its own
child process. That way no instances run out of control.

- **Start**: `StartUnit("ketaid.socket")` over D-Bus. The socket starts the
  daemon on the first connection.
- **Always start on login**: an optional checkbox next to the button; it
  also calls `EnableUnitFiles` for `ketaid.socket`.
- **Install and start**: copies the units to `~/.config/systemd/user/`,
  calls `Reload` and then starts as above.
- **Restart**: `RestartUnit("ketaid.service")`. If the hung process is not
  under systemd, it is sent `SIGTERM` and, if it does not exit within 5
  seconds, `SIGKILL`; then it starts normally.
- **Manual mode** (no systemd): the client runs `setsid -f ketaid`, so the
  daemon is detached from the terminal and is not a child of the client. The
  daemon is still protected by the single-instance lock.

After any action, the client runs the check again and shows the result. If
it fails, it shows the error from systemd or from the daemon.

### Single instance

The daemon runs its own check on startup, in addition to the client's. The
arbiter is an exclusive lock (`flock`) on
`$XDG_RUNTIME_DIR/ketai/ketaid.lock`:

- Only one process can hold the lock at a time.
- The kernel releases it automatically if that process dies, even with
  `SIGKILL`.
- The orphan scan is done only by the lock owner. An instance that is not the
  owner can only close an owner that was ready and stopped answering (see the
  startup sequence). That way, two instances starting at the same time cannot
  kill each other.

#### Startup sequence

The lock file stores the owner's PID on the first line and, once the owner
serves `ctl.sock`, the word `ready` on the second.

1. **Try to take the lock** (without waiting).
2. **If the lock already has an owner:**
   1. Send `daemon.hello` to `ctl.sock` with a 2-second timeout.
   2. If it answers: an instance is already in use. The new instance logs
      the reason and **exits with code 0**, without opening audio.
   3. If it does not answer and the owner is **not** marked `ready`: the
      owner is starting. It is not touched; wait and go back to step 1. If
      it is still not ready after 10 seconds, the new instance exits with an
      error.
   4. If it does not answer and the owner is marked `ready`: it is hung. Its
      PID is validated (see "Process validation"), it is sent `SIGTERM` and,
      if it is still alive after 5 seconds, `SIGKILL`, then up to 2 seconds
      are waited for it to exit (the kernel only releases the lock once it
      exits). Then go back to step 1.
   5. If the owner is marked `ready` but its PID can no longer be validated
      (for example, because it is finishing dying), nothing is sent: wait
      and go back to step 1 without counting it as an attempt.
   6. After 3 hung owners closed without getting the lock, exit with an
      error.
3. **If the lock is taken:** this instance is the owner.
   1. Write its PID to the lock file (without `ready`).
   2. Open `ctl.sock`.
   3. Mark `ready` in the lock file.
   4. **Scan** for orphan `ketaid` processes (see below) and close them, in
      the background: the daemon already serves clients meanwhile.
   5. Continue the normal startup: session, audio.

The only signal an instance that is not the owner sends is against an owner
marked `ready` that stopped answering. An owner that is starting never
receives signals.

#### Orphan scan

Done only by the lock-owning instance. An orphan is any `ketaid` process of
the same user that is not the owning instance; for example, leftovers of a
crash or of an old version.

1. Walk `/proc` and select the processes that pass the validation.
2. Send `SIGTERM` to each one.
3. Wait up to 5 seconds; to those still alive, send `SIGKILL`.
4. Log the PID, the executable and the signal used for each closed process.

#### Process validation

Before sending any signal, the process must meet all of this:

- It belongs to the same user (real UID) as the daemon.
- `/proc/<pid>/exe` points to a `ketaid` binary. The process name is not
  enough, since it can be faked.
- It is not the process itself.
- It belongs to the same session: its `XDG_RUNTIME_DIR` (read from
  `/proc/<pid>/environ`) equals the daemon's. A `ketaid` from another session
  is not an orphan of this one.
- Right before sending the signal, it checks again that the PID is still the
  same process (same start time in `/proc/<pid>/stat`), because a PID can be
  reused.

If any validation fails, the process is not touched.

#### Result

- If there is a healthy instance, that one stays and the new one closes
  itself.
- If there is none, the one starting takes the lock and is the one that
  stays.
- Either way, once startup ends there is at most one `ketaid` per session
  (`XDG_RUNTIME_DIR`).

## Files

- `dist/systemd/ketaid.socket`
- `dist/systemd/ketaid.service`

Manual install:

```sh
install -Dm644 dist/systemd/ketaid.{socket,service} -t ~/.config/systemd/user/
systemctl --user daemon-reload
systemctl --user enable --now ketaid.socket
```
