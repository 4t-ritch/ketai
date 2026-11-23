//! Conformidad del canal del visualizador: docs/protocol.md, seccion 7.

mod common;

use std::{
  io::{ErrorKind, Read},
  os::unix::{fs::PermissionsExt, net::UnixStream},
  path::PathBuf,
  thread,
  time::{Duration, Instant},
};

use common::{Daemon, fixture_uri};
use serde_json::json;

const BINS: usize = 64;
const FRAME: usize = BINS * 4;

fn viz_path(d: &Daemon) -> PathBuf {
  d.runtime_dir.join("ketai").join("viz.sock")
}

fn connect(d: &Daemon) -> UnixStream {
  let s = UnixStream::connect(viz_path(d)).expect("connect viz.sock");
  s.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
  s
}

fn header(s: &mut UnixStream) -> (Vec<u8>, u32, u32) {
  let mut h = [0u8; 12];
  s.read_exact(&mut h).unwrap();
  (h[..4].to_vec(), u32::from_le_bytes(h[4..8].try_into().unwrap()), u32::from_le_bytes(h[8..12].try_into().unwrap()))
}

fn frame(s: &mut UnixStream) -> Vec<f32> {
  let mut b = [0u8; FRAME];
  s.read_exact(&mut b).unwrap();
  b.chunks(4).map(|c| f32::from_le_bytes(c.try_into().unwrap())).collect()
}

fn play(d: &Daemon) {
  let mut c = d.connect();
  c.request(1, "queue.add", json!({"uris": [fixture_uri("vorbis.ogg")]}));
  c.request(2, "player.play", json!({}));
}

/// true si llega algun byte dentro del plazo.
fn receives_bytes(s: &mut UnixStream, within: Duration) -> bool {
  s.set_read_timeout(Some(within)).unwrap();
  let mut b = [0u8; 1];
  match s.read(&mut b) {
    Ok(n) => n > 0,
    Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => false,
    Err(e) => panic!("{e}"),
  }
}

#[test]
fn header_says_ktv1_with_bins_and_fps() {
  let d = Daemon::start();
  let mode = std::fs::metadata(viz_path(&d)).unwrap().permissions().mode() & 0o777;
  assert_eq!(mode, 0o600);
  let mut s = connect(&d);
  assert_eq!(header(&mut s), (b"KTV1".to_vec(), 64, 60));
}

#[test]
fn no_frames_unless_playing() {
  let d = Daemon::start();
  let mut s = connect(&d);
  header(&mut s);
  assert!(!receives_bytes(&mut s, Duration::from_millis(400)), "frames while stopped");

  play(&d);
  let mut c = d.connect();
  // Esperar a que suene y pausar.
  let deadline = Instant::now() + Duration::from_secs(3);
  while c.request(3, "player.state", json!({}))["result"]["position_ms"].as_u64().unwrap() < 300 {
    assert!(Instant::now() < deadline);
    thread::sleep(Duration::from_millis(20));
  }
  c.request(4, "player.pause", json!({}));
  // Vaciar lo que estaba en camino y comprobar que no llega mas.
  s.set_read_timeout(Some(Duration::from_millis(200))).unwrap();
  let mut sink = [0u8; 65536];
  while s.read(&mut sink).is_ok_and(|n| n > 0) {}
  assert!(!receives_bytes(&mut s, Duration::from_millis(400)), "frames while paused");
}

#[test]
fn frames_flow_while_playing_at_about_the_announced_rate() {
  let d = Daemon::start();
  let mut s = connect(&d);
  header(&mut s);
  play(&d);
  let first = frame(&mut s);
  assert_eq!(first.len(), BINS);

  let start = Instant::now();
  let mut count = 0;
  let mut any_signal = false;
  while start.elapsed() < Duration::from_millis(1000) {
    let f = frame(&mut s);
    assert!(f.iter().all(|v| (0.0..=1.0).contains(v)), "{f:?}");
    any_signal |= f.iter().any(|&v| v > 0.2);
    count += 1;
  }
  assert!((40..=75).contains(&count), "{count} frames in 1 s");
  assert!(any_signal, "playing audio must show up in the spectrum");
}

#[test]
fn a_client_that_never_reads_does_not_slow_anyone() {
  let d = Daemon::start();
  // Este cliente se conecta y nunca lee.
  let _stuck = connect(&d);
  let mut s = connect(&d);
  header(&mut s);
  play(&d);
  // Tiempo de sobra para que se llene el buffer del cliente trabado.
  thread::sleep(Duration::from_millis(1500));
  let start = Instant::now();
  for _ in 0..20 {
    frame(&mut s);
  }
  assert!(start.elapsed() < Duration::from_millis(800), "reader slowed down: {:?}", start.elapsed());
  // El canal de control sigue respondiendo enseguida.
  let mut c = d.connect();
  let t = Instant::now();
  c.request(9, "player.state", json!({}));
  assert!(t.elapsed() < Duration::from_millis(200));
}

fn open_fds(d: &Daemon) -> usize {
  std::fs::read_dir(format!("/proc/{}/fd", d.child.id())).unwrap().count()
}

fn threads(d: &Daemon) -> usize {
  std::fs::read_dir(format!("/proc/{}/task", d.child.id())).unwrap().count()
}

#[test]
fn clients_that_leave_while_nothing_plays_are_released() {
  // Un cliente que se reconecta en bucle con el daemon detenido no debe
  // dejar threads ni sockets abiertos.
  let d = Daemon::start();
  let mut first = connect(&d);
  header(&mut first);
  drop(first);
  thread::sleep(Duration::from_millis(1500));
  let (fds, tasks) = (open_fds(&d), threads(&d));
  for _ in 0..40 {
    let mut s = connect(&d);
    header(&mut s);
  }
  let deadline = Instant::now() + Duration::from_secs(4);
  loop {
    let (f, t) = (open_fds(&d), threads(&d));
    if f <= fds + 1 && t <= tasks + 1 {
      break;
    }
    assert!(Instant::now() < deadline, "leaked: fds {fds} -> {f}, threads {tasks} -> {t}");
    thread::sleep(Duration::from_millis(100));
  }
}
