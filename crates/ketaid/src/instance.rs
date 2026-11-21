//! Instancia unica (docs/servicio.md, seccion "Instancia unica").
//!
//! El arbitro es un `flock` exclusivo sobre `ketaid.lock`. Solo el dueno del
//! bloqueo puede cerrar otros procesos, y como hay un unico dueno, dos
//! instancias que arrancan a la vez no pueden matarse entre si.

use std::{
  ffi::OsStr,
  fs::{self, File, OpenOptions},
  io::{self, BufRead, BufReader, Seek, Write},
  os::unix::{
    ffi::OsStrExt,
    fs::{MetadataExt, OpenOptionsExt},
    io::AsRawFd,
    net::UnixStream,
  },
  path::Path,
  thread,
  time::{Duration, Instant},
};

const HELLO_TIMEOUT: Duration = Duration::from_secs(2);
const TERM_GRACE: Duration = Duration::from_secs(5);
const LOCK_ATTEMPTS: u32 = 3;

/// Resultado de intentar ser la instancia principal.
pub enum Claim {
  /// Esta instancia es la duena. El archivo debe vivir mientras corre el
  /// daemon: al cerrarlo, el kernel libera el bloqueo.
  Owner(File),
  /// Ya hay una instancia sana atendiendo; esta debe salir con codigo 0.
  AlreadyRunning,
}

pub fn claim(dir: &Path, socket_path: &Path) -> io::Result<Claim> {
  let lock_path = dir.join("ketaid.lock");
  for _ in 0..LOCK_ATTEMPTS {
    let file = OpenOptions::new()
      .read(true)
      .write(true)
      .create(true)
      .truncate(false)
      .mode(0o600)
      .open(&lock_path)?;

    if try_lock(&file)? {
      write_pid(&file)?;
      close_orphans();
      return Ok(Claim::Owner(file));
    }

    // El bloqueo tiene dueno: ver si responde.
    if owner_responds(socket_path) {
      eprintln!("ketaid: another instance is already running");
      return Ok(Claim::AlreadyRunning);
    }

    // Dueno colgado: cerrarlo si pasa la validacion y volver a intentar.
    if let Some(pid) = read_pid(&lock_path)
      && let Some(proc_) = Process::validate(pid)
    {
      eprintln!("ketaid: owner {pid} does not respond, terminating it");
      terminate(&proc_);
    }
  }
  Err(io::Error::other("could not acquire the instance lock"))
}

fn try_lock(file: &File) -> io::Result<bool> {
  // SAFETY: flock solo recibe un descriptor valido que pertenece a `file`.
  let rc = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
  if rc == 0 {
    return Ok(true);
  }
  let err = io::Error::last_os_error();
  if err.raw_os_error() == Some(libc::EWOULDBLOCK) {
    Ok(false)
  } else {
    Err(err)
  }
}

fn write_pid(mut file: &File) -> io::Result<()> {
  file.set_len(0)?;
  file.rewind()?;
  writeln!(file, "{}", std::process::id())?;
  file.sync_all()
}

fn read_pid(lock_path: &Path) -> Option<i32> {
  fs::read_to_string(lock_path).ok()?.trim().parse().ok()
}

/// Envia `daemon.hello` al socket y espera la respuesta con timeout.
fn owner_responds(socket_path: &Path) -> bool {
  let Ok(mut stream) = UnixStream::connect(socket_path) else {
    return false;
  };
  let deadline = Instant::now() + HELLO_TIMEOUT;
  let _ = stream.set_write_timeout(Some(HELLO_TIMEOUT));
  let _ = stream.set_read_timeout(Some(HELLO_TIMEOUT));
  let hello = b"{\"id\":1,\"method\":\"daemon.hello\",\"params\":{\"client\":\"ketaid\",\"protocol\":1}}\n";
  if stream.write_all(hello).is_err() {
    return false;
  }
  let mut reader = BufReader::new(stream);
  let mut line = String::new();
  // Basta con que llegue una respuesta al id 1 antes del timeout.
  while Instant::now() < deadline {
    line.clear();
    match reader.read_line(&mut line) {
      Ok(0) | Err(_) => return false,
      Ok(_) if line.contains("\"id\":1") => return true,
      Ok(_) => continue,
    }
  }
  false
}

/// Proceso validado como `ketaid` de esta misma sesion.
struct Process {
  pid: i32,
  start_time: u64,
}

