//! Metodos `playlist.*` y el evento `playlist.changed`.
//!
//! Las playlists son archivos `.m3u8` en `$XDG_DATA_HOME/ketai/playlists/`
//! (por defecto `~/.local/share/ketai/playlists/`). Todo el I/O lo hace un
//! unico thread, que procesa las operaciones en el orden en que llegan; el
//! loop central solo recibe el resultado.

use std::{
  collections::HashSet,
  fs,
  io,
  path::{Path, PathBuf},
  sync::mpsc::{self, Sender},
  thread,
};

use ketai_core::{playlist, queue::Track};
use ketai_protocol::{ErrorReply, Request, codes};
use serde_json::{Map, Value, json};

use crate::{
  player::{self, TrackIds},
  server::{ClientId, Message},
  session,
};

const PLAYLIST_NOT_FOUND: i64 = 1005;
const PLAYLIST_EXISTS: i64 = 1008;
const INDEX_OUT_OF_RANGE: i64 = 1002;

/// Carpeta de playlists.
pub fn dir() -> Option<PathBuf> {
  let base = std::env::var_os("XDG_DATA_HOME")
    .map(PathBuf::from)
    .filter(|p| p.is_absolute())
    .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))?;
  Some(base.join("ketai").join("playlists"))
}

struct Job {
  client: ClientId,
  request_id: u64,
  method: String,
  params: Map<String, Value>,
  /// URIs de la cola en el momento de la peticion (para `playlist.save`).
  queue_uris: Vec<String>,
}

/// Resultado de una operacion, para el loop central.
pub struct Done {
  pub client: ClientId,
  pub request_id: u64,
  pub result: Result<Value, ErrorReply>,
  /// Playlists modificadas, en orden (para `playlist.changed`).
  pub changed: Vec<String>,
  /// Pistas para la cola si fue un `playlist.load`, y si reemplaza.
  pub load: Option<(Vec<Track>, bool)>,
}

pub struct PlaylistService {
  jobs: Sender<Job>,
  /// Nombres para `playlist.changed` pendientes de publicar.
  changed: Vec<String>,
}

impl PlaylistService {
  pub fn new(tx: Sender<Message>, ids: TrackIds) -> Self {
    let (jobs, rx) = mpsc::channel::<Job>();
    let dir = dir();
    thread::Builder::new()
      .name("ketai-playlists".into())
      .spawn(move || {
        let worker = Worker { dir, ids };
        for job in rx {
          let done = worker.run(job);
          if tx.send(Message::Playlist(done)).is_err() {
            return;
          }
        }
      })
      .expect("spawn playlists thread");
    Self { jobs, changed: Vec::new() }
  }

  pub fn handles(method: &str) -> bool {
    method.starts_with("playlist.")
  }

  /// Encola la operacion; la respuesta llega como `Message::Playlist`.
  pub fn submit(&self, client: ClientId, req: &Request, queue_uris: Vec<String>) {
    let _ = self.jobs.send(Job {
      client,
      request_id: req.id,
      method: req.method.clone(),
      params: req.params.clone(),
      queue_uris,
    });
  }

  pub fn note_changed(&mut self, names: Vec<String>) {
    self.changed.extend(names);
  }

  pub fn take_changed(&mut self) -> Vec<String> {
    std::mem::take(&mut self.changed)
  }
}

struct Worker {
  dir: Option<PathBuf>,
  ids: TrackIds,
}

/// Error interno de una operacion.
type OpResult<T> = Result<T, ErrorReply>;

