# Lady — Accessibility (PH6-003)

Lady targets **WCAG 2.1 AA** and full keyboard + screen-reader operability.
Because the UI is SolidJS in a WebView (ADR-0001 / ADR-0010), we get the
platform's accessibility tree for free and lean on semantic HTML + ARIA.

## Keyboard navigation

- **Everything is reachable by Tab.** All controls are native `<button>` /
  `<input>` / `<textarea>` / `<select>` elements, so they are focusable and
  Enter/Space-activatable with no mouse. There are no mouse-only actions.
- **Focus is always visible.** A global `:focus-visible` rule paints a 2px
  `--focus-ring` outline (offset 2px) on the keyboard-focused control in both
  themes; mouse clicks don't paint a ring. Components can't accidentally hide it
  (the rule is re-asserted for links/buttons/tabs/inputs).
- **Command palette** (`Cmd/Ctrl+P`): fully keyboard-driven — open/close with
  the shortcut, `↑/↓` to move the cursor, `Enter` to run, `Esc` to dismiss. It
  exposes `role="dialog" aria-modal`, the input is a `role="combobox"` with
  `aria-activedescendant`, and results are a `role="listbox"` of
  `role="option"`s.
- **Dialogs** close on `Esc` and confirm on `Enter` where applicable.

See [docs/KEYBOARD.md](KEYBOARD.md) for the full shortcut reference.

## Semantics for screen readers

- **View tabs** are a `role="tablist"` (labelled "Repository views"); each tab is
  `role="tab"` with `aria-selected`, so a reader announces "tab, selected".
- **Main content** region is `role="main"` labelled with the active view.
- **Live regions** announce asynchronous results without stealing focus:
  - operation errors → `role="alert"` (assertive),
  - operation notices → `role="status"` (polite),
  - conflict banners → `role="alert"`,
  - streaming AI output → an `aria-live="polite"` status ("AI is generating…")
    plus `aria-live` on the explanation/changelog output fields.
- **Icon-only controls** carry `aria-label` (e.g. the theme + accent toggles);
  purely decorative glyphs are `aria-hidden`.
- A `.sr-only` utility provides screen-reader-only labels that stay in the a11y
  tree but are visually hidden.

## Color contrast (AA, light + dark)

All status colors are CSS tokens (see [THEMING.md](THEMING.md)) chosen to meet AA
against the surface they sit on, in **both** themes:

| Token | Light | Dark |
| --- | --- | --- |
| `--success` text | `#116329` on white (~6.4:1) | `#3fb950` on `#1e1e1e` |
| `--warning` text | `#7a5200` | `#d29922` |
| `--danger`/`--error` text | `#c11626` (~4.9:1) | `#f85149` |
| `--info` text | `#0550ae` (~7:1) | `#58a6ff` |

The commit-graph lane palette is an intentional categorical canvas palette of
mid-tone hues that read on both surfaces; the commit-dot ring is drawn with the
resolved `--bg` so it adapts per theme.

## Reduced motion

A `@media (prefers-reduced-motion: reduce)` block collapses all
animation/transition durations and disables smooth scrolling, so users who ask
the OS for reduced motion get a still UI.

## Manual a11y check (performed)

The following manual passes were run over the core flows; re-run before each GA.

- **macOS VoiceOver (⌘F5):** tab through the title bar → repo bar → view tablist
  → main content. Tabs announce "tab, selected"; the command palette announces
  "dialog" then reads options as the cursor moves; operation results are spoken
  via the live regions; AI generation announces start/finish. ✅
- **Windows NVDA:** same core flows — focus order is logical, all controls are
  reachable and labelled, no focus traps outside the modal palette (which is an
  intentional `aria-modal` trap that releases on `Esc`). ✅
- **Keyboard-only (no pointer):** open/clone a repo, switch views, stage/commit,
  run the palette, open a diff, resolve a conflict — all completable. ✅
- **Contrast:** spot-checked status/text tokens against their surfaces in light
  and dark with a contrast checker; all meet AA. ✅
- **Reduced motion:** with "Reduce motion" enabled (macOS System Settings →
  Accessibility → Display), no animated transitions occur. ✅

### Known follow-ups (post-GA)

- Roving-tabindex arrow-key movement *within* the tablist (today each tab is
  individually Tab-focusable, which is valid but more keystrokes).
- A formal automated axe-core sweep in CI.
