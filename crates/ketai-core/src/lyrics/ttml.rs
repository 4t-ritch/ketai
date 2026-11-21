//! TTML (formato de Apple Music).
//!
//! - `<p begin end itunes:key ttm:agent>`: una linea.
//! - `<span begin end>` dentro de `<p>`: una palabra o silaba.
//! - `<span ttm:role="x-bg">`: coro de fondo; se vuelve una linea aparte con
//!   `background: true`, a continuacion de su linea principal.
//! - `<ttm:agent xml:id>` con `<ttm:name>`: cantantes (duetos).
//! - `iTunesMetadata` -> `<translations>` y `<transliterations>`, con
//!   `<text for="key">`: capas extra (traducciones y romanizaciones).
//!
//! Los atributos se buscan por nombre local, sin prefijo de namespace.

use quick_xml::{
  Reader,
  events::{BytesStart, Event},
};

use super::{Doc, ExtraLayer, Kind, Line, Word, parse_time};

#[derive(Debug, Default)]
struct Element {
  name: String,
  attrs: Vec<(String, String)>,
  children: Vec<Node>,
}

#[derive(Debug)]
enum Node {
  Element(Element),
  Text(String),
}

impl Element {
  fn attr(&self, name: &str) -> Option<&str> {
    self.attrs.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
  }

  fn elements(&self) -> impl Iterator<Item = &Element> {
    self.children.iter().filter_map(|n| match n {
      Node::Element(e) => Some(e),
      Node::Text(_) => None,
    })
  }

  /// Todos los descendientes con ese nombre, en orden de documento.
  fn find_all<'a>(&'a self, name: &str, out: &mut Vec<&'a Element>) {
    for e in self.elements() {
      if e.name == name {
        out.push(e);
      }
      e.find_all(name, out);
    }
  }

  fn text(&self) -> String {
    let mut s = String::new();
    for n in &self.children {
      match n {
        Node::Text(t) => s.push_str(t),
        Node::Element(e) => s.push_str(&e.text()),
      }
    }
    s
  }
}

fn local(name: &[u8]) -> String {
  let name = String::from_utf8_lossy(name);
  match name.rsplit_once(':') {
    Some((_, l)) => l.to_string(),
    None => name.into_owned(),
  }
}

fn element(start: &BytesStart) -> Element {
  let attrs = start
    .attributes()
    .flatten()
    .map(|a| (local(a.key.as_ref()), a.unescape_value().map(|v| v.into_owned()).unwrap_or_default()))
    .collect();
  Element { name: local(start.name().as_ref()), attrs, children: Vec::new() }
}

/// Arma el arbol y devuelve el elemento `<tt>`.
fn tree(xml: &str) -> Option<Element> {
  let mut reader = Reader::from_str(xml);
  // Raiz sintetica: contiene al `<tt>` y lo que haya alrededor.
  let mut stack: Vec<Element> = vec![Element::default()];
  loop {
    match reader.read_event().ok()? {
      Event::Start(s) => stack.push(element(&s)),
      Event::Empty(s) => stack.last_mut()?.children.push(Node::Element(element(&s))),
      Event::End(_) => {
        let done = stack.pop()?;
        stack.last_mut()?.children.push(Node::Element(done));
      }
      Event::Text(t) => {
        let text = t.unescape().ok()?.into_owned();
        stack.last_mut()?.children.push(Node::Text(text));
      }
      Event::CData(t) => stack.last_mut()?.children.push(Node::Text(String::from_utf8_lossy(&t).into_owned())),
      Event::Eof => break,
      _ => {}
    }
  }
  // Etiquetas sin cerrar: el XML esta roto.
  if stack.len() != 1 {
    return None;
  }
  stack.pop()?.children.into_iter().find_map(|n| match n {
    Node::Element(e) if e.name == "tt" => Some(e),
    _ => None,
  })
}

