//! Metodos `player.*` y `queue.*` (docs/protocol.md seccion 5).
//!
//! Todo este estado lo toca solo el loop central. El I/O (leer metadatos,
//! abrir audio) ocurre en otros threads y vuelve como mensajes:
//! - `engine::Event` desde el motor de audio.
//! - `Resolved` desde el thread que lee metadatos para `queue.add`.

use std::{
  sync::mpsc::Sender,
  thread,
  time::Instant,
};

use ketai_core::{
  uri::file_uri_to_path,
  engine::{self, Backend, Command, EngineHandle, Event},
  metadata::{self, TrackInfo},
  queue::{CurrentAfterRemove, Queue, QueueError, Repeat, Track},
};
use ketai_protocol::{ErrorReply, Request, codes};
use serde_json::{Map, Value, json};

use crate::{
  server::{ClientId, Message},
  session::Saved,
};

/// Intervalo minimo entre eventos `player.position`.
const POSITION_EVENT_INTERVAL_MS: u128 = 1000;

/// "Anterior" reinicia la pista si ya sonaron mas de estos milisegundos.
const RESTART_THRESHOLD_MS: u64 = 3000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Status {
  Stopped,
  Playing,
  Paused,
}

impl Status {
  fn as_str(self) -> &'static str {
    match self {
      Status::Stopped => "stopped",
      Status::Playing => "playing",
      Status::Paused => "paused",
    }
  }
}

/// Resultado del thread que resuelve URIs para `queue.add`.
pub struct Resolved {
  client: ClientId,
  request_id: u64,
  position: Option<usize>,
  tracks: Result<Vec<Track>, ErrorReply>,
}

/// Resultado de restaurar la sesion guardada.
pub struct Restored {
  saved: Saved,
  /// Una entrada por URI guardada; `None` si ya no se puede reproducir.
  tracks: Vec<Option<Track>>,
}

/// Trabajo pendiente para el thread de metadatos.
enum ResolveJob {
  Add { client: ClientId, request_id: u64, position: Option<usize>, uris: Vec<String>, first_id: u64 },
  Restore { saved: Saved, first_id: u64 },
}

pub struct PlayerService {
  queue: Queue,
  engine: EngineHandle,
  /// Un unico thread de metadatos: resuelve los `queue.add` en el orden en
  /// que llegaron, asi se aplican en ese mismo orden.
  resolver: Sender<ResolveJob>,
  status: Status,
  /// Generacion de la ultima carga enviada al motor.
  generation: u64,
  /// La pista actual esta cargada en el motor (playing o paused).
  loaded: bool,
  /// Ultima posicion conocida y cuando se conocio.
  position_ms: u64,
  position_at: Instant,
  duration_ms: Option<u64>,
  volume: f64,
  muted: bool,
  /// Contador de ids de pista, compartido con el thread de playlists para
  /// que los ids sean unicos en todo el daemon.
  track_ids: TrackIds,
  /// Fallos de carga seguidos, para no girar sin fin sobre una cola rota.
  consecutive_failures: usize,
  /// Ultimo estado informado a los suscriptores, para detectar cambios.
  published: Published,
  /// Se esta restaurando la sesion guardada: las peticiones de reproductor y
  /// cola esperan, y no se guarda nada (pisaria la sesion a restaurar).
  restoring: bool,
  /// Salida elegida (la maneja OutputService); `None` sigue el default.
  output: Option<String>,
}

/// Lo ultimo que se publico por eventos.
struct Published {
  /// `PlayerState` sin `position_ms` (la posicion tiene su propio evento).
  state: Value,
  track_id: Option<String>,
  queue_ids: Vec<String>,
  position_at: Instant,
  /// Forzar un `player.position` en la proxima publicacion (tras un seek).
  force_position: bool,
}

