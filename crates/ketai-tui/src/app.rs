//! Estado de la app y reaccion a teclas y mensajes del daemon. No dibuja ni
//! hace I/O: devuelve las peticiones a enviar, asi se prueba sin terminal.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use serde_json::{Value, json};

use crate::{client::DaemonMsg, theme::Theme};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
  NowPlaying,
  Lyrics,
  Library,
  Search,
}

impl Tab {
  pub const ALL: [Tab; 4] = [Tab::NowPlaying, Tab::Lyrics, Tab::Library, Tab::Search];

  pub fn label(self) -> &'static str {
    match self {
      Tab::NowPlaying => "Reproduciendo",
      Tab::Lyrics => "Letras",
      Tab::Library => "Biblioteca",
      Tab::Search => "Buscar",
    }
  }

  fn index(self) -> usize {
    Tab::ALL.iter().position(|t| *t == self).unwrap_or(0)
  }
}

/// Peticion para el daemon: (metodo, params).
pub type Command = (String, Value);

pub struct App {
  pub tab: Tab,
  pub theme: Theme,
  /// Ultimo `PlayerState` conocido (docs/protocol.md, seccion 4).
  pub player: Value,
  pub connected: bool,
  pub quit: bool,
}

impl Default for App {
  fn default() -> Self {
    Self { tab: Tab::NowPlaying, theme: Theme::default(), player: Value::Null, connected: false, quit: false }
  }
}

impl App {
  pub fn on_key(&mut self, key: KeyEvent) -> Vec<Command> {
    let cmd = |m: &str| vec![(m.to_string(), json!({}))];
    match key.code {
      KeyCode::Char('q') => self.quit = true,
      KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => self.quit = true,
      KeyCode::Char(c @ '1'..='4') => self.tab = Tab::ALL[c as usize - '1' as usize],
      KeyCode::Tab => self.tab = Tab::ALL[(self.tab.index() + 1) % Tab::ALL.len()],
      KeyCode::BackTab => self.tab = Tab::ALL[(self.tab.index() + Tab::ALL.len() - 1) % Tab::ALL.len()],
      KeyCode::Char(' ') => return cmd("player.toggle"),
      KeyCode::Char('n') => return cmd("player.next"),
      KeyCode::Char('p') => return cmd("player.previous"),
      _ => {}
    }
    Vec::new()
  }

  pub fn on_daemon(&mut self, msg: DaemonMsg) {
    match msg {
      DaemonMsg::Event { name, data } => match name.as_str() {
        "player.state" => self.player = data,
        "player.position" => {
          if let (Some(obj), Some(pos)) = (self.player.as_object_mut(), data.get("position_ms")) {
            obj.insert("position_ms".into(), pos.clone());
          } else if self.player.is_null() {
            self.player = json!({"position_ms": data["position_ms"]});
          }
        }
        _ => {}
      },
      DaemonMsg::Reply { .. } => {}
      DaemonMsg::Closed => self.connected = false,
    }
  }
}
