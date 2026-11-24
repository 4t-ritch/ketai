//! Busqueda y arranque del servicio: docs/service.md, seccion "Clients".
//! systemd se reemplaza por un falso.

use ketai_tui::{
  app::{App, Screen},
  service::{Action, Observed, State, UnitInfo, diagnose},
  ui,
};
use ratatui::{
  Terminal,
  backend::TestBackend,
  crossterm::event::{KeyCode, KeyEvent, KeyModifiers},
};

fn units(socket_active: bool, service_active: bool, enabled: bool) -> Option<UnitInfo> {
  Some(UnitInfo { socket_active, service_active, enabled })
}

fn obs(socket_ok: bool, systemd: Option<Option<UnitInfo>>, process: Option<i32>) -> Observed {
  Observed { socket_ok, systemd, process, binary: Some("/home/u/.local/bin/ketaid".into()) }
}

#[test]
fn every_state_of_the_table() {
  // connected: el daemon responde.
  assert_eq!(diagnose(&obs(true, Some(units(true, true, true)), Some(10))), State::Connected);
  // starting: unidad activa pero el socket todavia no responde.
  assert_eq!(diagnose(&obs(false, Some(units(true, false, true)), None)), State::Starting);
  // stopped: instaladas pero inactivas.
  assert_eq!(diagnose(&obs(false, Some(units(false, false, false)), None)), State::Stopped { enabled: false });
  assert_eq!(diagnose(&obs(false, Some(units(false, false, true)), None)), State::Stopped { enabled: true });
  // not_installed: systemd sin unidades de ketai.
  assert_eq!(diagnose(&obs(false, Some(None), None)), State::NotInstalled);
  // unresponsive: hay un proceso pero no responde.
  assert_eq!(diagnose(&obs(false, Some(units(false, false, false)), Some(42))), State::Unresponsive { pid: 42 });
  assert_eq!(diagnose(&obs(false, None, Some(42))), State::Unresponsive { pid: 42 });
  // no_systemd: no hay systemd de usuario.
  assert_eq!(diagnose(&obs(false, None, None)), State::NoSystemd);
}

fn key(c: KeyCode) -> KeyEvent {
  KeyEvent::new(c, KeyModifiers::NONE)
}

fn on_service(state: State) -> App {
  App { screen: Screen::Service { state, observed: obs(false, None, None), message: None, busy: false }, ..App::default() }
}

#[test]
fn keys_on_the_service_screen_pick_the_action_of_each_state() {
  let mut app = on_service(State::Stopped { enabled: false });
  assert_eq!(app.service_action(key(KeyCode::Enter)), Some(Action::Start { enable: false }));
  assert_eq!(app.service_action(key(KeyCode::Char('e'))), Some(Action::Start { enable: true }));
  assert_eq!(app.service_action(key(KeyCode::Char('r'))), Some(Action::Retry));

  let mut app = on_service(State::NotInstalled);
  assert_eq!(app.service_action(key(KeyCode::Enter)), Some(Action::Install));

  // Reiniciar un proceso colgado pide confirmacion: primero Enter, despues y.
  let mut app = on_service(State::Unresponsive { pid: 42 });
  assert_eq!(app.service_action(key(KeyCode::Enter)), None);
  assert_eq!(app.service_action(key(KeyCode::Char('y'))), Some(Action::Restart { pid: 42 }));

  let mut app = on_service(State::NoSystemd);
  assert_eq!(app.service_action(key(KeyCode::Enter)), Some(Action::StartManual));

  let mut app = on_service(State::Stopped { enabled: false });
  app.service_action(key(KeyCode::Char('q')));
  assert!(app.quit);
}

fn screen_text(app: &App) -> String {
  let mut term = Terminal::new(TestBackend::new(120, 34)).unwrap();
  term.draw(|f| ui::draw(f, app)).unwrap();
  let buf = term.backend().buffer();
  (0..buf.area.height).map(|y| (0..buf.area.width).map(|x| buf[(x, y)].symbol().to_string()).collect::<String>() + "\n").collect()
}

#[test]
fn the_screen_explains_the_state_and_offers_its_buttons() {
  let text = screen_text(&on_service(State::Stopped { enabled: false }));
  assert!(text.contains("El servicio ketaid no está corriendo"), "{text}");
  assert!(text.contains("Iniciar servicio"));
  assert!(text.contains("Iniciar siempre con la sesión"));

  let text = screen_text(&on_service(State::NotInstalled));
  assert!(text.contains("Instalar e iniciar"), "{text}");

  let text = screen_text(&on_service(State::Unresponsive { pid: 42 }));
  assert!(text.contains("42"), "{text}");
  assert!(text.contains("Reiniciar"));

  let text = screen_text(&on_service(State::Starting));
  assert!(text.contains("Iniciando"), "{text}");
}

/// Consulta el systemd y el /proc reales, solo lectura. Se corre a mano:
/// `cargo test -p ketai-tui --test service -- --ignored --nocapture`.
#[test]
#[ignore = "reads the user's real systemd and /proc"]
fn observe_the_real_system() {
  let o = ketai_tui::service::observe(false);
  println!("{o:?} -> {:?}", diagnose(&o));
}