impl PlayerService {
  pub fn new(tx: Sender<Message>, ring: ketai_core::viz::SharedRing) -> Self {
    let (events_tx, events_rx) = std::sync::mpsc::channel();
    let engine = engine::spawn(Backend::from_env(), events_tx, ring);
    // Reenvia los eventos del motor al loop central.
    let forward = tx.clone();
    thread::spawn(move || {
      for ev in events_rx {
        if forward.send(Message::Engine(ev)).is_err() {
          return;
        }
      }
    });
    let (resolver, jobs) = std::sync::mpsc::channel::<ResolveJob>();
    let resolved_tx = tx.clone();
    thread::spawn(move || {
      for job in jobs {
        let msg = match job {
          ResolveJob::Add { client, request_id, position, uris, first_id } => {
            // Un archivo malformado puede hacer entrar en panico al decoder.
            // Sin esto, el thread moriria y ningun queue.add volveria a
            // responder.
            let tracks = std::panic::catch_unwind(|| resolve(request_id, &uris, first_id))
              .unwrap_or_else(|_| Err(ErrorReply::new(Some(request_id), 1003, "source unavailable: decoder failure")));
            Message::Resolved(Resolved { client, request_id, position, tracks })
          }
          ResolveJob::Restore { saved, first_id } => {
            // Al restaurar no es todo o nada: cada pista que falla se omite.
            let tracks = saved
              .queue
              .iter()
              .enumerate()
              .map(|(n, uri)| {
                let one = std::slice::from_ref(uri);
                std::panic::catch_unwind(|| resolve(0, one, first_id + n as u64))
                  .ok()
                  .and_then(Result::ok)
                  .and_then(|mut v| v.pop())
              })
              .collect();
            Message::Restored(Restored { saved, tracks })
          }
        };
        if resolved_tx.send(msg).is_err() {
          return;
        }
      }
    });
    let seed = std::time::SystemTime::now()
      .duration_since(std::time::UNIX_EPOCH)
      .map(|d| d.as_nanos() as u64)
      .unwrap_or(1);
    let mut service = Self {
      queue: Queue::new(seed),
      engine,
      resolver,
      status: Status::Stopped,
      generation: 0,
      loaded: false,
      position_ms: 0,
      position_at: Instant::now(),
      duration_ms: None,
      volume: 1.0,
      muted: false,
      track_ids: TrackIds::default(),
      consecutive_failures: 0,
      published: Published {
        state: Value::Null,
        track_id: None,
        queue_ids: Vec::new(),
        position_at: Instant::now(),
        force_position: false,
      },
      restoring: false,
      output: None,
    };
    service.published.state = service.state_without_position();
    service
  }

  /// Empieza a restaurar una sesion guardada (en el thread de metadatos).
  pub fn restore(&mut self, saved: Saved) {
    let first_id = self.track_ids.reserve(saved.queue.len());
    self.restoring = true;
    let _ = self.resolver.send(ResolveJob::Restore { saved, first_id });
  }

  pub fn restoring(&self) -> bool {
    self.restoring
  }

  /// Aplica la sesion restaurada. Nunca reproduce: queda en pausa en la
  /// posicion guardada (o detenida si estaba detenida).
  pub fn on_restored(&mut self, r: Restored) {
    self.restoring = false;
    let saved = r.saved;
    let mut tracks = Vec::new();
    // Indice nuevo de la pista actual y si es la misma que la guardada.
    let mut current: Option<(usize, bool)> = None;
    for (i, track) in r.tracks.into_iter().enumerate() {
      let Some(track) = track else { continue };
      if let Some(saved_cur) = saved.current
        && current.is_none()
        && i >= saved_cur
      {
        current = Some((tracks.len(), i == saved_cur));
      }
      tracks.push(track);
    }
    let _ = self.queue.add(tracks, None);
    self.volume = saved.volume;
    self.muted = saved.muted;
    self.apply_volume();
    self.queue.set_repeat(match saved.repeat.as_str() {
      "track" => Repeat::Track,
      "queue" => Repeat::Queue,
      _ => Repeat::Off,
    });
    if let Some((index, same)) = current {
      let _ = self.queue.jump(index);
      if saved.status == "stopped" {
        self.status = Status::Stopped;
      } else {
        let start = if same { saved.position_ms } else { 0 };
        self.load_current_at(false, start);
      }
    }
    self.queue.set_shuffle(saved.shuffle);
  }

