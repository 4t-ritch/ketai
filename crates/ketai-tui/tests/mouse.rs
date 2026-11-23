//! Mouse: un clic hace lo mismo que la tecla, sobre lo que se ve.

use std::time::{Duration, Instant};

use ketai_tui::{
  app::{App, LibFocus, LibView, Tab},
  client::DaemonMsg,
  ui,
};
use ratatui::{
  Terminal,
  backend::TestBackend,
  crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind},
};
use serde_json::{Value, json};

const URI: &str = "file:///m/burn.ogg";

fn playing() -> App {
  let mut app = App { connected: true, ..App::default() };
  app.on_daemon(DaemonMsg::Event {
    name: "player.state".into(),
    data: json!({"status": "playing", "position_ms": 0, "volume": 0.5, "repeat": "off", "shuffle": false,
      "track": {"id": "t", "uri": URI, "title": "LET THE WORLD BURN", "artist": "Midnight Blu", "duration_ms": 200000}}),
  });
  app
}

fn reply(app: &mut App, id: u64, m: &str, v: Value) -> Vec<(String, Value)> {
  app.sent(id, m);
  app.on_daemon(DaemonMsg::Reply { id, result: Ok(v) })
}

/// Dibuja (registra lo clickeable) y devuelve el texto de la pantalla.
fn draw(app: &mut App) -> Vec<String> {
  let mut term = Terminal::new(TestBackend::new(160, 44)).unwrap();
  term.draw(|f| ui::draw(f, app)).unwrap();
  app.hits = ui::take_hits();
  let buf = term.backend().buffer();
  (0..44).map(|y| (0..160).map(|x| buf[(x, y)].symbol().to_string()).collect()).collect()
}

/// Columna y fila (en celdas) de la primera aparicion de `needle`.
fn find(lines: &[String], needle: &str) -> (u16, u16) {
  for (y, l) in lines.iter().enumerate() {
    if let Some(byte) = l.find(needle) {
      return (l[..byte].chars().count() as u16, y as u16);
    }
  }
  panic!("{needle:?} not on screen\n{}", lines.join("\n"));
}

fn mouse(kind: MouseEventKind, (column, row): (u16, u16)) -> MouseEvent {
  MouseEvent { kind, column, row, modifiers: KeyModifiers::NONE }
}

fn click(app: &mut App, at: (u16, u16)) -> Vec<(String, Value)> {
  app.on_mouse(mouse(MouseEventKind::Down(MouseButton::Left), at), Instant::now())
}

#[test]
fn tabs_and_player_controls_respond_to_clicks() {
  let mut app = playing();
  let s = draw(&mut app);
  assert_eq!(click(&mut app, find(&s, ui::I_NEXT)), vec![("player.next".to_string(), json!({}))]);
  assert_eq!(click(&mut app, find(&s, ui::I_PREV)), vec![("player.previous".to_string(), json!({}))]);
  assert_eq!(click(&mut app, find(&s, ui::I_PAUSE)), vec![("player.toggle".to_string(), json!({}))]);
  assert_eq!(click(&mut app, find(&s, ui::I_SHUFFLE)), vec![("player.set_shuffle".to_string(), json!({"shuffle": true}))]);
  let fav = find(&s, "Favorito");
  assert_eq!(click(&mut app, fav), vec![("favorites.set".to_string(), json!({"uri": URI, "favorite": true}))]);
  click(&mut app, find(&s, "Letras"));
  assert_eq!(app.tab, Tab::Lyrics);
  // En otra vista, la barra de abajo tambien responde.
  app.tab = Tab::Library;
  let s = draw(&mut app);
  assert_eq!(click(&mut app, find(&s, ui::I_NEXT)), vec![("player.next".to_string(), json!({}))]);
}

