//! Colores. Toda la app usa el estilo "inmersivo": fondo oscuro tomado de
//! la caratula actual y un unico color de acento para lo activo.

use ratatui::style::{Color, Modifier, Style};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Theme {
  pub bg: Color,
  pub raised: Color,
  pub fg: Color,
  pub dim: Color,
  pub faint: Color,
  pub accent: Color,
  pub on_accent: Color,
  /// Fila bajo el cursor.
  pub row: Color,
  pub ok: Color,
  pub error: Color,
  /// Color de la parte alta del espectro.
  pub spectrum_top: Color,
  /// Controles interactivos: pestana activa, progreso, volumen, play.
  pub control: Color,
}

pub const fn rgb(hex: u32) -> Color {
  Color::Rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}

/// Mezcla dos colores RGB (`t` = 0 da `a`, 1 da `b`).
pub fn lerp(a: Color, b: Color, t: f32) -> Color {
  match (a, b) {
    (Color::Rgb(r1, g1, b1), Color::Rgb(r2, g2, b2)) => {
      let m = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t.clamp(0.0, 1.0)) as u8;
      Color::Rgb(m(r1, r2), m(g1, g2), m(b1, b2))
    }
    _ => a,
  }
}

impl Default for Theme {
  /// Colores sin caratula (antes de que suene algo).
  fn default() -> Self {
    Self {
      bg: rgb(0x0b1118),
      raised: rgb(0x1c2a3a),
      fg: rgb(0xffffff),
      dim: rgb(0xc8d3e0),
      faint: rgb(0x7f8ea3),
      accent: rgb(0x6fc3ff),
      on_accent: rgb(0x04121f),
      row: rgb(0x16202e),
      ok: rgb(0x7ee787),
      error: rgb(0xff7b8a),
      spectrum_top: rgb(0xff7ab6),
      control: rgb(0xffffff),
    }
  }
}

impl Theme {
  pub fn text(&self) -> Style {
    Style::new().fg(self.fg)
  }

  pub fn faint(&self) -> Style {
    Style::new().fg(self.faint)
  }

  pub fn dim(&self) -> Style {
    Style::new().fg(self.dim)
  }

  pub fn title(&self) -> Style {
    Style::new().fg(self.fg).add_modifier(Modifier::BOLD)
  }

  /// Fondo de los popups: el fondo, un poco mas oscuro.
  pub fn popup(&self) -> Color {
    lerp(self.bg, rgb(0x000000), 0.35)
  }

  /// Lineas de letra que todavia no se cantan (y su traduccion, mas tenue).
  pub fn upcoming(&self) -> Color {
    lerp(self.faint, self.bg, 0.15)
  }

  pub fn upcoming_secondary(&self) -> Color {
    lerp(self.faint, self.bg, 0.4)
  }

  /// Linea del karaoke que se canta: lo ya cantado es el acento aclarado
  /// (mas claro que la UI, mismo tono) y lo que falta queda tenue debajo,
  /// asi lo que se pinta encima resalta.
  pub fn karaoke_sung(&self) -> Color {
    lerp(self.accent, rgb(0xffffff), 0.35)
  }

  pub fn karaoke_unsung(&self) -> Color {
    lerp(self.faint, self.bg, 0.3)
  }

  pub fn accent(&self) -> Style {
    Style::new().fg(self.accent)
  }
}
