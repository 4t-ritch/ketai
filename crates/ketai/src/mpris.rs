//! MPRIS 2 por D-Bus: lo que usan playerctl, waybar y las teclas multimedia
//! del escritorio para manejar reproductores.
//!
//! Un thread propio es el dueno de la conexion D-Bus (libdbus, sin async).
//! No toca el estado del reproductor directamente:
//!
//! - Las propiedades se leen de una copia (`Snapshot`) que el loop central
//!   le manda despues de cada cambio.
//! - Los metodos y las propiedades escribibles se convierten en peticiones
//!   del protocolo (`player.toggle`, `player.seek`...) y viajan al loop
//!   central como `Message::Mpris`, igual que si las mandara un cliente.
//!
//! Si no hay bus de sesion, el thread lo registra y termina: el daemon sigue
//! funcionando sin MPRIS.

use std::{
  cell::RefCell,
  rc::Rc,
  sync::Arc,
  sync::mpsc::{self, Receiver, Sender, TryRecvError},
  thread,
  time::{Duration, Instant},
};

use dbus::{
  Message as DbusMessage, Path,
  arg::{PropMap, RefArg, Variant},
  blocking::LocalConnection,
  channel::{MatchingReceiver, Sender as _},
  message::MatchRule,
};
use dbus_crossroads::{Crossroads, IfaceBuilder, MethodErr};
use ketai_protocol::Request;
use serde_json::{Map, Value, json};

use crate::server::{Credit, Message};

const NAME: &str = "org.mpris.MediaPlayer2.ketai";
const OBJECT: &str = "/org/mpris/MediaPlayer2";
const ROOT: &str = "org.mpris.MediaPlayer2";
const PLAYER: &str = "org.mpris.MediaPlayer2.Player";
const NO_TRACK: &str = "/org/mpris/MediaPlayer2/TrackList/NoTrack";
/// Cada cuanto se revisan los cambios que manda el loop central.
const POLL: Duration = Duration::from_millis(50);

/// Copia del estado del reproductor para responder propiedades.
#[derive(Clone, Default, PartialEq)]
struct Snapshot {
  status: String,
  track: Option<Track>,
  position_ms: u64,
  volume: f64,
  muted: bool,
  repeat: String,
  shuffle: bool,
  can_next: bool,
  can_previous: bool,
  /// Hay algo en la cola: play arranca aunque no haya pista seleccionada.
  can_play: bool,
}

#[derive(Clone, Default, PartialEq)]
struct Track {
  id: String,
  uri: String,
  title: Option<String>,
  artist: Option<String>,
  album: Option<String>,
  duration_ms: Option<u64>,
}

impl Snapshot {
  fn from_state(state: &Value, queue_len: usize, current: Option<usize>) -> Self {
    let track = state["track"].as_object().map(|t| Track {
      id: t.get("id").and_then(Value::as_str).unwrap_or_default().to_string(),
      uri: t.get("uri").and_then(Value::as_str).unwrap_or_default().to_string(),
      title: t.get("title").and_then(Value::as_str).map(str::to_string),
      artist: t.get("artist").and_then(Value::as_str).map(str::to_string),
      album: t.get("album").and_then(Value::as_str).map(str::to_string),
      duration_ms: t.get("duration_ms").and_then(Value::as_u64),
    });
    let repeat = state["repeat"].as_str().unwrap_or("off").to_string();
    let has_track = track.is_some();
    Self {
      status: state["status"].as_str().unwrap_or("stopped").to_string(),
      position_ms: state["position_ms"].as_u64().unwrap_or(0),
      volume: state["volume"].as_f64().unwrap_or(1.0),
      muted: state["muted"].as_bool().unwrap_or(false),
      shuffle: state["shuffle"].as_bool().unwrap_or(false),
      can_next: has_track && (repeat != "off" || current.is_some_and(|c| c + 1 < queue_len)),
      can_previous: has_track,
      can_play: queue_len > 0,
      repeat,
      track,
    }
  }

