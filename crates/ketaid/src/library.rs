//! Metodos `library.*`, `track.cover` y `cache.*` (version local).
//!
//! Todo es I/O de disco, asi que lo atiende un unico thread (el worker), en
//! el orden en que llegan las peticiones. El worker es el dueno del indice.
//! Un escaneo corre en un thread aparte: informa el progreso al loop central
//! y al terminar le entrega las pistas al worker, que actualiza el indice y
//! lo guarda en `$XDG_DATA_HOME/ketai/library.json`.

use std::{
  collections::HashSet,
  fs,
  io::Write,
  os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt},
  path::{Path, PathBuf},
  sync::mpsc::{self, Sender},
  thread,
  time::{Duration, Instant},
};

use ketai_core::{
  library::{self, Entry},
  metadata::{self, TrackInfo},
  uri::path_to_file_uri,
};
use ketai_protocol::{ErrorReply, Request, codes};
use serde_json::{Map, Value, json};

use crate::{
  server::{ClientId, Message},
  store::Store,
};

const DEFAULT_LIMIT: u64 = 50;
/// Limite de la cache que se informa (las descargas llegan en una version
/// futura).
const CACHE_LIMIT_BYTES: u64 = 2 * 1024 * 1024 * 1024;
const INDEX_VERSION: u64 = 1;
/// Cada cuanto se informa el progreso de un escaneo como maximo.
/// Pistas por tanda de guardado durante un escaneo.
const BATCH: usize = 200;
const PROGRESS_EVERY: Duration = Duration::from_millis(200);

/// Pista de la cola a la que se refiere `track.cover`.
pub struct TrackRef {
  pub id: String,
  pub path: Option<PathBuf>,
}

enum Job {
  Request { client: ClientId, request_id: u64, method: String, params: Map<String, Value>, track: Option<TrackRef> },
  /// Tanda de un escaneo en curso: se guarda enseguida.
  Batch(Vec<Entry>),
  /// Un escaneo termino: se borra lo que ya no aparecio bajo cada carpeta.
  Indexed { job: String, roots: Vec<PathBuf>, seen: HashSet<PathBuf>, total: u64 },
}

pub struct Done {
  pub client: ClientId,
  pub request_id: u64,
  pub result: Result<Value, ErrorReply>,
  /// Cambiaron las carpetas: se emite `library.changed`.
  pub changed: bool,
}

/// Progreso de un escaneo para el evento `library.progress`.
pub struct Progress {
  pub job: String,
  pub done: u64,
  pub total: u64,
}

pub struct LibraryService {
  jobs: Sender<Job>,
  progress: Vec<Progress>,
  changed: bool,
}

impl LibraryService {
  pub fn new(tx: Sender<Message>) -> Self {
    let (jobs, rx) = mpsc::channel::<Job>();
    let worker_jobs = jobs.clone();
    thread::Builder::new()
      .name("ketai-library".into())
      .spawn(move || {
        let mut worker = Worker::load(tx.clone(), worker_jobs);
        for job in rx {
          match job {
            Job::Request { client, request_id, method, params, track } => {
              let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                worker.run(request_id, &method, &params, track.as_ref())
              }))
              .unwrap_or_else(|_| Err(ErrorReply::new(Some(request_id), codes::INVALID_PARAMS, "request failed")));
              let changed = result.is_ok() && matches!(method.as_str(), "library.add_folder" | "library.remove_folder");
              if tx.send(Message::Library(Done { client, request_id, result, changed })).is_err() {
                return;
              }
            }
            Job::Batch(entries) => {
              if let Err(e) = worker.store.save_batch(&entries) {
                eprintln!("ketaid: could not save library index: {e}");
              }
            }
            Job::Indexed { job, roots, seen, total } => {
              if let Err(e) = worker.store.finish_scan(&roots, &seen) {
                eprintln!("ketaid: could not save library index: {e}");
              }
              // El ultimo progreso sale despues de actualizar el indice: quien
              // busque al verlo ya encuentra las pistas nuevas.
              let _ = tx.send(Message::LibraryProgress(Progress { job, done: total, total }));
            }
          }
        }
      })
      .expect("spawn library thread");
    Self { jobs, progress: Vec::new(), changed: false }
  }

  pub fn handles(method: &str) -> bool {
    method.starts_with("library.") || method.starts_with("cache.") || method == "track.cover"
  }

  /// `track.cover` necesita la pista de la cola; el resto no.
  pub fn needs_track(method: &str) -> bool {
    method == "track.cover"
  }

  pub fn submit(&self, client: ClientId, req: &Request, track: Option<TrackRef>) {
    let _ = self.jobs.send(Job::Request {
      client,
      request_id: req.id,
      method: req.method.clone(),
      params: req.params.clone(),
      track,
    });
  }

  /// Cambiaron las carpetas: el proximo `publish` emite `library.changed`.
  pub fn note_changed(&mut self) {
    self.changed = true;
  }

  pub fn take_changed(&mut self) -> bool {
    std::mem::take(&mut self.changed)
  }

  pub fn note_progress(&mut self, p: Progress) {
    self.progress.push(p);
  }

  pub fn take_progress(&mut self) -> Vec<Progress> {
    std::mem::take(&mut self.progress)
  }
}

