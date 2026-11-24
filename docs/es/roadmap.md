[English](../roadmap.md) | **Espanol**

# Roadmap

Funcionalidades planificadas fuera de la v1.

## Proxima major update

### Fuentes remotas con descarga a cache

La v1 solo reproduce archivos locales (`file://`). Esta actualizacion agrega
pistas remotas.

- Fuentes `http(s)://` que apuntan a archivos de audio. Antes de empezar se
  define si se suma algun otro tipo de fuente.
- Cliente HTTP con TLS sincronico (por ejemplo `ureq` con `rustls`), en un
  thread propio, sin async.
- Descarga a `$XDG_CACHE_HOME/ketai/` con limite de tamano; la reproduccion
  arranca mientras descarga (`status: "buffering"`).
- Evento `download.progress` (topico `cache`), ya reservado en el protocolo.
- Letras y caratulas para pistas remotas.

### Instalacion con gestores de paquetes

Hoy ketai se compila desde el codigo fuente. Esta actualizacion lo deja
instalable con un comando, con el daemon y la TUI juntos.

- Paquetes para los gestores mas usados en Linux (por ejemplo un PKGBUILD
  para el AUR, y paquetes para otras distribuciones a definir), con los
  binarios, las unidades de systemd de usuario, la documentacion y los
  ejemplos de configuracion.
- Al instalar, el servicio queda listo para activarse
  (`systemctl --user enable --now ketaid.socket`).
- Binarios precompilados en cada release, para quien no quiera compilar.
- Antes de empezar se revisa la licencia de FDK AAC para distribuir binarios
  (ver [formats.md](formats.md#licencias)).

### Varios temas

La v1 tiene un solo tema activo: la seccion `[theme]` de `config.toml`. Esta
actualizacion permite tener varios guardados y elegir cual usar.

- Temas en archivos propios (por ejemplo `~/.config/ketai/themes/<nombre>.toml`),
  con las mismas claves que `[theme]`.
- Elegir el tema activo desde `config.toml` y desde la TUI (por ejemplo, en
  el popup de acciones), con recarga en vivo.
- Temas incluidos con ketai y posibilidad de compartirlos.
- Antes de empezar se define como conviven un tema elegido y `cover_color`
  (por ejemplo, un tema que diga que colores toma de la caratula).

### Varios idiomas en la interfaz

La v1 muestra la interfaz en espanol (con algunos textos en ingles, como
"Default" y "No device" en la salida de audio). Esta actualizacion permite
elegir el idioma.

- Todos los textos de la TUI salen de un catalogo por idioma, en vez de
  estar escritos en el codigo.
- El idioma se elige en `config.toml` (por ejemplo `[ui] language = "en"`)
  o se toma del sistema (`LANG`), con recarga en vivo.
- Idiomas iniciales: espanol e ingles. Agregar uno nuevo es sumar un
  catalogo.
- Antes de empezar se define que pasa con los mensajes del daemon (errores
  del protocolo): hoy estan en ingles y los clientes podrian traducirlos por
  codigo de error.

### Romanizacion automatica de letras

Generar la letra romanizada cuando el origen no la trae.

- **Coreano** (hangul): se resuelve con reglas, sin diccionario. El hangul es
  silabico y la romanizacion revisada se aplica por bloques.
- **Cirilico** (ruso y otros): se resuelve con tablas de transliteracion por
  idioma.
- **Japones**: necesita diccionario para leer los kanji (por ejemplo MeCab o
  kakasi). Los kana se resuelven con tablas.
- **Chino**: necesita diccionario de pinyin, y los caracteres con varias
  lecturas requieren contexto.

Impacto en el protocolo:

- Las capas generadas aparecen en `lyrics.layers` con `origin: "generated"`.

- Por ser texto generado, debe poder desactivarse por idioma en la
  configuracion.

### Estimacion de tiempos por palabra basada en diccionario

Reemplazar la estimacion proporcional por caracteres (`word-estimated`) por
una estimacion fonetica, sin depender de IA, para obtener un resultado
inicial bastante cercano.

Idea:

- Con los diccionarios de la romanizacion automatica, segmentar cada linea
  en unidades foneticas (moras en japones, silabas en coreano y chino,
  silabas en idiomas con alfabeto latino o cirilico).
- Asignar duracion a cada unidad con un modelo simple por idioma. Las
  vocales sostenidas concentran la mayor parte del tiempo de una linea; si se
  conocen la duracion de la linea y la de los sostenidos, el resto se reparte
  entre las unidades cortas.
- La hipotesis inicial (a validar con letras reales sincronizadas por
  palabra) es que este reparto acierta en la gran mayoria de los casos para
  romanizaciones.

Fases:

1. **Romanizacion**: estimar los tiempos de una capa romanizada a partir de
   la duracion de cada linea. Es el caso con menor margen de error porque la
   romanizacion sigue la fonetica del original.
2. **Traduccion**: mas pasos y mas margen de error. Una aproximacion posible
   es trabajar con pares de romanizacion entre idiomas para alinear segmentos
   antes de repartir tiempos.

Impacto en el protocolo:

- Nuevo nivel de tiempo, por ejemplo `word-phonetic`, distinto de
  `word-estimated`, para que el cliente sepa que metodo se uso.
- Medir la precision contra un conjunto de letras con tiempos reales antes de
  activarla por defecto.
