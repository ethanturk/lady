# Plan 016: Replace unwrap/expect with Proper Error Handling

**Status:** TODO  
**Priority:** P2  
**Effort:** M (2-3 days)  
**Risk:** Low  
**Created:** 2026-06-27  
**Commit:** 7ff3460

---

## Finding

**Category:** Correctness / Error Handling  
**Impact:** M  
**Evidence:** 22 `unwrap()/expect()` calls in production code across 7 files (not including tests)

| File | Count | Risk Level |
|------|-------|------------|
| `crates/lady-git/src/lib.rs` | 3 | Medium |
| `crates/lady-cli/src/lib.rs` | 4 | Low (infallible) |
| `crates/lady-ai/src/context.rs` | 3 | Low (initialization) |
| `crates/lady-license/src/lib.rs` | 4 | Medium |
| `src-tauri/src/ai.rs` | 3 | Low (mutex) |
| `src-tauri/src/watcher.rs` | 2 | Low (mutex) |
| `src-tauri/src/lib.rs` | 2 | Low (mutex) |

**Note:** The audit initially reported 659 unwrap/expect calls, but 637 of those are in test code (inside `#[cfg(test)]` modules). This plan focuses only on the 22 production instances.

---

## Why This Matters

Current issues:
1. **Crash risk**: 3 medium-risk `expect()` calls in `lady-git` could panic on valid user scenarios (mutex poisoning, git process failures)
2. **Error transparency**: `expect()` messages are often too vague for debugging ("mutex poisoned" doesn't help users report bugs)
3. **Inconsistent patterns**: Mix of `?`, `unwrap()`, and `expect()` makes code harder to review

This is NOT a high-severity finding - the codebase is actually well-written with most error handling already proper. This is a polish pass to eliminate the remaining panics.

---

## Scope

### In Scope
- Replace the 22 production `unwrap()/expect()` calls with proper error handling
- Use `?` operator where context already returns `Result`
- Use `map_err()` for better error context where needed
- Use `unwrap_or()` or `unwrap_or_default()` for truly infallible operations with a comment
- Add `#[track_caller]` to helper functions that propagate errors

### Out of Scope
- Test code unwrap/expect (637 instances - acceptable in tests)
- Refactoring existing error handling patterns beyond the identified unwrap/expect
- Adding new error types (use existing `Error` enums)
- Changing the behavior of infallible operations (e.g., `write!` to String)

---

## Current-State Code Excerpts

### 1. Mutex Poisoning (3 instances - lady-git)

**File:** `crates/lady-git/src/lib.rs:621`
```rust
fn repo(&self, id: &RepoId) -> Result<gix::Repository> {
    let guard = self
        .repos
        .lock()
        .expect("GixEngine repo registry mutex poisoned");  // ← PANIC HERE
    // ...
}
```

**File:** `crates/lady-git/src/lib.rs:1080`
```rust
self.repos
    .lock()
    .expect("GixEngine repo registry mutex poisoned")  // ← PANIC HERE
    .insert(id.clone(), repo.into_sync());
```

**File:** `src-tauri/src/watcher.rs:51`
```rust
let mut watchers = REPO_WATCHERS
    .lock()
    .expect("RepoWatchers mutex poisoned");  // ← PANIC HERE
```

**Problem:** Mutex poisoning is extremely rare in practice (requires panic during lock hold). However, if it happens, panicking crashes the app instead of returning a graceful error.

---

### 2. Git Process Failure (1 instance - lady-git)

**File:** `crates/lady-git/src/lib.rs:887`
```rust
child
    .stdin
    .take()
    .expect("git stdin was piped")  // ← PANIC HERE (should be infallible but still)
    .write_all(input)
```

**Problem:** This is technically infallible (we just created the child with `Stdio::piped()`), but `expect()` is still a panic point.

---

### 3. License Validation (4 instances - lady-license)

**File:** `crates/lady-license/src/lib.rs:164`
```rust
let payload_bytes = serde_json::to_vec(payload).expect("serialize payload");  // ← PANIC HERE
```

**File:** `crates/lady-license/src/lib.rs:197`
```rust
let got = verify(&lic, &pk, PRODUCT, 1_700_000_000).expect("verify");  // ← PANIC HERE
```

**File:** `crates/lady-license/src/lib.rs:206-209`
```rust
let dot = lic.find('.').unwrap();  // ← PANIC HERE
let tampered = String::from_utf8(bytes).unwrap();  // ← PANIC HERE
```

**Problem:** License validation should never panic - it should return an error to the user.

---

### 4. Infallible Operations (4 instances - lady-cli)

**File:** `crates/lady-cli/src/lib.rs:25-40`
```rust
writeln!(out, "Refs ({}):", refs.len()).expect("write to String is infallible");
// ... 3 more similar
```

**Problem:** These are correctly identified as infallible, but using `expect()` is still noisy. Better to use `unwrap()` with a comment or `expect_debug()` pattern.

---

### 5. Regex Compilation (3 instances - lady-ai)

**File:** `crates/lady-ai/src/context.rs:56, 78, 171`
```rust
let re = Regex::new(pattern).expect("valid regex");  // ← PANIC HERE
let bpe = BPE.get_or_init(|| tiktoken_rs::cl100k_base().expect("load cl100k_base"));  // ← PANIC HERE
```

**Problem:** These happen at initialization time (static init or lazy init). If they fail, the app can't function anyway, so panic is acceptable but should be clearer.

---

### 6. Mutex in Tauri (6 instances - src-tauri)

**File:** `src-tauri/src/ai.rs:38, 44, 194`
```rust
self.cancels.lock().expect("cancels lock")  // ← PANIC HERE (x3)
```

**File:** `src-tauri/src/lib.rs:2757, 2762, 2767`
```rust
Ok(self.0.lock().unwrap().get(key).cloned())  // ← PANIC HERE (x3)
```

**Problem:** Same as lady-git mutex poisoning - rare but avoidable.

---

## Implementation Steps

### Step 1: Define Error Handling Strategy (30 min)

**Decision:** For mutex poisoning, we have three options:

1. **Unwrap with better message**: `unwrap_or_else(|e| panic!("mutex poisoned: {e}"))` - still panics but clearer
2. **Return error**: Change function signature to `-> Result<T, MutexPoisonError>` - breaking change
3. **Recover silently**: `lock().unwrap_or_else(|e| e.into_inner())` - Rust's standard recovery pattern

**Selected approach:** Option 3 for mutex poisoning (recover silently), Option 2 for actual error cases (license, git).

---

### Step 2: Fix Mutex Poisoning (1 hour)

**Files:** `crates/lady-git/src/lib.rs`, `src-tauri/src/ai.rs`, `src-tauri/src/watcher.rs`, `src-tauri/src/lib.rs`

**Pattern:**
```rust
// Before:
let guard = self.repos.lock().expect("GixEngine repo registry mutex poisoned");

// After:
let guard = self.repos.lock().unwrap_or_else(|e| e.into_inner());
```

**Rationale:** Mutex poisoning is unrecoverable in practice (requires panic during critical section). If it happens, recovering and continuing is better than crashing.

**Verification:**
```sh
cargo clippy --all-targets --all-features -- -D warnings
cargo test -p lady-git
cargo test -p lady-tauri
```

---

### Step 3: Fix License Validation (1 hour)

**File:** `crates/lady-license/src/lib.rs`

**Changes:**

1. **Line 164** - Serialization:
```rust
// Before:
let payload_bytes = serde_json::to_vec(payload).expect("serialize payload");

// After:
let payload_bytes = serde_json::to_vec(payload)
    .map_err(|e| Error::License(format!("failed to serialize license: {e}")))?;
```

2. **Line 197** - Verification (already returns Result, just propagate):
```rust
// Before:
let got = verify(&lic, &pk, PRODUCT, 1_700_000_000).expect("verify");

// After:
let got = verify(&lic, &pk, PRODUCT, 1_700_000_000)?;
```

3. **Lines 206-209** - String operations (add validation):
```rust
// Before:
let dot = lic.find('.').unwrap();
let tampered = String::from_utf8(bytes).unwrap();

// After:
let dot = lic.find('.')
    .ok_or_else(|| Error::License("invalid license format: missing version separator".into()))?;
let tampered = String::from_utf8(bytes)
    .map_err(|_| Error::License("corrupted license data: invalid UTF-8".into()))?;
```

**Verification:**
```sh
cargo test -p lady-license
cargo clippy --all-targets --all-features -- -D warnings
```

---

### Step 4: Fix Git Process (30 min)

**File:** `crates/lady-git/src/lib.rs:887`

**Change:**
```rust
// Before:
child
    .stdin
    .take()
    .expect("git stdin was piped")
    .write_all(input)

// After:
match child.stdin.take() {
    Some(mut stdin) => stdin.write_all(input),
    None => Err(Error::Git("git process has no stdin".into())),
}
```

**Alternative (simpler):** Since this is truly infallible (we just created the child with piped stdin), just remove the expect:
```rust
child.stdin.take().unwrap().write_all(input)
// Add comment: // Infallible: stdin was piped above
```

**Verification:**
```sh
cargo test -p lady-git
cargo clippy --all-targets --all-features -- -D warnings
```

---

### Step 5: Fix Infallible Operations (30 min)

**File:** `crates/lady-cli/src/lib.rs`

**Change:**
```rust
// Before:
writeln!(out, "Refs ({}):", refs.len()).expect("write to String is infallible");

// After:
let _ = writeln!(out, "Refs ({}):", refs.len());
// or
writeln!(out, "Refs ({}):", refs.len()).expect("writing to String never fails");
```

**Rationale:** `writeln!` to a `String` is truly infallible. Either silence the warning with `let _ =` or keep `expect()` with clearer message.

**Decision:** Use `let _ =` pattern for truly infallible operations.

---

### Step 6: Fix Regex Compilation (30 min)

**File:** `crates/lady-ai/src/context.rs`

**Pattern:** These are initialization-time panics, which are acceptable but should be clearer.

```rust
// Before:
let re = Regex::new(pattern).expect("valid regex");

// After:
let re = Regex::new(pattern).expect("regex pattern must be valid (programming error if this panics)");
```

**Alternative:** Use `lazy_static!` or `once_cell!` with `expect()` at module load time (current approach is fine).

**Decision:** Keep as-is but improve message to indicate it's a programming error, not a runtime error.

---

### Step 7: Add Lint to Prevent Future Unwraps (30 min)

**File:** Add to `Cargo.toml` or create `.clippy.toml`:

```toml
# .clippy.toml
disallowed_methods = [
    { path = "std::result::Result::unwrap", reason = "use proper error handling with ? or map_err" },
    { path = "std::result::Result::expect", reason = "use ? operator with contextual error" },
]
```

**Exception:** Allow in test code via `#[cfg(test)]` gate.

**Alternative (less strict):** Add a clippy warning instead of hard error:
```toml
# clippy.toml
# No disallowed methods - just add documentation
```

**Decision:** Start with documentation only (update CONTRIBUTING.md), add clippy disallow later if needed.

---

### Step 8: Update Documentation (30 min)

**File:** `AGENTS.md` or `CONTRIBUTING.md`

Add error handling guidelines:

```markdown
## Error Handling Guidelines

- **Production code:** Never use `unwrap()` or `expect()` unless:
  - The operation is provably infallible (e.g., `writeln!` to String)
  - It's a static initialization that would prevent the app from starting
  - You add a comment explaining why it's safe

- **Preferred patterns:**
  - Propagate errors: `let x = some_fallible()?`
  - Add context: `some_fallible().map_err(|e| Error::Context(format!("{e}")))?`
  - Provide default: `some_fallible().unwrap_or_default()`
  - Handle explicitly: `match some_fallible() { Ok(x) => ..., Err(e) => ... }`

- **Mutex poisoning:** Use `lock().unwrap_or_else(|e| e.into_inner())` to recover
```

---

## Test Plan

### Unit Tests
No new tests needed - existing tests cover error paths.

### Verification Commands

```sh
# 1. No unwrap/expect in production code (except allowed cases)
grep -rn "\.unwrap()\|\.expect(" --include="*.rs" crates/ src-tauri/ | \
  grep -v "/tests/" | grep -v "#\[cfg(test)\]" | grep -v "let _ =" | \
  grep -v "unwrap_or_else.*into_inner" | grep -v "unwrap_or_default"

# Expected: Only infallible operations and mutex recovery patterns remain

# 2. All tests pass
cargo test -p lady-git
cargo test -p lady-license
cargo test -p lady-ai
cargo test

# 3. No clippy warnings
cargo clippy --all-targets --all-features -- -D warnings

# 4. Build check
cargo check --all-targets
```

---

## Done Criteria

- [ ] All 22 production `unwrap()/expect()` calls addressed
- [ ] Mutex poisoning cases use `unwrap_or_else(|e| e.into_inner())`
- [ ] License validation returns proper errors
- [ ] Git process error handling improved
- [ ] Infallible operations silenced with `let _ =` or documented
- [ ] Regex initialization messages improved
- [ ] `cargo clippy --all-targets --all-features -- -D warnings` passes
- [ ] `cargo test` passes (all crates)
- [ ] Documentation updated with error handling guidelines

---

## Risk Assessment

| Risk | Mitigation |
|------|------------|
| Breaking API changes | Only `lady-license` changes return different error types - check callers |
| Mutex recovery hides real bugs | Mutex poisoning is unrecoverable anyway - this is better than crash |
| Over-engineering simple cases | Keep changes minimal - only fix the 22 identified instances |

---

## Maintenance Notes

**Future work:**
- Consider adding `disallowed_methods` clippy lint in Phase 2
- Monitor error logs for new panic patterns
- Review PRs for new unwrap/expect usage

**Watch for:**
- New mutex usage patterns in refactored code
- License validation edge cases
- Git process failures in CI

---

## Dependencies

**Blocks:** None  
**Blocked by:** None  
**Related:** Plan 015 (test infrastructure), Plan 030 (e2e tests)

---

## Execution Log

*To be filled during implementation*

- [ ] Step 1: Strategy defined
- [ ] Step 2: Mutex poisoning fixed
- [ ] Step 3: License validation fixed
- [ ] Step 4: Git process fixed
- [ ] Step 5: Infallible ops silenced
- [ ] Step 6: Regex messages improved
- [ ] Step 7: Lint added (optional)
- [ ] Step 8: Documentation updated
- [ ] Verification: All gates pass
