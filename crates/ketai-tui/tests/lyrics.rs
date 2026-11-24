//! Letras (maqueta 2-letras): pedido al cambiar de pista, sincronia, palabra
//! actual, capas y offset.

use ketai_tui::{
  app::{App, Tab},
  client::DaemonMsg,
  ui,
};
use ratatui::{
  Terminal,
  backend::TestBackend,
  buffer::Buffer,
  crossterm::event::{KeyCode, KeyEvent, KeyModifiers},
  style::Modifier,
};
use serde_json::{Value, json};

fn key(c: KeyCode) -> KeyEvent {
  KeyEvent::new(c, KeyModifiers::NONE)
}

fn state(track_id: &str, pos: u64) -> DaemonMsg {
  DaemonMsg::Event {
    name: "player.state".into(),
    data: json!({"status": "paused", "position_ms": pos, "volume": 1.0, "repeat": "off", "shuffle": false,
                 "track": {"id": track_id, "title": "LET THE WORLD BURN", "artist": "Midnight Blu", "duration_ms": 201000}}),
  }
}

fn methods(cmds: &[(String, Value)]) -> Vec<&str> {
  cmds.iter().map(|(m, _)| m.as_str()).collect()
}

const LAYERS: &str = r#"{"layers": [
  {"id": "original", "kind": "original", "language": "en", "timing": "word", "format": "lrc", "origin": "sidecar"},
  {"id": "es", "kind": "translation", "language": "es", "timing": "line", "format": "lrc", "origin": "sidecar"},
  {"id": "en-Latn", "kind": "romanized", "language": "en-Latn", "timing": "line", "format": "text", "origin": "sidecar"}
]}"#;

fn package() -> Value {
  json!({
    "primary": "original", "secondary": ["es"], "timing": "word", "offset_ms": 0, "user_offset_ms": -250, "agents": [],
    "lines": [
      {"start_ms": 1000, "end_ms": 3000, "text": "let the world burn", "words": null, "agent": null, "background": false, "secondary": {"es": "deja que el mundo arda"}},
      {"start_ms": 3000, "end_ms": 6000, "text": "just to feel something", "agent": null, "background": false, "secondary": {"es": "solo para sentir algo"},
       "words": [{"start_ms": 3000, "end_ms": 3500, "text": "just "}, {"start_ms": 3500, "end_ms": 4000, "text": "to "},
                 {"start_ms": 4000, "end_ms": 4800, "text": "feel "}, {"start_ms": 4800, "end_ms": 6000, "text": "something"}]},
      {"start_ms": 6000, "end_ms": 9000, "text": "i'd set it all on fire", "words": null, "agent": null, "background": false, "secondary": {"es": "le prendería fuego a todo"}}
    ]
  })
}

/// App con la pista t_1, capas y paquete cargados, en `pos`.
fn loaded(pos: u64) -> App {
  let mut app = App { connected: true, ..App::default() };
  let cmds = app.on_daemon(state("t_1", pos));
  for (i, (m, _)) in cmds.iter().enumerate() {
    app.sent(100 + i as u64, m);
  }
  for (i, (m, _)) in cmds.iter().enumerate() {
    let result = match m.as_str() {
      "lyrics.layers" => serde_json::from_str(LAYERS).unwrap(),
      "lyrics.get" => package(),
      _ => continue,
    };
    app.on_daemon(DaemonMsg::Reply { id: 100 + i as u64, result: Ok(result) });
  }
  app
}

#[test]
fn a_new_track_asks_for_its_lyrics() {
  let mut app = App::default();
  let cmds = app.on_daemon(state("t_1", 0));
  assert!(methods(&cmds).contains(&"lyrics.layers"));
  let get = cmds.iter().find(|(m, _)| m == "lyrics.get").expect("lyrics.get");
  assert_eq!(get.1["track_id"], "t_1");
  assert_eq!(get.1["primary"], "original");
  // Mismo track: no se vuelve a pedir.
  assert!(!methods(&app.on_daemon(state("t_1", 500))).contains(&"lyrics.get"));
}

#[test]
fn the_first_translation_is_shown_as_secondary_once_layers_arrive() {
  let app = loaded(0);
  assert_eq!(app.lyrics_secondary, vec!["es".to_string()]);
}

fn render(app: &App) -> Buffer {
  let mut term = Terminal::new(TestBackend::new(160, 44)).unwrap();
  term.draw(|f| ui::draw(f, app)).unwrap();
  term.backend().buffer().clone()
}

