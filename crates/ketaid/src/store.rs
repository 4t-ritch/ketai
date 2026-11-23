//! Base de datos de ketai (SQLite, `XDG_DATA_HOME/ketai/library.db`).
//!
//! Guarda las carpetas de la biblioteca, las pistas indexadas (con su texto
//! de busqueda ya plegado), las playlists y los favoritos. Cada area tiene un
//! solo thread del daemon con su conexion, y todo cambio es una transaccion:
//! dos clientes que cambian algo al mismo tiempo quedan en fila, nunca se
//! pisan, y un corte a mitad de un cambio no deja la base a medias.
//!
//! La base manda sobre las playlists: los archivos `.m3u8` de la carpeta de
//! playlists son copias que escribe ketai (ver `playlists`).
//!
//! El esquema tiene version (`PRAGMA user_version`); cada version nueva se
//! aplica una vez al abrir.

use std::path::{Path, PathBuf};

use ketai_core::{
  library::{self, Entry},
  metadata::TrackInfo,
};
use rusqlite::{Connection, OptionalExtension, Transaction, params};

/// Pasos del esquema, en orden. El indice + 1 es la version.
const MIGRATIONS: &[&str] = &[
  // 1: carpetas y pistas.
  "CREATE TABLE folders (path TEXT PRIMARY KEY NOT NULL);
   CREATE TABLE tracks (
     path TEXT PRIMARY KEY NOT NULL,
     title TEXT, artist TEXT, album TEXT, album_artist TEXT, genre TEXT,
     year INTEGER, track_number INTEGER, disc_number INTEGER,
     duration_ms INTEGER, has_cover INTEGER NOT NULL DEFAULT 0,
     codec TEXT, sample_rate INTEGER, channels INTEGER,
     search_text TEXT NOT NULL
   );",
  // 2: playlists y favoritos. Una cancion puede estar en varias playlists
  // (y varias veces en la misma). `file_stamp` es la marca del archivo
  // copia que escribio ketai, para notar ediciones a mano.
  "CREATE TABLE playlists (
     id INTEGER PRIMARY KEY,
     name TEXT NOT NULL UNIQUE,
     kind TEXT NOT NULL DEFAULT 'user',
     file_stamp TEXT
   );
   CREATE TABLE playlist_items (
     playlist_id INTEGER NOT NULL REFERENCES playlists(id) ON DELETE CASCADE,
     position INTEGER NOT NULL,
     uri TEXT NOT NULL,
     PRIMARY KEY (playlist_id, position)
   );
   CREATE INDEX playlist_items_uri ON playlist_items(uri);",
  // 3: datos sueltos (por ejemplo, si ya se importo el indice viejo).
  "CREATE TABLE meta (key TEXT PRIMARY KEY NOT NULL, value TEXT);",
];

/// Nombre de la playlist de favoritos.
/// Ruta de la base: `XDG_DATA_HOME/ketai/library.db`.
pub fn default_path() -> Option<std::path::PathBuf> {
  crate::library::data_dir().map(|d| d.join("library.db"))
}

/// Base por defecto, o una en memoria si no hay carpeta de datos o no se
/// puede abrir (el daemon sigue funcionando, sin guardar).
pub fn open_default() -> Store {
  default_path()
    .and_then(|p| Store::open(&p).map_err(|e| eprintln!("ketaid: could not open library.db: {e}")).ok())
    .unwrap_or_else(|| Store::open(Path::new(":memory:")).expect("in-memory database"))
}

pub const FAVORITES: &str = "Favoritos";

/// Playlist de la lista: nombre, tipo y cantidad de pistas.
pub struct PlaylistInfo {
  pub name: String,
  pub kind: String,
  pub count: u64,
}

pub struct Store {
  conn: Connection,
}

