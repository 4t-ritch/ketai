**English** | [Espanol](../es/guide/architecture.md)

# How ketai is built

ketai is split into **two programs**: an audio service and a terminal client.
It is the same idea as mpd and its clients: the one that plays and the one
that shows are separate things.

```
                    +-------------------------------------------+
  ketai (TUI) ----> |  ketaid: audio service                    |
  ketai (mini) ---> |                                           | ---> PulseAudio / PipeWire
  waybar, scripts ->|  player, queue, library (SQLite),         |
  playerctl (MPRIS) |  playlists, lyrics, audio output          |
                    +-------------------------------------------+
                         ctl.sock (control)   viz.sock (spectrum)
```

## ketaid: the audio service

It is the one that **plays**. It runs in the background, usually as a systemd
user service, and keeps playing even with no TUI open.

It takes care of:

- **Playback**: decodes the audio (FLAC, MP3, AAC, ALAC, Vorbis, Opus, WAV)
  and sends it to the output. The output is only open while playing and is
  reopened by itself if the device fails.
- **Queue**: what plays, what comes next, shuffle and repeat.
- **Library**: scans your folders and stores the index in an SQLite database
  (`~/.local/share/ketai/library.db`), with the metadata and the search text
  already prepared.
- **Playlists and favorites**: they live in the same database. It also writes
  a `.m3u8` copy of each playlist in your playlists folder.
- **Lyrics**: finds each track's lyrics (files next to the audio or embedded
  lyrics) and their translations.
- **Audio output**: lists the devices and moves playback to the chosen one.
- **Covers**: extracts them from the file or the folder and hands them to the
  clients.
- **MPRIS**: publishes itself on D-Bus for media keys, `playerctl` and
  waybar.

Several parts of the daemon work in parallel (audio, library, playlists,
lyrics), each on its own thread with its own data. Every change goes through
a single place and is saved in a transaction: two clients changing something
at the same time never step on each other, and all of them hear about every
change.

There is a single instance per session: if another `ketaid` is started, the
one already running stays.

## ketai: the terminal client

It is what you **see**. It does not play anything by itself: it asks the
daemon and shows what the daemon tells it.

- Draws the screens (Now Playing, Lyrics, Library, Search), the popups and
  mini mode.
- Shows covers with the kitty graphics protocol and computes the dynamic
  theme colors.
- Handles the keyboard and the mouse, and reads `config.toml` (themes and
  shortcuts), reloading it live when saved.

Since all the state is in the daemon, you can open several TUIs at once (a
big one and a mini one in another pane) and they all show the same. Closing a
TUI does not stop the music.

## How they talk

Through local sockets in `$XDG_RUNTIME_DIR/ketai/`:

| Socket | What for |
|---|---|
| `ctl.sock` | Control: requests (play, search, add to a playlist...) and change notices (track changed, playlist modified...). JSON, one message per line. |
| `viz.sock` | The spectrum data, in binary, to draw it live. |

Any program can use the same protocol as the TUI. It is documented in
[../protocol.md](../protocol.md), which starts with a quick example of how to
connect, send a request and read the answer.

## Where things are stored

| What | Where |
|---|---|
| Configuration (themes, shortcuts, playlists folder) | `~/.config/ketai/config.toml` |
| Library, playlists and favorites | `~/.local/share/ketai/library.db` |
| Playlist copies | `~/music/playlists/*.m3u8` (configurable) |
| Last queue and position | `~/.local/state/ketai/` |
| Extracted covers (temporary) | `$XDG_RUNTIME_DIR/ketai/covers/` |
| Cache | `~/.cache/ketai/` |

## Why Linux only

ketai uses Linux-specific pieces:

- `/proc`, to find and close old daemon instances.
- systemd user services, to start the daemon only when needed.
- PulseAudio or PipeWire (through pipewire-pulse), to list and switch the
  audio output.
- D-Bus, for MPRIS.

On other systems those pieces do not exist or work differently, which is why
ketai only works on Linux for now.
