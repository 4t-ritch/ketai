//! Cliente IPC contra un servidor falso: framing, emparejado por id y
//! eventos.

use std::{
  io::{BufRead, BufReader, Write},
  os::unix::net::UnixListener,
  sync::mpsc,
  thread,
  time::Duration,
};

use ketai_tui::client::{Client, DaemonMsg};
use serde_json::{Value, json};

fn temp_socket() -> std::path::PathBuf {
  let dir = std::env::temp_dir().join(format!("ketai-tui-test-{}-{}", std::process::id(), rand_suffix()));
  std::fs::create_dir_all(&dir).unwrap();
  dir.join("ctl.sock")
}

fn rand_suffix() -> u64 {
  std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().subsec_nanos() as u64
}

#[test]
fn requests_get_their_replies_and_events_arrive() {
  let path = temp_socket();
  let listener = UnixListener::bind(&path).unwrap();
  // Servidor falso: responde cada peticion con su metodo y despues manda
  // un evento.
  let server = thread::spawn(move || {
    let (stream, _) = listener.accept().unwrap();
    let mut out = stream.try_clone().unwrap();
    let mut lines = BufReader::new(stream).lines();
    let mut seen = Vec::new();
    for _ in 0..2 {
      let req: Value = serde_json::from_str(&lines.next().unwrap().unwrap()).unwrap();
      seen.push(req.clone());
      writeln!(out, "{}", json!({"id": req["id"], "result": {"echo": req["method"]}})).unwrap();
    }
    writeln!(out, "{}", json!({"event": "player.state", "data": {"status": "playing"}})).unwrap();
    writeln!(out, "{}", json!({"id": 99, "error": {"code": -32601, "message": "method not found"}})).unwrap();
    seen
  });

  let (tx, rx) = mpsc::channel();
  let mut client = Client::connect(&path, tx, |m| m).unwrap();
  let a = client.request("player.state", json!({})).unwrap();
  let b = client.request("queue.list", json!({})).unwrap();
  assert_ne!(a, b, "ids are unique per connection");

  let mut got = Vec::new();
  for _ in 0..4 {
    got.push(rx.recv_timeout(Duration::from_secs(3)).expect("message"));
  }
  assert!(matches!(&got[0], DaemonMsg::Reply { id, result: Ok(r) } if *id == a && r["echo"] == "player.state"));
  assert!(matches!(&got[1], DaemonMsg::Reply { id, result: Ok(r) } if *id == b && r["echo"] == "queue.list"));
  assert!(matches!(&got[2], DaemonMsg::Event { name, data } if name == "player.state" && data["status"] == "playing"));
  assert!(matches!(&got[3], DaemonMsg::Reply { id: 99, result: Err(e) } if e["code"] == -32601));

  let seen = server.join().unwrap();
  assert_eq!(seen[0]["params"], json!({}));
  // Al cerrar el servidor llega Closed.
  assert!(matches!(rx.recv_timeout(Duration::from_secs(3)), Ok(DaemonMsg::Closed)));
  let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn connecting_to_a_missing_socket_fails() {
  let path = temp_socket();
  let (tx, _rx) = mpsc::channel::<DaemonMsg>();
  assert!(Client::connect(&path, tx, |m| m).is_err());
  let _ = std::fs::remove_dir_all(path.parent().unwrap());
}