  /// Sesion para guardar en disco, con la posicion actual.
  pub fn session_json(&self) -> Value {
    json!({
      "version": crate::session::VERSION,
      "queue": self.queue.tracks().iter().map(|t| t.uri.clone()).collect::<Vec<_>>(),
      "current": self.queue.current(),
      "position_ms": if self.status == Status::Stopped { 0 } else { self.current_position() },
      "volume": self.volume,
      "muted": self.muted,
      "repeat": repeat_str(self.queue.repeat()),
      "shuffle": self.queue.shuffle(),
      "output": self.output,
      "status": self.status.as_str(),
    })
  }

  /// URI y ruta de una pista de la cola, por id.
  pub fn find_track(&self, id: &str) -> Option<(String, Option<std::path::PathBuf>)> {
    self.queue.tracks().iter().find(|t| t.id == id).map(|t| (t.uri.clone(), t.path.clone()))
  }

  /// `track_id` y ruta local de la pista actual (None si no hay o es remota).
  pub fn current_local(&self) -> Option<(String, std::path::PathBuf)> {
    let t = self.queue.tracks().get(self.queue.current()?)?;
    Some((t.id.clone(), t.path.clone()?))
  }

  pub fn track_ids(&self) -> TrackIds {
    self.track_ids.clone()
  }

  /// URIs de la cola, en orden (para `playlist.save`).
  pub fn queue_uris(&self) -> Vec<String> {
    self.queue.tracks().iter().map(|t| t.uri.clone()).collect()
  }

  /// Aplica `playlist.load`: reemplaza la cola (y detiene) o agrega al final.
  pub fn load_tracks(&mut self, tracks: Vec<Track>, replace: bool) {
    if replace {
      self.queue.clear();
      self.halt();
    }
    let _ = self.queue.add(tracks, None);
  }

  pub fn set_output(&mut self, output: Option<String>) {
    self.output = output;
  }

  /// Largo de la cola y pista actual, para saber si hay siguiente.
  pub fn queue_position(&self) -> (usize, Option<usize>) {
    (self.queue.tracks().len(), self.queue.current())
  }

  pub fn is_playing(&self) -> bool {
    self.status == Status::Playing
  }

  /// Estado actual para enviar al suscribirse al topico `player`.
  pub fn state_event(&self) -> (&'static str, Value) {
    ("player.state", self.state_json())
  }

  /// Compara con lo ultimo publicado y devuelve los eventos a enviar, como
  /// `(topico, nombre, datos)`. Se llama despues de cada mensaje.
  pub fn take_events(&mut self) -> Vec<(&'static str, &'static str, Value)> {
    let mut events = Vec::new();

    let queue_ids: Vec<String> = self.queue.tracks().iter().map(|t| t.id.clone()).collect();
    if queue_ids != self.published.queue_ids {
      self.published.queue_ids = queue_ids;
      events.push(("queue", "queue.changed", json!({})));
    }

    let track_id = self.queue.current_track().map(|t| t.id.clone());
    if track_id != self.published.track_id {
      self.published.track_id = track_id;
      events.push(("player", "track.changed", json!({"track": self.queue.current_track().map(track_json)})));
    }

    let state = self.state_without_position();
    if state != self.published.state {
      self.published.state = state;
      events.push(("player", "player.state", self.state_json()));
    }

    let due = self.status == Status::Playing
      && self.published.position_at.elapsed().as_millis() >= POSITION_EVENT_INTERVAL_MS;
    if self.published.force_position || due {
      self.published.force_position = false;
      self.published.position_at = Instant::now();
      events.push(("player", "player.position", json!({"position_ms": self.current_position()})));
    }
    events
  }

  fn state_without_position(&self) -> Value {
    let mut state = self.state_json();
    if let Some(obj) = state.as_object_mut() {
      obj.remove("position_ms");
    }
    state
  }

