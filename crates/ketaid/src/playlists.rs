//! Metodos `playlist.*` y el evento `playlist.changed`.
//!
//! Metodos `favorites.set` y `track.playlists`.
//!
//! Las playlists viven en la base (`store`), que manda. En la carpeta de
//! playlists (ver `dir`) ketai escribe una copia `.m3u8` de cada una, para
//! usarlas con otros reproductores. Un archivo nuevo en la carpeta se
//! importa; uno conocido que se edita a mano se reescribe desde la base.
//! Todo lo hace un unico thread, que procesa las operaciones en el orden en
//! que llegan; el loop central solo recibe el resultado.

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
  store::{FAVORITES, Store},
};

const PLAYLIST_NOT_FOUND: i64 = 1005;
const PLAYLIST_EXISTS: i64 = 1008;
const ALREADY_IN_PLAYLIST: i64 = 1009;
const INDEX_OUT_OF_RANGE: i64 = 1002;

/// Carpeta de playlists: `[playlists] dir` de config.toml o, si no esta,
/// `XDG_MUSIC_DIR/playlists`. Se resuelve en cada operacion, asi un cambio en
/// config.toml se aplica sin reiniciar el daemon.
pub fn dir() -> Option<PathBuf> {
  configured().or_else(|| music_dir().map(|m| m.join("playlists")))
}

fn home() -> Option<PathBuf> {
  std::env::var_os("HOME").filter(|h| !h.is_empty()).map(PathBuf::from)
}

/// `~` al principio es la carpeta del usuario.
fn expand(path: &str) -> Option<PathBuf> {
  match path.strip_prefix('~') {
    Some(rest) => Some(home()?.join(rest.trim_start_matches('/'))),
    None => Some(PathBuf::from(path)),
  }
}

fn config_dir() -> Option<PathBuf> {
  std::env::var_os("XDG_CONFIG_HOME")
    .map(PathBuf::from)
    .filter(|p| p.is_absolute())
    .or_else(|| home().map(|h| h.join(".config")))
}

/// Ultimo problema con config.toml que se registro (para no repetirlo en
/// cada operacion).
static CONFIG_PROBLEM: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

fn report(problem: Option<String>) {
  let mut last = CONFIG_PROBLEM.lock().unwrap_or_else(|e| e.into_inner());
  if *last != problem {
    if let Some(p) = &problem {
      eprintln!("ketaid: config.toml: {p}; playlists use the default folder");
    }
    *last = problem;
  }
}

/// Carpeta de `[playlists] dir`, si config.toml la define y es valida.
fn configured() -> Option<PathBuf> {
  let path = config_dir()?.join("ketai").join("config.toml");
  let text = std::fs::read_to_string(path).ok()?;
  let table: toml::Table = match text.parse() {
    Ok(t) => t,
    Err(e) => {
      report(Some(e.to_string().lines().next().unwrap_or("invalid toml").to_string()));
      return None;
    }
  };
  let Some(value) = table.get("playlists").and_then(|p| p.get("dir")) else {
    report(None);
    return None;
  };
  let dir = value.as_str().and_then(expand).filter(|p| p.is_absolute());
  report(if dir.is_none() { Some("playlists.dir must be an absolute path or start with ~".into()) } else { None });
  dir
}

