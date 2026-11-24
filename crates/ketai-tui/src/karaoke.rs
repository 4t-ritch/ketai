//! Modo karaoke de la vista de Letras: la linea que se canta se dibuja como
//! dos imagenes del mismo tamano, una con el color "sin cantar" y otra con
//! el color "cantado". La cantada se ubica encima, recortada hasta el pixel
//! donde va la cancion, asi el relleno avanza por dentro de las letras.
//!
//! Cada linea se dibuja y se manda una sola vez; en cada cuadro solo cambia
//! el recorte de la cantada (unos 60 bytes), lo que es liviano incluso
//! dentro de un multiplexor.

use std::path::{Path, PathBuf};

use ab_glyph::{Font, FontVec, PxScale, ScaleFont, point};
use image::RgbaImage;

/// Fuentes para dibujar las letras: la primera es la principal; las demas
/// se usan para los caracteres que la principal no tiene (por ejemplo
/// japones o coreano).
pub struct Fonts {
  list: Vec<FontVec>,
}

impl Fonts {
  /// Carga las fuentes que se puedan leer, en orden. `None` si ninguna.
  /// Un `.ttc` (coleccion) aporta su primera fuente.
  pub fn from_paths(paths: &[PathBuf]) -> Option<Fonts> {
    let list: Vec<FontVec> = paths
      .iter()
      .filter_map(|p| std::fs::read(p).ok())
      .filter_map(|data| FontVec::try_from_vec_and_index(data, 0).ok())
      .collect();
    if list.is_empty() { None } else { Some(Fonts { list }) }
  }

  /// La fuente elegida (si hay) y las del sistema por fontconfig: una
  /// monoespaciada en negrita y respaldos para chino, japones y coreano.
  pub fn system(custom: Option<&Path>) -> Option<Fonts> {
    let mut paths: Vec<PathBuf> = custom.map(Path::to_path_buf).into_iter().collect();
    for pattern in ["monospace:bold", "sans:bold", "sans:lang=ja", "sans:lang=ko", "sans:lang=zh-cn"] {
      if let Some(p) = fc_match(pattern)
        && !paths.contains(&p)
      {
        paths.push(p);
      }
    }
    Fonts::from_paths(&paths)
  }

  /// Indice de la primera fuente que tiene el caracter (la principal si
  /// ninguna lo tiene).
  fn pick(&self, c: char) -> usize {
    self.list.iter().position(|f| f.glyph_id(c).0 != 0).unwrap_or(0)
  }
}

fn fc_match(pattern: &str) -> Option<PathBuf> {
  let out = std::process::Command::new("fc-match").args(["-f", "%{file}", pattern]).output().ok()?;
  let path = PathBuf::from(String::from_utf8(out.stdout).ok()?.trim());
  (out.status.success() && path.is_file()).then_some(path)
}

/// Una linea dibujada.
pub struct Rendered {
  pub unsung: RgbaImage,
  pub sung: RgbaImage,
  /// Donde empieza y termina (en pixeles) cada palabra, sin el espacio que
  /// la sigue.
  pub words: Vec<(f32, f32)>,
}

/// Colores de una linea dibujada.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Colors {
  pub background: [u8; 3],
  pub unsung: [u8; 3],
  pub sung: [u8; 3],
}

