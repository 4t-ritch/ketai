**English** | [Espanol](es/config.md)

# Configuration

The TUI reads `$XDG_CONFIG_HOME/ketai/config.toml` (by default
`~/.config/ketai/config.toml`). If it does not exist, the defaults are used.
There is a commented example in `dist/config.toml`, a complete example for
new users in `examples/config.toml`, and guides with examples per topic in
`examples/` (`examples/keys/` for shortcuts, `examples/theme/` for colors).

## Format

TOML: declarative, with comments and sections. Each customizable feature has
its own section (`[theme]`, `[keys]`, `[playlists]`); new sections are added
without breaking existing files. The TUI reads `[theme]` and `[keys]`, and
the daemon reads `[playlists]`.

- An unknown key or section is reported on the last row of the screen and
  ignored; the rest is applied.
- An error (syntax, invalid color) is shown on the last row with the key or
  the line, and the previous configuration is kept until the file is fixed.

## Live reload

Changes apply when you save the file, without closing ketai. The TUI checks
the file every half second; it works even if the editor saves by replacing
the file. If the file is deleted, the defaults come back.

## `[theme]`

| Key | Values | Default |
|---|---|---|
| `cover_color` | `true` or `false` | unset |
| `background` | `"cover"` or `"#rrggbb"` | `"cover"` |
| `accent` | `"cover"` or `"#rrggbb"` | `"cover"` |
| `foreground` | `"#rrggbb"` | `"#ffffff"` |
| `dim` | `"#rrggbb"` | `"#c8d3e0"` |
| `faint` | `"#rrggbb"` | `"#7f8ea3"` |
| `raised` | `"#rrggbb"` | `"#1c2a3a"` (with `background = "cover"`, it comes from the cover) |
| `on_accent` | `"#rrggbb"` | `"#04121f"` |
| `row` | `"#rrggbb"` | `"#16202e"` |
| `ok` | `"#rrggbb"` | `"#7ee787"` |
| `error` | `"#rrggbb"` | `"#ff7b8a"` |
| `spectrum_top` | `"#rrggbb"` | `"#ff7ab6"` |

`cover_color` decides where the colors come from:

