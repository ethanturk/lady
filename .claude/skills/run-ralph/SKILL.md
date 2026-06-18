---
name: run-ralph
description: Run the Ralph autonomous build loop for the Lady repo against scripts/ralph/prd.json using Claude Code. Use when the user says "run ralph", "start the ralph loop", "kick off ralph", "build the next stories", or invokes /run-ralph. Optional argument = max iterations (integer).
disable-model-invocation: true
user-invocable: true
---

# Run Ralph

Invoke this repo's `scripts/ralph/ralph.sh` with the correct arguments to autonomously build the stories in `scripts/ralph/prd.json`.

> ⚠️ This loop runs `claude --dangerously-skip-permissions` and **auto-commits** to a branch derived from the PRD's `branchName`. Never run it on an unrelated branch or a dirty tree you care about. It is manual-only by design.

## Necessary arguments (always)

- **`--tool claude`** — this repo uses Claude Code, never Amp. Always pass this.
- **max iterations** (positional integer) — how many story-iterations to run.

So the command is always:

```bash
./scripts/ralph/ralph.sh --tool claude <N>
```

Run it from the **repo root** (the spawned `claude` instances operate on the current working directory).

## Steps

1. **Preconditions** — verify, and stop with a clear message if any fail:
   - `scripts/ralph/prd.json` exists and is valid: `jq -e . scripts/ralph/prd.json`
   - `jq` and `claude` are on PATH (`command -v jq`, `command -v claude`)
   - Working tree is clean enough to auto-commit: `git status --short` (warn if unexpected changes are staged/unstaged; the loop will commit them).
2. **Pick the branch** — read it so the user knows where commits land:
   `jq -r '.branchName' scripts/ralph/prd.json`
3. **Decide `N` (max iterations):**
   - If the user passed an integer argument, use it.
   - Otherwise default to the number of incomplete stories:
     `jq '[.userStories[] | select(.passes==false)] | length' scripts/ralph/prd.json`
   - If that count is 0, tell the user all stories already pass (nothing to do) and stop.
   - Note to the user: a story that fails its quality checks is retried on the next iteration, so it may consume more than one — bump `N` if it stalls.
4. **Run the loop** — from the repo root:
   ```bash
   ./scripts/ralph/ralph.sh --tool claude <N>
   ```
   - For `N > 2`, run it with Bash `run_in_background: true` so the session isn't blocked, then monitor.
   - The loop stops early and prints success when it sees `<promise>COMPLETE</promise>` (all stories pass); otherwise it exits after `N` iterations.
5. **Report** — after it finishes (or per check-in while backgrounded):
   - Which stories flipped to `passes: true`:
     `jq -r '.userStories[] | select(.passes==true) | "\(.id) \(.title)"' scripts/ralph/prd.json`
   - Remaining incomplete count (same query with `==false`).
   - Tail of `scripts/ralph/progress.txt` for the latest learnings.
   - Recent commits on the branch: `git log --oneline -n <N>`.

## Notes

- One PRD = one branch. To build a later phase, a **new** `scripts/ralph/prd.json` with a different `branchName` is created first; running this skill then archives the previous run automatically (handled by `ralph.sh`).
- Do **not** pass `--tool amp`. This repo is Claude Code only.
- Runtime files (`progress.txt`, `.last-branch`, `archive/`) are git-ignored; `prd.json` is tracked.
