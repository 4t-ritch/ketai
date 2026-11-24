**English** | [Espanol](es/protocol.md)

# ketai IPC protocol - v1 (draft)

This document is the contract between `ketaid` (the daemon) and every client
(the TUI, waybar scripts, GUIs). Any implementation of the core (Rust or C)
is correct if and only if it follows this document and passes the
conformance suite.

## 0. Quick start: talking to the daemon

The daemon listens on a Unix socket. Each message is one line of JSON: you
send a request with an `id`, and the daemon answers with the same `id`.

With `socat`, interactively:

```sh
socat - UNIX-CONNECT:"$XDG_RUNTIME_DIR/ketai/ctl.sock"
```

Then type one request per line:

```json
{"id": 1, "method": "daemon.hello", "params": {"client": "my-script", "protocol": 1}}
{"id": 2, "method": "player.state"}
{"id": 3, "method": "player.toggle"}
{"id": 4, "method": "events.subscribe", "params": {"topics": ["player"]}}
```

The daemon answers each one on its own line:

```json
{"id": 1, "result": {"server": "ketaid", "protocol": 1, "version": "0.1.0"}}
{"id": 2, "result": {"status": "playing", "track": {"title": "...", "...": "..."}, "position_ms": 53400, "volume": 0.8, "muted": false, "repeat": "off", "shuffle": false, "output": null}}
{"id": 3, "result": {}}
{"id": 4, "result": {}}
{"event": "player.state", "data": {"status": "paused", "...": "..."}}
```

After `events.subscribe`, events (`{"event": ..., "data": ...}`, no `id`)
arrive whenever something changes.

The same in Python, with no dependencies:

```python
import json, os, socket

sock = socket.socket(socket.AF_UNIX)
sock.connect(os.path.join(os.environ["XDG_RUNTIME_DIR"], "ketai", "ctl.sock"))
f = sock.makefile("rw")
next_id = 0

def request(method, params=None):
    global next_id
    next_id += 1
    f.write(json.dumps({"id": next_id, "method": method, "params": params or {}}) + "\n")
    f.flush()
    while True:  # skip events until our answer arrives
        msg = json.loads(f.readline())
        if msg.get("id") == next_id:
            if "error" in msg:
                raise RuntimeError(msg["error"])
            return msg["result"]

request("daemon.hello", {"client": "my-script", "protocol": 1})
state = request("player.state")
print(state["status"], (state["track"] or {}).get("title"))  # track is null when nothing is loaded
request("queue.add", {"uris": ["file:///home/me/music/song.flac"]})
```

The rest of this document describes every message in detail.

Method, event and JSON field names are in English and are part of the
contract.

## 1. Transport

| Channel    | Path                                 | Format                          |
|------------|--------------------------------------|---------------------------------|
| Control    | `$XDG_RUNTIME_DIR/ketai/ctl.sock`    | Newline-delimited JSON          |
| Visualizer | `$XDG_RUNTIME_DIR/ketai/viz.sock`    | Fixed binary frames (section 7) |

- Both are `AF_UNIX`, `SOCK_STREAM`, with permissions `0600`.
- If `XDG_RUNTIME_DIR` is not set, the daemon refuses to start.
- The sockets may come from systemd (socket activation); see
  `docs/service.md`.
- Many clients can connect at once. The daemon is the only owner of the audio
  output; clients never open audio devices.

## 2. Framing (control channel)

- One JSON object per line, UTF-8, terminated by `\n`.
- Messages cannot contain raw newlines (JSON escapes them).
- Maximum line length: 1 MiB. A longer line closes the connection with error
  `-32600`.

## 3. Message shape

**Request** (client -> daemon):
```json
{"id": 1, "method": "player.play", "params": {}}
```

**Response** (daemon -> client, same `id`):
```json
{"id": 1, "result": {}}
{"id": 1, "error": {"code": -32601, "message": "method not found"}}
```
Some errors carry `error.data` with details (for example `1009`); clients
that do not use it ignore it.

**Event** (daemon -> client, no `id`, only after `events.subscribe`):
```json
{"event": "track.changed", "data": {"track": { ... }}}
```

- `id` is an unsigned integer chosen by the client, unique per connection.
- Responses can be interleaved with events; the client matches them by `id`.
- Requests from the same client run in the order they arrived, even if the
  client sends them without waiting for a response. Example: `queue.add`
  followed by `queue.list` always sees the added tracks. Responses to a
  client go out in that same order.
- A client can have up to 64 requests without a response (a response counts
  once it has been written to the socket). At that limit the daemon stops
  reading its socket until the client reads responses, and its writes block.
  Nothing is lost or rejected: a client that never reads simply stalls,
  without growing the daemon's memory.
- `params` can be omitted when empty.

## 4. Data types