/// Dibuja las palabras (cada una con el espacio que la sigue, como las da
/// el protocolo) centradas en un lienzo de `width x height` pixeles con el
/// fondo del tema: las celdas debajo de una imagen muestran el fondo de la
/// terminal, no el del tema, asi que la imagen las cubre enteras. Si el
/// texto no entra, achica la letra. `None` si no hay nada visible.
pub fn render(fonts: &Fonts, words: &[&str], width: u32, height: u32, colors: Colors) -> Option<Rendered> {
  if height == 0 || width == 0 || words.iter().all(|w| w.trim().is_empty()) {
    return None;
  }
  // Un margen a los costados para que la letra no toque el borde.
  let max_width = width.saturating_sub(width / 20).max(1);
  let mut size = height as f32 * 0.8;
  let text_width = measure(fonts, words, size);
  if text_width > max_width as f32 {
    size *= max_width as f32 / text_width;
    // El redondeo puede dejar un pixel de mas: se achica hasta que entre.
    while size > 1.0 && measure(fonts, words, size).ceil() > max_width as f32 {
      size *= 0.98;
    }
  }
  let main = fonts.list[0].as_scaled(PxScale::from(size));
  // La linea queda centrada en alto segun la fuente principal.
  let baseline = (height as f32 - (main.ascent() - main.descent())) / 2.0 + main.ascent();

  let left = ((width as f32 - measure(fonts, words, size)) / 2.0).max(0.0).floor();
  let mut alpha = vec![0f32; (width * height) as usize];
  let mut ranges = Vec::with_capacity(words.len());
  let mut x = left;
  let mut prev: Option<(usize, ab_glyph::GlyphId)> = None;
  for word in words {
    let start = x;
    let mut end = x;
    for c in word.chars() {
      let (fi, advance, kern, glyph) = glyph(fonts, c, size, prev);
      x += kern;
      let font = &fonts.list[fi];
      if let Some(outline) = font.outline_glyph(glyph.id.with_scale_and_position(PxScale::from(size), point(x, baseline))) {
        let b = outline.px_bounds();
        outline.draw(|gx, gy, cov| {
          let px = b.min.x as i32 + gx as i32;
          let py = b.min.y as i32 + gy as i32;
          if px >= 0 && py >= 0 && (px as u32) < width && (py as u32) < height {
            let a = &mut alpha[(py as u32 * width + px as u32) as usize];
            *a = a.max(cov);
          }
        });
      }
      x += advance;
      if !c.is_whitespace() {
        end = x;
      }
      prev = Some((fi, glyph.id));
    }
    ranges.push((start, end));
  }
  Some(Rendered {
    unsung: paint(&alpha, width, height, colors.background, colors.unsung),
    sung: paint(&alpha, width, height, colors.background, colors.sung),
    words: ranges,
  })
}

/// Fuente, avance, ajuste por el par con el caracter anterior y glifo.
fn glyph(fonts: &Fonts, c: char, size: f32, prev: Option<(usize, ab_glyph::GlyphId)>) -> (usize, f32, f32, ab_glyph::Glyph) {
  let fi = fonts.pick(c);
  let scaled = fonts.list[fi].as_scaled(PxScale::from(size));
  let id = scaled.glyph_id(c);
  let kern = match prev {
    Some((pf, pid)) if pf == fi => scaled.kern(pid, id),
    _ => 0.0,
  };
  (fi, scaled.h_advance(id), kern, id.with_scale(PxScale::from(size)))
}

fn measure(fonts: &Fonts, words: &[&str], size: f32) -> f32 {
  let mut x = 0.0;
  let mut prev = None;
  for c in words.iter().flat_map(|w| w.chars()) {
    let (fi, advance, kern, glyph) = glyph(fonts, c, size, prev);
    x += kern + advance;
    prev = Some((fi, glyph.id));
  }
  x
}

/// Texto del color dado sobre el fondo, opaco.
fn paint(alpha: &[f32], width: u32, height: u32, bg: [u8; 3], fg: [u8; 3]) -> RgbaImage {
  RgbaImage::from_fn(width, height, |x, y| {
    let a = alpha[(y * width + x) as usize].clamp(0.0, 1.0);
    let mix = |i: usize| (bg[i] as f32 + (fg[i] as f32 - bg[i] as f32) * a).round() as u8;
    image::Rgba([mix(0), mix(1), mix(2), 255])
  })
}

/// Hasta que pixel va el relleno en `pos_ms`. Dentro de una palabra avanza
/// en proporcion a su tiempo; entre palabras queda al final de la ultima
/// cantada. `times` y `xs` van en el mismo orden (una entrada por palabra).
pub fn fill_x(times: &[(u64, u64)], xs: &[(f32, f32)], pos_ms: u64) -> f32 {
  let mut edge = 0.0;
  for (&(start, end), &(x0, x1)) in times.iter().zip(xs) {
    if pos_ms >= end {
      edge = x1;
    } else if pos_ms > start {
      return x0 + (x1 - x0) * (pos_ms - start) as f32 / (end - start) as f32;
    } else {
      break;
    }
  }
  edge
}

/// La linea que la vista pide mostrar en este cuadro.
#[derive(Debug, Clone, PartialEq)]
pub struct Slot {
  /// Celdas que ocupa (todo el ancho de la vista, `size` filas).
  pub area: ratatui::layout::Rect,
  /// Palabras, cada una con el espacio que la sigue.
  pub words: Vec<String>,
  /// Inicio y fin de cada palabra, en ms.
  pub times: Vec<(u64, u64)>,
  pub colors: Colors,
}

