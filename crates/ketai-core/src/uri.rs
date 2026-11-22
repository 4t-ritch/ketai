//! Conversion entre URIs `file://` y rutas locales.

use std::{
  ffi::OsString,
  os::unix::ffi::{OsStrExt, OsStringExt},
  path::{Path, PathBuf},
};

/// `file:///ruta%20con%20espacios` -> `/ruta con espacios`. None si no es
/// una URI `file://` absoluta y bien codificada.
pub fn file_uri_to_path(uri: &str) -> Option<PathBuf> {
  let rest = uri.strip_prefix("file://")?;
  if !rest.starts_with('/') {
    return None;
  }
  let bytes = rest.as_bytes();
  let mut out = Vec::with_capacity(bytes.len());
  let mut i = 0;
  while i < bytes.len() {
    if bytes[i] == b'%' {
      let hex = std::str::from_utf8(bytes.get(i + 1..i + 3)?).ok()?;
      out.push(u8::from_str_radix(hex, 16).ok()?);
      i += 3;
    } else {
      out.push(bytes[i]);
      i += 1;
    }
  }
  Some(PathBuf::from(OsString::from_vec(out)))
}

/// `/ruta con espacios` -> `file:///ruta%20con%20espacios`. Solo se dejan
/// sin codificar los caracteres no reservados y `/`.
pub fn path_to_file_uri(path: &Path) -> String {
  let mut uri = String::from("file://");
  for &b in path.as_os_str().as_bytes() {
    if b.is_ascii_alphanumeric() || b"-._~/".contains(&b) {
      uri.push(b as char);
    } else {
      uri.push_str(&format!("%{b:02X}"));
    }
  }
  uri
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn decodes_file_uris() {
    assert_eq!(file_uri_to_path("file:///a%20b/c.ogg"), Some(PathBuf::from("/a b/c.ogg")));
    assert_eq!(file_uri_to_path("file://relative.ogg"), None);
    assert_eq!(file_uri_to_path("file:///bad%2"), None);
    assert_eq!(file_uri_to_path("http://x/a.ogg"), None);
  }

  #[test]
  fn encode_and_decode_round_trip() {
    for p in ["/music/a b/c.ogg", "/m/ILLIT-paw_paw!.ogg", "/m/케이팝/곡.flac", "/m/100%.mp3"] {
      let uri = path_to_file_uri(Path::new(p));
      assert_eq!(file_uri_to_path(&uri), Some(PathBuf::from(p)), "{uri}");
    }
    assert_eq!(path_to_file_uri(Path::new("/a b")), "file:///a%20b");
  }
}
