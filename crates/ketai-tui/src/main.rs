//! Binario `ketai`: prepara la terminal y corre el loop de eventos.
//!
//! Un unico canal recibe todo: teclas, mensajes del daemon y resultados de
//! las tareas en segundo plano (buscar el servicio, levantarlo). El loop
//! principal nunca espera a la red ni a systemd.

use std::{
  io,
  sync::mpsc::{self, Sender},
  thread,
  time::{Duration, Instant},
};

use ketai_tui::{
  app::{App, Screen},
  client::{self, Client, DaemonMsg},
  service::{self, Action, Observed},
  ui, viz,
};
use ratatui::crossterm::event::{self, Event, KeyEventKind};
use serde_json::json;

/// Todo lo que despierta al loop principal.
enum Input {
  Key(event::KeyEvent),
  Resize,
  Daemon(DaemonMsg),
  /// Resultado de buscar el servicio.
  Observed(Observed),
  /// Termino una accion sobre el servicio.
  ActionDone(Result<(), String>),
  /// Frame de `viz.sock`.
  Viz(Vec<f32>),
}

/// Envia una peticion y la anota para reconocer su respuesta.
fn send(client: &mut Option<Client>, app: &mut App, method: &str, params: serde_json::Value) {
  if let Some(c) = client.as_mut()
    && let Ok(id) = c.request(method, params)
  {
    app.sent(id, method);
  }
}

/// Lo que se pide al conectar: estado, cola y el espectro.
fn on_connected(client: &mut Option<Client>, app: &mut App, tx: &Sender<Input>) {
  app.connected = true;
  app.screen = Screen::Main;
  send(client, app, "player.state", json!({}));
  send(client, app, "queue.list", json!({}));
  send(client, app, "output.list", json!({}));
  if let Some(path) = viz::socket_path() {
    let _ = viz::spawn(&path, tx.clone(), Input::Viz);
  }
}

/// Cada cuanto se reintenta conectar desde la pantalla de servicio.
const RETRY: Duration = Duration::from_secs(2);

fn connect(tx: &Sender<Input>) -> Option<Client> {
  let mut c = Client::connect(&client::socket_path()?, tx.clone(), Input::Daemon).ok()?;
  c.request("daemon.hello", json!({"protocol": ketai_protocol::PROTOCOL_VERSION, "client": "ketai-tui"})).ok()?;
  c.request("events.subscribe", json!({"topics": ["player", "queue", "output", "lyrics", "playlist", "library"]})).ok()?;
  Some(c)
}

/// Busca el servicio en segundo plano (systemd puede tardar).
fn spawn_observe(tx: &Sender<Input>) {
  let tx = tx.clone();
  thread::spawn(move || {
    let _ = tx.send(Input::Observed(service::observe(false)));
  });
}

fn spawn_action(tx: &Sender<Input>, action: Action, observed: Observed) {
  let tx = tx.clone();
  thread::spawn(move || {
    let _ = tx.send(Input::ActionDone(service::execute(action, &observed)));
  });
}

fn show_service(app: &mut App, tx: &Sender<Input>) {
  app.connected = false;
  app.screen = Screen::Service { state: service::State::Starting, observed: Observed::default(), message: None, busy: true };
  spawn_observe(tx);
}

fn main() -> io::Result<()> {
  let (tx, rx) = mpsc::channel::<Input>();
  let mut app = App::default();
  let mut client = connect(&tx);
  if client.is_some() {
    on_connected(&mut client, &mut app, &tx);
  } else {
    show_service(&mut app, &tx);
  }

  let keys = tx.clone();
  thread::Builder::new().name("ketai-input".into()).spawn(move || {
    loop {
      let input = match event::read() {
        Ok(Event::Key(k)) if k.kind == KeyEventKind::Press => Input::Key(k),
        Ok(Event::Resize(..)) => Input::Resize,
        Ok(_) => continue,
        Err(_) => return,
      };
      if keys.send(input).is_err() {
        return;
      }
    }
  })?;

  let mut terminal = ratatui::init();
  let mut last_retry = Instant::now();
  let result = (|| -> io::Result<()> {
    loop {
      terminal.draw(|f| ui::draw(f, &app))?;
      let input = rx.recv_timeout(Duration::from_millis(250)).ok();

      // Desde la pantalla de servicio se reintenta conectar cada tanto.
      if matches!(app.screen, Screen::Service { busy: false, .. }) && last_retry.elapsed() >= RETRY {
        last_retry = Instant::now();
        if let Some(c) = connect(&tx) {
          client = Some(c);
          on_connected(&mut client, &mut app, &tx);
        }
      }

      match input {
        None | Some(Input::Resize) => {}
        Some(Input::Key(k)) => {
          if let Screen::Service { observed, .. } = &app.screen {
            let observed = observed.clone();
            match app.service_action(k) {
              Some(Action::Retry) => {
                if let Screen::Service { busy, message, .. } = &mut app.screen {
                  *busy = true;
                  *message = None;
                }
                spawn_observe(&tx);
              }
              Some(action) => {
                if let Screen::Service { busy, message, .. } = &mut app.screen {
                  *busy = true;
                  *message = None;
                }
                spawn_action(&tx, action, observed);
              }
              None => {}
            }
          } else {
            for (method, params) in app.on_key(k) {
              send(&mut client, &mut app, &method, params);
            }
          }
        }
        Some(Input::Daemon(DaemonMsg::Closed)) => {
          client = None;
          show_service(&mut app, &tx);
        }
        Some(Input::Daemon(msg)) => {
          for (method, params) in app.on_daemon(msg) {
            send(&mut client, &mut app, &method, params);
          }
        }
        Some(Input::Viz(frame)) => app.on_viz(frame),
        Some(Input::Observed(o)) => {
          let state = service::diagnose(&o);
          if let Screen::Service { state: s, observed, busy, .. } = &mut app.screen {
            *s = state;
            *observed = o;
            *busy = false;
          }
          // Lo encontrado puede ser que ya responde: probar enseguida.
          last_retry = Instant::now() - RETRY;
        }
        Some(Input::ActionDone(result)) => {
          if let Screen::Service { message, .. } = &mut app.screen {
            *message = result.err();
          }
          spawn_observe(&tx);
        }
      }
      if app.quit {
        return Ok(());
      }
    }
  })();
  ratatui::restore();
  result
}
