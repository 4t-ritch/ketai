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
  match app.tab {
    Tab::NowPlaying => now_playing(f, app, body),
    Tab::Lyrics => lyrics(f, app, body),
    Tab::Library => library(f, app, body),
    Tab::Search => search(f, app, body),
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
  let title = track_title(track);
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
  y += 3;
  lyrics_preview(buf, app, x, y, w);
  y += 5;
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
fn lyrics_preview(buf: &mut ratatui::buffer::Buffer, app: &App, x: u16, y: u16, w: u16) {
  let t = &app.theme;
  let (Some(lines), Some(cur)) = (app.lyrics["lines"].as_array(), app.lyrics_line()) else { return };
  let text = |i: usize| lines.get(i).and_then(|l| l["text"].as_str()).unwrap_or("").to_string();
  if cur > 0 {
    buf.set_stringn(x + 2, y, text(cur - 1), w as usize, t.faint());
  }
  buf.set_stringn(x, y + 1, format!("▶ {}", text(cur)), w as usize, t.title());
  buf.set_stringn(x + 2, y + 2, text(cur + 1), w as usize, t.dim());
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
fn lyrics(f: &mut Frame, app: &App, area: Rect) {
  let t = &app.theme;
  let track = &app.player["track"];
  if !track.is_object() {
    center(f, area.y + area.height / 3, Line::styled("Nada en reproducción", t.title()));
    return;
  }
  draw_cover_slot(f, t, Rect::new(area.x + 4, area.y + 1, 8, 4));
  let buf = f.buffer_mut();
  buf.set_string(area.x + 14, area.y + 2, track_title(track), t.title());
  buf.set_string(area.x + 14, area.y + 3, track["artist"].as_str().unwrap_or(""), t.dim());

  // Capas elegidas y offset, arriba a la derecha.
  let mut chips = vec![Span::styled(layer_label(app, &app.lyrics_primary), t.faint())];
  for id in &app.lyrics_secondary {
    chips.push(Span::styled("  ·  ", t.faint()));
    chips.push(Span::styled(format!("+ {}", layer_label(app, id)), t.accent().add_modifier(Modifier::BOLD)));
  }
  let chips = Line::from(chips);
  let right = area.x + area.width.saturating_sub(chips.width() as u16 + 4);
  buf.set_line(right, area.y + 2, &chips, area.width);
  let offset = app.lyrics["user_offset_ms"].as_i64().unwrap_or(0);
  let info = format!("offset {offset} ms   [ ] ajustar   t traducción   o capa");
  buf.set_string(area.x + area.width.saturating_sub(info.chars().count() as u16 + 4), area.y + 3, info, t.faint());

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
const I_SEARCH: &str = "\u{f002}";
const I_CLOCK: &str = "\u{f017}";

/// Columna de playlists (izquierda en Biblioteca y Buscar).
fn playlists_column(buf: &mut ratatui::buffer::Buffer, app: &App, area: Rect) -> Vec<Rect> {
  let t = &app.theme;
  let x = area.x + 2;
  buf.set_string(x, area.y, format!("{I_LIBRARY}  Tu biblioteca"), t.title());
  buf.set_string(x, area.y + 2, " Playlists ", Style::new().fg(t.on_accent).bg(t.accent));
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
          buf.set_string(area.x, y + dy, "▌", t.accent());
        }
      }
    }
    slots.push(Rect::new(x, y, 6, 3));
    let name = p["name"].as_str().unwrap_or("");
    let w = area.width.saturating_sub(11) as usize;
    buf.set_stringn(x + 8, y, name, w, if sel { t.accent().add_modifier(Modifier::BOLD) } else { t.text() });
    let count = p["count"].as_u64().unwrap_or(0);
    buf.set_stringn(x + 8, y + 1, format!("{count} canciones"), w, t.faint());
  }
  if app.playlists.is_empty() {
    buf.set_string(x, area.y + 4, if app.playlists_loaded { "Sin playlists" } else { "Cargando…" }, t.faint());
  }
  slots
}

