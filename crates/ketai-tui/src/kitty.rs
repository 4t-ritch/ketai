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

/// Borra todas las ubicaciones (los datos de las imagenes quedan).
pub const DELETE_PLACEMENTS: &str = "\x1b_Ga=d,d=a,q=2\x1b\\";
/// Borra ubicaciones y datos (al salir).
pub const DELETE_ALL: &str = "\x1b_Ga=d,d=A,q=2\x1b\\";

/// Lo que hay que mandar para pasar de `old` a `new`. Vacio si no cambio
/// nada (redibujar las imagenes en cada cuadro haria parpadear la pantalla).
pub fn update(old: &[Placement], new: &[Placement]) -> String {
  if old == new {
    return String::new();
  }
  let mut out = String::from(DELETE_PLACEMENTS);
  for p in new {
    let a = p.area;
    if a.width == 0 || a.height == 0 {
      continue;
    }
    // Cursor en la celda (1-based) y ubicacion sin moverlo (C=1).
    let _ = write!(
      out,
      "\x1b[{};{}H\x1b_Ga=p,i={},p=1,c={},r={},C=1,z={Z_BELOW_BACKGROUND},q=2\x1b\\",
      a.y + 1,
      a.x + 1,
      p.image,
      a.width,
      a.height
    );
  }
  out
}

/// Pregunta de soporte: una imagen de 1x1 con id 31, seguida de DA1 para
/// saber cuando termino de responder la terminal.
pub const QUERY: &str = "\x1b_Gi=31,s=1,v=1,a=q,t=d,f=24;AAAA\x1b\\\x1b[c";

/// La terminal contesto OK a la pregunta.
pub fn supported_reply(reply: &[u8]) -> bool {
  let text = String::from_utf8_lossy(reply);
  text.contains("\x1b_Gi=31;OK")
}
