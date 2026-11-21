//! Letras: modelo comun y parsers de cada formato.
//!
//! Cada parser produce un `Doc`: lineas con tiempos opcionales y palabras
//! opcionales. Los tiempos de un `Doc` son los del archivo, sin offsets
//! aplicados; el offset del archivo viaja aparte en `offset_ms`.

pub mod id3;
pub mod layers;
pub mod lrc;
pub mod package;
pub mod text;
pub mod ttml;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Word {
  pub start_ms: u64,
  pub end_ms: Option<u64>,
  pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Line {
  /// Identificador de la linea en el origen (TTML `itunes:key`).
  pub key: Option<String>,
  pub start_ms: Option<u64>,
  pub end_ms: Option<u64>,
  pub text: String,
  /// Palabras con tiempo propio; `None` si el origen no las tiene.
  pub words: Option<Vec<Word>>,
  /// Agente (cantante) de TTML.
  pub agent: Option<String>,
  /// Coro de fondo (TTML `ttm:role="x-bg"`).
  pub background: bool,
  /// Cantidad de fragmentos (`<span>`) que la linea tenia en el origen,
  /// aunque no tuvieran tiempo. Se usa para el emparejamiento 1 a 1.
  pub fragments: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Timing {
  None,
  Line,
  Word,
  /// Fragmentos emparejados 1 a 1 con la capa original, sin tiempo propio.
  WordPaired,
}

impl Timing {
  pub fn as_str(self) -> &'static str {
    match self {
      Timing::None => "none",
      Timing::Line => "line",
      Timing::Word => "word",
      Timing::WordPaired => "word-paired",
    }
  }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
  Text,
  Lrc,
  Ttml,
}

impl Format {
  pub fn as_str(self) -> &'static str {
    match self {
      Format::Text => "text",
      Format::Lrc => "lrc",
      Format::Ttml => "ttml",
    }
  }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Doc {
  pub lines: Vec<Line>,
  /// Offset del archivo (`[offset:]` de LRC). Positivo: la letra aparece
  /// antes.
  pub offset_ms: i64,
  /// Agentes de TTML: (id, nombre).
  pub agents: Vec<(String, String)>,
  /// Idioma declarado por el archivo (TTML `xml:lang`).
  pub language: Option<String>,
  /// Capas extra que trae el mismo archivo (TTML `translations` y
  /// `transliterations`).
  pub extra: Vec<ExtraLayer>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
  Original,
  Romanized,
  Translation,
}

impl Kind {
  pub fn as_str(self) -> &'static str {
    match self {
      Kind::Original => "original",
      Kind::Romanized => "romanized",
      Kind::Translation => "translation",
    }
  }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtraLayer {
  pub kind: Kind,
  pub language: String,
  pub doc: Doc,
}

impl Doc {
  /// Nivel de tiempo propio del documento.
  pub fn timing(&self) -> Timing {
    if self.lines.iter().any(|l| l.words.as_ref().is_some_and(|w| !w.is_empty())) {
      Timing::Word
    } else if self.lines.iter().any(|l| l.start_ms.is_some()) {
      Timing::Line
    } else {
      Timing::None
    }
  }
}

/// Lee un tiempo con alguno de estos formatos y lo devuelve en ms:
/// `12.345`, `12.345s`, `1:02.345`, `01:02:03.4`, `mm:ss:xx` (LRC viejo).
pub fn parse_time(s: &str) -> Option<u64> {
  let s = s.trim();
  let s = s.strip_suffix('s').unwrap_or(s);
  let parts: Vec<&str> = s.split(':').collect();
  let seconds = |p: &str| -> Option<f64> {
    let v: f64 = p.parse().ok()?;
    (v >= 0.0 && v.is_finite()).then_some(v)
  };
  let int = |p: &str| -> Option<f64> { p.parse::<u64>().ok().map(|v| v as f64) };
  let total = match parts.as_slice() {
    [sec] => seconds(sec)?,
    [min, sec] => int(min)? * 60.0 + seconds(sec)?,
    // hh:mm:ss.xx, o mm:ss:xx (centesimas separadas por ':').
    [a, b, c] if c.contains('.') || a.len() > 2 => int(a)? * 3600.0 + int(b)? * 60.0 + seconds(c)?,
    [min, sec, cs] => int(min)? * 60.0 + int(sec)? + int(cs)? / 10f64.powi(cs.len() as i32),
    _ => return None,
  };
  Some((total * 1000.0).round() as u64)
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn parses_time_formats() {
    assert_eq!(parse_time("12.345"), Some(12345));
    assert_eq!(parse_time("12.3s"), Some(12300));
    assert_eq!(parse_time("1:02.5"), Some(62500));
    assert_eq!(parse_time("01:02:03.4"), Some(3723400));
    assert_eq!(parse_time("00:12:50"), Some(12500));
    assert_eq!(parse_time("abc"), None);
    assert_eq!(parse_time("-1"), None);
  }
}
