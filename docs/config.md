# Configuracion

La TUI lee `$XDG_CONFIG_HOME/ketai/config.toml` (por defecto
`~/.config/ketai/config.toml`). Si no existe, usa los valores por defecto.
Hay un ejemplo comentado en `dist/config.toml`.

## Formato

TOML: declarativo, admite comentarios y secciones. Cada funcion
personalizable tiene su seccion (`[theme]` por ahora); las secciones nuevas
se agregan sin romper los archivos existentes.

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

Con `"cover"`, los colores salen de la caratula que suena: el fondo es el
color promedio oscurecido y el acento es el mismo tono, claro y saturado
para que se lea. Sin caratula se usan los colores por defecto.