struct Worker {
  /// Carpetas y pistas (`library.db`). Sin carpeta de datos, en memoria.
  store: Store,
  tx: Sender<Message>,
  jobs: Sender<Job>,
  next_job: u64,
}

impl Worker {
  fn load(tx: Sender<Message>, jobs: Sender<Job>) -> Self {
    let mut store = crate::store::open_default();
    // El indice de versiones anteriores se importa la primera vez.
    if let Some(d) = data_dir() {
      let (entries, folders) = load_index(&d.join("library.json"));
      if let Err(e) = store.import_legacy_once(&entries, &folders) {
        eprintln!("ketaid: could not import library.json: {e}");
      }
    }
    Self { store, tx, jobs, next_job: 0 }
  }

  fn run(&mut self, id: u64, method: &str, params: &Map<String, Value>, track: Option<&TrackRef>) -> Result<Value, ErrorReply> {
    let invalid = |msg: &str| ErrorReply::new(Some(id), codes::INVALID_PARAMS, msg.to_string());
    let db = |e: rusqlite::Error| ErrorReply::new(Some(id), codes::INTERNAL_ERROR, format!("library database: {e}"));
    match method {
      "library.scan" => {
        // Sin path: todas las carpetas de la biblioteca.
        let roots = match params.get("path") {
          None | Some(Value::Null) => {
            let folders = self.store.folders().map_err(db)?;
            if folders.is_empty() {
              return Err(invalid("there are no library folders: add one with library.add_folder"));
            }
            folders
          }
          Some(p) => vec![existing_folder(p).ok_or_else(|| invalid("path must be an existing absolute folder"))?],
        };
        self.next_job += 1;
        let job = format!("j_{}", self.next_job);
        self.spawn_scan(job.clone(), roots);
        Ok(json!({"job": job}))
      }
      "library.folders" => {
        let mut folders = Vec::new();
        for f in self.store.folders().map_err(db)? {
          folders.push(json!({"path": f, "tracks": self.store.count_under(&f).map_err(db)?}));
        }
        Ok(json!({"folders": folders}))
      }
      "library.add_folder" => {
        let folder = params.get("path").and_then(existing_folder).ok_or_else(|| invalid("path must be an existing absolute folder"))?;
        self.store.add_folder(&folder).map_err(db)?;
        Ok(json!({}))
      }
      "library.remove_folder" => {
        let folder = params.get("path").and_then(Value::as_str).map(PathBuf::from).ok_or_else(|| invalid("path is not a library folder"))?;
        // Sus canciones dejan de ser parte de la biblioteca.
        if !self.store.remove_folder(&folder).map_err(db)? {
          return Err(invalid("path is not a library folder"));
        }
        Ok(json!({}))
      }
      "library.search" => {
        let query = params.get("query").and_then(Value::as_str).ok_or_else(|| invalid("query is required"))?;
        let limit = match params.get("limit") {
          None | Some(Value::Null) => DEFAULT_LIMIT,
          Some(v) => v.as_u64().filter(|&n| n >= 1).ok_or_else(|| invalid("limit must be a positive integer"))?,
        };
        let words = library::words(query);
        let tracks: Vec<Value> = self.store.search(&words, limit).map_err(db)?.iter().map(track_json).collect();
        Ok(json!({"tracks": tracks}))
      }
      "library.groups" => {
        let by = params.get("by").and_then(Value::as_str).unwrap_or_default();
        let groups = self.store.groups(by).ok_or_else(|| invalid("by must be artist, album, folder, year or genre"))?.map_err(db)?;
        let groups: Vec<Value> = groups.into_iter().map(|(name, count)| json!({"name": name, "count": count})).collect();
        Ok(json!({"groups": groups}))
      }
      "library.tracks" => {
        let number = |k: &str, default: u64| match params.get(k) {
          None | Some(Value::Null) => Ok(default),
          Some(v) => v.as_u64().ok_or_else(|| invalid(&format!("{k} must be a non-negative integer"))),
        };
        let (offset, limit) = (number("offset", 0)?, number("limit", 200)?.clamp(1, 5000));
        let group = match params.get("by") {
          None | Some(Value::Null) => None,
          Some(by) => {
            let by = by.as_str().ok_or_else(|| invalid("by must be a string"))?;
            // Un grupo necesita `name` (null es el grupo sin dato).
            let name = match params.get("name") {
              None => return Err(invalid("name is required with by")),
              Some(Value::Null) => None,
              Some(v) => Some(v.as_str().ok_or_else(|| invalid("name must be a string or null"))?),
            };
            Some((by, name))
          }
        };
        let (tracks, total) =
          self.store.tracks(group, offset, limit).ok_or_else(|| invalid("by must be artist, album, folder, year or genre"))?.map_err(db)?;
        Ok(json!({"tracks": tracks.iter().map(track_json).collect::<Vec<_>>(), "total": total}))
      }
      "track.cover" => {
        let track = track.ok_or_else(|| invalid("track_id or uri is required"))?;
        // Pistas remotas: todavia no tienen caratula.
        let Some(path) = &track.path else { return Ok(Value::Null) };
        let Some(cover) = metadata::cover(path) else { return Ok(Value::Null) };
        let written = write_cover(&track.id, &cover).map_err(|e| invalid(&format!("cover could not be written: {e}")))?;
        Ok(json!({"path": written, "mime": cover.mime}))
      }
      "cache.info" => {
        let Some(dir) = cache_dir() else {
          return Ok(json!({"path": null, "size_bytes": 0, "entries": 0, "limit_bytes": CACHE_LIMIT_BYTES}));
        };
        let (size, entries) = dir_usage(&dir);
        Ok(json!({"path": dir, "size_bytes": size, "entries": entries, "limit_bytes": CACHE_LIMIT_BYTES}))
      }
      "cache.clear" => {
        // Si la carpeta de cache es un enlace, no se borra lo que hay del
        // otro lado.
        let dir = cache_dir().filter(|d| !fs::symlink_metadata(d).is_ok_and(|m| m.file_type().is_symlink()));
        let freed = dir.map(|d| clear_dir(&d)).unwrap_or(0);
        Ok(json!({"freed_bytes": freed}))
      }
      _ => Err(ErrorReply::new(Some(id), codes::METHOD_NOT_FOUND, "method not found")),
    }
  }

