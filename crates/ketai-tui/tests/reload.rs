//! Recarga en vivo de config.toml: escribir, reemplazar (como hacen vim o
//! helix) y borrar el archivo.

use std::{fs, sync::mpsc, time::Duration};

use ketai_tui::{
  config::{Color, Config},
  theme::rgb,
  watch,
};

fn next(rx: &mpsc::Receiver<Result<Config, String>>) -> Result<Config, String> {
  rx.recv_timeout(Duration::from_secs(3)).expect("a reload")
}

#[test]
fn saving_the_file_reloads_it() {
  let dir = std::env::temp_dir().join(format!("ketai-reload-{}", std::process::id()));
  let _ = fs::remove_dir_all(&dir);
  fs::create_dir_all(&dir).unwrap();
  let path = dir.join("config.toml");
  let (tx, rx) = mpsc::channel();
  watch::spawn(path.clone(), tx, |c| c);

  // Aparece el archivo.
  fs::write(&path, "[theme]\naccent = \"#ff0000\"\n").unwrap();
  assert_eq!(next(&rx).unwrap().theme.accent, Color::Fixed(rgb(0xff0000)));

  // El editor escribe un temporal y lo renombra encima.
  let tmp = dir.join("config.toml.swp");
  fs::write(&tmp, "[theme]\naccent = \"#00ff00\"\n").unwrap();
  fs::rename(&tmp, &path).unwrap();
  assert_eq!(next(&rx).unwrap().theme.accent, Color::Fixed(rgb(0x00ff00)));

  // Un error llega como error (la app mantiene el tema anterior).
  fs::write(&path, "[theme]\naccent = \"verde\"\n").unwrap();
  assert!(next(&rx).unwrap_err().contains("theme.accent"));

  // Borrarlo vuelve a los valores por defecto.
  fs::remove_file(&path).unwrap();
  assert_eq!(next(&rx).unwrap(), Config::default());

  // Sin cambios no llega nada.
  assert!(rx.recv_timeout(Duration::from_millis(1200)).is_err());
  let _ = fs::remove_dir_all(&dir);
}
