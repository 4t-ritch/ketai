**English** | [Espanol](README.es.md)

# Keyboard shortcuts

ketai reads the global shortcuts from the `[keys]` section of
`~/.config/ketai/config.toml` (or `$XDG_CONFIG_HOME/ketai/config.toml`).
Changes apply when you save the file, without closing ketai.

In this folder:

| File | What it is for |
|---|---|
| `defaults.toml` | Every shortcut with its default value, to copy and edit |
| `vim.toml` | vim-style movement, with Alt so it does not clash with the lists |
| `media.toml` | Playback with Ctrl, Alt and arrows, no plain letters |
| `minimal.toml` | Changes only two shortcuts; the rest stays default |

To use one, copy its `[keys]` section into your `config.toml`. If you already
have other sections (`[theme]`, `[playlists]`), leave them as they are: each
section is independent.

## How to write an action

```toml
[keys]
toggle = "space"              # one key
next = ["n", "ctrl+right"]    # several keys for the same action
```

- The name on the left is the action (see the table below).
- On the right goes one key, or a list of keys between brackets.
- What you set **replaces** that action's default keys. To add a key without
  losing the usual one, put both in the list.
- Actions that are not listed keep their default key.

## How to write a key

| Type | Examples |
|---|---|
| A character | `"n"`, `"P"` (uppercase = with Shift), `"+"`, `"*"`, `"/"` |
| A named key | `"space"`, `"enter"`, `"esc"`, `"tab"`, `"backspace"` |
| Arrows and navigation | `"left"`, `"right"`, `"up"`, `"down"`, `"home"`, `"end"`, `"pageup"`, `"pagedown"` |
| Function keys | `"f1"` to `"f12"` |
| With modifiers | `"ctrl+p"`, `"alt+x"`, `"shift+tab"`, `"ctrl+right"` |

- Modifiers go in front, separated by `+`: `ctrl+`, `alt+`, `shift+`. They
  can be combined: `"ctrl+alt+p"`.
- With Ctrl, uppercase and lowercase are the same key (`"ctrl+p"` and
  `"ctrl+P"` are equal): the terminal does not tell them apart.
- For the `+` key alone write `"+"`; with Ctrl, `"ctrl++"`.

## Actions

| Action | Default | What it does |
|---|---|---|
| `toggle` | `space` | Play or pause |
| `next` | `n` | Next song |
| `previous` | `p` | Previous song |
| `seek_forward` | `right` | Forward 5 seconds |
| `seek_back` | `left` | Back 5 seconds |
| `volume_up` | `+`, `=` | Volume up |
| `volume_down` | `-` | Volume down |
| `mute` | `m` | Mute or unmute |
| `shuffle` | `s` | Shuffle |
| `repeat` | `r` | Repeat: none, track, queue |
| `devices` | `d` | Choose the audio output |
| `folders` | `f` | Library folders |
| `favorite` | `*` | Mark or unmark the playing song as a favorite |
| `add_to_playlist` | `P` | Add the playing song to a playlist |
| `actions` | `ctrl+p`, `ctrl+k`, `/` | Actions popup and quick search |
| `quit` | `q` | Quit |
| `next_tab` | `tab` | Next tab |
| `prev_tab` | `shift+tab` | Previous tab |
| `tab_now_playing` | `1` | Go to Now Playing |
| `tab_lyrics` | `2` | Go to Lyrics |
| `tab_library` | `3` | Go to Library |
| `tab_search` | `4` | Go to Search |
| `lyrics_layer` | `o` | Lyrics: change the primary layer |
| `lyrics_translation` | `t` | Lyrics: show or hide the translation |
| `lyrics_offset_down` / `lyrics_offset_up` | `[` / `]` | Lyrics: offset -50 / +50 ms |
| `lyrics_mode` | `k` | Lyrics: classic or karaoke mode |
| `lyrics_estimate` | `w` | Lyrics: estimate word timing |

## Rules worth knowing

- **Ctrl+C always quits**, even if you use it for something else or change
  `quit`. That way a broken configuration never leaves you without a way to
  close ketai.
- **While typing in Search**, keys without Ctrl or Alt are text: with
  `toggle = "x"`, typing "x" in Search types the letter. Shortcuts with Ctrl
  or Alt work the same while typing.
- **A badly written key** (`"ctrl+banana"`) is an error: it shows on the last
  row of the screen with the action and the key, and ketai keeps the previous
  configuration until you fix it.
- **An action that does not exist** (`dance = "x"`) is reported and ignored;
  the rest is applied.
- **A repeated key blocks ketai** until you fix it: the same key in two
  global actions, in a global action and a view action (`lyrics_*`), or
  twice in the same view. A window lists every repeated key with its
  actions; save the fixed file and ketai unlocks by itself. The same key in
  two different views is fine.
- **The other keys of each view are fixed for now**: `h` / `l` to change
  view in Library, `a` to add to the queue, `c` to create a
  playlist, `x` to remove, and `Esc`, `Enter` and the arrows inside lists and
  popups. See `docs/tui.md`.

## Common problems

- **"Nothing happens when I press the key"**: check the last row for a
  repeated-key warning; another action may be using it.
- **A shortcut with Shift does not work**: for letters write the uppercase
  letter (`"P"`), not `"shift+p"`. `shift+` is for named keys
  (`"shift+tab"`).
- **Ctrl with some letters is another key**: to the terminal, `ctrl+m` is
  Enter, `ctrl+h` is Backspace, `ctrl+j` is a newline, `ctrl+i` is Tab and
  `ctrl+[` is Esc. Do not use them for shortcuts; prefer Alt (`"alt+m"`).
- **Some combinations never arrive**: the terminal or the multiplexer
  (herdr, tmux) may take them before they reach ketai. For example, many
  multiplexers use `ctrl+b` or `ctrl+a`. Choose another combination.