  /// Atiende un metodo. `None` significa que la respuesta llega despues
  /// (por `on_resolved`).
  pub fn handle(&mut self, client: ClientId, req: &Request) -> Option<Result<Value, ErrorReply>> {
    let id = req.id;
    let p = Params { id, map: &req.params };
    let result = match req.method.as_str() {
      "player.state" => Ok(self.state_json()),
      "player.play" => self.play(id),
      "player.pause" => self.pause(),
      "player.toggle" => match self.status {
        Status::Playing => self.pause(),
        _ => self.play(id),
      },
      "player.stop" => self.stop(),
      "player.next" => self.next(id),
      "player.previous" => self.previous(id),
      "player.seek" => p.u64("position_ms").and_then(|ms| self.seek(id, ms)),
      "player.seek_relative" => p.i64("offset_ms").and_then(|off| {
        let target = (self.current_position() as i64 + off).max(0) as u64;
        self.seek(id, target)
      }),
      "player.set_volume" => p.unit_f64("volume").map(|v| {
        self.volume = v;
        self.apply_volume();
        json!({})
      }),
      "player.set_muted" => p.bool("muted").map(|m| {
        self.muted = m;
        self.apply_volume();
        json!({})
      }),
      "player.set_repeat" => p.repeat().map(|r| {
        self.queue.set_repeat(r);
        json!({})
      }),
      "player.set_shuffle" => p.bool("shuffle").map(|s| {
        self.queue.set_shuffle(s);
        json!({})
      }),
      "queue.list" => Ok(self.queue_json()),
      "queue.add" => return self.add(client, &p),
      "queue.remove" => p.indexes().and_then(|ix| self.remove(id, &ix)),
      "queue.move" => p.u64("from").and_then(|from| {
        let to = p.u64("to")?;
        self.queue.move_track(from as usize, to as usize).map_err(|e| queue_error(id, e))?;
        Ok(json!({}))
      }),
      "queue.clear" => {
        self.queue.clear();
        self.halt();
        Ok(json!({}))
      }
      "queue.jump" => p.u64("index").and_then(|i| {
        self.queue.jump(i as usize).map_err(|e| queue_error(id, e))?;
        self.load_current(true);
        Ok(json!({}))
      }),
      _ => Err(ErrorReply::new(Some(id), codes::METHOD_NOT_FOUND, "method not found")),
    };
    Some(result)
  }

  pub fn handles(method: &str) -> bool {
    method.starts_with("player.") || method.starts_with("queue.")
  }

  // ---- reproductor ----------------------------------------------------------

  fn play(&mut self, id: u64) -> Result<Value, ErrorReply> {
    match self.status {
      Status::Playing => {}
      Status::Paused if self.loaded => {
        self.engine.send(Command::Play);
        self.set_position(self.position_ms);
        self.status = Status::Playing;
      }
      _ => {
        if self.queue.select_first().is_none() {
          return Err(nothing_to_play(id));
        }
        self.load_current(true);
      }
    }
    Ok(json!({}))
  }

  fn pause(&mut self) -> Result<Value, ErrorReply> {
    if self.status == Status::Playing {
      let pos = self.current_position();
      self.engine.send(Command::Pause);
      self.set_position(pos);
      self.status = Status::Paused;
    }
    Ok(json!({}))
  }

  fn stop(&mut self) -> Result<Value, ErrorReply> {
    self.halt();
    Ok(json!({}))
  }

  /// Detiene el motor y deja la posicion en 0. La pista sigue seleccionada.
  fn halt(&mut self) {
    self.engine.send(Command::Stop);
    self.status = Status::Stopped;
    self.loaded = false;
    // Invalida eventos pendientes de la carga anterior.
    self.generation += 1;
    self.set_position(0);
  }

  fn next(&mut self, id: u64) -> Result<Value, ErrorReply> {
    if self.queue.next().is_none() {
      return Err(nothing_to_play(id));
    }
    self.after_selection_change();
    Ok(json!({}))
  }

  fn previous(&mut self, id: u64) -> Result<Value, ErrorReply> {
    if self.queue.current().is_none() {
      return Err(nothing_to_play(id));
    }
    if self.loaded && self.current_position() > RESTART_THRESHOLD_MS {
      return self.seek(id, 0);
    }
    if self.queue.previous().is_none() {
      // Primera pista: se reinicia.
      return if self.loaded { self.seek(id, 0) } else { Ok(json!({})) };
    }
    self.after_selection_change();
    Ok(json!({}))
  }