#[test]
fn clicking_the_progress_bar_seeks_there() {
  let mut app = playing();
  let s = draw(&mut app);
  let (x0, y) = find(&s, "0:00");
  let (x1, _) = find(&s, "3:20");
  // A la mitad de la barra (entre las dos etiquetas).
  let mid = (x0 + 6 + x1 - 2) / 2;
  // Un clic es presionar y soltar: el salto sale al soltar.
  assert!(click(&mut app, (mid, y)).is_empty());
  let cmds = app.on_mouse(mouse(MouseEventKind::Up(MouseButton::Left), (mid, y)), Instant::now());
  assert_eq!(cmds.len(), 1);
  assert_eq!(cmds[0].0, "player.seek");
  let ms = cmds[0].1["position_ms"].as_u64().unwrap();
  assert!((90_000..=110_000).contains(&ms), "{ms}");
}

#[test]
fn clicking_the_volume_bar_sets_the_volume() {
  let mut app = playing();
  let s = draw(&mut app);
  let (x, y) = find(&s, "50%");
  // Cerca del final de la barra, que termina justo antes del porcentaje.
  let cmds = click(&mut app, (x - 4, y));
  assert_eq!(cmds[0].0, "player.set_volume", "{cmds:?}");
  assert!(cmds[0].1["volume"].as_f64().unwrap() > 0.8, "near the right end: {cmds:?}");
}

fn library_with_songs() -> App {
  let mut app = playing();
  app.on_key(KeyEvent::new(KeyCode::Char('3'), KeyModifiers::NONE));
  reply(&mut app, 2, "playlist.list", json!({"playlists": []}));
  app.on_key(KeyEvent::new(KeyCode::Char('l'), KeyModifiers::NONE));
  reply(&mut app, 3, "library.tracks", json!({"total": 3, "tracks": [
    {"uri": "file:///m/a.ogg", "title": "Alpha", "duration_ms": 1000},
    {"uri": "file:///m/b.ogg", "title": "Bravo", "duration_ms": 1000},
    {"uri": "file:///m/c.ogg", "title": "Charlie", "duration_ms": 1000}
  ]}));
  app
}

#[test]
fn one_click_selects_a_song_and_a_double_click_plays_it() {
  let mut app = library_with_songs();
  let s = draw(&mut app);
  let at = find(&s, "Bravo");
  let t0 = Instant::now();
  assert!(app.on_mouse(mouse(MouseEventKind::Down(MouseButton::Left), at), t0).is_empty());
  assert_eq!((app.lib_focus, app.track_cursor), (LibFocus::Tracks, 1));
  let cmds = app.on_mouse(mouse(MouseEventKind::Down(MouseButton::Left), at), t0 + Duration::from_millis(200));
  assert_eq!(cmds.last().unwrap(), &("queue.jump".to_string(), json!({"index": 1})));
  // Dos clics separados no son doble clic.
  let s = draw(&mut app);
  let c = find(&s, "Charlie");
  app.on_mouse(mouse(MouseEventKind::Down(MouseButton::Left), c), t0 + Duration::from_secs(5));
  assert!(app.on_mouse(mouse(MouseEventKind::Down(MouseButton::Left), c), t0 + Duration::from_secs(7)).is_empty());
  // La rueda mueve la seleccion.
  app.on_mouse(mouse(MouseEventKind::ScrollUp, c), t0);
  assert_eq!(app.track_cursor, 1);
}

#[test]
fn view_chips_and_sidebar_rows_respond() {
  let mut app = library_with_songs();
  let s = draw(&mut app);
  let cmds = click(&mut app, find(&s, "Artistas"));
  assert_eq!(app.lib_view, LibView::Artists);
  assert_eq!(cmds, vec![("library.groups".to_string(), json!({"by": "artist"}))]);
  reply(&mut app, 4, "library.groups", json!({"groups": [{"name": "A", "count": 1}, {"name": "Zeta", "count": 2}]}));
  let s = draw(&mut app);
  let cmds = click(&mut app, find(&s, "Zeta"));
  assert_eq!(cmds, vec![("library.tracks".to_string(), json!({"by": "artist", "name": "Zeta", "limit": 5000}))]);
}

