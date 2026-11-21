# Servicio del daemon

`ketaid` se instala como servicio de usuario de systemd para no tener que
levantarlo a mano.

## Modos de arranque

| Modo | Como se activa | Cuando arranca |
|------|----------------|----------------|
| Socket activation (recomendado) | `systemctl --user enable --now ketaid.socket` | Cuando un cliente se conecta por primera vez |
| Siempre activo | `systemctl --user enable --now ketaid.service` | Al iniciar la sesion del usuario |
| Manual | `ketaid` | Cuando se ejecuta |

Con socket activation, systemd crea los sockets al iniciar la sesion y
levanta el daemon recien cuando la TUI, waybar u otro cliente se conecta. No
consume recursos mientras nadie lo usa.

## Contrato del daemon

- **Sockets heredados**: si existe `LISTEN_FDS`, el daemon usa los sockets
  que le pasa systemd en el orden del `.socket` (primero `ctl.sock`,
  despues `viz.sock`). Si no existe, crea los sockets por su cuenta en
  `$XDG_RUNTIME_DIR/ketai/`.
- **Listo para usar**: envia `READY=1` por `sd_notify` cuando ya acepta
  conexiones (`Type=notify`).
- **Instancia unica**: si `ctl.sock` ya responde, una segunda instancia
  manual termina con error en lugar de abrir otra salida de audio.
- **Apagado limpio**: ante `SIGTERM` o `SIGINT` guarda la sesion (ver
  "Persistencia de sesion") y libera la salida de audio antes de salir.
- **Reinicio ante fallas**: `Restart=on-failure` con un limite de 5 intentos
  por minuto para evitar bucles.

## Persistencia de sesion

El objetivo es que al volver a abrir ketai la sesion de musica quede como
estaba: misma cola, misma pista, misma posicion y mismo volumen.

### Que se guarda

| Campo | Ejemplo | Notas |
|-------|---------|-------|
| Cola | lista de URIs en orden | Incluye el orden actual aunque `shuffle` este activo |
| Indice actual | `4` | Pista que estaba sonando o pausada |
| Posicion | `83400` ms | Posicion dentro de la pista actual |
| Volumen | `0.65` | |
| Mute | `false` | |
| Repeat / shuffle | `"queue"` / `true` | |
| Salida de audio | `"alsa_output.usb-headset"` o `null` | `null` = seguir el default del sistema |
| Estado | `"playing"` / `"paused"` | Solo informativo, ver "Restauracion" |

### Donde

`$XDG_STATE_HOME/ketai/session.json` (por defecto
`~/.local/state/ketai/session.json`). Es estado de la sesion, no
configuracion, por eso no va en `~/.config`.

El archivo lleva un campo `version` para poder migrarlo en el futuro.

### Cuando se guarda

Guardar solo al apagar no alcanza: si el equipo se corta o el daemon se
cuelga, `SIGTERM` nunca llega. Por eso se guarda en tres momentos:

1. **Al apagar** (`SIGTERM`/`SIGINT`): guardado final, con la posicion
   exacta.
2. **Al cambiar algo**: cola, pista actual, volumen, modos o salida. Los
   cambios seguidos se agrupan y se escriben como maximo una vez por segundo.
3. **Periodicamente mientras reproduce**: la posicion se guarda cada 10
   segundos. En el peor caso, tras un corte se pierden 10 segundos de
   posicion.

### Escritura segura

Para que un corte a mitad de escritura no deje el archivo roto:

1. Escribir en `session.json.tmp`.
2. `fsync` del archivo temporal.
3. `rename` atomico a `session.json`.

Si al arrancar `session.json` no se puede leer, se renombra a
`session.json.bad`, se registra el error y se arranca con una sesion vacia.

### Restauracion

- Al arrancar se reconstruye la cola, la pista actual, la posicion, el
  volumen, los modos y la salida.
- La reproduccion **queda en pausa** en la posicion guardada, aunque al
  apagar estuviera sonando. Asi el audio nunca arranca solo al iniciar la
  sesion; el usuario retoma con `player.play`.
- Si la salida guardada ya no existe, se usa el default del sistema.
- Si un archivo de la cola ya no existe, esa pista se quita de la cola. Si
  era la pista actual, la sesion queda en la siguiente pista disponible, en
  la posicion `0`.
- Las pistas remotas se restauran por URI; se vuelven a descargar o se leen
  del cache cuando se reproducen.

## Clientes

Cuando un cliente (por ejemplo la TUI) no puede hablar con el daemon, no
muestra comandos para copiar: ofrece dos funciones.

### 1. Buscar el servicio

Diagnostica en que estado esta el daemon, en este orden:

1. Conectar a `ctl.sock` y enviar `daemon.hello`.
2. Consultar a systemd por D-Bus (`org.freedesktop.systemd1`, bus de
   usuario) el estado de `ketaid.socket` y `ketaid.service`: si existen,
   si estan activos y si estan habilitados.
3. Buscar un proceso `ketaid` del usuario que no este bajo systemd.

Resultados posibles:

