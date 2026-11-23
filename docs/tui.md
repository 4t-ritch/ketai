# TUI de ketai

`ketai` es el cliente de terminal. No reproduce audio por su cuenta: habla
con `ketaid` por el protocolo IPC (`docs/protocol.md`). Se pueden abrir
varias TUIs a la vez contra el mismo daemon (por ejemplo una en modo mini en
un panel de herdr y otra completa).

## Requisitos

- Una terminal con el protocolo de graficos de kitty: kitty, Ghostty,
  WezTerm, Konsole, o herdr corriendo dentro de una de ellas. Al arrancar,
  ketai le pregunta a la terminal; si no lo soporta, explica que falta y
  sale.
- Una Nerd Font para los iconos.
- `KETAI_NO_IMAGES=1 ketai` arranca igual, sin caratulas (util por SSH).

Si `ketaid` no esta corriendo, la TUI muestra la pantalla de servicio:
diagnostica el estado y ofrece iniciarlo por systemd (ver
`docs/servicio.md`). Reintenta conectar cada 2 segundos.

## Pantallas

Pestanas arriba, en todas las pantallas:

| Tecla | Pestana |
|---|---|
| `1` | Reproduciendo: caratula, progreso, controles, letra, siguiente y espectro |
| `2` | Letras: linea actual con la palabra que suena, traducciones debajo |
| `3` | Biblioteca: playlists, todas las canciones, o por artista, album, carpeta, ano o genero |
| `4` | Buscar: busqueda en la biblioteca local |

`Tab` y `Shift+Tab` pasan a la pestana siguiente o anterior.

## Teclas

En cualquier pestana (salvo mientras se escribe en Buscar):

| Tecla | Accion |
|---|---|
| `espacio` | reproducir o pausar |
| `n` / `p` | siguiente / anterior |
| flecha izquierda / derecha | retroceder / adelantar 5 segundos |
| `+` / `-` | volumen |
| `m` | silenciar |
| `s` | aleatorio |
| `r` | repeticion: ninguna, pista, cola |
| `d` | elegir la salida de audio |
| `*` | marcar o desmarcar como favorita la cancion que suena |
| `P` | agregar la cancion que suena a una playlist |
| `f` | carpetas de la biblioteca |
| `/` o `Ctrl+K` | busqueda rapida sobre la pantalla actual |
| `q` | salir (`Ctrl+C` sale siempre) |

Letras:

| Tecla | Accion |
|---|---|
| `t` | mostrar u ocultar la traduccion |
| `o` | cambiar la capa principal (original, romanizada) |
| `[` / `]` | offset -50 / +50 ms (se guarda por archivo) |
| `w` | estimar tiempos por palabra cuando la capa no los trae |

Biblioteca:

| Tecla | Accion |
|---|---|
| flechas arriba y abajo, o `k` `j` | mover |
| `Enter` | abrir la playlist; sobre una cancion, reproducir la playlist desde ahi |
| `a` | agregar la cancion a la cola |
| `Esc` | volver a la lista de la izquierda |
| `h` / `l` | vista anterior / siguiente: Playlists, Canciones, Artistas, Albumes, Carpetas, Anos, Generos |
| `c` | en Playlists, crear una playlist nueva |

En Canciones y en los grupos, la lista va en columnas (numero, titulo y
artista, album, ano y duracion). `Enter` sobre una cancion pone en la cola
toda la lista que se ve y empieza desde esa. Las canciones sin el dato del
grupo (por ejemplo sin genero) quedan en "Sin dato".

Buscar: al entrar se escribe directo. `Esc` o la flecha abajo pasan a los resultados,
`/` vuelve a escribir. En los resultados: `Enter` reproduce, `a` agrega a la
cola, `h` / `l` cambian el filtro (Todo, Canciones, Playlists, Archivos
locales).

Carpetas de la biblioteca (`f`): la lista de carpetas que ketai indexa, con
cuantas canciones tiene cada una. `a` agrega una (se escribe la ruta; `~` es
tu carpeta personal), `x` la quita (con confirmacion; sus canciones salen
del indice), `s` escanea la carpeta elegida y `S` todas. Mientras escanea se
ve el progreso. Las carpetas las guarda el daemon: cualquier cliente ve las
mismas.

Busqueda rapida: escribir busca canciones y playlists; abajo estan las
acciones (salida de audio, escanear toda la biblioteca, letras, carpetas). `Enter` reproduce o
ejecuta, `Tab` agrega a la cola, `Esc` cierra.

## Favoritos y agregar a una playlist

En todas las vistas se ve el estado del reproductor activo: volumen,
salida y si la cancion es favorita (en Biblioteca y Buscar, en la barra de
abajo; en Letras, junto a los controles; en el modo mini, al lado del
volumen). En Reproduciendo, ademas, en cuantas playlists esta. La salida
nunca queda en blanco: "Default" si se sigue al dispositivo por defecto y
todavia no se sabe su nombre, "No device" si no hay ninguna disponible. La
cabecera ya no repite la salida. `*` la marca o desmarca como
favorita (queda en la playlist Favoritos). `P` abre la lista de playlists
(las que ya la tienen llevan una marca); `Enter` la agrega y `c` crea una
playlist nueva (se escribe el nombre) y la agrega ahi. Si ya estaba,
ketaid avisa y ketai pregunta: `a` la agrega de todos modos, `n` no.

## Modo mini

Si la terminal mide menos de 72 columnas o 15 filas, ketai pasa solo a una
vista compacta: caratula, cancion, linea de letra, progreso, controles y un
espectro chico. Pensado para un panel chico de herdr o tmux; las teclas de
reproduccion siguen funcionando.

En paneles medianos (por ejemplo 4:3, unas 88x40) la vista completa se
compacta: caratula mas chica, controles mas juntos, en Biblioteca se oculta
la columna de lo que suena (queda la barra de abajo) y, con poco alto, se
omiten la letra y lo que sigue.

## Caratulas

Las caratulas las entrega el daemon (`track.cover`, por pista o por URI) y
la TUI las transmite a la terminal con el protocolo de kitty. Se ubican por
debajo del fondo de las celdas: un popup, que tiene fondo propio, las tapa
sin recortes manuales. Detras de un popup se usa una version atenuada.

Cada ketai usa su propio rango de ids de imagen y solo borra las suyas: se
pueden tener varios abiertos en la misma terminal. Al cambiar el tamano las
caratulas se vuelven a mandar, porque algunos multiplexores (herdr) las
pierden.
