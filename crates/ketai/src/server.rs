//! Canal de control: socket, framing y loop central.

use std::{
  collections::{HashMap, HashSet, VecDeque},
  fs,
  io::{self, BufRead, BufReader, Read, Write},
  os::unix::{
    fs::{DirBuilderExt, PermissionsExt},
    net::{UnixListener, UnixStream},
  },
  path::{Path, PathBuf},
  sync::{
    Arc, Condvar, Mutex,
    mpsc::{self, Receiver, Sender},
  },
  thread,
  time::{Duration, Instant},
};

use ketai_protocol::{
  ErrorReply, MAX_LINE_BYTES, PROTOCOL_VERSION, Request, TOPICS, codes, encode_error, encode_event,
  encode_result, parse_request,
};
use serde_json::{Value, json};

use crate::{
  systemd,
  instance,
  library::{LibraryService, TrackRef as LibTrack},
  lyrics::{LyricsService, TrackRef},
  mpris::MprisService,
  viz::VizService,
  outputs::OutputService,
  playlists::PlaylistService,
  player::{PlayerService, Resolved, Restored},
  session,
};

/// Intervalo del pulso periodico del loop central.
const TICK: Duration = Duration::from_millis(250);
/// Los cambios se escriben como maximo una vez por este intervalo.
const SAVE_DEBOUNCE: Duration = Duration::from_secs(1);
/// Mientras reproduce, la posicion se guarda cada este intervalo.
const SAVE_INTERVAL_DEFAULT: Duration = Duration::from_secs(10);

pub type ClientId = u64;

/// Mensajes que llegan al loop central.
pub enum Message {
  Connected(ClientId, Outbox),
  Request(ClientId, Request),
  /// Error de framing o parseo: se responde sin pasar por los handlers.
  Invalid(ClientId, ErrorReply),
  Disconnected(ClientId),
  /// Evento del motor de audio.
  Engine(ketai_core::engine::Event),
  /// Resultado diferido de `queue.add`.
  Resolved(Resolved),
  /// Sesion guardada ya resuelta.
  Restored(Restored),
  /// Resultado de una operacion de letras.
  Lyrics(crate::lyrics::Done),
  /// Resultado de una operacion de playlists.
  Playlist(crate::playlists::Done),
  /// Respuesta diferida de `library.*`, `track.cover` o `cache.*`.
  Library(crate::library::Done),
  /// Progreso de un escaneo de biblioteca.
  LibraryProgress(crate::library::Progress),
  /// Evento del control de salidas.
  Output(ketai_core::output::Event),
  /// Llego SIGTERM o SIGINT.
  Signal,
  /// Pulso periodico para tareas con tiempo (guardado de sesion).
  Tick,
  /// Pedido que llego por MPRIS (D-Bus), ya traducido al protocolo.
  Mpris(Request),
}

/// Lo que el loop central debe hacer despues de atender una peticion.
enum Flow {
  Continue,
  Shutdown,
}

