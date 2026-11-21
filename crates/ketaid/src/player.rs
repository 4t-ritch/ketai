//! Metodos `player.*` y `queue.*` (docs/protocol.md seccion 5).
//!
//! Todo este estado lo toca solo el loop central. El I/O (leer metadatos,
//! abrir audio) ocurre en otros threads y vuelve como mensajes:
//! - `engine::Event` desde el motor de audio.
//! - `Resolved` desde el thread que lee metadatos para `queue.add`.

use std::{
  path::PathBuf,
  sync::mpsc::Sender,
  thread,
  time::Instant,
};

use ketai_core::{
  engine::{self, Backend, Command, EngineHandle, Event},
  metadata::{self, TrackInfo},
  queue::{CurrentAfterRemove, Queue, QueueError, Repeat, Track},
};
use ketai_protocol::{ErrorReply, Request, codes};
use serde_json::{Map, Value, json};

use crate::server::{ClientId, Message};

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

pub struct PlayerService {
  queue: Queue,
  engine: EngineHandle,
  /// Canal hacia el loop central, para el thread de metadatos.
  tx: Sender<Message>,
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
  next_track_id: u64,
  /// Fallos de carga seguidos, para no girar sin fin sobre una cola rota.
  consecutive_failures: usize,
}

impl PlayerService {
  pub fn new(tx: Sender<Message>) -> Self {
    let (events_tx, events_rx) = std::sync::mpsc::channel();
    let engine = engine::spawn(Backend::from_env(), events_tx);
    // Reenvia los eventos del motor al loop central.
    let forward = tx.clone();
    thread::spawn(move || {
      for ev in events_rx {
        if forward.send(Message::Engine(ev)).is_err() {
          return;
        }
      }
    });
    let seed = std::time::SystemTime::now()
      .duration_since(std::time::UNIX_EPOCH)
      .map(|d| d.as_nanos() as u64)
      .unwrap_or(1);
    Self {
      queue: Queue::new(seed),
      engine,
      tx,
      status: Status::Stopped,
      generation: 0,
      loaded: false,
      position_ms: 0,
      position_at: Instant::now(),
      duration_ms: None,
      volume: 1.0,
      muted: false,
      next_track_id: 0,
      consecutive_failures: 0,
    }
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
    Ok(json!({}))
  }

  fn apply_volume(&mut self) {
    let v = if self.muted { 0.0 } else { self.volume };
    self.engine.send(Command::Volume(v as f32));
  }

  /// Carga la pista seleccionada en el motor desde el principio.
  fn load_current(&mut self, play: bool) {
    let Some(track) = self.queue.current_track() else {
      self.halt();
      return;
    };
    self.generation += 1;
    self.duration_ms = track.info.duration_ms;
    match track.path.clone() {
      Some(path) => {
        self.engine.send(Command::Load { generation: self.generation, path, start_ms: 0, play });
        self.loaded = true;
        self.status = if play { Status::Playing } else { Status::Paused };
        self.set_position(0);
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
    let first_id = self.next_track_id;
    self.next_track_id += uris.len() as u64;
    let tx = self.tx.clone();
    // Leer metadatos es I/O de disco: se hace fuera del loop central.
    thread::spawn(move || {
      let tracks = resolve(id, &uris, first_id);
      let _ = tx.send(Message::Resolved(Resolved { client, request_id: id, position, tracks }));
    });
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
      "repeat": match self.queue.repeat() {
        Repeat::Off => "off",
        Repeat::Track => "track",
        Repeat::Queue => "queue",
      },
      "shuffle": self.queue.shuffle(),
      "output": Value::Null,
    })
  }

  fn queue_json(&self) -> Value {
    json!({
      "current": self.queue.current(),
      "tracks": self.queue.tracks().iter().map(track_json).collect::<Vec<_>>(),
    })
  }
}

fn track_json(t: &Track) -> Value {
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

/// Resuelve todas las URIs o ninguna (la cola no cambia si una falla).
fn resolve(id: u64, uris: &[String], first_id: u64) -> Result<Vec<Track>, ErrorReply> {
  uris
    .iter()
    .enumerate()
    .map(|(n, uri)| {
      let track_id = format!("t_{:x}", first_id + n as u64);
      if uri.starts_with("http://") || uri.starts_with("https://") {
        return Ok(Track { id: track_id, uri: uri.clone(), path: None, info: TrackInfo::default() });
      }
      let path = file_uri_path(uri).ok_or_else(|| invalid_params(id, format!("unsupported uri: {uri}")))?;
      let info = metadata::probe(&path)
        .map_err(|e| ErrorReply::new(Some(id), 1003, format!("source unavailable: {uri}: {e}")))?;
      Ok(Track { id: track_id, uri: uri.clone(), path: Some(path), info })
    })
    .collect()
}

/// `file:///ruta%20con%20espacios` -> `/ruta con espacios`.
fn file_uri_path(uri: &str) -> Option<PathBuf> {
  let rest = uri.strip_prefix("file://")?;
  if !rest.starts_with('/') {
    return None;
  }
  let bytes = rest.as_bytes();
  let mut out = Vec::with_capacity(bytes.len());
  let mut i = 0;
  while i < bytes.len() {
    if bytes[i] == b'%' {
      let hex = std::str::from_utf8(bytes.get(i + 1..i + 3)?).ok()?;
      out.push(u8::from_str_radix(hex, 16).ok()?);
      i += 3;
    } else {
      out.push(bytes[i]);
      i += 1;
    }
  }
  use std::os::unix::ffi::OsStringExt;
  Some(PathBuf::from(std::ffi::OsString::from_vec(out)))
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

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn decodes_file_uris() {
    assert_eq!(file_uri_path("file:///a%20b/c.ogg"), Some(PathBuf::from("/a b/c.ogg")));
    assert_eq!(file_uri_path("file://relative.ogg"), None);
    assert_eq!(file_uri_path("file:///bad%2"), None);
  }
}
