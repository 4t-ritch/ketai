//! Dibujo. Cada vista es una funcion pura del estado de la app.

use ratatui::{
  Frame,
  layout::Rect,
  style::{Modifier, Style},
  text::{Line, Span},
};

use crate::{
  app::{App, Screen, Tab},
  service::State,
  theme::Theme,
};

pub fn draw(f: &mut Frame, app: &App) {
  let t = &app.theme;
  let area = f.area();
  f.buffer_mut().set_style(area, Style::new().bg(t.bg));
  if let Screen::Service { state, observed, message, busy } = &app.screen {
    let binary = observed.binary.as_ref().map(|b| b.display().to_string());
    service(f, t, *state, binary.as_deref(), message.as_deref(), *busy, app.confirm_restart);
    return;
  }
  header(f, app, Rect::new(area.x, area.y, area.width, 3));
}

/// Texto centrado en una fila.
fn center(f: &mut Frame, y: u16, line: Line) {
  let area = f.area();
  let w = line.width() as u16;
  if y < area.height {
    f.buffer_mut().set_line(area.x + area.width.saturating_sub(w) / 2, y, &line, area.width);
  }
}

/// Pantalla cuando no hay daemon (maqueta 8-servicio).
fn service(f: &mut Frame, t: &Theme, state: State, binary: Option<&str>, message: Option<&str>, busy: bool, confirm: bool) {
  let top = f.area().height.saturating_sub(16) / 2;
  for (i, row) in ["█ █ █▀▀ ▀█▀ ▄▀█ █", "█▀▄ ██▄  █  █▀█ █"].iter().enumerate() {
    center(f, top + i as u16, Line::styled(*row, t.accent().add_modifier(Modifier::BOLD)));
  }
  let bin = binary.map(|b| format!(" · binario en {b}")).unwrap_or_else(|| " · binario ketaid no encontrado".into());
  let (title, sub, buttons, hints): (&str, String, Vec<&str>, &str) = match state {
    State::Connected => ("Conectado a ketaid", String::new(), vec![], ""),
    State::Starting => ("Iniciando ketaid…", "La unidad está activa; esperando que responda".into(), vec![], "reintentando automáticamente · q salir"),
    State::Stopped { enabled } => (
      "El servicio ketaid no está corriendo",
      format!("Unidad ketaid.socket instalada pero inactiva{}{bin}", if enabled { " (habilitada)" } else { "" }),
      vec!["Iniciar servicio", "Iniciar siempre con la sesión"],
      "⏎ iniciar · e habilitar · r reintentar · q salir",
    ),
    State::NotInstalled => ("El servicio ketaid no está instalado", format!("No hay unidades de systemd para ketaid{bin}"), vec!["Instalar e iniciar"], "⏎ instalar e iniciar · r reintentar · q salir"),
    State::Unresponsive { pid } => ("ketaid no responde", format!("El proceso {pid} no contesta por el socket de control"), vec!["Reiniciar"], "⏎ reiniciar · r reintentar · q salir"),
    State::NoSystemd => ("El servicio ketaid no está corriendo", format!("No hay systemd de usuario: se inicia en modo manual{bin}"), vec!["Iniciar servicio"], "⏎ iniciar · r reintentar · q salir"),
  };
  center(f, top + 5, Line::styled(title, t.title()));
  center(f, top + 7, Line::styled(sub, t.faint()));
  let mut spans = Vec::new();
  for (i, b) in buttons.iter().enumerate() {
    let style = if i == 0 { Style::new().fg(t.on_accent).bg(t.accent).add_modifier(Modifier::BOLD) } else { Style::new().fg(t.fg).bg(t.raised) };
    spans.push(Span::styled(format!("   {b}   "), style));
    spans.push(Span::raw("   "));
  }
  center(f, top + 10, Line::from(spans));
  let note = if busy {
    Line::styled("trabajando…", t.dim())
  } else if confirm {
    if let State::Unresponsive { pid } = state {
      Line::styled(format!("¿Reiniciar el proceso {pid}?  y confirmar · esc cancelar"), Style::new().fg(t.error))
    } else {
      Line::raw("")
    }
  } else if let Some(m) = message {
    Line::styled(m.to_string(), Style::new().fg(t.error))
  } else {
    Line::raw("")
  };
  center(f, top + 12, note);
  center(f, top + 14, Line::styled(hints, t.faint()));
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