pub fn run() -> io::Result<()> {
  // Los sockets de systemd se toman antes de crear threads: hay que borrar
  // variables de entorno.
  let inherited = systemd::listen_fds();
  // Las senales se atienden en un thread propio con sigwait. Se bloquean
  // aca, antes de crear cualquier thread, para que todos hereden la mascara
  // y ninguno las reciba de forma asincronica.
  let signals = block_shutdown_signals()?;
  let (tx, rx) = mpsc::channel();
  let signal_tx = tx.clone();
  thread::spawn(move || wait_signals(signals, signal_tx));

  let runtime = std::env::var_os("XDG_RUNTIME_DIR")
    .filter(|v| !v.is_empty())
    .map(PathBuf::from)
    .ok_or_else(|| io::Error::other("XDG_RUNTIME_DIR is not set"))?;

  let dir = runtime.join("ketai");
  create_private_dir(&dir)?;
  let socket_path = dir.join("ctl.sock");

  // El bloqueo debe vivir hasta el final de run(): al soltarlo se libera.
  let lock = match instance::claim(&dir, &socket_path)? {
    instance::Claim::Owner(lock) => lock,
    instance::Claim::AlreadyRunning => return Ok(()),
  };
  // Con socket activation el socket es de systemd: no se crea ni se borra.
  let viz_path = dir.join("viz.sock");
  let (listener, own_socket, inherited_viz) = match inherited {
    Some(sockets) => (sockets.ctl, None, sockets.viz),
    None => {
      // Tenemos el bloqueo, asi que ningun ketaid atiende ctl.sock. Si
      // igual acepta conexiones, es el socket de systemd (ketaid.socket):
      // no se toca. Esta misma conexion hace que systemd arranque el
      // daemon, y esta instancia le cede el lugar.
      if UnixStream::connect(&socket_path).is_ok() {
        eprintln!("ketaid: ctl.sock belongs to systemd (ketaid.socket); the daemon runs under systemd");
        drop(lock);
        return Ok(());
      }
      let l = bind_socket(&socket_path)?;
      (l, OwnPath::of(&socket_path), None)
    }
  };
  // viz.sock: el de systemd si lo paso; si no, uno propio.
  let (viz_listener, own_viz) = match inherited_viz {
    Some(l) => (l, None),
    None => {
      let l = bind_socket(&viz_path)?;
      (l, OwnPath::of(&viz_path))
    }
  };
  instance::mark_ready(&lock)?;
  systemd::notify("READY=1");
  // Los huerfanos se cierran en segundo plano: el daemon ya atiende clientes
  // mientras tanto.
  thread::spawn(instance::close_orphans);

  let accept_tx = tx.clone();
  thread::spawn(move || accept_loop(listener, accept_tx));
  let tick_tx = tx.clone();
  thread::spawn(move || {
    while tick_tx.send(Message::Tick).is_ok() {
      thread::sleep(TICK);
    }
  });

  let viz = VizService::new(viz_listener);
  let mut player = PlayerService::new(tx.clone(), viz.ring());
  let mut outputs = OutputService::new(tx.clone());
  let playlists = PlaylistService::new(tx.clone(), player.track_ids());
  let lyrics = LyricsService::new(tx.clone());
  let library = LibraryService::new(tx.clone());
  let mpris = MprisService::new(tx.clone());
  let persist = match session::path() {
    Some(path) => {
      if let Some(saved) = session::load(&path) {
        outputs.restore(saved.output.clone(), &mut player);
        player.restore(saved);
      }
      Some(Persist::new(session::Writer::spawn(path), &player))
    }
    None => {
      eprintln!("ketaid: no XDG_STATE_HOME or HOME; session will not be saved");
      None
    }
  };
  let result = central_loop(rx, Services { player, outputs, playlists, lyrics, library, mpris, viz }, persist);
  systemd::notify("STOPPING=1");
  // Los sockets propios se borran al salir para que los clientes no vean
  // uno muerto, pero solo si siguen siendo los nuestros.
  for own in [own_viz, own_socket].into_iter().flatten() {
    own.remove();
  }
  result
}

/// Socket creado por este proceso, identificado por dispositivo e inodo.
///
/// Si otra instancia lo reemplazo (por ejemplo, un dueno nuevo que cerro a
/// esta como huerfana), la ruta ya es de ella y no se borra.
struct OwnPath {
  path: PathBuf,
  dev: u64,
  ino: u64,
}

impl OwnPath {
  fn of(path: &Path) -> Option<Self> {
    use std::os::unix::fs::MetadataExt;
    let m = fs::symlink_metadata(path).ok()?;
    Some(Self { path: path.to_path_buf(), dev: m.dev(), ino: m.ino() })
  }

