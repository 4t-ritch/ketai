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
}

pub const fn rgb(hex: u32) -> Color {
  Color::Rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
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

  pub fn accent(&self) -> Style {
    Style::new().fg(self.accent)
  }
}
