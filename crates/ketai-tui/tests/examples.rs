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
