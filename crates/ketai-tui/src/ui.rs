//! Dibujo. Cada vista es una funcion pura del estado de la app.

use ratatui::{
  Frame as RFrame,
  buffer::Buffer,
  layout::Rect,
  style::{Modifier, Style},
  text::{Line, Span},
};

use std::cell::RefCell;

use crate::{
  kitty,
  app::{App, Screen, Tab, fmt_ms},
  service::State,
  theme::Theme,
};

pub fn draw(f: &mut RFrame, app: &App) {
  SLOTS.with(|s| s.borrow_mut().clear());
  WANTED.with(|w| w.borrow_mut().clear());
  let t = &app.theme;
  let area = f.area();
  f.buffer_mut().set_style(area, Style::new().bg(t.bg));
  if let Screen::Service { state, observed, message, busy } = &app.screen {
    let binary = observed.binary.as_ref().map(|b| b.display().to_string());
    service(f, t, *state, binary.as_deref(), message.as_deref(), *busy, app.confirm_restart);
    return;
  }
  if is_mini(area) {
    mini(f, app, area);
    return;
  }
  header(f, app, Rect::new(area.x, area.y, area.width, 3));
  let body = Rect::new(area.x, area.y + 3, area.width, area.height.saturating_sub(3));
  match app.tab {
    Tab::NowPlaying => now_playing(f, app, body),
    Tab::Lyrics => lyrics(f, app, body),
    Tab::Library => library(f, app, body),
    Tab::Search => search(f, app, body),
  }
  if app.palette_open {
    dim(f, t);
    begin_overlay();
    palette(f, app);
  }
  if app.devices_open {
    dim(f, t);
    begin_overlay();
    devices(f, app);
  }
  if app.folders_open {
    dim(f, t);
    begin_overlay();
    folders(f, app);
  }
  config_notice(f, app);
}

/// Error o aviso de config.toml en la ultima fila. Un error deja el tema
/// anterior: se muestra hasta que el archivo se corrige.
fn config_notice(f: &mut RFrame, app: &App) {
  let t = &app.theme;
  let area = f.area();
  let (text, color) = match (&app.config_error, app.config.warnings.first()) {
    (Some(e), _) => (format!("config.toml: {e}"), t.error),
    (None, Some(w)) => (format!("config.toml: {w}"), t.faint),
    _ => return,
  };
  if area.height == 0 {
    return;
  }
  let y = area.y + area.height - 1;
  f.buffer_mut().set_style(Rect::new(area.x, y, area.width, 1), Style::new().bg(t.bg));
  putn(f.buffer_mut(), area.x + 1, y, text, area.width.saturating_sub(2) as usize, Style::new().fg(color));
}

/// Atenua todo lo dibujado (fondo de un popup modal).
fn dim(f: &mut RFrame, t: &Theme) {
  let black = crate::theme::rgb(0x000000);
  for cell in f.buffer_mut().content.iter_mut() {
    cell.fg = crate::theme::lerp(cell.fg, black, 0.6);
    // Las celdas con fondo por defecto son caratulas: se atenua la imagen
    // (se usa su version oscura), no la celda.
    if cell.bg != ratatui::style::Color::Reset {
      cell.bg = crate::theme::lerp(cell.bg, black, 0.6);
    }
  }
  let _ = t;
}

/// Fondo de los popups.
const POPUP: ratatui::style::Color = crate::theme::rgb(0x0b0f16);

/// Recuadro redondeado con el borde en el color de acento.
fn popup_frame(f: &mut RFrame, t: &Theme, area: Rect) {
  use ratatui::widgets::{Block, BorderType, Clear, Widget};
  Clear.render(area, f.buffer_mut());
  Block::bordered().border_type(BorderType::Rounded).border_style(t.accent()).style(Style::new().bg(POPUP)).render(area, f.buffer_mut());
}

/// Tipo de salida a partir del nombre del sink.
fn device_kind(id: &str) -> &'static str {
  if id.starts_with("bluez") {
    "bluetooth"
  } else if id.contains("hdmi") {
    "HDMI"
  } else {
    "audio"
  }
}

/// Selector de salida (maqueta 6-dispositivos).
fn devices(f: &mut RFrame, app: &App) {
  let t = &app.theme;
  let area = f.area();
  let rows = app.device_rows();
  let h = (rows.len() as u16 * 3 + 10).min(area.height.saturating_sub(4));
  let w = 64.min(area.width.saturating_sub(4));
  let pop = Rect::new(area.x + (area.width - w) / 2, area.y + (area.height - h) / 3, w, h);
  popup_frame(f, t, pop);
  let buf = f.buffer_mut();
  let x = pop.x + 3;
  put(buf, x, pop.y + 1, "Conectar a un dispositivo", t.title());
  put(buf, pop.x + pop.width.saturating_sub(9), pop.y + 1, "esc ✕", t.faint());
  let default_name = app.outputs.iter().find(|d| d["is_default"] == true).and_then(|d| d["name"].as_str()).unwrap_or("ninguno");
  let mut y = pop.y + 3;
  for (i, (id, name)) in rows.iter().enumerate() {
    if i == 0 {
      put(buf, x, y, "DISPOSITIVO ACTUAL", t.faint());
      y += 1;
    } else if i == 1 {
      y += 1;
      put(buf, x, y, "OTROS DISPOSITIVOS", t.faint());
      y += 1;
    }
    if y + 2 > pop.y + pop.height - 1 {
      break;
    }
    let sel = i == app.devices_cursor;
    if sel {
      buf.set_style(Rect::new(pop.x + 1, y, pop.width.saturating_sub(2), 2), Style::new().bg(t.row));
      put(buf, pop.x + 1, y, "▌", t.accent());
      put(buf, pop.x + 1, y + 1, "▌", t.accent());
    }
    let (icon, detail) = match id {
      Some(id) => (if id.starts_with("bluez") { I_HEADPHONES } else { I_DEVICE }, format!("{} · {id}", device_kind(id))),
      None => (I_DEVICE, format!("sigue al default: {default_name}")),
    };
    let style = if i == 0 { t.accent().add_modifier(Modifier::BOLD) } else { t.text() };
    putn(buf, x, y, format!("{icon}   {name}"), pop.width.saturating_sub(6) as usize, style);
    putn(buf, x + 5, y + 1, detail, pop.width.saturating_sub(11) as usize, t.faint());
    y += 3;
  }
  if app.outputs.is_empty() {
    put(buf, x, y, if app.outputs_loaded { "No hay servidor de sonido disponible" } else { "Cargando…" }, t.faint());
  }
  put(buf, x, pop.y + pop.height - 2, "⏎ conectar · ↑↓ elegir · esc cerrar", t.faint());
}

