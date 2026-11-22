# TUI de ketai

Objetivo: cliente de terminal `ketai` (crate `ketai-tui`) que habla con `ketaid` por el protocolo IPC y sigue las vistas aprobadas.
Rama: `feat/tui` (local, sale de `feat/daemon`). Entrega: commits por tarea en la rama; la integracion a `dev` la decide el usuario.
Tests: `cargo test -p ketai-tui` (vistas con `ratatui::backend::TestBackend`, cliente IPC contra un `ketaid` real con `KETAI_AUDIO_BACKEND=null`).

## Specs

- S1: Vistas aprobadas, en `samples/tui-maquetas/aprobadas/` (fuera del repo por las caratulas): `1-inicio-reproduciendo`, `2-letras`, `3-busqueda-rapida`, `4-biblioteca-playlist`, `5-buscar`, `6-dispositivos`, `7-mini`, `8-servicio`. Usuario: "perfecto, con esto ahora si podemos arrancar con la TUI, quedan aprovadas las vistas de la carpeta /tmp/claude-1000/-home-ritch-dev-atomiracle-ketai/861b9c75-8c28-4758-b114-c5baa4818823/scratchpad/tui-mockups/out/H-hibrida/".
- S2: Estilo unico. Usuario: "quiero que hagas que todas las pantallas tengan el estilo de colores que tiene las vistas de la opcion C, para que se sientan todas con la misma sinergia, ademas de que quiero que se sientan en la UI lo mas parecidas, preservando lo que me gusta de cada una de ellas, en escencia quiero el estilo minimalista de la opcion C pero con las vistas que me gustaron de la opcion A."
- S3: Imagenes solo con el protocolo de graficos de kitty. Usuario: "y las imagenes quiero que las muestres bien, porque la idea es que se le de soporte solo a terminales que tengan la funcionalidad de mosrtar imagenes con el protocolo kitten, por eso mencionaba que usariamos rust para usar el renderizado de shaders en terminal". Bajo un popup, las imagenes de atras se recortan y se atenuan (como en las maquetas).
- S4: Rust. Usuario: "ahora vamos con la TUI, estaba revisando y lo mejor es aprovechar rust para crearla porque permite usar la gpu directamente en caos de requerir shaders sobre la terminal". Los shaders quedan fuera de esta entrega (asuncion: no se pidio implementarlos ahora).
- S5: herdr. Usuario: "solo contestame una cosa, como se podria integrar en herdr?". Respuesta acordada: la TUI corre en un panel; un panel chico usa el modo mini; varias TUIs pueden estar abiertas a la vez contra el mismo daemon.
- S6: Busqueda y arranque del servicio segun `docs/servicio.md`, seccion "Clientes: buscar y levantar el servicio" (estados `connected`, `starting`, `stopped`, `not_installed`, `unresponsive`, `no_systemd`; acciones por D-Bus de systemd, nunca como proceso hijo). Pedido original del usuario (resumido en el log del daemon): la TUI ofrece buscar si existe el servicio y levantarlo con un boton.

## Tasks

| ID  | Specs      | Ruta   | Estado    | Evidencia |
|-----|------------|--------|-----------|-----------|
| T1  | S2, S4     | inline | hecho     | Base: terminal, tema, pestanas, teclado, cliente IPC con eventos. RED (no compila) -> GREEN 6/6 |
| T2  | S6         | inline | hecho     | Pantalla de servicio, deteccion y arranque por systemd. RED -> GREEN 3/3 |
| T3  | S1, S2     | inline | hecho     | Reproduciendo (inicio), controles, visualizador (viz.sock). RED -> GREEN 7/7 |
| T4  | S1         | inline | hecho     | Letras (capas, sincronia, palabra, offset). RED -> GREEN 7/7 + prueba real contra ketaid |
| T5  | S1         | inline | pendiente | Biblioteca/playlist |
| T6  | S1         | inline | pendiente | Buscar y busqueda rapida (paleta) |
| T7  | S1         | inline | pendiente | Dispositivos (popup) |
| T8  | S1, S5     | inline | pendiente | Modo mini por tamano del panel |
| T9  | S3         | inline | pendiente | Caratulas con protocolo kitty, recorte y atenuado bajo popups, deteccion de soporte |

