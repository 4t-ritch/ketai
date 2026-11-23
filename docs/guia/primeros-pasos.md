# Primeros pasos

De cero a escuchar musica en cinco minutos.

## 1. Lo que hace falta

- **Linux**, con PulseAudio o PipeWire (casi todas las distribuciones lo traen).
- **Una terminal con el protocolo de graficos de kitty**, para ver caratulas:
  kitty, Ghostty, WezTerm o Konsole. Sin eso ketai funciona igual con
  `KETAI_NO_IMAGES=1 ketai`, sin caratulas.
- **Una Nerd Font** configurada en la terminal, para los iconos.
- Para compilar: Rust, `cmake`, un compilador de C y las bibliotecas de
  desarrollo de PulseAudio y D-Bus (en Arch: `libpulse` y `dbus`; en
  Debian/Ubuntu: `libpulse-dev` y `libdbus-1-dev`).

## 2. Instalar

```sh
git clone https://github.com/4t-ritch/ketai.git
cd ketai
cargo build --release
install -Dm755 target/release/ketaid target/release/ketai -t ~/.local/bin/
```

`~/.local/bin` tiene que estar en tu `PATH`.

## 3. Dejar el servicio de audio listo

ketai tiene dos partes: el servicio de audio (`ketaid`) y el cliente
(`ketai`). Lo mas comodo es que systemd levante el servicio solo, la primera
vez que lo necesitas:

```sh
install -Dm644 dist/systemd/ketaid.{socket,service} -t ~/.config/systemd/user/
systemctl --user daemon-reload
systemctl --user enable --now ketaid.socket
```

Si preferis no usar systemd, corre `ketaid` en una terminal y deja esa
terminal abierta. Si abris `ketai` sin el servicio, la TUI te muestra que
falta y te ofrece iniciarlo.

## 4. Agregar tu musica

1. Abri la TUI: `ketai`.
2. Apreta `f` para abrir **Carpetas de la biblioteca**.
3. Apreta `a`, escribi la carpeta (por ejemplo `~/music`) y `Enter`.
4. Apreta `S` para escanear todo. Se ve el progreso; con miles de canciones
   tarda unos segundos.

La biblioteca queda guardada: no hace falta volver a escanear cada vez.
Volve a escanear (`S`) cuando agregues canciones nuevas.

## 5. Escuchar

- **Buscar**: `4` y escribi parte del titulo, del artista o del album. No
  importan los acentos ni las mayusculas. `Esc` pasa a los resultados y
  `Enter` reproduce.
- **Desde cualquier pantalla**: `Ctrl+P`, escribi y `Enter`.
- **Por artista, album o genero**: `3` (Biblioteca) y `h` / `l` para cambiar
  de vista.
- **Con el mouse**: clic para elegir, doble clic para reproducir.

Mientras suena: `espacio` pausa, `n` / `p` siguiente y anterior, `+` / `-`
volumen, `*` favorito y `P` agregar a una playlist.

## 6. Hacerlo tuyo

Crea `~/.config/ketai/config.toml`. El punto de partida mas simple es el
ejemplo completo: [examples/config.toml](../../examples/config.toml).

- Colores: [examples/theme/](../../examples/theme/README.md).
- Atajos: [examples/keys/](../../examples/keys/README.md).

Los cambios se ven al guardar el archivo.

## Si algo no anda

- **No se ven caratulas**: la terminal tiene que soportar el protocolo de
  graficos de kitty. Ver [../tui.md](../tui.md#requisitos).
- **No suena**: `d` muestra la salida de audio elegida; proba con otra.
- **La TUI dice que no hay servicio**: ver el paso 3 o
  [../servicio.md](../servicio.md).
- **Algo raro con la terminal o un multiplexor**: `KETAI_LOG=/tmp/ketai.log
  ketai` anota lo que pasa, para reportarlo en un issue.
