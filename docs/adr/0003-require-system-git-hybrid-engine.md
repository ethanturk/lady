# Require system git; hybrid engine (gix + git2 + shell-out)

Lady requires a **system `git` install** for v1.0. The git engine is tiered behind one `GitEngine` trait:

- **`gix`** — reads: graph traversal, status, diff, blame, log / file history.
- **`git2`** (libgit2) — operations `gix` doesn't cover cleanly (some merge/rebase/cherry-pick/stash/index work).
- **shell-out to the user's `git`** — mutations and the long tail that must match git exactly: interactive rebase, hooks, LFS filters, GPG/SSH signing, credential helpers.

**Why:** exact git behavior with the least reimplementation risk, and Lady's audience (developers, same as Fork's) already have git installed.

**Trade-off / mitigation:** hard dependency on git's presence and version. Mitigated by a startup check + minimum-version guard with a clear error; **bundling a pinned git binary is deferred to Fast-follow** hardening (see [[CONTEXT]] → Fast-follow).