**Track**
```json
{
  "id": "t_9f2c",
  "source": "local",
  "uri": "file:///music/song.ogg",
  "title": "Song",
  "artist": "Artist",
  "album": "Album",
  "duration_ms": 201000,
  "has_cover": true,
  "codec": "vorbis",
  "sample_rate": 44100,
  "channels": 2,
  "album_artist": "Various",
  "genre": "J-Pop",
  "year": 2021,
  "track_number": 3,
  "disc_number": 1,
  "bitrate_kbps": 192
}
```
- `source`: `"local"` | `"remote"`.
- `album_artist`, `genre`, `year`, `track_number` and `disc_number` come from
  the tags (for now only in library results). `year` is the first four
  digits of the date; "3/12" becomes `3`.
- Unknown fields are `null`. Clients must ignore unknown keys.

**PlayerState**
```json
{
  "status": "playing",
  "track": { "...": "Track or null" },
  "position_ms": 53400,
  "volume": 0.8,
  "muted": false,
  "repeat": "off",
  "shuffle": false,
  "output": "alsa_output.usb-headset"
}
```
- `status`: `"stopped"` | `"playing"` | `"paused"` | `"buffering"`.
- `volume`: `0.0`-`1.0`.
- `repeat`: `"off"` | `"track"` | `"queue"`.
- `output`: device id, or `null`, which means "follow the system default".

**LyricsLayer** (one available lyrics layer)
```json
{
  "id": "ja-Latn-hepburn",
  "kind": "romanized",
  "language": "ja-Latn-hepburn",
  "timing": "word",
  "format": "ttml",
  "origin": "sidecar"
}
```
- `id`: stable identifier of the layer within the track. The original layer
  always has `id: "original"`. The others use their BCP 47 tag (see
  section 11).
- `kind`: `"original"` | `"romanized"` | `"translation"`.
- `timing`: the layer's own timing level (see section 11):
  `"none"` | `"line"` | `"word"` | `"word-paired"`.
- `format`: `"text"` | `"lrc"` | `"ttml"`.
- `origin`: `"sidecar"` (local file) | `"embedded"` (audio tags).

**LyricsPackage** (lyrics assembled as the client asked)
```json
{
  "primary": "ja-Latn-hepburn",
  "secondary": ["es"],
  "timing": "word",
  "offset_ms": 0,
  "user_offset_ms": -250,
  "agents": [{"id": "v1", "name": "Lead"}],
  "lines": [
    {
      "start_ms": 12000,
      "end_ms": 15500,
      "text": "kimi no koe ga",
      "agent": "v1",
      "background": false,
      "words": [
        {"start_ms": 12000, "end_ms": 12600, "text": "kimi "},
        {"start_ms": 12600, "end_ms": 12900, "text": "no"}
      ],
      "secondary": {"es": "tu voz"}
    }
  ]
}
```
- `timing`: effective timing level of `words` in this package. It is the
  primary layer's, or `"word-estimated"` if the client asked for it and the
  layer had no word timing.
- Each line's `start_ms` / `end_ms` always come from the original layer (the
  common anchor). If the original has no timing (`timing: "none"`), they are
  `null` and the package has `timing: "none"`.
