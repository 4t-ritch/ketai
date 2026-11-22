//! Conformidad de persistencia de sesion: docs/servicio.md, seccion
//! "Persistencia de sesion".

mod common;

use std::{
  fs,
  path::Path,
  process::Command,
  thread,
  time::{Duration, Instant},
};

use common::{Client, Daemon, fixture_uri, temp_runtime_dir};
use serde_json::{Value, json};

struct Run {
  daemon: Daemon,
  client: Client,
  next_id: u64,
}

impl Run {
  fn start(dir: &Path, envs: &[(&str, &str)]) -> Self {
    let daemon = Daemon::start_in(dir.to_path_buf(), envs);
    let client = daemon.connect();
    Self { daemon, client, next_id: 1 }
  }

  fn call(&mut self, method: &str, params: Value) -> Value {
    self.next_id += 1;
    let reply = self.client.request(self.next_id, method, params);
    assert!(reply.get("error").is_none(), "{method}: {reply}");
    reply["result"].clone()
  }

  fn state(&mut self) -> Value {
    self.call("player.state", json!({}))
  }

  fn shutdown(mut self) {
    self.call("daemon.shutdown", json!({}));
    assert_eq!(self.daemon.wait_exit(Duration::from_secs(5)), Some(true));
  }

  fn kill(mut self) {
    let _ = self.daemon.child.kill();
    let _ = self.daemon.child.wait();
  }

  /// Espera a que la sesion restaurada tenga `n` pistas en la cola.
  fn wait_queue_len(&mut self, n: usize) -> Value {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
      let list = self.call("queue.list", json!({}));
      if list["tracks"].as_array().unwrap().len() == n {
        return list;
      }
      assert!(Instant::now() < deadline, "queue never reached {n}: {list}");
      thread::sleep(Duration::from_millis(50));
    }
  }
}

fn session_json(dir: &Path) -> Value {
  let text = fs::read_to_string(dir.join("state/ketai/session.json")).expect("session.json exists");
  serde_json::from_str(&text).expect("valid session json")
}

fn wait_session(dir: &Path, cond: impl Fn(&Value) -> bool) -> Value {
  let deadline = Instant::now() + Duration::from_secs(5);
  loop {
    if let Ok(text) = fs::read_to_string(dir.join("state/ketai/session.json"))
      && let Ok(v) = serde_json::from_str::<Value>(&text)
      && cond(&v)
    {
      return v;
    }
    assert!(Instant::now() < deadline, "session never matched");
    thread::sleep(Duration::from_millis(50));
  }
}

fn uris(names: &[&str]) -> Vec<String> {
  names.iter().map(|n| fixture_uri(n)).collect()
}

#[test]
fn shutdown_saves_and_restart_restores_everything_paused() {
  let dir = temp_runtime_dir();
  let mut a = Run::start(&dir, &[]);
  a.call("queue.add", json!({"uris": uris(&["vorbis.ogg", "opus.ogg", "aac_lc.m4a"])}));
  a.call("queue.jump", json!({"index": 1}));
  a.call("player.seek", json!({"position_ms": 1200}));
  a.call("player.set_volume", json!({"volume": 0.4}));
  a.call("player.set_muted", json!({"muted": true}));
  a.call("player.set_repeat", json!({"repeat": "queue"}));
  a.call("player.set_shuffle", json!({"shuffle": true}));
  a.shutdown();

  let saved = session_json(&dir);
  assert_eq!(saved["version"], 1);
  assert_eq!(saved["queue"], json!(uris(&["vorbis.ogg", "opus.ogg", "aac_lc.m4a"])));
  assert_eq!(saved["current"], 1);
  assert_eq!(saved["status"], "playing");
  assert_eq!(saved["output"], Value::Null);

  let mut b = Run::start(&dir, &[]);
  let list = b.wait_queue_len(3);
  assert_eq!(list["current"], 1);
  let state = b.state();
  assert_eq!(state["status"], "paused", "restored session never autoplays");
  assert_eq!(state["track"]["uri"], fixture_uri("opus.ogg"));
  let pos = state["position_ms"].as_u64().unwrap();
  assert!((1100..1700).contains(&pos), "restored position {pos}");
  assert_eq!(state["volume"], 0.4);
  assert_eq!(state["muted"], true);
  assert_eq!(state["repeat"], "queue");
  assert_eq!(state["shuffle"], true);

  // Retomar con play funciona desde la posicion restaurada.
  b.call("player.play", json!({}));
  assert_eq!(b.state()["status"], "playing");
  b.kill();
  let _ = fs::remove_dir_all(&dir);
}

