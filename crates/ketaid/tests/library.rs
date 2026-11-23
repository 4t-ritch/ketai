//! Conformidad de biblioteca local, caratulas y cache: `library.scan`,
//! `library.search`, el evento `library.progress`, `track.cover`,
//! `cache.info` y `cache.clear`.

mod common;

use std::{
  fs,
  os::unix::fs::PermissionsExt,
  path::{Path, PathBuf},
  time::{Duration, Instant},
};

use common::{Client, Daemon, fixture_uri, temp_runtime_dir};
use serde_json::{Value, json};

struct S {
  dir: PathBuf,
  daemon: Option<Daemon>,
  c: Client,
  id: u64,
}

impl Drop for S {
  fn drop(&mut self) {
    self.daemon = None;
    let _ = fs::remove_dir_all(&self.dir);
  }
}

impl S {
  fn new() -> Self {
    let dir = temp_runtime_dir();
    fs::create_dir_all(dir.join("music")).unwrap();
    let daemon = Daemon::start_in(dir.clone(), &[]);
    let c = daemon.connect();
    Self { dir, daemon: Some(daemon), c, id: 1 }
  }

  fn restart(&mut self) {
    self.daemon = None;
    let daemon = Daemon::start_in(self.dir.clone(), &[]);
    self.c = daemon.connect();
    self.daemon = Some(daemon);
  }

  fn music(&self) -> PathBuf {
    self.dir.join("music")
  }

  fn put(&self, fixture: &str, rel: &str) -> PathBuf {
    let target = self.music().join(rel);
    fs::create_dir_all(target.parent().unwrap()).unwrap();
    fs::copy(fixture_uri(fixture).trim_start_matches("file://"), &target).unwrap();
    target
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
    r["error"]["code"].as_i64().unwrap_or_else(|| panic!("{method}: expected error, got {r}"))
  }

  /// Escanea todas las carpetas y espera el final.
  fn scan_all(&mut self) -> Vec<Value> {
    self.scan_with(json!({}))
  }

  /// Escanea y espera el ultimo `library.progress` (done == total).
  fn scan(&mut self, path: &Path) -> Vec<Value> {
    self.scan_with(json!({"path": path}))
  }

  fn scan_with(&mut self, params: Value) -> Vec<Value> {
    let mut w = self.daemon.as_ref().unwrap().connect();
    w.request(1, "events.subscribe", json!({"topics": ["library"]}));
    let job = self.ok("library.scan", params)["job"].as_str().unwrap().to_string();
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut events = Vec::new();
    loop {
      assert!(Instant::now() < deadline, "scan never finished: {events:?}");
      let m = w.read_message().expect("event");
      if m["event"] != "library.progress" || m["data"]["job"] != job.as_str() {
        continue;
      }
      let done = m["data"]["done"] == m["data"]["total"];
      events.push(m["data"].clone());
      if done {
        return events;
      }
    }
  }

  fn search(&mut self, query: &str) -> Vec<Value> {
    self.ok("library.search", json!({"query": query}))["tracks"].as_array().unwrap().clone()
  }

  fn add(&mut self, path: &Path) -> String {
    let uri = format!("file://{}", path.display());
    self.ok("queue.add", json!({"uris": [uri]}));
    let list = self.ok("queue.list", json!({}));
    list["tracks"].as_array().unwrap().last().unwrap()["id"].as_str().unwrap().to_string()
  }
}

fn titles(tracks: &[Value]) -> Vec<String> {
  let mut t: Vec<String> = tracks.iter().map(|t| t["uri"].as_str().unwrap().rsplit('/').next().unwrap().to_string()).collect();
  t.sort();
  t
}

