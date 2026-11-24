**English** | [Espanol](es/tui.md)

# ketai TUI

`ketai` is the terminal client. It does not play audio by itself: it talks to
`ketaid` through the IPC protocol (`docs/protocol.md`). Several TUIs can be
open at once against the same daemon (for example one in mini mode in a herdr
pane and a full one).

The interface is in Spanish for now (more languages are planned, see
`docs/roadmap.md`). This guide names each screen in English with the
on-screen Spanish label in italics, for example Now Playing
(*Reproduciendo*).

## Requirements

- A terminal with the kitty graphics protocol: kitty, Ghostty, WezTerm,
  Konsole, or herdr running inside one of them. On startup ketai asks the
  terminal; if it is not supported, it explains what is missing and exits.
- A Nerd Font for the icons.
- `KETAI_NO_IMAGES=1 ketai` starts anyway, without covers (useful over SSH).
- `KETAI_LOG=/tmp/ketai.log ketai` writes, with a UTC timestamp, the terminal
  events (focus, size, mouse and which element responded) and what is sent
  for the covers. Useful to diagnose problems with multiplexers such as
  herdr. Without the variable nothing is logged.

If `ketaid` is not running, the TUI shows the service screen: it diagnoses
the state and offers to start it through systemd (see `docs/service.md`). It
retries the connection every 2 seconds.

## Screens

Tabs at the top, on every screen:

| Key | Tab |
|---|---|
| `1` | Now Playing (*Reproduciendo*): cover, progress, controls, lyrics, next track and spectrum |
| `2` | Lyrics (*Letras*): current line with the word being sung, translations below |
| `3` | Library (*Biblioteca*): playlists, all songs, or by artist, album, folder, year or genre |
| `4` | Search (*Buscar*): search in the local library |

`Tab` and `Shift+Tab` go to the next or previous tab.

## Keys

Global shortcuts can be changed in `[keys]` of `config.toml` (see
`docs/config.md`); these are the defaults.

On any tab (except while typing in Search):

| Key | Action |
|---|---|
| `space` | play or pause |
| `n` / `p` | next / previous |
| left / right arrow | back / forward 5 seconds |
| `+` / `-` | volume |
| `m` | mute |
| `s` | shuffle |
| `r` | repeat: none, track, queue |
| `d` | choose the audio output |
| `*` | mark or unmark the playing song as a favorite |
| `P` | add the playing song to a playlist |
| `f` | library folders |
| `Ctrl+P` | actions and quick search over the current screen |
| `q` | quit (`Ctrl+C` always quits) |

