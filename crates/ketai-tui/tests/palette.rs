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
  if !query.is_empty() {
    assert_eq!(sent.last().unwrap(), &("library.search".to_string(), json!({"query": query, "limit": 5})));
    for _ in 1..query.len() {
      reply(&mut app, "library.search", json!({"tracks": []}));
    }
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
  // Sin texto se listan las acciones: escanear ~/Music es la segunda.
  app.on_key(key(KeyCode::Down));
  let home = std::env::var("HOME").unwrap();
  assert_eq!(app.on_key(key(KeyCode::Enter)), vec![("library.scan".to_string(), json!({"path": format!("{home}/Music")}))]);

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
  for want in ["burn", "CANCIONES", "LET THE WORLD BURN", "ACCIONES", "Cambiar salida de audio", "Escanear ~/Music"] {
    assert!(text.contains(want), "missing {want:?}\n{text}");
  }
  // La pestana activa de atras quedo atenuada.
  let row: String = (0..buf.area.width).map(|x| buf[(x, 1)].symbol().to_string()).collect();
  let x = row.find("Reproduciendo").unwrap() as u16;
  assert_ne!(buf[(x, 1)].fg, app.theme.fg);
}
