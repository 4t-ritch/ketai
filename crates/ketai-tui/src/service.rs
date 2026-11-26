//! Busqueda y arranque del servicio (docs/service.md, seccion
//! "Clients").
//!
//! `diagnose` es puro: decide el estado a partir de lo observado. `observe`
//! y `execute` hablan con el sistema (systemd de usuario por D-Bus,
//! `/proc`) y corren fuera del thread de la UI porque pueden tardar.

use std::{
  path::{Path, PathBuf},
  process::{Command, Stdio},
  time::{Duration, Instant},
};

use dbus::blocking::{Connection, stdintf::org_freedesktop_dbus::Properties};

const SYSTEMD: &str = "org.freedesktop.systemd1";
const MANAGER: &str = "org.freedesktop.systemd1.Manager";
const UNIT: &str = "org.freedesktop.systemd1.Unit";
const TIMEOUT: Duration = Duration::from_secs(3);

const SOCKET_UNIT: &str = include_str!("../systemd/ketaid.socket");
const SERVICE_UNIT: &str = include_str!("../systemd/ketaid.service");

/// Estado de las unidades de ketai en systemd.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnitInfo {
  pub socket_active: bool,
  pub service_active: bool,
  /// `ketaid.socket` arranca con la sesion.
  pub enabled: bool,
}

/// Lo que se encontro al buscar el servicio.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Observed {
  /// `ctl.sock` responde a `daemon.hello`.
  pub socket_ok: bool,
  /// `None`: no hay systemd de usuario. `Some(None)`: hay systemd pero no
  /// estan las unidades de ketai.
  pub systemd: Option<Option<UnitInfo>>,
  /// Un proceso `ketaid` del usuario.
  pub process: Option<i32>,
  pub binary: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
  Connected,
  Starting,
  Stopped { enabled: bool },
  NotInstalled,
  Unresponsive { pid: i32 },
  NoSystemd,
}

/// Lo que el usuario puede pedir desde la pantalla de servicio.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
  Start { enable: bool },
  Install,
  Restart { pid: i32 },
  StartManual,
  Retry,
}

pub fn diagnose(o: &Observed) -> State {
  if o.socket_ok {
    return State::Connected;
  }
  match (o.systemd, o.process) {
    // Unidad activa: el daemon esta arrancando (o se activa al conectar),
    // aunque su proceso ya exista.
    (Some(Some(u)), _) if u.socket_active || u.service_active => State::Starting,
    // Hay un proceso pero no responde por el socket.
    (_, Some(pid)) => State::Unresponsive { pid },
    (Some(Some(u)), None) => State::Stopped { enabled: u.enabled },
    (Some(None), None) => State::NotInstalled,
    (None, None) => State::NoSystemd,
  }
}

/// Busca el servicio: socket, unidades de systemd, proceso y binario.
pub fn observe(socket_ok: bool) -> Observed {
  Observed { socket_ok, systemd: query_units(), process: find_process(), binary: find_binary() }
}

fn session() -> Option<Connection> {
  Connection::new_session().ok()
}

/// Estado de una unidad: (cargada, activa, habilitada). `None` si systemd
/// no contesta.
fn unit_state(conn: &Connection, name: &str) -> Option<(bool, bool, bool)> {
  let manager = conn.with_proxy(SYSTEMD, "/org/freedesktop/systemd1", TIMEOUT);
  let (path,): (dbus::Path<'static>,) = manager.method_call(MANAGER, "LoadUnit", (name,)).ok()?;
  let unit = conn.with_proxy(SYSTEMD, path, TIMEOUT);
  let load: String = unit.get(UNIT, "LoadState").ok()?;
  let active: String = unit.get(UNIT, "ActiveState").ok()?;
  let file: String = unit.get(UNIT, "UnitFileState").unwrap_or_default();
  Some((load != "not-found", active == "active" || active == "activating", file == "enabled"))
}

fn query_units() -> Option<Option<UnitInfo>> {
  let conn = session()?;
  let (socket_loaded, socket_active, enabled) = unit_state(&conn, "ketaid.socket")?;
  let (service_loaded, service_active, _) = unit_state(&conn, "ketaid.service")?;
  if !socket_loaded && !service_loaded {
    return Some(None);
  }
  Some(Some(UnitInfo { socket_active, service_active, enabled }))
}

/// Proceso `ketaid` del mismo usuario.
fn find_process() -> Option<i32> {
  // SAFETY: getuid no tiene precondiciones.
  let uid = unsafe { libc::getuid() };
  for entry in std::fs::read_dir("/proc").ok()?.flatten() {
    let Some(pid) = entry.file_name().to_str().and_then(|n| n.parse::<i32>().ok()) else { continue };
    let path = entry.path();
    let Ok(exe) = std::fs::read_link(path.join("exe")) else { continue };
    if exe.file_name().is_some_and(|n| n == "ketaid") && owner(&path) == Some(uid) {
      return Some(pid);
    }
  }
  None
}

fn owner(path: &Path) -> Option<u32> {
  use std::os::unix::fs::MetadataExt;
  std::fs::metadata(path).ok().map(|m| m.uid())
}

/// `~/.local/bin/ketaid` o el primero del PATH (por ejemplo `~/.cargo/bin`).
fn find_binary() -> Option<PathBuf> {
  let home = std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/bin/ketaid"));
  let in_path = std::env::var_os("PATH")
    .map(|p| std::env::split_paths(&p).map(|d| d.join("ketaid")).collect::<Vec<_>>())
    .unwrap_or_default();
  home.into_iter().chain(in_path).find(|p| p.is_file())
}

/// Ejecuta una accion. El error se muestra tal cual en la pantalla.
pub fn execute(action: Action, observed: &Observed) -> Result<(), String> {
  match action {
    Action::Retry => Ok(()),
    Action::Start { enable } => start(enable),
    Action::Install => {
      install(observed.binary.as_deref().ok_or("no se encontró el binario ketaid")?)?;
      start(true)
    }
    Action::Restart { pid } => restart(pid, observed),
    Action::StartManual => {
      let binary = observed.binary.as_ref().ok_or("no se encontró el binario ketaid")?;
      // setsid -f: el daemon queda desacoplado de esta terminal y no es
      // hijo de la TUI. Lo protege su bloqueo de instancia unica.
      let status = Command::new("setsid")
        .arg("-f")
        .arg(binary)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|e| format!("no se pudo lanzar ketaid: {e}"))?;
      if status.success() { Ok(()) } else { Err(format!("setsid terminó con {status}")) }
    }
  }
}

