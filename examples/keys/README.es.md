[English](README.md) | **Espanol**

# Atajos de teclado

ketai lee los atajos globales de la seccion `[keys]` de
`~/.config/ketai/config.toml` (o `$XDG_CONFIG_HOME/ketai/config.toml`). Los
cambios se aplican al guardar el archivo, sin cerrar ketai.

En esta carpeta:

| Archivo | Para que sirve |
|---|---|
| `defaults.toml` | Todos los atajos con su valor por defecto, para copiar y editar |
| `vim.toml` | Movimiento estilo vim, con Alt para no chocar con las listas |
| `media.toml` | Reproduccion con Ctrl, Alt y flechas, sin letras sueltas |
| `minimal.toml` | Solo cambia dos atajos; el resto queda por defecto |

Para usar uno, copia su seccion `[keys]` a tu `config.toml`. Si ya tenes
otras secciones (`[theme]`, `[playlists]`), dejalas como estan: cada seccion
es independiente. Los comentarios dentro de los archivos `.toml` estan en
ingles; la explicacion en espanol es esta guia.

## Como se escribe una accion

```toml
[keys]
toggle = "space"              # una tecla
next = ["n", "ctrl+right"]    # varias teclas para la misma accion
```

- El nombre de la izquierda es la accion (ver la tabla de abajo).
- A la derecha va una tecla, o una lista de teclas entre corchetes.
- Lo que configuras **reemplaza** las teclas por defecto de esa accion. Si
  queres sumar una tecla sin perder la de siempre, ponelas las dos en la
  lista.
- Las acciones que no aparecen quedan con su tecla por defecto.

## Como se escribe una tecla

| Tipo | Ejemplos |
|---|---|
| Un caracter | `"n"`, `"P"` (mayuscula = con Shift), `"+"`, `"*"`, `"/"` |
| Una tecla con nombre | `"space"`, `"enter"`, `"esc"`, `"tab"`, `"backspace"` |
| Flechas y navegacion | `"left"`, `"right"`, `"up"`, `"down"`, `"home"`, `"end"`, `"pageup"`, `"pagedown"` |
| Teclas de funcion | `"f1"` a `"f12"` |
| Con modificadores | `"ctrl+p"`, `"alt+x"`, `"shift+tab"`, `"ctrl+right"` |

- Los modificadores van delante y separados con `+`: `ctrl+`, `alt+`,
  `shift+`. Se pueden combinar: `"ctrl+alt+p"`.
- Con Ctrl, mayusculas y minusculas son la misma tecla (`"ctrl+p"` y
  `"ctrl+P"` son iguales): la terminal no las distingue.
- Para la tecla `+` sola se escribe `"+"`; con Ctrl, `"ctrl++"`.

## Acciones

| Accion | Por defecto | Que hace |
|---|---|---|
| `toggle` | `space` | Reproducir o pausar |
| `next` | `n` | Siguiente cancion |
| `previous` | `p` | Cancion anterior |
| `seek_forward` | `right` | Adelantar 5 segundos |
| `seek_back` | `left` | Retroceder 5 segundos |
| `volume_up` | `+`, `=` | Subir el volumen |
| `volume_down` | `-` | Bajar el volumen |
| `mute` | `m` | Silenciar o volver a escuchar |
| `shuffle` | `s` | Aleatorio |
| `repeat` | `r` | Repeticion: ninguna, pista, cola |
| `devices` | `d` | Elegir la salida de audio |
| `folders` | `f` | Carpetas de la biblioteca |
| `favorite` | `*` | Marcar o desmarcar como favorita la cancion que suena |
| `add_to_playlist` | `P` | Agregar la cancion que suena a una playlist |
| `actions` | `ctrl+p` | Popup de acciones y busqueda rapida |
| `quit` | `q` | Salir |
| `next_tab` | `tab` | Pestana siguiente |
| `prev_tab` | `shift+tab` | Pestana anterior |
| `tab_now_playing` | `1` | Ir a Reproduciendo |
| `tab_lyrics` | `2` | Ir a Letras |
| `tab_library` | `3` | Ir a Biblioteca |
| `tab_search` | `4` | Ir a Buscar |
| `lyrics_layer` | `o` | Letras: cambiar la capa principal |
| `lyrics_translation` | `t` | Letras: mostrar u ocultar la traduccion |
| `lyrics_offset_down` | `[` | Letras: offset -50 ms |
| `lyrics_offset_up` | `]` | Letras: offset +50 ms |
| `lyrics_mode` | `k` | Letras: modo clasico o karaoke |
| `lyrics_estimate` | `w` | Letras: estimar tiempos por palabra |

## Reglas que conviene saber

- **Ctrl+C siempre sale**, aunque lo uses para otra cosa o cambies `quit`.
  Asi una configuracion rota nunca te deja sin forma de cerrar ketai.
- **Mientras escribis en Buscar**, las teclas sin Ctrl ni Alt son texto: si
  `toggle = "x"`, escribir "x" en Buscar escribe la letra. Los atajos con
  Ctrl o Alt funcionan igual mientras escribis.
- **Una tecla mal escrita** (`"ctrl+banana"`) es un error: aparece en la
  ultima fila de la pantalla con la accion y la tecla, y ketai sigue con la
  configuracion anterior hasta que lo corrijas.
- **Una accion que no existe** (`dance = "x"`) se avisa y se ignora; el resto
  se aplica.
- **Una tecla repetida bloquea ketai** hasta que la corrijas: la misma tecla
  en dos acciones globales, en una global y una de vista (`lyrics_*`), o dos
  veces en la misma vista. Una ventana muestra todas las teclas repetidas con
  sus acciones; al guardar el archivo corregido, ketai se desbloquea solo. La
  misma tecla en dos vistas distintas si vale.
- **Las demas teclas de cada vista son fijas por ahora**: `h` / `l` para
  cambiar de vista en Biblioteca, `a` para agregar a la cola, `c`
  para crear una playlist, `x` para quitar, `Esc`, `Enter` y las flechas dentro de listas y
  popups. Ver `docs/es/tui.md`.

## Problemas comunes

- **"No pasa nada al apretar la tecla"**: fijate en la ultima fila si hay un
  aviso de tecla repetida; otra accion puede estar usandola.
- **Un atajo con Shift no anda**: para letras se escribe la mayuscula
  (`"P"`), no `"shift+p"`. `shift+` sirve para teclas con nombre
  (`"shift+tab"`).
- **Ctrl con algunas letras es otra tecla**: para la terminal, `ctrl+m` es
  Enter, `ctrl+h` es Backspace, `ctrl+j` es un salto de linea, `ctrl+i` es
  Tab y `ctrl+[` es Esc. No las uses para atajos; preferi Alt
  (`"alt+m"`).
- **Algunas combinaciones no llegan**: la terminal o el multiplexor (herdr,
  tmux) pueden quedarse con ellas antes de que lleguen a ketai. Por ejemplo,
  muchos multiplexores usan `ctrl+b` o `ctrl+a`. Elegi otra combinacion.
