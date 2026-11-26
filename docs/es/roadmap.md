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

### Secuencias de teclas

Hoy cada atajo es una sola tecla, con modificadores (`ctrl+p`). Esta
actualizacion permite atajos de varias teclas apretadas una despues de la
otra, como en Vim o Emacs.

- Secuencias como `g g` o `ctrl+x ctrl+s` en `[keys]` (`top = "g g"`).
- Mientras una secuencia esta a medio escribir, la TUI espera la tecla
  siguiente un tiempo corto y muestra lo que se lleva escrito.
- La revision de teclas repetidas (que bloquea la TUI) tambien cubre las
  secuencias: una secuencia no puede empezar con una tecla que ya es un
  atajo entero, porque ese atajo ganaria siempre.
- Antes de empezar se decide cuanto esperar entre teclas y si se puede
  configurar.

### Cliente grafico de karaoke (video y shaders)

Un cliente aparte, con ventana propia, por ejemplo `ketai-karaoke`, que se
conecta al mismo `ketaid` por el protocolo, como la TUI. Es el lugar para lo
que una terminal no hace bien: shaders en la GPU y video a 60 cuadros por
segundo.

- **Letra de karaoke dibujada con shaders**: cada palabra se va pintando de a
  poco, por dentro de las letras, a medida que se canta, como en los
  karaokes de video.
- **Video de fondo opcional** detras de la letra, que se activa o desactiva
  en `config.toml`. Antes de empezar se define de donde sale el video (por
  ejemplo un archivo junto a la cancion, o un video que configure el
  usuario).
- Hecho con una biblioteca de GPU como `wgpu`; el video se decodifica con una
  biblioteca como FFmpeg.
- **Sincronizacion precisa**: hoy el daemon informa la posicion una vez por
  segundo y los clientes la interpolan. El video necesita un reloj mas
  preciso, asi que el protocolo podria sumar un evento de posicion mas fino
  o una marca de tiempo en cada aviso; es un agregado opcional y no rompe a
  los clientes existentes.
- La TUI mantiene su propia vista de letras. El video no va en la TUI: en
  una terminal cada cuadro es una imagen completa que viaja como datos, algo
  pesado e inestable dentro de multiplexores como herdr.

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

### Compatibilidad con Windows

v1 corre en Linux: depende de sockets Unix, systemd, MPRIS (D-Bus) y del
protocolo de graficos de kitty. Esta actualizacion hace que ketai funcione
en Windows.

- El daemon escucha en un named pipe o en un puerto TCP local en lugar de un
  socket Unix, con el mismo protocolo.
- Arranca con la sesion mediante un mecanismo de Windows (por ejemplo una
  tarea programada o un servicio de usuario) en lugar de systemd,
  manteniendo la idea de arrancar solo cuando un cliente lo necesita, si
  Windows lo permite.
- Teclas multimedia y el panel multimedia del sistema mediante la API de
  Windows (System Media Transport Controls), en lugar de MPRIS.
- La TUI funciona en Windows Terminal; las caratulas solo se muestran donde
  la terminal soporta imagenes.
- Antes de empezar se decide como se elige la salida de audio en Windows y
  que funciones quedan solo para Linux.

### Conexion de clientes moviles con codigo QR

Hoy los clientes se conectan por un socket Unix local. Esta actualizacion
permite que un telefono en la misma red se conecte al daemon escaneando un
codigo QR.

- El daemon puede escuchar en la red local (apagado por defecto), con
  cifrado y autenticacion: nadie se conecta sin emparejarse.
- La TUI muestra un codigo QR con la direccion y una clave de
  emparejamiento; escanearlo alcanza para conectarse, sin escribir nada.
- Los dispositivos emparejados se pueden listar y revocar.
- Requiere un transporte de red que hoy no existe: el protocolo sigue igual,
  solo cambia por donde viaja.
- Antes de empezar se decide que esquema de cifrado se usa y cuanto dura una
  clave de emparejamiento.

### Cliente movil

Una app para el telefono que controla `ketaid` como lo hace la TUI:
reproducir, cola, biblioteca, letras y caratulas.

- Se conecta con el emparejamiento por QR de la actualizacion anterior, que
  es un requisito.
- Primero control remoto: el audio sigue sonando en la computadora.
- Antes de empezar se decide que plataformas van primero (Android, iOS) y
  con que tecnologia se construye.
