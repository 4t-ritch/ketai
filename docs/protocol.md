# Protocolo IPC de ketai - v1 (borrador)

Este documento es el contrato entre `ketaid` (el daemon) y todos los clientes
(TUI, scripts de waybar, GUIs). Cualquier implementacion del core (Rust o C)
es correcta si y solo si cumple este documento y pasa la suite de conformidad.

Los nombres de metodos, eventos y campos JSON se mantienen en ingles porque
son parte del contrato.

## 1. Transporte

| Canal        | Ruta                                 | Formato                          |
|--------------|--------------------------------------|----------------------------------|
| Control      | `$XDG_RUNTIME_DIR/ketai/ctl.sock`    | JSON delimitado por saltos de linea |
| Visualizador | `$XDG_RUNTIME_DIR/ketai/viz.sock`    | Frames binarios fijos (seccion 7) |

- Ambos son `AF_UNIX`, `SOCK_STREAM`, con permisos `0600`.
- Si `XDG_RUNTIME_DIR` no esta definido, el daemon se niega a iniciar.
- Los sockets pueden venir de systemd (socket activation); ver
  `docs/servicio.md`.
- Pueden conectarse muchos clientes a la vez. El daemon es el unico dueno de
  la salida de audio; los clientes nunca abren dispositivos de audio.

## 2. Framing (canal de control)

- Un objeto JSON por linea, UTF-8, terminado en `\n`.
- Los mensajes no pueden contener saltos de linea crudos (JSON los escapa).
- Longitud maxima de linea: 1 MiB. Una linea mas larga cierra la conexion con
  el error `-32600`.

## 3. Forma de los mensajes

**Peticion** (cliente -> daemon):
```json
{"id": 1, "method": "player.play", "params": {}}
```

**Respuesta** (daemon -> cliente, mismo `id`):
```json
{"id": 1, "result": {}}
{"id": 1, "error": {"code": -32601, "message": "method not found"}}
```
Algunos errores traen `error.data` con el detalle (por ejemplo `1009`); los
clientes que no lo usan lo ignoran.

**Evento** (daemon -> cliente, sin `id`, solo despues de `events.subscribe`):
```json
{"event": "track.changed", "data": {"track": { ... }}}
```

- `id` es un entero sin signo elegido por el cliente, unico por conexion.
- Las respuestas pueden intercalarse con eventos; el cliente las empareja por `id`.
- Las peticiones de un mismo cliente se ejecutan en el orden en que llegaron,
  aunque el cliente las envie sin esperar respuesta. Ejemplo: `queue.add`
  seguido de `queue.list` siempre ve las pistas agregadas. Las respuestas a
  un mismo cliente salen en ese mismo orden.
- Un cliente puede tener hasta 64 peticiones sin respuesta (la respuesta
  cuenta cuando ya se escribio en el socket). Al llegar a ese tope el daemon
  deja de leer su socket hasta que el cliente lea respuestas, y sus
  escrituras se bloquean. No se pierde ni se rechaza nada: un cliente que
  nunca lee simplemente queda frenado, sin hacer crecer la memoria del
  daemon.
- `params` puede omitirse cuando esta vacio.

## 4. Tipos de datos

**Track**
```json
{
  "id": "t_9f2c",
  "source": "local",
  "uri": "file:///music/song.ogg",
  "title": "Song",
  "artist": "Artist",
  "album": "Album",
  "duration_ms": 201000,
  "has_cover": true,
  "codec": "vorbis",
  "sample_rate": 44100,
  "channels": 2,
  "album_artist": "Various",
  "genre": "J-Pop",
  "year": 2021,
  "track_number": 3,
  "disc_number": 1,
  "bitrate_kbps": 192
}
```
- `source`: `"local"` | `"remote"`.
- `album_artist`, `genre`, `year`, `track_number` y `disc_number` vienen de
  los tags (por ahora solo en resultados de biblioteca). `year` son los
  primeros cuatro digitos de la fecha; de "3/12" queda `3`.
- Los campos desconocidos valen `null`. Los clientes deben ignorar claves desconocidas.

**PlayerState**
```json
{
  "status": "playing",
  "track": { "...": "Track o null" },
  "position_ms": 53400,
  "volume": 0.8,
  "muted": false,
  "repeat": "off",
  "shuffle": false,
  "output": "alsa_output.usb-headset"
}
```
- `status`: `"stopped"` | `"playing"` | `"paused"` | `"buffering"`.
- `volume`: `0.0`-`1.0`.
- `repeat`: `"off"` | `"track"` | `"queue"`.
- `output`: id del dispositivo, o `null` que significa "seguir el default del sistema".

**LyricsLayer** (una capa de letra disponible)
```json
{
  "id": "ja-Latn-hepburn",
  "kind": "romanized",
  "language": "ja-Latn-hepburn",
  "timing": "word",
  "format": "ttml",
  "origin": "sidecar"
}
```
- `id`: identificador estable de la capa dentro de la pista. La capa
  original siempre tiene `id: "original"`. Las demas usan su etiqueta
  BCP 47 (ver seccion 11).
