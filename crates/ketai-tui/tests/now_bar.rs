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
  for want in ["40%", "Hisense HS2100", "Favorito  * ", "Agregar a playlist  P ", "En 2 playlists"] {
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

fn text_at(app: &App, w: u16, h: u16) -> String {
  let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
  term.draw(|f| ui::draw(f, app)).unwrap();
  let buf = term.backend().buffer();
  (0..buf.area.height).map(|y| (0..buf.area.width).map(|x| buf[(x, y)].symbol().to_string()).collect::<String>() + "\n").collect()
}

#[test]
fn every_view_shows_the_active_player_status() {
  let (mut app, _) = playing();
  reply(&mut app, "output.list", Ok(json!({"devices": [{"id": "bluez_output.x", "name": "Hisense HS2100", "is_default": true}]})));
  reply(&mut app, "track.playlists", Ok(json!({"playlists": [], "favorite": true})));
  for tab in ['2', '3', '4'] {
    app.search_editing = false;
    app.on_key(key(KeyCode::Char(tab)));
    let t = text(&app);
    for want in ["40%", "Hisense HS2100", ui::I_HEART] {
      assert!(t.contains(want), "tab {tab} missing {want:?}\n{t}");
    }
  }
  // Modo mini: el corazon junto a los controles.
  let t = text_at(&app, 60, 12);
  assert!(t.contains(ui::I_HEART) && t.contains("40%"), "{t}");
}

fn type_text(app: &mut App, s: &str) {
  for c in s.chars() {
    assert!(app.on_key(key(KeyCode::Char(c))).is_empty(), "typing {c:?} is text, not a shortcut");
  }
}

#[test]
fn a_new_playlist_can_be_created_from_the_add_popup() {
  let (mut app, _) = playing();
  reply(&mut app, "playlist.list", Ok(json!({"playlists": []})));
  app.on_key(key(KeyCode::Char('P')));
  assert!(text(&app).contains("c nueva playlist"));
  app.on_key(key(KeyCode::Char('c')));
  type_text(&mut app, "mi mix");
  assert!(text(&app).contains("mi mix"));
  assert_eq!(
    app.on_key(key(KeyCode::Enter)),
    vec![
      ("playlist.save".to_string(), json!({"name": "mi mix", "from_queue": false})),
      ("playlist.add".to_string(), json!({"name": "mi mix", "uris": [URI]}))
    ]
  );
  reply(&mut app, "playlist.save", Ok(json!({})));
  reply(&mut app, "playlist.add", Ok(json!({"added": 1})));
  assert!(!app.add_open);
}

#[test]
fn the_header_no_longer_repeats_the_output() {
  let (mut app, _) = playing();
  reply(&mut app, "output.list", Ok(json!({"devices": [{"id": "bluez_output.x", "name": "Hisense HS2100", "is_default": true}]})));
  let t = text(&app);
  let header: String = t.lines().take(3).collect();
  assert!(!header.contains("Hisense"), "{header}");
  assert!(t.contains("Hisense"), "still in the player");
}

#[test]
fn now_playing_puts_the_details_above_progress_and_controls() {
  let (mut app, _) = playing();
  reply(&mut app, "track.playlists", Ok(json!({"playlists": [], "favorite": false})));
  let t = text(&app);
  let row = |needle: &str| t.lines().position(|l| l.contains(needle)).unwrap_or_else(|| panic!("missing {needle:?}\n{t}"));
  assert!(row("40%") < row("3:21"), "details first, then progress\n{t}");
  assert!(row(ui::I_HEART_EMPTY) < row("3:21"));
  assert!(row("3:21") < row(ui::I_NEXT), "controls under the progress");
}

#[test]
fn titles_from_file_names_are_decoded() {
  let track = json!({"uri": "file:///m/ANIMATR%C3%93NICOS%20RAP%20Freddy%27s.mp3", "title": null});
  assert_eq!(ui::track_title(&track), "ANIMATR\u{d3}NICOS RAP Freddy's.mp3");
}

#[test]
fn the_output_is_never_blank() {
  // Sin la lista todavia: se sigue al default.
  let mut app = App { connected: true, ..App::default() };
  let cmds = app.on_daemon(DaemonMsg::Event {
    name: "player.state".into(),
    data: json!({"status": "paused", "volume": 0.35, "output": null, "track": {"id": "t_1", "uri": URI, "title": "x", "duration_ms": 1000}}),
  });
  assert!(cmds.contains(&("output.list".to_string(), json!({}))), "the list is asked once: {cmds:?}");
  app.on_key(key(KeyCode::Char('3')));
  assert!(text(&app).contains("Default"), "{}", text(&app));
  // Sin ninguna salida disponible.
  reply(&mut app, "output.list", Ok(json!({"devices": []})));
  assert!(text(&app).contains("No device"), "{}", text(&app));
}

#[test]
fn volume_and_output_show_their_keys() {
  let (mut app, _) = playing();
  reply(&mut app, "output.list", Ok(json!({"devices": [{"id": "bluez_output.x", "name": "Hisense HS2100", "is_default": true}]})));
  let t = text(&app);
  let line = t.lines().find(|l| l.contains("40%")).unwrap();
  assert!(line.contains("40%   -   + "), "{line}");
  assert!(line.contains("Hisense HS2100   d "), "{line}");
}

#[test]
fn the_volume_row_keeps_its_width_at_any_volume() {
  // De 100% a 40% o 5%, las teclas y la salida no se corren.
  let mut columns = Vec::new();
  for v in [1.0, 0.4, 0.05] {
    let (mut app, _) = playing();
    reply(&mut app, "output.list", Ok(json!({"devices": [{"id": "bluez_output.x", "name": "Hisense HS2100", "is_default": true}]})));
    if let Some(o) = app.player.as_object_mut() {
      o.insert("volume".into(), json!(v));
    }
    let t = text(&app);
    let line = t.lines().find(|l| l.contains("Hisense")).unwrap().to_string();
    let col = |s: &str| line[..line.find(s).unwrap()].chars().count();
    columns.push((col(" + "), col("Hisense")));
    app.tab = ketai_tui::app::Tab::Library;
    let t = text(&app);
    let bar = t.lines().rev().find(|l| l.contains("Hisense")).unwrap().to_string();
    columns.push((bar[..bar.find("Hisense").unwrap()].chars().count(), 0));
  }
  assert!(columns.chunks(2).all(|c| c == &columns[..2]), "{columns:?}");
}
