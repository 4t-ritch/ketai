---
name: dev-install
description: "Trigger: install from repo, dev build, test locally, update ketaid, reinstall ketai, try changes in the real app. Build and install ketai and ketaid from this repo and restart the daemon."
license: MIT
metadata:
  author: "4t-ritch"
  version: "1.0"
---

## Activation Contract

Use when the user wants the system `ketai` (TUI) and `ketaid` (daemon) to run
the code in this working tree, to try new integrations in clients and services
before publishing to crates.io.

## Hard Rules

- Install into `~/.cargo/bin` with `cargo install --path crates/ketai`: the
  systemd unit runs `~/.cargo/bin/ketaid`, so no unit change is needed.
- Always pass `--locked --force` (same version as crates.io would be skipped).
- Never edit `~/.config/systemd/user/*` and never publish anything.
- A running `ketaid` keeps the old binary in memory: restart it after install.
- Report failures with their output; do not retry blindly.

## Decision Gates

| Situation | Action |
|---|---|
| Unit `ketaid.service` active | `systemctl --user restart ketaid` |
| Unit inactive or missing | Do not start it; say the TUI starts it on first run |
| User wants the crates.io version back | `cargo install ketai --locked --force`, then restart |
| User asks for optimized build | Prefix with `CARGO_PROFILE_RELEASE_LTO=true CARGO_PROFILE_RELEASE_CODEGEN_UNITS=1 CARGO_PROFILE_RELEASE_STRIP=true` |

## Execution Steps

1. `git -C <repo> status --short` and `git rev-parse --short HEAD`: note branch,
   commit and whether the tree is dirty (dirty changes are installed too).
2. `cargo install --path crates/ketai --locked --force` (show only the tail).
3. `systemctl --user is-active ketaid`; restart if active (see gates).
4. Verify: `ls -la ~/.cargo/bin/ketai ~/.cargo/bin/ketaid` (fresh mtime) and
   `systemctl --user status ketaid --no-pager | head -3` (active, started
   after the install). Never run `ketai` or `ketaid` to check: there is no
   `--version`; `ketai` opens the TUI and `ketaid` starts a second daemon.

## Output Contract

Return in a few lines: commit/branch installed (dirty or clean), install
result, daemon state after restart, and how to go back to the crates.io
version.
