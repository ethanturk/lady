# Lady

A fast, friendly cross-platform Git client (Fork clone + GitKraken-style AI), built in Rust. This glossary pins terms that are specific to Lady's plan — not general Git or programming vocabulary.

## Language

**Ship fast**:
Lady's north star — maximize *build* velocity to Core Parity (via Tauri + the autonomous Ralph loop). It does NOT mean releasing an early partial product.
_Avoid_: MVP-first, early release, ship early

**Core Parity**:
The v1.0 release bar. Fork's daily-use surface: commit graph, diff viewer, partial (line/hunk) staging, commit/amend, branches/tags, fetch/pull/push, merge, cherry-pick/revert, stash, interactive rebase, 3-pane conflict resolver, blame, file history, worktrees, reflog, GPG+SSH signing, command palette. Excludes the niche long-tail (see Fast-follow).
_Avoid_: full parity, MVP

**Fast-follow**:
Niche Fork features deliberately cut from v1.0 and shipped as patches immediately after release: Git LFS, git-flow, submodule edge cases, and PR creation for non-GitHub forges. Not "someday" — committed, just post-v1.0.
_Avoid_: backlog, later, phase 4
