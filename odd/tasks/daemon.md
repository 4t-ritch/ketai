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
- S7: Dispositivos de salida: `output.list`, `output.set` y el evento
  `output.changed` (topico `output`). Incluye guardar `output` en la sesion
  y restaurarlo, volviendo al default si el dispositivo ya no existe
  (docs/servicio.md, "Restauracion").
- S8: Playlists: metodos `playlist.*`, incluido `playlist.transfer`, y el
  evento `playlist.changed` (topico `playlist`).
- S9: Letras: `lyrics.layers`, `lyrics.get`, `lyrics.set_offset`, reglas de
  la seccion 11 y el evento `lyrics.changed` (topico `lyrics`).
- S10: Integracion con systemd: socket activation (`LISTEN_FDS`) y
  `sd_notify` `READY=1`.
- S11: MPRIS 2 por D-Bus.
- S12: Canal del visualizador segun la seccion 7.
- S13: Biblioteca, cache y caratulas, version local: `library.*`,
  `cache.*`, `track.cover` y el evento `library.progress` (topico
  `library`). Las fuentes remotas, la descarga a cache y
  `download.progress` quedan para una major update futura (L32):
  "me gustaria que T14 quedara a futuro, ahorita me gustaria completar la
  version local para despues en una major update incorporar el sistema de
  descarga para remote".

## Tasks

| ID  | Specs | Ruta   | Estado    | Commit / evidencia |
|-----|-------|--------|-----------|--------------------|
| T1  | S1    | inline | hecho     | 80fd58c; 5 tests del decoder verdes tras mover |
| T2  | S2    | inline | hecho     | 4371266; RED: 13 fallos sin daemon; GREEN: 13/13 (x5 corridas) |
| T3  | S3    | inline | hecho     | 1224433 + fix 608b06a; verificador independiente: B1 FAIL -> PASS en recheck; 7/7 instance, 13/13 control |
| T4  | S4    | inline | hecho     | 5af0708 + fix 9e69fae (verificador: B1 FAIL -> PASS); RED: 19/19 fallan; GREEN: 19/19 conformidad + 15 unit (cola, metadatos); suite 64/64 x3; smoke backend real OK |
| T5  | S5    | inline | hecho     | RED: 11/12 fallan; GREEN: 12/12; suite 81/81 x3 |
| T6  | S6    | inline | hecho     | f96b5f4 + fix seek en pausa (verificador: sin bloqueantes); RED: 11/11 fallan; GREEN: 11/11 + 2 unit; suite 94/94 x3; ~/.local/state real intacto |
| T7  | S7    | inline | hecho     | 7e42a6f + 9263b20 (PID); RED: 6/6 fallan; GREEN: 6/6; suite 101/101 x2; smoke real: bluetooth -> parlantes -> default |
| T8  | S8    | inline | hecho     | c3da656 + fix (verificador: B1 FAIL); RED: 15/15 fallan; GREEN: 15/15 + 7 unit (uri, m3u8); suite 120/120 x3 |
| T9  | S9    | inline | hecho     | f2b8844 parsers, c8e3e31 capas/armado, slice 3 daemon; RED 9/9 -> GREEN 9/9 + 33 unit; suite 164/164 x3 |
| T10 | S10   | inline | hecho | 25dd63e RED 2/3 -> GREEN 3/3 |
| T11 | S11   | inline | hecho | 3777ed9 + fix 9c72637; verificador: (c) FAIL -> PASS en recheck |
| T12 | S12   | inline | hecho | f5f736a + fix af767d2; verificador: (b) fuga -> corregida |
| T13 | S13   | inline | hecho | dd5aa52 + df90635 + 70983fa; verificador PASS |

Ruta inline en todas: no hay unidades paralelas lanzadas juntas ni backstop
de contexto, asi que no aplica el trigger de writer.

## Log

