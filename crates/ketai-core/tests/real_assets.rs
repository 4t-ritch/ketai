//! Pruebas pendientes por falta de archivos reales.
//!
//! Las letras ID3 y TTML solo se probaron con datos armados en los tests. Lo
//! que falta es probarlas con archivos reales: MP3 etiquetados por otros
//! programas (Mp3tag, foobar2000, MusicBrainz Picard, kid3...) y TTML
//! exportados de Apple Music.
//!
//! Esos archivos tienen derechos de autor, asi que no entran al repo: van en
//! `samples/` (ignorado por git). Estos tests estan marcados `#[ignore]` y se
//! corren a mano con:
//!
//! ```sh
//! cargo test -p ketai-core --test real_assets -- --ignored
//! ```
//!
//! Estructura esperada:
//!
//! - `samples/lyrics/id3/*.mp3`: MP3 con letra embebida (USLT o SYLT).
//! - `samples/lyrics/ttml/*.ttml`: TTML de Apple Music.
//!
//! Si la carpeta no existe o esta vacia, el test falla con un mensaje claro
//! en lugar de pasar sin probar nada. Ver `docs/pending-tests.md`.

use std::{
  fs,
  path::{Path, PathBuf},
};

use ketai_core::lyrics::{
  Kind,
  layers::{self},
  ttml,
};

fn assets(sub: &str, ext: &str) -> Vec<PathBuf> {
  let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../samples/lyrics").join(sub);
  let files: Vec<PathBuf> = fs::read_dir(&dir)
    .unwrap_or_else(|_| panic!("missing assets: put real files in {}", dir.display()))
    .filter_map(|e| e.ok().map(|e| e.path()))
    .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case(ext)))
    .collect();
  assert!(!files.is_empty(), "missing assets: no .{ext} files in {}", dir.display());
  files
}

#[test]
#[ignore = "needs real MP3 files with embedded lyrics in samples/lyrics/id3/"]
fn real_id3_lyrics_are_read() {
  for path in assets("id3", "mp3") {
    let found = layers::discover(&path);
    let name = path.display();
    assert!(!found.invalid, "{name}: lyrics present but unreadable");
    let original = found.layers.iter().find(|l| l.kind == Kind::Original);
    let original = original.unwrap_or_else(|| panic!("{name}: no original layer"));
    assert_eq!(original.origin.as_str(), "embedded", "{name}");
    assert!(original.doc.lines.iter().any(|l| !l.text.trim().is_empty()), "{name}: empty lyrics");
    // Las demas capas (traducciones, romanizaciones) se listan para revisarlas
    // a ojo contra lo que muestra el programa que las etiqueto.
    for l in &found.layers {
      println!("{name}: {} {} {:?} {}", l.id, l.kind.as_str(), l.language, l.doc.timing().as_str());
    }
  }
}

#[test]
#[ignore = "needs real Apple Music TTML files in samples/lyrics/ttml/"]
fn real_apple_music_ttml_is_parsed() {
  for path in assets("ttml", "ttml") {
    let name = path.display();
    let xml = fs::read_to_string(&path).unwrap();
    let doc = ttml::parse(&xml).unwrap_or_else(|| panic!("{name}: TTML not parsed"));
    assert!(!doc.lines.is_empty(), "{name}: no lines");
    assert!(doc.lines.iter().all(|l| l.start_ms.is_some()), "{name}: lines without timing");
    // Si el archivo declara traducciones o transliteraciones, deben salir
    // como capas extra con contenido.
    if xml.contains("<translation") || xml.contains("<transliteration") {
      assert!(!doc.extra.is_empty(), "{name}: extra layers declared but not read");
      assert!(doc.extra.iter().all(|e| !e.doc.lines.is_empty()), "{name}: empty extra layer");
    }
    println!("{name}: {} lines, timing {}, {} extra", doc.lines.len(), doc.timing().as_str(), doc.extra.len());
  }
}
