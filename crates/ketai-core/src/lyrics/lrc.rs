//! LRC, con la extension A2 (tiempo por palabra).
//!
//! - `[mm:ss.xx]texto`: tiempo por linea. Una linea puede tener varias
//!   marcas (`[00:12.00][01:30.00]estribillo`): se repite en cada tiempo.
//! - `<mm:ss.xx>palabra`: tiempo de inicio de cada palabra (A2). El final de
//!   una palabra es el inicio de la siguiente, o la marca final de la linea.
//! - `[offset:+250]`: offset del archivo en ms.
//! - Otras etiquetas (`[ar:]`, `[ti:]`, ...) se ignoran.

use super::{Doc, Line, Word, parse_time};

/// Devuelve None si el texto no parece LRC (ninguna marca de tiempo).
pub fn parse(text: &str) -> Option<Doc> {
  let mut doc = Doc::default();
  for raw in text.lines() {
    let mut rest = raw.trim();
    let mut times = Vec::new();
    // Etiquetas `[...]` al principio de la linea.
    while let Some(inner) = rest.strip_prefix('[') {
      let Some(close) = inner.find(']') else { break };
      let tag = &inner[..close];
      rest = &inner[close + 1..];
      if let Some(value) = tag.strip_prefix("offset:") {
        doc.offset_ms = value.trim().parse().unwrap_or(0);
      } else if let Some(t) = parse_time(tag) {
        times.push(t);
      }
    }
    if times.is_empty() {
      continue;
    }
    let (text, words) = parse_words(rest.trim());
    for t in times {
      doc.lines.push(Line { start_ms: Some(t), text: text.clone(), words: words.clone(), ..Default::default() });
    }
  }
  if doc.lines.is_empty() {
    return None;
  }
  doc.lines.sort_by_key(|l| l.start_ms);
  // El final de cada linea es el inicio de la siguiente.
  for i in 0..doc.lines.len() {
    let next = doc.lines.get(i + 1).and_then(|l| l.start_ms);
    doc.lines[i].end_ms = next;
  }
  // Una marca sin texto ("[00:04.00]") es una pausa: ya cerro la linea
  // anterior (su inicio es el final de esa) y no es una linea.
  doc.lines.retain(|l| !l.text.is_empty() || l.words.is_some());
  if doc.lines.is_empty() {
    return None;
  }
  for line in &mut doc.lines {
    if let Some(words) = &mut line.words
      && let Some(last) = words.last_mut()
    {
      match last.end_ms {
        // La marca final de A2 tambien cierra la linea: no queda encendida
        // hasta la siguiente (igual que el `end` de TTML).
        Some(end) => line.end_ms = Some(line.end_ms.map_or(end, |e| e.min(end))),
        None => last.end_ms = line.end_ms,
      }
    }
  }
  Some(doc)
}

/// Separa las marcas `<mm:ss.xx>` de A2. Devuelve el texto limpio y las
/// palabras con tiempo, si las hay.
fn parse_words(s: &str) -> (String, Option<Vec<Word>>) {
  if !s.contains('<') {
    return (s.to_string(), None);
  }
  let mut words: Vec<Word> = Vec::new();
  let mut clean = String::new();
  let mut rest = s;
  let mut current: Option<(u64, String)> = None;
  while !rest.is_empty() {
    if let Some(inner) = rest.strip_prefix('<')
      && let Some(close) = inner.find('>')
      && let Some(t) = parse_time(&inner[..close])
    {
      if let Some((start, text)) = current.take() {
        push_word(&mut words, start, Some(t), text);
      }
      current = Some((t, String::new()));
      rest = &inner[close + 1..];
      continue;
    }
    let ch = rest.chars().next().expect("non-empty");
    clean.push(ch);
    if let Some((_, text)) = &mut current {
      text.push(ch);
    }
    rest = &rest[ch.len_utf8()..];
  }
  if let Some((start, text)) = current {
    push_word(&mut words, start, None, text);
  }
  // El espacio antes de la marca final no es parte de la ultima palabra.
  if let Some(last) = words.last_mut() {
    last.text.truncate(last.text.trim_end().len());
  }
  let clean = clean.trim().to_string();
  if words.is_empty() { (clean, None) } else { (clean, Some(words)) }
}

