//! Conformidad de playlists: metodos `playlist.*` y el evento
//! `playlist.changed`.

mod common;

use std::{
  fs,
  time::{Duration, Instant},
};

use common::{Client, Daemon, fixture_uri, temp_runtime_dir};
use serde_json::{Value, json};

struct S {
  daemon: Daemon,
  c: Client,
  id: u64,
}

impl S {
  fn new() -> Self {
    let daemon = Daemon::start();
    let c = daemon.connect();
    Self { daemon, c, id: 1 }
  }

  fn raw(&mut self, method: &str, params: Value) -> Value {
    self.id += 1;
    self.c.request(self.id, method, params)
  }

  fn ok(&mut self, method: &str, params: Value) -> Value {
    let r = self.raw(method, params);
    assert!(r.get("error").is_none(), "{method}: {r}");
    r["result"].clone()
  }

  fn code(&mut self, method: &str, params: Value) -> i64 {
    let r = self.raw(method, params);
    r["error"]["code"].as_i64().unwrap_or_else(|| panic!("{method} expected error, got {r}"))
  }

  fn uris_of(&mut self, name: &str) -> Vec<String> {
    let p = self.ok("playlist.get", json!({"name": name}));
    p["tracks"].as_array().unwrap().iter().map(|t| t["uri"].as_str().unwrap().to_string()).collect()
  }

  fn create(&mut self, name: &str, names: &[&str]) {
    self.ok("playlist.save", json!({"name": name, "from_queue": false}));
    if !names.is_empty() {
      let uris: Vec<String> = names.iter().map(|n| fixture_uri(n)).collect();
      self.ok("playlist.add", json!({"name": name, "uris": uris}));
    }
  }
}

fn u(name: &str) -> String {
  fixture_uri(name)
}

#[test]
fn save_from_queue_list_and_get() {
  let mut s = S::new();
  s.ok("queue.add", json!({"uris": [u("vorbis.ogg"), u("opus.ogg")]}));
  assert_eq!(s.ok("playlist.save", json!({"name": "chill", "from_queue": true})), json!({}));
  s.create("rock", &["aac_lc.m4a"]);
  assert_eq!(
    s.ok("playlist.list", json!({})),
    json!({"playlists": [{"name": "chill", "count": 2}, {"name": "rock", "count": 1}]})
  );
  let p = s.ok("playlist.get", json!({"name": "chill"}));
  assert_eq!(p["name"], "chill");
  let t = &p["tracks"][0];
  assert_eq!(t["uri"], u("vorbis.ogg"));
  assert_eq!(t["codec"], "vorbis");
  assert!(t["id"].as_str().unwrap().starts_with("t_"));
  assert_eq!(s.uris_of("chill"), [u("vorbis.ogg"), u("opus.ogg")]);
}

#[test]
fn save_replaces_existing_playlist() {
  let mut s = S::new();
  s.create("p", &["vorbis.ogg", "opus.ogg"]);
  s.ok("playlist.save", json!({"name": "p", "from_queue": true}));
  assert!(s.uris_of("p").is_empty());
}

#[test]
fn names_are_validated() {
  let mut s = S::new();
  for bad in ["", "a/b", ".hidden", "x\u{0}y"] {
    assert_eq!(s.code("playlist.save", json!({"name": bad, "from_queue": false})), -32602, "name {bad:?}");
  }
  assert_eq!(s.code("playlist.save", json!({"from_queue": false})), -32602);
  // Espacios, acentos y otros alfabetos son validos.
  s.ok("playlist.save", json!({"name": "Rock en espanol", "from_queue": false}));
  s.ok("playlist.save", json!({"name": "케이팝", "from_queue": false}));
}

#[test]
fn missing_playlist_reports_1005() {
  let mut s = S::new();
  assert_eq!(s.code("playlist.get", json!({"name": "nope"})), 1005);
  assert_eq!(s.code("playlist.delete", json!({"name": "nope"})), 1005);
  assert_eq!(s.code("playlist.load", json!({"name": "nope", "replace": true})), 1005);
  assert_eq!(s.code("playlist.add", json!({"name": "nope", "uris": [u("opus.ogg")]})), 1005);
}