  fn remove(self) {
    use std::os::unix::fs::MetadataExt;
    if fs::symlink_metadata(&self.path).is_ok_and(|m| m.dev() == self.dev && m.ino() == self.ino) {
      let _ = fs::remove_file(&self.path);
    }
  }
}

/// Crea el directorio con permisos 0700 (o los corrige si ya existia).
fn create_private_dir(dir: &Path) -> io::Result<()> {
  fs::DirBuilder::new().recursive(true).mode(0o700).create(dir)?;
  fs::set_permissions(dir, fs::Permissions::from_mode(0o700))
}

fn bind_socket(path: &Path) -> io::Result<UnixListener> {
  // Solo se llega aca siendo duenos del bloqueo, asi que un socket existente
  // es de una ejecucion anterior y se puede borrar sin riesgo.
  if path.exists() {
    fs::remove_file(path)?;
  }
  let listener = UnixListener::bind(path)?;
  fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
  Ok(listener)
}

fn accept_loop(listener: UnixListener, tx: Sender<Message>) {
  let mut next_id: ClientId = 0;
  for stream in listener.incoming() {
    let Ok(stream) = stream else { continue };
    next_id += 1;
    spawn_client(next_id, stream, tx.clone());
  }
}

fn spawn_client(id: ClientId, stream: UnixStream, tx: Sender<Message>) {
  let Ok(write_half) = stream.try_clone() else { return };

  // Thread escritor: todo lo que se envia al cliente pasa por este canal,
  // asi respuestas y eventos nunca se mezclan a mitad de linea.
  let (out_tx, out_rx) = mpsc::channel::<Line>();
  let credit = Arc::new(Credit::default());
  let writer_credit = credit.clone();
  thread::spawn(move || writer_loop(write_half, out_rx, writer_credit));

  if tx.send(Message::Connected(id, Outbox(out_tx))).is_err() {
    return;
  }
  thread::spawn(move || reader_loop(id, stream, tx, credit));
}

/// Maximo de peticiones de un cliente leidas y todavia sin responder (la
/// respuesta cuenta cuando ya se escribio en el socket). Al llegar al tope
/// el daemon deja de leer a ese cliente hasta que lea respuestas: asi un
/// cliente que no lee no hace crecer la memoria del daemon.
const MAX_IN_FLIGHT: usize = 64;

/// Contador de peticiones en vuelo de un cliente, compartido entre su
/// thread lector (suma) y su thread escritor (resta). En C: un mutex y una
/// variable de condicion.
#[derive(Default)]
pub struct Credit {
  state: Mutex<(usize, bool)>,
  freed: Condvar,
}

impl Credit {
  /// Espera lugar para una peticion mas. false si la conexion se cerro.
  pub fn acquire(&self) -> bool {
    let mut g = self.state.lock().unwrap_or_else(|e| e.into_inner());
    while g.0 >= MAX_IN_FLIGHT && !g.1 {
      g = self.freed.wait(g).unwrap_or_else(|e| e.into_inner());
    }
    if g.1 {
      return false;
    }
    g.0 += 1;
    true
  }

  pub fn release(&self) {
    let mut g = self.state.lock().unwrap_or_else(|e| e.into_inner());
    g.0 = g.0.saturating_sub(1);
    self.freed.notify_one();
  }

  fn close(&self) {
    self.state.lock().unwrap_or_else(|e| e.into_inner()).1 = true;
    self.freed.notify_all();
  }
}

/// Linea para el cliente. Solo las respuestas liberan lugar en `Credit`.
struct Line {
  text: String,
  reply: bool,
}

/// Canal hacia el thread escritor de un cliente.
pub struct Outbox(Sender<Line>);

impl Outbox {
  fn reply(&self, text: String) {
    let _ = self.0.send(Line { text, reply: true });
  }

  fn event(&self, text: String) {
    let _ = self.0.send(Line { text, reply: false });
  }
}