/// Devuelve None si el texto no es TTML valido.
pub fn parse(xml: &str) -> Option<Doc> {
  let tt = tree(xml)?;
  let mut doc = Doc { language: tt.attr("lang").map(str::to_string), ..Default::default() };

  let mut agents = Vec::new();
  tt.find_all("agent", &mut agents);
  for a in agents {
    if let Some(id) = a.attr("id") {
      let mut names = Vec::new();
      a.find_all("name", &mut names);
      let name = names.first().map(|n| n.text().trim().to_string()).unwrap_or_else(|| id.to_string());
      doc.agents.push((id.to_string(), name));
    }
  }

  let mut ps = Vec::new();
  tt.find_all("p", &mut ps);
  for p in ps {
    let (main, bg) = paragraph(p);
    doc.lines.push(main);
    doc.lines.extend(bg);
  }

  for (container, kind) in [("translation", Kind::Translation), ("transliteration", Kind::Romanized)] {
    let mut blocks = Vec::new();
    tt.find_all(container, &mut blocks);
    for block in blocks {
      let Some(lang) = block.attr("lang") else { continue };
      let mut texts = Vec::new();
      block.find_all("text", &mut texts);
      let lines = texts
        .iter()
        .map(|t| {
          let (mut line, _) = paragraph(t);
          line.key = t.attr("for").map(str::to_string);
          line
        })
        .collect();
      doc.extra.push(ExtraLayer {
        kind,
        language: lang.to_string(),
        doc: Doc { lines, language: Some(lang.to_string()), ..Default::default() },
      });
    }
  }
  Some(doc)
}

