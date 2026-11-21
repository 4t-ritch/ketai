# Feature: daemon ketaid

Objetivo: implementar `ketaid` segun `docs/protocol.md` y `docs/servicio.md`.
Rama: `feat/daemon` (local, sale de `dev`). Estrategia de entrega: ask-on-risk.
Runner de tests: `cargo test --workspace`.

## Specs

- S1: Workspace Rust con crates separados: `ketai-protocol` (tipos y framing),
  `ketai-core` (decodificacion, reproductor, cola), `ketaid` (binario del
  daemon). Estilo "traducible" a C: threads + canales, sin async/tokio.
- S2: Canal de control segun `docs/protocol.md` secciones 1-3 y 8: Unix socket
  en `$XDG_RUNTIME_DIR/ketai/ctl.sock` (0600), JSON por lineas, max 1 MiB por
  linea, peticion/respuesta/error, metodos `daemon.hello` y `daemon.shutdown`.
- S3: Instancia unica segun `docs/servicio.md` seccion "Instancia unica":
  `flock` en `ketaid.lock`, `daemon.hello` al dueno con timeout de 2 s,
  cierre del dueno colgado, escaneo y validacion de huerfanos.
- S4: Reproductor y cola: metodos `player.*` y `queue.*` del protocolo.
- S5: Eventos: `events.subscribe` / `events.unsubscribe` y eventos de la
  seccion 6.
- S6: Persistencia de sesion segun `docs/servicio.md`.
- S7: Dispositivos de salida: `output.list`, `output.set`.
- S8: Playlists: metodos `playlist.*`, incluido `playlist.transfer`.
- S9: Letras: `lyrics.layers`, `lyrics.get`, `lyrics.set_offset` y reglas de
  la seccion 11.
- S10: Integracion con systemd: socket activation (`LISTEN_FDS`) y
  `sd_notify` `READY=1`.
- S11: MPRIS 2 por D-Bus.
- S12: Canal del visualizador segun la seccion 7.
- S13: Biblioteca, cache y caratulas: `library.*`, `cache.*`, `track.cover`.

## Tasks

| ID  | Specs | Ruta   | Estado    | Commit / evidencia |
|-----|-------|--------|-----------|--------------------|
| T1  | S1    | inline | pendiente | |
| T2  | S2    | inline | pendiente | |
| T3  | S3    | inline | pendiente | |
| T4  | S4    | inline | pendiente | |
| T5  | S5    | inline | pendiente | |
| T6  | S6    | inline | pendiente | |
| T7  | S7    | inline | pendiente | |
| T8  | S8    | inline | pendiente | |
| T9  | S9    | inline | pendiente | |
| T10 | S10   | inline | pendiente | |
| T11 | S11   | inline | pendiente | |
| T12 | S12   | inline | pendiente | |
| T13 | S13   | inline | pendiente | |

Ruta inline en todas: no hay unidades paralelas lanzadas juntas ni backstop
de contexto, asi que no aplica el trigger de writer.

## Log

- L1 (2026-11-21, usuario): "arranquemos con el daemon"
- L2: Los tests de conformidad hablan con el daemon solo por el socket, para
  poder correrlos despues contra el core en C.