#[test]
fn popups_take_clicks_and_close_when_clicking_outside() {
  let mut app = playing();
  reply(&mut app, 5, "output.list", json!({"devices": [{"id": "a", "name": "Parlante", "is_default": true}, {"id": "b", "name": "Auriculares"}]}));
  app.on_key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE));
  let s = draw(&mut app);
  let cmds = click(&mut app, find(&s, "Auriculares"));
  assert_eq!(cmds, vec![("output.set".to_string(), json!({"id": "b"}))]);
  assert!(!app.devices_open);
  app.on_key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE));
  draw(&mut app);
  assert!(click(&mut app, (1, 42)).is_empty());
  assert!(!app.devices_open, "a click outside closes it");
  // Con un popup abierto, lo de atras no responde.
  app.on_key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE));
  draw(&mut app);
  assert!(!app.hits.iter().any(|(_, h)| matches!(h, ketai_tui::app::Hit::Tab(_))), "only the popup takes clicks");
}

#[test]
fn search_filters_and_results_respond() {
  let mut app = playing();
  app.on_key(KeyEvent::new(KeyCode::Char('4'), KeyModifiers::NONE));
  for c in "al".chars() {
    app.on_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
  }
  // La busqueda sale despues de una espera corta.
  app.tick(Instant::now() + Duration::from_millis(300));
  reply(&mut app, 9, "library.search", json!({"tracks": [
    {"uri": "file:///m/a.ogg", "title": "Alpha", "artist": "X", "duration_ms": 1000},
    {"uri": "file:///m/b.ogg", "title": "Alamo", "artist": "Y", "duration_ms": 1000}
  ]}));
  let s = draw(&mut app);
  click(&mut app, find(&s, "Canciones"));
  assert_eq!(app.search_filter, 1);
  assert!(!app.search_editing);
  let s = draw(&mut app);
  let at = find(&s, "Alamo");
  let t0 = Instant::now();
  app.on_mouse(mouse(MouseEventKind::Down(MouseButton::Left), at), t0);
  assert_eq!(app.search_cursor, 1);
  let cmds = app.on_mouse(mouse(MouseEventKind::Down(MouseButton::Left), at), t0 + Duration::from_millis(150));
  assert!(cmds.iter().any(|(m, p)| m == "queue.add" && p["uris"][0] == "file:///m/b.ogg"), "{cmds:?}");
}

fn at(app: &mut App, kind: MouseEventKind, pos: (u16, u16)) -> Vec<(String, Value)> {
  app.on_mouse(mouse(kind, pos), Instant::now())
}

#[test]
fn the_bottom_bar_status_is_clickable() {
  let mut app = library_with_songs();
  reply(&mut app, 10, "output.list", json!({"devices": [{"id": "o", "name": "QCY H3 Pro", "is_default": true}]}));
  let s = draw(&mut app);
  // El volumen silencia, la salida abre el popup, el corazon marca favorito.
  let (x, y) = find(&s, "50%");
  assert_eq!(click(&mut app, (x, y)), vec![("player.set_muted".to_string(), json!({"muted": true}))]);
  assert_eq!(click(&mut app, find(&s, "QCY H3 Pro")), Vec::<(String, Value)>::new());
  assert!(app.devices_open);
  app.devices_open = false;
  let s = draw(&mut app);
  let heart = find(&s, ui::I_HEART_EMPTY);
  assert_eq!(click(&mut app, heart), vec![("favorites.set".to_string(), json!({"uri": URI, "favorite": true}))]);
}

#[test]
fn the_view_arrows_and_the_now_playing_device_are_clickable() {
  let mut app = library_with_songs();
  let s = draw(&mut app);
  click(&mut app, find(&s, "›"));
  assert_eq!(app.lib_view, LibView::Artists);
  let s = draw(&mut app);
  click(&mut app, find(&s, "‹"));
  assert_eq!(app.lib_view, LibView::All);
  let mut app = playing();
  reply(&mut app, 10, "output.list", json!({"devices": [{"id": "o", "name": "QCY H3 Pro", "is_default": true}]}));
  let s = draw(&mut app);
  click(&mut app, find(&s, "QCY H3 Pro"));
  assert!(app.devices_open);
}

#[test]
fn the_wheel_works_anywhere_on_the_list() {
  let mut app = library_with_songs();
  let s = draw(&mut app);
  // En el encabezado de columnas, a la derecha, lejos de las filas.
  let (_, y) = find(&s, "ÁLBUM");
  at(&mut app, MouseEventKind::ScrollDown, (150, y));
  at(&mut app, MouseEventKind::ScrollDown, (150, y + 4));
  assert_eq!((app.lib_focus, app.track_cursor), (LibFocus::Tracks, 2));
}

