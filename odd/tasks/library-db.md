# Biblioteca en SQLite, favoritos, mouse, teclas y vistas

Objetivo: indice SQLite con metadatos y relaciones, favoritos, controles en Reproduciendo, mouse, teclas configurables, vistas de biblioteca y colores de caratula en toda la UI.
Rama: `feat/search` (local). Entrega: commits por tarea; la integracion la decide el usuario.
Tests: `cargo test` (workspace).

## Specs

- S1: Caratulas que no aparecen. Usuario: "el problema es que ahora canciones como esta ya no muestran su portada".
- S2: Barra de Reproduciendo. Usuario: "en esta zona deberia mostrase el volumen y el dispositivo activo, tambien el boton para agregar a una playlist, y el de favoritos".
- S3: Favoritos y varias playlists por cancion. Usuario: "el de favoritos que es algo que quiero incorporar tambien pero esto basado en lo que creo yo seria en una playlist pero guardando toda su metadata y relaciones en la sqlite para poder saber donde mas esta agregada la cancion, mucho ojo porque ketaid debe de admitir que se pueda agregar una cacion a mas de una playlist".
- S4: SQLite resiliente con varios clientes. Usuario: "Avanza con la incorporacion de SQLite, solo que debemos de armar un sistema lo suficientemente completo y resiliente porque al tener sockets activos es posible que dos clientes intenten realizar cambios al mismo tiempo lo cual genera interferencias en la forma en como se relacionan los datos, se actualizan y como se sincronizan entre clientes."
- S5: Mouse. Usuario: "agregar es el soporte interactivo mediante mouse ... una interaccion rapida y de un solo click podria estar bien agregada con la interactividad con el mouse."
- S6: Teclas configurables. Usuario: "Para el tema de comandos creo que estaria genial poder modificarlos a traves del fichero de configuracion que se genera para el programa ... por su puesto que de manera default se deben de tener unos en especifico si no se modifican. Y para esto propongo que el popup de acciones se muestre con Ctrl+P."
- S7: Vistas de biblioteca. Usuario: "la vista de biblioteca deberia de mostrarte todas las canciones que tengas escaneadas en ese momento en tu libreria por lo que deberia de tener un apartado o forma de poder listar todos lo archivos en una sola lista, ademas de tener otra vistas como listado por artista, album, carpeta, year, generos."
- S8: Colores de caratula en toda la UI. Usuario: "se adapta la UI en base al color predominante de la caratula de la cancion, eso esta genial pero deberia aplicarse a toda la UI, ademas de que deberia de ser un parametro configurable ... si se tienen los colores manuales y se coloca la opcion de cover_color deberia de respetar lo de cover color y los manuales se deberian omitir porque lo importante es si esta cover_color como true o false."
- S9: Columnas en las vistas. Usuario: "el punto tres creo que podria facilitar realizando columnas para pintar los datos de la cancion como lo hace spotify".
- S10: Texto largo en movimiento. Usuario: "cuando el texto no se ve por completo en el contenedor que tiene por que es muy largo debe de iniciar un movimiento en scroll horizontal para poder visualizarlo todo."
- S11: Boton de play. Usuario: "corrige el recuadro contenedor del boton de play/pause porque se ve feo como esta actualmente que un lado esta mas largo que el otro".
- S12: Audio. Usuario: "por momentos se traba como si tuviera saturacion en su buffer de audio es algo intermintente ... tarda como unos 500ms o un poco mas en detener la cancion o cuando se hace un seek tarda esos como 500 ms o mas en poder reflejar el cambio en el audio."
- S13: Confirmar repetidas. Usuario: "en la TUI hay que agregar una alerta de que ya se encuentra en la playlist pero hay que ofrecerle la opcion al usuario de agregarla de todos modos o no agregarla. pero esto debe de validarse desde el ketaid para que la confirmacion quede del lado del cliente."

## Tasks

| ID | Specs | Ruta | Estado | Evidencia |
|----|-------|------|--------|-----------|
| T1 | S1 | inline | hecho (falta ver en kitty real) | 6439a32 |
| T1b | S1 | inline | hecho (falta ver en herdr) | redimensionar y layout 4:3 |
| T2 | S3, S4, S7 | inline | hecho | 384d9fc: library.db (WAL, transacciones, un solo thread), importa library.json una vez, genero/ano/numeros/artista del album |
| T3 | S3, S4 | inline | hecho | ver commit; playlists y favoritos en library.db, copias .m3u8, favorites.set, track.playlists |
| T4 | S7, S9 | inline | pendiente | Biblioteca: todas, artista, album, carpeta, ano, genero |
| T5 | S2, S13 | inline | pendiente | Volumen, dispositivo, agregar a playlist y favorito en Reproduciendo |
| T6 | S8 | inline | pendiente | `cover_color` en toda la UI |
| T7 | S6 | inline | pendiente | `[keys]` en config.toml, acciones con Ctrl+P |
| T8 | S5 | inline | pendiente | Mouse |
| T9 | S10 | inline | pendiente | Texto largo con desplazamiento horizontal |
| T10 | S11 | inline | hecho | ed15b40 |
| T11 | S12 | inline | diagnosticando | ver L4 |

