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
}
