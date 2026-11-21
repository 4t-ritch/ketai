//! Conformidad de eventos: `events.subscribe` / `events.unsubscribe` y la
//! seccion 6 de docs/protocol.md.

mod common;

use std::time::{Duration, Instant};

use common::{Client, Daemon, fixture_uri};
use serde_json::{Value, json};

/// Cliente que separa respuestas de eventos.
struct Watcher {
  client: Client,
  next_id: u64,
  events: Vec<Value>,
}

impl Watcher {
  fn new(daemon: &Daemon) -> Self {
    Self { client: daemon.connect(), next_id: 1000, events: Vec::new() }
  }

  fn call(&mut self, method: &str, params: Value) -> Value {
    self.next_id += 1;
    let id = self.next_id;
    let msg = json!({"id": id, "method": method, "params": params});
    self.client.send_raw(format!("{msg}\n").as_bytes());
    loop {
      let m = self.client.read_message().expect("connection closed");
      if m.get("event").is_some() {
        self.events.push(m);
      } else if m["id"] == id {
        assert!(m.get("error").is_none(), "unexpected error: {m}");
        return m["result"].clone();
      }
    }
  }

  fn call_raw(&mut self, method: &str, params: Value) -> Value {
    self.next_id += 1;
    let id = self.next_id;
    let msg = json!({"id": id, "method": method, "params": params});
    self.client.send_raw(format!("{msg}\n").as_bytes());
    loop {
      let m = self.client.read_message().expect("connection closed");
      if m.get("event").is_some() {
        self.events.push(m);
      } else if m["id"] == id {
        return m;
      }
    }
  }

  /// Espera un evento con ese nombre que cumpla la condicion. Lo consume.
  fn wait_event(&mut self, name: &str, timeout: Duration, cond: impl Fn(&Value) -> bool) -> Value {
    let deadline = Instant::now() + timeout;
    loop {
      if let Some(pos) = self.events.iter().position(|e| e["event"] == name && cond(&e["data"])) {
        return self.events.remove(pos);
      }
      assert!(Instant::now() < deadline, "event {name} never arrived; got {:?}", self.events);
      let remaining = deadline.saturating_duration_since(Instant::now()).max(Duration::from_millis(1));
      self.client.writer.set_read_timeout(Some(remaining)).unwrap();
      if let Some(m) = self.client.read_message() {
        self.events.push(m);
      }
    }
  }

  /// Junta todos los eventos que lleguen durante `window`.
  fn collect(&mut self, window: Duration) -> Vec<Value> {
    let deadline = Instant::now() + window;
    while Instant::now() < deadline {
      let remaining = deadline.saturating_duration_since(Instant::now()).max(Duration::from_millis(1));
      self.client.writer.set_read_timeout(Some(remaining)).unwrap();
      if let Some(m) = self.client.read_message() {
        self.events.push(m);
      }
    }
    self.client.writer.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    std::mem::take(&mut self.events)
  }

  fn subscribe(&mut self, topics: &[&str]) {
    assert_eq!(self.call("events.subscribe", json!({"topics": topics})), json!({}));
  }
}

fn names(events: &[Value]) -> Vec<String> {
  events.iter().map(|e| e["event"].as_str().unwrap_or("").to_string()).collect()
}

#[test]
fn subscribe_to_player_sends_current_state_right_away() {
  let daemon = Daemon::start();
  let mut w = Watcher::new(&daemon);
  w.subscribe(&["player"]);
  let ev = w.wait_event("player.state", Duration::from_secs(2), |_| true);
  assert_eq!(ev["data"]["status"], "stopped");
  assert!(ev.get("id").is_none(), "events carry no id");
}

#[test]
fn subscribe_validates_topics() {
  let daemon = Daemon::start();
  let mut w = Watcher::new(&daemon);
  for params in [json!({"topics": ["nope"]}), json!({"topics": []}), json!({}), json!({"topics": "player"})] {
    let reply = w.call_raw("events.subscribe", params);
    assert_eq!(reply["error"]["code"], -32602);
  }
  let reply = w.call_raw("events.unsubscribe", json!({"topics": ["nope"]}));
  assert_eq!(reply["error"]["code"], -32602);
}

#[test]
fn every_documented_topic_is_accepted() {
  let daemon = Daemon::start();
  let mut w = Watcher::new(&daemon);
  w.subscribe(&["player", "queue", "playlist", "output", "library", "lyrics", "cache"]);
}

#[test]
fn no_events_without_subscription() {
  let daemon = Daemon::start();
  let mut w = Watcher::new(&daemon);
  w.call("queue.add", json!({"uris": [fixture_uri("vorbis.ogg")]}));
  w.call("player.play", json!({}));
  assert!(w.collect(Duration::from_millis(1500)).is_empty());
}

