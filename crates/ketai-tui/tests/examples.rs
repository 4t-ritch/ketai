//! Los ejemplos de examples/ se leen sin errores ni avisos, y cada uno hace
//! lo que dice.

use std::{fs, path::PathBuf};

use ketai_tui::{config::Config, keys::Action, theme::Theme};

fn examples() -> PathBuf {
  PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples")
}

fn load(rel: &str) -> Config {
  let path = examples().join(rel);
  let text = fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
  let c = Config::parse(&text).unwrap_or_else(|e| panic!("{rel}: {e}"));
  assert!(c.warnings.is_empty(), "{rel}: {:?}", c.warnings);
  c
}

#[test]
fn every_example_loads_without_errors_or_warnings() {
  let mut n = 0;
  for dir in ["keys", "theme"] {
    for entry in fs::read_dir(examples().join(dir)).unwrap() {
      let p = entry.unwrap().path();
      if p.extension().is_some_and(|e| e == "toml") {
        load(&format!("{dir}/{}", p.file_name().unwrap().to_string_lossy()));
        n += 1;
      }
    }
  }
  // El ejemplo general, en la raiz de examples/.
  load("config.toml");
  n += 1;
  assert!(n >= 9, "found {n} examples");
}

#[test]
fn the_defaults_examples_match_the_real_defaults() {
  assert_eq!(load("keys/defaults.toml").keys, Config::default().keys);
  assert_eq!(load("theme/defaults.toml").theme(None), Theme::default());
}

#[test]
fn the_general_example_has_every_section() {
  let text = fs::read_to_string(examples().join("config.toml")).unwrap();
  for section in ["[theme]", "[keys]", "[playlists]"] {
    assert!(text.contains(section), "missing {section}");
  }
  assert_eq!(load("config.toml").theme.cover_color, Some(true));
}

#[test]
fn examples_do_what_they_say() {
  assert_eq!(load("theme/cover.toml").theme.cover_color, Some(true));
  assert_eq!(load("theme/dark.toml").theme.cover_color, Some(false));
  let media = load("keys/media.toml");
  assert!(media.keys.keys(Action::Toggle).iter().all(|k| !k.is_text()), "media keys are never plain letters");
}

#[test]
fn every_action_and_its_default_keys_are_documented() {
  // Las tablas de acciones de la documentacion (ingles y espanol) tienen
  // cada accion configurable con sus teclas por defecto: si se agrega una
  // accion o cambia una tecla, este test lo exige en los cuatro documentos.
  let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
  for doc in ["docs/config.md", "docs/es/config.md", "examples/keys/README.md", "examples/keys/README.es.md"] {
    let text = std::fs::read_to_string(root.join(doc)).unwrap();
    for (name, _, keys, _) in ketai_tui::keys::ACTIONS {
      let row = text.lines().find(|l| l.starts_with('|') && l.contains(&format!("`{name}`"))).unwrap_or_else(|| panic!("{doc}: {name} is not in the table"));
      for k in *keys {
        assert!(row.contains(&format!("`{k}`")), "{doc}: the row of {name} does not show its default key {k:?}: {row}");
      }
    }
  }
  // El ejemplo con todos los valores por defecto los tiene todos.
  let defaults = std::fs::read_to_string(root.join("examples/keys/defaults.toml")).unwrap();
  for (name, _, _, _) in ketai_tui::keys::ACTIONS {
    assert!(defaults.lines().any(|l| l.starts_with(&format!("{name} ="))), "examples/keys/defaults.toml: {name} is missing");
  }
}
