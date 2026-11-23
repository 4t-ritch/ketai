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

/// Vista de Biblioteca: playlists, todas las canciones o agrupadas.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LibView {
  #[default]
  Playlists,
  All,
  Artists,
  Albums,
  Folders,
  Years,
  Genres,
}

impl LibView {
  pub const ALL: [LibView; 7] =
    [LibView::Playlists, LibView::All, LibView::Artists, LibView::Albums, LibView::Folders, LibView::Years, LibView::Genres];

  pub fn label(self) -> &'static str {
    match self {
      LibView::Playlists => "Playlists",
      LibView::All => "Canciones",
      LibView::Artists => "Artistas",
      LibView::Albums => "Álbumes",
      LibView::Folders => "Carpetas",
      LibView::Years => "Años",
      LibView::Genres => "Géneros",
    }
  }

  /// `by` de `library.groups` / `library.tracks`.
  pub fn by(self) -> Option<&'static str> {
    match self {
      LibView::Artists => Some("artist"),
      LibView::Albums => Some("album"),
      LibView::Folders => Some("folder"),
      LibView::Years => Some("year"),
      LibView::Genres => Some("genre"),
      _ => None,
    }
  }
}

/// Desde donde se crea una playlist.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NameFor {
  /// Popup "Agregar a una playlist": se crea y se agrega la que suena.
  Add,
  /// Biblioteca: solo se crea.
  Library,
}

/// Nombre valido de playlist (el mismo control que hace ketaid).
fn valid_playlist_name(name: &str) -> bool {
  !name.is_empty() && !name.starts_with('.') && !name.contains('/')
}

/// Pistas por pedido en las vistas de biblioteca.
pub const LIB_LIMIT: u64 = 5000;

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
  pub lib_view: LibView,
  /// Grupos de la vista (`library.groups`).
  pub lib_groups: Vec<Value>,
  /// Pistas de la vista o del grupo elegido (`library.tracks`).
  pub lib_tracks: Vec<Value>,
  pub lib_total: u64,
  /// Pedidos en vuelo: solo cuenta la respuesta del ultimo.
  pub lib_groups_inflight: u32,
  pub lib_tracks_inflight: u32,
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
  /// La pista que suena es favorita (`track.playlists`), si ya se sabe.
  pub now_favorite: Option<bool>,
  /// Playlists donde esta la pista que suena.
  pub now_playlists: Vec<String>,
  /// Popup "Agregar a una playlist".
  pub add_open: bool,
  pub add_cursor: usize,
  /// Playlist del `playlist.add` en vuelo.
  pub add_pending: Option<String>,
  /// El daemon dijo que ya estaba (1009): se espera la decision.
  pub add_confirm: Option<String>,
  /// Nombre de una playlist nueva que se esta escribiendo, y desde donde.
  pub naming: Option<(String, NameFor)>,
  /// Aviso corto abajo (por ejemplo "Agregada a «rock»"); se borra con la
  /// proxima tecla.
  pub notice: Option<String>,
  /// La terminal muestra imagenes (protocolo kitty).
  pub images: bool,
  pub covers: crate::covers::CoverCache,
  /// `output.list` / `output.changed`: dispositivos del servidor de sonido.
  pub outputs: Vec<Value>,
  pub outputs_loaded: bool,
  /// Ya se pidio `output.list` (para no repetirlo en cada estado).
  pub outputs_asked: bool,
  /// Eleccion del usuario; `None` sigue el default del sistema.
  pub output_active: Option<String>,
  pub devices_cursor: usize,
  /// Pantalla de carpetas de la biblioteca abierta.
  pub folders_open: bool,
  /// `library.folders`: `{path, tracks}`.
  pub folders: Vec<Value>,
  pub folders_cursor: usize,
  /// Ruta que se esta escribiendo para agregar (`None` si no se escribe).
  pub folder_input: Option<String>,
  /// Se pidio quitar la carpeta bajo el cursor y falta confirmar.
  pub folder_confirm_remove: bool,
  /// Ultimo error del daemon en esta pantalla.
  pub folders_error: Option<String>,
  /// Escaneo en curso: (job, hecho, total).
  pub scan: Option<(String, u64, u64)>,
  /// Configuracion vigente (config.toml).
  pub config: crate::config::Config,
  /// Error del ultimo intento de leer config.toml (se mantiene la anterior).
  pub config_error: Option<String>,
  /// Busqueda pendiente: se manda cuando pasa la espera sin teclas nuevas.
  pub search_due: Option<Instant>,
  pub palette_due: Option<Instant>,
}

