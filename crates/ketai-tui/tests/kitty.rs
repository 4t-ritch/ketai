//! Protocolo de graficos de kitty y cache de caratulas.

use ketai_tui::{
  covers::{CoverCache, CoverState},
  kitty::{self, Placement},
};
use ratatui::layout::Rect;
use serde_json::json;

fn b64_decode(s: &str) -> Vec<u8> {
  const T: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
  let mut out = Vec::new();
  let mut buf = 0u32;
  let mut bits = 0;
  for c in s.bytes().filter(|c| *c != b'=') {
    buf = (buf << 6) | T.iter().position(|t| *t == c).unwrap() as u32;
    bits += 6;
    if bits >= 8 {
      bits -= 8;
      out.push((buf >> bits) as u8);
    }
  }
  out
}

#[test]
fn transmit_is_chunked_and_carries_the_whole_image() {
  let data: Vec<u8> = (0..10_000u32).map(|i| (i % 251) as u8).collect();
  let seq = kitty::transmit(7, &data);
  let chunks: Vec<&str> = seq.split("\x1b\\").filter(|c| !c.is_empty()).collect();
  assert!(chunks.len() > 1, "large images go in several chunks");
  assert!(chunks[0].starts_with("\x1b_Ga=t,f=100,i=7,q=2,m=1;"), "{}", &chunks[0][..40]);
  assert!(chunks.last().unwrap().starts_with("\x1b_Gm=0;"));
  for c in &chunks[1..chunks.len() - 1] {
    assert!(c.starts_with("\x1b_Gm=1;"));
  }
  let payload: String = chunks.iter().map(|c| c.split_once(';').unwrap().1).collect();
  assert_eq!(b64_decode(&payload), data);
  for c in &chunks {
    assert!(c.split_once(';').unwrap().1.len() <= 4096, "chunks of at most 4096 bytes");
  }
}

#[test]
fn placements_are_redrawn_only_when_they_change() {
  let a = vec![Placement { image: 1, area: Rect::new(4, 2, 20, 9) }];
  let b = vec![Placement { image: 1, area: Rect::new(4, 2, 20, 9) }, Placement { image: 3, area: Rect::new(30, 5, 4, 2) }];
  assert_eq!(kitty::update(&a, &a), "", "nothing changed: nothing is sent");
  let out = kitty::update(&a, &b);
  assert!(out.starts_with("\x1b_Ga=d,d=i,i=1,q=2\x1b\\"), "old placements go away first: {out:?}");
  // Cursor en la celda (fila 3, columna 5) y ubicacion bajo los fondos.
  assert!(out.contains("\x1b[3;5H\x1b_Ga=p,i=1,p=1,c=20,r=9,C=1,z=-1073741825,q=2\x1b\\"), "{out:?}");
  assert!(out.contains("\x1b[6;31H\x1b_Ga=p,i=3,p=2,c=4,r=2,C=1,z=-1073741825,q=2\x1b\\"), "{out:?}");
  assert_eq!(kitty::update(&b, &[]), "\x1b_Ga=d,d=i,i=1,q=2\x1b\\\x1b_Ga=d,d=i,i=3,q=2\x1b\\");
}

#[test]
fn the_same_cover_can_be_shown_twice() {
  // Mejor resultado y fila de Canciones con la misma caratula.
  let two = vec![Placement { image: 5, area: Rect::new(2, 2, 10, 5) }, Placement { image: 5, area: Rect::new(30, 2, 2, 1) }];
  let out = kitty::update(&[], &two);
  assert!(out.contains("i=5,p=1,"), "{out:?}");
  assert!(out.contains("i=5,p=2,"), "each placement keeps its own id: {out:?}");
}

#[test]
fn another_ketai_in_the_same_terminal_keeps_its_images() {
  // Nunca se borra todo: solo las imagenes y ubicaciones propias.
  let a = vec![Placement { image: 7, area: Rect::new(0, 0, 4, 2) }];
  for out in [kitty::update(&a, &[]), kitty::delete_images(&[7, 8])] {
    assert!(!out.contains("d=a,") && !out.contains("d=A,"), "{out:?}");
  }
  assert_eq!(kitty::delete_images(&[7, 8]), "\x1b_Ga=d,d=I,i=7,q=2\x1b\\\x1b_Ga=d,d=I,i=8,q=2\x1b\\");
  // Cada proceso usa su propio rango de ids.
  let (mut x, mut y) = (CoverCache::with_base(1 << 20), CoverCache::with_base(2 << 20));
  for (c, base) in [(&mut x, 1u32 << 20), (&mut y, 2 << 20)] {
    c.want("u");
    c.sent(1, "u");
    let job = c.on_reply(1, Ok(serde_json::json!({"path": "/x.png"}))).unwrap();
    assert!(job.id > base && job.id < base + (1 << 20), "{}", job.id);
    assert_eq!(c.ids(), vec![base + 1, base + 2]);
  }
}