/// Agrega una palabra; una marca final sin texto solo cierra la anterior.
fn push_word(words: &mut Vec<Word>, start: u64, end: Option<u64>, text: String) {
  if text.trim().is_empty() {
    return;
  }
  words.push(Word { start_ms: start, end_ms: end, text });
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::lyrics::Timing;

  #[test]
  fn parses_line_times_offset_and_repeated_tags() {
    let doc = parse("[ar:Someone]\n[offset:+250]\n[00:12.00]first\n[00:20.50][01:00.00]chorus\n[00:15.00]second\n").unwrap();
    assert_eq!(doc.offset_ms, 250);
    let got: Vec<(Option<u64>, &str)> = doc.lines.iter().map(|l| (l.start_ms, l.text.as_str())).collect();
    assert_eq!(got, [(Some(12000), "first"), (Some(15000), "second"), (Some(20500), "chorus"), (Some(60000), "chorus")]);
    assert_eq!(doc.lines[0].end_ms, Some(15000));
    assert_eq!(doc.lines[3].end_ms, None);
    assert_eq!(doc.timing(), Timing::Line);
  }

  #[test]
  fn parses_a2_word_times() {
    let doc = parse("[00:12.00]<00:12.00>kimi <00:12.60>no <00:12.90>koe<00:13.50>\n[00:14.00]next").unwrap();
    let line = &doc.lines[0];
    assert_eq!(line.text, "kimi no koe");
    let words = line.words.as_ref().unwrap();
    let got: Vec<(u64, Option<u64>, &str)> = words.iter().map(|w| (w.start_ms, w.end_ms, w.text.as_str())).collect();
    assert_eq!(got, [(12000, Some(12600), "kimi "), (12600, Some(12900), "no "), (12900, Some(13500), "koe")]);
    assert_eq!(doc.timing(), Timing::Word);
  }

  #[test]
  fn last_word_without_closing_tag_ends_with_the_line() {
    let doc = parse("[00:01.00]<00:01.00>a <00:01.50>b\n[00:03.00]c").unwrap();
    let words = doc.lines[0].words.as_ref().unwrap();
    assert_eq!(words[1].end_ms, Some(3000));
  }

  #[test]
  fn negative_offset_and_non_lrc_text() {
    assert_eq!(parse("[offset:-300]\n[00:01.00]x").unwrap().offset_ms, -300);
    assert!(parse("just some lyrics\nwithout times").is_none());
  }

  #[test]
  fn an_a2_closing_tag_ends_the_line_too() {
    // La marca final de A2 dice cuando termina de cantarse la linea: la
    // linea no queda encendida hasta que empieza la siguiente (como en TTML).
    let doc = parse("[00:01.00]<00:01.00>a <00:01.50>b <00:02.00>\n[00:05.00]c").unwrap();
    let line = &doc.lines[0];
    assert_eq!(line.end_ms, Some(2000));
    let words = line.words.as_ref().unwrap();
    assert_eq!((words[1].end_ms, words[1].text.as_str()), (Some(2000), "b"), "no trailing space on the last word");
    // Sin marca final, la linea dura hasta la siguiente, como antes.
    let doc = parse("[00:01.00]<00:01.00>a <00:01.50>b\n[00:03.00]c").unwrap();
    assert_eq!(doc.lines[0].end_ms, Some(3000));
  }

  #[test]
  fn an_empty_time_tag_ends_the_previous_line_and_is_not_a_line() {
    // "[00:04.00]" sin texto es una pausa: cierra la linea anterior.
    let doc = parse("[00:01.00]a\n[00:04.00]\n[00:10.00]b\n[00:12.00]").unwrap();
    let got: Vec<(&str, Option<u64>, Option<u64>)> = doc.lines.iter().map(|l| (l.text.as_str(), l.start_ms, l.end_ms)).collect();
    assert_eq!(got, [("a", Some(1000), Some(4000)), ("b", Some(10000), Some(12000))]);
  }
}
