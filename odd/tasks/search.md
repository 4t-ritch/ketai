# Busqueda fluida y tolerante

Objetivo: que buscar no trabe la TUI y que encuentre coincidencias con acentos, dieresis, enie y otras variantes, en cualquier parte del texto.
Rama: `feat/search` (local, sale de `dev` en 7cd6085). Entrega: commits por tarea; la integracion la decide el usuario.
Tests: `cargo test` (workspace).

## Specs

- S1: Sin trabas. Usuario: "necesito que corrijas el sistema de busqueda porque se siente que se traba cuando hago busquedas de canciones en carpeta de ~/music, no son tantas, apenas son como 1.5k de canciones y se traba al realizar una busqueda, la idea es que haga busquedas mas eficientes".
- S2: Variantes de caracteres. Usuario: "Las busquedas deben ser fuertes y resilientes durante la busqueda, esto porque hay canciones que tienen acentos, vigulillas, dieresis, segiña, etc, y para esto el sistema debe de ser capaz de poder recibir esos caracteres, y realizar busquedas con todas las variantes que pudiera impleicar para devolver las listas de coincidencias".
- S3: Coincidencias parciales. Usuario: "ademas de poder realizar busquedas de nombres parciales y no que deban coincidir desde el inicio del nombre."
- S4: Indice SQLite con mas metadatos (propuesta del usuario, pendiente de decision): "lo que recomiendo es que durante el periodo de escaneo se use una db de sqlite para que se haga una scan mas complejo pero que permita realizar las busquedas con mayor precision, asi podemos tener busquedas por nombre, artista, album y mucha mas informacion, hasta se podria ampliar por playlist, tags y mas metadata que podria solo vivir en el sistema de ketai para que permita realizar busquedas de mejor calidad y eficiencia."

## Tasks

| ID | Specs | Ruta | Estado | Evidencia |
|----|-------|------|--------|-----------|
| T1 | S1 | inline | hecho | Caratulas fuera del loop principal, tamano chico, version atenuada solo cuando hace falta, busqueda con espera corta entre teclas, dependencias optimizadas en debug |
| T2 | S2, S3 | inline | hecho | Busqueda sin acentos ni mayusculas (ñ = n, ü = u, ß = ss...), en cualquier parte del texto |
| T3 | S4 | - | por decidir | Indice SQLite con metadatos extendidos |

## Log

- L1 (2026-11-22) Pedido original: "necesito que corrijas el sistema de busqueda porque se siente que se traba cuando hago busquedas de canciones en carpeta de ~/music, no son tantas, apenas son como 1.5k de canciones y se traba al realizar una busqueda, la idea es que haga busquedas mas eficientes, lo que recomiendo es que durante el periodo de escaneo se use una db de sqlite para que se haga una scan mas complejo pero que permita realizar las busquedas con mayor precision, asi podemos tener busquedas por nombre, artista, album y mucha mas informacion, hasta se podria ampliar por playlist, tags y mas metadata que podria solo vivir en el sistema de ketai para que permita realizar busquedas de mejor calidad y eficiencia. Las busquedas deben ser fuertes y resilientes durante la busqueda, esto porque hay canciones que tienen acentos, vigulillas, dieresis, segiña, etc, y para esto el sistema debe de ser capaz de poder recibir esos caracteres, y realizar busquedas con todas las variantes que pudiera impleicar para devolver las listas de coincidencias, ademas de poder realizar busquedas de nombres parciales y no que deban coincidir desde el inicio del nombre. pero antes de que continuemos con lo que te estoy pidiendo quiero que hagas el cierre de la rama deamon, y luego hagas el cierre de la rama actual, para despues hacer una nueva rama feat donde podamos esta trabajando estas nuevas features."
- L2 Ramas: feat/daemon ya estaba integrada en dev (3e7cf39) y se borro; feat/tui se integro con `--no-ff` (7cd6085) y se borro; feat/search sale de dev.
- L3 Medicion con ~/music real (1214 archivos, daemon aislado, solo lectura): escaneo 1,5 s; `library.search` 0,3-0,7 ms; 8 caratulas 14 ms; una busqueda detras de 8 caratulas 5 ms. TUI, por caratula: decodificar 205 ms en debug (12 ms release, en otro thread); dos PNG + base64 119 ms en debug (2,5 ms release) EN EL LOOP PRINCIPAL; 520 KB a la terminal. Con hasta 8 caratulas nuevas por tecla: cerca de 1 s congelado y ~4 MB a la terminal en debug. Conclusion: la traba es la TUI, no la busqueda; SQLite no la resuelve. Las coincidencias parciales ya existen (substring); lo que falta es plegar acentos.
- L4 (2026-11-22) T1 hecho. Cambios: dependencias con opt-level 2 en debug (`[profile.dev.package."*"]`); decodificar, achicar, PNG y base64 en un thread (`covers::prepare`), el loop solo escribe (`CoverCache::ready`); caratulas a 320 px; version atenuada solo cuando un popup la necesita (`want_dimmed`, se pide una vez); busqueda 150 ms despues de la ultima tecla (`App::tick`, Buscar y paleta). Medicion en debug despues: loop principal 2 us por caratula (antes 119 ms), 199 KB a la terminal (antes 520 KB), preparacion 92 ms fuera del loop. Cambios de regla deliberados con sus tests actualizados: no hay busqueda por tecla (una sola tras la espera) y la atenuada no se transmite de entrada. Suite TUI 68/68.
- L5 (2026-11-22) T2 hecho: `library::fold` (NFD, quitar U+0300-U+036F, plegados a mano ß/æ/œ/ø/ł/đ/þ/ı, minusculas, NFC). El test de japones encontro un error real del primer intento: sin recomponer, "ながれ" quedaba "なか゛れ" y "なか" coincidia; se recompone con NFC. El texto plegado de cada pista se calcula al indexar (`search_text`), asi la busqueda solo compara: peor caso en debug sobre ~/music real (1214) de 13 ms a 0,4 ms. Coincidencias parciales en cualquier parte, como antes. Tests: 3 unitarios en core + 1 del daemon con nombre de archivo acentuado.
- Siguiente paso: decision del usuario sobre SQLite (T3).