  fn spawn_scan(&self, job: String, roots: Vec<PathBuf>) {
    let (tx, jobs) = (self.tx.clone(), self.jobs.clone());
    thread::spawn(move || {
      let files: Vec<PathBuf> = roots.iter().flat_map(|r| library::candidates(r)).collect();
      let total = files.len() as u64;
      let progress = |done: u64| {
        let _ = tx.send(Message::LibraryProgress(Progress { job: job.clone(), done, total }));
      };
      progress(0);
      let mut last = Instant::now();
      let mut entries = Vec::new();
      let mut seen = HashSet::new();
      for (i, path) in files.into_iter().enumerate() {
        // Un archivo que no se puede reproducir no entra al indice.
        // Un archivo que hace fallar al decoder se salta: el escaneo siempre
        // termina y siempre llega el progreso final.
        let probed = std::panic::catch_unwind(|| metadata::probe(&path));
        if let Ok(Ok(info)) = probed {
          seen.insert(path.clone());
          entries.push(Entry { path, info });
        }
        // Se guarda por tandas: un corte a mitad no pierde lo ya leido.
        if entries.len() >= BATCH {
          let _ = jobs.send(Job::Batch(std::mem::take(&mut entries)));
        }
        let done = i as u64 + 1;
        if done < total && last.elapsed() >= PROGRESS_EVERY {
          progress(done);
          last = Instant::now();
        }
      }
      // El progreso final lo manda el worker, despues de indexar.
      if !entries.is_empty() {
        let _ = jobs.send(Job::Batch(entries));
      }
      let _ = jobs.send(Job::Indexed { job, roots, seen, total });
    });
  }
}

fn xdg(var: &str, home_rel: &str) -> Option<PathBuf> {
  std::env::var_os(var)
    .map(PathBuf::from)
    .filter(|p| p.is_absolute())
    .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(home_rel)))
    .map(|b| b.join("ketai"))
}

pub fn data_dir() -> Option<PathBuf> {
  xdg("XDG_DATA_HOME", ".local/share")
}

fn cache_dir() -> Option<PathBuf> {
  xdg("XDG_CACHE_HOME", ".cache")
}

fn track_json(e: &Entry) -> Value {
  let i = &e.info;
  json!({
    "id": null,
    "source": "local",
    "uri": path_to_file_uri(&e.path),
    "title": i.title,
    "artist": i.artist,
    "album": i.album,
    "duration_ms": i.duration_ms,
    "has_cover": i.has_cover,
    "codec": i.codec,
    "sample_rate": i.sample_rate,
    "channels": i.channels,
    "album_artist": i.album_artist,
    "genre": i.genre,
    "year": i.year,
    "track_number": i.track_number,
    "disc_number": i.disc_number,
    "bitrate_kbps": null,
  })
}