- `end_ms` can be `null` if the format does not provide it; the client uses
  the next line's `start_ms`. In LRC the line ends where the next one starts,
  except with A2 (word timing): the closing `<mm:ss.xx>` after the last word
  ends the line too, so it is not lit longer than it is sung (the same as
  TTML's `end`). A timestamp with no text (`[00:04.00]`) is a pause: it ends
  the previous line and is not a line itself.
- `text`: the primary layer's text for that line.
- `words`: the primary layer's words with their own timing, or `null` if
  `timing` is `"none"` or `"line"`. Each word carries the space that follows
  it (`"kimi "`, `"no"`), so joining the words gives the line's text.
- `secondary`: object `{layer_id: text}`, per line only, no words. A line
  with no text in a secondary layer does not have that key.
- `agent` and `background` come from TTML (duets and backing vocals); in
  other formats they are `null` and `false`.
- Times already include the offsets. The offset fields are informational.
- The per-letter animation is computed by the client, spreading each word's
  time across its characters.

**Device**
```json
{"id": "alsa_output.usb-headset", "name": "USB Headset", "is_default": true}
```

## 5. Methods

### Daemon
| Method            | Params | Result |
|-------------------|--------|--------|
| `daemon.hello`    | `{"client": "ketai-tui", "protocol": 1}` | `{"server": "ketaid", "protocol": 1, "version": "0.1.0"}` |
| `daemon.shutdown` | -      | `{}` |

Clients must call `daemon.hello` first. If the protocol version does not
match, error `1000` is returned.

### Player
| Method                 | Params                   | Result        |
|------------------------|--------------------------|---------------|
| `player.state`         | -                        | `PlayerState` |
| `player.play`          | -                        | `{}`          |
| `player.pause`         | -                        | `{}`          |
| `player.toggle`        | -                        | `{}`          |
| `player.stop`          | -                        | `{}`          |
| `player.next`          | -                        | `{}`          |
| `player.previous`      | -                        | `{}`          |
| `player.seek`          | `{"position_ms": 60000}` | `{}`          |
| `player.seek_relative` | `{"offset_ms": -5000}`   | `{}`          |
| `player.set_volume`    | `{"volume": 0.5}`        | `{}`          |
| `player.set_muted`     | `{"muted": true}`        | `{}`          |
| `player.set_repeat`    | `{"repeat": "queue"}`    | `{}`          |
| `player.set_shuffle`   | `{"shuffle": true}`      | `{}`          |

Player rules:

- `player.play` with an empty queue, or `player.next` with no next track,
  return `1001`. With a queue but no selected track, `player.play` plays the
  first one.
- `player.next` and `player.previous` keep the state: if it was playing it
  keeps playing; if paused it stays paused at the start of the new track; if
  stopped only the selection changes.
- `player.previous` restarts the current track if more than 3 seconds have
  played or if it is the first in the queue (without `repeat: "queue"`).
- `player.seek` and `player.seek_relative` need a loaded track (`playing` or
  `paused`); otherwise `1001`. The position is clamped to
  `0`..`duration_ms`.
- `player.stop` keeps the track selected and the position at `0`.
- `volume` outside `0.0`..`1.0` returns `-32602`. `muted` does not change
  `volume`.
- End of track: with `repeat: "track"` it repeats; otherwise it moves to the
  next one. With no next track, the state becomes `stopped` with the last
  track selected.
- If a track cannot be played, it is skipped. If every track in the queue
  fails in a row, the state becomes `stopped`.
- With `shuffle: true`, the next track is picked at random among those not
  played in the current cycle, and `player.previous` walks that history.
  When all have played: with `repeat: "queue"` a new cycle starts; otherwise
  there is no next track. `shuffle` does not change the order of
  `queue.list`.

### Queue
| Method         | Params                                          | Result |
|----------------|-------------------------------------------------|--------|
| `queue.list`   | -                                               | `{"current": 2, "tracks": [Track]}` |
| `queue.add`    | `{"uris": ["file:///a.ogg", "file:///b.ogg"], "position": null}` | `{"added": 2}` |
| `queue.remove` | `{"indexes": [3, 5]}`                           | `{"removed": 2}` |
| `queue.move`   | `{"from": 3, "to": 0}`                          | `{}` |
| `queue.clear`  | -                                               | `{}` |
| `queue.jump`   | `{"index": 4}`                                  | `{}` |

- `queue.add` accepts one or more `uris`. They are inserted together, in the
  order sent, starting at `position`. `position: null` appends at the end.
- `uris` accepts `file://`, `http://` and `https://`.
- `queue.remove` accepts one or more `indexes`, without repeats, counted on
  the queue before removing anything. For a single one: `{"indexes": [3]}`.
- If the playing track is removed, playback moves to the next one left in the
  queue; if none is left, the state becomes `stopped`.
- `queue.add` and `queue.remove` are atomic: on an invalid URI or an index
  out of range or repeated, the queue does not change.
- `queue.add` reads the metadata of each local file. A URI with an
  unsupported scheme returns `-32602`; a missing file or one that cannot be
  decoded returns `1003`. `file://` paths accept `%`-encoded characters (for
  example `%20` for a space).
- `queue.jump` selects the track and plays it from the start.
- `queue.clear` leaves the state `stopped` with no track selected.

### Playlists
| Method            | Params                                  | Result |
|-------------------|-----------------------------------------|--------|
| `playlist.list`   | -                                       | `{"playlists": [{"name": "chill", "count": 12}]}` |
| `playlist.get`    | `{"name": "chill"}`                     | `{"name": "chill", "path": "/home/u/music/playlists/chill.m3u8", "tracks": [Track]}` |
| `playlist.save`   | `{"name": "chill", "from_queue": true}` | `{}` |
| `playlist.delete` | `{"name": "chill"}`                     | `{}` |
| `playlist.load`   | `{"name": "chill", "replace": true}`    | `{}` |
| `playlist.rename` | `{"name": "chill", "new_name": "relax"}` | `{}` |
| `playlist.add`    | `{"name": "chill", "uris": ["file:///a.ogg", "file:///b.ogg"], "position": null, "duplicates": "ask"}` | `{"added": 2}` |
| `playlist.remove` | `{"name": "chill", "indexes": [3, 5]}`  | `{"removed": 2}` |
| `playlist.remove` | `{"name": "chill", "uris": ["file:///a.ogg"]}` | `{"removed": 1}` |
| `playlist.move`   | `{"name": "chill", "from": 3, "to": 0}` | `{}` |
| `playlist.transfer` | `{"from_name": "rock", "indexes": [2, 5, 7], "to_name": "metal", "position": null, "mode": "move"}` | `{"transferred": 3}` |
| `favorites.set`   | `{"uri": "file:///a.ogg", "favorite": true}` | `{}` |
| `track.playlists` | `{"uri": "file:///a.ogg"}`              | `{"playlists": ["chill", "rock"], "favorite": true}` |

- Playlists live in ketai's database (`library.db`, see the library section),
  which is the source of truth. Each change is a transaction and goes through
  a single thread: two clients adding to the same playlist at the same time
  never lose tracks. A song can be in several playlists, and several times in
  the same one.
- Favorites is one more playlist, named `Favoritos`, which in
  `playlist.list` carries `"kind": "favorites"` (the others have no `kind`).
  It is created by the first `favorites.set`. `favorites.set` with
  `favorite: true` adds the track if it was not there; with `false` it
  removes it. `uri` and `favorite` are required (`-32602`). It emits
  `playlist.changed` with `Favoritos`.
- `playlist.add` with tracks already in the playlist: `duplicates` decides.
  `"ask"` (or no field) adds nothing and returns error `1009` with
  `data: {"duplicates": [uris]}`, so the client can ask the user; `"add"`
  adds them anyway; `"skip"` adds only the missing ones (`added` counts the
  added tracks). Any other value returns `-32602`.
- `playlist.remove` removes by position (`indexes`) or by song (`uris`: every
  copy of each one); exactly one of the two must be given (`-32602`
  otherwise, or if both are given). `removed` counts the removed tracks; with
  `uris` that are not there, `removed: 0` and nothing changes.
- `track.playlists` tells which playlists contain a URI (without Favorites,
  ordered by name) and whether it is a favorite.
- Each playlist has a `.m3u8` copy in the playlists folder, compatible with
  other players, written by ketai. The folder is `[playlists] dir` in
  `config.toml` (see `docs/config.md`) or, if not set,
  `XDG_MUSIC_DIR/playlists` (`XDG_MUSIC_DIR` from the environment, from
  `~/.config/user-dirs.dirs`, or `~/music`). It is resolved on every
  operation, so a change in `config.toml` applies without restarting, and it
  is created if missing. `playlist.get` returns the file path in `path`.
  Local tracks are stored as absolute paths. A new `.m3u8` copied into that
  folder is imported as a playlist; when reading it, absolute paths, paths
  relative to the folder and URIs are accepted. The file of an existing
  playlist, edited or deleted by hand, is written again from the database.
  If the folder changes, every playlist is copied to the new one. The first
  time, the playlists already in the folder are imported.
- When a playlist changes, its file is rewritten with paths only: comments and
  `#EXTINF` lines of a `.m3u8` created by another program are lost.
- Only files with a lowercase `.m3u8` extension are recognized.
- The name is also the file name: it cannot be empty, start with `.` or
  contain `/`. An invalid name returns `-32602`.
- `playlist.save` creates the playlist or replaces an existing one. With
  `from_queue: false` it is empty.
- `playlist.rename` to a name that already exists returns `1008`.
- `playlist.get` also shows tracks whose file no longer exists, with
  metadata set to `null`.
- `playlist.load` adds to the queue only the tracks that can be played. With
  `replace: true` it clears the queue first and leaves the state `stopped`;
  with `replace: false` it appends without changing playback.
- `playlist.move` reorders the playlist: it takes the track at `from` and
  inserts it at `to`. Indexes count on the list before the move.
- `playlist.transfer` moves one or more tracks from one playlist to another:
  - `from_name` and `to_name` must differ (`-32602`); to reorder within a
    playlist use `playlist.move`.
  - `indexes` are positions in the source, without repeats. The tracks reach
    the destination in their source order, not in the order of the list sent.
  - `mode: "copy"` leaves them in the source and adds copies to the
    destination.
  - `mode: "move"` removes them from the source and adds them to the
    destination.
  - If any index is out of range or repeated, nothing is transferred and
    error `1002` or `-32602` is returned.
  - `position: null` appends at the end of the destination.
  - The operation is atomic: if the destination fails, the source does not
    change.
  - If `from_name` or `to_name` does not exist, it returns error `1005`.
  - It emits `playlist.changed` for each modified playlist: with `copy` only
    the destination.
  - If, with `move`, writing the source fails after writing the destination,
    the tracks stay in both playlists, the operation returns an error and
    `playlist.changed` is emitted for the destination.
- `playlist.add` accepts one or more `uris`. They are inserted together, in
  the order sent, starting at `position`. `position: null` appends at the
  end.
- `playlist.remove` accepts one or more `indexes`, without repeats. Indexes
  count on the list before removing anything. For a single item, send a
  one-item list: `{"indexes": [3]}`.
- `playlist.add` and `playlist.remove` are atomic: if a URI is invalid or an
  index is out of range or repeated, the playlist does not change.
- Out-of-range indexes return error `1002`.
- Every change emits the `playlist.changed` event.

### Lyrics
| Method              | Params                                   | Result |
|---------------------|------------------------------------------|--------|
| `lyrics.layers`     | `{"track_id": "t_9f2c"}`                 | `{"layers": [LyricsLayer]}` (empty if there are no lyrics) |
| `lyrics.get`        | `{"track_id": "t_9f2c", "primary": "ja-Latn-hepburn", "secondary": ["es"], "estimate_words": false}` | `LyricsPackage` or `null` |
| `lyrics.set_offset` | `{"track_id": "t_9f2c", "offset_ms": -250}` | `{}` |

- In `lyrics.get`, `primary` is optional (default `"original"`),
  `secondary` is optional (default `[]`) and `estimate_words` is optional
  (default `false`).
- A layer `id` that does not exist returns error `1007`.
- The same layer cannot be in both `primary` and `secondary`; that returns
  `-32602`.
- `lyrics.get` returns `null` if the track has no lyrics.
- `track_id` must be a track in the queue; otherwise `-32602`. Remote tracks
  have no lyrics yet (`layers` empty and `get` returns `null`).
- Only layers offered by `lyrics.layers` can be requested; a layer that
  exists but could not be aligned also returns `1007`.
- `lyrics.set_offset` stores the adjustment per file (URI), not per
  `track_id`: it is remembered in future sessions even if the id changes. It
  is stored in `$XDG_STATE_HOME/ketai/lyrics-offsets.json`. `offset_ms: 0`
  deletes it. It emits `lyrics.changed`.

See section 4 for the types and section 11 for the rules.

### Library / sources
| Method           | Params                            | Result |
|------------------|-----------------------------------|--------|
| `library.scan`   | `{"path": "/home/u/Music"}` or `{}` (every folder) | `{"job": "j_1"}` (progress through events) |
| `library.folders` | -                                | `{"folders": [{"path": "/home/u/Music", "tracks": 1204}]}` |
| `library.add_folder` | `{"path": "/home/u/Music"}`   | `{}` |
| `library.remove_folder` | `{"path": "/home/u/Music"}` | `{}` |
| `library.search` | `{"query": "illit", "limit": 50}` | `{"tracks": [Track]}` |
| `library.groups` | `{"by": "artist"}` | `{"groups": [{"name": "ILLIT", "count": 12}]}` |
| `library.tracks` | `{"by": "artist", "name": "ILLIT", "offset": 0, "limit": 200}` | `{"tracks": [Track], "total": 12}` |

### Cache
| Method        | Params | Result |
|---------------|--------|--------|
| `cache.info`  | -      | `{"path": "...", "size_bytes": 1048576, "entries": 12, "limit_bytes": 2147483648}` |
| `cache.clear` | -      | `{"freed_bytes": 1048576}` |

### Covers
| Method        | Params                   | Result |
|---------------|--------------------------|--------|
| `track.cover` | `{"track_id": "t_9f2c"}` | `{"path": "/run/user/1000/ketai/covers/t_9f2c.jpg", "mime": "image/jpeg"}` |
| `track.cover` | `{"uri": "file:///music/song.flac"}` | same; the file is named `u_<hash>` |

Covers are written to disk and their path is returned; they are never sent
embedded in base64.

Library, cover and cache rules (v1, local files only):

- The library folders are stored next to the index. `add_folder` takes an
  existing absolute folder (`-32602` otherwise); adding it twice does
  nothing. `remove_folder` of a folder that is not there returns `-32602`
  and, if it is there, removes its songs from the index. Both emit
  `library.changed` (topic `library`).
- `library.scan` with `path` takes an existing absolute folder; otherwise
  `-32602`. Without `path` it scans every library folder in a single job
  (`-32602` if there are none). It answers right away with the `job` and
  scans in the background, recursively, without following linked folders.
  Candidates are files with an audio extension (`flac`, `mp3`, `m4a`, `mp4`,
  `aac`, `ogg`, `oga`, `opus`, `wav`); those that cannot be played do not
  enter the index.
- `library.progress` arrives at the start (`done: 0`), at most every 200 ms,
  and at the end with `done == total`. The final event arrives once the
  index is updated: a search made on receiving it already sees the tracks.
- Scanning a folder again replaces everything indexed under it (what was
  deleted disappears at the end). What is read is saved in batches of 200
  tracks while scanning: if the daemon stops halfway, what was saved stays.
  The index and the folders are stored in an SQLite database,
  `$XDG_DATA_HOME/ketai/library.db`, and survive restarts. Each change is a
  transaction and the requests of every client go through a single thread:
  two clients at the same time never step on each other. If a `library.json`
  from an earlier version exists, it is imported the first time the database
  is created (the file is not deleted).
- `library.groups` groups the library by `artist`, `album`, `folder` (each
  file's folder), `year` (as text) or `genre`, in order. Tracks without that
  data form the group `name: null`, which comes first. Any other `by`
  returns `-32602`.
- `library.tracks` returns one page of tracks and the `total`. Without `by`,
  from the whole library, by path. With `by`, `name` is required (`null` is
  the group without data); within an album they go by disc and track number.
  `offset` starts at 0 and `limit` goes from 1 to 5000 (200 by default).
- `library.search`: `query` is required. Every word must appear, anywhere
  (not only at the start), in the title, artist, album, album artist, genre,
  year or file name. Case and marks do not matter: "cancion" finds the title
  with an accented o and "nino" the one with an n with tilde, and the other
  way around (acute, diaeresis, tilde, cedilla; also U+00DF = ss,
  U+00E6 = ae, U+0153 = oe, U+00F8 = o, U+0142 = l, U+0111 = d). Marks of
  other scripts are kept because they change the word (the Japanese
  dakuten, U+3099: "ga" is not "ka"). `""` returns everything. `limit`
  (default 50) must be a positive integer. Results are ordered by path and
  their `id` is `null`: they are not queue tracks (add them with
  `queue.add` using their `uri`).
- `track.cover` takes a `track_id` from the queue (`-32602` if it does not
  exist) or, for tracks not in the queue (playlists, search results), a local
  `uri`. If both are given, `track_id` is used. With `uri`, the file is named
  `u_<hash of the uri>` and is always the same for the same URI; a remote
  URI or a missing file returns `null`. It uses the embedded image
  (preferably the front cover) and, if there is none, an image in the
  folder: `cover`, `folder`, `front` or `album`, with extension `jpg`,
  `jpeg`, `png` or `webp`, case-insensitive. It is written to
  `$XDG_RUNTIME_DIR/ketai/covers/` (file `0600`). Without a cover it returns
  `null`. `has_cover` considers both sources.
- The cache lives in `$XDG_CACHE_HOME/ketai/`. In v1 nothing is downloaded,
  so it is usually empty. `cache.clear` deletes its contents, not the
  folder. Remote sources and `download.progress` arrive in a future version
  (see `docs/roadmap.md`).

### Output devices
| Method        | Params                                        | Result |
|---------------|-----------------------------------------------|--------|
| `output.list` | -                                             | `{"devices": [Device]}` |
| `output.set`  | `{"id": "alsa_output..."}` or `{"id": null}`  | `{}` |

`null` goes back to following the system default device.

Rules:

- The devices are the sound server's sinks (PulseAudio, or PipeWire through
  pipewire-pulse), so Bluetooth ones show up too. `id` is the sink name and
  `name` its description.
- `output.set` with an `id` not in the list returns `1004`; with a value that
  is neither text nor `null`, `-32602`.
- Switching outputs moves ketai's stream to the other sink; the stream is not
  reopened.
- With `id: null`, ketai follows the system default: if the default changes,
  the stream moves by itself.
- If the chosen device disconnects, audio goes to the default, but the
  choice is kept: when it reconnects, audio goes back to it.
- `PlayerState.output` and the `active` field of `output.changed` are the
  user's choice (`null` = following the default).
- `output.changed` is emitted when the list, the default or the choice
  changes.
- If no sound server is available, `output.list` returns an empty list.

### Events
| Method               | Params                            | Result |
|----------------------|-----------------------------------|--------|
| `events.subscribe`   | `{"topics": ["player", "queue"]}` | `{}` |
| `events.unsubscribe` | `{"topics": ["queue"]}`           | `{}` |

## 6. Events

| Topic      | Event               | Data |
|------------|---------------------|------|
| `player`   | `player.state`      | `PlayerState` (on any change of state, volume or mode) |
| `player`   | `player.position`   | `{"position_ms": 54400}` (at most 1 Hz while playing) |
| `player`   | `track.changed`     | `{"track": Track or null}` |
| `queue`    | `queue.changed`     | `{}` (the client asks `queue.list` again) |
| `playlist` | `playlist.changed`  | `{"name": "chill"}` |
| `output`   | `output.changed`    | `{"devices": [Device], "active": "id or null"}` |
| `library`  | `library.progress`  | `{"job": "j_1", "done": 120, "total": 400}` |
| `library`  | `library.changed`   | `{}` (the folders changed; the client asks `library.folders` again) |
| `lyrics`   | `lyrics.changed`    | `{"track_id": "t_9f2c"}` (offset changed; the client asks `lyrics.get` again) |
| `cache`    | `download.progress` | `{"track_id": "t_1", "bytes": 1000, "total": 5000}` |

On `events.subscribe`, if `player` is among the topics, the daemon sends the
current `player.state` immediately.

Rules:

- `events.subscribe` and `events.unsubscribe` take a non-empty list of topics
  from this table; an unknown topic returns `-32602` and changes no
  subscription. Subscribing twice to the same topic does not duplicate
  events.
- The response to a request always arrives before the events that request
  causes.
- `player.state` is only emitted when something other than the position
  changes.
- `player.position` is emitted at most once per second while playing, and
  also right after a `player.seek` or `player.seek_relative`.
- `track.changed` is emitted when the selected track changes, including the
  change to `null` (for example after `queue.clear`).
- `queue.changed` is emitted when the content or order of the queue changes,
  not when only the selected track changes.
- The `cache` topic is already accepted, but `download.progress` comes with
  remote sources (future version, see `docs/roadmap.md`).

## 7. Visualizer channel

Binary, little-endian. The client connects and only reads; it never writes.

Header, sent once on connect:
```
offset size field
0      4    magic "KTV1"
4      4    uint32 bins        (e.g. 64)
8      4    uint32 fps         (e.g. 60)
```

Then a stream of frames, each `bins` x `float32`, with values normalized to
`0.0`-`1.0`. Frames are only sent while `status == "playing"`. Slow clients
lose frames; the daemon never blocks on a visualizer client.

v1 details:

- Socket: `$XDG_RUNTIME_DIR/ketai/viz.sock`, permissions `0600`. With socket
  activation it is the second socket passed by systemd.
- `bins` = 64 and `fps` = 60. Clients must read them from the header and not
  assume them.
- Each frame is the spectrum of the last 2048 samples played, mixed down to
  mono, with a Hann window.
- Bands are logarithmic between 30 Hz and 16 kHz (or half the sample rate if
  lower): band 0 is the lowest.
- The value is the band's peak in dB: -60 dB or less is `0.0` and 0 dB (a
  full-scale sine) is `1.0`. There is no smoothing between frames: a client
  that wants it applies it.
