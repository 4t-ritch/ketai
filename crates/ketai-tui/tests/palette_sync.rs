//! Con una caratula roja, ninguna vista, popup, texto ni icono puede quedar
//! con los colores azulados por defecto: todo sigue a la caratula.

use ketai_tui::{app::App, client::DaemonMsg, config::Config, ui};
use ratatui::{
  Terminal,
  backend::TestBackend,
  crossterm::event::{KeyCode, KeyEvent, KeyModifiers},
  style::Color,
};
use serde_json::json;

const URI: &str = "file:///m/a.ogg";

fn red_app(config: &str) -> App {
  let mut app = App { connected: true, ..App::default() };
  app.config = Config::parse(config).unwrap();
  app.on_daemon(DaemonMsg::Event {
    name: "player.state".into(),
    data: json!({"status": "playing", "position_ms": 1000, "volume": 0.5, "output": "o",
      "track": {"id": "t", "uri": URI, "title": "Rojo", "artist": "A", "album": "B", "duration_ms": 200000, "codec": "vorbis", "sample_rate": 48000}}),
  });
  app.covers.want(URI);
  app.covers.sent(1, URI);
  app.covers.on_reply(1, Ok(json!({"path": "/x.png"})));
  app.covers.decoded(URI, Some(image::RgbImage::from_pixel(8, 8, image::Rgb([200, 30, 30]))));
  app.refresh_theme();
  for (id, m, v) in [
    (2, "queue.list", json!({"current": 0, "tracks": [{"title": "Rojo"}, {"title": "Otra", "artist": "X"}]})),
    (3, "playlist.list", json!({"playlists": [{"name": "rock", "count": 1}]})),
    (4, "output.list", json!({"devices": [{"id": "o", "name": "Parlante", "is_default": true}]})),
    (5, "track.playlists", json!({"playlists": ["rock"], "favorite": true})),
    (6, "lyrics.get", json!({"lines": [{"start_ms": 0, "text": "uno"}, {"start_ms": 5000, "text": "dos"}, {"start_ms": 9000, "text": "tres"}]})),
  ] {
    app.sent(id, m);
    app.on_daemon(DaemonMsg::Reply { id, result: Ok(v) });
  }
  app.viz = vec![0.6; 32];
  app
}

/// Un color azulado: el azul le gana claramente al rojo.
fn bluish(c: Color) -> bool {
  matches!(c, Color::Rgb(r, _, b) if b as i32 > r as i32 + 15)
}

fn check(app: &App, what: &str) {
  for (w, h) in [(160, 44), (88, 36)] {
    let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
    term.draw(|f| ui::draw(f, app)).unwrap();
    let buf = term.backend().buffer();
    for y in 0..h {
      for x in 0..w {
        let c = &buf[(x, y)];
        assert!(!bluish(c.fg) && !bluish(c.bg), "{what} {w}x{h}: cell ({x},{y}) {:?} fg {:?} bg {:?}", c.symbol(), c.fg, c.bg);
      }
    }
  }
}

fn key(c: KeyCode) -> KeyEvent {
  KeyEvent::new(c, KeyModifiers::NONE)
}

fn every_screen(config: &str) {
  for tab in ['1', '2', '3', '4'] {
    let mut app = red_app(config);
    app.on_key(key(KeyCode::Char(tab)));
    app.search_editing = false;
    check(&app, &format!("tab {tab}"));
    for popup in ['d', 'f', 'P'] {
      let mut a = red_app(config);
      a.on_key(key(KeyCode::Char(tab)));
      a.search_editing = false;
      a.on_key(key(KeyCode::Char(popup)));
      check(&a, &format!("tab {tab} popup {popup}"));
    }
    let mut a = red_app(config);
    a.on_key(key(KeyCode::Char(tab)));
    a.on_key(KeyEvent::new(KeyCode::Char('k'), KeyModifiers::CONTROL));
    check(&a, &format!("tab {tab} palette"));
  }
}

#[test]
fn cover_color_true_leaves_nothing_with_default_colors() {
  every_screen("[theme]\ncover_color = true\n");
}

#[test]
fn background_and_accent_from_the_cover_also_tint_the_rest() {
  every_screen("[theme]\nbackground = \"cover\"\naccent = \"cover\"\n");
}

