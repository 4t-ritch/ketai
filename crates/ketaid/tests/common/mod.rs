//! Utilidades para los tests de conformidad.
//!
//! Los tests hablan con el daemon solo por el socket, como un cliente real.
//! Asi la misma suite puede correrse despues contra el core en C.

#![allow(dead_code)]

use std::{
  io::{BufRead, BufReader, Write},
  os::unix::net::UnixStream,
  path::{Path, PathBuf},
  process::{Child, Command, Stdio},
  sync::atomic::{AtomicU32, Ordering},
  thread,
  time::{Duration, Instant},
};

use serde_json::Value;

static NEXT_DIR: AtomicU32 = AtomicU32::new(0);

/// Directorio temporal propio de cada test, fuera del worktree.
pub fn temp_runtime_dir() -> PathBuf {
  let n = NEXT_DIR.fetch_add(1, Ordering::SeqCst);
  let dir = std::env::temp_dir().join(format!("ketaid-test-{}-{n}", std::process::id()));
  let _ = std::fs::remove_dir_all(&dir);
  std::fs::create_dir_all(&dir).unwrap();
  dir
}

pub struct Daemon {
  pub child: Child,
  pub runtime_dir: PathBuf,
}

impl Daemon {
  /// Lanza el daemon con un `XDG_RUNTIME_DIR` aislado y espera el socket.
  pub fn start() -> Self {
    let runtime_dir = temp_runtime_dir();
    let child = Command::new(env!("CARGO_BIN_EXE_ketaid"))
      .env("XDG_RUNTIME_DIR", &runtime_dir)
      // Los tests nunca suenan por los parlantes.
      .env("KETAI_AUDIO_BACKEND", "null")
      .stdout(Stdio::null())
      .stderr(Stdio::null())
      .spawn()
      .expect("spawn ketaid");
    let daemon = Self { child, runtime_dir };
    daemon.wait_for_socket();
    daemon
  }

  pub fn socket_path(&self) -> PathBuf {
    self.runtime_dir.join("ketai").join("ctl.sock")
  }

  fn wait_for_socket(&self) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
      if UnixStream::connect(self.socket_path()).is_ok() {
        return;
      }
      thread::sleep(Duration::from_millis(20));
    }
    panic!("ketaid did not open {}", self.socket_path().display());
  }

  pub fn connect(&self) -> Client {
    Client::connect(&self.socket_path())
  }

  /// Espera a que el proceso termine y devuelve si salio con exito.
  pub fn wait_exit(&mut self, timeout: Duration) -> Option<bool> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
      if let Some(status) = self.child.try_wait().unwrap() {
        return Some(status.success());
      }
      thread::sleep(Duration::from_millis(20));
    }
    None
  }
}

impl Drop for Daemon {
  fn drop(&mut self) {
    let _ = self.child.kill();
    let _ = self.child.wait();
    let _ = std::fs::remove_dir_all(&self.runtime_dir);
  }
}

pub struct Client {
  pub writer: UnixStream,
  pub reader: BufReader<UnixStream>,
}

impl Client {
  pub fn connect(path: &Path) -> Self {
    let stream = UnixStream::connect(path).expect("connect ctl.sock");
    stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    let reader = BufReader::new(stream.try_clone().unwrap());
    Self { writer: stream, reader }
  }

  pub fn send_raw(&mut self, bytes: &[u8]) {
    self.writer.write_all(bytes).unwrap();
  }

  /// Lee la siguiente linea JSON. Devuelve None si el daemon cerro.
  pub fn read_message(&mut self) -> Option<Value> {
    let mut line = String::new();
    match self.reader.read_line(&mut line) {
      Ok(0) | Err(_) => None,
      Ok(_) => Some(serde_json::from_str(&line).expect("daemon sent invalid JSON")),
    }
  }

  /// Envia una peticion y devuelve la respuesta con el mismo id.
  pub fn request(&mut self, id: u64, method: &str, params: Value) -> Value {
    let msg = serde_json::json!({"id": id, "method": method, "params": params});
    self.send_raw(format!("{msg}\n").as_bytes());
    loop {
      let reply = self.read_message().expect("connection closed");
      if reply.get("id") == Some(&Value::from(id)) {
        return reply;
      }
    }
  }
}

/// URI `file://` de un fixture del core.
pub fn fixture_uri(name: &str) -> String {
  let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
    .join("../ketai-core/tests/fixtures")
    .join(name)
    .canonicalize()
    .expect("fixture exists");
  format!("file://{}", path.display())
}
