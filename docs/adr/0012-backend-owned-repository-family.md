# Backend-owned repository family

Lady models linked worktrees as one Repository Family, but each worktree remains an independently opened repo session for checkout-specific operations. The backend owns family discovery through a `repository_family(repo)` contract that returns a stable family id, the Main Worktree, and the member Worktrees, because Git's linked-worktree identity is filesystem-backed and should not be reconstructed in the UI.

The `RepositoryFamilyId` is the canonical common Git directory path (`git rev-parse --path-format=absolute --git-common-dir`, canonicalized where possible). Per-worktree `RepoId`s remain valid operation handles; the family id is only the grouping key shared by all linked worktrees.

The left Worktrees section is the primary switcher inside the active Repository Family. The top repo tab strip shows one tab per Repository Family or standalone repository; selecting a worktree from the left list changes the selected Worktree inside the family tab.

The Worktrees section is always visible when a repository is open, even when the family currently has only the Main Worktree. Keeping it present makes worktree creation discoverable and avoids layout changes when the first linked worktree is added.

The persistent Worktrees section lives in the left sidebar under the repo header/current branch area, above normal repo navigation. It is a context switcher, not an advanced tool; any full management surface can remain a detail view, but switching and creation entry points belong in the sidebar.

Selecting a worktree preserves the current view when possible. If the view or selected object is invalid in the target worktree, Lady falls back to the closest sensible view: Changes for working-tree-specific views and Commits for history or ref views.

Repository-level settings split by scope. Account assignment, remotes/hosting identity, default base branch, AI enablement, and AI model override are Repository Family scoped. Git identity (`user.name` and `user.email`) follows Git local config, so Lady displays where it is actually set rather than flattening it into one artificial scope.

Persisted repository state uses `RepositoryFamilyId` as the root key from the start, including settings, recents, and restored tabs. Records that represent concrete checkouts store the Worktree path/member identity inside the family record, so Lady can restore a specific selected Worktree without making path the primary repository identity.

Recent repositories show one row per Repository Family, with the last selected Worktree as secondary context. Opening a recent family restores that last Worktree and immediately shows the full Worktrees sidebar.

Custom commands are Repository Family scoped by default because they describe repo workflows. They execute in the selected Worktree's directory, and a later per-worktree visibility override can be added if real workflows need it.

Stashes are Repository Family scoped because Git stores one stash list for the repository. Applying or popping a stash targets the selected Worktree, and Lady makes that target visible before mutation.

The first persistent switcher does not support drag-and-drop actions. Worktree selection, creation, removal, and management use clicks, menus, and explicit buttons; branch or commit drag targets can be reconsidered after the core model is established.

Bare repositories are out of scope for the persistent Worktrees UI because Lady's primary operations assume a working directory. If `repository_family(repo)` encounters a bare repository, it reports a clear unsupported state instead of rendering a partial switcher.

Missing or stale worktrees reported by Git remain visible in the sidebar, marked as missing/stale and not treated as normal selectable workspaces. Their actions route to prune or removal so Lady reflects Git's state instead of hiding it.

The backend provides a suggested display name for each Worktree so the sidebar, command palette, and dialogs use consistent labels. The Main Worktree is named `main`; linked worktrees use their branch name when available, otherwise the path basename, with suffixes to resolve collisions. Local user renaming can be added later without changing family discovery.

The sidebar displays the Main Worktree as `main`, with its actual branch and path available as secondary text or tooltip.

The command palette includes active-family entries for `Switch Worktree: {display name}` and `Create Worktree...`. It does not search worktrees across every recent Repository Family until a separate cross-family search design exists.

Background remote work such as fetch and hosting notifications runs once per Repository Family because remotes and refs are shared. Local refresh, dirty status, filesystem watching, and in-progress operation state remain per Worktree.

Filesystem watchers attach only to selected or opened Worktrees. Unopened siblings get lightweight state refresh when the family list is reloaded or polled, avoiding recursive watchers across every checkout in a family.

Opening a worktree from the sidebar focuses the existing Repository Family tab and changes its selected Worktree. If the family is not open, Lady opens one family tab and selects that Worktree; multiple top tabs for the same family are not created.

Closing a Repository Family tab closes that family project context, including selected Worktree and cached per-worktree UI state. Reopening the family restores the last selected Worktree from family-rooted persistence.

Lady remembers lightweight per-Worktree UI state such as active view, selected commits, and selected file only for the current app session. Persistence stores the family tab and last selected Worktree, not every transient selection for every Worktree.

The persistent sidebar switcher is a new compact component. The existing `WorktreesView` remains a management/detail surface, refactored as needed for full table/form workflows instead of being reused as the always-visible switcher.

The existing More-menu entry remains, renamed to `Manage Worktrees`, and opens the detailed management surface for pruning, removal, path edits, and diagnostics. The sidebar `Worktrees` section handles everyday switching and creation.

The sidebar `Worktrees` section shows a count badge using the number of worktrees Git reports, including stale or missing entries.

The family summary includes lightweight per-worktree state for scanning: selected/active status, branch or detached HEAD, dirty marker, and locked or missing status. Expensive details such as ahead/behind and conflict file counts are loaded only for the selected worktree or an explicit detail surface.

Creating a worktree defaults to a sibling directory near the Main Worktree, named from the proposed branch or worktree name. The path remains editable, but the primary flow is choosing the branch/worktree name and confirming, not starting with a filesystem picker.

The create flow supports both a new branch and an existing branch, but defaults to a new branch/worktree name. Existing-branch checkout is secondary and surfaces Git's refusal clearly when the branch is already checked out by another worktree.

Creating a worktree from a commit or detached HEAD remains supported as a secondary path from commit context menus. The sidebar create form stays branch-oriented and refreshes/opens the resulting worktree through the same Repository Family flow.

Normal branch checkout keeps its existing meaning: it changes the selected Worktree's branch. Worktree creation remains explicit through `Checkout as Worktree...` or `Create Worktree...`; if checkout fails because the branch is already checked out elsewhere, Lady can offer to switch to that Worktree.

Removing a worktree goes through `git worktree remove`, which deletes the worktree directory when Git considers it safe and refuses dirty or locked worktrees unless forced. Lady confirms before removal, with a "Don't ask again" preference for clean unlocked removals only; dirty, missing, locked, or force-style removals always require explicit confirmation. Stale administrative entries are handled by prune, not by hiding items from the UI.
