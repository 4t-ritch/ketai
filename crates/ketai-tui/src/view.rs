use std::{fs::{self, File}, path::{Path, PathBuf}};
use directories::ProjectDirs;
use rodio::{Decoder, DeviceSinkBuilder, DeviceSinkError, Player, decoder::DecoderError};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum KetaiError {
  #[error("I/O error: {0}")]
  Io(#[from] std::io::Error),

  #[error("Could not open audio device: {0}")]
  AudioDevice(#[from] DeviceSinkError),

  #[error("Could not decode audio: {0}")]
  Decoder(#[from] DecoderError),

  #[error("Could not determine configuration directory")]
  ConfigDirectoryNotFound,
}

type KetaiResult<T> = Result<T, KetaiError>;

pub fn render() -> KetaiResult<()> {
  //Cuando se hace un join es importante tener en cuenta que no se debe de empezar con slash
  //esto porque puede interpretarse como que quieres tomar una nueva path desde raiz
  //es por eso que al hacer join se debe integrar colocando la ruta empezando por el nombre
  //del directorio/fichero y no por un slash que comunmente si pasa en windows.

  // let folder = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
    // .join("samples/");

  let config_dir = config_exist();

  println!("{}",config_dir.unwrap().display().to_string());



  Ok(())
}

fn config_exist() ->  KetaiResult<PathBuf>{
  //Se devuelve la ruta de configuracion de cada OS
  //appdata en win y .config en lnx
  let project_dir = ProjectDirs::from(
    "dev",
    "atomiracle",
    "ketai"
    )
    .ok_or(KetaiError::ConfigDirectoryNotFound)?;

  let config_dir = project_dir.config_dir();
  fs::create_dir_all(config_dir)?;


  let config_file = PathBuf::from(config_dir).join("example.json");
  //Escribe en el archivo
  fs::write(
    &config_file,
    String::new(),
  )?;

  // &config_file
  Ok(config_file)
}


fn entries_by_dir(dir: &Path)-> KetaiResult<()>{
  let entries = fs::read_dir(dir)?;
  for entry in entries {
    let entry = entry?;
    println!("{}", entry.path().display());

    let file = File::open(entry.path())?;
    play(file)?;
  }

  Ok(())
}

fn play(file: File) -> KetaiResult<()>{

  let sink = DeviceSinkBuilder::open_default_sink()?;
  let player = Player::connect_new(sink.mixer());
  let source = Decoder::try_from(file)?;

  player.append(source);
  player.sleep_until_end();

  Ok(())
}
