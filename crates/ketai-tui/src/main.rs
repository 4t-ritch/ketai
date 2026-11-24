//! Binario `ketai`: prepara la terminal y corre el loop de eventos.
//!
//! Un unico canal recibe todo: teclas, mensajes del daemon y resultados de
//! las tareas en segundo plano (buscar el servicio, levantarlo). El loop
//! principal nunca espera a la red ni a systemd.

use std::{
  io::{self, Read, Write},
  sync::mpsc::{self, Sender},
  thread,
  time::{Duration, Instant},
};

use ketai_tui::{
  app::{App, Screen},
  client::{self, Client, DaemonMsg},
  config::{self, Config},
  covers, kitty,
  service::{self, Action, Observed},
  ui, viz,
};
use ratatui::crossterm::event::{self, Event, KeyEventKind};
use serde_json::json;

/// Todo lo que despierta al loop principal.
enum Input {
  Key(event::KeyEvent),
  Resize,
  Mouse(ratatui::crossterm::event::MouseEvent),
  /// El panel recupero el foco.
  Focus,
  Daemon(DaemonMsg),
  /// Resultado de buscar el servicio.
  Observed(Observed),
  /// Termino una accion sobre el servicio.
  ActionDone(Result<(), String>),
  /// Frame de `viz.sock`.
  Viz(Vec<f32>),
  /// Caratula preparada en otro thread (o `None` si no se pudo leer).
  Cover(String, Option<covers::Prepared>),
  /// Version atenuada de una caratula, lista para transmitir.
  CoverDim(String, String),
  /// config.toml cambio en disco.
  Config(Result<Config, String>),
}

/// Pregunta a la terminal si soporta el protocolo de graficos de kitty. La
/// respuesta llega por la entrada estandar antes que la de DA1.
fn kitty_supported() -> bool {
  let mut out = io::stdout();
  if out.write_all(kitty::QUERY.as_bytes()).and_then(|_| out.flush()).is_err() {
    return false;
  }
  let mut reply = Vec::new();
  let deadline = Instant::now() + Duration::from_millis(1500);
  let mut stdin = io::stdin();
  while Instant::now() < deadline {
    let mut fds = libc::pollfd { fd: 0, events: libc::POLLIN, revents: 0 };
    // SAFETY: un pollfd valido y una sola entrada.
    let ready = unsafe { libc::poll(&mut fds, 1, 100) };
    if ready <= 0 {
      continue;
    }
    let mut chunk = [0u8; 256];
    match stdin.read(&mut chunk) {
      Ok(0) | Err(_) => break,
      Ok(n) => reply.extend_from_slice(&chunk[..n]),
    }
    // DA1 termina con "c": la terminal ya contesto todo.
    if reply.windows(3).any(|w| w == b"\x1b[?") && reply.ends_with(b"c") {
      break;
    }
  }
  kitty::supported_reply(&reply)
}

/// Caratulas que la terminal guarda como mucho. Pocas pantallas muestran mas
/// de una docena; el resto se borra y se vuelve a mandar si reaparece.
const KEEP_COVERS: usize = 48;
/// Cada cuanto se vuelven a poner las ubicaciones de las imagenes.
const REFRESH_PLACEMENTS: Duration = Duration::from_secs(2);
/// Caratulas que ketai guarda en memoria (~300 KB cada una, ~36 MB en total).
const KEEP_IN_MEMORY: usize = 120;

/// Trabajo para el thread de caratulas.
enum Prep {
  File(String, std::path::PathBuf, u32),
  Image(String, image::RgbImage, u32),
  Dimmed(String, image::RgbImage, u32),
}

