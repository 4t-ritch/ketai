//! Archivo de configuracion (config.toml): colores y acento de la caratula.

use ketai_tui::{
  app::App,
  client::DaemonMsg,
  config::{Color as Cfg, Config},
  theme::{Theme, rgb},
  ui,
};
use ratatui::{Terminal, backend::TestBackend};
use serde_json::json;

#[test]
fn an_empty_file_gives_the_defaults() {
  let c = Config::parse("").unwrap();
  assert_eq!(c.theme.accent, Cfg::Cover);
  assert_eq!(c.theme.background, Cfg::Cover);
  assert!(c.warnings.is_empty());
}

#[test]
fn colors_override_the_defaults() {
  let c = Config::parse("[theme]\naccent = \"#ff8800\"\nbackground = \"#101010\"\nforeground = \"#EEEEEE\"\n").unwrap();
  assert_eq!(c.theme.accent, Cfg::Fixed(rgb(0xff8800)));
  assert_eq!(c.theme.background, Cfg::Fixed(rgb(0x101010)));
  let t = c.theme(None);
  assert_eq!((t.accent, t.bg, t.fg), (rgb(0xff8800), rgb(0x101010), rgb(0xeeeeee)));
  // Lo que no se menciona queda como antes.
  assert_eq!(t.faint, Theme::default().faint);
}

#[test]
fn errors_name_the_key_or_the_line() {
  let e = Config::parse("[theme]\naccent = \"#zzz\"\n").unwrap_err();
  assert!(e.contains("theme.accent") && e.contains("#zzz"), "{e}");
  let e = Config::parse("[theme]\nforeground = \"cover\"\n").unwrap_err();
  assert!(e.contains("theme.foreground"), "only background and accent can follow the cover: {e}");
  let e = Config::parse("[theme\naccent = 1\n").unwrap_err();
  assert!(e.contains("line 1") || e.contains("línea 1"), "{e}");
  let e = Config::parse("theme = 3\n").unwrap_err();
  assert!(e.contains("theme"), "{e}");
}

#[test]
fn unknown_keys_are_warned_about_but_do_not_stop_the_rest() {
  let c = Config::parse("[theme]\naccent = \"#ff0000\"\naccnet = \"#00ff00\"\n[future]\nx = 1\n").unwrap();
  assert_eq!(c.theme.accent, Cfg::Fixed(rgb(0xff0000)));
  assert_eq!(c.warnings.len(), 2, "{:?}", c.warnings);
  // [playlists] es del daemon: la TUI no la avisa como desconocida.
  assert!(Config::parse("[playlists]\ndir = \"~/x\"\n").unwrap().warnings.is_empty());
  assert!(c.warnings.iter().any(|w| w.contains("theme.accnet")));
}

#[test]
fn cover_colors_follow_the_track_that_plays() {
  let mut app = App { connected: true, ..App::default() };
  app.on_daemon(DaemonMsg::Event { name: "player.state".into(), data: json!({"status": "playing", "track": {"id": "t", "uri": "file:///a.ogg"}}) });
  // Caratula roja.
  app.covers.want("file:///a.ogg");
  app.covers.sent(1, "file:///a.ogg");
  app.covers.on_reply(1, Ok(json!({"path": "/x.png"})));
  app.covers.decoded("file:///a.ogg", Some(image::RgbImage::from_pixel(8, 8, image::Rgb([200, 30, 30]))));
  app.refresh_theme();
  let ratatui::style::Color::Rgb(r, g, b) = app.theme.accent else { panic!() };
  assert!(r > g && r > b, "accent comes from the red cover: {:?}", app.theme.accent);
  let ratatui::style::Color::Rgb(br, _, _) = app.theme.bg else { panic!() };
  assert!(br < 100, "background is a dark tint");
  // Con colores fijos, la caratula no cambia nada.
  app.config = Config::parse("[theme]\naccent = \"#00ff00\"\nbackground = \"#000000\"\n").unwrap();
  app.refresh_theme();
  assert_eq!((app.theme.accent, app.theme.bg), (rgb(0x00ff00), rgb(0x000000)));
}

