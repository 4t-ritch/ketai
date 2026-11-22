//! Dibujo. Cada vista es una funcion pura del estado de la app.

use ratatui::{
  Frame,
  layout::Rect,
  style::{Modifier, Style},
  text::{Line, Span},
};

use crate::{
  app::{App, Screen, Tab, fmt_ms},
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
  let body = Rect::new(area.x, area.y + 3, area.width, area.height.saturating_sub(3));
  if app.tab == Tab::NowPlaying {
    now_playing(f, app, body);
  }
}

// Iconos de Nerd Font (rango FontAwesome, presente en todas).
pub const I_PLAY: &str = "\u{f04b}";
pub const I_PAUSE: &str = "\u{f04c}";
pub const I_NEXT: &str = "\u{f051}";
pub const I_PREV: &str = "\u{f048}";
pub const I_SHUFFLE: &str = "\u{f074}";
pub const I_REPEAT: &str = "\u{f01e}";
pub const I_VOLUME: &str = "\u{f028}";
pub const I_MUTE: &str = "\u{f026}";
pub const I_MUSIC: &str = "\u{f001}";

/// `44.1 kHz`, `48 kHz`.
fn fmt_rate(hz: u64) -> String {
  let k = hz as f64 / 1000.0;
  if k.fract() == 0.0 { format!("{k:.0} kHz") } else { format!("{k:.1} kHz") }
}

/// Barra de progreso fina: hecho en blanco, resto en tono de fondo.
fn progress(buf: &mut ratatui::buffer::Buffer, t: &Theme, x: u16, y: u16, w: u16, pos: u64, dur: Option<u64>) {
  let left = fmt_ms(pos);
  let right = dur.map(fmt_ms).unwrap_or_else(|| "--:--".into());
  let bar_w = w.saturating_sub(left.len() as u16 + right.len() as u16 + 4);
  let done = match dur {
    Some(d) if d > 0 => (bar_w as u64 * pos.min(d) / d) as u16,
    _ => 0,
  };
  buf.set_string(x, y, &left, t.dim());
  let bx = x + left.len() as u16 + 2;
  buf.set_string(bx, y, "━".repeat(done as usize), t.text());
  buf.set_string(bx + done, y, "●", t.text());
  buf.set_string(bx + done + 1, y, "━".repeat(bar_w.saturating_sub(done + 1) as usize), Style::new().fg(t.raised));
  buf.set_string(bx + bar_w + 2, y, &right, t.dim());
}

/// Espectro de `viz.sock` con bloques de octavos; degradado del acento al
/// rosa hacia arriba.
fn spectrum(buf: &mut ratatui::buffer::Buffer, t: &Theme, area: Rect, values: &[f32]) {
  const EIGHTHS: [&str; 9] = [" ", "▁", "▂", "▃", "▄", "▅", "▆", "▇", "█"];
  if values.is_empty() || area.width == 0 || area.height == 0 {
    return;
  }
  // Una banda por cada 2 columnas (barra + hueco); si no entran, se
  // agrupan.
  let bars = (area.width as usize / 2).min(values.len()).max(1);
  let per = values.len() as f32 / bars as f32;
  let x0 = area.x + (area.width - bars as u16 * 2) / 2;
  let top = crate::theme::rgb(0xff7ab6);
  for b in 0..bars {
    let from = (b as f32 * per) as usize;
    let to = (((b + 1) as f32 * per) as usize).max(from + 1).min(values.len());
    let v = values[from..to].iter().copied().fold(0.0f32, f32::max).clamp(0.0, 1.0);
    let units = (v * area.height as f32 * 8.0) as u16;
    for row in 0..area.height {
      let from_bottom = area.height - 1 - row;
      let n = units.saturating_sub(from_bottom * 8).min(8);
      if n == 0 {
        continue;
      }
      let color = crate::theme::lerp(t.accent, top, from_bottom as f32 / area.height as f32);
      buf.set_string(x0 + b as u16 * 2, area.y + row, EIGHTHS[n as usize], Style::new().fg(color));
    }
  }
}

