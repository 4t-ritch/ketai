//! Dibujo. Cada vista es una funcion pura del estado de la app.

use ratatui::{
  Frame,
  layout::Rect,
  style::{Modifier, Style},
  text::{Line, Span},
};

use crate::app::{App, Tab};

pub fn draw(f: &mut Frame, app: &App) {
  let t = &app.theme;
  let area = f.area();
  f.buffer_mut().set_style(area, Style::new().bg(t.bg));
  header(f, app, Rect::new(area.x, area.y, area.width, 3));
}

/// Pestanas a la izquierda; dispositivo y estado del daemon a la derecha.
fn header(f: &mut Frame, app: &App, area: Rect) {
  let t = &app.theme;
  let buf = f.buffer_mut();
  let mut x = area.x + 2;
  for tab in Tab::ALL {
    let style = if tab == app.tab { t.title().add_modifier(Modifier::UNDERLINED) } else { t.faint() };
    buf.set_string(x, area.y + 1, tab.label(), style);
    x += tab.label().chars().count() as u16 + 4;
  }
  let status = if app.connected {
    Line::from(vec![Span::styled("● ", Style::new().fg(t.ok)), Span::styled("ketaid", Style::new().fg(t.ok))])
  } else {
    Line::from(vec![Span::styled("● ", Style::new().fg(t.error)), Span::styled("ketaid sin conexión", Style::new().fg(t.error))])
  };
  let w = status.width() as u16;
  if area.width > w + 2 {
    buf.set_line(area.x + area.width - w - 2, area.y + 1, &status, w);
  }
}
