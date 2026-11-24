//! Configuracion de la TUI: `$XDG_CONFIG_HOME/ketai/config.toml`
//! (`~/.config/ketai/config.toml`). Ver docs/config.md.
//!
//! TOML: declarativo, con comentarios y secciones. Funciones nuevas se
//! agregan como secciones nuevas sin romper los archivos existentes; una
//! clave desconocida se avisa pero no frena el resto.

use std::path::{Path, PathBuf};

use ratatui::style::Color as RColor;

use crate::theme::Theme;

/// Un color del tema: fijo o tomado de la caratula que suena.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Color {
  Cover,
  Fixed(RColor),
}

#[derive(Debug, Clone, PartialEq)]
pub struct ThemeConfig {
  /// `cover_color`: `Some(true)` toda la UI sale de la caratula y los
  /// colores manuales se ignoran; `Some(false)` solo valen los manuales;
  /// sin la clave, cada color decide (`background`/`accent` pueden ser
  /// "cover").
  pub cover_color: Option<bool>,
  pub background: Color,
  pub accent: Color,
  /// Colores fijos opcionales; `None` deja el valor por defecto.
  pub fixed: Vec<(&'static str, RColor)>,
}

/// Como se muestra la letra en la vista de Letras.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LyricsMode {
  /// Texto de la terminal; resalta la palabra que se canta.
  #[default]
  Classic,
  /// La linea que se canta es una imagen que se pinta por dentro de las
  /// letras a medida que se canta.
  Karaoke,
}

/// `[lyrics]`.
#[derive(Debug, Clone, PartialEq)]
pub struct LyricsConfig {
  pub mode: LyricsMode,
  /// Alto, en filas, de la linea que se canta en modo karaoke.
  pub size: u16,
  /// Fuente para el modo karaoke; `None` la elige fontconfig.
  pub font: Option<std::path::PathBuf>,
}

/// Rango de `[lyrics] size`.
pub const LYRICS_SIZES: std::ops::RangeInclusive<u16> = 2..=8;

impl Default for LyricsConfig {
  fn default() -> Self {
    Self { mode: LyricsMode::Classic, size: 3, font: None }
  }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Config {
  pub theme: ThemeConfig,
  pub lyrics: LyricsConfig,
  /// Atajos globales (`[keys]`).
  pub keys: crate::keys::Keymap,
  /// Claves que no se reconocen (no frenan la carga).
  pub warnings: Vec<String>,
}

impl Default for Config {
  fn default() -> Self {
    Self { theme: ThemeConfig { cover_color: None, background: Color::Cover, accent: Color::Cover, fixed: Vec::new() },
      lyrics: LyricsConfig::default(),
      keys: crate::keys::Keymap::default(),
      warnings: Vec::new(),
    }
  }
}

/// Claves de `[theme]` que solo admiten un color fijo.
const FIXED_KEYS: [&str; 9] = ["foreground", "dim", "faint", "raised", "on_accent", "row", "ok", "error", "spectrum_top"];

/// `#rrggbb`.
fn hex(s: &str) -> Option<RColor> {
  let h = s.strip_prefix('#')?;
  if h.len() != 6 {
    return None;
  }
  let v = u32::from_str_radix(h, 16).ok()?;
  Some(crate::theme::rgb(v))
}

/// `~/` al principio es el home.
fn expand_home(path: &str) -> std::path::PathBuf {
  match (path.strip_prefix("~/"), std::env::var_os("HOME")) {
    (Some(rest), Some(home)) => std::path::Path::new(&home).join(rest),
    _ => std::path::PathBuf::from(path),
  }
}

impl Config {
  pub fn parse(text: &str) -> Result<Config, String> {
    let table: toml::Table = text.parse().map_err(|e: toml::de::Error| e.to_string().trim().to_string())?;
    let mut config = Config::default();
    for (key, value) in &table {
      match key.as_str() {
        "theme" => {
          let theme = value.as_table().ok_or("theme: tiene que ser una sección [theme]")?;
          for (k, v) in theme {
            let name = format!("theme.{k}");
            if k == "cover_color" {
              config.theme.cover_color = Some(v.as_bool().ok_or_else(|| format!("{name}: tiene que ser true o false"))?);
              continue;
            }
            let text = v.as_str().ok_or_else(|| format!("{name}: tiene que ser un texto como \"#rrggbb\""))?;
            match k.as_str() {
              "background" | "accent" => {
                let c = if text == "cover" {
                  Color::Cover
                } else {
                  Color::Fixed(hex(text).ok_or_else(|| format!("{name}: color inválido \"{text}\" (usá \"#rrggbb\" o \"cover\")"))?)
                };
                if k == "background" {
                  config.theme.background = c;
                } else {
                  config.theme.accent = c;
                }
              }
              other => match FIXED_KEYS.iter().find(|f| **f == other) {
                Some(f) => {
                  let c = hex(text).ok_or_else(|| format!("{name}: color inválido \"{text}\" (usá \"#rrggbb\")"))?;
                  config.theme.fixed.push((f, c));
                }
                None => config.warnings.push(format!("{name}: clave desconocida, se ignora")),
              },
            }
          }
        }
        "keys" => {
          let keys = value.as_table().ok_or("keys: tiene que ser una sección [keys]")?;
          for (k, v) in keys {
            let name = format!("keys.{k}");
            let Some(action) = crate::keys::action_named(k) else {
              config.warnings.push(format!("{name}: acción desconocida, se ignora"));
              continue;
            };
            let specs: Vec<&str> = match v {
              toml::Value::String(s) => vec![s.as_str()],
              toml::Value::Array(a) => {
                a.iter().map(|x| x.as_str().ok_or_else(|| format!("{name}: la lista tiene que ser de textos"))).collect::<Result<_, _>>()?
              }
              _ => return Err(format!("{name}: tiene que ser una tecla como \"ctrl+p\" o una lista")),
            };
            let mut parsed = Vec::new();
            for spec in specs {
              parsed.push(crate::keys::Key::parse(spec).ok_or_else(|| format!("{name}: tecla inválida \"{spec}\""))?);
            }
            config.keys.set(action, parsed);
          }
          config.warnings.extend(config.keys.conflicts());
        }
        "lyrics" => {
          let lyrics = value.as_table().ok_or("lyrics: tiene que ser una sección [lyrics]")?;
          for (k, v) in lyrics {
            let name = format!("lyrics.{k}");
            match k.as_str() {
              "mode" => {
                config.lyrics.mode = match v.as_str() {
                  Some("classic") => LyricsMode::Classic,
                  Some("karaoke") => LyricsMode::Karaoke,
                  _ => return Err(format!("{name}: tiene que ser \"classic\" o \"karaoke\"")),
                }
              }
              "size" => {
                let (lo, hi) = (*LYRICS_SIZES.start(), *LYRICS_SIZES.end());
                config.lyrics.size = v
                  .as_integer()
                  .and_then(|n| u16::try_from(n).ok())
                  .filter(|n| LYRICS_SIZES.contains(n))
                  .ok_or_else(|| format!("{name}: tiene que ser un número de {lo} a {hi} (filas)"))?;
              }
              "font" => {
                let path = v.as_str().ok_or_else(|| format!("{name}: tiene que ser la ruta de un archivo de fuente"))?;
                config.lyrics.font = Some(expand_home(path));
              }
              _ => config.warnings.push(format!("{name}: clave desconocida, se ignora")),
            }
          }
        }
        // La lee el daemon (carpeta de playlists).
        "playlists" => {}
        other => config.warnings.push(format!("{other}: sección desconocida, se ignora")),
      }
    }
    Ok(config)
  }