  /// Tras cambiar la pista seleccionada, mantiene el estado de reproduccion:
  /// si sonaba sigue sonando, si estaba en pausa queda en pausa al inicio.
  fn after_selection_change(&mut self) {
    match self.status {
      Status::Playing => self.load_current(true),
      Status::Paused => self.load_current(false),
      Status::Stopped => self.set_position(0),
    }
  }

  fn seek(&mut self, id: u64, ms: u64) -> Result<Value, ErrorReply> {
    if !self.loaded {
      return Err(nothing_to_play(id));
    }
    let target = match self.duration_ms {
      Some(d) => ms.min(d),
      None => ms,
    };
    self.engine.send(Command::Seek(target));
    self.set_position(target);
    self.published.force_position = true;
    Ok(json!({}))
  }

  fn apply_volume(&mut self) {
    let v = if self.muted { 0.0 } else { self.volume };
    self.engine.send(Command::Volume(v as f32));
  }

  /// Carga la pista seleccionada en el motor desde el principio.
  fn load_current(&mut self, play: bool) {
    self.load_current_at(play, 0);
  }

  /// Carga la pista seleccionada en el motor desde `start_ms`.
  fn load_current_at(&mut self, play: bool, start_ms: u64) {
    let Some(track) = self.queue.current_track() else {
      self.halt();
      return;
    };
    self.generation += 1;
    self.duration_ms = track.info.duration_ms;
    match track.path.clone() {
      Some(path) => {
        let start_ms = match self.duration_ms {
          Some(d) => start_ms.min(d),
          None => start_ms,
        };
        self.engine.send(Command::Load { generation: self.generation, path, start_ms, play });
        self.loaded = true;
        self.status = if play { Status::Playing } else { Status::Paused };
        self.set_position(start_ms);
      }
      // Origen remoto: todavia no soportado (T13). Se trata como fallo.
      None => {
        let generation = self.generation;
        self.on_engine(Event::Failed { generation, message: "remote sources not supported".into() });
      }
    }
  }

  pub fn on_engine(&mut self, event: Event) {
    match event {
      Event::Loaded { generation, duration_ms } if generation == self.generation => {
        self.consecutive_failures = 0;
        if duration_ms.is_some() {
          self.duration_ms = duration_ms;
        }
      }
      Event::Position { generation, position_ms } if generation == self.generation => {
        self.set_position(position_ms);
      }
      Event::Ended { generation } if generation == self.generation => {
        self.loaded = false;
        self.advance_after_end();
      }
      Event::Failed { generation, message } if generation == self.generation => {
        eprintln!("ketaid: could not play track: {message}");
        self.loaded = false;
        self.consecutive_failures += 1;
        if self.consecutive_failures >= self.queue.len().max(1) {
          self.consecutive_failures = 0;
          self.halt();
        } else {
          self.advance_after_end();
        }
      }
      // Evento de una carga anterior: se descarta.
      _ => {}
    }
  }

  fn advance_after_end(&mut self) {
    let was_playing = self.status == Status::Playing;
    if self.queue.next_on_end().is_some() {
      self.load_current(was_playing);
    } else {
      self.halt();
    }
  }

  fn set_position(&mut self, ms: u64) {
    self.position_ms = ms;
    self.position_at = Instant::now();
  }

  /// Posicion estimada: la ultima conocida mas lo que paso desde entonces.
  fn current_position(&self) -> u64 {
    let mut pos = self.position_ms;
    if self.status == Status::Playing {
      pos += self.position_at.elapsed().as_millis() as u64;
    }
    match self.duration_ms {
      Some(d) => pos.min(d),
      None => pos,
    }
  }

  // ---- cola -----------------------------------------------------------------

