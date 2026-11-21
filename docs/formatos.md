# Formatos de audio soportados

## Tabla de soporte

| Extension      | Contenedor | Codec             | Core Rust                         | Core C (FFmpeg) |
|----------------|------------|-------------------|-----------------------------------|-----------------|
| `.flac`        | FLAC       | FLAC              | symphonia                         | Si |
| `.mp3`         | MPEG       | MP3               | symphonia                         | Si |
| `.m4a`         | MP4        | AAC-LC            | FDK AAC (adaptador)               | Si |
| `.m4a`/`.aac`  | MP4 / ADTS | HE-AAC v1 y v2    | FDK AAC (adaptador)               | Si |
| `.m4a`         | MP4        | ALAC              | symphonia                         | Si |
| `.ogg`         | Ogg        | Vorbis            | symphonia                         | Si |
| `.ogg`/`.opus` | Ogg        | Opus              | libopus (adaptador)               | Si |
| `.wav`         | RIFF       | PCM               | symphonia                         | Si |

## Como funciona en el core Rust

- rodio siempre usa el registro de codecs por defecto de symphonia, que no
  incluye Opus ni HE-AAC. Por eso ketai decodifica con symphonia directamente
  en `crates/ketai-core/src/decoder.rs` y entrega el audio a rodio como una fuente propia.
- El registro de codecs se arma asi: primero los codecs de symphonia, despues
  `symphonia-adapter-fdk-aac` (reemplaza al AAC nativo, que solo soporta
  AAC-LC) y `symphonia-adapter-libopus`.
- Se usan las versiones `0.1.x` y `0.2.x` de los adaptadores porque son las
  compatibles con symphonia 0.5, que es la version que usa rodio 0.22.
- Canales y frecuencia de muestreo se toman del primer paquete decodificado,
  porque MP4 con AAC no siempre los declara y en HE-AAC el contenedor puede
  declarar la frecuencia del nucleo en lugar de la real.

## Dependencias de compilacion

- `libopus` y `fdk-aac` se compilan desde su codigo fuente (feature
  `bundled`), por lo que hace falta `cmake` y un compilador de C.

## Licencias

- FDK AAC usa la licencia "Fraunhofer FDK AAC Codec Library", que no es
  compatible con GPL. Hay que tenerlo en cuenta antes de distribuir binarios.

## Regla

El daemon detecta el formato por el contenido (probe), no por la extension.
Si un archivo no se puede decodificar, la operacion falla con el error `1003`
del protocolo.

## Pruebas

Los archivos en `crates/ketai-core/tests/fixtures/` son tonos de 440 Hz de 2 segundos:

- `opus.ogg`, `vorbis.ogg` y `aac_lc.m4a` se generaron con `ffmpeg`.
- `he_aac.m4a` se genero con el encoder de FDK (perfil HE-AAC) y se
  remultiplexo a MP4 con `ffmpeg -c copy`, porque el encoder AAC de ffmpeg
  solo produce AAC-LC.
