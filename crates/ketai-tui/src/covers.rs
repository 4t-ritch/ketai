//! Cache de caratulas: pedido al daemon (`track.cover` por URI), decodificado
//! y transmision a la terminal. Cada caratula tiene dos imagenes: la normal
//! y una atenuada para cuando hay un popup encima.

use std::{collections::HashMap, io::Cursor, path::PathBuf};

use image::{RgbImage, imageops::FilterType};
use serde_json::{Value, json};

use crate::{app::Command, kitty};

/// Lado maximo con el que se transmite una caratula.
const MAX_SIDE: u32 = 400;
/// Brillo de la version atenuada.
const DIM: f32 = 0.4;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoverState {
  Requested,
  Decoding,
  Ready { normal: u32, dimmed: u32 },
  Missing,
}

#[derive(Default)]
pub struct CoverCache {
  covers: HashMap<String, CoverState>,
  /// `track.cover` en vuelo: id -> uri.
  pending: HashMap<u64, String>,
  next_image: u32,
}

impl CoverCache {
  /// Pide la caratula de `uri` si todavia no se pidio.
  pub fn want(&mut self, uri: &str) -> Option<Command> {
    if self.covers.contains_key(uri) {
      return None;
    }
    self.covers.insert(uri.to_string(), CoverState::Requested);
    Some(("track.cover".into(), json!({"uri": uri})))
  }

  pub fn sent(&mut self, id: u64, uri: &str) {
    self.pending.insert(id, uri.to_string());
  }

  /// Respuesta a un `track.cover`. Devuelve (uri, archivo) a decodificar.
  pub fn on_reply(&mut self, id: u64, result: Result<Value, Value>) -> Option<(String, PathBuf)> {
    let uri = self.pending.remove(&id)?;
    match result.ok().and_then(|v| v["path"].as_str().map(PathBuf::from)) {
      Some(path) => {
        self.covers.insert(uri.clone(), CoverState::Decoding);
        Some((uri, path))
      }
      None => {
        self.covers.insert(uri, CoverState::Missing);
        None
      }
    }
  }

  pub fn owns(&self, id: u64) -> bool {
    self.pending.contains_key(&id)
  }

  pub fn state(&self, uri: &str) -> Option<&CoverState> {
    self.covers.get(uri)
  }

  /// Imagen ya decodificada (o `None` si fallo). Devuelve lo que hay que
  /// escribir en la terminal para transmitirla.
  pub fn decoded(&mut self, uri: &str, img: Option<RgbImage>) -> String {
    let Some(img) = img else {
      self.covers.insert(uri.to_string(), CoverState::Missing);
      return String::new();
    };
    let normal = self.next_image + 1;
    let dimmed = self.next_image + 2;
    self.next_image += 2;
    let mut dark = img.clone();
    for p in dark.pixels_mut() {
      for c in p.0.iter_mut() {
        *c = (*c as f32 * DIM) as u8;
      }
    }
    let mut out = String::new();
    for (id, im) in [(normal, &img), (dimmed, &dark)] {
      let mut png = Vec::new();
      if image::DynamicImage::ImageRgb8(im.clone()).write_to(&mut Cursor::new(&mut png), image::ImageFormat::Png).is_ok() {
        out.push_str(&kitty::transmit(id, &png));
      }
    }
    self.covers.insert(uri.to_string(), CoverState::Ready { normal, dimmed });
    out
  }

  /// Id de la imagen a ubicar para `uri`, si esta lista.
  pub fn image_for(&self, uri: &str, dimmed: bool) -> Option<u32> {
    match self.covers.get(uri) {
      Some(CoverState::Ready { normal, dimmed: d }) => Some(if dimmed { *d } else { *normal }),
      _ => None,
    }
  }
}

/// Lee y prepara una caratula: recorte cuadrado al centro y tamano maximo.
/// Corre fuera del thread de la UI (decodificar puede tardar).
pub fn decode(path: &std::path::Path) -> Option<RgbImage> {
  let img = image::open(path).ok()?.to_rgb8();
  let side = img.width().min(img.height());
  let (x, y) = ((img.width() - side) / 2, (img.height() - side) / 2);
  let square = image::imageops::crop_imm(&img, x, y, side, side).to_image();
  let target = side.min(MAX_SIDE);
  Some(image::imageops::resize(&square, target, target, FilterType::Triangle))
}
