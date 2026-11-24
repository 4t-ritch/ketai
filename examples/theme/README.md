**English** | [Espanol](README.es.md)

# Themes

ketai reads the colors from the `[theme]` section of
`~/.config/ketai/config.toml` (or `$XDG_CONFIG_HOME/ketai/config.toml`).
Changes apply when you save the file, without closing ketai.

For now there is **a single active theme**: the one in `config.toml`. Keeping
several themes and choosing between them is planned for a major update (see
`docs/roadmap.md`).

In this folder:

| File | What it is for |
|---|---|
| `cover.toml` | The whole interface with the colors of the playing cover |
| `dark.toml` | A fixed dark theme, taking nothing from the cover |
| `cover-accent.toml` | Background from the cover and a fixed accent of your choice |
| `defaults.toml` | Every color with its default value, to copy and edit |

To use one, copy its `[theme]` section into your `config.toml`. If you
already have other sections (`[keys]`, `[playlists]`), leave them as they
are.

## Where the colors come from: `cover_color`

The first decision is `cover_color`:

```toml
[theme]
cover_color = true
```

| Value | What happens |
|---|---|
| `true` | The whole interface comes from the playing cover. Colors written by hand **are ignored**, even if they are in the file. |
| `false` | Only the colors written by hand count. Where it says `"cover"`, the default color is used. |
| no line | Each color decides on its own: `background` and `accent` can be `"cover"` or a fixed color. |

With `cover_color = true` (or with `background = "cover"` and
`accent = "cover"`) the colors change with every song:

- the **background** is the cover's average color, darkened;
- the **panels** (`raised`) and the **selected row** (`row`) are the same
  color a bit lighter;
- the **accent** is the same hue, light and saturated, so it reads well on
  the background;
- the **secondary text** (`dim`, `faint`) leans to the cover's hue;
- the **spectrum** goes from the accent to a neighboring hue.

If the cover is gray (no color), everything stays gray. While there is no
cover (nothing playing, or a song without an image), the defaults are used.

## Colors you can change

All of them are written as `"#rrggbb"` (hexadecimal, as in CSS).

| Key | What it paints | Default |
|---|---|---|
| `background` | The background of the whole screen. Also accepts `"cover"` | `"cover"` |
| `accent` | What is active: selected tab, cursor, buttons, progress bar. Also accepts `"cover"` | `"cover"` |
| `foreground` | Main text: titles, names | `"#ffffff"` |
| `dim` | Secondary text: artist, details | `"#c8d3e0"` |
| `faint` | Help text: shortcuts, labels, dates | `"#7f8ea3"` |
| `raised` | Panels and raised backgrounds, empty bars | `"#1c2a3a"` |
| `on_accent` | Text on the accent (for example, on a button) | `"#04121f"` |
| `row` | The row under the cursor | `"#16202e"` |
| `ok` | Healthy state (the "ketaid" connected dot) | `"#7ee787"` |
| `error` | Errors | `"#ff7b8a"` |
| `spectrum_top` | Top of the spectrum | `"#ff7ab6"` |

What is not in the table comes from these colors: the popup background is
the background a bit darker and the lyric lines not sung yet are `faint`
mixed with the background. Nothing in the interface has a fixed color of its
own.

`ok` and `error` are status colors: they do not follow the cover, so an error
always stands out.

## Useful combinations

**Everything from the cover** (`cover.toml`):

```toml
[theme]
cover_color = true
```

**Everything fixed** (`dark.toml`): `cover_color = false` and the colors you
want.

**Background from the cover with your accent** (`cover-accent.toml`): no
`cover_color`, with `background = "cover"` and `accent = "#rrggbb"`. With
`background = "cover"`, `raised`, `row`, `dim` and `faint` also come from the
cover, except those you set by hand: a color written by hand always wins.

## Errors and warnings

- **A badly written color** (`"#zzz"`, `"red"`) is an error: it shows on the
  last row with the key, and ketai keeps the previous colors until you fix
  it.
- **A key that does not exist** is reported and ignored; the rest is
  applied.
- **If you delete the file**, the default colors come back.

## Tips

- Try `cover_color = true` first: it looks best with most covers.
- If you write a fixed theme, keep good contrast between `background` and
  `foreground`, and between `accent` and `on_accent`.
- Changes show up when you save: keep ketai open while you edit.
