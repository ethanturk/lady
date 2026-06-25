# Plan 008: Spike semantic commit search

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the next
> step. This is a design/spike plan, not a production feature build. If anything
> in the "STOP conditions" section occurs, stop and report - do not improvise.
> When done, update the status row for this plan in `plans/README.md` unless a
> reviewer told you they maintain the index.
>
> **Drift check (run first)**: `git diff --stat 9666d42..HEAD -- docs/RELEASE-CHECKLIST.md CHANGELOG.md crates/lady-mcp/src/lib.rs crates/lady-ai/src/context.rs docs/AI-PRIVACY.md`
> If any in-scope file changed since this plan was written, compare the
> "Current state" excerpts against the live code before proceeding; on a
> mismatch, treat it as a STOP condition.

## Status

- **Priority**: P3
- **Effort**: M
- **Risk**: LOW
- **Depends on**: none
- **Category**: direction
- **Planned at**: commit `9666d42`, 2026-06-24

## Why This Matters

Semantic commit search is explicitly deferred, but the architecture already has
pieces that make it plausible: commit search in MCP, AI provider abstraction,
token budgeting, and privacy rules. The risk is building too much too soon,
especially because embeddings introduce storage, privacy, provider, and
indexing tradeoffs. This plan produces a decision-ready spike document and, at
most, a tiny throwaway prototype behind tests if needed.

## Current State

Relevant files:

- `docs/RELEASE-CHECKLIST.md` - marks semantic search deferred.
- `CHANGELOG.md` - marks semantic search deferred in Phase 5 notes.
- `crates/lady-mcp/src/lib.rs` - current literal commit summary search.
- `crates/lady-ai/src/context.rs` - token budgeting and redaction constraints.
- `docs/AI-PRIVACY.md` - privacy posture.

Excerpts:

```text
docs/RELEASE-CHECKLIST.md:87
| Semantic commit search (stretch) | deferred | PH5-012 |
```

```text
docs/RELEASE-CHECKLIST.md:94-97
PH5-012 (semantic commit search) is explicitly deferred ...
A literal `search_commits` (message grep) ships via the MCP server and the
engine; embedding-based semantic ranking is a Fast-follow/Phase 6 candidate.
```

```text
CHANGELOG.md:54-56
- Read-only MCP server (`lady-mcp`) exposing repo context...
- _Deferred:_ semantic commit search (optional stretch).
```

```rust
crates/lady-mcp/src/lib.rs:153-163
#[tool(description = "Find commits whose summary contains a query (case-insensitive).")]
async fn search_commits(...) -> Result<Json<Vec<CommitRecord>>, ErrorData> {
    let commits = self.engine.search_commits(&self.repo, &query, limit).map_err(err)?;
    Ok(Json(commits.iter().map(to_record).collect()))
}
```

```rust
crates/lady-ai/src/context.rs:8-11
Redaction (ADR-0009) is best-effort, not a guarantee. `redact` strips obvious
credentials ... before any remote send; it reduces accidental leakage but does
not make sending code safe by itself.
```

```text
docs/AI-PRIVACY.md:13-17
AI is off for every repo until you enable it.
Before Lady ever calls a remote provider it requires a recorded, per-provider
consent. Until then, remote calls are blocked.
```

## Commands You Will Need

| Purpose | Command | Expected on success |
|---------|---------|---------------------|
| Rust tests | `cargo test` | all tests pass |
| Docs grep | `rg -n "semantic commit search|semantic search|embedding" docs crates/lady-mcp crates/lady-ai` | new spike doc appears; no accidental production claims |
| Git status | `git status --short` | only docs/spike files and `plans/README.md` unless a tiny prototype was explicitly approved |

## Scope

**In scope**:

- Create `docs/spikes/semantic-commit-search.md` (or
  `docs/semantic-commit-search-spike.md` if `docs/spikes` does not exist).
- Optional: a small pure Rust prototype or test-only module if needed to compare
  ranking approaches, but only after the doc defines why it is needed.

**Out of scope**:

- Shipping a UI.
- Adding a vector database dependency.
- Sending repo contents to a remote embeddings provider.
- Persisting embeddings in user config.
- Changing MCP tool behavior except documenting current literal search.

## Git Workflow

- Branch: `advisor/008-semantic-search-spike`
- Commit message: `docs: spike semantic commit search`
- Do not push or open a PR unless the operator instructed it.

## Steps

### Step 1: Create the spike document skeleton

Create `docs/spikes/semantic-commit-search.md` with sections:

- Problem statement.
- Current literal search behavior.
- User stories.
- Privacy constraints.
- Candidate architectures.
- Storage/index options.
- Evaluation plan.
- Recommendation.
- Open questions.

**Verify**: `test -f docs/spikes/semantic-commit-search.md || test -f docs/semantic-commit-search-spike.md` -> file exists.

### Step 2: Document candidate architectures

Compare at least three options:

- Local-only embeddings through Ollama or another local provider.
- BYOK remote embeddings with explicit per-provider consent and redaction.
- No embeddings: improve literal search with commit-message + path + diff-term
  ranking.

For each option, include privacy impact, implementation effort, expected search
quality, offline behavior, storage needs, and failure modes.

**Verify**: `rg -n "Local-only|BYOK remote|No embeddings|privacy|storage" docs/spikes docs` -> the spike covers all required dimensions.

### Step 3: Define an evaluation harness before implementation

Specify a deterministic evaluation fixture:

- A small synthetic repo with commits whose summaries and diffs use different
  vocabulary for the same concept.
- Queries such as "retry logic", "authentication failure", and "settings
  persistence".
- Expected top results.

Do not implement the full harness unless the operator explicitly expands scope.

**Verify**: `rg -n "Evaluation|fixture|expected top" docs/spikes docs` -> the
spike contains a concrete evaluation plan.

### Step 4: Write the recommendation

Make a recommendation that is honest about tradeoffs. A likely recommendation is:
start with an offline/local-only prototype and keep current literal MCP search
as the stable production path until ranking quality and privacy UX are proven.

**Verify**: `rg -n "Recommendation|Open questions" docs/spikes docs` -> the
spike has a recommendation and explicit open questions.

### Step 5: Run verification

**Verify**: `cargo test` -> all tests pass.

**Verify**: `git status --short` -> changes are docs-only plus
`plans/README.md`, unless a tiny prototype was explicitly approved in Step 3.

## Test Plan

- No production tests are required for a docs-only spike.
- If a tiny prototype is added, it must be pure/offline and covered by
  `cargo test -p <crate> <test_name>`.
- The spike must define the tests a future build plan would add.

## Done Criteria

- [ ] A semantic search spike document exists under `docs/`.
- [ ] It cites current literal MCP search and deferred roadmap status.
- [ ] It compares local-only, BYOK remote, and non-embedding search options.
- [ ] It defines privacy constraints from `docs/AI-PRIVACY.md`.
- [ ] It defines an evaluation fixture and expected result shape.
- [ ] It recommends the next action and lists open questions.
- [ ] `cargo test` exits 0.
- [ ] `plans/README.md` status row updated.

## STOP Conditions

Stop and report back if:

- The operator expects this plan to ship semantic search rather than spike it.
- A useful recommendation requires current provider docs or pricing that you
  cannot verify in the local repo.
- You need to add a new runtime dependency to prototype ranking.
- You discover a privacy constraint that contradicts remote embeddings entirely
  and needs an ADR-level decision.

## Maintenance Notes

The follow-up build plan should be written only after the spike is reviewed.
Do not let "semantic search" silently become a remote-provider default; it must
inherit Lady's explicit opt-in and local-first AI posture.