/// Carpetas de la biblioteca: agregar, quitar y escanear.
fn folders(f: &mut RFrame, app: &App) {
  let t = &app.theme;
  let area = f.area();
  let w = 84.min(area.width.saturating_sub(4));
  // En terminales bajas el maximo puede quedar por debajo de 16: manda el
  // maximo (clamp fallaria con min > max).
  let h = (app.folders.len() as u16 * 3 + 14).max(16).min(area.height.saturating_sub(4).max(1));
  let pop = Rect::new(area.x + (area.width.saturating_sub(w)) / 2, area.y + (area.height.saturating_sub(h)) / 3, w, h);
  popup_frame(f, t, pop);
  let buf = f.buffer_mut();
  let x = pop.x + 3;
  let inner = pop.width.saturating_sub(6);
  put(buf, x, pop.y + 1, "Carpetas de la biblioteca", t.title());
  put(buf, pop.x + pop.width.saturating_sub(9), pop.y + 1, "esc ✕", t.faint());
  let mut y = pop.y + 3;
  if app.folders.is_empty() {
    put(buf, x, y, "Todavía no hay carpetas. Agregá una con a.", t.dim());
    y += 2;
  }
  for (i, folder) in app.folders.iter().enumerate() {
    if y + 2 > pop.y + pop.height.saturating_sub(6) {
      break;
    }
    let sel = i == app.folders_cursor;
    if sel {
      buf.set_style(Rect::new(pop.x + 1, y, pop.width.saturating_sub(2), 2).intersection(buf.area), Style::new().bg(t.row));
      put(buf, pop.x + 1, y, "▌", t.accent());
      put(buf, pop.x + 1, y + 1, "▌", t.accent());
    }
    let path = folder["path"].as_str().unwrap_or("");
    putn(buf, x, y, format!("{I_FOLDER}  {path}"), inner as usize, if sel { t.accent().add_modifier(Modifier::BOLD) } else { t.text() });
    putn(buf, x + 5, y + 1, format!("{} canciones", folder["tracks"].as_u64().unwrap_or(0)), inner as usize, t.faint());
    y += 3;
  }
  // Agregar, confirmacion, progreso y errores, abajo.
  let bottom = pop.y + pop.height.saturating_sub(5);
  if let Some(input) = &app.folder_input {
    let line = Line::from(vec![Span::styled("Agregar: ", t.faint()), Span::styled(input.clone(), t.title()), Span::styled("█", t.text())]);
    put_line(buf, x, bottom, &line, inner);
  } else if app.folder_confirm_remove {
    let path = app.folders.get(app.folders_cursor).and_then(|f| f["path"].as_str()).unwrap_or("");
    putn(buf, x, bottom, format!("¿Quitar {path} y sus canciones del índice?  y confirmar · esc cancelar"), inner as usize, Style::new().fg(t.error));
  } else if let Some((_, done, total)) = app.scan {
    let label = format!("Escaneando… {done} / {total}");
    put(buf, x, bottom, &label, t.dim());
    let bar_w = inner.saturating_sub(label.chars().count() as u16 + 2);
    let filled = (bar_w as u64 * done.min(total)).checked_div(total).unwrap_or(0) as u16;
    let bx = x + label.chars().count() as u16 + 2;
    put(buf, bx, bottom, "━".repeat(filled as usize), t.accent());
    put(buf, bx + filled, bottom, "━".repeat(bar_w.saturating_sub(filled) as usize), Style::new().fg(t.raised));
  } else if let Some(err) = &app.folders_error {
    putn(buf, x, bottom, err, inner as usize, Style::new().fg(t.error));
  }
  let hints = if app.folder_input.is_some() { "⏎ agregar · esc cancelar · ~ es tu carpeta personal" } else { "a agregar · x quitar · s escanear esta · S escanear todas · esc cerrar" };
  putn(buf, x, pop.y + pop.height.saturating_sub(2), hints, inner as usize, t.faint());
}

