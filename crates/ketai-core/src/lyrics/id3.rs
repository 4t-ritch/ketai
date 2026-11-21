//! Lector minimo de ID3v2 (2.3 y 2.4) para letras: frames `USLT` (sin
//! tiempos) y `SYLT` (sincronizada).
//!
//! symphonia lee `USLT` pero descarta su descriptor, y no lee `SYLT`; por eso
//! este lector propio. Solo mira el tag al principio del archivo.

use super::{Doc, Line, Word};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
  pub synced: bool,
  /// Codigo ISO 639-2 (por ejemplo `spa`), tal como viene en el frame.
  pub language: String,
  pub descriptor: String,
  pub doc: Doc,
}

/// Devuelve los frames de letras del tag ID3v2 al principio de `data`.
pub fn read(data: &[u8]) -> Vec<Frame> {
  parse(data).unwrap_or_default()
}

fn parse(data: &[u8]) -> Option<Vec<Frame>> {
  if data.len() < 10 || &data[..3] != b"ID3" {
    return None;
  }
  let major = data[3];
  if major != 3 && major != 4 {
    return None;
  }
  let flags = data[5];
  let size = syncsafe(&data[6..10])? as usize;
  let body = data.get(10..10 + size)?;
  let tag_unsync = flags & 0x80 != 0;
  // En 2.3 la desincronizacion es de todo el tag; en 2.4 tambien puede
  // indicarse por frame.
  let owned;
  let body = if tag_unsync && major == 3 {
    owned = resync(body);
    &owned[..]
  } else {
    body
  };
  let mut pos = 0;
  if flags & 0x40 != 0 {
    // Encabezado extendido: se saltea.
    let ext = body.get(0..4)?;
    pos = if major == 4 { syncsafe(ext)? as usize } else { u32::from_be_bytes(ext.try_into().ok()?) as usize + 4 };
  }

  let mut frames = Vec::new();
  while pos + 10 <= body.len() {
    let id = &body[pos..pos + 4];
    if id[0] == 0 {
      break; // Relleno.
    }
    let raw_size = &body[pos + 4..pos + 8];
    let frame_size = if major == 4 { syncsafe(raw_size)? } else { u32::from_be_bytes(raw_size.try_into().ok()?) } as usize;
    let fflags = body[pos + 9];
    let start = pos + 10;
    let content = body.get(start..start + frame_size)?;
    pos = start + frame_size;

    if id != b"USLT" && id != b"SYLT" {
      continue;
    }
    let (compressed, encrypted, grouping, unsync, data_len) = if major == 4 {
      (fflags & 0x08 != 0, fflags & 0x04 != 0, fflags & 0x40 != 0, fflags & 0x02 != 0 || tag_unsync, fflags & 0x01 != 0)
    } else {
      (fflags & 0x80 != 0, fflags & 0x40 != 0, fflags & 0x20 != 0, false, false)
    };
    if compressed || encrypted {
      continue;
    }
    let mut content = content;
    if grouping {
      content = content.get(1..)?;
    }
    if data_len {
      content = content.get(4..)?;
    }
    let decoded;
    let content = if unsync {
      decoded = resync(content);
      &decoded[..]
    } else {
      content
    };
    let frame = if id == b"USLT" { uslt(content) } else { sylt(content) };
    if let Some(f) = frame {
      frames.push(f);
    }
  }
  Some(frames)
}

fn syncsafe(b: &[u8]) -> Option<u32> {
  if b.len() != 4 || b.iter().any(|x| x & 0x80 != 0) {
    return None;
  }
  Some(b.iter().fold(0u32, |acc, &x| (acc << 7) | x as u32))
}

/// Deshace la desincronizacion: `FF 00` -> `FF`.
fn resync(data: &[u8]) -> Vec<u8> {
  let mut out = Vec::with_capacity(data.len());
  let mut i = 0;
  while i < data.len() {
    out.push(data[i]);
    if data[i] == 0xFF && data.get(i + 1) == Some(&0) {
      i += 1;
    }
    i += 1;
  }
  out
}

/// Lee un texto terminado en NUL segun la codificacion. Devuelve el texto y
/// lo que sigue despues del terminador.
fn take_string(enc: u8, data: &[u8]) -> (String, &[u8]) {
  let wide = enc == 1 || enc == 2;
  let end = if wide {
    (0..data.len().saturating_sub(1)).step_by(2).find(|&i| data[i] == 0 && data[i + 1] == 0)
  } else {
    data.iter().position(|&b| b == 0)
  };
  let (text, rest) = match end {
    Some(i) => (&data[..i], &data[i + if wide { 2 } else { 1 }..]),
    None => (data, &data[data.len()..]),
  };
  (decode(enc, text), rest)
}