- `kind`: `"original"` | `"romanized"` | `"translation"`.
- `timing`: nivel de tiempo propio de la capa (ver seccion 11):
  `"none"` | `"line"` | `"word"` | `"word-paired"`.
- `format`: `"text"` | `"lrc"` | `"ttml"`.
- `origin`: `"sidecar"` (archivo local) | `"embedded"` (tags del audio).

**LyricsPackage** (letra armada segun lo que pidio el cliente)
```json
{
  "primary": "ja-Latn-hepburn",
  "secondary": ["es"],
  "timing": "word",
  "offset_ms": 0,
  "user_offset_ms": -250,
  "agents": [{"id": "v1", "name": "Lead"}],
  "lines": [
    {
      "start_ms": 12000,
      "end_ms": 15500,
      "text": "kimi no koe ga",
      "agent": "v1",
      "background": false,
      "words": [
        {"start_ms": 12000, "end_ms": 12600, "text": "kimi"},
        {"start_ms": 12600, "end_ms": 12900, "text": " no"}
      ],
      "secondary": {"es": "tu voz"}
    }
  ]
}
```
- `timing`: nivel de tiempo efectivo de `words` en este paquete. Es el de la
  capa principal, o `"word-estimated"` si el cliente lo pidio y la capa no
  tenia tiempo por palabra.
- `start_ms` / `end_ms` de cada linea vienen siempre de la capa original
  (ancla comun). Si la original no tiene tiempos (`timing: "none"`), son
  `null` y el paquete tiene `timing: "none"`.
- `end_ms` puede ser `null` si el formato no lo da; el cliente usa el
  `start_ms` de la linea siguiente.
- `text`: texto de la capa principal para esa linea.
- `words`: palabras de la capa principal con su propio tiempo, o `null` si
  `timing` es `"none"` o `"line"`.
- `secondary`: objeto `{id_de_capa: texto}` solo por linea, sin palabras.
  Una linea sin texto en una capa secundaria no tiene esa clave.
- `agent` y `background` vienen de TTML (duetos y coros); en otros formatos
  son `null` y `false`.
- Los tiempos ya vienen con los offsets aplicados. Los campos de offset son
  informativos.
- La animacion por letra la calcula el cliente repartiendo el tiempo de cada
  palabra entre sus caracteres.

**Device**
```json
{"id": "alsa_output.usb-headset", "name": "USB Headset", "is_default": true}
```

## 5. Metodos

### Daemon
| Metodo            | Params | Resultado |
|-------------------|--------|-----------|
| `daemon.hello`    | `{"client": "ketai-tui", "protocol": 1}` | `{"server": "ketaid", "protocol": 1, "version": "0.1.0"}` |
| `daemon.shutdown` | -      | `{}` |

Los clientes deben llamar primero a `daemon.hello`. Si la version de protocolo
no coincide, se devuelve el error `1000`.

### Reproductor
| Metodo                 | Params                   | Resultado     |
|------------------------|--------------------------|---------------|
| `player.state`         | -                        | `PlayerState` |
| `player.play`          | -                        | `{}`          |
| `player.pause`         | -                        | `{}`          |
| `player.toggle`        | -                        | `{}`          |
| `player.stop`          | -                        | `{}`          |
| `player.next`          | -                        | `{}`          |
| `player.previous`      | -                        | `{}`          |
| `player.seek`          | `{"position_ms": 60000}` | `{}`          |
| `player.seek_relative` | `{"offset_ms": -5000}`   | `{}`          |
| `player.set_volume`    | `{"volume": 0.5}`        | `{}`          |
| `player.set_muted`     | `{"muted": true}`        | `{}`          |
| `player.set_repeat`    | `{"repeat": "queue"}`    | `{}`          |
| `player.set_shuffle`   | `{"shuffle": true}`      | `{}`          |

Reglas del reproductor:

- `player.play` con la cola vacia, o `player.next` sin pista siguiente,
  devuelven `1001`. Con la cola sin pista seleccionada, `player.play`
  reproduce la primera.
- `player.next` y `player.previous` mantienen el estado: si sonaba sigue
  sonando; si estaba en pausa queda en pausa al inicio de la nueva pista; si
  estaba detenido solo cambia la seleccion.
- `player.previous` reinicia la pista actual si ya sonaron mas de 3 segundos
  o si es la primera de la cola (sin `repeat: "queue"`).
- `player.seek` y `player.seek_relative` necesitan una pista cargada
  (`playing` o `paused`); si no, `1001`. La posicion se limita al rango
  `0`..`duration_ms`.
- `player.stop` deja la pista seleccionada y la posicion en `0`.
- `volume` fuera de `0.0`..`1.0` devuelve `-32602`. `muted` no cambia
  `volume`.
- Fin de pista: con `repeat: "track"` se repite; si no, pasa a la siguiente.
  Sin siguiente, el estado queda `stopped` con la ultima pista seleccionada.
- Si una pista no se puede reproducir, se salta a la siguiente. Si fallan
  todas las de la cola seguidas, el estado queda `stopped`.
- Con `shuffle: true`, la siguiente pista se elige al azar entre las que no
  sonaron en el ciclo actual, y `player.previous` recorre ese historial.
  Cuando sonaron todas: con `repeat: "queue"` empieza un ciclo nuevo; si no,
  no hay siguiente. `shuffle` no cambia el orden de `queue.list`.

