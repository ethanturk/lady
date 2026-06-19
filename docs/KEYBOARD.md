# Lady — Keyboard reference

Lady is fully operable from the keyboard (PH6-003). Every control is a native,
Tab-focusable element with a visible focus ring, so you can reach and activate
anything without a mouse. This page lists the dedicated shortcuts; everything
else is reachable via **Tab** + **Enter/Space**, or through the command palette.

## Global

| Shortcut | Action |
| --- | --- |
| **Cmd/Ctrl + P** | Open / close the command palette |
| **Tab** / **Shift + Tab** | Move focus forward / back |
| **Enter** / **Space** | Activate the focused control |

## Command palette

| Key | Action |
| --- | --- |
| **↑ / ↓** | Move the selection cursor |
| **Enter** | Run the selected entry (action, branch, or file) |
| **Esc** | Close the palette |

The palette fuzzy-matches across **actions** (jump to a view), **branches** (jump
to Refs), and **files** (jump to Blame).

## Text inputs

| Key | Action |
| --- | --- |
| **Enter** | Submit the field — open/clone a repo, create a branch/tag, activate a license, connect a forge, track an LFS pattern, run a blame/history lookup, etc. |
| **Esc** | Dismiss the active dialog (e.g. the command palette, the interactive-rebase editor) |

## Notes

- Focus is always visible (a 2px accent outline) for keyboard users.
- The command palette is a modal dialog: focus is trapped inside it until you
  press **Esc** or choose an entry — standard `aria-modal` behavior.
- A roving-tabindex arrow-key pass *within* the view tablist is a tracked
  post-GA enhancement; today each tab is individually Tab-focusable.
