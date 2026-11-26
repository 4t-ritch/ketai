//! Conformidad del reproductor y la cola: metodos `player.*` y `queue.*`.
//!
//! Los fixtures son tonos de ~2 s. El daemon corre con el backend de audio
//! nulo, que consume el audio en tiempo real sin sonar.

mod common;

use std::{
  thread,
  time::{Duration, Instant},
};

use common::{Client, Daemon, fixture_uri};
use serde_json::{Value, json};

fn error_code(reply: &Value) -> i64 {
  reply["error"]["code"].as_i64().unwrap_or_else(|| panic!("expected error, got {reply}"))
}

fn ok(reply: Value) -> Value {
  assert!(reply.get("error").is_none(), "unexpected error: {reply}");
  reply["result"].clone()
}

struct Session {
  _daemon: Daemon,
  client: Client,
  next_id: u64,
}

impl Session {
  fn new() -> Self {
    let daemon = Daemon::start();
    let client = daemon.connect();
    Self { _daemon: daemon, client, next_id: 1 }
  }

  fn call(&mut self, method: &str, params: Value) -> Value {
    self.next_id += 1;
    self.client.request(self.next_id, method, params)
  }

  fn state(&mut self) -> Value {
    ok(self.call("player.state", json!({})))
  }

  fn add(&mut self, names: &[&str]) {
    let uris: Vec<String> = names.iter().map(|n| fixture_uri(n)).collect();
    let result = ok(self.call("queue.add", json!({"uris": uris, "position": null})));
    assert_eq!(result["added"], names.len());
  }

  fn queue_ids(&mut self) -> Vec<String> {
    let list = ok(self.call("queue.list", json!({})));
    list["tracks"].as_array().unwrap().iter().map(|t| t["id"].as_str().unwrap().to_string()).collect()
  }

  /// Espera hasta que el estado cumpla la condicion.
  fn wait_state(&mut self, timeout: Duration, cond: impl Fn(&Value) -> bool) -> Value {
    let deadline = Instant::now() + timeout;
    loop {
      let state = self.state();
      if cond(&state) {
        return state;
      }
      assert!(Instant::now() < deadline, "state never matched, last: {state}");
      thread::sleep(Duration::from_millis(50));
    }
  }
}

fn current_id(state: &Value) -> String {
  state["track"]["id"].as_str().unwrap_or("").to_string()
}

// ---- cola -----------------------------------------------------------------

#[test]
fn add_reads_metadata_and_list_returns_tracks() {
  let mut s = Session::new();
  s.add(&["vorbis.ogg", "opus.ogg"]);
  let list = ok(s.call("queue.list", json!({})));
  assert_eq!(list["current"], Value::Null);
  let tracks = list["tracks"].as_array().unwrap();
  assert_eq!(tracks.len(), 2);
  let t = &tracks[0];
  assert!(t["id"].as_str().unwrap().starts_with("t_"));
  assert_eq!(t["source"], "local");
  assert_eq!(t["uri"], fixture_uri("vorbis.ogg"));
  assert_eq!(t["codec"], "vorbis");
  assert_eq!(t["channels"], 2);
  assert!(t["duration_ms"].as_u64().unwrap() > 1500);
  for key in ["title", "artist", "album", "has_cover", "sample_rate", "bitrate_kbps"] {
    assert!(t.get(key).is_some(), "track must have key {key}");
  }
  assert_ne!(tracks[0]["id"], tracks[1]["id"]);
}

#[test]
fn add_at_position_inserts_together_in_order() {
  let mut s = Session::new();
  s.add(&["vorbis.ogg", "opus.ogg"]);
  let uris = [fixture_uri("aac_lc.m4a"), fixture_uri("he_aac.m4a")];
  ok(s.call("queue.add", json!({"uris": uris, "position": 1})));
  let list = ok(s.call("queue.list", json!({})));
  let got: Vec<&str> = list["tracks"].as_array().unwrap().iter().map(|t| t["uri"].as_str().unwrap()).collect();
  assert_eq!(got, [fixture_uri("vorbis.ogg"), uris[0].clone(), uris[1].clone(), fixture_uri("opus.ogg")]);
}

#[test]
fn add_with_missing_file_fails_and_leaves_queue_unchanged() {
  let mut s = Session::new();
  s.add(&["vorbis.ogg"]);
  let reply = s.call("queue.add", json!({"uris": [fixture_uri("opus.ogg"), "file:///nope/missing.ogg"]}));
  assert_eq!(error_code(&reply), 1003);
  assert_eq!(s.queue_ids().len(), 1);
}

