//! Caratulas en la UI: donde se ubican, cuales faltan pedir y como quedan
//! bajo un popup.

use ketai_tui::{app::App, client::DaemonMsg, ui};
use ratatui::{
  Terminal,
  backend::TestBackend,
  crossterm::event::{KeyCode, KeyEvent, KeyModifiers},
  style::Color,
};
use serde_json::json;

fn playing() -> App {
  let mut app = App { connected: true, images: true, ..App::default() };
  app.on_daemon(DaemonMsg::Event {
    name: "player.state".into(),
    data: json!({"status": "paused", "position_ms": 0, "track": {"id": "t_1", "uri": "file:///m/a.ogg", "title": "A", "duration_ms": 1000}}),
  });
  app
}

fn draw(app: &App) -> (ratatui::buffer::Buffer, ui::Frame) {
  let mut term = Terminal::new(TestBackend::new(160, 44)).unwrap();
  term.draw(|f| ui::draw(f, app)).unwrap();
  (term.backend().buffer().clone(), ui::take_frame(app))
}

#[test]
fn unknown_covers_are_requested_and_shown_as_a_placeholder() {
  let app = playing();
  let (_, frame) = draw(&app);
  assert_eq!(frame.wanted, vec!["file:///m/a.ogg".to_string()]);
  assert!(frame.placements.is_empty());
}

#[test]
fn ready_covers_are_placed_over_cells_with_the_default_background() {
  let mut app = playing();
  app.covers.want("file:///m/a.ogg");
  app.covers.sent(1, "file:///m/a.ogg");
  app.covers.on_reply(1, Ok(json!({"path": "/x.png"})));
  app.covers.decoded("file:///m/a.ogg", Some(image::RgbImage::new(4, 4)));
  let normal = app.covers.image_for("file:///m/a.ogg", false).unwrap();
  let (buf, frame) = draw(&app);
  assert!(frame.wanted.is_empty());
  let big = frame.placements.iter().find(|p| p.area.width >= 20).expect("the big cover");
  assert_eq!(big.image, normal);
  let cell = &buf[(big.area.x + 1, big.area.y + 1)];
  assert_eq!(cell.bg, Color::Reset, "the image shows only where the background is the default");
}

#[test]
fn covers_behind_a_popup_use_the_dimmed_image_once_it_is_ready() {
  let mut app = playing();
  app.covers.want("file:///m/a.ogg");
  app.covers.sent(1, "file:///m/a.ogg");
  app.covers.on_reply(1, Ok(json!({"path": "/x.png"})));
  app.covers.decoded("file:///m/a.ogg", Some(image::RgbImage::new(4, 4)));
  app.on_key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE));
  // Primero se pide la version atenuada; mientras tanto no se ubica.
  let (_, frame) = draw(&app);
  assert_eq!(frame.wanted_dimmed, vec!["file:///m/a.ogg".to_string()]);
  assert!(frame.placements.is_empty());
  let (_, id) = app.covers.want_dimmed("file:///m/a.ogg").unwrap();
  app.covers.dimmed_ready("file:///m/a.ogg");
  let (_, frame) = draw(&app);
  assert!(!frame.placements.is_empty());
  assert!(frame.placements.iter().all(|p| p.image == id), "{:?}", frame.placements);
}

#[test]
fn without_images_nothing_is_placed_or_requested() {
  let app = App { images: false, ..playing() };
  let (_, frame) = draw(&app);
  assert!(frame.wanted.is_empty() && frame.placements.is_empty());
}

#[test]
fn a_stale_cover_on_screen_asks_to_be_sent_again() {
  let mut app = playing();
  app.covers.want("file:///m/a.ogg");
  app.covers.sent(1, "file:///m/a.ogg");
  app.covers.on_reply(1, Ok(json!({"path": "/x.png"})));
  app.covers.decoded("file:///m/a.ogg", Some(image::RgbImage::new(4, 4)));
  app.covers.invalidate_all();
  let (_, frame) = draw(&app);
  assert!(frame.placements.is_empty());
  assert_eq!(frame.resend, vec!["file:///m/a.ogg".to_string()]);
  assert!(frame.wanted.is_empty(), "not asked to the daemon again");
}

fn queue(app: &mut App, current: u64) {
  app.sent(50, "queue.list");
  app.on_daemon(DaemonMsg::Reply {
    id: 50,
    result: Ok(json!({"current": current, "tracks": [
      {"uri": "file:///m/a.ogg", "title": "A"}, {"uri": "file:///m/b.ogg", "title": "B"}, {"uri": "file:///m/c.ogg", "title": "C"}
    ]})),
  });
}

fn ready(app: &mut App, uri: &str, rgb: [u8; 3]) {
  app.covers.want(uri);
  app.covers.sent(60, uri);
  app.covers.on_reply(60, Ok(json!({"path": "/x.png"})));
  app.covers.decoded(uri, Some(image::RgbImage::from_pixel(8, 8, image::Rgb(rgb))));
}

#[test]
fn the_next_track_cover_is_prepared_ahead_and_only_that_one() {
  let mut app = playing();
  queue(&mut app, 0);
  let (_, frame) = draw(&app);
  assert!(frame.wanted.contains(&"file:///m/b.ogg".to_string()), "{:?}", frame.wanted);
  assert!(!frame.wanted.contains(&"file:///m/c.ogg".to_string()), "never more than the next one: {:?}", frame.wanted);
  assert!(frame.keep.contains(&"file:///m/b.ogg".to_string()), "kept in memory until it plays");
}

#[test]
fn moving_to_the_next_track_changes_the_colors_at_once() {
  let mut app = playing();
  queue(&mut app, 0);
  ready(&mut app, "file:///m/a.ogg", [200, 30, 30]);
  ready(&mut app, "file:///m/b.ogg", [30, 30, 200]);
  app.refresh_theme();
  let red = app.theme.accent;
  // Llega el cambio de pista: sin esperar a nadie, el tema ya es el azul.
  app.on_daemon(DaemonMsg::Event {
    name: "player.state".into(),
    data: json!({"status": "playing", "position_ms": 0, "track": {"id": "t_2", "uri": "file:///m/b.ogg", "title": "B", "duration_ms": 1000}}),
  });
  let ratatui::style::Color::Rgb(r, _, b) = app.theme.accent else { panic!() };
  assert!(b > r && app.theme.accent != red, "{:?}", app.theme.accent);
}