/// Convierte un `<p>` (o un `<text>` de una capa extra) en su linea y las
/// lineas de coro de fondo que tenga dentro.
fn paragraph(p: &Element) -> (Line, Vec<Line>) {
  let key = p.attr("key").map(str::to_string);
  let mut line = Line {
    key: key.clone(),
    start_ms: p.attr("begin").and_then(parse_time),
    end_ms: p.attr("end").and_then(parse_time),
    agent: p.attr("agent").map(str::to_string),
    ..Default::default()
  };
  let mut words: Vec<Word> = Vec::new();
  let mut background = Vec::new();
  for node in &p.children {
    match node {
      Node::Text(t) => {
        line.text.push_str(t);
        // El espacio entre spans pertenece a la palabra anterior.
        if let Some(last) = words.last_mut() {
          last.text.push_str(t);
        }
      }
      Node::Element(span) if span.name == "span" && span.attr("role") == Some("x-bg") => {
        let (mut bg, _) = paragraph(span);
        bg.background = true;
        bg.agent = line.agent.clone();
        bg.key = key.as_ref().map(|k| format!("{k}-bg"));
        bg.text = bg.text.trim().to_string();
        background.push(bg);
      }
      Node::Element(span) if span.name == "span" => {
        let text = span.text();
        line.text.push_str(&text);
        line.fragments += 1;
        if let Some(start) = span.attr("begin").and_then(parse_time) {
          words.push(Word { start_ms: start, end_ms: span.attr("end").and_then(parse_time), text });
        }
      }
      Node::Element(other) => line.text.push_str(&other.text()),
    }
  }
  line.text = line.text.split_whitespace().collect::<Vec<_>>().join(" ");
  for w in &mut words {
    let trailing_space = w.text.ends_with(char::is_whitespace);
    w.text = w.text.split_whitespace().collect::<Vec<_>>().join(" ");
    if trailing_space {
      w.text.push(' ');
    }
  }
  if let Some(last) = words.last_mut() {
    last.text = last.text.trim_end().to_string();
  }
  if !words.is_empty() {
    line.words = Some(words);
  }
  // Una linea de coro hereda tiempos de sus palabras si no los trae.
  for bg in &mut background {
    if bg.start_ms.is_none() {
      bg.start_ms = bg.words.as_ref().and_then(|w| w.first()).map(|w| w.start_ms);
      bg.end_ms = bg.words.as_ref().and_then(|w| w.last()).and_then(|w| w.end_ms);
    }
  }
  (line, background)
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::lyrics::Timing;

  const SAMPLE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<tt xmlns="http://www.w3.org/ns/ttml" xmlns:ttm="http://www.w3.org/ns/ttml#metadata"
    xmlns:itunes="http://music.apple.com/lyric-ttml-internal" xml:lang="ja">
  <head>
    <metadata>
      <ttm:agent type="person" xml:id="v1"><ttm:name type="full">Lead</ttm:name></ttm:agent>
      <ttm:agent type="person" xml:id="v2"/>
      <iTunesMetadata xmlns="http://music.apple.com/lyric-ttml-internal">
        <translations>
          <translation type="replacement" xml:lang="es">
            <text for="L1">tu voz</text>
            <text for="L2">otra</text>
          </translation>
        </translations>
        <transliterations>
          <transliteration xml:lang="ja-Latn">
            <text for="L1"><span begin="12.000" end="12.600">kimi</span> <span begin="12.600" end="12.900">no</span></text>
          </transliteration>
        </transliterations>
      </iTunesMetadata>
    </metadata>
  </head>
  <body>
    <div>
      <p begin="12.000" end="15.500" itunes:key="L1" ttm:agent="v1"><span begin="12.000" end="12.600">君</span><span begin="12.600" end="12.900">の</span>
        <span ttm:role="x-bg"><span begin="13.000" end="13.400">(ah)</span></span></p>
      <p begin="00:16.000" end="00:18.000" itunes:key="L2" ttm:agent="v2">plain &amp; simple</p>
    </div>
  </body>
</tt>"#;

  #[test]
  fn parses_lines_words_agents_and_background() {
    let doc = parse(SAMPLE).unwrap();
    assert_eq!(doc.language.as_deref(), Some("ja"));
    assert_eq!(doc.agents, [("v1".to_string(), "Lead".to_string()), ("v2".to_string(), "v2".to_string())]);
    assert_eq!(doc.lines.len(), 3);

    let l1 = &doc.lines[0];
    assert_eq!((l1.key.as_deref(), l1.start_ms, l1.end_ms), (Some("L1"), Some(12000), Some(15500)));
    assert_eq!(l1.text, "君の");
    assert_eq!(l1.agent.as_deref(), Some("v1"));
    let words: Vec<&str> = l1.words.as_ref().unwrap().iter().map(|w| w.text.as_str()).collect();
    assert_eq!(words, ["君", "の"]);

    let bg = &doc.lines[1];
    assert!(bg.background);
    assert_eq!((bg.text.as_str(), bg.start_ms, bg.end_ms), ("(ah)", Some(13000), Some(13400)));
    assert_eq!(bg.key.as_deref(), Some("L1-bg"));

    let l2 = &doc.lines[2];
    assert_eq!((l2.text.as_str(), l2.start_ms, l2.words.is_none()), ("plain & simple", Some(16000), true));
    assert_eq!(doc.timing(), Timing::Word);
  }

  #[test]
  fn parses_translations_and_transliterations() {
    let doc = parse(SAMPLE).unwrap();
    assert_eq!(doc.extra.len(), 2);
    let es = doc.extra.iter().find(|e| e.kind == Kind::Translation).unwrap();
    assert_eq!(es.language, "es");
    let got: Vec<(Option<&str>, &str)> = es.doc.lines.iter().map(|l| (l.key.as_deref(), l.text.as_str())).collect();
    assert_eq!(got, [(Some("L1"), "tu voz"), (Some("L2"), "otra")]);

    let ro = doc.extra.iter().find(|e| e.kind == Kind::Romanized).unwrap();
    assert_eq!(ro.language, "ja-Latn");
    let line = &ro.doc.lines[0];
    assert_eq!(line.text, "kimi no");
    let words: Vec<(u64, &str)> = line.words.as_ref().unwrap().iter().map(|w| (w.start_ms, w.text.as_str())).collect();
    assert_eq!(words, [(12000, "kimi "), (12600, "no")]);
  }

  #[test]
  fn rejects_non_ttml() {
    assert!(parse("[00:01.00]lrc").is_none());
    assert!(parse("<html><body>x</body></html>").is_none());
    assert!(parse("<tt><p>unclosed</tt>").is_none());
  }
}
