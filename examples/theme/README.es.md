[English](README.md) | **Espanol**

# Temas

ketai lee los colores de la seccion `[theme]` de
`~/.config/ketai/config.toml` (o `$XDG_CONFIG_HOME/ketai/config.toml`). Los
cambios se aplican al guardar el archivo, sin cerrar ketai.

Por ahora hay **un solo tema activo**: el que esta en `config.toml`. Tener
varios temas guardados y elegir entre ellos esta planificado para una major
update (ver `docs/es/roadmap.md`).

En esta carpeta:

| Archivo | Para que sirve |
|---|---|
| `cover.toml` | Toda la interfaz con los colores de la caratula que suena |
| `dark.toml` | Tema oscuro fijo, sin tomar nada de la caratula |
| `cover-accent.toml` | Fondo de la caratula y un acento fijo elegido por vos |
| `defaults.toml` | Todos los colores con su valor por defecto, para copiar y editar |

Para usar uno, copia su seccion `[theme]` a tu `config.toml`. Si ya tenes
otras secciones (`[keys]`, `[playlists]`), dejalas como estan. Los comentarios
dentro de los archivos `.toml` estan en ingles; la explicacion en espanol es
esta guia.

## De donde salen los colores: `cover_color`

La primera decision es `cover_color`:

```toml
[theme]
cover_color = true
```

| Valor | Que pasa |
|---|---|
| `true` | Toda la interfaz sale de la caratula que suena. Los colores escritos a mano **se ignoran**, aunque esten en el archivo. |
| `false` | Solo valen los colores escritos a mano. Donde diga `"cover"`, se usa el color por defecto. |
| sin la linea | Cada color decide por su cuenta: `background` y `accent` pueden ser `"cover"` o un color fijo. |

Con `cover_color = true` (o con `background = "cover"` y `accent = "cover"`)
los colores cambian con cada cancion:

- todo sale del **tono predominante** de la caratula (el color vivo que mas
  aparece, no el promedio);
- el **fondo** es ese tono, oscuro;
- los **paneles** (`raised`) y la **fila elegida** (`row`) son el mismo color
  un poco mas claro;
- el **acento** es el mismo tono, claro y saturado, para que se lea sobre el
  fondo;
- los **textos** (`foreground`, `dim`, `faint`) son claros con un toque del
  tono de la caratula y los **controles** usan el acento; con
  `ui_text_cover_color = false` quedan blancos y grises;
- todos los colores mantienen contraste con el fondo: si uno no se leeria,
  se aclara u oscurece en el mismo tono;
- el **espectro** va del acento a un tono vecino.

Si la caratula es gris (sin color), todo queda en grises. Mientras no hay
caratula (nada sonando, o una cancion sin imagen), se usan los colores por
defecto.

## Colores que se pueden cambiar

Todos se escriben como `"#rrggbb"` (hexadecimal, como en CSS).

| Clave | Que pinta | Por defecto |
|---|---|---|
| `background` | Fondo de toda la pantalla. Tambien admite `"cover"` | `"cover"` |
| `accent` | Lo activo: pestana elegida, cursor, botones, barra de progreso. Tambien admite `"cover"` | `"cover"` |
| `foreground` | Texto principal: titulos, nombres | `"#ffffff"` |
| `dim` | Texto secundario: artista, detalles | `"#c8d3e0"` |
| `faint` | Texto de ayuda: atajos, etiquetas, fechas | `"#7f8ea3"` |
| `raised` | Paneles y fondos elevados, barras vacias | `"#1c2a3a"` |
| `on_accent` | Texto sobre el acento (por ejemplo, en un boton) | `"#04121f"` |
| `row` | Fila bajo el cursor | `"#16202e"` |
| `ok` | Estado correcto (el punto de "ketaid" conectado) | `"#7ee787"` |
| `error` | Errores | `"#ff7b8a"` |
| `spectrum_top` | Parte alta del espectro | `"#ff7ab6"` |

Lo que no esta en la tabla sale de estos colores: el fondo de los popups es
el fondo un poco mas oscuro y las lineas de letra que todavia no se cantan
son `faint` mezclado con el fondo. Nada en la interfaz tiene un color propio
fijo.

`ok` y `error` son colores de estado: no siguen a la caratula, para que un
error siempre se distinga.

## Combinaciones utiles

**Todo de la caratula** (`cover.toml`):

```toml
[theme]
cover_color = true
```

**Todo fijo** (`dark.toml`): `cover_color = false` y los colores que quieras.

**Fondo de la caratula con tu acento** (`cover-accent.toml`): sin
`cover_color`, con `background = "cover"` y `accent = "#rrggbb"`. Con
`background = "cover"` tambien salen de la caratula `raised`, `row`, `dim`
y `faint`, salvo los que fijes a mano: un color escrito a mano siempre gana.

## Errores y avisos

- **Un color mal escrito** (`"#zzz"`, `"rojo"`) es un error: aparece en la
  ultima fila con la clave, y ketai sigue con los colores anteriores hasta
  que lo corrijas.
- **Una clave que no existe** se avisa y se ignora; el resto se aplica.
- **Si borras el archivo**, vuelven los colores por defecto.

## Consejos

- Proba con `cover_color = true` primero: es lo que mejor queda con la
  mayoria de las caratulas.
- Si escribis un tema fijo, mantene buen contraste entre `background` y
  `foreground`, y entre `accent` y `on_accent`.
- Los cambios se ven al guardar: deja ketai abierto mientras editas.
