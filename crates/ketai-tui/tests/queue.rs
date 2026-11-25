//! Cola de canciones en Reproduciendo: se ve lo que viene, se elige, se
//! reordena, se quita y se agrega a una playlist, con teclado y mouse.

use ketai_tui::{
  app::{App, Hit},
  client::DaemonMsg,
  ui,
};
use ratatui::{
  Terminal,
  backend::TestBackend,
  buffer::Buffer,
  crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind},
};
use serde_json::{Value, json};

pub fn key(c: KeyCode) -> KeyEvent {
  KeyEvent::new(c, KeyModifiers::NONE)
}

/// Reproduciendo la primera de `n` canciones de la cola.
pub fn app_with_queue(n: usize) -> App {
  let mut app = App { connected: true, ..App::default() };
  app.on_daemon(DaemonMsg::Event {
    name: "player.state".into(),
    data: json!({"status": "paused", "position_ms": 1000, "volume": 0.7, "repeat": "off", "shuffle": false,
                 "track": {"id": "t_0", "title": "Song 0", "artist": "Artist 0", "duration_ms": 200000, "uri": "file:///m/0.ogg"}}),
  });
  set_queue(&mut app, 0, n);
  app
}

pub fn set_queue(app: &mut App, current: u64, n: usize) {
  let tracks: Vec<Value> =
    (0..n).map(|i| json!({"id": format!("t_{i}"), "title": format!("Song {i}"), "artist": format!("Artist {i}"), "uri": format!("file:///m/{i}.ogg")})).collect();
  app.sent(77, "queue.list");
  app.on_daemon(DaemonMsg::Reply { id: 77, result: Ok(json!({"current": current, "tracks": tracks})) });
}

pub fn render(app: &App) -> Buffer {
  let mut term = Terminal::new(TestBackend::new(160, 44)).unwrap();
  term.draw(|f| ui::draw(f, app)).unwrap();
  term.backend().buffer().clone()
}

pub fn text(buf: &Buffer) -> String {
  (0..buf.area.height).map(|y| (0..buf.area.width).map(|x| buf[(x, y)].symbol().to_string()).collect::<String>() + "\n").collect()
}

/// Como `find`, desde la columna `x0`.
pub fn find_from(buf: &Buffer, needle: &str, x0: u16) -> Option<(u16, u16)> {
  (0..buf.area.height).find_map(|y| {
    let row: String = (x0..buf.area.width).map(|x| buf[(x, y)].symbol().to_string()).collect();
    row.find(needle).map(|x| (x0 + row[..x].chars().count() as u16, y))
  })
}

pub fn find(buf: &Buffer, needle: &str) -> Option<(u16, u16)> {
  (0..buf.area.height).find_map(|y| {
    let row: String = (0..buf.area.width).map(|x| buf[(x, y)].symbol().to_string()).collect();
    row.find(needle).map(|x| (row[..x].chars().count() as u16, y))
  })
}

#[test]
fn the_queue_shows_the_whole_queue_with_the_playing_song_marked() {
  let mut app = app_with_queue(4);
  set_queue(&mut app, 1, 4);
  let buf = render(&app);
  let t = text(&buf);
  assert!(t.contains("COLA  ·  4 canciones"), "{t}");
  for want in ["Song 0", "Song 1", "Artist 1", "Song 2", "Song 3", "Artist 3"] {
    assert!(find_from(&buf, want, 100).is_some(), "missing {want:?}\n{t}");
  }
  // La que suena (Song 1) marcada y con acento; la que ya sono, tenue; las
  // que vienen, normales.
  let (x, y) = find_from(&buf, "Song 1", 100).unwrap();
  assert_eq!(buf[(x, y)].fg, app.theme.accent);
  assert_eq!(buf[(x - 2, y)].symbol(), "▶");
  assert_eq!(buf[(x, y)].bg, app.theme.row, "chosen: the playing one after a track change");
  let (x, y) = find_from(&buf, "Song 0", 100).unwrap();
  assert_eq!(buf[(x, y)].fg, app.theme.faint);
  let (x, y) = find_from(&buf, "Song 2", 100).unwrap();
  assert_eq!(buf[(x, y)].fg, app.theme.fg);
}

#[test]
fn the_header_says_when_the_order_is_not_the_list_one() {
  let mut app = app_with_queue(3);
  assert!(!text(&render(&app)).contains("se repite"));
  app.player["repeat"] = json!("queue");
  assert!(text(&render(&app)).contains("se repite al terminar"));
  app.player["shuffle"] = json!(true);
  assert!(text(&render(&app)).contains("orden aleatorio"));
}

