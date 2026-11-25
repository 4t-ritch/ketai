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
fn the_queue_shows_what_comes_next_with_title_and_artist() {
  let app = app_with_queue(4);
  let buf = render(&app);
  let t = text(&buf);
  assert!(t.contains("COLA  ·  3 canciones"), "{t}");
  for want in ["Song 1", "Artist 1", "Song 2", "Song 3", "Artist 3"] {
    assert!(t.contains(want), "missing {want:?}\n{t}");
  }
  // En la columna derecha: lo que suena (Song 0) no esta en la cola.
  let (x, _) = find(&buf, "Song 1").unwrap();
  assert!(x > 100, "on the right: {x}");
  assert!(find(&buf, "Artist 0").is_some_and(|(ax, _)| ax < 100));
  // La elegida (la primera) resaltada.
  let (x, y) = find(&buf, "Song 1").unwrap();
  assert_eq!(buf[(x, y)].fg, app.theme.accent);
  assert_eq!(buf[(x, y)].bg, app.theme.row);
  let (x, y) = find(&buf, "Song 2").unwrap();
  assert_eq!(buf[(x, y)].fg, app.theme.fg);
}

#[test]
fn an_empty_queue_says_so_and_a_narrow_screen_hides_it() {
  let app = app_with_queue(1);
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
  // Solo la columna de la cola (la de informacion dice "Siguiente  Song 1").
  let in_queue = |buf: &Buffer, s: &str| find_from(buf, s, 100).is_some();
  let buf = render(&app);
  assert!(in_queue(&buf, "Song 1 ") && !in_queue(&buf, "Song 30"), "{}", text(&buf));
  app.queue_cursor = 29;
  let buf = render(&app);
  assert!(in_queue(&buf, "Song 30") && !in_queue(&buf, "Song 1 "), "{}", text(&buf));
  let (x, y) = find(&buf, "Song 30").unwrap();
  assert_eq!(buf[(x, y)].fg, app.theme.accent);
}

#[test]
fn a_long_title_scrolls_sideways() {
  let mut app = app_with_queue(2);
  let long = "A very very long song title that never fits in the queue column";
  app.queue["tracks"][1]["title"] = json!(long);
  app.anim_ms = 0;
  let a = text(&render(&app));
  app.anim_ms = 4000;
  let b = text(&render(&app));
  assert!(a.contains("A very very long") && !a.contains(long), "{a}");
  assert_ne!(a, b, "it moves with time");
}

#[test]
fn every_row_is_a_click_target_with_its_queue_position() {
  let app = app_with_queue(4);
  render(&app);
  let hits = ui::take_hits();
  let rows: Vec<usize> = hits.iter().filter_map(|(_, h)| if let Hit::QueueRow(i) = h { Some(*i) } else { None }).collect();
  assert_eq!(rows, [0, 1, 2]);
  assert_eq!((app.queue_index(0), app.queue_index(2)), (1, 3), "position in queue.*");
}

pub fn click(app: &mut App, x: u16, y: u16) -> Vec<(String, Value)> {
  app.on_mouse(MouseEvent { kind: MouseEventKind::Down(MouseButton::Left), column: x, row: y, modifiers: KeyModifiers::NONE }, std::time::Instant::now())
}