impl Worker {
  fn run(&self, job: Job) -> Done {
    let id = job.request_id;
    let mut changed = Vec::new();
    let mut load = None;
    let result = (|| -> OpResult<Value> {
      let dir = self.dir.as_deref().ok_or_else(|| other(id, "no XDG_DATA_HOME or HOME"))?;
      let p = Params { id, map: &job.params };
      match job.method.as_str() {
        "playlist.list" => self.list(id, dir),
        "playlist.get" => {
          let name = p.name("name")?;
          let uris = self.read(id, dir, &name)?;
          let tracks = self.lenient(&uris);
          Ok(json!({"name": name, "tracks": tracks.iter().map(player::track_json).collect::<Vec<_>>()}))
        }
        "playlist.save" => {
          let name = p.name("name")?;
          let from_queue = p.bool("from_queue")?;
          let uris = if from_queue { job.queue_uris.clone() } else { Vec::new() };
          self.write(id, dir, &name, &uris)?;
          changed.push(name);
          Ok(json!({}))
        }
        "playlist.delete" => {
          let name = p.name("name")?;
          let path = file(dir, &name);
          if !path.exists() {
            return Err(not_found(id));
          }
          fs::remove_file(path).map_err(|e| other(id, e))?;
          changed.push(name);
          Ok(json!({}))
        }
        "playlist.rename" => {
          let name = p.name("name")?;
          let new_name = p.name("new_name")?;
          let (from, to) = (file(dir, &name), file(dir, &new_name));
          if !from.exists() {
            return Err(not_found(id));
          }
          if to.exists() {
            return Err(ErrorReply::new(Some(id), PLAYLIST_EXISTS, "playlist already exists"));
          }
          fs::rename(from, to).map_err(|e| other(id, e))?;
          changed.push(name);
          changed.push(new_name);
          Ok(json!({}))
        }
        "playlist.add" => {
          let name = p.name("name")?;
          let new = p.uris()?;
          let position = p.position()?;
          let mut uris = self.read(id, dir, &name)?;
          let at = insert_at(id, position, uris.len())?;
          // Todo o nada: se valida cada URI antes de escribir.
          player::resolve(id, &new, self.ids.reserve(new.len()))?;
          let count = new.len();
          uris.splice(at..at, new);
          self.write(id, dir, &name, &uris)?;
          changed.push(name);
          Ok(json!({"added": count}))
        }
        "playlist.remove" => {
          let name = p.name("name")?;
          let indexes = p.indexes()?;
          let mut uris = self.read(id, dir, &name)?;
          let remove = validate_indexes(id, &indexes, uris.len())?;
          let mut i = 0;
          uris.retain(|_| {
            let keep = !remove.contains(&i);
            i += 1;
            keep
          });
          self.write(id, dir, &name, &uris)?;
          changed.push(name);
          Ok(json!({"removed": indexes.len()}))
        }
        "playlist.move" => {
          let name = p.name("name")?;
          let (from, to) = (p.index("from")?, p.index("to")?);
          let mut uris = self.read(id, dir, &name)?;
          if from >= uris.len() || to >= uris.len() {
            return Err(out_of_range(id));
          }
          let uri = uris.remove(from);
          uris.insert(to, uri);
          self.write(id, dir, &name, &uris)?;
          changed.push(name);
          Ok(json!({}))
        }
        "playlist.transfer" => {
          let from_name = p.name("from_name")?;
          let to_name = p.name("to_name")?;
          let indexes = p.indexes()?;
          let position = p.position()?;
          let moving = match job.params.get("mode").and_then(Value::as_str) {
            Some("move") => true,
            Some("copy") => false,
            _ => return Err(p.invalid("mode")),
          };
          let mut source = self.read(id, dir, &from_name)?;
          let mut dest = if from_name == to_name { source.clone() } else { self.read(id, dir, &to_name)? };
          let picked = validate_indexes(id, &indexes, source.len())?;
          let at = insert_at(id, position, dest.len())?;
          // Las pistas llegan en el orden que tenian en el origen.
          let items: Vec<String> =
            source.iter().enumerate().filter(|(i, _)| picked.contains(i)).map(|(_, u)| u.clone()).collect();
          let count = items.len();
          dest.splice(at..at, items);
          // Primero el destino: si falla, el origen no cambio.
          self.write(id, dir, &to_name, &dest)?;
          if moving && from_name != to_name {
            let mut i = 0;
            source.retain(|_| {
              let keep = !picked.contains(&i);
              i += 1;
              keep
            });
            self.write(id, dir, &from_name, &source)?;
          }
          changed.push(from_name);
          changed.push(to_name);
          Ok(json!({"transferred": count}))
        }
        "playlist.load" => {
          let name = p.name("name")?;
          let replace = p.bool("replace")?;
          let uris = self.read(id, dir, &name)?;
          // Solo entran a la cola las pistas que se pueden reproducir.
          let playable =
            self.lenient(&uris).into_iter().filter(|t| t.path.is_none() || t.info.codec.is_some()).collect();
          load = Some((playable, replace));
          Ok(json!({}))
        }
        _ => Err(ErrorReply::new(Some(id), codes::METHOD_NOT_FOUND, "method not found")),
      }
    })();
    if result.is_err() {
      changed.clear();
      load = None;
    }
    Done { client: job.client, request_id: id, result, changed, load }
  }

  fn list(&self, id: u64, dir: &Path) -> OpResult<Value> {
    let mut items = Vec::new();
    let entries = match fs::read_dir(dir) {
      Ok(e) => e,
      Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(json!({"playlists": []})),
      Err(e) => return Err(other(id, e)),
    };
    for entry in entries.flatten() {
      let path = entry.path();
      if path.extension().and_then(|e| e.to_str()) != Some(playlist::EXTENSION) {
        continue;
      }
      let Some(name) = path.file_stem().and_then(|s| s.to_str()) else { continue };
      if !playlist::valid_name(name) {
        continue;
      }
      let count = fs::read_to_string(&path).map(|t| playlist::parse(&t, dir).len()).unwrap_or(0);
      items.push((name.to_string(), count));
    }
    items.sort();
    Ok(json!({"playlists": items.into_iter().map(|(name, count)| json!({"name": name, "count": count})).collect::<Vec<_>>()}))
  }