fn decode(enc: u8, data: &[u8]) -> String {
  match enc {
    0 => data.iter().map(|&b| b as char).collect(),
    1 | 2 => {
      let (big_endian, body) = match data {
        [0xFE, 0xFF, rest @ ..] => (true, rest),
        [0xFF, 0xFE, rest @ ..] => (false, rest),
        _ => (enc == 2, data),
      };
      let units: Vec<u16> = body
        .chunks_exact(2)
        .map(|c| if big_endian { u16::from_be_bytes([c[0], c[1]]) } else { u16::from_le_bytes([c[0], c[1]]) })
        .collect();
      String::from_utf16_lossy(&units)
    }
    _ => String::from_utf8_lossy(data).into_owned(),
  }
}

fn header(data: &[u8]) -> Option<(u8, String, &[u8])> {
  let enc = *data.first()?;
  let lang = String::from_utf8_lossy(data.get(1..4)?).into_owned();
  Some((enc, lang, data.get(4..)?))
}

fn uslt(data: &[u8]) -> Option<Frame> {
  let (enc, language, rest) = header(data)?;
  let (descriptor, rest) = take_string(enc, rest);
  let text = decode(enc, rest);
  Some(Frame { synced: false, language, descriptor, doc: super::text::parse(&text) })
}

fn sylt(data: &[u8]) -> Option<Frame> {
  let (enc, language, rest) = header(data)?;
  let format = *rest.first()?;
  // Solo se entienden marcas en milisegundos (formato 2).
  if format != 2 {
    return None;
  }
  let rest = rest.get(2..)?; // formato + tipo de contenido
  let (descriptor, mut rest) = take_string(enc, rest);
  let mut entries: Vec<(String, u64)> = Vec::new();
  while !rest.is_empty() {
    let (text, after) = take_string(enc, rest);
    let time = after.get(0..4)?;
    entries.push((text, u32::from_be_bytes(time.try_into().ok()?) as u64));
    rest = &after[4..];
  }
  Some(Frame { synced: true, language, descriptor, doc: sylt_doc(&entries) })
}

/// Si alguna entrada empieza con salto de linea, las entradas son silabas y
/// el salto marca una linea nueva; si no, cada entrada es una linea.
fn sylt_doc(entries: &[(String, u64)]) -> Doc {
  let syllables = entries.iter().any(|(t, _)| t.starts_with('\n') || t.starts_with('\r'));
  let mut lines: Vec<Line> = Vec::new();
  for (text, time) in entries {
    if !syllables {
      let text = text.trim();
      if !text.is_empty() {
        lines.push(Line { start_ms: Some(*time), text: text.to_string(), ..Default::default() });
      }
      continue;
    }
    let new_line = text.starts_with('\n') || text.starts_with('\r') || lines.is_empty();
    let piece = text.trim_start_matches(['\n', '\r']);
    if new_line {
      lines.push(Line { start_ms: Some(*time), words: Some(Vec::new()), ..Default::default() });
    }
    let line = lines.last_mut().expect("line exists");
    line.text.push_str(piece);
    line.words.as_mut().expect("words").push(Word { start_ms: *time, end_ms: None, text: piece.to_string() });
  }
  // Los finales salen del inicio siguiente.
  let starts: Vec<Option<u64>> = lines.iter().map(|l| l.start_ms).collect();
  for (i, line) in lines.iter_mut().enumerate() {
    line.end_ms = starts.get(i + 1).copied().flatten();
    line.text = line.text.trim().to_string();
    if let Some(words) = &mut line.words {
      let ends: Vec<Option<u64>> = words.iter().skip(1).map(|w| Some(w.start_ms)).chain([line.end_ms]).collect();
      for (w, end) in words.iter_mut().zip(ends) {
        w.end_ms = end;
      }
    }
  }
  Doc { lines, ..Default::default() }
}

#[cfg(test)]
pub(crate) mod tests {
  use super::*;

  pub fn synchsafe(n: usize) -> [u8; 4] {
    [(n >> 21) as u8 & 0x7F, (n >> 14) as u8 & 0x7F, (n >> 7) as u8 & 0x7F, n as u8 & 0x7F]
  }

