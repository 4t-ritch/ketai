//! Canal del visualizador (`viz.sock`): docs/protocol.md, seccion 7.
//!
//! - Un thread acepta clientes. Cada cliente tiene su propio thread
//!   escritor con una cola de solo 2 frames.
//! - Un thread analizador calcula un frame a `FPS` mientras suena algo y lo
//!   ofrece a cada cliente con `try_send`: si la cola de un cliente esta
//!   llena (cliente lento), ese frame se pierde para el. Nadie espera a un
//!   cliente del visualizador.

use std::{
  io::Write,
  os::unix::net::{UnixListener, UnixStream},
  sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
    mpsc::{self, SyncSender, TrySendError},
  },
  thread,
  time::{Duration, Instant},
};

use ketai_core::viz::{Analyzer, SharedRing};

pub const BINS: usize = 64;
pub const FPS: u32 = 60;
/// Frames que pueden esperar por cliente antes de empezar a perderse.
const CLIENT_QUEUE: usize = 2;

type Clients = Arc<Mutex<Vec<SyncSender<Arc<[u8]>>>>>;

pub struct VizService {
  ring: SharedRing,
  playing: Arc<AtomicBool>,
}

impl VizService {
  pub fn new(listener: UnixListener) -> Self {
    let ring = SharedRing::default();
    let playing = Arc::new(AtomicBool::new(false));
    let clients: Clients = Arc::default();

    let accept_clients = clients.clone();
    thread::Builder::new()
      .name("ketai-viz-accept".into())
      .spawn(move || accept_loop(listener, accept_clients))
      .expect("spawn viz accept thread");

    let (analyzer_ring, analyzer_playing) = (ring.clone(), playing.clone());
    thread::Builder::new()
      .name("ketai-viz".into())
      .spawn(move || analyzer_loop(analyzer_ring, analyzer_playing, clients))
      .expect("spawn viz thread");

    Self { ring, playing }
  }

  /// Donde el motor deja las muestras que suenan.
  pub fn ring(&self) -> SharedRing {
    self.ring.clone()
  }

  /// Solo se mandan frames mientras `status == "playing"`.
  pub fn set_playing(&self, playing: bool) {
    self.playing.store(playing, Ordering::Relaxed);
  }
}

fn header() -> [u8; 12] {
  let mut h = [0u8; 12];
  h[..4].copy_from_slice(b"KTV1");
  h[4..8].copy_from_slice(&(BINS as u32).to_le_bytes());
  h[8..12].copy_from_slice(&FPS.to_le_bytes());
  h
}

fn accept_loop(listener: UnixListener, clients: Clients) {
  for stream in listener.incoming() {
    let Ok(stream) = stream else { continue };
    let (tx, rx) = mpsc::sync_channel::<Arc<[u8]>>(CLIENT_QUEUE);
    thread::spawn(move || writer_loop(stream, rx));
    clients.lock().unwrap_or_else(|e| e.into_inner()).push(tx);
  }
}

/// Escribe el header y despues cada frame entero. Si el cliente lee lento,
/// solo se traba este thread; los frames que no entran en su cola se
/// pierden en el analizador.
fn writer_loop(mut stream: UnixStream, rx: mpsc::Receiver<Arc<[u8]>>) {
  if stream.write_all(&header()).is_err() {
    return;
  }
  for frame in rx {
    if stream.write_all(&frame).is_err() {
      return;
    }
  }
}

fn analyzer_loop(ring: SharedRing, playing: Arc<AtomicBool>, clients: Clients) {
  let period = Duration::from_secs(1) / FPS;
  let mut analyzer = Analyzer::new(BINS);
  let mut values = [0.0f32; BINS];
  let mut next = Instant::now();
  loop {
    next += period;
    let now = Instant::now();
    if next > now {
      thread::sleep(next - now);
    } else {
      // Atrasado (sistema cargado): no se intenta recuperar frames.
      next = now;
    }
    if !playing.load(Ordering::Relaxed) {
      continue;
    }
    let mut clients = clients.lock().unwrap_or_else(|e| e.into_inner());
    if clients.is_empty() {
      continue;
    }
    analyzer.frame_from(&ring, &mut values);
    let frame: Arc<[u8]> = values.iter().flat_map(|v| v.to_le_bytes()).collect();
    // Se quitan los clientes que se fueron; los llenos pierden este frame.
    clients.retain(|c| !matches!(c.try_send(frame.clone()), Err(TrySendError::Disconnected(_))));
  }
}
