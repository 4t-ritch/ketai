//! Cache de caratulas: pedido al daemon (`track.cover` por URI), preparacion
//! y transmision a la terminal.
//!
//! Todo lo pesado (decodificar, achicar, codificar PNG, base64) corre en un
//! thread aparte: el loop de la UI solo escribe lo que ya esta listo. Asi
//! escribir en Buscar no congela la pantalla aunque aparezcan muchas
//! caratulas nuevas.
//!
//! Cada caratula tiene dos imagenes en la terminal: la normal y una atenuada
//! para cuando hay un popup encima. La atenuada se prepara recien cuando
//! hace falta (la mayoria nunca la necesita).

use std::{collections::HashMap, io::Cursor, path::PathBuf};

use image::{RgbImage, imageops::FilterType};
use serde_json::{Value, json};

use crate::{app::Command, config::CoverPalette, kitty, theme::rgb};

/// Lado maximo con el que se transmite una caratula: alcanza para la grande
/// de Reproduciendo y mantiene chico lo que se manda a la terminal.
pub const MAX_SIDE: u32 = 320;
/// Brillo de la version atenuada.
const DIM: f32 = 0.4;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoverState {
  Requested,
  /// Preparandose en otro thread, con sus ids ya reservados.
  Preparing { normal: u32, dimmed: u32 },
  Ready { normal: u32, dimmed: u32, dim: DimState },
  /// La terminal pudo haberla perdido (cambio de tamano, foco) o se borro
  /// para no acumular: se vuelve a mandar cuando se dibuje.
  Stale { normal: u32, dimmed: u32 },
  Missing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DimState {
  NotSent,
  Preparing,
  Sent,
}

/// Caratula lista para la terminal, armada fuera del loop de la UI.
pub struct Prepared {
  pub image: RgbImage,
  pub palette: CoverPalette,
  /// Secuencias del protocolo kitty para transmitir la version normal.
  pub transmit: String,
}

/// Trabajo de preparacion para otro thread.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Job {
  pub uri: String,
  pub path: PathBuf,
  pub id: u32,
}

#[derive(Default)]
pub struct CoverCache {
  covers: HashMap<String, CoverState>,
  /// `track.cover` en vuelo: id -> uri.
  pending: HashMap<u64, String>,
  /// Imagen chica de cada caratula lista (para armar la atenuada).
  images: HashMap<String, RgbImage>,
  palettes: HashMap<String, CoverPalette>,
  next_image: u32,
  /// Ultimo uso de cada caratula (para borrar las viejas de la terminal).
  last_used: HashMap<String, u64>,
  clock: u64,
}

impl CoverCache {
  /// Cache cuyos ids de imagen arrancan despues de `base`. Cada proceso usa
  /// su propio rango para no pisar las imagenes de otro ketai en la misma
  /// terminal.
  pub fn with_base(base: u32) -> Self {
    CoverCache { next_image: base, ..Default::default() }
  }

  /// Rango de ids para este proceso: 2^20 ids a partir de un bloque segun el
  /// pid (los ids chicos quedan para la pregunta de soporte).
  pub fn process_base() -> u32 {
    ((std::process::id() % 4000) + 1) << 20
  }

  /// Algunas terminales y multiplexores (herdr) pierden las imagenes al
  /// cambiar de tamano o de pestana. Todas las listas pasan a `Stale`; cada
  /// una se vuelve a mandar recien cuando se dibuja (`restage`). Mandar todas
  /// de una (cientos) llenaba la memoria de imagenes del multiplexor y
  /// cargaba el sistema.
  pub fn invalidate_all(&mut self) {
    for state in self.covers.values_mut() {
      if let CoverState::Ready { normal, dimmed, .. } = *state {
        *state = CoverState::Stale { normal, dimmed };
      }
    }
  }

  /// Una caratula `Stale` que se dibuja: imagen e id para volver a
  /// prepararla en otro thread, una sola vez.
  pub fn restage(&mut self, uri: &str) -> Option<(RgbImage, u32)> {
    let Some(CoverState::Stale { normal, dimmed }) = self.covers.get(uri).cloned() else { return None };
    let img = self.images.get(uri)?.clone();
    self.covers.insert(uri.to_string(), CoverState::Preparing { normal, dimmed });
    Some((img, normal))
  }

