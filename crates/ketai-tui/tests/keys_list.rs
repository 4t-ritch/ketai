//! Lista de atajos de teclado, desde la busqueda rapida (Ctrl+P): cada
//! accion con sus teclas actuales.

use ketai_tui::{
  app::{App, PaletteAction, PaletteItem},
  config::Config,
  ui,
};
use ratatui::{
  Terminal,
  backend::TestBackend,
  buffer::Buffer,
  crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind},
};

fn key(c: KeyCode) -> KeyEvent {
  KeyEvent::new(c, KeyModifiers::NONE)
}

fn render(app: &App, h: u16) -> Buffer {
  let mut term = Terminal::new(TestBackend::new(160, h)).unwrap();
  term.draw(|f| ui::draw(f, app)).unwrap();
  term.backend().buffer().clone()
}

fn text(buf: &Buffer) -> String {
  (0..buf.area.height).map(|y| (0..buf.area.width).map(|x| buf[(x, y)].symbol().to_string()).collect::<String>() + "\n").collect()
}

/// Abre la lista como lo haria el usuario: Ctrl+P y la opcion.
fn open() -> App {
  let mut app = App { connected: true, ..App::default() };
  app.on_key(KeyEvent::new(KeyCode::Char('p'), KeyModifiers::CONTROL));
  let at = app.palette_items().iter().position(|i| *i == PaletteItem::Action(PaletteAction::Keys)).expect("the option is in Ctrl+P");
  app.palette_cursor = at;
  app.on_key(key(KeyCode::Enter));
  assert!(app.keys_open && !app.palette_open);
  app
}

#[test]
fn ctrl_p_has_an_option_that_lists_every_action_with_its_keys() {
  let app = open();
  let t = text(&render(&app, 120));
  for want in ["Atajos de teclado", "En todas las vistas", "En Letras", "En Reproduciendo (cola)", "Ctrl+C siempre sale"] {
    assert!(t.contains(want), "missing {want:?}\n{t}");
  }
  // Todas las acciones, con su nombre en [keys] y su descripcion (sin el
  // prefijo del grupo), en la misma fila.
  for (name, _, _, desc) in ketai_tui::keys::ACTIONS {
    let desc = desc.split_once(": ").map_or(*desc, |(_, d)| d);
    let row = t.lines().find(|l| l.contains(&format!(" {name} "))).unwrap_or_else(|| panic!("missing {name}\n{t}"));
    assert!(row.contains(desc), "{name}: {row}");
  }
  // Con todas sus teclas: volume_up tiene dos.
  let row = t.lines().find(|l| l.contains("volume_up")).unwrap();
  assert!(row.contains(" + ") && row.contains(" = "), "{row}");
}

#[test]
fn the_list_shows_the_keys_configured_right_now() {
  let mut app = open();
  app.config = Config::parse("[keys]\nvolume_up = \"ctrl+u\"\nqueue_remove = \"X\"").unwrap();
  let t = text(&render(&app, 120));
  let row = t.lines().find(|l| l.contains("volume_up")).unwrap();
  assert!(row.contains(" Ctrl+U ") && !row.contains(" = "), "{row}");
  assert!(t.lines().find(|l| l.contains("queue_remove")).unwrap().contains(" X "), "{t}");
}

#[test]
fn it_scrolls_closes_with_esc_and_keys_do_not_reach_the_view() {
  let mut app = open();
  let t = text(&render(&app, 30));
  assert!(t.contains("↑ ↓ desplazar") && !t.contains("queue_add_to_playlist"), "{t}");
  for _ in 0..40 {
    assert!(app.on_key(key(KeyCode::Down)).is_empty());
  }
  let t = text(&render(&app, 30));
  assert!(t.contains("queue_add_to_playlist"), "{t}");
  assert!(app.on_key(key(KeyCode::Char('n'))).is_empty(), "next does not play while open");
  app.on_key(key(KeyCode::Esc));
  assert!(!app.keys_open);
}

#[test]
fn a_click_outside_closes_it() {
  let mut app = open();
  render(&app, 44);
  app.hits = ui::take_hits();
  let ev = MouseEvent { kind: MouseEventKind::Down(MouseButton::Left), column: 0, row: 43, modifiers: KeyModifiers::NONE };
  app.on_mouse(ev, std::time::Instant::now());
  assert!(!app.keys_open);
}
