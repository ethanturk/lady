# Semantic Commit Search Spike

## Problem Statement

Lady currently ships literal commit search through `lady-mcp`: `search_commits`
returns commits whose summaries contain a case-insensitive query. PH5-012
semantic commit search is explicitly deferred in the release checklist and
changelog. The product question is whether Lady should add meaning-based search
for queries such as "retry logic" or "settings persistence" when the matching
commit may use different vocabulary.

The main risk is not the ranking algorithm. It is accidentally turning commit
messages, paths, and diffs into a remote data flow without the same local-first,
BYOK, explicit-consent posture as the rest of Lady's AI features.

## Current Literal Search Behavior

- `crates/lady-mcp/src/lib.rs` exposes `search_commits` as a read-only MCP tool.
- `crates/lady-git/src/lib.rs` implements it as a newest-first summary grep.
- The current search does not inspect paths or diffs.
- It is deterministic, offline, fast enough for the current MCP use case, and
  has no additional storage or privacy surface.

## User Stories

- As a developer, I want to find the commit that changed an idea even when I do
  not remember the exact commit wording.
- As a reviewer, I want to ask "where did auth failure handling change" and see
  commits that touched related paths, messages, and diff terms.
- As an AI-assistant user, I want MCP search to stay read-only and predictable.
- As a privacy-conscious user, I want semantic indexing to be local by default
  and never silently send repository history to a remote provider.

## Privacy Constraints

Lady's AI posture from `docs/AI-PRIVACY.md` applies:

- AI is off for every repo until enabled.
- Remote providers require recorded per-provider consent before use.
- BYOK keys live in the OS keychain and are never written to settings.
- Redaction is best-effort, not a guarantee; remote sends must be treated as
  disclosure to that provider.
- Local Ollama is the preferred private path because repo content does not leave
  the machine.

Semantic search also needs two extra constraints:

- Embeddings are derived repository content. Treat them as sensitive even when
  they are not directly human-readable.
- Do not persist embeddings in `settings.toml`; settings are for preferences,
  not repo-derived index data.

## Candidate Architectures

| Option | Privacy | Effort | Quality | Offline | Storage | Failure modes |
| --- | --- | --- | --- | --- | --- | --- |
| Local-only embeddings | Best fit. Content stays local through Ollama or another local embedding model. | Medium. Need embedding model selection, indexing, invalidation, and UI/MCP integration. | Good if model quality is acceptable; varies by local model. | Yes, once model is installed. | Per-repo cache under app data or repo-local cache; never settings. | Ollama/model missing, slow first index, poor model quality, stale cache. |
| BYOK remote embeddings | Requires explicit provider consent and clear disclosure that commit history/diffs are sent remotely. | Medium-high. Need remote embedding client, consent UX, redaction, rate/error handling. | Likely strong, provider-dependent. | No. | Same local cache, plus provider/model metadata for invalidation. | Privacy surprise, quota/rate failures, cost, provider-specific dimensions. |
| No embeddings | Keeps current privacy posture and offline behavior. Improve ranking with summaries, paths, filenames, touched symbols, and diff-term BM25-style scoring. | Low-medium. Can build on `lady-git` without new runtime dependencies. | Better than summary grep; weaker for true synonym matching. | Yes. | Optional lightweight per-repo term cache; can also compute on demand. | Still misses vocabulary gaps; large repos may need indexing. |

## Storage And Index Options

- **On demand scan**: simplest and safest first step. Search recent commits,
  summaries, paths, and bounded diff text without persistent index. Good for an
  MVP or evaluation harness, but may be slow on large histories.
- **Per-repo app-data cache**: store under Lady's app data directory keyed by
  repository family id and HEAD/index metadata. This avoids writing derived data
  into the repository or user settings.
- **Repo-local cache**: possible, but it dirties or adds files near the user's
  repo and can conflict with expectations. Avoid unless users explicitly opt in.
- **SQLite**: good first persistent store if needed. It is inspectable,
  transactional, and can store term statistics or vectors without a separate
  vector database.
- **Vector database or HNSW dependency**: defer. It raises dependency, license,
  maintenance, and migration cost before search quality is proven.

## Evaluation Plan

Create a deterministic synthetic repo fixture with 12-20 commits. Each commit
should include a summary, touched paths, and a small diff. Use deliberately
different vocabulary for related concepts.

Example fixture themes:

- Retry/backoff behavior in network commands.
- Authentication failures and credential-helper handling.
- Settings persistence and race prevention.
- Diff rendering and HTML escaping.
- Worktree repository-family behavior.

Queries and expected top results:

| Query | Expected top result shape |
| --- | --- |
| `retry logic` | Commits touching retry/backoff code even if summary says "transient network errors". |
| `authentication failure` | Commits changing credential-helper errors, token handling, or auth messaging. |
| `settings persistence` | Commits that serialize settings writes or preserve unrelated settings sections. |
| `script injection` | Commits around diff escaping, `innerHTML`, or CSP hardening. |
| `worktree family` | Commits around repository-family identity and worktree switching. |

Measure:

- Top-1 and top-3 hit rate against the expected commit ids.
- Latency for 100, 1,000, and 10,000 commits.
- Indexed storage size, if an index is used.
- Whether each option works fully offline.
- Whether a remote send would include commit summary only, paths, diff hunks, or
  all of the above.

Do not build the full harness as part of this spike. The follow-up build plan
should add the fixture and compare at least literal-summary grep, expanded
non-embedding ranking, and one local embedding prototype.

## Recommendation

Start with an offline, non-embedding ranking improvement before adding vector
embeddings. Expand the current literal search to score commit summaries, touched
paths, and bounded diff terms. That gives Lady better search immediately while
preserving deterministic offline behavior and avoiding a new privacy surface.

In parallel, create a local-only embedding prototype behind a feature flag or
test-only harness using Ollama-compatible embeddings. Use the evaluation fixture
above to prove it materially beats non-embedding ranking before adding UI or MCP
surface area.

Do not make remote embeddings a default. If remote embeddings are added later,
they must be opt-in per repo, require explicit remote-provider consent, disclose
that derived repository history is sent to the provider, and store derived index
data outside `settings.toml`.

Keep the current MCP `search_commits` behavior stable until the evaluation
shows a replacement is better and privacy UX has been reviewed.

## Open Questions

- Should semantic search index only commit summaries and paths first, or include
  bounded diffs from the start?
- Should MCP expose semantic search as a new tool name, leaving literal
  `search_commits` untouched for compatibility?
- What is the acceptable first-index latency for a large repo?
- Where should per-repo app-data cache invalidation be keyed: repository family
  id, HEAD oid, commit count, or a dedicated index metadata file?
- Is local embedding model setup acceptable product friction, or should the
  first shipped version avoid embeddings entirely?
