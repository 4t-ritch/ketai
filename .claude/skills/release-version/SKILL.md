---
name: release-version
description: "Trigger: push main, merge dev into main, release, bump version, version update. Bump the ketai version (major, minor or patch) before every push to main."
license: MIT
metadata:
  author: "4t-ritch"
  version: "1.0"
---

## Activation Contract

Use every time `main` is about to be pushed (usually after merging `dev`
into `main`). The version bump happens BEFORE that push, never after.

## Hard Rules

- The version lives in the root `Cargo.toml`: `[workspace.package] version`
  and the `version` of the three path dependencies (`ketai-core`,
  `ketai-protocol`, `ketai-tui`). Change all four together, then
  `cargo update -w`.
- The bump is decided from the commits that `main` does not have yet:
  `git log --no-merges <last tag>..dev` (last tag: `git describe --tags --abbrev=0`).
- One bump per push to `main`, the highest that applies. Reset lower parts
  (`0.2.3` + minor = `0.3.0`).
- Commit on `dev` as `chore(release): bump version to X.Y.Z`, then merge into
  `main`, then push. Tag `vX.Y.Z` (annotated) on that commit and push the tag.
- Publishing to crates.io and GitHub releases are separate decisions: offer
  them, do not run them unasked.
- If the type is unclear (for example a large `feat` that may be major), ask
  the user once before bumping.

## Decision Gates

| Commits since the last tag | Bump |
|---|---|
| Large change: a roadmap major update, a breaking change (`!` or `BREAKING CHANGE`), protocol or config incompatibility | **major** (`X+1.0.0`) |
| New features that do not break anything (`feat`) | **minor** (`X.Y+1.0`) |
| Only bug fixes (`fix`, `perf`) | **patch** (`X.Y.Z+1`) |
| Only `docs`, `chore`, `test`, `ci`, `style`, `refactor` | No bump: tell the user and push without it |

## Execution Steps

1. On `dev`, clean tree. Read the last tag and list the commits since it.
2. Classify with the table; state the chosen bump and the commits that decide it.
3. Edit the four versions, `cargo update -w`, run `cargo test -q` (stop on failures).
4. Commit, push `dev`, merge `dev` into `main` (`--no-ff`), push `main`.
5. `git tag -a vX.Y.Z -m "ketai X.Y.Z"` on the bump commit; push the tag.
6. Go back to `dev`.

## Output Contract

Report: previous and new version, bump type and the commits that justify it,
test result, pushed branches and tag, and offer `cargo publish --workspace`
and a GitHub release.
