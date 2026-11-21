//! Canal de control: socket, framing y loop central.

use std::{
  collections::HashMap,
  fs,
  io::{self, BufRead, BufReader, Read, Write},
  os::unix::{
    fs::{DirBuilderExt, PermissionsExt},
    net::{UnixListener, UnixStream},
  },
  path::{Path, PathBuf},
  sync::mpsc::{self, Receiver, Sender},
  thread,
};

use ketai_protocol::{
  ErrorReply, MAX_LINE_BYTES, PROTOCOL_VERSION, Request, codes, encode_error, encode_result,
  parse_request,
};
use serde_json::{Value, json};

use crate::instance;

type ClientId = u64;

/// Mensajes que llegan al loop central.
enum Message {
  Connected(ClientId, Sender<String>),
  Request(ClientId, Request),
  /// Error de framing o parseo: se responde sin pasar por los handlers.
  Invalid(ClientId, ErrorReply),
  Disconnected(ClientId),
}

/// Lo que el loop central debe hacer despues de atender una peticion.
enum Flow {
  Continue,
  Shutdown,
}

pub fn run() -> io::Result<()> {
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
  let listener = bind_socket(&socket_path)?;
  instance::mark_ready(&lock)?;
  // Los huerfanos se cierran en segundo plano: el daemon ya atiende clientes
  // mientras tanto.
  thread::spawn(instance::close_orphans);

  let (tx, rx) = mpsc::channel();
  thread::spawn(move || accept_loop(listener, tx));

  let result = central_loop(rx);
  // El socket se borra al salir para que los clientes no vean uno muerto.
  let _ = fs::remove_file(&socket_path);
  result
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
  let (out_tx, out_rx) = mpsc::channel::<String>();
  thread::spawn(move || writer_loop(write_half, out_rx));

  if tx.send(Message::Connected(id, out_tx)).is_err() {
    return;
  }
  thread::spawn(move || reader_loop(id, stream, tx));
}

fn writer_loop(mut stream: UnixStream, rx: Receiver<String>) {
  for line in rx {
    if stream.write_all(line.as_bytes()).is_err() {
      break;
    }
  }
  // El canal se cerro: el loop central pidio cerrar la conexion.
  let _ = stream.shutdown(std::net::Shutdown::Both);
}

fn reader_loop(id: ClientId, stream: UnixStream, tx: Sender<Message>) {
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
      let _ = tx.send(Message::Invalid(id, err));
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
    if tx.send(msg).is_err() {
      return;
    }
  }
  let _ = tx.send(Message::Disconnected(id));
}

fn central_loop(rx: Receiver<Message>) -> io::Result<()> {
  let mut clients: HashMap<ClientId, Sender<String>> = HashMap::new();

  for msg in rx {
    match msg {
      Message::Connected(id, out) => {
        clients.insert(id, out);
      }
      Message::Disconnected(id) => {
        // Soltar el Sender cierra el thread escritor y la conexion.
        clients.remove(&id);
      }
      Message::Invalid(id, err) => {
        if let Some(out) = clients.get(&id) {
          let _ = out.send(encode_error(&err));
        }
      }
      Message::Request(id, req) => {
        let (reply, flow) = handle(&req);
        if let Some(out) = clients.get(&id) {
          let line = match reply {
            Ok(result) => encode_result(req.id, result),
            Err(err) => encode_error(&err),
          };
          let _ = out.send(line);
        }
        if let Flow::Shutdown = flow {
          // Al soltar los Sender, cada escritor envia lo pendiente y cierra.
          drop(clients);
          // Pequena espera para que la respuesta salga antes del exit.
          thread::sleep(std::time::Duration::from_millis(50));
          return Ok(());
        }
      }
    }
  }
  Ok(())
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
