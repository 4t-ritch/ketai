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

/// Panel con el foco en Biblioteca.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LibFocus {
  Playlists,
  Tracks,
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
  /// Pista de la que son las letras cargadas.
  pub lyrics_track: Option<String>,
  /// Capas ofrecidas (`lyrics.layers`).
  pub lyrics_layers: Vec<Value>,
  /// Paquete de `lyrics.get` (`null` si la pista no tiene letra).
  pub lyrics: Value,
  /// Ya llego la respuesta de `lyrics.get` para la pista actual.
  pub lyrics_loaded: bool,
  pub lyrics_primary: String,
  pub lyrics_secondary: Vec<String>,
  pub lyrics_estimate: bool,
  /// El usuario eligio las capas a mano: no se cambian solas.
  pub lyrics_chosen: bool,
  /// `playlist.list`: `{name, count}`.
  pub playlists: Vec<Value>,
  pub playlists_loaded: bool,
  pub lib_cursor: usize,
  pub lib_focus: LibFocus,
  /// Playlist abierta (la ultima pedida con `playlist.get`).
  pub lib_open: Option<String>,
  /// Resultado de `playlist.get`.
  pub playlist: Value,
  pub track_cursor: usize,
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
      lyrics_track: None,
      lyrics_layers: Vec::new(),
      lyrics: Value::Null,
      lyrics_loaded: false,
      lyrics_primary: "original".into(),
      lyrics_secondary: Vec::new(),
      lyrics_estimate: false,
      lyrics_chosen: false,
      playlists: Vec::new(),
      playlists_loaded: false,
      lib_cursor: 0,
      lib_focus: LibFocus::Playlists,
      lib_open: None,
      playlist: Value::Null,
      track_cursor: 0,
    }
  }
}

