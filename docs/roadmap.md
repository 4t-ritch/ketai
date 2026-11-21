# Roadmap

Funcionalidades planificadas fuera de la v1.

## Proxima major update

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
