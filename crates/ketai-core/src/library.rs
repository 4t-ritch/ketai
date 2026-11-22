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

/// Palabras de una consulta, en minusculas.
pub fn words(query: &str) -> Vec<String> {
  query.split_whitespace().map(str::to_lowercase).collect()
}

/// true si cada palabra aparece en el titulo, artista, album o nombre de
/// archivo, sin distinguir mayusculas.
pub fn matches(entry: &Entry, words: &[String]) -> bool {
  let i = &entry.info;
  let name = entry.path.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
  let fields: Vec<String> = [&i.title, &i.artist, &i.album].iter().filter_map(|f| f.as_ref().map(|v| v.to_lowercase())).collect();
  words.iter().all(|w| name.contains(w.as_str()) || fields.iter().any(|f| f.contains(w.as_str())))
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