/// Reproduciendo (maqueta 1-inicio-reproduciendo).
fn now_playing(f: &mut Frame, app: &App, area: Rect) {
  let t = &app.theme;
  let track = &app.player["track"];
  if !track.is_object() {
    let y = area.y + area.height / 3;
    center(f, y, Line::styled(format!("{I_MUSIC}  Nada en reproducción"), t.title()));
    center(f, y + 2, Line::styled("Elegí algo en Biblioteca (3) o Buscar (4)", t.faint()));
    return;
  }
  let viz_h = (area.height / 4).clamp(3, 12);
  let top_h = area.height.saturating_sub(viz_h + 2);
  // La caratula ocupa un cuadrado: las celdas miden ~1:2.2.
  let cover_h = top_h.saturating_sub(2).min(24);
  let cover = Rect::new(area.x + 6, area.y + 1, (cover_h as u32 * 2) as u16, cover_h);
  draw_cover_slot(f, t, cover);
  let buf = f.buffer_mut();
  let x = cover.x + cover.width + 6;
  let w = (area.x + area.width).saturating_sub(x + 6);
  let s = |k: &str| track[k].as_str().unwrap_or("").to_string();
  let title = if s("title").is_empty() { s("uri").rsplit('/').next().unwrap_or_default().to_string() } else { s("title") };
  let mut y = area.y + 3;
  buf.set_string(x, y, "REPRODUCIENDO", t.faint());
  y += 2;
  buf.set_stringn(x, y, &title, w as usize, t.title());
  buf.set_string(x, y + 1, "▔".repeat(title.chars().count().min(w as usize)), t.accent());
  y += 3;
  buf.set_stringn(x, y, s("artist"), w as usize, t.text());
  buf.set_stringn(x, y + 1, s("album"), w as usize, t.faint());
  y += 4;
  progress(buf, t, x, y, w, app.position_ms(), track["duration_ms"].as_u64());
  y += 2;
  let on = |b: bool| if b { t.accent() } else { t.faint() };
  let shuffle = app.player["shuffle"].as_bool().unwrap_or(false);
  let repeat = app.player["repeat"].as_str().unwrap_or("off");
  let play = if app.playing() { I_PAUSE } else { I_PLAY };
  let controls = Line::from(vec![
    Span::styled(format!("{I_SHUFFLE}      "), on(shuffle)),
    Span::styled(format!("{I_PREV}      "), t.text()),
    Span::styled(format!("  {play}  "), Style::new().fg(t.on_accent).bg(t.fg)),
    Span::styled(format!("      {I_NEXT}      "), t.text()),
    Span::styled(format!("{I_REPEAT}{}", if repeat == "track" { "1" } else { "" }), on(repeat != "off")),
  ]);
  let cw = controls.width() as u16;
  buf.set_line(x + w.saturating_sub(cw) / 2, y, &controls, w);
  y += 4;
  if let Some(next) = app.next_track() {
    let line = Line::from(vec![
      Span::styled("Siguiente  ", t.faint()),
      Span::styled(next["title"].as_str().unwrap_or("").to_string(), t.text()),
      Span::styled(format!(" · {}", next["artist"].as_str().unwrap_or("")), t.faint()),
    ]);
    buf.set_line(x, y, &line, w);
  }
  y += 1;
  let muted = app.player["muted"].as_bool().unwrap_or(false);
  let vol = app.player["volume"].as_f64().unwrap_or(1.0);
  let filled = (vol * 18.0).round() as usize;
  let mut details = vec![
    Span::styled(format!("{} ", if muted { I_MUTE } else { I_VOLUME }), t.faint()),
    Span::styled("━".repeat(filled), t.dim()),
    Span::styled("●", t.dim()),
    Span::styled("━".repeat(18 - filled.min(18)), Style::new().fg(t.raised)),
    Span::styled(format!("  {}%", (vol * 100.0).round()), t.faint()),
  ];
  let codec = s("codec");
  if !codec.is_empty() {
    let rate = track["sample_rate"].as_u64().map(|r| format!(" · {}", fmt_rate(r))).unwrap_or_default();
    details.push(Span::styled(format!("   {codec}{rate}"), t.faint()));
  }
  buf.set_line(x, y, &Line::from(details), w);

  if app.playing() {
    let viz = Rect::new(area.x + 2, area.y + area.height - viz_h - 1, area.width.saturating_sub(4), viz_h);
    spectrum(buf, t, viz, &app.viz);
  }
}

/// Lugar de la caratula. La imagen real la pone la capa de graficos
/// (protocolo kitty); aca solo se reserva el espacio.
fn draw_cover_slot(f: &mut Frame, t: &Theme, area: Rect) {
  f.buffer_mut().set_style(area, Style::new().bg(t.raised));
  let y = area.y + area.height / 2;
  let x = area.x + area.width / 2;
  f.buffer_mut().set_string(x, y, I_MUSIC, t.faint());
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
