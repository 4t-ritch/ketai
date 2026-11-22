//! Lectura de metadatos de un archivo de audio.

use std::{fs::File, path::Path};

use symphonia::core::{
  formats::FormatOptions,
  io::MediaSourceStream,
  meta::{MetadataOptions, MetadataRevision, StandardTagKey, StandardVisualKey},
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
  if !info.has_cover {
    info.has_cover = folder_cover(path).is_some();
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

/// Imagen de caratula: bytes y tipo MIME.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cover {
  pub data: Vec<u8>,
  pub mime: String,
}

/// Caratula de una pista: la embebida (preferentemente la de tapa) y si no
/// hay, una imagen de la carpeta (`cover.jpg`, `folder.png`...).
pub fn cover(path: &Path) -> Option<Cover> {
  embedded_cover(path).or_else(|| {
    let image = folder_cover(path)?;
    let data = std::fs::read(&image).ok()?;
    let ext = image.extension()?.to_str()?.to_ascii_lowercase();
    let mime = match ext.as_str() {
      "png" => "image/png",
      "webp" => "image/webp",
      _ => "image/jpeg",
    };
    Some(Cover { data, mime: mime.to_string() })
  })
}

fn embedded_cover(path: &Path) -> Option<Cover> {
  let file = File::open(path).ok()?;
  let stream = MediaSourceStream::new(Box::new(file), Default::default());
  let mut hint = Hint::new();
  if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
    hint.with_extension(ext);
  }
  let mut probed =
    symphonia::default::get_probe().format(&hint, stream, &FormatOptions::default(), &MetadataOptions::default()).ok()?;
  let mut found: Vec<Cover> = Vec::new();
  let mut front: Option<Cover> = None;
  let mut collect = |rev: &MetadataRevision| {
    for v in rev.visuals() {
      let c = Cover { data: v.data.to_vec(), mime: v.media_type.clone() };
      if v.usage == Some(StandardVisualKey::FrontCover) && front.is_none() {
        front = Some(c.clone());
      }
      found.push(c);
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
  front.or_else(|| found.into_iter().next())
}

/// Imagen de caratula en la carpeta de la pista. Los nombres se comparan sin
/// distinguir mayusculas; si hay varias, gana la primera de esta lista.
pub fn folder_cover(path: &Path) -> Option<std::path::PathBuf> {
  const NAMES: [&str; 4] = ["cover", "folder", "front", "album"];
  const EXTS: [&str; 4] = ["jpg", "jpeg", "png", "webp"];
  let dir = path.parent()?;
  let mut best: Option<(usize, std::path::PathBuf)> = None;
  for entry in std::fs::read_dir(dir).ok()?.flatten() {
    let p = entry.path();
    let (Some(stem), Some(ext)) = (p.file_stem().and_then(|s| s.to_str()), p.extension().and_then(|e| e.to_str())) else {
      continue;
    };
    let (stem, ext) = (stem.to_ascii_lowercase(), ext.to_ascii_lowercase());
    if !EXTS.contains(&ext.as_str()) || !p.is_file() {
      continue;
    }
    if let Some(rank) = NAMES.iter().position(|n| *n == stem)
      && best.as_ref().is_none_or(|(r, _)| rank < *r)
    {
      best = Some((rank, p));
    }
  }
  best.map(|(_, p)| p)
}

/// Limpia el valor de una etiqueta: algunos programas dejan un BOM
/// (`U+FEFF`) al principio o espacios alrededor. None si queda vacio.
fn clean_tag(value: &str) -> Option<String> {
  let v = value.trim_matches(|c: char| c == '\u{feff}' || c.is_whitespace());
  (!v.is_empty()).then(|| v.to_string())
}

fn apply_tags(info: &mut TrackInfo, rev: &MetadataRevision) {
  for tag in rev.tags() {
    let Some(value) = clean_tag(&tag.value.to_string()) else { continue };
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
  fn tag_values_lose_bom_and_surrounding_spaces() {
    assert_eq!(clean_tag("\u{feff}#song title  ").as_deref(), Some("#song title"));
    assert_eq!(clean_tag(" \u{feff} "), None);
    assert_eq!(clean_tag("plain").as_deref(), Some("plain"));
  }

  #[test]
  fn reads_tags_and_finds_embedded_or_folder_covers() {
    let info = probe(&fixture("tagged_cover.mp3")).unwrap();
    assert_eq!(
      (info.title.as_deref(), info.artist.as_deref(), info.album.as_deref()),
      (Some("Red Song"), Some("Test Band"), Some("Fixture Album"))
    );
    assert!(info.has_cover);
    let c = cover(&fixture("tagged_cover.mp3")).unwrap();
    assert_eq!(c.mime, "image/png");
    assert!(cover(&fixture("opus.ogg")).is_none());
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
