//! Cola de reproduccion: logica pura, sin I/O ni audio.
//!
//! Los indices se cuentan desde 0. `current` es la pista seleccionada (la
//! que suena, esta en pausa o quedo detenida).

use std::{collections::HashSet, path::PathBuf};

use crate::metadata::TrackInfo;

#[derive(Debug, Clone, PartialEq)]
pub struct Track {
  pub id: String,
  pub uri: String,
  /// Ruta local; `None` para origenes remotos.
  pub path: Option<PathBuf>,
  pub info: TrackInfo,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Repeat {
  Off,
  Track,
  Queue,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueueError {
  OutOfRange,
  Duplicate,
}

/// Que paso con la pista actual al quitar elementos.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CurrentAfterRemove {
  /// La pista actual no se quito (su indice puede haber cambiado).
  Kept,
  /// Se quito; `current` ahora apunta a la siguiente que quedo.
  Replaced,
  /// Se quito y no queda ninguna pista despues.
  Gone,
}

pub struct Queue {
  tracks: Vec<Track>,
  current: Option<usize>,
  repeat: Repeat,
  shuffle: bool,
  /// Ids ya reproducidos en el ciclo actual de shuffle, en orden.
  history: Vec<String>,
  rng: u64,
}

impl Default for Queue {
  fn default() -> Self {
    Self::new(0x9e37_79b9_7f4a_7c15)
  }
}

impl Queue {
  /// `seed` inicializa el generador del shuffle (los tests usan uno fijo).
  pub fn new(seed: u64) -> Self {
    Self {
      tracks: Vec::new(),
      current: None,
      repeat: Repeat::Off,
      shuffle: false,
      history: Vec::new(),
      rng: seed.max(1),
    }
  }

  pub fn tracks(&self) -> &[Track] {
    &self.tracks
  }

  pub fn current(&self) -> Option<usize> {
    self.current
  }

  pub fn current_track(&self) -> Option<&Track> {
    self.current.map(|i| &self.tracks[i])
  }

  pub fn repeat(&self) -> Repeat {
    self.repeat
  }

  pub fn set_repeat(&mut self, repeat: Repeat) {
    self.repeat = repeat;
  }

  pub fn shuffle(&self) -> bool {
    self.shuffle
  }

  pub fn set_shuffle(&mut self, shuffle: bool) {
    self.shuffle = shuffle;
    self.reset_history();
  }

  pub fn is_empty(&self) -> bool {
    self.tracks.is_empty()
  }

  pub fn len(&self) -> usize {
    self.tracks.len()
  }

  /// Inserta las pistas juntas a partir de `position` (al final si es None).
  pub fn add(&mut self, tracks: Vec<Track>, position: Option<usize>) -> Result<usize, QueueError> {
    let at = position.unwrap_or(self.tracks.len());
    if at > self.tracks.len() {
      return Err(QueueError::OutOfRange);
    }
    let count = tracks.len();
    self.tracks.splice(at..at, tracks);
    if let Some(cur) = self.current
      && cur >= at
    {
      self.current = Some(cur + count);
    }
    Ok(count)
  }

  /// Quita las pistas en `indexes` (contados antes de quitar nada).
  pub fn remove(&mut self, indexes: &[usize]) -> Result<CurrentAfterRemove, QueueError> {
    let mut seen = HashSet::new();
    for &i in indexes {
      if i >= self.tracks.len() {
        return Err(QueueError::OutOfRange);
      }
      if !seen.insert(i) {
        return Err(QueueError::Duplicate);
      }
    }

    let old_current = self.current;
    let mut kept = Vec::with_capacity(self.tracks.len() - indexes.len());
    let mut new_current = None;
    let mut current_removed = false;
    for (i, track) in std::mem::take(&mut self.tracks).into_iter().enumerate() {
      if seen.contains(&i) {
        if Some(i) == old_current {
          current_removed = true;
        }
        let id = track.id;
        self.history.retain(|h| *h != id);
        continue;
      }
      // La siguiente pista que queda despues de la actual quitada.
      if Some(i) == old_current || (current_removed && new_current.is_none()) {
        new_current = Some(kept.len());
      }
      kept.push(track);
    }
    self.tracks = kept;
    self.current = new_current;

    Ok(match (old_current, current_removed, new_current) {
      (None, _, _) | (Some(_), false, _) => CurrentAfterRemove::Kept,
      (Some(_), true, Some(_)) => CurrentAfterRemove::Replaced,
      (Some(_), true, None) => CurrentAfterRemove::Gone,
    })
  }

