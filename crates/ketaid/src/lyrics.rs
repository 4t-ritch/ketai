//! Metodos `lyrics.*` y el evento `lyrics.changed`.
//!
//! Buscar y leer letras es I/O de disco: lo hace un unico thread, en el orden
//! en que llegan las peticiones. Los offsets del usuario se guardan por
//! archivo (URI) en `$XDG_STATE_HOME/ketai/lyrics-offsets.json`, porque el
//! `track_id` cambia en cada sesion.

use std::{
  collections::BTreeMap,
  fs,
  path::PathBuf,
  sync::mpsc::{self, Sender},
  thread,
};

use ketai_core::lyrics::{
  layers::{self, Layer},
  package::{self, BuildError, Package},
};
use ketai_protocol::{ErrorReply, Request, codes};
use serde_json::{Map, Value, json};

use crate::server::{ClientId, Message};

const INVALID_LYRICS: i64 = 1006;
const LAYER_NOT_FOUND: i64 = 1007;

/// Pista a la que se refiere la peticion, resuelta por el loop central.
pub struct TrackRef {
  pub uri: String,
  pub path: Option<PathBuf>,
}

struct Job {
  client: ClientId,
  request_id: u64,
  method: String,
  params: Map<String, Value>,
  track: TrackRef,
}

pub struct Done {
  pub client: ClientId,
  pub request_id: u64,
  pub result: Result<Value, ErrorReply>,
  /// `track_id` para `lyrics.changed` si cambio un offset.
  pub changed: Option<String>,
}

pub struct LyricsService {
  jobs: Sender<Job>,
  changed: Vec<String>,
}

impl LyricsService {
  pub fn new(tx: Sender<Message>) -> Self {
    let (jobs, rx) = mpsc::channel::<Job>();
    thread::Builder::new()
      .name("ketai-lyrics".into())
      .spawn(move || {
        let mut worker = Worker::load();
        for job in rx {
          let done = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| worker.run(&job))).unwrap_or_else(|_| Done {
            client: job.client,
            request_id: job.request_id,
            result: Err(ErrorReply::new(Some(job.request_id), INVALID_LYRICS, "lyrics could not be read")),
            changed: None,
          });
          if tx.send(Message::Lyrics(done)).is_err() {
            return;
          }
        }
      })
      .expect("spawn lyrics thread");
    Self { jobs, changed: Vec::new() }
  }

  pub fn handles(method: &str) -> bool {
    method.starts_with("lyrics.")
  }

  /// `track_id` de la peticion, o el error a responder si falta.
  pub fn track_id(req: &Request) -> Result<String, ErrorReply> {
    req
      .params
      .get("track_id")
      .and_then(Value::as_str)
      .map(str::to_string)
      .ok_or_else(|| ErrorReply::new(Some(req.id), codes::INVALID_PARAMS, "track_id is required"))
  }

  pub fn submit(&self, client: ClientId, req: &Request, track: TrackRef) {
    let _ = self.jobs.send(Job {
      client,
      request_id: req.id,
      method: req.method.clone(),
      params: req.params.clone(),
      track,
    });
  }

  pub fn note_changed(&mut self, track_id: Option<String>) {
    self.changed.extend(track_id);
  }

  pub fn take_changed(&mut self) -> Vec<String> {
    std::mem::take(&mut self.changed)
  }
}

struct Worker {
  path: Option<PathBuf>,
  offsets: BTreeMap<String, i64>,
}

impl Worker {
  fn load() -> Self {
    let path = crate::session::path().and_then(|p| p.parent().map(|d| d.join("lyrics-offsets.json")));
    let offsets = path
      .as_ref()
      .and_then(|p| fs::read_to_string(p).ok())
      .and_then(|t| serde_json::from_str::<BTreeMap<String, i64>>(&t).ok())
      .unwrap_or_default();
    Self { path, offsets }
  }

  fn run(&mut self, job: &Job) -> Done {
    let id = job.request_id;
    let track_id = job.params.get("track_id").and_then(Value::as_str).unwrap_or_default().to_string();
    let mut changed = None;
    let result = match job.method.as_str() {
      "lyrics.layers" => self.discover(id, &job.track).map(|layers| match layers {
        None => json!({"layers": []}),
        Some(layers) => {
          let offered = package::offered(&layers);
          json!({"layers": offered.iter().map(|(l, t)| layer_json(l, t.as_str())).collect::<Vec<_>>()})
        }
      }),
      "lyrics.get" => self.get(id, job),
      "lyrics.set_offset" => match job.params.get("offset_ms").and_then(Value::as_i64) {
        Some(offset) => {
          if offset == 0 {
            self.offsets.remove(&job.track.uri);
          } else {
            self.offsets.insert(job.track.uri.clone(), offset);
          }
          self.save();
          changed = Some(track_id);
          Ok(json!({}))
        }
        None => Err(ErrorReply::new(Some(id), codes::INVALID_PARAMS, "offset_ms must be an integer")),
      },
      _ => Err(ErrorReply::new(Some(id), codes::METHOD_NOT_FOUND, "method not found")),
    };
    Done { client: job.client, request_id: id, result, changed }
  }

