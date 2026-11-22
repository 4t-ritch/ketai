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
  /// Texto de la pestana Buscar.
  pub search_query: String,
  /// Se esta escribiendo (las letras son texto, no atajos).
  pub search_editing: bool,
  /// Resultados de `library.search` para `search_query`.
  pub search_results: Vec<Value>,
  /// Ya llego una respuesta para la consulta actual.
  pub search_done: bool,
  pub search_cursor: usize,
  /// Filtro: 0 Todo, 1 Canciones, 2 Playlists, 3 Archivos locales.
  pub search_filter: usize,
  /// Consultas enviadas sin respuesta, en orden: (de la paleta, texto).
  /// El daemon responde en el orden en que llegan las peticiones.
  pub search_inflight: std::collections::VecDeque<(bool, String)>,
  /// Busqueda rapida abierta.
  pub palette_open: bool,
  pub palette_query: String,
  pub palette_results: Vec<Value>,
  pub palette_cursor: usize,
  /// Selector de salida de audio abierto.
  pub devices_open: bool,
  /// La terminal muestra imagenes (protocolo kitty).
  pub images: bool,
  pub covers: crate::covers::CoverCache,
  /// `output.list` / `output.changed`: dispositivos del servidor de sonido.
  pub outputs: Vec<Value>,
  pub outputs_loaded: bool,
  /// Eleccion del usuario; `None` sigue el default del sistema.
  pub output_active: Option<String>,
  pub devices_cursor: usize,
}

/// Acciones de la busqueda rapida.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaletteAction {
  Devices,
  ScanMusic,
  Lyrics,
}

impl PaletteAction {
  pub const ALL: [PaletteAction; 3] = [PaletteAction::Devices, PaletteAction::ScanMusic, PaletteAction::Lyrics];

  pub fn label(self) -> &'static str {
    match self {
      PaletteAction::Devices => "Cambiar salida de audio…",
      PaletteAction::ScanMusic => "Escanear ~/Music",
      PaletteAction::Lyrics => "Letras: elegir capas…",
    }
  }
}

/// Una fila de la busqueda rapida.
#[derive(Debug, Clone, PartialEq)]
pub enum PaletteItem {
  Song(Value),
  Playlist(String),
  Action(PaletteAction),
}

/// Nombres de los filtros de Buscar.
pub const SEARCH_FILTERS: [&str; 4] = ["Todo", "Canciones", "Playlists", "Archivos locales"];
const SEARCH_LIMIT: u64 = 50;
const PALETTE_LIMIT: u64 = 5;

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
      search_query: String::new(),
      search_editing: false,
      search_results: Vec::new(),
      search_done: false,
      search_cursor: 0,
      search_filter: 0,
      search_inflight: Default::default(),
      palette_open: false,
      palette_query: String::new(),
      palette_results: Vec::new(),
      palette_cursor: 0,
      devices_open: false,
      images: false,
      covers: Default::default(),
      outputs: Vec::new(),
      outputs_loaded: false,
      output_active: None,
      devices_cursor: 0,
    }
  }
}

impl App {
  pub fn on_key(&mut self, key: KeyEvent) -> Vec<Command> {
    if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
      self.quit = true;
      return Vec::new();
    }
    if self.devices_open {
      return self.devices_key(key);
    }
    if self.palette_open {
      return self.palette_key(key);
    }
    let typing = self.tab == Tab::Search && self.search_editing;
    if key.code == KeyCode::Char('d') && !typing {
      return self.open_devices();
    }
    let ctrl_k = key.code == KeyCode::Char('k') && key.modifiers.contains(KeyModifiers::CONTROL);
    if ctrl_k || (key.code == KeyCode::Char('/') && self.tab != Tab::Search) {
      self.palette_open = true;
      self.palette_query.clear();
      self.palette_results.clear();
      self.palette_cursor = 0;
      return Vec::new();
    }
    if self.tab == Tab::Search
      && let Some(cmds) = self.search_key(key)
    {
      return cmds;
    }
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

  fn search(&mut self) -> Vec<Command> {
    self.search_done = false;
    self.search_cursor = 0;
    if self.search_query.trim().is_empty() {
      self.search_results.clear();
      return Vec::new();
    }
    self.search_inflight.push_back((false, self.search_query.clone()));
    vec![("library.search".into(), json!({"query": self.search_query, "limit": SEARCH_LIMIT}))]
  }

  /// Canciones que muestra Buscar con el filtro actual.
  pub fn search_tracks(&self) -> &[Value] {
    if self.search_filter == 2 { &[] } else { &self.search_results }
  }