#[test]
fn sigterm_saves_session_and_exits_cleanly() {
  let dir = temp_runtime_dir();
  let mut a = Run::start(&dir, &[]);
  a.call("queue.add", json!({"uris": uris(&["vorbis.ogg"])}));
  a.call("player.set_volume", json!({"volume": 0.7}));
  let pid = a.daemon.child.id();
  Command::new("kill").arg("-TERM").arg(pid.to_string()).status().unwrap();
  assert_eq!(a.daemon.wait_exit(Duration::from_secs(5)), Some(true), "SIGTERM must exit 0");
  let saved = session_json(&dir);
  assert_eq!(saved["volume"], 0.7);
  assert_eq!(saved["queue"], json!(uris(&["vorbis.ogg"])));
  assert!(!a.daemon.socket_path().exists(), "socket removed on SIGTERM");
  let _ = fs::remove_dir_all(&dir);
}

#[test]
fn sigint_also_saves_session() {
  let dir = temp_runtime_dir();
  let mut a = Run::start(&dir, &[]);
  a.call("player.set_volume", json!({"volume": 0.2}));
  let pid = a.daemon.child.id();
  Command::new("kill").arg("-INT").arg(pid.to_string()).status().unwrap();
  assert_eq!(a.daemon.wait_exit(Duration::from_secs(5)), Some(true));
  assert_eq!(session_json(&dir)["volume"], 0.2);
  let _ = fs::remove_dir_all(&dir);
}

#[test]
fn changes_are_saved_within_a_second_without_shutdown() {
  let dir = temp_runtime_dir();
  let mut a = Run::start(&dir, &[]);
  a.call("queue.add", json!({"uris": uris(&["vorbis.ogg", "opus.ogg"])}));
  a.call("player.set_volume", json!({"volume": 0.55}));
  let saved = wait_session(&dir, |v| v["volume"] == 0.55 && v["queue"].as_array().map(Vec::len) == Some(2));
  assert_eq!(saved["current"], Value::Null);
  // Sin apagado limpio (corte): la sesion igual se recupera.
  a.kill();
  let mut b = Run::start(&dir, &[]);
  b.wait_queue_len(2);
  assert_eq!(b.state()["volume"], 0.55);
  b.kill();
  let _ = fs::remove_dir_all(&dir);
}

#[test]
fn position_is_saved_periodically_while_playing() {
  let dir = temp_runtime_dir();
  // Intervalo corto solo para el test (por defecto 10 s).
  let mut a = Run::start(&dir, &[("KETAI_SESSION_SAVE_INTERVAL_MS", "300")]);
  a.call("queue.add", json!({"uris": uris(&["vorbis.ogg"])}));
  a.call("player.play", json!({}));
  wait_session(&dir, |v| v["position_ms"].as_u64().unwrap_or(0) >= 600);
  a.kill();
  let _ = fs::remove_dir_all(&dir);
}

#[test]
fn write_is_atomic_and_leaves_no_temp_file() {
  let dir = temp_runtime_dir();
  let mut a = Run::start(&dir, &[]);
  a.call("player.set_volume", json!({"volume": 0.9}));
  a.shutdown();
  let state_dir = dir.join("state/ketai");
  assert!(state_dir.join("session.json").exists());
  assert!(!state_dir.join("session.json.tmp").exists());
  let _ = fs::remove_dir_all(&dir);
}

#[test]
fn corrupt_session_is_moved_aside_and_daemon_starts_empty() {
  let dir = temp_runtime_dir();
  let state_dir = dir.join("state/ketai");
  fs::create_dir_all(&state_dir).unwrap();
  fs::write(state_dir.join("session.json"), b"{ this is not json").unwrap();

  let mut a = Run::start(&dir, &[]);
  let state = a.state();
  assert_eq!(state["status"], "stopped");
  assert!(a.call("queue.list", json!({}))["tracks"].as_array().unwrap().is_empty());
  assert_eq!(fs::read(state_dir.join("session.json.bad")).unwrap(), b"{ this is not json");
  a.kill();
  let _ = fs::remove_dir_all(&dir);
}

#[test]
fn missing_files_are_dropped_and_current_moves_to_next_at_zero() {
  let dir = temp_runtime_dir();
  let media = dir.join("media");
  fs::create_dir_all(&media).unwrap();
  let copy = |name: &str| {
    let target = media.join(name);
    fs::copy(fixture_uri(name).trim_start_matches("file://"), &target).unwrap();
    format!("file://{}", target.display())
  };
  let (a_uri, b_uri, c_uri) = (copy("vorbis.ogg"), copy("opus.ogg"), copy("aac_lc.m4a"));

  let mut a = Run::start(&dir, &[]);
  a.call("queue.add", json!({"uris": [a_uri, b_uri, c_uri]}));
  a.call("queue.jump", json!({"index": 1}));
  a.call("player.seek", json!({"position_ms": 1000}));
  a.shutdown();

  // Desaparece la pista actual.
  fs::remove_file(media.join("opus.ogg")).unwrap();

  let mut b = Run::start(&dir, &[]);
  let list = b.wait_queue_len(2);
  let got: Vec<&str> = list["tracks"].as_array().unwrap().iter().map(|t| t["uri"].as_str().unwrap()).collect();
  assert_eq!(got, [a_uri.as_str(), c_uri.as_str()]);
  assert_eq!(list["current"], 1, "current moves to the next available track");
  let state = b.state();
  assert_eq!(state["position_ms"], 0);
  assert_eq!(state["status"], "paused");
  b.kill();
  let _ = fs::remove_dir_all(&dir);
}

