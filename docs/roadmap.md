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
