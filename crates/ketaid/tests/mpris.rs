//! Conformidad MPRIS 2 (especificacion de freedesktop.org): el daemon se
//! publica como `org.mpris.MediaPlayer2.ketai` en el bus de sesion.
//!
//! Cada test levanta su propio `dbus-daemon`, asi nunca toca el bus del
//! usuario.

mod common;

use std::{
  io::{BufRead, BufReader},
  path::PathBuf,
  process::{Child, Command, Stdio},
  sync::{Arc, Mutex},
  thread,
  time::{Duration, Instant},
};

use common::{Client, Daemon, fixture_uri, temp_runtime_dir};
use dbus::{
  Message, Path,
  arg::{PropMap, prop_cast},
  blocking::{Connection, Proxy, stdintf::org_freedesktop_dbus::Properties},
  message::MatchRule,
};
use serde_json::{Value, json};

const NAME: &str = "org.mpris.MediaPlayer2.ketai";
const ROOT: &str = "org.mpris.MediaPlayer2";
const PLAYER: &str = "org.mpris.MediaPlayer2.Player";
const T: Duration = Duration::from_secs(3);

struct Bus {
  child: Child,
  address: String,
}

impl Bus {
  fn start(dir: &std::path::Path) -> Self {
    let address = format!("unix:path={}", dir.join("bus").display());
    let mut child = Command::new("dbus-daemon")
      .args(["--session", "--nofork", "--print-address", "--address"])
      .arg(&address)
      .stdout(Stdio::piped())
      .stderr(Stdio::null())
      .spawn()
      .expect("dbus-daemon");
    // La primera linea llega cuando el bus ya acepta conexiones.
    let mut line = String::new();
    BufReader::new(child.stdout.take().unwrap()).read_line(&mut line).unwrap();
    Self { child, address }
  }

  fn connect(&self) -> Connection {
    let mut ch = dbus::channel::Channel::open_private(&self.address).unwrap();
    ch.register().unwrap();
    Connection::from(ch)
  }
}

impl Drop for Bus {
  fn drop(&mut self) {
    let _ = self.child.kill();
    let _ = self.child.wait();
  }
}

struct S {
  // El orden importa al soltar: primero el daemon, despues el bus.
  daemon: Daemon,
  bus: Bus,
  dbus: Connection,
  c: Client,
  id: u64,
  dir: PathBuf,
}

impl Drop for S {
  fn drop(&mut self) {
    let _ = self.daemon.child.kill();
    let _ = self.daemon.child.wait();
    let _ = self.bus.child.kill();
    let _ = std::fs::remove_dir_all(&self.dir);
  }
}

impl S {
  fn new() -> Self {
    let dir = temp_runtime_dir();
    let bus = Bus::start(&dir);
    let addr = bus.address.clone();
    let daemon = Daemon::start_in(dir.clone(), &[("DBUS_SESSION_BUS_ADDRESS", &addr)]);
    let c = daemon.connect();
    let dbus = bus.connect();
    let s = Self { daemon, bus, dbus, c, id: 1, dir };
    // El nombre se publica en un thread aparte: esperar a que aparezca.
    let deadline = Instant::now() + T;
    while s.root::<String>("Identity").is_err() {
      assert!(Instant::now() < deadline, "{NAME} never appeared on the bus");
      thread::sleep(Duration::from_millis(20));
    }
    s
  }