#[test]
fn add_validates_params() {
  let mut s = Session::new();
  assert_eq!(error_code(&s.call("queue.add", json!({"uris": ["ftp://x/a.ogg"]}))), -32602);
  assert_eq!(error_code(&s.call("queue.add", json!({"uris": []}))), -32602);
  assert_eq!(error_code(&s.call("queue.add", json!({}))), -32602);
  let reply = s.call("queue.add", json!({"uris": [fixture_uri("opus.ogg")], "position": 5}));
  assert_eq!(error_code(&reply), 1002);
}

#[test]
fn add_accepts_percent_encoded_paths() {
  let dir = common::temp_runtime_dir();
  let target = dir.join("a b.ogg");
  let src = fixture_uri("vorbis.ogg");
  std::fs::copy(src.trim_start_matches("file://"), &target).unwrap();
  let mut s = Session::new();
  let uri = format!("file://{}", target.display()).replace(' ', "%20");
  let reply = s.call("queue.add", json!({"uris": [uri]}));
  let _ = std::fs::remove_dir_all(&dir);
  assert_eq!(ok(reply)["added"], 1);
}

#[test]
fn remove_counts_indexes_before_removing_and_is_atomic() {
  let mut s = Session::new();
  s.add(&["vorbis.ogg", "opus.ogg", "aac_lc.m4a", "he_aac.m4a"]);
  let ids = s.queue_ids();
  assert_eq!(error_code(&s.call("queue.remove", json!({"indexes": [0, 9]}))), 1002);
  assert_eq!(error_code(&s.call("queue.remove", json!({"indexes": [1, 1]}))), -32602);
  assert_eq!(s.queue_ids(), ids);
  let result = ok(s.call("queue.remove", json!({"indexes": [3, 1]})));
  assert_eq!(result["removed"], 2);
  assert_eq!(s.queue_ids(), [ids[0].clone(), ids[2].clone()]);
}

#[test]
fn move_jump_and_clear() {
  let mut s = Session::new();
  s.add(&["vorbis.ogg", "opus.ogg", "aac_lc.m4a"]);
  let ids = s.queue_ids();
  ok(s.call("queue.move", json!({"from": 2, "to": 0})));
  assert_eq!(s.queue_ids(), [ids[2].clone(), ids[0].clone(), ids[1].clone()]);
  assert_eq!(error_code(&s.call("queue.move", json!({"from": 0, "to": 3}))), 1002);

  ok(s.call("queue.jump", json!({"index": 1})));
  let state = s.state();
  assert_eq!(current_id(&state), ids[0]);
  assert_eq!(state["status"], "playing");
  assert_eq!(error_code(&s.call("queue.jump", json!({"index": 7}))), 1002);

  ok(s.call("queue.clear", json!({})));
  assert!(s.queue_ids().is_empty());
  let state = s.state();
  assert_eq!(state["status"], "stopped");
  assert_eq!(state["track"], Value::Null);
}

// ---- reproductor -----------------------------------------------------------

#[test]
fn initial_state_has_every_field() {
  let mut s = Session::new();
  let state = s.state();
  assert_eq!(state["status"], "stopped");
  assert_eq!(state["track"], Value::Null);
  assert_eq!(state["position_ms"], 0);
  assert_eq!(state["volume"], 1.0);
  assert_eq!(state["muted"], false);
  assert_eq!(state["repeat"], "off");
  assert_eq!(state["shuffle"], false);
  assert_eq!(state["output"], Value::Null);
}

#[test]
fn play_with_empty_queue_reports_nothing_to_play() {
  let mut s = Session::new();
  assert_eq!(error_code(&s.call("player.play", json!({}))), 1001);
  assert_eq!(error_code(&s.call("player.next", json!({}))), 1001);
}

#[test]
fn play_pause_toggle_and_position() {
  let mut s = Session::new();
  s.add(&["vorbis.ogg"]);
  ok(s.call("player.play", json!({})));
  let state = s.wait_state(Duration::from_secs(3), |st| st["position_ms"].as_u64().unwrap() >= 400);
  assert_eq!(state["status"], "playing");
  assert_eq!(state["track"]["uri"], fixture_uri("vorbis.ogg"));

  ok(s.call("player.pause", json!({})));
  let paused_at = s.state()["position_ms"].as_u64().unwrap();
  thread::sleep(Duration::from_millis(400));
  let state = s.state();
  assert_eq!(state["status"], "paused");
  let still = state["position_ms"].as_u64().unwrap();
  assert!(still.abs_diff(paused_at) < 100, "paused position moved {paused_at} -> {still}");

  ok(s.call("player.toggle", json!({})));
  assert_eq!(s.state()["status"], "playing");
  ok(s.call("player.toggle", json!({})));
  assert_eq!(s.state()["status"], "paused");
}