- `true`: the whole interface comes from the playing cover (background,
  raised background, selected row, accent, text on the accent, secondary
  text and spectrum, all in the cover's hue). Manual colors are ignored even
  if they are in the file. Without a cover, the defaults.
- `false`: only manual colors count; `"cover"` means the default color.
- Without the key: each color decides; `background` and `accent` can be
  `"cover"`. With `background = "cover"`, `raised`, `row`, `dim` and `faint`
  also come from the cover; with `accent = "cover"`, `on_accent` and
  `spectrum_top`. A color set by hand always wins.

Popups, lyric lines not sung yet and the rest of the interface derive from
these colors: nothing has a fixed color of its own. `ok` and `error` are
status colors (connected, error) and do not follow the cover.

With `"cover"`, the colors come from the playing cover: the background is the
darkened average color and the accent is the same hue, light and saturated
so it reads well. Without a cover the defaults are used.

## `[keys]`

Global shortcuts. Each action takes a key (`toggle = "x"`) or a list
(`next = ["n", "ctrl+right"]`); what you set replaces that action's default
keys and the rest stays the same.

Key format: a character (`"n"`, `"P"`, `"+"`), or a name (`space`, `enter`,
`esc`, `tab`, `backspace`, `left`, `right`, `up`, `down`, `home`, `end`,
`pageup`, `pagedown`, `f1` to `f12`), with `ctrl+`, `alt+` or `shift+`
modifiers in front (`"ctrl+p"`, `"shift+tab"`).

| Action | Default | What it does |
|---|---|---|
| `toggle` | `space` | Play or pause |
| `next` | `n` | Next |
| `previous` | `p` | Previous |
| `seek_forward` | `right` | Forward 5 s |
| `seek_back` | `left` | Back 5 s |
| `volume_up` | `+`, `=` | Volume up |
| `volume_down` | `-` | Volume down |
| `mute` | `m` | Mute |
| `shuffle` | `s` | Shuffle |
| `repeat` | `r` | Repeat |
| `devices` | `d` | Audio output |
| `folders` | `f` | Library folders |
| `favorite` | `*` | Mark as favorite |
| `add_to_playlist` | `P` | Add to a playlist |
| `actions` | `ctrl+p`, `ctrl+k`, `/` | Actions and quick search |
| `quit` | `q` | Quit |
| `next_tab` / `prev_tab` | `tab` / `shift+tab` | Next / previous tab |
| `tab_now_playing`, `tab_lyrics`, `tab_library`, `tab_search` | `1` to `4` | Go to each tab |
| `lyrics_layer` | `o` | Lyrics: change the primary layer |
| `lyrics_translation` | `t` | Lyrics: show or hide the translation |
| `lyrics_offset_down` / `lyrics_offset_up` | `[` / `]` | Lyrics: offset -50 / +50 ms |
| `lyrics_mode` | `m` | Lyrics: classic or karaoke mode |
| `lyrics_estimate` | `w` | Lyrics: estimate word timing |

- `Ctrl+C` always quits, whatever else is configured.
- While typing in Search, keys without Ctrl or Alt are text.
- An invalid key is an error (it is shown and the previous configuration is
  kept); an unknown action is reported.
- **A key in two or more actions blocks the TUI**, at start or when the
  file is saved: a window lists every repeated key with its actions, grouped
  by key (it scrolls with the arrows, `j` / `k` or the wheel). Only scrolling
  and quitting (`q`, `Ctrl+C`) work; the music keeps playing. Fix `[keys]`
  and save: the TUI unlocks by itself, without restarting.
- The `lyrics_*` actions only work in the Lyrics view, and there they take
  priority over the global ones: by default `m` mutes everywhere except in
  Lyrics, where it changes the mode. That is not reported as a conflict; two
  Lyrics actions with the same key are (and block the TUI, see below).
- The keys shown on screen (for example the ones in the Lyrics header and
  next to the volume in Now Playing) are always the configured ones: change
  a key and the screen shows the new one.
- The other keys of each view (`h` / `l` in Library, etc.) are fixed for
  now.

## `[lyrics]`

How the Lyrics view (*Letras*) shows the lyrics.

| Key | Values | Default |
|---|---|---|
| `mode` | `"classic"` or `"karaoke"` | `"classic"` |
| `size` | `2` to `8` (rows) | `3` |
| `lines_before` | `1` to `2` | `2` |
| `lines_after` | `1` to `3` | `3` |
| `font` | path to a font file (`.ttf`, `.otf`, `.ttc`), absolute or starting with `~/` | chosen by fontconfig |

```toml
[lyrics]
mode = "karaoke"
size = 4
# font = "~/.local/share/fonts/MyFont-Bold.ttf"
```

- **`classic`**: terminal text; the word being sung is highlighted.
- **`karaoke`**: the line being sung is drawn as an image and fills in
  gradually, inside the letters, as each word is sung (like video karaoke).
  `m` in the Lyrics view switches between the two modes and saves the choice
  in the file (only the `mode` line of `[lyrics]` changes; comments and the
  rest stay as they are), so it is kept the next time.
- **Live reload**: saving the file applies `size` and `font` right away.
  `mode` applies when it changes in the file.
- **`size`** is the height, in rows, of the line being sung in karaoke mode.
  Bigger is easier to read from far away. A line that does not fit the width
  is drawn smaller.
- **Text size in classic mode cannot be changed by ketai**: there the lyrics
  are terminal cells, and their size is the terminal's font size, the same
  for the whole screen. Change it in the terminal (for example
  `ctrl+shift+equal` in kitty) or use karaoke mode.
- **`lines_before`** and **`lines_after`** are how many lines are shown
  above and below the one being sung, which stays in the middle of the view.
  Less than 1 (or negative) is 1; more than the maximum is the default (2
  above, 3 below).
- **`font`** is only used in karaoke mode. Without it, ketai asks fontconfig
  for a bold monospace font and uses others for the characters it does not
  have (for example Japanese or Korean).
- Karaoke mode needs a terminal that reports its cell size in pixels (kitty
  does, and so do the multiplexers that pass it on). If it does not, or no
  font is found, the view stays in classic mode.

## `[playlists]`

| Key | Values | Default |
|---|---|---|
| `dir` | absolute path, or one starting with `~` | `XDG_MUSIC_DIR/playlists` |

`XDG_MUSIC_DIR` comes from the environment, from `~/.config/user-dirs.dirs`
or, if it is in neither, it is `~/music`. The folder is created if missing.
The daemon reads it on every playlist operation: changing it applies without
restarting. If the file has an error, the daemon uses the default folder and
logs it. Playlists are stored in ketai's database; when the folder changes,
every playlist is copied to the new one.