  fn proxy(&self) -> Proxy<'_, &Connection> {
    self.dbus.with_proxy(NAME, "/org/mpris/MediaPlayer2", T)
  }

  fn root<R: for<'b> dbus::arg::Get<'b> + 'static>(&self, prop: &str) -> Result<R, dbus::Error> {
    self.proxy().get(ROOT, prop)
  }

  fn get<R: for<'b> dbus::arg::Get<'b> + 'static>(&self, prop: &str) -> R {
    self.proxy().get(PLAYER, prop).unwrap_or_else(|e| panic!("{prop}: {e}"))
  }

  fn call<A: dbus::arg::AppendAll>(&self, method: &str, args: A) {
    let _: () = self.proxy().method_call(PLAYER, method, args).unwrap_or_else(|e| panic!("{method}: {e}"));
  }

  fn ok(&mut self, method: &str, params: Value) -> Value {
    self.id += 1;
    let r = self.c.request(self.id, method, params);
    assert!(r.get("error").is_none(), "{method}: {r}");
    r["result"].clone()
  }

  fn state(&mut self) -> Value {
    self.ok("player.state", json!({}))
  }

  /// Espera a que una propiedad del Player cumpla la condicion.
  fn wait_prop<R: for<'b> dbus::arg::Get<'b> + 'static + std::fmt::Debug>(&self, prop: &str, ok: impl Fn(&R) -> bool) -> R {
    let deadline = Instant::now() + T;
    loop {
      let v: R = self.get(prop);
      if ok(&v) {
        return v;
      }
      assert!(Instant::now() < deadline, "{prop} stayed {v:?}");
      thread::sleep(Duration::from_millis(20));
    }
  }

  fn wait_state(&mut self, ok: impl Fn(&Value) -> bool) -> Value {
    let deadline = Instant::now() + T;
    loop {
      let st = self.state();
      if ok(&st) {
        return st;
      }
      assert!(Instant::now() < deadline, "state stayed {st}");
      thread::sleep(Duration::from_millis(20));
    }
  }

  fn queue(&mut self, names: &[&str]) -> Vec<String> {
    let uris: Vec<String> = names.iter().map(|n| fixture_uri(n)).collect();
    self.ok("queue.add", json!({"uris": uris}));
    let list = self.ok("queue.list", json!({}));
    list["tracks"].as_array().unwrap().iter().map(|t| t["id"].as_str().unwrap().to_string()).collect()
  }

  /// Guarda las senales que coinciden con la regla.
  fn record(&self, rule: MatchRule<'static>) -> Arc<Mutex<Vec<Message>>> {
    let got = Arc::new(Mutex::new(Vec::new()));
    let sink = got.clone();
    self
      .dbus
      .add_match(rule, move |(): (), _, msg: &Message| {
        sink.lock().unwrap().push(msg.duplicate().unwrap());
        true
      })
      .unwrap();
    got
  }

  /// Procesa el bus hasta que alguna senal guardada cumpla la condicion.
  fn wait_signal(&self, got: &Arc<Mutex<Vec<Message>>>, ok: impl Fn(&Message) -> bool) -> Message {
    let deadline = Instant::now() + T;
    loop {
      self.dbus.process(Duration::from_millis(50)).unwrap();
      if let Some(m) = got.lock().unwrap().iter().find(|m| ok(m)) {
        return m.duplicate().unwrap();
      }
      assert!(Instant::now() < deadline, "signal never arrived");
    }
  }
}

fn track_path(id: &str) -> String {
  format!("/org/ketai/track/{id}")
}

#[test]
fn root_and_player_properties_when_idle() {
  let s = S::new();
  assert_eq!(s.root::<String>("Identity").unwrap(), "ketai");
  assert!(!s.root::<bool>("CanQuit").unwrap());
  assert!(!s.root::<bool>("CanRaise").unwrap());
  assert!(!s.root::<bool>("HasTrackList").unwrap());
  assert_eq!(s.root::<Vec<String>>("SupportedUriSchemes").unwrap(), vec!["file".to_string()]);

  assert_eq!(s.get::<String>("PlaybackStatus"), "Stopped");
  assert_eq!(s.get::<String>("LoopStatus"), "None");
  assert!(!s.get::<bool>("Shuffle"));
  assert_eq!(s.get::<f64>("Rate"), 1.0);
  assert_eq!(s.get::<f64>("Volume"), 1.0);
  assert_eq!(s.get::<i64>("Position"), 0);
  assert!(s.get::<bool>("CanControl"));
  assert!(!s.get::<bool>("CanPlay"));
  assert!(!s.get::<bool>("CanSeek"));
  let md: PropMap = s.get("Metadata");
  assert_eq!(prop_cast::<Path>(&md, "mpris:trackid").map(|p| p.to_string()), Some("/org/mpris/MediaPlayer2/TrackList/NoTrack".into()));
}

#[test]
fn metadata_and_status_follow_the_player() {
  let mut s = S::new();
  let changed = s.record(MatchRule::new_signal("org.freedesktop.DBus.Properties", "PropertiesChanged"));
  let ids = s.queue(&["vorbis.ogg", "opus.ogg"]);
  s.ok("player.play", json!({}));

  s.wait_prop::<String>("PlaybackStatus", |v| v == "Playing");
  let md: PropMap = s.get("Metadata");
  assert_eq!(prop_cast::<Path>(&md, "mpris:trackid").map(|p| p.to_string()), Some(track_path(&ids[0])));
  assert_eq!(prop_cast::<String>(&md, "xesam:url"), Some(&fixture_uri("vorbis.ogg")));
  let length = *prop_cast::<i64>(&md, "mpris:length").expect("mpris:length");
  assert!((1_950_000..2_050_000).contains(&length), "length {length}");
  assert!(s.get::<bool>("CanPlay") && s.get::<bool>("CanPause") && s.get::<bool>("CanSeek"));
  assert!(s.get::<bool>("CanGoNext"));
  let pos = s.wait_prop::<i64>("Position", |p| *p >= 200_000);
  assert!(pos < 2_000_000);

  // PropertiesChanged anuncia el cambio de estado, con la interfaz Player.
  s.wait_signal(&changed, |m| {
    let (iface, props): (String, PropMap) = m.read2().unwrap();
    iface == PLAYER && prop_cast::<String>(&props, "PlaybackStatus").is_some_and(|v| v == "Playing")
  });
}