  fn playback_status(&self) -> &'static str {
    match self.status.as_str() {
      "playing" => "Playing",
      "paused" => "Paused",
      _ => "Stopped",
    }
  }

  fn loop_status(&self) -> &'static str {
    match self.repeat.as_str() {
      "track" => "Track",
      "queue" => "Playlist",
      _ => "None",
    }
  }

  fn volume(&self) -> f64 {
    if self.muted { 0.0 } else { self.volume }
  }

  fn track_path(&self) -> Path<'static> {
    match &self.track {
      Some(t) => track_path(&t.id),
      None => Path::from(NO_TRACK),
    }
  }

  fn can_seek(&self) -> bool {
    self.track.as_ref().is_some_and(|t| t.duration_ms.is_some())
  }

  fn metadata(&self) -> PropMap {
    let mut md = PropMap::new();
    md.insert("mpris:trackid".into(), Variant(Box::new(self.track_path())));
    let Some(t) = &self.track else { return md };
    md.insert("xesam:url".into(), Variant(Box::new(t.uri.clone())));
    if let Some(d) = t.duration_ms {
      md.insert("mpris:length".into(), Variant(Box::new(d as i64 * 1000)));
    }
    if let Some(v) = &t.title {
      md.insert("xesam:title".into(), Variant(Box::new(v.clone())));
    }
    if let Some(v) = &t.artist {
      md.insert("xesam:artist".into(), Variant(Box::new(vec![v.clone()])));
    }
    if let Some(v) = &t.album {
      md.insert("xesam:album".into(), Variant(Box::new(v.clone())));
    }
    md
  }

  /// Propiedades del Player que cambiaron, para `PropertiesChanged`.
  /// `Position` nunca se anuncia (la especificacion usa `Seeked`).
  fn changes(&self, old: &Snapshot) -> PropMap {
    let mut out = PropMap::new();
    let mut put = |k: &str, v: Box<dyn RefArg>| {
      out.insert(k.to_string(), Variant(v));
    };
    if self.playback_status() != old.playback_status() {
      put("PlaybackStatus", Box::new(self.playback_status().to_string()));
    }
    if self.loop_status() != old.loop_status() {
      put("LoopStatus", Box::new(self.loop_status().to_string()));
    }
    if self.shuffle != old.shuffle {
      put("Shuffle", Box::new(self.shuffle));
    }
    if self.volume() != old.volume() {
      put("Volume", Box::new(self.volume()));
    }
    if self.track != old.track {
      put("Metadata", Box::new(self.metadata()));
      put("CanSeek", Box::new(self.can_seek()));
    }
    if self.can_play != old.can_play {
      put("CanPlay", Box::new(self.can_play));
      put("CanPause", Box::new(self.can_play));
    }
    if self.can_next != old.can_next {
      put("CanGoNext", Box::new(self.can_next));
    }
    if self.can_previous != old.can_previous {
      put("CanGoPrevious", Box::new(self.can_previous));
    }
    out
  }
}

/// Ruta D-Bus de una pista. Los ids (`t_<hex>`) solo usan caracteres validos.
fn track_path(id: &str) -> Path<'static> {
  Path::new(format!("/org/ketai/track/{id}")).unwrap_or_else(|_| Path::from(NO_TRACK))
}

/// Lo que el loop central le manda al thread de MPRIS.
struct Update {
  snapshot: Snapshot,
  /// Hubo un seek: se emite `Seeked` con la posicion nueva.
  seeked: bool,
}

/// Lado del loop central.
pub struct MprisService {
  updates: Sender<Update>,
  last: Option<Snapshot>,
  seeked: bool,
  /// Pedidos de MPRIS en vuelo hacia el loop central, con el mismo tope que
  /// un cliente: si un programa inunda el bus de llamadas, el thread de
  /// MPRIS espera en vez de llenar la cola del loop central.
  credit: Arc<Credit>,
}

impl MprisService {
  pub fn new(tx: Sender<Message>) -> Self {
    let (updates, rx) = mpsc::channel();
    let credit = Arc::new(Credit::default());
    let thread_credit = credit.clone();
    thread::Builder::new()
      .name("ketai-mpris".into())
      .spawn(move || {
        if let Err(e) = run(rx, tx, thread_credit) {
          eprintln!("ketaid: MPRIS disabled: {e}");
        }
      })
      .expect("spawn mpris thread");
    Self { updates, last: None, seeked: false, credit }
  }

  /// El loop central termino de atender un pedido de MPRIS.
  pub fn handled(&self) {
    self.credit.release();
  }

  /// Hubo un seek (de cualquier cliente o de MPRIS).
  pub fn note_seeked(&mut self) {
    self.seeked = true;
  }