fn text(buf: &Buffer) -> String {
  (0..buf.area.height).map(|y| (0..buf.area.width).map(|x| buf[(x, y)].symbol().to_string()).collect::<String>() + "\n").collect()
}

fn find(buf: &Buffer, needle: &str) -> (u16, u16) {
  for y in 0..buf.area.height {
    let row: String = (0..buf.area.width).map(|x| buf[(x, y)].symbol().to_string()).collect();
    if let Some(x) = row.find(needle) {
      return (row[..x].chars().count() as u16, y);
    }
  }
  panic!("{needle:?} not found\n{}", text(buf));
}

#[test]
fn lyrics_tab_shows_lines_translations_and_offset_with_the_current_word_lit() {
  let mut app = loaded(4200);
  app.tab = Tab::Lyrics;
  let buf = render(&app);
  let t = text(&buf);
  for want in ["let the world burn", "deja que el mundo arda", "just to feel something", "solo para sentir algo", "i'd set it all on fire", "-250 ms", "Traducción: es"] {
    assert!(t.contains(want), "missing {want:?}\n{t}");
  }
  // "feel" es la palabra que suena (4000..4800): acento y subrayado.
  let (x, y) = find(&buf, "feel ");
  let cell = &buf[(x, y)];
  assert_eq!(cell.fg, app.theme.accent);
  assert!(cell.modifier.contains(Modifier::UNDERLINED));
  // "just" ya se canto: blanco y en negrita.
  let (x, y) = find(&buf, "just ");
  assert_eq!(buf[(x, y)].fg, app.theme.fg);
}

#[test]
fn now_playing_previews_the_current_line() {
  let app = loaded(4200);
  let t = text(&render(&app));
  assert!(t.contains("▶ just to feel something"), "{t}");
}

#[test]
fn keys_change_layers_and_offset() {
  let mut app = loaded(0);
  app.tab = Tab::Lyrics;
  let cmds = app.on_key(key(KeyCode::Char(']')));
  assert_eq!(cmds, vec![("lyrics.set_offset".into(), json!({"track_id": "t_1", "offset_ms": -200}))]);
  let cmds = app.on_key(key(KeyCode::Char('[')));
  assert_eq!(cmds, vec![("lyrics.set_offset".into(), json!({"track_id": "t_1", "offset_ms": -300}))]);
  // t oculta la traduccion y vuelve a pedir el paquete.
  let cmds = app.on_key(key(KeyCode::Char('t')));
  assert_eq!(cmds[0].0, "lyrics.get");
  assert_eq!(cmds[0].1["secondary"], json!([]));
  // o pasa a la romanizada como principal.
  let cmds = app.on_key(key(KeyCode::Char('o')));
  assert_eq!(cmds[0].1["primary"], "en-Latn");
  // Fuera de la pestana Letras, ']' no hace nada.
  app.tab = Tab::NowPlaying;
  assert!(app.on_key(key(KeyCode::Char(']'))).is_empty());
}

#[test]
fn offset_changes_refetch_the_package() {
  let mut app = loaded(0);
  let cmds = app.on_daemon(DaemonMsg::Event { name: "lyrics.changed".into(), data: json!({"track_id": "t_1"}) });
  assert_eq!(methods(&cmds), vec!["lyrics.get"]);
  assert!(app.on_daemon(DaemonMsg::Event { name: "lyrics.changed".into(), data: json!({"track_id": "t_other"}) }).is_empty());
}

#[test]
fn a_track_without_lyrics_says_so() {
  let mut app = App { connected: true, ..App::default() };
  let cmds = app.on_daemon(state("t_9", 0));
  for (i, (m, _)) in cmds.iter().enumerate() {
    app.sent(200 + i as u64, m);
    let result = if m == "lyrics.layers" { json!({"layers": []}) } else { Value::Null };
    app.on_daemon(DaemonMsg::Reply { id: 200 + i as u64, result: Ok(result) });
  }
  app.tab = Tab::Lyrics;
  let t = text(&render(&app));
  assert!(t.contains("Esta canción no tiene letra"), "{t}");
}

/// App en Letras, lista para el karaoke (imagenes, celdas y fuentes).
fn karaoke_ready(pos: u64) -> App {
  let mut app = loaded(pos);
  app.tab = Tab::Lyrics;
  app.images = true;
  app.cell_px = Some((10, 20));
  app.karaoke_fonts = true;
  app.lyrics_mode = ketai_tui::config::LyricsMode::Karaoke;
  app
}

