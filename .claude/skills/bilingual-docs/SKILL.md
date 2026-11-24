---
name: bilingual-docs
description: Use whenever you create, edit, move or delete documentation in the ketai repo (README, CONTRIBUTING, docs/, examples/ guides, example config comments, issue templates). English is the primary documentation and every document has a Spanish copy kept in sync in the same change.
---

# Bilingual documentation (English primary, Spanish copy)

All public documentation in ketai exists in two languages. **English is the
primary version** that everyone sees; **Spanish is a full copy** so Spanish
speakers can read everything. Both always say the same thing.

## Where each version lives

The Spanish copy mirrors the English path:

| English (primary) | Spanish (copy) |
|---|---|
| `README.md` | `README.es.md` |
| `CONTRIBUTING.md` | `CONTRIBUTING.es.md` |
| `docs/<name>.md` | `docs/es/<name>.md` |
| `docs/guide/<name>.md` | `docs/es/guide/<name>.md` |
| `examples/README.md`, `examples/*/README.md` | `README.es.md` in the same folder |

- File names are the same (English) in both versions, so each pair is
  obvious. Never give the Spanish copy a Spanish file name.
- Images and other assets live once, in `docs/assets/`, and both versions
  point to them.
- Not documentation, so not mirrored: `odd/` (internal task logs), code
  comments, `CLAUDE.md`, this skill.

## Rules for every change

1. **Change both versions in the same commit.** Adding, rewording, moving or
   deleting a section in one language means doing the same in the other. A
   pull of either version must never describe different behavior.
2. **Write the English first**, then the Spanish from it. Translate the
   meaning, not word by word; keep the same headings, order, tables, code
   blocks and examples, so a reader can switch languages and find the same
   place.
3. **Language switch line** at the top of every document:
   - English: `**English** | [Espanol](<path to the Spanish copy>)`
   - Spanish: `[English](<path to the English version>) | **Espanol**`
4. **Links stay in their language**: English documents link to English
   documents; Spanish documents link to the Spanish copies
   (`docs/es/...`, `README.es.md`). In `README.md`, wherever a document is
   cited, also give its Spanish link (for example
   `[docs/config.md](docs/config.md) ([Spanish](docs/es/config.md))`).
5. **Spanish is ASCII-only** (project rule): no accents, no `n` with tilde,
   no inverted punctuation. Write `cancion`, `reproduccion`, `ano`. English
   is plain ASCII too.
6. **Do not translate contract names**: methods, events, JSON fields, config
   keys, file names, commands and code stay exactly as they are in both
   versions.
7. **The interface is in Spanish for now**: English documents name each
   screen in English and give the on-screen Spanish label in italics the
   first time, for example Now Playing (*Reproduciendo*).
8. **Single-copy files are in English**: comments inside example `.toml`
   files, `dist/config.toml` and templates under `.github/` are written once,
   in English (issue templates may add a Spanish hint). The Spanish README
   next to them explains them in Spanish.

## Checklist before committing

- [ ] Every changed document has its counterpart changed too.
- [ ] Both versions have the language switch line.
- [ ] No non-ASCII characters in the Spanish copies:
      `LC_ALL=C grep -rnP '[^\x00-\x7F]' README.es.md CONTRIBUTING.es.md docs/es examples --include='*.es.md'`
- [ ] Every relative link resolves (run the link check below).
- [ ] `cargo test -p ketai-tui --test examples` still passes if an example
      `.toml` changed.

Link check (from the repo root):

```sh
python3 - <<'EOF'
import re, pathlib
docs = [p for p in pathlib.Path(".").rglob("*.md")
        if not any(part in {"target", "odd", ".git", "samples", ".claude"} for part in p.parts)]
bad = []
for f in docs:
    for m in re.findall(r'\]\(([^)]+)\)|src="([^"]+)"|href="([^"]+)"', f.read_text()):
        link = next(x for x in m if x)
        if link.startswith(("http", "mailto:", "#")):
            continue
        if not (f.parent / link.split("#")[0]).resolve().exists():
            bad.append(f"{f}: {link}")
print("\n".join(bad) or "all links ok")
EOF
```