/// `XDG_MUSIC_DIR` del entorno, de `user-dirs.dirs` o `~/music`.
fn music_dir() -> Option<PathBuf> {
  if let Some(m) = std::env::var_os("XDG_MUSIC_DIR").map(PathBuf::from).filter(|p| p.is_absolute()) {
    return Some(m);
  }
  let from_user_dirs = config_dir().and_then(|c| std::fs::read_to_string(c.join("user-dirs.dirs")).ok()).and_then(|text| {
    let line = text.lines().find(|l| l.trim_start().starts_with("XDG_MUSIC_DIR="))?;
    let value = line.split_once('=')?.1.trim().trim_matches('"');
    let value = match value.strip_prefix("$HOME") {
      Some(rest) => home()?.join(rest.trim_start_matches('/')),
      None => PathBuf::from(value),
    };
    value.is_absolute().then_some(value)
  });
  from_user_dirs.or_else(|| home().map(|h| h.join("music")))
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
    thread::Builder::new()
      .name("ketai-playlists".into())
      .spawn(move || {
        let mut worker = Worker { ids, store: crate::store::open_default() };
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
    method.starts_with("playlist.") || method == "favorites.set" || method == "track.playlists"
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
  ids: TrackIds,
  /// Playlists y favoritos (`library.db`). Manda sobre los archivos.
  store: Store,
}

/// Error interno de una operacion.
type OpResult<T> = Result<T, ErrorReply>;

impl Worker {
  fn run(&mut self, job: Job) -> Done {
    let id = job.request_id;
    let mut changed = Vec::new();
    let mut load = None;
    let result = (|| -> OpResult<Value> {
      let dir = dir().ok_or_else(|| other(id, "no playlists folder: set HOME or [playlists] dir"))?;
      // La carpeta se crea si no existe.
      std::fs::create_dir_all(&dir).map_err(|e| other(id, format!("{}: {e}", dir.display())))?;
      let dir = dir.as_path();
      self.sync(id, dir)?;
      let p = Params { id, map: &job.params };
      match job.method.as_str() {
        "playlist.list" => {
          let items: Vec<Value> = self
            .store
            .playlists()
            .map_err(|e| other(id, e))?
            .into_iter()
            .map(|pl| {
              let mut v = json!({"name": pl.name, "count": pl.count});
              if pl.kind != "user" {
                v["kind"] = json!(pl.kind);
              }
              v
            })
            .collect();
          Ok(json!({"playlists": items}))
        }
        "playlist.get" => {
          let name = p.name("name")?;
          let uris = self.store.items(&name).map_err(|e| other(id, e))?.ok_or_else(|| not_found(id))?;
          let tracks = self.lenient(&uris);
          Ok(json!({"name": name, "path": file(dir, &name), "tracks": tracks.iter().map(player::track_json).collect::<Vec<_>>()}))
        }
        "playlist.save" => {
          let name = p.name("name")?;
          let from_queue = p.bool("from_queue")?;
          let uris = if from_queue { job.queue_uris.clone() } else { Vec::new() };
          self.update(id, &[&name], |l| {
            l[0] = Some(uris);
            Ok(())
          })?;
          changed.push(name);
          Ok(json!({}))
        }
        "playlist.delete" => {
          let name = p.name("name")?;
          self.update(id, &[&name], |l| l[0].take().map(|_| ()).ok_or_else(|| not_found(id)))?;
          changed.push(name);
          Ok(json!({}))
        }
        "playlist.rename" => {
          let name = p.name("name")?;
          let new_name = p.name("new_name")?;
          if self.store.items(&name).map_err(|e| other(id, e))?.is_none() {
            return Err(not_found(id));
          }
          if self.store.items(&new_name).map_err(|e| other(id, e))?.is_some() {
            return Err(ErrorReply::new(Some(id), PLAYLIST_EXISTS, "playlist already exists"));
          }
          self.store.rename(&name, &new_name).map_err(|e| other(id, e))?;
          changed.push(name);
          changed.push(new_name);
          Ok(json!({}))
        }
        "playlist.add" => {
          let name = p.name("name")?;
          let mut new = p.uris()?;
          let position = p.position()?;
          // Que hacer con las que ya estan: preguntar (error 1009 con la
          // lista, el cliente confirma), agregarlas igual u omitirlas.
          let duplicates = match job.params.get("duplicates") {
            None | Some(Value::Null) => "ask",
            Some(v) => v.as_str().filter(|d| ["ask", "add", "skip"].contains(d)).ok_or_else(|| p.invalid("duplicates"))?,
          };
          // Todo o nada: se valida cada URI antes de escribir.
          player::resolve(id, &new, self.ids.reserve(new.len()))?;
          let count = self.update(id, &[&name], |l| {
            let uris = l[0].as_mut().ok_or_else(|| not_found(id))?;
            let at = insert_at(id, position, uris.len())?;
            let mut repeated: Vec<&String> = new.iter().filter(|n| uris.contains(n)).collect();
            repeated.dedup();
            if !repeated.is_empty() && duplicates == "ask" {
              return Err(
                ErrorReply::new(Some(id), ALREADY_IN_PLAYLIST, "already in playlist").with_data(json!({"duplicates": repeated})),
              );
            }
            if duplicates == "skip" {
              new.retain(|n| !uris.contains(n));
            }
            let count = new.len();
            uris.splice(at..at, new);
            Ok(count)
          })?;
          if count > 0 {
            changed.push(name);
          }
          Ok(json!({"added": count}))
        }
        "playlist.remove" => {
          let name = p.name("name")?;
          // Por posicion (`indexes`) o por cancion (`uris`: todas sus copias),
          // uno de los dos.
          let by_uri = match (job.params.contains_key("indexes"), job.params.contains_key("uris")) {
            (true, false) => None,
            (false, true) => Some(p.uris()?),
            _ => return Err(ErrorReply::new(Some(id), codes::INVALID_PARAMS, "give either indexes or uris")),
          };
          let indexes = if by_uri.is_none() { p.indexes()? } else { Vec::new() };
          let removed = self.update(id, &[&name], |l| {
            let uris = l[0].as_mut().ok_or_else(|| not_found(id))?;
            let before = uris.len();
            match &by_uri {
              Some(gone) => uris.retain(|u| !gone.contains(u)),
              None => {
                let remove = validate_indexes(id, &indexes, uris.len())?;
                let mut i = 0;
                uris.retain(|_| {
                  let keep = !remove.contains(&i);
                  i += 1;
                  keep
                });
              }
            }
            Ok(before - uris.len())
          })?;
          if removed > 0 {
            changed.push(name);
          }
          Ok(json!({"removed": removed}))
        }
        "playlist.move" => {
          let name = p.name("name")?;
          let (from, to) = (p.index("from")?, p.index("to")?);
          self.update(id, &[&name], |l| {
            let uris = l[0].as_mut().ok_or_else(|| not_found(id))?;
            if from >= uris.len() || to >= uris.len() {
              return Err(out_of_range(id));
            }
            let uri = uris.remove(from);
            uris.insert(to, uri);
            Ok(())
          })?;
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
          // Dentro de la misma playlist se reordena con playlist.move.
          if from_name == to_name {
            return Err(ErrorReply::new(Some(id), codes::INVALID_PARAMS, "from_name and to_name must differ; use playlist.move"));
          }
          // Origen y destino cambian juntos, en una sola transaccion.
          let count = self.update(id, &[&from_name, &to_name], |l| {
            let (source, dest) = l.split_at_mut(1);
            let source = source[0].as_mut().ok_or_else(|| not_found(id))?;
            let dest = dest[0].as_mut().ok_or_else(|| not_found(id))?;
            let picked = validate_indexes(id, &indexes, source.len())?;
            let at = insert_at(id, position, dest.len())?;
            // Las pistas llegan en el orden que tenian en el origen.
            let items: Vec<String> =
              source.iter().enumerate().filter(|(i, _)| picked.contains(i)).map(|(_, u)| u.clone()).collect();
            let count = items.len();
            dest.splice(at..at, items);
            if moving {
              let mut i = 0;
              source.retain(|_| {
                let keep = !picked.contains(&i);
                i += 1;
                keep
              });
            }
            Ok(count)
          })?;
          changed.push(to_name);
          if moving {
            changed.push(from_name);
          }
          Ok(json!({"transferred": count}))
        }
        "playlist.load" => {
          let name = p.name("name")?;
          let replace = p.bool("replace")?;
          let uris = self.store.items(&name).map_err(|e| other(id, e))?.ok_or_else(|| not_found(id))?;
          // Solo entran a la cola las pistas que se pueden reproducir.
          let playable =
            self.lenient(&uris).into_iter().filter(|t| t.path.is_none() || t.info.codec.is_some()).collect();
          load = Some((playable, replace));
          Ok(json!({}))
        }
        "favorites.set" => {
          let uri = p.uri()?;
          let favorite = p.bool("favorite")?;
          self.update(id, &[FAVORITES], |l| {
            let list = l[0].get_or_insert_with(Vec::new);
            if favorite {
              if !list.contains(&uri) {
                list.push(uri);
              }
            } else {
              list.retain(|u| *u != uri);
            }
            Ok(())
          })?;
          changed.push(FAVORITES.to_string());
          Ok(json!({}))
        }
        "track.playlists" => {
          let uri = p.uri()?;
          let (playlists, favorite) = self.store.memberships(&uri).map_err(|e| other(id, e))?;
          Ok(json!({"playlists": playlists, "favorite": favorite}))
        }
        _ => Err(ErrorReply::new(Some(id), codes::METHOD_NOT_FOUND, "method not found")),
      }
    })();
    if result.is_err() {
      changed.clear();
      load = None;
    }
    // La base ya cambio: se escriben las copias. Si falla una copia, la
    // proxima operacion la vuelve a escribir.
    if let Some(dir) = dir() {
      for name in &changed {
        self.export(&dir, name);
      }
    }
    Done { client: job.client, request_id: id, result, changed, load }
  }

  /// Cambio atomico en la base (ver `Store::update`).
  fn update<T>(
    &mut self,
    id: u64,
    names: &[&str],
    change: impl FnOnce(&mut Vec<Option<Vec<String>>>) -> OpResult<T>,
  ) -> OpResult<T> {
    self.store.update(names, change).map_err(|e| other(id, e))?
  }

  /// Pone la carpeta al dia con la base. Un archivo que la base no conoce
  /// (copiado a mano) se importa; uno que cambio por fuera de ketai o que
  /// falta se vuelve a escribir desde la base.
  fn sync(&mut self, id: u64, dir: &Path) -> OpResult<()> {
    let known: HashSet<String> = self.store.playlists().map_err(|e| other(id, e))?.into_iter().map(|p| p.name).collect();
    let entries = match fs::read_dir(dir) {
      Ok(e) => e.flatten().collect::<Vec<_>>(),
      Err(e) if e.kind() == io::ErrorKind::NotFound => Vec::new(),
      Err(e) => return Err(other(id, e)),
    };
    let mut on_disk = HashSet::new();
    for entry in entries {
      let path = entry.path();
      if path.extension().and_then(|e| e.to_str()) != Some(playlist::EXTENSION) {
        continue;
      }
      let Some(name) = path.file_stem().and_then(|s| s.to_str()).map(str::to_string) else { continue };
      if !playlist::valid_name(&name) {
        continue;
      }
      on_disk.insert(name.clone());
      let stamp = stamp(&path);
      if !known.contains(&name) {
        let uris = fs::read_to_string(&path).map(|t| playlist::parse(&t, dir)).unwrap_or_default();
        self.update(id, &[&name], |l| {
          l[0] = Some(uris);
          Ok(())
        })?;
        self.store.set_file_stamp(&name, stamp.as_deref()).map_err(|e| other(id, e))?;
      } else if self.store.file_stamp(&name).map_err(|e| other(id, e))? != stamp {
        self.export(dir, &name);
      }
    }
    for name in known.difference(&on_disk) {
      self.export(dir, name);
    }
    Ok(())
  }

  /// Escribe la copia de una playlist (o la borra si ya no existe) y anota
  /// su marca.
  fn export(&mut self, dir: &Path, name: &str) {
    let path = file(dir, name);
    match self.store.items(name) {
      Ok(Some(uris)) => {
        if let Err(e) = session::write_atomic(&path, playlist::render(&uris).as_bytes()) {
          eprintln!("ketaid: could not write {}: {e}", path.display());
          return;
        }
        let _ = self.store.set_file_stamp(name, stamp(&path).as_deref());
      }
      Ok(None) => {
        let _ = fs::remove_file(&path);
      }
      Err(e) => eprintln!("ketaid: playlist {name}: {e}"),
    }
  }

  fn lenient(&self, uris: &[String]) -> Vec<Track> {
    let first = self.ids.reserve(uris.len());
    uris.iter().enumerate().map(|(n, uri)| player::resolve_lenient(uri, first + n as u64)).collect()
  }
}

/// Marca de un archivo: tamano, mtime con nanosegundos e inodo.
fn stamp(path: &Path) -> Option<String> {
  use std::os::unix::fs::MetadataExt;
  let m = fs::metadata(path).ok()?;
  Some(format!("{}:{}.{}:{}", m.len(), m.mtime(), m.mtime_nsec(), m.ino()))
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

  fn uri(&self) -> OpResult<String> {
    let s = self.map.get("uri").and_then(Value::as_str).ok_or_else(|| self.invalid("uri"))?;
    let ok = ["file://", "http://", "https://"].iter().any(|p| s.starts_with(p));
    if ok { Ok(s.to_string()) } else { Err(self.invalid("uri")) }
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
