//! Barra de Reproduciendo: volumen, dispositivo, favorito y agregar a una
//! playlist (con confirmacion si ya esta).

use ketai_tui::{app::App, client::DaemonMsg, ui};
use ratatui::{
  Terminal,
  backend::TestBackend,
  crossterm::event::{KeyCode, KeyEvent, KeyModifiers},
};
use serde_json::{Value, json};

const URI: &str = "file:///m/burn.ogg";

fn key(c: KeyCode) -> KeyEvent {
  KeyEvent::new(c, KeyModifiers::NONE)
}

fn reply(app: &mut App, method: &str, result: Result<Value, Value>) -> Vec<(String, Value)> {
  let id = 700 + app.pending.len() as u64;
  app.sent(id, method);
  app.on_daemon(DaemonMsg::Reply { id, result })
}

fn playing() -> (App, Vec<(String, Value)>) {
  let mut app = App { connected: true, ..App::default() };
  let cmds = app.on_daemon(DaemonMsg::Event {
    name: "player.state".into(),
    data: json!({"status": "playing", "position_ms": 0, "volume": 0.4, "output": "bluez_output.x",
      "track": {"id": "t_1", "uri": URI, "title": "LET THE WORLD BURN", "artist": "Midnight Blu", "duration_ms": 201000}}),
  });
  (app, cmds)
}

fn text(app: &App) -> String {
  let mut term = Terminal::new(TestBackend::new(160, 44)).unwrap();
  term.draw(|f| ui::draw(f, app)).unwrap();
  let buf = term.backend().buffer();
  (0..buf.area.height).map(|y| (0..buf.area.width).map(|x| buf[(x, y)].symbol().to_string()).collect::<String>() + "\n").collect()
}

#[test]
fn a_new_track_asks_whether_it_is_a_favorite_and_star_toggles_it() {
  let (mut app, cmds) = playing();
  assert!(cmds.contains(&("track.playlists".to_string(), json!({"uri": URI}))), "{cmds:?}");
  reply(&mut app, "track.playlists", Ok(json!({"playlists": ["rock"], "favorite": false})));
  assert_eq!(app.on_key(key(KeyCode::Char('*'))), vec![("favorites.set".to_string(), json!({"uri": URI, "favorite": true}))]);
  // Se marca enseguida; el daemon confirma con playlist.changed.
  assert_eq!(app.now_favorite, Some(true));
  let cmds = app.on_daemon(DaemonMsg::Event { name: "playlist.changed".into(), data: json!({"name": "Favoritos"}) });
  assert!(cmds.contains(&("track.playlists".to_string(), json!({"uri": URI}))), "{cmds:?}");
}

#[test]
fn the_bar_shows_volume_device_favorite_and_add() {
  let (mut app, _) = playing();
  reply(&mut app, "output.list", Ok(json!({"devices": [{"id": "bluez_output.x", "name": "Hisense HS2100", "is_default": true}]})));
  reply(&mut app, "track.playlists", Ok(json!({"playlists": ["rock", "chill"], "favorite": true})));
  let t = text(&app);
  for want in ["40%", "Hisense HS2100", "Favorito", "En 2 playlists", "* favorito", "P agregar"] {
    assert!(t.contains(want), "missing {want:?}\n{t}");
  }
}

#[test]
fn adding_to_a_playlist_asks_before_repeating() {
  let (mut app, _) = playing();
  reply(&mut app, "playlist.list", Ok(json!({"playlists": [{"name": "chill", "count": 3}, {"name": "rock", "count": 1}]})));
  assert!(app.on_key(key(KeyCode::Char('P'))).is_empty());
  assert!(app.add_open);
  assert!(text(&app).contains("Agregar a una playlist"));
  app.on_key(key(KeyCode::Down));
  assert_eq!(app.on_key(key(KeyCode::Enter)), vec![("playlist.add".to_string(), json!({"name": "rock", "uris": [URI]}))]);
  // Ya estaba: el daemon pide confirmar.
  reply(&mut app, "playlist.add", Err(json!({"code": 1009, "message": "already in playlist", "data": {"duplicates": [URI]}})));
  let t = text(&app);
  assert!(t.contains("ya está en «rock»"), "{t}");
  // n: no se agrega.
  assert!(app.on_key(key(KeyCode::Char('n'))).is_empty());
  assert!(!app.add_open);
  // Otra vez, y ahora agregar igual.
  app.on_key(key(KeyCode::Char('P')));
  app.on_key(key(KeyCode::Down));
  app.on_key(key(KeyCode::Enter));
  reply(&mut app, "playlist.add", Err(json!({"code": 1009, "message": "already in playlist", "data": {"duplicates": [URI]}})));
  assert_eq!(
    app.on_key(key(KeyCode::Char('a'))),
    vec![("playlist.add".to_string(), json!({"name": "rock", "uris": [URI], "duplicates": "add"}))]
  );
  reply(&mut app, "playlist.add", Ok(json!({"added": 1})));
  assert!(!app.add_open);
  assert!(app.notice.as_deref().unwrap_or("").contains("rock"), "{:?}", app.notice);
}