#[test]
fn scan_reports_progress_and_search_matches_any_field() {
  let mut s = S::new();
  s.put("tagged_cover.mp3", "rock/red.mp3");
  s.put("vorbis.ogg", "misc/plain song.ogg");
  s.put("opus.ogg", "misc/deep/other.opus.ogg");
  fs::write(s.music().join("notes.txt"), "not audio").unwrap();
  fs::write(s.music().join("misc/broken.mp3"), "not really an mp3").unwrap();

  let events = s.scan(&s.music());
  let last = events.last().unwrap();
  // Candidatos por extension: los 3 buenos y el roto.
  assert_eq!(last["total"], 4);
  assert!(events.windows(2).all(|w| w[0]["done"].as_u64() <= w[1]["done"].as_u64()));

  let red = s.search("red");
  assert_eq!(red.len(), 1, "{red:?}");
  let t = &red[0];
  assert_eq!((t["title"].clone(), t["artist"].clone(), t["album"].clone()), (json!("Red Song"), json!("Test Band"), json!("Fixture Album")));
  assert_eq!(t["id"], Value::Null, "library results are not queue items");
  assert_eq!(t["source"], "local");
  assert_eq!(t["has_cover"], true);
  assert_eq!(t["codec"], "mp3");

  // Varias palabras: todas deben aparecer, en cualquier campo.
  assert_eq!(s.search("test ALBUM").len(), 1);
  assert!(s.search("test nothing").is_empty());
  // Sin titulo se busca por nombre de archivo.
  assert_eq!(titles(&s.search("plain")), ["plain%20song.ogg"]);
  // Consulta vacia: todo, ordenado; el roto no esta.
  assert_eq!(titles(&s.search("")), ["other.opus.ogg", "plain%20song.ogg", "red.mp3"]);
  let limited = s.ok("library.search", json!({"query": "", "limit": 2}));
  assert_eq!(limited["tracks"].as_array().unwrap().len(), 2);
}

#[test]
fn index_survives_restart_and_rescans_replace_their_folder() {
  let mut s = S::new();
  let gone = s.put("vorbis.ogg", "a/gone.ogg");
  s.put("opus.ogg", "b/kept.ogg");
  s.scan(&s.music());
  s.restart();
  assert_eq!(titles(&s.search("")), ["gone.ogg", "kept.ogg"]);

  fs::remove_file(gone).unwrap();
  s.put("opus.ogg", "a/new.ogg");
  s.scan(&s.music().join("a"));
  assert_eq!(titles(&s.search("")), ["kept.ogg", "new.ogg"]);
}

#[test]
fn scan_and_search_validate_params() {
  let mut s = S::new();
  let file = s.put("vorbis.ogg", "x.ogg");
  assert_eq!(s.code("library.scan", json!({})), -32602);
  assert_eq!(s.code("library.scan", json!({"path": "relative/dir"})), -32602);
  assert_eq!(s.code("library.scan", json!({"path": s.dir.join("missing")})), -32602);
  assert_eq!(s.code("library.scan", json!({"path": file})), -32602);
  assert_eq!(s.code("library.search", json!({})), -32602);
  assert_eq!(s.code("library.search", json!({"query": "x", "limit": 0})), -32602);
}

#[test]
fn cover_from_embedded_picture_or_folder_image() {
  let mut s = S::new();
  let tagged = s.put("tagged_cover.mp3", "a/red.mp3");
  let id = s.add(&tagged);
  let cover = s.ok("track.cover", json!({"track_id": id}));
  assert_eq!(cover["mime"], "image/png");
  let path = PathBuf::from(cover["path"].as_str().unwrap());
  assert!(path.starts_with(s.dir.join("ketai/covers")), "{}", path.display());
  assert_eq!(&fs::read(&path).unwrap()[..8], b"\x89PNG\r\n\x1a\n");
  assert_eq!(fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);

  // Sin imagen embebida: se usa la de la carpeta.
  let plain = s.put("vorbis.ogg", "b/plain.ogg");
  fs::write(s.music().join("b/Cover.JPG"), b"\xff\xd8\xff\xe0fake jpeg").unwrap();
  let id = s.add(&plain);
  let cover = s.ok("track.cover", json!({"track_id": id}));
  assert_eq!(cover["mime"], "image/jpeg");
  assert_eq!(fs::read(cover["path"].as_str().unwrap()).unwrap(), b"\xff\xd8\xff\xe0fake jpeg");
  let state = s.ok("queue.list", json!({}));
  assert_eq!(state["tracks"][1]["has_cover"], true);

  // Ninguna: null.
  let bare = s.put("opus.ogg", "c/bare.ogg");
  let id = s.add(&bare);
  assert_eq!(s.ok("track.cover", json!({"track_id": id})), Value::Null);
  assert_eq!(s.code("track.cover", json!({"track_id": "t_nope"})), -32602);
  assert_eq!(s.code("track.cover", json!({})), -32602);
}