  /// Lee el archivo. Si no existe, la configuracion por defecto.
  pub fn load(path: &Path) -> Result<Config, String> {
    match std::fs::read_to_string(path) {
      Ok(text) => Config::parse(&text),
      Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Config::default()),
      Err(e) => Err(format!("no se pudo leer: {e}")),
    }
  }

  /// Tema final. `cover` son los colores de la caratula que suena (acento,
  /// fondo, fondo elevado), si hay.
  pub fn theme(&self, cover: Option<CoverPalette>) -> Theme {
    match self.theme.cover_color {
      // Toda la UI de la caratula; los manuales no cuentan.
      Some(true) => {
        let mut t = Theme::default();
        if let Some(p) = cover {
          t.bg = p.background;
          t.raised = p.raised;
          t.row = p.row;
          t.accent = p.accent;
          t.on_accent = p.on_accent;
          t.dim = p.dim;
          t.faint = p.faint;
          t.spectrum_top = p.spectrum_top;
        }
        return t;
      }
      // Solo los manuales: "cover" vale el color por defecto.
      Some(false) => return self.manual_theme(None),
      None => {}
    }
    self.manual_theme(cover)
  }

  fn manual_theme(&self, cover: Option<CoverPalette>) -> Theme {
    let mut t = Theme::default();
    for (key, c) in &self.theme.fixed {
      match *key {
        "foreground" => t.fg = *c,
        "dim" => t.dim = *c,
        "faint" => t.faint = *c,
        "raised" => t.raised = *c,
        "on_accent" => t.on_accent = *c,
        "row" => t.row = *c,
        "ok" => t.ok = *c,
        "error" => t.error = *c,
        "spectrum_top" => t.spectrum_top = *c,
        _ => {}
      }
    }
    match (self.theme.background, cover) {
      (Color::Fixed(c), _) => t.bg = c,
      (Color::Cover, Some(p)) => {
        t.bg = p.background;
        // Lo que acompana al fondo tambien sale de la caratula, salvo que
        // este fijado a mano.
        let mine = |k: &str| self.theme.fixed.iter().any(|(f, _)| *f == k);
        if !mine("raised") {
          t.raised = p.raised;
        }
        if !mine("row") {
          t.row = p.row;
        }
        if !mine("dim") {
          t.dim = p.dim;
        }
        if !mine("faint") {
          t.faint = p.faint;
        }
      }
      (Color::Cover, None) => {}
    }
    match (self.theme.accent, cover) {
      (Color::Fixed(c), _) => t.accent = c,
      (Color::Cover, Some(p)) => {
        t.accent = p.accent;
        let mine = |k: &str| self.theme.fixed.iter().any(|(f, _)| *f == k);
        if !mine("on_accent") {
          t.on_accent = p.on_accent;
        }
        if !mine("spectrum_top") {
          t.spectrum_top = p.spectrum_top;
        }
      }
      (Color::Cover, None) => {}
    }
    t
  }
}

/// Colores derivados de una caratula.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CoverPalette {
  pub accent: RColor,
  pub background: RColor,
  pub raised: RColor,
  /// Fila bajo el cursor.
  pub row: RColor,
  /// Texto sobre el acento.
  pub on_accent: RColor,
  /// Textos secundarios, del tono de la caratula.
  pub dim: RColor,
  pub faint: RColor,
  pub spectrum_top: RColor,
}

/// `$XDG_CONFIG_HOME/ketai/config.toml` o `~/.config/ketai/config.toml`.
pub fn path() -> Option<PathBuf> {
  let base = std::env::var_os("XDG_CONFIG_HOME")
    .map(PathBuf::from)
    .filter(|p| p.is_absolute())
    .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
  Some(base.join("ketai").join("config.toml"))
}