/// Busqueda rapida (maqueta 3-busqueda-rapida).
fn palette(f: &mut RFrame, app: &App) {
  use crate::app::PaletteItem;
  let t = &app.theme;
  let area = f.area();
  let w = 90.min(area.width.saturating_sub(4));
  let h = 30.min(area.height.saturating_sub(4));
  let pop = Rect::new(area.x + (area.width - w) / 2, area.y + 4.min(area.height / 8), w, h);
  popup_frame(f, t, pop);
  let x = pop.x + 3;
  let inner_w = pop.width.saturating_sub(6);
  let mut slots = Vec::new();
  {
    let buf = f.buffer_mut();
    let field = Line::from(vec![Span::styled(format!("{I_SEARCH}  "), t.accent()), Span::styled(app.palette_query.clone(), t.title()), Span::styled("█", t.text())]);
    put_line(buf, x, pop.y + 1, &field, inner_w);
    put(buf, pop.x + 1, pop.y + 2, "─".repeat(pop.width.saturating_sub(2) as usize), Style::new().fg(t.raised));
    let mut y = pop.y + 3;
    let mut section = "";
    for (i, item) in app.palette_items().iter().enumerate() {
      let title = match item {
        PaletteItem::Song(_) => "CANCIONES",
        PaletteItem::Playlist(_) => "PLAYLISTS",
        PaletteItem::Action(_) => "ACCIONES",
      };
      if title != section {
        if !section.is_empty() {
          y += 1;
        }
        section = title;
        put(buf, x, y, title, t.faint());
        y += 1;
      }
      let rows = if matches!(item, PaletteItem::Song(_)) { 2 } else { 1 };
      if y + rows > pop.y + pop.height - 1 {
        break;
      }
      let sel = i == app.palette_cursor;
      if sel {
        buf.set_style(Rect::new(pop.x + 1, y, pop.width.saturating_sub(2), rows), Style::new().bg(t.row));
      }
      match item {
        PaletteItem::Song(tr) => {
          slots.push((Rect::new(x, y, 4, 2), tr["uri"].as_str().map(str::to_string)));
          putn(buf, x + 6, y, track_title(tr), inner_w.saturating_sub(30) as usize, if sel { t.accent().add_modifier(Modifier::BOLD) } else { t.text() });
          putn(buf, x + 6, y + 1, tr["artist"].as_str().unwrap_or(""), inner_w.saturating_sub(30) as usize, t.faint());
          if sel {
            put(buf, pop.x + pop.width.saturating_sub(24), y, "⏎ reproducir  ⇥ cola", t.faint());
          }
        }
        PaletteItem::Playlist(name) => {
          put(buf, x, y, format!("{I_LIBRARY}  {name}"), if sel { t.accent() } else { t.text() });
        }
        PaletteItem::Action(a) => {
          put(buf, x, y, a.label(), if sel { t.accent() } else { t.dim() });
        }
      }
      y += rows;
    }
    put(buf, x, pop.y + pop.height - 2, "↑↓ elegir · ⏎ ejecutar · esc cerrar", t.faint());
  }
  for (s, uri) in slots {
    draw_cover(f, app, s, uri.as_deref());
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
fn progress(buf: &mut Buffer, t: &Theme, x: u16, y: u16, w: u16, pos: u64, dur: Option<u64>) {
  let left = fmt_ms(pos);
  let right = dur.map(fmt_ms).unwrap_or_else(|| "--:--".into());
  let bar_w = w.saturating_sub(left.len() as u16 + right.len() as u16 + 4);
  let done = match dur {
    Some(d) if d > 0 => (bar_w as u64 * pos.min(d) / d) as u16,
    _ => 0,
  };
  put(buf, x, y, &left, t.dim());
  let bx = x + left.len() as u16 + 2;
  put(buf, bx, y, "━".repeat(done as usize), t.text());
  put(buf, bx + done, y, "●", t.text());
  put(buf, bx + done + 1, y, "━".repeat(bar_w.saturating_sub(done + 1) as usize), Style::new().fg(t.raised));
  put(buf, bx + bar_w + 2, y, &right, t.dim());
}

/// Espectro de `viz.sock` con bloques de octavos; degradado del acento al
/// rosa hacia arriba.
fn spectrum(buf: &mut Buffer, t: &Theme, area: Rect, values: &[f32]) {
  const EIGHTHS: [&str; 9] = [" ", "▁", "▂", "▃", "▄", "▅", "▆", "▇", "█"];
  if values.is_empty() || area.width == 0 || area.height == 0 {
    return;
  }
  // Una banda por cada 2 columnas (barra + hueco); si no entran, se
  // agrupan.
  let bars = (area.width as usize / 2).min(values.len()).max(1);
  let per = values.len() as f32 / bars as f32;
  let x0 = area.x + (area.width - bars as u16 * 2) / 2;
  let top = t.spectrum_top;
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
      put(buf, x0 + b as u16 * 2, area.y + row, EIGHTHS[n as usize], Style::new().fg(color));
    }
  }
}

/// Reproduciendo (maqueta 1-inicio-reproduciendo).
fn now_playing(f: &mut RFrame, app: &App, area: Rect) {
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
  draw_cover(f, app, cover, app.player["track"]["uri"].as_str());
  let buf = f.buffer_mut();
  let x = cover.x + cover.width + 6;
  let w = (area.x + area.width).saturating_sub(x + 6);
  let s = |k: &str| track[k].as_str().unwrap_or("").to_string();
  let title = track_title(track);
  let mut y = area.y + 3;
  put(buf, x, y, "REPRODUCIENDO", t.faint());
  y += 2;
  putn(buf, x, y, &title, w as usize, t.title());
  put(buf, x, y + 1, "▔".repeat(title.chars().count().min(w as usize)), t.accent());
  y += 3;
  putn(buf, x, y, s("artist"), w as usize, t.text());
  putn(buf, x, y + 1, s("album"), w as usize, t.faint());
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
  put_line(buf, x + w.saturating_sub(cw) / 2, y, &controls, w);
  y += 3;
  lyrics_preview(buf, app, x, y, w);
  y += 5;
  if let Some(next) = app.next_track() {
    let line = Line::from(vec![
      Span::styled("Siguiente  ", t.faint()),
      Span::styled(next["title"].as_str().unwrap_or("").to_string(), t.text()),
      Span::styled(format!(" · {}", next["artist"].as_str().unwrap_or("")), t.faint()),
    ]);
    put_line(buf, x, y, &line, w);
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
  put_line(buf, x, y, &Line::from(details), w);

  if app.playing() {
    let viz = Rect::new(area.x + 2, area.y + area.height - viz_h - 1, area.width.saturating_sub(4), viz_h);
    spectrum(buf, t, viz, &app.viz);
  }
}

/// Titulo de la pista, o el nombre del archivo si no tiene etiqueta.
pub fn track_title(track: &serde_json::Value) -> String {
  match track["title"].as_str().filter(|t| !t.is_empty()) {
    Some(t) => t.to_string(),
    None => {
      let uri = track["uri"].as_str().unwrap_or("");
      let name = uri.rsplit('/').next().unwrap_or(uri);
      name.replace("%20", " ")
    }
  }
}

/// Color de las lineas que todavia no se cantan.
const UPCOMING: ratatui::style::Color = crate::theme::rgb(0x6b7a90);
const UPCOMING_SECONDARY: ratatui::style::Color = crate::theme::rgb(0x4f5d72);

/// Tres lineas alrededor de la actual (en Reproduciendo).
fn lyrics_preview(buf: &mut Buffer, app: &App, x: u16, y: u16, w: u16) {
  let t = &app.theme;
  let (Some(lines), Some(cur)) = (app.lyrics["lines"].as_array(), app.lyrics_line()) else { return };
  let text = |i: usize| lines.get(i).and_then(|l| l["text"].as_str()).unwrap_or("").to_string();
  if cur > 0 {
    putn(buf, x + 2, y, text(cur - 1), w as usize, t.faint());
  }
  putn(buf, x, y + 1, format!("▶ {}", text(cur)), w as usize, t.title());
  putn(buf, x + 2, y + 2, text(cur + 1), w as usize, t.dim());
}

/// Linea actual con la palabra que suena resaltada.
fn sung_line(app: &App, line: &serde_json::Value, pos: u64) -> Line<'static> {
  let t = &app.theme;
  let bold = Modifier::BOLD;
  let Some(words) = line["words"].as_array().filter(|w| !w.is_empty()) else {
    return Line::styled(line["text"].as_str().unwrap_or("").to_string(), t.title());
  };
  Line::from(
    words
      .iter()
      .map(|w| {
        let text = w["text"].as_str().unwrap_or("").to_string();
        let start = w["start_ms"].as_u64().unwrap_or(0);
        let end = w["end_ms"].as_u64().unwrap_or(u64::MAX);
        let style = if pos >= end {
          t.title()
        } else if pos >= start {
          Style::new().fg(t.accent).add_modifier(bold | Modifier::UNDERLINED)
        } else {
          Style::new().fg(UPCOMING).add_modifier(bold)
        };
        Span::styled(text, style)
      })
      .collect::<Vec<_>>(),
  )
}

