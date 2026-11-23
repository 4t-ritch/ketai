//! Buscar (maqueta 5-buscar): consulta, respuestas viejas descartadas,
//! navegacion y reproduccion.

use std::time::{Duration, Instant};

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

fn later() -> Instant {
  Instant::now() + Duration::from_millis(200)
}

/// Escribe y deja pasar la espera: devuelve lo que se mando.
fn type_text(app: &mut App, text: &str) -> Vec<(String, Value)> {
  let mut sent = Vec::new();
  for c in text.chars() {
    sent.extend(app.on_key(key(KeyCode::Char(c))));
  }
  assert!(sent.is_empty(), "no search per keystroke: {sent:?}");
  app.tick(later())
}

#[test]
fn typing_searches_once_after_a_short_pause() {
  let mut app = searching();
  // Antes de la espera no sale nada.
  app.on_key(key(KeyCode::Char('i')));
  assert!(app.tick(Instant::now()).is_empty());
  let sent = type_text(&mut app, "ll");
  assert_eq!(app.search_query, "ill");
  assert_eq!(sent, vec![("library.search".to_string(), json!({"query": "ill", "limit": 50}))]);
  assert!(app.tick(later()).is_empty(), "only once");
  reply(&mut app, "library.search", hits());
  assert_eq!(app.search_results.len(), 2);
  // Borrar tambien busca, despues de la espera.
  assert!(app.on_key(key(KeyCode::Backspace)).is_empty());
  assert_eq!(app.tick(later()), vec![("library.search".to_string(), json!({"query": "il", "limit": 50}))]);
}

#[test]
fn a_stale_reply_never_replaces_newer_results() {
  let mut app = searching();
  type_text(&mut app, "i");
  type_text(&mut app, "l");
  // Llega la respuesta de "i" cuando ya se escribe "il": se descarta.
  reply(&mut app, "library.search", json!({"tracks": [{"title": "old"}]}));
  assert!(app.search_results.is_empty());
  reply(&mut app, "library.search", hits());
  assert_eq!(app.search_results.len(), 2);
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
  type_text(&mut app, "ill");
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
  type_text(&mut app, "illit");
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
  type_text(&mut app, "z");
  reply(&mut app, "library.search", json!({"tracks": []}));
  assert!(text(&app).contains("Sin resultados para «z»"));
}
