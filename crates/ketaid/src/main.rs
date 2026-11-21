//! ketaid: daemon de ketai.
//!
//! Arquitectura (pensada para portarse a C con pthreads):
//! - Un thread acepta conexiones en `ctl.sock`.
//! - Cada cliente tiene un thread lector y un thread escritor.
//! - Todas las peticiones llegan por un canal a un unico loop central, que es
//!   el unico que toca el estado. No hace falta ningun mutex sobre el estado.

mod instance;
mod player;
mod server;
mod session;

use std::process::ExitCode;

fn main() -> ExitCode {
  match server::run() {
    Ok(()) => ExitCode::SUCCESS,
    Err(err) => {
      eprintln!("ketaid: {err}");
      ExitCode::FAILURE
    }
  }
}