  /// Playlists cuyo nombre contiene la consulta.
  pub fn search_playlists(&self) -> Vec<&Value> {
    let q = self.search_query.to_lowercase();
    if q.trim().is_empty() || self.search_filter == 1 || self.search_filter == 3 {
      return Vec::new();
    }
    self.playlists.iter().filter(|p| p["name"].as_str().is_some_and(|n| n.to_lowercase().contains(q.trim()))).collect()
  }

  /// Agrega la pista a la cola y la reproduce.
  fn play_uri(&self, uri: &str) -> Vec<Command> {
    let index = self.queue["tracks"].as_array().map(|t| t.len()).unwrap_or(0);
    vec![("queue.add".into(), json!({"uris": [uri]})), ("queue.jump".into(), json!({"index": index}))]
  }

  /// Teclas de Buscar. `None` si la tecla no es suya.
  fn search_key(&mut self, key: KeyEvent) -> Option<Vec<Command>> {
    if self.search_editing {
      return match key.code {
        KeyCode::Char(c) => {
          self.search_query.push(c);
          Some(self.search())
        }
        KeyCode::Backspace => {
          self.search_query.pop();
          Some(self.search())
        }
        KeyCode::Esc | KeyCode::Down | KeyCode::Enter => {
          self.search_editing = false;
          Some(Vec::new())
        }
        // Tab y BackTab cambian de pestana aunque se este escribiendo.
        _ => None,
      };
    }
    let n = self.search_tracks().len();
    match key.code {
      KeyCode::Char('/') => {
        self.search_editing = true;
        Some(Vec::new())
      }
      KeyCode::Up | KeyCode::Char('k') => {
        if self.search_cursor == 0 {
          self.search_editing = true;
        } else {
          self.search_cursor -= 1;
        }
        Some(Vec::new())
      }
      KeyCode::Down | KeyCode::Char('j') => {
        self.search_cursor = (self.search_cursor + 1).min(n.saturating_sub(1));
        Some(Vec::new())
      }
      KeyCode::Char(c @ ('h' | 'l')) => {
        let len = SEARCH_FILTERS.len();
        self.search_filter = if c == 'l' { (self.search_filter + 1) % len } else { (self.search_filter + len - 1) % len };
        self.search_cursor = 0;
        Some(Vec::new())
      }
      KeyCode::Enter => {
        let uri = self.search_tracks().get(self.search_cursor)?["uri"].as_str()?.to_string();
        Some(self.play_uri(&uri))
      }
      KeyCode::Char('a') => {
        let uri = self.search_tracks().get(self.search_cursor)?["uri"].as_str()?.to_string();
        Some(vec![("queue.add".into(), json!({"uris": [uri]}))])
      }
      _ => None,
    }
  }

  /// Opciones del selector de salida: (id, nombre). La actual va primero;
  /// `None` es seguir el default del sistema.
  pub fn device_rows(&self) -> Vec<(Option<String>, String)> {
    let name_of = |id: &str| {
      self.outputs.iter().find(|d| d["id"] == id).and_then(|d| d["name"].as_str()).unwrap_or(id).to_string()
    };
    let default = (None, "Predeterminado del sistema".to_string());
    let mut rows = Vec::new();
    match &self.output_active {
      Some(id) => rows.push((Some(id.clone()), name_of(id))),
      None => rows.push(default.clone()),
    }
    if self.output_active.is_some() {
      rows.push(default);
    }
    for d in &self.outputs {
      let Some(id) = d["id"].as_str() else { continue };
      if self.output_active.as_deref() != Some(id) {
        rows.push((Some(id.to_string()), name_of(id)));
      }
    }
    rows
  }

  /// Nombre del dispositivo que suena: el elegido o el default.
  pub fn output_name(&self) -> Option<String> {
    let found = match &self.output_active {
      Some(id) => self.outputs.iter().find(|d| d["id"] == id.as_str()),
      None => self.outputs.iter().find(|d| d["is_default"] == true),
    };
    found.and_then(|d| d["name"].as_str()).map(str::to_string).or_else(|| self.output_active.clone())
  }

  fn open_devices(&mut self) -> Vec<Command> {
    self.devices_open = true;
    self.palette_open = false;
    self.devices_cursor = 0;
    if self.outputs_loaded { Vec::new() } else { vec![("output.list".into(), json!({}))] }
  }

  fn devices_key(&mut self, key: KeyEvent) -> Vec<Command> {
    let rows = self.device_rows();
    match key.code {
      KeyCode::Esc | KeyCode::Char('d') | KeyCode::Char('q') => self.devices_open = false,
      KeyCode::Up | KeyCode::Char('k') => self.devices_cursor = self.devices_cursor.saturating_sub(1),
      KeyCode::Down | KeyCode::Char('j') => self.devices_cursor = (self.devices_cursor + 1).min(rows.len().saturating_sub(1)),
      KeyCode::Enter => {
        self.devices_open = false;
        if let Some((id, _)) = rows.get(self.devices_cursor) {
          return vec![("output.set".into(), json!({"id": id}))];
        }
      }
      _ => {}
    }
    Vec::new()
  }

