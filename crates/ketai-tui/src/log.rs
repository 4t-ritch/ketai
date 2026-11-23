//! Registro opcional para diagnosticar (`KETAI_LOG=/ruta/archivo`).
//!
//! Anota con hora (UTC, como el log de herdr) los eventos de la terminal
//! (foco, tamano, mouse) y lo que se manda de la capa de imagenes. Sin la
//! variable no hace nada.

use std::{
  fs::File,
  io::Write,
  sync::{Mutex, OnceLock},
  time::{SystemTime, UNIX_EPOCH},
};

static FILE: OnceLock<Option<Mutex<File>>> = OnceLock::new();

fn file() -> Option<&'static Mutex<File>> {
  FILE
    .get_or_init(|| {
      let path = std::env::var_os("KETAI_LOG")?;
      std::fs::OpenOptions::new().create(true).append(true).open(path).ok().map(Mutex::new)
    })
    .as_ref()
}

/// Hora UTC `HH:MM:SS.mmm`.
pub fn now() -> String {
  let d = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
  let s = d.as_secs() % 86_400;
  format!("{:02}:{:02}:{:02}.{:03}", s / 3600, s / 60 % 60, s % 60, d.subsec_millis())
}

/// Anota una linea si `KETAI_LOG` esta definida.
pub fn log(msg: impl std::fmt::Display) {
  if let Some(f) = file()
    && let Ok(mut f) = f.lock()
  {
    let _ = writeln!(f, "{} {msg}", now());
  }
}

pub fn enabled() -> bool {
  file().is_some()
}
