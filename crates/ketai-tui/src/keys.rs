//! Atajos configurables (`[keys]` de config.toml).
//!
//! Cada accion tiene teclas por defecto; en config.toml se reemplazan por
//! accion con un texto ("ctrl+p") o una lista (["n", "ctrl+right"]). Las
//! acciones de Letras (`lyrics_*`) valen solo en esa vista y ahi tienen
//! prioridad sobre las globales. Las demas teclas propias de cada vista (h /
//! l en Biblioteca) son fijas. Ctrl+C siempre sale.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
  Toggle,
  Next,
  Previous,
  SeekForward,
  SeekBack,
  VolumeUp,
  VolumeDown,
  Mute,
  Shuffle,
  Repeat,
  Devices,
  Folders,
  Favorite,
  AddToPlaylist,
  Actions,
  Quit,
  NextTab,
  PrevTab,
  TabNowPlaying,
  TabLyrics,
  TabLibrary,
  TabSearch,
  // Solo en Letras.
  LyricsLayer,
  LyricsTranslation,
  LyricsOffsetDown,
  LyricsOffsetUp,
  LyricsMode,
  LyricsEstimate,
}

impl Action {
  /// Accion de la vista de Letras (no global).
  pub fn is_lyrics(self) -> bool {
    matches!(
      self,
      Action::LyricsLayer
        | Action::LyricsTranslation
        | Action::LyricsOffsetDown
        | Action::LyricsOffsetUp
        | Action::LyricsMode
        | Action::LyricsEstimate
    )
  }
}

/// Nombre en config.toml, accion, teclas por defecto y descripcion.
pub const ACTIONS: &[(&str, Action, &[&str], &str)] = &[
  ("toggle", Action::Toggle, &["space"], "Reproducir o pausar"),
  ("next", Action::Next, &["n"], "Siguiente"),
  ("previous", Action::Previous, &["p"], "Anterior"),
  ("seek_forward", Action::SeekForward, &["right"], "Adelantar 5 s"),
  ("seek_back", Action::SeekBack, &["left"], "Retroceder 5 s"),
  ("volume_up", Action::VolumeUp, &["+", "="], "Subir volumen"),
  ("volume_down", Action::VolumeDown, &["-"], "Bajar volumen"),
  ("mute", Action::Mute, &["m"], "Silenciar"),
  ("shuffle", Action::Shuffle, &["s"], "Aleatorio"),
  ("repeat", Action::Repeat, &["r"], "Repetición"),
  ("devices", Action::Devices, &["d"], "Salida de audio"),
  ("folders", Action::Folders, &["f"], "Carpetas de la biblioteca"),
  ("favorite", Action::Favorite, &["*"], "Marcar como favorita"),
  ("add_to_playlist", Action::AddToPlaylist, &["P"], "Agregar a una playlist"),
  ("actions", Action::Actions, &["ctrl+p", "ctrl+k", "/"], "Acciones y búsqueda rápida"),
  ("quit", Action::Quit, &["q"], "Salir"),
  ("next_tab", Action::NextTab, &["tab"], "Pestaña siguiente"),
  ("prev_tab", Action::PrevTab, &["shift+tab"], "Pestaña anterior"),
  ("tab_now_playing", Action::TabNowPlaying, &["1"], "Ir a Reproduciendo"),
  ("tab_lyrics", Action::TabLyrics, &["2"], "Ir a Letras"),
  ("tab_library", Action::TabLibrary, &["3"], "Ir a Biblioteca"),
  ("tab_search", Action::TabSearch, &["4"], "Ir a Buscar"),
  ("lyrics_layer", Action::LyricsLayer, &["o"], "Letras: cambiar la capa principal"),
  ("lyrics_translation", Action::LyricsTranslation, &["t"], "Letras: mostrar u ocultar la traducción"),
  ("lyrics_offset_down", Action::LyricsOffsetDown, &["["], "Letras: offset -50 ms"),
  ("lyrics_offset_up", Action::LyricsOffsetUp, &["]"], "Letras: offset +50 ms"),
  ("lyrics_mode", Action::LyricsMode, &["m"], "Letras: modo clásico o karaoke"),
  ("lyrics_estimate", Action::LyricsEstimate, &["w"], "Letras: estimar tiempos por palabra"),
];

/// Una tecla con sus modificadores.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Key {
  pub code: KeyCode,
  pub ctrl: bool,
  pub alt: bool,
}

impl Key {
  /// "ctrl+p", "alt+x", "space", "left", "shift+tab", "f5", "P", "+".
  pub fn parse(spec: &str) -> Option<Key> {
    let mut key = Key { code: KeyCode::Null, ctrl: false, alt: false };
    let mut shift = false;
    let parts: Vec<&str> = spec.split('+').collect();
    // "+" solo, o "ctrl++": el ultimo tramo vacio es la tecla "+".
    let (mods, last) = match parts.as_slice() {
      [.., "", ""] => (&parts[..parts.len() - 2], "+"),
      [rest @ .., last] => (rest, *last),
      [] => return None,
    };
    for m in mods {
      match m.to_ascii_lowercase().as_str() {
        "ctrl" | "control" => key.ctrl = true,
        "alt" => key.alt = true,
        "shift" => shift = true,
        _ => return None,
      }
    }
    key.code = match last.to_ascii_lowercase().as_str() {
      "space" => KeyCode::Char(' '),
      "enter" => KeyCode::Enter,
      "esc" | "escape" => KeyCode::Esc,
      "tab" if shift => KeyCode::BackTab,
      "tab" => KeyCode::Tab,
      "backspace" => KeyCode::Backspace,
      "left" => KeyCode::Left,
      "right" => KeyCode::Right,
      "up" => KeyCode::Up,
      "down" => KeyCode::Down,
      "home" => KeyCode::Home,
      "end" => KeyCode::End,
      "pageup" => KeyCode::PageUp,
      "pagedown" => KeyCode::PageDown,
      f if f.len() > 1 && f.starts_with('f') && f[1..].parse::<u8>().is_ok_and(|n| (1..=12).contains(&n)) => {
        KeyCode::F(f[1..].parse().ok()?)
      }
      _ => {
        let mut chars = last.chars();
        let c = chars.next()?;
        if chars.next().is_some() {
          return None;
        }
        // Con Ctrl la terminal no distingue mayusculas.
        let c = if key.ctrl { c.to_ascii_lowercase() } else if shift { c.to_ascii_uppercase() } else { c };
        KeyCode::Char(c)
      }
    };
    Some(key)
  }