/// Lo que ya esta dibujado y mandado.
struct Shown {
  words: Vec<String>,
  size: (u32, u32),
  colors: Colors,
  xs: Vec<(f32, f32)>,
  /// Los datos estan en la terminal (se pierden al cambiar de tamano en
  /// algunos multiplexores).
  sent: bool,
}

/// Estado del modo karaoke: dos ids de imagen propios, que se reusan para
/// cada linea (mandar datos nuevos con el mismo id reemplaza la imagen).
pub struct Karaoke {
  unsung_id: u32,
  sung_id: u32,
  fonts: Option<Option<Fonts>>,
  font: Option<PathBuf>,
  shown: Option<Shown>,
  placed: Vec<crate::kitty::Overlay>,
}

impl Karaoke {
  pub fn new(unsung_id: u32, font: Option<PathBuf>) -> Self {
    Karaoke { unsung_id, sung_id: unsung_id + 1, fonts: None, font, shown: None, placed: Vec::new() }
  }

  /// Carga las fuentes la primera vez. `false` si no hay ninguna (la vista
  /// vuelve al modo clasico).
  pub fn ensure_fonts(&mut self) -> bool {
    let font = self.font.clone();
    self.fonts.get_or_insert_with(|| Fonts::system(font.as_deref())).is_some()
  }

  /// Fuentes ya cargadas (para tests o fuentes elegidas a mano).
  pub fn with_fonts(mut self, fonts: Fonts) -> Self {
    self.fonts = Some(Some(fonts));
    self
  }

  /// Cambio la fuente elegida: se vuelve a cargar y a dibujar.
  pub fn set_font(&mut self, font: Option<PathBuf>) {
    if self.font != font {
      self.font = font;
      self.fonts = None;
      self.shown = None;
    }
  }

  /// La terminal pudo perder las imagenes: se vuelven a mandar y ubicar.
  pub fn invalidate(&mut self) {
    if let Some(s) = &mut self.shown {
      s.sent = false;
    }
    self.placed.clear();
  }

  pub fn ids(&self) -> [u32; 2] {
    [self.unsung_id, self.sung_id]
  }

  /// Lo que hay que mandar a la terminal en este cuadro: la linea nueva si
  /// cambio (las dos imagenes) y las ubicaciones que cambiaron (en general,
  /// solo el recorte de la cantada). `cell` es el tamano de una celda en
  /// pixeles.
  pub fn frame(&mut self, slot: Option<&Slot>, cell: (u16, u16), pos_ms: u64) -> String {
    use crate::kitty::{Overlay, Z_BELOW_BACKGROUND};
    let mut out = String::new();
    let mut overlays = Vec::new();
    if let (Some(slot), Some(Some(fonts))) = (slot, &self.fonts) {
      let size = (slot.area.width as u32 * cell.0 as u32, slot.area.height as u32 * cell.1 as u32);
      let same = self.shown.as_ref().is_some_and(|s| s.words == slot.words && s.size == size && s.colors == slot.colors);
      let sent = self.shown.as_ref().is_some_and(|s| s.sent);
      if !same || !sent {
        let words: Vec<&str> = slot.words.iter().map(String::as_str).collect();
        self.shown = render(fonts, &words, size.0, size.1, slot.colors).map(|r| {
          out.push_str(&crate::kitty::transmit(self.unsung_id, &png(&r.unsung)));
          out.push_str(&crate::kitty::transmit(self.sung_id, &png(&r.sung)));
          Shown { words: slot.words.clone(), size, colors: slot.colors, xs: r.words, sent: true }
        });
        // Datos nuevos con el mismo id borran las ubicaciones anteriores.
        self.placed.clear();
      }
      if let Some(s) = &self.shown {
        let (x, y) = (slot.area.x, slot.area.y);
        overlays.push(Overlay { image: self.unsung_id, x, y, crop: None, z: Z_BELOW_BACKGROUND - 1 });
        let edge = fill_x(&slot.times, &s.xs, pos_ms).round() as u32;
        if edge > 0 {
          overlays.push(Overlay { image: self.sung_id, x, y, crop: Some((edge.min(s.size.0), s.size.1)), z: Z_BELOW_BACKGROUND });
        }
      }
    }
    out.push_str(&crate::kitty::overlay_update(&self.placed, &overlays));
    self.placed = overlays;
    out
  }