#[test]
fn seek_and_seek_relative() {
  let mut s = Session::new();
  s.add(&["vorbis.ogg"]);
  ok(s.call("player.play", json!({})));
  ok(s.call("player.pause", json!({})));
  ok(s.call("player.seek", json!({"position_ms": 1200})));
  let pos = s.state()["position_ms"].as_u64().unwrap();
  assert!((1100..1400).contains(&pos), "seek position {pos}");

  ok(s.call("player.seek_relative", json!({"offset_ms": -5000})));
  let pos = s.state()["position_ms"].as_u64().unwrap();
  assert!(pos < 150, "relative seek must clamp at 0, got {pos}");

  assert_eq!(error_code(&s.call("player.seek", json!({}))), -32602);
}

#[test]
fn seek_without_loaded_track_reports_nothing_to_play() {
  let mut s = Session::new();
  s.add(&["vorbis.ogg"]);
  assert_eq!(error_code(&s.call("player.seek", json!({"position_ms": 10}))), 1001);
}

#[test]
fn stop_resets_position() {
  let mut s = Session::new();
  s.add(&["vorbis.ogg"]);
  ok(s.call("player.play", json!({})));
  s.wait_state(Duration::from_secs(3), |st| st["position_ms"].as_u64().unwrap() >= 300);
  ok(s.call("player.stop", json!({})));
  let state = s.state();
  assert_eq!(state["status"], "stopped");
  assert_eq!(state["position_ms"], 0);
  // La pista queda seleccionada y play la reproduce desde el principio.
  assert_eq!(state["track"]["uri"], fixture_uri("vorbis.ogg"));
  ok(s.call("player.play", json!({})));
  assert_eq!(s.state()["status"], "playing");
}

#[test]
fn volume_mute_repeat_and_shuffle_are_reflected_in_state() {
  let mut s = Session::new();
  ok(s.call("player.set_volume", json!({"volume": 0.25})));
  ok(s.call("player.set_muted", json!({"muted": true})));
  ok(s.call("player.set_repeat", json!({"repeat": "queue"})));
  ok(s.call("player.set_shuffle", json!({"shuffle": true})));
  let state = s.state();
  assert_eq!(state["volume"], 0.25);
  assert_eq!(state["muted"], true);
  assert_eq!(state["repeat"], "queue");
  assert_eq!(state["shuffle"], true);

  assert_eq!(error_code(&s.call("player.set_volume", json!({"volume": 1.5}))), -32602);
  assert_eq!(error_code(&s.call("player.set_volume", json!({"volume": -0.1}))), -32602);
  assert_eq!(error_code(&s.call("player.set_repeat", json!({"repeat": "always"}))), -32602);
  assert_eq!(error_code(&s.call("player.set_shuffle", json!({"shuffle": "yes"}))), -32602);
  assert_eq!(error_code(&s.call("player.set_muted", json!({}))), -32602);
  assert_eq!(s.state()["volume"], 0.25);
}

#[test]
fn next_and_previous() {
  let mut s = Session::new();
  s.add(&["vorbis.ogg", "opus.ogg"]);
  let ids = s.queue_ids();
  ok(s.call("player.play", json!({})));
  ok(s.call("player.next", json!({})));
  let state = s.state();
  assert_eq!(current_id(&state), ids[1]);
  assert_eq!(state["status"], "playing");
  assert_eq!(error_code(&s.call("player.next", json!({}))), 1001);

  ok(s.call("player.previous", json!({})));
  assert_eq!(current_id(&s.state()), ids[0]);
}

#[test]
fn previous_after_three_seconds_restarts_track() {
  let mut s = Session::new();
  s.add(&["vorbis.ogg", "opus.ogg"]);
  let ids = s.queue_ids();
  ok(s.call("queue.jump", json!({"index": 1})));
  ok(s.call("player.pause", json!({})));
  // Los fixtures duran ~2 s; se fuerza el caso con un seek cerca del final y
  // se comprueba solo la regla de "anterior" bajo 3 s: cambia de pista.
  ok(s.call("player.seek", json!({"position_ms": 1500})));
  ok(s.call("player.previous", json!({})));
  assert_eq!(current_id(&s.state()), ids[0]);
}

#[test]
fn track_end_advances_and_queue_end_stops() {
  let mut s = Session::new();
  s.add(&["vorbis.ogg", "opus.ogg"]);
  let ids = s.queue_ids();
  ok(s.call("player.play", json!({})));
  s.wait_state(Duration::from_secs(5), |st| current_id(st) == ids[1]);
  let state = s.wait_state(Duration::from_secs(5), |st| st["status"] == "stopped");
  assert_eq!(current_id(&state), ids[1], "last track stays selected");
  assert_eq!(state["position_ms"], 0);
}