### Cola
| Metodo         | Params                                          | Resultado |
|----------------|-------------------------------------------------|-----------|
| `queue.list`   | -                                               | `{"current": 2, "tracks": [Track]}` |
| `queue.add`    | `{"uris": ["file:///a.ogg", "file:///b.ogg"], "position": null}` | `{"added": 2}` |
| `queue.remove` | `{"indexes": [3, 5]}`                           | `{"removed": 2}` |
| `queue.move`   | `{"from": 3, "to": 0}`                          | `{}` |
| `queue.clear`  | -                                               | `{}` |
| `queue.jump`   | `{"index": 4}`                                  | `{}` |

- `queue.add` acepta una o varias `uris`. Se insertan juntas, en el orden
  enviado, a partir de `position`. `position: null` agrega al final.
- `uris` acepta `file://`, `http://` y `https://`.
- `queue.remove` acepta uno o varios `indexes`, sin repetidos, contados sobre
  la cola antes de quitar nada. Para uno solo: `{"indexes": [3]}`.
- Si se quita la pista que esta sonando, la reproduccion pasa a la siguiente
  que quede en la cola; si no queda ninguna, el estado pasa a `stopped`.
- `queue.add` y `queue.remove` son atomicas: ante una URI invalida o un
  indice fuera de rango o repetido, la cola no cambia.
- `queue.add` lee los metadatos de cada archivo local. Una URI con esquema
  no soportado devuelve `-32602`; un archivo inexistente o que no se puede
  decodificar devuelve `1003`. Las rutas `file://` admiten caracteres
  codificados con `%` (por ejemplo `%20` para un espacio).
- `queue.jump` selecciona la pista y la reproduce desde el principio.
- `queue.clear` deja el estado `stopped` y sin pista seleccionada.

### Playlists
| Metodo            | Params                                  | Resultado |
|-------------------|-----------------------------------------|-----------|
| `playlist.list`   | -                                       | `{"playlists": [{"name": "chill", "count": 12}]}` |
| `playlist.get`    | `{"name": "chill"}`                     | `{"name": "chill", "path": "/home/u/music/playlists/chill.m3u8", "tracks": [Track]}` |
| `playlist.save`   | `{"name": "chill", "from_queue": true}` | `{}` |
| `playlist.delete` | `{"name": "chill"}`                     | `{}` |
| `playlist.load`   | `{"name": "chill", "replace": true}`    | `{}` |
| `playlist.rename` | `{"name": "chill", "new_name": "relax"}` | `{}` |
| `playlist.add`    | `{"name": "chill", "uris": ["file:///a.ogg", "file:///b.ogg"], "position": null, "duplicates": "ask"}` | `{"added": 2}` |
| `playlist.remove` | `{"name": "chill", "indexes": [3, 5]}`  | `{"removed": 2}` |
| `playlist.move`   | `{"name": "chill", "from": 3, "to": 0}` | `{}` |
| `playlist.transfer` | `{"from_name": "rock", "indexes": [2, 5, 7], "to_name": "metal", "position": null, "mode": "move"}` | `{"transferred": 3}` |
| `favorites.set`   | `{"uri": "file:///a.ogg", "favorite": true}` | `{}` |
| `track.playlists` | `{"uri": "file:///a.ogg"}`              | `{"playlists": ["chill", "rock"], "favorite": true}` |

- Las playlists viven en la base de ketai (`library.db`, ver biblioteca),
  que manda. Cada cambio es una transaccion y pasa por un solo thread: dos
  clientes que agregan a la misma playlist al mismo tiempo nunca pierden
  pistas. Una cancion puede estar en varias playlists, y varias veces en la
  misma.
- Favoritos es una playlist mas, llamada `Favoritos`, que en
  `playlist.list` trae `"kind": "favorites"` (las demas no traen `kind`). Se
  crea con el primer `favorites.set`. `favorites.set` con `favorite: true`
  la agrega si no estaba; con `false` la quita. `uri` y `favorite` son
  obligatorios (`-32602`). Emite `playlist.changed` con `Favoritos`.
- `playlist.add` con pistas que ya estan en la playlist: `duplicates`
  decide. `"ask"` (o sin el campo) no agrega nada y devuelve el error
  `1009` con `data: {"duplicates": [uris]}`, para que el cliente pregunte
  al usuario; `"add"` las agrega igual; `"skip"` agrega solo las que
  faltan (`added` cuenta las agregadas). Otro valor devuelve `-32602`.
- `track.playlists` dice en que playlists esta una URI (sin Favoritos, por
  orden de nombre) y si es favorita.