- L1 (2026-11-21, usuario): "arranquemos con el daemon"
- L2: Los tests de conformidad hablan con el daemon solo por el socket, para
  poder correrlos despues contra el core en C.
- L3: T1 quito tokio y stream-download (no se usaban); vuelven a evaluarse al
  implementar origenes remotos (T13), con HTTP bloqueante.
- L4: T2 usa un thread lector y uno escritor por cliente y un loop central
  unico dueno del estado. El socket viejo se borra al arrancar; T3 lo
  reemplaza por la revision de instancia unica.
- L5: T3 limita los huerfanos a la misma sesion (mismo XDG_RUNTIME_DIR en
  /proc/<pid>/environ). Sin esto, los tests en paralelo se mataban entre si,
  y es la definicion correcta de "misma sesion". Documentado en
  docs/servicio.md.
- L6: Verificador independiente sobre T3 encontro B1: una instancia nueva
  podia matar a un dueno sano que estaba arrancando (no respondia hello
  mientras cerraba huerfanos). Fix 608b06a: socket antes del escaneo, marca
  `ready` en el lock, solo se senala a un dueno `ready`. Recheck: PASS.
- L7: Riesgos residuales aceptados (previos al fix):
  - Un dueno `ready` que tarda mas de 2 s en responder hello se considera
    colgado. Invariante para T4 en adelante: el loop central nunca bloquea
    (nada de I/O de audio, disco o red dentro del loop).
  - El escaneo de huerfanos del dueno puede cerrar a una instancia nueva que
    todavia esta en `claim`. Solo afecta a la nueva, que igual iba a salir.
- L8: T4: el motor de audio es un thread propio que abre el dispositivo una
  sola vez; eventos con numero de generacion para descartar los viejos.
  `queue.add` resuelve metadatos en un thread aparte (el loop central no
  bloquea). Backend `null` (KETAI_AUDIO_BACKEND=null) para tests sin sonido.
  Reglas del reproductor documentadas en docs/protocol.md.
- L9: Verificador independiente sobre T4 (5af0708): B1 FAIL, `queue.add`
  podia aplicarse fuera de orden (un thread por add) y las peticiones
  siguientes del mismo cliente no lo veian. Fix: un unico thread de
  metadatos (orden global) y cola por cliente en el loop central mientras
  hay una peticion diferida. RED: respuestas [102,101,100] y play sin ver el
  add; GREEN: 69/69 x3. Tambien se corrigieron dos casos de shuffle
  (una pista con repeat queue; previous tras quitar la actual).
- L10: Riesgo residual aceptado: en rodio 0.22.2, `try_seek` puede quedar
  esperando para siempre si la pista termina justo durante el seek (solo
  bloquea el thread del motor). Revisar al actualizar rodio o envolver el
  seek.
- L11: Recheck T4: PASS. Nota aplicada: cada trabajo del thread de
  metadatos va dentro de `catch_unwind` y un panico del decoder responde
  `1003` en vez de matar el thread. Sin test RED: no hay forma determinista
  de provocar el panico con los fixtures. Nota aceptada: `pending` por
  cliente no tiene limite (socket local).
- L12: T5: el PlayerService compara una foto del estado despues de cada
  mensaje del loop central y genera los eventos; el servidor los reparte por
  topico. La respuesta sale siempre antes que los eventos que provoca.
- L13 (usuario): "espero que estos pendientes los tengas mapeados para
  realizarlos" -> los eventos pendientes de T5 quedan como parte explicita
  de S7 (output.changed), S8 (playlist.changed), S9 (lyrics.changed) y S13
  (library.progress, download.progress).
- L14: T6: guardado en un thread propio (tmp + fsync + rename + fsync del
  directorio); senales con sigwait; restauracion en el thread de metadatos
  con las peticiones de reproductor/cola en espera. Los tests aislan
  XDG_STATE_HOME para no tocar el estado real del usuario.