- Each client has a queue of 2 frames. If it does not read in time, the next
  frames are dropped for it, never halfway: a frame is written whole or not
  at all.

## 8. Errors

| Code    | Meaning |
|---------|---------|
| -32700  | Parse error (invalid JSON) |
| -32600  | Invalid request (wrong shape, line too long) |
| -32601  | Method not found |
| -32602  | Invalid params |
| -32603  | Internal error (for example, the library database failed) |
| 1000    | Incompatible protocol version |
| 1001    | Nothing to play / empty queue |
| 1002    | Index out of range |
| 1003    | Source unavailable (missing file, network error) |
| 1004    | Device not found |
| 1005    | Playlist not found |
| 1006    | Invalid lyrics (could not be parsed) |
| 1007    | Lyrics layer not found |
| 1008    | A playlist with that name already exists |
| 1009    | The track is already in the playlist (`data.duplicates`) |

## 9. Compatibility rules

- Adding methods, events or optional fields does not increase `protocol`.
- Removing something or changing its meaning does.
- Clients must ignore unknown fields and events.

## 10. Out of scope for v1

- MPRIS (D-Bus) is exposed by the daemon in parallel (see `docs/mpris.md`)
  and follows the MPRIS 2 specification; it is not part of this protocol.
- Lyrics from online providers (for example LRCLIB).
- Automatic romanization and dictionary-based timing estimation: planned for
  a major update, see `docs/roadmap.md`.

