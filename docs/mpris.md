**English** | [Espanol](es/mpris.md)

# MPRIS 2

The daemon publishes itself on the D-Bus session bus as
`org.mpris.MediaPlayer2.ketai`, at the object `/org/mpris/MediaPlayer2`. That
way playerctl, waybar's `mpris` modules and the desktop's media keys control
it without speaking ketai's protocol.

If the name is already taken on the bus,
`org.mpris.MediaPlayer2.ketai.instance<pid>` is used, as the specification
requires. If there is no session bus, the daemon logs it and keeps working
without MPRIS.

## How it works

- A dedicated thread owns the D-Bus connection (libdbus, no async).
- Properties are answered from a copy of the state that the main loop sends
  it after every change. Reading a property never waits for the main loop.
- Methods and writable properties are translated into protocol requests and
  travel to the main loop like any other request.
- MPRIS requests have the same limit as a client: up to 64 pending. If a
  program floods the bus with calls, the MPRIS thread waits for the main loop
  instead of filling its queue. None is lost.
- When `Volume`, `LoopStatus` or `Shuffle` is written, the copy is updated
  immediately: a read right after already sees the new value. A state that
  arrives later but belongs to an earlier change does not undo it; the
  written value is kept until the daemon confirms it (or for 2 seconds, if
  the daemon rejected it).
- Changes are announced with `PropertiesChanged`. `Position` is not
  announced: a jump in position (seek) emits `Seeked`, as the specification
  requires.

## Methods

| MPRIS | Protocol request |
|---|---|
| `Play` | `player.play` |
| `Pause` | `player.pause` |
| `PlayPause` | `player.toggle` |
| `Stop` | `player.stop` |
| `Next` | `player.next` |
| `Previous` | `player.previous` |
| `Seek(offset_us)` | `player.seek_relative`; past the end of the track it acts as `Next` (no effect if there is no next track) |
| `SetPosition(track, pos_us)` | `player.seek`; ignored if `track` is not the current track or the position is out of range |
| `OpenUri(uri)` | `queue.add` (only `file://`) |
| `Raise` | does nothing (`CanRaise` is false) |
| `Quit` | error (`CanQuit` is false): the daemon is not closed from MPRIS |

## Properties

| MPRIS | Source |
|---|---|
| `PlaybackStatus` | `status`: `Playing`, `Paused` or `Stopped` |
| `LoopStatus` | `repeat`: `off` = `None`, `track` = `Track`, `queue` = `Playlist` |
| `Shuffle` | `shuffle` |
| `Volume` | `volume`, or 0 if `muted`. On write it is clamped to 0..1 (ketai does not amplify) and a value above 0 unmutes |
| `Position` | last known position plus the time elapsed if playing, in microseconds |
| `Metadata` | `mpris:trackid` (`/org/ketai/track/<track_id>`), `xesam:url`, `mpris:length`, `xesam:title`, `xesam:artist`, `xesam:album` |
| `CanPlay`, `CanPause` | there are tracks in the queue |
| `CanSeek` | the current track has a known duration |
| `CanGoNext` | there is a next track or repeat is on |
| `CanGoPrevious` | there is a current track |
| `CanControl` | always true |
| `Rate`, `MinimumRate`, `MaximumRate` | 1.0 (no speed change) |

`HasTrackList` is false: the `TrackList` interface is not implemented. The
queue is managed with ketai's protocol.

## Tests

The conformance tests (`crates/ketai/tests/mpris.rs`) start their own
`dbus-daemon` per test. The rest of the suite points
`DBUS_SESSION_BUS_ADDRESS` at a path that does not exist: no test daemon
shows up on the user's bus.