  /// Arma un tag ID3v2.4 con los frames dados (id, contenido).
  pub fn tag(frames: &[(&[u8; 4], Vec<u8>)]) -> Vec<u8> {
    let mut body = Vec::new();
    for (id, content) in frames {
      body.extend_from_slice(*id);
      body.extend_from_slice(&synchsafe(content.len()));
      body.extend_from_slice(&[0, 0]);
      body.extend_from_slice(content);
    }
    let mut out = b"ID3\x04\x00\x00".to_vec();
    out.extend_from_slice(&synchsafe(body.len()));
    out.extend(body);
    out
  }

  pub fn uslt_frame(lang: &str, desc: &str, text: &str) -> Vec<u8> {
    let mut c = vec![3u8];
    c.extend_from_slice(lang.as_bytes());
    c.extend_from_slice(desc.as_bytes());
    c.push(0);
    c.extend_from_slice(text.as_bytes());
    c
  }

  pub fn sylt_frame(lang: &str, desc: &str, entries: &[(&str, u32)]) -> Vec<u8> {
    let mut c = vec![3u8];
    c.extend_from_slice(lang.as_bytes());
    c.extend_from_slice(&[2, 1]);
    c.extend_from_slice(desc.as_bytes());
    c.push(0);
    for (text, t) in entries {
      c.extend_from_slice(text.as_bytes());
      c.push(0);
      c.extend_from_slice(&t.to_be_bytes());
    }
    c
  }

  #[test]
  fn reads_uslt_with_descriptor_and_language() {
    let data = tag(&[(b"TIT2", vec![3, b'x']), (b"USLT", uslt_frame("spa", "translation", "hola\nmundo"))]);
    let frames = read(&data);
    assert_eq!(frames.len(), 1);
    let f = &frames[0];
    assert_eq!((f.synced, f.language.as_str(), f.descriptor.as_str()), (false, "spa", "translation"));
    let texts: Vec<&str> = f.doc.lines.iter().map(|l| l.text.as_str()).collect();
    assert_eq!(texts, ["hola", "mundo"]);
  }

  #[test]
  fn reads_sylt_lines() {
    let data = tag(&[(b"SYLT", sylt_frame("eng", "", &[("first", 1000), ("second", 2500)]))]);
    let f = &read(&data)[0];
    assert!(f.synced);
    let got: Vec<(Option<u64>, Option<u64>, &str)> =
      f.doc.lines.iter().map(|l| (l.start_ms, l.end_ms, l.text.as_str())).collect();
    assert_eq!(got, [(Some(1000), Some(2500), "first"), (Some(2500), None, "second")]);
  }

  #[test]
  fn reads_sylt_syllables_into_word_timed_lines() {
    let data = tag(&[(b"SYLT", sylt_frame("eng", "", &[("Ki", 1000), ("mi", 1200), ("\nno", 2000), ("koe", 2300)]))]);
    let doc = &read(&data)[0].doc;
    assert_eq!(doc.lines.len(), 2);
    assert_eq!(doc.lines[0].text, "Kimi");
    let w: Vec<(u64, Option<u64>)> = doc.lines[0].words.as_ref().unwrap().iter().map(|w| (w.start_ms, w.end_ms)).collect();
    assert_eq!(w, [(1000, Some(1200)), (1200, Some(2000))]);
    assert_eq!(doc.lines[1].start_ms, Some(2000));
  }

  #[test]
  fn decodes_utf16_and_unsynchronisation() {
    // UTF-16 con BOM little-endian: "a" + terminador doble.
    let mut c = vec![1u8];
    c.extend_from_slice(b"kor");
    c.extend_from_slice(&[0xFF, 0xFE, b'd', 0, 0, 0]);
    c.extend_from_slice(&[0xFF, 0xFE]);
    for u in "가사".encode_utf16() {
      c.extend_from_slice(&u.to_le_bytes());
    }
    let f = &read(&tag(&[(b"USLT", c)]))[0];
    assert_eq!((f.descriptor.as_str(), f.doc.lines[0].text.as_str()), ("d", "가사"));

    assert_eq!(resync(&[0xFF, 0x00, 0xE0, 0xFF, 0x00]), [0xFF, 0xE0, 0xFF]);
  }

  #[test]
  fn ignores_non_id3_and_broken_tags() {
    assert!(read(b"OggS....").is_empty());
    assert!(read(b"ID3\x04\x00\x00\x00\x00\x7F\x7F").is_empty());
  }
}
