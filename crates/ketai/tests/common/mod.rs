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
/// Carpetas creadas por este proceso: se borran al salir (ver `cleanup`).
static CREATED: std::sync::Mutex<Vec<PathBuf>> = std::sync::Mutex::new(Vec::new());
static SETUP: std::sync::Once = std::sync::Once::new();

const PREFIX: &str = "ketaid-test-";

/// Al terminar el proceso de tests (con tests que fallaron o no) se borran
/// todas sus carpetas. Un test que falla antes de su `remove_dir_all` ya no
/// deja nada.
extern "C" fn cleanup() {
  let dirs = std::mem::take(&mut *CREATED.lock().unwrap_or_else(|e| e.into_inner()));
  for d in dirs {
    let _ = std::fs::remove_dir_all(d);
  }
}

/// Borra carpetas de corridas que ya no existen (cortadas de golpe, por
/// ejemplo con Ctrl+C, que no llegan a `cleanup`).
pub fn sweep_dead_runs() {
  let Ok(entries) = std::fs::read_dir(std::env::temp_dir()) else { return };
  for e in entries.flatten() {
    let name = e.file_name().to_string_lossy().into_owned();
    let Some(pid) = name.strip_prefix(PREFIX).and_then(|r| r.split('-').next()).and_then(|p| p.parse::<i32>().ok()) else {
      continue;
    };
    // kill(pid, 0) falla con ESRCH si el proceso no existe.
    let alive = unsafe { libc::kill(pid, 0) } == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM);
    if !alive {
      let _ = std::fs::remove_dir_all(e.path());
    }
  }
}

/// Directorio temporal propio de cada test, fuera del worktree. Se borra
/// al terminar el proceso de tests aunque el test falle.
pub fn temp_runtime_dir() -> PathBuf {
  SETUP.call_once(|| {
    sweep_dead_runs();
    unsafe { libc::atexit(cleanup) };
  });
  let n = NEXT_DIR.fetch_add(1, Ordering::SeqCst);
  let dir = std::env::temp_dir().join(format!("{PREFIX}{}-{n}", std::process::id()));
  let _ = std::fs::remove_dir_all(&dir);
  std::fs::create_dir_all(&dir).unwrap();
  CREATED.lock().unwrap_or_else(|e| e.into_inner()).push(dir.clone());
  dir
}

/// Bus D-Bus que no existe: con esto el daemon arranca sin MPRIS.
pub const NO_BUS: &str = "unix:path=/dev/null/ketai-no-bus";
/// HOME que no existe: nada derivado de el se puede crear.
pub const NO_HOME: &str = "/dev/null/ketai-no-home";

/// Comando para lanzar ketaid desde un test. Es la unica forma permitida
/// (lo comprueba `every_test_spawns_ketaid_through_the_isolated_command`):
/// nunca usa el bus de sesion del usuario, asi un daemon de test no aparece
/// en su playerctl o waybar. Los tests de MPRIS pasan un bus propio con
/// `.env("DBUS_SESSION_BUS_ADDRESS", ...)`, que reemplaza a este.
///
/// Tampoco hereda los directorios del usuario: borra las `XDG_*_HOME` y
/// apunta `HOME` a una ruta inexistente. Cada test fija los que usa.
pub fn ketaid_command() -> Command {
  let mut cmd = Command::new(env!("CARGO_BIN_EXE_ketaid"));
  cmd
    .env("DBUS_SESSION_BUS_ADDRESS", NO_BUS)
    .env("HOME", NO_HOME)
    .env_remove("XDG_STATE_HOME")
    .env_remove("XDG_DATA_HOME")
    .env_remove("XDG_CACHE_HOME")
    .env_remove("XDG_CONFIG_HOME")
    .env_remove("XDG_MUSIC_DIR");
  cmd
}

pub struct Daemon {
  pub child: Child,
  pub runtime_dir: PathBuf,
  /// Si es false, el directorio sobrevive al Drop (para reiniciar el daemon
  /// sobre el mismo estado).
  cleanup: bool,
}

impl Daemon {
  /// Lanza el daemon con un `XDG_RUNTIME_DIR` aislado y espera el socket.
  pub fn start() -> Self {
    let mut daemon = Self::start_in(temp_runtime_dir(), &[]);
    daemon.cleanup = true;
    daemon
  }

  /// Lanza el daemon sobre un directorio existente, sin borrarlo al final.
  /// El estado de sesion va a `<dir>/state`, nunca al del usuario.
  pub fn start_in(runtime_dir: PathBuf, envs: &[(&str, &str)]) -> Self {
    let child = ketaid_command()
      .env("XDG_RUNTIME_DIR", &runtime_dir)
      .env("XDG_STATE_HOME", runtime_dir.join("state"))
      .env("XDG_DATA_HOME", runtime_dir.join("data"))
      .env("XDG_CACHE_HOME", runtime_dir.join("cache"))
      .env("XDG_CONFIG_HOME", runtime_dir.join("config"))
      // Las playlists van por defecto en XDG_MUSIC_DIR/playlists.
      .env("XDG_MUSIC_DIR", runtime_dir.join("music"))
      // Los tests nunca suenan por los parlantes.
      .env("KETAI_AUDIO_BACKEND", "null")
      .envs(envs.iter().copied())
      .stdout(Stdio::null())
      .stderr(Stdio::null())
      .spawn()
      .expect("spawn ketaid");
    let daemon = Self { child, runtime_dir, cleanup: false };
    daemon.wait_for_socket();
    daemon
  }

  pub fn session_file(&self) -> PathBuf {
    self.runtime_dir.join("state").join("ketai").join("session.json")
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
    if self.cleanup {
      let _ = std::fs::remove_dir_all(&self.runtime_dir);
    }
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