fn layer_label(app: &App, id: &str) -> String {
  let layer = app.lyrics_layers.iter().find(|l| l["id"] == id);
  let lang = layer.and_then(|l| l["language"].as_str()).map(|l| format!(" ({l})")).unwrap_or_default();
  match layer.and_then(|l| l["kind"].as_str()) {
    Some("translation") => format!("Traducción{lang}"),
    Some("romanized") => format!("Romanizada{lang}"),
    _ => "Original".into(),
  }
}

/// Letras (maqueta 2-letras).
fn lyrics(f: &mut RFrame, app: &App, area: Rect) {
  let t = &app.theme;
  let track = &app.player["track"];
  if !track.is_object() {
    center(f, area.y + area.height / 3, Line::styled("Nada en reproducción", t.title()));
    return;
  }
  draw_cover(f, app, Rect::new(area.x + 4, area.y + 1, 8, 4), app.player["track"]["uri"].as_str());
  let buf = f.buffer_mut();
  put(buf, area.x + 14, area.y + 2, track_title(track), t.title());
  put(buf, area.x + 14, area.y + 3, track["artist"].as_str().unwrap_or(""), t.dim());

  // Capas elegidas y offset, arriba a la derecha.
  let mut chips = vec![Span::styled(layer_label(app, &app.lyrics_primary), t.faint())];
  for id in &app.lyrics_secondary {
    chips.push(Span::styled("  ·  ", t.faint()));
    chips.push(Span::styled(format!("+ {}", layer_label(app, id)), t.accent().add_modifier(Modifier::BOLD)));
  }
  let chips = Line::from(chips);
  let right = area.x + area.width.saturating_sub(chips.width() as u16 + 4);
  put_line(buf, right, area.y + 2, &chips, area.width);
  let offset = app.lyrics["user_offset_ms"].as_i64().unwrap_or(0);
  let info = format!("offset {offset} ms   [ ] ajustar   t traducción   o capa");
  put(buf, area.x + area.width.saturating_sub(info.chars().count() as u16 + 4), area.y + 3, info, t.faint());

  let bottom = 8;
  let lines_area = Rect::new(area.x, area.y + 6, area.width, area.height.saturating_sub(6 + bottom));
  let lines = app.lyrics["lines"].as_array();
  match lines {
    None if !app.lyrics_loaded => center(f, lines_area.y + lines_area.height / 2, Line::styled("Cargando letra…", t.faint())),
    None => {
      center(f, lines_area.y + lines_area.height / 2 - 1, Line::styled("Esta canción no tiene letra", t.title()));
      center(f, lines_area.y + lines_area.height / 2 + 1, Line::styled("Agregá un .lrc, .ttml o .txt con el mismo nombre que el audio", t.faint()));
    }
    Some(lines) => {
      let pos = app.position_ms();
      let cur = app.lyrics_line();
      let rows = (lines_area.height / 4).max(1) as usize;
      // La linea actual queda en el segundo tercio de la vista.
      let first = cur.unwrap_or(0).saturating_sub(rows / 3);
      for (slot, i) in (first..lines.len()).take(rows).enumerate() {
        let line = &lines[i];
        let y = lines_area.y + slot as u16 * 4;
        let main = match cur {
          Some(c) if i == c => sung_line(app, line, pos),
          Some(c) if i < c => Line::styled(line["text"].as_str().unwrap_or("").to_string(), t.faint()),
          _ => Line::styled(line["text"].as_str().unwrap_or("").to_string(), Style::new().fg(UPCOMING).add_modifier(Modifier::BOLD)),
        };
        center(f, y, main);
        for (k, id) in app.lyrics_secondary.iter().enumerate() {
          let Some(tr) = line["secondary"][id].as_str() else { continue };
          let color = if cur == Some(i) { t.dim } else { UPCOMING_SECONDARY };
          center(f, y + 1 + k as u16, Line::styled(tr.to_string(), Style::new().fg(color).add_modifier(Modifier::ITALIC)));
        }
      }
    }
  }

  // Espectro, progreso y controles abajo.
  let buf = f.buffer_mut();
  if app.playing() {
    spectrum(buf, t, Rect::new(area.x + 2, area.y + area.height - bottom, area.width.saturating_sub(4), 3), &app.viz);
  }
  let pw = area.width.saturating_sub(60).max(40).min(area.width);
  progress(buf, t, area.x + (area.width - pw) / 2, area.y + area.height - 4, pw, app.position_ms(), track["duration_ms"].as_u64());
  let play = if app.playing() { I_PAUSE } else { I_PLAY };
  let controls = Line::from(vec![
    Span::styled(format!("{I_PREV}      "), t.text()),
    Span::styled(format!("  {play}  "), Style::new().fg(t.on_accent).bg(t.fg)),
    Span::styled(format!("      {I_NEXT}"), t.text()),
  ]);
  center(f, area.y + area.height - 2, controls);
}

const I_LIBRARY: &str = "\u{f02d}";
const I_FOLDER: &str = "\u{f07b}";
const I_HEADPHONES: &str = "\u{f025}";
const I_DEVICE: &str = "\u{f108}";
const I_SEARCH: &str = "\u{f002}";
const I_CLOCK: &str = "\u{f017}";