  /// Manda el estado nuevo si cambio algo. Se llama despues de cada mensaje.
  pub fn update(&mut self, state: &Value, queue_len: usize, current: Option<usize>) {
    let snapshot = Snapshot::from_state(state, queue_len, current);
    let seeked = std::mem::take(&mut self.seeked);
    if !seeked && self.last.as_ref() == Some(&snapshot) {
      return;
    }
    self.last = Some(snapshot.clone());
    // Si el thread termino (sin bus) no hay a quien avisar.
    let _ = self.updates.send(Update { snapshot, seeked });
  }

  /// Convierte un pedido de MPRIS en una peticion del protocolo.
  pub fn request(method: &str, params: Value) -> Request {
    let params = match params {
      Value::Object(m) => m,
      _ => Map::new(),
    };
    Request { id: 0, method: method.to_string(), params }
  }
}

/// Estado del objeto D-Bus: la copia del reproductor y el canal al loop.
/// Valores escritos por MPRIS que el daemon todavia no confirmo. Un estado
/// que llega despues pero es de un cambio anterior (por ejemplo, el de un
/// volumen escrito antes) no debe deshacerlos: `playerctl loop Playlist;
/// playerctl loop` tiene que ver "Playlist". Se olvidan al confirmarse o a
/// los 2 segundos (si el daemon lo rechazo, manda el estado real).
#[derive(Default)]
struct Pending {
  repeat: Option<(String, Instant)>,
  shuffle: Option<(bool, Instant)>,
  volume: Option<(f64, Instant)>,
  muted: Option<(bool, Instant)>,
}

const PENDING_FOR: Duration = Duration::from_secs(2);

impl Pending {
  /// Ajusta un estado recien llegado con lo escrito sin confirmar.
  fn apply(&mut self, s: &mut Snapshot, now: Instant) {
    fn keep<T: PartialEq + Clone>(slot: &mut Option<(T, Instant)>, field: &mut T, now: Instant) {
      if let Some((v, at)) = slot.clone() {
        if *field == v || now.duration_since(at) >= PENDING_FOR {
          *slot = None;
        } else {
          *field = v;
        }
      }
    }
    keep(&mut self.repeat, &mut s.repeat, now);
    keep(&mut self.shuffle, &mut s.shuffle, now);
    keep(&mut self.volume, &mut s.volume, now);
    keep(&mut self.muted, &mut s.muted, now);
  }
}

struct Data {
  /// Escrituras sin confirmar (ver `Pending`).
  pending: Pending,
  /// Lo que responden las propiedades. Las escrituras lo actualizan al
  /// instante (para que una lectura inmediata vea el valor nuevo) hasta que
  /// llega el estado real del loop central.
  snapshot: Snapshot,
  /// Ultimo estado real recibido: contra esto se calcula PropertiesChanged.
  announced: Snapshot,
  /// Momento en que se recibio `snapshot`, para estimar la posicion.
  at: Instant,
  tx: Sender<Message>,
  credit: Arc<Credit>,
}

impl Data {
  fn send(&self, method: &str, params: Value) {
    // Espera lugar si ya hay demasiados pedidos sin atender.
    self.credit.acquire();
    if self.tx.send(Message::Mpris(MprisService::request(method, params))).is_err() {
      self.credit.release();
    }
  }

  /// Posicion actual en microsegundos: la ultima conocida mas lo que paso
  /// desde entonces si esta sonando.
  fn position_us(&self) -> i64 {
    let mut ms = self.snapshot.position_ms;
    if self.snapshot.status == "playing" {
      ms += self.at.elapsed().as_millis() as u64;
    }
    if let Some(d) = self.snapshot.track.as_ref().and_then(|t| t.duration_ms) {
      ms = ms.min(d);
    }
    ms as i64 * 1000
  }
}

