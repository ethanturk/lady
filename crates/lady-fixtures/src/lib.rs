//! `lady-fixtures` — deterministic, offline synthetic fixtures for benchmarks
//! and tests (PH6-001).
//!
//! Everything here is seeded and reproducible: the same `seed` always yields
//! the same commits, text, and repository. Nothing touches the network, so
//! benches run identically on a developer machine and on a CI runner.
//!
//! Three generators, one per hot path benchmarked in Phase 6:
//! - [`synthetic_commits`] feeds the graph-layout bench (`lady-graph`).
//! - [`synthetic_text`] feeds the diff bench (`lady-diff`).
//! - [`build_synthetic_repo`] builds a real on-disk repo for the log-walk +
//!   status benches (`lady-git`), using the system `git` (ADR-0003 — Lady
//!   already requires system git).

use lady_proto::{CommitMeta, Oid, Signature};

/// A tiny seeded xorshift64 PRNG. Deterministic and dependency-free — we do not
/// pull `rand` into the fixture helper just to make benches reproducible.
#[derive(Clone)]
pub struct Rng(u64);

impl Rng {
    /// Create an RNG from a seed (0 is remapped to a non-zero state).
    pub fn new(seed: u64) -> Self {
        Rng(if seed == 0 {
            0x9E37_79B9_7F4A_7C15
        } else {
            seed
        })
    }

    /// Next pseudo-random `u64` (xorshift64).
    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    /// Uniform value in `0..n` (`n` must be non-zero).
    pub fn below(&mut self, n: u64) -> u64 {
        self.next_u64() % n
    }
}

/// Build a 40-hex-char OID deterministically from an index. Distinct indices
/// map to distinct OIDs (odd-constant multiply is a bijection mod 2^128).
fn oid_from(i: usize) -> Oid {
    let mixed = (i as u128 + 1).wrapping_mul(0x9E37_79B9_7F4A_7C15_F39C_C060_5CED_C835);
    // u128 is at most 32 hex digits; pad to a git-shaped 40.
    Oid::from(format!("{mixed:040x}"))
}

fn sig() -> Signature {
    Signature {
        name: "Synthetic Author".into(),
        email: "synthetic@lady.dev".into(),
    }
}

/// Generate `n` synthetic commits in topological order (newest first), the
/// shape `GixEngine::walk_log` produces and `lady_graph::layout` consumes.
///
/// The history is a linear backbone with seeded periodic two-parent merges, so
/// the layout engine exercises lane allocation, fan-out, and fan-in — not just
/// a trivial straight line. Parents always reference older (higher-index)
/// commits, so the DAG is always valid.
pub fn synthetic_commits(n: usize, seed: u64) -> Vec<CommitMeta> {
    let mut rng = Rng::new(seed);
    let mut commits = Vec::with_capacity(n);
    for i in 0..n {
        let mut parents = Vec::new();
        if i + 1 < n {
            parents.push(oid_from(i + 1));
            // ~1 in 8 commits is a merge with a second, slightly older parent.
            if rng.below(8) == 0 {
                let offset = 2 + rng.below(4) as usize;
                if i + offset < n {
                    parents.push(oid_from(i + offset));
                }
            }
        }
        commits.push(CommitMeta {
            oid: oid_from(i),
            parents,
            author: sig(),
            committer: sig(),
            summary: format!("synthetic commit {i}"),
            time: 1_700_000_000 - (i as i64) * 60,
        });
    }
    commits
}

/// Generate a deterministic (old, new) text pair of roughly `lines` lines, with
/// a seeded mix of modified lines plus one inserted and one deleted block — a
/// realistic-ish change set for the diff bench.
pub fn synthetic_text(lines: usize, seed: u64) -> (String, String) {
    let mut rng = Rng::new(seed);
    let base: Vec<String> = (0..lines)
        .map(|i| format!("line {i:06}: lorem ipsum dolor sit amet consectetur"))
        .collect();

    let mut new_lines: Vec<String> = Vec::with_capacity(lines + 64);
    for (i, line) in base.iter().enumerate() {
        // Delete a contiguous block once, near 1/3 in.
        if i == lines / 3 {
            continue;
        }
        // Insert a block once, near 2/3 in.
        if i == (2 * lines) / 3 {
            for k in 0..16 {
                new_lines.push(format!("inserted line {k} (seed {seed})"));
            }
        }
        // Modify ~10% of lines.
        if rng.below(10) == 0 {
            new_lines.push(format!("{line} [edited {}]", rng.next_u64() & 0xffff));
        } else {
            new_lines.push(line.clone());
        }
    }

    let mut old = base.join("\n");
    old.push('\n');
    let mut new = new_lines.join("\n");
    new.push('\n');
    (old, new)
}