#[test]
fn cache_info_and_clear() {
  let mut s = S::new();
  let info = s.ok("cache.info", json!({}));
  let cache = s.dir.join("cache/ketai");
  assert_eq!(info, json!({"path": cache, "size_bytes": 0, "entries": 0, "limit_bytes": 2147483648u64}));

  // Lo que haya en la cache (en la version local, solo lo que deje el
  // usuario o una version futura) se cuenta y se borra.
  fs::create_dir_all(cache.join("sub")).unwrap();
  fs::write(cache.join("a.bin"), vec![0u8; 1000]).unwrap();
  fs::write(cache.join("sub/b.bin"), vec![0u8; 24]).unwrap();
  let info = s.ok("cache.info", json!({}));
  assert_eq!((info["size_bytes"].clone(), info["entries"].clone()), (json!(1024), json!(2)));
  assert_eq!(s.ok("cache.clear", json!({})), json!({"freed_bytes": 1024}));
  assert_eq!(s.ok("cache.info", json!({}))["entries"], 0);
  assert!(cache.exists(), "the cache folder itself stays");
}

#[test]
fn cache_clear_never_follows_a_linked_cache_folder() {
  let mut s = S::new();
  let outside = s.dir.join("precious");
  fs::create_dir_all(&outside).unwrap();
  fs::write(outside.join("keep.txt"), b"keep").unwrap();
  fs::create_dir_all(s.dir.join("cache")).unwrap();
  std::os::unix::fs::symlink(&outside, s.dir.join("cache/ketai")).unwrap();
  assert_eq!(s.ok("cache.clear", json!({})), json!({"freed_bytes": 0}));
  assert!(outside.join("keep.txt").exists(), "files behind a linked cache folder must survive");
}

#[test]
fn a_planted_link_in_place_of_a_cover_is_not_followed() {
  let mut s = S::new();
  let tagged = s.put("tagged_cover.mp3", "a/red.mp3");
  let id = s.add(&tagged);
  let victim = s.dir.join("victim.txt");
  fs::write(&victim, b"original").unwrap();
  let covers = s.dir.join("ketai/covers");
  fs::create_dir_all(&covers).unwrap();
  std::os::unix::fs::symlink(&victim, covers.join(format!("{id}.png"))).unwrap();
  let _ = s.raw("track.cover", json!({"track_id": id}));
  assert_eq!(fs::read(&victim).unwrap(), b"original");
}

#[test]
fn cover_by_uri_for_tracks_outside_the_queue() {
  let mut s = S::new();
  let tagged = s.put("tagged_cover.mp3", "x/red song.mp3");
  let uri = format!("file://{}", tagged.display()).replace(' ', "%20");
  let cover = s.ok("track.cover", json!({"uri": uri}));
  assert_eq!(cover["mime"], "image/png");
  let path = PathBuf::from(cover["path"].as_str().unwrap());
  assert!(path.starts_with(s.dir.join("ketai/covers")), "{}", path.display());
  assert!(path.file_name().unwrap().to_str().unwrap().starts_with("u_"));
  assert_eq!(&fs::read(&path).unwrap()[..4], b"\x89PNG");
  // El mismo archivo siempre da la misma ruta.
  assert_eq!(s.ok("track.cover", json!({"uri": uri}))["path"], cover["path"]);
  // Sin caratula, inexistente o remoto: null.
  let plain = s.put("opus.ogg", "x/plain.ogg");
  assert_eq!(s.ok("track.cover", json!({"uri": format!("file://{}", plain.display())})), Value::Null);
  assert_eq!(s.ok("track.cover", json!({"uri": "file:///nope/missing.ogg"})), Value::Null);
  assert_eq!(s.ok("track.cover", json!({"uri": "https://example.com/a.ogg"})), Value::Null);
}

#[test]
fn library_folders_are_added_listed_removed_and_remembered() {
  let mut s = S::new();
  let a = s.music().join("a");
  let b = s.music().join("b");
  s.put("vorbis.ogg", "a/one.ogg");
  s.put("opus.ogg", "b/two.ogg");
  let mut w = s.daemon.as_ref().unwrap().connect();
  w.request(1, "events.subscribe", json!({"topics": ["library"]}));

  assert_eq!(s.ok("library.folders", json!({})), json!({"folders": []}));
  assert_eq!(s.ok("library.add_folder", json!({"path": a})), json!({}));
  assert_eq!(s.ok("library.add_folder", json!({"path": b})), json!({}));
  assert_eq!(s.ok("library.add_folder", json!({"path": a})), json!({}), "adding twice is harmless");
  let deadline = Instant::now() + Duration::from_secs(3);
  loop {
    let m = w.read_message().expect("event");
    if m["event"] == "library.changed" {
      break;
    }
    assert!(Instant::now() < deadline);
  }

  for bad in [json!({}), json!({"path": "relative"}), json!({"path": s.dir.join("missing")}), json!({"path": s.music().join("a/one.ogg")})] {
    assert_eq!(s.code("library.add_folder", bad), -32602);
  }

  // Escanear sin path recorre todas las carpetas.
  let events = s.scan_all();
  assert_eq!(events.last().unwrap()["total"], 2);
  assert_eq!(titles(&s.search("")), ["one.ogg", "two.ogg"]);
  let folders = s.ok("library.folders", json!({}));
  assert_eq!(folders, json!({"folders": [{"path": a, "tracks": 1}, {"path": b, "tracks": 1}]}));

  s.restart();
  assert_eq!(s.ok("library.folders", json!({}))["folders"].as_array().unwrap().len(), 2, "folders survive a restart");

  // Quitar una carpeta saca sus canciones del indice.
  assert_eq!(s.ok("library.remove_folder", json!({"path": a})), json!({}));
  assert_eq!(titles(&s.search("")), ["two.ogg"]);
  assert_eq!(s.code("library.remove_folder", json!({"path": a})), -32602, "not a library folder");
}

