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

/// Dibuja las palabras (cada una con el espacio que la sigue, como las da
/// el protocolo) en una imagen de `height` pixeles de alto. Si no entra en
/// `max_width`, achica la letra. `None` si no hay nada visible.
pub fn render(fonts: &Fonts, words: &[&str], height: u32, max_width: u32, unsung: [u8; 3], sung: [u8; 3]) -> Option<Rendered> {
  if height == 0 || max_width == 0 || words.iter().all(|w| w.trim().is_empty()) {
    return None;
  }
  let mut size = height as f32 * 0.8;
  let width = measure(fonts, words, size);
  if width > max_width as f32 {
    size *= max_width as f32 / width;
    // El redondeo puede dejar un pixel de mas: se achica hasta que entre.
    while size > 1.0 && measure(fonts, words, size).ceil() > max_width as f32 {
      size *= 0.98;
    }
  }
  let main = fonts.list[0].as_scaled(PxScale::from(size));
  // La linea queda centrada en alto segun la fuente principal.
  let baseline = (height as f32 - (main.ascent() - main.descent())) / 2.0 + main.ascent();

  let width = measure(fonts, words, size).ceil().max(1.0) as u32;
  let mut alpha = vec![0f32; (width * height) as usize];
  let mut ranges = Vec::with_capacity(words.len());
  let mut x = 0.0f32;
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
  Some(Rendered { unsung: paint(&alpha, width, height, unsung), sung: paint(&alpha, width, height, sung), words: ranges })
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

fn paint(alpha: &[f32], width: u32, height: u32, rgb: [u8; 3]) -> RgbaImage {
  RgbaImage::from_fn(width, height, |x, y| {
    let a = alpha[(y * width + x) as usize];
    image::Rgba([rgb[0], rgb[1], rgb[2], (a.clamp(0.0, 1.0) * 255.0).round() as u8])
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

  #[test]
  fn render_gives_two_images_of_the_same_shape_and_word_ranges() {
    let Some(fonts) = test_fonts() else { return };
    let r = render(&fonts, &["I ", "gave ", "you"], 40, 2000, [10, 20, 30], [200, 100, 50]).unwrap();
    assert_eq!(r.unsung.dimensions(), r.sung.dimensions());
    assert_eq!(r.unsung.height(), 40);
    assert_eq!(r.words.len(), 3);
    // Las palabras van en orden, sin pisarse, y el espacio queda afuera.
    for pair in r.words.windows(2) {
      assert!(pair[0].1 < pair[1].0, "{:?}", r.words);
    }
    assert!(r.words[2].1 <= r.unsung.width() as f32 + 0.5);
    // Mismo dibujo, distinto color.
    let same_alpha = r.unsung.pixels().zip(r.sung.pixels()).all(|(a, b)| a.0[3] == b.0[3]);
    assert!(same_alpha);
    let lit = r.sung.pixels().find(|p| p.0[3] == 255).expect("solid pixels");
    assert_eq!(&lit.0[..3], &[200, 100, 50]);
  }

  #[test]
  fn render_shrinks_a_line_that_does_not_fit() {
    let Some(fonts) = test_fonts() else { return };
    let wide = render(&fonts, &["a very long line of lyrics"], 40, 5000, [0; 3], [255; 3]).unwrap();
    let narrow = render(&fonts, &["a very long line of lyrics"], 40, 200, [0; 3], [255; 3]).unwrap();
    assert!(wide.unsung.width() > 200);
    assert!(narrow.unsung.width() <= 200);
    assert_eq!(narrow.unsung.height(), 40);
  }

  #[test]
  fn render_skips_empty_lines() {
    let Some(fonts) = test_fonts() else { return };
    assert!(render(&fonts, &["  "], 40, 200, [0; 3], [255; 3]).is_none());
  }
}