#[test]
fn an_empty_queue_says_so_and_a_narrow_screen_hides_it() {
  let mut app = app_with_queue(1);
  set_queue(&mut app, 0, 0);
  let t = text(&render(&app));
  assert!(t.contains("COLA  ·  0 canciones") && t.contains("La cola está vacía"), "{t}");
  let app = app_with_queue(4);
  let mut term = Terminal::new(TestBackend::new(100, 44)).unwrap();
  term.draw(|f| ui::draw(f, &app)).unwrap();
  assert!(!text(term.backend().buffer()).contains("COLA"), "no room for it");
}

#[test]
fn a_long_queue_scrolls_to_keep_the_chosen_song_visible() {
  let mut app = app_with_queue(40);
  // Solo la columna de la cola.
  let in_queue = |buf: &Buffer, s: &str| find_from(buf, s, 100).is_some();
  let buf = render(&app);
  assert!(in_queue(&buf, "Song 1 ") && !in_queue(&buf, "Song 30"), "{}", text(&buf));
  app.queue_cursor = 30;
  let buf = render(&app);
  assert!(in_queue(&buf, "Song 30") && !in_queue(&buf, "Song 1 "), "{}", text(&buf));
  let (x, y) = find_from(&buf, "Song 30", 100).unwrap();
  assert_eq!(buf[(x, y)].fg, app.theme.fg, "chosen (not playing): bright");
  assert_eq!(buf[(x, y)].bg, app.theme.row);
}

#[test]
fn only_the_playing_song_title_scrolls() {
  let mut app = app_with_queue(3);
  let long = "A very very long song title that never fits in the queue column";
  app.queue["tracks"][0]["title"] = json!(long);
  app.queue["tracks"][2]["title"] = json!(format!("Other {long}"));
  let col = |app: &App| -> String { let b = render(app); (0..b.area.height).map(|y| (100..160).map(|x| b[(x, y)].symbol().to_string()).collect::<String>() + "\n").collect() };
  app.anim_ms = 0;
  let a = col(&app);
  app.anim_ms = 4000;
  let b = col(&app);
  assert!(a.contains("A very very lo") && !a.contains(long), "{a}");
  // La que suena se mueve; la otra queda quieta (cortada).
  let line = |s: &str, start: &str| s.lines().find(|l| l.contains(start)).map(str::to_string);
  assert!(line(&a, "Other A very").is_some() && line(&a, "Other A very") == line(&b, "Other A very"), "{a}\n{b}");
  assert_ne!(a, b, "the playing one moves with time");
}

#[test]
fn every_row_is_a_click_target_with_its_queue_position() {
  let app = app_with_queue(4);
  render(&app);
  let hits = ui::take_hits();
  let rows: Vec<usize> = hits.iter().filter_map(|(_, h)| if let Hit::QueueRow(i) = h { Some(*i) } else { None }).collect();
  assert_eq!(rows, [0, 1, 2, 3]);
}

pub fn click(app: &mut App, x: u16, y: u16) -> Vec<(String, Value)> {
  app.on_mouse(MouseEvent { kind: MouseEventKind::Down(MouseButton::Left), column: x, row: y, modifiers: KeyModifiers::NONE }, std::time::Instant::now())
}

fn methods(cmds: &[(String, Value)]) -> Vec<&str> {
  cmds.iter().map(|(m, _)| m.as_str()).collect()
}

/// App con la cola dibujada (las teclas de la cola valen si se ve).
fn shown(n: usize) -> App {
  let app = app_with_queue(n);
  render(&app);
  app
}

#[test]
fn arrows_choose_and_enter_plays_the_chosen_song() {
  let mut app = shown(5);
  assert_eq!(app.queue_cursor, 0, "starts on the playing one");
  app.on_key(key(KeyCode::Down));
  app.on_key(key(KeyCode::Char('j')));
  assert_eq!(app.queue_cursor, 2);
  app.on_key(key(KeyCode::Char('k')));
  assert_eq!(app.queue_cursor, 1);
  for _ in 0..10 {
    app.on_key(key(KeyCode::Down));
  }
  assert_eq!(app.queue_cursor, 4, "stops at the last one");
  app.queue_cursor = 2;
  assert_eq!(app.on_key(key(KeyCode::Enter)), vec![("queue.jump".to_string(), json!({"index": 2}))]);
}

#[test]
fn shift_k_and_j_move_the_chosen_song_and_the_choice_follows_it() {
  let mut app = shown(5);
  app.queue_cursor = 2;
  assert_eq!(app.on_key(key(KeyCode::Char('K'))), vec![("queue.move".to_string(), json!({"from": 2, "to": 1}))]);
  assert_eq!(app.queue_cursor, 1);
  app.on_key(key(KeyCode::Char('K')));
  assert!(app.on_key(key(KeyCode::Char('K'))).is_empty(), "already the first one");
  assert_eq!(app.on_key(key(KeyCode::Char('J'))), vec![("queue.move".to_string(), json!({"from": 0, "to": 1}))]);
  app.queue_cursor = 4;
  assert!(app.on_key(key(KeyCode::Char('J'))).is_empty(), "already the last one");
}