- L15: Verificador T6: sin bloqueantes. Aplicado: un seek en pausa cuenta
  como cambio y se guarda (RED reproducido con test que separa el guardado
  de la pausa del seek). Movido a S7: guardar/restaurar `output`.
  Aceptados: un error de lectura transitorio tambien mueve la sesion a
  `.bad` (lo dice el doc); un apagado durante la restauracion no responde a
  las peticiones en espera.
- L16: T7 usa libpulse (funciona con PipeWire via pipewire-pulse): lista
  sinks, sigue el default y mueve los streams propios. El stream por el
  plugin ALSA de PipeWire no publica el PID por su cuenta.
- L17 (usuario): "realiza esta prueba no hay problema" / "resuelve el
  problkema que encontraste". Resuelto: el daemon define PIPEWIRE_PROPS al
  arrancar (antes de crear threads) con application.name=ketai, su PID y
  media.role=Music; el stream se reconoce por PID, sin heuristica de nombre.
  Prueba en vivo con pactl: arranque en default, output.set otro, null,
  cambio del default del sistema en ambos sentidos (el stream lo sigue), y
  con salida elegida fija el cambio de default no la mueve. Default del
  usuario restaurado al final.
- L18: T8: playlists en `.m3u8` (formato estandar) en XDG_DATA_HOME; un
  unico thread de I/O en orden; ids de pista compartidos (TrackIds) entre
  la cola y las playlists. La conversion URI <-> ruta se movio a
  ketai-core::uri y la usan la cola y las playlists. Error nuevo 1008
  (playlist ya existe). Los tests aislan XDG_DATA_HOME.
- L19: Verificador T8: B1, `transfer` con `move` dentro de la misma
  playlist duplicaba pistas. Fix: `from_name == to_name` devuelve -32602
  (para reordenar esta `playlist.move`). Notas aplicadas: `copy` solo
  avisa el destino; si falla la segunda escritura de un `move` se avisa el
  destino; `rename` usa hard_link (nunca pisa un archivo que aparezca a
  ultimo momento); el temporal se llama `<archivo>.tmp`. Aceptadas: los
  symlinks se reemplazan por archivos al reescribir; solo `.m3u8` en
  minusculas (documentado).
- L20: Recheck T8: PASS. Aplicado: rename vuelve a fs::rename en
  sistemas sin hard links y avisa el nombre nuevo si falla el borrado.
- L21: Test intermitente `hung_owner_is_replaced` (1 de 6): bug real de T3.
  Tras SIGKILL no se esperaba a que el proceso terminara; el siguiente
  intento no conseguia el bloqueo, el PID ya no validaba y se gastaban los
  3 intentos en milisegundos. Fix: esperar hasta 2 s tras SIGKILL y no
  contar como intento a un dueno `ready` que no se puede validar. 10/10 en
  aislado y suite x3.
- L22: T9 en 3 slices: (1) parsers txt/LRC+A2/TTML/ID3, (2) capas y
  armado, (3) metodos del daemon. symphonia 0.5.5 lee USLT pero descarta el
  descriptor, y no lee SYLT: se escribio un lector minimo de ID3v2 (2.3 y
  2.4, 4 codificaciones, desincronizacion). Vorbis y M4A (c)lyr si los lee
  symphonia. Dependencia nueva: quick-xml para TTML.
- L23: T9 slice 2: descubrimiento (sidecar por sufijo BCP 47, TTML extras,
  ID3 propio, Vorbis/M4A via symphonia), alineacion (key, tiempo +-500 ms,
  orden), word-paired, estimacion por caracteres. Decision: sin capa
  original no hay letra (no hay ancla). Fixture nuevo vorbis_lyrics.ogg.
- L24: T9 slice 3: servicio de letras con su thread de I/O; offset del
  usuario por URI en XDG_STATE_HOME/ketai/lyrics-offsets.json; lyrics.changed;
  track_id fuera de la cola -> -32602; peticiones lyrics.* esperan la
  restauracion de sesion.
