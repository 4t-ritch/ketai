//! Estado de la app y reaccion a teclas y mensajes del daemon. No dibuja ni
//! hace I/O: devuelve las peticiones a enviar, asi se prueba sin terminal.

use std::{collections::HashMap, time::Instant};

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
  /// Resultado de `queue.list`.
  pub queue: Value,
  /// Ultimo frame de `viz.sock`.
  pub viz: Vec<f32>,
  /// Momento en que se supo `player.position_ms`, para interpolar.
  pub pos_at: Instant,
  /// Peticiones enviadas sin respuesta: id -> metodo.
  pub pending: HashMap<u64, String>,
}

/// `m:ss` o `h:mm:ss`.
pub fn fmt_ms(ms: u64) -> String {
  let s = ms / 1000;
  if s >= 3600 { format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60) } else { format!("{}:{:02}", s / 60, s % 60) }
}

impl Default for App {
  fn default() -> Self {
    Self {
      screen: Screen::Main,
      confirm_restart: false,
      tab: Tab::NowPlaying,
      theme: Theme::default(),
      player: Value::Null,
      connected: false,
      quit: false,
      queue: Value::Null,
      viz: Vec::new(),
      pos_at: Instant::now(),
      pending: HashMap::new(),
    }
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
      KeyCode::Right => return vec![("player.seek_relative".into(), json!({"offset_ms": 5000}))],
      KeyCode::Left => return vec![("player.seek_relative".into(), json!({"offset_ms": -5000}))],
      KeyCode::Char(c @ ('+' | '=' | '-')) => {
        let step = if c == '-' { -0.05 } else { 0.05 };
        let v = self.player["volume"].as_f64().unwrap_or(1.0) + step;
        let v = (v.clamp(0.0, 1.0) * 100.0).round() / 100.0;
        return vec![("player.set_volume".into(), json!({"volume": v}))];
      }
      KeyCode::Char('m') => {
        let muted = self.player["muted"].as_bool().unwrap_or(false);
        return vec![("player.set_muted".into(), json!({"muted": !muted}))];
      }
      KeyCode::Char('s') => {
        let on = self.player["shuffle"].as_bool().unwrap_or(false);
        return vec![("player.set_shuffle".into(), json!({"shuffle": !on}))];
      }
      KeyCode::Char('r') => {
        let next = match self.player["repeat"].as_str() {
          Some("off") | None => "track",
          Some("track") => "queue",
          _ => "off",
        };
        return vec![("player.set_repeat".into(), json!({"repeat": next}))];
      }
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

  /// Se envio la peticion `id`: su respuesta se reconoce por el metodo.
  pub fn sent(&mut self, id: u64, method: &str) {
    self.pending.insert(id, method.to_string());
  }

  /// Mensaje del daemon. Devuelve las peticiones que hay que hacer en
  /// consecuencia (por ejemplo, releer la cola).
  pub fn on_daemon(&mut self, msg: DaemonMsg) -> Vec<Command> {
    match msg {
      DaemonMsg::Event { name, data } => match name.as_str() {
        "player.state" => {
          self.player = data;
          self.pos_at = Instant::now();
        }
        "player.position" => {
          if let (Some(obj), Some(pos)) = (self.player.as_object_mut(), data.get("position_ms")) {
            obj.insert("position_ms".into(), pos.clone());
          } else if self.player.is_null() {
            self.player = json!({"position_ms": data["position_ms"]});
          }
          self.pos_at = Instant::now();
        }
        "queue.changed" | "track.changed" => return vec![("queue.list".into(), json!({}))],
        _ => {}
      },
      DaemonMsg::Reply { id, result } => {
        let method = self.pending.remove(&id).unwrap_or_default();
        if let Ok(v) = result {
          match method.as_str() {
            "queue.list" => self.queue = v,
            "player.state" => {
              self.player = v;
              self.pos_at = Instant::now();
            }
            _ => {}
          }
        }
      }
      DaemonMsg::Closed => self.connected = false,
    }
    Vec::new()
  }

  /// Frame del visualizador.
  pub fn on_viz(&mut self, frame: Vec<f32>) {
    self.viz = frame;
  }

  pub fn playing(&self) -> bool {
    self.player["status"] == "playing"
  }

  /// Posicion actual: la ultima conocida mas lo que paso si esta sonando.
  pub fn position_ms(&self) -> u64 {
    let mut ms = self.player["position_ms"].as_u64().unwrap_or(0);
    if self.playing() {
      ms += self.pos_at.elapsed().as_millis() as u64;
    }
    match self.player["track"]["duration_ms"].as_u64() {
      Some(d) => ms.min(d),
      None => ms,
    }
  }

  /// Pista siguiente en la cola, si hay.
  pub fn next_track(&self) -> Option<&Value> {
    let current = self.queue["current"].as_u64()? as usize;
    self.queue["tracks"].get(current + 1)
  }
}