#[test]
fn a_config_error_is_shown_on_screen() {
  let app = App { connected: true, config_error: Some("config.toml: theme.accent: color inválido \"#zzz\"".into()), ..App::default() };
  let mut term = Terminal::new(TestBackend::new(120, 30)).unwrap();
  term.draw(|f| ui::draw(f, &app)).unwrap();
  let buf = term.backend().buffer();
  let t: String = (0..buf.area.height).map(|y| (0..buf.area.width).map(|x| buf[(x, y)].symbol().to_string()).collect::<String>() + "\n").collect();
  assert!(t.contains("color inválido"), "{t}");
}

/// Paleta de una caratula roja, armada por el mismo camino que la TUI.
fn red_palette() -> ketai_tui::config::CoverPalette {
  let mut app = App::default();
  app.covers.want("u");
  app.covers.sent(1, "u");
  app.covers.on_reply(1, Ok(json!({"path": "/x.png"})));
  app.covers.decoded("u", Some(image::RgbImage::from_pixel(8, 8, image::Rgb([200, 30, 30]))));
  app.covers.palette("u").unwrap()
}

#[test]
fn cover_color_true_paints_the_whole_ui_and_ignores_manual_colors() {
  let p = red_palette();
  let c = Config::parse("[theme]\ncover_color = true\nbackground = \"#000000\"\naccent = \"#00ff00\"\nfaint = \"#123456\"\nrow = \"#654321\"\n").unwrap();
  let t = c.theme(Some(p));
  assert_eq!((t.bg, t.accent, t.raised, t.row), (p.background, p.accent, p.raised, p.row));
  assert_eq!((t.dim, t.faint, t.on_accent, t.spectrum_top), (p.dim, p.faint, p.on_accent, p.spectrum_top));
  // Todos tiran al rojo de la caratula.
  for color in [t.faint, t.dim, t.row, t.spectrum_top] {
    let ratatui::style::Color::Rgb(r, g, b) = color else { panic!() };
    assert!(r >= g && r >= b, "{color:?} follows the red cover");
  }
  // Sin caratula todavia: los colores por defecto, no los manuales.
  assert_eq!(c.theme(None).bg, Theme::default().bg);
}

#[test]
fn cover_color_false_uses_only_manual_colors() {
  let p = red_palette();
  let c = Config::parse("[theme]\ncover_color = false\naccent = \"#00ff00\"\nbackground = \"cover\"\n").unwrap();
  let t = c.theme(Some(p));
  assert_eq!(t.accent, rgb(0x00ff00));
  assert_eq!(t.bg, Theme::default().bg, "\"cover\" means the default when cover_color is false");
  assert_eq!(t.faint, Theme::default().faint);
  assert!(Config::parse("[theme]\ncover_color = \"yes\"\n").unwrap_err().contains("theme.cover_color"));
}

#[test]
fn lyrics_mode_size_and_font() {
  use ketai_tui::config::{LyricsConfig, LyricsMode};
  assert_eq!(Config::parse("").unwrap().lyrics, LyricsConfig { mode: LyricsMode::Classic, size: 3, font: None });
  let c = Config::parse("[lyrics]\nmode = \"karaoke\"\nsize = 5\nfont = \"/fonts/a.ttf\"\n").unwrap();
  assert_eq!(c.lyrics, LyricsConfig { mode: LyricsMode::Karaoke, size: 5, font: Some("/fonts/a.ttf".into()) });
  assert!(c.warnings.is_empty(), "{:?}", c.warnings);
  assert_eq!(Config::parse("[lyrics]\nmode = \"big\"").unwrap_err(), "lyrics.mode: tiene que ser \"classic\" o \"karaoke\"");
  for bad in ["1", "9", "\"3\""] {
    assert_eq!(Config::parse(&format!("[lyrics]\nsize = {bad}")).unwrap_err(), "lyrics.size: tiene que ser un número de 2 a 8 (filas)");
  }
  assert_eq!(Config::parse("[lyrics]\nzoom = 2").unwrap().warnings, ["lyrics.zoom: clave desconocida, se ignora"]);
}
