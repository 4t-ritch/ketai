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

## Tasks

| ID | Specs | Ruta | Estado | Evidencia |
|----|-------|------|--------|-----------|
| T1 | S1 | inline | hecho (falta ver en kitty real) | 6439a32 |
| T1b | S1 | inline | hecho (falta ver en herdr) | redimensionar y layout 4:3 |
| T2 | S3, S4, S7 | inline | pendiente | Indice SQLite en el daemon: un solo escritor, transacciones, eventos a todos los clientes |
| T3 | S3 | inline | pendiente | Favoritos y pertenencia a varias playlists en la base |
| T4 | S7 | inline | pendiente | Biblioteca: todas, artista, album, carpeta, ano, genero |
| T5 | S2 | inline | pendiente | Volumen, dispositivo, agregar a playlist y favorito en Reproduciendo |
| T6 | S8 | inline | pendiente | `cover_color` en toda la UI |
| T7 | S6 | inline | pendiente | `[keys]` en config.toml, acciones con Ctrl+P |
| T8 | S5 | inline | pendiente | Mouse |

## Log

- L1 (2026-11-22) Pedido original: ver S1-S8 (texto completo del usuario en la conversacion del 2026-11-22).
- L2 T1: el daemon entrega la caratula y `covers::prepare` la arma bien (probado con los archivos reales). Causa: al salir, ketai borraba todas las imagenes de la terminal (`d=A`) y al redibujar borraba todas las ubicaciones (`d=a`); con otro ketai abierto (modo mini en herdr) se borraban entre si, y las caratulas ya listas no se vuelven a mandar. Ademas la misma caratula ubicada dos veces compartia id de ubicacion y solo se veia una (Mejor resultado). Arreglo: solo se borran ids propios, cada proceso tiene su rango de ids, cada ubicacion su id.
- L3 (2026-11-22) Usuario: "las imagenes se eliminan cuando se redimenciona la terminal" y "tambien debes trabajar en redimencionar las cosas o acomplarlas a un sistema mas compacto para que sea posible de visualizar todo en un espacion de 4/3 ya que la UI se ve cortada y no se muestra todo". Hecho en T1b: al redimensionar se vuelven a mandar las caratulas (`CoverCache::resend`); Reproduciendo, barra y Biblioteca se compactan en paneles angostos o bajos (tests a 88x40, 100x34, 76x28).
- Siguiente paso: T2.