#[test]
fn delete_removes_playlist() {
  let mut s = S::new();
  s.create("p", &[]);
  s.ok("playlist.delete", json!({"name": "p"}));
  assert_eq!(s.ok("playlist.list", json!({})), json!({"playlists": []}));
}

#[test]
fn rename_checks_both_names() {
  let mut s = S::new();
  s.create("a", &["opus.ogg"]);
  s.create("b", &[]);
  assert_eq!(s.code("playlist.rename", json!({"name": "a", "new_name": "b"})), 1008);
  assert_eq!(s.code("playlist.rename", json!({"name": "zz", "new_name": "c"})), 1005);
  assert_eq!(s.code("playlist.rename", json!({"name": "a", "new_name": "x/y"})), -32602);
  s.ok("playlist.rename", json!({"name": "a", "new_name": "c"}));
  assert_eq!(s.uris_of("c"), [u("opus.ogg")]);
  assert_eq!(s.code("playlist.get", json!({"name": "a"})), 1005);
}

#[test]
fn add_one_or_many_at_position_and_is_atomic() {
  let mut s = S::new();
  s.create("p", &["vorbis.ogg"]);
  let r = s.ok("playlist.add", json!({"name": "p", "uris": [u("opus.ogg"), u("aac_lc.m4a")], "position": 0}));
  assert_eq!(r, json!({"added": 2}));
  assert_eq!(s.uris_of("p"), [u("opus.ogg"), u("aac_lc.m4a"), u("vorbis.ogg")]);
  assert_eq!(s.code("playlist.add", json!({"name": "p", "uris": [u("opus.ogg"), "file:///no/such.ogg"]})), 1003);
  assert_eq!(s.code("playlist.add", json!({"name": "p", "uris": [u("opus.ogg")], "position": 9})), 1002);
  assert_eq!(s.code("playlist.add", json!({"name": "p", "uris": []})), -32602);
  assert_eq!(s.uris_of("p").len(), 3);
}

#[test]
fn remove_one_or_many_counted_before_removing() {
  let mut s = S::new();
  s.create("p", &["vorbis.ogg", "opus.ogg", "aac_lc.m4a", "he_aac.m4a"]);
  assert_eq!(s.code("playlist.remove", json!({"name": "p", "indexes": [0, 9]})), 1002);
  assert_eq!(s.code("playlist.remove", json!({"name": "p", "indexes": [1, 1]})), -32602);
  assert_eq!(s.ok("playlist.remove", json!({"name": "p", "indexes": [3, 1]})), json!({"removed": 2}));
  assert_eq!(s.uris_of("p"), [u("vorbis.ogg"), u("aac_lc.m4a")]);
  s.ok("playlist.remove", json!({"name": "p", "indexes": [0]}));
  assert_eq!(s.uris_of("p"), [u("aac_lc.m4a")]);
}

#[test]
fn move_reorders() {
  let mut s = S::new();
  s.create("p", &["vorbis.ogg", "opus.ogg", "aac_lc.m4a"]);
  s.ok("playlist.move", json!({"name": "p", "from": 2, "to": 0}));
  assert_eq!(s.uris_of("p"), [u("aac_lc.m4a"), u("vorbis.ogg"), u("opus.ogg")]);
  assert_eq!(s.code("playlist.move", json!({"name": "p", "from": 0, "to": 3})), 1002);
}

#[test]
fn transfer_copy_and_move_keep_source_order() {
  let mut s = S::new();
  s.create("rock", &["vorbis.ogg", "opus.ogg", "aac_lc.m4a", "he_aac.m4a"]);
  s.create("metal", &["vorbis.ogg"]);

  let r = s.ok("playlist.transfer", json!({
    "from_name": "rock", "indexes": [3, 1], "to_name": "metal", "position": 0, "mode": "copy"
  }));
  assert_eq!(r, json!({"transferred": 2}));
  assert_eq!(s.uris_of("metal"), [u("opus.ogg"), u("he_aac.m4a"), u("vorbis.ogg")]);
  assert_eq!(s.uris_of("rock").len(), 4, "copy leaves the source intact");

  s.ok("playlist.transfer", json!({
    "from_name": "rock", "indexes": [0, 2], "to_name": "metal", "position": null, "mode": "move"
  }));
  assert_eq!(s.uris_of("rock"), [u("opus.ogg"), u("he_aac.m4a")]);
  assert_eq!(s.uris_of("metal")[3..], [u("vorbis.ogg"), u("aac_lc.m4a")]);
}