- Cada playlist tiene una copia `.m3u8` en la carpeta de playlists,
  compatible con otros reproductores, que escribe ketai. La carpeta es `[playlists] dir` de
  `config.toml` (ver `docs/config.md`) o, si no esta, `XDG_MUSIC_DIR/playlists`
  (`XDG_MUSIC_DIR` del entorno, de `~/.config/user-dirs.dirs` o `~/music`).
  Se resuelve en cada operacion, asi un cambio en `config.toml` se aplica sin
  reiniciar, y se crea si no existe. `playlist.get` devuelve en `path` la
  ruta del archivo. Las pistas locales se guardan como rutas absolutas. Un
  `.m3u8` nuevo copiado a esa carpeta se importa como playlist; al leerlo se
  aceptan rutas absolutas, rutas relativas a la carpeta y URIs. Un archivo
  de una playlist que ya existe, editado o borrado a mano, se vuelve a
  escribir desde la base. Si cambia la carpeta, todas las playlists se
  copian a la nueva. La primera vez, las playlists que ya estaban en la
  carpeta se importan.
- Al modificar una playlist se reescribe el archivo solo con las rutas: se
  pierden comentarios y lineas `#EXTINF` que tuviera un `.m3u8` creado por
  otro programa.
- Solo se reconocen archivos con extension `.m3u8` en minusculas.
- El nombre es tambien el del archivo: no puede estar vacio, empezar con `.`
  ni contener `/`. Un nombre invalido devuelve `-32602`.
- `playlist.save` crea la playlist o reemplaza la existente. Con
  `from_queue: false` queda vacia.
- `playlist.rename` a un nombre que ya existe devuelve `1008`.
- `playlist.get` muestra tambien las pistas cuyo archivo ya no existe, con
  los metadatos en `null`.
- `playlist.load` agrega a la cola solo las pistas que se pueden reproducir.
  Con `replace: true` vacia la cola antes y deja el estado `stopped`; con
  `replace: false` agrega al final sin cambiar la reproduccion.
- `playlist.move` reordena la playlist: saca la pista en `from` y la inserta
  en `to`. Los indices se cuentan sobre la lista antes del movimiento.
- `playlist.transfer` pasa una o varias pistas de una playlist a otra:
  - `from_name` y `to_name` deben ser distintos (`-32602`); para reordenar
    dentro de una playlist se usa `playlist.move`.
  - `indexes` son posiciones en el origen, sin repetidos. Las pistas llegan
    al destino en el orden de su posicion en el origen, no en el orden de
    la lista enviada.
  - `mode: "copy"` las deja en el origen y agrega copias en el destino.
  - `mode: "move"` las quita del origen y las agrega en el destino.
  - Si algun indice esta fuera de rango o repetido, no se transfiere ninguna
    y se devuelve el error `1002` o `-32602`.
  - `position: null` agrega al final del destino.
  - La operacion es atomica: si falla el destino, el origen no cambia.
  - Si `from_name` o `to_name` no existen, devuelve el error `1005`.
  - Emite `playlist.changed` para cada playlist modificada: con `copy` solo
    el destino.
  - Si con `move` falla la escritura del origen despues de escribir el
    destino, las pistas quedan en las dos playlists, la operacion devuelve
    error y se emite `playlist.changed` para el destino.
- `playlist.add` acepta una o varias `uris`. Se insertan juntas, en el orden
  enviado, a partir de `position`. `position: null` agrega al final.
- `playlist.remove` acepta uno o varios `indexes`, sin repetidos. Los indices
  se cuentan sobre la lista antes de quitar nada. Para un solo elemento se
  envia una lista de un item: `{"indexes": [3]}`.
- `playlist.add` y `playlist.remove` son atomicas: si una URI es invalida o
  un indice esta fuera de rango o repetido, la playlist no cambia.
- Indices fuera de rango devuelven el error `1002`.
- Cada cambio emite el evento `playlist.changed`.

### Letras (lyrics)
| Metodo              | Params                                   | Resultado |
|---------------------|------------------------------------------|-----------|
| `lyrics.layers`     | `{"track_id": "t_9f2c"}`                 | `{"layers": [LyricsLayer]}` (vacio si no hay letra) |
| `lyrics.get`        | `{"track_id": "t_9f2c", "primary": "ja-Latn-hepburn", "secondary": ["es"], "estimate_words": false}` | `LyricsPackage` o `null` |
| `lyrics.set_offset` | `{"track_id": "t_9f2c", "offset_ms": -250}` | `{}` |

- En `lyrics.get`, `primary` es opcional (por defecto `"original"`),
  `secondary` es opcional (por defecto `[]`) y `estimate_words` es opcional
  (por defecto `false`).
- Un `id` de capa inexistente devuelve el error `1007`.
- La misma capa no puede estar en `primary` y en `secondary`; eso devuelve
  `-32602`.
- `lyrics.get` devuelve `null` si la pista no tiene letra.
- `track_id` debe ser una pista de la cola; si no, `-32602`. Las pistas
  remotas todavia no tienen letra (`layers` vacio y `get` en `null`).
- Solo se pueden pedir capas que `lyrics.layers` ofrece; una capa que existe
  pero no se pudo alinear tambien devuelve `1007`.
- `lyrics.set_offset` guarda el ajuste por archivo (URI), no por
  `track_id`: se recuerda en sesiones futuras aunque el id cambie. Se guarda
  en `$XDG_STATE_HOME/ketai/lyrics-offsets.json`. `offset_ms: 0` lo borra.
  Emite `lyrics.changed`.

Ver la seccion 4 para los tipos y la seccion 11 para las reglas.

