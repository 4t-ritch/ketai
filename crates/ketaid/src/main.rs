//! ketaid: daemon de ketai.
//!
//! Arquitectura (pensada para portarse a C con pthreads):
//! - Un thread acepta conexiones en `ctl.sock`.
//! - Cada cliente tiene un thread lector y un thread escritor.
//! - Todas las peticiones llegan por un canal a un unico loop central, que es
//!   el unico que toca el estado. No hace falta ningun mutex sobre el estado.

mod instance;
mod library;
mod lyrics;
mod mpris;
mod outputs;
mod player;
mod playlists;
mod server;
mod session;
mod systemd;
mod viz;

use std::process::ExitCode;

/// Hace que el stream de audio publique quien es: nombre de aplicacion y
/// PID. El plugin ALSA de PipeWire no publica el PID por su cuenta, y sin el
/// no se puede reconocer el stream propio para moverlo de salida.
///
/// Se llama al principio de main, antes de crear cualquier thread: cambiar
/// variables de entorno con otros threads corriendo no es seguro.
fn identify_audio_stream() {
  let ours = format!(
    "application.name = ketai application.process.id = {} media.role = Music",
    std::process::id()
  );
  // Si el usuario ya definio PIPEWIRE_PROPS, se agregan las propias.
  let value = match std::env::var("PIPEWIRE_PROPS") {
    Ok(existing) if existing.trim_end().ends_with('}') => {
      let trimmed = existing.trim_end();
      format!("{} {ours} }}", &trimmed[..trimmed.len() - 1])
    }
    _ => format!("{{ {ours} }}"),
  };
  // SAFETY: todavia no hay otros threads (ver arriba).
  unsafe { std::env::set_var("PIPEWIRE_PROPS", value) };
}

fn main() -> ExitCode {
  identify_audio_stream();
  match server::run() {
    Ok(()) => ExitCode::SUCCESS,
    Err(err) => {
      eprintln!("ketaid: {err}");
      ExitCode::FAILURE
    }
  }
}