## 11. Lyrics: sources, formats and offset

### Formats

| Format | Extension | Timing | Notes |
|--------|-----------|--------|-------|
| Plain text | `.txt` | None | `timing: "none"` |
| LRC | `.lrc` | Per line (`[mm:ss.xx]`) | `timing: "line"` |
| Enhanced LRC (A2) | `.lrc` | Per line and word (`<mm:ss.xx>`) | `timing: "word"`; detected within the same `.lrc` |
| TTML | `.ttml` | Per line and word or syllable | `timing: "word"` or `"line"`; supports agents (duets) and backing vocals |

### Source priority

1. **Local file (sidecar)**: same name as the audio, in the same folder.
   Example: `roses.flac` -> `roses.ttml`, `roses.lrc`, `roses.txt`.
   If there are several, the richest is used: `.ttml`, then `.lrc`, then
   `.txt`.
2. **Embedded lyrics** in the audio tags:
   - MP3 (ID3v2): `SYLT` (synced) and then `USLT` (unsynced).
   - FLAC / Ogg / Opus (Vorbis comments): `LYRICS` and then
     `UNSYNCEDLYRICS`.
   - M4A: atom `(c)lyr`.
   - If the embedded text has LRC timestamps, it is parsed as LRC.
3. If there is nothing, `lyrics.layers` returns an empty list and
   `lyrics.get` returns `null`.
