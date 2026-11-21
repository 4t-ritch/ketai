//! Formato de playlists: archivos `.m3u8` (estandar, compatible con otros
//! reproductores).
//!
//! Las pistas locales se escriben como rutas absolutas y las remotas como
//! URI. Al leer se aceptan rutas absolutas, rutas relativas a la carpeta de
//! la playlist y URIs.

use std::path::Path;

use crate::uri::{file_uri_to_path, path_to_file_uri};

pub const EXTENSION: &str = "m3u8";

/// Un nombre de playlist se usa como nombre de archivo: no puede estar
/// vacio, tener `/` ni NUL, ni empezar con `.` (archivo oculto).
pub fn valid_name(name: &str) -> bool {
  !name.is_empty()
    && name.len() <= 200
    && !name.starts_with('.')
    && !name.contains('/')
    && !name.contains('\0')
}

/// Lee el contenido de un `.m3u8` y devuelve URIs.
pub fn parse(text: &str, base_dir: &Path) -> Vec<String> {
  text
    .lines()
    .map(str::trim)
    .filter(|l| !l.is_empty() && !l.starts_with('#'))
    .map(|line| {
      if line.contains("://") {
        line.to_string()
      } else if line.starts_with('/') {
        path_to_file_uri(Path::new(line))
      } else {
        path_to_file_uri(&base_dir.join(line))
      }
    })
    .collect()
}

/// Genera el contenido de un `.m3u8` a partir de URIs.
pub fn render(uris: &[String]) -> String {
  let mut out = String::from("#EXTM3U\n");
  for uri in uris {
    match file_uri_to_path(uri) {
      Some(path) => out.push_str(&path.to_string_lossy()),
      None => out.push_str(uri),
    }
    out.push('\n');
  }
  out
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn validates_names() {
    assert!(valid_name("Rock en espanol"));
    assert!(valid_name("케이팝"));
    for bad in ["", ".x", "a/b", "a\0b"] {
      assert!(!valid_name(bad), "{bad:?}");
    }
  }

  #[test]
  fn parses_paths_relative_paths_uris_and_skips_comments() {
    let text = "#EXTM3U\n#EXTINF:123,Artist - Title\n/m/a b.ogg\n\nsub/c.flac\nhttps://x/y.mp3\n";
    assert_eq!(
      parse(text, Path::new("/pl")),
      ["file:///m/a%20b.ogg", "file:///pl/sub/c.flac", "https://x/y.mp3"]
    );
  }

  #[test]
  fn renders_local_paths_plainly() {
    let uris = ["file:///m/a%20b.ogg".to_string(), "https://x/y.mp3".to_string()];
    assert_eq!(render(&uris), "#EXTM3U\n/m/a b.ogg\nhttps://x/y.mp3\n");
    assert_eq!(parse(&render(&uris), Path::new("/")), uris);
  }
}
