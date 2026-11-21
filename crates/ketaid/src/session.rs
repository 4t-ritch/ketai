//! Persistencia de sesion (docs/servicio.md, "Persistencia de sesion").
//!
//! La escritura a disco ocurre en un thread propio: el loop central solo le
//! manda el JSON a guardar y nunca espera, salvo en el guardado final.

use std::{
  fs::{self, File},
  io::{self, Write},
  os::unix::fs::DirBuilderExt,
  path::{Path, PathBuf},
  sync::mpsc::{self, Sender},
  thread,
  time::Duration,
};

use serde_json::Value;

pub const VERSION: u64 = 1;

/// Ruta del archivo: `$XDG_STATE_HOME/ketai/session.json`, o
/// `~/.local/state/ketai/session.json` si `XDG_STATE_HOME` no esta definido.
pub fn path() -> Option<PathBuf> {
  let base = std::env::var_os("XDG_STATE_HOME")
    .map(PathBuf::from)
    .filter(|p| p.is_absolute())
    .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state")))?;
  Some(base.join("ketai").join("session.json"))
}

/// Sesion guardada, ya validada.
#[derive(Debug, Clone, PartialEq)]
pub struct Saved {
  pub queue: Vec<String>,
  pub current: Option<usize>,
  pub position_ms: u64,
  pub volume: f64,
  pub muted: bool,
  pub repeat: String,
  pub shuffle: bool,
  pub status: String,
  pub output: Option<String>,
}

/// Lee la sesion. Si no existe devuelve None. Si existe pero no se puede
/// usar, la renombra a `.bad`, lo registra y devuelve None.
pub fn load(path: &Path) -> Option<Saved> {
  let text = match fs::read_to_string(path) {
    Ok(t) => t,
    Err(e) if e.kind() == io::ErrorKind::NotFound => return None,
    Err(e) => return discard(path, &format!("unreadable: {e}")),
  };
  match parse(&text) {
    Some(saved) => Some(saved),
    None => discard(path, "invalid content"),
  }
}

fn discard(path: &Path, reason: &str) -> Option<Saved> {
  let bad = path.with_extension("json.bad");
  eprintln!("ketaid: session file {} is {reason}; moved to {}", path.display(), bad.display());
  let _ = fs::rename(path, bad);
  None
}

fn parse(text: &str) -> Option<Saved> {
  let v: Value = serde_json::from_str(text).ok()?;
  if v.get("version")?.as_u64()? != VERSION {
    return None;
  }
  let queue = v
    .get("queue")?
    .as_array()?
    .iter()
    .map(|u| u.as_str().map(str::to_string))
    .collect::<Option<Vec<_>>>()?;
  let current = match v.get("current") {
    None | Some(Value::Null) => None,
    Some(c) => Some(c.as_u64()? as usize).filter(|i| *i < queue.len()),
  };
  let volume = v.get("volume")?.as_f64().filter(|x| (0.0..=1.0).contains(x))?;
  let repeat = v.get("repeat")?.as_str().filter(|r| ["off", "track", "queue"].contains(r))?.to_string();
  Some(Saved {
    current,
    position_ms: v.get("position_ms").and_then(Value::as_u64).unwrap_or(0),
    volume,
    muted: v.get("muted")?.as_bool()?,
    repeat,
    shuffle: v.get("shuffle")?.as_bool()?,
    status: v.get("status").and_then(Value::as_str).unwrap_or("paused").to_string(),
    output: v.get("output").and_then(Value::as_str).map(str::to_string),
    queue,
  })
}

enum Job {
  Save(String),
  /// Avisa por el canal cuando todo lo anterior ya esta en disco.
  Flush(Sender<()>),
}

pub struct Writer {
  tx: Sender<Job>,
}

impl Writer {
  pub fn spawn(path: PathBuf) -> Self {
    let (tx, rx) = mpsc::channel::<Job>();
    thread::Builder::new()
      .name("ketai-session".into())
      .spawn(move || {
        for job in rx {
          match job {
            Job::Save(content) => {
              if let Err(e) = write_atomic(&path, content.as_bytes()) {
                eprintln!("ketaid: could not save session: {e}");
              }
            }
            Job::Flush(done) => {
              let _ = done.send(());
            }
          }
        }
      })
      .expect("spawn session writer");
    Self { tx }
  }

  /// Encola un guardado. No bloquea.
  pub fn save(&self, content: String) {
    let _ = self.tx.send(Job::Save(content));
  }

  /// Guarda y espera a que llegue al disco (solo para el apagado).
  pub fn save_and_wait(&self, content: String, timeout: Duration) {
    let (done_tx, done_rx) = mpsc::channel();
    self.save(content);
    let _ = self.tx.send(Job::Flush(done_tx));
    let _ = done_rx.recv_timeout(timeout);
  }
}

/// Escribe en `.tmp`, hace fsync y renombra: un corte nunca deja el archivo
/// a medio escribir.
pub fn write_atomic(path: &Path, content: &[u8]) -> io::Result<()> {
  let dir = path.parent().ok_or_else(|| io::Error::other("session path has no parent"))?;
  fs::DirBuilder::new().recursive(true).mode(0o700).create(dir)?;
  // `<archivo>.tmp` en la misma carpeta (el rename debe ser en el mismo
  // sistema de archivos).
  let mut tmp_name = path.file_name().ok_or_else(|| io::Error::other("path has no file name"))?.to_os_string();
  tmp_name.push(".tmp");
  let tmp = path.with_file_name(tmp_name);
  {
    let mut file = File::create(&tmp)?;
    file.write_all(content)?;
    file.sync_all()?;
  }
  fs::rename(&tmp, path)?;
  // fsync del directorio para que el rename tambien sea durable.
  File::open(dir)?.sync_all()
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn parse_rejects_wrong_version_and_bad_fields() {
    let ok = r#"{"version":1,"queue":["file:///a"],"current":0,"position_ms":5,"volume":0.5,"muted":false,"repeat":"off","shuffle":false,"output":null,"status":"paused"}"#;
    assert!(parse(ok).is_some());
    assert!(parse(&ok.replace("\"version\":1", "\"version\":2")).is_none());
    assert!(parse(&ok.replace("0.5", "3.0")).is_none());
    assert!(parse(&ok.replace("\"off\"", "\"sometimes\"")).is_none());
  }

  #[test]
  fn out_of_range_current_is_dropped() {
    let text = r#"{"version":1,"queue":["file:///a"],"current":4,"volume":1.0,"muted":false,"repeat":"off","shuffle":false}"#;
    assert_eq!(parse(text).unwrap().current, None);
  }
}