#[test]
fn x_removes_the_chosen_song_but_not_the_playing_one() {
  let mut app = shown(4);
  app.queue_cursor = 2;
  assert_eq!(app.on_key(key(KeyCode::Char('x'))), vec![("queue.remove".to_string(), json!({"indexes": [2]}))]);
  assert_eq!(app.queue_cursor, 2, "the next one takes its place");
  app.queue_cursor = 3;
  app.on_key(key(KeyCode::Char('x')));
  assert_eq!(app.queue_cursor, 2, "removing the last one chooses the previous");
  // La que suena (0) no se quita.
  app.queue_cursor = 0;
  assert!(app.on_key(key(KeyCode::Char('x'))).is_empty());
  assert!(app.notice.as_deref().is_some_and(|n| n.contains("Es la que suena")));
}

#[test]
fn a_track_change_moves_the_choice_to_the_playing_song() {
  let mut app = shown(5);
  app.queue_cursor = 3;
  set_queue(&mut app, 0, 5);
  assert_eq!(app.queue_cursor, 3, "same playing song: the choice stays");
  set_queue(&mut app, 2, 5);
  assert_eq!(app.queue_cursor, 2);
  set_queue(&mut app, 2, 1);
  assert_eq!(app.queue_cursor, 0, "kept inside the queue");
}

#[test]
fn queue_keys_do_nothing_when_the_queue_is_not_on_screen() {
  let mut app = app_with_queue(4);
  let mut term = Terminal::new(TestBackend::new(100, 44)).unwrap();
  term.draw(|f| ui::draw(f, &app)).unwrap();
  assert!(app.on_key(key(KeyCode::Char('x'))).is_empty());
  assert!(app.on_key(key(KeyCode::Char('K'))).is_empty());
}

#[test]
fn queue_keys_come_from_the_config() {
  let mut app = app_with_queue(4);
  app.config = ketai_tui::config::Config::parse("[keys]\nqueue_remove = \"X\"").unwrap();
  render(&app);
  app.queue_cursor = 2;
  assert!(app.on_key(key(KeyCode::Char('x'))).is_empty());
  assert_eq!(methods(&app.on_key(key(KeyCode::Char('X')))), ["queue.remove"]);
  // Una tecla de la cola no puede repetir una global.
  assert!(ketai_tui::config::Config::parse("[keys]\nqueue_remove = \"n\"").unwrap().keys.duplicates().len() == 1);
}

#[test]
fn the_queue_has_no_key_help_only_buttons_on_the_chosen_song() {
  let mut app = app_with_queue(4);
  let t = text(&render(&app));
  assert!(!t.contains("mover") && !t.contains("reproducir") && !t.contains("quitar"), "{t}");
  // La que suena (elegida al empezar) tiene + pero no ✕.
  let buf = render(&app);
  let (_, y) = find_from(&buf, "Song 0", 100).unwrap();
  let row: String = (100..160).map(|x| buf[(x, y + 1)].symbol().to_string()).collect();
  assert!(row.contains('+') && !row.contains('✕'), "{row}");
  app.queue_cursor = 2;
  let buf = render(&app);
  let (_, y) = find_from(&buf, "Song 2", 100).unwrap();
  let row: String = (100..160).map(|x| buf[(x, y + 1)].symbol().to_string()).collect();
  assert!(row.contains("+ ✕"), "{row}");
}

#[test]
fn a_adds_the_chosen_song_to_a_playlist_without_playing_it() {
  let mut app = shown(4);
  app.queue_cursor = 3;
  let cmds = app.on_key(key(KeyCode::Char('a')));
  assert_eq!(cmds[0], ("track.playlists".to_string(), json!({"uri": "file:///m/3.ogg"})));
  assert!(app.add_open);
  // Las respuestas: la de la cancion de la cola y despues la que suena.
  app.sent_with(90, "track.playlists", &json!({"uri": "file:///m/3.ogg"}));
  app.on_daemon(DaemonMsg::Reply { id: 90, result: Ok(json!({"playlists": ["rock"], "favorite": false})) });
  app.sent_with(91, "track.playlists", &json!({"uri": "file:///m/0.ogg"}));
  app.on_daemon(DaemonMsg::Reply { id: 91, result: Ok(json!({"playlists": ["chill"], "favorite": true})) });
  assert_eq!(app.add_playlists, ["rock"]);
  assert_eq!(app.now_playlists, ["chill"], "the playing song keeps its own");
  app.sent(92, "playlist.list");
  app.on_daemon(DaemonMsg::Reply { id: 92, result: Ok(json!({"playlists": [{"name": "chill", "count": 1}, {"name": "rock", "count": 1}]})) });
  // El popup dice que cancion se agrega y marca donde ya esta.
  let t = text(&render(&app));
  assert!(t.contains("Song 3") && t.contains("rock  ✓") && !t.contains("chill  ✓"), "{t}");
  // Enter agrega esa cancion, no la que suena.
  assert_eq!(app.on_key(key(KeyCode::Enter)), vec![("playlist.add".to_string(), json!({"name": "chill", "uris": ["file:///m/3.ogg"]}))]);
  // x la quita de una playlist que la tiene.
  app.add_cursor = 1;
  assert_eq!(app.on_key(key(KeyCode::Char('x'))), vec![("playlist.remove".to_string(), json!({"name": "rock", "uris": ["file:///m/3.ogg"]}))]);
  // Con P (la que suena) vuelve a ser la que suena.
  app.add_open = false;
  app.on_key(key(KeyCode::Char('P')));
  assert_eq!(app.add_target().as_deref(), Some("file:///m/0.ogg"));
}