### Biblioteca / origenes
| Metodo           | Params                            | Resultado |
|------------------|-----------------------------------|-----------|
| `library.scan`   | `{"path": "/home/u/Music"}` o `{}` (todas las carpetas) | `{"job": "j_1"}` (progreso via eventos) |
| `library.folders` | -                                | `{"folders": [{"path": "/home/u/Music", "tracks": 1204}]}` |
| `library.add_folder` | `{"path": "/home/u/Music"}`   | `{}` |
| `library.remove_folder` | `{"path": "/home/u/Music"}` | `{}` |
| `library.search` | `{"query": "illit", "limit": 50}` | `{"tracks": [Track]}` |
| `library.groups` | `{"by": "artist"}` | `{"groups": [{"name": "ILLIT", "count": 12}]}` |
| `library.tracks` | `{"by": "artist", "name": "ILLIT", "offset": 0, "limit": 200}` | `{"tracks": [Track], "total": 12}` |

### Cache
| Metodo        | Params | Resultado |
|---------------|--------|-----------|
| `cache.info`  | -      | `{"path": "...", "size_bytes": 1048576, "entries": 12, "limit_bytes": 2147483648}` |
| `cache.clear` | -      | `{"freed_bytes": 1048576}` |

### Caratulas
| Metodo        | Params                   | Resultado |
|---------------|--------------------------|-----------|
| `track.cover` | `{"track_id": "t_9f2c"}` | `{"path": "/run/user/1000/ketai/covers/t_9f2c.jpg", "mime": "image/jpeg"}` |
| `track.cover` | `{"uri": "file:///music/song.flac"}` | igual; el archivo se llama `u_<hash>` |

Las caratulas se escriben en disco y se devuelve la ruta; nunca se envian
embebidas en base64.

Reglas de biblioteca, caratulas y cache (v1, solo archivos locales):

- Las carpetas de la biblioteca se guardan junto al indice. `add_folder`
  recibe una carpeta absoluta existente (`-32602` si no); agregarla dos veces
  no hace nada. `remove_folder` de una carpeta que no esta devuelve
  `-32602` y, si esta, saca del indice sus canciones. Los dos emiten
  `library.changed` (topico `library`).
- `library.scan` con `path` recibe una carpeta absoluta existente; si no,
  `-32602`. Sin `path` escanea todas las carpetas de la biblioteca en un
  solo trabajo (`-32602` si no hay ninguna).
  Responde enseguida con el `job` y escanea en segundo plano, recursivo,
  sin seguir carpetas enlazadas. Los candidatos son los archivos con
  extension de audio (`flac`, `mp3`, `m4a`, `mp4`, `aac`, `ogg`, `oga`,
  `opus`, `wav`); los que no se pueden reproducir no entran al indice.
- `library.progress` llega al empezar (`done: 0`), como maximo cada 200 ms
  y al final con `done == total`. El evento final llega cuando el indice ya
  esta actualizado: una busqueda hecha al recibirlo ya ve las pistas.
- Volver a escanear una carpeta reemplaza todo lo indexado bajo ella (lo
  borrado desaparece al terminar). Lo leido se guarda por tandas de 200
  pistas mientras escanea: si el daemon se corta a mitad, lo guardado
  queda. El indice y las carpetas se guardan en una base
  SQLite, `$XDG_DATA_HOME/ketai/library.db`, y sobreviven a reinicios.
  Cada cambio es una transaccion y los pedidos de todos los clientes pasan
  por un solo thread: dos clientes al mismo tiempo nunca se pisan. Si
  existe un `library.json` de una version anterior, se importa la primera
  vez que se crea la base (el archivo no se borra).
- `library.groups` agrupa la biblioteca por `artist`, `album`, `folder`
  (carpeta de cada archivo), `year` (como texto) o `genre`, en orden. Las
  pistas sin ese dato forman el grupo `name: null`, que va primero. Otro
  `by` devuelve `-32602`.
- `library.tracks` devuelve una pagina de pistas y el `total`. Sin `by`, de
  toda la biblioteca, por ruta. Con `by`, `name` es obligatorio (`null` es
  el grupo sin dato); en un album van por disco y numero de pista.
  `offset` arranca en 0 y `limit` va de 1 a 5000 (200 por defecto).
- `library.search`: `query` es obligatorio. Cada palabra debe aparecer, en
  cualquier parte (no solo al principio), en el titulo, artista, album, artista
  del album, genero, ano o nombre de archivo. No importan las mayusculas ni las marcas: "cancion"
  encuentra el titulo con la o acentuada y "nino" el que tiene enie, y al
  reves (acentos, dieresis, virgulilla, cedilla; ademas U+00DF = ss,
  U+00E6 = ae, U+0153 = oe, U+00F8 = o, U+0142 = l, U+0111 = d). Las marcas de
  otras escrituras se respetan porque cambian la palabra (el dakuten
  japones, U+3099: "ga" no es "ka").
  `""` devuelve todo. `limit` (por defecto 50) debe ser un entero positivo.
  Los resultados van ordenados por ruta y su `id` es `null`: no son pistas
  de la cola (se agregan con `queue.add` usando su `uri`).