Lyrics (configurable in `[keys]`, see [config.md](config.md#keys)):

| Key | Action |
|---|---|
| `t` | show or hide the translation |
| `o` | change the primary layer (original, romanized) |
| `[` / `]` | offset -50 / +50 ms (stored per file) |
| `w` | estimate word timing when the layer does not have it |
| `k` | switch between classic and karaoke mode (see [config.md](config.md#lyrics)) |

The top right of Lyrics shows each option with its key, like Now Playing:
the layer (`o`), the translation (`t`), the offset (`[` / `]`) and the mode
(`k`). Clicking an option does the same as its key.

Library:

| Key | Action |
|---|---|
| up and down arrows, or `k` `j` | move |
| `Enter` | open the playlist; on a song, play the list from there |
| `a` | add the song to the queue |
| `Esc` | go back to the list on the left |
| `h` / `l` | previous / next view: Playlists, Songs (*Canciones*), Artists (*Artistas*), Albums (*Albumes*), Folders (*Carpetas*), Years (*Anos*), Genres (*Generos*) |
| `c` | in Playlists, create a new playlist |
| `x` | in a playlist, remove the selected song |

In Songs and in the groups, the list is in columns (number, title and
artist, album, year and duration). `Enter` on a song puts the whole visible
list in the queue and starts from that one. Songs without the group's data
(for example without a genre) go under "No data" (*Sin dato*).

Search: typing starts right away. `Esc` or the down arrow moves to the
results, `/` goes back to typing. In the results: `Enter` plays, `a` adds to
the queue, `h` / `l` change the filter (All, Songs, Playlists, Local files).

Library folders (`f`): the list of folders ketai indexes, with how many songs
each one has. `a` adds one (type the path; `~` is your home folder), `x`
removes it (with confirmation; its songs leave the index), `s` scans the
selected folder and `S` all of them. Progress is shown while scanning. The
folders are kept by the daemon: every client sees the same ones.

Quick search: typing searches songs and playlists; below are the actions
(audio output, scan the whole library, lyrics, folders). `Enter` plays or
runs, `Tab` adds to the queue, `Esc` closes.

## Favorites and adding to a playlist

Every view shows the state of the active player: volume, output and whether
the song is a favorite (in Library and Search, on the bottom bar; in Lyrics,
next to the controls; in mini mode, next to the volume). Now Playing also
shows in how many playlists the song is, and next to the volume and the
output shows their keys (`-` `+` and `d` by default, or the ones from
`[keys]`), like Favorite and Add to playlist; they can be clicked too. The
output is never blank: "Default" if it follows the default device and its
name is not known yet, "No device" if none is available.

- `*` marks or unmarks the song as a favorite (it goes into the Favoritos
  playlist).
- `P` opens the list of playlists (those that already have the song carry a
  mark): `Enter` adds it, `c` creates a new playlist (type the name) and adds
  it there, and `x` removes it from the selected playlist (one with the
  mark).
- If it was already there, ketaid says so and ketai asks: `a` adds it
  anyway, `n` does not.

## Long text

If a text does not fit in its place, it scrolls so it can be read in full: it
waits a second and a half, moves one character every quarter of a second
and, after a separator, starts over. The title, artist and album in Now
Playing, the title and artist on the bottom bar, and the selected row of the
Library and Search lists scroll; the rest is cut, so the screen is not full
of moving things.

## Mouse

Everything on screen can be used with the mouse; it does the same as its key.

- Click a tab, the controls (shuffle, previous, play or pause, next, repeat),
  Favorite or Add to playlist.
- The progress and volume bars can be dragged: press, move and release (a
  click works too). The volume changes while dragging; the progress bar
  shows the point while dragging and jumps on release.
- On the bottom bar, clicking the volume mutes, the output opens the list of
  outputs and the heart marks the favorite. In Now Playing, clicking the
  output also opens the list.
- In Library: click a view (Playlists, Songs, Artists...) or the arrows on
  each side of the views, a playlist or group on the left, or a song. One click selects; a
  double click plays (like `Enter`).
- In Search: click a filter or a result; a double click plays.
- The wheel scrolls the list under the pointer, anywhere on the list.
- With a popup open, only the popup responds: clicking an option chooses it
  (in Folders it only selects it; the actions keep their keys), in the
  duplicate confirmation the "add anyway" and "do not add" buttons, and a
  click outside closes it.

While ketai uses the mouse, the terminal does not select text with a plain
click; most terminals still allow selecting with Shift + drag.

## Mini mode

If the terminal is smaller than 72 columns or 15 rows, ketai switches by
itself to a compact view: cover, song, one lyrics line, progress, controls
and a small spectrum. Meant for a small herdr or tmux pane; the playback keys
keep working.

In medium panes (for example 4:3, about 88x40) the full view gets compact: a
smaller cover, tighter controls, in Library the Now Playing column is hidden
(the bottom bar stays) and, with little height, the lyrics and next track are
left out.

## Covers

Covers are delivered by the daemon (`track.cover`, by track or by URI) and the
TUI sends them to the terminal with the kitty protocol. They are placed below
the cells' background: a popup, which has its own background, covers them
without manual cropping. Behind a popup a dimmed version is used.

Each ketai uses its own range of image ids and only deletes its own: several
can be open in the same terminal. When the size changes or focus comes back,
covers are sent again as they are drawn (some multiplexers, such as herdr,
lose them), and the terminal keeps at most 48: those not used for a while are
deleted and sent again if they reappear. That way the multiplexer's image
memory never fills up. ketai keeps at most 120 covers in its own memory; the
rest are asked from the daemon again if they reappear. Every 2 seconds it
places the images on screen again (without deleting anything), in case the
multiplexer lost them without notice.

While a song plays, ketai prepares the next song's cover and colors in the
queue ahead of time (only that one). That way, with the cover colors, the
theme changes the very moment the track changes, without a flash of the
previous color. With shuffle, the next one is the one that follows in the
queue shown by the daemon.
