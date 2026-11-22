//! Metodos `library.*`, `track.cover` y `cache.*` (version local).
//!
//! Todo es I/O de disco, asi que lo atiende un unico thread (el worker), en
//! el orden en que llegan las peticiones. El worker es el dueno del indice.
//! Un escaneo corre en un thread aparte: informa el progreso al loop central
//! y al terminar le entrega las pistas al worker, que actualiza el indice y
//! lo guarda en `$XDG_DATA_HOME/ketai/library.json`.

use std::{
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

use crate::server::{ClientId, Message};

const DEFAULT_LIMIT: u64 = 50;
/// Limite de la cache que se informa (las descargas llegan en una version
/// futura).
const CACHE_LIMIT_BYTES: u64 = 2 * 1024 * 1024 * 1024;
const INDEX_VERSION: u64 = 1;
/// Cada cuanto se informa el progreso de un escaneo como maximo.
const PROGRESS_EVERY: Duration = Duration::from_millis(200);

/// Pista de la cola a la que se refiere `track.cover`.
pub struct TrackRef {
  pub id: String,
  pub path: Option<PathBuf>,
}

enum Job {
  Request { client: ClientId, request_id: u64, method: String, params: Map<String, Value>, track: Option<TrackRef> },
  /// Un escaneo termino: reemplazar lo indexado bajo cada carpeta.
  Indexed { job: String, roots: Vec<PathBuf>, entries: Vec<Entry>, total: u64 },
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
            Job::Indexed { job, roots, entries, total } => {
              // Si fallara, el worker sigue vivo para atender pedidos.
              if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| worker.replace(&roots, entries))).is_err() {
                eprintln!("ketaid: library index update failed");
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
  index: Vec<Entry>,
  /// Carpetas de la biblioteca (`library.add_folder`).
  folders: Vec<PathBuf>,
  index_path: Option<PathBuf>,
  tx: Sender<Message>,
  jobs: Sender<Job>,
  next_job: u64,
}

impl Worker {
  fn load(tx: Sender<Message>, jobs: Sender<Job>) -> Self {
    let index_path = data_dir().map(|d| d.join("library.json"));
    let (index, folders) = index_path.as_deref().map(load_index).unwrap_or_default();
    Self { index, folders, index_path, tx, jobs, next_job: 0 }
  }

  fn run(&mut self, id: u64, method: &str, params: &Map<String, Value>, track: Option<&TrackRef>) -> Result<Value, ErrorReply> {
    let invalid = |msg: &str| ErrorReply::new(Some(id), codes::INVALID_PARAMS, msg.to_string());
    match method {
      "library.scan" => {
        // Sin path: todas las carpetas de la biblioteca.
        let roots = match params.get("path") {
          None | Some(Value::Null) => {
            if self.folders.is_empty() {
              return Err(invalid("there are no library folders: add one with library.add_folder"));
            }
            self.folders.clone()
          }
          Some(p) => vec![existing_folder(p).ok_or_else(|| invalid("path must be an existing absolute folder"))?],
        };
        self.next_job += 1;
        let job = format!("j_{}", self.next_job);
        self.spawn_scan(job.clone(), roots);
        Ok(json!({"job": job}))
      }
      "library.folders" => {
        let folders: Vec<Value> = self
          .folders
          .iter()
          .map(|f| json!({"path": f, "tracks": self.index.iter().filter(|e| e.path.starts_with(f)).count()}))
          .collect();
        Ok(json!({"folders": folders}))
      }
      "library.add_folder" => {
        let folder = params.get("path").and_then(existing_folder).ok_or_else(|| invalid("path must be an existing absolute folder"))?;
        if !self.folders.contains(&folder) {
          self.folders.push(folder);
          self.save();
        }
        Ok(json!({}))
      }
      "library.remove_folder" => {
        let folder = params.get("path").and_then(Value::as_str).map(PathBuf::from);
        let at = folder.as_ref().and_then(|f| self.folders.iter().position(|x| x == f)).ok_or_else(|| invalid("path is not a library folder"))?;
        let folder = self.folders.remove(at);
        // Sus canciones dejan de ser parte de la biblioteca.
        self.index.retain(|e| !e.path.starts_with(&folder));
        self.save();
        Ok(json!({}))
      }
      "library.search" => {
        let query = params.get("query").and_then(Value::as_str).ok_or_else(|| invalid("query is required"))?;
        let limit = match params.get("limit") {
          None | Some(Value::Null) => DEFAULT_LIMIT,
          Some(v) => v.as_u64().filter(|&n| n >= 1).ok_or_else(|| invalid("limit must be a positive integer"))?,
        };
        let words = library::words(query);
        let tracks: Vec<Value> =
          self.index.iter().filter(|e| library::matches(e, &words)).take(limit as usize).map(track_json).collect();
        Ok(json!({"tracks": tracks}))
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
      for (i, path) in files.into_iter().enumerate() {
        // Un archivo que no se puede reproducir no entra al indice.
        // Un archivo que hace fallar al decoder se salta: el escaneo siempre
        // termina y siempre llega el progreso final.
        let probed = std::panic::catch_unwind(|| metadata::probe(&path));
        if let Ok(Ok(info)) = probed {
          entries.push(Entry { path, info });
        }
        let done = i as u64 + 1;
        if done < total && last.elapsed() >= PROGRESS_EVERY {
          progress(done);
          last = Instant::now();
        }
      }
      // El progreso final lo manda el worker, despues de indexar.
      let _ = jobs.send(Job::Indexed { job, roots, entries, total });
    });
  }

  fn replace(&mut self, roots: &[PathBuf], entries: Vec<Entry>) {
    self.index.retain(|e| !roots.iter().any(|r| e.path.starts_with(r)));
    self.index.extend(entries);
    self.index.sort_by(|a, b| a.path.cmp(&b.path));
    self.index.dedup_by(|a, b| a.path == b.path);
    self.save();
  }

  /// Guarda indice y carpetas en `library.json`.
  fn save(&self) {
    if let Some(path) = &self.index_path {
      let tracks: Vec<Value> = self.index.iter().map(index_json).collect();
      let content = json!({"version": INDEX_VERSION, "folders": self.folders, "tracks": tracks}).to_string();
      if let Err(e) = crate::session::write_atomic(path, content.as_bytes()) {
        eprintln!("ketaid: could not save library index: {e}");
      }
    }
  }
}

fn xdg(var: &str, home_rel: &str) -> Option<PathBuf> {
  std::env::var_os(var)
    .map(PathBuf::from)
    .filter(|p| p.is_absolute())
    .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(home_rel)))
    .map(|b| b.join("ketai"))
}

fn data_dir() -> Option<PathBuf> {
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
    "bitrate_kbps": null,
  })
}

fn index_json(e: &Entry) -> Value {
  let i = &e.info;
  json!({
    "path": e.path,
    "title": i.title,
    "artist": i.artist,
    "album": i.album,
    "duration_ms": i.duration_ms,
    "has_cover": i.has_cover,
    "codec": i.codec,
    "sample_rate": i.sample_rate,
    "channels": i.channels,
  })
}

/// Carpeta absoluta que existe.
fn existing_folder(v: &Value) -> Option<PathBuf> {
  let p = PathBuf::from(v.as_str()?);
  (p.is_absolute() && p.is_dir()).then_some(p)
}

/// Indice y carpetas guardados. Si no existe o no se entiende, se arranca
/// vacio (se reconstruye con un escaneo). Un indice sin `folders` (anterior
/// a las carpetas) se lee con la lista vacia.
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