| Estado | Que significa | Que ofrece el cliente |
|--------|---------------|-----------------------|
| `connected` | El daemon responde | Nada, se conecta |
| `starting` | El socket o el servicio estan activos pero el daemon todavia no responde | Reintentar durante unos segundos |
| `stopped` | Las unidades estan instaladas pero inactivas | Boton "Iniciar" |
| `not_installed` | No hay unidades de systemd del usuario | Boton "Instalar e iniciar" |
| `unresponsive` | Hay un proceso `ketaid` pero no responde por el socket | Boton "Reiniciar" (con confirmacion) |
| `no_systemd` | No hay systemd de usuario disponible | Boton "Iniciar" en modo manual |

### 2. Levantar el servicio con un boton

El cliente siempre levanta el daemon **a traves de systemd**, nunca como
proceso hijo propio. Asi no aparecen instancias fuera de control.

- **Iniciar**: `StartUnit("ketaid.socket")` por D-Bus. El socket levanta el
  daemon en la primera conexion.
- **Iniciar siempre al entrar a la sesion**: casilla opcional junto al
  boton; ademas llama a `EnableUnitFiles` para `ketaid.socket`.
- **Instalar e iniciar**: copia las unidades a `~/.config/systemd/user/`,
  llama a `Reload` y despues inicia como arriba.
- **Reiniciar**: `RestartUnit("ketaid.service")`. Si el proceso colgado no
  esta bajo systemd, se le envia `SIGTERM` y, si no termina en 5 segundos,
  `SIGKILL`; despues se inicia normalmente.
- **Modo manual** (sin systemd): el cliente lanza `ketaid --daemonize`
  desacoplado de la terminal. El daemon sigue protegido por el bloqueo de
  instancia unica.

Despues de cualquier accion, el cliente vuelve a ejecutar la busqueda y
muestra el resultado. Si falla, muestra el error de systemd o del daemon.

### Instancia unica

El daemon hace su propia revision al arrancar, ademas de la que hace el
cliente. El arbitro es un bloqueo exclusivo (`flock`) sobre
`$XDG_RUNTIME_DIR/ketai/ketaid.lock`:

- Solo un proceso puede tener el bloqueo a la vez.
- El kernel lo libera automaticamente si ese proceso muere, aunque sea con
  `SIGKILL`.
- Solo el dueno del bloqueo puede cerrar otros procesos. Como hay un unico
  dueno, dos instancias que arrancan a la vez no pueden matarse entre si.

#### Secuencia de arranque

1. **Intentar tomar el bloqueo** (sin esperar).
2. **Si el bloqueo ya tiene dueno:**
   1. Enviar `daemon.hello` a `ctl.sock` con 2 segundos de timeout.
   2. Si responde: ya hay una instancia en uso. La nueva instancia registra
      el motivo y **termina con codigo de salida 0**, sin abrir audio.
   3. Si no responde: el dueno esta colgado. Se lee su PID desde el archivo
      de bloqueo, se valida (ver "Validacion de procesos"), se envia
      `SIGTERM` y, si sigue vivo a los 5 segundos, `SIGKILL`.
   4. Volver al paso 1. Si tras 3 intentos no se consigue el bloqueo,
      terminar con error.
3. **Si se consigue el bloqueo:** esta instancia es la duena.
   1. Escribir su PID en el archivo de bloqueo.
   2. **Escanear** procesos `ketaid` huerfanos (ver abajo) y cerrarlos.
   3. Continuar el arranque normal: sockets, sesion, audio.

#### Escaneo de huerfanos

Lo hace solo la instancia duena del bloqueo. Un huerfano es cualquier
proceso `ketaid` del mismo usuario que no es la instancia duena; por
ejemplo, restos de una falla o de una version vieja.

1. Recorrer `/proc` y seleccionar los procesos que pasan la validacion.
2. Enviar `SIGTERM` a cada uno.
3. Esperar hasta 5 segundos; a los que sigan vivos, enviar `SIGKILL`.
4. Registrar en el log el PID, el ejecutable y la senal usada de cada
   proceso cerrado.

#### Validacion de procesos

Antes de enviar cualquier senal, el proceso debe cumplir todo esto:

- Pertenece al mismo usuario (UID real) que el daemon.
- `/proc/<pid>/exe` apunta a un binario `ketaid`. No alcanza con el nombre
  del proceso, que se puede falsificar.
- No es el propio proceso.
- Pertenece a la misma sesion: su `XDG_RUNTIME_DIR` (leido de
  `/proc/<pid>/environ`) es igual al del daemon. Un `ketaid` de otra sesion
  no es huerfano de esta.
- Justo antes de enviar la senal, se vuelve a comprobar que el PID sigue
  siendo el mismo proceso (mismo tiempo de inicio en `/proc/<pid>/stat`),
  porque un PID puede reutilizarse.

Si alguna validacion falla, el proceso no se toca.

#### Resultado

- Si hay una instancia sana, queda esa y la nueva se cierra sola.
- Si no hay ninguna sana, la que arranca toma el bloqueo y es la que queda.
- En cualquier caso, al terminar el arranque hay como maximo un `ketaid`
  por sesion (`XDG_RUNTIME_DIR`).

## Archivos

- `dist/systemd/ketaid.socket`
- `dist/systemd/ketaid.service`

Instalacion manual:

```sh
install -Dm644 dist/systemd/ketaid.{socket,service} -t ~/.config/systemd/user/
systemctl --user daemon-reload
systemctl --user enable --now ketaid.socket
```
