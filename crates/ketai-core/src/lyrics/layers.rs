//! Descubrimiento de capas de letra para una pista (docs/protocol.md,
//! seccion 11).
//!
//! Orden por capa: archivo local (sidecar) y despues letra embebida. Entre
//! archivos locales de la misma capa: `.ttml`, `.lrc`, `.txt`. Si un origen
//! no se puede leer, se pasa al siguiente.

use std::{
  fs,
  path::{Path, PathBuf},
};

use super::{Doc, Format, Kind, id3, lrc, text, ttml};
use crate::metadata;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
  Sidecar,
  Embedded,
}

impl Origin {
  pub fn as_str(self) -> &'static str {
    match self {
      Origin::Sidecar => "sidecar",
      Origin::Embedded => "embedded",
    }
  }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layer {
  pub id: String,
  pub kind: Kind,
  pub language: Option<String>,
  pub format: Format,
  pub origin: Origin,
  pub doc: Doc,
}

/// Resultado de buscar letras para una pista.
#[derive(Debug, Default)]
pub struct Found {
  /// La original primero, despues el resto ordenado por id.
  pub layers: Vec<Layer>,
  /// Hubo algun origen de letra pero ninguno se pudo leer.
  pub invalid: bool,
}

const SIDECAR_FORMATS: [(&str, Format); 3] = [("ttml", Format::Ttml), ("lrc", Format::Lrc), ("txt", Format::Text)];

/// Busca todas las capas de letra de un archivo de audio local.
pub fn discover(audio: &Path) -> Found {
  let mut found = Found::default();
  let mut sidecar = sidecars(audio, &mut found.invalid);
  let embedded = embedded(audio, &mut found.invalid);

  // Las capas extra de un TTML original cuentan como locales, pero un archivo
  // dedicado a esa capa (`cancion.es.lrc`) tiene prioridad.
  if let Some(original) = sidecar.iter().find(|l| l.id == "original" && l.format == Format::Ttml).cloned() {
    for extra in &original.doc.extra {
      if !sidecar.iter().any(|l| l.id == extra.language) {
        sidecar.push(Layer {
          id: extra.language.clone(),
          kind: extra.kind,
          language: Some(extra.language.clone()),
          format: Format::Ttml,
          origin: Origin::Sidecar,
          doc: extra.doc.clone(),
        });
      }
    }
  }

  let mut layers = sidecar;
  for layer in embedded {
    if !layers.iter().any(|l| l.id == layer.id) {
      layers.push(layer);
    }
  }
  // Sin capa original no hay ancla para sincronizar: no hay letra.
  if !layers.iter().any(|l| l.id == "original") {
    found.invalid |= !layers.is_empty();
    return found;
  }
  layers.sort_by(|a, b| (a.id != "original", &a.id).cmp(&(b.id != "original", &b.id)));
  found.layers = layers;
  found
}

/// Archivos locales `<stem>[.<etiqueta>].<ttml|lrc|txt>` junto al audio.
fn sidecars(audio: &Path, invalid: &mut bool) -> Vec<Layer> {
  let (Some(dir), Some(stem)) = (audio.parent(), audio.file_stem().and_then(|s| s.to_str())) else {
    return Vec::new();
  };
  let Ok(entries) = fs::read_dir(dir) else { return Vec::new() };
  // (id, formato, ruta)
  let mut candidates: Vec<(String, Format, PathBuf)> = Vec::new();
  for entry in entries.flatten() {
    let name = entry.file_name();
    let Some(name) = name.to_str() else { continue };
    let Some(rest) = name.strip_prefix(stem).and_then(|r| r.strip_prefix('.')) else { continue };
    let (tag, ext) = match rest.rsplit_once('.') {
      Some((tag, ext)) => (Some(tag), ext),
      None => (None, rest),
    };
    let Some((_, format)) = SIDECAR_FORMATS.iter().find(|(e, _)| e.eq_ignore_ascii_case(ext)) else { continue };
    let id = match tag {
      None => "original".to_string(),
      Some(t) if valid_tag(t) => t.to_string(),
      Some(_) => continue,
    };
    candidates.push((id, *format, entry.path()));
  }
  // Por capa, del formato mas rico al mas simple.
  candidates.sort_by_key(|(id, format, _)| (id.clone(), rank(*format)));
  let mut layers: Vec<Layer> = Vec::new();
  for (id, format, path) in candidates {
    if layers.iter().any(|l| l.id == id) {
      continue;
    }
    match fs::read_to_string(&path).ok().and_then(|t| parse(format, &t)) {
      Some((format, doc)) => layers.push(layer(&id, format, Origin::Sidecar, doc)),
      None => *invalid = true,
    }
  }
  layers
}

