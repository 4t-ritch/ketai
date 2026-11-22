//! Lector del canal del visualizador (`viz.sock`, docs/protocol.md
//! seccion 7). Un thread lee el header y despues frame por frame.

use std::{
  io::{self, Read},
  os::unix::net::UnixStream,
  path::{Path, PathBuf},
  sync::mpsc::Sender,
  thread,
};

pub fn socket_path() -> Option<PathBuf> {
  crate::client::socket_path().map(|p| p.with_file_name("viz.sock"))
}

/// Conecta y manda cada frame (`bins` valores 0..1) convertido con `wrap`.
/// El thread termina cuando el daemon cierra o el receptor se va.
pub fn spawn<T: Send + 'static>(path: &Path, tx: Sender<T>, wrap: fn(Vec<f32>) -> T) -> io::Result<()> {
  let mut stream = UnixStream::connect(path)?;
  thread::Builder::new().name("ketai-viz".into()).spawn(move || {
    let mut header = [0u8; 12];
    if stream.read_exact(&mut header).is_err() || &header[..4] != b"KTV1" {
      return;
    }
    let bins = u32::from_le_bytes([header[4], header[5], header[6], header[7]]) as usize;
    let mut raw = vec![0u8; bins * 4];
    while stream.read_exact(&mut raw).is_ok() {
      let frame = raw.chunks_exact(4).map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect();
      if tx.send(wrap(frame)).is_err() {
        return;
      }
    }
  })?;
  Ok(())
}