#[test]
fn scanning_everything_needs_at_least_one_folder() {
  let mut s = S::new();
  assert_eq!(s.code("library.scan", json!({})), -32602);
}

#[test]
fn search_ignores_accents_and_case_in_both_directions() {
  let mut s = S::new();
  s.put("vorbis.ogg", "Canción de niño.ogg");
  s.put("opus.ogg", "cafe tacuba.ogg");
  s.scan(&s.music());
  assert_eq!(titles(&s.search("cancion nino")), ["Canci%C3%B3n%20de%20ni%C3%B1o.ogg"]);
  assert_eq!(titles(&s.search("CANCIÓN")).len(), 1);
  assert_eq!(titles(&s.search("café")), ["cafe%20tacuba.ogg"]);
  assert_eq!(titles(&s.search("de ni")).len(), 1, "partial words anywhere");
}

#[test]
fn results_carry_genre_year_numbers_and_album_artist_and_search_them() {
  let mut s = S::new();
  s.put("full_tags.ogg", "jp/full.ogg");
  s.put("vorbis.ogg", "jp/plain.ogg");
  s.scan(&s.music());
  let hits = s.search("j-pop 2021");
  assert_eq!(hits.len(), 1, "{hits:?}");
  let t = &hits[0];
  assert_eq!(t["genre"], "J-Pop");
  assert_eq!(t["year"], 2021);
  assert_eq!((t["track_number"].clone(), t["disc_number"].clone()), (json!(3), json!(1)));
  assert_eq!(t["album_artist"], "Varios");
  assert_eq!(s.search("varios").len(), 1, "album artist is searchable");
}

#[test]
fn the_index_lives_in_a_database_and_imports_the_old_index_once() {
  let mut s = S::new();
  let song = s.put("vorbis.ogg", "old/song.ogg");
  let folder = s.music().join("old");
  // Indice de una version anterior (library.json), sin base todavia.
  let data = s.dir.join("data/ketai");
  s.daemon = None;
  for f in ["library.db", "library.db-wal", "library.db-shm"] {
    let _ = fs::remove_file(data.join(f));
  }
  fs::create_dir_all(&data).unwrap();
  let old = json!({"version": 1, "folders": [folder], "tracks": [{"path": song, "title": "From JSON", "artist": "Old", "album": null, "duration_ms": 2000, "has_cover": false, "codec": "vorbis", "sample_rate": 44100, "channels": 2}]});
  fs::write(data.join("library.json"), old.to_string()).unwrap();
  s.restart();
  let hits = s.search("json");
  assert_eq!(titles(&hits), ["song.ogg"]);
  assert_eq!(hits[0]["title"], "From JSON");
  assert_eq!(s.ok("library.folders", json!({}))["folders"][0]["path"], json!(folder));
  assert!(data.join("library.db").is_file());
  assert!(data.join("library.json").is_file(), "the old index is never deleted");
  // Se importa una sola vez: lo que cambie despues no se pisa.
  assert_eq!(s.ok("library.remove_folder", json!({"path": folder})), json!({}));
  s.restart();
  assert!(s.search("json").is_empty());
}

