//! Estado de la app y reaccion a teclas y mensajes del daemon. No dibuja ni
//! hace I/O: devuelve las peticiones a enviar, asi se prueba sin terminal.

use std::{collections::HashMap, time::Instant};

use ratatui::{
  crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind},
  layout::Rect,
};
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

/// Algo clickeable de la pantalla. Lo registra el dibujo (`ui::take_hits`),
/// asi el mouse siempre coincide con lo que se ve.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Hit {
  Tab(Tab),
  /// Una accion global (controles, favorito, agregar).
  Act(crate::keys::Action),
  /// Barra de progreso: columna donde empieza y ancho.
  Seek { x: u16, w: u16 },
  /// Barra de volumen.
  Volume { x: u16, w: u16 },
  /// Una cancion de la cola de Reproduciendo (su posicion en la cola).
  QueueRow(usize),
  /// Una linea de la letra: salta a su inicio (ms).
  LyricLine(u64),
  LibView(LibView),
  /// Fila de la columna izquierda de Biblioteca (playlist o grupo).
  LibSide(usize),
  /// Toda la columna izquierda de Biblioteca (para la rueda).
  LibSideArea,
  /// Toda la lista de canciones de Biblioteca (para la rueda).
  LibListArea,
  /// Botones de la confirmacion de repetidas: agregar igual o no.
  Confirm(bool),
  /// Fila de la lista de canciones de Biblioteca.
  LibTrack(usize),
  /// Filtro de Buscar (Todo, Canciones, ...).
  SearchFilter(usize),
  /// Fila de resultados de Buscar.
  SearchRow(usize),
  /// Fila de un popup (salidas o agregar a playlist).
  PopupRow(usize),
  /// Dentro de un popup, fuera de sus filas: no hace nada.
  Popup,
  /// Fuera de un popup abierto: lo cierra.
  Backdrop,
}

/// Tiempo maximo entre dos clics para que sean doble clic.
pub const DOUBLE_CLICK: std::time::Duration = std::time::Duration::from_millis(400);

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
  /// Ultimo paso de la caida del visualizador.
  pub viz_at: Option<Instant>,
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
  /// Playlist del `playlist.remove` en vuelo (para el aviso).
  pub remove_pending: Option<String>,
  /// El daemon dijo que ya estaba (1009): se espera la decision.
  pub add_confirm: Option<String>,
  /// Nombre de una playlist nueva que se esta escribiendo, y desde donde.
  pub naming: Option<(String, NameFor)>,
  /// Milisegundos desde que arranco la TUI, para animar texto largo
  /// (`marquee`). Lo pone el loop antes de dibujar.
  pub anim_ms: u64,
  /// Lo clickeable del ultimo cuadro (ver `Hit`).
  pub hits: Vec<(Rect, Hit)>,
  /// Ultimo clic, para detectar doble clic.
  pub last_click: Option<(Hit, Instant)>,
  /// Barra que se esta arrastrando (progreso o volumen).
  pub dragging: Option<Hit>,
  /// Posicion que muestra la barra mientras se arrastra el progreso; el
  /// salto se pide al soltar.
  pub seek_preview: Option<u64>,
  /// Aviso corto abajo (por ejemplo "Agregada a «rock»"); se borra con la
  /// proxima tecla.
  pub notice: Option<String>,
  /// La terminal muestra imagenes (protocolo kitty).
  pub images: bool,
  pub covers: crate::covers::CoverCache,
  /// Modo de la vista de Letras (empieza como dice `[lyrics] mode`; `m`
  /// lo cambia).
  pub lyrics_mode: crate::config::LyricsMode,
  /// `config.toml`, donde `m` guarda el modo (`None` en tests: no se escribe).
  pub config_path: Option<std::path::PathBuf>,
  /// Cancion del popup "Agregar a una playlist" cuando no es la que suena
  /// (una de la cola); `None` es la que suena.
  pub add_uri: Option<String>,
  /// Playlists que ya tienen `add_uri` (como `now_playlists`).
  pub add_playlists: Vec<String>,
  /// De que cancion se pidio cada `track.playlists` (por id de peticion).
  pub asked_playlists: std::collections::HashMap<u64, String>,
  /// Cancion elegida en la cola de Reproduciendo (posicion en la cola).
  pub queue_cursor: usize,
  /// Arrastre de una cancion de la cola con el mouse.
  pub queue_drag: Option<QueueDrag>,
  /// Lista de atajos (desde Ctrl+P) abierta, y su desplazamiento.
  pub keys_open: bool,
  pub keys_scroll: u16,
  /// Desplazamiento de la lista de teclas repetidas.
  pub dup_scroll: u16,
  /// Ultimo `mode` leido del archivo.
  pub lyrics_mode_file: Option<crate::config::LyricsMode>,
  /// Tamano de una celda en pixeles, si la terminal lo informa (sin el, el
  /// modo karaoke no puede dibujar y se ve el clasico).
  pub cell_px: Option<(u16, u16)>,
  pub karaoke: crate::karaoke::Karaoke,
  /// Hay fuentes para el karaoke (se cargan al activarlo).
  pub karaoke_fonts: bool,
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
  Keys,
  RescanLyrics,
}