fn writer_loop(mut stream: UnixStream, rx: Receiver<Line>, credit: Arc<Credit>) {
  for line in rx {
    if stream.write_all(line.text.as_bytes()).is_err() {
      break;
    }
    if line.reply {
      credit.release();
    }
  }
  // Despierta al lector si estaba esperando lugar.
  credit.close();
  // El canal se cerro: el loop central pidio cerrar la conexion.
  let _ = stream.shutdown(std::net::Shutdown::Both);
}

fn reader_loop(id: ClientId, stream: UnixStream, tx: Sender<Message>, credit: Arc<Credit>) {
  let mut reader = BufReader::new(stream);
  let mut line = Vec::new();
  loop {
    line.clear();
    // Se lee como maximo MAX_LINE_BYTES + 1 para detectar lineas largas sin
    // cargar en memoria todo lo que mande el cliente.
    let limit = (MAX_LINE_BYTES + 1) as u64;
    let read = match (&mut reader).take(limit).read_until(b'\n', &mut line) {
      Ok(n) => n,
      Err(_) => break,
    };
    if read == 0 {
      break;
    }
    let complete = line.last() == Some(&b'\n');
    if complete {
      line.pop();
    }
    if line.len() > MAX_LINE_BYTES || (!complete && read as u64 == limit) {
      let err = ErrorReply::new(None, codes::INVALID_REQUEST, "line too long");
      if credit.acquire() {
        let _ = tx.send(Message::Invalid(id, err));
      }
      break;
    }
    if !complete {
      // EOF sin salto de linea: el cliente cerro a mitad de mensaje.
      break;
    }
    if line.iter().all(u8::is_ascii_whitespace) {
      continue;
    }
    let msg = match parse_request(&line) {
      Ok(req) => Message::Request(id, req),
      Err(err) => Message::Invalid(id, err),
    };
    // Cada mensaje tiene respuesta: ocupa un lugar hasta que se escriba.
    if !credit.acquire() || tx.send(msg).is_err() {
      return;
    }
  }
  let _ = tx.send(Message::Disconnected(id));
}

/// Algo para responder a un cliente, en el orden en que llego.
enum Incoming {
  Request(Request),
  Invalid(ErrorReply),
}

struct ClientState {
  out: Outbox,
  /// Hay una peticion diferida sin responder. Mientras tanto, lo que manda
  /// este cliente espera en `pending` para respetar su orden.
  blocked: bool,
  pending: VecDeque<Incoming>,
  /// Topicos suscritos con `events.subscribe`.
  topics: HashSet<&'static str>,
}

fn block_shutdown_signals() -> io::Result<libc::sigset_t> {
  // SAFETY: sigset_t se inicializa con sigemptyset antes de usarse, y
  // pthread_sigmask solo lee el conjunto.
  unsafe {
    let mut set: libc::sigset_t = std::mem::zeroed();
    libc::sigemptyset(&mut set);
    libc::sigaddset(&mut set, libc::SIGTERM);
    libc::sigaddset(&mut set, libc::SIGINT);
    if libc::pthread_sigmask(libc::SIG_BLOCK, &set, std::ptr::null_mut()) != 0 {
      return Err(io::Error::last_os_error());
    }
    Ok(set)
  }
}

fn wait_signals(set: libc::sigset_t, tx: Sender<Message>) {
  loop {
    let mut sig: libc::c_int = 0;
    // SAFETY: `set` es un conjunto valido y `sig` es un entero propio.
    if unsafe { libc::sigwait(&set, &mut sig) } == 0 && tx.send(Message::Signal).is_err() {
      return;
    }
  }
}

/// Cuando y que guardar de la sesion.
struct Persist {
  writer: session::Writer,
  /// Ultima sesion escrita sin la posicion, para detectar cambios.
  last_key: Value,
  last_write: Instant,
  dirty: bool,
  interval: Duration,
}