#[test]
fn m_switches_between_classic_and_karaoke() {
  use ketai_tui::config::LyricsMode;
  let mut app = loaded(4200);
  app.tab = Tab::Lyrics;
  assert_eq!(app.lyrics_mode, LyricsMode::Classic);
  assert_eq!(app.on_key(key(KeyCode::Char('m'))), Vec::<(String, Value)>::new());
  assert_eq!(app.lyrics_mode, LyricsMode::Karaoke);
  app.on_key(key(KeyCode::Char('m')));
  assert_eq!(app.lyrics_mode, LyricsMode::Classic);
}

#[test]
fn karaoke_needs_images_cell_size_and_a_font() {
  let mut app = karaoke_ready(4200);
  assert!(app.karaoke_on());
  for off in [|a: &mut App| a.images = false, |a: &mut App| a.cell_px = None, |a: &mut App| a.karaoke_fonts = false] {
    let mut a = karaoke_ready(4200);
    off(&mut a);
    assert!(!a.karaoke_on());
  }
  app.lyrics_mode = ketai_tui::config::LyricsMode::Classic;
  assert!(!app.karaoke_on());
  assert_eq!(ketai_tui::app::cell_px(Some((100, 40, 1000, 800))), Some((10, 20)));
  assert_eq!(ketai_tui::app::cell_px(Some((100, 40, 0, 0))), None, "the terminal does not report pixels");
  assert_eq!(ketai_tui::app::cell_px(None), None);
}

#[test]
fn karaoke_leaves_the_current_line_to_the_image_and_keeps_the_rest_as_text() {
  let mut app = karaoke_ready(4200);
  app.config.lyrics.size = 4;
  let buf = render(&app);
  let t = text(&buf);
  let slot = ui::take_karaoke().expect("the current line goes to the karaoke");
  assert_eq!(slot.words, ["just ", "to ", "feel ", "something"]);
  assert_eq!(slot.times, [(3000, 3500), (3500, 4000), (4000, 4800), (4800, 6000)]);
  assert_eq!((slot.area.width, slot.area.height), (160, 4), "the whole width, `size` rows");
  // Sin texto en esas celdas y con el fondo del tema (la imagen va encima;
  // si falta un instante no se ve el fondo de la terminal).
  assert!(!t.contains("just to feel something"), "{t}");
  for y in slot.area.y..slot.area.y + slot.area.height {
    assert_eq!(buf[(5, y)].bg, app.theme.bg);
  }
  // Las otras lineas y la traduccion de la actual siguen como texto, debajo.
  for want in ["let the world burn", "i'd set it all on fire", "solo para sentir algo"] {
    assert!(t.contains(want), "missing {want:?}\n{t}");
  }
  assert!(find(&buf, "solo para sentir algo").1 >= slot.area.y + slot.area.height);
  // Modo clasico: no hay linea de karaoke.
  app.lyrics_mode = ketai_tui::config::LyricsMode::Classic;
  render(&app);
  assert!(ui::take_karaoke().is_none());
}

#[test]
fn karaoke_dims_the_lines_around_the_one_being_sung() {
  let app = karaoke_ready(4200);
  let buf = render(&app);
  ui::take_karaoke();
  let t = &app.theme;
  let (x, y) = find(&buf, "let the world burn");
  assert_eq!(buf[(x, y)].fg, ketai_tui::theme::lerp(t.faint, t.bg, 0.45), "sung lines, dimmer");
  let (x, y) = find(&buf, "i'd set it all on fire");
  assert_eq!(buf[(x, y)].fg, ketai_tui::theme::lerp(t.upcoming(), t.bg, 0.35), "coming lines, dimmer");
  // El modo clasico no cambia.
  let mut classic = karaoke_ready(4200);
  classic.lyrics_mode = ketai_tui::config::LyricsMode::Classic;
  let buf = render(&classic);
  let (x, y) = find(&buf, "let the world burn");
  assert_eq!(buf[(x, y)].fg, t.faint);
}

#[test]
fn the_header_says_which_mode_is_on() {
  let mut app = karaoke_ready(4200);
  app.lyrics_mode = ketai_tui::config::LyricsMode::Classic;
  assert!(text(&render(&app)).contains("Modo: clásico   m "));
  app.lyrics_mode = ketai_tui::config::LyricsMode::Karaoke;
  assert!(text(&render(&app)).contains("Modo: karaoke   m "));
  ui::take_karaoke();
  // Pedido pero sin tamano de celda: se ve el clasico y se avisa.
  app.cell_px = None;
  assert!(text(&render(&app)).contains("Modo: karaoke (no disponible)   m "));
}

