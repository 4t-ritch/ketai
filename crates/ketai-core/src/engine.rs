//! Motor de audio: un thread propio que es el unico dueno de la salida.
//!
//! El dispositivo se abre una sola vez (en la primera carga) y se reutiliza
//! para todas las pistas. Nunca se abre un stream por pista.
//!
//! Cada carga lleva un numero de generacion. Los eventos que llegan con una
//! generacion vieja son de una pista anterior y el receptor los descarta.
//!
//! El seek nunca usa `Player::try_seek` de rodio: espera una confirmacion del
//! thread de audio que no llega si la pista termina justo durante el seek, y
//! el motor quedaba trabado para siempre. En su lugar se reabre la pista y se
//! la posiciona antes de entregarla al player, sin esperar a nadie.

use std::{
  path::PathBuf,
  sync::mpsc::{self, Receiver, RecvTimeoutError, Sender},
  thread,
  time::{Duration, Instant},
};

use rodio::{DeviceSinkBuilder, MixerDeviceSink, Player, Source};

use crate::decoder::AudioSource;

/// Cada cuanto se informa la posicion mientras suena.
const POSITION_INTERVAL: Duration = Duration::from_millis(250);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
  /// Dispositivo por defecto del sistema.
  System,
  /// Sin dispositivo: consume el audio en tiempo real y lo descarta.
  /// Se usa en los tests para no sonar por los parlantes.
  Null,
}

impl Backend {
  /// `KETAI_AUDIO_BACKEND=null` selecciona el backend nulo.
  pub fn from_env() -> Self {
    match std::env::var("KETAI_AUDIO_BACKEND").as_deref() {
      Ok("null") => Backend::Null,
      _ => Backend::System,
    }
  }
}

#[derive(Debug)]
pub enum Command {
  Load { generation: u64, path: PathBuf, start_ms: u64, play: bool },
  Play,
  Pause,
  Stop,
  Seek(u64),
  Volume(f32),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Event {
  Loaded { generation: u64, duration_ms: Option<u64> },
  Failed { generation: u64, message: String },
  Position { generation: u64, position_ms: u64 },
  Ended { generation: u64 },
}

pub struct EngineHandle {
  tx: Sender<Command>,
}

impl EngineHandle {
  pub fn send(&self, cmd: Command) {
    // Si el motor murio no hay nada que hacer: el daemon lo reporta por los
    // eventos que dejan de llegar.
    let _ = self.tx.send(cmd);
  }
}

/// Lanza el thread del motor. Los eventos salen por `events`.
pub fn spawn(backend: Backend, events: Sender<Event>) -> EngineHandle {
  let (tx, rx) = mpsc::channel();
  thread::Builder::new()
    .name("ketai-engine".into())
    .spawn(move || Engine::new(backend, events).run(rx))
    .expect("spawn engine thread");
  EngineHandle { tx }
}

struct Engine {
  backend: Backend,
  events: Sender<Event>,
  // El sink del sistema debe vivir mientras se usa el player.
  _sink: Option<MixerDeviceSink>,
  player: Option<Player>,
  generation: u64,
  /// Pista cargada, para reabrirla en cada seek.
  path: Option<PathBuf>,
  /// Generacion de la ultima carga que ya informo Loaded.
  loaded_generation: u64,
  /// Posicion desde la que arranco la fuente actual. El player cuenta desde
  /// 0 cada fuente nueva, asi que la posicion real es `base_ms + get_pos`.
  base_ms: u64,
  loaded: bool,
  playing: bool,
  volume: f32,
  last_position: Instant,
}

impl Engine {
  fn new(backend: Backend, events: Sender<Event>) -> Self {
    Self {
      backend,
      events,
      _sink: None,
      player: None,
      generation: 0,
      path: None,
      loaded_generation: 0,
      base_ms: 0,
      loaded: false,
      playing: false,
      volume: 1.0,
      last_position: Instant::now(),
    }
  }

  fn run(mut self, rx: Receiver<Command>) {
    loop {
      match rx.recv_timeout(POSITION_INTERVAL / 2) {
        Ok(cmd) => self.handle(cmd),
        Err(RecvTimeoutError::Timeout) => {}
        Err(RecvTimeoutError::Disconnected) => return,
      }
      self.tick();
    }
  }