impl Store {
  /// Abre (o crea) la base. La pueden abrir varios threads del daemon, cada
  /// uno con su conexion: SQLite ordena las escrituras y el timeout espera
  /// a que termine la otra.
  pub fn open(path: &Path) -> rusqlite::Result<Self> {
    // Abrir y migrar, de a un thread por vez: pasar a WAL no espera el
    // timeout y fallaba con "database is locked" si dos threads abrian a la
    // vez.
    static OPENING: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _guard = OPENING.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(dir) = path.parent() {
      let _ = std::fs::create_dir_all(dir);
    }
    let conn = Connection::open(path)?;
    conn.busy_timeout(std::time::Duration::from_secs(5))?;
    // WAL: un lector nunca bloquea al que escribe.
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    conn.pragma_update(None, "foreign_keys", true)?;
    let mut store = Self { conn };
    store.migrate()?;
    Ok(store)
  }

  /// Importa el indice de versiones anteriores (`library.json`) una sola
  /// vez; ese archivo nunca se borra.
  pub fn import_legacy_once(&mut self, entries: &[Entry], folders: &[PathBuf]) -> rusqlite::Result<()> {
    let tx = self.conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    let done = tx.query_row("SELECT 1 FROM meta WHERE key = 'legacy_imported'", [], |_| Ok(())).optional()?.is_some();
    if !done {
      for f in folders {
        tx.execute("INSERT OR IGNORE INTO folders (path) VALUES (?1)", [path_text(f)])?;
      }
      for e in entries {
        upsert(&tx, e)?;
      }
      tx.execute("INSERT INTO meta (key, value) VALUES ('legacy_imported', '1')", [])?;
    }
    tx.commit()
  }

  /// Aplica las migraciones pendientes. Devuelve true si la base es nueva.
  fn migrate(&mut self) -> rusqlite::Result<()> {
    // Immediate: si dos threads abren a la vez, uno espera al otro y despues
    // ve la version ya aplicada.
    let tx = self.conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    let version: usize = tx.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    for (i, step) in MIGRATIONS.iter().enumerate().skip(version) {
      tx.execute_batch(step)?;
      tx.pragma_update(None, "user_version", i + 1)?;
    }
    // Una base de antes de `meta` ya habia importado el indice viejo.
    if (1..3).contains(&version) {
      tx.execute("INSERT OR IGNORE INTO meta (key, value) VALUES ('legacy_imported', '1')", [])?;
    }
    tx.commit()
  }

  pub fn folders(&self) -> rusqlite::Result<Vec<PathBuf>> {
    let mut st = self.conn.prepare_cached("SELECT path FROM folders ORDER BY rowid")?;
    st.query_map([], |r| r.get::<_, String>(0).map(PathBuf::from))?.collect()
  }

  /// Agrega una carpeta; si ya estaba no hace nada.
  pub fn add_folder(&self, folder: &Path) -> rusqlite::Result<()> {
    self.conn.execute("INSERT OR IGNORE INTO folders (path) VALUES (?1)", [path_text(folder)])?;
    Ok(())
  }

  /// Quita una carpeta y sus pistas, todo junto. False si no estaba.
  pub fn remove_folder(&mut self, folder: &Path) -> rusqlite::Result<bool> {
    let tx = self.conn.transaction()?;
    let removed = tx.execute("DELETE FROM folders WHERE path = ?1", [path_text(folder)])? > 0;
    if removed {
      delete_under(&tx, folder)?;
    }
    tx.commit()?;
    Ok(removed)
  }

  /// Cantidad de pistas bajo `folder`.
  pub fn count_under(&self, folder: &Path) -> rusqlite::Result<u64> {
    let (lo, hi) = range(folder);
    self.conn.query_row("SELECT count(*) FROM tracks WHERE path >= ?1 AND path < ?2", params![lo, hi], |r| r.get(0))
  }

  /// Reemplaza lo indexado bajo cada carpeta por `entries`, en una sola
  /// transaccion: quien busque ve el indice viejo o el nuevo, nunca uno a
  /// medias.
  pub fn replace(&mut self, roots: &[PathBuf], entries: &[Entry]) -> rusqlite::Result<()> {
    let tx = self.conn.transaction()?;
    for r in roots {
      delete_under(&tx, r)?;
    }
    for e in entries {
      upsert(&tx, e)?;
    }
    tx.commit()
  }

