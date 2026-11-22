//! Alineacion entre capas y armado del paquete (docs/protocol.md,
//! "Sincronizacion entre capas").
//!
//! - Las lineas se alinean siempre a la capa original (el ancla comun).
//! - El tiempo por palabra solo sale del origen de esa misma capa; nunca se
//!   proyectan las palabras de la original sobre otra capa.
//! - Excepcion: fragmentos emparejados 1 a 1 por el origen (`word-paired`).
//! - Estimacion por caracteres solo si el cliente la pide.

use super::{Doc, Line, Timing, Word, layers::Layer};

/// Tolerancia para asociar lineas LRC por tiempo.
const ALIGN_TOLERANCE_MS: u64 = 500;

/// Para cada linea de la original, el indice de la linea de la capa que le
/// corresponde. None si la capa no se puede alinear.
pub fn align(original: &Doc, layer: &Doc) -> Option<Vec<Option<usize>>> {
  let anchors = &original.lines;
  let mut map = vec![None; anchors.len()];

  let keyed = layer.lines.iter().all(|l| l.key.is_some()) && anchors.iter().any(|l| l.key.is_some());
  let timed = layer.lines.iter().any(|l| l.start_ms.is_some()) && anchors.iter().any(|l| l.start_ms.is_some());

  if keyed {
    for (i, a) in anchors.iter().enumerate() {
      map[i] = a.key.as_ref().and_then(|k| layer.lines.iter().position(|l| l.key.as_ref() == Some(k)));
    }
  } else if timed {
    // Cada linea de la capa va con la original de inicio igual o mas cercano
    // dentro de la tolerancia, comparando tiempos con su propio offset.
    for (j, l) in layer.lines.iter().enumerate() {
      let Some(t) = l.start_ms.map(|t| shift(t, layer.offset_ms, 0)) else { continue };
      let best = anchors
        .iter()
        .enumerate()
        .filter(|(i, _)| map[*i].is_none())
        .filter_map(|(i, a)| a.start_ms.map(|s| (i, shift(s, original.offset_ms, 0).abs_diff(t))))
        .filter(|(_, d)| *d <= ALIGN_TOLERANCE_MS)
        .min_by_key(|(_, d)| *d);
      if let Some((i, _)) = best {
        map[i] = Some(j);
      }
    }
  } else {
    // Sin tiempos: linea por linea, en orden, sobre las lineas principales
    // (los coros de fondo no cuentan). Si no coinciden, no se alinea.
    let main: Vec<usize> = (0..anchors.len()).filter(|&i| !anchors[i].background).collect();
    if main.len() != layer.lines.len() {
      return None;
    }
    for (j, &i) in main.iter().enumerate() {
      map[i] = Some(j);
    }
  }
  map.iter().any(Option::is_some).then_some(map)
}

/// Capas que se pueden ofrecer, con su nivel de tiempo: la original y las
/// que se alinean a ella. Las que no se alinean se descartan.
pub fn offered(layers: &[Layer]) -> Vec<(&Layer, Timing)> {
  let Some(original) = layers.iter().find(|l| l.id == "original") else { return Vec::new() };
  let identity: Vec<Option<usize>> = (0..original.doc.lines.len()).map(Some).collect();
  layers
    .iter()
    .filter_map(|l| {
      let map = if l.id == "original" { identity.clone() } else { align(&original.doc, &l.doc)? };
      Some((l, layer_timing(&original.doc, l, &map)))
    })
    .collect()
}

/// Nivel de tiempo que la capa ofrece, ya alineada a la original.
pub fn layer_timing(original: &Doc, layer: &Layer, map: &[Option<usize>]) -> Timing {
  if layer.id == "original" {
    return original.timing();
  }
  match layer.doc.timing() {
    Timing::Word => Timing::Word,
    _ if paired(original, &layer.doc, map) => Timing::WordPaired,
    Timing::None if original.timing() != Timing::None => Timing::Line,
    t => t,
  }
}