#[test]
fn dragging_the_bars_moves_them() {
  let mut app = playing();
  let s = draw(&mut app);
  // Volumen: se manda mientras se arrastra.
  let (x, y) = find(&s, "50%");
  let start = x - 17;
  assert_eq!(at(&mut app, MouseEventKind::Down(MouseButton::Left), (start, y))[0].1, json!({"volume": 0.0}));
  let cmds = at(&mut app, MouseEventKind::Drag(MouseButton::Left), (x - 4, y));
  assert!(cmds[0].1["volume"].as_f64().unwrap() > 0.8, "{cmds:?}");
  // Aunque el puntero salga de la barra, sigue siendo el arrastre.
  let cmds = at(&mut app, MouseEventKind::Drag(MouseButton::Left), (x + 20, y + 3));
  assert_eq!(cmds[0].1, json!({"volume": 1.0}));
  assert!(at(&mut app, MouseEventKind::Up(MouseButton::Left), (x + 20, y + 3)).is_empty());
  // Progreso: se ve mientras se arrastra y se salta al soltar.
  let (p0, py) = find(&s, "0:00");
  let (p1, _) = find(&s, "3:20");
  assert!(at(&mut app, MouseEventKind::Down(MouseButton::Left), (p0 + 7, py)).is_empty());
  assert!(at(&mut app, MouseEventKind::Drag(MouseButton::Left), ((p0 + p1) / 2, py)).is_empty(), "no seek while dragging");
  let shown = app.position_ms();
  assert!((80_000..=120_000).contains(&shown), "the bar follows the pointer: {shown}");
  let cmds = at(&mut app, MouseEventKind::Up(MouseButton::Left), ((p0 + p1) / 2, py));
  assert_eq!(cmds[0].0, "player.seek");
  assert_eq!(cmds[0].1["position_ms"].as_u64().unwrap(), shown);
}

#[test]
fn every_popup_takes_clicks() {
  // Carpetas: un clic elige la carpeta.
  let mut app = playing();
  app.on_key(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::NONE));
  reply(&mut app, 11, "library.folders", json!({"folders": [{"path": "/m/uno", "tracks": 1}, {"path": "/m/dos", "tracks": 2}]}));
  let s = draw(&mut app);
  click(&mut app, find(&s, "/m/dos"));
  assert_eq!(app.folders_cursor, 1);
  assert!(app.folders_open);
  // Busqueda rapida: un clic ejecuta la opcion.
  let mut app = playing();
  app.on_key(KeyEvent::new(KeyCode::Char('p'), KeyModifiers::CONTROL));
  let s = draw(&mut app);
  click(&mut app, find(&s, "Carpetas"));
  assert!(!app.palette_open);
  assert!(app.folders_open, "the clicked action ran");
  // Agregar a playlist: fila y confirmacion.
  let mut app = playing();
  reply(&mut app, 12, "playlist.list", json!({"playlists": [{"name": "rock", "count": 1}]}));
  app.on_key(KeyEvent::new(KeyCode::Char('P'), KeyModifiers::NONE));
  let s = draw(&mut app);
  assert_eq!(click(&mut app, find(&s, "rock")), vec![("playlist.add".to_string(), json!({"name": "rock", "uris": [URI]}))]);
  app.sent(13, "playlist.add");
  app.on_daemon(DaemonMsg::Reply { id: 13, result: Err(json!({"code": 1009, "message": "x", "data": {"duplicates": [URI]}})) });
  let s = draw(&mut app);
  assert_eq!(
    click(&mut app, find(&s, "agregar de todos modos")),
    vec![("playlist.add".to_string(), json!({"name": "rock", "uris": [URI], "duplicates": "add"}))]
  );
  app.add_confirm = Some("rock".into());
  let s = draw(&mut app);
  assert!(click(&mut app, find(&s, "no agregar")).is_empty());
  assert!(!app.add_open);
}
