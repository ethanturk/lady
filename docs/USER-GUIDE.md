# Lady — User Guide

Lady is a fast, friendly cross-platform Git client (Fork-style UI + GitKraken-style
AI), built in Rust. This guide covers the daily-use flows. For keyboard
shortcuts see [KEYBOARD.md](KEYBOARD.md); for AI/privacy see
[AI-PRIVACY.md](AI-PRIVACY.md); for the MCP server see [MCP.md](MCP.md).

> Lady requires **system git** on your PATH (ADR-0003) and shells out to it for
> credential-backed network and signing operations.

## Getting started

1. **Open or clone a repository** from the repo bar at the top:
   - *Open* — pick an existing local repo folder.
   - *Clone* — paste a URL and choose a destination.
   - *Add* — register a repo you already have; recent repos appear as tabs.
2. The first launch starts a **30-day trial** (ADR-0007). Enter a license key any
   time under **Settings → License**. (Offline, signed-key verification.)

## The core flows

| Task | Where |
| --- | --- |
| See working-tree changes, stage/unstage, partial (hunk/line) staging, discard | **Changes** tab |
| Commit / amend (with GPG/SSH signing if your git is configured) | **Changes** tab |
| Browse the commit **graph**, inspect a commit's diff, cherry-pick / revert / start interactive rebase | **Commits** tab |
| Branches & tags — create / delete / checkout, drag-&-drop merge/rebase | **Refs** tab |
| Fetch / pull / push with ahead-behind counts | the **sync bar** |
| Stash save / pop / apply / drop | **Changes** tab |
| Resolve merge/rebase conflicts in a 3-pane resolver | **Conflicts** tab (auto-surfaces) |
| Blame & file history | **Blame** / **History** tabs |
| Worktrees, reflog (restore lost commits), bisect | their named tabs |
| Custom commands, external diff/merge tools | **Commands** tab |
| LFS, git-flow, submodules | their named tabs |
| AI: commit messages, Commit Composer, explain, conflict help, PR text, changelog | **✨ AI** tab + ✨ buttons |
| Forge auth, create remote repo, app updates | **Settings** tab |
| GitHub notifications inbox | **Notifications** tab |

## Command palette

Press **Cmd/Ctrl + P** to open the command palette. Type to fuzzy-match an
action, branch, or file; **↑/↓** to move, **Enter** to run, **Esc** to dismiss.
It's the fastest way to jump between views, branches, and files.

## Keeping Lady up to date

**Settings → Updates → Check for updates.** If a newer version is available,
Lady shows the version + notes; click **Download & install** and Lady verifies
the signature, applies the update, and restarts. Updates are never silent — see
[PACKAGING.md](PACKAGING.md) for the signing/trust details.

## Accessibility & themes

Lady is keyboard-navigable and screen-reader-labelled, meets WCAG AA contrast in
light and dark, and honors *reduce motion* — see [ACCESSIBILITY.md](ACCESSIBILITY.md).
Toggle **System / Dark / Light** and the **accent color** from the top-right; see
[THEMING.md](THEMING.md).