impl Persist {
  fn new(writer: session::Writer, player: &PlayerService) -> Self {
    let interval = std::env::var("KETAI_SESSION_SAVE_INTERVAL_MS")
      .ok()
      .and_then(|v| v.parse().ok())
      .map(Duration::from_millis)
      .unwrap_or(SAVE_INTERVAL_DEFAULT);
    Self {
      writer,
      last_key: key(&player.session_json()),
      last_write: Instant::now(),
      dirty: false,
      interval,
    }
  }

  /// Se llama despues de cada mensaje del loop central.
  fn after_message(&mut self, player: &PlayerService) {
    // Mientras se restaura, guardar pisaria la sesion que se esta leyendo.
    if player.restoring() {
      return;
    }
    let session = player.session_json();
    let k = key(&session);
    if k != self.last_key {
      self.last_key = k;
      self.dirty = true;
    }
    let since = self.last_write.elapsed();
    let periodic = player.is_playing() && since >= self.interval;
    if (self.dirty && since >= SAVE_DEBOUNCE) || periodic {
      self.writer.save(session.to_string());
      self.last_write = Instant::now();
      self.dirty = false;
    }
  }

  /// Guardado final al apagar: espera a que llegue al disco.
  fn final_save(&self, player: &PlayerService) {
    if !player.restoring() {
      self.writer.save_and_wait(player.session_json().to_string(), Duration::from_secs(2));
    }
  }
}

/// Lo que cuenta como cambio. Mientras suena, la posicion avanza sola y se
/// guarda con el guardado periodico; en pausa o detenido, un cambio de
/// posicion (un seek) si cuenta, para que sobreviva a un corte.
fn key(session: &Value) -> Value {
  let mut k = session.clone();
  if session["status"] == "playing"
    && let Some(obj) = k.as_object_mut()
  {
    obj.remove("position_ms");
  }
  k
}

/// Todo el estado que maneja el loop central.
struct Services {
  player: PlayerService,
  outputs: OutputService,
  playlists: PlaylistService,
  lyrics: LyricsService,
  library: LibraryService,
  mpris: MprisService,
  viz: VizService,
}