#[test]
fn two_clients_changing_folders_at_once_both_count() {
  let s = S::new();
  let dirs: Vec<PathBuf> = (0..8).map(|i| s.music().join(format!("f{i}"))).collect();
  for d in &dirs {
    fs::create_dir_all(d).unwrap();
  }
  let handles: Vec<_> = dirs
    .chunks(4)
    .map(|chunk| {
      let mut c = s.daemon.as_ref().unwrap().connect();
      let chunk = chunk.to_vec();
      std::thread::spawn(move || {
        for (i, d) in chunk.iter().enumerate() {
          let r = c.request(10 + i as u64, "library.add_folder", json!({"path": d}));
          assert_eq!(r["result"], json!({}), "{r}");
        }
      })
    })
    .collect();
  for h in handles {
    h.join().unwrap();
  }
  let mut s = s;
  let folders = s.ok("library.folders", json!({}));
  assert_eq!(folders["folders"].as_array().unwrap().len(), 8, "{folders}");
  s.restart();
  assert_eq!(s.ok("library.folders", json!({}))["folders"].as_array().unwrap().len(), 8);
}

#[test]
fn a_scan_cut_halfway_keeps_what_it_already_read() {
  let mut s = S::new();
  let first = s.put("vorbis.ogg", "many/0000.ogg");
  for i in 1..3000 {
    fs::hard_link(&first, s.music().join(format!("many/{i:04}.ogg"))).unwrap();
  }
  let mut w = s.daemon.as_ref().unwrap().connect();
  w.request(1, "events.subscribe", json!({"topics": ["library"]}));
  s.ok("library.scan", json!({"path": s.music()}));
  let deadline = Instant::now() + Duration::from_secs(20);
  loop {
    assert!(Instant::now() < deadline);
    let m = w.read_message().expect("event");
    let done = m["data"]["done"].as_u64().unwrap_or(0);
    if m["event"] == "library.progress" && done >= 1200 {
      assert!(done < 3000, "the scan finished before it could be cut; use more files");
      break;
    }
  }
  // Corte de golpe (SIGKILL), sin cerrar nada.
  let mut d = s.daemon.take().unwrap();
  d.child.kill().unwrap();
  d.child.wait().unwrap();
  drop(d);
  s.restart();
  let kept = s.ok("library.search", json!({"query": "", "limit": 5000}))["tracks"].as_array().unwrap().len();
  assert!(kept >= 200, "only {kept} tracks survived the cut");
}

#[test]
fn browse_groups_and_pages_the_whole_library() {
  let mut s = S::new();
  s.put("full_tags.ogg", "jp/a/full.ogg");
  s.put("tagged_cover.mp3", "rock/red.mp3");
  s.put("vorbis.ogg", "rock/plain.ogg");
  s.scan(&s.music());
  // Todas, por pagina, ordenadas por ruta.
  let all = s.ok("library.tracks", json!({"offset": 0, "limit": 2}));
  assert_eq!(all["total"], 3);
  assert_eq!(titles(all["tracks"].as_array().unwrap()), ["full.ogg", "plain.ogg"]);
  let rest = s.ok("library.tracks", json!({"offset": 2, "limit": 2}));
  assert_eq!(titles(rest["tracks"].as_array().unwrap()), ["red.mp3"]);
  // Grupos con su cantidad; sin dato queda en un grupo null.
  assert_eq!(
    s.ok("library.groups", json!({"by": "genre"})),
    json!({"groups": [{"name": null, "count": 2}, {"name": "J-Pop", "count": 1}]})
  );
  assert_eq!(s.ok("library.groups", json!({"by": "year"}))["groups"][1], json!({"name": "2021", "count": 1}));
  let folders = s.ok("library.groups", json!({"by": "folder"}));
  assert_eq!(folders["groups"], json!([{"name": s.music().join("jp/a"), "count": 1}, {"name": s.music().join("rock"), "count": 2}]));
  assert_eq!(s.ok("library.groups", json!({"by": "artist"}))["groups"].as_array().unwrap().len(), 3);
  assert_eq!(s.ok("library.groups", json!({"by": "album"}))["groups"].as_array().unwrap().len(), 3);
  // Canciones de un grupo.
  let rock = s.ok("library.tracks", json!({"by": "folder", "name": s.music().join("rock")}));
  assert_eq!(titles(rock["tracks"].as_array().unwrap()), ["plain.ogg", "red.mp3"]);
  let none = s.ok("library.tracks", json!({"by": "genre", "name": null}));
  assert_eq!(none["total"], 2);
  let band = s.ok("library.tracks", json!({"by": "artist", "name": "Test Band"}));
  assert_eq!(titles(band["tracks"].as_array().unwrap()), ["red.mp3"]);
  for bad in [json!({"by": "mood"}), json!({})] {
    assert_eq!(s.code("library.groups", bad), -32602);
  }
  assert_eq!(s.code("library.tracks", json!({"by": "nope", "name": "x"})), -32602);
  assert_eq!(s.code("library.tracks", json!({"by": "artist"})), -32602, "a group needs a name (null is allowed)");
}
