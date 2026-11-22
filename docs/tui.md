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
| `3` | Biblioteca: playlists y la playlist abierta |
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
| `Esc` | volver a la lista de playlists |

Buscar: al entrar se escribe directo. `Esc` o la flecha abajo pasan a los resultados,
`/` vuelve a escribir. En los resultados: `Enter` reproduce, `a` agrega a la
cola, `h` / `l` cambian el filtro (Todo, Canciones, Playlists, Archivos
locales).

Busqueda rapida: escribir busca canciones y playlists; abajo estan las
acciones (salida de audio, escanear `~/Music`, letras). `Enter` reproduce o
ejecuta, `Tab` agrega a la cola, `Esc` cierra.

## Modo mini

Si la terminal mide menos de 72 columnas o 15 filas, ketai pasa solo a una
vista compacta: caratula, cancion, linea de letra, progreso, controles y un
espectro chico. Pensado para un panel chico de herdr o tmux; las teclas de
reproduccion siguen funcionando.

## Caratulas

Las caratulas las entrega el daemon (`track.cover`, por pista o por URI) y
la TUI las transmite a la terminal con el protocolo de kitty. Se ubican por
debajo del fondo de las celdas: un popup, que tiene fondo propio, las tapa
sin recortes manuales. Detras de un popup se usa una version atenuada.