- Siguiente paso: T10 (systemd: socket activation y sd_notify).
- L25 (2026-11-22) T10 hecho. Sin libsystemd: `systemd.rs` lee LISTEN_PID/
  LISTEN_FDS (fd 3 ctl, fd 4 viz, viz se cierra hasta T12), borra las
  variables, marca CLOEXEC; notify envia READY=1 tras mark_ready y
  STOPPING=1 al salir. El socket heredado no se borra. Tests: 3 en
  tests/systemd.rs (sh + LISTEN_PID=$$ simula systemd). Prueba real con
  systemd-socket-activate: arranque perezoso y respuesta en la conexion que
  lo disparo. Suite 167/167; una falla intermitente sin identificar en 1 de
  15 corridas (no se reprodujo en 14, ni bajo carga). Tambien se limpiaron
  advertencias de clippy en tests propios.
- L26 (2026-11-22) Falla intermitente identificada: serves_on_the_inherited_ctl_socket.
  Causa: con tests en paralelo el listener podia quedar en el fd 3; dup2(3,3)
  no quita CLOEXEC, el exec cerraba el fd 3 y el daemon tomaba el fd 4 (viz)
  como ctl. Arreglo: el harness copia a fds altos antes de dup2, y el daemon
  identifica cada socket por su posicion (viz nunca pasa a ctl). Test nuevo
  a_missing_first_socket_never_turns_viz_into_ctl (RED -> GREEN). 30/30
  corridas limpias.
- L27 (2026-11-22) Usuario: "si" (implementar que el modo manual ceda a
  systemd). Antes, un ketaid manual con ketaid.socket activo borraba el
  socket de systemd. Ahora, con el bloqueo tomado, si ctl.sock acepta
  conexiones es de systemd: no se toca, esa conexion lo activa y la manual
  sale con 0. Test manual_start_yields_to_a_live_systemd_socket (RED ->
  GREEN); prueba real con systemd-socket-activate ok. Suite 169/169.
- L28 (2026-11-22) Usuario: "de esta lista arregla el 1, 2 y 5 / para el 1
  como dices que es un problema de la libreria realiza una proteccion para
  esos casos para evitar problemas durante el uso del deamon" + "cuando
  termines avanza con el siguiente T, pero solo si todo esta en orden".
  1 (8cd3ff1): el seek reabre la pista y la posiciona antes de entregarla;
  nunca se llama Player::try_seek. Bug encontrado y corregido: tras
  restaurar, la posicion contaba desde 0 (base_ms). 2 (13f04f5): hasta 64
  peticiones sin respuesta por cliente; contrapresion (el daemon deja de
  leer). 5 (0930fd3): lectura fallida se reintenta 5x; luego
  `.unreadable` intacto; `.bad` solo para contenido invalido. Verificador
  independiente: PASS x3, sin bloqueantes. Nota: rodio clear() espera al
  thread de audio; solo colgaria si la salida deja de pedir muestras.
  Suite 10/10 (174 tests), clippy limpio.
- L29 (2026-11-22) T11 hecho (3777ed9). mpris.rs: thread propio con
  libdbus (dbus 0.9 + dbus-crossroads 0.5, sin async); propiedades desde
  una copia del estado; metodos -> peticiones del protocolo por
  Message::Mpris (cliente 0). Seeked tras seek ok; PropertiesChanged por
  diferencia. Prueba real con playerctl en bus privado encontro 2 bugs
  (CanPlay false con cola sin pista actual; lectura inmediata tras
  escribir devolvia el valor viejo), ambos con test RED -> GREEN. Docs:
  docs/mpris.md. Verificador independiente: (a)(b)(d) PASS, (c) FAIL:
  tests de instance/session lanzaban ketaid con el bus real del usuario.
  Fix 9c72637: common::ketaid_command() unico + test guardian. Recheck:
  suite 3/3 (182 tests) vigilando el bus real cada 50 ms: 0 nombres ketai.
  Notas no bloqueantes: Message::Mpris no tiene contrapresion (pedidos
  baratos); Seek mas alla del final se limita en vez de pasar a Next.