/// Emparejamiento 1 a 1: cada linea alineada tiene tantos fragmentos como
/// palabras con tiempo tiene su linea original.
fn paired(original: &Doc, layer: &Doc, map: &[Option<usize>]) -> bool {
  let mut any = false;
  for (i, m) in map.iter().enumerate() {
    let Some(j) = m else { continue };
    let words = original.lines[i].words.as_ref().map_or(0, Vec::len);
    let frags = layer.lines[*j].fragments;
    if frags == 0 || frags != words {
      return false;
    }
    any = true;
  }
  any
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageLine {
  pub start_ms: Option<u64>,
  pub end_ms: Option<u64>,
  pub text: String,
  pub agent: Option<String>,
  pub background: bool,
  pub words: Option<Vec<Word>>,
  /// (id de capa, texto) de cada secundaria con texto en esta linea.
  pub secondary: Vec<(String, String)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Package {
  pub primary: String,
  pub secondary: Vec<String>,
  /// `none`, `line`, `word`, `word-paired` o `word-estimated`.
  pub timing: &'static str,
  pub offset_ms: i64,
  pub user_offset_ms: i64,
  pub agents: Vec<(String, String)>,
  pub lines: Vec<PackageLine>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BuildError {
  UnknownLayer(String),
  /// La misma capa como principal y secundaria.
  Repeated(String),
}

/// Arma el paquete pedido. `layers` debe tener la original primero.
pub fn build(
  layers: &[Layer],
  primary: &str,
  secondary: &[String],
  estimate_words: bool,
  user_offset_ms: i64,
) -> Result<Package, BuildError> {
  let original = layers.iter().find(|l| l.id == "original").ok_or_else(|| BuildError::UnknownLayer("original".into()))?;
  let find = |id: &str| layers.iter().find(|l| l.id == id).ok_or_else(|| BuildError::UnknownLayer(id.to_string()));
  let primary_layer = find(primary)?;
  if secondary.iter().any(|s| s == primary) {
    return Err(BuildError::Repeated(primary.to_string()));
  }
  let secondaries = secondary.iter().map(|s| find(s)).collect::<Result<Vec<_>, _>>()?;

  let anchor = &original.doc;
  let identity: Vec<Option<usize>> = (0..anchor.lines.len()).map(Some).collect();
  let map_of = |l: &Layer| if l.id == "original" { Some(identity.clone()) } else { align(anchor, &l.doc) };
  let primary_map = map_of(primary_layer).ok_or_else(|| BuildError::UnknownLayer(primary.to_string()))?;
  let secondary_maps: Vec<(&Layer, Vec<Option<usize>>)> =
    secondaries.iter().filter_map(|l| map_of(l).map(|m| (*l, m))).collect();

  let anchor_timed = anchor.timing() != Timing::None;
  let primary_timing = layer_timing(anchor, primary_layer, &primary_map);
  let shift_anchor = |t: Option<u64>| t.map(|t| shift(t, anchor.offset_ms, user_offset_ms));

  let mut estimated = false;
  let mut lines = Vec::with_capacity(anchor.lines.len());
  for (i, a) in anchor.lines.iter().enumerate() {
    let start = if anchor_timed { shift_anchor(a.start_ms) } else { None };
    let end = if anchor_timed { shift_anchor(a.end_ms.or_else(|| anchor.lines.get(i + 1).and_then(|n| n.start_ms))) } else { None };
    let own: Option<&Line> = primary_map[i].map(|j| &primary_layer.doc.lines[j]);
    let text = own.map(|l| l.text.clone()).unwrap_or_default();

    let mut words = if !anchor_timed {
      None
    } else {
      match (primary_timing, own) {
        (Timing::Word, Some(l)) => l.words.as_ref().map(|w| shift_words(w, primary_layer.doc.offset_ms, user_offset_ms)),
        // Mismos fragmentos que la original: se reutilizan sus tiempos.
        (Timing::WordPaired, Some(_)) => a.words.as_ref().map(|aw| {
          let texts = fragments_text(own.expect("own line"));
          aw.iter()
            .zip(texts)
            .map(|(w, text)| Word { text, ..shift_word(w, anchor.offset_ms, user_offset_ms) })
            .collect()
        }),
        _ => None,
      }
    };
    if words.is_none() && estimate_words && anchor_timed && !text.is_empty()
      && let (Some(s), Some(e)) = (start, end)
    {
      words = Some(estimate(&text, s, e));
      estimated = true;
    }

    let secondary = secondary_maps
      .iter()
      .filter_map(|(l, m)| m[i].map(|j| (l.id.clone(), l.doc.lines[j].text.clone())))
      .filter(|(_, t)| !t.is_empty())
      .collect();
    lines.push(PackageLine { start_ms: start, end_ms: end, text, agent: a.agent.clone(), background: a.background, words, secondary });
  }

  let timing = if !anchor_timed {
    "none"
  } else {
    match primary_timing {
      Timing::Word => "word",
      Timing::WordPaired => "word-paired",
      _ if estimated => "word-estimated",
      _ => "line",
    }
  };
  Ok(Package {
    primary: primary.to_string(),
    secondary: secondaries.iter().map(|l| l.id.clone()).collect(),
    timing,
    offset_ms: anchor.offset_ms,
    user_offset_ms,
    agents: anchor.agents.clone(),
    lines,
  })
}

/// `tiempo - offset_archivo - offset_usuario`, con minimo 0.
fn shift(t: u64, file_offset: i64, user_offset: i64) -> u64 {
  (t as i64 - file_offset - user_offset).max(0) as u64
}

fn shift_word(w: &Word, file_offset: i64, user_offset: i64) -> Word {
  Word {
    start_ms: shift(w.start_ms, file_offset, user_offset),
    end_ms: w.end_ms.map(|e| shift(e, file_offset, user_offset)),
    text: w.text.clone(),
  }
}

fn shift_words(words: &[Word], file_offset: i64, user_offset: i64) -> Vec<Word> {
  words.iter().map(|w| shift_word(w, file_offset, user_offset)).collect()
}

/// Texto de cada fragmento de una linea emparejada: palabras separadas por
/// espacio, o el texto entero repartido si no hay espacios.
fn fragments_text(line: &Line) -> Vec<String> {
  let parts: Vec<&str> = line.text.split_whitespace().collect();
  if parts.len() == line.fragments {
    let last = parts.len().saturating_sub(1);
    return parts.iter().enumerate().map(|(i, p)| if i < last { format!("{p} ") } else { p.to_string() }).collect();
  }
  // Fallback: el primer fragmento lleva todo el texto.
  let mut out = vec![String::new(); line.fragments];
  if let Some(first) = out.first_mut() {
    *first = line.text.clone();
  }
  out
}

/// Reparte la ventana de la linea entre sus palabras segun su largo.
fn estimate(text: &str, start: u64, end: u64) -> Vec<Word> {
  let parts: Vec<&str> = text.split_whitespace().collect();
  let total: usize = parts.iter().map(|p| p.chars().count()).sum::<usize>().max(1);
  let span = end.saturating_sub(start);
  let mut words = Vec::with_capacity(parts.len());
  let mut acc = 0usize;
  let last = parts.len().saturating_sub(1);
  for (i, p) in parts.iter().enumerate() {
    let s = start + (span as u128 * acc as u128 / total as u128) as u64;
    acc += p.chars().count();
    let e = if i == last { end } else { start + (span as u128 * acc as u128 / total as u128) as u64 };
    let text = if i < last { format!("{p} ") } else { p.to_string() };
    words.push(Word { start_ms: s, end_ms: Some(e), text });
  }
  words
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::lyrics::{
    Format, Kind, lrc,
    layers::{Layer, Origin},
    text, ttml,
  };

  fn layer(id: &str, kind: Kind, format: Format, doc: Doc) -> Layer {
    Layer { id: id.into(), kind, language: None, format, origin: Origin::Sidecar, doc }
  }

  fn original_lrc(src: &str) -> Layer {
    layer("original", Kind::Original, Format::Lrc, lrc::parse(src).unwrap())
  }

  #[test]
  fn original_only_with_offsets_applied() {
    let o = original_lrc("[offset:+200]\n[00:01.00]a\n[00:03.00]b");
    let p = build(&[o], "original", &[], false, -300).unwrap();
    assert_eq!(p.timing, "line");
    // 1000 - 200 - (-300) = 1100
    let got: Vec<(Option<u64>, Option<u64>, &str)> = p.lines.iter().map(|l| (l.start_ms, l.end_ms, l.text.as_str())).collect();
    assert_eq!(got, [(Some(1100), Some(3100), "a"), (Some(3100), None, "b")]);
    assert_eq!((p.offset_ms, p.user_offset_ms), (200, -300));
  }

  #[test]
  fn negative_times_clamp_to_zero() {
    let o = original_lrc("[00:00.10]a");
    let p = build(&[o], "original", &[], false, 500).unwrap();
    assert_eq!(p.lines[0].start_ms, Some(0));
  }

  #[test]
  fn lrc_layers_align_by_nearest_time_within_500ms() {
    let o = original_lrc("[00:01.00]a\n[00:05.00]b\n[00:09.00]c");
    let es = layer("es", Kind::Translation, Format::Lrc, lrc::parse("[00:01.30]uno\n[00:05.00]dos\n[00:20.00]suelta").unwrap());
    let p = build(&[o, es], "original", &["es".into()], false, 0).unwrap();
    let sec: Vec<Vec<(String, String)>> = p.lines.iter().map(|l| l.secondary.clone()).collect();
    assert_eq!(sec, [vec![("es".into(), "uno".into())], vec![("es".into(), "dos".into())], vec![]]);
  }

  #[test]
  fn plain_layers_align_by_order_or_not_at_all() {
    let o = original_lrc("[00:01.00]a\n[00:02.00]b");
    let ok = text::parse("uno\ndos");
    let bad = text::parse("uno\ndos\ntres");
    assert!(align(&o.doc, &ok).is_some());
    assert!(align(&o.doc, &bad).is_none());
  }

  #[test]
  fn translation_as_primary_falls_back_to_line_timing() {
    let o = original_lrc("[00:01.00]<00:01.00>ai <00:01.50>shiteru<00:02.00>\n[00:03.00]x");
    let es = layer("es", Kind::Translation, Format::Text, text::parse("te amo\ny"));
    let p = build(&[o.clone(), es.clone()], "es", &[], false, 0).unwrap();
    assert_eq!(p.timing, "line");
    assert_eq!(p.lines[0].text, "te amo");
    assert!(p.lines[0].words.is_none(), "original words are never projected on another layer");

    let p = build(&[o, es], "es", &[], true, 0).unwrap();
    assert_eq!(p.timing, "word-estimated");
    let w: Vec<(u64, Option<u64>, &str)> =
      p.lines[0].words.as_ref().unwrap().iter().map(|w| (w.start_ms, w.end_ms, w.text.as_str())).collect();
    // "te" (2) + "amo" (3) sobre 1000..3000.
    assert_eq!(w, [(1000, Some(1800), "te "), (1800, Some(3000), "amo")]);
  }

  #[test]
  fn romanized_with_own_word_times_keeps_them() {
    let o = original_lrc("[00:01.00]<00:01.00>a<00:01.50>b<00:02.00>");
    let ro = layer("ja-Latn", Kind::Romanized, Format::Lrc, lrc::parse("[00:01.00]<00:01.00>ai <00:01.80>bi<00:02.00>").unwrap());
    let p = build(&[o, ro], "ja-Latn", &[], false, 0).unwrap();
    assert_eq!(p.timing, "word");
    let starts: Vec<u64> = p.lines[0].words.as_ref().unwrap().iter().map(|w| w.start_ms).collect();
    assert_eq!(starts, [1000, 1800]);
  }

  #[test]
  fn ttml_transliteration_with_paired_fragments_reuses_original_times() {
    let doc = ttml::parse(r#"<tt xml:lang="ja"><head><metadata><iTunesMetadata><transliterations>
      <transliteration xml:lang="ja-Latn"><text for="L1"><span>kimi</span> <span>no</span></text></transliteration>
      </transliterations></iTunesMetadata></metadata></head><body><div>
      <p begin="1.0" end="2.0" key="L1"><span begin="1.0" end="1.4">君</span><span begin="1.4" end="2.0">の</span></p>
      </div></body></tt>"#).unwrap();
    let ro = layer("ja-Latn", Kind::Romanized, Format::Ttml, doc.extra[0].doc.clone());
    let o = layer("original", Kind::Original, Format::Ttml, doc);
    let map = align(&o.doc, &ro.doc).unwrap();
    assert_eq!(layer_timing(&o.doc, &ro, &map), Timing::WordPaired);
    let p = build(&[o, ro], "ja-Latn", &[], false, 0).unwrap();
    assert_eq!(p.timing, "word-paired");
    let w: Vec<(u64, &str)> = p.lines[0].words.as_ref().unwrap().iter().map(|w| (w.start_ms, w.text.as_str())).collect();
    assert_eq!(w, [(1000, "kimi "), (1400, "no")]);
  }

  #[test]
  fn untimed_original_gives_untimed_package() {
    let o = layer("original", Kind::Original, Format::Text, text::parse("a\nb"));
    let p = build(&[o], "original", &[], true, 0).unwrap();
    assert_eq!(p.timing, "none");
    assert!(p.lines.iter().all(|l| l.start_ms.is_none() && l.words.is_none()));
  }

  #[test]
  fn unalignable_layers_are_not_offered() {
    let o = original_lrc("[00:01.00]a\n[00:02.00]b");
    let ok = layer("es", Kind::Translation, Format::Text, text::parse("uno\ndos"));
    let bad = layer("en", Kind::Translation, Format::Text, text::parse("one"));
    let layers = [o, ok, bad];
    let got: Vec<(&str, Timing)> = offered(&layers).into_iter().map(|(l, t)| (l.id.as_str(), t)).collect();
    assert_eq!(got, [("original", Timing::Line), ("es", Timing::Line)]);
  }

  #[test]
  fn errors_for_unknown_or_repeated_layers() {
    let o = original_lrc("[00:01.00]a");
    assert_eq!(build(std::slice::from_ref(&o), "nope", &[], false, 0), Err(BuildError::UnknownLayer("nope".into())));
    assert_eq!(build(std::slice::from_ref(&o), "original", &["zz".into()], false, 0), Err(BuildError::UnknownLayer("zz".into())));
    assert_eq!(build(&[o], "original", &["original".into()], false, 0), Err(BuildError::Repeated("original".into())));
  }
}
