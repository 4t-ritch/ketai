# MPRIS 2

El daemon se publica en el bus de sesion de D-Bus como
`org.mpris.MediaPlayer2.ketai`, en el objeto `/org/mpris/MediaPlayer2`. Asi
lo manejan playerctl, los modulos `mpris` de waybar y las teclas multimedia
del escritorio, sin hablar el protocolo de ketai.

Si el nombre ya esta tomado en el bus, se usa
`org.mpris.MediaPlayer2.ketai.instance<pid>`, como pide la especificacion.
Si no hay bus de sesion, el daemon lo registra y sigue funcionando sin MPRIS.

## Como funciona

- Un thread propio es el dueno de la conexion D-Bus (libdbus, sin async).
- Las propiedades se responden desde una copia del estado que el loop
  central le manda despues de cada cambio. Leer una propiedad nunca espera
  al loop central.
- Los metodos y las propiedades escribibles se traducen a peticiones del
  protocolo y viajan al loop central como cualquier otra peticion.
- Los pedidos de MPRIS tienen el mismo tope que un cliente: hasta 64 sin
  atender. Si un programa inunda el bus de llamadas, el thread de MPRIS
  espera al loop central en vez de llenar su cola. No se pierde ninguno.
- Al escribir `Volume`, `LoopStatus` o `Shuffle`, la copia se actualiza al
  instante: una lectura inmediata ya ve el valor nuevo.
- Los cambios se anuncian con `PropertiesChanged`. `Position` no se anuncia:
  un salto de posicion (seek) emite `Seeked`, como pide la especificacion.

## Metodos

| MPRIS | Peticion del protocolo |
|---|---|
| `Play` | `player.play` |
| `Pause` | `player.pause` |
| `PlayPause` | `player.toggle` |
| `Stop` | `player.stop` |
| `Next` | `player.next` |
| `Previous` | `player.previous` |
| `Seek(offset_us)` | `player.seek_relative`; si pasa el final de la pista actua como `Next` (sin efecto si no hay siguiente) |
| `SetPosition(track, pos_us)` | `player.seek`; se ignora si `track` no es la pista actual o la posicion esta fuera de rango |
| `OpenUri(uri)` | `queue.add` (solo `file://`) |
| `Raise` | no hace nada (`CanRaise` es false) |
| `Quit` | error (`CanQuit` es false): el daemon no se cierra desde MPRIS |

## Propiedades

| MPRIS | Origen |
|---|---|
| `PlaybackStatus` | `status`: `Playing`, `Paused` o `Stopped` |
| `LoopStatus` | `repeat`: `off` = `None`, `track` = `Track`, `queue` = `Playlist` |
| `Shuffle` | `shuffle` |
| `Volume` | `volume`, o 0 si `muted`. Al escribir se limita a 0..1 (ketai no amplifica) y un valor mayor a 0 quita el mute |
| `Position` | ultima posicion conocida mas el tiempo transcurrido si esta sonando, en microsegundos |
| `Metadata` | `mpris:trackid` (`/org/ketai/track/<track_id>`), `xesam:url`, `mpris:length`, `xesam:title`, `xesam:artist`, `xesam:album` |
| `CanPlay`, `CanPause` | hay pistas en la cola |
| `CanSeek` | la pista actual tiene duracion conocida |
| `CanGoNext` | hay pista siguiente o la repeticion esta activa |
| `CanGoPrevious` | hay pista actual |
| `CanControl` | siempre true |
| `Rate`, `MinimumRate`, `MaximumRate` | 1.0 (no hay cambio de velocidad) |

`HasTrackList` es false: la interfaz `TrackList` no se implementa. La cola se
maneja con el protocolo de ketai.

## Pruebas

Los tests de conformidad (`crates/ketaid/tests/mpris.rs`) levantan un
`dbus-daemon` propio por test. El resto de la suite apunta
`DBUS_SESSION_BUS_ADDRESS` a una ruta inexistente: ningun daemon de test
aparece en el bus del usuario.