#[test]
fn saving_the_config_keeps_the_mode_chosen_with_m_unless_mode_changed() {
  use ketai_tui::config::{Config, LyricsMode};
  let mut app = karaoke_ready(4200);
  app.config = Config::parse("[lyrics]\nsize = 3").unwrap();
  app.apply_lyrics_config();
  assert_eq!(app.lyrics_mode, LyricsMode::Classic, "first load takes the file's mode");
  app.on_key(key(KeyCode::Char('m')));
  // Se guarda otro tamano: el modo elegido con `m` se mantiene y el tamano cambia.
  app.config = Config::parse("[lyrics]\nsize = 5").unwrap();
  app.apply_lyrics_config();
  assert_eq!(app.lyrics_mode, LyricsMode::Karaoke);
  render(&app);
  assert_eq!(ui::take_karaoke().unwrap().area.height, 5, "the new size applies live");
  // Cambiar `mode` en el archivo si lo aplica.
  app.config = Config::parse("[lyrics]\nmode = \"karaoke\"\nsize = 5").unwrap();
  app.apply_lyrics_config();
  app.on_key(key(KeyCode::Char('m')));
  assert_eq!(app.lyrics_mode, LyricsMode::Classic);
  app.config = Config::parse("[lyrics]\nmode = \"karaoke\"\nsize = 4").unwrap();
  app.apply_lyrics_config();
  assert_eq!(app.lyrics_mode, LyricsMode::Classic, "mode did not change in the file");
  app.config = Config::parse("[lyrics]\nmode = \"classic\"\nsize = 4").unwrap();
  app.apply_lyrics_config();
  app.on_key(key(KeyCode::Char('m')));
  app.config = Config::parse("[lyrics]\nmode = \"karaoke\"\nsize = 4").unwrap();
  app.apply_lyrics_config();
  assert_eq!(app.lyrics_mode, LyricsMode::Karaoke);
  app.config = Config::parse("[lyrics]\nmode = \"classic\"\nsize = 5").unwrap();
  app.apply_lyrics_config();
  assert_eq!(app.lyrics_mode, LyricsMode::Classic);
}

/// Paquete con diez lineas, una cada 3 s, sin traducciones.
fn ten_lines(app: &mut App) {
  let lines: Vec<Value> = (0..10)
    .map(|i| json!({"start_ms": 1000 + i * 3000, "end_ms": 4000 + i * 3000, "text": format!("line number {i}"), "words": null,
                    "agent": null, "background": false, "secondary": {}}))
    .collect();
  app.lyrics["lines"] = json!(lines);
  app.lyrics_secondary.clear();
}

fn row_of(buf: &Buffer, needle: &str) -> Option<u16> {
  (0..buf.area.height).find(|&y| (0..buf.area.width).map(|x| buf[(x, y)].symbol().to_string()).collect::<String>().contains(needle))
}

#[test]
fn the_current_line_sits_in_the_middle_with_the_configured_lines_around() {
  let mut app = loaded(0);
  app.tab = Tab::Lyrics;
  ten_lines(&mut app);
  // Linea 5 (16000..19000).
  app.player["position_ms"] = json!(17000);
  let buf = render(&app);
  let y5 = row_of(&buf, "line number 5").unwrap();
  // Cerca del centro de la pantalla (44 filas), no arriba.
  assert!((20..=24).contains(&y5), "current at {y5}");
  // Por defecto, dos arriba y tres abajo, cada una a 4 filas.
  for (n, dy) in [(3, -8), (4, -4), (6, 4), (7, 8), (8, 12)] {
    assert_eq!(row_of(&buf, &format!("line number {n}")), Some((y5 as i32 + dy) as u16), "line {n}");
  }
  for n in [2, 9] {
    assert!(row_of(&buf, &format!("line number {n}")).is_none(), "line {n} is not shown");
  }
  // Configurable: una arriba y una abajo.
  app.config.lyrics.lines_before = 1;
  app.config.lyrics.lines_after = 1;
  let buf = render(&app);
  assert!(row_of(&buf, "line number 4").is_some() && row_of(&buf, "line number 6").is_some());
  assert!(row_of(&buf, "line number 3").is_none() && row_of(&buf, "line number 7").is_none());
  // Al principio no hay lineas arriba: la actual sigue en el centro.
  app.player["position_ms"] = json!(1500);
  let buf = render(&app);
  assert_eq!(row_of(&buf, "line number 0"), Some(y5));
}