/// Escribe secuencias de la capa de imagenes sin perder el cursor.
/// Tamano de una celda en pixeles, segun la terminal. `None` si no lo
/// informa (el modo karaoke queda en clasico).
fn cell_size() -> Option<(u16, u16)> {
  // SAFETY: TIOCGWINSZ solo escribe la estructura que se le pasa.
  let mut ws: libc::winsize = unsafe { std::mem::zeroed() };
  let ok = unsafe { libc::ioctl(1, libc::TIOCGWINSZ, &mut ws) } == 0;
  ketai_tui::app::cell_px(ok.then_some((ws.ws_col, ws.ws_row, ws.ws_xpixel, ws.ws_ypixel)))
}

fn emit(seq: &str) {
  if seq.is_empty() {
    return;
  }
  let mut out = io::stdout();
  let _ = out.write_all(b"\x1b7");
  let _ = out.write_all(seq.as_bytes());
  let _ = out.write_all(b"\x1b8");
  let _ = out.flush();
}

/// Envia una peticion y la anota para reconocer su respuesta.
fn send(client: &mut Option<Client>, app: &mut App, method: &str, params: serde_json::Value) {
  if let Some(c) = client.as_mut()
    && let Ok(id) = c.request(method, params)
  {
    app.sent(id, method);
  }
}

/// Lo que se pide al conectar: estado, cola y el espectro.
fn on_connected(client: &mut Option<Client>, app: &mut App, tx: &Sender<Input>) {
  app.connected = true;
  app.screen = Screen::Main;
  send(client, app, "player.state", json!({}));
  send(client, app, "queue.list", json!({}));
  send(client, app, "output.list", json!({}));
  if let Some(path) = viz::socket_path() {
    let _ = viz::spawn(&path, tx.clone(), Input::Viz);
  }
}

/// Cada cuanto se reintenta conectar desde la pantalla de servicio.
const RETRY: Duration = Duration::from_secs(2);

fn connect(tx: &Sender<Input>) -> Option<Client> {
  let mut c = Client::connect(&client::socket_path()?, tx.clone(), Input::Daemon).ok()?;
  c.request("daemon.hello", json!({"protocol": ketai_protocol::PROTOCOL_VERSION, "client": "ketai-tui"})).ok()?;
  c.request("events.subscribe", json!({"topics": ["player", "queue", "output", "lyrics", "playlist", "library"]})).ok()?;
  Some(c)
}

/// Busca el servicio en segundo plano (systemd puede tardar).
fn spawn_observe(tx: &Sender<Input>) {
  let tx = tx.clone();
  thread::spawn(move || {
    let _ = tx.send(Input::Observed(service::observe(false)));
  });
}

fn spawn_action(tx: &Sender<Input>, action: Action, observed: Observed) {
  let tx = tx.clone();
  thread::spawn(move || {
    let _ = tx.send(Input::ActionDone(service::execute(action, &observed)));
  });
}

fn show_service(app: &mut App, tx: &Sender<Input>) {
  app.connected = false;
  app.screen = Screen::Service { state: service::State::Starting, observed: Observed::default(), message: None, busy: true };
  spawn_observe(tx);
}

const NEEDS_KITTY: &str = "ketai necesita una terminal con el protocolo de gráficos de kitty \
(kitty, Ghostty, WezTerm, Konsole o herdr dentro de una de ellas).\n\
Para usarlo igual sin carátulas: KETAI_NO_IMAGES=1 ketai";