## Log

- L1 (2026-11-22) Pedido original: ver S1-S8 (texto completo del usuario en la conversacion del 2026-11-22).
- L2 T1: el daemon entrega la caratula y `covers::prepare` la arma bien (probado con los archivos reales). Causa: al salir, ketai borraba todas las imagenes de la terminal (`d=A`) y al redibujar borraba todas las ubicaciones (`d=a`); con otro ketai abierto (modo mini en herdr) se borraban entre si, y las caratulas ya listas no se vuelven a mandar. Ademas la misma caratula ubicada dos veces compartia id de ubicacion y solo se veia una (Mejor resultado). Arreglo: solo se borran ids propios, cada proceso tiene su rango de ids, cada ubicacion su id.
- L3 (2026-11-22) Usuario: "las imagenes se eliminan cuando se redimenciona la terminal" y "tambien debes trabajar en redimencionar las cosas o acomplarlas a un sistema mas compacto para que sea posible de visualizar todo en un espacion de 4/3 ya que la UI se ve cortada y no se muestra todo". Hecho en T1b: al redimensionar se vuelven a mandar las caratulas (`CoverCache::resend`); Reproduciendo, barra y Biblioteca se compactan en paneles angostos o bajos (tests a 88x40, 100x34, 76x28).
- L4 (2026-11-22) S12, medicion: decodificar no es la causa (debug 442x tiempo real, peor trozo de 50 ms en 1,5 ms; release 933x). La salida de ketaid en PipeWire pide 2048/44100 (46 ms) y el dispositivo activo es Bluetooth (Hisense HS2100), que suma su propia latencia (A2DP suele rondar 150-300 ms). Hipotesis: la demora al pausar o hacer seek es el audio ya entregado a ALSA, PipeWire y Bluetooth; los cortes, faltas de datos (xrun) con un buffer de 46 ms. Falta medir con el usuario: `pw-top` mientras suena (columna ERR) y comparar con salida cableada.
- L5 (2026-11-22) Usuario: "estoy viendo que la cancion de tokai_teio-my_jealousy se reproduce mas rapido en ketai que en su fichero original y no se porque". Causa: `AudioSource::current_span_len` devolvia None; la cola del player de rodio entra una sola vez al mezclador y sin tramos nunca vuelve a leer la frecuencia, asi que queda la de la primera pista. Archivo: vorbis 44,1 kHz; tras una de 48 kHz sonaba 8,8 % mas rapido. RED: test de cola opus 48k + vorbis 44,1k, 3,837 s en vez de 4,000 s. Arreglo b777e12: cada paquete es un tramo.
- L6 (2026-11-22) Decision del usuario (opcion B): la base manda y ketai escribe los archivos de playlist como copia; la primera vez importa las playlists existentes, un archivo nuevo en la carpeta se importa, uno ya conocido editado a mano se reescribe desde la base.
- L7 T2 hecho (384d9fc). Prueba con copia del library.json real en carpeta temporal: 1215 pistas importadas, busquedas de 0,4 a 0,7 ms, listar todo 36 ms en debug.
- L8 T3: playlists en la base (migraciones 2 y 3), copias .m3u8 escritas por ketai, sincronizacion de la carpeta en cada operacion (nuevo se importa, conocido cambiado o borrado se reescribe), Favoritos como playlist con kind, `favorites.set`, `track.playlists`. Cambio de regla deliberado (decision B): al cambiar la carpeta, las playlists siguen existiendo y se copian a la nueva (test de config actualizado). Error encontrado: dos threads abriendo la base a la vez daban "database is locked" al pasar a WAL; se serializa la apertura. RED: 3 tests nuevos fallaban; el de dos clientes agregando a la vez ya pasaba (un solo thread de playlists).
- L9 (2026-11-22) S13 lado daemon: `playlist.add` con `duplicates` ask/add/skip; ask devuelve 1009 con `error.data.duplicates` (campo `data` opcional nuevo en los errores). El test de dos clientes ahora manda `duplicates: "add"` (agrega repetidas a proposito). Falta el dialogo en la TUI (T5).
- L10 (2026-11-22) Usuario: "la idea es que el scan realice el guardado de todos los datos en la db, no importa que" y acepto guardar por tandas. Hecho: tandas de 200 (`Job::Batch`), al final se borra lo que no aparecio. RED: corte con SIGKILL a mitad de 3000 archivos dejaba 0 pistas.
- Siguiente paso: T4 (vistas de biblioteca en columnas); T11 espera la medicion del usuario.