fn run(updates: Receiver<Update>, tx: Sender<Message>, credit: Arc<Credit>) -> Result<(), dbus::Error> {
  let conn = LocalConnection::new_session()?;
  // Si el nombre ya esta tomado (otro ketai en el mismo bus), se usa uno
  // con el pid, como pide la especificacion.
  let reply = conn.request_name(NAME, false, false, true)?;
  if reply != dbus::blocking::stdintf::org_freedesktop_dbus::RequestNameReply::PrimaryOwner {
    conn.request_name(format!("{NAME}.instance{}", std::process::id()), false, false, true)?;
  }

  let mut cr = Crossroads::new();
  let root = cr.register(ROOT, root_iface);
  let player = cr.register(PLAYER, player_iface);
  cr.insert(OBJECT, &[root, player], Data { pending: Pending::default(), snapshot: Snapshot::default(), announced: Snapshot::default(), at: Instant::now(), tx, credit });
  let cr = Rc::new(RefCell::new(cr));

  let handler = cr.clone();
  conn.start_receive(
    MatchRule::new_method_call(),
    Box::new(move |msg, conn| {
      let _ = handler.borrow_mut().handle_message(msg, conn);
      true
    }),
  );

  loop {
    conn.process(POLL)?;
    loop {
      let update = match updates.try_recv() {
        Ok(u) => u,
        Err(TryRecvError::Empty) => break,
        // El daemon se esta cerrando.
        Err(TryRecvError::Disconnected) => return Ok(()),
      };
      let mut cr = cr.borrow_mut();
      let Some(data) = cr.data_mut::<Data>(&Path::from(OBJECT)) else { continue };
      let changes = update.snapshot.changes(&data.announced);
      data.announced = update.snapshot.clone();
      let mut snapshot = update.snapshot;
      data.pending.apply(&mut snapshot, Instant::now());
      data.snapshot = snapshot;
      data.at = Instant::now();
      if !changes.is_empty() {
        let msg = DbusMessage::new_signal(OBJECT, "org.freedesktop.DBus.Properties", "PropertiesChanged")
          .expect("valid signal")
          .append3(PLAYER, changes, Vec::<String>::new());
        let _ = conn.send(msg);
      }
      if update.seeked {
        let msg = DbusMessage::new_signal(OBJECT, PLAYER, "Seeked").expect("valid signal").append1(data.position_us());
        let _ = conn.send(msg);
      }
    }
  }
}

fn root_iface(b: &mut IfaceBuilder<Data>) {
  b.property("Identity").get(|_, _| Ok("ketai".to_string()));
  b.property("CanQuit").get(|_, _| Ok(false));
  b.property("CanRaise").get(|_, _| Ok(false));
  b.property("HasTrackList").get(|_, _| Ok(false));
  b.property("SupportedUriSchemes").get(|_, _| Ok(vec!["file".to_string()]));
  b.property("SupportedMimeTypes").get(|_, _| {
    Ok(
      ["audio/flac", "audio/mpeg", "audio/mp4", "audio/ogg", "audio/opus", "audio/wav"]
        .map(str::to_string)
        .to_vec(),
    )
  });
  b.method("Raise", (), (), |_, _, ()| Ok(()));
  b.method("Quit", (), (), |_, _, ()| Err::<(), _>(MethodErr::failed("ketai does not quit from MPRIS")));
}