fn main() -> io::Result<()> {
  let (tx, rx) = mpsc::channel::<Input>();
  let mut app = App::default();
  if let Some(path) = config::path() {
    match Config::load(&path) {
      Ok(c) => app.config = c,
      Err(e) => app.config_error = Some(e),
    }
    app.apply_lyrics_config();
    // Al guardar el archivo, la UI cambia sin reiniciar.
    ketai_tui::watch::spawn(path, tx.clone(), Input::Config);
  }
  app.refresh_theme();
  let mut terminal = ratatui::init();
  // Avisos de foco, para reenviar las caratulas al volver al panel.
  let _ = ratatui::crossterm::execute!(
    io::stdout(),
    ratatui::crossterm::event::EnableFocusChange,
    ratatui::crossterm::event::EnableMouseCapture
  );
  // La pregunta se hace antes de leer teclas: la respuesta llega por stdin.
  let no_images = std::env::var_os("KETAI_NO_IMAGES").is_some_and(|v| v == "1");
  app.images = !no_images && kitty_supported();
  app.covers = covers::CoverCache::with_base(covers::CoverCache::process_base());
  app.karaoke = ketai_tui::karaoke::Karaoke::new(ketai_tui::app::karaoke_ids(covers::CoverCache::process_base()), app.config.lyrics.font.clone());
  app.cell_px = cell_size();
  if !no_images && !app.images {
    ratatui::restore();
    eprintln!("{NEEDS_KITTY}");
    std::process::exit(1);
  }
  let mut client = connect(&tx);
  if client.is_some() {
    on_connected(&mut client, &mut app, &tx);
  } else {
    show_service(&mut app, &tx);
  }

  let keys = tx.clone();
  thread::Builder::new().name("ketai-input".into()).spawn(move || {
    loop {
      let input = match event::read() {
        Ok(Event::Key(k)) if k.kind == KeyEventKind::Press => Input::Key(k),
        Ok(Event::Resize(w, h)) => {
          ketai_tui::log::log(format_args!("resize {w}x{h}"));
          Input::Resize
        }
        // Al volver al panel o a la pestana (herdr, tmux) algunas terminales
        // perdieron las imagenes y lo dibujado.
        Ok(Event::FocusGained) => {
          ketai_tui::log::log("focus gained");
          Input::Focus
        }
        Ok(Event::FocusLost) => {
          ketai_tui::log::log("focus lost");
          continue;
        }
        Ok(Event::Mouse(m)) => {
          if !matches!(m.kind, event::MouseEventKind::Moved) {
            ketai_tui::log::log(format_args!("mouse {:?} at {},{}", m.kind, m.column, m.row));
          }
          Input::Mouse(m)
        }
        Ok(_) => continue,
        Err(_) => return,
      };
      if keys.send(input).is_err() {
        return;
      }
    }
  })?;

  // Un solo thread prepara las caratulas (decodificar, PNG, base64), en
  // orden. Con un thread por caratula, cientos a la vez cargaban el sistema.
  let (prep, prep_rx) = std::sync::mpsc::channel::<Prep>();
  {
    let tx = tx.clone();
    thread::Builder::new().name("ketai-covers".into()).spawn(move || {
      for job in prep_rx {
        let input = match job {
          Prep::File(uri, path, id) => Input::Cover(uri, covers::prepare(&path, id)),
          Prep::Image(uri, img, id) => Input::Cover(uri, Some(covers::prepare_image(img, id))),
          Prep::Dimmed(uri, img, id) => Input::CoverDim(uri, covers::dimmed_transmit(&img, id)),
        };
        if tx.send(input).is_err() {
          return;
        }
      }
    })?;
  }

  let mut last_retry = Instant::now();
  let mut last_refresh = Instant::now();
  let started = Instant::now();
  // Imagenes ubicadas en el ultimo cuadro.
  let mut placed: Vec<kitty::Placement> = Vec::new();
  let mut karaoke_live = false;
  let result = (|| -> io::Result<()> {
    loop {
      app.anim_ms = started.elapsed().as_millis() as u64;
      if app.images && app.lyrics_mode == config::LyricsMode::Karaoke {
        app.karaoke_fonts = app.karaoke.ensure_fonts();
        // El tamano de celda se vuelve a medir en cada cuadro: al arrancar
        // dentro de un multiplexor (herdr) puede no ser el definitivo, y no
        // siempre llega un aviso de cambio de tamano. Con una medida vieja
        // la linea salia chica y corrida del centro.
        let cell = cell_size();
        if cell != app.cell_px {
          ketai_tui::log::log(format_args!("cell size {:?} -> {:?}", app.cell_px, cell));
          app.cell_px = cell;
        }
      }
      terminal.draw(|f| ui::draw(f, &app))?;
      app.hits = ui::take_hits();
      if app.images {
        let frame = ui::take_frame(&app);
        for uri in frame.wanted {
          if let (Some((method, params)), Some(c)) = (app.covers.want(&uri), client.as_mut())
            && let Ok(id) = c.request(&method, params)
          {
            app.covers.sent(id, &uri);
          }
        }
        for uri in frame.wanted_dimmed {
          if let Some((img, id)) = app.covers.want_dimmed(&uri) {
            let _ = prep.send(Prep::Dimmed(uri, img, id));
          }
        }
        // Las que la terminal pudo haber perdido se mandan al dibujarse.
        for uri in frame.resend {
          if let Some((img, id)) = app.covers.restage(&uri) {
            ketai_tui::log::log(format_args!("resend {uri}"));
            let _ = prep.send(Prep::Image(uri, img, id));
          }
        }
        // La linea del karaoke: la primera vez sus imagenes, despues solo
        // el recorte que avanza.
        let slot = ui::take_karaoke();
        karaoke_live = slot.is_some() && app.playing();
        if let Some(cell) = app.cell_px {
          emit(&app.karaoke.frame(slot.as_ref(), cell, app.position_ms()));
        }
        let update = kitty::update(&placed, &frame.placements);
        if !update.is_empty() {
          ketai_tui::log::log(format_args!("place {} images", frame.placements.len()));
        }
        emit(&update);
        placed = frame.placements;
        // Si el multiplexor perdio las ubicaciones sin avisar, se vuelven a
        // poner cada tanto (pocos bytes, sin parpadeo).
        if !update.is_empty() {
          last_refresh = Instant::now();
        } else if !placed.is_empty() && last_refresh.elapsed() >= REFRESH_PLACEMENTS {
          emit(&kitty::refresh(&placed));
          emit(&app.karaoke.refresh());
          last_refresh = Instant::now();
        }
        // Las que no se usan hace rato se borran de la terminal.
        let mut in_use: Vec<String> = frame.on_screen;
        in_use.extend(frame.keep);
        app.covers.touch(&in_use);
        let evicted = app.covers.evict(KEEP_COVERS);
        app.covers.forget(KEEP_IN_MEMORY);
        if !evicted.is_empty() {
          ketai_tui::log::log("evicted old covers");
          emit(&evicted);
        }
      }
      // Con una busqueda esperando, se despierta seguido para mandarla a
      // tiempo.
      // Mientras se pinta una linea del karaoke, unos 30 cuadros por segundo.
      let wait = if app.search_pending() || karaoke_live { 30 } else { 250 };
      let input = rx.recv_timeout(Duration::from_millis(wait)).ok();
      for (method, params) in app.tick(Instant::now()) {
        send(&mut client, &mut app, &method, params);
      }

      // Desde la pantalla de servicio se reintenta conectar cada tanto.
      if matches!(app.screen, Screen::Service { busy: false, .. }) && last_retry.elapsed() >= RETRY {
        last_retry = Instant::now();
        if let Some(c) = connect(&tx) {
          client = Some(c);
          on_connected(&mut client, &mut app, &tx);
        }
      }

      match input {
        None => {}
        // Al cambiar el tamano se vuelven a mandar y ubicar todas las
        // imagenes: algunas terminales (herdr) las pierden.
        Some(input @ (Input::Resize | Input::Focus)) => {
          placed.clear();
          app.karaoke.invalidate();
          if matches!(input, Input::Resize) {
            app.cell_px = cell_size();
          }
          // Al recuperar el foco se redibuja todo: el multiplexor pudo haber
          // borrado el panel y ratatui solo dibuja lo que cambio.
          if matches!(input, Input::Focus) {
            let _ = terminal.clear();
          }
          // Solo se marcan: cada una se manda cuando se vuelve a dibujar.
          if app.images {
            app.covers.invalidate_all();
            ketai_tui::log::log("covers marked stale");
          }
        }
        Some(Input::Mouse(m)) if matches!(app.screen, Screen::Main) => {
          if ketai_tui::log::enabled() && !matches!(m.kind, event::MouseEventKind::Moved) {
            let found = app.hits.iter().rev().find(|(r, _)| m.column >= r.x && m.column < r.x + r.width && m.row >= r.y && m.row < r.y + r.height);
            ketai_tui::log::log(format_args!("  hit {:?}", found.map(|(_, h)| h)));
          }
          for (method, params) in app.on_mouse(m, Instant::now()) {
            send(&mut client, &mut app, &method, params);
          }
        }
        Some(Input::Mouse(_)) => {}
        Some(Input::Key(k)) => {
          if let Screen::Service { observed, .. } = &app.screen {
            let observed = observed.clone();
            match app.service_action(k) {
              Some(Action::Retry) => {
                if let Screen::Service { busy, message, .. } = &mut app.screen {
                  *busy = true;
                  *message = None;
                }
                spawn_observe(&tx);
              }
              Some(action) => {
                if let Screen::Service { busy, message, .. } = &mut app.screen {
                  *busy = true;
                  *message = None;
                }
                spawn_action(&tx, action, observed);
              }
              None => {}
            }
          } else {
            for (method, params) in app.on_key(k) {
              send(&mut client, &mut app, &method, params);
            }
          }
        }
        Some(Input::Daemon(DaemonMsg::Closed)) => {
          client = None;
          show_service(&mut app, &tx);
        }
        Some(Input::Daemon(DaemonMsg::Reply { id, result })) if app.covers.owns(id) => {
          if let Some(job) = app.covers.on_reply(id, result) {
            let _ = prep.send(Prep::File(job.uri, job.path, job.id));
          }
        }
        Some(Input::Config(result)) => {
          match result {
            Ok(c) => {
              app.config = c;
              app.config_error = None;
              app.apply_lyrics_config();
            }
            // Con error se mantiene la configuracion anterior.
            Err(e) => app.config_error = Some(e),
          }
          app.refresh_theme();
        }
        Some(Input::CoverDim(uri, seq)) => {
          emit(&seq);
          app.covers.dimmed_ready(&uri);
        }
        Some(Input::Cover(uri, prepared)) => {
          let seq = app.covers.ready(&uri, prepared);
          ketai_tui::log::log(format_args!("cover sent {} bytes {uri}", seq.len()));
          emit(&seq);
          // Si es la caratula que suena, el tema toma sus colores.
          app.refresh_theme();
        }
        Some(Input::Daemon(msg)) => {
          for (method, params) in app.on_daemon(msg) {
            send(&mut client, &mut app, &method, params);
          }
        }
        Some(Input::Viz(frame)) => app.on_viz(frame),
        Some(Input::Observed(o)) => {
          let state = service::diagnose(&o);
          if let Screen::Service { state: s, observed, busy, .. } = &mut app.screen {
            *s = state;
            *observed = o;
            *busy = false;
          }
          // Lo encontrado puede ser que ya responde: probar enseguida.
          last_retry = Instant::now() - RETRY;
        }
        Some(Input::ActionDone(result)) => {
          if let Screen::Service { message, .. } = &mut app.screen {
            *message = result.err();
          }
          spawn_observe(&tx);
        }
      }
      if app.quit {
        return Ok(());
      }
    }
  })();
  if app.images {
    emit(&kitty::delete_images(&app.covers.ids()));
    emit(&kitty::delete_images(&app.karaoke.ids()));
  }
  let _ = ratatui::crossterm::execute!(
    io::stdout(),
    ratatui::crossterm::event::DisableMouseCapture,
    ratatui::crossterm::event::DisableFocusChange
  );
  ratatui::restore();
  result
}