- `track.cover` recibe un `track_id` de la cola (`-32602` si no existe) o,
  para pistas que no estan en la cola (playlists, resultados de busqueda),
  una `uri` local. Si vienen los dos, se usa `track_id`. Con `uri`, el
  archivo se llama `u_<hash de la uri>` y es siempre el mismo para la misma
  URI; una URI remota o un archivo inexistente devuelven `null`.
  Usa la imagen embebida (preferentemente la de tapa) y si no hay, una
  imagen de la carpeta: `cover`, `folder`, `front` o `album`, con
  extension `jpg`, `jpeg`, `png` o `webp`, sin distinguir mayusculas. Se
  escribe en `$XDG_RUNTIME_DIR/ketai/covers/` (archivo `0600`). Sin
  caratula devuelve `null`. `has_cover` considera las dos fuentes.
- La cache vive en `$XDG_CACHE_HOME/ketai/`. En v1 no se descarga nada, asi
  que normalmente esta vacia. `cache.clear` borra su contenido, no la
  carpeta. Las fuentes remotas y `download.progress` llegan en una version
  futura (ver `docs/roadmap.md`).

### Dispositivos de salida
| Metodo        | Params                                        | Resultado |
|---------------|-----------------------------------------------|-----------|
| `output.list` | -                                             | `{"devices": [Device]}` |
| `output.set`  | `{"id": "alsa_output..."}` o `{"id": null}`   | `{}` |

`null` vuelve a seguir el dispositivo por defecto del sistema.

Reglas:

- Los dispositivos son los sinks del servidor de sonido (PulseAudio, o
  PipeWire a traves de pipewire-pulse), asi aparecen tambien los de
  bluetooth. `id` es el nombre del sink y `name` su descripcion.
- `output.set` con un `id` que no esta en la lista devuelve `1004`; con un
  valor que no es texto ni `null`, `-32602`.
- Cambiar de salida mueve el stream de ketai al otro sink; el stream no se
  reabre.
- Con `id: null`, ketai sigue el default del sistema: si el default cambia,
  el stream se mueve solo.
- Si el dispositivo elegido se desconecta, el audio pasa al default, pero la
  eleccion se mantiene: al volver a conectarse, el audio vuelve a el.
- `PlayerState.output` y el campo `active` de `output.changed` son la
  eleccion del usuario (`null` = siguiendo el default).
- `output.changed` se emite cuando cambia la lista, el default o la
  eleccion.
- Si no hay servidor de sonido disponible, `output.list` devuelve una lista
  vacia.

### Eventos
| Metodo               | Params                            | Resultado |
|----------------------|-----------------------------------|-----------|
| `events.subscribe`   | `{"topics": ["player", "queue"]}` | `{}` |
| `events.unsubscribe` | `{"topics": ["queue"]}`           | `{}` |

## 6. Eventos

| Topico     | Evento              | Datos |
|------------|---------------------|-------|
| `player`   | `player.state`      | `PlayerState` (en cualquier cambio de estado, volumen o modo) |
| `player`   | `player.position`   | `{"position_ms": 54400}` (maximo 1 Hz mientras reproduce) |
| `player`   | `track.changed`     | `{"track": Track o null}` |
| `queue`    | `queue.changed`     | `{}` (el cliente vuelve a pedir `queue.list`) |
| `playlist` | `playlist.changed`  | `{"name": "chill"}` |
| `output`   | `output.changed`    | `{"devices": [Device], "active": "id o null"}` |
| `library`  | `library.progress`  | `{"job": "j_1", "done": 120, "total": 400}` |
| `library`  | `library.changed`   | `{}` (cambiaron las carpetas; el cliente vuelve a pedir `library.folders`) |
| `lyrics`   | `lyrics.changed`    | `{"track_id": "t_9f2c"}` (cambio de offset; el cliente vuelve a pedir `lyrics.get`) |
| `cache`    | `download.progress` | `{"track_id": "t_1", "bytes": 1000, "total": 5000}` |

Al hacer `events.subscribe`, si `player` esta entre los topicos, el daemon
envia inmediatamente el `player.state` actual.

Reglas:

- `events.subscribe` y `events.unsubscribe` reciben una lista no vacia de
  topicos de esta tabla; un topico desconocido devuelve `-32602` y no cambia
  ninguna suscripcion. Suscribirse dos veces al mismo topico no duplica
  eventos.
- La respuesta a una peticion siempre llega antes que los eventos que esa
  peticion provoca.
- `player.state` solo se emite cuando cambia algo distinto de la posicion.
- `player.position` se emite como maximo una vez por segundo mientras
  reproduce, y ademas inmediatamente despues de un `player.seek` o
  `player.seek_relative`.
- `track.changed` se emite cuando cambia la pista seleccionada, incluido el
  paso a `null` (por ejemplo tras `queue.clear`).
- `queue.changed` se emite cuando cambia el contenido o el orden de la cola,
  no cuando solo cambia la pista seleccionada.
- El topico `cache` ya se acepta, pero `download.progress` llega con las
  fuentes remotas (version futura, ver `docs/roadmap.md`).

## 7. Canal del visualizador

Binario, little-endian. El cliente se conecta y solo lee; nunca escribe.

