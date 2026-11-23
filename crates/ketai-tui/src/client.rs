//! Cliente del canal de control (`ctl.sock`).
//!
//! Un thread lee las lineas del daemon y las manda por un canal, ya
//! separadas en respuestas y eventos. Las peticiones se escriben desde el
//! thread que llama; cada una lleva un `id` unico por conexion.

use std::{
  io::{self, BufRead, BufReader, Write},
  os::unix::net::UnixStream,
  path::{Path, PathBuf},
  sync::mpsc::Sender,
  thread,
};

use serde_json::{Value, json};

/// Lo que llega del daemon.
#[derive(Debug, Clone, PartialEq)]
pub enum DaemonMsg {
  /// Respuesta a una peticion: `result` o el objeto `error`.
  Reply { id: u64, result: Result<Value, Value> },
  Event { name: String, data: Value },
  /// El daemon cerro la conexion.
  Closed,
}

pub struct Client {
  writer: UnixStream,
  next_id: u64,
}

/// `$XDG_RUNTIME_DIR/ketai/ctl.sock`.
pub fn socket_path() -> Option<PathBuf> {
  let runtime = std::env::var_os("XDG_RUNTIME_DIR").filter(|v| !v.is_empty())?;
  Some(PathBuf::from(runtime).join("ketai").join("ctl.sock"))
}

impl Client {
  /// Conecta y arranca el thread lector. Cada mensaje se convierte con
  /// `wrap` antes de mandarlo, asi el llamador lo mezcla con sus propios
  /// eventos (teclado, ticks) en un solo canal.
  pub fn connect<T: Send + 'static>(path: &Path, tx: Sender<T>, wrap: fn(DaemonMsg) -> T) -> io::Result<Self> {
    let stream = UnixStream::connect(path)?;
    let reader = stream.try_clone()?;
    thread::Builder::new().name("ketai-ipc".into()).spawn(move || {
      for line in BufReader::new(reader).lines() {
        let Ok(line) = line else { break };
        let Some(msg) = parse(&line) else { continue };
        if tx.send(wrap(msg)).is_err() {
          return;
        }
      }
      let _ = tx.send(wrap(DaemonMsg::Closed));
    })?;
    Ok(Self { writer: stream, next_id: 1 })
  }

  /// Envia una peticion y devuelve su `id`; la respuesta llega por el canal.
  pub fn request(&mut self, method: &str, params: Value) -> io::Result<u64> {
    let id = self.next_id;
    self.next_id += 1;
    let mut line = json!({"id": id, "method": method, "params": params}).to_string();
    line.push('\n');
    self.writer.write_all(line.as_bytes())?;
    Ok(id)
  }
}

fn parse(line: &str) -> Option<DaemonMsg> {
  let v: Value = serde_json::from_str(line).ok()?;
  if let Some(name) = v.get("event").and_then(Value::as_str) {
    return Some(DaemonMsg::Event { name: name.to_string(), data: v.get("data").cloned().unwrap_or(Value::Null) });
  }
  let id = v.get("id").and_then(Value::as_u64)?;
  let result = match v.get("error") {
    Some(e) => Err(e.clone()),
    None => Ok(v.get("result").cloned().unwrap_or(Value::Null)),
  };
  Some(DaemonMsg::Reply { id, result })
}
