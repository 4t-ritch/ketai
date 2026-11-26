//! Metodos `output.*` y el evento `output.changed`.

use std::{sync::mpsc::Sender, thread};

use ketai_core::{
  engine::Backend,
  output::{self, Event, OutputHandle, Snapshot},
};
use ketai_protocol::{ErrorReply, Request, codes};
use serde_json::{Value, json};

use crate::{player::PlayerService, server::Message};

pub struct OutputService {
  handle: OutputHandle,
  snapshot: Snapshot,
  /// Dispositivo elegido por el usuario; `None` sigue el default.
  selected: Option<String>,
  /// Se restauro una seleccion de la sesion y todavia no se comprobo que el
  /// dispositivo exista.
  verify_restored: bool,
  /// Ultimo `output.changed` publicado, para no repetirlo.
  published: Value,
}

impl OutputService {
  pub fn new(tx: Sender<Message>) -> Self {
    let (events_tx, events_rx) = std::sync::mpsc::channel();
    let handle = output::spawn(Backend::from_env(), events_tx);
    thread::spawn(move || {
      for ev in events_rx {
        if tx.send(Message::Output(ev)).is_err() {
          return;
        }
      }
    });
    let mut service = Self {
      handle,
      snapshot: Snapshot::default(),
      selected: None,
      verify_restored: false,
      published: Value::Null,
    };
    service.published = service.changed_json();
    service
  }

  pub fn handles(method: &str) -> bool {
    method.starts_with("output.")
  }

  pub fn handle(&mut self, req: &Request, player: &mut PlayerService) -> Result<Value, ErrorReply> {
    let id = req.id;
    match req.method.as_str() {
      "output.list" => Ok(json!({"devices": self.devices_json()})),
      "output.set" => {
        let target = match req.params.get("id") {
          Some(Value::Null) => None,
          Some(Value::String(s)) => Some(s.clone()),
          _ => return Err(ErrorReply::new(Some(id), codes::INVALID_PARAMS, "id must be a string or null")),
        };
        if let Some(t) = &target
          && !self.snapshot.devices.iter().any(|d| d.id == *t)
        {
          return Err(ErrorReply::new(Some(id), 1004, "device not found"));
        }
        self.select(target, player);
        Ok(json!({}))
      }
      _ => Err(ErrorReply::new(Some(id), codes::METHOD_NOT_FOUND, "method not found")),
    }
  }

  /// Restaura la seleccion guardada. Si el dispositivo ya no existe, al
  /// conocer la lista se vuelve al default.
  pub fn restore(&mut self, saved: Option<String>, player: &mut PlayerService) {
    self.verify_restored = saved.is_some();
    self.select(saved, player);
  }

  pub fn on_event(&mut self, event: Event, player: &mut PlayerService) {
    match event {
      Event::Changed(snapshot) => {
        self.snapshot = snapshot;
        if self.verify_restored {
          self.verify_restored = false;
          let missing = self.selected.as_ref().is_some_and(|s| !self.snapshot.devices.iter().any(|d| d.id == *s));
          if missing {
            self.select(None, player);
          }
        }
      }
      Event::Unavailable(_) => {}
    }
  }

  fn select(&mut self, target: Option<String>, player: &mut PlayerService) {
    self.selected = target.clone();
    self.handle.set_target(target.clone());
    player.set_output(target);
  }

  /// Datos de `output.changed` si cambio algo desde la ultima vez.
  pub fn take_event(&mut self) -> Option<Value> {
    let now = self.changed_json();
    if now == self.published {
      return None;
    }
    self.published = now.clone();
    Some(now)
  }

  fn changed_json(&self) -> Value {
    json!({"devices": self.devices_json(), "active": self.selected})
  }

  fn devices_json(&self) -> Vec<Value> {
    self
      .snapshot
      .devices
      .iter()
      .map(|d| {
        json!({
          "id": d.id,
          "name": d.name,
          "is_default": self.snapshot.default.as_deref() == Some(d.id.as_str()),
        })
      })
      .collect()
  }
}
