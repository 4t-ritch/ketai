# Cola de canciones en Reproduciendo

Objetivo: ver y editar la cola desde la TUI (Reproduciendo), con caratulas, reordenar con mouse y teclado, quitar y agregar a playlist.
Rama: `feat/queue` (local, sale de `dev` en b4c3d70). Entrega: commits por tarea; la integracion la decide el usuario.
Tests: `cargo test` (workspace).

## Specs

- S1: Usuario: "elimina esto de todas las vistas" (el indicador "● ketaid" y "vorbis · 48 kHz").
- S2: Usuario: "en esa zona coloca la cola de canciones porque no es posible saber que canciones hay porque en ninguna vista se muestra la cola de canciones, por eso es importante que como minimo en la vista de reproduciendo se vea la cola de canciones". Zona aclarada con captura: "que la lista de canciones en cola te marque donde queria con color rojo" (columna a la derecha de Reproduciendo, desde arriba hasta la barra de progreso).
- S3: Usuario: "la cola de canciones debe de poder ser editable, es decir, se debe de poder mover las canciones de posicion de manera simple con el mouse haciendo drag and drop, pero tambien con el teclado".
- S4: Usuario: "tambien se deben poder agregar las canciones a una playlist sin tener que tenerla en reproduccion(mouse y teclado)".
- S5: Usuario: "se debe poder eliminar de la cola de canciones una cancion."
- S6: Usuario: "deben de poder verse las caratulas de las canciones que se van a reproducir, se debe de poder ver como minimo el nombre de la canciones(si es muy grande se debe aplicar el scroll horizontal) y el nombre del artista de la cancion."

## Tasks

| ID | Specs | Ruta | Estado | Evidencia |
|----|-------|------|--------|-----------|
| T1 | S1 | inline | hecho | c5106f0: sin indicador conectado (queda el aviso sin conexion) ni formato de audio |
| T2 | S2, S6 | inline | hecho | Columna de cola: caratula, titulo con desplazamiento, artista; cursor y desplazamiento |
| T3 | S3, S5 | inline | hecho | Teclado: elegir, mover arriba/abajo, quitar, Enter reproduce; teclas configurables (alcance Reproduciendo) |
| T4 | S4 | inline | hecho | Agregar la cancion elegida de la cola a una playlist (popup con destino propio) |
| T5 | S3, S4, S5 | inline | pendiente | Mouse: clic elige, doble clic reproduce, arrastrar y soltar mueve, botones quitar y playlist, rueda |
| T6 | S2-S6 | inline | pendiente | Docs en ingles y espanol |

## Log

- L1 (2026-11-24) Pedido original: "elimina esto de todas las vistas [indicador ketaid y formato de audio] en esa zona coloca la cola de canciones porque no es posible saber que canciones hay porque en ninguna vista se muestra la cola de canciones, por eso es importante que como minimo en la vista de reproduciendo se vea la cola de canciones, tambien la cola de canciones debe de poder ser editable, es decir, se debe de poder mover las canciones de posicion de manera simple con el mouse haciendo drag and drop, pero tambien con el teclado, tambien se deben poder agregar las canciones a una playlist sin tener que tenerla en reproduccion(mouse y teclado), se debe poder eliminar de la cola de canciones una cancion. y para agregarle mas valor, deben de poder verse las caratulas de las canciones que se van a reproducir, se debe de poder ver como minimo el nombre de la canciones(si es muy grande se debe aplicar el scroll horizontal) y el nombre del artista de la cancion."
- L2 Protocolo: el daemon ya tiene queue.list/move/remove/jump y queue.changed; trabajo solo en la TUI. Decision: el aviso "ketaid sin conexion" se mantiene (solo se oculta el estado conectado).
- L3 (2026-11-24) Usuario aclaro la zona con una captura (columna derecha). La vista previa de letra y lo demas quedan.
- T2 e610d5f: columna de cola (la informacion tiene prioridad de ancho: INFO_MIN_W 62).
- (2026-11-24) Usuario: "ahorita dice que no tengo mas canciones en cola, pero si activo el repeat all, y pulso next me aparecen en cola todas las canciones de una playlist que tuve antes de poder la cancion en curso" y "no coincide la cancion que muestra en la lista de la cola con la cancion que esta sonando en curso". Causa: la columna mostraba solo lo posterior a la actual; la cola del daemon es lineal y con repeat queue vuelve al principio; en shuffle el siguiente se elige al azar al pasar. Arreglo: cola completa en orden real (actual marcada, las ya sonadas tenues), nota "se repite al terminar" / "orden aleatorio"; posiciones = indices de queue.*; la actual no se quita.
- T3/T4: acciones queue_move_up K, queue_move_down J, queue_remove x, queue_add_to_playlist a (alcance Reproduciendo, chocan con globales); popup de playlist con destino propio (add_uri) y track.playlists asignado por id de peticion (sent_with).
- Siguiente paso: T5 (mouse).