fn central_loop(rx: Receiver<Message>, mut services: Services, mut persist: Option<Persist>) -> io::Result<()> {
  let mut clients: HashMap<ClientId, ClientState> = HashMap::new();

  for msg in rx {
    let flow = match msg {
      Message::Connected(id, out) => {
        clients.insert(id, ClientState { out, blocked: false, pending: VecDeque::new(), topics: HashSet::new() });
        Flow::Continue
      }
      Message::Disconnected(id) => {
        // Soltar el Sender cierra el thread escritor y la conexion.
        clients.remove(&id);
        Flow::Continue
      }
      Message::Engine(event) => {
        services.player.on_engine(event);
        Flow::Continue
      }
      Message::Tick => Flow::Continue,
      Message::Lyrics(done) => {
        services.lyrics.note_changed(done.changed);
        match clients.get_mut(&done.client) {
          Some(state) => {
            send_reply(&state.out, done.request_id, done.result);
            state.blocked = false;
            drain(&mut clients, &mut services, done.client)
          }
          None => Flow::Continue,
        }
      }
      Message::Library(done) => {
        if done.changed {
          services.library.note_changed();
        }
        match clients.get_mut(&done.client) {
          Some(state) => {
            send_reply(&state.out, done.request_id, done.result);
            state.blocked = false;
            drain(&mut clients, &mut services, done.client)
          }
          None => Flow::Continue,
        }
      }
      Message::LibraryProgress(p) => {
        services.library.note_progress(p);
        Flow::Continue
      }
      Message::Playlist(done) => {
        if let Some((tracks, replace)) = done.load {
          services.player.load_tracks(tracks, replace);
        }
        services.playlists.note_changed(done.changed);
        match clients.get_mut(&done.client) {
          Some(state) => {
            send_reply(&state.out, done.request_id, done.result);
            state.blocked = false;
            drain(&mut clients, &mut services, done.client)
          }
          None => Flow::Continue,
        }
      }
      Message::Output(event) => {
        services.outputs.on_event(event, &mut services.player);
        Flow::Continue
      }
      Message::Signal => Flow::Shutdown,
      Message::Restored(restored) => {
        services.player.on_restored(restored);
        // Las peticiones que esperaban la restauracion ya pueden seguir.
        let ids: Vec<ClientId> = clients.iter().filter(|(_, c)| c.blocked).map(|(id, _)| *id).collect();
        let mut flow = Flow::Continue;
        for id in ids {
          if let Some(c) = clients.get_mut(&id) {
            c.blocked = false;
          }
          if let Flow::Shutdown = drain(&mut clients, &mut services, id) {
            flow = Flow::Shutdown;
          }
        }
        flow
      }
      Message::Invalid(id, err) => incoming(&mut clients, &mut services, id, Incoming::Invalid(err)),
      Message::Request(id, req) => incoming(&mut clients, &mut services, id, Incoming::Request(req)),
      Message::Mpris(req) => {
        // Durante la restauracion se descarta: un pedido de las teclas
        // multimedia no puede esperar a ningun cliente.
        if PlayerService::handles(&req.method) && !services.player.restoring() {
          let reply = services.player.handle(MPRIS_CLIENT, &req);
          if matches!(reply, Some(Ok(_))) && is_seek(&req.method) {
            services.mpris.note_seeked();
          }
        }
        // Libera el lugar que ocupaba el pedido (ver `Credit`).
        services.mpris.handled();
        Flow::Continue
      }
      Message::Resolved(resolved) => {
        let (client, request_id, reply) = services.player.on_resolved(resolved);
        match clients.get_mut(&client) {
          Some(state) => {
            send_reply(&state.out, request_id, reply);
            state.blocked = false;
            drain(&mut clients, &mut services, client)
          }
          None => Flow::Continue,
        }
      }
    };
    publish(&clients, &mut services);
    if let Some(p) = persist.as_mut() {
      p.after_message(&services.player);
    }
    if let Flow::Shutdown = flow {
      if let Some(p) = &persist {
        p.final_save(&services.player);
      }
      // Al soltar los Sender, cada escritor envia lo pendiente y cierra.
      drop(clients);
      // Pequena espera para que la respuesta salga antes del exit.
      thread::sleep(std::time::Duration::from_millis(50));
      return Ok(());
    }
  }
  Ok(())
}

/// Encola lo que llego del cliente y procesa lo que se pueda.
fn incoming(
  clients: &mut HashMap<ClientId, ClientState>,
  services: &mut Services,
  id: ClientId,
  item: Incoming,
) -> Flow {
  let Some(state) = clients.get_mut(&id) else { return Flow::Continue };
  state.pending.push_back(item);
  drain(clients, services, id)
}