fn mouse(app: &mut App, kind: MouseEventKind, x: u16, y: u16) -> Vec<(String, Value)> {
  app.on_mouse(MouseEvent { kind, column: x, row: y, modifiers: KeyModifiers::NONE }, std::time::Instant::now())
}

/// App con la cola dibujada y sus zonas clickeables.
fn clickable(n: usize) -> (App, Buffer) {
  let mut app = app_with_queue(n);
  let buf = render(&app);
  app.hits = ui::take_hits();
  (app, buf)
}

#[test]
fn a_click_chooses_and_a_double_click_plays() {
  let (mut app, buf) = clickable(5);
  let (x, y) = find_from(&buf, "Song 3", 100).unwrap();
  assert!(click(&mut app, x, y).is_empty());
  assert_eq!(app.queue_cursor, 3);
  mouse(&mut app, MouseEventKind::Up(MouseButton::Left), x, y);
  assert_eq!(click(&mut app, x, y), vec![("queue.jump".to_string(), json!({"index": 3}))]);
}

#[test]
fn dragging_a_song_onto_another_row_moves_it_there() {
  let (mut app, buf) = clickable(5);
  let (x1, y1) = find_from(&buf, "Song 1", 100).unwrap();
  let (_, y3) = find_from(&buf, "Song 3", 100).unwrap();
  click(&mut app, x1, y1);
  // Mientras se arrastra, la fila donde caeria se marca.
  mouse(&mut app, MouseEventKind::Drag(MouseButton::Left), x1, y3);
  assert_eq!(app.queue_drag.map(|d| (d.from, d.over)), Some((1, 3)));
  let marked = render(&app);
  let (x, y) = find_from(&marked, "Song 3", 100).unwrap();
  assert_eq!(marked[(x, y)].bg, app.theme.raised);
  app.hits = ui::take_hits();
  let cmds = mouse(&mut app, MouseEventKind::Up(MouseButton::Left), x1, y3);
  assert_eq!(cmds, vec![("queue.move".to_string(), json!({"from": 1, "to": 3}))]);
  assert_eq!(app.queue_cursor, 3, "the choice follows it");
  assert!(app.queue_drag.is_none());
  // Soltarla en su mismo lugar no hace nada.
  click(&mut app, x1, y1);
  assert!(mouse(&mut app, MouseEventKind::Up(MouseButton::Left), x1, y1).is_empty());
}

#[test]
fn the_wheel_over_the_queue_moves_the_choice() {
  let (mut app, buf) = clickable(5);
  let (x, y) = find_from(&buf, "Song 1", 100).unwrap();
  mouse(&mut app, MouseEventKind::ScrollDown, x, y);
  mouse(&mut app, MouseEventKind::ScrollDown, x, y);
  assert_eq!(app.queue_cursor, 2);
  mouse(&mut app, MouseEventKind::ScrollUp, x, y);
  assert_eq!(app.queue_cursor, 1);
}

#[test]
fn the_buttons_of_the_chosen_song_remove_it_or_add_it_to_a_playlist() {
  let (mut app, buf) = clickable(5);
  let (x, y) = find_from(&buf, "Song 2", 100).unwrap();
  click(&mut app, x, y);
  mouse(&mut app, MouseEventKind::Up(MouseButton::Left), x, y);
  let buf = render(&app);
  app.hits = ui::take_hits();
  let (rx, ry) = find_from(&buf, "✕", 100).unwrap();
  assert_eq!(click(&mut app, rx, ry), vec![("queue.remove".to_string(), json!({"indexes": [2]}))]);
  let (px, py) = (rx - 2, ry);
  assert_eq!(buf[(px, py)].symbol(), "+");
  let cmds = click(&mut app, px, py);
  assert_eq!(cmds[0], ("track.playlists".to_string(), json!({"uri": "file:///m/2.ogg"})));
  assert!(app.add_open);
}