  fn read(&self, id: u64, dir: &Path, name: &str) -> OpResult<Vec<String>> {
    match fs::read_to_string(file(dir, name)) {
      Ok(text) => Ok(playlist::parse(&text, dir)),
      Err(e) if e.kind() == io::ErrorKind::NotFound => Err(not_found(id)),
      Err(e) => Err(other(id, e)),
    }
  }

  fn write(&self, id: u64, dir: &Path, name: &str, uris: &[String]) -> OpResult<()> {
    session::write_atomic(&file(dir, name), playlist::render(uris).as_bytes()).map_err(|e| other(id, e))
  }

  fn lenient(&self, uris: &[String]) -> Vec<Track> {
    let first = self.ids.reserve(uris.len());
    uris.iter().enumerate().map(|(n, uri)| player::resolve_lenient(uri, first + n as u64)).collect()
  }
}

fn file(dir: &Path, name: &str) -> PathBuf {
  dir.join(format!("{name}.{}", playlist::EXTENSION))
}

fn insert_at(id: u64, position: Option<usize>, len: usize) -> OpResult<usize> {
  let at = position.unwrap_or(len);
  if at > len { Err(out_of_range(id)) } else { Ok(at) }
}

/// Indices sin repetir y dentro del rango.
fn validate_indexes(id: u64, indexes: &[usize], len: usize) -> OpResult<HashSet<usize>> {
  let mut set = HashSet::new();
  for &i in indexes {
    if i >= len {
      return Err(out_of_range(id));
    }
    if !set.insert(i) {
      return Err(ErrorReply::new(Some(id), codes::INVALID_PARAMS, "duplicate index"));
    }
  }
  Ok(set)
}

fn not_found(id: u64) -> ErrorReply {
  ErrorReply::new(Some(id), PLAYLIST_NOT_FOUND, "playlist not found")
}

fn out_of_range(id: u64) -> ErrorReply {
  ErrorReply::new(Some(id), INDEX_OUT_OF_RANGE, "index out of range")
}

fn other(id: u64, e: impl std::fmt::Display) -> ErrorReply {
  ErrorReply::new(Some(id), -32603, format!("internal error: {e}"))
}

struct Params<'a> {
  id: u64,
  map: &'a Map<String, Value>,
}

impl Params<'_> {
  fn invalid(&self, key: &str) -> ErrorReply {
    ErrorReply::new(Some(self.id), codes::INVALID_PARAMS, format!("invalid or missing param: {key}"))
  }

  fn name(&self, key: &str) -> OpResult<String> {
    self
      .map
      .get(key)
      .and_then(Value::as_str)
      .filter(|n| playlist::valid_name(n))
      .map(str::to_string)
      .ok_or_else(|| self.invalid(key))
  }

  fn bool(&self, key: &str) -> OpResult<bool> {
    self.map.get(key).and_then(Value::as_bool).ok_or_else(|| self.invalid(key))
  }

  fn index(&self, key: &str) -> OpResult<usize> {
    self.map.get(key).and_then(Value::as_u64).map(|v| v as usize).ok_or_else(|| self.invalid(key))
  }

  fn position(&self) -> OpResult<Option<usize>> {
    match self.map.get("position") {
      None | Some(Value::Null) => Ok(None),
      Some(v) => v.as_u64().map(|n| Some(n as usize)).ok_or_else(|| self.invalid("position")),
    }
  }

  fn indexes(&self) -> OpResult<Vec<usize>> {
    let list = self.map.get("indexes").and_then(Value::as_array).filter(|l| !l.is_empty());
    let list = list.ok_or_else(|| self.invalid("indexes"))?;
    list.iter().map(|v| v.as_u64().map(|n| n as usize).ok_or_else(|| self.invalid("indexes"))).collect()
  }

  fn uris(&self) -> OpResult<Vec<String>> {
    let list = self.map.get("uris").and_then(Value::as_array).filter(|l| !l.is_empty());
    let list = list.ok_or_else(|| self.invalid("uris"))?;
    list
      .iter()
      .map(|v| {
        let s = v.as_str().ok_or_else(|| self.invalid("uris"))?;
        let ok = ["file://", "http://", "https://"].iter().any(|p| s.starts_with(p));
        if ok { Ok(s.to_string()) } else { Err(self.invalid("uris")) }
      })
      .collect()
  }
}
