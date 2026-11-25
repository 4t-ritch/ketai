//! Atajos configurables (`[keys]` de config.toml) y Ctrl+P para acciones.

use ketai_tui::{app::App, config::Config};
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use serde_json::json;

fn key(c: KeyCode) -> KeyEvent {
  KeyEvent::new(c, KeyModifiers::NONE)
}

fn ctrl(c: char) -> KeyEvent {
  KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
}

#[test]
fn ctrl_p_opens_the_actions_popup_by_default() {
  let mut app = App { connected: true, ..App::default() };
  app.on_key(ctrl('p'));
  assert!(app.palette_open);
  // Solo Ctrl+P: ni Ctrl+K ni "/" la abren.
  for k in [ctrl('k'), key(KeyCode::Char('/'))] {
    let mut app = App { connected: true, ..App::default() };
    app.on_key(k);
    assert!(!app.palette_open);
  }
}

#[test]
fn keys_from_the_config_replace_the_defaults() {
  let mut app = App { connected: true, ..App::default() };
  app.config = Config::parse("[keys]\ntoggle = \"y\"\nnext = [\"ctrl+right\", \"N\"]\nactions = \"ctrl+o\"\n").unwrap();
  assert_eq!(app.on_key(key(KeyCode::Char('y'))), vec![("player.toggle".to_string(), json!({}))]);
  assert!(app.on_key(key(KeyCode::Char(' '))).is_empty(), "space no longer toggles");
  assert_eq!(app.on_key(KeyEvent::new(KeyCode::Right, KeyModifiers::CONTROL)), vec![("player.next".to_string(), json!({}))]);
  assert_eq!(app.on_key(KeyEvent::new(KeyCode::Char('N'), KeyModifiers::SHIFT)), vec![("player.next".to_string(), json!({}))]);
  // Lo no configurado queda igual.
  assert_eq!(app.on_key(key(KeyCode::Char('p'))), vec![("player.previous".to_string(), json!({}))]);
  app.on_key(ctrl('p'));
  assert!(!app.palette_open, "ctrl+p was replaced");
  app.on_key(ctrl('o'));
  assert!(app.palette_open);
}

#[test]
fn ctrl_c_always_quits() {
  let mut app = App { connected: true, ..App::default() };
  app.config = Config::parse("[keys]\nquit = \"ctrl+q\"\n").unwrap();
  app.on_key(key(KeyCode::Char('q')));
  assert!(!app.quit);
  app.on_key(ctrl('c'));
  assert!(app.quit);
}

#[test]
fn while_typing_in_search_plain_letters_are_text() {
  let mut app = App { connected: true, ..App::default() };
  app.config = Config::parse("[keys]\ntoggle = \"y\"\nactions = \"ctrl+p\"\n").unwrap();
  app.on_key(key(KeyCode::Char('4')));
  assert!(app.on_key(key(KeyCode::Char('y'))).is_empty());
  assert_eq!(app.search_query, "y");
  app.on_key(ctrl('p'));
  assert!(app.palette_open, "keys with ctrl still work while typing");
}

#[test]
fn bad_keys_are_reported_and_unknown_actions_warned() {
  let err = Config::parse("[keys]\ntoggle = \"ctrl+banana\"\n").unwrap_err();
  assert!(err.contains("keys.toggle") && err.contains("ctrl+banana"), "{err}");
  let c = Config::parse("[keys]\ndance = \"x\"\n").unwrap();
  assert!(c.warnings.iter().any(|w| w.contains("keys.dance")), "{:?}", c.warnings);
  let c = Config::parse("[keys]\nnext = \"m\"\n").unwrap();
  assert!(c.warnings.iter().any(|w| w.contains("next") && w.contains("mute")), "{:?}", c.warnings);
}

#[test]
fn now_playing_shows_the_configured_keys() {
  use ratatui::{Terminal, backend::TestBackend};
  let mut app = App { connected: true, ..App::default() };
  app.config = Config::parse("[keys]\nfavorite = \"ctrl+f\"\nadd_to_playlist = \"alt+a\"\n").unwrap();
  app.on_daemon(ketai_tui::client::DaemonMsg::Event {
    name: "player.state".into(),
    data: json!({"status": "paused", "volume": 0.5, "track": {"id": "t", "uri": "file:///a.ogg", "title": "x", "duration_ms": 1000}}),
  });
  let mut term = Terminal::new(TestBackend::new(160, 44)).unwrap();
  term.draw(|f| ketai_tui::ui::draw(f, &app)).unwrap();
  let buf = term.backend().buffer();
  let t: String = (0..44).map(|y| (0..160).map(|x| buf[(x, y)].symbol().to_string()).collect::<String>() + "\n").collect();
  assert!(t.contains(" Ctrl+F ") && t.contains(" Alt+A "), "{t}");
}

