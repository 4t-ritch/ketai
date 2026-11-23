//! Texto largo que no entra: se desplaza para verlo completo.

use ketai_tui::marquee::{PAUSE_MS, STEP_MS, visible};
use unicode_width::UnicodeWidthStr;

#[test]
fn text_that_fits_never_moves() {
  for t in [0, 5_000, 99_999] {
    assert_eq!(visible("Magnetic", 20, t), "Magnetic");
  }
}

#[test]
fn long_text_waits_then_scrolls_one_char_at_a_time_and_loops() {
  let text = "LET THE WORLD BURN (Midnight Blu Cover)";
  // Quieto al principio.
  assert_eq!(visible(text, 10, 0), "LET THE WO");
  assert_eq!(visible(text, 10, PAUSE_MS - 1), "LET THE WO");
  // Despues avanza un caracter por paso.
  assert_eq!(visible(text, 10, PAUSE_MS + STEP_MS), "ET THE WOR");
  assert_eq!(visible(text, 10, PAUSE_MS + 3 * STEP_MS), " THE WORLD");
  // Al final muestra el separador y vuelve a empezar.
  let len = text.chars().count() as u64 + 5;
  let near_end = visible(text, 10, PAUSE_MS + (len - 3) * STEP_MS);
  assert!(near_end.contains('•') || near_end.starts_with(' '), "{near_end:?}");
  assert_eq!(visible(text, 10, PAUSE_MS + len * STEP_MS), "LET THE WO", "a new cycle starts still");
}

#[test]
fn wide_characters_are_never_cut_in_half() {
  let text = "kiss of life (키스오브라이프) - sugarcoat (natty solo)";
  for step in 0..80 {
    let v = visible(text, 12, PAUSE_MS + step * STEP_MS);
    assert!(v.width() <= 12, "step {step}: {v:?} is {} wide", v.width());
    assert!(v.width() >= 11, "step {step}: {v:?} fills the space");
  }
}

use ketai_tui::{app::App, client::DaemonMsg, ui};
use ratatui::{Terminal, backend::TestBackend};
use serde_json::json;

fn screen(app: &App) -> String {
  let mut term = Terminal::new(TestBackend::new(100, 30)).unwrap();
  term.draw(|f| ui::draw(f, app)).unwrap();
  let buf = term.backend().buffer();
  (0..30).map(|y| (0..100).map(|x| buf[(x, y)].symbol().to_string()).collect::<String>() + "\n").collect()
}

fn playing(title: &str) -> App {
  let mut app = App { connected: true, ..App::default() };
  app.on_daemon(DaemonMsg::Event {
    name: "player.state".into(),
    data: json!({"status": "playing", "position_ms": 0, "volume": 0.5,
      "track": {"id": "t", "uri": "file:///m/a.ogg", "title": title, "artist": "A", "duration_ms": 1000}}),
  });
  app
}

#[test]
fn now_playing_scrolls_a_title_that_does_not_fit() {
  let long = "ANIMATRONICOS RAP Five Nights at Freddys ZARCORT KRONNO NERY ITOWN FNAF audio";
  let mut app = playing(long);
  app.anim_ms = 0;
  let before = screen(&app);
  assert!(before.contains("ANIMATRONICOS RAP"), "{before}");
  app.anim_ms = PAUSE_MS + 14 * STEP_MS;
  let after = screen(&app);
  assert!(after.contains("RAP Five Nights") && !after.contains("ANIMATRONICOS RAP"), "{after}");
  // Uno que entra no se mueve.
  let mut app = playing("Magnetic");
  app.anim_ms = PAUSE_MS + 14 * STEP_MS;
  assert!(screen(&app).contains("Magnetic"));
}