/// Columna de playlists (izquierda en Biblioteca y Buscar).
fn playlists_column(buf: &mut Buffer, app: &App, area: Rect) -> Vec<(Rect, Option<String>)> {
  let t = &app.theme;
  let x = area.x + 2;
  put(buf, x, area.y, format!("{I_LIBRARY}  Tu biblioteca"), t.title());
  put(buf, x, area.y + 2, " Playlists ", Style::new().fg(t.on_accent).bg(t.accent));
  let mut slots = Vec::new();
  let focused = app.lib_focus == crate::app::LibFocus::Playlists && app.tab == Tab::Library;
  for (i, p) in app.playlists.iter().enumerate() {
    let y = area.y + 4 + i as u16 * 4;
    if y + 3 > area.y + area.height {
      break;
    }
    let sel = i == app.lib_cursor && app.tab == Tab::Library;
    if sel {
      buf.set_style(Rect::new(area.x, y, area.width, 3), Style::new().bg(t.row));
      if focused {
        for dy in 0..3 {
          put(buf, area.x, y + dy, "▌", t.accent());
        }
      }
    }
    slots.push((Rect::new(x, y, 6, 3), playlist_cover(app, p["name"].as_str().unwrap_or(""))));
    let name = p["name"].as_str().unwrap_or("");
    let w = area.width.saturating_sub(11) as usize;
    putn(buf, x + 8, y, name, w, if sel { t.accent().add_modifier(Modifier::BOLD) } else { t.text() });
    let count = p["count"].as_u64().unwrap_or(0);
    putn(buf, x + 8, y + 1, format!("{count} canciones"), w, t.faint());
  }
  if app.playlists.is_empty() {
    put(buf, x, area.y + 4, if app.playlists_loaded { "Sin playlists" } else { "Cargando…" }, t.faint());
  }
  slots
}

/// Columna derecha: lo que suena (como el panel de la opcion A).
fn now_column(f: &mut RFrame, app: &App, area: Rect) {
  let t = &app.theme;
  let track = &app.player["track"];
  let x = area.x + 1;
  let w = area.width.saturating_sub(2);
  put(f.buffer_mut(), x, area.y, "REPRODUCIENDO", t.faint());
  if !track.is_object() {
    put(f.buffer_mut(), x, area.y + 2, "Nada en reproducción", t.dim());
    return;
  }
  let side = w * 10 / 22 + 1;
  draw_cover(f, app, Rect::new(x, area.y + 2, w, side), app.player["track"]["uri"].as_str());
  let buf = f.buffer_mut();
  let y = area.y + 3 + side;
  putn(buf, x, y, track_title(track), w as usize, t.title());
  putn(buf, x, y + 1, track["artist"].as_str().unwrap_or(""), w as usize, t.dim());
  lyrics_preview(buf, app, x, y + 3, w);
  if let Some(next) = app.next_track() {
    put(buf, x, y + 8, "SIGUIENTE", t.faint());
    putn(buf, x, y + 10, track_title(next), w as usize, t.text());
    putn(buf, x, y + 11, next["artist"].as_str().unwrap_or(""), w as usize, t.faint());
  }
}

/// Barra de reproduccion de abajo, en las vistas de navegacion.
fn player_bar(f: &mut RFrame, app: &App, area: Rect) {
  let t = &app.theme;
  let track = &app.player["track"];
  if track.is_object() {
    draw_cover(f, app, Rect::new(area.x + 2, area.y, 6, 3), app.player["track"]["uri"].as_str());
  }
  let buf = f.buffer_mut();
  if track.is_object() {
    putn(buf, area.x + 10, area.y, track_title(track), 40, t.title());
    putn(buf, area.x + 10, area.y + 1, track["artist"].as_str().unwrap_or(""), 40, t.dim());
  }
  let play = if app.playing() { I_PAUSE } else { I_PLAY };
  let shuffle = app.player["shuffle"].as_bool().unwrap_or(false);
  let repeat = app.player["repeat"].as_str().unwrap_or("off") != "off";
  let controls = Line::from(vec![
    Span::styled(format!("{I_SHUFFLE}      "), if shuffle { t.accent() } else { t.faint() }),
    Span::styled(format!("{I_PREV}      "), t.text()),
    Span::styled(format!("  {play}  "), Style::new().fg(t.on_accent).bg(t.fg)),
    Span::styled(format!("      {I_NEXT}      "), t.text()),
    Span::styled(I_REPEAT, if repeat { t.accent() } else { t.faint() }),
  ]);
  let cx = area.x + area.width / 2;
  put_line(buf, cx.saturating_sub(controls.width() as u16 / 2), area.y, &controls, area.width);
  let pw = 70.min(area.width.saturating_sub(70));
  progress(buf, t, cx.saturating_sub(pw / 2), area.y + 2, pw, app.position_ms(), track["duration_ms"].as_u64());
}

fn fmt_total(ms: u64) -> String {
  let min = ms / 60_000;
  if min >= 60 { format!("{} h {} min", min / 60, min % 60) } else { format!("{min} min") }
}

