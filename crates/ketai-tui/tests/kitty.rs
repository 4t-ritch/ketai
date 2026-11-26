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
  // La imagen no queda en memoria: la atenuada se arma desde el archivo.
  let (path, id) = cache.want_dimmed("file:///a.ogg").expect("first time");
  assert_eq!((path.to_str().unwrap(), id), ("/run/c/u_1.png", dimmed));
  assert!(cache.want_dimmed("file:///a.ogg").is_none(), "only once");
  let seq = ketai_tui::covers::dimmed_transmit(&image::RgbImage::new(4, 4), id);
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

fn ready_cache(uris: &[&str]) -> CoverCache {
  let mut cache = CoverCache::default();
  for (n, u) in uris.iter().enumerate() {
    cache.want(u);
    cache.sent(n as u64, u);
    cache.on_reply(n as u64, Ok(json!({"path": "/x.png"})));
    cache.decoded(u, Some(image::RgbImage::new(4, 4)));
  }
  cache
}

#[test]
fn after_a_resize_only_what_is_drawn_is_sent_again() {
  // Algunas terminales y multiplexores (herdr) pierden las imagenes al
  // cambiar de tamano. Mandar todas las caratulas cargadas (cientos) llena
  // la memoria de imagenes del multiplexor: se marcan y se manda cada una
  // recien cuando se vuelve a dibujar, con el mismo id.
  let mut cache = ready_cache(&["u", "v"]);
  let id = cache.image_for("u", false).unwrap();
  cache.invalidate_all();
  assert_eq!(cache.image_for("u", false), None, "not placed until it is sent again");
  assert_eq!(cache.state("v"), Some(&CoverState::Stale { normal: cache_ids(&cache, "v").0, dimmed: cache_ids(&cache, "v").1 }));
  // Se vuelve a leer del archivo: la imagen no se guardo en memoria.
  let (path, again) = cache.restage("u").expect("drawn again: prepared once");
  assert_eq!((path.to_str().unwrap(), again), ("/x.png", id));
  assert!(cache.restage("u").is_none(), "only once");
  let seq = cache.ready("u", Some(ketai_tui::covers::prepare_image(image::RgbImage::new(4, 4), again)));
  assert!(seq.contains(&format!("i={id},")), "{seq:?}");
  assert_eq!(cache.image_for("u", false), Some(id));
}

#[test]
fn a_stale_cover_without_a_file_is_asked_for_again() {
  // Preparada sin pasar por el daemon (sin archivo): al perderla la
  // terminal no se puede rearmar, asi que se olvida y se vuelve a pedir.
  let mut cache = CoverCache::default();
  cache.decoded("u", Some(image::RgbImage::new(4, 4)));
  cache.invalidate_all();
  assert!(cache.restage("u").is_none());
  assert_eq!(cache.state("u"), None);
  assert!(cache.want("u").is_some());
}

fn cache_ids(cache: &CoverCache, uri: &str) -> (u32, u32) {
  match cache.state(uri) {
    Some(CoverState::Stale { normal, dimmed } | CoverState::Ready { normal, dimmed, .. } | CoverState::Preparing { normal, dimmed }) => (*normal, *dimmed),
    other => panic!("{other:?}"),
  }
}

#[test]
fn the_terminal_never_keeps_more_than_a_few_covers() {
  // Las que no se usan hace rato se borran de la terminal; si vuelven a
  // aparecer, se mandan otra vez.
  let uris: Vec<String> = (0..10).map(|i| format!("u{i}")).collect();
  let refs: Vec<&str> = uris.iter().map(String::as_str).collect();
  let mut cache = ready_cache(&refs);
  cache.touch(&["u9".to_string(), "u0".to_string()]);
  let seq = cache.evict(4);
  let gone: Vec<&str> = refs.iter().copied().filter(|u| matches!(cache.state(u), Some(CoverState::Stale { .. }))).collect();
  assert_eq!(gone.len(), 6, "{gone:?}");
  assert!(!gone.contains(&"u9") && !gone.contains(&"u0"), "recently used stay: {gone:?}");
  for u in &gone {
    let (normal, dimmed) = cache_ids(&cache, u);
    assert!(seq.contains(&format!("d=I,i={normal},")) && seq.contains(&format!("d=I,i={dimmed},")), "{seq:?}");
  }
  assert!(cache.evict(4).is_empty(), "nothing more to drop");
}