## Log

- L1 (2026-11-22) Pedido original: "ahora vamos con la TUI, estaba revisando y lo mejor es aprovechar rust para crearla porque permite usar la gpu directamente en caos de requerir shaders sobre la terminal. por lo que antes de poder empezar quiero que tomes de referencia la UI que usa spotify, para que podamos tomarla de ejemplo y en base a eso trabajar, pero antes de poder plasmar la UI quiero que me generes propuestas que pueda yo ver, no vale crear propuestas completamente inventadas con artifacts y demas cosas raras, debe ser una UI que sea creable en rust. dame varias propuestas, cada propuesta debe ir con su set de imagenes para cada vista y apartado que vayas planteando."
- L2 Seleccion: "me gusta como se ven las pantallas de lyrics, la busqueda mientras estas en reproduccion, la integracion con tmux(aunque yo uso herdr) y el home de la opcion C / la vista de playlist, la vista de dispositivos y la de busqueda general me gusto de la opcion A / la opcion B descartala."
- L3 Estilo e imagenes: ver S2 y S3. Aprobacion: ver S1.
- L4 Decisiones: el `view.rs` anterior del usuario (audio directo con rodio) se reemplaza; queda en la historia de git. Las maquetas y su generador se guardan en `samples/tui-maquetas/` (ignorado por git). Hueco detectado: `docs/servicio.md` menciona `ketaid --daemonize` para el modo manual, que el daemon no implementa; se resuelve en T2.
- L5 (2026-11-22) T1 hecho: lib `ketai_tui` (client, app, theme, ui) + binario `ketai`. Threads y canales: lector del daemon y lector de teclado alimentan un canal; `App` no hace I/O y devuelve las peticiones. Tests: cliente contra servidor falso (id, eventos, errores, cierre) y app/pestanas con TestBackend. Suite 211/211.
- L6 (2026-11-22) T2 hecho: `service.rs` (diagnose puro; observe/execute con systemd de usuario por D-Bus: LoadUnit, StartUnit, EnableUnitFiles, Reload, RestartUnit; proceso por /proc; binario en ~/.local/bin o PATH). Instalar copia las unidades de dist/systemd (incluidas en el binario). Modo manual: `setsid -f ketaid` (docs/servicio.md actualizado; reemplaza el `--daemonize` que no existia). Reiniciar pide confirmacion (Enter, despues y). Loop: reintento de conexion cada 2 s; si el daemon se cae se vuelve a la pantalla de servicio. Prueba real de solo lectura contra el systemd del usuario: NotInstalled (correcto: sin unidades ni binario). StartUnit/Install no se probaron en real: tocarian el systemd del usuario.
- L7 (2026-11-22) T3 hecho: vista Reproduciendo (titulo, artista, album, progreso interpolado entre eventos, controles, siguiente de la cola, volumen, codec), espectro de viz.sock solo mientras suena, teclas de control (flechas 5 s, +/- volumen, m, s, r). `on_daemon` devuelve peticiones (queue.changed -> queue.list) y las respuestas se reconocen por metodo (`App::sent`). Comparada contra la maqueta aprobada con el mismo exportador: coincide; faltan caratula (T9), vista previa de letra (T4) y dispositivo arriba (T7).
- L8 (2026-11-22) T4 hecho: al cambiar de pista se piden `lyrics.layers` y `lyrics.get`; la primera traduccion se muestra debajo salvo que el usuario elija; `lyrics.changed` de la pista actual vuelve a pedir el paquete. Vista Letras: linea actual con palabra resaltada (acento + subrayado), cantadas en blanco, pasadas tenues, traducciones en cursiva; teclas t, o, [ ], w solo en esa pestana. Vista previa de 3 lineas en Reproduciendo. Prueba real contra ketaid (null backend, .lrc A2 + .es.lrc): capas y traduccion llegan, linea actual correcta. Defecto encontrado y corregido: sin etiquetas no se mostraba titulo en Letras (ahora `track_title` usa el nombre del archivo en ambas vistas).
- Siguiente paso: T5 (biblioteca/playlist).
