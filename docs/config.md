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

Shortcuts. Each action takes a key (`toggle = "x"`) or a list
(`next = ["n", "ctrl+right"]`); what you set replaces that action's default
keys and the rest stays the same.

### Which keys are allowed

Each shortcut is **one key**, optionally with modifiers in front, separated
by `+`. An action can have several shortcuts with a list
(`next = ["n", "ctrl+right"]`).

**Allowed:**

| Type | How to write it | Examples |
|---|---|---|
| A character | the character itself | `"n"`, `"P"`, `"+"`, `"*"`, `"!"`, `"["` |
| Space | `space` | `"space"` |
| Named keys | `enter`, `esc` (or `escape`), `tab`, `backspace` | `"enter"` |
| Arrows and navigation | `left`, `right`, `up`, `down`, `home`, `end`, `pageup`, `pagedown` | `"ctrl+right"` |
| Function keys | `f1` to `f12` | `"f5"`, `"alt+f2"` |
| Ctrl | `ctrl+` (or `control+`) in front | `"ctrl+p"`, `"ctrl+left"` |
| Alt | `alt+` in front | `"alt+x"`, `"alt+enter"` |
| Ctrl and Alt together | both in front, in any order | `"ctrl+alt+p"` |
| Shift with a letter | `shift+` and the letter, or the uppercase letter | `"shift+p"` is the same as `"P"` |
| Alt, Shift and a letter | `alt+shift+` and the letter | `"alt+shift+p"` (shown as `Alt+P`) |
| Shift and Tab | `shift+tab` | `"shift+tab"` |
| The `+` key | `"+"` alone; with modifiers, `"ctrl++"` | `"ctrl++"` |

Names and modifiers are not case sensitive (`"CTRL+P"` is `"ctrl+p"`).

**Not allowed** (each one is an error: it is shown on the last row with the
action and the key, and the previous configuration is kept):

| What | Example | Why |
|---|---|---|
| Key sequences | `"g g"`, `"ctrl+x ctrl+s"` | each shortcut is a single key (sequences are planned, see [roadmap.md](roadmap.md)) |
| Two keys at once without a modifier | `"a+b"` | only `ctrl`, `alt` and `shift` combine with a key |
| Shift with anything other than a letter or Tab | `"shift+left"`, `"shift+enter"`, `"shift+f5"`, `"shift+space"` | the terminal does not tell Shift apart there |
| Shift with a number or symbol | `"shift+1"`, `"shift+/"` | the terminal sends another character: write that one instead (`"!"`, `"?"`) |
| Ctrl, Shift and a letter | `"ctrl+shift+p"` | with Ctrl the terminal does not tell uppercase apart: it would be `"ctrl+p"` |
| Other modifiers | `"super+p"`, `"meta+p"`, `"cmd+p"` | the terminal does not send them |
| Other named keys | `"insert"`, `"delete"`, `"f13"` | not supported for now |
| Empty or several characters | `""`, `"ab"`, `"ctrl+"` | not a key |

**Allowed but with special rules:**

- `Ctrl+C` always quits, whatever is configured, so a broken configuration
  never leaves you without a way out.
- While typing in Search, keys without Ctrl or Alt are text (`toggle = "x"`
  types an x there); shortcuts with Ctrl or Alt still work.
- With Ctrl, uppercase and lowercase are the same key (`"ctrl+P"` is
  `"ctrl+p"`).
- A key cannot be in two actions; see the rules after the table.

### Actions

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
| `actions` | `ctrl+p` | Actions and quick search |
| `quit` | `q` | Quit |
| `next_tab` | `tab` | Next tab |
| `prev_tab` | `shift+tab` | Previous tab |
| `tab_now_playing` | `1` | Go to Now Playing |
| `tab_lyrics` | `2` | Go to Lyrics |
| `tab_library` | `3` | Go to Library |
| `tab_search` | `4` | Go to Search |
| `lyrics_layer` | `o` | Lyrics: change the primary layer |
| `lyrics_translation` | `t` | Lyrics: show or hide the translation |
| `lyrics_offset_down` | `[` | Lyrics: offset -50 ms |
| `lyrics_offset_up` | `]` | Lyrics: offset +50 ms |
| `lyrics_mode` | `k` | Lyrics: classic or karaoke mode |
| `lyrics_estimate` | `w` | Lyrics: estimate word timing |
| `queue_move_up` | `K` | Queue (Now Playing): move the chosen song up |
| `queue_move_down` | `J` | Queue (Now Playing): move the chosen song down |
| `queue_remove` | `x` | Queue (Now Playing): remove the chosen song |
| `queue_add_to_playlist` | `a` | Queue (Now Playing): add the chosen song to a playlist |

- `Ctrl+C` always quits, whatever else is configured.
- While typing in Search, keys without Ctrl or Alt are text.
- An invalid key is an error (it is shown and the previous configuration is
  kept); an unknown action is reported.
- **A repeated key (by the rule above) blocks the TUI**, at start or when the
  file is saved: a window lists every repeated key with its actions, grouped
  by key (it scrolls with the arrows, `j` / `k` or the wheel). Only scrolling
  and quitting (`q`, `Ctrl+C`) work; the music keeps playing. Fix `[keys]`
  and save: the TUI unlocks by itself, without restarting.
- The `lyrics_*` actions only work in the Lyrics view. A key can be used in
  different views (each one only works in its own view), but never in a view
  and a global action at the same time, in two global actions, or twice in
  the same view: that way every function stays reachable everywhere (for
  example `m` mutes in every view, Lyrics included).
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
  `k` in the Lyrics view switches between the two modes and saves the choice
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