Header, enviado una sola vez al conectar:
```
offset tamano campo
0      4      magic "KTV1"
4      4      uint32 bins        (ej. 64)
8      4      uint32 fps         (ej. 60)
```

Despues, un flujo de frames, cada uno de `bins` x `float32`, con valores
normalizados `0.0`-`1.0`. Solo se envian frames mientras `status == "playing"`.
Los clientes lentos pierden frames; el daemon nunca se bloquea por un cliente
del visualizador.

Detalles de v1:

- Socket: `$XDG_RUNTIME_DIR/ketai/viz.sock`, permisos `0600`. Con socket
  activation es el segundo socket que pasa systemd.
- `bins` = 64 y `fps` = 60. Los clientes deben leerlos del header y no
  asumirlos.
- Cada frame es el espectro de las ultimas 2048 muestras que sonaron,
  mezcladas a mono, con ventana de Hann.
- Las bandas son logaritmicas entre 30 Hz y 16 kHz (o la mitad de la
  frecuencia de muestreo si es menor): la banda 0 es la mas grave.
- El valor es el pico de la banda en dB: -60 dB o menos es `0.0` y 0 dB
  (un seno a escala completa) es `1.0`. No hay suavizado entre frames: si
  el cliente lo quiere, lo aplica el.
- Cada cliente tiene una cola de 2 frames. Si no lee a tiempo, los frames
  siguientes se descartan para el, nunca a medias: un frame se escribe
  completo o no se escribe.

## 8. Errores

| Codigo  | Significado |
|---------|-------------|
| -32700  | Error de parseo (JSON invalido) |
| -32600  | Peticion invalida (forma incorrecta, linea demasiado larga) |
| -32601  | Metodo no encontrado |
| -32602  | Params invalidos |
| -32603  | Error interno (por ejemplo, la base de datos de la biblioteca fallo) |
| 1000    | Version de protocolo incompatible |
| 1001    | Nada que reproducir / cola vacia |
| 1002    | Indice fuera de rango |
| 1003    | Origen no disponible (archivo inexistente, error de red) |
| 1004    | Dispositivo no encontrado |
| 1005    | Playlist no encontrada |
| 1006    | Letra invalida (no se pudo parsear) |
| 1007    | Capa de letra no encontrada |
| 1008    | Ya existe una playlist con ese nombre |
| 1009    | La pista ya esta en la playlist (`data.duplicates`) |

## 9. Reglas de compatibilidad

- Agregar metodos, eventos o campos opcionales no incrementa `protocol`.
- Quitar algo o cambiar su significado si lo incrementa.
- Los clientes deben ignorar campos y eventos desconocidos.

## 10. Fuera del alcance de v1

- MPRIS (D-Bus) lo expone el daemon en paralelo (ver `docs/mpris.md`) y sigue la especificacion
  MPRIS 2; no forma parte de este protocolo.
- Letras desde proveedores online (por ejemplo LRCLIB).
- Romanizacion automatica y estimacion de tiempos por diccionario:
  planificadas para una major update, ver `docs/roadmap.md`.

## 11. Letras: origenes, formatos y offset

### Formatos

| Formato | Extension | Tiempos | Notas |
|---------|-----------|---------|-------|
| Texto plano | `.txt` | Ninguno | `timing: "none"` |
| LRC | `.lrc` | Por linea (`[mm:ss.xx]`) | `timing: "line"` |
| LRC mejorado (A2) | `.lrc` | Por linea y palabra (`<mm:ss.xx>`) | `timing: "word"`; se detecta dentro del mismo `.lrc` |
| TTML | `.ttml` | Por linea y palabra o silaba | `timing: "word"` o `"line"`; soporta agentes (duetos) y coros de fondo |

### Prioridad de origenes

1. **Archivo local (sidecar)**: mismo nombre que el audio, en la misma carpeta.
   Ejemplo: `roses.flac` -> `roses.ttml`, `roses.lrc`, `roses.txt`.
   Si hay varios, se usa el mas rico: `.ttml`, despues `.lrc`, despues `.txt`.
2. **Letra embebida** en los tags del audio:
   - MP3 (ID3v2): `SYLT` (sincronizada) y despues `USLT` (sin tiempos).
   - FLAC / Ogg / Opus (Vorbis comments): `LYRICS` y despues `UNSYNCEDLYRICS`.
   - M4A: atom `(c)lyr`.
   - Si el texto embebido tiene marcas de tiempo LRC, se parsea como LRC.
3. Si no hay nada, `lyrics.layers` devuelve una lista vacia y `lyrics.get`
   devuelve `null`.
4. **Sin capa original no hay letra**: las romanizaciones y traducciones se
   sincronizan contra la original, asi que sin ella no se ofrece ninguna.

Detalles de lectura:

- Un `.lrc` sin marcas de tiempo se lee como texto plano.
- ID3: solo el tag del principio del archivo, versiones 2.3 y 2.4. Los
  frames comprimidos o encriptados se ignoran. `SYLT` solo con marcas en
  milisegundos; si sus entradas empiezan con salto de linea se leen como
  silabas con tiempo por palabra.
