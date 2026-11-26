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
  let file = dir.join("music/playlists/mix.m3u8");
  let text = fs::read_to_string(&file).expect("m3u8 file");
  assert!(text.starts_with("#EXTM3U"), "{text}");
  assert!(text.contains(u("vorbis.ogg").trim_start_matches("file://")), "plain path for interop: {text}");

  // Una playlist copiada a mano tambien aparece.
  fs::write(dir.join("music/playlists/hand made.m3u8"), format!("#EXTM3U\n{}\n", u("opus.ogg").trim_start_matches("file://"))).unwrap();
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
  let pl = dir.join("music/playlists");
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
  let names: Vec<String> = fs::read_dir(dir.join("music/playlists"))
    .unwrap()
    .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
    .collect();
  assert_eq!(names, ["x.m3u8"]);
  drop(c);
  drop(d);
  let _ = fs::remove_dir_all(&dir);
}

/// Daemon con HOME propio y sin XDG_MUSIC_DIR, para probar de donde sale la
/// carpeta de playlists.
fn daemon_with_home(dir: &std::path::Path) -> Daemon {
  fs::create_dir_all(dir.join("home")).unwrap();
  let home = dir.join("home").display().to_string();
  let config = dir.join("config").display().to_string();
  Daemon::start_in(dir.to_path_buf(), &[("HOME", &home), ("XDG_MUSIC_DIR", ""), ("XDG_CONFIG_HOME", &config)])
}

#[test]
fn playlists_go_to_the_music_folder_and_it_is_created() {
  let dir = temp_runtime_dir();
  {
    let d = Daemon::start_in(dir.clone(), &[]);
    let mut c = d.connect();
    assert!(!dir.join("music/playlists").exists());
    c.request(1, "playlist.save", json!({"name": "mix", "from_queue": false}));
  }
  assert!(dir.join("music/playlists/mix.m3u8").is_file(), "default: XDG_MUSIC_DIR/playlists, created on demand");
  {
    let d = Daemon::start_in(dir.clone(), &[]);
    let mut c = d.connect();
    let got = c.request(1, "playlist.get", json!({"name": "mix"}));
    assert_eq!(got["result"]["path"], json!(dir.join("music/playlists/mix.m3u8")), "playlist.get says where the file is");
  }
  let _ = fs::remove_dir_all(&dir);
}

#[test]
fn the_music_folder_comes_from_user_dirs_when_the_variable_is_missing() {
  let dir = temp_runtime_dir();
  fs::create_dir_all(dir.join("config")).unwrap();
  fs::write(dir.join("config/user-dirs.dirs"), "# xdg\nXDG_DESKTOP_DIR=\"$HOME/desk\"\nXDG_MUSIC_DIR=\"$HOME/musica\"\n").unwrap();
  {
    let d = daemon_with_home(&dir);
    let mut c = d.connect();
    c.request(1, "playlist.save", json!({"name": "mix", "from_queue": false}));
  }
  assert!(dir.join("home/musica/playlists/mix.m3u8").is_file());
  // Sin user-dirs.dirs: ~/music.
  let dir2 = temp_runtime_dir();
  {
    let d = daemon_with_home(&dir2);
    let mut c = d.connect();
    c.request(1, "playlist.save", json!({"name": "mix", "from_queue": false}));
  }
  assert!(dir2.join("home/music/playlists/mix.m3u8").is_file());
  let _ = fs::remove_dir_all(&dir);
  let _ = fs::remove_dir_all(&dir2);
}