/// Procesa en orden lo pendiente del cliente hasta que se vacie o una
/// peticion quede diferida.
fn drain(clients: &mut HashMap<ClientId, ClientState>, services: &mut Services, id: ClientId) -> Flow {
  loop {
    let Some(state) = clients.get_mut(&id) else { return Flow::Continue };
    if state.blocked {
      return Flow::Continue;
    }
    let Some(item) = state.pending.pop_front() else { return Flow::Continue };
    let req = match item {
      Incoming::Invalid(err) => {
        state.out.reply(encode_error(&err));
        continue;
      }
      Incoming::Request(req) => req,
    };
    if req.method.starts_with("events.") {
      let reply = subscription(state, &req);
      let subscribed_player = reply.is_ok() && req.method == "events.subscribe" && state.topics.contains("player");
      send_reply(&state.out, req.id, reply);
      if subscribed_player {
        let (name, data) = services.player.state_event();
        state.out.event(encode_event(name, data));
      }
      continue;
    }
    if OutputService::handles(&req.method) {
      let reply = services.outputs.handle(&req, &mut services.player);
      send_reply(&state.out, req.id, reply);
      continue;
    }
    let gated = PlayerService::handles(&req.method)
      || PlaylistService::handles(&req.method)
      || LyricsService::handles(&req.method)
      || LibraryService::needs_track(&req.method);
    if services.player.restoring() && gated {
      // Hasta que termine la restauracion, para que vea la cola restaurada.
      state.pending.push_front(Incoming::Request(req));
      state.blocked = true;
      return Flow::Continue;
    }
    if LibraryService::handles(&req.method) {
      let by_uri = req.params.get("track_id").is_none().then(|| req.params.get("uri").and_then(Value::as_str)).flatten();
      let track = if let (true, Some(uri)) = (LibraryService::needs_track(&req.method), by_uri) {
        // Pista fuera de la cola (playlist, busqueda): se identifica por URI.
        Some(LibTrack { id: crate::library::uri_cover_id(uri), path: ketai_core::uri::file_uri_to_path(uri) })
      } else if LibraryService::needs_track(&req.method) {
        let found = LyricsService::track_id(&req).and_then(|tid| {
          services.player.find_track(&tid).map(|(_, path)| LibTrack { id: tid.clone(), path }).ok_or_else(|| {
            ErrorReply::new(Some(req.id), codes::INVALID_PARAMS, format!("unknown track_id: {tid}"))
          })
        });
        match found {
          Ok(t) => Some(t),
          Err(err) => {
            send_reply(&state.out, req.id, Err(err));
            continue;
          }
        }
      } else {
        None
      };
      // La respuesta llega despues como Message::Library.
      services.library.submit(id, &req, track);
      state.blocked = true;
      return Flow::Continue;
    }
    if LyricsService::handles(&req.method) {
      let track = LyricsService::track_id(&req).and_then(|tid| {
        services.player.find_track(&tid).ok_or_else(|| {
          ErrorReply::new(Some(req.id), codes::INVALID_PARAMS, format!("unknown track_id: {tid}"))
        })
      });
      match track {
        Ok((uri, path)) => {
          // La respuesta llega despues como Message::Lyrics.
          services.lyrics.submit(id, &req, TrackRef { uri, path });
          state.blocked = true;
          return Flow::Continue;
        }
        Err(err) => {
          send_reply(&state.out, req.id, Err(err));
          continue;
        }
      }
    }
    if PlaylistService::handles(&req.method) {
      // La respuesta llega despues como Message::Playlist.
      services.playlists.submit(id, &req, services.player.queue_uris());
      state.blocked = true;
      return Flow::Continue;
    }
    let (reply, flow) = if PlayerService::handles(&req.method) {
      match services.player.handle(id, &req) {
        Some(reply) => {
          if reply.is_ok() && is_seek(&req.method) {
            services.mpris.note_seeked();
          }
          (reply, Flow::Continue)
        }
        None => {
          // La respuesta llega despues como Message::Resolved.
          state.blocked = true;
          return Flow::Continue;
        }
      }
    } else {
      handle(&req)
    };
    send_reply(&state.out, req.id, reply);
    if let Flow::Shutdown = flow {
      return Flow::Shutdown;
    }
  }
}

/// `events.subscribe` / `events.unsubscribe`.
fn subscription(state: &mut ClientState, req: &Request) -> Result<Value, ErrorReply> {
  let invalid = || ErrorReply::new(Some(req.id), codes::INVALID_PARAMS, "topics must be a non-empty list of known topics");
  let list = req.params.get("topics").and_then(Value::as_array).ok_or_else(invalid)?;
  if list.is_empty() {
    return Err(invalid());
  }
  let topics = list
    .iter()
    .map(|t| t.as_str().and_then(|t| TOPICS.iter().find(|k| **k == t).copied()).ok_or_else(invalid))
    .collect::<Result<Vec<&'static str>, _>>()?;
  match req.method.as_str() {
    "events.subscribe" => state.topics.extend(topics),
    "events.unsubscribe" => topics.iter().for_each(|t| {
      state.topics.remove(t);
    }),
    _ => return Err(ErrorReply::new(Some(req.id), codes::METHOD_NOT_FOUND, "method not found")),
  }
  Ok(json!({}))
}

