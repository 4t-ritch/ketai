//! Base de datos de ketai (SQLite, `XDG_DATA_HOME/ketai/library.db`).
//!
//! Guarda las carpetas de la biblioteca y las pistas indexadas, con su texto
//! de busqueda ya plegado. Un solo thread del daemon es dueno de la conexion
//! y todo cambio pasa por el en una transaccion: dos clientes que cambian
//! algo al mismo tiempo quedan en fila, nunca se pisan, y un corte a mitad
//! de un cambio no deja la base a medias.
//!
//! El esquema tiene version (`PRAGMA user_version`); cada version nueva se
//! aplica una vez al abrir.

use std::path::{Path, PathBuf};

use ketai_core::{
  library::{self, Entry},
  metadata::TrackInfo,
};
use rusqlite::{Connection, Transaction, params};

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
];

pub struct Store {
  conn: Connection,
}

impl Store {
  /// Abre (o crea) la base. La primera vez importa `legacy`, el indice de
  /// versiones anteriores, si existe; ese archivo nunca se borra.
  pub fn open(path: &Path, legacy: Option<(Vec<Entry>, Vec<PathBuf>)>) -> rusqlite::Result<Self> {
    if let Some(dir) = path.parent() {
      let _ = std::fs::create_dir_all(dir);
    }
    let conn = Connection::open(path)?;
    // WAL: un lector nunca bloquea al que escribe. El timeout cubre a otro
    // proceso que tenga la base abierta un momento.
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    conn.pragma_update(None, "foreign_keys", true)?;
    conn.busy_timeout(std::time::Duration::from_secs(5))?;
    let mut store = Self { conn };
    let fresh = store.migrate()?;
    if fresh && let Some((entries, folders)) = legacy {
      let tx = store.conn.transaction()?;
      for f in &folders {
        tx.execute("INSERT OR IGNORE INTO folders (path) VALUES (?1)", [path_text(f)])?;
      }
      for e in &entries {
        upsert(&tx, e)?;
      }
      tx.commit()?;
    }
    Ok(store)
  }

  /// Aplica las migraciones pendientes. Devuelve true si la base es nueva.
  fn migrate(&mut self) -> rusqlite::Result<bool> {
    let version: usize = self.conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    let tx = self.conn.transaction()?;
    for (i, step) in MIGRATIONS.iter().enumerate().skip(version) {
      tx.execute_batch(step)?;
      tx.pragma_update(None, "user_version", i + 1)?;
    }
    tx.commit()?;
    Ok(version == 0)
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