  pub fn playlists(&self) -> rusqlite::Result<Vec<PlaylistInfo>> {
    let mut st = self.conn.prepare_cached(
      "SELECT name, kind, (SELECT count(*) FROM playlist_items i WHERE i.playlist_id = p.id) FROM playlists p ORDER BY name",
    )?;
    st.query_map([], |r| Ok(PlaylistInfo { name: r.get(0)?, kind: r.get(1)?, count: r.get(2)? }))?.collect()
  }

  /// URIs de una playlist, en orden. None si no existe.
  pub fn items(&self, name: &str) -> rusqlite::Result<Option<Vec<String>>> {
    items(&self.conn, name)
  }

  /// Marca del archivo copia que se escribio por ultima vez.
  pub fn file_stamp(&self, name: &str) -> rusqlite::Result<Option<String>> {
    Ok(self.conn.query_row("SELECT file_stamp FROM playlists WHERE name = ?1", [name], |r| r.get(0)).optional()?.flatten())
  }

  pub fn set_file_stamp(&self, name: &str, stamp: Option<&str>) -> rusqlite::Result<()> {
    self.conn.execute("UPDATE playlists SET file_stamp = ?2 WHERE name = ?1", params![name, stamp])?;
    Ok(())
  }

  /// Cambia varias playlists juntas, en una transaccion que toma la base
  /// para escribir desde el principio: lo que lee `change` no puede cambiar
  /// por otro lado antes de guardar. `change` recibe las playlists pedidas
  /// (None si no existe) y deja el contenido nuevo; si devuelve error no se
  /// guarda nada.
  pub fn update<T, E>(
    &mut self,
    names: &[&str],
    change: impl FnOnce(&mut Vec<Option<Vec<String>>>) -> Result<T, E>,
  ) -> Result<Result<T, E>, rusqlite::Error> {
    let tx = self.conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    let mut lists = names.iter().map(|n| items(&tx, n)).collect::<rusqlite::Result<Vec<_>>>()?;
    let before: Vec<bool> = lists.iter().map(Option::is_some).collect();
    let out = match change(&mut lists) {
      Ok(v) => v,
      Err(e) => return Ok(Err(e)),
    };
    for ((name, list), existed) in names.iter().zip(&lists).zip(before) {
      match list {
        Some(uris) => {
          if !existed {
            tx.execute("INSERT INTO playlists (name, kind) VALUES (?1, ?2)", params![name, kind_of(name)])?;
          }
          let pid: i64 = tx.query_row("SELECT id FROM playlists WHERE name = ?1", [name], |r| r.get(0))?;
          tx.execute("DELETE FROM playlist_items WHERE playlist_id = ?1", [pid])?;
          let mut st = tx.prepare_cached("INSERT INTO playlist_items (playlist_id, position, uri) VALUES (?1, ?2, ?3)")?;
          for (i, uri) in uris.iter().enumerate() {
            st.execute(params![pid, i as i64, uri])?;
          }
        }
        None if existed => {
          tx.execute("DELETE FROM playlists WHERE name = ?1", [name])?;
        }
        None => {}
      }
    }
    tx.commit()?;
    Ok(Ok(out))
  }

  /// Cambia el nombre. False si `from` no existe; error de unicidad si `to`
  /// ya existe.
  pub fn rename(&self, from: &str, to: &str) -> rusqlite::Result<bool> {
    Ok(self.conn.execute("UPDATE playlists SET name = ?2, file_stamp = NULL WHERE name = ?1", params![from, to])? > 0)
  }