/// Envia los eventos pendientes a los clientes suscritos a cada topico.
/// Id de "cliente" para los pedidos de MPRIS. Los clientes reales empiezan
/// en 1, asi que una respuesta diferida para este id simplemente se descarta.
const MPRIS_CLIENT: ClientId = 0;

fn is_seek(method: &str) -> bool {
  method == "player.seek" || method == "player.seek_relative"
}

fn publish(clients: &HashMap<ClientId, ClientState>, services: &mut Services) {
  services.viz.set_playing(services.player.is_playing());
  let (len, current) = services.player.queue_position();
  services.mpris.update(&services.player.state_event().1, len, current);
  for track_id in services.lyrics.take_changed() {
    let line = encode_event("lyrics.changed", json!({"track_id": track_id}));
    for state in clients.values().filter(|c| c.topics.contains("lyrics")) {
      state.out.event(line.clone());
    }
  }
  if services.library.take_changed() {
    let line = encode_event("library.changed", json!({}));
    for state in clients.values().filter(|c| c.topics.contains("library")) {
      state.out.event(line.clone());
    }
  }
  for p in services.library.take_progress() {
    let line = encode_event("library.progress", json!({"job": p.job, "done": p.done, "total": p.total}));
    for state in clients.values().filter(|c| c.topics.contains("library")) {
      state.out.event(line.clone());
    }
  }
  for name in services.playlists.take_changed() {
    let line = encode_event("playlist.changed", json!({"name": name}));
    for state in clients.values().filter(|c| c.topics.contains("playlist")) {
      state.out.event(line.clone());
    }
  }
  if let Some(data) = services.outputs.take_event() {
    let line = encode_event("output.changed", data);
    for state in clients.values().filter(|c| c.topics.contains("output")) {
      state.out.event(line.clone());
    }
  }
  for (topic, name, data) in services.player.take_events() {
    let line = encode_event(name, data);
    for state in clients.values().filter(|c| c.topics.contains(topic)) {
      state.out.event(line.clone());
    }
  }
}

fn send_reply(out: &Outbox, request_id: u64, reply: Result<Value, ErrorReply>) {
  let line = match reply {
    Ok(result) => encode_result(request_id, result),
    Err(err) => encode_error(&err),
  };
  out.reply(line);
}

fn handle(req: &Request) -> (Result<Value, ErrorReply>, Flow) {
  match req.method.as_str() {
    "daemon.hello" => (hello(req), Flow::Continue),
    "daemon.shutdown" => (Ok(json!({})), Flow::Shutdown),
    _ => (
      Err(ErrorReply::new(Some(req.id), codes::METHOD_NOT_FOUND, "method not found")),
      Flow::Continue,
    ),
  }
}

fn hello(req: &Request) -> Result<Value, ErrorReply> {
  let protocol = req
    .params
    .get("protocol")
    .and_then(Value::as_u64)
    .ok_or_else(|| ErrorReply::new(Some(req.id), codes::INVALID_PARAMS, "protocol is required"))?;
  if protocol != PROTOCOL_VERSION {
    return Err(ErrorReply::new(
      Some(req.id),
      codes::PROTOCOL_MISMATCH,
      format!("protocol mismatch: server speaks {PROTOCOL_VERSION}"),
    ));
  }
  Ok(json!({
    "server": "ketaid",
    "protocol": PROTOCOL_VERSION,
    "version": env!("CARGO_PKG_VERSION"),
  }))
}