  fn handle(&mut self, cmd: Command) {
    match cmd {
      Command::Load { generation, path, start_ms, play } => {
        self.generation = generation;
        if let Err(message) = self.load(path, start_ms, play) {
          self.loaded = false;
          self.playing = false;
          self.emit(Event::Failed { generation, message });
        }
      }
      Command::Play => {
        if let (true, Some(p)) = (self.loaded, &self.player) {
          p.play();
          self.playing = true;
        }
      }
      Command::Pause => {
        if let Some(p) = &self.player {
          p.pause();
        }
        self.playing = false;
      }
      Command::Stop => {
        if let Some(p) = &self.player {
          p.clear();
        }
        self.loaded = false;
        self.playing = false;
      }
      Command::Seek(ms) => {
        if let (true, Some(path)) = (self.loaded, self.path.clone()) {
          let play = self.playing;
          if self.load(path, ms, play).is_err() {
            // La pista ya se abrio bien al cargarla: si ahora no se puede
            // posicionar (por ejemplo, mas alla del final), se da por
            // terminada y la cola sigue.
            if let Some(p) = &self.player {
              p.clear();
            }
            self.loaded = false;
            self.playing = false;
            self.emit(Event::Ended { generation: self.generation });
            return;
          }
          self.report_position();
        }
      }
      Command::Volume(v) => {
        self.volume = v;
        if let Some(p) = &self.player {
          p.set_volume(v);
        }
      }
    }
  }

  fn load(&mut self, path: PathBuf, start_ms: u64, play: bool) -> Result<(), String> {
    let mut source = AudioSource::open(&path).map_err(|e| e.to_string())?;
    let duration_ms = source.total_duration().map(|d| d.as_millis() as u64);
    if start_ms > 0 {
      source.try_seek(Duration::from_millis(start_ms)).map_err(|e| e.to_string())?;
    }
    self.ensure_player()?;
    self.path = Some(path);
    self.base_ms = start_ms;
    let player = self.player.as_ref().expect("player ready");
    // clear() deja el player en pausa; se reanuda abajo si corresponde.
    player.clear();
    player.set_volume(self.volume);
    player.append(source);
    if play {
      player.play();
    }
    let first = !self.loaded || self.loaded_generation != self.generation;
    self.loaded = true;
    self.loaded_generation = self.generation;
    self.playing = play;
    // Un seek reabre la pista, pero para el receptor sigue siendo la misma
    // carga: Loaded se informa una sola vez por generacion.
    if first {
      self.emit(Event::Loaded { generation: self.generation, duration_ms });
    }
    Ok(())
  }

  fn ensure_player(&mut self) -> Result<(), String> {
    if self.player.is_some() {
      return Ok(());
    }
    match self.backend {
      Backend::System => {
        let mut sink = DeviceSinkBuilder::open_default_sink().map_err(|e| e.to_string())?;
        sink.log_on_drop(false);
        self.player = Some(Player::connect_new(sink.mixer()));
        self._sink = Some(sink);
      }
      Backend::Null => {
        let (player, output) = Player::new();
        thread::Builder::new()
          .name("ketai-null-output".into())
          .spawn(move || null_output(output))
          .map_err(|e| e.to_string())?;
        self.player = Some(player);
      }
    }
    Ok(())
  }

  fn tick(&mut self) {
    let Some(player) = &self.player else { return };
    if self.loaded && player.empty() {
      self.loaded = false;
      self.playing = false;
      self.emit(Event::Ended { generation: self.generation });
      return;
    }
    if self.playing && self.last_position.elapsed() >= POSITION_INTERVAL {
      self.report_position();
    }
  }

  fn report_position(&mut self) {
    if let Some(p) = &self.player {
      let position_ms = self.base_ms + p.get_pos().as_millis() as u64;
      self.last_position = Instant::now();
      self.emit(Event::Position { generation: self.generation, position_ms });
    }
  }

  fn emit(&self, event: Event) {
    let _ = self.events.send(event);
  }
}

/// Consume la salida del player al ritmo real del audio y la descarta.
fn null_output(mut output: impl Source<Item = f32>) {
  const CHUNK: Duration = Duration::from_millis(10);
  let mut next = Instant::now();
  loop {
    let per_second = output.sample_rate().get() as u64 * output.channels().get() as u64;
    let samples = per_second / 100;
    for _ in 0..samples {
      if output.next().is_none() {
        // El player se destruyo.
        return;
      }
    }
    next += CHUNK;
    let now = Instant::now();
    if next > now {
      thread::sleep(next - now);
    } else {
      next = now;
    }
  }
}