#[test]
fn config_toml_picks_the_folder_and_changes_apply_without_restart() {
  let dir = temp_runtime_dir();
  fs::create_dir_all(dir.join("config/ketai")).unwrap();
  let cfg = dir.join("config/ketai/config.toml");
  fs::write(&cfg, "[theme]\naccent = \"cover\"\n[playlists]\ndir = \"~/listas\"\n").unwrap();
  let d = daemon_with_home(&dir);
  let mut c = d.connect();
  c.request(1, "playlist.save", json!({"name": "a", "from_queue": false}));
  assert!(dir.join("home/listas/a.m3u8").is_file());
  // Otra carpeta: se usa desde la proxima operacion. Las playlists viven en
  // la base: todas se copian a la carpeta nueva.
  fs::write(&cfg, format!("[playlists]\ndir = \"{}\"\n", dir.join("otra").display())).unwrap();
  c.request(2, "playlist.save", json!({"name": "b", "from_queue": false}));
  assert!(dir.join("otra/b.m3u8").is_file());
  assert!(dir.join("otra/a.m3u8").is_file());
  let list = c.request(3, "playlist.list", json!({}));
  assert_eq!(list["result"]["playlists"], json!([{"name": "a", "count": 0}, {"name": "b", "count": 0}]));
  // Un archivo roto: se usa la carpeta por defecto.
  fs::write(&cfg, "[playlists\ndir = 3\n").unwrap();
  c.request(4, "playlist.save", json!({"name": "c", "from_queue": false}));
  assert!(dir.join("home/music/playlists/c.m3u8").is_file());
  drop(c);
  drop(d);
  let _ = fs::remove_dir_all(&dir);
}

#[test]
fn two_clients_adding_at_once_never_lose_a_track() {
  let mut s = S::new();
  s.create("shared", &[]);
  let handles: Vec<_> = (0..2)
    .map(|k| {
      let mut c = s.daemon.connect();
      std::thread::spawn(move || {
        for i in 0..20 {
          let name = if (i + k) % 2 == 0 { "vorbis.ogg" } else { "opus.ogg" };
          let r = c.request(100 + i, "playlist.add", json!({"name": "shared", "uris": [u(name)], "duplicates": "add"}));
          assert_eq!(r["result"], json!({"added": 1}), "{r}");
        }
      })
    })
    .collect();
  for h in handles {
    h.join().unwrap();
  }
  assert_eq!(s.uris_of("shared").len(), 40);
  let file = fs::read_to_string(s.daemon.runtime_dir.join("music/playlists/shared.m3u8")).unwrap();
  assert_eq!(file.lines().filter(|l| !l.starts_with('#')).count(), 40, "the file copy matches the database");
}

#[test]
fn a_track_can_be_in_many_playlists_and_in_favorites() {
  let mut s = S::new();
  s.create("rock", &["vorbis.ogg"]);
  s.create("chill", &["vorbis.ogg", "opus.ogg"]);
  assert_eq!(s.ok("track.playlists", json!({"uri": u("vorbis.ogg")})), json!({"playlists": ["chill", "rock"], "favorite": false}));
  assert_eq!(s.ok("favorites.set", json!({"uri": u("vorbis.ogg"), "favorite": true})), json!({}));
  assert_eq!(s.ok("favorites.set", json!({"uri": u("vorbis.ogg"), "favorite": true})), json!({}), "twice is harmless");
  assert_eq!(s.ok("track.playlists", json!({"uri": u("vorbis.ogg")})), json!({"playlists": ["chill", "rock"], "favorite": true}));
  assert_eq!(s.ok("track.playlists", json!({"uri": u("opus.ogg")})), json!({"playlists": ["chill"], "favorite": false}));
  // Favoritos es una playlist mas, marcada, con su copia en la carpeta.
  let list = s.ok("playlist.list", json!({}));
  let fav = list["playlists"].as_array().unwrap().iter().find(|p| p["name"] == "Favoritos").cloned();
  assert_eq!(fav, Some(json!({"name": "Favoritos", "count": 1, "kind": "favorites"})));
  assert_eq!(s.uris_of("Favoritos"), [u("vorbis.ogg")]);
  assert!(s.daemon.runtime_dir.join("music/playlists/Favoritos.m3u8").is_file());
  s.ok("favorites.set", json!({"uri": u("vorbis.ogg"), "favorite": false}));
  assert!(s.uris_of("Favoritos").is_empty());
  for bad in [json!({}), json!({"uri": "nope", "favorite": true}), json!({"uri": u("vorbis.ogg")})] {
    assert_eq!(s.code("favorites.set", bad), -32602);
  }
  assert_eq!(s.code("track.playlists", json!({})), -32602);
}