  fn add(&mut self, client: ClientId, p: &Params) -> Option<Result<Value, ErrorReply>> {
    let id = p.id;
    let uris = match p.uris() {
      Ok(u) => u,
      Err(e) => return Some(Err(e)),
    };
    let position = match p.optional_u64("position") {
      Ok(pos) => pos.map(|v| v as usize),
      Err(e) => return Some(Err(e)),
    };
    let first_id = self.track_ids.reserve(uris.len());
    // Leer metadatos es I/O de disco: se hace fuera del loop central.
    let _ = self.resolver.send(ResolveJob::Add { client, request_id: id, position, uris, first_id });
    None
  }

  /// Aplica un `queue.add` ya resuelto y devuelve la respuesta para el cliente.
  pub fn on_resolved(&mut self, r: Resolved) -> (ClientId, u64, Result<Value, ErrorReply>) {
    let result = r.tracks.and_then(|tracks| {
      let added = self.queue.add(tracks, r.position).map_err(|e| queue_error(r.request_id, e))?;
      Ok(json!({"added": added}))
    });
    (r.client, r.request_id, result)
  }

  fn remove(&mut self, id: u64, indexes: &[usize]) -> Result<Value, ErrorReply> {
    match self.queue.remove(indexes).map_err(|e| queue_error(id, e))? {
      CurrentAfterRemove::Kept => {}
      CurrentAfterRemove::Replaced => match self.status {
        Status::Playing => self.load_current(true),
        Status::Paused => self.load_current(false),
        Status::Stopped => {}
      },
      CurrentAfterRemove::Gone => self.halt(),
    }
    Ok(json!({"removed": indexes.len()}))
  }

  // ---- JSON -----------------------------------------------------------------

  fn state_json(&self) -> Value {
    json!({
      "status": self.status.as_str(),
      "track": self.queue.current_track().map(track_json),
      "position_ms": if self.status == Status::Stopped { 0 } else { self.current_position() },
      "volume": self.volume,
      "muted": self.muted,
      "repeat": repeat_str(self.queue.repeat()),
      "shuffle": self.queue.shuffle(),
      "output": self.output,
    })
  }

  fn queue_json(&self) -> Value {
    json!({
      "current": self.queue.current(),
      "tracks": self.queue.tracks().iter().map(track_json).collect::<Vec<_>>(),
    })
  }
}

fn repeat_str(r: Repeat) -> &'static str {
  match r {
    Repeat::Off => "off",
    Repeat::Track => "track",
    Repeat::Queue => "queue",
  }
}

pub fn track_json(t: &Track) -> Value {
  let i = &t.info;
  json!({
    "id": t.id,
    "source": if t.path.is_some() { "local" } else { "remote" },
    "uri": t.uri,
    "title": i.title,
    "artist": i.artist,
    "album": i.album,
    "duration_ms": i.duration_ms,
    "has_cover": i.has_cover,
    "codec": i.codec,
    "sample_rate": i.sample_rate,
    "channels": i.channels,
    "bitrate_kbps": Value::Null,
  })
}

/// Contador de ids de pista (`t_<hex>`), compartible entre threads.
#[derive(Clone, Default)]
pub struct TrackIds(std::sync::Arc<std::sync::atomic::AtomicU64>);

impl TrackIds {
  /// Reserva `n` ids seguidos y devuelve el primero.
  pub fn reserve(&self, n: usize) -> u64 {
    self.0.fetch_add(n as u64, std::sync::atomic::Ordering::Relaxed)
  }
}

pub fn track_id(n: u64) -> String {
  format!("t_{n:x}")
}

/// Pista para mostrar aunque el archivo ya no exista o no se pueda leer: en
/// ese caso los metadatos quedan vacios.
pub fn resolve_lenient(uri: &str, n: u64) -> Track {
  let path = file_uri_to_path(uri);
  let info = path.as_deref().and_then(|p| metadata::probe(p).ok()).unwrap_or_default();
  Track { id: track_id(n), uri: uri.to_string(), path, info }
}

