//! Estado de la app y reaccion a teclas y mensajes del daemon. No dibuja ni
//! hace I/O: devuelve las peticiones a enviar, asi se prueba sin terminal.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use serde_json::{Value, json};

use crate::{
  client::DaemonMsg,
  service::{Action, Observed, State},
  theme::Theme,
};

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

/// Pantalla principal o la de servicio (cuando no hay daemon).
#[derive(Debug, Clone, PartialEq)]
pub enum Screen {
  Main,
  Service {
    state: State,
    observed: Observed,
    /// Error de la ultima accion, si fallo.
    message: Option<String>,
    /// Hay una accion en curso.
    busy: bool,
  },
}

/// Peticion para el daemon: (metodo, params).
pub type Command = (String, Value);

pub struct App {
  pub screen: Screen,
  /// Se pidio reiniciar un proceso colgado y falta confirmar.
  pub confirm_restart: bool,
  pub tab: Tab,
  pub theme: Theme,
  /// Ultimo `PlayerState` conocido (docs/protocol.md, seccion 4).
  pub player: Value,
  pub connected: bool,
  pub quit: bool,
}

impl Default for App {
  fn default() -> Self {
    Self { screen: Screen::Main, confirm_restart: false, tab: Tab::NowPlaying, theme: Theme::default(), player: Value::Null, connected: false, quit: false }
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

  /// Tecla en la pantalla de servicio: la accion a ejecutar, si hay.
  pub fn service_action(&mut self, key: KeyEvent) -> Option<Action> {
    let Screen::Service { state, busy, .. } = &self.screen else { return None };
    let (state, busy) = (*state, *busy);
    match key.code {
      KeyCode::Char('q') => {
        self.quit = true;
        return None;
      }
      KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
        self.quit = true;
        return None;
      }
      KeyCode::Esc => {
        self.confirm_restart = false;
        return None;
      }
      _ => {}
    }
    if busy {
      return None;
    }
    match (key.code, state) {
      (KeyCode::Char('r'), _) => Some(Action::Retry),
      (KeyCode::Enter | KeyCode::Char('s'), State::Stopped { .. }) => Some(Action::Start { enable: false }),
      (KeyCode::Char('e'), State::Stopped { .. }) => Some(Action::Start { enable: true }),
      (KeyCode::Enter, State::NotInstalled) => Some(Action::Install),
      (KeyCode::Enter, State::NoSystemd) => Some(Action::StartManual),
      (KeyCode::Enter, State::Unresponsive { .. }) => {
        self.confirm_restart = true;
        None
      }
      (KeyCode::Char('y'), State::Unresponsive { pid }) if self.confirm_restart => {
        self.confirm_restart = false;
        Some(Action::Restart { pid })
      }
      _ => None,
    }
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