#[test]
fn clicking_a_line_seeks_to_its_start() {
  use ratatui::crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
  let mut app = loaded(0);
  app.tab = Tab::Lyrics;
  ten_lines(&mut app);
  app.player["position_ms"] = json!(17000);
  let buf = render(&app);
  app.hits = ui::take_hits();
  let (x, y) = find(&buf, "line number 7");
  let click = |x, y| MouseEvent { kind: MouseEventKind::Down(MouseButton::Left), column: x, row: y, modifiers: KeyModifiers::NONE };
  let cmds = app.on_mouse(click(x + 3, y), std::time::Instant::now());
  assert_eq!(cmds, vec![("player.seek".to_string(), json!({"position_ms": 22000}))]);
  // Solo el parrafo: a los costados del texto no hace nada.
  assert!(app.on_mouse(click(1, y), std::time::Instant::now()).is_empty());
}

#[test]
fn during_the_intro_the_karaoke_place_is_empty_and_the_first_line_waits_below() {
  let mut app = karaoke_ready(0);
  ten_lines(&mut app);
  app.config.lyrics.size = 3;
  // Antes de la primera linea (empieza en 1000).
  app.player["position_ms"] = json!(200);
  let buf = render(&app);
  assert!(ui::take_karaoke().is_none(), "nothing is sung yet");
  let waiting = row_of(&buf, "line number 0").unwrap();
  // Empieza la primera: sube al lugar del karaoke y la segunda ocupa el
  // lugar donde esperaba la primera.
  app.player["position_ms"] = json!(1500);
  let buf = render(&app);
  let slot = ui::take_karaoke().unwrap();
  assert!(waiting >= slot.area.y + slot.area.height, "it waited below the karaoke place ({waiting} vs {:?})", slot.area);
  assert_eq!(row_of(&buf, "line number 1"), Some(waiting));
}

#[test]
fn with_a_popup_open_the_karaoke_line_is_text() {
  let mut app = karaoke_ready(4200);
  app.devices_open = true;
  render(&app);
  assert!(ui::take_karaoke().is_none(), "the image would cover the popup");
}

