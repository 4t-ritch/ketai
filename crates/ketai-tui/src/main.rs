//! Binario `ketai`: prepara la terminal y corre el loop de eventos.

use std::{io, sync::mpsc, thread, time::Duration};

use ketai_tui::{
  app::App,
  client::{self, Client, DaemonMsg},
  ui,
};
use ratatui::crossterm::event::{self, Event, KeyEventKind};
use serde_json::json;

/// Todo lo que despierta al loop principal.
enum Input {
  Key(event::KeyEvent),
  Resize,
  Daemon(DaemonMsg),
}

fn main() -> io::Result<()> {
  let (tx, rx) = mpsc::channel::<Input>();
  let mut app = App::default();
  let mut client = client::socket_path().and_then(|p| Client::connect(&p, tx.clone(), Input::Daemon).ok());
  if let Some(c) = client.as_mut() {
    app.connected = true;
    let _ = c.request("daemon.hello", json!({"protocol": ketai_protocol::PROTOCOL_VERSION, "client": "ketai-tui"}));
    let _ = c.request("events.subscribe", json!({"topics": ["player", "queue", "output", "lyrics", "playlist", "library"]}));
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
  let result = (|| -> io::Result<()> {
    loop {
      terminal.draw(|f| ui::draw(f, &app))?;
      let Ok(input) = rx.recv_timeout(Duration::from_millis(250)) else { continue };
      match input {
        Input::Key(k) => {
          for (method, params) in app.on_key(k) {
            if let Some(c) = client.as_mut() {
              let _ = c.request(&method, params);
            }
          }
        }
        Input::Resize => {}
        Input::Daemon(msg) => app.on_daemon(msg),
      }
      if app.quit {
        return Ok(());
      }
    }
  })();
  ratatui::restore();
  result
}