4. **No original layer, no lyrics**: romanizations and translations are
   synced against the original, so without it none is offered.

Reading details:

- A `.lrc` without timestamps is read as plain text.
- ID3: only the tag at the start of the file, versions 2.3 and 2.4.
  Compressed or encrypted frames are ignored. `SYLT` only with millisecond
  timestamps; if its entries start with a newline they are read as syllables
  with per-word timing.
- The language of an ID3 frame (ISO 639-2) is converted to its short BCP 47
  form when it is a common language (`spa` -> `es`); otherwise the 3-letter
  code is used.

This priority applies per layer: each layer (original, each romanization and
each translation) is looked up separately with the same order.

If the highest-priority source exists but cannot be parsed, the next one is
used. Error `1006` is only returned if no source is valid.

### Offset

- **File offset**: the LRC `[offset:+/-ms]` tag. It does not apply to TTML
  (it is `0`). A positive value = the lyrics appear earlier.
- **User offset**: manual per-track adjustment with `lyrics.set_offset`. It
  is stored in the daemon's configuration and persists across sessions.
- Final time of each line and word:
  `original_time - offset_ms - user_offset_ms`, with a minimum of `0`.

### Layers

A track can have an original layer, several romanizations and several
translations. The client lists the layers with `lyrics.layers` and asks for
the assembly with `lyrics.get`, choosing one primary layer and zero or more
secondary ones.

