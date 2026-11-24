[English](../tui.md) | **Espanol**

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
- `KETAI_LOG=/tmp/ketai.log ketai` anota con hora (UTC) los eventos de la
  terminal (foco, tamano, mouse y que elemento respondio) y lo que se manda
  de las caratulas. Sirve para diagnosticar problemas con multiplexores como
  herdr. Sin la variable no se anota nada.

Si `ketaid` no esta corriendo, la TUI muestra la pantalla de servicio:
diagnostica el estado y ofrece iniciarlo por systemd (ver
`docs/service.md`). Reintenta conectar cada 2 segundos.

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

Los atajos globales se pueden cambiar en `[keys]` de `config.toml` (ver
`docs/config.md`); estos son los de por defecto.

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
| `Ctrl+P` | acciones y busqueda rapida sobre la pantalla actual |
| `q` | salir (`Ctrl+C` sale siempre) |

Letras (configurables en `[keys]`, ver [config.md](config.md#keys)):

| Tecla | Accion |
|---|---|
| `t` | mostrar u ocultar la traduccion |
| `o` | cambiar la capa principal (original, romanizada) |
| `[` / `]` | offset -50 / +50 ms (se guarda por archivo) |
| `w` | estimar tiempos por palabra cuando la capa no los trae |
| `k` | cambiar entre el modo clasico y el modo karaoke (ver [config.md](config.md#lyrics)) |

Arriba a la derecha de Letras se ve cada opcion con su tecla, como en
Reproduciendo: la capa (`o`), la traduccion (`t`), el offset (`[` / `]`) y el
modo (`k`). Un clic en una opcion hace lo mismo que su tecla.

Biblioteca:

| Tecla | Accion |
|---|---|
| flechas arriba y abajo, o `k` `j` | mover |
| `Enter` | abrir la playlist; sobre una cancion, reproducir la playlist desde ahi |
| `a` | agregar la cancion a la cola |
| `Esc` | volver a la lista de la izquierda |
| `h` / `l` | vista anterior / siguiente: Playlists, Canciones, Artistas, Albumes, Carpetas, Anos, Generos |
| `c` | en Playlists, crear una playlist nueva |
| `x` | en una playlist, quitar la cancion elegida |

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

En todas las vistas se ve el estado del reproductor activo: volumen, salida y
si la cancion es favorita (en Biblioteca y Buscar, en la barra de abajo; en
Letras, junto a los controles; en el modo mini, al lado del volumen).
Reproduciendo muestra ademas en cuantas playlists esta la cancion, y junto al
volumen y a la salida estan sus teclas (`-` `+` y `d` por defecto, o las de
`[keys]`), igual que en Favorito y Agregar a playlist; tambien se pueden
clickear. La salida nunca queda en blanco: "Default" si se sigue al
dispositivo por defecto y todavia no se sabe su nombre, "No device" si no hay
ninguna disponible.

- `*` marca o desmarca la cancion como favorita (queda en la playlist
  Favoritos).
- `P` abre la lista de playlists (las que ya la tienen llevan una marca):
  `Enter` la agrega, `c` crea una playlist nueva (se escribe el nombre) y la
  agrega ahi, y `x` la quita de la playlist elegida (una con la marca).
- Si ya estaba, ketaid avisa y ketai pregunta: `a` la agrega de todos modos,
  `n` no.

## Texto largo

Si un texto no entra en su lugar, se desplaza para leerlo completo: espera
un segundo y medio quieto, avanza un caracter cada cuarto de segundo y,
despues de un separador, vuelve a empezar. Se desplazan el titulo, artista
y album de Reproduciendo, el titulo y artista de la barra de abajo, y la
fila elegida de las listas de Biblioteca y Buscar; el resto se corta, para
que la pantalla no quede llena de cosas en movimiento.

## Mouse

Todo lo que se ve se puede usar con el mouse; hace lo mismo que su tecla.

- Clic en una pestana, en los controles (aleatorio, anterior,
  reproducir o pausar, siguiente, repeticion), en Favorito o en Agregar a
  playlist.
- La barra de progreso y la de volumen se arrastran: se aprieta, se mueve y
  se suelta (un clic tambien sirve). El volumen cambia mientras se arrastra;
  el progreso muestra el punto mientras se arrastra y salta al soltar.
- En la barra de abajo, clic en el volumen silencia, en la salida abre la
  lista de salidas y en el corazon marca favorita. En Reproduciendo, clic en
  la salida tambien abre la lista.
- En Biblioteca: clic en una vista (Playlists, Canciones, Artistas...) o en
  las flechas a los lados de las vistas, en
  una playlist o grupo de la izquierda, o en una cancion. Un clic elige; doble
  clic reproduce (como `Enter`).
- En Buscar: clic en un filtro o en un resultado; doble clic reproduce.
- La rueda mueve la lista que esta bajo el puntero, en cualquier parte de
  la lista.
- Con un popup abierto, solo responde el popup: clic en una opcion la elige
  (en Carpetas solo la marca; las acciones siguen con sus teclas), en la
  confirmacion de repetidas los botones "agregar de todos modos" y "no
  agregar", y un clic afuera lo cierra.

Mientras ketai usa el mouse, la terminal no selecciona texto con un clic
comun; la mayoria permite seleccionar igual con Shift + arrastrar.

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
pueden tener varios abiertos en la misma terminal. Al cambiar el tamano o
recuperar el foco, las caratulas se vuelven a mandar a medida que se
dibujan (algunos multiplexores, como herdr, las pierden), y la terminal
guarda como mucho 48: las que no se usan hace rato se borran y se mandan de
nuevo si reaparecen. Asi no se llena la memoria de imagenes del
multiplexor. ketai guarda en su memoria como mucho 120 caratulas; las demas
se vuelven a pedir al daemon si reaparecen. Cada 2 segundos vuelve a poner
las ubicaciones de las imagenes en pantalla (sin borrar nada), por si el
multiplexor las perdio sin avisar.

Mientras suena una cancion, ketai prepara por adelantado la caratula y los
colores de la siguiente en la cola (solo esa). Asi, con los colores de la
caratula, el tema cambia en el mismo instante en que cambia la pista, sin un
parpadeo del color anterior. Con aleatorio, la siguiente es la que sigue en
la cola que muestra el daemon.
