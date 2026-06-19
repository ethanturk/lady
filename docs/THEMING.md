# Lady — Theming (PH6-004)

Lady is themed entirely through **CSS custom properties (tokens)** declared in
[ui/src/styles.css](../ui/src/styles.css). Every view reads tokens — no view
hard-codes a semantic color — so a theme change is a single attribute flip on
`<html>` and nothing in component code needs to change.

## Token set

The palette follows the **Claude Design Fork-style UI** spec
([design/README.md](../design/README.md)). Tokens are declared per theme under
`:root[data-theme="light"]` and `:root[data-theme="dark"]`:

- **Design surfaces:** `--bg` (Main), `--panel` (sidebar / detail pane /
  composer), `--toolbar`, `--tabs`, `--tabact`, `--sub` (sub-headers / diff
  header), `--input`, `--pill` (repo pill / menus), `--btn`, `--btnh`
- **Design text:** `--tx` (primary), `--tx2`, `--tx3` (muted), `--tx4` (faint)
- **Design borders / hover:** `--bd`, `--bds` (stronger), `--hov`
- **Accent:** `--accent` (`oklch(0.65 0.15 255)`, themeable), `--accent-2`,
  `--selection` (`color-mix(--accent 15%)`), `--focus-ring`, `--on-accent`
  (white, on dark status fills), `--on-accent-strong` (`#0c0d10`, on accent fills)
- **Status (AA-contrast on their surface):** `--success` / `--success-bg` /
  `--success-border`, `--warning` / `--warning-bg` / `--warning-border`,
  `--danger` / `--danger-bg` / `--danger-border` / `--danger-strong`,
  `--error`, `--info`
- **Code / diff:** `--code-bg`, `--code-fg`, `--diff-add-bg`, `--diff-del-bg`,
  `--diff-sel-bg`, `--difftx`, `--diffgut`, `--lineno`, `--diff-add-tx`,
  `--diff-del-tx`, `--hunk-tx`, and the `--hl-*` highlight.js token palette
- **Fixed (non-themed) under bare `:root`:** status badges `--badge-m/-a/-d/-r`
  + `--on-badge`, graph `--lane-main` / `--lane-branch`, diff signs
  `--diff-add-sign` / `--diff-del-sign`, `--hunk-bg`, and the magenta branch
  chip `--chip-branch-tx/-bg/-bd`

### Legacy aliases

The pre-redesign token names are kept as **aliases onto the design palette**, so
the older views recolor automatically without rewriting their inline styles:

```css
--surface:   var(--panel);   --surface-2: var(--sub);
--border:    var(--bd);      --fg:        var(--tx);   --fg-muted: var(--tx3);
```

New shell/view components (`Toolbar`, `Sidebar`, `AllCommitsView`, `DiffView`,
`ChangesView`, …) consume the design token names directly.

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

Semantic colors (success/warning/danger/info/accent/surfaces/text) are all
token-driven. The intentional literal colors that remain are:

- the commit-graph **lane palette** (`LANE_COLORS`) and the dark node-initial
  fill (`#0c0d10`) in `GraphView.tsx` — a categorical canvas palette drawn on
  `<canvas>`, which can't reference CSS vars;
- the **author avatar** colors (`avatar.ts`), computed `hsl()` from the author
  name so the same author reads the same on both themes;
- the **diff gutter tints** (`ADD_GUT` / `DEL_GUT`) in `DiffView.tsx`, which are
  the design's fixed add/remove gutter rgba values.

Run this to confirm no *semantic* literals have crept into the views:

```sh
grep -rEn "#[0-9a-fA-F]{3,6}\b" ui/src/*.tsx | grep -v "var(--"
# → only the GraphView canvas palette, avatar hsl(), and DiffView gutter tints
```

## Reduced motion

A `prefers-reduced-motion: reduce` media query collapses animations/transitions
(shared with accessibility — see [ACCESSIBILITY.md](ACCESSIBILITY.md)).