#[test]
fn stopped_session_restores_stopped() {
  let dir = temp_runtime_dir();
  let mut a = Run::start(&dir, &[]);
  a.call("queue.add", json!({"uris": uris(&["vorbis.ogg"])}));
  a.call("player.play", json!({}));
  a.call("player.stop", json!({}));
  a.shutdown();
  let mut b = Run::start(&dir, &[]);
  b.wait_queue_len(1);
  let state = b.state();
  assert_eq!(state["status"], "stopped");
  assert_eq!(state["track"]["uri"], fixture_uri("vorbis.ogg"));
  b.kill();
  let _ = fs::remove_dir_all(&dir);
}

#[test]
fn requests_during_restore_see_the_restored_queue() {
  let dir = temp_runtime_dir();
  let mut a = Run::start(&dir, &[]);
  let many: Vec<String> = (0..40).map(|_| fixture_uri("vorbis.ogg")).collect();
  a.call("queue.add", json!({"uris": many}));
  a.shutdown();

  // Lo primero que se pide al arrancar ya debe ver la cola restaurada.
  let mut b = Run::start(&dir, &[]);
  let list = b.call("queue.list", json!({}));
  assert_eq!(list["tracks"].as_array().unwrap().len(), 40);
  b.kill();
  let _ = fs::remove_dir_all(&dir);
}

#[test]
fn default_location_is_under_home_local_state() {
  let dir = temp_runtime_dir();
  let home = dir.join("home");
  fs::create_dir_all(&home).unwrap();
  let mut child = Command::new(env!("CARGO_BIN_EXE_ketaid"))
    .env("XDG_RUNTIME_DIR", &dir)
    .env_remove("XDG_STATE_HOME")
    .env("HOME", &home)
    .env("KETAI_AUDIO_BACKEND", "null")
    .stderr(std::process::Stdio::null())
    .spawn()
    .unwrap();
  let sock = dir.join("ketai/ctl.sock");
  let deadline = Instant::now() + Duration::from_secs(5);
  while std::os::unix::net::UnixStream::connect(&sock).is_err() {
    assert!(Instant::now() < deadline);
    thread::sleep(Duration::from_millis(20));
  }
  let mut c = Client::connect(&sock);
  c.request(1, "player.set_volume", json!({"volume": 0.3}));
  c.request(2, "daemon.shutdown", json!({}));
  let _ = child.wait();
  assert!(home.join(".local/state/ketai/session.json").exists());
  let _ = fs::remove_dir_all(&dir);
}

#[test]
fn seek_while_paused_survives_a_crash() {
  let dir = temp_runtime_dir();
  let mut a = Run::start(&dir, &[]);
  a.call("queue.add", json!({"uris": uris(&["vorbis.ogg"])}));
  a.call("player.play", json!({}));
  a.call("player.pause", json!({}));
  // Primero se guarda el cambio a pausa; el seek llega despues.
  wait_session(&dir, |v| v["status"] == "paused");
  thread::sleep(Duration::from_millis(1100));
  a.call("player.seek", json!({"position_ms": 1500}));
  wait_session(&dir, |v| v["position_ms"].as_u64().is_some_and(|p| (1400..1700).contains(&p)));
  a.kill();
  let mut b = Run::start(&dir, &[]);
  b.wait_queue_len(1);
  let pos = b.state()["position_ms"].as_u64().unwrap();
  assert!((1400..1700).contains(&pos), "restored position {pos}");
  b.kill();
  let _ = fs::remove_dir_all(&dir);
}

#[test]
fn playing_a_restored_session_continues_from_the_saved_position() {
  let dir = temp_runtime_dir();
  let mut a = Run::start(&dir, &[]);
  a.call("queue.add", json!({"uris": uris(&["vorbis.ogg"])}));
  a.call("player.play", json!({}));
  a.call("player.pause", json!({}));
  a.call("player.seek", json!({"position_ms": 1200}));
  a.shutdown();

  let mut b = Run::start(&dir, &[]);
  b.wait_queue_len(1);
  b.call("player.play", json!({}));
  thread::sleep(Duration::from_millis(500));
  let pos = b.state()["position_ms"].as_u64().unwrap();
  // Sigue desde 1200: nunca vuelve a contar desde 0.
  assert!((1400..2000).contains(&pos), "position after restore and play: {pos}");
  b.shutdown();
  let _ = fs::remove_dir_all(&dir);
}