#[test]
fn m_saves_the_mode_in_the_config_file() {
  use ketai_tui::config::{Config, LyricsMode};
  let dir = std::env::temp_dir().join(format!("ketai-m-saves-{}", std::process::id()));
  let path = dir.join("config.toml");
  std::fs::create_dir_all(&dir).unwrap();
  std::fs::write(&path, "# keep me\n[lyrics]\nsize = 4\n").unwrap();
  let mut app = loaded(4200);
  app.tab = Tab::Lyrics;
  app.config_path = Some(path.clone());
  app.on_key(key(KeyCode::Char('m')));
  let text = std::fs::read_to_string(&path).unwrap();
  assert_eq!(text, "# keep me\n[lyrics]\nmode = \"karaoke\"\nsize = 4\n");
  // Al recargar lo guardado, el modo sigue siendo el elegido.
  app.config = Config::parse(&text).unwrap();
  app.apply_lyrics_config();
  assert_eq!(app.lyrics_mode, LyricsMode::Karaoke);
  assert!(app.notice.is_none());
  std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn the_lyrics_options_are_keycaps_that_respond_to_clicks() {
  use ratatui::crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
  let mut app = loaded(4200);
  app.tab = Tab::Lyrics;
  let buf = render(&app);
  let t = text(&buf);
  for want in ["Capa: Original   o ", "Traducción: es   t ", "Offset: -250 ms   [   ] ", "Modo: clásico   m "] {
    assert!(t.contains(want), "missing {want:?}\n{t}");
  }
  // La tecla va en un recuadro, como en Reproduciendo; etiqueta clara y
  // valor tenue.
  let (x, y) = find(&buf, "Modo: clásico   m ");
  assert_eq!(buf[(x, y)].fg, app.theme.fg, "label");
  assert_eq!(buf[(x + 6, y)].fg, app.theme.faint, "value");
  assert_eq!(buf[(x + 16, y)].bg, app.theme.raised, "keycap");
  // Una fila en blanco entre las dos filas de opciones.
  assert_eq!(find(&buf, "Modo:").1, find(&buf, "Capa:").1 + 2);
  app.hits = ui::take_hits();
  let click = |app: &mut App, needle: &str| {
    let (x, y) = find(&buf, needle);
    let ev = MouseEvent { kind: MouseEventKind::Down(MouseButton::Left), column: x + 1, row: y, modifiers: KeyModifiers::NONE };
    app.on_mouse(ev, std::time::Instant::now())
  };
  // Clic en el modo: cambia como con `m`.
  click(&mut app, "Modo:");
  assert_eq!(app.lyrics_mode, ketai_tui::config::LyricsMode::Karaoke);
  // Clic en `]`: offset +50 (como la tecla).
  let cmds = click(&mut app, " ] ");
  assert_eq!(cmds, vec![("lyrics.set_offset".to_string(), json!({"track_id": "t_1", "offset_ms": -200}))]);
  // Clic en la traduccion: la oculta.
  click(&mut app, "Traducción:");
  assert!(app.lyrics_secondary.is_empty());
  let t = text(&render(&app));
  assert!(t.contains("Traducción: no   t "), "{t}");
}

#[test]
fn lyrics_keys_come_from_the_config_and_the_header_shows_them_live() {
  use ketai_tui::config::{Config, LyricsMode};
  let mut app = loaded(4200);
  app.tab = Tab::Lyrics;
  // Por defecto, `m` cambia el modo en Letras.
  assert!(text(&render(&app)).contains("Modo: clásico   m "));
  app.on_key(key(KeyCode::Char('m')));
  assert_eq!(app.lyrics_mode, LyricsMode::Karaoke);
  // Se cambia la tecla en config.toml: la cabecera muestra la nueva.
  app.config = Config::parse("[keys]\nlyrics_mode = \"k\"\nlyrics_offset_up = \"ctrl+u\"").unwrap();
  let t = text(&render(&app));
  assert!(t.contains("Modo: karaoke   k "), "{t}");
  assert!(t.contains(" Ctrl+U "), "{t}");
  assert!(!t.contains("   m "), "the old key is not shown\n{t}");
  // Y es la que responde; `m` vuelve a ser la global (silenciar).
  app.on_key(key(KeyCode::Char('k')));
  assert_eq!(app.lyrics_mode, LyricsMode::Classic);
  let cmds = app.on_key(key(KeyCode::Char('m')));
  assert_eq!(app.lyrics_mode, LyricsMode::Classic);
  assert_eq!(methods(&cmds), ["player.set_muted"]);
}

#[test]
fn duplicated_keys_block_the_app_with_a_grouped_scrollable_list() {
  use ketai_tui::config::Config;
  let mut app = loaded(4200);
  let bad = Config::parse("[keys]\nnext = \"x\"\nprevious = \"x\"\nshuffle = \"z\"\nrepeat = \"z\"\nfavorite = \"y\"\nfolders = \"y\"").unwrap();
  app.config = bad.clone();
  assert!(app.keys_blocked());
  // Nada responde salvo desplazar y salir.
  assert!(app.on_key(key(KeyCode::Char('x'))).is_empty());
  assert!(app.on_key(key(KeyCode::Char(' '))).is_empty());
  assert!(app.on_key(key(KeyCode::Char('2'))).is_empty());
  assert_eq!(app.tab, Tab::NowPlaying);
  // La lista: cada tecla con sus acciones agrupadas.
  let mut term = Terminal::new(TestBackend::new(100, 16)).unwrap();
  term.draw(|f| ui::draw(f, &app)).unwrap();
  let t = text(term.backend().buffer());
  for want in ["Teclas repetidas en config.toml", "Corregí [keys] y guardá: ketai se desbloquea solo.", " x   en 2 acciones", "next", "previous", " z   en 2 acciones", "↑ ↓ desplazar", "q salir"] {
    assert!(t.contains(want), "missing {want:?}\n{t}");
  }
  // En 16 filas no entra todo: se desplaza hasta el ultimo grupo.
  assert!(!t.contains(" y   en 2 acciones"), "{t}");
  for _ in 0..20 {
    app.on_key(key(KeyCode::Down));
  }
  term.draw(|f| ui::draw(f, &app)).unwrap();
  let t = text(term.backend().buffer());
  assert!(t.contains(" y   en 2 acciones") && t.contains("folders"), "{t}");
  // Los clics no hacen nada (la rueda desplaza).
  app.hits = ui::take_hits();
  // Al guardar la configuracion corregida, se desbloquea.
  app.config = Config::parse("[keys]\nnext = \"x\"").unwrap();
  assert!(!app.keys_blocked());
  assert_eq!(methods(&app.on_key(key(KeyCode::Char('x')))), ["player.next"]);
  // `q` sale aun bloqueada.
  app.config = bad;
  app.on_key(key(KeyCode::Char('q')));
  assert!(app.quit);
}