#[test]
fn old_covers_are_forgotten_to_keep_memory_small() {
  // Cada caratula en memoria ocupa ~300 KB. Pasado el tope se olvidan las
  // menos usadas; si reaparecen, se le piden de nuevo al daemon.
  let uris: Vec<String> = (0..10).map(|i| format!("u{i}")).collect();
  let refs: Vec<&str> = uris.iter().map(String::as_str).collect();
  let mut cache = ready_cache(&refs);
  cache.touch(&["u9".to_string()]);
  cache.evict(2);
  cache.forget(5);
  assert_eq!(cache.in_memory(), 5);
  assert!(cache.state("u9").is_some(), "recently used stays");
  let forgotten = refs.iter().filter(|u| cache.state(u).is_none()).count();
  assert_eq!(forgotten, 5);
  // Una olvidada se vuelve a pedir.
  let gone = refs.iter().find(|u| cache.state(u).is_none()).unwrap();
  assert!(cache.want(gone).is_some());
}

#[test]
fn placements_can_be_refreshed_without_deleting_anything() {
  // Si el multiplexor perdio las ubicaciones sin avisar, se vuelven a poner
  // con los mismos ids: reemplazan en el lugar, sin parpadeo.
  let p = vec![Placement { image: 5, area: Rect::new(2, 2, 10, 5) }, Placement { image: 7, area: Rect::new(30, 2, 4, 2) }];
  let out = kitty::refresh(&p);
  assert!(!out.contains("a=d"), "{out:?}");
  assert!(out.contains("a=p,i=5,p=1,") && out.contains("a=p,i=7,p=2,"), "{out:?}");
  assert_eq!(kitty::refresh(&[]), "");
}

#[test]
fn karaoke_overlays_send_only_what_changed() {
  use kitty::Overlay;
  let z = kitty::Z_BELOW_BACKGROUND;
  let unsung = Overlay { image: 40, x: 0, y: 10, crop: None, z: z - 1 };
  let sung = |w| Overlay { image: 41, x: 0, y: 10, crop: Some((w, 60)), z };
  // Primera vez: las dos, a tamano real (sin c= ni r=), la cantada recortada.
  let out = kitty::overlay_update(&[], &[unsung, sung(100)]);
  assert_eq!(
    out,
    format!("\x1b[11;1H\x1b_Ga=p,i=40,p=1,C=1,z={},q=2\x1b\\\x1b[11;1H\x1b_Ga=p,i=41,p=1,x=0,y=0,w=100,h=60,C=1,z={z},q=2\x1b\\", z - 1)
  );
  // Avanza el relleno: solo la cantada, sin borrar nada.
  let out = kitty::overlay_update(&[unsung, sung(100)], &[unsung, sung(130)]);
  assert_eq!(out, format!("\x1b[11;1H\x1b_Ga=p,i=41,p=1,x=0,y=0,w=130,h=60,C=1,z={z},q=2\x1b\\"));
  assert_eq!(kitty::overlay_update(&[unsung, sung(130)], &[unsung, sung(130)]), "");
  // Sale de la vista: se quitan solo sus ubicaciones.
  assert_eq!(kitty::overlay_update(&[unsung, sung(130)], &[]), "\x1b_Ga=d,d=i,i=40,q=2\x1b\\\x1b_Ga=d,d=i,i=41,q=2\x1b\\");
  assert!(!kitty::overlay_refresh(&[unsung, sung(130)]).contains("a=d"));
}