impl PaletteAction {
  pub const ALL: [PaletteAction; 6] = [
    PaletteAction::Devices,
    PaletteAction::ScanAll,
    PaletteAction::Lyrics,
    PaletteAction::Folders,
    PaletteAction::Keys,
    PaletteAction::RescanLyrics,
  ];

  pub fn label(self) -> &'static str {
    match self {
      PaletteAction::Devices => "Cambiar salida de audio…",
      PaletteAction::ScanAll => "Escanear toda la biblioteca",
      PaletteAction::Lyrics => "Letras: elegir capas…",
      PaletteAction::Folders => "Carpetas de la biblioteca…",
      PaletteAction::Keys => "Ver atajos de teclado…",
      PaletteAction::RescanLyrics => "Letras: volver a buscar",
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
/// Fraccion de la distancia al cuadro que sube cada barra por cuadro.
const VIZ_ATTACK: f32 = 0.5;
/// Altura (0..1) que baja por segundo al pausar: de arriba al suelo en ~0.4 s.
const VIZ_FALL_PER_SEC: f32 = 2.5;

/// `m:ss` o `h:mm:ss`.
pub fn fmt_ms(ms: u64) -> String {
  let s = ms / 1000;
  if s >= 3600 { format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60) } else { format!("{}:{:02}", s / 60, s % 60) }
}

/// Primer id de las imagenes del karaoke en tests; el programa usa
/// `karaoke_ids` (dentro del rango propio del proceso).
const KARAOKE_IDS: u32 = 900;

/// Las tres ultimas del rango de ids del proceso: las caratulas cuentan
/// desde el principio y nunca llegan ahi.
pub fn karaoke_ids(process_base: u32) -> u32 {
  process_base + (1 << 20) - 3
}

/// Tamano de celda a partir de lo que informa la terminal (columnas,
/// filas, ancho y alto en pixeles). `None` si falta algun dato.
pub fn cell_px(size: Option<(u16, u16, u16, u16)>) -> Option<(u16, u16)> {
  let (cols, rows, w, h) = size?;
  if cols == 0 || rows == 0 || w < cols || h < rows {
    return None;
  }
  Some((w / cols, h / rows))
}

/// Una cancion de la cola que se esta arrastrando: de donde sale y sobre
/// cual esta el puntero (posiciones en la cola).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QueueDrag {
  pub from: usize,
  pub over: usize,
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
      viz_at: None,
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
      remove_pending: None,
      notice: None,
      naming: None,
      anim_ms: 0,
      hits: Vec::new(),
      last_click: None,
      dragging: None,
      seek_preview: None,
      images: false,
      covers: Default::default(),
      lyrics_mode: Default::default(),
      lyrics_mode_file: None,
      dup_scroll: 0,
      keys_open: false,
      keys_scroll: 0,
      queue_cursor: 0,
      add_uri: None,
      add_playlists: Vec::new(),
      asked_playlists: Default::default(),
      queue_drag: None,
      config_path: None,
      cell_px: None,
      karaoke: crate::karaoke::Karaoke::new(KARAOKE_IDS, None),
      karaoke_fonts: false,
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
    if self.keys_blocked() {
      return self.blocked_key(key);
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
    if self.keys_open {
      return self.keys_list_key(key);
    }
    let typing = self.tab == Tab::Search && self.search_editing;
    use crate::keys::Action as A;
    let action = self.config.keys.action(&key, typing);
    // Popups y busqueda rapida: antes que las teclas de cada vista. En
    // Buscar, "/" vuelve a escribir (lo maneja la vista).
    let early = matches!(action, Some(A::Devices | A::Favorite | A::AddToPlaylist | A::Folders))
      || (action == Some(A::Actions) && !(self.tab == Tab::Search && key.code == KeyCode::Char('/')));
    if let (true, Some(a)) = (early, action) {
      return self.do_action(a);
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
    if self.tab == Tab::NowPlaying
      && let Some(cmds) = self.queue_key(key)
    {
      return cmds;
    }
    if self.tab == Tab::Library
      && let Some(cmds) = self.library_key(key)
    {
      return cmds;
    }
    match action {
      Some(a) => self.do_action(a),
      None => Vec::new(),
    }
  }

  /// Evento del mouse. Usa lo registrado en el ultimo cuadro: lo de arriba
  /// (un popup) gana sobre lo de atras.
  pub fn on_mouse(&mut self, ev: MouseEvent, now: Instant) -> Vec<Command> {
    // Con teclas repetidas solo se desplaza la lista.
    if self.keys_blocked() {
      match ev.kind {
        MouseEventKind::ScrollDown => self.dup_scroll = self.dup_scroll.saturating_add(1),
        MouseEventKind::ScrollUp => self.dup_scroll = self.dup_scroll.saturating_sub(1),
        _ => {}
      }
      return Vec::new();
    }
    let at = |r: &Rect| ev.column >= r.x && ev.column < r.x + r.width && ev.row >= r.y && ev.row < r.y + r.height;
    let hit = self.hits.iter().rev().find(|(r, _)| at(r)).map(|(r, h)| (*r, *h));
    let key = |c: KeyCode| KeyEvent::new(c, KeyModifiers::NONE);
    match ev.kind {
      MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => {
        let code = if ev.kind == MouseEventKind::ScrollUp { KeyCode::Up } else { KeyCode::Down };
        // La rueda mueve la lista bajo el puntero.
        match hit.map(|(_, h)| h) {
          Some(Hit::LibTrack(_) | Hit::LibListArea) => {
            self.lib_focus = LibFocus::Tracks;
            self.on_key(key(code))
          }
          Some(Hit::LibSide(_) | Hit::LibSideArea) => {
            self.lib_focus = LibFocus::Playlists;
            self.on_key(key(code))
          }
          Some(Hit::Popup) if self.keys_open => {
            self.keys_scroll = if code == KeyCode::Up { self.keys_scroll.saturating_sub(1) } else { self.keys_scroll.saturating_add(1) };
            Vec::new()
          }
          Some(Hit::PopupRow(_) | Hit::Popup) => self.on_key(key(code)),
          // Sobre la cola: mueve la elegida (y con ella lo que se ve).
          Some(Hit::QueueRow(_)) => {
            let last = self.queue_tracks().len().saturating_sub(1);
            self.queue_cursor = if code == KeyCode::Up { self.queue_cursor.saturating_sub(1) } else { (self.queue_cursor + 1).min(last) };
            Vec::new()
          }
          _ => Vec::new(),
        }
      }
      // Arrastrar una cancion de la cola: se marca donde caeria.
      MouseEventKind::Drag(MouseButton::Left) if self.queue_drag.is_some() => {
        if let (Some(drag), Some((_, Hit::QueueRow(j)))) = (self.queue_drag.as_mut(), hit) {
          drag.over = j;
        }
        Vec::new()
      }
      // Soltarla en otro lugar la mueve ahi.
      MouseEventKind::Up(MouseButton::Left) if self.queue_drag.is_some() => match self.queue_drag.take() {
        Some(QueueDrag { from, over }) if from != over => {
          self.queue_cursor = over;
          vec![("queue.move".into(), json!({"from": from, "to": over}))]
        }
        _ => Vec::new(),
      },
      // Arrastrar una barra: sigue al puntero aunque salga de la barra.
      MouseEventKind::Drag(MouseButton::Left) => match self.dragging {
        Some(h @ Hit::Seek { .. }) => {
          self.seek_preview = self.bar_value(h, ev.column);
          Vec::new()
        }
        Some(h @ Hit::Volume { .. }) => self.set_volume_at(h, ev.column),
        _ => Vec::new(),
      },
      MouseEventKind::Up(MouseButton::Left) => match self.dragging.take() {
        Some(Hit::Seek { .. }) => match self.seek_preview.take() {
          Some(ms) => {
            // Hasta que llegue la posicion nueva del daemon, se muestra esta.
            if let Some(obj) = self.player.as_object_mut() {
              obj.insert("position_ms".into(), json!(ms));
            }
            self.pos_at = Instant::now();
            vec![("player.seek".into(), json!({"position_ms": ms}))]
          }
          None => Vec::new(),
        },
        _ => Vec::new(),
      },
      MouseEventKind::Down(MouseButton::Left) => {
        let Some((_, hit)) = hit else { return Vec::new() };
        let double = self.last_click.is_some_and(|(h, t)| h == hit && now.duration_since(t) <= DOUBLE_CLICK);
        self.last_click = if double { None } else { Some((hit, now)) };
        self.notice = None;
        match hit {
          Hit::Tab(tab) => self.go_to(tab),
          Hit::Act(a) => self.do_action(a),
          // El progreso se arrastra: se salta al soltar.
          Hit::Seek { .. } => {
            self.dragging = Some(hit);
            self.seek_preview = self.bar_value(hit, ev.column);
            Vec::new()
          }
          Hit::Volume { .. } => {
            self.dragging = Some(hit);
            self.set_volume_at(hit, ev.column)
          }
          Hit::QueueRow(i) => {
            self.queue_cursor = i;
            // Doble clic: la reproduce. Si no, puede empezar a arrastrarse.
            if double {
              self.queue_drag = None;
              self.queue_play()
            } else {
              self.queue_drag = Some(QueueDrag { from: i, over: i });
              Vec::new()
            }
          }
          Hit::LyricLine(ms) => {
            // Hasta que llegue la posicion nueva del daemon, se muestra esta.
            if let Some(obj) = self.player.as_object_mut() {
              obj.insert("position_ms".into(), json!(ms));
            }
            self.pos_at = Instant::now();
            vec![("player.seek".into(), json!({"position_ms": ms}))]
          }
          Hit::LibSideArea | Hit::LibListArea => Vec::new(),
          Hit::Confirm(yes) => self.on_key(key(KeyCode::Char(if yes { 'a' } else { 'n' }))),
          Hit::LibView(v) => self.set_lib_view(v),
          Hit::LibSide(i) => {
            self.lib_focus = LibFocus::Playlists;
            self.lib_cursor = i;
            let cmds = if self.lib_view == LibView::Playlists { self.open_selected() } else { self.open_group() };
            if double {
              self.lib_focus = LibFocus::Tracks;
            }
            cmds
          }
          Hit::LibTrack(i) => {
            self.lib_focus = LibFocus::Tracks;
            self.track_cursor = i;
            // Doble clic: como Enter.
            if double { self.on_key(key(KeyCode::Enter)) } else { Vec::new() }
          }
          Hit::SearchFilter(i) => {
            self.search_editing = false;
            self.search_filter = i;
            self.search_cursor = 0;
            Vec::new()
          }
          Hit::SearchRow(i) => {
            self.search_editing = false;
            self.search_cursor = i;
            if double { self.on_key(key(KeyCode::Enter)) } else { Vec::new() }
          }
          Hit::PopupRow(i) => {
            if self.folders_open {
              // En Carpetas un clic elige; las acciones van con sus teclas.
              self.folders_cursor = i;
              return Vec::new();
            }
            if self.palette_open {
              self.palette_cursor = i;
            } else if self.devices_open {
              self.devices_cursor = i;
            } else if self.add_open {
              self.add_cursor = i;
            }
            self.on_key(key(KeyCode::Enter))
          }
          Hit::Popup => Vec::new(),
          Hit::Backdrop => {
            self.devices_open = false;
            self.add_open = false;
            self.add_confirm = None;
            self.naming = None;
            self.folders_open = false;
            self.palette_open = false;
            self.keys_open = false;
            Vec::new()
          }
        }
      }
      _ => Vec::new(),
    }
  }

  /// Posicion de una barra de progreso para la columna `col`.
  fn bar_value(&self, bar: Hit, col: u16) -> Option<u64> {
    let Hit::Seek { x, w } = bar else { return None };
    let dur = self.player["track"]["duration_ms"].as_u64()?;
    let frac = (col.saturating_sub(x) as f64 / w.max(1) as f64).clamp(0.0, 1.0);
    Some((dur as f64 * frac) as u64)
  }

  /// Volumen para la columna `col` de la barra de volumen.
  fn set_volume_at(&mut self, bar: Hit, col: u16) -> Vec<Command> {
    let Hit::Volume { x, w } = bar else { return Vec::new() };
    let frac = (col.saturating_sub(x) as f64 / w.saturating_sub(1).max(1) as f64).clamp(0.0, 1.0);
    let v = (frac * 100.0).round() / 100.0;
    // Se ve enseguida, sin esperar al daemon.
    if let Some(obj) = self.player.as_object_mut() {
      obj.insert("volume".into(), json!(v));
    }
    vec![("player.set_volume".into(), json!({"volume": v}))]
  }

  /// Ejecuta una accion global (de una tecla o de un clic).
  pub fn do_action(&mut self, action: crate::keys::Action) -> Vec<Command> {
    use crate::keys::Action as A;
    if action.is_lyrics() {
      return self.lyrics_action(action).unwrap_or_default();
    }
    if action.scope() == crate::keys::Scope::NowPlaying {
      return self.queue_action(action);
    }
    let cmd = |m: &str| vec![(m.to_string(), json!({}))];
    let volume = |app: &App, step: f64| {
      let v = app.player["volume"].as_f64().unwrap_or(1.0) + step;
      let v = (v.clamp(0.0, 1.0) * 100.0).round() / 100.0;
      vec![("player.set_volume".into(), json!({"volume": v}))]
    };
    match action {
      // Ya resueltas arriba.
      A::LyricsLayer
      | A::LyricsTranslation
      | A::LyricsOffsetDown
      | A::LyricsOffsetUp
      | A::LyricsMode
      | A::LyricsEstimate
      | A::QueueMoveUp
      | A::QueueMoveDown
      | A::QueueRemove
      | A::QueueAddToPlaylist => {}
      A::Devices => return self.open_devices(),
      A::Favorite => return self.toggle_favorite(),
      A::AddToPlaylist if self.now_uri().is_some() => {
        self.add_uri = None;
        self.add_open = true;
        self.add_cursor = 0;
        self.add_confirm = None;
        if !self.playlists_loaded {
          return vec![("playlist.list".into(), json!({}))];
        }
      }
      A::AddToPlaylist => {}
      A::Folders => return self.open_folders(),
      A::Actions => {
        self.palette_open = true;
        self.palette_query.clear();
        self.palette_results.clear();
        self.palette_cursor = 0;
      }
      A::Quit => self.quit = true,
      A::TabNowPlaying => return self.go_to(Tab::NowPlaying),
      A::TabLyrics => return self.go_to(Tab::Lyrics),
      A::TabLibrary => return self.go_to(Tab::Library),
      A::TabSearch => return self.go_to(Tab::Search),
      A::NextTab => return self.go_to(Tab::ALL[(self.tab.index() + 1) % Tab::ALL.len()]),
      A::PrevTab => return self.go_to(Tab::ALL[(self.tab.index() + Tab::ALL.len() - 1) % Tab::ALL.len()]),
      A::Toggle => return cmd("player.toggle"),
      A::Next => return cmd("player.next"),
      A::Previous => return cmd("player.previous"),
      A::SeekForward => return vec![("player.seek_relative".into(), json!({"offset_ms": 5000}))],
      A::SeekBack => return vec![("player.seek_relative".into(), json!({"offset_ms": -5000}))],
      A::VolumeUp => return volume(self, 0.05),
      A::VolumeDown => return volume(self, -0.05),
      A::Mute => {
        let muted = self.player["muted"].as_bool().unwrap_or(false);
        return vec![("player.set_muted".into(), json!({"muted": !muted}))];
      }
      A::Shuffle => {
        let on = self.player["shuffle"].as_bool().unwrap_or(false);
        return vec![("player.set_shuffle".into(), json!({"shuffle": !on}))];
      }
      A::Repeat => {
        let next = match self.player["repeat"].as_str() {
          Some("off") | None => "track",
          Some("track") => "queue",
          _ => "off",
        };
        return vec![("player.set_repeat".into(), json!({"repeat": next}))];
      }
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
    let action = self.config.keys.lyrics_action(&key)?;
    self.lyrics_action(action)
  }

  /// Una accion de Letras (por tecla o por clic en la cabecera).
  fn lyrics_action(&mut self, action: crate::keys::Action) -> Option<Vec<Command>> {
    use crate::keys::Action as A;
    if action == A::LyricsMode {
      use crate::config::LyricsMode;
      self.lyrics_mode = match self.lyrics_mode {
        LyricsMode::Classic => LyricsMode::Karaoke,
        LyricsMode::Karaoke => LyricsMode::Classic,
      };
      // Queda guardado: al recargar el archivo el modo ya coincide.
      if let Some(path) = &self.config_path
        && let Err(e) = crate::config::save_lyrics_mode(path, self.lyrics_mode)
      {
        self.notice = Some(format!("No se pudo guardar el modo en config.toml: {e}"));
      }
      return Some(Vec::new());
    }
    let track = self.lyrics_track.clone()?;
    match action {
      A::LyricsOffsetDown | A::LyricsOffsetUp => {
        let step = if action == A::LyricsOffsetUp { 50 } else { -50 };
        let offset = self.lyrics["user_offset_ms"].as_i64().unwrap_or(0) + step;
        Some(vec![("lyrics.set_offset".into(), json!({"track_id": track, "offset_ms": offset}))])
      }
      A::LyricsTranslation => {
        self.lyrics_chosen = true;
        self.lyrics_secondary = if self.lyrics_secondary.is_empty() { self.first_translation().into_iter().collect() } else { Vec::new() };
        Some(vec![self.lyrics_get(&track)])
      }
      A::LyricsLayer => {
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
      A::LyricsEstimate => {
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
    self.viz_fall(now);
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

  /// Cancion del popup de playlists: la elegida de la cola o la que suena.
  pub fn add_target(&self) -> Option<String> {
    self.add_uri.clone().or_else(|| self.now_uri())
  }

  /// Playlists que ya tienen la cancion del popup.
  pub fn add_marks(&self) -> &[String] {
    if self.add_uri.is_some() { &self.add_playlists } else { &self.now_playlists }
  }

  /// Abre "Agregar a una playlist" para una cancion de la cola (sin que
  /// tenga que estar sonando).
  fn open_add_for(&mut self, uri: String) -> Vec<Command> {
    self.add_open = true;
    self.add_cursor = 0;
    self.add_confirm = None;
    self.add_playlists.clear();
    self.add_uri = Some(uri.clone());
    let mut cmds = vec![("track.playlists".into(), json!({"uri": uri}))];
    if !self.playlists_loaded {
      cmds.push(("playlist.list".into(), json!({})));
    }
    cmds
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
            if let Some(uri) = self.add_target() {
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
          let Some(uri) = self.add_target() else { return Vec::new() };
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
      // Quitar la que suena de la playlist elegida (si esta ahi).
      KeyCode::Char('x') => {
        if let (Some(name), Some(uri)) = (targets.get(self.add_cursor), self.add_target())
          && self.add_marks().contains(name)
        {
          self.remove_pending = Some(name.clone());
          return vec![("playlist.remove".into(), json!({"name": name, "uris": [uri]}))];
        }
      }
      KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('P') => self.add_open = false,
      KeyCode::Up | KeyCode::Char('k') => self.add_cursor = self.add_cursor.saturating_sub(1),
      KeyCode::Down | KeyCode::Char('j') => self.add_cursor = (self.add_cursor + 1).min(targets.len().saturating_sub(1)),
      KeyCode::Enter => {
        if let (Some(name), Some(uri)) = (targets.get(self.add_cursor), self.add_target()) {
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
          Some(PaletteItem::Action(PaletteAction::RescanLyrics)) => {
            self.palette_open = false;
            return vec![("lyrics.rescan".into(), json!({}))];
          }
          Some(PaletteItem::Action(PaletteAction::Folders)) => return self.open_folders(),
          Some(PaletteItem::Action(PaletteAction::Keys)) => {
            self.palette_open = false;
            self.keys_open = true;
            self.keys_scroll = 0;
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
          // Quitar de la playlist abierta (solo en la vista Playlists).
          KeyCode::Char('x') if self.lib_view == LibView::Playlists && tracks > 0 => {
            let name = self.lib_open.clone()?;
            self.remove_pending = Some(name.clone());
            Some(vec![("playlist.remove".into(), json!({"name": name, "indexes": [self.track_cursor]}))])
          }
          KeyCode::Char('x') => Some(Vec::new()),
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
  /// La vista de Letras dibuja en modo karaoke: lo pidio el usuario, la
  /// terminal muestra imagenes y dice el tamano de sus celdas, y hay una
  /// fuente.
  pub fn karaoke_on(&self) -> bool {
    self.lyrics_mode == crate::config::LyricsMode::Karaoke && self.images && self.cell_px.is_some() && self.karaoke_fonts
  }

  /// La cola completa, en su orden real: las que ya sonaron, la que suena
  /// y las que vienen (con repetir toda la cola, al terminar vuelve al
  /// principio, asi que todas pueden volver a sonar).
  pub fn queue_tracks(&self) -> &[Value] {
    self.queue["tracks"].as_array().map(Vec::as_slice).unwrap_or(&[])
  }

  /// Posicion de la que suena en la cola.
  pub fn queue_current(&self) -> Option<usize> {
    self.queue["current"].as_u64().map(|c| c as usize)
  }

  /// Teclas de la cola en Reproduciendo (solo si se ve): flechas o j / k
  /// eligen, Enter reproduce, y las acciones `queue_*` configurables.
  fn queue_key(&mut self, key: KeyEvent) -> Option<Vec<Command>> {
    if !crate::ui::queue_visible() {
      return None;
    }
    if let Some(action) = self.config.keys.view_action(&key, crate::keys::Scope::NowPlaying) {
      return Some(self.queue_action(action));
    }
    let last = self.queue_tracks().len().saturating_sub(1);
    match key.code {
      KeyCode::Up | KeyCode::Char('k') => self.queue_cursor = self.queue_cursor.saturating_sub(1).min(last),
      KeyCode::Down | KeyCode::Char('j') => self.queue_cursor = (self.queue_cursor + 1).min(last),
      KeyCode::Enter => return Some(self.queue_play()),
      _ => return None,
    }
    Some(Vec::new())
  }

  /// Una accion de la cola sobre la cancion elegida (su posicion en la cola
  /// es la del cursor).
  fn queue_action(&mut self, action: crate::keys::Action) -> Vec<Command> {
    use crate::keys::Action as A;
    let len = self.queue_tracks().len();
    if len == 0 {
      return Vec::new();
    }
    let i = self.queue_cursor.min(len - 1);
    match action {
      A::QueueMoveUp if i > 0 => {
        self.queue_cursor = i - 1;
        vec![("queue.move".into(), json!({"from": i, "to": i - 1}))]
      }
      A::QueueMoveDown if i + 1 < len => {
        self.queue_cursor = i + 1;
        vec![("queue.move".into(), json!({"from": i, "to": i + 1}))]
      }
      // La que suena no se quita: para eso esta pasar a la siguiente.
      A::QueueRemove if Some(i) == self.queue_current() => {
        self.notice = Some("Es la que suena: pasá a la siguiente para cambiarla".into());
        Vec::new()
      }
      A::QueueRemove => {
        // La elegida pasa a ser la que queda en ese lugar (o la anterior si
        // era la ultima).
        self.queue_cursor = if i + 1 == len { i.saturating_sub(1) } else { i };
        vec![("queue.remove".into(), json!({"indexes": [i]}))]
      }
      A::QueueAddToPlaylist => match self.queue_tracks()[i]["uri"].as_str() {
        Some(uri) => self.open_add_for(uri.to_string()),
        None => Vec::new(),
      },
      _ => Vec::new(),
    }
  }

  /// Reproduce la cancion elegida de la cola.
  fn queue_play(&mut self) -> Vec<Command> {
    if self.queue_cursor >= self.queue_tracks().len() {
      return Vec::new();
    }
    vec![("queue.jump".into(), json!({"index": self.queue_cursor}))]
  }

  /// Hay un popup abierto (tapa parte de la vista).
  pub fn popup_open(&self) -> bool {
    self.palette_open || self.devices_open || self.folders_open || self.add_open || self.keys_open || self.keys_blocked()
  }

  /// Hay teclas repetidas en `[keys]`: la TUI no se puede usar hasta
  /// corregirlas (se desbloquea sola al guardar el archivo arreglado).
  pub fn keys_blocked(&self) -> bool {
    !self.config.keys.duplicates().is_empty()
  }

  /// Teclas con la lista de atajos abierta: desplazar y cerrar.
  fn keys_list_key(&mut self, key: KeyEvent) -> Vec<Command> {
    match key.code {
      KeyCode::Esc | KeyCode::Char('q') => self.keys_open = false,
      KeyCode::Down | KeyCode::Char('j') => self.keys_scroll = self.keys_scroll.saturating_add(1),
      KeyCode::Up | KeyCode::Char('k') => self.keys_scroll = self.keys_scroll.saturating_sub(1),
      KeyCode::PageDown => self.keys_scroll = self.keys_scroll.saturating_add(10),
      KeyCode::PageUp => self.keys_scroll = self.keys_scroll.saturating_sub(10),
      KeyCode::Home => self.keys_scroll = 0,
      _ => {}
    }
    Vec::new()
  }

  /// Teclas con el bloqueo de teclas repetidas: solo desplazar la lista y
  /// salir.
  fn blocked_key(&mut self, key: KeyEvent) -> Vec<Command> {
    match key.code {
      KeyCode::Char('q') => self.quit = true,
      KeyCode::Down | KeyCode::Char('j') => self.dup_scroll = self.dup_scroll.saturating_add(1),
      KeyCode::Up | KeyCode::Char('k') => self.dup_scroll = self.dup_scroll.saturating_sub(1),
      KeyCode::PageDown => self.dup_scroll = self.dup_scroll.saturating_add(10),
      KeyCode::PageUp => self.dup_scroll = self.dup_scroll.saturating_sub(10),
      KeyCode::Home => self.dup_scroll = 0,
      _ => {}
    }
    Vec::new()
  }

  /// Aplica `[lyrics]` de la configuracion (al iniciar y al recargarla).
  /// El modo del archivo se aplica al iniciar y cuando cambia `mode`; si se
  /// guarda otra cosa (por ejemplo `size`), queda el elegido con `m`.
  pub fn apply_lyrics_config(&mut self) {
    if self.lyrics_mode_file != Some(self.config.lyrics.mode) {
      self.lyrics_mode = self.config.lyrics.mode;
      self.lyrics_mode_file = Some(self.config.lyrics.mode);
    }
    self.karaoke.set_font(self.config.lyrics.font.clone());
  }

  pub fn lyrics_line(&self) -> Option<usize> {
    let pos = self.position_ms();
    let lines = self.lyrics["lines"].as_array()?;
    lines.iter().rposition(|l| l["start_ms"].as_u64().is_some_and(|s| s <= pos))
  }

  /// Se envio la peticion `id`: su respuesta se reconoce por el metodo.
  /// Como `sent`, con los parametros: recuerda de que cancion se pidio cada
  /// `track.playlists` (la respuesta no lo dice).
  pub fn sent_with(&mut self, id: u64, method: &str, params: &Value) {
    if method == "track.playlists"
      && let Some(uri) = params["uri"].as_str()
    {
      self.asked_playlists.insert(id, uri.to_string());
    }
    self.sent(id, method);
  }

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
            // Puede ser un offset o un archivo de letra nuevo o editado.
            return vec![("lyrics.layers".into(), json!({"track_id": track})), self.lyrics_get(&track)];
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
        if method == "playlist.remove" && let Some(name) = self.remove_pending.take() {
          self.notice = Some(match &result {
            Ok(_) => format!("Quitada de «{name}»"),
            Err(e) => format!("No se pudo quitar: {}", e["message"].as_str().unwrap_or("error")),
          });
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
            "queue.list" => {
              // Al cambiar la que suena, la elegida pasa a ser esa.
              let before = self.queue_current();
              self.queue = v;
              let len = self.queue_tracks().len();
              if self.queue_current() != before
                && let Some(c) = self.queue_current()
              {
                self.queue_cursor = c;
              }
              self.queue_cursor = self.queue_cursor.min(len.saturating_sub(1));
            }
            "player.state" => {
              self.player = v;
              self.pos_at = Instant::now();
              return self.track_changed();
            }
            "lyrics.layers" => {
              self.lyrics_layers = v["layers"].as_array().cloned().unwrap_or_default();
              // Si una capa elegida ya no existe (se borro su archivo), se
              // vuelve a la original sin pedir capas que darian error.
              let offered = |id: &str| self.lyrics_layers.iter().any(|l| l["id"] == id);
              let stale = !self.lyrics_layers.is_empty()
                && (!offered(&self.lyrics_primary) || self.lyrics_secondary.iter().any(|s| !offered(s)));
              if stale && let Some(track) = self.lyrics_track.clone() {
                if !offered(&self.lyrics_primary) {
                  self.lyrics_primary = "original".into();
                }
                let keep: Vec<String> = self.lyrics_secondary.iter().filter(|s| offered(s)).cloned().collect();
                self.lyrics_secondary = keep;
                return vec![self.lyrics_get(&track)];
              }
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
              let list: Vec<String> =
                v["playlists"].as_array().map(|l| l.iter().filter_map(|n| n.as_str().map(str::to_string)).collect()).unwrap_or_default();
              // Sin cancion anotada, es la que suena (como antes).
              let asked = self.asked_playlists.remove(&id);
              if asked.is_none() || asked == self.now_uri() {
                self.now_favorite = v["favorite"].as_bool();
                self.now_playlists = list.clone();
              }
              if asked.is_some() && asked == self.add_uri {
                self.add_playlists = list;
              }
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

  /// Frame del visualizador. `viz` es lo que se dibuja: sube hacia el
  /// cuadro desde la altura actual (asi se recupera desde la caida) y baja
  /// con el audio. Sin reproducir se ignoran los cuadros y `tick` lo deja
  /// caer.
  pub fn on_viz(&mut self, frame: Vec<f32>) {
    if !self.playing() {
      return;
    }
    if self.viz.len() != frame.len() {
      self.viz = vec![0.0; frame.len()];
    }
    for (shown, target) in self.viz.iter_mut().zip(frame) {
      *shown = if target > *shown { *shown + (target - *shown) * VIZ_ATTACK } else { target };
    }
    self.viz_at = None;
  }

  /// Hay barras cayendo: el loop despierta seguido para animarlas.
  pub fn viz_animating(&self) -> bool {
    !self.playing() && self.viz.iter().any(|v| *v > 0.0)
  }

  /// Caida de las barras cuando no se reproduce.
  fn viz_fall(&mut self, now: Instant) {
    if !self.viz_animating() {
      self.viz_at = None;
      return;
    }
    let dt = self.viz_at.map_or(0.0, |at| now.saturating_duration_since(at).as_secs_f32());
    self.viz_at = Some(now);
    for v in &mut self.viz {
      *v = (*v - VIZ_FALL_PER_SEC * dt).max(0.0);
    }
  }

  pub fn playing(&self) -> bool {
    self.player["status"] == "playing"
  }

  /// Posicion actual: la ultima conocida mas lo que paso si esta sonando.
  pub fn position_ms(&self) -> u64 {
    if let Some(p) = self.seek_preview {
      return p;
    }
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