/// Carpeta absoluta que existe.
fn existing_folder(v: &Value) -> Option<PathBuf> {
  let p = PathBuf::from(v.as_str()?);
  (p.is_absolute() && p.is_dir()).then_some(p)
}

/// Indice y carpetas de versiones anteriores (`library.json`), para
/// importarlos a la base la primera vez. Si no existe o no se entiende,
/// queda vacio. Un indice sin `folders` se lee con la lista vacia.
fn load_index(path: &Path) -> (Vec<Entry>, Vec<PathBuf>) {
  let Ok(text) = fs::read_to_string(path) else { return Default::default() };
  let Ok(v) = serde_json::from_str::<Value>(&text) else { return Default::default() };
  if v["version"] != INDEX_VERSION {
    return Default::default();
  }
  let folders = v["folders"].as_array().map(|f| f.iter().filter_map(|p| p.as_str().map(PathBuf::from)).collect()).unwrap_or_default();
  let s = |t: &Value, k: &str| t[k].as_str().map(str::to_string);
  let tracks = v["tracks"]
    .as_array()
    .map(|tracks| {
      tracks
        .iter()
        .filter_map(|t| {
          Some(Entry {
            path: PathBuf::from(t["path"].as_str()?),
            info: TrackInfo {
              title: s(t, "title"),
              artist: s(t, "artist"),
              album: s(t, "album"),
              duration_ms: t["duration_ms"].as_u64(),
              has_cover: t["has_cover"].as_bool().unwrap_or(false),
              codec: s(t, "codec"),
              sample_rate: t["sample_rate"].as_u64().and_then(|n| u32::try_from(n).ok()),
              channels: t["channels"].as_u64().and_then(|n| u16::try_from(n).ok()),
              ..Default::default()
            },
          })
        })
        .collect()
    })
    .unwrap_or_default();
  (tracks, folders)
}

/// Nombre de archivo de la caratula de una pista pedida por URI:
/// `u_<hash>`, siempre el mismo para la misma URI (FNV-1a de 64 bits).
pub fn uri_cover_id(uri: &str) -> String {
  let mut h: u64 = 0xcbf2_9ce4_8422_2325;
  for b in uri.bytes() {
    h ^= b as u64;
    h = h.wrapping_mul(0x0100_0000_01b3);
  }
  format!("u_{h:016x}")
}

/// Escribe la caratula en `$XDG_RUNTIME_DIR/ketai/covers/<track_id>.<ext>`
/// (carpeta 0700, archivo 0600) y devuelve la ruta.
fn write_cover(track_id: &str, cover: &metadata::Cover) -> std::io::Result<PathBuf> {
  let runtime = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).ok_or_else(|| std::io::Error::other("XDG_RUNTIME_DIR is not set"))?;
  let dir = runtime.join("ketai").join("covers");
  fs::DirBuilder::new().recursive(true).mode(0o700).create(&dir)?;
  fs::set_permissions(&dir, fs::Permissions::from_mode(0o700))?;
  let ext = match cover.mime.as_str() {
    "image/png" => "png",
    "image/webp" => "webp",
    "image/gif" => "gif",
    _ => "jpg",
  };
  // Los ids de pista solo tienen letras, numeros y `_`; igual se filtra
  // para que nunca salga de la carpeta.
  let name: String = track_id.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '_').collect();
  let path = dir.join(format!("{name}.{ext}"));
  // O_NOFOLLOW: un enlace puesto en lugar de la caratula no se sigue.
  let mut f =
    fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).custom_flags(libc::O_NOFOLLOW).open(&path)?;
  f.write_all(&cover.data)?;
  Ok(path)
}

/// Tamano total y cantidad de archivos bajo `dir`.
fn dir_usage(dir: &Path) -> (u64, u64) {
  let (mut size, mut count) = (0, 0);
  let mut pending = vec![dir.to_path_buf()];
  while let Some(d) = pending.pop() {
    let Ok(entries) = fs::read_dir(&d) else { continue };
    for e in entries.flatten() {
      let Ok(kind) = e.file_type() else { continue };
      if kind.is_dir() {
        pending.push(e.path());
      } else if let Ok(meta) = e.metadata() {
        size += meta.len();
        count += 1;
      }
    }
  }
  (size, count)
}

/// Borra el contenido de `dir` (no la carpeta) y devuelve los bytes
/// liberados.
fn clear_dir(dir: &Path) -> u64 {
  let (size, _) = dir_usage(dir);
  let Ok(entries) = fs::read_dir(dir) else { return 0 };
  for e in entries.flatten() {
    let path = e.path();
    let _ = if e.file_type().is_ok_and(|k| k.is_dir()) { fs::remove_dir_all(&path) } else { fs::remove_file(&path) };
  }
  // Puede crecer mientras se borra: nunca un numero negativo.
  size.saturating_sub(dir_usage(dir).0)
}