fn check_at(app: &App, what: &str, w: u16, h: u16) {
  let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
  term.draw(|f| ui::draw(f, app)).unwrap();
  let buf = term.backend().buffer();
  for y in 0..h {
    for x in 0..w {
      let c = &buf[(x, y)];
      assert!(!bluish(c.fg) && !bluish(c.bg), "{what} {w}x{h}: cell ({x},{y}) {:?} fg {:?} bg {:?}", c.symbol(), c.fg, c.bg);
    }
  }
}

fn reply(app: &mut App, id: u64, m: &str, v: serde_json::Value) {
  app.sent(id, m);
  app.on_daemon(DaemonMsg::Reply { id, result: Ok(v) });
}

#[test]
fn the_rest_of_the_screens_follow_the_cover_too() {
  let cfg = "[theme]\ncover_color = true\n";
  // Modo mini.
  check_at(&red_app(cfg), "mini", 60, 12);
  // Buscar con resultados.
  let mut a = red_app(cfg);
  a.on_key(key(KeyCode::Char('4')));
  for c in "ro".chars() {
    a.on_key(key(KeyCode::Char(c)));
  }
  reply(&mut a, 20, "library.search", json!({"tracks": [{"uri": URI, "title": "Rojo", "artist": "A", "album": "B", "duration_ms": 1000}]}));
  check(&a, "search results");
  // Biblioteca por grupos, con la tabla.
  let mut a = red_app(cfg);
  a.on_key(key(KeyCode::Char('3')));
  a.on_key(key(KeyCode::Char('l')));
  a.on_key(key(KeyCode::Char('l')));
  reply(&mut a, 21, "library.groups", json!({"groups": [{"name": null, "count": 1}, {"name": "A", "count": 1}]}));
  reply(&mut a, 22, "library.tracks", json!({"total": 1, "tracks": [{"uri": URI, "title": "Rojo", "artist": "A", "album": "B", "year": 2024, "duration_ms": 1000}]}));
  a.on_key(key(KeyCode::Enter));
  check(&a, "library groups");
  // Nombre de una playlist nueva y confirmacion de repetida.
  let mut a = red_app(cfg);
  a.on_key(key(KeyCode::Char('P')));
  a.on_key(key(KeyCode::Char('c')));
  a.on_key(key(KeyCode::Char('x')));
  check(&a, "naming");
  let mut a = red_app(cfg);
  a.on_key(key(KeyCode::Char('P')));
  a.on_key(key(KeyCode::Enter));
  a.sent(30, "playlist.add");
  a.on_daemon(DaemonMsg::Reply { id: 30, result: Err(json!({"code": 1009, "message": "x", "data": {"duplicates": [URI]}})) });
  check(&a, "confirm");
  // Aviso abajo.
  let mut a = red_app(cfg);
  a.notice = Some("Agregada a «rock»".into());
  check(&a, "notice");
}

#[test]
fn with_cover_color_false_only_the_manual_colors_are_used() {
  let cfg = "[theme]\ncover_color = false\nbackground = \"#0a1a0a\"\naccent = \"#40ff40\"\nforeground = \"#e0ffe0\"\ndim = \"#a0d0a0\"\nfaint = \"#709070\"\nraised = \"#1a301a\"\non_accent = \"#002000\"\nrow = \"#143014\"\nok = \"#80ff80\"\nerror = \"#a0ff60\"\nspectrum_top = \"#c0ffc0\"\n";
  let reddish = |c: Color| matches!(c, Color::Rgb(r, g, _) if r as i32 > g as i32 + 15);
  for tab in ['1', '2', '3', '4'] {
    for popup in [None, Some('d'), Some('f'), Some('P')] {
      let mut a = red_app(cfg);
      a.on_key(key(KeyCode::Char(tab)));
      a.search_editing = false;
      if let Some(p) = popup {
        a.on_key(key(KeyCode::Char(p)));
      }
      let mut term = Terminal::new(TestBackend::new(160, 44)).unwrap();
      term.draw(|f| ui::draw(f, &a)).unwrap();
      let buf = term.backend().buffer();
      for y in 0..44 {
        for x in 0..160 {
          let c = &buf[(x, y)];
          let bad = |col: Color| bluish(col) || reddish(col);
          assert!(!bad(c.fg) && !bad(c.bg), "tab {tab} {popup:?}: ({x},{y}) {:?} fg {:?} bg {:?}", c.symbol(), c.fg, c.bg);
        }
      }
    }
  }
}