fn rank(f: Format) -> u8 {
  match f {
    Format::Ttml => 0,
    Format::Lrc => 1,
    Format::Text => 2,
  }
}

/// `romanized`, o una etiqueta BCP 47 simple (`es`, `ko-Latn`,
/// `ja-Latn-hepburn`).
fn valid_tag(tag: &str) -> bool {
  if tag == "romanized" {
    return true;
  }
  let mut parts = tag.split('-');
  let lang_ok = parts.next().is_some_and(|l| (2..=3).contains(&l.len()) && l.chars().all(|c| c.is_ascii_alphabetic()));
  lang_ok && parts.all(|p| !p.is_empty() && p.len() <= 8 && p.chars().all(|c| c.is_ascii_alphanumeric()))
}

/// Una etiqueta con script latino (`-Latn`) es romanizacion.
fn kind_of(id: &str) -> Kind {
  if id == "original" {
    Kind::Original
  } else if id == "romanized" || id.split('-').skip(1).any(|p| p.eq_ignore_ascii_case("latn")) {
    Kind::Romanized
  } else {
    Kind::Translation
  }
}

fn layer(id: &str, format: Format, origin: Origin, doc: Doc) -> Layer {
  let language = match id {
    "original" => doc.language.clone(),
    "romanized" => None,
    other => Some(other.to_string()),
  };
  Layer { id: id.to_string(), kind: kind_of(id), language, format, origin, doc }
}

/// Parsea segun el formato del archivo. Un `.lrc` sin marcas de tiempo se
/// toma como texto plano.
fn parse(format: Format, content: &str) -> Option<(Format, Doc)> {
  let (format, doc) = match format {
    Format::Ttml => (Format::Ttml, ttml::parse(content)?),
    Format::Lrc => match lrc::parse(content) {
      Some(doc) => (Format::Lrc, doc),
      None => (Format::Text, text::parse(content)),
    },
    Format::Text => (Format::Text, text::parse(content)),
  };
  (!doc.lines.is_empty()).then_some((format, doc))
}

/// Texto embebido: LRC si tiene marcas de tiempo, si no texto plano.
fn parse_embedded(content: &str) -> Option<(Format, Doc)> {
  parse(Format::Lrc, content)
}

fn embedded(audio: &Path, invalid: &mut bool) -> Vec<Layer> {
  let mut layers: Vec<Layer> = Vec::new();
  let mut push = |layers: &mut Vec<Layer>, id: &str, parsed: Option<(Format, Doc)>| {
    if layers.iter().any(|l| l.id == id) {
      return;
    }
    match parsed {
      Some((format, doc)) => layers.push(layer(id, format, Origin::Embedded, doc)),
      None => *invalid = true,
    }
  };

  // ID3: lector propio (USLT con descriptor y SYLT).
  if let Some(frames) = read_id3(audio) {
    // Para cada capa, la version sincronizada primero.
    let mut frames = frames;
    frames.sort_by_key(|f| !f.synced);
    for f in frames {
      let id = match f.descriptor.trim().to_ascii_lowercase().as_str() {
        "romanized" => "romanized".to_string(),
        "translation" => iso639_2_to_bcp47(&f.language),
        _ => "original".to_string(),
      };
      let format = if f.synced { Format::Lrc } else { Format::Text };
      let parsed = (!f.doc.lines.is_empty()).then_some((format, f.doc));
      push(&mut layers, &id, parsed);
    }
  }

  // Vorbis comments y M4A, via symphonia. `LYRICS` antes que
  // `UNSYNCEDLYRICS`; el USLT que ve symphonia ya se leyo arriba.
  let mut tags = metadata::lyrics_tags(audio);
  tags.retain(|(key, _, _)| !key.starts_with("USLT"));
  tags.sort_by_key(|(key, _, _)| key != "LYRICS");
  for (key, std, value) in tags {
    let id = if key == "LYRICS_ROMANIZED" {
      "romanized".to_string()
    } else if let Some(lang) = key.strip_prefix("LYRICS_TRANSLATION_") {
      lang.to_ascii_lowercase().replace('_', "-")
    } else if std {
      "original".to_string()
    } else {
      continue;
    };
    push(&mut layers, &id, parse_embedded(&value));
  }
  layers
}

