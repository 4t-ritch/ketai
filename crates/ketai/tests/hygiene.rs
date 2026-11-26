//! Los tests no dejan carpetas en el directorio temporal, aunque fallen.

mod common;

use std::{fs, path::PathBuf, process::Command};

use common::{sweep_dead_runs, temp_runtime_dir};

/// Un pid que no corresponde a ningun proceso.
fn dead_pid() -> u32 {
  (4_000_000..4_194_000).find(|p| unsafe { libc::kill(*p as i32, 0) } != 0).unwrap()
}

#[test]
fn folders_left_by_runs_that_died_are_swept() {
  let stale = std::env::temp_dir().join(format!("ketaid-test-{}-3", dead_pid()));
  fs::create_dir_all(stale.join("data")).unwrap();
  let mine = temp_runtime_dir();
  // Lo hace la primera carpeta de cada corrida; aca se llama directo para
  // no depender del orden de los tests.
  sweep_dead_runs();
  assert!(!stale.exists(), "a dead run's folder is removed");
  assert!(mine.exists(), "this run's folders stay");
}

/// Solo corre como proceso hijo del test de abajo: crea su carpeta y falla
/// antes de borrarla.
#[test]
#[ignore]
fn child_that_fails_before_cleaning_up() {
  let Some(out) = std::env::var_os("KETAID_HYGIENE_OUT") else { return };
  let dir = temp_runtime_dir();
  fs::write(out, dir.to_string_lossy().as_bytes()).unwrap();
  panic!("failing on purpose");
}

#[test]
fn a_failing_test_does_not_leave_its_folder() {
  let out = temp_runtime_dir().join("child-dir.txt");
  let status = Command::new(std::env::current_exe().unwrap())
    .args(["--exact", "child_that_fails_before_cleaning_up", "--ignored", "--test-threads=1"])
    .env("KETAID_HYGIENE_OUT", &out)
    .output()
    .unwrap();
  assert!(!status.status.success(), "the child test fails on purpose");
  let child_dir = PathBuf::from(fs::read_to_string(&out).unwrap());
  assert!(!child_dir.exists(), "{} was left behind", child_dir.display());
}
