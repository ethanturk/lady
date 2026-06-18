# Lady — Theming (PH6-004)

Lady is themed entirely through **CSS custom properties (tokens)** declared in
[ui/src/styles.css](../ui/src/styles.css). Every view reads tokens — no view
hard-codes a semantic color — so a theme change is a single attribute flip on
`<html>` and nothing in component code needs to change.

## Token set

Declared per theme under `:root[data-theme="light"]` and
`:root[data-theme="dark"]`:

- **Surfaces / structure:** `--bg`, `--surface`, `--surface-2`, `--border`
- **Text:** `--fg`, `--fg-muted`, `--on-accent`, `--on-light`
- **Accent:** `--accent`, `--accent-2`, `--selection`, `--focus-ring`
- **Status (AA-contrast on their surface):** `--success` / `--success-bg` /
  `--success-border`, `--warning` / `--warning-bg` / `--warning-border`,
  `--danger` / `--danger-bg` / `--danger-border` / `--danger-strong`,
  `--error`, `--info`
- **Code / diff:** `--code-bg`, `--code-fg`, `--diff-add-bg`, `--diff-del-bg`,
  `--diff-sel-bg`, and the `--hl-*` highlight.js token palette

## System / Dark / Light

The top-right toggle (`ThemeToggle.tsx`) cycles **System → Dark → Light**, writes
`data-theme` on `<html>`, and persists the choice in `localStorage`. **System**
follows the OS `prefers-color-scheme` live via a `matchMedia` listener.

## Extra theme — custom accent (proves the token system)

Next to the theme toggle is an **accent toggle** that cycles
`default → teal`. It sets `data-accent="teal"` on `<html>`; the stylesheet then
overrides only `--accent` and `--focus-ring`:

```css
:root[data-accent="teal"]            { --accent: #0d9488; --focus-ring: #0d9488; }
:root[data-theme="dark"][data-accent="teal"] { --accent: #2dd4bf; --focus-ring: #2dd4bf; }
```

Because tabs, links, the focus ring, palette highlights, and primary buttons all
read `--accent`, the entire app recolors with **zero component changes** — the
proof that theming is fully token-driven. The accent composes with light or dark
and persists in `localStorage`.

## Audit (PH6-004)

The Phase 1–5 views were audited for hard-coded colors: all semantic colors
(success/warning/danger/info/accent) were moved to tokens. The only remaining
literal colors are the commit-graph **lane palette** in `GraphView.tsx` — an
intentional categorical canvas palette of mid-tone hues that read on both light
and dark surfaces (the commit-dot ring uses the resolved `--bg` to adapt). Run
this to confirm no semantic literals have crept back in:

```sh
grep -rEn "#[0-9a-fA-F]{3,6}\b" ui/src/*.tsx | grep -v "var(--"
# → only GraphView lane palette + canvas dot
```

## Reduced motion

A `prefers-reduced-motion: reduce` media query collapses animations/transitions
(shared with accessibility — see [ACCESSIBILITY.md](ACCESSIBILITY.md)).