- L30 (2026-11-22) Usuario: "corrige los detalles antes de pasar al T12".
  Seek de MPRIS mas alla del final actua como Next (sin efecto si no hay
  siguiente): test seek_past_the_end_acts_like_next RED -> GREEN. Pedidos
  de MPRIS con el mismo Credit de 64 en vuelo (el thread de D-Bus espera
  al loop central); test de regresion con 2000 llamadas sin respuesta
  (no puede ser RED: la diferencia es memoria). Suite 3/3 (184 tests),
  clippy limpio.
- L31 (2026-11-22) Usuario: "dale" (T12). T12 hecho (f5f736a):
  ketai-core/viz.rs (Ring, Tap con try_lock en el thread de audio,
  Analyzer con FFT radix-2 propia, 64 bandas log 30 Hz-16 kHz, dB -60..0),
  ketaid/viz.rs (accept + analizador a 60 fps + escritor por cliente con
  cola de 2 frames). viz.sock propio 0600 o heredado de systemd (fd 4).
  Bug en el camino: bandas graves mas angostas que un bin -> bin del
  centro. Prueba real: fixture 440 Hz -> pico en banda ~424 Hz a -22 dB.
  Verificador: (a)(c)(d) PASS; (b) fuga: un cliente que se va en pausa
  dejaba thread + socket hasta volver a sonar. Fix af767d2: el escritor
  revisa cada 500 ms con recv MSG_PEEK si el cliente cerro; test RED (40
  reconexiones: fds 7->47) -> GREEN. Suite 3/3 (194 tests).
- L32 (2026-11-22) Usuario, sobre partir T13 (local) y T14 (remoto): "si,
  solo que me gustaria que T14 quedara a futuro, ahorita me gustaria
  completar la version local para despues en una major update incorporar el
  sistema de descarga para remote". S13 reescrito sin download.progress; las
  fuentes remotas van a docs/roadmap.md. No se crea T14 en este documento.
- L33 (2026-11-22) T13 implementado: core library.rs (candidatos,
  busqueda por palabras), metadata::cover (embebida o imagen de carpeta) y
  clean_tag (BOM y espacios, visto en archivos reales). ketaid/library.rs:
  worker unico dueno del indice (library.json en XDG_DATA_HOME), escaneo
  en thread aparte, progreso final tras indexar; track.cover a
  XDG_RUNTIME_DIR/ketai/covers 0600; cache.info/clear sobre
  XDG_CACHE_HOME/ketai. Aislamiento de tests reforzado: ketaid_command
  borra XDG_*_HOME y usa HOME inexistente. Tests: 5 de conformidad (RED ->
  GREEN), unitarios de core. Prueba real: samples/ (3 ogg) escaneo,
  busqueda y caratulas ok.
- L34 (2026-11-22) Verificador T13: PASS (a)-(d), 6 notas no bloqueantes,
  todas corregidas en df90635 (cache con enlace en la raiz no se sigue;
  caratula con O_NOFOLLOW; saturating_sub; probe con catch_unwind; replace
  con catch_unwind; covers 0700). Tests RED -> GREEN para los dos de
  seguridad. Usuario: "dale". Bug encontrado al investigar una falla
  intermitente de orphan_of_same_session_is_closed (1 de 3 corridas, sin
  mensaje capturado, 0/15 aislado): un huerfano borraba al salir ctl.sock y
  viz.sock del dueno nuevo. Fix 70983fa (OwnPath: borra solo si dev+ino
  coinciden); test extendido RED -> GREEN. Suite 10/10 (205 tests). La causa
  exacta de aquella falla unica no quedo confirmada; no se repitio.
- Estado: T1-T13 hechos. Daemon v1 local completo.
