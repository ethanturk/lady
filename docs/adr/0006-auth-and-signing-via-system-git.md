# Auth & signing via system git config

Lady reuses the user's existing git configuration for transport auth and commit signing, rather than reimplementing it:

- **HTTPS:** `git credential fill / approve / reject` (honors the user's `credential.helper`, osxkeychain, etc.).
- **SSH:** the user's `ssh-agent` / configured keys.
- **Signing:** git's `gpg.program` / `gpg.ssh` config + the user's keys and agent; Lady surfaces passphrase prompts via its credential UI.
- **`keyring` (OS keychain)** stores **only hosting-API tokens** (PR creation, notifications) — not git-transport credentials.

**Why:** git is already required ([[require-system-git]]), so leaning on its credential + signing setup is the least code and "just works" with whatever the user configured.

**Consequence (sharpens the tier boundary):** authenticated **network** ops — fetch / pull / push — route through the **shell-out tier** in v1 so they reuse git's credentials; `gix`/`git2` stay on local reads. Native authenticated transport is not pursued for v1.

**Carve-out — per-repo git identity:** Lady otherwise treats `.git/config` as read-only, but the per-repo settings (Plan 2) let a user set this repo's `user.name` / `user.email`. Those write the repo's **local** config via `git config --local` (an empty value unsets the key), matching real git semantics so the identity is honored even outside Lady. This is the same shell-out write path already used by git-flow init. All other overridable settings (signing default, merge fast-forward, base branch, AI model) live in Lady's own `settings.toml`, never in git config.