fn manager_call<A: dbus::arg::AppendAll, R: dbus::arg::ReadAll>(method: &str, args: A) -> Result<R, String> {
  let conn = session().ok_or("no hay bus de sesión de D-Bus")?;
  let manager = conn.with_proxy(SYSTEMD, "/org/freedesktop/systemd1", TIMEOUT);
  manager.method_call(MANAGER, method, args).map_err(|e| format!("systemd: {e}"))
}

fn start(enable: bool) -> Result<(), String> {
  if enable {
    let _: (bool, Vec<(String, String, String)>) = manager_call("EnableUnitFiles", (vec!["ketaid.socket"], false, true))?;
    let _: () = manager_call("Reload", ())?;
  }
  let _: (dbus::Path<'static>,) = manager_call("StartUnit", ("ketaid.socket", "replace"))?;
  Ok(())
}

/// Unidad del servicio que ejecuta `binary` (por ejemplo `~/.cargo/bin/ketaid`
/// tras un `cargo install`) en lugar de la ruta fija de `systemd/`.
pub fn service_unit(binary: &Path) -> String {
  let path = binary.display().to_string();
  let exec = if path.contains(char::is_whitespace) { format!("\"{path}\"") } else { path };
  SERVICE_UNIT
    .lines()
    .map(|l| if l.starts_with("ExecStart=") { format!("ExecStart={exec}") } else { l.to_string() })
    .collect::<Vec<_>>()
    .join("\n")
    + "\n"
}

/// Copia las unidades a `~/.config/systemd/user/`, con la del servicio
/// apuntando a `binary`, y recarga systemd.
fn install(binary: &Path) -> Result<(), String> {
  let home = std::env::var_os("HOME").map(PathBuf::from).ok_or("HOME no está definido")?;
  let config = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).filter(|p| p.is_absolute()).unwrap_or(home.join(".config"));
  let dir = config.join("systemd/user");
  std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
  for (name, content) in [("ketaid.socket", SOCKET_UNIT.to_string()), ("ketaid.service", service_unit(binary))] {
    std::fs::write(dir.join(name), content).map_err(|e| format!("{name}: {e}"))?;
  }
  let _: () = manager_call("Reload", ())?;
  Ok(())
}

fn restart(pid: i32, observed: &Observed) -> Result<(), String> {
  let under_systemd = matches!(observed.systemd, Some(Some(u)) if u.service_active);
  if under_systemd {
    let _: (dbus::Path<'static>,) = manager_call("RestartUnit", ("ketaid.service", "replace"))?;
    return Ok(());
  }
  // Fuera de systemd: SIGTERM, y SIGKILL si no termina en 5 segundos.
  // SAFETY: kill con un pid que encontramos como proceso ketaid propio.
  unsafe { libc::kill(pid, libc::SIGTERM) };
  let deadline = Instant::now() + Duration::from_secs(5);
  while Path::new(&format!("/proc/{pid}")).exists() && Instant::now() < deadline {
    std::thread::sleep(Duration::from_millis(100));
  }
  if Path::new(&format!("/proc/{pid}")).exists() {
    // SAFETY: idem.
    unsafe { libc::kill(pid, libc::SIGKILL) };
  }
  match observed.systemd {
    Some(Some(_)) => start(false),
    _ => execute(Action::StartManual, observed),
  }
}