  /// Marca caratulas como recien usadas.
  pub fn touch(&mut self, uris: &[String]) {
    self.clock += 1;
    for u in uris {
      self.last_used.insert(u.clone(), self.clock);
    }
  }

  /// Deja en la terminal como mucho `keep` caratulas: borra las usadas hace
  /// mas tiempo (pasan a `Stale`). Devuelve lo que hay que mandar.
  pub fn evict(&mut self, keep: usize) -> String {
    let mut ready: Vec<(u64, String, u32, u32)> = self
      .covers
      .iter()
      .filter_map(|(u, s)| match s {
        CoverState::Ready { normal, dimmed, .. } => Some((self.last_used.get(u).copied().unwrap_or(0), u.clone(), *normal, *dimmed)),
        _ => None,
      })
      .collect();
    if ready.len() <= keep {
      return String::new();
    }
    ready.sort();
    let drop = ready.len() - keep;
    let mut ids = Vec::new();
    for (_, uri, normal, dimmed) in ready.into_iter().take(drop) {
      self.covers.insert(uri, CoverState::Stale { normal, dimmed });
      ids.extend([normal, dimmed]);
    }
    crate::kitty::delete_images(&ids)
  }

  /// Deja en memoria como mucho `keep` caratulas: olvida las `Stale` usadas
  /// hace mas tiempo (ya no estan en la terminal). Si reaparecen, se piden
  /// de nuevo al daemon.
  pub fn forget(&mut self, keep: usize) {
    let mut stale: Vec<(u64, String)> = self
      .covers
      .iter()
      .filter(|(_, s)| matches!(s, CoverState::Stale { .. }))
      .map(|(u, _)| (self.last_used.get(u).copied().unwrap_or(0), u.clone()))
      .collect();
    let extra = self.images.len().saturating_sub(keep).min(stale.len());
    stale.sort();
    for (_, uri) in stale.into_iter().take(extra) {
      self.covers.remove(&uri);
      self.images.remove(&uri);
      self.palettes.remove(&uri);
      self.last_used.remove(&uri);
    }
  }

  /// Caratulas guardadas en memoria.
  pub fn in_memory(&self) -> usize {
    self.images.len()
  }

  /// Ids de imagen reservados por este cache (para borrarlos al salir).
  pub fn ids(&self) -> Vec<u32> {
    let mut ids: Vec<u32> = self
      .covers
      .values()
      .flat_map(|c| match c {
        CoverState::Preparing { normal, dimmed } | CoverState::Ready { normal, dimmed, .. } | CoverState::Stale { normal, dimmed } => {
          vec![*normal, *dimmed]
        }
        _ => vec![],
      })
      .collect();
    ids.sort_unstable();
    ids
  }

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

  pub fn owns(&self, id: u64) -> bool {
    self.pending.contains_key(&id)
  }

  /// Respuesta a un `track.cover`. Devuelve el trabajo de preparacion, con
  /// el id de imagen ya reservado.
  pub fn on_reply(&mut self, id: u64, result: Result<Value, Value>) -> Option<Job> {
    let uri = self.pending.remove(&id)?;
    match result.ok().and_then(|v| v["path"].as_str().map(PathBuf::from)) {
      Some(path) => {
        let (normal, dimmed) = (self.next_image + 1, self.next_image + 2);
        self.next_image += 2;
        self.covers.insert(uri.clone(), CoverState::Preparing { normal, dimmed });
        Some(Job { uri, path, id: normal })
      }
      None => {
        self.covers.insert(uri, CoverState::Missing);
        None
      }
    }
  }

  pub fn state(&self, uri: &str) -> Option<&CoverState> {
    self.covers.get(uri)
  }

  /// Llego la preparacion (o fallo). Devuelve lo que hay que escribir en la
  /// terminal; es barato: ya viene armado.
  pub fn ready(&mut self, uri: &str, prepared: Option<Prepared>) -> String {
    let (Some(p), Some(CoverState::Preparing { normal, dimmed })) = (prepared, self.covers.get(uri).cloned()) else {
      self.covers.insert(uri.to_string(), CoverState::Missing);
      return String::new();
    };
    self.palettes.insert(uri.to_string(), p.palette);
    self.images.insert(uri.to_string(), p.image);
    self.covers.insert(uri.to_string(), CoverState::Ready { normal, dimmed, dim: DimState::NotSent });
    p.transmit
  }

