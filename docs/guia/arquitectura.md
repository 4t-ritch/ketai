# Como esta armado ketai

ketai se divide en **dos programas**: un servicio de audio y un cliente de
terminal. Es la misma idea que mpd con sus clientes: el que reproduce y el
que muestra son cosas separadas.

```
                    +-------------------------------------------+
  ketai (TUI) ----> |  ketaid: servicio de audio                |
  ketai (mini) ---> |                                           | ---> PulseAudio / PipeWire
  waybar, scripts ->|  reproductor, cola, biblioteca (SQLite),   |
  playerctl (MPRIS) |  playlists, letras, salida de audio       |
                    +-------------------------------------------+
                         ctl.sock (control)   viz.sock (espectro)
```

## ketaid: el servicio de audio

Es el que **reproduce**. Corre en segundo plano, normalmente como servicio
de usuario de systemd, y sigue sonando aunque no haya ninguna TUI abierta.

Se encarga de:

- **Reproduccion**: decodifica el audio (FLAC, MP3, AAC, ALAC, Vorbis, Opus,
  WAV) y lo manda a la salida. La salida se abre solo mientras suena y se
  reabre sola si el dispositivo falla.
- **Cola**: lo que suena, lo que sigue, aleatorio y repeticion.
- **Biblioteca**: escanea tus carpetas y guarda el indice en una base SQLite
  (`~/.local/share/ketai/library.db`), con los metadatos y el texto de
  busqueda ya preparado.
- **Playlists y favoritos**: viven en la misma base. Ademas escribe una copia
  `.m3u8` de cada playlist en tu carpeta de playlists.
- **Letras**: busca las letras de cada pista (archivos junto al audio o
  letras embebidas) y sus traducciones.
- **Salida de audio**: lista los dispositivos y mueve la reproduccion al
  elegido.
- **Caratulas**: las extrae del archivo o de la carpeta y se las pasa a los
  clientes.
- **MPRIS**: se publica en D-Bus para las teclas multimedia, `playerctl` y
  waybar.

Varias partes del daemon trabajan en paralelo (audio, biblioteca, playlists,
letras), cada una en su propio hilo con sus datos. Todo cambio pasa por un
solo lugar y se guarda en una transaccion: dos clientes que cambian algo al
mismo tiempo nunca se pisan, y todos se enteran de cada cambio.

Hay una sola instancia por sesion: si se abre otro `ketaid`, se queda el que
ya estaba.

## ketai: el cliente de terminal

Es lo que **ves**. No reproduce nada por su cuenta: le pide al daemon y
muestra lo que el daemon le cuenta.

- Dibuja las pantallas (Reproduciendo, Letras, Biblioteca, Buscar), los
  popups y el modo mini.
- Muestra las caratulas con el protocolo de graficos de kitty y calcula los
  colores del tema dinamico.
- Atiende el teclado y el mouse, y lee `config.toml` (temas y atajos), que
  recarga en vivo al guardarlo.

Como todo el estado esta en el daemon, podes abrir varias TUIs a la vez (una
grande y una mini en otro panel) y todas muestran lo mismo. Cerrar una TUI no
corta la musica.

## Como se hablan

Por sockets locales en `$XDG_RUNTIME_DIR/ketai/`:

| Socket | Para que |
|---|---|
| `ctl.sock` | Control: pedidos (reproducir, buscar, agregar a una playlist...) y avisos de cambios (cambio de pista, playlist modificada...). JSON, un mensaje por linea. |
| `viz.sock` | Los datos del espectro, en binario, para dibujarlo en vivo. |

Cualquier programa puede usar el mismo protocolo que la TUI. Esta
documentado en [../protocol.md](../protocol.md).

## Donde guarda las cosas

| Que | Donde |
|---|---|
| Configuracion (temas, atajos, carpeta de playlists) | `~/.config/ketai/config.toml` |
| Biblioteca, playlists y favoritos | `~/.local/share/ketai/library.db` |
| Copias de las playlists | `~/music/playlists/*.m3u8` (configurable) |
| Ultima cola y posicion | `~/.local/state/ketai/` |
| Caratulas extraidas (temporales) | `$XDG_RUNTIME_DIR/ketai/covers/` |
| Cache | `~/.cache/ketai/` |

## Por que solo Linux

ketai usa piezas propias de Linux:

- `/proc`, para encontrar y cerrar instancias viejas del daemon.
- systemd de usuario, para levantar el daemon solo cuando hace falta.
- PulseAudio o PipeWire (a traves de pipewire-pulse), para listar y cambiar
  la salida de audio.
- D-Bus, para MPRIS.

En otros sistemas esas piezas no existen o funcionan distinto, por eso por
ahora ketai solo funciona en Linux.
