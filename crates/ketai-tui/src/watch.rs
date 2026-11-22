//! Recarga en vivo de config.toml.
//!
//! Un thread revisa el archivo cada 500 ms. Compara fecha de modificacion,
//! tamano e inodo: asi detecta tanto una escritura como un reemplazo (vim o
//! helix escriben un temporal y lo renombran encima). Sin dependencias ni
//! inotify: se traduce directo a C con `stat`.

use std::{
  os::unix::fs::MetadataExt,
  path::PathBuf,
  sync::mpsc::Sender,
  thread,
  time::Duration,
};

use crate::config::Config;

const POLL: Duration = Duration::from_millis(500);

/// Huella del archivo; `None` si no existe.
fn stamp(path: &PathBuf) -> Option<(i64, i64, u64, u64)> {
  let m = std::fs::metadata(path).ok()?;
  Some((m.mtime(), m.mtime_nsec(), m.size(), m.ino()))
}

/// Vigila `path` y manda la configuracion nueva (o el error) cada vez que
/// cambia. La carga inicial la hace quien llama.
pub fn spawn<T: Send + 'static>(path: PathBuf, tx: Sender<T>, wrap: fn(Result<Config, String>) -> T) {
  // La huella inicial se toma antes de lanzar el thread: un cambio justo
  // despues de la carga inicial no se pierde.
  let mut last = stamp(&path);
  let _ = thread::Builder::new().name("ketai-config".into()).spawn(move || {
    loop {
      thread::sleep(POLL);
      let now = stamp(&path);
      if now == last {
        continue;
      }
      last = now;
      if tx.send(wrap(Config::load(&path))).is_err() {
        return;
      }
    }
  });
}