/// Biblioteca: playlists, la playlist abierta y lo que suena (maqueta
/// 4-biblioteca-playlist).
fn library(f: &mut RFrame, app: &App, area: Rect) {
  let t = &app.theme;
  let body = Rect::new(area.x, area.y + 1, area.width, area.height.saturating_sub(6));
  let slots = playlists_column(f.buffer_mut(), app, Rect::new(body.x, body.y, 34, body.height));
  for (s, uri) in slots {
    draw_cover(f, app, s, uri.as_deref());
  }
  let right_w = 34;
  let center = Rect::new(body.x + 37, body.y, body.width.saturating_sub(37 + right_w + 3), body.height);
  now_column(f, app, Rect::new(body.x + body.width - right_w - 1, body.y, right_w, body.height));
  player_bar(f, app, Rect::new(area.x, area.y + area.height - 4, area.width, 3));

  let Some(name) = app.lib_open.clone() else { return };
  let tracks = app.playlist["tracks"].as_array().cloned().unwrap_or_default();
  draw_cover(f, app, Rect::new(center.x, center.y, 20, 9), tracks.first().and_then(|t| t["uri"].as_str()));
  let buf = f.buffer_mut();
  let x = center.x + 23;
  let w = center.width.saturating_sub(23);
  put(buf, x, center.y + 1, "PLAYLIST", t.faint());
  putn(buf, x, center.y + 3, &name, w as usize, t.title());
  put(buf, x, center.y + 4, "▔".repeat(name.chars().count().min(w as usize)), t.accent());
  let total: u64 = tracks.iter().filter_map(|t| t["duration_ms"].as_u64()).sum();
  put(buf, x, center.y + 6, format!("{} canciones · {}", tracks.len(), fmt_total(total)), t.dim());
  // La ruta la dice el daemon (la carpeta se configura en config.toml).
  putn(buf, x, center.y + 7, app.playlist["path"].as_str().unwrap_or(""), w as usize, t.faint());
  let y = center.y + 11;
  put(buf, center.x, y, format!("  {I_PLAY}  "), Style::new().fg(t.on_accent).bg(t.accent).add_modifier(Modifier::BOLD));
  put(buf, center.x + 9, y, "⏎ reproducir · a a la cola · esc volver", t.faint());

  let cols = [center.x + 1, center.x + 5, center.x + center.width * 58 / 100, center.x + center.width.saturating_sub(6)];
  let hy = y + 3;
  for (cx, h) in cols.iter().zip(["#", "TÍTULO", "ÁLBUM", I_CLOCK]) {
    put(buf, *cx, hy, h, t.faint());
  }
  let playing_uri = app.player["track"]["uri"].as_str();
  let rows = ((center.y + center.height).saturating_sub(hy + 2) / 3) as usize;
  let first = app.track_cursor.saturating_sub(rows.saturating_sub(1));
  let mut cover_slots = Vec::new();
  let tracks_focus = app.lib_focus == crate::app::LibFocus::Tracks;
  for (slot, i) in (first..tracks.len()).take(rows).enumerate() {
    let tr = &tracks[i];
    let ry = hy + 2 + slot as u16 * 3;
    let playing = playing_uri.is_some() && tr["uri"].as_str() == playing_uri;
    if tracks_focus && i == app.track_cursor {
      buf.set_style(Rect::new(center.x, ry, center.width, 2), Style::new().bg(t.row));
      put(buf, center.x, ry, "▌", t.accent());
      put(buf, center.x, ry + 1, "▌", t.accent());
    }
    put(buf, cols[0], ry, if playing { "▶".to_string() } else { (i + 1).to_string() }, if playing { t.accent() } else { t.faint() });
    cover_slots.push((Rect::new(cols[1], ry, 4, 2), tr["uri"].as_str().map(str::to_string)));
    let tw = cols[2].saturating_sub(cols[1] + 7) as usize;
    putn(buf, cols[1] + 6, ry, track_title(tr), tw, if playing { t.accent().add_modifier(Modifier::BOLD) } else { t.text() });
    putn(buf, cols[1] + 6, ry + 1, tr["artist"].as_str().unwrap_or(""), tw, t.faint());
    putn(buf, cols[2], ry, tr["album"].as_str().unwrap_or(""), cols[3].saturating_sub(cols[2] + 2) as usize, t.dim());
    put(buf, cols[3], ry, tr["duration_ms"].as_u64().map(fmt_ms).unwrap_or_default(), t.faint());
  }
  for (s, uri) in cover_slots {
    draw_cover(f, app, s, uri.as_deref());
  }
}

/// Ruta legible de una URI `file://`.
fn uri_path(uri: &str) -> String {
  uri.strip_prefix("file://").unwrap_or(uri).replace("%20", " ").replace("%25", "%")
}

/// Buscar (maqueta 5-buscar).
fn search(f: &mut RFrame, app: &App, area: Rect) {
  let t = &app.theme;
  let body = Rect::new(area.x, area.y + 1, area.width, area.height.saturating_sub(6));
  let slots = playlists_column(f.buffer_mut(), app, Rect::new(body.x, body.y, 34, body.height));
  for (s, uri) in slots {
    draw_cover(f, app, s, uri.as_deref());
  }
  player_bar(f, app, Rect::new(area.x, area.y + area.height - 4, area.width, 3));
  let c = Rect::new(body.x + 37, body.y, body.width.saturating_sub(39), body.height);
  let buf = f.buffer_mut();
  let mut x = c.x;
  for (i, name) in crate::app::SEARCH_FILTERS.iter().enumerate() {
    let style = if i == app.search_filter { Style::new().fg(t.on_accent).bg(t.accent) } else { t.faint() };
    put(buf, x, c.y, format!(" {name} "), style);
    x += name.chars().count() as u16 + 3;
  }
  let q = app.search_query.trim();
  let mid = c.y + c.height / 3;
  if q.is_empty() {
    put(buf, c.x, mid, "Escribí para buscar en tu biblioteca", t.title());
    put(buf, c.x, mid + 2, "Se busca en título, artista, álbum y nombre de archivo", t.faint());
    return;
  }
  if !app.search_done {
    put(buf, c.x, mid, "Buscando…", t.faint());
    return;
  }
  let tracks = app.search_tracks();
  let playlists = app.search_playlists();
  if tracks.is_empty() && playlists.is_empty() {
    put(buf, c.x, mid, format!("Sin resultados para «{q}»"), t.title());
    put(buf, c.x, mid + 2, "Si tu música no aparece, escaneá la carpeta desde la búsqueda rápida (/)", t.faint());
    return;
  }
  let mut slots = Vec::new();
  let mut y = c.y + 3;
  let files_only = app.search_filter == 3;
  if let (Some(best), false) = (tracks.first(), files_only) {
    // Mejor resultado a la izquierda, canciones a la derecha.
    put(buf, c.x, y, "MEJOR RESULTADO", t.faint());
    slots.push((Rect::new(c.x, y + 2, 20, 9), best["uri"].as_str().map(str::to_string)));
    let title = track_title(best);
    putn(buf, c.x, y + 12, &title, 38, t.title());
    put(buf, c.x, y + 13, "▔".repeat(title.chars().count().min(38)), t.accent());
    let what = [best["artist"].as_str(), best["album"].as_str()].into_iter().flatten().collect::<Vec<_>>().join(" · ");
    let kind = if what.is_empty() { "Canción".to_string() } else { format!("Canción · {what}") };
    putn(buf, c.x, y + 14, kind, 38, t.dim());
    put(buf, c.x, y + 16, format!("  {I_PLAY}  Reproducir  "), Style::new().fg(t.on_accent).bg(t.accent).add_modifier(Modifier::BOLD));
    let rx = c.x + 44;
    let rw = c.width.saturating_sub(44);
    put(buf, rx, y, "CANCIONES", t.faint());
    let rows = 6usize;
    let first = app.search_cursor.saturating_sub(rows - 1);
    for (slot, i) in (first..tracks.len()).take(rows).enumerate() {
      let tr = &tracks[i];
      let ry = y + 2 + slot as u16 * 3;
      if !app.search_editing && i == app.search_cursor {
        buf.set_style(Rect::new(rx, ry, rw, 2), Style::new().bg(t.row));
        put(buf, rx, ry, "▌", t.accent());
        put(buf, rx, ry + 1, "▌", t.accent());
      }
      slots.push((Rect::new(rx + 2, ry, 4, 2), tr["uri"].as_str().map(str::to_string)));
      putn(buf, rx + 8, ry, track_title(tr), rw.saturating_sub(16) as usize, t.text());
      putn(buf, rx + 8, ry + 1, tr["artist"].as_str().unwrap_or(""), rw.saturating_sub(16) as usize, t.faint());
      put(buf, rx + rw.saturating_sub(6), ry, tr["duration_ms"].as_u64().map(fmt_ms).unwrap_or_default(), t.faint());
    }
    y += 20;
  }
  if !playlists.is_empty() {
    put(buf, c.x, y, "PLAYLISTS", t.faint());
    for (i, p) in playlists.iter().take(3).enumerate() {
      put(buf, c.x, y + 1 + i as u16, format!("{I_LIBRARY}  {}", p["name"].as_str().unwrap_or("")), t.text());
    }
    y += 5;
  }
  if !tracks.is_empty() && y + 2 < c.y + c.height {
    put(buf, c.x, y, "EN TU BIBLIOTECA LOCAL", t.faint());
    let room = (c.y + c.height).saturating_sub(y + 2) as usize;
    for (i, tr) in tracks.iter().take(room).enumerate() {
      let selected = files_only && !app.search_editing && i == app.search_cursor;
      let style = if selected { t.accent() } else { t.dim() };
      let line = Line::from(vec![Span::styled(format!("{I_MUSIC}  "), t.faint()), Span::styled(uri_path(tr["uri"].as_str().unwrap_or("")), style)]);
      put_line(buf, c.x, y + 2 + i as u16, &line, c.width);
    }
  }
  for (s, uri) in slots {
    draw_cover(f, app, s, uri.as_deref());
  }
}

