[English](../config.md) | **Espanol**

# Configuracion

La TUI lee `$XDG_CONFIG_HOME/ketai/config.toml` (por defecto
`~/.config/ketai/config.toml`). Si no existe, usa los valores por defecto.
Hay un ejemplo comentado en `dist/config.toml`, un ejemplo completo para
usuarios nuevos en `examples/config.toml`, y guias con ejemplos por tema en
`examples/` (`examples/keys/` para atajos, `examples/theme/` para colores).

## Formato

TOML: declarativo, admite comentarios y secciones. Cada funcion
personalizable tiene su seccion (`[theme]`, `[keys]`, `[playlists]`); las
secciones nuevas se agregan sin romper los archivos existentes. La TUI lee
`[theme]` y `[keys]`, y el daemon lee `[playlists]`.

- Una clave o seccion desconocida se avisa en la ultima fila de la pantalla
  y se ignora; el resto se aplica.
- Un error (sintaxis, color invalido) se muestra en la ultima fila con la
  clave o la linea, y se mantiene la configuracion anterior hasta que el
  archivo se corrija.

## Recarga en vivo

Los cambios se aplican al guardar el archivo, sin cerrar ketai. La TUI
revisa el archivo cada medio segundo; funciona aunque el editor guarde
reemplazando el archivo. Si el archivo se borra, vuelven los valores por
defecto.

## `[theme]`

| Clave | Valores | Por defecto |
|---|---|---|
| `cover_color` | `true` o `false` | sin definir |
| `background` | `"cover"` o `"#rrggbb"` | `"cover"` |
| `accent` | `"cover"` o `"#rrggbb"` | `"cover"` |
| `foreground` | `"#rrggbb"` | `"#ffffff"` |
| `dim` | `"#rrggbb"` | `"#c8d3e0"` |
| `faint` | `"#rrggbb"` | `"#7f8ea3"` |
| `raised` | `"#rrggbb"` | `"#1c2a3a"` (con `background = "cover"`, sale de la caratula) |
| `on_accent` | `"#rrggbb"` | `"#04121f"` |
| `row` | `"#rrggbb"` | `"#16202e"` |
| `ok` | `"#rrggbb"` | `"#7ee787"` |
| `error` | `"#rrggbb"` | `"#ff7b8a"` |
| `spectrum_top` | `"#rrggbb"` | `"#ff7ab6"` |

`cover_color` decide de donde salen los colores:

- `true`: toda la interfaz sale de la caratula que suena (fondo, fondo
  elevado, fila elegida, acento, texto sobre el acento, textos secundarios
  y espectro, todos del tono de la caratula). Los colores manuales se
  ignoran aunque esten en el archivo. Sin caratula, los colores por
  defecto.
- `false`: solo valen los colores manuales; `"cover"` vale el color por
  defecto.
- Sin la clave: cada color decide; `background` y `accent` pueden ser
  `"cover"`. Con `background = "cover"` tambien salen de la caratula
  `raised`, `row`, `dim` y `faint`; con `accent = "cover"`, `on_accent` y
  `spectrum_top`. Un color fijado a mano siempre gana.

Los popups, las lineas de letra que todavia no se cantan y el resto de la
interfaz derivan de estos colores: nada tiene un color propio fijo. `ok` y
`error` son colores de estado (conectado, error) y no siguen a la caratula.

Con `"cover"`, los colores salen de la caratula que suena: el fondo es el
color promedio oscurecido y el acento es el mismo tono, claro y saturado
para que se lea. Sin caratula se usan los colores por defecto.

## `[keys]`

Atajos globales. Cada accion recibe una tecla (`toggle = "x"`) o una lista
(`next = ["n", "ctrl+right"]`); lo que se configura reemplaza las teclas por
defecto de esa accion y el resto queda igual.

Formato de una tecla: un caracter (`"n"`, `"P"`, `"+"`), o un nombre
(`space`, `enter`, `esc`, `tab`, `backspace`, `left`, `right`, `up`, `down`,
`home`, `end`, `pageup`, `pagedown`, `f1` a `f12`), con modificadores
`ctrl+`, `alt+` o `shift+` delante (`"ctrl+p"`, `"shift+tab"`).

| Accion | Por defecto | Que hace |
|---|---|---|
| `toggle` | `space` | Reproducir o pausar |
| `next` | `n` | Siguiente |
| `previous` | `p` | Anterior |
| `seek_forward` | `right` | Adelantar 5 s |
| `seek_back` | `left` | Retroceder 5 s |
| `volume_up` | `+`, `=` | Subir volumen |
| `volume_down` | `-` | Bajar volumen |
| `mute` | `m` | Silenciar |
| `shuffle` | `s` | Aleatorio |
| `repeat` | `r` | Repeticion |
| `devices` | `d` | Salida de audio |
| `folders` | `f` | Carpetas de la biblioteca |
| `favorite` | `*` | Marcar como favorita |
| `add_to_playlist` | `P` | Agregar a una playlist |
| `actions` | `ctrl+p`, `ctrl+k`, `/` | Acciones y busqueda rapida |
| `quit` | `q` | Salir |
| `next_tab` / `prev_tab` | `tab` / `shift+tab` | Pestana siguiente / anterior |
| `tab_now_playing`, `tab_lyrics`, `tab_library`, `tab_search` | `1` a `4` | Ir a cada pestana |

- `Ctrl+C` siempre sale, aunque se configure otra cosa.
- Mientras se escribe en Buscar, las teclas sin Ctrl ni Alt son texto.
- Una tecla invalida es un error (se muestra y se mantiene la configuracion
  anterior); una accion desconocida o una tecla en dos acciones se avisa.
- Las teclas propias de cada vista (`h` / `l` en Biblioteca, `t` / `o` en
  Letras, etc.) son fijas por ahora.

## `[playlists]`

| Clave | Valores | Por defecto |
|---|---|---|
| `dir` | ruta absoluta o que empiece con `~` | `XDG_MUSIC_DIR/playlists` |

`XDG_MUSIC_DIR` sale del entorno, de `~/.config/user-dirs.dirs` o, si no
esta en ninguno, es `~/music`. La carpeta se crea si no existe. El daemon la
lee en cada operacion de playlists: cambiarla se aplica sin reiniciar. Si el
archivo tiene un error, el daemon usa la carpeta por defecto y lo registra en
su log. Las playlists se guardan en la base de ketai; al cambiar la carpeta,
todas se copian a la nueva.
