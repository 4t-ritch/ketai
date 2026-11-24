**English** | [Espanol](es/roadmap.md)

# Roadmap

Features planned beyond v1.

## Next major update

### Remote sources with download to cache

v1 only plays local files (`file://`). This update adds remote tracks.

- `http(s)://` sources pointing to audio files. Before starting, it is decided
  whether any other kind of source is added.
- A synchronous HTTP client with TLS (for example `ureq` with `rustls`), on
  its own thread, no async.
- Download to `$XDG_CACHE_HOME/ketai/` with a size limit; playback starts
  while downloading (`status: "buffering"`).
- The `download.progress` event (topic `cache`), already reserved in the
  protocol.
- Lyrics and covers for remote tracks.

### Installation with package managers

Today ketai is built from source. This update makes it installable with one
command, with the daemon and the TUI together.

- Packages for the most used Linux package managers (for example a PKGBUILD
  for the AUR, and packages for other distributions to be decided), with the
  binaries, the systemd user units, the documentation and the configuration
  examples.
- After installing, the service is ready to be enabled
  (`systemctl --user enable --now ketaid.socket`).
- Prebuilt binaries for every release, for those who do not want to build.
- Before starting, the FDK AAC license is reviewed for distributing binaries
  (see [formats.md](formats.md#licenses)).

### Multiple themes

v1 has a single active theme: the `[theme]` section of `config.toml`. This
update allows keeping several and choosing which one to use.

- Themes in their own files (for example
  `~/.config/ketai/themes/<name>.toml`), with the same keys as `[theme]`.
- Choosing the active theme from `config.toml` and from the TUI (for example,
  in the actions popup), with live reload.
- Themes bundled with ketai and a way to share them.
- Before starting, it is decided how a chosen theme and `cover_color` live
  together (for example, a theme that says which colors it takes from the
  cover).

### Multiple interface languages

v1 shows the interface in Spanish (with a few texts in English, such as
"Default" and "No device" for the audio output). This update allows choosing
the language.

- Every TUI text comes from a catalog per language, instead of being written
  in the code.
- The language is chosen in `config.toml` (for example
  `[ui] language = "en"`) or taken from the system (`LANG`), with live
  reload.
- Initial languages: Spanish and English. Adding a new one is adding a
  catalog.
- Before starting, it is decided what happens with the daemon's messages
  (protocol errors): today they are in English and clients could translate
  them by error code.

### Key sequences

Today each shortcut is a single key, with modifiers (`ctrl+p`). This update
allows shortcuts made of several keys pressed one after the other, like in
Vim or Emacs.

- Sequences such as `g g` or `ctrl+x ctrl+s` in `[keys]`
  (`top = "g g"`).
- While a sequence is half typed, the TUI waits for the next key for a
  short time and shows what has been typed so far.
- The repeated keys check (which blocks the TUI) also covers sequences: a
  sequence cannot start with a key that is already a whole shortcut,
  because that shortcut would always win.
- Before starting, it is decided how long to wait between keys and whether
  it can be configured.

### Graphical karaoke client (video and shaders)

A separate window client, for example `ketai-karaoke`, that connects to the
same `ketaid` through the protocol, like the TUI does. It is the place for
what a terminal cannot do well: GPU shaders and video at 60 frames per
second.

- **Karaoke lyrics drawn with shaders**: each word fills in gradually, inside
  the letters, as it is sung, like video karaoke.
- **Optional background video** behind the lyrics, enabled or disabled in
  `config.toml`. Before starting, it is decided where the video comes from
  (for example a file next to the song, or a video configured by the user).
- Built with a GPU library such as `wgpu`; the video is decoded with a
  library such as FFmpeg.
- **Precise sync**: today the daemon reports the position once per second
  and clients interpolate it. Video needs a more precise clock, so the
  protocol may gain a finer position event or a timestamp per report; it is
  an optional addition and does not break existing clients.
- The TUI keeps its own lyrics view. Video does not go in the TUI: in a
  terminal each frame is a full image sent as data, which is heavy and
  unstable inside multiplexers such as herdr.

### Automatic lyrics romanization

Generate romanized lyrics when the source does not include them.

- **Korean** (hangul): solved with rules, no dictionary. Hangul is syllabic
  and the revised romanization is applied per block.
- **Cyrillic** (Russian and others): solved with transliteration tables per
  language.
- **Japanese**: needs a dictionary to read kanji (for example MeCab or
  kakasi). Kana are solved with tables.
- **Chinese**: needs a pinyin dictionary, and characters with several
  readings need context.

Impact on the protocol:

- Generated layers show up in `lyrics.layers` with `origin: "generated"`.
- Being generated text, it must be possible to turn it off per language in
  the configuration.

### Dictionary-based word timing estimation

Replace the proportional-by-characters estimation (`word-estimated`) with a
phonetic estimation, without depending on AI, to get a fairly close first
result.

Idea:

- With the dictionaries of the automatic romanization, split each line into
  phonetic units (morae in Japanese, syllables in Korean and Chinese,
  syllables in languages with a Latin or Cyrillic alphabet).
- Give each unit a duration with a simple model per language. Sustained
  vowels take most of a line's time; if the line's duration and the
  sustained ones are known, the rest is spread across the short units.
- The initial hypothesis (to be validated with real word-synced lyrics) is
  that this split gets it right in the vast majority of cases for
  romanizations.

Phases:

1. **Romanization**: estimate the timing of a romanized layer from each
   line's duration. It is the case with the smallest margin of error because
   the romanization follows the original's phonetics.
2. **Translation**: more steps and a larger margin of error. One possible
   approach is to work with romanization pairs between languages to align
   segments before spreading the timing.

Impact on the protocol:

- A new timing level, for example `word-phonetic`, distinct from
  `word-estimated`, so the client knows which method was used.
- Measure the precision against a set of lyrics with real timing before
  turning it on by default.
