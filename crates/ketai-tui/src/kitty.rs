//! Protocolo de graficos de kitty
//! (https://sw.kovidgoyal.net/kitty/graphics-protocol/).
//!
//! Las imagenes se transmiten una vez por id y despues se ubican sobre un
//! rectangulo de celdas. Se ubican por debajo del fondo de las celdas (z
//! menor a -1073741824): solo se ven donde la celda tiene el fondo por
//! defecto. Asi un popup, que tiene fondo propio, las tapa sin calculos.

use std::fmt::Write as _;

use ratatui::layout::Rect;

/// Debajo de los fondos de celda (ver el modulo).
pub const Z_BELOW_BACKGROUND: i32 = -1_073_741_825;
const CHUNK: usize = 4096;

/// Una imagen ubicada en la pantalla.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Placement {
  pub image: u32,
  pub area: Rect,
}

fn base64(data: &[u8]) -> String {
  const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
  let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
  for c in data.chunks(3) {
    let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
    out.push(T[(n >> 18) as usize & 63] as char);
    out.push(T[(n >> 12) as usize & 63] as char);
    out.push(if c.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
    out.push(if c.len() > 2 { T[n as usize & 63] as char } else { '=' });
  }
  out
}

/// Transmite un PNG con el id dado, en trozos de 4096 bytes de base64.
pub fn transmit(id: u32, png: &[u8]) -> String {
  let data = base64(png);
  let chunks: Vec<&str> = data.as_bytes().chunks(CHUNK).map(|c| std::str::from_utf8(c).unwrap_or("")).collect();
  let mut out = String::new();
  for (i, chunk) in chunks.iter().enumerate() {
    let more = if i + 1 < chunks.len() { 1 } else { 0 };
    if i == 0 {
      let _ = write!(out, "\x1b_Ga=t,f=100,i={id},q=2,m={more};{chunk}\x1b\\");
    } else {
      let _ = write!(out, "\x1b_Gm={more};{chunk}\x1b\\");
    }
  }
  out
}

/// Borra imagenes propias, con sus datos (al salir). Nunca se borra todo:
/// otro ketai en la misma terminal (por ejemplo en otro panel de herdr)
/// perderia sus caratulas.
pub fn delete_images(ids: &[u32]) -> String {
  ids.iter().map(|i| format!("\x1b_Ga=d,d=I,i={i},q=2\x1b\\")).collect()
}

/// Lo que hay que mandar para pasar de `old` a `new`. Vacio si no cambio
/// nada (redibujar las imagenes en cada cuadro haria parpadear la pantalla).
/// Vuelve a poner las mismas ubicaciones, sin borrar: con los mismos ids
/// reemplazan en el lugar. Para cuando el multiplexor las perdio sin avisar.
pub fn refresh(placements: &[Placement]) -> String {
  place_all(placements)
}

/// Ubica cada imagen: cursor en la celda (1-based) y ubicacion sin moverlo
/// (C=1), con su propio id de ubicacion.
fn place_all(new: &[Placement]) -> String {
  let mut out = String::new();
  for (n, p) in new.iter().enumerate() {
    let a = p.area;
    if a.width == 0 || a.height == 0 {
      continue;
    }
    let _ = write!(
      out,
      "\x1b[{};{}H\x1b_Ga=p,i={},p={},c={},r={},C=1,z={Z_BELOW_BACKGROUND},q=2\x1b\\",
      a.y + 1,
      a.x + 1,
      p.image,
      n + 1,
      a.width,
      a.height
    );
  }
  out
}

pub fn update(old: &[Placement], new: &[Placement]) -> String {
  if old == new {
    return String::new();
  }
  // Solo se quitan las ubicaciones propias (los datos quedan).
  let mut out = String::new();
  let mut gone: Vec<u32> = Vec::new();
  for p in old {
    if !gone.contains(&p.image) {
      gone.push(p.image);
      let _ = write!(out, "\x1b_Ga=d,d=i,i={},q=2\x1b\\", p.image);
    }
  }
  // Cada ubicacion con su propio id: la misma caratula puede verse dos veces.
  out.push_str(&place_all(new));
  out
}

/// Una imagen a su tamano real en pixeles (sin escalar a las celdas), con
/// su esquina en la celda `(x, y)`. `crop` muestra solo el rectangulo
/// `ancho x alto` de arriba a la izquierda. La usa el modo karaoke: la
/// imagen "cantada" va encima de la "sin cantar" (`z` mayor), recortada
/// hasta donde va la cancion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Overlay {
  pub image: u32,
  pub x: u16,
  pub y: u16,
  pub crop: Option<(u32, u32)>,
  pub z: i32,
}

fn place_overlay(o: &Overlay) -> String {
  let crop = match o.crop {
    Some((w, h)) => format!("x=0,y=0,w={w},h={h},"),
    None => String::new(),
  };
  // Cada imagen tiene una sola ubicacion (p=1): ponerla de nuevo la
  // reemplaza en el lugar, sin parpadeo.
  format!("\x1b[{};{}H\x1b_Ga=p,i={},p=1,{crop}C=1,z={},q=2\x1b\\", o.y + 1, o.x + 1, o.image, o.z)
}

/// Lo que hay que mandar para pasar de `old` a `new`: se quitan las que ya
/// no estan y se ubican solo las que cambiaron. En cada cuadro del karaoke
/// cambia solo el recorte de una imagen: se manda solo esa.
pub fn overlay_update(old: &[Overlay], new: &[Overlay]) -> String {
  let mut out = String::new();
  for o in old {
    if !new.iter().any(|n| n.image == o.image) {
      let _ = write!(out, "\x1b_Ga=d,d=i,i={},q=2\x1b\\", o.image);
    }
  }
  for n in new {
    if !old.contains(n) {
      out.push_str(&place_overlay(n));
    }
  }
  out
}

/// Vuelve a ubicar todas (para cuando el multiplexor las perdio).
pub fn overlay_refresh(all: &[Overlay]) -> String {
  all.iter().map(place_overlay).collect()
}

/// Pregunta de soporte: una imagen de 1x1 con id 31, seguida de DA1 para
/// saber cuando termino de responder la terminal.
pub const QUERY: &str = "\x1b_Gi=31,s=1,v=1,a=q,t=d,f=24;AAAA\x1b\\\x1b[c";

/// La terminal contesto OK a la pregunta.
pub fn supported_reply(reply: &[u8]) -> bool {
  let text = String::from_utf8_lossy(reply);
  text.contains("\x1b_Gi=31;OK")
}
