//! Dispositivos (maqueta 6-dispositivos): popup para elegir la salida.

use ketai_tui::{app::App, client::DaemonMsg, ui};
use ratatui::{
  Terminal,
  backend::TestBackend,
  crossterm::event::{KeyCode, KeyEvent, KeyModifiers},
};
use serde_json::{Value, json};

fn key(c: KeyCode) -> KeyEvent {
  KeyEvent::new(c, KeyModifiers::NONE)
}

fn devices() -> Value {
  json!({"devices": [
    {"id": "alsa_output.pci-0000_00_1f.3", "name": "Parlantes internos", "is_default": true},
    {"id": "bluez_output.88_C9_E8.1", "name": "WH-1000XM4", "is_default": false},
    {"id": "alsa_output.hdmi-stereo", "name": "Monitor LG HDMI", "is_default": false}
  ], "active": "bluez_output.88_C9_E8.1"})
}

fn with_devices() -> App {
  let mut app = App { connected: true, ..App::default() };
  app.on_daemon(DaemonMsg::Event { name: "output.changed".into(), data: devices() });
  app
}

#[test]
fn d_opens_the_picker_with_the_current_output_first() {
  let mut app = App { connected: true, ..App::default() };
  // Sin lista todavia, abrir la pide.
  assert_eq!(app.on_key(key(KeyCode::Char('d'))), vec![("output.list".to_string(), json!({}))]);
  assert!(app.devices_open);
  let mut app = with_devices();
  assert!(app.on_key(key(KeyCode::Char('d'))).is_empty());
  let rows = app.device_rows();
  assert_eq!(rows[0].0.as_deref(), Some("bluez_output.88_C9_E8.1"), "current first");
  assert!(rows.iter().any(|(id, _)| id.is_none()), "system default is an option");
}

#[test]
fn enter_switches_the_output_and_escape_closes() {
  let mut app = with_devices();
  app.on_key(key(KeyCode::Char('d')));
  app.on_key(key(KeyCode::Down));
  let (id, _) = app.device_rows()[1].clone();
  let cmds = app.on_key(key(KeyCode::Enter));
  assert_eq!(cmds, vec![("output.set".to_string(), json!({"id": id}))]);
  assert!(!app.devices_open);
  app.on_key(key(KeyCode::Char('d')));
  app.on_key(key(KeyCode::Esc));
  assert!(!app.devices_open);
}

#[test]
fn following_the_default_sends_null() {
  let mut app = with_devices();
  app.on_key(key(KeyCode::Char('d')));
  let at = app.device_rows().iter().position(|(id, _)| id.is_none()).unwrap();
  for _ in 0..at {
    app.on_key(key(KeyCode::Down));
  }
  assert_eq!(app.on_key(key(KeyCode::Enter)), vec![("output.set".to_string(), json!({"id": null}))]);
}

fn text(app: &App) -> String {
  let mut term = Terminal::new(TestBackend::new(160, 44)).unwrap();
  term.draw(|f| ui::draw(f, app)).unwrap();
  let buf = term.backend().buffer();
  (0..buf.area.height).map(|y| (0..buf.area.width).map(|x| buf[(x, y)].symbol().to_string()).collect::<String>() + "\n").collect()
}

#[test]
fn the_popup_lists_the_outputs_and_the_header_does_not_repeat_the_active_one() {
  let mut app = with_devices();
  // La salida activa se ve en el reproductor, no en la cabecera.
  let header = text(&app).lines().nth(1).unwrap().to_string();
  assert!(!header.contains("WH-1000XM4"), "{header}");
  app.on_key(key(KeyCode::Char('d')));
  let t = text(&app);
  for want in ["Conectar a un dispositivo", "DISPOSITIVO ACTUAL", "WH-1000XM4", "bluetooth · bluez_output.88_C9_E8.1", "OTROS DISPOSITIVOS", "Predeterminado del sistema", "sigue al default: Parlantes internos", "Monitor LG HDMI", "HDMI"] {
    assert!(t.contains(want), "missing {want:?}\n{t}");
  }
}
