//! Pantalla Reproduciendo (maqueta 1-inicio-reproduciendo), controles y
//! canal del visualizador.

use std::{
  io::Write,
  os::unix::net::UnixListener,
  sync::mpsc,
  time::Duration,
};

use ketai_tui::{
  app::{App, fmt_ms},
  client::DaemonMsg,
  ui,
  viz,
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

fn playing_app() -> App {
  let mut app = App { connected: true, ..App::default() };
  app.on_daemon(DaemonMsg::Event {
    name: "player.state".into(),
    data: json!({
      "status": "paused", "position_ms": 84000, "volume": 0.7, "muted": false, "repeat": "queue", "shuffle": true,
      "track": {"id": "t_1", "title": "LET THE WORLD BURN", "artist": "Midnight Blu", "album": "Covers, Vol. 2",
                "duration_ms": 201000, "codec": "vorbis", "sample_rate": 44100, "uri": "file:///m/a.ogg"}
    }),
  });
  let id = 7;
  app.sent(id, "queue.list");
  app.on_daemon(DaemonMsg::Reply {
    id,
    result: Ok(json!({"current": 0, "tracks": [
      {"id": "t_1", "title": "LET THE WORLD BURN", "artist": "Midnight Blu"},
      {"id": "t_2", "title": "My Jealousy", "artist": "Tokai Teio"}
    ]})),
  });
  app
}

fn text(app: &App) -> String {
  let mut term = Terminal::new(TestBackend::new(160, 44)).unwrap();
  term.draw(|f| ui::draw(f, app)).unwrap();
  let buf = term.backend().buffer();
  (0..buf.area.height).map(|y| (0..buf.area.width).map(|x| buf[(x, y)].symbol().to_string()).collect::<String>() + "\n").collect()
}

#[test]
fn shows_the_track_progress_next_and_audio_details() {
  let t = text(&playing_app());
  for want in ["LET THE WORLD BURN", "Midnight Blu", "Covers, Vol. 2", "1:24", "3:21", "Siguiente", "My Jealousy", "70%"] {
    assert!(t.contains(want), "missing {want:?}\n{t}");
  }
  assert!(!t.contains("vorbis") && !t.contains("kHz"), "the audio format is not shown");
}

#[test]
fn nothing_playing_says_so() {
  let t = text(&App { connected: true, ..App::default() });
  assert!(t.contains("Nada en reproducción"), "{t}");
}

#[test]
fn formats_times() {
  assert_eq!(fmt_ms(0), "0:00");
  assert_eq!(fmt_ms(84_000), "1:24");
  assert_eq!(fmt_ms(3_725_000), "1:02:05");
}

#[test]
fn control_keys_become_requests() {
  let mut app = playing_app();
  assert_eq!(app.on_key(key(KeyCode::Right)), vec![("player.seek_relative".into(), json!({"offset_ms": 5000}))]);
  assert_eq!(app.on_key(key(KeyCode::Left)), vec![("player.seek_relative".into(), json!({"offset_ms": -5000}))]);
  assert_eq!(app.on_key(key(KeyCode::Char('+'))), vec![("player.set_volume".into(), json!({"volume": 0.75}))]);
  assert_eq!(app.on_key(key(KeyCode::Char('-'))), vec![("player.set_volume".into(), json!({"volume": 0.65}))]);
  assert_eq!(app.on_key(key(KeyCode::Char('m'))), vec![("player.set_muted".into(), json!({"muted": true}))]);
  assert_eq!(app.on_key(key(KeyCode::Char('s'))), vec![("player.set_shuffle".into(), json!({"shuffle": false}))]);
  // queue -> track -> off -> queue
  assert_eq!(app.on_key(key(KeyCode::Char('r'))), vec![("player.set_repeat".into(), json!({"repeat": "off"}))]);
}

#[test]
fn queue_changes_ask_for_the_queue_again() {
  let mut app = playing_app();
  let cmds = app.on_daemon(DaemonMsg::Event { name: "queue.changed".into(), data: json!({}) });
  assert_eq!(cmds, vec![("queue.list".to_string(), Value::Object(Default::default()))]);
}

#[test]
fn spectrum_frames_draw_bars_and_stop_when_paused() {
  let mut app = playing_app();
  app.player["status"] = json!("playing");
  app.on_viz(vec![1.0; 64]);
  let full = text(&app);
  app.player["status"] = json!("paused");
  app.on_viz(vec![1.0; 64]);
  let paused = text(&app);
  let bars = |t: &str| t.matches('█').count();
  assert!(bars(&full) > 64 * 4, "playing spectrum should be visible");
  assert_eq!(bars(&paused), 0, "no spectrum while paused");
}

#[test]
fn viz_reader_parses_header_and_frames() {
  let dir = std::env::temp_dir().join(format!("ketai-tui-viz-{}", std::process::id()));
  std::fs::create_dir_all(&dir).unwrap();
  let path = dir.join("viz.sock");
  let _ = std::fs::remove_file(&path);
  let listener = UnixListener::bind(&path).unwrap();
  let server = std::thread::spawn(move || {
    let (mut s, _) = listener.accept().unwrap();
    let mut h = b"KTV1".to_vec();
    h.extend(4u32.to_le_bytes());
    h.extend(60u32.to_le_bytes());
    s.write_all(&h).unwrap();
    for v in [0.25f32, 0.5, 0.75, 1.0] {
      s.write_all(&v.to_le_bytes()).unwrap();
    }
  });
  let (tx, rx) = mpsc::channel();
  viz::spawn(&path, tx, |f| f).unwrap();
  let frame = rx.recv_timeout(Duration::from_secs(3)).unwrap();
  assert_eq!(frame, vec![0.25, 0.5, 0.75, 1.0]);
  server.join().unwrap();
  let _ = std::fs::remove_dir_all(dir);
}

fn text_at(app: &App, w: u16, h: u16) -> String {
  let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
  term.draw(|f| ui::draw(f, app)).unwrap();
  let buf = term.backend().buffer();
  (0..buf.area.height).map(|y| (0..buf.area.width).map(|x| buf[(x, y)].symbol().to_string()).collect::<String>() + "\n").collect()
}

#[test]
fn a_4_3_pane_shows_everything() {
  // Un panel de herdr de 4:3 (unas 88 columnas por 40 filas).
  for (w, h) in [(88, 40), (100, 34), (76, 28)] {
    let t = text_at(&playing_app(), w, h);
    for want in ["LET THE WORLD BURN", "Midnight Blu", "1:24", "3:21", ui::I_SHUFFLE, ui::I_PREV, ui::I_NEXT, ui::I_REPEAT, "70%"] {
      assert!(t.contains(want), "{w}x{h} missing {want:?}\n{t}");
    }
  }
}

#[test]
fn the_play_button_has_the_same_padding_on_both_sides() {
  for (w, h) in [(160, 44), (88, 40)] {
    let app = playing_app();
    let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
    term.draw(|f| ui::draw(f, &app)).unwrap();
    let buf = term.backend().buffer();
    let (x, y) = (0..h).flat_map(|y| (0..w).map(move |x| (x, y))).find(|&(x, y)| buf[(x, y)].symbol() == ui::I_PLAY).unwrap();
    let bg = buf[(x, y)].bg;
    let left = (0..x).rev().take_while(|&i| buf[(i, y)].bg == bg).count();
    let right = (x + 1..w).take_while(|&i| buf[(i, y)].bg == bg).count();
    assert_eq!(left, right, "{w}x{h}");
  }
}