#[test]
fn dbus_controls_drive_the_player() {
  let mut s = S::new();
  let ids = s.queue(&["vorbis.ogg", "opus.ogg"]);
  s.call("Play", ());
  s.wait_state(|st| st["status"] == "playing");
  s.call("PlayPause", ());
  s.wait_state(|st| st["status"] == "paused");
  s.call("Next", ());
  s.wait_state(|st| st["track"]["id"] == ids[1]);
  s.call("Previous", ());
  s.wait_state(|st| st["track"]["id"] == ids[0]);

  s.proxy().set(PLAYER, "Volume", 0.3f64).unwrap();
  s.wait_state(|st| st["volume"] == 0.3);
  s.proxy().set(PLAYER, "LoopStatus", "Playlist".to_string()).unwrap();
  s.wait_state(|st| st["repeat"] == "queue");
  s.proxy().set(PLAYER, "LoopStatus", "Track".to_string()).unwrap();
  s.wait_state(|st| st["repeat"] == "track");
  s.proxy().set(PLAYER, "Shuffle", true).unwrap();
  s.wait_state(|st| st["shuffle"] == true);
  assert_eq!(s.wait_prop::<String>("LoopStatus", |v| v == "Track"), "Track");

  s.call("Stop", ());
  s.wait_state(|st| st["status"] == "stopped");
  assert_eq!(s.wait_prop::<String>("PlaybackStatus", |v| v == "Stopped"), "Stopped");
}

#[test]
fn seek_and_set_position_emit_seeked() {
  let mut s = S::new();
  let seeked = s.record(MatchRule::new_signal(PLAYER, "Seeked"));
  let ids = s.queue(&["vorbis.ogg"]);
  s.ok("player.play", json!({}));
  s.ok("player.pause", json!({}));
  s.ok("player.seek", json!({"position_ms": 200}));

  s.call("Seek", (500_000i64,));
  let st = s.wait_state(|st| (650..850).contains(&st["position_ms"].as_u64().unwrap()));
  assert_eq!(st["status"], "paused");
  s.wait_signal(&seeked, |m| (650_000..850_000).contains(&m.read1::<i64>().unwrap()));

  let track = Path::from(track_path(&ids[0]));
  s.call("SetPosition", (track, 1_200_000i64));
  s.wait_state(|st| (1150..1300).contains(&st["position_ms"].as_u64().unwrap()));
  s.wait_signal(&seeked, |m| (1_150_000..1_300_000).contains(&m.read1::<i64>().unwrap()));

  // Una pista que no es la actual se ignora, como pide la especificacion.
  s.call("SetPosition", (Path::from("/org/ketai/track/t_nope"), 100_000i64));
  thread::sleep(Duration::from_millis(200));
  let pos = s.state()["position_ms"].as_u64().unwrap();
  assert!((1150..1300).contains(&pos), "stale SetPosition moved to {pos}");
}

#[test]
fn daemon_without_a_session_bus_still_serves_clients() {
  // Sin bus de sesion (el entorno por defecto de los tests) el daemon sigue
  // funcionando: MPRIS es opcional.
  let daemon = Daemon::start();
  let mut c = daemon.connect();
  let r = c.request(1, "player.state", json!({}));
  assert_eq!(r["result"]["status"], "stopped");
}

#[test]
fn a_queue_without_a_current_track_can_be_played() {
  // playerctl no manda Play si CanPlay es false.
  let mut s = S::new();
  s.queue(&["vorbis.ogg"]);
  assert!(s.wait_prop::<bool>("CanPlay", |v| *v));
  assert!(s.get::<bool>("CanPause"));
  s.call("Play", ());
  s.wait_state(|st| st["status"] == "playing");
}

#[test]
fn written_properties_read_back_immediately() {
  // Dos comandos seguidos (`playerctl volume 0.5; playerctl volume`) deben
  // ver el valor nuevo.
  let s = S::new();
  s.proxy().set(PLAYER, "Volume", 0.5f64).unwrap();
  assert_eq!(s.get::<f64>("Volume"), 0.5);
  s.proxy().set(PLAYER, "LoopStatus", "Playlist".to_string()).unwrap();
  assert_eq!(s.get::<String>("LoopStatus"), "Playlist");
  s.proxy().set(PLAYER, "Shuffle", true).unwrap();
  assert!(s.get::<bool>("Shuffle"));
}