impl Process {
  /// Aplica todas las validaciones de docs/servicio.md. Devuelve None si
  /// alguna falla; en ese caso el proceso no se debe tocar.
  fn validate(pid: i32) -> Option<Self> {
    if pid <= 0 || pid as u32 == std::process::id() {
      return None;
    }
    let proc_dir = format!("/proc/{pid}");

    // Mismo usuario.
    // SAFETY: getuid no tiene precondiciones.
    let my_uid = unsafe { libc::getuid() };
    if fs::metadata(&proc_dir).ok()?.uid() != my_uid {
      return None;
    }

    // El ejecutable real debe ser ketaid (el nombre del proceso se puede
    // falsificar; el enlace exe no). Un binario reemplazado aparece como
    // "ketaid (deleted)".
    let exe = fs::read_link(format!("{proc_dir}/exe")).ok()?;
    let name = exe.file_name()?.to_string_lossy();
    if name != "ketaid" && name != "ketaid (deleted)" {
      return None;
    }

    // Misma sesion: mismo XDG_RUNTIME_DIR.
    let mine = std::env::var_os("XDG_RUNTIME_DIR")?;
    if environ_var(pid, b"XDG_RUNTIME_DIR")?.as_slice() != mine.as_bytes() {
      return None;
    }

    Some(Self { pid, start_time: start_time(pid)? })
  }

  /// Comprueba que el PID sigue siendo el mismo proceso (no se reutilizo).
  fn still_same(&self) -> bool {
    start_time(self.pid) == Some(self.start_time)
  }
}

fn environ_var(pid: i32, key: &[u8]) -> Option<Vec<u8>> {
  let data = fs::read(format!("/proc/{pid}/environ")).ok()?;
  data.split(|b| *b == 0).find_map(|entry| {
    let rest = entry.strip_prefix(key)?;
    rest.strip_prefix(b"=").map(<[u8]>::to_vec)
  })
}

/// Campo 22 de /proc/<pid>/stat: momento de inicio del proceso.
fn start_time(pid: i32) -> Option<u64> {
  let stat = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
  // El nombre del proceso va entre parentesis y puede tener espacios; los
  // campos se cuentan despues del ultimo ')'. Ahi empieza el campo 3.
  let after = stat.rsplit_once(')')?.1;
  after.split_whitespace().nth(19)?.parse().ok()
}

fn is_running(pid: i32) -> bool {
  // Un zombie ya termino aunque siga en /proc.
  match fs::read_to_string(format!("/proc/{pid}/stat")) {
    Ok(stat) => !matches!(stat.rsplit_once(')').map(|(_, r)| r.trim_start().as_bytes().first()), Some(Some(b'Z'))),
    Err(_) => false,
  }
}

fn send_signal(proc_: &Process, sig: i32, name: &str) {
  // Se vuelve a validar justo antes de enviar: el PID podria reutilizarse.
  if !proc_.still_same() {
    return;
  }
  eprintln!("ketaid: sending {name} to {}", proc_.pid);
  // SAFETY: kill no tiene precondiciones de memoria.
  unsafe { libc::kill(proc_.pid, sig) };
}

/// SIGTERM y, si no termina dentro del plazo, SIGKILL.
fn terminate(proc_: &Process) {
  send_signal(proc_, libc::SIGTERM, "SIGTERM");
  let deadline = Instant::now() + TERM_GRACE;
  while Instant::now() < deadline {
    if !is_running(proc_.pid) || !proc_.still_same() {
      return;
    }
    thread::sleep(Duration::from_millis(50));
  }
  send_signal(proc_, libc::SIGKILL, "SIGKILL");
}

/// Escaneo de huerfanos: lo ejecuta solo la instancia duena del bloqueo.
fn close_orphans() {
  let Ok(entries) = fs::read_dir("/proc") else { return };
  let orphans: Vec<Process> = entries
    .filter_map(Result::ok)
    .filter_map(|e| parse_pid(&e.file_name()))
    .filter_map(Process::validate)
    .filter(|p| is_running(p.pid))
    .collect();
  // Se terminan en paralelo para no esperar 5 s por cada uno.
  let handles: Vec<_> = orphans
    .into_iter()
    .map(|p| thread::spawn(move || terminate(&p)))
    .collect();
  for h in handles {
    let _ = h.join();
  }
}

fn parse_pid(name: &OsStr) -> Option<i32> {
  name.to_str()?.parse().ok()
}