- El idioma de un frame ID3 (ISO 639-2) se pasa a su forma corta BCP 47
  cuando es un idioma comun (`spa` -> `es`); si no, se usa el codigo de 3
  letras.

Esta prioridad aplica por capa: cada capa (original, cada romanizacion y
cada traduccion) se busca por separado con el mismo orden.

Si el origen de mayor prioridad existe pero no se puede parsear, se pasa al
siguiente. Solo se devuelve el error `1006` si ningun origen es valido.

### Offset

- **Offset del archivo**: el tag `[offset:+/-ms]` de LRC. En TTML no aplica
  (vale `0`). Valor positivo = la letra aparece antes.
- **Offset del usuario**: ajuste manual por pista con `lyrics.set_offset`.
  Se guarda en la configuracion del daemon y persiste entre sesiones.
- Tiempo final de cada linea y palabra:
  `tiempo_original - offset_ms - user_offset_ms`, con minimo `0`.

### Capas

Una pista puede tener una capa original, varias romanizaciones y varias
traducciones. El cliente consulta las capas con `lyrics.layers` y pide el
armado con `lyrics.get`, eligiendo una capa principal y cero o mas
secundarias.

#### Identificadores de capa y archivos locales

El `id` de cada capa sale del sufijo del archivo local, usando etiquetas
BCP 47:

| Archivo                         | Capa | `kind` |
|---------------------------------|------|--------|
| `cancion.lrc`                   | `original` | `original` |
| `cancion.es.lrc`                | `es` | `translation` |
| `cancion.ko-Latn.lrc`           | `ko-Latn` | `romanized` |
| `cancion.ja-Latn-hepburn.lrc`   | `ja-Latn-hepburn` | `romanized` |
| `cancion.romanized.lrc`         | `romanized` | `romanized` |

- Una etiqueta con script `Latn` es romanizacion; una etiqueta solo de idioma
  es traduccion.
- Para la misma capa, entre archivos locales gana `.ttml`, despues `.lrc` y
  despues `.txt`.

#### Origenes embebidos y dentro de TTML

- **TTML**: los bloques `<transliterations>` y `<translations>` de
  `iTunesMetadata` generan capas `romanized` y `translation`. Su `id` es el
  atributo de idioma del bloque.
- **Vorbis comments**: `LYRICS_ROMANIZED` genera la capa `romanized` y
  `LYRICS_TRANSLATION_<IDIOMA>` genera la capa con ese idioma, por ejemplo
  `LYRICS_TRANSLATION_ES` -> `es`.
- **ID3v2**: un `USLT`/`SYLT` con descriptor `romanized` genera la capa
  `romanized`; con descriptor `translation` genera una traduccion cuyo idioma
  sale del campo de idioma del frame (ISO 639-2, por ejemplo `spa` -> `es`).

Estos nombres de tag y descriptor son una convencion de ketai, no un
estandar.

#### Sincronizacion entre capas

Las capas no comparten la segmentacion de palabras: "aishiteru" y "te amo"
ocupan la misma ventana de tiempo pero tienen distinta cantidad de palabras,
silabas y letras. Por eso:

1. **Las lineas se alinean siempre a la capa original.** Es el ancla comun y
   lo unico que se comparte entre capas.
   - LRC: cada linea se asocia a la linea original con el mismo tiempo de
     inicio, o a la mas cercana dentro de 500 ms; si no hay ninguna, se
     descarta.
   - Texto plano: se asocia linea por linea, en orden. Si la cantidad de
     lineas no coincide, la capa no se ofrece.
   - TTML: se asocia por el identificador de cada linea (`key`).
2. **El tiempo por palabra solo sale del origen de esa misma capa.** El
   daemon nunca proyecta las palabras de la capa original sobre otra capa.
3. **Excepcion segura:** si el origen empareja explicitamente cada fragmento
   de la capa con un fragmento de la original (por ejemplo `<span>` de TTML
   emparejados por `key`), se reutiliza el tiempo exacto de ese fragmento.
4. **Estimacion opcional:** si el cliente pide `estimate_words: true` y la
   capa principal no tiene tiempo por palabra, el daemon reparte la duracion
   de cada linea entre sus palabras de forma proporcional a la cantidad de
   caracteres, y el paquete se marca `word-estimated`.
5. Una capa que no se pueda alinear se descarta sin afectar a las demas.

#### Niveles de tiempo

| `timing`         | Significado |
|------------------|-------------|
| `none`           | Sin tiempos (texto plano) |
| `line`           | Solo tiempo por linea |
| `word`           | Tiempo por palabra que viene del origen de esa capa |
| `word-paired`    | Tiempo reutilizado de la original por emparejamiento explicito 1 a 1 |
| `word-estimated` | Estimado por el daemon dentro de la linea (solo en `LyricsPackage`, y solo si se pidio) |

#### Ejemplo

Principal romanizada para cantar y traduccion al espanol como subtitulo:

```json
{"id": 9, "method": "lyrics.get",
 "params": {"track_id": "t_1", "primary": "ja-Latn-hepburn", "secondary": ["es"]}}
```

- La principal se anima por palabra si su capa tiene `word` o `word-paired`;
  si no, por linea.
- La secundaria cambia de linea en sincronia con la principal.
