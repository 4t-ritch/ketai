//! Conformidad de letras: `lyrics.layers`, `lyrics.get`, `lyrics.set_offset`
//! y el evento `lyrics.changed`.

mod common;

use std::{
  fs,
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

impl S {
  /// Daemon con una carpeta de musica propia dentro del directorio del test.
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

  /// Copia un fixture a la carpeta de musica con otro nombre.
  fn track(&self, fixture: &str, name: &str) -> PathBuf {
    let target = self.music().join(name);
    fs::copy(fixture_uri(fixture).trim_start_matches("file://"), &target).unwrap();
    target
  }

  fn write(&self, name: &str, content: &str) {
    fs::write(self.music().join(name), content).unwrap();
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

  /// Agrega la pista a la cola y devuelve su track_id.
  fn add(&mut self, path: &Path) -> String {
    let uri = format!("file://{}", path.display()).replace(' ', "%20");
    self.ok("queue.add", json!({"uris": [uri]}));
    let list = self.ok("queue.list", json!({}));
    list["tracks"].as_array().unwrap().last().unwrap()["id"].as_str().unwrap().to_string()
  }
}

impl Drop for S {
  fn drop(&mut self) {
    self.daemon = None;
    let _ = fs::remove_dir_all(&self.dir);
  }
}

const ORIGINAL_LRC: &str = "[00:01.00]<00:01.00>kimi <00:01.50>no<00:02.00>\n[00:03.00]koe ga\n";

#[test]
fn layers_lists_every_layer_with_its_properties() {
  let mut s = S::new();
  let audio = s.track("vorbis.ogg", "song one.ogg");
  s.write("song one.lrc", ORIGINAL_LRC);
  s.write("song one.es.lrc", "[00:01.00]tu\n[00:03.00]voz\n");
  s.write("song one.ja-Latn.txt", "kimi no\nkoe ga\n");
  let id = s.add(&audio);
  let layers = s.ok("lyrics.layers", json!({"track_id": id}));
  assert_eq!(
    layers["layers"],
    json!([
      {"id": "original", "kind": "original", "language": null, "timing": "word", "format": "lrc", "origin": "sidecar"},
      {"id": "es", "kind": "translation", "language": "es", "timing": "line", "format": "lrc", "origin": "sidecar"},
      {"id": "ja-Latn", "kind": "romanized", "language": "ja-Latn", "timing": "line", "format": "text", "origin": "sidecar"},
    ])
  );
}

#[test]
fn get_defaults_to_the_original() {
  let mut s = S::new();
  let audio = s.track("vorbis.ogg", "a.ogg");
  s.write("a.lrc", ORIGINAL_LRC);
  let id = s.add(&audio);
  let p = s.ok("lyrics.get", json!({"track_id": id}));
  assert_eq!(p["primary"], "original");
  assert_eq!(p["secondary"], json!([]));
  assert_eq!(p["timing"], "word");
  assert_eq!((p["offset_ms"].clone(), p["user_offset_ms"].clone()), (json!(0), json!(0)));
  let l0 = &p["lines"][0];
  assert_eq!((l0["start_ms"].clone(), l0["end_ms"].clone(), l0["text"].clone()), (json!(1000), json!(2000), json!("kimi no")), "the A2 closing tag ends the line");
  assert_eq!(l0["words"][0], json!({"start_ms": 1000, "end_ms": 1500, "text": "kimi "}));
  assert_eq!((l0["agent"].clone(), l0["background"].clone(), l0["secondary"].clone()), (Value::Null, json!(false), json!({})));
  assert_eq!(p["lines"][1]["words"], Value::Null);
}

#[test]
fn romanized_primary_with_translation_secondary() {
  let mut s = S::new();
  let audio = s.track("vorbis.ogg", "a.ogg");
  s.write("a.lrc", ORIGINAL_LRC);
  s.write("a.es.lrc", "[00:01.10]tu\n[00:03.00]voz\n");
  s.write("a.ja-Latn.txt", "kimi no\nkoe ga\n");
  let id = s.add(&audio);
  let p = s.ok("lyrics.get", json!({"track_id": id, "primary": "ja-Latn", "secondary": ["es"]}));
  assert_eq!(p["timing"], "line");
  assert_eq!(p["lines"][0]["text"], "kimi no");
  assert_eq!(p["lines"][0]["words"], Value::Null, "original words are never projected");
  assert_eq!(p["lines"][0]["secondary"], json!({"es": "tu"}));
  assert_eq!(p["lines"][1]["secondary"], json!({"es": "voz"}));

  let p = s.ok("lyrics.get", json!({"track_id": id, "primary": "ja-Latn", "estimate_words": true}));
  assert_eq!(p["timing"], "word-estimated");
  assert_eq!(p["lines"][0]["words"].as_array().unwrap().len(), 2);
}

#[test]
fn get_validates_layers_and_track() {
  let mut s = S::new();
  let audio = s.track("vorbis.ogg", "a.ogg");
  s.write("a.lrc", ORIGINAL_LRC);
  let id = s.add(&audio);
  assert_eq!(s.code("lyrics.get", json!({"track_id": id, "primary": "zz"})), 1007);
  assert_eq!(s.code("lyrics.get", json!({"track_id": id, "secondary": ["zz"]})), 1007);
  assert_eq!(s.code("lyrics.get", json!({"track_id": id, "secondary": ["original"]})), -32602);
  assert_eq!(s.code("lyrics.get", json!({"track_id": "t_nope"})), -32602);
  assert_eq!(s.code("lyrics.layers", json!({})), -32602);
}

#[test]
fn track_without_lyrics() {
  let mut s = S::new();
  let audio = s.track("opus.ogg", "plain.ogg");
  let id = s.add(&audio);
  assert_eq!(s.ok("lyrics.layers", json!({"track_id": id})), json!({"layers": []}));
  assert_eq!(s.ok("lyrics.get", json!({"track_id": id})), Value::Null);
}

#[test]
fn only_broken_sources_report_1006() {
  let mut s = S::new();
  let audio = s.track("opus.ogg", "b.ogg");
  s.write("b.ttml", "<tt><p>broken");
  let id = s.add(&audio);
  assert_eq!(s.code("lyrics.layers", json!({"track_id": id})), 1006);
  assert_eq!(s.code("lyrics.get", json!({"track_id": id})), 1006);
}

#[test]
fn embedded_vorbis_lyrics() {
  let mut s = S::new();
  let audio = s.track("vorbis_lyrics.ogg", "e.ogg");
  let id = s.add(&audio);
  let layers = s.ok("lyrics.layers", json!({"track_id": id}));
  let ids: Vec<&str> = layers["layers"].as_array().unwrap().iter().map(|l| l["id"].as_str().unwrap()).collect();
  assert_eq!(ids, ["original", "es", "romanized"]);
  assert!(layers["layers"].as_array().unwrap().iter().all(|l| l["origin"] == "embedded"));
  let p = s.ok("lyrics.get", json!({"track_id": id, "secondary": ["es", "romanized"]}));
  assert_eq!(p["lines"][1]["text"], "world");
  assert_eq!(p["lines"][1]["secondary"], json!({"es": "mundo", "romanized": "waarudo"}));
}

#[test]
fn user_offset_applies_persists_and_emits_lyrics_changed() {
  let mut s = S::new();
  let audio = s.track("vorbis.ogg", "o.ogg");
  s.write("o.lrc", "[offset:+100]\n[00:02.00]line\n");
  let id = s.add(&audio);
  let mut w = s.daemon.as_ref().unwrap().connect();
  w.request(1, "events.subscribe", json!({"topics": ["lyrics"]}));

  assert_eq!(s.ok("lyrics.set_offset", json!({"track_id": id, "offset_ms": -250})), json!({}));
  let p = s.ok("lyrics.get", json!({"track_id": id}));
  assert_eq!((p["offset_ms"].clone(), p["user_offset_ms"].clone()), (json!(100), json!(-250)));
  // 2000 - 100 - (-250)
  assert_eq!(p["lines"][0]["start_ms"], 2150);

  let deadline = Instant::now() + Duration::from_secs(2);
  loop {
    let m = w.read_message().expect("event");
    if m["event"] == "lyrics.changed" {
      assert_eq!(m["data"], json!({"track_id": id}));
      break;
    }
    assert!(Instant::now() < deadline);
  }
  drop(w);

  assert_eq!(s.code("lyrics.set_offset", json!({"track_id": id})), -32602);

  s.restart();
  let id = s.add(&audio);
  let p = s.ok("lyrics.get", json!({"track_id": id}));
  assert_eq!(p["user_offset_ms"], -250, "offset is remembered per file");
}

#[test]
fn ttml_agents_and_background_lines() {
  let mut s = S::new();
  let audio = s.track("vorbis.ogg", "t.ogg");
  s.write("t.ttml", r#"<tt xmlns:ttm="http://www.w3.org/ns/ttml#metadata" xml:lang="ko"><head><metadata>
    <ttm:agent xml:id="v1"><ttm:name>Lead</ttm:name></ttm:agent></metadata></head>
    <body><div><p begin="1.0" end="2.0" ttm:agent="v1"><span begin="1.0" end="2.0">hi</span>
    <span ttm:role="x-bg"><span begin="1.5" end="1.9">(oh)</span></span></p></div></body></tt>"#);
  let id = s.add(&audio);
  let p = s.ok("lyrics.get", json!({"track_id": id}));
  assert_eq!(p["agents"], json!([{"id": "v1", "name": "Lead"}]));
  assert_eq!(p["lines"][0]["agent"], "v1");
  assert_eq!(p["lines"][1]["background"], true);
  assert_eq!(p["lines"][1]["text"], "(oh)");
  let layers = s.ok("lyrics.layers", json!({"track_id": id}));
  assert_eq!(layers["layers"][0]["language"], "ko");
}