  /// Filas de la busqueda rapida: canciones, playlists y acciones.
  pub fn palette_items(&self) -> Vec<PaletteItem> {
    let q = self.palette_query.trim().to_lowercase();
    let mut items: Vec<PaletteItem> = Vec::new();
    if !q.is_empty() {
      items.extend(self.palette_results.iter().cloned().map(PaletteItem::Song));
      items.extend(
        self
          .playlists
          .iter()
          .filter_map(|p| p["name"].as_str())
          .filter(|n| n.to_lowercase().contains(&q))
          .take(3)
          .map(|n| PaletteItem::Playlist(n.to_string())),
      );
    }
    // Las acciones siempre estan, debajo de los resultados (maqueta 3).
    items.extend(PaletteAction::ALL.into_iter().map(PaletteItem::Action));
    items
  }

  fn palette_key(&mut self, key: KeyEvent) -> Vec<Command> {
    let items = self.palette_items();
    let selected = items.get(self.palette_cursor).cloned();
    match key.code {
      KeyCode::Esc => self.palette_open = false,
      KeyCode::Up => self.palette_cursor = self.palette_cursor.saturating_sub(1),
      KeyCode::Down => self.palette_cursor = (self.palette_cursor + 1).min(items.len().saturating_sub(1)),
      KeyCode::Backspace | KeyCode::Char(_) => {
        match key.code {
          KeyCode::Char(c) => self.palette_query.push(c),
          _ => {
            self.palette_query.pop();
          }
        }
        self.palette_cursor = 0;
        self.palette_results.clear();
        if self.palette_query.trim().is_empty() {
          return Vec::new();
        }
        self.search_inflight.push_back((true, self.palette_query.clone()));
        return vec![("library.search".into(), json!({"query": self.palette_query, "limit": PALETTE_LIMIT}))];
      }
      KeyCode::Tab => {
        if let Some(PaletteItem::Song(t)) = selected
          && let Some(uri) = t["uri"].as_str()
        {
          return vec![("queue.add".into(), json!({"uris": [uri]}))];
        }
      }
      KeyCode::Enter => {
        self.palette_open = false;
        match selected {
          Some(PaletteItem::Song(t)) => {
            if let Some(uri) = t["uri"].as_str() {
              return self.play_uri(uri);
            }
          }
          Some(PaletteItem::Playlist(name)) => {
            return vec![("playlist.load".into(), json!({"name": name, "replace": true})), ("queue.jump".into(), json!({"index": 0}))];
          }
          Some(PaletteItem::Action(PaletteAction::Devices)) => return self.open_devices(),
          Some(PaletteItem::Action(PaletteAction::Lyrics)) => self.tab = Tab::Lyrics,
          Some(PaletteItem::Action(PaletteAction::ScanMusic)) => {
            let home = std::env::var("HOME").unwrap_or_default();
            return vec![("library.scan".into(), json!({"path": format!("{home}/Music")}))];
          }
          None => self.palette_open = true,
        }
      }
      _ => {}
    }
    Vec::new()
  }

  /// Cambia de pestana y pide lo que esa pestana necesita la primera vez.
  fn go_to(&mut self, tab: Tab) -> Vec<Command> {
    self.tab = tab;
    if tab == Tab::Search {
      self.search_editing = true;
    }
    // Biblioteca y Buscar muestran la columna de playlists.
    if matches!(tab, Tab::Library | Tab::Search) && !self.playlists_loaded {
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
          self.output_active = data["output"].as_str().map(str::to_string);
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
        "output.changed" => {
          self.outputs = data["devices"].as_array().cloned().unwrap_or_default();
          self.output_active = data["active"].as_str().map(str::to_string);
          self.outputs_loaded = true;
        }
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
        // Una busqueda fallida igual consume su lugar en la fila.
        if method == "library.search" && result.is_err() {
          self.search_inflight.pop_front();
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
            "output.list" => {
              self.outputs = v["devices"].as_array().cloned().unwrap_or_default();
              self.outputs_loaded = true;
            }
            "library.search" => {
              let tracks = v["tracks"].as_array().cloned().unwrap_or_default();
              match self.search_inflight.pop_front() {
                Some((true, q)) if q == self.palette_query => self.palette_results = tracks,
                Some((false, q)) if q == self.search_query => {
                  self.search_results = tracks;
                  self.search_done = true;
                }
                _ => {}
              }
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
