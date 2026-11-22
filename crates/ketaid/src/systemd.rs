//! Integracion con systemd sin libsystemd: el protocolo es simple y se
//! implementa con llamadas basicas (asi se traduce directo a C).
//!
//! - Socket activation: systemd pasa los sockets ya abiertos desde el fd 3
//!   (`SD_LISTEN_FDS_START`). `LISTEN_FDS` dice cuantos son y `LISTEN_PID`
//!   para que proceso son; si el pid no es el nuestro, no son nuestros.
//! - Notificacion: se envia un datagrama de texto (`READY=1`) al socket unix
//!   de `NOTIFY_SOCKET`. Si empieza con `@` es un socket abstracto.

use std::{
  io,
  os::{
    fd::{FromRawFd, RawFd},
    linux::net::SocketAddrExt,
    unix::net::{SocketAddr, UnixDatagram, UnixListener},
  },
};

const LISTEN_FDS_START: RawFd = 3;

/// Sockets heredados, en el orden del `.socket`: primero `ctl.sock`,
/// despues `viz.sock`.
pub struct Inherited {
  pub ctl: UnixListener,
  pub viz: Option<UnixListener>,
}

/// Toma los sockets que paso systemd, si los hay y son para este proceso.
/// Borra las variables para que los procesos hijos no los reclamen.
pub fn listen_fds() -> Option<Inherited> {
  let pid = std::env::var("LISTEN_PID").ok()?.parse::<u32>().ok();
  let count = std::env::var("LISTEN_FDS").ok()?.parse::<i32>().ok();
  // SAFETY: se llama al arrancar, antes de crear threads.
  unsafe {
    std::env::remove_var("LISTEN_PID");
    std::env::remove_var("LISTEN_FDS");
    std::env::remove_var("LISTEN_FDNAMES");
  }
  if pid != Some(std::process::id()) {
    return None;
  }
  let count = count.filter(|&n| n >= 1)?;
  // Cada socket se identifica por su posicion: si uno falta, los demas no
  // se corren de lugar (viz nunca se atiende como ctl).
  let viz = if count >= 2 { take_fd(LISTEN_FDS_START + 1) } else { None };
  for extra in 2..count {
    // SAFETY: fd que systemd nos dio y nadie mas usa.
    unsafe { libc::close(LISTEN_FDS_START + extra) };
  }
  let ctl = take_fd(LISTEN_FDS_START)?;
  Some(Inherited { ctl, viz })
}

/// Toma el fd si es un socket; marca close-on-exec como pide systemd.
fn take_fd(fd: RawFd) -> Option<UnixListener> {
  // SAFETY: solo consulta y modifica flags de un fd que systemd nos dio.
  unsafe {
    let mut st: libc::stat = std::mem::zeroed();
    if libc::fstat(fd, &mut st) != 0 || st.st_mode & libc::S_IFMT != libc::S_IFSOCK {
      eprintln!("ketaid: inherited fd {fd} is not a socket; ignored");
      return None;
    }
    libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC);
    Some(UnixListener::from_raw_fd(fd))
  }
}

/// Envia un aviso a systemd (`READY=1`, `STOPPING=1`). Sin `NOTIFY_SOCKET`
/// no hace nada: el daemon corre fuera de systemd.
pub fn notify(state: &str) {
  let Some(target) = std::env::var_os("NOTIFY_SOCKET").filter(|v| !v.is_empty()) else { return };
  let result = (|| -> io::Result<()> {
    let sock = UnixDatagram::unbound()?;
    let bytes = target.as_encoded_bytes();
    let addr = match bytes.strip_prefix(b"@") {
      Some(name) => SocketAddr::from_abstract_name(name)?,
      None => SocketAddr::from_pathname(&target)?,
    };
    sock.send_to_addr(state.as_bytes(), &addr)?;
    Ok(())
  })();
  if let Err(e) = result {
    eprintln!("ketaid: could not notify systemd: {e}");
  }
}
