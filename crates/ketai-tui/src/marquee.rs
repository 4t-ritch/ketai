//! Texto que no entra en su lugar: se desplaza para que se lea completo.
//!
//! Espera `PAUSE_MS` quieto al principio, avanza un caracter cada `STEP_MS`,
//! y al terminar muestra un separador y vuelve a empezar. Funcion pura del
//! tiempo: el dibujo pasa los milisegundos (`App::anim_ms`), asi los tests
//! fijan el instante.

use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

/// Tiempo quieto al principio de cada vuelta.
pub const PAUSE_MS: u64 = 1_500;
/// Tiempo por caracter.
pub const STEP_MS: u64 = 250;
/// Separador entre el final y el principio.
const GAP: &str = "  •  ";

/// Lo que se ve de `text` en `width` columnas en el instante `t_ms`.
pub fn visible(text: &str, width: usize, t_ms: u64) -> String {
  if text.width() <= width {
    return text.to_string();
  }
  let looped: Vec<char> = text.chars().chain(GAP.chars()).collect();
  let cycle = looped.len() as u64;
  let t = t_ms % (PAUSE_MS + cycle * STEP_MS);
  let offset = if t < PAUSE_MS { 0 } else { ((t - PAUSE_MS) / STEP_MS) as usize };
  let mut out = String::new();
  let mut used = 0;
  for c in looped.iter().cycle().skip(offset) {
    let w = c.width().unwrap_or(0);
    // Un caracter ancho que no entra entero no se corta a la mitad.
    if used + w > width {
      break;
    }
    out.push(*c);
    used += w;
  }
  out
}