  pub fn matches(&self, ev: &KeyEvent) -> bool {
    let ctrl = ev.modifiers.contains(KeyModifiers::CONTROL);
    let alt = ev.modifiers.contains(KeyModifiers::ALT);
    let code = match ev.code {
      KeyCode::Char(c) if ctrl => KeyCode::Char(c.to_ascii_lowercase()),
      c => c,
    };
    code == self.code && ctrl == self.ctrl && alt == self.alt
  }

  /// Si la tecla escribe texto (sin Ctrl ni Alt): no vale mientras se
  /// escribe en Buscar.
  pub fn is_text(&self) -> bool {
    !self.ctrl && !self.alt && matches!(self.code, KeyCode::Char(_))
  }

  /// Texto para mostrar ("Ctrl+P", "Espacio").
  pub fn label(&self) -> String {
    let mut s = String::new();
    if self.ctrl {
      s.push_str("Ctrl+");
    }
    if self.alt {
      s.push_str("Alt+");
    }
    s.push_str(&match self.code {
      KeyCode::Char(' ') => "Espacio".to_string(),
      KeyCode::Char(c) if self.ctrl || self.alt => c.to_ascii_uppercase().to_string(),
      KeyCode::Char(c) => c.to_string(),
      KeyCode::Enter => "Enter".into(),
      KeyCode::Esc => "Esc".into(),
      KeyCode::Tab => "Tab".into(),
      KeyCode::BackTab => "Shift+Tab".into(),
      KeyCode::Left => "←".into(),
      KeyCode::Right => "→".into(),
      KeyCode::Up => "↑".into(),
      KeyCode::Down => "↓".into(),
      KeyCode::F(n) => format!("F{n}"),
      other => format!("{other:?}"),
    });
    s
  }
}

/// Teclas de cada accion.
#[derive(Debug, Clone, PartialEq)]
pub struct Keymap {
  bindings: Vec<(Action, Vec<Key>)>,
}

impl Default for Keymap {
  fn default() -> Self {
    let bindings = ACTIONS
      .iter()
      .map(|(_, action, keys, _)| (*action, keys.iter().filter_map(|k| Key::parse(k)).collect()))
      .collect();
    Self { bindings }
  }
}

impl Keymap {
  /// Reemplaza las teclas de una accion.
  pub fn set(&mut self, action: Action, keys: Vec<Key>) {
    if let Some((_, k)) = self.bindings.iter_mut().find(|(a, _)| *a == action) {
      *k = keys;
    }
  }

  /// Accion de una tecla. `typing`: se esta escribiendo, solo valen las
  /// teclas con Ctrl o Alt.
  pub fn action(&self, ev: &KeyEvent, typing: bool) -> Option<Action> {
    self
      .bindings
      .iter()
      .filter(|(a, _)| !a.is_lyrics())
      .find(|(_, keys)| keys.iter().any(|k| k.matches(ev) && !(typing && k.is_text())))
      .map(|(a, _)| *a)
  }

  /// Accion de Letras de una tecla (se consulta antes que las globales
  /// cuando esa vista esta abierta).
  pub fn lyrics_action(&self, ev: &KeyEvent) -> Option<Action> {
    self.bindings.iter().filter(|(a, _)| a.is_lyrics()).find(|(_, keys)| keys.iter().any(|k| k.matches(ev))).map(|(a, _)| *a)
  }

  pub fn keys(&self, action: Action) -> &[Key] {
    self.bindings.iter().find(|(a, _)| *a == action).map(|(_, k)| k.as_slice()).unwrap_or(&[])
  }

  /// Teclas repetidas entre acciones, para avisar.
  pub fn conflicts(&self) -> Vec<String> {
    let mut out = Vec::new();
    for (i, (a, keys)) in self.bindings.iter().enumerate() {
      for (b, other) in &self.bindings[i + 1..] {
        // Una tecla de Letras puede repetir una global: en esa vista manda
        // la de Letras (la global sigue en las demas).
        if a.is_lyrics() != b.is_lyrics() {
          continue;
        }
        for k in keys {
          if other.contains(k) {
            out.push(format!("keys: {} está en {} y {}", k.label(), name(*a), name(*b)));
          }
        }
      }
    }
    out
  }
}

pub fn name(action: Action) -> &'static str {
  ACTIONS.iter().find(|(_, a, _, _)| *a == action).map(|(n, _, _, _)| *n).unwrap_or("?")
}

pub fn action_named(name: &str) -> Option<Action> {
  ACTIONS.iter().find(|(n, _, _, _)| *n == name).map(|(_, a, _, _)| *a)
}