/// Caratula de la playlist en la columna: la de su primera pista, si es la
/// playlist abierta (las demas todavia no se pidieron).
fn playlist_cover(app: &App, name: &str) -> Option<String> {
  if app.lib_open.as_deref() != Some(name) {
    return None;
  }
  app.playlist["tracks"][0]["uri"].as_str().map(str::to_string)
}

thread_local! {
  /// Caratulas del cuadro que se esta dibujando: (celdas, uri, atenuada).
  static SLOTS: RefCell<Vec<(Rect, String, bool)>> = const { RefCell::new(Vec::new()) };
  /// Caratulas que todavia no se pidieron.
  static WANTED: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

/// Lo que dejo el ultimo dibujo para la capa de imagenes.
#[derive(Debug, Default, PartialEq)]
pub struct Frame {
  pub placements: Vec<kitty::Placement>,
  pub wanted: Vec<String>,
  /// Caratulas detras de un popup que todavia no tienen version atenuada.
  pub wanted_dimmed: Vec<String>,
}

/// Ubicaciones y pedidos del ultimo `draw`.
pub fn take_frame(app: &App) -> Frame {
  let slots = SLOTS.with(|s| std::mem::take(&mut *s.borrow_mut()));
  let wanted = WANTED.with(|w| std::mem::take(&mut *w.borrow_mut()));
  let mut wanted_dimmed: Vec<String> = Vec::new();
  let mut placements = Vec::new();
  for (area, uri, dimmed) in slots {
    match app.covers.image_for(&uri, dimmed) {
      Some(image) => placements.push(kitty::Placement { image, area }),
      // Mientras se prepara la atenuada, esa caratula no se muestra.
      None if dimmed && !wanted_dimmed.contains(&uri) => wanted_dimmed.push(uri),
      None => {}
    }
  }
  Frame { placements, wanted, wanted_dimmed }
}

/// Lo que ya se dibujo queda detras de un popup: sus caratulas pasan a la
/// version atenuada. Las del popup, que se dibujan despues, no.
fn begin_overlay() {
  SLOTS.with(|s| s.borrow_mut().iter_mut().for_each(|slot| slot.2 = true));
}

/// Caratula de `uri` en `area`. Con la imagen lista, las celdas quedan con el
/// fondo por defecto para que la terminal muestre la imagen (protocolo
/// kitty, ver `kitty.rs`); si no, se ve un lugar reservado.
fn draw_cover(f: &mut RFrame, app: &App, area: Rect, uri: Option<&str>) {
  let t = &app.theme;
  let area = area.intersection(f.area());
  if area.width == 0 || area.height == 0 {
    return;
  }
  if let (true, Some(uri)) = (app.images, uri) {
    if app.covers.image_for(uri, false).is_some() {
      let buf = f.buffer_mut();
      for y in area.y..area.y + area.height {
        for x in area.x..area.x + area.width {
          buf[(x, y)].set_symbol(" ").set_bg(ratatui::style::Color::Reset);
        }
      }
      SLOTS.with(|s| s.borrow_mut().push((area, uri.to_string(), false)));
      return;
    }
    if app.covers.state(uri).is_none() {
      WANTED.with(|w| {
        let mut w = w.borrow_mut();
        if !w.iter().any(|u| u == uri) {
          w.push(uri.to_string());
        }
      });
    }
  }
  f.buffer_mut().set_style(area, Style::new().bg(t.raised));
  put(f.buffer_mut(), area.x + area.width / 2, area.y + area.height / 2, I_MUSIC, t.faint());
}

/// Escrituras que ignoran lo que cae fuera de la pantalla. Ratatui recorta
/// hacia la derecha, pero una fila o columna fuera de rango hace fallar el
/// dibujo; en terminales chicas eso pasa.
fn visible(buf: &Buffer, x: u16, y: u16) -> bool {
  let a = buf.area;
  x >= a.x && y >= a.y && x < a.x + a.width && y < a.y + a.height
}

fn put<S: AsRef<str>>(buf: &mut Buffer, x: u16, y: u16, text: S, style: Style) {
  if visible(buf, x, y) {
    buf.set_string(x, y, text, style);
  }
}

fn putn<S: AsRef<str>>(buf: &mut Buffer, x: u16, y: u16, text: S, width: usize, style: Style) {
  if visible(buf, x, y) {
    buf.set_stringn(x, y, text, width, style);
  }
}

fn put_line(buf: &mut Buffer, x: u16, y: u16, line: &Line, width: u16) {
  if visible(buf, x, y) {
    buf.set_line(x, y, line, width);
  }
}

/// Debajo de este tamano se usa el modo mini (panel chico de herdr o tmux).
pub const MINI_WIDTH: u16 = 72;
pub const MINI_HEIGHT: u16 = 15;

pub fn is_mini(area: Rect) -> bool {
  area.width < MINI_WIDTH || area.height < MINI_HEIGHT
}

/// Modo mini (maqueta 7-mini): caratula, cancion, linea de letra,
/// progreso, controles y un espectro chico.
fn mini(f: &mut RFrame, app: &App, area: Rect) {
  let t = &app.theme;
  let track = &app.player["track"];
  if !track.is_object() {
    let y = area.y + area.height / 2;
    center(f, y, Line::styled(format!("{I_MUSIC}  Nada en reproducción"), t.title()));
    return;
  }
  // Caratula cuadrada a la izquierda si entra.
  let cover_h = area.height.saturating_sub(2).min(8);
  let cover_w = cover_h * 2;
  let show_cover = area.width >= cover_w + 30 && cover_h >= 3;
  if show_cover {
    draw_cover(f, app, Rect::new(area.x + 1, area.y + 1, cover_w, cover_h), app.player["track"]["uri"].as_str());
  }
  let buf = f.buffer_mut();
  let x = if show_cover { area.x + cover_w + 3 } else { area.x + 1 };
  let w = (area.x + area.width).saturating_sub(x + 1);
  let viz_w = if w >= 50 { 12 } else { 0 };
  let text_w = w.saturating_sub(viz_w);
  let rows: Vec<u16> = (area.y..area.y + area.height).collect();
  let row = |i: usize| rows.get(i).copied();
  if let Some(y) = row(1) {
    putn(buf, x, y, track_title(track), text_w as usize, t.title());
  }
  if let Some(y) = row(2) {
    let sub = [track["artist"].as_str(), track["album"].as_str()].into_iter().flatten().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" · ");
    putn(buf, x, y, sub, text_w as usize, t.dim());
  }
  if let (Some(y), Some(line)) = (row(4), app.lyrics_line().and_then(|i| app.lyrics["lines"][i]["text"].as_str())) {
    putn(buf, x, y, format!("▶ {line}"), text_w as usize, t.text());
  }
  let prog_row = if area.height >= 8 { 6 } else { area.height.saturating_sub(2) as usize };
  if let Some(y) = row(prog_row)
    && text_w >= 20
  {
    progress(buf, t, x, y, text_w, app.position_ms(), track["duration_ms"].as_u64());
  }
  if let Some(y) = row(prog_row + 2) {
    let play = if app.playing() { I_PAUSE } else { I_PLAY };
    let vol = (app.player["volume"].as_f64().unwrap_or(1.0) * 100.0).round();
    let controls = format!("{I_SHUFFLE}  {I_PREV}  {play}  {I_NEXT}  {I_REPEAT}    {I_VOLUME} {vol}%");
    putn(buf, x, y, controls, text_w as usize, t.text());
  }
  if viz_w > 0 && app.playing() && area.height >= 6 {
    let h = 3.min(area.height.saturating_sub(4));
    spectrum(buf, t, Rect::new(x + text_w + 1, area.y + prog_row as u16 - h + 1, viz_w - 1, h), &app.viz);
  }
}

