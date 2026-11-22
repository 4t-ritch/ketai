//! Buscar (maqueta 5-buscar): consulta, respuestas viejas descartadas,
//! navegacion y reproduccion.

use ketai_tui::{
  app::{App, Tab},
  client::DaemonMsg,
  ui,
};
use ratatui::{
  Terminal,
  backend::TestBackend,
  crossterm::event::{KeyCode, KeyEvent, KeyModifiers},
};
use serde_json::{Value, json};

fn key(c: KeyCode) -> KeyEvent {
  KeyEvent::new(c, KeyModifiers::NONE)
}

fn reply(app: &mut App, method: &str, result: Value) {
  let id = 900 + app.pending.len() as u64;
  app.sent(id, method);
  app.on_daemon(DaemonMsg::Reply { id, result: Ok(result) });
}

fn hits() -> Value {
  json!({"tracks": [
    {"id": null, "uri": "file:///m/k/01%20Magnetic.flac", "title": "Magnetic", "artist": "ILLIT", "album": "SUPER REAL ME", "duration_ms": 160000},
    {"id": null, "uri": "file:///m/k/02%20My%20World.flac", "title": "My World", "artist": "ILLIT", "album": "SUPER REAL ME", "duration_ms": 151000}
  ]})
}

fn searching() -> App {
  let mut app = App { connected: true, ..App::default() };
  // La columna de playlists tambien se carga al entrar a Buscar.
  assert_eq!(app.on_key(key(KeyCode::Char('4'))), vec![("playlist.list".to_string(), json!({}))]);
  assert_eq!(app.tab, Tab::Search);
  app
}

#[test]
fn typing_searches_and_only_the_latest_reply_counts() {
  let mut app = searching();
  let mut sent = Vec::new();
  for c in "ill".chars() {
    sent.extend(app.on_key(key(KeyCode::Char(c))));
  }
  assert_eq!(app.search_query, "ill");
  assert_eq!(sent.len(), 3);
  assert_eq!(sent[2], ("library.search".to_string(), json!({"query": "ill", "limit": 50})));
  // Las respuestas llegan en orden: las de "i" e "il" ya no sirven.
  reply(&mut app, "library.search", json!({"tracks": [{"title": "old"}]}));
  reply(&mut app, "library.search", json!({"tracks": [{"title": "old"}]}));
  assert!(app.search_results.is_empty());
  reply(&mut app, "library.search", hits());
  assert_eq!(app.search_results.len(), 2);
  // Borrar tambien busca.
  assert_eq!(app.on_key(key(KeyCode::Backspace)), vec![("library.search".to_string(), json!({"query": "il", "limit": 50}))]);
}

#[test]
fn while_typing_letters_are_text_not_shortcuts() {
  let mut app = searching();
  app.on_key(key(KeyCode::Char('q')));
  app.on_key(key(KeyCode::Char(' ')));
  assert!(!app.quit);
  assert_eq!(app.search_query, "q ");
  app.on_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL));
  assert!(app.quit);
}

#[test]
fn results_play_or_queue_the_selected_track() {
  let mut app = searching();
  for c in "ill".chars() {
    app.on_key(key(KeyCode::Char(c)));
  }
  for _ in 0..2 {
    reply(&mut app, "library.search", json!({"tracks": []}));
  }
  reply(&mut app, "library.search", hits());
  reply(&mut app, "queue.list", json!({"current": 0, "tracks": [{"title": "a"}, {"title": "b"}, {"title": "c"}]}));
  app.on_key(key(KeyCode::Esc));
  assert!(!app.search_editing);
  app.on_key(key(KeyCode::Down));
  assert_eq!(
    app.on_key(key(KeyCode::Enter)),
    vec![("queue.add".to_string(), json!({"uris": ["file:///m/k/02%20My%20World.flac"]})), ("queue.jump".to_string(), json!({"index": 3}))]
  );
  assert_eq!(app.on_key(key(KeyCode::Char('a'))), vec![("queue.add".to_string(), json!({"uris": ["file:///m/k/02%20My%20World.flac"]}))]);
  // En resultados, las teclas globales vuelven a funcionar; '/' vuelve a escribir.
  assert_eq!(app.on_key(key(KeyCode::Char(' '))), vec![("player.toggle".to_string(), json!({}))]);
  app.on_key(key(KeyCode::Char('/')));
  assert!(app.search_editing);
}

fn text(app: &App) -> String {
  let mut term = Terminal::new(TestBackend::new(160, 44)).unwrap();
  term.draw(|f| ui::draw(f, app)).unwrap();
  let buf = term.backend().buffer();
  (0..buf.area.height).map(|y| (0..buf.area.width).map(|x| buf[(x, y)].symbol().to_string()).collect::<String>() + "\n").collect()
}

#[test]
fn shows_best_result_songs_and_local_files() {
  let mut app = searching();
  for c in "illit".chars() {
    app.on_key(key(KeyCode::Char(c)));
  }
  for _ in 0..4 {
    reply(&mut app, "library.search", json!({"tracks": []}));
  }
  reply(&mut app, "library.search", hits());
  let t = text(&app);
  for want in ["illit", "Todo", "Canciones", "Archivos locales", "MEJOR RESULTADO", "Magnetic", "CANCIONES", "My World", "2:31", "EN TU BIBLIOTECA LOCAL", "/m/k/02 My World.flac"] {
    assert!(t.contains(want), "missing {want:?}\n{t}");
  }
}

#[test]
fn empty_states_explain_what_to_do() {
  let app = searching();
  assert!(text(&app).contains("Escribí para buscar en tu biblioteca"));
  let mut app = searching();
  app.on_key(key(KeyCode::Char('z')));
  reply(&mut app, "library.search", json!({"tracks": []}));
  assert!(text(&app).contains("Sin resultados para «z»"));
}
