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
  app::{App, Hit, Screen, Tab, fmt_ms},
  service::State,
  theme::Theme,
};

pub fn draw(f: &mut RFrame, app: &App) {
  SLOTS.with(|s| s.borrow_mut().clear());
  WANTED.with(|w| w.borrow_mut().clear());
  RESEND.with(|r| r.borrow_mut().clear());
  HITS.with(|h| h.borrow_mut().clear());
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
  if app.add_open {
    dim(f, t);
    begin_overlay();
    add_popup(f, app);
  }
  config_notice(f, app);
  if let Some(n) = &app.notice {
    let area = f.area();
    if area.height > 0 {
      let y = area.y + area.height - 1;
      putn(f.buffer_mut(), area.x + 1, y, n, area.width.saturating_sub(2) as usize, t.accent());
    }
  }
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

/// Popup "Agregar a una playlist" y, si ya estaba, la confirmacion.
fn add_popup(f: &mut RFrame, app: &App) {
  let t = &app.theme;
  let area = f.area();
  let targets = app.add_targets();
  let h = (targets.len() as u16 * 2 + 8).clamp(10, area.height.saturating_sub(4).max(10));
  let w = 56.min(area.width.saturating_sub(4));
  let pop = Rect::new(area.x + (area.width.saturating_sub(w)) / 2, area.y + (area.height.saturating_sub(h)) / 3, w, h);
  popup_frame(f, t, pop);
  modal(f.area(), pop);
  let buf = f.buffer_mut();
  let x = pop.x + 3;
  let iw = pop.width.saturating_sub(6) as usize;
  if let Some(name) = &app.add_confirm {
    put(buf, x, pop.y + 1, "¿Agregar de nuevo?", t.title());
    putn(buf, x, pop.y + 3, format!("Esta canción ya está en «{name}»."), iw, t.text());
    put(buf, x, pop.y + 5, " a ", Style::new().fg(t.on_accent).bg(t.accent));
    put(buf, x + 4, pop.y + 5, "agregar de todos modos", t.text());
    put(buf, x, pop.y + 6, " n ", t.dim());
    put(buf, x + 4, pop.y + 6, "no agregar", t.text());
    hit(Rect::new(x, pop.y + 5, 26, 1), Hit::Confirm(true));
    hit(Rect::new(x, pop.y + 6, 26, 1), Hit::Confirm(false));
    return;
  }
  put(buf, x, pop.y + 1, "Agregar a una playlist", t.title());
  put(buf, pop.x + pop.width.saturating_sub(9), pop.y + 1, "esc ✕", t.faint());
  if app.naming.is_some() {
    name_field(buf, app, x, pop.y + 3, pop.width.saturating_sub(6));
    return;
  }
  let mut y = pop.y + 3;
  if targets.is_empty() {
    put(buf, x, y, if app.playlists_loaded { "No hay playlists" } else { "Cargando…" }, t.faint());
  }
  for (i, name) in targets.iter().enumerate() {
    if y + 1 > pop.y + pop.height - 2 {
      break;
    }
    hit(Rect::new(pop.x + 1, y, pop.width.saturating_sub(2), 1), Hit::PopupRow(i));
    let mark = if app.now_playlists.contains(name) { "  ✓" } else { "" };
    if i == app.add_cursor {
      buf.set_style(Rect::new(pop.x + 1, y, pop.width.saturating_sub(2), 1), Style::new().bg(t.row));
      put(buf, pop.x + 1, y, "▌", t.accent());
    }
    putn(buf, x, y, format!("{name}{mark}"), iw, t.text());
    y += 2;
  }
  putn(buf, x, pop.y + pop.height - 2, "⏎ agregar · x quitar · c nueva · esc cerrar", iw, t.faint());
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

/// Recuadro redondeado con el borde en el color de acento.
fn popup_frame(f: &mut RFrame, t: &Theme, area: Rect) {
  use ratatui::widgets::{Block, BorderType, Clear, Widget};
  Clear.render(area, f.buffer_mut());
  Block::bordered().border_type(BorderType::Rounded).border_style(t.accent()).style(Style::new().bg(t.popup())).render(area, f.buffer_mut());
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
  modal(f.area(), pop);
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
    hit(Rect::new(pop.x + 1, y, pop.width.saturating_sub(2), 2), Hit::PopupRow(i));
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
  modal(f.area(), pop);
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
    hit(Rect::new(pop.x + 1, y, pop.width.saturating_sub(2), 2), Hit::PopupRow(i));
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
  modal(f.area(), pop);
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
      hit(Rect::new(pop.x + 1, y, pop.width.saturating_sub(2), rows), Hit::PopupRow(i));
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
  hit(Rect::new(bx, y, bar_w, 1), Hit::Seek { x: bx, w: bar_w });
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
  // La caratula ocupa un cuadrado: las celdas miden ~1:2.2. En paneles
  // angostos (4:3) se achica para dejar al menos 34 columnas al texto.
  let cover_h = top_h.saturating_sub(2).min(24).min(area.width.saturating_sub(52) / 2);
  let margin = if area.width < 110 { 3 } else { 6 };
  let cover = Rect::new(area.x + margin, area.y + 1, (cover_h as u32 * 2) as u16, cover_h);
  draw_cover(f, app, cover, app.player["track"]["uri"].as_str());
  let buf = f.buffer_mut();
  let x = cover.x + cover.width + margin;
  let w = (area.x + area.width).saturating_sub(x + margin);
  let s = |k: &str| track[k].as_str().unwrap_or("").to_string();
  let title = track_title(track);
  let roomy = top_h >= 24;
  let mut y = area.y + if roomy { 3 } else { 1 };
  put(buf, x, y, "REPRODUCIENDO", t.faint());
  y += 2;
  putm(buf, app, x, y, &title, w as usize, t.title());
  put(buf, x, y + 1, "▔".repeat(title.chars().count().min(w as usize)), t.accent());
  y += 3;
  putm(buf, app, x, y, s("artist"), w as usize, t.text());
  putm(buf, app, x, y + 1, s("album"), w as usize, t.faint());
  y += if roomy { 4 } else { 3 };
  // Detalles: volumen y salida, favorito y agregar, formato. Cada fila con
  // aire; despues la letra y lo que sigue, y abajo progreso y controles.
  let keycap = |k: &str| Span::styled(format!(" {k} "), Style::new().fg(t.fg).bg(t.raised));
  let muted = app.player["muted"].as_bool().unwrap_or(false);
  let vol = app.player["volume"].as_f64().unwrap_or(1.0);
  let filled = (vol * 14.0).round() as usize;
  let volume = Line::from(vec![
    Span::styled(format!("{}  ", if muted { I_MUTE } else { I_VOLUME }), t.dim()),
    Span::styled("━".repeat(filled), t.dim()),
    Span::styled("●", t.text()),
    Span::styled("━".repeat(14 - filled.min(14)), Style::new().fg(t.raised)),
    // Ancho fijo ("100%", " 40%", "  5%"): lo de la derecha no se corre.
    Span::styled(format!("  {:>4}  ", format!("{}%", (vol * 100.0).round())), t.text()),
    keycap(&key_label(app, crate::keys::Action::VolumeDown)),
    Span::raw(" "),
    keycap(&key_label(app, crate::keys::Action::VolumeUp)),
  ]);
  put_line(buf, x, y, &volume, w);
  hit(Rect::new(x + 3, y, 15, 1), Hit::Volume { x: x + 3, w: 15 });
  {
    use crate::keys::Action as A;
    span_hits(x, y, &volume, &[None, None, None, None, None, Some(A::VolumeDown), None, Some(A::VolumeUp)]);
  }
  {
    let name = app.output_label();
    let icon = if app.output_active.as_deref().is_some_and(|o| o.starts_with("bluez")) { I_HEADPHONES } else { I_DEVICE };
    let device = Line::from(vec![
      Span::styled(format!("{icon}  "), t.dim()),
      Span::styled(format!("{name}  "), t.text()),
      keycap(&key_label(app, crate::keys::Action::Devices)),
    ]);
    // Al lado del volumen si entra; si no, en la fila siguiente.
    let vw = volume.width() as u16 + 5;
    let dw = device.width() as u16;
    let dx = if w >= vw + dw {
      x + vw
    } else {
      y += 1;
      x
    };
    put_line(buf, dx, y, &device, w);
    hit(Rect::new(dx, y, dw, 1), Hit::Act(crate::keys::Action::Devices));
  }
  y += 2;
  let fav = app.now_favorite == Some(true);
  let mine = vec![
    Span::styled(format!("{} Favorito ", if fav { I_HEART } else { I_HEART_EMPTY }), if fav { t.accent() } else { t.dim() }),
    keycap(&key_label(app, crate::keys::Action::Favorite)),
    Span::raw("     "),
    Span::styled("+ Agregar a playlist ", t.dim()),
    keycap(&key_label(app, crate::keys::Action::AddToPlaylist)),
  ];
  let mine = Line::from(mine);
  put_line(buf, x, y, &mine, w);
  {
    use crate::keys::Action as A;
    span_hits(x, y, &mine, &[Some(A::Favorite), Some(A::Favorite), None, Some(A::AddToPlaylist), Some(A::AddToPlaylist)]);
  }
  y += 2;
  // Formato y en cuantas playlists esta.
  let mut info = Vec::new();
  let codec = s("codec");
  if !codec.is_empty() {
    let rate = track["sample_rate"].as_u64().map(|r| format!(" · {}", fmt_rate(r))).unwrap_or_default();
    info.push(format!("{codec}{rate}"));
  }
  let n = app.now_playlists.len();
  if n > 0 {
    info.push(format!("En {n} {}", if n == 1 { "playlist" } else { "playlists" }));
  }
  putn(buf, x, y, info.join("   ·   "), w as usize, t.faint());
  y += 2;
  // Con poco alto se omiten la letra y lo que sigue.
  if roomy {
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
    y += 2;
  }
  // Progreso y controles al pie de la columna.
  let py = y.max((area.y + top_h).saturating_sub(3));
  progress(buf, t, x, py, w, app.position_ms(), track["duration_ms"].as_u64());
  let mut controls = controls(app, false, true);
  if controls.width() as u16 > w {
    controls = self::controls(app, true, true);
  }
  let cw = controls.width() as u16;
  put_line(buf, x + w.saturating_sub(cw) / 2, py + 2, &controls, w);
  span_hits(x + w.saturating_sub(cw) / 2, py + 2, &controls, &CONTROL_ACTIONS);

  if app.playing() {
    let viz = Rect::new(area.x + 2, area.y + area.height - viz_h - 1, area.width.saturating_sub(4), viz_h);
    spectrum(buf, t, viz, &app.viz);
  }
}

/// Controles de reproduccion. Compactos cuando el espacio no alcanza.
fn controls(app: &App, compact: bool, repeat_count: bool) -> Line<'static> {
  let t = &app.theme;
  let on = |b: bool| if b { t.accent() } else { t.faint() };
  let shuffle = app.player["shuffle"].as_bool().unwrap_or(false);
  let repeat = app.player["repeat"].as_str().unwrap_or("off");
  let play = if app.playing() { I_PAUSE } else { I_PLAY };
  let gap = if compact { "   " } else { "      " };
  let one = if repeat_count && repeat == "track" { "1" } else { "" };
  Line::from(vec![
    Span::styled(format!("{I_SHUFFLE}{gap}"), on(shuffle)),
    Span::styled(format!("{I_PREV}{gap}"), t.text()),
    Span::styled(format!("  {play}  "), Style::new().fg(t.on_accent).bg(t.fg)),
    Span::styled(format!("{gap}{I_NEXT}{gap}"), t.text()),
    Span::styled(format!("{I_REPEAT}{one}"), on(repeat != "off")),
  ])
}

/// Titulo de la pista, o el nombre del archivo si no tiene etiqueta.
pub fn track_title(track: &serde_json::Value) -> String {
  match track["title"].as_str().filter(|t| !t.is_empty()) {
    Some(t) => t.to_string(),
    None => {
      let uri = track["uri"].as_str().unwrap_or("");
      let name = uri.rsplit('/').next().unwrap_or(uri);
      percent_decode(name)
    }
  }
}

/// Decodifica los `%XX` de una URI (UTF-8). Lo que no es valido queda igual.
fn percent_decode(s: &str) -> String {
  let b = s.as_bytes();
  let mut out = Vec::with_capacity(b.len());
  let mut i = 0;
  while i < b.len() {
    let hex = |c: u8| (c as char).to_digit(16);
    if b[i] == b'%'
      && i + 2 < b.len()
      && let (Some(h), Some(l)) = (hex(b[i + 1]), hex(b[i + 2]))
    {
      out.push((h * 16 + l) as u8);
      i += 3;
      continue;
    }
    out.push(b[i]);
    i += 1;
  }
  String::from_utf8(out).unwrap_or_else(|_| s.to_string())
}


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
          Style::new().fg(app.theme.upcoming()).add_modifier(bold)
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
  // El modo elegido; si se pidio karaoke y la terminal no puede, se avisa.
  let mode = match app.lyrics_mode {
    crate::config::LyricsMode::Classic => "clásico",
    crate::config::LyricsMode::Karaoke if app.karaoke_on() || !app.images => "karaoke",
    crate::config::LyricsMode::Karaoke => "karaoke (no disponible)",
  };
  let info = format!("offset {offset} ms   [ ] ajustar   t traducción   o capa   modo: {mode} [m]");
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
      // En modo karaoke la linea actual es una imagen de `size` filas.
      // Con un popup abierto la linea va como texto: la imagen va encima del
      // texto y lo taparia.
      let karaoke_view = app.karaoke_on() && !app.popup_open();
      let karaoke = karaoke_view && cur.is_some();
      // Antes de la primera linea (la introduccion) el lugar del karaoke
      // queda vacio y la primera espera debajo, como las que vienen.
      let waiting = karaoke_view && cur.is_none();
      let size = app.config.lyrics.size;
      let big = if karaoke_view { size + 2 } else { 4 };
      // La linea actual va en el centro de la vista; las otras, hacia
      // arriba y hacia abajo, las que entren.
      let anchor = cur.unwrap_or(0);
      let secondary = app.lyrics_secondary.iter().filter(|id| lines[anchor]["secondary"][id.as_str()].is_string()).count() as u16;
      let block = if karaoke_view { size } else { 1 } + secondary;
      let bottom = lines_area.y + lines_area.height;
      let middle = lines_area.y + lines_area.height / 2;
      // Dos filas por encima del centro: abajo estan el espectro y el
      // progreso, y la ultima linea no queda pegada al espectro.
      let y_cur = middle.saturating_sub(block / 2 + 2).max(lines_area.y);
      let before = (app.config.lyrics.lines_before as usize).min(anchor);
      let after = app.config.lyrics.lines_after as usize;
      let mut slots: Vec<(usize, u16)> = Vec::new();
      for k in (1..=before).rev() {
        if let Some(y) = y_cur.checked_sub(4 * k as u16).filter(|y| *y >= lines_area.y) {
          slots.push((anchor - k, y));
        }
      }
      if !waiting {
        slots.push((anchor, y_cur));
      }
      // Esperando, la primera linea ocupa el primer lugar de abajo.
      let first_below = if waiting { 0 } else { anchor + 1 };
      for k in 0..after {
        let y = y_cur + big + 4 * k as u16;
        if first_below + k < lines.len() && y < bottom {
          slots.push((first_below + k, y));
        }
      }
      for (i, y) in slots {
        let line = &lines[i];
        let mut below = y + 1;
        let mut width: u16;
        if karaoke && cur == Some(i) {
          let next = lines.get(i + 1).and_then(|l| l["start_ms"].as_u64());
          let area = Rect::new(lines_area.x, y, lines_area.width, size.min(bottom.saturating_sub(y)));
          // Las celdas quedan con el fondo del tema: la imagen va encima, y
          // si por un instante falta (al cambiar de linea o de tamano) no se
          // ve el fondo de la terminal.
          if let Some((words, times)) = crate::karaoke::line_words(line, next) {
            let colors = crate::karaoke::Colors { background: rgb(t.bg), unsung: rgb(t.upcoming()), sung: rgb(t.accent) };
            KARAOKE.with(|k| *k.borrow_mut() = Some(crate::karaoke::Slot { area, words, times, colors }));
          }
          below = y + area.height;
          width = lines_area.width;
        } else {
          // En karaoke las otras lineas van mas tenues: la que se canta
          // es la que se lee.
          let (past, next) = if karaoke {
            (crate::theme::lerp(t.faint, t.bg, 0.45), crate::theme::lerp(t.upcoming(), t.bg, 0.35))
          } else {
            (t.faint, t.upcoming())
          };
          let text = line["text"].as_str().unwrap_or("").to_string();
          let main = match cur {
            Some(c) if i == c => sung_line(app, line, pos),
            Some(c) if i < c => Line::styled(text, Style::new().fg(past)),
            _ => Line::styled(text, Style::new().fg(next).add_modifier(Modifier::BOLD)),
          };
          width = main.width() as u16;
          center(f, y, main);
        }
        let mut rows = below - y;
        for (k, id) in app.lyrics_secondary.iter().enumerate() {
          let Some(tr) = line["secondary"][id].as_str() else { continue };
          let color = if cur == Some(i) { t.dim } else { t.upcoming_secondary() };
          let tr = Line::styled(tr.to_string(), Style::new().fg(color).add_modifier(Modifier::ITALIC));
          width = width.max(tr.width() as u16);
          center(f, below + k as u16, tr);
          rows = rows.max(below + k as u16 + 1 - y);
        }
        // El parrafo entero (linea y traducciones) salta a su inicio.
        if let Some(ms) = line["start_ms"].as_u64() {
          let w = width.min(lines_area.width);
          let x = lines_area.x + (lines_area.width - w) / 2;
          hit(Rect::new(x, y, w, rows.min(bottom.saturating_sub(y))), Hit::LyricLine(ms));
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
  let cw = controls.width() as u16;
  {
    use crate::keys::Action as A;
    span_hits(area.x + area.width.saturating_sub(cw) / 2, area.y + area.height - 2, &controls, &[Some(A::Previous), Some(A::Toggle), Some(A::Next)]);
  }
  center(f, area.y + area.height - 2, controls);
  let end = area.x + (area.width + cw) / 2;
  status_right(f.buffer_mut(), app, area, area.y + area.height - 2, end);
}

const I_LIBRARY: &str = "\u{f02d}";
pub const I_HEART: &str = "\u{f004}";
pub const I_HEART_EMPTY: &str = "\u{f08a}";
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
  if app.tab == Tab::Library {
    view_chips(buf, app, x, area.y + 2, area.width.saturating_sub(3));
    if app.naming.as_ref().is_some_and(|(_, f)| *f == crate::app::NameFor::Library) {
      name_field(buf, app, x, area.y + 4, area.width.saturating_sub(3));
      return Vec::new();
    }
  } else {
    put(buf, x, area.y + 2, " Playlists ", Style::new().fg(t.on_accent).bg(t.accent));
  }
  let mut slots = Vec::new();
  let focused = app.lib_focus == crate::app::LibFocus::Playlists && app.tab == Tab::Library;
  for (i, p) in app.playlists.iter().enumerate() {
    let y = area.y + 4 + i as u16 * 4;
    if app.tab == Tab::Library && i == 0 {
      hit(Rect::new(area.x, area.y + 4, area.width, area.height.saturating_sub(4)), Hit::LibSideArea);
    }
    if app.tab == Tab::Library {
      hit(Rect::new(area.x, y, area.width, 3), Hit::LibSide(i));
    }
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
    put(buf, x, area.y + 5, if app.playlists_loaded { "Sin playlists" } else { "Cargando…" }, t.faint());
    if app.playlists_loaded && app.tab == Tab::Library {
      put(buf, x, area.y + 6, "c crea una", t.faint());
    }
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

/// Primera tecla configurada para una accion (para mostrarla).
fn key_label(app: &App, action: crate::keys::Action) -> String {
  app.config.keys.keys(action).first().map(|k| k.label()).unwrap_or_else(|| "—".into())
}

/// Estado del reproductor activo, para todas las vistas: volumen, salida y
/// favorito.
fn player_status(app: &App) -> Line<'static> {
  let t = &app.theme;
  let muted = app.player["muted"].as_bool().unwrap_or(false);
  let vol = (app.player["volume"].as_f64().unwrap_or(1.0) * 100.0).round();
  let mut spans = vec![Span::styled(format!("{} {:>4}", if muted { I_MUTE } else { I_VOLUME }, format!("{vol}%")), t.faint())];
  {
    let name = app.output_label();
    let icon = if app.output_active.as_deref().is_some_and(|o| o.starts_with("bluez")) { I_HEADPHONES } else { I_DEVICE };
    spans.push(Span::styled(format!("   {icon} {name}"), t.faint()));
  }
  if app.player["track"].is_object() {
    let fav = app.now_favorite == Some(true);
    spans.push(Span::styled(format!("   {}", if fav { I_HEART } else { I_HEART_EMPTY }), if fav { t.accent() } else { t.faint() }));
  }
  Line::from(spans)
}

/// Pone el estado del reproductor a la derecha de `y`, si entra antes de
/// `limit` (la columna donde termina lo que ya esta en esa fila).
fn status_right(buf: &mut Buffer, app: &App, area: Rect, y: u16, limit: u16) {
  let status = player_status(app);
  let w = status.width() as u16;
  let x = (area.x + area.width).saturating_sub(w + 2);
  if x > limit + 2 {
    put_line(buf, x, y, &status, w);
    // Volumen: silenciar; salida: elegir otra; corazon: favorito.
    use crate::keys::Action as A;
    span_hits(x, y, &status, &[Some(A::Mute), Some(A::Devices), Some(A::Favorite)]);
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
  let mut controls = controls(app, false, false);
  if area.width < 110 {
    controls = self::controls(app, true, false);
  }
  let cx = area.x + area.width / 2;
  let start = cx.saturating_sub(controls.width() as u16 / 2);
  if track.is_object() {
    // El titulo termina antes de los controles.
    let tw = start.saturating_sub(area.x + 12).min(40) as usize;
    putm(buf, app, area.x + 10, area.y, track_title(track), tw, t.title());
    putm(buf, app, area.x + 10, area.y + 1, track["artist"].as_str().unwrap_or(""), tw, t.dim());
  }
  put_line(buf, start, area.y, &controls, area.width);
  span_hits(start, area.y, &controls, &CONTROL_ACTIONS);
  // Volumen, salida y favorito a la derecha (o en la fila de abajo si no
  // entran al lado de los controles).
  let end = start + controls.width() as u16;
  let status_w = player_status(app).width() as u16;
  if (area.x + area.width).saturating_sub(status_w + 2) > end + 2 {
    status_right(buf, app, area, area.y, end);
  } else {
    status_right(buf, app, area, area.y + 1, area.x + area.width / 2);
  }
  let pw = 70.min(area.width.saturating_sub(70)).max(controls.width() as u16 + 12);
  progress(buf, t, cx.saturating_sub(pw / 2), area.y + 2, pw, app.position_ms(), track["duration_ms"].as_u64());
}

fn fmt_total(ms: u64) -> String {
  let min = ms / 60_000;
  if min >= 60 { format!("{} h {} min", min / 60, min % 60) } else { format!("{min} min") }
}

/// Vistas de Biblioteca como pestanas chicas (h / l). Si no entran todas,
/// se muestran las que rodean a la elegida, con flechas que avisan que hay
/// mas y la posicion ("1/7").
fn view_chips(buf: &mut Buffer, app: &App, x: u16, y: u16, w: u16) {
  let t = &app.theme;
  let views = crate::app::LibView::ALL;
  let at = views.iter().position(|v| *v == app.lib_view).unwrap_or(0);
  let pos = format!("{}/{}", at + 1, views.len());
  // Lugar para "‹ ", " ›" y la posicion.
  let room = w.saturating_sub(pos.chars().count() as u16 + 5);
  let width = |from: usize, to: usize| views[from..=to].iter().map(|v| v.label().chars().count() as u16 + 3).sum::<u16>();
  let mut first = 0;
  while first < at && width(first, at) > room {
    first += 1;
  }
  let mut cx = x;
  let n = views.len();
  if first > 0 {
    put(buf, cx, y, "‹", t.accent());
    hit(Rect::new(cx, y, 1, 1), Hit::LibView(views[(at + n - 1) % n]));
  }
  cx += 2;
  let mut last = first;
  for (i, v) in views.iter().enumerate().skip(first) {
    let label = format!(" {} ", v.label());
    let lw = label.chars().count() as u16;
    if cx + lw > x + room + 2 {
      break;
    }
    let style = if *v == app.lib_view { Style::new().fg(t.on_accent).bg(t.accent) } else { t.dim() };
    put(buf, cx, y, &label, style);
    hit(Rect::new(cx, y, lw, 1), Hit::LibView(*v));
    cx += lw + 1;
    last = i;
  }
  if last + 1 < views.len() {
    put(buf, cx, y, "›", t.accent());
    hit(Rect::new(cx, y, 1, 1), Hit::LibView(views[(at + 1) % n]));
  }
  put(buf, x + w.saturating_sub(pos.chars().count() as u16), y, &pos, t.faint());
  put(buf, x + 2, y + 1, "h / l cambia de vista", t.faint());
}

/// Campo para escribir el nombre de una playlist nueva.
fn name_field(buf: &mut Buffer, app: &App, x: u16, y: u16, w: u16) {
  let t = &app.theme;
  let Some((name, _)) = &app.naming else { return };
  put(buf, x, y, "Nombre de la playlist nueva", t.faint());
  buf.set_style(Rect::new(x, y + 1, w, 1), Style::new().bg(t.raised));
  putn(buf, x + 1, y + 1, format!("{name}▏"), w.saturating_sub(2) as usize, t.title());
  put(buf, x, y + 2, "⏎ crear · esc cancelar", t.faint());
}

/// Columna de grupos (artistas, albumes, carpetas, anos, generos).
fn groups_column(buf: &mut Buffer, app: &App, area: Rect) {
  let t = &app.theme;
  let x = area.x + 2;
  put(buf, x, area.y, format!("{I_LIBRARY}  Tu biblioteca"), t.title());
  view_chips(buf, app, x, area.y + 2, area.width.saturating_sub(3));
  let focused = app.lib_focus == crate::app::LibFocus::Playlists;
  hit(Rect::new(area.x, area.y + 4, area.width, area.height.saturating_sub(4)), Hit::LibSideArea);
  if app.lib_view == crate::app::LibView::All {
    put(buf, x, area.y + 4, format!("{} canciones", app.lib_total), t.dim());
    return;
  }
  let rows = area.height.saturating_sub(4) / 2;
  let first = app.lib_cursor.saturating_sub((rows as usize).saturating_sub(1));
  for (slot, i) in (first..app.lib_groups.len()).take(rows as usize).enumerate() {
    let g = &app.lib_groups[i];
    let y = area.y + 4 + slot as u16 * 2;
    hit(Rect::new(area.x, y, area.width, 2), Hit::LibSide(i));
    if i == app.lib_cursor {
      buf.set_style(Rect::new(area.x, y, area.width, 2), Style::new().bg(t.row));
      if focused {
        put(buf, area.x, y, "▌", t.accent());
        put(buf, area.x, y + 1, "▌", t.accent());
      }
    }
    let name = g["name"].as_str().map(|n| {
      // Las carpetas se muestran por su ultimo tramo.
      if app.lib_view == crate::app::LibView::Folders { n.rsplit('/').next().unwrap_or(n).to_string() } else { n.to_string() }
    });
    let w = area.width.saturating_sub(3) as usize;
    match name {
      Some(n) => putn(buf, x, y, n, w, t.text()),
      None => putn(buf, x, y, "Sin dato", w, t.faint()),
    }
    let count = g["count"].as_u64().unwrap_or(0);
    put(buf, x, y + 1, format!("{count} {}", if count == 1 { "canción" } else { "canciones" }), t.faint());
  }
}

/// Tabla de canciones en columnas, como en Spotify: #, titulo y artista,
/// album, ano y duracion. Devuelve los lugares de las caratulas.
fn track_table(buf: &mut Buffer, app: &App, area: Rect, rows_src: &[serde_json::Value]) -> Vec<(Rect, Option<String>)> {
  let t = &app.theme;
  let wide = area.width >= 70;
  let dur_x = area.x + area.width.saturating_sub(6);
  let year_x = if wide { dur_x.saturating_sub(7) } else { dur_x };
  let album_x = area.x + area.width * 55 / 100;
  let cols = [area.x + 1, area.x + 5, album_x, year_x, dur_x];
  let mut heads = vec![(cols[0], "#"), (cols[1], "TÍTULO"), (cols[2], "ÁLBUM")];
  if wide {
    heads.push((cols[3], "AÑO"));
  }
  heads.push((cols[4], I_CLOCK));
  for (cx, h) in heads {
    put(buf, cx, area.y, h, t.faint());
  }
  // Toda la lista responde a la rueda; las filas, ademas, al clic.
  hit(area, Hit::LibListArea);
  let playing_uri = app.player["track"]["uri"].as_str();
  let rows = (area.height.saturating_sub(2) / 3) as usize;
  let first = app.track_cursor.saturating_sub(rows.saturating_sub(1));
  let mut slots = Vec::new();
  let focus = app.lib_focus == crate::app::LibFocus::Tracks;
  for (slot, i) in (first..rows_src.len()).take(rows).enumerate() {
    let tr = &rows_src[i];
    let ry = area.y + 2 + slot as u16 * 3;
    hit(Rect::new(area.x, ry, area.width, 2), Hit::LibTrack(i));
    let playing = playing_uri.is_some() && tr["uri"].as_str() == playing_uri;
    if focus && i == app.track_cursor {
      buf.set_style(Rect::new(area.x, ry, area.width, 2), Style::new().bg(t.row));
      put(buf, area.x, ry, "▌", t.accent());
      put(buf, area.x, ry + 1, "▌", t.accent());
    }
    put(buf, cols[0], ry, if playing { "▶".to_string() } else { (i + 1).to_string() }, if playing { t.accent() } else { t.faint() });
    slots.push((Rect::new(cols[1], ry, 4, 2), tr["uri"].as_str().map(str::to_string)));
    let tw = cols[2].saturating_sub(cols[1] + 7) as usize;
    // Solo la fila elegida se desplaza: una lista entera en movimiento cansa.
    let style = if playing { t.accent().add_modifier(Modifier::BOLD) } else { t.text() };
    let (title, artist) = (track_title(tr), tr["artist"].as_str().unwrap_or("").to_string());
    if focus && i == app.track_cursor {
      putm(buf, app, cols[1] + 6, ry, title, tw, style);
      putm(buf, app, cols[1] + 6, ry + 1, artist, tw, t.faint());
    } else {
      putn(buf, cols[1] + 6, ry, title, tw, style);
      putn(buf, cols[1] + 6, ry + 1, artist, tw, t.faint());
    }
    putn(buf, cols[2], ry, tr["album"].as_str().unwrap_or(""), cols[3].saturating_sub(cols[2] + 2) as usize, t.dim());
    if wide && let Some(y) = tr["year"].as_u64() {
      put(buf, cols[3], ry, y.to_string(), t.faint());
    }
    put(buf, cols[4], ry, tr["duration_ms"].as_u64().map(fmt_ms).unwrap_or_default(), t.faint());
  }
  slots
}

/// Biblioteca en las vistas de canciones y grupos.
fn library_browse(f: &mut RFrame, app: &App, body: Rect) {
  let t = &app.theme;
  let side_w = if body.width < 130 { (body.width / 3).clamp(18, 34) } else { 34 };
  groups_column(f.buffer_mut(), app, Rect::new(body.x, body.y, side_w, body.height));
  let center = Rect::new(body.x + side_w + 3, body.y, body.width.saturating_sub(side_w + 4), body.height);
  let buf = f.buffer_mut();
  let title = match app.lib_view {
    crate::app::LibView::All => "Todas las canciones".to_string(),
    _ => app
      .lib_groups
      .get(app.lib_cursor)
      .map(|g| g["name"].as_str().map(str::to_string).unwrap_or_else(|| "Sin dato".into()))
      .unwrap_or_default(),
  };
  putn(buf, center.x, center.y + 1, &title, center.width as usize, t.title());
  put(buf, center.x, center.y + 2, "▔".repeat(title.chars().count().min(center.width as usize)), t.accent());
  let n = app.lib_total;
  let total: u64 = app.lib_tracks.iter().filter_map(|t| t["duration_ms"].as_u64()).sum();
  put(buf, center.x, center.y + 3, format!("{n} {} · {}", if n == 1 { "canción" } else { "canciones" }, fmt_total(total)), t.dim());
  putn(buf, center.x, center.y + 4, "⏎ reproducir · a a la cola · h/l vista · esc volver", center.width as usize, t.faint());
  let table = Rect::new(center.x, center.y + 6, center.width, center.height.saturating_sub(6));
  let slots = track_table(buf, app, table, &app.lib_tracks);
  for (s, uri) in slots {
    draw_cover(f, app, s, uri.as_deref());
  }
}

/// Biblioteca: playlists, la playlist abierta y lo que suena (maqueta
/// 4-biblioteca-playlist).
fn library(f: &mut RFrame, app: &App, area: Rect) {
  let t = &app.theme;
  let body = Rect::new(area.x, area.y + 1, area.width, area.height.saturating_sub(6));
  if app.lib_view != crate::app::LibView::Playlists {
    library_browse(f, app, body);
    player_bar(f, app, Rect::new(area.x, area.y + area.height - 4, area.width, 3));
    return;
  }
  // En paneles angostos (4:3) la columna de lo que suena se oculta (ya esta
  // la barra de abajo) y la de playlists se achica.
  let side_w = if body.width < 130 { (body.width / 3).clamp(18, 34) } else { 34 };
  let slots = playlists_column(f.buffer_mut(), app, Rect::new(body.x, body.y, side_w, body.height));
  for (s, uri) in slots {
    draw_cover(f, app, s, uri.as_deref());
  }
  let right_w = if body.width < 130 { 0 } else { 34 };
  let center = Rect::new(body.x + side_w + 3, body.y, body.width.saturating_sub(side_w + 3 + right_w + 3), body.height);
  if right_w > 0 {
    now_column(f, app, Rect::new(body.x + body.width - right_w - 1, body.y, right_w, body.height));
  }
  player_bar(f, app, Rect::new(area.x, area.y + area.height - 4, area.width, 3));

  let Some(name) = app.lib_open.clone() else { return };
  let tracks = app.playlist["tracks"].as_array().cloned().unwrap_or_default();
  // Con poco alto la cabecera se achica para dejar lugar a las canciones.
  let short = center.height < 30;
  let (cw, ch) = if short { (10, 4) } else { (20, 9) };
  draw_cover(f, app, Rect::new(center.x, center.y, cw, ch), tracks.first().and_then(|t| t["uri"].as_str()));
  let buf = f.buffer_mut();
  let x = center.x + cw + 3;
  let w = center.width.saturating_sub(cw + 3);
  let total: u64 = tracks.iter().filter_map(|t| t["duration_ms"].as_u64()).sum();
  let count = format!("{} canciones · {}", tracks.len(), fmt_total(total));
  let y = if short {
    putn(buf, x, center.y, &name, w as usize, t.title());
    put(buf, x, center.y + 1, "▔".repeat(name.chars().count().min(w as usize)), t.accent());
    put(buf, x, center.y + 2, count, t.dim());
    putn(buf, x, center.y + 3, app.playlist["path"].as_str().unwrap_or(""), w as usize, t.faint());
    center.y + 5
  } else {
    put(buf, x, center.y + 1, "PLAYLIST", t.faint());
    putn(buf, x, center.y + 3, &name, w as usize, t.title());
    put(buf, x, center.y + 4, "▔".repeat(name.chars().count().min(w as usize)), t.accent());
    put(buf, x, center.y + 6, count, t.dim());
    // La ruta la dice el daemon (la carpeta se configura en config.toml).
    putn(buf, x, center.y + 7, app.playlist["path"].as_str().unwrap_or(""), w as usize, t.faint());
    center.y + 11
  };
  put(buf, center.x, y, format!("  {I_PLAY}  "), Style::new().fg(t.on_accent).bg(t.accent).add_modifier(Modifier::BOLD));
  putn(buf, center.x + 9, y, "⏎ reproducir · a a la cola · x quitar · esc volver", center.width.saturating_sub(9) as usize, t.faint());

  let hy = y + if short { 2 } else { 3 };
  let tracks_src = app.lib_rows().to_vec();
  let cover_slots = track_table(buf, app, Rect::new(center.x, hy, center.width, (center.y + center.height).saturating_sub(hy)), &tracks_src);
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
    hit(Rect::new(x, c.y, name.chars().count() as u16 + 2, 1), Hit::SearchFilter(i));
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
      hit(Rect::new(rx, ry, rw, 2), Hit::SearchRow(i));
      if !app.search_editing && i == app.search_cursor {
        buf.set_style(Rect::new(rx, ry, rw, 2), Style::new().bg(t.row));
        put(buf, rx, ry, "▌", t.accent());
        put(buf, rx, ry + 1, "▌", t.accent());
      }
      slots.push((Rect::new(rx + 2, ry, 4, 2), tr["uri"].as_str().map(str::to_string)));
      let tw = rw.saturating_sub(16) as usize;
      if !app.search_editing && i == app.search_cursor {
        putm(buf, app, rx + 8, ry, track_title(tr), tw, t.text());
        putm(buf, app, rx + 8, ry + 1, tr["artist"].as_str().unwrap_or(""), tw, t.faint());
      } else {
        putn(buf, rx + 8, ry, track_title(tr), tw, t.text());
        putn(buf, rx + 8, ry + 1, tr["artist"].as_str().unwrap_or(""), tw, t.faint());
      }
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
      hit(Rect::new(c.x, y + 2 + i as u16, c.width, 1), Hit::SearchRow(i));
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
  /// Caratulas `Stale` dibujadas en este cuadro.
  static RESEND: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
  /// Linea del karaoke del cuadro que se esta dibujando.
  static KARAOKE: RefCell<Option<crate::karaoke::Slot>> = const { RefCell::new(None) };
  /// Lo clickeable del cuadro que se esta dibujando.
  static HITS: RefCell<Vec<(Rect, Hit)>> = const { RefCell::new(Vec::new()) };
}

/// Registra algo clickeable. Lo que se registra despues queda encima.
fn hit(area: Rect, h: Hit) {
  if area.width > 0 && area.height > 0 {
    HITS.with(|v| v.borrow_mut().push((area, h)));
  }
}

/// Lo clickeable del ultimo `draw`.
pub fn take_hits() -> Vec<(Rect, Hit)> {
  HITS.with(|v| std::mem::take(&mut *v.borrow_mut()))
}

/// Un popup tapa todo: lo de atras deja de responder y un clic fuera del
/// popup lo cierra.
fn modal(area: Rect, pop: Rect) {
  HITS.with(|v| v.borrow_mut().clear());
  hit(area, Hit::Backdrop);
  hit(pop, Hit::Popup);
}

/// Registra cada tramo de una linea dibujada en (x, y) con su accion.
fn span_hits(x: u16, y: u16, line: &Line, actions: &[Option<crate::keys::Action>]) {
  let mut cx = x;
  for (span, action) in line.spans.iter().zip(actions) {
    let w = span.width() as u16;
    if let Some(a) = action {
      hit(Rect::new(cx, y, w, 1), Hit::Act(*a));
    }
    cx += w;
  }
}

/// Acciones de los cinco tramos de `controls`.
const CONTROL_ACTIONS: [Option<crate::keys::Action>; 5] = {
  use crate::keys::Action as A;
  [Some(A::Shuffle), Some(A::Previous), Some(A::Toggle), Some(A::Next), Some(A::Repeat)]
};

/// Lo que dejo el ultimo dibujo para la capa de imagenes.
#[derive(Debug, Default, PartialEq)]
pub struct Frame {
  pub placements: Vec<kitty::Placement>,
  pub wanted: Vec<String>,
  /// Caratulas detras de un popup que todavia no tienen version atenuada.
  pub wanted_dimmed: Vec<String>,
  /// Caratulas en pantalla que hay que volver a mandar (`Stale`).
  pub resend: Vec<String>,
  /// Todas las caratulas dibujadas en este cuadro (para no borrarlas).
  pub on_screen: Vec<String>,
  /// Caratulas que no se olvidan aunque no esten en pantalla: la que suena
  /// y la siguiente (sus colores se usan al cambiar de pista).
  pub keep: Vec<String>,
}

/// La linea del karaoke que pidio el ultimo `draw`, si hay.
pub fn take_karaoke() -> Option<crate::karaoke::Slot> {
  KARAOKE.with(|k| k.borrow_mut().take())
}

/// Un color del tema como RGB (los del tema son siempre RGB).
fn rgb(c: ratatui::style::Color) -> [u8; 3] {
  match c {
    ratatui::style::Color::Rgb(r, g, b) => [r, g, b],
    _ => [128, 128, 128],
  }
}

/// Ubicaciones y pedidos del ultimo `draw`.
pub fn take_frame(app: &App) -> Frame {
  let slots = SLOTS.with(|s| std::mem::take(&mut *s.borrow_mut()));
  let mut slots_uris: Vec<String> = slots.iter().map(|s| s.1.clone()).collect();
  slots_uris.sort();
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
  let resend = RESEND.with(|r| std::mem::take(&mut *r.borrow_mut()));
  let mut on_screen: Vec<String> = slots_uris;
  on_screen.dedup();
  let keep: Vec<String> =
    [app.now_uri(), app.next_track().and_then(|t| t["uri"].as_str()).map(str::to_string)].into_iter().flatten().collect();
  let mut wanted = wanted;
  // La siguiente se prepara por adelantado (solo esa): al cambiar de pista
  // sus colores ya estan y el tema no parpadea.
  if app.images
    && let Some(next) = keep.get(1).filter(|_| app.now_uri().is_some())
    && app.covers.state(next).is_none()
    && !wanted.contains(next)
  {
    wanted.push(next.clone());
  }
  Frame { placements, wanted, wanted_dimmed, resend, on_screen, keep }
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
    if let Some(crate::covers::CoverState::Stale { .. }) = app.covers.state(uri) {
      RESEND.with(|r| {
        let mut r = r.borrow_mut();
        if !r.iter().any(|u| u == uri) {
          r.push(uri.to_string());
        }
      });
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

/// Como `putn`, pero si el texto no entra se desplaza (ver `marquee`).
fn putm<S: AsRef<str>>(buf: &mut Buffer, app: &App, x: u16, y: u16, text: S, width: usize, style: Style) {
  putn(buf, x, y, crate::marquee::visible(text.as_ref(), width, app.anim_ms), width, style);
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
    let heart = if app.now_favorite == Some(true) { I_HEART } else { I_HEART_EMPTY };
    let controls = format!("{I_SHUFFLE}  {I_PREV}  {play}  {I_NEXT}  {I_REPEAT}    {I_VOLUME} {:>4}  {heart}", format!("{vol}%"));
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
    hit(Rect::new(x, area.y, tab.label().chars().count() as u16, 3), Hit::Tab(tab));
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
  // La salida activa se ve en el reproductor, no aca.
  let w = status.width() as u16;
  if area.width > w + 2 {
    put_line(buf, area.x + area.width - w - 2, area.y + 1, &status, w);
  }
}
