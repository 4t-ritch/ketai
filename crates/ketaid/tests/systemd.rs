//! Integracion con systemd: sockets heredados (`LISTEN_FDS`) y aviso
//! `READY=1` por `NOTIFY_SOCKET`.
//!
//! systemd se simula: el test crea los sockets, los pasa como fd 3 y 4, y
//! `sh` fija `LISTEN_PID` con su propio pid antes de hacer `exec` del daemon.

mod common;

use std::{
  fs,
  os::{
    fd::AsRawFd,
    unix::{
      net::{UnixDatagram, UnixListener},
      process::CommandExt,
    },
  },
  path::{Path, PathBuf},
  process::{Child, Command, Stdio},
  thread,
  time::{Duration, Instant},
};

use common::{Client, temp_runtime_dir};
use serde_json::json;

struct Activated {
  child: Child,
  dir: PathBuf,
}

impl Drop for Activated {
  fn drop(&mut self) {
    let _ = self.child.kill();
    let _ = self.child.wait();
    let _ = fs::remove_dir_all(&self.dir);
  }
}

fn base_command(dir: &Path) -> Command {
  let mut cmd = Command::new("sh");
  cmd
    .env("XDG_RUNTIME_DIR", dir)
    .env("XDG_STATE_HOME", dir.join("state"))
    .env("XDG_DATA_HOME", dir.join("data"))
    .env("KETAI_AUDIO_BACKEND", "null")
    .stdout(Stdio::null())
    .stderr(Stdio::null());
  cmd
}

/// Lanza el daemon como lo haria systemd, con `ctl` y `viz` como fd 3 y 4.
/// `listen_pid` decide que pid se anuncia: `$$` (el propio) o uno ajeno.
fn activate(dir: &Path, ctl: &UnixListener, viz: &UnixListener, listen_pid: &str, notify: Option<&Path>) -> Child {
  let (ctl_fd, viz_fd) = (ctl.as_raw_fd(), viz.as_raw_fd());
  let mut cmd = base_command(dir);
  cmd.arg("-c").arg(format!("LISTEN_PID={listen_pid} LISTEN_FDS=2 exec \"$0\"")).arg(env!("CARGO_BIN_EXE_ketaid"));
  if let Some(n) = notify {
    cmd.env("NOTIFY_SOCKET", n);
  }
  // SAFETY: entre fork y exec solo se llama a dup2, que es seguro ahi.
  unsafe {
    cmd.pre_exec(move || {
      if libc::dup2(ctl_fd, 3) < 0 || libc::dup2(viz_fd, 4) < 0 {
        return Err(std::io::Error::last_os_error());
      }
      Ok(())
    });
  }
  cmd.spawn().expect("spawn ketaid")
}

fn sockets(dir: &Path) -> (PathBuf, UnixListener, UnixListener) {
  let ketai = dir.join("ketai");
  fs::create_dir_all(&ketai).unwrap();
  let ctl_path = ketai.join("ctl.sock");
  let ctl = UnixListener::bind(&ctl_path).unwrap();
  let viz = UnixListener::bind(ketai.join("viz.sock")).unwrap();
  (ctl_path, ctl, viz)
}

fn hello(path: &Path) -> serde_json::Value {
  let mut c = Client::connect(path);
  c.request(1, "daemon.hello", json!({"protocol": 1, "client": "test"}))
}

fn wait_exit(child: &mut Child, timeout: Duration) -> bool {
  let deadline = Instant::now() + timeout;
  while Instant::now() < deadline {
    if child.try_wait().unwrap().is_some() {
      return true;
    }
    thread::sleep(Duration::from_millis(20));
  }
  false
}

#[test]
fn serves_on_the_inherited_ctl_socket() {
  let dir = temp_runtime_dir();
  let (ctl_path, ctl, viz) = sockets(&dir);
  let child = activate(&dir, &ctl, &viz, "$$", None);
  let mut d = Activated { child, dir };
  // El test cierra su copia: el unico que puede aceptar es el daemon.
  drop((ctl, viz));

  let r = hello(&ctl_path);
  assert!(r.get("result").is_some(), "{r}");

  // Al salir no borra el socket: es de systemd, que lo reutiliza.
  let _ = Command::new("kill").arg("-TERM").arg(d.child.id().to_string()).status();
  assert!(wait_exit(&mut d.child, Duration::from_secs(5)));
  assert!(ctl_path.exists(), "inherited socket must survive the daemon");
}

#[test]
fn ignores_sockets_meant_for_another_process() {
  let dir = temp_runtime_dir();
  let ketai = dir.join("ketai");
  fs::create_dir_all(&ketai).unwrap();
  // Sockets sueltos en otra ruta: si el daemon los usara, ctl.sock no
  // existiria.
  let ctl = UnixListener::bind(dir.join("other-ctl.sock")).unwrap();
  let viz = UnixListener::bind(dir.join("other-viz.sock")).unwrap();
  let child = activate(&dir, &ctl, &viz, "1", None);
  let _d = Activated { child, dir: dir.clone() };

  let ctl_path = ketai.join("ctl.sock");
  let deadline = Instant::now() + Duration::from_secs(5);
  while !ctl_path.exists() {
    assert!(Instant::now() < deadline, "daemon did not create its own socket");
    thread::sleep(Duration::from_millis(20));
  }
  let r = hello(&ctl_path);
  assert!(r.get("result").is_some(), "{r}");
}

#[test]
fn notifies_ready_once_it_accepts_connections() {
  let dir = temp_runtime_dir();
  let notify_path = dir.join("notify.sock");
  let notify = UnixDatagram::bind(&notify_path).unwrap();
  notify.set_read_timeout(Some(Duration::from_secs(5))).unwrap();

  let mut cmd = base_command(&dir);
  cmd.arg("-c").arg("exec \"$0\"").arg(env!("CARGO_BIN_EXE_ketaid")).env("NOTIFY_SOCKET", &notify_path);
  let child = cmd.spawn().unwrap();
  let _d = Activated { child, dir: dir.clone() };

  let mut buf = [0u8; 256];
  let n = notify.recv(&mut buf).expect("READY=1 notification");
  let msg = String::from_utf8_lossy(&buf[..n]).to_string();
  assert!(msg.lines().any(|l| l == "READY=1"), "{msg}");
  // Cuando llega READY=1 el socket ya atiende.
  let r = hello(&dir.join("ketai").join("ctl.sock"));
  assert!(r.get("result").is_some(), "{r}");
}
