//! Lectura de metadatos de un archivo de audio.

use std::{fs::File, path::Path};

use symphonia::core::{
  formats::FormatOptions,
  io::MediaSourceStream,
  meta::{MetadataOptions, MetadataRevision, StandardTagKey},
  probe::Hint,
};

use crate::decoder::{DecodeError, codecs};

#[derive(Debug, Clone, Default, PartialEq)]
pub struct TrackInfo {
  pub title: Option<String>,
  pub artist: Option<String>,
  pub album: Option<String>,
  pub duration_ms: Option<u64>,
  pub has_cover: bool,
  pub codec: Option<String>,
  pub sample_rate: Option<u32>,
  pub channels: Option<u16>,
}

/// Lee los metadatos y comprueba que exista un decoder para el track.
/// Falla si el archivo no se puede reproducir.
pub fn probe(path: &Path) -> Result<TrackInfo, DecodeError> {
  let file = File::open(path)?;
  let stream = MediaSourceStream::new(Box::new(file), Default::default());
  let mut hint = Hint::new();
  if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
    hint.with_extension(ext);
  }
  let mut probed = symphonia::default::get_probe().format(
    &hint,
    stream,
    &FormatOptions::default(),
    &MetadataOptions::default(),
  )?;

  let mut info = TrackInfo::default();

  // Tags fuera del contenedor (por ejemplo ID3 delante de un MP3) y despues
  // los del contenedor; los segundos tienen prioridad.
  if let Some(meta) = probed.metadata.get()
    && let Some(rev) = meta.current()
  {
    apply_tags(&mut info, rev);
  }
  if let Some(rev) = probed.format.metadata().current() {
    apply_tags(&mut info, rev);
  }

  let track = probed.format.default_track().ok_or(DecodeError::NoTrack)?;
  let params = &track.codec_params;
  let descriptor = codecs().get_codec(params.codec).ok_or(DecodeError::InvalidParams)?;
  info.codec = Some(descriptor.short_name.to_string());
  info.sample_rate = params.sample_rate;
  info.channels = params.channels.and_then(|c| u16::try_from(c.count()).ok());
  info.duration_ms = params
    .time_base
    .zip(params.n_frames)
    .map(|(base, frames)| {
      let t = base.calc_time(frames);
      t.seconds * 1000 + (t.frac * 1000.0) as u64
    });
  Ok(info)
}

/// Tags de letras de un archivo: `(clave, es_letra_estandar, valor)`.
///
/// `es_letra_estandar` es true para lo que symphonia reconoce como letra
/// (Vorbis `LYRICS`/`UNSYNCEDLYRICS`, M4A `(c)lyr`, ID3 `USLT`). Las claves
/// propias de ketai (`LYRICS_ROMANIZED`, `LYRICS_TRANSLATION_ES`) llegan con
/// su nombre crudo.
pub fn lyrics_tags(path: &Path) -> Vec<(String, bool, String)> {
  let Ok(file) = File::open(path) else { return Vec::new() };
  let stream = MediaSourceStream::new(Box::new(file), Default::default());
  let mut hint = Hint::new();
  if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
    hint.with_extension(ext);
  }
  let Ok(mut probed) =
    symphonia::default::get_probe().format(&hint, stream, &FormatOptions::default(), &MetadataOptions::default())
  else {
    return Vec::new();
  };
  let mut out = Vec::new();
  let mut collect = |rev: &MetadataRevision| {
    for tag in rev.tags() {
      let std = tag.std_key == Some(StandardTagKey::Lyrics);
      let key = tag.key.to_ascii_uppercase();
      if std || key.starts_with("LYRICS") {
        out.push((key, std, tag.value.to_string()));
      }
    }
  };
  if let Some(meta) = probed.metadata.get()
    && let Some(rev) = meta.current()
  {
    collect(rev);
  }
  if let Some(rev) = probed.format.metadata().current() {
    collect(rev);
  }
  out
}

fn apply_tags(info: &mut TrackInfo, rev: &MetadataRevision) {
  for tag in rev.tags() {
    let value = tag.value.to_string();
    if value.trim().is_empty() {
      continue;
    }
    match tag.std_key {
      Some(StandardTagKey::TrackTitle) => info.title = Some(value),
      Some(StandardTagKey::Artist) => info.artist = Some(value),
      Some(StandardTagKey::Album) => info.album = Some(value),
      _ => {}
    }
  }
  if !rev.visuals().is_empty() {
    info.has_cover = true;
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::path::PathBuf;

  fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(name)
  }

  #[test]
  fn reads_codec_and_stream_info() {
    let info = probe(&fixture("vorbis.ogg")).unwrap();
    assert_eq!(info.codec.as_deref(), Some("vorbis"));
    assert_eq!(info.channels, Some(2));
    let ms = info.duration_ms.unwrap();
    assert!((1900..2100).contains(&ms), "duration {ms}");
  }

  #[test]
  fn rejects_non_audio_files() {
    assert!(probe(Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml").as_path()).is_err());
  }
}
