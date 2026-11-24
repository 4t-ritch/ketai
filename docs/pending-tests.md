**English** | [Espanol](es/pending-tests.md)

# Tests pending for lack of files

Tests already written that have not run yet because real files are missing.
The files are copyrighted and do not go into the repo: they go in `samples/`,
which git ignores.

## ID3 lyrics in real MP3s

- **What is missing**: MP3s with embedded lyrics (`USLT` or `SYLT`) tagged by
  other programs (Mp3tag, foobar2000, MusicBrainz Picard, kid3, etc.).
- **Why**: ketai's own ID3 reader has only been tested with tags built in the
  tests. Different programs use different versions (2.3 or 2.4), encodings
  and descriptors.
- **Where they go**: `samples/lyrics/id3/*.mp3`.
- **Test**: `real_id3_lyrics_are_read` in
  `crates/ketai-core/tests/real_assets.rs`.

## Apple Music TTML

- **What is missing**: real TTML files exported from Apple Music, ideally
  with duets, backing vocals, translations and transliterations.
- **Why**: the TTML parser has only been tested with hand-written examples.
- **Where they go**: `samples/lyrics/ttml/*.ttml`.
- **Test**: `real_apple_music_ttml_is_parsed` in
  `crates/ketai-core/tests/real_assets.rs`.

## How to run them

```sh
cargo test -p ketai-core --test real_assets -- --ignored --nocapture
```

The tests are marked `#[ignore]`: the normal suite skips them. Run by hand,
they fail with a clear message if the folder is missing or empty, so they
never pass without testing anything. With `--nocapture` they print the layers
found, to check them by eye against the program that tagged them.