#[test]
fn transfer_is_atomic_and_validated() {
  let mut s = S::new();
  s.create("a", &["vorbis.ogg", "opus.ogg"]);
  s.create("b", &[]);
  let base = json!({"from_name": "a", "to_name": "b", "position": null, "mode": "move"});
  let with = |extra: Value| {
    let mut v = base.clone();
    v.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
    v
  };
  assert_eq!(s.code("playlist.transfer", with(json!({"indexes": [0, 5]}))), 1002);
  assert_eq!(s.code("playlist.transfer", with(json!({"indexes": [0, 0]}))), -32602);
  assert_eq!(s.code("playlist.transfer", with(json!({"indexes": [0], "mode": "teleport"}))), -32602);
  assert_eq!(s.code("playlist.transfer", with(json!({"indexes": [0], "to_name": "zz"}))), 1005);
  assert_eq!(s.code("playlist.transfer", with(json!({"indexes": [0], "position": 3}))), 1002);
  assert_eq!(s.uris_of("a").len(), 2);
  assert!(s.uris_of("b").is_empty());
}

#[test]
fn load_replaces_or_appends_the_queue() {
  let mut s = S::new();
  s.create("p", &["opus.ogg", "aac_lc.m4a"]);
  s.ok("queue.add", json!({"uris": [u("vorbis.ogg")]}));
  s.ok("player.play", json!({}));

  s.ok("playlist.load", json!({"name": "p", "replace": false}));
  let q = s.ok("queue.list", json!({}));
  assert_eq!(q["tracks"].as_array().unwrap().len(), 3);
  assert_eq!(s.ok("player.state", json!({}))["status"], "playing", "append keeps playing");

  s.ok("playlist.load", json!({"name": "p", "replace": true}));
  let q = s.ok("queue.list", json!({}));
  let got: Vec<&str> = q["tracks"].as_array().unwrap().iter().map(|t| t["uri"].as_str().unwrap()).collect();
  assert_eq!(got, [u("opus.ogg"), u("aac_lc.m4a")]);
  assert_eq!(s.ok("player.state", json!({}))["status"], "stopped");
}

#[test]
fn changes_emit_playlist_changed() {
  let mut s = S::new();
  let mut w = s.daemon.connect();
  w.request(1, "events.subscribe", json!({"topics": ["playlist"]}));
  s.create("rock", &["vorbis.ogg", "opus.ogg"]);
  s.create("metal", &[]);
  s.ok("playlist.transfer", json!({
    "from_name": "rock", "indexes": [0], "to_name": "metal", "position": null, "mode": "move"
  }));
  let deadline = Instant::now() + Duration::from_secs(2);
  let mut names = Vec::new();
  while names.len() < 5 && Instant::now() < deadline {
    if let Some(m) = w.read_message()
      && m["event"] == "playlist.changed"
    {
      names.push(m["data"]["name"].as_str().unwrap().to_string());
    }
  }
  // save rock, add rock, save metal, transfer: primero el destino (se
  // escribe primero) y despues el origen.
  assert_eq!(names, ["rock", "rock", "metal", "metal", "rock"]);
}