fn player_iface(b: &mut IfaceBuilder<Data>) {
  for (name, method) in [
    ("Play", "player.play"),
    ("Pause", "player.pause"),
    ("PlayPause", "player.toggle"),
    ("Stop", "player.stop"),
    ("Next", "player.next"),
    ("Previous", "player.previous"),
  ] {
    b.method(name, (), (), move |_, d: &mut Data, ()| {
      d.send(method, json!({}));
      Ok(())
    });
  }
  b.method("Seek", ("Offset",), (), |_, d: &mut Data, (offset_us,): (i64,)| {
    // La especificacion pide que pasar el final se comporte como Next (que
    // no hace nada si no hay siguiente). Antes del principio se queda en 0.
    let len_us = d.snapshot.track.as_ref().and_then(|t| t.duration_ms).map(|ms| ms as i64 * 1000);
    if len_us.is_some_and(|len| d.position_us().saturating_add(offset_us) > len) {
      if d.snapshot.can_next {
        d.send("player.next", json!({}));
      }
    } else {
      d.send("player.seek_relative", json!({"offset_ms": offset_us / 1000}));
    }
    Ok(())
  });
  b.method("SetPosition", ("TrackId", "Position"), (), |_, d: &mut Data, (track, pos_us): (Path<'static>, i64)| {
    // La especificacion pide ignorar pedidos para otra pista o fuera de
    // rango.
    let len_us = d.snapshot.track.as_ref().and_then(|t| t.duration_ms).map(|ms| ms as i64 * 1000);
    if track == d.snapshot.track_path() && pos_us >= 0 && len_us.is_none_or(|l| pos_us <= l) {
      d.send("player.seek", json!({"position_ms": pos_us / 1000}));
    }
    Ok(())
  });
  b.method("OpenUri", ("Uri",), (), |_, d: &mut Data, (uri,): (String,)| {
    if !uri.starts_with("file://") {
      return Err(MethodErr::invalid_arg("only file:// URIs are supported"));
    }
    d.send("queue.add", json!({"uris": [uri]}));
    Ok(())
  });

  b.property("PlaybackStatus").get(|_, d: &mut Data| Ok(d.snapshot.playback_status().to_string()));
  b.property("LoopStatus").get(|_, d: &mut Data| Ok(d.snapshot.loop_status().to_string())).set(
    |_, d: &mut Data, v: String| {
      let repeat = match v.as_str() {
        "None" => "off",
        "Track" => "track",
        "Playlist" => "queue",
        _ => return Err(MethodErr::invalid_arg("LoopStatus must be None, Track or Playlist")),
      };
      d.send("player.set_repeat", json!({"repeat": repeat}));
      d.snapshot.repeat = repeat.to_string();
      d.pending.repeat = Some((repeat.to_string(), Instant::now()));
      Ok(None)
    },
  );
  b.property("Shuffle").get(|_, d: &mut Data| Ok(d.snapshot.shuffle)).set(|_, d: &mut Data, v: bool| {
    d.send("player.set_shuffle", json!({"shuffle": v}));
    d.snapshot.shuffle = v;
    d.pending.shuffle = Some((v, Instant::now()));
    Ok(None)
  });
  b.property("Volume").get(|_, d: &mut Data| Ok(d.snapshot.volume())).set(|_, d: &mut Data, v: f64| {
    // MPRIS permite valores mayores a 1; ketai no amplifica.
    let v = v.clamp(0.0, 1.0);
    d.send("player.set_volume", json!({"volume": v}));
    if d.snapshot.muted && v > 0.0 {
      d.send("player.set_muted", json!({"muted": false}));
      d.snapshot.muted = false;
      d.pending.muted = Some((false, Instant::now()));
    }
    d.snapshot.volume = v;
    d.pending.volume = Some((v, Instant::now()));
    Ok(None)
  });
  b.property("Metadata").get(|_, d: &mut Data| Ok(d.snapshot.metadata()));
  b.property("Position").emits_changed_false().get(|_, d: &mut Data| Ok(d.position_us()));
  b.property("Rate").get(|_, _| Ok(1.0f64));
  b.property("MinimumRate").get(|_, _| Ok(1.0f64));
  b.property("MaximumRate").get(|_, _| Ok(1.0f64));
  b.property("CanGoNext").get(|_, d: &mut Data| Ok(d.snapshot.can_next));
  b.property("CanGoPrevious").get(|_, d: &mut Data| Ok(d.snapshot.can_previous));
  b.property("CanPlay").get(|_, d: &mut Data| Ok(d.snapshot.can_play));
  b.property("CanPause").get(|_, d: &mut Data| Ok(d.snapshot.can_play));
  b.property("CanSeek").get(|_, d: &mut Data| Ok(d.snapshot.can_seek()));
  b.property("CanControl").emits_changed_const().get(|_, _| Ok(true));
}

#[cfg(test)]
mod pending_tests {
  use super::*;

  fn snap(repeat: &str, shuffle: bool, volume: f64) -> Snapshot {
    Snapshot { repeat: repeat.into(), shuffle, volume, ..Snapshot::default() }
  }

  #[test]
  fn an_older_state_never_undoes_a_value_just_written() {
    // Se escribio LoopStatus = Playlist; llega el estado de un cambio
    // anterior (todavia con repeat "off"): la lectura sigue viendo "queue".
    let mut p = Pending::default();
    let now = Instant::now();
    p.repeat = Some(("queue".into(), now));
    p.volume = Some((0.5, now));
    let mut s = snap("off", false, 0.5);
    p.apply(&mut s, now);
    assert_eq!(s.repeat, "queue");
    // El volumen ya coincide: se confirma y se olvida.
    assert!(p.volume.is_none());
    // Llega el estado con el valor nuevo: se confirma.
    let mut s = snap("queue", false, 0.5);
    p.apply(&mut s, now);
    assert!(p.repeat.is_none());
    // Si el daemon nunca lo confirma (por ejemplo, lo rechazo), a los 2 s
    // manda el estado real.
    p.shuffle = Some((true, now));
    let mut s = snap("queue", false, 0.5);
    p.apply(&mut s, now + Duration::from_secs(3));
    assert!(!s.shuffle);
    assert!(p.shuffle.is_none());
  }
}