#[test]
fn the_database_wins_over_files_edited_by_hand() {
  let mut s = S::new();
  s.create("mine", &["vorbis.ogg"]);
  let file = s.daemon.runtime_dir.join("music/playlists/mine.m3u8");
  // Editado a mano: la base manda y el archivo se reescribe.
  fs::write(&file, "#EXTM3U\n/somewhere/else.ogg\n").unwrap();
  assert_eq!(s.uris_of("mine"), [u("vorbis.ogg")]);
  assert!(!fs::read_to_string(&file).unwrap().contains("else.ogg"));
  // Borrado a mano: vuelve a aparecer.
  fs::remove_file(&file).unwrap();
  s.ok("playlist.list", json!({}));
  assert!(file.is_file());
}

#[test]
fn adding_a_track_that_is_already_there_asks_first() {
  let mut s = S::new();
  s.create("mix", &["vorbis.ogg"]);
  // Sin decir que hacer: no se agrega nada y el error dice cuales estan.
  let r = s.raw("playlist.add", json!({"name": "mix", "uris": [u("opus.ogg"), u("vorbis.ogg")]}));
  assert_eq!(r["error"]["code"], 1009, "{r}");
  assert_eq!(r["error"]["data"], json!({"duplicates": [u("vorbis.ogg")]}));
  assert_eq!(s.uris_of("mix"), [u("vorbis.ogg")], "nothing was added");
  // El usuario decide: agregar de todos modos o solo las que faltan.
  assert_eq!(s.ok("playlist.add", json!({"name": "mix", "uris": [u("opus.ogg"), u("vorbis.ogg")], "duplicates": "skip"})), json!({"added": 1}));
  assert_eq!(s.uris_of("mix"), [u("vorbis.ogg"), u("opus.ogg")]);
  assert_eq!(s.ok("playlist.add", json!({"name": "mix", "uris": [u("vorbis.ogg")], "duplicates": "add"})), json!({"added": 1}));
  assert_eq!(s.uris_of("mix"), [u("vorbis.ogg"), u("opus.ogg"), u("vorbis.ogg")]);
  assert_eq!(s.code("playlist.add", json!({"name": "mix", "uris": [u("opus.ogg")], "duplicates": "maybe"})), -32602);
  // Sin repetidas sigue igual que antes.
  s.create("new", &[]);
  assert_eq!(s.ok("playlist.add", json!({"name": "new", "uris": [u("opus.ogg")]})), json!({"added": 1}));
}

#[test]
fn remove_by_uri_takes_out_every_copy_of_that_track() {
  let mut s = S::new();
  s.create("mix", &["vorbis.ogg", "opus.ogg"]);
  s.ok("playlist.add", json!({"name": "mix", "uris": [u("vorbis.ogg")], "duplicates": "add"}));
  assert_eq!(s.ok("playlist.remove", json!({"name": "mix", "uris": [u("vorbis.ogg")]})), json!({"removed": 2}));
  assert_eq!(s.uris_of("mix"), [u("opus.ogg")]);
  // Una que no esta: no pasa nada.
  assert_eq!(s.ok("playlist.remove", json!({"name": "mix", "uris": [u("vorbis.ogg")]})), json!({"removed": 0}));
  // Hace falta uno de los dos, no ambos.
  assert_eq!(s.code("playlist.remove", json!({"name": "mix"})), -32602);
  assert_eq!(s.code("playlist.remove", json!({"name": "mix", "indexes": [0], "uris": [u("opus.ogg")]})), -32602);
  // Por posicion sigue igual.
  assert_eq!(s.ok("playlist.remove", json!({"name": "mix", "indexes": [0]})), json!({"removed": 1}));
}