  /// Ubicaciones actuales, para volver a ponerlas cada tanto.
  pub fn refresh(&self) -> String {
    crate::kitty::overlay_refresh(&self.placed)
  }
}

/// Palabras de una linea y el inicio y fin de cada una, en ms.
pub type LineWords = (Vec<String>, Vec<(u64, u64)>);

/// Palabras y tiempos de una linea de `lyrics.get`. Sin tiempo por palabra,
/// la linea entera es una sola palabra que dura toda la linea. Una palabra
/// sin fin termina donde empieza la siguiente (o con la linea).
pub fn line_words(line: &serde_json::Value, next_start: Option<u64>) -> Option<LineWords> {
  let start = line["start_ms"].as_u64()?;
  let line_end = line["end_ms"].as_u64().or(next_start).unwrap_or(start + 5000).max(start + 1);
  let Some(words) = line["words"].as_array().filter(|w| !w.is_empty()) else {
    let text = line["text"].as_str().unwrap_or("").to_string();
    return (!text.trim().is_empty()).then(|| (vec![text], vec![(start, line_end)]));
  };
  let mut texts = Vec::new();
  let mut times = Vec::new();
  for (i, w) in words.iter().enumerate() {
    let s = w["start_ms"].as_u64().unwrap_or(start);
    let next = words.get(i + 1).and_then(|n| n["start_ms"].as_u64()).unwrap_or(line_end);
    let e = w["end_ms"].as_u64().unwrap_or(next).max(s + 1);
    texts.push(w["text"].as_str().unwrap_or("").to_string());
    times.push((s, e));
  }
  Some((texts, times))
}

/// PNG de una imagen con transparencia.
pub fn png(img: &RgbaImage) -> Vec<u8> {
  let mut out = Vec::new();
  let _ = img.write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png);
  out
}

#[cfg(test)]
mod tests {
  use super::*;

  const XS: [(f32, f32); 3] = [(0.0, 40.0), (50.0, 70.0), (80.0, 120.0)];
  const TIMES: [(u64, u64); 3] = [(1000, 2000), (2000, 2500), (3000, 4000)];

  #[test]
  fn fill_moves_inside_each_word_by_its_time() {
    assert_eq!(fill_x(&TIMES, &XS, 0), 0.0, "before the first word");
    assert_eq!(fill_x(&TIMES, &XS, 1500), 20.0, "half of the first word");
    assert_eq!(fill_x(&TIMES, &XS, 2250), 60.0, "half of the second");
    assert_eq!(fill_x(&TIMES, &XS, 2800), 70.0, "a pause keeps the end of the last sung word");
    assert_eq!(fill_x(&TIMES, &XS, 9000), 120.0, "the whole line after the last word");
  }

  /// Una fuente del sistema para los tests de dibujo; sin ella se saltean
  /// (no se lee nada del home del usuario).
  fn test_fonts() -> Option<Fonts> {
    let paths: Vec<PathBuf> = ["/usr/share/fonts/TTF/DejaVuSans-Bold.ttf", "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf"]
      .iter()
      .map(PathBuf::from)
      .filter(|p| p.is_file())
      .collect();
    Fonts::from_paths(&paths)
  }

  const C: Colors = Colors { background: [10, 20, 30], unsung: [90, 90, 90], sung: [200, 100, 50] };

  #[test]
  fn render_fills_the_canvas_with_the_background_and_centers_the_words() {
    let Some(fonts) = test_fonts() else { return };
    let r = render(&fonts, &["I ", "gave ", "you"], 600, 40, C).unwrap();
    assert_eq!(r.unsung.dimensions(), (600, 40));
    assert_eq!(r.sung.dimensions(), (600, 40));
    assert_eq!(r.words.len(), 3);
    // Las palabras van en orden, sin pisarse, y el espacio queda afuera.
    for pair in r.words.windows(2) {
      assert!(pair[0].1 < pair[1].0, "{:?}", r.words);
    }
    // Centrado: el mismo margen a cada lado (salvo redondeo).
    let (l, rgt) = (r.words[0].0, 600.0 - r.words[2].1);
    assert!((l - rgt).abs() < 4.0, "{l} vs {rgt}");
    // Opaco: las esquinas tienen el fondo del tema.
    assert_eq!(r.unsung.get_pixel(0, 0).0, [10, 20, 30, 255]);
    assert!(r.sung.pixels().all(|p| p.0[3] == 255));
    // Mismo dibujo, distinto color de letra.
    assert!(r.sung.pixels().any(|p| p.0 == [200, 100, 50, 255]));
    assert!(r.unsung.pixels().any(|p| p.0 == [90, 90, 90, 255]));
    assert!(!r.unsung.pixels().any(|p| p.0 == [200, 100, 50, 255]));
  }

