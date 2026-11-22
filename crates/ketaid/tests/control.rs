//! Conformidad del canal de control: docs/protocol.md secciones 1, 2, 3 y 8.

mod common;

use std::{io::Write, os::unix::fs::PermissionsExt, process::Command, thread, time::Duration};

use common::Daemon;
use serde_json::{Value, json};

fn error_code(reply: &Value) -> i64 {
  reply["error"]["code"].as_i64().expect("error.code")
}

#[test]
fn hello_returns_server_info() {
  let daemon = Daemon::start();
  let mut client = daemon.connect();
  let reply = client.request(1, "daemon.hello", json!({"client": "test", "protocol": 1}));
  assert_eq!(reply["result"]["server"], "ketaid");
  assert_eq!(reply["result"]["protocol"], 1);
  assert!(reply["result"]["version"].is_string());
  assert!(reply.get("error").is_none());
}

#[test]
fn hello_with_other_protocol_is_rejected() {
  let daemon = Daemon::start();
  let mut client = daemon.connect();
  let reply = client.request(1, "daemon.hello", json!({"client": "test", "protocol": 99}));
  assert_eq!(error_code(&reply), 1000);
}

#[test]
fn hello_without_protocol_is_invalid_params() {
  let daemon = Daemon::start();
  let mut client = daemon.connect();
  let reply = client.request(1, "daemon.hello", json!({"client": "test"}));
  assert_eq!(error_code(&reply), -32602);
}

#[test]
fn unknown_method_is_reported() {
  let daemon = Daemon::start();
  let mut client = daemon.connect();
  let reply = client.request(7, "nope.nothing", json!({}));
  assert_eq!(reply["id"], 7);
  assert_eq!(error_code(&reply), -32601);
}

#[test]
fn params_can_be_omitted() {
  let daemon = Daemon::start();
  let mut client = daemon.connect();
  client.send_raw(b"{\"id\": 3, \"method\": \"nope.nothing\"}\n");
  let reply = client.read_message().unwrap();
  assert_eq!(reply["id"], 3);
  assert_eq!(error_code(&reply), -32601);
}

#[test]
fn invalid_json_is_parse_error_and_connection_survives() {
  let daemon = Daemon::start();
  let mut client = daemon.connect();
  client.send_raw(b"{not json\n");
  let reply = client.read_message().unwrap();
  assert_eq!(reply["id"], Value::Null);
  assert_eq!(error_code(&reply), -32700);

  let reply = client.request(2, "daemon.hello", json!({"client": "t", "protocol": 1}));
  assert_eq!(reply["result"]["server"], "ketaid");
}

#[test]
fn request_without_method_is_invalid_request() {
  let daemon = Daemon::start();
  let mut client = daemon.connect();
  client.send_raw(b"{\"id\": 5}\n");
  let reply = client.read_message().unwrap();
  assert_eq!(reply["id"], 5);
  assert_eq!(error_code(&reply), -32600);
}

#[test]
fn request_with_invalid_id_is_invalid_request() {
  let daemon = Daemon::start();
  let mut client = daemon.connect();
  client.send_raw(b"{\"id\": -1, \"method\": \"daemon.hello\"}\n");
  let reply = client.read_message().unwrap();
  assert_eq!(reply["id"], Value::Null);
  assert_eq!(error_code(&reply), -32600);
}

#[test]
fn line_longer_than_1_mib_closes_connection() {
  let daemon = Daemon::start();
  let mut client = daemon.connect();
  let mut big = vec![b'a'; 1024 * 1024 + 1];
  big.push(b'\n');
  client.send_raw(&big);
  let reply = client.read_message().unwrap();
  assert_eq!(error_code(&reply), -32600);
  assert!(client.read_message().is_none(), "connection must be closed");

  // El daemon sigue atendiendo a otros clientes.
  let mut other = daemon.connect();
  let reply = other.request(1, "daemon.hello", json!({"client": "t", "protocol": 1}));
  assert_eq!(reply["result"]["server"], "ketaid");
}

#[test]
fn several_clients_are_served_at_once() {
  let daemon = Daemon::start();
  let mut a = daemon.connect();
  let mut b = daemon.connect();
  let ra = a.request(1, "daemon.hello", json!({"client": "a", "protocol": 1}));
  let rb = b.request(1, "daemon.hello", json!({"client": "b", "protocol": 1}));
  assert_eq!(ra["result"]["server"], "ketaid");
  assert_eq!(rb["result"]["server"], "ketaid");
}

#[test]
fn socket_has_owner_only_permissions() {
  let daemon = Daemon::start();
  let mode = std::fs::metadata(daemon.socket_path()).unwrap().permissions().mode();
  assert_eq!(mode & 0o777, 0o600);
  let dir_mode = std::fs::metadata(daemon.socket_path().parent().unwrap())
    .unwrap()
    .permissions()
    .mode();
  assert_eq!(dir_mode & 0o777, 0o700);
}

#[test]
fn shutdown_replies_exits_and_removes_socket() {
  let mut daemon = Daemon::start();
  let mut client = daemon.connect();
  let reply = client.request(1, "daemon.shutdown", json!({}));
  assert_eq!(reply["result"], json!({}));
  assert_eq!(daemon.wait_exit(Duration::from_secs(5)), Some(true));
  assert!(!daemon.socket_path().exists());
}

#[test]
fn refuses_to_start_without_xdg_runtime_dir() {
  let status = Command::new(env!("CARGO_BIN_EXE_ketaid"))
    .env("KETAI_AUDIO_BACKEND", "null")
    .env("XDG_STATE_HOME", std::env::temp_dir().join("ketaid-test-state-never-used"))
    .env_remove("XDG_RUNTIME_DIR")
    .stderr(std::process::Stdio::null())
    .status()
    .unwrap();
  assert!(!status.success());
}

#[test]
fn a_client_that_never_reads_is_slowed_down_not_buffered() {
  // Un cliente que manda peticiones sin leer las respuestas no debe hacer
  // crecer la memoria del daemon: llegado el tope, el daemon deja de leerlo
  // y las escrituras del cliente se bloquean.
  let daemon = Daemon::start();
  let mut client = daemon.connect();
  client.writer.set_nonblocking(true).unwrap();
  let line = b"{\"id\":1,\"method\":\"daemon.hello\",\"params\":{\"client\":\"t\",\"protocol\":1}}\n";
  let total = 200_000;
  let mut sent = 0;
  let mut stalled = 0;
  while sent < total {
    match client.writer.write(line) {
      Ok(n) if n == line.len() => {
        sent += 1;
        stalled = 0;
      }
      Ok(_) => panic!("partial write"),
      Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
        stalled += 1;
        if stalled > 50 {
          break;
        }
        thread::sleep(Duration::from_millis(10));
      }
      Err(e) => panic!("{e}"),
    }
  }
  assert!(sent < total, "daemon kept reading {sent} requests without replies being read");

  // Al leer, todo lo aceptado se responde en orden.
  client.writer.set_nonblocking(false).unwrap();
  for _ in 0..sent {
    let m = client.read_message().expect("reply");
    assert_eq!(m["result"]["protocol"], 1);
  }
}