#### Layer identifiers and local files

Each layer's `id` comes from the local file suffix, using BCP 47 tags:

| File                            | Layer | `kind` |
|---------------------------------|-------|--------|
| `song.lrc`                      | `original` | `original` |
| `song.es.lrc`                   | `es` | `translation` |
| `song.ko-Latn.lrc`              | `ko-Latn` | `romanized` |
| `song.ja-Latn-hepburn.lrc`      | `ja-Latn-hepburn` | `romanized` |
| `song.romanized.lrc`            | `romanized` | `romanized` |

- A tag with the `Latn` script is a romanization; a language-only tag is a
  translation.
- For the same layer, among local files `.ttml` wins, then `.lrc`, then
  `.txt`.

#### Embedded sources and sources inside TTML

- **TTML**: the `<transliterations>` and `<translations>` blocks of
  `iTunesMetadata` produce `romanized` and `translation` layers. Their `id`
  is the block's language attribute.
- **Vorbis comments**: `LYRICS_ROMANIZED` produces the `romanized` layer and
  `LYRICS_TRANSLATION_<LANGUAGE>` produces the layer with that language, for
  example `LYRICS_TRANSLATION_ES` -> `es`.
- **ID3v2**: a `USLT`/`SYLT` with the descriptor `romanized` produces the
  `romanized` layer; with the descriptor `translation` it produces a
  translation whose language comes from the frame's language field (ISO
  639-2, for example `spa` -> `es`).

