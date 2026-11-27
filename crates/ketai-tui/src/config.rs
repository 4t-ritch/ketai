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
  /// `ui_text_cover_color`: con colores de la caratula, true tine textos y
  /// controles con su tono; false los deja blancos y grises.
  pub ui_text_cover_color: bool,
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
  /// Lineas que se ven arriba y abajo de la que se canta.
  pub lines_before: u16,
  pub lines_after: u16,
}

/// Maximo (y valor por defecto) de `lines_before` y `lines_after`.
pub const LINES_BEFORE: u16 = 2;
pub const LINES_AFTER: u16 = 3;

/// Menos de 1 (o negativo) es 1; mas del maximo es el valor por defecto.
fn lines_around(n: i64, max: u16) -> u16 {
  if n < 1 {
    1
  } else if n > max as i64 {
    max
  } else {
    n as u16
  }
}

/// Rango de `[lyrics] size`.
pub const LYRICS_SIZES: std::ops::RangeInclusive<u16> = 2..=8;

impl Default for LyricsConfig {
  fn default() -> Self {
    Self { mode: LyricsMode::Classic, size: 3, font: None, lines_before: LINES_BEFORE, lines_after: LINES_AFTER }
  }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Config {
  pub theme: ThemeConfig,
  pub lyrics: LyricsConfig,
  /// Atajos globales (`[keys]`).
  pub keys: crate::keys::Keymap,
  pub terminal: TerminalConfig,
  /// Claves que no se reconocen (no frenan la carga).
  pub warnings: Vec<String>,
}

impl Default for Config {
  fn default() -> Self {
    Self { theme: ThemeConfig { cover_color: None, ui_text_cover_color: true, background: Color::Cover, accent: Color::Cover, fixed: Vec::new() },
      lyrics: LyricsConfig::default(),
      keys: crate::keys::Keymap::default(),
      terminal: TerminalConfig::default(),
      warnings: Vec::new(),
    }
  }
}

/// Seccion `[terminal]`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TerminalConfig {
  /// Redibujar todo (y reenviar las caratulas) al recuperar el foco. Solo
  /// hace falta en multiplexores que borran el panel al cambiar de pestana.
  pub redraw_on_focus: bool,
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
            if k == "ui_text_cover_color" {
              config.theme.ui_text_cover_color = v.as_bool().ok_or_else(|| format!("{name}: tiene que ser true o false"))?;
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
              "lines_before" | "lines_after" => {
                let n = v.as_integer().ok_or_else(|| format!("{name}: tiene que ser un número (líneas)"))?;
                if k == "lines_before" {
                  config.lyrics.lines_before = lines_around(n, LINES_BEFORE);
                } else {
                  config.lyrics.lines_after = lines_around(n, LINES_AFTER);
                }
              }
              "font" => {
                let path = v.as_str().ok_or_else(|| format!("{name}: tiene que ser la ruta de un archivo de fuente"))?;
                config.lyrics.font = Some(expand_home(path));
              }
              _ => config.warnings.push(format!("{name}: clave desconocida, se ignora")),
            }
          }
        }
        "terminal" => {
          let terminal = value.as_table().ok_or("terminal: tiene que ser una sección [terminal]")?;
          for (k, v) in terminal {
            let name = format!("terminal.{k}");
            match k.as_str() {
              "redraw_on_focus" => {
                config.terminal.redraw_on_focus = v.as_bool().ok_or_else(|| format!("{name}: tiene que ser true o false"))?;
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
          t.fg = p.fg;
          t.dim = p.dim;
          t.faint = p.faint;
          t.spectrum_top = p.spectrum_top;
        }
        return self.text_policy(t, cover.is_some());
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
        if !mine("foreground") {
          t.fg = p.fg;
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
    let from_cover = cover.is_some() && self.theme.background == Color::Cover;
    self.text_policy(t, from_cover)
  }

  /// Con colores de la caratula, `ui_text_cover_color` decide textos y
  /// controles: true, del tono de la caratula (controles con el acento);
  /// false, blancos y grises. Un color fijado a mano siempre gana.
  fn text_policy(&self, mut t: Theme, from_cover: bool) -> Theme {
    let mine = |k: &str| self.theme.cover_color != Some(true) && self.theme.fixed.iter().any(|(f, _)| *f == k);
    if !from_cover {
      t.control = t.fg;
      return t;
    }
    if self.theme.ui_text_cover_color {
      t.control = t.accent;
    } else {
      let d = Theme::default();
      if !mine("foreground") {
        t.fg = d.fg;
      }
      if !mine("dim") {
        t.dim = crate::theme::rgb(0xc8c8c8);
      }
      if !mine("faint") {
        t.faint = crate::theme::rgb(0x8c8c8c);
      }
      t.control = t.fg;
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
  /// Texto principal, claro con un toque del tono de la caratula.
  pub fg: RColor,
  /// Textos secundarios, del tono de la caratula.
  pub dim: RColor,
  pub faint: RColor,
  pub spectrum_top: RColor,
}

/// `$XDG_CONFIG_HOME/ketai/config.toml` o `~/.config/ketai/config.toml`.
/// Valor de `[lyrics] mode` en el archivo.
fn mode_name(mode: LyricsMode) -> &'static str {
  match mode {
    LyricsMode::Classic => "classic",
    LyricsMode::Karaoke => "karaoke",
  }
}

/// El texto del archivo con `[lyrics] mode` cambiado. Solo toca esa linea
/// (o la agrega, con la seccion si falta): los comentarios y el resto quedan
/// como estaban.
pub fn with_lyrics_mode(text: &str, mode: LyricsMode) -> String {
  let value = format!("mode = \"{}\"", mode_name(mode));
  let mut out: Vec<String> = Vec::new();
  let mut section = String::new();
  let mut header_at = None;
  let mut done = false;
  for line in text.lines() {
    let t = line.trim();
    if t.starts_with('[') && !t.starts_with("[[") {
      section = t.trim_start_matches('[').split(']').next().unwrap_or("").trim().to_string();
      if section == "lyrics" {
        header_at = Some(out.len());
      }
    } else if section == "lyrics" && !done && t.split('=').next().is_some_and(|k| k.trim() == "mode") && t.contains('=') {
      let indent = &line[..line.len() - line.trim_start().len()];
      out.push(format!("{indent}{value}"));
      done = true;
      continue;
    }
    out.push(line.to_string());
  }
  match (done, header_at) {
    (true, _) => {}
    (false, Some(at)) => out.insert(at + 1, value),
    (false, None) => {
      if out.last().is_some_and(|l| !l.trim().is_empty()) {
        out.push(String::new());
      }
      out.push("[lyrics]".into());
      out.push(value);
    }
  }
  out.join("\n") + "\n"
}

/// Guarda el modo en el archivo (lo crea si no existe). Escribe en un
/// temporal y lo renombra: nunca queda un archivo a medias.
pub fn save_lyrics_mode(path: &Path, mode: LyricsMode) -> Result<(), String> {
  let text = match std::fs::read_to_string(path) {
    Ok(t) => t,
    Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
    Err(e) => return Err(format!("no se pudo leer: {e}")),
  };
  let dir = path.parent().ok_or("ruta sin carpeta")?;
  std::fs::create_dir_all(dir).map_err(|e| format!("no se pudo crear la carpeta: {e}"))?;
  let tmp = dir.join(".config.toml.ketai-tmp");
  std::fs::write(&tmp, with_lyrics_mode(&text, mode)).map_err(|e| format!("no se pudo escribir: {e}"))?;
  std::fs::rename(&tmp, path).map_err(|e| format!("no se pudo guardar: {e}"))
}

pub fn path() -> Option<PathBuf> {
  let base = std::env::var_os("XDG_CONFIG_HOME")
    .map(PathBuf::from)
    .filter(|p| p.is_absolute())
    .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
  Some(base.join("ketai").join("config.toml"))
}