/// Texto centrado en una fila.
fn center(f: &mut RFrame, y: u16, line: Line) {
  let area = f.area();
  let w = line.width() as u16;
  if y < area.height {
    put_line(f.buffer_mut(), area.x + area.width.saturating_sub(w) / 2, y, &line, area.width);
  }
}

/// Pantalla cuando no hay daemon (maqueta 8-servicio).
fn service(f: &mut RFrame, t: &Theme, state: State, binary: Option<&str>, message: Option<&str>, busy: bool, confirm: bool) {
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
fn header(f: &mut RFrame, app: &App, area: Rect) {
  let t = &app.theme;
  let buf = f.buffer_mut();
  let mut x = area.x + 2;
  for tab in Tab::ALL {
    let style = if tab == app.tab { t.title().add_modifier(Modifier::UNDERLINED) } else { t.faint() };
    put(buf, x, area.y + 1, tab.label(), style);
    x += tab.label().chars().count() as u16 + 4;
  }
  // Campo de busqueda junto a las pestanas, en Buscar.
  let fx = x + 2;
  let fw = 52u16.min(area.width.saturating_sub(fx + 30));
  if app.tab == Tab::Search && fw >= 12 {
    let border = if app.search_editing { t.accent() } else { t.faint() };
    put(buf, fx, area.y, format!("╭{}╮", "─".repeat(fw as usize - 2)), border);
    put(buf, fx, area.y + 1, "│", border);
    put(buf, fx + fw - 1, area.y + 1, "│", border);
    put(buf, fx, area.y + 2, format!("╰{}╯", "─".repeat(fw as usize - 2)), border);
    let cursor = if app.search_editing { "█" } else { "" };
    let field = Line::from(vec![
      Span::styled(format!("{I_SEARCH}  "), t.accent()),
      Span::styled(app.search_query.clone(), t.title()),
      Span::styled(cursor, t.text()),
    ]);
    put_line(buf, fx + 2, area.y + 1, &field, fw.saturating_sub(4));
  }
  let status = if app.connected {
    Line::from(vec![Span::styled("● ", Style::new().fg(t.ok)), Span::styled("ketaid", Style::new().fg(t.ok))])
  } else {
    Line::from(vec![Span::styled("● ", Style::new().fg(t.error)), Span::styled("ketaid sin conexión", Style::new().fg(t.error))])
  };
  let mut spans = Vec::new();
  if let Some(name) = app.output_name() {
    spans.push(Span::styled(format!("{I_HEADPHONES} {name}    "), t.dim()));
  }
  spans.extend(status.spans);
  let status = Line::from(spans);
  let w = status.width() as u16;
  if area.width > w + 2 {
    put_line(buf, area.x + area.width - w - 2, area.y + 1, &status, w);
  }
}