/// Columna derecha: lo que suena (como el panel de la opcion A).
fn now_column(f: &mut Frame, app: &App, area: Rect) {
  let t = &app.theme;
  let track = &app.player["track"];
  let x = area.x + 1;
  let w = area.width.saturating_sub(2);
  f.buffer_mut().set_string(x, area.y, "REPRODUCIENDO", t.faint());
  if !track.is_object() {
    f.buffer_mut().set_string(x, area.y + 2, "Nada en reproducción", t.dim());
    return;
  }
  let side = w * 10 / 22 + 1;
  draw_cover_slot(f, t, Rect::new(x, area.y + 2, w, side));
  let buf = f.buffer_mut();
  let y = area.y + 3 + side;
  buf.set_stringn(x, y, track_title(track), w as usize, t.title());
  buf.set_stringn(x, y + 1, track["artist"].as_str().unwrap_or(""), w as usize, t.dim());
  lyrics_preview(buf, app, x, y + 3, w);
  if let Some(next) = app.next_track() {
    buf.set_string(x, y + 8, "SIGUIENTE", t.faint());
    buf.set_stringn(x, y + 10, track_title(next), w as usize, t.text());
    buf.set_stringn(x, y + 11, next["artist"].as_str().unwrap_or(""), w as usize, t.faint());
  }
}

/// Barra de reproduccion de abajo, en las vistas de navegacion.
fn player_bar(f: &mut Frame, app: &App, area: Rect) {
  let t = &app.theme;
  let track = &app.player["track"];
  if track.is_object() {
    draw_cover_slot(f, t, Rect::new(area.x + 2, area.y, 6, 3));
  }
  let buf = f.buffer_mut();
  if track.is_object() {
    buf.set_stringn(area.x + 10, area.y, track_title(track), 40, t.title());
    buf.set_stringn(area.x + 10, area.y + 1, track["artist"].as_str().unwrap_or(""), 40, t.dim());
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
  buf.set_line(cx.saturating_sub(controls.width() as u16 / 2), area.y, &controls, area.width);
  let pw = 70.min(area.width.saturating_sub(70));
  progress(buf, t, cx.saturating_sub(pw / 2), area.y + 2, pw, app.position_ms(), track["duration_ms"].as_u64());
}

fn fmt_total(ms: u64) -> String {
  let min = ms / 60_000;
  if min >= 60 { format!("{} h {} min", min / 60, min % 60) } else { format!("{min} min") }
}

/// Biblioteca: playlists, la playlist abierta y lo que suena (maqueta
/// 4-biblioteca-playlist).
fn library(f: &mut Frame, app: &App, area: Rect) {
  let t = &app.theme;
  let body = Rect::new(area.x, area.y + 1, area.width, area.height.saturating_sub(6));
  let slots = playlists_column(f.buffer_mut(), app, Rect::new(body.x, body.y, 34, body.height));
  for s in slots {
    draw_cover_slot(f, t, s);
  }
  let right_w = 34;
  let center = Rect::new(body.x + 37, body.y, body.width.saturating_sub(37 + right_w + 3), body.height);
  now_column(f, app, Rect::new(body.x + body.width - right_w - 1, body.y, right_w, body.height));
  player_bar(f, app, Rect::new(area.x, area.y + area.height - 4, area.width, 3));

  let Some(name) = app.lib_open.clone() else { return };
  let tracks = app.playlist["tracks"].as_array().cloned().unwrap_or_default();
  draw_cover_slot(f, t, Rect::new(center.x, center.y, 20, 9));
  let buf = f.buffer_mut();
  let x = center.x + 23;
  let w = center.width.saturating_sub(23);
  buf.set_string(x, center.y + 1, "PLAYLIST", t.faint());
  buf.set_stringn(x, center.y + 3, &name, w as usize, t.title());
  buf.set_string(x, center.y + 4, "▔".repeat(name.chars().count().min(w as usize)), t.accent());
  let total: u64 = tracks.iter().filter_map(|t| t["duration_ms"].as_u64()).sum();
  buf.set_string(x, center.y + 6, format!("{} canciones · {}", tracks.len(), fmt_total(total)), t.dim());
  let dir = std::env::var("XDG_DATA_HOME").unwrap_or_else(|_| "~/.local/share".into());
  buf.set_stringn(x, center.y + 7, format!("{dir}/ketai/playlists/{name}.m3u8"), w as usize, t.faint());
  let y = center.y + 11;
  buf.set_string(center.x, y, format!("  {I_PLAY}  "), Style::new().fg(t.on_accent).bg(t.accent).add_modifier(Modifier::BOLD));
  buf.set_string(center.x + 9, y, "⏎ reproducir · a a la cola · esc volver", t.faint());

  let cols = [center.x + 1, center.x + 5, center.x + center.width * 58 / 100, center.x + center.width.saturating_sub(6)];
  let hy = y + 3;
  for (cx, h) in cols.iter().zip(["#", "TÍTULO", "ÁLBUM", I_CLOCK]) {
    buf.set_string(*cx, hy, h, t.faint());
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
      buf.set_string(center.x, ry, "▌", t.accent());
      buf.set_string(center.x, ry + 1, "▌", t.accent());
    }
    buf.set_string(cols[0], ry, if playing { "▶".to_string() } else { (i + 1).to_string() }, if playing { t.accent() } else { t.faint() });
    cover_slots.push(Rect::new(cols[1], ry, 4, 2));
    let tw = cols[2].saturating_sub(cols[1] + 7) as usize;
    buf.set_stringn(cols[1] + 6, ry, track_title(tr), tw, if playing { t.accent().add_modifier(Modifier::BOLD) } else { t.text() });
    buf.set_stringn(cols[1] + 6, ry + 1, tr["artist"].as_str().unwrap_or(""), tw, t.faint());
    buf.set_stringn(cols[2], ry, tr["album"].as_str().unwrap_or(""), cols[3].saturating_sub(cols[2] + 2) as usize, t.dim());
    buf.set_string(cols[3], ry, tr["duration_ms"].as_u64().map(fmt_ms).unwrap_or_default(), t.faint());
  }
  for s in cover_slots {
    draw_cover_slot(f, t, s);
  }
}

