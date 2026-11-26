//! Conformidad de dispositivos de salida: `output.list`, `output.set` y el
//! evento `output.changed`.
//!
//! Con el backend nulo, los dispositivos salen de `KETAI_FAKE_OUTPUTS`
//! (`id=Nombre;id2=Nombre2`); el primero es el default del sistema.

mod common;

use std::{
  fs,
  thread,
  time::{Duration, Instant},
};

use common::{Client, Daemon, temp_runtime_dir};
use serde_json::{Value, json};

const FAKE: &str = "alsa_output.speakers=Speakers;bluez_output.headset=Headset";

fn start(dir: &std::path::Path) -> (Daemon, Client) {
  let daemon = Daemon::start_in(dir.to_path_buf(), &[("KETAI_FAKE_OUTPUTS", FAKE)]);
  let client = daemon.connect();
  (daemon, client)
}

fn call(c: &mut Client, id: u64, method: &str, params: Value) -> Value {
  c.request(id, method, params)
}

/// Espera a que el daemon conozca los dispositivos.
fn wait_devices(c: &mut Client, n: usize) -> Value {
  let deadline = Instant::now() + Duration::from_secs(3);
  loop {
    let r = call(c, 1, "output.list", json!({}));
    if r["result"]["devices"].as_array().map(Vec::len) == Some(n) {
      return r["result"].clone();
    }
    assert!(Instant::now() < deadline, "devices never listed: {r}");
    thread::sleep(Duration::from_millis(30));
  }
}

#[test]
fn list_returns_devices_with_default_flag() {
  let dir = temp_runtime_dir();
  let (_d, mut c) = start(&dir);
  let list = wait_devices(&mut c, 2);
  assert_eq!(
    list["devices"],
    json!([
      {"id": "alsa_output.speakers", "name": "Speakers", "is_default": true},
      {"id": "bluez_output.headset", "name": "Headset", "is_default": false},
    ])
  );
  let _ = fs::remove_dir_all(&dir);
}

#[test]
fn set_selects_device_and_null_follows_default() {
  let dir = temp_runtime_dir();
  let (_d, mut c) = start(&dir);
  wait_devices(&mut c, 2);
  assert_eq!(call(&mut c, 2, "output.set", json!({"id": "bluez_output.headset"}))["result"], json!({}));
  assert_eq!(call(&mut c, 3, "player.state", json!({}))["result"]["output"], "bluez_output.headset");
  assert_eq!(call(&mut c, 4, "output.set", json!({"id": null}))["result"], json!({}));
  assert_eq!(call(&mut c, 5, "player.state", json!({}))["result"]["output"], Value::Null);
  let _ = fs::remove_dir_all(&dir);
}

#[test]
fn set_validates_device() {
  let dir = temp_runtime_dir();
  let (_d, mut c) = start(&dir);
  wait_devices(&mut c, 2);
  assert_eq!(call(&mut c, 2, "output.set", json!({"id": "nope"}))["error"]["code"], 1004);
  assert_eq!(call(&mut c, 3, "output.set", json!({}))["error"]["code"], -32602);
  assert_eq!(call(&mut c, 4, "output.set", json!({"id": 5}))["error"]["code"], -32602);
  assert_eq!(call(&mut c, 5, "player.state", json!({}))["result"]["output"], Value::Null);
  let _ = fs::remove_dir_all(&dir);
}

#[test]
fn output_changed_is_sent_to_subscribers() {
  let dir = temp_runtime_dir();
  let (d, mut c) = start(&dir);
  wait_devices(&mut c, 2);
  let mut w = d.connect();
  w.request(1, "events.subscribe", json!({"topics": ["output"]}));
  call(&mut c, 2, "output.set", json!({"id": "bluez_output.headset"}));
  let deadline = Instant::now() + Duration::from_secs(2);
  loop {
    let m = w.read_message().expect("event");
    if m["event"] == "output.changed" {
      assert_eq!(m["data"]["active"], "bluez_output.headset");
      assert_eq!(m["data"]["devices"].as_array().unwrap().len(), 2);
      break;
    }
    assert!(Instant::now() < deadline);
  }
  let _ = fs::remove_dir_all(&dir);
}

#[test]
fn selected_output_is_saved_and_restored() {
  let dir = temp_runtime_dir();
  {
    let (mut d, mut c) = start(&dir);
    wait_devices(&mut c, 2);
    call(&mut c, 2, "output.set", json!({"id": "bluez_output.headset"}));
    call(&mut c, 3, "daemon.shutdown", json!({}));
    d.wait_exit(Duration::from_secs(5));
  }
  let saved: Value = serde_json::from_str(&fs::read_to_string(dir.join("state/ketai/session.json")).unwrap()).unwrap();
  assert_eq!(saved["output"], "bluez_output.headset");

  let (_d, mut c) = start(&dir);
  wait_devices(&mut c, 2);
  assert_eq!(call(&mut c, 2, "player.state", json!({}))["result"]["output"], "bluez_output.headset");
  let _ = fs::remove_dir_all(&dir);
}

#[test]
fn restored_output_that_no_longer_exists_falls_back_to_default() {
  let dir = temp_runtime_dir();
  {
    let (mut d, mut c) = start(&dir);
    wait_devices(&mut c, 2);
    call(&mut c, 2, "output.set", json!({"id": "bluez_output.headset"}));
    call(&mut c, 3, "daemon.shutdown", json!({}));
    d.wait_exit(Duration::from_secs(5));
  }
  // Los auriculares ya no estan.
  let d = Daemon::start_in(dir.clone(), &[("KETAI_FAKE_OUTPUTS", "alsa_output.speakers=Speakers")]);
  let mut c = d.connect();
  wait_devices(&mut c, 1);
  let deadline = Instant::now() + Duration::from_secs(2);
  loop {
    let out = call(&mut c, 2, "player.state", json!({}))["result"]["output"].clone();
    if out.is_null() {
      break;
    }
    assert!(Instant::now() < deadline, "output stayed {out}");
    thread::sleep(Duration::from_millis(30));
  }
  drop(d);
  let _ = fs::remove_dir_all(&dir);
}