#[test]
fn changes_by_another_client_reach_subscribers() {
  let daemon = Daemon::start();
  let mut watcher = Watcher::new(&daemon);
  watcher.subscribe(&["player", "queue"]);
  watcher.wait_event("player.state", Duration::from_secs(2), |_| true);

  let mut actor = Watcher::new(&daemon);
  actor.call("queue.add", json!({"uris": [fixture_uri("vorbis.ogg")]}));
  watcher.wait_event("queue.changed", Duration::from_secs(2), |d| *d == json!({}));

  actor.call("player.play", json!({}));
  let ev = watcher.wait_event("track.changed", Duration::from_secs(2), |_| true);
  assert_eq!(ev["data"]["track"]["uri"], fixture_uri("vorbis.ogg"));
  watcher.wait_event("player.state", Duration::from_secs(2), |d| d["status"] == "playing");

  actor.call("player.set_volume", json!({"volume": 0.3}));
  watcher.wait_event("player.state", Duration::from_secs(2), |d| d["volume"] == 0.3);

  // El actor no se suscribio: no recibe nada.
  assert!(actor.events.is_empty());
}

#[test]
fn only_subscribed_topics_are_delivered() {
  let daemon = Daemon::start();
  let mut w = Watcher::new(&daemon);
  w.subscribe(&["queue"]);
  w.call("queue.add", json!({"uris": [fixture_uri("vorbis.ogg")]}));
  w.call("player.play", json!({}));
  let got = w.collect(Duration::from_millis(1200));
  assert_eq!(names(&got), ["queue.changed"]);
}

#[test]
fn position_events_are_at_most_one_per_second() {
  let daemon = Daemon::start();
  let mut w = Watcher::new(&daemon);
  w.call("queue.add", json!({"uris": [fixture_uri("vorbis.ogg"), fixture_uri("opus.ogg")]}));
  w.subscribe(&["player"]);
  w.call("player.play", json!({}));
  let got = w.collect(Duration::from_millis(3000));
  let positions: Vec<&Value> = got.iter().filter(|e| e["event"] == "player.position").collect();
  assert!(positions.len() >= 2, "expected position updates, got {:?}", names(&got));
  assert!(positions.len() <= 4, "too many position updates: {}", positions.len());
  for p in &positions {
    assert!(p["data"]["position_ms"].is_u64());
  }
}

#[test]
fn seek_reports_position_immediately() {
  let daemon = Daemon::start();
  let mut w = Watcher::new(&daemon);
  w.call("queue.add", json!({"uris": [fixture_uri("vorbis.ogg")]}));
  w.call("player.play", json!({}));
  w.call("player.pause", json!({}));
  w.subscribe(&["player"]);
  w.call("player.seek", json!({"position_ms": 1000}));
  let ev = w.wait_event("player.position", Duration::from_millis(500), |_| true);
  let pos = ev["data"]["position_ms"].as_u64().unwrap();
  assert!((900..1200).contains(&pos), "position {pos}");
}

#[test]
fn track_end_emits_track_changed_and_queue_end_emits_stopped() {
  let daemon = Daemon::start();
  let mut w = Watcher::new(&daemon);
  w.call("queue.add", json!({"uris": [fixture_uri("vorbis.ogg"), fixture_uri("opus.ogg")]}));
  w.call("player.play", json!({}));
  w.subscribe(&["player"]);
  let ev = w.wait_event("track.changed", Duration::from_secs(5), |_| true);
  assert_eq!(ev["data"]["track"]["uri"], fixture_uri("opus.ogg"));
  w.wait_event("player.state", Duration::from_secs(5), |d| d["status"] == "stopped");
}

#[test]
fn clearing_the_queue_emits_track_changed_null() {
  let daemon = Daemon::start();
  let mut w = Watcher::new(&daemon);
  w.call("queue.add", json!({"uris": [fixture_uri("vorbis.ogg")]}));
  w.call("player.play", json!({}));
  w.subscribe(&["player", "queue"]);
  w.call("queue.clear", json!({}));
  let ev = w.wait_event("track.changed", Duration::from_secs(2), |_| true);
  assert_eq!(ev["data"]["track"], Value::Null);
  w.wait_event("queue.changed", Duration::from_secs(2), |_| true);
}

#[test]
fn unsubscribe_stops_events() {
  let daemon = Daemon::start();
  let mut w = Watcher::new(&daemon);
  w.subscribe(&["player", "queue"]);
  w.wait_event("player.state", Duration::from_secs(2), |_| true);
  assert_eq!(w.call("events.unsubscribe", json!({"topics": ["player", "queue"]})), json!({}));
  w.call("queue.add", json!({"uris": [fixture_uri("vorbis.ogg")]}));
  w.call("player.play", json!({}));
  assert!(w.collect(Duration::from_millis(1500)).is_empty());
}

#[test]
fn reply_comes_before_the_events_it_causes() {
  let daemon = Daemon::start();
  let mut w = Watcher::new(&daemon);
  w.call("queue.add", json!({"uris": [fixture_uri("vorbis.ogg")]}));
  w.subscribe(&["player"]);
  w.wait_event("player.state", Duration::from_secs(2), |_| true);
  w.collect(Duration::from_millis(100));
  // Se lee a mano: lo primero que llega tras play debe ser su respuesta.
  w.client.send_raw(b"{\"id\":77,\"method\":\"player.play\",\"params\":{}}\n");
  let first = w.client.read_message().unwrap();
  assert_eq!(first["id"], 77, "got {first}");
}