#[test]
fn detects_support_from_the_terminal_reply() {
  assert!(kitty::supported_reply(b"\x1b_Gi=31;OK\x1b\\\x1b[?62;22c"));
  assert!(!kitty::supported_reply(b"\x1b[?62;22c"));
  assert!(!kitty::supported_reply(b"\x1b_Gi=31;ENOTSUPPORTED:x\x1b\\\x1b[?62c"));
}

#[test]
fn the_cache_asks_once_per_uri_and_tracks_replies() {
  let mut cache = CoverCache::default();
  assert_eq!(cache.want("file:///a.ogg"), Some(("track.cover".to_string(), json!({"uri": "file:///a.ogg"}))));
  assert_eq!(cache.want("file:///a.ogg"), None, "already requested");
  cache.sent(10, "file:///a.ogg");
  // La respuesta reserva los ids y devuelve el trabajo para otro thread.
  let job = cache.on_reply(10, Ok(json!({"path": "/run/c/u_1.png", "mime": "image/png"}))).unwrap();
  assert_eq!((job.uri.as_str(), job.path.to_str().unwrap()), ("file:///a.ogg", "/run/c/u_1.png"));
  assert!(matches!(cache.state("file:///a.ogg"), Some(CoverState::Preparing { .. })));
  assert!(cache.on_reply(99, Ok(json!(null))).is_none(), "unknown id");

  cache.want("file:///b.ogg");
  cache.sent(11, "file:///b.ogg");
  assert!(cache.on_reply(11, Ok(json!(null))).is_none());
  assert_eq!(cache.state("file:///b.ogg"), Some(&CoverState::Missing));

  // Lo preparado llega armado: el loop solo lo escribe.
  let img = image::RgbImage::from_pixel(8, 8, image::Rgb([200, 100, 50]));
  let out = cache.decoded("file:///a.ogg", Some(img));
  let Some(CoverState::Ready { normal, dimmed, .. }) = cache.state("file:///a.ogg").cloned() else { panic!() };
  assert_eq!(normal, job.id);
  assert!(out.contains(&format!("i={normal},")));
  assert!(!out.contains(&format!("i={dimmed},")), "the dimmed copy is not sent until a popup needs it");
  assert_eq!(cache.image_for("file:///a.ogg", false), Some(normal));
  assert_eq!(cache.image_for("file:///a.ogg", true), None);

  // La atenuada se pide una sola vez y se usa cuando ya se transmitio.
  let (small, id) = cache.want_dimmed("file:///a.ogg").expect("first time");
  assert_eq!(id, dimmed);
  assert!(cache.want_dimmed("file:///a.ogg").is_none(), "only once");
  let seq = ketai_tui::covers::dimmed_transmit(&small, id);
  assert!(seq.contains(&format!("i={dimmed},")));
  cache.dimmed_ready("file:///a.ogg");
  assert_eq!(cache.image_for("file:///a.ogg", true), Some(dimmed));
  assert_eq!(cache.image_for("file:///b.ogg", false), None);
}

#[test]
fn covers_are_sent_small() {
  let dir = std::env::temp_dir().join(format!("ketai-cover-{}", std::process::id()));
  std::fs::create_dir_all(&dir).unwrap();
  let path = dir.join("big.png");
  image::RgbImage::from_pixel(1280, 720, image::Rgb([10, 20, 30])).save(&path).unwrap();
  let p = ketai_tui::covers::prepare(&path, 1).unwrap();
  assert_eq!((p.image.width(), p.image.height()), (ketai_tui::covers::MAX_SIDE, ketai_tui::covers::MAX_SIDE), "square crop, at most MAX_SIDE");
  let _ = std::fs::remove_dir_all(dir);
}