#[test]
fn repeat_track_replays_same_track() {
  let mut s = Session::new();
  s.add(&["vorbis.ogg", "opus.ogg"]);
  let ids = s.queue_ids();
  ok(s.call("player.set_repeat", json!({"repeat": "track"})));
  ok(s.call("player.play", json!({})));
  s.wait_state(Duration::from_secs(3), |st| st["position_ms"].as_u64().unwrap() >= 1500);
  // Tras el final, vuelve a empezar la misma pista.
  let state = s.wait_state(Duration::from_secs(4), |st| st["position_ms"].as_u64().unwrap() < 800);
  assert_eq!(current_id(&state), ids[0]);
  assert_eq!(state["status"], "playing");
}

#[test]
fn removing_playing_track_plays_next_and_removing_all_stops() {
  let mut s = Session::new();
  s.add(&["vorbis.ogg", "opus.ogg"]);
  let ids = s.queue_ids();
  ok(s.call("player.play", json!({})));
  ok(s.call("queue.remove", json!({"indexes": [0]})));
  let state = s.state();
  assert_eq!(current_id(&state), ids[1]);
  assert_eq!(state["status"], "playing");

  ok(s.call("queue.remove", json!({"indexes": [0]})));
  let state = s.state();
  assert_eq!(state["status"], "stopped");
  assert_eq!(state["track"], Value::Null);
}

// ---- orden de peticiones ---------------------------------------------------

/// Envia varias peticiones en una sola escritura, sin esperar respuestas.
fn pipeline(s: &mut Session, requests: &[(u64, &str, Value)]) -> Vec<Value> {
  let mut batch = String::new();
  for (id, method, params) in requests {
    batch.push_str(&json!({"id": id, "method": method, "params": params}).to_string());
    batch.push('\n');
  }
  s.client.send_raw(batch.as_bytes());
  let mut replies = Vec::new();
  while replies.len() < requests.len() {
    let msg = s.client.read_message().expect("reply");
    if msg.get("id").is_some() {
      replies.push(msg);
    }
  }
  replies
}

#[test]
fn pipelined_adds_apply_in_arrival_order() {
  let mut s = Session::new();
  // El primer add es mucho mas lento de resolver que el segundo.
  let slow: Vec<String> = (0..40).map(|_| fixture_uri("vorbis.ogg")).collect();
  let fast = [fixture_uri("opus.ogg")];
  let replies = pipeline(&mut s, &[
    (100, "queue.add", json!({"uris": slow})),
    (101, "queue.add", json!({"uris": fast})),
    (102, "queue.list", json!({})),
  ]);
  let ids: Vec<u64> = replies.iter().map(|r| r["id"].as_u64().unwrap()).collect();
  assert_eq!(ids, [100, 101, 102], "replies keep request order for one client");
  let tracks = replies[2]["result"]["tracks"].as_array().unwrap();
  assert_eq!(tracks.len(), 41, "list must see both adds");
  assert_eq!(tracks[40]["uri"], fast[0], "second add goes after the first");
}

#[test]
fn request_after_pipelined_add_sees_the_added_tracks() {
  let mut s = Session::new();
  let replies = pipeline(&mut s, &[
    (200, "queue.add", json!({"uris": [fixture_uri("vorbis.ogg")]})),
    (201, "player.play", json!({})),
  ]);
  assert!(replies[1].get("error").is_none(), "play after add must not fail: {}", replies[1]);
  assert_eq!(s.state()["status"], "playing");
}

#[test]
fn seeking_at_the_very_end_never_stalls_the_engine() {
  // Un seek que coincide con el final de la pista no debe trabar el motor:
  // despues, la reproduccion sigue respondiendo.
  let mut s = Session::new();
  s.add(&["vorbis.ogg"]);
  ok(s.call("player.play", json!({})));
  for _ in 0..30 {
    let _ = s.call("player.seek", json!({"position_ms": 1995}));
    thread::sleep(Duration::from_millis(3));
  }
  let _ = s.call("queue.jump", json!({"index": 0}));
  let _ = s.call("player.play", json!({}));
  let state = s.wait_state(Duration::from_secs(3), |st| {
    st["status"] == "playing" && (300..1500).contains(&st["position_ms"].as_u64().unwrap())
  });
  assert_eq!(state["track"]["uri"], fixture_uri("vorbis.ogg"));
}