  /// Saca la pista en `from` y la inserta en `to`.
  pub fn move_track(&mut self, from: usize, to: usize) -> Result<(), QueueError> {
    if from >= self.tracks.len() || to >= self.tracks.len() {
      return Err(QueueError::OutOfRange);
    }
    let track = self.tracks.remove(from);
    self.tracks.insert(to, track);
    if let Some(cur) = self.current {
      self.current = Some(if cur == from {
        to
      } else if from < cur && to >= cur {
        cur - 1
      } else if from > cur && to <= cur {
        cur + 1
      } else {
        cur
      });
    }
    Ok(())
  }

  pub fn clear(&mut self) {
    self.tracks.clear();
    self.current = None;
    self.reset_history();
  }

  pub fn jump(&mut self, index: usize) -> Result<(), QueueError> {
    if index >= self.tracks.len() {
      return Err(QueueError::OutOfRange);
    }
    self.select(index);
    Ok(())
  }

  /// Selecciona la primera pista si no hay ninguna seleccionada.
  pub fn select_first(&mut self) -> Option<usize> {
    if self.current.is_none() && !self.tracks.is_empty() {
      self.select(0);
    }
    self.current
  }

  /// Pista siguiente pedida por el usuario. Ignora `Repeat::Track`.
  #[allow(clippy::should_implement_trait)]
  /// Devuelve None si no hay siguiente (y no cambia la seleccion).
  pub fn next(&mut self) -> Option<usize> {
    let next = if self.shuffle { self.next_shuffled() } else { self.next_in_order() }?;
    self.select(next);
    Some(next)
  }

  /// Pista siguiente cuando la actual termina sola.
  pub fn next_on_end(&mut self) -> Option<usize> {
    if self.repeat == Repeat::Track {
      return self.current;
    }
    self.next()
  }

  /// Pista anterior. None si no hay anterior.
  pub fn previous(&mut self) -> Option<usize> {
    let cur = self.current?;
    let prev = if self.shuffle {
      // Se vuelve por el historial del ciclo actual.
      self.history.pop();
      let id = self.history.last()?.clone();
      self.tracks.iter().position(|t| t.id == id)?
    } else if cur > 0 {
      cur - 1
    } else if self.repeat == Repeat::Queue {
      self.tracks.len() - 1
    } else {
      return None;
    };
    self.current = Some(prev);
    Some(prev)
  }

  fn next_in_order(&self) -> Option<usize> {
    let cur = self.current?;
    if cur + 1 < self.tracks.len() {
      Some(cur + 1)
    } else if self.repeat == Repeat::Queue && !self.tracks.is_empty() {
      Some(0)
    } else {
      None
    }
  }

  fn next_shuffled(&mut self) -> Option<usize> {
    let mut pending = self.unvisited();
    if pending.is_empty() {
      if self.repeat != Repeat::Queue || self.tracks.len() < 2 {
        return None;
      }
      // Nuevo ciclo: todas menos la actual.
      self.reset_history();
      pending = self.unvisited();
    }
    let pick = (self.random() % pending.len() as u64) as usize;
    Some(pending[pick])
  }

  fn unvisited(&self) -> Vec<usize> {
    (0..self.tracks.len())
      .filter(|&i| Some(i) != self.current && !self.history.contains(&self.tracks[i].id))
      .collect()
  }

  fn select(&mut self, index: usize) {
    self.current = Some(index);
    let id = self.tracks[index].id.clone();
    if self.history.last() != Some(&id) {
      self.history.push(id);
    }
  }

  fn reset_history(&mut self) {
    self.history.clear();
    if let Some(track) = self.current_track() {
      let id = track.id.clone();
      self.history.push(id);
    }
  }

