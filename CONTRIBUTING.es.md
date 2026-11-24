[English](CONTRIBUTING.md) | **Espanol**

# Participar en ketai

Gracias por el interes. ketai es un proyecto personal, abierto para usar.

## Issues si, pull requests no

- **Todo pasa por issues**: errores, ideas, preguntas y propuestas de
  cambios. Abri uno en <https://github.com/4t-ritch/ketai/issues>, en espanol o en
  ingles.
- **El repositorio no acepta pull requests.** Si tenes un cambio en mente,
  describilo en un issue (que problema resuelve y como lo imaginas) y lo
  conversamos ahi.

## Para reportar un error

Incluye lo que ayude a reproducirlo:

- Distribucion de Linux y terminal (kitty, Ghostty, WezTerm...), y si usas un
  multiplexor (herdr, tmux).
- Version de ketai (`git log -1 --oneline` si lo compilaste).
- Que hiciste, que esperabas y que paso.
- Si tiene que ver con la terminal o las caratulas, el registro de la TUI:
  `KETAI_LOG=/tmp/ketai.log ketai`.
- Si tiene que ver con el audio, lo que muestra `ketaid` en su terminal (o
  `journalctl --user -u ketaid`).

## Plataforma

ketai funciona solo en Linux. Ver
[docs/es/guide/architecture.md](docs/es/guide/architecture.md#por-que-solo-linux).
