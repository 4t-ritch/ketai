//! Biblioteca local: que archivos se indexan y como se buscan.

use std::{
  fs,
  path::{Path, PathBuf},
};

use crate::metadata::TrackInfo;

/// Extensiones que se consideran audio (en minusculas).
pub const EXTENSIONS: [&str; 9] = ["flac", "mp3", "m4a", "mp4", "aac", "ogg", "oga", "opus", "wav"];

/// Una pista indexada.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
  pub path: PathBuf,
  pub info: TrackInfo,
}

/// Archivos de audio bajo `root`, recursivo y ordenado. No sigue carpetas
/// enlazadas (evita ciclos); los archivos enlazados si se incluyen.
pub fn candidates(root: &Path) -> Vec<PathBuf> {
  let mut out = Vec::new();
  let mut pending = vec![root.to_path_buf()];
  while let Some(dir) = pending.pop() {
    let Ok(entries) = fs::read_dir(&dir) else { continue };
    for e in entries.flatten() {
      let path = e.path();
      let Ok(kind) = e.file_type() else { continue };
      if kind.is_dir() {
        pending.push(path);
      } else if is_audio(&path) && (kind.is_file() || path.is_file()) {
        out.push(path);
      }
    }
  }
  out.sort();
  out
}

fn is_audio(path: &Path) -> bool {
  path
    .extension()
    .and_then(|e| e.to_str())
    .is_some_and(|e| EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
}

/// Texto plegado para buscar: minusculas y sin marcas diacriticas
/// (acentos, dieresis, virgulilla, cedilla...), asi "cancion" encuentra
/// el titulo con o acentuada (U+00F3) y al reves.
///
/// Se descompone con NFD, se quitan solo las marcas combinantes de U+0300 a
/// U+036F (latin, griego, cirilico) y se vuelve a componer con NFC. Las
/// marcas de otras escrituras se respetan y vuelven a su silaba: el dakuten
/// japones (U+3099) cambia la palabra ("ga" no es "ka") y el hangul queda en
/// silabas completas. Las letras que no se descomponen se pliegan a mano:
/// U+00DF = ss, U+00E6 = ae, U+0153 = oe, U+00F8 = o, U+0142 = l, U+0111 = d.
pub fn fold(text: &str) -> String {
  use unicode_normalization::UnicodeNormalization;
  let mut out = String::with_capacity(text.len());
  for c in text.nfd() {
    if ('\u{0300}'..='\u{036f}').contains(&c) {
      continue;
    }
    match c {
      'ß' | 'ẞ' => out.push_str("ss"),
      'æ' | 'Æ' => out.push_str("ae"),
      'œ' | 'Œ' => out.push_str("oe"),
      'ø' | 'Ø' => out.push('o'),
      'ł' | 'Ł' => out.push('l'),
      'đ' | 'Đ' | 'ð' | 'Ð' => out.push('d'),
      'þ' | 'Þ' => out.push_str("th"),
      'ı' => out.push('i'),
      _ => out.extend(c.to_lowercase()),
    }
  }
  out.nfc().collect()
}

/// Palabras de una consulta, plegadas (ver `fold`).
pub fn words(query: &str) -> Vec<String> {
  query.split_whitespace().map(fold).collect()
}

/// true si cada palabra aparece, en cualquier parte, en el titulo,
/// artista, album o nombre de archivo, sin importar mayusculas ni acentos.
pub fn matches(entry: &Entry, words: &[String]) -> bool {
  matches_text(&search_text(entry), words)
}

/// Texto donde se busca una pista: nombre de archivo, titulo, artista y
/// album, ya plegados. Se calcula al indexar para no plegar en cada
/// busqueda. Los campos van separados por un salto de linea, asi una
/// palabra no se arma con el final de un campo y el principio del otro.
pub fn search_text(entry: &Entry) -> String {
  let i = &entry.info;
  let name = entry.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
  let mut parts = vec![name];
  parts.extend([&i.title, &i.artist, &i.album, &i.album_artist, &i.genre].into_iter().flatten().cloned());
  parts.extend(i.year.map(|y| y.to_string()));
  fold(&parts.join("\n"))
}

/// Cada palabra (ya plegada) aparece en el texto de busqueda.
pub fn matches_text(text: &str, words: &[String]) -> bool {
  words.iter().all(|w| text.contains(w.as_str()))
}

#[cfg(test)]
mod tests {
  use super::*;

  fn entry(path: &str, title: Option<&str>, artist: Option<&str>) -> Entry {
    Entry {
      path: PathBuf::from(path),
      info: TrackInfo { title: title.map(str::to_string), artist: artist.map(str::to_string), ..Default::default() },
    }
  }

  #[test]
  fn every_word_must_match_some_field() {
    let e = entry("/m/a/song.flac", Some("Magnetic"), Some("ILLIT"));
    assert!(matches(&e, &words("illit magnetic")));
    assert!(matches(&e, &words("SONG")));
    assert!(!matches(&e, &words("illit roses")));
    assert!(matches(&e, &words("")));
  }

  #[test]
  fn accents_and_case_never_get_in_the_way() {
    let e = entry("/m/Canción de niño.flac", Some("Über Straße"), Some("Ærøskøbing Œuvre"));
    for q in ["cancion", "CANCIÓN", "nino", "NIÑO", "uber", "über", "strasse", "straße", "aeroskobing", "oeuvre", "ción de ni"] {
      assert!(matches(&e, &words(q)), "{q:?} should match");
    }
    // Las variantes funcionan en los dos sentidos.
    let plain = entry("/m/cancion de nino.flac", None, None);
    assert!(matches(&plain, &words("canción niño")));
    assert!(!matches(&e, &words("cancionx")));
  }

  #[test]
  fn other_scripts_keep_their_meaning() {
    // El dakuten (U+3099) cambia la palabra ("ga" no es "ka"): no se quita.
    let jp = entry("/m/a.flac", Some("ながれ"), None);
    assert!(matches(&jp, &words("なが")));
    assert!(!matches(&jp, &words("なか")));
    // Hangul y cirilico se buscan en cualquier parte.
    let ko = entry("/m/a.flac", Some("키스오브라이프"), Some("Кино"));
    assert!(matches(&ko, &words("오브")));
    assert!(matches(&ko, &words("кино")));
    // Diacriticos del cirilico (U+0439 = U+0438 + breve) si se pliegan.
    let ru = entry("/m/a.flac", Some("Мой"), None);
    assert!(matches(&ru, &words("мои")));
  }

  #[test]
  fn fold_is_lowercase_without_marks() {
    assert_eq!(fold("Ça Ñandú ÜBER Straße"), "ca nandu uber strasse");
  }

  #[test]
  fn candidates_are_audio_files_recursively_sorted() {
    let dir = std::env::temp_dir().join(format!("ketai-lib-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("b/c")).unwrap();
    for f in ["b/c/z.FLAC", "a.mp3", "notes.txt", "b/y.opus"] {
      fs::write(dir.join(f), b"x").unwrap();
    }
    let got: Vec<String> =
      candidates(&dir).iter().map(|p| p.strip_prefix(&dir).unwrap().display().to_string()).collect();
    assert_eq!(got, ["a.mp3", "b/c/z.FLAC", "b/y.opus"]);
    fs::remove_dir_all(&dir).unwrap();
  }
}