/// Espera despues de la ultima tecla antes de buscar: no se busca (ni se
/// piden caratulas) por cada letra intermedia.
pub const SEARCH_DELAY: std::time::Duration = std::time::Duration::from_millis(150);

/// Acciones de la busqueda rapida.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaletteAction {
  Devices,
  ScanAll,
  Lyrics,
  Folders,
}

impl PaletteAction {
  pub const ALL: [PaletteAction; 4] = [PaletteAction::Devices, PaletteAction::ScanAll, PaletteAction::Lyrics, PaletteAction::Folders];

  pub fn label(self) -> &'static str {
    match self {
      PaletteAction::Devices => "Cambiar salida de audio…",
      PaletteAction::ScanAll => "Escanear toda la biblioteca",
      PaletteAction::Lyrics => "Letras: elegir capas…",
      PaletteAction::Folders => "Carpetas de la biblioteca…",
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
      lib_view: LibView::Playlists,
      lib_groups: Vec::new(),
      lib_tracks: Vec::new(),
      lib_total: 0,
      lib_groups_inflight: 0,
      lib_tracks_inflight: 0,
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
      now_favorite: None,
      now_playlists: Vec::new(),
      add_open: false,
      add_cursor: 0,
      add_pending: None,
      add_confirm: None,
      notice: None,
      naming: None,
      images: false,
      covers: Default::default(),
      outputs: Vec::new(),
      outputs_loaded: false,
      outputs_asked: false,
      output_active: None,
      devices_cursor: 0,
      folders_open: false,
      folders: Vec::new(),
      folders_cursor: 0,
      folder_input: None,
      folder_confirm_remove: false,
      folders_error: None,
      scan: None,
      config: Default::default(),
      config_error: None,
      search_due: None,
      palette_due: None,
    }
  }
}