  /// xorshift64: suficiente para mezclar, sin dependencias.
  fn random(&mut self) -> u64 {
    let mut x = self.rng;
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    self.rng = x;
    x
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn track(n: u32) -> Track {
    Track {
      id: format!("t_{n}"),
      uri: format!("file:///{n}.ogg"),
      path: None,
      info: TrackInfo::default(),
    }
  }

  fn queue_of(n: u32) -> Queue {
    let mut q = Queue::new(42);
    q.add((0..n).map(track).collect(), None).unwrap();
    q
  }

  fn ids(q: &Queue) -> Vec<String> {
    q.tracks().iter().map(|t| t.id.clone()).collect()
  }

  #[test]
  fn add_inserts_in_order_and_shifts_current() {
    let mut q = queue_of(3);
    q.jump(1).unwrap();
    assert_eq!(q.add(vec![track(8), track(9)], Some(0)), Ok(2));
    assert_eq!(ids(&q), ["t_8", "t_9", "t_0", "t_1", "t_2"]);
    assert_eq!(q.current(), Some(3));
    assert_eq!(q.add(vec![track(7)], Some(9)), Err(QueueError::OutOfRange));
  }

  #[test]
  fn remove_counts_indexes_before_removing() {
    let mut q = queue_of(5);
    assert_eq!(q.remove(&[3, 1]), Ok(CurrentAfterRemove::Kept));
    assert_eq!(ids(&q), ["t_0", "t_2", "t_4"]);
  }

  #[test]
  fn remove_is_atomic_on_bad_indexes() {
    let mut q = queue_of(3);
    assert_eq!(q.remove(&[0, 7]), Err(QueueError::OutOfRange));
    assert_eq!(q.remove(&[1, 1]), Err(QueueError::Duplicate));
    assert_eq!(q.len(), 3);
  }

  #[test]
  fn removing_current_selects_next_remaining() {
    let mut q = queue_of(4);
    q.jump(1).unwrap();
    assert_eq!(q.remove(&[1, 2]), Ok(CurrentAfterRemove::Replaced));
    assert_eq!(q.current_track().unwrap().id, "t_3");
  }

  #[test]
  fn removing_current_with_nothing_after_is_gone() {
    let mut q = queue_of(3);
    q.jump(2).unwrap();
    assert_eq!(q.remove(&[2]), Ok(CurrentAfterRemove::Gone));
    assert_eq!(q.current(), None);
  }

  #[test]
  fn move_keeps_current_on_same_track() {
    let mut q = queue_of(4);
    q.jump(2).unwrap();
    q.move_track(0, 3).unwrap();
    assert_eq!(ids(&q), ["t_1", "t_2", "t_3", "t_0"]);
    assert_eq!(q.current_track().unwrap().id, "t_2");
    q.move_track(1, 0).unwrap();
    assert_eq!(q.current_track().unwrap().id, "t_2");
    assert_eq!(q.move_track(0, 4), Err(QueueError::OutOfRange));
  }

  #[test]
  fn next_stops_at_end_unless_repeat_queue() {
    let mut q = queue_of(2);
    q.select_first();
    assert_eq!(q.next(), Some(1));
    assert_eq!(q.next(), None);
    q.set_repeat(Repeat::Queue);
    assert_eq!(q.next(), Some(0));
  }

  #[test]
  fn repeat_track_only_applies_on_end() {
    let mut q = queue_of(2);
    q.select_first();
    q.set_repeat(Repeat::Track);
    assert_eq!(q.next_on_end(), Some(0));
    assert_eq!(q.next(), Some(1));
  }

  #[test]
  fn previous_goes_back_or_wraps_with_repeat_queue() {
    let mut q = queue_of(3);
    q.select_first();
    assert_eq!(q.previous(), None);
    q.set_repeat(Repeat::Queue);
    assert_eq!(q.previous(), Some(2));
    assert_eq!(q.previous(), Some(1));
  }

  #[test]
  fn shuffle_visits_every_track_once_per_cycle() {
    let mut q = queue_of(6);
    q.set_shuffle(true);
    q.select_first();
    let mut seen = vec![q.current_track().unwrap().id.clone()];
    while let Some(i) = q.next() {
      seen.push(q.tracks()[i].id.clone());
    }
    seen.sort();
    seen.dedup();
    assert_eq!(seen.len(), 6, "each track exactly once before stopping");
  }

  #[test]
  fn shuffle_previous_walks_history() {
    let mut q = queue_of(5);
    q.set_shuffle(true);
    q.select_first();
    let a = q.current_track().unwrap().id.clone();
    q.next().unwrap();
    let b = q.current_track().unwrap().id.clone();
    q.next().unwrap();
    q.previous().unwrap();
    assert_eq!(q.current_track().unwrap().id, b);
    q.previous().unwrap();
    assert_eq!(q.current_track().unwrap().id, a);
  }

  #[test]
  fn shuffle_with_repeat_queue_starts_new_cycle() {
    let mut q = queue_of(3);
    q.set_shuffle(true);
    q.set_repeat(Repeat::Queue);
    q.select_first();
    for _ in 0..10 {
      assert!(q.next().is_some());
    }
  }
}
