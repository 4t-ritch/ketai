//! Carpetas de la biblioteca: agregar, quitar y escanear una o todas.

use ketai_tui::{app::App, client::DaemonMsg, ui};
use ratatui::{
  Terminal,
  backend::TestBackend,
  crossterm::event::{KeyCode, KeyEvent, KeyModifiers},
};
use serde_json::{Value, json};

fn key(c: KeyCode) -> KeyEvent {
  KeyEvent::new(c, KeyModifiers::NONE)
}

fn reply(app: &mut App, method: &str, result: Result<Value, Value>) -> Vec<(String, Value)> {
  let id = 300 + app.pending.len() as u64;
  app.sent(id, method);
  app.on_daemon(DaemonMsg::Reply { id, result })
}

fn opened() -> App {
  let mut app = App { connected: true, ..App::default() };
  assert_eq!(app.on_key(key(KeyCode::Char('f'))), vec![("library.folders".to_string(), json!({}))]);
  assert!(app.folders_open);
  reply(&mut app, "library.folders", Ok(json!({"folders": [{"path": "/home/u/Music", "tracks": 1204}, {"path": "/mnt/nas/musica", "tracks": 8930}]})));
  app
}

fn text(app: &App) -> String {
  let mut term = Terminal::new(TestBackend::new(160, 44)).unwrap();
  term.draw(|f| ui::draw(f, app)).unwrap();
  let buf = term.backend().buffer();
  (0..buf.area.height).map(|y| (0..buf.area.width).map(|x| buf[(x, y)].symbol().to_string()).collect::<String>() + "\n").collect()
}

#[test]
fn lists_folders_with_their_tracks() {
  let t = text(&opened());
  for want in ["Carpetas de la biblioteca", "/home/u/Music", "1204 canciones", "/mnt/nas/musica", "8930 canciones"] {
    assert!(t.contains(want), "missing {want:?}\n{t}");
  }
}

#[test]
fn adding_a_folder_expands_the_home() {
  let mut app = opened();
  assert!(app.on_key(key(KeyCode::Char('a'))).is_empty());
  let mut cmds = Vec::new();
  for c in "~/Música q".chars() {
    cmds.extend(app.on_key(key(KeyCode::Char(c))));
  }
  assert!(cmds.is_empty() && !app.quit, "while typing, letters are text");
  app.on_key(key(KeyCode::Backspace));
  app.on_key(key(KeyCode::Backspace));
  let home = std::env::var("HOME").unwrap();
  assert_eq!(app.on_key(key(KeyCode::Enter)), vec![("library.add_folder".to_string(), json!({"path": format!("{home}/Música")}))]);
  assert!(app.folder_input.is_none());
}

#[test]
fn errors_from_the_daemon_are_shown() {
  let mut app = opened();
  reply(&mut app, "library.add_folder", Err(json!({"code": -32602, "message": "path must be an existing absolute folder"})));
  assert!(text(&app).contains("path must be an existing absolute folder"));
}

#[test]
fn scan_one_or_all_and_remove_with_confirmation() {
  let mut app = opened();
  app.on_key(key(KeyCode::Down));
  assert_eq!(app.on_key(key(KeyCode::Char('s'))), vec![("library.scan".to_string(), json!({"path": "/mnt/nas/musica"}))]);
  assert_eq!(app.on_key(KeyEvent::new(KeyCode::Char('S'), KeyModifiers::SHIFT)), vec![("library.scan".to_string(), json!({}))]);
  assert!(app.on_key(key(KeyCode::Char('x'))).is_empty());
  assert!(app.on_key(key(KeyCode::Esc)).is_empty(), "esc cancels the removal");
  assert!(app.folders_open);
  app.on_key(key(KeyCode::Char('x')));
  assert_eq!(app.on_key(key(KeyCode::Char('y'))), vec![("library.remove_folder".to_string(), json!({"path": "/mnt/nas/musica"}))]);
}

#[test]
fn scan_progress_is_shown_and_refreshes_counts_at_the_end() {
  let mut app = opened();
  app.on_key(key(KeyCode::Char('S')));
  reply(&mut app, "library.scan", Ok(json!({"job": "j_3"})));
  app.on_daemon(DaemonMsg::Event { name: "library.progress".into(), data: json!({"job": "j_3", "done": 120, "total": 400}) });
  let t = text(&app);
  assert!(t.contains("Escaneando") && t.contains("120 / 400"), "{t}");
  let cmds = app.on_daemon(DaemonMsg::Event { name: "library.progress".into(), data: json!({"job": "j_3", "done": 400, "total": 400}) });
  assert_eq!(cmds, vec![("library.folders".to_string(), json!({}))]);
  assert!(!text(&app).contains("Escaneando"));
}

#[test]
fn other_clients_changes_refresh_the_list() {
  let mut app = opened();
  assert_eq!(app.on_daemon(DaemonMsg::Event { name: "library.changed".into(), data: json!({}) }), vec![("library.folders".to_string(), json!({}))]);
  app.on_key(key(KeyCode::Esc));
  assert!(!app.folders_open);
}
