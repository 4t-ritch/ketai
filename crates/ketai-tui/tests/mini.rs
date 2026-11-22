//! Modo mini (maqueta 7-mini): panel chico de herdr o tmux.

use ketai_tui::{app::App, client::DaemonMsg, ui};
use ratatui::{Terminal, backend::TestBackend};
use serde_json::json;

fn app() -> App {
  let mut app = App { connected: true, ..App::default() };
  app.on_daemon(DaemonMsg::Event {
    name: "player.state".into(),
    data: json!({"status": "playing", "position_ms": 84000, "volume": 0.7, "repeat": "off", "shuffle": false,
                 "track": {"id": "t_1", "title": "LET THE WORLD BURN", "artist": "Midnight Blu", "album": "Covers, Vol. 2", "duration_ms": 201000}}),
  });
  app.lyrics = json!({"user_offset_ms": 0, "lines": [{"start_ms": 0, "text": "just to feel something", "words": null, "secondary": {}}]});
  app.on_viz(vec![0.8; 64]);
  app
}

fn text(app: &App, w: u16, h: u16) -> String {
  let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
  term.draw(|f| ui::draw(f, app)).unwrap();
  let buf = term.backend().buffer();
  (0..buf.area.height).map(|y| (0..buf.area.width).map(|x| buf[(x, y)].symbol().to_string()).collect::<String>() + "\n").collect()
}

#[test]
fn a_small_pane_gets_the_compact_view() {
  let t = text(&app(), 80, 10);
  for want in ["LET THE WORLD BURN", "Midnight Blu · Covers, Vol. 2", "▶ just to feel something", "1:", "3:21", "70%"] {
    assert!(t.contains(want), "missing {want:?}\n{t}");
  }
  assert!(!t.contains("Biblioteca"), "no tabs in mini mode\n{t}");
  assert!(t.contains('█') || t.contains('▇'), "small spectrum\n{t}");
}

#[test]
fn a_big_terminal_keeps_the_full_view() {
  assert!(text(&app(), 160, 44).contains("Biblioteca"));
}

/// Todas las pantallas y popups, en muchos tamanos, sin fallar.
#[test]
fn every_screen_survives_any_size() {
  use ketai_tui::app::Tab;
  use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
  let mut base = app();
  base.queue = json!({"current": 0, "tracks": [{"title": "a"}, {"title": "b"}]});
  base.playlists = vec![json!({"name": "metal", "count": 3}), json!({"name": "chill", "count": 1})];
  base.lib_open = Some("metal".into());
  base.playlist = json!({"name": "metal", "tracks": [{"uri": "file:///a.ogg", "title": "a"}, {"uri": "file:///b.ogg", "title": "b"}]});
  base.search_query = "a".into();
  base.search_done = true;
  base.search_results = vec![json!({"uri": "file:///a.ogg", "title": "a", "artist": "x"})];
  base.outputs = vec![json!({"id": "bluez_output.x", "name": "Auriculares", "is_default": true})];
  for (w, h) in [(72, 15), (73, 16), (80, 20), (90, 18), (100, 25), (120, 24), (140, 35), (160, 44), (250, 70)] {
    for tab in Tab::ALL {
      for overlay in 0..3 {
        let mut a = App { tab, ..app() };
        a.queue = base.queue.clone();
        a.playlists = base.playlists.clone();
        a.lib_open = base.lib_open.clone();
        a.playlist = base.playlist.clone();
        a.search_query = base.search_query.clone();
        a.search_done = true;
        a.search_results = base.search_results.clone();
        a.outputs = base.outputs.clone();
        match overlay {
          1 => {
            a.tab = Tab::NowPlaying;
            a.on_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
            a.tab = tab;
          }
          2 => a.devices_open = true,
          _ => {}
        }
        let _ = text(&a, w, h);
      }
    }
  }
}

#[test]
fn tiny_sizes_never_crash() {
  for (w, h) in [(1, 1), (20, 3), (40, 6), (71, 30), (200, 14), (72, 15), (90, 18), (120, 24)] {
    let _ = text(&app(), w, h);
  }
}

#[test]
fn the_service_screen_survives_any_size() {
  use ketai_tui::{app::Screen, service::{Observed, State}};
  for state in [State::Starting, State::Stopped { enabled: false }, State::NotInstalled, State::Unresponsive { pid: 7 }, State::NoSystemd] {
    for (w, h) in [(1, 1), (20, 5), (40, 10), (72, 15), (160, 44)] {
      let a = App { screen: Screen::Service { state, observed: Observed::default(), message: Some("error largo de systemd".into()), busy: false }, ..App::default() };
      let _ = text(&a, w, h);
    }
  }
}
