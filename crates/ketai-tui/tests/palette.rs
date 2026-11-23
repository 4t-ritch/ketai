//! Busqueda rapida (maqueta 3-busqueda-rapida): paleta sobre cualquier
//! pestana con canciones, playlists y acciones.

use ketai_tui::{
  app::{App, Tab},
  client::DaemonMsg,
  ui,
};
use ratatui::{
  Terminal,
  backend::TestBackend,
  buffer::Buffer,
  crossterm::event::{KeyCode, KeyEvent, KeyModifiers},
};
use serde_json::{Value, json};

fn key(c: KeyCode) -> KeyEvent {
  KeyEvent::new(c, KeyModifiers::NONE)
}

fn reply(app: &mut App, method: &str, result: Value) {
  let id = 700 + app.pending.len() as u64;
  app.sent(id, method);
  app.on_daemon(DaemonMsg::Reply { id, result: Ok(result) });
}

fn burn() -> Value {
  json!({"tracks": [{"uri": "file:///m/burn.ogg", "title": "LET THE WORLD BURN", "artist": "Midnight Blu", "duration_ms": 201000}]})
}

fn open_with(query: &str) -> App {
  let mut app = App { connected: true, ..App::default() };
  assert!(app.on_key(key(KeyCode::Char('/'))).is_empty());
  assert!(app.palette_open);
  let mut sent = Vec::new();
  for c in query.chars() {
    sent.extend(app.on_key(key(KeyCode::Char(c))));
  }
  // Mientras se escribe no se busca: una sola busqueda despues de la espera.
  assert!(sent.is_empty());
  if !query.is_empty() {
    let later = std::time::Instant::now() + std::time::Duration::from_millis(200);
    assert_eq!(app.tick(later), vec![("library.search".to_string(), json!({"query": query, "limit": 5}))]);
    reply(&mut app, "library.search", burn());
  }
  app
}

#[test]
fn typing_finds_songs_and_enter_plays_them() {
  let mut app = open_with("burn");
  assert_eq!(app.palette_results.len(), 1);
  assert!(app.search_results.is_empty(), "the search tab keeps its own results");
  reply(&mut app, "queue.list", json!({"current": 0, "tracks": [{"title": "a"}]}));
  assert_eq!(
    app.on_key(key(KeyCode::Enter)),
    vec![("queue.add".to_string(), json!({"uris": ["file:///m/burn.ogg"]})), ("queue.jump".to_string(), json!({"index": 1}))]
  );
  assert!(!app.palette_open, "playing closes the palette");
}

#[test]
fn tab_queues_and_escape_closes() {
  let mut app = open_with("burn");
  assert_eq!(app.on_key(key(KeyCode::Tab)), vec![("queue.add".to_string(), json!({"uris": ["file:///m/burn.ogg"]}))]);
  assert!(app.palette_open, "queueing keeps the palette open");
  app.on_key(key(KeyCode::Esc));
  assert!(!app.palette_open);
  assert_eq!(app.tab, Tab::NowPlaying);
}

#[test]
fn actions_run_from_the_palette() {
  let mut app = open_with("");
  // Sin texto se listan las acciones: escanear toda la biblioteca es la
  // segunda; la cuarta abre las carpetas.
  app.on_key(key(KeyCode::Down));
  assert_eq!(app.on_key(key(KeyCode::Enter)), vec![("library.scan".to_string(), json!({}))]);

  let mut app = open_with("");
  for _ in 0..3 {
    app.on_key(key(KeyCode::Down));
  }
  assert_eq!(app.on_key(key(KeyCode::Enter)), vec![("library.folders".to_string(), json!({}))]);
  assert!(app.folders_open);

  let mut app = open_with("");
  app.on_key(key(KeyCode::Down));
  app.on_key(key(KeyCode::Down));
  app.on_key(key(KeyCode::Enter));
  assert_eq!(app.tab, Tab::Lyrics);

  let mut app = open_with("");
  app.on_key(key(KeyCode::Enter));
  assert!(app.devices_open, "first action opens the output picker");
}

fn render(app: &App) -> Buffer {
  let mut term = Terminal::new(TestBackend::new(160, 44)).unwrap();
  term.draw(|f| ui::draw(f, app)).unwrap();
  term.backend().buffer().clone()
}

#[test]
fn the_palette_floats_over_a_dimmed_screen() {
  let app = open_with("burn");
  let buf = render(&app);
  let text: String = (0..buf.area.height).map(|y| (0..buf.area.width).map(|x| buf[(x, y)].symbol().to_string()).collect::<String>() + "\n").collect();
  for want in ["burn", "CANCIONES", "LET THE WORLD BURN", "ACCIONES", "Cambiar salida de audio", "Escanear toda la biblioteca", "Carpetas de la biblioteca"] {
    assert!(text.contains(want), "missing {want:?}\n{text}");
  }
  // La pestana activa de atras quedo atenuada.
  let row: String = (0..buf.area.width).map(|x| buf[(x, 1)].symbol().to_string()).collect();
  let x = row.find("Reproduciendo").unwrap() as u16;
  assert_ne!(buf[(x, 1)].fg, app.theme.fg);
}