/// Resuelve todas las URIs o ninguna (la cola no cambia si una falla).
pub fn resolve(id: u64, uris: &[String], first_id: u64) -> Result<Vec<Track>, ErrorReply> {
  uris
    .iter()
    .enumerate()
    .map(|(n, uri)| {
      let track_id = track_id(first_id + n as u64);
      if uri.starts_with("http://") || uri.starts_with("https://") {
        return Ok(Track { id: track_id, uri: uri.clone(), path: None, info: TrackInfo::default() });
      }
      let path = file_uri_to_path(uri).ok_or_else(|| invalid_params(id, format!("unsupported uri: {uri}")))?;
      let info = metadata::probe(&path)
        .map_err(|e| ErrorReply::new(Some(id), 1003, format!("source unavailable: {uri}: {e}")))?;
      Ok(Track { id: track_id, uri: uri.clone(), path: Some(path), info })
    })
    .collect()
}


fn nothing_to_play(id: u64) -> ErrorReply {
  ErrorReply::new(Some(id), 1001, "nothing to play")
}

fn invalid_params(id: u64, msg: impl Into<String>) -> ErrorReply {
  ErrorReply::new(Some(id), codes::INVALID_PARAMS, msg)
}

fn queue_error(id: u64, e: QueueError) -> ErrorReply {
  match e {
    QueueError::OutOfRange => ErrorReply::new(Some(id), 1002, "index out of range"),
    QueueError::Duplicate => invalid_params(id, "duplicate index"),
  }
}

/// Lectura validada de `params`.
struct Params<'a> {
  id: u64,
  map: &'a Map<String, Value>,
}

impl Params<'_> {
  fn err(&self, key: &str) -> ErrorReply {
    invalid_params(self.id, format!("invalid or missing param: {key}"))
  }

  fn u64(&self, key: &str) -> Result<u64, ErrorReply> {
    self.map.get(key).and_then(Value::as_u64).ok_or_else(|| self.err(key))
  }

  fn i64(&self, key: &str) -> Result<i64, ErrorReply> {
    self.map.get(key).and_then(Value::as_i64).ok_or_else(|| self.err(key))
  }

  fn bool(&self, key: &str) -> Result<bool, ErrorReply> {
    self.map.get(key).and_then(Value::as_bool).ok_or_else(|| self.err(key))
  }

  fn optional_u64(&self, key: &str) -> Result<Option<u64>, ErrorReply> {
    match self.map.get(key) {
      None | Some(Value::Null) => Ok(None),
      Some(v) => v.as_u64().map(Some).ok_or_else(|| self.err(key)),
    }
  }

  /// Numero entre 0.0 y 1.0 inclusive.
  fn unit_f64(&self, key: &str) -> Result<f64, ErrorReply> {
    self
      .map
      .get(key)
      .and_then(Value::as_f64)
      .filter(|v| (0.0..=1.0).contains(v))
      .ok_or_else(|| self.err(key))
  }

  fn repeat(&self) -> Result<Repeat, ErrorReply> {
    match self.map.get("repeat").and_then(Value::as_str) {
      Some("off") => Ok(Repeat::Off),
      Some("track") => Ok(Repeat::Track),
      Some("queue") => Ok(Repeat::Queue),
      _ => Err(self.err("repeat")),
    }
  }

  /// Lista no vacia de URIs con esquema soportado.
  fn uris(&self) -> Result<Vec<String>, ErrorReply> {
    let list = self.map.get("uris").and_then(Value::as_array).ok_or_else(|| self.err("uris"))?;
    if list.is_empty() {
      return Err(self.err("uris"));
    }
    list
      .iter()
      .map(|v| {
        let s = v.as_str().ok_or_else(|| self.err("uris"))?;
        let supported = ["file://", "http://", "https://"].iter().any(|p| s.starts_with(p));
        if supported { Ok(s.to_string()) } else { Err(self.err("uris")) }
      })
      .collect()
  }

  /// Lista no vacia de indices (los repetidos se validan en la cola).
  fn indexes(&self) -> Result<Vec<usize>, ErrorReply> {
    let list = self.map.get("indexes").and_then(Value::as_array).ok_or_else(|| self.err("indexes"))?;
    if list.is_empty() {
      return Err(self.err("indexes"));
    }
    list.iter().map(|v| v.as_u64().map(|n| n as usize).ok_or_else(|| self.err("indexes"))).collect()
  }
}