  /// Prepara una caratula en el mismo thread (para tests y herramientas).
  pub fn decoded(&mut self, uri: &str, img: Option<RgbImage>) -> String {
    let id = match self.covers.get(uri) {
      Some(CoverState::Preparing { normal, .. }) => *normal,
      _ => {
        let (normal, dimmed) = (self.next_image + 1, self.next_image + 2);
        self.next_image += 2;
        self.covers.insert(uri.to_string(), CoverState::Preparing { normal, dimmed });
        normal
      }
    };
    let prepared = img.map(|img| prepare_image(img, id));
    self.ready(uri, prepared)
  }

  /// Hace falta la version atenuada (hay un popup encima). Devuelve la
  /// imagen y el id para prepararla en otro thread, una sola vez.
  pub fn want_dimmed(&mut self, uri: &str) -> Option<(RgbImage, u32)> {
    let Some(CoverState::Ready { dimmed, dim, .. }) = self.covers.get_mut(uri) else { return None };
    if *dim != DimState::NotSent {
      return None;
    }
    *dim = DimState::Preparing;
    Some((self.images.get(uri)?.clone(), *dimmed))
  }

  /// La version atenuada ya se transmitio.
  pub fn dimmed_ready(&mut self, uri: &str) {
    if let Some(CoverState::Ready { dim, .. }) = self.covers.get_mut(uri) {
      *dim = DimState::Sent;
    }
  }

  /// Colores de la caratula de `uri`, si ya se preparo.
  pub fn palette(&self, uri: &str) -> Option<CoverPalette> {
    self.palettes.get(uri).copied()
  }

  /// Id de la imagen a ubicar para `uri`, si esta lista. La atenuada solo
  /// cuando ya se transmitio.
  pub fn image_for(&self, uri: &str, dimmed: bool) -> Option<u32> {
    match self.covers.get(uri) {
      Some(CoverState::Ready { normal, .. }) if !dimmed => Some(*normal),
      Some(CoverState::Ready { dimmed: d, dim: DimState::Sent, .. }) => Some(*d),
      _ => None,
    }
  }
}

/// Lee una caratula y la deja lista para la terminal. Corre en otro thread.
pub fn prepare(path: &std::path::Path, id: u32) -> Option<Prepared> {
  decode(path).map(|img| prepare_image(img, id))
}

fn png(img: &RgbImage) -> Vec<u8> {
  let mut out = Vec::new();
  let _ = image::DynamicImage::ImageRgb8(img.clone()).write_to(&mut Cursor::new(&mut out), image::ImageFormat::Png);
  out
}

/// Arma una caratula ya decodificada. Corre en otro thread.
pub fn prepare_image(image: RgbImage, id: u32) -> Prepared {
  let palette = palette(&image);
  let transmit = kitty::transmit(id, &png(&image));
  Prepared { image, palette, transmit }
}

/// Version atenuada lista para la terminal. Corre en otro thread.
pub fn dimmed_transmit(img: &RgbImage, id: u32) -> String {
  let mut dark = img.clone();
  for p in dark.pixels_mut() {
    for c in p.0.iter_mut() {
      *c = (*c as f32 * DIM) as u8;
    }
  }
  kitty::transmit(id, &png(&dark))
}

/// Lee y recorta una caratula: cuadrado al centro y tamano maximo.
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
  // Una caratula gris no tiene tono: todo queda en grises.
  let sat = s.max(0.55).min(if s < 0.05 { 0.0 } else { 1.0 });
  let soft = (s * 0.35).min(if s < 0.05 { 0.0 } else { 1.0 });
  let hsl = |h: f32, s: f32, l: f32| {
    let [r, g, b] = from_hsl(h.rem_euclid(1.0), s, l);
    rgb(((r * 255.0) as u32) << 16 | ((g * 255.0) as u32) << 8 | (b * 255.0) as u32)
  };
  CoverPalette {
    accent: hsl(h, sat, 0.72),
    background: scale(0.35),
    raised: scale(0.6),
    row: scale(0.5),
    on_accent: hsl(h, sat, 0.12),
    dim: hsl(h, soft, 0.82),
    faint: hsl(h, soft, 0.60),
    spectrum_top: hsl(h + 0.06, sat, 0.70),
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