/// Ruta legible de una URI `file://`.
fn uri_path(uri: &str) -> String {
  uri.strip_prefix("file://").unwrap_or(uri).replace("%20", " ").replace("%25", "%")
}

/// Buscar (maqueta 5-buscar).
fn search(f: &mut Frame, app: &App, area: Rect) {
  let t = &app.theme;
  let body = Rect::new(area.x, area.y + 1, area.width, area.height.saturating_sub(6));
  let slots = playlists_column(f.buffer_mut(), app, Rect::new(body.x, body.y, 34, body.height));
  for s in slots {
    draw_cover_slot(f, t, s);
  }
  player_bar(f, app, Rect::new(area.x, area.y + area.height - 4, area.width, 3));
  let c = Rect::new(body.x + 37, body.y, body.width.saturating_sub(39), body.height);
  let buf = f.buffer_mut();
  let mut x = c.x;
  for (i, name) in crate::app::SEARCH_FILTERS.iter().enumerate() {
    let style = if i == app.search_filter { Style::new().fg(t.on_accent).bg(t.accent) } else { t.faint() };
    buf.set_string(x, c.y, format!(" {name} "), style);
    x += name.chars().count() as u16 + 3;
  }
  let q = app.search_query.trim();
  let mid = c.y + c.height / 3;
  if q.is_empty() {
    buf.set_string(c.x, mid, "Escribí para buscar en tu biblioteca", t.title());
    buf.set_string(c.x, mid + 2, "Se busca en título, artista, álbum y nombre de archivo", t.faint());
    return;
  }
  if !app.search_done {
    buf.set_string(c.x, mid, "Buscando…", t.faint());
    return;
  }
  let tracks = app.search_tracks();
  let playlists = app.search_playlists();
  if tracks.is_empty() && playlists.is_empty() {
    buf.set_string(c.x, mid, format!("Sin resultados para «{q}»"), t.title());
    buf.set_string(c.x, mid + 2, "Si tu música no aparece, escaneá la carpeta desde la búsqueda rápida (/)", t.faint());
    return;
  }
  let mut slots = Vec::new();
  let mut y = c.y + 3;
  let files_only = app.search_filter == 3;
  if let (Some(best), false) = (tracks.first(), files_only) {
    // Mejor resultado a la izquierda, canciones a la derecha.
    buf.set_string(c.x, y, "MEJOR RESULTADO", t.faint());
    slots.push(Rect::new(c.x, y + 2, 20, 9));
    let title = track_title(best);
    buf.set_stringn(c.x, y + 12, &title, 38, t.title());
    buf.set_string(c.x, y + 13, "▔".repeat(title.chars().count().min(38)), t.accent());
    let what = [best["artist"].as_str(), best["album"].as_str()].into_iter().flatten().collect::<Vec<_>>().join(" · ");
    let kind = if what.is_empty() { "Canción".to_string() } else { format!("Canción · {what}") };
    buf.set_stringn(c.x, y + 14, kind, 38, t.dim());
    buf.set_string(c.x, y + 16, format!("  {I_PLAY}  Reproducir  "), Style::new().fg(t.on_accent).bg(t.accent).add_modifier(Modifier::BOLD));
    let rx = c.x + 44;
    let rw = c.width.saturating_sub(44);
    buf.set_string(rx, y, "CANCIONES", t.faint());
    let rows = 6usize;
    let first = app.search_cursor.saturating_sub(rows - 1);
    for (slot, i) in (first..tracks.len()).take(rows).enumerate() {
      let tr = &tracks[i];
      let ry = y + 2 + slot as u16 * 3;
      if !app.search_editing && i == app.search_cursor {
        buf.set_style(Rect::new(rx, ry, rw, 2), Style::new().bg(t.row));
        buf.set_string(rx, ry, "▌", t.accent());
        buf.set_string(rx, ry + 1, "▌", t.accent());
      }
      slots.push(Rect::new(rx + 2, ry, 4, 2));
      buf.set_stringn(rx + 8, ry, track_title(tr), rw.saturating_sub(16) as usize, t.text());
      buf.set_stringn(rx + 8, ry + 1, tr["artist"].as_str().unwrap_or(""), rw.saturating_sub(16) as usize, t.faint());
      buf.set_string(rx + rw.saturating_sub(6), ry, tr["duration_ms"].as_u64().map(fmt_ms).unwrap_or_default(), t.faint());
    }
    y += 20;
  }
  if !playlists.is_empty() {
    buf.set_string(c.x, y, "PLAYLISTS", t.faint());
    for (i, p) in playlists.iter().take(3).enumerate() {
      buf.set_string(c.x, y + 1 + i as u16, format!("{I_LIBRARY}  {}", p["name"].as_str().unwrap_or("")), t.text());
    }
    y += 5;
  }
  if !tracks.is_empty() && y + 2 < c.y + c.height {
    buf.set_string(c.x, y, "EN TU BIBLIOTECA LOCAL", t.faint());
    let room = (c.y + c.height).saturating_sub(y + 2) as usize;
    for (i, tr) in tracks.iter().take(room).enumerate() {
      let selected = files_only && !app.search_editing && i == app.search_cursor;
      let style = if selected { t.accent() } else { t.dim() };
      let line = Line::from(vec![Span::styled(format!("{I_MUSIC}  "), t.faint()), Span::styled(uri_path(tr["uri"].as_str().unwrap_or("")), style)]);
      buf.set_line(c.x, y + 2 + i as u16, &line, c.width);
    }
  }
  for s in slots {
    draw_cover_slot(f, t, s);
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
  // Campo de busqueda junto a las pestanas, en Buscar.
  if app.tab == Tab::Search {
    let fx = x + 2;
    let fw = 52u16.min(area.width.saturating_sub(fx + 30));
    let border = if app.search_editing { t.accent() } else { t.faint() };
    buf.set_string(fx, area.y, format!("╭{}╮", "─".repeat(fw as usize - 2)), border);
    buf.set_string(fx, area.y + 1, "│", border);
    buf.set_string(fx + fw - 1, area.y + 1, "│", border);
    buf.set_string(fx, area.y + 2, format!("╰{}╯", "─".repeat(fw as usize - 2)), border);
    let cursor = if app.search_editing { "█" } else { "" };
    let field = Line::from(vec![
      Span::styled(format!("{I_SEARCH}  "), t.accent()),
      Span::styled(app.search_query.clone(), t.title()),
      Span::styled(cursor, t.text()),
    ]);
    buf.set_line(fx + 2, area.y + 1, &field, fw.saturating_sub(4));
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