#[test]
fn playlists_are_m3u8_files_that_survive_restarts() {
  let dir = temp_runtime_dir();
  {
    let d = Daemon::start_in(dir.clone(), &[]);
    let mut c = d.connect();
    c.request(1, "playlist.save", json!({"name": "mix", "from_queue": false}));
    c.request(2, "playlist.add", json!({"name": "mix", "uris": [u("vorbis.ogg")]}));
  }
  let file = dir.join("data/ketai/playlists/mix.m3u8");
  let text = fs::read_to_string(&file).expect("m3u8 file");
  assert!(text.starts_with("#EXTM3U"), "{text}");
  assert!(text.contains(u("vorbis.ogg").trim_start_matches("file://")), "plain path for interop: {text}");

  // Una playlist copiada a mano tambien aparece.
  fs::write(dir.join("data/ketai/playlists/hand made.m3u8"), format!("#EXTM3U\n{}\n", u("opus.ogg").trim_start_matches("file://"))).unwrap();
  let d = Daemon::start_in(dir.clone(), &[]);
  let mut c = d.connect();
  let list = c.request(1, "playlist.list", json!({}));
  assert_eq!(list["result"]["playlists"], json!([{"name": "hand made", "count": 1}, {"name": "mix", "count": 1}]));
  let got = c.request(2, "playlist.get", json!({"name": "hand made"}));
  assert_eq!(got["result"]["tracks"][0]["uri"], u("opus.ogg"));
  drop(c);
  drop(d);
  let _ = fs::remove_dir_all(&dir);
}

#[test]
fn get_shows_missing_files_without_failing() {
  let dir = temp_runtime_dir();
  let d = Daemon::start_in(dir.clone(), &[]);
  let pl = dir.join("data/ketai/playlists");
  fs::create_dir_all(&pl).unwrap();
  fs::write(pl.join("old.m3u8"), format!("#EXTM3U\n/no/such/file.ogg\n{}\n", u("opus.ogg").trim_start_matches("file://"))).unwrap();
  let mut c = d.connect();
  let got = c.request(1, "playlist.get", json!({"name": "old"}));
  let tracks = got["result"]["tracks"].as_array().unwrap();
  assert_eq!(tracks.len(), 2);
  assert_eq!(tracks[0]["uri"], "file:///no/such/file.ogg");
  assert_eq!(tracks[0]["codec"], Value::Null);
  drop(c);
  drop(d);
  let _ = fs::remove_dir_all(&dir);
}

#[test]
fn transfer_within_the_same_playlist_is_rejected() {
  let mut s = S::new();
  s.create("rock", &["vorbis.ogg", "opus.ogg", "aac_lc.m4a"]);
  for mode in ["move", "copy"] {
    let code = s.code("playlist.transfer", json!({
      "from_name": "rock", "indexes": [0], "to_name": "rock", "position": 3, "mode": mode
    }));
    assert_eq!(code, -32602, "mode {mode}");
  }
  assert_eq!(s.uris_of("rock"), [u("vorbis.ogg"), u("opus.ogg"), u("aac_lc.m4a")]);
}

#[test]
fn transfer_copy_only_reports_the_destination_as_changed() {
  let mut s = S::new();
  s.create("a", &["vorbis.ogg"]);
  s.create("b", &[]);
  let mut w = s.daemon.connect();
  w.request(1, "events.subscribe", json!({"topics": ["playlist"]}));
  s.ok("playlist.transfer", json!({"from_name": "a", "indexes": [0], "to_name": "b", "position": null, "mode": "copy"}));
  s.ok("playlist.list", json!({}));
  w.writer.set_read_timeout(Some(Duration::from_millis(500))).unwrap();
  let mut names = Vec::new();
  while let Some(m) = w.read_message() {
    if m["event"] == "playlist.changed" {
      names.push(m["data"]["name"].as_str().unwrap().to_string());
    }
  }
  assert_eq!(names, ["b"]);
}

#[test]
fn temp_files_are_not_left_in_the_playlists_folder() {
  let dir = temp_runtime_dir();
  let d = Daemon::start_in(dir.clone(), &[]);
  let mut c = d.connect();
  c.request(1, "playlist.save", json!({"name": "x", "from_queue": false}));
  let names: Vec<String> = fs::read_dir(dir.join("data/ketai/playlists"))
    .unwrap()
    .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
    .collect();
  assert_eq!(names, ["x.m3u8"]);
  drop(c);
  drop(d);
  let _ = fs::remove_dir_all(&dir);
}