  /// Playlists (sin favoritos) que tienen la URI, y si es favorita.
  pub fn memberships(&self, uri: &str) -> rusqlite::Result<(Vec<String>, bool)> {
    let mut st = self.conn.prepare_cached(
      "SELECT DISTINCT p.name, p.kind FROM playlists p JOIN playlist_items i ON i.playlist_id = p.id WHERE i.uri = ?1 ORDER BY p.name",
    )?;
    let mut names = Vec::new();
    let mut favorite = false;
    for row in st.query_map([uri], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))? {
      let (name, kind) = row?;
      if kind == "favorites" {
        favorite = true;
      } else {
        names.push(name);
      }
    }
    Ok((names, favorite))
  }

  /// Pistas cuyo texto contiene todas las palabras (ya plegadas), ordenadas
  /// por ruta.
  pub fn search(&self, words: &[String], limit: u64) -> rusqlite::Result<Vec<Entry>> {
    let mut sql = String::from("SELECT * FROM tracks WHERE 1");
    for i in 0..words.len() {
      sql.push_str(&format!(" AND instr(search_text, ?{}) > 0", i + 1));
    }
    sql.push_str(&format!(" ORDER BY path LIMIT {limit}"));
    let mut st = self.conn.prepare(&sql)?;
    st.query_map(rusqlite::params_from_iter(words), entry_from_row)?.collect()
  }
}

fn kind_of(name: &str) -> &'static str {
  if name == FAVORITES { "favorites" } else { "user" }
}

fn items(conn: &Connection, name: &str) -> rusqlite::Result<Option<Vec<String>>> {
  let Some(pid) = conn.query_row("SELECT id FROM playlists WHERE name = ?1", [name], |r| r.get::<_, i64>(0)).optional()? else {
    return Ok(None);
  };
  let mut st = conn.prepare_cached("SELECT uri FROM playlist_items WHERE playlist_id = ?1 ORDER BY position")?;
  Ok(Some(st.query_map([pid], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?))
}

fn path_text(p: &Path) -> String {
  p.to_string_lossy().into_owned()
}

/// Rango de rutas que cuelgan de `folder` (`folder/` hasta `folder0`, el
/// caracter siguiente a `/`). Asi `/m/a` no incluye a `/m/ab`.
fn range(folder: &Path) -> (String, String) {
  let base = path_text(folder).trim_end_matches('/').to_string();
  (format!("{base}/"), format!("{base}0"))
}

fn delete_under(tx: &Transaction, folder: &Path) -> rusqlite::Result<()> {
  let (lo, hi) = range(folder);
  tx.execute("DELETE FROM tracks WHERE path >= ?1 AND path < ?2", params![lo, hi])?;
  Ok(())
}

fn upsert(tx: &Transaction, e: &Entry) -> rusqlite::Result<()> {
  let i = &e.info;
  tx.prepare_cached(
    "INSERT OR REPLACE INTO tracks (path, title, artist, album, album_artist, genre, year, track_number, disc_number,
       duration_ms, has_cover, codec, sample_rate, channels, search_text)
     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
  )?
  .execute(params![
    path_text(&e.path),
    i.title,
    i.artist,
    i.album,
    i.album_artist,
    i.genre,
    i.year,
    i.track_number,
    i.disc_number,
    i.duration_ms,
    i.has_cover,
    i.codec,
    i.sample_rate,
    i.channels,
    library::search_text(e),
  ])?;
  Ok(())
}

fn entry_from_row(r: &rusqlite::Row) -> rusqlite::Result<Entry> {
  Ok(Entry {
    path: PathBuf::from(r.get::<_, String>("path")?),
    info: TrackInfo {
      title: r.get("title")?,
      artist: r.get("artist")?,
      album: r.get("album")?,
      album_artist: r.get("album_artist")?,
      genre: r.get("genre")?,
      year: r.get("year")?,
      track_number: r.get("track_number")?,
      disc_number: r.get("disc_number")?,
      duration_ms: r.get("duration_ms")?,
      has_cover: r.get("has_cover")?,
      codec: r.get("codec")?,
      sample_rate: r.get("sample_rate")?,
      channels: r.get("channels")?,
    },
  })
}

