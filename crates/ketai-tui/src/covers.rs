//! Cache de caratulas: pedido al daemon (`track.cover` por URI), decodificado
//! y transmision a la terminal. Cada caratula tiene dos imagenes: la normal
//! y una atenuada para cuando hay un popup encima.

use std::{collections::HashMap, io::Cursor, path::PathBuf};

use image::{RgbImage, imageops::FilterType};
use serde_json::{Value, json};

use crate::{app::Command, config::CoverPalette, kitty, theme::rgb};

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
  /// Colores derivados de cada caratula.
  palettes: HashMap<String, CoverPalette>,
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
    self.palettes.insert(uri.to_string(), palette(&img));
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

  /// Colores de la caratula de `uri`, si ya se decodifico.
  pub fn palette(&self, uri: &str) -> Option<CoverPalette> {
    self.palettes.get(uri).copied()
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

/// Colores de una caratula: el promedio oscurecido para el fondo y el mismo
/// tono, claro y saturado, para el acento (asi se lee sobre el fondo).
fn palette(img: &RgbImage) -> CoverPalette {
  let (mut r, mut g, mut b) = (0u64, 0u64, 0u64);
  for p in img.pixels() {
    r += p[0] as u64;
    g += p[1] as u64;
    b += p[2] as u64;
  }
  let n = (img.width() as u64 * img.height() as u64).max(1);
  let avg = [(r / n) as f32 / 255.0, (g / n) as f32 / 255.0, (b / n) as f32 / 255.0];
  let scale = |k: f32| rgb(((avg[0] * k * 255.0) as u32) << 16 | ((avg[1] * k * 255.0) as u32) << 8 | (avg[2] * k * 255.0) as u32);
  let (h, s, _) = to_hsl(avg);
  // Una caratula gris no tiene tono: el acento queda casi blanco.
  let [ar, ag, ab] = from_hsl(h, s.max(0.55).min(if s < 0.05 { 0.0 } else { 1.0 }), 0.72);
  CoverPalette {
    accent: rgb(((ar * 255.0) as u32) << 16 | ((ag * 255.0) as u32) << 8 | (ab * 255.0) as u32),
    background: scale(0.35),
    raised: scale(0.6),
  }
}

fn to_hsl([r, g, b]: [f32; 3]) -> (f32, f32, f32) {
  let (max, min) = (r.max(g).max(b), r.min(g).min(b));
  let l = (max + min) / 2.0;
  if max == min {
    return (0.0, 0.0, l);
  }
  let d = max - min;
  let s = if l > 0.5 { d / (2.0 - max - min) } else { d / (max + min) };
  let h = if max == r {
    (g - b) / d + if g < b { 6.0 } else { 0.0 }
  } else if max == g {
    (b - r) / d + 2.0
  } else {
    (r - g) / d + 4.0
  };
  (h / 6.0, s, l)
}

fn from_hsl(h: f32, s: f32, l: f32) -> [f32; 3] {
  if s == 0.0 {
    return [l, l, l];
  }
  let q = if l < 0.5 { l * (1.0 + s) } else { l + s - l * s };
  let p = 2.0 * l - q;
  let f = |mut t: f32| {
    if t < 0.0 {
      t += 1.0;
    }
    if t > 1.0 {
      t -= 1.0;
    }
    if t < 1.0 / 6.0 {
      p + (q - p) * 6.0 * t
    } else if t < 0.5 {
      q
    } else if t < 2.0 / 3.0 {
      p + (q - p) * (2.0 / 3.0 - t) * 6.0
    } else {
      p
    }
  };
  [f(h + 1.0 / 3.0), f(h), f(h - 1.0 / 3.0)]
}
