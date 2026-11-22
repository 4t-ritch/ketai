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

  /// Escanea y espera el ultimo `library.progress` (done == total).
  fn scan(&mut self, path: &Path) -> Vec<Value> {
    let mut w = self.daemon.as_ref().unwrap().connect();
    w.request(1, "events.subscribe", json!({"topics": ["library"]}));
    let job = self.ok("library.scan", json!({"path": path}))["job"].as_str().unwrap().to_string();
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