  #[test]
  fn render_shrinks_a_line_that_does_not_fit() {
    let Some(fonts) = test_fonts() else { return };
    let line = ["a very long line of lyrics"];
    let wide = render(&fonts, &line, 5000, 40, C).unwrap();
    let narrow = render(&fonts, &line, 200, 40, C).unwrap();
    assert!(wide.words[0].1 - wide.words[0].0 > 200.0);
    assert!(narrow.words[0].0 >= 0.0 && narrow.words[0].1 <= 200.0, "{:?}", narrow.words);
    assert_eq!(narrow.unsung.dimensions(), (200, 40));
  }

  #[test]
  fn render_skips_empty_lines() {
    let Some(fonts) = test_fonts() else { return };
    assert!(render(&fonts, &["  "], 200, 40, C).is_none());
  }

  fn slot(words: &[&str]) -> Slot {
    Slot {
      area: ratatui::layout::Rect::new(0, 10, 30, 3),
      words: words.iter().map(|w| w.to_string()).collect(),
      times: vec![(1000, 2000), (2000, 3000)],
      colors: C,
    }
  }

  #[test]
  fn frames_send_a_line_once_and_then_only_the_crop() {
    let Some(fonts) = test_fonts() else { return };
    let mut k = Karaoke::new(500, None).with_fonts(fonts);
    let s = slot(&["hold ", "me"]);
    let first = k.frame(Some(&s), (10, 20), 0);
    assert!(first.contains("a=t,f=100,i=500,") && first.contains("a=t,f=100,i=501,"), "both images go out");
    assert!(first.contains("a=p,i=500,p=1,C=1,"), "unsung placed whole");
    assert!(!first.contains("a=p,i=501"), "nothing sung yet");
    // Avanza: solo la ubicacion de la cantada, recortada (300x60 pixeles).
    let next = k.frame(Some(&s), (10, 20), 1500);
    assert!(!next.contains("a=t") && !next.contains("i=500"), "{next:?}");
    assert!(next.contains("a=p,i=501,p=1,x=0,y=0,w=") && next.contains(",h=60,"), "{next:?}");
    assert_eq!(k.frame(Some(&s), (10, 20), 1500), "", "same position: nothing");
    // El multiplexor perdio las imagenes: se mandan de nuevo.
    k.invalidate();
    let again = k.frame(Some(&s), (10, 20), 1500);
    assert!(again.contains("a=t,f=100,i=500,") && again.contains("a=p,i=501,"), "{again:?}");
    // Otra linea: imagenes nuevas con los mismos ids.
    let other = k.frame(Some(&slot(&["so ", "sweet"])), (10, 20), 1500);
    assert!(other.contains("a=t,f=100,i=500,"));
    // Fuera de la vista: se quitan las ubicaciones, sin borrar otras imagenes.
    let gone = k.frame(None, (10, 20), 1500);
    assert_eq!(gone, "\x1b_Ga=d,d=i,i=500,q=2\x1b\\\x1b_Ga=d,d=i,i=501,q=2\x1b\\");
    assert!(!k.refresh().contains("i=500"));
  }

  #[test]
  fn words_and_times_come_from_the_protocol_line() {
    use serde_json::json;
    let line = json!({"start_ms": 1000, "end_ms": 3000, "text": "a b", "words": [
      {"start_ms": 1000, "end_ms": null, "text": "a "}, {"start_ms": 1600, "end_ms": null, "text": "b"}]});
    assert_eq!(line_words(&line, Some(4000)), Some((vec!["a ".into(), "b".into()], vec![(1000, 1600), (1600, 3000)])));
    // Sin tiempo por palabra: toda la linea como una palabra, hasta la siguiente.
    let line = json!({"start_ms": 1000, "end_ms": null, "text": "whole line", "words": null});
    assert_eq!(line_words(&line, Some(4000)), Some((vec!["whole line".into()], vec![(1000, 4000)])));
    assert_eq!(line_words(&json!({"start_ms": 5, "text": " "}), None), None);
    assert_eq!(line_words(&json!({"start_ms": null, "text": "x"}), None), None);
  }
}
