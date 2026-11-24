**English** | [Espanol](es/formats.md)

# Supported audio formats

## Support table

| Extension      | Container  | Codec             | Rust core                         | C core (FFmpeg) |
|----------------|------------|-------------------|-----------------------------------|-----------------|
| `.flac`        | FLAC       | FLAC              | symphonia                         | Yes |
| `.mp3`         | MPEG       | MP3               | symphonia                         | Yes |
| `.m4a`         | MP4        | AAC-LC            | FDK AAC (adapter)                 | Yes |
| `.m4a`/`.aac`  | MP4 / ADTS | HE-AAC v1 and v2  | FDK AAC (adapter)                 | Yes |
| `.m4a`         | MP4        | ALAC              | symphonia                         | Yes |
| `.ogg`         | Ogg        | Vorbis            | symphonia                         | Yes |
| `.ogg`/`.opus` | Ogg        | Opus              | libopus (adapter)                 | Yes |
| `.wav`         | RIFF       | PCM               | symphonia                         | Yes |

## How it works in the Rust core

- rodio always uses symphonia's default codec registry, which does not include
  Opus or HE-AAC. That is why ketai decodes with symphonia directly in
  `crates/ketai-core/src/decoder.rs` and hands the audio to rodio as its own
  source.
- The codec registry is built like this: first symphonia's codecs, then
  `symphonia-adapter-fdk-aac` (it replaces the native AAC, which only supports
  AAC-LC) and `symphonia-adapter-libopus`.
- The `0.1.x` and `0.2.x` versions of the adapters are used because they are
  the ones compatible with symphonia 0.5, the version rodio 0.22 uses.
- Channels and sample rate are taken from the first decoded packet, because
  MP4 with AAC does not always declare them and with HE-AAC the container may
  declare the core rate instead of the real one.

## Build dependencies

- `libopus` and `fdk-aac` are built from source (`bundled` feature), so
  `cmake` and a C compiler are needed.

## Licenses

- FDK AAC uses the "Fraunhofer FDK AAC Codec Library" license, which is not
  GPL-compatible. This must be taken into account before distributing
  binaries.

## Rule

The daemon detects the format by content (probe), not by extension. If a file
cannot be decoded, the operation fails with protocol error `1003`.

## Tests

The files in `crates/ketai-core/tests/fixtures/` are 2-second 440 Hz tones:

- `opus.ogg`, `vorbis.ogg` and `aac_lc.m4a` were generated with `ffmpeg`.
- `he_aac.m4a` was generated with the FDK encoder (HE-AAC profile) and
  remuxed to MP4 with `ffmpeg -c copy`, because ffmpeg's AAC encoder only
  produces AAC-LC.
