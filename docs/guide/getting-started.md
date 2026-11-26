**English** | [Espanol](../es/guide/getting-started.md)

# Getting started

From zero to listening to music in five minutes.

## 1. What you need

- **Linux**, with PulseAudio or PipeWire (almost every distribution has it).
- **A terminal with the kitty graphics protocol**, to see covers: kitty,
  Ghostty, WezTerm or Konsole. Without it ketai still works with
  `KETAI_NO_IMAGES=1 ketai`, without covers.
- **A Nerd Font** set in the terminal, for the icons.
- To build: Rust, `cmake`, a C compiler and the PulseAudio and D-Bus
  development libraries (on Arch: `libpulse` and `dbus`; on Debian/Ubuntu:
  `libpulse-dev` and `libdbus-1-dev`).

## 2. Install

```sh
git clone https://github.com/4t-ritch/ketai.git
cd ketai
cargo build --release
install -Dm755 target/release/ketaid target/release/ketai -t ~/.local/bin/
```

`~/.local/bin` must be in your `PATH`.

## 3. Get the audio service ready

ketai has two parts: the audio service (`ketaid`) and the client (`ketai`).
The easiest is to let systemd start the service by itself, the first time
you need it:

```sh
install -Dm644 crates/ketai-tui/systemd/ketaid.{socket,service} -t ~/.config/systemd/user/
systemctl --user daemon-reload
systemctl --user enable --now ketaid.socket
```

If you prefer not to use systemd, run `ketaid` in a terminal and leave that
terminal open. If you open `ketai` without the service, the TUI tells you
what is missing and offers to start it.

## 4. Add your music

1. Open the TUI: `ketai`.
2. Press `f` to open **Library folders** (*Carpetas de la biblioteca*).
3. Press `a`, type the folder (for example `~/music`) and `Enter`.
4. Press `S` to scan everything. Progress is shown; with thousands of songs
   it takes a few seconds.

The library is saved: there is no need to scan every time. Scan again (`S`)
when you add new songs.

## 5. Listen

- **Search**: `4` and type part of the title, the artist or the album.
  Accents and case do not matter. `Esc` moves to the results and `Enter`
  plays.
- **From any screen**: `Ctrl+P`, type and `Enter`.
- **By artist, album or genre**: `3` (Library) and `h` / `l` to change view.
- **With the mouse**: click to select, double click to play.

While playing: `space` pauses, `n` / `p` next and previous, `+` / `-` volume,
`*` favorite and `P` add to a playlist.

## 6. Make it yours

Create `~/.config/ketai/config.toml`. The simplest starting point is the
complete example: [examples/config.toml](../../examples/config.toml).

- Colors: [examples/theme/](../../examples/theme/README.md).
- Shortcuts: [examples/keys/](../../examples/keys/README.md).

Changes show up when you save the file.

## If something does not work

- **No covers**: the terminal must support the kitty graphics protocol. See
  [../tui.md](../tui.md#requirements).
- **No sound**: `d` shows the selected audio output; try another one.
- **The TUI says there is no service**: see step 3 or
  [../service.md](../service.md).
- **Something odd with the terminal or a multiplexer**:
  `KETAI_LOG=/tmp/ketai.log ketai` logs what happens, to report it in an
  issue.
