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

Atajos. Cada accion recibe una tecla (`toggle = "x"`) o una lista
(`next = ["n", "ctrl+right"]`); lo que se configura reemplaza las teclas por
defecto de esa accion y el resto queda igual.

### Que teclas se permiten

Cada atajo es **una tecla**, con modificadores opcionales delante,
separados con `+`. Una accion puede tener varios atajos con una lista
(`next = ["n", "ctrl+right"]`).

**Permitidas:**

| Tipo | Como se escribe | Ejemplos |
|---|---|---|
| Un caracter | el caracter mismo | `"n"`, `"P"`, `"+"`, `"*"`, `"!"`, `"["` |
| Espacio | `space` | `"space"` |
| Teclas con nombre | `enter`, `esc` (o `escape`), `tab`, `backspace` | `"enter"` |
| Flechas y navegacion | `left`, `right`, `up`, `down`, `home`, `end`, `pageup`, `pagedown` | `"ctrl+right"` |
| Teclas de funcion | `f1` a `f12` | `"f5"`, `"alt+f2"` |
| Ctrl | `ctrl+` (o `control+`) delante | `"ctrl+p"`, `"ctrl+left"` |
| Alt | `alt+` delante | `"alt+x"`, `"alt+enter"` |
| Ctrl y Alt juntos | los dos delante, en cualquier orden | `"ctrl+alt+p"` |
| Shift con una letra | `shift+` y la letra, o la letra en mayuscula | `"shift+p"` es lo mismo que `"P"` |
| Alt, Shift y una letra | `alt+shift+` y la letra | `"alt+shift+p"` (se muestra `Alt+P`) |
| Shift y Tab | `shift+tab` | `"shift+tab"` |
| La tecla `+` | `"+"` sola; con modificadores, `"ctrl++"` | `"ctrl++"` |

Los nombres y modificadores no distinguen mayusculas (`"CTRL+P"` es
`"ctrl+p"`).

**No permitidas** (cada una es un error: se muestra en la ultima fila con la
accion y la tecla, y se mantiene la configuracion anterior):

| Que | Ejemplo | Por que |
|---|---|---|
| Secuencias de teclas | `"g g"`, `"ctrl+x ctrl+s"` | cada atajo es una sola tecla (las secuencias estan planeadas, ver [roadmap.md](roadmap.md)) |
| Dos teclas a la vez sin modificador | `"a+b"` | solo `ctrl`, `alt` y `shift` se combinan con una tecla |
| Shift con algo que no sea una letra o Tab | `"shift+left"`, `"shift+enter"`, `"shift+f5"`, `"shift+space"` | la terminal no distingue Shift ahi |
| Shift con un numero o simbolo | `"shift+1"`, `"shift+/"` | la terminal manda otro caracter: se escribe ese (`"!"`, `"?"`) |
| Ctrl, Shift y una letra | `"ctrl+shift+p"` | con Ctrl la terminal no distingue mayusculas: seria `"ctrl+p"` |
| Otros modificadores | `"super+p"`, `"meta+p"`, `"cmd+p"` | la terminal no los manda |
| Otras teclas con nombre | `"insert"`, `"delete"`, `"f13"` | no se soportan por ahora |
| Vacia o varios caracteres | `""`, `"ab"`, `"ctrl+"` | no es una tecla |

**Permitidas pero con reglas especiales:**

- `Ctrl+C` siempre sale, se configure lo que se configure: una configuracion
  rota nunca te deja sin salida.
- Mientras se escribe en Buscar, las teclas sin Ctrl ni Alt son texto
  (`toggle = "x"` escribe una x ahi); los atajos con Ctrl o Alt siguen
  funcionando.
- Con Ctrl, mayusculas y minusculas son la misma tecla (`"ctrl+P"` es
  `"ctrl+p"`).
- Una tecla no puede estar en dos acciones; ver las reglas despues de la
  tabla.

### Acciones

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
| `actions` | `ctrl+p` | Acciones y busqueda rapida |
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
| `queue_move_up` | `K` | Cola (Reproduciendo): subir la cancion elegida |
| `queue_move_down` | `J` | Cola (Reproduciendo): bajar la cancion elegida |
| `queue_remove` | `x` | Cola (Reproduciendo): quitar la cancion elegida |
| `queue_add_to_playlist` | `a` | Cola (Reproduciendo): agregar la cancion elegida a una playlist |

