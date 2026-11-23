//! Biblioteca y playlist (maqueta 4-biblioteca-playlist).

use ketai_tui::{
  app::{App, LibFocus, Tab},
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

/// Responde una peticion como si fuera el daemon.
fn reply(app: &mut App, method: &str, result: Value) -> Vec<(String, Value)> {
  let id = 500 + app.pending.len() as u64;
  app.sent(id, method);
  app.on_daemon(DaemonMsg::Reply { id, result: Ok(result) })
}

fn tracks() -> Value {
  json!({"name": "metal", "tracks": [
    {"id": null, "uri": "file:///m/burn.ogg", "title": "LET THE WORLD BURN", "artist": "Midnight Blu", "album": "Covers, Vol. 2", "duration_ms": 201000},
    {"id": null, "uri": "file:///m/jealousy.ogg", "title": "My Jealousy", "artist": "Tokai Teio", "album": "Uma Musume", "duration_ms": 48000},
    {"id": null, "uri": "file:///m/gone.ogg", "title": null, "artist": null, "album": null, "duration_ms": null}
  ]})
}

fn library_app() -> App {
  let mut app = App { connected: true, ..App::default() };
  let cmds = app.on_key(key(KeyCode::Char('3')));
  assert_eq!(cmds, vec![("playlist.list".to_string(), json!({}))], "entering Biblioteca loads the playlists");
  let cmds = reply(&mut app, "playlist.list", json!({"playlists": [{"name": "chill", "count": 12}, {"name": "metal", "count": 3}]}));
  assert_eq!(cmds, vec![("playlist.get".to_string(), json!({"name": "chill"}))], "the first playlist opens");
  app
}

#[test]
fn moving_in_the_sidebar_opens_each_playlist() {
  let mut app = library_app();
  assert_eq!(app.tab, Tab::Library);
  assert_eq!(app.on_key(key(KeyCode::Down)), vec![("playlist.get".to_string(), json!({"name": "metal"}))]);
  assert!(app.on_key(key(KeyCode::Down)).is_empty(), "stays on the last one");
  assert_eq!(app.on_key(key(KeyCode::Char('k'))), vec![("playlist.get".to_string(), json!({"name": "chill"}))]);
}

#[test]
fn enter_plays_the_playlist_from_the_selected_track() {
  let mut app = library_app();
  app.on_key(key(KeyCode::Down));
  reply(&mut app, "playlist.get", tracks());
  assert!(app.on_key(key(KeyCode::Enter)).is_empty());
  assert_eq!(app.lib_focus, LibFocus::Tracks);
  app.on_key(key(KeyCode::Char('j')));
  assert_eq!(
    app.on_key(key(KeyCode::Enter)),
    vec![("playlist.load".to_string(), json!({"name": "metal", "replace": true})), ("queue.jump".to_string(), json!({"index": 1}))]
  );
  assert_eq!(app.on_key(key(KeyCode::Char('a'))), vec![("queue.add".to_string(), json!({"uris": ["file:///m/jealousy.ogg"]}))]);
  app.on_key(key(KeyCode::Esc));
  assert_eq!(app.lib_focus, LibFocus::Playlists);
}

#[test]
fn changes_to_the_open_playlist_reload_it() {
  let mut app = library_app();
  reply(&mut app, "playlist.get", json!({"name": "chill", "tracks": []}));
  let cmds = app.on_daemon(DaemonMsg::Event { name: "playlist.changed".into(), data: json!({"name": "chill"}) });
  assert_eq!(cmds, vec![("playlist.list".to_string(), json!({})), ("playlist.get".to_string(), json!({"name": "chill"}))]);
  let cmds = app.on_daemon(DaemonMsg::Event { name: "playlist.changed".into(), data: json!({"name": "other"}) });
  assert_eq!(cmds, vec![("playlist.list".to_string(), json!({}))]);
}

fn text(app: &App) -> String {
  let mut term = Terminal::new(TestBackend::new(160, 44)).unwrap();
  term.draw(|f| ui::draw(f, app)).unwrap();
  let buf = term.backend().buffer();
  (0..buf.area.height).map(|y| (0..buf.area.width).map(|x| buf[(x, y)].symbol().to_string()).collect::<String>() + "\n").collect()
}

#[test]
fn shows_sidebar_header_and_tracks_marking_the_one_that_plays() {
  let mut app = library_app();
  app.on_key(key(KeyCode::Down));
  reply(&mut app, "playlist.get", tracks());
  app.on_daemon(DaemonMsg::Event {
    name: "player.state".into(),
    data: json!({"status": "playing", "position_ms": 0, "track": {"id": "t_1", "uri": "file:///m/burn.ogg", "title": "LET THE WORLD BURN", "artist": "Midnight Blu", "duration_ms": 201000}}),
  });
  let t = text(&app);
  for want in ["Tu biblioteca", "chill", "12 canciones", "PLAYLIST", "3 canciones · 4 min", "TÍTULO", "ÁLBUM", "My Jealousy", "Uma Musume", "0:48", "gone.ogg", "REPRODUCIENDO"] {
    assert!(t.contains(want), "missing {want:?}\n{t}");
  }
  let burn_row = t.lines().find(|l| l.contains("Covers, Vol. 2")).unwrap();
  assert!(burn_row.contains('▶'), "playing track is marked: {burn_row}");
}

#[test]
fn a_4_3_pane_shows_the_playlist_and_a_bar_that_does_not_overlap() {
  let mut app = library_app();
  app.on_key(key(KeyCode::Down));
  reply(&mut app, "playlist.get", tracks());
  app.on_daemon(DaemonMsg::Event {
    name: "player.state".into(),
    data: json!({"status": "playing", "position_ms": 0, "repeat": "off", "track": {"id": "t_1", "uri": "file:///m/burn.ogg", "title": "#umamusume #umamusumeprettyderby long title", "artist": "Midnight Blu", "duration_ms": 201000}}),
  });
  for (w, h) in [(88, 40), (76, 28)] {
    let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
    term.draw(|f| ui::draw(f, &app)).unwrap();
    let buf = term.backend().buffer();
    let t: String = (0..h).map(|y| (0..w).map(|x| buf[(x, y)].symbol().to_string()).collect::<String>() + "\n").collect();
    for want in ["chill", "metal", "My Jealousy", ui::I_NEXT, ui::I_REPEAT] {
      assert!(t.contains(want), "{w}x{h} missing {want:?}\n{t}");
    }
    // El titulo de la barra nunca se mete en los controles.
    let bar = t.lines().find(|l| l.contains(ui::I_SHUFFLE)).unwrap();
    let title_end = bar.find("#uma").map(|i| i + bar[i..].find("  ").unwrap_or(bar.len() - i));
    let shuffle = bar.find(ui::I_SHUFFLE).unwrap();
    assert!(title_end.is_none_or(|e| e <= shuffle), "{w}x{h}: {bar}");
  }
}

fn songs() -> Value {
  json!({"total": 2, "tracks": [
    {"id": null, "uri": "file:///m/a.ogg", "title": "Magnetic", "artist": "ILLIT", "album": "SUPER REAL ME", "year": 2024, "genre": "K-Pop", "duration_ms": 160000},
    {"id": null, "uri": "file:///m/b.ogg", "title": "Sugarcoat", "artist": "KISS OF LIFE", "album": "KISS OF LIFE", "year": 2023, "genre": "K-Pop", "duration_ms": 179000}
  ]})
}

#[test]
fn h_and_l_switch_between_playlists_all_songs_and_groups() {
  let mut app = library_app();
  // Todas las canciones.
  assert_eq!(app.on_key(key(KeyCode::Char('l'))), vec![("library.tracks".to_string(), json!({"limit": 5000}))]);
  reply(&mut app, "library.tracks", songs());
  let t = text(&app);
  for want in ["Canciones", "TÍTULO", "ÁLBUM", "AÑO", "Magnetic", "ILLIT", "SUPER REAL ME", "2024", "2:40", "2 canciones"] {
    assert!(t.contains(want), "missing {want:?}\n{t}");
  }
  // Reproducir desde una cancion: la lista entera pasa a la cola.
  app.on_key(key(KeyCode::Enter));
  assert_eq!(app.lib_focus, LibFocus::Tracks);
  app.on_key(key(KeyCode::Down));
  assert_eq!(
    app.on_key(key(KeyCode::Enter)),
    vec![
      ("queue.clear".to_string(), json!({})),
      ("queue.add".to_string(), json!({"uris": ["file:///m/a.ogg", "file:///m/b.ogg"]})),
      ("queue.jump".to_string(), json!({"index": 1}))
    ]
  );
  assert_eq!(app.on_key(key(KeyCode::Char('a'))), vec![("queue.add".to_string(), json!({"uris": ["file:///m/b.ogg"]}))]);
  app.on_key(key(KeyCode::Esc));
  // Artistas: la lista de grupos y las canciones del primero.
  assert_eq!(app.on_key(key(KeyCode::Char('l'))), vec![("library.groups".to_string(), json!({"by": "artist"}))]);
  let cmds = reply(&mut app, "library.groups", json!({"groups": [{"name": null, "count": 3}, {"name": "ILLIT", "count": 1}]}));
  assert_eq!(cmds, vec![("library.tracks".to_string(), json!({"by": "artist", "name": null, "limit": 5000}))]);
  assert_eq!(app.on_key(key(KeyCode::Down)), vec![("library.tracks".to_string(), json!({"by": "artist", "name": "ILLIT", "limit": 5000}))]);
  let t = text(&app);
  for want in ["Artistas", "Sin dato", "ILLIT", "1 canción"] {
    assert!(t.contains(want), "missing {want:?}\n{t}");
  }
  // Siguen album, carpeta, ano y genero; despues vuelve a playlists.
  for by in ["album", "folder", "year", "genre"] {
    assert_eq!(app.on_key(key(KeyCode::Char('l'))), vec![("library.groups".to_string(), json!({"by": by}))]);
  }
  assert_eq!(app.on_key(key(KeyCode::Char('l'))), vec![("playlist.list".to_string(), json!({}))]);
  assert_eq!(app.on_key(key(KeyCode::Char('h'))), vec![("library.groups".to_string(), json!({"by": "genre"}))]);
}

#[test]
fn an_old_reply_for_another_group_is_ignored() {
  let mut app = library_app();
  app.on_key(key(KeyCode::Char('l')));
  app.on_key(key(KeyCode::Char('l')));
  reply(&mut app, "library.groups", json!({"groups": [{"name": "A", "count": 1}, {"name": "B", "count": 1}]}));
  // Se pidieron A y B; llega A cuando ya esta B elegido.
  app.on_key(key(KeyCode::Down));
  let id = 900;
  app.sent(id, "library.tracks");
  app.sent(id + 1, "library.tracks");
  app.on_daemon(DaemonMsg::Reply { id, result: Ok(json!({"total": 1, "tracks": [{"title": "from A"}]})) });
  assert!(!text(&app).contains("from A"));
  app.on_daemon(DaemonMsg::Reply { id: id + 1, result: Ok(json!({"total": 1, "tracks": [{"title": "from B"}]})) });
  assert!(text(&app).contains("from B"));
}

#[test]
fn the_sidebar_tells_there_are_more_views() {
  let app = library_app();
  let mut term = Terminal::new(TestBackend::new(88, 30)).unwrap();
  term.draw(|f| ui::draw(f, &app)).unwrap();
  let buf = term.backend().buffer();
  let t: String = (0..30).map(|y| (0..88).map(|x| buf[(x, y)].symbol().to_string()).collect::<String>() + "\n").collect();
  assert!(t.contains("1/7") && t.contains('›'), "{t}");
}

#[test]
fn c_creates_a_playlist_from_the_library() {
  let mut app = library_app();
  assert!(app.on_key(key(KeyCode::Char('c'))).is_empty());
  for ch in "nueva".chars() {
    assert!(app.on_key(key(KeyCode::Char(ch))).is_empty());
  }
  assert_eq!(app.on_key(key(KeyCode::Enter)), vec![("playlist.save".to_string(), json!({"name": "nueva", "from_queue": false}))]);
}

#[test]
fn x_removes_the_selected_track_from_the_open_playlist() {
  let mut app = library_app();
  app.on_key(key(KeyCode::Down));
  reply(&mut app, "playlist.get", tracks());
  app.on_key(key(KeyCode::Enter));
  app.on_key(key(KeyCode::Down));
  assert_eq!(app.on_key(key(KeyCode::Char('x'))), vec![("playlist.remove".to_string(), json!({"name": "metal", "indexes": [1]}))]);
  // En las vistas de canciones y grupos no hay playlist de la cual quitar.
  app.on_key(key(KeyCode::Char('l')));
  reply(&mut app, "library.tracks", json!({"total": 1, "tracks": [{"uri": "file:///m/a.ogg", "title": "A"}]}));
  app.on_key(key(KeyCode::Enter));
  assert!(app.on_key(key(KeyCode::Char('x'))).is_empty());
}