These tag and descriptor names are a ketai convention, not a standard.

#### Syncing between layers

Layers do not share word segmentation: "aishiteru" and "te amo" take the same
time window but have a different number of words, syllables and letters.
Therefore:

1. **Lines are always aligned to the original layer.** It is the common
   anchor and the only thing shared between layers.
   - LRC: each line is matched to the original line with the same start
     time, or the closest one within 500 ms; if there is none, it is
     dropped.
   - Plain text: matched line by line, in order. If the number of lines does
     not match, the layer is not offered.
   - TTML: matched by each line's identifier (`key`).
2. **Word timing only comes from that same layer's source.** The daemon never
   projects the original layer's words onto another layer.
3. **Safe exception:** if the source explicitly pairs each fragment of the
   layer with a fragment of the original (for example TTML `<span>`s paired
   by `key`), that fragment's exact timing is reused.
4. **Optional estimation:** if the client asks for `estimate_words: true`
   and the primary layer has no word timing, the daemon spreads each line's
   duration across its words in proportion to their number of characters,
   and the package is marked `word-estimated`.
5. A layer that cannot be aligned is dropped without affecting the others.

#### Timing levels

| `timing`         | Meaning |
|------------------|---------|
| `none`           | No timing (plain text) |
| `line`           | Line timing only |
| `word`           | Word timing that comes from that layer's source |
| `word-paired`    | Timing reused from the original through explicit 1-to-1 pairing |
| `word-estimated` | Estimated by the daemon within the line (only in `LyricsPackage`, and only if requested) |

#### Example

Romanized primary layer to sing along, Spanish translation as a subtitle:

```json
{"id": 9, "method": "lyrics.get",
 "params": {"track_id": "t_1", "primary": "ja-Latn-hepburn", "secondary": ["es"]}}
```

- The primary layer animates per word if its layer has `word` or
  `word-paired`; otherwise per line.
- The secondary layer changes line in sync with the primary.
