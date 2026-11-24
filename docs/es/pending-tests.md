[English](../pending-tests.md) | **Espanol**

# Pruebas pendientes por falta de archivos

Pruebas escritas que todavia no se corrieron porque faltan archivos reales.
Los archivos tienen derechos de autor y no entran al repo: van en
`samples/`, que git ignora.

## Letras ID3 en MP3 reales

- **Que falta**: MP3 con letra embebida (`USLT` o `SYLT`) etiquetados por
  otros programas (Mp3tag, foobar2000, MusicBrainz Picard, kid3, etc.).
- **Por que**: el lector ID3 propio solo se probo con etiquetas armadas en
  los tests. Programas distintos usan versiones (2.3 o 2.4), codificaciones
  y descriptores distintos.
- **Donde van**: `samples/lyrics/id3/*.mp3`.
- **Test**: `real_id3_lyrics_are_read` en
  `crates/ketai-core/tests/real_assets.rs`.

## TTML de Apple Music

- **Que falta**: archivos TTML reales exportados de Apple Music, idealmente
  con duetos, coros de fondo, traducciones y transliteraciones.
- **Por que**: el parser TTML solo se probo con ejemplos escritos a mano.
- **Donde van**: `samples/lyrics/ttml/*.ttml`.
- **Test**: `real_apple_music_ttml_is_parsed` en
  `crates/ketai-core/tests/real_assets.rs`.

## Como correrlas

```sh
cargo test -p ketai-core --test real_assets -- --ignored --nocapture
```

Los tests estan marcados `#[ignore]`: la suite normal los salta. Corridos a
mano, fallan con un mensaje claro si la carpeta no existe o esta vacia, para
que nunca pasen sin haber probado nada. Con `--nocapture` muestran las capas
encontradas para revisarlas a ojo contra el programa que las etiqueto.