- `Ctrl+C` siempre sale, aunque se configure otra cosa.
- Mientras se escribe en Buscar, las teclas sin Ctrl ni Alt son texto.
- Una tecla invalida es un error (se muestra y se mantiene la configuracion
  anterior); una accion desconocida se avisa.
- **Una tecla repetida (segun la regla de arriba) bloquea la TUI**, al iniciar o al
  guardar el archivo: una ventana muestra todas las teclas repetidas con sus
  acciones, agrupadas por tecla (se desplaza con las flechas, `j` / `k` o la
  rueda). Solo se puede desplazar y salir (`q`, `Ctrl+C`); la musica sigue
  sonando. Al corregir `[keys]` y guardar, la TUI se desbloquea sola, sin
  reiniciar.
- Las acciones `lyrics_*` solo valen en la vista de Letras. Una tecla puede
  usarse en vistas distintas (cada una vale solo en la suya), pero nunca en
  una vista y una accion global a la vez, en dos acciones globales, ni dos
  veces en la misma vista: asi todas las funciones quedan al alcance en todas
  las vistas (por ejemplo, `m` silencia en todas, Letras incluida).
- Las teclas que se ven en pantalla (por ejemplo las de la cabecera de Letras
  y las del volumen en Reproduciendo) son siempre las configuradas: si se
  cambia una tecla, la pantalla muestra la nueva. `Ctrl+P` > *Ver atajos de
  teclado* muestra cada accion con sus teclas actuales.
- Las demas teclas de cada vista (`h` / `l` en Biblioteca, etc.) son fijas
  por ahora.

## `[lyrics]`

Como muestra la letra la vista de Letras.

| Clave | Valores | Por defecto |
|---|---|---|
| `mode` | `"classic"` o `"karaoke"` | `"classic"` |
| `size` | `2` a `8` (filas) | `3` |
| `lines_before` | `1` a `2` | `2` |
| `lines_after` | `1` a `3` | `3` |
| `font` | ruta a un archivo de fuente (`.ttf`, `.otf`, `.ttc`), absoluta o que empiece con `~/` | la elige fontconfig |

```toml
[lyrics]
mode = "karaoke"
size = 4
# font = "~/.local/share/fonts/MiFuente-Bold.ttf"
```

- **`classic`**: texto de la terminal; se resalta la palabra que se canta.
- **`karaoke`**: la linea que se canta se dibuja como imagen y se va
  pintando de a poco, por dentro de las letras, a medida que se canta cada
  palabra (como los karaokes de video). `k` en la vista de Letras cambia
  entre los dos modos y guarda la eleccion en el archivo (solo cambia la
  linea `mode` de `[lyrics]`; los comentarios y el resto quedan como estan),
  asi se mantiene la proxima vez.
- **Recarga en vivo**: al guardar el archivo, `size` y `font` se aplican al
  momento. `mode` se aplica cuando cambia en el archivo.
- **`size`** es el alto, en filas, de la linea que se canta en modo
  karaoke. Mas grande se lee mejor de lejos. Una linea que no entra a lo
  ancho se dibuja mas chica.
- **En modo clasico ketai no puede cambiar el tamano del texto**: ahi la
  letra son celdas de la terminal, y su tamano es el de la fuente de la
  terminal, el mismo para toda la pantalla. Se cambia en la terminal (por
  ejemplo `ctrl+shift+equal` en kitty) o se usa el modo karaoke.
- **`lines_before`** y **`lines_after`** son cuantas lineas se ven arriba y
  abajo de la que se canta, que queda en el centro de la vista. Menos de 1 (o
  negativo) es 1; mas del maximo es el valor por defecto (2 arriba, 3
  abajo).
- **`font`** solo se usa en modo karaoke. Sin ella, ketai le pide a
  fontconfig una fuente monoespaciada en negrita y usa otras para los
  caracteres que esa no tiene (por ejemplo japones o coreano).
- El modo karaoke necesita una terminal que informe el tamano de sus celdas
  en pixeles (kitty lo hace, y tambien los multiplexores que lo pasan). Si no
  lo informa, o no se encuentra ninguna fuente, la vista queda en modo
  clasico.

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
