//! Conformidad de instancia unica: docs/servicio.md, seccion "Instancia unica".

mod common;

use std::{
  fs,
  path::Path,
  process::{Command, Stdio},
  thread,
  time::{Duration, Instant},
};

use common::{Daemon, temp_runtime_dir};
use serde_json::json;

fn spawn_in(runtime_dir: &Path) -> std::process::Child {
  Command::new(env!("CARGO_BIN_EXE_ketaid"))
    .env("XDG_RUNTIME_DIR", runtime_dir)
    .stdout(Stdio::null())
    .stderr(Stdio::null())
    .spawn()
    .unwrap()
}

fn alive(pid: u32) -> bool {
  // Un proceso zombie ya termino aunque su entrada siga en /proc.
  match fs::read_to_string(format!("/proc/{pid}/stat")) {
    Ok(stat) => !stat.rsplit(')').next().unwrap_or("").trim_start().starts_with('Z'),
    Err(_) => false,
  }
}

fn wait_until(timeout: Duration, mut cond: impl FnMut() -> bool) -> bool {
  let deadline = Instant::now() + timeout;
  while Instant::now() < deadline {
    if cond() {
      return true;
    }
    thread::sleep(Duration::from_millis(50));
  }
  false
}

fn signal(pid: u32, sig: &str) {
  Command::new("kill").arg(format!("-{sig}")).arg(pid.to_string()).status().unwrap();
}

#[test]
fn second_instance_exits_when_owner_is_healthy() {
  let daemon = Daemon::start();
  let status = Command::new(env!("CARGO_BIN_EXE_ketaid"))
    .env("XDG_RUNTIME_DIR", &daemon.runtime_dir)
    .stderr(Stdio::null())
    .status()
    .unwrap();
  assert!(status.success(), "second instance must exit with code 0");

  // El dueno original sigue atendiendo.
  let mut client = daemon.connect();
  let reply = client.request(1, "daemon.hello", json!({"client": "t", "protocol": 1}));
  assert_eq!(reply["result"]["server"], "ketaid");
}

#[test]
fn lock_file_holds_owner_pid() {
  let daemon = Daemon::start();
  let lock = daemon.runtime_dir.join("ketai").join("ketaid.lock");
  let content = fs::read_to_string(lock).unwrap();
  assert_eq!(content.trim(), daemon.child.id().to_string());
}

#[test]
fn stale_socket_without_owner_does_not_block_start() {
  let runtime = temp_runtime_dir();
  let dir = runtime.join("ketai");
  fs::create_dir_all(&dir).unwrap();
  fs::write(dir.join("ctl.sock"), b"").unwrap();

  let mut child = spawn_in(&runtime);
  let ok = wait_until(Duration::from_secs(5), || {
    std::os::unix::net::UnixStream::connect(dir.join("ctl.sock")).is_ok()
  });
  let _ = child.kill();
  let _ = child.wait();
  let _ = fs::remove_dir_all(&runtime);
  assert!(ok, "daemon must replace a stale socket");
}

#[test]
fn hung_owner_is_replaced() {
  let mut hung = Daemon::start();
  let hung_pid = hung.child.id();
  // Un proceso detenido mantiene el bloqueo pero no responde.
  signal(hung_pid, "STOP");

  let mut new = spawn_in(&hung.runtime_dir);
  let replaced = wait_until(Duration::from_secs(15), || {
    let _ = hung.child.try_wait();
    !alive(hung_pid)
  });
  let serving = replaced
    && wait_until(Duration::from_secs(5), || {
      let mut c = match std::os::unix::net::UnixStream::connect(hung.socket_path()) {
        Ok(s) => common::Client { reader: std::io::BufReader::new(s.try_clone().unwrap()), writer: s },
        Err(_) => return false,
      };
      let reply = c.request(1, "daemon.hello", json!({"client": "t", "protocol": 1}));
      reply["result"]["server"] == "ketaid"
    });
  let _ = new.kill();
  let _ = new.wait();
  assert!(replaced, "hung owner must be terminated");
  assert!(serving, "new instance must take over");
}

#[test]
fn orphan_of_same_session_is_closed() {
  let mut orphan = Daemon::start();
  let orphan_pid = orphan.child.id();
  // Sin lock ni socket, el proceso viejo queda como huerfano de la sesion.
  let dir = orphan.runtime_dir.join("ketai");
  fs::remove_file(dir.join("ketaid.lock")).unwrap();
  fs::remove_file(dir.join("ctl.sock")).unwrap();

  let mut owner = spawn_in(&orphan.runtime_dir);
  let closed = wait_until(Duration::from_secs(10), || {
    let _ = orphan.child.try_wait();
    !alive(orphan_pid)
  });
  let owner_alive = alive(owner.id());
  let _ = owner.kill();
  let _ = owner.wait();
  assert!(closed, "orphan must be closed by the new owner");
  assert!(owner_alive, "the owner must keep running");
}

#[test]
fn daemons_of_other_sessions_are_not_touched() {
  let other = Daemon::start();
  let mine = Daemon::start();
  thread::sleep(Duration::from_millis(300));
  assert!(alive(other.child.id()));
  assert!(alive(mine.child.id()));
}
