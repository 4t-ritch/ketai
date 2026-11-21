//! Dispositivos de salida.
//!
//! Un thread propio habla con el servidor de sonido (PulseAudio, o PipeWire a
//! traves de pipewire-pulse). Lista los sinks, sigue cual es el default y
//! mueve los streams de este proceso al sink que corresponde:
//! - el elegido con `set_target`, si existe;
//! - si no, el default del sistema (y lo sigue si cambia).
//!
//! El stream de audio se abre una sola vez; cambiar de salida lo mueve, no lo
//! reabre.

use std::{
  cell::{Cell, RefCell},
  rc::Rc,
  sync::mpsc::{self, Receiver, Sender, TryRecvError},
  thread,
  time::{Duration, Instant},
};

use libpulse_binding::{
  callbacks::ListResult,
  context::{Context, FlagSet, State, subscribe::InterestMaskSet},
  mainloop::standard::{IterateResult, Mainloop},
  operation::{Operation, State as OpState},
};

use crate::engine::Backend;

/// Cada cuanto se refresca aunque no lleguen notificaciones.
const REFRESH_FALLBACK: Duration = Duration::from_secs(2);
const POLL: Duration = Duration::from_millis(50);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Device {
  pub id: String,
  pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Snapshot {
  pub devices: Vec<Device>,
  pub default: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
  /// Cambio la lista de dispositivos o el default.
  Changed(Snapshot),
  /// No hay servidor de sonido con el que hablar.
  Unavailable(String),
}

enum Command {
  SetTarget(Option<String>),
}

pub struct OutputHandle {
  tx: Sender<Command>,
}

impl OutputHandle {
  /// `None` sigue el default del sistema.
  pub fn set_target(&self, id: Option<String>) {
    let _ = self.tx.send(Command::SetTarget(id));
  }
}

pub fn spawn(backend: Backend, events: Sender<Event>) -> OutputHandle {
  let (tx, rx) = mpsc::channel();
  thread::Builder::new()
    .name("ketai-output".into())
    .spawn(move || match backend {
      Backend::Null => fake(rx, events),
      Backend::System => {
        if let Err(msg) = pulse(rx, &events) {
          eprintln!("ketaid: output control unavailable: {msg}");
          let _ = events.send(Event::Unavailable(msg));
        }
      }
    })
    .expect("spawn output thread");
  OutputHandle { tx }
}

/// Backend de los tests: dispositivos fijos de `KETAI_FAKE_OUTPUTS`
/// (`id=Nombre;id2=Nombre2`, el primero es el default).
fn fake(rx: Receiver<Command>, events: Sender<Event>) {
  let devices: Vec<Device> = std::env::var("KETAI_FAKE_OUTPUTS")
    .unwrap_or_default()
    .split(';')
    .filter_map(|entry| {
      let (id, name) = entry.split_once('=')?;
      Some(Device { id: id.to_string(), name: name.to_string() })
    })
    .collect();
  let default = devices.first().map(|d| d.id.clone());
  let _ = events.send(Event::Changed(Snapshot { devices, default }));
  // Los comandos no hacen nada: no hay audio real que mover.
  for _ in rx {}
}

fn pulse(rx: Receiver<Command>, events: &Sender<Event>) -> Result<(), String> {
  let mut mainloop = Mainloop::new().ok_or("could not create mainloop")?;
  let mut ctx = Context::new(&mainloop, "ketai").ok_or("could not create context")?;
  ctx.connect(None, FlagSet::NOFLAGS, None).map_err(|e| e.to_string().unwrap_or_else(|| "connect failed".into()))?;
  loop {
    iterate(&mut mainloop)?;
    match ctx.get_state() {
      State::Ready => break,
      State::Failed | State::Terminated => return Err("could not connect to the sound server".into()),
      _ => {}
    }
  }

  // Cualquier cambio de sinks, streams o servidor marca para refrescar.
  let dirty = Rc::new(Cell::new(true));
  let flag = dirty.clone();
  ctx.set_subscribe_callback(Some(Box::new(move |_, _, _| flag.set(true))));
  ctx.subscribe(InterestMaskSet::SINK | InterestMaskSet::SINK_INPUT | InterestMaskSet::SERVER, |_| {});

  let own = OwnStream::current();
  let mut target: Option<String> = None;
  let mut published: Option<Snapshot> = None;
  let mut last_refresh = Instant::now();

  loop {
    loop {
      match rx.try_recv() {
        Ok(Command::SetTarget(t)) => {
          target = t;
          dirty.set(true);
        }
        Err(TryRecvError::Empty) => break,
        Err(TryRecvError::Disconnected) => return Ok(()),
      }
    }

    if dirty.get() || last_refresh.elapsed() >= REFRESH_FALLBACK {
      dirty.set(false);
      last_refresh = Instant::now();
      let state = query(&mut mainloop, &ctx, &own)?;
      if published.as_ref() != Some(&state.snapshot) {
        published = Some(state.snapshot.clone());
        let _ = events.send(Event::Changed(state.snapshot.clone()));
      }
      route(&mut mainloop, &mut ctx, &state, target.as_deref())?;
    }

    match mainloop.iterate(false) {
      IterateResult::Success(_) => {}
      _ => return Err("sound server connection lost".into()),
    }
    thread::sleep(POLL);
  }
}

/// Estado leido del servidor en un refresco.
struct ServerState {
  snapshot: Snapshot,
  /// Indice -> nombre de cada sink.
  sinks: Vec<(u32, String)>,
  /// Streams de este proceso: (indice del stream, indice del sink).
  own_inputs: Vec<(u32, u32)>,
}

/// Como reconocer los streams de este proceso.
#[derive(Clone)]
struct OwnStream {
  pid: String,
  /// El plugin ALSA de PipeWire no publica el PID, pero nombra el nodo
  /// `alsa_playback.<binario>`. Hay una sola instancia por sesion, asi que
  /// el nombre no choca con otro proceso.
  alsa_node: Option<String>,
}

impl OwnStream {
  fn current() -> Self {
    let binary = std::env::current_exe()
      .ok()
      .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()));
    Self { pid: std::process::id().to_string(), alsa_node: binary.map(|b| format!("alsa_playback.{b}")) }
  }

  fn matches(&self, proplist: &libpulse_binding::proplist::Proplist) -> bool {
    proplist.get_str("application.process.id").as_deref() == Some(self.pid.as_str())
      || (self.alsa_node.is_some() && proplist.get_str("node.name") == self.alsa_node)
  }
}