impl App {
  pub fn on_key(&mut self, key: KeyEvent) -> Vec<Command> {
    if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
      self.quit = true;
      return Vec::new();
    }
    if self.folders_open {
      return self.folders_key(key);
    }
    self.notice = None;
    if self.naming.is_some() {
      return self.naming_key(key);
    }
    if self.devices_open {
      return self.devices_key(key);
    }
    if self.add_open {
      return self.add_key(key);
    }
    if self.palette_open {
      return self.palette_key(key);
    }
    let typing = self.tab == Tab::Search && self.search_editing;
    use crate::keys::Action as A;
    let action = self.config.keys.action(&key, typing);
    // Popups y busqueda rapida: antes que las teclas de cada vista.
    match action {
      Some(A::Devices) => return self.open_devices(),
      Some(A::Favorite) => return self.toggle_favorite(),
      Some(A::AddToPlaylist) if self.now_uri().is_some() => {
        self.add_open = true;
        self.add_cursor = 0;
        self.add_confirm = None;
        if !self.playlists_loaded {
          return vec![("playlist.list".into(), json!({}))];
        }
        return Vec::new();
      }
      Some(A::Folders) => return self.open_folders(),
      // En Buscar, "/" vuelve a escribir (lo maneja la vista).
      Some(A::Actions) if !(self.tab == Tab::Search && key.code == KeyCode::Char('/')) => {
        self.palette_open = true;
        self.palette_query.clear();
        self.palette_results.clear();
        self.palette_cursor = 0;
        return Vec::new();
      }
      _ => {}
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
    let volume = |app: &App, step: f64| {
      let v = app.player["volume"].as_f64().unwrap_or(1.0) + step;
      let v = (v.clamp(0.0, 1.0) * 100.0).round() / 100.0;
      vec![("player.set_volume".into(), json!({"volume": v}))]
    };
    match action {
      Some(A::Quit) => self.quit = true,
      Some(A::TabNowPlaying) => return self.go_to(Tab::NowPlaying),
      Some(A::TabLyrics) => return self.go_to(Tab::Lyrics),
      Some(A::TabLibrary) => return self.go_to(Tab::Library),
      Some(A::TabSearch) => return self.go_to(Tab::Search),
      Some(A::NextTab) => return self.go_to(Tab::ALL[(self.tab.index() + 1) % Tab::ALL.len()]),
      Some(A::PrevTab) => return self.go_to(Tab::ALL[(self.tab.index() + Tab::ALL.len() - 1) % Tab::ALL.len()]),
      Some(A::Toggle) => return cmd("player.toggle"),
      Some(A::Next) => return cmd("player.next"),
      Some(A::Previous) => return cmd("player.previous"),
      Some(A::SeekForward) => return vec![("player.seek_relative".into(), json!({"offset_ms": 5000}))],
      Some(A::SeekBack) => return vec![("player.seek_relative".into(), json!({"offset_ms": -5000}))],
      Some(A::VolumeUp) => return volume(self, 0.05),
      Some(A::VolumeDown) => return volume(self, -0.05),
      Some(A::Mute) => {
        let muted = self.player["muted"].as_bool().unwrap_or(false);
        return vec![("player.set_muted".into(), json!({"muted": !muted}))];
      }
      Some(A::Shuffle) => {
        let on = self.player["shuffle"].as_bool().unwrap_or(false);
        return vec![("player.set_shuffle".into(), json!({"shuffle": !on}))];
      }
      Some(A::Repeat) => {
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

  /// Cambio el texto de Buscar: la busqueda sale despues de la espera.
  fn search(&mut self) -> Vec<Command> {
    self.search_done = false;
    self.search_cursor = 0;
    if self.search_query.trim().is_empty() {
      self.search_results.clear();
      self.search_due = None;
      return Vec::new();
    }
    self.search_due = Some(Instant::now() + SEARCH_DELAY);
    Vec::new()
  }

  /// Busquedas cuya espera ya paso. El loop principal lo llama seguido.
  pub fn tick(&mut self, now: Instant) -> Vec<Command> {
    let mut cmds = Vec::new();
    if self.search_due.is_some_and(|d| now >= d) {
      self.search_due = None;
      self.search_inflight.push_back((false, self.search_query.clone()));
      cmds.push(("library.search".into(), json!({"query": self.search_query, "limit": SEARCH_LIMIT})));
    }
    if self.palette_due.is_some_and(|d| now >= d) {
      self.palette_due = None;
      self.search_inflight.push_back((true, self.palette_query.clone()));
      cmds.push(("library.search".into(), json!({"query": self.palette_query, "limit": PALETTE_LIMIT})));
    }
    cmds
  }

  /// Hay una busqueda esperando (el loop despierta antes).
  pub fn search_pending(&self) -> bool {
    self.search_due.is_some() || self.palette_due.is_some()
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
  /// Nombre de la salida para mostrar: nunca vacio. "Default" si se sigue
  /// al default y todavia no se sabe su nombre; "No device" si no hay
  /// ninguna salida disponible.
  pub fn output_label(&self) -> String {
    if self.outputs_loaded && self.outputs.is_empty() {
      return "No device".into();
    }
    self.output_name().unwrap_or_else(|| "Default".into())
  }

  pub fn output_name(&self) -> Option<String> {
    let found = match &self.output_active {
      Some(id) => self.outputs.iter().find(|d| d["id"] == id.as_str()),
      None => self.outputs.iter().find(|d| d["is_default"] == true),
    };
    found.and_then(|d| d["name"].as_str()).map(str::to_string).or_else(|| self.output_active.clone())
  }

  fn open_folders(&mut self) -> Vec<Command> {
    self.folders_open = true;
    self.palette_open = false;
    self.devices_open = false;
    self.folder_input = None;
    self.folder_confirm_remove = false;
    self.folders_error = None;
    vec![("library.folders".into(), json!({}))]
  }

  fn selected_folder(&self) -> Option<String> {
    self.folders.get(self.folders_cursor).and_then(|f| f["path"].as_str()).map(str::to_string)
  }

  fn folders_key(&mut self, key: KeyEvent) -> Vec<Command> {
    // Escribiendo una ruta: las letras son texto.
    if let Some(input) = self.folder_input.as_mut() {
      match key.code {
        KeyCode::Char(c) => input.push(c),
        KeyCode::Backspace => {
          input.pop();
        }
        KeyCode::Esc => self.folder_input = None,
        KeyCode::Enter => {
          let typed = self.folder_input.take().unwrap_or_default();
          let typed = typed.trim();
          if typed.is_empty() {
            return Vec::new();
          }
          // `~` es la carpeta del usuario.
          let path = match typed.strip_prefix('~') {
            Some(rest) => format!("{}{rest}", std::env::var("HOME").unwrap_or_default()),
            None => typed.to_string(),
          };
          self.folders_error = None;
          return vec![("library.add_folder".into(), json!({"path": path}))];
        }
        _ => {}
      }
      return Vec::new();
    }
    if self.folder_confirm_remove {
      self.folder_confirm_remove = false;
      if key.code == KeyCode::Char('y')
        && let Some(path) = self.selected_folder()
      {
        return vec![("library.remove_folder".into(), json!({"path": path}))];
      }
      return Vec::new();
    }
    match key.code {
      KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('f') => self.folders_open = false,
      KeyCode::Up | KeyCode::Char('k') => self.folders_cursor = self.folders_cursor.saturating_sub(1),
      KeyCode::Down | KeyCode::Char('j') => self.folders_cursor = (self.folders_cursor + 1).min(self.folders.len().saturating_sub(1)),
      KeyCode::Char('a') => self.folder_input = Some(String::new()),
      KeyCode::Char('x') | KeyCode::Delete if !self.folders.is_empty() => self.folder_confirm_remove = true,
      KeyCode::Char('S') => {
        self.folders_error = None;
        return vec![("library.scan".into(), json!({}))];
      }
      KeyCode::Char('s') => {
        if let Some(path) = self.selected_folder() {
          self.folders_error = None;
          return vec![("library.scan".into(), json!({"path": path}))];
        }
      }
      _ => {}
    }
    Vec::new()
  }

  fn open_devices(&mut self) -> Vec<Command> {
    self.devices_open = true;
    self.palette_open = false;
    self.devices_cursor = 0;
    if self.outputs_loaded { Vec::new() } else { vec![("output.list".into(), json!({}))] }
  }

  /// URI de la pista que suena.
  pub fn now_uri(&self) -> Option<String> {
    self.player["track"]["uri"].as_str().map(str::to_string)
  }

  fn toggle_favorite(&mut self) -> Vec<Command> {
    let Some(uri) = self.now_uri() else { return Vec::new() };
    let favorite = !self.now_favorite.unwrap_or(false);
    // Se muestra enseguida; el daemon confirma con playlist.changed.
    self.now_favorite = Some(favorite);
    vec![("favorites.set".into(), json!({"uri": uri, "favorite": favorite}))]
  }

  /// Playlists a las que se puede agregar (sin Favoritos, que va con `*`).
  pub fn add_targets(&self) -> Vec<String> {
    self.playlists.iter().filter(|p| p["kind"].is_null()).filter_map(|p| p["name"].as_str().map(str::to_string)).collect()
  }

  /// Escribiendo el nombre de una playlist nueva: las letras son texto.
  fn naming_key(&mut self, key: KeyEvent) -> Vec<Command> {
    let Some((name, from)) = self.naming.as_mut() else { return Vec::new() };
    match key.code {
      KeyCode::Char(c) => name.push(c),
      KeyCode::Backspace => {
        name.pop();
      }
      KeyCode::Esc => self.naming = None,
      KeyCode::Enter => {
        let (name, from) = (name.trim().to_string(), *from);
        if !valid_playlist_name(&name) {
          self.notice = Some("Nombre inválido: no puede estar vacío, empezar con . ni tener /".into());
          return Vec::new();
        }
        self.naming = None;
        let exists = self.playlists.iter().any(|p| p["name"] == name.as_str());
        // playlist.save reemplaza: una que ya existe no se toca.
        let mut cmds = Vec::new();
        if exists {
          self.notice = Some(format!("Ya existe «{name}»"));
        } else {
          cmds.push(("playlist.save".into(), json!({"name": name, "from_queue": false})));
        }
        match from {
          NameFor::Add => {
            if let Some(uri) = self.now_uri() {
              self.add_pending = Some(name.clone());
              cmds.push(("playlist.add".into(), json!({"name": name, "uris": [uri]})));
            }
          }
          NameFor::Library if !exists => self.notice = Some(format!("Playlist «{name}» creada")),
          NameFor::Library => {}
        }
        return cmds;
      }
      _ => {}
    }
    Vec::new()
  }

  fn add_key(&mut self, key: KeyEvent) -> Vec<Command> {
    // Esperando la decision: ya estaba en la playlist.
    if let Some(name) = self.add_confirm.clone() {
      match key.code {
        KeyCode::Char('a') | KeyCode::Enter => {
          self.add_confirm = None;
          self.add_pending = Some(name.clone());
          let Some(uri) = self.now_uri() else { return Vec::new() };
          return vec![("playlist.add".into(), json!({"name": name, "uris": [uri], "duplicates": "add"}))];
        }
        KeyCode::Char('n') | KeyCode::Esc | KeyCode::Char('q') => {
          self.add_confirm = None;
          self.add_open = false;
        }
        _ => {}
      }
      return Vec::new();
    }
    let targets = self.add_targets();
    match key.code {
      KeyCode::Char('c') => self.naming = Some((String::new(), NameFor::Add)),
      KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('P') => self.add_open = false,
      KeyCode::Up | KeyCode::Char('k') => self.add_cursor = self.add_cursor.saturating_sub(1),
      KeyCode::Down | KeyCode::Char('j') => self.add_cursor = (self.add_cursor + 1).min(targets.len().saturating_sub(1)),
      KeyCode::Enter => {
        if let (Some(name), Some(uri)) = (targets.get(self.add_cursor), self.now_uri()) {
          self.add_pending = Some(name.clone());
          return vec![("playlist.add".into(), json!({"name": name, "uris": [uri]}))];
        }
      }
      _ => {}
    }
    Vec::new()
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
        self.palette_due = if self.palette_query.trim().is_empty() { None } else { Some(Instant::now() + SEARCH_DELAY) };
        return Vec::new();
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
          Some(PaletteItem::Action(PaletteAction::ScanAll)) => return vec![("library.scan".into(), json!({}))],
          Some(PaletteItem::Action(PaletteAction::Folders)) => return self.open_folders(),
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

  /// Pistas que muestra Biblioteca: las de la playlist abierta o las de la
  /// vista.
  pub fn lib_rows(&self) -> &[Value] {
    if self.lib_view == LibView::Playlists {
      self.playlist["tracks"].as_array().map(Vec::as_slice).unwrap_or(&[])
    } else {
      &self.lib_tracks
    }
  }

  /// Cambia de vista y pide lo que necesita.
  fn set_lib_view(&mut self, view: LibView) -> Vec<Command> {
    self.lib_view = view;
    self.lib_cursor = 0;
    self.track_cursor = 0;
    self.lib_focus = LibFocus::Playlists;
    self.lib_groups.clear();
    self.lib_tracks.clear();
    self.lib_total = 0;
    match view {
      LibView::Playlists => {
        self.lib_open = None;
        vec![("playlist.list".into(), json!({}))]
      }
      LibView::All => vec![("library.tracks".into(), json!({"limit": LIB_LIMIT}))],
      _ => vec![("library.groups".into(), json!({"by": view.by()}))],
    }
  }

  /// Pide las pistas del grupo elegido.
  fn open_group(&mut self) -> Vec<Command> {
    let (Some(by), Some(group)) = (self.lib_view.by(), self.lib_groups.get(self.lib_cursor)) else { return Vec::new() };
    self.track_cursor = 0;
    vec![("library.tracks".into(), json!({"by": by, "name": group["name"], "limit": LIB_LIMIT}))]
  }

  /// Teclas de Biblioteca. `None` si la tecla no es suya.
  fn library_key(&mut self, key: KeyEvent) -> Option<Vec<Command>> {
    let up = matches!(key.code, KeyCode::Up | KeyCode::Char('k'));
    let down = matches!(key.code, KeyCode::Down | KeyCode::Char('j'));
    // h / l: vista anterior o siguiente.
    if let KeyCode::Char(c @ ('h' | 'l')) = key.code {
      let views = LibView::ALL;
      let at = views.iter().position(|v| *v == self.lib_view).unwrap_or(0);
      let next = if c == 'l' { (at + 1) % views.len() } else { (at + views.len() - 1) % views.len() };
      return Some(self.set_lib_view(views[next]));
    }
    if key.code == KeyCode::Char('c') && self.lib_view == LibView::Playlists {
      self.naming = Some((String::new(), NameFor::Library));
      return Some(Vec::new());
    }
    match self.lib_focus {
      LibFocus::Playlists => {
        let side = match self.lib_view {
          LibView::Playlists => self.playlists.len(),
          LibView::All => 0,
          _ => self.lib_groups.len(),
        };
        if (up || down) && side > 0 {
          self.lib_cursor = if up { self.lib_cursor.saturating_sub(1) } else { (self.lib_cursor + 1).min(side - 1) };
          return Some(if self.lib_view == LibView::Playlists { self.open_selected() } else { self.open_group() });
        }
        if key.code == KeyCode::Enter && !self.lib_rows().is_empty() {
          self.lib_focus = LibFocus::Tracks;
          return Some(Vec::new());
        }
        None
      }
      LibFocus::Tracks => {
        let tracks = self.lib_rows().len();
        if up || down {
          self.track_cursor = if up { self.track_cursor.saturating_sub(1) } else { (self.track_cursor + 1).min(tracks.saturating_sub(1)) };
          return Some(Vec::new());
        }
        match key.code {
          KeyCode::Esc => {
            self.lib_focus = LibFocus::Playlists;
            Some(Vec::new())
          }
          KeyCode::Enter if tracks > 0 && self.lib_view == LibView::Playlists => {
            let name = self.lib_open.clone()?;
            Some(vec![
              ("playlist.load".into(), json!({"name": name, "replace": true})),
              ("queue.jump".into(), json!({"index": self.track_cursor})),
            ])
          }
          // La lista entera pasa a la cola, desde la cancion elegida.
          KeyCode::Enter if tracks > 0 => {
            let uris: Vec<Value> = self.lib_tracks.iter().map(|t| t["uri"].clone()).collect();
            Some(vec![
              ("queue.clear".into(), json!({})),
              ("queue.add".into(), json!({"uris": uris})),
              ("queue.jump".into(), json!({"index": self.track_cursor})),
            ])
          }
          KeyCode::Char('a') => {
            let uri = self.lib_rows().get(self.track_cursor)?["uri"].as_str()?.to_string();
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

  /// Recalcula el tema: config.toml mas los colores de la caratula que
  /// suena. Se llama al cambiar la configuracion, la pista o al llegar una
  /// caratula.
  pub fn refresh_theme(&mut self) {
    let palette = self.player["track"]["uri"].as_str().and_then(|u| self.covers.palette(u));
    self.theme = self.config.theme(palette);
  }

  /// Si cambio la pista, se piden sus letras desde cero.
  fn track_changed(&mut self) -> Vec<Command> {
    let id = self.player["track"]["id"].as_str().map(str::to_string);
    if id == self.lyrics_track {
      return Vec::new();
    }
    self.refresh_theme();
    self.lyrics_track = id.clone();
    self.lyrics = Value::Null;
    self.lyrics_layers.clear();
    self.lyrics_loaded = false;
    self.lyrics_primary = "original".into();
    self.lyrics_secondary.clear();
    self.lyrics_chosen = false;
    self.now_favorite = None;
    self.now_playlists.clear();
    let mut cmds = match id {
      Some(id) => vec![("lyrics.layers".into(), json!({"track_id": id})), self.lyrics_get(&id)],
      None => Vec::new(),
    };
    // Si es favorita y en que playlists esta.
    if let Some(uri) = self.now_uri() {
      cmds.push(("track.playlists".into(), json!({"uri": uri})));
    }
    cmds
  }

  /// Linea actual de la letra: la ultima que ya empezo.
  pub fn lyrics_line(&self) -> Option<usize> {
    let pos = self.position_ms();
    let lines = self.lyrics["lines"].as_array()?;
    lines.iter().rposition(|l| l["start_ms"].as_u64().is_some_and(|s| s <= pos))
  }

  /// Se envio la peticion `id`: su respuesta se reconoce por el metodo.
  pub fn sent(&mut self, id: u64, method: &str) {
    match method {
      "library.groups" => self.lib_groups_inflight += 1,
      "library.tracks" => self.lib_tracks_inflight += 1,
      _ => {}
    }
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
          let mut cmds = self.track_changed();
          // La salida se muestra en todas las vistas: la lista se pide una vez.
          if !self.outputs_loaded && !self.outputs_asked {
            self.outputs_asked = true;
            cmds.push(("output.list".into(), json!({})));
          }
          return cmds;
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
        "library.changed" => return vec![("library.folders".into(), json!({}))],
        "library.progress" => {
          if let Some((job, done, total)) = self.scan.as_mut()
            && data["job"] == job.as_str()
          {
            *done = data["done"].as_u64().unwrap_or(0);
            *total = data["total"].as_u64().unwrap_or(0);
            if *done >= *total {
              // Termino: las cantidades de canciones cambiaron.
              self.scan = None;
              return vec![("library.folders".into(), json!({}))];
            }
          }
        }
        "output.changed" => {
          self.outputs = data["devices"].as_array().cloned().unwrap_or_default();
          self.output_active = data["active"].as_str().map(str::to_string);
          self.outputs_loaded = true;
        }
        "playlist.changed" => {
          let mut cmds = vec![("playlist.list".into(), json!({}))];
          if let Some(uri) = self.now_uri() {
            cmds.push(("track.playlists".into(), json!({"uri": uri})));
          }
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
        // Solo la ultima respuesta de cada vista cuenta: una vieja (de otro
        // grupo) se descarta.
        let latest = match method.as_str() {
          "library.groups" => {
            self.lib_groups_inflight = self.lib_groups_inflight.saturating_sub(1);
            self.lib_groups_inflight == 0
          }
          "library.tracks" => {
            self.lib_tracks_inflight = self.lib_tracks_inflight.saturating_sub(1);
            self.lib_tracks_inflight == 0
          }
          _ => true,
        };
        // Una letra que no se puede leer (1006) se muestra como sin letra.
        if method == "lyrics.get" && result.is_err() {
          self.lyrics = Value::Null;
          self.lyrics_loaded = true;
        }
        if method == "playlist.add" && self.add_pending.is_some() {
          let name = self.add_pending.take().unwrap_or_default();
          match &result {
            Ok(_) => {
              self.add_open = false;
              self.notice = Some(format!("Agregada a «{name}»"));
            }
            // Ya estaba: se pregunta al usuario.
            Err(e) if e["code"] == 1009 => self.add_confirm = Some(name),
            Err(e) => {
              self.add_open = false;
              self.notice = Some(format!("No se pudo agregar: {}", e["message"].as_str().unwrap_or("error")));
            }
          }
        }
        // Errores de la pantalla de carpetas: se muestran en ella.
        if let (Err(e), "library.add_folder" | "library.remove_folder" | "library.scan") = (&result, method.as_str()) {
          self.folders_error = Some(e["message"].as_str().unwrap_or("error").to_string());
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
            "library.folders" => {
              self.folders = v["folders"].as_array().cloned().unwrap_or_default();
              self.folders_cursor = self.folders_cursor.min(self.folders.len().saturating_sub(1));
            }
            "library.scan" => {
              if let Some(job) = v["job"].as_str() {
                self.scan = Some((job.to_string(), 0, 0));
              }
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
              // En otras vistas el cursor es de los grupos.
              if self.lib_view == LibView::Playlists {
                self.lib_cursor = self.lib_cursor.min(self.playlists.len().saturating_sub(1));
                return self.open_selected();
              }
            }
            "track.playlists" => {
              self.now_favorite = v["favorite"].as_bool();
              self.now_playlists =
                v["playlists"].as_array().map(|l| l.iter().filter_map(|n| n.as_str().map(str::to_string)).collect()).unwrap_or_default();
            }
            "library.groups" if latest => {
              self.lib_groups = v["groups"].as_array().cloned().unwrap_or_default();
              self.lib_cursor = self.lib_cursor.min(self.lib_groups.len().saturating_sub(1));
              return self.open_group();
            }
            "library.tracks" if latest => {
              self.lib_tracks = v["tracks"].as_array().cloned().unwrap_or_default();
              self.lib_total = v["total"].as_u64().unwrap_or(self.lib_tracks.len() as u64);
              self.track_cursor = self.track_cursor.min(self.lib_tracks.len().saturating_sub(1));
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
