**English** | [Espanol](CONTRIBUTING.es.md)

# Contributing to ketai

Thanks for your interest. ketai is a personal project, open for anyone to use.

## Issues yes, pull requests no

- **Everything goes through issues**: bugs, ideas, questions and change
  proposals. Open one at <https://github.com/4t-ritch/ketai/issues>. English
  or Spanish, whichever you prefer.
- **The repository does not accept pull requests.** If you have a change in
  mind, describe it in an issue (what problem it solves and how you picture
  it) and we will talk it over there.

## Reporting a bug

Include whatever helps to reproduce it:

- Linux distribution and terminal (kitty, Ghostty, WezTerm...), and whether
  you use a multiplexer (herdr, tmux).
- ketai version (`git log -1 --oneline` if you built it).
- What you did, what you expected and what happened.
- If it has to do with the terminal or the covers, the TUI log:
  `KETAI_LOG=/tmp/ketai.log ketai`.
- If it has to do with audio, what `ketaid` shows in its terminal (or
  `journalctl --user -u ketaid`).

## Platform

ketai works on Linux only. See
[docs/guide/architecture.md](docs/guide/architecture.md#why-linux-only).