/// Lee solo el tag ID3v2 del principio del archivo.
fn read_id3(audio: &Path) -> Option<Vec<id3::Frame>> {
  use std::io::Read;
  let mut file = fs::File::open(audio).ok()?;
  let mut header = [0u8; 10];
  file.read_exact(&mut header).ok()?;
  if &header[..3] != b"ID3" {
    return None;
  }
  let size = header[6..10].iter().fold(0usize, |acc, &b| (acc << 7) | (b & 0x7F) as usize);
  let mut data = header.to_vec();
  data.resize(10 + size, 0);
  file.read_exact(&mut data[10..]).ok()?;
  Some(id3::read(&data))
}

/// Codigos ISO 639-2 frecuentes a su forma corta BCP 47. Si no se conoce,
/// se usa el codigo de 3 letras, que tambien es BCP 47 valido.
fn iso639_2_to_bcp47(code: &str) -> String {
  let code = code.to_ascii_lowercase();
  let short = match code.as_str() {
    "spa" => "es",
    "eng" => "en",
    "jpn" => "ja",
    "kor" => "ko",
    "zho" | "chi" => "zh",
    "rus" => "ru",
    "deu" | "ger" => "de",
    "fra" | "fre" => "fr",
    "ita" => "it",
    "por" => "pt",
    other => return other.to_string(),
  };
  short.to_string()
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::lyrics::id3::tests::{sylt_frame, tag, uslt_frame};

  struct Dir(PathBuf);
  impl Drop for Dir {
    fn drop(&mut self) {
      let _ = fs::remove_dir_all(&self.0);
    }
  }

  fn dir(name: &str) -> Dir {
    let d = std::env::temp_dir().join(format!("ketai-lyrics-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    Dir(d)
  }

  fn ids(found: &Found) -> Vec<(&str, Kind, Format, Origin)> {
    found.layers.iter().map(|l| (l.id.as_str(), l.kind, l.format, l.origin)).collect()
  }

  #[test]
  fn sidecars_pick_richest_format_and_classify_tags() {
    let d = dir("sidecars");
    let audio = d.0.join("hype boy.ogg");
    fs::write(&audio, b"").unwrap();
    fs::write(d.0.join("hype boy.lrc"), "[00:01.00]a\n[00:02.00]b").unwrap();
    fs::write(d.0.join("hype boy.txt"), "a\nb").unwrap();
    fs::write(d.0.join("hype boy.es.txt"), "uno\ndos").unwrap();
    fs::write(d.0.join("hype boy.ko-Latn.lrc"), "[00:01.00]ro").unwrap();
    fs::write(d.0.join("hype boy.romanized.txt"), "x\ny").unwrap();
    fs::write(d.0.join("hype boy.backup!.lrc"), "[00:01.00]no").unwrap();
    fs::write(d.0.join("other.lrc"), "[00:01.00]no").unwrap();
    let found = discover(&audio);
    assert_eq!(
      ids(&found),
      [
        ("original", Kind::Original, Format::Lrc, Origin::Sidecar),
        ("es", Kind::Translation, Format::Text, Origin::Sidecar),
        ("ko-Latn", Kind::Romanized, Format::Lrc, Origin::Sidecar),
        ("romanized", Kind::Romanized, Format::Text, Origin::Sidecar),
      ]
    );
    assert!(!found.invalid);
  }

  #[test]
  fn broken_sidecar_falls_back_to_next_format() {
    let d = dir("fallback");
    let audio = d.0.join("s.ogg");
    fs::write(&audio, b"").unwrap();
    fs::write(d.0.join("s.ttml"), "<tt><p>broken").unwrap();
    fs::write(d.0.join("s.lrc"), "[00:01.00]ok").unwrap();
    let found = discover(&audio);
    assert_eq!(found.layers[0].format, Format::Lrc);
    assert!(found.invalid, "the broken file is reported");
  }

  #[test]
  fn no_original_means_no_lyrics() {
    let d = dir("no-original");
    let audio = d.0.join("s.ogg");
    fs::write(&audio, b"").unwrap();
    fs::write(d.0.join("s.es.txt"), "uno").unwrap();
    assert!(discover(&audio).layers.is_empty());
  }

  #[test]
  fn ttml_extra_layers_are_offered_unless_a_dedicated_file_exists() {
    let d = dir("ttml-extra");
    let audio = d.0.join("s.ogg");
    fs::write(&audio, b"").unwrap();
    let ttml = r#"<tt xml:lang="ja"><head><metadata><iTunesMetadata>
      <translations><translation xml:lang="es"><text for="L1">hola</text></translation>
      <translation xml:lang="en"><text for="L1">hi</text></translation></translations>
      </iTunesMetadata></metadata></head>
      <body><div><p begin="1.0" end="2.0" key="L1">x</p></div></body></tt>"#;
    fs::write(d.0.join("s.ttml"), ttml).unwrap();
    fs::write(d.0.join("s.es.txt"), "dedicada").unwrap();
    let found = discover(&audio);
    let got: Vec<(&str, Format)> = found.layers.iter().map(|l| (l.id.as_str(), l.format)).collect();
    assert_eq!(got, [("original", Format::Ttml), ("en", Format::Ttml), ("es", Format::Text)]);
    assert_eq!(found.layers[0].language.as_deref(), Some("ja"));
  }

  #[test]
  fn id3_frames_become_layers_with_synced_first() {
    let d = dir("id3");
    let audio = d.0.join("s.mp3");
    let data = tag(&[
      (b"USLT", uslt_frame("eng", "", "plain one")),
      (b"SYLT", sylt_frame("eng", "", &[("synced one", 1000)])),
      (b"USLT", uslt_frame("spa", "translation", "uno")),
      (b"USLT", uslt_frame("jpn", "romanized", "ichi")),
    ]);
    fs::write(&audio, data).unwrap();
    let found = discover(&audio);
    let got: Vec<(&str, Kind, Format, Origin)> = ids(&found);
    assert_eq!(
      got,
      [
        ("original", Kind::Original, Format::Lrc, Origin::Embedded),
        ("es", Kind::Translation, Format::Text, Origin::Embedded),
        ("romanized", Kind::Romanized, Format::Text, Origin::Embedded),
      ]
    );
    assert_eq!(found.layers[0].doc.lines[0].text, "synced one");
  }

  #[test]
  fn sidecar_wins_over_embedded_for_the_same_layer() {
    let d = dir("priority");
    let audio = d.0.join("s.mp3");
    fs::write(&audio, tag(&[(b"USLT", uslt_frame("eng", "", "embedded"))])).unwrap();
    fs::write(d.0.join("s.txt"), "local").unwrap();
    let found = discover(&audio);
    assert_eq!((found.layers[0].origin, found.layers[0].doc.lines[0].text.as_str()), (Origin::Sidecar, "local"));
  }

  #[test]
  fn tag_validation_and_iso_codes() {
    for ok in ["es", "ko-Latn", "ja-Latn-hepburn", "romanized", "spa"] {
      assert!(valid_tag(ok), "{ok}");
    }
    for bad in ["", "e", "backup!", "toolongtag", "es-", "1a"] {
      assert!(!valid_tag(bad), "{bad}");
    }
    assert_eq!(iso639_2_to_bcp47("spa"), "es");
    assert_eq!(iso639_2_to_bcp47("tgl"), "tgl");
  }
}

#[cfg(test)]
mod vorbis_tests {
  use super::*;

  #[test]
  fn vorbis_comments_become_layers() {
    let audio = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/vorbis_lyrics.ogg");
    let found = discover(&audio);
    let got: Vec<(&str, Kind, Format, Origin)> =
      found.layers.iter().map(|l| (l.id.as_str(), l.kind, l.format, l.origin)).collect();
    assert_eq!(
      got,
      [
        ("original", Kind::Original, Format::Lrc, Origin::Embedded),
        ("es", Kind::Translation, Format::Lrc, Origin::Embedded),
        ("romanized", Kind::Romanized, Format::Text, Origin::Embedded),
      ]
    );
    assert_eq!(found.layers[0].doc.lines[1].text, "world");
  }
}
