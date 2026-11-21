//! Texto plano: una linea por linea no vacia, sin tiempos.

use super::{Doc, Line};

pub fn parse(text: &str) -> Doc {
  let lines = text
    .lines()
    .map(str::trim)
    .filter(|l| !l.is_empty())
    .map(|l| Line { text: l.to_string(), ..Default::default() })
    .collect();
  Doc { lines, ..Default::default() }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::lyrics::Timing;

  #[test]
  fn keeps_non_empty_lines_without_times() {
    let doc = parse("uno\n\n  dos  \r\ntres\n");
    let texts: Vec<&str> = doc.lines.iter().map(|l| l.text.as_str()).collect();
    assert_eq!(texts, ["uno", "dos", "tres"]);
    assert_eq!(doc.timing(), Timing::None);
  }
}
