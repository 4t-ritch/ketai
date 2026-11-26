//! Binario `ketai`: la TUI. Vive en este paquete para que `cargo install
//! ketai` instale el daemon y la TUI juntos.

fn main() -> std::io::Result<()> {
  ketai_tui::run::run()
}