fn query(mainloop: &mut Mainloop, ctx: &Context, own: &OwnStream) -> Result<ServerState, String> {
  let introspect = ctx.introspect();

  let default = Rc::new(RefCell::new(None));
  let d = default.clone();
  let op = introspect.get_server_info(move |info| {
    *d.borrow_mut() = info.default_sink_name.as_ref().map(|n| n.to_string());
  });
  wait(mainloop, &op)?;

  let sinks = Rc::new(RefCell::new(Vec::new()));
  let s = sinks.clone();
  let op = introspect.get_sink_info_list(move |item| {
    if let ListResult::Item(info) = item
      && let Some(name) = &info.name
    {
      let desc = info.description.as_ref().map(|d| d.to_string()).unwrap_or_else(|| name.to_string());
      s.borrow_mut().push((info.index, name.to_string(), desc));
    }
  });
  wait(mainloop, &op)?;

  let inputs = Rc::new(RefCell::new(Vec::new()));
  let i = inputs.clone();
  let own = own.clone();
  let op = introspect.get_sink_input_info_list(move |item| {
    if let ListResult::Item(info) = item
      && own.matches(&info.proplist)
    {
      i.borrow_mut().push((info.index, info.sink));
    }
  });
  wait(mainloop, &op)?;

  let sinks = sinks.take();
  Ok(ServerState {
    snapshot: Snapshot {
      devices: sinks.iter().map(|(_, id, name)| Device { id: id.clone(), name: name.clone() }).collect(),
      default: default.take(),
    },
    sinks: sinks.into_iter().map(|(idx, id, _)| (idx, id)).collect(),
    own_inputs: inputs.take(),
  })
}

/// Mueve los streams propios al sink que corresponde.
fn route(mainloop: &mut Mainloop, ctx: &mut Context, state: &ServerState, target: Option<&str>) -> Result<(), String> {
  let exists = |id: &str| state.sinks.iter().any(|(_, name)| name == id);
  // Un dispositivo elegido que no esta conectado cae al default.
  let wanted = match target.filter(|t| exists(t)) {
    Some(t) => t.to_string(),
    None => match &state.snapshot.default {
      Some(d) => d.clone(),
      None => return Ok(()),
    },
  };
  for (input, sink) in &state.own_inputs {
    let current = state.sinks.iter().find(|(idx, _)| idx == sink).map(|(_, n)| n.as_str());
    if current != Some(wanted.as_str()) {
      let op = ctx.introspect().move_sink_input_by_name(*input, &wanted, None);
      wait(mainloop, &op)?;
    }
  }
  Ok(())
}

fn wait<T: ?Sized>(mainloop: &mut Mainloop, op: &Operation<T>) -> Result<(), String> {
  while op.get_state() == OpState::Running {
    iterate(mainloop)?;
  }
  Ok(())
}

fn iterate(mainloop: &mut Mainloop) -> Result<(), String> {
  match mainloop.iterate(true) {
    IterateResult::Success(_) => Ok(()),
    IterateResult::Quit(_) => Err("mainloop quit".into()),
    IterateResult::Err(e) => Err(e.to_string().unwrap_or_else(|| "mainloop error".into())),
  }
}