/// Outcome of [`build_synthetic_repo`].
pub struct SyntheticRepo {
    /// The temp directory holding the repo (kept alive for the repo's lifetime).
    pub dir: tempfile::TempDir,
}

impl SyntheticRepo {
    /// Path to the repository working directory.
    pub fn path(&self) -> &std::path::Path {
        self.dir.path()
    }
}

/// Build a real on-disk git repository with `commits` commits, deterministically,
/// using the system `git`. Leaves the working tree dirty (one modified tracked
/// file + one untracked file) so the `status` bench has work to measure.
///
/// Offline and reproducible: fixed author/committer identity and dates, no
/// remotes, no network. Returns `None` if `git` is unavailable so benches can
/// skip gracefully on a machine without it.
pub fn build_synthetic_repo(commits: usize, seed: u64) -> Option<SyntheticRepo> {
    use std::process::Command;

    let dir = tempfile::tempdir().ok()?;
    let path = dir.path().to_path_buf();
    let mut rng = Rng::new(seed);

    let git = |args: &[&str]| -> Option<()> {
        let status = Command::new("git")
            .args(args)
            .current_dir(&path)
            .env("GIT_AUTHOR_NAME", "Synthetic")
            .env("GIT_AUTHOR_EMAIL", "synthetic@lady.dev")
            .env("GIT_COMMITTER_NAME", "Synthetic")
            .env("GIT_COMMITTER_EMAIL", "synthetic@lady.dev")
            .env("GIT_AUTHOR_DATE", "2023-01-01T00:00:00 +0000")
            .env("GIT_COMMITTER_DATE", "2023-01-01T00:00:00 +0000")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .ok()?;
        status.success().then_some(())
    };

    git(&["init", "-q", "-b", "main"])?;
    git(&["config", "user.email", "synthetic@lady.dev"])?;
    git(&["config", "user.name", "Synthetic"])?;
    git(&["config", "commit.gpgsign", "false"])?;

    for i in 0..commits {
        let fname = format!("file_{}.txt", i % 8);
        let fpath = path.join(&fname);
        let prev = std::fs::read_to_string(&fpath).unwrap_or_default();
        let line = format!("{prev}commit {i} rev {}\n", rng.next_u64());
        std::fs::write(&fpath, line).ok()?;
        git(&["add", "-A"])?;
        git(&["commit", "-q", "-m", &format!("synthetic commit {i}")])?;
    }

    // Verify the full history landed — partial builds must not reach benches.
    let count = Command::new("git")
        .args(["rev-list", "--count", "HEAD"])
        .current_dir(&path)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string());
    if count.as_deref() != Some(&commits.to_string()) {
        return None;
    }

    // Leave the tree dirty for the status bench: one modified tracked file and
    // one untracked file.
    if commits > 0 {
        let tracked = path.join("file_0.txt");
        let prev = std::fs::read_to_string(&tracked).unwrap_or_default();
        std::fs::write(&tracked, format!("{prev}uncommitted change\n")).ok()?;
    }
    std::fs::write(path.join("untracked.txt"), "untracked\n").ok()?;

    Some(SyntheticRepo { dir })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commits_are_deterministic_and_valid() {
        let a = synthetic_commits(500, 42);
        let b = synthetic_commits(500, 42);
        assert_eq!(a, b, "same seed → same commits");
        assert_eq!(a.len(), 500);
        // Newest first; last commit is a root.
        assert!(a.last().unwrap().parents.is_empty());
        // Every parent references a strictly-older (later-indexed) commit, so
        // the DAG is acyclic — verify via the OID set.
        let ids: std::collections::HashSet<_> = a.iter().map(|c| c.oid.clone()).collect();
        for c in &a {
            for p in &c.parents {
                assert!(ids.contains(p));
            }
        }
        assert!(a.iter().any(|c| c.parents.len() > 1), "has some merges");
    }

    #[test]
    fn text_is_deterministic_and_differs() {
        let (o1, n1) = synthetic_text(1000, 7);
        let (o2, n2) = synthetic_text(1000, 7);
        assert_eq!((&o1, &n1), (&o2, &n2));
        assert_ne!(o1, n1, "old and new must differ");
    }

    #[test]
    fn repo_builds_when_git_present() {
        // Small repo so the test stays fast; skips cleanly without git.
        if let Some(repo) = build_synthetic_repo(12, 1) {
            assert!(repo.path().join(".git").exists());
            assert!(repo.path().join("untracked.txt").exists());
        }
    }
}
