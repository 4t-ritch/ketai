//! Estado y navegacion de la app, y la barra de pestanas.

use ketai_tui::{
  app::{App, Tab},
  client::DaemonMsg,
  ui,
};
use ratatui::{
  Terminal,
  backend::TestBackend,
  crossterm::event::{KeyCode, KeyEvent, KeyModifiers},
  style::Modifier,
};
use serde_json::json;

fn key(c: KeyCode) -> KeyEvent {
  KeyEvent::new(c, KeyModifiers::NONE)
}

fn render(app: &App) -> ratatui::buffer::Buffer {
  let mut term = Terminal::new(TestBackend::new(120, 30)).unwrap();
  term.draw(|f| ui::draw(f, app)).unwrap();
  term.backend().buffer().clone()
}

fn row(buf: &ratatui::buffer::Buffer, y: u16) -> String {
  (0..buf.area.width).map(|x| buf[(x, y)].symbol().to_string()).collect()
}

#[test]
fn tabs_switch_with_numbers_and_tab_keys() {
  let mut app = App::default();
  assert_eq!(app.tab, Tab::NowPlaying);
  app.on_key(key(KeyCode::Char('3')));
  assert_eq!(app.tab, Tab::Library);
  app.on_key(key(KeyCode::Tab));
  assert_eq!(app.tab, Tab::Search);
  app.on_key(key(KeyCode::Tab));
  assert_eq!(app.tab, Tab::NowPlaying, "Tab wraps around");
  app.on_key(key(KeyCode::BackTab));
  assert_eq!(app.tab, Tab::Search);
  assert!(!app.quit);
  app.on_key(key(KeyCode::Char('q')));
  assert!(app.quit);
}

#[test]
fn playback_keys_become_protocol_requests() {
  let mut app = App::default();
  assert_eq!(app.on_key(key(KeyCode::Char(' '))), vec![("player.toggle".to_string(), json!({}))]);
  assert_eq!(app.on_key(key(KeyCode::Char('n'))), vec![("player.next".to_string(), json!({}))]);
  assert_eq!(app.on_key(key(KeyCode::Char('p'))), vec![("player.previous".to_string(), json!({}))]);
  assert!(app.on_key(key(KeyCode::Char('3'))).is_empty());
}

#[test]
fn player_state_events_update_the_app() {
  let mut app = App::default();
  app.on_daemon(DaemonMsg::Event { name: "player.state".into(), data: json!({"status": "playing", "volume": 0.4}) });
  assert_eq!(app.player["status"], "playing");
  app.on_daemon(DaemonMsg::Event { name: "player.position".into(), data: json!({"position_ms": 1234}) });
  assert_eq!(app.player["position_ms"], 1234);
  app.on_daemon(DaemonMsg::Closed);
  assert!(!app.connected);
}

#[test]
fn header_shows_tabs_with_the_active_one_highlighted() {
  let mut app = App { connected: true, ..App::default() };
  app.on_key(key(KeyCode::Char('2')));
  let buf = render(&app);
  let header = row(&buf, 1);
  for name in ["Reproduciendo", "Letras", "Biblioteca", "Buscar"] {
    assert!(header.contains(name), "{header}");
  }
  assert!(header.contains("ketaid"));
  let x = header.find("Letras").unwrap() as u16;
  let active = &buf[(x, 1)];
  assert!(active.modifier.contains(Modifier::BOLD | Modifier::UNDERLINED));
  let x = header.find("Biblioteca").unwrap() as u16;
  assert!(!buf[(x, 1)].modifier.contains(Modifier::BOLD));
}