#[test]
fn a_lyrics_key_cannot_repeat_a_global_one() {
  use ketai_tui::config::Config;
  // Por defecto no hay repetidas: `m` silencia y `k` cambia el modo.
  assert!(Config::parse("").unwrap().warnings.is_empty());
  // Una de Letras con la tecla de una global taparia la global en Letras.
  let c = Config::parse("[keys]\nlyrics_mode = \"m\"").unwrap();
  assert_eq!(c.warnings, ["keys: m está en mute y lyrics_mode"]);
  assert_eq!(c.keys.duplicates().len(), 1);
}

#[test]
fn duplicates_are_grouped_by_key() {
  use ketai_tui::config::Config;
  use ketai_tui::keys::name;
  assert!(Config::parse("").unwrap().keys.duplicates().is_empty(), "the defaults have none");
  let c = Config::parse("[keys]\nnext = \"x\"\nprevious = \"x\"\nshuffle = \"x\"\nrepeat = \"z\"\nmute = \"z\"\nlyrics_layer = \"t\"\nlyrics_estimate = \"d\"").unwrap();
  let got: Vec<(String, Vec<&str>)> = c.keys.duplicates().into_iter().map(|(k, a)| (k.label(), a.into_iter().map(name).collect())).collect();
  // Una tecla en tres acciones es un solo grupo; grupos y acciones van en
  // el orden de la tabla.
  assert_eq!(
    got,
    vec![
      // La de la cola (una vista) con globales tambien choca.
      ("x".to_string(), vec!["next", "previous", "shuffle", "queue_remove"]),
      ("z".to_string(), vec!["mute", "repeat"]),
      // Una de Letras con una global tambien.
      ("d".to_string(), vec!["devices", "lyrics_estimate"]),
      ("t".to_string(), vec!["lyrics_layer", "lyrics_translation"]),
    ]
  );
}

#[test]
fn the_same_key_is_valid_in_different_views_but_not_with_a_global_one() {
  use ketai_tui::keys::Scope::{Global, Lyrics};
  assert!(Global.clashes_with(Global));
  assert!(Global.clashes_with(Lyrics) && Lyrics.clashes_with(Global), "a view key would hide a global one");
  assert!(Lyrics.clashes_with(Lyrics), "twice in the same view");
}

#[test]
fn shift_is_only_accepted_where_the_terminal_tells_it_apart() {
  use ketai_tui::keys::Key;
  // Valen: letras (sin Ctrl), Alt+Shift+letra y Shift+Tab.
  assert_eq!(Key::parse("shift+p"), Key::parse("P"));
  assert!(Key::parse("alt+shift+p").is_some_and(|k| k.label() == "Alt+P"));
  assert!(Key::parse("shift+tab").is_some_and(|k| k.label() == "Shift+Tab"));
  // No valen: la terminal no manda Shift ahi o manda otro caracter.
  for bad in ["shift+left", "shift+enter", "shift+f5", "shift+1", "shift+/", "ctrl+shift+p", "shift+space"] {
    assert_eq!(Key::parse(bad), None, "{bad}");
  }
  // Y el error se muestra con la tecla, como cualquier tecla invalida.
  assert_eq!(
    ketai_tui::config::Config::parse("[keys]\nnext = \"shift+left\"").unwrap_err(),
    "keys.next: tecla inválida \"shift+left\""
  );
}

#[test]
fn what_the_key_format_accepts_and_rejects() {
  use ketai_tui::keys::Key;
  // Aceptadas (lo que documenta docs/config.md).
  for ok in ["n", "P", "+", "*", "!", "ctrl++", "space", "enter", "esc", "escape", "tab", "backspace", "left", "right", "up", "down",
             "home", "end", "pageup", "pagedown", "f1", "f12", "ctrl+p", "control+p", "alt+x", "ctrl+alt+x", "CTRL+P"] {
    assert!(Key::parse(ok).is_some(), "{ok}");
  }
  // Rechazadas.
  for bad in ["", "ab", "g g", "ctrl+x ctrl+s", "a+b", "f13", "f0", "insert", "delete", "super+p", "meta+p", "cmd+p", "ctrl+"] {
    assert_eq!(Key::parse(bad), None, "{bad:?}");
  }
  // Con Ctrl las mayusculas no cuentan: ctrl+P es ctrl+p.
  assert_eq!(Key::parse("ctrl+P"), Key::parse("ctrl+p"));
}