impl App {
  pub fn on_key(&mut self, key: KeyEvent) -> Vec<Command> {
    if self.tab == Tab::Lyrics
      && let Some(cmds) = self.lyrics_key(key)
    {
      return cmds;
    }
    if self.tab == Tab::Library
      && let Some(cmds) = self.library_key(key)
    {
      return cmds;
    }
    let cmd = |m: &str| vec![(m.to_string(), json!({}))];
    match key.code {
      KeyCode::Char('q') => self.quit = true,
      KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => self.quit = true,
      KeyCode::Char(c @ '1'..='4') => return self.go_to(Tab::ALL[c as usize - '1' as usize]),
      KeyCode::Tab => return self.go_to(Tab::ALL[(self.tab.index() + 1) % Tab::ALL.len()]),
      KeyCode::BackTab => return self.go_to(Tab::ALL[(self.tab.index() + Tab::ALL.len() - 1) % Tab::ALL.len()]),
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

  /// Teclas propias de la pestana Letras. `None` si la tecla no es suya.
  fn lyrics_key(&mut self, key: KeyEvent) -> Option<Vec<Command>> {
    let track = self.lyrics_track.clone()?;
    match key.code {
      KeyCode::Char(c @ ('[' | ']')) => {
        let step = if c == ']' { 50 } else { -50 };
        let offset = self.lyrics["user_offset_ms"].as_i64().unwrap_or(0) + step;
        Some(vec![("lyrics.set_offset".into(), json!({"track_id": track, "offset_ms": offset}))])
      }
      KeyCode::Char('t') => {
        self.lyrics_chosen = true;
        self.lyrics_secondary = if self.lyrics_secondary.is_empty() { self.first_translation().into_iter().collect() } else { Vec::new() };
        Some(vec![self.lyrics_get(&track)])
      }
      KeyCode::Char('o') => {
        // Capas que pueden ser principales: original y romanizadas.
        let ids: Vec<String> = self
          .lyrics_layers
          .iter()
          .filter(|l| l["kind"] != "translation")
          .filter_map(|l| l["id"].as_str().map(str::to_string))
          .collect();
        if ids.len() < 2 {
          return Some(Vec::new());
        }
        let at = ids.iter().position(|i| *i == self.lyrics_primary).unwrap_or(0);
        self.lyrics_primary = ids[(at + 1) % ids.len()].clone();
        self.lyrics_secondary.retain(|s| *s != self.lyrics_primary);
        self.lyrics_chosen = true;
        Some(vec![self.lyrics_get(&track)])
      }
      KeyCode::Char('w') => {
        self.lyrics_estimate = !self.lyrics_estimate;
        Some(vec![self.lyrics_get(&track)])
      }
      _ => None,
    }
  }

  /// Cambia de pestana y pide lo que esa pestana necesita la primera vez.
  fn go_to(&mut self, tab: Tab) -> Vec<Command> {
    self.tab = tab;
    if tab == Tab::Library && !self.playlists_loaded {
      return vec![("playlist.list".into(), json!({}))];
    }
    Vec::new()
  }

  fn open_selected(&mut self) -> Vec<Command> {
    let Some(name) = self.playlists.get(self.lib_cursor).and_then(|p| p["name"].as_str()).map(str::to_string) else {
      return Vec::new();
    };
    if self.lib_open.as_deref() == Some(&name) {
      return Vec::new();
    }
    self.lib_open = Some(name.clone());
    self.track_cursor = 0;
    vec![("playlist.get".into(), json!({"name": name}))]
  }

  /// Teclas de Biblioteca. `None` si la tecla no es suya.
  fn library_key(&mut self, key: KeyEvent) -> Option<Vec<Command>> {
    let up = matches!(key.code, KeyCode::Up | KeyCode::Char('k'));
    let down = matches!(key.code, KeyCode::Down | KeyCode::Char('j'));
    match self.lib_focus {
      LibFocus::Playlists => {
        if up || down {
          let max = self.playlists.len().saturating_sub(1);
          self.lib_cursor = if up { self.lib_cursor.saturating_sub(1) } else { (self.lib_cursor + 1).min(max) };
          return Some(self.open_selected());
        }
        if key.code == KeyCode::Enter && !self.playlists.is_empty() {
          self.lib_focus = LibFocus::Tracks;
          return Some(Vec::new());
        }
        None
      }
      LibFocus::Tracks => {
        let tracks = self.playlist["tracks"].as_array().map(|t| t.len()).unwrap_or(0);
        if up || down {
          self.track_cursor = if up { self.track_cursor.saturating_sub(1) } else { (self.track_cursor + 1).min(tracks.saturating_sub(1)) };
          return Some(Vec::new());
        }
        match key.code {
          KeyCode::Esc => {
            self.lib_focus = LibFocus::Playlists;
            Some(Vec::new())
          }
          KeyCode::Enter if tracks > 0 => {
            let name = self.lib_open.clone()?;
            Some(vec![
              ("playlist.load".into(), json!({"name": name, "replace": true})),
              ("queue.jump".into(), json!({"index": self.track_cursor})),
            ])
          }
          KeyCode::Char('a') => {
            let uri = self.playlist["tracks"][self.track_cursor]["uri"].as_str()?.to_string();
            Some(vec![("queue.add".into(), json!({"uris": [uri]}))])
          }
          _ => None,
        }
      }
    }
  }

  fn first_translation(&self) -> Option<String> {
    self.lyrics_layers.iter().find(|l| l["kind"] == "translation").and_then(|l| l["id"].as_str()).map(str::to_string)
  }

  fn lyrics_get(&self, track: &str) -> Command {
    (
      "lyrics.get".into(),
      json!({"track_id": track, "primary": self.lyrics_primary, "secondary": self.lyrics_secondary, "estimate_words": self.lyrics_estimate}),
    )
  }

  /// Si cambio la pista, se piden sus letras desde cero.
  fn track_changed(&mut self) -> Vec<Command> {
    let id = self.player["track"]["id"].as_str().map(str::to_string);
    if id == self.lyrics_track {
      return Vec::new();
    }
    self.lyrics_track = id.clone();
    self.lyrics = Value::Null;
    self.lyrics_layers.clear();
    self.lyrics_loaded = false;
    self.lyrics_primary = "original".into();
    self.lyrics_secondary.clear();
    self.lyrics_chosen = false;
    match id {
      Some(id) => vec![("lyrics.layers".into(), json!({"track_id": id})), self.lyrics_get(&id)],
      None => Vec::new(),
    }
  }

  /// Linea actual de la letra: la ultima que ya empezo.
  pub fn lyrics_line(&self) -> Option<usize> {
    let pos = self.position_ms();
    let lines = self.lyrics["lines"].as_array()?;
    lines.iter().rposition(|l| l["start_ms"].as_u64().is_some_and(|s| s <= pos))
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
          return self.track_changed();
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
        "playlist.changed" => {
          let mut cmds = vec![("playlist.list".into(), json!({}))];
          if let Some(open) = &self.lib_open
            && data["name"] == open.as_str()
          {
            cmds.push(("playlist.get".into(), json!({"name": open})));
          }
          return cmds;
        }
        "lyrics.changed" => {
          if let Some(track) = self.lyrics_track.clone()
            && data["track_id"] == track.as_str()
          {
            return vec![self.lyrics_get(&track)];
          }
        }
        _ => {}
      },
      DaemonMsg::Reply { id, result } => {
        let method = self.pending.remove(&id).unwrap_or_default();
        // Una letra que no se puede leer (1006) se muestra como sin letra.
        if method == "lyrics.get" && result.is_err() {
          self.lyrics = Value::Null;
          self.lyrics_loaded = true;
        }
        if let Ok(v) = result {
          match method.as_str() {
            "queue.list" => self.queue = v,
            "player.state" => {
              self.player = v;
              self.pos_at = Instant::now();
              return self.track_changed();
            }
            "lyrics.layers" => {
              self.lyrics_layers = v["layers"].as_array().cloned().unwrap_or_default();
              // Por defecto se muestra la primera traduccion debajo.
              if !self.lyrics_chosen
                && self.lyrics_secondary.is_empty()
                && let (Some(tr), Some(track)) = (self.first_translation(), self.lyrics_track.clone())
              {
                self.lyrics_secondary = vec![tr];
                return vec![self.lyrics_get(&track)];
              }
            }
            "lyrics.get" => {
              self.lyrics = v;
              self.lyrics_loaded = true;
            }
            "playlist.list" => {
              self.playlists = v["playlists"].as_array().cloned().unwrap_or_default();
              self.playlists_loaded = true;
              self.lib_cursor = self.lib_cursor.min(self.playlists.len().saturating_sub(1));
              return self.open_selected();
            }
            // Solo cuenta la respuesta de la playlist que sigue abierta.
            "playlist.get" if v["name"].as_str() == self.lib_open.as_deref() => {
              self.playlist = v;
              let n = self.playlist["tracks"].as_array().map(|t| t.len()).unwrap_or(0);
              self.track_cursor = self.track_cursor.min(n.saturating_sub(1));
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