  /// Capas de la pista. None si no tiene letra; error si solo hay origenes
  /// rotos.
  fn discover(&self, id: u64, track: &TrackRef) -> Result<Option<Vec<Layer>>, ErrorReply> {
    // Pistas remotas: todavia no hay letras para ellas.
    let Some(path) = &track.path else { return Ok(None) };
    let found = layers::discover(path);
    if found.layers.is_empty() {
      return if found.invalid {
        Err(ErrorReply::new(Some(id), INVALID_LYRICS, "lyrics found but none could be read"))
      } else {
        Ok(None)
      };
    }
    Ok(Some(found.layers))
  }

  fn get(&self, id: u64, job: &Job) -> Result<Value, ErrorReply> {
    let invalid = |msg: &str| ErrorReply::new(Some(id), codes::INVALID_PARAMS, msg.to_string());
    let primary = match job.params.get("primary") {
      None | Some(Value::Null) => "original".to_string(),
      Some(Value::String(s)) => s.clone(),
      Some(_) => return Err(invalid("primary must be a string")),
    };
    let secondary: Vec<String> = match job.params.get("secondary") {
      None | Some(Value::Null) => Vec::new(),
      Some(Value::Array(a)) => a
        .iter()
        .map(|v| v.as_str().map(str::to_string).ok_or_else(|| invalid("secondary must be a list of strings")))
        .collect::<Result<_, _>>()?,
      Some(_) => return Err(invalid("secondary must be a list of strings")),
    };
    let estimate = match job.params.get("estimate_words") {
      None | Some(Value::Null) => false,
      Some(Value::Bool(b)) => *b,
      Some(_) => return Err(invalid("estimate_words must be a boolean")),
    };

    let Some(layers) = self.discover(id, &job.track)? else { return Ok(Value::Null) };
    // Solo se pueden pedir capas que se ofrecen (las que se alinean).
    let offered: Vec<&str> = package::offered(&layers).iter().map(|(l, _)| l.id.as_str()).collect();
    for wanted in std::iter::once(&primary).chain(&secondary) {
      if !offered.contains(&wanted.as_str()) {
        return Err(ErrorReply::new(Some(id), LAYER_NOT_FOUND, format!("lyrics layer not found: {wanted}")));
      }
    }
    let user_offset = self.offsets.get(&job.track.uri).copied().unwrap_or(0);
    match package::build(&layers, &primary, &secondary, estimate, user_offset) {
      Ok(p) => Ok(package_json(&p)),
      Err(BuildError::UnknownLayer(l)) => Err(ErrorReply::new(Some(id), LAYER_NOT_FOUND, format!("lyrics layer not found: {l}"))),
      Err(BuildError::Repeated(_)) => Err(invalid("the same layer cannot be primary and secondary")),
    }
  }

  fn save(&self) {
    let Some(path) = &self.path else { return };
    let content = serde_json::to_string_pretty(&self.offsets).unwrap_or_default();
    if let Err(e) = crate::session::write_atomic(path, content.as_bytes()) {
      eprintln!("ketaid: could not save lyrics offsets: {e}");
    }
  }
}

fn layer_json(l: &Layer, timing: &str) -> Value {
  json!({
    "id": l.id,
    "kind": l.kind.as_str(),
    "language": l.language,
    "timing": timing,
    "format": l.format.as_str(),
    "origin": l.origin.as_str(),
  })
}

fn package_json(p: &Package) -> Value {
  json!({
    "primary": p.primary,
    "secondary": p.secondary,
    "timing": p.timing,
    "offset_ms": p.offset_ms,
    "user_offset_ms": p.user_offset_ms,
    "agents": p.agents.iter().map(|(id, name)| json!({"id": id, "name": name})).collect::<Vec<_>>(),
    "lines": p.lines.iter().map(|l| json!({
      "start_ms": l.start_ms,
      "end_ms": l.end_ms,
      "text": l.text,
      "agent": l.agent,
      "background": l.background,
      "words": l.words.as_ref().map(|ws| ws.iter().map(|w| json!({
        "start_ms": w.start_ms, "end_ms": w.end_ms, "text": w.text,
      })).collect::<Vec<_>>()),
      "secondary": l.secondary.iter().map(|(k, v)| (k.clone(), Value::from(v.clone()))).collect::<Map<_, _>>(),
    })).collect::<Vec<_>>(),
  })
}
