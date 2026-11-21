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
  "bitrate_kbps": 192
}
```
- `source`: `"local"` | `"remote"`.
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
| `playlist.get`    | `{"name": "chill"}`                     | `{"name": "chill", "tracks": [Track]}` |
| `playlist.save`   | `{"name": "chill", "from_queue": true}` | `{}` |
| `playlist.delete` | `{"name": "chill"}`                     | `{}` |
| `playlist.load`   | `{"name": "chill", "replace": true}`    | `{}` |
| `playlist.rename` | `{"name": "chill", "new_name": "relax"}` | `{}` |
| `playlist.add`    | `{"name": "chill", "uris": ["file:///a.ogg", "file:///b.ogg"], "position": null}` | `{"added": 2}` |
| `playlist.remove` | `{"name": "chill", "indexes": [3, 5]}`  | `{"removed": 2}` |
| `playlist.move`   | `{"name": "chill", "from": 3, "to": 0}` | `{}` |
| `playlist.transfer` | `{"from_name": "rock", "indexes": [2, 5, 7], "to_name": "metal", "position": null, "mode": "move"}` | `{"transferred": 3}` |

- `playlist.move` reordena la playlist: saca la pista en `from` y la inserta
  en `to`. Los indices se cuentan sobre la lista antes del movimiento.
- `playlist.transfer` pasa una o varias pistas de una playlist a otra:
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
  - Emite `playlist.changed` para cada playlist modificada.
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

Ver la seccion 4 para los tipos y la seccion 11 para las reglas.

### Biblioteca / origenes
| Metodo           | Params                            | Resultado |
|------------------|-----------------------------------|-----------|
| `library.scan`   | `{"path": "/home/u/Music"}`       | `{"job": "j_1"}` (progreso via eventos) |
| `library.search` | `{"query": "illit", "limit": 50}` | `{"tracks": [Track]}` |

### Cache
| Metodo        | Params | Resultado |
|---------------|--------|-----------|
| `cache.info`  | -      | `{"path": "...", "size_bytes": 1048576, "entries": 12, "limit_bytes": 2147483648}` |
| `cache.clear` | -      | `{"freed_bytes": 1048576}` |

### Caratulas
| Metodo        | Params                   | Resultado |
|---------------|--------------------------|-----------|
| `track.cover` | `{"track_id": "t_9f2c"}` | `{"path": "/run/user/1000/ketai/covers/t_9f2c.jpg", "mime": "image/jpeg"}` |

Las caratulas se escriben en disco y se devuelve la ruta; nunca se envian
embebidas en base64.

### Dispositivos de salida
| Metodo        | Params                                        | Resultado |
|---------------|-----------------------------------------------|-----------|
| `output.list` | -                                             | `{"devices": [Device]}` |
| `output.set`  | `{"id": "alsa_output..."}` o `{"id": null}`   | `{}` |

`null` vuelve a seguir el dispositivo por defecto del sistema.

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
| `lyrics`   | `lyrics.changed`    | `{"track_id": "t_9f2c"}` (cambio de offset; el cliente vuelve a pedir `lyrics.get`) |
| `cache`    | `download.progress` | `{"track_id": "t_1", "bytes": 1000, "total": 5000}` |

Al hacer `events.subscribe`, si `player` esta entre los topicos, el daemon
envia inmediatamente el `player.state` actual.

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

## 8. Errores

| Codigo  | Significado |
|---------|-------------|
| -32700  | Error de parseo (JSON invalido) |
| -32600  | Peticion invalida (forma incorrecta, linea demasiado larga) |
| -32601  | Metodo no encontrado |
| -32602  | Params invalidos |
| 1000    | Version de protocolo incompatible |
| 1001    | Nada que reproducir / cola vacia |
| 1002    | Indice fuera de rango |
| 1003    | Origen no disponible (archivo inexistente, error de red) |
| 1004    | Dispositivo no encontrado |
| 1005    | Playlist no encontrada |
| 1006    | Letra invalida (no se pudo parsear) |
| 1007    | Capa de letra no encontrada |

## 9. Reglas de compatibilidad

- Agregar metodos, eventos o campos opcionales no incrementa `protocol`.
- Quitar algo o cambiar su significado si lo incrementa.
- Los clientes deben ignorar campos y eventos desconocidos.

## 10. Fuera del alcance de v1

- MPRIS (D-Bus) lo expone el daemon en paralelo y sigue la especificacion
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
